use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tempfile::TempDir;

const METADATA: &str = r#"{
    "version": 1,
    "about": "Example's output [format]",
    "options": [
        {"long": "verbose", "short": "v", "help": "Show more detail"},
        {"long": "format", "value": {"choices": ["json", "text"]}},
        {"long": "output", "value": {"hint": "file"}},
        {"long": "directory", "value": {"hint": "directory"}}
    ],
    "subcommands": [{"name": "vm", "subcommands": [{
        "name": "start",
        "options": [
            {"long": "boot-disk", "value": {"choices": ["nvme", "usb"]}},
            {"long": "flash-net", "value": {"hint": "file", "optional": true}}
        ]
    }]}]
}"#;

fn plugin(dir: &Path, name: &str) {
    let path = dir.join(format!("teka-{name}"));
    fs::write(&path, "#!/bin/sh\necho executed > plugin-ran\nexit 99\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn completion(dir: &Path, path: &std::ffi::OsStr, shell: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_teka"))
        .current_dir(dir)
        .env("PATH", path)
        .env_remove("TEKA_HOST")
        .args(["completion", shell])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output);
    assert!(!dir.join("plugin-ran").exists());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn composes_plugins_without_changing_dispatch() {
    let dir = TempDir::new().unwrap();
    for name in [
        "example",
        "plain",
        "example-tool",
        "app",
        "help",
        "completion",
        "not-executable",
    ] {
        plugin(dir.path(), name);
    }
    fs::set_permissions(
        dir.path().join("teka-not-executable"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    // Shell metacharacters in executable names must not become shell code.
    plugin(dir.path(), "bad$(touch injected)");
    plugin(dir.path(), "bad__subcmd__name");
    fs::write(dir.path().join("teka-example.completion.json"), METADATA).unwrap();
    for shell in ["bash", "fish", "zsh"] {
        let script = completion(dir.path(), dir.path().as_os_str(), shell);
        for word in [
            "example", "plain", "verbose", "format", "json", "text", "aarch64", "arm64",
        ] {
            assert!(script.contains(word), "missing {word} in {shell}");
        }
        assert!(!script.contains("not-executable"));
        assert!(!script.contains("touch injected"));
        assert!(!script.contains("bad__subcmd__name"));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_teka"))
        .current_dir(dir.path())
        .env("PATH", dir.path())
        .args(["example", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(99));
    assert!(dir.path().join("plugin-ran").exists());
}

#[test]
fn invalid_metadata_falls_back_to_name_completion() {
    let dir = TempDir::new().unwrap();
    plugin(dir.path(), "example");
    let path = dir.path().join("teka-example.completion.json");
    for shell in ["bash", "fish", "zsh"] {
        let baseline = completion(dir.path(), dir.path().as_os_str(), shell);
        for metadata in [
            "not JSON".to_owned(),
            r#"{"version":2}"#.to_owned(),
            r#"{"version":1,"options":[{}]}"#.to_owned(),
            r#"{"version":1,"options":[{"long":"help"}]}"#.to_owned(),
            r#"{"version":1,"options":[{"short":"h"}]}"#.to_owned(),
            r#"{"version":1,"options":[{"long":"x"},{"long":"x"}]}"#.to_owned(),
            r#"{"version":1,"options":[{"short":"v"},{"short":"v"}]}"#.to_owned(),
            r#"{"version":1,"options":[{"long":"$(touch injected)"}]}"#.to_owned(),
            r#"{"version":1,"options":[{"long":"x","value":{"choices":["$(touch injected)"]}}]}"#
                .to_owned(),
            r#"{"version":1,"options":[{"long":"x","value":{"choices":[]}}]}"#.to_owned(),
            r#"{"version":1,"options":[{"long":"x","value":{"choices":["a"],"hint":"file"}}]}"#
                .to_owned(),
            r#"{"version":1,"options":[{"long":"x","value":{"hint":"unknown"}}]}"#.to_owned(),
            r#"{"version":1,"unknown":true}"#.to_owned(),
            r#"{"version":1,"subcommands":[{"name":"help"}]}"#.to_owned(),
            r#"{"version":1,"subcommands":[{"name":"vm"},{"name":"vm"}]}"#.to_owned(),
            r#"{"version":1,"subcommands":[{"name":"vm","subcommands":[{"name":"bad__subcmd__name"}]}]}"#.to_owned(),
            " ".repeat(65_537),
        ] {
            fs::write(&path, metadata).unwrap();
            assert_eq!(
                completion(dir.path(), dir.path().as_os_str(), shell),
                baseline
            );
        }
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(
            completion(dir.path(), dir.path().as_os_str(), shell),
            baseline
        );
        fs::remove_dir(&path).unwrap();
    }
}

#[test]
fn metadata_follows_the_first_executable_on_path() {
    let dir = TempDir::new().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    plugin(&first, "example");
    plugin(&second, "example");
    fs::write(second.join("teka-example.completion.json"), METADATA).unwrap();
    let path = std::env::join_paths([&first, &second, &first]).unwrap();
    assert!(!completion(dir.path(), &path, "bash").contains("--verbose"));
    fs::write(first.join("teka-example.completion.json"), METADATA).unwrap();
    assert!(completion(dir.path(), &path, "bash").contains("--verbose"));
    fs::remove_file(first.join("teka-example")).unwrap();
    assert!(completion(dir.path(), &path, "bash").contains("--verbose"));
}

#[test]
fn detects_shell_and_preserves_explicit_override() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    for (shell, marker) in [
        ("bash", "_teka()"),
        ("fish", "complete -c teka -e"),
        ("zsh", "#compdef teka"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_teka"))
            .env("PATH", "")
            .env("SHELL", format!("/bin/{shell}"))
            .arg("completion")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains(marker));

        match Command::new(shell)
            .args([
                "-c",
                "printf x > \"$HOME/.teka-home-write-test\" && \"$TEKA_TEST_BIN\" completion; exit",
            ])
            .env("TEKA_TEST_BIN", env!("CARGO_BIN_EXE_teka"))
            .env("HOME", home.path())
            .env("SHELL", "/bin/unsupported")
            .current_dir(dir.path())
            .output()
        {
            Ok(output) => {
                assert!(output.status.success(), "{shell}: {output:?}");
                assert!(output.stderr.is_empty(), "{shell}: {output:?}");
                assert!(String::from_utf8_lossy(&output.stdout).contains(marker));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("{shell} unavailable; shell checks run in the Nix build");
            }
            Err(error) => panic!("{shell}: {error}"),
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_teka"))
        .env("PATH", "")
        .env("SHELL", "/bin/fish")
        .args(["completion", "bash"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("_teka()"));
    for shell in [None, Some("/bin/unsupported")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_teka"));
        command.env("PATH", "").env_remove("SHELL");
        if let Some(shell) = shell {
            command.env("SHELL", shell);
        }
        let output = command.arg("completion").output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot detect shell"));
    }
}

#[test]
fn shells_load_and_refresh_snapshots() {
    let dir = TempDir::new().unwrap();
    plugin(dir.path(), "example");
    fs::write(dir.path().join("teka-example.completion.json"), METADATA).unwrap();
    for shell in ["bash", "fish", "zsh"] {
        let script = completion(dir.path(), dir.path().as_os_str(), shell);
        fs::write(dir.path().join(format!("before.{shell}")), script).unwrap();
    }
    fs::remove_file(dir.path().join("teka-example")).unwrap();
    for shell in ["bash", "fish", "zsh"] {
        let script = completion(dir.path(), dir.path().as_os_str(), shell);
        fs::write(dir.path().join(format!("after.{shell}")), script).unwrap();
        let check = match shell {
            "bash" => {
                r#"
source before.bash
COMP_WORDS=(teka example --format j); COMP_CWORD=3
_teka teka j --format
[[ ${COMPREPLY[*]} == json ]] || exit 1
COMP_WORDS=(teka example vm start --boot-disk n); COMP_CWORD=5
_teka teka n --boot-disk
[[ ${COMPREPLY[*]} == nvme ]] || exit 1
source after.bash
COMP_WORDS=(teka ex); COMP_CWORD=1
_teka teka ex teka
[[ ${#COMPREPLY[@]} == 0 ]]
"#
            }
            "fish" => {
                r#"
source before.fish
test (complete -C 'teka example --format j') = json; or exit 1
test (complete -C 'teka example vm start --boot-disk n') = nvme; or exit 1
source after.fish
test -z (complete -C 'teka ex' | string collect)
"#
            }
            "zsh" => {
                r#"
autoload -Uz compinit
compinit -D
source before.zsh
[[ $_comps[teka] == _teka ]] || exit 1
_describe() { print -rl -- "${commands[@]}"; }
[[ $(_teka_commands) == *example* ]] || exit 1
source after.zsh
[[ $(_teka_commands) != *example* ]]
"#
            }
            _ => unreachable!(),
        };
        match Command::new(shell)
            .args(["-c", check])
            .current_dir(dir.path())
            .output()
        {
            Ok(output) => assert!(output.status.success(), "{shell}: {output:?}"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("{shell} unavailable; shell checks run in the Nix build");
            }
            Err(error) => panic!("{shell}: {error}"),
        }
    }
}

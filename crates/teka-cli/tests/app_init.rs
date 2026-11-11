use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

const SDK_REV: &str = "0123456789012345678901234567890123456789";
const SDK_REFERENCE: &str =
    "git+file:///mock-sdk?ref=HEAD&rev=0123456789012345678901234567890123456789";

const LOCK: &str = "{\"nodes\":{\"root\":{\"inputs\":{}}},\"root\":\"root\",\"version\":7}\n";

const FILES: [&str; 8] = [
    ".gitignore",
    "AGENTS.md",
    "README.md",
    "default.nix",
    "flake.lock",
    "flake.nix",
    "hello-world.rs",
    "hello-world.service",
];

fn example_sdk(command: &mut Command) -> TempDir {
    let sdk = TempDir::new().unwrap();
    let source = sdk.path().join("source");
    let bin = sdk.path().join("bin");
    git(sdk.path(), &["init"]);
    fs::create_dir(&source).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(sdk.path().join("flake.nix"), "{}").unwrap();
    for (path, contents) in [
        (".gitignore", "result/\nresult-*/\n"),
        ("README.md", "Preserve @APP_NAME@ verbatim.\n"),
        ("flake.nix", "{ inputs.teka-sdk.url = \"github:ouster-lidar/teka-sdk/main\"; outputs = _: { teka.apps.hello-world = {}; }; }\n"),
        ("default.nix", "{}\n"),
        (
            "hello-world.rs",
            "fn main() { println!(\"hello world\"); }\n",
        ),
        ("hello-world.service", "[Service]\nExecStart=@program@\n"),
    ] {
        fs::write(source.join(path), contents).unwrap();
    }
    for (name, contents) in [
        (
            "nix",
            r#"#!/bin/sh
if [ -n "$TEKA_TEST_NIX_ARGS" ]; then
  printf '%s\n' "$@" >> "$TEKA_TEST_NIX_ARGS"
fi
if [ "$1 $2" = "flake metadata" ]; then
  printf '%s\n' "$@" > "$TEKA_TEST_METADATA_ARGS"
  printf '%s' "$TEKA_TEST_SDK_METADATA"
elif [ "$1 $2" = "flake new" ]; then
  printf '%s\n' "$@" > "$TEKA_TEST_INIT_ARGS"
  cd -- "$6" || exit 1
  if [ -n "$TEKA_TEST_INIT_FAIL" ]; then
    cp "$TEKA_TEST_EXAMPLE_SOURCE/default.nix" .
    echo 'refusing to overwrite existing file flake.nix' >&2
    exit 1
  fi
  cp -R "$TEKA_TEST_EXAMPLE_SOURCE/." .
  echo 'wrote: "default.nix"' >&2
elif [ "$1 $2" = "flake lock" ]; then
  printf '%s\n' "$@" > "$TEKA_TEST_LOCK_ARGS"
  if [ -n "$TEKA_TEST_LOCK_FAIL" ]; then
    echo 'mock lock failure' >&2
    exit 1
  fi
  [ -n "$TEKA_TEST_NO_LOCK" ] || printf '%s' "$TEKA_TEST_LOCK" > flake.lock
else
  printf '%s' "$TEKA_TEST_CATALOG"
fi
"#,
        ),
        ("teka", "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$*\"\n"),
    ] {
        let path = bin.join(name);
        fs::write(&path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::join_paths(
        [bin]
            .into_iter()
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    command
        .env("PATH", path)
        .env("TEKA_APPS_FLAKE_ROOT", sdk.path())
        .env("TEKA_TEST_EXAMPLE_SOURCE", source)
        .env("TEKA_TEST_INIT_ARGS", sdk.path().join("init-args"))
        .env(
            "TEKA_TEST_SDK_METADATA",
            serde_json::json!({ "revision": SDK_REV, "url": SDK_REFERENCE }).to_string(),
        )
        .env("TEKA_TEST_CATALOG", r#"[false,["default","hello-world"]]"#)
        .env("TEKA_TEST_METADATA_ARGS", sdk.path().join("metadata-args"))
        .env("TEKA_TEST_LOCK", LOCK)
        .env("TEKA_TEST_LOCK_ARGS", sdk.path().join("lock-args"));
    sdk
}

fn init(cwd: &Path, args: &[&str]) -> (Command, TempDir) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_teka"));
    command.current_dir(cwd).args(["app", "init"]).args(args);
    let sdk = example_sdk(&mut command);
    (command, sdk)
}

fn success(output: Output) -> String {
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    assert!(output.stdout.is_empty());
    stderr
}

fn failure(output: Output, message: &str) -> String {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("✗ App:"), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
    assert!(
        !stderr.contains("⚒\u{fe0e} Build:") && !stderr.contains("✈\u{fe0e} Deploy:"),
        "{stderr}"
    );
    stderr
}

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn copied_binary_initializes_selected_example_outside_checkout() {
    let temp = TempDir::new().unwrap();
    let binary = temp.path().join("teka");
    fs::copy(env!("CARGO_BIN_EXE_teka"), &binary).unwrap();
    let mut command = Command::new(&binary);
    let directory = "nested/not an app's ID \\\\ $x; (true)";
    command
        .current_dir(temp.path())
        .env("HOME", temp.path().canonicalize().unwrap())
        .args(["app", "init", "--example", "hello-world", directory]);
    let sdk = example_sdk(&mut command);
    let calls = sdk.path().join("nix-args");
    command
        .env("TEKA_TEST_NIX_ARGS", &calls)
        .env("TEKA_TEST_CATALOG", r#"[true,["default","hello-world"]]"#);
    let revision = "github:ouster-lidar/teka-sdk/0123456789012345678901234567890123456789";
    command.env(
        "TEKA_TEST_SDK_METADATA",
        serde_json::json!({ "url": revision }).to_string(),
    );
    let stderr = success(command.output().unwrap());
    assert!(
        stderr.starts_with("⏳\u{fe0e} Initializing example\n"),
        "{stderr}"
    );
    assert!(stderr.contains("✓ App: hello-world"), "{stderr}");
    assert!(stderr.contains("wrote: \"default.nix\""), "{stderr}");
    let app = temp.path().join(directory).canonicalize().unwrap();
    let build = stderr
        .lines()
        .find_map(|line| line.strip_prefix("⚒\u{fe0e} Build:  "))
        .unwrap();
    let deploy = stderr
        .lines()
        .find_map(|line| line.strip_prefix("✈\u{fe0e} Deploy: "))
        .unwrap();
    assert!(stderr.find("✓ App:").unwrap() < stderr.find("⚒\u{fe0e} Build:").unwrap());
    for shell in ["sh", "fish"] {
        let output = Command::new(shell)
            .current_dir(temp.path())
            .envs(
                command
                    .get_envs()
                    .filter_map(|(key, value)| value.map(|value| (key, value))),
            )
            .arg("-c")
            .arg(format!("{build}\n{deploy}"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "{}\napp build\n{}\napp deploy --build\n",
                app.display(),
                app.display()
            ),
            "{shell}"
        );
    }
    assert!(app.join(".git").is_dir());
    assert_eq!(git(&app, &["ls-files"]), FILES.join("\n") + "\n");
    assert_eq!(git(&app, &["rev-list", "--all", "--count"]), "0\n");
    assert_eq!(fs::read(app.join("flake.lock")).unwrap(), LOCK.as_bytes());
    assert_eq!(
        fs::read_to_string(sdk.path().join("init-args")).unwrap(),
        format!("flake\nnew\n--template\n{revision}#hello-world\n--\n{directory}\n")
    );
    let calls = fs::read_to_string(calls).unwrap();
    assert!(calls.contains("--inputs-from\n"), "{calls}");
    assert_eq!(
        fs::read_to_string(sdk.path().join("lock-args")).unwrap(),
        format!("flake\nlock\npath:.\n--override-input\nteka-sdk\n{revision}\n--output-lock-file\nflake.lock\n")
    );
    let guidance = fs::read_to_string(app.join("AGENTS.md")).unwrap();
    assert!(guidance.contains("README.md") && guidance.contains("flake.nix"));
    for path in FILES
        .iter()
        .filter(|path| !matches!(**path, "flake.lock" | "AGENTS.md"))
    {
        assert_eq!(
            fs::read(app.join(path)).unwrap(),
            fs::read(sdk.path().join("source").join(path)).unwrap()
        );
    }
}

#[test]
fn local_sdk_resolves_committed_head_and_locks_its_reference() {
    let temp = TempDir::new().unwrap();
    let (mut command, sdk) = init(temp.path(), &["new-app"]);
    success(command.output().unwrap());
    assert_eq!(
        fs::read_to_string(sdk.path().join("metadata-args")).unwrap(),
        format!(
            "flake\nmetadata\n--json\n--no-write-lock-file\ngit+file://{}?dir=&ref=HEAD\n",
            sdk.path().canonicalize().unwrap().display()
        )
    );
    assert_eq!(
        fs::read_to_string(sdk.path().join("lock-args")).unwrap(),
        format!("flake\nlock\npath:.\n--override-input\nteka-sdk\n{SDK_REFERENCE}\n--output-lock-file\nflake.lock\n")
    );
    assert_eq!(
        fs::read(temp.path().join("new-app/flake.nix")).unwrap(),
        fs::read(sdk.path().join("source/flake.nix")).unwrap()
    );
}

#[test]
fn rejects_sdk_sources_without_a_committed_revision() {
    let temp = TempDir::new().unwrap();
    let (mut command, sdk) = init(temp.path(), &["new-app"]);
    command.env(
        "TEKA_TEST_SDK_METADATA",
        serde_json::json!({ "url": SDK_REFERENCE }).to_string(),
    );
    failure(command.output().unwrap(), "no committed revision");
    assert!(!temp.path().join("new-app").exists());
    assert!(!sdk.path().join("init-args").exists());
}

#[test]
fn outside_checkout_keeps_the_remote_sdk_reference() {
    let temp = TempDir::new().unwrap();
    let (mut command, sdk) = init(temp.path(), &["new-app"]);
    let Some(reference) = option_env!("TEKA_SDK_REF").filter(|reference| !reference.is_empty())
    else {
        command.env_remove("TEKA_APPS_FLAKE_ROOT");
        failure(command.output().unwrap(), "this build has no pinned SDK");
        return;
    };
    let calls = sdk.path().join("nix-args");
    command
        .env_remove("TEKA_APPS_FLAKE_ROOT")
        .env("TEKA_TEST_NIX_ARGS", &calls);
    success(command.output().unwrap());
    let calls = fs::read_to_string(calls).unwrap();
    assert!(!calls.contains("flake\nmetadata\n"), "{calls}");
    assert_eq!(
        fs::read_to_string(sdk.path().join("init-args")).unwrap(),
        format!("flake\nnew\n--template\n{reference}\n--\nnew-app\n")
    );
    assert_eq!(
        fs::read_to_string(sdk.path().join("lock-args")).unwrap(),
        format!("flake\nlock\npath:.\n--override-input\nteka-sdk\n{reference}\n--output-lock-file\nflake.lock\n")
    );
}

#[test]
fn native_default_and_named_templates_do_not_require_hello_world() {
    for (name, catalog, directory) in [
        (None, r#"[false,["default"]]"#, "new-app"),
        (Some("default"), r#"[false,["default"]]"#, "new-app"),
        (Some("custom"), r#"[false,["custom"]]"#, "new-app"),
        (None, r#"[false,["default"]]"#, "-app"),
    ] {
        let temp = TempDir::new().unwrap();
        let (mut command, sdk) = init(temp.path(), &[]);
        let calls = sdk.path().join("nix-args");
        command
            .env("TEKA_TEST_CATALOG", catalog)
            .env("TEKA_TEST_NIX_ARGS", &calls);
        if let Some(name) = name {
            command.args(["--example", name]);
        }
        command.args(["--", directory]);
        success(command.output().unwrap());
        let args = fs::read_to_string(sdk.path().join("init-args")).unwrap();
        let args = args.lines().collect::<Vec<_>>();
        assert_eq!(&args[..3], ["flake", "new", "--template"]);
        assert_eq!(&args[4..], ["--", directory]);
        if let Some(name) = name {
            assert!(args[3].ends_with(&format!("#{name}")), "{args:?}");
        } else {
            assert!(!args[3].contains('#'), "{args:?}");
        }
        assert_eq!(
            fs::read_to_string(sdk.path().join("lock-args")).unwrap(),
            format!("flake\nlock\npath:.\n--override-input\nteka-sdk\n{}\n--output-lock-file\nflake.lock\n", SDK_REFERENCE)
        );
        assert_eq!(
            fs::read(temp.path().join(directory).join("flake.nix")).unwrap(),
            fs::read(sdk.path().join("source/flake.nix")).unwrap()
        );
        let calls = fs::read_to_string(calls).unwrap();
        assert!(!calls.contains("--inputs-from"), "{calls}");
        assert!(calls.contains("--override-input"), "{calls}");
    }
}

#[test]
fn missing_default_is_rejected_before_creating_files() {
    let temp = TempDir::new().unwrap();
    let (mut command, sdk) = init(temp.path(), &["new-app"]);
    command.env("TEKA_TEST_CATALOG", r#"[false,["hello-world"]]"#);
    failure(command.output().unwrap(), "unknown");
    assert!(!temp.path().join("new-app").exists());
    assert!(!sdk.path().join("init-args").exists());
}

#[test]
fn defaults_to_current_directory_and_preserves_unrelated_work() {
    for args in [&[][..], &["."][..]] {
        let temp = TempDir::new().unwrap();
        let app = temp.path().join("existing-app");
        fs::create_dir(&app).unwrap();
        git(&app, &["init"]);
        fs::write(app.join("notes"), "staged\n").unwrap();
        fs::write(app.join("AGENTS.md"), "staged guidance\n").unwrap();
        git(&app, &["add", "notes", "AGENTS.md"]);
        let index = git(&app, &["ls-files", "--stage", "notes", "AGENTS.md"]);
        fs::write(app.join("notes"), "unstaged\n").unwrap();
        fs::write(app.join("AGENTS.md"), "project guidance\n").unwrap();
        fs::write(app.join("untracked"), "keep\n").unwrap();
        success(init(&app, args).0.output().unwrap());
        assert_eq!(
            git(&app, &["ls-files", "--stage", "notes", "AGENTS.md"]),
            index
        );
        assert_eq!(fs::read_to_string(app.join("notes")).unwrap(), "unstaged\n");
        assert_eq!(
            fs::read_to_string(app.join("AGENTS.md")).unwrap(),
            "project guidance\n"
        );
        assert_eq!(fs::read_to_string(app.join("untracked")).unwrap(), "keep\n");
        assert!(git(&app, &["status", "--porcelain"]).contains("?? untracked"));
        assert!(fs::read_to_string(app.join("flake.nix"))
            .unwrap()
            .contains("hello-world"));
    }
}

#[test]
fn reuses_enclosing_repository() {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init"]);
    fs::write(repo.path().join("unrelated"), "keep").unwrap();
    git(repo.path(), &["add", "unrelated"]);
    success(init(repo.path(), &["apps/child-app"]).0.output().unwrap());
    let app = repo.path().join("apps/child-app");
    assert!(!app.join(".git").exists());
    let expected = FILES
        .iter()
        .map(|path| format!("apps/child-app/{path}\n"))
        .collect::<String>()
        + "unrelated\n";
    assert_eq!(git(repo.path(), &["ls-files"]), expected);
    let index = git(repo.path(), &["ls-files", "--stage"]);
    failure(
        init(&app, &[]).0.output().unwrap(),
        "flake.lock already exists",
    );
    assert_eq!(git(repo.path(), &["ls-files", "--stage"]), index);
}

#[test]
fn refuses_existing_lock_without_updating_or_staging_it() {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init"]);
    fs::write(repo.path().join("flake.lock"), "staged\n").unwrap();
    git(repo.path(), &["add", "flake.lock"]);
    let index = git(repo.path(), &["ls-files", "--stage"]);
    fs::write(repo.path().join("flake.lock"), "unstaged\n").unwrap();
    let (mut command, sdk) = init(repo.path(), &[]);
    failure(command.output().unwrap(), "flake.lock already exists");
    assert_eq!(
        fs::read_to_string(repo.path().join("flake.lock")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(git(repo.path(), &["ls-files", "--stage"]), index);
    assert!(!sdk.path().join("init-args").exists());
    assert!(!sdk.path().join("lock-args").exists());
}

#[test]
fn input_free_templates_do_not_require_a_lockfile() {
    let temp = TempDir::new().unwrap();
    let (mut command, sdk) = init(temp.path(), &["new-app"]);
    command.env("TEKA_TEST_NO_LOCK", "1");
    fs::write(
        sdk.path().join("source/flake.nix"),
        "{ outputs = _: {}; }\n",
    )
    .unwrap();
    success(command.output().unwrap());
    let app = temp.path().join("new-app");
    assert!(!app.join("flake.lock").exists());
    let mut files = FILES.to_vec();
    files.retain(|path| *path != "flake.lock");
    assert_eq!(git(&app, &["ls-files"]), files.join("\n") + "\n");
}

#[test]
fn template_conflict_stages_sources_for_retry_in_enclosing_repository() {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init"]);
    let temp = repo.path().join("apps/new-app");
    fs::create_dir_all(&temp).unwrap();
    fs::write(temp.join("flake.nix"), "keep\n").unwrap();
    let (mut command, sdk) = init(&temp, &[]);
    command.env("TEKA_TEST_INIT_FAIL", "1");
    failure(
        command.output().unwrap(),
        "refusing to overwrite existing file",
    );
    assert_eq!(
        fs::read_to_string(temp.join("flake.nix")).unwrap(),
        "keep\n"
    );
    assert!(!sdk.path().join("lock-args").exists());
    assert!(!temp.join("flake.lock").exists());
    assert_eq!(git(&temp, &["ls-files"]), "default.nix\n");

    fs::remove_file(temp.join("flake.nix")).unwrap();
    command.env_remove("TEKA_TEST_INIT_FAIL");
    success(command.output().unwrap());
    assert_eq!(git(&temp, &["ls-files"]), FILES.join("\n") + "\n");
}

#[test]
fn rejects_unknown_examples_without_creating_files() {
    let temp = TempDir::new().unwrap();
    for name in ["hello-http", "missing-example"] {
        failure(
            init(temp.path(), &["--example", name, "new-app"])
                .0
                .output()
                .unwrap(),
            "unknown example",
        );
        assert!(!temp.path().join("new-app").exists());
    }
}

#[test]
fn git_add_failure_preserves_unrelated_index_entries() {
    for ignored in ["flake.lock", "hello-world.rs"] {
        let repo = TempDir::new().unwrap();
        git(repo.path(), &["init"]);
        fs::write(repo.path().join("notes"), "staged").unwrap();
        git(repo.path(), &["add", "notes"]);
        let index = git(repo.path(), &["ls-files", "--stage", "notes"]);
        fs::write(
            repo.path().join(".git/info/exclude"),
            format!("{ignored}\n"),
        )
        .unwrap();
        let stderr = failure(
            init(repo.path(), &["my-app"]).0.output().unwrap(),
            "ignored",
        );
        assert!(!stderr.contains("teka: error:"), "{stderr}");
        assert_eq!(git(repo.path(), &["ls-files", "--stage", "notes"]), index);
        assert!(!git(repo.path(), &["ls-files"]).contains(ignored));
        assert_eq!(
            repo.path().join("my-app/flake.lock").is_file(),
            ignored == "flake.lock"
        );
    }
}

#[test]
fn lock_failure_stages_sources_for_retry_in_enclosing_repository() {
    let temp = TempDir::new().unwrap();
    git(temp.path(), &["init"]);
    let (mut command, _sdk) = init(temp.path(), &["apps/new-app"]);
    command.env("TEKA_TEST_LOCK_FAIL", "1");
    failure(command.output().unwrap(), "mock lock failure");
    let app = temp.path().join("apps/new-app");
    assert!(app.join("flake.nix").is_file());
    assert!(!app.join("flake.lock").exists());
    assert_eq!(git(&app, &["ls-files", "flake.nix"]), "flake.nix\n");

    command.env_remove("TEKA_TEST_LOCK_FAIL");
    success(command.output().unwrap());
    assert_eq!(git(&app, &["ls-files"]), FILES.join("\n") + "\n");
}

#[test]
fn help_documents_optional_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_teka"))
        .args(["app", "init", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("[DIR]"));
    assert!(help.contains("[default: .]"));
    assert!(help.contains("--example"));
    assert!(!help.contains("[default: hello-world]"));
}

#[test]
// macOS filesystems reject filenames containing invalid UTF-8.
#[cfg(not(target_os = "macos"))]
fn preserves_non_utf8_filenames_and_index() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    for target in [".", "apps/child-app"] {
        let repo = TempDir::new().unwrap();
        git(repo.path(), &["init"]);
        let app = repo.path().join(target);
        fs::create_dir_all(&app).unwrap();
        let staged = app.join(OsString::from_vec(b"notes-\xff".to_vec()));
        let untracked = app.join(OsString::from_vec(b"untracked-\xfe".to_vec()));
        fs::write(&staged, "staged\n").unwrap();
        git(&app, &["add", "."]);
        let index = git(&app, &["ls-files", "--stage", "--", "notes-*"]);
        assert!(!index.is_empty());
        fs::write(&staged, "unstaged\n").unwrap();
        fs::write(&untracked, "keep\n").unwrap();
        let others = git(&app, &["ls-files", "--others"]);

        success(init(repo.path(), &[target]).0.output().unwrap());

        assert_eq!(git(&app, &["ls-files", "--stage", "--", "notes-*"]), index);
        assert_eq!(git(&app, &["ls-files", "--others"]), others);
        assert_eq!(fs::read(&staged).unwrap(), b"unstaged\n");
        assert_eq!(fs::read(&untracked).unwrap(), b"keep\n");
        assert_eq!(git(&app, &["ls-files", "flake.lock"]), "flake.lock\n");
        if target != "." {
            assert!(!app.join(".git").exists());
        }
    }
}

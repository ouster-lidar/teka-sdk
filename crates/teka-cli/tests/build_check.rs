#![forbid(unsafe_code)]

use std::{
    env, fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::Command,
};
#[path = "common/build.rs"]
#[allow(dead_code)]
mod support;
use support::{contains, excludes, executable, script, CommandExt, Host};

const PREREQUISITES: &str =
    "--version\neval --json --expr builtins ? getFlake\nstore info --json\n";

const NIX: &str = r#"printf '%s\n' "$*" >> "$MOCK_CALLS"
case "$1" in
  --version)
    echo 'nix (Nix) 2.28.0'
    exit "${MOCK_VERSION_STATUS:-0}" ;;
  eval)
    if [ -n "${MOCK_FEATURE_CONF:-}" ]; then
      enabled=false
      while IFS= read -r line; do
        case "$line" in 'extra-experimental-features = nix-command flakes') enabled=true ;; esac
      done < "$MOCK_FEATURE_CONF"
      echo "$enabled"
    else
      printf '%s\n' "${MOCK_CONFIG-true}"
    fi
    exit "${MOCK_CONFIG_STATUS:-0}" ;;
  config)
    if [ -f "$MOCK_CALLS.registration" ]; then
      IFS= read -r registration < "$MOCK_CALLS.registration"
      printf '%s\n' "$registration"
    else
      printf '%s\n' "${MOCK_BUILDERS_JSON-}"
    fi
    exit 0 ;;
  store)
    echo 'mock store diagnostic' >&2
    echo '{}'
    exit "${MOCK_STORE_STATUS:-0}" ;;
  build)
    printf '%s\n' "$@" > "$MOCK_ARGS"
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --file) expression="$2"; shift ;;
        --out-link) out_link="$2"; shift ;;
      esac
      shift
    done
    printf '%s\n' "${MOCK_BUILD_DIAGNOSTIC-mock compiler diagnostic}" >&2
    [ "${MOCK_BUILD_STATUS:-0}" = 0 ] || exit "$MOCK_BUILD_STATUS"
    if [ -n "${expression:-}" ]; then
      "$MOCK_LN" -s "$MOCK_OUTPUT" "$out_link"
    fi
    printf '%s\n' "$MOCK_OUTPUT" ;;
  *) echo 'unexpected nix command' >&2; exit 99 ;;
esac"#;

impl Host {
    fn new() -> Self {
        let host = Self(tempfile::tempdir().unwrap());
        script(&host.path(".local/bin/nix"), NIX);
        executable(
            &host.path(".local/bin/sudo"),
            r#"#!/bin/sh
if [ "$1" = --prompt ]; then shift 2; fi
[ "${MOCK_SUDO_NOOP:-0}" = 1 ] && exit 0
while IFS= read -r line; do
  case "$line" in 'builders = '*) builders=${line#builders = } ;; esac
done < "$7"
printf '{"builders":{"value":"%s"},"builders-use-substitutes":{"value":true},"build-poll-interval":{"value":1},"extra-platforms":{"value":[]}}\n' "$builders" > "$MOCK_CALLS.registration"
"#,
        );
        // PATH resolution must inspect metadata without executing the discovered teka.
        executable(&host.path(".local/bin/teka"), b"not a runnable program\n");
        let mut elf = [0; 64];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16] = 2;
        elf[18] = 183;
        elf[20] = 1;
        elf[52] = 64;
        executable(&host.path("output/bin/hello"), elf);
        host
    }

    fn binary(&self, binary: impl AsRef<std::ffi::OsStr>) -> Command {
        let ln = env::split_paths(&env::var_os("PATH").unwrap())
            .map(|dir| dir.join("ln"))
            .find(|path| path.is_file())
            .unwrap();
        let mut command = self.process(binary, self.0.path(), &self.path(".local/bin"));
        command
            .env("TEKA_BUILD_VM", "never")
            .env("MOCK_BUILDERS_JSON", "{}")
            .env("MOCK_CALLS", self.path("calls"))
            .env("MOCK_ARGS", self.path("args"))
            .env("MOCK_OUTPUT", self.path("output"))
            .env("MOCK_LN", ln);
        command
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = self.binary(env!("CARGO_BIN_EXE_teka"));
        command.args(args);
        command
    }

    fn init(&self, overrides: &[(&str, &str)], code: i32) -> String {
        self.command(&["build", "init"])
            .envs(overrides.iter().copied())
            .report(code)
    }

    fn check(&self, full: bool, overrides: &[(&str, &str)], code: i32) -> String {
        let args = if full {
            &["build", "check", "--full"][..]
        } else {
            &["build", "check"]
        };
        self.command(args)
            .envs(overrides.iter().copied())
            .report(code)
    }
}

trait BuildReport {
    fn report(&mut self, code: i32) -> String;
}

impl BuildReport for Command {
    fn report(&mut self, code: i32) -> String {
        let output = self.checked(code);
        assert!(output.stdout.is_empty(), "{output:?}");
        excludes(
            &output.stderr,
            "Build environment checks passed.\nBuild environment initialization failed.",
        );
        assert!(!output
            .stderr
            .lines()
            .any(|line| ["Tools", "Nix configuration", "Next steps"].contains(&line)));
        for line in output.stderr.lines() {
            if ["✓", "✗", "→", "⏳", "?"]
                .iter()
                .any(|prefix| line.trim_start().starts_with(prefix))
            {
                assert_eq!(line, line.trim_start(), "indented result: {line}");
            }
        }
        if code != 0 {
            assert!(output.stderr.contains('✗'), "{output:?}");
        }
        output.stderr
    }
}

#[test]
fn quick_check_only_inspects_local_prerequisites() {
    let host = Host::new();
    let stderr = host.check(false, &[], 0);
    assert!(stderr.starts_with("✓ teka ("));
    assert!(stderr.contains("✓ Nix config: valid (flakes)\n✓ Nix store: reachable\n"));
    assert!(!stderr.lines().any(|line| line.starts_with("ok:")));
    host.files(&[("calls", PREREQUISITES)]);
    host.missing(&["args", "result"]);
}

#[test]
fn prerequisite_failures_skip_only_dependent_checks() {
    for (overrides, calls, diagnostics) in [
        (vec![("MOCK_VERSION_STATUS", "1")], "--version\n", "skipped"),
        (
            vec![("MOCK_CONFIG", "false")],
            PREREQUISITES,
            "flakes\nteka build init",
        ),
        (
            vec![("MOCK_STORE_STATUS", "1")],
            PREREQUISITES,
            "store\nmock store diagnostic",
        ),
        (
            vec![("MOCK_CONFIG_STATUS", "1"), ("MOCK_STORE_STATUS", "1")],
            PREREQUISITES,
            "Nix evaluation failed
store
mock store diagnostic
✗ Nix config:
✗ Nix store:",
        ),
    ] {
        let host = Host::new();
        let stderr = host.check(true, &overrides, 1);
        contains(&stderr, diagnostics);
        host.files(&[("calls", calls)]);
        host.missing(&["args"]);
        assert!(!stderr.contains("command failed:"));
    }
    let host = Host::new();
    host.remove(".local/bin/nix");
    let stderr = host.check(true, &[], 1);
    contains(&stderr, "Nix\ninstall\nskipped");
    host.missing(&["calls"]);
}

#[test]
fn malformed_configuration_still_checks_store() {
    for config in ["not JSON", "{}", "\"true\"", ""] {
        let host = Host::new();
        host.check(true, &[("MOCK_CONFIG", config)], 1);
        assert_eq!(host.read("calls"), PREREQUISITES, "{config}");
        host.missing(&["args"]);
    }
}

#[test]
fn teka_path_validation_does_not_prevent_nix_checks() {
    for kind in ["missing", "non-executable", "directory"] {
        let host = Host::new();
        let teka = host.path(".local/bin/teka");
        fs::remove_file(&teka).unwrap();
        match kind {
            "non-executable" => fs::write(&teka, b"plain file").unwrap(),
            "directory" => fs::create_dir(&teka).unwrap(),
            _ => {}
        }
        assert!(host.check(false, &[], 1).contains("PATH"));
        host.files(&[("calls", PREREQUISITES)]);
        host.check(true, &[], 1);
        assert!(host.read("calls").contains("build "));
    }
}

#[test]
fn full_check_builds_with_a_fresh_nonce_and_leaves_working_directory_alone() {
    let host = Host::new();
    host.write("result", "existing result");
    let mut previous_nonce = String::new();
    for _ in 0..2 {
        let stderr = host.check(true, &[], 0);
        assert!(!stderr.contains("mock compiler diagnostic"));
        assert!(stderr.contains("aarch64-linux executable verified"));
        excludes(&stderr, host.path("output").to_str().unwrap());
        let args = host.read("args");
        let args: Vec<_> = args.lines().collect();
        let value = |flag| args[args.iter().position(|arg| *arg == flag).unwrap() + 1];
        assert!(args.contains(&"--impure"), "{args:?}");
        assert!(!args.contains(&"--print-build-logs"), "{args:?}");
        assert_eq!(value("--argstr"), "nonce");
        let nonce = args[args.iter().position(|arg| *arg == "nonce").unwrap() + 1];
        assert!(!nonce.is_empty());
        assert_ne!(nonce, previous_nonce);
        previous_nonce = nonce.to_owned();
        assert!(!Path::new(value("--file")).exists());
        assert!(!Path::new(value("--out-link")).exists());
        host.files(&[("result", "existing result")]);
    }
}

#[test]
fn failed_full_build_reports_diagnostics_without_a_rerun() {
    let host = Host::new();
    for diagnostic in ["mock compiler diagnostic\nsecond line", ""] {
        let stderr = host.check(
            true,
            &[
                ("MOCK_BUILD_STATUS", "1"),
                ("MOCK_BUILD_DIAGNOSTIC", diagnostic),
            ],
            1,
        );
        assert!(stderr.contains("Nix exited with status 1"));
        if !diagnostic.is_empty() {
            assert!(stderr.contains("\n\nmock compiler diagnostic\nsecond line\n"));
        }
        assert!(!stderr.contains("rerun"));
    }
}

#[test]
fn verbose_full_check_streams_nix_logs_on_success_and_failure() {
    for flag in ["--verbose", "-v"] {
        for exit in ["0", "1"] {
            let host = Host::new();
            let stderr = host
                .command(&["build", "check", "--full", flag])
                .env("MOCK_BUILD_STATUS", exit)
                .report(exit.parse().unwrap());
            assert_eq!(stderr.matches("mock compiler diagnostic").count(), 1);
            assert!(host
                .read("args")
                .lines()
                .any(|arg| arg == "--print-build-logs"));
        }
    }
}

#[test]
fn full_check_validates_elf_headers_and_executable_permissions() {
    for (offset, byte, code) in [
        (0, 0, 1),
        (4, 1, 1),
        (5, 2, 1),
        (16, 1, 1),
        (18, 62, 1),
        (16, 3, 0),
    ] {
        let host = Host::new();
        let hello = host.path("output/bin/hello");
        let mut bytes = fs::read(&hello).unwrap();
        bytes[offset] = byte;
        executable(&hello, bytes);
        host.check(true, &[], code);
    }
    for kind in ["missing", "non-executable", "truncated"] {
        let host = Host::new();
        let hello = host.path("output/bin/hello");
        match kind {
            "missing" => fs::remove_file(&hello).unwrap(),
            "non-executable" => {
                fs::set_permissions(&hello, fs::Permissions::from_mode(0o644)).unwrap()
            }
            _ => executable(&hello, b"\x7fELF\x02\x01\x01"),
        }
        host.check(true, &[], 1);
    }
}

#[test]
fn init_installs_portable_binaries_and_leaves_nix_packages_managed() {
    let host = Host::new();
    let native = option_env!("TEKA_NIX_PACKAGE").is_some();
    if native {
        host.remove(".local/bin/teka");
    }
    let stderr = host.init(&[], 0);
    contains(&stderr, concat!("✓ teka ", env!("CARGO_PKG_VERSION"), " ("));
    for unique in ["✓ teka", "✓ Nix: ", "✓ Nix config: "] {
        assert_eq!(stderr.matches(unique).count(), 1);
    }
    excludes(&stderr, "✓ initialization\nFish:");
    assert!(stderr.contains("\n\n→ "));
    let destination = host.path(".local/bin/teka");
    if native {
        for _ in 0..2 {
            let stderr = host.init(&[], 0);
            contains(&stderr, "managed by Nix");
            assert_eq!(stderr.matches("✓ teka").count(), 1);
            host.missing(&[".local/bin/teka"]);
        }
        assert_eq!(host.read("calls"), PREREQUISITES.repeat(3));
    } else {
        contains(&stderr, "teka completion --help");
        let inode = fs::metadata(&destination).unwrap().ino();
        let stderr = host.binary(&destination).args(["build", "init"]).report(0);
        assert_eq!(stderr.matches("✓ teka").count(), 1);
        assert_eq!(fs::metadata(&destination).unwrap().ino(), inode);
        assert_eq!(host.read("calls"), PREREQUISITES.repeat(2));
    }
    host.missing(&["result"]);
}

#[test]
fn portable_init_reports_shadowing_and_copy_failures() {
    if option_env!("TEKA_NIX_PACKAGE").is_some() {
        return;
    }
    for (kind, expected) in [
        ("shadow", "shadows\nfirst on PATH"),
        ("directory", "destination is a directory"),
    ] {
        let host = Host::new();
        let mut command = host.command(&["build", "init"]);
        if kind == "shadow" {
            executable(&host.path("shadow/teka"), "#!/bin/sh\necho 'teka 0.0.1'\n");
            command.env(
                "PATH",
                env::join_paths([host.path("shadow"), host.path(".local/bin")]).unwrap(),
            );
        } else {
            host.remove(".local/bin/teka");
            fs::create_dir(host.path(".local/bin/teka")).unwrap();
        }
        let stderr = command.report(1);
        contains(&stderr, expected);
        assert!(!stderr.contains("Build environment initialized."));
        if kind == "directory" {
            assert!(host.path(".local/bin/teka").is_dir());
        }
    }
}

#[test]
fn broken_nix_or_configuration_stops_before_installation() {
    for (name, value) in [
        ("MOCK_VERSION_STATUS", "1"),
        ("MOCK_CONFIG_STATUS", "1"),
        ("MOCK_STORE_STATUS", "1"),
    ] {
        let host = Host::new();
        host.remove(".local/bin/teka");
        let stderr = host.init(&[(name, value)], 1);
        assert_eq!(stderr.matches('✗').count(), 1);
        assert!(!stderr.contains("Teka installation"));
        host.missing(&[".local/bin/teka"]);
        assert!(!stderr.contains("downloading"));
    }
}

#[test]
fn bootstrap_checks_nix_then_builds_portable_and_delegates_installation() {
    let bootstrap = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap.sh");
    // Packaged sources omit the repository bootstrap script.
    if !bootstrap.exists() {
        return;
    }
    for (setting, value) in [
        ("MOCK_VERSION_STATUS", "0"),
        ("MOCK_VERSION_STATUS", "1"),
        ("MOCK_CONFIG", "false"),
        ("MOCK_STORE_STATUS", "1"),
        ("MOCK_BUILD_STATUS", "1"),
    ] {
        let host = Host::new();
        executable(
            &host.path("output/bin/teka"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$MOCK_INIT\"",
        );
        let path = env::join_paths(
            [host.path(".local/bin")]
                .into_iter()
                .chain(env::split_paths(&env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let output = host
            .binary("/bin/sh")
            .arg(&bootstrap)
            .env("PATH", path)
            .env("XDG_DATA_HOME", host.path("data"))
            .env("MOCK_OUTPUT", host.path("output"))
            .env("MOCK_INIT", host.path("init"))
            .env(setting, value)
            .checked(if value == "0" { 0 } else { 1 });
        if value == "0" {
            assert!(output.stdout.is_empty(), "{output:?}");
            host.files(&[("init", "build init\n")]);
            assert!(host.read("calls").contains(".#portable"));
            contains(&output.stderr, "✓ Nix: nix (Nix) 2.28.0\n✓ Nix config: valid (flakes)\n✓ Nix store: reachable\n⏳︎ Building portable Teka with Nix...\n✓ Portable Teka: built (");
            excludes(&output.stderr, "mock compiler diagnostic");
        } else {
            host.missing(&["init"]);
            assert_eq!(output.stderr.matches('✗').count(), 1);
            if setting == "MOCK_VERSION_STATUS" {
                host.files(&[("calls", "--version\n")]);
            } else if setting == "MOCK_BUILD_STATUS" {
                contains(&output.stderr, "✗ Portable Teka: build failed (");
                assert!(output.stderr.ends_with("s)\n\nmock compiler diagnostic\n"));
                assert_eq!(output.stderr.matches("mock compiler diagnostic").count(), 1);
            } else {
                excludes(&host.read("calls"), ".#portable");
            }
        }
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn init_installs_a_standalone_vm_plugin_when_required() {
    if option_env!("TEKA_NIX_PACKAGE").is_some() {
        return;
    }
    let host = Host::new();
    executable(
        &host.path(".local/bin/launchctl"),
        r#"#!/bin/sh
case "$1" in
  print) [ -f "$MOCK_CALLS.loaded" ] || exit 113; echo 'state = running' ;;
  bootstrap) : > "$MOCK_CALLS.loaded" ;;
  enable) exit 0 ;;
  *) exit 99 ;;
esac
"#,
    );
    executable(
        &host.path(".local/bin/sysctl"),
        "#!/bin/sh\nprintf '8\\n17179869184\\n'\n",
    );
    let root = host.path("data/teka-build-vm");
    support::vm_fixture(&root, "private key", "public key");
    let diagnostic = host.init(
        &[
            ("TEKA_BUILD_VM", "auto"),
            ("XDG_DATA_HOME", host.path("data").to_str().unwrap()),
            ("MOCK_SUDO_NOOP", "1"),
        ],
        1,
    );
    assert!(diagnostic.contains("verify system builder registration"));
    assert!(!diagnostic.contains("✓ System remote builder: configured"));
    assert_eq!(diagnostic.matches('✗').count(), 1, "{diagnostic}");
    for _ in 0..2 {
        let stderr = host.init(
            &[
                ("TEKA_BUILD_VM", "auto"),
                ("XDG_DATA_HOME", host.path("data").to_str().unwrap()),
            ],
            0,
        );
        assert!(stderr.contains("✓ Launchd service: running; enabled at login"));
        assert!(!stderr.contains("VM initialization"));
        assert!(!stderr.lines().any(|line| line == "Linux build VM"));
        excludes(&stderr, "console.log");
        assert_eq!(stderr.matches("→ Run `teka build check --full`").count(), 1);
        let version = host
            .binary(host.path(".local/bin/teka-build-vm"))
            .arg("--version")
            .checked(0);
        assert_eq!(
            version.stdout,
            concat!("teka-build-vm ", env!("CARGO_PKG_VERSION"), "\n")
        );
    }
    host.missing(&["data/teka-build-vm/manual", "data/teka-build-vm/vm.log"]);
    assert!(root.join("known_hosts").is_file());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn init_uses_configured_linux_builders_without_installing_vm() {
    let host = Host::new();
    host.init(
        &[
            ("TEKA_BUILD_VM", "auto"),
            (
                "MOCK_BUILDERS_JSON",
                r#"{"builders":{"value":"ssh://remote aarch64-linux,x86_64-linux - 4"}}"#,
            ),
        ],
        0,
    );
    host.missing(&[".local/bin/teka-build-vm"]);
    assert!(host.read("calls").contains("config show --json"));
}

#[test]
fn init_repairs_only_missing_features_and_check_stays_read_only() {
    for custom in [false, true] {
        let host = Host::new();
        let configuration = host.path(if custom {
            "custom/nix.conf"
        } else {
            ".config/nix/nix.conf"
        });
        let original = "max-jobs = 4\nextra-experimental-features = ca-derivations\n";
        fs::create_dir_all(configuration.parent().unwrap()).unwrap();
        fs::write(&configuration, original).unwrap();
        let run = |init: bool, code| {
            let mut command = host.command(if init {
                &["build", "init"]
            } else {
                &["build", "check", "--full"]
            });
            command.env("MOCK_FEATURE_CONF", &configuration);
            if custom {
                command.env(
                    "NIX_USER_CONF_FILES",
                    env::join_paths([&configuration, &host.path("unused.conf")]).unwrap(),
                );
            }
            command.report(code)
        };
        let report = run(false, 1);
        contains(&report, "✗ Nix config: flakes unavailable\n→ AArch64 Linux test build: skipped (Nix config invalid)\n→ Run `teka build init`");
        assert_eq!(fs::read_to_string(&configuration).unwrap(), original);
        let report = run(true, 0);
        contains(
            &report,
            "⏳︎ Enabling Nix features...\n✓ Nix config: valid (flakes) (",
        );
        let repaired = fs::read_to_string(&configuration).unwrap();
        assert_eq!(
            repaired,
            format!("{original}\nextra-experimental-features = nix-command flakes\n")
        );
        let report = run(true, 0);
        assert!(!report.contains("Enabling Nix features"));
        assert_eq!(fs::read_to_string(&configuration).unwrap(), repaired);
    }
}

#[test]
fn feature_repair_preserves_managed_configuration() {
    let host = Host::new();
    let configuration = host.path(".config/nix/nix.conf");
    fs::create_dir_all(configuration.parent().unwrap()).unwrap();
    host.write("managed.conf", "max-jobs = 4\n");
    std::os::unix::fs::symlink(host.path("managed.conf"), &configuration).unwrap();
    let report = host.init(&[("MOCK_CONFIG", "false")], 1);
    contains(&report, "✗ Nix config:\nmanaged\n~/.config/nix/nix.conf");
    assert_eq!(host.read("managed.conf"), "max-jobs = 4\n");
}

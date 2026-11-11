#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![forbid(unsafe_code)]

use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[path = "common/build.rs"]
mod support;
use support::{
    contains, excludes, runtime, script, ssh_timing, vm_fixture, write, CommandExt, Host, Output,
};

impl Host {
    fn new() -> Self {
        let host = Self(tempfile::tempdir().unwrap());
        fs::create_dir_all(host.path("bin")).unwrap();
        fs::create_dir_all(host.path("home & space")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_teka"), host.path("bin/teka-build-vm"))
            .unwrap();
        vm_fixture(&host.vm_path(""), "key", "public");
        script(&host.path("bin/sysctl"), "printf '8\\n17179869184\\n'");
        script(
            &host.path("bin/launchctl"),
            r#"
printf '%s\n' "$*" >> "$MOCK_ROOT/service-calls"
case "$1" in
  print)
    [ -f "$MOCK_ROOT/loaded" ] || exit 113
    case "$2" in
      pid/*)
        expected=4242
        if [ -f "$MOCK_ROOT/service-pid" ]; then expected=$(/bin/cat "$MOCK_ROOT/service-pid"); fi
        [ "$2" = "pid/$expected" ] || exit 113
        printf '%s\n' "$2" > "$MOCK_ROOT/queried-domain"
        if [ "${MOCK_DOMAIN_STATUS:-0}" != 0 ]; then
          echo 'cannot inspect PID domain' >&2
          exit "$MOCK_DOMAIN_STATUS"
        fi
        if [ -f "$MOCK_ROOT/xpc-domain" ]; then
          /bin/cat "$MOCK_ROOT/xpc-domain"
        else
          printf '%s = {\n services = {\n  0 - com.apple.Virtualization.VirtualMachine\n }\n}\n' "$2"
        fi
        exit 0 ;;
    esac
    if [ -n "${MOCK_START_FAILURE:-}" ]; then
      echo 'state = waiting'; echo 'last exit code = 78'
    elif [ ! -f "$MOCK_ROOT/observed" ]; then
      : > "$MOCK_ROOT/observed"
      echo 'state = xpcproxy'; echo 'last exit code = (never exited)'
    else
      echo 'state = running'
    fi
    if [ -f "$MOCK_ROOT/service-pid" ]; then
      printf 'pid = '; /bin/cat "$MOCK_ROOT/service-pid"
    elif [ ! -f "$MOCK_ROOT/disabled" ]; then
      echo 'pid = 4242'
    fi ;;
  print-disabled)
    if [ -f "$MOCK_ROOT/disabled" ]; then state=disabled; else state=enabled; fi
    if [ "${MOCK_DISABLED_FORMAT:-}" = boolean ]; then
      if [ "$state" = disabled ]; then state=true; else state=false; fi
    fi
    printf '"com.teka.build-vm" => %s\n' "$state" ;;
  enable) /bin/rm -f "$MOCK_ROOT/disabled" ;;
  disable) : > "$MOCK_ROOT/disabled" ;;
  bootstrap)
    [ -f "$MOCK_ROOT/data & space/teka-build-vm/known_hosts" ]
    [ -f "$MOCK_ROOT/data & space/teka-build-vm/vm/nixos.qcow2" ]
    /bin/cp "$3" "$MOCK_ROOT/loaded"
    /bin/rm -f "$MOCK_ROOT/observed"
    echo bootstrap >> "$MOCK_ROOT/transitions" ;;
  bootout)
    [ "${MOCK_BOOTOUT_FAILURE:-0}" = 0 ] || exit "$MOCK_BOOTOUT_FAILURE"
    /bin/rm "$MOCK_ROOT/loaded"; echo bootout >> "$MOCK_ROOT/transitions" ;;
  *) exit 99 ;;
esac
"#,
        );
        script(
            &host.path("bin/ssh"),
            r#"
printf '%s\n' "$@" > "$MOCK_ROOT/ssh-args"
printf 'ssh\n' >> "$MOCK_ROOT/ssh-calls"
case "$*" in
  *'exit 3'*) exit 3 ;;
  *__teka_ssh_test__*|*'/bin/btop'*) /bin/cat; exit "${MOCK_SSH_EXIT:-0}" ;;
  *'getent ahosts cache.nixos.org'*)
    printf 'dns\n' >> "$MOCK_ROOT/network-calls"
    exit "${MOCK_DNS_STATUS:-0}" ;;
  *'https://cache.nixos.org/nix-cache-info'*)
    printf 'https\n' >> "$MOCK_ROOT/network-calls"
    printf '%s\n' "$@" > "$MOCK_ROOT/internet-args"
    if [ -n "${MOCK_INTERNET_HANG:-}" ]; then exec /bin/sleep 30; fi
    if [ "${MOCK_INTERNET_STATUS:-0}" != 0 ]; then
      echo "$MOCK_INTERNET_ERROR" >&2
      exit "$MOCK_INTERNET_STATUS"
    fi ;;
  *'/proc/vmstat'*)
    if [ "${MOCK_MEMORY_STATUS:-0}" != 0 ]; then
      echo 'memory probe failed' >&2
      exit "$MOCK_MEMORY_STATUS"
    fi
    exec /bin/cat "$MOCK_ROOT/memory" ;;
  *'/proc/uptime'*)
    if [ -n "${MOCK_METRICS_HANG:-}" ]; then exec /bin/sleep 30; fi
    if [ "${MOCK_METRICS_STATUS:-0}" != 0 ]; then
      echo 'guest metrics failed' >&2
      exit "$MOCK_METRICS_STATUS"
    fi
    exec /bin/cat "$MOCK_ROOT/metrics" ;;
  *)
    if [ "${MOCK_PROBE_STATUS:-0}" != 0 ]; then
      echo 'connection timed out' >&2
      exit "$MOCK_PROBE_STATUS"
    fi ;;

esac
"#,
        );
        script(
            &host.path("bin/nix"),
            r#"
if [ "$1" = config ]; then
  : > "$MOCK_ROOT/config-queried"
  if [ -f "$MOCK_ROOT/registration" ]; then
    /bin/cat "$MOCK_ROOT/registration"
  else
    echo '{"builders":{"value":""},"extra-platforms":{"value":["aarch64-linux"]}}'
  fi
else
  echo build >> "$MOCK_ROOT/runtime-builds"
  [ "${MOCK_EXIT:-0}" = 0 ] || exit "$MOCK_EXIT"
  runtime="$MOCK_ROOT/data & space/teka-build-vm/runtime"
  if [ ! -f "$runtime/bin/teka-build-vm-runner" ]; then
    /bin/mkdir -p "$runtime/bin" "$runtime/share/teka-build-vm"
    printf '#!/bin/sh\nexit 0\n' > "$runtime/bin/teka-build-vm-runner"
    /bin/chmod 755 "$runtime/bin/teka-build-vm-runner"
    printf '/nix/store/abc-btop/bin/btop\n' > "$runtime/share/teka-build-vm/btop"
  fi
fi
"#,
        );
        host.write(
            "memory",
            r#"{"total":8589934592,"available":7516192768,"swap":0,"oom_kills":0}"#,
        );
        script(
            &host.path("bin/ssh-keygen"),
            r#"
while [ "$#" -gt 0 ]; do
  if [ "$1" = -f ]; then key=$2; break; fi
  shift
done
printf 'fresh private key\n' > "$key"
printf 'ssh-ed25519 FRESH fixture\n' > "$key.pub"
"#,
        );
        script(
            &host.path("bin/sudo"),
            r#"
[ "${MOCK_SUDO_EXIT:-0}" = 0 ] || exit "$MOCK_SUDO_EXIT"
if [ "$1" = --prompt ]; then
  printf '%s\n' "$2" > "$MOCK_ROOT/sudo-prompt"
  shift 2
fi
while IFS= read -r line; do
  case "$line" in 'builders = '*) builders=${line#builders = } ;; esac
done < "$7"
/bin/cp "$6" "$MOCK_ROOT/system-private-key"
/bin/cp "$7" "$MOCK_ROOT/system-configuration"
printf '{"builders":{"value":"%s"},"builders-use-substitutes":{"value":true},"build-poll-interval":{"value":1},"extra-platforms":{"value":[]}}\n' "$builders" > "$MOCK_ROOT/registration"
"#,
        );
        host
    }

    fn read_vm(&self, name: &str) -> String {
        fs::read_to_string(self.vm_path(name)).unwrap()
    }
    fn write_vm(&self, name: &str, contents: impl AsRef<[u8]>) {
        write(self.vm_path(name), contents);
    }
    fn vm_files(&self, expected: &[(&str, &str)]) {
        for (name, contents) in expected {
            assert_eq!(self.read_vm(name), *contents, "{name}");
        }
    }
    fn vm_missing(&self, names: &[&str]) {
        for name in names {
            assert!(!self.vm_path(name).exists(), "{name}");
        }
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).checked(0)
    }
    fn report(&self, args: &[&str], code: i32, env: &[(&str, &str)]) -> String {
        self.command(args)
            .envs(env.iter().copied())
            .checked(code)
            .stderr
    }
    fn vm_path(&self, name: &str) -> PathBuf {
        self.path("data & space/teka-build-vm").join(name)
    }
    fn agent(&self) -> PathBuf {
        self.path("home & space/Library/LaunchAgents/com.teka.build-vm.plist")
    }
    fn plist(&self) -> serde_json::Value {
        let output = Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(self.agent())
            .checked(0);
        serde_json::from_str(&output.stdout).unwrap()
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = self.process(
            self.path("bin/teka-build-vm"),
            &self.path("home & space"),
            &self.path("bin"),
        );
        command
            .args(args)
            .env("XDG_DATA_HOME", self.path("data & space"))
            .env("MOCK_ROOT", self.0.path())
            .env_remove("NIX_SSL_CERT_FILE")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

#[test]
fn lifecycle_preserves_data_and_launch_agent() {
    let host = Host::new();
    for _ in 0..2 {
        let report = host.run(&["start"]).stderr;
        excludes(&report, "VM startup\nLinux build VM ready.");
        contains(&report, "✓ Launchd service: running; enabled at login");
        ssh_timing(&report);
    }
    host.files(&[("transitions", "bootstrap\n")]);
    assert_eq!(host.read("ssh-calls").lines().count(), 2);
    host.write_vm("saved-data", "keep me");
    host.run(&["init"]);
    host.vm_files(&[("saved-data", "keep me"), ("keys/builder_ed25519", "key")]);
    host.vm_missing(&["launch.json", "resources.json"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&host.read_vm("config.json")).unwrap(),
        serde_json::json!({"cpus": 6, "memoryMiB": 8192, "idleTimeoutSeconds": 1800}),
    );
    let plist = host.plist();
    let expected = serde_json::json!({
        "RunAtLoad": true, "KeepAlive": true, "ExitTimeOut": 45,
        "WorkingDirectory": host.vm_path("vm"),
        "ProgramArguments": [host.vm_path("runtime/bin/teka-build-vm-runner")],
    });
    for (key, value) in expected.as_object().unwrap() {
        assert_eq!(&plist[key], value, "{key}");
    }
    for (key, value) in [
        ("HOME", host.path("home & space")),
        ("KEYS", host.vm_path("keys")),
        ("NIX_SSL_CERT_FILE", "/etc/ssl/cert.pem".into()),
    ] {
        assert_eq!(plist["EnvironmentVariables"][key], value.to_str().unwrap());
    }
    assert_eq!(plist["StandardOutPath"], plist["StandardErrorPath"]);
    for _ in 0..2 {
        let report = host.run(&["stop"]).stderr;
        contains(&report, "✓ Launchd service: stopped; disabled at login");
        excludes(&report, "VM shutdown\nLinux build VM stopped.");
    }
    assert_eq!(host.read("transitions"), "bootstrap\nbootout\n");
    assert!(host.path("disabled").exists());
    assert_eq!(host.plist(), plist);
    host.run(&["status"]);
    assert!(host.path("disabled").exists());
}

#[test]
fn host_key_rotation_updates_both_pins_before_start_and_retries_failed_registration() {
    let host = Host::new();
    let key = host.vm_path("keys/ssh_host_ed25519_key");
    let rotate = || {
        fs::remove_file(&key).unwrap();
        fs::remove_file(key.with_extension("pub")).unwrap();
        Command::new("/usr/bin/ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "", "-f"])
            .arg(&key)
            .checked(0);
        let derived = Command::new("/usr/bin/ssh-keygen")
            .args(["-y", "-f"])
            .arg(&key)
            .checked(0)
            .stdout;
        assert_eq!(
            fs::read_to_string(key.with_extension("pub"))
                .unwrap()
                .trim(),
            derived.trim()
        );
        derived.trim().to_owned()
    };
    let original = rotate();
    host.run(&["start"]);
    let pin = host.read_vm("known_hosts");
    let public = rotate();
    assert_ne!(original, public);
    let private = fs::read_to_string(&key).unwrap();
    host.command(&["start"])
        .failed()
        .stderr_contains("stop the VM service before updating its SSH host key");
    assert_eq!(host.read_vm("known_hosts"), pin);
    host.run(&["stop"]);
    host.report(&["start"], 1, &[("MOCK_SUDO_EXIT", "1")]);
    assert_eq!(host.read_vm("known_hosts"), pin);
    host.missing(&["loaded"]);
    host.run(&["start"]);
    assert_eq!(
        host.read_vm("known_hosts"),
        format!("[127.0.0.1]:31023 {public}\n")
    );
    assert!(host
        .read("system-configuration")
        .contains(&openssl::base64::encode_block(public.as_bytes())));
    assert_eq!(fs::read_to_string(key).unwrap(), private);
    host.missing(&["runtime-builds"]);
}

#[test]
fn existing_vms_get_a_persistent_host_key_without_resetting_the_disk() {
    let host = Host::new();
    host.run(&["start"]);
    host.run(&["stop"]);
    host.remove("data & space/teka-build-vm/keys/ssh_host_ed25519_key");
    host.remove("data & space/teka-build-vm/keys/ssh_host_ed25519_key.pub");
    let disk = fs::metadata(host.vm_path("vm/nixos.qcow2")).unwrap().ino();
    host.run(&["init"]);
    assert_eq!(
        host.read_vm("keys/ssh_host_ed25519_key"),
        "fresh private key\n"
    );
    assert_eq!(
        fs::metadata(host.vm_path("vm/nixos.qcow2")).unwrap().ino(),
        disk
    );
    host.run(&["start"]);
    assert_eq!(
        host.read_vm("keys/ssh_host_ed25519_key"),
        "fresh private key\n"
    );
    assert_eq!(host.read_vm("keys/builder_ed25519"), "key");
}

#[test]
fn runtime_upgrade_requires_stop_and_preserves_disk_and_keys() {
    let host = Host::new();
    host.run(&["start"]);
    host.write_vm("launch.json", "{}");
    let old_runtime = "legacy always-on runtime; idleTimeout = null;";
    host.write_vm("runtime.nix", old_runtime);
    let disk = host.vm_path("vm/nixos.qcow2");
    let inode = fs::metadata(&disk).unwrap().ino();
    fs::OpenOptions::new()
        .write(true)
        .open(&disk)
        .unwrap()
        .write_all(b"old")
        .unwrap();
    for command in ["start", "init"] {
        let report = host.command(&[command]).failed().stderr;
        assert!(report.contains("stop the VM service before updating its runtime"));
        host.vm_files(&[
            ("runtime.nix", old_runtime),
            ("keys/builder_ed25519", "key"),
        ]);
        host.missing(&["runtime-builds"]);
    }
    host.run(&["stop"]);
    host.run(&["start"]);
    assert_eq!(host.read_vm("runtime.nix"), runtime());
    assert!(host.read_vm("runtime.nix").contains("idleTimeout = 1800;"));
    host.vm_files(&[("keys/builder_ed25519", "key"), ("launch.json", "{}")]);
    assert_eq!(host.read("runtime-builds"), "build\n");
    assert_eq!(fs::metadata(&disk).unwrap().ino(), inode);
    let mut prefix = [0; 3];
    fs::File::open(&disk)
        .unwrap()
        .read_exact(&mut prefix)
        .unwrap();
    assert_eq!(&prefix, b"old");
    host.write_vm("launch.json", "invalid legacy policy");
    host.run(&["init"]);
}

#[test]
fn start_reuses_saved_settings_and_updates_only_supplied_fields() {
    let host = Host::new();
    host.write_vm("resources.json", "invalid legacy allocation");
    host.write_vm("launch.json", "invalid legacy policy");
    let custom = [
        "start",
        "--cpus",
        "2",
        "--memory",
        "512MiB",
        "--idle-timeout",
        "5",
    ];
    host.run(&custom);
    let settings = host.read_vm("config.json");
    let runtime = host.read_vm("runtime.nix");
    let plist = host.plist();
    assert_eq!(plist["EnvironmentVariables"]["TEKA_VM_CPUS"], "2");
    assert_eq!(plist["EnvironmentVariables"]["TEKA_VM_MEMORY_MIB"], "512");
    assert!(runtime.contains("idleTimeout = 5;"));
    host.run(&["start"]);
    host.run(&custom);
    host.run(&["init"]);
    assert_eq!(host.read("transitions"), "bootstrap\n");
    for argument in ["--cpus=1", "--memory=1", "--idle-timeout=0"] {
        let report = host.command(&["start", argument]).failed().stderr;
        assert!(report.contains("stop the VM service before changing its settings"));
        host.vm_files(&[("config.json", &settings), ("runtime.nix", &runtime)]);
        assert_eq!(host.plist(), plist);
    }
    for argument in [
        "--cpus=0",
        "--cpus=9",
        "--memory=17",
        "--idle-timeout=604801",
    ] {
        host.command(&["start", argument]).failed();
        host.vm_files(&[("config.json", &settings), ("runtime.nix", &runtime)]);
        assert_eq!(host.plist(), plist);
    }
    host.run(&["stop"]);
    host.run(&["start"]);
    host.vm_files(&[("config.json", &settings), ("runtime.nix", &runtime)]);
    assert_eq!(host.plist(), plist);
    host.run(&["stop"]);
    host.run(&["start", "--memory", "1"]);
    let settings = serde_json::json!({"cpus": 2, "memoryMiB": 1024, "idleTimeoutSeconds": 5});
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&host.read_vm("config.json")).unwrap(),
        settings
    );
    host.run(&["stop"]);
    host.run(&["start", "--idle-timeout", "0"]);
    assert!(host.read_vm("runtime.nix").contains("idleTimeout = null;"));
    host.run(&["stop"]);
    host.run(&["start"]);
    assert!(host.read_vm("runtime.nix").contains("idleTimeout = null;"));
    host.vm_files(&[
        ("resources.json", "invalid legacy allocation"),
        ("launch.json", "invalid legacy policy"),
    ]);
}

#[test]
fn settings_survive_failed_start_and_reset_recovers_corruption() {
    let host = Host::new();
    host.write_vm("runtime.nix", "old runtime");
    host.report(
        &[
            "start",
            "--cpus",
            "2",
            "--memory",
            "8",
            "--idle-timeout",
            "300",
        ],
        1,
        &[("MOCK_EXIT", "7")],
    );
    let settings = host.read_vm("config.json");
    host.missing(&["loaded"]);
    host.run(&["start"]);
    host.vm_files(&[("config.json", &settings)]);
    assert!(host.read_vm("runtime.nix").contains("idleTimeout = 300;"));
    assert_eq!(host.plist()["EnvironmentVariables"]["TEKA_VM_CPUS"], "2");
    host.write_vm("config.json", "invalid settings");
    let report = host.command(&["start"]).failed().stderr;
    assert!(report.contains("read VM settings"));
    host.run(&["init", "--reset"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&host.read_vm("config.json")).unwrap(),
        serde_json::json!({"cpus": 6, "memoryMiB": 8192, "idleTimeoutSeconds": 1800})
    );
    host.run(&["stop"]);
    host.remove("data & space/teka-build-vm/config.json");
    host.run(&["start"]);
    host.remove("data & space/teka-build-vm/config.json");
    let runtime = host.read_vm("runtime.nix");
    let plist = host.plist();
    let report = host.command(&["start"]).failed().stderr;
    assert!(report.contains("stop the VM service before changing its settings"));
    host.vm_missing(&["config.json"]);
    host.vm_files(&[("runtime.nix", &runtime)]);
    assert_eq!(host.plist(), plist);
}

#[test]
fn invalid_host_capacity_does_not_load_service() {
    for output in [
        "0\n17179869184\n",
        "8\n0\n",
        "invalid\n17179869184\n",
        "8\ninvalid\n",
        "8\n",
        "",
    ] {
        let host = Host::new();
        script(&host.path("bin/sysctl"), &format!("printf '%s' '{output}'"));
        let report = host.command(&["start"]).failed().stderr;
        assert!(report.contains("host CPU") || report.contains("host RAM"));
        host.missing(&["loaded"]);
        assert!(!host.agent().exists());
    }
}

#[test]
fn startup_failures_leave_diagnostics_and_retryable_state() {
    let host = Host::new();
    host.write_vm("runtime.nix", "old runtime");
    for _ in 0..2 {
        let report = host.report(&["start"], 1, &[("MOCK_EXIT", "7")]);
        assert!(report.contains("Nix exited with status 7"));
        assert_eq!(host.read_vm("runtime.nix"), "old runtime");
        host.missing(&["loaded"]);
    }
    let report = host.report(&["start"], 1, &[("MOCK_START_FAILURE", "1")]);
    contains(&report, "vm.log\n78\n✗ Launchd service:");
    host.run(&["start"]);
    let transitions = host.read("transitions");
    let report = host.report(&["start"], 1, &[("MOCK_PROBE_STATUS", "1")]);
    contains(&report, "✗ SSH: connection timed out\n→ Check the VM log:");
    excludes(&report, "✓ SSH\n✓ Linux build VM ready.");
    assert_eq!(host.read("transitions"), transitions);
}

#[test]
fn ssh_and_btop_forward_arguments_input_and_exit_status() {
    let host = Host::new();
    host.run(&["start"]);
    let mut child = host
        .command(&["ssh", "-T", "__teka_ssh_test__", "two words", "--flag", ""])
        .env("MOCK_SSH_EXIT", "7")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"SSH input\n")
        .unwrap();
    let output = Output::from(child.wait_with_output().unwrap()).check(7);
    assert_eq!(output.stdout, "SSH input\n");
    output.stderr_contains("⏳︎ Starting VM...\n✓ SSH: reachable (");
    let args = host.read("ssh-args");
    assert!(args.ends_with("-T\n__teka_ssh_test__\ntwo words\n--flag\n\n"));
    contains(&args, "ConnectTimeout=180\nStrictHostKeyChecking=yes");
    let config = Command::new("/usr/bin/ssh")
        .args(["-G", "-F", "/dev/null"])
        .args(args.lines())
        .checked(0);
    config.stdout_contains(host.vm_path("known_hosts").to_str().unwrap());
    config.stdout_contains("identitiesonly yes");
    host.command(&["btop"])
        .env("MOCK_SSH_EXIT", "7")
        .stdin(Stdio::null())
        .checked(7);
    assert!(host
        .read("ssh-args")
        .ends_with("/nix/store/abc-btop/bin/btop\n"));
    host.vm_missing(&["usage.lock"]);
    host.run(&["stop"]);
    host.command(&["ssh"]).failed();
    assert!(host.path("disabled").exists());
}

#[test]
fn status_reports_stopped_or_waiting_without_contacting_guest_or_changing_enablement() {
    let host = Host::new();
    host.run(&["status"])
        .stdout_contains("Launchd: not installed; service stopped");
    host.run(&["start"]);
    host.remove("ssh-args");
    host.write_vm("vm/vzvm.json", "invalid guest configuration");
    host.write("service-calls", "");
    for stopped in [false, true] {
        if stopped {
            host.run(&["stop"]);
            host.write("service-calls", "");
        }
        let state = if stopped {
            "disabled at login; service stopped"
        } else {
            "enabled at login; service running"
        };
        for format in ["words", "boolean"] {
            let output = host
                .command(&["status"])
                .env("MOCK_DISABLED_FORMAT", format)
                .checked(0);
            assert!(output.stderr.is_empty(), "{output:?}");
            let guest = if stopped {
                "stopped"
            } else {
                "waiting for connection; guest details unavailable"
            };
            assert_eq!(
                output.stdout,
                format!("Launchd: {state}\nVM: {guest}\n{CONFIGURED_RESOURCES}")
            );
        }
        host.missing(&["ssh-args", "config-queried"]);
        assert!(host.read("service-calls").lines().all(|line| matches!(
            line.split_whitespace().next(),
            Some("print" | "print-disabled")
        )));
    }
    host.remove("disabled");
    host.run(&["status"])
        .stdout_contains("Launchd: enabled at login; service stopped");
    host.missing(&["ssh-args"]);
}

#[test]
fn status_reports_available_resources_with_missing_or_corrupt_settings() {
    let host = Host::new();
    host.run(&["start"]);
    host.run(&["stop"]);
    host.remove("ssh-args");
    let settings = host.read_vm("config.json");
    host.remove("data & space/teka-build-vm/config.json");
    let output = host.run(&["status"]);
    output.stdout_contains("VM: stopped\nDisk: 64 GiB capacity");
    excludes(&output.stdout, "CPUs:\nRAM:\nIdle timeout:\nConfiguration:");
    host.vm_missing(&["config.json"]);
    host.write_vm("config.json", "invalid settings");
    let output = host.run(&["status"]);
    output.stdout_contains("Configuration: unavailable\nDisk: 64 GiB capacity");
    host.vm_files(&[("config.json", "invalid settings")]);
    for (seconds, expected) in [(0, "disabled"), (61, "61 s"), (120, "2 min")] {
        let mut configuration: serde_json::Value = serde_json::from_str(&settings).unwrap();
        configuration["idleTimeoutSeconds"] = seconds.into();
        host.write_vm("config.json", configuration.to_string());
        host.run(&["status"])
            .stdout_contains(&format!("Idle timeout: {expected}"));
    }
    host.remove("data & space/teka-build-vm/vm/nixos.qcow2");
    let output = host.run(&["status"]);
    output.stdout_contains("CPUs: 6 configured\nRAM: 8 GiB configured");
    excludes(&output.stdout, "Disk:");
    host.vm_missing(&["vm/nixos.qcow2"]);
    host.missing(&["ssh-args", "config-queried"]);
}

const CONFIGURED_RESOURCES: &str = "CPUs: 6 configured
RAM: 8 GiB configured
Idle timeout: 30 min
Disk: 64 GiB capacity
";

const RUNNING_GUEST: &str = "pid/8787 = {
 services = {
  0 - com.apple.Virtualization.VirtualMachine
  95029 - com.apple.Virtualization.VirtualMachine.3CFF0562-71AA-42FF-83ED-5ACAF8958EAF
  0 - com.apple.diskimagescontroller
 }
}";
const GUEST_METRICS: &str = "Uptime: 2 min; load: 0.50 0.25 0.10
CPUs: 6
RAM: 1 GiB / 8 GiB
Disk: 2 GiB / 64 GiB
";

#[test]
fn status_probes_only_the_current_monitors_running_helper() {
    let host = Host::new();
    host.run(&["start"]);
    host.write("service-pid", "8787\n");
    host.remove("ssh-args");
    host.write("ssh-calls", "");
    host.write("service-calls", "");
    host.write(
        "xpc-domain",
        "pid/8787 = {
 environment = {
  95029 - com.apple.Virtualization.VirtualMachine.fake
 }
 services = {
  0 - com.apple.Virtualization.VirtualMachine
  95029 - com.apple.Virtualization.VirtualMachineOther
 }
 unmanaged processes = {
  95029 - com.apple.Virtualization.VirtualMachine.fake
 }
}",
    );
    let output = host.run(&["status"]);
    output.stdout_contains("VM: waiting for connection; guest details unavailable");
    host.missing(&["ssh-args"]);
    assert_eq!(host.read("queried-domain"), "pid/8787\n");
    host.write("xpc-domain", RUNNING_GUEST);
    host.write("metrics", r#"{"uptime":120,"load":[0.50,0.25,0.10],"cpus":6,"memory":[1073741824,8589934592],"disk":[2147483648,68719476736]}"#);
    let settings = host.read_vm("config.json");
    let runtime = host.read_vm("runtime.nix");
    let plist = host.plist();
    let output = host.run(&["status"]);
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout,
        format!(
            "Launchd: enabled at login; service running\nVM: running; SSH ready\n{GUEST_METRICS}"
        )
    );
    assert_eq!(host.read("ssh-calls"), "ssh\n");
    contains(
        &host.read("ssh-args"),
        "ConnectTimeout=2\nConnectionAttempts=1\nStrictHostKeyChecking=yes\n-p\n31023",
    );
    host.vm_files(&[("config.json", &settings), ("runtime.nix", &runtime)]);
    assert_eq!(host.plist(), plist);
    assert_eq!(host.read("transitions"), "bootstrap\n");
    assert!(host.read("service-calls").lines().all(|line| matches!(
        line.split_whitespace().next(),
        Some("print" | "print-disabled")
    )));
    host.missing(&["config-queried"]);
}

#[test]
fn status_skips_guest_when_monitor_or_domain_state_is_unknown() {
    let host = Host::new();
    host.run(&["start"]);
    host.write("service-pid", "8787\n");
    host.remove("ssh-args");
    for domain in [
        "invalid PID domain",
        "pid/8787 = {\n properties = slain\n}",
        "pid/8787 = {\n services = {\n invalid - com.apple.Virtualization.VirtualMachine.UUID\n }\n}",
    ] {
        host.write("xpc-domain", domain);
        let output = host.run(&["status"]);
        output.stdout_contains("VM: unknown; guest details unavailable");
        output.stdout_contains(CONFIGURED_RESOURCES);
        host.missing(&["ssh-args"]);
    }
    host.command(&["status"])
        .env("MOCK_DOMAIN_STATUS", "7")
        .checked(0)
        .stdout_contains("VM: unknown; guest details unavailable");
    host.remove("queried-domain");
    for pid in ["", "invalid\n", "0\n"] {
        host.write("service-pid", pid);
        host.run(&["status"])
            .stdout_contains("VM: unknown; guest details unavailable");
        host.missing(&["queried-domain", "ssh-args"]);
    }
}

#[test]
fn status_metrics_failures_are_unavailable_and_have_a_deadline() {
    let host = Host::new();
    host.run(&["start"]);
    host.write("service-pid", "8787\n");
    host.write("xpc-domain", RUNNING_GUEST);
    let output = host
        .command(&["status"])
        .env("MOCK_METRICS_STATUS", "7")
        .checked(0);
    output.stdout_contains("VM: running; guest details unavailable\nguest metrics failed");
    output.stdout_contains(CONFIGURED_RESOURCES);
    host.write("metrics", "invalid metrics");
    let output = host.run(&["status"]);
    output.stdout_contains("read guest metrics");
    output.stdout_contains(CONFIGURED_RESOURCES);
    let started = Instant::now();
    let output = host
        .command(&["status"])
        .env("MOCK_METRICS_HANG", "1")
        .checked(0);
    output.stdout_contains("VM: running; guest details unavailable");
    output.stdout_contains(CONFIGURED_RESOURCES);
    assert!(output.stdout.contains("timed out"), "{output:?}");
    assert!(started.elapsed() < Duration::from_secs(10), "{output:?}");
    assert_eq!(host.read("transitions"), "bootstrap\n");
}

#[test]
fn check_wakes_on_demand_guests_and_skips_unavailable_services() {
    let host = Host::new();
    host.run(&["start"]);
    let report = host.report(&["check"], 1, &[]);
    contains(
        &report,
        "✓ Launchd service: running
✓ Guest Nix daemon: available
✗ System remote builder:
⏳︎ Starting VM...",
    );
    ssh_timing(&report);
    contains(
        &report,
        "✓ Guest memory: 0 kernel OOM kills since boot; RAM: 7 GiB / 8 GiB available; swap: 0 B",
    );
    contains(
        &report,
        "✓ Guest DNS: cache.nixos.org resolves
✓ Guest internet (HTTPS): cache.nixos.org reachable",
    );
    assert_eq!(host.read("network-calls"), "dns\nhttps\n");
    contains(
        &host.read("internet-args"),
        "curl --disable --fail --silent --show-error --location --connect-timeout 5 --max-time 10 --output /dev/null https://cache.nixos.org/nix-cache-info",
    );
    host.remove("network-calls");
    let report = host.report(&["check"], 1, &[("MOCK_PROBE_STATUS", "1")]);
    contains(
        &report,
        "✗ SSH:\nconnection timed out\n→ Guest checks: skipped",
    );
    host.missing(&["network-calls"]);
    for stopped in [false, true] {
        if stopped {
            host.run(&["stop"]);
        }
        fs::remove_file(host.path("ssh-args")).ok();
        let report = host.report(&["check"], 1, &[("MOCK_START_FAILURE", "1")]);
        assert!(report
            .contains("✗ Launchd service: stopped\n→ Live checks: skipped (VM service stopped)\n"));
        assert_eq!(report.matches("stopped").count(), 2);
        assert_eq!(report.matches("teka build vm start").count(), 1);
        host.missing(&["ssh-args", "network-calls"]);
    }
}

#[test]
fn check_reports_guest_dns_and_https_failures_with_vpn_guidance() {
    let host = Host::new();
    host.run(&["start"]);
    let report = host.report(&["check"], 1, &[("MOCK_DNS_STATUS", "2")]);
    contains(
        &report,
        "✗ Guest DNS:
→ Guest internet (HTTPS): skipped (DNS failed)
Check host internet access and the VM log
✓ Guest memory:",
    );
    assert_eq!(host.read("network-calls"), "dns\n");
    for (status, error) in [
        ("7", "curl: (7) Failed to connect to cache.nixos.org"),
        ("28", "curl: (28) Connection timed out"),
        ("60", "curl: (60) SSL certificate problem"),
        ("22", "curl: (22) The requested URL returned error: 503"),
    ] {
        host.remove("network-calls");
        let report = host.report(
            &["check"],
            1,
            &[
                ("MOCK_INTERNET_STATUS", status),
                ("MOCK_INTERNET_ERROR", error),
            ],
        );
        contains(
            &report,
            "✓ Guest DNS:
✗ Guest internet (HTTPS):
Check host internet access and the VM log
✓ Guest memory:",
        );
        contains(&report, error);
        excludes(&report, "✓ Guest internet (HTTPS):");
        assert_eq!(host.read("network-calls"), "dns\nhttps\n");
    }
}

#[test]
fn check_internet_probe_has_a_deadline() {
    let host = Host::new();
    host.run(&["start"]);
    let report = host.report(&["check"], 1, &[("MOCK_INTERNET_HANG", "1")]);
    contains(
        &report,
        "✗ Guest internet (HTTPS): SSH guest probe timed out after 15s
✓ Guest memory:",
    );
}

#[test]
fn check_detects_oom_even_after_memory_is_freed_and_reports_probe_failures() {
    let host = Host::new();
    host.run(&["start"]);
    contains(&host.report(&["check"], 1, &[]), "✓ Guest memory:");
    host.write(
        "memory",
        r#"{"total":8589934592,"available":7516192768,"swap":0,"oom_kills":6}"#,
    );
    let report = host.report(&["check"], 1, &[]);
    contains(
        &report,
        "✗ Guest memory: 6 kernel OOM kills since boot; RAM: 7 GiB / 8 GiB available; swap: 0 B",
    );
    contains(&report, "journalctl -k -b0");
    contains(&report, "OOM counts reset on guest reboot");
    host.write("memory", "{}");
    contains(&host.report(&["check"], 1, &[]), "✗ Guest memory:");
    let report = host.report(&["check"], 1, &[("MOCK_MEMORY_STATUS", "7")]);
    contains(&report, "memory probe failed");
    excludes(&report, "Inspect OOM victims");
}

#[test]
fn reset_waits_for_shutdown_and_recreates_state_with_default_settings() {
    let host = Host::new();
    host.run(&["start"]);
    host.run(&["init"]);
    host.write_vm("resources.json", "legacy allocation");
    host.write_vm("saved-data", "discard me");
    host.write("unrelated-data", "preserve me");
    std::os::unix::fs::symlink(host.path("unrelated-data"), host.vm_path("external-link")).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(host.vm_path("vm/nixos.qcow2"))
        .unwrap()
        .write_all(b"old")
        .unwrap();
    let lock = host.vm_path("").with_extension("lock");
    let inode = fs::metadata(&lock).unwrap().ino();
    let mut process = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    host.write("service-pid", format!("{}\n", process.id()));
    let reset = host.command(&["init", "--reset"]).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while host.path("loaded").exists() {
        assert!(
            Instant::now() < deadline,
            "reset did not unload the service"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(host.vm_path("saved-data").exists());
    drop(process.stdin.take());
    assert!(process.wait().unwrap().success());
    let report = Output::from(reset.wait_with_output().unwrap())
        .check(0)
        .stderr;
    assert!(report.starts_with("⏳︎ Stopping VM service...\n"));
    contains(
        &report,
        "⏳︎ Stopping VM service...
✓ Launchd service: stopped; disabled at login
⏳︎ Removing old VM state...
✓ Old VM state: removed
⏳︎ Preparing VM runtime...
✓ VM runtime: ready
⏳︎ Registering system remote builder...
✓ System remote builder: configured
⏳︎ Starting VM service...",
    );
    assert!(report
        .lines()
        .last()
        .unwrap()
        .starts_with("✓ Launchd service: running; enabled at login ("));
    excludes(&report, "sudo\n?");
    excludes(&report, "VM shutdown\n✓ Linux build VM stopped.");
    assert_eq!(
        host.read("sudo-prompt"),
        "? macOS password for %p (sudo): \n"
    );
    host.vm_missing(&["saved-data", "external-link", "resources.json"]);
    assert_eq!(host.read("unrelated-data"), "preserve me");
    host.vm_missing(&["launch.json"]);
    assert_eq!(host.read_vm("keys/builder_ed25519"), "fresh private key\n");
    assert_eq!(host.read("system-private-key"), "fresh private key\n");
    assert_eq!(
        host.read_vm("keys/ssh_host_ed25519_key"),
        "fresh private key\n"
    );
    assert_eq!(
        host.read_vm("known_hosts"),
        "[127.0.0.1]:31023 ssh-ed25519 FRESH fixture\n"
    );
    assert!(host
        .read("system-configuration")
        .contains(&openssl::base64::encode_block(b"ssh-ed25519 FRESH fixture")));
    assert!(host
        .read("system-configuration")
        .contains("extra-platforms = \n"));
    assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
    assert_eq!(
        fs::metadata(host.vm_path("")).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let mut prefix = [0; 3];
    fs::File::open(host.vm_path("vm/nixos.qcow2"))
        .unwrap()
        .read_exact(&mut prefix)
        .unwrap();
    assert_eq!(prefix, [0; 3]);
    assert_eq!(
        fs::metadata(host.vm_path("vm/nixos.qcow2")).unwrap().len(),
        64 * 1024 * 1024 * 1024
    );
    let plist = host.plist();
    assert_eq!(plist["EnvironmentVariables"]["TEKA_VM_CPUS"], "6");
    assert_eq!(plist["EnvironmentVariables"]["TEKA_VM_MEMORY_MIB"], "8192");
    assert!(host.path("loaded").exists());
    host.missing(&["disabled"]);
    assert_eq!(host.read("transitions"), "bootstrap\nbootout\nbootstrap\n");
}

#[test]
fn reset_keeps_existing_state_when_the_service_cannot_be_stopped() {
    let host = Host::new();
    host.run(&["start"]);
    host.write_vm("saved-data", "keep me");
    let report = host.report(&["stop"], 1, &[("MOCK_BOOTOUT_FAILURE", "7")]);
    assert!(
        report.starts_with("⏳︎ Stopping VM service...\n") && report.contains("✗ Launchd service:"),
        "{report}"
    );
    assert!(!report.contains("✓ Linux build VM stopped."));
    let report = host.report(&["init", "--reset"], 1, &[("MOCK_BOOTOUT_FAILURE", "7")]);
    assert!(report.contains("✗ Launchd service:"));
    assert_eq!(host.read_vm("saved-data"), "keep me");
    assert_eq!(host.read_vm("keys/builder_ed25519"), "key");
    host.missing(&["runtime-builds"]);
    assert!(host.path("loaded").exists());
}

#[test]
fn failed_reset_setup_stays_disabled_and_can_be_retried_without_reset() {
    let host = Host::new();
    host.run(&["start"]);
    host.write_vm("saved-data", "discard me");
    host.report(&["init", "--reset"], 1, &[("MOCK_EXIT", "7")]);
    host.vm_missing(&["saved-data"]);
    assert!(host.path("disabled").exists());
    host.missing(&["loaded"]);
    assert!(!host.agent().exists());
    host.run(&["init"]);
    host.vm_missing(&["launch.json"]);
    assert!(host.path("loaded").exists());
    host.missing(&["disabled"]);
}

#[test]
fn ssh_and_btop_are_silent_for_running_guests_and_preserve_remote_exit_three() {
    let host = Host::new();
    host.run(&["start"]);
    for (domain, progress, connections) in [
        (
            "pid/4242 = {\n services = {\n  123 - com.apple.Virtualization.VirtualMachine\n }\n}\n",
            None,
            1,
        ),
        (
            "pid/4242 = {\n services = {\n }\n}\n",
            Some("⏳︎ Starting VM..."),
            2,
        ),
        ("unknown domain\n", Some("⏳︎ Connecting to guest..."), 2),
    ] {
        host.write("xpc-domain", domain);
        for args in [&["ssh", "exit", "3"][..], &["btop"][..]] {
            host.remove("ssh-calls");
            let output = host
                .command(args)
                .env("MOCK_SSH_EXIT", "3")
                .stdin(Stdio::null())
                .checked(3);
            assert!(output.stdout.is_empty());
            if let Some(progress) = progress {
                output.stderr_contains(progress);
                ssh_timing(&output.stderr);
            } else {
                assert!(output.stderr.is_empty(), "{output:?}");
            }
            assert_eq!(host.read("ssh-calls").lines().count(), connections);
        }
    }
}

use crate::{build_check, checked_output, host::CommandExt};
use anyhow::{bail, ensure, Context, Result};
use clap::{Parser, Subcommand};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::IsTerminal,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

mod config;
mod resources;
mod status;

fn fields<'a>(output: &'a str, prefix: &'a str) -> impl Iterator<Item = &'a str> {
    output
        .lines()
        .filter_map(move |line| line.trim().strip_prefix(prefix))
}

pub fn runtime_expression(idle_timeout: u32) -> Result<String> {
    let revision = crate::build_check::nixpkgs_revision()?;
    let lock: serde_json::Value = serde_json::from_str(include_str!("../../flake.lock"))?;
    let unstable = option_env!("TEKA_VM_NIXPKGS_UNSTABLE_REV")
        .or_else(|| lock["nodes"]["nixpkgs-unstable"]["locked"]["rev"].as_str())
        .context("unstable Nixpkgs revision missing from embedded flake.lock")?;
    let idle_timeout = if idle_timeout == 0 {
        "null".into()
    } else {
        idle_timeout.to_string()
    };
    Ok(format!(
        "({}) {{ nixpkgsRev = \"{revision}\"; nixpkgsUnstableRev = \"{unstable}\"; idleTimeout = {idle_timeout}; }}",
        include_str!("build_vm.nix")
    ))
}

const SSH_PORT: &str = "31023";
const LABEL: &str = "com.teka.build-vm";

#[derive(Parser)]
#[command(
    name = "teka-build-vm",
    version,
    about = "Linux build VM for Apple silicon"
)]
struct Cli {
    #[command(subcommand)]
    command: VmCommand,
}

#[derive(Subcommand)]
enum VmCommand {
    /// Initialize, register the host builder (sudo), and enable autostart at login
    Init {
        /// Stop the service and delete its disk, keys, logs and settings before setup
        #[arg(long)]
        reset: bool,
    },
    /// Enable the VM service and wait until guest SSH is ready
    Start(resources::Overrides),
    /// Disable and stop the VM service, interrupting any active clients
    Stop,
    /// Open an SSH session or run a command in the VM (may boot the guest)
    Ssh {
        /// SSH options followed by an optional remote command and arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Open the VM resource monitor (may boot the guest)
    Btop,
    /// Show service status and brief usage for an active guest
    Status,
    /// Check runtime, host registration, connectivity, and OOM kills (may boot the guest)
    Check,
}

// The same executable can be installed under the plugin name by portable init.
pub fn main_entry() -> i32 {
    let args = Cli::parse();
    let result = (|| {
        let vm = Vm::new()?;
        match args.command {
            VmCommand::Init { reset } => {
                let state = vm.lock()?;
                state.lock()?;
                if reset {
                    vm.stop_locked()?;
                    build_check::timed(
                        "Old VM state",
                        "Removing old VM state...",
                        "removed",
                        || {
                            if vm.agent.exists() {
                                fs::remove_file(&vm.agent).context("remove old VM LaunchAgent")?;
                            }
                            fs::remove_dir_all(&vm.root).context("remove old VM state directory")
                        },
                    )?;
                }
                let settings = vm.settings(resources::Overrides::default())?;
                vm.initialize_locked(&settings)?;
                vm.register()?;
                vm.pin_host_key()?;
                vm.launch_locked(&settings)?;
            }
            VmCommand::Start(overrides) => {
                let state = vm.lock()?;
                state.lock()?;
                let settings = vm.settings(overrides)?;
                if vm.initialize_locked(&settings)? {
                    vm.register()?;
                    vm.pin_host_key()?;
                }
                vm.launch_locked(&settings)?;
                if !vm.check_ssh() {
                    return Ok(1);
                }
            }
            VmCommand::Stop => {
                vm.stop()?;
            }
            VmCommand::Ssh { args } => return vm.ssh_session(&args),
            VmCommand::Btop => {
                vm.initialized()?;
                let btop = fs::read_to_string(vm.root.join("runtime/share/teka-build-vm/btop"))?;
                return vm.ssh_session(&[btop.trim().into()]);
            }
            VmCommand::Status => return vm.status(),
            VmCommand::Check => return vm.check(),
        };
        Ok(0)
    })();
    match result {
        Ok(code) => code,
        Err(error) => {
            if !error.is::<build_check::BuildFailed>() {
                build_check::failure("Linux build VM", &error);
            }
            1
        }
    }
}

/// Select a builder only during init; subsequent builds use Nix configuration.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn needed(nix: &std::path::Path) -> Result<bool> {
    match std::env::var("TEKA_BUILD_VM").as_deref() {
        Ok("always") => return Ok(true),
        Ok("never") => return Ok(false),
        Ok("auto") | Err(std::env::VarError::NotPresent) => {}
        _ => bail!("TEKA_BUILD_VM must be auto, always, or never"),
    }
    let config = checked_output(Command::new(nix).args(["config", "show", "--json"]))
        .context("read Nix builders configuration")?;
    let config: serde_json::Value = serde_json::from_str(&config)?;
    let builders = config["builders"]["value"].as_str().unwrap_or_default();
    needs_vm(builders)
}

fn needs_vm(builders: &str) -> Result<bool> {
    let rows = config::rows(builders)?;
    Ok(rows.iter().any(|row| config::managed(row))
        || !rows.iter().any(|row| {
            row.split_whitespace()
                .nth(1)
                .is_some_and(|systems| systems.split(',').any(|system| system.ends_with("-linux")))
        }))
}

struct Vm {
    root: PathBuf,
    agent: PathBuf,
    domain: String,
}

impl Vm {
    fn new() -> Result<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").context("HOME must be set")?);
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        let root = data.join("teka-build-vm");
        ensure!(
            root.is_absolute() && home.is_absolute(),
            "VM state and home directories must be absolute"
        );
        fs::create_dir_all(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let uid = fs::metadata(&root)?.uid();
        Ok(Self {
            root,
            agent: home.join(format!("Library/LaunchAgents/{LABEL}.plist")),
            domain: format!("gui/{uid}"),
        })
    }

    fn lock(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            // Reset removes the state directory while this lock is held.
            .open(self.root.with_extension("lock"))?)
    }

    fn launchctl(&self) -> Command {
        Command::new(
            crate::plugins::on_path("launchctl").unwrap_or_else(|| "/bin/launchctl".into()),
        )
    }

    fn service(&self) -> String {
        format!("{}/{LABEL}", self.domain)
    }

    fn monitor(&self) -> Result<Option<String>> {
        let output = self
            .launchctl()
            .args(["print", &self.service()])
            .output()
            .context("query VM LaunchAgent")?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
    }

    fn loaded(&self) -> Result<bool> {
        Ok(self.monitor()?.is_some())
    }

    fn require_stopped(&self, change: &str) -> Result<()> {
        ensure!(!self.loaded()?, "stop the VM service before {change}");
        Ok(())
    }

    fn initialize_locked(&self, settings: &resources::Settings) -> Result<bool> {
        fs::create_dir_all(&self.root)?;
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        let keys = self.root.join("keys");
        fs::create_dir_all(&keys)?;
        fs::set_permissions(&keys, fs::Permissions::from_mode(0o700))?;
        for name in ["builder_ed25519", "ssh_host_ed25519_key"] {
            let key = keys.join(name);
            if !key.exists() {
                self.require_stopped("replacing its SSH keys")?;
                Command::new("ssh-keygen")
                    .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                    .arg(&key)
                    .check_status("SSH key generation failed")
                    .context("generate VM SSH key")?;
            }
            ensure!(
                key.with_extension("pub").is_file(),
                "VM public key is missing"
            );
        }
        let host_key_changed = fs::read_to_string(self.root.join("known_hosts"))
            .ok()
            .as_deref()
            != Some(self.known_hosts()?.as_str());
        if host_key_changed {
            self.require_stopped("updating its SSH host key")?;
        }
        let expression = self.root.join("runtime.nix");
        let runtime = runtime_expression(settings.idle_timeout_seconds)?;
        if !self.root.join("runtime/bin/teka-build-vm-runner").exists()
            || fs::read_to_string(&expression).ok().as_deref() != Some(runtime.as_str())
        {
            self.require_stopped("updating its runtime")?;
            build_check::timed("VM runtime", "Preparing VM runtime...", "ready", || {
                build_check::run_command(
                    Command::new(config::nix())
                        .args(["build", "--impure", "--expr", &runtime])
                        .arg("--out-link")
                        .arg(self.root.join("runtime"))
                        .stdin(Stdio::null()),
                    "Nix",
                    false,
                )?;
                fs::write(&expression, runtime).context("save VM runtime expression")
            })?;
        }
        fs::create_dir_all(self.root.join("vm"))?;
        let disk = self.root.join("vm/nixos.qcow2");
        if !disk.exists() {
            self.require_stopped("creating its disk")?;
            File::create(disk)?.set_len(64 * 1024 * 1024 * 1024)?;
        }
        Ok(host_key_changed)
    }

    fn known_hosts(&self) -> Result<String> {
        let host_key = fs::read_to_string(self.root.join("keys/ssh_host_ed25519_key.pub"))?;
        Ok(format!("[127.0.0.1]:{SSH_PORT} {}\n", host_key.trim()))
    }

    fn pin_host_key(&self) -> Result<()> {
        fs::write(self.root.join("known_hosts"), self.known_hosts()?)?;
        Ok(())
    }

    fn launch_locked(&self, settings: &resources::Settings) -> Result<()> {
        let started = if !self.loaded()? {
            build_check::progress("Starting VM service...");
            Some(Instant::now())
        } else {
            None
        };
        let vm_log = build_check::display_path(&self.root.join("vm.log"));
        let result = (|| {
            if !self.loaded()? {
                let log = self.root.join("vm.log").to_string_lossy().into_owned();
                let plist = serde_json::json!({
                    "Label": LABEL,
                    "ProgramArguments": [self.root.join("runtime/bin/teka-build-vm-runner").to_string_lossy()],
                    "RunAtLoad": true,
                    "KeepAlive": true,
                    "ExitTimeOut": 45,
                    "WorkingDirectory": self.root.join("vm").to_string_lossy(),
                    "StandardOutPath": log,
                    "StandardErrorPath": log,
                    "EnvironmentVariables": {
                        "PATH": std::env::var("PATH").unwrap_or_default(),
                        "HOME": std::env::var("HOME")?,
                        "VZVM_STATE_DIR": self.root.join("vm").to_string_lossy(),
                        "KEYS": self.root.join("keys").to_string_lossy(),
                        "NIX_SSL_CERT_FILE": std::env::var("NIX_SSL_CERT_FILE")
                            .unwrap_or_else(|_| "/etc/ssl/cert.pem".into()),
                        "TEKA_VM_CPUS": settings.cpus.to_string(),
                        "TEKA_VM_MEMORY_MIB": settings.memory_mib.to_string(),
                    },
                });
                fs::create_dir_all(
                    self.agent
                        .parent()
                        .context("LaunchAgents directory missing")?,
                )?;
                let temporary = tempfile::NamedTempFile::new_in(self.agent.parent().unwrap())?;
                serde_json::to_writer(temporary.as_file(), &plist)?;
                checked_output(
                    Command::new("/usr/bin/plutil")
                        .args(["-convert", "xml1"])
                        .arg(temporary.path()),
                )
                .context("encode VM LaunchAgent plist")?;
                temporary.persist(&self.agent)?;
            }
            checked_output(self.launchctl().args(["enable", &self.service()]))
                .context("enable VM LaunchAgent")?;
            if !self.loaded()? {
                checked_output(
                    self.launchctl()
                        .args(["bootstrap", &self.domain])
                        .arg(&self.agent),
                )
                .context("load VM LaunchAgent (requires a logged-in GUI session)")?;
            }
            for _ in 0..20 {
                if let Some(monitor) = self.monitor()? {
                    if status::service_running(&monitor) {
                        return Ok(());
                    }
                    if let Some(code) = fields(&monitor, "last exit code = ")
                        .find_map(|code| code.parse::<i32>().ok())
                    {
                        bail!("VM runtime exited with {code}; see {vm_log}");
                    }
                }
                thread::sleep(Duration::from_millis(250));
            }
            bail!("VM service did not start; see {vm_log}")
        })();
        build_check::finish(
            "Launchd service",
            result,
            |()| "running; enabled at login".into(),
            started,
        )
    }

    fn stop(&self) -> Result<()> {
        let state = self.lock()?;
        state.lock()?;
        self.stop_locked()
    }

    fn stop_locked(&self) -> Result<()> {
        let monitor = self.monitor()?;
        let started = monitor.as_ref().map(|_| {
            build_check::progress("Stopping VM service...");
            Instant::now()
        });
        let result = (|| {
            checked_output(self.launchctl().args(["disable", &self.service()]))
                .context("disable VM LaunchAgent")?;
            if let Some(monitor) = monitor {
                checked_output(self.launchctl().args(["bootout", &self.service()]))
                    .context("unload VM LaunchAgent")?;
                if let Some(pid) = fields(&monitor, "pid = ")
                    .find_map(|pid| pid.parse::<u32>().ok().filter(|pid| *pid > 0))
                {
                    let deadline = Instant::now() + Duration::from_secs(60);
                    while Command::new("/bin/kill")
                        .args(["-0", &pid.to_string()])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()?
                        .success()
                    {
                        ensure!(
                            Instant::now() < deadline,
                            "VM service did not exit within 60s; see {}",
                            build_check::display_path(&self.root.join("vm.log"))
                        );
                        thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            Ok(())
        })();
        build_check::finish(
            "Launchd service",
            result,
            |()| "stopped; disabled at login".into(),
            started,
        )
    }

    fn ssh(&self) -> Command {
        self.ssh_with_timeout(180)
    }

    fn ssh_with_timeout(&self, timeout: u64) -> Command {
        let mut ssh = Command::new("ssh");
        ssh.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            &format!("ConnectTimeout={timeout}"),
            "-o",
            "StrictHostKeyChecking=yes",
        ])
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile=\"{}\"",
            self.root
                .join("known_hosts")
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        ))
        .args(["-p", SSH_PORT, "-i"])
        .arg(self.root.join("keys/builder_ed25519"))
        .arg("builder@127.0.0.1");
        ssh
    }

    fn ssh_session(&self, args: &[OsString]) -> Result<i32> {
        self.initialized()?;
        ensure!(
            self.loaded()?,
            "VM service is stopped; run `teka build vm start`"
        );
        let state = self.guest_state();
        if state != Some(true) {
            self.connect(state)?;
        }
        let mut command = self.ssh();
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            command.arg("-t");
        }
        let status = command.args(args).status().context("run VM SSH session")?;
        use std::os::unix::process::ExitStatusExt;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_linux_builders_in_inline_rows_and_machine_files() {
        assert!(needs_vm("").unwrap());
        assert!(needs_vm("ssh://mac aarch64-darwin - 4").unwrap());
        assert!(needs_vm("# ssh://linux aarch64-linux - 4").unwrap());
        assert!(
            !needs_vm("ssh://mac aarch64-darwin; ssh://linux aarch64-linux,x86_64-linux - 4")
                .unwrap()
        );
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), "# builders\nssh://linux aarch64-linux - 4\n").unwrap();
        assert!(!needs_vm(&format!("@{}", file.path().display())).unwrap());
        assert!(needs_vm("@/missing/teka-test-builders").unwrap());
    }
}

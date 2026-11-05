use super::{fields, resources::Settings, Vm, LABEL};
use crate::{
    build_check::{display_path, note, report, timed},
    checked_output,
};
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::{
    fs,
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

const METRICS: &str = r#"set -eu
export LC_ALL=C
awk '{printf "{\"uptime\":%s,", $1}' /proc/uptime
awk '{printf "\"load\":[%s,%s,%s],", $1, $2, $3}' /proc/loadavg
awk '/^processor[[:space:]]*:/ {count++} END {printf "\"cpus\":%d,", count}' /proc/cpuinfo
awk '/^MemTotal:/ {total=$2} /^MemAvailable:/ {available=$2} END {printf "\"memory\":[%.0f,%.0f],", (total-available)*1024, total*1024}' /proc/meminfo
disk=$(df -B1 --output=used,size /nix/.rw-store)
printf '%s\n' "$disk" | awk 'NR == 2 {printf "\"disk\":[%s,%s]}\n", $1, $2}'
"#;

#[derive(Deserialize)]
struct Metrics {
    uptime: f64,
    load: [f64; 3],
    cpus: u32,
    memory: [u64; 2],
    disk: [u64; 2],
}

fn size(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    let number = format!("{value:.1}");
    format!("{} {unit}", number.trim_end_matches(".0"))
}

impl Metrics {
    fn print(&self) {
        println!(
            "Uptime: {:.0} min; load: {:.2} {:.2} {:.2}",
            self.uptime / 60.0,
            self.load[0],
            self.load[1],
            self.load[2]
        );
        println!("CPUs: {}", self.cpus);
        println!("RAM: {} / {}", size(self.memory[0]), size(self.memory[1]));
        println!("Disk: {} / {}", size(self.disk[0]), size(self.disk[1]));
    }
}

pub(super) fn service_running(monitor: &str) -> bool {
    fields(monitor, "state = ").any(|state| state == "running")
}

fn guest_process(domain: &str, pid: u32) -> Result<bool> {
    let mut lines = domain
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    ensure!(
        lines.next() == Some(format!("pid/{pid} = {{").as_str()),
        "unrecognized launchd PID domain"
    );
    let mut depth = 1usize;
    let mut services = false;
    let mut found = false;
    let mut active = false;
    for line in lines {
        if line == "}" {
            depth = depth
                .checked_sub(1)
                .context("unbalanced launchd PID domain")?;
            if depth == 1 {
                services = false;
            }
        } else if line.ends_with('{') {
            ensure!(!services, "unrecognized launchd service row");
            if depth == 1 && line == "services = {" {
                ensure!(!found, "duplicate launchd services block");
                services = true;
                found = true;
            }
            depth += 1;
        } else if services {
            let row: Vec<_> = line.split_whitespace().collect();
            ensure!(row.len() == 3, "unrecognized launchd service row");
            let pid: u32 = row[0].parse().context("invalid launchd service PID")?;
            let name = row[2];
            active |= pid > 0
                && (name == "com.apple.Virtualization.VirtualMachine"
                    || name
                        .strip_prefix("com.apple.Virtualization.VirtualMachine.")
                        .is_some_and(|suffix| !suffix.is_empty()));
        } else if depth == 1 && line == "properties = slain" {
            bail!("monitor PID domain is no longer active");
        }
    }
    ensure!(depth == 0 && found, "incomplete launchd services block");
    Ok(active)
}

impl Vm {
    pub(super) fn status(&self) -> Result<i32> {
        let monitor = self.monitor()?;
        let running = monitor.as_deref().is_some_and(service_running);
        let startup = if self.agent.is_file() {
            let output = checked_output(self.launchctl().args(["print-disabled", &self.domain]))?;
            let label = format!("\"{LABEL}\"");
            let disabled =
                output
                    .lines()
                    .filter_map(|line| line.split_once("=>"))
                    .any(|(name, value)| {
                        name.trim() == label && matches!(value.trim(), "true" | "disabled")
                    });
            if disabled {
                "disabled at login"
            } else {
                "enabled at login"
            }
        } else {
            "not installed"
        };
        let activity = if running { "running" } else { "stopped" };
        println!("Launchd: {startup}; service {activity}");
        let unavailable = |state: &str, error: &anyhow::Error| {
            println!(
                "VM: {state}; guest details unavailable ({})",
                format!("{error:#}").replace('\n', "; ")
            );
        };
        match monitor.as_deref().filter(|_| running) {
            None => println!("VM: stopped"),
            Some(monitor) => match self.guest_process(monitor) {
                Ok(false) => println!("VM: waiting for connection; guest details unavailable"),
                Ok(true) => match self.guest_metrics() {
                    Ok(metrics) => {
                        println!("VM: running; SSH ready");
                        metrics.print();
                        return Ok(0);
                    }
                    Err(error) => unavailable("running", &error),
                },
                Err(error) => unavailable("unknown", &error),
            },
        }
        self.configured_resources();
        Ok(0)
    }

    fn configured_resources(&self) {
        match fs::read(self.root.join("config.json")) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(settings) => {
                    println!("CPUs: {} configured", settings.cpus);
                    println!(
                        "RAM: {} configured",
                        size(settings.memory_mib.saturating_mul(1024 * 1024))
                    );
                    if settings.idle_timeout_seconds == 0 {
                        println!("Idle timeout: disabled");
                    } else if settings.idle_timeout_seconds % 60 == 0 {
                        println!("Idle timeout: {} min", settings.idle_timeout_seconds / 60);
                    } else {
                        println!("Idle timeout: {} s", settings.idle_timeout_seconds);
                    }
                }
                Err(error) => println!("Configuration: unavailable ({error})"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => println!("Configuration: unavailable ({error})"),
        }
        if let Ok(metadata) = fs::metadata(self.root.join("vm/nixos.qcow2")) {
            if metadata.is_file() {
                println!("Disk: {} capacity", size(metadata.len()));
            }
        }
    }

    fn guest_process(&self, monitor: &str) -> Result<bool> {
        let pid = fields(monitor, "pid = ")
            .find_map(|pid| pid.parse::<u32>().ok().filter(|pid| *pid > 0))
            .context("monitor PID unavailable")?;
        // TODO: Replace XPC service inference when vzvm exposes guest state.
        let domain = checked_output(self.launchctl().args(["print", &format!("pid/{pid}")]))
            .context("cannot inspect monitor XPC services")?;
        guest_process(&domain, pid)
    }

    fn guest_metrics(&self) -> Result<Metrics> {
        let stdout = tempfile::NamedTempFile::new()?;
        let stderr = tempfile::NamedTempFile::new()?;
        let mut child = self
            .ssh_with_timeout(2)
            .args(["-o", "ConnectionAttempts=1", "-T"])
            .arg(METRICS)
            .stdin(Stdio::null())
            .stdout(stdout.reopen()?)
            .stderr(stderr.reopen()?)
            .spawn()
            .context("start guest status probe")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25));
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = result.context("wait for guest status probe")?;
                    bail!("SSH status probe timed out after 5s");
                }
            }
        };
        ensure!(
            status.success(),
            "SSH status probe failed ({status}): {}",
            fs::read_to_string(stderr.path())?.trim()
        );
        serde_json::from_reader(stdout.reopen()?).context("read guest metrics")
    }

    pub(super) fn guest_state(&self) -> Option<bool> {
        self.monitor()
            .ok()
            .flatten()
            .filter(|monitor| service_running(monitor))
            .and_then(|monitor| self.guest_process(&monitor).ok())
    }

    pub(super) fn connect(&self, state: Option<bool>) -> Result<()> {
        let message = if state == Some(false) {
            "Starting VM..."
        } else {
            "Connecting to guest..."
        };
        let result = timed("SSH", message, "reachable", || {
            checked_output(
                self.ssh()
                    .args(["-o", "ConnectionAttempts=180"])
                    .arg("true")
                    .stdin(Stdio::null()),
            )
            .map(|_| ())
        });
        if result.is_err() {
            eprintln!();
            note(format!(
                "Check the VM log: {}",
                display_path(&self.root.join("vm.log"))
            ));
        }
        result
    }

    pub(super) fn check_ssh(&self) -> bool {
        self.connect(self.guest_state()).is_ok()
    }

    pub(super) fn check(&self) -> Result<i32> {
        let mut healthy = report("VM runtime and SSH credentials", self.initialized(), |()| {
            "available".into()
        });
        healthy &= report("System remote builder", self.registration(), |detail| {
            detail.clone()
        });
        let monitor = self.monitor()?;
        let running = monitor.as_deref().is_some_and(service_running);
        healthy &= report(
            "Launchd service",
            if running {
                Ok(())
            } else {
                Err(anyhow::anyhow!("stopped"))
            },
            |()| "running".into(),
        );
        if running {
            let ssh = self.check_ssh();
            healthy &= ssh;
            if ssh {
                healthy &= report(
                    "Guest Nix daemon",
                    checked_output(
                        self.ssh()
                            .arg("nix --extra-experimental-features nix-command store ping --json")
                            .stdin(Stdio::null()),
                    ),
                    |_| "available".into(),
                );
            } else {
                note("Guest checks: skipped");
            }
        } else {
            note("Live checks: skipped (VM service stopped)");
            eprintln!();
            note("Run `teka build vm start` and retry");
        }
        Ok(if healthy { 0 } else { 1 })
    }

    pub(super) fn initialized(&self) -> Result<()> {
        ensure!(
            self.root.join("runtime/bin/teka-build-vm-runner").is_file()
                && self.root.join("keys/builder_ed25519").is_file()
                && self.root.join("keys/ssh_host_ed25519_key").is_file()
                && self.root.join("known_hosts").is_file(),
            "runtime or SSH credentials missing; run `teka build vm init`"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{guest_process, size, Metrics, METRICS};

    #[test]
    fn resource_sizes_use_explicit_binary_units() {
        for (bytes, expected) in [
            (0, "0 B"),
            (1023, "1023 B"),
            (1024, "1 KiB"),
            (8808038, "8.4 MiB"),
            (598 * 1024 * 1024, "598 MiB"),
            (11914 * 1024 * 1024, "11.6 GiB"),
            (64 * 1024 * 1024 * 1024, "64 GiB"),
        ] {
            assert_eq!(size(bytes), expected);
        }
    }

    #[test]
    fn guest_metrics_script_collects_numeric_byte_counts() {
        let directory = tempfile::tempdir().unwrap();
        let mut script = METRICS.to_owned();
        for (name, contents) in [
            ("uptime", "123.45 100.00\n"),
            ("loadavg", "0.50 0.25 0.10 1/100 42\n"),
            ("cpuinfo", "processor : 0\nprocessor : 1\n"),
            (
                "meminfo",
                "MemTotal: 8388608 kB\nMemAvailable: 7340032 kB\n",
            ),
        ] {
            let path = directory.path().join(name);
            std::fs::write(&path, contents).unwrap();
            script = script.replace(&format!("/proc/{name}"), &format!("\"{}\"", path.display()));
        }
        script = script.replace(
            "df -B1 --output=used,size /nix/.rw-store",
            "printf 'Used 1B-blocks\\n8808038 67645734912\\n'",
        );
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let metrics: Metrics = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(metrics.uptime, 123.45);
        assert_eq!(metrics.load, [0.50, 0.25, 0.10]);
        assert_eq!(metrics.cpus, 2);
        assert_eq!(metrics.memory, [1073741824, 8589934592]);
        assert_eq!(metrics.disk, [8808038, 67645734912]);
    }

    fn domain(services: &str) -> String {
        format!(
            "pid/91503 = {{\n environment = {{\n services = {{\n 111 - com.apple.Virtualization.VirtualMachine.fake\n }}\n }}\n services = {{\n{services}\n }}\n endpoints = {{\n 222 - com.apple.Virtualization.VirtualMachine.fake\n }}\n}}"
        )
    }

    #[test]
    fn guest_process_parses_running_services_and_malformed_domains() {
        for services in [
            "95029 - com.apple.Virtualization.VirtualMachine.3CFF0562-71AA-42FF-83ED-5ACAF8958EAF",
            "0 - com.apple.Virtualization.VirtualMachine\n95029 - com.apple.Virtualization.VirtualMachine.live",
            "95029 - com.apple.Virtualization.VirtualMachine",
            "95029 - com.apple.Virtualization.VirtualMachine.one\n95030 - com.apple.Virtualization.VirtualMachine.two",
        ] {
            assert!(guest_process(&domain(services), 91503).unwrap());
        }
        for services in [
            "",
            "0 - com.apple.Virtualization.VirtualMachine",
            "0 - com.apple.Virtualization.VirtualMachine.idle",
            "123 - com.apple.Virtualization.VirtualMachineOther\n124 - com.apple.Virtualization.VirtualMachine.",
        ] {
            assert!(!guest_process(&domain(services), 91503).unwrap());
        }
        assert!(guest_process(&domain(""), 85591).is_err());
        for output in [
            "pid/91503 = {\n}",
            "pid/91503 = {\nservices = {\n}",
            "pid/91503 = {\nservices = {\n}\nproperties = slain\n}",
            "pid/91503 = {\nservices = {\n}\nservices = {\n}\n}",
        ] {
            assert!(guest_process(output, 91503).is_err());
        }
        for row in [
            "invalid - com.apple.Virtualization.VirtualMachine.live",
            "-1 - com.apple.Virtualization.VirtualMachine.live",
            "95029 com.apple.Virtualization.VirtualMachine.live",
            "95029 - com.apple.Virtualization.VirtualMachine.live extra",
        ] {
            assert!(guest_process(&domain(row), 91503).is_err());
        }
    }
}

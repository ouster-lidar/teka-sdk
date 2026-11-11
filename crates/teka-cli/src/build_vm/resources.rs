use super::Vm;
use crate::{checked_output, plugins::on_path};
use anyhow::{ensure, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[derive(Args, Default)]
pub(super) struct Overrides {
    /// Virtual CPUs (default: host logical CPUs minus two)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    cpus: Option<u32>,
    /// RAM in GiB, or with an explicit MiB/GiB suffix (default: half the host RAM)
    #[arg(long = "memory", value_name = "SIZE", value_parser = parse_memory)]
    memory_mib: Option<u64>,
    /// Idle shutdown in seconds; zero keeps the guest always on (default: 1800)
    #[arg(long = "idle-timeout", value_name = "SECONDS", value_parser = clap::value_parser!(u32).range(..=604800))]
    idle_timeout_seconds: Option<u32>,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Settings {
    pub(super) cpus: u32,
    #[serde(rename = "memoryMiB")]
    pub(super) memory_mib: u64,
    pub(super) idle_timeout_seconds: u32,
}

fn parse_memory(value: &str) -> Result<u64, String> {
    let value = value.to_ascii_lowercase();
    let (number, factor) = if let Some(number) = value.strip_suffix("mib") {
        (number, 1)
    } else {
        (value.strip_suffix("gib").unwrap_or(&value), 1024)
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|size| size.checked_mul(factor))
        .filter(|size| *size > 0)
        .ok_or_else(|| "use a positive integer GiB size or an explicit MiB/GiB suffix".into())
}

fn defaults(host_cpus: u32, host_memory: u64) -> Settings {
    Settings {
        cpus: host_cpus.saturating_sub(2).max(1),
        memory_mib: host_memory / 2,
        idle_timeout_seconds: 1800,
    }
}

fn host_capacity() -> Result<(u32, u64)> {
    let sysctl = on_path("sysctl").unwrap_or_else(|| "/usr/sbin/sysctl".into());
    let output = checked_output(Command::new(sysctl).args(["-n", "hw.ncpu", "hw.memsize"]))?;
    let mut values = output.lines();
    let cpus = values
        .next()
        .context("host CPU count missing")?
        .parse()
        .context("invalid host CPU count")?;
    let memory: u64 = values
        .next()
        .context("host RAM size missing")?
        .parse()
        .context("invalid host RAM size")?;
    ensure!(cpus > 0, "host CPU count must be positive");
    ensure!(
        memory >= 2 * 1024 * 1024,
        "host RAM must provide at least 1 MiB to the VM"
    );
    Ok((cpus, memory / 1024 / 1024))
}

impl Vm {
    pub(super) fn settings(&self, overrides: Overrides) -> Result<Settings> {
        let path = self.root.join("config.json");
        let previous: Option<Settings> = match fs::read(&path) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes).context("read VM settings")?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("read VM settings"),
        };
        let (host_cpus, host_memory) = host_capacity()?;
        let mut settings = previous
            .clone()
            .unwrap_or_else(|| defaults(host_cpus, host_memory));
        settings.cpus = overrides.cpus.unwrap_or(settings.cpus);
        settings.memory_mib = overrides.memory_mib.unwrap_or(settings.memory_mib);
        settings.idle_timeout_seconds = overrides
            .idle_timeout_seconds
            .unwrap_or(settings.idle_timeout_seconds);
        ensure!(
            settings.cpus > 0 && settings.cpus <= host_cpus,
            "VM CPUs must be between 1 and {host_cpus}"
        );
        ensure!(
            settings.memory_mib > 0 && settings.memory_mib <= host_memory,
            "VM RAM must be between 1 and {host_memory} MiB"
        );
        ensure!(
            settings.idle_timeout_seconds <= 604800,
            "VM idle timeout must be between 0 and 604800 seconds"
        );
        if previous.as_ref() != Some(&settings) {
            self.require_stopped("changing its settings")?;
            fs::create_dir_all(&self.root)?;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
            let temporary = tempfile::NamedTempFile::new_in(&self.root)?;
            serde_json::to_writer(temporary.as_file(), &settings)?;
            temporary.persist(path)?;
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_leave_two_cpus_and_half_the_ram_for_the_host() {
        for (cpus, memory, expected) in [
            (8, 16384, (6, 8192)),
            (1, 1024, (1, 512)),
            (2, 1024, (1, 512)),
            (18, 24576, (16, 12288)),
        ] {
            let settings = defaults(cpus, memory);
            assert_eq!((settings.cpus, settings.memory_mib), expected);
            assert_eq!(settings.idle_timeout_seconds, 1800);
        }
    }

    #[test]
    fn memory_sizes_default_to_gib_and_accept_explicit_units() {
        for (value, expected) in [("16GiB", 16384), ("512MiB", 512), ("8", 8192)] {
            assert_eq!(parse_memory(value).unwrap(), expected);
        }
        for value in ["0", "-1GiB", "1.5GiB", "garbage", "18446744073709551615GiB"] {
            assert!(parse_memory(value).is_err());
        }
    }
}

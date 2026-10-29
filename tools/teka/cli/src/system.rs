use crate::device::DeviceClient;
use anyhow::{bail, ensure, Context, Result};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Subcommand)]
pub enum SystemCommand {
    /// Print device system status JSON
    Status(StatusOptions),
    /// Upload and apply a system update file
    Update(UpdateOptions),
}

#[derive(Clone, Debug, Args)]
pub struct StatusOptions {
    /// URL of the Teka device
    #[arg(short = 'H', long, env = "TEKA_HOST", value_name = "URL")]
    pub host: String,
}

#[derive(Clone, Debug, Args)]
pub struct UpdateOptions {
    /// URL of the Teka device
    #[arg(short = 'H', long, env = "TEKA_HOST", value_name = "URL")]
    pub host: String,
    /// System update image
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    pub file: PathBuf,
    /// Apply without rebooting the device
    #[arg(long)]
    pub no_reboot: bool,
    /// Wait for a rebooting update to commit
    #[arg(long)]
    pub wait: bool,
}

fn env_u64(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Ok(value) => value.parse().with_context(|| format!("invalid {name}")),
        Err(_) => Ok(default),
    }
}

pub fn poll_interval() -> Duration {
    Duration::from_millis(env_u64("TEKA_UPDATE_POLL_MS", 1000).unwrap_or(1000))
}

pub fn update_deadline() -> Result<Instant> {
    let secs = env_u64("TEKA_UPDATE_WAIT_SEC", 900)?;
    Ok(Instant::now() + Duration::from_secs(secs))
}

fn fetch_status(host: &str) -> Result<Value> {
    let client = DeviceClient::new(host)?;
    client.authenticate()?;
    client.jetstream_status()
}

fn status_version(status: &Value) -> &str {
    status["version"].as_str().unwrap_or("")
}

fn commit_state(status: &Value) -> &str {
    status["update"]["commit_state"].as_str().unwrap_or("")
}

pub fn apply(client: &DeviceClient, file: &Path, reboot: bool) -> Result<Value> {
    client.sysupdate_apply(file, reboot)
}

pub fn wait_for_commit(host: &str, before_version: &str, deadline: Instant) -> Result<Value> {
    let poll = poll_interval();
    let mut changed_version = loop {
        if Instant::now() >= deadline {
            bail!("timed out waiting for system update to commit");
        }
        match fetch_status(host) {
            Ok(status) => {
                if status_version(&status) != before_version {
                    break true;
                }
            }
            Err(_) => break false,
        }
        thread::sleep(poll);
    };

    // Services restart while the device boots, so failed polls are retried until the deadline.
    loop {
        if Instant::now() >= deadline {
            bail!("timed out waiting for system update to commit");
        }
        let Ok(status) = fetch_status(host) else {
            thread::sleep(poll);
            continue;
        };
        changed_version |= status_version(&status) != before_version;
        match commit_state(&status) {
            "COMMITTED" | "NONE" => {
                if status_version(&status) != before_version {
                    return Ok(status);
                }
                if changed_version {
                    bail!("system update rolled back");
                }
                thread::sleep(poll);
            }
            "ABORTED_UNHEALTHY" | "FAILED_SET_DEFAULT" => {
                let units = failed_units(&status);
                bail!("system update failed ({units})");
            }
            // PENDING, or a status without `update` while services start.
            _ => thread::sleep(poll),
        }
    }
}

fn failed_units(status: &Value) -> String {
    status["update"]["failed_units"]
        .as_array()
        .map(|units| {
            units
                .iter()
                .filter_map(|u| u.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "no failed_units listed".into())
}

pub fn run_status(args: StatusOptions) -> Result<()> {
    let client = DeviceClient::new(&args.host)?;
    client.authenticate()?;
    let status = client.jetstream_status()?;
    println!("{}", serde_json::to_string(&status)?);
    Ok(())
}

pub fn run_update(args: UpdateOptions) -> Result<()> {
    let manifest = teka_tpk::read_single(&args.file, teka_tpk::ComponentType::Os)?;
    let file = args
        .file
        .canonicalize()
        .with_context(|| format!("open {}", args.file.display()))?;
    let client = DeviceClient::new(&args.host)?;
    client.authenticate()?;
    let architecture = client.architecture()?;
    ensure!(
        manifest.arch.as_str() == architecture,
        "{} is for {}; device is {architecture}",
        args.file.display(),
        manifest.arch.as_str()
    );
    let before_version = if args.wait {
        Some(status_version(&client.jetstream_status()?).to_owned())
    } else {
        None
    };
    let reboot = !args.no_reboot;
    let response = apply(&client, &file, reboot)?;
    if args.wait && response["result"].as_str() == Some("REBOOTING") {
        let before = before_version.context("missing pre-update version")?;
        let final_status = wait_for_commit(&args.host, &before, update_deadline()?)?;
        println!("{}", serde_json::to_string(&final_status)?);
    }
    Ok(())
}

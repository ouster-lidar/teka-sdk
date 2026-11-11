use crate::device::DeviceClient;
use anyhow::{ensure, Context, Result};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
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

fn status_version(status: &Value) -> &str {
    status["version"].as_str().unwrap_or("")
}

pub fn apply(client: &DeviceClient, file: &Path, reboot: bool) -> Result<Value> {
    client.sysupdate_apply(file, reboot)
}

pub fn wait_for_commit(host: &str, before_version: &str, deadline: Instant) -> Result<Value> {
    crate::device::authentication(crate::client::wait_for_commit(
        host,
        crate::device::password().as_deref(),
        before_version,
        deadline,
        poll_interval(),
    ))
}

pub fn run_status(args: StatusOptions) -> Result<()> {
    let client = DeviceClient::new(&args.host)?;
    client.authenticate()?;
    let status = client.teka_status()?;
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
        Some(status_version(&client.teka_status()?).to_owned())
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

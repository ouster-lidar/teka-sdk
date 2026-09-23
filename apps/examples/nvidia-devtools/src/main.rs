//! NVIDIA devtools app — SSH on :2223 plus MC web UI for authorized_keys.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use axum::Router;
use clap::Parser;
use nvidia_devtools::api;
use nvidia_devtools::keys::KeyStore;
use nvidia_devtools::tegrastats::TegraMonitor;
use nvidia_devtools::{AppState, SSH_PORT};
use tokio::process::Command;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

const DEFAULT_STATE_ROOT: &str = "/var/lib/nvidia-devtools";
const SETUP_SSHD: &str = "/usr/lib/nvidia-devtools/setup-sshd.sh";
const SSHD_RUN: &str = "/usr/lib/nvidia-devtools/sshd-run.sh";

#[derive(Parser)]
struct Args {
    /// Override MC-allocated bind (dev only).
    #[arg(long)]
    bind: Option<SocketAddr>,
}

fn state_root() -> PathBuf {
    std::env::var("STATE_DIRECTORY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_STATE_ROOT))
}

fn setup_sshd() -> anyhow::Result<()> {
    let status = std::process::Command::new(SETUP_SSHD)
        .env("STATE_DIRECTORY", state_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        anyhow::bail!("{SETUP_SSHD} exited with {status}");
    }
    Ok(())
}

async fn spawn_sshd() -> anyhow::Result<tokio::process::Child> {
    Command::new(SSHD_RUN)
        .env("STATE_DIRECTORY", state_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(Into::into)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();
    setup_sshd()?;

    let bind = nvidia_devtools::app_runtime::resolve_bind(args.bind).map_err(anyhow::Error::msg)?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(bind))
}

async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    let mut sshd = spawn_sshd().await?;
    let tegra = TegraMonitor::spawn();
    let state = AppState {
        keys: Arc::new(KeyStore::new(state_root())),
        tegra,
    };

    let app = Router::new()
        .nest("/api/v1", api::routes())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = nvidia_devtools::app_runtime::bind_listener(bind).await?;
    let local = listener.local_addr()?;
    tracing::info!("nvidia-devtools listening on http://{local}");
    nvidia_devtools::app_runtime::sync_tcp_ports(&[SSH_PORT]);
    nvidia_devtools::app_runtime::notify_systemd_ready();

    tokio::select! {
        res = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()) => {
            res?;
        }
        status = sshd.wait() => {
            match status {
                Ok(s) if s.success() => tracing::warn!("sshd exited cleanly"),
                Ok(s) => anyhow::bail!("sshd exited with {s}"),
                Err(e) => anyhow::bail!("sshd wait failed: {e}"),
            }
        }
    }

    let _ = sshd.start_kill();
    Ok(())
}

async fn shutdown_signal() {
    nvidia_devtools::app_runtime::wait_shutdown().await;
    tracing::info!("nvidia-devtools shutting down");
    nvidia_devtools::app_runtime::release_async().await;
}

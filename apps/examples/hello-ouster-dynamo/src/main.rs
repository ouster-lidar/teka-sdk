//! hello-ouster-world HTTP backend: MC Console PTY + HDMI viz.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use clap::Parser;
use hello_ouster_world::api;
use hello_ouster_world::{display_config, AppState};
use tower_http::cors::CorsLayer;

#[derive(Parser)]
struct Args {
    /// Override MC-allocated bind (dev only).
    #[arg(long)]
    bind: Option<SocketAddr>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let bind = hello_ouster_world::app_runtime::resolve_bind(args.bind).map_err(anyhow::Error::msg)?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(bind))
}

async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    // Ensure MC Console has a log file to follow before viz starts printing.
    let log_path = std::env::var("HELLO_OUSTER_WORLD_CONSOLE_LOG")
        .unwrap_or_else(|_| "/run/hello-ouster-world/console.log".to_string());
    if let Some(parent) = PathBuf::from(&log_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path);

    let state_dir = display_config::state_dir();
    let display_cfg = display_config::load(&state_dir);
    tracing::info!(hdmi = display_cfg.hdmi, stream = display_cfg.stream, "display config");

    let state = AppState::new(&display_cfg);
    hello_ouster_world::ui_state::spawn_ui_watchers(state.clone());

    let app = Router::new()
        .nest("/api/v1", api::routes())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = hello_ouster_world::app_runtime::bind_listener(bind).await?;
    let local = listener.local_addr()?;
    tracing::info!("hello-ouster-world listening on http://{local}");
    // Lidar / IMU (MC default sensor dest). Manifest lists the same ports;
    // firewalld opens via PUT …/runtime/hello-ouster-world/ports, not the JSON.
    hello_ouster_world::app_runtime::sync_udp_ports(&[7502, 7503]);
    hello_ouster_world::app_runtime::notify_systemd_ready();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    hello_ouster_world::app_runtime::wait_shutdown().await;
    tracing::info!("hello-ouster-world shutting down");
    hello_ouster_world::app_runtime::release_async().await;
}

//! Hello-pub HTTP backend for the Jetstream `hello-pub` app.

use std::net::SocketAddr;

use ::teka_app::status_stream::SummaryStream;
use axum::Router;
use clap::Parser;
use hello_pub::api::{self, spawn_card_bridge, spawn_generator};
use hello_pub::store::EventStore;
use hello_pub::AppState;
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

    let bind = hello_pub::app_runtime::resolve_bind(args.bind).map_err(anyhow::Error::msg)?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(bind))
}

async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    let store = EventStore::new();
    let card_stream = SummaryStream::new();
    spawn_card_bridge(store.clone(), card_stream.clone());
    spawn_generator(store.clone());

    let state = AppState { store, card_stream };

    let app = Router::new()
        .nest("/api/v1", api::routes())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = hello_pub::app_runtime::bind_listener(bind).await?;
    let local = listener.local_addr()?;
    tracing::info!("hello-pub listening on http://{local}");
    hello_pub::app_runtime::notify_systemd_ready();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    hello_pub::app_runtime::wait_shutdown().await;
    tracing::info!("hello-pub shutting down");
    hello_pub::app_runtime::release_async().await;
}

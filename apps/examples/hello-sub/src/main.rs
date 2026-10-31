//! Hello-sub HTTP backend for the Teka `hello-sub` app.

use std::net::SocketAddr;

use ::teka_app::status_stream::SummaryStream;
use axum::Router;
use clap::Parser;
use hello_sub::api;
use hello_sub::peer::{self, spawn_subscriber};
use hello_sub::store::FeedStore;
use hello_sub::{peer_config, AppState};
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

    let bind = hello_sub::app_runtime::resolve_bind(args.bind).map_err(anyhow::Error::msg)?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(bind))
}

async fn run(bind: SocketAddr) -> anyhow::Result<()> {
    let state_dir = peer_config::state_dir();
    let publisher_urls =
        peer_config::load(&state_dir).unwrap_or_else(|| vec![peer::publisher_base_url_from_env()]);
    tracing::info!("publisher URLs {publisher_urls:?}");
    let store = FeedStore::new(publisher_urls, state_dir);
    let card_stream = SummaryStream::new();
    spawn_subscriber(store.clone(), card_stream.clone());

    let state = AppState { store, card_stream };

    let app = Router::new()
        .nest("/api/v1", api::routes())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = hello_sub::app_runtime::bind_listener(bind).await?;
    let local = listener.local_addr()?;
    tracing::info!("hello-sub listening on http://{local}");
    hello_sub::app_runtime::notify_systemd_ready();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    hello_sub::app_runtime::wait_shutdown().await;
    tracing::info!("hello-sub shutting down");
    hello_sub::app_runtime::release_async().await;
}

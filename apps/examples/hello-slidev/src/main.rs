//! hello-slidev — guest-access toggle; deck auth enforced in this backend.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::get,
    Json, Router,
};
use clap::Parser;
use mc_portable_runtime::api_error::ApiError;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

const DEFAULT_STATE_DIR: &str = "/var/lib/hello-slidev";
const DEFAULT_DECK_ROOT: &str = "/usr/lib/hello-slidev/deck";
/// MC injects this on proxied upstream requests when the browser has a session.
const HEADER_PRINCIPAL_ID: &str = "x-mc-principal-id";

#[derive(Clone)]
struct AppState {
    state_path: PathBuf,
    deck_root: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct GuestAccessFile {
    #[serde(default)]
    enabled: bool,
}

#[derive(Serialize)]
struct GuestAccessResponse {
    enabled: bool,
    editable: bool,
}

#[derive(Deserialize)]
struct GuestAccessBody {
    enabled: bool,
}

#[derive(Parser)]
struct Args {
    /// Override MC-allocated bind (dev only).
    #[arg(long)]
    bind: Option<SocketAddr>,
}

fn state_path() -> PathBuf {
    let dir = std::env::var("STATE_DIRECTORY").unwrap_or_else(|_| DEFAULT_STATE_DIR.into());
    PathBuf::from(dir).join("guest-access.json")
}

fn deck_root() -> PathBuf {
    std::env::var("DECK_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_DECK_ROOT))
}

fn read_guest_access(path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<GuestAccessFile>(&raw)
        .map(|f| f.enabled)
        .unwrap_or(false)
}

fn write_guest_access(path: &Path, enabled: bool) -> Result<(), ApiError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                format!("mkdir state: {e}"),
            )
        })?;
    }
    let payload = GuestAccessFile { enabled };
    let encoded = serde_json::to_string_pretty(&payload)
        .map_err(|e| ApiError::invalid_request(format!("encode state: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &encoded).map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("write state tmp: {e}"),
        )
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("replace state: {e}"),
        )
    })?;
    Ok(())
}

fn has_mc_principal(headers: &HeaderMap) -> bool {
    headers
        .get(HEADER_PRINCIPAL_ID)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| !s.trim().is_empty())
}

fn deck_allowed(headers: &HeaderMap, state_path: &Path) -> bool {
    has_mc_principal(headers) || read_guest_access(state_path)
}

async fn get_guest_access(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<GuestAccessResponse> {
    Json(GuestAccessResponse {
        enabled: read_guest_access(&state.state_path),
        editable: has_mc_principal(&headers),
    })
}

async fn put_guest_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GuestAccessBody>,
) -> Result<Json<GuestAccessResponse>, ApiError> {
    if !has_mc_principal(&headers) {
        return Err(ApiError::unauthorized("sign in required"));
    }
    write_guest_access(&state.state_path, body.enabled)?;
    Ok(Json(GuestAccessResponse {
        enabled: body.enabled,
        editable: true,
    }))
}

async fn serve_deck(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(tail): AxumPath<String>,
) -> Result<Response, ApiError> {
    if !deck_allowed(&headers, &state.state_path) {
        return Err(ApiError::unauthorized("guest access disabled"));
    }
    let rel = tail.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    serve_deck_file(&state.deck_root, rel).await
}

async fn serve_deck_file(root: &Path, rel: &str) -> Result<Response, ApiError> {
    if rel.is_empty() || rel.contains("..") || rel.contains('\\') {
        return Err(ApiError::not_found("invalid path"));
    }
    let path = root.join(rel);
    let (Ok(canon), Ok(base)) = (path.canonicalize(), root.canonicalize()) else {
        return Err(ApiError::not_found("not found"));
    };
    if !canon.starts_with(&base) || !canon.is_file() {
        return Err(ApiError::not_found("not found"));
    }
    let body = tokio::fs::read(&canon).await.map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("read deck file: {e}"),
        )
    })?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type_for_path(&canon))
        .body(Body::from(body))
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                format!("response: {e}"),
            )
        })
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

fn api_routes() -> Router<AppState> {
    let deck = get(serve_deck).head(serve_deck);
    Router::new()
        .route(
            "/guest-access",
            get(get_guest_access).put(put_guest_access),
        )
        .route("/deck", deck.clone())
        .route("/deck/", deck.clone())
        .route("/deck/{*path}", deck)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let bind = hello_slidev::mc_runtime::resolve_bind(args.bind).map_err(anyhow::Error::msg)?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(bind, state_path(), deck_root()))
}

async fn run(bind: SocketAddr, state_path: PathBuf, deck_root: PathBuf) -> anyhow::Result<()> {
    let state = AppState {
        state_path,
        deck_root,
    };

    let app = Router::new()
        .nest("/api/v1", api_routes())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = hello_slidev::mc_runtime::bind_listener(bind).await?;
    let local = listener.local_addr()?;
    tracing::info!("hello-slidev listening on http://{local}");
    hello_slidev::mc_runtime::notify_systemd_ready();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    hello_slidev::mc_runtime::wait_shutdown().await;
    tracing::info!("hello-slidev shutting down");
    hello_slidev::mc_runtime::release_async().await;
}

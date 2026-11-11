//! nvidia-devtools HTTP API.

use ::teka::app::resource_stream::send_snapshot;
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::keys::KeysBody;
use crate::tegrastats::TegraSample;
use crate::{AppState, SSH_PORT};

/// MC injects this on proxied upstream requests when the browser has a session.
const HEADER_PRINCIPAL_ID: &str = "x-mc-principal-id";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(get_status))
        .route("/tegrastats/stream", get(tegrastats_ws))
        .route(
            "/keys",
            get(get_keys)
                .put(put_keys)
                .route_layer(middleware::from_fn(require_mc_principal)),
        )
}

async fn require_mc_principal(request: axum::extract::Request, next: middleware::Next) -> Response {
    if principal_present(request.headers()) {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
            .into_response()
    }
}

fn principal_present(headers: &HeaderMap) -> bool {
    headers
        .get(HEADER_PRINCIPAL_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

#[derive(Serialize)]
struct StatusResponse {
    ok: bool,
    ssh_port: u16,
}

async fn get_status() -> Json<StatusResponse> {
    Json(StatusResponse {
        ok: true,
        ssh_port: SSH_PORT,
    })
}

async fn get_keys(State(state): State<AppState>) -> Result<Json<KeysBody>, StatusCode> {
    state
        .keys
        .read()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

async fn put_keys(
    State(state): State<AppState>,
    Json(body): Json<KeysBody>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    state
        .keys
        .write(&body)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorBody { error: e })))
}

async fn tegrastats_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| tegrastats_ws_loop(socket, state))
}

async fn tegrastats_ws_loop(mut socket: WebSocket, state: AppState) {
    let initial = state.tegra.last.read().clone();
    if let Some(sample) = initial {
        if !send_sample(&mut socket, &sample).await {
            return;
        }
    }
    let mut rx = state.tegra.tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(sample) => {
                if !send_sample(&mut socket, &sample).await {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
}

async fn send_sample(socket: &mut WebSocket, sample: &TegraSample) -> bool {
    send_snapshot(socket, sample).await
}

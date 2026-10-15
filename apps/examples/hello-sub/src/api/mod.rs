//! hello-sub HTTP API — see README.md.

use ::app_runtime::resource_stream::send_snapshot;
use ::app_runtime::status_stream::serve_summary_broadcast_with_initial;
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::peer;
use crate::peer_config;
use crate::store::Status;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(get_status))
        .route("/feed", get(get_feed))
        .route("/feed/stream", get(feed_ws))
        .route("/peer-config", get(get_peer_config).post(post_peer_config))
        .route("/peer-state", post(post_peer_state))
        .route("/cards/stream", get(cards_ws))
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct PeerConfigBody {
    #[serde(default)]
    publisher_urls: Option<Vec<String>>,
    /// Accepted as a convenience; normalized into a one-element list.
    #[serde(default)]
    publisher_url: Option<String>,
}

#[derive(Deserialize, Default)]
struct PeerStateBody {
    #[serde(default)]
    publisher_url: Option<String>,
}

async fn get_status(State(state): State<AppState>) -> Json<Status> {
    Json(state.store.status())
}

async fn get_feed(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(crate::store::MAX_MERGED_EVENTS);
    Json(json!({ "events": state.store.list(limit) }))
}

async fn get_peer_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "publisher_urls": state.store.publisher_urls(),
    }))
}

async fn post_peer_config(
    State(state): State<AppState>,
    Json(body): Json<PeerConfigBody>,
) -> Json<Status> {
    let raw = if let Some(list) = body.publisher_urls {
        list
    } else if let Some(one) = body.publisher_url {
        vec![one]
    } else {
        Vec::new()
    };
    let urls = peer_config::normalize_url_list(raw);
    state.store.apply_peer_config(urls);
    Json(state.store.status())
}

async fn post_peer_state(
    State(state): State<AppState>,
    Json(body): Json<PeerStateBody>,
) -> Json<serde_json::Value> {
    let filter = body.publisher_url.filter(|u| !u.trim().is_empty());
    let targets: Vec<String> = if let Some(raw) = filter {
        let url = peer::normalize_publisher_url(&raw);
        if url.is_empty() {
            return Json(json!({
                "ok": false,
                "publishers": [],
                "error": "publisher_url is empty"
            }));
        }
        vec![url]
    } else {
        state.store.publisher_urls()
    };

    let mut publishers = Vec::new();
    let mut any_ok = false;
    let mut last_err: Option<String> = None;
    for url in targets {
        match peer::fetch_peer_state(&url).await {
            Ok(ps) => {
                state.store.set_peer_state(&url, Some(ps.clone()));
                any_ok = true;
                publishers.push(json!({
                    "publisher_url": url,
                    "ok": true,
                    "peer_state": ps,
                    "error": null,
                }));
            }
            Err(e) => {
                last_err = Some(e.clone());
                publishers.push(json!({
                    "publisher_url": url,
                    "ok": false,
                    "peer_state": null,
                    "error": e,
                }));
            }
        }
    }
    Json(json!({
        "ok": any_ok || publishers.is_empty(),
        "publishers": publishers,
        "error": last_err,
    }))
}

async fn feed_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| feed_ws_loop(socket, state))
}

async fn feed_ws_loop(mut socket: WebSocket, state: AppState) {
    let data = state.store.stream_data();
    if !send_snapshot(&mut socket, &data).await {
        return;
    }
    let mut rx = state.store.notify_tx.subscribe();
    while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) = rx.recv().await {
        let data = state.store.stream_data();
        if !send_snapshot(&mut socket, &data).await {
            break;
        }
    }
}

async fn cards_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let stream = state.card_stream.clone();
    let initial = stream.cache.read().clone().unwrap_or_default();
    ws.on_upgrade(move |socket| async move {
        serve_summary_broadcast_with_initial(socket, stream, initial).await;
    })
}

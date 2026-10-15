//! hello-pub HTTP API — see hello-pub.md.

use std::sync::Arc;
use std::time::Duration;

use ::app_runtime::resource_stream::send_snapshot;
use ::app_runtime::status_stream::{serve_summary_broadcast_with_initial, SummaryStream};
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::store::{Event, EventStore, State as PubState};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/state", get(get_state))
        .route("/events", get(list_events).post(post_event))
        .route("/events/stream", get(events_ws))
        .route("/generator", post(post_generator))
        .route("/cards/stream", get(cards_ws))
}

pub fn spawn_generator(store: Arc<EventStore>) {
    tokio::spawn(async move {
        let phrases = [
            "sensor ping",
            "network flap",
            "object spotted",
            "heartbeat tick",
            "config changed",
        ];
        let mut phrase_idx = 0usize;
        let mut gen_rx = store.generator_rx();
        loop {
            let gen = *gen_rx.borrow();
            if !gen.running {
                if gen_rx.changed().await.is_err() {
                    break;
                }
                continue;
            }
            let message = format!("Random: {}", phrases[phrase_idx % phrases.len()]);
            phrase_idx = phrase_idx.wrapping_add(1);
            let _ = store.publish("random", Some(&message));
            let interval = gen.interval_secs.max(1);
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
                changed = gen_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    });
}

pub fn spawn_card_bridge(store: Arc<EventStore>, card_stream: Arc<SummaryStream>) {
    publish_card_summary(&store, &card_stream);
    tokio::spawn(async move {
        let mut rx = store.events_tx.subscribe();
        while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            rx.recv().await
        {
            publish_card_summary(&store, &card_stream);
        }
    });
}

fn publish_card_summary(store: &EventStore, card_stream: &SummaryStream) {
    let summary = store.state().summary;
    let mut last = card_stream.cache.read().clone().unwrap_or_default();
    card_stream.tick(&summary, &mut last);
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct PostEventBody {
    kind: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct GeneratorBody {
    running: bool,
    interval_secs: Option<u64>,
}

async fn get_state(State(state): State<AppState>) -> Json<PubState> {
    Json(state.store.state())
}

async fn list_events(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(crate::store::MAX_EVENTS);
    Json(serde_json::json!({ "events": state.store.list(limit) }))
}

async fn post_event(
    State(state): State<AppState>,
    body: Option<Json<PostEventBody>>,
) -> (StatusCode, Json<Event>) {
    let body = body.map(|j| j.0).unwrap_or(PostEventBody {
        kind: None,
        message: None,
    });
    let kind = body
        .kind
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "manual".to_string());
    let event = state.store.publish(&kind, body.message.as_deref());
    (StatusCode::CREATED, Json(event))
}

async fn post_generator(
    State(state): State<AppState>,
    Json(body): Json<GeneratorBody>,
) -> Json<PubState> {
    Json(state.store.set_generator(body.running, body.interval_secs))
}

async fn events_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| events_ws_loop(socket, state))
}

async fn events_ws_loop(mut socket: WebSocket, state: AppState) {
    let data = state.store.stream_data();
    if !send_snapshot(&mut socket, &data).await {
        return;
    }
    let mut rx = state.store.events_tx.subscribe();
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

//! Typed dashboard status WebSocket helpers for MC app backends.

use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

pub fn status_message(summary: &str) -> String {
    serde_json::json!({ "type": "STATUS", "summary": summary }).to_string()
}

pub fn heartbeat_message() -> String {
    r#"{"type":"HEARTBEAT"}"#.to_string()
}

pub fn cards_message(cards: &[(&str, &str)]) -> String {
    let cards_json: Vec<_> = cards
        .iter()
        .map(|(card_id, summary)| serde_json::json!({ "card_id": card_id, "summary": summary }))
        .collect();
    serde_json::json!({ "type": "STATUS", "cards": cards_json }).to_string()
}

fn new_text_bus() -> broadcast::Sender<String> {
    broadcast::channel(4).0
}

pub struct SummaryStream {
    pub cache: RwLock<Option<String>>,
    pub tx: broadcast::Sender<String>,
}

impl SummaryStream {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cache: RwLock::new(None),
            tx: new_text_bus(),
        })
    }

    pub fn set_cache(&self, summary: String) {
        *self.cache.write() = Some(summary);
    }

    pub fn tick(&self, summary: &str, last: &mut String) {
        self.set_cache(summary.to_string());
        if summary != *last {
            *last = summary.to_string();
            let _ = self.tx.send(status_message(summary));
        }
    }
}

pub struct MultiCardStream {
    pub cache: RwLock<Vec<(String, String)>>,
    pub tx: broadcast::Sender<String>,
}

impl MultiCardStream {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cache: RwLock::new(Vec::new()),
            tx: new_text_bus(),
        })
    }

    pub fn publish_if_changed(&self, cards: &[(String, String)], last: &mut Vec<(String, String)>) {
        if cards == last.as_slice() {
            return;
        }
        *last = cards.to_vec();
        *self.cache.write() = cards.to_vec();
        let refs: Vec<(&str, &str)> = cards
            .iter()
            .map(|(id, s)| (id.as_str(), s.as_str()))
            .collect();
        let _ = self.tx.send(cards_message(&refs));
    }
}

pub fn spawn_summary_poll<F, Fut>(stream: Arc<SummaryStream>, interval: Duration, mut poll: F)
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = String> + Send,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut last = String::new();
        loop {
            ticker.tick().await;
            let summary = poll().await;
            stream.tick(&summary, &mut last);
        }
    });
}

/// One `SummaryStream` per static slot; poll task starts on first call.
pub fn once_lazy_poll<F, Fut>(
    slot: &'static OnceLock<Arc<SummaryStream>>,
    interval: Duration,
    poll: F,
) -> Arc<SummaryStream>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = String> + Send,
{
    slot.get_or_init(|| {
        let stream = SummaryStream::new();
        spawn_summary_poll(stream.clone(), interval, poll);
        stream
    })
    .clone()
}

pub async fn serve_summary_broadcast(socket: WebSocket, stream: Arc<SummaryStream>) {
    let initial = stream
        .cache
        .read()
        .clone()
        .map(|summary| status_message(&summary));
    serve_broadcast(socket, stream.tx.subscribe(), initial).await;
}

pub async fn serve_summary_broadcast_with_initial(
    socket: WebSocket,
    stream: Arc<SummaryStream>,
    initial: String,
) {
    let initial = if initial.is_empty() {
        stream
            .cache
            .read()
            .clone()
            .map(|summary| status_message(&summary))
    } else {
        Some(status_message(&initial))
    };
    serve_broadcast(socket, stream.tx.subscribe(), initial).await;
}

pub async fn serve_broadcast(
    socket: WebSocket,
    rx: broadcast::Receiver<String>,
    initial: Option<String>,
) {
    let mut socket = socket;
    if let Some(text) = initial {
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    pump_text_broadcast(socket, rx).await;
}

pub async fn serve_multi_card_broadcast(socket: WebSocket, stream: Arc<MultiCardStream>) {
    let cached = stream.cache.read().clone();
    let initial = if cached.is_empty() {
        None
    } else {
        let refs: Vec<(&str, &str)> = cached
            .iter()
            .map(|(id, s)| (id.as_str(), s.as_str()))
            .collect();
        Some(cards_message(&refs))
    };
    serve_broadcast(socket, stream.tx.subscribe(), initial).await;
}

pub async fn serve_static_summary(mut socket: WebSocket, summary: &str) {
    if socket
        .send(Message::Text(status_message(summary).into()))
        .await
        .is_err()
    {
        return;
    }
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        heartbeat.tick().await;
        if socket
            .send(Message::Text(heartbeat_message().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn pump_text_broadcast(mut socket: WebSocket, mut rx: broadcast::Receiver<String>) {
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if socket.send(Message::Text(heartbeat_message().into())).await.is_err() {
                    break;
                }
            }
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }
}

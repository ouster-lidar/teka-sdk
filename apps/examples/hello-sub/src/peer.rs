//! Resolve hello-pub peer URLs and subscribe to their event streams.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ::app_runtime::runtime::app_base_url;
use ::app_runtime::status_stream::SummaryStream;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::store::{FeedStore, IncomingEvent};

#[derive(Debug, Deserialize)]
struct SnapshotEnvelope {
    #[serde(rename = "type")]
    frame_type: String,
    data: Option<SnapshotData>,
}

#[derive(Debug, Deserialize)]
struct SnapshotData {
    events: Option<Vec<IncomingEvent>>,
    state: Option<Value>,
}

/// `{MC_RUNTIME_URL}/api/v1/app/{id}` or `MC_HELLO_PUB_URL` override.
pub fn publisher_base_url_from_env() -> String {
    if let Ok(url) = std::env::var("MC_HELLO_PUB_URL") {
        let url = url.trim();
        if !url.is_empty() {
            return normalize_publisher_url(url);
        }
    }
    let peer = std::env::var("MC_HELLO_PUB_APP_ID").unwrap_or_else(|_| "hello-pub".into());
    let peer = peer.trim();
    let peer = if peer.is_empty() { "hello-pub" } else { peer };
    normalize_publisher_url(&format!(
        "{}/api/v1/app/{peer}",
        app_base_url().trim_end_matches('/')
    ))
}

/// Trim, ensure `http(s)` scheme, strip trailing slash and default ports
/// (`:80` / `:443` / http `:8080`), rewrite loopback `127.0.0.1` → `localhost`,
/// and append `/api/v1/app/hello-pub` when no app path is present.
pub fn normalize_publisher_url(raw: &str) -> String {
    let mut url = raw.trim().trim_end_matches('/').to_string();
    if url.is_empty() {
        return url;
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("http://{url}");
    }
    // Guard against accidental `http://http://…` from paste + prepend.
    if let Some(rest) = url.strip_prefix("http://http://") {
        url = format!("http://{rest}");
    } else if let Some(rest) = url.strip_prefix("https://https://") {
        url = format!("https://{rest}");
    } else if let Some(rest) = url.strip_prefix("http://https://") {
        url = format!("https://{rest}");
    } else if let Some(rest) = url.strip_prefix("https://http://") {
        url = format!("http://{rest}");
    }
    url = simplify_loopback_host(&url);
    url = strip_default_port(&url);
    if !url.contains("/api/v1/app/") {
        url = format!("{url}/api/v1/app/hello-pub");
    }
    url
}

fn simplify_loopback_host(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("http://127.0.0.1") {
        format!("http://localhost{rest}")
    } else if let Some(rest) = url.strip_prefix("https://127.0.0.1") {
        format!("https://localhost{rest}")
    } else {
        url.to_string()
    }
}

fn strip_default_port(url: &str) -> String {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        ("https://", r)
    } else if let Some(r) = url.strip_prefix("http://") {
        ("http://", r)
    } else {
        return url.to_string();
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let authority = if scheme == "https://" {
        authority
            .strip_suffix(":443")
            .unwrap_or(authority)
            .to_string()
    } else {
        authority
            .strip_suffix(":80")
            .or_else(|| authority.strip_suffix(":8080"))
            .unwrap_or(authority)
            .to_string()
    };
    format!("{scheme}{authority}{path}")
}

fn http_to_ws(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        url.to_string()
    }
}

struct SessionHandle {
    abort: tokio::sync::watch::Sender<bool>,
    join: JoinHandle<()>,
}

pub fn spawn_subscriber(store: Arc<FeedStore>, card_stream: Arc<SummaryStream>) {
    publish_card(&store, &card_stream);
    let store_cards = store.clone();
    let card_stream2 = card_stream.clone();
    tokio::spawn(async move {
        let mut rx = store_cards.notify_tx.subscribe();
        while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            rx.recv().await
        {
            publish_card(&store_cards, &card_stream2);
        }
    });

    tokio::spawn(async move {
        let mut settings_rx = store.settings_rx();
        let mut sessions: HashMap<String, SessionHandle> = HashMap::new();
        loop {
            let desired = settings_rx.borrow().clone();
            let desired_set: std::collections::HashSet<_> = desired.iter().cloned().collect();

            let running: Vec<String> = sessions.keys().cloned().collect();
            for url in running {
                if !desired_set.contains(&url) {
                    if let Some(handle) = sessions.remove(&url) {
                        let _ = handle.abort.send(true);
                        handle.join.abort();
                    }
                    store.remove_publisher(&url);
                    info!("stopped publisher session {url}");
                }
            }

            for url in &desired {
                if sessions.contains_key(url) {
                    continue;
                }
                let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
                let store_s = store.clone();
                let url_s = url.clone();
                let join = tokio::spawn(async move {
                    run_session_loop(store_s, url_s, abort_rx).await;
                });
                sessions.insert(
                    url.clone(),
                    SessionHandle {
                        abort: abort_tx,
                        join,
                    },
                );
                info!("started publisher session {url}");
            }

            if settings_rx.changed().await.is_err() {
                for (_, handle) in sessions.drain() {
                    let _ = handle.abort.send(true);
                    handle.join.abort();
                }
                break;
            }
        }
    });
}

async fn run_session_loop(
    store: Arc<FeedStore>,
    publisher_url: String,
    mut abort_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if *abort_rx.borrow() {
            break;
        }
        tokio::select! {
            biased;
            changed = abort_rx.changed() => {
                if changed.is_err() || *abort_rx.borrow() {
                    break;
                }
            }
            result = run_session(store.clone(), publisher_url.clone()) => {
                match result {
                    Ok(()) => info!("publisher stream closed ({publisher_url}); reconnecting"),
                    Err(e) => {
                        warn!("publisher stream error ({publisher_url}): {e}");
                        store.set_last_error(&publisher_url, Some(e));
                    }
                }
                store.set_connected(&publisher_url, false);
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {
                        backoff = (backoff * 2).min(Duration::from_secs(10));
                    }
                    changed = abort_rx.changed() => {
                        if changed.is_err() || *abort_rx.borrow() {
                            break;
                        }
                        backoff = Duration::from_secs(1);
                    }
                }
            }
        }
    }
    store.set_connected(&publisher_url, false);
}

fn publish_card(store: &FeedStore, card_stream: &SummaryStream) {
    let summary = store.status().summary;
    let mut last = card_stream.cache.read().clone().unwrap_or_default();
    card_stream.tick(&summary, &mut last);
}

async fn run_session(store: Arc<FeedStore>, publisher_url: String) -> Result<(), String> {
    let publisher_url = publisher_url.trim_end_matches('/').to_string();
    if publisher_url.is_empty() {
        return Err("publisher URL is empty".into());
    }
    if !publisher_url.starts_with("http://") && !publisher_url.starts_with("https://") {
        return Err("publisher URL must be http(s)".into());
    }

    let ws_http = format!("{}/events/stream", http_to_ws(&publisher_url));
    info!("connecting to publisher stream {ws_http}");
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_http)
        .await
        .map_err(|e| e.to_string())?;
    store.set_connected(&publisher_url, true);
    store.set_last_error(&publisher_url, None);

    while let Some(msg) = ws.next().await {
        let msg = msg.map_err(|e| e.to_string())?;
        match msg {
            Message::Text(text) => {
                handle_text(&store, &publisher_url, text.as_str());
            }
            Message::Ping(p) => {
                ws.send(Message::Pong(p)).await.map_err(|e| e.to_string())?;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    Ok(())
}

fn handle_text(store: &FeedStore, publisher_url: &str, text: &str) {
    let Ok(env) = serde_json::from_str::<SnapshotEnvelope>(text) else {
        return;
    };
    if !env.frame_type.eq_ignore_ascii_case("snapshot") {
        return;
    }
    let Some(data) = env.data else {
        return;
    };
    let events = data.events.unwrap_or_default();
    store.apply_snapshot(publisher_url, events, data.state);
}

pub async fn fetch_peer_state(publisher_url: &str) -> Result<Value, String> {
    let publisher_url = publisher_url.trim_end_matches('/').to_string();
    if publisher_url.is_empty() {
        return Err("publisher URL is empty".into());
    }
    let url = format!("{publisher_url}/state");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("publisher returned {}", resp.status()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

pub async fn fetch_all_peer_states(store: &FeedStore) -> Vec<(String, Result<Value, String>)> {
    let urls = store.publisher_urls();
    let mut out = Vec::with_capacity(urls.len());
    for url in urls {
        out.push((url.clone(), fetch_peer_state(&url).await));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize_publisher_url;

    #[test]
    fn prepends_http_for_bare_host() {
        assert_eq!(
            normalize_publisher_url("other-box"),
            "http://other-box/api/v1/app/hello-pub"
        );
        assert_eq!(
            normalize_publisher_url("192.168.1.10:8080"),
            "http://192.168.1.10/api/v1/app/hello-pub"
        );
    }

    #[test]
    fn keeps_scheme_and_path() {
        assert_eq!(
            normalize_publisher_url("http://other-box/api/v1/app/hello-pub"),
            "http://other-box/api/v1/app/hello-pub"
        );
        assert_eq!(
            normalize_publisher_url("https://other-box/api/v1/app/hello-pub/"),
            "https://other-box/api/v1/app/hello-pub"
        );
    }

    #[test]
    fn dedupes_doubled_scheme() {
        assert_eq!(
            normalize_publisher_url("http://http://other-box"),
            "http://other-box/api/v1/app/hello-pub"
        );
    }

    #[test]
    fn loopback_and_default_ports() {
        assert_eq!(
            normalize_publisher_url("http://127.0.0.1:80/api/v1/app/hello-pub"),
            "http://localhost/api/v1/app/hello-pub"
        );
    }
}

//! Mirrored multi-publisher feed store — hello-sub.md.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, watch};

pub const MAX_EVENTS: usize = 10;
pub const MAX_MERGED_EVENTS: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub at: String,
    pub kind: String,
    pub message: String,
    pub publisher_url: String,
}

/// Event shape as received from hello-pub (no `publisher_url` stamp).
#[derive(Debug, Clone, Deserialize)]
pub struct IncomingEvent {
    pub id: u64,
    pub at: String,
    pub kind: String,
    pub message: String,
}

impl IncomingEvent {
    pub fn stamp(self, publisher_url: &str) -> Event {
        Event {
            id: self.id,
            at: self.at,
            kind: self.kind,
            message: self.message,
            publisher_url: publisher_url.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PublisherStatus {
    pub publisher_url: String,
    pub connected: bool,
    pub last_error: Option<String>,
    pub peer_state: Option<Value>,
    pub events_received: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub publishers: Vec<PublisherStatus>,
    pub connected_count: usize,
    pub publisher_count: usize,
    pub events_received: u64,
    pub last_event: Option<Event>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamData {
    pub events: Vec<Event>,
    pub status: Status,
}

#[derive(Debug, Clone)]
struct PublisherSlot {
    connected: bool,
    last_error: Option<String>,
    peer_state: Option<Value>,
    events_received: u64,
    events: VecDeque<Event>,
}

impl PublisherSlot {
    fn new() -> Self {
        Self {
            connected: false,
            last_error: None,
            peer_state: None,
            events_received: 0,
            events: VecDeque::with_capacity(MAX_EVENTS),
        }
    }

    fn to_status(&self, publisher_url: &str) -> PublisherStatus {
        PublisherStatus {
            publisher_url: publisher_url.to_string(),
            connected: self.connected,
            last_error: self.last_error.clone(),
            peer_state: self.peer_state.clone(),
            events_received: self.events_received,
        }
    }
}

pub struct FeedStore {
    /// Ordered publisher URLs (config order).
    publisher_urls: Mutex<Vec<String>>,
    slots: Mutex<HashMap<String, PublisherSlot>>,
    state_dir: std::path::PathBuf,
    pub notify_tx: broadcast::Sender<()>,
    pub settings_tx: watch::Sender<Vec<String>>,
}

impl FeedStore {
    pub fn new(publisher_urls: Vec<String>, state_dir: std::path::PathBuf) -> Arc<Self> {
        let (notify_tx, _) = broadcast::channel(64);
        let (settings_tx, _) = watch::channel(publisher_urls.clone());
        let mut slots = HashMap::new();
        for url in &publisher_urls {
            slots.insert(url.clone(), PublisherSlot::new());
        }
        Arc::new(Self {
            publisher_urls: Mutex::new(publisher_urls),
            slots: Mutex::new(slots),
            state_dir,
            notify_tx,
            settings_tx,
        })
    }

    pub fn publisher_urls(&self) -> Vec<String> {
        self.publisher_urls.lock().clone()
    }

    pub fn settings_rx(&self) -> watch::Receiver<Vec<String>> {
        self.settings_tx.subscribe()
    }

    /// Returns `true` when the normalized URL list changed.
    pub fn apply_peer_config(&self, publisher_urls: Vec<String>) -> bool {
        let mut cur = self.publisher_urls.lock();
        if *cur == publisher_urls {
            return false;
        }
        *cur = publisher_urls.clone();
        drop(cur);

        {
            let mut slots = self.slots.lock();
            let keep: std::collections::HashSet<_> = publisher_urls.iter().cloned().collect();
            slots.retain(|k, _| keep.contains(k));
            for url in &publisher_urls {
                slots.entry(url.clone()).or_insert_with(PublisherSlot::new);
            }
        }

        crate::peer_config::save(&self.state_dir, &publisher_urls);
        let _ = self.settings_tx.send(publisher_urls);
        let _ = self.notify_tx.send(());
        true
    }

    pub fn remove_publisher(&self, publisher_url: &str) {
        self.slots.lock().remove(publisher_url);
        let _ = self.notify_tx.send(());
    }

    pub fn apply_snapshot(
        &self,
        publisher_url: &str,
        events: Vec<IncomingEvent>,
        peer_state: Option<Value>,
    ) {
        let mut slots = self.slots.lock();
        let Some(slot) = slots.get_mut(publisher_url) else {
            return;
        };
        slot.events.clear();
        for ev in events.into_iter().take(MAX_EVENTS) {
            slot.events.push_back(ev.stamp(publisher_url));
        }
        if let Some(ref ps) = peer_state {
            slot.peer_state = Some(ps.clone());
        }
        let buf_len = slot.events.len() as u64;
        let total = peer_state
            .as_ref()
            .and_then(|v| v.get("total_published"))
            .and_then(|v| v.as_u64())
            .unwrap_or(buf_len);
        slot.events_received = total;
        slot.connected = true;
        slot.last_error = None;
        drop(slots);
        let _ = self.notify_tx.send(());
    }

    pub fn set_connected(&self, publisher_url: &str, connected: bool) {
        if let Some(slot) = self.slots.lock().get_mut(publisher_url) {
            slot.connected = connected;
        }
        let _ = self.notify_tx.send(());
    }

    pub fn set_peer_state(&self, publisher_url: &str, peer_state: Option<Value>) {
        if let Some(slot) = self.slots.lock().get_mut(publisher_url) {
            slot.peer_state = peer_state;
        }
        let _ = self.notify_tx.send(());
    }

    pub fn set_last_error(&self, publisher_url: &str, err: Option<String>) {
        if let Some(slot) = self.slots.lock().get_mut(publisher_url) {
            slot.last_error = err;
        }
        let _ = self.notify_tx.send(());
    }

    pub fn list(&self, limit: usize) -> Vec<Event> {
        let limit = limit.min(MAX_MERGED_EVENTS);
        let urls = self.publisher_urls.lock().clone();
        let slots = self.slots.lock();
        let mut merged: Vec<Event> = Vec::new();
        for url in &urls {
            if let Some(slot) = slots.get(url) {
                merged.extend(slot.events.iter().cloned());
            }
        }
        merged.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
        merged.truncate(limit);
        merged
    }

    pub fn status(&self) -> Status {
        let last_event = self.list(1).into_iter().next();
        let urls = self.publisher_urls();
        let slots = self.slots.lock();
        let publishers: Vec<PublisherStatus> = urls
            .iter()
            .map(|url| {
                slots
                    .get(url)
                    .map(|s| s.to_status(url))
                    .unwrap_or_else(|| PublisherStatus {
                        publisher_url: url.clone(),
                        connected: false,
                        last_error: None,
                        peer_state: None,
                        events_received: 0,
                    })
            })
            .collect();
        drop(slots);
        let connected_count = publishers.iter().filter(|p| p.connected).count();
        let publisher_count = publishers.len();
        let events_received: u64 = publishers.iter().map(|p| p.events_received).sum();
        let summary = if publisher_count == 0 {
            "No publishers configured".to_string()
        } else if connected_count == 0 {
            publishers
                .iter()
                .find_map(|p| p.last_error.clone())
                .unwrap_or_else(|| "Waiting for publishers".to_string())
        } else {
            format!("{connected_count}/{publisher_count} publishers connected")
        };
        Status {
            publishers,
            connected_count,
            publisher_count,
            events_received,
            last_event,
            summary,
        }
    }

    pub fn stream_data(&self) -> StreamData {
        StreamData {
            events: self.list(MAX_MERGED_EVENTS),
            status: self.status(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hello-sub-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn apply_snapshot_does_not_wipe_other_peers() {
        let dir = test_dir("merge");
        let urls = vec![
            "http://a/api/v1/app/hello-pub".into(),
            "http://b/api/v1/app/hello-pub".into(),
        ];
        let store = FeedStore::new(urls, dir.clone());
        store.apply_snapshot(
            "http://a/api/v1/app/hello-pub",
            vec![IncomingEvent {
                id: 1,
                at: "2026-01-01T00:00:01Z".into(),
                kind: "tick".into(),
                message: "a1".into(),
            }],
            None,
        );
        store.apply_snapshot(
            "http://b/api/v1/app/hello-pub",
            vec![IncomingEvent {
                id: 2,
                at: "2026-01-01T00:00:02Z".into(),
                kind: "tick".into(),
                message: "b1".into(),
            }],
            None,
        );
        store.apply_snapshot(
            "http://a/api/v1/app/hello-pub",
            vec![IncomingEvent {
                id: 3,
                at: "2026-01-01T00:00:03Z".into(),
                kind: "tick".into(),
                message: "a2".into(),
            }],
            None,
        );
        let events = store.list(10);
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| e.message == "a2"));
        assert!(events.iter().any(|e| e.message == "b1"));
        assert!(!events.iter().any(|e| e.message == "a1"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_peer_config_updates_slots() {
        let dir = test_dir("cfg");
        let store = FeedStore::new(vec!["http://a/api/v1/app/hello-pub".into()], dir.clone());
        assert!(store.apply_peer_config(vec![
            "http://a/api/v1/app/hello-pub".into(),
            "http://b/api/v1/app/hello-pub".into(),
        ]));
        assert_eq!(store.publisher_urls().len(), 2);
        assert!(!store.apply_peer_config(vec![
            "http://a/api/v1/app/hello-pub".into(),
            "http://b/api/v1/app/hello-pub".into(),
        ]));
        let _ = fs::remove_dir_all(&dir);
    }
}

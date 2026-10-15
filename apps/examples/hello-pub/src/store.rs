//! In-memory event ring buffer and generator control — README.md.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

pub const MAX_EVENTS: usize = 10;
pub const DEFAULT_INTERVAL_SECS: u64 = 5;
pub const MIN_INTERVAL_SECS: u64 = 1;
pub const MAX_INTERVAL_SECS: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub at: String,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub last_event: Option<Event>,
    pub total_published: u64,
    pub generator_running: bool,
    pub generator_interval_secs: u64,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamData {
    pub events: Vec<Event>,
    pub state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorSettings {
    pub running: bool,
    pub interval_secs: u64,
}

pub struct EventStore {
    events: Mutex<VecDeque<Event>>,
    next_id: AtomicU64,
    total_published: AtomicU64,
    pub events_tx: broadcast::Sender<()>,
    generator_tx: watch::Sender<GeneratorSettings>,
}

impl EventStore {
    pub fn new() -> Arc<Self> {
        let (events_tx, _) = broadcast::channel(64);
        let (generator_tx, _) = watch::channel(GeneratorSettings {
            running: false,
            interval_secs: DEFAULT_INTERVAL_SECS,
        });
        Arc::new(Self {
            events: Mutex::new(VecDeque::with_capacity(MAX_EVENTS)),
            next_id: AtomicU64::new(1),
            total_published: AtomicU64::new(0),
            events_tx,
            generator_tx,
        })
    }

    /// Publish an event. When `message` is `None` or blank, fills `Manual event #{id}`
    /// using the assigned id (no race with `total_published`).
    pub fn publish(&self, kind: &str, message: Option<&str>) -> Event {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let message = match message.map(str::trim).filter(|s| !s.is_empty()) {
            Some(m) => m.to_string(),
            None => format!("Manual event #{id}"),
        };
        let event = Event {
            id,
            at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            kind: kind.to_string(),
            message,
        };
        {
            let mut q = self.events.lock();
            if q.len() >= MAX_EVENTS {
                q.pop_back();
            }
            q.push_front(event.clone());
        }
        self.total_published.fetch_add(1, Ordering::Relaxed);
        let _ = self.events_tx.send(());
        event
    }

    pub fn list(&self, limit: usize) -> Vec<Event> {
        let limit = limit.min(MAX_EVENTS);
        self.events.lock().iter().take(limit).cloned().collect()
    }

    pub fn state(&self) -> State {
        let last_event = self.events.lock().front().cloned();
        let total_published = self.total_published.load(Ordering::Relaxed);
        let gen = *self.generator_tx.borrow();
        let summary = if total_published == 0 {
            "No events published".to_string()
        } else {
            format!(
                "{total_published} event{} published",
                if total_published == 1 { "" } else { "s" }
            )
        };
        State {
            last_event,
            total_published,
            generator_running: gen.running,
            generator_interval_secs: gen.interval_secs,
            summary,
        }
    }

    pub fn stream_data(&self) -> StreamData {
        StreamData {
            events: self.list(MAX_EVENTS),
            state: self.state(),
        }
    }

    pub fn set_generator(&self, running: bool, interval_secs: Option<u64>) -> State {
        let mut next = *self.generator_tx.borrow();
        next.running = running;
        if let Some(secs) = interval_secs {
            next.interval_secs = secs.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);
        }
        let _ = self.generator_tx.send_if_modified(|cur| {
            if *cur == next {
                false
            } else {
                *cur = next;
                true
            }
        });
        let _ = self.events_tx.send(());
        self.state()
    }

    pub fn generator_rx(&self) -> watch::Receiver<GeneratorSettings> {
        self.generator_tx.subscribe()
    }
}

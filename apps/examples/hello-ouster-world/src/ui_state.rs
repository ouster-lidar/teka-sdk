//! UI state WebSocket stream: snapshot on connect, delta patches on change.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde_json::{json, Value};
use tokio::time::MissedTickBehavior;

use crate::display_config;
use crate::source_config;
use crate::AppState;

pub struct UiStateStream {
    pub cache: RwLock<Option<Value>>,
    pub tx: tokio::sync::broadcast::Sender<String>,
}

impl UiStateStream {
    pub fn new() -> Arc<Self> {
        let (tx, _) = tokio::sync::broadcast::channel(4);
        Arc::new(Self {
            cache: RwLock::new(None),
            tx,
        })
    }

    /// Fill cache without broadcasting. First real change then emits a delta.
    pub fn seed(&self, next: Value) {
        let mut cache = self.cache.write();
        if cache.is_none() {
            *cache = Some(next);
        }
    }

    pub fn publish_if_changed(&self, next: Value) {
        let mut cache = self.cache.write();
        if cache.as_ref() == Some(&next) {
            return;
        }
        let patch = diff_ui(cache.as_ref(), &next);
        *cache = Some(next);
        drop(cache);
        if let Some(patch) = patch {
            if patch.as_object().is_some_and(|o| !o.is_empty()) {
                let msg = json!({ "type": "delta", "patch": patch }).to_string();
                let _ = self.tx.send(msg);
            }
        }
    }
}

pub fn snapshot_message(data: &Value) -> String {
    json!({ "type": "snapshot", "data": data }).to_string()
}

pub fn build_ui_state(state: &AppState) -> Value {
    let cfg = source_config::load(&display_config::state_dir());
    let runtime = source_config::load_runtime();
    json!({
        "source": source_config::persisted_json(&cfg),
        "runtime": source_config::runtime_json(&runtime),
        "display": display_ui_json(state),
        "display_output_enabled": state.display_output_enabled(),
        "record": state.record.status_json(),
    })
}

/// Drop per-frame fields so HDMI/source deltas aren't drowned by capture ticks.
fn display_ui_json(state: &AppState) -> Value {
    let mut display = state.display.status_json();
    if let Some(obj) = display.as_object_mut() {
        obj.remove("last_capture_ms");
        obj.remove("need_render");
    }
    display
}

pub fn diff_ui(prev: Option<&Value>, next: &Value) -> Option<Value> {
    let next_obj = next.as_object()?;
    let prev_obj = prev.and_then(|v| v.as_object());
    let mut patch = serde_json::Map::new();
    for (key, next_val) in next_obj {
        let changed = match prev_obj {
            None => true,
            Some(prev) => prev.get(key) != Some(next_val),
        };
        if changed {
            patch.insert(key.clone(), next_val.clone());
        }
    }
    if patch.is_empty() {
        None
    } else {
        Some(Value::Object(patch))
    }
}

pub fn publish_ui(state: &AppState) {
    state
        .ui_state
        .publish_if_changed(build_ui_state(state));
    let summary = source_config::load(&display_config::state_dir()).dashboard_summary();
    let mut last = state.card_stream.cache.read().clone().unwrap_or_default();
    state.card_stream.tick(&summary, &mut last);
}

pub fn spawn_ui_watchers(state: AppState) {
    state.ui_state.seed(build_ui_state(&state));
    let poll_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            publish_ui(&poll_state);
        }
    });
}

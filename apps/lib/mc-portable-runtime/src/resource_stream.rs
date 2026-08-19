//! Resource live-picture WebSocket frames — mission-control/specs/error-codes.md lock 9a.

use axum::extract::ws::{Message, WebSocket};
use serde::Serialize;

pub const SNAPSHOT: &str = "snapshot";

/// `{ "type": "snapshot", "data": T }` where `T` matches the sibling GET body.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotFrame<T> {
    #[serde(rename = "type")]
    pub frame_type: &'static str,
    pub data: T,
}

impl<T> SnapshotFrame<T> {
    pub fn new(data: T) -> Self {
        Self {
            frame_type: SNAPSHOT,
            data,
        }
    }
}

pub fn snapshot_message<T: Serialize>(data: &T) -> String {
    serde_json::to_string(&SnapshotFrame {
        frame_type: SNAPSHOT,
        data,
    })
    .unwrap_or_else(|_| {
        serde_json::json!({ "type": SNAPSHOT, "data": null }).to_string()
    })
}

/// Emit a snapshot frame on a WebSocket. Returns `false` if the send failed.
pub async fn send_snapshot<T: Serialize>(socket: &mut WebSocket, data: &T) -> bool {
    socket
        .send(Message::Text(snapshot_message(data).into()))
        .await
        .is_ok()
}

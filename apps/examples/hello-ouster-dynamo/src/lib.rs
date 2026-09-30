//! hello-ouster-dynamo MC backend: PTY console + display stream + HDMI viz.

pub mod api;
pub mod display;
pub mod display_config;
pub mod screencopy;
pub mod osf;
pub mod record;
pub mod source_config;
pub mod ui_state;
pub mod vpointer;

::app_runtime::declare_app_runtime!("hello-ouster-dynamo");

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::display::DisplayState;
use crate::osf::ImportHandle;
use crate::record::RecordHandle;
use ::app_runtime::status_stream::SummaryStream;

/// Max concurrent Console log followers (WebSocket, no PTY).
pub const MAX_PTY_SESSIONS: usize = 16;

#[derive(Clone)]
pub struct AppState {
    pub pty_sessions: Arc<AtomicUsize>,
    pub display: DisplayState,
    pub osf_import: ImportHandle,
    pub record: RecordHandle,
    pub ui_state: Arc<ui_state::UiStateStream>,
    pub card_stream: Arc<SummaryStream>,
}

impl AppState {
    pub fn new(cfg: &display_config::PersistedDisplayConfig) -> Self {
        Self {
            pty_sessions: Arc::new(AtomicUsize::new(0)),
            display: DisplayState::new(cfg.hdmi, cfg.stream),
            osf_import: ImportHandle::new(),
            record: RecordHandle::new(),
            ui_state: ui_state::UiStateStream::new(),
            card_stream: SummaryStream::new(),
        }
    }

    pub fn try_acquire_pty(&self) -> bool {
        let mut cur = self.pty_sessions.load(Ordering::Relaxed);
        loop {
            if cur >= MAX_PTY_SESSIONS {
                return false;
            }
            match self.pty_sessions.compare_exchange_weak(
                cur,
                cur + 1,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(v) => cur = v,
            }
        }
    }

    pub fn release_pty(&self) {
        self.pty_sessions.fetch_sub(1, Ordering::SeqCst);
    }
}

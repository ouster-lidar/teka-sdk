//! hello-ouster-world MC backend: PTY console + display stream + HDMI viz.

pub mod api;
pub mod display;
pub mod display_config;
pub mod osf;
pub mod record;
pub mod source_config;
pub mod ui_state;
pub mod vpointer;

mc_portable_runtime::declare_mc_runtime!("hello-ouster-world");

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::display::DisplayState;
use crate::osf::ImportHandle;
use crate::record::RecordHandle;
use mc_portable_runtime::status_stream::SummaryStream;

/// Max concurrent Console log followers (WebSocket, no PTY).
pub const MAX_PTY_SESSIONS: usize = 16;

#[derive(Clone)]
pub struct AppState {
    pub pty_sessions: Arc<AtomicUsize>,
    pub display: DisplayState,
    /// Persisted display-output enable (Sway/SimpleViz). Soft state mirrors disk.
    pub display_output_enabled: Arc<AtomicBool>,
    pub osf_import: ImportHandle,
    pub record: RecordHandle,
    pub ui_state: Arc<ui_state::UiStateStream>,
    pub card_stream: Arc<SummaryStream>,
}

impl AppState {
    pub fn new(display_output_enabled: bool) -> Self {
        Self {
            pty_sessions: Arc::new(AtomicUsize::new(0)),
            display: DisplayState::new(),
            display_output_enabled: Arc::new(AtomicBool::new(display_output_enabled)),
            osf_import: ImportHandle::new(),
            record: RecordHandle::new(),
            ui_state: ui_state::UiStateStream::new(),
            card_stream: SummaryStream::new(),
        }
    }

    pub fn display_output_enabled(&self) -> bool {
        self.display_output_enabled.load(Ordering::Relaxed)
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

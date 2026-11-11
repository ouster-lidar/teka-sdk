//! NVIDIA devtools app — SSH dev environment and MC web UI backend.

pub mod api;
pub mod keys;
pub mod tegrastats;

pub const SSH_PORT: u16 = 2223;

::teka::declare_app_runtime!("nvidia-devtools");

use std::sync::Arc;

use crate::keys::KeyStore;
use crate::tegrastats::TegraMonitor;

#[derive(Clone)]
pub struct AppState {
    pub keys: Arc<KeyStore>,
    pub tegra: TegraMonitor,
}

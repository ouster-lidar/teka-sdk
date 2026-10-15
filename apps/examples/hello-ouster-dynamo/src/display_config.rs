//! Persisted HDMI/browser display toggles — `{state_dir}/display-config.json`.
//!
//! Survives reboots via systemd `StateDirectory=hello-ouster-dynamo`.
//! The Sway/SimpleViz unit starts with the app; disable the app to stop it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub fn state_dir() -> PathBuf {
    std::env::var_os("STATE_DIRECTORY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tmp"))
}

const FILE_NAME: &str = "display-config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedDisplayConfig {
    /// Show SimpleViz on the physical HDMI/DP panel.
    #[serde(default = "default_true")]
    pub hdmi: bool,
    /// JPEG stream to the browser Display panel.
    #[serde(default = "default_true")]
    pub stream: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PersistedDisplayConfig {
    fn default() -> Self {
        Self {
            hdmi: true,
            stream: true,
        }
    }
}

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE_NAME)
}

pub fn load(state_dir: &Path) -> PersistedDisplayConfig {
    let p = path(state_dir);
    let raw = match fs::read_to_string(&p) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return PersistedDisplayConfig::default();
        }
        Err(e) => {
            warn!("failed to read {}: {e}", p.display());
            return PersistedDisplayConfig::default();
        }
    };
    match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to parse {}: {e}", p.display());
            PersistedDisplayConfig::default()
        }
    }
}

pub fn save(state_dir: &Path, cfg: &PersistedDisplayConfig) -> Result<(), String> {
    let p = path(state_dir);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&p, body).map_err(|e| format!("write {}: {e}", p.display()))?;
    info!(
        hdmi = cfg.hdmi,
        stream = cfg.stream,
        "saved display config to {}",
        p.display()
    );
    Ok(())
}

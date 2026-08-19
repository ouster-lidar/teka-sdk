//! Persisted HDMI/browser display-output enable — `{state_dir}/display-config.json`.
//!
//! Survives reboots via systemd `StateDirectory=hello-ouster-world`.

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
/// Presence gates `hello-ouster-world-display.service` (`ConditionPathExists=`).
pub const ENABLED_FLAG: &str = "display.enabled";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedDisplayConfig {
    /// When false, Sway/SimpleViz display unit is not started (no DRM seat claim).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for PersistedDisplayConfig {
    fn default() -> Self {
        Self { enabled: true }
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

pub fn sync_enabled_flag(state_dir: &Path, enabled: bool) -> Result<(), String> {
    let flag = state_dir.join(ENABLED_FLAG);
    if let Some(parent) = flag.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    if enabled {
        fs::write(&flag, b"").map_err(|e| format!("write {}: {e}", flag.display()))?;
    } else if flag.exists() {
        fs::remove_file(&flag).map_err(|e| format!("remove {}: {e}", flag.display()))?;
    }
    Ok(())
}

pub fn save(state_dir: &Path, enabled: bool) -> Result<(), String> {
    let p = path(state_dir);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let cfg = PersistedDisplayConfig { enabled };
    let body = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    fs::write(&p, body).map_err(|e| format!("write {}: {e}", p.display()))?;
    sync_enabled_flag(state_dir, enabled)?;
    info!(enabled, "saved display config to {}", p.display());
    Ok(())
}

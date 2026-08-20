//! Persisted FrameSet source selection — `{state_dir}/source-config.json`.
//! Runtime bridge status — `{runtime_dir}/source-runtime.json` (Python viz).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};

const FILE_NAME: &str = "source-config.json";
const RUNTIME_FILE_NAME: &str = "source-runtime.json";

pub const SOURCE_LOCAL_FILE: &str = "local_file";
pub const SOURCE_APP: &str = "app";
pub const SOURCE_SENSOR: &str = "sensor";
pub const PRODUCER_OSF: &str = "osf";
pub const PRODUCER_SENSOR: &str = "sensor";
pub const DEFAULT_DYNAMO_APP_SOCKET: &str = "/run/ouster-dynamo/frameset.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSourceConfig {
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub osf_path: String,
    #[serde(default)]
    pub sensor_hostname: String,
    #[serde(default = "default_app_socket")]
    pub app_socket: String,
    /// Dynamo ingest (`osf` | `sensor`). Always the lidar origin (OSF vs live).
    #[serde(default = "default_producer_kind")]
    pub producer_kind: String,
    /// SimpleViz consumes Dynamo FrameSets instead of opening lidar itself.
    /// `None` on disk = infer from ``source == app`` (legacy files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_dynamo: Option<bool>,
    /// Apply with sensor source: SimpleViz saves frames to ``record_filename`` (or auto name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_enabled: Option<bool>,
    /// Basename under ``/srv/recordings/``; empty = ``hello-ouster-world-YYYYMMDD-HHMMSS.osf``.
    #[serde(default)]
    pub record_filename: String,
    /// When false, SimpleViz idles until Apply re-enables (Stop button).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_enabled: Option<bool>,
}

fn default_source() -> String {
    SOURCE_LOCAL_FILE.to_string()
}

fn default_app_socket() -> String {
    DEFAULT_DYNAMO_APP_SOCKET.to_string()
}

fn default_producer_kind() -> String {
    PRODUCER_OSF.to_string()
}

fn normalize_producer_kind(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        PRODUCER_SENSOR => PRODUCER_SENSOR.to_string(),
        _ => PRODUCER_OSF.to_string(),
    }
}

impl Default for PersistedSourceConfig {
    fn default() -> Self {
        Self {
            source: default_source(),
            osf_path: String::new(),
            sensor_hostname: String::new(),
            app_socket: default_app_socket(),
            producer_kind: default_producer_kind(),
            via_dynamo: None,
            record_enabled: None,
            record_filename: String::new(),
            source_enabled: None,
        }
    }
}

impl PersistedSourceConfig {
    pub fn is_via_dynamo(&self) -> bool {
        self.via_dynamo.unwrap_or(false)
    }

    pub fn normalize(mut self) -> Self {
        let src = self.source.trim().to_ascii_lowercase();
        let src = match src.as_str() {
            SOURCE_APP => SOURCE_APP.to_string(),
            SOURCE_SENSOR => SOURCE_SENSOR.to_string(),
            _ => SOURCE_LOCAL_FILE.to_string(),
        };
        let via = self.via_dynamo.unwrap_or(src == SOURCE_APP);
        self.producer_kind = match self.via_dynamo {
            Some(_) => normalize_producer_kind(&self.producer_kind),
            None if src == SOURCE_SENSOR => PRODUCER_SENSOR.to_string(),
            None if src == SOURCE_LOCAL_FILE => PRODUCER_OSF.to_string(),
            None => normalize_producer_kind(&self.producer_kind),
        };
        self.via_dynamo = Some(via);
        self.source = if via {
            SOURCE_APP.to_string()
        } else if self.producer_kind == PRODUCER_SENSOR {
            SOURCE_SENSOR.to_string()
        } else {
            SOURCE_LOCAL_FILE.to_string()
        };
        self.osf_path = self.osf_path.trim().to_string();
        if via || self.app_socket.trim().is_empty() {
            self.app_socket = DEFAULT_DYNAMO_APP_SOCKET.to_string();
        }
        self.sensor_hostname = self.sensor_hostname.trim().to_string();
        self.record_filename = self.record_filename.trim().to_string();
        self
    }

    pub fn app_socket_available(&self) -> bool {
        Path::new(&self.app_socket).exists()
    }

    /// Dashboard card line: lidar origin + Dynamo toggle.
    pub fn dashboard_summary(&self) -> String {
        let cfg = self.clone().normalize();
        let lidar = if cfg.producer_kind == PRODUCER_SENSOR {
            "Sensor"
        } else {
            "Local file"
        };
        let dynamo = if cfg.is_via_dynamo() {
            "Dynamo on"
        } else {
            "Dynamo off"
        };
        format!("{lidar} · {dynamo}")
    }
}

/// Viz-side bridge lifecycle written by Python (`waiting` / `connected` / …).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceRuntimeStatus {
    #[serde(default)]
    pub source: String,
    #[serde(default = "default_bridge_na")]
    pub bridge: String,
    #[serde(default)]
    pub message: String,
}

fn default_bridge_na() -> String {
    "n/a".to_string()
}

fn runtime_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/run/hello-ouster-world".into()),
    )
}

pub fn runtime_path() -> PathBuf {
    runtime_dir().join(RUNTIME_FILE_NAME)
}

pub fn load_runtime() -> SourceRuntimeStatus {
    let p = runtime_path();
    let raw = match fs::read_to_string(&p) {
        Ok(s) => s,
        Err(_) => return SourceRuntimeStatus::default(),
    };
    match serde_json::from_str::<SourceRuntimeStatus>(&raw) {
        Ok(s) => s,
        Err(e) => {
            warn!("failed to parse {}: {e}", p.display());
            SourceRuntimeStatus::default()
        }
    }
}

pub fn runtime_json(rt: &SourceRuntimeStatus) -> serde_json::Value {
    json!({
        "source": rt.source,
        "bridge": rt.bridge,
        "message": rt.message,
    })
}

pub fn persisted_json(cfg: &PersistedSourceConfig) -> serde_json::Value {
    json!({
        "source": cfg.source,
        "osf_path": cfg.osf_path,
        "sensor_hostname": cfg.sensor_hostname,
        "producer_kind": cfg.producer_kind,
        "via_dynamo": cfg.is_via_dynamo(),
        "app_socket": cfg.app_socket,
        "record_enabled": cfg.record_enabled.unwrap_or(false),
        "record_filename": cfg.record_filename,
        "source_enabled": cfg.source_enabled.unwrap_or(true),
        "app_socket_available": cfg.app_socket_available(),
        "valid_sources": [
            SOURCE_LOCAL_FILE,
            SOURCE_APP,
            SOURCE_SENSOR,
        ],
        "app_presets": [
            {
                "id": "ouster_dynamo",
                "label": "ouster-dynamo",
                "app_socket": DEFAULT_DYNAMO_APP_SOCKET,
            },
        ],
    })
}

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE_NAME)
}

pub fn load(state_dir: &Path) -> PersistedSourceConfig {
    let p = path(state_dir);
    let raw = match fs::read_to_string(&p) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return PersistedSourceConfig::default();
        }
        Err(e) => {
            warn!("failed to read {}: {e}", p.display());
            return PersistedSourceConfig::default();
        }
    };
    match serde_json::from_str::<PersistedSourceConfig>(&raw) {
        Ok(c) => c.normalize(),
        Err(e) => {
            warn!("failed to parse {}: {e}", p.display());
            PersistedSourceConfig::default()
        }
    }
}

pub fn save(state_dir: &Path, cfg: &PersistedSourceConfig) -> Result<(), String> {
    let p = path(state_dir);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let normalized = cfg.clone().normalize();
    let body = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
    fs::write(&p, body).map_err(|e| format!("write {}: {e}", p.display()))?;
    info!(source = %normalized.source, "saved source config to {}", p.display());
    Ok(())
}

/// POST Dynamo `/source-config` when SimpleViz is consuming Dynamo FrameSets.
///
/// hello-ouster-world save must succeed even if Dynamo is down. Direct lidar (via off) skips
/// the POST so UDP/OSF is not opened twice.
pub async fn push_to_dynamo(cfg: &PersistedSourceConfig) -> serde_json::Value {
    if !cfg.is_via_dynamo() {
        return json!({
            "ok": true,
            "skipped": true,
            "reason": "how_owns_lidar",
        });
    }
    let kind = normalize_producer_kind(&cfg.producer_kind);
    let url = format!(
        "{}/api/v1/app/ouster-dynamo/source-config",
        mc_portable_runtime::runtime::mc_base_url().trim_end_matches('/')
    );
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .connect_timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("dynamo HTTP client: {e}");
            return json!({ "ok": false, "error": e.to_string() });
        }
    };
    let body = json!({
        "kind": kind,
        "osf_path": cfg.osf_path,
        "sensor_hostname": cfg.sensor_hostname,
    });
    match client.post(&url).json(&body).send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                json!({ "ok": true, "skipped": false, "status": status.as_u16() })
            } else {
                warn!(%status, %url, "dynamo source-config POST failed");
                json!({
                    "ok": false,
                    "status": status.as_u16(),
                    "error": format!("HTTP {status}"),
                })
            }
        }
        Err(e) => {
            warn!("dynamo source-config POST: {e}");
            json!({ "ok": false, "error": e.to_string() })
        }
    }
}

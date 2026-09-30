//! Persisted direct source selection and the status shared with the display.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};

const FILE_NAME: &str = "source-config.json";
const RUNTIME_FILE_NAME: &str = "source-runtime.json";

pub const SOURCE_LOCAL_FILE: &str = "local_file";
pub const SOURCE_SENSOR: &str = "sensor";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSourceConfig {
    pub source: String,
    pub osf_path: String,
    pub sensor_hostname: String,
    pub record_enabled: bool,
    pub record_filename: String,
    pub record_duration_seconds: u64,
    pub source_enabled: bool,
}

impl Default for PersistedSourceConfig {
    fn default() -> Self {
        Self {
            source: SOURCE_SENSOR.to_string(),
            osf_path: String::new(),
            sensor_hostname: String::new(),
            record_enabled: false,
            record_filename: String::new(),
            record_duration_seconds: 120,
            source_enabled: true,
        }
    }
}

impl PersistedSourceConfig {
    pub fn normalize(mut self) -> Result<Self, String> {
        self.source = match self.source.trim().to_ascii_lowercase().as_str() {
            SOURCE_LOCAL_FILE => SOURCE_LOCAL_FILE.to_string(),
            SOURCE_SENSOR => SOURCE_SENSOR.to_string(),
            _ => return Err("unsupported source value".into()),
        };
        self.osf_path = self.osf_path.trim().to_string();
        self.sensor_hostname = self.sensor_hostname.trim().to_string();
        self.record_filename = self.record_filename.trim().to_string();
        if !self.osf_path.is_empty() {
            let path = Path::new(&self.osf_path);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.parent() != Some(&crate::osf::osf_dir())
                || crate::osf::sanitize_osf_filename(name).is_err()
            {
                return Err("osf_path must be an .osf file in the recordings directory".into());
            }
        }
        if self.sensor_hostname.len() > 253
            || !self
                .sensor_hostname
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'))
        {
            return Err("invalid sensor hostname".into());
        }
        if !(1..=3600).contains(&self.record_duration_seconds) {
            return Err("record duration must be between 1 and 3600 seconds".into());
        }
        if self.source != SOURCE_SENSOR {
            self.record_enabled = false;
        }
        Ok(self)
    }

    pub fn dashboard_summary(&self) -> String {
        if !self.source_enabled {
            return "Source stopped".into();
        }
        match self.source.as_str() {
            SOURCE_SENSOR if self.sensor_hostname.is_empty() => "Discovering sensors".into(),
            SOURCE_SENSOR => "Sensor".into(),
            _ => "Local file".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedSourceConfig {
    pub config: PersistedSourceConfig,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceRuntimeStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub message: String,
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
    let path = runtime_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return SourceRuntimeStatus::default(),
    };
    match serde_json::from_str(&raw) {
        Ok(status) => status,
        Err(error) => {
            warn!("failed to parse {}: {error}", path.display());
            SourceRuntimeStatus::default()
        }
    }
}

pub fn set_runtime_status(state: &str, message: &str) -> Result<(), String> {
    let path = runtime_path();
    let status = SourceRuntimeStatus {
        state: state.to_string(),
        message: message.to_string(),
    };
    atomic_write(
        &path,
        &serde_json::to_vec(&status).map_err(|e| e.to_string())?,
    )
}

pub fn runtime_json(status: &SourceRuntimeStatus) -> serde_json::Value {
    let controls = fs::read_to_string(runtime_dir().join("viz-controls.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
    json!({ "state": status.state, "message": status.message, "controls": controls })
}

pub fn persisted_json(cfg: &PersistedSourceConfig, error: Option<&str>) -> serde_json::Value {
    json!({
        "source": cfg.source,
        "osf_path": cfg.osf_path,
        "sensor_hostname": cfg.sensor_hostname,
        "record_enabled": cfg.record_enabled,
        "record_filename": cfg.record_filename,
        "record_duration_seconds": cfg.record_duration_seconds,
        "source_enabled": cfg.source_enabled,
        "config_error": error,
        "valid_sources": [SOURCE_LOCAL_FILE, SOURCE_SENSOR],
    })
}

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE_NAME)
}

pub fn load_details(state_dir: &Path) -> LoadedSourceConfig {
    let path = path(state_dir);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedSourceConfig {
                config: PersistedSourceConfig::default(),
                error: None,
            };
        }
        Err(error) => {
            return LoadedSourceConfig {
                config: PersistedSourceConfig::default(),
                error: Some(format!("cannot read source configuration: {error}")),
            };
        }
    };
    let loaded = serde_json::from_str::<PersistedSourceConfig>(&raw)
        .map_err(|error| format!("invalid source configuration: {error}"))
        .and_then(PersistedSourceConfig::normalize);
    match loaded {
        Ok(config) => LoadedSourceConfig {
            config,
            error: None,
        },
        Err(error) => {
            warn!("{}: {error}", path.display());
            LoadedSourceConfig {
                config: PersistedSourceConfig::default(),
                error: Some(error),
            }
        }
    }
}

pub fn load(state_dir: &Path) -> PersistedSourceConfig {
    load_details(state_dir).config
}

pub fn save(state_dir: &Path, cfg: &PersistedSourceConfig) -> Result<(), String> {
    let path = path(state_dir);
    let config = cfg.clone().normalize()?;
    let body = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    atomic_write(&path, &body)?;
    info!(source = %config.source, "saved source config to {}", path.display());
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, body: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    let tmp = path.with_extension(format!(
        "{}.{}.tmp",
        path.extension().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let mut file =
        fs::File::create(&tmp).map_err(|error| format!("create {}: {error}", tmp.display()))?;
    file.write_all(body)
        .map_err(|error| format!("write {}: {error}", tmp.display()))?;
    file.sync_all()
        .map_err(|error| format!("sync {}: {error}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|error| format!("rename {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{load_details, save, PersistedSourceConfig, SOURCE_LOCAL_FILE, SOURCE_SENSOR};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("hello-world-source-{id}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_config_defaults_to_enabled_sensor_discovery() {
        let dir = test_dir();
        let loaded = load_details(&dir);
        assert!(loaded.error.is_none());
        assert_eq!(loaded.config.source, SOURCE_SENSOR);
        assert!(loaded.config.source_enabled);
        assert!(!loaded.config.record_enabled);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recording_duration_defaults_and_bounds() {
        let old: PersistedSourceConfig = serde_json::from_str(r#"{"source":"sensor"}"#).unwrap();
        assert_eq!(old.record_duration_seconds, 120);
        for seconds in [1, 120, 3600] {
            assert!(PersistedSourceConfig {
                record_duration_seconds: seconds,
                ..Default::default()
            }.normalize().is_ok());
        }
        for seconds in [0, 3601] {
            assert!(PersistedSourceConfig {
                record_duration_seconds: seconds,
                ..Default::default()
            }.normalize().is_err());
        }
    }

    #[test]
    fn source_targets_are_confined() {
        let ok = |osf_path: &str, sensor_hostname: &str| {
            PersistedSourceConfig {
                osf_path: osf_path.into(),
                sensor_hostname: sensor_hostname.into(),
                ..Default::default()
            }
            .normalize()
            .is_ok()
        };
        assert!(ok("/srv/recordings/a.osf", "os-122.local"));
        assert!(ok("", "192.168.1.5"));
        for path in ["/etc/passwd", "/srv/recordings/../etc/x.osf", "/srv/recordings/a.txt", "/srv/recordings/sub/a.osf"] {
            assert!(!ok(path, ""), "{path}");
        }
        for host in ["a b", "http://x/", "x;y", "a/b"] {
            assert!(!ok("", host), "{host}");
        }
    }

    #[test]
    fn saved_source_and_stop_state_survive_load() {
        let dir = test_dir();
        let cfg = PersistedSourceConfig {
            source: SOURCE_LOCAL_FILE.into(),
            osf_path: "/srv/recordings/drive.osf".into(),
            source_enabled: false,
            ..Default::default()
        };
        save(&dir, &cfg).unwrap();
        let loaded = load_details(&dir);
        assert!(loaded.error.is_none());
        assert_eq!(loaded.config.osf_path, "/srv/recordings/drive.osf");
        assert!(!loaded.config.source_enabled);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_and_unsupported_configs_are_reported() {
        let dir = test_dir();
        fs::write(dir.join("source-config.json"), "{").unwrap();
        assert!(load_details(&dir).error.is_some());
        fs::write(dir.join("source-config.json"), r#"{"source":"app"}"#).unwrap();
        assert_eq!(
            load_details(&dir).error.as_deref(),
            Some("unsupported source value")
        );
        let _ = fs::remove_dir_all(dir);
    }
}

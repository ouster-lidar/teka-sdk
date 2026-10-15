//! In-process OSF save: SimpleViz writes `/srv/recordings/hello-ouster-dynamo-YYYYMMDD-HHMMSS.osf`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::source_config::PersistedSourceConfig;
use crate::ui_state;
use crate::AppState;

const RUNTIME_FILE_NAME: &str = "record-runtime.json";

#[derive(Debug, Clone)]
struct RecordStatus {
    active: bool,
    path: String,
    filename: String,
    started_at: Option<u64>,
    error: Option<String>,
}

impl RecordStatus {
    fn idle() -> Self {
        Self {
            active: false,
            path: String::new(),
            filename: String::new(),
            started_at: None,
            error: None,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "active": self.active,
            "path": self.path,
            "filename": self.filename,
            "started_at": self.started_at,
            "error": self.error,
        })
    }
}

pub struct RecordHandle {
    inner: Arc<Mutex<RecordStatus>>,
}

impl Clone for RecordHandle {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Default for RecordHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordHandle {
    pub fn new() -> Self {
        let h = Self {
            inner: Arc::new(Mutex::new(RecordStatus::idle())),
        };
        let _ = write_runtime(&RecordStatus::idle());
        h
    }

    pub fn status_json(&self) -> Value {
        self.inner.lock().to_json()
    }

    pub fn is_active(&self) -> bool {
        self.inner.lock().active
    }

    /// Start after ``source-config.json`` was saved with ``record_enabled`` (Apply flow).
    pub fn start_from_config(
        &self,
        state: &AppState,
        cfg: &PersistedSourceConfig,
    ) -> Result<Value, String> {
        if cfg.sensor_hostname.trim().is_empty() {
            return Err("sensor_hostname is required for recording".into());
        }
        if self.is_active() {
            return Ok(self.status_json());
        }

        let (osf_path, filename) = allocate_recording_path(cfg.record_filename.trim())?;
        let st = RecordStatus {
            active: true,
            path: osf_path.to_string_lossy().into_owned(),
            filename,
            started_at: Some(unix_now()),
            error: None,
        };
        *self.inner.lock() = st.clone();
        write_runtime(&st)?;
        info!(path = %st.path, "recording started (SimpleViz save)");
        ui_state::publish_ui(state);
        Ok(st.to_json())
    }

    pub fn stop(&self, state: &AppState) -> Value {
        let mut st = self.inner.lock().clone();
        st.active = false;
        *self.inner.lock() = st.clone();
        if let Err(e) = write_runtime(&st) {
            warn!("record runtime write on stop: {e}");
        }
        info!("recording stopped");
        ui_state::publish_ui(state);
        st.to_json()
    }
}

fn write_runtime(status: &RecordStatus) -> Result<(), String> {
    let dir = PathBuf::from(
        std::env::var("HELLO_OUSTER_DYNAMO_XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/run/hello-ouster-dynamo".into()),
    );
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(RUNTIME_FILE_NAME);
    let body = serde_json::to_string_pretty(&status.to_json()).map_err(|e| e.to_string())?;
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sanitize_record_basename(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Ok(String::new());
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("record filename must be a basename only".into());
    }
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    if base.is_empty() {
        return Err("invalid record filename".into());
    }
    let with_ext = if base.to_ascii_lowercase().ends_with(".osf") {
        base.to_string()
    } else {
        format!("{base}.osf")
    };
    Ok(with_ext)
}

fn allocate_recording_path(filename_override: &str) -> Result<(PathBuf, String), String> {
    let dir = crate::osf::osf_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    if !filename_override.is_empty() {
        let name = sanitize_record_basename(filename_override)?;
        let path = dir.join(&name);
        return Ok((path, name));
    }
    let secs = unix_now();
    let (y, mo, d, h, mi, s) = utc_ymd_hms(secs);
    let date = format!("{y:04}{mo:02}{d:02}");
    let name = format!("hello-ouster-dynamo-{date}-{h:02}{mi:02}{s:02}.osf");
    let path = dir.join(&name);
    if !path.exists() {
        return Ok((path, name));
    }
    let alt = format!("hello-ouster-dynamo-{date}-{h:02}{mi:02}{s:02}-2.osf");
    Ok((dir.join(&alt), alt))
}

fn utc_ymd_hms(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = secs / 86400;
    let time = secs % 86400;
    let h = (time / 3600) as u32;
    let mi = ((time % 3600) / 60) as u32;
    let s = (time % 60) as u32;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mp < 10 { y } else { y + 1 };
    (y as u32, mo as u32, d as u32, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::{sanitize_record_basename, utc_ymd_hms};

    #[test]
    fn utc_ymd_formats_epoch() {
        assert_eq!(utc_ymd_hms(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn sanitize_basename_adds_osf() {
        assert_eq!(sanitize_record_basename("run-1").unwrap(), "run-1.osf");
    }
}

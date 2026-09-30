//! In-process OSF save: SimpleViz writes `/srv/recordings/hello-ouster-world-YYYYMMDD-HHMMSS.osf`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde_json::{json, Value};
use tracing::{info, warn};
use tokio::time::Instant;

use crate::source_config::PersistedSourceConfig;
use crate::ui_state;
use crate::AppState;

const RUNTIME_FILE_NAME: &str = "record-runtime.json";
const WRITER_CLOSED_FILE_NAME: &str = "record-writer-closed";
const WRITER_CLOSE_FAILED_FILE_NAME: &str = "record-writer-close-failed";
const WRITER_STARTED_FILE_NAME: &str = "record-writer-started";

#[derive(Debug, Clone)]
struct RecordStatus {
    active: bool,
    path: String,
    filename: String,
    started_at: Option<u64>,
    error: Option<String>,
    expires_at: Option<u64>,
    deadline: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WriterClose {
    Closed,
    Failed,
}

impl RecordStatus {
    fn idle() -> Self {
        Self {
            active: false,
            path: String::new(),
            filename: String::new(),
            started_at: None,
            error: None,
            expires_at: None,
            deadline: None,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "active": self.active,
            "path": self.path,
            "filename": self.filename,
            "started_at": self.started_at,
            "error": self.error,
            "expires_at": self.expires_at,
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

    pub fn active_filename(&self) -> Option<String> {
        let status = self.inner.lock();
        status.active.then(|| status.filename.clone())
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
        clear_writer_closed()?;
        let duration = Duration::from_secs(cfg.record_duration_seconds);
        let deadline = Instant::now() + duration;
        let st = RecordStatus {
            active: true,
            path: osf_path.to_string_lossy().into_owned(),
            filename,
            started_at: Some(unix_now()),
            error: None,
            expires_at: Some(unix_millis() + cfg.record_duration_seconds * 1000),
            deadline: Some(deadline),
        };
        write_runtime(&st)?;
        *self.inner.lock() = st.clone();
        let timer_state = state.clone();
        tokio::spawn(async move {
            let mut shutdown = timer_state.shutdown.subscribe();
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {},
                _ = shutdown.changed() => return,
            }
            let _operation = timer_state.source_operation.lock().await;
            let current = timer_state.record.inner.lock().clone();
            if current.active && current.deadline == Some(deadline) {
                if let Err(error) = timer_state.record.stop(&timer_state).await {
                    warn!(%error, "record duration stop failed");
                    timer_state.record.inner.lock().error = Some(error);
                    ui_state::publish_ui(&timer_state);
                }
            }
        });
        info!(path = %st.path, "recording started (SimpleViz save)");
        ui_state::publish_ui(state);
        Ok(st.to_json())
    }

    pub async fn stop(&self, state: &AppState) -> Result<Value, String> {
        let mut st = self.inner.lock().clone();
        if !st.active {
            return Ok(st.to_json());
        }

        let mut runtime_status = st.clone();
        runtime_status.active = false;
        write_runtime(&runtime_status)?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        if !tokio::fs::read_to_string(writer_started_path())
            .await
            .is_ok_and(|started| started.trim() == st.path.as_str())
        {
            st.active = false;
            st.error = Some("recording ended before any frames were received".into());
            *self.inner.lock() = st.clone();
            if let Err(e) = write_runtime(&st) {
                warn!("record runtime write on stop: {e}");
            }
            ui_state::publish_ui(state);
            return Ok(st.to_json());
        }
        let close = match wait_for_writer_close(&st.path, Some(Duration::from_secs(15))).await {
            Ok(close) => close,
            Err(error) => {
                // Keep the file busy while the writer finalizes, without restoring save intent.
                st.error = Some(error.clone());
                *self.inner.lock() = st.clone();
                ui_state::publish_ui(state);
                let finalize_state = state.clone();
                tokio::spawn(async move {
                    let mut shutdown = finalize_state.shutdown.subscribe();
                    let close = tokio::select! {
                        close = wait_for_writer_close(&st.path, None) => match close {
                            Ok(close) => close,
                            Err(_) => return,
                        },
                        _ = shutdown.changed() => return,
                    };
                    let _operation = finalize_state.source_operation.lock().await;
                    let current = finalize_state.record.inner.lock().clone();
                    if current.active && current.path == st.path && current.deadline == st.deadline
                    {
                        finalize_state
                            .record
                            .finish_stop(&finalize_state, st, close);
                    }
                });
                return Err(error);
            }
        };
        Ok(self.finish_stop(state, st, close))
    }

    fn finish_stop(&self, state: &AppState, mut st: RecordStatus, close: WriterClose) -> Value {
        st.active = false;
        st.error = None;
        if close == WriterClose::Failed {
            st.error = Some("SimpleViz failed to close the recording; OSF is unavailable".into());
            if let Err(error) = crate::osf::mark_recording_incomplete(&st.filename) {
                warn!(%error, "failed to mark incomplete recording unavailable");
            }
        } else {
            crate::osf::clear_incomplete_recording(&st.filename);
        }
        *self.inner.lock() = st.clone();
        if let Err(e) = write_runtime(&st) {
            warn!("record runtime write on stop: {e}");
        }
        info!(path = %st.path, "recording stopped");
        ui_state::publish_ui(state);
        st.to_json()
    }
}

fn runtime_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/run/hello-ouster-world".into()),
    )
}

fn writer_closed_path() -> PathBuf {
    runtime_dir().join(WRITER_CLOSED_FILE_NAME)
}

fn writer_started_path() -> PathBuf {
    runtime_dir().join(WRITER_STARTED_FILE_NAME)
}

fn writer_close_failed_path() -> PathBuf {
    runtime_dir().join(WRITER_CLOSE_FAILED_FILE_NAME)
}

fn clear_writer_closed() -> Result<(), String> {
    for path in [
        writer_closed_path(),
        writer_close_failed_path(),
        writer_started_path(),
    ] {
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("remove writer-close state: {e}")),
        }
    }
    Ok(())
}

async fn wait_for_writer_close(
    path: &str,
    timeout: Option<Duration>,
) -> Result<WriterClose, String> {
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    loop {
        if tokio::fs::read_to_string(writer_closed_path())
            .await
            .is_ok_and(|closed| closed.trim() == path)
        {
            return Ok(WriterClose::Closed);
        }
        if tokio::fs::read_to_string(writer_close_failed_path())
            .await
            .is_ok_and(|failed| failed.trim() == path)
        {
            return Ok(WriterClose::Failed);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(format!("timed out waiting for SimpleViz to close {path}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn write_runtime(status: &RecordStatus) -> Result<(), String> {
    let dir = runtime_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(RUNTIME_FILE_NAME);
    let body = serde_json::to_string_pretty(&status.to_json()).map_err(|e| e.to_string())?;
    crate::source_config::atomic_write(&path, body.as_bytes())
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
    let name = format!("hello-ouster-world-{date}-{h:02}{mi:02}{s:02}.osf");
    let path = dir.join(&name);
    if !path.exists() {
        return Ok((path, name));
    }
    let alt = format!("hello-ouster-world-{date}-{h:02}{mi:02}{s:02}-2.osf");
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
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::{sanitize_record_basename, utc_ymd_hms};

    #[test]
    fn utc_ymd_formats_epoch() {
        assert_eq!(utc_ymd_hms(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn sanitize_basename_adds_osf() {
        assert_eq!(sanitize_record_basename("run-1").unwrap(), "run-1.osf");
    }

    #[test]
    fn active_record_exposes_only_its_own_filename() {
        let record = super::RecordHandle {
            inner: Arc::new(Mutex::new(super::RecordStatus {
                active: true,
                path: "/srv/recordings/run-1.osf".into(),
                filename: "run-1.osf".into(),
                started_at: Some(1),
                ..super::RecordStatus::idle()
            })),
        };
        assert_eq!(record.active_filename().as_deref(), Some("run-1.osf"));

        let idle = super::RecordHandle {
            inner: Arc::new(Mutex::new(super::RecordStatus::idle())),
        };
        assert_eq!(idle.active_filename(), None);
    }
}

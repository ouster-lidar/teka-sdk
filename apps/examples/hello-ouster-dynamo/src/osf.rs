//! OSF collection under ``/srv/recordings`` (host bind, not StateDirectory):
//! list, client upload, device Studio import.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ::app_runtime::api_error::ApiError;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path as AxumPath, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::stream::unfold;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use crate::AppState;

pub fn osf_dir() -> PathBuf {
    PathBuf::from("/srv/recordings")
}

pub const SAMPLE_LIBRARY_URL: &str =
    "https://studio.ouster.com/public/sample_library/public_collection";

const FREEWAY_NAME: &str = "Freeway_with_Billboards_in_Native_Color_4096x5Hz.osf";
const FREEWAY_SHARE: &str = "https://studio.ouster.com/share/2W8AYGFCF2F0L3OY";
const URBAN_NAME: &str = "Urban_Drive_in_Pacific_Heights_San_Francisco_2048x10Hz.osf";
const URBAN_SHARE: &str = "https://studio.ouster.com/share/OA0R1CJRZSQKEWIN";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsfSeed {
    pub name: String,
    pub share_url: String,
    pub present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsfFileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStatus {
    pub state: String,
    pub name: String,
    pub downloaded: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ImportStatus {
    fn idle() -> Self {
        Self {
            state: "idle".into(),
            name: String::new(),
            downloaded: 0,
            total: 0,
            error: None,
        }
    }
}

#[derive(Clone)]
pub struct ImportHandle {
    inner: Arc<Mutex<ImportStatus>>,
}

impl Default for ImportHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ImportHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ImportStatus::idle())),
        }
    }

    pub fn snapshot(&self) -> ImportStatus {
        self.inner.lock().clone()
    }

    fn try_begin(&self, name: &str) -> Result<(), ApiError> {
        let mut g = self.inner.lock();
        if g.state == "running" {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "conflict",
                "an OSF import is already running",
            ));
        }
        *g = ImportStatus {
            state: "running".into(),
            name: name.to_string(),
            downloaded: 0,
            total: 0,
            error: None,
        };
        Ok(())
    }

    fn set_progress(&self, downloaded: u64, total: u64) {
        let mut g = self.inner.lock();
        if g.state == "running" {
            g.downloaded = downloaded;
            g.total = total;
        }
    }

    fn finish_ok(&self) {
        let mut g = self.inner.lock();
        g.state = "idle".into();
        g.error = None;
    }

    fn finish_err(&self, msg: String) {
        let mut g = self.inner.lock();
        g.state = "error".into();
        g.error = Some(msg);
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/osfs", get(list_osfs))
        .route("/osfs/imports", get(get_import).post(post_import))
        .route(
            "/osfs/{name}",
            get(get_osf)
                .post(post_osf)
                .delete(delete_osf)
                .layer(DefaultBodyLimit::disable()),
        )
}

fn seeds() -> Vec<OsfSeed> {
    vec![
        OsfSeed {
            name: FREEWAY_NAME.into(),
            share_url: FREEWAY_SHARE.into(),
            present: false,
        },
        OsfSeed {
            name: URBAN_NAME.into(),
            share_url: URBAN_SHARE.into(),
            present: false,
        },
    ]
}

fn osf_path(name: &str) -> PathBuf {
    osf_dir().join(name)
}

fn file_ok(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

async fn ensure_osf_dir() -> Result<(), ApiError> {
    fs::create_dir_all(osf_dir()).await.map_err(io_err)
}

fn io_err(e: std::io::Error) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
}

fn skip_listing_name(name: &str) -> bool {
    name.starts_with('.')
        || name.ends_with(".partial")
        || name.ends_with(".lock")
        || name.ends_with(".tmp")
        || name == ".keep"
        || name == "selected.json"
}

/// Basename only, must end with ``.osf``, no path separators or ``..``.
pub fn sanitize_osf_filename(name: &str) -> Result<String, ApiError> {
    if name.contains('\0') {
        return Err(ApiError::invalid_request("invalid OSF filename"));
    }
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if base.is_empty() || base != name.trim() {
        return Err(ApiError::invalid_request("invalid OSF filename"));
    }
    if base.contains('/') || base.contains('\\') || base.contains("..") {
        return Err(ApiError::invalid_request("invalid OSF filename"));
    }
    if !base.ends_with(".osf") {
        return Err(ApiError::invalid_request("OSF filename must end with .osf"));
    }
    Ok(base.to_string())
}

fn share_id_from_url(url: &str) -> Option<String> {
    let id = url.trim().trim_end_matches('/').rsplit('/').next()?;
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

fn seed_by_name(name: &str) -> Option<OsfSeed> {
    seeds().into_iter().find(|s| s.name == name)
}

async fn list_osf_files() -> Result<Vec<OsfFileEntry>, ApiError> {
    ensure_osf_dir().await?;
    let dir = osf_dir();
    let mut files = Vec::new();
    let mut rd = match fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(io_err(e)),
    };
    while let Some(ent) = rd.next_entry().await.map_err(io_err)? {
        let name = ent.file_name().to_string_lossy().into_owned();
        if skip_listing_name(&name) || !name.ends_with(".osf") {
            continue;
        }
        let meta = ent.metadata().await.map_err(io_err)?;
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        files.push(OsfFileEntry {
            path: osf_path(&name).to_string_lossy().into_owned(),
            name,
            size,
        });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn with_present(mut seed_list: Vec<OsfSeed>) -> Vec<OsfSeed> {
    for seed in &mut seed_list {
        seed.present = file_ok(&osf_path(&seed.name));
    }
    seed_list
}

async fn catalog_json(state: &AppState) -> Result<Value, ApiError> {
    let files = list_osf_files().await?;
    let dir = osf_dir();
    Ok(json!({
        "dir": dir.to_string_lossy(),
        "sample_library_url": SAMPLE_LIBRARY_URL,
        "files": files,
        "seeds": with_present(seeds()),
        "import": state.osf_import.snapshot(),
    }))
}

async fn list_osfs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(catalog_json(&state).await?))
}

async fn get_import(State(state): State<AppState>) -> Json<ImportStatus> {
    Json(state.osf_import.snapshot())
}

#[derive(Debug, Deserialize)]
struct ImportBody {
    name: String,
}

async fn post_import(
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> Result<Json<Value>, ApiError> {
    let name = sanitize_osf_filename(&body.name)?;
    let seed = seed_by_name(&name).ok_or_else(|| {
        ApiError::invalid_request("unknown OSF seed; download a listed demo recording")
    })?;
    ensure_osf_dir().await?;
    let path = osf_path(&name);
    if file_ok(&path) {
        return Ok(Json(json!({
            "ok": true,
            "already_present": true,
            "name": name,
            "path": path.to_string_lossy(),
        })));
    }
    state.osf_import.try_begin(&name)?;
    let handle = state.osf_import.clone();
    tokio::spawn(async move {
        match import_seed(&handle, &seed).await {
            Ok(()) => handle.finish_ok(),
            Err(e) => handle.finish_err(e),
        }
    });
    Ok(Json(json!({
        "ok": true,
        "already_present": false,
        "name": name,
        "path": path.to_string_lossy(),
    })))
}

async fn import_seed(handle: &ImportHandle, seed: &OsfSeed) -> Result<(), String> {
    let share_id =
        share_id_from_url(&seed.share_url).ok_or_else(|| "invalid Studio share URL".to_string())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(6 * 3600))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let studio = format!(
        "https://studio.ouster.com/api/v1/published_recordings/{}",
        share_id
    );
    let files: Value = client
        .get(format!("{studio}/output/files/"))
        .send()
        .await
        .map_err(|e| format!("Studio listing failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Studio listing failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Studio listing JSON: {e}"))?;
    let obj = find_studio_osf(&files).ok_or_else(|| "share has no OSF output".to_string())?;
    let object_id = obj
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "OSF object missing path".to_string())?;
    let listed_size = obj.get("size").and_then(Value::as_u64).unwrap_or(0);
    let signing: Value = client
        .post(format!("{studio}/signings/"))
        .json(&json!({ "object_id": object_id }))
        .send()
        .await
        .map_err(|e| format!("Studio signing failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Studio signing failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Studio signing JSON: {e}"))?;
    let url = signing
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "Studio signing response missing url".to_string())?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("cloud download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("cloud download failed: {e}"))?;
    let total = resp.content_length().unwrap_or(listed_size);
    handle.set_progress(0, total);

    let dest = osf_path(&seed.name);
    let partial = format!("{}.partial", dest.to_string_lossy());
    let mut file = File::create(&partial)
        .await
        .map_err(|e| format!("create {partial}: {e}"))?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_report: u64 = 0;
    let write = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("download stream: {e}"))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("write: {e}"))?;
            downloaded += chunk.len() as u64;
            if downloaded.saturating_sub(last_report) >= 1024 * 1024 {
                handle.set_progress(downloaded, total);
                last_report = downloaded;
            }
        }
        file.flush().await.map_err(|e| format!("flush: {e}"))?;
        Ok::<(), String>(())
    }
    .await;
    drop(file);
    if let Err(e) = write {
        let _ = fs::remove_file(&partial).await;
        return Err(e);
    }
    handle.set_progress(downloaded, total.max(downloaded));
    fs::rename(&partial, &dest)
        .await
        .map_err(|e| format!("rename: {e}"))?;
    tracing::info!(name = %seed.name, downloaded, "OSF import complete");
    Ok(())
}

fn find_studio_osf(files: &Value) -> Option<&Value> {
    let items = if let Some(arr) = files.as_array() {
        arr.as_slice()
    } else if let Some(arr) = files.get("results").and_then(Value::as_array) {
        arr.as_slice()
    } else if let Some(arr) = files.get("files").and_then(Value::as_array) {
        arr.as_slice()
    } else {
        return None;
    };
    items.iter().find(|o| {
        o.get("output_type").and_then(Value::as_str) == Some("osf")
            && o.get("type").and_then(Value::as_str) == Some("osf")
    })
}

const DOWNLOAD_CHUNK: usize = 64 * 1024;

async fn get_osf(AxumPath(name): AxumPath<String>) -> Result<Response, ApiError> {
    let name = sanitize_osf_filename(&name)?;
    let path = osf_path(&name);
    let meta = fs::metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ApiError::new(StatusCode::NOT_FOUND, "not_found", "OSF not on device")
        } else {
            io_err(e)
        }
    })?;
    if !meta.is_file() || meta.len() == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "OSF not on device",
        ));
    }
    let file = File::open(&path).await.map_err(io_err)?;
    let len = meta.len();
    let stream = unfold(file, |mut file| async move {
        let mut buf = vec![0u8; DOWNLOAD_CHUNK];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(Bytes::from(buf)), file))
            }
            Err(e) => Some((Err(e), file)),
        }
    });
    let body = Body::from_stream(stream);
    let disposition = format!("attachment; filename=\"{}\"", name);
    let mut resp = Response::new(body);
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition)
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    if let Ok(v) = HeaderValue::from_str(&len.to_string()) {
        headers.insert(header::CONTENT_LENGTH, v);
    }
    Ok(resp)
}

async fn delete_osf(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let name = sanitize_osf_filename(&name)?;
    let snap = state.osf_import.snapshot();
    if snap.state == "running" && snap.name == name {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "cannot delete while that OSF is downloading",
        ));
    }
    let path = osf_path(&name);
    match fs::remove_file(&path).await {
        Ok(()) => Ok(Json(json!({ "ok": true, "name": name }))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "OSF not on device",
        )),
        Err(e) => Err(io_err(e)),
    }
}

async fn post_osf(AxumPath(name): AxumPath<String>, body: Body) -> Result<Json<Value>, ApiError> {
    let name = sanitize_osf_filename(&name)?;
    ensure_osf_dir().await?;
    let path = osf_path(&name);
    let partial = format!("{}.partial", path.to_string_lossy());

    let mut file = File::create(&partial).await.map_err(io_err)?;
    let mut body = body.into_data_stream();
    let mut size: u64 = 0;
    let write_result: Result<(), ApiError> = async {
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
            })?;
            file.write_all(&chunk).await.map_err(io_err)?;
            size += chunk.len() as u64;
        }
        file.flush().await.map_err(io_err)?;
        Ok(())
    }
    .await;
    drop(file);
    if let Err(e) = write_result {
        let _ = fs::remove_file(&partial).await;
        return Err(e);
    }
    fs::rename(&partial, &path).await.map_err(io_err)?;
    Ok(Json(json!({
        "ok": true,
        "name": name,
        "path": path.to_string_lossy(),
        "size": size,
    })))
}

#[cfg(test)]
mod tests {
    use super::{sanitize_osf_filename, seed_by_name, share_id_from_url, URBAN_NAME};

    #[test]
    fn accepts_valid_basename() {
        assert_eq!(sanitize_osf_filename("demo.osf").unwrap(), "demo.osf");
    }

    #[test]
    fn rejects_path_components() {
        assert!(sanitize_osf_filename("foo/bar.osf").is_err());
        assert!(sanitize_osf_filename("..\\evil.osf").is_err());
        assert!(sanitize_osf_filename("../x.osf").is_err());
    }

    #[test]
    fn rejects_non_osf_extension() {
        assert!(sanitize_osf_filename("demo.osf.partial").is_err());
        assert!(sanitize_osf_filename("readme.txt").is_err());
    }

    #[test]
    fn rejects_nul() {
        assert!(sanitize_osf_filename("bad\0.osf").is_err());
    }

    #[test]
    fn share_id_parses() {
        assert_eq!(
            share_id_from_url("https://studio.ouster.com/share/OA0R1CJRZSQKEWIN").as_deref(),
            Some("OA0R1CJRZSQKEWIN")
        );
    }

    #[test]
    fn urban_seed_matches_extrinsics_name() {
        let s = seed_by_name(URBAN_NAME).unwrap();
        assert_eq!(s.name, URBAN_NAME);
    }
}

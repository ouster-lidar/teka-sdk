//! HTTP + WebSocket API for Console and Display.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use inotify::{EventMask, Inotify, WatchMask};
use mc_portable_runtime::api_error::ApiError;
use mc_portable_runtime::status_stream::{
    serve_broadcast, serve_summary_broadcast_with_initial,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::fs::File;
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::display;
use crate::display_config;
use crate::osf;
use crate::source_config;
use crate::ui_state;
use crate::AppState;

const DEFAULT_SHELL: &str = "/bin/bash";
const DEFAULT_CWD: &str = "/var/lib/hello-ouster-world";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/status/stream", get(status_stream))
        .route("/cards/stream", get(cards_ws))
        .route("/pty", get(pty_ws))
        .route(
            "/source-config",
            get(get_source_config).post(post_source_config),
        )
        .route("/record", get(get_record).post(post_record))
        .merge(display::routes())
        .merge(osf::routes())
}

async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = source_config::load(&display_config::state_dir());
    Json(json!({
        "ok": true,
        "app": "hello-ouster-world",
        "pty_sessions": state.pty_sessions.load(std::sync::atomic::Ordering::Relaxed),
        "pty_max": crate::MAX_PTY_SESSIONS,
        "shell": shell_path(),
        "cwd": cwd_path(),
        "console_log": console_log_path(),
        "display": state.display.status_json(),
        "source": source_config_json(&cfg),
        "record": state.record.status_json(),
        "summary": cfg.dashboard_summary(),
    }))
}

#[derive(Deserialize)]
struct SourceConfigBody {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    osf_path: Option<String>,
    #[serde(default)]
    sensor_hostname: Option<String>,
    #[serde(default)]
    app_socket: Option<String>,
    #[serde(default)]
    producer_kind: Option<String>,
    #[serde(default)]
    via_dynamo: Option<bool>,
    #[serde(default)]
    record_enabled: Option<bool>,
    #[serde(default)]
    record_filename: Option<String>,
    #[serde(default)]
    source_enabled: Option<bool>,
}

fn source_config_json(cfg: &source_config::PersistedSourceConfig) -> serde_json::Value {
    let runtime = source_config::load_runtime();
    let mut v = source_config::persisted_json(cfg);
    v["runtime"] = source_config::runtime_json(&runtime);
    v
}

async fn get_source_config() -> Json<serde_json::Value> {
    let cfg = source_config::load(&display_config::state_dir());
    Json(source_config_json(&cfg))
}

async fn post_source_config(
    State(state): State<AppState>,
    Json(body): Json<SourceConfigBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let state_dir = display_config::state_dir();
    let mut cfg = source_config::load(&state_dir);
    if let Some(s) = body.source {
        cfg.source = s;
    }
    if let Some(p) = body.osf_path {
        cfg.osf_path = p;
    }
    if let Some(h) = body.sensor_hostname {
        cfg.sensor_hostname = h;
    }
    if let Some(s) = body.app_socket {
        cfg.app_socket = s;
    }
    if let Some(k) = body.producer_kind {
        cfg.producer_kind = k;
    }
    if let Some(via) = body.via_dynamo {
        cfg.via_dynamo = Some(via);
    }
    if let Some(rec) = body.record_enabled {
        cfg.record_enabled = Some(rec);
    }
    if let Some(name) = body.record_filename {
        cfg.record_filename = name;
    }
    if let Some(en) = body.source_enabled {
        cfg.source_enabled = Some(en);
    }
    let source_on = cfg.source_enabled.unwrap_or(true);
    let want_record = source_on && cfg.record_enabled.unwrap_or(false);
    if want_record {
        cfg.via_dynamo = Some(false);
        cfg.producer_kind = source_config::PRODUCER_SENSOR.to_string();
    }
    cfg = cfg.normalize();
    source_config::save(&state_dir, &cfg).map_err(|e| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            e,
        )
    })?;
    let dynamo = source_config::push_to_dynamo(&cfg).await;
    if dynamo.get("ok") == Some(&json!(false)) {
        tracing::warn!(%dynamo, "ouster-dynamo source-config push failed (hello-ouster-world save kept)");
    }
    let record = if want_record {
        if cfg.sensor_hostname.trim().is_empty() {
            return Err(ApiError::invalid_request(
                "sensor_hostname is required when recording is enabled",
            ));
        }
        state
            .record
            .start_from_config(&state, &cfg)
            .map_err(|e| ApiError::invalid_request(e))?
    } else if state.record.is_active() {
        state.record.stop(&state)
    } else {
        state.record.status_json()
    };
    // SimpleViz watches source-config.json mtime and execv restarts on Apply.
    ui_state::publish_ui(&state);
    Ok(Json(json!({
        "ok": true,
        "restart_required": true,
        "dynamo": dynamo,
        "record": record,
        "config": source_config_json(&cfg),
    })))
}

#[derive(Deserialize)]
struct RecordBody {
    active: bool,
    #[serde(default)]
    sensor_hostname: Option<String>,
}

async fn get_record(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(state.record.status_json())
}

async fn post_record(
    State(state): State<AppState>,
    Json(body): Json<RecordBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let state_dir = display_config::state_dir();
    let mut cfg = source_config::load(&state_dir);
    cfg.record_enabled = Some(body.active);
    if let Some(h) = body.sensor_hostname {
        let h = h.trim();
        if !h.is_empty() {
            cfg.sensor_hostname = h.to_string();
        }
    }
    if body.active {
        cfg.via_dynamo = Some(false);
        cfg.producer_kind = source_config::PRODUCER_SENSOR.to_string();
    }
    cfg = cfg.normalize();
    source_config::save(&state_dir, &cfg).map_err(|e| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            e,
        )
    })?;
    let rec = if body.active {
        state
            .record
            .start_from_config(&state, &cfg)
            .map_err(|e| ApiError::invalid_request(e))?
    } else {
        state.record.stop(&state)
    };
    ui_state::publish_ui(&state);
    Ok(Json(json!({ "ok": true, "record": rec })))
}

async fn status_stream(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let rx = state.ui_state.tx.subscribe();
    let initial = ui_state::snapshot_message(&ui_state::build_ui_state(&state));
    ws.on_upgrade(move |socket| serve_broadcast(socket, rx, Some(initial)))
}

async fn cards_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let stream = state.card_stream.clone();
    let initial = stream.cache.read().clone().unwrap_or_default();
    ws.on_upgrade(move |socket| async move {
        serve_summary_broadcast_with_initial(socket, stream, initial).await;
    })
}

async fn pty_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| pty_session(socket, state))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[allow(dead_code)]
enum ClientMsg {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ServerMsg {
    Output { data: String },
    Error { message: String },
}

fn shell_path() -> String {
    std::env::var("HELLO_OUSTER_WORLD_SHELL").unwrap_or_else(|_| DEFAULT_SHELL.to_string())
}

fn cwd_path() -> String {
    let cwd = std::env::var("HELLO_OUSTER_WORLD_CWD").unwrap_or_else(|_| DEFAULT_CWD.to_string());
    if Path::new(&cwd).is_dir() {
        cwd
    } else {
        "/".to_string()
    }
}

async fn pty_session(mut socket: WebSocket, state: AppState) {
    if !state.try_acquire_pty() {
        let _ = send_msg(
            &mut socket,
            ServerMsg::Error {
                message: format!(
                    "too many console sessions (max {})",
                    crate::MAX_PTY_SESSIONS
                ),
            },
        )
        .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    // Ensure the session counter drops even if the PTY loop panics or hangs on kill.
    struct PtyGuard(AppState);
    impl Drop for PtyGuard {
        fn drop(&mut self) {
            self.0.release_pty();
        }
    }
    let state_for_pty = state.clone();
    let _guard = PtyGuard(state);

    let result = run_console(&mut socket, &state_for_pty).await;
    if let Err(err) = result {
        tracing::warn!(?err, "console session ended with error");
        let _ = send_msg(
            &mut socket,
            ServerMsg::Error {
                message: err.to_string(),
            },
        )
        .await;
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn run_console(socket: &mut WebSocket, state: &AppState) -> anyhow::Result<()> {
    let log_path = console_log_path();
    ensure_console_log(&log_path);
    let log_pathbuf = PathBuf::from(&log_path);
    let log_name = log_pathbuf
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();

    if let Some(tail) = log_tail(&log_path, 200).await {
        if send_msg(socket, ServerMsg::Output { data: tail }).await.is_err() {
            return Ok(());
        }
    }

    let mut file = File::open(&log_path).await?;
    let mut pos = file.metadata().await?.len();
    file.seek(SeekFrom::Start(pos)).await?;

    let mut inotify = Inotify::init()?;
    watch_console_log(&mut inotify, &log_pathbuf)?;
    let mut afd = AsyncFd::new(inotify)?;
    let mut buf = vec![0u8; 8192];
    let mut evbuf = [0u8; 4096];

    loop {
        tokio::select! {
            biased;
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMsg>(&text) {
                            Ok(ClientMsg::Input { data }) => {
                                if data.as_bytes().contains(&0x03) {
                                    note_ctrl_c(&log_path, state);
                                }
                            }
                            Ok(ClientMsg::Resize { .. }) => {}
                            Err(e) => {
                                let _ = send_msg(
                                    socket,
                                    ServerMsg::Error {
                                        message: format!("bad client message: {e}"),
                                    },
                                ).await;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bin))) => {
                        if bin.contains(&0x03) {
                            note_ctrl_c(&log_path, state);
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            ready = afd.readable() => {
                let mut guard = ready?;
                guard.clear_ready();
                drop(guard);
                let events = match afd.get_mut().read_events(&mut evbuf) {
                    Ok(it) => it.collect::<Vec<_>>(),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(e) => return Err(e.into()),
                };
                let mut recreate = false;
                for ev in &events {
                    if ev.mask.intersects(
                        EventMask::DELETE_SELF | EventMask::MOVE_SELF | EventMask::IGNORED,
                    ) || (ev.mask.intersects(EventMask::CREATE | EventMask::MOVED_TO)
                        && ev.name.as_deref() == Some(log_name.as_os_str()))
                    {
                        recreate = true;
                    }
                }
                if recreate {
                    ensure_console_log(&log_path);
                    let _ = watch_console_log(afd.get_mut(), &log_pathbuf);
                    file = File::open(&log_path).await?;
                    pos = 0;
                    file.seek(SeekFrom::Start(0)).await?;
                }
                if drain_log(socket, &mut file, &mut pos, &mut buf).await? {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

fn watch_console_log(inotify: &mut Inotify, log_path: &Path) -> std::io::Result<()> {
    inotify.watches().add(
        log_path,
        WatchMask::MODIFY | WatchMask::ATTRIB | WatchMask::DELETE_SELF | WatchMask::MOVE_SELF,
    )?;
    if let Some(parent) = log_path.parent() {
        inotify
            .watches()
            .add(parent, WatchMask::CREATE | WatchMask::MOVED_TO)?;
    }
    Ok(())
}

/// Returns true if the WebSocket is gone.
async fn drain_log(
    socket: &mut WebSocket,
    file: &mut File,
    pos: &mut u64,
    buf: &mut [u8],
) -> anyhow::Result<bool> {
    let meta = file.metadata().await?;
    if meta.len() < *pos {
        *pos = 0;
        file.seek(SeekFrom::Start(0)).await?;
    }
    loop {
        let n = file.read(buf).await?;
        if n == 0 {
            return Ok(false);
        }
        *pos += n as u64;
        let data = String::from_utf8_lossy(&buf[..n]).into_owned();
        if send_msg(socket, ServerMsg::Output { data }).await.is_err() {
            return Ok(true);
        }
    }
}

fn note_ctrl_c(log_path: &str, _state: &AppState) {
    let state_dir = display_config::state_dir();
    let cfg = source_config::load(&state_dir);
    if source_config::save(&state_dir, &cfg).is_ok() {
        let _ = append_console_note(
            log_path,
            "hello-ouster-world: ^C — re-saved source-config; SimpleViz will reload",
        );
    } else {
        let _ = append_console_note(
            log_path,
            "hello-ouster-world: ^C — failed to re-save source-config",
        );
    }
}

async fn log_tail(path: &str, max_lines: usize) -> Option<String> {
    let mut file = File::open(path).await.ok()?;
    let len = file.metadata().await.ok()?.len();
    const WINDOW: u64 = 256 * 1024;
    let start = len.saturating_sub(WINDOW);
    file.seek(SeekFrom::Start(start)).await.ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await.ok()?;
    if buf.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let skip = lines.len().saturating_sub(max_lines);
    let mut tail = lines[skip..].join("\n");
    if !tail.is_empty() {
        tail.push('\n');
    }
    Some(tail)
}

fn append_console_note(path: &str, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

fn console_log_path() -> String {
    std::env::var("HELLO_OUSTER_WORLD_CONSOLE_LOG").unwrap_or_else(|_| {
        "/run/hello-ouster-world/console.log".to_string()
    })
}

fn ensure_console_log(path: &str) {
    if let Some(parent) = Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
}

async fn send_msg(
    socket: &mut WebSocket,
    msg: ServerMsg,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = serde_json::to_string(&msg)?;
    socket.send(Message::Text(text.into())).await?;
    Ok(())
}

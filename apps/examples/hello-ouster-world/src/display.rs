//! Display stream: PointViz JPEG frames → WS, HDMI toggle, resolution, remote input.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, watch};

use crate::display_config;
use crate::ui_state;
use crate::vpointer::{VPtrButton, VPtrCmd, VirtualPointer};
use crate::AppState;

pub const MAX_DISPLAY_VIEWERS: usize = 2;
// Match OSF content rate. Capture is gated on PointViz.update() so screenshots
// ride the real drawn frame (no forced extra scene draw on Tegra).
const DEFAULT_FPS: u64 = 5;
const ALLOWED_FPS: &[u64] = &[2, 5, 10];
const VIZFRAME_SOCK_NAME: &str = "vizframe.sock";
const STREAM_TARGET_NAME: &str = "stream_target.json";
const VIZ_WINDOW_TITLE: &str = "Ouster Viz";
/// Must match `GLFW_WAYLAND_APP_ID` in default.nix.
const VIZ_APP_ID: &str = "hello-ouster-world";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    #[serde(rename = "480")]
    P480,
    #[serde(rename = "720")]
    P720,
    #[serde(rename = "1080")]
    P1080,
    #[serde(rename = "max", alias = "native")]
    Max,
}

impl Resolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P480 => "480",
            Self::P720 => "720",
            Self::P1080 => "1080",
            Self::Max => "max",
        }
    }

    pub fn max_width(self) -> Option<u32> {
        match self {
            Self::P480 => Some(854),
            Self::P720 => Some(1280),
            Self::P1080 => Some(1920),
            Self::Max => None,
        }
    }
}

impl Default for Resolution {
    fn default() -> Self {
        Self::Max
    }
}

#[derive(Debug)]
enum InjectCmd {
    Move { x: i32, y: i32 },
    Button {
        press: bool,
        button: VPtrButton,
        x: i32,
        y: i32,
    },
    Axis { delta_y: f64 },
    Key {
        code: String,
        key: String,
        down: bool,
        mods: u32,
    },
}

#[derive(Clone)]
pub struct DisplayState {
    inner: Arc<DisplayInner>,
}

struct DisplayInner {
    viewers: AtomicUsize,
    next_viewer: AtomicU64,
    resolution: Mutex<Resolution>,
    max_framerate: AtomicU64,
    fps_watch: watch::Sender<u64>,
    hdmi_enabled: Mutex<bool>,
    stream_enabled: Mutex<bool>,
    on_device_active: Mutex<bool>,
    need_render: Mutex<bool>,
    /// Last sway output name that hosts PointViz (for vpointer binding).
    viz_output: Mutex<Option<String>>,
    /// Physical HDMI/DP connector currently present (cable plugged).
    hdmi_present: Mutex<bool>,
    source_size: Mutex<(u32, u32)>,
    scaled_size: Mutex<(u32, u32)>,
    controlling: Mutex<Option<u64>>,
    frame_tx: broadcast::Sender<Arc<Vec<u8>>>,
    capture_gate: watch::Sender<bool>,
    inject_tx: mpsc::UnboundedSender<InjectCmd>,
    vpointer: Mutex<VirtualPointer>,
    last_error: Mutex<Option<String>>,
    last_capture_ms: AtomicU64,
}

impl DisplayState {
    pub fn new(hdmi: bool, stream: bool) -> Self {
        let (frame_tx, _) = broadcast::channel(2);
        let (capture_gate, _) = watch::channel(false);
        let (fps_watch, _) = watch::channel(DEFAULT_FPS);
        let (inject_tx, inject_rx) = mpsc::unbounded_channel();
        let vpointer = VirtualPointer::spawn();
        let state = Self {
            inner: Arc::new(DisplayInner {
                viewers: AtomicUsize::new(0),
                next_viewer: AtomicU64::new(1),
                resolution: Mutex::new(Resolution::default()),
                max_framerate: AtomicU64::new(DEFAULT_FPS),
                fps_watch,
                hdmi_enabled: Mutex::new(hdmi),
                stream_enabled: Mutex::new(stream),
                // Optimistic until output manager places the window.
                on_device_active: Mutex::new(hdmi),
                need_render: Mutex::new(hdmi || stream),
                viz_output: Mutex::new(None),
                hdmi_present: Mutex::new(true),
                source_size: Mutex::new((1920, 1080)),
                scaled_size: Mutex::new((1920, 1080)),
                controlling: Mutex::new(None),
                frame_tx,
                capture_gate,
                inject_tx,
                vpointer: Mutex::new(vpointer),
                last_error: Mutex::new(None),
                last_capture_ms: AtomicU64::new(0),
            }),
        };
        let _ = write_stream_target(&state);
        spawn_inject_loop(state.clone(), inject_rx);
        spawn_capture_loop(state.clone());
        spawn_output_manager(state.clone());
        state
    }

    pub fn status_json(&self) -> serde_json::Value {
        let (sw, sh) = *self.inner.source_size.lock();
        let (dw, dh) = *self.inner.scaled_size.lock();
        let controlling = self.inner.controlling.lock().is_some();
        let fps = self.inner.max_framerate.load(Ordering::Relaxed);
        let viewers = self.inner.viewers.load(Ordering::Relaxed);
        let stream_enabled = *self.inner.stream_enabled.lock();
        let streaming = stream_enabled && viewers > 0;
        json!({
            "hdmi": *self.inner.hdmi_enabled.lock(),
            "hdmi_present": *self.inner.hdmi_present.lock(),
            "stream": stream_enabled,
            "streaming": streaming,
            "on_device": *self.inner.on_device_active.lock(),
            "need_render": *self.inner.need_render.lock(),
            "viz_output": self.inner.viz_output.lock().clone(),
            "resolution": self.inner.resolution.lock().as_str(),
            "width": sw,
            "height": sh,
            "stream_width": dw,
            "stream_height": dh,
            "fps": fps,
            "max_framerate": fps,
            "viewers": viewers,
            "viewers_max": MAX_DISPLAY_VIEWERS,
            "controlling": controlling,
            "error": self.inner.last_error.lock().clone(),
            "last_capture_ms": self.inner.last_capture_ms.load(Ordering::Relaxed),
        })
    }

    fn set_error(&self, msg: Option<String>) {
        *self.inner.last_error.lock() = msg;
    }

    fn max_framerate(&self) -> u64 {
        self.inner.max_framerate.load(Ordering::Relaxed)
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/display/status", get(display_status))
        .route("/display/stream", get(display_stream))
        .route("/display/stream-enabled", post(set_stream_enabled))
        .route("/display/hdmi", post(set_hdmi))
        .route("/display/resolution", post(set_resolution))
        .route("/display/fps", post(set_fps))
}

async fn display_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(state.display.status_json())
}

#[derive(Debug, Deserialize)]
struct HdmiBody {
    enabled: bool,
}

async fn set_hdmi(
    State(state): State<AppState>,
    Json(body): Json<HdmiBody>,
) -> Json<serde_json::Value> {
    match set_hdmi_enabled(&state.display, body.enabled).await {
        Ok(()) => {
            state.display.set_error(None);
        }
        Err(e) => {
            state.display.set_error(Some(e.to_string()));
            tracing::warn!(?e, "hdmi toggle failed");
        }
    }
    persist_display_toggles(&state.display);
    ui_state::publish_ui(&state);
    Json(state.display.status_json())
}

#[derive(Debug, Deserialize)]
struct StreamEnabledBody {
    enabled: bool,
}

async fn set_stream_enabled(
    State(state): State<AppState>,
    Json(body): Json<StreamEnabledBody>,
) -> Json<serde_json::Value> {
    *state.display.inner.stream_enabled.lock() = body.enabled;
    persist_display_toggles(&state.display);
    let _ = write_stream_target(&state.display);
    // Browser capture is optional. HDMI-on must keep SimpleViz on the panel
    // even when JPEG grab stops — otherwise the window stays on HEADLESS.
    if *state.display.inner.hdmi_enabled.lock() {
        if let Err(e) = place_viz_window(&state.display).await {
            tracing::warn!(?e, "viz placement after stream toggle failed");
        }
    }
    state.display.set_error(None);
    ui_state::publish_ui(&state);
    Json(state.display.status_json())
}

#[derive(Debug, Deserialize)]
struct ResolutionBody {
    resolution: Resolution,
}

async fn set_resolution(
    State(state): State<AppState>,
    Json(body): Json<ResolutionBody>,
) -> Json<serde_json::Value> {
    *state.display.inner.resolution.lock() = body.resolution;
    let _ = write_stream_target(&state.display);
    ui_state::publish_ui(&state);
    Json(state.display.status_json())
}

#[derive(Debug, Deserialize)]
struct FpsBody {
    max_framerate: u64,
}

async fn set_fps(
    State(state): State<AppState>,
    Json(body): Json<FpsBody>,
) -> Json<serde_json::Value> {
    if !ALLOWED_FPS.contains(&body.max_framerate) {
        state.display.set_error(Some(format!(
            "max_framerate must be one of {ALLOWED_FPS:?}"
        )));
        ui_state::publish_ui(&state);
        return Json(state.display.status_json());
    }
    state
        .display
        .inner
        .max_framerate
        .store(body.max_framerate, Ordering::Relaxed);
    let _ = state.display.inner.fps_watch.send(body.max_framerate);
    let _ = write_stream_target(&state.display);
    state.display.set_error(None);
    ui_state::publish_ui(&state);
    Json(state.display.status_json())
}

async fn display_stream(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_session(socket, state))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientEvent {
    Control { action: String },
    PointerMove { x: f64, y: f64 },
    PointerButton {
        button: String,
        state: String,
        x: f64,
        y: f64,
    },
    PointerAxis { delta_x: f64, delta_y: f64 },
    Key {
        key: String,
        code: String,
        state: String,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        meta: bool,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEvent {
    Meta {
        resolution: String,
        width: u32,
        height: u32,
        stream_width: u32,
        stream_height: u32,
        hdmi: bool,
        hdmi_present: bool,
        stream: bool,
        streaming: bool,
        need_render: bool,
        controlling: bool,
        fps: u64,
        max_framerate: u64,
    },
    Control { ok: bool, message: String },
    Error { message: String },
}

async fn stream_session(mut socket: WebSocket, state: AppState) {
    let viewers = state.display.inner.viewers.fetch_add(1, Ordering::SeqCst) + 1;
    if viewers > MAX_DISPLAY_VIEWERS {
        state.display.inner.viewers.fetch_sub(1, Ordering::SeqCst);
        let _ = send_event(
            &mut socket,
            ServerEvent::Error {
                message: format!("too many display viewers (max {MAX_DISPLAY_VIEWERS})"),
            },
        )
        .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    let viewer_id = state.display.inner.next_viewer.fetch_add(1, Ordering::SeqCst);
    let _ = state.display.inner.capture_gate.send(true);
    let _ = write_stream_target(&state.display);

    struct ViewerGuard {
        state: AppState,
        viewer_id: u64,
    }
    impl Drop for ViewerGuard {
        fn drop(&mut self) {
            let n = self.state.display.inner.viewers.fetch_sub(1, Ordering::SeqCst);
            if n <= 1 {
                let _ = self.state.display.inner.capture_gate.send(false);
            }
            let _ = write_stream_target(&self.state.display);
            let mut c = self.state.display.inner.controlling.lock();
            if *c == Some(self.viewer_id) {
                *c = None;
            }
        }
    }
    let _guard = ViewerGuard {
        state: state.clone(),
        viewer_id,
    };

    let mut frames = state.display.inner.frame_tx.subscribe();
    let _ = send_meta(&mut socket, &state.display).await;

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientEvent>(&text) {
                            Ok(ev) => {
                                let is_control = matches!(ev, ClientEvent::Control { .. });
                                match handle_client_event(&state, viewer_id, ev) {
                                    Ok(()) if is_control => {
                                        let controlling =
                                            *state.display.inner.controlling.lock() == Some(viewer_id);
                                        let _ = send_event(
                                            &mut socket,
                                            ServerEvent::Control {
                                                ok: true,
                                                message: if controlling {
                                                    "controlling".into()
                                                } else {
                                                    "released".into()
                                                },
                                            },
                                        )
                                        .await;
                                        let _ = send_meta(&mut socket, &state.display).await;
                                    }
                                    Ok(()) => {}
                                    Err(e) => {
                                        let msg = e.to_string();
                                        let _ = send_event(
                                            &mut socket,
                                            ServerEvent::Error {
                                                message: msg.clone(),
                                            },
                                        )
                                        .await;
                                        if is_control {
                                            let _ = send_event(
                                                &mut socket,
                                                ServerEvent::Control {
                                                    ok: false,
                                                    message: msg,
                                                },
                                            )
                                            .await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = send_event(
                                    &mut socket,
                                    ServerEvent::Error { message: format!("bad client message: {e}") },
                                ).await;
                            }
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
            frame = frames.recv() => {
                // Keep only the newest frame to cut glass-to-glass latency.
                let mut latest = match frame {
                    Ok(j) => j,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                while let Ok(j) = frames.try_recv() {
                    latest = j;
                }
                if socket
                    .send(Message::Binary(latest.as_slice().to_vec().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

async fn send_meta(
    socket: &mut WebSocket,
    display: &DisplayState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let meta = {
        let (sw, sh) = *display.inner.source_size.lock();
        let (dw, dh) = *display.inner.scaled_size.lock();
        let fps = display.max_framerate();
        let stream_enabled = *display.inner.stream_enabled.lock();
        let viewers = display.inner.viewers.load(Ordering::Relaxed);
        ServerEvent::Meta {
            resolution: display.inner.resolution.lock().as_str().to_string(),
            width: sw,
            height: sh,
            stream_width: dw,
            stream_height: dh,
            hdmi: *display.inner.hdmi_enabled.lock(),
            hdmi_present: *display.inner.hdmi_present.lock(),
            stream: stream_enabled,
            streaming: stream_enabled && viewers > 0,
            need_render: *display.inner.need_render.lock(),
            controlling: display.inner.controlling.lock().is_some(),
            fps,
            max_framerate: fps,
        }
    };
    send_event(socket, meta).await
}

async fn send_event(
    socket: &mut WebSocket,
    ev: ServerEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = serde_json::to_string(&ev)?;
    socket.send(Message::Text(text.into())).await?;
    Ok(())
}

fn handle_client_event(state: &AppState, viewer_id: u64, ev: ClientEvent) -> anyhow::Result<()> {
    match ev {
        ClientEvent::Control { action } => {
            let mut c = state.display.inner.controlling.lock();
            match action.as_str() {
                "acquire" => {
                    if let Some(other) = *c {
                        if other != viewer_id {
                            anyhow::bail!("another session is controlling");
                        }
                    }
                    *c = Some(viewer_id);
                    drop(c);
                    // Best-effort: focus the viz window so pointer events land on SimpleViz.
                    tokio::spawn(async {
                        let _ = focus_viz_window().await;
                    });
                }
                "release" => {
                    if *c == Some(viewer_id) {
                        *c = None;
                    }
                }
                _ => anyhow::bail!("unknown control action"),
            }
            Ok(())
        }
        ClientEvent::PointerMove { x, y } => {
            ensure_controlling(state, viewer_id)?;
            enqueue_inject(
                state,
                InjectCmd::Move {
                    x: x.round() as i32,
                    y: y.round() as i32,
                },
            )
        }
        ClientEvent::PointerButton {
            button,
            state: st,
            x,
            y,
        } => {
            ensure_controlling(state, viewer_id)?;
            let btn = map_button(&button)?;
            let press = match st.as_str() {
                "down" => true,
                "up" => false,
                _ => return Ok(()),
            };
            enqueue_inject(
                state,
                InjectCmd::Button {
                    press,
                    button: btn,
                    x: x.round() as i32,
                    y: y.round() as i32,
                },
            )
        }
        ClientEvent::PointerAxis { delta_y, delta_x: _delta_x } => {
            ensure_controlling(state, viewer_id)?;
            enqueue_inject(state, InjectCmd::Axis { delta_y })
        }
        ClientEvent::Key {
            key,
            code,
            state: st,
            shift,
            ctrl,
            alt,
            meta,
        } => {
            ensure_controlling(state, viewer_id)?;
            let mut mods = 0u32;
            if shift {
                mods |= 0x0001; // GLFW_MOD_SHIFT
            }
            if ctrl {
                mods |= 0x0002; // GLFW_MOD_CONTROL
            }
            if alt {
                mods |= 0x0004; // GLFW_MOD_ALT
            }
            if meta {
                mods |= 0x0008; // GLFW_MOD_SUPER
            }
            enqueue_inject(
                state,
                InjectCmd::Key {
                    code,
                    key,
                    down: st == "down",
                    mods,
                },
            )
        }
    }
}

fn enqueue_inject(state: &AppState, cmd: InjectCmd) -> anyhow::Result<()> {
    state
        .display
        .inner
        .inject_tx
        .send(cmd)
        .map_err(|_| anyhow::anyhow!("input injector stopped"))
}

fn ensure_controlling(state: &AppState, viewer_id: u64) -> anyhow::Result<()> {
    if *state.display.inner.controlling.lock() == Some(viewer_id) {
        Ok(())
    } else {
        anyhow::bail!("control toggle is off")
    }
}

fn map_button(button: &str) -> anyhow::Result<VPtrButton> {
    Ok(match button {
        "left" | "0" => VPtrButton::Left,
        "middle" | "1" => VPtrButton::Middle,
        "right" | "2" => VPtrButton::Right,
        other => anyhow::bail!("unknown button {other}"),
    })
}

fn wayland_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    let xdg = std::env::var("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/run/hello-ouster-world".into());
    let wd = std::env::var("HELLO_OUSTER_WORLD_WAYLAND_DISPLAY")
        .unwrap_or_else(|_| discover_wayland_display(&xdg));
    let path = std::env::var("HELLO_OUSTER_WORLD_PATH").unwrap_or_else(|_| {
        std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".into())
    });
    env.insert("XDG_RUNTIME_DIR".into(), xdg.clone());
    env.insert("WAYLAND_DISPLAY".into(), wd);
    env.insert("PATH".into(), path);
    if let Some(sock) = discover_sway_sock(&xdg) {
        env.insert("SWAYSOCK".into(), sock);
    }
    env
}

fn discover_wayland_display(xdg: &str) -> String {
    for name in ["wayland-1", "wayland-0"] {
        let p = std::path::Path::new(xdg).join(name);
        if p.exists() {
            return name.to_string();
        }
    }
    "wayland-1".into()
}

fn discover_sway_sock(xdg: &str) -> Option<String> {
    if let Ok(sock) = std::env::var("HELLO_OUSTER_WORLD_SWAYSOCK") {
        if !sock.is_empty() && std::path::Path::new(&sock).exists() {
            return Some(sock);
        }
    }
    let dir = std::path::Path::new(xdg);
    let mut socks: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
        })
        .collect();
    socks.sort();
    socks.pop().map(|p| p.to_string_lossy().into_owned())
}

fn tool(name: &str, env_key: &str) -> String {
    std::env::var(env_key).unwrap_or_else(|_| name.to_string())
}

async fn run_swaymsg(args: &[&str]) -> anyhow::Result<String> {
    let env = wayland_env();
    let bin = tool("swaymsg", "HELLO_OUSTER_WORLD_SWAYMSG");
    let mut cmd = Command::new(&bin);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in &env {
        cmd.env(k, v);
    }
    let out = cmd.output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "swaymsg {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn viz_selectors() -> [String; 2] {
    [
        format!("[title=\"{VIZ_WINDOW_TITLE}\"]"),
        format!("[app_id=\"{VIZ_APP_ID}\"]"),
    ]
}

async fn sway_viz(args_after_sel: &[&str]) -> anyhow::Result<()> {
    let mut last_err = None;
    for sel in viz_selectors() {
        let mut args: Vec<&str> = Vec::with_capacity(1 + args_after_sel.len());
        args.push(&sel);
        args.extend_from_slice(args_after_sel);
        match run_swaymsg(&args).await {
            Ok(_) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no viz window selector matched")))
}

async fn focus_viz_window() -> anyhow::Result<()> {
    let _ = sway_viz(&["focus"]).await;
    Ok(())
}

/// Walk `swaymsg -t get_tree` and return the output name hosting the viz window.
/// GLFW Wayland windows often show up as `app_id`, not the GLFW title.
fn is_viz_con(node: &serde_json::Value) -> bool {
    if node.get("type").and_then(|t| t.as_str()) != Some("con") {
        return false;
    }
    let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let app_id = node.get("app_id").and_then(|n| n.as_str()).unwrap_or("");
    name == VIZ_WINDOW_TITLE || app_id == VIZ_APP_ID
}

fn find_viz_output_in_tree(node: &serde_json::Value, current_output: Option<&str>) -> Option<String> {
    let output = if node.get("type").and_then(|t| t.as_str()) == Some("output") {
        node.get("name").and_then(|n| n.as_str())
    } else {
        current_output
    };

    if is_viz_con(node) {
        return output.map(|s| s.to_string());
    }

    if let Some(nodes) = node.get("nodes").and_then(|n| n.as_array()) {
        for child in nodes {
            if let Some(found) = find_viz_output_in_tree(child, output) {
                return Some(found);
            }
        }
    }
    if let Some(nodes) = node.get("floating_nodes").and_then(|n| n.as_array()) {
        for child in nodes {
            if let Some(found) = find_viz_output_in_tree(child, output) {
                return Some(found);
            }
        }
    }
    None
}

async fn viz_window_output() -> anyhow::Result<Option<String>> {
    let tree = run_swaymsg(&["-t", "get_tree"]).await?;
    let v: serde_json::Value = serde_json::from_str(&tree)?;
    Ok(find_viz_output_in_tree(&v, None))
}

fn output_name(o: &serde_json::Value) -> &str {
    o.get("name").and_then(|n| n.as_str()).unwrap_or("")
}

fn output_active(o: &serde_json::Value) -> bool {
    o.get("active").and_then(|a| a.as_bool()) == Some(true)
}

fn find_headless<'a>(arr: &'a [serde_json::Value]) -> Option<&'a serde_json::Value> {
    arr.iter()
        .find(|o| is_headless_name(output_name(o)) && output_active(o))
        .or_else(|| arr.iter().find(|o| is_headless_name(output_name(o))))
}

/// Any non-HEADLESS connector (enabled or blanked). Used for cable presence.
fn find_physical_any<'a>(arr: &'a [serde_json::Value]) -> Option<&'a serde_json::Value> {
    arr.iter().find(|o| !is_headless_name(output_name(o)))
}

/// Active (enabled) physical output currently driving a panel.
fn find_physical_active<'a>(arr: &'a [serde_json::Value]) -> Option<&'a serde_json::Value> {
    arr.iter()
        .find(|o| !is_headless_name(output_name(o)) && output_active(o))
}

fn find_physical<'a>(arr: &'a [serde_json::Value]) -> Option<&'a serde_json::Value> {
    find_physical_active(arr).or_else(|| find_physical_any(arr))
}

fn output_size(o: &serde_json::Value) -> Option<(u32, u32)> {
    if let Some((w, h, _)) = o.get("current_mode").and_then(parse_mode) {
        return Some((w, h));
    }
    if let Some(rect) = o.get("rect") {
        let w = rect.get("width").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let h = rect.get("height").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if w > 0 && h > 0 {
            return Some((w, h));
        }
    }
    None
}

async fn ensure_headless_output() -> anyhow::Result<String> {
    let arr = list_outputs().await?;
    if let Some(o) = find_headless(&arr) {
        let name = output_name(o).to_string();
        let _ = run_swaymsg(&["output", &name, "enable"]).await;
        let _ = run_swaymsg(&["output", &name, "resolution", "1920x1080"]).await;
        return Ok(name);
    }
    let _ = run_swaymsg(&["create_output"]).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let arr = list_outputs().await?;
    let o = find_headless(&arr).ok_or_else(|| anyhow::anyhow!("no HEADLESS output"))?;
    let name = output_name(o).to_string();
    let _ = run_swaymsg(&["output", &name, "enable"]).await;
    let _ = run_swaymsg(&["output", &name, "resolution", "1920x1080"]).await;
    Ok(name)
}

async fn move_viz_to_output(output: &str) -> anyhow::Result<()> {
    let _ = run_swaymsg(&["focus", "output", output]).await;
    // Drop fullscreen before the move — sway often leaves an empty fullscreen
    // workspace on the old output if you move a fullscreen container as-is.
    let _ = sway_viz(&["fullscreen", "disable"]).await;
    let _ = sway_viz(&["move", "workspace", "to", "output", output]).await;
    let _ = sway_viz(&["move", "container", "to", "output", output]).await;
    let _ = sway_viz(&["focus"]).await;
    // Fill the panel so HDMI is not a tiny floating window on a black desktop.
    let _ = sway_viz(&["fullscreen", "enable"]).await;
    Ok(())
}

/// Place PointViz on physical HDMI when on-device is desired and available;
/// otherwise keep it on the persistent HEADLESS home output.
async fn place_viz_window(display: &DisplayState) -> anyhow::Result<()> {
    let hdmi_want = *display.inner.hdmi_enabled.lock();
    let _ = ensure_headless_output().await;
    let mut arr = list_outputs().await?;
    // Cable/connector presence ≠ output currently enabled. Disabling DP for
    // "HDMI off" must not grey out the toggle as if the cable were unplugged.
    *display.inner.hdmi_present.lock() = find_physical_any(&arr).is_some();

    if hdmi_want {
        if find_physical_active(&arr).is_none() {
            if let Some(phys) = find_physical_any(&arr) {
                let name = output_name(phys).to_string();
                let _ = run_swaymsg(&["output", &name, "enable"]).await;
                tokio::time::sleep(Duration::from_millis(250)).await;
                arr = list_outputs().await?;
                *display.inner.hdmi_present.lock() = find_physical_any(&arr).is_some();
            }
        }
    }

    let headless = find_headless(&arr);
    let physical = find_physical_active(&arr);

    let (target_name, on_device, size) = if hdmi_want {
        if let Some(phys) = physical {
            let name = output_name(phys).to_string();
            let size = output_size(phys).unwrap_or((1920, 1080));
            (name, true, size)
        } else if let Some(h) = headless {
            let name = output_name(h).to_string();
            let size = output_size(h).unwrap_or((1920, 1080));
            (name, false, size)
        } else {
            anyhow::bail!("no sway outputs available");
        }
    } else if let Some(h) = headless {
        let name = output_name(h).to_string();
        let size = output_size(h).unwrap_or((1920, 1080));
        (name, false, size)
    } else {
        anyhow::bail!("no HEADLESS home output");
    };

    // PointViz may not exist yet at boot — do not claim on_device until the
    // window is actually present and moved (otherwise HDMI stays black forever).
    let before = viz_window_output().await.ok().flatten();
    if before.is_none() {
        anyhow::bail!("Ouster Viz window not ready yet");
    }

    move_viz_to_output(&target_name).await?;
    let _ = focus_viz_window().await;

    let after = viz_window_output().await.ok().flatten();
    if after.as_deref() != Some(target_name.as_str()) {
        anyhow::bail!(
            "Ouster Viz still on {:?} after move to {target_name}",
            after
        );
    }

    *display.inner.on_device_active.lock() = on_device;
    *display.inner.source_size.lock() = size;

    // Rebind virtual pointer only when the viz host output changes. Binding to
    // HEADLESS while the window is on DP-1 breaks browser click-drag.
    let prev = display.inner.viz_output.lock().clone();
    if prev.as_deref() != Some(target_name.as_str()) {
        *display.inner.viz_output.lock() = Some(target_name.clone());
        *display.inner.vpointer.lock() =
            VirtualPointer::spawn_for_output(Some(target_name.clone()));
        tracing::info!(%target_name, "rebound virtual pointer to viz output");
    }

    let _ = write_stream_target(display);
    tracing::info!(%target_name, on_device, ?size, "placed Ouster Viz");

    if !hdmi_want {
        let arr = list_outputs().await.unwrap_or_default();
        if let Some(phys) = find_physical_active(&arr) {
            let name = output_name(phys).to_string();
            let _ = run_swaymsg(&["output", &name, "disable"]).await;
        }
        let arr = list_outputs().await.unwrap_or_default();
        *display.inner.hdmi_present.lock() = find_physical_any(&arr).is_some();
    }
    Ok(())
}

async fn set_hdmi_enabled(display: &DisplayState, enabled: bool) -> anyhow::Result<()> {
    *display.inner.hdmi_enabled.lock() = enabled;
    if enabled {
        // Power the panel first, then move viz onto it.
        let arr = list_outputs().await?;
        if let Some(phys) = find_physical_any(&arr) {
            let name = output_name(phys).to_string();
            let _ = run_swaymsg(&["output", &name, "enable"]).await?;
            tokio::time::sleep(Duration::from_millis(250)).await;
        } else {
            tracing::info!("hdmi enable requested but no physical output present");
        }
        place_viz_window(display).await?;
    } else {
        // Move viz to HEADLESS before blanking the physical panel.
        place_viz_window(display).await?;
    }
    Ok(())
}

fn spawn_inject_loop(display: DisplayState, mut rx: mpsc::UnboundedReceiver<InjectCmd>) {
    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            // Coalesce queued moves so click-drag stays responsive.
            let mut cmd = first;
            if matches!(cmd, InjectCmd::Move { .. }) {
                while let Ok(next) = rx.try_recv() {
                    match next {
                        InjectCmd::Move { .. } => cmd = next,
                        other => {
                            apply_inject(&display, cmd).await;
                            cmd = other;
                            break;
                        }
                    }
                }
            }
            apply_inject(&display, cmd).await;
        }
    });
}

async fn apply_inject(display: &DisplayState, cmd: InjectCmd) {
    let (ew, eh) = {
        let (w, h) = *display.inner.source_size.lock();
        (w.max(1), h.max(1))
    };
    // Keep SimpleViz focused so vpointer events hit the GLFW window (critical
    // when HEADLESS + physical outputs both exist). Focus on click, not every
    // move — repeated focus mid-drag can steal relative motion.
    if matches!(cmd, InjectCmd::Button { press: true, .. }) {
        let _ = focus_viz_window().await;
    }
    match cmd {
        InjectCmd::Move { x, y } => {
            display.inner.vpointer.lock().send(VPtrCmd::Move {
                x,
                y,
                extent_w: ew,
                extent_h: eh,
            });
        }
        InjectCmd::Button {
            press,
            button,
            x,
            y,
        } => {
            display.inner.vpointer.lock().send(VPtrCmd::Button {
                pressed: press,
                button,
                x,
                y,
                extent_w: ew,
                extent_h: eh,
            });
        }
        InjectCmd::Axis { delta_y } => {
            display.inner.vpointer.lock().send(VPtrCmd::Axis { delta_y });
        }
        InjectCmd::Key {
            code,
            key,
            down,
            mods,
        } => {
            // PointViz only handles PRESS/REPEAT; skip releases.
            if down {
                send_viz_key(&code, &key, mods);
            }
        }
    }
}

const VIZKEYS_SOCK_NAME: &str = "vizkeys.sock";

fn vizkeys_sock_path() -> PathBuf {
    runtime_dir().join(VIZKEYS_SOCK_NAME)
}

fn send_viz_key(code: &str, key: &str, mods: u32) {
    let path = vizkeys_sock_path();
    let payload = json!({
        "code": code,
        "key": key,
        "mods": mods,
    });
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&path) else {
        tracing::debug!(path = %path.display(), "vizkeys sock not ready");
        return;
    };
    use std::io::Write;
    let line = format!("{payload}\n");
    if let Err(e) = stream.write_all(line.as_bytes()) {
        tracing::debug!(?e, "vizkeys write failed");
    }
}

fn runtime_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/run/hello-ouster-world".into()),
    )
}

fn vizframe_sock_path() -> PathBuf {
    runtime_dir().join(VIZFRAME_SOCK_NAME)
}

fn persist_display_toggles(display: &DisplayState) {
    let dir = display_config::state_dir();
    let mut cfg = display_config::load(&dir);
    cfg.hdmi = *display.inner.hdmi_enabled.lock();
    cfg.stream = *display.inner.stream_enabled.lock();
    if let Err(e) = display_config::save(&dir, &cfg) {
        tracing::warn!(%e, "failed to persist display toggles");
    }
}

fn stream_target_path() -> PathBuf {
    runtime_dir().join(STREAM_TARGET_NAME)
}

fn stream_dims(res: Resolution, src_w: u32, src_h: u32) -> (u32, u32) {
    // Until Sway reports an output, assume 1080p so native/1080p are not 1×1.
    let (src_w, src_h) = if src_w > 0 && src_h > 0 {
        (src_w, src_h)
    } else {
        (1920, 1080)
    };
    match res.max_width() {
        Some(max_w) => {
            let h = ((f64::from(src_h) * f64::from(max_w) / f64::from(src_w)).round() as u32).max(1);
            (max_w, h)
        }
        None => (src_w, src_h),
    }
}

fn write_stream_target(display: &DisplayState) -> std::io::Result<()> {
    let dir = runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let viewers = display.inner.viewers.load(Ordering::Relaxed);
    let stream_enabled = *display.inner.stream_enabled.lock();
    let hdmi_enabled = *display.inner.hdmi_enabled.lock();
    let on_device = *display.inner.on_device_active.lock();
    // Browser encode only when stream toggle is on and a viewer is connected.
    let active = stream_enabled && viewers > 0;
    // HDMI toggle (not lagging on_device) so browser-off still plays on the panel.
    let need_render = hdmi_enabled || active;
    *display.inner.need_render.lock() = need_render;
    let _ = display.inner.capture_gate.send(active);
    let res = *display.inner.resolution.lock();
    let (sw, sh) = *display.inner.source_size.lock();
    let (dw, dh) = stream_dims(res, sw, sh);
    *display.inner.scaled_size.lock() = (dw, dh);
    let fps = display.max_framerate();
    let body = json!({
        "active": active,
        "need_render": need_render,
        "hdmi_enabled": hdmi_enabled,
        "stream_enabled": stream_enabled,
        "on_device": on_device,
        "max_framerate": fps,
        "width": dw,
        "height": dh,
        "resolution": res.as_str(),
        "source_width": sw,
        "source_height": sh,
    });
    let path = stream_target_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&body)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn parse_mode(mode: &serde_json::Value) -> Option<(u32, u32, u64)> {
    let w = mode.get("width").and_then(|x| x.as_u64())? as u32;
    let h = mode.get("height").and_then(|x| x.as_u64())? as u32;
    let refresh = mode.get("refresh").and_then(|x| x.as_u64()).unwrap_or(0);
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h, refresh))
}

fn is_headless_name(name: &str) -> bool {
    name.starts_with("HEADLESS")
}

async fn list_outputs() -> anyhow::Result<Vec<serde_json::Value>> {
    let outputs = run_swaymsg(&["-t", "get_outputs"]).await?;
    let v: serde_json::Value = serde_json::from_str(&outputs)?;
    Ok(v.as_array()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no outputs"))?)
}

async fn fetch_output_size() -> anyhow::Result<(u32, u32)> {
    let arr = list_outputs().await?;
    // Prefer the output that hosts viz: physical when active, else HEADLESS.
    let o = find_physical(&arr)
        .filter(|o| output_active(o))
        .or_else(|| find_headless(&arr))
        .ok_or_else(|| anyhow::anyhow!("no viz output"))?;
    output_size(o).ok_or_else(|| anyhow::anyhow!("could not read output size"))
}

fn spawn_output_manager(display: DisplayState) {
    tokio::spawn(async move {
        // Retry until PointViz exists and is on the intended output. A one-shot
        // place before the GLFW window appears used to mark on_device=true while
        // the real window stayed on HEADLESS (blank HDMI).
        for attempt in 1..=30 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            match place_viz_window(&display).await {
                Ok(()) => {
                    tracing::info!(attempt, "initial viz placement ok");
                    break;
                }
                Err(e) => {
                    tracing::debug!(attempt, ?e, "initial viz placement waiting");
                    if attempt == 30 {
                        tracing::warn!(?e, "initial viz placement gave up");
                    }
                }
            }
        }
        let mut last_on_device = *display.inner.on_device_active.lock();
        let mut last_hdmi = *display.inner.hdmi_enabled.lock();
        let mut last_present = *display.inner.hdmi_present.lock();
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let hdmi_want = *display.inner.hdmi_enabled.lock();
            let arr = match list_outputs().await {
                Ok(a) => a,
                Err(_) => {
                    *display.inner.hdmi_present.lock() = false;
                    let _ = write_stream_target(&display);
                    last_hdmi = hdmi_want;
                    continue;
                }
            };
            // Connector still listed ⇒ cable/port present (even if we blanked it).
            let physical_present = find_physical_any(&arr).is_some();
            *display.inner.hdmi_present.lock() = physical_present;
            let physical_active = find_physical_active(&arr).is_some();
            let want_on_device = hdmi_want && physical_present;
            let cur_on_device = *display.inner.on_device_active.lock();
            let viz_out = viz_window_output().await.ok().flatten();
            let viz_misplaced = match (want_on_device, viz_out.as_deref()) {
                (true, Some(name)) => is_headless_name(name),
                (false, Some(name)) => !is_headless_name(name),
                (_, None) => true, // window gone / restarting — keep trying
            };
            // Re-place when intent changes, cable changes, HDMI wanted but panel
            // inactive, or the window is not on the intended output.
            if want_on_device != cur_on_device
                || hdmi_want != last_hdmi
                || physical_present != last_present
                || (want_on_device && !physical_active)
                || viz_misplaced
            {
                if let Err(e) = place_viz_window(&display).await {
                    tracing::debug!(?e, "viz placement refresh failed");
                }
            } else if let Ok((nw, nh)) = fetch_output_size().await {
                let mut sz = display.inner.source_size.lock();
                if *sz != (nw, nh) {
                    *sz = (nw, nh);
                    drop(sz);
                    let _ = write_stream_target(&display);
                }
            } else {
                let _ = write_stream_target(&display);
            }
            last_on_device = *display.inner.on_device_active.lock();
            last_hdmi = hdmi_want;
            last_present = physical_present;
        }
    });
}

fn bind_vizframe_listener() -> anyhow::Result<UnixListener> {
    let path = vizframe_sock_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    // Allow the viz process (same runtime dir) to connect.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666));
    }
    tracing::info!(path = %path.display(), "vizframe socket listening");
    Ok(listener)
}

fn vizframe_producer_gone(err: &anyhow::Error) -> bool {
    err.chain().any(|c| {
        c.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                ErrorKind::UnexpectedEof
                    | ErrorKind::ConnectionReset
                    | ErrorKind::BrokenPipe
                    | ErrorKind::ConnectionAborted
            )
        })
    })
}

async fn read_framed_jpeg(stream: &mut UnixStream) -> anyhow::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 16 * 1024 * 1024 {
        anyhow::bail!("invalid vizframe length {len}");
    }
    let mut jpeg = vec![0u8; len];
    stream.read_exact(&mut jpeg).await?;
    if jpeg.get(0..2) != Some(&[0xff, 0xd8]) {
        anyhow::bail!("vizframe payload is not JPEG");
    }
    Ok(jpeg)
}

fn spawn_capture_loop(display: DisplayState) {
    tokio::spawn(async move {
        let mut gate = display.inner.capture_gate.subscribe();
        let listener = match bind_vizframe_listener() {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(?e, "failed to bind vizframe socket");
                display.set_error(Some(format!("vizframe socket: {e}")));
                return;
            }
        };
        let mut conn: Option<UnixStream> = None;
        let mut frames: u64 = 0;
        let mut last_frame_at = Instant::now();

        loop {
            if !*gate.borrow() {
                conn = None;
                let _ = write_stream_target(&display);
                if gate.changed().await.is_err() {
                    break;
                }
                continue;
            }
            let _ = write_stream_target(&display);

            if frames % 60 == 0 {
                if let Ok((nw, nh)) = fetch_output_size().await {
                    *display.inner.source_size.lock() = (nw, nh);
                    let _ = write_stream_target(&display);
                }
            }

            // Accept a new viz connection when idle.
            if conn.is_none() {
                tokio::select! {
                    acc = listener.accept() => {
                        match acc {
                            Ok((s, _)) => {
                                tracing::info!("vizframe producer connected");
                                conn = Some(s);
                            }
                            Err(e) => {
                                tracing::warn!(?e, "vizframe accept failed");
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                continue;
                            }
                        }
                    }
                    _ = gate.changed() => continue,
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {
                        if last_frame_at.elapsed() > Duration::from_secs(3) {
                            display.set_error(Some(
                                "waiting for PointViz frames (is SimpleViz running?)".into(),
                            ));
                        }
                        continue;
                    }
                }
            }

            let Some(stream) = conn.as_mut() else {
                continue;
            };
            let started = Instant::now();
            let read = tokio::time::timeout(Duration::from_secs(2), read_framed_jpeg(stream)).await;
            match read {
                Ok(Ok(jpeg)) => {
                    let (dw, dh) = {
                        let res = *display.inner.resolution.lock();
                        let (sw, sh) = *display.inner.source_size.lock();
                        stream_dims(res, sw, sh)
                    };
                    *display.inner.scaled_size.lock() = (dw, dh);
                    display
                        .inner
                        .last_capture_ms
                        .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                    display.set_error(None);
                    let _ = display.inner.frame_tx.send(Arc::new(jpeg));
                    frames = frames.wrapping_add(1);
                    last_frame_at = Instant::now();
                }
                Ok(Err(e)) => {
                    tracing::debug!(?e, "vizframe read failed; reconnecting");
                    // Apply rewrites source-config; viz reload is expected on socket EOF.
                    if !vizframe_producer_gone(&e) {
                        display.set_error(Some(e.to_string()));
                    }
                    conn = None;
                }
                Err(_) => {
                    // Timeout: producer may be paused; keep connection.
                    if last_frame_at.elapsed() > Duration::from_secs(3) {
                        display.set_error(Some(
                            "no PointViz frames (capture idle or viz stuck)".into(),
                        ));
                    }
                }
            }
        }

        let path = vizframe_sock_path();
        let _ = std::fs::remove_file(path);
    });
}

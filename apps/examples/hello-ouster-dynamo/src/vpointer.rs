//! Wayland `zwlr_virtual_pointer_v1` injector for remote Display control.
//!
//! SimpleViz (GLFW) orbits from cursor-position deltas while the left button
//! is held. We warp with absolute motion, then drive drags with relative
//! motion so wlroots delivers events to the focused client after button grab.
//!
//! Absolute motion is scoped to a specific `wl_output` (the one hosting
//! PointViz). Binding the first registry output breaks when HEADLESS + HDMI
//! both exist — clicks land on the wrong screen.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

#[derive(Debug, Clone)]
pub enum VPtrCmd {
    Move {
        x: i32,
        y: i32,
        extent_w: u32,
        extent_h: u32,
    },
    Button {
        button: VPtrButton,
        pressed: bool,
        x: i32,
        y: i32,
        extent_w: u32,
        extent_h: u32,
    },
    Axis {
        delta_y: f64,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum VPtrButton {
    Left,
    Middle,
    Right,
}

impl VPtrButton {
    fn linux_code(self) -> u32 {
        match self {
            Self::Left => BTN_LEFT,
            Self::Middle => BTN_MIDDLE,
            Self::Right => BTN_RIGHT,
        }
    }

    fn mask(self) -> u8 {
        match self {
            Self::Left => 1,
            Self::Middle => 2,
            Self::Right => 4,
        }
    }
}

pub struct VirtualPointer {
    tx: Sender<VPtrCmd>,
}

impl VirtualPointer {
    pub fn spawn() -> Self {
        Self::spawn_for_output(None)
    }

    /// Bind absolute motion to `preferred_output` (e.g. `DP-1` or `HEADLESS-1`).
    pub fn spawn_for_output(preferred_output: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("hello-ouster-dynamo-vpointer".into())
            .spawn(move || {
                if let Err(e) = run_loop(rx, preferred_output) {
                    tracing::warn!(?e, "virtual pointer thread exited");
                }
            })
            .expect("spawn virtual pointer thread");
        Self { tx }
    }

    pub fn send(&self, cmd: VPtrCmd) {
        if self.tx.send(cmd).is_err() {
            tracing::debug!("virtual pointer thread not accepting commands");
        }
    }
}

struct OutputInfo {
    output: wl_output::WlOutput,
    name: Option<String>,
}

struct State {
    seat: Option<wl_seat::WlSeat>,
    outputs: Vec<OutputInfo>,
    manager: Option<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
    pointer: Option<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            if let Some(info) = state.outputs.iter_mut().find(|o| o.output == *output) {
                info.name = Some(name);
            }
        }
    }
}

impl Dispatch<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
        _: zwlr_virtual_pointer_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
        _: zwlr_virtual_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn apply_wayland_env() {
    let xdg = std::env::var("HELLO_OUSTER_DYNAMO_XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/run/hello-ouster-dynamo".into());
    let wd = std::env::var("HELLO_OUSTER_DYNAMO_WAYLAND_DISPLAY").unwrap_or_else(|_| {
        for name in ["wayland-1", "wayland-0"] {
            if std::path::Path::new(&xdg).join(name).exists() {
                return name.to_string();
            }
        }
        "wayland-1".into()
    });
    std::env::set_var("XDG_RUNTIME_DIR", &xdg);
    std::env::set_var("WAYLAND_DISPLAY", &wd);
}

fn now_ms() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

fn pick_output<'a>(
    outputs: &'a [OutputInfo],
    preferred: Option<&str>,
) -> Option<&'a wl_output::WlOutput> {
    if let Some(want) = preferred {
        if let Some(info) = outputs.iter().find(|o| o.name.as_deref() == Some(want)) {
            return Some(&info.output);
        }
    }
    // Prefer a physical output over HEADLESS when no preference matched.
    outputs
        .iter()
        .find(|o| {
            o.name
                .as_deref()
                .map(|n| !n.starts_with("HEADLESS"))
                .unwrap_or(false)
        })
        .or_else(|| outputs.first())
        .map(|o| &o.output)
}

fn setup(
    preferred_output: Option<&str>,
) -> anyhow::Result<(Connection, EventQueue<State>, State)> {
    apply_wayland_env();
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init::<State>(&conn)?;
    let qh = event_queue.handle();

    let mut state = State {
        seat: None,
        outputs: Vec::new(),
        manager: None,
        pointer: None,
    };

    if let Ok(seat) = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=9, ()) {
        state.seat = Some(seat);
    }
    let registry = globals.registry().clone();
    for global in globals.contents().clone_list() {
        if global.interface != "wl_output" {
            continue;
        }
        let version = global.version.min(4).max(1);
        let output = registry.bind::<wl_output::WlOutput, _, _>(global.name, version, &qh, ());
        state.outputs.push(OutputInfo {
            output,
            name: None,
        });
    }
    let manager = globals
        .bind::<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, _, _>(
            &qh,
            1..=2,
            (),
        )?;
    state.manager = Some(manager);
    // Two roundtrips so wl_output.name events arrive before we pick.
    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    let mgr = state
        .manager
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("virtual pointer manager missing"))?;
    let chosen = pick_output(&state.outputs, preferred_output);
    let chosen_name = chosen.and_then(|out| {
        state
            .outputs
            .iter()
            .find(|o| &o.output == out)
            .and_then(|o| o.name.clone())
    });
    let pointer = if let Some(output) = chosen {
        if mgr.version() >= 2 {
            mgr.create_virtual_pointer_with_output(None, Some(output), &qh, ())
        } else {
            mgr.create_virtual_pointer(None, &qh, ())
        }
    } else {
        mgr.create_virtual_pointer(None, &qh, ())
    };
    state.pointer = Some(pointer);
    event_queue.roundtrip(&mut state)?;
    tracing::info!(
        has_seat = state.seat.is_some(),
        outputs = state.outputs.len(),
        preferred = ?preferred_output,
        bound = ?chosen_name,
        "virtual pointer ready"
    );
    Ok((conn, event_queue, state))
}

fn motion_abs(
    ptr: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    x: i32,
    y: i32,
    extent_w: u32,
    extent_h: u32,
) {
    let ew = extent_w.max(1);
    let eh = extent_h.max(1);
    let ax = x.clamp(0, ew.saturating_sub(1) as i32) as u32;
    let ay = y.clamp(0, eh.saturating_sub(1) as i32) as u32;
    ptr.motion_absolute(now_ms(), ax, ay, ew, eh);
    ptr.frame();
}

fn motion_rel(ptr: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, dx: f64, dy: f64) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    ptr.motion(now_ms(), dx, dy);
    ptr.frame();
}

fn flush(conn: &Connection, event_queue: &mut EventQueue<State>, state: &mut State) {
    let _ = conn.flush();
    let _ = event_queue.dispatch_pending(state);
}

fn apply_cmd(
    conn: &Connection,
    event_queue: &mut EventQueue<State>,
    state: &mut State,
    cmd: VPtrCmd,
    last: &mut Option<(i32, i32)>,
    buttons: &mut u8,
) {
    let Some(ptr) = state.pointer.clone() else {
        return;
    };
    match cmd {
        VPtrCmd::Move {
            x,
            y,
            extent_w,
            extent_h,
        } => {
            if let Some((lx, ly)) = *last {
                let dx = f64::from(x - lx);
                let dy = f64::from(y - ly);
                if *buttons != 0 {
                    // Drag: relative motion is what GLFW/SimpleViz consume for orbit.
                    motion_rel(&ptr, dx, dy);
                } else {
                    motion_abs(&ptr, x, y, extent_w, extent_h);
                }
            } else {
                motion_abs(&ptr, x, y, extent_w, extent_h);
            }
            *last = Some((x, y));
            flush(conn, event_queue, state);
        }
        VPtrCmd::Button {
            button,
            pressed,
            x,
            y,
            extent_w,
            extent_h,
        } => {
            // Warp first so enter/focus land on SimpleViz before the click.
            motion_abs(&ptr, x, y, extent_w, extent_h);
            flush(conn, event_queue, state);
            let _ = event_queue.roundtrip(state);

            let st = if pressed {
                wl_pointer::ButtonState::Pressed
            } else {
                wl_pointer::ButtonState::Released
            };
            ptr.button(now_ms(), button.linux_code(), st);
            ptr.frame();
            if pressed {
                *buttons |= button.mask();
            } else {
                *buttons &= !button.mask();
            }
            *last = Some((x, y));
            flush(conn, event_queue, state);
            tracing::debug!(?button, pressed, x, y, "vpointer button");
        }
        VPtrCmd::Axis { delta_y } => {
            if delta_y.abs() < 0.1 {
                return;
            }
            let value = -delta_y;
            ptr.axis(now_ms(), wl_pointer::Axis::VerticalScroll, value);
            ptr.frame();
            flush(conn, event_queue, state);
        }
    }
}

fn run_loop(rx: Receiver<VPtrCmd>, preferred_output: Option<String>) -> anyhow::Result<()> {
    let mut setup_result = None;
    for attempt in 1..=60 {
        match setup(preferred_output.as_deref()) {
            Ok(v) => {
                setup_result = Some(v);
                break;
            }
            Err(e) => {
                tracing::debug!(?e, attempt, "virtual pointer setup retry");
                thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
    let (conn, mut event_queue, mut state) =
        setup_result.ok_or_else(|| anyhow::anyhow!("virtual pointer setup failed"))?;

    let mut last: Option<(i32, i32)> = None;
    let mut buttons: u8 = 0;

    while let Ok(first) = rx.recv() {
        let mut cmd = first;
        if matches!(cmd, VPtrCmd::Move { .. }) {
            while let Ok(next) = rx.try_recv() {
                match next {
                    VPtrCmd::Move { .. } => cmd = next,
                    other => {
                        apply_cmd(
                            &conn,
                            &mut event_queue,
                            &mut state,
                            cmd,
                            &mut last,
                            &mut buttons,
                        );
                        cmd = other;
                        break;
                    }
                }
            }
        }
        apply_cmd(
            &conn,
            &mut event_queue,
            &mut state,
            cmd,
            &mut last,
            &mut buttons,
        );
    }
    Ok(())
}

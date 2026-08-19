"""PointViz helpers: frame publish, key inject, VizProxy, JetstreamDisplayBridge.

Tegra note: PointViz.get_screenshot() from a helper thread often runs in a
process_frame where nothing was drawn yet. The same-size path then forces an
extra full point-cloud draw() — that alone pins HDMI viz near ~2 FPS.

Fix: wrap PointViz so update() marks the scene dirty, and only request a
screenshot after a real content update. That way process_frame draws once,
glReadPixels the same FB, then swaps — no second scene draw.
"""

from __future__ import annotations

import io
import json
import os
import queue
import socket
import struct
import threading
import time
import numpy as np
from PIL import Image
from pathlib import Path
from typing import Any, Callable, Optional

import ouster.sdk.viz.core as viz_core
from ouster.sdk.viz import PointViz, WindowCtx

from source_config import (
    AppSocketStatus,
    FrameSetSourceType,
    is_app_source,
    parse_source,
    set_runtime_status,
)

# Shared with Rust via HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR (systemd).
RUNTIME_DIR = Path(
    os.environ.get("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR", "/run/hello-ouster-world")
)
# str(): Python 3.10 AF_UNIX bind/connect reject pathlib.Path.
VIZFRAME_SOCK = str(RUNTIME_DIR / "vizframe.sock")
VIZKEYS_SOCK = str(RUNTIME_DIR / "vizkeys.sock")
JPEG_QUALITY = 40
CAPTURE_MAX_FPS = 10

# SimpleViz calls add_default_controls(self._viz); pybind requires a real PointViz.
_ORIG_ADD_DEFAULT_CONTROLS = viz_core.add_default_controls


def _add_default_controls(viz: Any, *args: Any, **kwargs: Any) -> Any:
    real = getattr(viz, "_real", None)
    if type(viz).__name__ == "VizProxy" and real is not None:
        viz = real
    return _ORIG_ADD_DEFAULT_CONTROLS(viz, *args, **kwargs)


viz_core.add_default_controls = _add_default_controls  # type: ignore[assignment]

# GLFW key codes (PointViz handlers use these; letters equal uppercase ASCII).
_GLFW_DOM = {
    "Escape": 256,
    "Enter": 257,
    "Tab": 258,
    "Backspace": 259,
    "Insert": 260,
    "Delete": 261,
    "ArrowRight": 262,
    "ArrowLeft": 263,
    "ArrowDown": 264,
    "ArrowUp": 265,
    "PageUp": 266,
    "PageDown": 267,
    "Home": 268,
    "End": 269,
    "CapsLock": 280,
    "Space": 32,
    "Minus": 45,
    "Equal": 61,
    "ShiftLeft": 340,
    "ShiftRight": 344,
    "ControlLeft": 341,
    "ControlRight": 345,
    "AltLeft": 342,
    "AltRight": 346,
    "MetaLeft": 343,
    "MetaRight": 347,
}
for _i, _ch in enumerate("0123456789"):
    _GLFW_DOM[f"Digit{_ch}"] = ord(_ch)
for _i, _ch in enumerate("ABCDEFGHIJKLMNOPQRSTUVWXYZ"):
    _GLFW_DOM[f"Key{_ch}"] = ord(_ch)
for _i in range(1, 13):
    _GLFW_DOM[f"F{_i}"] = 289 + _i


def dom_to_glfw_key(code: str, key: str) -> Optional[int]:
    if code in _GLFW_DOM:
        return _GLFW_DOM[code]
    if key and len(key) == 1:
        return ord(key.upper())
    named = {
        "Escape": 256,
        "Enter": 257,
        "Tab": 258,
        "Backspace": 259,
        "Delete": 261,
        "ArrowRight": 262,
        "ArrowLeft": 263,
        "ArrowDown": 264,
        "ArrowUp": 265,
        " ": 32,
    }
    return named.get(key)


class StreamTarget:
    """Reads whether Mission Control still wants frames from the viz.

    Rust writes ``stream_target.json`` with active/need_render, stream size,
    and max framerate; capture and pause logic poll that file.
    """

    _DEFAULT = {"active": False, "need_render": True}

    def __init__(self, path: Path | None = None) -> None:
        self.path = path if path is not None else RUNTIME_DIR / "stream_target.json"

    def read(self) -> dict:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
            if isinstance(data, dict):
                return data
        except (OSError, json.JSONDecodeError):
            pass
        return dict(self._DEFAULT)


class VizPid:
    """Publishes this process's pid so Rust can SIGINT SimpleViz cleanly.

    Used when the Display panel applies a new source or the console sends Ctrl-C.
    """

    def __init__(self, path: Path | None = None) -> None:
        self.path = path if path is not None else RUNTIME_DIR / "viz.pid"

    def write(self) -> None:
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self.path.write_text(str(os.getpid()), encoding="utf-8")
        except OSError as exc:
            print(f"hello-ouster-world: pid file write failed: {exc!r}", flush=True)

    def clear(self) -> None:
        try:
            if self.path.is_file() and self.path.read_text(encoding="utf-8").strip() == str(
                os.getpid()
            ):
                self.path.unlink()
        except OSError:
            pass

class DemandController:
    """Pauses SimpleViz when nobody is watching (HDMI idle and browser closed).

    Polls StreamTarget and toggles pause so playback does not burn GPU for
    an unused stream.
    """

    def __init__(self, stream_target: StreamTarget | None = None) -> None:
        self._stream_target = stream_target or StreamTarget()
        self._simple: Any = None
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self._idle = False
        self._lock = threading.Lock()

    def attach(self, simple: Any) -> None:
        with self._lock:
            self._simple = simple

    def start(self) -> None:
        self._stop.clear()
        self._thread = threading.Thread(
            name="viz-demand", target=self._loop, daemon=True
        )
        self._thread.start()

    def close(self) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=2.0)
            self._thread = None

    def _set_paused(self, simple: Any, paused: bool) -> None:
        """Match SimpleViz.toggle_pause without flipping incorrectly."""
        cv = getattr(simple, "_cv", None)
        if cv is None:
            if bool(getattr(simple, "_paused", False)) != paused:
                simple.toggle_pause()
            return
        with cv:
            if bool(getattr(simple, "_paused", False)) == paused:
                return
            simple._paused = paused
            try:
                simple._update_right_osd()
            except Exception:
                pass
            if not paused:
                cv.notify()

    def _loop(self) -> None:
        last_need: Optional[bool] = None
        while not self._stop.is_set():
            target = self._stream_target.read()
            # Default True so we keep playing until Rust publishes demand.
            need = bool(target.get("need_render", True))
            with self._lock:
                simple = self._simple
            if simple is not None and need != last_need:
                try:
                    if need:
                        self._set_paused(simple, False)
                        if self._idle:
                            print(
                                "hello-ouster-world: resume SimpleViz (need_render)",
                                flush=True,
                            )
                        self._idle = False
                    else:
                        self._set_paused(simple, True)
                        if not self._idle:
                            print(
                                "hello-ouster-world: pause SimpleViz (no display consumers)",
                                flush=True,
                            )
                        self._idle = True
                    last_need = need
                except Exception as exc:  # noqa: BLE001
                    print(f"hello-ouster-world: demand control error: {exc!r}", flush=True)
            self._stop.wait(0.25)


class KeyBridge:
    """Forwards keyboard input from the browser Display panel into PointViz.

    Rust writes key events to a Unix socket; this class queues them and
    applies them on the viz thread (including default camera controls).
    """

    def __init__(self) -> None:
        self._handlers: list = []
        self._q: queue.Queue = queue.Queue(maxsize=64)
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self._sock_path = VIZKEYS_SOCK
        self._orthographic = False
        self._fov_levels = (30, 45, 60, 75, 90, 105, 120, 135, 150)
        self._fov_index = 4

    def push_handler(self, handler: Callable) -> None:
        self._handlers.append(handler)

    def start(self) -> None:
        self._stop.clear()
        self._thread = threading.Thread(
            name="vizkeys-listen", target=self._listen_loop, daemon=True
        )
        self._thread.start()

    def close(self) -> None:
        self._stop.set()
        try:
            if os.path.exists(self._sock_path):
                s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                try:
                    s.connect(self._sock_path)
                except OSError:
                    pass
                finally:
                    s.close()
        except OSError:
            pass
        if self._thread is not None:
            self._thread.join(timeout=2.0)
            self._thread = None
        try:
            os.unlink(self._sock_path)
        except OSError:
            pass

    @staticmethod
    def _cameras(viz: Any) -> list:
        """Resolve PointViz camera(s). Python binds `camera` as a property."""
        cams: list = []
        for name in ("camera", "current_camera"):
            attr = getattr(viz, name, None)
            if attr is None:
                continue
            # C++ uses camera(); Python exposes property Camera (not callable).
            cam = attr() if callable(attr) else attr
            if cam is not None:
                cams.append(cam)
        return cams

    def _move_camera(self, viz: Any, key: int, amount: float) -> bool:
        cams = self._cameras(viz)
        if not cams:
            return False
        if key == ord("A"):
            for cam in cams:
                cam.yaw(amount)
            return True
        if key == ord("D"):
            for cam in cams:
                cam.yaw(-amount)
            return True
        if key == ord("W"):
            for cam in cams:
                cam.pitch(amount)
            return True
        if key == ord("S"):
            for cam in cams:
                cam.pitch(-amount)
            return True
        if key == ord("="):
            for cam in cams:
                cam.dolly(amount)
            return True
        if key == ord("-"):
            for cam in cams:
                cam.dolly(-amount)
            return True
        if key == ord("Q"):
            for cam in cams:
                cam.roll(amount)
            return True
        if key == ord("E"):
            for cam in cams:
                cam.roll(-amount)
            return True
        return False

    def _apply_default_controls(self, viz: Any, key: int, mods: int) -> None:
        """Mirror PointViz.add_default_controls for remote inject."""
        if mods == 0:
            if self._move_camera(viz, key, 5.0):
                return
            if key == ord("0"):
                self._orthographic = not self._orthographic
                for cam in self._cameras(viz):
                    cam.set_orthographic(self._orthographic)
                return
            if key == 256:  # Escape — ignore remotely (would stop the viz)
                return
        elif mods == 1:  # SHIFT
            if self._move_camera(viz, key, 0.3):
                return
            if key == ord("R"):
                for cam in self._cameras(viz):
                    cam.reset()
                return
        elif mods == 2:  # CONTROL
            if key in (ord("-"), ord("=")):
                if not self._orthographic:
                    if key == ord("="):
                        self._fov_index = (self._fov_index + len(self._fov_levels) - 1) % len(
                            self._fov_levels
                        )
                    else:
                        self._fov_index = (self._fov_index + 1) % len(self._fov_levels)
                    fov = float(self._fov_levels[self._fov_index])
                    for cam in self._cameras(viz):
                        cam.set_fov(fov)
                return
            if key == ord("R"):
                for cam in self._cameras(viz):
                    cam.birds_eye_view()

    def flush(self, viz: Any) -> None:
        """Apply queued keys on the PointViz thread."""
        while True:
            try:
                key, mods = self._q.get_nowait()
            except queue.Empty:
                break
            try:
                self._apply_default_controls(viz, int(key), int(mods))
            except Exception as exc:  # noqa: BLE001
                print(f"hello-ouster-world: camera key inject error: {exc!r}", flush=True)
            ctx = WindowCtx()
            for handler in list(self._handlers):
                try:
                    keep = handler(ctx, int(key), int(mods))
                except Exception as exc:  # noqa: BLE001
                    print(f"hello-ouster-world: key handler error: {exc!r}", flush=True)
                    break
                if keep is False:
                    break

    def _listen_loop(self) -> None:
        try:
            os.makedirs(os.path.dirname(self._sock_path) or ".", exist_ok=True)
        except OSError:
            pass
        try:
            os.unlink(self._sock_path)
        except OSError:
            pass
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            srv.bind(self._sock_path)
            srv.listen(4)
            srv.settimeout(1.0)
            print(f"hello-ouster-world: vizkeys listening on {self._sock_path}", flush=True)
            while not self._stop.is_set():
                try:
                    conn, _ = srv.accept()
                except socket.timeout:
                    continue
                except OSError:
                    if self._stop.is_set():
                        break
                    continue
                with conn:
                    buf = b""
                    conn.settimeout(1.0)
                    while not self._stop.is_set():
                        try:
                            chunk = conn.recv(4096)
                        except socket.timeout:
                            continue
                        except OSError:
                            break
                        if not chunk:
                            break
                        buf += chunk
                        while b"\n" in buf:
                            line, buf = buf.split(b"\n", 1)
                            self._handle_line(line)
        finally:
            try:
                srv.close()
            except OSError:
                pass
            try:
                os.unlink(self._sock_path)
            except OSError:
                pass

    def _handle_line(self, line: bytes) -> None:
        try:
            msg = json.loads(line.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            return
        if not isinstance(msg, dict):
            return
        code = str(msg.get("code") or "")
        key = str(msg.get("key") or "")
        mods = int(msg.get("mods") or 0)
        glfw_key = dom_to_glfw_key(code, key)
        if glfw_key is None:
            return
        try:
            self._q.put_nowait((glfw_key, mods))
        except queue.Full:
            try:
                _ = self._q.get_nowait()
            except queue.Empty:
                pass
            try:
                self._q.put_nowait((glfw_key, mods))
            except queue.Full:
                pass


class FramePublisher:
    """Captures HDMI PointViz frames and streams JPEGs to Mission Control.

    After each scene update, grabs a screenshot (when demand is active),
    encodes it, and sends it over the vizframe Unix socket.
    """

    def __init__(self, stream_target: StreamTarget | None = None) -> None:
        self._stream_target = stream_target or StreamTarget()
        self._sock: Optional[socket.socket] = None
        self._connect_attempt = 0.0
        self._np: Any = None
        self._Image: Any = None
        self._encode_q: queue.Queue = queue.Queue(maxsize=1)
        self._stop = threading.Event()
        self._worker: Optional[threading.Thread] = None
        self._dirty = threading.Event()
        self._grab_busy = False
        self._next_due = 0.0
        self._logged = False

    def note_scene_update(self) -> None:
        """Called from PointViz.update() when new scan content is staged."""
        self._dirty.set()

    def start(self) -> None:
        self._ensure_encode_deps()
        self._stop.clear()
        self._worker = threading.Thread(
            name="vizframe-encode", target=self._encode_loop, daemon=True
        )
        self._worker.start()

    def close(self) -> None:
        self._stop.set()
        self._dirty.set()
        try:
            self._encode_q.put_nowait(None)
        except queue.Full:
            try:
                _ = self._encode_q.get_nowait()
            except queue.Empty:
                pass
            try:
                self._encode_q.put_nowait(None)
            except queue.Full:
                pass
        if self._worker is not None:
            self._worker.join(timeout=2.0)
            self._worker = None
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None

    def _ensure_encode_deps(self) -> bool:
        if self._np is not None and self._Image is not None:
            return True
        try:
            self._np = np
            self._Image = Image
            warm = Image.fromarray(np.zeros((16, 16, 3), dtype=np.uint8), mode="RGB")
            warm.save(io.BytesIO(), format="JPEG", quality=JPEG_QUALITY, optimize=False)
            print("hello-ouster-world: encode deps ready", flush=True)
            return True
        except Exception as exc:  # noqa: BLE001
            print(f"hello-ouster-world: encode deps missing: {exc!r}", flush=True)
            return False

    def _ensure_connected(self) -> bool:
        if self._sock is not None:
            return True
        now = time.monotonic()
        if now - self._connect_attempt < 0.5:
            return False
        self._connect_attempt = now
        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.settimeout(0.5)
            s.connect(VIZFRAME_SOCK)
            s.settimeout(2.0)
            self._sock = s
            print("hello-ouster-world: connected to vizframe socket", flush=True)
            return True
        except OSError:
            self._sock = None
            return False

    def _encode_loop(self) -> None:
        while not self._stop.is_set():
            try:
                item = self._encode_q.get(timeout=0.2)
            except queue.Empty:
                continue
            if item is None:
                break
            rgb, stream_w, stream_h = item
            try:
                if not self._ensure_connected():
                    # Keep trying — do not drop the only queued frame.
                    try:
                        self._encode_q.put_nowait(item)
                    except queue.Full:
                        pass
                    time.sleep(0.1)
                    continue
                Image = self._Image
                img = Image.fromarray(rgb, mode="RGB")
                if img.size != (stream_w, stream_h):
                    img = img.resize((stream_w, stream_h), resample=Image.NEAREST)
                buf = io.BytesIO()
                img.save(
                    buf,
                    format="JPEG",
                    quality=JPEG_QUALITY,
                    optimize=False,
                    subsampling=2,
                )
                payload = buf.getvalue()
                assert self._sock is not None
                self._sock.sendall(struct.pack("<I", len(payload)) + payload)
            except Exception as exc:  # noqa: BLE001
                print(f"hello-ouster-world: encode/send failed: {exc!r}", flush=True)
                if self._sock is not None:
                    try:
                        self._sock.close()
                    except OSError:
                        pass
                    self._sock = None
                    try:
                        self._encode_q.put_nowait(item)
                    except queue.Full:
                        pass

    def try_grab(self, point_viz: Any) -> float:
        """Grab only if scene was updated. Returns seconds to sleep."""
        target = self._stream_target.read()
        if not target.get("need_render", True):
            self._dirty.clear()
            return 0.25
        if not target.get("active"):
            self._dirty.clear()
            return 0.15
        want_fps = max(1, int(target.get("max_framerate") or CAPTURE_MAX_FPS))
        fps = min(want_fps, CAPTURE_MAX_FPS)
        period = 1.0 / fps
        now = time.monotonic()
        if now < self._next_due:
            return max(0.01, self._next_due - now)
        if not self._dirty.is_set():
            # Wait for next content update — do NOT screenshot (would force draw).
            return 0.02
        if self._grab_busy or not self._encode_q.empty():
            return 0.02
        if not self._ensure_encode_deps():
            return 0.2

        stream_w = max(1, int(target.get("width") or 854))
        stream_h = max(1, int(target.get("height") or 480))
        if not self._logged:
            print(
                f"hello-ouster-world: capture=post-update/native1.0 "
                f"{stream_w}x{stream_h}@{fps}fps (cap={CAPTURE_MAX_FPS})",
                flush=True,
            )
            self._logged = True

        self._grab_busy = True
        self._dirty.clear()
        self._next_due = now + period
        try:
            rgb = point_viz.get_screenshot(1.0)
            np = self._np
            if not isinstance(rgb, np.ndarray) or rgb.size == 0:
                return period
            if rgb.dtype != np.uint8:
                rgb = rgb.astype(np.uint8, copy=False)
            if rgb.ndim == 3:
                rgb = np.ascontiguousarray(rgb[:, :, :3])
            else:
                return period
            try:
                self._encode_q.put_nowait((rgb, stream_w, stream_h))
            except queue.Full:
                pass
        except Exception as exc:  # noqa: BLE001
            msg = repr(exc)
            if "already in progress" not in msg:
                print(f"hello-ouster-world: grab failed: {msg}", flush=True)
        finally:
            self._grab_busy = False
        return max(0.01, self._next_due - time.monotonic())


class VizProxy:
    """Thin wrapper around PointViz so we can hook update and run.

    PointViz's pybind methods cannot be overridden in place, so SimpleViz
    gets this proxy instead: update marks the scene dirty for capture, run
    starts the grab loop, and key handlers also register with KeyBridge.
    ``add_default_controls`` is unwrapped to the real PointViz at module load.
    """

    def __init__(
        self,
        real: PointViz,
        publisher: FramePublisher,
        on_run: Callable[[Any], None],
        keys: KeyBridge,
    ):
        object.__setattr__(self, "_real", real)
        object.__setattr__(self, "_publisher", publisher)
        object.__setattr__(self, "_on_run", on_run)
        object.__setattr__(self, "_keys", keys)

    def push_key_handler(self, handler: Callable) -> None:
        self._keys.push_handler(handler)
        self._real.push_key_handler(handler)

    def update(self) -> None:
        self._keys.flush(self._real)
        self._real.update()
        self._publisher.note_scene_update()

    def run(self) -> None:
        self._on_run(self._real)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._real, name)


class JetstreamDisplayBridge:
    """Bridges between the browser's display panel and the HDMI output, 
       allowing the browser's display panel to mirror the HDMI output 
       and control it via mouse/keyboard.
    
        -Creates a shared PointViz (with capture + remote mouse/keyboard)
        -Sets playback rate / EOF behavior from the source type
        -Publishes app-socket waiting/connected status for the UI
    """

    def __init__(self, source_type: Any, frame_set_source: str) -> None:
        self.source_type = parse_source(source_type)
        self.frame_set_source = frame_set_source
        # Play once; outer main() reopens live/app sources or idles after OSF EOF.
        self.on_eof = "stop"
        self.rate: Optional[float] = None if self.is_app else 1.0
        self.pointviz: Any = None
        self._source: Any = None
        self._publisher: Optional[FramePublisher] = None
        self._keys: Optional[KeyBridge] = None
        self._demand: Optional[DemandController] = None

    @property
    def is_app(self) -> bool:
        return is_app_source(self.source_type)

    def __enter__(self) -> "JetstreamDisplayBridge":
        if self.is_app:
            self._set_status(
                AppSocketStatus.WAITING,
                f"Waiting for app FrameSets on {self.frame_set_source}",
            )
        else:
            self._set_status(AppSocketStatus.N_A)

        self._start_bridges()
        self.pointviz = self._build_pointviz()
        return self

    def __exit__(self, *exc: Any) -> None:
        if self.is_app:
            self._set_status(
                AppSocketStatus.DISCONNECTED,
                f"App source ended ({self.frame_set_source}). "
                "Restart the producer — visualization resumes automatically.",
            )
        if self._source is not None:
            close = getattr(self._source, "close", None)
            if callable(close):
                try:
                    close()
                except Exception:
                    pass
            self._source = None
        self._close_bridges()
        self._destroy_pointviz()
        return None

    def prepare(self, simple: Any, source: Any) -> None:
        """After SimpleViz is constructed: bind source, keys, and status."""
        self._source = source
        on_disconnect = getattr(source, "set_on_disconnect", None)
        if callable(on_disconnect):
            on_disconnect(self.request_stop)

        if self.is_app:
            self._set_status(
                AppSocketStatus.CONNECTED,
                f"Connected to {self.frame_set_source}",
            )

        assert self.pointviz is not None and self._demand is not None
        self._demand.attach(simple)

    def request_stop(self) -> None:
        """Unblock PointViz.run so SimpleViz can exit when a live source ends."""
        viz = self.pointviz
        if viz is None:
            return
        try:
            viz.running(False)
        except Exception as exc:  # noqa: BLE001
            print(f"hello-ouster-world: request_stop failed: {exc!r}", flush=True)

    def _destroy_pointviz(self) -> None:
        viz = self.pointviz
        self.pointviz = None
        if viz is None:
            return
        for name, arg in (("running", False), ("visible", False)):
            try:
                getattr(viz, name)(arg)
            except Exception:
                pass

    def _set_status(self, bridge: AppSocketStatus, message: str = "") -> None:
        set_runtime_status(source=self.source_type, bridge=bridge, message=message)

    def _start_bridges(self) -> None:
        stream_target = StreamTarget()
        self._publisher = FramePublisher(stream_target)
        self._keys = KeyBridge()
        self._demand = DemandController(stream_target)
        for bridge in (self._publisher, self._keys, self._demand):
            bridge.start()

    def _build_pointviz(self) -> VizProxy:
        print("hello-ouster-world: constructing PointViz + capture proxy", flush=True)
        real = PointViz("Ouster Viz", maximized=True, fullscreen=False)
        assert self._publisher is not None and self._keys is not None
        return VizProxy(real, self._publisher, self._run_with_capture, self._keys)

    def _run_with_capture(self, viz: Any) -> None:
        """PointViz.run + background JPEG grab for the MC Display stream."""
        print("hello-ouster-world: PointViz.run + post-update capture", flush=True)
        publisher = self._publisher
        assert publisher is not None
        stop = threading.Event()

        def capture_loop() -> None:
            while not stop.is_set():
                try:
                    if not viz.running():
                        break
                    time.sleep(publisher.try_grab(viz))
                except Exception as exc:  # noqa: BLE001
                    print(
                        f"hello-ouster-world: capture loop error: {exc!r}",
                        flush=True,
                    )
                    time.sleep(0.1)

        try:
            viz.running(True)
            viz.visible(True)
        except Exception as exc:  # noqa: BLE001
            print(f"hello-ouster-world: visible/running failed: {exc!r}", flush=True)

        cap_thread = threading.Thread(
            name="vizframe-grab", target=capture_loop, daemon=True
        )
        cap_thread.start()
        try:
            type(viz).run(viz)
        finally:
            stop.set()
            publisher.note_scene_update()
            try:
                viz.running(False)
            except Exception:
                pass
            try:
                viz.visible(False)
            except Exception:
                pass
            cap_thread.join(timeout=2.0)
            self._close_bridges()

    def _close_bridges(self) -> None:
        for obj in (self._publisher, self._keys, self._demand):
            if obj is None:
                continue
            try:
                obj.close()
            except Exception:
                pass
        self._publisher = None
        self._keys = None
        self._demand = None

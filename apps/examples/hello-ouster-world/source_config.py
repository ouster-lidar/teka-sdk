"""Read FrameSet source selection from ``source-config.json`` (Rust-owned).

Also writes ``source-runtime.json`` under the app runtime dir so the MC UI can
show app-socket waiting / connected / disconnected state.

Source selection is JSON-only (Display dropdown / ``POST /api/v1/source-config``).
"""

from __future__ import annotations

import ctypes
import json
import os
import select
import sys
import time
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any, Dict, Optional, Tuple, Union

_LOG_INTERVAL_S = 5.0


class FrameSetSourceType(str, Enum):
    """Values of ``source`` in ``source-config.json`` / Display dropdown."""
    LOCAL_FILE = "local_file"
    APP        = "app"
    SENSOR     = "sensor"


class AppSocketStatus(str, Enum):
    """App-socket lifecycle in ``source-runtime.json`` (Display UI)."""
    N_A          = "n/a"
    WAITING      = "waiting"
    CONNECTED    = "connected"
    DISCONNECTED = "disconnected"


@dataclass(frozen=True)
class SourceDefaults:
    """Paths for source selection JSON and field fallbacks when omitted."""

    config_path: str = "/var/lib/hello-ouster-world/source-config.json"
    runtime_status_path: str = "/run/hello-ouster-world/source-runtime.json"
    osf_path: str = ""
    app_socket: str = ""
    sensor_hostname: str = ""

    def as_config(self) -> Dict[str, Any]:
        return {
            "source": FrameSetSourceType.LOCAL_FILE.value,
            "osf_path": self.osf_path,
            "sensor_hostname": self.sensor_hostname,
            "app_socket": self.app_socket,
            "source_enabled": True,
        }


def set_runtime_status(
    *,
    source: Union[FrameSetSourceType, str],
    bridge: Union[AppSocketStatus, str] = AppSocketStatus.N_A,
    message: str = "",
) -> None:
    """Atomically publish viz-side source/app status for the Rust API / UI."""
    status_path = Path(SourceDefaults().runtime_status_path)
    payload = {
        "source": source.value if isinstance(source, FrameSetSourceType) else str(source),
        "bridge": bridge.value if isinstance(bridge, AppSocketStatus) else str(bridge),
        "message": message,
    }
    try:
        status_path.parent.mkdir(parents=True, exist_ok=True)
        tmp = status_path.with_suffix(".json.tmp")
        tmp.write_text(json.dumps(payload), encoding="utf-8")
        os.replace(tmp, status_path)
    except OSError as exc:
        print(f"hello-ouster-world: source-runtime write failed: {exc!r}", flush=True)


def parse_source(src: Union[FrameSetSourceType, str, None] = None) -> FrameSetSourceType:
    """Parse a stored/UI source id (``local_file`` / ``app`` / ``sensor``)."""
    if isinstance(src, FrameSetSourceType):
        return src
    s = str(src or FrameSetSourceType.LOCAL_FILE.value).strip().lower()
    if s == FrameSetSourceType.APP.value:
        return FrameSetSourceType.APP
    try:
        return FrameSetSourceType(s)
    except ValueError:
        return FrameSetSourceType.LOCAL_FILE


def is_app_source(source: Union[FrameSetSourceType, str]) -> bool:
    return parse_source(source) is FrameSetSourceType.APP


def _file_ok(path: str) -> bool:
    try:
        return os.path.isfile(path) and os.path.getsize(path) > 0
    except OSError:
        return False


def wait_for_osf(path: str, *, defaults: Optional[SourceDefaults] = None) -> str:
    """Block until an OSF exists. Empty path reloads source-config (Display UI)."""
    d = defaults or SourceDefaults()
    while True:
        resolved = str(path).strip() or str(load(defaults=d).get("osf_path") or "").strip()
        if resolved and _file_ok(resolved):
            return resolved
        print(
            "hello-ouster-world: waiting for OSF "
            f"({resolved or 'none selected'}); upload or pick a catalog example in the Display UI",
            flush=True,
        )
        time.sleep(_LOG_INTERVAL_S)
        path = ""


def resolve_source(
    cfg: Dict[str, Any],
    *,
    defaults: Optional[SourceDefaults] = None,
) -> Tuple[FrameSetSourceType, str]:
    """Resolve source type + path/hostname/sock for ``open_source()``."""
    mode = parse_source(cfg.get("source"))
    if mode is FrameSetSourceType.APP:
        sock = str(cfg.get("app_socket") or "").strip()
        if not sock:
            raise SystemExit(
                "hello-ouster-world: source=app requires app_socket "
                "(e.g. /run/<app-id>/frameset.sock)"
            )
        return mode, sock
    if mode is FrameSetSourceType.SENSOR:
        host = str(cfg.get("sensor_hostname") or "").strip()
        if not host:
            raise SystemExit(
                "hello-ouster-world: source=sensor requires sensor_hostname "
                "(set in source-config.json)"
            )
        return mode, host
    osf = str(cfg.get("osf_path") or "").strip()
    return mode, wait_for_osf(osf, defaults=defaults)


def load(*, defaults: Optional[SourceDefaults] = None) -> Dict[str, Any]:
    """Load ``source-config.json``, filling defaults when absent or incomplete."""
    d = defaults or SourceDefaults()
    cfg = d.as_config()
    path = Path(d.config_path)
    if path.is_file():
        try:
            with path.open() as f:
                raw = json.load(f)
            if isinstance(raw, dict):
                cfg.update(raw)
        except (OSError, json.JSONDecodeError) as exc:
            print(f"hello-ouster-world: source-config load failed: {exc!r}", flush=True)

    cfg["source"] = parse_source(str(cfg.get("source", ""))).value
    if not cfg.get("osf_path"):
        cfg["osf_path"] = d.osf_path
    sock = cfg.get("app_socket") or d.app_socket
    cfg["app_socket"] = sock if isinstance(sock, str) else d.app_socket
    if not isinstance(cfg.get("sensor_hostname"), str) or not cfg.get("sensor_hostname"):
        cfg["sensor_hostname"] = d.sensor_hostname
    return cfg


def config_fingerprint(cfg: Dict[str, Any]) -> str:
    """Stable fingerprint of persisted source fields SimpleViz reads on startup."""
    keys = ("source", "osf_path", "sensor_hostname", "app_socket", "producer_kind", "via_dynamo", "record_enabled", "record_filename", "source_enabled")
    payload = {k: cfg.get(k) for k in keys}
    return json.dumps(payload, sort_keys=True, separators=(",", ":"))


@dataclass
class SourceConfigWatch:
    """Detect ``source-config.json`` rewrites while SimpleViz is running (Apply / ^C)."""

    active_fingerprint: str
    defaults: SourceDefaults = field(default_factory=SourceDefaults)
    _start_mtime_ns: int = field(init=False, repr=False)

    def __post_init__(self) -> None:
        path = Path(self.defaults.config_path)
        try:
            self._start_mtime_ns = path.stat().st_mtime_ns
        except OSError:
            self._start_mtime_ns = 0

    def reload_requested(self) -> bool:
        path = Path(self.defaults.config_path)
        try:
            mtime = path.stat().st_mtime_ns
        except OSError:
            return False
        if mtime == self._start_mtime_ns:
            return False
        try:
            config_fingerprint(load(defaults=self.defaults))
        except Exception:
            return False
        return True


def wait_for_source_config_change(
    active_fingerprint: str,
    *,
    defaults: Optional[SourceDefaults] = None,
) -> None:
    """Block until ``source-config.json`` differs from ``active_fingerprint``.

    Apply saves ``source-config.json``; SimpleViz polls mtime while running and
    ``execv`` restarts the process for a clean GLFW teardown.
    """
    d = defaults or SourceDefaults()
    path = Path(d.config_path)

    def changed() -> bool:
        return config_fingerprint(load(defaults=d)) != active_fingerprint

    if changed():
        return

    if not sys.platform.startswith("linux"):
        raise RuntimeError("wait_for_source_config_change requires Linux inotify")

    inotify = _LinuxInotify()
    try:
        inotify.add_watch(path)
        if path.parent != path:
            inotify.add_watch_dir(path.parent)
        while True:
            inotify.wait()
            if changed():
                return
    finally:
        inotify.close()


class _LinuxInotify:
    """Minimal inotify wrapper (stdlib only, no PyPI deps)."""

    _IN_MODIFY = 0x00000002
    _IN_CLOSE_WRITE = 0x00000008
    _IN_ATTRIB = 0x00000004
    _IN_DELETE_SELF = 0x00000400
    _IN_MOVE_SELF = 0x00000800
    _IN_CREATE = 0x00000100
    _IN_MOVED_TO = 0x00000040
    _IN_NONBLOCK = 0x00000800

    def __init__(self) -> None:
        libc = ctypes.CDLL("libc.so.6", use_errno=True)
        self._libc = libc
        fd = int(libc.inotify_init1(self._IN_NONBLOCK))
        if fd < 0:
            err = ctypes.get_errno()
            raise OSError(err, os.strerror(err))
        self._fd = fd

    def add_watch(self, path: Path) -> None:
        mask = (
            self._IN_MODIFY
            | self._IN_CLOSE_WRITE
            | self._IN_ATTRIB
            | self._IN_DELETE_SELF
            | self._IN_MOVE_SELF
        )
        self._add_watch_path(path, mask)

    def add_watch_dir(self, path: Path) -> None:
        self._add_watch_path(path, self._IN_CREATE | self._IN_MOVED_TO)

    def _add_watch_path(self, path: Path, mask: int) -> None:
        wd = int(self._libc.inotify_add_watch(self._fd, os.fsencode(str(path)), mask))
        if wd < 0:
            err = ctypes.get_errno()
            raise OSError(err, os.strerror(err))

    def wait(self) -> None:
        while True:
            try:
                select.select([self._fd], [], [])
            except InterruptedError:
                raise KeyboardInterrupt from None
            if os.read(self._fd, 4096):
                return

    def close(self) -> None:
        os.close(self._fd)


def load_record_runtime() -> Dict[str, Any]:
    path = Path(os.environ.get("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR", "/run/hello-ouster-world"))
    path = path / "record-runtime.json"
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return raw if isinstance(raw, dict) else {}

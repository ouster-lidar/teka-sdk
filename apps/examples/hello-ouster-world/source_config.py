"""Read the Rust-owned source config and publish the shared display status."""

from __future__ import annotations

import ctypes
import json
import os
import select
import sys
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any, Dict, Optional, Union


class FrameSetSourceType(str, Enum):
    LOCAL_FILE = "local_file"
    SENSOR = "sensor"


@dataclass(frozen=True)
class SourceDefaults:
    config_path: str = "/var/lib/hello-ouster-world/source-config.json"
    runtime_status_path: str = "/run/hello-ouster-world/source-runtime.json"
    osf_path: str = ""
    sensor_hostname: str = ""

    def as_config(self) -> Dict[str, Any]:
        return {
            "source": FrameSetSourceType.SENSOR.value,
            "osf_path": self.osf_path,
            "sensor_hostname": self.sensor_hostname,
            "record_enabled": False,
            "record_filename": "",
            "record_duration_seconds": 120,
            "source_enabled": True,
        }


def set_runtime_status(state: str, message: str = "") -> None:
    """Atomically publish the status shared by the browser and HDMI display."""
    path = Path(
        os.environ.get("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR", "/run/hello-ouster-world")
    ) / "source-runtime.json"
    payload = {"state": state, "message": message}
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(f"{path.name}.{os.getpid()}.tmp")
        tmp.write_text(json.dumps(payload), encoding="utf-8")
        os.replace(tmp, path)
    except OSError as exc:
        print(f"hello-ouster-world: source-runtime write failed: {exc!r}", flush=True)


def load(*, defaults: Optional[SourceDefaults] = None) -> Dict[str, Any]:
    """Load validated source fields; expose malformed persisted data as an error."""
    d = defaults or SourceDefaults()
    cfg = d.as_config()
    path = Path(d.config_path)
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return cfg
    except (OSError, json.JSONDecodeError) as exc:
        cfg["config_error"] = f"invalid source configuration: {exc}"
        return cfg

    if not isinstance(raw, dict):
        cfg["config_error"] = "invalid source configuration: expected an object"
        return cfg
    cfg.update(raw)

    try:
        cfg["source"] = FrameSetSourceType(str(cfg.get("source", ""))).value
    except ValueError:
        cfg["config_error"] = "unsupported source value; select a supported source and Apply"
        return cfg

    for key in ("osf_path", "sensor_hostname", "record_filename"):
        value = cfg.get(key, "")
        if not isinstance(value, str):
            cfg["config_error"] = f"invalid source configuration: {key} must be a string"
            return cfg
        cfg[key] = value.strip()
    for key, default in (("record_enabled", False), ("source_enabled", True)):
        value = cfg.get(key, default)
        if not isinstance(value, bool):
            cfg["config_error"] = f"invalid source configuration: {key} must be a boolean"
            return cfg
        cfg[key] = value
    duration = cfg["record_duration_seconds"]
    if type(duration) is not int or not 1 <= duration <= 3600:
        cfg["config_error"] = "record duration must be between 1 and 3600 seconds"
        return cfg
    if cfg["source"] != FrameSetSourceType.SENSOR.value:
        cfg["record_enabled"] = False
    cfg.pop("config_error", None)
    return cfg


@dataclass
class SourceConfigWatch:
    """Detect atomic source-config replacements while SimpleViz is running."""

    defaults: SourceDefaults = field(default_factory=SourceDefaults)
    _start_mtime_ns: int = field(init=False, repr=False)

    def __post_init__(self) -> None:
        self._start_mtime_ns = _mtime_ns(Path(self.defaults.config_path))

    def reload_requested(self) -> bool:
        return _mtime_ns(Path(self.defaults.config_path)) != self._start_mtime_ns


def _mtime_ns(path: Path) -> int:
    try:
        return path.stat().st_mtime_ns
    except OSError:
        return 0


def wait_for_source_config_change(config_watch: SourceConfigWatch) -> None:
    """Wait for an explicit config-file write, including an unchanged Ctrl-C apply."""
    path = Path(config_watch.defaults.config_path)
    parent = path.parent
    parent.mkdir(parents=True, exist_ok=True)

    if not sys.platform.startswith("linux"):
        raise RuntimeError("wait_for_source_config_change requires Linux inotify")

    inotify = _LinuxInotify()
    try:
        if path.exists():
            inotify.add_watch(path)
        inotify.add_watch_dir(parent)
        if config_watch.reload_requested():
            return
        while True:
            inotify.wait()
            if config_watch.reload_requested():
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
    try:
        raw = json.loads((path / "record-runtime.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return raw if isinstance(raw, dict) else {}

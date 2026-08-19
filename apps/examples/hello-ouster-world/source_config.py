"""Read FrameSet source selection from ``source-config.json`` (Rust-owned).

Also writes ``source-runtime.json`` under the app runtime dir so the MC UI can
show app-socket waiting / connected / disconnected state.

Source selection is JSON-only (Display dropdown / ``POST /api/v1/source-config``).
"""

from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass
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
            f"({resolved or 'none selected'}); upload or Download one in the Display UI",
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

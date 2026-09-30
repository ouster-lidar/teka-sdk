#!/usr/bin/env python3
"""HDMI SimpleViz for the FrameSet source selected in source-config.json.

Canonical SDK shape (same idea as ouster-sdk examples)::

    src = open_source(frame_set_source)

    def fun():
        for frames in src:
            for frame in frames:
                if frame is None:
                    continue
                # custom LidarFrame processing
            yield frames

    SimpleViz(src.sensor_info, rate=1, on_eof="stop").run(fun())

``frame_set_source`` comes from the Display **FrameSet source** dropdown
(OSF path / sensor host / app ``/run/<app>/frameset.sock``).
App peers (e.g. ouster-dynamo, installed separately) embed ``trk_objs`` /
``ego_obj`` on the FrameSet (``frames.objects``).
"""

from __future__ import annotations

import atexit
import os
import sys
import time
from pathlib import Path
from typing import Optional

from osf_socket import open_source
from ouster.sdk import osf
from ouster.sdk.viz import SimpleViz

from dynamo_viz_utils import clear_slam_viz_accumulators, enable_app_source_overlays
from ouster_dynamo_api import unpack_dynamo_objects
from source_config import (
    FrameSetSourceType,
    SourceConfigWatch,
    SourceDefaults,
    AppSocketStatus,
    config_fingerprint,
    load as load_source_config,
    load_record_runtime,
    resolve_source,
    set_runtime_status,
    wait_for_source_config_change,
)
from viz_utils import JetstreamDisplayBridge, VizPid

SOURCE_DEFAULTS = SourceDefaults(
    config_path="/var/lib/hello-ouster-dynamo/source-config.json",
    osf_path="",
    app_socket="/run/ouster-dynamo/frameset.sock",
    sensor_hostname="",
)

# Written by ouster-dynamo on Reset ego; hello-ouster-dynamo consumes + unlinks.
_CLEAR_SLAM_VIZ_FLAG = Path("/run/ouster-dynamo/clear_slam_viz")

_active_bridge: Optional[JetstreamDisplayBridge] = None
_record_runtime_cache: Optional[dict] = None
_record_runtime_mtime: Optional[float] = None


def _record_runtime_path() -> Path:
    return Path(
        os.environ.get("HELLO_OUSTER_DYNAMO_XDG_RUNTIME_DIR", "/run/hello-ouster-dynamo")
    ) / "record-runtime.json"


def _get_record_runtime() -> dict:
    global _record_runtime_cache, _record_runtime_mtime
    path = _record_runtime_path()
    try:
        mtime = path.stat().st_mtime
    except OSError:
        _record_runtime_cache = {}
        _record_runtime_mtime = None
        return {}
    if _record_runtime_mtime != mtime:
        _record_runtime_cache = load_record_runtime()
        _record_runtime_mtime = mtime
    return _record_runtime_cache or {}


def _source_enabled(cfg: dict) -> bool:
    v = cfg.get("source_enabled")
    if v is None:
        return True
    return bool(v)


def _consume_clear_slam_viz_flag() -> bool:
    if not _CLEAR_SLAM_VIZ_FLAG.is_file():
        return False
    try:
        _CLEAR_SLAM_VIZ_FLAG.unlink()
    except OSError:
        pass
    return True


def main() -> FrameSetSourceType:
    global _active_bridge
    src_config = load_source_config(defaults=SOURCE_DEFAULTS)
    src_type, frame_set_source = resolve_source(src_config, defaults=SOURCE_DEFAULTS)

    with JetstreamDisplayBridge(src_type, frame_set_source) as display_ctx:
        _active_bridge = display_ctx
        try:
            src = open_source(frame_set_source)

            # Hold SensorInfo identities for SimpleViz; osf_socket mutates
            # sensor_to_body in place when Dynamo live-config extrinsics change.
            viz_sensor_info = list(src.sensor_info)
            viz_holder: list = []
            osf_writer = None
            osf_writer_path = ""
            loop_osf = src_type is FrameSetSourceType.LOCAL_FILE

            def make_fun(active_src):
                def fun():
                    nonlocal osf_writer, osf_writer_path
                    for frames in active_src:
                        if active_src.sensor_info and len(active_src.sensor_info) == len(
                            viz_sensor_info
                        ):
                            for dst, src_si in zip(viz_sensor_info, active_src.sensor_info):
                                try:
                                    dst.sensor_to_body = src_si.sensor_to_body
                                except Exception:
                                    pass
                        unpacked = unpack_dynamo_objects(frames)
                        if (
                            (unpacked.clear_slam_viz or _consume_clear_slam_viz_flag())
                            and viz_holder
                        ):
                            clear_slam_viz_accumulators(viz_holder[0])
                            print(
                                "hello-ouster-dynamo: cleared SLAM viz accumulators "
                                "(Dynamo reset ego)",
                                flush=True,
                            )
                        rec = (
                            _get_record_runtime()
                            if src_type is FrameSetSourceType.SENSOR
                            else {}
                        )
                        rec_path = str(rec.get("path") or "").strip()
                        want = (
                            src_type is FrameSetSourceType.SENSOR
                            and rec.get("active")
                            and rec_path
                        )
                        if want:
                            if osf_writer is None or osf_writer_path != rec_path:
                                _close_osf_writer(osf_writer)
                                osf_writer = osf.Writer(rec_path, active_src.sensor_info)
                                osf_writer_path = rec_path
                                print(f"hello-ouster-dynamo: recording {rec_path}", flush=True)
                            try:
                                osf_writer.save(frames)
                            except Exception as exc:  # noqa: BLE001
                                print(
                                    f"hello-ouster-dynamo: OSF save failed: {exc!r}",
                                    flush=True,
                                )
                        elif osf_writer is not None:
                            _close_osf_writer(osf_writer)
                            osf_writer = None
                            osf_writer_path = ""
                            print("hello-ouster-dynamo: recording closed", flush=True)
                        yield frames

                return fun

            viz = SimpleViz(
                viz_sensor_info,
                rate=display_ctx.rate,
                on_eof="loop" if loop_osf else display_ctx.on_eof,
                _override_pointviz=display_ctx.pointviz,
            )
            viz_holder.append(viz)

            config_watch = SourceConfigWatch(
                config_fingerprint(load_source_config(defaults=SOURCE_DEFAULTS)),
                defaults=SOURCE_DEFAULTS,
            )
            display_ctx.prepare(viz, src, config_watch=config_watch)
            if src_type is FrameSetSourceType.APP:
                enable_app_source_overlays(viz)
            try:
                if loop_osf:
                    viz.run(src)
                else:
                    viz.run(make_fun(src)())
            finally:
                _close_osf_writer(osf_writer)
        finally:
            _active_bridge = None

    return src_type


def _close_osf_writer(writer) -> None:
    if writer is None:
        return
    close = getattr(writer, "close", None)
    if callable(close):
        try:
            close()
        except Exception as exc:  # noqa: BLE001
            print(f"hello-ouster-dynamo: OSF writer close failed: {exc!r}", flush=True)


def _reexec_viz() -> None:
    """Replace this process so GLFW cannot keep a leftover Ouster Viz window."""
    os.execv(sys.executable, [sys.executable, *sys.argv])


if __name__ == "__main__":
    viz_pid = VizPid()
    viz_pid.write()
    atexit.register(viz_pid.clear)
    while True:
        try:
            src_config = load_source_config(defaults=SOURCE_DEFAULTS)
            if not _source_enabled(src_config):
                set_runtime_status(
                    source=src_config.get("source", FrameSetSourceType.LOCAL_FILE.value),
                    bridge=AppSocketStatus.N_A,
                    message="Source stopped — select source and Apply to resume",
                )
                print(
                    "hello-ouster-dynamo: source stopped; waiting for Apply",
                    flush=True,
                )
                wait_for_source_config_change(
                    config_fingerprint(src_config),
                    defaults=SOURCE_DEFAULTS,
                )
                continue
            main()
            print("hello-ouster-dynamo: SimpleViz exited; restarting", flush=True)
            _reexec_viz()
        except KeyboardInterrupt:
            print(
                "hello-ouster-dynamo: interrupted (^C); restarting SimpleViz",
                flush=True,
            )
            _reexec_viz()
        except SystemExit as exc:
            if exc.code not in (0, None):
                print(
                    f"hello-ouster-dynamo: exit {exc.code!r}; retrying in 5s",
                    flush=True,
                )
                time.sleep(5)
                _reexec_viz()
            raise
        except Exception as exc:  # noqa: BLE001 — keep HDMI loop alive
            print(f"hello-ouster-dynamo: error: {exc!r}; retrying in 5s", flush=True)
            time.sleep(5)
            _reexec_viz()

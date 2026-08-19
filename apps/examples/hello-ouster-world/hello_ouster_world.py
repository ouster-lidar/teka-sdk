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

from osf_socket import open_source
from ouster.sdk.viz import SimpleViz

from dynamo_viz_utils import clear_slam_viz_accumulators, enable_app_source_overlays
from ouster_dynamo_api import unpack_dynamo_objects
from source_config import (
    FrameSetSourceType,
    SourceDefaults,
    load as load_source_config,
    resolve_source,
)
from viz_utils import JetstreamDisplayBridge, VizPid

SOURCE_DEFAULTS = SourceDefaults(
    config_path="/var/lib/hello-ouster-world/source-config.json",
    osf_path="",
    app_socket="/run/ouster-dynamo/frameset.sock",
    sensor_hostname="",
)

# Written by ouster-dynamo on Reset ego; hello-ouster-world consumes + unlinks.
_CLEAR_SLAM_VIZ_FLAG = Path("/run/ouster-dynamo/clear_slam_viz")


def _consume_clear_slam_viz_flag() -> bool:
    if not _CLEAR_SLAM_VIZ_FLAG.is_file():
        return False
    try:
        _CLEAR_SLAM_VIZ_FLAG.unlink()
    except OSError:
        pass
    return True


def main() -> FrameSetSourceType:
    src_config = load_source_config(defaults=SOURCE_DEFAULTS)
    src_type, frame_set_source = resolve_source(src_config, defaults=SOURCE_DEFAULTS)

    with JetstreamDisplayBridge(src_type, frame_set_source) as display_ctx:

        src = open_source(frame_set_source)

        # Hold SensorInfo identities for SimpleViz; osf_socket mutates
        # sensor_to_body in place when Dynamo live-config extrinsics change.
        viz_sensor_info = list(src.sensor_info)
        viz_holder: list = []

        def fun():
            for frames in src:
                # Belt-and-suspenders if the source replaces its sensor_info list.
                if src.sensor_info and len(src.sensor_info) == len(viz_sensor_info):
                    for dst, src_si in zip(viz_sensor_info, src.sensor_info):
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
                        "hello-ouster-world: cleared SLAM viz accumulators "
                        "(Dynamo reset ego)",
                        flush=True,
                    )
                trk_objs = unpacked.tracked
                ego_obj = unpacked.ego
                ouster_dynamo_api_version = unpacked.ouster_dynamo_api_version
                for frame in frames:
                    if frame is None:
                        continue
                    # Custom LidarFrame / object processing:
                    # trk_objs, ego_obj, ouster_dynamo_api_version
                    # SimpleViz still draws from FrameSet.objects on yield.
                yield frames

        viz = SimpleViz(
            viz_sensor_info,
            rate=display_ctx.rate,
            on_eof=display_ctx.on_eof,
            _override_pointviz=display_ctx.pointviz,
        )
        viz_holder.append(viz)

        display_ctx.prepare(viz, src)
        if src_type is FrameSetSourceType.APP:
            enable_app_source_overlays(viz)
        viz.run(fun())

    return src_type


def _reexec_viz() -> None:
    """Replace this process so GLFW cannot keep a leftover Ouster Viz window.

    Sway stays up (this Python is the ``exec`` child). Same pid → viz.pid still
    valid. ``execv`` skips atexit, which is what we want.
    """
    os.execv(sys.executable, [sys.executable, *sys.argv])


if __name__ == "__main__":
    viz_pid = VizPid()
    viz_pid.write()
    atexit.register(viz_pid.clear)
    while True:
        try:
            src_type = main()
            if src_type is FrameSetSourceType.LOCAL_FILE:
                # One-shot OSF: idle until source Apply SIGINTs the viz process.
                print(
                    "hello-ouster-world: OSF playback finished; waiting for source change",
                    flush=True,
                )
                while True:
                    time.sleep(3600)
            print("hello-ouster-world: SimpleViz exited; restarting", flush=True)
            _reexec_viz()
        except KeyboardInterrupt:
            print(
                "hello-ouster-world: interrupted (^C); restarting SimpleViz",
                flush=True,
            )
            _reexec_viz()
        except SystemExit as exc:
            if exc.code not in (0, None):
                print(
                    f"hello-ouster-world: exit {exc.code!r}; retrying in 5s",
                    flush=True,
                )
                time.sleep(5)
                _reexec_viz()
            raise
        except Exception as exc:  # noqa: BLE001 — keep HDMI loop alive
            print(f"hello-ouster-world: error: {exc!r}; retrying in 5s", flush=True)
            time.sleep(5)
            _reexec_viz()

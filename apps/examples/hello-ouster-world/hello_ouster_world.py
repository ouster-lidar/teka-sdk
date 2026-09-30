#!/usr/bin/env python3
"""Display an OSF file or a directly connected Ouster sensor with SimpleViz."""

from __future__ import annotations

import atexit
import os
import queue
import sys
import threading
import time
from pathlib import Path
from typing import Optional

from ouster.sdk import open_source, osf
from ouster.sdk.viz import SimpleViz

from source_config import (
    FrameSetSourceType,
    SourceConfigWatch,
    SourceDefaults,
    load as load_source_config,
    load_record_runtime,
    set_runtime_status,
    wait_for_source_config_change,
)
from viz_utils import JetstreamDisplayBridge, VizPid

SOURCE_DEFAULTS = SourceDefaults(
    config_path="/var/lib/hello-ouster-world/source-config.json",
    runtime_status_path="/run/hello-ouster-world/source-runtime.json",
)

_record_runtime_cache: Optional[dict] = None
_record_runtime_mtime: Optional[int] = None
_SOURCE_END = object()


class SourceConfigChanged(Exception):
    pass


def _record_runtime_path() -> Path:
    return Path(
        os.environ.get("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR", "/run/hello-ouster-world")
    ) / "record-runtime.json"


def _mark_record_writer(path: str, state: str) -> bool:
    if not path:
        return False
    marker = _record_runtime_path().with_name(f"record-writer-{state}")
    temporary = marker.with_suffix(".tmp")
    try:
        marker.parent.mkdir(parents=True, exist_ok=True)
        temporary.write_text(path + "\n", encoding="utf-8")
        os.replace(temporary, marker)
        if state == "closed":
            marker.with_name("record-writer-close-failed").unlink(missing_ok=True)
        return True
    except OSError as exc:
        print(f"hello-ouster-world: could not mark recording {state}: {exc!r}", flush=True)
        return False


def _record_writer_close_failed(path: str) -> bool:
    marker = _record_runtime_path().with_name("record-writer-close-failed")
    try:
        return marker.read_text(encoding="utf-8").strip() == path
    except OSError:
        return False


def _get_record_runtime() -> dict:
    global _record_runtime_cache, _record_runtime_mtime
    path = _record_runtime_path()
    try:
        mtime = path.stat().st_mtime_ns
    except OSError:
        _record_runtime_cache = {}
        _record_runtime_mtime = None
        return {}
    if _record_runtime_mtime != mtime:
        _record_runtime_cache = load_record_runtime()
        _record_runtime_mtime = mtime
    return _record_runtime_cache or {}


def _valid_osf(path: str) -> bool:
    try:
        return os.path.isfile(path) and os.path.getsize(path) > 0
    except OSError:
        return False


def _configure_sensor(hostname: str) -> None:
    """Apply the SDK's supported fresh-sensor defaults without persisting them."""
    from ouster.sdk import core, sensor

    active = sensor.get_config(hostname, active=True)
    config = core.SensorConfig()
    config.udp_port_lidar = 7502
    config.udp_port_imu = 7503
    config.azimuth_window = (0, 360000)
    config.signal_multiplier = 1
    config.operating_mode = core.OperatingMode.NORMAL

    # Match the pinned SDK CLI's compatibility defaults for sensors that use
    # the packet-count IMU configuration, while leaving all unrelated settings
    # untouched.
    if active.imu_packets_per_frame is not None:
        if (
            active.udp_profile_lidar != core.UDPProfileLidar.LEGACY
            and active.udp_profile_imu == core.UDPProfileIMU.LEGACY
        ):
            config.udp_profile_imu = core.UDPProfileIMU.ACCEL32_GYRO32_NMEA
        if active.accel_fsr != core.FullScaleRange.EXTENDED:
            config.accel_fsr = core.FullScaleRange.EXTENDED
        if active.gyro_fsr != core.FullScaleRange.EXTENDED:
            config.gyro_fsr = core.FullScaleRange.EXTENDED

    sensor.set_config(hostname, config, udp_dest_auto=True, persist=False)
    active = sensor.get_config(hostname, active=True)
    expected = {
        "udp_port_lidar": 7502,
        "udp_port_imu": 7503,
        "azimuth_window": (0, 360000),
        "signal_multiplier": 1,
        "operating_mode": core.OperatingMode.NORMAL,
    }
    mismatches = []
    for key, value in expected.items():
        actual = getattr(active, key, None)
        if key == "azimuth_window" and actual is not None:
            actual = tuple(actual)
        if actual != value:
            mismatches.append(f"{key}={actual!r} (expected {value!r})")
    if mismatches:
        raise RuntimeError("sensor did not accept defaults: " + ", ".join(mismatches))


def _validated_source(cfg: dict) -> tuple[FrameSetSourceType, str]:
    source = FrameSetSourceType(cfg["source"])
    target = str(cfg["osf_path"] if source is FrameSetSourceType.LOCAL_FILE else cfg["sensor_hostname"]).strip()
    if source is FrameSetSourceType.LOCAL_FILE and not _valid_osf(target):
        raise ValueError("selected OSF file is missing; select or upload a file and Apply")
    if source is FrameSetSourceType.SENSOR and not target:
        raise ValueError("select a discovered sensor or enter its hostname")
    return source, target


def _start_sensor_reader(src):
    frames = queue.Queue(maxsize=2)

    def read_source():
        try:
            for frame in src:
                frames.put((frame, None))
        except Exception as exc:  # noqa: BLE001
            frames.put((_SOURCE_END, exc))
        else:
            frames.put((_SOURCE_END, None))

    threading.Thread(name="sensor-source", target=read_source, daemon=True).start()
    return frames


def _next_sensor_frame(frames, config_watch):
    while True:
        if config_watch.reload_requested():
            raise SourceConfigChanged
        try:
            frame, error = frames.get(timeout=0.2)
        except queue.Empty:
            continue
        if frame is _SOURCE_END:
            if error is not None:
                raise error
            raise RuntimeError("sensor source ended before receiving a frame")
        return frame


def _sensor_frames(frames, first_frame, config_watch):
    yield first_frame
    while True:
        if config_watch.reload_requested():
            raise SourceConfigChanged
        try:
            frame, error = frames.get(timeout=0.2)
        except queue.Empty:
            continue
        if frame is _SOURCE_END:
            if error is not None:
                raise error
            return
        yield frame


def main(cfg: dict, config_watch: SourceConfigWatch) -> FrameSetSourceType:
    source_type, target = _validated_source(cfg)
    if config_watch.reload_requested():
        raise SourceConfigChanged
    if source_type is FrameSetSourceType.SENSOR:
        set_runtime_status("configuring", f"Applying supported defaults to {target}")
        try:
            _configure_sensor(target)
        except Exception as exc:  # noqa: BLE001
            set_runtime_status("configuration_error", str(exc))
            print(f"hello-ouster-world: sensor configuration failed: {exc!r}; waiting for Apply", flush=True)
            wait_for_source_config_change(config_watch)
            raise SourceConfigChanged from exc
        if config_watch.reload_requested():
            raise SourceConfigChanged

    if source_type is FrameSetSourceType.SENSOR:
        src = open_source(target, do_not_reinitialize=True, no_auto_udp_dest=True)
    else:
        src = open_source(target)
    try:
        return _run_viz(cfg, config_watch, source_type, target, src)
    finally:
        # Reloads execv; unclosed SDK UDP sockets would keep 7502/7503 bound.
        src.close()


def _run_viz(cfg, config_watch, source_type, target, src):
    sensor_frames = None
    first_sensor_frame = None
    if source_type is FrameSetSourceType.SENSOR:
        set_runtime_status("waiting_for_frames", f"Waiting for frames from {target}")
        sensor_frames = _start_sensor_reader(src)
        first_sensor_frame = _next_sensor_frame(sensor_frames, config_watch)

    with JetstreamDisplayBridge(source_type) as display_ctx:
        viz_sensor_info = list(src.sensor_info)
        osf_writer = None
        osf_writer_path = ""
        loop_osf = source_type is FrameSetSourceType.LOCAL_FILE
        first_frame = True
        writer_lock = threading.Lock()
        record_stop = threading.Event()

        def close_recording(path):
            nonlocal osf_writer, osf_writer_path
            if (
                osf_writer is not None
                and _close_osf_writer(osf_writer, osf_writer_path)
                and not _record_writer_close_failed(path)
            ):
                _mark_record_writer(osf_writer_path or path, "closed")
            osf_writer = None
            osf_writer_path = ""

        def recording_wanted(rec):
            return bool(rec.get("active") and rec.get("path")) and time.time() * 1000 < rec["expires_at"]

        def monitor_recording():
            # Finalize even when SimpleViz is paused or the sensor stops sending.
            while not record_stop.wait(0.1):
                with writer_lock:
                    rec = _get_record_runtime()
                    path = str(rec.get("path") or "")
                    if path and not recording_wanted(rec):
                        close_recording(path)
                        return

        def process_sensor_frames(frames):
            nonlocal osf_writer, osf_writer_path, first_frame
            for frame_set in frames:
                if first_frame:
                    first_frame = False
                    set_runtime_status("streaming", f"Receiving frames from {target}")
                if src.sensor_info and len(src.sensor_info) == len(viz_sensor_info):
                    for dst, src_si in zip(viz_sensor_info, src.sensor_info):
                        try:
                            dst.sensor_to_body = src_si.sensor_to_body
                        except Exception:
                            pass
                with writer_lock:
                    rec = _get_record_runtime()
                    rec_path = str(rec.get("path") or "").strip()
                    if recording_wanted(rec):
                        if osf_writer is None or osf_writer_path != rec_path:
                            close_recording(osf_writer_path)
                            if not _mark_record_writer(rec_path, "started"):
                                raise RuntimeError("could not announce recording writer")
                            # Stop may precede intent publication; reread before touching the OSF.
                            if load_record_runtime() != rec or not recording_wanted(rec):
                                _mark_record_writer(rec_path, "closed")
                            else:
                                try:
                                    osf_writer = osf.Writer(rec_path, src.sensor_info)
                                except Exception:
                                    _mark_record_writer(rec_path, "close-failed")
                                    raise
                                osf_writer_path = rec_path
                                print(f"hello-ouster-world: recording {rec_path}", flush=True)
                        if osf_writer is not None:
                            try:
                                osf_writer.save(frame_set)
                            except Exception as exc:  # noqa: BLE001
                                print(f"hello-ouster-world: OSF save failed: {exc!r}", flush=True)
                    elif osf_writer is not None:
                        close_recording(rec_path)
                yield frame_set

        viz = SimpleViz(
            viz_sensor_info,
            rate=display_ctx.rate,
            on_eof="loop" if loop_osf else "stop",
            _override_pointviz=display_ctx.pointviz,
        )
        display_ctx.prepare(viz, src, config_watch=config_watch)
        set_runtime_status(
            "playing_osf" if loop_osf else "waiting_for_frames",
            f"Opening {Path(target).name}" if loop_osf else f"Receiving frames from {target}",
        )
        record_thread = None
        if sensor_frames is not None and cfg["record_enabled"]:
            record_thread = threading.Thread(name="record-close", target=monitor_recording, daemon=True)
            record_thread.start()
        try:
            if sensor_frames is None:
                viz.run(src)
            else:
                frames = _sensor_frames(sensor_frames, first_sensor_frame, config_watch)
                viz.run(process_sensor_frames(frames))
        finally:
            record_stop.set()
            if record_thread is not None:
                record_thread.join()
            with writer_lock:
                close_recording(osf_writer_path)
    return source_type


def _close_osf_writer(writer, path: str) -> bool:
    if writer is None:
        return True
    close = getattr(writer, "close", None)
    if callable(close):
        try:
            close()
        except Exception as exc:  # noqa: BLE001
            print(f"hello-ouster-world: OSF writer close failed: {exc!r}", flush=True)
            _mark_record_writer(path, "close-failed")
            return False
    return True


def _reexec_viz() -> None:
    """Replace this process so GLFW cannot keep a leftover Ouster Viz window."""
    os.execv(sys.executable, [sys.executable, *sys.argv])


if __name__ == "__main__":
    viz_pid = VizPid()
    viz_pid.write()
    atexit.register(viz_pid.clear)
    while True:
        record = load_record_runtime()
        record_path = str(record.get("path") or "")
        if not record.get("active") and record_path:
            if _record_writer_close_failed(record_path):
                print(f"hello-ouster-world: leaving incomplete recording unavailable: {record_path}", flush=True)
            else:
                _mark_record_writer(record_path, "closed")
        config_watch = SourceConfigWatch(defaults=SOURCE_DEFAULTS)
        src_config = load_source_config(defaults=SOURCE_DEFAULTS)
        config_error = src_config.get("config_error")
        if config_error:
            set_runtime_status("configuration_error", config_error)
            print(f"hello-ouster-world: {config_error}; waiting for Apply", flush=True)
            wait_for_source_config_change(config_watch)
            continue
        if not src_config["source_enabled"]:
            set_runtime_status("stopped", "Source stopped — select a source and Apply to resume")
            wait_for_source_config_change(config_watch)
            continue
        source_type = FrameSetSourceType(src_config["source"])
        target = src_config["osf_path"] if source_type is FrameSetSourceType.LOCAL_FILE else src_config["sensor_hostname"]
        if source_type is FrameSetSourceType.SENSOR and not target:
            set_runtime_status("discovering", "Waiting for sensor discovery or manual selection")
            wait_for_source_config_change(config_watch)
            continue
        if source_type is FrameSetSourceType.LOCAL_FILE and not _valid_osf(target):
            set_runtime_status("waiting_for_osf", "Select or upload a valid OSF file and Apply")
            wait_for_source_config_change(config_watch)
            continue
        try:
            main(src_config, config_watch)
            print("hello-ouster-world: SimpleViz exited; restarting", flush=True)
            _reexec_viz()
        except KeyboardInterrupt:
            print("hello-ouster-world: interrupted (^C); restarting SimpleViz", flush=True)
            _reexec_viz()
        except SourceConfigChanged:
            print("hello-ouster-world: source config changed while waiting; reloading", flush=True)
            _reexec_viz()
        except Exception as exc:  # noqa: BLE001 — keep HDMI loop alive
            set_runtime_status("error", str(exc))
            print(f"hello-ouster-world: error: {exc!r}; retrying in 5s", flush=True)
            time.sleep(5)
            _reexec_viz()

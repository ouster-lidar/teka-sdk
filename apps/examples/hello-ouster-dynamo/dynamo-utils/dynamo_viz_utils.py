"""SimpleViz overlay helpers for app FrameSet sources (labels, box colors, OSD)."""
from __future__ import annotations

import math
import time
import types
from pathlib import Path
from typing import Any, Optional, Sequence, Tuple

import numpy as np

from configs.viz_config import VIZ_CONFIG, load_viz_config
from detect_utils import CLASS_COLORS, CLASS_IDS, ID_TO_CLASS, tracking_class_map_set
from ouster_dynamo_api import KEY_EGO, KEY_TRACKED
from ouster.sdk._bindings.client import Pose

from ouster.sdk.viz import PointViz
from ouster.sdk.viz.widgets import ToggleImage

try:
    from PIL import Image as _PilImage
    from PIL import ImageDraw as _PilImageDraw
    from PIL import ImageFont as _PilImageFont
except Exception:  # noqa: BLE001
    _PilImage = None  # type: ignore[misc, assignment]
    _PilImageDraw = None  # type: ignore[misc, assignment]
    _PilImageFont = None  # type: ignore[misc, assignment]

_label_font_cache: dict[int, Any] = {}
# PIL default bitmap vs gltext; keep panels a bit larger than raw default.
_PIL_FONT_SIZE_SCALE = 1.125
# Rasterize panels at a few discrete sizes; stretch on screen so orbit/pan
# only updates Image.position (set_image every mouse move is what lagged).
_PANEL_SCALE_BUCKET = 0.5
# Cap stream recapture while orbiting (HDMI still redraws every update).
_ORBIT_CAPTURE_MIN_DT_S = 0.1

# Cuboid color modes (cycled with V).
BOX_COLOR = "velocity"
BOX_COLOR_SPEED_MIN_MPS = 0.0
BOX_COLOR_SPEED_MAX_MPS = 11.0  # ~24 mph
BOX_COLOR_SLOW_RGB = (128, 128, 128)
BOX_COLOR_MED_RGB = (255, 255, 0)
BOX_COLOR_FAST_RGB = (255, 0, 0)
_BOX_COLOR_MODES = ("id", "class", "velocity")
_BOX_COLOR_MODE_LABELS = {
    "id": "object id",
    "class": "class color",
    "velocity": "velocity color",
}


def clear_slam_viz_accumulators(simple_viz: Any) -> None:
    """Clear SimpleViz MAP / ACCUM / trajectory after Dynamo reset ego.

    SimpleViz has no public clear-map API; poke the accumulators used for
    world-frame SLAM point clouds (key ``7`` MAP, key ``6`` ACCUM).
    """
    fv = getattr(simple_viz, "_frame_viz", None) or getattr(simple_viz, "_scan_viz", None)
    if fv is None:
        return
    try:
        clear_pose = getattr(fv, "clear_pose_histories", None)
        if callable(clear_pose):
            clear_pose()
        clear_traj = getattr(fv, "clear_trajectory", None)
        if callable(clear_traj):
            clear_traj()
    except Exception as exc:  # noqa: BLE001
        print(f"hello-ouster-dynamo: clear pose/trajectory failed: {exc!r}", flush=True)

    accum = getattr(fv, "_frames_accum", None)
    if accum is None:
        return
    try:
        clear_track = getattr(accum, "clear_track", None)
        if callable(clear_track):
            clear_track()
    except Exception as exc:  # noqa: BLE001
        print(f"hello-ouster-dynamo: clear_track failed: {exc!r}", flush=True)

    ma = getattr(accum, "_ma", None)
    if ma is not None:
        try:
            xyz = getattr(ma, "_map_xyz", None)
            if xyz is not None and getattr(xyz, "size", 0):
                xyz[:] = 0
            ma._map_idx = 0
            ma._map_overflow = False
            for arr in (getattr(ma, "_map_keys", None) or {}).values():
                try:
                    arr[:] = 0
                except Exception:  # noqa: BLE001
                    pass
            draw = getattr(ma, "_draw_map", None)
            if callable(draw):
                draw()
        except Exception as exc:  # noqa: BLE001
            print(f"hello-ouster-dynamo: clear MAP failed: {exc!r}", flush=True)

    sa = getattr(accum, "_sa", None)
    if sa is None:
        return
    try:
        for sensor_clouds in getattr(sa, "_sensor_clouds", None) or []:
            clouds_list = getattr(sensor_clouds, "_clouds_accum", None) or []
            for i in range(len(clouds_list)):
                remove = getattr(sensor_clouds, "remove_cloud", None)
                if callable(remove):
                    remove(i)
    except Exception as exc:  # noqa: BLE001
        print(f"hello-ouster-dynamo: clear ACCUM failed: {exc!r}", flush=True)


_DEFAULT_BILLBOARD_BG_RGBA = (0.0, 0.0, 0.0, 0.60)
_DEFAULT_LABEL_SCALE = 3.0

# PointViz Camera::view_distance uses LOG_DISTANCE_0 * exp(dolly * 0.01).
_CAMERA_LOG_DISTANCE_0 = 50.0
_CAMERA_FAR = 10000.0
_CAMERA_NEAR = 0.1
# gltext default/sans line height in font units (see gltext_font_*_meta.inc).
_GLTEXT_GLYPH_HEIGHT = 17.0
# 3D Labels scale font units by this many meters (GLLabel::draw).
_LABEL_WORLD_PER_FONT_UNIT = 0.02
# Per-glyph advances / line height. Tracked labels are space-padded; spaces are
# much narrower than letters (default space=4, sans space=6).
_BG_SPACE_EM = 5.0 / _GLTEXT_GLYPH_HEIGHT
_BG_CHAR_EM = 9.0 / _GLTEXT_GLYPH_HEIGHT
_BG_PAD_EM = 0.25

# Unit-cube corner: max_x, min_y, max_z (front-right-top in FLU when yaw ≈ 0).
_LABEL_UNIT_CORNER = np.array([0.5, -0.5, 0.5, 1.0], dtype=np.float64)


def _estimate_label_text_width_em(lines: Sequence[str]) -> float:
    """Approx gltext advance width in ems (relative to line height)."""
    widest = 0.0
    for line in lines:
        width = 0.0
        for ch in line:
            width += _BG_SPACE_EM if ch == " " else _BG_CHAR_EM
        if width > widest:
            widest = width
    return widest


def _rot_x(rad: float) -> np.ndarray:
    c, s = math.cos(rad), math.sin(rad)
    return np.array(
        [[1.0, 0.0, 0.0, 0.0], [0.0, c, -s, 0.0], [0.0, s, c, 0.0], [0.0, 0.0, 0.0, 1.0]],
        dtype=np.float64,
    )


def _rot_y(rad: float) -> np.ndarray:
    c, s = math.cos(rad), math.sin(rad)
    return np.array(
        [[c, 0.0, s, 0.0], [0.0, 1.0, 0.0, 0.0], [-s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]],
        dtype=np.float64,
    )


def _rot_z(rad: float) -> np.ndarray:
    c, s = math.cos(rad), math.sin(rad)
    return np.array(
        [[c, -s, 0.0, 0.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        dtype=np.float64,
    )


def _translate(x: float, y: float, z: float) -> np.ndarray:
    out = np.eye(4, dtype=np.float64)
    out[0, 3] = x
    out[1, 3] = y
    out[2, 3] = z
    return out


def _as_mat4(mat: Any) -> np.ndarray:
    arr = np.asarray(mat, dtype=np.float64).reshape(4, 4)
    return np.ascontiguousarray(arr)


def _camera_target_mat4(cam: Any) -> np.ndarray:
    """Camera target as a row-major 4x4 for ``proj @ view @ target @ p``.

    Ouster ``mat4d`` / ``set_target`` are column-major. ``get_target`` is often
    exposed as a C-order (4, 4) view of that storage — transpose when the
    translation lives in the last row (column-major layout).
    """
    arr = _as_mat4(cam.get_target())
    row_t = abs(float(arr[3, 0])) + abs(float(arr[3, 1])) + abs(float(arr[3, 2]))
    col_t = abs(float(arr[0, 3])) + abs(float(arr[1, 3])) + abs(float(arr[2, 3]))
    if row_t > col_t * 1.05:
        arr = np.ascontiguousarray(arr.T)
    return arr


def _camera_proj_view_target(
    point_viz: PointViz,
) -> Tuple[np.ndarray, np.ndarray, np.ndarray, float, int, int]:
    """Rebuild PointViz Camera::matrices (row-major) for world→NDC projection."""
    cam = point_viz.camera
    vw = max(1, int(point_viz.viewport_width))
    vh = max(1, int(point_viz.viewport_height))
    aspect = vw / float(vh)

    # get_yaw / get_pitch return degrees; get_roll returns decidegrees (SDK quirk).
    yaw_rad = float(cam.get_yaw()) * math.pi / 180.0
    pitch_rad = float(cam.get_pitch()) * math.pi / 180.0
    roll_rad = float(cam.get_roll()) * math.pi / 1800.0
    log_distance = float(cam.get_dolly())
    view_distance = _CAMERA_LOG_DISTANCE_0 * math.exp(log_distance * 0.01)
    fov_deg = float(cam.get_fov())
    orthographic = bool(cam.is_orthographic())
    offset = np.asarray(cam.get_view_offset(), dtype=np.float64).reshape(3)
    proj_off = np.asarray(cam.get_proj_offset(), dtype=np.float64).reshape(2)
    target = _camera_target_mat4(cam)

    view = (
        _translate(0.0, 0.0, -view_distance)
        @ _rot_x(pitch_rad)
        @ _rot_y(roll_rad)
        @ _rot_z(yaw_rad)
        @ _translate(float(offset[0]), float(offset[1]), float(offset[2]))
    )

    # Diagonal FoV; get_fov returns degrees (C++ stores decidegrees).
    scale = math.tan(math.pi / 360.0 * fov_deg)
    aspect_angle = math.atan(aspect)
    aspect_w = math.sin(aspect_angle)
    aspect_h = math.cos(aspect_angle)
    # Match Camera::matrices near/far (far depends on view distance).
    far_dist = min(_CAMERA_FAR, 100.0 * view_distance)
    near_dist = _CAMERA_NEAR
    proj = np.zeros((4, 4), dtype=np.float64)
    if orthographic:
        proj[0, 0] = 1.0 / (aspect_w * scale * view_distance)
        proj[0, 3] = -float(proj_off[0])
        proj[1, 1] = 1.0 / (aspect_h * scale * view_distance)
        proj[1, 3] = -float(proj_off[1])
        proj[2, 2] = -2.0 / (far_dist - near_dist)
        proj[2, 3] = -(far_dist + near_dist) / (far_dist - near_dist)
        proj[3, 3] = 1.0
    else:
        proj[0, 0] = 1.0 / (aspect_w * scale)
        proj[0, 2] = float(proj_off[0])
        proj[1, 1] = 1.0 / (aspect_h * scale)
        proj[1, 2] = float(proj_off[1])
        proj[2, 2] = -(far_dist + near_dist) / (far_dist - near_dist)
        proj[2, 3] = -2.0 * far_dist * near_dist / (far_dist - near_dist)
        proj[3, 2] = -1.0

    return proj, view, target, aspect, vw, vh


# Shared camera matrices for one reproject pass (orbit/draw).
_CamState = Tuple[np.ndarray, np.ndarray, np.ndarray, float, int, int]


def _project_world_to_label_xy(
    point_viz: PointViz,
    world_xyz: Sequence[float],
    cam_state: Optional[_CamState] = None,
) -> Optional[Tuple[float, float, float, float, int, int]]:
    """Project world XYZ to Label 2D coords.

    Label 2D uses ``x∈[0,1]`` left→right and ``y∈[0,1]`` **top→bottom**
    (OSD sits near ``y=1``). OpenGL Image NDC is the opposite in Y.
    """
    if cam_state is None:
        cam_state = _camera_proj_view_target(point_viz)
    proj, view, target, aspect, vw, vh = cam_state
    p = np.array(
        [float(world_xyz[0]), float(world_xyz[1]), float(world_xyz[2]), 1.0],
        dtype=np.float64,
    )
    clip = proj @ view @ target @ p
    if abs(float(clip[3])) < 1e-8:
        return None
    ndc = clip[:3] / clip[3]
    # Behind camera or far outside the frustum.
    if float(ndc[2]) < -1.0 or float(ndc[2]) > 1.0:
        return None
    if abs(float(ndc[0])) > 1.35 or abs(float(ndc[1])) > 1.35:
        return None
    lx = (float(ndc[0]) + 1.0) * 0.5
    # Label y=0 is top of the viewport; NDC y=+1 is top.
    ly = (1.0 - float(ndc[1])) * 0.5
    return lx, ly, aspect, float(ndc[1]), vw, vh


def _matched_2d_label_scale(
    point_viz: PointViz,
    world_xyz: Sequence[float],
    scale_3d: float,
    cam_state: Optional[_CamState] = None,
) -> float:
    """2D Label scale whose on-screen size matches a 3D Label at ``scale_3d``."""
    if cam_state is None:
        cam_state = _camera_proj_view_target(point_viz)
    proj, view, target, _aspect, _vw, vh = cam_state
    p0 = np.array(
        [float(world_xyz[0]), float(world_xyz[1]), float(world_xyz[2]), 1.0],
        dtype=np.float64,
    )
    vt = view @ target
    up = vt[1, :3].copy()
    nrm = float(np.linalg.norm(up))
    if nrm < 1e-9:
        return float(np.clip(scale_3d * 0.35, 0.45, 2.0))
    up /= nrm
    world_h = _LABEL_WORLD_PER_FONT_UNIT * float(scale_3d) * _GLTEXT_GLYPH_HEIGHT
    p1 = np.array(
        [p0[0] + up[0] * world_h, p0[1] + up[1] * world_h, p0[2] + up[2] * world_h, 1.0],
        dtype=np.float64,
    )
    c0 = proj @ view @ target @ p0
    c1 = proj @ view @ target @ p1
    if abs(float(c0[3])) < 1e-8 or abs(float(c1[3])) < 1e-8:
        return float(np.clip(scale_3d * 0.35, 0.45, 2.0))
    n0 = c0[:3] / c0[3]
    n1 = c1[:3] / c1[3]
    # |Δndc_y| * vh/2 = pixel height of one 3D line.
    pixel_h = abs(float(n1[1] - n0[1])) * 0.5 * float(vh)
    # 2D draw uses scale * ui_scale; text_height includes ui_scale. Approximate
    # ui_scale=1 here; HiDPI still looks close because both text and panel use
    # the same resulting text_height for sizing.
    scale_2d = pixel_h / _GLTEXT_GLYPH_HEIGHT
    return float(np.clip(scale_2d, 0.4, 2.75))


def _label_xy_to_image_rect(
    lx: float,
    ly: float,
    width_px: float,
    height_px: float,
    *,
    aspect: float,
    vw: int,
    vh: int,
) -> Tuple[float, float, float, float]:
    """Image ``(x0,x1,y0,y1)`` for a bottom-left Label anchor at ``(lx, ly)``.

    Label Y grows downward; Image Y is OpenGL NDC (up is +).
    """
    w_frac = width_px / float(vw)
    h_frac = height_px / float(vh)
    # Bottom-aligned text occupies [ly - h, ly] in label space (upward = smaller y).
    x0_l = lx
    x1_l = lx + w_frac
    y_bottom_l = ly
    y_top_l = ly - h_frac
    x0 = (2.0 * x0_l - 1.0) * aspect
    x1 = (2.0 * x1_l - 1.0) * aspect
    y0 = 1.0 - 2.0 * y_bottom_l
    y1 = 1.0 - 2.0 * y_top_l
    return x0, x1, y0, y1


def _bucket_label_scale(scale_2d: float) -> float:
    """Quantize on-screen scale so panel textures are reused while dragging."""
    step = _PANEL_SCALE_BUCKET
    return float(max(step, round(float(scale_2d) / step) * step))


def _label_font(px: int) -> tuple[Any, bool]:
    """PIL default bitmap font.

    Returns ``(font, honors_size)``. Newer Pillow accepts ``load_default(size=)``;
    older builds ignore size — callers should upscale the raster instead.
    """
    size = max(8, int(px))
    cached = _label_font_cache.get(size)
    if cached is not None:
        return cached
    font = None
    honors_size = False
    if _PilImageFont is not None:
        try:
            font = _PilImageFont.load_default(size=size)
            honors_size = True
        except TypeError:
            try:
                font = _PilImageFont.load_default()
            except Exception:  # noqa: BLE001
                font = None
        except Exception:  # noqa: BLE001
            font = None
    result = (font, honors_size)
    _label_font_cache[size] = result
    return result


def _render_label_panel_rgba(
    lines: Sequence[str],
    *,
    scale_2d: float,
    text_rgba: Sequence[float],
    bg_rgba: Sequence[float],
) -> Tuple[np.ndarray, int, int]:
    """Rasterize bg + text into a float32 RGBA image (H, W, 4).

    Drawn as a PointViz Image so it shares the panorama underlay pass (Images
    draw before Labels).
    """
    if _PilImage is None or _PilImageDraw is None:
        # Solid fallback if Pillow is unavailable.
        h = max(
            2,
            int(
                round(
                    scale_2d
                    * _GLTEXT_GLYPH_HEIGHT
                    * _PIL_FONT_SIZE_SCALE
                    * max(1, len(lines))
                )
            ),
        )
        w = max(
            2,
            int(
                round(
                    _estimate_label_text_width_em(lines)
                    * (h / max(1, len(lines)))
                )
            ),
        )
        panel = np.zeros((h, w, 4), dtype=np.float32)
        panel[:] = tuple(float(c) for c in bg_rgba)
        return panel, w, h

    base_px = max(8, int(round(scale_2d * _GLTEXT_GLYPH_HEIGHT)))
    target_px = max(8, int(round(base_px * _PIL_FONT_SIZE_SCALE)))
    font, honors_size = _label_font(target_px)
    # Layout at the size the font actually draws; upscale later if needed.
    layout_px = target_px if honors_size else base_px
    pad = max(2, int(round(_BG_PAD_EM * layout_px)))
    line_gap = max(1, layout_px // 8)
    text_lines = [str(line) for line in lines] or [""]

    scratch = _PilImage.new("RGBA", (8, 8), (0, 0, 0, 0))
    draw = _PilImageDraw.Draw(scratch)
    widths: list[int] = []
    heights: list[int] = []
    for line in text_lines:
        if hasattr(draw, "textbbox"):
            bbox = draw.textbbox((0, 0), line, font=font)
            widths.append(max(1, int(bbox[2] - bbox[0])))
            heights.append(max(1, int(bbox[3] - bbox[1])))
        else:
            tw, th = draw.textsize(line, font=font)  # type: ignore[attr-defined]
            widths.append(max(1, int(tw)))
            heights.append(max(1, int(th)))
    text_w = max(widths) if widths else layout_px
    text_h = sum(heights) + line_gap * (len(text_lines) - 1)
    img_w = max(2, text_w + 2 * pad)
    img_h = max(2, text_h + 2 * pad)

    bg = tuple(int(round(float(c) * 255.0)) for c in bg_rgba)
    fg = tuple(int(round(float(c) * 255.0)) for c in text_rgba)
    img = _PilImage.new("RGBA", (img_w, img_h), bg)
    draw = _PilImageDraw.Draw(img)
    y = pad
    for line, lh in zip(text_lines, heights):
        draw.text((pad, y), line, fill=fg, font=font)
        y += lh + line_gap

    # Fixed-size load_default(): scale the whole panel by _PIL_FONT_SIZE_SCALE.
    if not honors_size and _PIL_FONT_SIZE_SCALE != 1.0:
        new_w = max(2, int(round(img_w * _PIL_FONT_SIZE_SCALE)))
        new_h = max(2, int(round(img_h * _PIL_FONT_SIZE_SCALE)))
        resample = getattr(_PilImage, "NEAREST", 0)
        img = img.resize((new_w, new_h), resample)
        img_w, img_h = new_w, new_h

    rgba = np.asarray(img, dtype=np.float32) / 255.0
    return np.ascontiguousarray(rgba), img_w, img_h


def _call_update_objects_for_list(orig, key, object_list, frame=None):
    """Call SDK update_objects_for_list (2- or 3-arg variants)."""
    try:
        return orig(key, object_list, frame)
    except TypeError:
        return orig(key, object_list)


def patch_overlay_cuboid_copy(model) -> None:
    """Avoid SDK copy.copy(cuboid) on 2D overlay — duplicates select_id and SIGSEGVs."""
    from ouster.sdk._bindings.viz import Cuboid
    from ouster.sdk.viz import model as viz_model
    from ouster.sdk.viz.model import ObjectViewMode

    # SDK overlay cuboid helpers: `_overlay_cuboid_transform` (+ set_transform)
    # or newer `_object_for_overlay_cuboid` (+ from_object).
    object_for_overlay = getattr(viz_model, "_object_for_overlay_cuboid", None)
    overlay_transform = getattr(viz_model, "_overlay_cuboid_transform", None)
    if object_for_overlay is None and overlay_transform is None:
        raise ImportError(
            "ouster.sdk.viz.model missing overlay cuboid helper "
            "(_object_for_overlay_cuboid or _overlay_cuboid_transform)"
        )

    def update_object_cuboids(self, frames=None):
        if self._object_view_mode in (
            ObjectViewMode.OVERLAY_AND_MAIN_VIEW,
            ObjectViewMode.OVERLAY_ONLY,
        ):
            overlay_rgba = getattr(self, "_overlay_cuboid_rgba", {})
            frame_set = frames if frames is not None else getattr(self, "_frame_set", None)
            for sensor_idx, sensor in enumerate(self._sensors):
                frame = None
                if frame_set is not None and sensor_idx < len(frame_set):
                    frame = frame_set[sensor_idx]
                overlay_cuboids = []
                if frame is not None:
                    for obj, _cuboid in self._objects.items():
                        rgba = overlay_rgba.get(obj, (1.0, 1.0, 1.0, 0.3))
                        if object_for_overlay is not None:
                            overlay_obj = object_for_overlay(obj, frame)
                            overlay_cuboids.append(
                                Cuboid.from_object(overlay_obj, rgba)
                            )
                        else:
                            cuboid = Cuboid.from_object(obj, rgba)
                            cuboid.set_transform(overlay_transform(obj, frame))
                            overlay_cuboids.append(cuboid)
                sensor._object_overlay.set_cuboids(overlay_cuboids)
        else:
            for sensor in self._sensors:
                sensor._object_overlay.set_cuboids([])

    model.update_object_cuboids = types.MethodType(update_object_cuboids, model)


def convert_color_rgb_to_rgba(
    color: tuple[int, int, int], opacity: float = 1.0,
) -> tuple[float, float, float, float]:
    """Convert a tuple of ints [0-255] to floats [0-1] per channel."""
    r, g, b = [c / 255.0 for c in color]
    return (r, g, b, opacity)


def lerp_rgb(
    rgb_a: tuple[int, int, int],
    rgb_b: tuple[int, int, int],
    t: float,
) -> tuple[int, int, int]:
    """Linear blend between two 8-bit RGB colors; ``t`` clamped to [0, 1]."""
    t = float(np.clip(t, 0.0, 1.0))
    return tuple(int(round(a + t * (b - a))) for a, b in zip(rgb_a, rgb_b))


def _clone_pose(pose: Pose) -> Pose:
    out = Pose()
    out.position = np.asarray(pose.position, dtype=np.float64)
    out.set_rotation(np.asarray(pose.euler_angles(), dtype=np.float64))
    return out


def _label_object_to_body(obj, list_key=None, ego_center_z=None) -> Pose:
    """Pose used for label corner math (ego may be offset from body origin)."""
    if list_key != "ego_obj" or ego_center_z is None:
        return obj.object_to_body
    # Ego cuboid center sits below the lidar/body origin; match that offset so
    # the label tracks the visible ego box, not the body-frame origin.
    o2b = _clone_pose(obj.object_to_body)
    o2b.position = np.array([0.0, 0.0, float(ego_center_z)], dtype=np.float64)
    return o2b


def object_label_position(
    obj,
    z_offset: float = 0.0,
    unit_corner=_LABEL_UNIT_CORNER,
    *,
    object_to_body: Optional[Pose] = None,
) -> np.ndarray:
    """World anchor: label bottom-left pinned to cuboid front-right-top corner."""
    pos = _object_cuboid_corner(
        obj, unit_corner=unit_corner, object_to_body=object_to_body
    )
    if z_offset:
        pos = pos.copy()
        pos[2] += z_offset
    return pos


def _label_anchor_position(
    obj,
    *,
    list_key=None,
    ego_center_z=None,
    z_offset: float = 0.0,
    unit_corner=_LABEL_UNIT_CORNER,
) -> tuple[float, float, float]:
    """World anchor for a label (cuboid front-right-top + optional z offset)."""
    pos = object_label_position(
        obj,
        z_offset,
        unit_corner=unit_corner,
        object_to_body=_label_object_to_body(obj, list_key, ego_center_z),
    )
    return (float(pos[0]), float(pos[1]), float(pos[2]))


def _object_cuboid_corner(
    obj,
    unit_corner=_LABEL_UNIT_CORNER,
    *,
    object_to_body: Optional[Pose] = None,
) -> np.ndarray:
    """World XYZ at a unit-cube corner, matching Cuboid.from_object geometry."""
    o2b = obj.object_to_body if object_to_body is None else object_to_body
    length, width, height = (abs(float(v)) for v in obj.dimensions)
    pose = (obj.body_to_world * o2b).to_matrix()
    transform = pose @ np.diag([length, width, height, 1.0])
    return (transform @ unit_corner)[:3]


class BillboardLabel:
    """Screen-space Image panel with baked text (under the 2D panorama)."""

    def __init__(
        self,
        point_viz: PointViz,
        lines: Sequence[str],
        anchor: tuple[float, float, float],
        *,
        scale: float = _DEFAULT_LABEL_SCALE,
        text_rgba: tuple[float, float, float, float] = (1.0, 1.0, 1.0, 1.0),
        bg_rgba: tuple[float, float, float, float] = _DEFAULT_BILLBOARD_BG_RGBA,
    ):
        self._viz = point_viz
        self._scale = float(scale)
        self._text_rgba = tuple(float(v) for v in text_rgba)
        self._bg_rgba = tuple(float(v) for v in bg_rgba)
        self._bg_image = ToggleImage(point_viz, initially_visible=False)
        self._world_anchor = tuple(float(v) for v in anchor)
        self._lines: list[str] = list(lines)
        self._panel_cache_key: Optional[tuple] = None
        self._panel_size: tuple[float, float] = (2.0, 2.0)
        self._panel_scale_bucket: float = _PANEL_SCALE_BUCKET
        self._scale_2d: Optional[float] = None
        self.reproject()

    def hide(self) -> None:
        self._bg_image.hide()

    def show(self) -> None:
        self._bg_image.show()

    def set_scale(self, scale: float) -> None:
        self._scale = float(scale)

    def reproject(
        self,
        *,
        cam_state: Optional[_CamState] = None,
        refresh_scale: bool = True,
    ) -> None:
        """Refresh screen-space panel from the stored world anchor.

        ``refresh_scale=False`` (orbit drag): reuse last 2D scale — avoids
        per-label camera scale matching on every mouse event. Draw sync still
        refreshes scale so size tracks camera distance.
        """
        projected = _project_world_to_label_xy(
            self._viz, self._world_anchor, cam_state=cam_state
        )
        if projected is None:
            self.hide()
            return
        lx, ly, aspect, _ndc_y, vw, vh = projected
        if refresh_scale or self._scale_2d is None:
            self._scale_2d = _matched_2d_label_scale(
                self._viz,
                self._world_anchor,
                self._scale,
                cam_state=cam_state,
            )
        scale_2d = float(self._scale_2d)
        # Rasterize at a coarse scale bucket; stretch the Image rect to the
        # true scale so orbit only dirties positions (not GPU textures).
        scale_bucket = _bucket_label_scale(scale_2d)
        cache_key = (
            tuple(self._lines),
            scale_bucket,
            self._text_rgba,
            self._bg_rgba,
        )
        if cache_key != self._panel_cache_key:
            panel, width_px, height_px = _render_label_panel_rgba(
                self._lines,
                scale_2d=scale_bucket,
                text_rgba=self._text_rgba,
                bg_rgba=self._bg_rgba,
            )
            self._bg_image.set_image(panel)
            self._panel_size = (float(width_px), float(height_px))
            self._panel_scale_bucket = scale_bucket
            self._panel_cache_key = cache_key
        else:
            width_px, height_px = self._panel_size

        stretch = scale_2d / float(self._panel_scale_bucket or scale_bucket or 1.0)
        self._bg_image.position = _label_xy_to_image_rect(
            lx,
            ly,
            width_px * stretch,
            height_px * stretch,
            aspect=aspect,
            vw=vw,
            vh=vh,
        )
        self._bg_image.show()

    def sync(
        self,
        lines: Sequence[str],
        anchor: tuple[float, float, float],
        *,
        cam_state: Optional[_CamState] = None,
    ) -> None:
        """Update label text/position; keep dark background behind the text."""
        self._lines = list(lines)
        self._world_anchor = tuple(float(v) for v in anchor)
        self.reproject(cam_state=cam_state, refresh_scale=True)


def _object_label_lines(obj, class_map=None):
    class_name = ID_TO_CLASS.get(obj.class_id, "unknown")
    if class_map is not None:
        class_name = class_map.get(obj.class_id, class_name)
    speed_mph = float(np.linalg.norm(obj.velocity)) * 2.23694
    speed_hdr = "vel:"
    speed_text = f"{speed_mph:.1f} mph"
    if class_name == "ego":
        return [class_name, speed_text]
    ljust = 6
    rjust = 8
    return [
        f"{class_name:<{ljust}} {obj.id:>{rjust}}",
        f"{speed_hdr:<{ljust}}{speed_text:>{rjust}}",
    ]


def enable_object_labels(
    scan_viz,
    z_offset=0.0,
    object_list_keys=None,
    *,
    label_scale: float = _DEFAULT_LABEL_SCALE,
    ego_center_offset_z: float | None = None,
):
    """Draw a billboard label on each detection cuboid.

    SimpleViz only creates labels in _object_labels when a cuboid is picked, and
    right-click clears that dict. Keep ours in a separate map and sync on draw.
    Labels are screen-space Images (text baked in) so they draw under the 2D
    panorama.

    object_list_keys: if set, only label objects from these scan.objects keys
        (e.g. ["trk_objs", "ego_obj"]).
    """
    lsv = scan_viz
    model = lsv._model
    model._auto_object_labels = {}
    model._object_list_keys = {}  # Object -> scan.objects key
    model._label_z_offset = z_offset
    model._label_scale = float(label_scale)
    model._label_object_list_keys = object_list_keys
    model._ego_center_offset_z = ego_center_offset_z
    model._object_labels_enabled = True
    orig_draw = lsv.draw
    orig_update_objects = model.update_objects
    orig_update_objects_for_list = model.update_objects_for_list

    def update_objects(self, scans):
        self._object_list_keys.clear()
        if hasattr(self, "_overlay_cuboid_rgba"):
            self._overlay_cuboid_rgba.clear()
        orig_update_objects(scans)

    def update_objects_for_list(self, key, object_list, frame=None):
        _call_update_objects_for_list(orig_update_objects_for_list, key, object_list, frame)
        for obj in object_list:
            self._object_list_keys[obj] = key

    model.update_objects = types.MethodType(update_objects, model)
    model.update_objects_for_list = types.MethodType(update_objects_for_list, model)

    def _sync_object_labels():
        if not model._object_labels_enabled:
            for label in model._auto_object_labels.values():
                label.hide()
            return

        live_keys = set(model._objects)
        for label_key, label in list(model._auto_object_labels.items()):
            if label_key not in live_keys:
                label.hide()
                del model._auto_object_labels[label_key]

        # One camera matrix rebuild per draw — not per label.
        cam_state = _camera_proj_view_target(model._viz)

        for obj in model._objects:
            list_key = model._object_list_keys.get(obj)
            if (
                model._label_object_list_keys is not None
                and list_key not in model._label_object_list_keys
            ):
                continue
            class_map = model._class_maps.get(list_key) if list_key else None
            # Live object pose (same math as Cuboid). Screen-space Images also
            # reproject after FOLLOW updates in ``_draw_update_camera_pose``.
            pos = _label_anchor_position(
                obj,
                list_key=list_key,
                ego_center_z=model._ego_center_offset_z,
                z_offset=model._label_z_offset,
            )
            lines = _object_label_lines(obj, class_map)
            label = model._auto_object_labels.get(obj)

            if label is None:
                label = BillboardLabel(model._viz, lines, pos, scale=model._label_scale)
                model._auto_object_labels[obj] = label
            else:
                label.set_scale(model._label_scale)
            label.sync(lines, pos, cam_state=cam_state)

    def _sync_labels():
        _sync_object_labels()

    def draw_with_labels(self, update=True, update_camera=True):
        with lsv._lock:
            orig_draw(update=False, update_camera=update_camera)
            _sync_labels()
            if update:
                model._viz.update()

    lsv.draw = types.MethodType(draw_with_labels, lsv)

    def handle_label_toggle(_ctx, key: int, mods: int) -> bool:
        if mods != 0 or key != ord("L"):
            return True
        model._object_labels_enabled = not model._object_labels_enabled
        state = "on" if model._object_labels_enabled else "off"
        model._viz.set_notification(f"Object labels: {state}")
        with lsv._lock:
            _sync_labels()
            model._viz.update()
        return False

    model._viz.push_key_handler(handle_label_toggle)

    # Screen-space Images bake NDC; reproject after orbit and after FOLLOW
    # camera updates. Intercept orbit (apply deltas, skip default) then
    # PointViz.update under lsv._lock — never VizProxy.update (KeyBridge).
    _reproject_guard = {"busy": False}
    _orbit_capture = {"t": 0.0}

    def _point_viz_real() -> Any:
        return getattr(model._viz, "_real", model._viz)

    def _note_capture_dirty(*, force: bool = False) -> None:
        now = time.monotonic()
        if (
            not force
            and (now - float(_orbit_capture["t"])) < _ORBIT_CAPTURE_MIN_DT_S
        ):
            return
        _orbit_capture["t"] = now
        pub = getattr(model._viz, "_publisher", None)
        note = getattr(pub, "note_scene_update", None)
        if callable(note):
            note()

    def _cameras() -> list[Any]:
        cams: list[Any] = []
        seen: set[int] = set()
        for name in ("camera", "current_camera"):
            attr = getattr(model._viz, name, None)
            if attr is None:
                continue
            cam = attr() if callable(attr) else attr
            if cam is None:
                continue
            cam_id = id(cam)
            if cam_id in seen:
                continue
            seen.add(cam_id)
            cams.append(cam)
        return cams

    def _reproject_all_screen_labels(*, refresh_scale: bool) -> None:
        cam_state = _camera_proj_view_target(_point_viz_real())
        for label in list(model._auto_object_labels.values()):
            reproject = getattr(label, "reproject", None)
            if callable(reproject):
                reproject(cam_state=cam_state, refresh_scale=refresh_scale)

    # Live play draws with update_camera=False, then FOLLOW set_target in
    # _draw_update_camera_pose before viz.update — reproject here or labels
    # drift until pause+orbit.
    orig_cam_pose = getattr(lsv, "_draw_update_camera_pose", None)
    if callable(orig_cam_pose):

        def _draw_update_camera_pose_with_labels(self, *args, **kwargs):
            orig_cam_pose(*args, **kwargs)
            if not model._object_labels_enabled or not model._auto_object_labels:
                return
            try:
                _reproject_all_screen_labels(refresh_scale=True)
            except Exception as exc:  # noqa: BLE001
                print(
                    "hello-ouster-dynamo: label reproject after camera "
                    f"pose failed: {exc!r}",
                    flush=True,
                )

        lsv._draw_update_camera_pose = types.MethodType(
            _draw_update_camera_pose_with_labels, lsv
        )
    else:
        print(
            "hello-ouster-dynamo: no _draw_update_camera_pose on frame viz; "
            "screen labels may drift while FOLLOW camera moves",
            flush=True,
        )

    def _on_camera_moved(*, refresh_scale: bool = False) -> None:
        """Reproject + swap label buffers. Caller must hold ``lsv._lock``."""
        if _reproject_guard["busy"]:
            return
        if not model._object_labels_enabled:
            return
        if not model._auto_object_labels:
            return
        _reproject_guard["busy"] = True
        try:
            _reproject_all_screen_labels(refresh_scale=refresh_scale)
            _point_viz_real().update()
            _note_capture_dirty()
        except Exception as exc:  # noqa: BLE001
            print(
                f"hello-ouster-dynamo: screen-label reproject failed: {exc!r}",
                flush=True,
            )
        finally:
            _reproject_guard["busy"] = False

    def _orbit_reproject_mouse(ctx, x: float, y: float) -> bool:
        dx = float(x) - float(getattr(ctx, "mouse_x", x))
        dy = float(y) - float(getattr(ctx, "mouse_y", y))
        moved = False
        with lsv._lock:
            cams = _cameras()
            if bool(getattr(ctx, "lbutton_down", False)):
                sens = 0.3
                for cam in cams:
                    cam.yaw(sens * dx)
                    cam.pitch(sens * dy)
                moved = True
            elif bool(getattr(ctx, "mbutton_down", False)):
                ww = float(getattr(ctx, "window_width", 0) or 0) or float(
                    model._viz.viewport_width
                )
                wh = float(getattr(ctx, "window_height", 0) or 0) or float(
                    model._viz.viewport_height
                )
                diag = math.sqrt(ww * ww + wh * wh) or 1.0
                dx *= 2.0 / diag
                dy *= 2.0 / diag
                for cam in cams:
                    cam.dolly_xy(dx, dy)
                moved = True
            if not moved:
                return True
            _on_camera_moved(refresh_scale=False)
        return False

    def _orbit_reproject_scroll(_ctx, _x: float, yoff: float) -> bool:
        with lsv._lock:
            for cam in _cameras():
                cam.dolly(float(yoff) * 5.0)
            _on_camera_moved(refresh_scale=True)
        return False

    model._viz.push_mouse_pos_handler(_orbit_reproject_mouse)
    model._viz.push_scroll_handler(_orbit_reproject_scroll)
    print(
        "hello-ouster-dynamo: screen-space labels use orbit-intercept + "
        "post-FOLLOW reproject",
        flush=True,
    )


def enable_osd_text_scale(frame_viz, multiplier: float) -> None:
    """Scale SimpleViz HUD text (top-left sensor/cloud/time block)."""
    orig = frame_viz._update_multi_viz_osd
    osd_none = type(frame_viz).OsdState.NONE

    def _update_multi_viz_osd(self):
        orig()
        if self._osd_state == osd_none:
            return
        text_scale = (self._osd_scale / 2 + 1) * multiplier
        self._osd.set_scale(text_scale)
        for sensor in self._model._sensors:
            sensor._sensor_label.scale = text_scale

    frame_viz._update_multi_viz_osd = types.MethodType(_update_multi_viz_osd, frame_viz)


def _set_label_font(viz, name: str) -> None:
    set_font = getattr(viz, "set_label_font", None)
    if not callable(set_font):
        return
    try:
        set_font(name)
    except (TypeError, ValueError):
        return


def enable_sans_serif_font(simple_viz):
    """Use sans-serif labels before the render loop starts (no-op if unsupported)."""
    pending = [True]
    orig_run = simple_viz.run

    def run(self, frames):
        if pending[0]:
            pending[0] = False
            _set_label_font(self._viz, "sans")
        return orig_run(frames)

    simple_viz.run = types.MethodType(run, simple_viz)


def enable_mono_font(simple_viz):
    """Force monospace labels before the render loop starts (no-op if unsupported)."""
    pending = [True]
    orig_run = simple_viz.run

    def run(self, frames):
        if pending[0]:
            pending[0] = False
            _set_label_font(self._viz, "monospace")
        return orig_run(frames)

    simple_viz.run = types.MethodType(run, simple_viz)


def enable_cuboid_color_modes(
    scan_viz,
    point_viz,
    *,
    initial_mode: str = "class",
    alpha: float = 0.35,
    speed_min_mps: float = BOX_COLOR_SPEED_MIN_MPS,
    speed_max_mps: float = BOX_COLOR_SPEED_MAX_MPS,
    color_min_rgb: tuple[int, int, int] = BOX_COLOR_SLOW_RGB,
    color_mid_rgb: tuple[int, int, int] = BOX_COLOR_MED_RGB,
    color_max_rgb: tuple[int, int, int] = BOX_COLOR_FAST_RGB,
    on_color_mode_changed=None,
):
    """Cycle cuboid colors with ``V``: object id → class → velocity."""
    if initial_mode not in _BOX_COLOR_MODES:
        initial_mode = "class"
    state = {"mode": initial_mode}
    speed_span = speed_max_mps - speed_min_mps
    ego_class_id = CLASS_IDS["ego"]
    ego_rgb = (255, 255, 255)
    # GLFW key codes match uppercase ASCII for letter keys (GLFW_KEY_V == ord("V")).
    _COLOR_MODE_KEY = ord("V")

    model = scan_viz._model
    orig_update_objects_for_list = model.update_objects_for_list

    def _is_ego(obj) -> bool:
        return obj.class_id == ego_class_id

    def box_color_rgb(obj) -> tuple[int, int, int]:
        if _is_ego(obj):
            return ego_rgb
        mode = state["mode"]
        if mode == "id":
            idx = obj.id % len(model._flags_palette)
            pal = model._flags_palette[idx]
            return tuple(int(round(c * 255)) for c in pal[:3])
        if mode == "class":
            return CLASS_COLORS.get(obj.class_id, (200, 200, 200))
        speed = float(np.linalg.norm(np.asarray(obj.velocity, dtype=np.float64)))
        if speed == 0.0:
            return ego_rgb
        if speed_span <= 0.0:
            t = 1.0 if speed >= speed_max_mps else 0.0
        else:
            t = (speed - speed_min_mps) / speed_span
        if t < 0.5:
            return lerp_rgb(color_min_rgb, color_mid_rgb, t * 2.0)
        return lerp_rgb(color_mid_rgb, color_max_rgb, (t - 0.5) * 2.0)

    def box_color_rgba(obj) -> tuple[float, float, float, float]:
        cuboid_alpha = 0.3 if state["mode"] == "id" and not _is_ego(obj) else alpha
        return convert_color_rgb_to_rgba(box_color_rgb(obj), cuboid_alpha)

    def update_objects_for_list(self, key, object_list, frame=None):
        _call_update_objects_for_list(orig_update_objects_for_list, key, object_list, frame)
        if not hasattr(model, "_overlay_cuboid_rgba"):
            model._overlay_cuboid_rgba = {}
        for obj in object_list:
            cuboid = self._objects.get(obj)
            if cuboid is None:
                continue
            rgba = box_color_rgba(obj)
            model._overlay_cuboid_rgba[obj] = rgba
            if _is_ego(obj) or state["mode"] != "id":
                cuboid.set_rgba(rgba)

    model.update_objects_for_list = types.MethodType(update_objects_for_list, model)

    def apply_mode_to_cuboids() -> None:
        if not hasattr(model, "_overlay_cuboid_rgba"):
            model._overlay_cuboid_rgba = {}
        for obj, cuboid in model._objects.items():
            rgba = box_color_rgba(obj)
            model._overlay_cuboid_rgba[obj] = rgba
            cuboid.set_rgba(rgba)

    def handle_color_mode(_ctx, key: int, mods: int) -> bool:
        if mods != 0 or key != _COLOR_MODE_KEY:
            return True
        idx = _BOX_COLOR_MODES.index(state["mode"])
        state["mode"] = _BOX_COLOR_MODES[(idx + 1) % len(_BOX_COLOR_MODES)]
        label = _BOX_COLOR_MODE_LABELS[state["mode"]]
        print(f"box_color={state['mode']} ({label})")
        point_viz.set_notification(f"box color: {label}")
        with scan_viz._lock:
            apply_mode_to_cuboids()
            if on_color_mode_changed is not None:
                on_color_mode_changed()
            point_viz.update()
        return False

    point_viz.push_key_handler(handle_color_mode)
    return box_color_rgb


def enable_app_source_overlays(
    simple: Any,
    *,
    config_path: Optional[Path] = None,
) -> None:
    """Load Dynamo viz config (if present) and apply peer-source overlays."""
    path = config_path or (
        Path(__file__).resolve().parent / "configs" / "viz_configs.json"
    )
    try:
        if path.is_file():
            load_viz_config(path)
        configure_dynamo_parity_viz(simple)
    except Exception as exc:  # noqa: BLE001
        print(
            f"hello-ouster-dynamo: peer overlay setup failed: {exc!r}",
            flush=True,
        )


def configure_dynamo_parity_viz(
    simple: Any, viz_cfg: Optional[dict] = None
) -> None:
    """Apply ouster-dynamo SimpleViz overlays (tracked objects, ego, colors).

    Enables labels (L), font toggle (SHIFT+T), and box colormap (V) so
    hello-ouster-dynamo matches Dynamo's run_session viz setup.
    """
    cfg = viz_cfg or VIZ_CONFIG
    frame_viz = getattr(simple, "_frame_viz", None) or getattr(simple, "_scan_viz", None)
    if frame_viz is None:
        print("hello-ouster-dynamo: dynamo parity: no frame_viz on SimpleViz", flush=True)
        return
    model = getattr(frame_viz, "_model", None)
    if model is not None:
        patch_overlay_cuboid_copy(model)
    if float(cfg.get("osd_scale", 1.0)) != 1.0:
        enable_osd_text_scale(frame_viz, float(cfg["osd_scale"]))
    key_defs = getattr(frame_viz, "_key_definitions", None)
    if isinstance(key_defs, dict):
        key_defs["L"] = "Toggle object labels"
        key_defs["SHIFT+T"] = "Toggle label font (mono / sans-serif)"
        key_defs["V"] = "Cycle box color (object id / class / velocity)"
    set_class_maps = getattr(simple, "set_class_maps", None)
    if callable(set_class_maps):
        set_class_maps(tracking_class_map_set())
    enable_object_labels(
        frame_viz,
        z_offset=float(cfg.get("label_z_offset", 0.0)),
        label_scale=float(cfg.get("label_scale", 2.25)),
        object_list_keys=[KEY_TRACKED, KEY_EGO],
        ego_center_offset_z=0.0,
    )
    enable_cuboid_color_modes(
        frame_viz,
        simple._viz,
        initial_mode=BOX_COLOR,
        speed_min_mps=BOX_COLOR_SPEED_MIN_MPS,
        speed_max_mps=BOX_COLOR_SPEED_MAX_MPS,
        color_min_rgb=BOX_COLOR_SLOW_RGB,
        color_max_rgb=BOX_COLOR_FAST_RGB,
    )
    if cfg.get("sans_font"):
        enable_sans_serif_font(simple)
    else:
        enable_mono_font(simple)
    print(
        "hello-ouster-dynamo: Dynamo SimpleViz parity enabled "
        "(L labels, SHIFT+T font, V box colormap; "
        "screen-space Image label panels)",
        flush=True,
    )

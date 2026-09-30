"""Load viz knobs from viz_configs.json."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, Mapping

_CONFIGS_DIR = Path(__file__).resolve().parent
DEFAULT_VIZ_CONFIG = _CONFIGS_DIR / "viz_configs.json"

_REQUIRED_KEYS = (
    "label_scale",
    "label_z_offset",
    "osd_scale",
    "sans_font",
    "show_trk_history_pts",
    "print_tracks",
    "print_timing",
)

# Mutated in place by load_viz_config so importers keep a live view.
VIZ_CONFIG: Dict[str, Any] = {}


def apply_viz_config(cfg: Dict[str, Any]) -> None:
    """Validate and install viz config into VIZ_CONFIG."""
    missing = [k for k in _REQUIRED_KEYS if k not in cfg]
    if missing:
        raise ValueError(
            f"Viz config missing required keys: {', '.join(missing)}"
        )

    parsed: Dict[str, Any] = {
        "label_scale": float(cfg["label_scale"]),
        "label_z_offset": float(cfg["label_z_offset"]),
        "osd_scale": float(cfg["osd_scale"]),
        "sans_font": bool(cfg["sans_font"]),
        "show_trk_history_pts": bool(cfg["show_trk_history_pts"]),
        "print_tracks": bool(cfg["print_tracks"]),
        "print_timing": bool(cfg["print_timing"]),
    }
    if parsed["label_scale"] <= 0:
        raise ValueError("label_scale must be > 0")

    VIZ_CONFIG.clear()
    VIZ_CONFIG.update(parsed)


def patch_viz_config(partial: Mapping[str, Any]) -> None:
    """Merge a partial controllable viz patch into VIZ_CONFIG (live sync)."""
    if not partial:
        return
    merged = dict(VIZ_CONFIG)
    for key in _REQUIRED_KEYS:
        if key in partial:
            merged[key] = partial[key]
    apply_viz_config(merged)


def load_viz_config(path: Path) -> Dict[str, Any]:
    with path.open() as f:
        cfg = json.load(f)
    if not isinstance(cfg, dict):
        raise ValueError("Viz config must be a JSON object")
    apply_viz_config(cfg)
    return dict(VIZ_CONFIG)


load_viz_config(DEFAULT_VIZ_CONFIG)

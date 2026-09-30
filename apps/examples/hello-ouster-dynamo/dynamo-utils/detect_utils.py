"""Class maps for Dynamo-parity SimpleViz (no ML engine in hello-ouster-dynamo)."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, Tuple

from ouster.sdk.core import ClassMap, ClassMapSet

_DEFAULT_DETECT = Path(__file__).resolve().parent / "configs" / "detect_configs.json"

CLASS_IDS: Dict[str, int] = {}
CLASS_COLORS: Dict[int, Tuple[int, int, int]] = {}
ID_TO_CLASS: Dict[int, str] = {}


def apply_class_config(cfg: Dict[str, Any]) -> None:
    class_ids = cfg["class_ids"]
    class_colors = cfg["class_colors"]
    parsed_ids = {str(k): int(v) for k, v in class_ids.items()}
    parsed_colors: Dict[int, Tuple[int, int, int]] = {}
    for name, rgb in class_colors.items():
        cid = parsed_ids[str(name)]
        r, g, b = (int(rgb[0]), int(rgb[1]), int(rgb[2]))
        parsed_colors[cid] = (r, g, b)
    CLASS_IDS.clear()
    CLASS_IDS.update(parsed_ids)
    CLASS_COLORS.clear()
    CLASS_COLORS.update(parsed_colors)
    ID_TO_CLASS.clear()
    ID_TO_CLASS.update({v: k for k, v in parsed_ids.items()})


def load_config(path: Path | None = None) -> Dict[str, Any]:
    p = path or _DEFAULT_DETECT
    with p.open() as f:
        cfg = json.load(f)
    apply_class_config(cfg)
    return cfg


def tracking_class_map_set() -> ClassMapSet:
    from ouster_dynamo_api import KEY_EGO, KEY_TRACKED

    if not CLASS_IDS:
        load_config()
    tracked = ClassMap({cid: name for name, cid in CLASS_IDS.items()})
    ego = ClassMap({CLASS_IDS["ego"]: "ego"})
    return ClassMapSet({KEY_TRACKED: tracked, KEY_EGO: ego})


load_config()

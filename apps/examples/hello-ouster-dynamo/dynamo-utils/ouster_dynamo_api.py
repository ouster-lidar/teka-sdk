"""Ouster Dynamo API: heartbeat & telemetry + FrameSet dynamo_objects (V1).

Consumer copy owned by hello-ouster-dynamo (this tree). Compatible with the
ouster-dynamo producer FrameSet / EventPublisher contract. hello-ouster-dynamo
builds without any producer app checkout.

V1 channels:
  - Heartbeat & telemetry (EventPublisher): ``ouster_dynamo_api_version``, ``frame_num``
  - FrameSet.objects: ``trk_objs``, ``ego_obj``, meta key ``jetstream``
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import IntEnum
from typing import Any, Sequence

import numpy as np

from ouster.sdk.core import Object


class OusterDynamoApiVersion(IntEnum):
    """Supported Ouster Dynamo API versions (V1 only)."""

    V1 = 1


SUPPORTED_API_VERSIONS: list[int] = [int(OusterDynamoApiVersion.V1)]

PROP_OUSTER_DYNAMO_API_VERSION = "ouster_dynamo_api_version"
# One-shot: hello-ouster-dynamo / SimpleViz should clear MAP+ACCUM after reset ego.
PROP_CLEAR_SLAM_VIZ = "clear_slam_viz"

# FrameSet.objects content + meta keys (V1 dynamo_objects).
META_KEY = "jetstream"
KEY_TRACKED = "trk_objs"
KEY_EGO = "ego_obj"


class NotSupportedError(Exception):
    """Raised when an unsupported OusterDynamoAPI version is requested."""


def raise_not_supported(ouster_dynamo_api_version: int) -> None:
    raise NotSupportedError(
        f"OusterDynamoAPI version {ouster_dynamo_api_version} is not supported "
        f"(supported: {SUPPORTED_API_VERSIONS})"
    )


@dataclass
class OusterDynamoAPI_Rx:
    """Publisher-boundary payload (unpack / receive). Heartbeat & telemetry."""

    ouster_dynamo_api_version: int
    frame_num: int = 0

    @classmethod
    def from_dict(cls, payload: Any) -> OusterDynamoAPI_Rx:
        """Unpack a publisher message dict into a received API object."""
        data = payload if isinstance(payload, dict) else {}
        if PROP_OUSTER_DYNAMO_API_VERSION not in data:
            raise NotSupportedError(
                "OusterDynamoAPI_Rx payload missing required field "
                f"'{PROP_OUSTER_DYNAMO_API_VERSION}'"
            )
        version = int(data[PROP_OUSTER_DYNAMO_API_VERSION])
        match version:
            case OusterDynamoApiVersion.V1 | 1:
                return cls(
                    ouster_dynamo_api_version=version,
                    frame_num=int(data.get("frame_num", 0)),
                )
            case _:
                raise_not_supported(version)


@dataclass
class OusterDynamoAPI_Tx:
    """Publisher-boundary payload (pack / transmit).

    Serializes to ``{ouster_dynamo_api_version, frame_num}`` on the heartbeat &
    telemetry EventPublisher channel. Tracked objects travel on the FrameSet UDS
    bridge, not this event API.
    """

    frame_num: int = 0
    ouster_dynamo_api_version: int = int(OusterDynamoApiVersion.V1)

    def raise_not_supported(self) -> None:
        raise NotSupportedError(
            f"OusterDynamoAPI_Tx version {self.ouster_dynamo_api_version} is not "
            f"supported (supported: {SUPPORTED_API_VERSIONS})"
        )

    def to_dict(self) -> dict[str, Any]:
        match self.ouster_dynamo_api_version:
            case OusterDynamoApiVersion.V1 | 1:
                return {
                    PROP_OUSTER_DYNAMO_API_VERSION: int(self.ouster_dynamo_api_version),
                    "frame_num": int(self.frame_num),
                }
            case _:
                self.raise_not_supported()

    def check_rx_unpack(self, payload: dict[str, Any]) -> bool:
        """Round-trip unpack check so pack/unpack schema drift fails loudly."""
        rx = OusterDynamoAPI_Rx.from_dict(payload)
        assert rx.ouster_dynamo_api_version == payload[PROP_OUSTER_DYNAMO_API_VERSION]
        assert rx.frame_num == int(payload.get("frame_num", 0))
        return True


@dataclass
class UnpackedDynamoObjects:
    """FrameSet dynamo_objects after unpack."""

    ouster_dynamo_api_version: int
    tracked: list
    ego: list
    clear_slam_viz: bool = False


def _meta_object(
    ouster_dynamo_api_version: int,
    *,
    clear_slam_viz: bool = False,
) -> Object:
    """Tiny Object used only to carry API version in FrameSet.objects."""
    obj = Object()
    obj.id = 0
    obj.class_id = 0
    obj.class_confidence = 0.0
    obj.dimensions = np.asarray([1e-3, 1e-3, 1e-3], dtype=np.float32)
    obj.properties[PROP_OUSTER_DYNAMO_API_VERSION] = str(int(ouster_dynamo_api_version))
    if clear_slam_viz:
        obj.properties[PROP_CLEAR_SLAM_VIZ] = "1"
    return obj


def pack_dynamo_objects(
    frame_set: Any,
    *,
    tracked: Sequence[Any],
    ego: Sequence[Any],
    ouster_dynamo_api_version: OusterDynamoApiVersion | int = OusterDynamoApiVersion.V1,
    clear_slam_viz: bool = False,
) -> None:
    """Write V1 dynamo_objects onto ``frame_set.objects`` (trk_objs, ego_obj, jetstream)."""
    version = int(ouster_dynamo_api_version)
    if version not in SUPPORTED_API_VERSIONS:
        raise_not_supported(version)
    match version:
        case OusterDynamoApiVersion.V1 | 1:
            frame_set.objects[KEY_TRACKED] = list(tracked)
            frame_set.objects[KEY_EGO] = list(ego)
            frame_set.objects[META_KEY] = [
                _meta_object(version, clear_slam_viz=bool(clear_slam_viz))
            ]
        case _:
            raise_not_supported(version)


def unpack_dynamo_objects(frame_set: Any) -> UnpackedDynamoObjects:
    """Read V1 dynamo_objects from ``frame_set.objects``.

    FrameSets with no Dynamo content (e.g. OSF / sensor) return empty lists and
    ``ouster_dynamo_api_version=0``. If the meta object is present with an
    unsupported version, raises ``NotSupportedError``.
    """
    objects = getattr(frame_set, "objects", None)
    if objects is None:
        return UnpackedDynamoObjects(
            ouster_dynamo_api_version=0, tracked=[], ego=[], clear_slam_viz=False
        )

    has_meta = META_KEY in objects
    has_content = KEY_TRACKED in objects or KEY_EGO in objects
    if not has_meta and not has_content:
        return UnpackedDynamoObjects(
            ouster_dynamo_api_version=0, tracked=[], ego=[], clear_slam_viz=False
        )

    if not has_meta:
        raise NotSupportedError(
            f"FrameSet has dynamo object lists but missing meta key '{META_KEY}' "
            f"with '{PROP_OUSTER_DYNAMO_API_VERSION}'"
        )

    meta_list = objects[META_KEY] or []
    if not meta_list:
        raise NotSupportedError(f"FrameSet meta '{META_KEY}' is empty")
    # SDK Object.properties is DictStrStr (mapping without .get).
    props = dict(getattr(meta_list[0], "properties", None) or {})
    raw_ver = props.get(PROP_OUSTER_DYNAMO_API_VERSION)
    if raw_ver is None:
        raise NotSupportedError(
            f"FrameSet meta '{META_KEY}' missing '{PROP_OUSTER_DYNAMO_API_VERSION}'"
        )
    version = int(raw_ver)
    if version not in SUPPORTED_API_VERSIONS:
        raise_not_supported(version)

    match version:
        case OusterDynamoApiVersion.V1 | 1:
            tracked = list(objects[KEY_TRACKED]) if KEY_TRACKED in objects else []
            ego = list(objects[KEY_EGO]) if KEY_EGO in objects else []
            clear_flag = str(props.get(PROP_CLEAR_SLAM_VIZ, "")).strip().lower() in (
                "1",
                "true",
                "yes",
            )
            return UnpackedDynamoObjects(
                ouster_dynamo_api_version=version,
                tracked=tracked,
                ego=ego,
                clear_slam_viz=clear_flag,
            )
        case _:
            raise_not_supported(version)

# Ouster Dynamo API (V1)

The ouster-dynamo producer is **Ouster proprietary software**, distributed and installed separately from this repo.

This API defines how **ouster-dynamo** shares data with other Jetstream apps (for example hello-ouster-dynamo).

It is a small, versioned wire format.

## Where the code lives

| Copy | Path |
|------|------|
| **Consumer copy** (hello-ouster-dynamo) | `dynamo-utils/ouster_dynamo_api.py` (this tree) |
| **Producer** (another Jetstream app, e.g. ouster-dynamo) | ships its own copy of the same constants |

hello-ouster-dynamo’s Nix build uses only the consumer copy above. A separate producer app checkout is optional and never required to build or run hello-ouster-dynamo.

## Versioning


| Enum                        | Value | Notes           |
| --------------------------- | ----- | --------------- |
| `OusterDynamoApiVersion.V1` | `1`   | initial version |


On the wire, the version field is always named `ouster_dynamo_api_version`. Older field names (`api_version`) and older FrameSet keys (`tracked_objs`, `ego`) are rejected so broken or half-migrated clients fail loudly instead of silently misreading data.

When you publish a frame, pass the **same** version into both channels (EventPublisher and FrameSet) so timing and objects stay aligned.

## Two channels

**ouster-dynamo** splits shared data into two channels:

1. **Heartbeat & telemetry** — EventPublisher channel for per-frame heartbeat / telemetry (`frame_num` + version today; more telemetry fields may ride here later).
2. **FrameSet UDS bridge** — provides the actual tracked objects and ego_object, stored under `FrameSet.objects`.

Consumers usually listen for the Channel 1 heartbeat, then read the matching FrameSet for that frame’s objects.

Live tracking/viz/extrinsics knobs are **not** part of this API. They are adjusted only inside ouster-dynamo’s Jetstream Configs panel via a local `/live-config` HTTP endpoint (pending file → once-per-frame apply).

### Channel 1: Heartbeat & telemetry

EventPublisher channel for “frame N is ready” heartbeat signaling and related telemetry.

- **Pack (send):** `OusterDynamoAPI_Tx(...).to_dict()`
- **Unpack (receive):** `OusterDynamoAPI_Rx.from_dict(payload)`

Example payload:

```json
{
  "ouster_dynamo_api_version": 1,
  "frame_num": 42
}
```



### Channel 2: FrameSet `dynamo_objects`

This channel passes the dynamo objects embedded within FrameSets. Helpers write and read three keys on `FrameSet.objects`:

- **Pack (send):** `pack_dynamo_objects(...)`
- **Unpack (receive):** `unpack_dynamo_objects(...)` → `UnpackedDynamoObjects`


| Key         | What it holds                                                                     | Why it exists                                                  |
| ----------- | --------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `trk_objs`  | Tracked SDK `Object` list                                                         | Dynamic objects ouster-dynamo detected / tracked               |
| `ego_obj`   | Ego list (length 1)                                                               | The vehicle / sensor ego pose as an `Object`                   |
| `jetstream` | Meta only: one tiny `Object` whose `properties` carry `ouster_dynamo_api_version` | Lets the consumer know which schema to expect without guessing |


**Edge cases when unpacking**

- FrameSets with no Dynamo content (plain OSF / sensor data) unpack to empty lists and `ouster_dynamo_api_version=0`. That means “nothing Dynamo-related here,” not an error.
- If content keys are present but the `jetstream` meta is missing, or the version is unsupported, unpack raises `NotSupportedError`.



## Producer example

On the ouster-dynamo side: write objects onto the FrameSet, then publish the timing event for that frame number.

```python
from ouster_dynamo_api import (
    OusterDynamoApiVersion,
    OusterDynamoAPI_Tx,
    pack_dynamo_objects,
)

ver = OusterDynamoApiVersion.V1
pack_dynamo_objects(
    frame_set, tracked=tracks, ego=[ego_obj], ouster_dynamo_api_version=ver,
)
tx = OusterDynamoAPI_Tx(frame_num=frame_num, ouster_dynamo_api_version=int(ver))
publisher.publish(tx.to_dict(), kind="frame")
```



## Consumer example

On an app like hello-ouster-dynamo: unpack the FrameSet and use the tracked / ego lists (and the version if you need to branch later).

```python
from ouster_dynamo_api import unpack_dynamo_objects

unpacked = unpack_dynamo_objects(frames)
trk_objs = unpacked.tracked
ego_obj = unpacked.ego
ver = unpacked.ouster_dynamo_api_version
```



## Visualization tip

If you draw FrameSet objects in SimpleViz, register only the content keys (`trk_objs`, `ego_obj`) in `object_list_keys` / class maps.

Do **not** register `jetstream` — that key is metadata for the API version, not something meant to appear in the scene.
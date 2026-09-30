# dynamo-utils

The ouster-dynamo producer is **Ouster proprietary software**, distributed and
installed separately from this repo.

SimpleViz overlay helpers for **app** FrameSet sources (e.g. an ouster-dynamo
peer on `/run/ouster-dynamo/frameset.sock`):

| File | Role |
|------|------|
| `ouster_dynamo_api.py` | V1 peer API constants (heartbeat + FrameSet object keys) |
| `ouster_dynamo_api_README.md` | Peer protocol notes for consumers |
| `dynamo_viz_utils.py` | Labels, box colors, OSD overlays on SimpleViz |
| `detect_utils.py` | Class id / color maps for tracked objects |
| `configs/` | Viz overlay defaults (`viz_configs.json`, etc.) |

Installed under `/usr/lib/hello-ouster-world/dynamo-utils` on `PYTHONPATH`
when the app runs.

## Ownership

These files are **part of hello-ouster-world** and are required for its Nix
build. They define the consumer-side peer API for another Jetstream app (for
example ouster-dynamo). Keep the peer API constants compatible with the
producer when that app changes; optional live knobs stay on the producer.

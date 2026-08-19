# hello-ouster-world

Renders lidar FrameSets in SimpleViz on the HDMI panel and/or the web
**Display**. Every mode uses the SDK’s `open_source()`:


| Source | `open_source` argument | via ouster-dynamo |
| ---------------------- | ------------------------------------------------------------------------------------------ | --- |
| **Local file**         | OSF path under `/srv/recordings/`                                                          | off: hello-ouster-world plays the OSF; on: Dynamo plays it, hello-ouster-world reads `/run/ouster-dynamo/frameset.sock` |
| **connected sensor**   | hostname / IP                                                                              | off: hello-ouster-world owns UDP lidar; on: Dynamo owns lidar, hello-ouster-world reads the FrameSet socket |


Selection is persisted in `/var/lib/hello-ouster-world/source-config.json` and
exposed at `GET/POST /api/v1/source-config`. **Lidar** is OSF vs sensor.
**via ouster-dynamo** is a separate toggle (not a third source). **Ouster Dynamo**
is proprietary Ouster software, installed separately (not built from this repo).
Changing source SIGINTs SimpleViz so it rebinds (Sway stays up). App FrameSet
sockets: see [apps/specs/frameset-socket.md](../../specs/frameset-socket.md).

## Ouster SDK

Uses `apps/nix/ouster-sdk.nix` (`ps.ousterSdk`) — source build against nixpkgs
GLFW (native Wayland). Python 3.12 from the same nixpkgs pin. Flake input is
public `github:ouster-lidar/ouster-sdk` **v1.0.1**. Peer `.sock` paths use
`osf_socket.open_source` (installed with this app; not patched into the SDK).

Runtime: `GLFW_PLATFORM=wayland`, Sway with **no Xwayland**. SimpleViz is a native
Wayland GLFW client (same pattern as hello-ouster-terminal’s `ouster-cli viz`).

## Peer FrameSet sockets (e.g. ouster-dynamo)

Any producer publishes with `OsfSocketPublisher` on `/run/<app-id>/frameset.sock`.
**Ouster Dynamo** (proprietary, installed separately) publishes
`/run/ouster-dynamo/frameset.sock` while it runs. hello-ouster-world opens that
path with `open_source` when **via ouster-dynamo** is on.

Track-overlay viz helpers (not transport) live under `dynamo-utils/` (shipped
with this app). FrameSet object layout (`trk_objs`, `ego_obj`, version meta) is
documented in
[`dynamo-utils/ouster_dynamo_api_README.md`](dynamo-utils/ouster_dynamo_api_README.md).

If the peer stops, hello-ouster-world shows waiting/disconnected overlay text and
reopens automatically when FrameSets return.

Dynamo API v1 `kind=frame` events are timing-only (`ouster_dynamo_api_version` +
`frame_num`); tracked objects ride on the FrameSet UDS bridge.

## Display panel

- **Display** master toggle enables or disables the companion
`hello-ouster-world-display` unit (Sway/SimpleViz). Persisted in
`/var/lib/hello-ouster-world/display-config.json` (+ `display.enabled` flag).
- **Lidar** (Local file / Sensor) + **via ouster-dynamo** toggle + Apply.
  Direct: SimpleViz opens the OSF or sensor. Via Dynamo: SimpleViz reads
  `/run/ouster-dynamo/frameset.sock` and waits until FrameSets arrive.
- **HDMI** / **Browser** toggles are independent; when both are on they show the
same PointViz. Browser stream keeps working if HDMI is blanked or unplugged
(persistent Sway `HEADLESS` home output).
- When neither HDMI nor a browser stream consumer is active, SimpleViz pauses.
- Hover the stream for mouse/keyboard into the viz. **Console** follows app
stdout/stderr; Ctrl-C interrupts SimpleViz only (Sway stays up).
- Layout: **Display** → **Console**.

## Build

```bash
nix build .#hello-ouster-world .#hello-ouster-world-cross-aarch64-linux
```

Output: `result/` (erofs DDI image)

## Run on the device

### 1. Deploy to device

```bash
export JETSTREAM_MC_PASSWORD='…'
./scripts/jsd-app-deploy.sh hello-ouster-world http://HOST/
```

The deploy script nix-builds the image for the device, then uploads it.
Skip that and install a `.raw` from `nix build` if you already built it.

### 2a. Running on local-file OSF

In the Display panel, pick **Local file**, then **Download** an example from
the catalog (or **Upload** your own `.osf`). Apply.

### 2b. Running with ouster-dynamo (optional)

Install **Ouster Dynamo** on the device (proprietary app, separate from this
repo). Turn **via ouster-dynamo** on, Display ON, **Apply**. SimpleViz reads
`/run/ouster-dynamo/frameset.sock`. Off: hello-ouster-world opens the OSF or
sensor itself.

### 2c. Running on a live sensor

Set source to **connected sensor**, enter hostname/IP, Apply.
The backend opens UDP **7502** (lidar) and **7503** (IMU). Point the sensor at
this host on those ports.

### 3. To remotely view logs

```bash
ssh js@HOST 'journalctl -u hello-ouster-world -f'
```

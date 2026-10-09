# hello-ouster-world

`hello-ouster-world` is the direct Ouster SDK viewer. It opens OSF files or a network sensor with SimpleViz, displays on HDMI and in the browser, and can record live sensor frames to OSF. The separate [`hello-ouster-dynamo`](../hello-ouster-dynamo/hello-ouster-dynamo.md) app keeps the advanced workflow with optional Ouster Dynamo FrameSet input and overlays.

## Build and deploy

From `apps/`, build the native and Jetson images:

```sh
nix build .#hello-ouster-world
nix build .#hello-ouster-world-cross-aarch64-linux
```

Both outputs are `.raw` EROFS DDI images. To build and deploy to an existing Jetstream host, run:

```sh
teka app deploy --build hello-ouster-world --host http://HOST/
```

Open `http://HOST/app/hello-ouster-world` to control the display. The tested native and cross images were built from revision `2be1287c02f934b298ea3324af8ee6a5cc26e577`:

| Target | Flake output | DDI size |
| --- | --- | ---: |
| `x86_64-linux` (native) | `.#hello-ouster-world` | 743,849,984 bytes (709.39 MiB) |
| `aarch64-linux` (cross) | `.#hello-ouster-world-cross-aarch64-linux` | 639,840,256 bytes (610.20 MiB) |

The measured Ouster viewer image sizes are also listed in [`apps/specs/erofs-ddi.md`](../../specs/erofs-ddi.md).

## Device tests

From the repository root, build, deploy and test on a Jetstream device:

```sh
nix run ./apps/examples/hello-ouster-world/tests#test-hello-ouster-world -- --host "$HOST"
```

The standalone [`tests/`](tests/) flake pins Python, pytest, Playwright and Chromium. Set `TEKA_PASSWORD` when the device requires login. The tests invoke `teka app deploy` against the checkout's `apps/flake.nix`; pass `--apps-flake PATH` to the pytest runner when running outside the checkout.

Tests are grouped into four modules:

| Module | Coverage |
|--------|----------|
| `test_discovery.py` | Sensor roster and picker selection |
| `test_recording.py` | Cloud download, timed/manual recording, active-file protection, recording download/delete/re-upload and byte integrity |
| `test_visualization.py` | Live sensor, recording and cloud playback; pause/step/resume and camera input |
| `test_ui.py` | Display controls, fullscreen, help modal, PNG screenshots, console and guest restrictions |
| `test_security.py` | Signed-out, guest and signed-in route access; source and OSF input confinement |

Pass `--sensor SENSOR_HOST` to enable discovery, recording and live-sensor checks. These tests skip when no hostname is supplied; existing device settings are not used to choose a sensor. UI and cloud playback checks run without a sensor, sharing one sample per session. Discovery checks the roster and picker, not cold-boot auto-selection. HDMI checks require a monitor; guest restrictions are skipped in open mode.

Run against an idle viewer. Tests refuse to interrupt an active or configured recording, restore settings at session teardown and remove their own device files. Studio import can download several gigabytes; `--skip-import` uses cached samples and skips sample-dependent tests if none are available. `--skip-deploy` uses the installed image.

Standard pytest options work, including `-k discovery`, `-k recording`, `-k visualization`, `-k ui`, `--collect-only`, `--junitxml FILE`, `--output DIR`, and `--tracing on`. Failure screenshots and traces default to ignored `tmp/test-hello-ouster-world/` and contain device URLs. For example:

```sh
nix run ./apps/examples/hello-ouster-world/tests -- --host "$HOST" --sensor "$SENSOR_HOST" --skip-deploy -k recording
```

## Sources

A fresh installation starts enabled in sensor mode with no selected host and recording disabled. The backend subscribes to the Jetstream `sensors` app roster, even before a browser connects. A sensor whose UDP destination is this device is applied immediately; otherwise it scans for 30 seconds after the first snapshot, then applies a sole usable sensor. Multiple sensors with none streaming here require an explicit selection. The viz placeholder shows the scan countdown and auto-configuration progress. An empty roster leaves manual hostname entry available; unavailable discovery is shown separately and retries. The sensor picker freezes its displayed rows during selection, while the backend keeps consuming roster updates. Discovery stays available while a source is selected or stopped. Choose a dropdown entry to fill the hostname/IP field, then **Apply** to start it. Sensors whose UDP destination is this device are starred and listed first when Jetstream's sensors app reports `udp_dest`.

Before streaming, the app applies supported measurement defaults, sets NORMAL mode and UDP ports 7502/7503, and verifies the active configuration. A configuration error remains visible for retry. A saved sensor target receives the same setup. The app does not factory-reset sensor networking or identity.

Choose **Local file** to play an OSF from `/srv/recordings`, import one from the Studio catalog, or upload one. **Apply** starts the chosen source; **Stop** preserves the selection but disables it. **Record** writes live sensor frames to `/srv/recordings/hello-ouster-world-YYYYMMDD-HHMMSS.osf`. The duration field limits recording to 120 seconds by default, with a range of 1–3,600 seconds. The limit starts on Apply; finalizing the recording leaves visualization running.

## Display

HDMI and browser display can be enabled independently. A persistent Sway headless output keeps browser capture and backend discovery available without an attached monitor. The Ouster logo and source status appear while waiting for frames. Browser controls include source selection, recording, display settings, fullscreen, and the app console. Source and display settings collapse to status chips while a source is running; **Configure** reopens them. The console starts collapsed and disconnected; **Open** connects it. Mouse, keyboard, and Ctrl-C console input are forwarded to the visualization; the HDMI pointer is hidden.

The display fits its width and height to the available viewport while preserving the stream aspect ratio. The **?** button beside fullscreen opens a scrollable keybinding dialog; **Close** or Esc dismisses it, including in fullscreen. The camera button copies the current frame as a PNG at the actual stream resolution, without the web controls or resizing. Clipboard access requires a secure browser context (HTTPS or localhost). If unavailable or denied, the PNG is downloaded instead and a message explains why.

Hover over the browser display to take control. Press **?** (**Shift+/**) to open the browser help dialog. Its smaller, grouped key list follows the loaded source: replay and advanced controls appear for recordings, sensor keys match the loaded sensor count, and object/zone controls require matching data. Exit, device screenshot, and disabled map/accumulation commands are blocked by the remote input bridge and omitted from browser help. A local USB keyboard still uses the SDK's original help and controls.

The bindings below come from the pinned Ouster SDK 1.0.1 [viewer](https://github.com/ouster-lidar/ouster-sdk/blob/46b7bceee7995914e2f3f3facf4d78418335b2d7/python/src/ouster/sdk/viz/core.py) and [camera controls](https://github.com/ouster-lidar/ouster-sdk/blob/46b7bceee7995914e2f3f3facf4d78418335b2d7/ouster_viz/src/point_viz.cpp). Letters are unshifted unless stated; Shift reverses cycles where listed.

| Keys | Action |
| --- | --- |
| `?` | Toggle help and print bindings to the console |
| `w` / `s` | Pitch camera down / up |
| `a` / `d` | Yaw camera right / left |
| `q` / `e` | Roll camera left / right |
| `=` / `-` | Dolly in / out |
| Shift + camera movement keys | Move camera in smaller increments |
| `Shift+r` | Reset camera orientation |
| `Shift+1` / `Shift+2` / `Shift+3` | Top / front / left view |
| `Ctrl+r` | Bird's-eye view |
| `0` | Toggle orthographic camera |
| `Ctrl+-` / `Ctrl+=` | Increase / decrease field of view in perspective mode |
| `i` / `Shift+i` | Increase / decrease 2D image size |
| `Ctrl+i` | Flip or hide 2D images |
| `b` / `Shift+b` | Cycle top 2D image forward / backward |
| `n` / `Shift+n` | Cycle bottom 2D image forward / backward |
| `p` / `Shift+p` | Increase / decrease point size |
| `1` / `2` | Toggle first / second return cloud |
| `Ctrl+1` … `Ctrl+9` | Toggle the corresponding sensor's clouds (up to the available sensor count) |
| `m` / `Shift+m` | Cycle cloud coloring mode forward / backward |
| `f` / `Shift+f` | Cycle cloud palette forward / backward |
| `c` / `Shift+c` | Cycle highlight mode forward / backward |
| `Ctrl+c` | Cycle object palette |
| `Ctrl+Shift+c` | Cycle object color attribute |
| `5` | Cycle object display mode |
| `y` | Cycle zone set |
| `Ctrl+y` | Cycle zone render mode |
| `u` / `Shift+u` | Cycle camera mode forward / backward |
| `8` | Cycle track view mode |
| `9` | Toggle axes at the frame origin |
| `'` / `Shift+'` (`"`) | Increase / decrease range marker spacing |
| `Ctrl+'` | Cycle range marker thickness or hide markers |
| `Ctrl+s` | Toggle sky visualization |
| `o` | Toggle information overlay |
| `Ctrl+o` | Cycle overlay scale |
| `j` / `Shift+j` | Increase / decrease accumulated cloud or map point size |
| `k` / `Shift+k` | Cycle accumulated cloud or map coloring forward / backward |
| `g` / `Shift+g` | Cycle accumulated cloud or map palette forward / backward |
| `6` / `7` | Toggle accumulation / map view |
| Space | Pause / resume |
| `.` / `,` | Step forward / backward one frame |
| `Ctrl+.` / `Ctrl+,` | Step forward / backward ten frames |
| `>` / `<` (`Shift+.` / `Shift+,`) | Increase / decrease replay rate |
| `h` / `Shift+h` | Increase / decrease camera subframes |
| `Shift+z` | Take a screenshot on the device |
| `Shift+x` | Toggle continuous screenshot saving on the device |
| `Ctrl+x` | Cycle continuous screenshot format |
| `v` / `Shift+v` | Cycle screenshot resolution factor forward / backward |

Object and zone controls need matching source data. Accumulation and map controls are registered by the SDK, but this app uses the SDK defaults with accumulation and map collection disabled. Replay rate controls apply to recorded sources. The SDK help text labels range marker keys as `"`; its handlers use the unshifted apostrophe key, as listed above.

On a local USB keyboard, Shift with mouse drag translates the camera and Esc exits the viewer. Browser pointer events currently do not forward modifiers; remote Esc is ignored by the bridge and can exit browser fullscreen. Some browser or operating system shortcuts may take priority over viewer keys.

## Guest view

Signed-out visitors can watch the browser point-cloud stream only (no console, source settings, or remote control). **Guest view** defaults on and is toggled by signed-in users in the Display panel. Jetstream without a password (open mode) treats everyone as signed in for API access.

| Surface | Who decides |
|---------|-------------|
| `/app/hello-ouster-world` shell + static assets | Host page-session; manifest `ui.anonymous: true` |
| `GET /api/v1/app/hello-ouster-world/status` | App backend: public liveness probe (`ok`, `app` only) |
| `GET /api/v1/app/hello-ouster-world/guest-access` | App backend: public (guest view flag) |
| `GET /api/v1/app/hello-ouster-world/display/stream`, `/display/status` | App backend: guests when guest view is on (view only; input rejected) |
| All other app API routes | App backend: signed-in Jetstream session (`X-MC-Principal-Id`) or open-mode Jetstream (no password) |

## Existing installations

Stop both viewer apps before migrating the previous combined demo. Move its state directory from `/var/lib/hello-ouster-world` to `/var/lib/hello-ouster-dynamo` for the advanced app. Start the new `hello-ouster-world` with a fresh state directory so it gets the direct sensor default. Do not copy the old `display-config.json` into the new World app or silently reinterpret its Dynamo source as direct sensor input. The terminal viewer is unchanged.

## Architecture

```mermaid
flowchart LR
    MC[Jetstream sensors roster] -->|WebSocket| API[World backend]
    UI[Browser controls] <-->|HTTP and WebSocket| API
    API -->|apply source and status| VIZ[Ouster SDK SimpleViz]
    OSF[OSF file] --> VIZ
    SENSOR[Ouster sensor] -->|UDP 7502 / 7503| VIZ
    VIZ -->|Wayland| HDMI[HDMI display]
    VIZ -->|capture stream| UI
    VIZ -->|optional recording| REC[/srv/recordings]
```

The app builds against the public Ouster SDK pinned in `apps/nix/ouster-sdk.nix`. It uses SDK `open_source` directly for sensor and OSF input.

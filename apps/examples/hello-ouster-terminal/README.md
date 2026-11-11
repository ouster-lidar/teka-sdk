# hello-ouster-terminal

## Goal

**Sway** kiosk app with **foot** terminal running **ouster-cli viz** (Wayland/GLFW). Leases the configured UDP range (7000–8000 inclusive by default) through the [SDK runtime](https://github.com/ouster-lidar/teka-sdk/blob/main/apps/lib/app-runtime/teka-runner.md) for lidar/IMU traffic.
The manifest uses `backend.mode = none`; the host must support resource-only activation.

## Build and deploy

```sh
teka app deploy --build
```

## Host

**`X-Teka-PortableGPU=wayland`**, **`/run/opengl-driver`**, **`seatd`**. x86 hosts may need the portable GL extension to bind Mesa **`libLLVM`** store roots (see comments in `default.nix`).

## In-kiosk docs

Bundled under `~/docs` in the app image (`README.md`, `sway.md`, `viz.md`), shown on
first foot launch via `bat -p`.

Plain-terminal friendly for foot/bat:

- Use `#` headings (not underline-style titles).
- Inline `` `command` `` is fine; do **not** use triple-backtick fenced code blocks.
- Indent example commands (four spaces) instead of fences.
- No bold; cross-ref `sway.md` / `viz.md` for layout and viz details.

## Debug shell (host)

From the host as root, join the app unit namespace with `systemd-run` (always on the
host; do not document `nsenter`):

    systemd-run -tp JoinsNamespaceOf=hello-ouster-terminal.service bash

The DDI `RootImage` `/bin` symlink exposes bash and busybox applets for that shell.

## Verify

Configure the sensor destination to this host with lidar UDP 7602 and IMU UDP 7603, then run `ouster-cli viz` in the kiosk, pointing it at the sensor hostname/IP.
Override these ports and the lease range in `default.nix`.

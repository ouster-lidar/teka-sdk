# hello-ouster-terminal

## Goal

Portable **Sway** kiosk with **foot** terminal running **ouster-cli viz** (Wayland/GLFW). Opens a UDP port range via **firewalld** for lidar/IMU traffic when using custom sensor ports.

## Build

`nix build .#hello-ouster-terminal` (cross attr for aarch64 from x86_64).

## Host

**`X-Jetstream-PortableGPU=wayland`**, **`/run/opengl-driver`**, **`seatd`**. x86 hosts may need the portable GL extension to bind Mesa **`libLLVM`** store roots (see comments in `default.nix`).

## Deploy

Point viz at a sensor hostname/IP after attach. `./scripts/jsd-app-deploy.sh hello-ouster-terminal http://HOST/`

## In-kiosk docs

Bundled under `~/docs` in the portable image (`README.md`, `sway.md`, `viz.md`), shown on
first foot launch via `bat -p`.

Plain-terminal friendly for foot/bat:

- Use `#` headings (not underline-style titles).
- Inline `` `command` `` is fine; do **not** use triple-backtick fenced code blocks.
- Indent example commands (four spaces) instead of fences.
- No bold; cross-ref `sway.md` / `viz.md` for layout and viz details.

## Debug shell (host)

From the host as root, join the portable unit namespace with `systemd-run` (always on the
host; do not document `nsenter`):

    systemd-run -tp JoinsNamespaceOf=hello-ouster-terminal.service bash

The DDI `RootImage` `/bin` symlink exposes bash and busybox applets for that shell.

## Non-goals

Native X11 + GLX (`hello-ouster-terminal-x11`).

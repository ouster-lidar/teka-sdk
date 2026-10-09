# apps — `hello-firefox-kiosk` (Wayland kiosk + WebGL on Jetstream)

## Goal

An **app image** DDI (read-only erofs **`.raw`**) that runs **Cage** (single-client wlroots compositor) and **Firefox** in **kiosk** mode, loading **[webglreport.com](https://webglreport.com/?v=2)** (WebGL 2 report) on a Jetstream host with an **NVIDIA** stack. The image follows **`hello-wayland-gpu`**: it relies on the host for **kernel / DRM / NVIDIA** plus GLVND, EGL vendor data, GBM, and paths under **`/run/opengl-driver`**.

## Browser choice

- **`firefox --kiosk`** with **`MOZ_ENABLE_WAYLAND=1`** — current nixpkgs Firefox is the less stupid option here after Chromium-derived cross builds hit host-tool / bindgen breakage on **x86_64 → aarch64**. **Cage** runs a single client fullscreen; the inner script restarts Firefox in a loop if it exits and seeds a writable kiosk profile under **`$HOME`**.

## Kiosk page

- Default URL: **[WebGL Report](https://webglreport.com/?v=2)** (WebGL 2 capability page).
- The concrete URL is embedded in the run script; operators can change it by editing **`apps/examples/hello-firefox-kiosk/default.nix`** (rebuild the image) if a different page is required.

## Build

- **Flake:** **`apps/`** (`flake.nix`).
- **Service key:** **`hello-firefox-kiosk`** under `services` → **`nix build .#hello-firefox-kiosk`** and **`nix flake check`** use that name.
- **Layout:** this directory: **`default.nix`**, **`run.sh.in`**, **`hello-firefox-kiosk.md`**. **DBus session** ( **`dbus-run-session`**) + **`cage --`** + **Firefox** loop, with the same DRM and EGL external platform checks as **`hello-wayland-gpu`**.
- **Zed Box Mini / aarch64:** use **`hello-firefox-kiosk-cross-aarch64-linux`** when building on **x86_64** so the app image closure matches the device CPU. From repo root: **`nix build ./apps#hello-firefox-kiosk-cross-aarch64-linux`**, or **`cd apps && nix build .#hello-firefox-kiosk-cross-aarch64-linux`**. Deploying an **x86_64** **`hello-firefox-kiosk.raw`** on aarch64 yields **`systemd` 203 / EXEC** (wrong ELF / interpreter).
- **Input / seat handling:** wlroots on an app **system** service does not get a real logind session. The app unit therefore binds the host **`/run/seatd.sock`**, orders after **`seatd.service`**, and forces **`LIBSEAT_BACKEND=seatd`** instead of the broken **`builtin`** fallback that can open DRM but miss **`/dev/input/event*`**. Also, do **not** force **`WLR_BACKENDS=drm`** by itself: that disables the **`libinput`** backend, so you get video and zero keyboard / mouse input. Use **`drm,libinput`** (or let wlroots auto-create backends) when you want local input devices.
- **Certificate trust:** the image ships its own CA bundle for sane defaults, then overlays the host CA bundle files when present and registers **`p11-kit-trust`** in the Firefox profile. Binding the host **`/etc/ssl/certs`** directory alone is not enough on NixOS because those entries are often symlinks into a host **`/nix/store`** path the app namespace cannot follow.
- **Exec entrypoint:** **`ExecStart`** points at a **`#!/bin/sh`** stub (app image’s **`/bin/sh`**, same idea as **`hello-world`**) that **`exec`s** bash and the real **`run.sh.in`** body.

## Host integration (GPU)

- **`X-Jetstream-PortableGPU=wayland`** (same as **`hello-wayland-gpu`**).
- Host **`seatd`**: bind **`/run/seatd.sock`**, order after **`seatd.service`** (see **Input / seat handling** above).

## Seat management

- **`Cage`** / **`wlroots`** talk to **`libseat`** for DRM and input device access.
- **`libseat`** can use different backends: **`logind`**, **`seatd`**, or **`builtin`**. These are alternate seat brokers, not the same implementation with different branding.
- Backend selection is separate from wlroots backend selection: **`LIBSEAT_BACKEND=seatd`** chooses the seat broker, while **`WLR_BACKENDS=drm,libinput`** chooses the compositor backends. Setting the first one correctly does **not** magically enable the second one.
- **`systemd-logind`** is the full systemd session / seat manager. It works best when the compositor runs inside a real login session on a VT (for example from **`agetty`** + PAM). A root-owned **app system service** does **not** naturally get that session context.
- **`seatd`** is a small dedicated seat broker exposed over **`/run/seatd.sock`**. For this kiosk it is the least stupid fit: the service can bind the host socket into the image and **`libseat`** can broker **`/dev/input/event*`** and DRM access without pretending the app service is a user login session.
- **`builtin`** is **`libseat`**'s fallback path. It may open DRM directly but still fail to get useful input devices in this setup, which is how you end up with video output and a dead keyboard.
- Current policy for **`hello-firefox-kiosk`**: force **`LIBSEAT_BACKEND=seatd`**, require host **`seatd.service`**, and do **not** rely on **`logind`** for this app service launch model.

## Operator flow

- Build the **`.raw`** on a workstation (`nix build` / cross like other **app** images).
- Attach on a Teka host with **`portablectl`** or **`teka app deploy --build hello-firefox-kiosk --host http://HOST/`**.

## Non-goals

- A full **Electron** app beyond the kiosk one-window flow; multi-tab UI. Input focus and escape behavior follow **Firefox** and **Cage** defaults (no custom keybinding layer).

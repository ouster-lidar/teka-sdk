# hello-wayland-gpu

## Goal

Portable **Sway** compositor with **`vkcube --wsi wayland`** and **`glmark2-wayland`** demo loops. Uses host DRM/Vulkan/EGL via **`X-Jetstream-PortableGPU=wayland`** and optional **`EnvironmentFile=/etc/jetstream/wlroots.env`**.

## Build

`nix build .#hello-wayland-gpu` (or `hello-wayland-gpu-cross-aarch64-linux` from x86_64).

## Host

Requires **`/run/opengl-driver`** on the appliance. **`WLR_BACKENDS=drm,libinput`** when the host supplies `wlroots.env` (do not use **`drm`** alone — input dies).

## Deploy

`./scripts/jsd-app-deploy.sh hello-wayland-gpu http://HOST/`

## Verify

`journalctl -u hello-wayland-gpu -f`; HDMI or headless per host `wlroots.env`.

## Non-goals

Firefox or sensor apps. No X11 / Xwayland (compositor and image are Wayland-only).

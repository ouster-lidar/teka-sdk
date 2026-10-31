# hello-firefox

## Goal

Portable **Sway** + **Firefox** on Wayland. Default start URL is a Babylon.js 3D tiles demo (see `default.nix`).

## Build

`nix build .#hello-firefox` (cross attr for ZBM when building on x86_64).

## Host

**`X-Teka-PortableGPU=wayland`**, host **`seatd`** socket, CA bundle overlay for HTTPS. Use **`WLR_BACKENDS=drm,libinput`** (not **`drm`** alone).

## Deploy

`teka app deploy --build hello-firefox --host http://HOST/`

## Non-goals

Kiosk-only Cage flow (`hello-firefox-kiosk`); shipping a custom browser build beyond nixpkgs Firefox.

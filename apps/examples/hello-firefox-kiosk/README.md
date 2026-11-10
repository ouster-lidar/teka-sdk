# hello-firefox-kiosk

Firefox on Wayland using Cage. The default page is [WebGL Report](https://webglreport.com/?v=2). Change `startUrl` in `default.nix` to use another page.

Requires a Teka device with GPU integration and `seatd`. The SDK supplies the compositor launch, browser profile, certificate trust, and service policy.

## Build

- **Flake:** **`apps/`** (`flake.nix`).
- **Service key:** **`hello-firefox-kiosk`** under `services` → **`nix build .#hello-firefox-kiosk`** and **`nix flake check`** use that name.
- **Zed Box Mini / aarch64:** use **`hello-firefox-kiosk-cross-aarch64-linux`** when building on **x86_64** so the app image closure matches the device CPU. From repo root: **`nix build ./apps#hello-firefox-kiosk-cross-aarch64-linux`**, or **`cd apps && nix build .#hello-firefox-kiosk-cross-aarch64-linux`**. The CLI rejects packages whose architecture differs from the device.

## Operator flow

- Build the **`.tpk`** package on a workstation (`nix build` / cross like other **app** packages).
- Deploy on a Teka host with **`teka app deploy --build hello-firefox-kiosk --host http://HOST/`**.

Verify that the browser opens the configured page and that local keyboard and mouse input work.

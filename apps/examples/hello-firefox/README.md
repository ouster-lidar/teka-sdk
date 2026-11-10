# hello-firefox

Firefox on Wayland using Sway. The default page is [a Babylon.js 3D tiles demo](https://demos.babylonjs.com/3d-tiles/). Change `startUrl` in `default.nix` to use another page.

Requires a Teka device with GPU integration and `seatd`. The SDK supplies the compositor launch, browser profile, certificate trust, and service policy.

## Build

`nix build .#hello-firefox` (cross attr for ZBM when building on x86_64).

## Deploy

`teka app deploy --build hello-firefox --host http://HOST/`

Verify that the browser opens the configured page and that local keyboard and mouse input work.

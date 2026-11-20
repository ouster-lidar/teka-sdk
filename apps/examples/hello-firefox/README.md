# hello-firefox

Firefox on Wayland using Sway. The default page is [a Babylon.js 3D tiles demo](https://demos.babylonjs.com/3d-tiles/). Set `startUrl` in `configuration.nix` to use another page. The `mode` setting selects `browser` (Sway) or `kiosk` (Cage).

Requires a Teka device with GPU integration and `seatd`. The SDK supplies the compositor launch, browser profile, certificate trust, and service policy.

## Build and deploy

Run this command from the app directory.

```sh
teka app deploy
```

Verify that the browser opens the configured page and that local keyboard and mouse input work.

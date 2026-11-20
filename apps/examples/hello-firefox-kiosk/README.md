# hello-firefox-kiosk

Firefox on Wayland using Cage. The default page is [WebGL Report](https://webglreport.com/?v=2). Set `startUrl` in `configuration.nix` to use another page. The `mode` setting selects `browser` (Sway) or `kiosk` (Cage).

Requires a Teka device with GPU integration and `seatd`. The SDK supplies the compositor launch, browser profile, certificate trust, and service policy.

## Build and deploy

Run this command from the app directory.

```sh
teka app deploy
```

Verify that the browser opens the configured page and that local keyboard and mouse input work.

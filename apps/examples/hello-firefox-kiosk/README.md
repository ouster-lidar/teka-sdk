# hello-firefox-kiosk

Firefox on Wayland using Cage. The default page is [WebGL Report](https://webglreport.com/?v=2). Change `startUrl` in `default.nix` to use another page.

Requires a Teka device with GPU integration and `seatd`. The SDK supplies the compositor launch, browser profile, certificate trust, and service policy.

## Build and deploy

```sh
teka app deploy --build
```

Verify that the browser opens the configured page and that local keyboard and mouse input work.

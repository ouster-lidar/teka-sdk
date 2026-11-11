# hello-slidev

Slidev presentation demo packaged as a Teka app. Builds a static SPA with [Slidev](https://sli.dev/) and serves it through the Teka app shell.

## What it demonstrates

- **Slidev** deck with Teka + example-apps overview
- **Integrated panel** at `/app/hello-slidev` (Slidev deck in an iframe)
- **Guest access toggle** (signed-in users) stored in app `StateDirectory`; the Rust backend enforces deck access from the toggle + `X-MC-Principal-Id` (host forwards session identity, does not gate the deck)
- **Deck** at `/api/v1/app/hello-slidev/deck/…` (feature API proxy — app backend enforces guest-access; avoids host HTML login redirect on `/app/…/deck/`)

## Auth model

| Surface | Who decides |
|---------|-------------|
| `/app/hello-slidev` shell + panel static assets | Host page-session; manifest `ui.anonymous: true` so guests can load the panel |
| `/api/v1/app/hello-slidev/guest-access` | App backend: public GET, PUT requires principal |
| `/api/v1/app/hello-slidev/deck/…` | App backend: allow when guest access is on **or** request has `X-MC-Principal-Id` |

## Build and deploy

```sh
teka app deploy --build
```

Slidev runs at **build time** on the build host (`pkgs.buildPackages`); the app image ships the `dist/` tree under `usr/lib/hello-slidev/deck/`. Ouster marketing images are pinned with `fetchurl` in `ouster-assets.nix` so the deck works offline on edge devices. Cross builds (`teka.apps.hello-slidev.packages.x86_64-linux.aarch64-linux`) do not compile Node on the target architecture.

## Verify

1. Open `/app/hello-slidev` signed in — deck loads in the panel iframe
2. Enable **Guest access**, then open `/app/hello-slidev` in a private window — deck loads
3. Disable guest access — private window shows the panel but the iframe stays blank (`401` on deck)
4. Direct deck URL: `/api/v1/app/hello-slidev/deck/index.html` (respects guest-access toggle)
5. Press `f` in Slidev for presenter fullscreen
6. Journal: `hello-slidev listening on http://127.0.0.1:…`

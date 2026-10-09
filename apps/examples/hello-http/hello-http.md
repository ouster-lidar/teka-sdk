# hello-http

## Goal

**Python 3** HTTP server app on **0.0.0.0:8000** returning a small HTML page.
Jetstream proxies directly to this fixed listener and manages its TCP 8000 lease through the [SDK runtime](../../lib/app-runtime/runtime-process.md).

## Build

`nix build .#hello-http` from `apps/`.

## Deploy

`teka app deploy --build hello-http --host http://HOST/` from `apps/`.

## Verify

`curl http://<host>:8000/` → 200 and `Hello world` body.
Check `/app/hello-http` through Jetstream as well; its **Direct endpoint** link opens the same service on `http://<host>:8000/`.

## App UI

The image ships a Jetstream app manifest. After install, open **Hello HTTP** in the nav (`/app/hello-http`) or the installed card on `/apps`.

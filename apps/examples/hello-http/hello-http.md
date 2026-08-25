# hello-http

## Goal

Portable **Python 3** HTTP server on **0.0.0.0:8000** returning a small HTML page. Uses host **firewalld** to open/close the port while the service runs.

## Build

`nix build .#hello-http` from `apps/`.

## Deploy

`./scripts/jsd-app-deploy.sh hello-http http://HOST/` from `apps/` (host **firewalld** must be active).

## Verify

`curl http://<host>:8000/` → 200 and `Hello world` body. `firewall-cmd --query-port=8000/tcp` is **yes** while the unit is active.

## App UI

The image ships a Jetstream app manifest. After install, open **Hello HTTP** in the nav (`/app/hello-http`) or the installed card on `/apps`.

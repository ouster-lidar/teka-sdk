# hello-webui

Static web UI demo: animated hero, counters, and a Snake game panel. No backend.

## Build

`nix build .#hello-webui` from `apps/`.

## Deploy

`./scripts/jsd-app-deploy.sh hello-webui http://HOST/`

## Verify

Open `/app/hello-webui` (Demo and Snake panels). Journal: `hello-webui: static web UI demo ready`.

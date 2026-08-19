# hello-world

## Goal

Minimal portable smoke test: a **oneshot** unit that logs `hello world` to the journal. No GPU, no network listeners.

## Build

`nix build .#hello-world` from `apps/`.

## Deploy

Attach like any other app portable image (`portablectl`, or `./scripts/jsd-app-deploy.sh hello-world http://HOST/`).

## Verify

`systemctl status hello-world.service` — active/exited with `RemainAfterExit=yes`; `journalctl -u hello-world.service` shows the message.

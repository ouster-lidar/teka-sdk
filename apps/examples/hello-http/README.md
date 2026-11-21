# hello-http

## Goal

**Python 3** HTTP server app on **0.0.0.0:8000** returning a small HTML page.
Set the port in `manifest.json` under `backend.upstream`. Teka proxies to this listener and manages its TCP lease through the [SDK runtime](https://github.com/ouster-lidar/teka-sdk/blob/main/crates/teka/teka-runner.md).

## Build and deploy

```sh
teka app deploy
```

## Verify

`curl http://<host>:8000/` → 200 and `Hello world` body.
Check `/app/hello-http` through Teka as well; its **Direct endpoint** link opens the same service on `http://<host>:8000/`.

## App UI

The image ships a Teka app manifest. After install, open **Hello HTTP** in the nav (`/app/hello-http`) or the installed card on `/apps`.

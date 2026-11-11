# hello-http

## Goal

**Python 3** HTTP server app on **0.0.0.0:8000** returning a small HTML page.
Teka proxies directly to this fixed listener and manages its TCP 8000 lease through the [SDK runtime](https://github.com/ouster-lidar/teka-sdk/blob/main/apps/lib/app-runtime/teka-runner.md).

## Build and deploy

```sh
teka app deploy --build
```

## Verify

`curl http://<host>:8000/` → 200 and `Hello world` body.
Check `/app/hello-http` through Teka as well; its **Direct endpoint** link opens the same service on `http://<host>:8000/`.

## App UI

The image ships a Teka app manifest. After install, open **Hello HTTP** in the nav (`/app/hello-http`) or the installed card on `/apps`.

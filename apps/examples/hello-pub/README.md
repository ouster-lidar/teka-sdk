# hello-pub

Demo publisher app: owns an in-process event ring buffer and current aggregate state. Peers and browsers publish, query, and subscribe at `/api/v1/app/hello-pub/…`.

Companion consumer: [hello-sub](../hello-sub/README.md). Together they demonstrate **owner-app topics** (publisher owns state + stream; subscribers reach it via Jetstream), not a shared platform bus.

## Naming

| Scope | Name |
|-------|------|
| Portable image / unit / state | `hello-pub` |
| Manifest id / devres / `APP_ID` | `hello-pub` |
| Rust package / binary | `hello-pub` |
| Source dir | `apps/examples/hello-pub/` |

## Jetstream integration

| Env | Default |
|-----|---------|
| `MC_RUNTIME_URL` | From host / `http://localhost` |
| `APP_ID` | `hello-pub` |

Startup: `declare_app_runtime!("hello-pub")`, activate/bind loopback, serve `/api/v1/…`, `sd_notify(READY=1)`. Manifest `backend.mode: proxy`, `runtime_listen: true`, `health_path: /api/v1/state`, `card_stream_path: /api/v1/cards/stream`. Auth: `ojs-proxy-identity` / `0.1.0`.

## Event schema

```json
{
  "id": 1,
  "at": "2026-08-07T23:00:00.000Z",
  "kind": "manual",
  "message": "hello from UI"
}
```

| Field | Type | Notes |
|-------|------|-------|
| `id` | u64 | Monotonic per process (starts at 1) |
| `at` | string | RFC 3339 UTC |
| `kind` | string | `manual` (API/UI), `random` (generator) |
| `message` | string | Human-readable payload |

Ring buffer retains the most recent **10** events. Older events are dropped.

## State schema

```json
{
  "last_event": { "id": 1, "at": "…", "kind": "manual", "message": "…" },
  "total_published": 1,
  "generator_running": false,
  "generator_interval_secs": 5,
  "summary": "1 event published"
}
```

`last_event` is `null` when nothing has been published yet. `summary` is a short dashboard-card string.

## API

All paths are on the app; browsers and peers use `/api/v1/app/hello-pub/…`.

### `GET /api/v1/state`

Current aggregate state (health probe target).

### `GET /api/v1/events?limit=N`

Recent events, newest first. `limit` defaults to 10, capped at 10.

```json
{ "events": [ /* Event… */ ] }
```

### `POST /api/v1/events`

Publish one event. Body optional:

```json
{ "kind": "manual", "message": "optional text" }
```

When `message` is omitted or empty, the server fills a default that includes the assigned event `id` (e.g. `Manual event #3`). `kind` defaults to `manual` if omitted. Response **201** with the created `Event`.

### `POST /api/v1/generator`

Start/stop the background random generator.

```json
{ "running": true, "interval_secs": 5 }
```

`interval_secs` optional (default **5**, min **1**, max **10**). Changing `running` or `interval_secs` wakes the generator loop immediately (no wait for the previous sleep). Response: current `State`.

### `WS /api/v1/events/stream`

Resource live picture (lock 9a style). On connect and on every publish:

```json
{
  "type": "snapshot",
  "data": {
    "events": [ /* newest first, up to 10 */ ],
    "state": { /* State */ }
  }
}
```

`data` matches the combined picture of `GET /events` + `GET /state`.

### `WS /api/v1/cards/stream`

Dashboard card STATUS/HEARTBEAT via `SummaryStream` (`summary` from state). Card updates follow the events broadcast (including after a lagged subscriber catches up).

## Generator

While `generator_running` is true, a background task publishes a `kind: "random"` event every `generator_interval_secs` with a short random message. Publishing (manual or random) updates the ring buffer, bumps `total_published`, refreshes `last_event` / `summary`, and fans out on the events broadcast (UI stream + card bridge).

## UI

Static panel under `/app/hello-pub`:

- Show `state` (totals, generator status)
- **Publisher URL** for remote subscribers: `{window.location.origin}/api/v1/app/hello-pub` (scheme/host/port the browser used to reach this Jetstream host), with a one-click copy control
- Button to `POST /events` (optional message field)
- Toggle generator and set interval 1–10s (`POST /generator`)
- Live event list via `WS /events/stream` (or refetch on snapshot)

## Build / deploy

```bash
cd apps
nix build .#hello-pub
```

Deploy with `teka app deploy --build hello-pub --host http://HOST/`. Open `/app/hello-pub`. Pair with `hello-sub` for the subscriber demo.

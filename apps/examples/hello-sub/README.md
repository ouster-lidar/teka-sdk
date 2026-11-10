# hello-sub

Demo subscriber app: connects to one or more [hello-pub](https://github.com/ouster-lidar/teka-sdk/blob/main/apps/examples/hello-pub/README.md) publishers through Teka’s feature gateway, merges received events into a local feed (tagged by source), and exposes its own GET/WS for the subscriber UI.

Supports **remote** publishers by pointing at other hosts’ Teka feature URLs (`/api/v1/app/hello-pub`). This demo does **not** perform remote Teka password login; use open-mode / unauthenticated peers for now (password auth may return later).

## Naming

| Scope | Name |
|-------|------|
| Portable image / unit / state | `hello-sub` |
| Manifest id / devres / `APP_ID` | `hello-sub` |
| Rust package / binary | `hello-sub` |

## Teka integration

| Env | Default |
|-----|---------|
| `MC_RUNTIME_URL` | From host / `http://localhost` |
| `APP_ID` | `hello-sub` |
| `MC_HELLO_PUB_APP_ID` | `hello-pub` |
| `MC_HELLO_PUB_URL` | Optional full base override for the publisher feature API. When set, wins over `MC_HELLO_PUB_APP_ID`. Seed for the runtime peer config (one-element list). |

Startup peer URL resolution (initial config):

1. If `{StateDirectory}/peer-config.json` has `publisher_urls` (or legacy `publisher_url`), use that list (last Apply).
2. Else if `MC_HELLO_PUB_URL` is non-empty, use it as a one-element list.
3. Else `[{MC_RUNTIME_URL}/api/v1/app/{MC_HELLO_PUB_APP_ID}]`.

URL normalization (Apply and env/file load), per URL:

- Trim; strip trailing `/`
- If there is no `http://` or `https://` scheme, prepend `http://` (bare host/IP pastes)
- Rewrite `127.0.0.1` → `localhost`
- Drop default ports (`:80`, `:443`, http `:8080`)
- If the path has no `/api/v1/app/…` segment, append `/api/v1/app/hello-pub`
- Deduplicate (first wins)

Operators change the list at runtime via `POST /api/v1/peer-config` (UI). Added URLs start new sessions; removed URLs are aborted; unchanged URLs keep running. An empty list disconnects everything. Persisted to `{StateDirectory}/peer-config.json` as `publisher_urls`.

Startup: `declare_app_runtime!("hello-sub")`, activate/bind, serve `/api/v1/…`. Manifest `backend.mode: proxy`, `runtime_listen: true`, `health_path: /api/v1/status`, `card_stream_path: /api/v1/cards/stream`. Auth: `ojs-proxy-identity` / `0.1.0`.

## Local feed

Each mirrored event keeps hello-pub fields (`id`, `at`, `kind`, `message`) and adds `publisher_url` stamped by this subscriber. Per publisher, the ring retains the most recent **10** events. The merged feed API returns up to **30** events (newest first across all publishers). The UI can filter which publishers’ events are shown.

## Status schema

```json
{
  "publishers": [
    {
      "publisher_url": "http://localhost/api/v1/app/hello-pub",
      "connected": false,
      "last_error": null,
      "peer_state": null,
      "events_received": 0
    }
  ],
  "connected_count": 0,
  "publisher_count": 1,
  "events_received": 0,
  "last_event": null,
  "summary": "Waiting for publishers"
}
```

| Field | Notes |
|-------|-------|
| `publishers[]` | Per-peer connection, error, `peer_state`, and `events_received` |
| `connected_count` / `publisher_count` | Aggregate for the header chip (`N/M connected`) |
| `events_received` | Sum of per-peer totals |
| `last_event` | Newest tagged event across peers |
| `summary` | Short status line for the dashboard card |

## API

### `GET /api/v1/status`

Current subscriber status (health probe).

### `GET /api/v1/feed?limit=N`

Merged mirrored events, newest first. `limit` defaults to 30, capped at 30. Each event includes `publisher_url`.

```json
{ "events": [ /* Event… */ ] }
```

### `GET /api/v1/peer-config`

```json
{
  "publisher_urls": ["http://localhost/api/v1/app/hello-pub"]
}
```

### `POST /api/v1/peer-config`

Replace the publisher list and reconcile sessions.

```json
{
  "publisher_urls": ["http://other-box/api/v1/app/hello-pub"]
}
```

| Field | Notes |
|-------|-------|
| `publisher_urls` | Array of URLs (may be empty). Normalized and deduped. |
| `publisher_url` | Optional convenience; treated as a one-element list when `publisher_urls` is omitted. |

Response **200**: current `Status` after applying config (session changes are asynchronous).

### `POST /api/v1/peer-state`

Fetch publisher `GET /state` now (does not require WS). Body optional:

```json
{ "publisher_url": "http://other-box/api/v1/app/hello-pub" }
```

Omit `publisher_url` (or send `{}`) to refresh **all** configured publishers.

```json
{
  "ok": true,
  "publishers": [
    {
      "publisher_url": "…",
      "ok": true,
      "peer_state": { },
      "error": null
    }
  ],
  "error": null
}
```

### `WS /api/v1/feed/stream`

On connect and whenever the mirrored feed or connection status changes:

```json
{
  "type": "snapshot",
  "data": {
    "events": [ /* … */ ],
    "status": { /* Status */ }
  }
}
```

### `WS /api/v1/cards/stream`

Dashboard STATUS/HEARTBEAT from `status.summary`. Card updates follow the status notify broadcast (including after lag).

## Peer subscribe behavior

1. Read current `publisher_urls` from in-memory config.
2. For each URL, connect to `WS {publisher_url}/events/stream` (no remote login cookie).
3. On snapshot: replace that peer’s ring, stamp `publisher_url` on events, update that peer’s `peer_state` / connected / error, fan out merged feed.
4. On WS close/error: mark that peer disconnected, reconnect with backoff. Config remove aborts that session immediately.

Local loopback peers keep today’s behavior (`kind=service` when enforcement is on). Password-protected remote Teka hosts are out of scope for this demo.

Publisher process restart or Teka proxy blips must not crash the subscriber.

## UI

Static panel under `/app/hello-sub`:

- Header chip: `N/M connected`
- Publishers list: URL, connected chip, generator, last error, **Remove**
- Form: **Add publisher URL** + **Apply & reconnect** (plain text input — not `type=url`)
- Live merged event list (`WS /feed/stream`) with per-event source label
- Feed filter checkboxes (All + per publisher); selection persisted in `localStorage`
- **Refresh peer state** (`POST /peer-state` for all)

## Build / deploy

```bash
cd apps
nix build .#hello-sub
```

Deploy with `teka app deploy --build hello-sub --host http://HOST/` (and `hello-pub`). Open `/app/hello-sub` while hello-pub is running.

Remote example: on the publisher UI, copy the Publisher URL (browser origin + `/api/v1/app/hello-pub`), paste into hello-sub on the other box, Add + Apply.

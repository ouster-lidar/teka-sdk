# Operator-facing copy

Casing and wording for text operators read in app UIs and host app-shell chrome. Wire/API field names stay snake_case; this spec is display copy only.

Related:

- Portable web helpers in [nix/apps/web/](../../nix/apps/web/) (`formatChipLabel`, `formatEnumLabel`, `formatServiceSummary`).

## Layers

| Layer | Rule | Examples |
|-------|------|----------|
| **Summary** | Sentence case; count-led or single phrase | `Disabled`, `2 fallback passive`, `1 leader, 3 followers` |
| **Service header** | `status.summary` when present; else `Disabled`. Fetch failure → `Unavailable`. | Netboot pattern |
| **Meta chip** | Sentence case; never raw wire enums | `Running`, `Link: up`, `Fallback: passive` |
| **Status badge** (`.status`) | Display-formatted; map wire → operator | `RUNNING` → `Running` |
| **Acronym / role** | UPPER when it is a proper name | `CVM`, `CVB`, `DHCP` |
| **Technical id** | As-is | `enP1p1s0f0`, MACs, hostnames, firmware version strings |

Do **not** show API keys (`FALLBACK`, `MANUAL`, `RUNNING`) verbatim in operator UI.

## Sentence case

Capitalize only the first letter when it starts a word. Other words use lowercase unless they are acronyms (see above).

| Pattern | Rule | Examples |
|---------|------|----------|
| Leading count | Digit first; following words lowercase | `4 managed interfaces`, `2 fallback passive` |
| Single phrase | First letter capitalized | `Disabled`, `Running` |
| Label with colon | Capitalize label; lowercase value after colon | `Link: up`, `Fallback: passive`, `Primary: 10.0.0.1` |

Do **not** use title case for mode words (`Fallback Passive`). Prefer `Fallback: passive` or count-led `2 fallback passive`.

## Service headers

`.service-header` uses CSS `text-transform: uppercase` for section chrome. Store **sentence case** in JS; CSS renders it loud.

- Paint `status.summary` from `GET …/status` or stream payload.
- When `summary` is absent and nothing is running, use `Disabled`.
- On status fetch failure, use `Unavailable` (not `Stopped`).

## Meta chips (`app-meta-chip`)

Shared variants in host `app.css`:

| Variant | Meaning | Color | Examples |
|---------|---------|-------|----------|
| `--usage` | On, serving, healthy, capable | Green | `Running`, `Hardware`, `Eligible` |
| `--state` | Mode, FSM, degraded | Amber | `Software`, `Fallback: passive` |
| `--type` | Neutral metadata | Grey | `Link: up`, `Primary: 10.0.0.1` |
| `--ro` | Warning, read-only, invalid | Red-ish | `Read-only`, `Fail` |

Use `window.formatChipLabel()` for chip text. Use `window.formatEnumLabel()` when mapping wire enum tokens to chip or badge text.

Never show a duplicate runtime chip on `/apps` when the runtime switch already signals on/off.

## Status badges (`.status`)

Table and inline badges use the same sentence-case policy. Map wire enums before display:

| Wire | Display |
|------|---------|
| `RUNNING` | `Running` |
| `STANDBY` | `Standby` |
| `ERROR` | `Error` |
| `UNKNOWN` | `Unknown` |
| `FALLBACK` | `Fallback` |
| `MANUAL` | `Manual` |

Use `window.formatEnumLabel()` unless the value is already operator copy from the backend `summary`.

## Exceptions (documented passthrough)

| Source | Policy |
|--------|--------|
| NetworkManager device state | Pass through as returned (`Activated`, `Unavailable`, …) until a dedicated NM display map exists |
| Hostnames, IPs, MACs, serials | Literal |
| Firmware / version strings | Literal |

## Checklist for new apps

- [ ] `summary` uses sentence case and count-led patterns above.
- [ ] Service header uses `formatServiceSummary(status)` (or equivalent).
- [ ] Chips and badges use sentence case; wire enums formatted.
- [ ] No raw `SCREAMING_SNAKE` in operator-visible chip/badge text.

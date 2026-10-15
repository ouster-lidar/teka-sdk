# App naming

Naming layers for `apps/` app image DDIs and host platform integration.

## Host contract

Install the app manifest at host path `usr/lib/jetstream/mission-control/manifest.json`. Web assets install under `usr/lib/jetstream/mission-control/web/`. Systemd portable-service mechanics (`portablectl`, `/var/lib/portables/`, `mission-control.service`) live in [jetstream-os](https://github.com/ouster-lidar/jetstream-os) — this repo builds app images only.

## Layers

| Layer | Pattern | Example |
|-------|---------|---------|
| App id / flake attr | `hello-{topic}` | `hello-http` |
| Source dir | topic | `examples/hello-http/` |
| App guide | `README.md` | `examples/hello-http/README.md` |
| Shared web helper (source) | `apps/web/app-api.js` | — |
| Shared web helper (DDI install) | `usr/lib/jetstream/mission-control/web/app-api.js` | prepended into each app `*.js` at build |
| App panel JS global | `jetstreamAppApi(defaultId)` | not `appApi` (host `window.appApi` in Mission Control shell) |

The Ouster viewers use distinct IDs and roles: `hello-ouster-world` is the direct SDK viewer, and `hello-ouster-dynamo` preserves the advanced FrameSet and tracked-object workflow. See [hello-ouster-viewers.md](hello-ouster-viewers.md).

## Example discovery

Each directory under `examples/` with a `default.nix` is discovered as an app. Cargo lockfile updates find projects from `Cargo.toml`, and app-specific image checks from `tests/image-check.nix`. `simpleCrossServices` in `apps/flake.nix` names the few apps that do not need the heavier cross-build workarounds.

## Distributable artifact vs install identity

Two filename layers — do not conflate them.

| Layer | Pattern | Where |
|-------|---------|-------|
| **Distributable** | `<id>-<system>-v<compose>.raw` | `nix build` `$out`, `#default` bundle |
| **Install / catalog** | `<id>.raw` | `/var/lib/portables/`, live-USB catalog on persist |

- **`<id>`** — app id (`os-release` **`ID=`**, `portablectl` name on the host).
- **`<system>`** — Nix platform (`x86_64-linux`, `aarch64-linux`); cross attrs use the **target**.
- **`<compose>`** — **`${IMAGE_VERSION}+${BUILD_ID}`** from the **apps** flake (`apps/nix/version.nix`). Same *scheme* as appliance `{machine}-liveusb-v{version}`, not the same token (different `self`). **`v`** prefix matches appliance artifacts.

- Catalog / `jetstream-portable-install` copy **to** `<id>.raw`. Install identity is **`ID=`** inside the DDI.
- Replace identity is **SHA-256 of DDI bytes**, not **`VERSION_ID`** and not SemVer order. Newer or older bytes take the same **digest-differ** branch (upgrade and downgrade).
- Mixed-arch / bundle folders: resolve the basename via apps flake **`lib.appArtifactRawName`**. Do not glob `*.raw` there.
- A single app image derivation `$out` contains exactly one `.raw`; deploy may require that (avoids flake-eval vs skip-build skew).
- App manifest **`version`** is the staging path segment; at build time it should match **`IMAGE_VERSION`** from `apps/nix/version.nix`. The host `/apps` list shows stream row **`version`** as DDI compose **`${IMAGE_VERSION}+${BUILD_ID}`**, not manifest `version` alone.

Built `os-release` (same *scheme* as appliance `/etc/os-release`; app image `self`, not host `self`):

| Key | Value |
|-----|--------|
| `ID` | app id (install / `portablectl` name) |
| `ID_LIKE` | `jetstream-portable` |
| `IMAGE_ID` | same as `ID` |
| `IMAGE_VERSION` | SemVer core (`MAJOR.MINOR.PATCH` plus optional pre-release) |
| `VERSION` | same as `IMAGE_VERSION` |
| `VERSION_ID` | `1` — GPU compatibility generation (not SemVer; not the compose string) |
| `BUILD_ID` | `${lastModifiedDate}.g${rev}.n${narHash7}` (`rev` = git shortRev, or `0000000` on path flakes) |
| `PRETTY_NAME` | `<id> portable <IMAGE_VERSION>` |
| `PORTABLE_PREFIXES` | app id |

- Filename **`<compose>`** is **`${IMAGE_VERSION}+${BUILD_ID}`** from `apps/nix/version.nix`. **`JETSTREAM_TEST_VERSION`** overrides the SemVer core when set (flake eval reads env only with **`--impure`**); unset defaults to **`0.1.0-alpha.1`**. Malformed SemVer throws at eval.
- Flake **`checks.<system>.<id>`** (and **`<id>-cross-<system>`**) asserts `$out` basename is **`appArtifactRawName`** and DDI **`etc/os-release`** **`ID`** / **`IMAGE_ID`** / **`IMAGE_VERSION`** / **`VERSION`** / **`BUILD_ID`** match `apps/nix/version.nix`, and **`VERSION_ID=1`**. Derivation names are **`app-image-check-*`**.
- Host GPU **`extension-release.portable-gl-extension`** matches **`ID=jetstream-portable`** via **`ID_LIKE`** and **`VERSION_ID=1`** (same GPU compatibility generation as app images). Bump both sides only for a breaking GPU extension change.

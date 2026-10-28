# App naming

Naming layers for `apps/` app image DDIs and host platform integration.

## Host contract

Install the app manifest at host path `usr/lib/jetstream/mission-control/manifest.json`.
Web assets install under `usr/lib/jetstream/mission-control/web/`.

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

## Package names and identity

App outputs use `<id>` and `<id>-cross-<system>` attributes and contain one signed `.tpk` each.
`default` bundles the native app packages.

Package filenames use `<id>-<system>-v<compose>.tpk` in `nix build` outputs and the `#default` bundle.

- **`<id>`** — app id (`os-release` **`ID=`**).
- **`<system>`** — Nix platform (`x86_64-linux`, `aarch64-linux`); cross attrs use the **target**.
- **`<compose>`** — **`${IMAGE_VERSION}+${BUILD_ID}`** from `apps/nix/version.nix`.

- Replace identity is **`(id, root_hash)`** from the verified package, independent of **`VERSION_ID`** and SemVer order.
- Deploy requires exactly one **`.tpk`** per app output.
- App manifest **`version`** should match the release version **`IMAGE_VERSION`** from `apps/nix/version.nix`.

App image `os-release`:

| Key | Value |
|-----|--------|
| `ID` | app id |
| `ID_LIKE` | `jetstream-portable` |
| `IMAGE_ID` | same as `ID` |
| `IMAGE_VERSION` | SemVer core (`MAJOR.MINOR.PATCH` plus optional pre-release) |
| `VERSION` | same as `IMAGE_VERSION` |
| `VERSION_ID` | `1` — GPU compatibility generation (not SemVer; not the compose string) |
| `BUILD_ID` | `${lastModifiedDate}.g${rev}.n${narHash7}` (`rev` = git shortRev, or `0000000` on path flakes) |
| `PRETTY_NAME` | `<id> portable <IMAGE_VERSION>` |
| `PORTABLE_PREFIXES` | app id |

- Filename **`<compose>`** is **`${IMAGE_VERSION}+${BUILD_ID}`** from `apps/nix/version.nix`. **`JETSTREAM_TEST_VERSION`** overrides the SemVer core when set (flake eval reads env only with **`--impure`**); unset defaults to **`0.1.0-alpha.1`**. Malformed SemVer throws at eval.
- Flake **`checks.<system>.<id>`** (and **`<id>-cross-<system>`**) validates image naming and `os-release` fields against `apps/nix/version.nix`, including **`VERSION_ID=1`**.
- Host GPU **`extension-release.portable-gl-extension`** matches **`ID=jetstream-portable`** via **`ID_LIKE`** and **`VERSION_ID=1`** (same GPU compatibility generation as app images). Bump both sides only for a breaking GPU extension change.

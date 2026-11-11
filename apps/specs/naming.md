# App naming

Naming layers for `apps/` app image DDIs and host platform integration.

## Host contract

Install the app manifest at host path `usr/lib/teka/mission-control/manifest.json`.
Web assets install under `usr/lib/teka/mission-control/web/`.

## Layers

| Layer | Pattern | Example |
|-------|---------|---------|
| App id / registry key | `hello-{topic}` | `teka.apps.hello-http` |
| Source dir | topic | `examples/hello-http/` |
| App guide | `README.md` | `examples/hello-http/README.md` |
| Shared web helper (source) | `nix/apps/web/app-api.js` | — |
| Shared web helper (DDI install) | `usr/lib/teka/mission-control/web/app-api.js` | prepended into each app `*.js` at build |
| App panel JS global | `tekaAppApi(defaultId)` | not `appApi` (host `window.appApi` in Mission Control shell) |

The Ouster viewers use distinct IDs and roles: `hello-ouster-world` is the direct SDK viewer, and `hello-ouster-dynamo` preserves the advanced FrameSet and tracked-object workflow. See [hello-ouster-viewers.md](hello-ouster-viewers.md).

## Example discovery

Each directory under `examples/` with a `default.nix` is discovered as an app. Cargo lockfile updates find projects from `Cargo.toml`, and app-specific image checks from `tests/image-check.nix`. `simpleCrossApps` in `apps/default.nix` names the few apps that do not need the heavier cross-build workarounds.

## Package names and identity

App artifacts live under `teka.apps.<id>.packages.<LinuxBuildSystem>.<LinuxTargetSystem>` and contain one signed `.tpk` each. Native builds use equal systems; cross-builds use distinct systems.

Package filenames use `<id>-<system>-v<compose>.tpk` in each app output.

- **`<id>`** — app id (`os-release` **`ID=`**).
- **`<system>`** — Nix platform (`x86_64-linux`, `aarch64-linux`); the filename uses the **target**.
- **`<compose>`** — **`${IMAGE_VERSION}+${BUILD_ID}`** from `nix/apps/version.nix`.

- Replace identity is **`(id, root_hash)`** from the verified package, independent of **`VERSION_ID`** and SemVer order.
- Deploy requires exactly one **`.tpk`** per app output.
- App manifest **`version`** should match the release version **`IMAGE_VERSION`** from `nix/apps/version.nix`.

App image `os-release`:

| Key | Value |
|-----|--------|
| `ID` | app id |
| `ID_LIKE` | `teka-portable` |
| `IMAGE_ID` | same as `ID` |
| `IMAGE_VERSION` | SemVer core (`MAJOR.MINOR.PATCH` plus optional pre-release) |
| `VERSION` | same as `IMAGE_VERSION` |
| `VERSION_ID` | `1` — GPU compatibility generation (not SemVer; not the compose string) |
| `BUILD_ID` | `${lastModifiedDate}.g${rev}.n${narHash7}` (`rev` = git shortRev, or `0000000` on path flakes) |
| `PRETTY_NAME` | `<id> portable <IMAGE_VERSION>` |
| `PORTABLE_PREFIXES` | app id |

- Filename **`<compose>`** is **`${IMAGE_VERSION}+${BUILD_ID}`** from `nix/apps/version.nix`. **`TEKA_TEST_VERSION`** overrides the SemVer core when set (flake eval reads env only with **`--impure`**); unset defaults to **`0.1.0-alpha.1`**. Malformed SemVer throws at eval.
- Flake **`checks.<system>.<id>`** (and **`<id>-cross-<system>`**) validates image naming and `os-release` fields against `nix/apps/version.nix`, including **`VERSION_ID=1`**.
- Host GPU **`extension-release.portable-gl-extension`** matches **`ID=teka-portable`** via **`ID_LIKE`** and **`VERSION_ID=1`** (same GPU compatibility generation as app images). Bump both sides only for a breaking GPU extension change.

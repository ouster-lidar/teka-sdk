# App naming

Naming layers for `apps/` app image DDIs and host platform integration.

## Host contract

Install the source app manifest at `usr/lib/teka/app/manifest.json`.
Web assets install under `usr/lib/teka/app/web/`.

## Layers

| Layer | Pattern | Example |
|-------|---------|---------|
| App id / registry key | `hello-{topic}` | `teka.apps.hello-http` |
| Source dir | topic | `examples/hello-http/` |
| App guide | `README.md` | `examples/hello-http/README.md` |
| Shared web helper (source) | `nix/packages/teka-app-api-js/app-api.js` | — |
| Shared web helper (DDI install) | `/usr/lib/teka/app/web/app-api.js` | TekaOS loads it before the panel script |
| App panel JS global | `tekaAppApi(defaultId)` | app panels call this helper |

The Ouster viewers use distinct IDs and roles: `hello-ouster-world` is the direct SDK viewer, and `hello-ouster-dynamo` preserves the advanced FrameSet and tracked-object workflow. See [hello-ouster-viewers.md](hello-ouster-viewers.md).

## Example discovery

Each directory under `examples/` with a `package.nix` is discovered as an app. Cargo lockfile updates find projects from `Cargo.toml`, and app-specific image checks from `tests/image-check.nix`.

## Package names and identity

The `.tpk`, `.raw`, and `.roothash` files share the basename `<id>-<system>-v<version>+<buildId>`. The signed TPK component version is `<version>+<buildId>`.

- **`<id>`** — manifest `id` (`os-release` **`ID=`**).
- **`<system>`** — Nix platform (`x86_64-linux`, `aarch64-linux`); the filename uses the **target**.
- **`<version>`** — manifest `version` (`os-release` **`IMAGE_VERSION=`**).
- **`<buildId>`** — `<lastModifiedDate>.g<rev>.n<hash>`. Use the flake's `shortRev`, falling back to `dirtyShortRev` without `-dirty` or to `0000000`. The date defaults to `19700101000000`. The hash is the first seven hex digits of the SHA-256 of the flake's `narHash` string.

- Replace identity is **`(id, root_hash)`** from the verified package, independent of **`VERSION_ID`** and SemVer order.
- Deploy requires exactly one **`.tpk`** per app output.

App image `os-release`:

| Key | Value |
|-----|--------|
| `ID` | app id |
| `ID_LIKE` | `teka-portable` |
| `IMAGE_ID` | same as `ID` |
| `IMAGE_VERSION` | SemVer core (`MAJOR.MINOR.PATCH` plus optional pre-release) |
| `VERSION` | same as `IMAGE_VERSION` |
| `VERSION_ID` | `1` — GPU compatibility generation |
| `BUILD_ID` | `<buildId>` |
| `PRETTY_NAME` | `<id> portable <IMAGE_VERSION>` |
| `PORTABLE_PREFIXES` | app id |

- Flake **`checks.<system>.<id>`** (and **`<id>-cross-<target>`**) validates the image filesystem, copied manifest, primary service file, and `os-release` identity and version fields.
- Host GPU **`extension-release.portable-gl-extension`** matches **`ID=teka-portable`** via **`ID_LIKE`** and **`VERSION_ID=1`** (same GPU compatibility generation as app images). Bump both sides only for a breaking GPU extension change.

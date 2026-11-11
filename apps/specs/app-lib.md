# App build library

The root `flake.nix` exports `lib.mkAppOutputs` for both the example collection and standalone downstream apps. These Nix interfaces are internal; update callers together when changing them.

The shared SDK Cargo package is `teka`, with runtime APIs under `teka::app`, per [Rust crate naming](../../specs/rust-crate-naming.md). The `teka-runner` Nix package provides the standalone runtime executable; `teka-src` supplies SDK sources with workspace metadata materialized.

## Ownership

Keep app-specific build expressions and runtime helpers inside their example.
Shared implementations under `nix/apps/` need multiple actual consumers or a documented public-library purpose; a central import alone does not establish sharing.
Group shared browser, SDK, overlay, and Wayland support by domain.

## `lib` API

| Export | Role |
|--------|------|
| `mkAppOutputs` | App registry with signed `.tpk` artifacts and image/Cargo `checks` |
| `mkVersion` | `version.nix` — `IMAGE_VERSION`, `BUILD_ID`, `appArtifactRawName` |
| `paths` | Paths to shared Nix expressions (`erofs-mkfs`, `core-version-validate`) |

## Composition

```nix
teka-sdk.lib.mkAppOutputs {
  inherit self;
  apps.my-app = import ./.;
  advisoryDb = teka-sdk.inputs.rustsecAdvisoryDb;
}
```

`apps` contains app definitions with an `image` function. `self` provides version metadata. Optional `systems`, `version`, `overlays`, `ousterSdkSrc`, and `tekaSrc` configure actual build inputs. `advisoryDb` enables Cargo audit checks; omitting it disables them.

`systems` defaults to `x86_64-linux` and `aarch64-linux`; only those Linux systems are supported. `mkAppOutputs` exports artifacts under `teka.apps.<id>.packages.<LinuxBuildSystem>.<LinuxTargetSystem>` and checks under `checks.<LinuxBuildSystem>`. App artifacts stay in this registry; root `packages` remains dedicated to SDK and CLI outputs.

The root `packages.<system>.teka-src` output materializes workspace metadata for standalone SDK consumers. Pass a source path or source derivation as `tekaSrc` to override it. The runner and app binaries use target packages and their Crane library; source assembly uses `pkgs.buildPackages`, and TPK signing runs the build-platform CLI. macOS delegates app image builds to Linux builders.

## App-specific image checks

`mkAppOutputs` accepts `imageChecks`, an attrset keyed by service ID.
Each value is a function receiving `{ pkgs, imageVersion }` and returning a shell snippet added to that service's native and cross image checks.
The snippet can inspect the image at `$raw`; generic filesystem, unit, and `os-release` validation remains shared.

`mkAppOutputs` accepts `simpleCrossApps` for apps that do not need the additional cross-build workarounds; it defaults to an empty list. `apps/default.nix` keeps this small exception list explicit.

## Host contract

Install app manifests and web assets at host paths under `usr/lib/teka/mission-control/…`.

## Runtime nftables leases

- Apps request generic native nftables resources through the fallible `sync_nftables` SDK helper after devres activation and before work that depends on those leases or readiness.
- `PUT /api/v1/manager/runtime/{id}/nftables` replaces the app's complete nftables lease list with `{ "leases": [...] }`; an empty list releases nftables leases while preserving ports and the HTTP allocation.
- Each lease contains `id`, `hook`, `priority`, `sets`, and `rules`. Sets contain local `name`, `type`, `flags`, and optional native nftables JSON `timeout`, `gc-interval`, and `size`; rules are arrays of native nftables JSON statements referencing only lease-local sets.
- Teka validates requests and assigns table, chain, and set ownership. Resources are holder- and lease-local, with no shared named policies. Apps supply packet policy without invoking firewall tools or selecting host-defined policy IDs.
- Compatible updates preserve existing set elements and attached state. Compatibility includes the set schema, writer key expression, and attached stateful statement configuration. Incompatible changes atomically reset only affected sets; conflicting writers to one set are rejected without changing accepted resources.
- The SDK caches only successfully accepted nftables requests. Rejected requests preserve the accepted replay state; reconnect reactivates devres and replays the current accepted nftables leases before ports, retrying failures without replaying stale snapshots.
- A disconnected registration stream or unreachable control plane never triggers SDK release. Transient reconnect preserves installed kernel state; actual Teka restart cleanup resets ephemeral state, and replay recreates the accepted resources.
- The SDK exposes resource-loss and restored notifications. Notify apps whenever the control-plane stream is lost, even transiently; this reports uncertain resource availability without assuming kernel resources are absent. Retain accepted lease caches throughout recovery.
- Running apps that depend on leases pause dependent work until all accepted resource replay succeeds, or exit if they cannot pause. The SDK retries and reports replay failures; failed nftables replay prevents port replay and restored notification. Notify restored only after the full accepted replay succeeds.
- Acquisition, reconnect replay, and final release are serialized. Final release prevents later reactivation or replay and releases all runtime resources.
- SDK lifecycle tests cover rejected updates, transient reconnect preserving state, resource-loss and restored notifications during failed replay and recovery, replay order and retry, current accepted state during concurrent updates, and final-release races. Kernel behavior is verified separately in disposable host VM tests.

## Flake input `follows`

The root flake pins Nixpkgs, Crane, and Jetpack, with Jetpack following the same Nixpkgs pin. `apps/default.nix` selects the Jetpack overlays for the example collection. Standalone examples consume the root SDK flake.

## Downstream compatibility

Commit and merge `lib` / app-api / SDK changes in **this repo** before downstreams bump their flake input. Consumers record the pinned git **commit** in `flake.lock`—not a release tag.

## Change checklist

1. Commit and merge shared nix / rust / app API JS changes in this repo.
2. Downstream bumps its `example-apps` input rev when the exported API changes.

## Rust app builds

App definitions receive crate, web, and manifest helpers through `ps.appCrate`:

```nix
app.buildAppCrate (
  (app.prepareAppSource {
    crateName = "my-app";
    src = app.appRustSrc { srcDir = ./.; };
  })
  // { meta.mainProgram = "my-app"; }
)
```

`prepareAppSource` returns `src`, `dummySrc`, `cargoRoot`, `cargoToml`, and `cargoLock`. It assembles both source trees with the same `apps/examples/<crateName>` and `crates/teka` layout so relative Cargo dependencies resolve consistently, including SDK overrides. Manifest and lockfile paths refer to the app source so reading build metadata does not require assembling the tree. For generated app sources, pass an evaluation-readable `cargoSrc` with matching manifests, lockfile and target layout. Generated SDK sources can expose a matching `dummySrc` passthru; the default `teka-src` provides this to avoid building sources during evaluation. `buildAppCrate` consumes those explicit inputs; it does not assemble sources. Optional `nativeBuildInputs`, `buildInputs`, and `meta` apply to the package; dependency builds receive the same build inputs. Cargo lockfiles drive vendoring, dependency artifacts are cached separately, and native builds run Cargo tests. Cross builds compile without executing target tests.

Put build-platform tools in `nativeBuildInputs` and target libraries in `buildInputs`. Cargo provides crate names and caps third-party dependency lints; local Rust warnings remain errors. Run `nix run .#update-cargo-lock` from the repository root when updating lockfiles.

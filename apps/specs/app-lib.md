# App lib flake exports

Stable `lib` and `packages` outputs from `apps/flake.nix` for downstream flake consumers.

## Ownership

Keep app-specific build expressions and runtime helpers inside their example.
Shared implementations under `apps/nix/` need multiple actual consumers or a documented public-library purpose; a central import alone does not establish sharing.
Group shared browser, SDK, overlay, and Wayland support by domain.

## `lib` API

| Export | Role |
|--------|------|
| `mkApp` | App image builder context (`ps` in `default.nix`) |
| `mkImageBuilder` | DDI image staging (`makeAppImage`, …) |
| `flakeOutputs` | Full `packages` + `checks` for a `services` attrset |
| `mkVersion` | `version.nix` — `IMAGE_VERSION`, `BUILD_ID`, `appArtifactRawName` |
| `appCrate` | `app-crate.nix` — crate/web/manifest helpers (also on `ps.appCrate`) |
| `paths` | Stable paths to shared nix (`erofs-mkfs`, `core-version-validate`) |
| `appArtifactRawName` | `<id>-<system>-v<compose>.raw` basename helper |

## Shared infra `packages.<system>.*`

| Attr | Purpose |
|------|---------|
| `app-api-js` | Web helper prepended into app `*.js` at image build |
| `app-runtime-src` | Source tree for app-runtime path dependency overrides |
| `app-runtime` | Standalone build of the runtime crate and its tests |

Resolve attrs via `nix flake show`.

## `exampleAppsPkgsFor`

`flakeOutputs` accepts `exampleAppsPkgsFor` so consumers can supply shared infra packages from their pinned input:

```nix
exampleAppsPkgsFor = _: {
  app-api-js = example-apps + "/web/app-api.js";
  app-runtime-src = example-apps + "/lib/app-runtime";
};
```

Those are source paths, not `packages.${system}` derivations. App `default.nix` uses `ps.appCrate` (wired inside `mkApp`).

## Host contract

Install app manifests and web assets at host paths under `usr/lib/jetstream/mission-control/…`. Unit files reference `mission-control.service`; portable host checks use `jetstream-portable-check-*` and `ID_LIKE=jetstream-portable` in `os-release`. See jetstream-os for portable-service mechanics.

## Runtime nftables leases

- Apps request generic native nftables resources through the fallible `sync_nftables` SDK helper after devres activation and before work that depends on those leases or readiness.
- `PUT /api/v1/manager/runtime/{id}/nftables` replaces the app's complete nftables lease list with `{ "leases": [...] }`; an empty list releases nftables leases while preserving ports and the HTTP allocation.
- Each lease contains `id`, `hook`, `priority`, `sets`, and `rules`. Sets contain local `name`, `type`, `flags`, and optional native nftables JSON `timeout`, `gc-interval`, and `size`; rules are arrays of native nftables JSON statements referencing only lease-local sets.
- Jetstream validates requests and assigns table, chain, and set ownership. Resources are holder- and lease-local, with no shared named policies. Apps supply packet policy without invoking firewall tools or selecting host-defined policy IDs.
- Compatible updates preserve existing set elements and attached state. Compatibility includes the set schema, writer key expression, and attached stateful statement configuration. Incompatible changes atomically reset only affected sets; conflicting writers to one set are rejected without changing accepted resources.
- The SDK caches only successfully accepted nftables requests. Rejected requests preserve the accepted replay state; reconnect reactivates devres and replays the current accepted nftables leases before ports, retrying failures without replaying stale snapshots.
- A disconnected registration stream or unreachable control plane never triggers SDK release. Transient reconnect preserves installed kernel state; actual Jetstream restart cleanup resets ephemeral state, and replay recreates the accepted resources.
- The SDK exposes resource-loss and restored notifications. Notify apps whenever the control-plane stream is lost, even transiently; this reports uncertain resource availability without assuming kernel resources are absent. Retain accepted lease caches throughout recovery.
- Running apps that depend on leases pause dependent work until all accepted resource replay succeeds, or exit if they cannot pause. The SDK retries and reports replay failures; failed nftables replay prevents port replay and restored notification. Notify restored only after the full accepted replay succeeds.
- Acquisition, reconnect replay, and final release are serialized. Final release prevents later reactivation or replay and releases all runtime resources.
- SDK lifecycle tests cover rejected updates, transient reconnect preserving state, resource-loss and restored notifications during failed replay and recovery, replay order and retry, current accepted state during concurrent updates, and final-release races. Kernel behavior is verified separately in disposable host VM tests.

## Flake input `follows`

This flake pins `nixpkgs`, `crane`, and `jetpack`. Downstream flakes should use `inputs.<this-flake>.inputs.*.follows` so app builds inherit the consumer's toolchain pins.

## Downstream compatibility

Commit and merge `lib` / app-api / app-runtime changes in **this repo** before downstreams bump their flake input. Consumers record the pinned git **commit** in `flake.lock`—not a release tag.

## Change checklist

1. Commit and merge shared nix / rust / app API JS changes in this repo.
2. Downstream bumps its `example-apps` input rev when the exported API changes.

## Rust app builds

`flakeOutputs` requires the `crane` input. Direct `mkApp` and `appCrate` callers supply `craneLib = crane.mkLib pkgs` using the native or cross package set. Follow the apps flake's `crane` input downstream to share its pin.

Use `ps.appCrate.buildAppCrate { crateName = "my-app"; filteredSrc = ps.appCrate.appRustSrc { srcDir = ./.; }; }`. Optional `nativeBuildInputs`, `buildInputs`, and `meta` apply to the package; dependency builds receive the same build inputs. Cargo lockfiles drive vendoring, dependency artifacts are cached separately, and native builds run Cargo tests. Cross builds compile without executing target tests. Sources retain the `examples/<crateName>` and `lib/app-runtime` layout, with `appRuntimeSrc` supplying the shared runtime (including downstream overrides).

Put build-platform tools in `nativeBuildInputs` and target libraries in `buildInputs`. Cargo provides crate names and caps third-party dependency lints; local Rust warnings remain errors. Run `nix run .#update-cargo-lock` from the apps directory when updating lockfiles.

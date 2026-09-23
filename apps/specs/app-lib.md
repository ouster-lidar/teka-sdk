# App lib flake exports

Stable `lib` and `packages` outputs from `apps/flake.nix` for downstream flake consumers.

## `lib` API

| Export | Role |
|--------|------|
| `mkApp` | App image builder context (`ps` in `default.nix`) |
| `mkImageBuilder` | DDI image staging (`makeAppImage`, …) |
| `flakeOutputs` | Full `packages` + `checks` for a `services` attrset |
| `mkVersion` | `version.nix` — `IMAGE_VERSION`, `BUILD_ID`, `appArtifactRawName` |
| `appCrate` | `app-crate.nix` — crate/web/manifest helpers (also on `ps.appCrate`) |
| `paths` | Stable paths to shared nix (`erofs-mkfs`, `cross-crate2nix`, …) |
| `appArtifactRawName` | `<id>-<system>-v<compose>.raw` basename helper |

## Shared infra `packages.<system>.*`

| Attr | Purpose |
|------|---------|
| `app-api-js` | Web helper prepended into app `*.js` at image build |
| `app-runtime-src` | Source tree for crate2nix app-runtime path-dep overrides |
| `app-runtime` | Standalone build of the runtime crate (verification / non-crate2nix) |

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

## Flake input `follows`

This flake pins `nixpkgs`, `crate2nix`, and `jetpack`. Downstream flakes should use `inputs.<this-flake>.inputs.*.follows` so app builds inherit the consumer's toolchain pins.

## Release tags

Tag shared-infra releases when the app API JS helper or builder API changes:

```
app-lib-v0.1.0
```

Document the minimum compatible tag in downstream consumer specs when the API changes.

## Change checklist

1. Land shared nix / rust / app API JS changes here first.
2. Tag if the app API JS or `lib` API changed.
3. Notify downstream consumers to bump their flake input rev.

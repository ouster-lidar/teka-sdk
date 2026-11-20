# Nix app API

This specification defines the Nix interfaces that downstream consumers of the Teka SDK can rely on, including app recipes, flake outputs, and SDK exports.

## Register an app

In `flake.nix`:

```nix
teka-sdk.lib.mkAppOutputs {
  inherit self;
  apps = [ ./package.nix ];
}
```

Each recipe needs a sibling `manifest.json` with `id` and `version`. The SDK copies it unchanged to `/usr/lib/teka/app/manifest.json`. IDs must be unique, match `[a-z0-9][a-z0-9_-]{0,63}`, and differ from the reserved alias `default`. Versions use `MAJOR.MINOR.PATCH` with an optional prerelease, up to 64 characters. See [artifact naming](../apps/specs/naming.md).

| Optional argument | Effect |
| --- | --- |
| `nixpkgs` | Selects the app package set. Defaults to the SDK pin. The packaging CLI always uses the SDK pin. |
| `overlays` | Applied before the SDK’s Wayland and cross overlays. |
| `systems` | Nonempty list of distinct builders and targets. Defaults to `x86_64-linux` and `aarch64-linux`, the two supported systems. |
| `audit` | When `true`, exposes `tests.audit` on Rust builds using the SDK’s advisory database. Add it to flake `checks` explicitly. |

Use an attribute set with `package` to customize an app entry. Set `modules` to override recipe settings or `imageCheck = { pkgs, imageVersion }: ...` to run a shell snippet against native and cross images. The snippet receives build-platform packages and can read the image at `$raw` and the root filesystem byte offset at `$root_offset`.

## Write a recipe

The SDK calls `package.nix` with the target package scope and the app-specific `teka` argument.

```nix
{ pkgs, teka }:
teka.lib.mkAppImage {
  services.main.program = pkgs.hello;
  services.main.systemd.serviceConfig.Type = "oneshot";
}
```

`mkAppImage` accepts a module attrset or a function with `config`, `lib`, `pkgs`, and `teka`. Include additional modules in `imports`. Set overridable recipe defaults with `lib.mkDefault`. To build a separate image with additional modules, use `package.image.override { modules = [ ./configuration.nix ]; }`.

### Services

The SDK generates `<id>.service` for `services.main`, adds it to `teka-apps.target`, and defaults each service’s `APP_ID` to `teka.appId`. Companion roles generate `<id>-<role>.service`. Use their `unitName` in dependencies and start them through explicit dependencies or `systemd.wantedBy`. Choose app IDs and roles that produce unique unit names on the host.

Supply `program` to default `ExecStart` to the package’s main executable, or set `systemd.script` or `systemd.serviceConfig.ExecStart`. Portable units use NixOS unit and service settings. Host installation options such as `aliases` and switch-time options such as `restartIfChanged` are unavailable.

| Runtime option | Effect |
| --- | --- |
| `runtime.managed` | Adds `After=` and `Wants=` for `teka-manager.target`. |
| `runtime.notify` | Defaults to `runtime.managed`. Sets overridable defaults of `Type=notify` and `NotifyAccess=main`. |
| `runtime.network` | `"loopback"` limits traffic to host loopback, `"host"` allows host networking, and `"private"` creates a private namespace. Defaults to `"loopback"` for managed services and `null` otherwise. |
| `runtime.gpu` | Selects `"cuda"` or `"wayland"`. Use the Wayland profile on at most one service per image. |

### Image contents

`image.packages` lists package outputs to copy into the image root in order, following symlinks. Later packages can replace earlier files and BusyBox applets. SDK-managed paths are reserved. The image includes the store closure referenced by the assembled files and services.

Publish assets at `${teka.appDir}/web`, where `teka.appDir` is the image-relative `usr/lib/teka/app`. Include `pkgs.teka-app-api-js` in `image.packages` to publish the SDK's `app-api.js`.

Static web apps still need a primary service. Use `Type = "oneshot"; ExecStart = "/bin/true"; RemainAfterExit = true;` in `services.main.systemd.serviceConfig`.

### Recipe helpers

- `teka.lib.buildRustApp` takes exactly one of `srcDir` (a crate directory) and `src` (a source tree with evaluation-readable Cargo metadata and lockfile). It accepts standard `nativeBuildInputs`, `buildInputs`, and `meta`. A local dependency on the bundled SDK uses `path = "../../../crates/teka"`. The result exposes `tests.clippy`, `tests.fmt`, and optional `tests.audit` for inclusion in flake checks.
- `teka.lib.tekaRunner` supplies the runner for the app target.
- `teka.lib.wayland` provides `dbus`, `fontsConf`, `mesaLlvmPreload`, and `rootPackage`. The Wayland profile adds default files, loads `/etc/teka/wlroots.env`, and runs host checks. Use `hostEtcBindPaths` for host `/etc` binds. The `mkShellEnv { clientPath, clientLd }` snippet puts client tools first on `PATH` and host libraries first on `LD_LIBRARY_PATH`.

## Outputs

| Path | Value |
| --- | --- |
| `packages.<builder>.<id>` | Signed TPK for AArch64 when enabled, otherwise x86_64. |
| `packages.<builder>.<id>.targets.<target>` | Signed TPK for the selected target. |
| `packages.<builder>.default` | Alias for the sole app in a single-app flake. |
| `teka.apps.<id>.path` | Recipe directory relative to the flake root, `"."` at the root, or `null` outside the flake. |
| `teka.lib.linkFarm` | Returns the selected Nixpkgs `linkFarm` for a builder system so the CLI can keep artifacts rooted and accessible by name. |
| `checks.<builder>.<id>`, `checks.<builder>.<id>-cross-<target>` | Native and cross image checks. Names must be unique within the builder. |

In a standalone single-app flake, `nix build` selects its app. The SDK flake defaults to the CLI. Use `nix build .#hello-world.targets.x86_64-linux` to select a target. When AArch64 is enabled, `packages.aarch64-darwin` also exposes the app packages built by `aarch64-linux`. See [cross-builds](../apps/specs/cross-build.md) for builder selection.

The package exposes `pname` and `version` from the manifest, `image` for the signed `.raw` and its `.roothash`, and `tpkFileName` for the path `"${package}/${package.tpkFileName}"`.

## SDK exports

- `lib.certs.app.dev` is the SDK development app certificate in PEM form.
- `lib.mkErofsImageScript { erofs-utils, output, sourceDir, extraArgs ? [ ] }` returns shell code to create and verify an EROFS filesystem. Pass build-platform tools, shell expressions for `output` and `sourceDir`, and literal arguments in `extraArgs`.
- `packages.<system>.teka-src` supplies the Rust SDK source with Cargo workspace metadata materialized.
- `overlays.default` provides `pkgs.teka`. Use `pkgs.buildPackages.teka` to run the CLI during a cross-build.

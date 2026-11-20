# Nix

This repository uses Nix as the **build system** for app images and assumes a multi-user Nix install.

## Install

Install the latest teka release:

```sh
curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | sh
```

The installer sets up the CLI and Nix if needed. It runs `teka build init`. On Arm Linux, the installer isn't available. From a checkout of this repository, run `./bootstrap.sh` instead.

See the [CLI installation guide](../crates/teka-cli/README.md#install) for other options.

Existing Nix installations need `nix-command` and `flakes` enabled before teka can build.
For declaratively managed installations, enable them through your system's Nix options.

## Verify

Check local prerequisites without fetching inputs, building, or changing configuration:

```sh
teka build check
```

To also compile and validate an AArch64 Linux test executable:

```sh
teka build check --full
```

On Apple Silicon macOS, `teka build init` prepares a Linux build VM when no Linux builder is configured.
See [macOS build VM](../crates/teka-cli/README.md#macos-build-vm) for setup and troubleshooting.

See [Nix cache](../specs/nix-cache.md) for cache configuration, publishing and release retention.

## Flake outputs and naming

The root Cargo workspace contains `teka` and `teka-cli`. Rust examples remain independent Cargo packages with their own lockfiles.

| Entry point | Responsibility |
|-------------|----------------|
| Root `flake.nix` | Dependency pins, SDK and CLI packages, app registry, templates and checks |
| `nix/default.nix` | Workspace packages, development shells and CLI checks |
| `apps/default.nix` | Example collection, Jetpack overlays and collection checks |
| Example `flake.nix` | Standalone downstream SDK consumer |

Workspace package definitions live under `nix/packages/`, reusable app builders under `nix/apps/`, and CLI checks under `nix/tests/`. App definitions live under `apps/examples/`, with collection checks under `apps/tests/`. Both the collection and standalone templates use `teka-sdk.lib.mkAppOutputs`. See the [Nix app API](../specs/nix-api.md) for recipes and outputs.

App artifacts live at `packages.<LinuxBuildSystem>.<id>.targets.<LinuxTargetSystem>`. Run the following commands from the repository root.

**Cross-build** (x86_64-linux host → aarch64-linux box):

```bash
teka app build --arch aarch64 hello-world
```

The equivalent explicit Nix build is:

```bash
nix build .#packages.x86_64-linux.hello-world.targets.aarch64-linux
```

On macOS, `teka app build` selects a Linux build system and uses a configured Linux builder.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Missing Nix features | Run `teka build init` for an installed CLI, or enable `nix-command flakes` in Nix configuration before bootstrapping. |
| Slow builds after cache miss | Run `teka build init` to configure public caches; changed or uncached dependencies still need building. |

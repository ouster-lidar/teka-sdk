# Nix

This repository uses Nix as the **build system** for app images and assumes a multi-user Nix install.

## Install

From a source checkout, run `./bootstrap.sh` at the repository root.
It installs Nix interactively if needed, checks its configuration and store, builds the portable CLI, and runs `teka build init` to install it.
For other installation methods, see the [CLI installation guide](../crates/teka-cli/README.md#install).

Existing Nix installations need `nix-command` and `flakes` enabled before bootstrap can build the CLI.
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

## Flake outputs and naming

The root Cargo workspace contains `teka` and `teka-cli`. Rust examples remain independent Cargo packages with their own lockfiles.

| Entry point | Responsibility |
|-------------|----------------|
| Root `flake.nix` | Dependency pins, SDK and CLI packages, app registry, templates and checks |
| `nix/default.nix` | Workspace packages, development shells and CLI checks |
| `apps/default.nix` | Example collection, Jetpack overlays and collection checks |
| Example `flake.nix` | Standalone downstream SDK consumer |

Workspace package definitions live under `nix/packages/`, reusable app builders under `nix/apps/`, and CLI checks under `nix/tests/`. App definitions live under `apps/examples/`, with collection checks under `apps/tests/`. Both the collection and standalone templates use `teka-sdk.lib.mkAppOutputs`. See the [app build library](../apps/specs/app-lib.md) for composition and Cargo source preparation.

App artifacts live at `teka.apps.<id>.packages.<LinuxBuildSystem>.<LinuxTargetSystem>`. Root `packages` contains SDK and CLI outputs; app builds use the registry directly. Run the following commands from the repository root.

**Cross-build** (x86_64-linux host → aarch64-linux device):

```bash
teka app build --arch aarch64 hello-world
```

The equivalent explicit Nix build is:

```bash
nix build .#teka.apps.hello-world.packages.x86_64-linux.aarch64-linux
```

On macOS, `teka app build` selects a Linux build system and uses a configured Linux builder.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Missing Nix features | Run `teka build init` for an installed CLI, or enable `nix-command flakes` in Nix configuration before bootstrapping. |
| Slow builds after cache miss | Expected without extra binary caches; add substituters to `/etc/nix/nix.custom.conf` if needed. |

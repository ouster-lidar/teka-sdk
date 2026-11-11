# Nix

This repository uses Nix as the **build system** for app images and assumes a multi-user Nix install.

## Install

Install Nix using the [Determinate Nix documentation](https://docs.determinate.systems/determinate-nix/):

```bash
curl -fsSL https://install.determinate.systems/nix | sh -s -- install
```

Determinate Nix handles configuration for you, including flakes. See [Configuring Determinate Nix](https://docs.determinate.systems/determinate-nix/#configuration) for `/etc/nix/nix.conf` and `/etc/nix/nix.custom.conf`.

## Verify

From any directory, test that the `nix` CLI can download and run a Nix package:

```bash
$ nix run nixpkgs#hello
Hello, world!
```

From the repository root (`teka-sdk/`), list the example apps:

```bash
$ teka app list
```

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
$ teka app build --arch aarch64 hello-world
```

The equivalent explicit Nix build is:

```bash
$ nix build .#teka.apps.hello-world.packages.x86_64-linux.aarch64-linux
```

On macOS, `teka app build` selects a Linux build system and uses a configured Linux builder.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Slow builds after cache miss | Expected without extra binary caches; add substituters to `/etc/nix/nix.custom.conf` if needed. |

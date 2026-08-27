# Nix

This repository uses Nix as the **build system** for portable app images and assumes a multi-user Nix install.

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

From `apps/` (repository path `jetstream-example-apps/apps`), read `flake.nix` and list the example apps that can be built:

```bash
$ nix flake show
```

## Flake outputs and naming

All build commands run from `apps/` (flake: `apps/flake.nix`).

**Cross-build** (x86_64-linux host → aarch64-linux device):

```bash
$ nix build .#hello-world-cross-aarch64-linux
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Slow builds after cache miss | Expected without extra binary caches; add substituters to `/etc/nix/nix.custom.conf` if needed. |

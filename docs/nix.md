# Nix

This repo uses Nix as the **build system** for portable app images. **Running** an image requires a Jetstream host (for example a ZedBox with Mission Control).

Install Nix with flakes enabled: [https://nixos.org/download/](https://nixos.org/download/)

All commands below run from `apps/` (flake: `apps/flake.nix`).

## Minimal nix.conf

Append to `/etc/nix/nix.conf` or `~/.config/nix/nix.conf`:

```
extra-experimental-features = nix-command flakes

substituters = https://cache.nixos.org/ https://nix-community.cachix.org
trusted-public-keys = cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY= nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=
```

## Flake outputs

```bash
cd apps
nix flake show --all-systems
nix build .#hello-world
nix build .#hello-ouster-world-cross-aarch64-linux   # cross example
```

Deploy on Jetstream:

```bash
./scripts/jsd-app-deploy.sh hello-ouster-world http://HOST/
```

See [README](../README.md) for app list and deploy notes.

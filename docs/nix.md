# Nix

This repository uses Nix as the **build system** for portable app images and assumes a multi-user Nix install.

## Install

Follow the [official Nix install guide](https://nixos.org/download/) and choose a **multi-user** install.

Alternative one-line installer (same multi-user daemon install):

```bash
curl --proto '=https' --tlsv1.2 -L https://nixos.org/nix/install | sh -s -- --daemon
```

## nix.conf

Enable flakes and add the nix-community cache. **Append** these lines to `/etc/nix/nix.conf` (do not replace the whole file):

```ini
extra-experimental-features = nix-command flakes

extra-substituters = https://nix-community.cachix.org
extra-trusted-public-keys = nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=
```

Restart the Nix daemon after editing `nix.conf`:

```bash
sudo systemctl restart nix-daemon
```

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
| `experimental feature 'flakes' is disabled` | Review `nix.conf` configuration above. |
| Untrusted substituters / missing binary cache | Review `nix.conf` configuration above. |
| Slow builds after cache miss | Review `nix.conf` configuration above. |

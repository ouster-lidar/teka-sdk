# Nix

This repository uses Nix as the **build system** for app images and assumes a multi-user Nix install.

## Install

New users get Nix from [Set Up a Development Host](../03_tekaos/02_set-up-a-development-host/00_README.md). The teka installer installs Determinate Nix when `nix` is not already on `PATH`.

Determinate Nix handles configuration for you, including flakes. See [Configuring Determinate Nix](https://docs.determinate.systems/determinate-nix/#configuration) for `/etc/nix/nix.conf` and `/etc/nix/nix.custom.conf`. On Apple silicon, the installer runs `teka build init`, which registers the Teka Linux build VM and configures public Nix caches. Do not turn Determinate Nix's Native Linux Builder back on.

## Verify

Check that your computer can build apps, without fetching inputs or changing configuration:

```bash
teka build check
```

To also compile a small AArch64 Linux test program:

```bash
teka build check --full
```

## Flake outputs and naming

An app's packages live at `packages.<build-system>.<id>.targets.<target-system>`. `teka app build` picks the systems for you. From the repository root, build hello-world for an aarch64 box:

```bash
teka app build hello-world --arch aarch64
```

The same build with plain Nix, from an x86_64 Linux host:

```bash
nix build .#packages.x86_64-linux.hello-world.targets.aarch64-linux
```

On a Mac, `teka app build` uses an aarch64 Linux build system on the Linux build VM.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Missing Nix features | Run `teka build init`. |
| Slow builds after a cache miss | Run `teka build init` to configure public caches. Changed or uncached dependencies still need building. |

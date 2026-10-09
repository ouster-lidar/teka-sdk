# Teka CLI

Build app images and deploy them to Teka devices.

## Install

### Shell only (Nix)

Run `nix develop` at the repository root to put `teka` on `PATH` without installing anything.

### Persistent install

From the repository root, run:

```sh
./bootstrap.sh
```

It builds with Nix (or Cargo) and installs to `~/.local/bin/teka` if that is on `PATH`, else `/usr/local/bin/teka` via `sudo`.

Verify with `teka --version`. Rerun the bootstrap after updating your checkout. Remove the binary from the install path it reported to uninstall.

### NixOS / Home Manager

Add the repository-root flake as an input, following your nixpkgs:

```nix
inputs.teka = {
  url = "git+ssh://git@<host>/<path>/jetstream-example-apps";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then apply `overlays.default` and install `pkgs.teka`. Use `overlays.teka` instead to pick overlays individually.

```nix
nixpkgs.overlays = [ inputs.teka.overlays.default ];
environment.systemPackages = [ pkgs.teka ]; # Home Manager: home.packages
```

The package is dynamically linked and prebuilt per system; `nix run` also works via `apps.<system>.teka`. The `tools/teka` flake's `portable` package is used for bootstrap installs.

## Use

From the repository root or `apps/`:

```sh
teka app list
teka app build hello-world --arch aarch64
teka app deploy --build hello-world --host http://device.local
```

`build` links `./result/<app>` at the requested images and prints those paths. `deploy` uploads from `./result`; `deploy --build` selects the device architecture automatically.

App builds use `apps/flake.nix` at the root of the enclosing git checkout; it must export `teka.apps`. Outside the checkout, use `--flake /path/to/apps` or set `TEKA_APPS_FLAKE_ROOT` to that directory.

Set `TEKA_PASSWORD` to the device administrator password. Use `TEKA_BOOTSTRAP_TOKEN` when initial setup requires a token. Host URLs accept HTTP and HTTPS; `TEKA_HOST` supplies a default for `--host`.

## Develop

For Rust changes under `tools/teka/`:

```sh
cd tools/teka
nix flake check
nix develop
cargo run -p ouster-teka -- --help
```

Agent implementation requirements live in [specs/teka-cli.md](specs/teka-cli.md). See the [developer guide](../../docs/developer-guide.md) for app workflows.

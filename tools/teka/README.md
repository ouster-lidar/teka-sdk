# Teka CLI

Build app images and deploy them to Teka devices.

## Install

### Shell only (Nix)

Run `nix develop` at the repository root or in `apps/` to put `teka` on `PATH` without installing anything.

### Persistent install

From the repository root, run:

```sh
./bootstrap.sh
```

It builds with Nix (or Cargo) and installs to `~/.local/bin/teka` if that is on `PATH`, else `/usr/local/bin/teka` via `sudo`.

Nix builds include the UKI verifier.
Cargo installs require `sbverify` from `sbsigntool` on Linux, or `brew install osslsigncode` on Darwin.

Verify with `teka --version`. Rerun the bootstrap after updating your checkout. Remove the binary from the install path it reported to uninstall.

### NixOS / Home Manager

Add the repository-root flake as an input, following your nixpkgs:

```nix
inputs.teka = {
  url = "git+ssh://git@<host>/<path>/jetstream-example-apps";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then apply `overlays.default` and install `pkgs.teka`:

```nix
nixpkgs.overlays = [ inputs.teka.overlays.default ];
environment.systemPackages = [ pkgs.teka ]; # Home Manager: home.packages
```

The package is dynamically linked and prebuilt per system; `nix run` also works via `apps.<system>.teka`. The `tools/teka` flake's `portable` package is used for bootstrap installs.

## Use

From the repository root or `apps/`:

```sh
teka app list
teka app build hello-world
teka app deploy --host http://device.local hello-world hello-pub
teka app deploy --host http://device.local --build hello-world hello-pub
teka package pack --manifest manifest.json --member hello-world.raw=hello-world.raw --private-key apps.key --certificate apps.pem --output hello-world.tpk
teka package verify hello-world.tpk --trust ./trust
teka package inspect hello-world.tpk --verify --trust ./trust
teka package stream hello-world.tpk --trust ./trust --member hello-world.raw > hello-world.raw
teka system status --host http://device.local
teka system update ./image.tpk --wait --host http://device.local
```

`build` links `./result/<app>` at the requested images and prints those paths. `deploy APP…` uploads named apps from `./result`; adding `--build` builds those apps for the device architecture first.

- `package pack` zstd-encodes every payload and signs the manifest with the image signing key and certificate.
- `package verify` emits JSON and requires `apps.pem` / `os.pem` certificates under the trust directory.
- `package inspect --verify` reports trust as unavailable unless `--trust` is supplied.
- `package stream` requires publisher trust and emits authenticated member bytes. Disk input reads the selected member; HTTP(S) input validates the complete transport. Output may precede a late failure; check command success before using output.

`system update --wait` waits for a rebooting update to commit or fail.

Unknown commands run `teka-<name>` from `PATH`; `teka --help` lists plugins.

App builds use `apps/flake.nix` at the root of the enclosing git checkout; it must export `teka.apps`. Outside the checkout, use `--flake /path/to/apps` or set `TEKA_APPS_FLAKE_ROOT` to that directory.

Set `TEKA_PASSWORD` to the device administrator password. Host URLs accept HTTP and HTTPS.

## Shell completion

`teka completion [bash|fish|zsh]` prints completion code. Without an argument, it detects the invoking shell, then falls back to `$SHELL`. If neither identifies a supported shell, pass one explicitly.

Load completion once per shell. Run the same line again after updating Teka or adding or removing plugins to refresh the snapshot.

```bash
# Bash: add to ~/.bashrc
eval "$(teka completion)"

# Zsh: add to ~/.zshrc
eval "$(teka completion)"
```

```fish
# Fish: put this line in ~/.config/fish/completions/teka.fish
teka completion | source
```

Nix packages include completion loaders for all three shells. Enable your shell's completion support in NixOS or Home Manager to load them. They discover plugins when loaded, including plugins installed after Teka was built. For bootstrap/Cargo installs or `nix develop`, use the activation lines above.

Completion covers built-in commands, flags, architecture choices, paths, and plugin names. App IDs and device addresses are not suggested. Generation does not run plugins, evaluate Nix flakes, or contact devices.

### Plugin completion metadata

A plugin can supply a `teka-<name>.completion.json` file beside its executable on `PATH`:

```json
{
  "version": 1,
  "about": "Example plugin",
  "options": [
    { "long": "verbose", "short": "v", "help": "Show more detail" },
    { "long": "format", "value": { "choices": ["json", "text"] } },
    { "long": "output", "value": { "hint": "file" } }
  ]
}
```

The optional `about` field also supplies its description in `teka --help`; plugins without a description show “External plugin.”

Options require `long`, `short`, or both, without leading dashes. Without `value`, an option is a boolean flag; `value` makes it take one argument. A value may provide nonempty `choices`, a `file` or `directory` hint, or neither. Set `value.optional` to `true` for an optional argument. Choices and hints cannot be combined. `options` defaults to an empty list; `about` and option `help` are optional. Positional metadata is not supported.

Nested commands use `subcommands`, an optional list of objects with `name`, optional `about`, `options`, and further `subcommands`. Child names follow the plugin-name restrictions and must be unique within their parent; `help` is reserved. For example, `"subcommands": [{"name": "build", "options": [{"long": "verbose"}]}]` adds completion for `teka <plugin> build --verbose`.

Plugin names, long flags, and choices must start with an ASCII letter or digit and contain only ASCII letters, digits, hyphens, or underscores. Plugin names cannot contain `__subcmd__`, which the Bash generator reserves. Short flags must be one ASCII letter or digit. Duplicate flags and `--help`/`-h` are reserved. Unknown metadata fields are rejected. These constraints keep generated shell code safe; plugin names outside this format are omitted from completion but remain dispatchable.

The first executable on `PATH` wins, and its directory supplies the metadata. Built-ins and `help` take precedence over plugins. Missing, unreadable, invalid, unsupported-version, or oversized metadata (over 64 KiB) falls back to plugin-name completion without failing generation. Sidecars must be regular files; symlinks to regular files are supported.

## Develop

For Rust changes under `tools/teka/`:

```sh
cd tools/teka
nix flake check
nix develop
cargo run -p ouster-teka -- --help
```

Agent implementation requirements live in [specs/teka-cli.md](specs/teka-cli.md). See the [developer guide](../../docs/developer-guide.md) for app workflows.

# Teka CLI

Build app images and deploy them to Teka devices.

## Install

From the repository root, run:

```sh
./bootstrap.sh
```

Bootstrap installs Nix if needed, builds Teka, and installs it to `~/.local/bin` when that directory is on `PATH`, otherwise `/usr/local/bin`. Rerun it after updating your checkout. For a downloaded portable binary, run `./teka-<system> build init`.

For a temporary shell without installing Teka, run `nix develop` at the repository root, in `apps/`, or in `tools/teka/`. If Fish startup configuration puts an older installed Teka first on `PATH`, use `nix develop --command fish --no-config`, or run commands directly with `nix develop --command teka …`.

Nix packages include the UKI verifier. Portable and Cargo installs require `sbverify` from `sbsigntool` on Linux, or `brew install osslsigncode` on Darwin.

Supports Linux x86_64/AArch64 and macOS Apple Silicon. `teka build init` prepares a Linux build VM on macOS when needed.

### NixOS / Home Manager

Add the repository-root flake as an input, following your nixpkgs:

```nix
inputs.teka = {
  url = "git+ssh://git@<host>/<path>/teka-sdk";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then apply `overlays.default` and install `pkgs.teka`:

```nix
nixpkgs.overlays = [ inputs.teka.overlays.default ];
environment.systemPackages = [ pkgs.teka ]; # Home Manager: home.packages
```

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

### Nested plugins

Built-in command groups can opt into the same runtime plugin dispatch using their command path as the executable prefix. The `app` and `build` groups enable this: `teka app <name> ARGS…` runs `teka-app-<name> ARGS…` from `PATH`; `teka build <name>` runs `teka-build-<name>`. Help and completion list plugins under their parent group using the local name. Built-in commands and `help` take precedence. Plugins parse their own arguments, including `--help`.

## Check the build environment

```sh
teka build check
teka build check --full
```

The quick check validates local prerequisites. `--full` also compiles a minimal AArch64 Linux program; add `--verbose` to see Nix logs.

### macOS build VM

On Apple silicon, `teka build init` installs the standalone `teka-build-vm` plugin when no Linux builder is configured. Existing Linux builders take precedence until you explicitly register this VM with `vm init`; `TEKA_BUILD_VM=always` or `never` overrides selection during init. Afterward, Nix configuration selects builders for app builds, build-and-deploy, full build checks, and ordinary `nix build` commands. The host daemon retrieves outputs into its store and creates normal result links without changes to trust or signature settings.

```sh
teka build vm init   # initialize, register the host builder (sudo), and enable the VM service
teka build vm init --reset  # delete VM state and initialize defaults (sudo)
teka build vm start  # wait for SSH using saved settings
teka build vm stop         # disable the service; may interrupt active clients
teka build vm start --cpus 12 --memory 8 --idle-timeout 300  # save settings after stopping
teka build vm ssh          # open a guest shell (may boot the guest)
teka build vm ssh top      # run an interactive guest command
teka build vm btop         # open the bundled resource monitor
teka build vm ssh -T uname -a
teka build vm ssh -T journalctl -f -u nix-daemon  # stream guest Nix daemon logs
teka build vm status       # service activity, guest state, and brief live usage
teka build vm check        # prerequisites, host registration, and connectivity; may boot
```

The plugin uses Nixpkgs’ cached Linux guest and vzvm 1.1.0, with native NAT networking and no GUI. A separate `nixpkgs-unstable` input supplies only vzvm and its NixOS VM module; guest packages retain the main Nixpkgs pin. The module configures idle timeout, console logging, and SSH forwarding through Nix options. Rosetta must already be installed on the Mac (`softwareupdate --install-rosetta --agree-to-license`). Keys, the sparse 64 GiB data disk, and logs live in `${XDG_DATA_HOME:-~/.local/share}/teka-build-vm`. The `runtime` out-link retains the runtime as a Nix GC root. SSH listens on localhost port 31023. A unique host key stays in the private state directory; init/reset updates the CLI and system builder pins automatically.

The user LaunchAgent at `~/Library/LaunchAgents/com.teka.build-vm.plist` runs the runtime in the foreground, starts after GUI login, and restarts it after a failure. `init` installs/enables it; `start` enables/loads it; `stop` disables/unloads it, including for future logins. Explicit stop can interrupt builds and SSH sessions. Idle shutdown releases guest resources while leaving the service listening for new connections. SSH, btop, checks, and Nix builder connections may wake an idle guest; they do not enable an explicitly stopped service.

`start` always connects to the guest and waits for SSH readiness before succeeding, including when the service is already running. It reports connection time, including any cold boot. `--idle-timeout SECONDS` selects on-demand shutdown after 1–604800 seconds without forwarded connections; `0` keeps the guest on until stopped. On-demand guests boot again on the next connection.

The first `init` or `start` saves defaults in `config.json`: host logical CPUs minus two (at least one), half the host RAM, and a 1800-second idle timeout. Later starts and ordinary `init` reuse these values; supplied `--cpus`, `--memory`, and `--idle-timeout` arguments replace only their own settings. Bare memory values mean GiB; integer MiB/GiB suffixes are also accepted. CPU/RAM allocations cannot exceed host capacity. Stop the service before changing settings or updating its runtime; repeated starts with unchanged settings do not restart it.

Settings are validated before saving. If preparation or startup fails while stopped, valid requested settings remain saved for a plain `start` retry. Old `resources.json` and `launch.json` files are ignored.

`vm init` registers a system builder in `/etc/nix/teka-build-vm.conf`, installs a root-only SSH identity, and restarts the daemon through sudo. It sets the host daemon's `build-poll-interval` to one second instead of Nix's five-second default so postponed remote builds retry promptly. Rerun `vm init` to update existing registrations. Initialization verifies that Nix loads the registration before reporting success. Existing builders are preserved; declaratively managed Nix configuration must use its system options.

`vm init --reset` stops and disables the service, waits for its process to exit, and removes the entire VM state directory and old LaunchAgent. It then creates fresh SSH keys and a new disk, restores default CPU/RAM and the 1800s on-demand policy, updates the system builder credentials through sudo, and enables the service at login. This deletes the old VM's writable store, logs, and saved settings, including invalid configuration. Normal `init` preserves existing state. If runtime setup or registration fails after deletion, the service stays disabled; retry `vm init` to finish setup.

Nix builds embed both resolved Nixpkgs input revisions in the CLI; direct Cargo builds use `tools/teka/flake.lock`. After updating the flake lock, rebuild or upgrade Teka, then stop the VM service. The next start or `vm init` updates the runtime and store image while preserving the writable disk. To debug startup, inspect `~/.local/share/teka-build-vm/vm.log` and `vm/console.log` under the same data directory.

`vm ssh` forwards SSH options and commands and returns the SSH exit status. Its handshake timeout allows cold boot. It allocates a TTY when stdin and stdout are terminals; use `-T` to disable or `-t`/`-tt` to request one. `vm btop` opens the monitor bundled with the guest's Nixpkgs pin without runtime package downloads. The monitor is also on the guest PATH, so `vm ssh btop` and plain `btop` in the guest shell work.

`status` reports login enablement and launchd service activity, then inspects the monitor's launchd PID domain for a running Virtualization helper. Waiting guests are left alone; active helpers trigger one SSH request for uptime, load, CPUs, RAM usage excluding reclaimable cache, and disk usage. The SSH handshake timeout is two seconds and the entire request is limited to five seconds. Missing or unrecognized process information is reported as unknown; failed guest requests show details as unavailable. Status never enables or reconfigures the service. Helper presence is a best-effort activity signal: a guest shutting down between inspection and SSH can wake again. Use `vm btop` or SSH for further guest diagnostics. `check` explicitly probes connectivity and may boot an on-demand VM; it verifies credentials, system builder registration, SSH, and the guest Nix daemon. Check results include inline failure reasons and SSH connection time; faults or a stopped service return failure.

When live status metrics are unavailable, `status` shows saved CPU/RAM allocation, idle timeout, and disk capacity from local state. Missing or unreadable settings do not hide available disk capacity. Resource sizes use explicit binary units (`KiB`, `MiB`, `GiB`), scaled to at most one decimal place. Configured RAM and disk capacity can exceed the guest's usable totals.

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

The optional `about` field also supplies its description in the parent command's help; plugins without a description show “External plugin.” Nested plugins use the full executable name for their sidecar, such as `teka-app-test.completion.json`.

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
cargo run -p teka-cli -- --help
```

Build downloadable binaries for the current platform from the repository root with `nix build .#teka.dist`. The `result/` directory contains the executable and `SHA256SUMS`.

`nix run .#test-build-init` tests installation and AArch64 Linux compilation in a fresh Ubuntu VM on x86_64 Linux or an ARM macOS guest on Apple Silicon, streaming guest output and build logs. On macOS, a software-emulated Linux builder runs inside the Tart guest without nested hardware virtualization. See the [test spec](specs/installer-test.md).

Agent implementation requirements live in [specs/teka-cli.md](specs/teka-cli.md). See the [developer guide](../../docs/developer-guide.md) for app workflows.

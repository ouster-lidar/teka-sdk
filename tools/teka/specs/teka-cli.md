# `teka` CLI contracts

Agent-facing requirements. Commit spec changes before implementing behavior changes. Human usage and development instructions live in [README.md](../README.md); implementation details live in code and tests.

## CLI output conventions

- Apply these conventions to `teka build` and `teka app init`; other commands retain their existing formats. Unknown-command errors omit internal plugin lookup details.
- Preserve existing icons. Print flat results and prompts without headings, indentation, or redundant final summaries. Use lowercase states and self-contained labels; reserve `→` for guidance and skipped checks.
- Every `⏳︎` step ends with a success or failure result and actual elapsed seconds to one decimal. Already-completed work needs no wait or timing.
- Failures identify the problem and return non-zero. Separate recovery guidance with a blank line. Failed builds print a timed result, a blank line, then complete captured logs unchanged; verbose logs stream without duplication.
- Display paths under the current user's home as `~/…`. Standalone copy-paste commands have blank lines around them and no backticks; inline command references may use backticks.
- Share checks, output, timing, path formatting, and shell detection through small functions. Keep status data on stdout, diagnostics on stderr, and subprocess output intact.
- `build init` enables missing `nix-command flakes` while preserving unrelated settings, then validates the configuration. `build check` remains read-only and recommends init for missing features; `--full` only adds the test build.
- SSH and btop reuse guest-state detection: running guests connect silently without a readiness probe; sleeping guests show startup and timed SSH readiness; unknown state uses neutral connection progress. Preserve the session's exit status, including remote exit 3.
- Shell guidance says “load the Nix environment” and shows only the detected shell's daemon profile command: Fish sources `nix-daemon.fish`; Bash/Zsh source `nix-daemon.sh`. Reuse invoking-shell detection with `SHELL` fallback; unknown shells get the new-shell suggestion.

## Structure

- Host tooling lives in `tools/teka/`; app images live in `apps/`. `teka-cli` exports the shared library and `teka` binary.
- Cargo packages follow [Rust crate naming](../../../specs/rust-crate-naming.md); signed TPK operations live in `teka-tpk`.
- Use verbs for workflows and `<noun> <verb>` for resource commands.

## Commands and devices

| Command | Contract |
|---------|----------|
| `teka build init` | Initialize Nix and install the portable CLI |
| `teka build check [--full [--verbose]]` | Check host prerequisites; optionally compile an AArch64 Linux test |
| `teka app init [--example NAME] [DIR]` | Create and stage a standalone app from an example |
| `teka app build [APP…]` / `teka app list [--examples]` | Build artifacts and print paths, or list apps; no device connection |
| `teka app deploy [--build] [-H URL]… [APP…]` | Deploy the same apps to every host from `./result`, building for the device architectures first with `--build`; then verify startup logs |
| `teka package pack --manifest PATH --member NAME=FILE --private-key KEY --certificate CERT --output FILE [--zstd-level N]` | Finalize the Bao root and stored payload descriptors, sign the manifest, and atomically create a Bao-authenticated TPK package |
| `teka package verify FILE --trust DIR` | Require trusted manifest, image, verity and UKI verification; print the verification report as JSON |
| `teka package stream FILE_OR_HTTP_URL --trust DIR --member NAME [--offset N --size N]` | Authenticate accessed groups on disk or complete sequential HTTP transport; emit only verified logical bytes |
| `teka package inspect FILE [--verify [--trust DIR]] [--json] [-v]` | Read the manifest offline; `--verify` checks manifest and native signatures, BLAKE3, root hash binding and the dm-verity tree, checking trust only with `--trust` |
| `teka system status` | Print the device system status JSON |
| `teka system update FILE [--no-reboot] [--wait]` | Upload and apply a `.tpk` package |
| `teka <name> ARGS…` | Run plugin `teka-<name>` from `PATH` |

- `teka app build` passes a Nix expression on stdin (`nix build --file -`) whose value is a `linkFarm` of the requested apps (dependencies are the existing flake packages). `./result` is the one out-link, and `./result/<app>` refers to each app's store output. Stdout prints those `./result/<app>` paths. The farm name is `teka-<system>`; a later build replaces `./result`. A failed Nix build includes Nix's stderr in the error.

- Deploy replaces apps in place; `--remove` removes existing apps before install. App logs use the install response's `app_id`; known runtime failures are errors. Inputs are [`.tpk`](teka-package.md) files or app names under `./result`; validate members, a single app component and architecture before upload.
- Deploy accepts repeatable `-H` / `--host`. Authenticate all hosts and read their architectures before building. `--build` builds unique targets in one Nix invocation; one target uses `result/<app>`, multiple targets use `result/<target>/<app>`. Without `--build`, artifacts must match every target. `--jobs` (default 4) limits concurrent host deployments; apps stay sequential per host. Collect failures by host and finish all deployments before failing; partial updates are possible.
- App uploads use `/api/v1/manager/install`, and authentication uses `/api/v1/auth/*`. Support setup and login; reject unsuccessful HTTP responses and login redirects.
- Accept HTTP(S) URLs with ports and base paths. Use HTTPS when given; read the password from `TEKA_PASSWORD`; keep session cookies in memory.
- Explicit `-H`/`--host` values override the single-host `TEKA_HOST` fallback. `deploy` fails with a usage error (exit 2) if neither is set; there is no default host. `system` commands accept one host.
- `teka app build` takes `--arch <x86_64|aarch64>` (default `aarch64`; `amd64` and `arm64` are accepted aliases) and appends `-linux` to form the Nix system. For both `teka app build` and `teka app deploy`, infer an omitted app name only when the selected flake's `teka.apps` contains exactly one entry. Fail with guidance to select an app when the registry is empty or has multiple entries. Explicit names select only those apps. App discovery does not select a device; deployment still requires `--host` or `TEKA_HOST`.
- Apps-flake precedence is `--flake`, `TEKA_APPS_FLAKE_ROOT`, then upward discovery from the current directory. Inspect each directory's `flake.nix` through Nix and select the nearest one exporting `teka.apps`. Continue past flakes without that output; never search child directories. Follow Nix's path-reference boundaries: stop at the Git root, filesystem root, or a different filesystem device. Explicit overrides select only the specified directory.
- An apps flake exports `teka.apps`, an attrset of app definitions keyed by app id. Inspect names without forcing metadata values or writing lock files. Evaluation failures and invalid outputs are errors; only a missing output permits continuing upward. `teka app list` prints the names, including an empty list for an empty registry. Builds reject apps missing from `teka.apps`, and unsupported targets fail at build time. Other teka flake data goes under `teka.<name>`. CLI-specific environment variables use `TEKA_*`.
- `teka app list --examples` uses the same directory overrides and discovery boundaries, preferring the project's own catalog before its SDK input. Print sorted names without evaluating app builds, hiding the `default` alias. Outside a project, use the catalog recorded in the CLI build; unpinned builds require an explicit source.

## App initialization

- Select an example with `--example NAME`, using the catalog's default when omitted; `DIR` defaults to `.`. Share catalog discovery with `app list --examples`, accept the `default` alias, and reject unknown names before creating files.
- Leverage Nix flake templates for selection, copying, and conflict handling instead of maintaining a separate initializer. Preserve example files, app IDs, and input declarations; skip identical files and report conflicts without overwriting them.
- Lock declared dependencies, honoring any supplied lockfile. Refuse an existing destination lockfile. Initialization does not build the app.
- Reuse or initialize a Git repository and stage new files, including partial copies needed for retries. Preserve unrelated files, index entries, and existing project guidance; never commit. Add shared `AGENTS.md` guidance only when absent, referring to the app README and locked inputs.
- Report one timed app result and subprocess diagnostics on stderr. After success, print `⚒︎ Build: cd … && teka app build` and `✈︎ Deploy: cd … && teka app deploy --build` with shell-quoted paths and text-presentation icons.

## Build environment

### `teka build init`

- Support Linux x86_64/AArch64 and macOS Apple Silicon.
- Find Nix on `PATH`, then at `/nix/var/nix/profiles/default/bin/nix`. If missing, run the official Determinate installer interactively, preserving confirmation and sudo prompts.
- Enable missing required Nix features through an additive user configuration entry, preserving other settings and honoring `NIX_USER_CONF_FILES`/`XDG_CONFIG_HOME`. Verify the result; stop with guidance for other broken installations or managed configuration. Do not repair the daemon or offer unattended installation.
- Atomically install the running portable binary to `~/.local/bin` when on `PATH`, otherwise `/usr/local/bin`, confirming with `? Install Teka to PATH? [y/N]` before sudo. Use a `?` password prompt with `%p` and `(sudo)`, mentioning macOS only on macOS; request authentication only when sudo needs it. Skip an already installed binary.
- Require portable installs to resolve to that binary on `PATH`; report shadowing with repair guidance.
- Native Nix packages skip self-copying and check the running executable, including through `nix run` without `teka` on `PATH`.
- Verify the installed `teka --version`, then report its version and path once.
- Link to `teka completion --help`; show Nix shell setup only when needed. Leave compilation opt-in through `build check --full`.
- Required failures and cancellations return exit 1 with a concrete failure result; declined installation reports `✗ Teka installation: declined`.

### `teka build check`

- Check executable `teka` resolution on `PATH`, Nix, enabled `nix-command` / flakes, and access to the configured store. The quick check does not fetch inputs, build or change configuration.
- Probe functionality without overriding feature flags. Accept implementations where features are stable and absent from `experimental-features`.
- Share Nix, feature and store checks with `build init`, passing the resolved Nix executable explicitly.
- Report on stderr with repair guidance. Continue independent checks and skip dependent checks whose prerequisites failed; any failed check returns exit 1.
- Share the Nix version/config/store reporting routine with init. Print `teka (PATH)`, `Nix config: valid (flakes)`, and `Nix store: reachable`; failed configuration checks recommend `teka build init`. Dependent skips explain the failed prerequisite.
- Follow the common output conventions for success and failure; keep quick checks identical to `--full` except for the test build.

#### `--full`

- Compile a minimal C hello-world for `aarch64-linux` using pinned Nixpkgs' `writeCBin`, without configure phases, documentation or target execution. Work outside a checkout without channels or the flake registry.
- Compile natively on AArch64 Linux, cross-compile on other Linux hosts, and use a configured AArch64 Linux builder on Darwin. Do not change builder settings or require emulation.
- Give each test derivation a fresh nonce and disable substitution; dependencies may use caches. Keep the display name short and pass the nonce separately.
- Verify an executable AArch64 Linux ELF without running it. Use a temporary out-link and leave the working directory untouched.
- Hide successful Nix output by default; include captured failure diagnostics inline. `--verbose` / `-v` requires `--full` and streams Nix's `--print-build-logs` output on success and failure.

### `teka build vm`

- Check guest DNS resolution and certificate-validated HTTPS access to `cache.nixos.org/nix-cache-info` after SSH succeeds. Bound DNS to five seconds and HTTPS to a ten-second transfer limit with a fifteen-second SSH deadline. Fail the command on either error, skip HTTPS when DNS fails, show the underlying error and a VPN/NAT/DNS troubleshooting hint, and continue independent guest checks.
- Check available RAM, swap, and kernel OOM kills since boot. Any kills or unreadable counters fail the check, even after memory recovers.
- Keep normal builds in RAM; provide a separate sparse 8 GiB swap disk so high-concurrency memory peaks can slow down instead of causing OOM kills. Preserve build concurrency and the cached guest image.
- Include core tools such as `btop` in the VM image using the same Nixpkgs pin as the guest. Use NixOS VM options to include their closures and reuse binary caches where available, keeping first-time setup buildable from Darwin.
- Expose bundled tools on the guest PATH for interactive shells and SSH commands. Stage `/etc/profile.local` through the existing boot-time key service; keep the cached guest system unchanged and include the profile file through `virtualisation.additionalPaths`.
- `btop` launches the packaged guest executable through a direct SSH session, without runtime Nix evaluation, downloads or Nix profile installation. Preserve terminal handling and exit status; allow a cold-boot handshake.
- Embed the CLI build's resolved guest and unstable Nixpkgs revisions in the runtime expression, including root-flake input overrides; direct Cargo builds use `tools/teka/flake.lock`. A rebuilt CLI detects pin or expression changes and updates the runtime at initialization or the next start while stopped, preserving the writable disk. Never update a loaded monitor.
- Persist CPU count, RAM in MiB, and idle timeout seconds in one `config.json`. The first `init` or `start` resolves and saves defaults: `max(1, host logical CPUs - 2)` vCPUs, half the host RAM, and 1800 seconds. Later starts and ordinary `init` reuse saved values. `start --cpus N --memory SIZE --idle-timeout SECONDS` replaces only explicitly supplied fields; all three flags are optional without parser defaults. Accept positive CPU counts and integer MiB/GiB memory sizes bounded by host capacity; bare memory values mean GiB. Accept idle timeout `0` for always-on or 1–604800 seconds for on-demand shutdown. Ignore legacy `resources.json` and `launch.json` files.
- Validate the complete settings before writing, and reject actual changes while the monitor is loaded without changing config, runtime, or LaunchAgent. Equal supplied values and plain starts preserve the loaded service. While stopped, atomically save valid desired settings before runtime preparation; retain them on setup/start failure so plain `start` can retry. Use the same settings to derive the runtime and LaunchAgent allocation.
- Use the separately pinned `nixpkgs-unstable` input only for vzvm and its NixOS VM module. Configure `virtualisation.vz.idleTimeout`, console logging, and the builder host port through Nix options; retain native NAT, the main guest package pin, and boot fixes. Forward localhost port 31023 to guest vsock port 22.
- Every successful start waits for guest SSH readiness, including repeated starts, without rebooting a running guest. Report the service state and timed SSH result using state-aware startup or connection progress.
- Restrict VM SSH authentication to the supplied identity with `IdentitiesOnly=yes`, so unrelated agent keys cannot exhaust the guest's authentication attempts. Rotation tests use matching Ed25519 pairs and verify both pins against the private-derived public key.
- Install a user LaunchAgent with `RunAtLoad`, `KeepAlive`, explicit working directory, environment, and logs. Initialize keys, disk, and directories before loading; run the runtime runner in the foreground and retain its Nix out-link as a GC root. `init` installs/enables it, `start` enables/loads it, and `stop` disables/unloads it. Require stopping the monitor before runtime or launch configuration changes. The service runs after GUI login. Report shutdown progress and timed service completion only when stopping a loaded service; an already-stopped service reports its state without timing.
- Use the registered system Nix builder for app builds, build-and-deploy, full checks, and ordinary Nix builds. Preserve external builder selection during init; afterward use Nix configuration. Let the host daemon retrieve outputs and create result links without changing trust/signature settings. Do not wrap commands, select an SSH store, or export artifacts privately.
- Delegate connection tracking, guest shutdown, and reconnects to vzvm. Explicit stop may interrupt clients and remains disabled at login; idle shutdown leaves the monitor available. SSH, btop, start, and check may wake the guest. Check reports concrete failure reasons inline, skips dependent checks without repeating the cause, and measures the full SSH connectivity attempt in seconds with one decimal place.
- Status falls back to saved CPU/RAM allocation, idle timeout, and local disk capacity whenever live metrics are unavailable, including stopped, waiting, unknown, and failed SSH probes. Read saved settings without creating defaults or changing configuration; missing or unreadable settings must not hide available disk capacity. Label configured allocations and disk capacity separately from live usage. Format local and guest resource sizes with explicit IEC units, scaled to at most one decimal place; collect numeric guest byte counts instead of parsing human-readable sizes.
- Status reports login enablement and service activity, then queries `launchctl print pid/MONITOR_PID` using the current running monitor's PID. Inspect only the domain's `services` block for a positive-PID `com.apple.Virtualization.VirtualMachine` service or suffixed instance. Empty or zero-only entries indicate waiting; missing, malformed, stale, or unavailable process information is unknown and must not trigger SSH. An active helper permits one SSH request for uptime, load, CPU count, RAM used/total excluding reclaimable cache, and disk used/total. Limit the handshake to two seconds and the whole request to five seconds; failed requests report guest details unavailable. Status never enables or changes the service and returns success when guest details are unavailable. Keep a TODO to replace this activity heuristic when vzvm exposes guest state; shutdown can race with the SSH request and wake the guest. Use launchctl without sudo, additional process-inspection tools, or a patched VM runtime.
- `init --reset` stops and disables the service, waits for its process to exit, deletes the complete state directory and old LaunchAgent before reading configuration, then initializes and registers a fresh VM with default resources and the 1800s on-demand policy. Corrupt saved configuration must not prevent reset. Regenerate SSH credentials and update the system builder identity while preserving unrelated builders. Keep the lifecycle lock outside the deleted directory. If stopping fails, retain the existing VM files; if runtime setup or registration fails, leave the service disabled and allow a normal `init` retry.
- Failed initialization, startup, and shutdown print concrete reasons and exit non-zero. Timed SSH failures include the elapsed time and a separate VM-log hint. SSH and btop preserve guest output and remote exit status; running guests connect silently without a readiness probe, sleeping guests show startup and timed readiness, and unknown state uses neutral connection progress.
- Init and reset print flat step results; time runtime preparation, reset removal, registration, and service changes. Use `⏳︎` for waits and `--prompt "? macOS password for %p (sudo): "` for actual password requests. Share lifecycle output when invoked by `build init`; keep console-log troubleshooting in documentation.

## System

- `teka system` uses `GET /api/v1/app/teka/status` and `POST /api/v1/app/teka/sysupdate/apply`.
- `update` validates package structure and transport, a single OS component and architecture, then streams `FILE` as `application/octet-stream`, adds `?reboot=false` with `--no-reboot`, and prints the apply response JSON. The response `result` is `REBOOTING`, `INSTALLED_NO_REBOOT`, or `NO_UPDATE_NEEDED`.
- `--wait` records the status `version` before upload. For `REBOOTING`, it reauthenticates and polls `update.commit_state`:
    - `COMMITTED` or `NONE` with a changed `version`: success; print the final status JSON.
    - `ABORTED_UNHEALTHY` or `FAILED_SET_DEFAULT`: error listing `update.failed_units`.
    - Original `version` after observing a changed version: error (rolled back).
    - No result within `TEKA_UPDATE_WAIT_SEC` (default 900): error.
    - Polls every `TEKA_UPDATE_POLL_MS` (default 1000) and retries failed polls until the deadline.
- The library exposes the update and wait steps.

## Plugins

- An unknown command `<name>` runs `teka-<name>` from `PATH` with the remaining arguments; the plugin's exit status is teka's exit status. On Unix, teka replaces itself with the plugin.
- Built-in command groups can opt into nested plugin dispatch. An unknown child `<name>` under a command path `<parent>…` runs `teka-<parent>-…-<name>` from `PATH`, forwarding only the arguments after `<name>` using the same lookup and process replacement as top-level plugins. The plugin owns parsing of its remaining arguments, including help.
- Built-in commands and `help` always win over plugins.
- An unknown command with no plugin on `PATH` is a usage error.
- Help and completion list plugins under matching built-in command groups that accept external children, using the local child name. Nested plugins and collisions with built-in children or `help` are omitted from root listings. Hyphenated names without a matching extensible command prefix remain top-level. The first executable on `PATH` wins, and its full executable name determines the metadata sidecar path.

## Shell completion

- `teka completion [bash|fish|zsh]` prints a static completion snapshot composed from built-in Clap definitions and executable plugins on `PATH`. An explicit shell wins; otherwise detect the invoking shell, then fall back to `SHELL`. Failed detection is a usage error. Generation must not execute plugins, evaluate flakes, or contact devices. Reloading replaces the previous snapshot.
- Optional version-1 `teka-<name>.completion.json` sidecars alongside the winning executable describe nested commands, flags, optional values, fixed choices, and file/directory hints. Missing, unreadable, invalid, unsupported, or oversized metadata falls back to plugin-name completion. See the README for the schema and token restrictions.
- Completion augments a separate command tree; normal external-command parsing and dispatch remain unchanged. Built-ins and `help` retain precedence. Unsafe plugin names are omitted from shell code.
- Nix packages install Bash, Fish, and Zsh completion files. Bootstrap/Cargo and development-shell users load them through the documented activation commands and `completion --help`. Dynamic app/device suggestions, plugin callbacks, and positional plugin metadata are deferred.

## Output and exit codes

- Package JSON reports keep the parsed manifest under `manifest`. `package verify` includes runtime results under `verification`; `package inspect` includes that key only with `--verify`.
- Plain inspection reads the bounded prefix and CMS object, then extracts the manifest JSON without authenticating it or reading payload bytes. Preserve its parsed fields without schema normalization; JSON output contains only `manifest`.
- Verified inspection authenticates the manifest before parsing it, verifies complete BLAKE3 transport and native payloads, and keeps every runtime result under `verification`.
- Native verification reports include decoded image and UKI sizes; these come from authenticated frame headers, not manifest storage lengths.
- Text inspection starts with `Manifest` and indents component summaries and stored payload sizes beneath it. With `--verify`, add `Verification` with `Signatures`, `Integrity`, and `Publisher trust` groups indented beneath it, each itemizing its checks one level deeper; omit verification without `--verify`.
- `Signatures` lists the manifest signature with its signing role and certificate fingerprint, then each component's image signature with its certificate fingerprint and, for OS components, its UKI Authenticode signature. `Integrity` lists BLAKE3 transport, then each component's root hash binding and dm-verity tree. Fingerprints are abbreviated like root hashes unless `--verbose`.
- Mirror these groups under JSON `verification`: `signatures.manifest` and `signatures.native` each include `valid`; retain signer and component diagnostics under the corresponding signature group. `integrity` contains boolean `blake3`, `root_hash_binding`, and `dm_verity_tree` results. Do not duplicate trust flags or retain legacy verification keys.
- Signature validity alone does not establish publisher trust. Without configured certificates, JSON `publisher_trust` is `{"status":"not_checked","reason":"no_trust_directory"}` and text shows exactly `· not checked (add --trust DIR to verify the publisher)`, with no separate trust hint. With trusted certificates, use `{"status":"trusted"}` and `✓ signing certificates match configured trust`.
- A failed verification emits no success report or unchecked-trust result. `package verify` returns exit 3 for rejected package contents; input-file access, trust configuration, staging I/O and verifier process failures return exit 1. Keep these categories machine-readable without matching diagnostic text.
- Keep data on stdout and diagnostics on stderr. Exit codes are 0 for success (including `--help` and bare `teka`), 1 for operational errors, 2 for usage errors, and 3 for rejected contents from `package verify`. Only build initialization may ask for setup confirmation.

## Packaging and checks

- The repository-root flake re-exports a non-static `teka` package, `apps.<system>.teka`, and `overlays.default` for `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin` for NixOS/Home Manager consumers. Consumers use the root flake, not `?dir=tools/teka`, and follow only `nixpkgs` on that input; the root flake forwards `crane` into `tools/teka`. The overlay reuses `packages.<system>.teka`; it does not rebuild with consumer nixpkgs, and there is no Home Manager module. Root `nix develop` puts `teka` on `PATH` without installation. The `apps/` flake's dev shell contains only `teka` (from `tools/teka`) and builds no app images.
- The `tools/teka` flake owns Crane and exports native `teka`/`default` and `portable` packages, plus `apps.<system>.teka` and `overlays.default`, for those systems. Bootstrap uses `portable`, with static Linux outputs; the root flake and device tests consume `teka`.
- Portable outputs resolve UKI verifiers on `PATH`, with no embedded Nix-store paths. Linux binaries are static; Apple Silicon binaries statically link third-party libraries and retain macOS system linkage.
- Both flakes expose `teka.dist` for the selected system: a directory containing `teka-<system>` and `SHA256SUMS` with relative filenames. The distribution must have no store references (`allowedReferences = [ ]`).
- Set `meta.mainProgram = "teka"` and provide relative `bin/teka` and `bin/teka-<system>` links so `nix run` and `nix shell` can execute the distribution.
- Read the package version from the CLI Cargo manifest. Distribution checks cover checksums, copied executable behavior, platform linkage and completions.
- `bootstrap.sh` supports `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin`. Resolve or interactively install Nix, check its features and store, build portable Teka, then invoke `build init`.
- Keep bootstrap's portable output rooted under `${XDG_DATA_HOME:-$HOME/.local/share}/teka`.
- Source checkouts install through root `bootstrap.sh`; downloaded portable executables install through `teka build init`. Workflow docs use plain `teka` commands and link to the installation guide.
- Use Crane with cached Cargo dependencies; documentation changes must not invalidate Rust builds. Name branches in flake inputs and keep exact revisions in lockfiles.
- `nix flake check` covers CLI dispatch and device protocol behavior. Validate `bootstrap.sh` on supported hosts.

# `teka` CLI contracts

Agent-facing requirements. Commit spec changes before implementing behavior changes. Human usage and development instructions live in [README.md](../README.md); implementation details live in code and tests.

## CLI output conventions

- Apply these conventions to `teka build`; other command groups retain their existing formats. Unknown-command errors omit internal plugin lookup details.
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
| `teka app build APP…` / `teka app list` | Build artifacts and print paths, or list apps; no device connection |
| `teka app deploy [--build] [-H URL]… APP…` | Deploy the same apps to every host from `./result`, building for the device architectures first with `--build`; then verify startup logs |
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
- `teka app build` takes `--arch <x86_64|aarch64>` (default `aarch64`; `amd64` and `arm64` are accepted aliases) and appends `-linux` to form the Nix system.
- Apps-flake precedence is `--flake`, `TEKA_APPS_FLAKE_ROOT`, then `apps/flake.nix` at the root of the enclosing git checkout. An apps flake exports `teka.apps`, an attrset of per-app metadata keyed by app id; `teka app list` prints its names without forcing values. Builds reject apps missing from `teka.apps`, and unsupported targets fail at build time. Other teka flake data goes under `teka.<name>`. CLI-specific environment variables use `TEKA_*`.

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
- Keep data on stdout and diagnostics on stderr. Exit codes are 0 for success (including `--help` and bare `teka`), 1 for operational errors, 2 for usage errors, and 3 for rejected contents from `package verify`. Do not ask for confirmation.

## Packaging and checks

- The repository-root flake re-exports a non-static `teka` package, `apps.<system>.teka`, and `overlays.default` for `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin` for NixOS/Home Manager consumers. Consumers use the root flake, not `?dir=tools/teka`, and follow only `nixpkgs` on that input; the root flake forwards `crane` into `tools/teka`. The overlay reuses `packages.<system>.teka`; it does not rebuild with consumer nixpkgs, and there is no Home Manager module. Root `nix develop` puts `teka` on `PATH` without installation. The `apps/` flake's dev shell contains only `teka` (from `tools/teka`) and builds no app images.
- The `tools/teka` flake owns Crane and exports native `teka`/`default` and `portable` packages, plus `apps.<system>.teka` and `overlays.default`, for those systems. Bootstrap uses `portable`, with static Linux outputs; the root flake and device tests consume `teka`.
- `bootstrap.sh` supports `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin`. Prefer Nix when available; otherwise use Cargo, and fail with a future binary-release note when neither is installed. Report the builder and install destination before building. Install to `~/.local/bin` only when that directory is on `PATH`; otherwise use `/usr/local/bin`, confirming before privileged installation.
- The root `bootstrap.sh` is the only supported install entry point. Workflow docs use plain `teka` commands and link to the installation guide.
- Use Crane with cached Cargo dependencies; documentation changes must not invalidate Rust builds. Name branches in flake inputs and keep exact revisions in lockfiles.
- `nix flake check` covers CLI dispatch and device protocol behavior. Validate `bootstrap.sh` on supported hosts.

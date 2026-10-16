# `teka` CLI contracts

Agent-facing requirements. Commit spec changes before implementing behavior changes. Human usage and development instructions live in [README.md](../README.md); implementation details live in code and tests.

## Structure

- Host tooling lives in `tools/teka/`; app images live in `apps/`. `ouster-teka` exports the shared library and `teka` binary.
- Use verbs for workflows and `<noun> <verb>` for resource commands.

## Commands and devices

| Command | Contract |
|---------|----------|
| `teka app build APP…` / `teka app list` | Build artifacts and print paths, or list apps; no device connection |
| `teka app deploy [--build] APP…` | Deploy apps linked in `./result`, or build the positional apps for the device architecture first; then verify startup logs |
| `teka system status` | Print the device system status JSON |
| `teka system update FILE [--no-reboot] [--wait]` | Upload and apply a system update file |
| `teka <name> ARGS…` | Run plugin `teka-<name>` from `PATH` |

- `teka app build` passes a Nix expression on stdin (`nix build --file -`) whose value is a `linkFarm` of the requested apps (dependencies are the existing flake packages). `./result` is the one out-link, and `./result/<app>` refers to each app's store output. Stdout prints those `./result/<app>` paths. The farm name is `teka-<system>`; a later build replaces `./result`. A failed Nix build includes Nix's stderr in the error.

- Deploy replaces apps in place; `--remove` removes existing apps before install. App logs use the install response's `app_id`; known runtime failures are errors.
- App uploads use `/api/v1/manager/install`, and authentication uses `/api/v1/auth/*`. Support setup, bootstrap tokens, and login; reject unsuccessful HTTP responses and login redirects.
- Accept HTTP(S) URLs with ports and base paths. Use HTTPS when given; read secrets from `TEKA_PASSWORD` / `TEKA_BOOTSTRAP_TOKEN`; keep session cookies in memory.
- Host precedence is `-H`/`--host`, then `TEKA_HOST`; `deploy` fails with a usage error (exit 2) if neither is set. There is no default host.
- `teka app build` takes `--arch <x86_64|aarch64>` (default `aarch64`; `amd64` and `arm64` are accepted aliases) and appends `-linux` to form the Nix system. `deploy --build` uses the device's architecture.
- Apps-flake precedence is `--flake`, `TEKA_APPS_FLAKE_ROOT`, then `apps/flake.nix` at the root of the enclosing git checkout. An apps flake exports `teka.apps`, an attrset of per-app metadata keyed by app id; `teka app list` prints its names without forcing values. Builds reject apps missing from `teka.apps`, and unsupported targets fail at build time. Other teka flake data goes under `teka.<name>`. CLI-specific environment variables use `TEKA_*`.

## System

- `teka system` uses `GET /api/v1/app/jetstream/status` and `POST /api/v1/app/jetstream/sysupdate/apply`.
- `update` streams `FILE` as `application/octet-stream`, adds `?reboot=false` with `--no-reboot`, and prints the apply response JSON. The response `result` is `REBOOTING`, `INSTALLED_NO_REBOOT`, or `NO_UPDATE_NEEDED`.
- `--wait` records the status `version` before upload. For `REBOOTING`, it reauthenticates and polls `update.commit_state`:
    - `COMMITTED` or `NONE` with a changed `version`: success; print the final status JSON.
    - `ABORTED_UNHEALTHY` or `FAILED_SET_DEFAULT`: error listing `update.failed_units`.
    - Original `version` after observing a changed version: error (rolled back).
    - No result within `TEKA_UPDATE_WAIT_SEC` (default 900): error.
    - Polls every `TEKA_UPDATE_POLL_MS` (default 1000) and retries failed polls until the deadline.
- The library exposes the update and wait steps.

## Plugins

- An unknown command `<name>` runs `teka-<name>` from `PATH` with the remaining arguments; the plugin's exit status is teka's exit status. On Unix, teka replaces itself with the plugin.
- Built-in commands and `help` always win over plugins.
- An unknown command with no plugin on `PATH` is a usage error.
- `teka --help` lists plugins discovered on `PATH`, excluding built-ins; the first match on `PATH` wins.

## Shell completion

- `teka completion [bash|fish|zsh]` prints a static completion snapshot composed from built-in Clap definitions and executable plugins on `PATH`. An explicit shell wins; otherwise detect the invoking shell, then fall back to `SHELL`. Failed detection is a usage error. Generation must not execute plugins, evaluate flakes, or contact devices. Reloading replaces the previous snapshot.
- Optional version-1 `teka-<name>.completion.json` sidecars alongside the winning executable describe nested commands, flags, optional values, fixed choices, and file/directory hints. Missing, unreadable, invalid, unsupported, or oversized metadata falls back to plugin-name completion. See the README for the schema and token restrictions.
- Completion augments a separate command tree; normal external-command parsing and dispatch remain unchanged. Built-ins and `help` retain precedence. Unsafe plugin names are omitted from shell code.
- Nix packages install Bash, Fish, and Zsh completion files. Bootstrap/Cargo and development-shell users load them through the documented activation commands and `completion --help`. Dynamic app/device suggestions, plugin callbacks, and positional plugin metadata are deferred.

## Output and exit codes

- Keep data on stdout and diagnostics on stderr. Exit codes are 0 for success (including `--help` and bare `teka`), 1 for operational errors, and 2 for usage errors. Do not ask for confirmation.

## Packaging and checks

- The repository-root flake re-exports a non-static `teka` package, `apps.<system>.teka`, and `overlays.default` for `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin` for NixOS/Home Manager consumers. Consumers use the root flake, not `?dir=tools/teka`, and follow only `nixpkgs` on that input; the root flake forwards `crane` into `tools/teka`. The overlay reuses `packages.<system>.teka`; it does not rebuild with consumer nixpkgs, and there is no Home Manager module. Root `nix develop` puts `teka` on `PATH` without installation. The `apps/` flake's dev shell contains only `teka` (from `tools/teka`) and builds no app images.
- The `tools/teka` flake owns Crane and exports native `teka`/`default` and `portable` packages, plus `apps.<system>.teka` and `overlays.default`, for those systems. Bootstrap uses `portable`, with static Linux outputs; the root flake and device tests consume `teka`.
- `bootstrap.sh` supports `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin`. Prefer Nix when available; otherwise use Cargo, and fail with a future binary-release note when neither is installed. Report the builder and install destination before building. Install to `~/.local/bin` only when that directory is on `PATH`; otherwise use `/usr/local/bin`, confirming before privileged installation.
- The root `bootstrap.sh` is the only supported install entry point. Workflow docs use plain `teka` commands and link to the installation guide.
- Use Crane with cached Cargo dependencies; documentation changes must not invalidate Rust builds. Name branches in flake inputs and keep exact revisions in lockfiles.
- `nix flake check` covers CLI dispatch and device protocol behavior. Validate `bootstrap.sh` on supported hosts.

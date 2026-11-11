# Rust crate naming

## Packages and features

- The root Cargo workspace contains exactly `crates/teka` and `crates/teka-cli`, with shared release metadata, edition 2021 and resolver 2.
- Both packages start at `0.1.0` and use BSD-3-Clause. Declare and test the minimum supported Rust version before release.
- `teka` is the SDK dependency. Its default `app` feature exposes runtime, devres, startup and HTTP/WebSocket helpers under `teka::app`, plus `teka::declare_app_runtime!` at the crate root.
- Optional `client` exposes blocking device HTTP/session APIs under `teka::client`, independently of `tpk`. Authentication inputs are explicit; credentials from the environment, progress and terminal output belong in CLI adapters.
- Optional `tpk` exposes packing, parsing, streaming, trust verification, image and UKI APIs under `teka::tpk`. Preserve wire bytes, trust enforcement, streaming failure behavior and the module's unsafe-code restriction.
- Use optional dependencies and explicit `dep:` feature entries. Keep features additive and retain target-specific dependencies. App builds exclude TPK-only dependencies and CLI code.
- `teka-cli` provides the `teka` executable and shared command implementation library. It depends on `teka` with defaults disabled and `client,tpk` enabled.
- Example apps remain independent packages with explicit workspace boundaries and lockfiles. Migrate consumers directly without compatibility aliases.

## Executables and source layout

- The `teka` package provides `teka-runner`, gated by `app`, for registration, resource leases and lifecycle management of external app executables.
- Appliance services invoke `teka-runner` directly. CLI plugin discovery also exposes it as `teka runner` without a built-in subcommand.
- TPK commands use only `teka tpk`; preserve existing subcommands, arguments, output and exit codes.
- Macro expansion uses `$crate` support paths, including a documentation-hidden Tokio re-export, and works with a renamed SDK dependency.
- Nix workspace composition lives in `nix/default.nix`, package definitions in `nix/packages/`, and CLI checks in `nix/tests/`; reusable app builders live in `nix/apps/`. Export native `teka`, `teka.dist`, `teka-runner` and standalone `teka-src`; keep CLI and runner derivations and Cargo artifacts separate.
- `teka-src` materializes workspace metadata for use outside this checkout. Cargo consumers build the SDK from source; the runner output installs only `bin/teka-runner`.
- CLI archives include embedded VM resources and package-local fallback pins, with one documented update procedure and Nix overrides preserved.

## Validation and release

- Validate isolated feature combinations, downstream macro expansion, independent examples, runtime/Python lifecycle, TPK trust/transport and CLI dispatch/completion/plugins.
- Run formatting, Clippy, documentation, packaging dry-runs and affected Nix builds/checks on supported Linux and Apple Silicon macOS systems where available.
- Verify self-contained crate archives outside the checkout, and document build and external-tool prerequisites.
- Publication, registry installs and remote changes are separate from local implementation and require explicit authorization.

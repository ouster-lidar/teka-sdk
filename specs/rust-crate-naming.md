# Rust crate naming

## Motivation

Namespace Teka tooling and reusable libraries under `teka` so package names identify the product, remain recognizable across repositories, and avoid generic names such as `app-runtime`. Use descriptive suffixes consistently. `teka-tpk` identifies the signed TPK format supported by its readers, packers and verifiers. `teka-app` names the app developer integration SDK, including runtime support.

## Names

| Purpose | Cargo package | Rust crate |
|---------|---------------|------------|
| Main CLI and shared CLI library | `teka-cli` | `teka_cli` |
| Signed TPK format library | `teka-tpk` | `teka_tpk` |
| Application SDK and runtime | `teka-app` | `teka_app` |

- Supporting tooling and libraries use `teka-<purpose>` with concise, descriptive suffixes. Prefer full words unless an abbreviation is established in the relevant domain.
- Cargo dependency keys match package names; Rust imports use the corresponding underscore spelling without compatibility aliases.
- Example application packages retain their app names, such as `hello-pub`.
- Executable names remain `teka` and `app-runtime-process`; plugins retain `teka-<name>`.
- Source directories, app IDs, macro names and the macro-generated app-local `app_runtime` module retain their existing names.
- Nix derivation `pname` values for these crates match their Cargo package names. Existing flake attributes such as `teka`, `app-runtime` and `app-runtime-src` remain stable.
- Nix-installed Rust libraries and consumers use the names produced by Cargo, including `libteka_app.rlib`.

## Validation

- Update manifests, Rust imports, package selectors and lockfiles together, including downstream consumers.
- Verify CLI and runtime tests, example app builds and Nix checks without changing command behavior or refreshing unrelated dependencies.

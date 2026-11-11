# Teka Rust SDK

Rust integration for Teka applications, devices and signed TPK files.

```toml
[dependencies]
teka = "0.1.0"
```

- The default `app` feature provides registration, resource leases, startup and HTTP/WebSocket helpers through `teka::app`.
- `teka::declare_app_runtime!("my-app")` declares an app-local runtime module.
- Enable `client` for the blocking device session client under `teka::client`; pass credentials explicitly.
- Enable `tpk` for packing, reading, streaming and verifying signed packages under `teka::tpk`.
- Disable default features when only device or TPK APIs are needed:

```toml
teka = { version = "0.1.0", default-features = false, features = ["client"] }
```

## External app executables

Install the lifecycle runner separately from the development CLI:

```sh
cargo install teka --version 0.1.0 --bin teka-runner --locked
teka-runner my-app tcp 8000 python3 server.py
```

See [runner usage](teka-runner.md) for lifecycle and resource behavior.

## Build requirements

- Linux and Apple Silicon macOS are supported; Rust 1.95 or newer is required.
- The `tpk` feature requires a C compiler, `pkg-config` and OpenSSL development headers/libraries. Cargo builds the compression dependencies from source.
- Native image verification may invoke `sbverify` (Linux) or `osslsigncode` (macOS); install the verifier on `PATH`.
- The blocking device client uses Rustls for HTTPS and keeps session cookies in memory.
- Nix development and release checks run from the repository root with `nix develop -c cargo …`.

Licensed under BSD-3-Clause; see [LICENSE](LICENSE).

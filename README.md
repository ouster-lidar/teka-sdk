# Teka SDK and Example Apps

Develop apps with the Rust SDK and `teka` CLI, build the examples in `apps/`, and deploy them to a Teka device (for example a ZedBox).

## Start here

- **Recommended demo:** [`hello-ouster-world`](apps/examples/hello-ouster-world/README.md) opens OSF files or a live sensor with the Ouster SDK, and supports browser display and recording.
- [`hello-world`](apps/examples/hello-world/README.md) is a minimal oneshot smoke test.
- [`hello-webui`](apps/examples/hello-webui/README.md) is a static web UI with no backend.

Browse [all examples](apps/examples/); each app has its own README.

## Quick start

1. Clone this repository on Linux x86_64/AArch64 or Apple Silicon macOS. For Windows, start with the [WSL 2 host setup](docs/wsl2.md).
2. Run `./bootstrap.sh` from the repository root to install Nix if needed and build and install the CLI. See the [installation guide](crates/teka-cli/README.md#install) for alternatives.
3. Build and deploy an app, or upload its package in the web UI.

From the repository root:

**Build** (AArch64 package for web UI upload or distribution):

```bash
teka app build hello-world
```

**Build + Deploy** (normal workflow — build for the device, upload, sample logs):

```bash
export TEKA_PASSWORD='your-device-admin-password'
teka app deploy --build hello-ouster-world --device http://device.local/
```

Builds create packages under `result/<app>/`.
Replace `device.local` with your device's hostname or IP address.
Deployment replaces an existing install and uses the device administrator password in `TEKA_PASSWORD`.
`--build` builds for the device architecture before uploading.

Full procedures: [Developer Guide](docs/developer-guide.md).

## Documentation

| Guide                                        | Contents                                         |
| -------------------------------------------- | ------------------------------------------------ |
| [Developer Guide](docs/developer-guide.md)   | Build, deploy, and auth workflows                |
| [Linux host setup](docs/linux.md)            | Packages and Nix                                 |
| [WSL 2 host setup](docs/wsl2.md)             | Windows development host, networking             |
| [Nix install and flake outputs](docs/nix.md) | Nix install and flake outputs                    |

## Rust SDK

- [`teka`](crates/teka/README.md) provides app runtime integration and optional device-client and TPK APIs.
- [`teka-cli`](crates/teka-cli/README.md) provides the `teka` development CLI.
- [`teka-runner`](crates/teka/teka-runner.md) manages registration, resource leases and lifecycle for external app executables.

# Teka SDK and Example Apps

Develop apps with the Rust SDK and `teka` CLI, start from an example without cloning this repository, and deploy to a Teka device (for example a ZED Box).

## Start here

- **Recommended demo:** [`hello-ouster-world`](apps/examples/hello-ouster-world/README.md) opens OSF files or a live sensor with the Ouster SDK, and supports browser display and recording.
- [`hello-world`](apps/examples/hello-world/README.md) is a minimal oneshot smoke test.
- [`hello-webui`](apps/examples/hello-webui/README.md) is a static web UI with no backend.
- [`hello-gpio`](apps/examples/hello-gpio/README.md) toggles GPIO lines at 1 Hz from Python or C++.

`teka app init --example NAME` starts from `hello-world`, `hello-webui`, or `hello-ouster-world`.
Browse [all examples](apps/examples/); each app has its own README.

## Quick start

1. Install the latest released CLI on Linux x86_64 or Apple Silicon macOS:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | sh
   ```

   The installer sets up the CLI and Nix if needed. For Windows, start with the [WSL 2 host setup](docs/wsl2.md).
2. Browse the examples and create your first app:

   ```sh
   teka app list --examples
   teka app init --example hello-world my-first-app
   cd my-first-app
   ```

   The CLI fetches the example and sets up a standalone project. No repository clone is needed.

**If you have a device**, select it and build and deploy your app:

```sh
teka box select
teka app deploy
```

Selection discovers devices, prompts for the administrator password, and saves your deployment targets and credentials. Deploy selects the box architectures and installs the packages. Nix substitutes cached outputs and builds on a cache miss.

View the deployed app at `http://<teka-device>.local` or your device's IP address.

To distribute a package, build for the box architecture and copy the printed `.tpk`. Build defaults to `aarch64`. Use `--arch x86_64` for an x86_64 box.

```sh
package=$(teka app build)
cp "$package" ./my-app.tpk
teka app deploy ./my-app.tpk
```

You can deploy the copy from any directory or machine with teka and access to the box, or upload it in the web UI.

See the [installation guide](crates/teka-cli/README.md#install) for other installation options and the [Developer Guide](docs/developer-guide.md) for full build, deploy, and auth procedures.

## Documentation

| Guide                                        | Contents                                         |
| -------------------------------------------- | ------------------------------------------------ |
| [Getting Started](docs/01_getting-started/01_zed-box-with-tekaos.md) | Set up a ZED Box with TekaOS and deploy your first app |
| [Teka Apps](docs/02_teka-apps/00_README.md)  | Start your own app, templates, and packaging     |
| [TekaOS](docs/03_tekaos/00_README.md)        | CLI, system apps, and updates                    |
| [Edge compute & accessories](docs/04_edge_compute_&_accessories/00_README.md) | ZED Box ports, GPIOs, reset, and troubleshooting |
| [Developer Guide](docs/developer-guide.md)   | Build, deploy, and auth workflows                |
| [Linux host setup](docs/linux.md)            | Packages and Nix                                 |
| [WSL 2 host setup](docs/wsl2.md)             | Windows development host, networking             |
| [Nix install and flake outputs](docs/nix.md) | Nix install and flake outputs                    |

## Rust SDK

- [`teka`](crates/teka/README.md) provides app runtime integration and optional device-client and TPK APIs.
- [`teka-cli`](crates/teka-cli/README.md) provides the `teka` development CLI.
- [`teka-runner`](crates/teka/teka-runner.md) manages registration, resource leases and lifecycle for external app executables.

# Jetstream Example Apps

Build app images from `apps/`, deploy to a Jetstream device (for example a ZedBox), and run them alongside other appliance apps.

## Start here

- **Recommended demo:** [`hello-ouster-world`](apps/examples/hello-ouster-world/README.md) opens OSF files or a live sensor with the Ouster SDK, and supports browser display and recording.
- [`hello-world`](apps/examples/hello-world/README.md) is a minimal oneshot smoke test.
- [`hello-webui`](apps/examples/hello-webui/README.md) is a static web UI with no backend.

Browse [all examples](apps/examples/); each app has its own README.

## Quick start

1. Complete the [Linux host setup](docs/linux.md).
2. Clone this repository.
3. [Install the CLI](tools/teka/README.md#install), or run `nix develop` at the repo root for `teka` on `PATH` without installing.
4. Build and deploy an app, or upload its image in the web UI.

From `apps/`:

**Build** (image on disk only, for web UI or app distribution):

```bash
cd jetstream-example-apps/apps
teka app build hello-world
```

**Build + Deploy** (normal workflow — build for the device, upload, sample logs):

```bash
cd jetstream-example-apps/apps
teka app deploy --build hello-ouster-world --host http://<device>/
```

Full procedures: [Developer Guide](docs/developer-guide.md).

## Documentation

| Guide                                        | Contents                                         |
| -------------------------------------------- | ------------------------------------------------ |
| [Developer Guide](docs/developer-guide.md)   | Build, deploy, and auth workflows                |
| [Linux host setup](docs/linux.md)            | Packages and Nix                                 |
| [WSL 2 host setup](docs/wsl2.md)             | Windows development host, networking             |
| [Nix install and flake outputs](docs/nix.md) | Nix install and flake outputs                    |

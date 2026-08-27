# Jetstream Example Apps

Build portable app images from `apps/`, deploy to a Jetstream device (for example a ZedBox), and run them alongside other appliance apps.

`hello-ouster-terminal` is the simplest lidar app: a terminal-based Ouster CLI viewer. The remaining examples cover HTTP, GPU, Firefox, pub/sub, web UI, Slidev, LLM, and profiling tools.

| App                     | Description                                          |
| ----------------------- | ---------------------------------------------------- |
| `hello-firefox`         | Firefox on Wayland                                   |
| `hello-firefox-kiosk`   | Firefox kiosk                                        |
| `hello-http`            | Python HTTP on 8000                                  |
| `hello-llm`             | Chat UI + Ollama (host CUDA via PortableGPU)         |
| `hello-ouster-terminal` | Terminal lidar viewer (ouster-cli viz)               |
| `hello-ouster-world`    | SimpleViz + Display; FrameSet / OSF / sensor sources |
| `hello-pub`             | Event publisher demo                                 |
| `hello-sub`             | Event subscriber demo                                |
| `hello-slidev`          | Slidev presentation demo                             |
| `hello-wayland-gpu`     | Vulkan / OpenGL GPU demos                            |
| `hello-webui`           | Static web UI demo                                   |
| `hello-world`           | Minimal oneshot                                      |
| `nvidia-devtools`       | Nsight / CUDA profiling and debug tools              |

## Quick start

1. Complete the [Linux host setup](docs/linux.md).
2. Clone this repository.
3. Build an app.
4. Upload to the web UI.

From `apps/`:

**Build** (image on disk only, for web UI or app distribution):

```bash
cd jetstream-example-apps/apps
nix build .#hello-world-cross-aarch64-linux
```

**Build + Deploy** (normal workflow — build for the device, upload, sample logs):

```bash
cd jetstream-example-apps/apps
./scripts/jsd-app-deploy.sh hello-world http://<device>/
```

Full procedures: [Developer Guide](docs/developer-guide.md).

## Documentation

| Guide                                        | Contents                                         |
| -------------------------------------------- | ------------------------------------------------ |
| [Developer Guide](docs/developer-guide.md)   | Build, deploy, and auth workflows                |
| [Linux host setup](docs/linux.md)            | Packages and Nix                                 |
| [WSL 2 host setup](docs/wsl2.md)             | Windows development host, networking             |
| [Nix install and flake outputs](docs/nix.md) | Nix install and flake outputs                    |

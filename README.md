# jetstream-demo-apps

Customer-facing **Jetstream** demo apps. Build portable app images from `apps/`, deploy to a Jetstream device (for example a ZedBox), and run them alongside other appliance apps.

`hello-ouster-world` is the lidar viz demo (SimpleViz, OSF / live sensor / a peer FrameSet socket). The other `hello-*` apps are smaller examples of the same packaging: HTTP, GPU, Firefox, Ouster CLI viz, pub/sub, and so on.

## Apps

| App | Role |
|-----|------|
| `hello-world` | Minimal oneshot |
| `hello-http` | Python HTTP on 8000 |
| `hello-wayland-gpu` | Sway + vkcube / glmark2 |
| `hello-firefox` | Sway + Firefox |
| `hello-firefox-kiosk` | Cage + Firefox kiosk |
| `hello-ouster-terminal` | Sway + foot + ouster-cli viz |
| `hello-ouster-world` | SimpleViz + Display; FrameSet / OSF / sensor sources |
| `hello-llm` | Chat UI + Ollama (host CUDA via PortableGPU) |
| `nvidia-devtools` | Nsight / CUDA profiling and debug tools (host libcuda) |
| `hello-webui` | Static web UI demo |
| `hello-pub` | Event publisher demo |
| `hello-sub` | Event subscriber demo |

### hello-ouster-world

Lidar viz. SDK matches `hello-ouster-terminal`. Get recordings from the Display UI (**Download** an example or **Upload** an `.osf`). Optional peer: **Ouster Dynamo** — proprietary app, installed separately, not built from this repo — publishes on `/run/ouster-dynamo/frameset.sock`.

## Build and deploy

From `apps/`. Nix install and config: [docs/nix.md](docs/nix.md).

**Build** the portable image (native, or cross for an aarch64 device):

```bash
nix build .#hello-ouster-world
nix build .#hello-ouster-world-cross-aarch64-linux
```

**Deploy** with `./scripts/jsd-app-deploy.sh`. That script also nix-builds the
matching target for the device, then uploads it:

```bash
./scripts/jsd-app-deploy.sh hello-ouster-world http://HOST/
```

Same pattern for the other apps.

## Specs

Layout and packaging: [specs/](specs/). SDK/viz details: [specs/sdk.md](specs/sdk.md).

# nvidia-devtools

> **This app does not work yet.** Do not treat it as a supported example until it is fixed.

NVIDIA CUDA profiling tools with SSH shell and a web UI.

## Access

Open **NVIDIA DevTools** in the Jetstream nav (`/app/nvidia-devtools`) for SSH commands and authorized keys. No keys are seeded; paste a public key in the UI before `ssh`.

```bash
ssh -p 2223 js@zedbox-mini-nx-….local
ncu --version
nsys --version
tegrastats --interval 1000
sudo nsys profile --pid <PID> --duration 30
```

`libcuda` comes from the host via `X-Jetstream-PortableGPU=cuda`.

## Build and deploy

From `apps/`:

```bash
nix build .#nvidia-devtools-cross-aarch64-linux
teka app deploy --build nvidia-devtools --host http://HOST/
```

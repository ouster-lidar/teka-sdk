# Ouster SDK (demo)

HDMI GPU apps use Sway and host `/run/opengl-driver` (`X-Jetstream-PortableGPU`). Pattern: `hello-ouster-terminal`.

## Viz: follow hello-ouster-terminal

- `ouster-sdk = ps.ousterSdk` → `apps/nix/ouster-sdk.nix`
- Source build against **nixpkgs** cmake, nanobind, and GLFW
- Flake input: public `github:ouster-lidar/ouster-sdk` v1.0.1
- hello-ouster-world peer sockets: `apps/nix/osf-socket/osf_socket.py` on the
  app PYTHONPATH (wraps `open_source`; public 1.0.1 has no OSF_SOCKET)
- Python from the one nixpkgs pin
- `GLFW_PLATFORM=wayland` via `ouster-portable-common.nix` (`wayland = true`)
- `ouster-cli source … viz`

hello-ouster-world uses the same SDK and GLFW.

## nixpkgs first

Packages, GLFW, Python, Firefox, and the SDK source build come from the single nixpkgs pin. Prefer a source build; use a wheel only as a last resort (one artifact, with a comment explaining why). hello-llm and hello-ouster-world follow the same host-GPU split: **`libcuda` from the host** (`X-Jetstream-PortableGPU`); CUDA **plugins** (e.g. hello-llm’s ollama plugin) may live in the DDI, but **L4T BSP does not** — do not use `config.cudaSupport` / `cudaCapabilities` on cross builds.

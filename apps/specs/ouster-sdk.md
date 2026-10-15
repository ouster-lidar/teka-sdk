# Ouster SDK integration

HDMI GPU apps use Sway and host `/run/opengl-driver` (`X-Jetstream-PortableGPU`). The two Ouster SDK viewers build the public SDK source as part of their Nix closures. `hello-ouster-world` is the direct sensor and OSF viewer; `hello-ouster-dynamo` retains the advanced peer FrameSet workflow.

## Viz: follow hello-ouster-terminal

- `ouster-sdk = ps.ousterSdk` → `apps/nix/ouster/ouster-sdk.nix`
- Source build against **nixpkgs** cmake, nanobind, and GLFW
- Flake input: public `github:ouster-lidar/ouster-sdk` v1.0.1
- `hello-ouster-world` imports `open_source` directly for sensor and OSF sources.
- `hello-ouster-dynamo` uses `apps/examples/hello-ouster-dynamo/osf-socket/osf_socket.py` for peer FrameSet sockets; public SDK 1.0.1 has no `IoType.OSF_SOCKET`.
- Python from the one nixpkgs pin
- `hello-ouster-terminal` sets `GLFW_PLATFORM=wayland` through its local `ouster-app-common.nix` (`wayland = true`).
- `hello-ouster-terminal` uses `ouster-cli source … viz`.

## nixpkgs first

- Use the single nixpkgs pin for packages, GLFW, Python, and the SDK source build.
- Prefer a source build.
- Use a wheel only as a last resort, with a comment explaining why.

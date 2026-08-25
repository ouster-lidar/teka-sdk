# Ouster SDK integration

HDMI GPU apps use Sway and host `/run/opengl-driver` (`X-Jetstream-PortableGPU`).
Pattern: `hello-ouster-terminal`.

## Viz: follow hello-ouster-terminal

- `ouster-sdk = ps.ousterSdk` → `apps/nix/ouster-sdk.nix`
- Source build against **nixpkgs** cmake, nanobind, and GLFW
- Flake input: public `github:ouster-lidar/ouster-sdk` v1.0.1
- hello-ouster-world peer sockets: `apps/nix/osf-socket/osf_socket.py` on the app PYTHONPATH (wraps `open_source`; public 1.0.1 has no OSF_SOCKET)
- Python from the one nixpkgs pin
- `GLFW_PLATFORM=wayland` via `ouster-portable-common.nix` (`wayland = true`)
- `ouster-cli source … viz`

## nixpkgs first

- Use the single nixpkgs pin for packages, GLFW, Python, and the SDK source build.
- Prefer a source build.
- Use a wheel only as a last resort, with a comment explaining why.

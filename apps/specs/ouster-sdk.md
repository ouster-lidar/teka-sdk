# Ouster SDK integration

HDMI GPU apps use Sway and host `/run/opengl-driver` (`X-Teka-PortableGPU`). The two Ouster SDK viewers build the public SDK source as part of their Nix closures. `hello-ouster-world` is the direct sensor and OSF viewer; `hello-ouster-dynamo` retains the advanced peer FrameSet workflow.

## Viz: follow hello-ouster-terminal

- `ouster-sdk = ps.ousterSdk` → `pkgs.python3Packages.ouster-sdk`
- Source build against **nixpkgs** cmake, nanobind, and GLFW
- `hello-ouster-world` imports `open_source` directly for sensor and OSF sources.
- `hello-ouster-dynamo` uses `apps/examples/hello-ouster-dynamo/osf-socket/osf_socket.py` for peer FrameSet sockets; public SDK 1.0.1 has no `IoType.OSF_SOCKET`.
- `hello-ouster-terminal` sets `GLFW_PLATFORM=wayland` in `scripts/viz-loop.sh.in`.
- `hello-ouster-terminal` uses `ouster-cli source … viz`.

## nixpkgs first

- Use the same package set for GLFW, Python, and the SDK.
- Prefer a source build.
- Use a wheel only as a last resort, with a comment explaining why.

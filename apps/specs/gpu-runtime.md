# GPU runtime integration

GPU app images get host GPU libraries through `X-Jetstream-PortableGPU`; the DDI does not include the L4T BSP.

## Wayland GPU apps

- Use `X-Jetstream-PortableGPU=wayland` for Sway and other Wayland GPU clients.
- App images build Sway and Cage with **Xwayland disabled** (`nix/wayland-compositors-overlay.nix`); closures must not ship an `xwayland` binary. X11 clients are out of scope.
- The host provides DRM, Vulkan, EGL, GBM, and `/run/opengl-driver`.
- OpenGL and Vulkan clients can share the runtime: `glmark2-wayland` exercises OpenGL, while `vkcube --wsi wayland` exercises Vulkan.
- When wlroots manages local input, use `WLR_BACKENDS=drm,libinput`; `drm` alone disables input.

### Image size without Xwayland (`aarch64-linux` cross DDI)

Device images use `nix build .#<service>-cross-aarch64-linux` erofs `.raw` outputs. Compared to `main` @ `1dca095` (runtime `xwayland disable` only) vs compositor packages built with Xwayland off (2026-09-29):

| Service | Before | After | Δ |
|---------|-------:|------:|--:|
| `hello-wayland-gpu` | 271.7 MiB | 233.1 MiB | **−38.5 MiB (−14%)** |
| `hello-ouster-terminal` | 410.6 MiB | 407.8 MiB | −2.8 MiB |
| `hello-firefox` / `hello-firefox-kiosk` | ~692 / ~683 MiB | ~690 / ~681 MiB | ~−2.6 MiB each |
| `hello-ouster-world` | 612.5 MiB | 609.9 MiB | −2.6 MiB |

Largest savings are on compositor-heavy images (no `xwayland` store path, smaller wlroots/sway). Firefox- and SDK-heavy closures change little because the compositor slice is a small fraction of the tree.

For CUDA apps:

- `libcuda` comes from the host through `X-Jetstream-PortableGPU=cuda`.
- CUDA plugins, such as hello-llm’s Ollama plugin, may live in the DDI.
- Do not use `config.cudaSupport` or `cudaCapabilities` on cross builds.

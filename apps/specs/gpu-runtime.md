# GPU runtime integration

GPU app images get host GPU libraries through `X-Teka-PortableGPU`; the DDI does not include the L4T BSP.

## Wayland GPU apps

- Use `X-Teka-PortableGPU=wayland` for Sway and other Wayland GPU clients.
- App images build Sway and Cage with **Xwayland disabled** (`nix/overlays/wayland-compositors-overlay.nix`); closures must not ship an `xwayland` binary. X11 clients are out of scope.
- The host provides DRM, Vulkan, EGL, GBM, and `/run/opengl-driver`.
- OpenGL and Vulkan clients can share the runtime: `glmark2-wayland` exercises OpenGL, while `vkcube --wsi wayland` exercises Vulkan.
- When wlroots manages local input, use `WLR_BACKENDS=drm,libinput`; `drm` alone disables input.

For CUDA apps:

- `libcuda` comes from the host through `X-Teka-PortableGPU=cuda`.
- CUDA plugins, such as hello-llm’s Ollama plugin, may live in the DDI.
- Do not use `config.cudaSupport` or `cudaCapabilities` on cross builds.

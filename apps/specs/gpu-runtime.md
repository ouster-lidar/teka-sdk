# GPU runtime integration

GPU app images get host GPU libraries through `X-Jetstream-PortableGPU`; the DDI does not include the L4T BSP.

## Wayland GPU apps

- Use `X-Jetstream-PortableGPU=wayland` for Sway and other Wayland GPU clients.
- The host provides DRM, Vulkan, EGL, GBM, and `/run/opengl-driver`.
- OpenGL and Vulkan clients can share the runtime: `glmark2-wayland` exercises OpenGL, while `vkcube --wsi wayland` exercises Vulkan.
- When wlroots manages local input, use `WLR_BACKENDS=drm,libinput`; `drm` alone disables input.

For CUDA apps:

- `libcuda` comes from the host through `X-Jetstream-PortableGPU=cuda`.
- CUDA plugins, such as hello-llm’s Ollama plugin, may live in the DDI.
- Do not use `config.cudaSupport` or `cudaCapabilities` on cross builds.

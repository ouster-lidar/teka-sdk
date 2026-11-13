# GPU and Display

Your app uses the box's own GPU drivers. To turn on GPU access, add one line to the [Unit] section of your service.

| Add this line | For |
| --- | --- |
| X-Teka-PortableGPU=cuda | CUDA and other GPU work |
| X-Teka-PortableGPU=wayland | Apps that draw on the screen |

## Example

```ini
[Unit]
Description=My CUDA app
X-Teka-PortableGPU=cuda

[Service]
ExecStart=/usr/bin/my-cuda-app

[Install]
WantedBy=teka-apps.target
```

## Apps that use the screen

- Only one app gets the screen at a time. Starting a new one stops the old one.
- Set only one screen app to start on boot.
- Bring your own Wayland compositor, like Sway or Cage.

Want a working one? Check out `hello-llm` (CUDA) and `hello-wayland-gpu` (screen) in [App Examples](../02_app-examples.md).

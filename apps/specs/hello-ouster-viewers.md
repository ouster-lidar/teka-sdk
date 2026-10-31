# Ouster viewer apps

`hello-ouster-world` and `hello-ouster-dynamo` are separate Teka apps. The terminal viewer remains an independent CLI example.

| App | Role |
| --- | --- |
| `hello-ouster-world` | Direct Ouster SDK visualization of OSF files and sensors, with HDMI and browser display, recording, and sensor discovery. |
| `hello-ouster-dynamo` | Optional advanced workflow with FrameSet input and tracked-object overlays. Ouster Dynamo is proprietary software installed separately. |
| `hello-ouster-terminal` | Terminal-based `ouster-cli viz` example. |

## World source and discovery contract

A new world installation starts enabled in sensor mode with an empty host and recording disabled. The backend discovers sensors through Teka's `sensors` app roster WebSocket, including when no browser is open. A sensor already configured to stream to this device is selected immediately. Otherwise, it waits thirty seconds after receiving a fresh roster snapshot before automatically applying a sole usable sensor. If discovery reports no usable sensors, the app keeps manual hostname entry available. The roster remains available while a source is stopped, selected, or playing an OSF. Only an enabled sensor source without a hostname may select a sole candidate automatically. If discovery is unavailable or malformed, it shows that failure separately and continues retrying. Multiple usable sensors without a preferred destination require an explicit picker selection; roster changes do not change an active selection. A disconnected roster stream is not an empty roster.

The selected sensor is reset to supported measurement defaults before NORMAL streaming is configured to the app's UDP destination and ports 7502/7503. Sensor HTTP connection and timeout failures retry every two seconds for up to sixty seconds so a saved target can recover while the network starts. Apply or Stop interrupts the retry wait; process shutdown remains immediate. Other configuration failures, or exhausted retries, remain visible and require an explicit Apply or another selection. The same setup applies to a saved sensor target. OSF playback and direct-sensor recording remain available alongside Apply and Stop.

An existing world configuration keeps a deliberate OSF selection, configured sensor hostname, and stopped state. A missing configuration receives the new sensor default. Invalid or unsupported configuration is reported and waits for an explicit Apply instead of silently selecting a sensor.

## Migration from the previous advanced demo

Stop both viewers before moving state. Move the old viewer's state directory to `/var/lib/hello-ouster-dynamo/` for the preserved advanced workflow. Start `hello-ouster-world` with its new state directory so it receives the direct sensor default; do not copy display state into it or reinterpret an old FrameSet/Dynamo configuration as direct sensor ingest. Deploy the two viewers manually on an existing Teka host, and switch display apps when testing so they do not compete for the DRM seat or sensor UDP ports.

# OSF socket helpers (public SDK)

Public ouster-sdk **v1.0.1** has no `IoType.OSF_SOCKET`. This module provides the peer FrameSet Unix-socket publisher/consumer used by `hello-ouster-dynamo`.

- `osf_socket.py` — `OsfSocketPublisher`, `OsfSocketFrameSetSource`, and `open_source()` (SDK `open_source` plus `.sock` / `osf+unix://`)
- The SDK tree is not patched. `hello-ouster-world` uses SDK `open_source` directly for its OSF and sensor sources.

Delete this when the public SDK can open peer sockets natively.

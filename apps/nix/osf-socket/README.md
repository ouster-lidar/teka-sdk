# OSF socket helpers (public SDK)

Public ouster-sdk **v1.0.1** has no `IoType.OSF_SOCKET`. This module is the
peer FrameSet Unix-socket publisher/consumer used by hello-ouster-world.

- `osf_socket.py` — `OsfSocketPublisher`, `OsfSocketFrameSetSource`, and
  `open_source()` (SDK `open_source` plus `.sock` / `osf+unix://`)
- hello-ouster-world installs the file on `PYTHONPATH` and imports
  `open_source` from here. The SDK tree is not patched.

Delete this when the public SDK can open peer sockets natively.

# Peer FrameSet sockets

Jetstream apps can publish live Ouster SDK `FrameSet` streams to peer apps on the same device using a Unix-domain socket. Consumers open the path with `osf_socket.open_source()` (SDK `open_source` plus `.sock` paths).

## Path convention

| Role | Path |
|------|------|
| Producer app `<app-id>` | `/run/<app-id>/frameset.sock` |
| Override (optional) | env `<APP>_FRAMESET_SOCK` or app config |

`RuntimeDirectory=<app-id>` in the app unit provides `/run/<app-id>/`.

## SDK API

- **Publisher:** `osf_socket.OsfSocketPublisher(path)` → `start()` / `publish(frame_set, sensor_info)` / `close()`
- **Consumer:** `osf_socket.open_source("/run/<app-id>/frameset.sock")` (wraps the SDK; also accepts `osf+unix://…`)
- **CLI:** public `ouster-cli` does not open `.sock` paths until upstream ships OSF_SOCKET

Wire format: `uint32` LE length + single-frame OSF bytes (objects/tracks included). Latest-wins to one connected client.

Nothing in the protocol is producer-specific. **Ouster Dynamo** is proprietary Ouster software (installed separately; not built from this repo) and publishes on `/run/ouster-dynamo/frameset.sock`. The advanced `hello-ouster-dynamo` viewer consumes that stream. The direct `hello-ouster-world` viewer opens OSF files or sensors itself and does not use FrameSet sockets.

**Object list layout** (when a producer embeds tracks on the FrameSet) is outside this transport spec. For ouster-dynamo V1 (`trk_objs`, `ego_obj`, `ouster_dynamo_api_version` on a `jetstream` meta object), see [`hello-ouster-dynamo/dynamo-utils/ouster_dynamo_api_README.md`](../examples/hello-ouster-dynamo/dynamo-utils/ouster_dynamo_api_README.md).

## Producer snippet

```python
from osf_socket import OsfSocketPublisher

pub = OsfSocketPublisher("/run/my-app/frameset.sock")
pub.start()
for frame_set in source:
    # ... process ...
    pub.publish(frame_set, source.sensor_info)
pub.close()
```

## Consumer snippet

```python
from osf_socket import open_source
from ouster.sdk.viz import SimpleViz

source = open_source("/run/my-app/frameset.sock")
SimpleViz(source.sensor_info, rate=None, on_eof="stop").run(source)
```

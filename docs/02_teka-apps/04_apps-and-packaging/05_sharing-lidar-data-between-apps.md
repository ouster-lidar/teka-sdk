# Sharing Lidar Data Between Apps

Apps on the same box can share live lidar data. One app publishes Ouster SDK frames on a local socket. Other apps read them like any Ouster SDK source.

| Who publishes | Socket |
| --- | --- |
| Your app | `/run/\<app-id>/frameset.sock` |

## Publish frames

Add `RuntimeDirectory=\<app-id>` to your service, then publish:

```python
from osf_socket import OsfSocketPublisher

pub = OsfSocketPublisher("/run/my-app/frameset.sock")
pub.start()
for frame_set in source:
    pub.publish(frame_set, source.sensor_info)
pub.close()
```

## Read frames

```python
from osf_socket import open_source
from ouster.sdk.viz import SimpleViz

source = open_source("/run/my-app/frameset.sock")
SimpleViz(source.sensor_info).run(source)
```

Readers always get the newest frame. If a reader falls behind, it skips frames instead of lagging.

> **Note:**
> You'll find `osf_socket.py` in the app examples. Add it to your app until the Ouster SDK supports sockets on its own.

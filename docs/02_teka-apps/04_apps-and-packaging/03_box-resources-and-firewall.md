# Box Resources and Firewall

Your app borrows network ports and other resources while it runs. TekaOS takes them back when it stops.

## Ask for resources

When your app starts, it checks in with TekaOS:

1. Call POST `/api/v1/manager/runtime/\<id>/activate`. If your manifest asks for one, you'll get a local HTTP port back.
2. Keep GET `/api/v1/manager/runtime/\<id>/register` open while the app runs.
3. Open the network ports you need.
4. When the app stops, call POST `/api/v1/manager/runtime/\<id>/release`.

These calls only work from the box itself. You'll find the base URL in TEKA_RUNTIME_URL.

## Open network ports

Managed services can only use loopback by default. To listen on the box's network interfaces, set `services.main.runtime.network = "host"`.

The firewall blocks incoming traffic until you ask. For example, to get lidar data on UDP 7502 and 7503:

```text
PUT /api/v1/manager/runtime/my-app/ports
{ "ports": [ { "port": 7502, "proto": "udp" }, { "port": 7503, "proto": "udp" } ] }
```

The ports close when your app stops.

## Know who's calling

Set `auth.scheme` to `teka-proxy-identity` and `auth.version` to `0.1.0` to opt into proxy identity. TekaOS sends the caller's principal ID in the `X-Teka-Principal-Id` header when the caller has a session.

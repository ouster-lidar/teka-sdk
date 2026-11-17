# System Update

System Update installs a new TekaOS version. After the box restarts, this page shows **Verifying update** until the new version is kept, then **Update complete**. Learn more in [Update and Recover a Box](../05_update-tekaos/02_update-and-recover.md).

Device ID, factory reset, firmware, and power are on **General**, under **System Utilities**.

## What you'll see

- Your TekaOS version
- How the last update went
- A file chooser and **Apply**, to install an update from this page

## Use the CLI

```bash
teka system status --box teka-<serial>.local
teka system update <update>.tpk --wait --box teka-<serial>.local
```

Learn more in [Teka CLI](../03_teka-cli.md).

## Use the API

The System Update page calls **General** for these routes.

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/app/teka/status` | Gets the version and update state. |
| POST | `/api/v1/app/teka/sysupdate/apply` | Installs an update file. |
| POST | `/api/v1/app/general/firmware/apply` | Installs new firmware. |
| POST | `/api/v1/app/general/host/reboot` | Restarts the box. |
| POST | `/api/v1/app/general/host/poweroff` | Powers off the box. |
| POST | `/api/v1/app/general/host/factory-reset` | Resets the box. |

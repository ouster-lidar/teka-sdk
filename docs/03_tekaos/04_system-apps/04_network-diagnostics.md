# Network Diagnostics

Network Diagnostics tells you whether your box's network can keep up with lidar traffic.

## Get started

1. Start **Network Diagnostics** from **System Utilities**.
2. Open it and pick a network port.
3. Check the link speed, error counts, and queues. They update every second.

## Use the API

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/app/net-diag/diagnostics/interfaces` | Lists network ports. |
| GET | `/api/v1/app/net-diag/diagnostics/interfaces/\<port>` | Gets stats for one port. |
| GET | `/api/v1/app/net-diag/diagnostics/client` | Gets box and system info. |

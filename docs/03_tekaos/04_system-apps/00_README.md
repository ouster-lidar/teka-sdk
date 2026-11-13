# TekaOS System Utilities

System utilities come with TekaOS and run your box: sensors, networking, updates, and more. You'll find the ones that are running under **System Utilities** in the sidebar.

## Get around the web interface

The top bar runs across the window. It shows the TekaOS logo, the hostname, and **Account**. Click the edit icon next to the hostname to rename the box.

The sidebar is a column on the left:

- **Home** is the TekaOS background.
- **Apps** lists the apps you install, in alphabetical order. A stopped app stays in the list. Click **+** to install one.
- **System Utilities** lists the utilities that are running. A stopped utility leaves the sidebar. Open **System Utilities** to start it again.

## System utilities

| Utility | What it does |
| --- | --- |
| General | Device ID, host facts, factory reset, firmware, and power. |
| [Sensors](01_sensors.md) | Finds, checks, and tests your Ouster sensors. |
| [Network](02_dhcp-and-dns.md) | Sets addresses and Wi-Fi, and gives your sensors DHCP. |
| [Time Sync (PTP)](03_time-sync-ptp.md) | Keeps sensor clocks in sync with the box. |
| [Network Diagnostics](04_network-diagnostics.md) | Checks your network can keep up with lidar traffic. |
| [UDP Relay](05_udp-relay.md) | Forwards sensor data to other computers. |
| [Packet Monitor and Simulator](06_packet-monitor-and-simulator.md) | Watches sensor traffic and sends test traffic. |
| [System Update](07_system-update.md) | Installs a TekaOS update. |

Network and System Update start on boot. Time Sync, Network Diagnostics, UDP Relay, and Packet Monitor and Simulator stay off until you start them from **System Utilities**.

## Use the APIs

Each utility has its own API at `/api/v1/app/\<id>/`, like `/api/v1/app/network/dhcp/leases`. Sign in the same way as in [Using the App API](../../02_teka-apps/04_apps-and-packaging/02_using-the-app-api.md).

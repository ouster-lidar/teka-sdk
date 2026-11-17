# Network

Network sets the addresses on your box and, on boxes with Wi-Fi, runs the radio as a hotspot or as a client. The same page gives addresses to the sensors plugged into the box.

Open **Network** under **System Utilities**.

## Set a static address

Ethernet ports use DHCP until you say otherwise. To plug sensors straight into the box, give that port a static address:

1. On **Network**, open **Configure…** for the port.
2. Choose static addressing. We'll fill in a free address, like 10.42.0.1/24.
3. Click **Apply**.

To go back to DHCP, click **Reset**. To give your sensors addresses, use the **DHCP** card on the same page.

## Wi-Fi

A Wi-Fi radio is one or the other: **AP (publish a hotspot)** or **Client (connect to existing network)**. Each row has its own switch. Turning one on turns the other off.

Need a laptop in the field? Turn on the hotspot. The box is at 10.41.0.1 on that network, and the network name starts as the hostname.

To join a network you already have, turn on **Client**, scan, pick the network, and join it. That disconnects the hotspot. **Forget** drops the saved network and leaves the hotspot off.

> **Note:**
> LTE is coming soon.

## DHCP

The **DHCP** card hands addresses to the sensors on a port. It can do that all the time, or only when nothing else on the network does.

| Mode | What happens |
| --- | --- |
| Off | TekaOS leaves the port alone. |
| Fallback | The default when **Fallback DHCP** is on. TekaOS steps in only when no other DHCP server answers, and steps back when one returns. |
| Manual | TekaOS always hands out addresses on the port. |

1. Make sure the port has a static address, like 10.42.0.1/24.
2. On the **DHCP** card, pick **Manual** for that port. TekaOS starts handing out addresses right away.

Your sensors get addresses from 10.42.0.100 to 10.42.0.200. You'll see each one in the lease list.

If a sensor sends its hostname, you can reach it from the box at `<hostname>.jet`.

## Use the API

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/app/network/devices` | Lists ports and their settings. |
| PUT | `/api/v1/app/network/ethernet/\<port>` | Sets a port's address. |
| DELETE | `/api/v1/app/network/ethernet/\<port>` | Puts a port back on DHCP. |
| PUT | `/api/v1/app/network/wifi/\<port>/ap` | Starts the hotspot. |
| PUT | `/api/v1/app/network/wifi/\<port>/client` | Joins a Wi-Fi network. |
| GET | `/api/v1/app/network/dhcp/status` | Gets the DHCP state. |
| GET | `/api/v1/app/network/dhcp/leases` | Lists DHCP leases. |
| POST | `/api/v1/app/network/dhcp/start` | Starts handing out addresses. |
| POST | `/api/v1/app/network/dhcp/stop` | Stops handing out addresses. |

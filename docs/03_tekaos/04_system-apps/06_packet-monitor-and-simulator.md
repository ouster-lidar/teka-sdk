# Packet Monitor and Simulator

This app helps you test sensor networks. **Monitor** shows the sensor traffic coming in. **Simulator** sends test traffic, so you can test a network without a sensor.

## Watch traffic

1. Start the app from **System Utilities** and open **Monitor**.
2. Enter the ports to watch, like 7502 and 7503, and start.
3. Check each stream's data rate and packet count.

## Send test traffic

1. Open **Simulator**.
2. Set a destination, like 192.168.1.20:7502, and a data rate.
3. Start the stream. Click **Stop all** when you're done.

> [!note]
> The simulator sends real traffic. Keep it off production networks.

## Use the API

| Method | Path | What it does |
| --- | --- | --- |
| POST | `/api/v1/app/net-sim-mon/packet-monitor/start` | Starts watching ports. |
| POST | `/api/v1/app/net-sim-mon/packet-monitor/stop` | Stops watching. |
| POST | `/api/v1/app/net-sim-mon/simulator/start` | Starts a test stream. |
| POST | `/api/v1/app/net-sim-mon/simulator/stop-all` | Stops all test streams. |

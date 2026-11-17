# UDP Relay

UDP Relay forwards your sensor data to other computers. Use it to send one sensor's data to several computers, or to a computer on another network.

## Set up a relay

1. Start **UDP Relay** from **System Utilities**.
2. Open it and add a listen port, like 7502.
3. Add one or more destinations, like 192.168.1.20:7502.
4. Start the relay. Turn on **auto_enable** if you want it to start with the box.

## Use the API

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/app/relay/status` | Gets your relays and traffic counts. |
| POST | `/api/v1/app/relay/config` | Sets your list of relays. |
| POST | `/api/v1/app/relay/start` | Starts the relays. |
| POST | `/api/v1/app/relay/stop` | Stops the relays. |

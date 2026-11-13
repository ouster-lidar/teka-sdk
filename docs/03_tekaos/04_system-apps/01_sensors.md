# Sensors

Sensors finds every Ouster lidar sensor on your box's networks. From here you can check a sensor, test it, or reset its network settings.

## Get started

1. Open **Sensors**. You'll see your sensors under **Sensors Found on Network**.
2. Pick a sensor and click **Connect**.
3. Pick a connected sensor to see its data and details.

## Run a self test

If a sensor supports self tests, you can run them from its page. One test runs at a time.

## Reset network settings

Sensor on the wrong address for your network? Click **Reset Config**. The sensor restarts its network, and TekaOS finds it again on its own.

## Use the API

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/app/sensors/devices` | Lists your sensors. |
| GET | `/api/v1/app/sensors/devices/\<serial>` | Gets one sensor's details. |
| POST | `/api/v1/app/sensors/devices/\<serial>/self_test` | Starts a self test. |
| DELETE | `/api/v1/app/sensors/devices/\<serial>/config` | Resets the sensor's network settings. |

```bash
curl -b cookies.txt http://teka-<serial>.local/api/v1/app/sensors/devices
```

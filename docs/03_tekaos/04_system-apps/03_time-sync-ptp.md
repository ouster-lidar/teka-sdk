# Time Sync (PTP)

Time Sync makes your box the clock for your Ouster sensors, so every sensor agrees on the time.

## Get started

1. Start **Time Sync** from **System Utilities**.
2. Open it and pick the Ethernet ports your sensors are on.
3. Click **Start**. Turn on **auto_enable** if you want PTP to start with the box.

The status table shows your box as the leader and each sensor as a follower, with how far off its clock is.

> **Note:**
> Turn on PTP on each sensor too.

## Settings

| Setting | Options |
| --- | --- |
| Profile | default or gptp |
| PTP version | 2.0 or 2.1 |
| Priority 1, Priority 2 | 128 unless you change it |

## Use the API

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/app/ptp/status` | Gets the current state and your sensors. |
| POST | `/api/v1/app/ptp/start` | Starts PTP on one or more ports. |
| POST | `/api/v1/app/ptp/stop` | Stops PTP. |

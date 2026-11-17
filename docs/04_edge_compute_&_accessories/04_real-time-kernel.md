# Real-Time Kernel on TekaOS

TekaOS doesn't support a real-time (PREEMPT_RT) kernel yet. Real-time kernel options and scheduling are on the roadmap.

## What you can use today

- Time Sync (PTP): makes the ZED Box Mini the clock for your Ouster sensors, so every sensor agrees on the time. Start it from Apps in the web interface.
- Dedicated GPU access: CUDA apps use the box's own GPU drivers. See the [TekaOS documentation](../03_tekaos/00_README.md).

> **Warning:**
> Don't run the Stereolabs real-time flash scripts on a TekaOS box. They replace TekaOS with Ubuntu.

## Need real-time support?

Tell your Ouster contact what latency and timing your app needs. It helps us plan real-time support.

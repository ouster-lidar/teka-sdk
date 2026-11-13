# Getting Started with TekaOS

TekaOS is the operating system for perception apps on edge hardware. It provides a stable & secure platform, making it easy for you to build and deploy apps.  

Your box comes with TekaOS installed. You can have your first app running in a few minutes.

Setting up a ZED Box Mini or ZED Box 2? Start with the [ZED Box quick start](../01_getting-started/01_zed-box-with-tekaos.md).

## How Teka fits together

| Layer | What it is |
| --- | --- |
| TekaOS | The stable, secure OS on every Teka box. It's read-only, updates itself safely, and handles hardware, security, networking, and updates. It comes with lidar, detection, and SLAM engines built on Ouster SDK Core, so your app starts at perception, not drivers. |
| Teka Apps | Your apps, built by you or your agents. They run as sealed packages on top of TekaOS, so you can ship fast without putting the box at risk. |

## What you get

- **Built for uptime:** An immutable OS made for robotics and industrial applications. Apps can't break it, and a failed update rolls back on its own.
- **Zero-maintenance hardware stack:** No more debugging JetPack versions, CUDA drivers, or sensor networking.
- **One CLI for every box:** Build, deploy, and update apps with the Teka CLI. People, CI pipelines, and AI agents all use the same commands.
- **Ready for agents:** Apps are sealed from the OS, so you and your agents can iterate at full speed.
- **A web interface on every box:** See sensor data and manage the box from your browser.
- **Fleet-level scale:** Deploy apps to many boxes at once with one command.

## Secure and EU CRA ready

TekaOS is built to meet the EU Cyber Resilience Act (CRA), so your compliance work can focus on the thing you manage: your app.

- **Secure by design:** An immutable OS, with Secure Boot and encrypted storage coming to every box.
- **Continuous vulnerability management:** Ongoing security updates, plus an SBOM with every release.
- **Clear boundaries:** Ouster manages the OS. You manage your app.

## Get started

1. **Set up your ZED Box:** Power on, connect, and open the web interface. See [Getting Started](../01_getting-started/01_zed-box-with-tekaos.md).
2. **Check your box:** See [Specifications](01_specifications.md) for the software versions and what your computer needs.
3. **Set up your computer:** Install the Teka CLI on Linux, macOS, or Windows. See [Set Up a Development Host](02_set-up-a-development-host/00_README.md).
4. **Deploy your first app:** Try `hello-world`. See [App Examples](../02_teka-apps/02_app-examples.md).

## Tools

| Tool | Use it to |
| --- | --- |
| [Starting Your Own App](../02_teka-apps/01_starting-your-own-app.md) | Create a new app from the template. |
| [Teka CLI](03_teka-cli.md) | Connect to boxes, build and deploy apps, and update TekaOS from a terminal, a CI pipeline, or an AI agent. |
| [Web interface](04_system-apps/00_README.md) | Set up one box and manage it from your browser. |

## Works with

| Product | What it does |
| --- | --- |
| Ouster SDK | Read, view, and record lidar data in your app. |
| NVIDIA JetPack | TekaOS for Jetson is built on JetPack 7.2 and CUDA 13.2. |
| Docker | Turn most Docker images into Teka Apps. See [Teka Apps](../02_teka-apps/00_README.md). |
| Nix | Builds your apps the same way on every computer. See [Nix](../02_teka-apps/03_nix.md). |

## Next steps

- **New to TekaOS?** Start with [Getting Started](../01_getting-started/01_zed-box-with-tekaos.md).
- **Ready to build?** Go to [Teka Apps](../02_teka-apps/00_README.md).

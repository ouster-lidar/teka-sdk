# Teka Apps

Teka Apps are your apps, built by you or your agents. Each one is a sealed, read-only package with your program, its libraries, and a systemd service. Apps run on top of TekaOS, so installing or removing one never changes the OS. You can ship fast without putting the box at risk.

**New to the box?** Start with [Getting Started](../01_getting-started/01_zed-box-with-tekaos.md). **Ready to build?** Install the Teka CLI in [Set Up a Development Host](../03_tekaos/02_set-up-a-development-host/00_README.md), then follow [Starting your own app](01_starting-your-own-app.md).

## A few rules

- **App ID:** Letters, numbers, - and _ only, up to 64 characters. Don't start it with mc-. That's for system apps.
- **Service name:** Call your main service `\<app-id>.service`.
- **Start on boot:** Add WantedBy=teka-apps.target to the [Install] section.
- **Data:** Save data with StateDirectory=. It survives updates. A factory reset erases it.

## Bring your Docker

Already have a Docker image? You can turn most images into a Teka App without rewriting your Dockerfile or changing your CI. One-command import is coming to the [Teka CLI](../03_tekaos/03_teka-cli.md) soon.

## Build with agents

Teka Apps are a safe place for AI agents to work.

- **Safe by structure:** Apps are sealed from the read-only OS, so an agent-built app can't break the box.
- **Guardrails built in:** Pinned toolchains and verified deploys keep agents on track.
- **Spec-first:** Have agents write requirements before code, so you can review every change.

## In this section

- [Starting your own app](01_starting-your-own-app.md)
- [App examples](02_app-examples.md)
- [Nix](03_nix.md)
- [App manifest](04_apps-and-packaging/01_app-manifest.md)
- [Using the App API](04_apps-and-packaging/02_using-the-app-api.md)
- [Box resources and firewall](04_apps-and-packaging/03_box-resources-and-firewall.md)
- [GPU and display](04_apps-and-packaging/04_gpu-and-display.md)
- [Sharing lidar data between apps](04_apps-and-packaging/05_sharing-lidar-data-between-apps.md)
- [Error codes](04_apps-and-packaging/06_error-codes.md)

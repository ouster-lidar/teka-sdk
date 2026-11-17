# Update and Recover a Box

TekaOS keeps the version you're running and the one before it. If an update doesn't start cleanly, your box goes back to the old one. Your settings and apps stay put either way.

## How updates work

1. You send an update file to the box.
2. TekaOS writes the new version next to the current one.
3. The box restarts into the new version.
4. If everything starts, TekaOS keeps it. If not, the next restart takes you back.

To install an update, see [Update TekaOS on a Box](00_README.md).

## Update the firmware

When new UEFI firmware is out, **General** shows a **UEFI firmware** card. Click **Update UEFI firmware**, and the firmware installs on the next restart.

## Go back to an older version

Press Space while the box starts, then pick the older version from the boot menu. You'll need a screen to see the menu.

## Factory reset

A factory reset erases your settings, custom hostname, admin password, logs, and apps. TekaOS itself stays. Start it from **General**. For the full steps, see [Reset to factory settings](../../04_edge_compute_&_accessories/05_reset-to-factory-settings.md).

> **Warning:**
> You can't undo a factory reset. It also removes the system apps, like Sensors.

## If something goes wrong

- **An update failed:** Restart the box. It goes back to the last version that worked.
- **teka can't reach the box:** Check the Ethernet cable, then run `teka box list --discover`. See [Teka CLI](../03_teka-cli.md).
- **Nothing starts:** Contact Ouster support.

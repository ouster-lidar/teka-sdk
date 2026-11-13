# How to Update TekaOS on a Box

Update TekaOS from your computer with the Teka CLI. TekaOS replaces the whole system and keeps your settings and apps. If the new version doesn't start cleanly, the box goes back to the previous one.

> [!note]
> **Before you start:** Install the Teka CLI. See [Set Up a Development Host](../02_set-up-a-development-host/00_README.md).

## 01. Download the update

Download the update file for your box: \<box>-sysupdate-v\<version>.tpk. [TBD: download link.]

## 02. Select your box

Find the TekaOS boxes on your network:

```bash
teka box list --discover
```

Then select yours:

```bash
teka box select teka-<serial>
```

Enter your box's admin password when you're asked. teka saves it, so you won't need it again next time.

## 03. Check your version

```bash
teka system status
```

You'll see the TekaOS version your box runs and the state of its last update.

## 04. Install the update

```bash
teka system update <box>-sysupdate-v<version>.tpk --wait
```

teka sends the file, and the box restarts into the new version. With `--wait`, teka signs in again after the restart and prints the final status when the update is done.

If the box rolls back instead, teka exits with an error that lists the services that failed. Your box keeps running the version that worked.

## Install now, restart later

Use `--no-reboot` to install the update without restarting. The new version starts the next time the box restarts.

```bash
teka system update <box>-sysupdate-v<version>.tpk --no-reboot
```

## Update several boxes

System commands work on one box at a time. Run the update once per box with `--box`:

```bash
teka system update <box>-sysupdate-v<version>.tpk --wait --box teka-box-1.local
teka system update <box>-sysupdate-v<version>.tpk --wait --box teka-box-2.local
```

## Next steps

- Update the firmware, roll back, or reset a box. See [Update and Recover](02_update-and-recover.md).
- See every system option in [Teka CLI](../03_teka-cli.md).

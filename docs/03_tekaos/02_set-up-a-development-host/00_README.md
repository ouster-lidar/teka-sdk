# How to Set Up a TekaOS Development Host

Now that your box is set up, build an app on your computer and run it on the box with the Teka CLI.

> **Note:**
> **Before you start:** Finish [Connect to your ZED Box](../../01_getting-started/02_connect-your-zed-box.md) first. You'll need your box's hostname and the admin password you created.

## 01. Set up your development computer

You can build on Ubuntu or Debian (Intel/AMD and Arm), Windows 11 with WSL 2, or a Mac with Apple silicon. Intel Macs aren't supported.

> **Note:**
> **Running Windows?** Set up [Windows (WSL 2)](02_windows-wsl-2.md) first, then come back here.

### Install the Teka CLI

Install the latest teka release:

```bash
curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | sh
```

The installer sets up the CLI and Nix if needed. For Windows, start with [WSL 2](02_windows-wsl-2.md). It runs `teka build init`, which configures public Nix caches. On Arm Linux, the installer isn't available. From a checkout of this repository, run `./bootstrap.sh` instead. From a downloaded `nix build .#teka.dist` binary, run `./teka-<system> build init` instead.

The first time, allow Nix to install, then open a new terminal. You may need your sudo password, or on a Mac your password, Touch ID, and **Allow** for the login keychain. The Mac build VM can take a few minutes the first time. See [macOS](01_macos.md).

### Restart your terminal

Close your terminal and open a new one so the changes take effect.

### Check

In your new terminal run:

```bash
teka build check
```

Every line ends in a check mark when your computer is ready to build. Then list the starter examples:

```bash
teka app list --examples
```

You'll see:

- `hello-world`
- `hello-webui`
- `hello-ouster-world`

## 02. Connect to your box

Check that your box is connected to your computer or network and powered on. Then find the TekaOS boxes on your network:

```bash
teka box list --discover
```

Select yours by part of its hostname or serial number:

```bash
teka box select teka-<serial>
```

Enter your box's admin password. teka saves it in macOS Keychain or the Linux Secret Service, so you won't need it next time. Run `teka box status` to check that teka can sign in. To remove a saved box and its password, run `teka box forget`.

## 03. Deploy your first app

Follow [Starting your own app](../../02_teka-apps/01_starting-your-own-app.md) to create an app and deploy it.

## Teka CLI at a glance

| Command | What it does |
| --- | --- |
| `teka box list` | Lists the boxes you've signed in to before. Add `--discover` to search your network. |
| `teka box select [PATTERN…]` | Signs in to boxes and makes them your default. |
| `teka box status` | Shows whether teka can reach and sign in to your selected boxes. |
| `teka box forget [PATTERN…]` | Removes boxes and their saved passwords. |
| `teka app init [DIR]` | Creates a new app in a folder, or in the current folder. Add `--example NAME` to start from a specific starter example. |
| `teka app list` | Lists the apps in the current folder's project. Add `--examples` to list the starter examples. |
| `teka app build [APP…]` | Builds app packages and prints their file paths. Omit the name for a project with one app. |
| `teka app deploy [INPUT…]` | Builds and deploys named apps or installs package files. Add `--box URL` to pick another box. |
| `teka system status` | Shows the box's TekaOS version and update state. |
| `teka system update FILE --wait` | Installs a TekaOS update and waits for it to finish. |
| `teka --help` | Shows every command and option. |

Learn more in [Teka CLI](../03_teka-cli.md).

## Next steps

- [Starting Your Own App](../../02_teka-apps/01_starting-your-own-app.md): Create a new app from a starter example.
- [App Examples](../../02_teka-apps/02_app-examples.md): Drive GPIO pins with `hello-gpio`, or view an OSF recording or a live Ouster sensor in your browser with `hello-ouster-world`.
- [Teka Apps](../../02_teka-apps/00_README.md): Build your own apps.
- [Using the App API](../../02_teka-apps/04_apps-and-packaging/02_using-the-app-api.md): Control apps on your box from scripts.

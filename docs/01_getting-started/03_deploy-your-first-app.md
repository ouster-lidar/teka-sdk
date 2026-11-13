# Deploy your first app on your ZED Box

Now that your ZED Box is set up, build an app on your computer and run it on the box with the teka command-line tool.

> [!note]
> Finish Setup first. You'll need your box's hostname and the admin password you created previously.

## 01. Set up your development computer

You can build on Ubuntu or Debian (Intel/AMD or Arm), Windows 11 with WSL 2, or a Mac with Apple silicon. Intel Macs aren't supported.

> [!note]
> Running Windows? [Set up WSL 2](#windows-only-set-up-wsl-2) first, then come back here.

### Install teka CLI

Install the latest teka release:

```bash
curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | sh
```

The installer sets up the CLI and Nix if needed. For Windows, start with the [WSL 2 host setup](../03_tekaos/02_set-up-a-development-host/02_windows-wsl-2.md). teka goes to `~/.local/bin` when that folder is on your `PATH`, otherwise `/usr/local/bin`. On a Mac it also sets up the Linux build machine. Run the same command again to update teka.

The script may ask for:

- Permission to install Nix, the first time. Open a new terminal when it's done.
- Your sudo password, to install teka to `/usr/local/bin`.
- Your Mac password or Touch ID, to set up the Linux build machine. Its first start takes about a minute.
- Your login keychain password on a Mac, the first time teka saves a box password. Choose Allow.

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

You'll see the examples you can start from:

- hello-world
- hello-webui
- hello-ouster-world


## 02. Connect to your ZED Box

Check that your ZED Box is connected to your computer or network and powered on. Then find the TekaOS boxes on your network:

```bash
teka box list --discover
```

Each box prints its hostname, URL, hardware, and serial number. Select yours by part of its hostname or serial number:

```bash
teka box select teka-<serial>
```

Run `teka box select` with no name to pick from a menu instead. teka asks for your box's admin password, signs in, and makes the box your default. Check the connection:

```bash
teka box status
```

You'll see `Authentication: ok` when teka can sign in. teka saves the password in macOS Keychain or the Linux Secret Service, so you don't need to type it again. To remove a saved box and its password, run `teka box forget`.

## 03. Deploy your first app

Create your own copy of the hello-world example app. Run this from an empty folder:

```bash
teka app init --example hello-world
```

This fills the current folder with everything the app needs, and pins the Teka SDK commit it came from. Take a look:

```bash
ls
```

You'll see `default.nix`, `flake.nix`, `hello-world.rs`, `hello-world.service`, and `README.md`. From this folder, build the app and deploy it to your selected box:

```bash
teka app deploy --build
```

teka builds the app for the box, uploads it, and checks its log for a clean start. To see your app in the web interface, open Apps. You'll find hello-world under Installed apps. Click Logs to see `hello world`.

![03. Deploy your first app](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/hello-world-installed.png)

![03. Deploy your first app](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/hello-world-logs.png)

> [!note]
> After you modify the app, run `teka app deploy --build` again to rebuild and redeploy your changes to the box.

### Prefer to install from your browser?

Build the app image without deploying it:

```bash
teka app build
```

teka prints the path of the package under `result/hello-world`. Then do the following:

1. Find the Apps tab in your box's browser session.
2. Choose the .tpk file from that folder under Install App.
3. Keep the Trusted (default) security profile.
4. Click Install.

### Start your own app

Create a new app with `teka app init`, then build and deploy it to your box.

```bash
teka app init my-app
cd my-app
```

The app keeps the hello-world ID until you change it. Edit the program and `default.nix`, then build and deploy from the folder:

```bash
teka app deploy --build
```

Run the same command after each change. teka replaces the app and keeps its data. See [Starting your own app](../02_teka-apps/01_starting-your-own-app.md) for what each file does.

## Teka CLI at a glance

- `teka box list`: list the boxes you've signed in to before. Add `--discover` to search your network.
- `teka box select [PATTERN]`: sign in to boxes and make them the default.
- `teka box status`: show whether teka can reach and sign in to your selected boxes.
- `teka box forget [PATTERN]`: remove a saved box and its password.
- `teka app init [DIR]`: create a new app in a folder, or in the current folder. Add `--example NAME` to start from a specific starter example.
- `teka app list`: list the apps in the current folder's project. Add `--examples` to list the starter examples.
- `teka app build [APP…]`: build app packages into `./result`. With no name, builds the app in the current folder.
- `teka app deploy [APP…]`: install apps on your selected boxes. Add `--build` to build them first. Add `--box URL` to pick another box.
- `teka system status`: show the box's TekaOS version and update state.
- `teka system update FILE --wait`: install a TekaOS update and wait for it to finish.
- `teka --help`: see every command and flag.

## Next steps

- [ZED Box Mini connectivity](../04_edge_compute_&_accessories/01_zed-box-mini-connectivity.md): learn about the ZED Box Mini's ports.
- [Using the GPIOs](../04_edge_compute_&_accessories/02_using-the-gpios.md): build an app that drives the GPIO pins.
- [hello-ouster-world](https://gitlab.com/ouster/ia/teka-sdk/-/tree/main/apps/examples/hello-ouster-world): open an OSF recording or a live Ouster sensor and view it in your browser.
- [Uninstall TekaOS](../04_edge_compute_&_accessories/06_uninstall-tekaos.md): go back to Stereolabs' JetPack image.
- [Teka Apps](../02_teka-apps/00_README.md): build your own apps and try more examples.
- [TekaOS documentation](../03_tekaos/00_README.md): use the box API and system utilities.

## Windows only: set up WSL 2

Do this before step 01. You'll install Ubuntu in WSL 2, then run the install commands inside Ubuntu.

Set your Windows network connection to Private so Windows can resolve .local box names. Then install Ubuntu from PowerShell:

```bash
wsl --install -d Ubuntu-26.04
```

Add these lines to %USERPROFILE%\.wslconfig. This needs Windows 11 22H2 or newer.

```ini
[wsl2]
networkingMode=mirrored
```

Then shut WSL down so the change applies:

```bash
wsl --shutdown
```

Open Ubuntu and go back to step 01. Run the commands there from the Linux filesystem, such as ~/src, not under /mnt/c.

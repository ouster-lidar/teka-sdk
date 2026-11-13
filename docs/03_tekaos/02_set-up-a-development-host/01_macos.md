# How to Set Up a Development Host on macOS

You can build Teka apps on a Mac with Apple silicon. Teka apps run on Linux, so the Mac needs a Linux build VM. `teka build init` installs that VM, registers it with Nix, and configures public Nix caches. Intel Macs aren't supported.

Rosetta must be installed (`softwareupdate --install-rosetta --agree-to-license`). The VM data, keys, and logs live in `~/.local/share/teka-build-vm`. `teka build vm init` registers the builder in `/etc/nix/teka-build-vm.conf` and restarts the Nix daemon through sudo.

## Install

Install the latest teka release:

```bash
curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | sh
```

The installer sets up the CLI and Nix if needed. It runs `teka build init`, which sets up the Linux build VM and configures public Nix caches.

From a downloaded portable binary:

```bash
./teka-aarch64-darwin build init
```

## Check it

```bash
teka build check
teka build vm check
```

## Manage the build VM

| Command | What it does |
| --- | --- |
| `teka build vm init` | Registers the VM as the Nix Linux builder. |
| `teka build vm start` | Keeps the VM running across builds. |
| `teka build vm stop` | Stops it after builds and SSH sessions finish. |
| `teka build vm status` | Shows runtime state and resource use. |
| `teka build vm ssh` | Opens a shell in the guest. |
| `teka build vm check` | Checks registration, connectivity, and kernel OOM kills. |

SSH listens on localhost port 31023. The console log is `~/.local/share/teka-build-vm/vm/console.log`.

`teka build vm check` fails when the guest has had a kernel OOM kill since boot, even if memory has recovered. Normal builds should fit in RAM. A separate sparse 8 GiB disk, `vm/swap.img`, gives high-concurrency builds somewhere to spill instead of being killed. After you upgrade Teka, run `teka build vm stop` and `teka build vm start` so the guest picks up that disk. No reset is required.

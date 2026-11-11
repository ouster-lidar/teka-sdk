# WSL 2 Host Setup

WSL 2 can provide an **x86_64-linux** development host on Windows. Complete this guide first, then follow [Linux host setup](linux.md) for packages and Nix. Clone and build steps are in the [Developer Guide](developer-guide.md).

## Tested configuration

- Windows 11 version: `25H2`
- WSL version: `2.7.11.0`
- WSL distro: Ubuntu 26.04 (`6.18.33.2-microsoft-standard-WSL2`)
- Hardware: `x86_64` system with 32GiB of RAM

## Setup

### Network config

To use hostnames ending in `.local`, set the Windows network connection to **Private** so Windows queries and listens for mDNS from local devices.

### Install WSL

Install [WSL 2](https://learn.microsoft.com/en-us/windows/wsl/install) with Ubuntu 26.04 from PowerShell:

```powershell
wsl --install -d Ubuntu-26.04
```

### Mirrored networking

Windows 11 22H2+ is required for mirrored mode. Add to `%USERPROFILE%\.wslconfig`:

```toml
[wsl2]
networkingMode=mirrored
```

Shut down WSL so the change takes effect:

```powershell
wsl --shutdown
```

## Next steps

Follow [linux.md](linux.md) for packages and Nix, then the [Developer Guide](developer-guide.md) to clone, build, and deploy.

## Additional notes

Clone and build inside the WSL filesystem (for example `~/src/...`), not under `/mnt/c/`, for reasonable I/O and Unix permissions.

When deploying, pass an explicit routable `--device` URL to `teka app deploy`.

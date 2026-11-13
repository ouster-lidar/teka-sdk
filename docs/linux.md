# Linux Build Host Setup

Linux x86_64 and AArch64 build hosts are supported. App builds target AArch64 by default; use `--arch x86_64` for x86_64 boxes.

- Nix install and flake configuration: [Nix install and flake outputs](nix.md)
- Build + deploy workflow: [Developer Guide](developer-guide.md)

## Setup steps

### 1. Install packages

Install Git, `curl`, `jq`, `xz-utils`, mDNS hostname support, and GNU coreutils (`timeout`, `stat`, `grep`, etc.) through your distribution package manager.

**Debian / Ubuntu example:**

```bash
sudo apt update
sudo apt install -y git curl jq xz-utils libnss-mdns
```

Other distributions use their own package names; only the Debian/Ubuntu commands above are distro-specific examples.

### 2. Install the CLI and Nix

Install the latest teka release:

```bash
curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | sh
```

The installer sets up the CLI and Nix if needed. For Windows, start with the [WSL 2 host setup](wsl2.md). On Arm Linux, the installer isn't available. From a checkout of this repository, run `./bootstrap.sh` instead.

See the [CLI installation guide](../crates/teka-cli/README.md#install) for other options and [nix.md](nix.md) for build outputs and troubleshooting.

### 3. Next steps

Continue with the [Developer Guide](developer-guide.md) to build and deploy apps.

# Linux Build Host Setup

Linux x86_64 and AArch64 build hosts are supported. App builds target AArch64 by default; use `--arch x86_64` for x86_64 devices.

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

Clone this repository and run `./bootstrap.sh` from its root. Bootstrap installs Nix interactively if needed, builds the portable CLI, and runs `teka build init`.

See the [CLI installation guide](../crates/teka-cli/README.md#install) for alternatives and [nix.md](nix.md) for build outputs and troubleshooting.

### 3. Next steps

Continue with the [Developer Guide](developer-guide.md) to clone, build, and deploy apps.

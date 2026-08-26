# Linux Build Host Setup

Linux is the primary supported developer platform. This guide currently focuses on **x86_64-linux** build hosts; see the `docs/` directory for other build hosts.

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

### 2. Install and configure Nix

Install and configure Nix per [nix.md](nix.md).

### 3. Next steps

Continue with the [Developer Guide](developer-guide.md) to clone, build, and deploy apps.

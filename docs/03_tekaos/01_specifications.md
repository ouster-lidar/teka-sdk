# Recommended Specifications for TekaOS

Here's what TekaOS runs on, and what you need to build apps for it.

## Supported boxes

| Box | Compute | Storage |
| --- | --- | --- |
| ZED Box Mini | NVIDIA Jetson Orin NX 16 GB, up to 157 TOPS | 256 GB NVMe SSD |
| ZED Box 2, 16GB, 1TB (SKU ZBX-820120) | NVIDIA Jetson Orin NX 16 GB, up to 157 TOPS | 1 TB NVMe SSD |

Both come with TekaOS installed. Every box names itself teka- plus its serial number, like teka-gtw-onx2-x11xxxxx.local. Learn more in [Connect to your ZED Box](../01_getting-started/02_connect-your-zed-box.md).

## Software versions

| Component | Version |
| --- | --- |
| NVIDIA JetPack | 7.2 (L4T r39.2) |
| CUDA | 13.2 |
| Linux kernel | 6.8 |

## Your computer

|  | Requirement |
| --- | --- |
| Operating system | Ubuntu or Debian (x86_64 or Arm), Windows 11 with WSL 2, or a Mac with Apple silicon. Intel Macs aren't supported. |
| Build tools | The teka installer sets up Nix and `teka`. On a Mac it also sets up the [Linux build VM](02_set-up-a-development-host/01_macos.md). |
| Network | The same local network as your box |

## How storage works

TekaOS keeps the system and your data on separate partitions. Updates never touch your data.

| Partition | What's on it |
| --- | --- |
| Root A and Root B | Two copies of the read-only system. Updates go to the copy you're not using. |
| srv | Your settings, logs, apps, and app data. It fills the rest of the disk. |

## Next steps

- [Connect to your ZED Box](../01_getting-started/02_connect-your-zed-box.md)
- [Network defaults](05_update-tekaos/01_first-boot-and-access.md)
- [Set Up a Development Host](02_set-up-a-development-host/00_README.md)

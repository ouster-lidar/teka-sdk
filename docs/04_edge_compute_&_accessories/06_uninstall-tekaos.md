# Uninstall TekaOS from your ZED Box Mini or ZED Box 2

Want to remove TekaOS from your ZED Box and install Stereolabs' Ubuntu-based JetPack image instead? Flash the box from an Ubuntu computer with Stereolabs' script.

> [!warning]
> Flashing erases the internal disk, including your apps, settings, and admin password. Copy anything you want to keep off the box first.

## Before you start

You'll need:

- A computer running Ubuntu 20.04 or 22.04 with at least 80 GB free. A virtual machine or WSL 2 won't work. No Ubuntu computer? Start one from an Ubuntu live USB.
- A USB-A to micro-USB cable.
- Two pins or straightened paper clips.
- A display, keyboard, and ethernet cable, for setup after flashing.

## 01. Put the box in Force Recovery mode

1. Connect the power.
2. Use a pin to press and hold the RCV button.
3. While you hold RCV, press the RST button. Then release both.

In Force Recovery mode, the power LED stays off. You don't need to open the case, and opening it voids the warranty.

## 02. Check Force Recovery mode

Connect the cable from your computer to the box's micro-USB OTG port, then run:

```bash
lsusb -d '0955:'
```

You should see NVIDIA Corp. APX with one of these IDs:

- Orin NX 16 GB: 0955:7323
- Orin NX 8 GB: 0955:7423
- Orin Nano 8 GB: 0955:7523
- Orin Nano 4 GB: 0955:7623

If you see 0955:7020 instead, the box isn't in Force Recovery mode. Go back to step 01.

## 03. Flash JetPack

Make a working folder and install the tool the script needs:

```bash
mkdir -p ~/zedbox_flash
cd ~/zedbox_flash
sudo apt install qemu-user-static
```

Pick a JetPack version:

- JetPack 6.2 (L4T 36.4.3): [zedbox_mini_usb_flash_6.2_gpio.sh](https://download.stereolabs.com/scripts/zedbox_mini_usb_flash_6.2_gpio.sh)
- JetPack 7.2 (L4T 39.2): [zedbox_mini_usb_flash_7.2_gpio.sh](https://download.stereolabs.com/scripts/zedbox_mini_usb_flash_7.2_gpio.sh)

Download the script and run it. This example uses JetPack 6.2:

```bash
wget https://download.stereolabs.com/scripts/zedbox_mini_usb_flash_6.2_gpio.sh
chmod +x zedbox_mini_usb_flash_6.2_gpio.sh
BSP_ROOT=./stereolabs_bsp/ ./zedbox_mini_usb_flash_6.2_gpio.sh
```

The script downloads NVIDIA's BSP into stereolabs_bsp and asks for your password to run as root. It takes a while. When it's done, you'll see:

```bash
Flash is successful
Reboot box
Cleaning up...
FLASH SUCCESS
```

Flash failing? See "I cannot flash the ZED Box Mini" in the [Stereolabs troubleshooting guide](https://www.stereolabs.com/docs/embedded/zed-box-mini/troubleshooting).

## 04. Set up JetPack

Connect the display, keyboard, and ethernet cable, then power on. Sign in with user name user and password admin. Then open a terminal and install JetPack:

```bash
sudo apt update
sudo apt install nvidia-jetpack
```

> [!note]
> Don't run sudo apt upgrade yet. A newer kernel can break the ZED X camera driver.

To use ZED X cameras, install the Stereolabs GMSL2 driver, hold the kernel packages, and install the ZED SDK for your JetPack version. Stereolabs' [post-flash steps](https://docs.stereolabs.com/docs/products/embedded/zed-box-mini/reset-update) walk you through each one.

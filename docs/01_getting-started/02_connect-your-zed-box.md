# Connect to your ZED Box Mini or ZED Box 2 with TekaOS

Your ZED Box Mini or ZED Box 2 comes with TekaOS installed. TekaOS runs headless, so you set it up and manage it from a web browser on any computer on the same network. This guide takes you from unboxing to running your first app.

## Before you start

You'll need:

- Your ZED Box Mini or ZED Box 2 and its power supply
- An Ethernet cable, and a network with a DHCP server (most routers have one)
- A computer on the same network, with a web browser
- To build apps: an x86_64 Linux computer, Windows 11 with WSL 2, or a Mac

Optional:

- An [Ouster lidar](https://ouster.com/products/hardware/os1-lidar-sensor)
- An Ethernet switch, to connect a lidar to a ZED Box Mini, or to connect more than one lidar to your ZED Box.

## 01. Power on the ZED Box

Connect the supplied power supply to the power input. The ZED Box turns on by itself. Give it a minute or two to start.

![01. Power on the ZED Box](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/power-on.png)

> **Note:**
> You don't need a display, keyboard, or mouse.

## 02. Connect to your network

Plug an Ethernet cable into the ZED Box's Ethernet port. Connect the other end to your network, or straight to your computer. Push the connector in until it clicks.

![02. Connect to your network](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/connect-ethernet.png)

On a network, TekaOS gets its IP address over DHCP, so there's nothing to configure.

## 03. [Optional] Connect your Ouster lidar

- ZED Box Mini: it has one Ethernet port. Plug your network, your [Ouster lidar](https://ouster.com/products/hardware/os1-lidar-sensor), and the ZED Box Mini into an Ethernet switch.
- ZED Box 2: it has two Ethernet ports. Plug your network into one and your lidar into the other. You don't need a switch.

![03. Optional Connect your Ouster lidar](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/connect-lidar-with-switch.png)

![03. Optional Connect your Ouster lidar](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/connect-lidar-zed-box.png)

> **Note:**
> Your lidar needs power. Use a PoE switch, or power the sensor through the interface box that came with it.

## 04. [Optional] Connect a display

You don't need a display. TekaOS is managed virtually from a browser on your computer, and the box’s HDMI output stays blank unless an app uses it.

If your custom app requires a display, connect a display to the HDMI port.

![04. Optional Connect a display](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/connect-display.png)

## 05. Connect via a web browser

### Open the TekaOS web interface

Find the serial number on your ZED Box's label. It's 15 characters long and looks like GTW-ONX2-X11XXXXX.

![Open the TekaOS web interface](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/serial-number-label.png)

On your computer, open a browser and go to teka- followed by your serial number, dashes included:

```bash
http://teka-<serial>.local
```

(For example, if your serial number is GTW-ONX2-X11XXXXX you would type http://teka-gtw-onx2-x11xxxxx.local. Capital letters work too.)

> **Note:**
> Page doesn't load? First check your ethernet connectors are pushed in until they click. If that doesn’t work, then try waiting 2 minutes and trying again. If it still doesn’t load, look up the box's IP address in your router's list of connected devices and go to `http://<ip-address>` instead. On Linux, install libnss-mdns so your computer can find .local names.

## 06. Configure your hostname and password

### 06a: [Optional] Give your box a shorter hostname

Want to rename your ZED Box? Give it a short name, like teka-robot-7, that's easier to type and remember than a serial number.

1. In the web interface, click the “edit” icon next to the current hostname in the top bar.
2. Under Manual hostname, type the part that comes after teka-, for example robot-7.
3. Click Save. The new name works right away, so reopen the web interface at http://teka-robot-7.local.

Your name will persist through TekaOS updates. To reset back to the serial-number name, open the same dialog and click Reset. A factory reset also resets it.

![06a: Optional Give your box a shorter hostname](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/hostname-home.png)

![06a: Optional Give your box a shorter hostname](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/hostname-dialog.png)

### 06b. Create an admin password

New boxes don't have a password. You can't install apps until you set one.

1. Go to `http://teka-<serial>.local/login`.
2. Enter a password with at least 10 characters, then enter it again to confirm.
3. Click Create admin password.

Keep this password somewhere safe. You'll use it to sign in, and the teka command-line tool uses it to install apps.

![06b: Create an admin password](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/admin-password.png)

## 07. Discover your sensors

If you connected a lidar in step 03, open Sensors in the sidebar. Your lidar’s hostname will show up under Sensor Discovery. Click it to see its status, data streams, and self tests.

![07. Discover your sensors](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/sensor-discovery.png)

> **Note:**
> Don't see your lidar listed in Sensor Discovery? Make sure your sensor is powered on, give it a minute to boot up, and then refresh the Sensors page. Check that the lidar’s Ethernet link light is on, and that it's on the same network as the ZED Box (the same switch, or the ZED Box 2's second port). TekaOS finds sensors by their .local names, which don't pass through routers or between VLANs. To check the sensor itself, open `http://os-<sensor-serial>.local` in your browser.

## 08. Visualize your sensors

The hello-ouster-world app shows live point clouds from your lidar in your browser.

1. Open `http://teka-<serial>.local/app/hello-ouster-world`. Or open Apps and click hello-ouster-world under Installed apps.
2. Choose Connected sensor.
3. Choose your lidar from the sensor dropdown. Its hostname fills in for you. Sensors already sending data to your ZED Box have a star and are listed first.
4. Click Apply.

![08. Visualize your sensors](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/visualize-sensor.png)

The point cloud appears in a few seconds. If you connected a display in step 04, it shows there too. To save the data, turn on Record before you click Apply.

> **Tip:**
> Hover over the 3D viewer, then drag with your mouse to rotate it and scroll to zoom. Click ? in the viewer to see the keyboard shortcuts.

> **Note:**
> Don't see hello-ouster-world under Installed apps? Set up your computer as a development host first. See [Deploy your first App](03_deploy-your-first-app.md). Then, from your computer, run teka app init --example hello-ouster-world, then cd hello-ouster-world, then teka app deploy.

## Next steps

- [Deploy your first App](03_deploy-your-first-app.md): set up your computer, connect to your ZED Box with the teka CLI, and run your first app.

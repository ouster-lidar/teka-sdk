# ZED Box Mini Connectivity

The ZED Box Mini fits power, networking, display, and expansion ports into a compact case. This page covers each port and how TekaOS uses it.

## Connection ports

### Front side

![Front side](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-front.png)

- Power input
- ethernet port
- Power LED
- Reset button (RST)
- Force Recovery button (RCV)

### Left side

![Left side](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-left-wifi.png)

### Wi-Fi antenna connectors (optional)

### Rear side

![Rear side](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-rear.png)

### Micro-USB 2.0 OTG port

- USB 3.0 port
- Two GMSL2 connectors

### Right side

![Right side](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-right.png)

- GPIO header
- Fan connector
- HDMI port
- SYNC-IN
- SYNC-OUT

## Power input

DC 10.5 V to 13.5 V, 60 W max. Use the supplied 12 V, 60 W power supply or a regulated 12 V source.

![Power input](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-power-input.png)

- Pin 1: DC IN 12 V
- Pin 2: GND

Connector type: TBP01P1W-508-02GR. For battery power, use a 4S lithium or lithium polymer battery with a 60 W, 12 V DC-DC converter so the voltage stays stable.

> **Warning:**
> Check the voltage before you connect power. More than 16 V can damage the box.

## ethernet

One Gigabit ethernet port (10/100/1000BASE-T) with an RJ45 connector. Two green LEDs show link and activity.

TekaOS gets an IP address over DHCP by default. To set a static address, or to plug a sensor straight into the box, open Connectivity in the web interface.

![ethernet](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-ethernet.png)

### Pinout

- Pin 1: MDI0_P (TX+)
- Pin 2: MDI0_N (TX-)
- Pin 3: MDI1_P (RX+)
- Pin 4: MDI2_P
- Pin 5: MDI2_N
- Pin 6: MDI1_N (RX-)
- Pin 7: MDI3_P
- Pin 8: MDI3_N

## Wi-Fi and Bluetooth (optional)

ZED Boxes with TekaOS pre-installed also are all wifi-enabled by default with the Intel AX210 module, with Wi-Fi in TekaOS with no setup. The module sits in the M.2 Key-E 2230 socket and uses two 2.4/5 GHz antennas.

> **Note:**
> TekaOS doesn't support bluetooth yet, but support is coming soon.

Need to connect a laptop in the field? Open Network, turn on the Wi-Fi access point, and pick a network name and a password of at least 8 characters. The box is at 10.41.0.1 on that network. The access point doesn't hand out addresses, so give your laptop a static address such as 10.41.0.2.

## GPIO header

A 10-pin header with two GPIOs, CAN, UART, 5 V, 3.3 V, and ground. To use the GPIOs, see [Using GPIO](02_using-the-gpios.md).

![GPIO header](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-gpio-header.png)

### Pinout

- Pin 01: VCC_5V
- Pin 02: GND
- Pin 03: GPIO_11 (CMOS 3.3 V)
- Pin 04: GPIO_12 (CMOS 3.3 V)
- Pin 05: CAN_RX (CMOS 3.3 V)
- Pin 06: CAN_TX (CMOS 3.3 V)
- Pin 07: VCC_3V3
- Pin 08: GND
- Pin 09: UART_RXD (CMOS 3.3 V)
- Pin 10: UART_TXD (CMOS 3.3 V)

> **Warning:**
> Every signal on this header is 3.3 V CMOS. Signals outside the spec can damage the box, and that damage isn't covered by the warranty.

The CAN pins connect straight to the Jetson module. To use them as a CAN bus, add a transceiver such as the [Waveshare SN65HVD230](https://www.waveshare.com/sn65hvd230-can-board.htm). TekaOS doesn't set up the CAN or UART pins yet.

## Fan

A 12 V fan connector for active cooling. A fan is a good idea for heavy GPU workloads. The [fan with mounting plate](https://www.stereolabs.com/store/products/fan-for-zed-box-mini) fits the aluminum case.

![Fan](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-fan.png)

- Pin 01: FAN_12V
- Pin 02: GND

The connector is a JST 2-pin, 2.50 mm pitch, and mates with XHP-2. A third-party fan must run at 12 V and draw 500 mA or less. Above that, a fuse on the board cuts power to the fan.

## HDMI

One HDMI type-A port, up to 3840×2160 at 60 Hz.

TekaOS has no desktop, so the screen stays blank until an app uses it. Apps draw on the screen with Wayland, and only one app can use it at a time. Learn more in the [TekaOS documentation](../03_tekaos/00_README.md).

## USB 3.0

One USB 3.0 Type-A port, up to 5 Gbps.

## Micro-USB 2.0 OTG

Use this port with the recovery button to reflash the firmware. See [Reset and Update](05_reset-to-factory-settings.md).

## GMSL2 cameras and sync ports

The ZED Box Mini has two GMSL2 FAKRA connectors for Stereolabs ZED X cameras, plus SYNC-IN and SYNC-OUT terminal blocks for triggering cameras across several boxes.

> **Note:**
> TekaOS doesn't support GMSL2 cameras or the sync ports yet, but support is coming soon.

### Sync pinouts

Both connectors are Same Sky 3-pin terminal blocks that mate with TBP02P1W-381-03BE.

- SYNC-IN: pin 01 TRIG_INFO, pin 02 GND, pin 03 TRIG_IN
- SYNC-OUT: pin 01 GND, pin 02 GND, pin 03 TRIG_OUT

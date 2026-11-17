# Using the GPIOs on your ZED Box Mini

The GPIO header has two general-purpose pins, GPIO_11 and GPIO_12. Each one works as an input or an output, at 3.3 V.

![Illustration](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/zed-box-mini-gpio-header.png)

- Pin 03: GPIO_11, line name PQ.06
- Pin 04: GPIO_12, line name PN.01

For the full pinout, [see Connectivity](01_zed-box-mini-connectivity.md).

## How GPIO works on TekaOS

TekaOS is read-only, so you don't install GPIO tools on the box. Instead, your Teka App brings its own copy of [libgpiod](https://libgpiod.readthedocs.io/) and asks for access to the GPIO device. Apps run as root inside their sandbox, so they don't need a gpio group.

## Wire an input

An input pin needs a pull-up or pull-down resistor. Without one, the pin holds its last value and you'll get unreliable readings. A 470 Ω resistor works.

- Pull-down: the pin reads high when you connect it to 3.3 V (pin 07).

![Wire an input](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/gpio-wire-input.png)

- Pull-up: the pin reads low when you connect it to GND (pin 02 or 08).

![Wire an input](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/gpio-wire-input-detail.png)

## Build a GPIO app

The [hello-gpio](https://gitlab.com/ouster/ia/teka-sdk/-/tree/main/apps/examples/hello-gpio) template toggles GPIO lines once a second with libgpiod. It ships the same program in Python (toggle_gpio.py) and C++ (toggle_gpio.cpp). You change the code and a few settings. The template handles the packaging, so you don't need to know Nix.

### Get the template

Each app folder is a standalone project. Create one from the hello-gpio example and go into it:

```bash
teka app init --example hello-gpio hello-gpio
cd hello-gpio
```

### Pick your pins

hello-gpio drives line 7 on /dev/gpiochip0. Lines are gpiochip line offsets, not header pin numbers. To find the offsets for GPIO_11 (PQ.06) and GPIO_12 (PN.01), run gpioinfo and look for those names. Then set them in default.nix:

```nix
# Offsets from `gpioinfo` on the target chip, not header pin numbers.
chip = "/dev/gpiochip0";
lines = "7";
```

To drive several lines, separate them with commas, such as "7,8".

### Pick Python or C++

The app runs toggle_gpio.py by default. To run toggle_gpio.cpp instead, change this line in default.nix:

```bash
Environment=HELLO_GPIO_IMPL=cpp
```

Put your own logic in either file. Both read the chip and lines from HELLO_GPIO_CHIP and HELLO_GPIO_LINES. Keep PrivateDevices=no in default.nix. Without it, the sandbox hides /dev/gpiochip* from your app.

### Deploy it

From the hello-gpio folder, with your box selected as in step 02 of [Deploy your first App](../01_getting-started/03_deploy-your-first-app.md):

```bash
teka app deploy --build
```

Connect an LED or a multimeter to the pin, and you'll see it switch every second. Open Apps in the web interface and click Logs next to hello-gpio. You'll see value=active and value=inactive twice a second.

## Read a pin

To read a pin, request its line as an input, then get its value. Wire it with a pull-up or pull-down resistor first, as in Wire an input.

In Python:

```python
import gpiod
from gpiod.line import Direction
LINE = 7  # line offset from gpioinfo
settings = {LINE: gpiod.LineSettings(direction=Direction.INPUT)}
with gpiod.request_lines("/dev/gpiochip0", consumer="my-app", config=settings) as req:
    print(req.get_value(LINE))  # Value.ACTIVE or Value.INACTIVE
```

In C++:

```cpp
const unsigned int line = 7;  // line offset from gpioinfo
auto req = gpiod::chip("/dev/gpiochip0")
    .prepare_request()
    .set_consumer("my-app")
    .add_line_settings(line, gpiod::line_settings().set_direction(
        gpiod::line::direction::INPUT))
    .do_request();
std::cout << (req.get_value(line) == gpiod::line::value::ACTIVE) << std::endl;
```

For everything else, see the libgpiod [Python API](https://libgpiod.readthedocs.io/en/master/python_api.html) and [C++ API](https://libgpiod.readthedocs.io/en/master/cpp_api.html) guides.

> **Note:**
> TekaOS doesn't support the CAN and UART pins on this header yet.

# App Examples

The starter examples are the quickest way to see what TekaOS can do. Deploy one, change it, then use it as a starting point for your own.

## Deploy an example

Install teka and select your box once. See [Set Up a Development Host](../03_tekaos/02_set-up-a-development-host/00_README.md). Then initialize an example and deploy it:

```bash
teka app init my-app --example hello-ouster-world
```

Then build and deploy it from the new folder:

```bash
cd my-app && teka app deploy
```

Run `teka app list --examples` to see the examples. Every example folder is a complete project with its own `flake.nix`. Learn more about the [Teka CLI](../03_tekaos/03_teka-cli.md).

## Main examples

| App | What it does |
| --- | --- |
| `hello-ouster-world` | Shows lidar data from a sensor or a recording, on the screen and in your browser. Start here. |
| `hello-world` | The smallest possible app. Writes hello world to the log. |
| `hello-webui` | A static web page with simulated graphs. No sensor and no backend. |
| `hello-gpio` | Turns GPIO lines on and off once a second. |

The teka-sdk repository has more examples under `apps/examples/`, such as hello-http and resource-monitor. Build them from the repository root with `teka app deploy NAME`.

## `hello-world`

To check that it ran, open **Apps** in the web interface, find `hello-world` under **Installed apps**, and click **Logs**. You'll see `hello world`.

## `hello-ouster-world`

Shows lidar data on a monitor plugged into the box, and in the app's web page. Pick a source:

| Source | What it is |
| --- | --- |
| Local file | An `.osf` recording on the box |
| Connected sensor | A sensor's hostname or IP address. Click **Record** to save the data. |

## `hello-webui`

A small dashboard of simulated traffic, latency, and CPU load. Plain HTML, CSS, and JavaScript. Open `/app/hello-webui` and select **Refresh samples**. Nothing on the page comes from a sensor.

## `hello-gpio`

Turns GPIO lines on and off once a second, with a Python or a C++ program. By default it uses line 7 on `/dev/gpiochip0`. Line numbers are the chip's line offsets, not 40-pin header numbers, so check yours on the box first. To use other lines, change `lines` in `default.nix`. To run the C++ program, set `HELLO_GPIO_IMPL=cpp` in `default.nix`. Then deploy again.

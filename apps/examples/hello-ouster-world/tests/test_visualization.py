"""Rendered point clouds from sensors, device recordings and cloud samples."""

from playwright.sync_api import expect

from viewer import SOURCE_KEYS


def test_live_pointcloud(sensor):
    page = sensor.page
    page.locator("#btn-keybindings").click()
    expect(page.locator("#keybindings-playback")).not_to_be_visible()
    expect(page.locator("#keybindings-advanced")).not_to_be_visible()
    expect(page.locator("#keybindings-sensor-rows tr")).to_have_count(1)
    page.keyboard.press("Escape")


def test_recorded_pointcloud(viewer, recorded, sensor_hostname):
    viewer.start("osf", recorded, stop=False)
    viewer.start("sensor", sensor_hostname, stop=False)
    viewer.start("osf", recorded, stop=False)


def test_cloud_pointcloud(replay):
    source = replay.api("/source-config")
    replay.page.reload(wait_until="domcontentloaded")
    replay.state("playing_osf")
    restored = replay.api("/source-config")
    assert {k: restored[k] for k in SOURCE_KEYS} == {k: source[k] for k in SOURCE_KEYS}
    replay.configure()
    expect(replay.page.locator("#cfg-osf-select")).to_have_value(source["osf_path"])
    replay.frames()


def test_visualization_controls(replay):
    wrap = replay.page.locator("#stream-wrap")
    wrap.hover()
    replay.wait(lambda: replay.api("/display/status")["controlling"])
    wrap.press("Space")
    replay.page.wait_for_timeout(1000)
    replay.frames(10)
    assert len(set(replay.jpeg[-5:])) == 1, "Pause did not stop playback"
    canvas = replay.page.locator("#stream-canvas")
    replay.page.locator("#btn-keybindings").click()
    replay.page.keyboard.press("Space")
    replay.frames()
    assert len(set(replay.jpeg[-5:])) == 1, "Help forwarded playback input"
    replay.page.keyboard.press("Escape")
    wrap.hover()
    for key in (".", ",", "w", "Shift+r"):
        before = canvas.evaluate("c => c.toDataURL()")
        wrap.press(key)
        replay.wait(lambda: canvas.evaluate("c => c.toDataURL()") != before)
    before = canvas.evaluate("c => c.toDataURL()")
    box = wrap.bounding_box()
    mouse = replay.page.mouse
    x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
    mouse.move(x, y)
    mouse.down()
    mouse.move(x + 80, y + 40, steps=10)
    mouse.up()
    replay.wait(lambda: canvas.evaluate("c => c.toDataURL()") != before)
    before = canvas.evaluate("c => c.toDataURL()")
    mouse.wheel(0, 120)
    replay.wait(lambda: canvas.evaluate("c => c.toDataURL()") != before)
    wrap.press("Space")
    replay.frames(10)
    assert len(set(replay.jpeg[-5:])) > 1, "Resume did not advance playback"
    mouse.move(0, 0)
    replay.wait(lambda: not replay.api("/display/status")["controlling"])

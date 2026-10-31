"""Browser controls, dialogs, screenshots, console and guest access."""

import uuid

from PIL import Image
from playwright.sync_api import expect
import pytest

from viewer import API, Viewer


def test_display_controls(replay):
    page = replay.page
    replay.configure()
    replay.toggle("stream-toggle", False)
    replay.wait(lambda: not replay.api("/display/status")["stream"])
    page.reload(wait_until="domcontentloaded")
    replay.configure()
    expect(page.locator("#stream-toggle")).not_to_be_checked()
    expect(page.locator("#btn-screenshot")).to_be_disabled()
    page.wait_for_timeout(500)
    replay.jpeg.clear()
    page.wait_for_timeout(750)
    assert not replay.jpeg
    replay.toggle("stream-toggle", True)
    replay.frames()
    display = replay.api("/display/status")
    if display["hdmi_present"]:
        for enabled in (False, True, False, True):
            replay.toggle("hdmi-toggle", enabled)
            replay.wait(lambda: replay.api("/display/status")["on_device"] == enabled)
            # Let the periodic output refresh run after moving the viewer.
            page.wait_for_timeout(3000)
            replay.frames()
            display = replay.api("/display/status")
            assert display["viz_output"].startswith("HEADLESS") == (not enabled)
            if not enabled:
                assert (display["width"], display["height"]) == (1920, 1080)
                assert (display["stream_width"], display["stream_height"]) == (1920, 1080)
    else:
        expect(page.locator("#hdmi-toggle")).to_be_disabled()


def test_fullscreen_and_help(replay):
    page = replay.page
    page.locator("#btn-fullscreen").click()
    page.wait_for_function("document.fullscreenElement !== null")
    page.locator("#btn-keybindings").click()
    expect(page.locator("#keybindings-dialog")).to_be_visible()
    expect(page.locator("#keybindings-playback")).to_be_visible()
    expect(page.locator("#keybindings-advanced")).to_be_visible()
    page.locator("#btn-close-keybindings").click()
    page.locator("#btn-fullscreen").click()
    page.wait_for_function("document.fullscreenElement === null")
    page.locator("#stream-wrap").hover()
    page.locator("#stream-wrap").press("Shift+/")
    expect(page.locator("#keybindings-dialog")).to_be_visible()
    page.keyboard.press("Escape")
    expect(page.locator("#keybindings-dialog")).not_to_be_visible()
    for width, height in ((640, 480), (1440, 1080)):
        page.set_viewport_size({"width": width, "height": height})
        page.wait_for_function(
            """() => {
            const canvas = document.getElementById('stream-canvas');
            const box = canvas.getBoundingClientRect();
            return box.width > 0 && box.height > 0 &&
                box.left >= 0 && box.right <= innerWidth && box.height <= innerHeight &&
                Math.abs(box.width / box.height - canvas.width / canvas.height) < 0.02;
        }""",
            timeout=5000,
        )


def test_screenshot(replay, tmp_path):
    page = replay.page
    display = replay.api("/display/status")
    size = (display["stream_width"], display["stream_height"])
    if page.evaluate("window.isSecureContext"):
        page.context.grant_permissions(["clipboard-read", "clipboard-write"])
        page.locator("#btn-screenshot").click()
        expect(page.locator("#screenshot-status")).to_have_text("Screenshot copied")
        copied = page.evaluate("""async () => {
            const items = await navigator.clipboard.read();
            const image = await createImageBitmap(await items[0].getType('image/png'));
            return [image.width, image.height];
        }""")
        assert tuple(copied) == size
    else:
        with page.expect_download() as download:
            page.locator("#btn-screenshot").click()
        png = tmp_path / "frame.png"
        download.value.save_as(png)
        with Image.open(png) as image:
            image.load()
            assert image.format == "PNG" and image.size == size
            assert any(lo != hi for lo, hi in image.convert("RGB").getextrema())


def test_console(viewer):
    page = viewer.page
    sessions = viewer.api("/status/detail")["pty_sessions"]
    with page.expect_websocket(
        lambda socket: socket.url.endswith("/pty"), timeout=5000
    ) as console:
        page.locator("#btn-console-toggle").click()
    viewer.wait(lambda: viewer.api("/status/detail")["pty_sessions"] > sessions)
    expect(page.locator("#terminal .xterm-screen")).to_be_visible()
    expect(page.locator("#terminal .xterm-rows")).to_contain_text("hello-ouster-world")
    output = []
    console.value.on("framereceived", lambda message: output.append(message))
    page.locator("#terminal .xterm-helper-textarea").press("Control+c")
    viewer.wait(
        lambda: any("source config reloaded" in message for message in output),
        timeout=5,
    )
    expect(page.locator("#terminal .xterm-rows")).to_contain_text(
        "^C — source config reloaded"
    )
    page.locator("#btn-console-toggle").click()
    viewer.wait(lambda: viewer.api("/status/detail")["pty_sessions"] == sessions)


def test_guest_access(replay, browser, app):
    if app["auth"]["auth_disabled"] or not app["auth"]["enforcement"]:
        pytest.skip("Teka open mode has no guest restrictions")
    replay.configure()
    replay.toggle("guest-access-toggle", True)
    replay.wait(lambda: replay.api("/guest-access")["enabled"])
    with browser.new_context(base_url=app["base_url"]) as context:
        page = context.new_page()
        guest = Viewer(page)
        with page.expect_websocket(
            lambda socket: socket.url.endswith("/display/stream"), timeout=5000
        ) as stream:
            page.goto("/app/hello-ouster-world", wait_until="domcontentloaded")
        guest.frames()
        expect(page.locator("#btn-apply-source")).not_to_be_visible()
        response = context.request.post(
            API + "/source-config", data={"source_enabled": False}
        )
        assert response.status in (401, 403), response.text()
        guest.api("/record", "POST", expected=(401, 403), data={"active": True})
        guest.api("/pty", expected=(401, 403))
        filename = "hello-ouster-world-test-guest-" + uuid.uuid4().hex[:12] + ".osf"
        app["files"].append(filename)
        for method in ("POST", "DELETE"):
            guest.api("/osfs/" + filename, method, expected=(401, 403))
        result = page.evaluate("""() => new Promise(resolve => {
            const ws = new WebSocket(location.origin.replace(/^http/, 'ws') +
                '/api/v1/app/hello-ouster-world/display/stream');
            ws.onopen = () => ws.send(JSON.stringify({type: 'control', action: 'acquire'}));
            ws.onmessage = event => {
                if (typeof event.data !== 'string') return;
                const msg = JSON.parse(event.data);
                if (msg.type === 'control' || msg.type === 'error') { ws.close(); resolve(msg); }
            };
            ws.onerror = () => { ws.close(); resolve({type: 'connection_error'}); };
            setTimeout(() => { ws.close(); resolve({type: 'timeout'}); }, 10000);
        })""")
        assert result["type"] in ("control", "error") and not result.get("ok"), result
        replay.toggle("guest-access-toggle", False)
        replay.wait(lambda: not replay.api("/guest-access")["enabled"])
        replay.wait(lambda: stream.value.is_closed(), timeout=5)
        denied = page.evaluate("""() => new Promise(resolve => {
            const ws = new WebSocket(location.origin.replace(/^http/, 'ws') +
                '/api/v1/app/hello-ouster-world/display/stream');
            ws.onmessage = event => {
                if (typeof event.data !== 'string') { resolve(false); ws.close(); }
            };
            ws.onclose = () => resolve(true);
            ws.onerror = () => resolve(true);
            setTimeout(() => { resolve(false); ws.close(); }, 10000);
        })""")
        assert denied, "Signed-out stream accepted with guest access off"

"""Small helpers for the viewer's UI and asynchronous device state."""

import io
import time
import uuid

from PIL import Image
from playwright.sync_api import expect

API = "/api/v1/app/hello-ouster-world"
SOURCE_KEYS = (
    "source",
    "osf_path",
    "sensor_hostname",
    "record_enabled",
    "record_filename",
    "record_duration_seconds",
    "source_enabled",
)


class Viewer:
    def __init__(self, page, files=None):
        self.page = page
        self.files = files if files is not None else []
        self.errors, self.jpeg = [], []
        page.on("pageerror", lambda error: self.errors.append(str(error)))
        page.on("websocket", self.watch)

    def watch(self, socket):
        if socket.url.endswith("/display/stream"):

            def received(payload):
                if isinstance(payload, bytes):
                    self.jpeg.append(payload)
                    self.jpeg[:] = self.jpeg[-30:]

            socket.on("framereceived", received)

    def api(self, path, method="GET", expected=200, **kwargs):
        response = self.page.context.request.fetch(API + path, method=method, **kwargs)
        try:
            assert response.status in (
                (expected,) if isinstance(expected, int) else expected
            ), response.text()[:500]
            return response.json()
        finally:
            response.dispose()

    def wait(self, predicate, timeout=120):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if result := predicate():
                return result
            self.page.wait_for_timeout(250)
        raise AssertionError(f"Timed out waiting for {predicate}")

    def state(self, expected):
        def ready():
            runtime = self.api("/source-config")["runtime"]
            assert runtime["state"] not in ("error", "configuration_error"), runtime
            return runtime["state"] == expected

        self.wait(ready)

    def configure(self):
        expect(self.page.locator("#source-status-chips")).not_to_be_empty()
        if not self.page.locator("#source-fields").is_visible():
            self.page.locator("#btn-source-toggle").click()

    def toggle(self, element_id, enabled):
        checkbox = self.page.locator("#" + element_id)
        if checkbox.is_checked() != enabled:
            self.page.locator(f"label:has(#{element_id})").click()
        expect(checkbox).to_be_checked(checked=enabled)

    def apply(self):
        with self.page.expect_response(
            lambda r: r.url.endswith("/source-config") and r.request.method == "POST"
        ) as response:
            self.page.locator("#btn-apply-source").click()
        assert response.value.ok, response.value.text()

    def stop(self):
        with self.page.expect_response(
            lambda r: r.url.endswith("/source-config") and r.request.method == "POST"
        ) as response:
            self.page.locator("#btn-stop-source").click()
        assert response.value.ok, response.value.text()
        self.wait(lambda: not self.api("/record")["active"])
        self.state("stopped")
        # Discard delayed UI updates from the previous source before configuring another.
        self.page.reload(wait_until="domcontentloaded")
        self.configure()

    def start(self, source, target, *, stop=True):
        if stop:
            self.stop()
        else:
            self.configure()
        self.page.locator(f'[data-lidar="{source}"]').click()
        self.toggle("record-enabled-toggle", False)
        if source == "osf":
            self.page.locator("#cfg-osf-select").select_option(
                "/srv/recordings/" + target
            )
        elif isinstance(target, dict):
            picker = self.page.locator("#sensor-candidate-select")
            expect(picker.locator(f'option[value="{target["id"]}"]')).to_be_attached()
            picker.select_option(target["id"])
            expect(self.page.locator("#cfg-sensor-host")).to_have_value(
                target["target"]
            )
        else:
            self.page.locator("#cfg-sensor-host").fill(target)
        self.apply()
        self.state("playing_osf" if source == "osf" else "streaming")
        self.frames()

    def record(self, duration, hostname):
        self.stop()
        self.page.locator('[data-lidar="sensor"]').click()
        self.page.locator("#cfg-sensor-host").fill(hostname)
        filename = "hello-ouster-world-test-" + uuid.uuid4().hex[:12] + ".osf"
        self.files.append(filename)
        self.toggle("record-enabled-toggle", True)
        self.page.locator("#record-filename").fill(filename)
        self.page.locator("#record-duration").fill(str(duration or 60))
        self.apply()
        self.wait(lambda: self.api("/record")["active"])
        self.state("streaming")
        self.frames()
        return filename

    def frames(self, count=5):
        self.jpeg.clear()
        self.wait(lambda: len(self.jpeg) >= count)
        image = Image.open(io.BytesIO(self.jpeg[-1]))
        image.load()
        display = self.api("/display/status")
        assert image.format == "JPEG" and image.size == (
            display["stream_width"],
            display["stream_height"],
        )
        assert min(image.size) > 0 and not display["error"], display
        assert any(lo != hi for lo, hi in image.getextrema()), "Blank visualization"
        self.page.wait_for_function(
            """([width, height]) => {
            const canvas = document.getElementById('stream-canvas');
            if (canvas.width !== width || canvas.height !== height) return false;
            const pixels = canvas.getContext('2d').getImageData(0, 0, width, height).data;
            return pixels.some((value, i) => i % 4 < 3 && value !== pixels[i % 4]);
        }""",
            arg=list(image.size),
            timeout=5000,
        )
        return image

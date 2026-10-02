"""Cloud imports and the sensor recording/file lifecycle."""

import hashlib

from playwright.sync_api import expect
import pytest


def test_cloud_download(sample, viewer):
    assert any(
        f["name"] == sample and f["size"] > 0 for f in viewer.api("/osfs")["files"]
    )


@pytest.mark.parametrize("duration", [0, 10], ids=["manual", "timer"])
def test_recording(viewer, sensor_hostname, duration):
    filename = viewer.record(duration, sensor_hostname)
    record = viewer.api("/record")
    assert record["filename"] == filename
    if duration:
        assert record["expires_at"]
    else:
        viewer.api(
            "/source-config",
            "POST",
            expected=409,
            data={"source_enabled": True, "record_enabled": True},
        )
        assert viewer.api("/record")["filename"] == filename
    assert filename not in [f["name"] for f in viewer.api("/osfs")["files"]]
    for method in ("GET", "DELETE", "POST"):
        viewer.api(
            "/osfs/" + filename,
            method,
            expected=409,
            **(
                {
                    "data": b"do not overwrite",
                    "headers": {"Content-Type": "application/octet-stream"},
                }
                if method == "POST"
                else {}
            ),
        )
    if duration:
        viewer.wait(lambda: not viewer.api("/record")["active"], timeout=45)
        viewer.state("streaming")
    else:
        wrap = viewer.page.locator("#stream-wrap")
        wrap.hover()
        viewer.wait(lambda: viewer.api("/display/status")["controlling"])
        wrap.press("Space")
        viewer.frames(15)
        assert len(set(viewer.jpeg[-5:])) == 1, "Recording playback did not pause"
        viewer.stop()
    assert not viewer.api("/record")["error"]
    assert any(
        f["name"] == filename and f["size"] > 0 for f in viewer.api("/osfs")["files"]
    )
    viewer.start("osf", filename, stop=bool(duration))


def test_recording_download_delete_upload(viewer, recorded, tmp_path):
    page = viewer.page
    page.locator('[data-lidar="osf"]').click()
    page.locator("#cfg-osf-select").select_option("/srv/recordings/" + recorded)
    with page.expect_download() as download:
        page.locator("#btn-osf-download").click()
    assert download.value.suggested_filename == recorded
    local = tmp_path / recorded
    download.value.save_as(local)
    listed = next(f for f in viewer.api("/osfs")["files"] if f["name"] == recorded)
    assert local.stat().st_size == listed["size"] > 0
    with page.expect_response(
        lambda r: r.url.endswith("/osfs/" + recorded) and r.request.method == "DELETE"
    ) as response:
        page.locator("#btn-osf-delete").click()
    assert response.value.ok, response.value.text()
    assert recorded not in [f["name"] for f in viewer.api("/osfs")["files"]]
    with page.expect_response(
        lambda r: r.url.endswith("/osfs/" + recorded) and r.request.method == "POST"
    ) as response:
        page.locator("#cfg-osf-upload-input").set_input_files(str(local))
    assert response.value.ok, response.value.text()
    expect(page.locator("#cfg-osf-select")).to_have_value("/srv/recordings/" + recorded)
    with page.expect_download() as download:
        page.locator("#btn-osf-download").click()
    roundtrip = tmp_path / "roundtrip.osf"
    download.value.save_as(roundtrip)
    with local.open("rb") as original, roundtrip.open("rb") as copy:
        assert (
            hashlib.file_digest(original, "sha256").digest()
            == hashlib.file_digest(copy, "sha256").digest()
        )
    viewer.start("osf", recorded, stop=False)
    viewer.stop()

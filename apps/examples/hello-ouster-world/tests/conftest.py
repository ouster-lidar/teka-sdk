"""Authenticate and deploy once; restore device settings after the pytest session."""

import os
from pathlib import Path
import subprocess
from urllib.parse import quote, urlsplit

import pytest

from viewer import API, SOURCE_KEYS, Viewer


def pytest_addoption(parser):
    group = parser.getgroup("hello-ouster-world")
    group.addoption("--host", help="Jetstream hostname or HTTP(S) origin")
    group.addoption(
        "--sensor", help="Sensor hostname/IP; required for discovery and recording"
    )
    group.addoption(
        "--skip-deploy", action="store_true", help="Use the installed image"
    )
    group.addoption(
        "--skip-import", action="store_true", help="Use cached Studio samples"
    )
    group.addoption(
        "--apps-flake", type=Path, help="Directory containing apps/flake.nix"
    )


@pytest.fixture(scope="session")
def app(browser, pytestconfig):
    host = pytestconfig.getoption("host")
    if not host:
        raise pytest.UsageError("Supply --host HOST")
    base = (host if "://" in host else "http://" + host).rstrip("/")
    url = urlsplit(base)
    if (
        url.scheme not in ("http", "https")
        or not url.hostname
        or any((url.path, url.query, url.fragment, url.username))
    ):
        raise pytest.UsageError("--host must be an HTTP(S) origin")
    with browser.new_context(base_url=base) as context:
        response = context.request.get("/api/v1/auth/status")
        assert response.ok, response.text()
        auth = response.json()
        if not auth["auth_disabled"] and not auth["authenticated"]:
            password = os.environ.get("TEKA_PASSWORD")
            assert password, "Set TEKA_PASSWORD"
            response = context.request.post(
                "/api/v1/auth/login", data={"password": password}
            )
            assert response.ok, "Jetstream login failed"
        viewer = Viewer(context.new_page())
        response = context.request.get(API + "/record")
        assert response.status in (200, 404, 503), response.text()
        if response.ok:
            assert not response.json()["active"], "Stop the existing recording first"
            source = viewer.api("/source-config")
            assert not (source["source_enabled"] and source["record_enabled"]), (
                "Stop the configured recording first"
            )
        if not pytestconfig.getoption("skip_deploy"):
            checkout = pytestconfig.getoption("apps_flake")
            if checkout is None:
                checkout = next(
                    (
                        p
                        for parent in (Path.cwd(), *Path.cwd().parents)
                        for p in (parent / "apps", parent)
                        if (p / "examples/default.nix").is_file()
                    ),
                    None,
                )
            assert checkout and (checkout / "flake.nix").is_file(), (
                "Use --apps-flake PATH"
            )
            env = {"TEKA_POST_DEPLOY_SEC": "0", "TEKA_LOG_SEC": "2"} | os.environ
            subprocess.run(
                [
                    "teka",
                    "app",
                    "deploy",
                    "--build",
                    "hello-ouster-world",
                    "--flake",
                    str(checkout.resolve()),
                    "--host",
                    base,
                ],
                env=env,
                check=True,
            )
        viewer.wait(
            lambda: (
                (response := context.request.get(API + "/status")).ok
                and response.json()["ok"]
            )
        )
        source_config = viewer.api("/source-config")
        source = {key: source_config[key] for key in SOURCE_KEYS}
        display = viewer.api("/display/status")
        guest = viewer.api("/guest-access")
        files = []
        try:
            yield {
                "base_url": base,
                "storage_state": context.storage_state(),
                "auth": auth,
                "files": files,
                "context": context,
            }
        finally:
            operations = [
                (
                    "/source-config",
                    "POST",
                    {"source_enabled": False, "record_enabled": False},
                ),
                *[("/osfs/" + quote(name), "DELETE", None) for name in files],
                ("/source-config", "POST", source),
                ("/display/hdmi", "POST", {"enabled": display["hdmi"]}),
                ("/display/stream-enabled", "POST", {"enabled": display["stream"]}),
                ("/guest-access", "PUT", {"enabled": guest["enabled"]}),
            ]
            failures = []
            for path, method, data in operations:
                try:
                    viewer.api(
                        path,
                        method,
                        expected=(200, 404) if method == "DELETE" else 200,
                        **({"data": data} if data is not None else {}),
                    )
                except Exception as error:
                    failures.append(str(error))
            assert not failures, "Cleanup failed: " + "; ".join(failures)


@pytest.fixture(scope="session")
def browser_context_args(browser_context_args, app):
    return browser_context_args | {
        "base_url": app["base_url"],
        "storage_state": app["storage_state"],
        "viewport": {"width": 1440, "height": 1080},
        "accept_downloads": True,
    }


@pytest.fixture
def viewer(page, app):
    viewer = Viewer(page, app["files"])
    viewer.api("/display/stream-enabled", "POST", data={"enabled": True})
    page.goto("/app/hello-ouster-world", wait_until="domcontentloaded")
    yield viewer
    assert not viewer.errors, viewer.errors


@pytest.fixture(scope="session")
def sensor_hostname(pytestconfig):
    hostname = pytestconfig.getoption("sensor")
    if not hostname:
        pytest.skip("Provide --sensor HOSTNAME for this test")
    return hostname


@pytest.fixture
def sensor(sensor_hostname, viewer):
    viewer.start("sensor", sensor_hostname)
    return viewer


@pytest.fixture
def recorded(viewer, sensor_hostname):
    filename = viewer.record(0, sensor_hostname)
    viewer.frames(15)
    viewer.stop()
    assert not viewer.api("/record")["error"]
    return filename


@pytest.fixture
def replay(viewer, sample):
    viewer.start("osf", sample)
    return viewer


@pytest.fixture(scope="session")
def sample(app, pytestconfig):
    with app["context"].new_page() as page:
        viewer = Viewer(page, app["files"])
        page.goto("/app/hello-ouster-world", wait_until="domcontentloaded")
        catalog = viewer.api("/osfs")
        assert catalog["seeds"] and catalog["sample_library_url"].startswith(
            "https://studio.ouster.com/"
        )
        seed = next((s for s in catalog["seeds"] if s["present"]), None)
        if not pytestconfig.getoption("skip_import"):
            seed = next((s for s in catalog["seeds"] if not s["present"]), seed)
        if seed is None:
            pytest.skip("No cached Studio sample; --skip-import disables downloading")
        viewer.stop()
        viewer.page.locator('[data-lidar="osf"]').click()
        if seed["present"]:
            assert viewer.api("/osfs/imports", "POST", data={"name": seed["name"]})[
                "already_present"
            ]
            viewer.start("osf", seed["name"], stop=False)
        else:
            viewer.files.append(seed["name"])
            viewer.page.locator("#cfg-osf-select").select_option("seed:" + seed["name"])
            with viewer.page.expect_response(
                lambda r: r.url.endswith("/osfs/imports") and r.request.method == "POST"
            ) as response:
                viewer.page.locator("#btn-apply-source").click()
            assert response.value.ok and not response.value.json()["already_present"]

            def complete():
                status = viewer.api("/osfs/imports")
                assert status["state"] != "error", status
                return status["state"] == "idle"

            viewer.wait(complete, timeout=1800)
            viewer.wait(
                lambda: (
                    viewer.api("/source-config")["osf_path"]
                    == "/srv/recordings/" + seed["name"]
                )
            )
            viewer.state("playing_osf")
            viewer.frames()
        return seed["name"]

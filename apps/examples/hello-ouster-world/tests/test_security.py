"""API access control: signed-out, guest and signed-in callers, plus input confinement."""

import pytest

from viewer import API

# (method, path) for every route the backend serves.
ROUTES = [
    ("GET", "/status"),
    ("GET", "/status/detail"),
    ("GET", "/status/stream"),
    ("GET", "/cards/stream"),
    ("GET", "/pty"),
    ("GET", "/source-config"),
    ("POST", "/source-config"),
    ("GET", "/record"),
    ("POST", "/record"),
    ("GET", "/sensors/picker"),
    ("GET", "/guest-access"),
    ("PUT", "/guest-access"),
    ("GET", "/display/status"),
    ("GET", "/display/stream"),
    ("POST", "/display/stream-enabled"),
    ("POST", "/display/hdmi"),
    ("POST", "/display/fps"),
    ("GET", "/osfs"),
    ("GET", "/osfs/imports"),
    ("POST", "/osfs/imports"),
    ("GET", "/osfs/security-test.osf"),
    ("POST", "/osfs/security-test.osf"),
    ("DELETE", "/osfs/security-test.osf"),
]
PUBLIC = {("GET", "/status"), ("GET", "/guest-access")}
GUEST = PUBLIC | {("GET", "/display/stream"), ("GET", "/display/status")}
DENIED = (401, 403)


@pytest.fixture
def signed_out(browser, app):
    if app["auth"]["auth_disabled"] or not app["auth"]["enforcement"]:
        pytest.skip("Jetstream open mode has no signed-out callers")
    with browser.new_context(base_url=app["base_url"]) as context:
        yield context.request


@pytest.fixture
def editor(app):
    return app["context"].request


@pytest.fixture
def set_guest(editor):
    original = editor.get(API + "/guest-access").json()["enabled"]

    def set_(enabled):
        assert editor.put(API + "/guest-access", data={"enabled": enabled}).ok

    yield set_
    set_(original)


def probe(request, method, path):
    return request.fetch(API + path, method=method, data={}).status


def check(request, allowed):
    failures = []
    for method, path in ROUTES:
        status = probe(request, method, path)
        if ((method, path) in allowed) != (status not in DENIED):
            failures.append(f"{method} {path} -> {status}")
    assert not failures, failures


def test_signed_out_reaches_only_public_routes(signed_out, set_guest):
    set_guest(False)
    check(signed_out, PUBLIC)


def test_guest_reaches_only_essentials(signed_out, set_guest):
    set_guest(True)
    check(signed_out, GUEST)


def test_signed_in_reaches_every_route(editor):
    # Skip routes that would change device state or write a file with an empty body.
    unsafe = {
        ("POST", "/source-config"),
        ("POST", "/osfs/security-test.osf"),
        ("DELETE", "/osfs/security-test.osf"),
    }
    for method, path in ROUTES:
        if (method, path) not in unsafe:
            assert probe(editor, method, path) not in DENIED, f"{method} {path}"


def test_unauthenticated_status_is_liveness_only(signed_out):
    assert signed_out.get(API + "/status").json() == {
        "ok": True,
        "app": "hello-ouster-world",
    }


def test_unknown_routes_are_not_served(signed_out):
    assert probe(signed_out, "GET", "/not-a-route") in (*DENIED, 404)


@pytest.mark.parametrize(
    "body",
    [
        {"source": "local_file", "osf_path": "/etc/passwd"},
        {"source": "local_file", "osf_path": "/srv/recordings/../../etc/x.osf"},
        {"source": "local_file", "osf_path": "/srv/recordings/sub/a.osf"},
        {"source": "sensor", "sensor_hostname": "http://169.254.169.254/"},
        {"source": "sensor", "sensor_hostname": "a b"},
        {"source": "sensor", "sensor_hostname": "x" * 300},
    ],
)
def test_source_config_rejects_unconfined_targets(editor, body):
    before = editor.get(API + "/source-config").json()
    response = editor.post(API + "/source-config", data=body)
    assert response.status == 400, response.text()
    after = editor.get(API + "/source-config").json()
    assert {k: after[k] for k in body} == {k: before[k] for k in body}


@pytest.mark.parametrize(
    "name", ["../evil.osf", "..%2Fevil.osf", "a%5Cb.osf", "notes.txt", "x.osf.partial"]
)
def test_osf_names_are_confined(editor, name):
    for method in ("POST", "DELETE", "GET"):
        status = editor.fetch(API + "/osfs/" + name, method=method).status
        assert status in (400, 404, 405), f"{method} {name} -> {status}"

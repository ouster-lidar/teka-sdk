"""Discover the explicitly supplied sensor and select it in the UI."""


def test_sensor_discovery(sensor_hostname, viewer):
    matches = viewer.wait(
        lambda: [
            candidate
            for candidate in viewer.api("/source-config")["discovery"]["candidates"]
            if sensor_hostname in (candidate["target"], candidate["hostname"])
        ]
    )
    viewer.start("sensor", matches[0])

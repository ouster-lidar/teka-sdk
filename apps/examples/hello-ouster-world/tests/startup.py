"""Sensor startup recovery without sensor hardware or the visualization SDK."""

from pathlib import Path
import select
import subprocess
import sys
from types import ModuleType
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sdk = ModuleType("ouster.sdk")
sdk.open_source = mock.Mock()
sdk.osf = mock.Mock()
viz = ModuleType("ouster.sdk.viz")
viz.SimpleViz = mock.Mock()
bridge = ModuleType("viz_utils")
bridge.JetstreamDisplayBridge = mock.Mock()
bridge.PointCloudViz = mock.Mock()
bridge.VizPid = mock.Mock()
with mock.patch.dict(sys.modules, {"ouster": ModuleType("ouster"), "ouster.sdk": sdk,
                                  "ouster.sdk.viz": viz, "viz_utils": bridge}):
    import hello_ouster_world as world


def connection_error(reason="Could not connect to server"):
    return RuntimeError("CurlClient::execute_request failed for the url: "
                        "[http://sensor/api/v1/sensor/metadata/sensor_info] "
                        "with the error message: " + reason)


class StartupTests(unittest.TestCase):
    def setUp(self):
        self.now = 0
        self.watch = mock.Mock(reload_requested=mock.Mock(return_value=False))
        for name, replacement in (
            ("time", mock.Mock(monotonic=lambda: self.now, sleep=self.sleep)),
            ("set_runtime_status", mock.Mock()),
            ("_configure_sensor", mock.Mock()),
        ):
            patch = mock.patch.object(world, name, replacement)
            patch.start()
            self.addCleanup(patch.stop)
        patch = mock.patch("builtins.print")
        patch.start()
        self.addCleanup(patch.stop)

    def sleep(self, seconds):
        self.now += seconds

    def test_recovers_from_connection_and_timeout_failures(self):
        world._configure_sensor.side_effect = [
            connection_error(), connection_error("Couldn't connect to server"),
            connection_error("Timeout was reached"), None,
        ]
        world._configure_sensor_with_retry("sensor", self.watch)
        self.assertEqual(world._configure_sensor.call_count, 4)
        self.assertAlmostEqual(self.now, 6)
        world.set_runtime_status.assert_called_with("configuring", "Waiting for sensor connection to sensor")

    def test_other_errors_require_apply(self):
        for error in (RuntimeError("sensor did not accept defaults: udp_port_lidar=0"),
                      RuntimeError("CurlClient::execute_request failed for url: [sensor] with the code: [400]"),
                      connection_error("Could not resolve host")):
            with self.subTest(error=error):
                world._configure_sensor.side_effect = error
                with mock.patch.object(world, "wait_for_source_config_change") as wait:
                    with self.assertRaises(world.SourceConfigChanged):
                        world.main({"source": "sensor", "sensor_hostname": "sensor"}, self.watch)
                wait.assert_called_once_with(self.watch)
                world.set_runtime_status.assert_called_with("configuration_error", str(error))
                self.assertEqual(self.now, 0)

    def test_retry_budget_exhaustion_preserves_error(self):
        error = connection_error()
        world._configure_sensor.side_effect = error
        with self.assertRaises(RuntimeError) as raised:
            world._configure_sensor_with_retry("sensor", self.watch)
        self.assertIs(raised.exception, error)
        self.assertEqual(self.now, 60)
        self.assertEqual(world._configure_sensor.call_count, 30)

    def test_apply_interrupts_wait_without_waiting_for_another_apply(self):
        world._configure_sensor.side_effect = connection_error()
        self.watch.reload_requested.side_effect = lambda: self.now > 0
        with mock.patch.object(world, "wait_for_source_config_change") as wait:
            with self.assertRaises(world.SourceConfigChanged):
                world.main({"source": "sensor", "sensor_hostname": "sensor"}, self.watch)
        wait.assert_not_called()
        self.assertEqual(world._configure_sensor.call_count, 1)
        self.assertLessEqual(self.now, 0.2)

    def test_sigterm_interrupts_retry_wait(self):
        with subprocess.Popen([sys.executable, __file__, "--shutdown-child"],
                              stdout=subprocess.PIPE, text=True) as child:
            try:
                self.assertTrue(select.select([child.stdout], [], [], 5)[0])
                self.assertEqual(child.stdout.readline().strip(), "waiting")
                child.terminate()
                self.assertEqual(child.wait(timeout=5), -15)
            finally:
                if child.poll() is None:
                    child.kill()


if __name__ == "__main__":
    if sys.argv[1:] == ["--shutdown-child"]:
        world._configure_sensor = mock.Mock(side_effect=connection_error())
        world.set_runtime_status = lambda *_: print("waiting", flush=True)
        world._configure_sensor_with_retry("sensor", mock.Mock(reload_requested=lambda: False))
    else:
        unittest.main()

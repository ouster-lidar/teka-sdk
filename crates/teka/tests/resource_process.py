"""Resource-only client lifecycle; host VM tests cover native crash reclamation."""

import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class RuntimeHandler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, status, body):
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path.endswith("/register"):
            if self.server.reject_register:
                self.reply(409, {"error": "register rejected"})
                return
            event = threading.Event()
            self.server.streams.append(event)
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"\n")
            self.wfile.flush()
            event.wait(30)
        else:
            self.reply(200, {})

    def do_PUT(self):
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        self.server.calls.append((self.path, json.loads(raw)))
        status = 409 if self.server.reject_ports else 200
        self.server.reject_ports = max(0, self.server.reject_ports - 1)
        self.reply(status, {"ok": status == 200})

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        self.server.calls.append((self.path, json.loads(raw or b"{}")))
        if self.path.endswith("/activate"):
            if self.server.activation_gate:
                self.server.activation_gate.wait(10)
            self.reply(409 if self.server.reject_activation else 200, {
                "ok": not self.server.reject_activation,
                "app_id": "hello-ouster-terminal",
                "error": "activation rejected" if self.server.reject_activation else None,
            })
        else:
            self.reply(500 if self.server.reject_release else 204, {})


CHILD = """
import os, signal, sys, time
signal.signal(signal.SIGINT, lambda *_: sys.exit(42))
open(sys.argv[1], 'w').write(str(os.getpid()))
while True: time.sleep(1)
"""


class ProcessFixture(unittest.TestCase):
    app_id = "hello-ouster-terminal"
    proto = "udp"

    def wait_for(self, predicate):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.05)
        self.fail("timed out waiting for lifecycle event")

    def calls(self, suffix):
        return [body for path, body in self.mc.calls if path.endswith(suffix)]

    def released(self):
        self.assertTrue(self.calls("/release"))
        self.assertTrue(all(f"/runtime/{self.app_id}/" in path for path, _ in self.mc.calls))
        if self.pid_file.exists():
            with self.assertRaises(ProcessLookupError):
                os.kill(int(self.pid_file.read_text()), 0)

    def tearDown(self):
        if self.proc:
            if self.proc.poll() is None:
                self.proc.terminate()
            self.proc.communicate(timeout=20)
        for event in self.mc.streams:
            event.set()
        self.mc.shutdown()
        self.mc.server_close()
        self.notify.close()
        self.temp.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.pid_file = Path(self.temp.name) / "pid"
        self.notify = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        self.notify.bind(self.temp.name + "/notify")
        self.notify.settimeout(0.2)
        self.mc = ThreadingHTTPServer(("127.0.0.1", 0), RuntimeHandler)
        self.mc.reject_register = False
        self.mc.reject_activation = False
        self.mc.activation_gate = None
        self.mc.reject_ports = 0
        self.mc.reject_release = False
        self.mc.calls = []
        self.mc.streams = []
        self.thread = threading.Thread(target=self.mc.serve_forever, daemon=True)
        self.thread.start()
        self.proc = None

    def start(self, command=None, ports="7000-8000"):
        command = command or [sys.executable, "-c", CHILD, str(self.pid_file)]
        self.proc = subprocess.Popen([
            os.environ["TEKA_RUNNER"], self.app_id, self.proto, ports, *command,
        ], env={**os.environ,
            "APP_ID": "wrong-holder",
            "MC_RUNTIME_URL": f"http://127.0.0.1:{self.mc.server_port}",
            "NOTIFY_SOCKET": self.temp.name + "/notify",
        }, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def ready(self):
        self.notify.settimeout(5)
        self.assertIn(b"READY=1", self.notify.recv(1024))
        self.assertTrue(self.calls("/ports"))
        self.wait_for(self.pid_file.exists)


class ResourceTests(ProcessFixture):
    def test_full_range_and_graceful_release(self):
        self.start()
        self.ready()
        self.assertEqual(self.calls("/activate"), [{}])
        self.assertEqual(self.calls("/ports"), [{"ports": [
            {"port": port, "proto": self.proto} for port in range(7000, 8001)
        ]}])
        self.proc.terminate()
        self.assertEqual(self.proc.wait(timeout=15), -signal.SIGTERM)
        self.released()

    def test_custom_range_and_interrupt_forwarding(self):
        self.start(ports="9000-9002")
        self.ready()
        self.assertEqual(self.calls("/ports")[0], {"ports": [
            {"port": port, "proto": self.proto} for port in range(9000, 9003)
        ]})
        self.proc.send_signal(signal.SIGINT)
        self.assertEqual(self.proc.wait(timeout=15), 42)
        self.released()

    def test_activation_failure_does_not_start_child(self):
        self.mc.reject_activation = True
        self.start()
        self.assertEqual(self.proc.wait(timeout=10), 1)
        self.assertFalse(self.pid_file.exists())
        self.assertFalse(self.calls("/ports"))
        self.released()
        with self.assertRaises(socket.timeout):
            self.notify.recv(1024)

    def test_port_failure_does_not_start_child_and_releases(self):
        self.mc.reject_ports = 1
        self.start()
        self.assertEqual(self.proc.wait(timeout=15), 1)
        self.assertFalse(self.pid_file.exists())
        self.released()
        with self.assertRaises(socket.timeout):
            self.notify.recv(1024)

    def test_signal_during_acquisition_releases_without_starting_child(self):
        self.mc.activation_gate = threading.Event()
        self.start()
        self.wait_for(lambda: self.calls("/activate"))
        self.proc.terminate()
        time.sleep(0.2)
        self.mc.activation_gate.set()
        self.assertEqual(self.proc.wait(timeout=15), -signal.SIGTERM)
        self.assertFalse(self.pid_file.exists())
        self.assertFalse(self.calls("/ports"))
        self.released()
        self.assertTrue(self.mc.calls[-1][0].endswith("/release"))
        with self.assertRaises(socket.timeout):
            self.notify.recv(1024)

    def test_spawn_failure_releases(self):
        self.start(["/nonexistent-terminal-child"])
        # posix_spawn may report exec failure through child exit status 127.
        self.assertIn(self.proc.wait(timeout=15), (1, 127))
        self.released()

    def test_child_exit_preserves_status_and_releases(self):
        for code in (7, 130, 143):
            with self.subTest(code=code):
                releases = len(self.calls("/release"))
                self.start([sys.executable, "-c", f"raise SystemExit({code})"])
                self.proc.communicate(timeout=15)
                self.assertEqual(self.proc.returncode, code)
                self.assertEqual(len(self.calls("/release")), releases + 1)
                self.released()

    def test_child_interrupt_preserves_signal_and_releases(self):
        command = CHILD.replace(
            "signal.signal(signal.SIGINT, lambda *_: sys.exit(42))",
            "signal.signal(signal.SIGINT, signal.SIG_DFL)",
        )
        self.start([sys.executable, "-c", command, str(self.pid_file)])
        self.ready()
        self.proc.send_signal(signal.SIGINT)
        self.assertEqual(self.proc.wait(timeout=15), -signal.SIGINT)
        self.released()

    def test_child_crash_preserves_status_and_releases(self):
        self.start()
        self.ready()
        os.kill(int(self.pid_file.read_text()), signal.SIGKILL)
        self.assertEqual(self.proc.wait(timeout=15), -signal.SIGKILL)
        self.released()

    def test_registration_loss_stops_dependent_child(self):
        self.start()
        self.ready()
        self.wait_for(lambda: self.mc.streams)
        self.mc.reject_ports = 20
        self.mc.streams[0].set()
        self.assertEqual(self.proc.wait(timeout=15), 1)
        self.released()
        self.assertEqual(len(self.calls("/ports")), 1)
        self.assertTrue(self.mc.calls[-1][0].endswith("/release"))

    def test_stop_during_registration_retry_does_not_reactivate_after_release(self):
        self.mc.reject_register = True
        self.start()
        self.wait_for(lambda: len(self.calls("/activate")) >= 2)
        self.assertFalse(self.pid_file.exists())
        self.proc.terminate()
        self.assertEqual(self.proc.wait(timeout=15), -signal.SIGTERM)
        self.released()
        self.assertTrue(self.mc.calls[-1][0].endswith("/release"))


    def test_release_failure_is_visible_without_masking_exit(self):
        self.mc.reject_release = True
        self.start([sys.executable, "-c", "raise SystemExit(7)"])
        _, stderr = self.proc.communicate(timeout=15)
        self.assertEqual(self.proc.returncode, 7)
        self.assertIn(b"500", stderr)
        self.assertTrue(self.calls("/release"))

    def test_exit_kills_descendants_that_ignore_termination(self):
        descendant = Path(self.temp.name) / "descendant"
        command = """
import os, signal, subprocess, sys, time
subprocess.Popen([sys.executable, '-c',
    'import os,signal,sys,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); '
    'open(sys.argv[1], "w").write(str(os.getpid())); time.sleep(60)', sys.argv[2]])
open(sys.argv[1], 'w').write(str(os.getpid()))
while True: time.sleep(1)
"""
        self.start([sys.executable, "-c", command, str(self.pid_file), str(descendant)])
        self.ready()
        self.wait_for(descendant.exists)
        self.proc.terminate()
        self.proc.wait(timeout=15)
        pid = int(descendant.read_text())
        def dead():
            status = Path(f"/proc/{pid}/stat")
            return not status.exists() or status.read_text().split()[2] == "Z"
        self.wait_for(dead)
        self.released()


class TcpTests(ResourceTests):
    app_id = "hello-http"
    proto = "tcp"


if __name__ == "__main__":
    unittest.main()

"""Model-free supervisor checks for the WebUI/Ollama process topology."""

import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "lib/app-runtime/tests"))
import resource_process


class LlmProcessTests(resource_process.ProcessFixture):
    app_id = "hello-llm"
    proto = "tcp"

    def test_shutdown_stops_background_backend(self):
        backend_pid = Path(self.temp.name) / "backend-pid"
        code = """
import signal, subprocess, sys
backend = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
open(sys.argv[1], 'w').write(str(backend.pid))
sys.argv = sys.argv[2:]
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
try:
    exec(sys.argv[0])
finally:
    try:
        backend.wait(timeout=5)
    except subprocess.TimeoutExpired:
        backend.kill()
        backend.wait()
        raise
"""
        self.start([sys.executable, "-c", code, str(backend_pid), resource_process.CHILD,
                    str(self.pid_file)])
        self.ready()
        self.proc.terminate()
        self.assertEqual(self.proc.wait(timeout=15), 0)
        self.released()
        with self.assertRaises(ProcessLookupError):
            os.kill(int(backend_pid.read_text()), 0)


if __name__ == "__main__":
    unittest.main()

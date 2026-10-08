"""Live SDK handoff keeps fresh scans without skipping upstream recording."""

import threading
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from viz_utils import LatestLiveConsumer as LiveConsumer


class LiveConsumerTests(unittest.TestCase):
    def check_latest_frame(self, count_dropped):
        consumed = []
        ready = threading.Event()
        release = threading.Event()

        def source():
            for frame in range(100):
                consumed.append(frame)
                yield frame
            ready.set()
            release.wait(timeout=5)

        reader = LiveConsumer(source(), lambda: count_dropped)
        reader.start()
        try:
            self.assertTrue(ready.wait(timeout=2), "Viewer blocked ingestion")
            self.assertEqual(next(reader), 99)
            self.assertEqual(consumed, list(range(100)))
            self.assertEqual(reader.dropped_frames, 99 if count_dropped else 0)
        finally:
            release.set()
            reader.shutdown()

    def test_stalled_viewer_receives_latest_frame(self):
        self.check_latest_frame(True)

    def test_paused_viewer_receives_latest_without_counting_drops(self):
        self.check_latest_frame(False)


if __name__ == "__main__":
    unittest.main()

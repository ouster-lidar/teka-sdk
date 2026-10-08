"""Calibrated reflectivity retains byte values and legacy fallbacks."""

from pathlib import Path
import sys
import unittest

from ouster.sdk import core
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from viz_utils import ReflectivityMode as ReflMode
import numpy as np


class ReflectivityTests(unittest.TestCase):
    def setUp(self):
        self.meta = core.SensorInfo.from_default(core.LidarMode._1024x10)

    def frame(self, dtype):
        frame = core.LidarFrame(self.meta, [
            core.FieldType(name, dtype) for name in ("REFLECTIVITY", "REFLECTIVITY2")
        ])
        for name in ("REFLECTIVITY", "REFLECTIVITY2"):
            data = frame.field(name)
            data[:] = np.arange(data.size).reshape(data.shape) % 256
        return frame

    def test_calibrated_bytes_preserve_both_returns(self):
        frame = self.frame(np.uint8)
        mode = ReflMode(info=self.meta)
        mode._normalized_refl = True
        for return_num, field in enumerate(("REFLECTIVITY", "REFLECTIVITY2")):
            actual = mode._prepare_data(frame, return_num)
            self.assertEqual(actual.dtype, np.uint8)
            np.testing.assert_array_equal(actual, frame.field(field))

    def test_other_types_and_uncalibrated_fields_keep_float_path(self):
        for dtype, normalized in ((np.uint16, True), (np.uint8, False)):
            with self.subTest(dtype=dtype, normalized=normalized):
                frame = self.frame(dtype)
                mode = ReflMode(info=self.meta)
                mode._normalized_refl = normalized
                for return_num in (0, 1):
                    self.assertEqual(mode._prepare_data(frame, return_num).dtype,
                                     np.float32)
        empty = core.LidarFrame(self.meta, [])
        self.assertIsNone(ReflMode(info=self.meta)._prepare_data(empty))


if __name__ == "__main__":
    unittest.main()

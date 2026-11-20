"""Exercise the real fixture supervisor with bounded synthetic children."""

import os
from pathlib import Path
import signal
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import m1_limits


MIB = 1 << 20


@unittest.skipUnless(sys.platform in ("linux", "darwin"), "needs Darwin or Linux fixture telemetry")
class RealLimitsTests(unittest.TestCase):
    def setUp(self):
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.cwd = Path(directory)

    def run_fixture(self, source, *, timeout=4, rss_limit_bytes=512 * MIB):
        return m1_limits.run_limited(
            [sys.executable, "-I", "-c", source], cwd=self.cwd,
            timeout=timeout, rss_limit_bytes=rss_limit_bytes,
        )

    def test_reports_individual_peak_and_preserves_child_diagnostics(self):
        self.assertIn(m1_limits.rss_backend(), (
            "darwin-libproc-wait4", "linux-smaps-rollup-wait4",
        ))
        result = self.run_fixture(
            "import sys; assert sys.stdin.buffer.read() == b''; "
            "print('output'); print('diagnosis', file=sys.stderr); sys.exit(3)"
        )
        self.assertEqual(result.returncode, 3)
        self.assertEqual(result.stdout, b"output\n")
        self.assertEqual(result.stderr, b"diagnosis\n")
        self.assertIsNone(result.termination)
        self.assertIsNone(result.trigger)
        self.assertGreater(result.kernel_peak_rss_bytes, 0)
        self.assertGreater(result.elapsed_seconds, 0)

    def test_live_rss_overage_kills_the_fixture_and_keeps_prior_output(self):
        result = self.run_fixture(
            "import sys, time\n"
            "print('ready', file=sys.stderr, flush=True)\n"
            "data = bytearray(96 * 1024**2)\n"
            "for index in range(0, len(data), 4096): data[index] = 1\n"
            "time.sleep(6)\n",
            rss_limit_bytes=64 * MIB,
        )
        self.assertEqual(result.returncode, -signal.SIGKILL)
        self.assertEqual(result.termination, "resource_limit")
        self.assertEqual(result.trigger, "sampled_rss")
        self.assertGreater(result.sampled_peak_rss_bytes, 64 * MIB)
        self.assertEqual(result.stderr, b"ready\n")
        self.assertLess(result.elapsed_seconds, 4)

    def test_peak_verification_denies_overage_after_successful_child_exit(self):
        with patch.object(m1_limits, "RSS_SAMPLE_INTERVAL_SECONDS", 30):
            result = self.run_fixture(
                "import time\n"
                "time.sleep(0.2)\n"
                "data = bytearray(96 * 1024**2)\n"
                "for index in range(0, len(data), 4096): data[index] = 1\n"
                "print('complete')\n",
                rss_limit_bytes=64 * MIB,
            )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.termination, "resource_limit")
        self.assertEqual(result.trigger, "peak_rss")
        self.assertGreater(result.kernel_peak_rss_bytes, 64 * MIB)
        self.assertEqual(result.stdout, b"complete\n")

    def test_timeout_kills_group_and_preserves_partial_streams(self):
        marker = self.cwd / "descendant-was-running"
        descendant = (
            "import pathlib, time; time.sleep(1); "
            f"pathlib.Path({str(marker)!r}).write_text('late')"
        )
        result = self.run_fixture(
            "import subprocess, sys, time; "
            f"subprocess.Popen([sys.executable, '-I', '-c', {descendant!r}]); "
            "print('early', flush=True); print('context', file=sys.stderr, flush=True); "
            "time.sleep(6)",
            timeout=0.6,
        )
        self.assertEqual(result.returncode, -signal.SIGKILL)
        self.assertEqual(result.termination, "timeout")
        self.assertEqual(result.trigger, "wall_time")
        self.assertEqual(result.stdout, b"early\n")
        self.assertEqual(result.stderr, b"context\n")
        self.assertGreaterEqual(result.elapsed_seconds, 0.6)
        self.assertLess(result.elapsed_seconds, 2)
        time.sleep(0.6)
        self.assertFalse(marker.exists())

    def test_drains_both_pipes_without_waiting_for_child_exit(self):
        result = self.run_fixture(
            "import sys; sys.stdout.buffer.write(b'A' * 200_000); "
            "sys.stderr.buffer.write(b'B' * 200_000)"
        )
        self.assertIsNone(result.termination)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"A" * 200_000)
        self.assertEqual(result.stderr, b"B" * 200_000)

    def test_diagnostic_capture_has_a_separate_limit(self):
        with patch.object(m1_limits, "OUTPUT_LIMIT_BYTES", 40_000):
            result = self.run_fixture("import sys; sys.stdout.buffer.write(b'A' * 500_000)")
        self.assertEqual(result.termination, "resource_limit")
        self.assertEqual(result.trigger, "diagnostic_bytes")
        self.assertEqual(result.stdout, b"A" * 40_000)


class LimitsPreflightTests(unittest.TestCase):
    def test_backend_error_prevents_child_start(self):
        with patch.object(m1_limits, "_backend", side_effect=OSError("RSS unavailable")), (
            patch.object(m1_limits.subprocess, "Popen")
        ) as launch:
            with self.assertRaisesRegex(OSError, "RSS unavailable"):
                m1_limits.run_limited(["fixture"], cwd=Path.cwd(), timeout=2, rss_limit_bytes=MIB)
        launch.assert_not_called()

    def test_rejects_invalid_limits_before_child_start(self):
        with patch.object(m1_limits.subprocess, "Popen") as launch:
            for timeout, rss in ((float("nan"), MIB), (0, MIB), (1, 0), (1, True)):
                with self.subTest(timeout=timeout, rss=rss), self.assertRaises(ValueError):
                    m1_limits.run_limited(["fixture"], cwd=Path.cwd(), timeout=timeout, rss_limit_bytes=rss)
        launch.assert_not_called()

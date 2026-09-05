import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock


class ProcessEvidenceTests(unittest.TestCase):
    def test_capture_io_failure_fails_closed(self):
        from bench.process_evidence import ProcessEvidenceError, run_bounded_command

        with tempfile.TemporaryDirectory() as temporary, mock.patch(
            "bench.process_evidence._capture_pipe", side_effect=OSError("disk full")
        ):
            root = Path(temporary)
            with self.assertRaisesRegex(ProcessEvidenceError, "capture"):
                run_bounded_command(
                    [sys.executable, "-c", "print('output')"],
                    stdout_path=root / "stdout.txt",
                    stderr_path=root / "stderr.txt",
                    timeout_secs=5,
                )

    def test_gnu_time_metrics_use_kib_and_linux_flags(self):
        from bench.process_evidence import _measurement_command, parse_gnu_time_metrics

        self.assertEqual(
            parse_gnu_time_metrics(
                "wall_seconds=1.25\nuser_seconds=0.50\n"
                "system_seconds=0.10\npeak_rss_kib=123\n"
            ),
            {
                "wall_seconds": 1.25,
                "user_seconds": 0.5,
                "system_seconds": 0.1,
                "peak_rss_bytes": 123 * 1024,
            },
        )
        command = _measurement_command(
            ["wsi-dicom", "--version"], Path("resources.txt"), "Linux"
        )
        self.assertEqual(command[0], "/usr/bin/time")
        self.assertIn("-f", command)
        self.assertNotIn("-lp", command)

    def test_output_is_capped_while_observed_size_and_truncation_are_retained(self):
        from bench.process_evidence import run_bounded_command

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stdout = root / "stdout.txt"
            stderr = root / "stderr.txt"
            result = run_bounded_command(
                [
                    sys.executable,
                    "-c",
                    "import sys; sys.stdout.write('x' * 4096); sys.stderr.write('y' * 2048)",
                ],
                stdout_path=stdout,
                stderr_path=stderr,
                timeout_secs=5,
                max_output_bytes=64,
            )

            self.assertEqual(stdout.stat().st_size, 64)
            self.assertEqual(stderr.stat().st_size, 64)
            self.assertEqual(result["stdout_bytes"], 4096)
            self.assertEqual(result["stderr_bytes"], 2048)
            self.assertTrue(result["stdout_truncated"])
            self.assertTrue(result["stderr_truncated"])

    def test_success_records_bounded_process_evidence(self):
        from bench.process_evidence import run_bounded_command

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stdout = root / "stdout.txt"
            stderr = root / "stderr.txt"
            result = run_bounded_command(
                [sys.executable, "-c", "import sys; print('ok'); print('warn', file=sys.stderr)"],
                stdout_path=stdout,
                stderr_path=stderr,
                timeout_secs=5,
            )

        self.assertEqual(result["returncode"], 0)
        self.assertFalse(result["timed_out"])
        self.assertIsNone(result["launch_error"])
        self.assertEqual(result["command"][0], sys.executable)
        self.assertGreaterEqual(result["elapsed_seconds"], 0)
        self.assertEqual(result["stdout_path"], str(stdout))
        self.assertEqual(result["stderr_path"], str(stderr))

    def test_timeout_is_evidence_not_an_exception(self):
        from bench.process_evidence import run_bounded_command

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = run_bounded_command(
                [sys.executable, "-c", "import time; time.sleep(2)"],
                stdout_path=root / "stdout.txt",
                stderr_path=root / "stderr.txt",
                timeout_secs=1,
            )

        self.assertIsNone(result["returncode"])
        self.assertTrue(result["timed_out"])
        self.assertIsNone(result["launch_error"])

    @unittest.skipUnless(os.name == "posix", "process-group regression is POSIX-specific")
    def test_timeout_terminates_descendants_before_they_can_publish_output(self):
        from bench.process_evidence import run_bounded_command

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            sentinel = root / "descendant-survived"
            child = (
                "import pathlib,signal,sys,time; "
                "signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(2); "
                "pathlib.Path(sys.argv[1]).write_text('survived')"
            )
            parent = (
                "import subprocess,sys,time; "
                "subprocess.Popen([sys.executable, '-c', sys.argv[1], sys.argv[2]]); "
                "time.sleep(10)"
            )
            result = run_bounded_command(
                [sys.executable, "-c", parent, child, str(sentinel)],
                stdout_path=root / "stdout.txt",
                stderr_path=root / "stderr.txt",
                timeout_secs=1,
            )
            time.sleep(1.5)

            self.assertTrue(result["timed_out"])
            self.assertFalse(sentinel.exists())

    def test_resource_measurement_is_portable_and_structured(self):
        from bench.process_evidence import run_bounded_command

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = run_bounded_command(
                [sys.executable, "-c", "print('measured')"],
                stdout_path=root / "stdout.txt",
                stderr_path=root / "stderr.txt",
                timeout_secs=5,
                measure_resources=True,
            )

            self.assertEqual(result["returncode"], 0)
            self.assertIsInstance(result["resource_usage"], dict)
            self.assertGreater(result["resource_usage"]["peak_rss_bytes"], 0)
            self.assertTrue(Path(result["resource_usage_path"]).is_file())


if __name__ == "__main__":
    unittest.main()

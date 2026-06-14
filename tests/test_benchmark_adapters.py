import importlib
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
BENCH_DIR = REPO_ROOT / "bench"
sys.path.insert(0, str(BENCH_DIR))


class BenchmarkAdapterCommonTests(unittest.TestCase):
    def test_common_run_outcome_builds_stable_result_contract(self):
        from adapters.common import (
            RunOutcome,
            build_run_outcome,
            bytes_in_dir,
            parse_time_l_stderr,
            run_in_process,
        )

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "nested").mkdir()
            (root / "a.bin").write_bytes(b"abc")
            (root / "nested" / "b.bin").write_bytes(b"de")

            outcome = build_run_outcome(
                ok=True,
                wall_seconds=1.25,
                peak_rss_bytes=123,
                output_dir=root,
                extra_metrics={"frames": 2},
                stderr="abcdef",
                stderr_tail_chars=3,
            )

            self.assertIsInstance(outcome, RunOutcome)
            self.assertTrue(outcome.ok)
            self.assertEqual(outcome.wall_seconds, 1.25)
            self.assertEqual(outcome.peak_rss_bytes, 123)
            self.assertEqual(outcome.output_dir, str(root))
            self.assertEqual(outcome.output_size_bytes, 5)
            self.assertEqual(outcome.extra_metrics, {"frames": 2})
            self.assertEqual(outcome.stderr_tail, "def")
            self.assertIsNone(outcome.error)
            self.assertEqual(bytes_in_dir(root), 5)

        self.assertEqual(
            parse_time_l_stderr(" 456 maximum resident set size\n"),
            456,
        )
        self.assertIsNone(parse_time_l_stderr("not rss"))

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "out.bin").write_bytes(b"x")
            outcome = run_in_process(
                output_dir=root,
                action=lambda: {"frames": 1},
            )
            self.assertTrue(outcome.ok)
            self.assertGreaterEqual(outcome.wall_seconds, 0.0)
            self.assertEqual(outcome.output_size_bytes, 1)
            self.assertEqual(outcome.extra_metrics, {"frames": 1})
            self.assertIsNone(outcome.error)

    def test_adapter_modules_share_common_run_outcome_contract(self):
        common = importlib.import_module("adapters.common")

        for name in [
            "adapters.wsi_dicom",
            "adapters.wsidicomizer_adapter",
            "adapters.highdicom_adapter",
        ]:
            with self.subTest(name=name):
                module = importlib.import_module(name)

                self.assertIs(module.RunOutcome, common.RunOutcome)
                self.assertTrue(callable(module.run))


if __name__ == "__main__":
    unittest.main()

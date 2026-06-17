import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
BENCHMARK_PATH = REPO_ROOT / "bench" / "gdc_benchmark.py"


def load_benchmark_module():
    spec = importlib.util.spec_from_file_location("gdc_benchmark", BENCHMARK_PATH)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class GdcBenchmarkTests(unittest.TestCase):
    def test_discovers_gdc_slides_and_maps_manifest_by_file_id(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            download = root / "gdc_download_20260222_001131.215561"
            slide_dir = download / "e2170a4b-c9d6-4f9d-b95c-0be2ecf42196"
            slide_dir.mkdir(parents=True)
            (download / "MANIFEST.txt").write_text(
                "\t".join(["id", "filename", "md5", "size", "state"])
                + "\n"
                + "\t".join(
                    [
                        "e2170a4b-c9d6-4f9d-b95c-0be2ecf42196",
                        "e2170a4b-c9d6-4f9d-b95c-0be2ecf42196/TCGA-WE-A8ZR.svs",
                        "abc123",
                        "3",
                        "validated",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            (slide_dir / "renamed melanoma.svs").write_bytes(b"svs")
            (slide_dir / "renamed melanoma.svs.svcache").write_bytes(b"ignore")

            slides = bench.discover_gdc_slides(root)

        self.assertEqual(len(slides), 1)
        self.assertEqual(slides[0].display_name, "TCGA-WE-A8ZR.svs")
        self.assertEqual(slides[0].gdc_file_id, "e2170a4b-c9d6-4f9d-b95c-0be2ecf42196")
        self.assertEqual(slides[0].manifest_md5, "abc123")
        self.assertEqual(slides[0].manifest_size, 3)

    def test_wsi_dicom_cpu_command_uses_no_device_decode(self):
        bench = load_benchmark_module()

        command = bench.command_for_tool(
            "wsi-dicom-cpu",
            wsi_dicom_command=["target/release/wsi-dicom"],
            wsidicomizer_command=["wsidicomizer"],
            source=Path("slide.svs"),
            output_dir=Path("out"),
            profile="htj2k-lossless-rpcl",
            scope="base",
            tile_size=512,
            jpeg_quality=80,
            workers=8,
            offset_table="extended",
            device_source_decode=True,
        )

        self.assertEqual(
            command,
            [
                "target/release/wsi-dicom",
                "convert",
                "slide.svs",
                "--out",
                "out",
                "--research-placeholder",
                "--tile-size",
                "512",
                "--backend",
                "cpu",
                "--json",
                "--level",
                "0",
                "--transfer-syntax",
                "htj2k-lossless-rpcl",
            ],
        )

    def test_wsi_dicom_device_command_can_request_source_device_decode(self):
        bench = load_benchmark_module()

        command = bench.command_for_tool(
            "wsi-dicom-device",
            wsi_dicom_command=["target/release/wsi-dicom"],
            wsidicomizer_command=["wsidicomizer"],
            source=Path("slide.svs"),
            output_dir=Path("out"),
            profile="jpeg-baseline",
            scope="pyramid",
            tile_size=256,
            jpeg_quality=80,
            workers=8,
            offset_table="extended",
            device_source_decode=True,
        )

        self.assertEqual(command[-4:], ["fast-jpeg", "--jpeg-quality", "80", "--source-device-decode"])
        self.assertIn("require-device", command)
        self.assertNotIn("--level", command)

    def test_wsi_dicom_profile_command_supports_device_preflight(self):
        bench = load_benchmark_module()

        command = bench.build_wsi_dicom_profile_command(
            ["target/release/wsi-dicom"],
            Path("slide.svs"),
            profile="htj2k-lossless-rpcl",
            scope="base",
            tile_size=512,
            jpeg_quality=80,
            backend="require-device",
            source_device_decode=True,
            max_frames=64,
        )

        self.assertEqual(
            command,
            [
                "target/release/wsi-dicom",
                "profile",
                "slide.svs",
                "--backend",
                "require-device",
                "--tile-size",
                "512",
                "--jpeg-quality",
                "80",
                "--max-frames",
                "64",
                "--json",
                "--level",
                "0",
                "--transfer-syntax",
                "htj2k-lossless-rpcl",
                "--source-device-decode",
            ],
        )

    def test_wsidicomizer_command_uses_matching_public_profile(self):
        bench = load_benchmark_module()

        command = bench.command_for_tool(
            "wsidicomizer",
            wsi_dicom_command=["target/release/wsi-dicom"],
            wsidicomizer_command=["./.venv/bin/wsidicomizer"],
            source=Path("slide.svs"),
            output_dir=Path("out"),
            profile="htj2k-lossless-rpcl",
            scope="base",
            tile_size=512,
            jpeg_quality=80,
            workers=12,
            offset_table="extended",
            device_source_decode=True,
        )

        self.assertEqual(
            command,
            [
                "./.venv/bin/wsidicomizer",
                "--input",
                "slide.svs",
                "--output",
                "out",
                "--tile-size",
                "512",
                "--workers",
                "12",
                "--offset-table",
                "extended",
                "--no-confidential",
                "--levels",
                "0",
                "--format",
                "htjpeg2000",
            ],
        )

    def test_resume_keys_include_profile_and_scope(self):
        bench = load_benchmark_module()

        keys = bench.completed_result_keys(
            [
                {
                    "slide": "tcga-a",
                    "tool": "wsi-dicom-cpu",
                    "profile": "htj2k-lossless-rpcl",
                    "scope": "base",
                    "run_index": 2,
                }
            ]
        )

        self.assertEqual(
            keys,
            {("tcga-a", "wsi-dicom-cpu", "htj2k-lossless-rpcl", "base", 2)},
        )

    def test_markdown_summary_reports_device_speedups(self):
        bench = load_benchmark_module()
        rows = [
            {
                "slide": "tcga-a",
                "tool": "wsi-dicom-cpu",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "passed",
                "elapsed_secs": 10.0,
            },
            {
                "slide": "tcga-a",
                "tool": "wsi-dicom-device",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "passed",
                "elapsed_secs": 2.0,
            },
            {
                "slide": "tcga-a",
                "tool": "wsidicomizer",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "passed",
                "elapsed_secs": 12.0,
            },
        ]

        markdown = bench.render_markdown_summary(rows, title="GDC")

        self.assertIn("wsi-dicom Device status", markdown)
        self.assertIn(
            "| tcga-a | htj2k-lossless-rpcl | base | passed | 10.000 | passed | 2.000 | passed | 12.000 | 5.00x | 6.00x |",
            markdown,
        )

    def test_markdown_summary_keeps_failure_only_rows(self):
        bench = load_benchmark_module()
        rows = [
            {
                "slide": "tcga-a",
                "tool": "wsi-dicom-device",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "timeout",
                "elapsed_secs": 180.0,
            }
        ]

        markdown = bench.render_markdown_summary(rows, title="GDC")

        self.assertIn(
            "| tcga-a | htj2k-lossless-rpcl | base | timeout |  |",
            markdown,
        )

    def test_markdown_summary_keeps_metal_and_cuda_device_rows_separate(self):
        bench = load_benchmark_module()
        rows = [
            {
                "slide": "tcga-a",
                "tool": "wsi-dicom-cpu",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "passed",
                "elapsed_secs": 10.0,
            },
            {
                "slide": "tcga-a",
                "tool": "wsi-dicom-device",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "timeout",
                "result_set": "gdc-local-metal-device-smoke",
            },
            {
                "slide": "tcga-a",
                "tool": "wsi-dicom-device",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "failed",
                "result_set": "gdc-cuda-device-smoke",
            },
            {
                "slide": "tcga-a",
                "tool": "wsidicomizer",
                "profile": "htj2k-lossless-rpcl",
                "scope": "base",
                "status": "passed",
                "elapsed_secs": 12.0,
            },
        ]

        markdown = bench.render_markdown_summary(rows, title="GDC")

        self.assertIn("wsi-dicom Metal status", markdown)
        self.assertIn("wsi-dicom CUDA status", markdown)
        self.assertIn(
            "| tcga-a | htj2k-lossless-rpcl | base | passed | 10.000 | timeout |  | failed |  | passed | 12.000 |",
            markdown,
        )

    def test_trial_creates_output_parent_only(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output_dir = root / "nested" / "tool-output"
            artifact_dir = root / "artifacts"
            source = root / "slide.svs"
            source.write_bytes(b"svs")
            slide = bench.Slide(
                slide_id="tcga-a",
                display_name="TCGA-A.svs",
                path=source,
                download_dir=root,
                relative_path="slide.svs",
                gdc_file_id="file-id",
                manifest_filename="file-id/TCGA-A.svs",
                manifest_md5="abc",
                manifest_size=3,
                manifest_state="validated",
                bytes_on_disk=3,
            )
            command = [
                sys.executable,
                "-c",
                (
                    "from pathlib import Path; import sys; "
                    "out=Path(sys.argv[1]); "
                    "assert out.parent.exists(); "
                    "assert not out.exists(); "
                    "out.mkdir(); "
                    "(out / 'ok.bin').write_bytes(b'ok')"
                ),
                str(output_dir),
            ]

            row = bench.benchmark_trial(
                slide=slide,
                tool="fake-tool",
                command=command,
                output_dir=output_dir,
                artifact_dir=artifact_dir,
                cwd=root,
                timeout_secs=10,
                run_index=1,
                profile="htj2k-lossless-rpcl",
                scope="base",
                validate=False,
                wsi_dicom_command=["wsi-dicom"],
            )

        self.assertEqual(row["status"], "passed")
        self.assertEqual(row["produced_files"], 1)

    def test_trial_fails_when_strict_validation_fails(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output_dir = root / "nested" / "tool-output"
            artifact_dir = root / "artifacts"
            source = root / "slide.svs"
            source.write_bytes(b"svs")
            fake_wsi_dicom = root / "fake_wsi_dicom.py"
            fake_wsi_dicom.write_text(
                "import json\n"
                "import sys\n"
                "if sys.argv[1] != 'validate':\n"
                "    raise SystemExit(2)\n"
                "if '--strict' not in sys.argv:\n"
                "    raise SystemExit('missing --strict')\n"
                "print(json.dumps({'checks': [{'name': 'pixel-htj2k', 'status': 'failed'}]}))\n"
                "sys.stderr.write('1 validation check(s) failed\\n')\n"
                "raise SystemExit(1)\n",
                encoding="utf-8",
            )
            slide = bench.Slide(
                slide_id="tcga-a",
                display_name="TCGA-A.svs",
                path=source,
                download_dir=root,
                relative_path="slide.svs",
                gdc_file_id="file-id",
                manifest_filename="file-id/TCGA-A.svs",
                manifest_md5="abc",
                manifest_size=3,
                manifest_state="validated",
                bytes_on_disk=3,
            )
            command = [
                sys.executable,
                "-c",
                (
                    "from pathlib import Path; import sys; "
                    "out=Path(sys.argv[1]); "
                    "out.mkdir(parents=True); "
                    "(out / 'ok.bin').write_bytes(b'ok')"
                ),
                str(output_dir),
            ]

            row = bench.benchmark_trial(
                slide=slide,
                tool="fake-tool",
                command=command,
                output_dir=output_dir,
                artifact_dir=artifact_dir,
                cwd=root,
                timeout_secs=10,
                run_index=1,
                profile="htj2k-lossless-rpcl",
                scope="base",
                validate=True,
                wsi_dicom_command=[sys.executable, str(fake_wsi_dicom)],
            )

        self.assertEqual(row["status"], "failed")
        self.assertEqual(row["validation"]["status"], "failed")
        self.assertIn("--strict", row["validation"]["command"])
        self.assertEqual(row["error"], "validation failed: 1 validation check(s) failed")

    def test_device_preflight_fails_when_device_is_slower_than_cpu(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cpu_stdout = root / "cpu.json"
            device_stdout = root / "device.json"
            cpu_stdout.write_text(
                '{"metrics": {"total_frames": 64, "gpu_encode_frames": 0}}\n',
                encoding="utf-8",
            )
            device_stdout.write_text(
                '{"metrics": {"total_frames": 64, "gpu_encode_frames": 64}}\n',
                encoding="utf-8",
            )

            preflight = bench.evaluate_device_preflight(
                cpu_result={
                    "status": "passed",
                    "returncode": 0,
                    "elapsed_secs": 0.1,
                    "stdout_path": str(cpu_stdout),
                    "stderr_path": str(root / "cpu.stderr.txt"),
                },
                device_result={
                    "status": "passed",
                    "returncode": 0,
                    "elapsed_secs": 26.0,
                    "stdout_path": str(device_stdout),
                    "stderr_path": str(root / "device.stderr.txt"),
                },
                min_speedup=1.0,
                min_device_frame_pct=100.0,
            )

        self.assertEqual(preflight["status"], "failed")
        self.assertIn("device preflight speedup", preflight["reason"])
        self.assertAlmostEqual(preflight["device_frame_pct"], 100.0)

    def test_device_preflight_fails_when_device_frames_are_missing(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cpu_stdout = root / "cpu.json"
            device_stdout = root / "device.json"
            cpu_stdout.write_text(
                '{"metrics": {"total_frames": 10, "gpu_encode_frames": 0}}\n',
                encoding="utf-8",
            )
            device_stdout.write_text(
                '{"metrics": {"total_frames": 10, "gpu_encode_frames": 9}}\n',
                encoding="utf-8",
            )

            preflight = bench.evaluate_device_preflight(
                cpu_result={
                    "status": "passed",
                    "returncode": 0,
                    "elapsed_secs": 10.0,
                    "stdout_path": str(cpu_stdout),
                    "stderr_path": str(root / "cpu.stderr.txt"),
                },
                device_result={
                    "status": "passed",
                    "returncode": 0,
                    "elapsed_secs": 1.0,
                    "stdout_path": str(device_stdout),
                    "stderr_path": str(root / "device.stderr.txt"),
                },
                min_speedup=1.0,
                min_device_frame_pct=100.0,
            )

        self.assertEqual(preflight["status"], "failed")
        self.assertIn("used device encode", preflight["reason"])

    def test_device_preflight_failure_includes_device_stderr(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cpu_stdout = root / "cpu.json"
            device_stdout = root / "device.json"
            device_stderr = root / "device.stderr.txt"
            cpu_stdout.write_text(
                '{"metrics": {"total_frames": 1, "gpu_encode_frames": 0}}\n',
                encoding="utf-8",
            )
            device_stdout.write_text("", encoding="utf-8")
            device_stderr.write_text(
                "unsupported export request: backend unavailable\n",
                encoding="utf-8",
            )

            preflight = bench.evaluate_device_preflight(
                cpu_result={
                    "status": "passed",
                    "returncode": 0,
                    "elapsed_secs": 0.1,
                    "stdout_path": str(cpu_stdout),
                    "stderr_path": str(root / "cpu.stderr.txt"),
                },
                device_result={
                    "status": "failed",
                    "returncode": 1,
                    "elapsed_secs": 0.1,
                    "stdout_path": str(device_stdout),
                    "stderr_path": str(device_stderr),
                },
                min_speedup=1.0,
                min_device_frame_pct=100.0,
            )

        self.assertEqual(preflight["status"], "failed")
        self.assertIn("backend unavailable", preflight["reason"])

    def test_preflight_failure_row_uses_distinct_status(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "slide.svs"
            source.write_bytes(b"svs")
            slide = bench.Slide(
                slide_id="tcga-a",
                display_name="TCGA-A.svs",
                path=source,
                download_dir=root,
                relative_path="slide.svs",
                gdc_file_id="file-id",
                manifest_filename="file-id/TCGA-A.svs",
                manifest_md5="abc",
                manifest_size=3,
                manifest_state="validated",
                bytes_on_disk=3,
            )

            row = bench.preflight_failure_row(
                slide=slide,
                tool="wsi-dicom-device",
                command=["wsi-dicom", "convert"],
                output_dir=root / "out",
                profile="htj2k-lossless-rpcl",
                scope="base",
                run_index=1,
                preflight={
                    "status": "failed",
                    "reason": "device slower than CPU",
                    "device": {
                        "stdout_path": str(root / "device.stdout.json"),
                        "stderr_path": str(root / "device.stderr.txt"),
                    },
                },
                system_label="macos-metal",
            )

        self.assertEqual(row["status"], "preflight-failed")
        self.assertEqual(row["error"], "device slower than CPU")
        self.assertEqual(row["result_label"], "wsi-dicom Metal")
        self.assertTrue(row["stdout_path"].endswith("device.stdout.json"))
        self.assertTrue(row["stderr_path"].endswith("device.stderr.txt"))

    def test_rows_from_result_dirs_merges_jsonl_rows(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            first = root / "first"
            second = root / "second"
            first.mkdir()
            second.mkdir()
            (first / "results.jsonl").write_text('{"slide": "a"}\n', encoding="utf-8")
            (second / "results.jsonl").write_text('{"slide": "b"}\n', encoding="utf-8")

            rows = bench.rows_from_result_dirs([first, second])

        self.assertEqual(rows, [{"slide": "a"}, {"slide": "b"}])

    def test_rows_from_result_dirs_can_annotate_merged_rows(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            result_dir = root / "gdc-cuda-device-smoke"
            result_dir.mkdir()
            (result_dir / "results.jsonl").write_text(
                '{"slide": "a", "tool": "wsi-dicom-device"}\n',
                encoding="utf-8",
            )

            rows = bench.rows_from_result_dirs([result_dir], annotate=True)

        self.assertEqual(rows[0]["result_set"], "gdc-cuda-device-smoke")
        self.assertEqual(rows[0]["result_label"], "wsi-dicom CUDA")


if __name__ == "__main__":
    unittest.main()

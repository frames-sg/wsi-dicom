import importlib.util
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "benchmark_wsidicomizer.py"


def load_benchmark_module():
    spec = importlib.util.spec_from_file_location("benchmark_wsidicomizer", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class BenchmarkHarnessTests(unittest.TestCase):
    def test_discovers_supported_slides_in_stable_order(self):
        bench = load_benchmark_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Aperio").mkdir()
            (root / "Hamamatsu").mkdir()
            (root / "Aperio" / "CMU-1.svs").write_bytes(b"svs")
            (root / "Hamamatsu" / "CMU-1.ndpi").write_bytes(b"ndpi")
            (root / "Hamamatsu" / "notes.txt").write_text("ignore")

            slides = bench.discover_slides(root)

        self.assertEqual(
            [slide.relative_path for slide in slides],
            ["Aperio/CMU-1.svs", "Hamamatsu/CMU-1.ndpi"],
        )

    def test_wsi_dicom_command_uses_fast_jpeg_preset(self):
        bench = load_benchmark_module()
        command = bench.build_wsi_dicom_command(
            ["target/release/wsi-dicom"],
            Path("slide.svs"),
            Path("out/wsi-dicom"),
            tile_size=256,
            level=0,
            profile="fast-jpeg",
        )

        self.assertEqual(
            command,
            [
                "target/release/wsi-dicom",
                "convert",
                "slide.svs",
                "--out",
                "out/wsi-dicom",
                "--research-placeholder",
                "--tile-size",
                "256",
                "--level",
                "0",
                "--json",
                "--preset",
                "fast-jpeg",
                "--jpeg-quality",
                "80",
            ],
        )

    def test_wsidicomizer_command_uses_matching_fast_jpeg_settings(self):
        bench = load_benchmark_module()
        command = bench.build_wsidicomizer_command(
            ["./.venv/bin/wsidicomizer"],
            Path("slide.svs"),
            Path("out/wsidicomizer"),
            tile_size=256,
            level=0,
            workers=8,
            offset_table="extended",
            profile="fast-jpeg",
        )

        self.assertEqual(
            command,
            [
                "./.venv/bin/wsidicomizer",
                "--input",
                "slide.svs",
                "--output",
                "out/wsidicomizer",
                "--tile-size",
                "256",
                "--levels",
                "0",
                "--workers",
                "8",
                "--offset-table",
                "extended",
                "--no-confidential",
                "--format",
                "jpeg",
                "--quality",
                "80",
            ],
        )

    def test_markdown_summary_reports_speed_ratio_and_equal_target(self):
        bench = load_benchmark_module()
        rows = [
            {
                "slide": "Aperio/CMU-1.svs",
                "tool": "wsi-dicom",
                "status": "passed",
                "elapsed_secs": 2.0,
                "output_bytes": 100,
                "produced_files": 1,
                "dicom_outputs": [
                    {
                        "transfer_syntax_uid": "1.2.840.10008.1.2.4.50",
                        "rows": 512,
                        "columns": 512,
                        "number_of_frames": 4,
                        "total_pixel_matrix_columns": 1024,
                        "total_pixel_matrix_rows": 1024,
                    }
                ],
            },
            {
                "slide": "Aperio/CMU-1.svs",
                "tool": "wsidicomizer",
                "status": "passed",
                "elapsed_secs": 5.0,
                "output_bytes": 120,
                "produced_files": 1,
                "dicom_outputs": [
                    {
                        "transfer_syntax_uid": "1.2.840.10008.1.2.4.50",
                        "rows": 512,
                        "columns": 512,
                        "number_of_frames": 4,
                        "total_pixel_matrix_columns": 1024,
                        "total_pixel_matrix_rows": 1024,
                    }
                ],
            },
        ]

        markdown = bench.render_markdown_summary(rows, title="Smoke")

        self.assertIn(
            "| Aperio/CMU-1.svs | passed | 2.000 | passed | 5.000 | 2.50x | equal-target |",
            markdown,
        )

    def test_dicom_metadata_from_dataset_extracts_geometry(self):
        bench = load_benchmark_module()

        class FileMeta:
            TransferSyntaxUID = "1.2.840.10008.1.2.4.50"

        class Dataset:
            file_meta = FileMeta()
            Rows = 512
            Columns = 512
            NumberOfFrames = "12"
            TotalPixelMatrixColumns = 2048
            TotalPixelMatrixRows = 1536

        self.assertEqual(
            bench.dicom_metadata_from_dataset(Path("level.dcm"), Dataset()),
            {
                "file": "level.dcm",
                "transfer_syntax_uid": "1.2.840.10008.1.2.4.50",
                "rows": 512,
                "columns": 512,
                "number_of_frames": 12,
                "total_pixel_matrix_columns": 2048,
                "total_pixel_matrix_rows": 1536,
            },
        )

    def test_target_comparison_detects_mixed_geometry(self):
        bench = load_benchmark_module()
        wsi = {
            "status": "passed",
            "dicom_outputs": [
                {
                    "transfer_syntax_uid": "1.2.840.10008.1.2.4.202",
                    "rows": 256,
                    "columns": 256,
                    "number_of_frames": 16,
                    "total_pixel_matrix_columns": 1024,
                    "total_pixel_matrix_rows": 1024,
                }
            ],
        }
        wsidicomizer = {
            "status": "passed",
            "dicom_outputs": [
                {
                    "transfer_syntax_uid": "1.2.840.10008.1.2.4.50",
                    "rows": 512,
                    "columns": 512,
                    "number_of_frames": 4,
                    "total_pixel_matrix_columns": 1024,
                    "total_pixel_matrix_rows": 1024,
                }
            ],
        }

        self.assertEqual(bench.target_comparison(wsi, wsidicomizer), "mixed-target")

    def test_parse_cargo_pkgid_version_extracts_package_version(self):
        bench = load_benchmark_module()

        named_version = bench.parse_cargo_pkgid_version(
            "path+file:///repo/wsi-dicom#wsi-dicom@0.3.0"
        )
        local_version = bench.parse_cargo_pkgid_version(
            "path+file:///repo/wsi-dicom#0.3.0"
        )

        self.assertEqual(named_version, "0.3.0")
        self.assertEqual(local_version, "0.3.0")

    def test_resume_helpers_skip_completed_pairs_and_pick_available_path(self):
        bench = load_benchmark_module()
        rows = [{"slide": "Aperio/CMU-1.svs", "tool": "wsi-dicom", "run_index": 1}]

        keys = bench.completed_result_keys(rows)

        self.assertIn(("Aperio/CMU-1.svs", "wsi-dicom", 1), keys)
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp) / "output"
            base.mkdir()
            resumed = bench.next_available_path(base)

        self.assertEqual(resumed.name, "output-resume-1")


if __name__ == "__main__":
    unittest.main()

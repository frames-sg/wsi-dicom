import argparse
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


def write_htj2k_dicom(path):
    from pydicom.dataset import FileDataset, FileMetaDataset
    from pydicom.encaps import encapsulate

    file_meta = FileMetaDataset()
    file_meta.TransferSyntaxUID = "1.2.840.10008.1.2.4.201"
    file_meta.MediaStorageSOPClassUID = "1.2.840.10008.5.1.4.1.1.77.1.6"
    file_meta.MediaStorageSOPInstanceUID = "1.2.826.0.1.3680043.10.543.1"
    dataset = FileDataset(path, {}, file_meta=file_meta, preamble=b"\0" * 128)
    dataset.SOPClassUID = file_meta.MediaStorageSOPClassUID
    dataset.SOPInstanceUID = file_meta.MediaStorageSOPInstanceUID
    dataset.NumberOfFrames = 1
    dataset.PixelData = encapsulate([b"codestream"])
    dataset.save_as(path, enforce_file_format=True)


class FormatCoverageComparisonTests(unittest.TestCase):
    def test_cli_reports_preflight_failures_without_a_traceback(self):
        from bench.format_coverage import compare

        stderr = io.StringIO()
        with tempfile.TemporaryDirectory() as temporary, contextlib.redirect_stderr(stderr):
            root = Path(temporary)
            result = compare.main(
                [
                    "--cpu",
                    str(root / "cpu"),
                    "--metal",
                    str(root / "metal"),
                    "--output",
                    str(root / "output"),
                    "--htj2k-decoder",
                    str(root / "missing-decoder"),
                ]
            )

        self.assertEqual(result, 2)
        self.assertIn("decoder does not exist", stderr.getvalue())

    def test_bundle_loader_rejects_malformed_duplicate_and_unsafe_cases(self):
        from bench.format_coverage import compare
        from bench.format_coverage.report import write_checksums

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            report_path = root / "format-coverage-report.json"
            documents = (
                ([], "object"),
                (
                    {"status": "passed", "policy": {"backend": "cpu"}, "cases": [None]},
                    "case 0",
                ),
                (
                    {
                        "status": "passed",
                        "policy": {"backend": "cpu"},
                        "cases": [{"id": "same"}, {"id": "same"}],
                    },
                    "duplicate",
                ),
                (
                    {
                        "status": "passed",
                        "policy": {"backend": "cpu"},
                        "cases": [{"id": r"..\outside"}],
                    },
                    "safe identifier",
                ),
            )
            for document, message in documents:
                with self.subTest(message=message):
                    report_path.write_text(json.dumps(document), encoding="utf-8")
                    write_checksums(root)
                    with self.assertRaisesRegex(compare.FormatCoverageError, message):
                        compare.load_bundle(root, "cpu")

    def test_bundle_loader_rejects_tampering_and_missing_counterparts(self):
        from bench.format_coverage import compare
        from bench.format_coverage.report import write_checksums

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "format-coverage-report.json").write_text(
                json.dumps(
                    {
                        "status": "passed",
                        "policy": {"backend": "cpu"},
                        "cases": [{"id": "case"}],
                    }
                )
            )
            payload = root / "payload.txt"
            payload.write_text("original")
            write_checksums(root)
            payload.write_text("tampered")
            with self.assertRaisesRegex(compare.FormatCoverageError, "checksum mismatch"):
                compare.load_bundle(root, "cpu")

        with self.assertRaisesRegex(compare.FormatCoverageError, "different case IDs"):
            compare.matched_metal_case_ids(
                {"cpu-only": {}},
                {
                    "metal-only": {
                        "conversion": {"route_classification": "metal_encode"}
                    }
                },
            )

    def test_comparison_failure_removes_staging_pixels(self):
        from bench.format_coverage import compare

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            decoder = root / "decoder"
            decoder.write_bytes(b"decoder")
            args = argparse.Namespace(
                cpu=root / "cpu",
                metal=root / "metal",
                output=root / "comparison",
                htj2k_decoder=decoder,
                decode_timeout_secs=1,
            )
            cpu_case = {"conversion": {"route_classification": "cpu_encode"}}
            metal_case = {"conversion": {"route_classification": "metal_encode"}}
            with mock.patch.object(
                compare,
                "load_bundle",
                side_effect=[({}, {"case": cpu_case}), ({}, {"case": metal_case})],
            ), mock.patch.object(
                compare,
                "case_comparison",
                side_effect=compare.FormatCoverageError("decode failed"),
            ), self.assertRaisesRegex(compare.FormatCoverageError, "decode failed"):
                compare.run(args)

            self.assertFalse(args.output.exists())
            self.assertEqual(list(root.glob(".comparison.staging-*")), [])

    def test_htj2k_decode_timeout_uses_the_shared_process_owner(self):
        from bench.format_coverage import compare

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dicom_path = root / "input.dcm"
            write_htj2k_dicom(dicom_path)

            with mock.patch.object(
                compare,
                "run_bounded_command",
                return_value={
                    "returncode": None,
                    "timed_out": True,
                    "launch_error": None,
                    "elapsed_seconds": 1.0,
                },
            ):
                with self.assertRaisesRegex(compare.FormatCoverageError, "timed out"):
                    compare.decoded_instance_digest(
                        dicom_path, root / "decoder", root / "decoded", 1
                    )

    def test_htj2k_decode_rejects_truncated_process_evidence(self):
        from bench.format_coverage import compare

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dicom_path = root / "input.dcm"
            write_htj2k_dicom(dicom_path)

            def execute(command, *, stdout_path, stderr_path, **_kwargs):
                Path(command[4]).write_bytes(b"P5\n1 1\n255\n\x00")
                stdout_path.write_text("x" * 65536, encoding="utf-8")
                stderr_path.write_text("", encoding="utf-8")
                return {
                    "returncode": 0,
                    "timed_out": False,
                    "launch_error": None,
                    "stdout_truncated": True,
                    "stderr_truncated": False,
                }

            with mock.patch.object(compare, "run_bounded_command", side_effect=execute):
                with self.assertRaisesRegex(compare.FormatCoverageError, "output was truncated"):
                    compare.decoded_instance_digest(
                        dicom_path, root / "decoder", root / "decoded", 1
                    )

    def test_semantic_digest_ignores_generated_uids_but_detects_geometry(self):
        from pydicom.dataset import Dataset
        from pydicom.sequence import Sequence

        from bench.format_coverage.compare import semantic_digest

        def dataset(uid: str, rows: int) -> Dataset:
            value = Dataset()
            value.SOPInstanceUID = uid
            value.Rows = rows
            value.Columns = 512
            value.PhotometricInterpretation = "MONOCHROME2"
            item = Dataset()
            item.PixelSpacing = ["0.0005", "0.00025"]
            value.SharedFunctionalGroupsSequence = Sequence([item])
            value.PixelData = b"not-semantic"
            value.ExtendedOffsetTable = b"transport offsets"
            value.ExtendedOffsetTableLengths = b"transport lengths"
            return value

        self.assertEqual(
            semantic_digest([dataset("1.2.3", 512)]),
            semantic_digest([dataset("1.2.4", 512)]),
        )
        transport_variant = dataset("1.2.3", 512)
        transport_variant.ExtendedOffsetTable = b"different offsets"
        transport_variant.ExtendedOffsetTableLengths = b"different lengths"
        self.assertEqual(
            semantic_digest([dataset("1.2.3", 512)]),
            semantic_digest([transport_variant]),
        )
        self.assertNotEqual(
            semantic_digest([dataset("1.2.3", 512)]),
            semantic_digest([dataset("1.2.3", 256)]),
        )

    def test_parse_pnm_canonicalizes_comments_and_16_bit_payloads(self):
        from bench.format_coverage.compare import parse_pnm

        left = b"P5\n# decoder comment\n2 1\n65535\n\x00\x01\xff\xff"
        right = b"P5 2 1 65535\n\x00\x01\xff\xff"

        self.assertEqual(parse_pnm(left), parse_pnm(right))
        self.assertEqual(parse_pnm(left)[0:3], ("P5", 2, 1))

    def test_performance_comparison_reports_direction_without_claiming_speedup(self):
        from bench.format_coverage.compare import performance_comparison

        comparison = performance_comparison(
            cpu_wall=0.25,
            metal_wall=1.0,
            cpu_peak_rss=20,
            metal_peak_rss=40,
            frames=10,
        )

        self.assertEqual(comparison["faster_backend"], "cpu")
        self.assertEqual(comparison["lower_peak_memory_backend"], "cpu")
        self.assertEqual(comparison["cpu_throughput_frames_per_second"], 40.0)
        self.assertEqual(comparison["metal_throughput_frames_per_second"], 10.0)


if __name__ == "__main__":
    unittest.main()

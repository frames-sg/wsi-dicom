import hashlib
import json
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


class FormatCoverageManifestTests(unittest.TestCase):
    def test_v2_manifest_requires_rule_catalog_provenance(self):
        from bench.format_coverage.manifest import FormatCoverageError, load_manifest

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.json"
            manifest = {
                "schema_version": "wsi-dicom-format-coverage-v2",
                "cases": [self.case("case", "slide.scn")],
            }
            path.write_text(json.dumps(manifest), encoding="utf-8")

            with self.assertRaisesRegex(FormatCoverageError, "rule_catalog"):
                load_manifest(path)

            manifest["rule_catalog"] = {
                "version": "wsi-dicom-bench-rules-2026c-v2",
                "path": "rules/wsi-dicom-bench-rules-2026c-v2.json",
                "sha256": "f" * 64,
            }
            path.write_text(json.dumps(manifest), encoding="utf-8")
            self.assertEqual(
                load_manifest(path)["rule_catalog"], manifest["rule_catalog"]
            )

    def test_rule_catalog_provenance_must_match_configured_catalog(self):
        from bench.format_coverage.manifest import (
            FormatCoverageError,
            validate_rule_catalog_provenance,
        )

        with tempfile.TemporaryDirectory() as tmp:
            catalog_path = Path(tmp) / "catalog.json"
            catalog_path.write_text(
                json.dumps({"catalog_version": "catalog-v2"}), encoding="utf-8"
            )
            manifest = {
                "rule_catalog": {
                    "version": "catalog-v2",
                    "path": "rules/catalog.json",
                    "sha256": hashlib.sha256(catalog_path.read_bytes()).hexdigest(),
                }
            }

            self.assertEqual(
                validate_rule_catalog_provenance(manifest, catalog_path),
                manifest["rule_catalog"],
            )
            manifest["rule_catalog"]["sha256"] = "0" * 64
            with self.assertRaisesRegex(FormatCoverageError, "SHA-256"):
                validate_rule_catalog_provenance(manifest, catalog_path)

    def test_installed_catalog_path_uses_the_benchmark_package_resource(self):
        from bench.format_coverage.manifest import installed_catalog_path

        resource = mock.Mock()
        resource.joinpath.return_value = Path(__file__)
        with mock.patch("importlib.resources.files", return_value=resource):
            self.assertEqual(installed_catalog_path(), Path(__file__))
        resource.joinpath.assert_called_once_with(
            "rules/wsi-dicom-bench-rules-2026c-v2.json"
        )

    def test_load_manifest_rejects_duplicate_case_ids_and_unsafe_paths(self):
        from bench.format_coverage.manifest import FormatCoverageError, load_manifest

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = {
                "schema_version": "wsi-dicom-format-coverage-v1",
                "cases": [
                    self.case("duplicate", "slide.scn"),
                    self.case("duplicate", "../outside.scn"),
                ],
            }
            path = root / "manifest.json"
            path.write_text("[]", encoding="utf-8")
            with self.assertRaisesRegex(FormatCoverageError, "object"):
                load_manifest(path)

            path.write_text(json.dumps(manifest), encoding="utf-8")

            with self.assertRaisesRegex(FormatCoverageError, "duplicate case id"):
                load_manifest(path)

            manifest["cases"][1]["id"] = "unsafe"
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(FormatCoverageError, "relative path"):
                load_manifest(path)

            manifest["cases"][1] = self.case("../../outside", "slide.scn")
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(FormatCoverageError, "safe identifier"):
                load_manifest(path)

            manifest["cases"][1] = self.case("windows", r"..\outside.scn")
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(FormatCoverageError, "relative path"):
                load_manifest(path)

    def test_materialize_source_verifies_archive_and_rejects_traversal(self):
        from bench.format_coverage.manifest import FormatCoverageError
        from bench.format_coverage.source import materialize_source

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            corpus = root / "corpus"
            corpus.mkdir()
            archive = corpus / "slide.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("slide/slide.vsi", b"entry")
                bundle.writestr("slide/frame.ets", b"pixels")
            case = self.case("vsi", "slide.zip")
            case.update(
                {
                    "archive_entry": "slide/slide.vsi",
                    "source_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                }
            )

            materialized = materialize_source(case, corpus, root / "extracted")

            self.assertEqual(materialized.entry_path.read_bytes(), b"entry")
            self.assertEqual(len(materialized.extracted_files), 2)
            self.assertEqual(materialized.container_sha256, case["source_sha256"])

            unsafe = corpus / "unsafe.zip"
            with zipfile.ZipFile(unsafe, "w") as bundle:
                bundle.writestr("../outside.vsi", b"unsafe")
            unsafe_case = self.case("unsafe", "unsafe.zip")
            unsafe_case.update(
                {
                    "archive_entry": "../outside.vsi",
                    "source_sha256": hashlib.sha256(unsafe.read_bytes()).hexdigest(),
                }
            )
            with self.assertRaisesRegex(FormatCoverageError, "unsafe archive member"):
                materialize_source(unsafe_case, corpus, root / "unsafe-extracted")

            windows_unsafe = corpus / "windows-unsafe.zip"
            with zipfile.ZipFile(windows_unsafe, "w") as bundle:
                bundle.writestr(r"..\outside.vsi", b"unsafe")
            windows_case = self.case("windows-unsafe", "windows-unsafe.zip")
            windows_case.update(
                {
                    "archive_entry": r"..\outside.vsi",
                    "source_sha256": hashlib.sha256(
                        windows_unsafe.read_bytes()
                    ).hexdigest(),
                }
            )
            with self.assertRaisesRegex(FormatCoverageError, "unsafe archive member"):
                materialize_source(
                    windows_case, corpus, root / "windows-unsafe-extracted"
                )

    def test_conversion_command_is_explicit_and_expected_rejection_is_exact(self):
        from bench.format_coverage.conversion import (
            build_conversion_command,
            build_profile_command,
            classify_conversion,
            classify_route,
        )
        from bench.process_evidence import parse_bsd_time_metrics

        case = self.case("czi", "slide.czi")
        case.update(
            {
                "level": 0,
                "transfer_syntax": "htj2k-lossless-rpcl",
                "source_pixel_spacing_mm": [0.000346053, 0.000346056],
                "expected": {
                    "outcome": "rejected",
                    "message_contains": "requires direct subblock composition",
                },
            }
        )
        command = build_conversion_command(
            case,
            Path("/bin/wsi-dicom"),
            Path("slide.czi"),
            Path("out"),
            "require-device",
        )

        self.assertIn("--research-placeholder", command)
        self.assertIn("--codec-validation", command)
        self.assertIn("round-trip", command)
        self.assertIn("--uid-policy", command)
        self.assertIn("deterministic", command)
        self.assertEqual(command[command.index("--backend") + 1], "require-device")
        self.assertIn("--source-pixel-spacing-mm", command)
        self.assertIn("0.000346053,0.000346056", command)
        self.assertEqual(
            classify_conversion(
                case, 1, "", "requires direct subblock composition"
            ),
            "expected_rejection",
        )
        self.assertEqual(
            classify_conversion(case, 1, "", "different failure"), "failed"
        )
        self.assertEqual(classify_conversion(case, 0, "{}", ""), "failed")
        self.assertEqual(
            classify_route(
                "converted",
                {"metrics": {"total_frames": 2, "gpu_encode_frames": 2}},
                "require-device",
            ),
            "metal_encode",
        )
        self.assertEqual(
            classify_route(
                "converted",
                {"metrics": {"total_frames": 2, "jpeg_passthrough_frames": 2}},
                "require-device",
            ),
            "passthrough",
        )
        self.assertEqual(
            classify_route("expected_rejection", None, "require-device"),
            "unsupported",
        )
        self.assertEqual(
            parse_bsd_time_metrics(
                "real 1.25\nuser 0.50\nsys 0.10\n  123456 maximum resident set size\n"
            ),
            {
                "wall_seconds": 1.25,
                "user_seconds": 0.5,
                "system_seconds": 0.1,
                "peak_rss_bytes": 123456,
            },
        )
        profile = build_profile_command(
            case,
            Path("/bin/wsi-dicom"),
            Path("slide.scn"),
            "require-device",
        )
        self.assertEqual(profile[1], "profile")
        self.assertEqual(profile[profile.index("--max-frames") + 1], "1")

    def test_source_evidence_records_materialized_source_once(self):
        from bench.format_coverage.source_evidence import collect_source_evidence

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            corpus = root / "corpus"
            corpus.mkdir()
            source = corpus / "slide.scn"
            source.write_bytes(b"source")
            case = self.case("source", "slide.scn")
            case["source_sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
            case["source_pixel_spacing_mm"] = [0.00025, 0.0005]
            case["source_pixel_spacing_provenance"] = "scanner metadata"
            case_root = root / "case"
            case_root.mkdir()

            materialized, evidence = collect_source_evidence(case, corpus, case_root)

            self.assertEqual(materialized.entry_path, source)
            self.assertEqual(
                evidence,
                {
                    "format": "test",
                    "container_path": str(source),
                    "container_bytes": 6,
                    "container_sha256": case["source_sha256"],
                    "entry_path": str(source),
                    "entry_bytes": 6,
                    "entry_sha256": case["source_sha256"],
                    "supplied_source_pixel_spacing_mm": [0.00025, 0.0005],
                    "source_pixel_spacing_provenance": "scanner metadata",
                    "extracted_files": [],
                },
            )
            self.assertEqual(
                json.loads((case_root / "source.json").read_text(encoding="utf-8")),
                evidence,
            )

    def test_conversion_execution_preserves_evidence_and_classification(self):
        from bench.format_coverage.conversion import execute_conversion

        case = self.case("conversion", "slide.scn")
        case["expected_route"] = {"cpu": "cpu_encode"}
        with tempfile.TemporaryDirectory() as tmp:
            case_root = Path(tmp)

            def execute(_command, *, stdout_path, stderr_path, **_kwargs):
                stdout_path.write_text(
                    json.dumps(
                        {
                            "metrics": {
                                "total_frames": 2,
                                "cpu_input_frames": 2,
                                "input_decode_micros": 11,
                                "compose_micros": 7,
                            }
                        }
                    ),
                    encoding="utf-8",
                )
                stderr_path.write_text("", encoding="utf-8")
                return {
                    "command": list(_command),
                    "returncode": 0,
                    "timed_out": False,
                }

            with mock.patch(
                "bench.format_coverage.conversion.run_bounded_command",
                side_effect=execute,
            ):
                outcome = execute_conversion(
                    case,
                    wsi_dicom=Path("/bin/wsi-dicom"),
                    source=Path("slide.scn"),
                    case_root=case_root,
                    backend="cpu",
                    timeout_secs=30,
                )

            self.assertEqual(outcome.status, "converted")
            self.assertEqual(outcome.evaluation_mode, "convert")
            self.assertEqual(outcome.output_path, case_root / "dicom")
            self.assertEqual(outcome.evidence["route_classification"], "cpu_encode")
            self.assertEqual(
                outcome.evidence["source_stage"],
                {
                    "cpu_input_frames": 2,
                    "gpu_input_decode_frames": 0,
                    "input_decode_micros": 11,
                    "compose_micros": 7,
                    "classification": "cpu_decode_or_composition",
                },
            )

    def test_conversion_process_failures_and_truncation_fail_closed(self):
        from bench.format_coverage.conversion import execute_conversion

        case = self.case("conversion-failure", "slide.scn")
        failures = {
            "launch": {
                "returncode": None,
                "timed_out": False,
                "launch_error": "FileNotFoundError: missing",
                "stdout_truncated": False,
                "stderr_truncated": False,
            },
            "truncated": {
                "returncode": 0,
                "timed_out": False,
                "launch_error": None,
                "stdout_truncated": True,
                "stderr_truncated": False,
            },
        }
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name, evidence in failures.items():
                with self.subTest(failure=name):
                    case_root = root / name
                    case_root.mkdir()

                    def execute(_command, *, stdout_path, stderr_path, **_kwargs):
                        stdout_path.write_text('{"metrics":{"total_frames":1}}')
                        stderr_path.write_text("")
                        return {"command": list(_command), **evidence}

                    with mock.patch(
                        "bench.format_coverage.conversion.run_bounded_command",
                        side_effect=execute,
                    ):
                        outcome = execute_conversion(
                            case,
                            wsi_dicom=Path("/bin/wsi-dicom"),
                            source=Path("slide.scn"),
                            case_root=case_root,
                            backend="cpu",
                            timeout_secs=30,
                        )

                    self.assertEqual(outcome.status, "failed")
                    self.assertEqual(outcome.evidence["route_classification"], "failed")

    def test_workbench_execution_and_case_finalization_preserve_schema(self):
        from bench.format_coverage.finalization import finalize_case
        from bench.format_coverage.workbench import execute_workbench

        case = self.case("final", "slide.scn")
        case["expected_route"] = {"cpu": "cpu_encode"}
        with tempfile.TemporaryDirectory() as tmp:
            case_root = Path(tmp)
            commands = []

            def execute(command, *, stdout_path, stderr_path, **_kwargs):
                commands.append(command)
                stdout_path.write_text('{"status":"passed"}', encoding="utf-8")
                stderr_path.write_text("", encoding="utf-8")
                return {"returncode": 0, "timed_out": False}

            with mock.patch(
                "bench.format_coverage.workbench.run_bounded_command",
                side_effect=execute,
            ):
                workbench = execute_workbench(
                    conversion_output=case_root / "dicom",
                    case_root=case_root,
                    workbench_command="wsi-dicom-bench-workbench",
                    wsi_dicom=Path("target/release/wsi-dicom"),
                    catalog=Path("bench/rules/catalog.json"),
                    timeout_secs=30,
                )

            conversion = {
                "status": "converted",
                "route_classification": "cpu_encode",
            }
            result = finalize_case(
                case,
                backend="cpu",
                evaluation_mode="convert",
                source_record={"format": "test"},
                conversion=conversion,
                workbench=workbench,
                case_root=case_root,
            )

            self.assertEqual(workbench["status"], "passed")
            self.assertEqual(workbench["report"], {"status": "passed"})
            self.assertEqual(commands[0][0], "wsi-dicom-bench-workbench")
            self.assertEqual(
                list(result),
                [
                    "id",
                    "format",
                    "level",
                    "transfer_syntax",
                    "backend",
                    "evaluation_mode",
                    "expected_route",
                    "expected",
                    "status",
                    "source",
                    "conversion",
                    "workbench",
                ],
            )
            self.assertEqual(result["status"], "passed")
            self.assertEqual(
                json.loads(
                    (case_root / "case-report.json").read_text(encoding="utf-8")
                ),
                result,
            )

    def test_workbench_process_failures_and_invalid_reports_fail_closed(self):
        from bench.format_coverage.workbench import execute_workbench

        failures = {
            "launch": {
                "returncode": None,
                "timed_out": False,
                "launch_error": "FileNotFoundError: missing",
                "stdout_truncated": False,
                "stderr_truncated": False,
                "stdout": '{"status":"passed"}',
            },
            "truncated": {
                "returncode": 0,
                "timed_out": False,
                "launch_error": None,
                "stdout_truncated": True,
                "stderr_truncated": False,
                "stdout": '{"status":"passed"}',
            },
            "invalid-report": {
                "returncode": 0,
                "timed_out": False,
                "launch_error": None,
                "stdout_truncated": False,
                "stderr_truncated": False,
                "stdout": "not-json",
            },
            "empty-report": {
                "returncode": 0,
                "timed_out": False,
                "launch_error": None,
                "stdout_truncated": False,
                "stderr_truncated": False,
                "stdout": "{}",
            },
        }
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name, evidence in failures.items():
                with self.subTest(failure=name):
                    case_root = root / name
                    case_root.mkdir()

                    def execute(_command, *, stdout_path, stderr_path, **_kwargs):
                        stdout_path.write_text(evidence["stdout"])
                        stderr_path.write_text("")
                        return {
                            key: value
                            for key, value in evidence.items()
                            if key != "stdout"
                        }

                    with mock.patch(
                        "bench.format_coverage.workbench.run_bounded_command",
                        side_effect=execute,
                    ):
                        result = execute_workbench(
                            conversion_output=case_root / "dicom",
                            case_root=case_root,
                            workbench_command="wsi-dicom-bench-workbench",
                            wsi_dicom=Path("target/release/wsi-dicom"),
                            catalog=Path("rules/catalog.json"),
                            timeout_secs=30,
                        )

                    self.assertEqual(result["status"], "failed")

    def test_case_finalization_requires_a_truthful_route_without_an_expectation(self):
        from bench.format_coverage.finalization import finalize_case

        case = self.case("unclassified", "slide.scn")
        conversion = {
            "status": "converted",
            "route_classification": "failed",
        }
        with tempfile.TemporaryDirectory() as tmp:
            result = finalize_case(
                case,
                backend="cpu",
                evaluation_mode="convert",
                source_record={"format": "test"},
                conversion=conversion,
                workbench={"status": "passed"},
                case_root=Path(tmp),
            )
            self.assertEqual(result["status"], "failed")

            conversion["route_classification"] = "cpu_only_source_stage"
            result = finalize_case(
                case,
                backend="require-device",
                evaluation_mode="convert",
                source_record={"format": "test"},
                conversion=conversion,
                workbench={"status": "passed"},
                case_root=Path(tmp),
            )
            self.assertEqual(result["status"], "failed")

    def test_failed_run_removes_staging_directory(self):
        import argparse

        from bench.format_coverage import runner
        from bench.format_coverage.manifest import FormatCoverageError

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            corpus = root / "corpus"
            corpus.mkdir()
            binary = root / "wsi-dicom"
            workbench = root / "workbench.py"
            manifest = root / "manifest.json"
            catalog = root / "catalog.json"
            for path in (binary, workbench, manifest, catalog):
                path.write_text("fixture")
            workbench.chmod(0o755)
            output = root / "output"
            args = argparse.Namespace(
                corpus_root=corpus,
                output=output,
                manifest=manifest,
                catalog=catalog,
                wsi_dicom=binary,
                workbench_command=str(workbench),
                backend="cpu",
                conversion_timeout_secs=30,
                workbench_timeout_secs=30,
            )
            document = {
                "schema_version": "wsi-dicom-format-coverage-v1",
                "cases": [self.case("failure", "slide.scn")],
            }

            with mock.patch.object(runner, "load_manifest", return_value=document), mock.patch.object(
                runner, "run_case", side_effect=FormatCoverageError("expected failure")
            ):
                with self.assertRaisesRegex(FormatCoverageError, "expected failure"):
                    runner.run(args)

            self.assertFalse(output.exists())
            self.assertEqual(list(root.glob(".output.staging-*")), [])

    def test_run_finalization_preserves_top_level_report_schema(self):
        from bench.format_coverage import finalization

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = root / "manifest.json"
            manifest.write_text("{}", encoding="utf-8")
            catalog = root / "catalog.json"
            catalog.write_text("{}", encoding="utf-8")
            executable = root / "wsi-dicom"
            executable.write_bytes(b"binary")
            cases = [{"status": "passed"}]

            with mock.patch.object(
                finalization, "command_version", return_value="wsi-dicom 0.7.5"
            ):
                report = finalization.build_run_report(
                    manifest_path=manifest,
                    catalog_path=catalog,
                    catalog_provenance={
                        "version": "catalog-v2",
                        "path": "rules/catalog-v2.json",
                        "sha256": "f" * 64,
                    },
                    wsi_dicom=executable,
                    conversion_timeout_secs=60,
                    workbench_timeout_secs=90,
                    backend="cpu",
                    cases=cases,
                )

            self.assertEqual(
                list(report),
                [
                    "schema_version",
                    "status",
                    "manifest",
                    "rule_catalog",
                    "software",
                    "policy",
                    "cases",
                    "limitations",
                ],
            )
            self.assertEqual(report["status"], "passed")
            self.assertEqual(report["cases"], cases)
            self.assertEqual(
                report["rule_catalog"]["configured_path"], str(catalog)
            )
            self.assertEqual(
                report["software"]["wsi_dicom"]["version"], "wsi-dicom 0.7.5"
            )

    def test_version_probe_uses_the_shared_bounded_process_owner(self):
        from bench.format_coverage import finalization

        def execute(command, *, stdout_path, stderr_path, **kwargs):
            stdout_path.write_text("wsi-dicom 0.7.5\n", encoding="utf-8")
            stderr_path.write_text("", encoding="utf-8")
            self.assertEqual(kwargs["timeout_secs"], 30)
            self.assertEqual(kwargs["max_output_bytes"], 64 * 1024)
            return {
                "command": command,
                "returncode": 0,
                "timed_out": False,
                "launch_error": None,
                "stdout_truncated": False,
                "stderr_truncated": False,
            }

        with mock.patch.object(
            finalization, "run_bounded_command", side_effect=execute, create=True
        ) as bounded:
            self.assertEqual(
                finalization.command_version(["wsi-dicom", "--version"]),
                "wsi-dicom 0.7.5",
            )

        bounded.assert_called_once()

    @staticmethod
    def case(case_id: str, source: str) -> dict:
        return {
            "id": case_id,
            "format": "test",
            "source": source,
            "source_sha256": "0" * 64,
            "level": 0,
            "transfer_syntax": "htj2k-lossless-rpcl",
            "expected": {"outcome": "success"},
        }


if __name__ == "__main__":
    unittest.main()

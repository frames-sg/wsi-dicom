import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest import mock

import pydicom


REPO_ROOT = Path(__file__).resolve().parents[1]
MANIFEST = REPO_ROOT / "bench" / "negative_bench" / "manifest-v4.json"
EVIDENCE_ROOT = os.environ.get("WSI_DICOM_BENCH_EVIDENCE_ROOT")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


class NegativeBenchGenerationTests(unittest.TestCase):
    def test_manifest_shapes_and_identifiers_fail_closed_at_every_consumer(self):
        from bench.negative_bench.analyze import AnalysisError, analyze_challenge
        from bench.negative_bench.generate import GenerationError, load_manifest
        from bench.negative_bench.run import ChallengeRunError, run_challenge

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest_path = root / "manifest.json"
            for document in (
                [],
                {
                    "schema_version": "wsi-dicom-negative-bench-manifest-v4",
                    "resource_limits": [],
                    "valid_controls": [],
                    "evaluation_cases": [],
                },
                {
                    "schema_version": "wsi-dicom-negative-bench-manifest-v4",
                    "resource_limits": {"max_controls": 1, "max_cases": 1},
                    "valid_controls": [None],
                    "evaluation_cases": [],
                },
                {
                    "schema_version": "wsi-dicom-negative-bench-manifest-v4",
                    "resource_limits": {"max_controls": 1, "max_cases": 1},
                    "valid_controls": [{"control_id": ["unhashable"]}],
                    "evaluation_cases": [],
                },
            ):
                with self.subTest(document=document):
                    manifest_path.write_text(json.dumps(document), encoding="utf-8")
                    with self.assertRaises(GenerationError):
                        load_manifest(manifest_path)

            malicious = {
                "schema_version": "wsi-dicom-negative-bench-manifest-v4",
                "challenge_id": "unsafe",
                "resource_limits": {
                    "max_controls": 1,
                    "max_cases": 1,
                    "max_command_output_bytes": 1024,
                    "max_pixel_frames": 1,
                    "command_timeout_secs": 1,
                    "case_timeout_secs": 1,
                },
                "valid_controls": [{"control_id": "../../outside"}],
                "evaluation_cases": [],
            }
            manifest_path.write_text(json.dumps(malicious), encoding="utf-8")
            with self.assertRaisesRegex(GenerationError, "safe identifier"):
                load_manifest(manifest_path)

            package = root / "package"
            package.mkdir()
            (package / "manifest.json").write_text(json.dumps(malicious), encoding="utf-8")
            binary = root / "wsi-dicom"
            binary.write_bytes(b"binary")
            with mock.patch(
                "bench.negative_bench.run.run_bounded_command"
            ) as bounded, self.assertRaisesRegex(ChallengeRunError, "safe identifier"):
                run_challenge(package, root / "workbench.py", binary)
            bounded.assert_not_called()
            self.assertFalse((root / "outside").exists())

            adjudications = root / "adjudications.json"
            adjudications.write_text(
                json.dumps(
                    {
                        "schema_version": "wsi-dicom-negative-bench-adjudications-v1",
                        "cases": {},
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(AnalysisError, "safe identifier"):
                analyze_challenge(package, adjudications)

    def test_control_encoder_uses_shared_output_and_process_bounds(self):
        from bench.negative_bench import finalize_synthetic_controls as finalize

        evidence = {
            "returncode": 0,
            "timed_out": False,
            "launch_error": None,
            "stdout_truncated": True,
            "stderr_truncated": False,
        }
        def execute(_command, *, stdout_path, stderr_path, **_kwargs):
            stdout_path.write_text("")
            stderr_path.write_text("")
            return evidence

        with mock.patch.object(
            finalize, "run_bounded_command", side_effect=execute, create=True
        ) as bounded:
            with self.assertRaisesRegex(RuntimeError, "output exceeded limit"):
                finalize._run_encoder(["fake-encoder", "input.ppm"])

        self.assertEqual(
            bounded.call_args.kwargs["max_output_bytes"],
            finalize.MAX_COMMAND_OUTPUT_BYTES,
        )

    def test_expected_results_lock_requires_exact_versioned_provenance(self):
        from bench.negative_bench.generate import (
            GenerationError,
            _verify_expected_results_lock,
        )

        protocol = MANIFEST.with_name("protocol-v4.md")
        catalog = REPO_ROOT / "rules" / "wsi-dicom-bench-rules-2026c-v4.json"
        baseline = json.loads(
            MANIFEST.with_name("expected-results-lock-v4.json").read_text()
        )
        invalid_locks = {
            "files": {**baseline, "files": []},
            "schema": {**baseline, "schema_version": "unexpected-lock"},
            "challenge": {**baseline, "challenge_id": "wrong-challenge"},
            "counts": {
                **baseline,
                "expected_counts": {**baseline["expected_counts"], "evaluation_cases": 0},
            },
            "duplicate": {
                **baseline,
                "files": [baseline["files"][0], baseline["files"][0], baseline["files"][2]],
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            lock_path = Path(temporary) / "lock.json"
            for name, document in invalid_locks.items():
                with self.subTest(lock=name):
                    lock_path.write_text(json.dumps(document), encoding="utf-8")
                    with self.assertRaises(GenerationError):
                        _verify_expected_results_lock(
                            lock_path, MANIFEST, protocol, catalog, REPO_ROOT / "rules/wsi-dicom-core-profile-2026c-v2.json"
                        )

    def test_repository_operation_scripts_own_their_import_path(self):
        for relative_path in (
            "bench/negative_bench/generate.py",
            "bench/negative_bench/run.py",
            "bench/negative_bench/analyze.py",
            "bench/negative_bench/finalize.py",
            "bench/negative_bench/finalize_synthetic_controls.py",
        ):
            with self.subTest(script=relative_path):
                completed = subprocess.run(
                    [sys.executable, str(REPO_ROOT / relative_path), "--help"],
                    cwd=REPO_ROOT,
                    capture_output=True,
                    text=True,
                    timeout=10,
                    check=False,
                )
                self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_repository_generator_script_runs_from_the_repository_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "negative-bench-current"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(REPO_ROOT / "bench" / "negative_bench" / "generate.py"),
                    "--manifest",
                    str(MANIFEST),
                    "--output",
                    str(output),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertTrue((output / "manifest.json").is_file())

    def test_mutation_registry_covers_current_cases_and_rejects_duplicates(self):
        from bench.negative_bench.mutations import (
            MUTATION_REGISTRY,
            MutationRegistrationError,
            build_registry,
        )

        manifests = [
            json.loads(MANIFEST.read_text()),
        ]
        expected = {
            case["mutation_name"]
            for manifest in manifests
            for case in manifest["evaluation_cases"]
        }
        self.assertEqual(
            set(MUTATION_REGISTRY) - expected,
            {"invalid_specimen_uid", "total_matrix_too_large"},
        )
        self.assertTrue(expected.issubset(MUTATION_REGISTRY))
        with self.assertRaisesRegex(MutationRegistrationError, "duplicate mutation"):
            build_registry((("one", {"same": object()}), ("two", {"same": object()})))

    def test_generator_finds_repository_from_nested_private_manifest(self):
        from bench.negative_bench.generate import _find_repository_root

        nested = REPO_ROOT / "bench" / "results" / "private-holdout" / "manifest.json"
        self.assertEqual(_find_repository_root(nested), REPO_ROOT)

    def test_current_matrix_is_diversified_and_balanced(self):
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        adjudications = json.loads(
            MANIFEST.with_name("adjudications-v1.json").read_text(encoding="utf-8")
        )["cases"]
        controls = manifest["valid_controls"]
        cases = manifest["evaluation_cases"]

        self.assertEqual(len(controls), 14)
        self.assertEqual(len(cases), 69)
        self.assertEqual(
            {syntax for control in controls for syntax in control.get("transfer_syntax_uids", [control.get("transfer_syntax_uid")])},
            {
                "1.2.840.10008.1.2.1",
                "1.2.840.10008.1.2.4.50",
                "1.2.840.10008.1.2.4.90",
                "1.2.840.10008.1.2.4.91",
                "1.2.840.10008.1.2.4.201",
                "1.2.840.10008.1.2.4.202",
                "1.2.840.10008.1.2.4.203",
            },
        )
        self.assertEqual(
            Counter(case["domain"] for case in cases),
            Counter(pixel=7, geometry=14, color=12, identity=17, conformance=19),
        )
        self.assertEqual(len({case["deterministic_seed"] for case in cases}), 69)
        self.assertEqual(
            {control["source_kind"] for control in controls},
            {"repository_generated_synthetic_no_patient_data"},
        )
        encoded = MANIFEST.read_text(encoding="utf-8")
        self.assertNotIn("/Users/", encoded)
        self.assertNotIn("/Volumes/", encoded)
        self.assertNotIn("private_evaluation", encoded)
        self.assertTrue(
            all(not Path(path).is_absolute() for control in controls for path in control.get("original_paths", [control.get("original_path")]))
        )
        self.assertTrue(
            set(adjudications).issubset({case["case_id"] for case in cases})
        )
        self.assertFalse(any(case_id.startswith("TH-") for case_id in adjudications))

    def test_current_end_to_end_generation_uses_only_locked_sources(self):
        from bench.negative_bench.finalize import _verify_generation_sums
        from bench.negative_bench.generate import generate_challenge

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "negative-bench-current"
            generate_challenge(MANIFEST, output)
            generated = json.loads((output / "manifest.json").read_text())

            self.assertEqual(len(generated["valid_controls"]), 14)
            self.assertEqual(len(generated["evaluation_cases"]), 69)
            self.assertEqual(len(list((output / "controls").rglob("*.dcm"))), 15)
            self.assertEqual(len(list((output / "cases").glob("*/input/*.dcm"))), 79)
            self.assertFalse((output / "source-inventory-private.json").exists())
            for relative in (
                "analysis/analysis_artifacts.py",
                "analysis/adjudications-v1.json",
                "analysis/file_digest.py",
                "generator/bench/process_evidence.py",
                "generator/bench/path_identifiers.py",
                "generator/bench/negative_bench/identifiers.py",
                "generator/finalize.py",
            ):
                self.assertTrue((output / relative).is_file(), relative)
            for case in generated["evaluation_cases"]:
                report = json.loads(
                    (output / "cases" / case["case_id"] / "generation.json").read_text()
                )
                self.assertEqual(report["source_control_sha256"], case["source_control_sha256"])
                self.assertTrue(report["changes"])

            reproduced = Path(temporary) / "reproduced"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(output / "generator" / "generate.py"),
                    "--manifest",
                    str(output / "manifest.json"),
                    "--output",
                    str(reproduced),
                ],
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)

            fake_workbench = Path(temporary) / "fake-workbench.py"
            fake_workbench.write_text(
                """import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("dicom", type=Path)
parser.add_argument("--output", type=Path, required=True)
args, _ = parser.parse_known_args()
root = args.dicom.resolve()
while root != root.parent and not (root / "manifest.json").is_file():
    root = root.parent
manifest = json.loads((root / "manifest.json").read_text())
cases = {item["case_id"]: item for item in manifest["evaluation_cases"]}
identifier = args.dicom.stem if args.dicom.parent.name == "controls" else next(part for part in args.dicom.parts if part in cases)
case = cases.get(identifier)
status = "failed" if case else "passed"
domains = {name: {"status": "failed" if case and name == case["domain"] else "passed"} for name in ("pixel", "geometry", "color", "identity", "conformance")}
profile_path = root / "expected-results" / Path(manifest["core_profile"]["path"]).name
catalog_path = root / "expected-results" / Path(manifest["rule_catalog"]["path"]).name
catalog = json.loads(catalog_path.read_text())
inputs = sorted(args.dicom.rglob("*.dcm")) if args.dicom.is_dir() else [args.dicom]
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
findings = [
    {"status": "failed" if case and rule["rule_id"] in case["expected_failing_rules"] else "passed",
     "rule_id": rule["rule_id"], "primary_domain": rule["primary_domain"], "rule_kind": rule["rule_kind"],
     "check_name": name, "path": str(path), "catalog_status": "mapped"}
    for path in inputs for rule in catalog["rules"] for name in rule["check_names"]
]
report = {
    "schema_version": "wsi-dicom-bench-workbench-report-v2", "status": status,
    "profile": {"id": manifest["core_profile"]["profile_id"], "sha256": digest(profile_path)},
    "catalog": {"sha256": digest(catalog_path)},
    "slide": {"instances": [{"path": str(path), "sha256": digest(path)} for path in inputs]},
    "findings": findings, "domains": domains, "validator_comparison": {"external": {}},
}
args.output.mkdir(parents=True)
(args.output / "workbench-report.json").write_text(json.dumps(report) + "\\n")
print(json.dumps({"status": status}))
raise SystemExit(0 if status == "passed" else 1)
""",
                encoding="utf-8",
            )
            fake_binary = Path(temporary) / "wsi-dicom"
            fake_binary.write_bytes(b"test binary")
            commands = (
                [
                    sys.executable,
                    str(output / "generator" / "run.py"),
                    "--package",
                    str(output),
                    "--workbench",
                    str(fake_workbench),
                    "--wsi-dicom",
                    str(fake_binary),
                ],
                [
                    sys.executable,
                    str(output / "analysis" / "analyze.py"),
                    "--package",
                    str(output),
                    "--adjudications",
                    str(output / "analysis" / "adjudications-v1.json"),
                ],
                [
                    sys.executable,
                    str(output / "generator" / "finalize.py"),
                    "--package",
                    str(output),
                ],
            )
            for command in commands:
                completed = subprocess.run(
                    command,
                    cwd=output,
                    capture_output=True,
                    text=True,
                    timeout=30,
                    check=False,
                )
                retained_errors = "\n".join(
                    path.read_text(encoding="utf-8", errors="replace")
                    for path in (output / "validator-results").rglob("*.stderr.txt")
                )
                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr + "\n" + retained_errors,
                )
            self.assertEqual(list(output.rglob("__pycache__")), [])
            _verify_generation_sums(output)
            self.assertTrue((output / "SHA256SUMS").is_file())

    def test_file_meta_dataset_sop_mismatch_survives_serialization(self):
        from bench.negative_bench.generate import _generate_case

        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        control = manifest["valid_controls"][0]
        source = REPO_ROOT / control["original_path"]
        case = {
            "mutation_name": "file_meta_dataset_sop_mismatch",
            "deterministic_seed": "identity-mismatch-regression",
            "mutation_parameters": {},
        }

        with tempfile.TemporaryDirectory() as temporary:
            outputs, changes = _generate_case(case, source, Path(temporary))
            dataset = pydicom.dcmread(outputs[0], stop_before_pixels=True)

        self.assertNotEqual(
            str(dataset.file_meta.MediaStorageSOPInstanceUID),
            str(dataset.SOPInstanceUID),
        )
        self.assertTrue(
            any(change.get("attribute") == "MediaStorageSOPInstanceUID" for change in changes)
        )

    def test_seed_selects_and_records_the_corrupted_frame(self):
        from bench.negative_bench.generate import _generate_case

        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        base_case = next(
            case
            for case in manifest["evaluation_cases"]
            if case["mutation_name"] == "truncate_first_codestream"
        )
        control = next(
            item
            for item in manifest["valid_controls"]
            if item["control_id"] == base_case["source_control_ids"][0]
        )
        source = REPO_ROOT / control["original_path"]

        selected = []
        with tempfile.TemporaryDirectory() as temporary:
            for index, seed in enumerate(["seed-boundary-a", "seed-boundary-b"]):
                case = {**base_case, "deterministic_seed": seed}
                destination = Path(temporary) / str(index)
                destination.mkdir()
                _, changes = _generate_case(case, source, destination)
                selected.append(
                    next(
                        change["frame_index"]
                        for change in changes
                        if change.get("kind") == "codestream_truncation"
                    )
                )

        self.assertNotEqual(selected[0], selected[1])

    def test_generation_is_atomic_deterministic_and_preserves_sources(self):
        from bench.negative_bench.generate import generate_challenge

        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        source_hashes = {
            REPO_ROOT / path: digest
            for control in manifest["valid_controls"]
            for path, digest in zip(control.get("original_paths", [control.get("original_path")]), control["sha256"] if isinstance(control["sha256"], list) else [control["sha256"]], strict=True)
        }
        for path, expected in source_hashes.items():
            self.assertEqual(sha256_file(path), expected)

        with tempfile.TemporaryDirectory() as temporary:
            first = Path(temporary) / "first"
            second = Path(temporary) / "second"
            generate_challenge(MANIFEST, first)
            generate_challenge(MANIFEST, second)

            first_manifest = json.loads((first / "manifest.json").read_text())
            self.assertEqual(len(first_manifest["valid_controls"]), 14)
            self.assertEqual(len(first_manifest["evaluation_cases"]), 69)
            self.assertEqual(len(list((first / "controls").rglob("*.dcm"))), 15)
            self.assertEqual(len(list((first / "cases").glob("*/input/*.dcm"))), 79)
            self.assertEqual(
                (first / "generator" / "requirements.txt").read_text(),
                "pydicom==3.0.2\n",
            )

            first_sums = (first / "generation-SHA256SUMS").read_text()
            second_sums = (second / "generation-SHA256SUMS").read_text()
            self.assertEqual(first_sums, second_sums)
            for path, expected in source_hashes.items():
                self.assertEqual(sha256_file(path), expected)

    def test_generation_refuses_existing_output(self):
        from bench.negative_bench.generate import GenerationError, generate_challenge

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "existing"
            output.mkdir()
            with self.assertRaisesRegex(GenerationError, "already exists"):
                generate_challenge(MANIFEST, output)

    def test_identifying_control_is_rejected_without_publishing_output(self):
        from bench.negative_bench.generate import GenerationError, generate_challenge

        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "identifying.dcm"
            dataset = pydicom.dcmread(
                REPO_ROOT / manifest["valid_controls"][0]["original_path"]
            )
            dataset.PatientName = "IDENTIFIED^PATIENT"
            dataset.save_as(source)
            control = manifest["valid_controls"][0]
            manifest["valid_controls"] = [
                {
                    **control,
                    "original_path": str(source),
                    "size_bytes": source.stat().st_size,
                    "sha256": sha256_file(source),
                }
            ]
            manifest["evaluation_cases"] = []
            manifest_path = root / "manifest-v4.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            output = root / "challenge"

            with self.assertRaisesRegex(GenerationError, "patient identity policy"):
                generate_challenge(manifest_path, output)
            self.assertFalse(output.exists())


class NegativeBenchRunTests(unittest.TestCase):
    def test_runner_preserves_nonzero_workbench_output_as_observation(self):
        from bench.negative_bench.run import run_challenge

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary) / "package"
            (package / "controls").mkdir(parents=True)
            (package / "cases" / "NB-ONE" / "input").mkdir(parents=True)
            (package / "observed-results").mkdir()
            (package / "validator-results").mkdir()
            (package / "manifest.json").write_text(
                json.dumps(
                    {
                        "resource_limits": {
                            "case_timeout_secs": 30,
                            "max_command_output_bytes": 4096,
                            "max_pixel_frames": 1,
                            "command_timeout_secs": 5,
                        },
                        "valid_controls": [{"control_id": "VC-ONE"}],
                        "evaluation_cases": [{"case_id": "NB-ONE"}],
                    }
                )
            )
            (package / "controls" / "VC-ONE.dcm").write_bytes(b"control")
            (package / "cases" / "NB-ONE" / "input" / "one.dcm").write_bytes(
                b"case"
            )
            fake_binary = package / "wsi-dicom"
            fake_binary.write_bytes(b"binary")

            def fake_run(command, *, stdout_path, stderr_path, **kwargs):
                self.assertEqual(
                    Path(command[command.index("--catalog") + 1]).name,
                    "wsi-dicom-bench-rules-2026c-v4.json",
                )
                output_index = command.index("--output") + 1
                workbench = Path(command[output_index])
                workbench.mkdir(parents=True)
                (workbench / "workbench-report.json").write_text(
                    json.dumps(
                        {
                            "status": "failed",
                            "findings": [],
                            "nested_paths": {
                                "package": str(package.resolve()),
                                "binary": str(fake_binary.resolve()),
                                "python": str(Path(sys.executable).resolve()),
                            },
                        }
                    )
                )
                stdout_path.write_text(
                    json.dumps(
                        {"status": "failed", "output": str(package.resolve())}
                    )
                    + "\n"
                )
                stderr_path.write_bytes(b"retained diagnostic\n")
                return {
                    "command": command,
                    "returncode": 1,
                    "timed_out": False,
                    "launch_error": None,
                    "elapsed_seconds": 0.25,
                    "stdout_truncated": False,
                    "stderr_truncated": False,
                }

            with mock.patch("bench.negative_bench.run.run_bounded_command", fake_run), mock.patch(
                "bench.negative_bench.run.platform.platform", return_value="test-os"
            ), mock.patch(
                "bench.negative_bench.run.platform.machine", return_value="test-machine"
            ):
                summary = run_challenge(
                    package,
                    Path("fake-workbench.py"),
                    fake_binary,
                )

            self.assertEqual(summary["executions"][1]["returncode"], 1)
            raw = package / "validator-results" / "NB-ONE" / "launcher.stderr.txt"
            self.assertEqual(raw.read_text(), "retained diagnostic\n")
            encoded = json.dumps(summary)
            self.assertNotIn(str(package.resolve()), encoded)
            self.assertNotIn(str(Path(sys.executable).resolve()), encoded)
            self.assertFalse(
                any(
                    Path(argument).is_absolute()
                    for execution in summary["executions"]
                    for argument in execution["command"]
                )
            )
            retained_text = "\n".join(
                path.read_text(encoding="utf-8", errors="replace")
                for root in (
                    package / "observed-results",
                    package / "validator-results",
                )
                for path in root.rglob("*")
                if path.is_file()
            )
            self.assertNotIn(str(package.resolve()), retained_text)
            self.assertNotIn(str(fake_binary.resolve()), retained_text)
            self.assertNotIn(str(Path(sys.executable).resolve()), retained_text)

    def test_runner_failures_never_publish_a_summary(self):
        from bench.negative_bench.run import ChallengeRunError, run_challenge

        failure_cases = {
            "timeout": {"timed_out": True},
            "stdout-truncated": {"stdout_truncated": True},
            "stderr-truncated": {"stderr_truncated": True},
            "launch-error": {"launch_error": "spawn failed"},
            "bad-returncode": {"returncode": 2},
            "missing-report": {"write_report": False},
        }
        for name, changes in failure_cases.items():
            with self.subTest(failure=name), tempfile.TemporaryDirectory() as temporary:
                package = Path(temporary) / "package"
                (package / "controls").mkdir(parents=True)
                (package / "observed-results").mkdir()
                (package / "validator-results").mkdir()
                (package / "expected-results").mkdir()
                (package / "manifest.json").write_text(
                    json.dumps(
                        {
                            "resource_limits": {
                                "case_timeout_secs": 30,
                                "max_command_output_bytes": 4096,
                                "max_pixel_frames": 1,
                                "command_timeout_secs": 5,
                            },
                            "valid_controls": [{"control_id": "VC-ONE"}],
                            "evaluation_cases": [],
                        }
                    )
                )
                (package / "controls" / "VC-ONE.dcm").write_bytes(b"control")
                binary = package / "wsi-dicom"
                binary.write_bytes(b"binary")

                def fake_run(command, *, stdout_path, stderr_path, **kwargs):
                    stdout_path.write_bytes(b"{}\n")
                    stderr_path.write_bytes(b"")
                    if changes.get("write_report", True):
                        evidence = Path(command[command.index("--output") + 1])
                        evidence.mkdir(parents=True)
                        (evidence / "workbench-report.json").write_text("{}")
                    return {
                        "command": command,
                        "returncode": 0,
                        "timed_out": False,
                        "launch_error": None,
                        "elapsed_seconds": 0.1,
                        "stdout_truncated": False,
                        "stderr_truncated": False,
                        **{key: value for key, value in changes.items() if key != "write_report"},
                    }

                with mock.patch(
                    "bench.negative_bench.run.run_bounded_command", fake_run
                ), self.assertRaises(ChallengeRunError):
                    run_challenge(package, Path("workbench.py"), binary)
                self.assertFalse(
                    (package / "observed-results" / "run-summary.json").exists()
                )

    def test_runner_refuses_preexisting_per_input_results_before_launch(self):
        from bench.negative_bench.run import ChallengeRunError, run_challenge

        for relative in (
            "observed-results/VC-ONE/workbench",
            "validator-results/VC-ONE",
        ):
            with self.subTest(relative=relative), tempfile.TemporaryDirectory() as temporary:
                package = Path(temporary) / "package"
                (package / "controls").mkdir(parents=True)
                (package / "observed-results").mkdir()
                (package / "validator-results").mkdir()
                (package / relative).mkdir(parents=True)
                (package / "manifest.json").write_text(
                    json.dumps(
                        {
                            "resource_limits": {
                                "case_timeout_secs": 30,
                                "max_command_output_bytes": 4096,
                                "max_pixel_frames": 1,
                                "command_timeout_secs": 5,
                            },
                            "valid_controls": [{"control_id": "VC-ONE"}],
                            "evaluation_cases": [],
                        }
                    )
                )
                (package / "controls" / "VC-ONE.dcm").write_bytes(b"control")
                binary = package / "wsi-dicom"
                binary.write_bytes(b"binary")

                with mock.patch(
                    "bench.negative_bench.run.run_bounded_command"
                ) as bounded, self.assertRaisesRegex(
                    ChallengeRunError, "result path already exists"
                ):
                    run_challenge(package, Path("workbench.py"), binary)

                bounded.assert_not_called()
                self.assertFalse(
                    (package / "observed-results" / "run-summary.json").exists()
                )


class NegativeBenchAnalysisTests(unittest.TestCase):
    def test_execution_error_cannot_count_as_detection_or_localization(self):
        from bench.negative_bench.analyze import _build_case_rows

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            report_dir = package / "observed-results/NB-ONE/workbench"
            report_dir.mkdir(parents=True)
            (report_dir / "workbench-report.json").write_text(json.dumps({
                "status": "execution_error",
                "findings": [{"status": "execution_error", "rule_id": "R1", "primary_domain": "pixel", "rule_kind": "independent_decoder", "catalog_status": "mapped"}],
                "validator_comparison": {"external": {"dciodvfy": "execution_error"}},
            }))
            rows = _build_case_rows(package, {
                "valid_controls": [], "evaluation_cases": [{"case_id": "NB-ONE", "domain": "pixel", "expected_failing_rules": ["R1"], "expected_unaffected_rules": []}],
            }, {"executions": [{"id": "NB-ONE", "runtime_seconds": 1}]})
            self.assertTrue(rows[0]["execution_failure"])
            self.assertFalse(rows[0]["bench_detected"])
            self.assertFalse(rows[0]["expected_rule_localized"])

    def test_synthetic_package_drives_rows_metrics_and_artifacts(self):
        from bench.negative_bench.analyze import analyze_challenge

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary) / "package"
            reports = package / "observed-results"
            for identifier in ("VC-ONE", "NB-ONE"):
                (reports / identifier / "workbench").mkdir(parents=True)
            manifest = {
                "challenge_id": "synthetic-characterization",
                "valid_controls": [{"control_id": "VC-ONE"}],
                "evaluation_cases": [
                    {
                        "case_id": "NB-ONE",
                        "domain": "pixel",
                        "expected_failing_rules": ["WSI-PX-001"],
                        "expected_unaffected_rules": [],
                    }
                ],
            }
            (package / "manifest.json").write_text(json.dumps(manifest))
            (reports / "run-summary.json").write_text(
                json.dumps(
                    {
                        "executions": [
                            {"id": "VC-ONE", "runtime_seconds": 0.25},
                            {"id": "NB-ONE", "runtime_seconds": 0.75},
                        ]
                    }
                )
            )
            external_control = {
                name: "passed"
                for name in ("dciodvfy", "dcentvfy", "validate_iods")
            }
            control_report = {
                "status": "passed",
                "findings": [],
                "domains": {"pixel": {"status": "passed"}},
                "validator_comparison": {"external": external_control},
            }
            case_report = {
                "status": "failed",
                "findings": [
                    {
                        "status": "failed",
                        "rule_id": "WSI-PX-001",
                        "primary_domain": "pixel",
                        "rule_kind": "intrinsic",
                        "catalog_status": "mapped",
                    }
                ],
                "domains": {"pixel": {"status": "failed"}},
                "validator_comparison": {
                    "external": {
                        "dciodvfy": "failed",
                        "dcentvfy": "passed",
                        "validate_iods": "passed",
                    }
                },
            }
            (reports / "VC-ONE" / "workbench" / "workbench-report.json").write_text(
                json.dumps(control_report)
            )
            (reports / "NB-ONE" / "workbench" / "workbench-report.json").write_text(
                json.dumps(case_report)
            )
            adjudications = Path(temporary) / "adjudications.json"
            adjudications.write_text(
                json.dumps(
                    {
                        "schema_version": "wsi-dicom-negative-bench-adjudications-v1",
                        "cases": {},
                    }
                )
            )

            summary = analyze_challenge(package, adjudications)

            self.assertEqual(summary["bench_detected"], 1)
            self.assertEqual(summary["validator_detection_counts"]["dciodvfy"], 1)
            self.assertEqual(summary["validator_unique_detection_counts"]["dciodvfy"], 0)
            self.assertEqual(summary["total_runtime_seconds"], 1.0)
            rows = json.loads(
                (package / "analysis/case-validator-matrix.json").read_text()
            )
            self.assertEqual([row["id"] for row in rows], ["VC-ONE", "NB-ONE"])
            domains = json.loads((package / "analysis/domain-metrics.json").read_text())
            self.assertEqual(domains[0]["controls_passing_domain"], 1)
            self.assertTrue((package / "analysis/USCAP-results.md").is_file())
            self.assertTrue((package / "analysis/validator-detection.svg").is_file())

            case_report["status"] = "passed"
            case_report["findings"] = []
            case_report["domains"]["pixel"]["status"] = "passed"
            (reports / "NB-ONE" / "workbench" / "workbench-report.json").write_text(
                json.dumps(case_report)
            )
            missed_summary = analyze_challenge(package, adjudications)

            self.assertEqual(missed_summary["bench_detected"], 0)
            self.assertEqual(missed_summary["rule_localization_accuracy_among_detected"], None)
            self.assertEqual(missed_summary["domain_localization_accuracy_among_detected"], None)
            self.assertEqual(missed_summary["rule_localization_accuracy"], 0)
            self.assertEqual(missed_summary["domain_localization_accuracy"], 0)

    def test_results_markdown_uses_dynamic_case_and_domain_counts(self):
        from bench.negative_bench.analyze import _results_markdown

        summary = {
            "bench_detected": 15,
            "evaluation_cases": 15,
            "defect_detection_sensitivity": 1.0,
            "defect_detection_sensitivity_ci95": [0.78, 1.0],
            "valid_controls_accepted": 10,
            "valid_controls": 10,
            "valid_control_specificity": 1.0,
            "valid_control_specificity_ci95": [0.69, 1.0],
            "rule_localized_detected_cases": 15,
            "rule_localization_accuracy": 1.0,
            "intrinsic_detected": 15,
            "cases_not_detected": [],
            "cases_not_rule_localized": [],
            "validator_detection_counts": {
                "dciodvfy": 2,
                "dcentvfy": 1,
                "validate_iods": 3,
            },
            "intrinsic_external_agreement": {
                name: {"agreeing_cases": count, "executed_cases": 15}
                for name, count in {
                    "dciodvfy": 2,
                    "dcentvfy": 1,
                    "validate_iods": 3,
                }.items()
            },
            "validator_unique_detection_counts": {
                "wsi_dicom_intrinsic": 12,
                "dciodvfy": 0,
            },
            "execution_failures": 0,
            "unmapped_findings": 0,
            "total_runtime_seconds": 1.0,
        }
        domain_rows = [
            {"domain": "pixel", "detected": 3, "evaluation_cases": 3, "domain_localized": 3}
        ]

        text = _results_markdown(summary, domain_rows)

        self.assertIn("pixel (3/3)", text)
        self.assertIn("2/15 cases with dciodvfy", text)
        self.assertIn("valid-control specificity", text)
        self.assertNotIn("synthetic-control specificity", text)
        self.assertNotIn("/30", text)

    def test_exact_binomial_interval_handles_all_successes_and_failures(self):
        from bench.negative_bench.analyze import exact_binomial

        self.assertEqual(exact_binomial(30, 30)[1], 1.0)
        self.assertAlmostEqual(exact_binomial(30, 30)[0], 0.025 ** (1 / 30), places=12)
        self.assertEqual(exact_binomial(0, 10)[0], 0.0)
        self.assertAlmostEqual(
            exact_binomial(0, 10)[1], 1 - 0.025 ** (1 / 10), places=12
        )

    @unittest.skipUnless(
        EVIDENCE_ROOT,
        "set WSI_DICOM_BENCH_EVIDENCE_ROOT to run retained-evidence regression",
    )
    def test_frozen_run_metrics_are_reproducible(self):
        from bench.negative_bench.analyze import summarize_challenge

        package = Path(EVIDENCE_ROOT) / "wsi-dicom-negative-bench-v3"
        summary = summarize_challenge(package)
        self.assertEqual(summary["evaluation_cases"], 69)
        self.assertEqual(summary["valid_controls"], 14)
        self.assertEqual(summary["bench_detected"], 69)
        self.assertEqual(summary["intrinsic_detected"], 69)
        self.assertEqual(summary["valid_controls_accepted"], 14)
        self.assertEqual(summary["rule_localized_detected_cases"], 69)
        self.assertEqual(summary["domain_localized_detected_cases"], 69)
        adjudications = REPO_ROOT / "bench/negative_bench/adjudications-v1.json"
        self.assertEqual(
            summary["adjudications"],
            {
                "schema_version": "wsi-dicom-negative-bench-adjudications-v1",
                "sha256": sha256_file(adjudications),
            },
        )


class NegativeBenchFinalizationTests(unittest.TestCase):
    def test_analysis_refuses_to_modify_a_sealed_package(self):
        from bench.negative_bench.analyze import AnalysisError, analyze_challenge

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            seal = package / "SHA256SUMS"
            seal.write_text("retained seal\n")
            with self.assertRaisesRegex(AnalysisError, "sealed"):
                analyze_challenge(package)
            self.assertEqual(seal.read_text(), "retained seal\n")

    @staticmethod
    def _write_completed_package(package: Path) -> None:
        (package / "observed-results").mkdir(parents=True)
        (package / "analysis").mkdir()
        manifest = package / "manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "challenge_id": "test-challenge",
                    "valid_controls": [{"control_id": "VC-ONE"}],
                    "evaluation_cases": [{"case_id": "NB-ONE"}],
                }
            )
            + "\n"
        )
        control = package / "controls" / "VC-ONE.dcm"
        control.parent.mkdir()
        control.write_bytes(b"control")
        case = package / "cases" / "NB-ONE" / "input" / "instance.dcm"
        case.parent.mkdir(parents=True)
        case.write_bytes(b"case")
        generation_files = (manifest, control, case)
        (package / "generation-SHA256SUMS").write_text(
            "".join(
                f"{sha256_file(path)}  {path.relative_to(package)}\n"
                for path in sorted(generation_files)
            ),
            encoding="utf-8",
        )
        executions = []
        for identifier, cohort in (
            ("VC-ONE", "valid-controls-v1"),
            ("NB-ONE", "evaluation-v1"),
        ):
            executions.append({"id": identifier, "cohort": cohort})
            report = (
                package
                / "observed-results"
                / identifier
                / "workbench"
                / "workbench-report.json"
            )
            report.parent.mkdir(parents=True)
            report.write_text(
                json.dumps(
                    {"schema_version": "wsi-dicom-bench-workbench-report-v1"}
                )
                + "\n"
            )
        (package / "observed-results" / "run-summary.json").write_text(
            json.dumps(
                {
                    "schema_version": "wsi-dicom-negative-bench-run-summary-v1",
                    "challenge_id": "test-challenge",
                    "executions": executions,
                }
            )
            + "\n"
        )
        (package / "analysis" / "summary.json").write_text(
            json.dumps(
                {
                    "schema_version": "wsi-dicom-negative-bench-analysis-v1",
                    "challenge_id": "test-challenge",
                    "evaluation_cases": 1,
                    "valid_controls": 1,
                }
            )
            + "\n"
        )
        for relative in (
            "adjudication.csv",
            "analysis/USCAP-results.md",
            "analysis/case-validator-matrix.csv",
            "analysis/case-validator-matrix.json",
            "analysis/disagreements.csv",
            "analysis/disagreements.json",
            "analysis/domain-metrics.csv",
            "analysis/domain-metrics.json",
            "analysis/validator-detection.svg",
        ):
            path = package / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture\n")

    def test_final_seal_is_atomic_complete_and_exclusive(self):
        from bench.negative_bench.finalize import FinalizationError, finalize_package

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary) / "package"
            self._write_completed_package(package)

            result = finalize_package(package)
            seal = package / "SHA256SUMS"
            lines = seal.read_text(encoding="utf-8").splitlines()

            self.assertEqual(result["files"], len(lines))
            self.assertEqual(lines, sorted(lines, key=lambda line: line.split("  ", 1)[1]))
            self.assertFalse(any(str(package) in line for line in lines))
            self.assertFalse((package / ".finalize.lock").exists())
            for line in lines:
                digest, relative = line.split("  ", 1)
                self.assertEqual(sha256_file(package / relative), digest)
            with self.assertRaisesRegex(FinalizationError, "already exists"):
                finalize_package(package)

    def test_finalizer_cannot_seal_core_reports_without_the_profile_gate(self):
        from bench.negative_bench.finalize import FinalizationError, finalize_package
        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            self._write_completed_package(package)
            path = package / "manifest.json"
            manifest = json.loads(path.read_text())
            manifest["core_profile"] = {"path": "missing-profile.json", "profile_id": "missing-profile"}
            manifest["rule_catalog"] = {"path": "missing-catalog.json"}
            path.write_text(json.dumps(manifest) + "\n")
            sums = package / "generation-SHA256SUMS"
            entries = [line.split("  ", 1)[1] for line in sums.read_text().splitlines()]
            sums.write_text("".join(f"{sha256_file(package / relative)}  {relative}\n" for relative in entries))
            with self.assertRaisesRegex(FinalizationError, "core evidence gate failed"):
                finalize_package(package)
            self.assertFalse((package / "SHA256SUMS").exists())

    def test_final_seal_requires_run_and_analysis_outputs(self):
        from bench.negative_bench.finalize import FinalizationError, finalize_package

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            (package / "manifest.json").write_text("{}\n")
            (package / "generation-SHA256SUMS").write_text(
                f"{sha256_file(package / 'manifest.json')}  manifest.json\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(FinalizationError, "required finalization input"):
                finalize_package(package)

            self.assertFalse((package / "SHA256SUMS").exists())

    def test_final_seal_rejects_generation_hash_mismatch(self):
        from bench.negative_bench.finalize import FinalizationError, finalize_package

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            self._write_completed_package(package)
            (package / "manifest.json").write_text('{"tampered": true}\n')

            with self.assertRaisesRegex(FinalizationError, "generation SHA-256 mismatch"):
                finalize_package(package)

            self.assertFalse((package / "SHA256SUMS").exists())

    def test_final_seal_rejects_incomplete_execution_evidence(self):
        from bench.negative_bench.finalize import FinalizationError, finalize_package

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            self._write_completed_package(package)
            summary_path = package / "observed-results" / "run-summary.json"
            summary = json.loads(summary_path.read_text())
            summary["executions"].pop()
            summary_path.write_text(json.dumps(summary) + "\n")

            with self.assertRaisesRegex(FinalizationError, "executions do not match"):
                finalize_package(package)

    def test_final_seal_rejects_incomplete_or_unsafe_generation_manifest(self):
        from bench.negative_bench.finalize import FinalizationError, finalize_package

        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            self._write_completed_package(package)
            unsealed = package / "protocol.md"
            unsealed.write_text("protocol\n")

            with self.assertRaisesRegex(FinalizationError, "generation file set"):
                finalize_package(package)

            (package / "generation-SHA256SUMS").write_text(
                f"{sha256_file(package / 'manifest.json')}  ../manifest.json\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(FinalizationError, "unsafe generation path"):
                finalize_package(package)


if __name__ == "__main__":
    unittest.main()

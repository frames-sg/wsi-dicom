import copy
import json
import tempfile
import unittest
from pathlib import Path

import pydicom


REPO_ROOT = Path(__file__).resolve().parents[1]
PROFILE_PATH = REPO_ROOT / "rules" / "wsi-dicom-core-profile-2026c-v2.json"
CATALOG_PATH = REPO_ROOT / "rules" / "wsi-dicom-bench-rules-2026c-v4.json"
MANIFEST_PATH = REPO_ROOT / "bench" / "negative_bench" / "manifest-v4.json"
EXPECTED_LOCK_PATH = (
    REPO_ROOT / "bench" / "negative_bench" / "expected-results-lock-v4.json"
)


class CoreProfileCoverageTests(unittest.TestCase):
    def test_profile_has_complete_locked_coverage(self):
        from bench.core_profile import load_profile, validate_profile_coverage

        profile = load_profile(PROFILE_PATH)
        catalog = json.loads(CATALOG_PATH.read_text(encoding="utf-8"))
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))

        summary = validate_profile_coverage(profile, catalog, manifest)

        self.assertEqual(profile["profile_id"], "wsi-dicom-core-profile-2026c-v2")
        self.assertEqual(profile["profile_version"], "2")
        self.assertEqual(profile["dicom_edition"], "2026c")
        self.assertEqual(summary["uncovered_requirements"], [])
        self.assertEqual(summary["intrinsic_rule_count"], 18)
        self.assertEqual(summary["intrinsic_rules_without_negative_cases"], [])
        self.assertEqual(
            profile["required_external_validators"],
            ["dciodvfy", "dcentvfy", "validate_iods"],
        )
        expected_lock = json.loads(EXPECTED_LOCK_PATH.read_text(encoding="utf-8"))
        self.assertEqual(
            expected_lock["core_profile"],
            {
                "profile_id": profile["profile_id"],
                "version": profile["profile_version"],
                "sha256": next(
                    item["sha256"]
                    for item in expected_lock["files"]
                    if item["path"].endswith(PROFILE_PATH.name)
                ),
            },
        )

    def test_profile_gate_rejects_missing_positive_or_negative_coverage(self):
        from bench.core_profile import CoreProfileError, load_profile, validate_profile_coverage

        profile = load_profile(PROFILE_PATH)
        catalog = json.loads(CATALOG_PATH.read_text(encoding="utf-8"))
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))

        for field in ("positive_control_ids", "negative_case_ids"):
            broken = copy.deepcopy(profile)
            broken["requirements"][0][field] = []
            with self.subTest(field=field), self.assertRaises(CoreProfileError):
                validate_profile_coverage(broken, catalog, manifest)

    def test_profile_loader_rejects_duplicate_requirement_ids(self):
        from bench.core_profile import CoreProfileError, load_profile

        document = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
        document["requirements"].append(copy.deepcopy(document["requirements"][0]))
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "profile.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaisesRegex(CoreProfileError, "duplicate requirement"):
                load_profile(path)

    def test_profile_loader_requires_an_explicit_profile_version(self):
        from bench.core_profile import CoreProfileError, load_profile

        document = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
        document.pop("profile_version", None)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "profile.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaisesRegex(CoreProfileError, "profile_version"):
                load_profile(path)

    def test_current_expected_results_lock_binds_profile_identity_version_and_hash(self):
        from bench.negative_bench.generate import (
            GenerationError,
            _verify_expected_results_lock,
        )

        baseline = json.loads(EXPECTED_LOCK_PATH.read_text(encoding="utf-8"))
        for field, value in (
            ("profile_id", "wrong-profile"),
            ("version", "wrong-version"),
            ("sha256", "0" * 64),
        ):
            broken = copy.deepcopy(baseline)
            broken["core_profile"][field] = value
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                lock_path = Path(temporary) / "expected-results-lock-v4.json"
                lock_path.write_text(json.dumps(broken), encoding="utf-8")
                with self.assertRaisesRegex(GenerationError, "core-profile identity"):
                    _verify_expected_results_lock(
                        lock_path,
                        MANIFEST_PATH,
                        MANIFEST_PATH.with_name("protocol-v4.md"),
                        CATALOG_PATH,
                        PROFILE_PATH,
                    )

    def test_core_control_builder_adds_monochrome_and_multilevel_controls(self):
        from bench.negative_bench.build_core_profile_controls import build_controls

        source = REPO_ROOT / "bench" / "negative_bench" / "controls-v2"
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "controls"
            build_controls(source, output)
            root_controls = sorted(output.glob("CP*.dcm"))
            pyramid = sorted((output / "CP12-PYRAMID-2L").glob("*.dcm"))

            self.assertEqual(len(root_controls), 13)
            self.assertEqual(len(pyramid), 2)
            mono = pydicom.dcmread(output / "CP11-MONO-EVRLE-4F.dcm")
            self.assertEqual(mono.PhotometricInterpretation, "MONOCHROME2")
            self.assertEqual(mono.SamplesPerPixel, 1)
            self.assertEqual(mono.PresentationLUTShape, "IDENTITY")
            self.assertEqual(float(mono.RescaleIntercept), 0.0)
            self.assertEqual(float(mono.RescaleSlope), 1.0)

            levels = [pydicom.dcmread(path) for path in pyramid]
            derivation = levels[1].SharedFunctionalGroupsSequence[0].DerivationImageSequence[0]
            self.assertEqual(
                str(derivation.SourceImageSequence[0].ReferencedSOPInstanceUID),
                str(levels[0].SOPInstanceUID),
            )
            self.assertEqual(derivation.DerivationCodeSequence[0].CodeValue, "113085")
            self.assertEqual({str(level.PyramidUID) for level in levels}, {str(levels[0].PyramidUID)})
            self.assertEqual(
                {
                    (
                        round(float(level.ImagedVolumeWidth), 9),
                        round(float(level.ImagedVolumeHeight), 9),
                    )
                    for level in levels
                },
                {(0.001, 0.002)},
            )
            # Independent oracle: four 2x2 source tiles form a 4x4 raster.
            tiles = levels[0].pixel_array
            expected_pixels = b"".join(bytes(tiles[index, 0, 0]) for index in range(4))
            self.assertEqual(bytes(levels[1].PixelData), expected_pixels)
            source = levels[1].SourceImageSequence[0]
            self.assertEqual(str(source.ReferencedSOPClassUID), str(levels[0].SOPClassUID))
            self.assertEqual(
                str(source.ReferencedSOPInstanceUID), str(levels[0].SOPInstanceUID)
            )
            common_reference = levels[1].ReferencedSeriesSequence[0]
            self.assertEqual(
                str(common_reference.SeriesInstanceUID), str(levels[0].SeriesInstanceUID)
            )
            self.assertEqual(
                str(common_reference.ReferencedInstanceSequence[0].ReferencedSOPInstanceUID),
                str(levels[0].SOPInstanceUID),
            )

    def test_new_functional_group_mutations_remove_only_the_authored_group(self):
        from bench.negative_bench.mutations import generate_case
        from bench.negative_bench.build_core_profile_controls import build_controls
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build_controls(REPO_ROOT / "bench/negative_bench/controls-v2", root / "controls")
            for name, source, expected_group in (
                ("remove_wsi_frame_type", root / "controls/CP01-EVRLE-4F-ANISO.dcm", "WholeSlideMicroscopyImageFrameTypeSequence"),
                ("remove_derivation_image", root / "controls/CP12-PYRAMID-2L/level-0.dcm", "DerivationImageSequence"),
            ):
                output = root / name
                output.mkdir()
                case = {"mutation_name": name, "deterministic_seed": name, "mutation_parameters": {}}
                paths, _ = generate_case(case, source, output)
                target = pydicom.dcmread(paths[-1])
                self.assertNotIn(expected_group, target.SharedFunctionalGroupsSequence[0])
                self.assertIn("PixelMeasuresSequence", target.SharedFunctionalGroupsSequence[0])
                self.assertEqual(len(paths), 2 if name == "remove_derivation_image" else 1)

    def test_manifest_inventory_rebuild_uses_the_current_case_specification(self):
        from bench.negative_bench.build_core_profile_manifest import build_manifest
        source = REPO_ROOT / "bench/negative_bench/manifest-v4.json"
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "manifest.json"
            rebuilt = build_manifest(source, REPO_ROOT / "bench/negative_bench/controls-v4", output)
            self.assertEqual(rebuilt, json.loads(source.read_text()))
            self.assertEqual(json.loads(output.read_text()), rebuilt)

    def test_amended_challenge_generates_derived_multifile_case(self):
        from bench.negative_bench.generate import generate_challenge
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "challenge"
            generate_challenge(REPO_ROOT / "bench/negative_bench/manifest-v4.json", output)
            paths = sorted((output / "cases/CP-CF-011/input").glob("*.dcm"))
            self.assertEqual(len(paths), 2)
            self.assertNotIn("DerivationImageSequence", pydicom.dcmread(paths[1]).SharedFunctionalGroupsSequence[0])

    def test_current_generator_materializes_multifile_controls(self):
        from bench.negative_bench.generate import _materialize_controls

        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "controls"
            destination.mkdir()
            materialized = _materialize_controls(
                MANIFEST_PATH,
                manifest,
                destination,
                manifest["resource_limits"]["max_file_bytes"],
            )

            self.assertEqual(len(materialized["CP12-PYRAMID-2L"]), 2)
            self.assertEqual(
                {path.name for path in materialized["CP12-PYRAMID-2L"]},
                {"level-0.dcm", "level-1.dcm"},
            )
            self.assertTrue(
                (destination / "CP12-PYRAMID-2L" / "level-0.dcm").is_file()
            )

class CoreEvidenceGateTests(unittest.TestCase):
    def test_gate_rejects_missing_negative_validator_and_unadjudicated_cascade(self):
        from bench.core_profile import CoreProfileError, validate_evidence_coverage
        import hashlib

        rule = {"rule_id": "rule", "rule_kind": "intrinsic", "check_names": ["check"]}
        catalog = {"dicom_edition": "2026c", "rules": [rule, {"rule_id": "external", "rule_kind": "independent_validator", "check_names": ["validator"]}]}
        profile = {"profile_id": "profile", "dicom_edition": "2026c", "required_external_validators": ["validator"], "requirements": [{"requirement_id": "R", "applicability": "required", "enforcement": {"rule_ids": ["rule"]}, "positive_control_ids": ["C"], "negative_case_ids": ["N"]}]}
        manifest = {"dicom_edition": "2026c", "core_profile": {"profile_id": "profile", "path": "profile.json"}, "rule_catalog": {"path": "catalog.json"}, "valid_controls": [{"control_id": "C"}], "evaluation_cases": [{"case_id": "N", "domain": "pixel", "expected_failing_rules": ["rule"]}]}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            expected = root / "expected-results"
            expected.mkdir()
            (expected / "profile.json").write_text(json.dumps(profile))
            (expected / "catalog.json").write_text(json.dumps(catalog))
            for identifier in ("C", "N"):
                input_path = root / ("controls/C.dcm" if identifier == "C" else "cases/N/input/instance.dcm")
                input_path.parent.mkdir(parents=True)
                input_path.write_bytes(identifier.encode())
                status = "passed" if identifier == "C" else "failed"
                report = {"status": status, "profile": {"id": "profile", "sha256": hashlib.sha256((expected / "profile.json").read_bytes()).hexdigest()}, "catalog": {"sha256": hashlib.sha256((expected / "catalog.json").read_bytes()).hexdigest()}, "slide": {"instances": [{"path": str(input_path), "sha256": hashlib.sha256(input_path.read_bytes()).hexdigest()}]}, "findings": [{"rule_id": "rule", "check_name": "check", "path": str(input_path), "status": status, "primary_domain": "pixel", "catalog_status": "mapped"}, {"rule_id": "external", "check_name": "validator", "status": "passed", "catalog_status": "mapped"}]}
                path = root / "observed-results" / identifier / "workbench/workbench-report.json"
                path.parent.mkdir(parents=True)
                path.write_text(json.dumps(report))
            baseline = validate_evidence_coverage(profile, catalog, manifest, root)
            self.assertEqual(baseline["controls_passing"], 1)
            path = root / "observed-results/N/workbench/workbench-report.json"
            report = json.loads(path.read_text())
            for defect in ("missing_validator", "execution_error", "unmapped", "cascade", "input_digest"):
                broken = copy.deepcopy(report)
                if defect == "missing_validator":
                    broken["findings"].pop()
                elif defect == "execution_error":
                    broken["findings"][-1]["status"] = "execution_error"
                elif defect == "unmapped":
                    broken["findings"][-1]["catalog_status"] = "unmapped"
                elif defect == "cascade":
                    broken["findings"].append({"rule_id": "unexpected", "check_name": "unexpected", "rule_kind": "intrinsic", "status": "failed"})
                else:
                    broken["slide"]["instances"][0]["sha256"] = "0" * 64
                path.write_text(json.dumps(broken))
                with self.subTest(defect=defect), self.assertRaises(CoreProfileError):
                    validate_evidence_coverage(profile, catalog, manifest, root)


if __name__ == "__main__":
    unittest.main()

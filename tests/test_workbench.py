import contextlib
import io
import json
import re
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = REPO_ROOT / "rules" / "wsi-dicom-bench-rules-2026c-v4.json"
PROFILE_PATH = REPO_ROOT / "rules" / "wsi-dicom-core-profile-2026c-v2.json"


class WorkbenchCatalogTests(unittest.TestCase):
    def test_process_errors_are_not_validator_defect_detections(self):
        from bench.workbench.report import map_findings, validator_comparison

        catalog = json.loads((REPO_ROOT / "rules/wsi-dicom-bench-rules-2026c-v4.json").read_text())
        findings = map_findings({"checks": [{
            "name": "dciodvfy", "status": "failed",
            "execution": {"failure": "timeout", "return_code": None, "elapsed_millis": 1000},
        }]}, catalog)
        self.assertEqual(findings[0]["status"], "execution_error")
        self.assertEqual(validator_comparison(findings)["external"]["dciodvfy"], "execution_error")

    def test_contract_rejects_missing_instance_checks_and_wrong_profile(self):
        from bench.workbench.contract import validation_contract_errors
        from bench.workbench.catalog import load_catalog

        catalog = load_catalog(CATALOG_PATH)
        validation = {"profile": "core-2026c", "files": ["one.dcm", "two.dcm"], "checks": [
            {"name": name, "path": "one.dcm", "status": "passed"}
            for rule in catalog["rules"] if rule["rule_kind"] == "intrinsic"
            for name in rule["check_names"]
        ]}
        errors = validation_contract_errors(validation, catalog, True)
        self.assertTrue(any("two.dcm" in error for error in errors))
        validation["profile"] = "general"
        self.assertTrue(any("profile" in error for error in validation_contract_errors(validation, catalog, True)))

    def test_default_catalog_is_the_current_v4_catalog(self):
        from bench.workbench.cli import DEFAULT_CATALOG

        self.assertEqual(DEFAULT_CATALOG, CATALOG_PATH)

    def test_catalog_has_unique_versioned_rules_and_maps_every_emitted_check(self):
        from bench.workbench.catalog import catalog_index, load_catalog

        catalog = load_catalog(CATALOG_PATH)
        index = catalog_index(catalog)

        expected_checks = {
            "intrinsic-pixel-structure",
            "intrinsic-wsi-dicom-2026c-icc-profile",
            "intrinsic-wsi-dicom-2026c-monochrome-presentation",
            "intrinsic-wsi-dicom-2026c-lossy-history",
            "intrinsic-wsi-dicom-2026c-specimen-identity",
            "intrinsic-wsi-dicom-2026c-specimen-uid-set",
            "intrinsic-wsi-dicom-2026c-dimension-order",
            "intrinsic-wsi-dicom-2026c-tile-geometry",
            "intrinsic-wsi-dicom-2026c-identity-set",
            "intrinsic-wsi-dicom-2026c-image-metadata",
            "intrinsic-wsi-dicom-2026c-core-image-profile",
            "intrinsic-wsi-dicom-2026c-slide-coordinate-system",
            "intrinsic-wsi-dicom-2026c-optical-path-structure",
            "intrinsic-wsi-dicom-2026c-specimen-container",
            "intrinsic-wsi-dicom-2026c-clinical-identity-set",
            "intrinsic-wsi-dicom-2026c-pyramid-geometry",
            "intrinsic-wsi-dicom-2026c-source-relationship",
            "pixel-decode",
            "pixel-djpeg",
            "pixel-opj-decompress",
            "pixel-htj2k",
            "dciodvfy",
            "dcentvfy",
            "validate_iods",
            "dcmvalidate",
        }
        self.assertEqual(set(index), expected_checks)
        self.assertEqual(catalog["catalog_version"], "wsi-dicom-bench-rules-2026c-v4")
        self.assertEqual(
            len({rule["rule_id"] for rule in catalog["rules"]}),
            len(catalog["rules"]),
        )
        for rule in catalog["rules"]:
            self.assertIn(
                rule["primary_domain"],
                {"conformance", "pixel", "geometry", "color", "identity"},
            )
            self.assertIn(rule["clinical_severity"], {"none", "minor", "major", "critical"})
            self.assertIn(rule["operational_severity"], {"minor", "major", "critical"})
            self.assertTrue(rule["likely_fix_owners"])
            self.assertTrue(rule["citations"])
            for citation in rule["citations"]:
                self.assertEqual(citation["edition"], "2026c")
                self.assertTrue(citation["url"].startswith("https://dicom.nema.org/"))

    def test_catalog_check_names_match_the_validation_implementation(self):
        from bench.workbench.catalog import catalog_index, load_catalog

        conformance_root = REPO_ROOT / "src/validation/wsi_conformance"
        conformance = "\n".join(
            path.read_text() for path in sorted(conformance_root.glob("*.rs"))
        )
        pixel_structure = (REPO_ROOT / "src/validation/pixel_structure.rs").read_text()
        pixel_decode = (REPO_ROOT / "src/validation/pixel_decode.rs").read_text()
        orchestration = (REPO_ROOT / "src/validation.rs").read_text()
        emitted = set(
            re.findall(r'const \w+_RULE: &str\s*=\s*"([^"]+)"', conformance)
        )
        emitted.add("intrinsic-pixel-structure")
        emitted.update(re.findall(r'check_name: "([^"]+)"', pixel_decode))
        emitted.update(
            re.findall(r'(?:failed_check|skipped_check)\(\s*"([^"]+)"', pixel_decode)
        )
        emitted.update(
            re.findall(
                r'name: "(dciodvfy|dcentvfy|validate_iods|dcmvalidate)"',
                orchestration,
            )
        )

        self.assertIn('name: "intrinsic-pixel-structure"', pixel_structure)
        self.assertEqual(set(catalog_index(load_catalog(CATALOG_PATH))), emitted)

    def test_validation_checks_become_cataloged_findings_and_domain_summaries(self):
        from bench.workbench.catalog import load_catalog
        from bench.workbench.report import (
            map_findings,
            summarize_domains,
            validator_comparison,
        )

        validation = {
            "input": "dicom",
            "files": ["dicom/slide.dcm"],
            "checks": [
                {
                    "name": "intrinsic-pixel-structure",
                    "path": "dicom/slide.dcm",
                    "status": "passed",
                    "command": [],
                    "message": "valid",
                    "stdout": "",
                    "stderr": "",
                },
                {
                    "name": "intrinsic-wsi-dicom-2026c-dimension-order",
                    "path": "dicom/slide.dcm",
                    "status": "failed",
                    "command": [],
                    "message": "row and column indices are reversed",
                    "stdout": "",
                    "stderr": "",
                },
                {
                    "name": "dciodvfy",
                    "path": "dicom/slide.dcm",
                    "status": "passed",
                    "command": ["dciodvfy", "slide.dcm"],
                    "message": "passed",
                    "stdout": "",
                    "stderr": "",
                },
                {
                    "name": "dcentvfy",
                    "path": None,
                    "status": "skipped",
                    "command": [],
                    "message": "not installed",
                    "stdout": "",
                    "stderr": "",
                },
            ],
        }

        findings = map_findings(validation, load_catalog(CATALOG_PATH))
        domains = summarize_domains(findings)
        comparison = validator_comparison(findings)

        self.assertTrue(all(finding["catalog_status"] == "mapped" for finding in findings))
        self.assertEqual(domains["geometry"]["status"], "failed")
        self.assertEqual(domains["pixel"]["status"], "passed")
        self.assertEqual(domains["color"]["status"], "not_evaluated")
        self.assertEqual(comparison["external"]["dciodvfy"], "passed")
        self.assertEqual(comparison["external"]["dcentvfy"], "skipped")
        self.assertEqual(comparison["intrinsic"], "failed")
        self.assertTrue(comparison["disagreement"])


class WorkbenchDicomInspectionTests(unittest.TestCase):
    def test_inspection_unifies_geometry_color_and_identity_facts(self):
        from pydicom.dataset import Dataset, FileDataset, FileMetaDataset
        from pydicom.sequence import Sequence
        from pydicom.uid import ExplicitVRLittleEndian

        from bench.workbench.report import inspect_dicom_set

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "slide.dcm"
            meta = FileMetaDataset()
            meta.TransferSyntaxUID = ExplicitVRLittleEndian
            dataset = FileDataset(path, {}, file_meta=meta, preamble=b"\0" * 128)
            dataset.SOPClassUID = "1.2.840.10008.5.1.4.1.1.77.1.6"
            dataset.SOPInstanceUID = "1.2.3.4.1"
            dataset.StudyInstanceUID = "1.2.3.1"
            dataset.SeriesInstanceUID = "1.2.3.2"
            dataset.FrameOfReferenceUID = "1.2.3.3"
            dataset.ContainerIdentifier = "SLIDE-001"
            dataset.Rows = 512
            dataset.Columns = 256
            dataset.NumberOfFrames = "6"
            dataset.TotalPixelMatrixRows = 1024
            dataset.TotalPixelMatrixColumns = 768
            dataset.PhotometricInterpretation = "RGB"
            pixel_measures = Dataset()
            pixel_measures.PixelSpacing = [0.0005, 0.00025]
            shared = Dataset()
            shared.PixelMeasuresSequence = Sequence([pixel_measures])
            dataset.SharedFunctionalGroupsSequence = Sequence([shared])
            optical_path = Dataset()
            optical_path.OpticalPathIdentifier = "1"
            optical_path.ICCProfile = b"test-icc"
            dataset.OpticalPathSequence = Sequence([optical_path])
            specimen = Dataset()
            specimen.SpecimenIdentifier = "SPEC-001"
            specimen.SpecimenUID = "1.2.3.5"
            dataset.SpecimenDescriptionSequence = Sequence([specimen])
            dataset.save_as(path, enforce_file_format=True)

            inspection = inspect_dicom_set([path])

        self.assertEqual(inspection["slide_id"], "SLIDE-001")
        self.assertEqual(inspection["instance_count"], 1)
        instance = inspection["instances"][0]
        self.assertEqual(instance["geometry"]["pixel_spacing_mm"], [0.0005, 0.00025])
        self.assertEqual(instance["geometry"]["expected_full_frame_count"], 6)
        self.assertEqual(instance["color"]["photometric_interpretation"], "RGB")
        self.assertEqual(len(instance["color"]["icc_profile_sha256"]), 1)
        self.assertEqual(instance["identity"]["specimens"][0]["identifier"], "SPEC-001")
        self.assertEqual(instance["identity"]["specimens"][0]["uid"], "1.2.3.5")

    def test_inspection_records_conflicting_slide_scopes_without_aborting(self):
        from pydicom.dataset import FileDataset, FileMetaDataset
        from pydicom.uid import ExplicitVRLittleEndian

        from bench.workbench.report import inspect_dicom_set

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            paths = []
            for index, container in enumerate(("SLIDE-A", "SLIDE-B"), start=1):
                path = root / f"slide-{index}.dcm"
                meta = FileMetaDataset()
                meta.TransferSyntaxUID = ExplicitVRLittleEndian
                dataset = FileDataset(path, {}, file_meta=meta, preamble=b"\0" * 128)
                dataset.SOPClassUID = "1.2.840.10008.5.1.4.1.1.77.1.6"
                dataset.SOPInstanceUID = f"1.2.3.4.{index}"
                dataset.FrameOfReferenceUID = "1.2.3.5"
                dataset.ContainerIdentifier = container
                dataset.save_as(path, enforce_file_format=True)
                paths.append(path)

            inspection = inspect_dicom_set(paths)

        self.assertEqual(inspection["slide_identity_status"], "conflicting")
        self.assertEqual(inspection["slide_id"], "multiple-slide-identities")
        self.assertEqual(len(inspection["slide_scopes"]), 2)


class WorkbenchCliTests(unittest.TestCase):
    def test_core_profile_catalog_requires_profile_argument(self):
        from bench.workbench.cli import WorkbenchError, parse_args, run_workbench

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dicom_path = root / "slide.dcm"
            binary = root / "wsi-dicom"
            dicom_path.write_bytes(b"DICOM")
            binary.write_bytes(b"binary")
            args = parse_args(
                [
                    str(dicom_path),
                    "--output",
                    str(root / "evidence"),
                    "--wsi-dicom",
                    str(binary),
                    "--catalog",
                    str(CATALOG_PATH),
                ]
            )

            with self.assertRaisesRegex(WorkbenchError, "--profile is required"):
                run_workbench(args)

    def test_core_catalog_rejects_a_different_profile_version(self):
        from bench.workbench.cli import WorkbenchError, parse_args, run_workbench
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dicom = root / "one.dcm"
            dicom.write_bytes(b"placeholder")
            binary = root / "binary"
            binary.write_bytes(b"placeholder")
            incompatible = json.loads(PROFILE_PATH.read_text())
            incompatible["profile_id"] = "retired-profile"
            incompatible["profile_version"] = "1"
            profile = root / "profile.json"
            profile.write_text(json.dumps(incompatible))
            args = parse_args([str(dicom), "--output", str(root / "out"), "--wsi-dicom", str(binary), "--catalog", str(CATALOG_PATH), "--profile", str(profile)])
            with self.assertRaisesRegex(WorkbenchError, "profile version"):
                run_workbench(args)

    def test_complete_entry_point_writes_one_unified_evidence_bundle(self):
        from pydicom.dataset import FileDataset, FileMetaDataset
        from pydicom.uid import ExplicitVRLittleEndian

        from bench.workbench.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dicom_path = root / "slide.dcm"
            meta = FileMetaDataset()
            meta.TransferSyntaxUID = ExplicitVRLittleEndian
            dataset = FileDataset(dicom_path, {}, file_meta=meta, preamble=b"\0" * 128)
            dataset.SOPClassUID = "1.2.840.10008.5.1.4.1.1.77.1.6"
            dataset.SOPInstanceUID = "1.2.3.4.1"
            dataset.StudyInstanceUID = "1.2.3.1"
            dataset.SeriesInstanceUID = "1.2.3.2"
            dataset.FrameOfReferenceUID = "1.2.3.3"
            dataset.ContainerIdentifier = "SLIDE-001"
            dataset.Rows = 1
            dataset.Columns = 1
            dataset.NumberOfFrames = "1"
            dataset.TotalPixelMatrixRows = 1
            dataset.TotalPixelMatrixColumns = 1
            dataset.PhotometricInterpretation = "MONOCHROME2"
            dataset.save_as(dicom_path, enforce_file_format=True)

            fake = root / "fake-wsi-dicom"
            checks = [
                "intrinsic-wsi-dicom-2026c-tile-geometry",
                "intrinsic-wsi-dicom-2026c-identity-set",
                "intrinsic-wsi-dicom-2026c-image-metadata",
                "intrinsic-pixel-structure",
                "intrinsic-wsi-dicom-2026c-icc-profile",
                "intrinsic-wsi-dicom-2026c-monochrome-presentation",
                "intrinsic-wsi-dicom-2026c-lossy-history",
                "intrinsic-wsi-dicom-2026c-specimen-identity",
                "intrinsic-wsi-dicom-2026c-specimen-uid-set",
                "intrinsic-wsi-dicom-2026c-dimension-order",
                "intrinsic-wsi-dicom-2026c-core-image-profile",
                "intrinsic-wsi-dicom-2026c-slide-coordinate-system",
                "intrinsic-wsi-dicom-2026c-optical-path-structure",
                "intrinsic-wsi-dicom-2026c-specimen-container",
                "intrinsic-wsi-dicom-2026c-clinical-identity-set",
                "pixel-decode",
                "dciodvfy",
                "dcentvfy",
                "validate_iods",
            ]
            fake.write_text(
                "#!/usr/bin/env python3\n"
                "import json, pathlib, sys\n"
                "if '--version' in sys.argv:\n"
                "    print('wsi-dicom 0.7.4')\n"
                "elif sys.argv[1] == 'doctor':\n"
                "    print(json.dumps({'tools': []}))\n"
                "elif sys.argv[1] == 'validate':\n"
                f"    names = {checks!r}\n"
                f"    path = {str(dicom_path)!r}\n"
                "    rows = [{'name': name, 'path': path, 'status': 'passed', "
                "'command': [], 'message': 'passed', 'stdout': '', 'stderr': ''} "
                "for name in names]\n"
                "    print(json.dumps({'profile': 'core-2026c', 'input': path, 'files': [path], 'checks': rows}))\n"
                "else:\n"
                "    raise SystemExit(2)\n",
                encoding="utf-8",
            )
            fake.chmod(fake.stat().st_mode | 0o111)
            output = root / "evidence"

            with contextlib.redirect_stdout(io.StringIO()):
                returncode = main(
                    [
                        str(dicom_path),
                        "--output",
                        str(output),
                        "--wsi-dicom",
                        str(fake),
                        "--catalog",
                        str(CATALOG_PATH),
                        "--profile",
                        str(PROFILE_PATH),
                    ]
                )

            report = json.loads((output / "workbench-report.json").read_text())
            summary = (output / "summary.md").read_text()

            self.assertTrue((output / "doctor.json").is_file())
            self.assertTrue((output / "validation.json").is_file())

        self.assertEqual(returncode, 0)
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["slide"]["slide_id"], "SLIDE-001")
        self.assertEqual(report["domains"]["identity"]["status"], "passed")
        self.assertFalse(report["validator_comparison"]["disagreement"])
        self.assertEqual(report["catalog"]["version"], "wsi-dicom-bench-rules-2026c-v4")
        self.assertEqual(report["profile"]["id"], "wsi-dicom-core-profile-2026c-v2")
        self.assertEqual(report["profile"]["version"], "2")
        self.assertEqual(len(report["profile"]["sha256"]), 64)
        self.assertEqual(report["software"]["wsi_dicom"]["version"], "wsi-dicom 0.7.4")
        self.assertEqual(len(report["software"]["wsi_dicom"]["sha256"]), 64)
        self.assertIn("SLIDE-001", summary)
        self.assertIn("Independent validator comparison", summary)


if __name__ == "__main__":
    unittest.main()

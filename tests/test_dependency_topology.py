import json
import re
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from collections import Counter
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
GIT_REVISION = re.compile(r"[?&]rev=([0-9a-f]{40})(?:$|[&#])")


def load_toml(relative_path):
    return tomllib.loads((REPO_ROOT / relative_path).read_text(encoding="utf-8"))


def dependency_version(manifest, name):
    dependency = manifest["dependencies"][name]
    requirement = dependency if isinstance(dependency, str) else dependency["version"]
    requirement = requirement.lstrip("=~^<>")
    return tuple(map(int, requirement.split(".")))


def cargo_package(manifest_path, package_name):
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            str(REPO_ROOT / manifest_path),
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(f"cargo metadata failed: {result.stderr}")
    packages = [
        package
        for package in json.loads(result.stdout)["packages"]
        if package["name"] == package_name
    ]
    if len(packages) != 1:
        raise AssertionError(f"expected one {package_name} package, found {len(packages)}")
    return packages[0]


class DependencyTopologyTests(unittest.TestCase):
    def test_ci_separates_source_topology_from_registry_package_gate(self):
        workflow = (REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )
        publish_workflow = (
            REPO_ROOT / ".github" / "workflows" / "publish.yml"
        ).read_text(encoding="utf-8")
        self.assertIn("dependency-topology:", workflow)
        self.assertIn("cargo metadata --locked --format-version 1", workflow)
        self.assertIn("cargo package --list", workflow)
        self.assertNotIn("cargo package --locked", workflow)
        self.assertIn("cargo package --locked", publish_workflow)
        self.assertIn(
            "python -m unittest discover -s tests -p 'test_dependency_topology.py'",
            workflow,
        )

    def test_ci_runs_a_workspace_wide_rustsec_scan(self):
        workflow = (REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn(
            "cargo install cargo-audit --locked --version 0.22.1",
            workflow,
        )
        self.assertIn(
            "cargo audit --file Cargo.lock "
            "--ignore RUSTSEC-2021-0153 --ignore RUSTSEC-2024-0436",
            workflow,
        )

    def test_direct_project_dependencies_use_reproducible_sources(self):
        package = cargo_package("Cargo.toml", "wsi-dicom")
        project_dependencies = [
            dependency
            for dependency in package["dependencies"]
            if dependency["name"].startswith("j2k")
            or dependency["name"] in {"wsi-rs", "wsi-dicom-annotations"}
        ]
        self.assertGreater(len(project_dependencies), 0)
        for dependency in project_dependencies:
            self.assertTrue(dependency["req"], dependency["name"])
            source = dependency["source"]
            if source == REGISTRY_SOURCE:
                continue
            self.assertIsNotNone(source, dependency["name"])
            self.assertTrue(source.startswith("git+https://"), dependency["name"])
            self.assertIsNotNone(GIT_REVISION.search(source), dependency["name"])

    def test_workspace_metadata_is_reproducible_without_siblings(self):
        with tempfile.TemporaryDirectory() as directory:
            clean_root = Path(directory) / "wsi-dicom"
            shutil.copytree(
                REPO_ROOT,
                clean_root,
                ignore=shutil.ignore_patterns(".git", ".codex", "target", ".venv*"),
            )
            result = subprocess.run(
                ["cargo", "metadata", "--locked", "--format-version", "1"],
                cwd=clean_root,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_fuzz_cargo_metadata_matches_root_first_party_sources(self):
        root_package = cargo_package("Cargo.toml", "wsi-dicom")
        root_dependencies = {
            dependency["name"]: dependency for dependency in root_package["dependencies"]
        }
        fuzz_package = cargo_package("fuzz/Cargo.toml", "wsi-dicom-fuzz")
        fuzz_dependencies = {
            dependency["name"]: dependency for dependency in fuzz_package["dependencies"]
        }
        self.assertEqual(
            fuzz_dependencies["wsi-rs"]["source"],
            root_dependencies["wsi-rs"]["source"],
        )
        self.assertEqual(
            fuzz_dependencies["wsi-rs"]["req"],
            root_dependencies["wsi-rs"]["req"],
        )
        self.assertIsNone(fuzz_dependencies["wsi-dicom"]["source"])
        self.assertEqual(
            Path(fuzz_dependencies["wsi-dicom"]["path"]).resolve(),
            REPO_ROOT.resolve(),
        )

    def test_lockfiles_match_the_manifest_j2k_family(self):
        minimum_version = dependency_version(load_toml("Cargo.toml"), "j2k")
        minimum_wsi_version = dependency_version(load_toml("Cargo.toml"), "wsi-rs")
        for relative_path in ("Cargo.lock", "fuzz/Cargo.lock"):
            with self.subTest(lockfile=relative_path):
                packages = load_toml(relative_path)["package"]
                j2k_packages = [
                    package
                    for package in packages
                    if package["name"].startswith("j2k")
                ]
                self.assertGreater(len(j2k_packages), 0)
                counts = Counter(package["name"] for package in j2k_packages)
                self.assertTrue(
                    all(count == 1 for count in counts.values()),
                    f"duplicate j2k package identities in {relative_path}: {counts}",
                )
                versions = {package["version"] for package in j2k_packages}
                self.assertEqual(
                    len(versions),
                    1,
                    f"mixed j2k release families in {relative_path}: {versions}",
                )
                major, minor, patch = map(int, next(iter(versions)).split("."))
                self.assertEqual((major, minor), minimum_version[:2])
                self.assertGreaterEqual((major, minor, patch), minimum_version)
                sources = {package.get("source") for package in j2k_packages}
                self.assertEqual(
                    len(sources),
                    1,
                    f"mixed j2k source identities in {relative_path}: {sources}",
                )
                source = next(iter(sources))
                if source == REGISTRY_SOURCE:
                    for package in j2k_packages:
                        self.assertRegex(package.get("checksum", ""), r"^[0-9a-f]{64}$")
                else:
                    self.assertIsNotNone(source)
                    match = GIT_REVISION.search(source)
                    self.assertIsNotNone(match, source)
                    self.assertTrue(source.endswith(f"#{match.group(1)}"), source)
                self.assertFalse(
                    any(package["name"].startswith("signinum") for package in packages)
                )
                wsi_packages = [
                    package for package in packages if package["name"] == "wsi-rs"
                ]
                self.assertEqual(len(wsi_packages), 1)
                locked_wsi_version = tuple(
                    map(int, wsi_packages[0]["version"].split("."))
                )
                self.assertEqual(locked_wsi_version[:2], minimum_wsi_version[:2])
                self.assertGreaterEqual(locked_wsi_version, minimum_wsi_version)

if __name__ == "__main__":
    unittest.main()

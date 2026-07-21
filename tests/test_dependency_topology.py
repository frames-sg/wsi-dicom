import json
import subprocess
import tomllib
import unittest
from collections import Counter
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
def load_toml(relative_path):
    return tomllib.loads((REPO_ROOT / relative_path).read_text(encoding="utf-8"))


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
    def test_cargo_metadata_uses_registry_codec_dependencies(self):
        package = cargo_package("Cargo.toml", "wsi-dicom")
        codecs = [
            dependency
            for dependency in package["dependencies"]
            if dependency["name"].startswith("j2k") or dependency["name"] == "wsi-rs"
        ]
        self.assertGreater(len(codecs), 0)
        for dependency in codecs:
            self.assertTrue(dependency["req"], dependency["name"])
            self.assertEqual(dependency["source"], REGISTRY_SOURCE, dependency["name"])

    def test_fuzz_cargo_metadata_keeps_only_the_local_fuzz_target_path(self):
        package = cargo_package("fuzz/Cargo.toml", "wsi-dicom-fuzz")
        dependencies = {dependency["name"]: dependency for dependency in package["dependencies"]}
        self.assertEqual(dependencies["wsi-rs"]["source"], REGISTRY_SOURCE)
        self.assertIsNone(dependencies["wsi-dicom"]["source"])
        self.assertEqual(
            Path(dependencies["wsi-dicom"]["path"]).resolve(),
            REPO_ROOT.resolve(),
        )

    def test_lockfiles_pin_one_checksummed_registry_j2k_family(self):
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
                self.assertEqual((major, minor), (0, 7))
                self.assertGreaterEqual(patch, 3)
                for package in j2k_packages:
                    self.assertEqual(package.get("source"), REGISTRY_SOURCE, package["name"])
                    self.assertRegex(package.get("checksum", ""), r"^[0-9a-f]{64}$")
                self.assertFalse(
                    any(package["name"].startswith("signinum") for package in packages)
                )

if __name__ == "__main__":
    unittest.main()

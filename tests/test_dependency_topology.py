import tomllib
import unittest
from collections import Counter
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
J2K_VERSION = "=0.6.2"
DIRECT_J2K_DEPENDENCIES = {
    "j2k",
    "j2k-core",
    "j2k-cuda",
    "j2k-jpeg",
    "j2k-jpeg-metal",
    "j2k-metal",
    "j2k-transcode",
    "j2k-transcode-metal",
}


def load_toml(relative_path):
    return tomllib.loads((REPO_ROOT / relative_path).read_text(encoding="utf-8"))


def dependency_version(specification):
    if isinstance(specification, str):
        return specification
    return specification.get("version")


class DependencyTopologyTests(unittest.TestCase):
    def test_root_manifest_uses_registry_j2k_dependencies(self):
        manifest = load_toml("Cargo.toml")
        dependencies = manifest["dependencies"]
        direct_j2k = {
            name for name in dependencies if name.startswith("j2k")
        }
        self.assertEqual(direct_j2k, DIRECT_J2K_DEPENDENCIES)

        for name in sorted(direct_j2k):
            specification = dependencies[name]
            self.assertEqual(dependency_version(specification), J2K_VERSION, name)
            if isinstance(specification, dict):
                self.assertNotIn("path", specification, name)
                self.assertNotIn("git", specification, name)
                self.assertNotIn("registry", specification, name)

        self.assertNotIn("patch", manifest)

    def test_only_documented_wsi_path_bridge_remains(self):
        root_dependencies = load_toml("Cargo.toml")["dependencies"]
        root_paths = {
            name: specification["path"]
            for name, specification in root_dependencies.items()
            if isinstance(specification, dict) and "path" in specification
        }
        self.assertEqual(root_paths, {"wsi-rs": "../wsi-rs"})
        self.assertEqual(dependency_version(root_dependencies["wsi-rs"]), "=0.5.0")

        fuzz_dependencies = load_toml("fuzz/Cargo.toml")["dependencies"]
        fuzz_paths = {
            name: specification["path"]
            for name, specification in fuzz_dependencies.items()
            if isinstance(specification, dict) and "path" in specification
        }
        self.assertEqual(
            fuzz_paths,
            {"wsi-rs": "../../wsi-rs", "wsi-dicom": ".."},
        )
        self.assertEqual(dependency_version(fuzz_dependencies["wsi-rs"]), "=0.5.0")

    def test_fuzz_manifest_does_not_patch_codec_crates(self):
        manifest = load_toml("fuzz/Cargo.toml")
        source = (REPO_ROOT / "fuzz/Cargo.toml").read_text(encoding="utf-8")
        self.assertNotIn("patch", manifest)
        self.assertNotIn("../j2k/", source)

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
                for package in j2k_packages:
                    self.assertEqual(package["version"], "0.6.2", package["name"])
                    self.assertEqual(package.get("source"), REGISTRY_SOURCE, package["name"])
                    self.assertRegex(package.get("checksum", ""), r"^[0-9a-f]{64}$")
                self.assertFalse(
                    any(package["name"].startswith("signinum") for package in packages)
                )

    def test_ci_runs_dependency_policy_without_cargo(self):
        workflow = (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn(
            "python -m unittest discover -s tests -p 'test_dependency_topology.py'",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()

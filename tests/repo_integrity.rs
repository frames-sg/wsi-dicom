use std::{fs, path::Path};

fn crate_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn source_does_not_import_signinum_j2k_native_directly() {
    let mut offenders = Vec::new();
    for path in rust_sources(&crate_root().join("src")) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        if source.contains("signinum_j2k_native") {
            offenders.push(relative_path(&path));
        }
    }

    assert!(
        offenders.is_empty(),
        "wsi-dicom must call signinum-j2k encode APIs instead of importing signinum_j2k_native:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn lockfile_has_no_duplicate_signinum_package_sources() {
    let lockfile = fs::read_to_string(crate_root().join("Cargo.lock")).expect("read Cargo.lock");
    let mut duplicates = Vec::new();

    for package in [
        "signinum-core",
        "signinum-j2k",
        "signinum-j2k-metal",
        "signinum-j2k-native",
        "signinum-jpeg",
        "signinum-tilecodec",
    ] {
        let count = lockfile
            .lines()
            .filter(|line| line.trim() == format!("name = \"{package}\""))
            .count();
        if count > 1 {
            duplicates.push(format!("{package}: {count} entries"));
        }
    }

    assert!(
        duplicates.is_empty(),
        "Cargo.lock must not contain duplicate signinum package identities:\n{}",
        duplicates.join("\n")
    );
}

fn rust_sources(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    visit_rust_sources(root, &mut out);
    out
}

fn visit_rust_sources(path: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display())) {
        let entry =
            entry.unwrap_or_else(|err| panic!("read dir entry in {}: {err}", path.display()));
        let path = entry.path();
        if path.is_dir() {
            visit_rust_sources(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn relative_path(path: &Path) -> String {
    path.strip_prefix(crate_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

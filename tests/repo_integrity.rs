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
fn lib_rs_stays_facade_sized() {
    let lib = crate_root().join("src/lib.rs");
    let source =
        fs::read_to_string(&lib).unwrap_or_else(|err| panic!("read {}: {err}", lib.display()));
    let nonblank_lines = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    assert!(
        nonblank_lines <= 80,
        "src/lib.rs must stay facade-sized; found {nonblank_lines} nonblank lines"
    );
}

#[test]
fn export_rs_line_budget_ratchets_down() {
    let export = crate_root().join("src/export.rs");
    let source = fs::read_to_string(&export)
        .unwrap_or_else(|err| panic!("read {}: {err}", export.display()));
    let nonblank_lines = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    assert!(
        nonblank_lines <= 10_900,
        "src/export.rs must keep shrinking as route/default/passthrough modules take ownership; found {nonblank_lines} nonblank lines"
    );
}

#[test]
fn cli_report_module_does_not_call_export_internals_directly() {
    let path = crate_root().join("src/cli_report.rs");
    let source =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));

    for forbidden in [
        "export_dicom(",
        "profile_dicom_routes(",
        "profile_dicom_route_coverage(",
        "profile_dicom_route_corpus_coverage(",
        "DefaultTransferSyntaxRequest",
    ] {
        assert!(
            !source.contains(forbidden),
            "src/cli_report.rs must format reports only, but contains `{forbidden}`"
        );
    }
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
        "signinum-jpeg-metal",
        "signinum-tilecodec",
    ] {
        let count = lockfile_package_name_count(&lockfile, package);
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

fn lockfile_package_name_count(lockfile: &str, package: &str) -> usize {
    let package_name = format!("name = \"{package}\"");
    let mut in_package = false;
    let mut count = 0usize;
    for line in lockfile.lines().map(str::trim) {
        if line.starts_with("[[") {
            in_package = line == "[[package]]";
            continue;
        }
        if in_package && line == package_name {
            count += 1;
        }
    }
    count
}

#[test]
fn jpeg_dependencies_are_limited_to_signinum_crates() {
    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");
    let lockfile = fs::read_to_string(crate_root().join("Cargo.lock")).expect("read Cargo.lock");
    for dependency in ["jpeg-encoder", "turbojpeg", "mozjpeg", "zune-jpeg"] {
        assert!(
            !manifest.contains(dependency),
            "wsi-dicom must use signinum JPEG APIs, not direct {dependency} dependencies"
        );
        assert!(
            !lockfile.contains(&format!("name = \"{dependency}\"")),
            "Cargo.lock includes non-signinum JPEG dependency {dependency}"
        );
    }
}

#[test]
fn metal_feature_enables_statumen_metal_decode_plumbing() {
    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");
    assert!(
        manifest.contains("\"statumen/metal\""),
        "wsi-dicom's metal feature must enable statumen/metal for input decode plumbing"
    );
    assert!(
        manifest.contains("\"dep:signinum-jpeg-metal\""),
        "wsi-dicom's metal feature must include signinum-jpeg-metal so statumen can decode JPEG WSI tiles on Metal"
    );
    assert!(
        manifest.contains("\"dep:metal\""),
        "wsi-dicom's metal feature must include metal so JPEG and J2K decode sessions share one device"
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

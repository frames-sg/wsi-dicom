use std::{fs, path::Path};

fn crate_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn in_source_checkout() -> bool {
    crate_root().join(".git").exists()
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
        nonblank_lines <= 3_300,
        "src/export.rs must keep shrinking as route/default/passthrough modules take ownership; found {nonblank_lines} nonblank lines"
    );

    let metal_row_batch = crate_root().join("src/export/metal_row_batch.rs");
    let source = fs::read_to_string(&metal_row_batch)
        .unwrap_or_else(|err| panic!("read {}: {err}", metal_row_batch.display()));
    let nonblank_lines = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    assert!(
        nonblank_lines <= 650,
        "src/export/metal_row_batch.rs must stay focused on row-batch orchestration; found {nonblank_lines} nonblank lines"
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

#[test]
fn lockfile_pins_single_metal_version() {
    let lockfile = fs::read_to_string(crate_root().join("Cargo.lock")).expect("read Cargo.lock");

    assert_eq!(
        lockfile_package_name_count(&lockfile, "metal"),
        1,
        "Cargo.lock must contain exactly one metal package entry"
    );
    assert!(
        lockfile.contains("name = \"metal\"\nversion = \"0.33.0\""),
        "Cargo.lock must pin metal to 0.33.0"
    );
    assert!(
        !lockfile.contains("name = \"metal\"\nversion = \"0.31.0\""),
        "Cargo.lock must not retain metal 0.31.0"
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
fn release_build_uses_only_approved_local_metal_patches() {
    if !in_source_checkout() {
        return;
    }

    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");
    let workflow =
        fs::read_to_string(crate_root().join(".github/workflows/ci.yml")).expect("read CI");

    let expected_patches = [
        r#"statumen = { path = "vendor/metal-0.33-patches/statumen-0.3.1" }"#,
        r#"signinum-j2k-metal = { path = "vendor/metal-0.33-patches/signinum-j2k-metal-0.4.4" }"#,
        r#"signinum-jpeg-metal = { path = "vendor/metal-0.33-patches/signinum-jpeg-metal-0.4.4" }"#,
        r#"signinum-transcode-metal = { path = "vendor/metal-0.33-patches/signinum-transcode-metal-0.4.4" }"#,
    ];
    assert_eq!(
        manifest_patch_crates(&manifest),
        expected_patches,
        "Cargo.toml must only carry the approved local Metal 0.33 patch set"
    );
    for required in [
        "path: wsi-dicom",
        "working-directory: wsi-dicom",
        "manifest-path: ./wsi-dicom/Cargo.toml",
    ] {
        assert!(
            workflow.contains(required),
            "CI must keep workspace checkout metadata `{required}`"
        );
    }
    for forbidden in [
        "repository: frames-sg/signinum",
        "path: signinum",
        "repository: frames-sg/statumen",
        "path: statumen",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "CI must not check out unused local patches; found `{forbidden}`"
        );
    }
}

#[test]
fn vendored_metal_patches_pin_new_metal_version() {
    for manifest_path in [
        "vendor/metal-0.33-patches/statumen-0.3.1/Cargo.toml",
        "vendor/metal-0.33-patches/signinum-j2k-metal-0.4.4/Cargo.toml",
        "vendor/metal-0.33-patches/signinum-jpeg-metal-0.4.4/Cargo.toml",
        "vendor/metal-0.33-patches/signinum-transcode-metal-0.4.4/Cargo.toml",
    ] {
        let manifest = fs::read_to_string(crate_root().join(manifest_path))
            .unwrap_or_else(|err| panic!("read {manifest_path}: {err}"));
        assert!(
            manifest.contains("version = \"=0.33.0\""),
            "{manifest_path} must pin metal to =0.33.0"
        );
        assert!(
            !manifest.contains("version = \"0.31\""),
            "{manifest_path} must not retain metal 0.31"
        );
    }
}

fn manifest_patch_crates(manifest: &str) -> Vec<&str> {
    let mut patches = Vec::new();
    let mut in_patch_crates_io = false;

    for line in manifest.lines().map(str::trim) {
        if line.starts_with("[patch.") && line != "[patch.crates-io]" {
            patches.push(line);
            continue;
        }
        if line.starts_with('[') {
            in_patch_crates_io = line == "[patch.crates-io]";
            continue;
        }
        if in_patch_crates_io && !line.is_empty() && !line.starts_with('#') {
            patches.push(line);
        }
    }

    patches
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
            !lockfile_package_dependencies(&lockfile, "wsi-dicom").contains(&dependency.to_string()),
            "wsi-dicom package must not depend directly on non-signinum JPEG dependency {dependency}"
        );
    }
}

fn lockfile_package_dependencies(lockfile: &str, package: &str) -> Vec<String> {
    let mut in_package = false;
    let mut in_dependencies = false;
    let mut dependencies = Vec::new();
    for line in lockfile.lines().map(str::trim) {
        if line == "[[package]]" {
            in_package = false;
            in_dependencies = false;
            continue;
        }
        if line == format!("name = \"{package}\"") {
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if line == "dependencies = [" {
            in_dependencies = true;
            continue;
        }
        if in_dependencies && line == "]" {
            break;
        }
        if in_dependencies {
            dependencies.push(line.trim_matches(',').trim_matches('"').to_string());
        }
    }
    dependencies
}

#[test]
fn metal_feature_enables_statumen_metal_decode_plumbing() {
    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");
    assert!(
        !manifest.contains("gpu = ["),
        "wsi-dicom must not expose a non-portable aggregate gpu feature; use cuda or metal explicitly"
    );
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

#[test]
fn cuda_feature_keeps_published_encode_plumbing_and_documents_blockers() {
    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");
    assert!(
        manifest.contains("cuda = [\"dep:signinum-j2k-cuda\"]"),
        "wsi-dicom's cuda feature must keep published signinum-j2k-cuda encode acceleration buildable"
    );
    let readme = fs::read_to_string(crate_root().join("README.md")).expect("read README");
    assert!(
        !readme.contains("features = [\"gpu\"]") && !readme.contains("| `gpu` |"),
        "README.md must document cuda/metal features explicitly instead of a non-portable gpu aggregate"
    );
    assert!(
        readme.contains("statumen CUDA tile decode waits on a published statumen 0.4.x crate/API"),
        "README.md must document why statumen CUDA tile decode is not yet wired"
    );
    assert!(
        readme.contains("Direct JPEG-to-HTJ2K CUDA acceleration waits on a published `signinum-transcode-cuda` crate/API"),
        "README.md must document why direct CUDA transcode is not yet wired"
    );
}

#[test]
fn release_profiles_use_aggressive_rust_optimization_settings() {
    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");
    for required in [
        "[profile.release]",
        "lto = \"fat\"",
        "codegen-units = 1",
        "panic = \"abort\"",
        "[profile.bench]",
    ] {
        assert!(
            manifest.contains(required),
            "Cargo.toml release/bench profiles must include `{required}`"
        );
    }
    let readme = fs::read_to_string(crate_root().join("README.md")).expect("read README");
    assert!(
        readme.contains("RUSTFLAGS=\"-C target-cpu=native\" cargo build --release"),
        "README.md must document the native CPU release build command"
    );
}

#[test]
fn readme_keeps_public_quickstart_current() {
    let readme = fs::read_to_string(crate_root().join("README.md")).expect("read README");
    let crate_requirement = format!("wsi-dicom = \"{}\"", env!("CARGO_PKG_VERSION"));
    for required in [
        "cargo install wsi-dicom",
        crate_requirement.as_str(),
        "## Quickstart",
        "wsi-dicom convert slide.ndpi --out dicom-out --research-placeholder",
        "Export::from_slide",
    ] {
        assert!(
            readme.contains(required),
            "README.md must keep the public quickstart current; missing `{required}`"
        );
    }
}

#[test]
fn package_metadata_points_to_public_project_surfaces() {
    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");

    for required in [
        "repository = \"https://github.com/frames-sg/wsi-dicom\"",
        "homepage = \"https://github.com/frames-sg/wsi-dicom\"",
        "documentation = \"https://docs.rs/wsi-dicom\"",
    ] {
        assert!(
            manifest.contains(required),
            "Cargo.toml package metadata must include `{required}`"
        );
    }
}

#[test]
fn public_prelude_exports_common_api_types() {
    use wsi_dicom::prelude::{
        Error as PreludeError, Export as PreludeExport, ExportOptions as PreludeExportOptions,
        MetadataSource as PreludeMetadataSource, TransferSyntax as PreludeTransferSyntax,
    };

    let _export = PreludeExport::from_slide("slide.ndpi")
        .to_directory("out")
        .with_metadata(PreludeMetadataSource::ResearchPlaceholder);
    assert_eq!(
        PreludeExportOptions::default().transfer_syntax,
        PreludeTransferSyntax::Htj2kLosslessRpcl
    );
    let _ = std::any::type_name::<PreludeError>();
}

#[test]
fn pre_1_0_public_api_hardening_is_enforced() {
    let lib = fs::read_to_string(crate_root().join("src/lib.rs")).expect("read lib.rs");
    assert!(
        lib.contains("missing_docs"),
        "src/lib.rs must enable a missing-docs lint before 1.0"
    );

    let api = fs::read_to_string(crate_root().join("src/api.rs")).expect("read api.rs");
    for required in [
        "#[must_use =",
        "pub struct Export",
        "pub fn from_slide",
        "pub fn to_directory",
        "pub fn with_options",
        "pub fn with_metadata",
        "pub fn level",
        "pub fn transfer_syntax",
        "pub fn jpeg_direct_htj2k_profile",
        "pub fn tile_size",
        "pub fn jpeg_quality",
        "pub fn icc_profile_policy",
        "pub fn encode_backend",
        "pub fn codec_validation",
        "pub fn source_device_decode",
        "pub fn j2k_decomposition_levels",
        "pub fn gpu_encode_inflight_tiles",
        "pub fn gpu_encode_memory_mib",
        "pub fn gpu_pipeline_depth",
        "pub fn gpu_row_batch_rows",
        "pub fn gpu_row_batch_target_tiles",
        "pub fn source_aware_transfer_syntax",
    ] {
        assert!(
            api.contains(required),
            "src/api.rs must keep builder API hardening marker `{required}`"
        );
    }

    for file in [
        "src/error.rs",
        "src/metadata.rs",
        "src/options.rs",
        "src/request.rs",
        "src/report.rs",
        "src/validation.rs",
        "src/diagnostics.rs",
    ] {
        let source = fs::read_to_string(crate_root().join(file))
            .unwrap_or_else(|err| panic!("read {file}: {err}"));
        assert!(
            source.contains("#[non_exhaustive]"),
            "{file} must mark stable public structs/enums non_exhaustive before 1.0"
        );
    }
}

#[test]
fn advanced_frame_encode_api_does_not_expose_signinum_sample_types() {
    let request = fs::read_to_string(crate_root().join("src/request.rs")).expect("read request.rs");

    for required in [
        "pub struct FrameSamples",
        "impl<'a> FrameSamples<'a>",
        "pub fn new(",
        "pub samples: FrameSamples<'a>",
    ] {
        assert!(
            request.contains(required),
            "src/request.rs must expose wsi-dicom owned frame samples API; missing `{required}`"
        );
    }

    assert!(
        !request.contains("pub samples: J2kLosslessSamples"),
        "J2kFrameEncodeRequest must not expose signinum_j2k::J2kLosslessSamples directly"
    );
}

#[test]
fn pre_1_0_release_gates_are_documented_and_automated() {
    if !in_source_checkout() {
        return;
    }

    let workflow =
        fs::read_to_string(crate_root().join(".github/workflows/ci.yml")).expect("read CI");
    for required in [
        "cargo rustdoc --lib --no-default-features -- -D missing_docs",
        "cargo llvm-cov --package wsi-dicom --lib --bins --tests --no-default-features --summary-only --fail-under-lines 80",
        "cargo semver-checks check-release",
        "cargo check --features cuda --lib",
        "cargo check --features metal --lib",
    ] {
        assert!(
            workflow.contains(required),
            "CI must include pre-1.0 release gate `{required}`"
        );
    }
    assert!(
        !workflow.contains("--features gpu"),
        "CI must check cuda and metal explicitly instead of a non-portable gpu aggregate feature"
    );

    let xtask = fs::read_to_string(crate_root().join("xtask/src/main.rs")).expect("read xtask");
    for required in [
        "\"docs-strict\"",
        "\"coverage\"",
        "\"semver\"",
        "\"rustdoc\"",
        "\"missing_docs\"",
        "\"llvm-cov\"",
        "\"--fail-under-lines\"",
        "\"80\"",
        "\"semver-checks\"",
        "\"check-release\"",
    ] {
        assert!(
            xtask.contains(required),
            "xtask must expose pre-1.0 release gate `{required}`"
        );
    }

    let security =
        fs::read_to_string(crate_root().join(".github/SECURITY.md")).expect("read SECURITY");
    for required in [
        "latest published pre-1.0",
        "1.0 and later",
        "critical security fixes",
    ] {
        assert!(
            security.contains(required),
            ".github/SECURITY.md must describe pre-1.0 and 1.0 support policy; missing `{required}`"
        );
    }

    let readme = fs::read_to_string(crate_root().join("README.md")).expect("read README");
    for required in [
        "Pre-1.0 release gates",
        "cargo xtask docs-strict",
        "cargo xtask coverage",
        "cargo xtask semver",
        "representative real-slide corpus",
        "1.0 release candidate",
    ] {
        assert!(
            readme.contains(required),
            "README.md must include pre-1.0 release gate `{required}`"
        );
    }

    let contributing = fs::read_to_string(crate_root().join(".github/CONTRIBUTING.md"))
        .expect("read CONTRIBUTING");
    for required in ["MSRV", "Semantic Versioning", "JSON report fields"] {
        assert!(
            contributing.contains(required),
            ".github/CONTRIBUTING.md must document public compatibility policy; missing `{required}`"
        );
    }
}

#[test]
fn release_hygiene_files_are_present_and_current() {
    if !in_source_checkout() {
        return;
    }

    for required in [
        ".github/CHANGELOG.md",
        ".github/CODE_OF_CONDUCT.md",
        ".github/CONTRIBUTING.md",
        ".github/SECURITY.md",
        ".github/typos.toml",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "xtask/Cargo.toml",
        "xtask/src/main.rs",
        "examples/basic_export.rs",
        "examples/profile_coverage.rs",
    ] {
        assert!(
            crate_root().join(required).is_file(),
            "wsi-dicom release hygiene file missing: {required}"
        );
    }

    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read manifest");
    assert!(
        !manifest.contains("vendored-codecs"),
        "unused vendored-codecs feature should not be advertised"
    );

    let changelog =
        fs::read_to_string(crate_root().join(".github/CHANGELOG.md")).expect("read changelog");
    for required in [
        "## [0.4.0]",
        "## [0.3.0]",
        "## [0.2.0]",
        "## [0.1.0]",
        "DICOM VL Whole Slide Microscopy",
    ] {
        assert!(
            changelog.contains(required),
            ".github/CHANGELOG.md must backfill public release history; missing `{required}`"
        );
    }

    let xtask = fs::read_to_string(crate_root().join("xtask/src/main.rs")).expect("read xtask");
    for required in ["fmt", "clippy", "test", "deny", "package", "release-test"] {
        assert!(xtask.contains(required), "xtask must expose `{required}`");
    }
}

#[test]
fn ci_checks_workspace_and_gui_app_without_deleted_validation_bench() {
    if !in_source_checkout() {
        return;
    }

    let workflow =
        fs::read_to_string(crate_root().join(".github/workflows/ci.yml")).expect("read CI");

    for required in [
        "cargo clippy --workspace --no-default-features --all-targets -- -D warnings",
        "cargo test --workspace --no-default-features --all-targets",
        "cargo check -p wsi-dicom-gui",
    ] {
        assert!(
            workflow.contains(required),
            "CI must exercise the workspace and GUI app; missing `{required}`"
        );
    }

    assert!(
        !workflow.contains("bench/validate_transcode"),
        "deleted validation bench crate must not remain in CI"
    );
}

#[test]
fn adoption_surfaces_are_documented_and_packaging_stays_focused() {
    if !in_source_checkout() {
        return;
    }

    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).expect("read manifest");
    let readme = fs::read_to_string(crate_root().join("README.md")).expect("read README");

    assert!(
        manifest.contains("\"apps/wsi-dicom-gui\""),
        "Cargo.toml must include the GUI app in the workspace members"
    );
    assert!(
        manifest.contains("exclude = ["),
        "Cargo.toml must keep package exclusions explicit"
    );
    for required in [
        "\"bench/**\"",
        "\"apps/wsi-dicom-gui/**\"",
        "\".github/**\"",
        "\".cargo/**\"",
    ] {
        assert!(
            manifest.contains(required),
            "Cargo.toml package exclusions must include adoption surface `{required}`"
        );
    }

    for required in [
        "wsi-dicom doctor",
        "wsi-dicom self-test",
        "apps/wsi-dicom-gui",
    ] {
        assert!(
            readme.contains(required),
            "README.md must advertise adoption surface `{required}`"
        );
    }
}

#[test]
fn root_and_docs_do_not_accumulate_markdown_bloat() {
    if !in_source_checkout() {
        return;
    }

    let mut root_docs = fs::read_dir(crate_root())
        .expect("read crate root")
        .filter_map(|entry| {
            let path = entry.expect("read crate root entry").path();
            (path.extension().and_then(|value| value.to_str()) == Some("md"))
                .then(|| path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    root_docs.sort();
    assert_eq!(
        root_docs,
        vec!["README.md"],
        "repository root must keep Markdown limited to README.md"
    );

    let docs_dir = crate_root().join("docs");
    if !docs_dir.exists() {
        return;
    }

    let mut docs = fs::read_dir(&docs_dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", docs_dir.display()))
        .filter_map(|entry| {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {}: {err}", docs_dir.display()))
                .path();
            (path.extension().and_then(|value| value.to_str()) == Some("md"))
                .then(|| path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    docs.sort();

    assert_eq!(
        docs,
        Vec::<String>::new(),
        "docs/ must not contain standalone Markdown docs"
    );
}

#[test]
fn crates_io_publish_path_is_explicit() {
    if !in_source_checkout() {
        return;
    }

    let workflow = fs::read_to_string(crate_root().join(".github/workflows/publish.yml"))
        .expect("read publish workflow");
    let script = fs::read_to_string(crate_root().join("scripts/publish-crate.sh"))
        .expect("read publish script");

    assert!(
        workflow.contains("scripts/publish-crate.sh"),
        "publish workflow must call the checked-in publish script"
    );
    assert!(
        workflow.contains("CRATES_IO_API_TOKEN"),
        "publish workflow must require the crates.io token secret"
    );
    for required in [
        "cargo publish --dry-run",
        "cargo info \"${crate}@${version}\"",
        "cargo publish",
    ] {
        assert!(
            script.contains(required),
            "publish script must include `{required}`"
        );
    }
}

#[test]
fn tracked_text_files_do_not_include_agent_private_artifacts() {
    let private_docs_name = ["super", "powers"].concat();
    let private_dir = ["docs", private_docs_name.as_str()].join("/");
    let migration_doc = ["MIGRATION", ".md"].concat();
    let migration_doc_lower = migration_doc.to_ascii_lowercase();
    let assistant_markers = [
        ["Fast Path For L", "LM-Assisted Use"].concat(),
        ["L", "LM-friendly"].concat(),
        ["asking an L", "LM"].concat(),
        ["Chat", "GPT"].concat(),
        ["Clau", "de"].concat(),
        ["Co", "dex"].concat(),
        ["AI", "-generated"].concat(),
        ["generated", " by ", "A", "I"].concat(),
    ];
    let mut offenders = Vec::new();

    for path in tracked_text_files(crate_root()) {
        let relative = relative_path(&path);
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if relative.starts_with(&private_dir) || file_name == migration_doc_lower {
            offenders.push(relative);
            continue;
        }

        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        if assistant_markers
            .iter()
            .any(|marker| source.contains(marker))
        {
            offenders.push(relative);
        }
    }

    assert!(
        offenders.is_empty(),
        "tracked text files must not include agent-private planning docs, migration scratch files, or assistant-facing artifacts:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn tracked_text_files_do_not_include_local_user_paths() {
    let unix_user_home = ["/", "Users", "/"].concat();
    let windows_user_home = ["C:", "\\", "Users", "\\"].concat();
    let mut offenders = Vec::new();

    for path in tracked_text_files(crate_root()) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        if source.contains(&unix_user_home) || source.contains(&windows_user_home) {
            offenders.push(relative_path(&path));
        }
    }

    assert!(
        offenders.is_empty(),
        "tracked text files must not include local user-home paths:\n{}",
        offenders.join("\n")
    );
}

fn rust_sources(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    visit_rust_sources(root, &mut out);
    out
}

fn tracked_text_files(root: &Path) -> Vec<std::path::PathBuf> {
    if in_source_checkout() {
        return tracked_text_files_from_git(root);
    }

    let mut out = Vec::new();
    visit_text_files(root, &mut out);
    out
}

fn tracked_text_files_from_git(root: &Path) -> Vec<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .output()
        .expect("run git ls-files");
    assert!(
        output.status.success(),
        "git ls-files failed with {}",
        output.status
    );
    String::from_utf8(output.stdout)
        .expect("git ls-files output must be UTF-8")
        .lines()
        .filter_map(|path| {
            let relative = Path::new(path);
            let absolute = root.join(relative);
            (absolute.is_file() && is_text_file(relative)).then_some(absolute)
        })
        .collect()
}

fn visit_text_files(path: &Path, out: &mut Vec<std::path::PathBuf>) {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if matches!(
        file_name,
        ".git" | "target" | ".venv" | ".venv-bench" | "__pycache__"
    ) {
        return;
    }

    for entry in fs::read_dir(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display())) {
        let entry =
            entry.unwrap_or_else(|err| panic!("read dir entry in {}: {err}", path.display()));
        let path = entry.path();
        if path.is_dir() {
            visit_text_files(&path, out);
        } else if is_text_file(&path) {
            out.push(path);
        }
    }
}

fn is_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("md" | "rs" | "toml" | "yaml" | "yml" | "sh" | "txt")
    ) || path.file_name().and_then(|value| value.to_str()) == Some(".gitignore")
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

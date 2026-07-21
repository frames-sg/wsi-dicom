use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;
use syn::visit::{self, Visit};

fn crate_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn in_source_checkout() -> bool {
    crate_root().join(".git").exists()
}

fn cargo_metadata() -> &'static Value {
    static METADATA: OnceLock<Value> = OnceLock::new();
    METADATA.get_or_init(|| {
        let output = Command::new(env!("CARGO"))
            .args(["metadata", "--locked", "--format-version", "1"])
            .current_dir(crate_root())
            .output()
            .expect("run cargo metadata");
        assert!(
            output.status.success(),
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("cargo metadata must emit JSON")
    })
}

fn workspace_package(name: &str) -> &'static Value {
    cargo_metadata()["packages"]
        .as_array()
        .expect("metadata packages")
        .iter()
        .find(|package| {
            package["name"] == name
                && package["manifest_path"]
                    .as_str()
                    .is_some_and(|path| Path::new(path).starts_with(crate_root()))
        })
        .unwrap_or_else(|| panic!("workspace package {name} missing from cargo metadata"))
}

#[derive(Default)]
struct ArchitecturePaths {
    j2k_native_paths: Vec<String>,
    forbidden_cli_calls: Vec<String>,
}

impl<'ast> Visit<'ast> for ArchitecturePaths {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "j2k_native")
        {
            self.j2k_native_paths.push(path_to_string(path));
        }
        visit::visit_path(self, path);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let called = path_to_string(&path.path);
            if matches!(
                path.path.segments.last().map(|segment| segment.ident.to_string()),
                Some(name)
                    if [
                        "export_dicom",
                        "profile_dicom_routes",
                        "profile_dicom_route_coverage",
                        "profile_dicom_route_corpus_coverage",
                    ]
                    .contains(&name.as_str())
            ) {
                self.forbidden_cli_calls.push(called);
            }
        }
        visit::visit_expr_call(self, call);
    }
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn parse_architecture(path: &Path) -> ArchitecturePaths {
    let source =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let file = syn::parse_file(&source)
        .unwrap_or_else(|error| panic!("parse {} as Rust: {error}", path.display()));
    let mut policy = ArchitecturePaths::default();
    policy.visit_file(&file);
    policy
}

#[test]
fn source_dependency_boundaries_are_enforced_by_rust_syntax() {
    let mut direct_native_imports = Vec::new();
    for path in rust_sources(&crate_root().join("src")) {
        let policy = parse_architecture(&path);
        if !policy.j2k_native_paths.is_empty() {
            direct_native_imports.push(format!(
                "{}: {}",
                relative_path(&path),
                policy.j2k_native_paths.join(", ")
            ));
        }
    }
    assert!(
        direct_native_imports.is_empty(),
        "wsi-dicom must use the stable j2k facade instead of j2k-native:\n{}",
        direct_native_imports.join("\n")
    );

    let cli_policy = parse_architecture(&crate_root().join("src/cli_report.rs"));
    assert!(
        cli_policy.forbidden_cli_calls.is_empty(),
        "cli_report must format reports without calling export orchestration: {}",
        cli_policy.forbidden_cli_calls.join(", ")
    );
}

#[test]
fn cargo_metadata_enforces_dependency_and_feature_topology() {
    let package = workspace_package("wsi-dicom");
    assert_eq!(package["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        package["repository"],
        "https://github.com/frames-sg/wsi-dicom"
    );
    assert_eq!(
        package["homepage"],
        "https://github.com/frames-sg/wsi-dicom"
    );
    assert_eq!(package["documentation"], "https://docs.rs/wsi-dicom");

    let dependencies = package["dependencies"]
        .as_array()
        .expect("package dependencies");
    let direct_names = dependencies
        .iter()
        .map(|dependency| dependency["name"].as_str().expect("dependency name"))
        .collect::<Vec<_>>();
    for forbidden in [
        "j2k-native",
        "jpeg-encoder",
        "turbojpeg",
        "mozjpeg",
        "zune-jpeg",
    ] {
        assert!(
            !direct_names.contains(&forbidden),
            "wsi-dicom must not directly depend on {forbidden}"
        );
    }
    for dependency in dependencies.iter().filter(|dependency| {
        dependency["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("j2k") || name == "wsi-rs")
    }) {
        assert!(
            dependency["path"].is_null(),
            "release dependencies must not point at sibling checkouts: {dependency}"
        );
        assert!(
            dependency["source"]
                .as_str()
                .is_some_and(|source| source.starts_with("registry+")),
            "release dependency must resolve from a registry: {dependency}"
        );
    }

    let features = package["features"].as_object().expect("package features");
    assert!(!features.contains_key("gpu"));
    assert_eq!(
        features["cuda"].as_array().expect("cuda feature"),
        &[Value::String("dep:j2k-cuda".into())]
    );
    let metal = features["metal"].as_array().expect("metal feature");
    for member in [
        "dep:metal",
        "dep:j2k-metal",
        "dep:j2k-jpeg-metal",
        "wsi-rs/metal",
    ] {
        assert!(
            metal.contains(&Value::String(member.into())),
            "missing {member}"
        );
    }
}

#[test]
fn resolved_codec_graph_has_one_source_per_package_identity() {
    let packages = cargo_metadata()["packages"]
        .as_array()
        .expect("metadata packages");
    for name in [
        "j2k",
        "j2k-core",
        "j2k-jpeg",
        "j2k-jpeg-metal",
        "j2k-metal",
        "j2k-native",
        "j2k-tilecodec",
        "j2k-transcode",
        "j2k-transcode-metal",
        "wsi-rs",
    ] {
        let identities = packages
            .iter()
            .filter(|package| package["name"] == name)
            .map(|package| {
                (
                    package["version"].as_str().unwrap_or_default(),
                    package["source"].as_str().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            identities.len() <= 1,
            "resolved graph contains duplicate {name} identities: {identities:?}"
        );
    }
}

#[test]
fn public_api_compiles_through_owned_types_and_prelude() {
    use wsi_dicom::prelude::{
        Error as PreludeError, Export as PreludeExport, ExportOptions as PreludeExportOptions,
        FrameSamples as PreludeFrameSamples, MetadataSource as PreludeMetadataSource,
        TransferSyntax as PreludeTransferSyntax,
    };

    let _export = PreludeExport::from_slide("slide.ndpi")
        .to_directory("out")
        .with_metadata(PreludeMetadataSource::ResearchPlaceholder)
        .max_instance_metadata_bytes(256 * 1024 * 1024)
        .max_total_metadata_bytes(1024 * 1024 * 1024);
    assert_eq!(
        PreludeExportOptions::default().transfer_syntax,
        PreludeTransferSyntax::Htj2kLosslessRpcl
    );
    let samples = PreludeFrameSamples::new(&[0], 1, 1, 1, 8, false).expect("valid sample");
    assert_eq!(samples.data, &[0]);
    let _ = std::any::type_name::<PreludeError>();
}

#[test]
fn cargo_package_contains_runtime_sources_and_excludes_repository_only_assets() {
    if !in_source_checkout() {
        return;
    }
    let output = Command::new(env!("CARGO"))
        .args(["package", "--list", "--locked", "--allow-dirty"])
        .current_dir(crate_root())
        .output()
        .expect("run cargo package --list");
    assert!(
        output.status.success(),
        "cargo package --list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files = String::from_utf8(output.stdout).expect("package list must be UTF-8");
    for required in [
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "README.md",
        "src/lib.rs",
        "src/writer/frame_index.rs",
        "src/writer/persistence.rs",
    ] {
        assert!(
            files.lines().any(|path| path == required),
            "package is missing {required}"
        );
    }
    for forbidden_prefix in [".github/", "apps/", "bench/", "fuzz/", "supply-chain/"] {
        assert!(
            files
                .lines()
                .all(|path| !path.starts_with(forbidden_prefix)),
            "package includes repository-only path {forbidden_prefix}"
        );
    }
}

#[test]
fn tracked_text_files_do_not_include_local_user_paths() {
    let unix_user_home = ["/", "Users", "/"].concat();
    let windows_user_home = ["C:", "\\", "Users", "\\"].concat();
    let mut offenders = Vec::new();

    for path in tracked_text_files(crate_root()) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
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

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    visit_rust_sources(root, &mut out);
    out
}

fn visit_rust_sources(path: &Path, out: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    {
        let entry =
            entry.unwrap_or_else(|error| panic!("read dir entry in {}: {error}", path.display()));
        let path = entry.path();
        if path.is_dir() {
            visit_rust_sources(&path, out);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn tracked_text_files(root: &Path) -> Vec<PathBuf> {
    if !in_source_checkout() {
        return rust_sources(&root.join("src"));
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .output()
        .expect("run git ls-files");
    assert!(output.status.success(), "git ls-files failed");
    String::from_utf8(output.stdout)
        .expect("git ls-files output must be UTF-8")
        .lines()
        .filter_map(|relative| {
            let relative = Path::new(relative);
            let path = root.join(relative);
            (path.is_file() && is_first_party_text_file(relative)).then_some(path)
        })
        .collect()
}

fn is_first_party_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("md" | "rs" | "toml" | "yaml" | "yml" | "sh" | "txt")
    ) && !path.starts_with("vendor")
}

fn relative_path(path: &Path) -> String {
    path.strip_prefix(crate_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

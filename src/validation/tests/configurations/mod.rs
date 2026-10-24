//! Every VL WSI configuration PS3.3 A.32.8 allows, each as a conformant
//! fixture that passes every check of the general profile, and broken
//! variants that fail the requirement specific to the configuration.
//!
//! Set `WSI_DICOM_CONFIGURATION_FIXTURES` to a directory to keep the
//! conformant fixtures there, e.g. to run dciodvfy and dcentvfy on them.

mod broken;
mod fixture;
mod frames;

use std::path::{Path, PathBuf};

use dicom_dictionary_std::{tags, uids};

use super::{FakeRunner, ValidationOptions, ValidationProfile, ValidationStatus};
use crate::validation::{validate_dicom_path_with_runner, ValidationReport};
use fixture::{Concatenation, Dimension, Flavor, OpticalPath, Organization, Study, Wsi};

/// Where a test's fixtures are written: a kept directory when
/// `WSI_DICOM_CONFIGURATION_FIXTURES` is set, otherwise a temporary one.
struct FixtureDir {
    _temporary: Option<tempfile::TempDir>,
    path: PathBuf,
}

impl FixtureDir {
    fn new(name: &str) -> Self {
        match std::env::var_os("WSI_DICOM_CONFIGURATION_FIXTURES") {
            Some(root) => {
                let path = PathBuf::from(root).join(name);
                std::fs::create_dir_all(&path).expect("fixture directory");
                Self {
                    _temporary: None,
                    path,
                }
            }
            None => {
                let temporary = tempfile::tempdir().expect("tempdir");
                let path = temporary.path().to_path_buf();
                Self {
                    _temporary: Some(temporary),
                    path,
                }
            }
        }
    }

    /// A scratch subdirectory for broken variants, never kept.
    fn broken(&self) -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }
}

fn report(path: &Path, profile: ValidationProfile) -> ValidationReport {
    validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .expect("validation report")
}

/// Every intrinsic check of the general profile passes for `path`, a file or
/// a set; the external validators are not run here.
#[track_caller]
fn assert_conformant(path: &Path) {
    let report = report(path, ValidationProfile::General);
    let failures: Vec<String> = report
        .checks
        .iter()
        .filter(|check| check.name.starts_with("intrinsic-"))
        .filter(|check| check.status != ValidationStatus::Passed)
        .map(|check| format!("{} [{:?}]: {}", check.name, check.status, check.message))
        .collect();
    assert!(
        failures.is_empty(),
        "{}: {} failing checks:\n{}",
        path.display(),
        failures.len(),
        failures.join("\n")
    );
}

/// Check `rule` fails for `path` with a message containing `expected`.
#[track_caller]
fn assert_fails(path: &Path, rule: &str, expected: &str) {
    assert_fails_in(path, ValidationProfile::General, rule, expected);
}

#[track_caller]
fn assert_fails_in(path: &Path, profile: ValidationProfile, rule: &str, expected: &str) {
    let report = report(path, profile);
    let check = report
        .checks
        .iter()
        .find(|check| check.name == rule && check.status == ValidationStatus::Failed)
        .unwrap_or_else(|| {
            let seen: Vec<String> = report
                .checks
                .iter()
                .filter(|check| check.status != ValidationStatus::Passed)
                .map(|check| format!("{}: {}", check.name, check.message))
                .collect();
            panic!(
                "{}: {rule} did not fail; failing checks: {seen:#?}",
                path.display()
            )
        });
    assert!(
        check.message.contains(expected),
        "{rule} message lacks {expected:?}: {}",
        check.message
    );
}

/// Writes one conformant configuration and asserts it passes.
#[track_caller]
fn conformant(dir: &FixtureDir, name: &str, wsi: &Wsi) -> PathBuf {
    let study = Study::new(wsi.instance);
    let path = wsi.write(&study, &dir.path, name);
    assert_conformant(&path);
    path
}

#[test]
fn image_flavors_are_conformant() {
    let dir = FixtureDir::new("flavors");
    conformant(&dir, "volume", &Wsi::volume(1));
    for (instance, flavor) in [
        (2, Flavor::Label),
        (3, Flavor::Overview),
        (4, Flavor::Thumbnail),
    ] {
        conformant(
            &dir,
            &format!("{flavor:?}").to_lowercase(),
            &Wsi::associated(instance, flavor),
        );
    }
}

#[test]
fn dimension_organizations_are_conformant() {
    let dir = FixtureDir::new("organizations");
    conformant(
        &dir,
        "tiled-full-explicit",
        &Wsi {
            organization: Organization::TiledFull { explicit: true },
            dimensions: vec![Dimension::Row, Dimension::Column],
            ..Wsi::volume(10)
        },
    );
    conformant(
        &dir,
        "tiled-sparse",
        &Wsi {
            organization: Organization::TiledSparse,
            dimensions: vec![Dimension::Row, Dimension::Column],
            omitted: vec![1],
            ..Wsi::volume(11)
        },
    );
    conformant(
        &dir,
        "3d",
        &Wsi {
            organization: Organization::ThreeD,
            planes: 3,
            dimensions: vec![Dimension::Plane, Dimension::Row, Dimension::Column],
            ..Wsi::volume(12)
        },
    );
}

#[test]
fn focal_planes_optical_paths_and_extended_depth_of_field_are_conformant() {
    let dir = FixtureDir::new("planes-paths");
    conformant(
        &dir,
        "focal-planes",
        &Wsi {
            planes: 3,
            ..Wsi::volume(20)
        },
    );
    let fluorescence = vec![
        OpticalPath::fluorescence("DAPI", 405.0),
        OpticalPath::fluorescence("FITC", 488.0),
    ];
    conformant(
        &dir,
        "optical-paths",
        &Wsi {
            photometric: "MONOCHROME2",
            paths: fluorescence.clone(),
            ..Wsi::volume(21)
        },
    );
    conformant(
        &dir,
        "optical-paths-planes-sparse",
        &Wsi {
            photometric: "MONOCHROME2",
            paths: fluorescence,
            planes: 2,
            organization: Organization::TiledSparse,
            dimensions: vec![
                Dimension::Path,
                Dimension::Plane,
                Dimension::Row,
                Dimension::Column,
            ],
            omitted: vec![2],
            ..Wsi::volume(22)
        },
    );
    conformant(
        &dir,
        "extended-depth-of-field",
        &Wsi {
            extended_depth_of_field: true,
            ..Wsi::volume(23)
        },
    );
}

#[test]
fn concatenations_are_conformant() {
    let dir = FixtureDir::new("concatenation");
    let study = Study::new(30);
    let whole = Wsi {
        planes: 2,
        ..Wsi::volume(30)
    };
    let frames = whole.positions().len();
    let source = whole.sop_instance_uid(&study);
    for (number, range) in [(1u16, 0..3), (2, 3..frames)] {
        Wsi {
            concatenation: Some(Concatenation {
                number,
                total: 2,
                frames: range,
                source: source.clone(),
            }),
            ..whole.clone()
        }
        .write(&study, &dir.path, &format!("member-{number}"));
    }
    assert_conformant(&dir.path);
}

#[test]
fn transfer_syntaxes_are_conformant() {
    let dir = FixtureDir::new("transfer-syntaxes");
    for (index, (syntax, photometric, bits, tile)) in [
        (uids::IMPLICIT_VR_LITTLE_ENDIAN, "RGB", 8, (8, 8)),
        (uids::EXPLICIT_VR_LITTLE_ENDIAN, "RGB", 8, (8, 8)),
        (uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN, "RGB", 8, (8, 8)),
        (
            uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
            "RGB",
            8,
            (8, 8),
        ),
        (uids::DEFLATED_IMAGE_FRAME_COMPRESSION, "RGB", 8, (8, 8)),
        (uids::RLE_LOSSLESS, "RGB", 8, (8, 8)),
        (uids::RLE_LOSSLESS, "MONOCHROME2", 16, (8, 8)),
        (uids::JPEG_BASELINE8_BIT, "YBR_FULL_422", 8, (8, 16)),
        (uids::JPEG_BASELINE8_BIT, "MONOCHROME2", 8, (8, 8)),
        (uids::JPEG_EXTENDED12_BIT, "MONOCHROME2", 8, (8, 8)),
        (uids::JPEG_LOSSLESS, "RGB", 8, (8, 8)),
        (uids::JPEG_LOSSLESS_SV1, "MONOCHROME2", 16, (8, 8)),
        (uids::JPEGLS_LOSSLESS, "RGB", 8, (4, 6)),
        (uids::JPEGLS_NEAR_LOSSLESS, "RGB", 8, (4, 6)),
        (uids::JPEG2000_LOSSLESS, "YBR_RCT", 8, (8, 8)),
        (uids::JPEG2000_LOSSLESS, "MONOCHROME2", 16, (8, 8)),
        (uids::JPEG2000, "YBR_ICT", 8, (8, 8)),
        (uids::JPEG2000MC_LOSSLESS, "YBR_RCT", 8, (8, 8)),
        (uids::JPEG2000MC, "YBR_ICT", 8, (8, 8)),
        (uids::HTJ2K_LOSSLESS, "RGB", 8, (8, 8)),
        (uids::HTJ2K_LOSSLESS_RPCL, "YBR_RCT", 8, (8, 8)),
        (uids::HTJ2K, "YBR_ICT", 8, (8, 8)),
        (uids::JPEGXL_LOSSLESS, "MONOCHROME2", 8, (8, 8)),
        (uids::JPEGXL, "RGB", 8, (8, 8)),
        (uids::JPEGXLJPEG_RECOMPRESSION, "MONOCHROME2", 8, (8, 8)),
    ]
    .into_iter()
    .enumerate()
    {
        let wsi = Wsi {
            syntax,
            photometric,
            bits,
            tile,
            matrix: (u32::from(tile.0), u32::from(tile.1) * 2),
            paths: vec![OpticalPath {
                icc: photometric != "MONOCHROME2",
                ..OpticalPath::brightfield()
            }],
            ..Wsi::volume(100 + index as u32)
        };
        conformant(
            &dir,
            &format!("ts-{index:02}-{syntax}-{photometric}-{bits}"),
            &wsi,
        );
    }
}

#[test]
fn monochrome_palette_and_sixteen_bit_images_are_conformant() {
    let dir = FixtureDir::new("color");
    conformant(
        &dir,
        "monochrome-8",
        &Wsi {
            photometric: "MONOCHROME2",
            paths: vec![OpticalPath {
                icc: false,
                ..OpticalPath::brightfield()
            }],
            ..Wsi::volume(40)
        },
    );
    conformant(
        &dir,
        "monochrome-16",
        &Wsi {
            photometric: "MONOCHROME2",
            bits: 16,
            paths: vec![OpticalPath::fluorescence("1", 561.0)],
            ..Wsi::volume(41)
        },
    );
    conformant(
        &dir,
        "palette",
        &Wsi {
            photometric: "MONOCHROME2",
            paths: vec![OpticalPath {
                palette: true,
                ..OpticalPath::fluorescence("1", 488.0)
            }],
            ..Wsi::volume(42)
        },
    );
    conformant(
        &dir,
        "rgb-16",
        &Wsi {
            bits: 16,
            ..Wsi::volume(43)
        },
    );
}

#[test]
fn a_study_with_a_pyramid_and_associated_images_is_conformant() {
    let dir = FixtureDir::new("study");
    let study = Study::new(50);
    let base = Wsi {
        tile: (8, 8),
        matrix: (32, 32),
        ..Wsi::volume(50)
    };
    let base_uid = base.sop_instance_uid(&study);
    base.write(&study, &dir.path, "level-0");
    Wsi {
        matrix: (16, 16),
        spacing_mm: 0.001,
        resampled_from: Some(base_uid.clone()),
        ..Wsi::volume(51)
    }
    .write(&study, &dir.path, "level-1");
    Wsi {
        matrix: (8, 8),
        spacing_mm: 0.002,
        resampled_from: Some(base_uid.clone()),
        ..Wsi::volume(52)
    }
    .write(&study, &dir.path, "level-2");
    Wsi {
        spacing_mm: 0.004,
        resampled_from: Some(base_uid),
        ..Wsi::associated(53, Flavor::Thumbnail)
    }
    .with_matrix((4, 4))
    .write(&study, &dir.path, "thumbnail");
    Wsi::associated(54, Flavor::Label).write(&study, &dir.path, "label");
    Wsi::associated(55, Flavor::Overview).write(&study, &dir.path, "overview");
    assert_conformant(&dir.path);
}

impl Wsi {
    fn with_matrix(mut self, matrix: (u32, u32)) -> Self {
        self.matrix = matrix;
        self
    }
}

#[test]
fn optical_path_identification_names_a_declared_path() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let study = Study::new(60);
    let mut object = Wsi {
        photometric: "MONOCHROME2",
        paths: vec![
            OpticalPath::fluorescence("DAPI", 405.0),
            OpticalPath::fluorescence("FITC", 488.0),
        ],
        ..Wsi::volume(60)
    }
    .object(&study);
    let mut groups = object
        .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .expect("per-frame groups");
    groups.items_mut().expect("items")[0].put(fixture::sq(
        tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
        vec![fixture::item(vec![fixture::text(
            tags::OPTICAL_PATH_IDENTIFIER,
            dicom_core::VR::SH,
            "TRITC",
        )])],
    ));
    object.put(groups);
    let path = fixture::write(object, broken.path(), "unknown-path");
    assert_fails(
        &path,
        "intrinsic-wsi-dicom-wsi-image",
        "Optical Path Identifier \"TRITC\" names no Optical Path Sequence Item",
    );
}

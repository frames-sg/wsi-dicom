//! Broken variants of the conformant configurations: each breaks one
//! requirement specific to its configuration and fails the check for it.

use dicom_core::VR;
use dicom_dictionary_std::{tags, uids};
use dicom_object::DefaultDicomObject;

use super::super::{edit_items, first_item};
use super::fixture::{self, code, item, sq, text, Concatenation, Flavor, OpticalPath};
use super::fixture::{Dimension, Organization, Study, Wsi};
use super::{assert_conformant, assert_fails, assert_fails_in, report, FixtureDir};
use crate::validation::{ValidationProfile, ValidationStatus};

const TILE_GEOMETRY: &str = "intrinsic-wsi-dicom-tile-geometry";
const WSI_IMAGE: &str = "intrinsic-wsi-dicom-wsi-image";
const LOSSY: &str = "intrinsic-wsi-dicom-lossy-history";
const CORE_IMAGE: &str = "intrinsic-wsi-dicom-core-image-profile";

/// `wsi` with `edit` applied, written to `dir/<name>.dcm`.
fn variant(
    dir: &std::path::Path,
    name: &str,
    wsi: &Wsi,
    edit: impl FnOnce(&mut DefaultDicomObject),
) -> std::path::PathBuf {
    let study = Study::new(wsi.instance);
    let mut object = wsi.object(&study);
    edit(&mut object);
    fixture::write(object, dir, name)
}

fn fluorescence() -> Vec<OpticalPath> {
    vec![
        OpticalPath::fluorescence("DAPI", 405.0),
        OpticalPath::fluorescence("FITC", 488.0),
    ]
}

#[test]
fn tiled_full_frames_cover_every_tile_plane_and_path() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let planes = Wsi {
        planes: 2,
        ..Wsi::volume(70)
    };
    let short = variant(broken.path(), "short", &planes, |object| {
        fixture::put(object, tags::NUMBER_OF_FRAMES, VR::IS, "7");
    });
    assert_fails(
        &short,
        TILE_GEOMETRY,
        "2x2 tiles of 8x8 pixels, 2 focal plane(s) and 1 optical path(s) make 8 Frames, but Number of Frames is 7",
    );

    // The last member of a Concatenation ends before the last Frame.
    let study = Study::new(70);
    let member = Wsi {
        concatenation: Some(Concatenation {
            number: 2,
            total: 2,
            frames: 3..7,
            source: planes.sop_instance_uid(&study),
        }),
        ..planes.clone()
    };
    let path = member.write(&study, broken.path(), "short-member");
    assert_fails(
        &path,
        TILE_GEOMETRY,
        "this last member holds Frames 4 to 7 (Concatenation Frame Offset Number 3, Number of Frames 4)",
    );
}

#[test]
fn explicit_tiled_full_frames_follow_the_implicit_order() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    // Focal planes from the coverslip towards the glass slide.
    let descending = Wsi {
        organization: Organization::TiledFull { explicit: true },
        planes: 2,
        plane_spacing_um: -2.0,
        ..Wsi::volume(71)
    };
    let path = variant(broken.path(), "descending-planes", &descending, |_| {});
    assert_fails(
        &path,
        TILE_GEOMETRY,
        "TILED_FULL focal planes go from the glass slide towards the coverslip, but Frame 5 (Z Offset -2 µm) follows Frame 1 (Z Offset 0 µm) of the same tile",
    );

    // Optical paths in an order other than the Optical Path Sequence's.
    let paths = Wsi {
        organization: Organization::TiledFull { explicit: true },
        photometric: "MONOCHROME2",
        paths: fluorescence(),
        ..Wsi::volume(72)
    };
    let swapped = variant(broken.path(), "swapped-paths", &paths, |object| {
        edit_items(object, tags::OPTICAL_PATH_SEQUENCE, |items| {
            items.swap(0, 1)
        });
    });
    assert_fails(
        &swapped,
        TILE_GEOMETRY,
        "Frame 1 has Optical Path Identifier \"DAPI\", not \"FITC\" of Optical Path Sequence Item 1",
    );
}

#[test]
fn three_d_planes_are_equally_spaced() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let volume = Wsi {
        organization: Organization::ThreeD,
        planes: 3,
        dimensions: vec![Dimension::Plane, Dimension::Row, Dimension::Column],
        ..Wsi::volume(73)
    };
    let uneven = variant(broken.path(), "uneven", &volume, |object| {
        edit_items(
            object,
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            |frames| {
                for frame in frames {
                    first_item(frame, tags::PLANE_POSITION_SLIDE_SEQUENCE, |position| {
                        let depth = position
                            .element(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                            .expect("Z Offset")
                            .to_float64()
                            .expect("Z Offset value");
                        if depth == 4.0 {
                            position.put(text(
                                tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                                VR::DS,
                                "5",
                            ));
                        }
                    });
                }
            },
        );
    });
    assert_fails(
        &uneven,
        TILE_GEOMETRY,
        "Dimension Organization Type is 3D, a set of equally spaced parallel planes, but its focal planes are 2 µm and 3 µm apart",
    );
    let spacing = variant(broken.path(), "spacing", &volume, |object| {
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            first_item(shared, tags::PIXEL_MEASURES_SEQUENCE, |measures| {
                measures.put(text(tags::SPACING_BETWEEN_SLICES, VR::DS, "0.003"));
            });
        });
    });
    assert_fails(
        &spacing,
        TILE_GEOMETRY,
        "Spacing Between Slices (0018,0088) is 0.003 mm, but the 3D focal planes are 2 µm apart",
    );
}

#[test]
fn the_imaged_volume_is_the_total_pixel_matrix_at_the_pixel_spacing() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let path = variant(broken.path(), "imaged-volume", &Wsi::volume(74), |object| {
        object.put(fixture::float(tags::IMAGED_VOLUME_WIDTH, 1.0));
    });
    assert_fails(
        &path,
        TILE_GEOMETRY,
        "Imaged Volume Width and Height are 1x",
    );
    // A LABEL image need not declare an imaged volume or a Pixel Spacing.
    let label = variant(
        broken.path(),
        "label",
        &Wsi::associated(75, Flavor::Label),
        |_| {},
    );
    assert_conformant(&label);
}

#[test]
fn optical_paths_tiles_and_specimens_are_described_where_frames_refer_to_them() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let count = variant(broken.path(), "path-count", &Wsi::volume(76), |object| {
        object.put(fixture::long(tags::NUMBER_OF_OPTICAL_PATHS, 2));
    });
    assert_fails(
        &count,
        WSI_IMAGE,
        "Number of Optical Paths (0048,0302) is 2, but the Optical Path Sequence (0048,0105) has 1 Items",
    );

    let sparse = Wsi {
        organization: Organization::TiledSparse,
        dimensions: vec![Dimension::Row, Dimension::Column],
        omitted: vec![1],
        ..Wsi::volume(77)
    };
    let twice = variant(broken.path(), "same-tile-twice", &sparse, |object| {
        edit_items(
            object,
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            |frames| {
                let position = frames[0]
                    .element(tags::PLANE_POSITION_SLIDE_SEQUENCE)
                    .expect("Plane Position (Slide)")
                    .clone();
                frames[1].put(position);
            },
        );
    });
    assert_fails(
        &twice,
        WSI_IMAGE,
        "Tiles Overlap (0048,0304) is NONE, but Per-Frame Functional Groups Sequence Items 1 and 2 encode the same tile",
    );

    let specimen = |uid: String| {
        move |object: &mut DefaultDicomObject| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(sq(
                    tags::SPECIMEN_REFERENCE_SEQUENCE,
                    vec![item(vec![text(tags::SPECIMEN_UID, VR::UI, &uid)])],
                ));
            });
        }
    };
    let described = Wsi::volume(78);
    let referenced = variant(
        broken.path(),
        "specimen-described",
        &described,
        specimen(Study::new(78).specimen),
    );
    assert_conformant(&referenced);
    let undescribed = variant(
        broken.path(),
        "specimen-undescribed",
        &described,
        specimen("1.2.826.0.1.3680043.10.999.7.78.99".into()),
    );
    assert_fails(
        &undescribed,
        WSI_IMAGE,
        "Specimen Reference Sequence (0048,0110) > Specimen UID 1.2.826.0.1.3680043.10.999.7.78.99 is not in the Specimen Description Sequence (0040,0560)",
    );
}

#[test]
fn lossy_history_follows_the_frames_coding() {
    let dir = FixtureDir::new("lossy");
    let broken = dir.broken();
    // A reversible codestream in the lossy-capable JPEG 2000 syntax may be
    // lossless, so 00 conforms.
    let reversible = Wsi {
        syntax: uids::JPEG2000_LOSSLESS,
        photometric: "YBR_RCT",
        ..Wsi::volume(80)
    };
    let path = variant(&dir.path, "jpeg-2000-reversible", &reversible, |object| {
        let meta = object.meta_mut();
        meta.transfer_syntax = uids::JPEG2000.to_string();
        meta.update_information_group_length();
    });
    assert_conformant(&path);

    let irreversible = Wsi {
        syntax: uids::JPEG2000,
        photometric: "YBR_ICT",
        ..Wsi::volume(81)
    };
    let reset = variant(broken.path(), "irreversible-00", &irreversible, |object| {
        fixture::put(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "00");
        assert!(object.remove_element(tags::LOSSY_IMAGE_COMPRESSION_RATIO));
        assert!(object.remove_element(tags::LOSSY_IMAGE_COMPRESSION_METHOD));
    });
    assert_fails(
        &reset,
        LOSSY,
        "Lossy Image Compression is 00, but the Frames are JPEG 2000 codestreams whose COD selects the irreversible 9-7 wavelet",
    );

    let near_lossless = Wsi {
        syntax: uids::JPEGLS_NEAR_LOSSLESS,
        tile: (4, 6),
        matrix: (4, 12),
        ..Wsi::volume(82)
    };
    let method = variant(broken.path(), "jpeg-ls-method", &near_lossless, |object| {
        fixture::put(
            object,
            tags::LOSSY_IMAGE_COMPRESSION_METHOD,
            VR::CS,
            "ISO_10918_1",
        );
    });
    assert_fails(
        &method,
        LOSSY,
        "the Frames are JPEG-LS scans with a NEAR parameter greater than 0, so the last Lossy Image Compression Method is ISO_14495_1, but it is \"ISO_10918_1\"",
    );

    // A lossy compressed version of another image records it.
    let derived = variant(
        broken.path(),
        "lossy-derivation",
        &Wsi::volume(83),
        |object| {
            fixture::put(
                object,
                tags::IMAGE_TYPE,
                VR::CS,
                "DERIVED\\PRIMARY\\VOLUME\\NONE",
            );
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(sq(
                    tags::DERIVATION_IMAGE_SEQUENCE,
                    vec![item(vec![sq(
                        tags::DERIVATION_CODE_SEQUENCE,
                        vec![code("DCM", "113040", "Lossy Compression")],
                    )])],
                ));
            });
        },
    );
    assert_fails(
        &derived,
        WSI_IMAGE,
        "but Lossy Image Compression (0028,2110) is not 01",
    );
}

#[test]
fn jpeg_2000_part_2_photometric_follows_the_part_1_transform_rule() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let part_2 = Wsi {
        syntax: uids::JPEG2000MC_LOSSLESS,
        photometric: "YBR_RCT",
        ..Wsi::volume(84)
    };
    let path = variant(broken.path(), "part-2-ict", &part_2, |object| {
        fixture::put(object, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "YBR_ICT");
    });
    assert_fails(
        &path,
        "intrinsic-wsi-dicom-image-metadata",
        "Photometric Interpretation YBR_ICT is not permitted with Transfer Syntax 1.2.840.10008.1.2.4.92",
    );
}

#[test]
fn pyramid_levels_place_the_same_image_within_a_pixel() {
    let dir = FixtureDir::new("pyramid");
    let study = Study::new(85);
    let base = Wsi {
        matrix: (32, 32),
        ..Wsi::volume(85)
    };
    let base_uid = base.sop_instance_uid(&study);
    let level = Wsi {
        matrix: (16, 16),
        spacing_mm: 0.001,
        resampled_from: Some(base_uid),
        ..Wsi::volume(86)
    };
    let shifted = |shift: &str, name: &str| {
        let directory = dir.path.join(name);
        std::fs::create_dir_all(&directory).expect("pyramid directory");
        base.write(&study, &directory, "level-0");
        let mut object = level.object(&study);
        first_item(
            &mut object,
            tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
            |origin| {
                origin.put(text(
                    tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                    VR::DS,
                    shift,
                ));
            },
        );
        fixture::write(object, &directory, "level-1");
        directory
    };
    // Half a level-1 pixel: the center and the corner conventions differ so.
    assert_conformant(&shifted("20.0005", "half-pixel"));
    assert_fails(
        &shifted("21", "one-millimetre"),
        "intrinsic-wsi-dicom-slide-coordinate-system",
        "has conflicting total-matrix origin or orientation",
    );
}

#[test]
fn core_rejections_name_the_configuration_outside_its_scope() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let study = Study::new(90);
    let cases: [(Wsi, &str, &str); 7] = [
        (
            Wsi::associated(90, Flavor::Label),
            CORE_IMAGE,
            "Image Type Value 3 is LABEL; the core profile accepts VOLUME images only",
        ),
        (
            Wsi {
                organization: Organization::TiledSparse,
                dimensions: vec![Dimension::Row, Dimension::Column],
                ..Wsi::volume(91)
            },
            CORE_IMAGE,
            "Dimension Organization Type is TILED_SPARSE; the core profile accepts TILED_FULL only",
        ),
        (
            Wsi {
                organization: Organization::ThreeD,
                planes: 2,
                dimensions: vec![Dimension::Plane, Dimension::Row, Dimension::Column],
                ..Wsi::volume(92)
            },
            CORE_IMAGE,
            "Dimension Organization Type is 3D; the core profile accepts TILED_FULL only",
        ),
        (
            Wsi {
                planes: 3,
                ..Wsi::volume(93)
            },
            CORE_IMAGE,
            "Total Pixel Matrix Focal Planes is 3; the core profile accepts a single focal plane only",
        ),
        (
            Wsi {
                extended_depth_of_field: true,
                ..Wsi::volume(94)
            },
            CORE_IMAGE,
            "Extended Depth of Field is YES; the core profile accepts images without extended depth of field",
        ),
        (
            Wsi {
                photometric: "MONOCHROME2",
                paths: fluorescence(),
                ..Wsi::volume(95)
            },
            "intrinsic-wsi-dicom-optical-path-structure",
            "Number of Optical Paths is 2 and the Optical Path Sequence has 2 Items; the core profile accepts a single optical path only",
        ),
        (
            Wsi {
                concatenation: Some(Concatenation {
                    number: 1,
                    total: 2,
                    frames: 0..2,
                    source: "1.2.826.0.1.3680043.10.999.7.90.200".into(),
                }),
                ..Wsi::volume(96)
            },
            CORE_IMAGE,
            "Concatenation UID (0020,9161) is present; the core profile excludes Concatenations",
        ),
    ];
    for (index, (wsi, rule, expected)) in cases.into_iter().enumerate() {
        let path = wsi.write(&study, broken.path(), &format!("core-{index}"));
        assert_fails_in(&path, ValidationProfile::Core, rule, expected);
    }
}

#[test]
fn core_set_identity_admits_label_images_without_a_frame_of_reference() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let study = Study::new(97);
    Wsi::volume(97).write(&study, broken.path(), "volume");
    Wsi::associated(98, Flavor::Label).write(&study, broken.path(), "label");
    let report = report(broken.path(), ValidationProfile::Core);
    let identity = report
        .checks
        .iter()
        .find(|check| check.name == "intrinsic-wsi-dicom-clinical-identity-set")
        .expect("clinical identity set check");
    assert_eq!(
        identity.status,
        ValidationStatus::Passed,
        "{}",
        identity.message
    );
}

#[test]
fn dimension_index_values_are_ordinals_from_one() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    let wsi = Wsi {
        organization: Organization::TiledFull { explicit: true },
        dimensions: vec![Dimension::Row, Dimension::Column],
        ..Wsi::volume(99)
    };
    let path = variant(broken.path(), "zero-index", &wsi, |object| {
        edit_items(
            object,
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            |frames| {
                first_item(&mut frames[0], tags::FRAME_CONTENT_SEQUENCE, |content| {
                    content.put(dicom_core::DataElement::new(
                        tags::DIMENSION_INDEX_VALUES,
                        VR::UL,
                        dicom_core::PrimitiveValue::U32([0u32, 1].into_iter().collect()),
                    ));
                });
            },
        );
    });
    assert_fails(
        &path,
        "intrinsic-wsi-dicom-dimension-order",
        "1 Frame(s) have a Dimension Index Value of 0",
    );
}

#[test]
fn image_flavors_depth_of_field_and_color_follow_their_own_requirements() {
    let dir = FixtureDir::new("broken");
    let broken = dir.broken();
    for (instance, flavor, term) in [
        (100, Flavor::Label, "LABEL"),
        (101, Flavor::Overview, "OVERVIEW"),
    ] {
        let path = variant(
            broken.path(),
            &format!("{term}-without-label"),
            &Wsi::associated(instance, flavor),
            |object| fixture::put(object, tags::SPECIMEN_LABEL_IN_IMAGE, VR::CS, "NO"),
        );
        assert_fails(
            &path,
            WSI_IMAGE,
            &format!("Specimen Label in Image (0048,0010) is NO; it shall be YES for Image Type Value 3 {term}"),
        );
    }
    let barcode = variant(
        broken.path(),
        "label-without-barcode",
        &Wsi::associated(102, Flavor::Label),
        |object| assert!(object.remove_element(tags::BARCODE_VALUE)),
    );
    assert_fails(
        &barcode,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Barcode Value (2200,0005): Type 2 missing",
    );
    let unregistered = variant(
        broken.path(),
        "thumbnail-without-frame-of-reference",
        &Wsi::associated(103, Flavor::Thumbnail),
        |object| {
            assert!(object.remove_element(tags::FRAME_OF_REFERENCE_UID));
            assert!(object.remove_element(tags::POSITION_REFERENCE_INDICATOR));
        },
    );
    assert_fails(
        &unregistered,
        "intrinsic-wsi-dicom-iod-modules",
        "required by Table A.32.8-1 but none of its Attributes is present",
    );
    let two_frames = variant(
        broken.path(),
        "thumbnail-two-frames",
        &Wsi::associated(104, Flavor::Thumbnail),
        |object| fixture::put(object, tags::NUMBER_OF_FRAMES, VR::IS, "2"),
    );
    assert_fails(
        &two_frames,
        WSI_IMAGE,
        "Number of Frames (0028,0008) is 2; it is 1 for Image Type Value 3 THUMBNAIL",
    );
    let stacked = Wsi {
        extended_depth_of_field: true,
        ..Wsi::volume(105)
    };
    let focal = variant(broken.path(), "edf-without-planes", &stacked, |object| {
        assert!(object.remove_element(tags::NUMBER_OF_FOCAL_PLANES));
    });
    assert_fails(
        &focal,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Number of Focal Planes (0048,0013): Type 1C missing",
    );
    // An ICC Profile describes a MONOCHROME2 path only beside its palette.
    let monochrome = Wsi {
        photometric: "MONOCHROME2",
        paths: vec![OpticalPath {
            icc: true,
            ..OpticalPath::fluorescence("1", 488.0)
        }],
        ..Wsi::volume(106)
    };
    let icc = variant(broken.path(), "monochrome-icc", &monochrome, |_| {});
    assert_fails(
        &icc,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "ICC Profile (0028,2000): Type 1C present although its condition is not met",
    );
}

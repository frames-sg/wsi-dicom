//! VL Whole Slide Microscopy Image objects built from a description of their
//! configuration: Image Type flavor, Dimension Organization Type, focal
//! planes, optical paths, Concatenation, Transfer Syntax, color and depth.
//! Every attribute is written as PS3.3 A.32.8 requires for that configuration,
//! so the default of each description is a conformant object.

use std::path::{Path, PathBuf};

use dicom_core::value::{DataSetSequence, PixelFragmentSequence};
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};

use super::frames::{self, Format};

const ROOT: &str = "1.2.826.0.1.3680043.10.999.7";

/// Image Type Value 3 (PS3.3 C.8.12.4.1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flavor {
    Volume,
    Label,
    Overview,
    Thumbnail,
}

impl Flavor {
    fn term(self) -> &'static str {
        match self {
            Self::Volume => "VOLUME",
            Self::Label => "LABEL",
            Self::Overview => "OVERVIEW",
            Self::Thumbnail => "THUMBNAIL",
        }
    }

    /// VOLUME and THUMBNAIL images are registered to the slide (A.32.8).
    fn registered(self) -> bool {
        matches!(self, Self::Volume | Self::Thumbnail)
    }
}

/// Dimension Organization Type (C.7.6.17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Organization {
    /// Implicit positions; `explicit` also encodes them per Frame.
    TiledFull {
        explicit: bool,
    },
    TiledSparse,
    /// Equally spaced parallel planes.
    ThreeD,
}

/// One Item of the Optical Path Sequence.
#[derive(Clone, Debug)]
pub(super) struct OpticalPath {
    pub(super) identifier: String,
    /// Illumination Wave Length in nm; brightfield white light when absent.
    pub(super) wavelength: Option<f32>,
    pub(super) icc: bool,
    pub(super) palette: bool,
}

impl OpticalPath {
    pub(super) fn brightfield() -> Self {
        Self {
            identifier: "1".into(),
            wavelength: None,
            icc: true,
            palette: false,
        }
    }

    pub(super) fn fluorescence(identifier: &str, wavelength: f32) -> Self {
        Self {
            identifier: identifier.into(),
            wavelength: Some(wavelength),
            icc: false,
            palette: false,
        }
    }
}

/// A Dimension Index Sequence Item: what it indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dimension {
    Row,
    Column,
    Plane,
    Path,
}

/// This instance's part of a Concatenation.
#[derive(Clone, Debug)]
pub(super) struct Concatenation {
    pub(super) number: u16,
    pub(super) total: u16,
    /// The Frames of the whole Concatenation this instance holds.
    pub(super) frames: std::ops::Range<usize>,
    /// SOP Instance UID of the Concatenation Source.
    pub(super) source: String,
}

/// Identifiers shared by the instances of one Study.
#[derive(Clone, Debug)]
pub(super) struct Study {
    pub(super) study: String,
    pub(super) series: String,
    pub(super) frame_of_reference: String,
    pub(super) pyramid: String,
    pub(super) specimen: String,
    pub(super) concatenation: String,
}

impl Study {
    pub(super) fn new(suffix: u32) -> Self {
        let uid = |role: u32| format!("{ROOT}.{suffix}.{role}");
        Self {
            study: uid(1),
            series: uid(2),
            frame_of_reference: uid(3),
            pyramid: uid(4),
            specimen: uid(5),
            concatenation: uid(6),
        }
    }
}

/// One frame position: optical path, focal plane and tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Position {
    pub(super) path: usize,
    pub(super) plane: usize,
    pub(super) row: u32,
    pub(super) column: u32,
}

/// The configuration of one VL WSI object.
#[derive(Clone, Debug)]
pub(super) struct Wsi {
    pub(super) instance: u32,
    pub(super) flavor: Flavor,
    /// Value 1 DERIVED and Value 4 RESAMPLED, derived from `source`.
    pub(super) resampled_from: Option<String>,
    pub(super) organization: Organization,
    /// Rows and Columns of each Frame.
    pub(super) tile: (u16, u16),
    /// Total Pixel Matrix Rows and Columns.
    pub(super) matrix: (u32, u32),
    pub(super) planes: usize,
    /// Distance between focal planes in µm.
    pub(super) plane_spacing_um: f64,
    pub(super) paths: Vec<OpticalPath>,
    pub(super) extended_depth_of_field: bool,
    pub(super) photometric: &'static str,
    pub(super) bits: u16,
    pub(super) syntax: &'static str,
    /// Pixel Spacing in mm, both directions.
    pub(super) spacing_mm: f64,
    pub(super) concatenation: Option<Concatenation>,
    pub(super) dimensions: Vec<Dimension>,
    /// Tile indices (row-major) left out of a TILED_SPARSE image.
    pub(super) omitted: Vec<u32>,
    pub(super) pyramid: bool,
}

impl Wsi {
    /// A conformant single-path, single-plane TILED_FULL VOLUME image of
    /// 2x2 tiles of 8x8 RGB pixels, Explicit VR Little Endian.
    pub(super) fn volume(instance: u32) -> Self {
        Self {
            instance,
            flavor: Flavor::Volume,
            resampled_from: None,
            organization: Organization::TiledFull { explicit: false },
            tile: (8, 8),
            matrix: (16, 16),
            planes: 1,
            plane_spacing_um: 2.0,
            paths: vec![OpticalPath::brightfield()],
            extended_depth_of_field: false,
            photometric: "RGB",
            bits: 8,
            syntax: uids::EXPLICIT_VR_LITTLE_ENDIAN,
            spacing_mm: 0.0005,
            concatenation: None,
            dimensions: Vec::new(),
            omitted: Vec::new(),
            pyramid: true,
        }
    }

    /// A single-frame LABEL, OVERVIEW or THUMBNAIL image.
    pub(super) fn associated(instance: u32, flavor: Flavor) -> Self {
        Self {
            flavor,
            tile: (8, 8),
            matrix: (8, 8),
            pyramid: flavor == Flavor::Thumbnail,
            ..Self::volume(instance)
        }
    }

    /// Concatenation members share the Instance Number, not the UID.
    pub(super) fn sop_instance_uid(&self, study: &Study) -> String {
        let member = self
            .concatenation
            .as_ref()
            .map_or(0, |concatenation| u32::from(concatenation.number));
        format!("{}.{}.{member}", study.study, 100 + self.instance)
    }

    fn samples(&self) -> u16 {
        if self.photometric == "MONOCHROME2" {
            1
        } else {
            3
        }
    }

    fn format(&self) -> Format {
        Format {
            rows: self.tile.0,
            columns: self.tile.1,
            samples: self.samples(),
            bits: self.bits,
        }
    }

    /// Tiles across and down the Total Pixel Matrix.
    pub(super) fn grid(&self) -> (u32, u32) {
        (
            self.matrix.1.div_ceil(u32::from(self.tile.1)),
            self.matrix.0.div_ceil(u32::from(self.tile.0)),
        )
    }

    fn tiled_full(&self) -> bool {
        matches!(self.organization, Organization::TiledFull { .. })
    }

    /// Every Frame of the image, or of the whole Concatenation, in encoded
    /// order: TILED_FULL varies column, row, plane, then path (C.7.6.17.3);
    /// other organizations list the same Frames last to first, so nothing
    /// may assume their order.
    pub(super) fn positions(&self) -> Vec<Position> {
        let (across, down) = self.grid();
        let mut positions = Vec::new();
        for path in 0..self.paths.len() {
            for plane in 0..self.planes {
                for row in 0..down {
                    for column in 0..across {
                        let tile = row * across + column;
                        if !self.omitted.contains(&tile) {
                            positions.push(Position {
                                path,
                                plane,
                                row,
                                column,
                            });
                        }
                    }
                }
            }
        }
        if !self.tiled_full() {
            positions.reverse();
        }
        positions
    }

    /// The Frames this instance holds.
    fn own_positions(&self) -> Vec<(usize, Position)> {
        let all = self.positions();
        let range = self
            .concatenation
            .as_ref()
            .map_or(0..all.len(), |concatenation| concatenation.frames.clone());
        all.into_iter()
            .enumerate()
            .skip(range.start)
            .take(range.len())
            .collect()
    }

    pub(super) fn object(&self, study: &Study) -> DefaultDicomObject {
        let mut object = InMemDicomObject::new_empty();
        self.identification(&mut object, study);
        self.patient_study_series(&mut object, study);
        self.equipment(&mut object);
        self.image(&mut object);
        self.specimen(&mut object, study);
        self.tile_organization(&mut object);
        self.optical_paths(&mut object);
        self.dimensions(&mut object, study);
        self.functional_groups(&mut object);
        self.pixel_data(&mut object);
        let meta = FileMetaTableBuilder::new()
            .media_storage_sop_class_uid(crate::VL_WSI_SOP_CLASS_UID)
            .media_storage_sop_instance_uid(self.sop_instance_uid(study))
            .transfer_syntax(self.syntax);
        object.with_meta(meta).expect("file meta")
    }

    /// Writes the object to `dir/<name>.dcm`.
    pub(super) fn write(&self, study: &Study, dir: &Path, name: &str) -> PathBuf {
        write(self.object(study), dir, name)
    }

    fn identification(&self, object: &mut InMemDicomObject, study: &Study) {
        let value_1 = if self.resampled_from.is_some() {
            "DERIVED"
        } else {
            "ORIGINAL"
        };
        let value_4 = if self.resampled_from.is_some() {
            "RESAMPLED"
        } else {
            "NONE"
        };
        put(
            object,
            tags::IMAGE_TYPE,
            VR::CS,
            &format!("{value_1}\\PRIMARY\\{}\\{value_4}", self.flavor.term()),
        );
        put(
            object,
            tags::SOP_CLASS_UID,
            VR::UI,
            crate::VL_WSI_SOP_CLASS_UID,
        );
        put(
            object,
            tags::SOP_INSTANCE_UID,
            VR::UI,
            &self.sop_instance_uid(study),
        );
        put(
            object,
            tags::INSTANCE_NUMBER,
            VR::IS,
            &self.instance.to_string(),
        );
        if self.pyramid && self.flavor.registered() {
            put(object, tags::PYRAMID_UID, VR::UI, &study.pyramid);
        }
        if let Some(source) = &self.resampled_from {
            let reference = referenced_instance(source);
            object.put(sq(tags::SOURCE_IMAGE_SEQUENCE, vec![reference.clone()]));
            object.put(sq(
                tags::REFERENCED_SERIES_SEQUENCE,
                vec![item(vec![
                    text(tags::SERIES_INSTANCE_UID, VR::UI, &study.series),
                    sq(tags::REFERENCED_INSTANCE_SEQUENCE, vec![reference]),
                ])],
            ));
        }
        if let Some(concatenation) = &self.concatenation {
            put(
                object,
                tags::CONCATENATION_UID,
                VR::UI,
                &study.concatenation,
            );
            put(
                object,
                tags::SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
                VR::UI,
                &concatenation.source,
            );
            object.put(unsigned(
                tags::IN_CONCATENATION_NUMBER,
                concatenation.number,
            ));
            object.put(unsigned(
                tags::IN_CONCATENATION_TOTAL_NUMBER,
                concatenation.total,
            ));
            object.put(long(
                tags::CONCATENATION_FRAME_OFFSET_NUMBER,
                concatenation.frames.start as u32,
            ));
        }
    }

    fn patient_study_series(&self, object: &mut InMemDicomObject, study: &Study) {
        for (tag, vr, value) in [
            (tags::PATIENT_NAME, VR::PN, "FIXTURE^SLIDE"),
            (tags::PATIENT_ID, VR::LO, "FIXTURE"),
            (tags::PATIENT_BIRTH_DATE, VR::DA, ""),
            (tags::PATIENT_SEX, VR::CS, ""),
            (tags::STUDY_INSTANCE_UID, VR::UI, &study.study),
            (tags::STUDY_DATE, VR::DA, "20260102"),
            (tags::STUDY_TIME, VR::TM, "101500"),
            (tags::REFERRING_PHYSICIAN_NAME, VR::PN, ""),
            (tags::STUDY_ID, VR::SH, "1"),
            (tags::ACCESSION_NUMBER, VR::SH, "A1"),
            (tags::MODALITY, VR::CS, "SM"),
            (tags::SERIES_INSTANCE_UID, VR::UI, &study.series),
            (tags::SERIES_NUMBER, VR::IS, "1"),
        ] {
            put(object, tag, vr, value);
        }
        if self.flavor.registered() {
            put(
                object,
                tags::FRAME_OF_REFERENCE_UID,
                VR::UI,
                &study.frame_of_reference,
            );
            put(
                object,
                tags::POSITION_REFERENCE_INDICATOR,
                VR::LO,
                "SLIDE_CORNER",
            );
        }
    }

    fn equipment(&self, object: &mut InMemDicomObject) {
        for (tag, vr, value) in [
            (tags::MANUFACTURER, VR::LO, "Fixture Scanner Co"),
            (tags::MANUFACTURER_MODEL_NAME, VR::LO, "F1"),
            (tags::DEVICE_SERIAL_NUMBER, VR::LO, "SN1"),
            (tags::SOFTWARE_VERSIONS, VR::LO, "1.0"),
            (tags::CONTENT_DATE, VR::DA, "20260102"),
            (tags::CONTENT_TIME, VR::TM, "101500"),
            (tags::ACQUISITION_DATE_TIME, VR::DT, "20260102101500"),
        ] {
            put(object, tag, vr, value);
        }
    }

    fn image(&self, object: &mut InMemDicomObject) {
        let samples = self.samples();
        object.put(unsigned(tags::SAMPLES_PER_PIXEL, samples));
        put(
            object,
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            self.photometric,
        );
        if samples == 3 {
            object.put(unsigned(tags::PLANAR_CONFIGURATION, 0));
        }
        let frames = self.own_positions().len();
        put(object, tags::NUMBER_OF_FRAMES, VR::IS, &frames.to_string());
        object.put(unsigned(tags::ROWS, self.tile.0));
        object.put(unsigned(tags::COLUMNS, self.tile.1));
        object.put(unsigned(tags::BITS_ALLOCATED, self.bits));
        object.put(unsigned(tags::BITS_STORED, self.bits));
        object.put(unsigned(tags::HIGH_BIT, self.bits - 1));
        object.put(unsigned(tags::PIXEL_REPRESENTATION, 0));
        if self.photometric == "MONOCHROME2" {
            put(object, tags::PRESENTATION_LUT_SHAPE, VR::CS, "IDENTITY");
            put(object, tags::RESCALE_INTERCEPT, VR::DS, "0");
            put(object, tags::RESCALE_SLOPE, VR::DS, "1");
        }
        let label = matches!(self.flavor, Flavor::Label | Flavor::Overview);
        let yes_no = |value: bool| if value { "YES" } else { "NO" };
        for (tag, value) in [
            (tags::VOLUMETRIC_PROPERTIES, "VOLUME"),
            (tags::SPECIMEN_LABEL_IN_IMAGE, yes_no(label)),
            (tags::BURNED_IN_ANNOTATION, yes_no(label)),
            (tags::FOCUS_METHOD, "AUTO"),
            (
                tags::EXTENDED_DEPTH_OF_FIELD,
                yes_no(self.extended_depth_of_field),
            ),
        ] {
            put(object, tag, VR::CS, value);
        }
        if self.extended_depth_of_field {
            object.put(unsigned(tags::NUMBER_OF_FOCAL_PLANES, 5));
            object.put(float(tags::DISTANCE_BETWEEN_FOCAL_PLANES, 0.5));
        }
        if self.flavor == Flavor::Label {
            put(object, tags::BARCODE_VALUE, VR::LT, "FIXTURE-1");
            put(object, tags::LABEL_TEXT, VR::UT, "");
        }
        self.lossy_history(object);
    }

    /// The lossy coding of the fixture's encoders: JPEG DCT, JPEG-LS NEAR 3,
    /// the 9-7 irreversible JPEG 2000 wavelet, and JPEG data recompressed.
    fn lossy_history(&self, object: &mut InMemDicomObject) {
        let method = match self.syntax {
            uids::JPEG_BASELINE8_BIT
            | uids::JPEG_EXTENDED12_BIT
            | uids::JPEGXLJPEG_RECOMPRESSION => Some("ISO_10918_1"),
            uids::JPEGLS_NEAR_LOSSLESS => Some("ISO_14495_1"),
            uids::JPEG2000 | uids::JPEG2000MC => Some("ISO_15444_1"),
            uids::HTJ2K => Some("ISO_15444_15"),
            _ => None,
        };
        match method {
            Some(method) => {
                put(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "01");
                put(object, tags::LOSSY_IMAGE_COMPRESSION_RATIO, VR::DS, "10");
                put(object, tags::LOSSY_IMAGE_COMPRESSION_METHOD, VR::CS, method);
            }
            None => put(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "00"),
        }
    }

    fn specimen(&self, object: &mut InMemDicomObject, study: &Study) {
        put(object, tags::CONTAINER_IDENTIFIER, VR::LO, "SLIDE-1");
        object.put(sq(
            tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE,
            Vec::new(),
        ));
        object.put(sq(
            tags::CONTAINER_TYPE_CODE_SEQUENCE,
            vec![code("SCT", "433466003", "Microscope slide")],
        ));
        object.put(sq(tags::ACQUISITION_CONTEXT_SEQUENCE, Vec::new()));
        object.put(sq(
            tags::SPECIMEN_DESCRIPTION_SEQUENCE,
            vec![item(vec![
                text(tags::SPECIMEN_IDENTIFIER, VR::LO, "SPECIMEN-1"),
                text(tags::SPECIMEN_UID, VR::UI, &study.specimen),
                sq(tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE, Vec::new()),
                sq(tags::SPECIMEN_PREPARATION_SEQUENCE, Vec::new()),
            ])],
        ));
    }

    fn tile_organization(&self, object: &mut InMemDicomObject) {
        object.put(long(tags::TOTAL_PIXEL_MATRIX_COLUMNS, self.matrix.1));
        object.put(long(tags::TOTAL_PIXEL_MATRIX_ROWS, self.matrix.0));
        object.put(long(
            tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES,
            self.planes as u32,
        ));
        object.put(sq(
            tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
            vec![item(vec![
                text(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "20"),
                text(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "10"),
            ])],
        ));
        put(
            object,
            tags::IMAGE_ORIENTATION_SLIDE,
            VR::DS,
            "0\\-1\\0\\-1\\0\\0",
        );
        // No fixture encodes a tile twice, in any organization.
        put(object, tags::TILES_OVERLAP, VR::CS, "NONE");
        if self.flavor == Flavor::Volume {
            let width = self.matrix.1 as f64 * self.spacing_mm;
            let height = self.matrix.0 as f64 * self.spacing_mm;
            let depth = (self.planes as f64 * self.plane_spacing_um).max(1.0);
            object.put(float(tags::IMAGED_VOLUME_WIDTH, width as f32));
            object.put(float(tags::IMAGED_VOLUME_HEIGHT, height as f32));
            object.put(float(tags::IMAGED_VOLUME_DEPTH, depth as f32));
        }
    }

    fn optical_paths(&self, object: &mut InMemDicomObject) {
        object.put(long(tags::NUMBER_OF_OPTICAL_PATHS, self.paths.len() as u32));
        let items = self
            .paths
            .iter()
            .map(|path| self.optical_path(path))
            .collect();
        object.put(sq(tags::OPTICAL_PATH_SEQUENCE, items));
    }

    fn optical_path(&self, path: &OpticalPath) -> InMemDicomObject {
        let mut elements = vec![text(
            tags::OPTICAL_PATH_IDENTIFIER,
            VR::SH,
            &path.identifier,
        )];
        match path.wavelength {
            Some(wavelength) => {
                elements.push(sq(
                    tags::ILLUMINATION_TYPE_CODE_SEQUENCE,
                    vec![code("DCM", "111743", "Epifluorescence illumination")],
                ));
                elements.push(float(tags::ILLUMINATION_WAVE_LENGTH, wavelength));
            }
            None => {
                elements.push(sq(
                    tags::ILLUMINATION_TYPE_CODE_SEQUENCE,
                    vec![code("DCM", "111744", "Brightfield illumination")],
                ));
                elements.push(sq(
                    tags::ILLUMINATION_COLOR_CODE_SEQUENCE,
                    vec![code("SCT", "371251000", "White")],
                ));
            }
        }
        if path.icc || self.photometric != "MONOCHROME2" || path.palette {
            let profile = crate::icc::synthetic_srgb_icc_profile().expect("sRGB profile");
            elements.push(DataElement::new(
                tags::ICC_PROFILE,
                VR::OB,
                PrimitiveValue::from(profile),
            ));
        }
        if path.palette {
            elements.push(sq(
                tags::PALETTE_COLOR_LOOKUP_TABLE_SEQUENCE,
                vec![palette()],
            ));
        }
        item(elements)
    }

    fn dimensions(&self, object: &mut InMemDicomObject, study: &Study) {
        let uid = self.dimension_organization_uid(study);
        object.put(sq(
            tags::DIMENSION_ORGANIZATION_SEQUENCE,
            vec![item(vec![text(
                tags::DIMENSION_ORGANIZATION_UID,
                VR::UI,
                &uid,
            )])],
        ));
        let term = match self.organization {
            Organization::TiledFull { .. } => "TILED_FULL",
            Organization::TiledSparse => "TILED_SPARSE",
            Organization::ThreeD => "3D",
        };
        put(object, tags::DIMENSION_ORGANIZATION_TYPE, VR::CS, term);
        if self.dimensions.is_empty() {
            return;
        }
        let items = self
            .dimensions
            .iter()
            .map(|dimension| {
                let (pointer, group) = match dimension {
                    Dimension::Row => (
                        tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                        tags::PLANE_POSITION_SLIDE_SEQUENCE,
                    ),
                    Dimension::Column => (
                        tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                        tags::PLANE_POSITION_SLIDE_SEQUENCE,
                    ),
                    Dimension::Plane => (
                        tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                        tags::PLANE_POSITION_SLIDE_SEQUENCE,
                    ),
                    Dimension::Path => (
                        tags::OPTICAL_PATH_IDENTIFIER,
                        tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
                    ),
                };
                item(vec![
                    text(tags::DIMENSION_ORGANIZATION_UID, VR::UI, &uid),
                    pointer_element(tags::DIMENSION_INDEX_POINTER, pointer),
                    pointer_element(tags::FUNCTIONAL_GROUP_POINTER, group),
                ])
            })
            .collect();
        object.put(sq(tags::DIMENSION_INDEX_SEQUENCE, items));
    }

    fn dimension_organization_uid(&self, study: &Study) -> String {
        match &self.concatenation {
            Some(_) => format!("{}.7", study.concatenation),
            None => format!("{}.{}", self.sop_instance_uid(study), 7),
        }
    }

    /// Whether Frames carry their own position and optical path.
    fn explicit(&self) -> bool {
        !matches!(
            self.organization,
            Organization::TiledFull { explicit: false }
        )
    }

    fn functional_groups(&self, object: &mut InMemDicomObject) {
        let mut shared = Vec::new();
        let mut measures = Vec::new();
        if self.flavor != Flavor::Label {
            measures.push(text(
                tags::PIXEL_SPACING,
                VR::DS,
                &format!("{}\\{}", self.spacing_mm, self.spacing_mm),
            ));
        }
        if self.flavor.registered() {
            measures.push(text(tags::SLICE_THICKNESS, VR::DS, "0.001"));
        }
        if self.planes > 1 {
            measures.push(text(
                tags::SPACING_BETWEEN_SLICES,
                VR::DS,
                &format!("{}", self.plane_spacing_um / 1000.0),
            ));
        }
        shared.push(sq(tags::PIXEL_MEASURES_SEQUENCE, vec![item(measures)]));
        let image_type = object
            .element(tags::IMAGE_TYPE)
            .expect("Image Type")
            .to_str()
            .expect("Image Type text")
            .into_owned();
        shared.push(sq(
            tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
            vec![item(vec![text(tags::FRAME_TYPE, VR::CS, &image_type)])],
        ));
        if let Some(source) = &self.resampled_from {
            shared.push(sq(
                tags::DERIVATION_IMAGE_SEQUENCE,
                vec![derivation(source)],
            ));
        }
        let paths_per_frame = self.paths.len() > 1 || self.explicit();
        if !paths_per_frame {
            shared.push(path_identification(&self.paths[0].identifier));
        }
        object.put(sq(
            tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![item(shared)],
        ));
        if !self.explicit() && self.dimensions.is_empty() && !paths_per_frame {
            return;
        }
        let frames = self
            .own_positions()
            .into_iter()
            .map(|(_, position)| self.frame_group(position, paths_per_frame))
            .collect();
        object.put(sq(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, frames));
    }

    fn frame_group(&self, position: Position, path_per_frame: bool) -> InMemDicomObject {
        let mut elements = Vec::new();
        if !self.dimensions.is_empty() {
            let values: Vec<u32> = self
                .dimensions
                .iter()
                .map(|dimension| self.index(*dimension, position))
                .collect();
            elements.push(sq(
                tags::FRAME_CONTENT_SEQUENCE,
                vec![item(vec![DataElement::new(
                    tags::DIMENSION_INDEX_VALUES,
                    VR::UL,
                    PrimitiveValue::U32(values.into()),
                )])],
            ));
        }
        if self.explicit() {
            elements.push(sq(
                tags::PLANE_POSITION_SLIDE_SEQUENCE,
                vec![self.plane_position(position)],
            ));
        }
        if path_per_frame {
            elements.push(path_identification(&self.paths[position.path].identifier));
        }
        item(elements)
    }

    /// The ordinal of the indexed value among the values this image uses
    /// (C.7.6.17.1).
    fn index(&self, dimension: Dimension, position: Position) -> u32 {
        let key = |position: &Position| match dimension {
            Dimension::Row => position.row as usize,
            Dimension::Column => position.column as usize,
            Dimension::Plane => position.plane,
            Dimension::Path => position.path,
        };
        let mut used: Vec<usize> = self.positions().iter().map(key).collect();
        used.sort_unstable();
        used.dedup();
        used.binary_search(&key(&position)).expect("used value") as u32 + 1
    }

    fn plane_position(&self, position: Position) -> InMemDicomObject {
        let row = position.row * u32::from(self.tile.0) + 1;
        let column = position.column * u32::from(self.tile.1) + 1;
        // Image Orientation (Slide) 0\-1\0\-1\0\0: rows run along -Y, columns along -X.
        let x = 20.0 - f64::from(row - 1) * self.spacing_mm;
        let y = 10.0 - f64::from(column - 1) * self.spacing_mm;
        let z = position.plane as f64 * self.plane_spacing_um;
        item(vec![
            DataElement::new(
                tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                VR::SL,
                PrimitiveValue::from(column as i32),
            ),
            DataElement::new(
                tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                VR::SL,
                PrimitiveValue::from(row as i32),
            ),
            text(
                tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                VR::DS,
                &decimal(x),
            ),
            text(
                tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                VR::DS,
                &decimal(y),
            ),
            text(
                tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                VR::DS,
                &decimal(z),
            ),
        ])
    }

    fn pixel_data(&self, object: &mut InMemDicomObject) {
        let format = self.format();
        let frames: Vec<usize> = self
            .own_positions()
            .into_iter()
            .map(|(frame, _)| frame)
            .collect();
        let encoded: Option<Vec<Vec<u8>>> = frames
            .iter()
            .map(|frame| frames::encoded(self.syntax, self.photometric, format, *frame))
            .collect();
        match encoded {
            Some(fragments) => {
                let fragments = fragments.into_iter().map(even).collect::<Vec<_>>();
                object.put(DataElement::new(
                    tags::PIXEL_DATA,
                    VR::OB,
                    PixelFragmentSequence::new(Vec::<u32>::new(), fragments),
                ));
            }
            None => {
                let bytes: Vec<u8> = frames
                    .iter()
                    .flat_map(|frame| frames::native(format, *frame))
                    .collect();
                let vr = if self.bits == 16 { VR::OW } else { VR::OB };
                object.put(DataElement::new(
                    tags::PIXEL_DATA,
                    vr,
                    PrimitiveValue::U8(even(bytes).into()),
                ));
            }
        }
    }
}

/// Writes `object` to `dir/<name>.dcm`. The writer leaves an odd-length
/// deflated Data Set unpadded, so the NUL PS3.5 A.5 requires is appended; the
/// File Meta Information before it is always of even length.
pub(super) fn write(object: DefaultDicomObject, dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.dcm"));
    let deflated = object.meta().transfer_syntax() == uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN;
    object.write_to_file(&path).expect("write fixture");
    if deflated {
        let mut bytes = std::fs::read(&path).expect("read fixture");
        if bytes.len() % 2 == 1 {
            bytes.push(0);
            std::fs::write(&path, bytes).expect("pad fixture");
        }
    }
    path
}

fn even(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

fn decimal(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

/// A 256-entry 16-bit grey ramp Palette Color Lookup Table (C.7.9).
fn palette() -> InMemDicomObject {
    let descriptor = |tag| {
        DataElement::new(
            tag,
            VR::US,
            PrimitiveValue::U16([256, 0, 16].into_iter().collect()),
        )
    };
    let entries: Vec<u16> = (0..256u16).map(|value| value * 257).collect();
    let table = |tag| DataElement::new(tag, VR::OW, PrimitiveValue::U16(entries.clone().into()));
    item(vec![
        descriptor(tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR),
        descriptor(tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR),
        descriptor(tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR),
        table(tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA),
        table(tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA),
        table(tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA),
    ])
}

fn path_identification(identifier: &str) -> InMemElement {
    sq(
        tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
        vec![item(vec![text(
            tags::OPTICAL_PATH_IDENTIFIER,
            VR::SH,
            identifier,
        )])],
    )
}

fn referenced_instance(uid: &str) -> InMemDicomObject {
    item(vec![
        text(
            tags::REFERENCED_SOP_CLASS_UID,
            VR::UI,
            crate::VL_WSI_SOP_CLASS_UID,
        ),
        text(tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, uid),
    ])
}

/// A spatial resampling of `source` (C.7.6.16.2.6).
fn derivation(source: &str) -> InMemDicomObject {
    let mut reference = referenced_instance(source);
    reference.put(sq(
        tags::PURPOSE_OF_REFERENCE_CODE_SEQUENCE,
        vec![code(
            "DCM",
            "121322",
            "Source image for image processing operation",
        )],
    ));
    reference.put(text(tags::SPATIAL_LOCATIONS_PRESERVED, VR::CS, "YES"));
    item(vec![
        sq(
            tags::DERIVATION_CODE_SEQUENCE,
            vec![code("DCM", "113085", "Spatial resampling")],
        ),
        sq(tags::SOURCE_IMAGE_SEQUENCE, vec![reference]),
    ])
}

pub(super) fn put(object: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    object.put(text(tag, vr, value));
}

pub(super) fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    let values: Vec<String> = value.split('\\').map(str::to_string).collect();
    if value.is_empty() {
        DataElement::new(tag, vr, PrimitiveValue::Empty)
    } else if matches!(vr, VR::LT | VR::UT | VR::ST) {
        DataElement::new(tag, vr, PrimitiveValue::from(value))
    } else {
        DataElement::new(tag, vr, PrimitiveValue::Strs(values.into()))
    }
}

pub(super) fn unsigned(tag: Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

pub(super) fn long(tag: Tag, value: u32) -> InMemElement {
    DataElement::new(tag, VR::UL, PrimitiveValue::from(value))
}

pub(super) fn float(tag: Tag, value: f32) -> InMemElement {
    DataElement::new(tag, VR::FL, PrimitiveValue::from(value))
}

pub(super) fn pointer_element(tag: Tag, pointer: Tag) -> InMemElement {
    DataElement::new(
        tag,
        VR::AT,
        PrimitiveValue::Tags([pointer].into_iter().collect()),
    )
}

pub(super) fn item(elements: Vec<InMemElement>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

pub(super) fn sq(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

pub(super) fn code(scheme: &str, value: &str, meaning: &str) -> InMemDicomObject {
    item(vec![
        text(tags::CODE_VALUE, VR::SH, value),
        text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme),
        text(tags::CODE_MEANING, VR::LO, meaning),
    ])
}

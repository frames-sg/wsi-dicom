use super::{
    assemble_encapsulated_frames, doctor_dicom_environment_with_runner,
    fragment_payload_without_padding, run_named_command_check,
    staged_dicom3tools_probe_enabled_from, validate_dicom_path_with_runner, CommandCheckRequest,
    CommandOutcome, DoctorOptions, DoctorStatus, SystemCommandRunner, ValidationCommandRunner,
    ValidationOptions, ValidationProfile, ValidationStatus, VALIDATOR_SET_FILE_CHUNK_SIZE,
};
use crate::TransferSyntax;
use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Default)]
struct FakeRunner {
    commands: BTreeSet<String>,
    outcomes: BTreeMap<String, CommandOutcome>,
}

impl FakeRunner {
    fn with_command(mut self, name: &str) -> Self {
        self.commands.insert(name.to_string());
        self
    }

    fn with_outcome(mut self, command: &str, outcome: CommandOutcome) -> Self {
        self.outcomes.insert(command.to_string(), outcome);
        self
    }
}

impl ValidationCommandRunner for FakeRunner {
    fn find_command(&self, name: &str) -> Option<PathBuf> {
        self.commands.contains(name).then(|| PathBuf::from(name))
    }

    fn run(
        &self,
        program: &Path,
        args: &[OsString],
        _timeout: Duration,
        _max_output_bytes: usize,
    ) -> Result<CommandOutcome, std::io::Error> {
        let mut key = program.display().to_string();
        for arg in args {
            key.push(' ');
            key.push_str(&arg.to_string_lossy());
        }
        let outcome = self.outcomes.get(&key).cloned().unwrap_or(CommandOutcome {
            return_code: Some(0),
            elapsed_millis: 0,
            success: true,
            timed_out: false,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        });
        if outcome.success {
            for pair in args.windows(2) {
                if matches!(pair[0].to_str(), Some("-o" | "-outfile")) {
                    std::fs::write(PathBuf::from(&pair[1]), b"P6\n1 1\n255\n\x00\x00\x00")?;
                }
            }
        }
        Ok(outcome)
    }
}

mod codestream;
mod conformance;
mod digital_signatures;
mod doctor;
mod encoding;
mod iod;
mod orchestration;
mod pixel_structure;
mod profile_contract;
mod relations;
mod runner;
mod series_set;
mod wsi_image;

fn write_encapsulated_dicom(path: &Path, transfer_syntax: &str, frame: &[u8]) {
    let mut object = InMemDicomObject::new_empty();
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        PrimitiveValue::from("1.2.840.10008.5.1.4.1.1.77.1.6"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        PrimitiveValue::from("1.2.826.0.1.3680043.10.999.200"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::ROWS,
        VR::US,
        PrimitiveValue::from(1u16),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(1u16),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        PrimitiveValue::from("1"),
    ));
    let meta = FileMetaTableBuilder::new()
        .media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.77.1.6")
        .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.200")
        .transfer_syntax(transfer_syntax);
    let file = File::create(path).expect("create DICOM");
    let mut output = BufWriter::new(file);
    object
        .with_meta(meta)
        .expect("file meta")
        .write_all(&mut output)
        .expect("write DICOM object");
    crate::writer::write_encapsulated_pixel_data_from_frames(
        &mut output,
        &[u64::try_from(frame.len()).expect("frame len")],
        |_, output| output.write_all(frame),
    )
    .expect("write pixel data");
    output.flush().expect("flush DICOM");
}

fn write_primitive_pixel_dicom(path: &Path, transfer_syntax: &str, bytes: &[u8]) {
    write_primitive_pixel_dicom_with_geometry(path, transfer_syntax, bytes, 1, 1, 1, 8);
}

fn write_primitive_pixel_dicom_with_geometry(
    path: &Path,
    transfer_syntax: &str,
    bytes: &[u8],
    rows: u16,
    columns: u16,
    samples_per_pixel: u16,
    bits_allocated: u16,
) {
    const SECONDARY_CAPTURE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.7";
    let mut object = InMemDicomObject::new_empty();
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        PrimitiveValue::from(SECONDARY_CAPTURE_SOP_CLASS_UID),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        PrimitiveValue::from("1.2.826.0.1.3680043.10.999.201"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::ROWS,
        VR::US,
        PrimitiveValue::from(rows),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(columns),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SAMPLES_PER_PIXEL,
        VR::US,
        PrimitiveValue::from(samples_per_pixel),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::BITS_ALLOCATED,
        VR::US,
        PrimitiveValue::from(bits_allocated),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        PrimitiveValue::from("1"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::U8(bytes.to_vec().into()),
    ));
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(SECONDARY_CAPTURE_SOP_CLASS_UID)
                .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.201")
                .transfer_syntax(transfer_syntax),
        )
        .expect("file meta")
        .write_to_file(path)
        .expect("write primitive Pixel Data DICOM");
}

fn export_valid_color_wsi_for_validation(root: &Path, name: &str) -> PathBuf {
    export_valid_color_wsi_grid_for_validation(root, name, 2, 2, 2)
}

fn export_valid_color_wsi_grid_for_validation(
    root: &Path,
    name: &str,
    width: u32,
    height: u32,
    tile_size: u32,
) -> PathBuf {
    export_color_wsi(
        root,
        name,
        (width, height, tile_size),
        TransferSyntax::Jpeg2000Lossless,
    )
}

/// The first instance exported from a synthetic RGB source of `width` by
/// `height` pixels in `tile_size` tiles.
fn export_color_wsi(
    root: &Path,
    name: &str,
    (width, height, tile_size): (u32, u32, u32),
    transfer_syntax: TransferSyntax,
) -> PathBuf {
    let source = root.join(format!("{name}-source.dcm"));
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(width, height).unwrap();
    crate::synthetic_source::write_rgb_source_dicom(
        &source,
        "1.2.826.0.1.3680043.10.999.301",
        "1.2.826.0.1.3680043.10.999.302",
        width,
        height,
        pixels,
    )
    .unwrap();
    crate::export_dicom(crate::ExportRequest {
        source_path: source,
        output_dir: root.join(format!("{name}-out")),
        options: crate::ExportOptions {
            tile_size,
            transfer_syntax,
            encode_backend: crate::EncodeBackendPreference::CpuOnly,
            ..crate::ExportOptions::default()
        },
        color_management: crate::ColorManagement::SourceOrSrgb,
        metadata: crate::MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap()
    .instances
    .remove(0)
    .path
}

/// Exports a valid WSI, applies `edit`, and writes it back.
fn edited_wsi(
    root: &Path,
    name: &str,
    edit: impl FnOnce(&mut dicom_object::DefaultDicomObject),
) -> PathBuf {
    edited(export_valid_color_wsi_for_validation(root, name), edit)
}

/// Rewrites the DICOM file at `path` after applying `edit`.
fn edited(path: PathBuf, edit: impl FnOnce(&mut dicom_object::DefaultDicomObject)) -> PathBuf {
    let mut object = dicom_object::open_file(&path).unwrap();
    edit(&mut object);
    object.write_to_file(&path).unwrap();
    path
}

type Edit = fn(&mut dicom_object::DefaultDicomObject);

/// The status and message of check `rule` for the object at `path`.
fn rule_result(path: &Path, rule: &str) -> (ValidationStatus, String) {
    let report = validate_dicom_path_with_runner(
        path,
        &ValidationOptions::default(),
        &FakeRunner::default(),
    )
    .unwrap();
    let check = report
        .checks
        .into_iter()
        .find(|check| check.name == rule)
        .unwrap_or_else(|| panic!("missing check {rule}"));
    (check.status, check.message)
}

fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::from(value))
}

fn item(elements: Vec<InMemElement>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

fn sq(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

fn code(scheme: &str, value: &str, meaning: &str) -> InMemDicomObject {
    item(vec![
        text(tags::CODE_VALUE, VR::SH, value),
        text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme),
        text(tags::CODE_MEANING, VR::LO, meaning),
    ])
}

/// Applies `edit` to the Items of the Sequence `tag` of `object`.
fn edit_items(object: &mut InMemDicomObject, tag: Tag, edit: impl FnOnce(&mut [InMemDicomObject])) {
    let mut element = object.take(tag).unwrap();
    edit(element.items_mut().unwrap());
    object.put(element);
}

fn first_item(object: &mut InMemDicomObject, tag: Tag, edit: impl FnOnce(&mut InMemDicomObject)) {
    edit_items(object, tag, |items| edit(&mut items[0]));
}

fn assert_intrinsic_rule_fails(path: &Path, name: &str) {
    let report = validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.name == name && check.status == ValidationStatus::Failed }),
        "missing failed intrinsic check {name}"
    );
}

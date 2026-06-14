#![no_main]

use libfuzzer_sys::fuzz_target;
use wsi_dicom::DicomMetadata;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) {
        let _ = DicomMetadata::from_fhir_r4_bundle(&value);
    }
});

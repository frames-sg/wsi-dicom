#![no_main]

use libfuzzer_sys::fuzz_target;
use wsi_dicom::bench_support::validation_fragment_payload_len_for_bench;

fuzz_target!(|data: &[u8]| {
    let _ = validation_fragment_payload_len_for_bench(data);
});

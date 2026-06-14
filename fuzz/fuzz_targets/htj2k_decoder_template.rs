#![no_main]

use libfuzzer_sys::fuzz_target;
use wsi_dicom::bench_support::htj2k_decoder_template_summary_for_bench;

fuzz_target!(|data: &[u8]| {
    if let Ok(template) = std::str::from_utf8(data) {
        let _ = htj2k_decoder_template_summary_for_bench(template);
    }
});

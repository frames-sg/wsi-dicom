mod encoding;
mod frame_index;
mod functional_groups;
mod object_construction;
mod persistence;
mod pixel_data;

#[cfg(test)]
use crate::icc::{synthetic_display_p3_icc_profile, synthetic_srgb_icc_profile};
pub(crate) use crate::lossy::LossyCompressionHistory;
#[cfg(test)]
use encoding::format_ds;
#[cfg(test)]
use frame_index::FrameIndexSpool;
#[cfg(test)]
use functional_groups::{checked_dimension_index_value, per_frame_items};
pub(crate) use functional_groups::{FrameGrid, PerFrameFunctionalGroupsPlan};
pub(crate) use object_construction::{
    build_dicom_object, DicomObjectIdentifiers, DicomObjectParams,
};
#[cfg(any(test, feature = "bench-internals"))]
pub(crate) use pixel_data::pixel_data_offsets_from_lengths;
#[cfg(test)]
use pixel_data::{
    dicom_file_writer, write_dicom_object_with_pixel_data, DICOM_FILE_WRITE_BUFFER_BYTES,
};
pub(crate) use pixel_data::{
    extended_offset_table_metadata_bytes, unique_spool_path,
    write_dicom_object_with_streamed_pixel_data, BufferedPixelDataSink, PixelDataSink,
    PixelDataSpool, StreamedDicomWritePlan,
};
#[cfg(test)]
pub(crate) use pixel_data::{
    write_dicom_object_with_spooled_pixel_data, write_encapsulated_pixel_data_from_frames,
    write_encapsulated_pixel_data_from_spool, SpooledPixelDataFragment,
};

#[cfg(test)]
mod tests;

use crate::Error;
use j2k_metal_support::{MetalImageLayout, ResidentMetalImage, SubmittedMetalImages};
use metal::{BlitCommandEncoderRef, Buffer, CommandBuffer, ComputeCommandEncoderRef, DeviceRef};

pub(crate) fn support_error(
    context: &'static str,
    source: j2k_metal_support::MetalSupportError,
) -> Error {
    Error::Encode {
        message: format!("{context}: {source}"),
    }
}

pub(crate) fn device_tile_image(
    tile: &wsi_rs::output::metal::MetalDeviceTile,
) -> Result<&ResidentMetalImage, Error> {
    tile.validated_resident_image()
        .map_err(|source| Error::Unsupported {
            reason: format!("Metal device tile is not a validated resident image: {source}"),
        })
}

pub(crate) fn bind_resident_compute_input(
    encoder: &ComputeCommandEncoderRef,
    index: u64,
    image: &ResidentMetalImage,
) {
    // SAFETY: this audited operation binds the logically immutable resident
    // allocation for a GPU read. The submission owner separately retains the
    // image through completion.
    encoder.set_buffer(
        index,
        Some(unsafe { image.raw_buffer() }),
        image.byte_offset() as u64,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn copy_resident_rows(
    encoder: &BlitCommandEncoderRef,
    image: &ResidentMetalImage,
    source_offset: u64,
    source_pitch: u64,
    destination: &Buffer,
    destination_offset: u64,
    destination_pitch: u64,
    row_bytes: u64,
    height: u64,
) {
    // SAFETY: the immutable input is read only, and the submission retains it
    // through completion. The destination is a fresh owned output allocation.
    let source = unsafe { image.raw_buffer() };
    for row in 0..height {
        encoder.copy_from_buffer(
            source,
            source_offset + row * source_pitch,
            destination,
            destination_offset + row * destination_pitch,
            row_bytes,
        );
    }
}

pub(crate) fn submit_images(
    device: &DeviceRef,
    command_buffer: CommandBuffer,
    outputs: Vec<(Buffer, MetalImageLayout)>,
    inputs: Vec<ResidentMetalImage>,
) -> Result<SubmittedMetalImages, Error> {
    // SAFETY: pack/compose callers pass fresh output allocations whose only
    // writers are encoded in this command buffer, plus every bound resident
    // input as a keepalive.
    unsafe { SubmittedMetalImages::from_uncommitted(device, command_buffer, outputs, inputs) }
        .map_err(|source| support_error("Metal image submission", source))
}

#[cfg(test)]
pub(crate) fn test_tile_from_shared_bytes(
    device: &DeviceRef,
    bytes: &[u8],
    width: u32,
    height: u32,
    format: j2k_core::PixelFormat,
) -> wsi_rs::output::metal::MetalDeviceTile {
    let pitch_bytes = width as usize * format.bytes_per_pixel();
    let buffer = j2k_metal_support::checked_shared_buffer_with_slice(device, bytes)
        .expect("test Metal upload");
    test_tile_from_completed_buffer(
        buffer,
        0,
        width,
        height,
        pitch_bytes,
        wsi_rs::PixelFormat::try_from(format).expect("supported test Metal pixel format"),
    )
}

#[cfg(test)]
pub(crate) fn test_tile_from_completed_buffer(
    buffer: Buffer,
    byte_offset: usize,
    width: u32,
    height: u32,
    pitch_bytes: usize,
    format: wsi_rs::PixelFormat,
) -> wsi_rs::output::metal::MetalDeviceTile {
    // SAFETY: the synchronous upload is complete, and no writable raw handle
    // survives the move into the resident image.
    unsafe {
        wsi_rs::output::metal::MetalDeviceTile::from_completed_buffer(
            buffer,
            byte_offset,
            width,
            height,
            pitch_bytes,
            format,
        )
    }
    .expect("test resident Metal tile")
}

#[cfg(test)]
pub(crate) fn test_tile_bytes(tile: &wsi_rs::output::metal::MetalDeviceTile) -> Vec<u8> {
    let image = device_tile_image(tile).expect("test resident Metal tile");
    // SAFETY: test callers inspect only completed shared-memory outputs, and
    // this snapshot does not retain a mutable pointer or mutate the allocation.
    unsafe {
        j2k_metal_support::checked_buffer_read_vec::<u8>(
            image.raw_buffer(),
            image.byte_offset(),
            image.byte_len(),
        )
    }
    .expect("test resident Metal readback")
}

#[cfg(test)]
pub(crate) fn test_u64_buffer_values(buffer: &Buffer, len: usize) -> Vec<u64> {
    // SAFETY: the test command has completed and the shared output is read
    // only while the snapshot is created.
    unsafe { j2k_metal_support::checked_buffer_read_vec::<u64>(buffer, 0, len) }
        .expect("test Metal u64 readback")
}

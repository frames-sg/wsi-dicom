use crate::Error;
use j2k_metal_support::{MetalImageLayout, ResidentMetalImage, SubmittedMetalImages};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::{
    MTLBlitCommandEncoder, MTLBuffer, MTLCommandBuffer, MTLComputeCommandEncoder, MTLDevice,
};

pub(crate) type MetalBuffer = wsi_rs::output::metal::MetalBuffer;
pub(crate) type MetalDevice = wsi_rs::output::metal::MetalDevice;
type CommandBuffer = Retained<ProtocolObject<dyn MTLCommandBuffer>>;

// SAFETY: a Metal-backed encoded frame is constructed only after its writer
// has completed. Its public operations read immutable codestream bytes or
// consume the retained buffer; all other frame fields are ordinary Send data.
unsafe impl Send for crate::encode::EncodedDicomJ2kFrame {}

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
    encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>,
    index: usize,
    image: &ResidentMetalImage,
) {
    assert!(index < 31, "Metal buffer index exceeds the binding table");
    // SAFETY: the binding index is part of the fixed shader ABI, the offset
    // was validated by `ResidentMetalImage`, and support-created command
    // buffers retain the immutable input through completion.
    unsafe {
        encoder.setBuffer_offset_atIndex(Some(image.raw_buffer()), image.byte_offset(), index)
    };
}

pub(crate) fn bind_compute_buffer(
    encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>,
    index: usize,
    buffer: &ProtocolObject<dyn MTLBuffer>,
) {
    assert!(index < 31, "Metal buffer index exceeds the binding table");
    // SAFETY: every call site uses this fresh or immutable allocation according
    // to its fixed shader ABI. The support-created retaining command buffer
    // keeps the resource alive through completion.
    unsafe { encoder.setBuffer_offset_atIndex(Some(buffer), 0, index) };
}

pub(crate) fn bind_compose_params(
    encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>,
    index: usize,
    params: &crate::export::metal_compose::MetalComposeStripsParams,
) {
    assert!(index < 31, "Metal byte index exceeds the binding table");
    let pointer = std::ptr::NonNull::from(params).cast();
    // SAFETY: `MetalComposeStripsParams` is `repr(C)` and contains fifteen
    // initialized `u32` fields with no padding. Metal copies the bytes during
    // this call, and the fixed shader ABI uses the same layout and index.
    unsafe {
        encoder.setBytes_length_atIndex(
            pointer,
            core::mem::size_of::<crate::export::metal_compose::MetalComposeStripsParams>(),
            index,
        )
    };
}

#[cfg(test)]
pub(crate) fn bind_probe_coordinate(
    encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>,
    index: usize,
    coordinate: &[u32; 2],
) {
    assert!(index < 31, "Metal byte index exceeds the binding table");
    let pointer = std::ptr::NonNull::from(coordinate).cast();
    // SAFETY: the two initialized `u32` values exactly match the probe
    // shader's `uint2` binding, and Metal copies them synchronously.
    unsafe { encoder.setBytes_length_atIndex(pointer, core::mem::size_of_val(coordinate), index) };
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn copy_resident_rows(
    encoder: &ProtocolObject<dyn MTLBlitCommandEncoder>,
    image: &ResidentMetalImage,
    source_offset: u64,
    source_pitch: u64,
    destination: &ProtocolObject<dyn MTLBuffer>,
    destination_offset: u64,
    destination_pitch: u64,
    row_bytes: u64,
    height: u64,
) {
    let source_offset = usize::try_from(source_offset).expect("Metal source offset fits usize");
    let source_pitch = usize::try_from(source_pitch).expect("Metal source pitch fits usize");
    let destination_offset =
        usize::try_from(destination_offset).expect("Metal destination offset fits usize");
    let destination_pitch =
        usize::try_from(destination_pitch).expect("Metal destination pitch fits usize");
    let row_bytes = usize::try_from(row_bytes).expect("Metal row byte count fits usize");
    let height = usize::try_from(height).expect("Metal row count fits usize");
    // SAFETY: `image` validated the source allocation and both row ranges were
    // preflighted by the pack plan. The input is read only, the destination is
    // fresh, and the submission retains both resources through completion.
    let source = unsafe { image.raw_buffer() };
    for row in 0..height {
        unsafe {
            encoder.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                source,
                source_offset + row * source_pitch,
                destination,
                destination_offset + row * destination_pitch,
                row_bytes,
            )
        };
    }
}

pub(crate) fn submit_images(
    device: &ProtocolObject<dyn MTLDevice>,
    command_buffer: CommandBuffer,
    outputs: Vec<(MetalBuffer, MetalImageLayout)>,
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
    device: &ProtocolObject<dyn MTLDevice>,
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
    buffer: MetalBuffer,
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
pub(crate) fn test_u64_buffer_values(
    buffer: &ProtocolObject<dyn MTLBuffer>,
    len: usize,
) -> Vec<u64> {
    // SAFETY: the test command has completed and the shared output is read
    // only while the snapshot is created.
    unsafe { j2k_metal_support::checked_buffer_read_vec::<u64>(buffer, 0, len) }
        .expect("test Metal u64 readback")
}

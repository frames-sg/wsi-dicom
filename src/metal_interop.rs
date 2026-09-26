use crate::Error;
use j2k_metal_support::{MetalImageLayout, ResidentMetalImage};
use objc2::{rc::Retained, runtime::ProtocolObject, Message};
use objc2_metal::{
    MTLBlitCommandEncoder, MTLBuffer, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandQueue,
    MTLComputeCommandEncoder, MTLDevice, MTLResource,
};

pub(crate) type MetalBuffer = wsi_rs::output::metal::MetalBuffer;
pub(crate) type MetalDevice = wsi_rs::output::metal::MetalDevice;
pub(crate) type CommandBuffer = Retained<ProtocolObject<dyn MTLCommandBuffer>>;
pub(crate) type CommandQueue = Retained<ProtocolObject<dyn MTLCommandQueue>>;

pub(crate) struct FreshMetalImageOutput {
    buffer: MetalBuffer,
    layout: MetalImageLayout,
}

impl FreshMetalImageOutput {
    pub(crate) fn allocate(
        device: &ProtocolObject<dyn MTLDevice>,
        byte_len: usize,
        layout: MetalImageLayout,
    ) -> Result<Self, j2k_metal_support::MetalSupportError> {
        Ok(Self {
            buffer: j2k_metal_support::checked_shared_buffer_for_len::<u8>(device, byte_len)?,
            layout,
        })
    }

    pub(crate) fn buffer(&self) -> &ProtocolObject<dyn MTLBuffer> {
        &self.buffer
    }
}

pub(crate) struct MetalProducerDependency {
    queue: CommandQueue,
    command_buffer: Option<CommandBuffer>,
    _inputs: Vec<ResidentMetalImage>,
}

// SAFETY: the dependency owns its command queue, command buffer, and immutable
// input keepalives. Completion is the only mutation; Metal command queues and
// command buffers are cross-thread objects.
unsafe impl Send for MetalProducerDependency {}

impl MetalProducerDependency {
    pub(crate) fn validate_consumer_session(
        &self,
        session: &j2k_metal::MetalBackendSession,
    ) -> Result<(), Error> {
        if session
            .uses_command_queue(&self.queue)
            .map_err(|source| Error::Encode {
                message: format!("JPEG 2000 Metal producer queue validation failed: {source}"),
            })?
        {
            return Ok(());
        }
        Err(Error::Unsupported {
            reason:
                "pending Metal images require the encoder session to use the producer command queue"
                    .into(),
        })
    }

    fn complete(&mut self) -> Result<(), j2k_metal_support::MetalSupportError> {
        let Some(command_buffer) = self.command_buffer.take() else {
            return Ok(());
        };
        if matches!(
            command_buffer.status(),
            MTLCommandBufferStatus::NotEnqueued | MTLCommandBufferStatus::Enqueued
        ) {
            command_buffer.commit();
        }
        j2k_metal_support::wait_for_completion(&command_buffer)
    }

    pub(crate) fn wait(mut self) -> Result<(), Error> {
        self.complete()
            .map_err(|source| support_error("Metal input producer completion", source))
    }
}

impl Drop for MetalProducerDependency {
    fn drop(&mut self) {
        if let Err(error) = self.complete() {
            eprintln!("wsi-dicom: Metal input producer failed while being dropped: {error}");
        }
    }
}

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

pub(crate) fn upload_image(
    device: &ProtocolObject<dyn MTLDevice>,
    bytes: &[u8],
    layout: MetalImageLayout,
) -> Result<ResidentMetalImage, Error> {
    let buffer = j2k_metal_support::checked_shared_buffer_with_slice(device, bytes)
        .map_err(|source| support_error("Metal frame upload", source))?;
    // SAFETY: the synchronous upload initialized this fresh allocation. The raw
    // handle is moved into the immutable image; no writable alias survives.
    unsafe { ResidentMetalImage::from_completed_buffer(buffer, layout) }
        .map_err(|source| support_error("Metal uploaded frame layout", source))
}

pub(crate) fn resident_allocation_identity(image: &ResidentMetalImage) -> (usize, usize) {
    // SAFETY: inspect the immutable allocation identity/length only. The image
    // retains it; no contents or writable handle escapes this function.
    let buffer = unsafe { image.raw_buffer() };
    (
        std::ptr::from_ref(buffer).cast::<()>() as usize,
        buffer.length(),
    )
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
    if source_pitch == row_bytes && destination_pitch == row_bytes {
        let length = row_bytes
            .checked_mul(height)
            .expect("validated Metal copy span");
        // SAFETY: the validated rows form one contiguous span in both allocations.
        unsafe {
            encoder.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                source,
                source_offset,
                destination,
                destination_offset,
                length,
            )
        };
        return;
    }
    for row in 0..height {
        // SAFETY: the preflighted pitched row spans lie within both allocations.
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

/// Couples fresh producer outputs to their completion token for an immediately
/// submitted consumer on the exact same Metal command queue. The opaque return
/// value cannot expose its device tiles until the producer has completed.
pub(crate) fn submit_images_for_same_queue_consumer(
    device: &ProtocolObject<dyn MTLDevice>,
    producer_queue: &ProtocolObject<dyn MTLCommandQueue>,
    consumer_queue: &ProtocolObject<dyn MTLCommandQueue>,
    command_buffer: CommandBuffer,
    outputs: Vec<FreshMetalImageOutput>,
    inputs: Vec<ResidentMetalImage>,
) -> Result<crate::encode::PendingDicomJ2kMetalTileBatch, Error> {
    if outputs.is_empty() {
        return Err(Error::Encode {
            message: "Metal image submission requires at least one output".into(),
        });
    }
    let registry_id = device.registryID();
    for requested_registry_id in [
        producer_queue.device().registryID(),
        consumer_queue.device().registryID(),
    ] {
        if requested_registry_id != registry_id {
            return Err(support_error(
                "Metal pending image queue device",
                j2k_metal_support::MetalSupportError::MetalImageDeviceMismatch {
                    image_registry_id: registry_id,
                    requested_registry_id,
                },
            ));
        }
    }
    if !core::ptr::eq(producer_queue, consumer_queue)
        || !core::ptr::eq(command_buffer.commandQueue().as_ref(), producer_queue)
    {
        return Err(Error::Unsupported {
            reason: "pending Metal images require the producer and consumer to use the exact same command queue"
                .into(),
        });
    }
    for input in &inputs {
        input
            .validate_device(device)
            .map_err(|source| support_error("Metal pending image input device", source))?;
    }

    let mut tiles = Vec::new();
    tiles
        .try_reserve_exact(outputs.len())
        .map_err(|_| Error::Unsupported {
            reason: "pending Metal tile batch exceeds available memory".into(),
        })?;
    for FreshMetalImageOutput { buffer, layout } in outputs {
        if buffer.device().registryID() != registry_id {
            return Err(support_error(
                "Metal pending image output device",
                j2k_metal_support::MetalSupportError::MetalImageDeviceMismatch {
                    image_registry_id: buffer.device().registryID(),
                    requested_registry_id: registry_id,
                },
            ));
        }
        // SAFETY: `FreshMetalImageOutput` owns an allocation created by this
        // module and exposes only a borrowed handle for command encoding. The
        // caller has supplied the command buffer on the validated producer
        // queue; the opaque pending batch retains that command and never
        // exposes a ready tile before producer completion.
        let image = unsafe { ResidentMetalImage::from_exclusive_pending_buffer(buffer, layout) }
            .map_err(|source| support_error("Metal pending image layout", source))?;
        tiles.push(
            wsi_rs::output::metal::MetalDeviceTile::from_resident(image).map_err(|source| {
                Error::Encode {
                    message: format!("Metal composed resident tile conversion failed: {source}"),
                }
            })?,
        );
    }

    command_buffer.commit();
    Ok(crate::encode::PendingDicomJ2kMetalTileBatch::new(
        tiles,
        MetalProducerDependency {
            queue: producer_queue.retain(),
            command_buffer: Some(command_buffer),
            _inputs: inputs,
        },
    ))
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
pub(crate) fn test_buffer_bytes(buffer: &ProtocolObject<dyn MTLBuffer>, len: usize) -> Vec<u8> {
    // SAFETY: test callers wait for GPU writes to complete before this read;
    // the snapshot owns its bytes and retains no pointer into the allocation.
    unsafe { j2k_metal_support::checked_buffer_read_vec::<u8>(buffer, 0, len) }
        .expect("test Metal byte readback")
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

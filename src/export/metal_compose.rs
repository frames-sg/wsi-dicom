use super::*;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::{MTLCommandQueue, MTLComputePipelineState};

mod addressing;
mod compose;
mod pack;
mod types;

pub(crate) use types::MetalComposeStripsParams;
pub(super) use types::{MetalComposeTileRequest, PackedMetalStrips};

type MetalCommandQueue = Retained<ProtocolObject<dyn MTLCommandQueue>>;
type MetalComputePipeline = Retained<ProtocolObject<dyn MTLComputePipelineState>>;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalStripComposer {
    pub(super) device: crate::metal_interop::MetalDevice,
    pub(super) queue: MetalCommandQueue,
    pub(super) loader: j2k_metal_support::MetalPipelineLoader,
    pub(super) pipeline_u32: MetalComputePipeline,
    pub(super) pipeline_u64: OnceLock<Result<MetalComputePipeline, String>>,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_profile_stages_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("J2K_METAL_PROFILE_STAGES"),
            Ok(value) if value == "1"
        )
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalStripComposer {
    pub(super) fn new(device: crate::metal_interop::MetalDevice) -> Result<Self, Error> {
        let loader = j2k_metal_support::MetalPipelineLoader::new(&device, WSI_COMPOSE_STRIPS_METAL)
            .map_err(|source| {
                crate::metal_interop::support_error("Metal strip compose shader", source)
            })?;
        let pipeline_u32 = loader
            .pipeline("wsi_compose_strips_u32")
            .map_err(|source| {
                crate::metal_interop::support_error("Metal u32 strip compose pipeline", source)
            })?;
        let queue = j2k_metal_support::checked_command_queue(&device).map_err(|source| {
            crate::metal_interop::support_error("Metal strip compose command queue", source)
        })?;
        Ok(Self {
            device,
            queue,
            loader,
            pipeline_u32,
            pipeline_u64: OnceLock::new(),
        })
    }

    pub(super) fn pipeline_u64(
        &self,
    ) -> Result<&ProtocolObject<dyn MTLComputePipelineState>, Error> {
        self.pipeline_u64
            .get_or_init(|| {
                self.loader
                    .pipeline("wsi_compose_strips")
                    .map_err(|source| {
                        format!("Metal u64 strip compose pipeline unavailable: {source}")
                    })
            })
            .as_deref()
            .map_err(|message| Error::Encode {
                message: message.clone(),
            })
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const WSI_COMPOSE_STRIPS_METAL: &str = include_str!("metal_compose/compose.metal");

#[cfg(all(test, feature = "metal", target_os = "macos"))]
mod tests;

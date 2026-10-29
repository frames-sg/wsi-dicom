//! What a raw tile from wsi-rs may be used for.
//!
//! `Slide::read_raw_compressed_tile` returns a tile's stored bytes. The
//! encoded image can be larger than the tile ([`RawCompressedTile::tile_region`]
//! locates the tile in it), its samples can carry fewer significant bits than
//! they occupy, and a read can be refused for want of stored bytes or memory,
//! or because the stored JPEG is smaller than its tile. These helpers decide
//! when such a tile may be copied as an output frame and when its frame takes
//! the decode route instead.

use wsi_rs::{Compression, RawCompressedTile, Slide, TileLocation, TileRequest, WsiError};

use super::frame_region::FrameExtent;
use crate::error::Error;
use crate::lossy::{uncompressed_pixel_bytes, LossyCompressionByteCounts, JPEG_BASELINE_METHOD};

/// How a wsi-rs tile read error reason for a stored TIFF JPEG smaller than its
/// tile starts and continues, as in "TIFF JPEG tile is 32x16, smaller than its
/// 32x32 tile". wsi-rs reports it only as message text.
const WSI_RS_UNDERSIZED_JPEG_REASON: (&str, &str) = ("TIFF JPEG tile is ", ", smaller than its ");

/// Whether `raw`'s encoded image, copied unchanged, is a correct `frame`.
///
/// A copy puts the encoded image's top-left pixel at the frame origin, so
/// the tile must start at the image origin and cover every pixel of the
/// frame inside the Total Pixel Matrix. wsi-rs reports other tiles by their
/// region: a JPEG crop that starts at an earlier MCU boundary, or the whole
/// stored JPEG, JPEG 2000 codestream or JPEG XR subblock a tile is cut
/// from. Those take the decode route.
///
/// A tile region smaller than its image at the image origin is accepted
/// only when the image is exactly one frame and the region covers the
/// frame's in-matrix part: a right or bottom edge frame, such as a DICOM
/// TILED_FULL edge frame, whose pixels past the region are padding outside
/// the matrix. The same region in an interior frame would put padding
/// inside the matrix.
///
/// The writer declares Bits Stored equal to Bits Allocated, so a tile whose
/// samples carry fewer significant bits (DICOM Bits Stored 12 of 16) is not
/// copied either: its unused high bits would become pixel data.
pub(crate) fn raw_tile_copies_as_frame(raw: &RawCompressedTile, frame: FrameExtent) -> bool {
    let region = raw.tile_region();
    let image = (raw.width(), raw.height());
    let whole_image = (region.x, region.y, region.width, region.height) == (0, 0, image.0, image.1);
    let one_frame_image = image == (frame.columns, frame.rows);
    region.x == 0
        && region.y == 0
        && region.width <= frame.columns
        && region.height <= frame.rows
        && region.width >= frame.inside_columns
        && region.height >= frame.inside_rows
        && (whole_image || one_frame_image)
        && raw
            .significant_bits()
            .is_none_or(|bits| bits == raw.bits_allocated())
}

/// Whether a failed raw tile read only means the tile has no stored form to
/// use, so its frame takes the decode route.
///
/// wsi-rs refuses tiles with no stored bytes as `Unsupported`. It builds some
/// raw tiles it used to refuse, such as lossless crops of a larger JPEG, and
/// refuses those as `ResourceLimit` when building one exceeds the read's
/// memory reservation. It refuses a stored JPEG smaller than its tile as a
/// tile read error (see [`undersized_jpeg_tile`]). The decode route serves all
/// of these. Other errors are read failures, including a stored JPEG 2000
/// codestream smaller than its tile, which wsi-rs cannot decode either.
pub(crate) fn raw_tile_read_declined(error: &WsiError) -> bool {
    matches!(
        error,
        WsiError::Unsupported { .. } | WsiError::ResourceLimit { .. }
    ) || undersized_jpeg_tile(error)
}

/// Whether `error` refuses a stored TIFF JPEG whose image is smaller than the
/// tile it holds, such as an edge tile encoding fewer rows than the level has
/// left. wsi-rs decodes such a tile padded. It still holds JPEG pixels, so its
/// frame is a compressed-source re-encode under the pixel re-encode policy,
/// with the source's lossy history.
pub(crate) fn undersized_jpeg_tile(error: &WsiError) -> bool {
    let (prefix, size_clause) = WSI_RS_UNDERSIZED_JPEG_REASON;
    matches!(
        error,
        WsiError::TileRead { reason, .. }
            if reason.starts_with(prefix) && reason.contains(size_clause)
    )
}

/// JPEG lossy byte counts of the undersized stored tile at `request`, from
/// its stored location: the samples of the stored image, at the size its
/// container declares, against its stored bytes. A raw read measures a padded
/// edge tile the same way. The location does not read the JPEG frame header,
/// so the ratio is an upper bound for a JPEG smaller than its declared size.
/// `None` when the location is not one stored JPEG.
pub(crate) fn undersized_jpeg_tile_lossy_counts(
    slide: &Slide,
    request: &TileRequest,
) -> Result<Option<LossyCompressionByteCounts>, Error> {
    let location = slide
        .tile_location(request)
        .map_err(|source| Error::SlideRead {
            message: format!(
                "location of undersized JPEG tile ({}, {}): {source}",
                request.col, request.row
            ),
        })?;
    let TileLocation::Stored(stored) = location else {
        return Ok(None);
    };
    let Some(unit) = stored.single_unit() else {
        return Ok(None);
    };
    let codec = unit.codec();
    let Some(compressed_bytes) = unit.stored_len() else {
        return Ok(None);
    };
    if codec.compression() != Compression::Jpeg {
        return Ok(None);
    }
    let (width, height) = unit.stored_size();
    let uncompressed_bytes = uncompressed_pixel_bytes(
        u64::from(width),
        u64::from(height),
        u64::from(codec.samples_per_pixel()),
        codec.bits_per_sample(),
    )?;
    LossyCompressionByteCounts::new(JPEG_BASELINE_METHOD, uncompressed_bytes, compressed_bytes)
        .map(Some)
}

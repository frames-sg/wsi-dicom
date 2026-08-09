use std::io::{self, Write};

#[cfg(test)]
use dicom_core::value::DataSetSequence;
use dicom_core::Tag;
#[cfg(test)]
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
#[cfg(test)]
use dicom_object::InMemDicomObject;

use super::encoding::{format_ds, write_item_header, write_tag};
use crate::Error;

const PER_FRAME_SEQUENCE_CONTAINER_BYTES: u64 = 20;
const MIN_PER_FRAME_ITEM_BYTES: u64 = 158;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameGrid {
    pub(crate) frame_columns: u32,
    pub(crate) frame_rows: u32,
    pub(crate) matrix_columns: u64,
    pub(crate) matrix_rows: u64,
}

impl FrameGrid {
    pub(super) fn validate(self) -> Result<(), Error> {
        if self.frame_columns == 0 || self.frame_rows == 0 {
            return Err(Error::Unsupported {
                reason: "DICOM per-frame positions require non-zero frame dimensions".into(),
            });
        }
        if self.matrix_columns == 0 || self.matrix_rows == 0 {
            return Err(Error::Unsupported {
                reason: "DICOM total pixel matrix requires non-zero dimensions".into(),
            });
        }
        Ok(())
    }

    fn tiles_across(self) -> Result<u64, Error> {
        self.validate()?;
        Ok(self.matrix_columns.div_ceil(u64::from(self.frame_columns)))
    }

    fn location_for_frame(self, frame_index: u32) -> Result<FrameLocation, Error> {
        let tiles_across = self.tiles_across()?;
        Ok(FrameLocation {
            row: u64::from(frame_index) / tiles_across,
            column: u64::from(frame_index) % tiles_across,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameLocation {
    row: u64,
    column: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PerFrameFunctionalGroupsPlan {
    frame_count: u32,
    frame_grid: FrameGrid,
    row_spacing_mm: f64,
    column_spacing_mm: f64,
}

impl PerFrameFunctionalGroupsPlan {
    pub(crate) fn new(
        frame_count: u32,
        frame_grid: FrameGrid,
        row_spacing_mm: f64,
        column_spacing_mm: f64,
    ) -> Result<Self, Error> {
        frame_grid.validate()?;
        if frame_count == 0 {
            return Err(Error::Unsupported {
                reason: "DICOM per-frame functional groups require at least one frame".into(),
            });
        }
        for (name, value) in [
            ("row pixel spacing", row_spacing_mm),
            ("column pixel spacing", column_spacing_mm),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(Error::Metadata {
                    reason: format!("{name} must be finite and greater than zero"),
                });
            }
        }
        Ok(Self {
            frame_count,
            frame_grid,
            row_spacing_mm,
            column_spacing_mm,
        })
    }

    #[cfg(test)]
    pub(crate) fn encoded_len(self) -> Result<u64, Error> {
        self.encoded_len_with_limit(u64::MAX)
    }

    pub(crate) fn minimum_encoded_len(self) -> Result<u64, Error> {
        u64::from(self.frame_count)
            .checked_mul(MIN_PER_FRAME_ITEM_BYTES)
            .and_then(|bytes| bytes.checked_add(PER_FRAME_SEQUENCE_CONTAINER_BYTES))
            .ok_or_else(|| Error::InvalidOptions {
                reason: "DICOM per-frame metadata lower-bound overflow".into(),
            })
    }

    pub(crate) fn encoded_len_with_limit(self, max_metadata_bytes: u64) -> Result<u64, Error> {
        let minimum = self.minimum_encoded_len()?;
        if minimum > max_metadata_bytes {
            return Err(metadata_budget_error(minimum, max_metadata_bytes));
        }

        let mut counter = CountingWriter {
            written: 0,
            limit: max_metadata_bytes,
            required: None,
        };
        let result = self.write_encoded(&mut counter);
        if let Some(required) = counter.required {
            return Err(metadata_budget_error(required, max_metadata_bytes));
        }
        result.map_err(per_frame_stream_error)?;
        Ok(counter.written)
    }

    pub(crate) fn frame_count(self) -> u32 {
        self.frame_count
    }

    pub(crate) fn write_to(
        self,
        output: &mut impl Write,
        max_metadata_bytes: u64,
    ) -> Result<u64, Error> {
        self.encoded_len_with_limit(max_metadata_bytes)?;
        let mut bounded = MetadataBudgetWriter {
            inner: output,
            written: 0,
            limit: max_metadata_bytes,
            required: None,
        };
        let result = self.write_encoded(&mut bounded);
        if let Some(required) = bounded.required {
            return Err(metadata_budget_error(required, max_metadata_bytes));
        }
        result.map_err(per_frame_stream_error)?;
        Ok(bounded.written)
    }

    fn write_encoded(self, output: &mut impl Write) -> io::Result<()> {
        write_sequence_header(output, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)?;
        for frame_index in 0..self.frame_count {
            let location = self
                .frame_grid
                .location_for_frame(frame_index)
                .map_err(io::Error::other)?;
            write_per_frame_functional_group_item(
                output,
                location,
                self.frame_grid,
                self.row_spacing_mm,
                self.column_spacing_mm,
            )?;
        }
        write_sequence_delimiter(output)
    }
}

fn per_frame_stream_error(source: io::Error) -> Error {
    Error::Unsupported {
        reason: format!("failed to stream DICOM per-frame metadata: {source}"),
    }
}

fn metadata_budget_error(required: u64, limit: u64) -> Error {
    Error::InvalidOptions {
        reason: format!(
            "DICOM per-frame metadata requires at least {required} bytes, exceeding the {limit}-byte metadata budget"
        ),
    }
}

struct CountingWriter {
    written: u64,
    limit: u64,
    required: Option<u64>,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.written = self
            .written
            .checked_add(u64::try_from(bytes.len()).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other("metadata byte count overflow"))?;
        if self.written > self.limit {
            self.required = Some(self.written);
            return Err(io::Error::other("metadata byte budget exceeded"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct MetadataBudgetWriter<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    written: u64,
    limit: u64,
    required: Option<u64>,
}

impl<W: Write + ?Sized> Write for MetadataBudgetWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let requested = u64::try_from(bytes.len()).map_err(io::Error::other)?;
        let next = self
            .written
            .checked_add(requested)
            .ok_or_else(|| io::Error::other("metadata byte count overflow"))?;
        if next > self.limit {
            self.required = Some(next);
            return Err(io::Error::other(format!(
                "metadata byte budget {} exceeded",
                self.limit
            )));
        }
        let written = self.inner.write(bytes)?;
        self.written = self
            .written
            .checked_add(u64::try_from(written).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other("metadata byte count overflow"))?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn write_per_frame_functional_group_item(
    output: &mut impl Write,
    location: FrameLocation,
    grid: FrameGrid,
    row_spacing_mm: f64,
    column_spacing_mm: f64,
) -> io::Result<()> {
    write_item_header(output, u32::MAX)?;

    write_sequence_header(output, tags::FRAME_CONTENT_SEQUENCE)?;
    write_item_header(output, u32::MAX)?;
    let [row_index, column_index] = location
        .dimension_index_values()
        .map_err(io::Error::other)?;
    write_u32_values(
        output,
        tags::DIMENSION_INDEX_VALUES,
        &[row_index, column_index],
    )?;
    write_item_delimiter(output)?;
    write_sequence_delimiter(output)?;

    write_sequence_header(output, tags::PLANE_POSITION_SLIDE_SEQUENCE)?;
    write_item_header(output, u32::MAX)?;
    let (x_offset, y_offset) =
        location.slide_coordinate_offsets(grid, row_spacing_mm, column_spacing_mm);
    write_ds_value(output, tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, x_offset)?;
    write_ds_value(output, tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, y_offset)?;
    write_ds_value(output, tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, 0.0)?;
    let (column_position, row_position) = location
        .slide_matrix_positions(grid)
        .map_err(io::Error::other)?;
    write_i32_value(
        output,
        tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
        column_position,
    )?;
    write_i32_value(
        output,
        tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
        row_position,
    )?;
    write_item_delimiter(output)?;
    write_sequence_delimiter(output)?;

    write_item_delimiter(output)
}

fn write_sequence_header(output: &mut impl Write, tag: Tag) -> io::Result<()> {
    write_tag(output, tag.0, tag.1)?;
    output.write_all(b"SQ")?;
    output.write_all(&[0, 0])?;
    output.write_all(&u32::MAX.to_le_bytes())
}

fn write_item_delimiter(output: &mut impl Write) -> io::Result<()> {
    write_tag(output, 0xFFFE, 0xE00D)?;
    output.write_all(&0u32.to_le_bytes())
}

fn write_sequence_delimiter(output: &mut impl Write) -> io::Result<()> {
    write_tag(output, 0xFFFE, 0xE0DD)?;
    output.write_all(&0u32.to_le_bytes())
}

fn write_u32_values(output: &mut impl Write, tag: Tag, values: &[u32]) -> io::Result<()> {
    let value_bytes = values
        .len()
        .checked_mul(std::mem::size_of::<u32>())
        .and_then(|len| u16::try_from(len).ok())
        .ok_or_else(|| io::Error::other("UL value length overflow"))?;
    write_tag(output, tag.0, tag.1)?;
    output.write_all(b"UL")?;
    output.write_all(&value_bytes.to_le_bytes())?;
    for value in values {
        output.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

fn write_i32_value(output: &mut impl Write, tag: Tag, value: i32) -> io::Result<()> {
    write_tag(output, tag.0, tag.1)?;
    output.write_all(b"SL")?;
    output.write_all(&4u16.to_le_bytes())?;
    output.write_all(&value.to_le_bytes())
}

fn write_ds_value(output: &mut impl Write, tag: Tag, value: f64) -> io::Result<()> {
    let text = format_ds(value);
    if text.len() > 16 {
        return Err(io::Error::other("DS value exceeds 16 encoded bytes"));
    }
    let padded_len = text
        .len()
        .checked_add(text.len() % 2)
        .and_then(|len| u16::try_from(len).ok())
        .ok_or_else(|| io::Error::other("DS encoded length overflow"))?;
    write_tag(output, tag.0, tag.1)?;
    output.write_all(b"DS")?;
    output.write_all(&padded_len.to_le_bytes())?;
    output.write_all(text.as_bytes())?;
    if !text.len().is_multiple_of(2) {
        output.write_all(b" ")?;
    }
    Ok(())
}

impl FrameLocation {
    fn dimension_index_values(self) -> Result<[u32; 2], Error> {
        Ok([
            checked_dimension_index_value(self.row, "row")?,
            checked_dimension_index_value(self.column, "column")?,
        ])
    }

    fn slide_matrix_positions(self, grid: FrameGrid) -> Result<(i32, i32), Error> {
        Ok((
            checked_slide_matrix_position(self.column, grid.frame_columns, "column")?,
            checked_slide_matrix_position(self.row, grid.frame_rows, "row")?,
        ))
    }

    fn slide_coordinate_offsets(
        self,
        grid: FrameGrid,
        row_spacing_mm: f64,
        column_spacing_mm: f64,
    ) -> (f64, f64) {
        (
            self.column as f64 * f64::from(grid.frame_columns) * column_spacing_mm,
            self.row as f64 * f64::from(grid.frame_rows) * row_spacing_mm,
        )
    }
}

#[cfg(test)]
pub(super) fn per_frame_items(
    frame_count: u32,
    frame_grid: FrameGrid,
    row_spacing_mm: f64,
    column_spacing_mm: f64,
) -> Result<Vec<InMemDicomObject>, Error> {
    frame_grid.validate()?;
    let mut items = Vec::new();
    items
        .try_reserve_exact(frame_count as usize)
        .map_err(|_| Error::Unsupported {
            reason: "test per-frame functional group graph exceeds available memory".into(),
        })?;
    for frame_index in 0..frame_count {
        let location = frame_grid.location_for_frame(frame_index)?;
        let [row_index_value, column_index_value] = location.dimension_index_values()?;
        let (column_position, row_position) = location.slide_matrix_positions(frame_grid)?;
        let (x_offset, y_offset) =
            location.slide_coordinate_offsets(frame_grid, row_spacing_mm, column_spacing_mm);
        let mut position = InMemDicomObject::new_empty();
        position.put(DataElement::new(
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from(column_position),
        ));
        position.put(DataElement::new(
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from(row_position),
        ));
        put_ds(
            &mut position,
            tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            x_offset,
        );
        put_ds(
            &mut position,
            tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            y_offset,
        );
        put_ds(
            &mut position,
            tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            0.0,
        );
        let mut frame_content = InMemDicomObject::new_empty();
        frame_content.put(DataElement::new(
            tags::DIMENSION_INDEX_VALUES,
            VR::UL,
            PrimitiveValue::U32(vec![row_index_value, column_index_value].into()),
        ));
        let mut item = InMemDicomObject::new_empty();
        item.put(DataElement::<InMemDicomObject>::new(
            tags::FRAME_CONTENT_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![frame_content]),
        ));
        item.put(DataElement::<InMemDicomObject>::new(
            tags::PLANE_POSITION_SLIDE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![position]),
        ));
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
fn put_ds(object: &mut InMemDicomObject, tag: Tag, value: f64) {
    object.put(DataElement::new(tag, VR::DS, format_ds(value)));
}

fn checked_slide_matrix_position(
    index: u64,
    frame_extent: u32,
    axis: &'static str,
) -> Result<i32, Error> {
    let position = index
        .checked_mul(u64::from(frame_extent))
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| Error::Unsupported {
            reason: format!("DICOM {axis} position overflow"),
        })?;
    i32::try_from(position).map_err(|_| Error::Unsupported {
        reason: format!("DICOM {axis} position exceeds SL range: {position}"),
    })
}

pub(super) fn checked_dimension_index_value(index: u64, axis: &'static str) -> Result<u32, Error> {
    let value = index.checked_add(1).ok_or_else(|| Error::Unsupported {
        reason: format!("DICOM {axis} dimension index overflow"),
    })?;
    u32::try_from(value).map_err(|_| Error::Unsupported {
        reason: format!("DICOM {axis} dimension index exceeds UL range: {value}"),
    })
}

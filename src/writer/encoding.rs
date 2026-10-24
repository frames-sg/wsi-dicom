use std::io::{self, Write};

pub(super) use wsi_dicom_annotations::format_dicom_decimal as format_ds;

pub(super) fn write_item_header(output: &mut impl Write, length: u32) -> io::Result<()> {
    write_tag(output, 0xFFFE, 0xE000)?;
    output.write_all(&length.to_le_bytes())
}

pub(super) fn write_tag(output: &mut impl Write, group: u16, element: u16) -> io::Result<()> {
    output.write_all(&group.to_le_bytes())?;
    output.write_all(&element.to_le_bytes())
}

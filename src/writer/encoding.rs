use std::io::{self, Write};

pub(super) fn write_item_header(output: &mut impl Write, length: u32) -> io::Result<()> {
    write_tag(output, 0xFFFE, 0xE000)?;
    output.write_all(&length.to_le_bytes())
}

pub(super) fn write_tag(output: &mut impl Write, group: u16, element: u16) -> io::Result<()> {
    output.write_all(&group.to_le_bytes())?;
    output.write_all(&element.to_le_bytes())
}

pub(super) fn format_ds(value: f64) -> String {
    for precision in (0..=12).rev() {
        let mut text = format!("{value:.precision$}");
        while text.contains('.') && text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        if text.len() <= 16 {
            return text;
        }
    }
    format!("{value:.8e}")
}

//! Marker segments of the ISO/IEC 10918-1 syntax that JPEG and JPEG-LS share,
//! read from SOI through the first SOS. Entropy-coded data is never scanned.

pub(super) const SOI: u8 = 0xD8;
pub(super) const EOI: u8 = 0xD9;
pub(super) const SOS: u8 = 0xDA;

/// One marker and its parameters; standalone markers have none.
pub(super) struct Segment<'a> {
    pub(super) marker: u8,
    pub(super) payload: &'a [u8],
}

/// The segments after SOI, ending with the first SOS, EOI or a malformation.
/// Each step consumes at least one byte, so the walk is bounded by the header.
pub(super) struct HeaderSegments<'a> {
    bytes: &'a [u8],
    position: usize,
    done: bool,
}

/// Walks a stream known to begin with SOI.
pub(super) fn header_segments(bytes: &[u8]) -> HeaderSegments<'_> {
    HeaderSegments {
        bytes,
        position: 2,
        done: false,
    }
}

impl<'a> Iterator for HeaderSegments<'a> {
    type Item = Result<Segment<'a>, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let result = self.read();
        self.done = match &result {
            Ok(segment) => matches!(segment.marker, SOS | EOI),
            Err(_) => true,
        };
        Some(result)
    }
}

impl<'a> HeaderSegments<'a> {
    fn read(&mut self) -> Result<Segment<'a>, String> {
        let bytes = self.bytes;
        match bytes.get(self.position) {
            Some(0xFF) => {}
            Some(byte) => {
                return Err(format!(
                    "byte {} is {byte:02X}H where a marker is expected",
                    self.position
                ))
            }
            None => return Err("the header ends before an SOS marker".into()),
        }
        // Any number of FFH fill bytes may precede a marker (ISO/IEC 10918-1 B.1.1.2).
        while bytes.get(self.position) == Some(&0xFF) {
            self.position += 1;
        }
        let Some(&marker) = bytes.get(self.position) else {
            return Err("the header ends inside a marker".into());
        };
        self.position += 1;
        if marker == 0x00 {
            return Err(format!(
                "byte {} is a stuffed FF00H outside entropy-coded data",
                self.position - 2
            ));
        }
        if marker == 0x01 || (0xD0..=EOI).contains(&marker) {
            return Ok(Segment {
                marker,
                payload: &[],
            });
        }
        let length = bytes
            .get(self.position..self.position + 2)
            .map(|length| usize::from(u16::from_be_bytes([length[0], length[1]])))
            .ok_or_else(|| format!("marker FF{marker:02X}H has no length"))?;
        let end = self.position + length;
        if length < 2 || end > bytes.len() {
            return Err(format!(
                "marker FF{marker:02X}H declares length {length}, beyond the Frame"
            ));
        }
        let payload = &bytes[self.position + 2..end];
        self.position = end;
        Ok(Segment { marker, payload })
    }
}

/// Whether `marker` is a Start Of Frame of ISO/IEC 10918-1 or 14495-1.
pub(super) fn is_start_of_frame(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC)) || marker == 0xF7
}

/// "SOF3 (lossless, Huffman)".
pub(super) fn start_of_frame_name(marker: u8) -> String {
    let process = match marker {
        0xC0 => "baseline DCT",
        0xC1 => "extended sequential DCT, Huffman",
        0xC2 => "progressive DCT, Huffman",
        0xC3 => "lossless, Huffman",
        0xC5 => "differential sequential DCT, Huffman",
        0xC6 => "differential progressive DCT, Huffman",
        0xC7 => "differential lossless, Huffman",
        0xC9 => "extended sequential DCT, arithmetic",
        0xCA => "progressive DCT, arithmetic",
        0xCB => "lossless, arithmetic",
        0xCD => "differential sequential DCT, arithmetic",
        0xCE => "differential progressive DCT, arithmetic",
        0xCF => "differential lossless, arithmetic",
        0xF7 => return "SOF55 (JPEG-LS)".into(),
        _ => "unknown",
    };
    format!("SOF{} ({process})", marker - 0xC0)
}

/// The frame header parameters common to SOFn and SOF55.
pub(super) struct FrameHeader {
    pub(super) marker: u8,
    pub(super) precision: u8,
    pub(super) lines: u16,
    pub(super) samples_per_line: u16,
    /// (identifier, horizontal and vertical sampling factors, table selector).
    pub(super) components: Vec<(u8, (u8, u8), u8)>,
}

impl FrameHeader {
    pub(super) fn parse(marker: u8, payload: &[u8]) -> Result<Self, String> {
        let name = start_of_frame_name(marker);
        let [precision, lines_high, lines_low, columns_high, columns_low, count, components @ ..] =
            payload
        else {
            return Err(format!("{name} is shorter than its fixed parameters"));
        };
        if components.len() != usize::from(*count) * 3 {
            return Err(format!(
                "{name} declares {count} components in {} bytes of component parameters",
                components.len()
            ));
        }
        Ok(Self {
            marker,
            precision: *precision,
            lines: u16::from_be_bytes([*lines_high, *lines_low]),
            samples_per_line: u16::from_be_bytes([*columns_high, *columns_low]),
            components: components
                .as_chunks::<3>()
                .0
                .iter()
                .map(|&[identifier, sampling, table]| {
                    (identifier, (sampling >> 4, sampling & 0x0F), table)
                })
                .collect(),
        })
    }

    pub(super) fn name(&self) -> String {
        start_of_frame_name(self.marker)
    }

    /// "SOF0", or "SOF55" for JPEG-LS, for naming the header's fields.
    pub(super) fn marker_name(&self) -> String {
        if self.marker == 0xF7 {
            "SOF55".into()
        } else {
            format!("SOF{}", self.marker - 0xC0)
        }
    }

    /// Whether some component is sampled at a different rate than another,
    /// which encodes image planes of different sizes.
    pub(super) fn subsamples(&self) -> bool {
        self.components
            .windows(2)
            .any(|pair| pair[0].1 != pair[1].1)
    }

    pub(super) fn sampling_text(&self) -> String {
        self.components
            .iter()
            .map(|(_, (horizontal, vertical), _)| format!("{horizontal}x{vertical}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The scan header: component selectors with their table selectors, then
/// the three process-specific parameters (Ss, Se and Ah/Al; for JPEG-LS,
/// NEAR, ILV and the point transform).
pub(super) struct ScanHeader {
    pub(super) components: Vec<(u8, u8)>,
    pub(super) parameters: [u8; 3],
}

impl ScanHeader {
    pub(super) fn parse(payload: &[u8]) -> Result<Self, String> {
        let [count, rest @ ..] = payload else {
            return Err("SOS is empty".into());
        };
        let selectors = usize::from(*count) * 2;
        if rest.len() != selectors + 3 {
            return Err(format!(
                "SOS declares {count} components in {} bytes of scan parameters",
                rest.len()
            ));
        }
        Ok(Self {
            components: rest[..selectors]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[selector, tables]| (selector, tables))
                .collect(),
            parameters: [rest[selectors], rest[selectors + 1], rest[selectors + 2]],
        })
    }
}

/// Whether the stream, less padding, ends with EOI.
pub(super) fn ends_with_end_of_image(bytes: &[u8]) -> bool {
    super::without_trailing_padding(bytes).ends_with(&[0xFF, EOI])
}

/// The Fragment (numbered from 1 within the Frame) that begins a second
/// stream right after an earlier Fragment ended one with EOI. A marker pair
/// EOI, SOI cannot occur inside one stream, so such a Frame span holds more
/// than one Frame.
pub(super) fn second_stream_fragment(bytes: &[u8], fragment_starts: &[usize]) -> Option<usize> {
    fragment_starts
        .iter()
        .position(|start| {
            bytes[*start..].starts_with(&[0xFF, SOI]) && ends_with_end_of_image(&bytes[..*start])
        })
        .map(|index| index + 2)
}

//! JPEG 2000 Part 1, Part 2 and HTJ2K (Part 15) codestreams (PS3.5 8.2.4,
//! 8.2.14, 10.18 and A.4.4).
//!
//! The main header is parsed marker by marker, then each tile-part is
//! reached through its Psot length and only its header is parsed, so a Frame
//! costs its headers however large its packet data.

mod coding;
mod size;

use dicom_dictionary_std::uids;

use super::{signature, without_trailing_padding, Declared, Frame, FrameReport, Reader};

/// The JPEG 2000 encapsulation of PS3.5 A.4.4.
const ENCAPSULATION: &str = "PS3.5 A.4.4";
const RPCL: &str = "PS3.5 10.18.1";

const SOC: u16 = 0xFF4F;
const SIZ: u16 = 0xFF51;
const COD: u16 = 0xFF52;
const COC: u16 = 0xFF53;
const TLM: u16 = 0xFF55;
const QCD: u16 = 0xFF5C;
const QCC: u16 = 0xFF5D;
const POC: u16 = 0xFF5F;
const CAP: u16 = 0xFF50;
const SOT: u16 = 0xFF90;
const SOD: u16 = 0xFF93;
const EOC: u16 = 0xFFD9;
/// DCO, VMS, DFS, ADS, MCT, MCC, NLT, MCO, CBD and ATK (ISO/IEC 15444-2 Annex A).
const PART2_MARKERS: std::ops::RangeInclusive<u16> = 0xFF70..=0xFF79;
const MCC: u16 = 0xFF75;

const JP2_SIGNATURE_BOX: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];
/// Rsiz bit 15: Part 2 capabilities; bit 14: HTJ2K (ISO/IEC 15444-15).
const RSIZ_PART2: u16 = 0x8000;
const RSIZ_HTJ2K: u16 = 0x4000;
/// Part 2 Rsiz bit 8: the Annex J multiple component transformations.
const RSIZ_PART2_MCT: u16 = 0x0100;
/// Pcap bit for Part 15 capabilities (ISO/IEC 15444-15).
const PCAP_PART15: u32 = 1 << (32 - 15);
/// SPcod code-block style bit 6: HT code-blocks (ISO/IEC 15444-15).
const HT_CODE_BLOCKS: u8 = 0x40;
const RPCL_ORDER: u8 = 2;
/// Coarsest resolution side that HTJ2K Lossless RPCL guarantees (10.18.1).
const MAX_BASE_RESOLUTION: u64 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// ISO/IEC 15444-1 only: 1.2.840.10008.1.2.4.90 and .91.
    Part1,
    /// Part 1 with the Part 2 Annex J multiple component extensions: .92, .93.
    Part2,
    /// ISO/IEC 15444-15: .201, .202 and .203.
    Htj2k,
}

/// What a JPEG 2000 Transfer Syntax admits.
#[derive(Clone, Copy, Debug)]
pub(super) struct Syntax {
    family: Family,
    lossless: bool,
    rpcl: bool,
}

impl Syntax {
    pub(super) fn of(uid: &str) -> Option<Self> {
        let (family, lossless, rpcl) = match uid {
            uids::JPEG2000_LOSSLESS => (Family::Part1, true, false),
            uids::JPEG2000 => (Family::Part1, false, false),
            uids::JPEG2000MC_LOSSLESS => (Family::Part2, true, false),
            uids::JPEG2000MC => (Family::Part2, false, false),
            uids::HTJ2K_LOSSLESS => (Family::Htj2k, true, false),
            uids::HTJ2K_LOSSLESS_RPCL => (Family::Htj2k, true, true),
            uids::HTJ2K => (Family::Htj2k, false, false),
            _ => return None,
        };
        Some(Self {
            family,
            lossless,
            rpcl,
        })
    }

    pub(super) fn is_htj2k(self) -> bool {
        self.family == Family::Htj2k
    }

    /// The consistency section of PS3.5 8.2 for this family.
    fn consistent(self) -> &'static str {
        if self.is_htj2k() {
            "PS3.5 8.2.14"
        } else {
            "PS3.5 8.2.4"
        }
    }
}

/// One component's SIZ parameters.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Component {
    precision: u8,
    signed: bool,
    horizontal: u8,
    vertical: u8,
}

struct Size {
    rsiz: u16,
    x: u32,
    y: u32,
    x_offset: u32,
    y_offset: u32,
    components: Vec<Component>,
}

impl Size {
    /// The component's horizontal and vertical extent on the reference grid
    /// sampled by its separation (ISO/IEC 15444-1 B.2).
    fn component_bounds(&self, component: &Component) -> ((u64, u64), (u64, u64)) {
        let axis = |start: u32, end: u32, step: u8| {
            let step = u64::from(step.max(1));
            (
                u64::from(start).div_ceil(step),
                u64::from(end).div_ceil(step),
            )
        };
        (
            axis(self.x_offset, self.x, component.horizontal),
            axis(self.y_offset, self.y, component.vertical),
        )
    }

    fn component_size(&self, component: &Component) -> (u64, u64) {
        let ((x0, x1), (y0, y1)) = self.component_bounds(component);
        (x1 - x0, y1 - y0)
    }

    /// Component indices are one byte when Csiz < 257 (ISO/IEC 15444-1 A.6.2).
    fn index_bytes(&self) -> usize {
        if self.components.len() < 257 {
            1
        } else {
            2
        }
    }
}

/// Facts gathered across the main header.
#[derive(Default)]
struct MainHeader {
    capabilities: Option<u32>,
    tile_lengths: bool,
    part2_markers: Vec<u16>,
    component_collections: Vec<u8>,
}

/// Where a marker segment is: the main header or a tile's header.
#[derive(Clone, Copy)]
enum Place {
    Main,
    Tile(u16),
}

impl Place {
    fn at(self, marker: &'static str) -> Location {
        Location {
            place: self,
            marker,
        }
    }
}

/// A marker segment named for messages: "main header COD", "tile 3 QCC".
#[derive(Clone, Copy)]
struct Location {
    place: Place,
    marker: &'static str,
}

impl std::fmt::Display for Location {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.place {
            Place::Main => write!(formatter, "main header {}", self.marker),
            Place::Tile(tile) => write!(formatter, "tile {tile} {}", self.marker),
        }
    }
}

struct Context<'r, 'f, 'a> {
    declared: &'a Declared,
    uid: &'a str,
    single_fragment: bool,
    syntax: Syntax,
    size: Size,
    report: &'r mut FrameReport<'f>,
}

pub(super) fn check(frame: &Frame<'_>, syntax: Syntax, report: &mut FrameReport<'_>) {
    let bytes = frame.bytes;
    if bytes.starts_with(&JP2_SIGNATURE_BOX) {
        report.violation(ENCAPSULATION, || {
            "begins with the JP2 signature box: the optional JP2 file format header shall not be included".into()
        });
        return;
    }
    if !bytes.starts_with(&[0xFF, 0x4F, 0xFF, 0x51]) {
        let uid = frame.declared.syntax.as_str();
        report.violation("PS3.10 7.2 and PS3.5 A.4.4", || {
            format!(
                "begins with {}, not the SOC and SIZ markers of the JPEG 2000 codestream Transfer Syntax {uid} identifies",
                signature(bytes)
            )
        });
        return;
    }
    let Some((size, mut position)) = parse_size(bytes) else {
        report.violation(ENCAPSULATION, || "SIZ marker segment is malformed".into());
        return;
    };
    let mut context = Context {
        declared: frame.declared,
        uid: frame.declared.syntax.as_str(),
        single_fragment: frame.fragment_starts.is_empty(),
        syntax,
        size,
        report,
    };
    context.check_size();
    let mut main = MainHeader::default();
    loop {
        match context.segment(bytes, position) {
            Ok((SOT, _, _)) => break,
            Ok((marker, payload, next)) => {
                context.main_marker(marker, payload, &mut main);
                position = next;
            }
            Err(reason) => {
                context.report.violation(ENCAPSULATION, || {
                    format!("main header is malformed: {reason}")
                });
                return;
            }
        }
    }
    context.check_main_header(&main);
    context.walk_tile_parts(bytes, position);
}

fn parse_size(bytes: &[u8]) -> Option<(Size, usize)> {
    let mut reader = Reader::new(bytes.get(4..)?);
    let length = usize::from(reader.u16()?);
    let rsiz = reader.u16()?;
    let x = reader.u32()?;
    let y = reader.u32()?;
    let x_offset = reader.u32()?;
    let y_offset = reader.u32()?;
    reader.take(16)?;
    let count = usize::from(reader.u16()?);
    if length != 38 + 3 * count || count == 0 || x <= x_offset || y <= y_offset {
        return None;
    }
    let (components, _) = reader.take(3 * count)?.as_chunks::<3>();
    let components = components
        .iter()
        .map(|&[depth, horizontal, vertical]| Component {
            precision: (depth & 0x7F) + 1,
            signed: depth & 0x80 != 0,
            horizontal,
            vertical,
        })
        .collect();
    Some((
        Size {
            rsiz,
            x,
            y,
            x_offset,
            y_offset,
            components,
        },
        4 + length,
    ))
}

impl Context<'_, '_, '_> {
    /// The marker at `position`, its parameters and the next position.
    /// Delimiting markers carry no length and stop a header walk.
    fn segment<'b>(
        &self,
        bytes: &'b [u8],
        position: usize,
    ) -> Result<(u16, &'b [u8], usize), String> {
        let mut reader = Reader::new(bytes.get(position..).unwrap_or(&[]));
        let marker = reader
            .u16()
            .ok_or_else(|| "the header ends before the first SOT".to_string())?;
        if marker & 0xFF00 != 0xFF00 {
            return Err(format!(
                "byte {position} holds {marker:04X}H where a marker is expected"
            ));
        }
        if matches!(marker, SOT | SOD) {
            return Ok((marker, &[], position));
        }
        if matches!(marker, SOC | SIZ | EOC) {
            return Err(format!("marker {marker:04X}H appears inside a header"));
        }
        let length = usize::from(reader.u16().ok_or("a marker segment has no length")?);
        let payload = length
            .checked_sub(2)
            .and_then(|count| reader.take(count))
            .ok_or_else(|| {
                format!("marker {marker:04X}H declares length {length}, beyond the Frame")
            })?;
        Ok((marker, payload, position + 2 + length))
    }

    fn main_marker(&mut self, marker: u16, payload: &[u8], main: &mut MainHeader) {
        match marker {
            CAP => main.capabilities = Reader::new(payload).u32(),
            TLM => main.tile_lengths = true,
            MCC => {
                // Zmcc (2 bytes), then Imcc: the collection index (ISO/IEC 15444-2 Annex A).
                if let Some(&index) = payload.get(2) {
                    if !main.component_collections.contains(&index) {
                        main.component_collections.push(index);
                    }
                }
                main.part2_markers.push(marker);
            }
            marker if PART2_MARKERS.contains(&marker) => main.part2_markers.push(marker),
            _ => self.coding_marker(marker, payload, Place::Main),
        }
    }

    fn check_main_header(&mut self, main: &MainHeader) {
        let uid = self.uid;
        let ht_capable = main
            .capabilities
            .is_some_and(|capabilities| capabilities & PCAP_PART15 != 0);
        match self.syntax.family {
            Family::Htj2k if !ht_capable => self.report.violation(ENCAPSULATION, || {
                format!("main header has no CAP marker segment declaring Part 15 capabilities, which every HTJ2K codestream of Transfer Syntax {uid} carries")
            }),
            Family::Part1 | Family::Part2 if ht_capable => self.report.violation(ENCAPSULATION, || {
                format!("CAP marker segment declares HTJ2K (Part 15) capabilities, which Transfer Syntax {uid} does not identify")
            }),
            _ => {}
        }
        if self.syntax.rpcl && !main.tile_lengths {
            self.report.violation(RPCL, || {
                "main header has no TLM (tile-part length) marker segment, which HTJ2K Lossless RPCL requires".into()
            });
        }
        for &marker in &main.part2_markers {
            self.part2_marker(marker, Place::Main);
        }
        if self.syntax.family == Family::Part2
            && main.component_collections.len() > 1
            && self.single_fragment
        {
            let collections = main.component_collections.len();
            self.report.violation(ENCAPSULATION, || {
                format!("MCC marker segments define {collections} component collections in a codestream held by a single Fragment, but each Fragment contains data of one component collection")
            });
        }
    }

    fn part2_marker(&mut self, marker: u16, place: Place) {
        let location = place.at(part2_marker_name(marker));
        let uid = self.uid;
        match self.syntax.family {
            Family::Part1 | Family::Htj2k => self.report.violation(ENCAPSULATION, || {
                format!(
                    "{location} ({marker:04X}H) is a JPEG 2000 Part 2 extension that Transfer Syntax {uid} does not admit"
                )
            }),
            // Annex J uses MCT, MCC, MCO and CBD; the others belong to other Annexes.
            Family::Part2 if !matches!(marker, 0xFF74 | 0xFF75 | 0xFF77 | 0xFF78) => {
                self.report.violation(ENCAPSULATION, || {
                    format!(
                        "{location} ({marker:04X}H) belongs to a JPEG 2000 Part 2 Annex other than the Annex J multiple component transformations"
                    )
                });
            }
            Family::Part2 => {}
        }
    }

    /// Hops SOT to SOT by Psot and parses each tile-part header; the
    /// codestream ends with EOC, followed by nothing but padding.
    fn walk_tile_parts(&mut self, bytes: &[u8], mut position: usize) {
        loop {
            let mut reader = Reader::new(bytes.get(position..).unwrap_or(&[]));
            match reader.u16() {
                Some(SOT) => {}
                Some(EOC) => {
                    if bytes[position + 2..].iter().any(|byte| *byte != 0) {
                        self.report.violation(ENCAPSULATION, || {
                            format!("data follows EOC at byte {position}, so the Frame holds more than one codestream")
                        });
                    }
                    return;
                }
                None => {
                    self.report.violation(ENCAPSULATION, || {
                        "codestream ends without EOC (FFD9H), so the Frame is not one complete codestream".into()
                    });
                    return;
                }
                Some(other) => {
                    self.report.violation(ENCAPSULATION, || {
                        format!("byte {position} holds {other:04X}H where SOT or EOC is expected, so the Frame is not one complete codestream")
                    });
                    return;
                }
            }
            let parsed = (|| {
                (reader.u16()? == 10).then_some(())?;
                let tile = reader.u16()?;
                let length = usize::try_from(reader.u32()?).ok()?;
                Some((tile, length))
            })();
            let Some((tile, length)) = parsed else {
                self.report
                    .violation(ENCAPSULATION, || "SOT marker segment is malformed".into());
                return;
            };
            // Psot 0 extends the last tile-part to EOC (ISO/IEC 15444-1 A.4.2).
            let end = if length == 0 {
                let content = without_trailing_padding(bytes);
                if !content.ends_with(&[0xFF, 0xD9]) {
                    self.report.violation(ENCAPSULATION, || {
                        "codestream ends without EOC (FFD9H), so the Frame is not one complete codestream".into()
                    });
                    return;
                }
                content.len() - 2
            } else {
                position.saturating_add(length)
            };
            if end < position + 14 || end > bytes.len() {
                self.report.violation(ENCAPSULATION, || {
                    format!("tile {tile} tile-part length Psot {length} runs past the Frame's {} bytes, so the codestream is incomplete", bytes.len())
                });
                return;
            }
            if !self.tile_part_header(bytes, position + 12, end, tile) {
                return;
            }
            position = end;
        }
    }

    /// Parses one tile-part header up to SOD; false when it is malformed.
    fn tile_part_header(
        &mut self,
        bytes: &[u8],
        mut position: usize,
        end: usize,
        tile: u16,
    ) -> bool {
        let bytes = &bytes[..end];
        loop {
            let reason = match self.segment(bytes, position) {
                Ok((SOD, _, _)) => return true,
                Ok((SOT, _, _)) => "it reaches another SOT".to_string(),
                Ok((marker, payload, next)) => {
                    if PART2_MARKERS.contains(&marker) {
                        self.part2_marker(marker, Place::Tile(tile));
                    } else {
                        self.coding_marker(marker, payload, Place::Tile(tile));
                    }
                    position = next;
                    continue;
                }
                Err(reason) => reason,
            };
            self.report.violation(ENCAPSULATION, || {
                format!("tile {tile} tile-part header does not reach SOD within its Psot length: {reason}")
            });
            return false;
        }
    }
}

fn part2_marker_name(marker: u16) -> &'static str {
    match marker {
        0xFF70 => "DCO",
        0xFF71 => "VMS",
        0xFF72 => "DFS",
        0xFF73 => "ADS",
        0xFF74 => "MCT",
        0xFF75 => "MCC",
        0xFF76 => "NLT",
        0xFF77 => "MCO",
        0xFF78 => "CBD",
        0xFF79 => "ATK",
        _ => "Part 2 marker",
    }
}

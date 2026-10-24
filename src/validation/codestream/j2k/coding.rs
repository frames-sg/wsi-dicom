//! COD, COC, QCD, QCC and POC: the coding process against the Transfer
//! Syntax, and the multiple component transformation against Photometric
//! Interpretation.

use super::{
    Context, Family, Location, Place, Reader, COC, COD, ENCAPSULATION, HT_CODE_BLOCKS,
    MAX_BASE_RESOLUTION, POC, QCC, QCD, RPCL, RPCL_ORDER,
};

impl Context<'_, '_, '_> {
    /// COD, COC, QCD, QCC and POC, wherever they appear.
    pub(super) fn coding_marker(&mut self, marker: u16, payload: &[u8], place: Place) {
        let index_bytes = self.size.index_bytes();
        let mut reader = Reader::new(payload);
        match marker {
            COD => {
                let parsed = (|| {
                    reader.u8()?;
                    let progression = reader.u8()?;
                    reader.u16()?;
                    let transform = reader.u8()?;
                    Some((progression, transform, CodingStyle::parse(&mut reader)?))
                })();
                let location = place.at("COD");
                match parsed {
                    Some((progression, multiple_component, style)) => {
                        self.check_style(location, &style, None);
                        self.check_progression(location, progression);
                        self.check_multiple_component(location, multiple_component, &style);
                    }
                    None => self.malformed(location),
                }
            }
            COC => {
                let parsed = (|| {
                    let component = read_index(&mut reader, index_bytes)?;
                    reader.u8()?;
                    Some((component, CodingStyle::parse(&mut reader)?))
                })();
                match parsed {
                    Some((component, style)) => {
                        self.check_style(place.at("COC"), &style, Some(component));
                    }
                    None => self.malformed(place.at("COC")),
                }
            }
            QCD | QCC => {
                let location = place.at(if marker == QCD { "QCD" } else { "QCC" });
                if marker == QCC && read_index(&mut reader, index_bytes).is_none() {
                    return self.malformed(location);
                }
                match reader.u8() {
                    Some(style) => self.check_quantization(location, style & 0x1F),
                    None => self.malformed(location),
                }
            }
            POC if self.syntax.rpcl => {
                let entry = 5 + 2 * index_bytes;
                if payload.is_empty() || !payload.len().is_multiple_of(entry) {
                    return self.malformed(place.at("POC"));
                }
                for change in payload.chunks_exact(entry) {
                    self.check_progression(place.at("POC"), change[entry - 1]);
                }
            }
            _ => {}
        }
    }

    fn malformed(&mut self, name: Location) {
        self.report.violation(ENCAPSULATION, || {
            format!("{name} marker segment is malformed")
        });
    }

    fn check_style(&mut self, name: Location, style: &CodingStyle, component: Option<usize>) {
        let uid = self.uid;
        if self.syntax.family != Family::Htj2k && style.block_style & HT_CODE_BLOCKS != 0 {
            self.report.violation(ENCAPSULATION, || {
                format!("{name} code-block style {:02X}H selects HT code-blocks (ISO/IEC 15444-15), which Transfer Syntax {uid} does not identify", style.block_style)
            });
        }
        if style.transform > 1 {
            self.report.violation(ENCAPSULATION, || {
                format!("{name} wavelet transformation {} is an arbitrary (Part 2 Annex H) kernel, not the 5-3 reversible or 9-7 irreversible transformation", style.transform)
            });
        } else if self.syntax.lossless && style.transform != 1 {
            self.report.violation(ENCAPSULATION, || {
                format!("{name} selects the 9-7 irreversible wavelet transformation, but lossless Transfer Syntax {uid} uses the reversible 5-3 transformation")
            });
        }
        if self.syntax.rpcl {
            self.check_base_resolution(name, style.levels, component);
        }
    }

    fn check_quantization(&mut self, name: Location, style: u8) {
        if self.syntax.lossless && style != 0 {
            let uid = self.uid;
            self.report.violation(ENCAPSULATION, || {
                format!("{name} quantization style {style} quantizes, but lossless Transfer Syntax {uid} uses no quantization")
            });
        }
    }

    fn check_progression(&mut self, name: Location, progression: u8) {
        if self.syntax.rpcl && progression != RPCL_ORDER {
            self.report.violation(RPCL, || {
                format!(
                    "{name} progression order {} is not RPCL, which HTJ2K Lossless RPCL requires",
                    progression_name(progression)
                )
            });
        }
    }

    /// The lowest resolution of a component with `levels` decompositions
    /// spans ceil(x1 / 2^levels) - ceil(x0 / 2^levels) on each axis (ISO/IEC
    /// 15444-1 B.5). The statement asks for its width or its height to be at
    /// most 64.
    fn check_base_resolution(&mut self, name: Location, levels: u8, component: Option<usize>) {
        let Some(component) = self.size.components.get(component.unwrap_or(0)) else {
            return;
        };
        let ((x0, x1), (y0, y1)) = self.size.component_bounds(component);
        let reduce = |coordinate: u64| match 1u64.checked_shl(u32::from(levels)) {
            Some(scale) => coordinate.div_ceil(scale),
            None => u64::from(coordinate > 0),
        };
        let (width, height) = (reduce(x1) - reduce(x0), reduce(y1) - reduce(y0));
        if width > MAX_BASE_RESOLUTION && height > MAX_BASE_RESOLUTION {
            self.report.violation(RPCL, || {
                format!("{name} has {levels} decomposition levels, leaving a {width}x{height} base resolution with neither side at most 64")
            });
        }
    }

    /// SGcod multiple component transformation 1 is the Part 1 RCT with the
    /// 5-3 wavelet and the ICT with the 9-7 wavelet (ISO/IEC 15444-1 G.2, G.3).
    fn check_multiple_component(
        &mut self,
        name: Location,
        transformation: u8,
        style: &CodingStyle,
    ) {
        let declared = self.declared;
        let photometric = declared.photometric();
        let consistent = self.syntax.consistent();
        let uid = self.uid;
        match transformation {
            0 if declared.photometric_is(&["YBR_RCT", "YBR_ICT"]) => {
                self.report.violation(consistent, || {
                    format!("{name} applies no multiple component transformation, so the components are not the {photometric} that Photometric Interpretation declares")
                });
            }
            1 => {
                let (expected, kind) = if style.transform == 1 {
                    ("YBR_RCT", "reversible (RCT)")
                } else {
                    ("YBR_ICT", "irreversible (ICT)")
                };
                if declared.photometric.is_some() && !declared.photometric_is(&[expected]) {
                    self.report.violation(consistent, || {
                        format!("{name} applies the Part 1 {kind} multiple component transformation, so Photometric Interpretation is {expected}, not {photometric}")
                    });
                }
            }
            2 if self.syntax.family != Family::Part2 => {
                self.report.violation(ENCAPSULATION, || {
                    format!("{name} selects a Part 2 array-based multiple component transformation, which Transfer Syntax {uid} does not admit")
                });
            }
            0..=2 => {}
            other => self.report.violation(ENCAPSULATION, || {
                format!("{name} multiple component transformation {other} is undefined")
            }),
        }
    }
}

/// The SPcod / SPcoc parameters (ISO/IEC 15444-1 Table A.15).
struct CodingStyle {
    levels: u8,
    block_style: u8,
    transform: u8,
}

impl CodingStyle {
    fn parse(reader: &mut Reader<'_>) -> Option<Self> {
        let levels = reader.u8()?;
        reader.take(2)?;
        let block_style = reader.u8()?;
        let transform = reader.u8()?;
        Some(Self {
            levels,
            block_style,
            transform,
        })
    }
}

fn read_index(reader: &mut Reader<'_>, bytes: usize) -> Option<usize> {
    if bytes == 1 {
        reader.u8().map(usize::from)
    } else {
        reader.u16().map(usize::from)
    }
}

fn progression_name(progression: u8) -> String {
    match progression {
        0 => "LRCP".into(),
        1 => "RLCP".into(),
        2 => "RPCL".into(),
        3 => "PCRL".into(),
        4 => "CPRL".into(),
        other => format!("{other} (undefined)"),
    }
}

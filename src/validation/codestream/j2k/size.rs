//! SIZ: the image the codestream encodes, against the Image Pixel attributes
//! and the capabilities the Transfer Syntax admits.

use super::{Context, Family, ENCAPSULATION, RSIZ_HTJ2K, RSIZ_PART2, RSIZ_PART2_MCT};

const COMPONENTS_ALIKE: &str = "PS3.3 C.7-11c and PS3.3 C.7.6.3.1.1";

impl Context<'_, '_, '_> {
    pub(super) fn check_size(&mut self) {
        let syntax = self.syntax;
        let consistent = syntax.consistent();
        let rsiz = self.size.rsiz;
        let uid = self.uid;
        match syntax.family {
            Family::Part1 => {
                if rsiz & RSIZ_PART2 != 0 {
                    self.report.violation(ENCAPSULATION, || {
                        format!("SIZ Rsiz {rsiz:04X}H declares JPEG 2000 Part 2 capabilities, extensions that Part 1 decoders cannot decode, in Part 1 Transfer Syntax {uid}")
                    });
                }
                if rsiz & RSIZ_HTJ2K != 0 {
                    self.report.violation(ENCAPSULATION, || {
                        format!("SIZ Rsiz {rsiz:04X}H declares HTJ2K (ISO/IEC 15444-15) coding, which Transfer Syntax {uid} does not identify")
                    });
                }
            }
            Family::Part2 => {
                let other = rsiz & !(RSIZ_PART2 | RSIZ_PART2_MCT);
                if rsiz & RSIZ_PART2 != 0 && other != 0 {
                    self.report.violation(ENCAPSULATION, || {
                        format!("SIZ Rsiz {rsiz:04X}H declares Part 2 extensions other than the Annex J multiple component transformations (bits {other:04X}H)")
                    });
                }
            }
            Family::Htj2k => {
                if rsiz & RSIZ_HTJ2K == 0 {
                    self.report.violation(ENCAPSULATION, || {
                        format!("SIZ Rsiz {rsiz:04X}H lacks the HTJ2K capability bit 14 of an ISO/IEC 15444-15 codestream, which Transfer Syntax {uid} identifies")
                    });
                }
                if rsiz & RSIZ_PART2 != 0 {
                    self.report.violation(ENCAPSULATION, || {
                        format!("SIZ Rsiz {rsiz:04X}H declares JPEG 2000 Part 2 capabilities, which HTJ2K Transfer Syntax {uid} does not identify")
                    });
                }
            }
        }

        let declared = self.declared;
        let first = self.size.components[0];
        let (width, height) = self.size.component_size(&first);
        let clamp = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        declared.columns.compare(
            self.report,
            consistent,
            "SIZ width (Xsiz - XOsiz)",
            clamp(width),
        );
        declared.rows.compare(
            self.report,
            consistent,
            "SIZ height (Ysiz - YOsiz)",
            clamp(height),
        );
        let count = u32::try_from(self.size.components.len()).unwrap_or(u32::MAX);
        declared
            .samples_per_pixel
            .compare(self.report, consistent, "SIZ Csiz", count);
        declared.bits_stored.compare(
            self.report,
            consistent,
            "SIZ component 0 precision (Ssiz)",
            u32::from(first.precision),
        );
        declared.pixel_representation.compare(
            self.report,
            consistent,
            "SIZ component 0 sign (Ssiz)",
            u32::from(first.signed),
        );

        let components = &self.size.components;
        if components
            .iter()
            .any(|component| component.precision != first.precision)
        {
            let precisions = list(components.iter().map(|component| component.precision));
            self.report.violation(COMPONENTS_ALIKE, || {
                format!("SIZ component precisions differ ({precisions} bits), but every sample has the same Bits Allocated, Bits Stored and High Bit")
            });
        }
        if components
            .iter()
            .any(|component| component.signed != first.signed)
        {
            let signs = list(
                components
                    .iter()
                    .map(|component| u8::from(component.signed)),
            );
            self.report.violation(COMPONENTS_ALIKE, || {
                format!("SIZ component signs differ ({signs}), but every sample has the same Pixel Representation")
            });
        }
        if components
            .iter()
            .any(|component| self.size.component_size(component) != (width, height))
        {
            let separations = list(
                components
                    .iter()
                    .map(|component| format!("{}x{}", component.horizontal, component.vertical)),
            );
            self.report.violation("PS3.3 C.7.6.3.1.1", || {
                format!("SIZ component separations ({separations}) encode image planes of different sizes, but all image planes have the same Rows and Columns")
            });
        }
    }
}

fn list<T: std::fmt::Display>(values: impl Iterator<Item = T>) -> String {
    values
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

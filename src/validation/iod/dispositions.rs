//! The disposition of every normative statement in `requirements.rs` that the
//! generated tables do not enforce: the check that enforces it, or why a file
//! cannot be held to it. An `Unchecked` statement is decidable from the
//! encoded objects and awaits the named check family.

// The matches name Requirement Variants through `use Requirement::*`: a name
// no Variant has any more (a statement gone from the edition or the IODs)
// would bind as a catch-all pattern instead of failing to compile.
#![deny(unreachable_patterns, non_snake_case)]

use super::requirements::Requirement::{self, *};
use crate::validation::RequirementDisposition::{
    self, Behavior, Checked, OutsideScope, Tables, Undecidable,
};

impl Requirement {
    pub(super) const fn disposition(self) -> RequirementDisposition {
        match self {
            // ---- Enforced by an intrinsic check. ----
            // Number of Frames of zero rejected
            Ps3_C_7_6_6_1_1_8d631a => Checked("intrinsic-pixel-structure"),
            // Explicit VR structure violations misframe and fail parsing; reported generically
            Ps5_7_1_2_489caa => Checked("intrinsic-pixel-structure"),
            // Undefined length on other VRs is parsed as SQ and fails unless content is Items, then VR mismatch
            Ps5_7_1_2_d61905 => Checked("intrinsic-pixel-structure"),
            // Implicit VR structure violations misframe and fail parsing; reported generically
            Ps5_7_1_3_527d85 => Checked("intrinsic-pixel-structure"),
            // Wrong implicit-VR lengths misframe later elements, usually failing parse; no specific length finding
            Ps5_7_1_3_db44fd => Checked("intrinsic-pixel-structure"),
            // Mixed VR encoding misframes and fails parsing; invalid VR bytes silently become UN (explicit_le.rs:45)
            Ps5_7_1_77af7f => Checked("intrinsic-pixel-structure"),
            // Malformed element headers fail parsing, reported generically as an unreadable file
            Ps5_7_1_f793ac => Checked("intrinsic-pixel-structure"),
            // Byte-swapped tags and lengths fail parsing; byte order of Values is otherwise undecidable
            Ps5_7_3_3c0c4e => Checked("intrinsic-wsi-dicom-encoding"),
            // Item or delimiter tags written with a VR field misframe and fail parsing
            Ps5_7_5_1593f5 => Checked("intrinsic-pixel-structure"),
            // Non-(FFFE,E000) tag at an Item position fails parsing (UnexpectedTag)
            Ps5_7_5_1_16ec1d => Checked("intrinsic-pixel-structure"),
            // Any length other than FFFFFFFFH is defined; misuse misframes and fails parsing
            Ps5_7_5_1_272077 => Undecidable("Every Item Length other than FFFFFFFFH denotes a defined-length Item; whether it was intended to be undefined is not encoded, so no distinct negative encoding exists"),
            // Undefined-length Item without Item Delimitation ends in a PrematureEnd parse error
            Ps5_7_5_1_4c33bb => Checked("intrinsic-pixel-structure"),
            // Item content that is not Data Elements fails parsing
            Ps5_7_5_1_83352b => Checked("intrinsic-pixel-structure"),
            // Missing or misplaced Item Delimitation Item fails parsing
            Ps5_7_5_1_95977a => Checked("intrinsic-pixel-structure"),
            // Item length mismatch raises InconsistentSequenceEnd or misframes; parse failure reported generically
            Ps5_7_5_1_9986b0 => Checked("intrinsic-pixel-structure"),
            // Non-zero length on an empty Item misframes following bytes and fails parsing
            Ps5_7_5_1_f0461e => Checked("intrinsic-pixel-structure"),
            // Non-zero length for an empty Sequence misframes and fails parsing
            Ps5_7_5_2_5a0961 => Checked("intrinsic-pixel-structure"),
            // Lengths other than FFFFFFFFH are defined lengths; misuse misframes and fails parsing
            Ps5_7_5_2_5f93c9 => Undecidable("Every Sequence Length other than FFFFFFFFH denotes a defined-length Sequence; whether it was intended to be undefined is not encoded, so no distinct negative encoding exists"),
            // Sequence length mismatch raises InconsistentSequenceEnd or misframes; generic parse failure
            Ps5_7_5_2_83af70 => Checked("intrinsic-pixel-structure"),
            // Lead-in: a Sequence ends by length or delimiter; framing errors fail parsing
            Ps5_7_5_2_957daa => Checked("intrinsic-pixel-structure"),
            // Missing Sequence Delimitation Item fails parsing for SQ elements
            Ps5_7_5_2_b8c23d => Checked("intrinsic-pixel-structure"),
            // Item Data Sets in another VR encoding misframe and fail parsing; generic finding
            Ps5_7_5_e1942b => Checked("intrinsic-pixel-structure"),
            // Encapsulated Pixel Data without fragments rejected; Number of Frames > 0 required (line 150)
            Ps5_8_2_42fb24 => Checked("intrinsic-pixel-structure"),
            // Umbrella for Data Set encoding; only parser rejection enforces it; items classified separately
            Ps5_A_11_6a17ab => Checked("intrinsic-pixel-structure"),
            // Umbrella for Data Set encoding; only parser rejection enforces it; items classified separately
            Ps5_A_1_9969cd => Checked("intrinsic-pixel-structure"),
            // Umbrella for Data Set encoding; only parser rejection enforces it; items classified separately
            Ps5_A_2_cd3108 => Checked("intrinsic-pixel-structure"),
            // Only a malformed (undefined) BOT length can violate it; parser rejects UndefinedItemLength
            Ps5_A_4_1a92fb => Undecidable("An Item Length of zero denotes an absent Item Value, while a nonzero length denotes that many Value bytes; a different intended absence is not encoded"),
            // Nested native Pixel Data with undefined length fails item parsing; no explicit check
            Ps5_A_4_890675 => Checked("intrinsic-pixel-structure"),
            // Non-item tag in pixel sequence raises dicom-core UnexpectedTag; reported as read failure
            Ps5_A_4_b9093d => Checked("intrinsic-pixel-structure"),
            // Defined-length Pixel Data under an Encapsulated syntax rejected
            Ps5_A_4_d4b47e => Checked("intrinsic-pixel-structure"),
            // Umbrella for Data Set encoding; only parser rejection enforces it; items classified separately
            Ps5_A_4_e82862 => Checked("intrinsic-pixel-structure"),
            // Umbrella for Data Set encoding; only parser rejection enforces it; items classified separately
            Ps5_A_6_581e00 => Checked("intrinsic-pixel-structure"),
            // Group 0002 parsed as Explicit VR LE; invalid VR or non-UL group length rejected
            Ps10_7_1_02d941 => Checked("intrinsic-wsi-dicom-file-meta"),
            // UN on a Table 7.1-1 element flagged as VR mismatch; no UN-specific message
            Ps10_7_1_2f09d3 => Checked("intrinsic-wsi-dicom-file-meta"),
            // Bytes 128-131 must equal "DICM"
            Ps10_7_1_607718 => Checked("intrinsic-wsi-dicom-file-meta"),
            // Odd Value Lengths and wrong padding characters in File Meta flagged (file_meta.rs:124)
            Ps10_7_1_6b69cd => Checked("intrinsic-wsi-dicom-file-meta"),
            // Media Storage SOP Class must match SOP Class; full IOD checks run when meta declares WSI
            Ps10_7_1_7cdb3d => Checked("intrinsic-wsi-dicom-file-meta"),
            // (0002,0102) without (0002,0100) flagged forbidden via Table 7.1-1 1C condition (engine.rs:168)
            Ps10_7_1_a0dff3 => Checked("intrinsic-wsi-dicom-file-meta"),
            // Preamble, DICM prefix, Group Length and Table 7.1-1 presence enforced
            Ps10_7_1_a2a04a => Checked("intrinsic-wsi-dicom-file-meta"),
            // File Meta SOP Class/Instance UIDs compared to Data Set; DIMSE part out of scope. Also identity.rs:106
            Ps3_C_12_1_1_1_ac243d => Checked("intrinsic-wsi-dicom-file-meta"),
            // Raw group 0002 VRs checked against PS3.6; UN flagged as VR mismatch
            Ps5_6_2_2_78baa9 => Checked("intrinsic-wsi-dicom-file-meta"),
            // Every Optical Path ICC Profile validated against C.11.15.1.1 header constraints
            Ps3_C_8_12_5_1_4_b19588 => Checked("intrinsic-wsi-dicom-icc-profile"),
            // Planar Configuration 0 enforced for 3-sample WSI images; also C.8.12.4-1 enumeration
            Ps3_C_7_6_3_1_2_93fa52 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // YBR_ICT/YBR_RCT with any native Transfer Syntax rejected as incompatible
            Ps3_C_7_6_3_1_2_dbe575 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Bits Stored must equal Bits Allocated (8 or 16)
            Ps3_C_8_12_4_1_BitsStored_ed2b6b => Checked("intrinsic-wsi-dicom-image-metadata"),
            // High Bit must equal Bits Stored minus one
            Ps3_C_8_12_4_1_HighBit_e8f419 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // WSI requires Bits Stored equal to Bits Allocated, stricter than not larger
            Ps5_8_1_1_14376b => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Bits Allocated restricted to 8 or 16; also WSI enumerated-values rule
            Ps5_8_1_1_65182e => Checked("intrinsic-wsi-dicom-image-metadata"),
            // WSI Pixel Representation must be 0; every bit field is then a valid unsigned integer
            Ps5_8_1_1_873394 => Undecidable("Every stored bit field admits the unsigned or twos-complement interpretation that Pixel Representation specifies; the bytes cannot reveal an intended different interpretation"),
            // High Bit equal to Bits Stored minus 1 enforced
            Ps5_8_1_1_b0091d => Checked("intrinsic-wsi-dicom-image-metadata"),
            // WSI forbids Pixel Representation 1 (enforced), so 2's complement samples cannot conformantly occur
            Ps5_8_1_1_c9d618 => Undecidable("For a declared signed field, High Bit defines the sign bit by interpretation; the bytes cannot reveal an intended different sign-bit position"),
            // Planar Configuration depends on the syntax and sample count
            Ps5_8_2_10_06b5ac => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The pixel sample representation is constrained by the IOD and syntax
            Ps5_8_2_10_4bebe5 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Planar Configuration depends on the syntax and sample count
            Ps5_8_2_11_d5254e => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The pixel sample representation is constrained by the IOD and syntax
            Ps5_8_2_11_f04f36 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_14_b2f50a => Checked("intrinsic-wsi-dicom-image-metadata"),
            // YBR_RCT for .201/.202/.203, YBR_ICT only .203, YBR_FULL_422 rejected; matches Table 8.2.14-1
            Ps5_8_2_14_f3a8d2 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_15_167cfb => Checked("intrinsic-wsi-dicom-image-metadata"),
            // RLE allows only MONOCHROME2/RGB of WSI values; YBR_* with RLE rejected; Planar/Bits rows enforced
            Ps5_8_2_2_8b7a45 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_3_4e64b0 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // JPEG-LS: only MONOCHROME2/RGB pass; YBR_* rejected for .80/.81, matching Table 8.2.3-1
            Ps5_8_2_3_a4a3ff => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_4_d71faf => Checked("intrinsic-wsi-dicom-image-metadata"),
            // YBR_RCT only .90/.91, YBR_ICT only .91, YBR_FULL_422 rejected; matches Table 8.2.4-1 for WSI values
            Ps5_8_2_4_fad88b => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Planar Configuration depends on the syntax and sample count
            Ps5_8_2_5_0943da => OutsideScope("Table 8.2.5-1 requires Planar Configuration to be absent for single-component MPEG2; its value 0 applies only to multi-component YBR_PARTIAL_420, which C.8.12.4.1.5 excludes from VL WSI"),
            // The pixel sample representation is constrained by the IOD and syntax
            Ps5_8_2_7_3ffde9 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Planar Configuration depends on the syntax and sample count
            Ps5_8_2_7_b89ecb => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The pixel sample representation is constrained by the IOD and syntax
            Ps5_8_2_8_6614d4 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Planar Configuration depends on the syntax and sample count
            Ps5_8_2_8_7f832e => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // YBR_RCT/ICT rejected outside J2K/HTJ2K syntaxes; YBR_PARTIAL_420 rejected by WSI enumerations
            Ps5_8_2_8a024f => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Long/URN Code Value forbidden when Code Value present; short code requires Code Value (conditions.rs:70)
            Ps3_8_1_1c95ed => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Long Code Value required for non-URN code without Code Value; short Long values force Code Value
            Ps3_8_1_597bf2 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Code Value condition forbids >16-char code in Code Value; SH length also checked in values.rs
            Ps3_8_1_5e1957 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // URN Code Value required/forbidden by URN detection; heuristic is urn: prefix or :// only
            Ps3_8_1_aeece1 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Extension Creator UID 1C required when Extension Flag is Y (conditions.rs:106)
            Ps3_8_7_2d698d => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // HL7 Structured Document Reference items include SOP Instance Reference Macro with Type 1 UIDs
            Ps3_C_12_1_1_6_0f08ec => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Engine selects the lowest Type across overlapping Module rows
            Ps3_C_1_2_3_a1fe07 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // PALETTE COLOR (Icon items) requires R/G/B descriptors and data via presence condition
            Ps3_C_7_6_3_1_2_00abdd => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Planar Configuration forbidden when Samples per Pixel is 1 (engine.rs:168)
            Ps3_C_7_6_3_1_3_4a62de => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Planar Configuration required when Samples per Pixel > 1 (engine.rs:139)
            Ps3_C_7_6_3_1_3_5d2797 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Type 1 attribute present without a Value flagged
            Ps5_7_4_1_1b4c29 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Missing Type 1 attributes from the generated tables flagged
            Ps5_7_4_1_200da1 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Type 1C present when its condition is false and no 'may be present otherwise' flagged
            Ps5_7_4_2_7dcac3 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Type 1C conditions evaluated in conditions.rs; Unknown conditions reported undetermined, not failed
            Ps5_7_4_2_d74db3 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Absent Type 2 attributes flagged
            Ps5_7_4_3_828c2e => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Missing Type 2 attributes flagged
            Ps5_7_4_3_aa6998 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Type 2C present when its condition is false flagged
            Ps5_7_4_4_31eee8 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Missing required Standard elements flagged regardless of private substitutes
            Ps5_7_8_f1a77f => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Bits Allocated, Bits Stored, High Bit are Type 1 in WSI module table; presence enforced
            Ps5_8_1_1_3229af => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Decidable part: Pixel Data (Type 1C) presence enforced; image data hidden elsewhere undecidable; overlays not in IOD
            Ps5_8_1_80d60f => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_10_3d6c2c => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_11_d69e04 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_6_d4f25d => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_7_391ee8 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_8_302528 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // C.8.12.4-1 IDENTITY-only enumeration applies to every PI; monochrome rule also checks (instance.rs:258)
            Ps3_C_7_9_PresentationLUTShape_92f0e9 => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // Value 2 Enumerated PRIMARY; Frame Type same at tables.rs:2237
            Ps3_C_8_12_4_1_1_1befe3 => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // Value 1 Enumerated ORIGINAL/DERIVED; Frame Type same at tables.rs:2237
            Ps3_C_8_12_4_1_1_296245 => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // Multi-Resolution Pyramid Module forbidden unless Image Type Value 3 is VOLUME or THUMBNAIL
            Ps3_A_32_8_3_4f8395 => Tables,
            // NotShared placement flags Frame Content in Shared Functional Groups
            Ps3_A_32_8_4_408ebf => Tables,
            // SharedOnly placement flags Pixel Measures or Frame Type groups in Per-Frame Items
            Ps3_A_32_8_4_706768 => Tables,
            // Enforced for standard WSI Functional Groups; private Functional Groups are not compared
            Ps3_C_7_6_16_1_1_e8853c => Checked("intrinsic-wsi-dicom-iod-modules"),
            // Item counts from Attribute Descriptions enforced; presence by Type at engine.rs:139
            Ps5_7_4_6_048def => Checked("intrinsic-wsi-dicom-iod-sequence-items"),
            // Only CS syntax decidable; non-listed values are legal Defined Term extensions (PS3.5)
            Ps3_8_4_79bf54 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Non-ASCII text without Specific Character Set flagged; default Latin-1 decode keeps bytes observable
            Ps5_6_1_2_1_91c5aa => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Only one trailing SPACE stripped; NUL or other padding fails repertoire/format checks; odd length flagged values.rs:116
            Ps5_6_1_2_3_51ae57 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // U+007F is a control character, rejected in text VRs; AE/CS/UR/numeric repertoires exclude it
            Ps5_6_1_2_3_a287c9 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // SO (0EH) and SI (0FH) are control characters rejected in all text VRs
            Ps5_6_1_2_5_2_0f91c6 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Only TAB, LF, FF, CR (text VRs) and ESC accepted; all other control characters rejected
            Ps5_6_1_3_62ad2e => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // DS exponent after E or e must be optionally signed digits
            Ps5_6_2_0c85c6 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // AE Value consisting solely of spaces rejected
            Ps5_6_2_10b562 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Trailing '.' yields an empty fraction, rejected by the digit check
            Ps5_6_2_14f3d9 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // DA must be eight digits forming a valid Gregorian date
            Ps5_6_2_176ec1 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Group length counted in Unicode scalar values, so combining characters count separately
            Ps5_6_2_1_2_e3d9df => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Characters outside the RFC 3986 set rejected; '%' must precede two hex digits
            Ps5_6_2_2653e9 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // TM fraction requires HHMMSS and 1 to 6 digits
            Ps5_6_2_2720d0 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Maximum lengths checked per Value; only the last Value has one padding character removed, matching the exception
            Ps5_6_2_3981c3 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // ESC and non-ASCII characters outside the RFC 3986 set rejected in UR
            Ps5_6_2_3_019dcb => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Percent-escape syntax checked; percent-decoded bytes not verified as valid UTF-8
            Ps5_6_2_3_f3fccc => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Raw non-ASCII hosts rejected; percent-encoded non-ASCII host names (should be IDNA) not rejected
            Ps5_6_2_3_fb1c60 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // DT fractional second requires full seconds and 1 to 6 digits
            Ps5_6_2_3ed511 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // IS restricted to optional sign and digits after trimming spaces
            Ps5_6_2_4f76de => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // More than three component groups rejected
            Ps5_6_2_57497d => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // DT month and day validated against the Gregorian calendar, including leap years
            Ps5_6_2_5b8243 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Odd UI length flagged values.rs:116; single NUL stripped, other padding fails UID syntax
            Ps5_6_2_5e4dd5 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // DT must start with four year digits
            Ps5_6_2_75c3d6 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Offset -0000 rejected, so UTC must be encoded +0000
            Ps5_6_2_768fdd => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // AS must be three digits followed by D, W, M or Y
            Ps5_6_2_7e81a8 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // More than five components per component group rejected
            Ps5_6_2_80b965 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Odd length flagged values.rs:116; one NUL stripped, SPACE or extra NUL padding fails UID syntax
            Ps5_6_2_82cca4 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // IS outside -2^31..2^31-1 rejected
            Ps5_6_2_b9e2da => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Trailing '.' gives an empty fraction, rejected
            Ps5_6_2_c534bd => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Control characters other than ESC rejected in LO
            Ps5_6_2_daeeb8 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Odd OB length flagged; value of the pad byte (00H) not verified
            Ps5_6_2_e0c655 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Hour limited to 00-23, so 2400 is rejected in TM and DT
            Ps5_6_2_e33c94 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Control characters other than ESC rejected in SH
            Ps5_6_2_f0355c => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Odd Value Length flagged; non-SPACE padding (e.g. NUL) fails repertoire/format checks after single-SPACE strip
            Ps5_6_2_f340d0 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // DT offset suffix must be a sign followed by four digits
            Ps5_6_2_f7c84a => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // DS mantissa restricted to digits, optional sign and one decimal point; embedded spaces rejected
            Ps5_6_2_f892eb => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Wrong delimiters fail format in restricted VRs (CS, DS, IS, UI, DA); free-text VRs undecidable
            Ps5_6_4_0a3e14 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Odd Value Length of the whole Value Field flagged
            Ps5_6_4_3472c2 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Binary Value Length must be a multiple of the element size; delimiters break this or VM
            Ps5_6_4_7a0ae1 => Checked("intrinsic-pixel-structure"),
            // Lead-in to 6.4 delimiting; parser splits strings on 5CH and binary Values by size
            Ps5_6_4_8bab9c => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Padding removed only from the last Value; padding on earlier Values fails restricted formats
            Ps5_6_4_f2e91a => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // NUL stripped only from the last UID; NUL in earlier UIDs fails UID syntax
            Ps5_6_4_fde9f8 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Values split on 5CH and formats checked per Value; wrong delimiters in free text undecidable
            Ps5_7_1_1_88ebbc => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Value formats checked per VR; VM checked by iod-vr-vm at values.rs:85
            Ps5_7_4_1_01939a => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // VR/VM validity checked; whether a known Value was omitted is undecidable
            Ps5_7_4_3_0c7254 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Private Values format-checked by encoded VR in explicit VR; implicit-VR private data is UN
            Ps5_7_8_2_c93d2d => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Odd Value Length of native Pixel Data reported; fragment lengths not covered here
            Ps5_8_1_1_649518 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Only ASCII digits and periods accepted
            Ps5_9_1_5dab00 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Separators limited to '.'; any other character rejected
            Ps5_9_1_98ae51 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Odd UI length flagged (values.rs:116); one trailing NUL removed before the syntax check
            Ps5_9_1_9be579 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Empty components (leading, trailing or doubled periods) and non-digits rejected
            Ps5_9_1_e2c578 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Components with a leading zero rejected unless the single digit 0
            Ps5_9_1_e86dd2 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // UID over 64 characters rejected; a padded 65-byte Value is also flagged as odd length
            Ps5_9_1_fcd445 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // Decidable part: OW even length enforced; LUT entry byte order undecidable
            Ps5_A_1_04740e => Checked("intrinsic-pixel-structure"),
            // Decidable part: OW implies even Value Length, enforced; sample byte order undecidable (Bits Stored = Allocated)
            Ps5_A_1_0482a6 => Checked("intrinsic-pixel-structure"),
            // Umbrella for per-VR encoding under Implicit VR; even-length rule enforced; items classified separately
            Ps5_A_1_0a4a49 => Checked("intrinsic-pixel-structure"),
            // Umbrella for OB/OL/OV/OW; Value Length multiples enforced (values.rs:116-130); items separate
            Ps5_A_1_8481bf => Checked("intrinsic-pixel-structure"),
            // Standard Extended Modality/VOI LUT only; even length enforced; byte order undecidable
            Ps5_A_1_86c594 => Checked("intrinsic-pixel-structure"),
            // Overlay only as Standard Extended attribute; OW even length enforced; byte order undecidable
            Ps5_A_1_ea8197 => Checked("intrinsic-pixel-structure"),
            // PS3.6 VM 2 enforced; anatomical meaning of the two directions is undecidable
            Ps3_C_7_6_1_1_1_b53257 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Parser splits on 5CH, so stray backslash becomes extra Value; VM-1 elements flagged; UR rejects values.rs:574
            Ps5_6_1_2_3_5f1078 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Explicit-VR Private Creators (gggg,0010-00FF) must be LO; UN flagged at values.rs:73
            Ps5_6_2_2_b4d2eb => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // UN on standard tags flagged in explicit VR; >65534 exemption missed (false positive); encoding history undecidable
            Ps5_6_2_2_b96c3e => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // LT parsed as one string; a multi-valued LT cannot be encoded, VM 1 by construction
            Ps5_6_2_5e8cdb => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // SH split on 5CH by parser; stray backslash appears as extra Value, flagged for VM-1 elements
            Ps5_6_2_7c3538 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // UR parsed as a single string, VM 1 by construction; backslash rejected by uri() values.rs:574
            Ps5_6_2_89a67a => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // These VRs parse as one Value (string or byte stream); VM 1 holds by construction
            Ps5_6_4_29bb04 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Explicit VR of standard elements compared with PS3.6 VR alternatives
            Ps5_7_1_1_c8d4ce => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Explicit VR checked against PS3.6 alternatives; implicit VR takes the dictionary VR
            Ps5_7_1_1_ca2b1d => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Explicit VR SQ mismatch flagged; parser re-types undefined-length non-SQ elements as SQ (read.rs:573)
            Ps5_7_5_251684 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // A Sequence element holds one Sequence by construction; VM 1 always
            Ps5_7_5_379e6d => Undecidable("SQ defines one Sequence Value regardless of its Item count; an intended different Value Multiplicity is not encoded"),
            // Retired entries' VR/VM checked; formats use current rules, older-version specifics unchecked
            Ps5_7_7_b39a73 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Walk recurses into private Sequences; standard tags' explicit VR compared with PS3.6
            Ps5_7_8_2_fc38c1 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Even-group tags absent from the PS3.6 registry flagged as unregistered
            Ps5_7_8_992cc6 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_11_75f6c4 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_11_a86261 => OutsideScope("Binds Overlay Data; the VL WSI IOD has no Overlay Plane Module"),
            // Umbrella for OB/OW per tag under JPIP HTJ2K TS; items classified separately
            Ps5_A_11_ef1304 => OutsideScope("The following OB/OW rules bind only Pixel Data and Overlay Data: JPIP replaces Pixel Data with a Provider URL, and the supported pathology IODs have no Overlay Plane Module"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_2_45d343 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Umbrella for OB/OL/OV/OW per tag; PS3.6 VR enforced; items classified separately
            Ps5_A_2_4fe8ab => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR US/OW enforced vs PS3.6; only via Standard Extended Modality/VOI LUT; byte order undecidable
            Ps5_A_2_54edc7 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_2_7687c9 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR OB or OW enforced vs PS3.6 for Pixel Data
            Ps5_A_2_7e2850 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR OW enforced vs PS3.6; LUT entry byte order not decidable
            Ps5_A_2_972477 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR OW enforced vs PS3.6; LUT entry byte order not decidable
            Ps5_A_4_3b7a41 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Umbrella for OB/OW per tag under encapsulated TS; items classified separately
            Ps5_A_4_42ecb9 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_4_45b4b3 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Nested native Pixel Data VR OB or OW enforced vs PS3.6
            Ps5_A_4_5af91b => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_4_5cc995 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR US/OW enforced vs PS3.6; only via Standard Extended Modality/VOI LUT; byte order undecidable
            Ps5_A_4_995a1f => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_6_871dd9 => OutsideScope("Binds Overlay Data; the VL WSI IOD has no Overlay Plane Module"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_6_a6e1bb => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Umbrella for OB/OW per tag under JPIP TS; items classified separately
            Ps5_A_6_c1784c => OutsideScope("The following OB/OW rules bind only Pixel Data and Overlay Data: JPIP replaces Pixel Data with a Provider URL, and the supported pathology IODs have no Overlay Plane Module"),
            // Equal method/ratio VM enforced when flag is 01; per-step correspondence otherwise undecidable
            Ps3_C_7_6_1_1_5_1_f408e9 => Checked("intrinsic-wsi-dicom-lossy-history"),
            // Equal ratio/method VM enforced when flag is 01; per-step correspondence otherwise undecidable
            Ps3_C_7_6_1_1_5_2_a81e8a => Checked("intrinsic-wsi-dicom-lossy-history"),
            // Lossy Transfer Syntax with 00 flagged; earlier lossy history under lossless syntax is undetectable
            Ps3_C_7_6_1_1_5_dfc992 => Checked("intrinsic-wsi-dicom-lossy-history"),
            // UID syntax (identity.rs:208) and in-set UID collisions checked; global uniqueness undecidable
            Ps3_C_7_6_22_1_2_464a33 => Checked("intrinsic-wsi-dicom-specimen-uid-set"),
            // Image Type Value 3 present and non-empty in every profile
            Ps3_C_7_6_1_1_2_ef6680 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Image Flavor non-empty; its Defined Terms are extensible
            Ps3_C_8_12_4_1_1_f4d4e2 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Derived Pixel non-empty; its Defined Terms are extensible
            Ps3_C_8_12_4_1_1_b18b99 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Image Type and every Frame Type have exactly four Values
            Ps3_C_8_12_4_1_1_75b5c5 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Imaged Volume Depth of 0 fails
            Ps3_C_8_12_4_1_2_3e7c7c => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Specimen Label in Image is YES for OVERVIEW and LABEL
            Ps3_C_8_12_4_1_SpecimenLabelInImage_fd0f41 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Specimen Label in Image is NO for THUMBNAIL and VOLUME
            Ps3_C_8_12_4_1_SpecimenLabelInImage_0b6709 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Tiles Overlap other than NONE with TILED_FULL fails
            Ps3_C_8_12_14_1_TilesOverlap_0699b8 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Duplicate Optical Path Identifiers fail
            Ps3_C_8_12_5_1_OpticalPathIdentifier_d84211 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Every Optical Path Identification of a Frame resolves to an Optical Path Sequence Item
            Ps3_C_8_12_5_1_1_d6f6c9 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Position Reference Indicator other than SLIDE_CORNER fails
            Ps3_C_7_4_1_1_2_71f60f => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Lossy Compression derivation (DCM 113040) requires Image Type DERIVED
            Ps3_C_7_6_1_1_5_808c0a => Checked("intrinsic-wsi-dicom-wsi-image"),
            // A derived image naming its own SOP Instance UID as a source fails
            Ps3_C_7_6_1_1_2_c26085 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Pixel Spacing positive; zero only for a single row or column
            Ps3_10_7_1_3_758737 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Negative Spacing Between Slices fails
            Ps3_C_7_6_16_2_SpacingBetweenSlices_7cbf0e => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Frame positions keep one residue modulo Columns and Rows
            Ps3_A_32_8_4_1_2_2c5e3d => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Column/Row Position is a multiple of Columns/Rows plus a constant
            Ps3_A_32_8_4_1_2_922f97 => Checked("intrinsic-wsi-dicom-wsi-image"),
            // Frames lacking the indexed Attribute share one index of their own
            Ps3_C_7_6_17_1_97159c => Checked("intrinsic-wsi-dicom-dimension-order"),
            // A pointer naming an Attribute no Frame has fails
            Ps3_C_7_6_17_1_da4366 => Checked("intrinsic-wsi-dicom-dimension-order"),
            // Pointer to Frame Content Sequence or Dimension Index Values fails
            Ps3_C_7_6_17_1_849d42 => Checked("intrinsic-wsi-dicom-dimension-order"),
            // Functional Group Pointer with a pointer that names a Functional Group Sequence fails
            Ps3_C_7_6_17_1_83308f => Checked("intrinsic-wsi-dicom-dimension-order"),
            // Dimension Index Private Creator matches the reserving block
            Ps3_C_7_6_17_1_0f809b => Checked("intrinsic-wsi-dicom-dimension-order"),
            // Functional Group Private Creator matches the reserving block
            Ps3_C_7_6_17_1_39ce9a => Checked("intrinsic-wsi-dicom-dimension-order"),
            // Same index, nominally same Value (integers exact; decimals apart from other indices)
            Ps3_C_7_6_17_1_2ec4fa => Checked("intrinsic-wsi-dicom-dimension-order"),
            // Dimension Index Values VM equals the Dimension Index Sequence Item count
            Ps3_C_7_6_16_3_DimensionIndexValues_e56fe8 => Checked("intrinsic-wsi-dicom-dimension-order"),
            // Two Frame of Reference UIDs in one Series fail
            Ps3_C_7_4_1_1_1_ea9bd3 => Checked("intrinsic-wsi-dicom-series-set"),
            // A slide-related Frame of Reference UID shared across slides (Container Identifier) fails
            Ps3_C_7_4_1_c35eb2 => Checked("intrinsic-wsi-dicom-series-set"),
            // Images sharing a Frame of Reference UID are of one slide (Container Identifier)
            Ps3_C_7_4_1_1_1_eaeaff => Checked("intrinsic-wsi-dicom-series-set"),
            // One UID in two identifying roles fails within the set; global uniqueness undecidable
            Ps5_9_ad5517 => Checked("intrinsic-wsi-dicom-series-set"),
            // One UID in two identifying roles fails within the set; global uniqueness undecidable
            Ps5_9_5a69d2 => Checked("intrinsic-wsi-dicom-series-set"),
            // Instance Number shared outside one Concatenation within a Series fails
            Ps3_C_7_6_16_1_InstanceNumber_9d6547 => Checked("intrinsic-wsi-dicom-series-set"),
            // Concatenation members' Dimension Index Sequences compared by digest
            Ps3_C_7_6_16_2_2_4_241add => Checked("intrinsic-wsi-dicom-series-set"),
            // Concatenation members' Shared Functional Groups compared by digest
            Ps3_C_7_6_16_2_2_4_3fcf86 => Checked("intrinsic-wsi-dicom-series-set"),
            // Concatenation members' Per-Frame Item structure compared
            Ps3_C_7_6_16_2_2_4_b6a910 => Checked("intrinsic-wsi-dicom-series-set"),
            // Concatenation members agree on all Attributes outside the per-instance ones
            Ps3_C_7_6_16_2_2_4_c53142 => Checked("intrinsic-wsi-dicom-series-set"),
            // SOP Instance UID of Concatenation Source agrees across members
            Ps3_C_7_6_16_1_SOPInstanceUIDOfConcatenationSource_487ffc => Checked("intrinsic-wsi-dicom-series-set"),
            // In-concatenation Numbers are 1..n in Concatenation Frame Offset Number order
            Ps3_C_7_6_16_1_InConcatenationNumber_f490dd => Checked("intrinsic-wsi-dicom-series-set"),
            // In-concatenation Total Number of 1 or less fails
            Ps3_C_7_6_16_1_InConcatenationTotalNumber_4fdb6d => Checked("intrinsic-wsi-dicom-series-set"),
            // A Dimension Organization UID indexing different Attributes across Instances fails
            Ps3_C_7_6_17_2_4ebae3 => Checked("intrinsic-wsi-dicom-series-set"),
            // A localizer in the set with another Frame of Reference UID fails
            Ps3_C_7_6_16_2_5_1_17ac5a => Checked("intrinsic-wsi-dicom-series-set"),
            // One Specimen Identifier and issuer naming two Specimen UIDs in a Study fails
            Ps3_C_7_6_22_1_2_19f6e2 => Checked("intrinsic-wsi-dicom-series-set"),
            // Values 1 and 2 by the enumerations; Values 3 and 4 and the count by the wsi-image rule
            Ps3_C_7_6_1_1_2_a9e514 => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // An empty Value among several of an Enumerated or Defined Term Attribute fails
            Ps5_6_3_ed2fcf => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // A Type 3 Sequence present with zero Items fails
            Ps5_7_4_5_4ef00e => Checked("intrinsic-wsi-dicom-iod-sequence-items"),
            // A private Selector Attribute or Sequence Pointer Value other than (gggg,00xx) fails
            Ps3_10_17_1_2_e5ece3 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // DCMR Context Identifier other than digits without leading zeros fails
            Ps3_8_6_2191da => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // A Data Elements Signed Tag absent from the MAC Parameters Sequence's Data Set fails
            Ps3_C_12_1_1_3_1_1_5b9eec => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Data Elements Signed not in ascending Tag order fails
            Ps3_C_12_1_1_3_1_1_13525c => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Listing an excluded Tag (lengths, group < 0008, UN, FFFA, MAC Parameters, padding, delimiters) fails
            Ps3_C_12_1_1_3_1_1_eb25f8 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // HL7 Structured Document Referenced SOP Class UID outside the HL7 OID root fails
            Ps3_C_12_1_1_6_b6eb69 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // A nonconforming top-level Attribute missing or non-empty in Modified Attributes fails
            Ps3_C_12_1_1_9_1_09372b => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Nonconforming Modified Attributes Item without Selector Attribute fails
            Ps3_C_12_1_1_9_2_52e512 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Source Instance Sequence naming a PS3.6 Image Storage SOP Class fails
            Ps3_C_12_4_1_2_a52a75 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Patient photo references other than VL Photographic or Secondary Capture fail
            Ps3_C_2_2_1_1_70c5e1 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Subject Relative Position Values below 1 fail; holder order is not encoded
            Ps3_C_7_1_4_1_1_1_a03c8a => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // One group Patient ID with the same members in two arrangements fails
            Ps3_C_7_1_4_1_1_1_093320 => Checked("intrinsic-wsi-dicom-series-set"),
            // Date/Time of Last Calibration pairs not ascending fail
            Ps3_C_7_5_1_1_1_697183 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Date and Time of Last Calibration with different Value counts fail
            Ps3_C_7_5_1_1_1_6f6b31 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Biped Patient Orientation Values parse as A/P/R/L/H/F
            Ps3_C_7_6_1_1_1_b78dcd => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Quadruped Patient Orientation Values parse as the C.7.6.1.1.1 abbreviations
            Ps3_C_7_6_1_1_1_d49ff3 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Each non-empty Patient Orientation Value has at least one abbreviation
            Ps3_C_7_6_1_1_1_1e27ca => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // At most three abbreviations per Patient Orientation Value
            Ps3_C_7_6_1_1_1_2a7108 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Preparation steps' DateTime of processing (DCM 111702) not ascending fails; undated steps unordered
            Ps3_C_7_6_22_1_3_372144 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Referenced Frame Number below 1 fails
            Ps3_10_3b_ReferencedFrameNumber_f9c771 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Referenced Frame Number below 1 fails
            Ps3_10_2_ReferencedFrameNumber_587608 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Referenced Frame Number below 1 fails
            Ps3_10_3_ReferencedFrameNumber_174e7c => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_3_ConsultingPhysicianIdentificationSequence_5a5065 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_3_PhysiciansOfRecordIdentificationSequence_19326f => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_3_PhysiciansReadingStudyIdentificationSequence_d559d9 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_5a_PerformingPhysicianIdentificationSequence_0e6615 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_5a_OperatorIdentificationSequence_830798 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Contributing Equipment Operator Identification Items counted against Operators' Name, single Item included
            Ps3_C_12_1_OperatorIdentificationSequence_a0dcbe => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // A caret-delimited Code Meaning is checked as a PN Value; caret-free names are LO
            Ps3_10_1_PersonIdentificationCodeSequence_7d0584 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Numeric Value with more than one Value in a Measured Value Item fails
            Ps3_10_2_NumericValue_728e74 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Floating Point Value VM differs from Numeric Value
            Ps3_10_2_FloatingPointValue_796684 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Rational Numerator Value VM differs from Numeric Value
            Ps3_10_2_RationalNumeratorValue_f9fd94 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Rational Denominator Value VM differs from Numeric Value
            Ps3_10_2_RationalDenominatorValue_cc4bf1 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Image Laterality against Laterality and SCT/SRT laterality modifier codes
            Ps3_C_7_9_ImageLaterality_0d7859 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Color Space against a matrix/TRC ICC Profile's primaries; table-based profiles undecidable
            Ps3_C_8_12_5_1_ColorSpace_36ee23 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Selector Value Number other than 1 for a PS3.6 VM 1 Attribute fails
            Ps3_10_20_SelectorValueNumber_350d86 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Each public Selector Sequence Pointer Value names a Sequence, one per nesting level
            Ps3_10_20_SelectorSequencePointer_baa08d => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Selector Sequence Pointer Private Creator VM equals the pointer's
            Ps3_10_20_SelectorSequencePointerPrivateCreator_379163 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // A Private Creator for a public Selector Sequence Pointer Value fails
            Ps3_10_20_SelectorSequencePointerPrivateCreator_46d4e2 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // Selector Sequence Pointer Items VM equals the pointer's
            Ps3_10_20_SelectorSequencePointerItems_fcd3e1 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // An extraction from a set member with a Frame Extraction Sequence carries its Items plus one
            Ps3_C_12_9_FrameExtractionSequence_74490f => Checked("intrinsic-wsi-dicom-series-set"),
            // Presentation Address dicom:<host>:<port> form
            Ps10_7_1_1_1_5ec65a => Checked("intrinsic-wsi-dicom-file-meta"),
            // Presentation Address absolute http URL with a host
            Ps10_7_1_1_1_1add3e => Checked("intrinsic-wsi-dicom-file-meta"),
            // Presentation Address with scheme https fails
            Ps10_7_1_1_1_3653a7 => Checked("intrinsic-wsi-dicom-file-meta"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_4ea599 => Checked("intrinsic-wsi-dicom-icc-profile"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_43d1c6 => Checked("intrinsic-wsi-dicom-icc-profile"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_6cb654 => Checked("intrinsic-wsi-dicom-icc-profile"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_efa57f => Checked("intrinsic-wsi-dicom-icc-profile"),
            // Pixel Padding Value and Range Limit lie in the stored range, in order
            Ps3_C_7_5_1_1_2_8d8819 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Pixel Padding Value and Range Limit lie in the stored range, in order
            Ps3_C_7_5_1_1_2_62cd75 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_8478b8 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_b7c7c0 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_c2da50 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_c5156c => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_cc071c => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_6776f0 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_aeb525 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_1_1_6_502a69 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_a1e82c => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_c598ef => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_1747e4 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_98671f => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_ad0ea4 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_ed0814 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Optical Path palettes are 16-bit single-sample tables
            Ps3_C_7_6_3_1_5_6b3ce7 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Optical Path palettes are 16-bit single-sample tables
            Ps3_C_7_6_3_1_6_e94d80 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Pixel Aspect Ratio Values are positive
            Ps3_C_7_6_3_1_7_fbe0d7 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Extended Offset Table needs an empty Basic Offset Table and whole Frames
            Ps3_C_7_6_3_1_8_a81faf => Checked("intrinsic-pixel-structure"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_84cfee => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Single-sample tables checked; whether segmented data is desirable is the producer's choice
            Ps3_C_7_9_2_a92a46 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_1_dfe589 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_2_c7a7b2 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_3_a8252a => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_3_01ce43 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_11c_BitsAllocated_f6fab6 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_11c_HighBit_41bada => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            // Bytes after the last Data Element, or a Value past the end, fail from the encoded bytes
            Ps5_7_1_2_85dc52 => Checked("intrinsic-wsi-dicom-encoding"),
            // Top-level Pixel Data without a Sequence Delimitation Item fails from the encoded bytes
            Ps5_7_1_2_d38d67 => Checked("intrinsic-wsi-dicom-encoding"),
            // Top-level Pixel Data without a Sequence Delimitation Item fails from the encoded bytes
            Ps5_7_5_2_e7bdad => Checked("intrinsic-wsi-dicom-encoding"),
            // Native length checked; consistency with codestream headers is the codestream family
            Ps5_8_1_8ae787 => Checked("intrinsic-pixel-structure"),
            // Cell count fixes the Native length; bit packing within a cell is not encoded
            Ps5_8_1_1_993326 => Undecidable("The declared Pixel Cell width defines one sample field; the bytes cannot reveal an intended subdivision into multiple sample fields"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_2b593e => Checked("intrinsic-pixel-structure"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_b26202 => Checked("intrinsic-pixel-structure"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_018c03 => Checked("intrinsic-pixel-structure"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_5123f3 => Checked("intrinsic-pixel-structure"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_5_2ae16d => Checked("intrinsic-wsi-dicom-image-metadata"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_5_eab09f => Checked("intrinsic-wsi-dicom-image-metadata"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_6_d69f14 => OutsideScope("MPEG2 Main Profile always applies the irreversible multi-component transformation specified by 8.2.5 and 8.2.6; the condition that no multi-component transformation was applied cannot occur in these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_6_601be2 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_6_303ae3 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // The image-metadata check enforces the syntax-specific dimensions and declared frame-rate limits
            Ps5_8_2_5_7430f5 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_5_8536a7 => Checked("intrinsic-pixel-structure"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_5_f85091 => Checked("intrinsic-pixel-structure"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_6_862741 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_6_ae499d => Checked("intrinsic-wsi-dicom-image-metadata"),
            // The image-metadata check enforces the syntax-specific dimensions and declared frame-rate limits
            Ps5_8_2_6_fdcabf => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_6_a4b86d => Checked("intrinsic-pixel-structure"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_6_6b3ca2 => Checked("intrinsic-pixel-structure"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_7_e55180 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_7_020679 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_7_53db66 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_7_8993db => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_7_bb3858 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The image-metadata check enforces the syntax-specific dimensions and declared frame-rate limits
            Ps5_8_2_7_c328ad => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_7_7c3a60 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_7_9565ae => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_8_1d080a => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_8_6f91d1 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_8_8fdb21 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_8_fedd49 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_8_1973b2 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The image-metadata check enforces the syntax-specific dimensions and declared frame-rate limits
            Ps5_8_2_8_7718ba => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_8_bc3138 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_8_e51cd9 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_9_ff677c => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_10_b7f63b => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_10_226719 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_10_bd5c63 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_10_58520f => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_10_d25e20 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The image-metadata check enforces the syntax-specific dimensions and declared frame-rate limits
            Ps5_8_2_10_84b09e => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_11_e52f4e => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_11_514e75 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_11_9c3b42 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_11_649e03 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The syntax-specific attributes are checked; color-only motion encodings have no conformant VL WSI form
            Ps5_8_2_11_385c34 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // The image-metadata check enforces the syntax-specific dimensions and declared frame-rate limits
            Ps5_8_2_11_f9f4eb => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_1_3fcdef => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_1_777bab => Checked("intrinsic-wsi-dicom-encoding"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_1_2efc4a => Undecidable("Defines the UID assigned to this Transfer Syntax; an alternative UID identifies a different syntax, and the encoded bytes do not record an intended different UID assignment"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_2_2fd46a => Checked("intrinsic-wsi-dicom-encoding"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_2_6239c9 => Undecidable("Defines the UID assigned to this Transfer Syntax; an alternative UID identifies a different syntax, and the encoded bytes do not record an intended different UID assignment"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_A_4_61f90c => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_4_238b32 => Checked("intrinsic-wsi-dicom-encoding"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_33a347 => Checked("intrinsic-pixel-structure"),
            // Fragment Items have an even length of at least two bytes, from the encoded bytes
            Ps5_A_4_fd123f => Checked("intrinsic-wsi-dicom-encoding"),
            // A missing Basic Offset Table Item misaligns the Fragments and fails
            Ps5_A_4_9063da => Checked("intrinsic-pixel-structure"),
            // Basic Offset Table validated whenever present; empty beside an Extended Offset Table
            Ps5_A_4_82c271 => Checked("intrinsic-pixel-structure"),
            // Encapsulated Pixel Data is OB and ends with a zero-length Sequence Delimitation Item
            Ps5_A_4_f5f245 => Checked("intrinsic-wsi-dicom-encoding"),
            // One Fragment per Frame checked; the RLE header in each Fragment is the codestream family
            Ps5_A_4_2_c5d6d5 => Checked("intrinsic-pixel-structure"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_2_6687ec => Checked("intrinsic-pixel-structure"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_4_72c08f => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_3b48ff => Undecidable("Defines the UID assigned to this Transfer Syntax; an alternative UID identifies a different syntax, and the encoded bytes do not record an intended different UID assignment"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_dd388a => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_f1cc7f => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_acf161 => Checked("intrinsic-pixel-structure"),
            // Encapsulated Pixel Data is OB and ends with a zero-length Sequence Delimitation Item
            Ps5_A_4_11_bd0aed => Checked("intrinsic-wsi-dicom-encoding"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_12_a741e8 => Checked("intrinsic-pixel-structure"),
            // One Fragment per Frame checked; separate RFC 1951 streams would need inflating
            Ps5_A_4_13_7626f4 => Checked("intrinsic-pixel-structure"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_13_14f644 => Checked("intrinsic-pixel-structure"),
            // Deflated Data Sets inflate; undeflated bytes declaring the syntax fail parsing
            Ps5_A_5_97bfdd => Undecidable("Defines the UID assigned to this Transfer Syntax; an alternative UID identifies a different syntax, and the encoded bytes do not record an intended different UID assignment"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_A_6_dcb6d6 => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_6_e0a5fd => OutsideScope("JPIP Referenced syntaxes are network transfers: a Part 10 file cannot use them and DICOM JSON carries no Data Set byte order, so no object wsi-dicom validates has a JPIP Data Set encoding"),
            // JPIP Referenced syntaxes admit MONOCHROME1/2, YBR_ICT and YBR_RCT only
            Ps5_A_6_71e374 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_A_11_3cba9b => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_11_ce7500 => OutsideScope("JPIP Referenced syntaxes are network transfers: a Part 10 file cannot use them and DICOM JSON carries no Data Set byte order, so no object wsi-dicom validates has a JPIP Data Set encoding"),
            // Pixel Data forbidden and Pixel Data Provider URL required under all four JPIP syntaxes
            Ps5_A_11_3b11ba => Checked("intrinsic-pixel-structure"),
            // JPIP Referenced syntaxes admit MONOCHROME1/2, YBR_ICT and YBR_RCT only
            Ps5_A_11_5f6235 => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Pixel Data Provider URL required with .4.204/.4.205 too (PS3.5 A.11, A.12)
            Ps5_A_11_47ddc0 => Undecidable("Assigns the JPIP Referenced Transfer Syntax UID; an alternative UID identifies another syntax, and the bytes do not record an intended different assignment"),
            // Pixel Data Provider URL required with .4.204/.4.205 too (PS3.5 A.11, A.12)
            Ps5_A_12_6b70c4 => Undecidable("Defines the UID assigned to this Transfer Syntax; an alternative UID identifies a different syntax, and the encoded bytes do not record an intended different UID assignment"),
            // JPEG Extended admits only its listed Photometric Interpretations
            Ps5_8_2_1_fe09df => Checked("intrinsic-wsi-dicom-image-metadata"),
            // JPEG XL Photometric Interpretation follows Table 8.2.15-1
            Ps5_8_2_15_c4fa2a => Checked("intrinsic-wsi-dicom-image-metadata"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_8_4_1_dc90bc => Checked("intrinsic-pixel-structure"),
            // JPIP Referenced syntaxes, all four, are not used in PS3.10 files
            Ps5_10_8_d4693e => Checked("intrinsic-wsi-dicom-file-meta"),
            // Pixel Data forbidden and Pixel Data Provider URL required under all four JPIP syntaxes
            Ps5_A_6_be37ad => Checked("intrinsic-pixel-structure"),
            // PS3.10 files declaring .4.94 or .4.95 fail file-meta; the stream type is on the server
            Ps5_A_6_3ef132 => Undecidable("Assigns the JPIP Referenced Transfer Syntax UID; an alternative UID identifies another syntax, and the bytes do not record an intended different assignment"),
            // PS3.10 files declaring .4.94 or .4.95 fail file-meta; the stream type is on the server
            Ps5_A_7_e5c0fa => Undecidable("Defines the UID assigned to this Transfer Syntax; an alternative UID identifies a different syntax, and the encoded bytes do not record an intended different UID assignment"),
            // All components share size precision and sign
            Ps3_C_7_6_3_1_1_c17c68 => Checked("intrinsic-wsi-dicom-codestream"),
            // Every frame is checked against the attributes that define it
            Ps3_C_7_6_6_1_1_899f77 => Checked("intrinsic-wsi-dicom-codestream"),
            // Color declarations agree with photometric interpretation
            Ps3_C_8_12_4_1_5_2d19f4 => Checked("intrinsic-wsi-dicom-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_BitsAllocated_69f390 => Checked("intrinsic-wsi-dicom-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_BitsStored_d52069 => Checked("intrinsic-wsi-dicom-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_HighBit_a62636 => Checked("intrinsic-wsi-dicom-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_PixelRepresentation_178735 => Checked("intrinsic-wsi-dicom-codestream"),
            // Frame header agrees with rows columns samples and bits stored
            Ps5_8_2_1_803014 => Checked("intrinsic-wsi-dicom-codestream"),
            // Embedded ICC profiles equal the ICC profile of the frame
            Ps5_8_2_1_d49433 => Checked("intrinsic-wsi-dicom-codestream"),
            // Header and segments agree with the image pixel attributes
            Ps5_8_2_2_c961f8 => Checked("intrinsic-wsi-dicom-codestream"),
            // Frame header agrees with the image pixel attributes
            Ps5_8_2_3_c509e8 => Checked("intrinsic-wsi-dicom-codestream"),
            // Image and tile size agrees with the image pixel attributes
            Ps5_8_2_4_47cc48 => Checked("intrinsic-wsi-dicom-codestream"),
            // Multiple component transformation matches photometric interpretation
            Ps5_8_2_4_478fac => Checked("intrinsic-wsi-dicom-codestream"),
            // Multiple component transformation matches photometric interpretation
            Ps5_8_2_4_045661 => Checked("intrinsic-wsi-dicom-codestream"),
            // Multiple component transformation matches photometric interpretation
            Ps5_8_2_4_f23101 => Checked("intrinsic-wsi-dicom-codestream"),
            // Image and tile size agrees with the image pixel attributes
            Ps5_8_2_14_b9d8c7 => Checked("intrinsic-wsi-dicom-codestream"),
            // HTJ2K multiple component transformation matches photometric interpretation
            Ps5_8_2_14_557a1e => Checked("intrinsic-wsi-dicom-codestream"),
            // HTJ2K multiple component transformation matches photometric interpretation
            Ps5_8_2_14_8c86ea => Checked("intrinsic-wsi-dicom-codestream"),
            // HTJ2K multiple component transformation matches photometric interpretation
            Ps5_8_2_14_6f1193 => Checked("intrinsic-wsi-dicom-codestream"),
            // Headers agree with the image pixel attributes
            Ps5_8_2_15_091321 => Checked("intrinsic-wsi-dicom-codestream"),
            // HTJ2K RPCL codestreams progress by resolution with tile lengths
            Ps5_10_18_1_b86460 => Checked("intrinsic-wsi-dicom-codestream"),
            // HTJ2K RPCL codestreams progress by resolution with tile lengths
            Ps5_10_18_1_a950cd => Checked("intrinsic-wsi-dicom-codestream"),
            // HTJ2K RPCL codestreams progress by resolution with tile lengths
            Ps5_10_18_1_080cc2 => Checked("intrinsic-wsi-dicom-codestream"),
            // Coding process is the one its transfer syntax identifies
            Ps5_A_4_1_1e2871 => Checked("intrinsic-wsi-dicom-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_1_55978a => Checked("intrinsic-wsi-dicom-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_1_b3fb04 => Checked("intrinsic-wsi-dicom-codestream"),
            // Frames carry their table specifications
            Ps5_A_4_1_c5999c => Checked("intrinsic-wsi-dicom-codestream"),
            // Lossless transfer syntax admits only near zero
            Ps5_A_4_3_e86721 => Checked("intrinsic-wsi-dicom-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_3_af4cd4 => Checked("intrinsic-wsi-dicom-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_3_116d52 => Checked("intrinsic-wsi-dicom-codestream"),
            // Frames carry all parameter specifications
            Ps5_A_4_3_79e44a => Checked("intrinsic-wsi-dicom-codestream"),
            // Lossless transfer syntaxes use the reversible path
            Ps5_A_4_4_7d7f64 => Checked("intrinsic-wsi-dicom-codestream"),
            // Part 1 transfer syntaxes admit no HTJ2K or Part 2 coding
            Ps5_A_4_4_5e53e6 => Checked("intrinsic-wsi-dicom-codestream"),
            // HTJ2K transfer syntaxes identify HTJ2K codestreams
            Ps5_A_4_4_259d88 => Checked("intrinsic-wsi-dicom-codestream"),
            // Each frame is one complete separately encoded codestream
            Ps5_A_4_4_e0caa8 => Checked("intrinsic-wsi-dicom-codestream"),
            // Each frame is one complete separately encoded codestream
            Ps5_A_4_4_480185 => Checked("intrinsic-wsi-dicom-codestream"),
            // Frames hold a JPEG 2000 codestream
            Ps5_A_4_4_05e878 => Checked("intrinsic-wsi-dicom-codestream"),
            // JP2 file format header is not included
            Ps5_A_4_4_044fb8 => Checked("intrinsic-wsi-dicom-codestream"),
            // Part 2 transfer syntaxes admit only annex j extensions
            Ps5_A_4_4_ab7220 => Checked("intrinsic-wsi-dicom-codestream"),
            // A fragment holds one component collection
            Ps5_A_4_4_6e9c4b => Checked("intrinsic-wsi-dicom-codestream"),
            // Coding mode is the one its transfer syntax identifies
            Ps5_A_4_12_814b6e => Checked("intrinsic-wsi-dicom-codestream"),
            // Segments are an even number of bytes padded with zero
            Ps5_G_3_3774f3 => Checked("intrinsic-wsi-dicom-codestream"),
            // Three byte repeats are replicate runs
            Ps5_G_3_1_961ac0 => Checked("intrinsic-wsi-dicom-codestream"),
            // Runs do not cross row boundaries
            Ps5_G_3_1_5f6b0b => Checked("intrinsic-wsi-dicom-codestream"),
            // Unused segment offsets are zero
            Ps5_G_5_92c185 => Checked("intrinsic-wsi-dicom-codestream"),
            // Stream kind is the one its transfer syntax identifies
            Ps10_7_2_770df5 => Checked("intrinsic-wsi-dicom-codestream"),
            // A character set is named once
            Ps3_C_12_1_1_2_fd6abd => Checked("intrinsic-wsi-dicom-encoding"),
            // Unicode and GB character sets are the only value
            Ps3_C_12_1_1_2_fd82cf => Checked("intrinsic-wsi-dicom-encoding"),
            // UTF-8 is minimal and valid
            Ps3_C_12_1_1_2_a2828b => Checked("intrinsic-wsi-dicom-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_1_7_1_45ff05 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_1_7_1_30c8d1 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_1_7_2_98e21e => Checked("intrinsic-wsi-dicom-encoding"),
            // Timezone offset from UTC is encoded as signed hours and minutes
            Ps3_C_12_1_1_8_5d989e => Checked("intrinsic-wsi-dicom-encoding"),
            // Timezone offset from UTC is encoded as signed hours and minutes
            Ps3_C_12_1_1_8_589a40 => Checked("intrinsic-wsi-dicom-encoding"),
            // Items reserve their own blocks
            Ps3_C_12_1_1_9_1_8190ee => Checked("intrinsic-wsi-dicom-encoding"),
            // A Series has a single Frame of Reference UID
            Ps3_C_7_4_1_1_1_ca125c => Checked("intrinsic-wsi-dicom-series-set"),
            // An empty Image Type Value 1 or 2 is not an Enumerated Value
            Ps3_C_7_6_1_1_2_e953ef => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // An empty Image Type Value 1 or 2 is not an Enumerated Value
            Ps3_C_7_6_1_1_2_d4e4dd => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // Signature and encryption attributes follow their encoding rules
            Ps3_C_12_1_EncryptedContentTransferSyntaxUID_6d1938 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_NonidentifyingPrivateElements_1b7b56 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_IdentifyingPrivateElements_53d273 => Checked("intrinsic-wsi-dicom-encoding"),
            // Signature and encryption attributes follow their encoding rules
            Ps3_C_12_6_MACCalculationTransferSyntaxUID_c73a1b => Checked("intrinsic-wsi-dicom-encoding"),
            // Signature and encryption attributes follow their encoding rules
            Ps3_C_12_6_DigitalSignatureDateTime_c19323 => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_1_c5c47d => Checked("intrinsic-wsi-dicom-encoding"),
            // The C1 set is not used
            Ps5_6_1_1_676368 => Checked("intrinsic-wsi-dicom-encoding"),
            // Text beyond the default repertoire needs specific character set
            Ps5_6_1_2_2_aaf21c => Checked("intrinsic-wsi-dicom-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_3_2a236f => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_3_93d068 => Checked("intrinsic-wsi-dicom-encoding"),
            // Code extension needs a multi valued specific character set
            Ps5_6_1_2_4_213779 => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_d4ec20 => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_1_1c6da1 => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_1_54e495 => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_2_396321 => Checked("intrinsic-wsi-dicom-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_5_3_4b3ce3 => Checked("intrinsic-wsi-dicom-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_5_3_68512a => Checked("intrinsic-wsi-dicom-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_5_3_1fdc6c => Checked("intrinsic-wsi-dicom-encoding"),
            // Code extension needs a multi valued specific character set
            Ps5_6_1_2_5_4_3ea86a => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_3_c65780 => Checked("intrinsic-wsi-dicom-encoding"),
            // Text lines end in CR LF
            Ps5_6_1_3_b90cab => Checked("intrinsic-wsi-dicom-encoding"),
            // Padding is space for text null for UIDs and OB
            Ps5_6_2_93b003 => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences are not counted in value lengths
            Ps5_6_2_76a6fd => Checked("intrinsic-wsi-dicom-encoding"),
            // Backslash bytes inside multi byte characters are not present
            Ps5_6_2_8c8195 => Checked("intrinsic-wsi-dicom-encoding"),
            // A veterinary patient name has two components
            Ps5_6_2_ff8eab => Checked("intrinsic-wsi-dicom-encoding"),
            // The alphabetic component group uses value 1 without escapes
            Ps5_6_2_1_2_c9f7ed => Checked("intrinsic-wsi-dicom-encoding"),
            // The alphabetic component group uses value 1 without escapes
            Ps5_6_2_1_2_951141 => Checked("intrinsic-wsi-dicom-encoding"),
            // The alphabetic component group uses value 1 without escapes
            Ps5_6_2_1_2_ca8329 => Checked("intrinsic-wsi-dicom-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_2_1_2_12c186 => Checked("intrinsic-wsi-dicom-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_2_1_2_8d573f => Checked("intrinsic-wsi-dicom-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_2_1_2_ed7e68 => Checked("intrinsic-wsi-dicom-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_2_1_2_46301a => Checked("intrinsic-wsi-dicom-encoding"),
            // Undefined length unknown items stay in implicit VR
            Ps5_6_2_2_894175 => Checked("intrinsic-wsi-dicom-encoding"),
            // Data elements are ordered and unique in data sets and items
            Ps5_7_1_c4fdc1 => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_7_1_1_6bb31f => Checked("intrinsic-wsi-dicom-encoding"),
            // Reserved bytes after a long VR are zero
            Ps5_7_1_2_6cc044 => Checked("intrinsic-wsi-dicom-encoding"),
            // Group lengths match their groups
            Ps5_7_2_e5da2e => Checked("intrinsic-wsi-dicom-encoding"),
            // Delimiters have zero length and no value
            Ps5_7_5_1_aaae21 => Checked("intrinsic-wsi-dicom-encoding"),
            // Reserved groups are not used
            Ps5_7_5_1_0563be => Checked("intrinsic-wsi-dicom-encoding"),
            // Data elements are ordered and unique in data sets and items
            Ps5_7_5_1_a89833 => Checked("intrinsic-wsi-dicom-encoding"),
            // Items hold no command file meta or directory groups
            Ps5_7_5_1_3363bd => Checked("intrinsic-wsi-dicom-encoding"),
            // Delimiters have zero length and no value
            Ps5_7_5_2_29cc8f => Checked("intrinsic-wsi-dicom-encoding"),
            // Delimiters have zero length and no value
            Ps5_7_5_2_94b19c => Checked("intrinsic-wsi-dicom-encoding"),
            // Specific character set appears in items only where the IOD defines it
            Ps5_7_5_3_397ec3 => Checked("intrinsic-wsi-dicom-encoding"),
            // Repeating groups are the overlay groups only
            Ps5_7_6_dea12f => Checked("intrinsic-wsi-dicom-encoding"),
            // Data elements are ordered and unique in data sets and items
            Ps5_7_8_127399 => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_7_8_ec77b4 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private groups do not use the unreservable elements
            Ps5_7_8_1_8e6821 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private elements need their private creator
            Ps5_7_8_1_645952 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private creators hold one unique default repertoire code
            Ps5_7_8_1_783078 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private creators hold one unique default repertoire code
            Ps5_7_8_1_8472d4 => Checked("intrinsic-wsi-dicom-encoding"),
            // Private creators hold one unique default repertoire code
            Ps5_7_8_1_9dc403 => Checked("intrinsic-wsi-dicom-encoding"),
            // Reserved groups are not used
            Ps5_7_8_1_83055e => Checked("intrinsic-wsi-dicom-encoding"),
            // Items reserve their own blocks
            Ps5_7_8_1_195bb9 => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_7_8_2_2daf2a => Checked("intrinsic-wsi-dicom-encoding"),
            // Private sequences hold no pixel overlay or waveform data
            Ps5_7_8_2_f3885e => Checked("intrinsic-wsi-dicom-encoding"),
            // Privately defined items do not use the DICOM root
            Ps5_9_b45977 => Checked("intrinsic-wsi-dicom-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_1_debced => Checked("intrinsic-wsi-dicom-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_1_75aab3 => Checked("intrinsic-wsi-dicom-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_1_461476 => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_2_465474 => Checked("intrinsic-wsi-dicom-encoding"),
            // Native pixel data VR follows bits allocated
            Ps5_A_2_abe7d8 => Checked("intrinsic-wsi-dicom-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_2_2c73c1 => Checked("intrinsic-wsi-dicom-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_2_5dcbfa => Checked("intrinsic-wsi-dicom-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_2_497172 => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_4_878ca2 => Checked("intrinsic-wsi-dicom-encoding"),
            // Native pixel data VR follows bits allocated
            Ps5_A_4_7baed6 => Checked("intrinsic-wsi-dicom-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_4_bda4a7 => Checked("intrinsic-wsi-dicom-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_4_19a420 => Checked("intrinsic-wsi-dicom-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_4_c9f614 => Checked("intrinsic-wsi-dicom-encoding"),
            // Deflated data sets are inflated and padded to even length
            Ps5_A_5_04de07 => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_6_34f1d3 => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_11_836dbf => Checked("intrinsic-wsi-dicom-encoding"),
            // Items hold no command file meta or directory groups
            Ps10_7_1_3a8346 => Checked("intrinsic-wsi-dicom-encoding"),
            // A file holds one data set
            Ps10_7_2_f911da => Checked("intrinsic-wsi-dicom-encoding"),
            // A UI Value under the DICOM root that PS3.6 Annex A does not register fails; private roots are undecidable
            Ps5_9_2_b903b5 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            // The MAC is recomputed over the listed Data Elements; a Signature that does not verify fails
            Ps3_C_12_1_1_3_1_1_b3d0ca => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // A signed Sequence enters the MAC with every Item's Data Elements but the excluded ones
            Ps3_C_12_1_1_3_1_1_9cd4e0 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // Item Tags and a Sequence Delimitation Item Tag enter the MAC of a signed Sequence
            Ps3_C_12_1_1_3_1_1_5a8e43 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // Encapsulated Pixel Data enters the MAC with an Item Tag before each Fragment
            Ps3_C_12_1_1_3_1_1_88f6d4 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // A MAC Calculation Transfer Syntax that is not Explicit VR Little Endian, or one encoding the Pixel Data otherwise than the file, fails
            Ps3_C_12_1_1_3_1_2_793fd1 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // The stream is hashed with MAC Algorithm; a term outside Table C.12.1.1.3.1.2-1 fails
            Ps3_C_12_1_1_3_1_2_e64d07 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // Other Data Elements enter the MAC whole, with their Value Length
            Ps3_C_12_1_1_3_1_2_f55b08 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // SQ and undefined-length OB enter the MAC without their Value Length
            Ps3_C_12_1_1_3_1_2_974e7a => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // A Sequence Delimitation Item Tag follows every signed Sequence and encapsulated Pixel Data
            Ps3_C_12_1_1_3_1_2_725031 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // The signature Item's Attributes but certificate, Signature and timestamp follow; verified with the RSA key of Certificate of Signer, whose trust is undecidable
            Ps3_C_12_1_1_3_1_2_e3cab5 => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // A CMS_TSP token whose messageImprint is not the Signature's hash, or whose embedded TSA signature fails, fails; when it was obtained is undecidable
            Ps3_C_12_6_CertifiedTimestamp_bedcdd => Checked("intrinsic-wsi-dicom-digital-signatures"),
            // ---- Bind IODs, encodings or contexts a VL WSI object cannot carry. ----
            Ps10_7_4_fb6b0c => OutsideScope("A Secure DICOM File is a CMS envelope (PS3.10 7.4), not a Part 10 file: it lacks the DICM prefix the validator reads files by, and the DICOM File it encapsulates is encrypted for recipients whose keys a validator does not hold"),
            Ps10_8_1_02c223 => OutsideScope("Binds how a Media Format (PS3.12) holds the File ID naming space; a file's bytes record no File ID or media structure"),
            Ps10_8_1_1b75db => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_1_b3ba17 => OutsideScope("Binds the PS3.12 Media Format specifications themselves, not files or File-sets"),
            Ps10_8_1_efcf92 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_6_0ebc18 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_6_572268 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_6_6f3918 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_6_cab9e6 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_6_cda158 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_6_f6c0f3 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            Ps10_8_968f45 => OutsideScope("Lead-in to the requirements on physical media and Media Formats (PS3.12); a file's bytes carry no medium"),
            Ps3_C_2_1_122fa1 => OutsideScope("C.2.1 Patient Relationship Module belongs to the normalized Patient IOD of PS3.4 Annex F; no composite VL WSI object carries it"),
            Ps3_C_2_3_1a990f => OutsideScope("C.2.3 Patient Demographic Module belongs to the normalized Patient IOD of PS3.4 Annex F; VL WSI patient data come from the C.7.1.1 rows the IOD tables check"),
            Ps3_C_2_3_99887b => OutsideScope("C.2.3 Patient Demographic Module belongs to the normalized Patient IOD of PS3.4 Annex F; no composite VL WSI object carries it"),
            Ps3_C_2_4_9fde48 => OutsideScope("C.2.4 Patient Medical Module belongs to the normalized Patient IOD of PS3.4 Annex F; no composite VL WSI object carries it"),
            Ps3_C_7_10_1_1_1_053155 => OutsideScope("Defines the X-Ray irradiation event an Irradiation Event UID names; visible light microscopy has no ionizing irradiation source, so a VL WSI acquisition has no such event to count"),
            Ps3_C_7_10_1_1_1_b59c2b => OutsideScope("Pulsed fluoroscopy example of an X-Ray irradiation event; a VL WSI acquisition has no ionizing irradiation source"),
            Ps3_C_7_11c_Columns_0168ee => OutsideScope("Native downsampled chroma only; VL WSI admits a downsampled Photometric Interpretation (YBR_FULL_422) only in JPEG syntaxes, never Native (C.8.12.4.1.5, checked by intrinsic-wsi-dicom-image-metadata), and Icon Images are single-sample"),
            Ps3_C_7_11c_Rows_456628 => OutsideScope("Native downsampled chroma only; VL WSI admits a downsampled Photometric Interpretation (YBR_FULL_422) only in JPEG syntaxes, never Native (C.8.12.4.1.5, checked by intrinsic-wsi-dicom-image-metadata), and Icon Images are single-sample"),
            Ps3_C_7_4_1_1_2_c02f4d => OutsideScope("CORNEAL_VERTEX_R and _L identify an ophthalmic Frame of Reference; a VL WSI Frame of Reference is slide-related, identified by SLIDE_CORNER, which intrinsic-wsi-dicom-wsi-image checks"),
            Ps3_C_7_5_1_1_2_efdd0a => OutsideScope("Binds MONOCHROME1 padding; VL WSI images are MONOCHROME2 or color (C.8.12.4.1.5, checked by intrinsic-wsi-dicom-image-metadata), and Icon Images carry no Pixel Padding Value"),
            Ps3_C_7_6_16_2_25_2f8353 => OutsideScope("Unassigned Shared Converted Attributes Macro is Legacy Converted only; not in Table A.32.8-2"),
            Ps3_C_7_6_16_2_25_8fe231 => OutsideScope("Matching rule for Legacy Converted conversion; WSI has no converted-attribute macros"),
            Ps3_C_7_6_16_2_25_b8dc4e => OutsideScope("Unassigned Per-Frame Converted Attributes Macro is Legacy Converted only; not in Table A.32.8-2"),
            Ps3_C_7_6_16_2_2_6_d4a473 => OutsideScope("Binds PET Dynamic images, Image Type Value 3 DYNAMIC; VL WSI Image Type Value 3 is VOLUME, LABEL, OVERVIEW or THUMBNAIL"),
            Ps3_C_7_6_16_2_2_6_f8bd55 => OutsideScope("Binds the dimensions of PET Dynamic images, Image Type Value 3 DYNAMIC; VL WSI Image Type Value 3 is VOLUME, LABEL, OVERVIEW or THUMBNAIL"),
            Ps3_C_7_6_16_2_3_1_209538 => OutsideScope("SAMPLED Volumetric Properties excluded by WSI enumeration VOLUME; Plane Position/Orientation (Patient) absent"),
            Ps3_C_7_6_16_2_3_1_39c820 => OutsideScope("Slice Thickness for SAMPLED frames; WSI Volumetric Properties is always VOLUME"),
            Ps3_C_7_6_16_2_3_1_94e7cc => OutsideScope("Image Position (Patient) for SAMPLED frames; neither occurs in WSI"),
            Ps3_C_7_6_16_2_3_1_95795d => OutsideScope("Image Orientation (Patient) for SAMPLED frames; neither occurs in WSI"),
            Ps3_C_7_6_1_1_1_ab0c02 => OutsideScope("Image Orientation (Patient) belongs to no Module or Functional Group of the WSI IOD"),
            Ps3_C_7_6_2_1_1_29ac83 => OutsideScope("Image Orientation (Patient) normalization; WSI uses Image Orientation (Slide) instead"),
            Ps3_C_7_6_2_1_1_34cf77 => OutsideScope("Image Orientation (Patient) cosines; WSI uses Image Orientation (Slide) instead"),
            Ps3_C_7_6_2_1_1_859096 => OutsideScope("Image Position/Orientation (Patient) pair; Image Plane Module not in WSI IOD"),
            Ps3_C_7_6_3_1_2_5fa00b => OutsideScope("Native YBR_FULL_422 sample layout; VL WSI admits YBR_FULL_422 only in JPEG syntaxes, whose codestreams fix the layout (C.8.12.4.1.5, checked by intrinsic-wsi-dicom-image-metadata), and Icon Images are single-sample"),
            Ps3_C_7_6_3_1_2_7be829 => OutsideScope("YBR_PARTIAL_420 chroma siting; that Photometric Interpretation is excluded from VL WSI images (C.8.12.4.1.5) and Icon Images (C.7.6.1.1.6), and both exclusions are checked"),
            Ps3_C_7_6_3_1_3_f9dea3 => OutsideScope("Planar Configuration 1 layout; VL WSI enumerates Planar Configuration 0 (checked by intrinsic-wsi-dicom-image-metadata) and Icon Images carry none (checked by intrinsic-wsi-dicom-pixel-attributes)"),
            Ps3_C_7_6_3_1_5_1d78b7 => OutsideScope("Applies only to the Segmentation IOD"),
            Ps3_C_7_6_3_1_5_7659ed => OutsideScope("Binds Palette Color Lookup Tables within the Color Palette IOD A.58; a VL WSI Optical Path palette has 16-bit entries (Table C.7-22a in an Image IOD, checked by intrinsic-wsi-dicom-pixel-attributes)"),
            Ps3_C_7_6_3_1_5_b1076b => OutsideScope("Alpha Palette Color Lookup Table Descriptor (0028,1104) belongs to the Enhanced Palette Color Lookup Table Module of presentation and blending IODs; no VL WSI Module or Macro includes it"),
            Ps3_C_7_9_1_a8a229 => OutsideScope("Binds Palette Color Lookup Table UID within the Color Palette IOD A.58; VL WSI includes no Palette Color Lookup Table Module"),
            Ps3_C_7_9_PresentationLUTShape_579186 => OutsideScope("INVERSE binds MONOCHROME1; VL WSI images are MONOCHROME2 or color, with Presentation LUT Shape IDENTITY for MONOCHROME2 (C.8.12.4, checked by intrinsic-wsi-dicom-monochrome-presentation)"),
            Ps3_C_8_12_2_1_1_4fc3ea => OutsideScope("Image Center Point Coordinates Sequence (Slide Coordinates Module) is not in WSI IOD"),
            Ps3_C_8_12_5_1_1_6f325a => OutsideScope("WSI always includes Multi-frame Functional Groups (M); condition targets other IODs"),
            Ps3_C_8_16_1_1_3d5347 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_1_d275fb => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_2_9cda19 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_2_d4b014 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_3_66c2ea => OutsideScope("C.8.16.1.3 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_3e29fe => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_4_0067a1 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_4_2f3775 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_4_e1a716 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_4d8cea => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_95e046 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_1_a44052 => OutsideScope("C.8.16.1 specializes Image Type and Frame Type for Enhanced CT, MR, US and Photoacoustic images; VL WSI specializes them in C.8.12.4.1.1 (checked by intrinsic-wsi-dicom-wsi-image), and its Frame Type is a Shared Functional Group, so MIXED never arises"),
            Ps3_C_8_16_2_1_3_2c806a => OutsideScope("Volume Based Calculation Technique (0008,9207) belongs to the Common CT/MR and Photoacoustic Image Description Macro, which no VL WSI Module or Functional Group includes"),
            Ps3_C_8_16_2_1_3_49f9d6 => OutsideScope("Volume Based Calculation Technique (0008,9207) belongs to the Common CT/MR and Photoacoustic Image Description Macro, which no VL WSI Module or Functional Group includes"),
            Ps3_C_8_30_3_1_4_6f526b => OutsideScope("Binds Corneal Topography Map data (C.8.30.3); no VL WSI Module carries corneal coordinates"),
            Ps3_C_8_30_3_1_4_b39027 => OutsideScope("Binds Corneal Topography Map data (C.8.30.3); no VL WSI Module carries corneal coordinates"),
            Ps3_C_8_30_3_1_4_df3f8b => OutsideScope("Binds Corneal Topography Map data (C.8.30.3); no VL WSI Module carries corneal coordinates"),
            Ps3_C_8_30_3_1_4_fc2365 => OutsideScope("Binds Corneal Topography Map data (C.8.30.3); no VL WSI Module carries corneal coordinates"),
            Ps5_6_2_a9e8be => OutsideScope("Binds the VRs that future editions of the Standard define; an object encoded under the pinned edition uses only its VRs"),
            Ps5_6_4_7e8758 => OutsideScope("A notation rule for the VM column of the Standard's own tables, not a property of encoded Values"),
            Ps5_7_6_481a49 => OutsideScope("States how future editions of the Standard will design Data Elements; binds the Standard, not encoded objects"),
            Ps5_7_7_2c17ff => OutsideScope("Binds later editions of the Standard not to redefine retired Tags; not a property of encoded objects"),
            Ps5_8_1_2_05616b => OutsideScope("Overlay Plane Module absent from VL WSI IOD (Table A.32.8-1); 60xx overlays not carried"),
            Ps5_8_1_2_3b62f6 => OutsideScope("Overlay Data bit packing; no Overlay Plane Module in VL WSI IOD"),
            Ps5_8_1_2_70a5ff => OutsideScope("Overlay Data frame concatenation (8.1.2); no Overlay Plane Module in VL WSI IOD"),
            Ps5_8_2_0fadc3 => OutsideScope("Float Pixel Data belongs to no VL WSI IOD module; only Pixel Data (7FE0,0010) carried"),
            Ps5_8_2_10_17fcd6 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_10_64b5e7 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_10_7e5161 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_10_af1171 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_10_c1679c => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_11_455596 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_11_7456b1 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_11_84bfbd => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_11_a9912e => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_11_ce01f7 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_12_b38f9a => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_18602a => OutsideScope("Float Pixel Data (Floating Point Image Pixel Module) not in VL WSI IOD"),
            Ps5_8_2_4bcda5 => OutsideScope("Double Float Pixel Data (Double Floating Point Image Pixel Module) not in VL WSI IOD"),
            Ps5_8_2_5_5c5c79 => OutsideScope("Constrains the MPEG2 video bit stream; wsi-dicom validates pathology slides, not video, and holds an MPEG2 object only to its declared Pixel Data attributes and encapsulation"),
            Ps5_8_2_5_6cde8d => OutsideScope("Constrains the MPEG2 video bit stream; wsi-dicom validates pathology slides, not video, and holds an MPEG2 object only to its declared Pixel Data attributes and encapsulation"),
            Ps5_8_2_5_c5b5a7 => OutsideScope("MPEG2 Main Profile always applies the irreversible multi-component transformation specified by 8.2.5 and 8.2.6; the condition that no multi-component transformation was applied cannot occur in these encodings"),
            Ps5_8_2_6_405812 => OutsideScope("Constrains the MPEG2 video bit stream; wsi-dicom validates pathology slides, not video, and holds an MPEG2 object only to its declared Pixel Data attributes and encapsulation"),
            Ps5_8_2_6_4d8fa8 => OutsideScope("Constrains the MPEG2 video bit stream; wsi-dicom validates pathology slides, not video, and holds an MPEG2 object only to its declared Pixel Data attributes and encapsulation"),
            Ps5_8_2_6_571188 => OutsideScope("Constrains the MPEG2 video bit stream; wsi-dicom validates pathology slides, not video, and holds an MPEG2 object only to its declared Pixel Data attributes and encapsulation"),
            Ps5_8_2_7_3618e9 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_7_7773fd => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_7_7886b4 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_7_ce01ab => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_7_d08204 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_8_34a4e3 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_8_68acb5 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_8_7fe9c4 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_8_9d4541 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_8_d6c85e => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_9_a52046 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_9_b1098e => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_8_2_9_c7bb6c => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Explicit VR Waveform Data is OB when Waveform Bits Allocated is 8
            Ps5_8_3_7a124d => Checked("intrinsic-wsi-dicom-encoding"),
            // Channel Minimum/Maximum and Waveform Padding Value share the Waveform Data VR
            Ps5_8_3_7c241f => Checked("intrinsic-wsi-dicom-encoding"),
            // Explicit VR Waveform Data is OW unless Waveform Bits Allocated is 8
            Ps5_8_3_8dc825 => Checked("intrinsic-wsi-dicom-encoding"),
            Ps5_A_10_2fa6d0 => OutsideScope("Constrains a SMPTE ST 2110 Transfer Syntax, which describes a DICOM-RTV Flow (PS3.22) sent outside any Part 10 file; a VL WSI file does not use it, and the Waveform audio attributes it constrains belong to no VL WSI module"),
            Ps5_A_10_3289c0 => OutsideScope("Constrains a SMPTE ST 2110 Transfer Syntax, which describes a DICOM-RTV Flow (PS3.22) sent outside any Part 10 file; a VL WSI file does not use it, and the Waveform audio attributes it constrains belong to no VL WSI module"),
            Ps5_A_10_3cc0b0 => OutsideScope("Constrains a SMPTE ST 2110 Transfer Syntax, which describes a DICOM-RTV Flow (PS3.22) sent outside any Part 10 file; a VL WSI file does not use it, and the Waveform audio attributes it constrains belong to no VL WSI module"),
            Ps5_A_10_adf45d => OutsideScope("Constrains a SMPTE ST 2110 Transfer Syntax, which describes a DICOM-RTV Flow (PS3.22) sent outside any Part 10 file; a VL WSI file does not use it, and the Waveform audio attributes it constrains belong to no VL WSI module"),
            Ps5_A_10_c6cb25 => OutsideScope("Constrains a SMPTE ST 2110 Transfer Syntax, which describes a DICOM-RTV Flow (PS3.22) sent outside any Part 10 file; a VL WSI file does not use it, and the Waveform audio attributes it constrains belong to no VL WSI module"),
            Ps5_A_1_326b50 => Undecidable("Implicit VR encodes no VR, so the OW a reader applies to Waveform Data cannot be contradicted by the bytes, and the byte order of its Values is not observable"),
            Ps5_A_1_8ce325 => Undecidable("Implicit VR encodes no VR, so the OW a reader applies to Blending Lookup Table Data cannot be contradicted by the bytes, and the byte order of its Values is not observable"),
            Ps5_A_1_d0c6a0 => Undecidable("Implicit VR encodes no VR, so the OL a reader applies to Track Point Index List cannot be contradicted by the bytes, and the byte order of its Values is not observable; reading it as unsigned binds readers"),
            Ps5_A_2_367471 => Undecidable("The byte order of Waveform Data samples is not observable: their bytes read as some valid Little Endian sample. The VR the Explicit VR field gives is checked by intrinsic-wsi-dicom-encoding (PS3.5 8.3)"),
            // Explicit VR Track Point Index List is OL, wherever a Standard Extended object carries it
            Ps5_A_2_5cbc84 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            // Explicit VR Blending Lookup Table Data is OW, wherever a Standard Extended object carries it
            Ps5_A_2_e15d71 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            Ps5_A_4_10_7a2eb4 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Explicit VR Blending Lookup Table Data is OW, wherever a Standard Extended object carries it
            Ps5_A_4_3dedae => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            Ps5_A_4_5_54be61 => Undecidable("Assigns the MPEG2 Transfer Syntax UIDs; an alternative UID identifies a different syntax, and the bytes cannot reveal an intended different UID assignment"),
            Ps5_A_4_6_eaaff4 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_A_4_7_313c85 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_A_4_7_7502f5 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_A_4_7_9b3674 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_A_4_7_d07e3a => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_A_4_8_c22295 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            Ps5_A_4_9_85a504 => OutsideScope("The AVC/H.264 and HEVC/H.265 formats require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11); VL WSI excludes YBR_PARTIAL_420 (PS3.3 C.8.12.4.1.5), so no conformant VL WSI uses these encodings"),
            // Explicit VR Track Point Index List is OL, wherever a Standard Extended object carries it
            Ps5_A_4_cd73d1 => Checked("intrinsic-wsi-dicom-iod-vr-vm"),
            Ps5_A_4_cfe2f9 => Undecidable("The byte order of Waveform Data samples is not observable: their bytes read as some valid Little Endian sample. The VR the Explicit VR field gives is checked by intrinsic-wsi-dicom-encoding (PS3.5 8.3)"),
            Ps5_A_8_68d505 => OutsideScope("Constrains a SMPTE ST 2110-20 Transfer Syntax, which describes a DICOM-RTV Video Flow (PS3.22) sent outside any Part 10 file; the pixel encoding it requires is the Flow's, not that of a Part 10 Pixel Data Value"),
            Ps5_A_8_ae75ef => OutsideScope("Constrains a SMPTE ST 2110-20 Transfer Syntax, which describes a DICOM-RTV Video Flow (PS3.22) sent outside any Part 10 file; the pixel encoding it requires is the Flow's, not that of a Part 10 Pixel Data Value"),
            Ps5_A_9_988e06 => OutsideScope("Constrains a SMPTE ST 2110-20 Transfer Syntax, which describes a DICOM-RTV Video Flow (PS3.22) sent outside any Part 10 file; the pixel encoding it requires is the Flow's, not that of a Part 10 Pixel Data Value"),
            Ps10_8_5_e938f0 => OutsideScope("Binds a File-set and its DICOMDIR, not this IOD's objects; the File-set check holds every File-set of a validated directory to it in the file-set scope"),
            // Color Space checked against the primaries of the ICC Profile beside it in every Item, the Icon Image included; the top level carries neither beside the Optical Path Sequence
            Ps3_C_7_11c_ColorSpace_763a29 => Checked("intrinsic-wsi-dicom-attribute-relations"),
            // ---- Bind applications or processes, not the encoded objects. ----
            Ps10_7_1_581c44 => Behavior("Binds readers of File Meta Information Version (0002,0001). The validator complies: intrinsic-wsi-dicom-file-meta tests only its two bytes and bit 0 of the second"),
            Ps10_7_1_ad3b69 => Behavior("Binds readers to ignore (0002,xxxx) Tags that Table 7.1-1 does not define, which may come from later editions; a file cannot break it. The validator complies: intrinsic-wsi-dicom-file-meta checks only the Table 7.1-1 elements, and of the others only that none is encoded as UN (PS3.5 6.2.2)"),
            Ps10_7_1_b959ab => Behavior("Binds File-set Readers and Updaters. The validator complies: it recognises a DICOM File by the DICM prefix at byte 128 and its File Meta Information, never by the Preamble's content"),
            Ps10_7_2_b5e9bc => Behavior("Binds File-set Readers and Updaters to process Data Set Trailing Padding; the validator accepts it in the top-level Data Set and in Items alike"),
            Ps10_7_2_bf7e26 => Behavior("Binds readers to ignore the Value of Data Set Trailing Padding. The validator complies: no rule reads that Value, only its PS3.6 VR"),
            Ps10_7_5_bc5a1c => Behavior("Requires a Conformance Statement to describe Preamble sanitization; Conformance Statements are not in the file"),
            Ps10_8_1_4746d4 => Behavior("Binds File-set Updaters across edits: comparing File-set UIDs needs the File-set before and after a change"),
            Ps10_8_2_a79088 => Behavior("Binds AEs reading or creating files not to depend on the semantics a creator gave its File-set structure"),
            Ps10_8_3_11afdd => Behavior("Binds the File-set Reader role; whether it modified a file needs that file's earlier state"),
            Ps10_8_3_ab0ab0 => Behavior("Binds the Media Operations the DICOM File Service supports for each role (Table 8.3-1), not file content"),
            Ps10_8_3_ce660f => Behavior("Binds which roles (File-set Reader, Creator, Updater) an implementation supports, stated in its Conformance Statement"),
            Ps10_8_3_fb41de => Behavior("Binds the File-set Updater role; whether it modified files other than the DICOMDIR needs their earlier state"),
            Ps10_8_4_129fce => Behavior("Binds the DICOM File Service to detect and report reads beyond the end of a File"),
            Ps10_8_4_1591a2 => Behavior("Lead-in to the capabilities the DICOM File Service offers; binds the service, not files"),
            Ps10_8_6_056f88 => Behavior("Binds AEs reading or updating a File-set not to change its File-set UID; needs the File-set's earlier state"),
            Ps3_8_3_9db03b => Behavior("Binds applications using coded entries; the validator complies: it matches coded entries by Coding Scheme Designator with Code Value, Long Code Value or URN Code Value, never by Code Meaning"),
            Ps3_C_7_4_1_1_2_cd0d16 => Behavior("Binds how applications use the Value; the validator complies: it checks the slide-related Value SLIDE_CORNER and derives no geometry from it"),
            Ps3_C_7_5_1_1_2_8a51b2 => Behavior("Binds modifying equipment that changes padding; whether the new padding values occur in the native image needs the unpadded image, which the encoded pixels do not separate"),
            Ps3_C_7_5_1_1_2_9b8ebe => Behavior("Binds modifying equipment that changes padding; the modified object does not record the earlier padding values, so it cannot show which changed"),
            Ps3_C_7_6_17_3_ad808a => Behavior("Binds recipients of TILED_SPARSE images; the validator complies: it places their Frames only by Plane Position (Slide) and Optical Path Identification, and the configuration fixtures encode them last to first"),
            Ps3_C_7_6_1_1_2_97936c => Behavior("Binds the content of the implementation Conformance Statement, which is not in the file"),
            Ps5_10_1_0cbe46 => Behavior("Binds every implementation to support Implicit VR Little Endian; support is not a property of a stored file"),
            Ps5_10_1_d41f69 => Behavior("Binds A-ASSOCIATE requests and their Presentation Contexts (PS3.8); a stored file records no association"),
            Ps5_10_20_b47931 => Behavior("Binds A-ASSOCIATE requests and their Presentation Contexts (PS3.8); a stored file records no association"),
            Ps5_10_2_3a0b36 => Behavior("Binds A-ASSOCIATE requests and their Presentation Contexts (PS3.8); a stored file records no association"),
            Ps5_10_2_956d41 => Behavior("Binds implementations that support a lossless JPEG process to support 1.2.840.10008.1.2.4.70; support is not a property of a stored file"),
            Ps5_10_3_db3079 => Behavior("Binds A-ASSOCIATE requests and their Presentation Contexts (PS3.8); a stored file records no association"),
            Ps5_10_4_d49808 => Behavior("Binds A-ASSOCIATE requests and their Presentation Contexts (PS3.8); a stored file records no association"),
            Ps5_10_5_0029bf => Behavior("Binds implementations that support JPEG-LS Near-Lossless to support JPEG-LS Lossless; support is not a property of a stored file"),
            Ps5_10_744125 => Behavior("Binds organizations defining private Transfer Syntaxes to register their UID roots (PS3.5 9.2); registration is not recorded in a file"),
            Ps5_6_2_2_f023ca => Behavior("Binds applications that convert byte order not to swap UN Values; a stored UN Value's bytes cannot show whether they were swapped"),
            Ps5_6_2_904f94 => Behavior("Binds readers to ignore the trailing spaces of a UR Value; the validator complies: the UR format check trims them first"),
            Ps5_6_3_3c8903 => Behavior("Binds comparisons with IS and DS Defined Terms to their numeric meaning; the validator complies: it compares those Values numerically"),
            Ps5_6_3_77e461 => Behavior("Binds receivers and validators not to reject Values outside the Defined Terms; the validator complies: of the Values the Standard lists, only Enumerated Values are enforced"),
            Ps5_7_2_bd3564 => Behavior("Binds implementations not to require Group Length elements. The validator complies: outside group 0002, where PS3.10 Table 7.1-1 makes (0002,0000) Type 1, a Group Length is only checked when present"),
            Ps5_7_3_4b4c77 => Behavior("Binds how Big Endian machines process Little Endian Data Sets; processing is not recorded in the file"),
            Ps5_7_3_5d552b => Behavior("Binds every AE to support the default Transfer Syntax; support is not a property of a stored file"),
            Ps5_7_4_5_70d404 => Behavior("Binds readers, this validator included, to read a zero-length Type 3 element as absent; a file cannot break it. The encoding and codestream checks treat a zero-length Value as absent"),
            Ps5_7_5_1_e3cceb => Behavior("Binds decoders to read Items of explicit and undefined length alike; the parser and the encoding check read both"),
            Ps5_7_5_2_7d3e46 => Behavior("Binds decoders to read Sequences of explicit and undefined length alike; the parser and the encoding check read both"),
            Ps5_7_5_3_b35e95 => Behavior("Binds decoders to scope an Item's Specific Character Set to that Item. The encoding check complies, but dicom-parser 0.9.1 keeps an Item's character set active after the Item ends, so the parsed text other checks read may be decoded with it"),
            Ps5_7_5_822bd2 => Behavior("Defines Items as an ordered set referenced by ordinal position; an encoded Sequence is ordered by construction, so no file can break it"),
            Ps5_7_5_96c73f => Behavior("Defines the implicit ordinal of each Item, counted from 1; ordinals are not encoded, so no file can break it"),
            Ps5_7_5_d718e7 => Behavior("Follows from the implicit ordinal numbering of Items; ordinals are not encoded, so no file can break it"),
            Ps5_7_8_1_4da0de => Behavior("Binds encoders' ability to assign private blocks dynamically; a file shows one assignment, not the capability"),
            Ps5_7_8_1_538c39 => Behavior("Binds decoders to accept reserved private blocks at any position; the validator complies: Private Data Elements are matched to the Private Creator of their block, wherever it is reserved"),
            Ps5_8_1_1_6dd7d7 => Behavior("Binds readers not to treat the pad byte of an odd-length Native Pixel Data Value as image data; the validator complies: only the padded length is checked, not the pad's Value"),
            Ps5_8_1_1_d109a2 => Behavior("States the design goal that Native Format accommodates any bit depth; binds no encoded property"),
            Ps5_8_2_10_fd07b0 => Behavior("Binds decoders to take the image characteristics from the HEVC/H.265 bit stream; VL WSI images shall not use this syntax (PS3.3 C.8.12.4.1.5), which intrinsic-wsi-dicom-image-metadata rejects"),
            Ps5_8_2_11_f8bc4f => Behavior("Binds decoders to take the image characteristics from the HEVC/H.265 bit stream; VL WSI images shall not use this syntax (PS3.3 C.8.12.4.1.5), which intrinsic-wsi-dicom-image-metadata rejects"),
            Ps5_8_2_14_ffedc6 => Behavior("Binds decoders to take the image characteristics from the HTJ2K bit stream; the agreement of those characteristics with the Image Pixel attributes is checked as their own statements by intrinsic-wsi-dicom-codestream"),
            Ps5_8_2_15_4c0d86 => Behavior("Binds decoders to take the image characteristics from the JPEG XL bit stream; the agreement of those characteristics with the Image Pixel attributes is checked as their own statements by intrinsic-wsi-dicom-codestream"),
            Ps5_8_2_1_2dc897 => Behavior("Binds decoders to take the image characteristics from the JPEG Interchange Format; the agreement of those characteristics with the Image Pixel attributes is checked as their own statements by intrinsic-wsi-dicom-codestream"),
            Ps5_8_2_1_43326a => Behavior("Binds implementations that support a 12-bit lossy JPEG syntax to support Process 4; support is not a property of a stored file"),
            Ps5_8_2_1_fdb6f4 => Behavior("Binds implementations that support an 8-bit lossy JPEG syntax to support Process 1; support is not a property of a stored file"),
            Ps5_8_2_1_fe71fd => Behavior("Binds implementations that support a lossless JPEG syntax to support Process 14 Selection Value 1; support is not a property of a stored file"),
            Ps5_8_2_3_3b4454 => Behavior("Binds decoders to take the image characteristics from the JPEG-LS Interchange Format; the agreement of those characteristics with the Image Pixel attributes is checked as their own statements by intrinsic-wsi-dicom-codestream"),
            Ps5_8_2_4_c79fec => Behavior("Binds decoders to take the image characteristics from the JPEG 2000 bit stream; the agreement of those characteristics with the Image Pixel attributes is checked as their own statements by intrinsic-wsi-dicom-codestream"),
            Ps5_8_2_5_fc541c => Behavior("Binds decoders to take the image characteristics from the MPEG2 bit stream; the encoded object cannot show which characteristics an application used"),
            Ps5_8_2_6_9618ea => Behavior("Binds decoders to take the image characteristics from the MPEG2 bit stream; the encoded object cannot show which characteristics an application used"),
            Ps5_8_2_7_c4778c => Behavior("Binds decoders to take the image characteristics from the MPEG-4 AVC/H.264 bit stream; VL WSI images shall not use this syntax (PS3.3 C.8.12.4.1.5), which intrinsic-wsi-dicom-image-metadata rejects"),
            Ps5_8_2_8_f5ebd6 => Behavior("Binds decoders to take the image characteristics from the MPEG-4 AVC/H.264 bit stream; VL WSI images shall not use this syntax (PS3.3 C.8.12.4.1.5), which intrinsic-wsi-dicom-image-metadata rejects"),
            Ps5_8_4_1_0eb868 => Behavior("Binds the JPIP server's Content-type and the client's support for it; neither is in the file, which holds only the Pixel Data Provider URL"),
            Ps5_8_4_1_2e3bb2 => Behavior("Binds JPIP client requests; the file holds only the Pixel Data Provider URL"),
            Ps5_8_4_1_5fbd87 => Behavior("Binds how the JPIP server numbers the code streams of the URL target; the file holds only the URL, not the target's streams"),
            Ps5_8_5_4a5789 => Behavior("Informative security advice to implementers of encapsulated formats; binds no encoded property"),
            Ps5_9_0c8924 => Behavior("Binds organizations' UID policies; uniqueness across every UID an organization issues cannot be seen in any set of files, though a SOP Instance UID repeated in the validated set (intrinsic-wsi-dicom-identity-set) or one UID given two roles (intrinsic-wsi-dicom-series-set) is reported"),
            Ps5_9_2_2_4bc3d5 => Behavior("Binds the organization defining a private UID to ensure its uniqueness; responsibility is not recorded in a file"),
            Ps5_9_86308f => Behavior("Binds applications not to parse UIDs for meaning; a UID's bytes cannot show whether anyone parses them"),
            Ps5_A_5_431c1c => Behavior("Binds implementations that support Deflated Explicit VR Little Endian to support Explicit VR Little Endian too; support is not a property of a stored file"),
            // ---- Bind the objects on facts their bytes cannot show. ----
            Ps10_7_1_849343 => Undecidable("A non-zero Preamble conforms whenever a Media Storage Application Profile or the implementation uses it (e.g. a TIFF header for dual-format files); the file does not record whether one does, so its bytes cannot show a violation"),
            Ps10_8_2_c25c0a => Undecidable("Whether a File ID's structure conveys semantics depends on its creator's intent, which no name or byte records"),
            Ps3_10_14_cb9917 => Undecidable("A Local Namespace Entity ID means something only within its namespace and an entity may hold several Universal Entity IDs, so no pairing, in one file or across files, shows the two name different entities"),
            Ps3_10_18_UniversalEntityID_ccba0a => Undecidable("Issuer of Patient ID is free text and an authority may hold several Universal Entity IDs, so neither one file nor several show that the two name different authorities"),
            Ps3_10_3b_DICOMMediaRetrievalSequence_bd171b => Undecidable("What the identified media File-set holds is outside the validated files"),
            Ps3_10_3b_DICOMRetrievalSequence_3b7839 => Undecidable("What the identified Application Entity holds is outside the validated files"),
            Ps3_10_3b_XDSRetrievalSequence_2b69ce => Undecidable("What the identified XDS repository holds is outside the validated files"),
            Ps3_8_3_73025f => Undecidable("PS3.16 permits translations and synonyms of a Code Meaning and the chosen language is not encoded, so no Code Meaning text can be shown not to be an available value"),
            Ps3_C_12_1_1_4_1_76ea58 => Undecidable("The embedded Encrypted Attributes Data Sets are enveloped for recipients whose private keys are not in the file, so their Values cannot be compared"),
            Ps3_C_12_1_1_9_1_5cf653 => Undecidable("Whether a recorded Sequence is the entire prior Value is known only against the prior version, which keeps the SOP Instance UID and need not be recorded, so no file in the set can be shown to be that version"),
            Ps3_C_12_1_1_9_1_9f0b3d => Undecidable("Which Attributes a coercion changed is known only against the prior version, which keeps the SOP Instance UID and need not be recorded, so no file in the set can be shown to be that version"),
            Ps3_C_12_1_1_9_211e3d => Undecidable("Coercion keeps the SOP Instance UID and adding an Item is optional, so two files of one Instance may be parallel coercions; no pair of files shows that one succeeds the other"),
            Ps3_C_12_1_ConversionSourceAttributesSequence_b7a045 => Undecidable("Whether a conversion used one Frame of the source is not recorded; a source in the set does not show which Frames were converted"),
            Ps3_C_2_2_1_1_efbdaa => Undecidable("Whether the referenced server is WADO-compliant and what its Retrieve URI returns are outside the file"),
            Ps3_C_7_1_1_1_1_a30ea8 => Undecidable("The breed Attributes are Type 2C for a non-human organism and their presence is checked by the IOD tables; whether the breed is known, which would make them non-empty, is not encoded"),
            Ps3_C_7_1_1_1_1_b0068e => Undecidable("Each of the three mixed-breed encodings is valid, and whether a breed is mixed is not encoded"),
            // A group image whose Patient Module Value differs from that of a member imaged alone in the set
            Ps3_C_7_1_4_1_1_64e8d5 => Checked("intrinsic-wsi-dicom-series-set"),
            Ps3_C_7_1_4_1_1_e288bb => Undecidable("Which acquisition Attributes are specific to one organism depends on the acquisition (e.g. a dose given to one subject); nothing in the bytes marks an Attribute as subject-specific"),
            // VL WSI Modality LUT is the identity (Rescale Slope 1, Intercept 0), so a padding Value is a stored value exactly when it lies in the stored range
            Ps3_C_7_5_1_1_2_6790e6 => Checked("intrinsic-wsi-dicom-pixel-attributes"),
            Ps3_C_7_5_1_1_2_ec1c1c => Undecidable("Whether the padding value also occurs in the native image needs the unpadded image, which the encoded pixels do not separate from the padding"),
            Ps3_C_7_6_14_2b9b4f => Undecidable("No mapping says which Acquisition Context Items restate which Module Attributes, so a replacing description cannot be identified"),
            Ps3_C_7_6_16_2_5_1_1c4f41 => Undecidable("Whether another object was used to plan this acquisition is not encoded; a missing Referenced Image Item leaves no trace"),
            Ps3_C_7_6_16_2_5_1_aac449 => Undecidable("An Item references a planning image exactly when its purpose says Localizer; with no other record of planning, a missing Localizer code cannot be told from a reference made for another purpose"),
            Ps3_C_7_6_1_1_1_200b65 => Undecidable("Which direction is principal depends on the true orientation of the imaged anatomy, which the encoding does not record"),
            // Two files of one SOP Instance with 00 and 01 show a reset; a 00 image with lossy history or lossy coding also fails the IOD presence and lossy-history checks
            Ps3_C_7_6_1_1_5_491487 => Checked("intrinsic-wsi-dicom-series-set"),
            Ps3_C_7_6_1_1_5_4caf86 => Undecidable("Whether the sensor itself delivered lossy data is not recorded; Image Type Value 1 of a lossy derivation is checked by intrinsic-wsi-dicom-wsi-image"),
            // A Frame whose Specimen Reference names a Specimen UID the Sequence lacks images an unidentified specimen; specimens no Frame references are not visible in the bytes
            Ps3_C_7_6_22_2_SpecimenDescriptionSequence_184620 => Checked("intrinsic-wsi-dicom-wsi-image"),
            Ps3_C_7_6_3_1_2_2da8f9 => Undecidable("Chroma siting is signalled neither in Native Pixel Data nor in JPEG headers, and VL WSI uses YBR_FULL_422 only in JPEG"),
            Ps3_C_7_6_3_1_2_8fee6e => Undecidable("Chroma siting is signalled neither in Native Pixel Data nor in JPEG headers, and VL WSI uses YBR_FULL_422 only in JPEG"),
            Ps3_C_7_6_3_1_2_fc3f71 => Undecidable("Chroma siting is signalled neither in Native Pixel Data nor in JPEG headers, and VL WSI uses YBR_FULL_422 only in JPEG"),
            Ps3_C_7_6_3_1_3_f6f69c => Undecidable("Interleaved samples in R G B order and in any other order are equally valid byte streams; which sample is red is recorded only by Photometric Interpretation"),
            Ps3_C_7_6_3_1_6_14a40e => Undecidable("A palette whose entries span a small range is a dark palette or an unscaled one, and the file does not say which"),
            Ps3_C_7_6_6_1_1_5e31a4 => Undecidable("Whether an Image IE Attribute describes the first Frame rather than another is its meaning, which the encoding does not record"),
            Ps3_C_7_6_6_1_3_c71343 => Undecidable("Whether Frames are left and right stereoscopic pairs is image content; VL WSI Frames are tiles"),
            // Two files of one SOP Instance with 00 and 01 show a reset; a 00 image with lossy history or lossy coding also fails the IOD presence and lossy-history checks
            Ps3_C_7_9_LossyImageCompression_e641df => Checked("intrinsic-wsi-dicom-series-set"),
            Ps3_C_8_12_2_1_1_9040d9 => Undecidable("The physical label edge of the slide in Reference Slide Orientation is not recorded, so the direction of increasing Y against it cannot be checked"),
            Ps3_C_8_12_2_1_1_bdac29 => Undecidable("The substrate surface is not recorded and a negative Z offset is not invalid; TILED_FULL plane order along increasing Z is checked by intrinsic-wsi-dicom-tile-geometry"),
            Ps3_C_8_12_2_1_1_df0fc6 => Undecidable("The physical edges of the slide in Reference Slide Orientation are not recorded, so the direction of increasing X against them cannot be checked"),
            Ps3_C_8_12_4_1_1_8b67da => Undecidable("Whether OVERVIEW pixels show the label, all specimen and the empty space is image content; Specimen Label in Image YES is checked by intrinsic-wsi-dicom-wsi-image"),
            Ps3_C_8_12_4_1_1_cd67be => Undecidable("Whether THUMBNAIL pixels show the label is image content; Specimen Label in Image NO is checked by intrinsic-wsi-dicom-wsi-image"),
            Ps3_C_8_12_4_1_1_db2a55 => Undecidable("Whether VOLUME pixels show the label is image content; Specimen Label in Image NO is checked by intrinsic-wsi-dicom-wsi-image"),
            Ps3_C_8_12_4_1_1_fa1cda => Undecidable("Whether LABEL pixels show only the label is image content; Specimen Label in Image YES is checked by intrinsic-wsi-dicom-wsi-image"),
            // Two files of one SOP Instance with 00 and 01 show a reset; a 00 image with lossy history or lossy coding also fails the IOD presence and lossy-history checks
            Ps3_C_8_12_4_1_LossyImageCompression_debf6c => Checked("intrinsic-wsi-dicom-series-set"),
            Ps3_C_8_12_5_1_ChannelDescriptionCodeSequence_49e204 => Undecidable("Which light color each encoded channel holds is recorded only by the codes themselves, so their order cannot be checked; the Item count is checked by the IOD tables"),
            Ps3_C_8_13_2_1_1_5edea3 => Undecidable("Whether there is doubt about the approval of the hardware and software is not encoded"),
            Ps3_C_8_13_2_1_1_80e1f3 => Undecidable("Whether the hardware and software are approved is not encoded"),
            Ps5_6_2_1_2_db1198 => Undecidable("Whether a component group is the ideographic (second) or phonetic (third) representation is linguistic: Korean names, for instance, put Hangul in the ideographic group (PS3.5 Annex I), so no character class decides it"),
            Ps5_6_2_1_2_df98c6 => Undecidable("Another character meant as the component group delimiter is indistinguishable from name text; only '=' splits groups, and the group count and lengths it implies are checked as their own statements"),
            Ps5_6_2_1_2_ed37aa => Undecidable("Whether a component group is the ideographic (second) or phonetic (third) representation is linguistic: Korean names, for instance, put Hangul in the ideographic group (PS3.5 Annex I), so no character class decides it"),
            Ps5_6_2_921c95 => Undecidable("Another character meant as the component delimiter is indistinguishable from name text; only '^' splits components, and the component count it implies is checked as its own statement"),
            Ps5_6_2_b45dec => Undecidable("Another character meant as the component group delimiter is indistinguishable from name text; only '=' splits groups, and the group count and lengths it implies are checked as their own statements"),
            Ps5_6_3_310276 => Undecidable("Conformance Statement content, and what an implementer's new Defined Term means, are not in the file"),
            Ps5_6_3_4b377b => Undecidable("What an implementer-defined Value means, e.g. unknown, is not encoded"),
            Ps5_7_8_0ac332 => Undecidable("Whether Private Data Elements change the semantics of the IOD or SOP Class depends on their meaning, which only their creator's documentation records"),
            Ps5_8_1_bfa7ac => Undecidable("OB Values are byte streams whose bytes are the same in any byte order; only the retired Explicit VR Big Endian could differ, and a swapped OB Value is indistinguishable without its intended content"),
            Ps5_8_2_23a2a1 => Undecidable("Where a Pixel Cell spans a word or byte boundary its bits continue least significant first; any bit order yields valid samples, so the order is unverifiable without the intended sample values"),
            Ps5_A_11_e5440b => Undecidable("A numeric Value's byte order is not observable: its bytes read as some valid Little Endian Value. Only Values with known ranges reveal swapping, and those are checked as their own statements (LUT Descriptors, Segmented Palette data)"),
            Ps5_A_1_77e469 => Undecidable("A numeric Value's byte order is not observable: its bytes read as some valid Little Endian Value. Only Values with known ranges reveal swapping, and those are checked as their own statements (LUT Descriptors, Segmented Palette data)"),
            Ps5_A_2_8b71de => Undecidable("A numeric Value's byte order is not observable: its bytes read as some valid Little Endian Value. Only Values with known ranges reveal swapping, and those are checked as their own statements (LUT Descriptors, Segmented Palette data)"),
            Ps5_A_4_7b5903 => Undecidable("A numeric Value's byte order is not observable: its bytes read as some valid Little Endian Value. Only Values with known ranges reveal swapping, and those are checked as their own statements (LUT Descriptors, Segmented Palette data)"),
            Ps5_A_6_351dc0 => Undecidable("A numeric Value's byte order is not observable: its bytes read as some valid Little Endian Value. Only Values with known ranges reveal swapping, and those are checked as their own statements (LUT Descriptors, Segmented Palette data)"),
            // Each Deflated Image Frame inflated: complete stream, Native length, single NULL pad only after odd length
            Ps5_A_4_13_a456ca => Checked("intrinsic-wsi-dicom-codestream"),
            Ps3_C_12_1_1_6_442d5f => Undecidable("The HL7 II identifier is in the referenced HL7 document, outside the validated set; the file holds only the UID, so their mapping cannot be checked"),
            // ---- Statements of the IODs beside VL WSI. ----
            // Window Width of at least 1 when VOI LUT Function is absent or LINEAR, in each Frame VOI LUT Item
            Ps3_C_11_2_1_2_1_2b93d6 => Checked("intrinsic-wsi-dicom-parametric-map"),
            // Window Center, Width and VOI LUT Function only with MONOCHROME1 or MONOCHROME2
            Ps3_C_11_2_1_2_2_1336e7 => Checked("intrinsic-wsi-dicom-parametric-map"),
            // Window Center and Width have the same number of Values
            Ps3_C_11_2_1_2_2_4dae85 => Checked("intrinsic-wsi-dicom-parametric-map"),
            // Color Space against the ICC Profile primaries in each IOD's family check; table-based profiles reported unexamined
            Ps3_C_11_15_1_ColorSpace_47a7bc => Checked("intrinsic-wsi-dicom-segmentation"),
            Ps3_C_7_4_2_1_1_ca7e38 => Undecidable("Whether Instances are temporally related is not encoded apart from this UID"),
            // Set check: the Series of a synchronized Parametric Map or SR document shares its Frame of Reference
            Ps3_C_7_4_2_1_1_110d46 => Checked("intrinsic-wsi-dicom-synchronization"),
            // NTP Source Address parsed as dotted decimal IPv4 or colon hexadecimal IPv6
            Ps3_C_7_7_NTPSourceAddress_1dc1cf => Checked("intrinsic-wsi-dicom-synchronization"),
            // NTP Source Address parsed as dotted decimal IPv4 or colon hexadecimal IPv6
            Ps3_C_7_7_NTPSourceAddress_426275 => Checked("intrinsic-wsi-dicom-synchronization"),
            // Frame Laterality against the CID 244 modifiers in each Frame Anatomy Item
            Ps3_C_7_6_16_9_FrameLaterality_3ce32c => Checked("intrinsic-wsi-dicom-parametric-map"),
            // FRAME: one Referenced Image Item with one Referenced Frame Number
            Ps3_C_37_1_2_1_1_bcbcea => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // VOLUME: one Referenced Image Item without Referenced Frame Number
            Ps3_C_37_1_2_1_1_2d725e => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Set check: a referenced image in the set has no Concatenation UID
            Ps3_C_37_1_2_1_1_9a4f58 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            Ps3_C_37_1_2_1_1_0aeb21 => Undecidable("Which shape an Annotation Group's coordinates describe is encoded only by Graphic Type itself; the counts, indices and geometry that each Graphic Type implies are checked by intrinsic-wsi-dicom-bulk-annotations"),
            // Whole coordinate tuples, and a Z common to every point factored out into Common Z Coordinate Value
            Ps3_C_37_1_2_1_1_235c62 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // No Long Primitive Point Index List with POINT
            Ps3_C_37_1_2_1_1_5c3618 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // POINT: Number of Annotations equals the coordinate tuples
            Ps3_C_37_1_2_1_1_bde27e => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            Ps3_C_37_1_2_1_1_deb20c => Undecidable("Which shape an Annotation Group's coordinates describe is encoded only by Graphic Type itself; the counts, indices and geometry that each Graphic Type implies are checked by intrinsic-wsi-dicom-bulk-annotations"),
            // Whole coordinate tuples, and a Z common to every point factored out into Common Z Coordinate Value
            Ps3_C_37_1_2_1_1_57f979 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // POLYLINE: Long Primitive Point Index List has Number of Annotations Values at tuple starts
            Ps3_C_37_1_2_1_1_263121 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            Ps3_C_37_1_2_1_1_709885 => Undecidable("Which shape an Annotation Group's coordinates describe is encoded only by Graphic Type itself; the counts, indices and geometry that each Graphic Type implies are checked by intrinsic-wsi-dicom-bulk-annotations"),
            // Whole coordinate tuples, and a Z common to every point factored out into Common Z Coordinate Value
            Ps3_C_37_1_2_1_1_dc0ee5 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // POLYGON first and last points differ
            Ps3_C_37_1_2_1_1_e2166b => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // POLYGON: Long Primitive Point Index List has Number of Annotations Values at tuple starts
            Ps3_C_37_1_2_1_1_506ec9 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            Ps3_C_37_1_2_1_1_7ddd77 => Undecidable("Which shape an Annotation Group's coordinates describe is encoded only by Graphic Type itself; the counts, indices and geometry that each Graphic Type implies are checked by intrinsic-wsi-dicom-bulk-annotations"),
            // Whole coordinate tuples, and a Z common to every point factored out into Common Z Coordinate Value
            Ps3_C_37_1_2_1_1_7d942e => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // No Long Primitive Point Index List with ELLIPSE or RECTANGLE
            Ps3_C_37_1_2_1_1_2c0f5d => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // ELLIPSE: four coordinate tuples per Annotation
            Ps3_C_37_1_2_1_1_430f9f => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            Ps3_C_37_1_2_1_1_3fac41 => Undecidable("Which shape an Annotation Group's coordinates describe is encoded only by Graphic Type itself; the counts, indices and geometry that each Graphic Type implies are checked by intrinsic-wsi-dicom-bulk-annotations"),
            // Whole coordinate tuples, and a Z common to every point factored out into Common Z Coordinate Value
            Ps3_C_37_1_2_1_1_db2f1e => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // RECTANGLE: four coordinate tuples per Annotation
            Ps3_C_37_1_2_1_1_7874da => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Long Primitive Point Index List starts at 1
            Ps3_C_37_1_2_1_1_dde335 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Long Primitive Point Index List strictly increasing
            Ps3_C_37_1_2_1_1_810c10 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Clockwise from the top: 3D outlines in the object; 2D ones in the set check by the referenced image's Image Orientation (Slide)
            Ps3_C_37_1_2_1_1_aa5274 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // No two polyline or polygon segments cross or touch beyond a shared vertex (sweep over projected segments, bounded)
            Ps3_C_37_1_2_1_1_e4f629 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Annotation Group Numbers 1, 2, ... in Item order
            Ps3__C_37_1_2_AnnotationGroupNumber_5fe4bc => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // The statement is the Enumerated Values cell of Table C.37.1-2. The
            // generated IOD check rejects every Value outside that closed list;
            // the family check separately enforces the geometry each admitted
            // Graphic Type implies.
            Ps3__C_37_1_2_GraphicType_707986 => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // Point Coordinates Data holds whole (X, Y) or, without Common Z, (X, Y, Z) tuples
            Ps3__C_37_1_2_PointCoordinatesData_8218cf => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Double Point Coordinates Data holds whole (X, Y) or, without Common Z, (X, Y, Z) tuples
            Ps3__C_37_1_2_DoublePointCoordinatesData_0cb021 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Floating Point Values count equals the Annotation Index List
            Ps3__C_37_1_2_FloatingPointValues_8cc06d => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Floating Point Values count equals the Long Primitive Point Index List
            Ps3__C_37_1_2_FloatingPointValues_76b057 => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Floating Point Values count equals Number of Annotations
            Ps3__C_37_1_2_FloatingPointValues_53fe2c => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Annotation Index List Values index Annotations of the group
            Ps3__C_37_1_2_AnnotationIndexList_67905a => Checked("intrinsic-wsi-dicom-bulk-annotations"),
            // Set check: the Frame of Reference of each referenced image in the set
            Ps3_A_51_1_68d35f => Checked("intrinsic-wsi-dicom-segmentation"),
            // Set check: Rows, Columns and Pixel Spacing of a referenced image without Frame of Reference
            Ps3_A_51_1_34d27f => Checked("intrinsic-wsi-dicom-segmentation"),
            // Shared Functional Group placement of Tables A.90.1.5-1, A.90.2.5-1, A.51-2 and A.75-2
            Ps3_A_51_5_ac52d6 => Tables,
            // Each Frame has Pixel Measures and a patient plane or slide position (TILED_FULL implied)
            Ps3_A_51_5_1_1767c6 => Checked("intrinsic-wsi-dicom-segmentation"),
            // Each Frame has a Derivation Image Source Image; the set check matches its Rows and Columns
            Ps3_A_51_5_1_6b8f76 => Checked("intrinsic-wsi-dicom-segmentation"),
            // Segmentation: Palette Color Lookup Table Data and no Segmented Data; another Image IOD exactly one
            Ps3_C_7_9_0dab54 => Checked("intrinsic-wsi-dicom-segmentation"),
            Ps3_C_7_9_1df529 => OutsideScope("Binds Presentation State IODs"),
            Ps3_C_7_9_a0e419 => OutsideScope("Binds the Color Palette IOD (A.58)"),
            // Native BINARY Pixel Data length is the bits of every Frame packed 8 to a byte, padded once
            Ps3_C_8_20_2_1_672661 => Checked("intrinsic-wsi-dicom-segmentation"),
            // A Segmentation not 01 whose source image in the set is 01 fails
            Ps3_C_8_20_2_2_042c5f => Checked("intrinsic-wsi-dicom-series-set"),
            // Lossy syntax or lossy codestream (JPEG, irreversible J2K, near-lossless JPEG-LS, motion) with 00 or absent fails
            Ps3_C_8_20_2_2_1e508c => Checked("intrinsic-wsi-dicom-lossy-history"),
            // Segmentation Fractional Type Type 1C on FRACTIONAL (Table C.8.20-2)
            Ps3_C_8_20_2_3_2_f2ea4d => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // Segment Numbers unique
            Ps3_C_8_20_2_4_75482d => Checked("intrinsic-wsi-dicom-segmentation"),
            // BINARY and FRACTIONAL Segment Numbers are 1 to the number of Segments
            Ps3_C_8_20_2_4_517b9d => Checked("intrinsic-wsi-dicom-segmentation"),
            // Image Type has two Values
            Ps3_C_8_20_2_ImageType_e16bb7 => Checked("intrinsic-wsi-dicom-segmentation"),
            // Two files of one SOP Instance differing in Lossy Image Compression fail
            Ps3_C_8_20_2_LossyImageCompression_9c833d => Checked("intrinsic-wsi-dicom-series-set"),
            // FRACTIONAL values scanned in Native, uncompressed, Deflated, RLE and JPEG 2000 Frames; other codings reported unexamined
            Ps3_C_8_20_2_MaximumFractionalValue_8e2125 => Checked("intrinsic-wsi-dicom-segmentation"),
            // Segments Overlap NO for LABELMAP
            Ps3_C_8_20_2_SegmentsOverlap_c35c89 => Checked("intrinsic-wsi-dicom-segmentation"),
            // Segment Numbers unique
            Ps3_C_8_20_4_SegmentNumber_9ad373 => Checked("intrinsic-wsi-dicom-segmentation"),
            Ps3_C_8_20_4_TrackingID_69472d => Undecidable("The domain in which a Tracking ID is used is not encoded"),
            // Referenced Segment Number has one Value, a Segment Number
            Ps3_C_8_20_3_ReferencedSegmentNumber_36aaef => Checked("intrinsic-wsi-dicom-segmentation"),
            Ps3_A_75_4_24a8b8 => Undecidable("Its Attributes are those of the Palette Color Lookup Table Module the IOD uses; which Module encodes a palette is not encoded"),
            // Shared Functional Group placement of Tables A.90.1.5-1, A.90.2.5-1, A.51-2 and A.75-2
            Ps3_A_75_5_eac12a => Tables,
            // LUT Descriptor first Value 0 for 2^16 LUT Data entries
            Ps3_C_11_2_1_1_fd1510 => Checked("intrinsic-wsi-dicom-parametric-map"),
            // Explicit VR LUT Descriptor US or SS by the signedness of the stored values
            Ps3_C_11_2_1_1_ed95f2 => Checked("intrinsic-wsi-dicom-parametric-map"),
            // LUT Descriptor third Value 8 or 16
            Ps3_C_11_2_1_1_9b32b3 => Checked("intrinsic-wsi-dicom-parametric-map"),
            Ps3_C_11_2_1_1_15aed3 => OutsideScope("Binds Presentation State IODs"),
            // LUT Data length as 8 or 16 bits allocated per entry
            Ps3_C_11_2_1_1_88b85b => Checked("intrinsic-wsi-dicom-parametric-map"),
            Ps3_C_11_2_1_3_ff7619 => Undecidable("Whether a VOI LUT Function descriptor is unique to its function is not encoded"),
            Ps3_C_11_2_1_3_f54033 => Behavior("Binds how applications interpret Window Center and Width"),
            Ps3_C_11_2_1_3_227a10 => Undecidable("Which function a VOI LUT Function descriptor defines is not encoded"),
            // Window Width greater than 0 with SIGMOID
            Ps3_C_11_2_1_3_1_18115e => Checked("intrinsic-wsi-dicom-parametric-map"),
            // Window Width greater than 0 with LINEAR_EXACT
            Ps3_C_11_2_1_3_2_05afe8 => Checked("intrinsic-wsi-dicom-parametric-map"),
            Ps3_C_7_6_16_2_26_1_8ab8b0 => Behavior("Binds how applications map stored values to RGB"),
            Ps3_C_7_6_16_2_26_1_360e8f => Behavior("Binds how applications map stored values to RGB"),
            Ps3_C_7_6_16_2_26_1_087b40 => Behavior("Binds how applications map stored values to RGB"),
            Ps3_C_7_6_16_2_26_1_9294d4 => Behavior("Binds how applications map stored values to RGB"),
            Ps3_C_7_6_16_2_7_1_274131 => Undecidable("How a trigger delay was calculated is not encoded"),
            // A Parametric Map not 01 whose source image in the set is 01 fails
            Ps3_C_8_32_2_1_7fa3d3 => Checked("intrinsic-wsi-dicom-series-set"),
            // Lossy syntax or lossy codestream (JPEG, irreversible J2K, near-lossless JPEG-LS, motion) with 00 or absent fails
            Ps3_C_8_32_2_1_d04416 => Checked("intrinsic-wsi-dicom-lossy-history"),
            Ps3_C_7_6_24_1_BitsAllocated_500a9f => OutsideScope("Native Float and Double Float Pixel Data have one Bits Allocated for every sample; no encoding can make them differ"),
            Ps3_C_7_6_25_1_BitsAllocated_2081d1 => OutsideScope("Native Float and Double Float Pixel Data have one Bits Allocated for every sample; no encoding can make them differ"),
            // Image Type Value 3 present and not zero length; Defined Terms may be extended
            Ps3_C_8_32_2_ImageType_06ce58 => Checked("intrinsic-wsi-dicom-parametric-map"),
            // Image Type Value 4 present and not zero length; Defined Terms may be extended
            Ps3_C_8_32_2_ImageType_c7ce98 => Checked("intrinsic-wsi-dicom-parametric-map"),
            // Two files of one SOP Instance differing in Lossy Image Compression fail
            Ps3_C_8_32_2_LossyImageCompression_a5f421 => Checked("intrinsic-wsi-dicom-series-set"),
            // Color Space against the ICC Profile primaries; table-based profiles reported unexamined
            Ps3_C_8_32_2_ColorSpace_0cd0fd => Checked("intrinsic-wsi-dicom-parametric-map"),
            Ps3_C_7_6_16_8_HeartRate_f41446 => Undecidable("Which beats a Heart Rate counted is not encoded"),
            // ---- PS3.15 Annex E Attribute Confidentiality Profiles (every pathology-slide IOD). ----
            // X actions leave the Attribute absent and Z and D ones replace it; a replaced Value's origin is not encoded
            Ps15_E_1_1_3479d4 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_81c445 => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            Ps15_E_1_1_9e2599 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_d220db => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_097048 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_7dcfd6 => Undecidable("Whether a UID was replaced or encrypted needs the object before de-identification; the de-identified object holds only the UID it carries"),
            Ps15_E_1_1_a9ad8b => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_9278e7 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_08300d => Undecidable("Whether the File Meta Information describes the de-identifying application rather than the original one is not encoded"),
            Ps15_E_1_1_baaf0e => OutsideScope("Binds Data Sets communicated by DICOM Real-Time Video (PS3.22), not stored objects"),
            Ps15_E_1_1_3a437a => Checked("intrinsic-wsi-dicom-deidentification"),
            // A DICOMDIR record over de-identified Files holding a Value the Profile removes, or a Record Key other than theirs (set check)
            Ps15_E_1_1_610bd1 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_0dc920 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_1_130cad => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_2_52230e => Behavior("Binds how a re-identifier decrypts the Encrypted Attributes Sequence"),
            Ps15_E_1_2_59c737 => Behavior("Binds the decryption capabilities of a re-identifier"),
            Ps15_E_1_2_b4daa0 => Behavior("Binds how a re-identifier restores the Modified Attributes Sequence"),
            Ps15_E_1_2_3efb4e => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_1_3_25711f => Undecidable("Conformance Statement content is not in the file"),
            Ps15_E_2_8ecf99 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_1_40d8f8 => Undecidable("Whether Pixel Data shows burned-in identifying text is image content; Burned In Annotation (0028,0301) NO is its own statement"),
            Ps15_E_3_1_3a4bfe => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_2_8c7718 => Undecidable("Whether pixels let an individual be recognized is image content; Recognizable Visual Features (0028,0302) NO is its own statement"),
            Ps15_E_3_2_4931da => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_3_1cc02d => Undecidable("The values the Profile removed are no longer in the object, so what remains cannot be compared with them"),
            // Table E.3.4-1 names the Content Item Concept Names and the action on each
            Ps15_E_3_4_4c3e71 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_5_058346 => Undecidable("The values the Profile removed are no longer in the object, so what remains cannot be compared with them"),
            Ps15_E_3_5_fb5a06 => Undecidable("Conformance Statement content is not in the file"),
            Ps15_E_3_6_63c876 => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            Ps15_E_3_6_cce630 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_6_d4c37e => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            Ps15_E_3_6_c42405 => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            Ps15_E_3_6_6c1923 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_6_4c36ce => Undecidable("Conformance Statement content is not in the file"),
            Ps15_E_3_7_db14da => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            Ps15_E_3_7_e380ce => Undecidable("Conformance Statement content is not in the file"),
            Ps15_E_3_8_557d78 => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            Ps15_E_3_9_8d9d37 => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            // Private Data Element Characteristics Sequence (0008,0300) records which Private blocks are safe
            Ps15_E_3_10_d4f53a => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_10_c41c02 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_10_813a17 => Checked("intrinsic-wsi-dicom-deidentification"),
            Ps15_E_3_10_4b1adf => Undecidable("Conformance Statement content is not in the file"),
            Ps15_E_3_11_ad484b => Undecidable("Whether a Value was kept unchanged, or was modified, needs the object before de-identification, which the de-identified object does not hold"),
            // ---- PS3.3 Annex F Basic Directory IOD and the Macros its Directory Record keys include (file-set). ----
            Ps3_C_18_6_0d9ea4 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_6_c2d2c9 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_7_c08e13 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_8_1_2_96463f => Undecidable("Which Template, if any, a document or sub-tree was created from is encoded only by the Content Template Sequence this sentence requires; a KOS is constructed from TID 2010 (A.35.4.3.1.3)"),
            Ps3_C_18_9_1_2_e8d12e => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_38_2_1_1_1_bc1d91 => OutsideScope("Binds INVENTORY Directory Records and their Scope of Inventory keys, which reference and describe Inventory objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_C_38_2_1_1_2_9ca5bd => OutsideScope("Binds INVENTORY Directory Records and their Scope of Inventory keys, which reference and describe Inventory objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_C_38_2_1_1_3_e13522 => OutsideScope("Binds INVENTORY Directory Records and their Scope of Inventory keys, which reference and describe Inventory objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_C_38_2_1_1_3_cfe364 => OutsideScope("Binds INVENTORY Directory Records and their Scope of Inventory keys, which reference and describe Inventory objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_C_38_2_1_1_5_78b883 => OutsideScope("Binds INVENTORY Directory Records and their Scope of Inventory keys, which reference and describe Inventory objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_2_1_ea4925 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_2_1_7d9cf7 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_2_1_9936a3 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_2_1_7f516a => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_2_1_ac61bd => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_1_e7d1bd => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_1_b0329b => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_19_9bc3d7 => OutsideScope("Binds RT DOSE, RT STRUCTURE SET, RT PLAN, RT TREAT RECORD and RADIOTHERAPY Directory Records, which reference radiotherapy objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_2_22d39d => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_2_e0336d => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_20_de0905 => OutsideScope("Binds RT DOSE, RT STRUCTURE SET, RT PLAN, RT TREAT RECORD and RADIOTHERAPY Directory Records, which reference radiotherapy objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_21_bffc26 => OutsideScope("Binds RT DOSE, RT STRUCTURE SET, RT PLAN, RT TREAT RECORD and RADIOTHERAPY Directory Records, which reference radiotherapy objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_22_b25913 => OutsideScope("Binds RT DOSE, RT STRUCTURE SET, RT PLAN, RT TREAT RECORD and RADIOTHERAPY Directory Records, which reference radiotherapy objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_23_39afea => OutsideScope("Binds PRESENTATION Directory Records, which reference Presentation State and Structured Display objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_24_c89381 => OutsideScope("Binds WAVEFORM and WF PRESENTATION Directory Records, which reference Waveform and Waveform Presentation State objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_25_c37466 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_26_e9e261 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_27_caf60c => OutsideScope("Binds SPECTROSCOPY and RAW DATA Directory Records, which reference Spectroscopy and Raw Data objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_28_6e0525 => OutsideScope("Binds SPECTROSCOPY and RAW DATA Directory Records, which reference Spectroscopy and Raw Data objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_29_975695 => OutsideScope("Binds REGISTRATION, FIDUCIAL and STEREOMETRIC Directory Records, which reference Spatial Registration, Spatial Fiducials and Stereometric Relationship objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_3_b5858e => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_3_2b5a27 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_30_059951 => OutsideScope("Binds REGISTRATION, FIDUCIAL and STEREOMETRIC Directory Records, which reference Spatial Registration, Spatial Fiducials and Stereometric Relationship objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_31_d5539d => OutsideScope("Binds HANGING PROTOCOL and PALETTE Directory Records, which reference Hanging Protocol and Color Palette objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_32_229ea2 => OutsideScope("Binds ENCAP DOC Directory Records, which reference Encapsulated Document objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_34_dc9c16 => OutsideScope("Binds VALUE MAP Directory Records, which reference Real World Value Mapping objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_35_1812bc => OutsideScope("Binds REGISTRATION, FIDUCIAL and STEREOMETRIC Directory Records, which reference Spatial Registration, Spatial Fiducials and Stereometric Relationship objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_36_3f0764 => OutsideScope("Binds HANGING PROTOCOL and PALETTE Directory Records, which reference Hanging Protocol and Color Palette objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_37_c8b140 => OutsideScope("Binds IMPLANT, IMPLANT ASSY and IMPLANT GROUP Directory Records, which reference implant template objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_38_c98f85 => OutsideScope("Binds IMPLANT, IMPLANT ASSY and IMPLANT GROUP Directory Records, which reference implant template objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_39_e4af35 => OutsideScope("Binds IMPLANT, IMPLANT ASSY and IMPLANT GROUP Directory Records, which reference implant template objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_4_5c9817 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_40_cf99bb => OutsideScope("Binds PLAN, MEASUREMENT, SURFACE, SURFACE SCAN, TRACT and ASSESSMENT Directory Records, which reference plan, measurement, surface, surface scan, tractography and content assessment objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_41_d41cf3 => OutsideScope("Binds PLAN, MEASUREMENT, SURFACE, SURFACE SCAN, TRACT and ASSESSMENT Directory Records, which reference plan, measurement, surface, surface scan, tractography and content assessment objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_42_e2a4e8 => OutsideScope("Binds PLAN, MEASUREMENT, SURFACE, SURFACE SCAN, TRACT and ASSESSMENT Directory Records, which reference plan, measurement, surface, surface scan, tractography and content assessment objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_43_03da94 => OutsideScope("Binds PLAN, MEASUREMENT, SURFACE, SURFACE SCAN, TRACT and ASSESSMENT Directory Records, which reference plan, measurement, surface, surface scan, tractography and content assessment objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_44_77d30e => OutsideScope("Binds PLAN, MEASUREMENT, SURFACE, SURFACE SCAN, TRACT and ASSESSMENT Directory Records, which reference plan, measurement, surface, surface scan, tractography and content assessment objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_45_a4f994 => OutsideScope("Binds PLAN, MEASUREMENT, SURFACE, SURFACE SCAN, TRACT and ASSESSMENT Directory Records, which reference plan, measurement, surface, surface scan, tractography and content assessment objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_46_0a8641 => OutsideScope("Binds RT DOSE, RT STRUCTURE SET, RT PLAN, RT TREAT RECORD and RADIOTHERAPY Directory Records, which reference radiotherapy objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_47_1711ae => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_48_120d8a => OutsideScope("Binds INVENTORY Directory Records and their Scope of Inventory keys, which reference and describe Inventory objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_49_5daee3 => OutsideScope("Binds WAVEFORM and WF PRESENTATION Directory Records, which reference Waveform and Waveform Presentation State objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_6_1_01b3be => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_6_1_f85aa6 => Undecidable("What a private Directory Record Type extends in the Information Model is defined only by its private definition, which its Private Record UID names"),
            Ps3_F_6_1_427b33 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_7_f0f851 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_7_9af80c => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_7_1b9490 => Behavior("Binds the Icon sizes File-set Readers and Updaters support"),
            Ps3_F_7_8da9c5 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_7_5a988c => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_7_c7d6e0 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_7_486583 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_7_277053 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_3_3_OffsetOfTheFirstDirectoryRecordOfTheRootDirectoryEntity_800e32 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_3_3_OffsetOfTheLastDirectoryRecordOfTheRootDirectoryEntity_894d9e => Checked("intrinsic-wsi-dicom-file-set"),
            // File-set Consistency Flag Enumerated Value 0000H (Table F.3-3)
            Ps3_F_3_3_FileSetConsistencyFlag_343323 => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            Ps3_F_3_3_OffsetOfTheNextDirectoryRecord_deaeb7 => Checked("intrinsic-wsi-dicom-file-set"),
            // Record In-use Flag Enumerated Value FFFFH (Table F.3-3)
            Ps3_F_3_3_RecordInUseFlag_cb4e2b => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            // Record In-use Flag Enumerated Value FFFFH (Table F.3-3); how readers interpret others is behavior
            Ps3_F_3_3_RecordInUseFlag_a50813 => Checked("intrinsic-wsi-dicom-iod-enumerated-values"),
            Ps3_F_3_3_OffsetOfReferencedLowerLevelDirectoryEntity_82f314 => Checked("intrinsic-wsi-dicom-file-set"),
            // Private Record UID Type 1C on Directory Record Type PRIVATE (Table F.3-3)
            Ps3_F_3_3_DirectoryRecordType_45a11c => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            Ps3_F_3_3_ReferencedFileID_325467 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_3_3_ReferencedFileID_a6aede => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_3_3_ReferencedFileID_0dc388 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_F_5_25_ContentSequence_65305f => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_C_17_5_TextValue_cbe4b1 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_5_TextValue_ebeaa5 => Behavior("Binds how applications interpret Text Value"),
            Ps3_C_18_1_1_MeasuredValueSequence_fcf6c2 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_1_1_NumericValue_04da1c => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_1_1_FloatingPointValue_929170 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_1_1_RationalNumeratorValue_93b0c9 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_1_1_RationalDenominatorValue_7a7dc3 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_4_1_ReferencedFrameNumber_cb2c38 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_4_1_IconImageSequence_9594fc => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_6_1_GraphicData_fb751a => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_7_1_ReferencedSamplePositions_581602 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_18_10_1_TabulatedValuesSequence_1093d3
            | Ps3_C_18_10_1_TableRowDefinitionSequence_da170e
            | Ps3_C_18_10_1_TableColumnDefinitionSequence_7fba82
            | Ps3_C_18_10_1_CellValuesSequence_9090da => OutsideScope("TABLE is not an admitted Value Type of any supported A.35 SR or KOS IOD; an SR DOCUMENT or KEY OBJECT DOC Directory Record copies only legal HAS CONCEPT MOD Content Items from its referenced document, so it cannot legally carry a TABLE Item either"),
            Ps3_F_5_26_ContentSequence_3fb953 => Checked("intrinsic-wsi-dicom-file-set"),
            Ps3_C_17_3_DisplayURI_29ba6d => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_3_DisplayURI_c5e304 => Undecidable("Which Study a URI identifies is known only to the service that resolves it"),
            Ps3_C_17_3a_MACCalculationTransferSyntaxUID_1ead61 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_F_5_49_ReferencedInstanceSequence_d79489 => OutsideScope("Binds the Referenced Instance and Referenced Waveform Sequence keys of WF PRESENTATION Directory Records, which describe Waveform Presentation State objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_49_ReferencedInstanceSequence_64ffb3 => OutsideScope("Binds the Referenced Instance and Referenced Waveform Sequence keys of WF PRESENTATION Directory Records, which describe Waveform Presentation State objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_49_ReferencedWaveformSequence_a9ed46 => OutsideScope("Binds the Referenced Instance and Referenced Waveform Sequence keys of WF PRESENTATION Directory Records, which describe Waveform Presentation State objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            Ps3_F_5_49_ReferencedWaveformSequence_3adeca => OutsideScope("Binds the Referenced Instance and Referenced Waveform Sequence keys of WF PRESENTATION Directory Records, which describe Waveform Presentation State objects; no pathology-slide File-set holds them, so a File-set mixing them is outside the validated scope: the file-set check still holds such records to F.2, F.3 and Table F.4-1 and names their Record Type as not validated, but neither the SOP Class they reference nor the referenced objects are validated"),
            // ---- PS3.3 A.35 Key Object Selection Document and SR document IODs. ----
            Ps3_A_35_4_3_1_2_fa2e8d => Checked("intrinsic-wsi-dicom-key-object-selection"),
            Ps3_A_35_4_3_1_3_ee396a => Checked("intrinsic-wsi-dicom-key-object-selection"),
            Ps3_C_17_2_2_867911 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_2_cfad85 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_2_d342cd => Undecidable("Whether a document took content from another, and in which Requested Procedure, is not encoded apart from the Sequences this sentence governs"),
            Ps3_C_17_2_2_b4da73 => Undecidable("Which documents a new document took content from is not encoded apart from the Predecessor Documents Sequence this sentence governs"),
            // Concept Name Code Sequence is Type 1C on the root CONTAINER (Table C.17-5).
            Ps3_C_17_3_1_c9b830 => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            Ps3_C_17_3_2_4_d212eb => Undecidable("What Observation Context an observation needs to be unambiguous is its meaning"),
            Ps3_C_17_6_2_1_599086 => Checked("intrinsic-wsi-dicom-key-object-selection"),
            Ps3_C_17_6_2_CurrentRequestedProcedureEvidenceSequence_e9fc34 => Checked("intrinsic-wsi-dicom-key-object-selection"),
            Ps3_A_35_1_3_1_2_41a954 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_3_05558b => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_3_926d02 => Undecidable("Which Composite SOP Instances the current Requested Procedure created is not encoded in the document"),
            Ps3_C_17_2_3_612a8f => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_3_9a0301 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_5_496e9d => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_7_aef65e => Undecidable("Conformance Statement content is not in the file"),
            Ps3_C_17_2_VerificationFlag_4ef4cd => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_C_17_2_PertinentOtherEvidenceSequence_4c72e4 => Undecidable("For which Requested Procedure the evidence was acquired is not encoded"),
            Ps3_A_35_2_3_1_2_c2ed9c => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_A_35_3_3_1_2_c62eb7 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_A_35_13_3_1_2_46ad78 => Checked("intrinsic-wsi-dicom-structured-report"),
            // ---- PS3.18 DICOMweb origin servers and the DICOM JSON Model ("dicomweb" scope). ----
            Ps18_6_d91688 => Behavior("States conformance as following the mandatory sections, each of whose sentences is a statement of its own"),
            Ps18_6_63e622 => OutsideScope("Defers to the other Parts: the retrieved Instances and Metadata are held to them by the file checks, whose statements are catalogued in the IOD scopes"),
            Ps18_6_a4ac92 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_6_c5fe84 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_8_57a047 => Behavior("Lead-in to Section 8, whose sentences are statements of their own"),
            Ps18_8_1_2_2f06d1 => Checked("dicomweb-response-messages"),
            Ps18_8_1_2_1_b2a834 => Checked("dicomweb-response-messages"),
            Ps18_8_3_9b50d1 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_e050ee => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_a6dc97 => Undecidable("Which Query Parameters an origin server defines or extends is recorded only in its Conformance Statement"),
            Ps18_8_3_e6a7e1 => Checked("dicomweb-search-parameters"),
            Ps18_8_3_b43156 => Checked("dicomweb-search-parameters"),
            Ps18_8_3_83893d => Checked("dicomweb-search-parameters"),
            Ps18_8_3_1_25887b => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_1_29ec53 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_1_709662 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_1_bfb069 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_1_1af926 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_1_364bab => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_1_1_176962 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_2_9b4ce6 => Behavior("Introduces the usage columns of the Query Parameter tables, whose requirements are the Services' statements"),
            Ps18_8_3_3_1_bfdd64 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_3_1_2ce95e => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_3_1_f66157 => Checked("dicomweb-accept-negotiation"),
            Ps18_8_3_3_1_02034f => Checked("dicomweb-accept-negotiation"),
            Ps18_8_3_3_1_678a8f => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_3_1_5471db => Checked("dicomweb-accept-negotiation"),
            Ps18_8_3_3_2_d188ed => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_3_2_f5a10c => Checked("dicomweb-character-sets"),
            Ps18_8_3_4_004aab => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_1_96ce71 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_19e123 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_e6719a => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_99a9b1 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_90ad2e => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_f1c09e => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_cd456c => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_0f2b63 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_659035 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_1_1_65511e => Checked("dicomweb-search-matching"),
            Ps18_8_3_4_1_1_2ddb8d => Checked("dicomweb-search-matching"),
            Ps18_8_3_4_2_0dc70f => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_2_99a62c => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_2_6532de => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_2_974cbd => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_3_3b16a0 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_3_449008 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_4_535c8f => Checked("dicomweb-search-paging"),
            Ps18_8_3_4_4_57201f => Checked("dicomweb-search-paging"),
            Ps18_8_3_4_4_1_bc96f3 => Checked("dicomweb-search-paging"),
            Ps18_8_3_4_4_1_30e63d => Checked("dicomweb-search-matching"),
            Ps18_8_3_4_4_1_c89a24 => Checked("dicomweb-search-matching"),
            Ps18_8_3_4_4_1_aeb4b5 => Checked("dicomweb-search-matching"),
            Ps18_8_3_4_4_1_23f2e6 => Checked("dicomweb-search-studies"),
            Ps18_8_3_4_4_1_cf6139 => Checked("dicomweb-search-paging"),
            Ps18_8_3_4_5_c08063 => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_5_0d71fe => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_5_42ba9c => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_5_b72b36 => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_6_f33457 => Undecidable("Without Multiple Value Matching the meaning of a multi-valued Key Attribute is undefined (PS3.4 C.2.2.2.8), so no result shows whether a search was performed without it"),
            Ps18_8_3_4_6_47ff4f => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_6_b1bede => Undecidable("Without Multiple Value Matching the meaning of a multi-valued Key Attribute is undefined (PS3.4 C.2.2.2.8), so no result shows whether a search was performed without it"),
            Ps18_8_3_4_6_8921d5 => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_5_79fa39 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_59eb59 => Undecidable("Whether the rendering parameters were applied as if they were a Presentation State is a judgment of the rendered pixels"),
            Ps18_8_3_5_1_1_88319d => Undecidable("Whether a rendered image is annotated with patient information is a judgment of its burned-in pixels"),
            Ps18_8_3_5_1_1_25f294 => Undecidable("Whether a rendered image is annotated with information about the procedure is a judgment of its burned-in pixels"),
            Ps18_8_3_5_1_1_6631f6 => Undecidable("Whether a rendered image carries annotations is a judgment of its pixels"),
            Ps18_8_3_5_1_1_bebf80 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_8_3_5_1_1_f144a7 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_1_f0beb3 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_1_3e159b => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_2_2de2b0 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_2_b4e81b => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_8_3_5_1_3_03151c => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_3_4d74be => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_3_5_1_3_27beb9 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_3_f3ea9d => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_5_1_3_e4ca74 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_5_1_3_097d44 => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_3_5_1_3_32815d => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_3_5_1_3_4361fc => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_4_4b96ca => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_5_1_4_ffbc37 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_4_1e699e => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_5_1_4_b22e5b => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_5_de2c1f => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_5_22e85a => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_5_ef6e16 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_5_09e3f7 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_5_e387a8 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_5_acc817 => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_5_3831ed => Checked("dicomweb-rendered-parameters"),
            Ps18_8_3_5_1_5_349394 => OutsideScope("Binds Rendered Media Types that do not embed ICC Profiles; the iccprofile requests are of image/jpeg, which embeds them in APP2 segments"),
            Ps18_8_3_5_1_6_19bd1d => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_b27329 => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_f98680 => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_002d1a => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_57e951 => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_bc7191 => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_34f954 => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_b3c82d => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_1_6_8680aa => OutsideScope("Binds rendering through a Presentation State (8.3.5.1.6), which the DICOMweb checks do not request"),
            Ps18_8_3_5_3_1da87b => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_6d180c => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_1_effae8 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_1_701b2e => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_1_ec6f09 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_1_70bdbc => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_2_3cd7db => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_2_44db1a => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_2_8b972f => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_3_8db39b => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_4_37219e => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_4_e99a2c => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_4_70f383 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_5_f6469f => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_6_9e8a7f => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_7_6b200e => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_8_4d60bb => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_9_114552 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_9_a3ae43 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_9_22e88e => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_10_782fd7 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_10_651130 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_10_097876 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_10_c9e8b0 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_11_26ebb7 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_11_215ece => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_12_cede21 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_13_5db0db => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_13_5a0f6a => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_13_abcde0 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_3_5_3_13_fa0104 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_8_4_1_35dcfd => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_4_1_1_5522f3 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_4_2_c379f4 => Behavior("Introduces Table 8.4.2-1, whose rows are statements of their own"),
            Ps18_8_4_2_4547a2 => Checked("dicomweb-payload-headers"),
            Ps18_8_4_2_00363a => Undecidable("A transfer coding or content coding of a payload shows only in the Header Field this row requires"),
            Ps18_8_4_2_941c33 => Checked("dicomweb-content-location"),
            Ps18_8_4_3_5f9c2b => Behavior("Introduces Table 8.4.3-1, whose rows are statements of their own"),
            Ps18_8_4_3_844f2f => Checked("dicomweb-payload-headers"),
            Ps18_8_4_3_d18d5e => Checked("dicomweb-payload-headers"),
            Ps18_8_4_3_36d7ba => Checked("dicomweb-payload-headers"),
            Ps18_8_4_3_9f15c9 => Undecidable("A transfer coding or content coding of a payload shows only in the Header Field this row requires"),
            Ps18_8_5_c46151 => Checked("dicomweb-error-responses"),
            Ps18_8_5_33d660 => Undecidable("Whether a resource was deleted is known only to the origin server"),
            Ps18_8_5_4d465c => Checked("dicomweb-error-responses"),
            Ps18_8_5_5238e7 => Undecidable("Whether a failure is a SOP Class Not Supported error is known only to the origin server; a Store answers that condition with 409 (Table 10.5.3-1)"),
            Ps18_8_5_9d29c5 => Behavior("Advises implementers on what an error discloses; binds no observable property"),
            Ps18_8_6_f9a38b => Checked("dicomweb-payload-headers"),
            Ps18_8_6_d75753 => Undecidable("A transfer coding or content coding of a payload shows only in the Header Field this row requires"),
            Ps18_8_6_594d78 => Checked("dicomweb-payload-headers"),
            Ps18_8_6_779724 => Checked("dicomweb-payload-headers"),
            Ps18_8_6_1_1_1b7688 => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_6_1_1_6ca86c => Checked("dicomweb-range-requests"),
            Ps18_8_6_1_2_a37638 => Checked("dicomweb-retrieve-frames"),
            Ps18_8_6_1_2_831ad9 => Checked("dicomweb-retrieve-frames"),
            Ps18_8_6_1_2_0641f0 => Checked("dicomweb-retrieve-frames"),
            Ps18_8_6_1_2_7d414d => Checked("dicomweb-content-location"),
            Ps18_8_6_1_2_e4cdcf => Checked("dicomweb-retrieve-frames"),
            Ps18_8_6_1_2_d6fc8c => OutsideScope("Binds a multipart payload holding both Metadata and Bulk Data, as a Store of separate Metadata and Bulk Data does; no request of the checks sends or receives one"),
            Ps18_8_6_1_2_47ab48 => Checked("dicomweb-retrieve-bulkdata"),
            Ps18_8_6_2_1_0ad6af => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_96d987 => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_32730c => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_38b852 => Checked("dicomweb-accept-negotiation"),
            Ps18_8_7_3_af7a6d => Behavior("Defines the Required usage of the Media Type tables; 10.4.4 and 10.6.4 require each resource's Required Media Types"),
            Ps18_8_7_3_1_0156f9 => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_3_abe49a => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_4_189432 => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_4_4d50c4 => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_4_d251f5 => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_3_5_1_ba160e => Checked("dicomweb-retrieve-frames"),
            Ps18_8_7_3_5_2_5f0eab => Checked("dicomweb-retrieve-frames-uncompressed"),
            Ps18_8_7_3_5_2_c627c4 => Checked("dicomweb-retrieve-frames"),
            Ps18_8_7_3_5_2_4e68e5 => Checked("dicomweb-retrieve-instance"),
            Ps18_8_7_3_5_2_26061c => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_8_7_3_5_2_0a2ea1 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_7_3_5_3_fd72cc => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_7_4_9908e2 => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_7_4_203a41 => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_7_4_cb1519 => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_7_4_a53bbc => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_8_7_5_cefa65 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_7_5_6c4207 => Checked("dicomweb-accept-required"),
            Ps18_8_7_5_022dcf => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_7_5_8edd5d => Checked("dicomweb-accept-negotiation"),
            Ps18_8_7_7_4cbea7 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_7_7_2b0224 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_7_7_3bc9f7 => Checked("dicomweb-accept-required"),
            Ps18_8_7_7_f85808 => Checked("dicomweb-accept-negotiation"),
            Ps18_8_7_8_1_15cea2 => Checked("dicomweb-accept-negotiation"),
            Ps18_8_7_8_1_400d8d => Behavior("Leaves a tie to the origin server; any choice conforms"),
            Ps18_8_7_8_2_86982b => Checked("dicomweb-transfer-syntaxes"),
            Ps18_8_7_9_866a62 => Checked("dicomweb-payload-headers"),
            Ps18_8_7_9_6332fc => Checked("dicomweb-retrieve-frames"),
            Ps18_8_8_82a3c8 => Checked("dicomweb-payload-headers"),
            Ps18_8_8_6509a0 => Checked("dicomweb-character-sets"),
            Ps18_8_8_e453e6 => Checked("dicomweb-payload-headers"),
            Ps18_8_8_9568d4 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_8_8_bed339 => Checked("dicomweb-capabilities"),
            Ps18_8_8_1_f28f95 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_8_1_adc968 => Checked("dicomweb-character-sets"),
            Ps18_8_8_4_82a730 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_8_5_66f071 => Undecidable("The Selected Character Set shows only in the strings of a payload: the DICOM JSON Model is UTF-8 whatever is selected (F.2), and a rendered image holds no strings"),
            Ps18_8_8_5_49daaf => OutsideScope("Binds renderings that hold strings, such as the text/html of a Structured Report; the checks render image Frames, which hold none"),
            Ps18_8_8_5_e0818e => Undecidable("Whether the origin server supports transcoding all glyphs of the Target Resource into the Selected Character Set is not observable"),
            Ps18_8_8_5_6484c0 => Undecidable("Whether the origin server supports transcoding all glyphs of the Target Resource into the Selected Character Set is not observable"),
            Ps18_8_9_3807fb => Checked("dicomweb-capabilities"),
            Ps18_8_9_1_83e9b2 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_9_2_257548 => Checked("dicomweb-capabilities"),
            Ps18_8_9_3_1_c18832 => Checked("dicomweb-capabilities"),
            Ps18_8_9_3_1_1_56b84b => Checked("dicomweb-payload-headers"),
            Ps18_8_9_3_1_1_a92016 => Undecidable("A transfer coding or content coding of a payload shows only in the Header Field this row requires"),
            Ps18_8_9_3_2_e15f6c => Checked("dicomweb-capabilities"),
            Ps18_8_9_3_2_e951e9 => Checked("dicomweb-capabilities"),
            Ps18_8_10_2_155142 => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_2_19fa61 => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_3_2e922e => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_4_1_dca2f3 => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_4_1_1_02c820 => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_4_2_863806 => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_4_3_a4ebc9 => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_5_1_3cd98c => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_5_1_1_426d0e => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_5_1_1_85268e => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_8_10_5_2_60518d => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_10_1_2_62533e => Checked("dicomweb-accept-negotiation"),
            Ps18_10_1_2_c6ec1b => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_1_3_e8962d => Checked("dicomweb-character-sets"),
            Ps18_10_2_ff7bbc => Checked("dicomweb-capabilities"),
            Ps18_10_2_61f20f => OutsideScope("Holds the origin server to every Mandatory Retrieve resource of Table 10.3-2; the Study and Series resources and their renderings hold every Instance of a Study, which the bounded checks do not download, while the Instance, Metadata, Frames, Rendered Frames and Bulk Data resources are held by the dicomweb-retrieve-* checks"),
            Ps18_10_2_f19832 => Checked("dicomweb-store"),
            Ps18_10_2_87e1ae => Checked("dicomweb-search-resources"),
            Ps18_10_4_1_fbb8ca => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_4_1_1_4_063876 => Checked("dicomweb-retrieve-thumbnail"),
            Ps18_10_4_1_1_7_9b24ce => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_10_4_1_1_8_75d4f5 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_10_4_1_2_218fea => Checked("dicomweb-rendered-parameters"),
            Ps18_10_4_1_2_3912ad => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_4_1_3_2e2f5e => Checked("dicomweb-character-sets"),
            Ps18_10_4_1_3_694856 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_4_2_b7f863 => Checked("dicomweb-retrieve-frames"),
            Ps18_10_4_2_cdfc30 => Checked("dicomweb-retrieve-frames"),
            Ps18_10_4_2_275fa3 => Checked("dicomweb-retrieve-rendered"),
            Ps18_10_4_3_7c06ce => Checked("dicomweb-response-messages"),
            Ps18_10_4_3_2_4aeaf7 => Checked("dicomweb-payload-headers"),
            Ps18_10_4_3_2_4a819f => Checked("dicomweb-payload-headers"),
            Ps18_10_4_3_3_08dd32 => Checked("dicomweb-retrieve-frames"),
            Ps18_10_4_3_3_5f84f7 => Checked("dicomweb-payload-headers"),
            Ps18_10_4_3_3_1_1aa24a => Checked("dicomweb-retrieve-instance"),
            Ps18_10_4_3_3_2_095703 => Checked("dicomweb-retrieve-instance"),
            Ps18_10_4_3_3_3_cf0723 => Undecidable("Which SOP Classes conformance is claimed for, and so which Instances must render, is Conformance Statement content; that a rendering is in a Rendered Media Type is checked by dicomweb-retrieve-rendered"),
            Ps18_10_4_3_3_3_cb42e6 => Checked("dicomweb-retrieve-rendered"),
            Ps18_10_4_3_3_4_a69cbd => Undecidable("Whether a Thumbnail is a meaningful representation of its resource is a judgment of its pixels; that it is a single image in a Rendered Media Type is checked by dicomweb-retrieve-thumbnail"),
            Ps18_10_4_3_3_4_0a09b4 => Undecidable("Whether a Thumbnail shows Patient Identifying Information is a judgment of its pixels"),
            Ps18_10_4_3_3_4_c79fe6 => Checked("dicomweb-retrieve-thumbnail"),
            Ps18_10_4_3_3_5_403fbb => Checked("dicomweb-retrieve-instance"),
            Ps18_10_4_3_3_5_abaa16 => Checked("dicomweb-retrieve-bulkdata"),
            Ps18_10_4_3_3_6_f8d19e => Checked("dicomweb-retrieve-frames-uncompressed"),
            Ps18_10_4_3_3_6_759e1b => Checked("dicomweb-retrieve-frames"),
            Ps18_10_4_3_3_7_06b035 => OutsideScope("Binds Rendered MPR and 3D Volume resources (8.3.5.3), which the DICOMweb checks do not request; a whole-slide viewer retrieves no volume renderings"),
            Ps18_10_4_4_eca186 => Checked("dicomweb-retrieve-rendered"),
            Ps18_10_4_4_76da6b => Checked("dicomweb-payload-headers"),
            Ps18_10_4_4_7c2906 => Checked("dicomweb-character-sets"),
            Ps18_10_4_5_97b5d7 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_10_4_5_c0c860 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_10_5_1_2679bb => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_1_1_fd05e8 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_3_32bd96 => Checked("dicomweb-store"),
            Ps18_10_5_1_3_7f0811 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_3_507839 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_3_0744da => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_3_3c381a => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_f145f8 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_ae1ee1 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_424476 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_c91609 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_aa2e62 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_4e8828 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_119083 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_55bcba => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_91aeab => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_08d3fd => OutsideScope("Binds a Store of separate Metadata and compressed Bulk Data (10.5.1.4), which the store check does not send; it stores the PS3.10 file the user names"),
            Ps18_10_5_1_4_44c68a => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_835b72 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_36b177 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_4fe378 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_1_4_b233e9 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_5_2_1454bb => Checked("dicomweb-store-readback"),
            Ps18_10_5_2_52e705 => Checked("dicomweb-store-readback"),
            Ps18_10_5_2_0f4a00 => OutsideScope("Binds a Store of separate Metadata and compressed Bulk Data (10.5.1.4), which the store check does not send; it stores the PS3.10 file the user names"),
            Ps18_10_5_2_b32f16 => OutsideScope("Binds a Store of separate Metadata and compressed Bulk Data (10.5.1.4), which the store check does not send; it stores the PS3.10 file the user names"),
            Ps18_10_5_2_37adbc => OutsideScope("Binds a Store of separate Metadata and compressed Bulk Data (10.5.1.4), which the store check does not send; it stores the PS3.10 file the user names"),
            Ps18_10_5_2_3d12cc => OutsideScope("Binds a Store of separate Metadata and compressed Bulk Data (10.5.1.4), which the store check does not send; it stores the PS3.10 file the user names"),
            Ps18_10_5_3_d53623 => Checked("dicomweb-response-messages"),
            Ps18_10_5_3_791d84 => Checked("dicomweb-store"),
            Ps18_10_5_3_82c0fb => Checked("dicomweb-store-readback"),
            Ps18_10_5_3_2_564228 => Checked("dicomweb-store"),
            Ps18_10_5_3_2_5fed17 => Checked("dicomweb-payload-headers"),
            Ps18_10_5_3_2_c7f5a8 => Undecidable("A transfer coding or content coding of a payload shows only in the Header Field this row requires"),
            Ps18_10_5_3_2_44dbf9 => Checked("dicomweb-store"),
            Ps18_10_5_3_2_6b4aa8 => Checked("dicomweb-payload-headers"),
            Ps18_10_5_3_3_5d01b8 => Checked("dicomweb-store"),
            Ps18_10_5_4_8e8031 => OutsideScope("Binds the Store resources and Media Types beyond the one the store check sends, a PS3.10 file as multipart/related; type=\"application/dicom\" to All Studies; storing the Default and Required Metadata and Bulk Data forms would need them authored from the file and a second stored copy"),
            Ps18_10_5_5_570533 => OutsideScope("Binds the Store resources and Media Types beyond the one the store check sends, a PS3.10 file as multipart/related; type=\"application/dicom\" to All Studies; storing the Default and Required Metadata and Bulk Data forms would need them authored from the file and a second stored copy"),
            Ps18_10_5_5_0dd1cc => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_10_5_5_3f9a25 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_10_6_1_13e21f => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_6_1_1_fbf487 => Checked("dicomweb-search-resources"),
            Ps18_10_6_1_1_739f9a => Checked("dicomweb-search-resources"),
            Ps18_10_6_1_2_b70763 => Checked("dicomweb-search-matching-parameters"),
            Ps18_10_6_1_2_33e0e4 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_6_1_2_1_053f5d => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_6_1_2_3_5f3ea4 => Checked("dicomweb-search-resources"),
            Ps18_10_6_1_2_3_ea8783 => Checked("dicomweb-search-matching-attributes"),
            Ps18_10_6_1_3_4a0146 => Checked("dicomweb-character-sets"),
            Ps18_10_6_1_3_313eda => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_10_6_2_2e92e4 => Checked("dicomweb-search-matching"),
            Ps18_10_6_3_5fdf5d => Checked("dicomweb-response-messages"),
            Ps18_10_6_3_1_d3c202 => Checked("dicomweb-search-paging"),
            Ps18_10_6_3_2_f25a3d => Checked("dicomweb-payload-headers"),
            Ps18_10_6_3_2_3b1518 => Checked("dicomweb-payload-headers"),
            Ps18_10_6_3_2_19b797 => Checked("dicomweb-payload-headers"),
            Ps18_10_6_3_2_011962 => Undecidable("A transfer coding or content coding of a payload shows only in the Header Field this row requires"),
            Ps18_10_6_3_3_95b5f5 => Checked("dicomweb-search-studies"),
            Ps18_10_6_3_3_eb938f => Checked("dicomweb-search-matching"),
            Ps18_10_6_3_3_42127c => Checked("dicomweb-search-matching"),
            Ps18_10_6_3_3_1_fa8f38 => Checked("dicomweb-search-studies"),
            Ps18_10_6_3_3_1_90f5de => Undecidable("Whether the origin server knows the Instance Availability or Timezone Offset From UTC of a Study is not observable"),
            Ps18_10_6_3_3_1_03907b => Checked("dicomweb-search-retrieve-url"),
            Ps18_10_6_3_3_1_fa17b0 => Checked("dicomweb-search-matching-attributes"),
            Ps18_10_6_3_3_1_48c8f7 => Checked("dicomweb-search-studies"),
            Ps18_10_6_3_3_2_c466e3 => Checked("dicomweb-search-series"),
            Ps18_10_6_3_3_2_f2bda1 => Undecidable("Whether the origin server knows a Series' Timezone Offset From UTC, Series Description, Performed Procedure Step Start Date and Time or Request Attributes is not observable"),
            Ps18_10_6_3_3_2_bd76da => Checked("dicomweb-search-retrieve-url"),
            Ps18_10_6_3_3_2_a9a64b => Checked("dicomweb-search-resources"),
            Ps18_10_6_3_3_2_e21078 => Checked("dicomweb-search-series"),
            Ps18_10_6_3_3_3_7eefe5 => Checked("dicomweb-search-instances"),
            Ps18_10_6_3_3_3_664a46 => Undecidable("Whether the origin server knows an Instance's Availability, Timezone Offset From UTC or Number of Frames is not observable; the Rows, Columns and Bits Allocated of an image Instance it holds are checked by dicomweb-search-instances"),
            Ps18_10_6_3_3_3_c3aa52 => Checked("dicomweb-search-retrieve-url"),
            Ps18_10_6_3_3_3_80ba54 => Checked("dicomweb-search-resources"),
            Ps18_10_6_4_0018ca => Checked("dicomweb-search-studies"),
            Ps18_10_6_5_a721a2 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_10_6_5_952a59 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_10_6_5_02ff26 => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_F_1_f1d5d4 => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_1_3d429f => Behavior("Binds a recipient converting the DICOM JSON Model back into a binary Data Set, not the document"),
            Ps18_F_2_dc9ab9 => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_2_2_467f17 => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_2_2_f11873 => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_2_3_523a5a => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_2_3_ac289e => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_2_3_219bd2 => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_2_5_9122d6 => Checked("intrinsic-wsi-dicom-json-model"),
            Ps18_F_3_1_aa15a9 => Behavior("Binds converting other DICOM formats into the DICOM JSON Model; a document records no conversion"),
            Ps18_H_292de8 => Checked("dicomweb-capabilities"),
            Ps18_H_a22678 => Checked("dicomweb-capabilities"),
            Ps18_H_9988db => Checked("dicomweb-capabilities"),
            Ps18_I_1_63fa4c => Checked("dicomweb-store"),
            Ps18_I_2_1_613139 => Checked("dicomweb-store-readback"),
            Ps18_I_2_1_564016 => Undecidable("Which of several applicable codes is the most appropriate is the origin server's judgment"),
            Ps18_I_2_2_d7bc8b => Undecidable("Why an Instance could not be stored is known only to the origin server, so which Failure Reason applies is not observable; that each failed Item has one is checked by dicomweb-store"),
            Ps18_I_2_2_5d438b => Undecidable("Conformance Statement content is not part of an origin server's responses"),
            Ps18_I_2_2_6d4077 => Undecidable("Which of several applicable codes is the most appropriate is the origin server's judgment"),
            // ---- Recommendations ("should"): a departure is an advisory, never a failure. ----
            Ps3_10_29_1_afd1eb => Behavior("Binds applications reading the Values, not the encoded object"),
            Ps3_10_41_1_bd6e85 => Undecidable("Whether acquisition and reconstruction were recorded in separate Performed Procedure Protocol SOP Instances is not encoded apart from this Sequence"),
            Ps3_8_3_0f73df => Behavior("Remarks that a code may have several Code Meanings; binds no encoded property"),
            Ps3_C_11_15_1_1_2d2993 => Behavior("Lead-in to the ICC Profile recommendations of C.11.15.1.1, each a statement of its own"),
            // The value walk advises the Rendering Intent and LUT tag types of each ICC Profile (0028,2000).
            Ps3_C_11_15_1_1_108351 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            Ps3_C_11_15_1_1_ee816f => Checked("intrinsic-wsi-dicom-iod-value-format"),
            Ps3_C_11_15_1_1_7708db => Undecidable("The illumination source of the acquisition is not encoded, so whether it was D50 is unknown"),
            Ps3_C_2_3_87ba73 => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_2_3_8d494e => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_2_3_ef150f => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_2_3_fd5e88 => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_2_4_8e42ca => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_7_9_2_2_abf79d => Behavior("Binds applications expanding a Segmented Palette Color Lookup Table"),
            Ps3_C_8_16_2_1_2_13b5ce => Behavior("Advises applications on the use of the image; binds no encoded property"),
            Ps3_C_7_4a_MedicalAlerts_dac93c => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_7_4a_GenderIdentityComment_45abf7 => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_7_4a_PersonNamesToUseSequence_14184e => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_7_4a_NameToUse_03e192 => Behavior("Describes what the Attribute conveys to the people who read it; binds no encoded property"),
            Ps3_C_8_12_4_1_RecommendedAbsentPixelCIELabValue_7c9a62 => Behavior("Binds display applications, which render the content in this recommended color"),
            Ps3_C_12_1_SOPInstanceStatus_dd34ff => Behavior("Defines the storage statuses, which bind how storage applications treat the SOP Instance"),
            Ps5_8_2_4_1391f2 => Behavior("Binds decoders to take the image characteristics from the compressed data stream when the Data Elements disagree with it; the codestream check reports the disagreement"),
            Ps5_8_2_5_9b8ddc => Behavior("Binds decoders to take the image characteristics from the compressed data stream when the Data Elements disagree with it; the codestream check reports the disagreement"),
            Ps5_8_2_6_9fc2f4 => Behavior("Binds decoders to take the image characteristics from the compressed data stream when the Data Elements disagree with it; the codestream check reports the disagreement"),
            Ps5_8_2_7_fa9835 => Behavior("Binds decoders to take the image characteristics from the compressed data stream when the Data Elements disagree with it; the codestream check reports the disagreement"),
            Ps5_8_2_8_b60cd8 => Behavior("Binds decoders to take the image characteristics from the compressed data stream when the Data Elements disagree with it; the codestream check reports the disagreement"),
            Ps5_8_2_14_d699a9 => Behavior("Binds decoders to take the image characteristics from the compressed data stream when the Data Elements disagree with it; the codestream check reports the disagreement"),
            Ps5_9_58062f => Behavior("Binds applications interpreting UIDs, which carry no semantics"),
            Ps5_10_8_1c99cd => Behavior("Binds applications retrieving Pixel Data by its URL, not the encoded object"),
            Ps5_10_11_8bbf95 => Undecidable("Whether video content is stereoscopic 3D with frame packing is not encoded apart from the Transfer Syntax this sentence recommends"),
            Ps5_10_16_1_179222 => OutsideScope("Recommends how a DICOM-RTV Flow (PS3.22) carries video or audio under a SMPTE ST 2110 Transfer Syntax; such a Flow is sent outside any Part 10 file"),
            Ps5_10_16_1_ab829e => OutsideScope("Recommends how a DICOM-RTV Flow (PS3.22) carries video or audio under a SMPTE ST 2110 Transfer Syntax; such a Flow is sent outside any Part 10 file"),
            Ps5_A_4_7_7d8047 => Undecidable("Whether video content is stereoscopic 3D with frame packing is not encoded apart from the Transfer Syntax this sentence recommends"),
            Ps5_A_4_7_3f459c => Undecidable("Whether video content is stereoscopic 3D with frame packing is not encoded apart from the Transfer Syntax this sentence recommends"),
            Ps5_A_10_6c9355 => OutsideScope("Recommends how a DICOM-RTV Flow (PS3.22) carries video or audio under a SMPTE ST 2110 Transfer Syntax; such a Flow is sent outside any Part 10 file"),
            Ps10_7_3_6a4a13 => Behavior("Binds the Media Format Layer of a file system, not files"),
            Ps3__C_37_1_2_RecommendedDisplayCIELabValue_055322 => Behavior("Binds display applications, which render the content in this recommended color"),
            Ps3_C_8_20_2_RecommendedDisplayGrayscaleValue_540ae9 => Behavior("Binds display applications, which render the content in this recommended color"),
            Ps3_C_8_20_2_RecommendedDisplayCIELabValue_2cdc83 => Behavior("Binds display applications, which render the content in this recommended color"),
            Ps18_8_1_2_3_9d5b65 => Checked("dicomweb-error-responses"),
            Ps18_8_2_d8dfb9 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_3_1_b942f2 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_3_4_3_fd4aeb => Checked("dicomweb-search-matching-parameters"),
            Ps18_8_3_4_3_09ab10 => Checked("dicomweb-search-matching-parameters"),
            // The scaling this describes is 8.3.5.1.3's requirement (shall), which the rendered check holds.
            Ps18_8_3_5_1_3_668754 => Checked("dicomweb-retrieve-rendered"),
            Ps18_8_4_1_1_9edde2 => Behavior("Binds the user agent's requests: what the DICOMweb checks send, not what an origin server returns"),
            Ps18_8_5_5bffa1 => Checked("dicomweb-error-responses"),
            Ps18_8_5_baf5aa => OutsideScope("Recommends 204 for an upload answered without a payload; a Store success carries a Store Instances Response Module (10.5.3.3), and the checks upload nothing else"),
            Ps18_8_5_3bae37 => Checked("dicomweb-error-responses"),
            Ps18_8_5_91af9a => Checked("dicomweb-error-responses"),
            Ps18_8_5_65673b => Checked("dicomweb-error-responses"),
            Ps18_8_5_9640cd => Undecidable("Whether the origin server knows that a resource's absence is permanent is not observable"),
            Ps18_8_5_66cd47 => Checked("dicomweb-error-responses"),
            Ps18_8_6_3_cba0ee => Undecidable("Whether a Status Report is clear and succinct is a judgment of its text"),
            Ps18_8_6_3_de8b45 => Checked("dicomweb-accept-required"),
            Ps18_8_7_1_af6537 => Checked("dicomweb-payload-headers"),
            Ps18_8_7_1_6e1f64 => Checked("dicomweb-payload-headers"),
            Ps18_8_10_4_1_6d25c9 => OutsideScope("Binds Notifications (8.10), which the Worklist Service uses and the DICOMweb checks do not cover"),
            Ps18_10_4_3_3_6cc548 => Checked("dicomweb-error-responses"),
            Ps18_10_5_3_2_27ad16 => Checked("dicomweb-store"),
            Ps18_10_6_3_1_180987 => Checked("dicomweb-error-responses"),
            // ---- Prohibitions worded "is not permitted", "is prohibited", "is forbidden", "may not be used" or "is required to be". ----
            Ps15_E_1_8d5445 => Behavior("States what de-identified SOP Instances are for; binds no encoded property"),
            Ps18_8_7_3_5_2_7824a0 => OutsideScope("Binds the URI Service (Chapter 9), which the DICOMweb checks do not cover"),
            Ps18_8_7_4_30b2f6 => Checked("dicomweb-retrieve-rendered"),
            Ps3_A_35_13_3_1_2_67a070 => Checked("intrinsic-wsi-dicom-structured-report"),
            Ps3_A_35_3_3_1_2_c338b5 => Checked("intrinsic-wsi-dicom-structured-report"),
            // Placement::SharedOnly and NotShared of the Unassigned Converted Attributes groups (Table A.75-2).
            Ps3_A_75_5_6e2343 => Tables,
            Ps3_A_75_5_d8b43b => Tables,
            Ps3_C_12_1_1_7_1_03edd3 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            Ps3_C_7_6_3_1_2_2ac29a => OutsideScope("Binds the US Image Module, which neither these IODs nor an Icon Image includes"),
            Ps3_C_8_20_2_3_3_3dd2ed => Checked("intrinsic-wsi-dicom-segmentation"),
            // DT components are positional: a missing middle component leaves the PS3.5 6.2 form.
            Ps5_6_2_b82926 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            Ps5_8_2_ed0a1a => Checked("intrinsic-wsi-dicom-iod-attribute-presence"),
            // A run header of -128 fails, wherever the segment holds one
            Ps5_G_3_1_48c218 => Checked("intrinsic-wsi-dicom-codestream"),
            // ---- Prohibitions worded "may not" (NORMATIVE_SENTENCES of the generator). ----
            Ps3_C_7_6_16_1_1_b640e8 => Checked("intrinsic-wsi-dicom-iod-modules"),
            Ps3_C_8_16_1_3_6f1c70 => OutsideScope("C.8.16.1.3 binds the Image Type of Enhanced CT, MR, US and Photoacoustic images; VL WSI Image Type Value 3 is VOLUME, LABEL, OVERVIEW or THUMBNAIL (C.8.12.4.1.1), which intrinsic-wsi-dicom-wsi-image checks, so MIXED is no VL WSI Value"),
            // ---- Prohibitions worded "is not allowed". ----
            Ps18_10_3_98a104 => OutsideScope("A blank cell binds the Transaction on that resource: the Store ones would change the origin server's state, so the store check sends none, and the Retrieve and Search ones name the URIs of the other Transaction's resources, so no request tells them apart"),
            // Table C.8.32-3 Values 1 and 2 are the generated tables'; MIXED is the Parametric Map check's.
            Ps3_C_8_32_3_FrameType_4a0dab => Checked("intrinsic-wsi-dicom-parametric-map"),
            Ps5_10_11_194813 => OutsideScope("Binds AVC/H.264 encodings, which require YBR_PARTIAL_420 (PS3.5 8.2.7 and 8.2.8); VL WSI excludes that Photometric Interpretation (PS3.3 C.8.12.4.1.5)"),
            Ps5_6_2_4f6b04 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            Ps5_6_2_ce2868 => Checked("intrinsic-wsi-dicom-iod-value-format"),
            Ps5_6_2_e1decf => Checked("intrinsic-wsi-dicom-iod-value-format"),
            Ps5_7_8_1_502f66 => Checked("intrinsic-wsi-dicom-encoding"),
        }
    }
}

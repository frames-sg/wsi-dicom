//! The disposition of every normative statement in `requirements.rs` that the
//! generated tables do not enforce: the check that enforces it, or why a file
//! cannot be held to it. An `Unchecked` statement is decidable from the
//! encoded objects and awaits the named check family.

use super::requirements::Requirement::{self, *};
use crate::validation::RequirementDisposition::{
    self, Behavior, Checked, OutsideScope, Undecidable,
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
            Ps5_7_3_3c0c4e => Checked("intrinsic-pixel-structure"),
            // Item or delimiter tags written with a VR field misframe and fail parsing
            Ps5_7_5_1593f5 => Checked("intrinsic-pixel-structure"),
            // Non-(FFFE,E000) tag at an Item position fails parsing (UnexpectedTag)
            Ps5_7_5_1_16ec1d => Checked("intrinsic-pixel-structure"),
            // Any length other than FFFFFFFFH is defined; misuse misframes and fails parsing
            Ps5_7_5_1_272077 => Checked("intrinsic-pixel-structure"),
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
            Ps5_7_5_2_5f93c9 => Checked("intrinsic-pixel-structure"),
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
            Ps5_A_4_1a92fb => Checked("intrinsic-pixel-structure"),
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
            Ps10_7_1_02d941 => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // UN on a Table 7.1-1 element flagged as VR mismatch; no UN-specific message
            Ps10_7_1_2f09d3 => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Bytes 128-131 must equal "DICM"
            Ps10_7_1_607718 => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Odd Value Lengths and wrong padding characters in File Meta flagged (file_meta.rs:124)
            Ps10_7_1_6b69cd => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Media Storage SOP Class must match SOP Class; full IOD checks run when meta declares WSI
            Ps10_7_1_7cdb3d => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // (0002,0102) without (0002,0100) flagged forbidden via Table 7.1-1 1C condition (engine.rs:168)
            Ps10_7_1_a0dff3 => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Preamble, DICM prefix, Group Length and Table 7.1-1 presence enforced
            Ps10_7_1_a2a04a => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // File Meta SOP Class/Instance UIDs compared to Data Set; DIMSE part out of scope. Also identity.rs:106
            Ps3_C_12_1_1_1_ac243d => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Raw group 0002 VRs checked against PS3.6; UN flagged as VR mismatch
            Ps5_6_2_2_78baa9 => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Every Optical Path ICC Profile validated against C.11.15.1.1 header constraints
            Ps3_C_8_12_5_1_4_b19588 => Checked("intrinsic-wsi-dicom-2026c-icc-profile"),
            // Planar Configuration 0 enforced for 3-sample WSI images; also C.8.12.4-1 enumeration
            Ps3_C_7_6_3_1_2_93fa52 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // YBR_ICT/YBR_RCT with any native Transfer Syntax rejected as incompatible
            Ps3_C_7_6_3_1_2_dbe575 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Bits Stored must equal Bits Allocated (8 or 16)
            Ps3_C_8_12_4_1_BitsStored_ed2b6b => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // High Bit must equal Bits Stored minus one
            Ps3_C_8_12_4_1_HighBit_e8f419 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI requires Bits Stored equal to Bits Allocated, stricter than not larger
            Ps5_8_1_1_14376b => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Bits Allocated restricted to 8 or 16; also WSI enumerated-values rule
            Ps5_8_1_1_65182e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI Pixel Representation must be 0; every bit field is then a valid unsigned integer
            Ps5_8_1_1_873394 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // High Bit equal to Bits Stored minus 1 enforced
            Ps5_8_1_1_b0091d => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI forbids Pixel Representation 1 (enforced), so 2's complement samples cannot conformantly occur
            Ps5_8_1_1_c9d618 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3, forbidden when 1; motion TS itself still unflagged
            Ps5_8_2_10_06b5ac => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI requires Pixel Representation 0 unconditionally; motion TS itself still unflagged
            Ps5_8_2_10_4bebe5 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3, forbidden when 1; motion TS itself still unflagged
            Ps5_8_2_11_d5254e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI requires Pixel Representation 0 unconditionally; motion TS itself still unflagged
            Ps5_8_2_11_f04f36 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_14_b2f50a => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // YBR_RCT for .201/.202/.203, YBR_ICT only .203, YBR_FULL_422 rejected; matches Table 8.2.14-1
            Ps5_8_2_14_f3a8d2 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_15_167cfb => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // RLE allows only MONOCHROME2/RGB of WSI values; YBR_* with RLE rejected; Planar/Bits rows enforced
            Ps5_8_2_2_8b7a45 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_3_4e64b0 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // JPEG-LS: only MONOCHROME2/RGB pass; YBR_* rejected for .80/.81, matching Table 8.2.3-1
            Ps5_8_2_3_a4a3ff => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3; forbidden when 1 (presence rule)
            Ps5_8_2_4_d71faf => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // YBR_RCT only .90/.91, YBR_ICT only .91, YBR_FULL_422 rejected; matches Table 8.2.4-1 for WSI values
            Ps5_8_2_4_fad88b => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3, forbidden when 1; motion TS itself still unflagged
            Ps5_8_2_5_0943da => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3, forbidden when 1; motion TS itself still unflagged
            Ps5_8_2_5_4aabac => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI requires Pixel Representation 0 unconditionally; motion TS itself still unflagged
            Ps5_8_2_5_ebb365 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI requires Pixel Representation 0 unconditionally; motion TS itself still unflagged
            Ps5_8_2_6_3b5e91 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3, forbidden when 1; motion TS itself still unflagged
            Ps5_8_2_6_797c38 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI requires Pixel Representation 0 unconditionally; motion TS itself still unflagged
            Ps5_8_2_7_3ffde9 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3, forbidden when 1; motion TS itself still unflagged
            Ps5_8_2_7_b89ecb => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // WSI requires Pixel Representation 0 unconditionally; motion TS itself still unflagged
            Ps5_8_2_8_6614d4 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Planar Configuration 0 enforced when Samples 3, forbidden when 1; motion TS itself still unflagged
            Ps5_8_2_8_7f832e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // YBR_RCT/ICT rejected outside J2K/HTJ2K syntaxes; YBR_PARTIAL_420 rejected by WSI enumerations
            Ps5_8_2_8a024f => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Long/URN Code Value forbidden when Code Value present; short code requires Code Value (conditions.rs:70)
            Ps3_8_1_1c95ed => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Long Code Value required for non-URN code without Code Value; short Long values force Code Value
            Ps3_8_1_597bf2 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Code Value condition forbids >16-char code in Code Value; SH length also checked in values.rs
            Ps3_8_1_5e1957 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // URN Code Value required/forbidden by URN detection; heuristic is urn: prefix or :// only
            Ps3_8_1_aeece1 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Extension Creator UID 1C required when Extension Flag is Y (conditions.rs:106)
            Ps3_8_7_2d698d => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // HL7 Structured Document Reference items include SOP Instance Reference Macro with Type 1 UIDs
            Ps3_C_12_1_1_6_0f08ec => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Engine selects the lowest Type across overlapping Module rows
            Ps3_C_1_2_3_a1fe07 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // PALETTE COLOR (Icon items) requires R/G/B descriptors and data via presence condition
            Ps3_C_7_6_3_1_2_00abdd => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Planar Configuration forbidden when Samples per Pixel is 1 (engine.rs:168)
            Ps3_C_7_6_3_1_3_4a62de => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Planar Configuration required when Samples per Pixel > 1 (engine.rs:139)
            Ps3_C_7_6_3_1_3_5d2797 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Type 1 attribute present without a Value flagged
            Ps5_7_4_1_1b4c29 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Missing Type 1 attributes from the generated tables flagged
            Ps5_7_4_1_200da1 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Type 1C present when its condition is false and no 'may be present otherwise' flagged
            Ps5_7_4_2_7dcac3 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Type 1C conditions evaluated in conditions.rs; Unknown conditions reported undetermined, not failed
            Ps5_7_4_2_d74db3 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Absent Type 2 attributes flagged
            Ps5_7_4_3_828c2e => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Missing Type 2 attributes flagged
            Ps5_7_4_3_aa6998 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Type 2C present when its condition is false flagged
            Ps5_7_4_4_31eee8 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Missing required Standard elements flagged regardless of private substitutes
            Ps5_7_8_f1a77f => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Bits Allocated, Bits Stored, High Bit are Type 1 in WSI module table; presence enforced
            Ps5_8_1_1_3229af => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Decidable part: Pixel Data (Type 1C) presence enforced; image data hidden elsewhere undecidable; overlays not in IOD
            Ps5_8_1_80d60f => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_10_3d6c2c => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_11_d69e04 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_6_d4f25d => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_7_391ee8 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Pixel Aspect Ratio forbidden when Pixel Spacing specified (conditions.rs:388); WSI always specifies it
            Ps5_8_2_8_302528 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // C.8.12.4-1 IDENTITY-only enumeration applies to every PI; monochrome rule also checks (instance.rs:258)
            Ps3_C_7_9_PresentationLUTShape_92f0e9 => Checked("intrinsic-wsi-dicom-2026c-iod-enumerated-values"),
            // Value 2 Enumerated PRIMARY; Frame Type same at tables.rs:2237
            Ps3_C_8_12_4_1_1_1befe3 => Checked("intrinsic-wsi-dicom-2026c-iod-enumerated-values"),
            // Value 1 Enumerated ORIGINAL/DERIVED; Frame Type same at tables.rs:2237
            Ps3_C_8_12_4_1_1_296245 => Checked("intrinsic-wsi-dicom-2026c-iod-enumerated-values"),
            // Multi-Resolution Pyramid Module forbidden unless Image Type Value 3 is VOLUME or THUMBNAIL
            Ps3_A_32_8_3_4f8395 => Checked("intrinsic-wsi-dicom-2026c-iod-modules"),
            // NotShared placement flags Frame Content in Shared Functional Groups
            Ps3_A_32_8_4_408ebf => Checked("intrinsic-wsi-dicom-2026c-iod-modules"),
            // SharedOnly placement flags Pixel Measures or Frame Type groups in Per-Frame Items
            Ps3_A_32_8_4_706768 => Checked("intrinsic-wsi-dicom-2026c-iod-modules"),
            // Enforced for standard WSI Functional Groups; private Functional Groups are not compared
            Ps3_C_7_6_16_1_1_e8853c => Checked("intrinsic-wsi-dicom-2026c-iod-modules"),
            // Item counts from Attribute Descriptions enforced; presence by Type at engine.rs:139
            Ps5_7_4_6_048def => Checked("intrinsic-wsi-dicom-2026c-iod-sequence-items"),
            // Only CS syntax decidable; non-listed values are legal Defined Term extensions (PS3.5)
            Ps3_8_4_79bf54 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Non-ASCII text without Specific Character Set flagged; default Latin-1 decode keeps bytes observable
            Ps5_6_1_2_1_91c5aa => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Only one trailing SPACE stripped; NUL or other padding fails repertoire/format checks; odd length flagged values.rs:116
            Ps5_6_1_2_3_51ae57 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // U+007F is a control character, rejected in text VRs; AE/CS/UR/numeric repertoires exclude it
            Ps5_6_1_2_3_a287c9 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // SO (0EH) and SI (0FH) are control characters rejected in all text VRs
            Ps5_6_1_2_5_2_0f91c6 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Only TAB, LF, FF, CR (text VRs) and ESC accepted; all other control characters rejected
            Ps5_6_1_3_62ad2e => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // DS exponent after E or e must be optionally signed digits
            Ps5_6_2_0c85c6 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // AE Value consisting solely of spaces rejected
            Ps5_6_2_10b562 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Trailing '.' yields an empty fraction, rejected by the digit check
            Ps5_6_2_14f3d9 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // DA must be eight digits forming a valid Gregorian date
            Ps5_6_2_176ec1 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Group length counted in Unicode scalar values, so combining characters count separately
            Ps5_6_2_1_2_e3d9df => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Characters outside the RFC 3986 set rejected; '%' must precede two hex digits
            Ps5_6_2_2653e9 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // TM fraction requires HHMMSS and 1 to 6 digits
            Ps5_6_2_2720d0 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Maximum lengths checked per Value; only the last Value has one padding character removed, matching the exception
            Ps5_6_2_3981c3 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // ESC and non-ASCII characters outside the RFC 3986 set rejected in UR
            Ps5_6_2_3_019dcb => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Percent-escape syntax checked; percent-decoded bytes not verified as valid UTF-8
            Ps5_6_2_3_f3fccc => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Raw non-ASCII hosts rejected; percent-encoded non-ASCII host names (should be IDNA) not rejected
            Ps5_6_2_3_fb1c60 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // DT fractional second requires full seconds and 1 to 6 digits
            Ps5_6_2_3ed511 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // IS restricted to optional sign and digits after trimming spaces
            Ps5_6_2_4f76de => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // More than three component groups rejected
            Ps5_6_2_57497d => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // DT month and day validated against the Gregorian calendar, including leap years
            Ps5_6_2_5b8243 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Odd UI length flagged values.rs:116; single NUL stripped, other padding fails UID syntax
            Ps5_6_2_5e4dd5 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // DT must start with four year digits
            Ps5_6_2_75c3d6 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Offset -0000 rejected, so UTC must be encoded +0000
            Ps5_6_2_768fdd => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // AS must be three digits followed by D, W, M or Y
            Ps5_6_2_7e81a8 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // More than five components per component group rejected
            Ps5_6_2_80b965 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Odd length flagged values.rs:116; one NUL stripped, SPACE or extra NUL padding fails UID syntax
            Ps5_6_2_82cca4 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // IS outside -2^31..2^31-1 rejected
            Ps5_6_2_b9e2da => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Trailing '.' gives an empty fraction, rejected
            Ps5_6_2_c534bd => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Control characters other than ESC rejected in LO
            Ps5_6_2_daeeb8 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Odd OB length flagged; value of the pad byte (00H) not verified
            Ps5_6_2_e0c655 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Hour limited to 00-23, so 2400 is rejected in TM and DT
            Ps5_6_2_e33c94 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Control characters other than ESC rejected in SH
            Ps5_6_2_f0355c => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Odd Value Length flagged; non-SPACE padding (e.g. NUL) fails repertoire/format checks after single-SPACE strip
            Ps5_6_2_f340d0 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // DT offset suffix must be a sign followed by four digits
            Ps5_6_2_f7c84a => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // DS mantissa restricted to digits, optional sign and one decimal point; embedded spaces rejected
            Ps5_6_2_f892eb => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Wrong delimiters fail format in restricted VRs (CS, DS, IS, UI, DA); free-text VRs undecidable
            Ps5_6_4_0a3e14 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Odd Value Length of the whole Value Field flagged
            Ps5_6_4_3472c2 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Binary Value Length must be a multiple of the element size; delimiters break this or VM
            Ps5_6_4_7a0ae1 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Lead-in to 6.4 delimiting; parser splits strings on 5CH and binary Values by size
            Ps5_6_4_8bab9c => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Padding removed only from the last Value; padding on earlier Values fails restricted formats
            Ps5_6_4_f2e91a => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // NUL stripped only from the last UID; NUL in earlier UIDs fails UID syntax
            Ps5_6_4_fde9f8 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Values split on 5CH and formats checked per Value; wrong delimiters in free text undecidable
            Ps5_7_1_1_88ebbc => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Value formats checked per VR; VM checked by iod-vr-vm at values.rs:85
            Ps5_7_4_1_01939a => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // VR/VM validity checked; whether a known Value was omitted is undecidable
            Ps5_7_4_3_0c7254 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Private Values format-checked by encoded VR in explicit VR; implicit-VR private data is UN
            Ps5_7_8_2_c93d2d => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Odd Value Length of native Pixel Data reported; fragment lengths not covered here
            Ps5_8_1_1_649518 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Only ASCII digits and periods accepted
            Ps5_9_1_5dab00 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Separators limited to '.'; any other character rejected
            Ps5_9_1_98ae51 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Odd UI length flagged (values.rs:116); one trailing NUL removed before the syntax check
            Ps5_9_1_9be579 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Empty components (leading, trailing or doubled periods) and non-digits rejected
            Ps5_9_1_e2c578 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Components with a leading zero rejected unless the single digit 0
            Ps5_9_1_e86dd2 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // UID over 64 characters rejected; a padded 65-byte Value is also flagged as odd length
            Ps5_9_1_fcd445 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Decidable part: OW even length enforced; LUT entry byte order undecidable
            Ps5_A_1_04740e => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Decidable part: OW implies even Value Length, enforced; sample byte order undecidable (Bits Stored = Allocated)
            Ps5_A_1_0482a6 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Umbrella for per-VR encoding under Implicit VR; even-length rule enforced; items classified separately
            Ps5_A_1_0a4a49 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Umbrella for OB/OL/OV/OW; Value Length multiples enforced (values.rs:116-130); items separate
            Ps5_A_1_8481bf => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Standard Extended Modality/VOI LUT only; even length enforced; byte order undecidable
            Ps5_A_1_86c594 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // Overlay only as Standard Extended attribute; OW even length enforced; byte order undecidable
            Ps5_A_1_ea8197 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // PS3.6 VM 2 enforced; anatomical meaning of the two directions is undecidable
            Ps3_C_7_6_1_1_1_b53257 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Parser splits on 5CH, so stray backslash becomes extra Value; VM-1 elements flagged; UR rejects values.rs:574
            Ps5_6_1_2_3_5f1078 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Explicit-VR Private Creators (gggg,0010-00FF) must be LO; UN flagged at values.rs:73
            Ps5_6_2_2_b4d2eb => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // UN on standard tags flagged in explicit VR; >65534 exemption missed (false positive); encoding history undecidable
            Ps5_6_2_2_b96c3e => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // LT parsed as one string; a multi-valued LT cannot be encoded, VM 1 by construction
            Ps5_6_2_5e8cdb => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // SH split on 5CH by parser; stray backslash appears as extra Value, flagged for VM-1 elements
            Ps5_6_2_7c3538 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // UR parsed as a single string, VM 1 by construction; backslash rejected by uri() values.rs:574
            Ps5_6_2_89a67a => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // These VRs parse as one Value (string or byte stream); VM 1 holds by construction
            Ps5_6_4_29bb04 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Explicit VR of standard elements compared with PS3.6 VR alternatives
            Ps5_7_1_1_c8d4ce => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Explicit VR checked against PS3.6 alternatives; implicit VR takes the dictionary VR
            Ps5_7_1_1_ca2b1d => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Explicit VR SQ mismatch flagged; parser re-types undefined-length non-SQ elements as SQ (read.rs:573)
            Ps5_7_5_251684 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // A Sequence element holds one Sequence by construction; VM 1 always
            Ps5_7_5_379e6d => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Retired entries' VR/VM checked; formats use current rules, older-version specifics unchecked
            Ps5_7_7_b39a73 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Walk recurses into private Sequences; standard tags' explicit VR compared with PS3.6
            Ps5_7_8_2_fc38c1 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Even-group tags absent from the PS3.6 registry flagged as unregistered
            Ps5_7_8_992cc6 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_11_75f6c4 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_11_a86261 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for OB/OW per tag under JPIP HTJ2K TS; items classified separately
            Ps5_A_11_ef1304 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_2_45d343 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for OB/OL/OV/OW per tag; PS3.6 VR enforced; items classified separately
            Ps5_A_2_4fe8ab => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR US/OW enforced vs PS3.6; only via Standard Extended Modality/VOI LUT; byte order undecidable
            Ps5_A_2_54edc7 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_2_7687c9 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR OB or OW enforced vs PS3.6 for Pixel Data
            Ps5_A_2_7e2850 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR OW enforced vs PS3.6; LUT entry byte order not decidable
            Ps5_A_2_972477 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR OW enforced vs PS3.6; LUT entry byte order not decidable
            Ps5_A_4_3b7a41 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for OB/OW per tag under encapsulated TS; items classified separately
            Ps5_A_4_42ecb9 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_4_45b4b3 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Nested native Pixel Data VR OB or OW enforced vs PS3.6
            Ps5_A_4_5af91b => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_4_5cc995 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR US/OW enforced vs PS3.6; only via Standard Extended Modality/VOI LUT; byte order undecidable
            Ps5_A_4_995a1f => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // VR OB/OW enforced vs PS3.6; overlay only as Standard Extended attribute; byte order undecidable
            Ps5_A_6_871dd9 => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for per-VR encoding; VR enforced generically vs PS3.6; items classified separately
            Ps5_A_6_a6e1bb => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Umbrella for OB/OW per tag under JPIP TS; items classified separately
            Ps5_A_6_c1784c => Checked("intrinsic-wsi-dicom-2026c-iod-vr-vm"),
            // Equal method/ratio VM enforced when flag is 01; per-step correspondence otherwise undecidable
            Ps3_C_7_6_1_1_5_1_f408e9 => Checked("intrinsic-wsi-dicom-2026c-lossy-history"),
            // Equal ratio/method VM enforced when flag is 01; per-step correspondence otherwise undecidable
            Ps3_C_7_6_1_1_5_2_a81e8a => Checked("intrinsic-wsi-dicom-2026c-lossy-history"),
            // Lossy Transfer Syntax with 00 flagged; earlier lossy history under lossless syntax is undetectable
            Ps3_C_7_6_1_1_5_dfc992 => Checked("intrinsic-wsi-dicom-2026c-lossy-history"),
            // UID syntax (identity.rs:208) and in-set UID collisions checked; global uniqueness undecidable
            Ps3_C_7_6_22_1_2_464a33 => Checked("intrinsic-wsi-dicom-2026c-specimen-uid-set"),
            // Image Type Value 3 present and non-empty in every profile
            Ps3_C_7_6_1_1_2_ef6680 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Image Flavor non-empty; its Defined Terms are extensible
            Ps3_C_8_12_4_1_1_f4d4e2 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Derived Pixel non-empty; its Defined Terms are extensible
            Ps3_C_8_12_4_1_1_b18b99 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Image Type and every Frame Type have exactly four Values
            Ps3_C_8_12_4_1_1_75b5c5 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Imaged Volume Depth of 0 fails
            Ps3_C_8_12_4_1_2_3e7c7c => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Specimen Label in Image is YES for OVERVIEW and LABEL
            Ps3_C_8_12_4_1_SpecimenLabelInImage_fd0f41 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Specimen Label in Image is NO for THUMBNAIL and VOLUME
            Ps3_C_8_12_4_1_SpecimenLabelInImage_0b6709 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Tiles Overlap other than NONE with TILED_FULL fails
            Ps3_C_8_12_14_1_TilesOverlap_0699b8 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Duplicate Optical Path Identifiers fail
            Ps3_C_8_12_5_1_OpticalPathIdentifier_d84211 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Every Optical Path Identification of a Frame resolves to an Optical Path Sequence Item
            Ps3_C_8_12_5_1_1_d6f6c9 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Position Reference Indicator other than SLIDE_CORNER fails
            Ps3_C_7_4_1_1_2_71f60f => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Lossy Compression derivation (DCM 113040) requires Image Type DERIVED
            Ps3_C_7_6_1_1_5_808c0a => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // A derived image naming its own SOP Instance UID as a source fails
            Ps3_C_7_6_1_1_2_c26085 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Pixel Spacing positive; zero only for a single row or column
            Ps3_10_7_1_3_758737 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Negative Spacing Between Slices fails
            Ps3_C_7_6_16_2_SpacingBetweenSlices_7cbf0e => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Frame positions keep one residue modulo Columns and Rows
            Ps3_A_32_8_4_1_2_2c5e3d => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Column/Row Position is a multiple of Columns/Rows plus a constant
            Ps3_A_32_8_4_1_2_922f97 => Checked("intrinsic-wsi-dicom-2026c-wsi-image"),
            // Frames lacking the indexed Attribute share one index of their own
            Ps3_C_7_6_17_1_97159c => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // A pointer naming an Attribute no Frame has fails
            Ps3_C_7_6_17_1_da4366 => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // Pointer to Frame Content Sequence or Dimension Index Values fails
            Ps3_C_7_6_17_1_849d42 => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // Functional Group Pointer with a pointer that names a Functional Group Sequence fails
            Ps3_C_7_6_17_1_83308f => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // Dimension Index Private Creator matches the reserving block
            Ps3_C_7_6_17_1_0f809b => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // Functional Group Private Creator matches the reserving block
            Ps3_C_7_6_17_1_39ce9a => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // Same index, nominally same Value (numbers within 1e-6 relative)
            Ps3_C_7_6_17_1_2ec4fa => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // Dimension Index Values VM equals the Dimension Index Sequence Item count
            Ps3_C_7_6_16_3_DimensionIndexValues_e56fe8 => Checked("intrinsic-wsi-dicom-2026c-dimension-order"),
            // Two Frame of Reference UIDs in one Series fail
            Ps3_C_7_4_1_1_1_ea9bd3 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // A slide-related Frame of Reference UID shared across slides (Container Identifier) fails
            Ps3_C_7_4_1_c35eb2 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Images sharing a Frame of Reference UID are of one slide (Container Identifier)
            Ps3_C_7_4_1_1_1_eaeaff => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // One UID in two identifying roles fails within the set; global uniqueness undecidable
            Ps5_9_ad5517 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // One UID in two identifying roles fails within the set; global uniqueness undecidable
            Ps5_9_5a69d2 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Instance Number shared outside one Concatenation within a Series fails
            Ps3_C_7_6_16_1_InstanceNumber_9d6547 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Concatenation members' Dimension Index Sequences compared by digest
            Ps3_C_7_6_16_2_2_4_241add => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Concatenation members' Shared Functional Groups compared by digest
            Ps3_C_7_6_16_2_2_4_3fcf86 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Concatenation members' Per-Frame Item structure compared
            Ps3_C_7_6_16_2_2_4_b6a910 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Concatenation members agree on all Attributes outside the per-instance ones
            Ps3_C_7_6_16_2_2_4_c53142 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // SOP Instance UID of Concatenation Source agrees across members
            Ps3_C_7_6_16_1_SOPInstanceUIDOfConcatenationSource_487ffc => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // In-concatenation Numbers are 1..n in Concatenation Frame Offset Number order
            Ps3_C_7_6_16_1_InConcatenationNumber_f490dd => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // In-concatenation Total Number of 1 or less fails
            Ps3_C_7_6_16_1_InConcatenationTotalNumber_4fdb6d => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // A Dimension Organization UID indexing different Attributes across Instances fails
            Ps3_C_7_6_17_2_4ebae3 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // A localizer in the set with another Frame of Reference UID fails
            Ps3_C_7_6_16_2_5_1_17ac5a => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // One Specimen Identifier and issuer naming two Specimen UIDs in a Study fails
            Ps3_C_7_6_22_1_2_19f6e2 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Values 1 and 2 by the enumerations; Values 3 and 4 and the count by the wsi-image rule
            Ps3_C_7_6_1_1_2_a9e514 => Checked("intrinsic-wsi-dicom-2026c-iod-enumerated-values"),
            // An empty Value among several of an Enumerated or Defined Term Attribute fails
            Ps5_6_3_ed2fcf => Checked("intrinsic-wsi-dicom-2026c-iod-enumerated-values"),
            // A Type 3 Sequence present with zero Items fails
            Ps5_7_4_5_4ef00e => Checked("intrinsic-wsi-dicom-2026c-iod-sequence-items"),
            // A private Selector Attribute or Sequence Pointer Value other than (gggg,00xx) fails
            Ps3_10_17_1_2_e5ece3 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // DCMR Context Identifier other than digits without leading zeros fails
            Ps3_8_6_2191da => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // A Data Elements Signed Tag absent from the MAC Parameters Sequence's Data Set fails
            Ps3_C_12_1_1_3_1_1_5b9eec => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Data Elements Signed not in ascending Tag order fails
            Ps3_C_12_1_1_3_1_1_13525c => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Listing an excluded Tag (lengths, group < 0008, UN, FFFA, MAC Parameters, padding, delimiters) fails
            Ps3_C_12_1_1_3_1_1_eb25f8 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // HL7 Structured Document Referenced SOP Class UID outside the HL7 OID root fails
            Ps3_C_12_1_1_6_b6eb69 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // A nonconforming top-level Attribute missing or non-empty in Modified Attributes fails
            Ps3_C_12_1_1_9_1_09372b => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Nonconforming Modified Attributes Item without Selector Attribute fails
            Ps3_C_12_1_1_9_2_52e512 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Source Instance Sequence naming a PS3.6 Image Storage SOP Class fails
            Ps3_C_12_4_1_2_a52a75 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Patient photo references other than VL Photographic or Secondary Capture fail
            Ps3_C_2_2_1_1_70c5e1 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Subject Relative Position Values below 1 fail; holder order is not encoded
            Ps3_C_7_1_4_1_1_1_a03c8a => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // One group Patient ID with the same members in two arrangements fails
            Ps3_C_7_1_4_1_1_1_093320 => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Date/Time of Last Calibration pairs not ascending fail
            Ps3_C_7_5_1_1_1_697183 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Date and Time of Last Calibration with different Value counts fail
            Ps3_C_7_5_1_1_1_6f6b31 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Biped Patient Orientation Values parse as A/P/R/L/H/F
            Ps3_C_7_6_1_1_1_b78dcd => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Quadruped Patient Orientation Values parse as the C.7.6.1.1.1 abbreviations
            Ps3_C_7_6_1_1_1_d49ff3 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Each non-empty Patient Orientation Value has at least one abbreviation
            Ps3_C_7_6_1_1_1_1e27ca => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // At most three abbreviations per Patient Orientation Value
            Ps3_C_7_6_1_1_1_2a7108 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Preparation steps' DateTime of processing (DCM 111702) not ascending fails; undated steps unordered
            Ps3_C_7_6_22_1_3_372144 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Referenced Frame Number below 1 fails
            Ps3_10_3b_ReferencedFrameNumber_f9c771 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Referenced Frame Number below 1 fails
            Ps3_10_2_ReferencedFrameNumber_587608 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Referenced Frame Number below 1 fails
            Ps3_10_3_ReferencedFrameNumber_174e7c => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_3_ConsultingPhysicianIdentificationSequence_5a5065 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_3_PhysiciansOfRecordIdentificationSequence_19326f => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_3_PhysiciansReadingStudyIdentificationSequence_d559d9 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_5a_PerformingPhysicianIdentificationSequence_0e6615 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Item count against name Values; the Items carry no name to check the order
            Ps3_C_7_5a_OperatorIdentificationSequence_830798 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Contributing Equipment Operator Identification Items counted against Operators' Name, single Item included
            Ps3_C_12_1_OperatorIdentificationSequence_a0dcbe => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // A caret-delimited Code Meaning is checked as a PN Value; caret-free names are LO
            Ps3_10_1_PersonIdentificationCodeSequence_7d0584 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Numeric Value with more than one Value in a Measured Value Item fails
            Ps3_10_2_NumericValue_728e74 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Floating Point Value VM differs from Numeric Value
            Ps3_10_2_FloatingPointValue_796684 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Rational Numerator Value VM differs from Numeric Value
            Ps3_10_2_RationalNumeratorValue_f9fd94 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Rational Denominator Value VM differs from Numeric Value
            Ps3_10_2_RationalDenominatorValue_cc4bf1 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Image Laterality against Laterality and SCT/SRT laterality modifier codes
            Ps3_C_7_9_ImageLaterality_0d7859 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Color Space against a matrix/TRC ICC Profile's primaries; table-based profiles undecidable
            Ps3_C_8_12_5_1_ColorSpace_36ee23 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Selector Value Number other than 1 for a PS3.6 VM 1 Attribute fails
            Ps3_10_20_SelectorValueNumber_350d86 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Each public Selector Sequence Pointer Value names a Sequence, one per nesting level
            Ps3_10_20_SelectorSequencePointer_baa08d => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Selector Sequence Pointer Private Creator VM equals the pointer's
            Ps3_10_20_SelectorSequencePointerPrivateCreator_379163 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // A Private Creator for a public Selector Sequence Pointer Value fails
            Ps3_10_20_SelectorSequencePointerPrivateCreator_46d4e2 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // Selector Sequence Pointer Items VM equals the pointer's
            Ps3_10_20_SelectorSequencePointerItems_fcd3e1 => Checked("intrinsic-wsi-dicom-2026c-attribute-relations"),
            // An extraction from a set member with a Frame Extraction Sequence carries its Items plus one
            Ps3_C_12_9_FrameExtractionSequence_74490f => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // Presentation Address dicom:<host>:<port> form
            Ps10_7_1_1_1_5ec65a => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Presentation Address absolute http URL with a host
            Ps10_7_1_1_1_1add3e => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Presentation Address with scheme https fails
            Ps10_7_1_1_1_3653a7 => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_4ea599 => Checked("intrinsic-wsi-dicom-2026c-icc-profile"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_43d1c6 => Checked("intrinsic-wsi-dicom-2026c-icc-profile"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_6cb654 => Checked("intrinsic-wsi-dicom-2026c-icc-profile"),
            // Icon Image ICC Profiles validated like Optical Path ICC Profiles
            Ps3_C_11_15_1_1_efa57f => Checked("intrinsic-wsi-dicom-2026c-icc-profile"),
            // Pixel Padding Value and Range Limit lie in the stored range, in order
            Ps3_C_7_5_1_1_2_8d8819 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Pixel Padding Value and Range Limit lie in the stored range, in order
            Ps3_C_7_5_1_1_2_62cd75 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_8478b8 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_b7c7c0 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_c2da50 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_c5156c => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_cc071c => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_6776f0 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_6_1_1_6_aeb525 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_1_1_6_502a69 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_a1e82c => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_c598ef => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_1747e4 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_98671f => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_ad0ea4 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Palette icons need 8-bit samples and consistent Palette Color tables
            Ps3_C_7_6_3_1_5_ed0814 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Optical Path palettes are 16-bit single-sample tables
            Ps3_C_7_6_3_1_5_6b3ce7 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Optical Path palettes are 16-bit single-sample tables
            Ps3_C_7_6_3_1_6_e94d80 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Pixel Aspect Ratio Values are positive
            Ps3_C_7_6_3_1_7_fbe0d7 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Extended Offset Table needs an empty Basic Offset Table and whole Frames
            Ps3_C_7_6_3_1_8_a81faf => Checked("intrinsic-pixel-structure"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_84cfee => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Single-sample tables checked; whether segmented data is desirable is the producer's choice
            Ps3_C_7_9_2_a92a46 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_1_dfe589 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_2_c7a7b2 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_3_a8252a => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Segmented palettes expand to the declared number of entries
            Ps3_C_7_9_2_3_01ce43 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_11c_BitsAllocated_f6fab6 => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Icon Image Sequence Items follow the icon restrictions
            Ps3_C_7_11c_HighBit_41bada => Checked("intrinsic-wsi-dicom-2026c-pixel-attributes"),
            // Bytes after the last Data Element, or a Value past the end, fail from the encoded bytes
            Ps5_7_1_2_85dc52 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Top-level Pixel Data without a Sequence Delimitation Item fails from the encoded bytes
            Ps5_7_1_2_d38d67 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Top-level Pixel Data without a Sequence Delimitation Item fails from the encoded bytes
            Ps5_7_5_2_e7bdad => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Native length checked; consistency with codestream headers is the codestream family
            Ps5_8_1_8ae787 => Checked("intrinsic-pixel-structure"),
            // Cell count fixes the Native length; bit packing within a cell is not encoded
            Ps5_8_1_1_993326 => Checked("intrinsic-pixel-structure"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_2b593e => Checked("intrinsic-pixel-structure"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_b26202 => Checked("intrinsic-pixel-structure"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_018c03 => Checked("intrinsic-pixel-structure"),
            // Native Pixel Data holds exactly the Frames, padded to even length
            Ps5_8_1_1_5123f3 => Checked("intrinsic-pixel-structure"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_5_2ae16d => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_5_01fdd1 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_5_37899b => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_5_853226 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_5_da7e9c => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_5_06062e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntax fails image-metadata; Cine timing and codec level limits are outside the IOD
            Ps5_8_2_5_7430f5 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_5_8536a7 => Checked("intrinsic-pixel-structure"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_5_f85091 => Checked("intrinsic-pixel-structure"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_6_73ab6e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_6_22a85e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_6_d39cdd => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_6_4a24d0 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_6_f573f7 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_6_862741 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_6_ae499d => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntax fails image-metadata; Cine timing and codec level limits are outside the IOD
            Ps5_8_2_6_fdcabf => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_6_a4b86d => Checked("intrinsic-pixel-structure"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_6_6b3ca2 => Checked("intrinsic-pixel-structure"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_7_e55180 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_7_020679 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_7_53db66 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_7_8993db => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_7_bb3858 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntax fails image-metadata; Cine timing and codec level limits are outside the IOD
            Ps5_8_2_7_c328ad => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_7_7c3a60 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_7_9565ae => Checked("intrinsic-pixel-structure"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_8_1d080a => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_8_6f91d1 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_8_8fdb21 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_8_fedd49 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_8_1973b2 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntax fails image-metadata; Cine timing and codec level limits are outside the IOD
            Ps5_8_2_8_7718ba => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_8_bc3138 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Video streams follow their Fragment and offset table rules, for every object
            Ps5_8_2_8_e51cd9 => Checked("intrinsic-pixel-structure"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_9_ff677c => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_10_b7f63b => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_10_226719 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_10_bd5c63 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_10_58520f => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_10_d25e20 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntax fails image-metadata; Cine timing and codec level limits are outside the IOD
            Ps5_8_2_10_84b09e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_11_e52f4e => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_11_514e75 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_11_9c3b42 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_11_649e03 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntaxes fail image-metadata in VL WSI (C.8.12.4.1.5), naming the departure
            Ps5_8_2_11_385c34 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Motion syntax fails image-metadata; Cine timing and codec level limits are outside the IOD
            Ps5_8_2_11_f9f4eb => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_1_3fcdef => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_1_777bab => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_1_2efc4a => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_2_2fd46a => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_2_6239c9 => Checked("intrinsic-pixel-structure"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_A_4_61f90c => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_4_238b32 => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_33a347 => Checked("intrinsic-pixel-structure"),
            // Fragment Items have an even length of at least two bytes, from the encoded bytes
            Ps5_A_4_fd123f => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // A missing Basic Offset Table Item misaligns the Fragments and fails
            Ps5_A_4_9063da => Checked("intrinsic-pixel-structure"),
            // Basic Offset Table validated whenever present; empty beside an Extended Offset Table
            Ps5_A_4_82c271 => Checked("intrinsic-pixel-structure"),
            // Encapsulated Pixel Data is OB and ends with a zero-length Sequence Delimitation Item
            Ps5_A_4_f5f245 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // One Fragment per Frame checked; the RLE header in each Fragment is the codestream family
            Ps5_A_4_2_c5d6d5 => Checked("intrinsic-pixel-structure"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_2_6687ec => Checked("intrinsic-pixel-structure"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_4_72c08f => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_3b48ff => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_dd388a => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_f1cc7f => Checked("intrinsic-pixel-structure"),
            // Encapsulated Uncompressed Fragments hold one Native-encoded Frame each
            Ps5_A_4_11_acf161 => Checked("intrinsic-pixel-structure"),
            // Encapsulated Pixel Data is OB and ends with a zero-length Sequence Delimitation Item
            Ps5_A_4_11_bd0aed => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_12_a741e8 => Checked("intrinsic-pixel-structure"),
            // One Fragment per Frame checked; separate RFC 1951 streams would need inflating
            Ps5_A_4_13_7626f4 => Checked("intrinsic-pixel-structure"),
            // One-Fragment-per-Frame syntaxes reject Frames split across Fragments
            Ps5_A_4_13_14f644 => Checked("intrinsic-pixel-structure"),
            // Deflated Data Sets inflate; undeflated bytes declaring the syntax fail parsing
            Ps5_A_5_97bfdd => Checked("intrinsic-pixel-structure"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_A_6_dcb6d6 => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_6_e0a5fd => Checked("intrinsic-pixel-structure"),
            // JPIP Referenced syntaxes admit MONOCHROME1/2, YBR_ICT and YBR_RCT only
            Ps5_A_6_71e374 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_A_11_3cba9b => Checked("intrinsic-pixel-structure"),
            // A Data Set not encoded in the declared syntax misframes and fails; trailing bytes fail
            Ps5_A_11_ce7500 => Checked("intrinsic-pixel-structure"),
            // Pixel Data forbidden and Pixel Data Provider URL required under all four JPIP syntaxes
            Ps5_A_11_3b11ba => Checked("intrinsic-pixel-structure"),
            // JPIP Referenced syntaxes admit MONOCHROME1/2, YBR_ICT and YBR_RCT only
            Ps5_A_11_5f6235 => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Pixel Data Provider URL required with .4.204/.4.205 too (PS3.5 A.11, A.12)
            Ps5_A_11_47ddc0 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // Pixel Data Provider URL required with .4.204/.4.205 too (PS3.5 A.11, A.12)
            Ps5_A_12_6b70c4 => Checked("intrinsic-wsi-dicom-2026c-iod-attribute-presence"),
            // JPEG Extended admits only its listed Photometric Interpretations
            Ps5_8_2_1_fe09df => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // JPEG XL Photometric Interpretation follows Table 8.2.15-1
            Ps5_8_2_15_c4fa2a => Checked("intrinsic-wsi-dicom-2026c-image-metadata"),
            // Pixel Data, Float and Double Float Pixel Data follow the Transfer Syntax
            Ps5_8_4_1_dc90bc => Checked("intrinsic-pixel-structure"),
            // JPIP Referenced syntaxes, all four, are not used in PS3.10 files
            Ps5_10_8_d4693e => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // Pixel Data forbidden and Pixel Data Provider URL required under all four JPIP syntaxes
            Ps5_A_6_be37ad => Checked("intrinsic-pixel-structure"),
            // PS3.10 files declaring .4.94 or .4.95 fail file-meta; the stream type is on the server
            Ps5_A_6_3ef132 => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // PS3.10 files declaring .4.94 or .4.95 fail file-meta; the stream type is on the server
            Ps5_A_7_e5c0fa => Checked("intrinsic-wsi-dicom-2026c-file-meta"),
            // All components share size precision and sign
            Ps3_C_7_6_3_1_1_c17c68 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Every frame is checked against the attributes that define it
            Ps3_C_7_6_6_1_1_899f77 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Color declarations agree with photometric interpretation
            Ps3_C_8_12_4_1_5_2d19f4 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_BitsAllocated_69f390 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_BitsStored_d52069 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_HighBit_a62636 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // All components share size precision and sign
            Ps3_C_7_11c_PixelRepresentation_178735 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Frame header agrees with rows columns samples and bits stored
            Ps5_8_2_1_803014 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Embedded ICC profiles equal the ICC profile of the frame
            Ps5_8_2_1_d49433 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Header and segments agree with the image pixel attributes
            Ps5_8_2_2_c961f8 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Frame header agrees with the image pixel attributes
            Ps5_8_2_3_c509e8 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Image and tile size agrees with the image pixel attributes
            Ps5_8_2_4_47cc48 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Multiple component transformation matches photometric interpretation
            Ps5_8_2_4_478fac => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Multiple component transformation matches photometric interpretation
            Ps5_8_2_4_045661 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Multiple component transformation matches photometric interpretation
            Ps5_8_2_4_f23101 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Image and tile size agrees with the image pixel attributes
            Ps5_8_2_14_b9d8c7 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // HTJ2K multiple component transformation matches photometric interpretation
            Ps5_8_2_14_557a1e => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // HTJ2K multiple component transformation matches photometric interpretation
            Ps5_8_2_14_8c86ea => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // HTJ2K multiple component transformation matches photometric interpretation
            Ps5_8_2_14_6f1193 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Headers agree with the image pixel attributes
            Ps5_8_2_15_091321 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // HTJ2K RPCL codestreams progress by resolution with tile lengths
            Ps5_10_18_1_b86460 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // HTJ2K RPCL codestreams progress by resolution with tile lengths
            Ps5_10_18_1_a950cd => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // HTJ2K RPCL codestreams progress by resolution with tile lengths
            Ps5_10_18_1_080cc2 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Coding process is the one its transfer syntax identifies
            Ps5_A_4_1_1e2871 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_1_55978a => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_1_b3fb04 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Frames carry their table specifications
            Ps5_A_4_1_c5999c => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Lossless transfer syntax admits only near zero
            Ps5_A_4_3_e86721 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_3_af4cd4 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Each frame is one complete separately encoded stream
            Ps5_A_4_3_116d52 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Frames carry all parameter specifications
            Ps5_A_4_3_79e44a => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Lossless transfer syntaxes use the reversible path
            Ps5_A_4_4_7d7f64 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Part 1 transfer syntaxes admit no HTJ2K or Part 2 coding
            Ps5_A_4_4_5e53e6 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // HTJ2K transfer syntaxes identify HTJ2K codestreams
            Ps5_A_4_4_259d88 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Each frame is one complete separately encoded codestream
            Ps5_A_4_4_e0caa8 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Each frame is one complete separately encoded codestream
            Ps5_A_4_4_480185 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Frames hold a JPEG 2000 codestream
            Ps5_A_4_4_05e878 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // JP2 file format header is not included
            Ps5_A_4_4_044fb8 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Part 2 transfer syntaxes admit only annex j extensions
            Ps5_A_4_4_ab7220 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // A fragment holds one component collection
            Ps5_A_4_4_6e9c4b => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Coding mode is the one its transfer syntax identifies
            Ps5_A_4_12_814b6e => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Segments are an even number of bytes padded with zero
            Ps5_G_3_3774f3 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Three byte repeats are replicate runs
            Ps5_G_3_1_961ac0 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Runs do not cross row boundaries
            Ps5_G_3_1_5f6b0b => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Unused segment offsets are zero
            Ps5_G_5_92c185 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // Stream kind is the one its transfer syntax identifies
            Ps10_7_2_770df5 => Checked("intrinsic-wsi-dicom-2026c-codestream"),
            // A character set is named once
            Ps3_C_12_1_1_2_fd6abd => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Unicode and GB character sets are the only value
            Ps3_C_12_1_1_2_fd82cf => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // UTF-8 is minimal and valid
            Ps3_C_12_1_1_2_a2828b => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_1_7_1_45ff05 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_1_7_1_30c8d1 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_1_7_2_98e21e => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Timezone offset from UTC is encoded as signed hours and minutes
            Ps3_C_12_1_1_8_5d989e => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Timezone offset from UTC is encoded as signed hours and minutes
            Ps3_C_12_1_1_8_589a40 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Items reserve their own blocks
            Ps3_C_12_1_1_9_1_8190ee => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // A Series has a single Frame of Reference UID
            Ps3_C_7_4_1_1_1_ca125c => Checked("intrinsic-wsi-dicom-2026c-series-set"),
            // An empty Image Type Value 1 or 2 is not an Enumerated Value
            Ps3_C_7_6_1_1_2_e953ef => Checked("intrinsic-wsi-dicom-2026c-iod-enumerated-values"),
            // An empty Image Type Value 1 or 2 is not an Enumerated Value
            Ps3_C_7_6_1_1_2_d4e4dd => Checked("intrinsic-wsi-dicom-2026c-iod-enumerated-values"),
            // Signature and encryption attributes follow their encoding rules
            Ps3_C_12_1_EncryptedContentTransferSyntaxUID_6d1938 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_NonidentifyingPrivateElements_1b7b56 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private data element characteristics encode counts and lists
            Ps3_C_12_1_IdentifyingPrivateElements_53d273 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Signature and encryption attributes follow their encoding rules
            Ps3_C_12_6_MACCalculationTransferSyntaxUID_c73a1b => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Signature and encryption attributes follow their encoding rules
            Ps3_C_12_6_DigitalSignatureDateTime_c19323 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_1_c5c47d => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The C1 set is not used
            Ps5_6_1_1_676368 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Text beyond the default repertoire needs specific character set
            Ps5_6_1_2_2_aaf21c => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_3_2a236f => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_3_93d068 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Code extension needs a multi valued specific character set
            Ps5_6_1_2_4_213779 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_d4ec20 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_1_1c6da1 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_1_54e495 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_2_5_2_396321 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_5_3_4b3ce3 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_5_3_68512a => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_1_2_5_3_1fdc6c => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Code extension needs a multi valued specific character set
            Ps5_6_1_2_5_4_3ea86a => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_1_3_c65780 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Text lines end in CR LF
            Ps5_6_1_3_b90cab => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Padding is space for text null for UIDs and OB
            Ps5_6_2_93b003 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences are not counted in value lengths
            Ps5_6_2_76a6fd => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Backslash bytes inside multi byte characters are not present
            Ps5_6_2_8c8195 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // A veterinary patient name has two components
            Ps5_6_2_ff8eab => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The alphabetic component group uses value 1 without escapes
            Ps5_6_2_1_2_c9f7ed => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The alphabetic component group uses value 1 without escapes
            Ps5_6_2_1_2_951141 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The alphabetic component group uses value 1 without escapes
            Ps5_6_2_1_2_ca8329 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Escape sequences designate declared sets of the tables
            Ps5_6_2_1_2_12c186 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_2_1_2_8d573f => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_2_1_2_ed7e68 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // The value 1 set is active at value line and delimiter boundaries
            Ps5_6_2_1_2_46301a => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Undefined length unknown items stay in implicit VR
            Ps5_6_2_2_894175 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Data elements are ordered and unique in data sets and items
            Ps5_7_1_c4fdc1 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_7_1_1_6bb31f => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Reserved bytes after a long VR are zero
            Ps5_7_1_2_6cc044 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Group lengths match their groups
            Ps5_7_2_e5da2e => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Delimiters have zero length and no value
            Ps5_7_5_1_aaae21 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Reserved groups are not used
            Ps5_7_5_1_0563be => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Data elements are ordered and unique in data sets and items
            Ps5_7_5_1_a89833 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Items hold no command file meta or directory groups
            Ps5_7_5_1_3363bd => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Delimiters have zero length and no value
            Ps5_7_5_2_29cc8f => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Delimiters have zero length and no value
            Ps5_7_5_2_94b19c => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Specific character set appears in items only where the IOD defines it
            Ps5_7_5_3_397ec3 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Repeating groups are the overlay groups only
            Ps5_7_6_dea12f => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Data elements are ordered and unique in data sets and items
            Ps5_7_8_127399 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_7_8_ec77b4 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private groups do not use the unreservable elements
            Ps5_7_8_1_8e6821 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private elements need their private creator
            Ps5_7_8_1_645952 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private creators hold one unique default repertoire code
            Ps5_7_8_1_783078 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private creators hold one unique default repertoire code
            Ps5_7_8_1_8472d4 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private creators hold one unique default repertoire code
            Ps5_7_8_1_9dc403 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Reserved groups are not used
            Ps5_7_8_1_83055e => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Items reserve their own blocks
            Ps5_7_8_1_195bb9 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_7_8_2_2daf2a => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Private sequences hold no pixel overlay or waveform data
            Ps5_7_8_2_f3885e => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Privately defined items do not use the DICOM root
            Ps5_9_b45977 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_1_debced => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_1_75aab3 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_1_461476 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_2_465474 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Native pixel data VR follows bits allocated
            Ps5_A_2_abe7d8 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_2_2c73c1 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_2_5dcbfa => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_2_497172 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_4_878ca2 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Native pixel data VR follows bits allocated
            Ps5_A_4_7baed6 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_4_bda4a7 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Lookup table descriptors are little endian
            Ps5_A_4_19a420 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // LUT descriptors are little endian in every little endian syntax
            Ps5_A_4_c9f614 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Deflated data sets are inflated and padded to even length
            Ps5_A_5_04de07 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_6_34f1d3 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Explicit VR bytes are upper case VRs of part 5
            Ps5_A_11_836dbf => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // Items hold no command file meta or directory groups
            Ps10_7_1_3a8346 => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // A file holds one data set
            Ps10_7_2_f911da => Checked("intrinsic-wsi-dicom-2026c-encoding"),
            // A UI Value under the DICOM root that PS3.6 Annex A does not register fails; private roots are undecidable
            Ps5_9_2_b903b5 => Checked("intrinsic-wsi-dicom-2026c-iod-value-format"),
            // The MAC is recomputed over the listed Data Elements; a Signature that does not verify fails
            Ps3_C_12_1_1_3_1_1_b3d0ca => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // A signed Sequence enters the MAC with every Item's Data Elements but the excluded ones
            Ps3_C_12_1_1_3_1_1_9cd4e0 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // Item Tags and a Sequence Delimitation Item Tag enter the MAC of a signed Sequence
            Ps3_C_12_1_1_3_1_1_5a8e43 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // Encapsulated Pixel Data enters the MAC with an Item Tag before each Fragment
            Ps3_C_12_1_1_3_1_1_88f6d4 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // A MAC Calculation Transfer Syntax that is not Explicit VR Little Endian, or one encoding the Pixel Data otherwise than the file, fails
            Ps3_C_12_1_1_3_1_2_793fd1 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // The stream is hashed with MAC Algorithm; a term outside Table C.12.1.1.3.1.2-1 fails
            Ps3_C_12_1_1_3_1_2_e64d07 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // Other Data Elements enter the MAC whole, with their Value Length
            Ps3_C_12_1_1_3_1_2_f55b08 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // SQ and undefined-length OB enter the MAC without their Value Length
            Ps3_C_12_1_1_3_1_2_974e7a => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // A Sequence Delimitation Item Tag follows every signed Sequence and encapsulated Pixel Data
            Ps3_C_12_1_1_3_1_2_725031 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // The signature Item's Attributes but certificate, Signature and timestamp follow; verified with the RSA key of Certificate of Signer, whose trust is undecidable
            Ps3_C_12_1_1_3_1_2_e3cab5 => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // A CMS_TSP token whose messageImprint is not the Signature's hash, or whose embedded TSA signature fails, fails; when it was obtained is undecidable
            Ps3_C_12_6_CertifiedTimestamp_bedcdd => Checked("intrinsic-wsi-dicom-2026c-digital-signatures"),
            // ---- Bind IODs, encodings or contexts a VL WSI object cannot carry. ----
            Ps10_7_4_fb6b0c => OutsideScope("Secure DICOM File is a CMS envelope, not a Part 10 WSI file; encrypted content unreadable"),
            Ps10_8_1_02c223 => OutsideScope("Binds media format mapping of File ID namespace (PS3.12)"),
            Ps10_8_1_1b75db => OutsideScope("Defines File-set DICOMDIR membership; a WSI file or plain directory cannot violate it"),
            Ps10_8_1_b3ba17 => OutsideScope("Binds PS3.12 media format specifications, not files"),
            Ps10_8_1_efcf92 => OutsideScope("File-set UID lives in DICOMDIR; DICOMDIR content is outside WSI scope"),
            Ps10_8_6_0ebc18 => OutsideScope("Binds the DICOMDIR file's Transfer Syntax, not WSI files"),
            Ps10_8_6_572268 => OutsideScope("Binds DICOMDIR File Meta SOP Class, not WSI files"),
            Ps10_8_6_6f3918 => OutsideScope("Binds the DICOMDIR file's format, not WSI files"),
            Ps10_8_6_cab9e6 => OutsideScope("Binds DICOMDIR references; DICOMDIR is not a WSI object"),
            Ps10_8_6_cda158 => OutsideScope("Defines File-set DICOMDIR membership; a WSI file or plain directory cannot violate it"),
            Ps10_8_6_f6c0f3 => OutsideScope("Binds DICOMDIR File Meta SOP Instance UID, not WSI files"),
            Ps10_8_968f45 => OutsideScope("Binds physical media and media formats, not WSI file content"),
            Ps3_C_2_1_122fa1 => OutsideScope("C.2.1 Patient Relationship is a normalized module; WSI uses C.7 rows, none cite C.2.1"),
            Ps3_C_2_3_1a990f => OutsideScope("C.2.3 normalized Patient Demographic table; WSI equivalents come from C.7 rows, already generated"),
            Ps3_C_2_3_99887b => OutsideScope("C.2.3 normalized Patient Demographic table not cited by WSI; statement is vacuous anyway"),
            Ps3_C_2_4_9fde48 => OutsideScope("C.2.4 Patient Medical Module not in WSI IOD; statement is vacuous anyway"),
            Ps3_C_7_10_1_1_1_053155 => OutsideScope("Defines X-Ray irradiation events; VL microscopy acquisition has no X-Ray irradiation"),
            Ps3_C_7_10_1_1_1_b59c2b => OutsideScope("Pulsed fluoroscopy X-Ray example; cannot apply to a VL WSI acquisition"),
            Ps3_C_7_11c_Columns_0168ee => OutsideScope("Native downsampled chroma only; WSI uncompressed must be MONOCHROME2 or RGB (C.8.12.4.1.5)"),
            Ps3_C_7_11c_Rows_456628 => OutsideScope("Native downsampled chroma only; WSI uncompressed must be MONOCHROME2 or RGB (C.8.12.4.1.5)"),
            Ps3_C_7_4_1_1_2_c02f4d => OutsideScope("Corneal coordinate system is ophthalmic only; WSI Frame of Reference is slide-based"),
            Ps3_C_7_5_1_1_2_efdd0a => OutsideScope("WSI forbids MONOCHROME1 (C.8.12.4-1 enumeration); Pixel Padding is not in Icon Image items"),
            Ps3_C_7_6_16_2_25_2f8353 => OutsideScope("Unassigned Shared Converted Attributes Macro is Legacy Converted only; not in Table A.32.8-2"),
            Ps3_C_7_6_16_2_25_8fe231 => OutsideScope("Matching rule for Legacy Converted conversion; WSI has no converted-attribute macros"),
            Ps3_C_7_6_16_2_25_b8dc4e => OutsideScope("Unassigned Per-Frame Converted Attributes Macro is Legacy Converted only; not in Table A.32.8-2"),
            Ps3_C_7_6_16_2_2_6_d4a473 => OutsideScope("PET Dynamic image stacks; not applicable to the VL WSI IOD"),
            Ps3_C_7_6_16_2_2_6_f8bd55 => OutsideScope("PET Dynamic dimension ordering; Temporal Position Index/Stack ID not used by WSI"),
            Ps3_C_7_6_16_2_3_1_209538 => OutsideScope("SAMPLED Volumetric Properties excluded by WSI enumeration VOLUME; Plane Position/Orientation (Patient) absent"),
            Ps3_C_7_6_16_2_3_1_39c820 => OutsideScope("Slice Thickness for SAMPLED frames; WSI Volumetric Properties is always VOLUME"),
            Ps3_C_7_6_16_2_3_1_94e7cc => OutsideScope("Image Position (Patient) for SAMPLED frames; neither occurs in WSI"),
            Ps3_C_7_6_16_2_3_1_95795d => OutsideScope("Image Orientation (Patient) for SAMPLED frames; neither occurs in WSI"),
            Ps3_C_7_6_1_1_1_ab0c02 => OutsideScope("Image Orientation (Patient) belongs to no Module or Functional Group of the WSI IOD"),
            Ps3_C_7_6_2_1_1_29ac83 => OutsideScope("Image Orientation (Patient) normalization; WSI uses Image Orientation (Slide) instead"),
            Ps3_C_7_6_2_1_1_34cf77 => OutsideScope("Image Orientation (Patient) cosines; WSI uses Image Orientation (Slide) instead"),
            Ps3_C_7_6_2_1_1_859096 => OutsideScope("Image Position/Orientation (Patient) pair; Image Plane Module not in WSI IOD"),
            Ps3_C_7_6_3_1_2_5fa00b => OutsideScope("Native YBR_FULL_422 sample layout; WSI uncompressed must be MONOCHROME2 or RGB (C.8.12.4.1.5)"),
            Ps3_C_7_6_3_1_2_7be829 => OutsideScope("YBR_PARTIAL_420 siting; that Photometric Interpretation is not permitted in WSI or Icon images"),
            Ps3_C_7_6_3_1_3_f9dea3 => OutsideScope("Planar Configuration 1 is excluded in WSI (enumeration 0) and in Icon images"),
            Ps3_C_7_6_3_1_5_1d78b7 => OutsideScope("Applies only to the Segmentation IOD"),
            Ps3_C_7_6_3_1_5_7659ed => OutsideScope("Applies only to the Color Palette IOD (A.58)"),
            Ps3_C_7_6_3_1_5_b1076b => OutsideScope("Alpha Palette Color LUT Descriptor is in no WSI Module or Macro"),
            Ps3_C_7_9_1_a8a229 => OutsideScope("Applies only when the Palette Color LUT Module is in the Color Palette IOD (A.58)"),
            Ps3_C_7_9_PresentationLUTShape_579186 => OutsideScope("WSI forbids MONOCHROME1 (C.8.12.4-1); Presentation LUT Shape is top-level only"),
            Ps3_C_8_12_2_1_1_4fc3ea => OutsideScope("Image Center Point Coordinates Sequence (Slide Coordinates Module) is not in WSI IOD"),
            Ps3_C_8_12_5_1_1_6f325a => OutsideScope("WSI always includes Multi-frame Functional Groups (M); condition targets other IODs"),
            Ps3_C_8_16_1_1_3d5347 => OutsideScope("C.8.16.1.1 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_1_d275fb => OutsideScope("C.8.16.1.1 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_2_9cda19 => OutsideScope("C.8.16.1.2 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_2_d4b014 => OutsideScope("C.8.16.1.2 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_3_66c2ea => OutsideScope("C.8.16.1.3 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_3e29fe => OutsideScope("C.8.16.1 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_4_0067a1 => OutsideScope("C.8.16.1.4 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_4_2f3775 => OutsideScope("C.8.16.1.4 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_4_e1a716 => OutsideScope("C.8.16.1.4 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_1_4d8cea => OutsideScope("C.8.16.1 binds CT/MR/US/PA; WSI four-value rule is C.8.12.4.1.1 statement"),
            Ps3_C_8_16_1_95e046 => OutsideScope("C.8.16.1 binds CT/MR/US/PA Image Type; WSI Frame Type is shared-only, no MIXED"),
            Ps3_C_8_16_1_a44052 => OutsideScope("C.8.16.1 binds CT/MR/US/PA Image Type; WSI defines its own in C.8.12.4.1.1"),
            Ps3_C_8_16_2_1_3_2c806a => OutsideScope("Volume Based Calculation Technique (0008,9207) is not in the WSI IOD"),
            Ps3_C_8_16_2_1_3_49f9d6 => OutsideScope("Volume Based Calculation Technique (0008,9207) is not in the WSI IOD"),
            Ps3_C_8_30_3_1_4_6f526b => OutsideScope("Corneal coordinate system binds Corneal Topography Map data, absent from WSI"),
            Ps3_C_8_30_3_1_4_b39027 => OutsideScope("Corneal coordinate system binds Corneal Topography Map data, absent from WSI"),
            Ps3_C_8_30_3_1_4_df3f8b => OutsideScope("Corneal coordinate system binds Corneal Topography Map data, absent from WSI"),
            Ps3_C_8_30_3_1_4_fc2365 => OutsideScope("Corneal coordinate system binds Corneal Topography Map data, absent from WSI"),
            Ps5_6_2_a9e8be => OutsideScope("Binds VR definitions of future editions of the Standard, not any encoded 2026c object"),
            Ps5_6_4_7e8758 => OutsideScope("Notation rule for VM in the Standard's tables, not an encoded property"),
            Ps5_7_6_481a49 => OutsideScope("Statement about future Standard design, not encoded objects"),
            Ps5_7_7_2c17ff => OutsideScope("Binds future editions of the Standard"),
            Ps5_8_1_2_05616b => OutsideScope("Overlay Plane Module absent from VL WSI IOD (Table A.32.8-1); 60xx overlays not carried"),
            Ps5_8_1_2_3b62f6 => OutsideScope("Overlay Data bit packing; no Overlay Plane Module in VL WSI IOD"),
            Ps5_8_1_2_70a5ff => OutsideScope("Overlay Data frame concatenation (8.1.2); no Overlay Plane Module in VL WSI IOD"),
            Ps5_8_2_0fadc3 => OutsideScope("Float Pixel Data belongs to no VL WSI IOD module; only Pixel Data (7FE0,0010) carried"),
            Ps5_8_2_10_17fcd6 => OutsideScope("Video container format inside Pixel Data; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_10_64b5e7 => OutsideScope("PTS/DTS use inside transport stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_10_7e5161 => OutsideScope("aspect_ratio_idc inside video bitstream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_10_af1171 => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_10_c1679c => OutsideScope("Attributes vs HEVC stream need video header parsing; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_11_455596 => OutsideScope("Attributes vs HEVC Main10 stream need video header parsing; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_11_7456b1 => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_11_84bfbd => OutsideScope("aspect_ratio_idc inside video bitstream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_11_a9912e => OutsideScope("Video container format inside Pixel Data; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_11_ce01f7 => OutsideScope("PTS/DTS use inside transport stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_12_b38f9a => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_18602a => OutsideScope("Float Pixel Data (Floating Point Image Pixel Module) not in VL WSI IOD"),
            Ps5_8_2_4bcda5 => OutsideScope("Double Float Pixel Data (Double Floating Point Image Pixel Module) not in VL WSI IOD"),
            Ps5_8_2_5_5c5c79 => OutsideScope("Attributes vs MPEG2 stream need video header parsing; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_5_6cde8d => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_5_c5b5a7 => OutsideScope("MPEG2 component semantics inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_6_405812 => OutsideScope("Attributes vs MPEG2 MP@HL stream need video header parsing; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_6_4d8fa8 => OutsideScope("MPEG2 aspect_ratio_information inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_6_571188 => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_7_3618e9 => OutsideScope("PTS/DTS use inside transport stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_7_7773fd => OutsideScope("aspect_ratio_idc inside video bitstream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_7_7886b4 => OutsideScope("Attributes vs H.264 stream need video header parsing; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_7_ce01ab => OutsideScope("Video container format inside Pixel Data; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_7_d08204 => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_8_34a4e3 => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_8_68acb5 => OutsideScope("Video container format inside Pixel Data; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_8_7fe9c4 => OutsideScope("PTS/DTS use inside transport stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_8_9d4541 => OutsideScope("aspect_ratio_idc inside video bitstream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_8_d6c85e => OutsideScope("Attributes vs H.264 stream need video header parsing; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_9_a52046 => OutsideScope("Audio components inside video stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_9_b1098e => OutsideScope("PTS/DTS use inside transport stream; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_2_9_c7bb6c => OutsideScope("Video container format inside Pixel Data; WSI forbids motion TS (C.8.12.4.1.5), validator must flag declared motion TS"),
            Ps5_8_3_7a124d => OutsideScope("Waveform Bits Allocated/VR; no Waveform Module in VL WSI IOD"),
            Ps5_8_3_7c241f => OutsideScope("Waveform channel attributes VR; no Waveform Module in VL WSI IOD"),
            Ps5_8_3_8dc825 => OutsideScope("Waveform Data (5400,1010); no Waveform Module in VL WSI IOD (Table A.32.8-1)"),
            Ps5_A_10_2fa6d0 => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps5_A_10_3289c0 => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps5_A_10_3cc0b0 => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps5_A_10_adf45d => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps5_A_10_c6cb25 => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps5_A_1_326b50 => OutsideScope("Waveform Data cannot occur in a VL WSI object; waveforms belong to waveform IODs"),
            Ps5_A_1_8ce325 => OutsideScope("Blending LUT Data exists only in Presentation State / blending IODs, never a VL WSI object"),
            Ps5_A_1_d0c6a0 => OutsideScope("Track Point Index List exists only in Tractography Results IOD, never a VL WSI object"),
            Ps5_A_2_367471 => OutsideScope("Waveform Data cannot occur in a VL WSI object; waveforms belong to waveform IODs"),
            Ps5_A_2_5cbc84 => OutsideScope("Track Point Index List exists only in Tractography Results IOD, never a VL WSI object"),
            Ps5_A_2_e15d71 => OutsideScope("Blending LUT Data exists only in Presentation State / blending IODs, never a VL WSI object"),
            Ps5_A_4_10_7a2eb4 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_3dedae => OutsideScope("Blending LUT Data exists only in Presentation State / blending IODs, never a VL WSI object"),
            Ps5_A_4_5_54be61 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_6_eaaff4 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_7_313c85 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_7_7502f5 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_7_9b3674 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_7_d07e3a => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_8_c22295 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_9_85a504 => OutsideScope("Motion TS internals; a WSI declaring it violates C.8.12.4.1.5, unchecked (RGB passes instance.rs:393)"),
            Ps5_A_4_cd73d1 => OutsideScope("Track Point Index List exists only in Tractography Results IOD, never a VL WSI object"),
            Ps5_A_4_cfe2f9 => OutsideScope("Waveform Data cannot occur in a VL WSI object; waveforms belong to waveform IODs"),
            Ps5_A_8_68d505 => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps5_A_8_ae75ef => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps5_A_9_988e06 => OutsideScope("SMPTE ST 2110 TS is for DICOM-RTV flows, not Part 10 files; a WSI declaring it is invalid"),
            Ps10_8_5_e938f0 => OutsideScope("File IDs name File-set members recorded in a DICOMDIR; validation reads .dcm files, not File-sets"),
            Ps3_C_7_11c_ColorSpace_763a29 => OutsideScope("Color Space shall not be present beside the Optical Path Sequence, which VL WSI requires"),
            // ---- Bind applications or processes, not the encoded objects. ----
            Ps10_7_1_581c44 => Behavior("Binds readers: only bit 0 of byte 2 may be interpreted; validator complies"),
            Ps10_7_1_ad3b69 => Behavior("Binds readers; note validator flags unregistered (0002,xxxx) tags (values.rs:96)"),
            Ps10_7_1_b959ab => Behavior("Binds File-set Readers/Updaters, not file content"),
            Ps10_7_2_b5e9bc => Behavior("Binds File-set Readers/Updaters processing capability"),
            Ps10_7_2_bf7e26 => Behavior("Binds readers to ignore Trailing Padding value"),
            Ps10_7_5_bc5a1c => Behavior("Binds implementation Conformance Statement content"),
            Ps10_8_1_4746d4 => Behavior("Binds File-set Updaters across edits (history)"),
            Ps10_8_2_a79088 => Behavior("Binds AEs reading or creating files"),
            Ps10_8_3_11afdd => Behavior("Binds File-set Reader behavior"),
            Ps10_8_3_ab0ab0 => Behavior("Binds File Service media operations"),
            Ps10_8_3_ce660f => Behavior("Binds implementation role support"),
            Ps10_8_3_fb41de => Behavior("Binds File-set Updater behavior"),
            Ps10_8_4_129fce => Behavior("Binds File Service end-of-file handling"),
            Ps10_8_4_1591a2 => Behavior("Binds File Service capabilities"),
            Ps10_8_6_056f88 => Behavior("Binds AEs reading or updating a File-set"),
            Ps3_8_3_9db03b => Behavior("Constrains how applications use Code Meaning, not encoded content"),
            Ps3_C_7_4_1_1_2_cd0d16 => Behavior("Restricts how applications use the Value (annotation only), not its encoding"),
            Ps3_C_7_5_1_1_2_8a51b2 => Behavior("Binds modifying equipment's process when padding becomes indistinguishable"),
            Ps3_C_7_5_1_1_2_9b8ebe => Behavior("Binds modifying equipment's process when it changes padding values"),
            Ps3_C_7_6_17_3_ad808a => Behavior("Constrains recipient assumptions about TILED_SPARSE frames"),
            Ps3_C_7_6_1_1_2_97936c => Behavior("Requires documentation in a conformance statement, not a property of the object"),
            Ps5_10_1_0cbe46 => Behavior("Binds implementation support of the Default Transfer Syntax"),
            Ps5_10_1_d41f69 => Behavior("Binds AE association negotiation (Presentation Contexts); not a property of a stored file"),
            Ps5_10_20_b47931 => Behavior("Binds AE association negotiation (Presentation Contexts); not a property of a stored file"),
            Ps5_10_2_3a0b36 => Behavior("Binds AE association negotiation (Presentation Contexts); not a property of a stored file"),
            Ps5_10_2_956d41 => Behavior("Binds implementation support for lossless JPEG processes"),
            Ps5_10_3_db3079 => Behavior("Binds AE association negotiation (Presentation Contexts); not a property of a stored file"),
            Ps5_10_4_d49808 => Behavior("Binds AE association negotiation (Presentation Contexts); not a property of a stored file"),
            Ps5_10_5_0029bf => Behavior("Binds implementation support (JPEG-LS baseline), not file content"),
            Ps5_10_744125 => Behavior("Binds organizations registering private Transfer Syntax UIDs, not file content"),
            Ps5_6_2_2_f023ca => Behavior("Binds byte-order conversion by applications; UN bytes are left unchanged by design"),
            Ps5_6_2_904f94 => Behavior("Receivers ignore trailing UR spaces; validator already trims them (values.rs:559)"),
            Ps5_6_3_3c8903 => Behavior("Comparison rule for IS/DS terms; validator already compares numerically (engine.rs:344)"),
            Ps5_6_3_77e461 => Behavior("Binds receivers and validators not to reject unknown Defined Terms; validator checks only Enumerated Values"),
            Ps5_7_2_bd3564 => Behavior("Binds implementations not to require Group Length elements"),
            Ps5_7_3_4b4c77 => Behavior("Binds processing on Big Endian machines"),
            Ps5_7_3_5d552b => Behavior("Binds Application Entity support of the default Transfer Syntax"),
            Ps5_7_4_5_70d404 => Behavior("Interpretation rule: a zero-length Type 3 element means the same as absent"),
            Ps5_7_5_1_e3cceb => Behavior("Binds decoders to support both Item length encodings"),
            Ps5_7_5_2_7d3e46 => Behavior("Binds decoders to support both Sequence delimitation methods"),
            Ps5_7_5_3_b35e95 => Behavior("Decoder scoping rule; note dicom-parser keeps an Item's SCS active after the Item (decode.rs:872)"),
            Ps5_7_5_822bd2 => Behavior("Defines Item ordinal semantics that applications must preserve; encoded Items are always ordered"),
            Ps5_7_5_96c73f => Behavior("Defines implicit ordinal numbering used for referencing; no encoded property"),
            Ps5_7_5_d718e7 => Behavior("Definitional consequence of ordinal numbering; cannot be violated in bytes"),
            Ps5_7_8_1_4da0de => Behavior("Binds encoder capabilities for assigning private blocks"),
            Ps5_7_8_1_538c39 => Behavior("Binds decoder acceptance of private blocks at any position"),
            Ps5_8_1_1_6dd7d7 => Behavior("Receivers must not treat padding as significant; not an encoded-object property"),
            Ps5_8_1_1_d109a2 => Behavior("Design goal that implementations accommodate bit depths; no encoded-object property"),
            Ps5_8_2_10_fd07b0 => Behavior("Decoder obligation to use HEVC bitstream characteristics"),
            Ps5_8_2_11_f8bc4f => Behavior("Decoder obligation to use HEVC Main 10 bitstream characteristics"),
            Ps5_8_2_14_ffedc6 => Behavior("Decoder obligation to use HTJ2K codestream characteristics"),
            Ps5_8_2_15_4c0d86 => Behavior("Decoder obligation to use JPEG XL codestream characteristics"),
            Ps5_8_2_1_2dc897 => Behavior("Decoder obligation to use JPEG interchange format characteristics"),
            Ps5_8_2_1_43326a => Behavior("Implementation support obligation for JPEG Process 4"),
            Ps5_8_2_1_fdb6f4 => Behavior("Implementation support obligation for JPEG Baseline Process 1"),
            Ps5_8_2_1_fe71fd => Behavior("Implementation support obligation for JPEG lossless Process 14 SV1"),
            Ps5_8_2_3_3b4454 => Behavior("Decoder obligation to use JPEG-LS interchange format characteristics"),
            Ps5_8_2_4_c79fec => Behavior("Decoder obligation to use JPEG 2000 codestream characteristics"),
            Ps5_8_2_5_fc541c => Behavior("Decoder obligation to use MPEG2 bitstream characteristics"),
            Ps5_8_2_6_9618ea => Behavior("Decoder obligation to use MPEG2 MP@HL bitstream characteristics"),
            Ps5_8_2_7_c4778c => Behavior("Decoder obligation to use H.264 bitstream characteristics"),
            Ps5_8_2_8_f5ebd6 => Behavior("Decoder obligation to use H.264 bitstream characteristics"),
            Ps5_8_4_1_0eb868 => Behavior("JPIP server Content-type and client support obligations"),
            Ps5_8_4_1_2e3bb2 => Behavior("JPIP client request behavior"),
            Ps5_8_4_1_5fbd87 => Behavior("Server-side code stream numbering of the JPIP target; not encoded in the object"),
            Ps5_8_5_4a5789 => Behavior("Informative security advice for implementers"),
            Ps5_9_0c8924 => Behavior("Binds organizations' UID registration policies"),
            Ps5_9_2_2_4bc3d5 => Behavior("Binds the defining organization's responsibility for uniqueness"),
            Ps5_9_86308f => Behavior("Binds applications not to parse UIDs for semantics"),
            Ps5_A_5_431c1c => Behavior("Binds implementation support of Explicit VR LE alongside Deflate"),
            // ---- Bind the objects on facts their bytes cannot show. ----
            Ps10_7_1_849343 => Undecidable("Non-zero preamble is legitimate when a profile or implementation uses it (e.g., TIFF dual-format)"),
            Ps10_8_2_c25c0a => Undecidable("Whether File ID structure conveys semantics is not observable from names or bytes"),
            Ps3_10_14_cb9917 => Undecidable("Whether local and universal IDs name the same entity is not encoded"),
            Ps3_10_18_UniversalEntityID_ccba0a => Undecidable("Whether free-text Issuer of Patient ID names the same authority is not decidable"),
            Ps3_10_3b_DICOMMediaRetrievalSequence_bd171b => Undecidable("What a media file-set holds is external to the file"),
            Ps3_10_3b_DICOMRetrievalSequence_3b7839 => Undecidable("What a retrieval source holds is external to the file"),
            Ps3_10_3b_XDSRetrievalSequence_2b69ce => Undecidable("What an XDS repository holds is external to the file"),
            Ps3_8_3_73025f => Undecidable("Synonyms and translations permitted; correct meaning not derivable from bytes"),
            Ps3_C_12_1_1_4_1_76ea58 => Undecidable("Comparing values requires decrypting Encrypted Content; recipient keys are not in the file"),
            Ps3_C_12_1_1_9_1_5cf653 => Undecidable("Whether a recorded value is the entire prior Sequence value is not knowable"),
            Ps3_C_12_1_1_9_1_9f0b3d => Undecidable("Which attributes changed and their prior values are not recorded elsewhere in the file"),
            Ps3_C_12_1_1_9_211e3d => Undecidable("Preservation across modifications needs prior versions of the instance; history not in bytes"),
            Ps3_C_12_1_ConversionSourceAttributesSequence_b7a045 => Undecidable("Whether conversion came from a specific source Frame is not encoded"),
            Ps3_C_2_2_1_1_efbdaa => Undecidable("Depends on whether the referenced server is WADO-compliant and what the URI returns"),
            Ps3_C_7_1_1_1_1_a30ea8 => Undecidable("Whether the breed is known is not encoded; non-human status is undeterminable (conditions.rs non_human)"),
            Ps3_C_7_1_1_1_1_b0068e => Undecidable("Whether a breed is mixed is not encoded; any of the three encodings is valid"),
            Ps3_C_7_1_4_1_1_64e8d5 => Undecidable("Whether a Patient Attribute is shared by the whole group is a fact outside the bytes"),
            Ps3_C_7_1_4_1_1_e288bb => Undecidable("Which acquisition Attributes are subject-specific depends on facts the bytes cannot show"),
            Ps3_C_7_5_1_1_2_6790e6 => Undecidable("Whether the Value denotes stored rather than transformed pixel values is not encoded"),
            Ps3_C_7_5_1_1_2_ec1c1c => Undecidable("Whether the padding value occurs in the native (unpadded) image region is unknowable from bytes"),
            Ps3_C_7_6_14_2b9b4f => Undecidable("Semantic overlap between context items and other Attributes has no defined mapping"),
            Ps3_C_7_6_16_2_5_1_1c4f41 => Undecidable("Whether other objects were used to plan acquisition is not encoded"),
            Ps3_C_7_6_16_2_5_1_aac449 => Undecidable("Planning purpose is only knowable through the code itself; circular, not decidable"),
            Ps3_C_7_6_1_1_1_200b65 => Undecidable("Which direction is principal depends on true geometry, not encoded bytes"),
            Ps3_C_7_6_1_1_5_491487 => Undecidable("Object history across versions is not visible in one encoded object"),
            Ps3_C_7_6_1_1_5_4caf86 => Undecidable("Whether lossy compression happened in the sensor is not encoded"),
            Ps3_C_7_6_22_2_SpecimenDescriptionSequence_184620 => Undecidable("Which specimens appear in the pixels is not knowable from the bytes"),
            Ps3_C_7_6_3_1_2_2da8f9 => Undecidable("Chroma siting is not signalled in native bytes or JPEG headers"),
            Ps3_C_7_6_3_1_2_8fee6e => Undecidable("Chroma siting is not signalled in native bytes or JPEG headers"),
            Ps3_C_7_6_3_1_2_fc3f71 => Undecidable("Chroma siting is not signalled in native bytes or JPEG headers"),
            Ps3_C_7_6_3_1_3_f6f69c => Undecidable("Interleaving order of native samples cannot be distinguished from the bytes"),
            Ps3_C_7_6_3_1_6_14a40e => Undecidable("A dark palette is indistinguishable from an unscaled one"),
            Ps3_C_7_6_6_1_1_5e31a4 => Undecidable("Semantic relation of Image IE Attributes to the first frame is not encoded"),
            Ps3_C_7_6_6_1_3_c71343 => Undecidable("Whether frames are stereoscopic pairs is not encoded"),
            Ps3_C_7_9_LossyImageCompression_e641df => Undecidable("Object history across versions is not visible in one encoded object"),
            Ps3_C_8_12_2_1_1_9040d9 => Undecidable("Axis direction relative to physical label edge not determinable from bytes"),
            Ps3_C_8_12_2_1_1_bdac29 => Undecidable("Physical Z reference plane not determinable; negative Z offsets are not strictly invalid"),
            Ps3_C_8_12_2_1_1_df0fc6 => Undecidable("Axis direction relative to physical slide edges not determinable from bytes"),
            Ps3_C_8_12_4_1_1_8b67da => Undecidable("Whether pixels include label, all specimen and empty space is image content"),
            Ps3_C_8_12_4_1_1_cd67be => Undecidable("Label absence in THUMBNAIL pixels not decidable; attribute side is SpecimenLabelInImage rule"),
            Ps3_C_8_12_4_1_1_db2a55 => Undecidable("Label absence in VOLUME pixels not decidable; attribute side is SpecimenLabelInImage rule"),
            Ps3_C_8_12_4_1_1_fa1cda => Undecidable("Whether pixels show only the label is image content, not decidable"),
            Ps3_C_8_12_4_1_LossyImageCompression_debf6c => Undecidable("Requires the object's history; prior Lossy Image Compression values are not in bytes"),
            Ps3_C_8_12_5_1_ChannelDescriptionCodeSequence_49e204 => Undecidable("Which code describes which encoded channel is semantic; item count checked elsewhere"),
            Ps3_C_8_13_2_1_1_5edea3 => Undecidable("Doubt about approval status is not encoded"),
            Ps3_C_8_13_2_1_1_80e1f3 => Undecidable("Approval status of hardware and software is not encoded"),
            Ps5_6_2_1_2_db1198 => Undecidable("Whether group content is a phonetic representation is semantic, not checkable from bytes"),
            Ps5_6_2_1_2_df98c6 => Undecidable("A group delimiter other than '=' is indistinguishable from name text; parser splits on '=' only"),
            Ps5_6_2_1_2_ed37aa => Undecidable("Whether group content is an ideographic representation is semantic, not checkable from bytes"),
            Ps5_6_2_921c95 => Undecidable("A non-caret character meant as delimiter is indistinguishable from name text; parser splits on ^ only"),
            Ps5_6_2_b45dec => Undecidable("A group delimiter other than '=' is indistinguishable from name text; parser splits on '=' only"),
            Ps5_6_3_310276 => Undecidable("Conformance Statement content and meaning of implementer Defined Terms are not in the bytes"),
            Ps5_6_3_4b377b => Undecidable("Meaning of implementer-defined new Values is not encoded"),
            Ps5_7_8_0ac332 => Undecidable("Semantic effect of private data on the IOD is not encoded"),
            Ps5_8_1_bfa7ac => Undecidable("Whether OB content was byte-swapped is invisible without intended values; only retired Big Endian differs"),
            Ps5_8_2_23a2a1 => Undecidable("Bit order of cells spanning bytes (16-bit cells in OB) unverifiable without intended sample values"),
            Ps5_A_11_e5440b => Undecidable("Byte order of a numeric Value is not observable; swapped Values surface only via other range rules"),
            Ps5_A_1_77e469 => Undecidable("Byte order of a numeric Value is not observable; swapped Values surface only via other range rules"),
            Ps5_A_2_8b71de => Undecidable("Byte order of a numeric Value is not observable; swapped Values surface only via other range rules"),
            Ps5_A_4_7b5903 => Undecidable("Byte order of a numeric Value is not observable; swapped Values surface only via other range rules"),
            Ps5_A_6_351dc0 => Undecidable("Byte order of a numeric Value is not observable; swapped Values surface only via other range rules"),
            Ps5_A_4_13_a456ca => Undecidable("Whether a Fragment's last byte is the NUL pad or deflate data is known only after inflating"),
            Ps3_C_12_1_1_6_442d5f => Undecidable("Whether a UID is the one an HL7 identifier maps to is not in the bytes"),
        }
    }
}

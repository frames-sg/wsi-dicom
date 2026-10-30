use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

#[derive(Clone, Copy, Debug)]
pub(super) struct PixelSpacingMm {
    row: f64,
    column: f64,
}

impl PixelSpacingMm {
    pub(super) fn from_shared_functional_groups(
        object: &DefaultDicomObject,
    ) -> Result<Self, String> {
        let shared = sequence_items(
            object,
            tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            "Shared Functional Groups Sequence",
        )?
        .first()
        .ok_or_else(|| "Shared Functional Groups Sequence is empty".to_string())?;
        let measures = sequence_items(
            shared,
            tags::PIXEL_MEASURES_SEQUENCE,
            "Pixel Measures Sequence",
        )?
        .first()
        .ok_or_else(|| "Pixel Measures Sequence is empty".to_string())?;
        let spacing = measures
            .element(tags::PIXEL_SPACING)
            .map_err(|_| "Pixel Spacing is missing".to_string())?
            .to_multi_float64()
            .map_err(|err| format!("Pixel Spacing is invalid: {err}"))?;
        if spacing.len() != 2
            || spacing
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err("Pixel Spacing must contain two positive finite values".into());
        }
        Ok(Self {
            row: spacing[0],
            column: spacing[1],
        })
    }

    pub(super) fn row(self) -> f64 {
        self.row
    }

    pub(super) fn column(self) -> f64 {
        self.column
    }

    pub(super) fn matrix_width(self, columns: u64) -> f64 {
        columns as f64 * self.column
    }

    pub(super) fn matrix_height(self, rows: u64) -> f64 {
        rows as f64 * self.row
    }
}

pub(super) fn sequence_items<'a>(
    object: &'a InMemDicomObject,
    tag: Tag,
    name: &str,
) -> Result<&'a [InMemDicomObject], String> {
    object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .items()
        .ok_or_else(|| format!("{name} is not a sequence"))
}

pub(super) fn required_string(
    object: &InMemDicomObject,
    tag: Tag,
    name: &str,
) -> Result<String, String> {
    let value = object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_str()
        .map_err(|err| format!("{name} is invalid: {err}"))?
        .trim()
        .to_string();
    if value.is_empty() {
        Err(format!("{name} is empty"))
    } else {
        Ok(value)
    }
}

pub(super) fn required_type_two_string(
    object: &InMemDicomObject,
    tag: Tag,
    name: &str,
) -> Result<String, String> {
    object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_str()
        .map(|value| value.trim().to_string())
        .map_err(|err| format!("{name} is invalid: {err}"))
}

pub(super) fn optional_string(
    object: &InMemDicomObject,
    tag: Tag,
) -> Result<Option<String>, String> {
    match object.element(tag) {
        Ok(element) => {
            let value = element
                .to_str()
                .map_err(|err| format!("element {tag:?} is invalid: {err}"))?
                .trim()
                .to_string();
            if value.is_empty() {
                Err(format!("element {tag:?} is empty"))
            } else {
                Ok(Some(value))
            }
        }
        Err(_) => Ok(None),
    }
}

pub(super) fn required_positive_u64(
    object: &InMemDicomObject,
    tag: Tag,
    name: &str,
) -> Result<u64, String> {
    let value = object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_int::<u64>()
        .map_err(|err| format!("{name} is invalid: {err}"))?;
    if value == 0 {
        Err(format!("{name} must be positive"))
    } else {
        Ok(value)
    }
}

pub(super) fn required_finite_float64(
    object: &InMemDicomObject,
    tag: Tag,
    name: &str,
) -> Result<f64, String> {
    let value = object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_float64()
        .map_err(|err| format!("{name} is invalid: {err}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("{name} must be finite"))
    }
}

pub(super) fn require_numeric_value(
    object: &InMemDicomObject,
    tag: Tag,
    name: &str,
    expected: f64,
) -> Result<(), String> {
    let value = object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_float64()
        .map_err(|err| format!("{name} is invalid: {err}"))?;
    if value == expected {
        Ok(())
    } else {
        Err(format!("{name} is {value}, expected {expected}"))
    }
}

//! Shared selection and result placement for compressed-domain direct routes.

use crate::error::Error;

pub(super) fn encode_selected_frames<'a, P, F: 'a, O>(
    planned: &'a [P],
    codec: &'static str,
    mut selected: impl FnMut(&P) -> bool,
    mut source_frame: impl FnMut(&'a P) -> Result<&'a F, Error>,
    encode: impl FnOnce(&[&F]) -> Result<Vec<Result<O, Error>>, Error>,
) -> Result<Vec<Option<Result<O, Error>>>, Error> {
    let mut indices = Vec::new();
    indices
        .try_reserve_exact(planned.len())
        .map_err(|_| Error::Unsupported {
            reason: format!("direct {codec} batch index exceeds available memory"),
        })?;
    indices.extend(
        planned
            .iter()
            .enumerate()
            .filter_map(|(idx, frame)| selected(frame).then_some(idx)),
    );

    let mut outcomes = Vec::new();
    outcomes
        .try_reserve_exact(planned.len())
        .map_err(|_| Error::Unsupported {
            reason: format!("direct {codec} batch outcome exceeds available memory"),
        })?;
    outcomes.resize_with(planned.len(), || None);
    if indices.is_empty() {
        return Ok(outcomes);
    }

    let mut frames = Vec::new();
    frames
        .try_reserve_exact(indices.len())
        .map_err(|_| Error::Unsupported {
            reason: format!("direct {codec} batch frame list exceeds available memory"),
        })?;
    for &idx in &indices {
        frames.push(source_frame(&planned[idx])?);
    }
    let encoded = encode(&frames)?;
    if encoded.len() != indices.len() {
        return Err(Error::Encode {
            message: format!(
                "direct {codec} batch returned {} result(s) for {} selected frame(s)",
                encoded.len(),
                indices.len()
            ),
        });
    }
    super::scatter_indexed_results(&mut outcomes, indices.into_iter().zip(encoded))?;
    Ok(outcomes)
}

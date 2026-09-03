//! Fixed integer predictors and block-local predictor selection.

use crate::{CoreError, rice::zigzag_encode};

/// Predictor state resets to zero at every block boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Predictor {
    Raw = 0,
    Delta = 1,
    Linear2 = 2,
    Cubic3 = 3,
    CrossAxis = 4,
}

impl Predictor {
    pub const TEMPORAL: [Self; 4] = [Self::Raw, Self::Delta, Self::Linear2, Self::Cubic3];

    pub fn from_id(id: u8) -> Result<Self, CoreError> {
        match id {
            0 => Ok(Self::Raw),
            1 => Ok(Self::Delta),
            2 => Ok(Self::Linear2),
            3 => Ok(Self::Cubic3),
            4 => Ok(Self::CrossAxis),
            _ => Err(CoreError::InvalidPredictor(id)),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Delta => "delta",
            Self::Linear2 => "linear2",
            Self::Cubic3 => "cubic3",
            Self::CrossAxis => "cross-axis",
        }
    }

    fn estimate(self, history: &[i32], reference: Option<i32>) -> i64 {
        let p1 = history.last().copied().map(i64::from).unwrap_or(0);
        let p2 = history
            .len()
            .checked_sub(2)
            .and_then(|index| history.get(index))
            .copied()
            .map(i64::from)
            .unwrap_or(0);
        let p3 = history
            .len()
            .checked_sub(3)
            .and_then(|index| history.get(index))
            .copied()
            .map(i64::from)
            .unwrap_or(0);
        match self {
            Self::Raw => 0,
            Self::Delta => p1,
            Self::Linear2 => 2 * p1 - p2,
            Self::Cubic3 => 3 * p1 - 3 * p2 + p3,
            Self::CrossAxis => reference.map(i64::from).unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Prediction {
    pub predictor: Predictor,
    pub rice_k: u8,
    pub residuals: Vec<i64>,
    pub estimated_bits: u64,
}

pub fn residuals(samples: &[i32], predictor: Predictor) -> Vec<i64> {
    let mut result = Vec::with_capacity(samples.len());
    for (index, &sample) in samples.iter().enumerate() {
        let predicted = predictor.estimate(&samples[..index], None);
        result.push(i64::from(sample) - predicted);
    }
    result
}

pub fn reconstruct(residuals: &[i64], predictor: Predictor) -> Result<Vec<i32>, CoreError> {
    reconstruct_with_reference(residuals, predictor, None)
}

pub fn reconstruct_with_reference(
    residuals: &[i64],
    predictor: Predictor,
    reference: Option<&[i32]>,
) -> Result<Vec<i32>, CoreError> {
    if reference.is_some_and(|values| values.len() != residuals.len())
        || (predictor == Predictor::CrossAxis && reference.is_none())
    {
        return Err(CoreError::SizeOverflow);
    }
    let mut samples = Vec::with_capacity(residuals.len());
    for (index, &residual) in residuals.iter().enumerate() {
        let value = predictor
            .estimate(&samples, reference.map(|values| values[index]))
            .checked_add(residual)
            .ok_or(CoreError::SizeOverflow)?;
        samples.push(i32::try_from(value).map_err(|_| CoreError::SampleOutOfRange(value))?);
    }
    Ok(samples)
}

/// Select the predictor and Rice parameter with the smallest exact bit count.
pub fn choose(samples: &[i32]) -> Result<Prediction, CoreError> {
    choose_with_reference(samples, None)
}

/// Select from temporal predictors and, when supplied, the current sample of a
/// previously encoded component as a simple cross-axis predictor.
pub fn choose_with_reference(
    samples: &[i32],
    reference: Option<&[i32]>,
) -> Result<Prediction, CoreError> {
    if reference.is_some_and(|values| values.len() != samples.len()) {
        return Err(CoreError::SizeOverflow);
    }
    let mut best: Option<Prediction> = None;
    let candidates = Predictor::TEMPORAL
        .into_iter()
        .chain(reference.map(|_| Predictor::CrossAxis));
    for predictor in candidates {
        let values: Vec<i64> = samples
            .iter()
            .enumerate()
            .map(|(index, &sample)| {
                i64::from(sample)
                    - predictor.estimate(&samples[..index], reference.map(|values| values[index]))
            })
            .collect();
        for rice_k in 0..=31 {
            let bits = rice_bit_count(&values, rice_k)?;
            if best
                .as_ref()
                .is_none_or(|candidate| bits < candidate.estimated_bits)
            {
                best = Some(Prediction {
                    predictor,
                    rice_k,
                    residuals: values.clone(),
                    estimated_bits: bits,
                });
            }
        }
    }
    best.ok_or(CoreError::SizeOverflow)
}

fn rice_bit_count(values: &[i64], k: u8) -> Result<u64, CoreError> {
    values.iter().try_fold(0_u64, |total, &value| {
        let quotient = zigzag_encode(value) >> k;
        total
            .checked_add(quotient)
            .and_then(|sum| sum.checked_add(1 + u64::from(k)))
            .ok_or(CoreError::SizeOverflow)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_axis_is_selected_and_reversible_for_identical_components() {
        let reference = vec![10, -20, 30, -40, 50];
        let selected = choose_with_reference(&reference, Some(&reference)).unwrap();
        assert_eq!(selected.predictor, Predictor::CrossAxis);
        let decoded =
            reconstruct_with_reference(&selected.residuals, selected.predictor, Some(&reference))
                .unwrap();
        assert_eq!(decoded, reference);
    }
}

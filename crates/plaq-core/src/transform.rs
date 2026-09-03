//! Integer-only, reversible channel transforms.

use crate::CoreError;

/// Convert stereo left/right samples to virtual stylus `mid`/`side` axes.
///
/// The division uses Euclidean division so negative odd values are rounded
/// toward negative infinity, matching the format specification.
pub fn stereo_to_mid_side(left: i32, right: i32) -> (i32, i32) {
    let side = i64::from(left) - i64::from(right);
    let mid = i64::from(right) + side.div_euclid(2);
    // With i32 inputs side may need 33 bits. PLAQ currently calls this only
    // for 16/24-bit PCM, so these conversions are exact.
    (mid as i32, side as i32)
}

/// Reconstruct stereo samples from virtual stylus `mid`/`side` axes.
pub fn mid_side_to_stereo(mid: i32, side: i32) -> Result<(i32, i32), CoreError> {
    let side64 = i64::from(side);
    let right = i64::from(mid) - side64.div_euclid(2);
    let left = side64 + right;
    let left = i32::try_from(left).map_err(|_| CoreError::SampleOutOfRange(left))?;
    let right = i32::try_from(right).map_err(|_| CoreError::SampleOutOfRange(right))?;
    Ok((left, right))
}

/// Split interleaved mono/stereo PCM into independently predicted axes.
pub fn to_components(interleaved: &[i32], channels: u8) -> Result<Vec<Vec<i32>>, CoreError> {
    match channels {
        1 => Ok(vec![interleaved.to_vec()]),
        2 => {
            if !interleaved.len().is_multiple_of(2) {
                return Err(CoreError::SizeOverflow);
            }
            let frames = interleaved.len() / 2;
            let mut mid = Vec::with_capacity(frames);
            let mut side = Vec::with_capacity(frames);
            for frame in interleaved.chunks_exact(2) {
                let (m, s) = stereo_to_mid_side(frame[0], frame[1]);
                mid.push(m);
                side.push(s);
            }
            Ok(vec![mid, side])
        }
        _ => Err(CoreError::SizeOverflow),
    }
}

/// Merge predicted axes back into interleaved mono/stereo PCM.
pub fn from_components(components: &[Vec<i32>], channels: u8) -> Result<Vec<i32>, CoreError> {
    match channels {
        1 if components.len() == 1 => Ok(components[0].clone()),
        2 if components.len() == 2 && components[0].len() == components[1].len() => {
            let mut samples = Vec::with_capacity(components[0].len() * 2);
            for (&mid, &side) in components[0].iter().zip(&components[1]) {
                let (left, right) = mid_side_to_stereo(mid, side)?;
                samples.push(left);
                samples.push(right);
            }
            Ok(samples)
        }
        _ => Err(CoreError::SizeOverflow),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_odd_side_rounds_reversibly() {
        let original = (-3, 4);
        let axes = stereo_to_mid_side(original.0, original.1);
        assert_eq!(mid_side_to_stereo(axes.0, axes.1).unwrap(), original);
    }
}

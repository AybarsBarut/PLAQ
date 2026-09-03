use plaq_core::{
    predictor::{Predictor, reconstruct, residuals},
    transform::{mid_side_to_stereo, stereo_to_mid_side},
};
use proptest::prelude::*;

proptest! {
    #[test]
    fn lifting_is_reversible_for_24_bit(left in -8_388_608_i32..=8_388_607, right in -8_388_608_i32..=8_388_607) {
        let (mid, side) = stereo_to_mid_side(left, right);
        prop_assert_eq!(mid_side_to_stereo(mid, side).unwrap(), (left, right));
    }

    #[test]
    fn predictors_are_reversible(samples in prop::collection::vec(-16_777_215_i32..=16_777_215, 0..2048)) {
        for predictor in Predictor::TEMPORAL {
            let encoded = residuals(&samples, predictor);
            prop_assert_eq!(reconstruct(&encoded, predictor).unwrap(), samples.clone());
        }
    }
}

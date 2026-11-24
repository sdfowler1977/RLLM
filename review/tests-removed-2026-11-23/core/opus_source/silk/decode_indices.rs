// core/opus_source/silk/decode_indices.rs original lines 144-198
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    /// Every stereo predictor level, from libopus's quantizer
    /// (stereo_quant_pred.c): `LEVELS_Q13[i][j]` is quantization interval
    /// `i`, sub-step `j`.
    const LEVELS_Q13: [[i32; 5]; 15] = [
        [-13364, -12628, -11892, -11156, -10420],
        [-9872, -9516, -9160, -8804, -8448],
        [-8192, -8044, -7896, -7748, -7600],
        [-7424, -7220, -7016, -6812, -6608],
        [-6350, -6050, -5750, -5450, -5150],
        [-4795, -4385, -3975, -3565, -3155],
        [-2737, -2311, -1885, -1459, -1033],
        [-656, -328, 0, 328, 656],
        [1033, 1459, 1885, 2311, 2737],
        [3155, 3565, 3975, 4385, 4795],
        [5150, 5450, 5750, 6050, 6350],
        [6602, 6806, 7010, 7214, 7418],
        [7600, 7748, 7896, 8044, 8192],
        [8444, 8800, 9156, 9512, 9868],
        [10418, 11154, 11890, 12626, 13362],
    ];

    /// Range-code two predictors as libopus's `silk_stereo_encode_pred`
    /// does, each given as (interval, sub-step), and decode them back.
    fn decode(pred: [(usize, usize); 2]) -> [i32; 2] {
        let mut enc = RangeCoder::new_encoder(64);
        let joint = 5 * (pred[0].0 / 3) + pred[1].0 / 3;
        enc.encode_icdf(joint as i32, &SILK_STEREO_PRED_JOINT_ICDF, 8);
        for (i, j) in pred {
            enc.encode_icdf((i % 3) as i32, &SILK_UNIFORM3_ICDF, 8);
            enc.encode_icdf(j as i32, &SILK_UNIFORM5_ICDF, 8);
        }
        let bytes = enc.finish();
        silk_stereo_decode_pred(&mut RangeCoder::new_decoder(&bytes))
    }

    #[test]
    fn stereo_pred_levels_match_libopus() {
        for (i, row) in LEVELS_Q13.iter().enumerate() {
            for (j, &level) in row.iter().enumerate() {
                // The second predictor is subtracted from the first; keep it
                // at the zero level so the first reads back unchanged.
                assert_eq!(decode([(i, j), (7, 2)]), [level, 0], "level ({i}, {j})");
            }
        }
    }

    #[test]
    fn stereo_pred_zero_level_is_neutral() {
        assert_eq!(decode([(7, 2), (7, 2)]), [0, 0]);
    }
}

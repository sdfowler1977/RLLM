//! Stereo SILK encoding front end: convert left/right to an adaptive mid/side
//! representation and quantize the predictors that let the decoder undo it.
//! Bit-exact ports of libopus 1.5.2 `silk/stereo_LR_to_MS.c`,
//! `stereo_find_predictor.c` and `stereo_quant_pred.c`.

use super::super::silk::define::*;
use super::super::silk::macros::*;
use super::super::silk::sigproc_fix::{silk_inner_prod_aligned_scale, silk_sum_sqr_shift_exact};
use super::super::silk::structs::SilkStereoState;
use super::super::silk::tables::SILK_STEREO_PRED_QUANT_Q13;

/// `SILK_FIX_CONST(STEREO_RATIO_SMOOTH_COEF, 16)` with
/// `STEREO_RATIO_SMOOTH_COEF = 0.01`: how fast the mid/residual norms and the
/// width follow a 20 ms frame (scaled by the speech activity squared).
const RATIO_SMOOTH_COEF_20MS_Q16: i32 = 655;
/// `SILK_FIX_CONST(STEREO_RATIO_SMOOTH_COEF / 2, 16)`, for 10 ms frames.
const RATIO_SMOOTH_COEF_10MS_Q16: i32 = 328;
/// `SILK_FIX_CONST(8 + 5, 16)`: the 13 parts of the default mid/side split.
const MID_SIDE_PARTS_Q16: i32 = 13 << 16;
/// `SILK_FIX_CONST(0.05, 14)`: below this `frac * smoothed width`, a frame
/// that had zero width stays panned mono.
const PANNED_MONO_THRES_Q14: i32 = 819;
/// `SILK_FIX_CONST(0.02, 14)`: below this `frac * smoothed width`, a frame
/// collapses to zero width.
const ZERO_WIDTH_THRES_Q14: i32 = 328;
/// `SILK_FIX_CONST(0.95, 14)`: above this smoothed width, code full width.
const FULL_WIDTH_THRES_Q14: i32 = 15565;

/// Least-squares predictor of `y` from `x` (Q13, limited to ±2), and the
/// ratio of the smoothed residual norm to the smoothed norm of `x` (Q14).
/// `mid_res_amp_q0` holds the two smoothed norms. Port of
/// `silk_stereo_find_predictor`.
pub fn silk_stereo_find_predictor(
    ratio_q14: &mut i32,
    x: &[i16],
    y: &[i16],
    mid_res_amp_q0: &mut [i32],
    length: usize,
    smooth_coef_q16: i32,
) -> i32 {
    let (mut nrgx, mut nrgy, mut scale1, mut scale2) = (0, 0, 0, 0);
    silk_sum_sqr_shift_exact(&mut nrgx, &mut scale1, x, length);
    silk_sum_sqr_shift_exact(&mut nrgy, &mut scale2, y, length);
    let mut scale = scale1.max(scale2);
    scale += scale & 1; // make even
    nrgy = silk_rshift32(nrgy, scale - scale2);
    nrgx = silk_rshift32(nrgx, scale - scale1);
    nrgx = nrgx.max(1);
    let corr = silk_inner_prod_aligned_scale(x, y, scale, length);
    let mut pred_q13 = silk_div32_varq(corr, nrgx, 13);
    pred_q13 = silk_limit(pred_q13, -(1 << 14), 1 << 14);
    let pred2_q10 = silk_smulwb(pred_q13, pred_q13);

    // Faster update for signals with large prediction parameters.
    let smooth_coef_q16 = smooth_coef_q16.max(pred2_q10.abs());

    // Smoothed mid and residual norms.
    let scale = scale >> 1;
    mid_res_amp_q0[0] = silk_smlawb(
        mid_res_amp_q0[0],
        silk_lshift(silk_sqrt_approx(nrgx), scale) - mid_res_amp_q0[0],
        smooth_coef_q16,
    );
    // Residual energy = nrgy - 2 * pred * corr + pred^2 * nrgx.
    nrgy = silk_sub_lshift32(nrgy, silk_smulwb(corr, pred_q13), 3 + 1);
    nrgy = silk_add_lshift32(nrgy, silk_smulwb(nrgx, pred2_q10), 6);
    mid_res_amp_q0[1] = silk_smlawb(
        mid_res_amp_q0[1],
        silk_lshift(silk_sqrt_approx(nrgy), scale) - mid_res_amp_q0[1],
        smooth_coef_q16,
    );

    // Ratio of the smoothed residual and mid norms.
    *ratio_q14 = silk_div32_varq(mid_res_amp_q0[1], mid_res_amp_q0[0].max(1), 14);
    *ratio_q14 = silk_limit(*ratio_q14, 0, 32767);

    pred_q13
}

/// Quantize the two predictors in place and return their indices
/// `[interval % 3, sub-step, interval / 3]`; then subtract the second
/// predictor from the first, as the decoder expects. Port of
/// `silk_stereo_quant_pred`.
pub fn silk_stereo_quant_pred(pred_q13: &mut [i32; 2], ix: &mut [[i8; 3]; 2]) {
    let mut quant_pred_q13 = 0;
    for n in 0..2 {
        // Brute-force search over the quantization levels. The levels rise
        // monotonically, so the first level that doesn't improve on the
        // previous one ends the search (libopus's `goto done`).
        let mut err_min_q13 = i32::MAX;
        'search: for i in 0..STEREO_QUANT_TAB_SIZE - 1 {
            let low_q13 = SILK_STEREO_PRED_QUANT_Q13[i] as i32;
            let step_q13 = silk_smulwb(
                SILK_STEREO_PRED_QUANT_Q13[i + 1] as i32 - low_q13,
                STEREO_HALF_SUB_STEP_Q16,
            );
            for j in 0..STEREO_QUANT_SUB_STEPS {
                let lvl_q13 = silk_smlabb(low_q13, step_q13, 2 * j + 1);
                let err_q13 = (pred_q13[n] - lvl_q13).abs();
                if err_q13 < err_min_q13 {
                    err_min_q13 = err_q13;
                    quant_pred_q13 = lvl_q13;
                    ix[n][0] = i as i8;
                    ix[n][1] = j as i8;
                } else {
                    break 'search;
                }
            }
        }
        ix[n][2] = ix[n][0] / 3;
        ix[n][0] -= ix[n][2] * 3;
        pred_q13[n] = quant_pred_q13;
    }

    // Subtract the second predictor from the first (helps when applying them).
    pred_q13[0] -= pred_q13[1];
}

/// Convert one frame of left/right to adaptive mid/side, in place. Port of
/// `silk_stereo_LR_to_MS`.
///
/// `x1` and `x2` are laid out like libopus's per-channel `inputBuf`: two
/// history slots, then the frame's `frame_length` left (`x1`) or right (`x2`)
/// samples at `[2..2 + frame_length]`. On return:
/// - `x1[0..frame_length + 2]` is the mid with its two history samples; the
///   mid channel codes `x1[1..1 + frame_length]`;
/// - `x2[1..1 + frame_length]` is the side residual after prediction from the
///   mid, which the side channel codes.
///
/// Also returns the predictor indices to write, whether to code the mid only,
/// and the split of `total_rate_bps` between mid and side.
pub fn silk_stereo_lr_to_ms(
    state: &mut SilkStereoState,
    x1: &mut [i16],
    x2: &mut [i16],
    ix: &mut [[i8; 3]; 2],
    mid_only_flag: &mut i8,
    mid_side_rates_bps: &mut [i32; 2],
    mut total_rate_bps: i32,
    prev_speech_act_q8: i32,
    to_mono: bool,
    fs_khz: i32,
    frame_length: usize,
) {
    let fl = frame_length;
    let x1 = &mut x1[..fl + 2];
    let x2 = &mut x2[..fl + 2];

    // Convert to basic mid/side signals. The mid overwrites the left input.
    let mut side = [0i16; MAX_FRAME_LENGTH + 2];
    for n in 0..fl + 2 {
        let sum = x1[n] as i32 + x2[n] as i32;
        let diff = x1[n] as i32 - x2[n] as i32;
        x1[n] = silk_rshift_round(sum, 1) as i16;
        side[n] = silk_sat16(silk_rshift_round(diff, 1)) as i16;
    }
    let mid = x1;
    let side = &mut side[..fl + 2];

    // Buffering.
    mid[..2].copy_from_slice(&state.s_mid);
    side[..2].copy_from_slice(&state.s_side);
    state.s_mid.copy_from_slice(&mid[fl..fl + 2]);
    state.s_side.copy_from_slice(&side[fl..fl + 2]);

    // LP and HP filter the mid and side signals.
    let mut lp_mid = [0i16; MAX_FRAME_LENGTH];
    let mut hp_mid = [0i16; MAX_FRAME_LENGTH];
    let mut lp_side = [0i16; MAX_FRAME_LENGTH];
    let mut hp_side = [0i16; MAX_FRAME_LENGTH];
    for n in 0..fl {
        let sum = silk_rshift_round(
            silk_add_lshift32(mid[n] as i32 + mid[n + 2] as i32, mid[n + 1] as i32, 1),
            2,
        );
        lp_mid[n] = sum as i16;
        hp_mid[n] = (mid[n + 1] as i32 - sum) as i16;
        let sum = silk_rshift_round(
            silk_add_lshift32(side[n] as i32 + side[n + 2] as i32, side[n + 1] as i32, 1),
            2,
        );
        lp_side[n] = sum as i16;
        hp_side[n] = (side[n + 1] as i32 - sum) as i16;
    }

    // Find energies and predictors.
    let is_10ms_frame = fl as i32 == 10 * fs_khz;
    let mut smooth_coef_q16 = if is_10ms_frame {
        RATIO_SMOOTH_COEF_10MS_Q16
    } else {
        RATIO_SMOOTH_COEF_20MS_Q16
    };
    smooth_coef_q16 = silk_smulwb(
        silk_smulbb(prev_speech_act_q8, prev_speech_act_q8),
        smooth_coef_q16,
    );

    let (mut lp_ratio_q14, mut hp_ratio_q14) = (0, 0);
    let mut pred_q13 = [
        silk_stereo_find_predictor(
            &mut lp_ratio_q14,
            &lp_mid,
            &lp_side,
            &mut state.mid_side_amp_q0[0..2],
            fl,
            smooth_coef_q16,
        ),
        silk_stereo_find_predictor(
            &mut hp_ratio_q14,
            &hp_mid,
            &hp_side,
            &mut state.mid_side_amp_q0[2..4],
            fl,
            smooth_coef_q16,
        ),
    ];
    // Ratio of the norms of the residual and mid signals.
    let mut frac_q16 = silk_smlabb(hp_ratio_q14, lp_ratio_q14, 3);
    frac_q16 = frac_q16.min(1 << 16);

    // Bitrate split between mid and side, possibly reducing the stereo width.
    // Subtract the approximate bitrate of the stereo parameters.
    total_rate_bps -= if is_10ms_frame { 1200 } else { 600 };
    if total_rate_bps < 1 {
        total_rate_bps = 1;
    }
    let min_mid_rate_bps = silk_smlabb(2000, fs_khz, 600);
    // Default split: 8 parts for the mid and 5 + 3 * frac for the side, so
    // mid_rate = 8 / (13 + 3 * frac) * total_rate.
    let frac_3_q16 = silk_mul(3, frac_q16);
    mid_side_rates_bps[0] =
        silk_div32_varq(total_rate_bps, MID_SIDE_PARTS_Q16 + frac_3_q16, 16 + 3);
    let mut width_q14;
    if mid_side_rates_bps[0] < min_mid_rate_bps {
        // Mid rate below the minimum: reduce the stereo width.
        mid_side_rates_bps[0] = min_mid_rate_bps;
        mid_side_rates_bps[1] = total_rate_bps - mid_side_rates_bps[0];
        // width = 4 * (2 * side_rate - min_rate) / ((1 + 3 * frac) * min_rate)
        width_q14 = silk_div32_varq(
            silk_lshift(mid_side_rates_bps[1], 1) - min_mid_rate_bps,
            silk_smulwb((1 << 16) + frac_3_q16, min_mid_rate_bps),
            14 + 2,
        );
        width_q14 = silk_limit(width_q14, 0, 1 << 14);
    } else {
        mid_side_rates_bps[1] = total_rate_bps - mid_side_rates_bps[0];
        width_q14 = 1 << 14;
    }

    // Smoother.
    state.smth_width_q14 = silk_smlawb(
        state.smth_width_q14 as i32,
        width_q14 - state.smth_width_q14 as i32,
        smooth_coef_q16,
    ) as i16;
    let smth_width_q14 = state.smth_width_q14 as i32;

    // At very low bitrates, or for inputs that are nearly amplitude panned,
    // switch to panned-mono coding.
    *mid_only_flag = 0;
    if to_mono {
        // Last frame before a stereo -> mono transition: collapse the width.
        width_q14 = 0;
        pred_q13 = [0, 0];
        silk_stereo_quant_pred(&mut pred_q13, ix);
    } else if state.width_prev_q14 == 0
        && (8 * total_rate_bps < 13 * min_mid_rate_bps
            || silk_smulwb(frac_q16, smth_width_q14) < PANNED_MONO_THRES_Q14)
    {
        // Panned mono; the previous frame already had zero width. Scale down
        // and quantize the predictors, then collapse the width.
        scale_preds(&mut pred_q13, smth_width_q14);
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = 0;
        pred_q13 = [0, 0];
        mid_side_rates_bps[0] = total_rate_bps;
        mid_side_rates_bps[1] = 0;
        *mid_only_flag = 1;
    } else if state.width_prev_q14 != 0
        && (8 * total_rate_bps < 11 * min_mid_rate_bps
            || silk_smulwb(frac_q16, smth_width_q14) < ZERO_WIDTH_THRES_Q14)
    {
        // Transition to zero-width stereo.
        scale_preds(&mut pred_q13, smth_width_q14);
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = 0;
        pred_q13 = [0, 0];
    } else if smth_width_q14 > FULL_WIDTH_THRES_Q14 {
        // Full-width stereo coding.
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = 1 << 14;
    } else {
        // Reduced-width stereo coding.
        scale_preds(&mut pred_q13, smth_width_q14);
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = smth_width_q14;
    }

    // Keep coding the side until its tapered output has been transmitted.
    if *mid_only_flag == 1 {
        state.silent_side_len =
            (state.silent_side_len as i32 + fl as i32 - STEREO_INTERP_LEN_MS * fs_khz) as i16;
        if (state.silent_side_len as i32) < LA_SHAPE_MS as i32 * fs_khz {
            *mid_only_flag = 0;
        } else {
            // Limit to avoid wrapping around.
            state.silent_side_len = 10000;
        }
    } else {
        state.silent_side_len = 0;
    }

    if *mid_only_flag == 0 && mid_side_rates_bps[1] < 1 {
        mid_side_rates_bps[1] = 1;
        mid_side_rates_bps[0] = (total_rate_bps - mid_side_rates_bps[1]).max(1);
    }

    // Interpolate the predictors and width over the first
    // STEREO_INTERP_LEN_MS, then subtract the prediction from the side.
    let interp_len = (STEREO_INTERP_LEN_MS * fs_khz) as usize;
    let mut pred0_q13 = -(state.pred_prev_q13[0] as i32);
    let mut pred1_q13 = -(state.pred_prev_q13[1] as i32);
    let mut w_q24 = silk_lshift(state.width_prev_q14 as i32, 10);
    let denom_q16 = silk_div32_16(1 << 16, STEREO_INTERP_LEN_MS * fs_khz);
    let delta0_q13 = -silk_rshift_round(
        silk_smulbb(pred_q13[0] - state.pred_prev_q13[0] as i32, denom_q16),
        16,
    );
    let delta1_q13 = -silk_rshift_round(
        silk_smulbb(pred_q13[1] - state.pred_prev_q13[1] as i32, denom_q16),
        16,
    );
    let deltaw_q24 = silk_lshift(
        silk_smulwb(width_q14 - state.width_prev_q14 as i32, denom_q16),
        10,
    );
    // C writes `x2[n - 1]` with `x2` at `inputBuf + 2`: index `n + 1` here.
    let residual = |n: usize, pred0_q13: i32, pred1_q13: i32, w_q24: i32| -> i16 {
        let mut sum = silk_lshift(
            silk_add_lshift32(mid[n] as i32 + mid[n + 2] as i32, mid[n + 1] as i32, 1),
            9,
        ); // Q11
        sum = silk_smlawb(silk_smulwb(w_q24, side[n + 1] as i32), sum, pred0_q13); // Q8
        sum = silk_smlawb(sum, silk_lshift(mid[n + 1] as i32, 11), pred1_q13); // Q8
        silk_sat16(silk_rshift_round(sum, 8)) as i16
    };
    for n in 0..interp_len {
        pred0_q13 += delta0_q13;
        pred1_q13 += delta1_q13;
        w_q24 += deltaw_q24;
        x2[n + 1] = residual(n, pred0_q13, pred1_q13, w_q24);
    }
    let (pred0_q13, pred1_q13) = (-pred_q13[0], -pred_q13[1]);
    let w_q24 = silk_lshift(width_q14, 10);
    for n in interp_len..fl {
        x2[n + 1] = residual(n, pred0_q13, pred1_q13, w_q24);
    }
    state.pred_prev_q13 = [pred_q13[0] as i16, pred_q13[1] as i16];
    state.width_prev_q14 = width_q14 as i16;
}

/// Scale both predictors by the smoothed width (Q14).
fn scale_preds(pred_q13: &mut [i32; 2], smth_width_q14: i32) {
    for p in pred_q13 {
        *p = silk_rshift(silk_smulbb(smth_width_q14, *p), 14);
    }
}


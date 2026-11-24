// core/opus_source/silk/sigproc_fix.rs original lines 1415-1458
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    fn lpc_analysis_filter_ref(out: &mut [i16], input: &[i16], b: &[i16], len: usize, d: usize) {
        out[..d].fill(0);
        for ix in d..len {
            let mut s = 0i32;
            for k in 0..d {
                s = s.wrapping_add((input[ix - 1 - k] as i32).wrapping_mul(b[k] as i32));
            }
            let out32_q12 = ((input[ix] as i32) << 12).wrapping_sub(s);
            out[ix] = silk_sat16(silk_rshift_round(out32_q12, 12)) as i16;
        }
    }

    fn make_signal(len: usize, seed: u32) -> Vec<i16> {
        let mut s = seed;
        let mut out = vec![0i16; len];
        for v in &mut out {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (s >> 16) as i16;
        }
        out
    }

    #[test]
    fn test_silk_lpc_analysis_filter_matches_reference() {
        for d in (6usize..=16).step_by(2) {
            for &len in &[d + 8, d + 31, 160, 320] {
                let input = make_signal(len, (d as u32) * 17 + len as u32);
                let b = make_signal(d, 0x1234_5678 ^ d as u32);

                let mut out_opt = vec![0i16; len];
                let mut out_ref = vec![0i16; len];

                silk_lpc_analysis_filter(&mut out_opt, &input, &b, len, d, 0);
                lpc_analysis_filter_ref(&mut out_ref, &input, &b, len, d);

                assert_eq!(out_opt, out_ref, "mismatch for d={d}, len={len}");
            }
        }
    }
}

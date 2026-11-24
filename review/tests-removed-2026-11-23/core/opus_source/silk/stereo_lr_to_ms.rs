// core/opus_source/silk/stereo_lr_to_ms.rs original lines 369-650
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use super::super::range_coder::RangeCoder;
    use super::super::silk::decode_indices::silk_stereo_decode_pred;
    use super::super::silk::encode_indices::{silk_stereo_encode_mid_only, silk_stereo_encode_pred};

    /// The integer test-signal generator of the libopus oracle harness used to
    /// verify this port (xorshift32 noise in seven shapes), so that its golden
    /// hashes can be reproduced here.
    struct Gen {
        rng: u32,
        lp: i32,
    }

    impl Gen {
        fn xs(&mut self) -> u32 {
            let mut x = self.rng;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.rng = x;
            x
        }

        fn noise(&mut self) -> i32 {
            (self.xs() >> 16) as i32 - 32768
        }

        fn pair(&mut self, kind: u32, sh: u32) -> (i32, i32) {
            let sat = |v: i32| v.clamp(-32768, 32767);
            let (s, n1, n2) = (self.noise(), self.noise(), self.noise());
            let (l, r) = match kind {
                0 => (s >> sh, n1 >> sh),
                1 => ((s * 29 / 32) >> sh, (s * 9 / 32) >> sh),
                2 => ((s + n1 / 8) >> sh, (s * 3 / 4 + n2 / 8) >> sh),
                3 => (0, 0),
                4 => {
                    let c = [-32768, 32767, 0];
                    let l = c[(self.xs() % 3) as usize];
                    (l, c[(self.xs() % 3) as usize])
                }
                5 => (s >> sh, sat(-(s >> sh))),
                _ => {
                    self.lp = self.lp - (self.lp >> 3) + (s >> 3);
                    (sat(self.lp >> sh), sat((self.lp + (n1 >> 6)) >> sh))
                }
            };
            (sat(l), sat(r))
        }
    }

    fn fnv_i32(h: &mut u64, v: i32) {
        for b in v.to_le_bytes() {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x100000001b3);
        }
    }

    /// FNV-1a-64 of every output of `frames` frames of oracle stream `st`:
    /// the mid and side buffers, the indices, the mid-only flag, the rates,
    /// the stereo state, then the range-coded predictors and flags.
    fn stream_hash(st: u32, frames: usize) -> u64 {
        let mut g = Gen {
            rng: 0x9E37_79B9 ^ st.wrapping_mul(2_654_435_761),
            lp: 0,
        };
        let fs_khz = [8, 12, 16][(g.xs() % 3) as usize];
        let fl = if g.xs() & 1 != 0 {
            10 * fs_khz
        } else {
            20 * fs_khz
        } as usize;
        let mut kind = st % 7;
        let sh = g.xs() % 9;
        let mut state = SilkStereoState::default();
        let mut rc = RangeCoder::new_encoder(1275);
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for f in 0..frames {
            let rate = match g.xs() % 6 {
                0 => 1 + g.xs() % 2000,
                1 => 1 + g.xs() % 12000,
                _ => 1 + g.xs() % 80000,
            } as i32;
            let sa = (g.xs() % 256) as i32;
            let to_mono = g.xs() & 15 == 0;
            let (mut x1, mut x2) = ([0i16; MAX_FRAME_LENGTH + 2], [0i16; MAX_FRAME_LENGTH + 2]);
            for n in 0..fl + 2 {
                let (l, r) = if n < 2 {
                    (g.noise(), g.noise())
                } else {
                    g.pair(kind, sh)
                };
                x1[n] = l as i16;
                x2[n] = r as i16;
            }
            if st % 5 == 4 && f == frames / 2 {
                kind = (kind + 3) % 7;
            }
            let (mut ix, mut mid_only, mut rates) = ([[0i8; 3]; 2], 0i8, [0i32; 2]);
            silk_stereo_lr_to_ms(
                &mut state,
                &mut x1,
                &mut x2,
                &mut ix,
                &mut mid_only,
                &mut rates,
                rate,
                sa,
                to_mono,
                fs_khz,
                fl,
            );
            silk_stereo_encode_pred(&mut rc, &ix);
            silk_stereo_encode_mid_only(&mut rc, mid_only);
            for &v in x1[..fl + 2].iter().chain(&x2[..fl + 2]) {
                fnv_i32(&mut h, v as i32);
            }
            for v in ix.iter().flatten() {
                fnv_i32(&mut h, *v as i32);
            }
            for v in [mid_only as i32, rates[0], rates[1]] {
                fnv_i32(&mut h, v);
            }
            let s = &state;
            for v in [
                s.pred_prev_q13[0] as i32,
                s.pred_prev_q13[1] as i32,
                s.mid_side_amp_q0[0],
                s.mid_side_amp_q0[1],
                s.mid_side_amp_q0[2],
                s.mid_side_amp_q0[3],
                s.smth_width_q14 as i32,
                s.width_prev_q14 as i32,
                s.silent_side_len as i32,
            ] {
                fnv_i32(&mut h, v);
            }
        }
        rc.done();
        for &b in &rc.buf[..rc.offs as usize] {
            fnv_i32(&mut h, b as i32);
        }
        h
    }

    /// Golden hashes from libopus 1.5.2's own `silk_stereo_LR_to_MS`,
    /// `silk_stereo_encode_pred` and `silk_stereo_encode_mid_only`, driven by
    /// the same generator: 12 frames per stream, every signal shape, all three
    /// internal rates, 10 and 20 ms frames. The port was also checked against
    /// a 60 000-frame trace of the same harness.
    #[test]
    fn lr_to_ms_matches_libopus_golden_hashes() {
        const GOLDEN: [u64; 14] = [
            0xbc97853bc7159113,
            0x2f6bd3e1297bf691,
            0x944514f2e0ed6f31,
            0x8aeb5777da033f30,
            0xdd8a9cbc17c9f343,
            0x700b37dd8a30969e,
            0x198e0299dda3e429,
            0xb87ea872eb3f5c82,
            0xa2316219c7ca9e98,
            0xf87f9d49621cf386,
            0xd9e1034e331c3d27,
            0xb89856069d5ed7e1,
            0x20d6b330fbe21897,
            0xc85d45c3b82c0698,
        ];
        for (st, &golden) in GOLDEN.iter().enumerate() {
            assert_eq!(stream_hash(st as u32, 12), golden, "oracle stream {st}");
        }
    }

    /// Range-code `ix` and decode it, as the decoder sees it.
    fn decoded(ix: &[[i8; 3]; 2]) -> [i32; 2] {
        let mut enc = RangeCoder::new_encoder(64);
        silk_stereo_encode_pred(&mut enc, ix);
        let bytes = enc.finish();
        silk_stereo_decode_pred(&mut RangeCoder::new_decoder(&bytes))
    }

    #[test]
    fn quant_pred_returns_the_level_the_decoder_reads() {
        // Every Q13 value in range quantizes to a level, and the decoder reads
        // back exactly what the encoder subtracts.
        for p0 in (-16384..=16384).step_by(61) {
            for p1 in [-16384, -13364, -2000, 0, 1, 656, 9000, 16384] {
                let mut pred = [p0, p1];
                let mut ix = [[0i8; 3]; 2];
                silk_stereo_quant_pred(&mut pred, &mut ix);
                assert_eq!(decoded(&ix), pred, "pred ({p0}, {p1})");
            }
        }
    }

    #[test]
    fn quant_pred_picks_the_nearest_level() {
        // An exact level quantizes to itself; zero to the neutral index.
        for p in [-13364, -8192, -656, 0, 328, 7600, 13362] {
            let mut pred = [p, 0];
            let mut ix = [[0i8; 3]; 2];
            silk_stereo_quant_pred(&mut pred, &mut ix);
            assert_eq!(pred, [p, 0]);
        }
        let mut pred = [0, 0];
        let mut ix = [[0i8; 3]; 2];
        silk_stereo_quant_pred(&mut pred, &mut ix);
        assert_eq!(ix, [[1, 2, 2], [1, 2, 2]]);
    }

    #[test]
    fn find_predictor_is_the_least_squares_gain() {
        let x: Vec<i16> = (0..320)
            .map(|n| ((n * 7919) % 2001 - 1000) as i16 * 9)
            .collect();
        let pred = |y: &[i16]| {
            let (mut ratio, mut amp) = (0, [0i32, 1]);
            let p = silk_stereo_find_predictor(&mut ratio, &x, y, &mut amp, x.len(), 0);
            (p, ratio)
        };
        let neg: Vec<i16> = x.iter().map(|&v| -v).collect();
        let triple: Vec<i16> = x.iter().map(|&v| v * 3).collect();
        // A copy predicts with gain 1 (8192 in Q13, less the rounding of
        // silk_DIV32_varQ) and leaves no residual.
        let (copy, ratio) = pred(&x);
        assert!((copy - 8192).abs() <= 2, "copy: {copy}");
        assert_eq!(ratio, 0);
        let inverse = pred(&neg).0;
        assert!((inverse + 8192).abs() <= 2, "inverse: {inverse}");
        // Gains beyond 2 are clamped.
        assert_eq!(pred(&triple).0, 1 << 14);
        assert_eq!(pred(&[0; 320]).0, 0);
    }

    /// Run `frames` frames of identical L and R through `silk_stereo_lr_to_ms`
    /// and return each frame's mid-only flag; the side residual must be zero.
    fn mono_input_mid_only(fs_khz: i32, frame_ms: i32, frames: usize) -> Vec<i8> {
        let fl = (frame_ms * fs_khz) as usize;
        let mut state = SilkStereoState::default();
        let mut flags = Vec::new();
        for f in 0..frames {
            let (mut x1, mut x2) = ([0i16; MAX_FRAME_LENGTH + 2], [0i16; MAX_FRAME_LENGTH + 2]);
            for n in 2..fl + 2 {
                let v = (((f * fl + n) * 2654435761) >> 20) as i16;
                x1[n] = v;
                x2[n] = v;
            }
            let last = [x1[fl], x1[fl + 1]];
            let (mut ix, mut mid_only, mut rates) = ([[0i8; 3]; 2], 0i8, [0i32; 2]);
            silk_stereo_lr_to_ms(
                &mut state,
                &mut x1,
                &mut x2,
                &mut ix,
                &mut mid_only,
                &mut rates,
                32000,
                255,
                false,
                fs_khz,
                fl,
            );
            assert!(
                x2[1..1 + fl].iter().all(|&s| s == 0),
                "frame {f}: side residual"
            );
            assert_eq!(state.s_mid, last, "frame {f}: mid history");
            flags.push(mid_only);
        }
        flags
    }

    #[test]
    fn mono_input_codes_mid_only_once_the_side_taper_is_sent() {
        // The side keeps being coded until the 8 ms predictor taper plus the
        // 5 ms shaping lookahead are out: one 20 ms frame, three 10 ms frames.
        assert_eq!(mono_input_mid_only(16, 20, 3), [1, 1, 1]);
        assert_eq!(mono_input_mid_only(16, 10, 4), [0, 0, 1, 1]);
        assert_eq!(mono_input_mid_only(8, 10, 4), [0, 0, 1, 1]);
    }
}

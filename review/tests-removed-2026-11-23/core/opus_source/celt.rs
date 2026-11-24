// core/opus_source/celt.rs original lines 1733-1735
    /// The last frame's allocation decisions, which unit tests inspect.
    #[cfg(test)]
    decisions: FrameDecisions,
// core/opus_source/celt.rs original lines 1738-1746
#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct FrameDecisions {
    alloc_trim: i32,
    tf_select: i32,
    tf_res: [i32; MAX_NB_EBANDS],
    is_transient: bool,
    weak_transient: bool,
}
// core/opus_source/celt.rs original lines 2280-2281
            #[cfg(test)]
            decisions: FrameDecisions::default(),
// core/opus_source/celt.rs original lines 2939-2944
        #[cfg(test)]
        let tf_raw = {
            let mut raw = [0i32; MAX_NB_EBANDS];
            raw[..end].copy_from_slice(&tf_res[..end]);
            raw
        };
// core/opus_source/celt.rs original lines 3080-3089
        #[cfg(test)]
        {
            self.decisions = FrameDecisions {
                alloc_trim,
                tf_select,
                tf_res: tf_raw,
                is_transient,
                weak_transient,
            };
        }
// core/opus_source/celt.rs original lines 4290-4400
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use super::{modes, range_coder::RangeCoder};

    #[test]
    fn frames_without_a_transient_flag_are_coded_as_not_transient() {
        // A 2.5 ms frame codes no transient flag, so decoders take it as not
        // transient. libopus codes it with long blocks and counts it as a
        // transient frame (celt_encoder.c:1728-1735, 2805-2808): every frame
        // of a 2.5 ms stream counts, whatever its transient analysis says.
        let mut enc = CeltEncoder::new(modes::default_mode(), 1);
        enc.set_end_band(21);
        enc.complexity = 10;
        let code = |enc: &mut CeltEncoder, pcm: &[f32]| {
            let mut rc = RangeCoder::new_encoder(20);
            enc.encode_with_budget_vbr(pcm, pcm.len(), &mut rc, 0, 160, false);
            rc.done();
        };
        let tone: Vec<f32> = (0..120 * 8).map(|i| 0.1 * (i as f32 * 0.3).sin()).collect();
        for f in 0..7 {
            code(&mut enc, &tone[f * 120..(f + 1) * 120]);
        }
        let mut clicks = tone[840..].to_vec();
        for (i, x) in clicks[60..90].iter_mut().enumerate() {
            *x = if i % 2 == 0 { 0.9 } else { -0.9 };
        }
        code(&mut enc, &clicks);
        assert_eq!(enc.consec_transient, 8);
    }

    // Regression test: directly drive CeltEncoder with an invalid frame_size=48,
    // bypassing the OpusEncoder::encode() validation layer.
    //
    // This reproduces the crash that was reported against opus-rs 0.1.19 when
    // G.729-decoded PCM (8 kHz) reached the 48 kHz Opus encoder without correct
    // resampling, producing a 48-sample frame instead of 480.
    //
    // Root cause: the lm-search in encode_impl finds no valid match for frame_size=48
    // (valid sizes are 120, 240, 480, 960) and silently falls back to lm=0.
    // With lm=0 and shift=max_lm=3: n=1920>>3=240, n2=120, overlap2=60.
    // The in_buf slice has only frame_size+overlap=168 elements, but forward()
    // requires input.len() >= n2+overlap2 = 180, so it panics immediately.
    // In opus-rs 0.1.19 this assertion was absent and the crash reached the MDCT
    // output write: "index out of bounds: the len is 48 but the index is 119".
    //
    // Either way: the call panics, confirming the crash path is real.
    // The fix in OpusEncoder::encode() returns Err before reaching CeltEncoder.
    #[test]
    #[should_panic]
    fn test_celt_frame_size_48_panics_confirms_crash_path() {
        let mode = modes::default_mode();
        let mut enc = CeltEncoder::new(mode, 1);
        // frame_size=48: lm-search fails, falls back to lm=0.
        // forward() will panic — either on the input-size assertion (0.1.21+) or
        // on the output write (0.1.19): "len is 48 but the index is 119".
        let pcm = vec![0.0f32; 48 + mode.overlap]; // supply ≥ frame_size samples
        let mut rc = RangeCoder::new_encoder(100);
        enc.encode_with_budget(&pcm, 48, &mut rc, 0, 800);
    }

    // Issue #54: a frame whose comb filter is cancelled must still fade the
    // previous frame's filter out over the overlap, because the decoder's
    // post-filter always does. Pitched history into a silent frame: any
    // filtering adds energy, so the filter is cancelled after an "on" frame.
    #[test]
    fn cancelled_prefilter_still_fades_out_previous_filter() {
        let mode = modes::default_mode();
        let (n, overlap, max_period) = (960, mode.overlap, COMBFILTER_MAXPERIOD);
        let mut mem: Vec<f32> = (0..max_period)
            .map(|i| (i as f32 * 0.3).sin() * 1000.0)
            .collect();
        let mut in_buf = vec![0.0f32; n + overlap];
        let mut pre = vec![0.0f32; max_period + n];
        let mut pitch_buf = vec![0.0f32; (max_period + n) >> 1];
        let (mut before, mut after) = ([0.0f32], [0.0f32]);
        let (pf_on, gain, _) = run_prefilter(
            &mut in_buf,
            &mut mem,
            100,
            0.5,
            0,
            0,
            mode.window,
            1,
            n,
            overlap,
            &mut pre,
            &mut pitch_buf,
            &mut before,
            &mut after,
            &AnalysisInfo::default(),
            0,
            0.0,
            100,
            -1.0,
            0.0,
            9,
        );
        assert!(!pf_on && gain == 0.0, "expected a cancelled filter");
        let body = &in_buf[overlap..];
        assert!(
            body[..overlap].iter().any(|&v| v.abs() > 1.0),
            "previous filter was not faded out over the overlap"
        );
        assert!(
            body[overlap..].iter().all(|&v| v == 0.0),
            "frame past the overlap must be the unfiltered input"
        );
    }
}
// core/opus_source/celt.rs original lines 4402-4485
#[cfg(all(test, feature = "std"))]
mod mode_switch_tests {
    use super::*;
    use super::{modes, range_coder::RangeCoder};

    /// A 200 Hz harmonic series with a little noise, at 48 kHz from sample
    /// `start`.
    fn voiced(n: usize, start: usize, seed: &mut u32) -> Vec<f32> {
        (start..start + n)
            .map(|i| {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let t = i as f32 / 48000.0;
                let harmonics: f32 = (1..8)
                    .map(|k| (2.0 * core::f32::consts::PI * 200.0 * k as f32 * t).sin() / k as f32)
                    .sum();
                0.2 * harmonics + ((*seed >> 9) as f32 / (1 << 23) as f32 - 0.5) * 0.01
            })
            .collect()
    }

    fn encoder() -> CeltEncoder {
        let mut enc = CeltEncoder::new(modes::default_mode(), 1);
        enc.set_end_band(21);
        enc.complexity = 10;
        enc
    }

    /// A CBR frame of `bytes` bytes.
    fn encode(enc: &mut CeltEncoder, pcm: &[f32], bytes: usize) -> Vec<u8> {
        let mut rc = RangeCoder::new_encoder(bytes as u32);
        enc.encode_with_budget_vbr(pcm, pcm.len(), &mut rc, 0, (bytes * 8) as i32, false);
        rc.done();
        rc.buf[..bytes].to_vec()
    }

    /// The post-filter and intra-energy flags of a 20 ms frame.
    fn flags(frame: &[u8]) -> (bool, bool) {
        let mut rc = RangeCoder::new_decoder(frame);
        assert!(!rc.decode_bit_logp(15), "silence");
        let postfilter = rc.decode_bit_logp(1);
        if postfilter {
            let octave = rc.dec_uint(6);
            rc.dec_bits(4 + octave);
            rc.dec_bits(3);
            rc.decode_icdf(&TAPSET_ICDF, 2);
        }
        rc.decode_bit_logp(3); // transient
        (postfilter, rc.decode_bit_logp(3))
    }

    #[test]
    fn prediction_off_drops_the_prefilter_and_codes_energy_intra() {
        // libopus CELT_SET_PREDICTION(0), which the first frame after a mode
        // switch uses: no pitch pre-filter and intra-coded energy.
        let (mut a, mut b) = (encoder(), encoder());
        let (mut sa, mut sb) = (1, 1);
        for f in 0..6 {
            encode(&mut a, &voiced(960, f * 960, &mut sa), 160);
            encode(&mut b, &voiced(960, f * 960, &mut sb), 160);
        }
        b.set_prediction(0);
        let with = flags(&encode(&mut a, &voiced(960, 6 * 960, &mut sa), 160));
        let without = flags(&encode(&mut b, &voiced(960, 6 * 960, &mut sb), 160));
        assert_eq!(with, (true, false));
        assert_eq!(without, (false, true));
    }

    #[test]
    fn reset_state_restarts_like_a_new_encoder() {
        // libopus OPUS_RESET_STATE, which a mode switch's redundancy frames
        // and prefill rely on.
        let mut used = encoder();
        let mut seed = 3;
        for f in 0..5 {
            encode(&mut used, &voiced(960, f * 960, &mut seed), 120);
        }
        used.reset_state();
        let frame = voiced(960, 0, &mut 9);
        assert_eq!(
            encode(&mut used, &frame, 120),
            encode(&mut encoder(), &frame, 120)
        );
    }
}
// core/opus_source/celt.rs original lines 4487-4692
#[cfg(all(test, feature = "std"))]
mod hybrid_decision_tests {
    use super::*;
    use super::{modes, range_coder::RangeCoder};

    /// Deterministic noise, channel-major like the CELT input.
    fn noise(n: usize, seed: &mut u32, amplitude: f32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((*seed >> 9) as f32 / (1 << 23) as f32 - 0.5) * amplitude
            })
            .collect()
    }

    fn vbr_encoder(channels: usize, bitrate: i32) -> CeltEncoder {
        let mut enc = CeltEncoder::new(modes::default_mode(), channels);
        enc.set_end_band(21);
        enc.set_bitrate(bitrate);
        enc.set_vbr(true);
        enc.set_constrained_vbr(false);
        enc.complexity = 9;
        enc
    }

    fn encode(enc: &mut CeltEncoder, pcm: &[f32], start_band: usize, bytes: usize) -> i32 {
        let mut rc = RangeCoder::new_encoder(bytes as u32);
        let vbr = enc.vbr;
        enc.encode_with_budget_vbr(pcm, 960, &mut rc, start_band, (bytes * 8) as i32, vbr);
        rc.tell()
    }

    #[test]
    fn hybrid_frames_fix_the_trim_and_spread_aggressively() {
        let mut enc = vbr_encoder(1, 8000);
        enc.set_silk_info(1, 240);
        let mut seed = 1;
        let mut steady = 0;
        for _ in 0..6 {
            let pcm = noise(960, &mut seed, 0.2);
            encode(&mut enc, &pcm, 17, 1499);
            assert_eq!(enc.decisions.alloc_trim, 5);
            let transient = enc.decisions.is_transient;
            steady += usize::from(!transient);
            let spread = if transient {
                SPREAD_NORMAL
            } else {
                SPREAD_AGGRESSIVE
            };
            assert_eq!(enc.spread_decision, spread);
        }
        assert!(steady >= 3);
    }

    /// A quiet frame with a loud click in its second half.
    fn click(seed: &mut u32) -> Vec<f32> {
        let mut pcm = noise(960, seed, 0.001);
        for (i, x) in pcm[700..760].iter_mut().enumerate() {
            *x += if i % 2 == 0 { 0.8 } else { -0.8 };
        }
        pcm
    }

    #[test]
    fn low_rate_hybrid_transients_keep_long_resolution() {
        // Unvoiced SILK: a weak transient codes TF on the long window, a
        // strong one keeps 5 ms resolution (celt_encoder.c:2261-2276).
        let mut enc = vbr_encoder(1, 2000);
        enc.set_silk_info(1, 240);
        let mut seed = 7;
        encode(&mut enc, &noise(960, &mut seed, 0.001), 17, 1499);
        encode(&mut enc, &click(&mut seed), 17, 1499);
        let d = enc.decisions;
        assert!(d.is_transient || d.weak_transient);
        if d.weak_transient {
            assert!(!d.is_transient);
            assert_eq!((d.tf_select, &d.tf_res[17..21]), (0, &[1; 4][..]));
        } else {
            assert_eq!((d.tf_select, &d.tf_res[17..21]), (1, &[0; 4][..]));
        }
        // Voiced SILK: no weak transients, TF follows the transient flag.
        let mut enc = vbr_encoder(1, 2000);
        enc.set_silk_info(2, 32);
        let mut seed = 7;
        encode(&mut enc, &noise(960, &mut seed, 0.001), 17, 1499);
        encode(&mut enc, &click(&mut seed), 17, 1499);
        let d = enc.decisions;
        assert!(d.is_transient && !d.weak_transient);
        assert_eq!((d.tf_select, &d.tf_res[17..21]), (0, &[1; 4][..]));
    }

    #[test]
    fn celt_frames_without_tf_analysis_follow_the_transient_flag() {
        // Too few bytes for TF analysis: tf_res is the transient flag.
        let mut enc = CeltEncoder::new(modes::default_mode(), 1);
        enc.set_end_band(21);
        enc.complexity = 9;
        let mut seed = 3;
        encode(&mut enc, &noise(960, &mut seed, 0.001), 0, 12);
        encode(&mut enc, &click(&mut seed), 0, 12);
        let d = enc.decisions;
        assert!(d.is_transient);
        assert_eq!(d.tf_select, 0);
        assert!(d.tf_res[..21].iter().all(|&tf| tf == 1));
    }

    #[test]
    fn weak_transients_are_found_only_when_allowed() {
        let mode = modes::default_mode();
        let len = 960 + mode.overlap;
        let mut tmp = vec![0.0f32; MAX_TRANSIENT_LEN];
        let mut tmp2 = vec![0.0f32; MAX_TRANSIENT_LEN / 2];
        let mut found = false;
        for step in [1.5f32, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0] {
            let mut seed = 11;
            let mut x = noise(len, &mut seed, 1000.0);
            for v in &mut x[len / 2..] {
                *v *= step;
            }
            let analyse = |allow: bool, tmp: &mut [f32], tmp2: &mut [f32]| {
                let (mut tf_estimate, mut tf_chan, mut weak) = (0.0, 0, false);
                let transient = transient_analysis(
                    &x,
                    len,
                    1,
                    &mut tf_estimate,
                    &mut tf_chan,
                    allow,
                    &mut weak,
                    0.0,
                    0.0,
                    tmp,
                    tmp2,
                );
                (transient, weak)
            };
            let (transient, weak) = analyse(false, &mut tmp, &mut tmp2);
            assert!(!weak);
            let (allowed_transient, allowed_weak) = analyse(true, &mut tmp, &mut tmp2);
            assert!(!(allowed_transient && allowed_weak));
            found |= transient && allowed_weak;
        }
        assert!(found, "no step gave a weak transient");
    }

    #[test]
    fn stereo_intensity_follows_the_celt_rate() {
        let mut seed = 5;
        // Hybrid at a tiny CELT share: intensity stereo from the first band.
        let mut enc = vbr_encoder(2, 4000);
        enc.set_silk_info(1, 240);
        encode(&mut enc, &noise(1920, &mut seed, 0.2), 17, 1499);
        assert_eq!(enc.intensity, 17);
        // CELT at 24 kb/s: the equivalent rate is the target, not the
        // packet capacity, so intensity stereo starts at band 10.
        let mut enc = vbr_encoder(2, 24000);
        encode(&mut enc, &noise(1920, &mut seed, 0.2), 0, 1275);
        assert_eq!(enc.intensity, 10);
    }

    #[test]
    fn silk_offset_moves_the_hybrid_target() {
        let mut seed = 9;
        let pcm = noise(960, &mut seed, 0.2);
        let mut bits = [0; 2];
        for (bits, (signal_type, offset)) in bits.iter_mut().zip([(2, 32), (1, 240)]) {
            let mut enc = vbr_encoder(1, 6000);
            enc.set_silk_info(signal_type, offset);
            *bits = encode(&mut enc, &pcm, 17, 1499);
        }
        // +12 bits for tonal, -18 for noisy 20 ms frames.
        assert!(bits[0] > bits[1] + 16, "{bits:?}");
    }

    #[test]
    fn loss_rate_reaches_the_coarse_energy() {
        let mut out = [Vec::new(), Vec::new()];
        for (out, loss) in out.iter_mut().zip([0, 25]) {
            let mut enc = vbr_encoder(1, 32000);
            enc.set_loss_rate(loss);
            let mut seed = 13;
            for frame in 0..30 {
                let amplitude = if frame % 7 == 0 { 0.6 } else { 0.05 };
                out.push(encode(&mut enc, &noise(960, &mut seed, amplitude), 0, 1275));
            }
        }
        assert_ne!(out[0], out[1]);
    }

    #[test]
    fn coded_band_count_moves_by_one_band_per_frame() {
        let mut enc = CeltEncoder::new(modes::default_mode(), 1);
        enc.set_end_band(21);
        enc.complexity = 9;
        let mut seed = 17;
        let mut last: Option<i32> = None;
        for frame in 0..12 {
            let bytes = if frame % 2 == 0 { 1275 } else { 16 };
            encode(&mut enc, &noise(960, &mut seed, 0.3), 0, bytes);
            if let Some(last) = last {
                assert!((enc.last_coded_bands - last).abs() <= 1);
            }
            last = Some(enc.last_coded_bands);
        }
    }
}

// core/opus_source/mdct.rs original lines 995-1223
#[cfg(all(test, feature = "std"))]
mod mdct_tests {
    #[test]
    fn test_mdct_backward_transient_no_blowup() {
        let mode = super::modes::default_mode();
        let shift = 3;
        let n = mode.mdct.n >> shift; // 120
        let overlap = mode.overlap; // 120
        let stride = 8;

        let frame_size = 960usize;
        let mut freq = vec![0.0f32; frame_size];
        for i in 0..frame_size {
            freq[i] = ((i as f32) * 0.01).sin() * 10.0;
        }

        let out_len = n + overlap; // 240
        let mut output0 = vec![0.0f32; out_len];
        let mut output1 = vec![0.0f32; out_len];

        mode.mdct.backward(
            &freq[0..],
            &mut output0,
            mode.window,
            overlap,
            shift,
            stride,
        );
        mode.mdct.backward(
            &freq[1..],
            &mut output1,
            mode.window,
            overlap,
            shift,
            stride,
        );

        let max0 = output0.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let max1 = output1.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

        assert!(max0.abs() < 500.0, "sub0 blowup: {}", max0);
        assert!(max1.abs() < 500.0, "sub1 blowup: {}", max1);
    }

    #[test]
    fn test_mdct_backward_stride1_neon_matches_scalar() {
        let mode = super::modes::default_mode();
        let shift = 0; // non-transient full-size MDCT
        let n = mode.mdct.n >> shift; // 1920
        let n2 = n / 2; // 960
        let n4 = n / 4; // 480
        let overlap = mode.overlap; // 120
        let overlap2 = overlap / 2; // 60
        let stride = 1;

        let freq_len = n2;
        let mut freq = vec![0.0f32; freq_len + 4];
        for i in 0..freq_len {
            freq[i] = ((i as f32) * 0.01).sin() * 4577.0;
        }

        let out_len = overlap2 + n2; // 60 + 960 = 1020
        let mut output_hw = vec![0.0f32; out_len + 100];
        mode.mdct.backward(
            &freq[..],
            &mut output_hw,
            mode.window,
            overlap,
            shift,
            stride,
        );

        let st = mode.mdct.kfft[shift].as_ref().unwrap();
        let (trig, _) = mode.mdct.get_trig(shift);

        use super::kiss_fft::KissCpx;
        let mut f2 = vec![KissCpx::new(0.0, 0.0); n4];
        for i in 0..n4 {
            let rev = st.bitrev[i] as usize;
            let x1 = freq[2 * i * stride];
            let x2 = freq[stride * (n2 - 1 - 2 * i)];
            let t0 = trig[i];
            let t1 = trig[n4 + i];
            let yr = x2 * t0 + x1 * t1;
            let yi = x1 * t0 - x2 * t1;
            f2[rev] = KissCpx::new(yi, yr);
        }
        super::kiss_fft::opus_fft_impl(st, &mut f2);

        let mut output_scalar = vec![0.0f32; out_len + 100];
        for i in 0..((n4 + 1) >> 1) {
            let im0 = f2[i].r;
            let re0 = f2[i].i;
            let t0_0 = trig[i];
            let t1_0 = trig[n4 + i];
            let yr0 = re0 * t0_0 + im0 * t1_0;
            let yi0 = re0 * t1_0 - im0 * t0_0;
            let j = n4 - 1 - i;
            let im1 = f2[j].r;
            let re1 = f2[j].i;
            let t0_1 = trig[j];
            let t1_1 = trig[n4 + j];
            let yr1 = re1 * t0_1 + im1 * t1_1;
            let yi1 = re1 * t1_1 - im1 * t0_1;
            output_scalar[overlap2 + 2 * i] = yr0;
            output_scalar[overlap2 + n2 - 1 - 2 * i] = yi0;
            output_scalar[overlap2 + n2 - 2 - 2 * i] = yr1;
            output_scalar[overlap2 + 2 * i + 1] = yi1;
        }
        // TDAC
        for i in 0..overlap2 {
            let x1 = output_scalar[overlap - 1 - i];
            let x2 = output_scalar[i];
            let wp1 = mode.window[i];
            let wp2 = mode.window[overlap - 1 - i];
            output_scalar[i] = x2 * wp2 - x1 * wp1;
            output_scalar[overlap - 1 - i] = x2 * wp1 + x1 * wp2;
        }

        let max_diff = output_hw[..out_len]
            .iter()
            .zip(output_scalar[..out_len].iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_diff < 0.5,
            "stride=1 NEON vs scalar mismatch: max_diff={}",
            max_diff
        );
    }

    #[test]
    fn test_mdct_backward_neon_matches_scalar() {
        let mode = super::modes::default_mode();
        let shift = 3;
        let n = mode.mdct.n >> shift; // 240
        let n2 = n / 2; // 120
        let n4 = n / 4; // 60
        let overlap = mode.overlap; // 120
        let overlap2 = overlap / 2; // 60
        let stride = 8;

        // Build a realistic freq vector (sine wave @ 440Hz)
        let frame_size = 960usize;
        let mut freq = vec![0.0f32; frame_size];
        for i in 0..frame_size {
            freq[i] = ((i as f32) * 0.01).sin() * 200.0;
        }

        let out_len = n + overlap; // 360
        let mut output_hw = vec![0.0f32; out_len];
        mode.mdct.backward(
            &freq[0..],
            &mut output_hw,
            mode.window,
            overlap,
            shift,
            stride,
        );

        // Scalar reference
        let st = mode.mdct.kfft[shift].as_ref().unwrap();
        let (trig, _) = mode.mdct.get_trig(shift);

        use super::kiss_fft::KissCpx;
        let mut f2 = vec![KissCpx::new(0.0, 0.0); n4];
        for i in 0..n4 {
            let rev = st.bitrev[i] as usize;
            let x1 = freq[2 * i * stride];
            let x2 = freq[stride * (n2 - 1 - 2 * i)];
            let t0 = trig[i];
            let t1 = trig[n4 + i];
            let yr = x2 * t0 + x1 * t1;
            let yi = x1 * t0 - x2 * t1;
            f2[rev] = KissCpx::new(yi, yr);
        }
        super::kiss_fft::opus_fft_impl(st, &mut f2);

        let mut output_scalar = vec![0.0f32; out_len];
        for i in 0..((n4 + 1) >> 1) {
            let im0 = f2[i].r;
            let re0 = f2[i].i;
            let t0_0 = trig[i];
            let t1_0 = trig[n4 + i];
            let yr0 = re0 * t0_0 + im0 * t1_0;
            let yi0 = re0 * t1_0 - im0 * t0_0;
            let j = n4 - 1 - i;
            let im1 = f2[j].r;
            let re1 = f2[j].i;
            let t0_1 = trig[j];
            let t1_1 = trig[n4 + j];
            let yr1 = re1 * t0_1 + im1 * t1_1;
            let yi1 = re1 * t1_1 - im1 * t0_1;
            output_scalar[overlap2 + 2 * i] = yr0;
            output_scalar[overlap2 + n2 - 1 - 2 * i] = yi0;
            output_scalar[overlap2 + n2 - 2 - 2 * i] = yr1;
            output_scalar[overlap2 + 2 * i + 1] = yi1;
        }
        // TDAC
        for i in 0..overlap2 {
            let x1 = output_scalar[overlap - 1 - i];
            let x2 = output_scalar[i];
            let wp1 = mode.window[i];
            let wp2 = mode.window[overlap - 1 - i];
            output_scalar[i] = x2 * wp2 - x1 * wp1;
            output_scalar[overlap - 1 - i] = x2 * wp1 + x1 * wp2;
        }

        for i in 0..out_len {
            let diff = (output_hw[i] - output_scalar[i]).abs();
            if diff > 1e-3 {
                eprintln!(
                    "Mismatch at output[{}]: hw={} scalar={} diff={}",
                    i, output_hw[i], output_scalar[i], diff
                );
            }
        }
        let max_diff = output_hw
            .iter()
            .zip(output_scalar.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_diff < 0.1,
            "NEON/HW vs scalar mismatch: max_diff={}",
            max_diff
        );
    }
}

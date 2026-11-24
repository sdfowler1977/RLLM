// core/opus_source/kiss_fft.rs original lines 1299-1515
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    fn almost_equal(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() < tolerance
    }

    fn cpx_almost_equal(a: &KissCpx, b: &KissCpx, tolerance: f32) -> bool {
        almost_equal(a.r, b.r, tolerance) && almost_equal(a.i, b.i, tolerance)
    }

    #[test]
    fn test_kf_factor_60() {
        let st = KissFftState::new(60).unwrap();
        assert!(!st.factors.iter().all(|&f| f == 0));
        assert_eq!(st.bitrev.len(), 60);
    }

    #[test]
    fn test_kf_factor_240() {
        let st = KissFftState::new(240).unwrap();
        assert!(!st.factors.iter().all(|&f| f == 0));
        assert_eq!(st.bitrev.len(), 240);
    }

    #[test]
    fn test_bitrev_permutation() {
        for &nfft in &[60, 120, 240, 480] {
            let st = KissFftState::new(nfft).unwrap();
            let mut sorted: Vec<i16> = st.bitrev.as_slice().to_vec();
            sorted.sort();
            let expected: Vec<i16> = (0..nfft).map(|x| x as i16).collect();
            assert_eq!(
                sorted,
                expected,
                "bitrev for nfft={} should be a permutation of 0..{}",
                nfft,
                nfft - 1
            );
        }
    }

    #[test]
    fn test_sub_fft() {
        let nfft = 480;
        let base = KissFftState::new(nfft).unwrap();

        for &n in &[60, 120, 240] {
            let sub = KissFftState::new_sub(&base, n).unwrap();
            assert_eq!(sub.nfft(), n);
            assert_eq!(sub.twiddles.len(), nfft);
        }
    }

    #[test]
    fn test_fft_roundtrip_60() {
        let nfft = 60;
        let st = KissFftState::new(nfft).unwrap();

        let mut fin = vec![KissCpx::default(); nfft];
        let mut fout = vec![KissCpx::default(); nfft];
        let mut finv = vec![KissCpx::default(); nfft];

        fin[0] = KissCpx::new(1.0, 0.0);

        opus_fft(&st, &fin, &mut fout);
        opus_ifft(&st, &fout, &mut finv);

        for i in 0..nfft {
            let expected = if i == 0 { 1.0 } else { 0.0 };
            assert!(
                cpx_almost_equal(&finv[i], &KissCpx::new(expected, 0.0), 1e-5),
                "Roundtrip failed at index {}: got ({}, {}), expected ({}, 0)",
                i,
                finv[i].r,
                finv[i].i,
                expected
            );
        }
    }

    #[test]
    fn test_fft_roundtrip_120() {
        let nfft = 120;
        let st = KissFftState::new(nfft).unwrap();

        let mut fin = vec![KissCpx::default(); nfft];
        let mut fout = vec![KissCpx::default(); nfft];
        let mut finv = vec![KissCpx::default(); nfft];

        for i in 0..nfft {
            fin[i] = KissCpx::new((2.0 * PI * 3.0 * i as f32 / nfft as f32).sin(), 0.0);
        }

        opus_fft(&st, &fin, &mut fout);
        opus_ifft(&st, &fout, &mut finv);

        for i in 0..nfft {
            assert!(
                cpx_almost_equal(&finv[i], &fin[i], 1e-4),
                "Roundtrip failed at index {}: got ({}, {}), expected ({}, {})",
                i,
                finv[i].r,
                finv[i].i,
                fin[i].r,
                fin[i].i
            );
        }
    }

    #[test]
    fn test_fft_roundtrip_480() {
        let nfft = 480;
        let st = KissFftState::new(nfft).unwrap();

        let mut fin = vec![KissCpx::default(); nfft];
        let mut fout = vec![KissCpx::default(); nfft];
        let mut finv = vec![KissCpx::default(); nfft];

        for i in 0..nfft {
            fin[i] = KissCpx::new(
                (2.0 * PI * 7.0 * i as f32 / nfft as f32).sin(),
                (2.0 * PI * 11.0 * i as f32 / nfft as f32).cos(),
            );
        }

        opus_fft(&st, &fin, &mut fout);
        opus_ifft(&st, &fout, &mut finv);

        for i in 0..nfft {
            assert!(
                cpx_almost_equal(&finv[i], &fin[i], 1e-4),
                "Roundtrip failed at index {}: got ({}, {}), expected ({}, {})",
                i,
                finv[i].r,
                finv[i].i,
                fin[i].r,
                fin[i].i
            );
        }
    }

    #[test]
    fn test_fft_dc_component() {
        let nfft = 120;
        let st = KissFftState::new(nfft).unwrap();

        let mut fin = vec![KissCpx::default(); nfft];
        let mut fout = vec![KissCpx::default(); nfft];

        for i in 0..nfft {
            fin[i] = KissCpx::new(1.0, 0.0);
        }

        opus_fft(&st, &fin, &mut fout);

        assert!(almost_equal(fout[0].r, 1.0, 1e-5));
        assert!(almost_equal(fout[0].i, 0.0, 1e-5));

        for i in 1..nfft {
            assert!(
                cpx_almost_equal(&fout[i], &KissCpx::new(0.0, 0.0), 1e-5),
                "Non-DC component at index {} is ({}, {}), expected (0, 0)",
                i,
                fout[i].r,
                fout[i].i
            );
        }
    }

    #[test]
    fn test_fft_performance() {
        use std::time::Instant;

        let nfft = 480;
        let st = KissFftState::new(nfft).unwrap();

        let mut fin = vec![KissCpx::default(); nfft];
        let mut fout = vec![KissCpx::default(); nfft];
        let mut finv = vec![KissCpx::default(); nfft];

        for i in 0..nfft {
            fin[i] = KissCpx::new(
                (2.0 * PI * 7.0 * i as f32 / nfft as f32).sin(),
                (2.0 * PI * 11.0 * i as f32 / nfft as f32).cos(),
            );
        }

        for _ in 0..100 {
            opus_fft(&st, &fin, &mut fout);
            opus_ifft(&st, &fout, &mut finv);
        }

        let iterations = 10000;
        let start = Instant::now();

        for _ in 0..iterations {
            opus_fft(&st, &fin, &mut fout);
            opus_ifft(&st, &fout, &mut finv);
        }

        let elapsed = start.elapsed();
        let ns_per_iter = elapsed.as_nanos() as f64 / (iterations as f64 * 2.0);

        println!("\nFFT/IFFT performance (nfft={}):", nfft);
        println!(
            "  Total time for {} FFT+IFFT pairs: {:?}",
            iterations, elapsed
        );
        println!("  Time per FFT or IFFT: {:.2} ns", ns_per_iter);
        println!(
            "  Throughput: {:.2} M points/sec",
            (nfft as f64) / ns_per_iter * 1000.0
        );
    }
}

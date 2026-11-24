// core/opus_source/quant_bands.rs original lines 517-689
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use super::range_coder::RangeCoder;

    #[test]
    fn test_coarse_fine_energy() {
        let mode = super::modes::default_mode();
        let mut e_bands = vec![0.0; mode.nb_ebands];
        for (i, v) in e_bands.iter_mut().enumerate() {
            *v = 5.0 + (i as f32 * 0.5).sin() * 2.0;
        }

        let mut old_e_bands = vec![0.0; mode.nb_ebands];
        let mut error = vec![0.0; mode.nb_ebands];
        let mut enc = RangeCoder::new_encoder(1000);

        quant_coarse_energy(
            mode,
            0,
            mode.nb_ebands,
            &e_bands,
            &mut old_e_bands,
            10000,
            &mut error,
            &mut enc,
            1,
            3,
            false,
            80,
        );

        let mut fine_quant = vec![0; mode.nb_ebands];
        for (i, v) in fine_quant.iter_mut().enumerate() {
            *v = (i % 3) as i32;
        }

        quant_fine_energy(
            mode,
            0,
            mode.nb_ebands,
            &mut old_e_bands,
            &mut error,
            &fine_quant,
            &mut enc,
            1,
        );

        let mut fine_priority = vec![0i32; mode.nb_ebands];
        for (i, v) in fine_priority.iter_mut().enumerate() {
            *v = (i % 2) as i32;
        }

        quant_energy_finalise(
            mode,
            0,
            mode.nb_ebands,
            &mut old_e_bands,
            &mut error,
            &fine_quant,
            &fine_priority,
            10,
            &mut enc,
            1,
        );

        enc.done();
        let _compressed = &enc.buf;

        let mut dec = RangeCoder::new_decoder(&enc.buf);

        let mut decoded_old_e_bands = vec![0.0; mode.nb_ebands];
        let intra = dec.decode_bit_logp(3);
        unquant_coarse_energy(
            mode,
            0,
            mode.nb_ebands,
            &mut decoded_old_e_bands,
            intra,
            &mut dec,
            1,
            3,
        );

        unquant_fine_energy(
            mode,
            0,
            mode.nb_ebands,
            &mut decoded_old_e_bands,
            &fine_quant,
            &mut dec,
            1,
        );

        unquant_energy_finalise(
            mode,
            0,
            mode.nb_ebands,
            &mut decoded_old_e_bands,
            &fine_quant,
            &fine_priority,
            10,
            &mut dec,
            1,
        );

        for i in 0..mode.nb_ebands {
            if (decoded_old_e_bands[i] - old_e_bands[i]).abs() >= 1e-5 {
                println!(
                    "Mismatch at band {}: enc={} dec={} diff={}",
                    i,
                    old_e_bands[i],
                    decoded_old_e_bands[i],
                    (decoded_old_e_bands[i] - old_e_bands[i]).abs()
                );
            }
            assert!((decoded_old_e_bands[i] - old_e_bands[i]).abs() < 1e-5);
        }
    }

    /// Regression test: extreme/corrupt energy values must not cause an
    /// "attempt to add with overflow" panic in `quant_coarse_energy_impl`.
    /// Previously `(qi0 - qi).abs()` overflowed i32 when the float->int cast
    /// of `qi` saturated near i32::MAX/MIN.
    #[test]
    fn test_coarse_energy_extreme_no_overflow() {
        let mode = super::modes::default_mode();
        let n = mode.nb_ebands;

        for &extreme in &[f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 1.0e30, -1.0e30] {
            let e_bands = vec![extreme; n];
            let mut old_e_bands = vec![0.0; n];
            let mut error = vec![0.0; n];
            let mut enc = RangeCoder::new_encoder(1000);

            // tiny budget forces the `else { qi = -1 }` path, which combined
            // with a saturated qi0 triggered the overflow at the badness line.
            quant_coarse_energy(
                mode,
                0,
                n,
                &e_bands,
                &mut old_e_bands,
                0,
                &mut error,
                &mut enc,
                1,
                3,
                false,
                80,
            );

            // large budget exercises the laplace-encode path with extreme qi.
            let mut old_e_bands2 = vec![0.0; n];
            let mut error2 = vec![0.0; n];
            let mut enc2 = RangeCoder::new_encoder(1000);
            quant_coarse_energy(
                mode,
                0,
                n,
                &e_bands,
                &mut old_e_bands2,
                10000,
                &mut error2,
                &mut enc2,
                1,
                3,
                false,
                80,
            );
        }
    }
}

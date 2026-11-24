// core/opus_source/lib.rs original lines 368-390
#[cfg(test)]
mod fec_rate_tests {
    use super::{OpusMode, compute_equiv_rate};

    #[test]
    fn fec_uses_the_selected_modes_loss_cost() {
        assert_eq!(compute_equiv_rate(20000, 1, 50, true, None, 9, 10), 18386);
        for mode in [OpusMode::SilkOnly, OpusMode::Hybrid] {
            assert_eq!(
                compute_equiv_rate(20000, 1, 50, true, Some(mode), 9, 10),
                16972
            );
            assert_eq!(
                compute_equiv_rate(16000, 2, 50, false, Some(mode), 9, 40),
                12197
            );
        }
        assert_eq!(
            compute_equiv_rate(20000, 1, 50, true, Some(OpusMode::CeltOnly), 9, 10),
            19800,
        );
    }
}
// core/opus_source/lib.rs original lines 890-1062
#[cfg(all(test, feature = "std"))]
mod silk_rate_tests {
    use super::{Bandwidth, compute_silk_rate_for_hybrid, gain_fade, hybrid_hb_gain};

    #[test]
    fn test_reference_table_exact_entries() {
        assert_eq!(
            compute_silk_rate_for_hybrid(12000, Bandwidth::Wideband, true, true, false, 1),
            10000
        );
        assert_eq!(
            compute_silk_rate_for_hybrid(16000, Bandwidth::Wideband, true, true, false, 1),
            13500
        );
        assert_eq!(
            compute_silk_rate_for_hybrid(20000, Bandwidth::Wideband, true, true, false, 1),
            16000
        );
        assert_eq!(
            compute_silk_rate_for_hybrid(24000, Bandwidth::Wideband, true, true, false, 1),
            18000
        );
        assert_eq!(
            compute_silk_rate_for_hybrid(32000, Bandwidth::Wideband, true, true, false, 1),
            22000
        );
        assert_eq!(
            compute_silk_rate_for_hybrid(64000, Bandwidth::Wideband, true, true, false, 1),
            38000
        );
    }

    #[test]
    fn test_32kbps_gives_22kbps_silk() {
        assert_eq!(
            compute_silk_rate_for_hybrid(32000, Bandwidth::Wideband, true, true, false, 1),
            22000
        );
    }

    #[test]
    fn test_interpolation_between_table_entries() {
        let r = compute_silk_rate_for_hybrid(18000, Bandwidth::Wideband, true, true, false, 1);
        assert_eq!(r, 14750);
    }

    #[test]
    fn test_above_table_max_gives_half_extra() {
        let r = compute_silk_rate_for_hybrid(72000, Bandwidth::Wideband, true, true, false, 1);
        assert_eq!(r, 38000 + (72000 - 64000) / 2);
    }

    /// Stereo looks the rate up per channel, doubles it and, from 12 kb/s per
    /// channel, takes 1 kb/s off (libopus opus_encoder.c).
    #[test]
    fn test_stereo_allocates_per_channel() {
        let stereo =
            |rate| compute_silk_rate_for_hybrid(rate, Bandwidth::Wideband, true, true, false, 2);
        assert_eq!(stereo(32000), 2 * 13500 - 1000);
        assert_eq!(stereo(24000), 2 * 10000 - 1000);
        assert_eq!(stereo(20000), 2 * 8333);
        assert_eq!(stereo(64000), 2 * 22000 - 1000);
    }

    #[test]
    fn fec_columns_and_rate_boosts_match_libopus() {
        for frame20ms in [false, true] {
            for (rate, expected) in [
                (12000, 11000),
                (16000, 15000),
                (18000, 16500),
                (20000, 18000),
                (24000, 21000),
                (32000, 28000),
                (64000, 50000),
                (72000, 54000),
            ] {
                for vbr in [false, true] {
                    for bandwidth in [Bandwidth::Fullband, Bandwidth::Superwideband] {
                        let boost = i32::from(!vbr) * 100
                            + i32::from(bandwidth == Bandwidth::Superwideband) * 300;
                        assert_eq!(
                            compute_silk_rate_for_hybrid(rate, bandwidth, frame20ms, vbr, true, 1),
                            expected + boost,
                        );
                        assert_eq!(
                            compute_silk_rate_for_hybrid(
                                rate * 2,
                                bandwidth,
                                frame20ms,
                                vbr,
                                true,
                                2
                            ),
                            (expected + boost) * 2 - 1000,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn celt_rate_follows_libopus_bitrate_ctl() {
        use super::{OpusMode, celt_bitrate};
        let max = self::celt::OPUS_BITRATE_MAX;
        assert_eq!(celt_bitrate(OpusMode::Hybrid, true, 12000, 9966, 1), 2034);
        // At or below 500 b/s CELT keeps the unlimited rate.
        assert_eq!(celt_bitrate(OpusMode::Hybrid, true, 2500, 2050, 1), max);
        assert_eq!(celt_bitrate(OpusMode::Hybrid, true, 2500, 2000, 1), max);
        assert_eq!(celt_bitrate(OpusMode::Hybrid, true, 2501, 2000, 1), 501);
        // CBR never sets a CELT rate.
        assert_eq!(celt_bitrate(OpusMode::Hybrid, false, 32000, 22000, 1), max);
        assert_eq!(celt_bitrate(OpusMode::CeltOnly, false, 64000, 0, 1), max);
        assert_eq!(celt_bitrate(OpusMode::CeltOnly, true, 64000, 0, 2), 64000);
        assert_eq!(
            celt_bitrate(OpusMode::CeltOnly, true, 2_000_000, 0, 2),
            1_500_000
        );
        assert_eq!(celt_bitrate(OpusMode::CeltOnly, true, 500, 0, 1), max);
    }

    #[test]
    fn redundancy_size_follows_libopus() {
        use super::compute_redundancy_bytes;
        // (packet bytes, b/s, frames/s, channels, libopus's size)
        for (bytes, rate, frame_rate, channels, expected) in [
            (1276, 20000, 50, 1, 27),
            (50, 20000, 50, 1, 14),
            (15, 6000, 50, 1, 0),
            (1276, 100000, 100, 2, 103),
            (1276, 500000, 50, 1, 257),
            (1276, 32000, 25, 1, 39),
            (125, 100000, 50, 1, 29),
            (25, 20000, 100, 1, 0),
            (60, 24000, 50, 2, 0),
            (1276, 8000, 50, 1, 15),
        ] {
            assert_eq!(
                compute_redundancy_bytes(bytes, rate, frame_rate, channels),
                expected,
                "{bytes} bytes, {rate} b/s, {frame_rate} frames/s, {channels} channel(s)"
            );
        }
    }

    #[test]
    fn high_band_gain_follows_the_celt_share() {
        assert_eq!(hybrid_hb_gain(0), 0.5);
        assert_eq!(hybrid_hb_gain(1024), 0.75);
        assert_eq!(hybrid_hb_gain(2048), 0.875);
        assert!(hybrid_hb_gain(8000) > 0.997);
    }

    #[test]
    fn high_band_gain_fades_over_the_overlap_at_each_api_rate() {
        for fs in [24000, 48000] {
            let overlap = fs as usize / 400;
            for channels in [1, 2] {
                let mut pcm = vec![1.0; fs as usize / 100 * channels];
                gain_fade(&mut pcm, 1.0, 0.75, channels, fs);
                assert!(pcm[0] > 0.999);
                assert_eq!(pcm[overlap * channels], 0.75);
                for samples in pcm.chunks_exact(channels) {
                    assert!(samples.iter().all(|&sample| sample == samples[0]));
                }
                assert!(pcm.windows(channels + 1).all(|w| w[0] >= w[channels]));
                gain_fade(&mut pcm, 0.75, 1.0, channels, fs);
                assert_eq!(pcm[overlap * channels], 0.75);
            }
        }
    }
}
// core/opus_source/lib.rs original lines 4814-4964
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    fn frame_size_from_toc(toc: u8, sampling_rate: i32) -> Option<usize> {
        let mode = mode_from_toc(toc);
        match mode {
            OpusMode::CeltOnly => {
                let period = ((toc >> 3) & 0x03) as i32;
                let frame_rate = 400 >> period;
                if frame_rate == 0 || sampling_rate % frame_rate != 0 {
                    return None;
                }
                Some((sampling_rate / frame_rate) as usize)
            }
            OpusMode::SilkOnly => {
                let duration_ms = frame_duration_ms_from_toc(toc);
                Some((sampling_rate as i64 * duration_ms as i64 / 1000) as usize)
            }
            OpusMode::Hybrid => {
                let duration_ms = frame_duration_ms_from_toc(toc);
                Some((sampling_rate as i64 * duration_ms as i64 / 1000) as usize)
            }
        }
    }

    #[test]
    fn gen_toc_matches_celt_reference_values() {
        let sampling_rate = 48_000;
        let cases = [
            (120usize, 0xE0u8),
            (240usize, 0xE8u8),
            (480usize, 0xF0u8),
            (960usize, 0xF8u8),
        ];

        for (frame_size, expected_toc) in cases {
            let frame_rate = frame_rate_from_params(sampling_rate, frame_size).unwrap();
            let toc = gen_toc(OpusMode::CeltOnly, frame_rate, Bandwidth::Fullband, 1);
            assert_eq!(
                toc, expected_toc,
                "frame_size {} expected TOC {:02X} got {:02X}",
                frame_size, expected_toc, toc
            );
            let decoded_size = frame_size_from_toc(toc, sampling_rate).unwrap();
            assert_eq!(decoded_size, frame_size);
        }

        let stereo_toc = gen_toc(
            OpusMode::CeltOnly,
            frame_rate_from_params(sampling_rate, 960).unwrap(),
            Bandwidth::Fullband,
            2,
        );
        assert_eq!(channels_from_toc(stereo_toc), 2);
    }

    #[test]
    fn test_celt_decoder_large_frame_sizes() {
        let sampling_rate = 48000;
        let channels = 1;

        let mut decoder = OpusDecoder::new(sampling_rate, channels).unwrap();

        let frame_sizes = [120, 240, 480, 960];

        for frame_size in frame_sizes {
            let toc = gen_toc(
                OpusMode::CeltOnly,
                frame_rate_from_params(sampling_rate, frame_size).unwrap(),
                Bandwidth::Fullband,
                channels,
            );
            let packet = [toc, 0, 0, 0, 0];

            let mut output = vec![0.0f32; frame_size * channels];

            let _ = decoder.decode(&packet, frame_size, &mut output);
        }

        let channels = 2;
        let mut decoder = OpusDecoder::new(sampling_rate, channels).unwrap();

        for frame_size in frame_sizes {
            let toc = gen_toc(
                OpusMode::CeltOnly,
                frame_rate_from_params(sampling_rate, frame_size).unwrap(),
                Bandwidth::Fullband,
                channels,
            );
            let packet = [toc, 0, 0, 0, 0];

            let mut output = vec![0.0f32; frame_size * channels];
            let _ = decoder.decode(&packet, frame_size, &mut output);
        }
    }

    #[test]
    fn test_celt_decoder_edge_case_frame_sizes() {
        let sampling_rate = 48000;
        let channels = 1;
        let mut decoder = OpusDecoder::new(sampling_rate, channels).unwrap();

        let edge_sizes = [2048, 2167, 2168, 2169, 2880, 3072];

        for frame_size in edge_sizes {
            let mut output = vec![0.0f32; frame_size * channels];

            let _ = decoder.decode(&[0x80, 0, 0, 0], frame_size, &mut output);
        }
    }

    // Regression test for: "index out of bounds: the len is 48 but the index is 119"
    // Root cause: frame_size=48 at 48kHz gives frame_rate=1000, which is not a valid
    // Hybrid-mode frame rate but was not validated.  CELT's lm-search then silently
    // fell back to lm=0, computed n2=120, and wrote output[119] into a 48-element
    // slice.  Triggered via G.729-decoded PCM (8kHz) passed to a 48kHz Opus encoder
    // without proper resampling, so the encoder received 48 samples instead of 480.
    #[test]
    fn test_invalid_small_frame_size_returns_error_not_panic() {
        let mut enc = OpusEncoder::new(48000, 2, Application::Voip).unwrap();
        enc.bitrate_bps = 64000;
        enc.complexity = 5;
        enc.use_cbr = true;

        // 48 samples at 48kHz = 1ms → frame_rate=1000, invalid for Hybrid mode.
        let input = vec![0.0f32; 48 * 2]; // stereo interleaved
        let mut output = vec![0u8; 256];

        let result = enc.encode(&input, 48, &mut output);
        assert!(
            result.is_err(),
            "encode with invalid frame_size=48 should return Err, not panic"
        );
    }

    // Also verify that the Audio application path (always Hybrid at 48 kHz) rejects
    // the same bad frame size.
    #[test]
    fn test_invalid_small_frame_size_audio_application_returns_error() {
        let mut enc = OpusEncoder::new(48000, 1, Application::Audio).unwrap();
        let input = vec![0.0f32; 48];
        let mut output = vec![0u8; 256];

        let result = enc.encode(&input, 48, &mut output);
        assert!(
            result.is_err(),
            "Audio/48kHz encoder with frame_size=48 should return Err"
        );
    }
}

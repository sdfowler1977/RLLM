// core/opus_source/silk/enc_api.rs original lines 1253-1343
#[cfg(test)]
mod tests {
    use super::*;

    /// An encoder coding `packet_size_ms` packets of `n_frames` frames, with
    /// `n_frames_encoded` of them coded so far.
    fn encoder(packet_size_ms: i32, n_frames: i32, n_frames_encoded: i32) -> SilkEncoderState {
        let mut enc = SilkEncoderState::default();
        enc.s_cmn.packet_size_ms = packet_size_ms;
        enc.s_cmn.n_frames_per_packet = n_frames;
        enc.s_cmn.n_frames_encoded = n_frames_encoded;
        enc
    }

    #[test]
    fn frame_target_takes_out_the_lbrr_average() {
        let mut enc = encoder(20, 1, 0);
        // No LBRR: the packet's 640 bits.
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 0, 0), 32000);
        // The first LBRR packet isn't averaged: (640 - 200) * 50.
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 200, 0), 22000);
        assert_eq!(enc.n_bits_used_lbrr, 200);
        // Then the average: (200 + 100) / 2 = 150, (640 - 150) * 50.
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 100, 0), 24500);
        assert_eq!(enc.n_bits_used_lbrr, 150);
        // Under 10 bits counts as no LBRR, which drops the average at once.
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 9, 0), 32000);
        assert_eq!(enc.n_bits_used_lbrr, 0);
    }

    #[test]
    fn frame_target_pays_back_the_reservoir() {
        let mut enc = encoder(20, 1, 0);
        enc.n_bits_exceeded = 1000;
        // 1000 bits over 500 ms: 2000 b/s off.
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 0, 0), 30000);
        // 10 ms packets: (320 bits) * 100, less the same payback.
        let mut enc = encoder(10, 1, 0);
        enc.n_bits_exceeded = 1000;
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 0, 0), 30000);
    }

    #[test]
    fn later_frames_count_the_lbrr_section_as_overshoot() {
        // 40 ms at 32 kb/s: 1280 bits in two frames.
        let mut enc = encoder(40, 2, 0);
        // Frame 0: (1280 - 200) / 2 = 540 bits.
        assert_eq!(
            silk_frame_target_rate_bps(&mut enc, 32000, 200, 2 + 200),
            27000
        );
        // Frame 1 drops the LBRR average (libopus resets `curr_nBitsUsedLBRR`
        // per frame): 640 bits a frame, and the 2 flag bits, the 200-bit
        // section and frame 0's 540 bits are 102 over 640: 204 b/s off.
        enc.s_cmn.n_frames_encoded = 1;
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 0, 742), 31796);
        assert_eq!(enc.n_bits_used_lbrr, 0);
    }

    #[test]
    fn frame_target_stays_within_5000_and_the_input_rate() {
        // An LBRR section nearly as large as the packet: floored at 5000.
        let mut enc = encoder(20, 1, 0);
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 600, 0), 5000);
        // Frames under budget so far raise the target, but never above the
        // input rate.
        let mut enc = encoder(40, 2, 1);
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 32000, 0, 100), 32000);
        // Below 5000 the bounds swap, as in silk_LIMIT: 4000 stays 4000, and
        // a payback below the input rate stops at it.
        let mut enc = encoder(20, 1, 0);
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 4000, 0, 0), 4000);
        enc.n_bits_exceeded = 100;
        assert_eq!(silk_frame_target_rate_bps(&mut enc, 4000, 0, 0), 4000);
    }

    #[test]
    fn bit_reservoir_keeps_the_overshoot_within_0_and_10000() {
        // 32 kb/s at 20 ms: 640 bits a packet.
        let mut enc = encoder(20, 1, 0);
        silk_update_bit_reservoir(&mut enc, 32000, 90);
        assert_eq!(enc.n_bits_exceeded, 80);
        silk_update_bit_reservoir(&mut enc, 32000, 70);
        assert_eq!(enc.n_bits_exceeded, 0);
        silk_update_bit_reservoir(&mut enc, 32000, 70);
        assert_eq!(enc.n_bits_exceeded, 0, "an undershoot isn't banked");
        enc.n_bits_exceeded = 9900;
        silk_update_bit_reservoir(&mut enc, 32000, 200);
        assert_eq!(enc.n_bits_exceeded, 10000);
    }
}

// core/opus_source/silk/dec_api.rs original lines 460-518
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_creation() {
        let dec = SilkDecoder::new();
        assert_eq!(dec.n_channels_api, 1);
        assert_eq!(dec.n_channels_internal, 1);
    }

    #[test]
    fn test_decoder_init() {
        let mut dec = SilkDecoder::new();

        let ret = dec.init(16000, 1);
        assert_eq!(ret, 0);
        assert_eq!(dec.sample_rate(), 16000);
    }

    #[test]
    fn test_decoder_16khz() {
        let mut dec = SilkDecoder::new();
        let ret = dec.init(16000, 1);
        assert_eq!(ret, 0);
        assert_eq!(dec.frame_length(), 320);
    }

    /// Conceal one lost 20 ms stereo frame after a good frame whose mid-only
    /// flag was `prev_mid_only`, and return the decoder.
    fn conceal_after(prev_mid_only: i32) -> SilkDecoder {
        let mut dec = SilkDecoder::new();
        assert_eq!(dec.init(16000, 2), 0);
        dec.prev_decode_only_middle = prev_mid_only;
        let mut out = [0i16; 2 * 320];
        let mut rc = RangeCoder::new_decoder(&[]);
        let n = dec.decode(&mut rc, &mut out, FLAG_PACKET_LOST, true, 20, 16000);
        assert_eq!(n, 320);
        dec
    }

    /// libopus dec_API.c: a lost frame conceals the side channel only if
    /// the last good frame coded one, and leaves the mid-only flag alone.
    #[test]
    fn a_lost_frame_after_a_mid_only_frame_conceals_the_mid_only() {
        let dec = conceal_after(1);
        assert_eq!(dec.prev_decode_only_middle, 1);
        assert_eq!(dec.channel_state[0].loss_cnt, 1);
        assert_eq!(dec.channel_state[1].loss_cnt, 0, "side was concealed");
    }

    #[test]
    fn a_lost_frame_after_a_side_coded_frame_conceals_both() {
        let dec = conceal_after(0);
        assert_eq!(dec.prev_decode_only_middle, 0);
        assert_eq!(dec.channel_state[0].loss_cnt, 1);
        assert_eq!(dec.channel_state[1].loss_cnt, 1);
    }
}

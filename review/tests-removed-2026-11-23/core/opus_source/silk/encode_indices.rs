// core/opus_source/silk/encode_indices.rs original lines 209-232
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use super::super::silk::decode_indices::{silk_stereo_decode_mid_only, silk_stereo_decode_pred};

    fn round_trip(only_middle: i8) -> ([i32; 2], bool) {
        let mut enc = RangeCoder::new_encoder(64);
        silk_encode_stereo(&mut enc, 0, 0, only_middle);
        let bytes = enc.finish();
        let mut dec = RangeCoder::new_decoder(&bytes);
        let pred = silk_stereo_decode_pred(&mut dec);
        (pred, silk_stereo_decode_mid_only(&mut dec))
    }

    #[test]
    fn stereo_header_is_neutral_mid_only() {
        assert_eq!(round_trip(1), ([0, 0], true));
    }

    #[test]
    fn stereo_header_keeps_the_mid_only_flag() {
        assert_eq!(round_trip(0), ([0, 0], false));
    }
}

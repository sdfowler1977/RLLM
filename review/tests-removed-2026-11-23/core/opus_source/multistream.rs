// core/opus_source/multistream.rs original lines 252-408
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    fn toc_byte(config: u8, stereo: bool, code: u8) -> u8 {
        (config << 3) | ((stereo as u8) << 2) | (code & 0x03)
    }

    #[test]
    fn mapping_table_parses_5_1_shape() {
        let tail = [4u8, 2, 0, 1, 4, 5, 2, 3];
        let table = ChannelMappingTable::parse(1, 6, &tail).unwrap();
        assert_eq!(table.stream_count, 4);
        assert_eq!(table.coupled_count, 2);
        assert_eq!(table.mapping, vec![0, 1, 4, 5, 2, 3]);
        assert_eq!(table.decoded_channel_count(), 6);
    }

    #[test]
    fn mapping_family_zero_has_no_table() {
        assert!(ChannelMappingTable::parse(0, 2, &[9, 9, 9, 9]).is_none());
    }

    #[test]
    fn mapping_table_rejects_truncated_tail() {
        assert!(ChannelMappingTable::parse(1, 6, &[4, 2, 0, 1]).is_none());
    }

    #[test]
    fn split_code0_single_frame() {
        let bytes = [toc_byte(0, false, 0), 3, 0xAA, 0xBB, 0xCC, 0xFF];
        let mut scratch = Vec::new();
        let (pkt, consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
        assert_eq!(consumed, 1 + 1 + 3);
        // reconstructed packet excises the length byte: TOC + frame data only
        assert_eq!(pkt, &[toc_byte(0, false, 0), 0xAA, 0xBB, 0xCC][..]);
    }

    #[test]
    fn split_code0_two_byte_length() {
        let mut bytes = vec![toc_byte(0, false, 0), 252, 0];
        bytes.extend(std::iter::repeat_n(0x7Fu8, 252));
        let mut scratch = Vec::new();
        let (_, consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
        assert_eq!(consumed, 1 + 2 + 252);
    }

    #[test]
    fn split_code1_equal_frames() {
        let bytes = [toc_byte(0, false, 1), 2, 0xAA, 0xBB, 0xCC, 0xDD, 0x11];
        let mut scratch = Vec::new();
        let (_, consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
        assert_eq!(consumed, 1 + 1 + 4);
    }

    #[test]
    fn split_code2_two_lengths() {
        let bytes = [toc_byte(0, false, 2), 3, 2, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
        let mut scratch = Vec::new();
        let (pkt, consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
        assert_eq!(consumed, 1 + 1 + 1 + 3 + 2);
        // inline length (3, for frame 1) is kept; the extra Appendix B
        // length (2, for frame 2) is excised
        assert_eq!(pkt, &[toc_byte(0, false, 2), 3, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE][..]);
    }

    #[test]
    fn split_code3_cbr_three_frames() {
        let bytes = [toc_byte(0, false, 3), 3, 2, 0x10, 0x11, 0x20, 0x21, 0x30, 0x31];
        let mut scratch = Vec::new();
        let (_, consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
        assert_eq!(consumed, 1 + 1 + 1 + 6);
    }

    #[test]
    fn split_code3_cbr_with_padding() {
        let fc = 0x40 | 2u8;
        let bytes = [
            toc_byte(0, false, 3), fc, 2, 3,
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
            0x55, 0x66,
        ];
        let mut scratch = Vec::new();
        let (_, consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
        assert_eq!(consumed, 12);
    }

    #[test]
    fn split_code3_vbr_three_frames() {
        let fc = 0x80 | 3u8;
        let bytes = [
            toc_byte(0, false, 3), fc, 2, 3, 1,
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
        ];
        let mut scratch = Vec::new();
        let (pkt, consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
        assert_eq!(consumed, 1 + 1 + 1 + 1 + 1 + 2 + 3 + 1);
        // inline lengths for frames 1-2 (2, 3) are kept; the extra
        // Appendix B length for frame 3 (1) is excised
        assert_eq!(pkt, &[toc_byte(0, false, 3), fc, 2, 3, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF][..]);
    }

    #[test]
    fn split_two_streams_chained() {
        let bytes = [
            toc_byte(0, false, 0), 2, 0xA0, 0xA1,
            toc_byte(0, false, 1), 1, 0xB0, 0xB1,
        ];
        let mut scratch = Vec::new();
        let a_consumed = {
            let (a, a_consumed) = split_self_delimited(&bytes, &mut scratch).unwrap();
            // reconstructed: TOC + frame data, length byte excised
            assert_eq!(a, &[toc_byte(0, false, 0), 0xA0, 0xA1][..]);
            a_consumed
        };
        assert_eq!(a_consumed, 4);
        let (_, b_consumed) = split_self_delimited(&bytes[a_consumed..], &mut scratch).unwrap();
        assert_eq!(b_consumed, 4);
    }

    #[test]
    fn split_truncated_frame_is_error() {
        let bytes = [toc_byte(0, false, 0), 4, 0xAA, 0xBB];
        let mut scratch = Vec::new();
        assert!(split_self_delimited(&bytes, &mut scratch).is_err());
    }

    #[test]
    fn split_code3_zero_frames_is_error() {
        let bytes = [toc_byte(0, false, 3), 0, 1, 0xAA];
        let mut scratch = Vec::new();
        assert!(split_self_delimited(&bytes, &mut scratch).is_err());
    }

    #[test]
    fn multistream_decoder_rejects_bad_coupled_count() {
        let table = ChannelMappingTable {
            channels: 2,
            stream_count: 1,
            coupled_count: 2,
            mapping: vec![0, 1],
        };
        assert!(MultistreamDecoder::new(48000, table).is_err());
    }

    #[test]
    fn multistream_decoder_builds_expected_stream_shape() {
        let table = ChannelMappingTable {
            channels: 6,
            stream_count: 4,
            coupled_count: 2,
            mapping: vec![0, 1, 4, 5, 2, 3],
        };
        let ms = MultistreamDecoder::new(48000, table).unwrap();
        assert_eq!(ms.decoders.len(), 4);
    }
}
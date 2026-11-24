// core/opus_source/range_coder.rs original lines 669-798
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_laplace() {
        let mut enc = RangeCoder::new_encoder(100);
        let mut val = -3;
        let fs = 100 << 7;
        let decay = 120 << 6;
        enc.laplace_encode(&mut val, fs, decay);
        enc.done();

        assert_eq!(enc.offs, 1);
        assert_eq!(enc.buf[0], 224);

        let mut dec = RangeCoder::new_decoder(&enc.buf[..enc.offs as usize]);
        let decoded_val = dec.laplace_decode(fs, decay);
        assert_eq!(decoded_val, -3);
    }

    #[test]
    fn test_icdf_consistency() {
        let mut enc = RangeCoder::new_encoder(1024);
        let icdf = [2, 1, 0];
        enc.encode_icdf(0, &icdf, 2);
        enc.encode_icdf(1, &icdf, 2);
        enc.encode_icdf(2, &icdf, 2);
        enc.done();
        let data = enc.buf[..enc.offs as usize].to_vec();

        let mut dec = RangeCoder::new_decoder(&data);
        let s0 = dec.decode_icdf(&icdf, 2);
        let s1 = dec.decode_icdf(&icdf, 2);
        let s2 = dec.decode_icdf(&icdf, 2);

        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
    }

    #[test]
    fn test_icdf_last_symbol_no_oob() {
        let icdf: &[u8] = &[170, 85, 0];
        let ftb = 8u32;

        for sym in 0..3i32 {
            let mut enc = RangeCoder::new_encoder(256);
            enc.encode_icdf(sym, icdf, ftb);
            enc.done();
            let data = enc.buf[..enc.offs as usize].to_vec();

            let mut dec = RangeCoder::new_decoder(&data);
            let decoded = dec.decode_icdf(icdf, ftb);
            assert_eq!(decoded, sym, "往返失败: 编码 symbol={sym} 解码得 {decoded}");
        }
    }

    #[test]
    fn test_icdf_decode_terminates() {
        let icdf: &[u8] = &[192, 128, 64, 0];
        let ftb = 8u32;

        let symbols = [0i32, 1, 2, 3];
        let mut enc = RangeCoder::new_encoder(256);
        for &s in &symbols {
            enc.encode_icdf(s, icdf, ftb);
        }
        enc.done();
        let data = enc.buf[..enc.offs as usize].to_vec();

        let mut dec = RangeCoder::new_decoder(&data);
        for &expected in &symbols {
            let got = dec.decode_icdf(icdf, ftb);
            assert_eq!(got, expected, "解码器输出 {got}，期望 {expected}");
        }
    }

    #[test]
    fn test_bits_only() {
        let mut enc = RangeCoder::new_encoder(1024);

        enc.enc_bits(1, 1);
        enc.enc_bits(5, 3);
        enc.enc_bits(7, 3);
        enc.enc_bits(0, 2);

        let data = enc.finish();
        let mut dec = RangeCoder::new_decoder(&data);

        let b1 = dec.dec_bits(1);
        let b2 = dec.dec_bits(3);
        let b3 = dec.dec_bits(3);
        let b4 = dec.dec_bits(2);

        assert_eq!(b1, 1);
        assert_eq!(b2, 5);
        assert_eq!(b3, 7);
        assert_eq!(b4, 0);
    }

    #[test]
    fn test_interleaved_bits_entropy() {
        let mut enc = RangeCoder::new_encoder(1024);

        enc.enc_bits(1, 1);

        enc.encode(10, 20, 100);

        enc.enc_bits(5, 3);

        enc.encode(50, 60, 100);

        let data = enc.finish();

        let mut dec = RangeCoder::new_decoder(&data);

        let b1 = dec.dec_bits(1);
        let d1 = dec.decode(100);
        dec.update(10, 20, 100);
        let b2 = dec.dec_bits(3);
        let d2 = dec.decode(100);
        dec.update(50, 60, 100);

        assert_eq!(b1, 1);
        assert!((10..20).contains(&d1), "d1={} expected in [10, 20)", d1);
        assert_eq!(b2, 5);
        assert!((50..60).contains(&d2), "d2={} expected in [50, 60)", d2);
    }
}

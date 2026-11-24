// core/opus_source/bands.rs original lines 3012-3027
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_bitexact_primitives_reference_values() {
        assert_eq!(bitexact_cos(64), 32767);
        assert_eq!(bitexact_cos(8192), 23171);
        assert_eq!(bitexact_cos(16320), 200);

        assert_eq!(bitexact_log2tan(32767, 200), 15059);
        assert_eq!(bitexact_log2tan(30274, 12540), 2611);
        assert_eq!(bitexact_log2tan(23171, 23171), 0);
        assert_eq!(bitexact_log2tan(200, 32767), -15059);
    }
}

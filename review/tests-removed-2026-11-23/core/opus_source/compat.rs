// core/opus_source/compat.rs original lines 411-421
#[cfg(all(test, feature = "std", target_arch = "x86_64"))]
mod tests {
    use super::*;

    #[test]
    fn fma_probes_require_both_features() {
        let fma = std::arch::is_x86_feature_detected!("fma");
        assert_eq!(x86_has_avx_fma(), x86_has_avx() && fma);
        assert_eq!(x86_has_avx2_fma(), x86_has_avx2() && fma);
    }
}

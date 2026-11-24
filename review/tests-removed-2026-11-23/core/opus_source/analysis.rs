// core/opus_source/analysis.rs original lines 1020-1051
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float2int_rounds_ties_to_even() {
        for (x, r) in [
            (0.5f32, 0),
            (1.5, 2),
            (2.5, 2),
            (-0.5, 0),
            (-1.5, -2),
            (0.49, 0),
            (-0.51, -1),
        ] {
            assert_eq!(float2int(x), r, "{x}");
        }
    }

    #[test]
    fn fast_atan2f_follows_atan2() {
        for (y, x) in [
            (1.0f32, 1.0f32),
            (-1.0, 2.0),
            (3.0, -0.5),
            (-0.2, -4.0),
            (0.0, 1.0),
        ] {
            assert!((fast_atan2f(y, x) - y.atan2(x)).abs() < 1e-4, "{y} {x}");
        }
    }
}

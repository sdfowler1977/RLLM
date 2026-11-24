// core/opus_source/mlp.rs original lines 168-181
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tansig_is_bounded_and_odd() {
        for x in [-20.0f32, -3.0, -0.5, 0.0, 0.25, 2.0, 50.0] {
            let y = tansig_approx(x);
            assert!((-1.0..=1.0).contains(&y));
            assert_eq!(y, -tansig_approx(-x));
        }
        assert_eq!(tansig_approx(0.0), 0.0);
    }
}

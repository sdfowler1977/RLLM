//! Eight Goertzel bins of little-endian signed 16-bit mono PCM.
//! The samples are not played.

const NOT_ENOUGH: &str = "not enough samples. The samples were not played.";
const BIN_COUNT: u32 = 8;

pub fn spectrum_bins(pcm: &[u8], rate: u32) -> String {
    if pcm.len() / 2 < 16 {
        return NOT_ENOUGH.to_string();
    }
    let mut lines = Vec::with_capacity(BIN_COUNT as usize + 1);
    for index in 1..=BIN_COUNT {
        let hz = bin_hz(rate, index);
        let energy = magnitude_integer(goertzel(pcm, rate, hz));
        lines.push(format!("bin {hz} energy {energy}"));
    }
    lines.push("The samples were not played.".to_string());
    lines.join("\n")
}

/// Frequency of bin `index` (1..=8): equal integer steps from rate/16 through rate/2.
fn bin_hz(rate: u32, index: u32) -> u32 {
    ((u64::from(index) * u64::from(rate)) / 16) as u32
}

fn goertzel(pcm: &[u8], rate: u32, freq: u32) -> f64 {
    let coeff = bounded_coefficient(rate, freq);
    let mut prev = 0.0;
    let mut prev2 = 0.0;
    for pair in pcm.chunks_exact(2) {
        let sample = i16::from_le_bytes([pair[0], pair[1]]) as f64;
        let next = sample + coeff * prev - prev2;
        prev2 = prev;
        prev = next;
    }
    let power = prev * prev + prev2 * prev2 - coeff * prev * prev2;
    if power.is_finite() && power > 0.0 {
        power.sqrt()
    } else {
        0.0
    }
}

/// 2*cos(2*pi*f/rate), clamped to the cosine range [-2, 2].
fn bounded_coefficient(rate: u32, freq: u32) -> f64 {
    if rate == 0 {
        return 2.0;
    }
    let theta = 2.0 * std::f64::consts::PI * (freq as f64) / (rate as f64);
    let coeff = 2.0 * theta.cos();
    if coeff.is_finite() {
        coeff.clamp(-2.0, 2.0)
    } else {
        0.0
    }
}

fn magnitude_integer(magnitude: f64) -> u64 {
    if !magnitude.is_finite() || magnitude <= 0.0 {
        return 0;
    }
    let rounded = magnitude.round();
    if rounded >= u64::MAX as f64 {
        u64::MAX
    } else {
        rounded as u64
    }
}

#[cfg(test)]
mod tests {
    use super::spectrum_bins;

    fn listed_bins(report: &str) -> Vec<(u32, u64)> {
        assert!(
            report.ends_with("The samples were not played."),
            "{report}"
        );
        let mut bins = Vec::new();
        for line in report.lines() {
            if line == "The samples were not played." {
                continue;
            }
            let rest = line.strip_prefix("bin ").unwrap_or_else(|| panic!("{line}"));
            let (hz, energy) = rest
                .split_once(" energy ")
                .unwrap_or_else(|| panic!("{line}"));
            bins.push((
                hz.parse().unwrap_or_else(|_| panic!("{hz}")),
                energy.parse().unwrap_or_else(|_| panic!("{energy}")),
            ));
        }
        bins
    }

    #[test]
    fn silence_is_energy_zero() {
        let rate = 16_000u32;
        let report = spectrum_bins(&[0u8; 32], rate);
        let bins = listed_bins(&report);
        assert_eq!(bins.len(), 8, "{report}");
        let expected: Vec<(u32, u64)> = (1..=8).map(|index| (index * rate / 16, 0)).collect();
        assert_eq!(bins, expected, "{report}");
        assert!(!report.contains("not enough samples"), "{report}");
        let zero_rate = spectrum_bins(&[0u8; 32], 0);
        let zero_bins = listed_bins(&zero_rate);
        assert_eq!(zero_bins, vec![(0, 0); 8], "{zero_rate}");
    }

    #[test]
    fn tone_near_one_bin_is_largest() {
        let rate = 16_000u32;
        let near = 4_050u32;
        let count = 160usize;
        let amplitude = 8_000.0f64;
        let mut pcm = Vec::with_capacity(count * 2);
        for index in 0..count {
            let angle = 2.0 * std::f64::consts::PI * (near as f64) * (index as f64) / (rate as f64);
            let sample = (amplitude * angle.sin()).round() as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        let report = spectrum_bins(&pcm, rate);
        let bins = listed_bins(&report);
        assert_eq!(bins.len(), 8, "{report}");
        let (best_hz, best_energy) = bins
            .iter()
            .copied()
            .max_by_key(|(_, energy)| *energy)
            .expect("eight bins");
        let nearest = bins
            .iter()
            .map(|(hz, _)| *hz)
            .min_by_key(|hz| hz.abs_diff(near))
            .expect("eight bins");
        assert_eq!(best_hz, nearest, "{report}");
        assert!(best_energy > 0, "{report}");
        assert_eq!(
            bins.iter().filter(|(_, energy)| *energy == best_energy).count(),
            1,
            "{report}"
        );
    }

    #[test]
    fn short_buffer_is_refused() {
        let expected = "not enough samples. The samples were not played.";
        assert_eq!(spectrum_bins(&[], 16_000), expected);
        assert_eq!(spectrum_bins(&[0u8; 30], 16_000), expected);
        assert_eq!(spectrum_bins(&[1u8; 31], 44_100), expected);
    }
}

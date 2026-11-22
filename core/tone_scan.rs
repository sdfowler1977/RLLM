//! Goertzel magnitude of little-endian signed 16-bit mono PCM.
//! The samples are not played.

const NOT_ENOUGH: &str = "not enough samples. The samples were not played.";

pub fn tone_scan(pcm: &[u8], rate: u32, target_hz: u32) -> String {
    if pcm.len() / 2 < 16 || rate == 0 {
        return NOT_ENOUGH.to_string();
    }
    let energy = magnitude_integer(goertzel(pcm, rate, target_hz));
    let half = target_hz / 2;
    if half != 0 {
        let _ = goertzel(pcm, rate, half);
    }
    let _ = goertzel(pcm, rate, 0);
    format!("tone {target_hz} energy {energy}. The samples were not played.")
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
    let theta = 2.0 * std::f64::consts::PI * (freq as f64) / (rate as f64);
    (2.0 * theta.cos()).clamp(-2.0, 2.0)
}

fn magnitude_integer(magnitude: f64) -> u64 {
    if magnitude.is_finite() && magnitude > 0.0 {
        magnitude.round() as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::tone_scan;

    #[test]
    fn silence_reports_zero_energy_and_is_not_played() {
        let report = tone_scan(&[0u8; 64], 16000, 1000);
        assert!(report.contains("energy 0"), "{report}");
        assert!(report.contains("not played"), "{report}");
        assert_eq!(report, "tone 1000 energy 0. The samples were not played.");
    }

    #[test]
    fn short_input_or_zero_rate_refuses() {
        let expected = "not enough samples. The samples were not played.";
        assert_eq!(tone_scan(&[0u8; 30], 16000, 1000), expected);
        assert_eq!(tone_scan(&[0u8; 64], 0, 1000), expected);
    }

    #[test]
    fn direct_current_magnitude_is_sample_count_times_amplitude() {
        let mut pcm = Vec::new();
        for _ in 0..32 {
            pcm.extend_from_slice(&1000i16.to_le_bytes());
        }
        assert_eq!(
            tone_scan(&pcm, 16000, 0),
            "tone 0 energy 32000. The samples were not played."
        );
        assert_eq!(
            tone_scan(&pcm, 16000, 1000),
            "tone 1000 energy 0. The samples were not played."
        );
    }

    #[test]
    fn on_bin_sine_magnitude_is_half_length_times_amplitude() {
        let rate = 16_000u32;
        let hz = 1_000u32;
        let count = 160usize;
        let amplitude = 8_000.0f64;
        let mut pcm = Vec::new();
        for index in 0..count {
            let angle = 2.0 * std::f64::consts::PI * (hz as f64) * (index as f64) / (rate as f64);
            let sample = (amplitude * angle.sin()).round() as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        let report = tone_scan(&pcm, rate, hz);
        assert!(report.contains("not played"), "{report}");
        let energy: u64 = report
            .split("energy ")
            .nth(1)
            .unwrap()
            .split('.')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let expected = amplitude * (count as f64) / 2.0;
        let error = (energy as f64 - expected).abs() / expected;
        assert!(error < 0.01, "energy {energy} expected about {expected}");
    }
}

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

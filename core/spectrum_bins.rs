//! Nine Goertzel bins of little-endian signed 16-bit mono PCM.
//! Calculated bins are 0 Hz and equal integer steps from rate/16 through rate/2.
//! The next bin, at rate/32, is not yet calculated. Every other frequency stays unavailable.
//! The samples are not played.

const NOT_ENOUGH: &str = "not enough samples. The samples were not played.";
const BIN_COUNT: u32 = 9;

pub fn spectrum_bins(pcm: &[u8], rate: u32) -> String {
    if pcm.len() / 2 < 16 {
        return NOT_ENOUGH.to_string();
    }
    let mut lines = Vec::with_capacity(BIN_COUNT as usize + 1);
    for index in 0..BIN_COUNT {
        let hz = bin_hz(rate, index);
        let energy = magnitude_integer(goertzel(pcm, rate, hz));
        lines.push(format!("bin {hz} energy {energy}"));
    }
    lines.push("The samples were not played.".to_string());
    lines.join("\n")
}

/// Frequency of bin `index` (0..=8): 0 Hz, then equal integer steps from rate/16 through rate/2.
/// The next bin, at rate/32, is not yet calculated. Every other frequency stays unavailable.
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

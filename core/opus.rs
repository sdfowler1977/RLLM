//! Real Opus audio for the open-microphone path.
//!
//! The speech peer on /api/chat reads and writes standard Opus inside an Ogg
//! Opus stream: every audio message carries one Ogg page, and the pages hold
//! Opus packets that any standard decoder must be able to play. The peer
//! parses the pages of the stream it receives, skips the identification and
//! comment pages, and hands every other packet to its Opus decoder. The same
//! holds for the audio the peer returns, so this module reads and writes the
//! container as well as the codec.
//!
//! The codec itself lives in core/opus_source: a vendored copy of the
//! pure-Rust Opus implementation published as the `opus-rs` crate, version
//! 0.1.42, which carries the Opus reference software under the BSD 3-Clause
//! licence. The upstream notices are kept in those files, as the licence
//! requires. The vendored copy is adapted in two ways only, so that it builds
//! with the product's plain compiler invocation: the crate root paths became
//! module paths, and the optional feature switches became fixed choices
//! (standard library and heap on, the other options off). The framing, the Ogg
//! container, the product interface and the range handling below are written
//! for this product.

#[path = "opus_source/lib.rs"]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::needless_range_loop)]
#[allow(dead_code)]
#[allow(unused_imports)]
mod source;

use source::{Application, OpusDecoder, OpusEncoder};

/// Samples in one 20 ms hop at the product rate.
pub const FRAME: usize = 480;

/// Product sample rate of one hop.
pub const RATE: usize = 24_000;

/// Rate the container counts its granule positions in.
const GRANULE_RATE: usize = 48_000;

/// Samples one packet of the product carries, counted at the container rate.
const GRANULE_STEP: u64 = (GRANULE_RATE / 1000 * 20) as u64;

/// Largest Opus packet the format allows.
const PACKET_LIMIT: usize = 1276;

/// Largest number of samples one packet can produce at the product rate.
const DECODE_LIMIT: usize = RATE / 2;

/// Samples in one hop.
pub const fn frame_samples() -> usize {
    FRAME
}

/// Product sample rate of one hop.
pub const fn frame_rate() -> usize {
    RATE
}

/// Encoder of speech frames.
pub struct Encoder {
    inner: Option<OpusEncoder>,
    error: Option<String>,
}

/// Decoder of speech frames.
pub struct Decoder {
    inner: Option<OpusDecoder>,
    error: Option<String>,
    stream: OggReader,
    skip: usize,
    skip_set: bool,
}

impl Encoder {
    /// New encoder for one channel of speech at the product rate.
    pub fn new() -> Self {
        match OpusEncoder::new(RATE as i32, 1, Application::Voip) {
            Ok(inner) => Self { inner: Some(inner), error: None },
            Err(error) => Self { inner: None, error: Some(format!("Opus encoder is not available: {error}")) },
        }
    }

    /// The encoder's delay in samples at the product rate. The Ogg header of
    /// the stream declares it as the pre-skip of the stream.
    pub fn lookahead(&self) -> usize {
        self.inner.as_ref().map(|inner| inner.lookahead()).unwrap_or(0)
    }

    /// The pre-skip the identification page of a stream declares for this
    /// encoder, in samples at the container rate.
    pub fn pre_skip(&self) -> u16 {
        (self.lookahead() * GRANULE_RATE / RATE).min(u16::MAX as usize) as u16
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// New decoder for one channel of speech at the product rate.
    pub fn new() -> Self {
        match OpusDecoder::new(RATE as i32, 1) {
            Ok(inner) => Self { inner: Some(inner), error: None, stream: OggReader::new(), skip: 0, skip_set: false },
            Err(error) => Self {
                inner: None,
                error: Some(format!("Opus decoder is not available: {error}")),
                stream: OggReader::new(),
                skip: 0,
                skip_set: false,
            },
        }
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode one 20 ms frame of speech at 24 kHz into one Opus packet.
pub fn encode(encoder: &mut Encoder, pcm: &[i16]) -> Result<Vec<u8>, String> {
    if pcm.len() != FRAME {
        return Err("A speech frame is 480 samples at 24 kHz".into());
    }
    if let Some(error) = &encoder.error {
        return Err(error.clone());
    }
    let inner = encoder.inner.as_mut().ok_or("Opus encoder is not available")?;
    let mut packet = vec![0u8; PACKET_LIMIT];
    let size = inner.encode_i16(pcm, FRAME, &mut packet).map_err(|error| format!("Opus frame was not coded: {error}"))?;
    if size == 0 || size > PACKET_LIMIT {
        return Err(format!("Opus frame has an unusable length of {size} bytes"));
    }
    packet.truncate(size);
    Ok(packet)
}

/// Decode one Opus packet, or the pages of an Ogg Opus stream, into speech at
/// the product rate.
pub fn decode(decoder: &mut Decoder, payload: &[u8]) -> Result<Vec<i16>, String> {
    if payload.is_empty() {
        return Err("Opus payload is empty".into());
    }
    if decoder.stream.is_stream() || payload.starts_with(b"OggS") {
        decoder.stream.push(payload);
        let mut samples = Vec::new();
        for packet in decoder.stream.packets() {
            samples.extend(decode_packet(decoder, &packet)?);
        }
        decoder.apply_skip();
        return Ok(samples);
    }
    decode_packet(decoder, payload)
}

/// Decode one Opus packet into speech at the product rate.
fn decode_packet(decoder: &mut Decoder, packet: &[u8]) -> Result<Vec<i16>, String> {
    if let Some(error) = &decoder.error {
        return Err(error.clone());
    }
    let inner = decoder.inner.as_mut().ok_or("Opus decoder is not available")?;
    let mut samples = vec![0f32; DECODE_LIMIT];
    let count = inner.decode(packet, DECODE_LIMIT, &mut samples).map_err(|error| format!("Opus packet was rejected: {error}"))?;
    if count > samples.len() {
        return Err("Opus packet produced more samples than the frame allows".into());
    }
    let samples: Vec<i16> = samples[..count].iter().map(|value| (value * 32768.0).round().clamp(-32768.0, 32767.0) as i16).collect();
    Ok(decoder.take_skip(samples))
}

impl Decoder {
    /// Hold back the samples the stream's pre-skip covers.
    fn apply_skip(&mut self) {
        if self.skip_set {
            return;
        }
        self.skip_set = true;
        let pre_skip = self.stream.pre_skip() as usize;
        if pre_skip == 0 {
            return;
        }
        self.skip = pre_skip * RATE / GRANULE_RATE;
    }

    /// Drop the samples the stream's pre-skip holds back.
    fn take_skip(&mut self, samples: Vec<i16>) -> Vec<i16> {
        if self.skip == 0 {
            return samples;
        }
        let take = self.skip.min(samples.len());
        self.skip -= take;
        samples[take..].to_vec()
    }
}

/// Encode then decode `pcm` and report the signal to noise ratio. The codec
/// delays its output by its lookahead, so the two are lined up before the
/// ratio is measured.
#[allow(dead_code)]
pub fn roundtrip_snr(pcm: &[i16]) -> Result<f64, String> {
    if pcm.len() < FRAME * 4 || pcm.len() % FRAME != 0 {
        return Err("The speech needs whole 20 ms frames and at least 80 ms".into());
    }
    let mut encoder = Encoder::new();
    let mut decoder = Decoder::new();
    let mut decoded = Vec::with_capacity(pcm.len());
    for frame in pcm.chunks_exact(FRAME) {
        let packet = encode(&mut encoder, frame)?;
        decoded.extend(decode(&mut decoder, &packet)?);
    }
    if decoded.len() < pcm.len() {
        return Err("Decoded audio is shorter than the speech".into());
    }
    let delay = best_delay(pcm, &decoded);
    let mut signal = 0.0f64;
    let mut noise = 0.0f64;
    for (index, original) in pcm.iter().enumerate() {
        let position = index + delay;
        if position >= decoded.len() {
            break;
        }
        let value = *original as f64;
        let error = value - decoded[position] as f64;
        signal += value * value;
        noise += error * error;
    }
    if signal <= 0.0 {
        return Err("The speech has no energy".into());
    }
    if noise <= 1e-12 {
        return Ok(120.0);
    }
    Ok(10.0 * (signal / noise).log10())
}

/// The delay of the decoded signal that lines it up with the original.
#[allow(dead_code)]
fn best_delay(original: &[i16], decoded: &[i16]) -> usize {
    let span = decoded.len().saturating_sub(original.len()).min(GRANULE_RATE / 50);
    let mut best = 0usize;
    let mut best_score = f64::MIN;
    for delay in 0..=span {
        let mut score = 0.0f64;
        for (index, value) in original.iter().enumerate() {
            let position = index + delay;
            if position >= decoded.len() {
                break;
            }
            score += (*value as f64) * (decoded[position] as f64);
        }
        if score > best_score {
            best_score = score;
            best = delay;
        }
    }
    best
}

/// Raise samples from 16 kHz to 24 kHz.
pub fn upsample_16_to_24(input: &[i16]) -> Result<Vec<i16>, String> {
    if input.len() % 2 != 0 {
        return Err("16 kHz audio needs an even sample count to reach 24 kHz".into());
    }
    let mut out = Vec::with_capacity(input.len() / 2 * 3);
    for pair in input.chunks_exact(2) {
        let left = pair[0] as i32;
        let right = pair[1] as i32;
        out.push(left as i16);
        out.push(((left * 2 + right) / 3) as i16);
        out.push(((left + right * 2) / 3) as i16);
    }
    Ok(out)
}

/// Lower samples from 24 kHz to 16 kHz.
pub fn downsample_24_to_16(input: &[i16]) -> Result<Vec<i16>, String> {
    if input.len() % 3 != 0 {
        return Err("24 kHz audio needs groups of three samples to reach 16 kHz".into());
    }
    let mut out = Vec::with_capacity(input.len() / 3 * 2);
    for trio in input.chunks_exact(3) {
        out.push(trio[0]);
        out.push(trio[2]);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Ogg Opus container
// ---------------------------------------------------------------------------
//
// The container of the peer: pages of the framing of Ogg, carrying Opus
// packets, with the identification and the comment page of the stream at the
// front. The checksums are the ones of the framing of Ogg, over the whole
// page with the checksum field cleared.

/// Serial number of the stream the product writes.
const STREAM_SERIAL: u32 = 0x524c_4c4d;

/// Vendor string of the comment page.
const VENDOR: &[u8] = b"rllm";

/// Writer of an Ogg Opus stream.
pub struct OggWriter {
    sequence: u32,
    granule: u64,
}

impl OggWriter {
    /// New writer of a stream.
    pub fn new() -> Self {
        Self { sequence: 0, granule: 0 }
    }

    /// The identification and the comment page of the stream.
    pub fn header(&mut self, pre_skip: u16, rate: u32) -> Vec<u8> {
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1);
        head.push(1);
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&rate.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes());
        head.push(0);
        let mut tags = Vec::with_capacity(16);
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
        tags.extend_from_slice(VENDOR);
        tags.extend_from_slice(&0u32.to_le_bytes());
        let mut out = Vec::new();
        out.extend_from_slice(&self.build_page(0, 0x02, &head));
        out.extend_from_slice(&self.build_page(1, 0x00, &tags));
        self.sequence = 2;
        out
    }

    /// One page carrying one Opus packet.
    pub fn page(&mut self, packet: &[u8]) -> Vec<u8> {
        self.granule += GRANULE_STEP;
        let sequence = self.sequence;
        self.sequence += 1;
        self.build_page(sequence, 0x00, packet)
    }

    /// One page of the stream.
    fn build_page(&mut self, sequence: u32, header_type: u8, packet: &[u8]) -> Vec<u8> {
        let mut segments = Vec::new();
        let mut rest = packet.len();
        loop {
            let take = rest.min(255);
            segments.push(take as u8);
            rest -= take;
            if take < 255 {
                break;
            }
        }
        let mut page = Vec::with_capacity(27 + segments.len() + packet.len());
        page.extend_from_slice(b"OggS");
        page.push(0);
        page.push(header_type);
        page.extend_from_slice(&self.granule.to_le_bytes());
        page.extend_from_slice(&STREAM_SERIAL.to_le_bytes());
        page.extend_from_slice(&sequence.to_le_bytes());
        page.extend_from_slice(&0u32.to_le_bytes());
        page.push(segments.len() as u8);
        page.extend_from_slice(&segments);
        page.extend_from_slice(packet);
        let checksum = ogg_crc(&page);
        page[22..26].copy_from_slice(&checksum.to_le_bytes());
        page
    }
}

impl Default for OggWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Reader of an Ogg Opus stream.
pub struct OggReader {
    buffer: Vec<u8>,
    packet: Vec<u8>,
    pre_skip: u32,
    seen: bool,
}

impl OggReader {
    /// New reader of a stream.
    pub fn new() -> Self {
        Self { buffer: Vec::new(), packet: Vec::new(), pre_skip: 0, seen: false }
    }

    /// Add the bytes of one message to the stream.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Whether the bytes so far belong to a container.
    pub fn is_stream(&self) -> bool {
        self.seen || !self.buffer.is_empty()
    }

    /// The pre-skip the identification page declared, in samples at the
    /// container rate.
    pub fn pre_skip(&self) -> u32 {
        self.pre_skip
    }

    /// The Opus packets the stream holds so far. The identification and the
    /// comment page are read and dropped.
    pub fn packets(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some((segments, body)) = self.next_page() {
            self.seen = true;
            let mut cursor = 0usize;
            for length in segments {
                let end = cursor + length as usize;
                if end > body.len() {
                    break;
                }
                self.packet.extend_from_slice(&body[cursor..end]);
                cursor = end;
                if length < 255 {
                    let packet = std::mem::take(&mut self.packet);
                    if packet.starts_with(b"OpusHead") {
                        self.read_head(&packet);
                    } else if !packet.starts_with(b"OpusTags") && !packet.is_empty() {
                        out.push(packet);
                    }
                }
            }
        }
        out
    }

    /// Read the pre-skip of the identification page.
    fn read_head(&mut self, packet: &[u8]) {
        if packet.len() < 12 {
            return;
        }
        let pre_skip = u16::from_le_bytes([packet[10], packet[11]]) as u32;
        self.pre_skip = pre_skip;
    }

    /// The next complete page of the stream.
    fn next_page(&mut self) -> Option<(Vec<u8>, Vec<u8>)> {
        if self.buffer.len() < 27 {
            return None;
        }
        if &self.buffer[0..4] != b"OggS" {
            // Not a container after all: drop the bytes and let the caller
            // read them as a packet.
            self.buffer.clear();
            self.seen = false;
            return None;
        }
        if self.buffer[4] != 0 {
            self.buffer.clear();
            return None;
        }
        let count = self.buffer[26] as usize;
        if self.buffer.len() < 27 + count {
            return None;
        }
        let segments: Vec<u8> = self.buffer[27..27 + count].to_vec();
        let body_len: usize = segments.iter().map(|length| *length as usize).sum();
        if self.buffer.len() < 27 + count + body_len {
            return None;
        }
        let body = self.buffer[27 + count..27 + count + body_len].to_vec();
        self.buffer.drain(..27 + count + body_len);
        Some((segments, body))
    }
}

impl Default for OggReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Checksum of one page of the framing of Ogg.
fn ogg_crc(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut value = (index as u32) << 24;
        for _ in 0..8 {
            value = if value & 0x8000_0000 != 0 { (value << 1) ^ 0x04c1_1db7 } else { value << 1 };
        }
        *slot = value;
    }
    let mut value = 0u32;
    for byte in data {
        value = (value << 8) ^ table[(((value >> 24) & 0xFF) as usize) ^ (*byte as usize)];
    }
    value
}

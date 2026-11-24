//! Strings named by an Android binary XML pool. Nothing is installed.

const MAGIC: u32 = 0x0008_0003;
const POOL: u16 = 0x0001;
const UTF8_FLAG: u32 = 0x100;
const POOL_HEADER: usize = 28;
const MAX_STRINGS: usize = 32;
const SCAN_STRINGS: usize = 512;
const TEXT_CAP: usize = 64;

const NOT_XML: &str = "Not an Android binary XML document.";
const SHORT_HEADER: &str = "The binary XML document is shorter than its header.";
const NOT_INSTALLED: &str = "The document was not installed.";

/// Read at most 32 strings from a binary XML document that begins with the
/// little-endian magic `0x00080003`.
///
/// The string-pool chunk, when present, begins immediately after that magic:
/// `u16` type, `u16` headerSize, `u32` chunkSize, `u32` stringCount,
/// `u32` styleCount, `u32` flags, `u32` stringsStart, `u32` stylesStart.
/// Type `0x0001` is the pool. Flag bit 8 (`0x100`) selects UTF-8; any other
/// value selects UTF-16LE. A truncated string ends the list, and the report
/// says later strings were not listed.
pub fn manifest_pool(bytes: &[u8]) -> String {
    if bytes.len() < 4 || u32_at(bytes, 0) != Some(MAGIC) {
        return NOT_XML.to_string();
    }
    if bytes.len() < 4 + 8 {
        return SHORT_HEADER.to_string();
    }
    let Some(chunk) = pool_chunk(bytes) else {
        let mut report = String::new();
        report.push_str(NOT_INSTALLED);
        return report;
    };
    let mut report = String::new();
    for line in pool_lines(chunk) {
        report.push_str(&line);
        report.push('\n');
    }
    report.push_str(NOT_INSTALLED);
    report
}

struct PoolHeader {
    kind: u16,
    chunk_size: u32,
    string_count: usize,
    utf8: bool,
    strings_start: usize,
}

fn pool_lines(chunk: &[u8]) -> Vec<String> {
    let Some(header) = pool_header(chunk) else {
        return Vec::new();
    };
    if header.kind != POOL {
        return Vec::new();
    }
    let Some(end) = pool_end(&header, chunk.len()) else {
        return Vec::new();
    };
    let mut at = header.strings_start;
    let mut lines = Vec::new();
    let mut extra = 0usize;
    let mut read = 0usize;
    for index in 0..header.string_count.min(SCAN_STRINGS) {
        let Some((text, next)) = one_string(chunk, at, end, header.utf8) else {
            break;
        };
        let keep = index < MAX_STRINGS || (extra < MAX_STRINGS && notable(&text));
        if keep {
            if index >= MAX_STRINGS {
                extra += 1;
            }
            lines.push(format!("string {index} {text}"));
        }
        at = next;
        read += 1;
    }
    if header.string_count > read || read > lines.len() {
        lines.push("later strings were not listed".to_string());
    }
    lines
}

fn notable(text: &str) -> bool {
    text.contains("permission") || text.contains("Audio") || text.contains("microphone") || text.contains("MediaRecorder") || text.contains("RECORD_") || text.contains("getUserMedia") || text.contains("ultrasonic")
}

fn pool_header(chunk: &[u8]) -> Option<PoolHeader> {
    if chunk.len() < POOL_HEADER {
        return None;
    }
    Some(PoolHeader {
        kind: u16_at(chunk, 0)?,
        chunk_size: u32_at(chunk, 4)?,
        string_count: u32_at(chunk, 8)? as usize,
        utf8: u32_at(chunk, 16)? & UTF8_FLAG != 0,
        strings_start: u32_at(chunk, 20)? as usize,
    })
}

fn pool_end(header: &PoolHeader, len: usize) -> Option<usize> {
    let declared = header.chunk_size as usize;
    if declared < POOL_HEADER || header.strings_start > declared {
        return None;
    }
    Some(declared.min(len))
}

fn pool_chunk(bytes: &[u8]) -> Option<&[u8]> {
    let header_size = u16_at(bytes, 2)? as usize;
    if header_size >= 8 && bytes.len() >= header_size.saturating_add(2) && u16_at(bytes, header_size) == Some(POOL) {
        return Some(&bytes[header_size..]);
    }
    if u16_at(bytes, 4) == Some(POOL) {
        return Some(&bytes[4..]);
    }
    None
}

fn one_string(chunk: &[u8], at: usize, end: usize, utf8: bool) -> Option<(String, usize)> {
    if utf8 {
        if let Some(found) = utf8_single(chunk, at, end) {
            return Some(found);
        }
        return utf8_pair(chunk, at, end);
    }
    let count = u16_at(chunk, at)? as usize;
    let start = at.checked_add(2)?;
    let bytes = count.checked_mul(2)?;
    let stop = start.checked_add(bytes)?;
    let next = stop.checked_add(2)?;
    if next > end || stop > chunk.len() {
        return None;
    }
    let mut units = Vec::with_capacity(count);
    let mut cursor = start;
    while cursor < stop {
        units.push(u16_at(chunk, cursor)?);
        cursor += 2;
    }
    Some((bound_text(String::from_utf16_lossy(&units)), next))
}

fn utf8_single(chunk: &[u8], at: usize, end: usize) -> Option<(String, usize)> {
    let count = *chunk.get(at)? as usize;
    if count & 0x80 != 0 {
        return None;
    }
    let start = at.checked_add(1)?;
    let stop = start.checked_add(count)?;
    let next = stop.checked_add(1)?;
    if next > end || chunk.get(stop) != Some(&0) {
        return None;
    }
    Some((bound_text(String::from_utf8_lossy(&chunk[start..stop]).into_owned()), next))
}

fn utf8_len(chunk: &[u8], at: usize) -> Option<(usize, usize)> {
    let first = *chunk.get(at)? as usize;
    if first & 0x80 != 0 {
        let second = *chunk.get(at + 1)? as usize;
        Some((((first & 0x7f) << 8) | second, at + 2))
    } else {
        Some((first, at + 1))
    }
}

fn utf8_pair(chunk: &[u8], at: usize, end: usize) -> Option<(String, usize)> {
    let (_chars, byte_at) = utf8_len(chunk, at)?;
    let (count, start) = utf8_len(chunk, byte_at)?;
    let stop = start.checked_add(count)?;
    let next = stop.checked_add(1)?;
    if next > end || chunk.get(stop) != Some(&0) {
        return None;
    }
    Some((bound_text(String::from_utf8_lossy(&chunk[start..stop]).into_owned()), next))
}

fn bound_text(full: String) -> String {
    let mut text = String::new();
    let mut more = false;
    for (index, ch) in full.chars().enumerate() {
        if index == TEXT_CAP {
            more = true;
            break;
        }
        text.push(ch);
    }
    if more {
        text.push_str("...");
    }
    text
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

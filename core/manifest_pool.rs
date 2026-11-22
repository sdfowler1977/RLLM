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

#[cfg(test)]
mod tests {
    use super::manifest_pool;

    fn magic() -> Vec<u8> {
        0x0008_0003u32.to_le_bytes().to_vec()
    }

    fn pool(flags: u32, strings: &[Vec<u8>]) -> Vec<u8> {
        let mut data = Vec::new();
        for string in strings {
            data.extend_from_slice(string);
        }
        let strings_start = 28u32;
        let chunk_size = strings_start + data.len() as u32;
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&1u16.to_le_bytes());
        chunk.extend_from_slice(&28u16.to_le_bytes());
        chunk.extend_from_slice(&chunk_size.to_le_bytes());
        chunk.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&0u32.to_le_bytes());
        chunk.extend_from_slice(&flags.to_le_bytes());
        chunk.extend_from_slice(&strings_start.to_le_bytes());
        chunk.extend_from_slice(&0u32.to_le_bytes());
        chunk.extend_from_slice(&data);
        let mut file = magic();
        file.extend_from_slice(&chunk);
        file
    }

    fn utf16(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    fn utf8(text: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.push(text.len() as u8);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes
    }

    #[test]
    fn short_doc() {
        let mut bytes = magic();
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        let report = manifest_pool(&bytes);
        assert!(report.contains("shorter than its header"));
        assert_eq!(report, "The binary XML document is shorter than its header.");
    }

    #[test]
    fn missing_magic() {
        assert_eq!(manifest_pool(b""), "Not an Android binary XML document.");
        assert_eq!(manifest_pool(&[0x03, 0x00, 0x08]), "Not an Android binary XML document.");
        assert_eq!(
            manifest_pool(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            "Not an Android binary XML document."
        );
    }

    #[test]
    fn utf16_strings_and_closing_line() {
        let report = manifest_pool(&pool(0, &[utf16("manifest"), utf16("package")]));
        assert_eq!(
            report,
            "string 0 manifest\nstring 1 package\nThe document was not installed."
        );
    }

    #[test]
    fn utf8_flag_selects_bytes() {
        let report = manifest_pool(&pool(0x100, &[utf8("android")]));
        assert_eq!(report, "string 0 android\nThe document was not installed.");
    }

    #[test]
    fn truncated_string_stops_the_list() {
        let mut file = pool(0, &[utf16("one"), utf16("two")]);
        file.pop();
        let report = manifest_pool(&file);
        assert_eq!(report, "string 0 one\nlater strings were not listed\nThe document was not installed.");
    }

    #[test]
    fn document_header_and_two_length_utf8() {
        let text = "android.permission.RECORD_AUDIO";
        let mut string = Vec::new();
        string.push(text.len() as u8);
        string.push(text.len() as u8);
        string.extend_from_slice(text.as_bytes());
        string.push(0);
        let mut chunk = pool(0x100, &[string]);
        let size = (8 + chunk.len() - 4) as u32;
        let mut file = Vec::new();
        file.extend_from_slice(&3u16.to_le_bytes());
        file.extend_from_slice(&8u16.to_le_bytes());
        file.extend_from_slice(&size.to_le_bytes());
        file.extend_from_slice(&chunk[4..]);
        let report = manifest_pool(&file);
        assert!(report.contains("string 0 android.permission.RECORD_AUDIO"), "{report}");
    }

    #[test]
    fn long_utf8_string_is_marked() {
        let text = "A".repeat(65);
        let shown = "A".repeat(64);
        assert_eq!(
            manifest_pool(&pool(0x100, &[utf8(&text)])),
            format!("string 0 {shown}...\nThe document was not installed.")
        );
    }

    #[test]
    fn at_most_thirty_two_strings() {
        let strings: Vec<Vec<u8>> = (0..40).map(|index| utf8(&format!("s{index}"))).chain(std::iter::once(utf8("android.permission.RECORD_AUDIO"))).collect();
        let report = manifest_pool(&pool(0x100, &strings));
        assert!(report.contains("string 31 s31"));
        assert!(!report.contains("string 32 "));
        assert!(report.contains("string 40 android.permission.RECORD_AUDIO"), "{report}");
        assert!(report.contains("later strings were not listed\nThe document was not installed."));
    }
}

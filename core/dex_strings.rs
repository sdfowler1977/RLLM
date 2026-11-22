//! DEX string ids from a little-endian header.
//! The file is not executed.

#![forbid(unsafe_code)]

const MAX_STRINGS: usize = 32;
const SCAN_STRINGS: usize = 512;
const HEADER: usize = 64;
const STRING_IDS_SIZE: usize = 56;
const STRING_IDS_OFF: usize = 60;

const TEXT_CAP: usize = 64;

const NOT_DEX: &str = "Not a DEX file.";
const SHORT_HEADER: &str = "The DEX header is shorter than its string ids.";
const NOT_EXECUTED: &str = "The file was not executed.";

/// Read at most 32 DEX string ids.
///
/// The slice must begin with `dex\n`. `string_ids_size` is the little-endian
/// `u32` at offset 56 and `string_ids_off` is the little-endian `u32` at
/// offset 60. Each id is a `u32` file offset of an unsigned LEB128 length,
/// at most four bytes, followed by that many MUTF-8 bytes. Bytes are copied
/// until a 0. A string id that is not listed is named. The file is not executed.
pub fn dex_strings(bytes: &[u8]) -> String {
    if !bytes.starts_with(b"dex\n") {
        return NOT_DEX.to_string();
    }
    if bytes.len() < HEADER {
        return SHORT_HEADER.to_string();
    }
    let Some(count) = u32_at(bytes, STRING_IDS_SIZE) else {
        return SHORT_HEADER.to_string();
    };
    let Some(table) = u32_at(bytes, STRING_IDS_OFF) else {
        return SHORT_HEADER.to_string();
    };
    let mut report = String::new();
    let limit = (count as usize).min(SCAN_STRINGS);
    let mut extra = 0usize;
    let mut listed = 0usize;
    for index in 0..limit {
        let Some(offset) = id_offset(bytes, table, index) else {
            continue;
        };
        let Some(text) = string_text(bytes, offset) else {
            continue;
        };
        let keep = index < MAX_STRINGS || (extra < MAX_STRINGS && notable(&text));
        if keep {
            if index >= MAX_STRINGS {
                extra += 1;
            }
            report.push_str(&format!("string {index} {text}"));
            report.push('\n');
            listed += 1;
        }
    }
    if count as usize > listed {
        report.push_str("later strings were not listed\n");
    }
    report.push_str(NOT_EXECUTED);
    report
}

fn notable(text: &str) -> bool {
    text.contains("permission") || text.contains("Audio") || text.contains("microphone") || text.contains("MediaRecorder") || text.contains("RECORD_") || text.contains("getUserMedia") || text.contains("ultrasonic")
}

fn id_offset(bytes: &[u8], table: u32, index: usize) -> Option<usize> {
    let at = (table as usize).checked_add(index.checked_mul(4)?)?;
    Some(u32_at(bytes, at)? as usize)
}

fn string_text(bytes: &[u8], at: usize) -> Option<String> {
    let (len, start) = uleb128(bytes, at)?;
    let stop = start.checked_add(len)?;
    let data = bytes.get(start..stop)?;
    let end = data.iter().position(|byte| *byte == 0).unwrap_or(data.len());
    let full = String::from_utf8_lossy(&data[..end]);
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
    Some(text)
}

/// Unsigned LEB128 of at most four bytes. A fifth continuation byte is truncated.
fn uleb128(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    let mut value = 0u32;
    let mut shift = 0u32;
    for step in 0..4 {
        let pos = at.checked_add(step)?;
        let byte = *bytes.get(pos)?;
        value |= u32::from(byte & 0x7f) << shift;
        let next = pos.checked_add(1)?;
        if byte & 0x80 == 0 {
            return Some((value as usize, next));
        }
        shift += 7;
    }
    None
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    use super::dex_strings;

    fn encoded(text: &str) -> Vec<u8> {
        let bytes = text.as_bytes();
        let mut out = vec![bytes.len() as u8];
        out.extend_from_slice(bytes);
        out
    }

    fn file(strings: &[Vec<u8>]) -> Vec<u8> {
        let table = 64usize;
        let data = table + strings.len() * 4;
        let mut bytes = vec![0u8; data];
        bytes[..4].copy_from_slice(b"dex\n");
        bytes[56..60].copy_from_slice(&(strings.len() as u32).to_le_bytes());
        bytes[60..64].copy_from_slice(&(table as u32).to_le_bytes());
        let mut cursor = data;
        for (index, string) in strings.iter().enumerate() {
            let at = table + index * 4;
            bytes[at..at + 4].copy_from_slice(&(cursor as u32).to_le_bytes());
            bytes.extend_from_slice(string);
            cursor += string.len();
        }
        bytes
    }

    #[test]
    fn missing_prefix() {
        let report = dex_strings(b"nope");
        assert!(report.contains("Not a DEX file"));
        assert_eq!(report, "Not a DEX file.");
    }

    #[test]
    fn short_header() {
        assert_eq!(
            dex_strings(b"dex\n"),
            "The DEX header is shorter than its string ids."
        );
    }

    #[test]
    fn one_string_and_closing_line() {
        let report = dex_strings(&file(&[encoded("hello")]));
        assert_eq!(report, "string 0 hello\nThe file was not executed.");
    }

    #[test]
    fn two_byte_length() {
        let report = dex_strings(&file(&[vec![0x82, 0x00, b'a', b'b']]));
        assert_eq!(report, "string 0 ab\nThe file was not executed.");
    }

    #[test]
    fn copy_stops_at_zero() {
        let report = dex_strings(&file(&[vec![5, b'h', b'e', b'l', 0, b'o']]));
        assert_eq!(report, "string 0 hel\nThe file was not executed.");
    }

    #[test]
    fn truncated_id_is_skipped() {
        let kept = encoded("kept");
        let table = 64usize;
        let kept_at = table + 8;
        let short_at = kept_at + kept.len();
        let mut bytes = vec![0u8; kept_at];
        bytes[..4].copy_from_slice(b"dex\n");
        bytes[56..60].copy_from_slice(&2u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&(table as u32).to_le_bytes());
        bytes[64..68].copy_from_slice(&(short_at as u32).to_le_bytes());
        bytes[68..72].copy_from_slice(&(kept_at as u32).to_le_bytes());
        bytes.extend_from_slice(&kept);
        bytes.push(10);
        assert_eq!(
            dex_strings(&bytes),
            "string 1 kept\nlater strings were not listed\nThe file was not executed."
        );
    }

    #[test]
    fn leb_longer_than_four_bytes_is_skipped() {
        let kept = encoded("kept");
        let table = 64usize;
        let long_at = table + 8;
        let kept_at = long_at + 4;
        let mut bytes = vec![0u8; long_at];
        bytes[..4].copy_from_slice(b"dex\n");
        bytes[56..60].copy_from_slice(&2u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&(table as u32).to_le_bytes());
        bytes[64..68].copy_from_slice(&(long_at as u32).to_le_bytes());
        bytes[68..72].copy_from_slice(&(kept_at as u32).to_le_bytes());
        bytes.extend_from_slice(&[0x80, 0x80, 0x80, 0x80]);
        bytes.extend_from_slice(&kept);
        assert_eq!(
            dex_strings(&bytes),
            "string 1 kept\nlater strings were not listed\nThe file was not executed."
        );
    }

    #[test]
    fn at_most_thirty_two_strings() {
        let strings: Vec<Vec<u8>> = (0..40).map(|index| encoded(&format!("s{index}"))).chain([encoded("android.permission.RECORD_AUDIO"), encoded("getUserMedia")]).collect();
        let report = dex_strings(&file(&strings));
        assert!(report.contains("string 31 s31"));
        assert!(!report.contains("string 32 "));
        assert!(report.contains("string 40 android.permission.RECORD_AUDIO"), "{report}");
        assert!(report.contains("string 41 getUserMedia"), "{report}");
        assert!(report.contains("later strings were not listed\nThe file was not executed."));
    }

    #[test]
    fn long_string_is_marked() {
        let mut raw = vec![65u8];
        raw.extend(std::iter::repeat(b'A').take(65));
        let shown = "A".repeat(64);
        assert_eq!(
            dex_strings(&file(&[raw])),
            format!("string 0 {shown}...\nThe file was not executed.")
        );
    }
}

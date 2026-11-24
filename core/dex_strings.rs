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

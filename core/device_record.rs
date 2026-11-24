//! Device fields from a caller-supplied byte record.
//! `DEV1`, a little-endian field count, then that many little-endian
//! length-prefixed UTF-8 fields (`key=value`).
//! A live phone is not inspected, and no device command is run.
#![forbid(unsafe_code)]

const MAGIC: &[u8] = b"DEV1";
const LIST_CAP: usize = 16;
const REFUSED: &str = "The device record was refused.";
const LATER: &str = "later fields were not listed";
const CLOSING: &str = "No device command was run.";

/// List at most 16 fields from a caller-supplied record.
///
/// Keys `model`, `abi`, and `sdk` are named when present.
/// A 17th field is not listed; the report says later fields were not listed.
/// The report ends with `No device command was run.`
/// A bad magic, a short record, or a field that is not UTF-8 is refused.
pub fn device_record(bytes: &[u8]) -> String {
    let Some(fields) = read_fields(bytes) else {
        return REFUSED.to_string();
    };
    let mut out = String::new();
    let shown = fields.len().min(LIST_CAP);
    for field in &fields[..shown] {
        out.push_str(&field_line(field));
        out.push('\n');
    }
    if fields.len() > LIST_CAP {
        out.push_str(LATER);
        out.push('\n');
    }
    out.push_str(CLOSING);
    out.push('\n');
    out
}

fn read_fields(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.get(..4) != Some(MAGIC) {
        return None;
    }
    let count = u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?) as usize;
    let mut at = 6usize;
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let len_end = at.checked_add(2)?;
        let len = u16::from_le_bytes(bytes.get(at..len_end)?.try_into().ok()?) as usize;
        at = len_end;
        let end = at.checked_add(len)?;
        let text = std::str::from_utf8(bytes.get(at..end)?).ok()?;
        fields.push(text.to_string());
        at = end;
    }
    Some(fields)
}

fn field_line(field: &str) -> String {
    match field.split_once('=') {
        Some((key, value)) if matches!(key, "model" | "abi" | "sdk") => format!("{key} {value}"),
        Some((key, value)) => format!("field {key} {value}"),
        None => format!("field {field}"),
    }
}

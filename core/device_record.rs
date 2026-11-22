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

#[cfg(test)]
mod tests {
    use super::{device_record, REFUSED};

    fn record(fields: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::from(b"DEV1".as_slice());
        bytes.extend_from_slice(&(fields.len() as u16).to_le_bytes());
        for field in fields {
            let raw = field.as_bytes();
            bytes.extend_from_slice(&(raw.len() as u16).to_le_bytes());
            bytes.extend_from_slice(raw);
        }
        bytes
    }

    #[test]
    fn model_and_abi() {
        let text = device_record(&record(&["model=Pixel", "abi=arm64-v8a", "sdk=34"]));
        assert!(text.lines().any(|line| line == "model Pixel"), "{text}");
        assert!(text.lines().any(|line| line == "abi arm64-v8a"), "{text}");
        assert!(text.lines().any(|line| line == "sdk 34"), "{text}");
        assert!(!text.contains("later fields were not listed"), "{text}");
        assert!(text.ends_with("No device command was run.\n"), "{text}");
    }

    #[test]
    fn seventeen_fields_name_the_cap() {
        let owned: Vec<String> = (0..17).map(|index| format!("k{index}=v{index}")).collect();
        let fields: Vec<&str> = owned.iter().map(String::as_str).collect();
        let text = device_record(&record(&fields));
        let listed: Vec<&str> = text.lines().filter(|line| line.starts_with("field ")).collect();
        assert_eq!(listed.len(), 16, "{text}");
        assert_eq!(listed[0], "field k0 v0");
        assert_eq!(listed[15], "field k15 v15");
        assert!(text.contains("later fields were not listed\n"), "{text}");
        assert!(!text.contains("k16"), "{text}");
        assert!(!text.contains("v16"), "{text}");
        assert!(text.ends_with("No device command was run.\n"), "{text}");
    }

    #[test]
    fn bad_magic_is_refused() {
        let mut bytes = record(&["model=Pixel", "abi=arm64-v8a"]);
        bytes[..4].copy_from_slice(b"XXXX");
        assert_eq!(device_record(&bytes), REFUSED);
        assert_eq!(device_record(b"dev1"), REFUSED);
        assert_eq!(device_record(b"DEV1"), REFUSED);
        assert_eq!(device_record(b""), REFUSED);
    }
}

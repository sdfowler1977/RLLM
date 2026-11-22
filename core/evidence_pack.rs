//! Evidence rows: EVPK, a little-endian count, then length-prefixed payloads.
#![forbid(unsafe_code)]

const MAGIC: [u8; 4] = *b"EVPK";
const MAX_ROWS: usize = 32;
const MAX_PAYLOAD: usize = 1_048_576;
const UNREAD: &str = "The pack was not read.";

/// Store at most 32 rows and at most 1_048_576 payload bytes.
/// Further rows are left out. A row that would pass the payload cap is left out,
/// and a later row that still fits may be stored.
pub fn evidence_pack(rows: &[&[u8]]) -> Vec<u8> {
    pack_with_notes(rows).bytes
}

/// One line per stored row, then the packed count.
/// A row left out by the row cap or the payload cap is named.
pub fn evidence_listing(rows: &[&[u8]]) -> String {
    let packed = pack_with_notes(rows);
    let mut text = evidence_rows(&evidence_pack(rows));
    if packed.oversized {
        text.push('\n');
        text.push_str("a row above the payload cap was not stored");
    }
    if packed.later {
        text.push('\n');
        text.push_str("later rows were not stored");
    }
    text
}

struct Packed {
    bytes: Vec<u8>,
    later: bool,
    oversized: bool,
}

fn pack_with_notes(rows: &[&[u8]]) -> Packed {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut count: u32 = 0;
    let mut payload: usize = 0;
    let mut later = false;
    let mut oversized = false;
    for row in rows {
        if count as usize == MAX_ROWS {
            later = true;
            break;
        }
        let Some(next) = payload.checked_add(row.len()) else {
            oversized = true;
            continue;
        };
        if next > MAX_PAYLOAD {
            oversized = true;
            continue;
        }
        payload = next;
        out.extend_from_slice(&(row.len() as u32).to_le_bytes());
        out.extend_from_slice(row);
        count += 1;
    }
    out[4..8].copy_from_slice(&count.to_le_bytes());
    Packed { bytes: out, later, oversized }
}

/// One line per stored row, then the packed count.
/// A missing magic or a short length or payload yields a single unread line.
pub fn evidence_rows(pack: &[u8]) -> String {
    let Some(lengths) = row_lengths(pack) else {
        return UNREAD.to_string();
    };
    let mut lines = Vec::with_capacity(lengths.len().saturating_add(1));
    for (index, length) in lengths.iter().enumerate() {
        lines.push(format!("row {index} length {length}"));
    }
    lines.push(format!("pack rows {}", lengths.len()));
    lines.join("\n")
}

fn read_u32(pack: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let bytes: [u8; 4] = pack.get(at..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn row_lengths(pack: &[u8]) -> Option<Vec<usize>> {
    if !pack.starts_with(&MAGIC) {
        return None;
    }
    let count = read_u32(pack, 4)? as usize;
    let mut at = 8usize;
    let mut lengths = Vec::new();
    for _ in 0..count {
        let len = read_u32(pack, at)? as usize;
        at = at.checked_add(4)?.checked_add(len)?;
        if at > pack.len() {
            return None;
        }
        lengths.push(len);
    }
    Some(lengths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_row() {
        let text = evidence_rows(&evidence_pack(&[b"mic"]));
        assert!(text.contains("length 3"), "{text}");
        assert!(text.contains("pack rows 1"), "{text}");
    }

    #[test]
    fn one_row_bytes_and_text() {
        let packed = evidence_pack(&[b"mic"]);
        assert_eq!(&packed[..4], b"EVPK");
        assert_eq!(&packed[4..8], 1u32.to_le_bytes());
        assert_eq!(&packed[8..12], 3u32.to_le_bytes());
        assert_eq!(&packed[12..], b"mic");
        assert_eq!(evidence_rows(&packed), "row 0 length 3\npack rows 1");
    }

    #[test]
    fn extra_rows_and_payload_are_omitted() {
        let byte = b"ab".as_slice();
        let many: Vec<&[u8]> = (0..40).map(|_| byte).collect();
        let text = evidence_rows(&evidence_pack(&many));
        assert!(text.contains("pack rows 32"), "{text}");
        assert!(!text.contains("row 32 "));

        let huge = vec![0u8; MAX_PAYLOAD + 1];
        let kept = evidence_rows(&evidence_pack(&[&huge, b"mic"]));
        assert_eq!(kept, "row 0 length 3\npack rows 1");

        let first = vec![1u8; 600_000];
        let second = vec![2u8; 600_000];
        let third = b"z".as_slice();
        let mixed = evidence_rows(&evidence_pack(&[&first, &second, third]));
        assert_eq!(mixed, "row 0 length 600000\nrow 1 length 1\npack rows 2");
    }

    #[test]
    fn later_rows_are_named() {
        let byte = b"ab".as_slice();
        let many: Vec<&[u8]> = (0..40).map(|_| byte).collect();
        let text = evidence_listing(&many);
        assert!(text.contains("pack rows 32\n"), "{text}");
        assert!(text.ends_with("later rows were not stored"), "{text}");
        assert!(!text.contains("row 32 "));
    }

    #[test]
    fn oversized_row_is_named() {
        let huge = vec![0u8; MAX_PAYLOAD + 1];
        assert_eq!(
            evidence_listing(&[&huge, b"mic"]),
            "row 0 length 3\npack rows 1\na row above the payload cap was not stored"
        );
    }

    #[test]
    fn unread_magic_and_truncated() {
        assert_eq!(evidence_rows(b""), UNREAD);
        assert_eq!(evidence_rows(b"EVPK"), UNREAD);
        assert_eq!(evidence_rows(b"NOPE\x01\x00\x00\x00"), UNREAD);
        let mut short = Vec::from(&MAGIC);
        short.extend_from_slice(&1u32.to_le_bytes());
        short.extend_from_slice(&3u32.to_le_bytes());
        short.push(b'x');
        assert_eq!(evidence_rows(&short), UNREAD);
    }
}

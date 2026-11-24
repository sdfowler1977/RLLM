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

//! Saved call records read from a little-endian byte slice.
//! Magic is `TRC1`, then a `u16` count, then that many `u64` address and `u32` length records.
//! At most 32 calls are listed. A further call is named.
//! No process is attached.

#![forbid(unsafe_code)]

const MAGIC: &[u8] = b"TRC1";
const CALL_LIMIT: usize = 32;
const HEADER_LEN: usize = 6;
const RECORD_LEN: usize = 12;

const REFUSED: &str = "bad magic was refused";
const SHORT_COUNT: &str = "trace count was not recorded";
const SHORT_CALL: &str = "a saved call was not fully recorded";
const LATER: &str = "later calls were not listed";
const CLOSED: &str = "No process was attached.";

/// List saved calls from `bytes`.
///
/// This reads a recorded list. It does not attach to a process.
pub fn trace_record(bytes: &[u8]) -> String {
    let mut body = String::new();
    let count = match declared_count(bytes) {
        Ok(count) => count,
        Err(reason) => {
            body.push_str(reason);
            body.push('\n');
            return finish(body);
        }
    };
    let want = usize::from(count).min(CALL_LIMIT);
    for index in 0..want {
        let Some((address, length)) = record_at(bytes, index) else {
            body.push_str(SHORT_CALL);
            body.push('\n');
            break;
        };
        body.push_str(&format!(
            "call {} address {address:#x} length {length}\n",
            index + 1
        ));
    }
    if usize::from(count) > CALL_LIMIT {
        body.push_str(LATER);
        body.push('\n');
    }
    finish(body)
}

fn finish(mut body: String) -> String {
    body.push_str(CLOSED);
    body
}

fn declared_count(bytes: &[u8]) -> Result<u16, &'static str> {
    if !bytes.starts_with(MAGIC) {
        return Err(REFUSED);
    }
    let Some(raw) = bytes.get(4..6) else {
        return Err(SHORT_COUNT);
    };
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn record_at(bytes: &[u8], index: usize) -> Option<(u64, u32)> {
    let start = HEADER_LEN.checked_add(index.checked_mul(RECORD_LEN)?)?;
    let address = read_u64(bytes, start)?;
    let length = read_u32(bytes, start.checked_add(8)?)?;
    Some((address, length))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let raw = bytes.get(at..end)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    let raw = bytes.get(at..end)?;
    Some(u64::from_le_bytes(raw.try_into().ok()?))
}

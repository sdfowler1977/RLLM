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

#[cfg(test)]
mod tests {
    use super::trace_record;

    fn encode(calls: &[(u64, u32)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TRC1");
        let count = u16::try_from(calls.len()).expect("call count fits in u16");
        bytes.extend_from_slice(&count.to_le_bytes());
        for (address, length) in calls {
            bytes.extend_from_slice(&address.to_le_bytes());
            bytes.extend_from_slice(&length.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn two_calls_keep_their_addresses() {
        let text = trace_record(&encode(&[(0x401000, 6), (0x401020, 16)]));
        assert_eq!(
            text,
            "call 1 address 0x401000 length 6\n\
             call 2 address 0x401020 length 16\n\
             No process was attached."
        );
    }

    #[test]
    fn thirty_three_calls_name_the_cap() {
        let calls: Vec<(u64, u32)> = (0..33).map(|n| (0x1000 + n, 4)).collect();
        let text = trace_record(&encode(&calls));
        assert!(text.contains("call 1 address 0x1000 length 4\n"), "{text}");
        assert!(text.contains("call 32 address 0x101f length 4\n"), "{text}");
        assert!(!text.contains("call 33"), "{text}");
        assert!(
            text.contains("later calls were not listed\nNo process was attached."),
            "{text}"
        );

        let within: Vec<(u64, u32)> = (0..32).map(|n| (0x2000 + n, 2)).collect();
        let exact = trace_record(&encode(&within));
        assert!(exact.contains("call 32 address 0x201f length 2\n"), "{exact}");
        assert!(!exact.contains("later calls were not listed"), "{exact}");
        assert!(exact.ends_with("No process was attached."), "{exact}");
    }

    #[test]
    fn bad_magic_is_refused() {
        assert_eq!(
            trace_record(b"NOPE\x01\x00"),
            "bad magic was refused\nNo process was attached."
        );
        assert_eq!(
            trace_record(&[]),
            "bad magic was refused\nNo process was attached."
        );
        let mut swapped = encode(&[(0x401000, 6)]);
        swapped[3] = b'2';
        let text = trace_record(&swapped);
        assert_eq!(text, "bad magic was refused\nNo process was attached.");
        assert!(!text.contains("0x401000"), "{text}");
    }
}

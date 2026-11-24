//! Plaintext from a caller-supplied workspace blob.
//! The blob is the magic `KEY1`, a 32-byte key, and records of a
//! little-endian u16 length plus that many bytes. Each record is XORed
//! with the key repeated. The key is only the material already in the blob.
//! When the magic is absent, the key was not supplied and no plaintext is emitted.
//! At most 8 records are listed. The network is not contacted.

const MAGIC: &[u8] = b"KEY1";
const KEY_LEN: usize = 32;
const LIST_CAP: usize = 8;

pub fn record_plain(blob: &[u8]) -> String {
    let Some((key, body)) = split_key(blob) else {
        return closed("The key was not supplied.");
    };
    let (records, more) = take_records(body);
    if records.is_empty() {
        return closed("No record was supplied.");
    }
    let mut out = String::new();
    for (index, record) in records.iter().enumerate() {
        let plain = xor_repeat(record, &key);
        out.push_str(&format!("record {} {}\n", index + 1, one_line(&plain)));
    }
    if more {
        out.push_str("later records were not listed\n");
    }
    out.push_str("The network was not contacted.\n");
    out
}

fn closed(reason: &str) -> String {
    format!("{reason}\nThe network was not contacted.\n")
}

fn split_key(blob: &[u8]) -> Option<([u8; 32], &[u8])> {
    let rest = blob.strip_prefix(MAGIC)?;
    let bytes = rest.get(..KEY_LEN)?;
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(bytes);
    Some((key, &rest[KEY_LEN..]))
}

fn take_records(mut body: &[u8]) -> (Vec<Vec<u8>>, bool) {
    let mut records = Vec::new();
    while body.len() >= 2 {
        let len = usize::from(u16::from_le_bytes([body[0], body[1]]));
        let Some(data) = body.get(2..2 + len) else {
            break;
        };
        if records.len() == LIST_CAP {
            return (records, true);
        }
        records.push(data.to_vec());
        body = &body[2 + len..];
    }
    (records, false)
}

fn xor_repeat(record: &[u8], key: &[u8; KEY_LEN]) -> Vec<u8> {
    record
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % KEY_LEN])
        .collect()
}

fn one_line(plain: &[u8]) -> String {
    String::from_utf8_lossy(plain)
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect()
}

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

#[cfg(test)]
mod checks {
    use super::record_plain;

    fn xor(bytes: &[u8], key: &[u8; 32]) -> Vec<u8> {
        bytes
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ key[index % 32])
            .collect()
    }

    fn blob(key: &[u8; 32], plains: &[&[u8]]) -> Vec<u8> {
        let mut out = b"KEY1".to_vec();
        out.extend_from_slice(key);
        for plain in plains {
            let sealed = xor(plain, key);
            let len = u16::try_from(sealed.len()).expect("record fits in u16");
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&sealed);
        }
        out
    }

    #[test]
    fn one_record_round_trips() {
        let key = [0x5A; 32];
        let plain = b"round-trip-record";
        let sealed = blob(&key, &[plain]);
        assert!(!sealed.windows(plain.len()).any(|window| window == plain));
        assert_eq!(
            record_plain(&sealed),
            "record 1 round-trip-record\nThe network was not contacted.\n"
        );
    }

    #[test]
    fn missing_magic_emits_no_plaintext() {
        let key = [0x5A; 32];
        let plain = b"vault-plain-9f3c";
        let mut shifted = b"xxxx".to_vec();
        shifted.extend_from_slice(&blob(&key, &[plain]));
        let report = record_plain(&shifted);
        assert_eq!(
            report,
            "The key was not supplied.\nThe network was not contacted.\n"
        );
        assert!(!report.contains("vault-plain-9f3c"));
        assert!(!record_plain(plain).contains("vault-plain-9f3c"));
    }

    #[test]
    fn nine_records_name_the_cap() {
        let key = [0x5A; 32];
        let plains: Vec<Vec<u8>> = (1..=9).map(|n| format!("item-{n:02}").into_bytes()).collect();
        let refs: Vec<&[u8]> = plains.iter().map(Vec::as_slice).collect();
        let report = record_plain(&blob(&key, &refs));
        let mut expected = String::new();
        for n in 1..=8 {
            expected.push_str(&format!("record {n} item-{n:02}\n"));
        }
        expected.push_str("later records were not listed\n");
        expected.push_str("The network was not contacted.\n");
        assert_eq!(report, expected);
        assert!(!report.contains("item-09"));
        assert!(report.contains("later records were not listed"));
    }
}

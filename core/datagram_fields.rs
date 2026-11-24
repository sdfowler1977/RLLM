//! One DNS question name or TLS server name from a datagram payload.
//! The payload is not decrypted.

const SEARCH_WINDOW: usize = 512;
const TEXT_CAP: usize = 64;

/// Decode one DNS question name, or a TLS client-hello server name.
///
/// A DNS header is at least 12 bytes with a big-endian question count at
/// offset 4 of at least 1. One QNAME follows at offset 12: each label is a
/// length byte and that many bytes, labels are separated by dots, and a 0
/// length ends the name. A compression pointer is not followed. The server
/// name is extension type `0x0000` in a client hello, read from at most 512
/// bytes. The payload is not decrypted.
pub fn datagram_fields(bytes: &[u8]) -> String {
    // A client hello's record length shares the DNS question-count bytes.
    // When a server name is actually present, report that name.
    if bytes.first() == Some(&0x16) && bytes.get(5) == Some(&0x01) {
        if let Some(name) = server_name(bytes) {
            return format!("server {}. The payload was not decrypted.", shown(&name));
        }
    }
    if let Some(name) = question_name(bytes) {
        return format!("question {}. The payload was not decrypted.", shown(&name));
    }
    "No question or server name was decoded. The payload was not decrypted.".to_string()
}

fn question_name(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 12 {
        return None;
    }
    let questions = u16::from_be_bytes([bytes[4], bytes[5]]);
    if questions < 1 {
        return None;
    }
    let mut at = 12usize;
    let mut labels = Vec::new();
    let mut rooted = false;
    loop {
        let Some(len_byte) = bytes.get(at).copied() else {
            break;
        };
        if len_byte == 0 {
            rooted = true;
            break;
        }
        // Top two bits set: a compression pointer. Do not follow it.
        if len_byte & 0xC0 == 0xC0 {
            break;
        }
        let len = usize::from(len_byte);
        let Some(start) = at.checked_add(1) else {
            break;
        };
        let Some(end) = start.checked_add(len) else {
            break;
        };
        let Some(label) = bytes.get(start..end) else {
            break;
        };
        labels.push(String::from_utf8_lossy(label).into_owned());
        at = end;
    }
    if labels.is_empty() || !rooted {
        return None;
    }
    Some(labels.join("."))
}

fn server_name(bytes: &[u8]) -> Option<String> {
    let data = &bytes[..bytes.len().min(SEARCH_WINDOW)];
    if data.first() != Some(&0x16) || data.get(5) != Some(&0x01) {
        return None;
    }
    let record_len = usize::from(be_u16(data, 3)?);
    let record_end = 5usize.checked_add(record_len)?.min(data.len());
    let handshake = data.get(5..record_end)?;
    if handshake.first() != Some(&0x01) || handshake.len() < 4 {
        return None;
    }
    let hs_len = (usize::from(handshake[1]) << 16)
        | (usize::from(handshake[2]) << 8)
        | usize::from(handshake[3]);
    let hello_end = 4usize.checked_add(hs_len)?.min(handshake.len());
    let hello = &handshake[..hello_end];
    // Handshake header, client version, and 32-byte random.
    let mut at = 4 + 2 + 32;
    let session_len = usize::from(*hello.get(at)?);
    at = at.checked_add(1 + session_len)?;
    let cipher_len = usize::from(be_u16(hello, at)?);
    at = at.checked_add(2 + cipher_len)?;
    let comp_len = usize::from(*hello.get(at)?);
    at = at.checked_add(1 + comp_len)?;
    let ext_len = usize::from(be_u16(hello, at)?);
    at = at.checked_add(2)?;
    let ext_end = at.checked_add(ext_len)?.min(hello.len());
    while at + 4 <= ext_end {
        let kind = be_u16(hello, at)?;
        let len = usize::from(be_u16(hello, at + 2)?);
        at = at.checked_add(4)?;
        let Some(body_end) = at.checked_add(len) else {
            return None;
        };
        if body_end > ext_end {
            return None;
        }
        if kind == 0x0000 {
            return host_name(hello.get(at..body_end)?);
        }
        at = body_end;
    }
    None
}

fn host_name(body: &[u8]) -> Option<String> {
    let list_len = usize::from(be_u16(body, 0)?);
    if body.len() < 2 + list_len || list_len < 3 {
        return None;
    }
    let list = &body[2..2 + list_len];
    if list[0] != 0 {
        return None;
    }
    let name_len = usize::from(be_u16(list, 1)?);
    let name = list.get(3..3 + name_len)?;
    if name.is_empty() || !name.iter().all(|byte| byte.is_ascii_graphic() || *byte == b' ') {
        return None;
    }
    Some(String::from_utf8_lossy(name).into_owned())
}

fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut more = false;
    for (index, ch) in text.chars().enumerate() {
        if index == TEXT_CAP {
            more = true;
            break;
        }
        out.push(ch);
    }
    if more {
        out.push_str("...");
    }
    out
}

fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let chunk = bytes.get(at..end)?;
    Some(u16::from_be_bytes([chunk[0], chunk[1]]))
}

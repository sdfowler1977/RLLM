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

#[cfg(test)]
mod tests {
    use super::datagram_fields;

    #[test]
    fn question_ab() {
        let mut bytes = vec![0u8; 12];
        bytes[5] = 1;
        bytes.extend_from_slice(&[1, b'a', 1, b'b', 0]);
        let report = datagram_fields(&bytes);
        assert!(report.contains("question a.b"), "{report}");
        assert!(report.contains("not decrypted"), "{report}");
    }

    #[test]
    fn long_question_is_marked() {
        let mut bytes = vec![0u8; 12];
        bytes[5] = 1;
        bytes.push(63);
        bytes.extend(std::iter::repeat(b'A').take(63));
        bytes.push(1);
        bytes.push(b'b');
        bytes.push(0);
        let head = format!("{}.", "A".repeat(63));
        assert_eq!(
            datagram_fields(&bytes),
            format!("question {head}.... The payload was not decrypted.")
        );
    }

    #[test]
    fn long_server_name_is_marked() {
        let host = "a".repeat(65);
        let shown = "a".repeat(64);
        assert_eq!(
            datagram_fields(&client_hello(host.as_bytes(), 2)),
            format!("server {shown}.... The payload was not decrypted.")
        );
    }

    #[test]
    fn server_name_in_client_hello() {
        let report = datagram_fields(&client_hello(b"example.com", 2));
        assert!(report.contains("server example.com"), "{report}");
        assert!(report.contains("not decrypted"), "{report}");

        let mut noisy = client_hello(b"example.com", 2);
        for byte in &mut noisy[11..43] {
            *byte = 0x61;
        }
        let noisy_report = datagram_fields(&noisy);
        assert!(noisy_report.contains("server example.com"), "{noisy_report}");
    }

    #[test]
    fn question_with_handshake_marker_is_still_a_question() {
        let mut bytes = vec![0u8; 12];
        bytes[0] = 0x16;
        bytes[5] = 1;
        bytes.extend_from_slice(&[1, b'a', 1, b'b', 0]);
        let report = datagram_fields(&bytes);
        assert!(report.contains("question a.b"), "{report}");
    }

    #[test]
    fn pointer_is_not_followed() {
        let mut bytes = vec![0u8; 12];
        bytes[5] = 1;
        // Pointer to the label below; that label must not be read.
        bytes.extend_from_slice(&[0xC0, 0x0E, 1, b'z', 0]);
        let report = datagram_fields(&bytes);
        assert!(!report.contains("question z"), "{report}");
        assert!(report.contains("not decrypted"), "{report}");
    }

    #[test]
    fn truncated_label_and_short_handshake() {
        let mut truncated = vec![0u8; 12];
        truncated[5] = 1;
        truncated.extend_from_slice(&[5, b'a', b'b']);
        let short = datagram_fields(&truncated);
        assert!(short.contains("No question or server name was decoded"), "{short}");
        assert!(short.contains("not decrypted"), "{short}");

        let handshake = [0x16, 0x03, 0x01, 0x00, 0x01, 0x01];
        let report = datagram_fields(&handshake);
        assert!(
            report.contains("No question or server name was decoded"),
            "{report}"
        );
        assert!(report.contains("not decrypted"), "{report}");
    }

    #[test]
    fn server_name_outside_the_window_is_not_read() {
        let report = datagram_fields(&client_hello(b"example.com", 600));
        assert!(
            report.contains("No question or server name was decoded"),
            "{report}"
        );
        assert!(!report.contains("example.com"), "{report}");
    }

    /// ClientHello with one cipher suite and `cipher_pad` extra cipher bytes
    /// before the server-name extension.
    fn client_hello(host: &[u8], cipher_pad: usize) -> Vec<u8> {
        let list_len = 3 + host.len();
        let sni_body_len = 2 + list_len;
        let ext_len = 4 + sni_body_len;
        let cipher_len = 2 + cipher_pad;
        let hello_len = 2 + 32 + 1 + 2 + cipher_len + 1 + 1 + 2 + ext_len;
        let record_len = 4 + hello_len;
        let mut out = Vec::new();
        out.push(0x16);
        out.extend_from_slice(&[0x03, 0x03]);
        out.extend_from_slice(&(record_len as u16).to_be_bytes());
        out.push(0x01);
        out.push((hello_len >> 16) as u8);
        out.push((hello_len >> 8) as u8);
        out.push(hello_len as u8);
        out.extend_from_slice(&[0x03, 0x03]);
        out.extend_from_slice(&[0u8; 32]);
        out.push(0);
        out.extend_from_slice(&(cipher_len as u16).to_be_bytes());
        out.extend_from_slice(&[0x00, 0x2f]);
        out.extend(std::iter::repeat(0x00).take(cipher_pad));
        out.push(1);
        out.push(0);
        out.extend_from_slice(&(ext_len as u16).to_be_bytes());
        out.extend_from_slice(&0x0000u16.to_be_bytes());
        out.extend_from_slice(&(sni_body_len as u16).to_be_bytes());
        out.extend_from_slice(&(list_len as u16).to_be_bytes());
        out.push(0);
        out.extend_from_slice(&(host.len() as u16).to_be_bytes());
        out.extend_from_slice(host);
        out
    }
}

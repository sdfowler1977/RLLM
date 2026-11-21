//! Saved packet-capture records. Classic little- and big-endian captures, plus
//! section and enhanced-packet blocks of the next capture format. Payload notes
//! are what the bytes themselves show.
#![cfg_attr(not(test), allow(dead_code))]

#[derive(Clone)]
pub struct Record {
    pub index: u64,
    pub timestamp_seconds: u64,
    pub timestamp_fraction: u64,
    pub original_len: u64,
    pub link: u32,
    pub summary: String,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    little: bool,
}

impl<'a> Cursor<'a> {
    fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.at..).unwrap_or(&[])
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }
    #[allow(dead_code)]
    fn u16(&mut self) -> Option<u16> {
        let bytes: [u8; 2] = self.take(2)?.try_into().ok()?;
        Some(if self.little { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) })
    }
    fn u32(&mut self) -> Option<u32> {
        let bytes: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(if self.little { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) })
    }
}

fn payload_note(data: &[u8]) -> String {
    let mut notes = Vec::new();
    if data.len() >= 4 && matches!(&data[..4], b"GET " | b"POST" | b"HEAD" | b"PUT ") {
        let line_end = data.iter().position(|b| *b == b'\r' || *b == b'\n').unwrap_or(data.len().min(80));
        notes.push(format!("http {}", String::from_utf8_lossy(&data[..line_end])));
    }
    if data.first() == Some(&22) && data.len() > 5 && data[5] == 1 {
        if let Some(name) = server_name(data) {
            notes.push(format!("handshake server name {name}"));
        } else {
            notes.push("handshake client hello".into());
        }
    }
    if notes.is_empty() {
        "payload unclassified".into()
    } else {
        notes.join("; ")
    }
}

fn server_name(data: &[u8]) -> Option<String> {
    // Handshake bytes follow the 5-byte record header. The server-name extension
    // is type 0 and carries one host name. Other extensions are skipped by length.
    let hello = data.get(5..)?;
    if hello.first() != Some(&1) || hello.len() < 38 {
        return None;
    }
    let mut at = 4 + 2 + 32; // type+length, version, random
    let session = *hello.get(at)? as usize;
    at = at.checked_add(1 + session)?;
    let cipher_bytes = u16::from_be_bytes(hello.get(at..at + 2)?.try_into().ok()?) as usize;
    at = at.checked_add(2 + cipher_bytes)?;
    let comp = *hello.get(at)? as usize;
    at = at.checked_add(1 + comp)?;
    let ext_bytes = u16::from_be_bytes(hello.get(at..at + 2)?.try_into().ok()?) as usize;
    at += 2;
    let ext_end = at.checked_add(ext_bytes)?.min(hello.len());
    while at + 4 <= ext_end {
        let kind = u16::from_be_bytes(hello.get(at..at + 2)?.try_into().ok()?);
        let len = u16::from_be_bytes(hello.get(at + 2..at + 4)?.try_into().ok()?) as usize;
        at += 4;
        let body = hello.get(at..at.checked_add(len)?)?;
        if kind == 0 && body.len() >= 5 {
            let name_len = u16::from_be_bytes(body[3..5].try_into().ok()?) as usize;
            let name = body.get(5..5 + name_len)?;
            if name.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                return Some(String::from_utf8_lossy(name).into_owned());
            }
        }
        at += len;
    }
    None
}

fn frame_note(link: u32, frame: &[u8]) -> String {
    let l3 = match link {
        1 if frame.len() >= 14 => {
            let kind = u16::from_be_bytes([frame[12], frame[13]]);
            let (kind, rest) = if kind == 0x8100 && frame.len() >= 18 {
                (u16::from_be_bytes([frame[16], frame[17]]), &frame[18..])
            } else {
                (kind, &frame[14..])
            };
            Some((kind, rest))
        }
        101 => Some((if frame.first().is_some_and(|b| b >> 4 == 4) { 0x0800 } else { 0x86DD }, frame)),
        _ => None,
    };
    let Some((kind, packet)) = l3 else {
        return format!("link {link}; {}", payload_note(frame));
    };
    match kind {
        0x0800 if packet.len() >= 20 => {
            let ihl = (packet[0] & 0x0f) as usize * 4;
            if ihl < 20 || packet.len() < ihl {
                return "ipv4 header short".into();
            }
            let proto = packet[9];
            let src = format!("{}.{}.{}.{}", packet[12], packet[13], packet[14], packet[15]);
            let dst = format!("{}.{}.{}.{}", packet[16], packet[17], packet[18], packet[19]);
            transport(proto, &packet[ihl..], &src, &dst)
        }
        0x86DD if packet.len() >= 40 => {
            let proto = packet[6];
            let src = ipv6(&packet[8..24]);
            let dst = ipv6(&packet[24..40]);
            transport(proto, &packet[40..], &src, &dst)
        }
        0x0806 => "arp".into(),
        _ => format!("ethertype {kind:#06x}; {}", payload_note(packet)),
    }
}

fn ipv6(bytes: &[u8]) -> String {
    let mut parts = Vec::new();
    for chunk in bytes.chunks(2) {
        parts.push(format!("{:02x}{:02x}", chunk[0], chunk[1]));
    }
    parts.join(":")
}

fn transport(proto: u8, bytes: &[u8], src: &str, dst: &str) -> String {
    match proto {
        6 | 17 if bytes.len() >= 4 => {
            let sport = u16::from_be_bytes([bytes[0], bytes[1]]);
            let dport = u16::from_be_bytes([bytes[2], bytes[3]]);
            let name = if proto == 6 { "tcp" } else { "udp" };
            let payload = if proto == 6 && bytes.len() >= 13 {
                let offset = ((bytes[12] >> 4) as usize) * 4;
                bytes.get(offset..).unwrap_or(&[])
            } else if proto == 17 && bytes.len() >= 8 {
                &bytes[8..]
            } else {
                &[]
            };
            let extra = if sport == 53 || dport == 53 {
                dns_question(payload).unwrap_or_else(|| payload_note(payload))
            } else {
                payload_note(payload)
            };
            format!("{name} {src}:{sport} -> {dst}:{dport}; {extra}")
        }
        1 => format!("ipv4 icmp {src} -> {dst}"),
        58 => format!("ipv6 icmp {src} -> {dst}"),
        other => format!("protocol {other} {src} -> {dst}; {}", payload_note(bytes)),
    }
}

fn dns_question(payload: &[u8]) -> Option<String> {
    if payload.len() < 12 {
        return None;
    }
    let mut at = 12;
    let mut name = String::new();
    while at < payload.len() {
        let len = payload[at] as usize;
        at += 1;
        if len == 0 {
            break;
        }
        if len > 63 || at + len > payload.len() {
            return None;
        }
        if !name.is_empty() {
            name.push('.');
        }
        let label = &payload[at..at + len];
        if !label.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_') {
            return None;
        }
        name.push_str(&String::from_utf8_lossy(label));
        at += len;
    }
    if name.is_empty() { None } else { Some(format!("dns question {name}")) }
}

fn classic(bytes: &[u8], little: bool, nano: bool, cancelled: &mut dyn FnMut() -> bool) -> Result<Vec<Record>, String> {
    let mut cursor = Cursor { bytes, at: 24, little };
    let link = {
        let mut head = Cursor { bytes, at: 20, little };
        head.u32().ok_or("capture header is short")?
    };
    let mut records = Vec::new();
    while cursor.rest().len() >= 16 {
        if records.len() % 32 == 0 && cancelled() {
            return Err("cancelled".into());
        }
        let seconds = cursor.u32().ok_or("packet header is short")? as u64;
        let fraction = cursor.u32().ok_or("packet header is short")? as u64;
        let included = cursor.u32().ok_or("packet header is short")? as usize;
        let original = cursor.u32().ok_or("packet header is short")? as u64;
        let frame = cursor.take(included).ok_or("packet payload is short")?;
        records.push(Record {
            index: records.len() as u64,
            timestamp_seconds: seconds,
            timestamp_fraction: fraction,
            original_len: original,
            link,
            summary: format!("{}{}", frame_note(link, frame), if nano { " (fraction is nanoseconds)" } else { "" }),
        });
    }
    if !cursor.rest().is_empty() {
        return Err("trailing capture bytes do not form a packet header".into());
    }
    Ok(records)
}

fn blocks(bytes: &[u8], cancelled: &mut dyn FnMut() -> bool) -> Result<Vec<Record>, String> {
    if bytes.len() < 12 || &bytes[..4] != b"\n\r\r\n" {
        return Err("capture does not start with a section header".into());
    }
    let little = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) == 0x1A2B3C4D;
    if !little && u32::from_be_bytes(bytes[8..12].try_into().unwrap()) != 0x1A2B3C4D {
        return Err("section byte order is not recognized".into());
    }
    let mut at = 0usize;
    let mut link = 1u32;
    let mut records = Vec::new();
    while at + 8 <= bytes.len() {
        if records.len() % 32 == 0 && cancelled() {
            return Err("cancelled".into());
        }
        let kind = if little { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) } else { u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) };
        let total = if little { u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) } else { u32::from_be_bytes(bytes[at + 4..at + 8].try_into().unwrap()) } as usize;
        if total < 12 || at + total > bytes.len() {
            return Err("capture block length is outside the file".into());
        }
        let body = &bytes[at + 8..at + total - 4];
        if kind == 0x0000_0001 && body.len() >= 2 {
            link = if little { u16::from_le_bytes(body[0..2].try_into().unwrap()) } else { u16::from_be_bytes(body[0..2].try_into().unwrap()) } as u32;
        }
        if kind == 0x0000_0006 && body.len() >= 20 {
            let included = if little { u32::from_le_bytes(body[12..16].try_into().unwrap()) } else { u32::from_be_bytes(body[12..16].try_into().unwrap()) } as usize;
            let original = if little { u32::from_le_bytes(body[16..20].try_into().unwrap()) } else { u32::from_be_bytes(body[16..20].try_into().unwrap()) } as u64;
            let frame = body.get(20..20 + included).ok_or("enhanced packet is short")?;
            let seconds = if little { u32::from_le_bytes(body[4..8].try_into().unwrap()) } else { u32::from_be_bytes(body[4..8].try_into().unwrap()) } as u64;
            records.push(Record {
                index: records.len() as u64,
                timestamp_seconds: seconds,
                timestamp_fraction: 0,
                original_len: original,
                link,
                summary: frame_note(link, frame),
            });
        }
        at += total;
    }
    if at != bytes.len() {
        return Err("trailing capture bytes do not form a block".into());
    }
    Ok(records)
}

pub fn read(bytes: &[u8], cancelled: &mut dyn FnMut() -> bool) -> Result<Vec<Record>, String> {
    if bytes.len() < 24 {
        return Err("capture is shorter than a file header".into());
    }
    match &bytes[..4] {
        b"\xd4\xc3\xb2\xa1" => classic(bytes, true, false, cancelled),
        b"\xa1\xb2\xc3\xd4" => classic(bytes, false, false, cancelled),
        b"\x4d\x3c\xb2\xa1" => classic(bytes, true, true, cancelled),
        b"\xa1\xb2\x3c\x4d" => classic(bytes, false, true, cancelled),
        b"\n\r\r\n" => blocks(bytes, cancelled),
        _ => Err("capture magic is not a classic or section-block file".into()),
    }
}

fn read_exact(input: &mut dyn std::io::Read, n: usize) -> Result<Option<Vec<u8>>, String> {
    let mut buf = vec![0u8; n];
    let mut filled = 0;
    while filled < n {
        match input.read(&mut buf[filled..]).map_err(|e| e.to_string())? {
            0 if filled == 0 => return Ok(None),
            0 => return Err("capture record is shorter than its header".into()),
            got => filled += got,
        }
    }
    Ok(Some(buf))
}

fn read_some(input: &mut dyn std::io::Read, n: usize) -> Result<Vec<u8>, String> {
    read_exact(input, n)?.ok_or_else(|| "capture record is shorter than its header".into())
}

fn discard(input: &mut dyn std::io::Read, mut n: usize) -> Result<(), String> {
    let mut buf = [0u8; 65536];
    while n > 0 {
        let chunk = n.min(buf.len());
        let got = input.read(&mut buf[..chunk]).map_err(|e| e.to_string())?;
        if got == 0 {
            return Err("capture record is shorter than its header".into());
        }
        n -= got;
    }
    Ok(())
}

/// Write one line per packet without retaining the capture. A classic file and a
/// section-block file both stream forward; a truncated record is an error.
/// Notes use the first 4096 bytes of a packet. The rest is read and counted.
pub fn transcribe(input: &mut dyn std::io::Read, output: &mut dyn std::io::Write, cancelled: &mut dyn FnMut() -> bool) -> Result<u64, String> {
    const NOTE: usize = 4096;
    let magic = read_some(input, 4)?;
    let mut count = 0u64;
    match magic.as_slice() {
        b"\xd4\xc3\xb2\xa1" | b"\xa1\xb2\xc3\xd4" | b"\x4d\x3c\xb2\xa1" | b"\xa1\xb2\x3c\x4d" => {
            let little = matches!(magic.as_slice(), b"\xd4\xc3\xb2\xa1" | b"\x4d\x3c\xb2\xa1");
            let nano = matches!(magic.as_slice(), b"\x4d\x3c\xb2\xa1" | b"\xa1\xb2\x3c\x4d");
            let rest = read_some(input, 20)?;
            let mut head = Cursor { bytes: &rest, at: 16, little };
            let link = head.u32().ok_or("capture header is short")?;
            loop {
                if count % 32 == 0 && cancelled() {
                    return Err("cancelled".into());
                }
                let Some(header) = read_exact(input, 16)? else { break };
                let mut cursor = Cursor { bytes: &header, at: 0, little };
                let seconds = cursor.u32().ok_or("packet header is short")? as u64;
                let fraction = cursor.u32().ok_or("packet header is short")? as u64;
                let included = cursor.u32().ok_or("packet header is short")? as usize;
                let original = cursor.u32().ok_or("packet header is short")? as u64;
                let note_len = included.min(NOTE);
                let frame = read_some(input, note_len)?;
                if included > note_len {
                    discard(input, included - note_len)?;
                }
                let note = if nano { " (fraction is nanoseconds)" } else { "" };
                let tail = if included > note_len { " (note covers the first 4096 packet bytes)" } else { "" };
                writeln!(output, "#{count} t={seconds}.{fraction:06} original={original} link={link} {}{note}{tail}", frame_note(link, &frame)).map_err(|e| e.to_string())?;
                count += 1;
            }
        }
        b"\n\r\r\n" => {
            let first = read_some(input, 8)?;
            let mut block = magic;
            block.extend_from_slice(&first);
            let little = u32::from_le_bytes(block[8..12].try_into().unwrap()) == 0x1A2B3C4D;
            if !little && u32::from_be_bytes(block[8..12].try_into().unwrap()) != 0x1A2B3C4D {
                return Err("section byte order is not recognized".into());
            }
            let mut link = 1u32;
            loop {
                if count % 32 == 0 && cancelled() {
                    return Err("cancelled".into());
                }
                let kind = if little { u32::from_le_bytes(block[0..4].try_into().unwrap()) } else { u32::from_be_bytes(block[0..4].try_into().unwrap()) };
                let total = if little { u32::from_le_bytes(block[4..8].try_into().unwrap()) } else { u32::from_be_bytes(block[4..8].try_into().unwrap()) } as usize;
                if total < block.len() || total < 12 {
                    return Err("capture block length is outside the file".into());
                }
                let body = total - block.len();
                if kind == 0x0000_0006 && body >= 24 {
                    let head = read_some(input, 20)?;
                    let included = if little { u32::from_le_bytes(head[12..16].try_into().unwrap()) } else { u32::from_be_bytes(head[12..16].try_into().unwrap()) } as usize;
                    let original = if little { u32::from_le_bytes(head[16..20].try_into().unwrap()) } else { u32::from_be_bytes(head[16..20].try_into().unwrap()) } as u64;
                    let seconds = if little { u32::from_le_bytes(head[4..8].try_into().unwrap()) } else { u32::from_be_bytes(head[4..8].try_into().unwrap()) } as u64;
                    if included > body - 24 {
                        return Err("enhanced packet is short".into());
                    }
                    let note_len = included.min(NOTE);
                    let frame = read_some(input, note_len)?;
                    discard(input, body - 20 - note_len)?;
                    let tail = if included > note_len { " (note covers the first 4096 packet bytes)" } else { "" };
                    writeln!(output, "#{count} t={seconds}.000000 original={original} link={link} {}{tail}", frame_note(link, &frame)).map_err(|e| e.to_string())?;
                    count += 1;
                } else if kind == 0x0000_0006 {
                    return Err("enhanced packet is short".into());
                } else {
                    let take = body.min(64);
                    let sample = read_some(input, take)?;
                    if body > take {
                        discard(input, body - take)?;
                    }
                    if kind == 0x0000_0001 && sample.len() >= 2 {
                        link = if little { u16::from_le_bytes(sample[0..2].try_into().unwrap()) } else { u16::from_be_bytes(sample[0..2].try_into().unwrap()) } as u32;
                    }
                }
                match read_exact(input, 8)? {
                    Some(next) => block = next,
                    None => break,
                }
            }
        }
        _ => return Err("capture magic is not a classic or section-block file".into()),
    }
    Ok(count)
}

pub fn report(records: &[Record]) -> String {
    let mut out = format!("packets: {}\n", records.len());
    for record in records {
        out.push_str(&format!(
            "#{} t={}.{:06} original={} link={} {}\n",
            record.index, record.timestamp_seconds, record.timestamp_fraction, record.original_len, record.link, record.summary
        ));
    }
    if records.is_empty() {
        out.push_str("The capture contains no packet records.\n");
    }
    out
}

#[cfg(test)]
mod checks {
    use super::*;

    fn classic_http() -> Vec<u8> {
        let mut file = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 1, 0, 0, 0];
        let mut frame = vec![0u8; 14];
        frame[12] = 0x08;
        let mut ip = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 6, 0, 0, 10, 0, 0, 2, 10, 0, 0, 1];
        let mut tcp = vec![0, 80, 0x1f, 0x90, 0, 0, 0, 0, 0, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, 0];
        tcp.extend_from_slice(b"GET /index HTTP/1.0\r\n");
        ip.extend_from_slice(&tcp);
        frame.extend_from_slice(&ip);
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        file.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        file.extend(frame);
        file
    }

    #[test]
    fn ethernet_ipv4_tcp_http_is_named_from_the_bytes() {
        let records = read(&classic_http(), &mut || false).unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].summary.contains("tcp 10.0.0.2:80 -> 10.0.0.1:8080"));
        assert!(records[0].summary.contains("http GET /index HTTP/1.0"));
        let mut streamed = Vec::new();
        let n = transcribe(&mut std::io::Cursor::new(classic_http()), &mut streamed, &mut || false).unwrap();
        assert_eq!(n, 1);
        assert!(String::from_utf8(streamed).unwrap().contains("http GET /index HTTP/1.0"));
    }

    #[test]
    fn dns_question_is_a_label_not_a_guess() {
        let mut payload = vec![0u8; 12];
        payload.extend_from_slice(b"\x07example\x03com\x00");
        assert_eq!(dns_question(&payload).as_deref(), Some("dns question example.com"));
    }
}

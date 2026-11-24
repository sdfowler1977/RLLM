//! Stored-method and raw-deflate zip members, including one HAR text member.
//! At most 16 entry names are listed. The first name that ends with `.har`
//! is extracted when its method is stored (method 0) or a raw deflate block
//! (method 8) using fixed Huffman codes, including a length-distance copy.
//! A dynamic Huffman block is not read.
//! At most 16 URL rows are taken from the literal marker `"url": "`.
//! The 17th entry is named when another entry exists, and later rows are
//! reported as not listed when another URL exists.
//! The archive is not installed and the session is not replayed.
//! This is a literal marker scan, not a document parser.

const ENTRY_CAP: usize = 16;
const ROW_CAP: usize = 16;
const SCAN_CAP: usize = 256;
const TEXT_CAP: usize = 64;
const MEMBER_CAP: u32 = 1024 * 1024;
const URL_MARK: &str = "\"url\": \"";

struct StoredHar {
    name: String,
    method: u16,
    size: u32,
    compressed: u32,
    crc: u32,
    local: u32,
}

struct Directory {
    listed: Vec<String>,
    omitted: bool,
    omitted_name: Option<String>,
    har: Option<StoredHar>,
    extra_har: bool,
}

pub fn saved_archive(bytes: &[u8]) -> String {
    let mut report = report_body(bytes);
    if !report.ends_with('\n') {
        report.push('\n');
    }
    report.push_str("The archive was not installed.\n");
    report.push_str("The session was not replayed.\n");
    report
}

fn report_body(bytes: &[u8]) -> String {
    if bytes.len() < 22 || !bytes.starts_with(b"PK\x03\x04") {
        return "Not a zip archive.".to_string();
    }
    let Some(end) = find_eocd(bytes) else {
        return "The zip directory was not found.".to_string();
    };
    let Some(directory) = read_directory(bytes, end) else {
        return "The zip directory was not found.".to_string();
    };
    let mut lines = Vec::new();
    for (index, name) in directory.listed.iter().enumerate() {
        lines.push(format!("entry {index} {name}"));
    }
    if directory.omitted {
        match &directory.omitted_name {
            Some(name) => lines.push(format!("later entries were not listed: {name}")),
            None => lines.push("later entries were not listed".to_string()),
        }
    }
    match &directory.har {
        None => lines.push("No har member was declared.".to_string()),
        Some(har) => {
            match member_bytes(bytes, har) {
                Err(error) => lines.push(format!("member {} was not extracted: {error}", har.name)),
                Ok(body) => match std::str::from_utf8(&body) {
                    Err(_) => lines.push("har text was not read".to_string()),
                    Ok(text) => push_rows(&mut lines, text),
                },
            }
            if directory.extra_har {
                lines.push("later har members were not read".to_string());
            }
        }
    }
    lines.join("\n")
}

fn push_rows(lines: &mut Vec<String>, text: &str) {
    let (urls, more) = url_rows(text);
    if urls.is_empty() {
        lines.push("No url row was declared.".to_string());
        return;
    }
    for (index, url) in urls.iter().enumerate() {
        lines.push(format!("row {} {url}", index + 1));
    }
    if more {
        lines.push("later rows were not listed".to_string());
    }
}

fn url_rows(text: &str) -> (Vec<String>, bool) {
    let mut urls = Vec::new();
    let mut from = 0usize;
    let mut more = false;
    while from < text.len() {
        let Some(rel) = text[from..].find(URL_MARK) else {
            break;
        };
        let start = from + rel + URL_MARK.len();
        let Some(end_rel) = text[start..].find('"') else {
            break;
        };
        let candidate = &text[start..start + end_rel];
        from = start + end_rel + 1;
        if candidate.is_empty() || candidate.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
            continue;
        }
        if urls.len() == ROW_CAP {
            more = true;
            break;
        }
        urls.push(shown(candidate));
    }
    (urls, more)
}

fn read_directory(bytes: &[u8], end: usize) -> Option<Directory> {
    let disk_count = read_u16(bytes, end + 8)?;
    let count = usize::from(read_u16(bytes, end + 10)?);
    let size = read_u32(bytes, end + 12)?;
    let offset = read_u32(bytes, end + 16)?;
    if disk_count == 0xFFFF || count == 0xFFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
        return None;
    }
    if usize::from(disk_count) != count {
        return None;
    }
    let offset = offset as usize;
    if offset > end || offset > bytes.len() {
        return None;
    }
    if count > 0 && bytes.get(offset..offset.checked_add(4)?) != Some(b"PK\x01\x02") {
        return None;
    }
    let mut directory = Directory {
        listed: Vec::new(),
        omitted: count > SCAN_CAP,
        omitted_name: None,
        har: None,
        extra_har: false,
    };
    let mut at = offset;
    let walk = count.min(SCAN_CAP);
    for _ in 0..walk {
        if bytes.get(at..at.checked_add(4)?) != Some(b"PK\x01\x02") {
            directory.omitted = true;
            break;
        }
        let method = read_u16(bytes, at + 10)?;
        let crc = read_u32(bytes, at + 16)?;
        let compressed = read_u32(bytes, at + 20)?;
        let size = read_u32(bytes, at + 24)?;
        let name_len = usize::from(read_u16(bytes, at + 28)?);
        let extra_len = usize::from(read_u16(bytes, at + 30)?);
        let comment_len = usize::from(read_u16(bytes, at + 32)?);
        let local = read_u32(bytes, at + 42)?;
        let name_at = at.checked_add(46)?;
        let name_end = name_at.checked_add(name_len)?;
        let next = name_end.checked_add(extra_len)?.checked_add(comment_len)?;
        if next > end {
            directory.omitted = true;
            break;
        }
        let raw = bytes.get(name_at..name_end)?;
        let name = display_name(raw);
        if directory.listed.len() < ENTRY_CAP {
            directory.listed.push(name.clone());
        } else {
            directory.omitted = true;
            if directory.omitted_name.is_none() {
                directory.omitted_name = Some(name.clone());
            }
        }
        if raw.ends_with(b".har") {
            if directory.har.is_none() {
                directory.har = Some(StoredHar {
                    name,
                    method,
                    size,
                    compressed,
                    crc,
                    local,
                });
            } else {
                directory.extra_har = true;
            }
        }
        if next <= at {
            directory.omitted = true;
            break;
        }
        at = next;
    }
    Some(directory)
}

fn member_bytes(bytes: &[u8], har: &StoredHar) -> Result<Vec<u8>, String> {
    if har.method != 0 && har.method != 8 {
        return Err(format!("method {} is not stored", har.method));
    }
    if har.size > MEMBER_CAP || har.compressed > MEMBER_CAP {
        return Err("member exceeds the size cap".to_string());
    }
    if har.method == 0 && har.compressed != har.size {
        return Err("stored size does not match".to_string());
    }
    let local = har.local as usize;
    if bytes.get(local..local.checked_add(4).ok_or("local header is absent")?) != Some(b"PK\x03\x04") {
        return Err("local header is absent".to_string());
    }
    let flags = read_u16(bytes, local + 6).ok_or("local header is short")?;
    if flags & 1 != 0 {
        return Err("encrypted member is not extracted".to_string());
    }
    let local_method = read_u16(bytes, local + 8).ok_or("local header is short")?;
    if local_method != har.method {
        return Err(format!("method {local_method} is not stored"));
    }
    if flags & 8 == 0 {
        let local_crc = read_u32(bytes, local + 14).ok_or("local header is short")?;
        let local_compressed = read_u32(bytes, local + 18).ok_or("local header is short")?;
        let local_size = read_u32(bytes, local + 22).ok_or("local header is short")?;
        if local_crc != har.crc || local_compressed != har.compressed || local_size != har.size {
            return Err("stored size does not match".to_string());
        }
    }
    let name_len = usize::from(read_u16(bytes, local + 26).ok_or("local header is short")?);
    let extra_len = usize::from(read_u16(bytes, local + 28).ok_or("local header is short")?);
    let data_at = local
        .checked_add(30)
        .and_then(|value| value.checked_add(name_len))
        .and_then(|value| value.checked_add(extra_len))
        .ok_or("local header is short")?;
    let span = if har.method == 8 { har.compressed } else { har.size };
    let data_end = data_at.checked_add(span as usize).ok_or("member bytes are outside the archive")?;
    let data = bytes.get(data_at..data_end).ok_or("member bytes are outside the archive")?;
    let plain = if har.method == 8 {
        inflate_deflate(data, har.size as usize)?
    } else {
        data.to_vec()
    };
    if plain.len() != har.size as usize {
        return Err("deflated size does not match".to_string());
    }
    if crc32(&plain) != har.crc {
        return Err("stored checksum does not match".to_string());
    }
    Ok(plain)
}

fn inflate_deflate(data: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    if let Some(plain) = inflate_stored_block(data, expected) {
        return Ok(plain);
    }
    if let Some(plain) = inflate_fixed_blocks(data, expected) {
        return Ok(plain);
    }
    #[cfg(not(test))]
    {
        return crate::compression::inflate(data, expected).map_err(|error| error.to_string());
    }
}

struct InflateBits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl InflateBits<'_> {
    fn bit(&mut self) -> Option<u32> {
        if self.pos >= self.data.len().saturating_mul(8) {
            return None;
        }
        let value = u32::from((self.data[self.pos / 8] >> (self.pos % 8)) & 1);
        self.pos += 1;
        Some(value)
    }

    fn take(&mut self, count: u32) -> Option<u32> {
        let mut value = 0;
        for shift in 0..count {
            value |= self.bit()? << shift;
        }
        Some(value)
    }
}

fn fixed_symbol(bits: &mut InflateBits<'_>) -> Option<usize> {
    const LENGTHS: [usize; 288] = {
        let mut lens = [0usize; 288];
        let mut index = 0;
        while index < 144 {
            lens[index] = 8;
            index += 1;
        }
        while index < 256 {
            lens[index] = 9;
            index += 1;
        }
        while index < 280 {
            lens[index] = 7;
            index += 1;
        }
        while index < 288 {
            lens[index] = 8;
            index += 1;
        }
        lens
    };
    let mut counts = [0usize; 16];
    for length in LENGTHS {
        counts[length] += 1;
    }
    let mut offsets = [0usize; 16];
    let mut next = 0usize;
    for length in 1..16 {
        offsets[length] = next;
        next += counts[length];
    }
    let mut symbols = [0usize; 288];
    for (symbol, length) in LENGTHS.iter().copied().enumerate() {
        symbols[offsets[length]] = symbol;
        offsets[length] += 1;
    }
    let mut code = 0i32;
    let mut first = 0i32;
    let mut index = 0usize;
    for length in 1..=15 {
        code |= bits.bit()? as i32;
        let count = counts[length] as i32;
        if code - count < first {
            return Some(symbols[index + (code - first) as usize]);
        }
        index += count as usize;
        first += count;
        first <<= 1;
        code <<= 1;
    }
    None
}

fn inflate_fixed_blocks(data: &[u8], expected: usize) -> Option<Vec<u8>> {
    const LENGTH_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DISTANCE_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DISTANCE_EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
    ];
    let mut bits = InflateBits { data, pos: 0 };
    let mut out = Vec::new();
    loop {
        let last = bits.bit()?;
        let kind = bits.take(2)?;
        if kind == 0 {
            let skip = (8 - (bits.pos % 8)) % 8;
            bits.pos += skip;
            let start = bits.pos / 8;
            if start + 4 > data.len() {
                return None;
            }
            let len = u16::from_le_bytes([data[start], data[start + 1]]) as usize;
            let nlen = u16::from_le_bytes([data[start + 2], data[start + 3]]);
            if (len as u16) ^ nlen != 0xffff || start + 4 + len > data.len() {
                return None;
            }
            out.extend_from_slice(&data[start + 4..start + 4 + len]);
            bits.pos = (start + 4 + len) * 8;
        } else if kind == 1 {
            loop {
                let symbol = fixed_symbol(&mut bits)?;
                if symbol < 256 {
                    out.push(symbol as u8);
                } else if symbol == 256 {
                    break;
                } else if symbol <= 285 {
                    let index = symbol - 257;
                    let extra = bits.take(LENGTH_EXTRA[index])? as usize;
                    let length = LENGTH_BASE[index] + extra;
                    let distance_symbol = bits.take(5)? as usize;
                    if distance_symbol >= 30 {
                        return None;
                    }
                    let distance_extra = bits.take(DISTANCE_EXTRA[distance_symbol])? as usize;
                    let distance = DISTANCE_BASE[distance_symbol] + distance_extra;
                    if distance == 0 || distance > out.len() || out.len() + length > expected {
                        return None;
                    }
                    for _ in 0..length {
                        let byte = out[out.len() - distance];
                        out.push(byte);
                    }
                } else {
                    return None;
                }
                if out.len() > expected {
                    return None;
                }
            }
        } else {
            return None;
        }
        if out.len() > expected {
            return None;
        }
        if last == 1 {
            break;
        }
    }
    if out.len() == expected { Some(out) } else { None }
}

fn inflate_stored_block(data: &[u8], expected: usize) -> Option<Vec<u8>> {
    if data.first() != Some(&0x01) || data.len() < 5 {
        return None;
    }
    let len = u16::from_le_bytes([data[1], data[2]]) as usize;
    let nlen = u16::from_le_bytes([data[3], data[4]]);
    if (len as u16) ^ nlen != 0xffff || len != expected || data.len() < 5 + len {
        return None;
    }
    Some(data[5..5 + len].to_vec())
}

fn display_name(raw: &[u8]) -> String {
    let sample = if raw.len() > 256 { &raw[..256] } else { raw };
    shown(&String::from_utf8_lossy(sample))
}

fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut more = false;
    for (index, ch) in text.chars().enumerate() {
        if index == TEXT_CAP {
            more = true;
            break;
        }
        if ch.is_control() {
            out.push('?');
        } else {
            out.push(ch);
        }
    }
    if more {
        out.push_str("...");
    }
    out
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 22 {
        return None;
    }
    let start = bytes.len().saturating_sub(22 + 65_535);
    let mut at = bytes.len() - 22;
    loop {
        if bytes.get(at..at + 4) == Some(b"PK\x05\x06") {
            if let Some(comment) = read_u16(bytes, at + 20) {
                let comment = usize::from(comment);
                if at.checked_add(22).and_then(|value| value.checked_add(comment)) == Some(bytes.len()) {
                    return Some(at);
                }
            }
        }
        if at == start {
            return None;
        }
        at -= 1;
    }
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let slice = bytes.get(at..end)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let slice = bytes.get(at..end)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

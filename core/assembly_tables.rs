//! CLI metadata table bits read from an opaque byte slice.
//! The slice is not loaded and the assembly is not executed.

#![forbid(unsafe_code)]

pub fn assembly_tables(bytes: &[u8]) -> String {
    let Some(origin) = find_signature(bytes) else {
        return "CLI metadata was not found.".to_string();
    };
    let Some(root) = read_root(bytes, origin) else {
        return "CLI metadata was not found.".to_string();
    };

    let mut lines = vec![format!("cli {}.{}", root.major, root.minor)];
    let mut tables = "none".to_string();
    let mut saw_tables = false;
    let mut unread_tables = false;
    let mut read = 0usize;
    let mut unread = root.count > 8;
    if let Some(mut cursor) = root.streams {
        for _ in 0..root.count.min(8) {
            let Some(stream) = read_stream(bytes, origin, cursor) else {
                unread = true;
                break;
            };
            lines.push(format!(
                "stream {} offset {:#x} size {}",
                stream.name, stream.offset, stream.size
            ));
            if !saw_tables {
                if let Some(valid) = stream.valid {
                    let (names, more) = table_names(valid);
                    tables = names;
                    unread_tables = more;
                    saw_tables = true;
                }
            }
            cursor = stream.next;
            read += 1;
        }
        if read < root.count {
            unread = true;
        }
    }
    if unread {
        lines.push("later streams were not listed".to_string());
    }
    if tables != "none" || !unread_tables {
        lines.push(format!("tables {tables}"));
    }
    if unread_tables {
        lines.push("later tables were not listed".to_string());
    }
    lines.push("The assembly was not executed.".to_string());
    lines.join("\n")
}

struct Root {
    major: u16,
    minor: u16,
    count: usize,
    streams: Option<usize>,
}

struct Stream {
    offset: u32,
    size: u32,
    name: String,
    next: usize,
    valid: Option<u64>,
}

fn find_signature(bytes: &[u8]) -> Option<usize> {
    let end = bytes.len().min(64);
    bytes[..end].windows(4).position(|word| word == b"BSJB")
}

fn read_root(bytes: &[u8], origin: usize) -> Option<Root> {
    let major = read_u16(bytes, origin.checked_add(4)?)?;
    let minor = read_u16(bytes, origin.checked_add(6)?)?;
    let version_len = match read_u32(bytes, origin.checked_add(8)?) {
        Some(len) => len as usize,
        None => {
            return Some(Root {
                major,
                minor,
                count: 0,
                streams: None,
            })
        }
    };
    let after_version = 12usize.checked_add(version_len)?;
    let version_end = origin.checked_add(after_version)?;
    if version_end > bytes.len() {
        return Some(Root {
            major,
            minor,
            count: 0,
            streams: None,
        });
    }
    let aligned = align4(after_version)?;
    let flags_at = origin.checked_add(aligned)?;
    let Some(count) = read_u16(bytes, flags_at.checked_add(2)?) else {
        return Some(Root {
            major,
            minor,
            count: 0,
            streams: None,
        });
    };
    Some(Root {
        major,
        minor,
        count: count as usize,
        streams: Some(flags_at.checked_add(4)?),
    })
}

fn read_stream(bytes: &[u8], origin: usize, cursor: usize) -> Option<Stream> {
    let offset = read_u32(bytes, cursor)?;
    let size = read_u32(bytes, cursor.checked_add(4)?)?;
    let name_at = cursor.checked_add(8)?;
    let (raw, next) = read_name(bytes, name_at)?;
    let valid = if raw == b"#~" || raw == b"#-" {
        read_valid(bytes, origin, offset)
    } else {
        None
    };
    Some(Stream {
        offset,
        size,
        name: name_text(raw),
        next,
        valid,
    })
}

fn read_name(bytes: &[u8], name_at: usize) -> Option<(&[u8], usize)> {
    let rest = bytes.get(name_at..)?;
    let nul = rest.iter().position(|&byte| byte == 0)?;
    let stored = align4(nul.checked_add(1)?)?;
    let next = name_at.checked_add(stored)?;
    Some((&rest[..nul], next))
}

fn read_valid(bytes: &[u8], origin: usize, offset: u32) -> Option<u64> {
    let body = origin.checked_add(offset as usize)?;
    let _sorted = read_u64(bytes, body.checked_add(16)?)?;
    read_u64(bytes, body.checked_add(8)?)
}

fn table_names(valid: u64) -> (String, bool) {
    let mut names = Vec::new();
    if valid & 1 != 0 {
        names.push("Module");
    }
    if valid & 2 != 0 {
        names.push("TypeRef");
    }
    if valid & 4 != 0 {
        names.push("TypeDef");
    }
    let text = if names.is_empty() {
        "none".to_string()
    } else {
        names.join(" ")
    };
    (text, valid & !0b111 != 0)
}

fn name_text(raw: &[u8]) -> String {
    raw.iter()
        .map(|&byte| {
            if (0x20..0x7f).contains(&byte) {
                byte as char
            } else {
                '?'
            }
        })
        .collect()
}

fn align4(value: usize) -> Option<usize> {
    let rem = value % 4;
    if rem == 0 {
        Some(value)
    } else {
        value.checked_add(4 - rem)
    }
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let word = bytes.get(at..end)?;
    Some(u16::from_le_bytes([word[0], word[1]]))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let word = bytes.get(at..end)?;
    Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    let word = bytes.get(at..end)?;
    Some(u64::from_le_bytes([
        word[0], word[1], word[2], word[3], word[4], word[5], word[6], word[7],
    ]))
}

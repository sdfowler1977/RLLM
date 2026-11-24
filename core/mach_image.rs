//! Bounded read of a captured 64-bit little-endian Mach-O image.
//! The image is not mapped and not executed.

const MAGIC: u32 = 0xFEEDFACF;
const HEADER_SIZE: usize = 32;
const SEGMENT_64: u32 = 0x19;
const MAX_COMMANDS: usize = 8;

pub fn mach_image(bytes: &[u8]) -> String {
    if read_u32(bytes, 0) != Some(MAGIC) {
        return "Not a 64-bit little-endian image.\n".to_string();
    }
    let (Some(cputype), Some(filetype), Some(ncmds), Some(sizeofcmds)) = (
        read_u32(bytes, 4),
        read_u32(bytes, 12),
        read_u32(bytes, 16),
        read_u32(bytes, 20),
    ) else {
        return "The image was not mapped.\n".to_string();
    };

    let mut out = format!("image cpu 0x{cputype:x} type {filetype} commands {ncmds}\n");
    let Some(declared_end) = HEADER_SIZE.checked_add(sizeofcmds as usize) else {
        out.push_str("later load commands were not read\nThe image was not mapped.\n");
        return out;
    };
    let unread = (ncmds as usize) > MAX_COMMANDS || commands_unread(bytes, ncmds, declared_end);

    let mut offset = HEADER_SIZE;
    let walk = (ncmds as usize).min(MAX_COMMANDS);
    for _ in 0..walk {
        let Some(header_end) = offset.checked_add(8) else {
            break;
        };
        if header_end > declared_end || header_end > bytes.len() {
            break;
        }
        let Some(cmd) = read_u32(bytes, offset) else {
            break;
        };
        let Some(cmdsize) = read_u32(bytes, offset + 4) else {
            break;
        };
        if cmdsize < 8 {
            break;
        }
        let Some(next) = offset.checked_add(cmdsize as usize) else {
            break;
        };
        if next > declared_end || next > bytes.len() {
            break;
        }
        if cmd == SEGMENT_64 {
            if let Some(line) = segment_line(bytes, offset, next) {
                out.push_str(&line);
            }
            out.push_str(&section_lines(bytes, offset, next));
        }
        offset = next;
    }

    if unread {
        out.push_str("later load commands were not read\n");
    }
    out.push_str("The image was not mapped.\n");
    out
}

fn commands_unread(bytes: &[u8], ncmds: u32, declared_end: usize) -> bool {
    if ncmds as usize > MAX_COMMANDS {
        return true;
    }
    let mut offset = HEADER_SIZE;
    let mut read = 0usize;
    for _ in 0..ncmds {
        let Some(header_end) = offset.checked_add(8) else {
            return true;
        };
        if header_end > declared_end || header_end > bytes.len() {
            return true;
        }
        let Some(cmdsize) = read_u32(bytes, offset + 4) else {
            return true;
        };
        if cmdsize < 8 {
            return read < ncmds as usize;
        }
        let Some(next) = offset.checked_add(cmdsize as usize) else {
            return true;
        };
        if next > declared_end || next > bytes.len() {
            return true;
        }
        offset = next;
        read += 1;
    }
    read < ncmds as usize
}

fn segment_line(bytes: &[u8], command: usize, command_end: usize) -> Option<String> {
    let fields_end = command.checked_add(40)?;
    if fields_end > command_end {
        return None;
    }
    let name_bytes = bytes.get(command + 8..command + 24)?;
    let name_end = name_bytes.iter().position(|b| *b == 0).unwrap_or(name_bytes.len());
    let raw = &name_bytes[..name_end];
    let name = if raw.is_empty() {
        "(unnamed)".to_string()
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };
    let vmaddr = read_u64(bytes, command + 24)?;
    let vmsize = read_u64(bytes, command + 32)?;
    Some(format!("segment {name} vm 0x{vmaddr:x} size {vmsize}\n"))
}

fn section_lines(bytes: &[u8], command: usize, command_end: usize) -> String {
    let Some(nsects) = read_u32(bytes, command.saturating_add(64)) else {
        return String::new();
    };
    let mut out = String::new();
    let shown = (nsects as usize).min(16);
    let mut read = 0usize;
    for index in 0..shown {
        let Some(at) = command.checked_add(72).and_then(|base| base.checked_add(index.saturating_mul(80))) else {
            break;
        };
        if at.saturating_add(48) > command_end {
            break;
        }
        let name = fixed_name(bytes, at, 16);
        let Some(addr) = read_u64(bytes, at + 32) else {
            break;
        };
        let Some(size) = read_u64(bytes, at + 40) else {
            break;
        };
        out.push_str(&format!("section {name} addr 0x{addr:x} size {size}\n"));
        read += 1;
    }
    if nsects as usize > read {
        out.push_str("later sections were not listed\n");
    }
    out
}

fn fixed_name(bytes: &[u8], at: usize, len: usize) -> String {
    let Some(raw) = bytes.get(at..at + len) else {
        return "(unnamed)".to_string();
    };
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    if end == 0 {
        "(unnamed)".to_string()
    } else {
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    Some(u64::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

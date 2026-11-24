//! Bounded read of library names in a captured 64-bit little-endian Mach-O image.
//! The libraries are not loaded.

const MAGIC: u32 = 0xFEEDFACF;
const HEADER_SIZE: usize = 32;
const LOAD_DYLIB: u32 = 0xC;
const ID_DYLIB: u32 = 0xD;
const MAX_COMMANDS: usize = 16;
const MAX_NAME: usize = 128;
const TEXT_CAP: usize = 64;

pub fn mach_libraries(bytes: &[u8]) -> String {
    if read_u32(bytes, 0) != Some(MAGIC) {
        return "Not a 64-bit little-endian image.\n".to_string();
    }
    let (Some(ncmds), Some(sizeofcmds)) = (read_u32(bytes, 16), read_u32(bytes, 20)) else {
        return "No library command was declared.\nThe libraries were not loaded.\n".to_string();
    };

    let mut out = String::new();
    let mut found = false;
    let Some(declared_end) = HEADER_SIZE.checked_add(sizeofcmds as usize) else {
        out.push_str("No library command was declared.\nlater load commands were not read\nThe libraries were not loaded.\n");
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
        if cmd == LOAD_DYLIB || cmd == ID_DYLIB {
            found = true;
            if let Some(name) = library_name(bytes, offset, next) {
                out.push_str(&format!("library {}\n", shown_name(&name)));
            }
        }
        offset = next;
    }

    if !found {
        out.push_str("No library command was declared.\n");
    }
    if unread {
        out.push_str("later load commands were not read\n");
    }
    out.push_str("The libraries were not loaded.\n");
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

fn library_name(bytes: &[u8], command: usize, command_end: usize) -> Option<String> {
    let field = command.checked_add(8)?;
    let field_end = field.checked_add(4)?;
    if field_end > command_end {
        return None;
    }
    let name_off = read_u32(bytes, field)? as usize;
    let start = command.checked_add(name_off)?;
    if start >= command_end {
        return None;
    }
    let cap_end = start.saturating_add(MAX_NAME);
    let end = cap_end.min(command_end);
    let slice = bytes.get(start..end)?;
    let name_end = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
    Some(String::from_utf8_lossy(&slice[..name_end]).into_owned())
}

fn shown_name(name: &str) -> String {
    let mut out = String::new();
    let mut more = false;
    for (index, ch) in name.chars().enumerate() {
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

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

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

#[cfg(test)]
mod tests {
    use super::mach_libraries;

    #[test]
    fn one_lib() {
        let mut bytes = vec![0u8; 32 + 32];
        bytes[0..4].copy_from_slice(&0xFEEDFACFu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&32u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0xCu32.to_le_bytes());
        bytes[36..40].copy_from_slice(&32u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&24u32.to_le_bytes());
        bytes[56..61].copy_from_slice(b"libz\0");
        let report = mach_libraries(&bytes);
        assert!(report.contains("libz"), "{report}");
        assert!(report.contains("not loaded"), "{report}");
    }

    #[test]
    fn short_command_table_is_named() {
        let mut bytes = vec![0u8; 64];
        bytes[0..4].copy_from_slice(&0xFEEDFACFu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&64u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0xCu32.to_le_bytes());
        bytes[36..40].copy_from_slice(&32u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&24u32.to_le_bytes());
        bytes[56..61].copy_from_slice(b"libz\0");
        assert_eq!(
            mach_libraries(&bytes),
            "library libz\nlater load commands were not read\nThe libraries were not loaded.\n"
        );
    }

    #[test]
    fn long_library_name_is_marked() {
        let name = "A".repeat(65);
        let mut bytes = vec![0u8; 32 + 24 + 66];
        bytes[0..4].copy_from_slice(&0xFEEDFACFu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&90u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0xCu32.to_le_bytes());
        bytes[36..40].copy_from_slice(&90u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&24u32.to_le_bytes());
        bytes[56..121].copy_from_slice(name.as_bytes());
        let shown = "A".repeat(64);
        assert_eq!(
            mach_libraries(&bytes),
            format!("library {shown}...\nThe libraries were not loaded.\n")
        );
    }

    #[test]
    fn absent_magic() {
        let report = mach_libraries(&[0xCE, 0xFA, 0xED, 0xFE]);
        assert_eq!(report, "Not a 64-bit little-endian image.\n");
    }

    #[test]
    fn no_library() {
        let mut bytes = vec![0u8; 32 + 8];
        bytes[0..4].copy_from_slice(&0xFEEDFACFu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&8u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&8u32.to_le_bytes());
        let report = mach_libraries(&bytes);
        assert_eq!(
            report,
            "No library command was declared.\nThe libraries were not loaded.\n"
        );
    }
}

//! Bounded read of a Mach-O symbol table in a captured 64-bit little-endian image.
//! The symbols are not linked.

const MAGIC: u32 = 0xFEEDFACF;
const HEADER_SIZE: usize = 32;
const SYMTAB: u32 = 0x2;
const MAX_COMMANDS: usize = 16;
const MAX_SYMBOLS: usize = 16;
const TEXT_CAP: usize = 64;

pub fn mach_symbols(bytes: &[u8]) -> String {
    if read_u32(bytes, 0) != Some(MAGIC) {
        return "Not a 64-bit little-endian image.\n".to_string();
    }
    let (Some(ncmds), Some(sizeofcmds)) = (read_u32(bytes, 16), read_u32(bytes, 20)) else {
        return "No symbol table was declared.\nThe symbols were not linked.\n".to_string();
    };
    let Some(declared_end) = HEADER_SIZE.checked_add(sizeofcmds as usize) else {
        return "No symbol table was declared.\nlater load commands were not read\nThe symbols were not linked.\n".to_string();
    };
    let unread = (ncmds as usize) > MAX_COMMANDS || commands_unread(bytes, ncmds, declared_end);
    let table = first_symtab(bytes, ncmds, declared_end);

    let mut out = String::new();
    match table {
        None => out.push_str("No symbol table was declared.\n"),
        Some(table) => out.push_str(&symbol_lines(bytes, &table)),
    }
    if unread {
        out.push_str("later load commands were not read\n");
    }
    out.push_str("The symbols were not linked.\n");
    out
}

struct Symtab {
    symoff: usize,
    nsyms: usize,
    stroff: usize,
    strsize: usize,
}

fn first_symtab(bytes: &[u8], ncmds: u32, declared_end: usize) -> Option<Symtab> {
    let mut offset = HEADER_SIZE;
    let walk = (ncmds as usize).min(MAX_COMMANDS);
    for _ in 0..walk {
        let header_end = offset.checked_add(8)?;
        if header_end > declared_end || header_end > bytes.len() {
            return None;
        }
        let cmd = read_u32(bytes, offset)?;
        let cmdsize = read_u32(bytes, offset + 4)?;
        if cmdsize < 8 {
            return None;
        }
        let next = offset.checked_add(cmdsize as usize)?;
        if next > declared_end || next > bytes.len() {
            return None;
        }
        if cmd == SYMTAB && cmdsize >= 24 {
            let symoff = read_u32(bytes, offset + 8)? as usize;
            let nsyms = read_u32(bytes, offset + 12)? as usize;
            let stroff = read_u32(bytes, offset + 16)? as usize;
            let strsize = read_u32(bytes, offset + 20)? as usize;
            return Some(Symtab { symoff, nsyms, stroff, strsize });
        }
        offset = next;
    }
    None
}

fn symbol_lines(bytes: &[u8], table: &Symtab) -> String {
    let mut out = String::new();
    let mut listed = 0usize;
    let mut missing = false;
    let walk = table.nsyms.min(MAX_SYMBOLS);
    for index in 0..walk {
        let Some(at) = table.symoff.checked_add(index.saturating_mul(16)) else {
            missing = true;
            break;
        };
        if at.checked_add(16).is_none_or(|end| end > bytes.len()) {
            missing = true;
            break;
        }
        let strx = read_u32(bytes, at).unwrap_or(0) as usize;
        let kind = bytes[at + 4];
        let section = bytes[at + 5];
        let value = read_u64(bytes, at + 8).unwrap_or(0);
        let name = symbol_name(bytes, table.stroff, table.strsize, strx);
        out.push_str(&format!("symbol {index} {name} strx {strx} type 0x{kind:x} sect {section} value 0x{value:x}\n"));
        listed += 1;
    }
    if missing || listed < table.nsyms.min(MAX_SYMBOLS) {
        out.push_str("later symbols were not read\n");
    } else if table.nsyms > MAX_SYMBOLS {
        out.push_str("later symbols were not listed\n");
    }
    out
}

fn symbol_name(bytes: &[u8], stroff: usize, strsize: usize, strx: usize) -> String {
    if strx == 0 {
        return "(unnamed)".to_string();
    }
    if strx >= strsize {
        return "unreadable".to_string();
    }
    let Some(start) = stroff.checked_add(strx) else {
        return "unreadable".to_string();
    };
    let Some(table_end) = stroff.checked_add(strsize) else {
        return "unreadable".to_string();
    };
    let table_end = table_end.min(bytes.len());
    let Some(raw) = bytes.get(start..table_end) else {
        return "unreadable".to_string();
    };
    let mut name = String::new();
    let mut ended = false;
    for byte in raw.iter().copied() {
        if byte == 0 {
            ended = true;
            break;
        }
        if name.chars().count() == TEXT_CAP {
            name.push_str("...");
            return name;
        }
        name.push(byte as char);
    }
    if name.is_empty() {
        return "unreadable".to_string();
    }
    if !ended {
        name.push_str("...");
    }
    name
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

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    Some(u64::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

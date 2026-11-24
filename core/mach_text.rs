//! 64-bit little-endian Mach-O text runs reached by ADRP plus ADD or load.
//! A listed reference is not executed.

#![forbid(unsafe_code)]

const MAGIC: u32 = 0xFEEDFACF;
const HEADER: usize = 32;
const CODE_CAP: usize = 262_144;
const STRING_CAP: usize = 128;
const REPORT_CAP: usize = 32;
const LOOKAHEAD: usize = 8;
const MIN_RUN: usize = 6;

struct Load {
    file_off: u64,
    file_end: u64,
    vaddr: u64,
    exec: bool,
}

struct Section {
    name: String,
    addr: u64,
    end: u64,
}

struct Run {
    vaddr: u64,
    text: String,
}

pub fn mach_text(bytes: &[u8]) -> String {
    let mut report = String::new();
    if read_u32(bytes, 0) == Some(MAGIC) && read_u32(bytes, 4) != Some(0x0100_000c) {
        report.push_str("not an ARM64 image\n");
        report.push_str("The reference was not executed.");
        return report;
    }
    let Some((loads, sections, code, unread, unread_sections)) = image(bytes) else {
        report.push_str("no 64-bit little-endian Mach-O image was read\n");
        report.push_str("The reference was not executed.");
        return report;
    };
    if unread_sections && code.is_empty() {
        report.push_str("no address reference reached a text run\n");
        report.push_str("later segment sections were not read\n");
        if unread {
            report.push_str("later load commands were not read\n");
        }
        report.push_str("The reference was not executed.");
        return report;
    }
    if section_misses_segment(&loads, &code) {
        report.push_str("instruction section did not overlap an executable segment\n");
        if unread {
            report.push_str("later load commands were not read\n");
        }
        report.push_str("The reference was not executed.");
        return report;
    }
    let runs = runs(bytes, &loads);
    let mut listed = 0usize;
    let mut hidden = false;
    for load in loads.iter().filter(|load| load.exec) {
        if hidden {
            break;
        }
        let start = load.file_off as usize;
        let end = (load.file_end as usize).min(bytes.len()).min(start.saturating_add(CODE_CAP));
        let mut off = start;
        let commands_end = read_u32(bytes, 20)
            .map(|size| HEADER.saturating_add(size as usize))
            .unwrap_or(HEADER)
            .min(bytes.len());
        if off < commands_end {
            off = commands_end;
        }
        if off % 4 != 0 {
            off += 4 - (off % 4);
        }
                while off + 4 <= end {
                    if !code.is_empty() && !code.iter().any(|&(from, to)| off >= from && off < to) {
                        off += 4;
                        continue;
                    }
                    let word = word_at(bytes, off);
            let Some(pc) = vaddr_at(&loads, off) else {
                off += 4;
                continue;
            };
            if let Some((register, page)) = page_address(word, pc) {
                if let Some(hit) = following(bytes, &loads, &sections, &runs, off, end, register, page) {
                    if listed >= REPORT_CAP {
                        hidden = true;
                        break;
                    }
                    if listed > 0 {
                        report.push('\n');
                    }
                    report.push_str(&hit);
                    listed += 1;
                }
            }
            off += 4;
        }
    }
    if listed == 0 {
        report.push_str("no address reference reached a text run\n");
    } else {
        report.push('\n');
        if hidden {
            report.push_str("later references were not listed\n");
        }
    }
    if unread {
        report.push_str("later load commands were not read\n");
    }
    if unread_sections {
        report.push_str("later segment sections were not read\n");
    }
    report.push_str("The reference was not executed.");
    report
}

fn image(bytes: &[u8]) -> Option<(Vec<Load>, Vec<Section>, Vec<(usize, usize)>, bool, bool)> {
    if bytes.len() < HEADER || read_u32(bytes, 0)? != MAGIC {
        return None;
    }
    let commands = read_u32(bytes, 16)? as usize;
    let size = read_u32(bytes, 20)? as usize;
    let declared = HEADER.checked_add(size)?;
    let mut loads = Vec::new();
    let mut sections = Vec::new();
    let mut code = Vec::new();
    let mut offset = HEADER;
    let mut read = 0usize;
    let mut unread_sections = false;
    for _ in 0..commands.min(64) {
        if offset.checked_add(8)? > declared.min(bytes.len()) {
            break;
        }
        let cmd = read_u32(bytes, offset)?;
        let cmdsize = read_u32(bytes, offset + 4)? as usize;
        if cmdsize < 8 {
            break;
        }
        let next = offset.checked_add(cmdsize)?;
        if next > declared.min(bytes.len()) {
            break;
        }
        if cmd == 0x19 && cmdsize >= 72 {
            let vmaddr = read_u64(bytes, offset + 24)?;
            let file_off = read_u64(bytes, offset + 40)?;
            let filesz = read_u64(bytes, offset + 48)?;
            let prot = read_u32(bytes, offset + 60)?;
            if let Some(file_end) = file_off.checked_add(filesz) {
                loads.push(Load { file_off, file_end, vaddr: vmaddr, exec: prot & 4 != 0 });
            }
            let count = read_u32(bytes, offset + 64).unwrap_or(0) as usize;
            if count > 16 {
                unread_sections = true;
            }
            for index in 0..count.min(16) {
                if sections.len() >= 32 {
                    unread_sections = true;
                    break;
                }
                let Some(at) = offset.checked_add(72).and_then(|base| base.checked_add(index.saturating_mul(80))) else {
                    unread_sections = true;
                    break;
                };
                if at.saturating_add(48) > next {
                    unread_sections = true;
                    break;
                }
                let Some(addr) = read_u64(bytes, at + 32) else {
                    break;
                };
                let Some(size) = read_u64(bytes, at + 40) else {
                    break;
                };
                let Some(end) = addr.checked_add(size) else {
                    continue;
                };
                sections.push(Section { name: fixed_name(bytes, at, 16), addr, end });
                if at.saturating_add(68) <= next {
                    if let Some(flags) = read_u32(bytes, at + 64) {
                        if flags & 0x8000_0400 != 0 {
                            if let (Some(file_off), Some(size)) = (read_u32(bytes, at + 48), usize::try_from(size).ok()) {
                                let file_off = file_off as usize;
                                if size > 0 && file_off < bytes.len() {
                                    let code_end = file_off.saturating_add(size).min(bytes.len());
                                    if code_end > file_off {
                                        code.push((file_off, code_end));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        offset = next;
        read += 1;
    }
    Some((loads, sections, code, read < commands, unread_sections))
}

fn section_misses_segment(loads: &[Load], code: &[(usize, usize)]) -> bool {
    if code.is_empty() {
        return false;
    }
    let mut executable = false;
    for load in loads.iter().filter(|load| load.exec) {
        executable = true;
        let Ok(from) = usize::try_from(load.file_off) else {
            continue;
        };
        let Ok(end) = usize::try_from(load.file_end) else {
            continue;
        };
        if code.iter().any(|&(start, stop)| start < end && stop > from) {
            return false;
        }
    }
    executable
}

fn runs(bytes: &[u8], loads: &[Load]) -> Vec<Run> {
    let mut kept = Vec::new();
    let limit = bytes.len().min(1 << 20);
    let mut index = 0usize;
    while index < limit && kept.len() < STRING_CAP {
        if !printable(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < limit && printable(bytes[index]) {
            index += 1;
        }
        if index - start < MIN_RUN {
            continue;
        }
        let Some(vaddr) = vaddr_at(loads, start) else {
            continue;
        };
        let end_off = index - 1;
        if vaddr_at(loads, end_off) != Some(vaddr + (end_off - start) as u64) {
            continue;
        }
        kept.push(Run {
            vaddr,
            text: bytes[start..index].iter().copied().map(char::from).collect(),
        });
    }
    kept
}

fn following(bytes: &[u8], loads: &[Load], sections: &[Section], runs: &[Run], off: usize, end: usize, register: u8, page: u64) -> Option<String> {
    for step in 1..=LOOKAHEAD {
        let next = off + step * 4;
        if next + 4 > end {
            return None;
        }
        let word = word_at(bytes, next);
        let Some(use_pc) = vaddr_at(loads, next) else {
            return None;
        };
        if let Some((next_register, _)) = page_address(word, use_pc) {
            if next_register == register {
                return None;
            }
        }
        if let Some((dest, base, displacement)) = add_immediate(word) {
            if base == register {
                return line(runs, sections, page.wrapping_add(displacement), off, loads, next, "add");
            }
            if dest == register {
                return None;
            }
        }
        if let Some((base, displacement)) = load_immediate(word) {
            if base == register {
                return line(runs, sections, page.wrapping_add(displacement), off, loads, next, "load");
            }
        }
    }
    None
}

fn line(runs: &[Run], sections: &[Section], address: u64, adrp_off: usize, loads: &[Load], use_off: usize, kind: &str) -> Option<String> {
    let run = runs.iter().find(|run| {
        let end = run.vaddr + run.text.len() as u64;
        address >= run.vaddr && address < end
    })?;
    let adrp = vaddr_at(loads, adrp_off)?;
    let used = vaddr_at(loads, use_off)?;
    let shown = shown_text(&run.text);
    let place = sections.iter().find(|section| run.vaddr >= section.addr && run.vaddr < section.end).map(|section| format!(" section {}", section.name)).unwrap_or_default();
    Some(format!("text {:#x}{place} near adrp {adrp:#x} {kind} {used:#x} {shown}", run.vaddr))
}

fn shown_text(text: &str) -> String {
    const TEXT_CAP: usize = 48;
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

fn page_address(word: u32, pc: u64) -> Option<(u8, u64)> {
    if word & 0x9f00_0000 != 0x9000_0000 {
        return None;
    }
    let register = (word & 31) as u8;
    let immlo = ((word >> 29) & 3) as i64;
    let immhi = ((word >> 5) & 0x7_ffff) as i64;
    let mut immediate = (immhi << 2) | immlo;
    if immediate & (1 << 20) != 0 {
        immediate -= 1 << 21;
    }
    let page = (pc & !0xfff) as i64;
    let address = page.checked_add(immediate.checked_mul(4096)?)?;
    if address < 0 {
        return None;
    }
    Some((register, address as u64))
}

fn add_immediate(word: u32) -> Option<(u8, u8, u64)> {
    let shift = match word & 0xffc0_0000 {
        0x9100_0000 => 0,
        0x9140_0000 => 12,
        _ => return None,
    };
    let immediate = ((word >> 10) & 0xfff) as u64;
    let base = ((word >> 5) & 31) as u8;
    let dest = (word & 31) as u8;
    Some((dest, base, immediate << shift))
}

fn load_immediate(word: u32) -> Option<(u8, u64)> {
    let scale = match word & 0xffc0_0000 {
        0xf940_0000 => 8,
        0xb940_0000 => 4,
        0x3940_0000 => 1,
        _ => return None,
    };
    let immediate = ((word >> 10) & 0xfff) as u64;
    let base = ((word >> 5) & 31) as u8;
    Some((base, immediate * scale))
}

fn vaddr_at(loads: &[Load], offset: usize) -> Option<u64> {
    let offset = offset as u64;
    loads.iter().find(|load| offset >= load.file_off && offset < load.file_end).map(|load| load.vaddr + (offset - load.file_off))
}

fn word_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let raw = bytes.get(offset..offset + 8)?;
    Some(u64::from_le_bytes(raw.try_into().ok()?))
}

fn printable(byte: u8) -> bool {
    (0x20..=0x7e).contains(&byte)
}

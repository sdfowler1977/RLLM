//! 64-bit little-endian AArch64 text runs reached by ADRP plus ADD or load.
//! A listed reference is not executed.

#![forbid(unsafe_code)]

const CODE_CAP: usize = 262_144;
const STRING_CAP: usize = 128;
const EXTRA_CAP: usize = 32;
const REPORT_CAP: usize = 32;
const LOOKAHEAD: usize = 8;
const MIN_RUN: usize = 6;
const HEADER: usize = 64;
const PROGRAM: usize = 56;

struct Load {
    file_off: u64,
    file_end: u64,
    vaddr: u64,
    exec: bool,
}

struct Run {
    vaddr: u64,
    text: String,
}

struct Section {
    name: String,
    addr: u64,
    end: u64,
}

pub fn arm_text(bytes: &[u8]) -> String {
    let mut report = String::new();
    match image(bytes) {
        Image::Other => report.push_str("no 64-bit little-endian AArch64 image was read\n"),
        Image::Short => report.push_str("the image is shorter than an ELF program header\n"),
        Image::Ready(loads) => {
            if too_many_sections(bytes) {
                report.push_str("the section table has more than 64 headers\n");
            } else if too_many_program_headers(bytes) {
                report.push_str("the program header table has more than 64 headers\n");
            } else if truncated_program_headers(bytes) && loads.is_empty() {
                report.push_str("the program header table was truncated\n");
            } else if truncated_section_table(bytes) && exec_ranges(bytes).is_empty() {
                report.push_str("the section table was truncated\n");
            } else if section_misses_load(bytes, &loads) {
                report.push_str("executable section did not overlap an executable load\n");
            } else {
            let sections = sections(bytes);
            let runs = runs(bytes, &loads);
            let preamble = preamble_end(bytes);
            let section_table = section_table(bytes);
            let executable = exec_ranges(bytes);
            let mut listed = 0usize;
            let mut hidden = false;
            for load in loads.iter().filter(|load| load.exec) {
                if hidden {
                    break;
                }
                let start = load.file_off as usize;
                let end = (load.file_end as usize).min(bytes.len()).min(start.saturating_add(CODE_CAP));
                let mut off = start;
                if off < preamble {
                    off = preamble;
                }
                if off % 4 != 0 {
                    off += 4 - (off % 4);
                }
                while off + 4 <= end {
                    if let Some((from, to)) = section_table {
                        if off >= from && off < to {
                            off = to;
                            if off % 4 != 0 {
                                off += 4 - (off % 4);
                            }
                            continue;
                        }
                    }
                    if !executable.is_empty() && !executable.iter().any(|&(from, to)| off >= from && off < to) {
                        off += 4;
                        continue;
                    }
                    let word = word_at(bytes, off);
                    let Some(pc) = vaddr_at(&loads, off) else {
                        off += 4;
                        continue;
                    };
                    if let Some((rd, page)) = page_address(word, pc) {
                        if let Some(hit) = following(bytes, &loads, &sections, &runs, off, end, rd, page) {
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
            if truncated_section_table(bytes) {
                report.push_str("later section headers were not read\n");
            }
            if truncated_program_headers(bytes) {
                report.push_str("later program headers were not read\n");
            }
            }
        }
    }
    report.push_str("The reference was not executed.");
    report
}

enum Image {
    Other,
    Short,
    Ready(Vec<Load>),
}

fn exec_ranges(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let Some((table, _)) = section_table(bytes) else {
        return ranges;
    };
    if bytes.len() < 64 {
        return ranges;
    }
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]) as usize;
    let shnum = u16::from_le_bytes([bytes[60], bytes[61]]) as usize;
    for index in 0..shnum.min(64) {
        let Some(at) = table.checked_add(index.saturating_mul(entsize)) else {
            break;
        };
        let Some(header) = bytes.get(at..at.saturating_add(64)) else {
            break;
        };
        let flags = u64::from_le_bytes(header[8..16].try_into().unwrap_or([0; 8]));
        if flags & 4 == 0 {
            continue;
        }
        let Ok(offset) = usize::try_from(u64::from_le_bytes(header[24..32].try_into().unwrap_or([0; 8]))) else {
            continue;
        };
        let Ok(size) = usize::try_from(u64::from_le_bytes(header[32..40].try_into().unwrap_or([0; 8]))) else {
            continue;
        };
        if size == 0 || offset >= bytes.len() {
            continue;
        }
        let end = offset.saturating_add(size).min(bytes.len());
        if end > offset {
            ranges.push((offset, end));
        }
    }
    ranges
}

fn too_many_program_headers(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || &bytes[..4] != b"\x7fELF" || bytes[4] != 2 {
        return false;
    }
    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap_or([0; 8]));
    let phentsize = u16::from_le_bytes([bytes[54], bytes[55]]);
    let phnum = u16::from_le_bytes([bytes[56], bytes[57]]);
    phoff != 0 && phentsize >= 56 && phnum > 64
}

fn truncated_program_headers(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || &bytes[..4] != b"\x7fELF" || bytes[4] != 2 {
        return false;
    }
    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap_or([0; 8]));
    let phentsize = u16::from_le_bytes([bytes[54], bytes[55]]) as u64;
    let phnum = u16::from_le_bytes([bytes[56], bytes[57]]) as u64;
    if phoff == 0 || phentsize < 56 || phnum == 0 || phnum > 64 {
        return false;
    }
    let Some(end) = phentsize.checked_mul(phnum).and_then(|size| phoff.checked_add(size)) else {
        return true;
    };
    end > bytes.len() as u64
}

fn too_many_sections(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || &bytes[..4] != b"\x7fELF" || bytes[4] != 2 {
        return false;
    }
    let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap_or([0; 8]));
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]);
    let shnum = u16::from_le_bytes([bytes[60], bytes[61]]);
    shoff != 0 && entsize >= 64 && shnum > 64
}

fn truncated_section_table(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || &bytes[..4] != b"\x7fELF" || bytes[4] != 2 {
        return false;
    }
    let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap_or([0; 8]));
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]) as u64;
    let shnum = u16::from_le_bytes([bytes[60], bytes[61]]) as u64;
    if shoff == 0 || entsize < 64 || shnum == 0 || shnum > 64 {
        return false;
    }
    let Some(end) = entsize.checked_mul(shnum).and_then(|size| shoff.checked_add(size)) else {
        return true;
    };
    end > bytes.len() as u64
}

fn section_misses_load(bytes: &[u8], loads: &[Load]) -> bool {
    let sections = exec_ranges(bytes);
    if sections.is_empty() {
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
        let to = end.min(bytes.len());
        if from < to && sections.iter().any(|&(start, stop)| start < to && stop > from) {
            return false;
        }
    }
    executable
}

fn section_table(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 64 {
        return None;
    }
    let shoff = u64::from_le_bytes(bytes[40..48].try_into().ok()?);
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]) as u64;
    let shnum = u16::from_le_bytes([bytes[60], bytes[61]]) as u64;
    if shoff == 0 || entsize < 64 || shnum == 0 || shnum > 64 {
        return None;
    }
    let end = entsize.checked_mul(shnum)?.checked_add(shoff)?;
    let start = usize::try_from(shoff).ok()?;
    let end = usize::try_from(end).ok()?.min(bytes.len());
    if start >= end {
        return None;
    }
    Some((start, end))
}

fn preamble_end(bytes: &[u8]) -> usize {
    let mut end = 64.min(bytes.len());
    if bytes.len() < 64 {
        return end;
    }
    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap_or([0; 8]));
    let phentsize = u16::from_le_bytes([bytes[54], bytes[55]]) as u64;
    let phnum = u16::from_le_bytes([bytes[56], bytes[57]]) as u64;
    if phoff == 0 || phentsize < 56 || phnum == 0 || phnum > 64 {
        return end;
    }
    let Some(table) = phentsize.checked_mul(phnum).and_then(|size| phoff.checked_add(size)) else {
        return end;
    };
    if let Ok(table) = usize::try_from(table) {
        end = end.max(table).min(bytes.len());
    }
    end
}

fn image(bytes: &[u8]) -> Image {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[4] != 2 || bytes[5] != 1 {
        return Image::Other;
    }
    if u16::from_le_bytes([bytes[18], bytes[19]]) != 183 {
        return Image::Other;
    }
    if bytes.len() < HEADER {
        return Image::Short;
    }
    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
    let phentsize = u16::from_le_bytes([bytes[54], bytes[55]]) as u64;
    let phnum = u16::from_le_bytes([bytes[56], bytes[57]]) as u64;
    if phentsize < PROGRAM as u64 || phnum > 64 {
        return Image::Ready(Vec::new());
    }
    let mut loads = Vec::new();
    for index in 0..phnum {
        let Some(at) = phoff.checked_add(index * phentsize) else {
            break;
        };
        let at = at as usize;
        if at.checked_add(PROGRAM).map(|end| end > bytes.len()).unwrap_or(true) {
            break;
        }
        if u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) != 1 {
            continue;
        }
        let flags = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
        let file_off = u64::from_le_bytes(bytes[at + 8..at + 16].try_into().unwrap());
        let vaddr = u64::from_le_bytes(bytes[at + 16..at + 24].try_into().unwrap());
        let filesz = u64::from_le_bytes(bytes[at + 32..at + 40].try_into().unwrap());
        let Some(file_end) = file_off.checked_add(filesz) else {
            continue;
        };
        loads.push(Load { file_off, file_end, vaddr, exec: flags & 1 != 0 });
    }
    Image::Ready(loads)
}

fn runs(bytes: &[u8], loads: &[Load]) -> Vec<Run> {
    let mut kept = Vec::new();
    let limit = bytes.len().min(1 << 20);
    let mut index = 0usize;
    while index < limit {
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
        let run = Run {
            vaddr,
            text: bytes[start..index].iter().copied().map(char::from).collect(),
        };
        if wanted(&kept, &run) {
            kept.push(run);
        }
    }
    kept
}

fn wanted(kept: &[Run], run: &Run) -> bool {
    let ordinary = kept.iter().filter(|item| !notable(&item.text)).count();
    let extra = kept.len() - ordinary;
    if notable(&run.text) {
        ordinary < STRING_CAP || extra < EXTRA_CAP
    } else {
        ordinary < STRING_CAP
    }
}

fn notable(text: &str) -> bool {
    text.contains("permission")
        || text.contains("Audio")
        || text.contains("microphone")
        || text.contains("MediaRecorder")
        || text.contains("RECORD_")
        || text.contains("getUserMedia")
        || text.contains("ultrasonic")
}

fn sections(bytes: &[u8]) -> Vec<Section> {
    if bytes.len() < HEADER {
        return Vec::new();
    }
    let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]) as usize;
    let count = u16::from_le_bytes([bytes[60], bytes[61]]) as usize;
    let names = u16::from_le_bytes([bytes[62], bytes[63]]) as usize;
    if shoff == 0 || entsize < 64 || count == 0 || names >= count {
        return Vec::new();
    }
    let shown = count.min(64);
    let Some(name_header) = shoff.checked_add(names.saturating_mul(entsize)) else {
        return Vec::new();
    };
    if name_header.saturating_add(64) > bytes.len() {
        return Vec::new();
    }
    let name_off = u64::from_le_bytes(bytes[name_header + 24..name_header + 32].try_into().unwrap()) as usize;
    let name_size = u64::from_le_bytes(bytes[name_header + 32..name_header + 40].try_into().unwrap()) as usize;
    let Some(table) = bytes.get(name_off..name_off.saturating_add(name_size)) else {
        return Vec::new();
    };
    let mut sections = Vec::new();
    for index in 0..shown {
        if sections.len() >= 32 {
            break;
        }
        let Some(at) = shoff.checked_add(index.saturating_mul(entsize)) else {
            break;
        };
        if at.saturating_add(64) > bytes.len() {
            break;
        }
        let name_at = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let addr = u64::from_le_bytes(bytes[at + 16..at + 24].try_into().unwrap());
        let size = u64::from_le_bytes(bytes[at + 32..at + 40].try_into().unwrap());
        let Some(end) = addr.checked_add(size) else {
            continue;
        };
        if size == 0 {
            continue;
        }
        let name = table.get(name_at..).and_then(|raw| raw.iter().position(|byte| *byte == 0).map(|end| &raw[..end])).unwrap_or(&[]);
        if name.is_empty() {
            continue;
        }
        sections.push(Section { name: String::from_utf8_lossy(name).into_owned(), addr, end });
    }
    sections
}

fn following(bytes: &[u8], loads: &[Load], sections: &[Section], runs: &[Run], off: usize, end: usize, rd: u8, page: u64) -> Option<String> {
    for step in 1..=LOOKAHEAD {
        let next = off + step * 4;
        if next + 4 > end {
            return None;
        }
        let word = word_at(bytes, next);
        let Some(use_pc) = vaddr_at(loads, next) else {
            return None;
        };
        if let Some((next_rd, _)) = page_address(word, use_pc) {
            if next_rd == rd {
                return None;
            }
        }
        if let Some((dest, base, displacement)) = add_immediate(word) {
            if base == rd {
                return line(runs, sections, page.wrapping_add(displacement), off, loads, next, "add");
            }
            if dest == rd {
                return None;
            }
        }
        if let Some((base, displacement)) = load_immediate(word) {
            if base == rd {
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

fn word_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn vaddr_at(loads: &[Load], offset: usize) -> Option<u64> {
    let offset = offset as u64;
    loads.iter().find(|load| offset >= load.file_off && offset < load.file_end).map(|load| load.vaddr + (offset - load.file_off))
}

fn printable(byte: u8) -> bool {
    (0x20..=0x7e).contains(&byte)
}

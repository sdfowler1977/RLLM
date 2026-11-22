//! Little-endian AArch64 and ARM64 Mach-O branches, reported and left unexecuted.

#![forbid(unsafe_code)]

const INSTRUCTION_LIMIT: usize = 64;
const BYTE_CAP: usize = 1 << 18;

pub fn arm_branch(bytes: &[u8]) -> String {
    if let Some(report) = mach_branch(bytes) {
        return report;
    }
    if let Some(report) = pe_branch(bytes) {
        return report;
    }
    if too_many_program_headers(bytes) {
        return "the program header table has more than 64 headers\nThe branch was not executed.".to_string();
    }
    if truncated_program_headers(bytes) && executable_loads(bytes).is_empty() {
        return "the program header table was truncated\nThe branch was not executed.".to_string();
    }
    let unread_program_headers = truncated_program_headers(bytes);
    let mut report = String::new();
    let mut listed = 0usize;
    let start = code_start(bytes);
    let spans = section_spans(bytes);
    let pauth = pauth_declared(bytes);
    let mut more = false;
    let mut unread_headers = false;
    match exec_ranges(bytes) {
        Coverage::NoOverlap => {
            report.push_str("executable section did not overlap an executable load\n");
            report.push_str("The branch was not executed.");
            return report;
        }
        Coverage::TooMany => {
            report.push_str("the section table has more than 64 headers\n");
            report.push_str("The branch was not executed.");
            return report;
        }
        Coverage::Truncated => {
            report.push_str("the section table was truncated\n");
            report.push_str("The branch was not executed.");
            return report;
        }
        Coverage::Ranges(ranges, unread) => {
        unread_headers = unread;
        let mut examined = 0usize;
        for (index, (from, to)) in ranges.iter().enumerate() {
            let mut off = (*from).max(start);
            if off % 4 != 0 {
                off += 4 - (off % 4);
            }
            while off + 4 <= *to && listed < INSTRUCTION_LIMIT && examined < BYTE_CAP {
                note_branch(bytes, off, &spans, pauth, &mut report, &mut listed);
                off += 4;
                examined += 4;
            }
            if listed >= INSTRUCTION_LIMIT || examined >= BYTE_CAP {
                if off + 4 <= *to || index + 1 < ranges.len() {
                    more = true;
                }
                break;
            }
        }
        }
        Coverage::Window => {
            let available = bytes.len().saturating_sub(start) / 4;
            let words = available.min(INSTRUCTION_LIMIT);
            for index in 0..words {
                note_branch(bytes, start + index * 4, &spans, pauth, &mut report, &mut listed);
            }
            more = available > INSTRUCTION_LIMIT;
        }
    }
    if listed == 0 {
        report.push_str("no aligned branch was decoded");
    }
    if more {
        if listed > 0 {
            report.push('\n');
        }
        report.push_str("later branches were not listed");
    }
    if unread_headers {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str("later section headers were not read");
    }
    if unread_program_headers {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str("later program headers were not read");
    }
    report.push('\n');
    report.push_str("The branch was not executed.");
    report
}

fn too_many_program_headers(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 {
        return false;
    }
    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap_or([0; 8]));
    let phentsize = u16::from_le_bytes([bytes[54], bytes[55]]);
    let phnum = u16::from_le_bytes([bytes[56], bytes[57]]);
    phoff != 0 && phentsize >= 56 && phnum > 64
}

fn truncated_program_headers(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 {
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

fn note_branch(bytes: &[u8], offset: usize, spans: &[Span], pauth: bool, report: &mut String, listed: &mut usize) {
    let Some(raw) = bytes.get(offset..offset + 4) else {
        return;
    };
    let word = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let Some(found) = decode_word(word, pauth) else {
        return;
    };
    if *listed > 0 {
        report.push('\n');
    }
    report.push_str(&format!("branch at {offset:#x}"));
    write_place(report, spans, offset as i64, bytes.len());
    report.push_str(" kind ");
    found.write_kind(report);
    found.write_target(report, offset, spans, bytes.len());
    *listed += 1;
}

struct Span {
    name: String,
    start: usize,
    end: usize,
}

fn write_place(report: &mut String, spans: &[Span], address: i64, file_len: usize) {
    if spans.is_empty() {
        return;
    }
    let Some(at) = usize::try_from(address).ok() else {
        report.push_str(" outside");
        return;
    };
    if at >= file_len {
        report.push_str(" outside");
        return;
    }
    if let Some(span) = spans.iter().find(|span| at >= span.start && at < span.end) {
        report.push_str(" section ");
        report.push_str(&span.name);
    } else {
        report.push_str(" no section");
    }
}

enum Word {
    Named(&'static str, i64),
    Compare { nonzero: bool, wide: bool, register: u8, imm: i64 },
    Test { nonzero: bool, bit: u8, register: u8, imm: i64 },
    Register { kind: &'static str, register: u8 },
    Authenticated { name: &'static str, register: u8, modifier: Option<u8> },
}

impl Word {
    fn write_target(&self, report: &mut String, offset: usize, spans: &[Span], file_len: usize) {
        match *self {
            Word::Register { .. } => report.push_str(" target unresolved"),
            Word::Authenticated { .. } => {
                report.push_str(" target unresolved authentication was not performed")
            }
            Word::Named(_, imm) | Word::Compare { imm, .. } | Word::Test { imm, .. } => {
                let target = offset as i64 + imm * 4;
                report.push_str(&format!(" targets {target:#x}"));
                write_place(report, spans, target, file_len);
            }
        }
    }

    fn write_kind(&self, report: &mut String) {
        match *self {
            Word::Named(name, _) => report.push_str(name),
            Word::Compare { nonzero, wide, register, .. } => {
                report.push_str(if nonzero { "cbnz " } else { "cbz " });
                write_register(report, wide, register);
            }
            Word::Test { nonzero, bit, register, .. } => {
                report.push_str(if nonzero { "tbnz " } else { "tbz " });
                write_register(report, bit >= 32, register);
                report.push_str(" bit ");
                report.push_str(&bit.to_string());
            }
            Word::Register { kind, register } => {
                report.push_str(kind);
                report.push(' ');
                write_register(report, true, register);
            }
            Word::Authenticated { name, register, modifier } => {
                report.push_str(name);
                report.push(' ');
                write_register(report, true, register);
                match modifier {
                    None => {}
                    Some(31) => report.push_str(" sp modifier"),
                    Some(modifier) => {
                        report.push(' ');
                        write_register(report, true, modifier);
                        report.push_str(" modifier");
                    }
                }
            }
        }
    }
}

fn write_register(report: &mut String, wide: bool, register: u8) {
    report.push(if wide { 'x' } else { 'w' });
    if register == 31 {
        report.push_str("zr");
    } else {
        report.push_str(&register.to_string());
    }
}

fn decode_word(word: u32, pauth: bool) -> Option<Word> {
    if let Some(name) = branch_kind(word) {
        let imm = if conditional_kind(word).is_some() {
            signed((word >> 5) & 0x7_ffff, 19)
        } else {
            signed(word & 0x03ff_ffff, 26)
        };
        return Some(Word::Named(name, imm));
    }
    if word & 0x7e00_0000 == 0x3400_0000 {
        return Some(Word::Compare {
            nonzero: word & (1 << 24) != 0,
            wide: word >> 31 != 0,
            register: (word & 31) as u8,
            imm: signed((word >> 5) & 0x7_ffff, 19),
        });
    }
    if word & 0x7e00_0000 == 0x3600_0000 {
        return Some(Word::Test {
            nonzero: word & (1 << 24) != 0,
            bit: (((word >> 26) & 32) | ((word >> 19) & 31)) as u8,
            register: (word & 31) as u8,
            imm: signed((word >> 5) & 0x3fff, 14),
        });
    }
    if pauth {
        if matches!(word & 0xfeff_f800, 0xd61f_0800 | 0xd63f_0800) || word & 0xffff_fbff == 0xd65f_0bff {
            return authenticated(word);
        }
    }
    match word & 0xffff_fc1f {
        0xd61f_0000 => Some(Word::Register { kind: "br", register: ((word >> 5) & 31) as u8 }),
        0xd63f_0000 => Some(Word::Register { kind: "blr", register: ((word >> 5) & 31) as u8 }),
        0xd65f_0000 => Some(Word::Register { kind: "ret", register: ((word >> 5) & 31) as u8 }),
        _ => None,
    }
}

fn authenticated(word: u32) -> Option<Word> {
    if let Some(kind) = match word & 0xfeff_f800 {
        0xd61f_0800 => Some(0u8),
        0xd63f_0800 => Some(1),
        _ => None,
    } {
        let register = ((word >> 5) & 31) as u8;
        let modifier_reg = (word & 31) as u8;
        let nonzero = word & (1 << 24) != 0;
        if !nonzero && modifier_reg != 31 {
            return None;
        }
        let modifier = if nonzero { Some(modifier_reg) } else { None };
        let key_b = word & (1 << 10) != 0;
        return Some(Word::Authenticated {
            name: auth_name(kind, key_b, modifier.is_some()),
            register,
            modifier,
        });
    }
    if word & 0xffff_fbff == 0xd65f_0bff {
        let key_b = word & (1 << 10) != 0;
        return Some(Word::Authenticated {
            name: if key_b { "retab" } else { "retaa" },
            register: 30,
            modifier: Some(31),
        });
    }
    None
}

fn auth_name(kind: u8, key_b: bool, modifier: bool) -> &'static str {
    match (kind, key_b, modifier) {
        (0, false, true) => "braa",
        (0, true, true) => "brab",
        (0, false, false) => "braaz",
        (0, true, false) => "brabz",
        (1, false, true) => "blraa",
        (1, true, true) => "blrab",
        (1, false, false) => "blraaz",
        (1, true, false) => "blrabz",
        _ => "unclassified",
    }
}

fn pauth_declared(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 {
        return false;
    }
    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap_or([0; 8]));
    let phentsize = u16::from_le_bytes([bytes[54], bytes[55]]) as usize;
    let phnum = u16::from_le_bytes([bytes[56], bytes[57]]) as usize;
    if phoff != 0 && phentsize >= 56 && phnum > 0 && phnum <= 64 {
        if let Some(table) = usize::try_from(phoff).ok() {
            for index in 0..phnum.min(16) {
                let Some(at) = table.checked_add(index.saturating_mul(phentsize)) else {
                    break;
                };
                let Some(header) = bytes.get(at..at.saturating_add(56)) else {
                    break;
                };
                if u32::from_le_bytes(header[0..4].try_into().unwrap_or([0; 4])) != 4 {
                    continue;
                }
                let Ok(offset) = usize::try_from(u64::from_le_bytes(header[8..16].try_into().unwrap_or([0; 8]))) else {
                    continue;
                };
                let Ok(size) = usize::try_from(u64::from_le_bytes(header[32..40].try_into().unwrap_or([0; 8]))) else {
                    continue;
                };
                if range_has_pac(bytes, offset, size) {
                    return true;
                }
            }
        }
    }
    let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap_or([0; 8]));
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]) as usize;
    let shnum = u16::from_le_bytes([bytes[60], bytes[61]]) as usize;
    if shoff == 0 || entsize < 64 || shnum == 0 || shnum > 64 {
        return false;
    }
    let Some(table) = usize::try_from(shoff).ok() else {
        return false;
    };
    for index in 0..shnum.min(16) {
        let Some(at) = table.checked_add(index.saturating_mul(entsize)) else {
            break;
        };
        let Some(header) = bytes.get(at..at.saturating_add(64)) else {
            break;
        };
        if u32::from_le_bytes(header[4..8].try_into().unwrap_or([0; 4])) != 7 {
            continue;
        }
        let Ok(offset) = usize::try_from(u64::from_le_bytes(header[24..32].try_into().unwrap_or([0; 8]))) else {
            continue;
        };
        let Ok(size) = usize::try_from(u64::from_le_bytes(header[32..40].try_into().unwrap_or([0; 8]))) else {
            continue;
        };
        if range_has_pac(bytes, offset, size) {
            return true;
        }
    }
    false
}

fn range_has_pac(bytes: &[u8], start: usize, size: usize) -> bool {
    if size == 0 || start >= bytes.len() {
        return false;
    }
    let end = start.saturating_add(size).min(bytes.len()).min(start.saturating_add(4096));
    let mut at = start;
    let mut notes = 0usize;
    while notes < 8 && at + 12 <= end {
        let namesz = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or([0; 4])) as usize;
        let descsz = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap_or([0; 4])) as usize;
        let kind = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap_or([0; 4]));
        if namesz > 64 || descsz > 1024 {
            return false;
        }
        let name_at = at + 12;
        let desc_at = at + align8(name_at.saturating_add(namesz) - at);
        let desc_end = desc_at.saturating_add(descsz);
        let next = at + align8(desc_end - at);
        if next <= at || desc_end > end {
            return false;
        }
        if kind == 5 && namesz >= 4 && bytes.get(name_at..name_at + 4) == Some(b"GNU\0") && properties_have_pac(bytes, desc_at, descsz) {
            return true;
        }
        at = next;
        notes += 1;
    }
    false
}

fn properties_have_pac(bytes: &[u8], start: usize, size: usize) -> bool {
    let end = start.saturating_add(size).min(bytes.len());
    let mut at = start;
    let mut props = 0usize;
    while props < 8 && at + 8 <= end {
        let kind = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or([0; 4]));
        let data_size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap_or([0; 4]));
        if data_size > 64 {
            return false;
        }
        let data = at + 8;
        if kind == 0xc000_0000 && data_size >= 4 && data + 4 <= end {
            let feature = u32::from_le_bytes(bytes[data..data + 4].try_into().unwrap_or([0; 4]));
            if feature & 2 != 0 {
                return true;
            }
        }
        let next = start + align8(data.saturating_add(data_size as usize) - start);
        if next <= at {
            return false;
        }
        at = next;
        props += 1;
    }
    false
}

fn align8(value: usize) -> usize {
    value.saturating_add(7) & !7
}

fn pe_branch(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(bytes.get(0x3c..0x40)?.try_into().ok()?) as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return None;
    }
    let machine = u16::from_le_bytes(bytes.get(pe + 4..pe + 6)?.try_into().ok()?);
    if machine != 0xAA64 {
        if matches!(machine, 0x01C0 | 0x01C2 | 0x01C4) {
            return Some("not an AArch64 image\nThe branch was not executed.".to_string());
        }
        return None;
    }
    let (ranges, spans, unread) = pe_parts(bytes, pe);
    if ranges.is_empty() {
        return Some(finish_pe("no executable section was located".to_string(), unread));
    }
    let header_end = pe_header_end(bytes, pe);
    if ranges.iter().all(|&(_, to)| to <= header_end) {
        return Some(finish_pe("executable section did not extend past the headers".to_string(), unread));
    }
    let mut report = String::new();
    let mut listed = 0usize;
    let mut examined = 0usize;
    let mut more = false;
    for (index, (from, to)) in ranges.iter().enumerate() {
        let mut off = (*from).max(header_end);
        if off % 4 != 0 {
            off += 4 - (off % 4);
        }
        while off + 4 <= *to && listed < INSTRUCTION_LIMIT && examined < BYTE_CAP {
            note_branch(bytes, off, &spans, false, &mut report, &mut listed);
            off += 4;
            examined += 4;
        }
        if listed >= INSTRUCTION_LIMIT || examined >= BYTE_CAP {
            if off + 4 <= *to || index + 1 < ranges.len() {
                more = true;
            }
            break;
        }
    }
    if listed == 0 {
        report.push_str("no aligned branch was decoded");
    }
    if more {
        if listed > 0 {
            report.push('\n');
        }
        report.push_str("later branches were not listed");
    }
    Some(finish_pe(report, unread))
}

fn finish_pe(mut report: String, unread: bool) -> String {
    if unread {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str("later sections were not listed");
    }
    report.push('\n');
    report.push_str("The branch was not executed.");
    report
}

fn pe_header_end(bytes: &[u8], pe: usize) -> usize {
    let count = bytes.get(pe + 6..pe + 8).and_then(|raw| raw.try_into().ok()).map(u16::from_le_bytes).unwrap_or(0) as usize;
    let optional = bytes.get(pe + 20..pe + 22).and_then(|raw| raw.try_into().ok()).map(u16::from_le_bytes).unwrap_or(0) as usize;
    let Some(table) = pe.checked_add(24).and_then(|base| base.checked_add(optional)) else {
        return bytes.len();
    };
    table.saturating_add(count.min(96).saturating_mul(40)).min(bytes.len())
}

fn pe_parts(bytes: &[u8], pe: usize) -> (Vec<(usize, usize)>, Vec<Span>, bool) {
    let mut ranges = Vec::new();
    let mut spans = Vec::new();
    let Some(count_bytes) = bytes.get(pe + 6..pe + 8) else {
        return (ranges, spans, false);
    };
    let Some(opt_bytes) = bytes.get(pe + 20..pe + 22) else {
        return (ranges, spans, false);
    };
    let count = u16::from_le_bytes(count_bytes.try_into().unwrap_or([0; 2])) as usize;
    let optional = u16::from_le_bytes(opt_bytes.try_into().unwrap_or([0; 2])) as usize;
    let Some(table) = pe.checked_add(24).and_then(|base| base.checked_add(optional)) else {
        return (ranges, spans, false);
    };
    let limit = count.min(96);
    for index in 0..limit {
        if spans.len() >= 32 && ranges.len() >= 32 {
            return (ranges, spans, true);
        }
        let Some(at) = table.checked_add(index.saturating_mul(40)) else {
            return (ranges, spans, true);
        };
        let Some(header) = bytes.get(at..at.saturating_add(40)) else {
            return (ranges, spans, true);
        };
        let raw_size = u32::from_le_bytes(header[16..20].try_into().unwrap_or([0; 4])) as usize;
        let raw_ptr = u32::from_le_bytes(header[20..24].try_into().unwrap_or([0; 4])) as usize;
        let flags = u32::from_le_bytes(header[36..40].try_into().unwrap_or([0; 4]));
        if raw_size == 0 || raw_ptr >= bytes.len() {
            continue;
        }
        let end = raw_ptr.saturating_add(raw_size).min(bytes.len());
        if end <= raw_ptr {
            continue;
        }
        let name = pe_name(header);
        if !name.is_empty() && spans.len() < 32 {
            spans.push(Span { name, start: raw_ptr, end });
        }
        if flags & 0x2000_0000 != 0 && ranges.len() < 32 {
            ranges.push((raw_ptr, end));
        }
    }
    (ranges, spans, count > 96)
}

fn pe_name(header: &[u8]) -> String {
    let mut name = String::new();
    for byte in header.iter().copied().take(8) {
        if byte == 0 {
            break;
        }
        if !(0x20..=0x7e).contains(&byte) {
            return String::new();
        }
        name.push(byte as char);
    }
    name
}

fn mach_branch(bytes: &[u8]) -> Option<String> {
    let le64 = bytes.get(0..4) == Some(&[0xCF, 0xFA, 0xED, 0xFE]);
    let other = matches!(
        bytes.get(0..4),
        Some(&[0xCE, 0xFA, 0xED, 0xFE] | &[0xFE, 0xED, 0xFA, 0xCF] | &[0xFE, 0xED, 0xFA, 0xCE])
    );
    if !le64 && !other {
        return None;
    }
    let arm64 = le64 && bytes.len() >= 8 && u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])) == 0x0100_000c;
    if !arm64 {
        return Some("not an ARM64 image\nThe branch was not executed.".to_string());
    }
    if bytes.len() < 32 {
        return Some("not an ARM64 image\nThe branch was not executed.".to_string());
    }
    let (ranges, spans, start, missed, unread, unread_sections) = mach_parts(bytes);
    if missed {
        let mut text = String::from("instruction section did not overlap an executable segment");
        if unread {
            text.push_str("\nlater load commands were not read");
        }
        if unread_sections {
            text.push_str("\nlater segment sections were not read");
        }
        text.push_str("\nThe branch was not executed.");
        return Some(text);
    }
    let mut report = String::new();
    let mut listed = 0usize;
    let mut examined = 0usize;
    let mut more = false;
    for (index, (from, to)) in ranges.iter().enumerate() {
        let mut off = (*from).max(start);
        if off % 4 != 0 {
            off += 4 - (off % 4);
        }
        while off + 4 <= *to && listed < INSTRUCTION_LIMIT && examined < BYTE_CAP {
            note_branch(bytes, off, &spans, false, &mut report, &mut listed);
            off += 4;
            examined += 4;
        }
        if listed >= INSTRUCTION_LIMIT || examined >= BYTE_CAP {
            if off + 4 <= *to || index + 1 < ranges.len() {
                more = true;
            }
            break;
        }
    }
    if listed == 0 {
        report.push_str("no aligned branch was decoded");
    }
    if more {
        if listed > 0 {
            report.push('\n');
        }
        report.push_str("later branches were not listed");
    }
    if unread {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str("later load commands were not read");
    }
    if unread_sections {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str("later segment sections were not read");
    }
    report.push('\n');
    report.push_str("The branch was not executed.");
    Some(report)
}

fn mach_parts(bytes: &[u8]) -> (Vec<(usize, usize)>, Vec<Span>, usize, bool, bool, bool) {
    let commands = u32::from_le_bytes(bytes.get(16..20).unwrap_or(&[0; 4]).try_into().unwrap_or([0; 4])) as usize;
    let size = u32::from_le_bytes(bytes.get(20..24).unwrap_or(&[0; 4]).try_into().unwrap_or([0; 4])) as usize;
    let commands_end = 32usize.saturating_add(size).min(bytes.len());
    let mut offset = 32usize;
    let mut segments = Vec::new();
    let mut code = Vec::new();
    let mut spans = Vec::new();
    let mut saw_instruction = false;
    let mut read = 0usize;
    let mut unread_sections = false;
    for _ in 0..commands.min(64) {
        if offset.saturating_add(8) > commands_end {
            break;
        }
        let cmd = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap_or([0; 4]));
        let cmdsize = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap_or([0; 4])) as usize;
        if cmdsize < 8 {
            break;
        }
        let Some(next) = offset.checked_add(cmdsize) else {
            break;
        };
        if next > commands_end {
            break;
        }
        if cmd == 0x19 && cmdsize >= 72 {
            let file_off = u64::from_le_bytes(bytes[offset + 40..offset + 48].try_into().unwrap_or([0; 8]));
            let filesz = u64::from_le_bytes(bytes[offset + 48..offset + 56].try_into().unwrap_or([0; 8]));
            let prot = u32::from_le_bytes(bytes[offset + 60..offset + 64].try_into().unwrap_or([0; 4]));
            let seg_start = usize::try_from(file_off).ok().filter(|start| *start < bytes.len());
            let seg_end = seg_start.and_then(|start| {
                let span = usize::try_from(filesz).ok()?;
                let end = start.saturating_add(span).min(bytes.len());
                (span > 0 && end > start).then_some(end)
            });
            if prot & 4 != 0 {
                if let (Some(start), Some(end)) = (seg_start, seg_end) {
                    segments.push((start, end));
                }
            }
            let count = u32::from_le_bytes(bytes[offset + 64..offset + 68].try_into().unwrap_or([0; 4])) as usize;
            if count > 16 {
                unread_sections = true;
            }
            for index in 0..count.min(16) {
                if spans.len() >= 32 && code.len() >= 32 {
                    unread_sections = true;
                    break;
                }
                let Some(at) = offset.checked_add(72).and_then(|base| base.checked_add(index.saturating_mul(80))) else {
                    unread_sections = true;
                    break;
                };
                if at.saturating_add(68) > next {
                    unread_sections = true;
                    break;
                }
                let sec_size = u64::from_le_bytes(bytes[at + 40..at + 48].try_into().unwrap_or([0; 8]));
                let file_off = u32::from_le_bytes(bytes[at + 48..at + 52].try_into().unwrap_or([0; 4])) as usize;
                let flags = u32::from_le_bytes(bytes[at + 64..at + 68].try_into().unwrap_or([0; 4]));
                let Ok(span) = usize::try_from(sec_size) else {
                    continue;
                };
                if span == 0 || file_off >= bytes.len() {
                    continue;
                }
                let end = file_off.saturating_add(span).min(bytes.len());
                if end <= file_off {
                    continue;
                }
                let name = mach_name(bytes, at);
                if !name.is_empty() && spans.len() < 32 {
                    spans.push(Span { name, start: file_off, end });
                }
                if prot & 4 != 0 && flags & 0x8000_0400 != 0 {
                    saw_instruction = true;
                    if code.len() < 32 {
                        if let (Some(seg_from), Some(seg_to)) = (seg_start, seg_end) {
                            let from = file_off.max(seg_from);
                            let to = end.min(seg_to);
                            if from < to {
                                code.push((from, to));
                            }
                        }
                    }
                }
            }
        }
        offset = next;
        read += 1;
    }
    let unread = read < commands;
    if saw_instruction && code.is_empty() {
        return (Vec::new(), spans, commands_end, true, unread, unread_sections);
    }
    if unread_sections && code.is_empty() {
        return (Vec::new(), spans, commands_end, false, unread, true);
    }
    let ranges = if code.is_empty() { segments } else { code };
    (ranges, spans, commands_end, false, unread, unread_sections)
}

fn mach_name(bytes: &[u8], at: usize) -> String {
    let Some(raw) = bytes.get(at..at + 16) else {
        return String::new();
    };
    let mut name = String::new();
    for byte in raw.iter().copied() {
        if byte == 0 {
            return name;
        }
        if name.len() == 16 || !(0x20..=0x7e).contains(&byte) {
            return String::new();
        }
        name.push(byte as char);
    }
    name
}

fn signed(value: u32, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((value as i64) << shift) >> shift
}

enum Coverage {
    Window,
    Ranges(Vec<(usize, usize)>, bool),
    NoOverlap,
    TooMany,
    Truncated,
}

fn exec_ranges(bytes: &[u8]) -> Coverage {
    if bytes.len() < 64 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 {
        return Coverage::Window;
    }
    let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap_or([0; 8]));
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]) as usize;
    let shnum = u16::from_le_bytes([bytes[60], bytes[61]]) as usize;
    if shnum > 64 && shoff != 0 && entsize >= 64 {
        return Coverage::TooMany;
    }
    if shoff == 0 || entsize < 64 || shnum == 0 || shnum > 64 {
        return Coverage::Window;
    }
    let Some(table) = usize::try_from(shoff).ok() else {
        return Coverage::Window;
    };
    let mut ranges = Vec::new();
    let mut read = 0usize;
    for index in 0..shnum {
        let Some(at) = table.checked_add(index.saturating_mul(entsize)) else {
            break;
        };
        let Some(header) = bytes.get(at..at.saturating_add(64)) else {
            break;
        };
        read += 1;
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
    if ranges.is_empty() {
        return if read < shnum { Coverage::Truncated } else { Coverage::Window };
    }
    let loads = executable_loads(bytes);
    if loads.is_empty() {
        return Coverage::Ranges(ranges, read < shnum);
    }
    let mut clipped = Vec::new();
    for section in ranges {
        for (from, to) in &loads {
            if clipped.len() >= 32 {
                break;
            }
            let start = section.0.max(*from);
            let end = section.1.min(*to);
            if start < end {
                clipped.push((start, end));
            }
        }
    }
    if clipped.is_empty() {
        Coverage::NoOverlap
    } else {
        Coverage::Ranges(clipped, read < shnum)
    }
}

fn executable_loads(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut loads = Vec::new();
    if bytes.len() < 64 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 {
        return loads;
    }
    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap_or([0; 8]));
    let phentsize = u16::from_le_bytes([bytes[54], bytes[55]]) as usize;
    let phnum = u16::from_le_bytes([bytes[56], bytes[57]]) as usize;
    if phoff == 0 || phentsize < 56 || phnum == 0 || phnum > 64 {
        return loads;
    }
    let Some(table) = usize::try_from(phoff).ok() else {
        return loads;
    };
    for index in 0..phnum {
        if loads.len() >= 32 {
            break;
        }
        let Some(at) = table.checked_add(index.saturating_mul(phentsize)) else {
            break;
        };
        let Some(header) = bytes.get(at..at.saturating_add(56)) else {
            break;
        };
        let kind = u32::from_le_bytes(header[0..4].try_into().unwrap_or([0; 4]));
        let flags = u32::from_le_bytes(header[4..8].try_into().unwrap_or([0; 4]));
        if kind != 1 || flags & 1 == 0 {
            continue;
        }
        let Ok(offset) = usize::try_from(u64::from_le_bytes(header[8..16].try_into().unwrap_or([0; 8]))) else {
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
            loads.push((offset, end));
        }
    }
    loads
}

fn section_spans(bytes: &[u8]) -> Vec<Span> {
    let mut spans = Vec::new();
    if bytes.len() < 64 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 {
        return spans;
    }
    let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap_or([0; 8]));
    let entsize = u16::from_le_bytes([bytes[58], bytes[59]]) as usize;
    let shnum = u16::from_le_bytes([bytes[60], bytes[61]]) as usize;
    let shstrndx = u16::from_le_bytes([bytes[62], bytes[63]]) as usize;
    if shoff == 0 || entsize < 64 || shnum == 0 || shnum > 64 || shstrndx == 0 || shstrndx >= shnum {
        return spans;
    }
    let Some(table) = usize::try_from(shoff).ok() else {
        return spans;
    };
    let Some(name_at) = table.checked_add(shstrndx.saturating_mul(entsize)) else {
        return spans;
    };
    let Some(name_header) = bytes.get(name_at..name_at.saturating_add(64)) else {
        return spans;
    };
    let Ok(name_off) = usize::try_from(u64::from_le_bytes(name_header[24..32].try_into().unwrap_or([0; 8]))) else {
        return spans;
    };
    let Ok(name_size) = usize::try_from(u64::from_le_bytes(name_header[32..40].try_into().unwrap_or([0; 8]))) else {
        return spans;
    };
    let Some(strings) = bytes.get(name_off..name_off.saturating_add(name_size).min(bytes.len())) else {
        return spans;
    };
    if strings.is_empty() {
        return spans;
    }
    for index in 0..shnum {
        if spans.len() >= 32 {
            break;
        }
        let Some(at) = table.checked_add(index.saturating_mul(entsize)) else {
            break;
        };
        let Some(header) = bytes.get(at..at.saturating_add(64)) else {
            break;
        };
        let name_index = u32::from_le_bytes(header[0..4].try_into().unwrap_or([0; 4])) as usize;
        let name = ascii_name(strings, name_index);
        if name.is_empty() {
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
            spans.push(Span { name, start: offset, end });
        }
    }
    spans
}

fn ascii_name(strings: &[u8], index: usize) -> String {
    let Some(raw) = strings.get(index..) else {
        return String::new();
    };
    let mut name = String::new();
    for byte in raw.iter().copied() {
        if byte == 0 {
            return name;
        }
        if name.len() == 64 || !(0x20..=0x7e).contains(&byte) {
            return String::new();
        }
        name.push(byte as char);
    }
    String::new()
}

fn code_start(bytes: &[u8]) -> usize {
    if bytes.len() < 5 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 {
        return 0;
    }
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
    if let Some(table) = phentsize.checked_mul(phnum).and_then(|size| phoff.checked_add(size)) {
        if let Ok(table) = usize::try_from(table) {
            end = end.max(table).min(bytes.len());
        }
    }
    end
}

fn branch_kind(word: u32) -> Option<&'static str> {
    match word >> 26 {
        0b000101 => Some("b"),
        0b100101 => Some("bl"),
        _ => conditional_kind(word),
    }
}

fn conditional_kind(word: u32) -> Option<&'static str> {
    const NAMES: [&str; 32] = [
        "b.eq", "b.ne", "b.cs", "b.cc", "b.mi", "b.pl", "b.vs", "b.vc", "b.hi", "b.ls", "b.ge",
        "b.lt", "b.gt", "b.le", "b.al", "b.nv", "bc.eq", "bc.ne", "bc.cs", "bc.cc", "bc.mi",
        "bc.pl", "bc.vs", "bc.vc", "bc.hi", "bc.ls", "bc.ge", "bc.lt", "bc.gt", "bc.le", "bc.al",
        "bc.nv",
    ];
    if word & 0xff00_0010 == 0x5400_0000 {
        Some(NAMES[(word & 15) as usize])
    } else if word & 0xff00_0010 == 0x5400_0010 {
        Some(NAMES[16 + (word & 15) as usize])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::arm_branch;

    #[test]
    fn elf_header_is_not_a_branch() {
        let mut bytes = vec![0u8; 64];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[8..12].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes.extend_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x40 kind bl targets 0x44\nThe branch was not executed."
        );
    }

    #[test]
    fn program_header_is_not_a_branch() {
        let mut bytes = vec![0u8; 124];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[120..124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        let report = arm_branch(&bytes);
        assert_eq!(
            report,
            "branch at 0x78 kind bl targets 0x7c\nThe branch was not executed."
        );
        assert!(!report.contains("branch at 0x40"), "{report}");
    }

    #[test]
    fn executable_section_is_scanned() {
        let mut bytes = vec![0u8; 0x210];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[120..124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x200..0x204].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        let shoff = bytes.len();
        bytes.resize(shoff + 128, 0);
        bytes[shoff + 24..shoff + 32].copy_from_slice(&120u64.to_le_bytes());
        bytes[shoff + 32..shoff + 40].copy_from_slice(&16u64.to_le_bytes());
        let text = shoff + 64;
        bytes[text + 8..text + 16].copy_from_slice(&4u64.to_le_bytes());
        bytes[text + 24..text + 32].copy_from_slice(&0x200u64.to_le_bytes());
        bytes[text + 32..text + 40].copy_from_slice(&16u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&2u16.to_le_bytes());
        let report = arm_branch(&bytes);
        assert_eq!(
            report,
            "branch at 0x200 kind bl targets 0x204\nThe branch was not executed."
        );
        assert!(!report.contains("branch at 0x78"), "{report}");
    }

    #[test]
    fn too_many_sections_are_not_scanned_as_a_window() {
        let mut bytes = vec![0u8; 68];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[40..48].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&65u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "the section table has more than 64 headers\nThe branch was not executed."
        );
    }

    #[test]
    fn truncated_section_table_is_not_a_window() {
        let mut bytes = vec![0u8; 128];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[40..48].copy_from_slice(&64u64.to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&2u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "the section table was truncated\nThe branch was not executed."
        );
    }

    #[test]
    fn too_many_program_headers_are_not_a_window() {
        let mut bytes = vec![0u8; 68];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&65u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "the program header table has more than 64 headers\nThe branch was not executed."
        );
    }

    #[test]
    fn truncated_program_headers_are_not_a_window() {
        let mut bytes = vec![0u8; 132];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
        bytes[128..132].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "the program header table was truncated\nThe branch was not executed."
        );
    }

    #[test]
    fn partial_program_headers_keep_the_load() {
        let mut bytes = vec![0u8; 140];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&1u32.to_le_bytes());
        bytes[96..104].copy_from_slice(&140u64.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "no aligned branch was decoded\nlater program headers were not read\nThe branch was not executed."
        );
    }

    #[test]
    fn partial_section_table_names_the_unread_header() {
        let mut bytes = vec![0u8; 192];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[40..48].copy_from_slice(&128u64.to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&2u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[136..144].copy_from_slice(&4u64.to_le_bytes());
        bytes[152..160].copy_from_slice(&64u64.to_le_bytes());
        bytes[160..168].copy_from_slice(&16u64.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x40 kind bl targets 0x44\nlater section headers were not read\nThe branch was not executed."
        );
    }

    #[test]
    fn executable_section_is_clipped_to_its_load() {
        let mut bytes = vec![0u8; 0x210];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&1u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x200u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&16u64.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x200..0x204].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        let shoff = bytes.len();
        bytes.resize(shoff + 64, 0);
        bytes[shoff + 8..shoff + 16].copy_from_slice(&4u64.to_le_bytes());
        bytes[shoff + 24..shoff + 32].copy_from_slice(&0x120u64.to_le_bytes());
        bytes[shoff + 32..shoff + 40].copy_from_slice(&0xf0u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x200 kind bl targets 0x204\nThe branch was not executed."
        );
    }

    #[test]
    fn executable_section_misses_the_load() {
        let mut bytes = vec![0u8; 0x210];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&1u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x200u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&16u64.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        let shoff = bytes.len();
        bytes.resize(shoff + 64, 0);
        bytes[shoff + 8..shoff + 16].copy_from_slice(&4u64.to_le_bytes());
        bytes[shoff + 24..shoff + 32].copy_from_slice(&0x120u64.to_le_bytes());
        bytes[shoff + 32..shoff + 40].copy_from_slice(&16u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "executable section did not overlap an executable load\nThe branch was not executed."
        );
    }

    #[test]
    fn section_names_the_branch() {
        let bytes = named_text(0x9400_0008);
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x200 section .text kind bl targets 0x220 no section\nThe branch was not executed."
        );
    }

    #[test]
    fn target_outside_the_file() {
        let past = named_text(0x9400_0040);
        assert_eq!(
            arm_branch(&past),
            "branch at 0x200 section .text kind bl targets 0x300 outside\nThe branch was not executed."
        );
        let back = named_text(0x54ff_e000);
        assert_eq!(
            arm_branch(&back),
            "branch at 0x200 section .text kind b.eq targets 0xfffffffffffffe00 outside\nThe branch was not executed."
        );
    }

    fn named_text(word: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x210];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[120..124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x200..0x204].copy_from_slice(&word.to_le_bytes());
        let shoff = bytes.len();
        bytes.resize(shoff + 192, 0);
        bytes[shoff + 24..shoff + 32].copy_from_slice(&120u64.to_le_bytes());
        bytes[shoff + 32..shoff + 40].copy_from_slice(&16u64.to_le_bytes());
        bytes[shoff..shoff + 4].copy_from_slice(&7u32.to_le_bytes());
        let text = shoff + 64;
        bytes[text..text + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[text + 8..text + 16].copy_from_slice(&4u64.to_le_bytes());
        bytes[text + 24..text + 32].copy_from_slice(&0x200u64.to_le_bytes());
        bytes[text + 32..text + 40].copy_from_slice(&16u64.to_le_bytes());
        let strings_at = bytes.len();
        let strings = b"\0.text\0.note\0";
        bytes.extend_from_slice(strings);
        let strtab = shoff + 128;
        bytes[strtab + 24..strtab + 32].copy_from_slice(&(strings_at as u64).to_le_bytes());
        bytes[strtab + 32..strtab + 40].copy_from_slice(&(strings.len() as u64).to_le_bytes());
        bytes[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&3u16.to_le_bytes());
        bytes[62..64].copy_from_slice(&2u16.to_le_bytes());
        bytes
    }

    #[test]
    fn conditional_branch() {
        assert_eq!(
            arm_branch(&0x5400_0020u32.to_le_bytes()),
            "branch at 0x0 kind b.eq targets 0x4\nThe branch was not executed."
        );
        let back = 0x5400_0000u32 | (0x7_ffff << 5) | 1;
        assert_eq!(
            arm_branch(&back.to_le_bytes()),
            "branch at 0x0 kind b.ne targets 0xfffffffffffffffc\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0x5400_0030u32.to_le_bytes()),
            "branch at 0x0 kind bc.eq targets 0x4\nThe branch was not executed."
        );
    }

    #[test]
    fn compare_and_test_branch() {
        assert_eq!(
            arm_branch(&0x3400_0020u32.to_le_bytes()),
            "branch at 0x0 kind cbz w0 targets 0x4\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xb5ff_ffe1u32.to_le_bytes()),
            "branch at 0x0 kind cbnz x1 targets 0xfffffffffffffffc\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0x3400_003fu32.to_le_bytes()),
            "branch at 0x0 kind cbz wzr targets 0x4\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0x3610_0023u32.to_le_bytes()),
            "branch at 0x0 kind tbz w3 bit 2 targets 0x4\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xb700_0020u32.to_le_bytes()),
            "branch at 0x0 kind tbnz x0 bit 32 targets 0x4\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0x3607_ffe0u32.to_le_bytes()),
            "branch at 0x0 kind tbz w0 bit 0 targets 0xfffffffffffffffc\nThe branch was not executed."
        );
    }

    #[test]
    fn register_branch() {
        assert_eq!(
            arm_branch(&0xd61f_0020u32.to_le_bytes()),
            "branch at 0x0 kind br x1 target unresolved\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xd63f_0040u32.to_le_bytes()),
            "branch at 0x0 kind blr x2 target unresolved\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xd65f_03c0u32.to_le_bytes()),
            "branch at 0x0 kind ret x30 target unresolved\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xd65f_03e0u32.to_le_bytes()),
            "branch at 0x0 kind ret xzr target unresolved\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xd61f_0001u32.to_le_bytes()),
            "no aligned branch was decoded\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xd71f_0864u32.to_le_bytes()),
            "no aligned branch was decoded\nThe branch was not executed."
        );
    }

    #[test]
    fn authenticated_branch_requires_declared_pac() {
        assert_eq!(
            arm_branch(&pac_text(0xd71f_0864, 2)),
            "branch at 0x200 section .text kind braa x3 x4 modifier target unresolved authentication was not performed\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&pac_text(0xd71f_0c64, 2)),
            "branch at 0x200 section .text kind brab x3 x4 modifier target unresolved authentication was not performed\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&pac_text(0xd61f_08bf, 2)),
            "branch at 0x200 section .text kind braaz x5 target unresolved authentication was not performed\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&pac_text(0xd65f_0bff, 2)),
            "branch at 0x200 section .text kind retaa x30 sp modifier target unresolved authentication was not performed\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&pac_text(0xd71f_0864, 1)),
            "no aligned branch was decoded\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&pac_text(0xd61f_0800, 2)),
            "no aligned branch was decoded\nThe branch was not executed."
        );
    }

    #[test]
    fn authenticated_branch_from_section_note() {
        assert_eq!(
            arm_branch(&pac_section(0xd71f_0864)),
            "branch at 0x200 section .text kind braa x3 x4 modifier target unresolved authentication was not performed\nThe branch was not executed."
        );
    }

    #[test]
    fn aarch64_pe_section_is_scanned() {
        let mut bytes = vec![0u8; 0x150];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x01C4u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x98..0x9d].copy_from_slice(b".text");
        bytes[0xa8..0xac].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xac..0xb0].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        bytes[0xc0..0xc5].copy_from_slice(b".data");
        bytes[0xd0..0xd4].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xd4..0xd8].copy_from_slice(&0x140u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x140..0x144].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "not an AArch64 image\nThe branch was not executed."
        );
        bytes[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x120 section .text kind bl targets 0x124 section .text\nThe branch was not executed."
        );
        bytes[0xbc..0xc0].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "no executable section was located\nThe branch was not executed."
        );
    }

    #[test]
    fn aarch64_pe_second_section_and_outside_target() {
        let mut bytes = vec![0u8; 0x170];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[8..12].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&3u16.to_le_bytes());
        bytes[0x98..0x9d].copy_from_slice(b".text");
        bytes[0xa8..0xac].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xac..0xb0].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        bytes[0xc0..0xc5].copy_from_slice(b".data");
        bytes[0xd0..0xd4].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xd4..0xd8].copy_from_slice(&0x140u32.to_le_bytes());
        bytes[0xe8..0xed].copy_from_slice(b".stub");
        bytes[0xf8..0xfc].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xfc..0x100].copy_from_slice(&0x160u32.to_le_bytes());
        bytes[0x10c..0x110].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x140..0x144].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x160..0x164].copy_from_slice(&0x9400_0008u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x120 section .text kind bl targets 0x124 section .text\nbranch at 0x160 section .stub kind bl targets 0x180 outside\nThe branch was not executed."
        );
    }

    #[test]
    fn aarch64_pe_branch_cap_is_named() {
        let mut bytes = vec![0u8; 0x120 + 65 * 4];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x98..0x9d].copy_from_slice(b".text");
        bytes[0xa8..0xac].copy_from_slice(&(65u32 * 4).to_le_bytes());
        bytes[0xac..0xb0].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        for index in 0..65 {
            let at = 0x120 + index * 4;
            bytes[at..at + 4].copy_from_slice(&0x1400_0000u32.to_le_bytes());
        }
        let report = arm_branch(&bytes);
        assert_eq!(report.matches("branch at ").count(), 64);
        assert!(report.contains("branch at 0x21c section .text kind b targets 0x21c section .text"));
        assert!(!report.contains("branch at 0x220"));
        assert!(report.contains("later branches were not listed\nThe branch was not executed."));
    }

    #[test]
    fn aarch64_pe_names_the_target_section() {
        let mut bytes = vec![0u8; 0x150];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x98..0x9d].copy_from_slice(b".text");
        bytes[0xa8..0xac].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xac..0xb0].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        bytes[0xc0..0xc5].copy_from_slice(b".data");
        bytes[0xd0..0xd4].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xd4..0xd8].copy_from_slice(&0x140u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0008u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x120 section .text kind bl targets 0x140 section .data\nThe branch was not executed."
        );
    }

    #[test]
    fn aarch64_pe_header_is_not_a_branch() {
        let mut bytes = vec![0u8; 0xd0];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[8..12].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x98..0x9d].copy_from_slice(b".text");
        bytes[0xa8..0xac].copy_from_slice(&(0xd0u32).to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        bytes[0xc0..0xc4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0xc0 section .text kind bl targets 0xc4 section .text\nThe branch was not executed."
        );
        bytes[0xa8..0xac].copy_from_slice(&16u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "executable section did not extend past the headers\nThe branch was not executed."
        );
    }

    #[test]
    fn aarch64_pe_short_section_table_is_named() {
        let mut bytes = vec![0u8; 0xd0];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x98..0x9d].copy_from_slice(b".text");
        bytes[0xa8..0xac].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xac..0xb0].copy_from_slice(&0xc0u32.to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        bytes[0xc0..0xc4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "executable section did not extend past the headers\nlater sections were not listed\nThe branch was not executed."
        );
    }

    #[test]
    fn aarch64_pe_names_unread_sections() {
        let mut bytes = vec![0u8; 0x1030];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&97u16.to_le_bytes());
        bytes[0x98..0x9d].copy_from_slice(b".text");
        bytes[0xa8..0xac].copy_from_slice(&16u32.to_le_bytes());
        bytes[0xac..0xb0].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        let unread = 0x98 + 96 * 40;
        bytes[unread..unread + 5].copy_from_slice(b".late");
        bytes[unread + 16..unread + 20].copy_from_slice(&16u32.to_le_bytes());
        bytes[unread + 20..unread + 24].copy_from_slice(&0x1020u32.to_le_bytes());
        bytes[unread + 36..unread + 40].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        bytes[0x1000..0x1004].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x1020..0x1024].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x1000 section .text kind bl targets 0x1004 section .text\nlater sections were not listed\nThe branch was not executed."
        );
    }

    #[test]
    fn other_mach_o_is_not_scanned_as_arm() {
        for magic in [
            [0xCE, 0xFA, 0xED, 0xFE],
            [0xFE, 0xED, 0xFA, 0xCF],
            [0xFE, 0xED, 0xFA, 0xCE],
        ] {
            let mut bytes = vec![0u8; 36];
            bytes[0..4].copy_from_slice(&magic);
            bytes[32..36].copy_from_slice(&0x9400_0001u32.to_le_bytes());
            assert_eq!(
                arm_branch(&bytes),
                "not an ARM64 image\nThe branch was not executed."
            );
        }
        let mut short = vec![0xCF, 0xFA, 0xED, 0xFE];
        short.extend_from_slice(&0x0100_000cu32.to_le_bytes());
        short.extend_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&short),
            "not an ARM64 image\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_section_is_clipped_to_its_segment() {
        let mut bytes = vec![0u8; 0x190];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&152u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&152u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x180u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&16u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[104..110].copy_from_slice(b"__text");
        bytes[144..152].copy_from_slice(&0x70u64.to_le_bytes());
        bytes[152..156].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[168..172].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x180..0x184].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x180 section __text kind bl targets 0x184 section __text\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_section_misses_its_segment() {
        let mut bytes = vec![0u8; 0x190];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&152u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&152u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x180u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&16u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[104..110].copy_from_slice(b"__text");
        bytes[144..152].copy_from_slice(&16u64.to_le_bytes());
        bytes[152..156].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[168..172].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x180..0x184].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "instruction section did not overlap an executable segment\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_instruction_section_is_scanned() {
        let mut bytes = vec![0u8; 0x190];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_0007u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "not an ARM64 image\nThe branch was not executed."
        );
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[8..12].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&232u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&232u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&0x90u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&2u32.to_le_bytes());
        bytes[104..110].copy_from_slice(b"__text");
        bytes[144..152].copy_from_slice(&16u64.to_le_bytes());
        bytes[152..156].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[168..172].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[184..190].copy_from_slice(b"__data");
        bytes[224..232].copy_from_slice(&16u64.to_le_bytes());
        bytes[232..236].copy_from_slice(&0x180u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x180..0x184].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x120 section __text kind bl targets 0x124 section __text\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_executable_segment_is_scanned() {
        let mut bytes = vec![0u8; 0xf0];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[8..12].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&144u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&72u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0xc0u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&16u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[104..108].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[108..112].copy_from_slice(&72u32.to_le_bytes());
        bytes[144..152].copy_from_slice(&0xe0u64.to_le_bytes());
        bytes[152..160].copy_from_slice(&16u64.to_le_bytes());
        bytes[164..168].copy_from_slice(&1u32.to_le_bytes());
        bytes[0xc0..0xc4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0xe0..0xe4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0xc0 kind bl targets 0xc4\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_names_an_unread_load_command() {
        let mut bytes = vec![0u8; 0xa0];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&72u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&72u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x90u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&16u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x90..0x94].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x90 kind bl targets 0x94\nlater load commands were not read\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_command_cap_is_named() {
        let count = 65usize;
        let sizeofcmds = 64 * 8 + 72;
        let code_at = 32 + sizeofcmds;
        let mut bytes = vec![0u8; code_at + 16];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&(count as u32).to_le_bytes());
        bytes[20..24].copy_from_slice(&(sizeofcmds as u32).to_le_bytes());
        for index in 0..64 {
            let at = 32 + index * 8;
            bytes[at + 4..at + 8].copy_from_slice(&8u32.to_le_bytes());
        }
        let segment = 32 + 64 * 8;
        bytes[segment..segment + 4].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[segment + 4..segment + 8].copy_from_slice(&72u32.to_le_bytes());
        bytes[segment + 40..segment + 48].copy_from_slice(&(code_at as u64).to_le_bytes());
        bytes[segment + 48..segment + 56].copy_from_slice(&16u64.to_le_bytes());
        bytes[segment + 60..segment + 64].copy_from_slice(&4u32.to_le_bytes());
        bytes[code_at..code_at + 4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "no aligned branch was decoded\nlater load commands were not read\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_section_cap_is_named() {
        let section_count = 17usize;
        let command = 72 + section_count * 80;
        let code_at = 32 + command;
        let mut bytes = vec![0u8; code_at + 16];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&(command as u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&(command as u32).to_le_bytes());
        bytes[72..80].copy_from_slice(&(code_at as u64).to_le_bytes());
        bytes[80..88].copy_from_slice(&16u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&(section_count as u32).to_le_bytes());
        let section = 32 + 72 + 16 * 80;
        bytes[section..section + 6].copy_from_slice(b"__text");
        bytes[section + 40..section + 48].copy_from_slice(&16u64.to_le_bytes());
        bytes[section + 48..section + 52].copy_from_slice(&(code_at as u32).to_le_bytes());
        bytes[section + 64..section + 68].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[code_at..code_at + 4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "no aligned branch was decoded\nlater segment sections were not read\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_short_section_list_is_named() {
        let mut bytes = vec![0u8; 0xd0];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&152u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&152u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0xc0u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&16u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&2u32.to_le_bytes());
        bytes[0xc0..0xc4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "no aligned branch was decoded\nlater segment sections were not read\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_keeps_a_section_when_the_next_does_not_fit() {
        let mut bytes = vec![0u8; 0xe0];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&152u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&152u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0xc0u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&16u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&2u32.to_le_bytes());
        bytes[104..110].copy_from_slice(b"__text");
        bytes[144..152].copy_from_slice(&16u64.to_le_bytes());
        bytes[152..156].copy_from_slice(&0xc0u32.to_le_bytes());
        bytes[168..172].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[0xc0..0xc4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0xc0 section __text kind bl targets 0xc4 section __text\nlater segment sections were not read\nThe branch was not executed."
        );
    }

    #[test]
    fn mach_o_keeps_the_first_section_past_the_cap() {
        let section_count = 17usize;
        let command = 72 + section_count * 80;
        let code_at = 32 + command;
        let mut bytes = vec![0u8; code_at + 32];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&(command as u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&(command as u32).to_le_bytes());
        bytes[72..80].copy_from_slice(&(code_at as u64).to_le_bytes());
        bytes[80..88].copy_from_slice(&32u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&(section_count as u32).to_le_bytes());
        let first = 32 + 72;
        bytes[first..first + 6].copy_from_slice(b"__text");
        bytes[first + 40..first + 48].copy_from_slice(&16u64.to_le_bytes());
        bytes[first + 48..first + 52].copy_from_slice(&(code_at as u32).to_le_bytes());
        bytes[first + 64..first + 68].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        let last = 32 + 72 + 16 * 80;
        bytes[last + 40..last + 48].copy_from_slice(&16u64.to_le_bytes());
        bytes[last + 48..last + 52].copy_from_slice(&((code_at + 16) as u32).to_le_bytes());
        bytes[last + 64..last + 68].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[code_at..code_at + 4].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[code_at + 16..code_at + 20].copy_from_slice(&0x1400_0000u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            format!("branch at {code_at:#x} section __text kind bl targets {next:#x} section __text\nlater segment sections were not read\nThe branch was not executed.", next = code_at + 4)
        );
    }

    #[test]
    fn mach_o_branch_cap_is_named() {
        let mut bytes = vec![0u8; 0x100 + 65 * 4];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&152u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&152u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&(65u64 * 4).to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[104..110].copy_from_slice(b"__text");
        bytes[144..152].copy_from_slice(&(65u64 * 4).to_le_bytes());
        bytes[152..156].copy_from_slice(&0x100u32.to_le_bytes());
        bytes[168..172].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        for index in 0..65 {
            let at = 0x100 + index * 4;
            bytes[at..at + 4].copy_from_slice(&0x1400_0000u32.to_le_bytes());
        }
        let report = arm_branch(&bytes);
        assert_eq!(report.matches("branch at ").count(), 64);
        assert!(report.contains("branch at 0x1fc section __text kind b targets 0x1fc section __text"));
        assert!(!report.contains("branch at 0x200"));
        assert!(report.contains("later branches were not listed\nThe branch was not executed."));
    }

    #[test]
    fn mach_o_second_section_and_outside_target() {
        let mut bytes = vec![0u8; 0x190];
        bytes[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&232u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&232u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0x120u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&0x70u64.to_le_bytes());
        bytes[92..96].copy_from_slice(&4u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&2u32.to_le_bytes());
        bytes[104..110].copy_from_slice(b"__text");
        bytes[144..152].copy_from_slice(&16u64.to_le_bytes());
        bytes[152..156].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[168..172].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[184..190].copy_from_slice(b"__stub");
        bytes[224..232].copy_from_slice(&16u64.to_le_bytes());
        bytes[232..236].copy_from_slice(&0x180u32.to_le_bytes());
        bytes[248..252].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x160..0x164].copy_from_slice(&0x9400_0001u32.to_le_bytes());
        bytes[0x180..0x184].copy_from_slice(&0x9400_0008u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x120 section __text kind bl targets 0x124 section __text\nbranch at 0x180 section __stub kind bl targets 0x1a0 outside\nThe branch was not executed."
        );
    }

    fn gnu_pac_note(feature: u32) -> Vec<u8> {
        let mut note = Vec::new();
        note.extend_from_slice(&4u32.to_le_bytes());
        note.extend_from_slice(&12u32.to_le_bytes());
        note.extend_from_slice(&5u32.to_le_bytes());
        note.extend_from_slice(b"GNU\0");
        note.extend_from_slice(&0xc000_0000u32.to_le_bytes());
        note.extend_from_slice(&4u32.to_le_bytes());
        note.extend_from_slice(&feature.to_le_bytes());
        note
    }

    fn pac_text(word: u32, feature: u32) -> Vec<u8> {
        let mut bytes = named_text(word);
        let note = gnu_pac_note(feature);
        let note_at = bytes.len();
        bytes.extend_from_slice(&note);
        bytes[64..68].copy_from_slice(&4u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&(note_at as u64).to_le_bytes());
        bytes[96..104].copy_from_slice(&(note.len() as u64).to_le_bytes());
        bytes
    }

    fn pac_section(word: u32) -> Vec<u8> {
        let bytes = named_text(word);
        let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
        let insert = shoff + 192;
        let mut grown = bytes[..insert].to_vec();
        grown.extend(std::iter::repeat(0).take(64));
        grown.extend_from_slice(&bytes[insert..]);
        let strtab = shoff + 128;
        let old = u64::from_le_bytes(grown[strtab + 24..strtab + 32].try_into().unwrap());
        grown[strtab + 24..strtab + 32].copy_from_slice(&(old + 64).to_le_bytes());
        grown[60..62].copy_from_slice(&4u16.to_le_bytes());
        let note = gnu_pac_note(2);
        let note_at = grown.len();
        grown.extend_from_slice(&note);
        let header = shoff + 192;
        grown[header + 4..header + 8].copy_from_slice(&7u32.to_le_bytes());
        grown[header + 24..header + 32].copy_from_slice(&(note_at as u64).to_le_bytes());
        grown[header + 32..header + 40].copy_from_slice(&(note.len() as u64).to_le_bytes());
        grown
    }

    #[test]
    fn one_bl() {
        let report = arm_branch(&0x9400_0001u32.to_le_bytes());
        assert!(report.contains("bl"));
        assert!(report.contains("0x0"));
        assert!(report.contains("0x4"));
        assert!(report.contains("not executed"));
        assert_eq!(
            report,
            "branch at 0x0 kind bl targets 0x4\nThe branch was not executed."
        );
    }

    #[test]
    fn one_b_and_negative_bl() {
        assert_eq!(
            arm_branch(&0x1400_0001u32.to_le_bytes()),
            "branch at 0x0 kind b targets 0x4\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0x97ff_ffffu32.to_le_bytes()),
            "branch at 0x0 kind bl targets 0xfffffffffffffffc\nThe branch was not executed."
        );
    }

    #[test]
    fn none_and_unaligned_tail() {
        assert_eq!(
            arm_branch(&[]),
            "no aligned branch was decoded\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&[0x94, 0x00, 0x00]),
            "no aligned branch was decoded\nThe branch was not executed."
        );
        assert_eq!(
            arm_branch(&0xd503_201fu32.to_le_bytes()),
            "no aligned branch was decoded\nThe branch was not executed."
        );
        let mut bytes = 0x9400_0001u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0x01, 0x00, 0x00]);
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x0 kind bl targets 0x4\nThe branch was not executed."
        );
    }

    #[test]
    fn later_word_and_read_cap() {
        let mut bytes = 0xd503_201fu32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&0x9400_0001u32.to_le_bytes());
        assert_eq!(
            arm_branch(&bytes),
            "branch at 0x4 kind bl targets 0x8\nThe branch was not executed."
        );
        let mut many = Vec::new();
        for _ in 0..65 {
            many.extend_from_slice(&0x1400_0000u32.to_le_bytes());
        }
        let report = arm_branch(&many);
        assert_eq!(report.matches("branch at ").count(), 64);
        assert!(report.contains("branch at 0xfc kind b targets 0xfc"));
        assert!(!report.contains("0x100"));
        assert!(report.contains("later branches were not listed\nThe branch was not executed."));
    }
}

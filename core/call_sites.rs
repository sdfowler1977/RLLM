//! Relative call, jump, loop, and conditional displacements read from a byte slice.
//! A short conditional jump that follows a printable byte is left alone, because those opcodes overlap letters.
//! A listed instruction is not proof it executes.

const CODE_LIMIT: usize = 4096;
const EXECUTE: u32 = 0x2000_0000;

struct Window {
    name: String,
    start: usize,
    end: usize,
    truncated: bool,
}

struct Found {
    body: String,
    listed: usize,
    more: bool,
}

pub fn call_sites(bytes: &[u8]) -> String {
    if arm_mach(bytes) {
        return "x86 call was not scanned\nThe instruction was not executed.".to_string();
    }
    finish(sites(bytes, 0, bytes.len(), &[]), None)
}

fn arm_pe(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let Some(pe_bytes) = bytes.get(0x3c..0x40).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let pe = u32::from_le_bytes(pe_bytes) as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return false;
    }
    let Some(machine_bytes) = bytes.get(pe + 4..pe + 6).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    matches!(u16::from_le_bytes(machine_bytes), 0x01C0 | 0x01C2 | 0x01C4 | 0xAA64)
}

fn arm_mach(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return false;
    }
    let cpu = if bytes.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) || bytes.starts_with(&[0xCE, 0xFA, 0xED, 0xFE]) {
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
    } else if bytes.starts_with(&[0xFE, 0xED, 0xFA, 0xCF]) || bytes.starts_with(&[0xFE, 0xED, 0xFA, 0xCE]) {
        u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
    } else {
        return false;
    };
    cpu == 12 || cpu == 0x0100_000c || cpu == 0x0200_000c
}

pub fn pe_code_sites(bytes: &[u8]) -> String {
    if arm_pe(bytes) {
        return "x86 call was not scanned\nThe instruction was not executed.".to_string();
    }
    let unread = pe_sections_unread(bytes);
    let Some(window) = executable_window(bytes) else {
        let mut text = String::from("no executable section was located\n");
        if unread {
            text.push_str("later sections were not listed\n");
        }
        text.push_str("The instruction was not executed.");
        return text;
    };
    let place = format!("section {} file {:#x}", window.name, window.start);
    let mut extra = String::new();
    if window.truncated {
        extra.push_str("later bytes of this section were not listed");
    }
    if another_executable(bytes) {
        if !extra.is_empty() {
            extra.push('\n');
        }
        extra.push_str("later executable sections were not listed");
    }
    if unread {
        if !extra.is_empty() {
            extra.push('\n');
        }
        extra.push_str("later sections were not listed");
    }
    let note = if extra.is_empty() { None } else { Some(extra.as_str()) };
    let spans = pe_spans(bytes);
    finish(sites(bytes, window.start, window.end, &spans), Some((place, note)))
}

pub fn elf_code_sites(bytes: &[u8]) -> Option<String> {
    let window = x86_load(bytes)?;
    let unread = elf_program_headers_unread(bytes);
    let unread_sections = elf_sections_unread(bytes);
    let Some(window) = window else {
        let mut text = String::from("no x86 ELF executable load was located\n");
        if unread {
            text.push_str("later program headers were not read\n");
        }
        if unread_sections {
            text.push_str("later section headers were not read\n");
        }
        text.push_str("The instruction was not executed.");
        return Some(text);
    };
    let place = format!("load file {:#x}", window.start);
    let mut extra = String::new();
    if window.truncated {
        extra.push_str("later bytes of this load were not listed");
    }
    if another_load(bytes) {
        if !extra.is_empty() {
            extra.push('\n');
        }
        extra.push_str("later executable loads were not listed");
    }
    if unread {
        if !extra.is_empty() {
            extra.push('\n');
        }
        extra.push_str("later program headers were not read");
    }
    if unread_sections {
        if !extra.is_empty() {
            extra.push('\n');
        }
        extra.push_str("later section headers were not read");
    }
    let note = if extra.is_empty() { None } else { Some(extra.as_str()) };
    let spans = elf_spans(bytes);
    Some(finish(sites(bytes, window.start, window.end, &spans), Some((place, note))))
}

pub fn macho_code_sites(bytes: &[u8]) -> Option<String> {
    let window = x86_mach_segment(bytes)?;
    let unread = macho_commands_unread(bytes);
    let Some(window) = window else {
        let mut text = String::from("no x86 Mach-O executable segment was located\n");
        if unread {
            text.push_str("later load commands were not read\n");
        }
        text.push_str("The instruction was not executed.");
        return Some(text);
    };
    let place = format!("segment {} file {:#x}", window.name, window.start);
    let mut extra = String::new();
    if window.truncated {
        extra.push_str("later bytes of this segment were not listed");
    }
    if another_segment(bytes) {
        if !extra.is_empty() {
            extra.push('\n');
        }
        extra.push_str("later executable segments were not listed");
    }
    if unread {
        if !extra.is_empty() {
            extra.push('\n');
        }
        extra.push_str("later load commands were not read");
    }
    let note = if extra.is_empty() { None } else { Some(extra.as_str()) };
    let spans = macho_spans(bytes);
    Some(finish(sites(bytes, window.start, window.end, &spans), Some((place, note))))
}

fn finish(found: Found, section: Option<(String, Option<&str>)>) -> String {
    let mut report = String::new();
    if let Some((place, truncated)) = section {
        report.push_str(&place);
        report.push('\n');
        if found.listed == 0 {
            report.push_str("no call, jump, or loop sites were decoded");
        } else {
            report.push_str(&found.body);
        }
        if let Some(note) = truncated {
            report.push('\n');
            report.push_str(note);
        }
    } else if found.listed == 0 {
        report.push_str("no call, jump, or loop sites were decoded");
    } else {
        report.push_str(&found.body);
    }
    if found.more {
        report.push('\n');
        report.push_str("later instructions were not listed");
    }
    report.push('\n');
    report.push_str("The instruction was not executed.");
    report
}

struct Span {
    word: &'static str,
    name: String,
    start: usize,
    end: usize,
}

fn sites(bytes: &[u8], start: usize, end: usize, spans: &[Span]) -> Found {
    let end = end.min(bytes.len());
    let mut report = String::new();
    let mut listed = 0usize;
    let mut index = start.min(end);
    while index + 2 <= end && listed < 32 {
        if (bytes[index] == 0xE8 || bytes[index] == 0xE9) && index + 5 <= end {
            let displacement = i32::from_le_bytes([
                bytes[index + 1],
                bytes[index + 2],
                bytes[index + 3],
                bytes[index + 4],
            ]);
            let next_instruction = (index + 5) as i64;
            let target = next_instruction + i64::from(displacement);
            let outside = target < 0 || target > bytes.len() as i64;
            let kind = if bytes[index] == 0xE8 { "call" } else { "jump" };
            record(&mut report, listed, kind, index, target, outside, spans);
            listed += 1;
        } else if bytes[index] == 0xEB {
            let displacement = bytes[index + 1] as i8;
            let next_instruction = (index + 2) as i64;
            let target = next_instruction + i64::from(displacement);
            let outside = target < 0 || target > bytes.len() as i64;
            record(&mut report, listed, "short jump", index, target, outside, spans);
            listed += 1;
        } else if matches!(bytes[index], 0xE0 | 0xE1 | 0xE2) {
            let displacement = bytes[index + 1] as i8;
            let next_instruction = (index + 2) as i64;
            let target = next_instruction + i64::from(displacement);
            let outside = target < 0 || target > bytes.len() as i64;
            let kind = match bytes[index] {
                0xE0 => "loopne",
                0xE1 => "loope",
                _ => "loop",
            };
            record(&mut report, listed, kind, index, target, outside, spans);
            listed += 1;
        } else if bytes[index] == 0x0F && index + 6 <= end {
            if let Some(kind) = near_condition(bytes[index + 1]) {
                let displacement = i32::from_le_bytes([
                    bytes[index + 2],
                    bytes[index + 3],
                    bytes[index + 4],
                    bytes[index + 5],
                ]);
                let next_instruction = (index + 6) as i64;
                let target = next_instruction + i64::from(displacement);
                let outside = target < 0 || target > bytes.len() as i64;
                record(&mut report, listed, kind, index, target, outside, spans);
                listed += 1;
            }
        } else if let Some(kind) = short_condition(bytes[index]) {
            if !follows_printable(bytes, index) {
                let displacement = bytes[index + 1] as i8;
                let next_instruction = (index + 2) as i64;
                let target = next_instruction + i64::from(displacement);
                let outside = target < 0 || target > bytes.len() as i64;
                record(&mut report, listed, kind, index, target, outside, spans);
                listed += 1;
            }
        }
        index += 1;
    }
    Found {
        body: report,
        listed,
        more: listed == 32 && index + 2 <= end,
    }
}

fn record(report: &mut String, listed: usize, kind: &str, index: usize, target: i64, outside: bool, spans: &[Span]) {
    if listed > 0 {
        report.push('\n');
    }
    let place = if outside { " outside" } else { "" };
    let source = spans
        .iter()
        .find(|span| index >= span.start && index < span.end)
        .map(|span| format!(" {} {}", span.word, span.name))
        .unwrap_or_default();
    let section = if outside || spans.is_empty() {
        String::new()
    } else {
        match usize::try_from(target).ok().and_then(|at| spans.iter().find(|span| at >= span.start && at < span.end)) {
            Some(span) => format!(" {} {}", span.word, span.name),
            None => " no section".to_string(),
        }
    };
    report.push_str(&format!("{kind} at {index:#x}{source} targets {target:#x}{place}{section}"));
}

fn executable_window(bytes: &[u8]) -> Option<Window> {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(bytes.get(0x3c..0x40)?.try_into().ok()?) as usize;
    if bytes.get(pe..pe.checked_add(4)?) != Some(b"PE\0\0") {
        return None;
    }
    let coff = pe + 4;
    let count = u16::from_le_bytes(bytes.get(coff + 2..coff + 4)?.try_into().ok()?) as usize;
    let optional = u16::from_le_bytes(bytes.get(coff + 16..coff + 18)?.try_into().ok()?) as usize;
    let mut at = coff.checked_add(20)?.checked_add(optional)?;
    for _ in 0..count.min(96) {
        let header = bytes.get(at..at.checked_add(40)?)?;
        let raw_size = u32::from_le_bytes(header[16..20].try_into().ok()?) as usize;
        let raw = u32::from_le_bytes(header[20..24].try_into().ok()?) as usize;
        let flags = u32::from_le_bytes(header[36..40].try_into().ok()?);
        at = at.checked_add(40)?;
        if flags & EXECUTE == 0 || raw_size == 0 || raw >= bytes.len() {
            continue;
        }
        let available = raw_size.min(bytes.len() - raw);
        let taken = available.min(CODE_LIMIT);
        let end_name = header[..8].iter().position(|byte| *byte == 0).unwrap_or(8);
        let name = if end_name == 0 {
            "(unnamed)".to_string()
        } else {
            String::from_utf8_lossy(&header[..end_name]).into_owned()
        };
        return Some(Window {
            name,
            start: raw,
            end: raw + taken,
            truncated: available > CODE_LIMIT,
        });
    }
    None
}

fn pe_sections_unread(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let Some(pe_bytes) = bytes.get(0x3c..0x40).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let pe = u32::from_le_bytes(pe_bytes) as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return false;
    }
    let coff = pe + 4;
    let Some(count_bytes) = bytes.get(coff + 2..coff + 4).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let Some(optional_bytes) = bytes.get(coff + 16..coff + 18).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let count = u16::from_le_bytes(count_bytes) as usize;
    let optional = u16::from_le_bytes(optional_bytes) as usize;
    if count > 96 {
        return true;
    }
    let Some(mut at) = coff.checked_add(20).and_then(|base| base.checked_add(optional)) else {
        return false;
    };
    for _ in 0..count {
        if bytes.get(at..at.saturating_add(40)).is_none() {
            return true;
        }
        let Some(next) = at.checked_add(40) else {
            return true;
        };
        at = next;
    }
    false
}

fn another_executable(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let Some(pe_bytes) = bytes.get(0x3c..0x40).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let pe = u32::from_le_bytes(pe_bytes) as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return false;
    }
    let coff = pe + 4;
    let Some(count_bytes) = bytes.get(coff + 2..coff + 4).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let Some(optional_bytes) = bytes.get(coff + 16..coff + 18).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let count = u16::from_le_bytes(count_bytes) as usize;
    let optional = u16::from_le_bytes(optional_bytes) as usize;
    let Some(mut at) = coff.checked_add(20).and_then(|base| base.checked_add(optional)) else {
        return false;
    };
    let mut found = 0usize;
    for _ in 0..count.min(96) {
        let Some(header) = bytes.get(at..at.saturating_add(40)) else {
            break;
        };
        let raw_size = u32::from_le_bytes([header[16], header[17], header[18], header[19]]) as usize;
        let raw = u32::from_le_bytes([header[20], header[21], header[22], header[23]]) as usize;
        let flags = u32::from_le_bytes([header[36], header[37], header[38], header[39]]);
        let Some(next) = at.checked_add(40) else {
            break;
        };
        at = next;
        if flags & EXECUTE == 0 || raw_size == 0 || raw >= bytes.len() {
            continue;
        }
        found += 1;
        if found > 1 {
            return true;
        }
    }
    false
}

fn pe_spans(bytes: &[u8]) -> Vec<Span> {
    let mut spans = Vec::new();
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return spans;
    }
    let Some(pe_bytes) = bytes.get(0x3c..0x40) else {
        return spans;
    };
    let Ok(pe_raw) = pe_bytes.try_into() else {
        return spans;
    };
    let pe = u32::from_le_bytes(pe_raw) as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return spans;
    }
    let coff = pe + 4;
    let Some(count_bytes) = bytes.get(coff + 2..coff + 4) else {
        return spans;
    };
    let Some(optional_bytes) = bytes.get(coff + 16..coff + 18) else {
        return spans;
    };
    let count = u16::from_le_bytes(count_bytes.try_into().unwrap()) as usize;
    let optional = u16::from_le_bytes(optional_bytes.try_into().unwrap()) as usize;
    let Some(mut at) = coff.checked_add(20).and_then(|base| base.checked_add(optional)) else {
        return spans;
    };
    for _ in 0..count.min(96) {
        let Some(header) = bytes.get(at..at.saturating_add(40)) else {
            break;
        };
        let raw_size = u32::from_le_bytes([header[16], header[17], header[18], header[19]]) as usize;
        let raw = u32::from_le_bytes([header[20], header[21], header[22], header[23]]) as usize;
        let Some(next) = at.checked_add(40) else {
            break;
        };
        at = next;
        if raw_size == 0 || raw >= bytes.len() {
            continue;
        }
        let end_name = header[..8].iter().position(|byte| *byte == 0).unwrap_or(8);
        let name = if end_name == 0 {
            "(unnamed)".to_string()
        } else {
            String::from_utf8_lossy(&header[..end_name]).into_owned()
        };
        spans.push(Span {
            word: "section",
            name,
            start: raw,
            end: raw + raw_size.min(bytes.len() - raw),
        });
    }
    spans
}

fn x86_load(bytes: &[u8]) -> Option<Option<Window>> {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[5] != 1 {
        return None;
    }
    let class = bytes[4];
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    if class != 1 && class != 2 || (machine != 3 && machine != 62) {
        return None;
    }
    let (phoff, phentsize, phnum, minimum) = if class == 2 {
        if bytes.len() < 64 {
            return Some(None);
        }
        let phoff = u64::from_le_bytes(bytes.get(32..40)?.try_into().ok()?);
        let phentsize = u16::from_le_bytes(bytes.get(54..56)?.try_into().ok()?) as usize;
        let phnum = u16::from_le_bytes(bytes.get(56..58)?.try_into().ok()?) as usize;
        (phoff, phentsize, phnum, 56usize)
    } else {
        if bytes.len() < 52 {
            return Some(None);
        }
        let phoff = u32::from_le_bytes(bytes.get(28..32)?.try_into().ok()?) as u64;
        let phentsize = u16::from_le_bytes(bytes.get(42..44)?.try_into().ok()?) as usize;
        let phnum = u16::from_le_bytes(bytes.get(44..46)?.try_into().ok()?) as usize;
        (phoff, phentsize, phnum, 32usize)
    };
    if phentsize < minimum || phnum > 64 {
        return Some(None);
    }
    for index in 0..phnum {
        let Some(at) = phoff.checked_add(index as u64 * phentsize as u64) else {
            break;
        };
        let Ok(at) = usize::try_from(at) else {
            break;
        };
        let Some(end) = at.checked_add(minimum) else {
            break;
        };
        let Some(header) = bytes.get(at..end) else {
            break;
        };
        let (kind, flags, raw, raw_size) = if class == 2 {
            (
                u32::from_le_bytes(header[0..4].try_into().ok()?),
                u32::from_le_bytes(header[4..8].try_into().ok()?),
                u64::from_le_bytes(header[8..16].try_into().ok()?),
                u64::from_le_bytes(header[32..40].try_into().ok()?),
            )
        } else {
            (
                u32::from_le_bytes(header[0..4].try_into().ok()?),
                u32::from_le_bytes(header[24..28].try_into().ok()?),
                u64::from(u32::from_le_bytes(header[4..8].try_into().ok()?)),
                u64::from(u32::from_le_bytes(header[16..20].try_into().ok()?)),
            )
        };
        if kind != 1 || flags & 1 == 0 || raw_size == 0 {
            continue;
        }
        let raw = usize::try_from(raw).ok()?;
        if raw >= bytes.len() {
            continue;
        }
        let raw_size = usize::try_from(raw_size).unwrap_or(usize::MAX);
        let available = raw_size.min(bytes.len() - raw);
        let taken = available.min(CODE_LIMIT);
        return Some(Some(Window {
            name: String::new(),
            start: raw,
            end: raw + taken,
            truncated: available > CODE_LIMIT,
        }));
    }
    Some(None)
}

fn elf_program_headers_unread(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[5] != 1 {
        return false;
    }
    let class = bytes[4];
    let Some(machine_bytes) = bytes.get(18..20).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let machine = u16::from_le_bytes(machine_bytes);
    if class != 1 && class != 2 || (machine != 3 && machine != 62) {
        return false;
    }
    let (phoff, phentsize, phnum) = if class == 2 {
        if bytes.len() < 64 {
            return false;
        }
        (
            u64::from_le_bytes(bytes[32..40].try_into().unwrap_or([0; 8])),
            u16::from_le_bytes([bytes[54], bytes[55]]) as u64,
            u16::from_le_bytes([bytes[56], bytes[57]]) as u64,
        )
    } else if bytes.len() < 52 {
        return false;
    } else {
        (
            u64::from(u32::from_le_bytes(bytes[28..32].try_into().unwrap_or([0; 4]))),
            u16::from_le_bytes([bytes[42], bytes[43]]) as u64,
            u16::from_le_bytes([bytes[44], bytes[45]]) as u64,
        )
    };
    if phoff == 0 || phentsize == 0 || phnum == 0 {
        return false;
    }
    if phnum > 64 {
        return true;
    }
    let Some(end) = phentsize.checked_mul(phnum).and_then(|size| phoff.checked_add(size)) else {
        return true;
    };
    end > bytes.len() as u64
}

fn another_load(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[5] != 1 {
        return false;
    }
    let class = bytes[4];
    let Some(machine_bytes) = bytes.get(18..20).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let machine = u16::from_le_bytes(machine_bytes);
    if class != 1 && class != 2 || (machine != 3 && machine != 62) {
        return false;
    }
    let (phoff, phentsize, phnum, minimum) = if class == 2 {
        if bytes.len() < 64 {
            return false;
        }
        (
            u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
            u16::from_le_bytes([bytes[54], bytes[55]]) as usize,
            u16::from_le_bytes([bytes[56], bytes[57]]) as usize,
            56usize,
        )
    } else {
        if bytes.len() < 52 {
            return false;
        }
        (
            u64::from(u32::from_le_bytes(bytes[28..32].try_into().unwrap())),
            u16::from_le_bytes([bytes[42], bytes[43]]) as usize,
            u16::from_le_bytes([bytes[44], bytes[45]]) as usize,
            32usize,
        )
    };
    if phentsize < minimum || phnum == 0 {
        return false;
    }
    let mut found = 0usize;
    for index in 0..phnum.min(64) {
        let Some(at) = phoff.checked_add(index as u64 * phentsize as u64).and_then(|at| usize::try_from(at).ok()) else {
            break;
        };
        let Some(header) = bytes.get(at..at.saturating_add(minimum)) else {
            break;
        };
        let (kind, flags, raw, raw_size) = if class == 2 {
            (
                u32::from_le_bytes(header[0..4].try_into().unwrap()),
                u32::from_le_bytes(header[4..8].try_into().unwrap()),
                u64::from_le_bytes(header[8..16].try_into().unwrap()),
                u64::from_le_bytes(header[32..40].try_into().unwrap()),
            )
        } else {
            (
                u32::from_le_bytes(header[0..4].try_into().unwrap()),
                u32::from_le_bytes(header[24..28].try_into().unwrap()),
                u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())),
                u64::from(u32::from_le_bytes(header[16..20].try_into().unwrap())),
            )
        };
        if kind != 1 || flags & 1 == 0 || raw_size == 0 {
            continue;
        }
        if usize::try_from(raw).ok().is_none_or(|at| at >= bytes.len()) {
            continue;
        }
        found += 1;
        if found > 1 {
            return true;
        }
    }
    false
}

fn elf_spans(bytes: &[u8]) -> Vec<Span> {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[5] != 1 {
        return Vec::new();
    }
    let class = bytes[4];
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    if class != 1 && class != 2 || (machine != 3 && machine != 62) {
        return Vec::new();
    }
    let (shoff, entsize, shnum, link, minimum) = if class == 2 {
        if bytes.len() < 64 {
            return Vec::new();
        }
        (
            u64::from_le_bytes(bytes[40..48].try_into().unwrap()),
            u16::from_le_bytes([bytes[58], bytes[59]]) as usize,
            u16::from_le_bytes([bytes[60], bytes[61]]) as usize,
            u16::from_le_bytes([bytes[62], bytes[63]]) as usize,
            64usize,
        )
    } else {
        if bytes.len() < 52 {
            return Vec::new();
        }
        (
            u64::from(u32::from_le_bytes(bytes[32..36].try_into().unwrap())),
            u16::from_le_bytes([bytes[46], bytes[47]]) as usize,
            u16::from_le_bytes([bytes[48], bytes[49]]) as usize,
            u16::from_le_bytes([bytes[50], bytes[51]]) as usize,
            40usize,
        )
    };
    if shoff == 0 || shnum == 0 || link >= shnum || entsize < minimum {
        return Vec::new();
    }
    let shnum = shnum.min(64);
    let Some(strings) = elf_section(bytes, shoff, entsize, link, class, minimum) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    for index in 0..shnum {
        let Some(section) = elf_section(bytes, shoff, entsize, index, class, minimum) else {
            break;
        };
        if section.size == 0 || section.offset >= bytes.len() {
            continue;
        }
        let Some(name) = elf_name(bytes, strings.offset, strings.size, section.name) else {
            continue;
        };
        let raw_size = usize::try_from(section.size).unwrap_or(usize::MAX);
        let available = raw_size.min(bytes.len() - section.offset);
        if available == 0 {
            continue;
        }
        spans.push(Span {
            word: "section",
            name,
            start: section.offset,
            end: section.offset + available,
        });
    }
    spans
}

fn elf_sections_unread(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[5] != 1 {
        return false;
    }
    let class = bytes[4];
    let Some(machine_bytes) = bytes.get(18..20).and_then(|raw| raw.try_into().ok()) else {
        return false;
    };
    let machine = u16::from_le_bytes(machine_bytes);
    if class != 1 && class != 2 || (machine != 3 && machine != 62) {
        return false;
    }
    let (shoff, entsize, shnum, minimum) = if class == 2 {
        if bytes.len() < 64 {
            return false;
        }
        (
            u64::from_le_bytes(bytes[40..48].try_into().unwrap_or([0; 8])),
            u16::from_le_bytes([bytes[58], bytes[59]]) as u64,
            u16::from_le_bytes([bytes[60], bytes[61]]) as u64,
            64u64,
        )
    } else if bytes.len() < 52 {
        return false;
    } else {
        (
            u64::from(u32::from_le_bytes(bytes[32..36].try_into().unwrap_or([0; 4]))),
            u16::from_le_bytes([bytes[46], bytes[47]]) as u64,
            u16::from_le_bytes([bytes[48], bytes[49]]) as u64,
            40u64,
        )
    };
    if shoff == 0 || shnum == 0 || entsize < minimum {
        return false;
    }
    if shnum > 64 {
        return true;
    }
    let Some(end) = entsize.checked_mul(shnum).and_then(|size| shoff.checked_add(size)) else {
        return true;
    };
    end > bytes.len() as u64
}

struct ElfSection {
    name: u32,
    offset: usize,
    size: usize,
}

fn elf_section(bytes: &[u8], shoff: u64, entsize: usize, index: usize, class: u8, minimum: usize) -> Option<ElfSection> {
    let at = usize::try_from(shoff).ok()?.checked_add(index.checked_mul(entsize)?)?;
    let header = bytes.get(at..at.checked_add(minimum)?)?;
    if class == 2 {
        let offset = usize::try_from(u64::from_le_bytes(header[24..32].try_into().ok()?)).ok()?;
        let size = usize::try_from(u64::from_le_bytes(header[32..40].try_into().ok()?)).unwrap_or(usize::MAX);
        Some(ElfSection {
            name: u32::from_le_bytes(header[0..4].try_into().ok()?),
            offset,
            size,
        })
    } else {
        let offset = u32::from_le_bytes(header[16..20].try_into().ok()?) as usize;
        let size = u32::from_le_bytes(header[20..24].try_into().ok()?) as usize;
        Some(ElfSection {
            name: u32::from_le_bytes(header[0..4].try_into().ok()?),
            offset,
            size,
        })
    }
}

fn elf_name(bytes: &[u8], strings: usize, size: usize, name_at: u32) -> Option<String> {
    let name_at = name_at as usize;
    if name_at >= size {
        return None;
    }
    let start = strings.checked_add(name_at)?;
    let table_end = strings.saturating_add(size).min(bytes.len());
    let cap_end = start.saturating_add(64);
    let limit = table_end.min(cap_end);
    let raw = bytes.get(start..limit)?;
    if let Some(end) = raw.iter().position(|byte| *byte == 0) {
        if end == 0 {
            return None;
        }
        return Some(String::from_utf8_lossy(&raw[..end]).into_owned());
    }
    if raw.is_empty() || limit < cap_end {
        return None;
    }
    let mut name = String::from_utf8_lossy(raw).into_owned();
    name.push_str("...");
    Some(name)
}

fn x86_mach_segment(bytes: &[u8]) -> Option<Option<Window>> {
    let magic = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
    let (header, segment_cmd, minimum) = match magic {
        0xFEEDFACF => (32usize, 0x19u32, 72usize),
        0xFEEDFACE => (28usize, 1u32, 48usize),
        _ => return None,
    };
    let cpu = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    if cpu != 7 && cpu != 0x0100_0007 {
        return None;
    }
    if bytes.len() < header {
        return Some(None);
    }
    let commands = u32::from_le_bytes(bytes.get(16..20)?.try_into().ok()?) as usize;
    let size = u32::from_le_bytes(bytes.get(20..24)?.try_into().ok()?) as usize;
    let declared = header.checked_add(size)?;
    let mut offset = header;
    for _ in 0..commands.min(64) {
        if offset.checked_add(8)? > declared.min(bytes.len()) {
            break;
        }
        let cmd = u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?);
        let cmdsize = u32::from_le_bytes(bytes.get(offset + 4..offset + 8)?.try_into().ok()?) as usize;
        if cmdsize < 8 {
            break;
        }
        let next = offset.checked_add(cmdsize)?;
        if next > declared.min(bytes.len()) {
            break;
        }
        if cmd == segment_cmd && cmdsize >= minimum {
            let (name, file_off, file_size, prot) = if magic == 0xFEEDFACF {
                (
                    segment_name(bytes, offset + 8)?,
                    u64::from_le_bytes(bytes.get(offset + 40..offset + 48)?.try_into().ok()?),
                    u64::from_le_bytes(bytes.get(offset + 48..offset + 56)?.try_into().ok()?),
                    u32::from_le_bytes(bytes.get(offset + 60..offset + 64)?.try_into().ok()?),
                )
            } else {
                (
                    segment_name(bytes, offset + 8)?,
                    u64::from(u32::from_le_bytes(bytes.get(offset + 32..offset + 36)?.try_into().ok()?)),
                    u64::from(u32::from_le_bytes(bytes.get(offset + 36..offset + 40)?.try_into().ok()?)),
                    u32::from_le_bytes(bytes.get(offset + 44..offset + 48)?.try_into().ok()?),
                )
            };
            if prot & 4 != 0 && file_size != 0 {
                if let Some(raw) = usize::try_from(file_off).ok().filter(|raw| *raw < bytes.len()) {
                    let raw_size = usize::try_from(file_size).unwrap_or(usize::MAX);
                    let available = raw_size.min(bytes.len() - raw);
                    let taken = available.min(CODE_LIMIT);
                    return Some(Some(Window {
                        name,
                        start: raw,
                        end: raw + taken,
                        truncated: available > CODE_LIMIT,
                    }));
                }
            }
        }
        offset = next;
    }
    Some(None)
}

fn macho_commands_unread(bytes: &[u8]) -> bool {
    let Some(magic) = bytes.get(0..4).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    let header = match magic {
        0xFEEDFACF => 32usize,
        0xFEEDFACE => 28usize,
        _ => return false,
    };
    let Some(cpu) = bytes.get(4..8).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    if cpu != 7 && cpu != 0x0100_0007 || bytes.len() < header {
        return false;
    }
    let Some(commands) = bytes.get(16..20).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    let Some(size) = bytes.get(20..24).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    let commands = commands as usize;
    let Some(declared) = header.checked_add(size as usize) else {
        return true;
    };
    if commands > 64 {
        return true;
    }
    let mut offset = header;
    let mut read = 0usize;
    for _ in 0..commands {
        let Some(header_end) = offset.checked_add(8) else {
            return true;
        };
        if header_end > declared.min(bytes.len()) {
            return true;
        }
        let Some(cmdsize) = bytes.get(offset + 4..offset + 8).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
            return true;
        };
        let cmdsize = cmdsize as usize;
        if cmdsize < 8 {
            return read < commands;
        }
        let Some(next) = offset.checked_add(cmdsize) else {
            return true;
        };
        if next > declared.min(bytes.len()) {
            return true;
        }
        offset = next;
        read += 1;
    }
    read < commands
}

fn another_segment(bytes: &[u8]) -> bool {
    let Some(magic) = bytes.get(0..4).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    let (header, segment_cmd, minimum) = match magic {
        0xFEEDFACF => (32usize, 0x19u32, 72usize),
        0xFEEDFACE => (28usize, 1u32, 48usize),
        _ => return false,
    };
    let Some(cpu) = bytes.get(4..8).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    if cpu != 7 && cpu != 0x0100_0007 || bytes.len() < header {
        return false;
    }
    let Some(commands) = bytes.get(16..20).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    let Some(size) = bytes.get(20..24).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
        return false;
    };
    let Some(declared) = header.checked_add(size as usize) else {
        return false;
    };
    let mut found = 0usize;
    let mut offset = header;
    for _ in 0..(commands as usize).min(64) {
        if offset.saturating_add(8) > declared.min(bytes.len()) {
            break;
        }
        let Some(cmd) = bytes.get(offset..offset + 4).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
            break;
        };
        let Some(cmdsize) = bytes.get(offset + 4..offset + 8).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
            break;
        };
        let cmdsize = cmdsize as usize;
        if cmdsize < 8 {
            break;
        }
        let Some(next) = offset.checked_add(cmdsize) else {
            break;
        };
        if next > declared.min(bytes.len()) {
            break;
        }
        if cmd == segment_cmd && cmdsize >= minimum {
            let fields = if magic == 0xFEEDFACF {
                bytes.get(offset + 40..offset + 64).and_then(|raw| {
                    let file_off = u64::from_le_bytes(raw[0..8].try_into().ok()?);
                    let file_size = u64::from_le_bytes(raw[8..16].try_into().ok()?);
                    let prot = u32::from_le_bytes(raw[20..24].try_into().ok()?);
                    Some((file_off, file_size, prot))
                })
            } else {
                bytes.get(offset + 32..offset + 48).and_then(|raw| {
                    let file_off = u64::from(u32::from_le_bytes(raw[0..4].try_into().ok()?));
                    let file_size = u64::from(u32::from_le_bytes(raw[4..8].try_into().ok()?));
                    let prot = u32::from_le_bytes(raw[12..16].try_into().ok()?);
                    Some((file_off, file_size, prot))
                })
            };
            if let Some((file_off, file_size, prot)) = fields {
                if prot & 4 != 0 && file_size != 0 {
                    if usize::try_from(file_off).ok().is_some_and(|raw| raw < bytes.len()) {
                        found += 1;
                        if found > 1 {
                            return true;
                        }
                    }
                }
            }
        }
        offset = next;
    }
    false
}

fn macho_spans(bytes: &[u8]) -> Vec<Span> {
    let magic = match bytes.get(0..4).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) {
        Some(0xFEEDFACF) => 0xFEEDFACFu32,
        Some(0xFEEDFACE) => 0xFEEDFACEu32,
        _ => return Vec::new(),
    };
    let (header, segment_cmd, minimum) = if magic == 0xFEEDFACF {
        (32usize, 0x19u32, 72usize)
    } else {
        (28usize, 1u32, 48usize)
    };
    let Some(cpu_bytes) = bytes.get(4..8) else {
        return Vec::new();
    };
    let cpu = u32::from_le_bytes(cpu_bytes.try_into().unwrap());
    if cpu != 7 && cpu != 0x0100_0007 || bytes.len() < header {
        return Vec::new();
    }
    let Some(command_bytes) = bytes.get(16..20) else {
        return Vec::new();
    };
    let Some(size_bytes) = bytes.get(20..24) else {
        return Vec::new();
    };
    let commands = u32::from_le_bytes(command_bytes.try_into().unwrap()) as usize;
    let size = u32::from_le_bytes(size_bytes.try_into().unwrap()) as usize;
    let Some(declared) = header.checked_add(size) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    let mut offset = header;
    for _ in 0..commands.min(64) {
        if offset.saturating_add(8) > declared.min(bytes.len()) {
            break;
        }
        let Some(cmd) = bytes.get(offset..offset + 4).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
            break;
        };
        let Some(cmdsize) = bytes.get(offset + 4..offset + 8).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes) else {
            break;
        };
        let cmdsize = cmdsize as usize;
        if cmdsize < 8 {
            break;
        }
        let Some(next) = offset.checked_add(cmdsize) else {
            break;
        };
        if next > declared.min(bytes.len()) {
            break;
        }
        if cmd == segment_cmd && cmdsize >= minimum {
            let parsed = if magic == 0xFEEDFACF {
                segment_name(bytes, offset + 8).and_then(|name| {
                    let file_off = u64::from_le_bytes(bytes.get(offset + 40..offset + 48)?.try_into().ok()?);
                    let file_size = u64::from_le_bytes(bytes.get(offset + 48..offset + 56)?.try_into().ok()?);
                    Some((name, file_off, file_size))
                })
            } else {
                segment_name(bytes, offset + 8).and_then(|name| {
                    let file_off = u64::from(u32::from_le_bytes(bytes.get(offset + 32..offset + 36)?.try_into().ok()?));
                    let file_size = u64::from(u32::from_le_bytes(bytes.get(offset + 36..offset + 40)?.try_into().ok()?));
                    Some((name, file_off, file_size))
                })
            };
            if let Some((name, file_off, file_size)) = parsed {
                if let Some(raw) = usize::try_from(file_off).ok().filter(|raw| file_size != 0 && *raw < bytes.len()) {
                    let raw_size = usize::try_from(file_size).unwrap_or(usize::MAX);
                    let available = raw_size.min(bytes.len() - raw);
                    if available > 0 {
                        spans.push(Span {
                            word: "segment",
                            name,
                            start: raw,
                            end: raw + available,
                        });
                    }
                }
            }
        }
        offset = next;
    }
    spans
}

fn segment_name(bytes: &[u8], at: usize) -> Option<String> {
    let raw = bytes.get(at..at.checked_add(16)?)?;
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let name = &raw[..end];
    Some(if name.is_empty() {
        "(unnamed)".to_string()
    } else {
        String::from_utf8_lossy(name).into_owned()
    })
}

fn follows_printable(bytes: &[u8], offset: usize) -> bool {
    offset
        .checked_sub(1)
        .is_some_and(|previous| (0x20..=0x7e).contains(&bytes[previous]))
}

fn near_condition(op: u8) -> Option<&'static str> {
    match op {
        0x80 => Some("jo32"),
        0x81 => Some("jno32"),
        0x82 => Some("jb32"),
        0x83 => Some("jae32"),
        0x84 => Some("je32"),
        0x85 => Some("jne32"),
        0x86 => Some("jbe32"),
        0x87 => Some("ja32"),
        0x88 => Some("js32"),
        0x89 => Some("jns32"),
        0x8A => Some("jp32"),
        0x8B => Some("jnp32"),
        0x8C => Some("jl32"),
        0x8D => Some("jge32"),
        0x8E => Some("jle32"),
        0x8F => Some("jg32"),
        _ => None,
    }
}

fn short_condition(op: u8) -> Option<&'static str> {
    match op {
        0x70 => Some("jo"),
        0x71 => Some("jno"),
        0x72 => Some("jb"),
        0x73 => Some("jae"),
        0x74 => Some("je"),
        0x75 => Some("jne"),
        0x76 => Some("jbe"),
        0x77 => Some("ja"),
        0x78 => Some("js"),
        0x79 => Some("jns"),
        0x7A => Some("jp"),
        0x7B => Some("jnp"),
        0x7C => Some("jl"),
        0x7D => Some("jge"),
        0x7E => Some("jle"),
        0x7F => Some("jg"),
        _ => None,
    }
}

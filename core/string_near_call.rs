//! Printable ASCII runs and the earliest prior relative call.
//! A listed call is not executed.

const RUN_LIMIT: usize = 16;
const EXTRA_LIMIT: usize = 16;
const MIN_RUN: usize = 6;
const LOOKBACK: usize = 64;
const NOTABLE_LOOKBACK: usize = 256;

pub fn string_near_call(bytes: &[u8]) -> String {
    let sections = sections(bytes);
    let scan_x86 = !arm_elf(bytes) && !arm_mach(bytes) && !arm_pe(bytes) && !dex_image(bytes) && !zip_image(bytes);
    let mut report = String::new();
    let mut ordinary = 0usize;
    let mut extra = 0usize;
    let mut skipped = false;
    let mut index = 0usize;
    while index < bytes.len() && open(ordinary, extra) {
        if !printable(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < bytes.len() && printable(bytes[index]) {
            index += 1;
        }
        if index - start < MIN_RUN {
            continue;
        }
        let text = run_text(bytes, start, index);
        if !push_run(&mut report, &sections, bytes, "text", start, index, &text, scan_x86, &mut ordinary, &mut extra) {
            skipped = true;
        }
    }
    let ascii_at = index;
    let mut wide = Vec::new();
    let mut index = 0usize;
    while index + 1 < bytes.len() && open(ordinary, extra) {
        if !(bytes[index] == 0 && printable(bytes[index + 1])) {
            index += 1;
            continue;
        }
        let start = index;
        let mut text = String::new();
        while index + 1 < bytes.len() && bytes[index] == 0 && printable(bytes[index + 1]) {
            text.push(char::from(bytes[index + 1]));
            index += 2;
        }
        if text.len() < MIN_RUN || little_endian_follows(bytes, start, &text) {
            continue;
        }
        if !keep(ordinary, extra, &text) {
            skipped = true;
            continue;
        }
        wide.push((start, index));
        push_run(&mut report, &sections, bytes, "utf16be", start, index, &text, scan_x86, &mut ordinary, &mut extra);
    }
    let wide_at = index;
    let mut index = 0usize;
    while index + 1 < bytes.len() && open(ordinary, extra) {
        if !(printable(bytes[index]) && bytes[index + 1] == 0) {
            index += 1;
            continue;
        }
        let start = index;
        let mut text = String::new();
        while index + 1 < bytes.len() && printable(bytes[index]) && bytes[index + 1] == 0 {
            text.push(char::from(bytes[index]));
            index += 2;
        }
        if text.len() < MIN_RUN || wide.iter().any(|&(from, to)| start >= from && start < to) {
            continue;
        }
        if !keep(ordinary, extra, &text) {
            skipped = true;
            continue;
        }
        push_run(&mut report, &sections, bytes, "utf16", start, index, &text, scan_x86, &mut ordinary, &mut extra);
    }
    let le_at = index;
    if ordinary + extra == 0 {
        report.push_str("no text runs were found");
    }
    if skipped || text_runs_unread(bytes, ascii_at, wide_at, le_at, &wide) {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str("later text runs were not listed");
    }
    if sections_unread(bytes) {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str("later sections were not listed");
    }
    report.push('\n');
    report.push_str("The instruction was not executed.");
    report
}

struct PeSection {
    name: String,
    rva: u64,
    virtual_end: u64,
    raw: usize,
    raw_end: usize,
}

fn little_endian_follows(bytes: &[u8], start: usize, text: &str) -> bool {
    let mut at = start + 1;
    for byte in text.bytes() {
        if at + 1 >= bytes.len() || bytes[at] != byte || bytes[at + 1] != 0 {
            return false;
        }
        at += 2;
    }
    true
}

fn open(ordinary: usize, extra: usize) -> bool {
    ordinary < RUN_LIMIT || extra < EXTRA_LIMIT
}

fn text_runs_unread(bytes: &[u8], ascii_at: usize, wide_at: usize, le_at: usize, wide: &[(usize, usize)]) -> bool {
    if ascii_at < bytes.len() {
        return ascii_run_after(bytes, ascii_at) || wide_run_after(bytes, 0) || le_run_after(bytes, 0, &[]);
    }
    if wide_at + 1 < bytes.len() {
        return wide_run_after(bytes, wide_at) || le_run_after(bytes, 0, wide);
    }
    if le_at + 1 < bytes.len() {
        return le_run_after(bytes, le_at, wide);
    }
    false
}

fn ascii_run_after(bytes: &[u8], mut index: usize) -> bool {
    while index < bytes.len() {
        if !printable(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < bytes.len() && printable(bytes[index]) {
            index += 1;
        }
        if index - start >= MIN_RUN {
            return true;
        }
    }
    false
}

fn wide_run_after(bytes: &[u8], mut index: usize) -> bool {
    while index + 1 < bytes.len() {
        if !(bytes[index] == 0 && printable(bytes[index + 1])) {
            index += 1;
            continue;
        }
        let start = index;
        let mut text = String::new();
        while index + 1 < bytes.len() && bytes[index] == 0 && printable(bytes[index + 1]) {
            text.push(char::from(bytes[index + 1]));
            index += 2;
        }
        if text.len() >= MIN_RUN && !little_endian_follows(bytes, start, &text) {
            return true;
        }
    }
    false
}

fn le_run_after(bytes: &[u8], mut index: usize, wide: &[(usize, usize)]) -> bool {
    while index + 1 < bytes.len() {
        if !(printable(bytes[index]) && bytes[index + 1] == 0) {
            index += 1;
            continue;
        }
        let start = index;
        let mut count = 0usize;
        while index + 1 < bytes.len() && printable(bytes[index]) && bytes[index + 1] == 0 {
            count += 1;
            index += 2;
        }
        if count >= MIN_RUN && !wide.iter().any(|&(from, to)| start >= from && start < to) {
            return true;
        }
    }
    false
}

fn keep(ordinary: usize, extra: usize, text: &str) -> bool {
    if notable(text) {
        ordinary < RUN_LIMIT || extra < EXTRA_LIMIT
    } else {
        ordinary < RUN_LIMIT
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

fn dex_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"dex\n")
}

fn zip_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
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

fn arm_elf(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || !bytes.starts_with(b"\x7fELF") || (bytes[4] != 1 && bytes[4] != 2) {
        return false;
    }
    let machine = if bytes[5] == 1 {
        u16::from_le_bytes([bytes[18], bytes[19]])
    } else if bytes[5] == 2 {
        u16::from_be_bytes([bytes[18], bytes[19]])
    } else {
        return false;
    };
    machine == 40 || machine == 183
}

fn push_run(report: &mut String, sections: &[PeSection], bytes: &[u8], label: &str, start: usize, end: usize, text: &str, scan_x86: bool, ordinary: &mut usize, extra: &mut usize) -> bool {
    if !keep(*ordinary, *extra, text) {
        return false;
    }
    let shown_text = shown(text);
    if *ordinary + *extra > 0 {
        report.push('\n');
    }
    let place = section_place(sections, start);
    if !scan_x86 {
        report.push_str(&format!("{label} {start:#x}{place} x86 call was not scanned {shown_text}"));
        if notable(text) && *ordinary >= RUN_LIMIT {
            *extra += 1;
        } else {
            *ordinary += 1;
        }
        return true;
    }
    let lookback = if notable(text) { NOTABLE_LOOKBACK } else { LOOKBACK };
    match earliest_transfer(bytes, start, lookback) {
        Some((site, kind, width)) => {
            let site_rva = section_at(sections, site).map(|(_, rva)| rva);
            let site_place = site_rva.map(|rva| format!(" rva {rva:#x}")).unwrap_or_default();
            let text_rva = section_at(sections, start).map(|(_, rva)| rva);
            let target_place = if let Some(rva) = call_target(bytes, site, site_rva, width) {
                let name = section_of_rva(sections, rva).map(|name| format!(" section {name}")).unwrap_or_default();
                let same = text_rva.filter(|text_rva| *text_rva == rva).map(|_| " targets this text").unwrap_or("");
                format!(" target rva {rva:#x}{name}{same}")
            } else {
                match transfer_file(bytes, site, width) {
                    Some(FileTarget::At(file)) => {
                        let same = if file >= start && file < end { " targets this text" } else { "" };
                        format!(" target file {file:#x}{same}")
                    }
                    Some(FileTarget::Outside) => " target outside".to_string(),
                    None => String::new(),
                }
            };
            report.push_str(&format!("{label} {start:#x}{place} near {kind} {site:#x}{site_place}{target_place} {shown_text}"));
        }
        None => report.push_str(&format!("{label} {start:#x}{place} has no prior call or jump {shown_text}")),
    }
    if notable(text) && *ordinary >= RUN_LIMIT {
        *extra += 1;
    } else {
        *ordinary += 1;
    }
    true
}

fn section_place(sections: &[PeSection], offset: usize) -> String {
    let Some((name, rva)) = section_at(sections, offset) else {
        return String::new();
    };
    format!(" section {name} rva {rva:#x}")
}

fn section_at(sections: &[PeSection], offset: usize) -> Option<(String, u64)> {
    sections.iter().find(|section| section.raw_end > section.raw && offset >= section.raw && offset < section.raw_end).map(|section| {
        (section.name.clone(), section.rva + (offset - section.raw) as u64)
    })
}

fn section_of_rva(sections: &[PeSection], rva: u64) -> Option<String> {
    sections.iter().find(|section| section.virtual_end > section.rva && rva >= section.rva && rva < section.virtual_end).map(|section| section.name.clone())
}

fn sections(bytes: &[u8]) -> Vec<PeSection> {
    let mut found = Vec::new();
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return found;
    }
    let Some(pe_bytes) = bytes.get(0x3c..0x40) else {
        return found;
    };
    let Ok(pe_raw) = pe_bytes.try_into() else {
        return found;
    };
    let pe = u32::from_le_bytes(pe_raw) as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return found;
    }
    let coff = pe + 4;
    let Some(count_bytes) = bytes.get(coff + 2..coff + 4) else {
        return found;
    };
    let Some(optional_bytes) = bytes.get(coff + 16..coff + 18) else {
        return found;
    };
    let count = u16::from_le_bytes([count_bytes[0], count_bytes[1]]) as usize;
    let optional = u16::from_le_bytes([optional_bytes[0], optional_bytes[1]]) as usize;
    let Some(mut at) = coff.checked_add(20).and_then(|base| base.checked_add(optional)) else {
        return found;
    };
    for _ in 0..count.min(96) {
        let Some(header) = bytes.get(at..at.saturating_add(40)) else {
            break;
        };
        let virtual_size = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        let rva = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        let raw_size = u32::from_le_bytes([header[16], header[17], header[18], header[19]]) as usize;
        let raw = u32::from_le_bytes([header[20], header[21], header[22], header[23]]) as usize;
        let end = header[..8].iter().position(|byte| *byte == 0).unwrap_or(8);
        let name = if end == 0 { "(unnamed)".to_string() } else { String::from_utf8_lossy(&header[..end]).into_owned() };
        let virtual_end = u64::from(rva).saturating_add(u64::from(virtual_size));
        let raw_end = raw.saturating_add(raw_size);
        found.push(PeSection { name, rva: u64::from(rva), virtual_end, raw, raw_end });
        let Some(next) = at.checked_add(40) else {
            break;
        };
        at = next;
    }
    found
}

fn sections_unread(bytes: &[u8]) -> bool {
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

enum FileTarget {
    At(usize),
    Outside,
}

fn transfer_file(bytes: &[u8], site: usize, width: u8) -> Option<FileTarget> {
    let (next, displacement) = displacement(bytes, site, width)?;
    let Some(target) = (site as i64).checked_add(next).and_then(|value| value.checked_add(displacement)) else {
        return Some(FileTarget::Outside);
    };
    if target < 0 || target > bytes.len() as i64 {
        return Some(FileTarget::Outside);
    }
    Some(FileTarget::At(target as usize))
}

fn displacement(bytes: &[u8], call: usize, width: u8) -> Option<(i64, i64)> {
    if width == 2 {
        let displacement = *bytes.get(call + 1)? as i8;
        Some((2, displacement as i64))
    } else if width == 6 {
        let raw = bytes.get(call + 2..call + 6)?;
        Some((6, i32::from_le_bytes(raw.try_into().ok()?) as i64))
    } else {
        let raw = bytes.get(call + 1..call + 5)?;
        Some((5, i32::from_le_bytes(raw.try_into().ok()?) as i64))
    }
}

fn call_target(bytes: &[u8], call: usize, call_rva: Option<u64>, width: u8) -> Option<u64> {
    let call_rva = call_rva?;
    let (next, displacement) = displacement(bytes, call, width)?;
    let target = (call_rva as i64).checked_add(next)?.checked_add(displacement)?;
    if target < 0 {
        return None;
    }
    Some(target as u64)
}

fn printable(byte: u8) -> bool {
    (0x20..=0x7e).contains(&byte)
}

/// Earliest `0xE8` call, `0xE9` jump, or `0xEB` short jump before `run_at`.
/// A relative call or jump counts only when its displacement bytes are present.
fn follows_printable(bytes: &[u8], offset: usize) -> bool {
    offset.checked_sub(1).is_some_and(|previous| printable(bytes[previous]))
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

fn earliest_transfer(bytes: &[u8], run_at: usize, lookback: usize) -> Option<(usize, &'static str, u8)> {
    let from = run_at.saturating_sub(lookback);
    (from..run_at).find_map(|offset| {
        let op = *bytes.get(offset)?;
        match op {
            0xE8 | 0xE9 if bytes.len() - offset >= 5 => Some((offset, if op == 0xE8 { "call" } else { "jump" }, 5)),
            0xE0 if bytes.len() - offset >= 2 => Some((offset, "loopne", 2)),
            0xE1 if bytes.len() - offset >= 2 => Some((offset, "loope", 2)),
            0xE2 if bytes.len() - offset >= 2 => Some((offset, "loop", 2)),
            0xEB if bytes.len() - offset >= 2 => Some((offset, "short jump", 2)),
            0x0F if bytes.len() - offset >= 6 => near_condition(bytes[offset + 1]).map(|kind| (offset, kind, 6)),
            op if bytes.len() - offset >= 2 && !follows_printable(bytes, offset) => short_condition(op).map(|kind| (offset, kind, 2)),
            _ => None,
        }
    })
}

fn run_text(bytes: &[u8], start: usize, end: usize) -> String {
    bytes[start..end].iter().copied().map(char::from).collect()
}

fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut more = false;
    for (index, ch) in text.chars().enumerate() {
        if index == 64 {
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

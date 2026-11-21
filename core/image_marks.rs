//! Embedded container marks inside an opaque image.
//! Each mark is a published container signature checked against the following
//! header bytes. A mark is a candidate, not proof the span is a complete member.
#![cfg_attr(not(test), allow(dead_code))]

#[derive(Clone, Copy)]
struct Mark {
    name: &'static str,
    bytes: &'static [u8],
}

const MARKS: &[Mark] = &[
    Mark { name: "elf", bytes: b"\x7fELF" },
    Mark { name: "dex", bytes: b"dex\n" },
    Mark { name: "png", bytes: b"\x89PNG\r\n\x1a\n" },
    Mark { name: "gzip", bytes: b"\x1f\x8b" },
    Mark { name: "xz", bytes: b"\xfd7zXZ\x00" },
    Mark { name: "zstd", bytes: &[0x28, 0xb5, 0x2f, 0xfd] },
    Mark { name: "bzip2", bytes: b"BZh" },
    Mark { name: "zip local", bytes: b"PK\x03\x04" },
    Mark { name: "zip central", bytes: b"PK\x01\x02" },
    Mark { name: "zip end", bytes: b"PK\x05\x06" },
    Mark { name: "seven-zip", bytes: &[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c] },
    Mark { name: "wasm", bytes: b"\0asm" },
    Mark { name: "ar archive", bytes: b"!<arch>\n" },
    Mark { name: "cpio new", bytes: b"070701" },
    Mark { name: "cpio crc", bytes: b"070702" },
    Mark { name: "squashfs", bytes: b"hsqs" },
    Mark { name: "squashfs", bytes: b"sqsh" },
    Mark { name: "cramfs", bytes: &[0x45, 0x3d, 0xcd, 0x28] },
    Mark { name: "cramfs", bytes: &[0x28, 0xcd, 0x3d, 0x45] },
    Mark { name: "legacy image", bytes: &[0x27, 0x05, 0x19, 0x56] },
    Mark { name: "boot image", bytes: b"ANDROID!" },
];

#[derive(Clone)]
pub struct Hit {
    pub offset: u64,
    pub name: &'static str,
}

fn at(bytes: &[u8], index: usize, mark: &[u8]) -> bool {
    bytes.get(index..index + mark.len()).is_some_and(|found| found == mark)
}

fn pe_at(bytes: &[u8], index: usize) -> bool {
    let Some(rest) = bytes.get(index..) else { return false };
    if rest.len() < 0x40 || &rest[..2] != b"MZ" {
        return false;
    }
    let pe = u32::from_le_bytes(rest[0x3c..0x40].try_into().unwrap()) as usize;
    rest.get(pe..pe + 4).is_some_and(|sig| sig == b"PE\0\0")
}

fn scan_range(bytes: &[u8], base: u64) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if pe_at(bytes, index) {
            hits.push(Hit { offset: base + index as u64, name: "pe image" });
        }
        for mark in MARKS {
            if at(bytes, index, mark.bytes) {
                hits.push(Hit { offset: base + index as u64, name: mark.name });
            }
        }
        index += 1;
    }
    hits
}

/// Find container marks. Independent slices run on separate cores when the image
/// is large enough for the overlap to matter; hits are ordered by file offset.
pub fn find(bytes: &[u8], cancelled: &mut dyn FnMut() -> bool) -> Result<Vec<Hit>, &'static str> {
    if cancelled() {
        return Err("cancelled");
    }
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    if bytes.len() < 256 * 1024 || cores < 2 {
        return Ok(scan_range(bytes, 0));
    }
    let overlap = 16usize;
    let chunk = (bytes.len() / cores).max(overlap + 1);
    let mut ranges = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let end = (start + chunk + overlap).min(bytes.len());
        ranges.push((start, end));
        if end == bytes.len() {
            break;
        }
        start += chunk;
    }
    let hits = std::thread::scope(|scope| {
        let mut jobs = Vec::new();
        for (start, end) in ranges {
            jobs.push(scope.spawn(move || scan_range(&bytes[start..end], start as u64)));
        }
        let mut all = Vec::new();
        for job in jobs {
            all.extend(job.join().unwrap_or_default());
        }
        all
    });
    if cancelled() {
        return Err("cancelled");
    }
    let mut hits = hits;
    hits.sort_by_key(|hit| (hit.offset, hit.name));
    hits.dedup_by(|a, b| a.offset == b.offset && a.name == b.name);
    Ok(hits)
}

/// Fixed container marks in one window. A PE image is not included: its signature
/// sits at a header-declared offset that may lie outside this window.
pub fn fixed_hits(bytes: &[u8], base: u64) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        for mark in MARKS {
            if at(bytes, index, mark.bytes) {
                hits.push(Hit { offset: base + index as u64, name: mark.name });
            }
        }
        index += 1;
    }
    hits
}

/// `e_lfanew` when `bytes[index..]` begins with a DOS header long enough to hold it.
pub fn mz_lfanew(bytes: &[u8], index: usize) -> Option<u32> {
    let rest = bytes.get(index..)?;
    if rest.len() < 0x40 || &rest[..2] != b"MZ" {
        return None;
    }
    Some(u32::from_le_bytes(rest[0x3c..0x40].try_into().ok()?))
}

/// Byte length declared by a container header, measured from `bytes[0]`.
/// `Need` means the header continues past this prefix. `Unknown` means the
/// header does not state a member length.
pub enum Measure {
    Bytes(u64),
    Unknown,
    Need(usize),
}

pub fn declared_length(bytes: &[u8], name: &str) -> Measure {
    match name {
        "zip local" => zip_local(bytes),
        "pe image" => pe_image(bytes),
        "elf" => elf_image(bytes),
        "squashfs" => squashfs(bytes),
        "png" => png_image(bytes),
        _ => Measure::Unknown,
    }
}

fn zip_local(bytes: &[u8]) -> Measure {
    if bytes.len() < 30 || &bytes[..4] != b"PK\x03\x04" {
        return Measure::Need(30);
    }
    let flags = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    if flags & 0x0008 != 0 {
        return Measure::Unknown;
    }
    let compressed = u32::from_le_bytes(bytes[18..22].try_into().unwrap()) as u64;
    let name = u16::from_le_bytes(bytes[26..28].try_into().unwrap()) as u64;
    let extra = u16::from_le_bytes(bytes[28..30].try_into().unwrap()) as u64;
    Measure::Bytes(30 + name + extra + compressed)
}

fn pe_image(bytes: &[u8]) -> Measure {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return Measure::Need(0x40);
    }
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    let coff = pe.saturating_add(4);
    let need = coff.saturating_add(20);
    if bytes.len() < need || pe > bytes.len() {
        return Measure::Need(need);
    }
    if bytes.get(pe..pe + 4) != Some(b"PE\0\0".as_ref()) {
        return Measure::Unknown;
    }
    let count = u16::from_le_bytes(bytes[coff + 2..coff + 4].try_into().unwrap()) as usize;
    let optional = u16::from_le_bytes(bytes[coff + 16..coff + 18].try_into().unwrap()) as usize;
    let table = coff.saturating_add(20).saturating_add(optional);
    let table_end = table.saturating_add(count.saturating_mul(40));
    if bytes.len() < table_end {
        return Measure::Need(table_end);
    }
    let mut end = table_end as u64;
    for index in 0..count {
        let at = table + index * 40;
        let raw_size = u32::from_le_bytes(bytes[at + 16..at + 20].try_into().unwrap()) as u64;
        let raw = u32::from_le_bytes(bytes[at + 20..at + 24].try_into().unwrap()) as u64;
        end = end.max(raw.saturating_add(raw_size));
    }
    Measure::Bytes(end)
}

fn elf_image(bytes: &[u8]) -> Measure {
    if bytes.len() < 16 || &bytes[..4] != b"\x7fELF" {
        return Measure::Need(16);
    }
    if !matches!(bytes[4], 1 | 2) || bytes[5] != 1 {
        return Measure::Unknown;
    }
    let wide = bytes[4] == 2;
    let header = if wide { 64 } else { 52 };
    if bytes.len() < header {
        return Measure::Need(header);
    }
    let (phoff, phentsize, phnum) = if wide {
        (
            u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
            u16::from_le_bytes(bytes[54..56].try_into().unwrap()) as u64,
            u16::from_le_bytes(bytes[56..58].try_into().unwrap()) as u64,
        )
    } else {
        (
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as u64,
            u16::from_le_bytes(bytes[42..44].try_into().unwrap()) as u64,
            u16::from_le_bytes(bytes[44..46].try_into().unwrap()) as u64,
        )
    };
    if phnum == 0 || phentsize < if wide { 56 } else { 32 } {
        return Measure::Unknown;
    }
    let Some(table_bytes) = phentsize.checked_mul(phnum) else { return Measure::Unknown };
    let Some(table_end) = phoff.checked_add(table_bytes) else { return Measure::Unknown };
    if table_end > usize::MAX as u64 {
        return Measure::Unknown;
    }
    if (bytes.len() as u64) < table_end {
        return Measure::Need(table_end as usize);
    }
    let mut end = table_end;
    for index in 0..phnum {
        let at = (phoff + index * phentsize) as usize;
        let (offset, size) = if wide {
            (
                u64::from_le_bytes(bytes[at + 8..at + 16].try_into().unwrap()),
                u64::from_le_bytes(bytes[at + 32..at + 40].try_into().unwrap()),
            )
        } else {
            (
                u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as u64,
                u32::from_le_bytes(bytes[at + 16..at + 20].try_into().unwrap()) as u64,
            )
        };
        end = end.max(offset.saturating_add(size));
    }
    Measure::Bytes(end)
}

fn squashfs(bytes: &[u8]) -> Measure {
    if bytes.len() < 48 {
        return Measure::Need(48);
    }
    let little = &bytes[..4] == b"hsqs";
    if !little && &bytes[..4] != b"sqsh" {
        return Measure::Unknown;
    }
    let major = if little { u16::from_le_bytes(bytes[28..30].try_into().unwrap()) } else { u16::from_be_bytes(bytes[28..30].try_into().unwrap()) };
    let used = if little { u64::from_le_bytes(bytes[40..48].try_into().unwrap()) } else { u64::from_be_bytes(bytes[40..48].try_into().unwrap()) };
    if major == 4 && used >= 48 {
        Measure::Bytes(used)
    } else {
        Measure::Unknown
    }
}

fn png_image(bytes: &[u8]) -> Measure {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Measure::Need(8);
    }
    let mut at = 8usize;
    loop {
        if bytes.len() < at + 12 {
            return Measure::Need(at + 12);
        }
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = &bytes[at + 4..at + 8];
        let next = match at.checked_add(12).and_then(|n| n.checked_add(len)) {
            Some(n) => n,
            None => return Measure::Unknown,
        };
        if bytes.len() < next {
            return Measure::Need(next);
        }
        at = next;
        if kind == b"IEND" {
            return Measure::Bytes(at as u64);
        }
    }
}

pub fn report(hits: &[Hit]) -> String {
    let mut out = format!("container marks: {}\n", hits.len());
    for hit in hits {
        out.push_str(&format!("{:#x}  {}\n", hit.offset, hit.name));
    }
    if hits.is_empty() {
        out.push_str("No container mark was found. Unmarked bytes stay unclassified.\n");
    }
    out
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn marks_are_offsets_not_inferred_members() {
        let mut image = vec![0u8; 64];
        image[8..12].copy_from_slice(b"\x7fELF");
        image[20..24].copy_from_slice(b"PK\x03\x04");
        let mut pe = vec![0u8; 0x80];
        pe[0] = b'M';
        pe[1] = b'Z';
        pe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        image.extend_from_slice(&pe);
        let hits = find(&image, &mut || false).unwrap();
        assert!(hits.iter().any(|hit| hit.offset == 8 && hit.name == "elf"));
        assert!(hits.iter().any(|hit| hit.offset == 20 && hit.name == "zip local"));
        assert!(hits.iter().any(|hit| hit.offset == 64 && hit.name == "pe image"));
        assert!(!hits.iter().any(|hit| hit.name == "pe image" && hit.offset == 0));
    }

    #[test]
    fn a_local_archive_header_declares_its_own_length() {
        let mut local = b"PK\x03\x04".to_vec();
        local.extend_from_slice(&[0u8; 14]);
        local.extend_from_slice(&5u32.to_le_bytes());
        local.extend_from_slice(&5u32.to_le_bytes());
        local.extend_from_slice(&1u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.push(b'a');
        local.extend_from_slice(b"hello");
        assert!(matches!(declared_length(&local, "zip local"), Measure::Bytes(36)));
        assert!(matches!(declared_length(&local[..20], "zip local"), Measure::Need(30)));
    }
}

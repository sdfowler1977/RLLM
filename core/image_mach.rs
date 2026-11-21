//! Declared Mach-O and universal-image fields from the published header layout.

pub(crate) fn recognized(bytes: &[u8]) -> bool {
    kind(bytes).is_some()
}

pub(crate) fn report(bytes: &[u8]) -> String {
    let Some(kind) = kind(bytes) else {
        return "No Mach-O or universal-image magic was identified.\n".into();
    };
    match kind {
        Kind::Thin { little, wide } => thin(bytes, little, wide, 0),
        Kind::Fat { little, wide } => fat(bytes, little, wide),
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Thin { little: bool, wide: bool },
    Fat { little: bool, wide: bool },
}

fn kind(bytes: &[u8]) -> Option<Kind> {
    let magic = bytes.get(..4)?;
    let value = u32::from_be_bytes(magic.try_into().ok()?);
    Some(match value {
        0xFEEDFACE => Kind::Thin { little: false, wide: false },
        0xCEFAEDFE => Kind::Thin { little: true, wide: false },
        0xFEEDFACF => Kind::Thin { little: false, wide: true },
        0xCFFAEDFE => Kind::Thin { little: true, wide: true },
        0xCAFEBABE => Kind::Fat { little: false, wide: false },
        0xBEBAFECA => Kind::Fat { little: true, wide: false },
        0xCAFEBABF => Kind::Fat { little: false, wide: true },
        0xBFBAFECA => Kind::Fat { little: true, wide: true },
        _ => return None,
    })
}

struct Words<'a> {
    bytes: &'a [u8],
    little: bool,
}

impl Words<'_> {
    fn u32(&self, at: usize) -> Option<u32> {
        let raw = self.bytes.get(at..at + 4)?;
        let raw: [u8; 4] = raw.try_into().ok()?;
        Some(if self.little { u32::from_le_bytes(raw) } else { u32::from_be_bytes(raw) })
    }

    fn u64(&self, at: usize) -> Option<u64> {
        let raw = self.bytes.get(at..at + 8)?;
        let raw: [u8; 8] = raw.try_into().ok()?;
        Some(if self.little { u64::from_le_bytes(raw) } else { u64::from_be_bytes(raw) })
    }

    fn text(&self, at: usize, len: usize) -> String {
        let raw = self.bytes.get(at..at + len).unwrap_or(&[]);
        let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }
}

fn thin(bytes: &[u8], little: bool, wide: bool, origin: usize) -> String {
    let words = Words { bytes, little };
    let header = if wide { 32 } else { 28 };
    let mut out = String::new();
    let Some(cpu) = words.u32(4) else {
        return format!("Mach-O header at {origin:#x} is shorter than its declared header.\n");
    };
    let Some(subtype) = words.u32(8) else {
        return format!("Mach-O header at {origin:#x} is shorter than its declared header.\n");
    };
    let Some(filetype) = words.u32(12) else {
        return format!("Mach-O header at {origin:#x} is shorter than its declared header.\n");
    };
    let Some(commands) = words.u32(16) else {
        return format!("Mach-O header at {origin:#x} is shorter than its declared header.\n");
    };
    let Some(command_bytes) = words.u32(20) else {
        return format!("Mach-O header at {origin:#x} is shorter than its declared header.\n");
    };
    let Some(flags) = words.u32(24) else {
        return format!("Mach-O header at {origin:#x} is shorter than its declared header.\n");
    };
    if bytes.len() < header {
        return format!("Mach-O header at {origin:#x} is shorter than its declared header.\n");
    }
    let endian = if little { "little" } else { "big" };
    let width = if wide { 64 } else { 32 };
    out.push_str(&format!(
        "Mach-O {width}-bit {endian}-endian header at {origin:#x}: cpu {} ({cpu:#x}); subtype {subtype:#x} ({}); file type {} ({filetype}); commands {commands}; command bytes {command_bytes}; flags {flags:#x} ({}).\n",
        cpu_name(cpu),
        subtype_name(cpu, subtype),
        file_type(filetype),
        flag_names(flags)
    ));
    let mut at = header;
    let declared_end = header.saturating_add(command_bytes as usize);
    for index in 0..commands {
        if at + 8 > bytes.len() || at + 8 > declared_end {
            out.push_str(&format!("Command {index} continues past the examined prefix.\n"));
            break;
        }
        let Some(cmd) = words.u32(at) else { break };
        let Some(size) = words.u32(at + 4) else { break };
        let size = size as usize;
        let align = if wide { 8 } else { 4 };
        if size < 8 || size % align != 0 {
            out.push_str(&format!("Command {index} declares size {size}, which is not a multiple of {align} and is at least 8. The command list stops.\n"));
            break;
        }
        if at + size > bytes.len() || at + size > declared_end {
            out.push_str(&format!("Command {index} ({}) declares {size} bytes that continue past the examined prefix.\n", command_name(cmd)));
            break;
        }
        out.push_str(&command_line(&words, at, cmd, size));
        at += size;
    }
    if at < declared_end && declared_end <= bytes.len() && at == header + command_bytes as usize {
        // The walk consumed the declared command bytes.
    } else if commands > 0 && at < declared_end.min(bytes.len()) && at >= header {
        let consumed = at - header;
        if consumed < command_bytes as usize && !out.contains("continues past") && !out.contains("command list stops") {
            out.push_str(&format!("Declared command bytes are {command_bytes}; the walk consumed {consumed}.\n"));
        }
    }
    out
}

fn command_line(words: &Words, at: usize, cmd: u32, size: usize) -> String {
    let base = cmd & 0x7fff_ffff;
    let required = if cmd & 0x8000_0000 != 0 { "; the dynamic linker requires this command" } else { "" };
    let mut line = format!("command {at:#x} {} ({base:#x}) size {size}{required}", command_name(cmd));
    match base {
        0x1 | 0x19 => {
            let wide = base == 0x19;
            let header = if wide { 72 } else { 56 };
            if size < header {
                line.push_str("\n  command is shorter than a segment header");
            } else {
            let name = words.text(at + 8, 16);
            let (vmaddr, vmsize, fileoff, filesize, prot_at, sect_at, sect_size) = if wide {
                (
                    words.u64(at + 24).unwrap_or(0),
                    words.u64(at + 32).unwrap_or(0),
                    words.u64(at + 40).unwrap_or(0),
                    words.u64(at + 48).unwrap_or(0),
                    at + 56,
                    at + 72,
                    80,
                )
            } else {
                (
                    words.u32(at + 24).unwrap_or(0) as u64,
                    words.u32(at + 28).unwrap_or(0) as u64,
                    words.u32(at + 32).unwrap_or(0) as u64,
                    words.u32(at + 36).unwrap_or(0) as u64,
                    at + 40,
                    at + 56,
                    68,
                )
            };
            let maxprot = words.u32(prot_at).unwrap_or(0);
            let initprot = words.u32(prot_at + 4).unwrap_or(0);
            let nsects = words.u32(prot_at + 8).unwrap_or(0);
            let flags = words.u32(prot_at + 12).unwrap_or(0);
            line.push_str(&format!(
                "\n  segment {name} vm {vmaddr:#x}+{vmsize:#x} file {fileoff:#x}+{filesize:#x} max {} init {} flags {flags:#x} sections {nsects}",
                prot_names(maxprot),
                prot_names(initprot)
            ));
            let mut sect = sect_at;
            for index in 0..nsects {
                if sect + sect_size > at + size {
                    line.push_str(&format!("\n  section {index} does not fit in the command"));
                    break;
                }
                let sectname = words.text(sect, 16);
                let segname = words.text(sect + 16, 16);
                let (addr, span, offset, flags) = if wide {
                    (
                        words.u64(sect + 32).unwrap_or(0),
                        words.u64(sect + 40).unwrap_or(0),
                        words.u32(sect + 48).unwrap_or(0),
                        words.u32(sect + 64).unwrap_or(0),
                    )
                } else {
                    (
                        words.u32(sect + 32).unwrap_or(0) as u64,
                        words.u32(sect + 36).unwrap_or(0) as u64,
                        words.u32(sect + 40).unwrap_or(0),
                        words.u32(sect + 56).unwrap_or(0),
                    )
                };
                line.push_str(&format!(
                    "\n  section {sectname} in {segname} address {addr:#x} size {span:#x} file {offset:#x} {}",
                    section_flags(flags)
                ));
                sect += sect_size;
            }
            }
        }
        0xc | 0xd | 0xe | 0xf | 0x10 | 0x18 | 0x1c | 0x1f | 0x20 | 0x23 | 0x27 => {
            if let Some(offset) = words.u32(at + 8) {
                let start = at + offset as usize;
                if offset >= 8 && start < at + size {
                    let end = bytes_end(words.bytes, start, at + size);
                    let name = String::from_utf8_lossy(&words.bytes[start..end]);
                    line.push_str(&format!("\n  path {name}"));
                }
            }
        }
        0x1b if size >= 24 => {
            let uuid = &words.bytes[at + 8..at + 24];
            line.push_str(&format!("\n  uuid {}", hex(uuid)));
        }
        0x28 if size >= 24 => {
            if let (Some(entry), Some(stack)) = (words.u64(at + 8), words.u64(at + 16)) {
                line.push_str(&format!("\n  entry file offset {entry:#x}; stack {stack:#x}"));
            }
        }
        0x1d | 0x1e | 0x26 | 0x29 | 0x2b | 0x2e | 0x33 | 0x34 if size >= 16 => {
            if let (Some(offset), Some(span)) = (words.u32(at + 8), words.u32(at + 12)) {
                line.push_str(&format!("\n  link-edit data file {offset:#x} size {span:#x}"));
            }
        }
        0x32 if size >= 24 => {
            if let (Some(platform), Some(min), Some(sdk)) = (words.u32(at + 8), words.u32(at + 12), words.u32(at + 16)) {
                line.push_str(&format!("\n  platform {platform} minimum {} sdk {}", packed_version(min), packed_version(sdk)));
            }
        }
        _ => {}
    }
    line.push('\n');
    line
}

fn fat(bytes: &[u8], little: bool, wide: bool) -> String {
    let words = Words { bytes, little };
    let Some(count) = words.u32(4) else {
        return "Universal image header is shorter than its count.\n".into();
    };
    let stride = if wide { 32 } else { 20 };
    let mut out = format!(
        "Universal Mach-O image, {}-endian, {} architectures, {}-byte records.\n",
        if little { "little" } else { "big" },
        count,
        stride
    );
    for index in 0..count {
        let at = 8 + index as usize * stride;
        if at + stride > bytes.len() {
            out.push_str(&format!("Architecture {index} continues past the examined prefix.\n"));
            break;
        }
        let cpu = words.u32(at).unwrap_or(0);
        let subtype = words.u32(at + 4).unwrap_or(0);
        let (offset, size) = if wide {
            (words.u64(at + 8).unwrap_or(0), words.u64(at + 16).unwrap_or(0))
        } else {
            (words.u32(at + 8).unwrap_or(0) as u64, words.u32(at + 12).unwrap_or(0) as u64)
        };
        let align = if wide { words.u32(at + 24).unwrap_or(0) } else { words.u32(at + 16).unwrap_or(0) };
        out.push_str(&format!(
            "architecture {index}: cpu {} ({cpu:#x}); subtype {subtype:#x}; file {offset:#x}+{size:#x}; align 2^{align}.\n",
            cpu_name(cpu)
        ));
        let end = offset.saturating_add(size);
        if offset > usize::MAX as u64 || end > bytes.len() as u64 {
            out.push_str("  slice is outside the examined prefix.\n");
            continue;
        }
        let slice = &bytes[offset as usize..end as usize];
        if recognized(slice) {
            out.push_str(&thin_or_note(slice, offset as usize));
        } else {
            out.push_str("  slice does not start with a Mach-O magic.\n");
        }
    }
    out
}

fn thin_or_note(bytes: &[u8], origin: usize) -> String {
    match kind(bytes) {
        Some(Kind::Thin { little, wide }) => thin(bytes, little, wide, origin),
        _ => "  nested image is not a thin Mach-O header.\n".into(),
    }
}

fn bytes_end(bytes: &[u8], start: usize, limit: usize) -> usize {
    let limit = limit.min(bytes.len());
    bytes[start..limit].iter().position(|byte| *byte == 0).map(|at| start + at).unwrap_or(limit)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn cpu_name(cpu: u32) -> &'static str {
    match cpu {
        7 => "x86",
        0x0100_0007 => "x86-64",
        12 => "Arm",
        0x0100_000c => "Arm64",
        0x0200_000c => "Arm64_32",
        18 => "PowerPC",
        0x0100_0012 => "PowerPC64",
        _ => "unclassified cpu",
    }
}

fn subtype_name(cpu: u32, subtype: u32) -> &'static str {
    let value = subtype & 0x00ff_ffff;
    match (cpu, value) {
        (0x0100_0007, 3) | (7, 3) => "all",
        (0x0100_000c, 0) => "all",
        (0x0100_000c, 1) => "v8",
        (0x0100_000c, 2) => "e",
        _ => "unclassified subtype",
    }
}

fn file_type(value: u32) -> &'static str {
    match value {
        1 => "object",
        2 => "execute",
        3 => "fixed virtual-memory library",
        4 => "core",
        5 => "preload",
        6 => "dynamic library",
        7 => "dynamic linker",
        8 => "bundle",
        9 => "dynamic library stub",
        10 => "debug symbols",
        11 => "kernel extension bundle",
        12 => "file set",
        _ => "unclassified file type",
    }
}

fn flag_names(flags: u32) -> String {
    const BITS: &[(u32, &str)] = &[
        (0x1, "no undefined symbols"),
        (0x2, "incremental link"),
        (0x4, "dynamic link"),
        (0x8, "bind at load"),
        (0x10, "prebound"),
        (0x20, "split segments"),
        (0x40, "lazy init"),
        (0x80, "two-level namespace"),
        (0x100, "force flat"),
        (0x200, "no multiple definitions"),
        (0x400, "no fixed prebinding"),
        (0x800, "prebindable"),
        (0x1000, "all modules bound"),
        (0x2000, "subsections via symbols"),
        (0x4000, "canonical"),
        (0x8000, "weak defines"),
        (0x10000, "binds to weak"),
        (0x20000, "allow stack execution"),
        (0x40000, "root safe"),
        (0x80000, "setuid safe"),
        (0x100000, "no reexported libraries"),
        (0x200000, "position independent"),
        (0x400000, "dead-strippable library"),
        (0x800000, "thread-local descriptors"),
        (0x1000000, "no heap execution"),
        (0x0200_0000, "application-extension safe"),
        (0x0400_0000, "symbol table out of sync with dynamic info"),
        (0x0800_0000, "simulator support"),
        (0x8000_0000, "library in shared cache"),
    ];
    let mut names = Vec::new();
    let mut seen = 0u32;
    for (bit, name) in BITS {
        if flags & bit != 0 {
            names.push(*name);
            seen |= bit;
        }
    }
    let rest = flags & !seen;
    if rest != 0 {
        names.push("unclassified flag bits remain");
    }
    if names.is_empty() { "none".into() } else { names.join(", ") }
}

fn prot_names(value: u32) -> String {
    let mut names = Vec::new();
    if value & 1 != 0 { names.push("read"); }
    if value & 2 != 0 { names.push("write"); }
    if value & 4 != 0 { names.push("execute"); }
    if value & !7 != 0 { names.push("unclassified"); }
    if names.is_empty() { "none".into() } else { names.join("|") }
}

fn section_flags(flags: u32) -> String {
    let kind = match flags & 0xff {
        0 => "regular",
        1 => "zero-fill",
        2 => "c strings",
        3 => "4-byte literals",
        4 => "8-byte literals",
        5 => "literal pointers",
        6 => "non-lazy symbol pointers",
        7 => "lazy symbol pointers",
        8 => "symbol stubs",
        9 => "module init",
        10 => "module term",
        11 => "coalesced",
        12 => "zero-fill 16-byte",
        13 => "interposing",
        14 => "16-byte literals",
        15 => "dtrace",
        _ => "unclassified section type",
    };
    let mut extra = Vec::new();
    if flags & 0x8000_0000 != 0 { extra.push("pure instructions"); }
    if flags & 0x400 != 0 { extra.push("some instructions"); }
    if flags & 0x200 != 0 { extra.push("external relocations"); }
    if flags & 0x100 != 0 { extra.push("local relocations"); }
    if extra.is_empty() { kind.into() } else { format!("{kind}; {}", extra.join(", ")) }
}

fn command_name(cmd: u32) -> &'static str {
    match cmd & 0x7fff_ffff {
        0x1 => "segment",
        0x2 => "symbol table",
        0xb => "dynamic symbol table",
        0xc => "load dynamic library",
        0xd => "identify dynamic library",
        0xe => "load dynamic linker",
        0xf => "identify dynamic linker",
        0x19 => "segment",
        0x1b => "uuid",
        0x1c => "run path",
        0x1d => "code signature",
        0x1e => "split segment info",
        0x1f => "reexport dynamic library",
        0x20 => "lazy load dynamic library",
        0x21 => "encryption info",
        0x22 => "dynamic linker info",
        0x23 => "load upward dynamic library",
        0x24 => "minimum macOS version",
        0x25 => "minimum iPhone version",
        0x26 => "function starts",
        0x27 => "dynamic linker environment",
        0x28 => "entry point",
        0x29 => "data in code",
        0x2a => "source version",
        0x2b => "code-signing data",
        0x2c => "encryption info",
        0x2d => "linker option",
        0x2e => "linker optimization hint",
        0x2f => "minimum tvOS version",
        0x30 => "minimum watchOS version",
        0x31 => "note",
        0x32 => "build version",
        0x33 => "export trie",
        0x34 => "chained fixups",
        0x35 => "file-set entry",
        _ => "unclassified command",
    }
}

fn packed_version(value: u32) -> String {
    format!("{}.{}.{}", value >> 16, (value >> 8) & 0xff, value & 0xff)
}

#[cfg(test)]
mod checks {
    use super::*;

    fn put_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u64(out: &mut Vec<u8>, value: u64) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn name16(text: &str) -> [u8; 16] {
        let mut raw = [0u8; 16];
        let bytes = text.as_bytes();
        raw[..bytes.len()].copy_from_slice(bytes);
        raw
    }

    #[test]
    fn thin_execute_reports_segment_section_and_uuid() {
        let mut image = Vec::new();
        image.extend_from_slice(&0xFEEDFACFu32.to_le_bytes());
        put_u32(&mut image, 0x0100_0007);
        put_u32(&mut image, 3);
        put_u32(&mut image, 2);
        put_u32(&mut image, 2);
        put_u32(&mut image, 72 + 80 + 24);
        put_u32(&mut image, 0x200000);
        put_u32(&mut image, 0);
        put_u32(&mut image, 0x19);
        put_u32(&mut image, 72 + 80);
        image.extend_from_slice(&name16("__TEXT"));
        put_u64(&mut image, 0x1000_0000);
        put_u64(&mut image, 0x1000);
        put_u64(&mut image, 0);
        put_u64(&mut image, 0x1000);
        put_u32(&mut image, 7);
        put_u32(&mut image, 5);
        put_u32(&mut image, 1);
        put_u32(&mut image, 0);
        image.extend_from_slice(&name16("__text"));
        image.extend_from_slice(&name16("__TEXT"));
        put_u64(&mut image, 0x1000_0000);
        put_u64(&mut image, 0x10);
        put_u32(&mut image, 0);
        put_u32(&mut image, 2);
        put_u32(&mut image, 0);
        put_u32(&mut image, 0);
        put_u32(&mut image, 0x8000_0400);
        put_u32(&mut image, 0);
        put_u32(&mut image, 0);
        put_u32(&mut image, 0);
        put_u32(&mut image, 0x1b);
        put_u32(&mut image, 24);
        image.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00]);
        let text = report(&image);
        assert!(text.contains("x86-64"));
        assert!(text.contains("execute"));
        assert!(text.contains("position independent"));
        assert!(text.contains("segment __TEXT"));
        assert!(text.contains("section __text"));
        assert!(text.contains("pure instructions"));
        assert!(text.contains("uuid 112233445566778899aabbccddeeff00"));
        assert!(recognized(&image));
    }

    #[test]
    fn short_header_does_not_invent_fields() {
        let text = report(&[0xcf, 0xfa, 0xed, 0xfe, 0x07]);
        assert!(text.contains("shorter than its declared header"));
        assert!(!text.contains("segment"));
    }
}

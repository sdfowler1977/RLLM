//! Workspace reports for an instruction listing, container marks, and a saved capture.
//! Each report is a new workspace-root file. Bytes are streamed and read back.
//! A refusal is a real failure: the subject cannot be read, the name exists, or
//! the written bytes do not match the readback.
use super::*;
use crate::host_fs::{MetadataExt, OpenOptionsExt};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

fn keys(a: &Json, allowed: &[&str]) -> Result<()> {
    let Obj(fields) = a else { return Err("The operation requires an object".into()); };
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("The operation accepts {}", allowed.join(", ")));
    }
    Ok(())
}

fn relative_file(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 1024 || path.starts_with('/') || path.split('/').any(|part| part == ".." || part.is_empty()) || path.contains('\\') || path.contains('\0') {
        return Err("Choose a workspace file path".into());
    }
    Ok(())
}

fn root_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err("Choose a new workspace-root output filename".into());
    }
    Ok(())
}

pub(super) fn validate_listing(a: &Json) -> Result<()> {
    keys(a, &["path", "output", "width"])?;
    relative_file(&text(a, "path"))?;
    root_name(&text(a, "output"))?;
    if !["", "32", "64"].contains(&text(a, "width").as_str()) {
        return Err("Width is 32, 64, or omitted when the file declares an x86 image".into());
    }
    Ok(())
}

pub(super) fn validate_regions(a: &Json) -> Result<()> {
    keys(a, &["path", "output"])?;
    relative_file(&text(a, "path"))?;
    root_name(&text(a, "output"))?;
    Ok(())
}

pub(super) fn validate_packets(a: &Json) -> Result<()> {
    keys(a, &["path", "output"])?;
    relative_file(&text(a, "path"))?;
    root_name(&text(a, "output"))?;
    Ok(())
}

struct Staged {
    file: File,
    hasher: incremental_digest::Digest,
    total: u64,
}

impl Write for Staged {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hasher.update(&buf[..n]).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.total += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn begin(root: &Path, name: &str, cancelled: &impl Fn() -> bool) -> Result<Staged> {
    if cancelled() {
        return Err("Cancelled before creating the report".into());
    }
    let path = root.join(name);
    let file = fs::OpenOptions::new().read(true).write(true).create_new(true).custom_flags(0x20000).open(&path).map_err(|e| format!("Cannot create report output: {e}"))?;
    Ok(Staged { file, hasher: incremental_digest::Digest::new(), total: 0 })
}

fn commit(staged: Staged, root: &Path, name: &str, cancelled: &impl Fn() -> bool) -> Result<String> {
    let mut staged = staged;
    staged.file.sync_all().map_err(|e| format!("Report output not verified ({name}): {e}"))?;
    let sha = staged.hasher.finish();
    staged.file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut check = incremental_digest::Digest::new();
    let mut buffer = [0u8; 65536];
    let mut read_total = 0u64;
    loop {
        if cancelled() {
            return Err(format!("Cancelled during readback; new file {name} retained"));
        }
        let n = staged.file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        check.update(&buffer[..n]).map_err(|e| e.to_string())?;
        read_total += n as u64;
    }
    if read_total != staged.total || check.finish() != sha {
        return Err(format!("Report output readback mismatch; new file {name} retained"));
    }
    let held = staged.file.metadata().map_err(|e| e.to_string())?;
    let output = root.join(name);
    let current = fs::symlink_metadata(&output).map_err(|e| e.to_string())?;
    if !current.is_file() || current.dev() != held.dev() || current.ino() != held.ino() {
        return Err(format!("Report output name changed; created file {name} requires inspection"));
    }
    Ok(sha)
}

fn open_subject(path: &Path) -> Result<(File, u64)> {
    let meta = fs::metadata(path).map_err(|e| format!("Cannot open the named subject: {e}"))?;
    if !meta.is_file() {
        return Err("The named subject is not a regular file".into());
    }
    let file = File::open(path).map_err(|e| format!("Cannot open the named subject: {e}"))?;
    Ok((file, meta.len()))
}

fn read_at(file: &mut File, at: u64, n: usize) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(at)).map_err(|e| format!("Cannot read the named subject: {e}"))?;
    let mut buf = vec![0u8; n];
    file.read_exact(&mut buf).map_err(|e| format!("Cannot read the named subject: {e}"))?;
    Ok(buf)
}

enum Declared {
    NotImage,
    Other(String),
    Code { bits64: bool, spans: Vec<(u64, u64, u64)> },
}

fn declared_code(file: &mut File, len: u64) -> Result<Declared> {
    if len < 16 {
        return Ok(Declared::NotImage);
    }
    let head = read_at(file, 0, 64.min(len as usize))?;
    if head.len() >= 2 && &head[..2] == b"MZ" {
        if head.len() < 0x40 {
            return Ok(Declared::Other("DOS header continues past the file".into()));
        }
        let pe = u32::from_le_bytes(head[0x3c..0x40].try_into().unwrap()) as u64;
        if pe.saturating_add(24) > len {
            return Ok(Declared::Other("PE header continues past the file".into()));
        }
        let coff = read_at(file, pe, 24)?;
        if &coff[..4] != b"PE\0\0" {
            return Ok(Declared::NotImage);
        }
        let machine = u16::from_le_bytes(coff[4..6].try_into().unwrap());
        let count = u16::from_le_bytes(coff[6..8].try_into().unwrap()) as u64;
        let optional = u16::from_le_bytes(coff[20..22].try_into().unwrap()) as u64;
        let bits64 = match machine {
            0x8664 => true,
            0x14c => false,
            other => return Ok(Declared::Other(format!("declared machine {other:#06x} is not x86"))),
        };
        let table_at = pe + 24 + optional;
        let table_bytes = count.saturating_mul(40);
        if table_at.saturating_add(table_bytes) > len {
            return Ok(Declared::Other("section table continues past the file".into()));
        }
        let table = read_at(file, table_at, table_bytes as usize)?;
        let mut spans = Vec::new();
        for index in 0..count as usize {
            let at = index * 40;
            let raw_size = u32::from_le_bytes(table[at + 16..at + 20].try_into().unwrap()) as u64;
            let raw = u32::from_le_bytes(table[at + 20..at + 24].try_into().unwrap()) as u64;
            let rva = u32::from_le_bytes(table[at + 12..at + 16].try_into().unwrap()) as u64;
            let flags = u32::from_le_bytes(table[at + 36..at + 40].try_into().unwrap());
            if flags & 0x2000_0000 != 0 && raw_size > 0 {
                spans.push((raw, rva, raw_size));
            }
        }
        return Ok(Declared::Code { bits64, spans });
    }
    if &head[..4] != b"\x7fELF" {
        return Ok(Declared::NotImage);
    }
    if !matches!(head[4], 1 | 2) || head[5] != 1 {
        return Ok(Declared::Other("ELF class or endian is not a little-endian x86 image".into()));
    }
    let wide = head[4] == 2;
    let header_len = if wide { 64 } else { 52 };
    if len < header_len {
        return Ok(Declared::Other("ELF header continues past the file".into()));
    }
    let header = read_at(file, 0, header_len as usize)?;
    let machine = u16::from_le_bytes(header[18..20].try_into().unwrap());
    let bits64 = match machine {
        62 => true,
        3 => false,
        other => return Ok(Declared::Other(format!("declared machine {other} is not x86"))),
    };
    let (phoff, phentsize, phnum) = if wide {
        (
            u64::from_le_bytes(header[32..40].try_into().unwrap()),
            u16::from_le_bytes(header[54..56].try_into().unwrap()) as u64,
            u16::from_le_bytes(header[56..58].try_into().unwrap()) as u64,
        )
    } else {
        (
            u32::from_le_bytes(header[28..32].try_into().unwrap()) as u64,
            u16::from_le_bytes(header[42..44].try_into().unwrap()) as u64,
            u16::from_le_bytes(header[44..46].try_into().unwrap()) as u64,
        )
    };
    if phnum > 0 && phoff.saturating_add(phentsize.saturating_mul(phnum)) > len {
        return Ok(Declared::Other("program header table continues past the file".into()));
    }
    let mut spans = Vec::new();
    if phnum > 0 && phentsize >= if wide { 56 } else { 32 } {
        let table = read_at(file, phoff, (phentsize * phnum) as usize)?;
        for index in 0..phnum as usize {
            let at = index * phentsize as usize;
            let kind = u32::from_le_bytes(table[at..at + 4].try_into().unwrap());
            let (offset, address, size, flags) = if wide {
                (
                    u64::from_le_bytes(table[at + 8..at + 16].try_into().unwrap()),
                    u64::from_le_bytes(table[at + 16..at + 24].try_into().unwrap()),
                    u64::from_le_bytes(table[at + 32..at + 40].try_into().unwrap()),
                    u32::from_le_bytes(table[at + 4..at + 8].try_into().unwrap()),
                )
            } else {
                (
                    u32::from_le_bytes(table[at + 4..at + 8].try_into().unwrap()) as u64,
                    u32::from_le_bytes(table[at + 8..at + 12].try_into().unwrap()) as u64,
                    u32::from_le_bytes(table[at + 16..at + 20].try_into().unwrap()) as u64,
                    u32::from_le_bytes(table[at + 24..at + 28].try_into().unwrap()),
                )
            };
            if kind == 1 && flags & 1 != 0 && size > 0 {
                spans.push((offset, address, size));
            }
        }
    }
    Ok(Declared::Code { bits64, spans })
}

fn write_span(source: &mut File, out: &mut Staged, offset: u64, address: u64, size: u64, file_len: u64, bits64: bool, cancelled: &impl Fn() -> bool) -> Result<u64> {
    if offset.saturating_add(size) > file_len {
        writeln!(out, "executable mapping at file {offset:#x} length {size} continues past the file").map_err(|e| e.to_string())?;
        return Ok(0);
    }
    writeln!(out, "executable file [{offset:#x}, {:#x}) virtual {address:#x}", offset + size).map_err(|e| e.to_string())?;
    let mut pending = Vec::new();
    let mut pos = 0u64;
    let mut virtual_at = address;
    let mut instructions = 0u64;
    while pos < size || !pending.is_empty() {
        if cancelled() {
            return Err("Listing cancelled; incomplete new file may remain".into());
        }
        if pending.len() < 15 && pos < size {
            let room = ((size - pos) as usize).min(65536);
            let chunk = read_at(source, offset + pos, room)?;
            pos += chunk.len() as u64;
            pending.extend_from_slice(&chunk);
            continue;
        }
        let more = pos < size;
        let mut transcript = super::x86_words::transcribe(&pending, virtual_at, bits64, more, out, &mut || cancelled())?;
        if transcript.consumed == 0 && more && transcript.stopped.is_none() {
            transcript = super::x86_words::transcribe(&pending, virtual_at, bits64, false, out, &mut || cancelled())?;
        }
        if transcript.consumed == 0 {
            break;
        }
        pending.drain(..transcript.consumed);
        virtual_at += transcript.consumed as u64;
        instructions += transcript.instructions;
        if transcript.stopped.is_some() {
            break;
        }
    }
    Ok(instructions)
}

pub(super) fn run_listing(root: &Path, subject: &Path, args: &Json, cancelled: &impl Fn() -> bool) -> Result<String> {
    validate_listing(args)?;
    let name = text(args, "output");
    let (mut source, len) = open_subject(subject)?;
    let width = text(args, "width");
    let (bits64, spans) = if width.is_empty() {
        match declared_code(&mut source, len)? {
            Declared::NotImage => return Err("This file does not declare an x86 image. Pass width 32 or 64 to list its bytes as instructions.".into()),
            Declared::Other(reason) => return Err(reason),
            Declared::Code { bits64, spans } => (bits64, spans),
        }
    } else {
        (width == "64", vec![(0, 0, len)])
    };
    let mut out = begin(root, &name, cancelled)?;
    writeln!(out, "{}", if bits64 { "x86-64 instruction listing" } else { "x86 instruction listing" }).map_err(|e| e.to_string())?;
    let mut instructions = 0u64;
    if spans.is_empty() {
        writeln!(out, "No declared executable mapping was available to list.").map_err(|e| e.to_string())?;
    }
    for (offset, address, size) in spans {
        instructions += write_span(&mut source, &mut out, offset, address, size, len, bits64, cancelled)?;
    }
    writeln!(out, "instructions {instructions}").map_err(|e| e.to_string())?;
    let sha = commit(out, root, &name, cancelled)?;
    Ok(format!("Instruction listing written to {name} ({instructions} instructions, SHA-256 {sha}). Existing files were not replaced."))
}

struct Site {
    offset: u64,
    name: &'static str,
}

fn scan_marks(path: &Path, len: u64, cancelled: &impl Fn() -> bool) -> Result<Vec<Site>> {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let chunk = 1usize << 20;
    let overlap = 64usize;
    let mut sites = Vec::new();
    let mut mz = Vec::new();
    let mut offset = 0u64;
    while offset < len {
        if cancelled() {
            return Err("cancelled".into());
        }
        let mut windows = Vec::new();
        for _ in 0..cores {
            if offset >= len {
                break;
            }
            let start = offset;
            let take = ((len - start) as usize).min(chunk + overlap);
            let mut file = File::open(path).map_err(|e| format!("Cannot open the named subject: {e}"))?;
            let buf = read_at(&mut file, start, take)?;
            let report = if start + chunk as u64 >= len { buf.len() } else { chunk.min(buf.len()) };
            windows.push((buf, start, report));
            offset = offset.saturating_add(chunk as u64);
        }
        let found = std::thread::scope(|scope| {
            let mut jobs = Vec::new();
            for (buf, start, report) in &windows {
                jobs.push(scope.spawn(move || {
                    let mut hits = super::image_marks::fixed_hits(buf, *start);
                    hits.retain(|hit| hit.offset >= *start && hit.offset - *start < *report as u64);
                    let mut dos = Vec::new();
                    for index in 0..*report {
                        if let Some(lfanew) = super::image_marks::mz_lfanew(buf, index) {
                            dos.push((start + index as u64, lfanew));
                        }
                    }
                    (hits, dos)
                }));
            }
            let mut hits = Vec::new();
            let mut dos = Vec::new();
            for job in jobs {
                let (part, part_dos) = match job.join() {
                    Ok(value) => value,
                    Err(_) => return Err(String::from("Mark scan worker failed")),
                };
                hits.extend(part);
                dos.extend(part_dos);
            }
            Ok((hits, dos))
        })?;
        for hit in found.0 {
            sites.push(Site { offset: hit.offset, name: hit.name });
        }
        mz.extend(found.1);
    }
    let mut file = File::open(path).map_err(|e| format!("Cannot open the named subject: {e}"))?;
    for (at, lfanew) in mz {
        let sig_at = at.saturating_add(lfanew as u64);
        if sig_at.saturating_add(4) > len {
            continue;
        }
        let sig = read_at(&mut file, sig_at, 4)?;
        if sig.as_slice() == b"PE\0\0" {
            sites.push(Site { offset: at, name: "pe image" });
        }
    }
    sites.sort_by_key(|site| (site.offset, site.name));
    sites.dedup_by(|a, b| a.offset == b.offset && a.name == b.name);
    Ok(sites)
}

fn member_length(file: &mut File, offset: u64, len: u64, name: &str) -> Result<Option<u64>> {
    if name == "png" {
        return png_length(file, offset, len);
    }
    let mut prefix = Vec::new();
    loop {
        match super::image_marks::declared_length(&prefix, name) {
            super::image_marks::Measure::Bytes(n) => {
                return Ok(if offset.saturating_add(n) <= len { Some(n) } else { None });
            }
            super::image_marks::Measure::Unknown => return Ok(None),
            super::image_marks::Measure::Need(n) => {
                if n <= prefix.len() || offset.saturating_add(n as u64) > len {
                    return Ok(None);
                }
                let more = read_at(file, offset + prefix.len() as u64, n - prefix.len())?;
                prefix.extend_from_slice(&more);
            }
        }
    }
}

fn png_length(file: &mut File, offset: u64, len: u64) -> Result<Option<u64>> {
    if offset.saturating_add(8) > len {
        return Ok(None);
    }
    let sig = read_at(file, offset, 8)?;
    if sig.as_slice() != b"\x89PNG\r\n\x1a\n" {
        return Ok(None);
    }
    let mut at = offset + 8;
    loop {
        if at.saturating_add(12) > len {
            return Ok(None);
        }
        let head = read_at(file, at, 8)?;
        let chunk = u32::from_be_bytes(head[..4].try_into().unwrap()) as u64;
        let kind = &head[4..8];
        let next = match at.checked_add(12).and_then(|n| n.checked_add(chunk)) {
            Some(n) if n <= len => n,
            _ => return Ok(None),
        };
        at = next;
        if kind == b"IEND" {
            return Ok(Some(at - offset));
        }
    }
}

fn copy_range(source: &mut File, offset: u64, size: u64, root: &Path, name: &str, cancelled: &impl Fn() -> bool) -> Result<String> {
    let mut out = begin(root, name, cancelled)?;
    let mut left = size;
    let mut at = offset;
    let mut buffer = [0u8; 65536];
    while left > 0 {
        if cancelled() {
            return Err(format!("Cancelled while copying a member; incomplete new file {name} may remain"));
        }
        let room = (left as usize).min(buffer.len());
        source.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        source.read_exact(&mut buffer[..room]).map_err(|e| e.to_string())?;
        out.write_all(&buffer[..room]).map_err(|e| e.to_string())?;
        at += room as u64;
        left -= room as u64;
    }
    let sha = commit(out, root, name, cancelled)?;
    Ok(sha)
}

pub(super) fn run_regions(root: &Path, subject: &Path, args: &Json, cancelled: &impl Fn() -> bool) -> Result<String> {
    validate_regions(args)?;
    let name = text(args, "output");
    let (_source, len) = open_subject(subject)?;
    let sites = scan_marks(subject, len, cancelled).map_err(|error| if error == "cancelled" { "Cancelled while scanning marks".into() } else { error })?;
    if let Some(last) = sites.len().checked_sub(1) {
        root_name(&format!("{name}.{last:04}"))?;
    }
    let mut out = begin(root, &name, cancelled)?;
    writeln!(out, "container marks: {}", sites.len()).map_err(|e| e.to_string())?;
    let mut source = File::open(subject).map_err(|e| format!("Cannot open the named subject: {e}"))?;
    let mut copied = 0u64;
    for (index, site) in sites.iter().enumerate() {
        if cancelled() {
            return Err(format!("Cancelled while writing marks; incomplete new file {name} may remain"));
        }
        match member_length(&mut source, site.offset, len, site.name)? {
            Some(size) => {
                let member = format!("{name}.{index:04}");
                root_name(&member)?;
                let sha = copy_range(&mut source, site.offset, size, root, &member, cancelled)?;
                copied += 1;
                writeln!(out, "{:#x}  {}  declared {size} bytes  extracted {member}  SHA-256 {sha}", site.offset, site.name).map_err(|e| e.to_string())?;
            }
            None => {
                writeln!(out, "{:#x}  {}  member length was not established", site.offset, site.name).map_err(|e| e.to_string())?;
            }
        }
    }
    if sites.is_empty() {
        writeln!(out, "No container mark was found. Unmarked bytes stay unclassified.").map_err(|e| e.to_string())?;
    }
    let sha = commit(out, root, &name, cancelled)?;
    Ok(format!("Container marks written to {name} ({} marks, {copied} members whose headers declared a length, SHA-256 {sha}). Existing files were not replaced.", sites.len()))
}

pub(super) fn run_packets(root: &Path, subject: &Path, args: &Json, cancelled: &impl Fn() -> bool) -> Result<String> {
    validate_packets(args)?;
    let name = text(args, "output");
    let (mut source, _len) = open_subject(subject)?;
    let mut out = begin(root, &name, cancelled)?;
    let count = super::packet_words::transcribe(&mut source, &mut out, &mut || cancelled()).map_err(|e| format!("Saved capture was not decoded: {e}"))?;
    writeln!(out, "packets: {count}").map_err(|e| e.to_string())?;
    let sha = commit(out, root, &name, cancelled)?;
    Ok(format!("Packet report written to {name} ({count} packets, SHA-256 {sha}). Existing files were not replaced."))
}

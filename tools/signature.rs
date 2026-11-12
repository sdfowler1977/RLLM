// Replacement for tools/signature.py. Run the native binary from
// tools/build-tools.sh. Do not run the Python file.
//
// One positional APK path. Help is on stdout. A missing path or any other
// bad argument prints usage on stderr and exits 2. Each refusal keeps the
// Python exception line and exits 1.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

struct PyErr {
    class: &'static str,
    message: String,
}

impl PyErr {
    fn value(message: &str) -> Self {
        Self { class: "ValueError", message: message.to_string() }
    }
    fn bad(message: &str) -> Self {
        Self { class: "BadZipFile", message: message.to_string() }
    }
    fn zlib(message: String) -> Self {
        Self { class: "zlib.error", message }
    }
    fn runtime(message: &str) -> Self {
        Self { class: "RuntimeError", message: message.to_string() }
    }
    fn eof() -> Self {
        Self { class: "EOFError", message: String::new() }
    }
    fn uncaught(class: &'static str, message: String) -> Self {
        Self { class, message }
    }
    fn text(&self) -> String {
        self.message.clone()
    }
    fn traceback_line(&self) -> String {
        let message = self.text();
        if message.is_empty() {
            self.class.to_string()
        } else {
            format!("{}: {}", self.class, message)
        }
    }
}

fn py_repr(text: &str) -> String {
    let quote = if !text.contains('\'') || text.contains('"') { '\'' } else { '"' };
    let mut out = String::new();
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' if quote == '\'' => out.push_str("\\'"),
            '"' if quote == '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn io_err(err: &std::io::Error, path: &str) -> PyErr {
    let code = err.raw_os_error().unwrap_or(0);
    let (class, text) = match code {
        2 => ("FileNotFoundError", "No such file or directory"),
        13 => ("PermissionError", "Permission denied"),
        17 => ("FileExistsError", "File exists"),
        20 => ("NotADirectoryError", "Not a directory"),
        21 => ("IsADirectoryError", "Is a directory"),
        _ => ("OSError", "Input/output error"),
    };
    let message = if code == 0 {
        err.to_string()
    } else {
        format!("[Errno {code}] {text}: {}", py_repr(path))
    };
    PyErr { class, message }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

struct Bits<'a> {
    bytes: &'a [u8],
    index: usize,
    buf: u64,
    have: u32,
}

impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, index: 0, buf: 0, have: 0 }
    }
    fn take(&mut self, count: usize) -> Result<usize, String> {
        if count > 16 {
            return Err("Truncated compressed member".into());
        }
        while (self.have as usize) < count {
            if self.index >= self.bytes.len() {
                return Err("Truncated compressed member".into());
            }
            self.buf |= (self.bytes[self.index] as u64) << self.have;
            self.index += 1;
            self.have += 8;
        }
        let mask = if count == 0 { 0 } else { (1u64 << count) - 1 };
        let value = (self.buf & mask) as usize;
        self.buf >>= count;
        self.have -= count as u32;
        Ok(value)
    }
    fn align(&mut self) {
        let extra = self.have % 8;
        if extra != 0 {
            self.buf >>= extra;
            self.have -= extra;
        }
    }
    fn consumed(&self) -> usize {
        let at = self.index * 8 - self.have as usize;
        (at + 7) / 8
    }
    fn read_stored(&mut self, length: usize) -> Result<&'a [u8], String> {
        self.align();
        if self.have != 0 {
            return Err("Truncated compressed stored block".into());
        }
        let start = self.index;
        let end = start.checked_add(length).ok_or("Truncated compressed stored block")?;
        if end > self.bytes.len() {
            return Err("Truncated compressed stored block".into());
        }
        self.index = end;
        Ok(&self.bytes[start..end])
    }
}

struct Codes {
    counts: [usize; 16],
    first: [usize; 16],
    offsets: [usize; 16],
    symbols: Vec<usize>,
}

impl Codes {
    fn new(lengths: &[usize], single: bool, empty: bool) -> Result<Self, String> {
        let mut counts = [0usize; 16];
        for &length in lengths {
            if length > 15 {
                return Err("Invalid compressed code length".into());
            }
            if length != 0 {
                counts[length] += 1;
            }
        }
        let size: usize = counts.iter().sum();
        let mut left = 1isize;
        for &count in &counts[1..] {
            left = (left << 1) - count as isize;
            if left < 0 {
                return Err("Oversubscribed compressed codes".into());
            }
        }
        if left != 0 && !(single && size == 1 && counts[1] == 1) && !(empty && size == 0) {
            return Err("Incomplete compressed codes".into());
        }
        let mut first = [0usize; 16];
        let mut offsets = [0usize; 16];
        for length in 1..16 {
            first[length] = (first[length - 1] + counts[length - 1]) << 1;
            offsets[length] = offsets[length - 1] + counts[length - 1];
        }
        let mut symbols = vec![0; size];
        let mut next = offsets;
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[next[length]] = symbol;
                next[length] += 1;
            }
        }
        Ok(Self { counts, first, offsets, symbols })
    }
    fn symbol(&self, bits: &mut Bits<'_>) -> Result<usize, String> {
        let mut code = 0usize;
        for length in 1..16 {
            code = (code << 1) | bits.take(1)?;
            if code >= self.first[length] {
                let index = code - self.first[length];
                if index < self.counts[length] {
                    return Ok(self.symbols[self.offsets[length] + index]);
                }
            }
        }
        Err("Invalid compressed symbol".into())
    }
}

fn tables(bits: &mut Bits<'_>) -> Result<(Codes, Codes), String> {
    let literals = bits.take(5)? + 257;
    let distances = bits.take(5)? + 1;
    if literals > 286 {
        return Err("Reserved compressed literal count".into());
    }
    let count = bits.take(4)? + 4;
    let order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut lengths = [0usize; 19];
    for &symbol in &order[..count] {
        lengths[symbol] = bits.take(3)?;
    }
    let code = Codes::new(&lengths, false, false)?;
    let mut lengths = Vec::with_capacity(literals + distances);
    while lengths.len() < literals + distances {
        let symbol = code.symbol(bits)?;
        let (value, repeat) = match symbol {
            0..=15 => (symbol, 1),
            16 => (lengths.last().copied().ok_or("Compressed repeat has no previous length")?, bits.take(2)? + 3),
            17 => (0, bits.take(3)? + 3),
            18 => (0, bits.take(7)? + 11),
            _ => return Err("Invalid compressed length symbol".into()),
        };
        if lengths.len() + repeat > literals + distances {
            return Err("Compressed length repeat overflow".into());
        }
        lengths.resize(lengths.len() + repeat, value);
    }
    if lengths[256] == 0 {
        return Err("Compressed member lacks end symbol".into());
    }
    Ok((Codes::new(&lengths[..literals], true, false)?, Codes::new(&lengths[literals..], true, true)?))
}

fn inflate(bytes: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    const LIMIT: usize = 64 * 1024 * 1024;
    if expected > LIMIT || bytes.len() > LIMIT {
        return Err("Compressed member exceeds resource limit".into());
    }
    let mut bits = Bits::new(bytes);
    let mut out = Vec::new();
    out.try_reserve_exact(expected).map_err(|_| "Compressed member allocation failed")?;
    let mut literal_lengths = [8usize; 288];
    literal_lengths[144..256].fill(9);
    literal_lengths[256..280].fill(7);
    let fixed = (Codes::new(&literal_lengths, false, false)?, Codes::new(&[5usize; 32], false, false)?);
    let mut blocks = 0usize;
    loop {
        blocks += 1;
        if blocks > 1_000_000 {
            return Err("Compressed member exceeds block-processing limit".into());
        }
        let final_block = bits.take(1)? != 0;
        match bits.take(2)? {
            0 => {
                let length = {
                    bits.align();
                    bits.take(16)?
                };
                if (length ^ bits.take(16)?) != 0xffff {
                    return Err("Compressed stored length mismatch".into());
                }
                if out.len() + length > expected {
                    return Err("Compressed member exceeds declared size".into());
                }
                out.extend_from_slice(bits.read_stored(length)?);
            }
            kind @ (1 | 2) => {
                let dynamic;
                let (literal, distance) = if kind == 1 {
                    (&fixed.0, &fixed.1)
                } else {
                    dynamic = tables(&mut bits)?;
                    (&dynamic.0, &dynamic.1)
                };
                loop {
                    match literal.symbol(&mut bits)? {
                        symbol @ 0..=255 => {
                            if out.len() == expected {
                                return Err("Compressed member exceeds declared size".into());
                            }
                            out.push(symbol as u8);
                        }
                        256 => break,
                        symbol @ 257..=285 => {
                            const BASE: [usize; 29] = [
                                3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
                                115, 131, 163, 195, 227, 258,
                            ];
                            const EXTRA: [usize; 29] = [
                                0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
                            ];
                            let length = BASE[symbol - 257] + bits.take(EXTRA[symbol - 257])?;
                            let symbol = distance.symbol(&mut bits)?;
                            if symbol > 29 {
                                return Err("Reserved compressed distance".into());
                            }
                            let extra = if symbol < 4 { 0 } else { symbol / 2 - 1 };
                            let base = if symbol < 4 { symbol + 1 } else { (2 + symbol % 2) * (1 << extra) + 1 };
                            let offset = base + bits.take(extra)?;
                            if offset == 0 || offset > out.len() || out.len() + length > expected {
                                return Err("Compressed back-reference outside member bounds".into());
                            }
                            for _ in 0..length {
                                let byte = out[out.len() - offset];
                                out.push(byte);
                            }
                        }
                        _ => return Err("Reserved compressed literal".into()),
                    }
                }
            }
            _ => return Err("Reserved compressed block type".into()),
        }
        if final_block {
            break;
        }
    }
    if out.len() != expected || bits.consumed() != bytes.len() {
        return Err("Compressed member size or trailing data mismatch".into());
    }
    Ok(out)
}

fn cp437(bytes: &[u8]) -> String {
    const HIGH: [char; 128] = [
        'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô', 'ö', 'ò',
        'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½',
        '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴',
        '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫',
        '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ',
        'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{00a0}',
    ];
    let mut out = String::new();
    for &byte in bytes {
        if byte < 128 {
            out.push(byte as char);
        } else {
            out.push(HIGH[(byte - 128) as usize]);
        }
    }
    out
}

fn decode_name(bytes: &[u8], flags: u16) -> Result<String, PyErr> {
    let cut = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..cut];
    if flags & 0x800 != 0 {
        match std::str::from_utf8(bytes) {
            Ok(text) => Ok(text.to_string()),
            Err(_) => Err(PyErr::uncaught("UnicodeDecodeError", "zip filename is not valid UTF-8".into())),
        }
    } else {
        Ok(cp437(bytes))
    }
}

struct Member {
    name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_offset: u64,
}

struct Archive {
    data: Vec<u8>,
    members: Vec<Member>,
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, PyErr> {
    let bytes = data.get(offset..offset + 2).ok_or_else(|| PyErr::bad("Truncated central directory"))?;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, PyErr> {
    let bytes = data.get(offset..offset + 4).ok_or_else(|| PyErr::bad("Truncated central directory"))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

impl Archive {
    fn open(path: &Path) -> Result<Self, PyErr> {
        let data = fs::read(path).map_err(|err| io_err(&err, &path.display().to_string()))?;
        Self::parse(data)
    }
    fn parse(data: Vec<u8>) -> Result<Self, PyErr> {
        if data.len() < 22 {
            return Err(PyErr::bad("File is not a zip file"));
        }
        let max_back = data.len().min(22 + 65535);
        let start = data.len() - max_back;
        let mut eocd = None;
        let mut index = data.len() - 22;
        loop {
            if data[index..index + 4] == [0x50, 0x4b, 0x05, 0x06] {
                let comment = u16::from_le_bytes(data[index + 20..index + 22].try_into().unwrap()) as usize;
                if index + 22 + comment == data.len() {
                    eocd = Some(index);
                    break;
                }
            }
            if index == start {
                break;
            }
            index -= 1;
        }
        let eocd = eocd.ok_or_else(|| PyErr::bad("File is not a zip file"))?;
        let disk = u16_at(&data, eocd + 4)?;
        let disk_cd = u16_at(&data, eocd + 6)?;
        if disk != 0 || disk_cd != 0 {
            return Err(PyErr::bad("zipfiles that span multiple disks are not supported"));
        }
        let entries = u16_at(&data, eocd + 8)? as usize;
        let entries_total = u16_at(&data, eocd + 10)? as usize;
        if entries != entries_total {
            return Err(PyErr::bad("zipfiles that span multiple disks are not supported"));
        }
        let size_cd = u32_at(&data, eocd + 12)? as u64;
        let offset_cd = u32_at(&data, eocd + 16)? as u64;
        if size_cd == 0xffff_ffff || offset_cd == 0xffff_ffff {
            return Err(PyErr::bad("Truncated central directory"));
        }
        let location = eocd as u64;
        let concat = location as i64 - size_cd as i64 - offset_cd as i64;
        if concat < 0 {
            return Err(PyErr::bad("Bad offset for central directory"));
        }
        let start_dir = offset_cd + concat as u64;
        let end_dir = start_dir + size_cd;
        if end_dir > data.len() as u64 {
            return Err(PyErr::bad("Truncated central directory"));
        }
        let mut cursor = start_dir as usize;
        let end = end_dir as usize;
        let mut members = Vec::with_capacity(entries);
        while cursor < end {
            if cursor + 46 > end {
                return Err(PyErr::bad("Truncated central directory"));
            }
            if data[cursor..cursor + 4] != [0x50, 0x4b, 0x01, 0x02] {
                return Err(PyErr::bad("Bad magic number for central directory"));
            }
            let flags = u16_at(&data, cursor + 8)?;
            let method = u16_at(&data, cursor + 10)?;
            let crc = u32_at(&data, cursor + 16)?;
            let compressed_size = u32_at(&data, cursor + 20)? as u64;
            let uncompressed_size = u32_at(&data, cursor + 24)? as u64;
            let name_len = u16_at(&data, cursor + 28)? as usize;
            let extra_len = u16_at(&data, cursor + 30)? as usize;
            let comment_len = u16_at(&data, cursor + 32)? as usize;
            let local_offset = u32_at(&data, cursor + 42)? as u64;
            if compressed_size == 0xffff_ffff || uncompressed_size == 0xffff_ffff || local_offset == 0xffff_ffff {
                return Err(PyErr::bad("Truncated central directory"));
            }
            let name_at = cursor + 46;
            let name_end = name_at + name_len;
            let extra_end = name_end + extra_len;
            let comment_end = extra_end + comment_len;
            if comment_end > end {
                return Err(PyErr::bad("Truncated central directory"));
            }
            let name = decode_name(&data[name_at..name_end], flags)?;
            members.push(Member {
                name,
                method,
                flags,
                crc,
                compressed_size,
                uncompressed_size,
                local_offset: local_offset + concat as u64,
            });
            cursor = comment_end;
        }
        if cursor != end {
            return Err(PyErr::bad("Truncated central directory"));
        }
        Ok(Self { data, members })
    }
    fn read_member(&self, member: &Member) -> Result<Vec<u8>, PyErr> {
        if member.flags & 0x01 != 0 {
            return Err(PyErr::runtime(&format!(
                "File {} is encrypted, password required for extraction",
                py_repr(&member.name)
            )));
        }
        if member.flags & 0x20 != 0 {
            return Err(PyErr::uncaught("NotImplementedError", "compressed patched data (flag bit 5)".into()));
        }
        if member.flags & 0x40 != 0 {
            return Err(PyErr::uncaught("NotImplementedError", "strong encryption (flag bit 6)".into()));
        }
        let offset = member.local_offset as usize;
        if offset + 30 > self.data.len() {
            return Err(PyErr::bad("Truncated file header"));
        }
        if self.data[offset..offset + 4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(PyErr::bad("Bad magic number for file header"));
        }
        let local_flags = u16_at(&self.data, offset + 6)?;
        let name_len = u16_at(&self.data, offset + 26)? as usize;
        let extra_len = u16_at(&self.data, offset + 28)? as usize;
        let name_at = offset + 30;
        let name_end = name_at + name_len;
        if name_end + extra_len > self.data.len() {
            return Err(PyErr::bad("Truncated file header"));
        }
        let local_name = decode_name(&self.data[name_at..name_end], local_flags)?;
        if local_name != member.name {
            return Err(PyErr::bad(&format!(
                "File name in directory {} and header {} differ.",
                py_repr(&member.name),
                py_repr(&local_name)
            )));
        }
        let data_at = name_end + extra_len;
        let data_end = data_at as u64 + member.compressed_size;
        if data_end > self.data.len() as u64 {
            return Err(PyErr::eof());
        }
        let compressed = &self.data[data_at..data_end as usize];
        let plain = match member.method {
            0 => {
                if compressed.len() as u64 != member.uncompressed_size && member.compressed_size != member.uncompressed_size {
                    return Err(PyErr::value("APK payload length differs"));
                }
                compressed.to_vec()
            }
            8 => inflate(compressed, member.uncompressed_size as usize).map_err(PyErr::zlib)?,
            other => {
                return Err(PyErr::uncaught(
                    "NotImplementedError",
                    format!("compression type {other}"),
                ));
            }
        };
        if plain.len() as u64 != member.uncompressed_size {
            return Err(PyErr::value("APK payload length differs"));
        }
        if crc32(&plain) != member.crc {
            return Err(PyErr::bad(&format!("Bad CRC-32 for file {}", py_repr(&member.name))));
        }
        Ok(plain)
    }
}

const CERTIFICATE: &str = "ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5";
const V2_LINE: &str = "Verified using v2 scheme (APK Signature Scheme v2): true";
const V3_LINE: &str = "Verified using v3 scheme (APK Signature Scheme v3): true";
const USAGE: &str = "usage: signature.py [-h] apk";
const HELP: &str = "\
usage: signature.py [-h] apk

positional arguments:
  apk

options:
  -h, --help  show this help message and exit
";

impl Archive {
    fn copy_member(&self, index: usize) -> Member {
        let source = &self.members[index];
        Member {
            name: source.name.clone(),
            method: source.method,
            flags: source.flags,
            crc: source.crc,
            compressed_size: source.compressed_size,
            uncompressed_size: source.uncompressed_size,
            local_offset: source.local_offset,
        }
    }
    fn testzip(&self) -> Result<Option<String>, PyErr> {
        for index in 0..self.members.len() {
            let member = self.copy_member(index);
            match self.read_member(&member) {
                Ok(_) => {}
                Err(err) if err.class == "BadZipFile" => return Ok(Some(member.name)),
                Err(err) => return Err(err),
            }
        }
        Ok(None)
    }
}

fn env_key(name: &str) -> Result<String, PyErr> {
    env::var(name).map_err(|_| PyErr { class: "KeyError", message: py_repr(name) })
}

fn lines_of(text: &str) -> Vec<&str> {
    text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect()
}

fn signer_digests(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for line in lines_of(text) {
        let Some(rest) = line.strip_prefix("Signer #") else { continue };
        let Some((number, rest)) = rest.split_once(' ') else { continue };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Some(digest) = rest.strip_prefix("certificate SHA-256 digest: ") else { continue };
        if digest.len() == 64 && digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
            found.push(digest);
        }
    }
    found
}

fn classify(name: &str, stdout: &str) -> Result<(), PyErr> {
    if !stdout.contains(V2_LINE) || !stdout.contains(V3_LINE) {
        return Err(PyErr::value(&format!("Delivery APK requires verified v2 and v3 signatures: {name}")));
    }
    let certificates = signer_digests(stdout);
    let one_signer = lines_of(stdout).iter().any(|line| *line == "Number of signers: 1");
    if certificates != [CERTIFICATE] || !one_signer {
        return Err(PyErr::value(&format!(
            "Delivery APK signing identity differs from the retained certificate: {name}"
        )));
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|text| text.to_string_lossy().into_owned()).unwrap_or_else(|| "apk".into())
}

fn verify(path: &Path) -> Result<(), PyErr> {
    let jdk = env_key("CLIENT_JDK")?;
    let inherited = env_key("PATH")?;
    let tools = env_key("CLIENT_TOOLS")?;
    let program = PathBuf::from(&tools).join("apksigner");
    let name = file_name(path);
    let output = Command::new(&program)
        .args(["verify", "--verbose", "--print-certs"])
        .arg(path)
        .env("PATH", format!("{jdk}/bin:{inherited}"))
        .output()
        .map_err(|err| io_err(&err, &program.display().to_string()))?;
    if !output.status.success() {
        return Err(PyErr::value(&format!("APK signature verification failed: {name}")));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| {
        PyErr::uncaught("UnicodeDecodeError", "'utf-8' codec can't decode apksigner output".into())
    })?;
    classify(&name, &text)?;
    archive_integrity(path)
}

fn archive_integrity(path: &Path) -> Result<(), PyErr> {
    let name = file_name(path);
    let archive = Archive::open(path)?;
    if archive.testzip()?.is_some() {
        return Err(PyErr::value(&format!("APK archive integrity verification failed: {name}")));
    }
    Ok(())
}

fn usage_error(message: &str) -> i32 {
    eprintln!("{USAGE}");
    eprintln!("signature.py: error: {message}");
    2
}

fn looks_like_option(token: &str) -> bool {
    token.starts_with('-') && token != "-"
}

fn parse_cli(args: &[String]) -> Result<String, i32> {
    let mut positionals = Vec::new();
    let mut unrecognized = Vec::new();
    let mut index = 0;
    let mut options = true;
    while index < args.len() {
        let arg = &args[index];
        if options && arg == "--" {
            options = false;
            index += 1;
            continue;
        }
        if options && (arg == "-h" || arg == "--help") {
            print!("{HELP}");
            let _ = io::stdout().flush();
            return Err(0);
        }
        if options && looks_like_option(arg) {
            unrecognized.push(arg.clone());
            index += 1;
            continue;
        }
        positionals.push(arg.clone());
        index += 1;
    }
    if positionals.is_empty() {
        return Err(usage_error("the following arguments are required: apk"));
    }
    if positionals.len() > 1 {
        unrecognized.extend(positionals[1..].iter().cloned());
    }
    if !unrecognized.is_empty() {
        return Err(usage_error(&format!("unrecognized arguments: {}", unrecognized.join(" "))));
    }
    Ok(positionals.remove(0))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match parse_cli(&args) {
        Err(code) => code,
        Ok(path) => match verify(Path::new(&path)) {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("{}", err.traceback_line());
                1
            }
        },
    };
    std::process::exit(code);
}

#[cfg(test)]
fn good_certificate_text() -> String {
    format!(
        "Verified using v1 scheme (JAR signing): false\n{V2_LINE}\n{V3_LINE}\nNumber of signers: 1\nSigner #1 certificate SHA-256 digest: {CERTIFICATE}\n"
    )
}

#[cfg(test)]
fn stored_zip(name: &str, data: &[u8], crc: u32) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let mut local = Vec::new();
    local.extend_from_slice(&0x04034b50u32.to_le_bytes());
    local.extend_from_slice(&20u16.to_le_bytes());
    local.extend_from_slice(&0u16.to_le_bytes());
    local.extend_from_slice(&0u16.to_le_bytes());
    local.extend_from_slice(&0u16.to_le_bytes());
    local.extend_from_slice(&0u16.to_le_bytes());
    local.extend_from_slice(&crc.to_le_bytes());
    local.extend_from_slice(&(data.len() as u32).to_le_bytes());
    local.extend_from_slice(&(data.len() as u32).to_le_bytes());
    local.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    local.extend_from_slice(&0u16.to_le_bytes());
    local.extend_from_slice(name_bytes);
    local.extend_from_slice(data);
    let local_len = local.len() as u32;
    let mut central = Vec::new();
    central.extend_from_slice(&0x02014b50u32.to_le_bytes());
    central.extend_from_slice(&0x0314u16.to_le_bytes());
    central.extend_from_slice(&20u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&crc.to_le_bytes());
    central.extend_from_slice(&(data.len() as u32).to_le_bytes());
    central.extend_from_slice(&(data.len() as u32).to_le_bytes());
    central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u32.to_le_bytes());
    central.extend_from_slice(&0u32.to_le_bytes());
    central.extend_from_slice(name_bytes);
    let central_len = central.len() as u32;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&central_len.to_le_bytes());
    out.extend_from_slice(&local_len.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn missing_v2_or_v3_line() {
        let v3_only = format!("{V3_LINE}\nNumber of signers: 1\nSigner #1 certificate SHA-256 digest: {CERTIFICATE}\n");
        let err = classify("app.apk", &v3_only).unwrap_err();
        assert_eq!(err.traceback_line(), "ValueError: Delivery APK requires verified v2 and v3 signatures: app.apk");
        let v2_only = format!("{V2_LINE}\nNumber of signers: 1\nSigner #1 certificate SHA-256 digest: {CERTIFICATE}\n");
        let err = classify("app.apk", &v2_only).unwrap_err();
        assert_eq!(err.traceback_line(), "ValueError: Delivery APK requires verified v2 and v3 signatures: app.apk");
    }

    #[test]
    fn wrong_certificate_digest() {
        let text = format!("{V2_LINE}\n{V3_LINE}\nNumber of signers: 1\nSigner #1 certificate SHA-256 digest: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n");
        let err = classify("app.apk", &text).unwrap_err();
        assert_eq!(
            err.traceback_line(),
            "ValueError: Delivery APK signing identity differs from the retained certificate: app.apk"
        );
        assert!(classify("app.apk", &good_certificate_text()).is_ok());
    }

    #[test]
    fn more_than_one_signer() {
        let text = format!(
            "{V2_LINE}\n{V3_LINE}\nNumber of signers: 2\nSigner #1 certificate SHA-256 digest: {CERTIFICATE}\nSigner #2 certificate SHA-256 digest: {CERTIFICATE}\n"
        );
        let err = classify("app.apk", &text).unwrap_err();
        assert_eq!(
            err.traceback_line(),
            "ValueError: Delivery APK signing identity differs from the retained certificate: app.apk"
        );
    }

    #[test]
    fn unparsable_signer_output() {
        let text = format!("{V2_LINE}\n{V3_LINE}\nNumber of signers: 1\nSigner #1 certificate SHA-256 digest: not-a-digest\n");
        let err = classify("app.apk", &text).unwrap_err();
        assert_eq!(
            err.traceback_line(),
            "ValueError: Delivery APK signing identity differs from the retained certificate: app.apk"
        );
        assert!(signer_digests("Signer # certificate SHA-256 digest: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_empty());
    }

    #[test]
    fn bad_zip_crc_refuses_integrity() {
        let dir = std::env::temp_dir().join(format!("rllm-sig-crc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.apk");
        fs::write(&path, stored_zip("payload", b"abc", 0)).unwrap();
        let err = archive_integrity(&path).unwrap_err();
        assert_eq!(err.traceback_line(), "ValueError: APK archive integrity verification failed: broken.apk");
        fs::write(&path, stored_zip("payload", b"abc", crc32(b"abc"))).unwrap();
        assert!(archive_integrity(&path).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }
}

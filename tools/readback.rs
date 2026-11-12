// Replacement for tools/readback.py. Run the native binary from
// tools/build-tools.sh. Do not run the Python file.
//
// Flags, usage text and exit codes match that program: --apk, --source
// and --checksums are required, usage is on stderr with exit 2 when they
// are missing, and a failed check exits 1. The signing check is the
// behavior of tools/signature.py, which stays a separate file.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[allow(dead_code)]
#[derive(Debug)]
struct PyErr {
    class: &'static str,
    message: String,
    caught: bool,
}

#[allow(dead_code)]
impl PyErr {
    fn value(message: &str) -> Self {
        Self { class: "ValueError", message: message.to_string(), caught: true }
    }
    fn bad(message: &str) -> Self {
        Self { class: "BadZipFile", message: message.to_string(), caught: true }
    }
    fn zlib(message: String) -> Self {
        Self { class: "zlib.error", message, caught: true }
    }
    fn runtime(message: &str) -> Self {
        Self { class: "RuntimeError", message: message.to_string(), caught: true }
    }
    fn eof() -> Self {
        Self { class: "EOFError", message: String::new(), caught: true }
    }
    fn uncaught(class: &'static str, message: String) -> Self {
        Self { class, message, caught: false }
    }
    fn text(&self) -> String {
        self.message.clone()
    }
    fn traceback_line(&self) -> String {
        if self.message.is_empty() {
            self.class.to_string()
        } else {
            format!("{}: {}", self.class, self.message)
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
    // FileNotFoundError and PermissionError are OSError subclasses, which
    // alignment.py catches inside the retry. NotFound during the initial
    // inventory is printed by the caller as an uncaught traceback.
    PyErr { class, message, caught: true }
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut message = Vec::with_capacity(data.len() + 72);
    message.extend_from_slice(data);
    let bits = (data.len() as u64).saturating_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for chunk in message.chunks_exact(64) {
        let mut words = [0u32; 64];
        for index in 0..16 {
            words[index] = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut a: u32 = state[0];
        let mut b: u32 = state[1];
        let mut c: u32 = state[2];
        let mut d: u32 = state[3];
        let mut e: u32 = state[4];
        let mut f: u32 = state[5];
        let mut g: u32 = state[6];
        let mut h: u32 = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let t1 = h.wrapping_add(s1).wrapping_add(choice).wrapping_add(K[index]).wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
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
const USAGE: &str = "usage: readback.py [-h] --apk APK --source SOURCE --checksums CHECKSUMS";
const HELP: &str = "\
usage: readback.py [-h] --apk APK --source SOURCE --checksums CHECKSUMS

options:
  -h, --help            show this help message and exit
  --apk APK
  --source SOURCE
  --checksums CHECKSUMS
";

enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

fn json_error(message: &str) -> PyErr {
    PyErr { class: "JSONDecodeError", message: message.to_string(), caught: false }
}

fn key_error(message: &str) -> PyErr {
    PyErr { class: "KeyError", message: py_repr(message), caught: false }
}

fn type_error(message: &str) -> PyErr {
    PyErr { class: "TypeError", message: message.to_string(), caught: false }
}

fn assertion(message: Option<&str>) -> PyErr {
    PyErr { class: "AssertionError", message: message.unwrap_or("").to_string(), caught: false }
}

fn value_error(message: &str) -> PyErr {
    PyErr { class: "ValueError", message: message.to_string(), caught: false }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while matches!(self.bytes.get(self.index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }
    fn bump(&mut self) -> Result<u8, PyErr> {
        let byte = self.peek().ok_or_else(|| json_error("Expecting value"))?;
        self.index += 1;
        Ok(byte)
    }
    fn parse_value(&mut self) -> Result<Json, PyErr> {
        self.skip();
        match self.peek() {
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(json_error("Expecting value")),
        }
    }
    fn literal(&mut self, text: &[u8], value: Json) -> Result<Json, PyErr> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err(json_error("Expecting value"))
        }
    }
    fn number(&mut self) -> Result<Json, PyErr> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        if self.peek() == Some(b'0') {
            self.index += 1;
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(json_error("Expecting value"));
            }
        } else if matches!(self.peek(), Some(b'1'..=b'9')) {
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        } else {
            return Err(json_error("Expecting value"));
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let mark = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == mark {
                return Err(json_error("Expecting value"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let mark = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == mark {
                return Err(json_error("Expecting value"));
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.index]).unwrap().to_string();
        Ok(Json::Number(token))
    }
    fn hex4(&mut self) -> Result<u32, PyErr> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.bump()?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(json_error("Invalid \\uXXXX escape")),
            };
            value = (value << 4) | digit as u32;
        }
        Ok(value)
    }
    fn string(&mut self) -> Result<String, PyErr> {
        if self.bump()? != b'"' {
            return Err(json_error("Expecting value"));
        }
        let mut out = String::new();
        loop {
            let byte = self.peek().ok_or_else(|| json_error("Unterminated string"))?;
            if byte == b'"' {
                self.index += 1;
                return Ok(out);
            }
            if byte == b'\\' {
                self.index += 1;
                match self.bump()? {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let unit = self.hex4()?;
                        let code = if (0xD800..=0xDBFF).contains(&unit) {
                            if self.bump()? != b'\\' || self.bump()? != b'u' {
                                return Err(json_error("Invalid \\uXXXX escape"));
                            }
                            let low = self.hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err(json_error("Invalid \\uXXXX escape"));
                            }
                            0x10000 + (((unit - 0xD800) << 10) | (low - 0xDC00))
                        } else if (0xDC00..=0xDFFF).contains(&unit) {
                            return Err(json_error("Invalid \\uXXXX escape"));
                        } else {
                            unit
                        };
                        out.push(char::from_u32(code).ok_or_else(|| json_error("Invalid \\uXXXX escape"))?);
                    }
                    _ => return Err(json_error("Invalid \\uXXXX escape")),
                }
                continue;
            }
            if byte < 0x20 {
                return Err(json_error("Invalid control character"));
            }
            let width = match byte {
                0x00..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => return Err(json_error("Invalid UTF-8")),
            };
            let end = self.index + width;
            let text = std::str::from_utf8(self.bytes.get(self.index..end).ok_or_else(|| json_error("Invalid UTF-8"))?)
                .map_err(|_| json_error("Invalid UTF-8"))?;
            out.push_str(text);
            self.index = end;
        }
    }
    fn object(&mut self) -> Result<Json, PyErr> {
        self.bump()?;
        let mut pairs = Vec::new();
        loop {
            self.skip();
            if self.peek() == Some(b'}') {
                self.index += 1;
                return Ok(Json::Obj(pairs));
            }
            if !pairs.is_empty() {
                if self.bump()? != b',' {
                    return Err(json_error("Expecting ',' delimiter"));
                }
                self.skip();
                if self.peek() == Some(b'}') {
                    return Err(json_error("Expecting property name"));
                }
            }
            if self.peek() != Some(b'"') {
                return Err(json_error("Expecting property name"));
            }
            let key = self.string()?;
            self.skip();
            if self.bump()? != b':' {
                return Err(json_error("Expecting ':' delimiter"));
            }
            let value = self.parse_value()?;
            if let Some((_, slot)) = pairs.iter_mut().find(|(name, _)| name == &key) {
                *slot = value;
            } else {
                pairs.push((key, value));
            }
        }
    }
    fn array(&mut self) -> Result<Json, PyErr> {
        self.bump()?;
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.peek() == Some(b']') {
                self.index += 1;
                return Ok(Json::Arr(items));
            }
            if !items.is_empty() {
                if self.bump()? != b',' {
                    return Err(json_error("Expecting ',' delimiter"));
                }
                self.skip();
                if self.peek() == Some(b']') {
                    return Err(json_error("Expecting value"));
                }
            }
            items.push(self.parse_value()?);
        }
    }
}

fn parse_json(text: &str) -> Result<Json, PyErr> {
    let mut parser = Parser { bytes: text.as_bytes(), index: 0 };
    let value = parser.parse_value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err(json_error("Extra data"));
    }
    Ok(value)
}

fn dump_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) > 0x7F => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn dump_into(out: &mut String, value: &Json, indent: Option<usize>, level: usize) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(token) => out.push_str(token),
        Json::Str(text) => dump_string(out, text),
        Json::Arr(items) if items.is_empty() => out.push_str("[]"),
        Json::Obj(pairs) if pairs.is_empty() => out.push_str("{}"),
        Json::Arr(items) => {
            if let Some(width) = indent {
                out.push_str("[\n");
                for (index, item) in items.iter().enumerate() {
                    out.push_str(&" ".repeat(width * (level + 1)));
                    dump_into(out, item, indent, level + 1);
                    if index + 1 != items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&" ".repeat(width * level));
                out.push(']');
            } else {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    dump_into(out, item, None, 0);
                }
                out.push(']');
            }
        }
        Json::Obj(pairs) => {
            if let Some(width) = indent {
                out.push_str("{\n");
                for (index, (key, item)) in pairs.iter().enumerate() {
                    out.push_str(&" ".repeat(width * (level + 1)));
                    dump_string(out, key);
                    out.push_str(": ");
                    dump_into(out, item, indent, level + 1);
                    if index + 1 != pairs.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&" ".repeat(width * level));
                out.push('}');
            } else {
                out.push('{');
                for (index, (key, item)) in pairs.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    dump_string(out, key);
                    out.push_str(": ");
                    dump_into(out, item, None, 0);
                }
                out.push('}');
            }
        }
    }
}

fn dump(value: &Json, indent: Option<usize>) -> String {
    let mut out = String::new();
    dump_into(&mut out, value, indent, 0);
    out
}

fn object<'a>(value: &'a Json, key: &str) -> Result<&'a Json, PyErr> {
    let Json::Obj(pairs) = value else {
        return Err(type_error("expected an object"));
    };
    pairs.iter().find(|(name, _)| name == key).map(|(_, item)| item).ok_or_else(|| key_error(key))
}

fn string_field(value: &Json, key: &str) -> Result<String, PyErr> {
    match object(value, key)? {
        Json::Str(text) => Ok(text.clone()),
        _ => Err(type_error("expected a string")),
    }
}

fn integer_token(text: &str) -> Option<i128> {
    let bare = text.strip_suffix(".0").unwrap_or(text);
    if bare.is_empty() || bare.contains(['.', 'e', 'E', '+']) {
        return None;
    }
    bare.parse::<i128>().ok()
}

fn json_i128(value: &Json) -> Result<i128, PyErr> {
    match value {
        Json::Number(text) => integer_token(text).ok_or_else(|| type_error("expected an integer")),
        _ => Err(type_error("expected an integer")),
    }
}

fn json_equal(left: &Json, right: &Json) -> bool {
    match (left, right) {
        (Json::Null, Json::Null) => true,
        (Json::Bool(a), Json::Bool(b)) => a == b,
        (Json::Str(a), Json::Str(b)) => a == b,
        (Json::Number(a), Json::Number(b)) => match (integer_token(a), integer_token(b)) {
            (Some(left), Some(right)) => left == right,
            _ => a == b,
        },
        _ => false,
    }
}

fn upsert(value: &mut Json, key: &str, item: Json) -> Result<(), PyErr> {
    let Json::Obj(pairs) = value else {
        return Err(type_error("expected an object"));
    };
    if let Some((_, slot)) = pairs.iter_mut().find(|(name, _)| name == key) {
        *slot = item;
    } else {
        pairs.push((key.to_string(), item));
    }
    Ok(())
}

fn base_name(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
    }
}

fn shown(value: &Json) -> String {
    match value {
        Json::Number(token) => token.clone(),
        Json::Str(text) => text.clone(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Null => "None".into(),
        other => dump(other, None),
    }
}

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
    fn names(&self) -> Vec<String> {
        self.members.iter().map(|member| member.name.clone()).collect()
    }
    fn read_name(&self, name: &str) -> Result<Vec<u8>, PyErr> {
        let index = self.members.iter().rposition(|member| member.name == name).ok_or_else(|| {
            let sentence = format!("There is no item named {} in the archive", py_repr(name));
            key_error(&sentence)
        })?;
        let member = self.copy_member(index);
        self.read_member(&member)
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

fn project_root() -> Result<PathBuf, PyErr> {
    let mut starts = Vec::new();
    if let Ok(path) = env::current_exe() {
        starts.push(path);
    }
    if let Ok(path) = env::current_dir() {
        starts.push(path);
    }
    if let Some(arg0) = env::args().next() {
        starts.push(PathBuf::from(arg0));
    }
    for start in starts {
        let mut dir = fs::canonicalize(&start).unwrap_or(start);
        loop {
            if dir.join("tools/readback.rs").is_file() {
                return Ok(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    Err(value_error("cannot locate the RLLM project root from tools/readback.rs"))
}

fn signer_digest(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("Signer #")?;
    let (number, rest) = rest.split_once(' ')?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let digest = rest.strip_prefix("certificate SHA-256 digest: ")?;
    if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        Some(digest)
    } else {
        None
    }
}

fn signature(path: &Path) -> Result<Json, PyErr> {
    let jdk = env::var("CLIENT_JDK").map_err(|_| key_error("CLIENT_JDK"))?;
    let tools = env::var("CLIENT_TOOLS").map_err(|_| key_error("CLIENT_TOOLS"))?;
    let inherited = env::var("PATH").map_err(|_| key_error("PATH"))?;
    let program = Path::new(&tools).join("apksigner");
    let output = Command::new(&program)
        .args(["verify", "--verbose", "--print-certs"])
        .arg(path)
        .env("PATH", format!("{jdk}/bin:{inherited}"))
        .output()
        .map_err(|err| io_err(&err, &program.display().to_string()))?;
    if !output.status.success() {
        let name = path.file_name().and_then(|text| text.to_str()).unwrap_or("apk");
        return Err(value_error(&format!("APK signature verification failed: {name}")));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let v2 = "Verified using v2 scheme (APK Signature Scheme v2): true";
    let v3 = "Verified using v3 scheme (APK Signature Scheme v3): true";
    let name = path.file_name().and_then(|text| text.to_str()).unwrap_or("apk");
    if !text.contains(v2) || !text.contains(v3) {
        return Err(value_error(&format!("Delivery APK requires verified v2 and v3 signatures: {name}")));
    }
    let certificates: Vec<&str> = text.lines().filter_map(signer_digest).collect();
    let one_signer = text.lines().any(|line| line == "Number of signers: 1");
    if certificates != [CERTIFICATE] || !one_signer {
        return Err(value_error(&format!(
            "Delivery APK signing identity differs from the retained certificate: {name}"
        )));
    }
    let archive = Archive::open(path)?;
    if archive.testzip()?.is_some() {
        return Err(value_error(&format!("APK archive integrity verification failed: {name}")));
    }
    Ok(Json::Obj(vec![
        ("v2".into(), Json::Bool(true)),
        ("v3".into(), Json::Bool(true)),
        ("certificate_sha256".into(), Json::Str(CERTIFICATE.into())),
    ]))
}

fn same_file(path: &Path, bytes: u64, digest: &str) -> Result<Vec<u8>, PyErr> {
    let metadata = fs::metadata(path).map_err(|err| io_err(&err, &path.display().to_string()))?;
    let data = fs::read(path).map_err(|err| io_err(&err, &path.display().to_string()))?;
    if metadata.len() == bytes && hex_digest(&sha256(&data)) == digest {
        Ok(data)
    } else {
        Err(assertion(None))
    }
}

fn readback(apk: &Path, source: &Path, checksums: &Path) -> Result<(), PyErr> {
    let root = project_root()?;
    let packaged = root.join("output/checks/packaged.json");
    let text = fs::read_to_string(&packaged).map_err(|err| io_err(&err, &packaged.display().to_string()))?;
    let mut record = parse_json(&text)?;
    let paths = object(&record, "paths")?;
    let files = object(&record, "files")?;
    for (key, supplied) in [("apk", apk), ("source", source), ("checksums", checksums)] {
        let recorded = string_field(paths, key)?;
        let meta = object(files, &base_name(&recorded))?;
        let size = json_i128(object(meta, "bytes")?)?;
        let digest = string_field(meta, "sha256")?;
        if size < 0 {
            return Err(assertion(Some(key)));
        }
        match same_file(supplied, size as u64, &digest) {
            Ok(_) => {}
            Err(err) if err.class == "AssertionError" => return Err(assertion(Some(key))),
            Err(err) => return Err(err),
        }
    }
    let downloaded_signature = signature(apk)?;
    let archive = Archive::open(source)?;
    let names = archive.names();
    let members = json_i128(object(&record, "members")?)?;
    if archive.testzip()?.is_some() || names.len() as i128 != members {
        return Err(assertion(None));
    }
    let manifest_bytes = archive.read_name("RLLM/PUBLICATION-MANIFEST.json")?;
    let manifest_text = String::from_utf8(manifest_bytes).map_err(|_| value_error("publication manifest is not UTF-8"))?;
    let manifest = parse_json(&manifest_text)?;
    let version_matches = json_equal(object(&manifest, "version")?, object(&record, "version")?);
    let build_inputs = object(&manifest, "build_inputs")?;
    let build_count = match build_inputs {
        Json::Obj(pairs) => pairs.len() as i128,
        Json::Arr(items) => items.len() as i128,
        _ => return Err(type_error("object of type 'int' has no len()")),
    };
    if !version_matches || build_count != json_i128(object(&record, "build_inputs")?)? {
        return Err(assertion(None));
    }
    let Json::Obj(manifest_files) = object(&manifest, "files")? else {
        return Err(type_error("expected an object"));
    };
    for (name, meta) in manifest_files {
        let size = json_i128(object(meta, "bytes")?)?;
        let digest = string_field(meta, "sha256")?;
        let data = archive.read_name(&format!("RLLM/{name}"))?;
        if size < 0 || data.len() as i128 != size || hex_digest(&sha256(&data)) != digest {
            return Err(assertion(Some(name)));
        }
    }
    let signed = object(&manifest, "signed_apk")?;
    let signed_bytes = json_i128(object(signed, "bytes")?)?;
    let signed_sha = string_field(signed, "sha256")?;
    let apk_bytes = fs::read(apk).map_err(|err| io_err(&err, &apk.display().to_string()))?;
    let apk_len_differs = signed_bytes < 0 || apk_bytes.len() as i128 != signed_bytes;
    let apk_hash_differs = hex_digest(&sha256(&apk_bytes)) != signed_sha;
    if apk_len_differs || apk_hash_differs || apk_bytes.len() as u64 != signed_bytes as u64 {
        return Err(assertion(None));
    }
    let signed_location = match object(signed, "location") {
        Ok(Json::Str(text)) => text.clone(),
        Ok(_) => String::new(),
        Err(err) if err.class == "KeyError" => String::new(),
        Err(err) => return Err(err),
    };
    let apk_name = base_name(&string_field(object(&record, "paths")?, "apk")?);
    let apks: Vec<String> = names.into_iter().filter(|name| name.ends_with(".apk")).collect();
    if signed_location == "companion" {
        let signed_path = string_field(signed, "path")?;
        if !apks.is_empty() || signed_path != apk_name {
            return Err(assertion(None));
        }
    } else if apks.len() != 1 || apks[0] != "RLLM/release/RLLM.apk" || archive.read_name("RLLM/release/RLLM.apk")? != apk_bytes {
        return Err(assertion(None));
    }
    upsert(&mut record, "remote_verified", Json::Bool(true))?;
    upsert(&mut record, "downloaded_signature", downloaded_signature)?;
    let destination = root.join("output/checks/readback.json");
    fs::write(&destination, format!("{}\n", dump(&record, Some(2)))).map_err(|err| io_err(&err, &destination.display().to_string()))?;
    println!(
        "Downloaded APK/source/checksums and every source member verified for build {}",
        shown(object(&record, "version")?)
    );
    Ok(())
}

fn usage_error(message: &str) -> i32 {
    eprintln!("{USAGE}");
    eprintln!("readback.py: error: {message}");
    2
}

fn looks_like_option(token: &str) -> bool {
    token.starts_with('-') && token != "-"
}

fn parse_cli(args: &[String]) -> Result<(String, String, String), i32> {
    let names = ["apk", "source", "checksums"];
    let mut values = [None, None, None];
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
            return Err(0);
        }
        let matched = if options {
            names.iter().position(|name| arg == &format!("--{name}") || arg.starts_with(&format!("--{name}=")))
        } else {
            None
        };
        if let Some(slot) = matched {
            let name = names[slot];
            let value = if let Some(inline) = arg.strip_prefix(&format!("--{name}=")) {
                inline.to_string()
            } else {
                let next = args.get(index + 1);
                if next.map(|token| looks_like_option(token)).unwrap_or(true) {
                    return Err(usage_error(&format!("argument --{name}: expected one argument")));
                }
                index += 1;
                next.unwrap().clone()
            };
            values[slot] = Some(value);
            index += 1;
            continue;
        }
        if options && looks_like_option(arg) {
            unrecognized.push(arg.clone());
            index += 1;
            continue;
        }
        unrecognized.push(arg.clone());
        index += 1;
    }
    let mut missing = Vec::new();
    for (slot, name) in names.iter().enumerate() {
        if values[slot].is_none() {
            missing.push(format!("--{name}"));
        }
    }
    if !missing.is_empty() {
        return Err(usage_error(&format!(
            "the following arguments are required: {}",
            missing.join(", ")
        )));
    }
    if !unrecognized.is_empty() {
        return Err(usage_error(&format!("unrecognized arguments: {}", unrecognized.join(" "))));
    }
    Ok((values[0].clone().unwrap(), values[1].clone().unwrap(), values[2].clone().unwrap()))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match parse_cli(&args) {
        Err(code) => code,
        Ok((apk, source, checksums)) => match readback(Path::new(&apk), Path::new(&source), Path::new(&checksums)) {
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
mod checks {
    use super::*;

    #[test]
    fn json_dump_matches_python() {
        let raw = r#"{"b":1,"a":true,"s":"a\"b","n":null,"arr":[1,"x",{"z":false}],"o":{"k":"v"},"u":"é"}"#;
        let value = parse_json(raw).unwrap();
        let indented = "{\n  \"b\": 1,\n  \"a\": true,\n  \"s\": \"a\\\"b\",\n  \"n\": null,\n  \"arr\": [\n    1,\n    \"x\",\n    {\n      \"z\": false\n    }\n  ],\n  \"o\": {\n    \"k\": \"v\"\n  },\n  \"u\": \"\\u00e9\"\n}";
        assert_eq!(dump(&value, Some(2)), indented);
        let compact = r#"{"b": 1, "a": true, "s": "a\"b", "n": null, "arr": [1, "x", {"z": false}], "o": {"k": "v"}, "u": "\u00e9"}"#;
        assert_eq!(dump(&value, None), compact);
    }

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    struct ZipSpec {
        name: &'static [u8],
        flags: u16,
        method: u16,
        local_method: u16,
        payload: &'static [u8],
        plain: &'static [u8],
        forced_offset: Option<u32>,
    }

    struct SyntheticZip {
        bytes: Vec<u8>,
        cd_offset: usize,
        local_offsets: Vec<usize>,
        payload_offsets: Vec<usize>,
    }

    fn synthetic_zip(prefix: &[u8], members: &[ZipSpec]) -> SyntheticZip {
        let mut bytes = prefix.to_vec();
        let mut local_offsets = Vec::new();
        let mut payload_offsets = Vec::new();
        for member in members {
            local_offsets.push(bytes.len());
            bytes.extend_from_slice(b"PK\x03\x04");
            push_u16(&mut bytes, 20);
            push_u16(&mut bytes, member.flags);
            push_u16(&mut bytes, member.local_method);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u16(&mut bytes, member.name.len() as u16);
            push_u16(&mut bytes, 0);
            bytes.extend_from_slice(member.name);
            payload_offsets.push(bytes.len());
            bytes.extend_from_slice(member.payload);
        }
        let cd_offset = bytes.len();
        for (member, offset) in members.iter().zip(local_offsets.iter()) {
            let recorded = member.forced_offset.unwrap_or(*offset as u32);
            bytes.extend_from_slice(b"PK\x01\x02");
            push_u16(&mut bytes, 20);
            push_u16(&mut bytes, 20);
            push_u16(&mut bytes, member.flags);
            push_u16(&mut bytes, member.method);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, crc32(member.plain));
            push_u32(&mut bytes, member.payload.len() as u32);
            push_u32(&mut bytes, member.plain.len() as u32);
            push_u16(&mut bytes, member.name.len() as u16);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, recorded);
            bytes.extend_from_slice(member.name);
        }
        let cd_size = (bytes.len() - cd_offset) as u32;
        bytes.extend_from_slice(b"PK\x05\x06");
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, members.len() as u16);
        push_u16(&mut bytes, members.len() as u16);
        push_u32(&mut bytes, cd_size);
        push_u32(&mut bytes, cd_offset as u32);
        push_u16(&mut bytes, 0);
        SyntheticZip { bytes, cd_offset, local_offsets, payload_offsets }
    }

    fn two_members(forced_second_offset: Option<u32>) -> Vec<ZipSpec> {
        vec![
            ZipSpec {
                name: b"na\x8bve.dat",
                flags: 0,
                method: 0,
                local_method: 99,
                payload: b"hello",
                plain: b"hello",
                forced_offset: None,
            },
            ZipSpec {
                name: b"caf\xc3\xa9.txt",
                flags: 0x800,
                method: 8,
                local_method: 0,
                payload: &[0x73, 0x04, 0x02, 0x00],
                plain: b"AAAA",
                forced_offset: forced_second_offset,
            },
        ]
    }

    fn sample_zip() -> SyntheticZip {
        synthetic_zip(b"PK\x03\x04decoy", &two_members(None))
    }

    fn err_string<T>(result: Result<T, String>) -> String {
        match result {
            Err(err) => err,
            Ok(_) => panic!("expected an error"),
        }
    }

    fn err_archive(result: Result<Archive, PyErr>) -> PyErr {
        match result {
            Err(err) => err,
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn inflate_stored_block_returns_bytes() {
        let stream = [0x01, 0x02, 0x00, 0xfd, 0xff, b'H', b'i'];
        assert_eq!(inflate(&stream, 2).unwrap(), b"Hi");
    }

    #[test]
    fn inflate_fixed_huffman_backreference_returns_bytes() {
        let stream = [0x73, 0x04, 0x02, 0x00];
        assert_eq!(inflate(&stream, 4).unwrap(), b"AAAA");
    }

    #[test]
    fn inflate_dynamic_huffman_returns_literal() {
        let stream = [0x05, 0xc0, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x90, 0x36, 0xff, 0x53, 0x08];
        assert_eq!(inflate(&stream, 1).unwrap(), b"A");
    }

    #[test]
    fn inflate_truncated_stored_block_refuses() {
        let stream = [0x01, 0x02, 0x00, 0xfd, 0xff, b'H'];
        assert_eq!(inflate(&stream, 2).unwrap_err(), "Truncated compressed stored block");
    }

    #[test]
    fn inflate_missing_final_block_bit_refuses() {
        let stream = [0x00, 0x01, 0x00, 0xfe, 0xff, b'Z'];
        assert_eq!(inflate(&stream, 1).unwrap_err(), "Truncated compressed member");
    }

    #[test]
    fn inflate_output_over_expected_refuses() {
        let stored = [0x01, 0x02, 0x00, 0xfd, 0xff, b'H', b'i'];
        assert_eq!(inflate(&stored, 1).unwrap_err(), "Compressed member exceeds declared size");
        let huffman = [0x73, 0x04, 0x02, 0x00];
        assert_eq!(inflate(&huffman, 0).unwrap_err(), "Compressed member exceeds declared size");
        assert_eq!(inflate(&huffman, 3).unwrap_err(), "Compressed back-reference outside member bounds");
    }

    #[test]
    fn inflate_trailing_byte_refuses() {
        let stream = [0x01, 0x02, 0x00, 0xfd, 0xff, b'H', b'i', 0x00];
        assert_eq!(inflate(&stream, 2).unwrap_err(), "Compressed member size or trailing data mismatch");
    }

    #[test]
    fn bit_reader_aligns_before_stored_bytes() {
        let data = [0b0001_1111, 0x99];
        let mut bits = Bits::new(&data);
        assert_eq!(bits.take(4).unwrap(), 0b1111);
        bits.align();
        assert_eq!(bits.read_stored(1).unwrap(), &[0x99]);
        assert_eq!(bits.consumed(), 2);
    }

    #[test]
    fn bit_reader_truncated_take_refuses() {
        let mut empty = Bits::new(&[]);
        assert_eq!(empty.take(1).unwrap_err(), "Truncated compressed member");
        let mut wide = Bits::new(&[0xff]);
        assert_eq!(wide.take(17).unwrap_err(), "Truncated compressed member");
        let mut short = Bits::new(&[0xff]);
        assert_eq!(short.take(9).unwrap_err(), "Truncated compressed member");
        let mut stored = Bits::new(&[0x01]);
        stored.take(3).unwrap();
        assert_eq!(stored.read_stored(5).unwrap_err(), "Truncated compressed stored block");
    }

    #[test]
    fn huffman_codes_reject_bad_lengths_and_decode_symbol() {
        assert_eq!(err_string(Codes::new(&[1, 1, 1], false, false)), "Oversubscribed compressed codes");
        assert_eq!(err_string(Codes::new(&[1], false, false)), "Incomplete compressed codes");
        assert_eq!(err_string(Codes::new(&[16], false, false)), "Invalid compressed code length");
        let codes = Codes::new(&[1, 1], false, false).unwrap();
        let mut bits = Bits::new(&[0x02]);
        assert_eq!(codes.symbol(&mut bits).unwrap(), 0);
        assert_eq!(codes.symbol(&mut bits).unwrap(), 1);
        let single = Codes::new(&[1], true, false).unwrap();
        let mut miss = Bits::new(&[0xff, 0xff]);
        assert_eq!(single.symbol(&mut miss).unwrap_err(), "Invalid compressed symbol");
    }

    #[test]
    fn dynamic_tables_reject_reserved_literal_count() {
        let mut bits = Bits::new(&[0x1f, 0x00]);
        assert_eq!(err_string(tables(&mut bits)), "Reserved compressed literal count");
    }

    #[test]
    fn zip_member_list_comes_from_central_directory() {
        let zip = sample_zip();
        assert_eq!(zip.local_offsets, vec![9, 53]);
        assert_eq!(zip.payload_offsets, vec![48, 92]);
        assert_eq!(zip.cd_offset, 96);
        assert_eq!(&zip.bytes[0..4], b"PK\x03\x04");
        assert_eq!(&zip.bytes[48..53], b"hello");
        assert_eq!(&zip.bytes[92..96], [0x73, 0x04, 0x02, 0x00]);
        let archive = Archive::parse(zip.bytes).unwrap();
        assert_eq!(archive.members.len(), 2);
        assert_eq!(archive.members[0].name, "naïve.dat");
        assert_eq!(archive.members[0].method, 0);
        assert_eq!(archive.members[0].compressed_size, 5);
        assert_eq!(archive.members[0].uncompressed_size, 5);
        assert_eq!(archive.members[0].local_offset, 9);
        assert_eq!(archive.members[1].name, "café.txt");
        assert_eq!(archive.members[1].method, 8);
        assert_eq!(archive.members[1].compressed_size, 4);
        assert_eq!(archive.members[1].uncompressed_size, 4);
        assert_eq!(archive.members[1].local_offset, 53);
        assert_eq!(&archive.data[96..100], b"PK\x01\x02");
        assert_eq!(u16_at(&archive.data, 17).unwrap(), 99);
        assert_eq!(u32_at(&archive.data, 27).unwrap(), 0);
        assert_eq!(u16_at(&archive.data, 61).unwrap(), 0);
        assert_eq!(u32_at(&archive.data, 71).unwrap(), 0);
        assert_ne!(archive.members[0].method, u16_at(&archive.data, 17).unwrap());
        assert_ne!(archive.members[1].method, u16_at(&archive.data, 61).unwrap());
    }

    #[test]
    fn zip_bad_central_directory_signature_refuses() {
        let mut zip = sample_zip();
        zip.bytes[zip.cd_offset] = 0x00;
        let err = err_archive(Archive::parse(zip.bytes));
        assert_eq!(err.class, "BadZipFile");
        assert_eq!(err.text(), "Bad magic number for central directory");
        assert_eq!(err.traceback_line(), "BadZipFile: Bad magic number for central directory");
    }

    #[test]
    fn zip_truncated_archive_refuses() {
        let zip = sample_zip();
        let err = err_archive(Archive::parse(zip.bytes[..zip.bytes.len() - 4].to_vec()));
        assert_eq!(err.class, "BadZipFile");
        assert_eq!(err.text(), "File is not a zip file");
        assert_eq!(err.traceback_line(), "BadZipFile: File is not a zip file");
        let err = err_archive(Archive::parse(vec![0u8; 21]));
        assert_eq!(err.traceback_line(), "BadZipFile: File is not a zip file");
    }

    #[test]
    fn zip_name_length_mismatch_refuses() {
        let mut zip = sample_zip();
        let name_len_at = zip.cd_offset + 28;
        zip.bytes[name_len_at..name_len_at + 2].copy_from_slice(&200u16.to_le_bytes());
        let err = err_archive(Archive::parse(zip.bytes));
        assert_eq!(err.class, "BadZipFile");
        assert_eq!(err.text(), "Truncated central directory");
        assert_eq!(err.traceback_line(), "BadZipFile: Truncated central directory");
    }

    #[test]
    fn decode_name_utf8_flag_and_cp437() {
        assert_eq!(cp437(&[0x82]), "é");
        assert_eq!(cp437(&[0x8b]), "ï");
        assert_eq!(decode_name(b"caf\xc3\xa9.txt", 0x800).unwrap(), "café.txt");
        assert_eq!(decode_name(b"na\x8bve.dat", 0).unwrap(), "naïve.dat");
        assert_eq!(decode_name(b"caf\xc3\xa9.txt", 0).unwrap(), "caf├⌐.txt");
        let err = decode_name(b"na\x8bve.dat", 0x800).unwrap_err();
        assert_eq!(err.class, "UnicodeDecodeError");
        assert_eq!(err.text(), "zip filename is not valid UTF-8");
        assert_eq!(err.traceback_line(), "UnicodeDecodeError: zip filename is not valid UTF-8");
    }

    #[test]
    fn read_member_deflate_round_trip() {
        let archive = Archive::parse(sample_zip().bytes).unwrap();
        assert_eq!(archive.read_member(&archive.members[0]).unwrap(), b"hello");
        assert_eq!(archive.read_member(&archive.members[1]).unwrap(), b"AAAA");
    }

    #[test]
    fn read_member_corrupt_deflate_is_zlib_error() {
        let zip = sample_zip();
        let mut bytes = zip.bytes;
        bytes[zip.payload_offsets[1]] = 0x07;
        let archive = Archive::parse(bytes).unwrap();
        let err = archive.read_member(&archive.members[1]).unwrap_err();
        assert_eq!(err.class, "zlib.error");
        assert_eq!(err.text(), "Reserved compressed block type");
        assert_eq!(err.traceback_line(), "zlib.error: Reserved compressed block type");
        assert_ne!(err.class, "ValueError");
        assert_ne!(err.class, "RuntimeError");
    }

    #[test]
    fn read_member_offset_outside_archive_refuses() {
        let archive = Archive::parse(synthetic_zip(b"PK\x03\x04decoy", &two_members(Some(50_000))).bytes).unwrap();
        assert_eq!(archive.members[1].local_offset, 50_000);
        assert_eq!(archive.read_member(&archive.members[0]).unwrap(), b"hello");
        let err = archive.read_member(&archive.members[1]).unwrap_err();
        assert_eq!(err.class, "BadZipFile");
        assert_eq!(err.text(), "Truncated file header");
        assert_eq!(err.traceback_line(), "BadZipFile: Truncated file header");
    }

    #[test]
    fn little_endian_directory_words_and_short_reads() {
        assert_eq!(u16_at(&[0x34, 0x12], 0).unwrap(), 0x1234);
        assert_eq!(u32_at(&[0x78, 0x56, 0x34, 0x12], 0).unwrap(), 0x1234_5678);
        let err = u16_at(&[0x00], 0).unwrap_err();
        assert_eq!(err.class, "BadZipFile");
        assert_eq!(err.text(), "Truncated central directory");
        assert_eq!(err.traceback_line(), "BadZipFile: Truncated central directory");
        assert_eq!(u32_at(&[0, 1, 2], 1).unwrap_err().text(), "Truncated central directory");
    }

    #[test]
    fn py_repr_matches_python_for_quote_slash_control_and_non_ascii() {
        assert_eq!(py_repr("a\"b\\c\u{0001}é"), "'a\"b\\\\c\\x01é'");
    }

    #[test]
    fn py_err_text_and_traceback_match_python() {
        let value = PyErr::value("bad zip offset");
        assert_eq!(value.text(), "bad zip offset");
        assert_eq!(value.traceback_line(), "ValueError: bad zip offset");
        let eof = PyErr::eof();
        assert_eq!(eof.text(), "");
        assert_eq!(eof.traceback_line(), "EOFError");
        let zlib = PyErr::zlib("invalid block type".to_string());
        assert_eq!(zlib.text(), "invalid block type");
        assert_eq!(zlib.traceback_line(), "zlib.error: invalid block type");
    }

    #[test]
    fn io_err_errno_text_matches_python() {
        let missing = io_err(&std::io::Error::from_raw_os_error(2), "a\"b\\c");
        assert_eq!(missing.class, "FileNotFoundError");
        assert_eq!(missing.text(), "[Errno 2] No such file or directory: 'a\"b\\\\c'");
        assert_eq!(
            missing.traceback_line(),
            "FileNotFoundError: [Errno 2] No such file or directory: 'a\"b\\\\c'"
        );
        let denied = io_err(&std::io::Error::from_raw_os_error(13), "x");
        assert_eq!(denied.text(), "[Errno 13] Permission denied: 'x'");
        assert_eq!(denied.traceback_line(), "PermissionError: [Errno 13] Permission denied: 'x'");
    }
}

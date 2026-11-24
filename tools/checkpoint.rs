// Replacement for tools/checkpoint.py. Run the native binary from
// tools/build-tools.sh. Do not run the Python file.
//
// The positional description, usage text and exit codes match that
// program. A description outside [A-Za-z]+(?:-[A-Za-z]+)* is refused
// before any directory is created. The archive goes under the project
// root's grandparent recovery directory, the same path checkpoint.py
// builds as parents[1] of the project / 'recovery'.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
fn bare_assert() -> PyErr {
    PyErr { class: "AssertionError", message: String::new(), caught: false }
}

fn valid_description(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    text.split('-').all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
}

fn name_has_a_digit(name: &str) -> bool {
    name.as_bytes().windows(2).any(|pair| (pair[0] == b'a' || pair[0] == b'A') && pair[1].is_ascii_digit())
}

fn utc_stamp(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    let hour = secs / 3600;
    let minute = (secs % 3600) / 60;
    let second = secs % 60;
    format!("{year:04}-{month:02}-{day:02}__{hour:02}-{minute:02}-{second:02}Z")
}

fn now_unix() -> Result<i64, PyErr> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| PyErr::uncaught("OSError", "system clock is before 1970".into()))?;
    Ok(elapsed.as_secs() as i64)
}

fn grandparent(root: &Path) -> Result<&Path, PyErr> {
    root.parent()
        .and_then(Path::parent)
        .ok_or_else(|| PyErr::value("checkpoint root has no grandparent"))
}

fn deflate_store(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0x01, 0x00, 0x00, 0xff, 0xff];
    }
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 8);
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + 65535).min(data.len());
        let chunk = &data[offset..end];
        out.push(if end == data.len() { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
        offset = end;
    }
    out
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

struct Stored {
    name: String,
    crc: u32,
    compressed: Vec<u8>,
    plain_len: u32,
}

fn fit_u32(value: usize, what: &str) -> Result<u32, PyErr> {
    u32::try_from(value).map_err(|_| PyErr::runtime(&format!("checkpoint {what} exceeds ZIP32 limits")))
}

fn store_member(name: &str, plain: &[u8]) -> Result<Stored, PyErr> {
    if name.len() > u16::MAX as usize {
        return Err(PyErr::runtime("checkpoint member name exceeds ZIP32 limits"));
    }
    Ok(Stored {
        name: name.to_string(),
        crc: crc32(plain),
        compressed: deflate_store(plain),
        plain_len: fit_u32(plain.len(), "member")?,
    })
}

fn write_archive(path: &Path, members: &[Stored]) -> Result<(), PyErr> {
    if members.len() > u16::MAX as usize {
        return Err(PyErr::runtime("checkpoint has too many members for ZIP32"));
    }
    let mut body = Vec::new();
    let mut directory = Vec::new();
    for member in members {
        let offset = fit_u32(body.len(), "archive")?;
        let name = member.name.as_bytes();
        push_u32(&mut body, 0x0403_4b50);
        push_u16(&mut body, 20);
        push_u16(&mut body, 0x0800);
        push_u16(&mut body, 8);
        push_u16(&mut body, 0);
        push_u16(&mut body, 0x0021);
        push_u32(&mut body, member.crc);
        push_u32(&mut body, fit_u32(member.compressed.len(), "member")?);
        push_u32(&mut body, member.plain_len);
        push_u16(&mut body, name.len() as u16);
        push_u16(&mut body, 0);
        body.extend_from_slice(name);
        body.extend_from_slice(&member.compressed);
        push_u32(&mut directory, 0x0201_4b50);
        push_u16(&mut directory, 0x0314);
        push_u16(&mut directory, 20);
        push_u16(&mut directory, 0x0800);
        push_u16(&mut directory, 8);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0x0021);
        push_u32(&mut directory, member.crc);
        push_u32(&mut directory, fit_u32(member.compressed.len(), "member")?);
        push_u32(&mut directory, member.plain_len);
        push_u16(&mut directory, name.len() as u16);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u32(&mut directory, 0);
        push_u32(&mut directory, offset);
        directory.extend_from_slice(name);
    }
    let directory_offset = fit_u32(body.len(), "archive")?;
    let directory_len = fit_u32(directory.len(), "central directory")?;
    body.extend_from_slice(&directory);
    push_u32(&mut body, 0x0605_4b50);
    push_u16(&mut body, 0);
    push_u16(&mut body, 0);
    push_u16(&mut body, members.len() as u16);
    push_u16(&mut body, members.len() as u16);
    push_u32(&mut body, directory_len);
    push_u32(&mut body, directory_offset);
    push_u16(&mut body, 0);
    fs::write(path, &body).map_err(|err| io_err(&err, &path.display().to_string()))
}

enum Json {
    Number(String),
    Str(String),
    Obj(Vec<(String, Json)>),
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
        Json::Number(token) => out.push_str(token),
        Json::Str(text) => dump_string(out, text),
        Json::Obj(pairs) if pairs.is_empty() => out.push_str("{}"),
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
    #[allow(dead_code)]
    fn names(&self) -> Vec<String> {
        self.members.iter().map(|member| member.name.clone()).collect()
    }
    fn read_name(&self, name: &str) -> Result<Vec<u8>, PyErr> {
        let index = self.members.iter().rposition(|member| member.name == name).ok_or_else(|| {
            let sentence = format!("There is no item named {} in the archive", py_repr(name));
            PyErr { class: "KeyError", message: py_repr(&sentence), caught: false }
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
            if dir.join("tools/checkpoint.rs").is_file() {
                return Ok(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    Err(PyErr::value("cannot locate the RLLM project root from tools/checkpoint.rs"))
}

fn path_key(path: &Path) -> Vec<std::ffi::OsString> {
    path.components().map(|component| component.as_os_str().to_os_string()).collect()
}

fn rel_posix(root: &Path, path: &Path) -> Result<String, PyErr> {
    let relative = path.strip_prefix(root).map_err(|_| PyErr::value("checkpoint file escaped its root"))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let text = component.as_os_str().to_str().ok_or_else(|| {
            PyErr::uncaught("UnicodeEncodeError", "checkpoint path is not UTF-8".into())
        })?;
        parts.push(text.to_string());
    }
    Ok(parts.join("/"))
}

fn excluded(relative: &str) -> bool {
    const SKIP: &[&str] = &["output", "release", "__pycache__", ".git"];
    relative.split('/').any(|part| SKIP.contains(&part))
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), PyErr> {
    let entries = fs::read_dir(dir).map_err(|err| io_err(&err, &dir.display().to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|err| io_err(&err, &dir.display().to_string()))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|err| io_err(&err, &path.display().to_string()))?;
        if meta.file_type().is_symlink() {
            match fs::metadata(&path) {
                Ok(target) if target.is_file() => files.push(path),
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(io_err(&err, &path.display().to_string())),
            }
            continue;
        }
        if meta.is_dir() {
            collect_files(&path, files)?;
        } else if meta.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn checkpoint_at(root: &Path, description: &str, stamp: &str) -> Result<String, PyErr> {
    if !valid_description(description) {
        return Err(bare_assert());
    }
    let name = format!("RLLM__{description}__{stamp}.zip");
    if name_has_a_digit(&name) {
        return Err(bare_assert());
    }
    let recovery = grandparent(root)?.join("recovery");
    fs::create_dir_all(&recovery).map_err(|err| io_err(&err, &recovery.display().to_string()))?;
    let output = recovery.join(&name);
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.sort_by(|left, right| path_key(left).cmp(&path_key(right)));
    let mut kept = Vec::new();
    for path in files {
        let relative = rel_posix(root, &path)?;
        if !excluded(&relative) {
            kept.push((path, relative));
        }
    }
    if kept.iter().any(|(path, _)| fs::symlink_metadata(path).map(|meta| meta.file_type().is_symlink()).unwrap_or(false)) {
        return Err(bare_assert());
    }
    let mut manifest = Vec::new();
    let mut members = Vec::new();
    for (path, relative) in &kept {
        let plain = fs::read(path).map_err(|err| io_err(&err, &path.display().to_string()))?;
        let digest = hex_digest(&sha256(&plain));
        members.push(store_member(&format!("RLLM/{relative}"), &plain)?);
        manifest.push((relative.clone(), Json::Str(digest)));
    }
    let record = Json::Obj(vec![
        ("status".into(), Json::Str("in-progress; not a signed release".into())),
        ("files".into(), Json::Obj(manifest)),
    ]);
    let checkpoint_json = dump(&record, Some(2));
    members.push(store_member("CHECKPOINT.json", checkpoint_json.as_bytes())?);
    write_archive(&output, &members)?;
    let archive = Archive::open(&output)?;
    if archive.testzip()?.is_some() {
        return Err(bare_assert());
    }
    for (relative, digest) in kept.iter().map(|(_, relative)| relative).zip(manifest_digests(&record)) {
        let data = archive.read_name(&format!("RLLM/{relative}"))?;
        if hex_digest(&sha256(&data)) != digest {
            return Err(bare_assert());
        }
    }
    let bytes = fs::read(&output).map_err(|err| io_err(&err, &output.display().to_string()))?;
    let printed = Json::Obj(vec![
        ("path".into(), Json::Str(output.display().to_string())),
        ("name".into(), Json::Str(name)),
        ("sha256".into(), Json::Str(hex_digest(&sha256(&bytes)))),
        ("files".into(), Json::Number(kept.len().to_string())),
    ]);
    Ok(dump(&printed, None))
}

fn manifest_digests(record: &Json) -> Vec<String> {
    let Json::Obj(pairs) = record else { return Vec::new() };
    let Some(Json::Obj(files)) = pairs.iter().find(|(key, _)| key == "files").map(|(_, value)| value) else {
        return Vec::new();
    };
    files.iter().filter_map(|(_, value)| match value {
        Json::Str(text) => Some(text.clone()),
        _ => None,
    }).collect()
}

fn execute(description: &str) -> Result<String, PyErr> {
    if !valid_description(description) {
        return Err(bare_assert());
    }
    let root = project_root()?;
    checkpoint_at(&root, description, &utc_stamp(now_unix()?))
}

const USAGE: &str = "usage: checkpoint.py [-h] description";
const HELP: &str = "\
usage: checkpoint.py [-h] description

positional arguments:
  description

options:
  -h, --help   show this help message and exit
";

fn usage_error(message: &str) -> i32 {
    eprintln!("{USAGE}");
    eprintln!("checkpoint.py: error: {message}");
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
        return Err(usage_error("the following arguments are required: description"));
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
        Ok(description) => match execute(&description) {
            Ok(line) => {
                println!("{line}");
                0
            }
            Err(err) => {
                eprintln!("{}", err.traceback_line());
                1
            }
        },
    };
    std::process::exit(code);
}

// Replacement for tools/toolchain.py. Run the native binary from
// tools/build-tools.sh. Do not run the Python file.
//
// No arguments restores the pinned toolchain into the current directory.
// Help is on stdout. Any other argument prints usage on stderr and exits 2.
// Each refusal keeps the Python exception line and exits 1.

use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug)]
struct PyErr {
    class: &'static str,
    message: String,
    caught: bool,
}

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
    const LIMIT: usize = 1024 * 1024 * 1024 - 1;
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
    external: u32,
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
            let external = u32_at(&data, cursor + 38)?;
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
                external,
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


const USAGE: &str = "usage: toolchain.py [-h]";
const HELP: &str = "\
usage: toolchain.py [-h]

options:
  -h, --help  show this help message and exit
";
const ENV_SH: &str = "project_tooling=$(cd \"$(dirname \"${BASH_SOURCE[0]}\")\" && pwd)\n\
export CLIENT_RUST=\"$project_tooling/rust\"\n\
export CLIENT_JDK=\"$project_tooling/jdk/jdk-17.0.21+9\"\n\
export CLIENT_ANDROID=\"$project_tooling/platform/android-35\"\n\
export CLIENT_TOOLS=\"$project_tooling/buildtools/android-15\"\n\
export CLIENT_NDK=\"$project_tooling/ndk/android-ndk-r28c\"\n\
export LD_LIBRARY_PATH=\"$CLIENT_RUST/lib:$CLIENT_TOOLS/lib64:$CLIENT_JDK/lib:$CLIENT_JDK/lib/server:${LD_LIBRARY_PATH:-}\"\n";
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;
const FIVE_GIB: u64 = 5 * 1024 * 1024 * 1024;
const ONE_GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
struct Pin {
    component: String,
    target: Option<String>,
    url: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug)]
struct TarEntry {
    name: String,
    size: u64,
    mode: u32,
    flag: u8,
    linkname: String,
    data: Vec<u8>,
}

enum Json {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

struct Digester {
    state: [u32; 8],
    buffer: [u8; 64],
    filled: usize,
    bits: u64,
}

impl Digester {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0; 64],
            filled: 0,
            bits: 0,
        }
    }
    fn update(&mut self, mut data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.bits = self.bits.wrapping_add((data.len() as u64).wrapping_mul(8));
        if self.filled > 0 {
            let need = 64 - self.filled;
            if data.len() < need {
                self.buffer[self.filled..self.filled + data.len()].copy_from_slice(data);
                self.filled += data.len();
                return;
            }
            self.buffer[self.filled..64].copy_from_slice(&data[..need]);
            compress_block(&mut self.state, &self.buffer);
            data = &data[need..];
            self.filled = 0;
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            compress_block(&mut self.state, &block);
            data = &data[64..];
        }
        self.buffer[..data.len()].copy_from_slice(data);
        self.filled = data.len();
    }
    fn finish(mut self) -> [u8; 32] {
        let mut block = [0u8; 64];
        block[..self.filled].copy_from_slice(&self.buffer[..self.filled]);
        block[self.filled] = 0x80;
        if self.filled + 1 > 56 {
            compress_block(&mut self.state, &block);
            block = [0u8; 64];
        }
        block[56..64].copy_from_slice(&self.bits.to_be_bytes());
        compress_block(&mut self.state, &block);
        let mut out = [0u8; 32];
        for (index, value) in self.state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        out
    }
}

fn compress_block(state: &mut [u32; 8], chunk: &[u8; 64]) {
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
    let mut words = [0u32; 64];
    for index in 0..16 {
        words[index] = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
    }
    for index in 16..64 {
        let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
        let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
        words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
    }
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];
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

fn display(path: &Path) -> String {
    path.display().to_string()
}

fn key_error(key: &str) -> PyErr {
    PyErr { class: "KeyError", message: py_repr(key), caught: false }
}

fn bare_assert() -> PyErr {
    PyErr::uncaught("AssertionError", String::new())
}

fn assert_msg(message: &str) -> PyErr {
    PyErr::uncaught("AssertionError", message.to_string())
}

fn read_error(message: &str) -> PyErr {
    PyErr::uncaught("ReadError", message.to_string())
}

fn incomplete(path: &Path) -> PyErr {
    assert_msg(&format!("(PosixPath({}), 'incomplete extraction')", py_repr(&display(path))))
}

fn file_digest(path: &Path) -> Result<String, PyErr> {
    let mut file = File::open(path).map_err(|err| io_err(&err, &display(path)))?;
    let mut digester = Digester::new();
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    loop {
        let count = file.read(&mut buf).map_err(|err| io_err(&err, &display(path)))?;
        if count == 0 {
            break;
        }
        digester.update(&buf[..count]);
    }
    Ok(hex_digest(&digester.finish()))
}

fn verified(path: &Path, pin: &Pin) -> Result<bool, PyErr> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(io_err(&err, &display(path))),
    };
    if !meta.is_file() || meta.len() != pin.bytes {
        return Ok(false);
    }
    Ok(file_digest(path)? == pin.sha256)
}

fn sibling(path: &Path, extra: &str) -> PathBuf {
    let name = path.file_name().map(|item| item.to_string_lossy().into_owned()).unwrap_or_else(|| "download".into());
    path.parent().unwrap_or_else(|| Path::new(".")).join(format!("{name}{extra}"))
}

fn preserve(path: &Path) -> Result<(), PyErr> {
    if !path.exists() {
        return Ok(());
    }
    let name = path.file_name().map(|item| item.to_string_lossy().into_owned()).unwrap_or_else(|| "download".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut number = 1u32;
    loop {
        let candidate = parent.join(format!("{name}.rejected-{number}"));
        if !candidate.exists() {
            fs::rename(path, &candidate).map_err(|err| io_err(&err, &display(path)))?;
            return Ok(());
        }
        number += 1;
    }
}

fn stream_partial(pin: &Pin, partial: &Path, mut reader: impl Read) -> Result<u64, PyErr> {
    preserve(partial)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(partial)
        .map_err(|err| io_err(&err, &display(partial)))?;
    let mut size = 0u64;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let count = reader.read(&mut buf).map_err(|err| io_err(&err, &display(partial)))?;
        if count == 0 {
            break;
        }
        let next = size + count as u64;
        if next > pin.bytes {
            return Err(PyErr::value(&format!(
                "Official tool download exceeds its pinned size: {}",
                pin.component
            )));
        }
        file.write_all(&buf[..count]).map_err(|err| io_err(&err, &display(partial)))?;
        size = next;
    }
    file.flush().map_err(|err| io_err(&err, &display(partial)))?;
    file.sync_all().map_err(|err| io_err(&err, &display(partial)))?;
    Ok(size)
}

fn commit_partial(pin: &Pin, path: &Path, partial: &Path, size: u64, status_ok: bool) -> Result<(), PyErr> {
    if !status_ok {
        return Err(PyErr::value(&format!("Official tool download failed: {}", pin.component)));
    }
    if size != pin.bytes || file_digest(partial)? != pin.sha256 {
        return Err(PyErr::value(&format!("Persisted official tool bytes differ: {}", pin.component)));
    }
    preserve(path)?;
    fs::rename(partial, path).map_err(|err| io_err(&err, &display(path)))?;
    if let Some(parent) = path.parent() {
        let dir = File::open(parent).map_err(|err| io_err(&err, &display(parent)))?;
        dir.sync_all().map_err(|err| io_err(&err, &display(parent)))?;
    }
    if !verified(path, pin)? {
        return Err(PyErr::value(&format!("Renamed official tool bytes differ: {}", pin.component)));
    }
    Ok(())
}

fn durable_from_reader(pin: &Pin, path: &Path, reader: impl Read) -> Result<(), PyErr> {
    let partial = sibling(path, ".partial");
    let size = stream_partial(pin, &partial, reader)?;
    commit_partial(pin, path, &partial, size, true)
}

fn durable_curl(pin: &Pin, path: &Path) -> Result<(), PyErr> {
    let partial = sibling(path, ".partial");
    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--connect-timeout",
            "15",
            "--max-time",
            "600",
            &pin.url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| io_err(&err, "curl"))?;
    let streamed = match child.stdout.take() {
        Some(stdout) => stream_partial(pin, &partial, stdout),
        None => Err(PyErr::value(&format!("Official tool download failed: {}", pin.component))),
    };
    let result = match streamed {
        Err(err) => Err(err),
        Ok(size) => match child.wait() {
            Ok(status) if status.success() => commit_partial(pin, path, &partial, size, true),
            Ok(_) => Err(PyErr::value(&format!("Official tool download failed: {}", pin.component))),
            Err(err) => Err(io_err(&err, "curl")),
        },
    };
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn retryable(err: &PyErr) -> bool {
    matches!(
        err.class,
        "ValueError" | "OSError" | "FileNotFoundError" | "PermissionError" | "FileExistsError" | "NotADirectoryError" | "IsADirectoryError"
    )
}

fn shown_target(pin: &Pin) -> &str {
    pin.target.as_deref().unwrap_or("")
}

fn filename_target(pin: &Pin) -> &str {
    pin.target.as_deref().unwrap_or("host")
}

fn archive_suffix(url: &str) -> &'static str {
    if url.ends_with(".zip") {
        ".zip"
    } else if url.ends_with(".tar.gz") {
        ".tar.gz"
    } else {
        ".tar.xz"
    }
}

fn cache_name(pin: &Pin) -> String {
    format!("{}-{}{}", pin.component, filename_target(pin), archive_suffix(&pin.url))
}

fn download_with<F>(pin: &Pin, path: &Path, mut fetch: F) -> Result<(), PyErr>
where
    F: FnMut(&Pin, &Path) -> Result<(), PyErr>,
{
    for attempt in 0..2 {
        if verified(path, pin)? {
            println!("Verified persisted official {} {}", pin.component, shown_target(pin));
            let _ = io::stdout().flush();
            return Ok(());
        }
        preserve(path)?;
        match fetch(pin, path) {
            Ok(()) => {}
            Err(err) if retryable(&err) => {
                if attempt == 1 {
                    return Err(err);
                }
                println!("Retrying rejected official download once: {} {}", pin.component, err.message);
                let _ = io::stdout().flush();
            }
            Err(err) => return Err(err),
        }
    }
    if !verified(path, pin)? {
        return Err(PyErr::value(&format!(
            "Official tool download could not be persisted: {}",
            pin.component
        )));
    }
    println!("Verified persisted official {} {}", pin.component, shown_target(pin));
    let _ = io::stdout().flush();
    Ok(())
}

fn download_one(pin: &Pin, path: &Path) -> Result<(), PyErr> {
    download_with(pin, path, |pin, path| durable_curl(pin, path))
}

fn download_all(pins: &[Pin], downloads: &Path) -> Result<(), PyErr> {
    let mut index = 0;
    while index < pins.len() {
        let end = (index + 4).min(pins.len());
        let slice = &pins[index..end];
        let mut outcomes = Vec::new();
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for pin in slice {
                let path = downloads.join(cache_name(pin));
                handles.push(scope.spawn(move || download_one(pin, &path)));
            }
            for handle in handles {
                outcomes.push(handle.join().unwrap_or_else(|_| {
                    Err(PyErr::value("Official tool download could not be persisted: thread"))
                }));
            }
        });
        for outcome in outcomes {
            outcome?;
        }
        index = end;
    }
    Ok(())
}

fn posix_parts(name: &str) -> Vec<String> {
    name.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(|part| part.to_string())
        .collect()
}

fn contained_name(name: &str) -> Result<Vec<String>, PyErr> {
    let parts = posix_parts(name);
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || parts.iter().any(|part| part == "..") {
        return Err(assert_msg(name));
    }
    Ok(parts)
}

fn posix_parent(path: &str) -> String {
    let trimmed = if path.len() > 1 { path.trim_end_matches('/') } else { path };
    match trimmed.rfind('/') {
        Some(0) => "/".to_string(),
        Some(index) => trimmed[..index].to_string(),
        None => ".".to_string(),
    }
}

fn posix_join(left: &str, right: &str) -> String {
    if right.starts_with('/') {
        return right.to_string();
    }
    if left.is_empty() {
        return right.to_string();
    }
    if left.ends_with('/') {
        format!("{left}{right}")
    } else {
        format!("{left}/{right}")
    }
}

fn posix_norm(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if out.last().copied() == Some("..") || out.is_empty() {
                if !absolute {
                    out.push("..");
                }
            } else {
                out.pop();
            }
        } else {
            out.push(part);
        }
    }
    if absolute {
        format!("/{}", out.join("/"))
    } else if out.is_empty() {
        ".".to_string()
    } else {
        out.join("/")
    }
}

fn resolved_link(member: &str, target: &str) -> String {
    posix_norm(&posix_join(&posix_parent(member), target))
}

fn join_parts(dest: &Path, parts: &[String]) -> PathBuf {
    let mut path = dest.to_path_buf();
    for part in parts {
        path.push(part);
    }
    path
}

fn chmod_extracted(path: &Path, mode: u32) -> Result<(), PyErr> {
    let bits = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(bits)).map_err(|err| io_err(&err, &display(path)))
}

fn create_links(dest: &Path, links: &[(Vec<String>, String, bool)]) -> Result<(), PyErr> {
    for (parts, target, hard) in links {
        let path = join_parts(dest, parts);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| io_err(&err, &display(parent)))?;
        }
        let is_link = fs::symlink_metadata(&path).map(|meta| meta.file_type().is_symlink()).unwrap_or(false);
        if is_link || path.exists() {
            let same = is_link
                && fs::read_link(&path)
                    .ok()
                    .and_then(|item| item.into_os_string().into_string().ok())
                    .as_deref()
                    == Some(target.as_str());
            if !same {
                return Err(bare_assert());
            }
            continue;
        }
        if *hard {
            fs::hard_link(dest.join(target), &path).map_err(|err| io_err(&err, &display(&path)))?;
        } else {
            symlink(target, &path).map_err(|err| io_err(&err, &display(&path)))?;
        }
    }
    Ok(())
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
            external: source.external,
        }
    }
}

fn looks_like_zip(data: &[u8]) -> bool {
    if data.len() < 22 {
        return false;
    }
    let max_back = data.len().min(22 + 65535);
    let start = data.len() - max_back;
    let mut index = data.len() - 22;
    loop {
        if data[index..index + 4] == [0x50, 0x4b, 0x05, 0x06] {
            let comment = u16::from_le_bytes(data[index + 20..index + 22].try_into().unwrap()) as usize;
            if index + 22 + comment == data.len() {
                return true;
            }
        }
        if index == start {
            break;
        }
        index -= 1;
    }
    false
}

fn extract_zip(archive: &Archive, dest: &Path) -> Result<usize, PyErr> {
    if archive.members.len() >= 30000 {
        return Err(bare_assert());
    }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut links = Vec::new();
    for index in 0..archive.members.len() {
        let member = archive.copy_member(index);
        let parts = contained_name(&member.name)?;
        if !seen.insert(member.name.clone()) {
            return Err(bare_assert());
        }
        total = total.saturating_add(member.uncompressed_size);
        if total >= FIVE_GIB || member.uncompressed_size >= ONE_GIB {
            return Err(bare_assert());
        }
        let kind = (member.external >> 16) & 0o170000;
        if kind != 0 && kind != S_IFREG && kind != S_IFDIR && kind != S_IFLNK {
            return Err(bare_assert());
        }
        if kind == S_IFLNK {
            let raw = archive.read_member(&member)?;
            let target = std::str::from_utf8(&raw).map_err(|_| {
                PyErr::uncaught("UnicodeDecodeError", "symlink target is not valid UTF-8".into())
            })?;
            let resolved = resolved_link(&member.name, target);
            contained_name(&resolved)?;
            links.push((parts, target.to_string(), false));
            continue;
        }
        let target_path = join_parts(dest, &parts);
        if member.name.ends_with('/') {
            fs::create_dir_all(&target_path).map_err(|err| io_err(&err, &display(&target_path)))?;
            continue;
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|err| io_err(&err, &display(parent)))?;
        }
        let bytes = archive.read_member(&member)?;
        fs::write(&target_path, &bytes).map_err(|err| io_err(&err, &display(&target_path)))?;
        let written = fs::metadata(&target_path).map_err(|err| io_err(&err, &display(&target_path)))?.len();
        if written != member.uncompressed_size {
            return Err(incomplete(&target_path));
        }
        chmod_extracted(&target_path, member.external >> 16)?;
    }
    create_links(dest, &links)?;
    Ok(seen.len())
}

fn tar_checksum(block: &[u8]) -> u32 {
    let mut sum = 0u32;
    for (index, &byte) in block.iter().enumerate() {
        if (148..156).contains(&index) {
            sum += 32;
        } else {
            sum += byte as u32;
        }
    }
    sum
}

fn parse_tar_number(field: &[u8]) -> Option<u64> {
    if field.is_empty() {
        return None;
    }
    if field[0] & 0x80 != 0 {
        let mut value = (field[0] & 0x7f) as u128;
        for &byte in &field[1..] {
            value = (value << 8) | byte as u128;
        }
        if value > u64::MAX as u128 {
            return None;
        }
        return Some(value as u64);
    }
    let mut value = 0u64;
    let mut saw = false;
    for &byte in field {
        if byte == 0 || byte == b' ' {
            if saw {
                break;
            }
            continue;
        }
        if !matches!(byte, b'0'..=b'7') {
            return None;
        }
        saw = true;
        value = value * 8 + (byte - b'0') as u64;
    }
    Some(value)
}

fn cstr_field(bytes: &[u8]) -> Result<String, PyErr> {
    let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).map_err(|_| {
        PyErr::uncaught("UnicodeDecodeError", "tar header is not valid UTF-8".into())
    })
}

fn parse_tar(data: &[u8]) -> Result<Vec<TarEntry>, PyErr> {
    if data.is_empty() {
        return Err(read_error("truncated header"));
    }
    let mut offset = 0usize;
    let mut entries = Vec::new();
    let mut long_name: Option<String> = None;
    let mut long_link: Option<String> = None;
    loop {
        if offset == data.len() {
            break;
        }
        if offset + 512 > data.len() {
            if offset == 0 {
                return Err(read_error("truncated header"));
            }
            return Err(read_error("unexpected end of data"));
        }
        let block = &data[offset..offset + 512];
        let header_at = offset;
        offset += 512;
        if block.iter().all(|&byte| byte == 0) {
            break;
        }
        let stored = parse_tar_number(&block[148..156]).unwrap_or(u64::MAX);
        if stored != tar_checksum(block) as u64 {
            if header_at == 0 {
                return Err(read_error("bad checksum"));
            }
            return Err(PyErr::uncaught("InvalidHeaderError", "bad checksum".into()));
        }
        let flag = block[156];
        let size = parse_tar_number(&block[124..136]).unwrap_or(0);
        let mode = parse_tar_number(&block[100..108]).unwrap_or(0o644) as u32;
        let name_field = cstr_field(&block[0..100])?;
        let link_field = cstr_field(&block[157..257])?;
        let prefix = if &block[257..262] == b"ustar" {
            cstr_field(&block[345..500])?
        } else {
            String::new()
        };
        let parsed_name = if prefix.is_empty() { name_field } else { format!("{prefix}/{name_field}") };
        let padded = ((size.saturating_add(511)) / 512) * 512;
        if offset + padded as usize > data.len() {
            return Err(read_error("unexpected end of data"));
        }
        let payload = data[offset..offset + size as usize].to_vec();
        offset += padded as usize;
        if flag == b'L' {
            long_name = Some(cstr_field(&payload)?);
            continue;
        }
        if flag == b'K' {
            long_link = Some(cstr_field(&payload)?);
            continue;
        }
        let name = long_name.take().unwrap_or(parsed_name);
        let linkname = long_link.take().unwrap_or(link_field);
        entries.push(TarEntry { name, size, mode, flag, linkname, data: payload });
    }
    Ok(entries)
}

fn extract_tar_entries(entries: &[TarEntry], dest: &Path) -> Result<usize, PyErr> {
    if entries.len() >= 30000 {
        return Err(bare_assert());
    }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut links = Vec::new();
    for entry in entries {
        let parts = contained_name(&entry.name)?;
        if !seen.insert(entry.name.clone()) {
            return Err(bare_assert());
        }
        total = total.saturating_add(entry.size);
        if total >= FIVE_GIB || entry.size >= ONE_GIB {
            return Err(bare_assert());
        }
        let regular = entry.flag == 0 || entry.flag == b'0';
        let directory = entry.flag == b'5';
        let symlink_entry = entry.flag == b'2';
        let hardlink = entry.flag == b'1';
        if !regular && !directory && !symlink_entry && !hardlink {
            return Err(assert_msg(&entry.name));
        }
        if symlink_entry || hardlink {
            let resolved = if symlink_entry {
                resolved_link(&entry.name, &entry.linkname)
            } else {
                entry.linkname.clone()
            };
            contained_name(&resolved)?;
            links.push((parts, entry.linkname.clone(), hardlink));
            continue;
        }
        let target_path = join_parts(dest, &parts);
        if directory {
            fs::create_dir_all(&target_path).map_err(|err| io_err(&err, &display(&target_path)))?;
            continue;
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|err| io_err(&err, &display(parent)))?;
        }
        fs::write(&target_path, &entry.data).map_err(|err| io_err(&err, &display(&target_path)))?;
        let written = fs::metadata(&target_path).map_err(|err| io_err(&err, &display(&target_path)))?.len();
        if written != entry.size {
            return Err(incomplete(&target_path));
        }
        chmod_extracted(&target_path, entry.mode)?;
    }
    create_links(dest, &links)?;
    Ok(seen.len())
}

fn is_gzip(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b
}

fn is_xz(data: &[u8]) -> bool {
    data.len() >= 6 && data[..6] == [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]
}

fn decompress_tool(tool: &str, path: &Path) -> Result<Vec<u8>, PyErr> {
    let output = Command::new(tool).arg("-dc").arg(path).output().map_err(|err| io_err(&err, tool))?;
    if !output.status.success() {
        return Err(read_error("unexpected end of data"));
    }
    Ok(output.stdout)
}

fn extract_archive(path: &Path, dest: &Path) -> Result<usize, PyErr> {
    fs::create_dir_all(dest).map_err(|err| io_err(&err, &display(dest)))?;
    let data = fs::read(path).map_err(|err| io_err(&err, &display(path)))?;
    if looks_like_zip(&data) {
        let archive = Archive::parse(data)?;
        return extract_zip(&archive, dest);
    }
    let plain = if is_gzip(&data) {
        decompress_tool("gzip", path)?
    } else if is_xz(&data) {
        decompress_tool("xz", path)?
    } else {
        data
    };
    let entries = parse_tar(&plain)?;
    extract_tar_entries(&entries, dest)
}

fn extract_verified(pin: &Pin, path: &Path, base: &Path) -> Result<(), PyErr> {
    if !verified(path, pin)? {
        download_one(pin, path)?;
    }
    let folder = if pin.component.starts_with("rust") {
        "components".to_string()
    } else {
        pin.component.clone()
    };
    let dest = base.join("tooling").join(folder);
    let count = extract_archive(path, &dest)?;
    let file_name = path.file_name().map(|item| item.to_string_lossy().into_owned()).unwrap_or_default();
    println!("Extracted contained {file_name} {count} members");
    let _ = io::stdout().flush();
    Ok(())
}

fn mkdir_exist(path: &Path) -> Result<(), PyErr> {
    if path.is_dir() {
        return Ok(());
    }
    fs::create_dir(path).map_err(|err| io_err(&err, &display(path)))
}

fn copy_children(src: &Path, dst: &Path) -> Result<(), PyErr> {
    let entries = fs::read_dir(src).map_err(|err| io_err(&err, &display(src)))?;
    for entry in entries {
        let entry = entry.map_err(|err| io_err(&err, &display(src)))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = fs::symlink_metadata(&from).map_err(|err| io_err(&err, &display(&from)))?;
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&from).map_err(|err| io_err(&err, &display(&from)))?;
            if to.symlink_metadata().is_ok() {
                fs::remove_file(&to).map_err(|err| io_err(&err, &display(&to)))?;
            }
            symlink(&target, &to).map_err(|err| io_err(&err, &display(&to)))?;
        } else if meta.is_dir() {
            if !to.exists() {
                fs::create_dir(&to).map_err(|err| io_err(&err, &display(&to)))?;
            }
            copy_children(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|err| io_err(&err, &display(&to)))?;
            fs::set_permissions(&to, meta.permissions()).map_err(|err| io_err(&err, &display(&to)))?;
        }
    }
    Ok(())
}

fn copytree(src: &Path, dst: &Path) -> Result<(), PyErr> {
    let meta = fs::symlink_metadata(src).map_err(|err| io_err(&err, &display(src)))?;
    if !meta.is_dir() {
        return Err(io_err(&io::Error::from_raw_os_error(20), &display(src)));
    }
    if !dst.exists() {
        fs::create_dir(dst).map_err(|err| io_err(&err, &display(dst)))?;
    }
    copy_children(src, dst)
}

fn install_rust(base: &Path, pins: &[Pin]) -> Result<(), PyErr> {
    let rust = base.join("tooling/rust");
    mkdir_exist(&rust)?;
    for pin in pins {
        if !pin.component.starts_with("rust") {
            continue;
        }
        let target = pin.target.clone().ok_or_else(|| key_error("target"))?;
        let outer = base.join("tooling/components").join(format!("{}-1.99.1-{target}", pin.component));
        let inner_name = if pin.component == "rustc" {
            "rustc".to_string()
        } else {
            format!("rust-std-{target}")
        };
        copytree(&outer.join(inner_name), &rust)?;
    }
    Ok(())
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> JsonParser<'a> {
    fn skip_ws(&mut self) {
        while let Some(byte) = self.bytes.get(self.index) {
            if byte.is_ascii_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }
    fn peek(&self) -> Result<u8, PyErr> {
        self.bytes.get(self.index).copied().ok_or_else(|| {
            PyErr::uncaught("json.decoder.JSONDecodeError", "Expecting value".into())
        })
    }
    fn parse_value(&mut self) -> Result<Json, PyErr> {
        self.skip_ws();
        match self.peek()? {
            b'n' => self.consume_literal(b"null", Json::Null),
            b't' => self.consume_literal(b"true", Json::Bool(true)),
            b'f' => self.consume_literal(b"false", Json::Bool(false)),
            b'"' => Ok(Json::String(self.parse_string()?)),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => Ok(Json::Number(self.parse_number()?)),
            _ => Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Expecting value".into())),
        }
    }
    fn consume_literal(&mut self, literal: &[u8], value: Json) -> Result<Json, PyErr> {
        if self.bytes.get(self.index..self.index + literal.len()) == Some(literal) {
            self.index += literal.len();
            Ok(value)
        } else {
            Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Expecting value".into()))
        }
    }
    fn parse_string(&mut self) -> Result<String, PyErr> {
        self.index += 1;
        let mut out = String::new();
        while self.index < self.bytes.len() {
            let byte = self.bytes[self.index];
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.bytes.get(self.index).copied().ok_or_else(|| {
                        PyErr::uncaught("json.decoder.JSONDecodeError", "Unterminated string".into())
                    })?;
                    self.index += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => out.push(escaped as char),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.bytes.get(self.index..self.index + 4).ok_or_else(|| {
                                PyErr::uncaught("json.decoder.JSONDecodeError", "Invalid \\uXXXX escape".into())
                            })?;
                            let text = std::str::from_utf8(hex).map_err(|_| {
                                PyErr::uncaught("json.decoder.JSONDecodeError", "Invalid \\uXXXX escape".into())
                            })?;
                            let code = u32::from_str_radix(text, 16).map_err(|_| {
                                PyErr::uncaught("json.decoder.JSONDecodeError", "Invalid \\uXXXX escape".into())
                            })?;
                            out.push(char::from_u32(code).ok_or_else(|| {
                                PyErr::uncaught("json.decoder.JSONDecodeError", "Invalid \\uXXXX escape".into())
                            })?);
                            self.index += 4;
                        }
                        _ => return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Invalid escape".into())),
                    }
                }
                byte => out.push(byte as char),
            }
        }
        Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Unterminated string".into()))
    }
    fn parse_number(&mut self) -> Result<u64, PyErr> {
        let start = self.index;
        if self.bytes.get(self.index) == Some(&b'-') {
            return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "negative size".into()));
        }
        while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if matches!(self.bytes.get(self.index), Some(b'.' | b'e' | b'E')) {
            return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "non-integer size".into()));
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).unwrap_or("");
        text.parse::<u64>().map_err(|_| PyErr::uncaught("json.decoder.JSONDecodeError", "invalid number".into()))
    }
    fn parse_array(&mut self) -> Result<Json, PyErr> {
        self.index += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek()? == b']' {
            self.index += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.index += 1,
                b']' => {
                    self.index += 1;
                    break;
                }
                _ => return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Expecting ',' delimiter".into())),
            }
        }
        Ok(Json::Array(items))
    }
    fn parse_object(&mut self) -> Result<Json, PyErr> {
        self.index += 1;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.peek()? == b'}' {
            self.index += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip_ws();
            if self.peek()? != b'"' {
                return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Expecting property name".into()));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek()? != b':' {
                return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Expecting ':' delimiter".into()));
            }
            self.index += 1;
            fields.push((key, self.parse_value()?));
            self.skip_ws();
            match self.peek()? {
                b',' => self.index += 1,
                b'}' => {
                    self.index += 1;
                    break;
                }
                _ => return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Expecting ',' delimiter".into())),
            }
        }
        Ok(Json::Object(fields))
    }
}

fn parse_json(text: &str) -> Result<Json, PyErr> {
    let mut parser = JsonParser { bytes: text.as_bytes(), index: 0 };
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.index != parser.bytes.len() {
        return Err(PyErr::uncaught("json.decoder.JSONDecodeError", "Extra data".into()));
    }
    Ok(value)
}

fn object_get<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Json::Object(fields) => fields.iter().find(|(name, _)| name == key).map(|(_, item)| item),
        _ => None,
    }
}

fn load_pins(text: &str) -> Result<Vec<Pin>, PyErr> {
    let value = parse_json(text)?;
    let tools = object_get(&value, "tools").ok_or_else(|| key_error("tools"))?;
    let Json::Array(items) = tools else {
        return Err(PyErr::uncaught("TypeError", "tools is not a list".into()));
    };
    let mut pins = Vec::new();
    for item in items {
        let component = match object_get(item, "component") {
            Some(Json::String(text)) => text.clone(),
            None => return Err(key_error("component")),
            Some(_) => return Err(PyErr::uncaught("TypeError", "component is not a string".into())),
        };
        let url = match object_get(item, "url") {
            Some(Json::String(text)) => text.clone(),
            None => return Err(key_error("url")),
            Some(_) => return Err(PyErr::uncaught("TypeError", "url is not a string".into())),
        };
        let sha256 = match object_get(item, "sha256") {
            Some(Json::String(text)) => text.clone(),
            None => return Err(key_error("sha256")),
            Some(_) => return Err(PyErr::uncaught("TypeError", "sha256 is not a string".into())),
        };
        let bytes = match object_get(item, "bytes").or_else(|| object_get(item, "size")) {
            Some(Json::Number(value)) => *value,
            None => return Err(key_error("bytes")),
            Some(_) => return Err(PyErr::uncaught("TypeError", "bytes is not an integer".into())),
        };
        let target = match object_get(item, "target") {
            Some(Json::String(text)) => Some(text.clone()),
            None => None,
            Some(_) => return Err(PyErr::uncaught("TypeError", "target is not a string".into())),
        };
        pins.push(Pin { component, target, url, sha256, bytes });
    }
    Ok(pins)
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
            if dir.join("tools/toolchain.rs").is_file() {
                return Ok(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    Err(PyErr::value("cannot locate the RLLM project root from tools/toolchain.rs"))
}

fn restore() -> Result<(), PyErr> {
    // Empty arguments reach this function and write tooling/ under the current
    // directory. Unit tests never call it.
    let base = env::current_dir().map_err(|err| io_err(&err, "."))?;
    let source = project_root()?;
    let downloads = base.join("tooling/downloads");
    fs::create_dir_all(&downloads).map_err(|err| io_err(&err, &display(&downloads)))?;
    let spec = source.join("docs/TOOLCHAIN.json");
    let text = fs::read_to_string(&spec).map_err(|err| io_err(&err, &display(&spec)))?;
    let pins = load_pins(&text)?;
    download_all(&pins, &downloads)?;
    for pin in &pins {
        extract_verified(pin, &downloads.join(cache_name(pin)), &base)?;
    }
    install_rust(&base, &pins)?;
    let env_path = base.join("tooling/env.sh");
    fs::write(&env_path, ENV_SH).map_err(|err| io_err(&err, &display(&env_path)))?;
    println!("Verified toolchain restored.");
    let _ = io::stdout().flush();
    Ok(())
}

fn usage_message(message: &str) -> String {
    format!("{USAGE}\ntoolchain.py: error: {message}\n")
}

fn usage_error(message: &str) -> i32 {
    eprint!("{}", usage_message(message));
    2
}

fn parse_cli(args: &[String]) -> Result<(), i32> {
    let option_end = args.iter().position(|arg| arg == "--").unwrap_or(args.len());
    if args[..option_end].iter().any(|arg| arg == "-h" || arg == "--help") {
        print!("{HELP}");
        let _ = io::stdout().flush();
        return Err(0);
    }
    if !args.is_empty() {
        return Err(usage_error(&format!("unrecognized arguments: {}", args.join(" "))));
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match parse_cli(&args) {
        Err(code) => code,
        Ok(()) => match restore() {
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
fn write_octal(field: &mut [u8], value: u64) {
    let text = format!("{value:o}");
    let width = field.len();
    assert!(text.len() < width);
    field[..width - 1].fill(b'0');
    let start = width - 1 - text.len();
    field[start..width - 1].copy_from_slice(text.as_bytes());
    field[width - 1] = 0;
}

#[cfg(test)]
fn tar_block(name: &str, size: u64, typeflag: u8, linkname: &str, mode: u32) -> [u8; 512] {
    assert!(name.len() <= 100, "tar name fits the ustar field");
    assert!(linkname.len() <= 100, "tar linkname fits the ustar field");
    let mut buf = [0u8; 512];
    buf[..name.len()].copy_from_slice(name.as_bytes());
    write_octal(&mut buf[100..108], mode as u64);
    write_octal(&mut buf[108..116], 0);
    write_octal(&mut buf[116..124], 0);
    write_octal(&mut buf[124..136], size);
    write_octal(&mut buf[136..148], 0);
    buf[156] = typeflag;
    buf[157..157 + linkname.len()].copy_from_slice(linkname.as_bytes());
    buf[257..262].copy_from_slice(b"ustar");
    buf[262] = 0;
    buf[263..265].copy_from_slice(b"00");
    for byte in &mut buf[148..156] {
        *byte = b' ';
    }
    let sum = tar_checksum(&buf);
    let text = format!("{sum:06o}");
    buf[148..154].copy_from_slice(text.as_bytes());
    buf[154] = 0;
    buf[155] = b' ';
    buf
}

#[cfg(test)]
fn push_tar_member(out: &mut Vec<u8>, name: &str, data: &[u8], flag: u8, link: &str) {
    if name.len() > 100 {
        let mut body = name.as_bytes().to_vec();
        body.push(0);
        out.extend_from_slice(&tar_block("././@LongLink", body.len() as u64, b'L', "", 0o644));
        out.extend_from_slice(&body);
        let pad = (512 - (body.len() % 512)) % 512;
        out.extend(std::iter::repeat(0u8).take(pad));
        let short = &name[..100];
        out.extend_from_slice(&tar_block(short, data.len() as u64, flag, link, 0o644));
    } else {
        out.extend_from_slice(&tar_block(name, data.len() as u64, flag, link, 0o644));
    }
    out.extend_from_slice(data);
    let pad = (512 - (data.len() % 512)) % 512;
    out.extend(std::iter::repeat(0u8).take(pad));
}

#[cfg(test)]
fn finish_tar(members: &[(&str, &[u8], u8, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data, flag, link) in members {
        push_tar_member(&mut out, name, data, *flag, link);
    }
    out.extend(std::iter::repeat(0u8).take(1024));
    out
}

#[cfg(test)]
fn stored_zip(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, data, external) in entries {
        offsets.push(local.len() as u32);
        let name_bytes = name.as_bytes();
        let crc = crc32(data);
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
        central.extend_from_slice(&external.to_le_bytes());
        central.extend_from_slice(&offsets[offsets.len() - 1].to_le_bytes());
        central.extend_from_slice(name_bytes);
    }
    let local_len = local.len() as u32;
    let central_len = central.len() as u32;
    let count = entries.len() as u16;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_len.to_le_bytes());
    out.extend_from_slice(&local_len.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rllm-toolchain-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn checksum_mismatch() {
        assert_eq!(hex_digest(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex_digest(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let dir = scratch("checksum");
        fs::write(dir.join("abc"), b"abc").unwrap();
        fs::write(dir.join("empty"), b"").unwrap();
        assert_eq!(file_digest(&dir.join("abc")).unwrap(), hex_digest(&sha256(b"abc")));
        assert_eq!(file_digest(&dir.join("empty")).unwrap(), hex_digest(&sha256(b"")));
        let pin = Pin {
            component: "demo".into(),
            target: None,
            url: "https://example.invalid/demo.zip".into(),
            sha256: hex_digest(&sha256(b"")),
            bytes: 4,
        };
        assert_eq!(cache_name(&pin), "demo-host.zip");
        let path = dir.join(cache_name(&pin));
        let partial = sibling(&path, ".partial");
        fs::write(&partial, b"OLD!").unwrap();
        let err = durable_from_reader(&pin, &path, &b"abcd"[..]).unwrap_err();
        assert_eq!(err.traceback_line(), "ValueError: Persisted official tool bytes differ: demo");
        assert_eq!(fs::read(&partial).unwrap(), b"abcd");
        assert_eq!(fs::metadata(&partial).unwrap().len(), pin.bytes);
        assert_ne!(file_digest(&partial).unwrap(), pin.sha256);
        let rejected = sibling(&path, ".partial.rejected-1");
        assert_eq!(fs::read(&rejected).unwrap(), b"OLD!");
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated_archive() {
        let dir = scratch("truncated");
        let pin = Pin {
            component: "demo".into(),
            target: None,
            url: "https://example.invalid/demo.tar.xz".into(),
            sha256: hex_digest(&sha256(b"abcd")),
            bytes: 10,
        };
        let path = dir.join(cache_name(&pin));
        let err = durable_from_reader(&pin, &path, &b"abcd"[..]).unwrap_err();
        assert_eq!(err.traceback_line(), "ValueError: Persisted official tool bytes differ: demo");
        let partial = sibling(&path, ".partial");
        assert_eq!(fs::read(&partial).unwrap(), b"abcd");
        assert_ne!(fs::metadata(&partial).unwrap().len(), pin.bytes);
        let raw = {
            let mut bytes = tar_block("file.txt", 10, b'0', "", 0o644).to_vec();
            bytes.extend_from_slice(b"abcd");
            bytes
        };
        let err = parse_tar(&raw).unwrap_err();
        assert_eq!(err.traceback_line(), "ReadError: unexpected end of data");
        let archive = dir.join("broken.tar");
        fs::write(&archive, &raw).unwrap();
        let dest = dir.join("out");
        let err = extract_archive(&archive, &dest).unwrap_err();
        assert_eq!(err.traceback_line(), "ReadError: unexpected end of data");
        assert!(!dest.join("file.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extraction_path_escape() {
        assert_eq!(resolved_link("a/b/link", "../c"), "a/c");
        assert!(contained_name("a/c").is_ok());
        assert_eq!(resolved_link("top", "../x"), "../x");
        assert_eq!(contained_name("../x").unwrap_err().traceback_line(), "AssertionError: ../x");
        assert_eq!(resolved_link("jdk/lib/link", "../java.base/LICENSE"), "jdk/java.base/LICENSE");
        assert!(contained_name("jdk/java.base/LICENSE").is_ok());
        assert_eq!(resolved_link("dir/sub/link", "../../../../tmp/x"), "../../tmp/x");
        assert_eq!(contained_name("../../tmp/x").unwrap_err().traceback_line(), "AssertionError: ../../tmp/x");
        assert_eq!(posix_norm("/a/../../b"), "/b");
        assert_eq!(posix_join(".", "../x"), "./../x");
        assert_eq!(posix_parent("foo/../bar"), "foo/..");
        assert_eq!(posix_parts("foo/./bar"), vec!["foo".to_string(), "bar".to_string()]);
        assert!(posix_parts(".").is_empty());
        assert!(contained_name(".").is_ok());

        let dir = scratch("escape");
        let outside = dir.join("escape");
        let tar_path = dir.join("escape.tar");
        fs::write(&tar_path, finish_tar(&[("../escape", b"nope", b'0', "")])).unwrap();
        let dest = dir.join("dest");
        let err = extract_archive(&tar_path, &dest).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: ../escape");
        assert!(!outside.exists());
        assert!(!dir.join("dest").join("escape").exists());

        let zip_path = dir.join("escape.zip");
        fs::write(&zip_path, stored_zip(&[("../escape", b"nope", 0)])).unwrap();
        let zip_dest = dir.join("zipdest");
        let err = extract_archive(&zip_path, &zip_dest).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: ../escape");
        assert!(!outside.exists());

        let ok_tar = dir.join("ok.tar");
        fs::write(&ok_tar, finish_tar(&[("ok.txt", b"yes", b'0', "")])).unwrap();
        let ok_dest = dir.join("ok");
        assert_eq!(extract_archive(&ok_tar, &ok_dest).unwrap(), 1);
        assert_eq!(fs::read(ok_dest.join("ok.txt")).unwrap(), b"yes");

        let link_tar = dir.join("link.tar");
        fs::write(&link_tar, finish_tar(&[("link", b"", b'2', "../out")])).unwrap();
        let link_dest = dir.join("linkdest");
        let err = extract_archive(&link_tar, &link_dest).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: ../out");
        assert!(!dir.join("out").exists());
        assert!(!link_dest.join("link").exists());

        let hard_tar = dir.join("hard.tar");
        fs::write(&hard_tar, finish_tar(&[("h", b"", b'1', "../x")])).unwrap();
        let err = extract_archive(&hard_tar, &dir.join("harddest")).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: ../x");
        assert!(!dir.join("x").exists());

        let zip_link = dir.join("ziplink.zip");
        let external = S_IFLNK << 16;
        fs::write(&zip_link, stored_zip(&[("link", b"../out", external)])).unwrap();
        let err = extract_archive(&zip_link, &dir.join("ziplinkdest")).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: ../out");

        let long_name = format!("deep/{}", "n".repeat(120));
        let long_tar = dir.join("long.tar");
        fs::write(&long_tar, finish_tar(&[(&long_name, b"long", b'0', "")])).unwrap();
        let long_dest = dir.join("longdest");
        assert_eq!(extract_archive(&long_tar, &long_dest).unwrap(), 1);
        assert_eq!(fs::read(long_dest.join("deep").join("n".repeat(120))).unwrap(), b"long");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_pinned_entry() {
        let text = "{\"tools\":[{\"component\":\"rustc\",\"target\":\"x86_64-unknown-linux-gnu\",\"url\":\"https://example.invalid/rustc.tar.xz\",\"bytes\":1}]}";
        let err = load_pins(text).unwrap_err();
        assert_eq!(err.traceback_line(), "KeyError: 'sha256'");
        let root = project_root().unwrap();
        let pins = load_pins(&fs::read_to_string(root.join("docs/TOOLCHAIN.json")).unwrap()).unwrap();
        assert_eq!(pins.len(), 8);
        assert_eq!(pins[0].component, "rustc");
        assert_eq!(cache_name(&pins[0]), "rustc-x86_64-unknown-linux-gnu.tar.xz");
        assert_eq!(pins[4].component, "buildtools");
        assert!(pins[4].target.is_none());
        assert_eq!(cache_name(&pins[4]), "buildtools-host.zip");
        assert_eq!(pins[7].component, "ndk");
        assert_eq!(pins[7].bytes, 722261334);
        assert_eq!(fs::read_to_string(root.join("tooling/env.sh")).unwrap(), ENV_SH);
        let dir = scratch("missing");
        fs::create_dir_all(dir.join("tooling")).unwrap();
        let pin = Pin {
            component: "rustc".into(),
            target: Some("x86_64-unknown-linux-gnu".into()),
            url: "https://example.invalid/rustc.tar.xz".into(),
            sha256: "ab".into(),
            bytes: 1,
        };
        let err = install_rust(&dir, &[pin]).unwrap_err();
        let expected = dir.join("tooling/components/rustc-1.99.1-x86_64-unknown-linux-gnu/rustc");
        assert_eq!(
            err.traceback_line(),
            format!("FileNotFoundError: [Errno 2] No such file or directory: {}", py_repr(&display(&expected)))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bounded_retry_once() {
        let dir = scratch("retry");
        let payload = b"abc";
        let pin = Pin {
            component: "demo".into(),
            target: Some("tgt".into()),
            url: "https://example.invalid/demo.tar.xz".into(),
            sha256: hex_digest(&sha256(payload)),
            bytes: 3,
        };
        let path = dir.join(cache_name(&pin));
        let mut calls = 0;
        download_with(&pin, &path, |pin, path| {
            calls += 1;
            if calls == 1 {
                return Err(PyErr::value("Official tool download failed: demo"));
            }
            durable_from_reader(pin, path, &payload[..])
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert!(verified(&path, &pin).unwrap());
        calls = 0;
        let err = download_with(&pin, &dir.join("second"), |_, _| {
            calls += 1;
            Err(PyErr::value("Official tool download failed: demo"))
        })
        .unwrap_err();
        assert_eq!(calls, 2);
        assert_eq!(err.traceback_line(), "ValueError: Official tool download failed: demo");
        calls = 0;
        let err = download_with(&pin, &dir.join("third"), |_, _| {
            calls += 1;
            Err(assert_msg("nope"))
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(err.traceback_line(), "AssertionError: nope");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn argument_refusal() {
        assert_eq!(
            HELP,
            "usage: toolchain.py [-h]\n\noptions:\n  -h, --help  show this help message and exit\n"
        );
        assert_eq!(
            usage_message("unrecognized arguments: -- -h"),
            "usage: toolchain.py [-h]\ntoolchain.py: error: unrecognized arguments: -- -h\n"
        );
        assert!(parse_cli(&[]).is_ok());
        assert_eq!(parse_cli(&["-h".into()]).unwrap_err(), 0);
        assert_eq!(parse_cli(&["-h".into(), "nope".into()]).unwrap_err(), 0);
        assert_eq!(parse_cli(&["--".into(), "-h".into()]).unwrap_err(), 2);
        assert_eq!(parse_cli(&["nope".into()]).unwrap_err(), 2);
        assert_eq!(format!("Verified persisted official {} {}", "buildtools", ""), "Verified persisted official buildtools ");
    }
}

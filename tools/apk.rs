// Replacement for tools/apk.py. Run the native binary from
// tools/build-tools.sh. Do not run the Python file.
//
// Flags, usage text and exit codes match that program: the APK path and
// --version are required, help is on stdout, and a bad argument prints
// usage on stderr and exits 2. Inspection failures keep the Python
// exception line and exit 1.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
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
const V2_LINE: &str = "Verified using v2 scheme (APK Signature Scheme v2): true";
const V3_LINE: &str = "Verified using v3 scheme (APK Signature Scheme v3): true";
const ALIGNER: &str = "tools/alignment.rs";
const USAGE: &str = "usage: apk.py [-h] --version VERSION apk";
const HELP: &str = "\
usage: apk.py [-h] --version VERSION apk

positional arguments:
  apk

options:
  -h, --help         show this help message and exit
  --version VERSION
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
    fn names(&self) -> Vec<String> {
        self.members.iter().map(|member| member.name.clone()).collect()
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
    fn read_name(&self, name: &str) -> Result<Vec<u8>, PyErr> {
        let index = self.members.iter().rposition(|member| member.name == name).ok_or_else(|| {
            let sentence = format!("There is no item named {} in the archive", py_repr(name));
            PyErr { class: "KeyError", message: format!("\"{sentence}\"") }
        })?;
        let member = self.copy_member(index);
        self.read_member(&member)
    }
}

fn bare() -> PyErr {
    PyErr { class: "AssertionError", message: String::new() }
}

fn assertion(message: &str) -> PyErr {
    PyErr { class: "AssertionError", message: message.to_string() }
}

fn env_key(name: &str) -> Result<String, PyErr> {
    env::var(name).map_err(|_| PyErr { class: "KeyError", message: py_repr(name) })
}

fn py_bytes(data: &[u8]) -> String {
    let quote = if !data.contains(&b'\'') || data.contains(&b'"') { '\'' } else { '"' };
    let mut out = String::from("b");
    out.push(quote);
    for &byte in data {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\'' if quote == '\'' => out.push_str("\\'"),
            b'"' if quote == '"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push(quote);
    out
}

fn marker_pair(abi: &str, marker: &[u8]) -> PyErr {
    assertion(&format!("({}, {})", py_repr(abi), py_bytes(marker)))
}

fn contains_at(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|window| window == needle)
}

fn require_bytes(hay: &[u8], marker: &[u8], err: PyErr) -> Result<(), PyErr> {
    if contains_at(hay, marker) { Ok(()) } else { Err(err) }
}

fn forbid_bytes(hay: &[u8], marker: &[u8], err: PyErr) -> Result<(), PyErr> {
    if contains_at(hay, marker) { Err(err) } else { Ok(()) }
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
            if dir.join("tools/apk.rs").is_file() {
                return Ok(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    Err(PyErr::value("cannot locate the RLLM project root from tools/apk.rs"))
}

fn rel_posix(root: &Path, path: &Path) -> Result<String, PyErr> {
    let relative = path.strip_prefix(root).map_err(|_| PyErr::value("build input escaped its root"))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let text = component.as_os_str().to_str().ok_or_else(|| {
            PyErr::uncaught("UnicodeEncodeError", "build input path is not UTF-8".into())
        })?;
        parts.push(text.to_string());
    }
    Ok(parts.join("/"))
}

fn collect_files(dir: &Path, root: &Path, out: &mut BTreeSet<String>) -> Result<(), PyErr> {
    let entries = fs::read_dir(dir).map_err(|err| io_err(&err, &dir.display().to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|err| io_err(&err, &dir.display().to_string()))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|err| io_err(&err, &path.display().to_string()))?;
        if meta.file_type().is_symlink() {
            if path.is_file() {
                out.insert(rel_posix(root, &path)?);
            }
            continue;
        }
        if meta.is_dir() {
            collect_files(&path, root, out)?;
        } else if meta.is_file() {
            out.insert(rel_posix(root, &path)?);
        }
    }
    Ok(())
}

fn current_inputs(root: &Path) -> Result<BTreeSet<String>, PyErr> {
    let mut set = BTreeSet::new();
    for directory in ["core", "platform", "res", "notices"] {
        collect_files(&root.join(directory), root, &mut set)?;
    }
    set.insert("AndroidManifest.xml".into());
    set.insert("package.sh".into());
    set.insert(ALIGNER.into());
    Ok(set)
}

fn inventory_changed(recorded: &BTreeSet<String>, current: &BTreeSet<String>) -> Result<(), PyErr> {
    if recorded == current {
        Ok(())
    } else {
        Err(assertion("Build input inventory changed after compilation"))
    }
}

fn digest_shape(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn digest_ok(relative: &str, recorded: &str, present: bool, bytes: &[u8]) -> Result<(), PyErr> {
    let matches = present && digest_shape(recorded) && hex_digest(&sha256(bytes)) == recorded;
    if digest_shape(recorded) && matches {
        Ok(())
    } else {
        Err(assertion(relative))
    }
}

fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' || bytes[index] == b'\r' {
            lines.push(&text[start..index]);
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            index += 1;
            start = index;
        } else {
            index += 1;
        }
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn resolve_loose(path: &Path) -> Result<PathBuf, PyErr> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().map_err(|err| io_err(&err, "."))?.join(path)
    };
    let mut parts = Vec::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if matches!(parts.last(), Some(std::path::Component::Normal(_))) {
                    parts.pop();
                }
            }
            other => parts.push(other),
        }
    }
    let mut resolved = PathBuf::new();
    let mut pending = Vec::new();
    for component in parts {
        if !pending.is_empty() {
            pending.push(component);
            continue;
        }
        let next = resolved.join(component);
        if next.exists() {
            resolved = fs::canonicalize(&next).map_err(|err| io_err(&err, &next.display().to_string()))?;
        } else {
            pending.push(component);
        }
    }
    for component in pending {
        resolved.push(component);
    }
    Ok(resolved)
}

fn subpath(path: &Path, root: &Path) -> Result<String, PyErr> {
    match path.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() || path == root => {
            let mut parts = Vec::new();
            for component in relative.components() {
                let text = component.as_os_str().to_str().ok_or_else(|| {
                    PyErr::uncaught("UnicodeEncodeError", "build input path is not UTF-8".into())
                })?;
                parts.push(text.to_string());
            }
            Ok(parts.join("/"))
        }
        _ => Err(PyErr::value(&format!("'{}' is not in the subpath of '{}'", path.display(), root.display()))),
    }
}

fn check_recorded(root: &Path, digest: &str, name: &str) -> Result<String, PyErr> {
    let source = Path::new(name);
    let resolved = resolve_loose(source)?;
    let root_resolved = resolve_loose(root)?;
    let relative = subpath(&resolved, &root_resolved)?;
    let present = source.is_file();
    let bytes = if present {
        fs::read(source).map_err(|err| io_err(&err, &source.display().to_string()))?
    } else {
        Vec::new()
    };
    digest_ok(&relative, digest, present, &bytes)?;
    Ok(relative)
}

fn elf_machine_matches(raw: &[u8], machine: u16) -> Result<bool, PyErr> {
    if raw.len() < 6 || &raw[..6] != b"\x7fELF\x02\x01" {
        return Ok(false);
    }
    if raw.len() < 20 {
        return Err(PyErr::uncaught("struct.error", format!("unpack_from requires a buffer of at least 20 bytes (actual buffer size is {})", raw.len())));
    }
    Ok(u16::from_le_bytes(raw[18..20].try_into().unwrap()) == machine)
}

fn load_alignments(raw: &[u8]) -> Result<Vec<u64>, PyErr> {
    if raw.len() < 58 {
        return Err(PyErr::uncaught("struct.error", format!("unpack_from requires a buffer of at least 58 bytes (actual buffer size is {})", raw.len())));
    }
    let phoff = u64::from_le_bytes(raw[32..40].try_into().unwrap());
    let ents = u16::from_le_bytes(raw[54..56].try_into().unwrap()) as u64;
    let count = u16::from_le_bytes(raw[56..58].try_into().unwrap()) as u64;
    if ents == 0 && count > 0 {
        return Err(PyErr::uncaught("struct.error", "program header entry size is zero".into()));
    }
    let mut aligns = Vec::new();
    for index in 0..count {
        let at = phoff.checked_add(index.checked_mul(ents).ok_or_else(|| PyErr::uncaught("OverflowError", "ELF program header offset overflow".into()))?).ok_or_else(|| PyErr::uncaught("OverflowError", "ELF program header offset overflow".into()))?;
        let type_at = usize::try_from(at).unwrap_or(usize::MAX);
        let align_at = usize::try_from(at.saturating_add(48)).unwrap_or(usize::MAX);
        if type_at.checked_add(4).map(|end| end > raw.len()).unwrap_or(true) || align_at.checked_add(8).map(|end| end > raw.len()).unwrap_or(true) {
            return Err(PyErr::uncaught("struct.error", format!("unpack_from requires a buffer of at least {} bytes (actual buffer size is {})", align_at.saturating_add(8), raw.len())));
        }
        let kind = u32::from_le_bytes(raw[type_at..type_at + 4].try_into().unwrap());
        if kind == 1 {
            aligns.push(u64::from_le_bytes(raw[align_at..align_at + 8].try_into().unwrap()));
        }
    }
    Ok(aligns)
}

fn require_elf(raw: &[u8], machine: u16) -> Result<Vec<u64>, PyErr> {
    if !elf_machine_matches(raw, machine)? {
        return Err(bare());
    }
    let aligns = load_alignments(raw)?;
    if aligns.is_empty() || aligns.iter().any(|value| *value < 16384) {
        return Err(bare());
    }
    Ok(aligns)
}

fn require_marker(abi: &str, raw: &[u8], marker: &str) -> Result<(), PyErr> {
    let bytes = marker.as_bytes();
    require_bytes(raw, bytes, marker_pair(abi, bytes))
}

fn forbid_marker(abi: &str, raw: &[u8], marker: &str) -> Result<(), PyErr> {
    let bytes = marker.as_bytes();
    forbid_bytes(raw, bytes, marker_pair(abi, bytes))
}

fn native_markers(abi: &str, raw: &[u8]) -> Result<(), PyErr> {
    let groups: &[&[&str]] = &[
        &[
            "embedded PNG textures and alpha cutouts",
            "Scene texture coordinates differ from position count or format",
            "Scene image exceeds its embedded buffer or memory budget",
            "Scene alpha cutoff must be nonnegative",
        ],
        &[
            "Investigation is intrinsic",
            "MODEL AND DELEGATION CONFIGURATION",
            "Delegation result: blocked before start.",
            "PE base-relocation declarations",
            "Returning the operation error to the requesting model",
        ],
        &[
            "GET routes cannot upload a selected file",
            "Input selection is no longer owned",
            "File upload is no longer owned",
            "Media storage reservation exceeded",
            "Artifact or status response exceeds",
            "Service response did not contain a complete generated artifact",
        ],
        &[
            "voice-onset:",
            "voice_interrupt_listen",
            "voice_interruption",
            "voice_watch_expected",
            "Spoken interruption ownership changed",
            "Interrupt replies by speaking",
        ],
        &["Voice conversation stopped after an audio route or focus interruption; review voice to start again"],
        &[
            "Waiting for voice request approval",
            "resume_available",
            "Voice request was not started; resume the microphone when ready",
        ],
        &["Voice request stopped; microphone paused until you resume it"],
        &["Microphone paused while another operation is pending; resume it when finished"],
        &[
            "Review proposed request",
            "Review recovered request",
            "Response details",
            "Alternative source only",
            "No current reply is eligible for request recovery",
        ],
        &[
            "Sign in with device code",
            "Check authorization",
            "Device authorization",
            "Device verification URI contains private protocol fields",
        ],
        &["voice_background", "voice_lease_ms", "Continue voice outside the app"],
        &[
            "Document providers",
            "Provider document selected",
            "cloud_release_result",
            "Selected document grants removed",
        ],
        &[
            "Request framing is ambiguous; no TLS identity was applied",
            "Request headers are incomplete; no TLS identity was applied",
        ],
        &[
            "Voice and microphone",
            "Save microphone preferences",
            "Review open microphone conversation",
            "Key/token header active; username/password retained",
            "Network storage access key exceeds8192bytes",
        ],
        &[
            "Network storage authorization",
            "This network storage connection only; model credentials remain separate",
            "Storage authorization active; manual credentials retained",
            "Network storage authorization destination changed; sign in again",
            "Network storage authorization mode must be a toggle",
        ],
        &[
            "Design brief (Markdown)",
            "Export contents",
            "Design brief exceeds256KiB",
            "Unknown entries without an authored resolution:",
            "The user does not need to fill manual forms first.",
        ],
        &[
            "scene_ticket",
            "Static geometry preview",
            "Scene viewport ownership changed",
            "Scene source bytes changed while loading",
        ],
    ];
    for group in groups {
        for marker in *group {
            require_marker(abi, raw, marker)?;
        }
    }
    for marker in ["RLLM Workspace", "ReverseEngineerAnything", "reverse_engineer_anything"] {
        forbid_marker(abi, raw, marker)?;
    }
    Ok(())
}

fn dex_marker(raw: &[u8], marker: &str, present: bool) -> Result<(), PyErr> {
    let bytes = marker.as_bytes();
    if present {
        require_bytes(raw, bytes, assertion(&py_bytes(bytes)))
    } else {
        forbid_bytes(raw, bytes, assertion(&py_bytes(bytes)))
    }
}

fn dex_markers(raw: &[u8]) -> Result<(), PyErr> {
    let required: &[&[&str]] = &[
        &["media_input_open", "media_input_feed", "media_input_finish", "media_upload_read", "upload_bytes", "media_receipt"],
        &["voice_watch", "voice_watch_start", "voice_watch_unavailable", "voice_interrupt_listen", "Landroid/media/audiofx/AcousticEchoCanceler;", "echo_active"],
        &["Lcom/local/client/AudioRoute;", "setWillPauseWhenDucked", "voice_route_lost", "android.media.AUDIO_BECOMING_NOISY", "resume_available"],
        &["Lcom/local/client/VoiceSession;", "Landroid/os/PowerManager$WakeLock;", "voiceSessionStopped", "startCapture"],
        &["Lcom/local/client/DocumentAccess;", "completeCloudSelection", "selectedDocumentAccess", "cloud_release_result"],
        &["microphone_permissions", "microphone_granted", "android.settings.APPLICATION_DETAILS_SETTINGS"],
    ];
    for group in required {
        for marker in *group {
            dex_marker(raw, marker, true)?;
        }
    }
    for marker in ["com.google.android.apps.docs", "Choose Google Drive", "com.google"] {
        dex_marker(raw, marker, false)?;
    }
    for marker in ["Lcom/local/client/SceneView;", "scene_frame", "scene_close", "Rotate left", "Reset view"] {
        dex_marker(raw, marker, true)?;
    }
    Ok(())
}

fn development_leak(names: &[String]) -> Result<(), PyErr> {
    let leaked = names.iter().any(|name| {
        name.starts_with("verification/") || name.starts_with("tests/") || name.starts_with("docs/") || name.ends_with(".log") || name.ends_with(".py")
    });
    if leaked { Err(assertion("Development files in the signed APK")) } else { Ok(()) }
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
        return Err(PyErr::value(&format!("Delivery APK signing identity differs from the retained certificate: {name}")));
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|text| text.to_string_lossy().into_owned()).unwrap_or_else(|| "apk".into())
}

fn verify_signature(path: &Path) -> Result<(), PyErr> {
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
    let archive = Archive::open(path)?;
    if archive.testzip()?.is_some() {
        return Err(PyErr::value(&format!("APK archive integrity verification failed: {name}")));
    }
    Ok(())
}

fn command_error(argv: &[String], code: i32) -> PyErr {
    let mut list = String::from("[");
    for (index, arg) in argv.iter().enumerate() {
        if index > 0 {
            list.push_str(", ");
        }
        list.push_str(&py_repr(arg));
    }
    list.push(']');
    PyErr {
        class: "subprocess.CalledProcessError",
        message: format!("Command '{list}' returned non-zero exit status {code}."),
    }
}

fn tool_output(argv: &[String]) -> Result<String, PyErr> {
    let output = Command::new(&argv[0]).args(&argv[1..]).output().map_err(|err| io_err(&err, &argv[0]))?;
    if !output.status.success() {
        return Err(command_error(argv, output.status.code().unwrap_or(-1)));
    }
    String::from_utf8(output.stdout).map_err(|_| PyErr::uncaught("UnicodeDecodeError", "'utf-8' codec can't decode tool output".into()))
}

fn tool_status(argv: &[String]) -> Result<(), PyErr> {
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|err| io_err(&err, &argv[0]))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(argv, output.status.code().unwrap_or(-1)))
    }
}

fn badging_ok(text: &str, version: &str) -> bool {
    text.contains("package: name='com.local.client.lab'")
        && text.contains(&format!("versionCode='{version}'"))
        && text.contains("application-label:'RLLM'")
        && text.contains("sdkVersion:'26'")
}

fn is_ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn network_resource_id(manifest: &str) -> Result<String, PyErr> {
    let key = "android:networkSecurityConfig";
    let mut search = 0;
    while let Some(found) = manifest[search..].find(key) {
        let at = search + found + key.len();
        let bytes = manifest.as_bytes();
        let mut index = at;
        while index < bytes.len() && bytes[index] != b'=' {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            if bytes.get(index) == Some(&b'@') {
                index += 1;
                let start = index;
                if bytes.get(index) == Some(&b'0') && bytes.get(index + 1) == Some(&b'x') {
                    index += 2;
                    let hex = index;
                    while index < bytes.len() && matches!(bytes[index], b'0'..=b'9' | b'a'..=b'f') {
                        index += 1;
                    }
                    if index > hex {
                        return Ok(manifest[start..index].to_string());
                    }
                }
            }
        }
        search = at;
    }
    Err(PyErr::uncaught("TypeError", "'NoneType' object is not subscriptable".into()))
}

fn resources_bind(resources: &str, id: &str) -> bool {
    let bytes = resources.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if resources[index..].starts_with("resource") {
            let before_ok = index == 0 || !is_word(bytes[index - 1]);
            let after = index + "resource".len();
            let after_ok = after >= bytes.len() || !is_word(bytes[after]);
            if before_ok && after_ok {
                let mut cursor = after;
                while cursor < bytes.len() && is_ws(bytes[cursor]) {
                    cursor += 1;
                }
                if resources[cursor..].starts_with(id) {
                    cursor += id.len();
                    while cursor < bytes.len() && is_ws(bytes[cursor]) {
                        cursor += 1;
                    }
                    let start = cursor;
                    while cursor < bytes.len() && !is_ws(bytes[cursor]) {
                        cursor += 1;
                    }
                    let chunk = &resources[start..cursor];
                    if let Some(pos) = chunk.find(":xml/network:") {
                        if pos >= 1 {
                            return true;
                        }
                    }
                }
            }
        }
        index += 1;
    }
    false
}

fn split_before<'a>(text: &'a str, needle: &str) -> Vec<&'a str> {
    let mut points = vec![0usize];
    let mut index = 0usize;
    while index + needle.len() <= text.len() {
        if text[index..].starts_with(needle) {
            if index != 0 {
                points.push(index);
            }
            index += needle.len();
        } else {
            index += text[index..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(1);
        }
    }
    points.push(text.len());
    let mut out = Vec::new();
    if text.starts_with(needle) {
        out.push("");
    }
    for pair in points.windows(2) {
        out.push(&text[pair[0]..pair[1]]);
    }
    if out.is_empty() {
        out.push(text);
    }
    out
}

fn voice_block(manifest: &str) -> Result<String, PyErr> {
    let blocks = split_before(manifest, "      E: service ");
    let Some(block) = blocks.into_iter().find(|block| block.contains("com.local.client.VoiceSession")) else {
        return Err(PyErr { class: "StopIteration", message: String::new() });
    };
    let after = block.split("      E: service ").nth(1).unwrap_or(block);
    Ok(after.split("      E: activity ").next().unwrap_or(after).to_string())
}

fn permissions_ok(manifest: &str) -> bool {
    manifest.contains("android.permission.MODIFY_AUDIO_SETTINGS")
        && manifest.contains("android.permission.RECORD_AUDIO")
        && [
            "android.permission.FOREGROUND_SERVICE_MICROPHONE",
            "android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK",
            "android.permission.WAKE_LOCK",
            "com.local.client.VoiceSession",
        ]
        .iter()
        .all(|name| manifest.contains(name))
}

fn voice_ok(block: &str) -> bool {
    block.contains("android:exported(0x01010010)=(type 0x12)0x0")
        && block.contains("android:foregroundServiceType(0x01010599)=(type 0x11)0x82")
}

fn network_ok(text: &str) -> bool {
    text.matches("E: base-config").count() == 1
        && text.matches("E: trust-anchors").count() == 1
        && text.matches("E: certificates").count() == 2
        && !text.contains("E: debug-overrides")
        && ["system", "user"].iter().all(|name| text.contains(&format!("A: src=\"{name}\"")))
        && text.matches("A: overridePins=(type 0x12)0x0").count() == 2
        && text.contains("A: cleartextTrafficPermitted=(type 0x12)0xffffffff")
}

enum Json {
    Bool(bool),
    Number(String),
    Str(String),
    Arr(Vec<Json>),
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

fn dump(value: &Json) -> String {
    let mut out = String::new();
    dump_into(&mut out, value, Some(2), 0);
    out
}

struct Native {
    abi: String,
    machine: u16,
    aligns: Vec<u64>,
    bytes: usize,
}

fn inspect(apk: &Path, version: &str) -> Result<String, PyErr> {
    let root = project_root()?;
    let apk = resolve_loose(apk)?;
    verify_signature(&apk)?;
    let list_path = apk.parent().ok_or_else(|| PyErr::value("APK path has no parent"))?.join("build-inputs.sha256");
    let list_text = fs::read_to_string(&list_path).map_err(|err| io_err(&err, &list_path.display().to_string()))?;
    let mut build_inputs = Vec::new();
    for line in splitlines(&list_text) {
        let Some((digest, name)) = line.split_once("  ") else {
            return Err(PyErr::value("not enough values to unpack (expected 2, got 1)"));
        };
        let relative = check_recorded(&root, digest, name)?;
        if let Some((_, slot)) = build_inputs.iter_mut().find(|(key, _)| key == &relative) {
            *slot = digest.to_string();
        } else {
            build_inputs.push((relative, digest.to_string()));
        }
    }
    let recorded: BTreeSet<String> = build_inputs.iter().map(|(key, _)| key.clone()).collect();
    inventory_changed(&recorded, &current_inputs(&root)?)?;
    let tools = env_key("CLIENT_TOOLS")?;
    let aapt = PathBuf::from(&tools).join("aapt").display().to_string();
    let zipalign = PathBuf::from(&tools).join("zipalign").display().to_string();
    let apk_text = apk.display().to_string();
    let badging = tool_output(&[aapt.clone(), "dump".into(), "badging".into(), apk_text.clone()])?;
    if !badging_ok(&badging, version) {
        return Err(bare());
    }
    tool_status(&[zipalign, "-c".into(), "-P".into(), "16".into(), "-v".into(), "4".into(), apk_text.clone()])?;
    let manifest = tool_output(&[aapt.clone(), "dump".into(), "xmltree".into(), apk_text.clone(), "AndroidManifest.xml".into()])?;
    let resource_id = network_resource_id(&manifest)?;
    let resources = tool_output(&[aapt.clone(), "dump".into(), "resources".into(), apk_text.clone()])?;
    if !resources_bind(&resources, &resource_id) || !permissions_ok(&manifest) {
        return Err(bare());
    }
    let voice = voice_block(&manifest)?;
    if !voice_ok(&voice) {
        return Err(bare());
    }
    let network = tool_output(&[aapt, "dump".into(), "xmltree".into(), apk_text.clone(), "res/xml/network.xml".into()])?;
    if !network_ok(&network) {
        return Err(bare());
    }
    let archive = Archive::open(&apk)?;
    if archive.testzip()?.is_some() {
        return Err(bare());
    }
    let mut native = Vec::new();
    for (abi, machine) in [("arm64-v8a", 183u16), ("x86_64", 62u16)] {
        let raw = archive.read_name(&format!("lib/{abi}/libclient.so"))?;
        let aligns = require_elf(&raw, machine)?;
        native_markers(abi, &raw)?;
        native.push(Native { abi: abi.into(), machine, aligns, bytes: raw.len() });
    }
    let names = archive.names();
    development_leak(&names)?;
    let dex = archive.read_name("classes.dex")?;
    dex_markers(&dex)?;
    let bytes = fs::read(&apk).map_err(|err| io_err(&err, &apk.display().to_string()))?;
    let size = apk.metadata().map_err(|err| io_err(&err, &apk.display().to_string()))?.len();
    let record = Json::Obj(vec![
        ("version".into(), Json::Number(version.into())),
        ("path".into(), Json::Str(apk.display().to_string())),
        ("bytes".into(), Json::Number(size.to_string())),
        ("sha256".into(), Json::Str(hex_digest(&sha256(&bytes)))),
        (
            "native".into(),
            Json::Arr(native.iter().map(|item| {
                Json::Obj(vec![
                    ("abi".into(), Json::Str(item.abi.clone())),
                    ("machine".into(), Json::Number(item.machine.to_string())),
                    ("load_alignments".into(), Json::Arr(item.aligns.iter().map(|value| Json::Number(value.to_string())).collect())),
                    ("bytes".into(), Json::Number(item.bytes.to_string())),
                ])
            }).collect()),
        ),
        ("label".into(), Json::Str("RLLM".into())),
        ("package".into(), Json::Str("com.local.client.lab".into())),
        ("signature".into(), Json::Str("v2/v3; one pinned current signer".into())),
        ("zip_crc".into(), Json::Str("passed".into())),
        ("build_inputs".into(), Json::Obj(build_inputs.into_iter().map(|(key, value)| (key, Json::Str(value))).collect())),
        (
            "network_security".into(),
            Json::Obj(vec![
                ("path".into(), Json::Str("res/xml/network.xml".into())),
                ("resource_id".into(), Json::Str(resource_id)),
                ("trust_anchors".into(), Json::Arr(vec![Json::Str("system".into()), Json::Str("user".into())])),
                ("pin_overrides".into(), Json::Bool(false)),
            ]),
        ),
    ]);
    let text = dump(&record);
    let out = root.join("output/checks");
    fs::create_dir_all(&out).map_err(|err| io_err(&err, &out.display().to_string()))?;
    let destination = out.join("apk.json");
    fs::write(&destination, format!("{text}\n")).map_err(|err| io_err(&err, &destination.display().to_string()))?;
    Ok(text)
}

fn usage_error(message: &str) -> i32 {
    eprintln!("{USAGE}");
    eprintln!("apk.py: error: {message}");
    2
}

fn looks_like_option(token: &str) -> bool {
    token.starts_with('-') && token != "-"
}

fn parse_py_int(token: &str) -> Option<String> {
    let mut body = token;
    let negative = if let Some(rest) = body.strip_prefix('+') {
        body = rest;
        false
    } else if let Some(rest) = body.strip_prefix('-') {
        body = rest;
        true
    } else {
        false
    };
    if body.is_empty() {
        return None;
    }
    let bytes = body.as_bytes();
    let mut digits = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'_' {
            if index == 0 || index + 1 == bytes.len() || !bytes[index - 1].is_ascii_digit() || !bytes[index + 1].is_ascii_digit() {
                return None;
            }
            index += 1;
            continue;
        }
        if !bytes[index].is_ascii_digit() {
            return None;
        }
        digits.push(bytes[index] as char);
        index += 1;
    }
    let trimmed = digits.trim_start_matches('0');
    let digits = if trimmed.is_empty() { "0" } else { trimmed };
    if negative && digits != "0" { Some(format!("-{digits}")) } else { Some(digits.to_string()) }
}

struct Cli {
    apk: String,
    version: String,
}

fn parse_cli(args: &[String]) -> Result<Cli, i32> {
    let mut version = None;
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
        if options && (arg == "--version" || arg.starts_with("--version=")) {
            let token = if let Some(value) = arg.strip_prefix("--version=") {
                value.to_string()
            } else {
                let next = args.get(index + 1);
                if next.map(|value| looks_like_option(value)).unwrap_or(true) {
                    return Err(usage_error("argument --version: expected one argument"));
                }
                index += 1;
                next.unwrap().clone()
            };
            match parse_py_int(&token) {
                Some(value) => version = Some(value),
                None => return Err(usage_error(&format!("argument --version: invalid int value: {}", py_repr(&token)))),
            }
            index += 1;
            continue;
        }
        if options && looks_like_option(arg) {
            unrecognized.push(arg.clone());
            index += 1;
            continue;
        }
        positionals.push(arg.clone());
        index += 1;
    }
    let mut missing = Vec::new();
    if positionals.is_empty() {
        missing.push("apk");
    }
    if version.is_none() {
        missing.push("--version");
    }
    if !missing.is_empty() {
        return Err(usage_error(&format!("the following arguments are required: {}", missing.join(", "))));
    }
    if positionals.len() > 1 {
        unrecognized.extend(positionals[1..].iter().cloned());
    }
    if !unrecognized.is_empty() {
        return Err(usage_error(&format!("unrecognized arguments: {}", unrecognized.join(" "))));
    }
    Ok(Cli { apk: positionals.remove(0), version: version.unwrap() })
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match parse_cli(&args) {
        Err(code) => code,
        Ok(cli) => match inspect(Path::new(&cli.apk), &cli.version) {
            Ok(text) => {
                println!("{text}");
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

#[cfg(test)]
fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crc32(data);
        let offset = local.len() as u32;
        let name_bytes = name.as_bytes();
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
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }
    let count = files.len() as u16;
    let central_len = central.len() as u32;
    let central_at = local.len() as u32;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_len.to_le_bytes());
    out.extend_from_slice(&central_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
fn elf_image(machine: u16, align: u64) -> Vec<u8> {
    let mut raw = vec![0u8; 120];
    raw[0..6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
    raw[18..20].copy_from_slice(&machine.to_le_bytes());
    raw[32..40].copy_from_slice(&64u64.to_le_bytes());
    raw[54..56].copy_from_slice(&56u16.to_le_bytes());
    raw[56..58].copy_from_slice(&1u16.to_le_bytes());
    raw[64..68].copy_from_slice(&1u32.to_le_bytes());
    raw[112..120].copy_from_slice(&align.to_le_bytes());
    raw
}

#[cfg(test)]
mod checks {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rllm-apk-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn inventory_drift_added_and_removed() {
        assert_eq!(ALIGNER, "tools/alignment.rs");
        let root = scratch("inventory");
        for directory in ["core", "platform", "res", "notices"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        fs::write(root.join("core/kept.rs"), b"kept").unwrap();
        fs::write(root.join("core/added.rs"), b"added").unwrap();
        let current = current_inputs(&root).unwrap();
        assert!(current.contains("core/kept.rs"));
        assert!(current.contains("core/added.rs"));
        assert!(current.contains(ALIGNER));
        let mut recorded = current.clone();
        recorded.remove("core/added.rs");
        let err = inventory_changed(&recorded, &current).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: Build input inventory changed after compilation");
        recorded = current.clone();
        recorded.insert("core/removed.rs".into());
        let err = inventory_changed(&recorded, &current).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: Build input inventory changed after compilation");
        assert!(inventory_changed(&current, &current).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_mismatch() {
        let root = scratch("digest");
        fs::create_dir_all(root.join("core")).unwrap();
        let file = root.join("core/item.rs");
        fs::write(&file, b"actual").unwrap();
        let relative = "core/item.rs";
        let wrong = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let err = check_recorded(&root, wrong, file.to_str().unwrap()).unwrap_err();
        assert_eq!(err.traceback_line(), format!("AssertionError: {relative}"));
        let right = hex_digest(&sha256(b"actual"));
        assert_eq!(check_recorded(&root, &right, file.to_str().unwrap()).unwrap(), relative);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_native_marker() {
        let raw = b"voice-onset: only";
        let err = require_marker("arm64-v8a", raw, "Scene source bytes changed while loading").unwrap_err();
        assert_eq!(
            err.traceback_line(),
            "AssertionError: ('arm64-v8a', b'Scene source bytes changed while loading')"
        );
        assert!(require_marker("x86_64", raw, "voice-onset:").is_ok());
    }

    #[test]
    fn wrong_elf_machine() {
        let raw = elf_image(3, 16384);
        assert!(!elf_machine_matches(&raw, 183).unwrap());
        assert!(load_alignments(&raw).unwrap().iter().all(|value| *value >= 16384));
        assert_eq!(require_elf(&raw, 183).unwrap_err().traceback_line(), "AssertionError");
        assert!(elf_machine_matches(&elf_image(183, 16384), 183).unwrap());
    }

    #[test]
    fn load_alignment_below_16kib() {
        let raw = elf_image(183, 4096);
        assert!(elf_machine_matches(&raw, 183).unwrap());
        let aligns = load_alignments(&raw).unwrap();
        assert_eq!(aligns, vec![4096]);
        assert_eq!(require_elf(&raw, 183).unwrap_err().traceback_line(), "AssertionError");
        assert_eq!(require_elf(&elf_image(62, 16384), 62).unwrap(), vec![16384]);
    }

    #[test]
    fn development_file_inside_archive() {
        let dir = scratch("leak");
        let path = dir.join("dev.apk");
        fs::write(&path, stored_zip(&[("docs/secret.txt", b"leak"), ("classes.dex", b"dex")])).unwrap();
        let archive = Archive::open(&path).unwrap();
        let err = development_leak(&archive.names()).unwrap_err();
        assert_eq!(err.traceback_line(), "AssertionError: Development files in the signed APK");
        fs::write(&path, stored_zip(&[("classes.dex", b"dex"), ("lib/arm64-v8a/libclient.so", b"so")])).unwrap();
        let archive = Archive::open(&path).unwrap();
        assert!(development_leak(&archive.names()).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }
}

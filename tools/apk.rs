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

// ---------------------------------------------------------------------------
// Release floor and next version code.
//
// Android refuses to install an APK whose android:versionCode is not strictly
// greater than the code already installed on the device, so a release number
// may only ever grow. The floor below is derived from measured bytes -- the
// compiled AndroidManifest versionCode of every retained artifact the tree
// holds -- joined with the values recorded in PUBLICATION-MANIFEST.json. It is
// never a constant and never the wall clock, so the same tree on any host
// derives the same number.
//
// The compiled manifest is decoded here instead of through aapt: the Android
// SDK on this host is not the one the toolchain pins, and a release number
// must not depend on which build-tools happen to be installed.
// ---------------------------------------------------------------------------

const ANDROID_RES_NAMESPACE: &str = "http://schemas.android.com/apk/res/android";
const RES_XML_TYPE: u16 = 0x0003;
const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const RES_STRING_POOL_UTF8_FLAG: u32 = 1 << 8;
const ATTRIBUTE_TYPE_STRING: u8 = 0x03;
const ATTRIBUTE_TYPE_INT_DEC: u8 = 0x10;
const ATTRIBUTE_TYPE_INT_HEX: u8 = 0x11;
const NO_ENTRY: u32 = 0xFFFF_FFFF;

fn u16_le(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_length_u8(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = *bytes.get(at)?;
    if first & 0x80 == 0 {
        return Some((first as u32, at + 1));
    }
    let second = *bytes.get(at + 1)?;
    Some(((((first & 0x7f) as u32) << 8) | second as u32, at + 2))
}

fn read_length_u16(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = u16_le(bytes, at)?;
    if first & 0x8000 == 0 {
        return Some((first as u32, at + 2));
    }
    let second = u16_le(bytes, at + 2)?;
    Some(((((first & 0x7fff) as u32) << 16) | second as u32, at + 4))
}

struct AxmlPool {
    strings_start: usize,
    offsets: Vec<u32>,
    utf8: bool,
}

impl AxmlPool {
    fn parse(bytes: &[u8], chunk: usize, header: usize) -> Option<AxmlPool> {
        let count = u32_le(bytes, chunk + 8)? as usize;
        let flags = u32_le(bytes, chunk + 16)?;
        let strings_start = u32_le(bytes, chunk + 20)? as usize;
        let mut offsets = Vec::with_capacity(count);
        for index in 0..count {
            offsets.push(u32_le(bytes, chunk + header + index * 4)?);
        }
        Some(AxmlPool {
            strings_start: chunk + strings_start,
            offsets,
            utf8: flags & RES_STRING_POOL_UTF8_FLAG != 0,
        })
    }

    fn string(&self, bytes: &[u8], index: u32) -> Option<String> {
        if index == NO_ENTRY {
            return None;
        }
        let offset = *self.offsets.get(index as usize)? as usize;
        let at = self.strings_start + offset;
        if self.utf8 {
            let (_, cursor) = read_length_u8(bytes, at)?;
            let (length, cursor) = read_length_u8(bytes, cursor)?;
            let slice = bytes.get(cursor..cursor + length as usize)?;
            Some(String::from_utf8_lossy(slice).to_string())
        } else {
            let (length, mut cursor) = read_length_u16(bytes, at)?;
            let mut units = Vec::with_capacity(length as usize);
            for _ in 0..length {
                units.push(u16_le(bytes, cursor)?);
                cursor += 2;
            }
            Some(String::from_utf16_lossy(&units))
        }
    }
}

// What a compiled manifest declares about itself: the package it belongs to
// and the release number it carries. The release number is the
// android-namespaced versionCode attribute and only that attribute; the
// package id is the unqualified package attribute of the manifest element.
struct ManifestFacts {
    package: Option<String>,
    version_code: Option<u64>,
}

// The compiled manifest, read from the attribute table of the manifest
// element. A missing value means the manifest carries no such value, which is
// the case for the resource-only packages the build leaves behind.
fn axml_manifest_facts(bytes: &[u8]) -> ManifestFacts {
    axml_manifest_facts_of(bytes).unwrap_or(ManifestFacts { package: None, version_code: None })
}

fn axml_manifest_facts_of(bytes: &[u8]) -> Option<ManifestFacts> {
    let mut facts = ManifestFacts { package: None, version_code: None };
    if u16_le(bytes, 0)? != RES_XML_TYPE {
        return None;
    }
    let header = u16_le(bytes, 2)? as usize;
    let declared = u32_le(bytes, 4)? as usize;
    let end = if declared == 0 { bytes.len() } else { declared.min(bytes.len()) };
    let mut cursor = header;
    let mut pool: Option<AxmlPool> = None;
    while cursor + 8 <= end {
        let chunk_type = u16_le(bytes, cursor)?;
        let chunk_header = u16_le(bytes, cursor + 2)? as usize;
        let chunk_size = u32_le(bytes, cursor + 4)? as usize;
        if chunk_size < 8 {
            return None;
        }
        let chunk_end = cursor + chunk_size;
        if chunk_end > end {
            return None;
        }
        match chunk_type {
            RES_STRING_POOL_TYPE => pool = AxmlPool::parse(bytes, cursor, chunk_header),
            RES_XML_START_ELEMENT_TYPE => {
                let pool = pool.as_ref()?;
                let attributes = cursor + 16;
                let element_name = u32_le(bytes, cursor + 20)?;
                let attribute_start = u16_le(bytes, attributes + 8)? as usize;
                let attribute_size = u16_le(bytes, attributes + 10)? as usize;
                let attribute_count = u16_le(bytes, attributes + 12)? as usize;
                if attribute_size < 20 {
                    return None;
                }
                let is_manifest = pool.string(bytes, element_name).as_deref() == Some("manifest");
                for index in 0..attribute_count {
                    let at = attributes + attribute_start + index * attribute_size;
                    let namespace = u32_le(bytes, at)?;
                    let name = u32_le(bytes, at + 4)?;
                    let data_type = *bytes.get(at + 15)?;
                    let data = u32_le(bytes, at + 16)?;
                    let attribute_name = pool.string(bytes, name);
                    if attribute_name.as_deref() == Some("versionCode") {
                        // The release number is the android-namespaced attribute
                        // and only that attribute: an attribute named
                        // versionCode that belongs to no namespace is a
                        // different attribute of the manifest, and reading it as
                        // the release number would take the version of a package
                        // that never declared one.
                        let resolved = pool.string(bytes, namespace).unwrap_or_default();
                        if resolved != ANDROID_RES_NAMESPACE {
                            continue;
                        }
                        facts.version_code = match data_type {
                            ATTRIBUTE_TYPE_INT_DEC | ATTRIBUTE_TYPE_INT_HEX => Some(data as u64),
                            _ => None,
                        };
                        continue;
                    }
                    // The package id is the unqualified package attribute of the
                    // manifest element: it names the application the artifact
                    // carries, and every release number belongs to one of them.
                    if is_manifest
                        && namespace == NO_ENTRY
                        && attribute_name.as_deref() == Some("package")
                        && data_type == ATTRIBUTE_TYPE_STRING
                    {
                        facts.package = pool.string(bytes, data);
                    }
                }
                if is_manifest {
                    return Some(facts);
                }
            }
            _ => {}
        }
        cursor = chunk_end;
    }
    Some(facts)
}

// The android:versionCode attribute of the compiled manifest.

fn json_string_at(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    let mut index = at + 1;
    let mut raw = Vec::new();
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            index += 1;
            let escape = *bytes.get(index)?;
            match escape {
                b'n' => raw.push(b'\n'),
                b't' => raw.push(b'\t'),
                b'r' => raw.push(b'\r'),
                b'b' => raw.push(0x08),
                b'f' => raw.push(0x0c),
                b'u' => {
                    let hex = bytes.get(index + 1..index + 5)?;
                    let code = u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                    let character = char::from_u32(code).unwrap_or('\u{fffd}');
                    let mut encoded = [0u8; 4];
                    raw.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                    index += 4;
                }
                other => raw.push(other),
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            return Some((String::from_utf8_lossy(&raw).to_string(), index + 1));
        }
        raw.push(byte);
        index += 1;
    }
    None
}

// A top-level integer member of a JSON object. Nested objects are ignored, so
// a "version" recorded inside another member never stands in for the record's
// own version. The member's value span is located once and read from, so the
// record has one reader for both the floor and the advance.
fn json_top_level_number(text: &str, key: &str) -> Option<u64> {
    let (start, end) = json_top_level_member(text, key)?;
    let raw = text.get(start..end)?.trim();
    let digits: String = raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

// A top-level string member of a JSON object. Nested objects are ignored, so
// a "package" recorded inside another member never stands in for the record's
// own package: the release floor of a package belongs to the package the
// record names.
fn json_top_level_string(text: &str, key: &str) -> Option<String> {
    let (start, end) = json_top_level_member(text, key)?;
    let raw = text.get(start..end)?.trim();
    if !raw.starts_with('"') {
        return None;
    }
    let (value, _) = json_string_at(raw.as_bytes(), 0)?;
    Some(value)
}

// The members of a top-level object member whose value is a flat object of
// package id to release floor. The member's value span is located once, so a
// "distributed_baselines" recorded inside another object is never read, and a
// member that is present but unreadable is refused rather than ignored: a
// floor that cannot be read must not become a default.
fn json_top_level_number_map(text: &str, key: &str) -> Result<Option<Vec<(String, u64)>>, String> {
    let (start, end) = match json_top_level_member(text, key) {
        Some(span) => span,
        None => return Ok(None),
    };
    let raw = text
        .get(start..end)
        .ok_or_else(|| format!("the record's {key} member is out of bounds"))?;
    Ok(Some(json_number_object_members(raw)?))
}

// A flat JSON object of string keys to integer values.
fn json_number_object_members(raw: &str) -> Result<Vec<(String, u64)>, String> {
    let bytes = raw.as_bytes();
    let mut index = 0usize;
    json_skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return Err("the member is not a JSON object".to_string());
    }
    index += 1;
    let mut members = Vec::new();
    loop {
        json_skip_ws(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            return Ok(members);
        }
        let (name, next) = json_string_at(bytes, index)
            .ok_or_else(|| "the member has a name that is not a string".to_string())?;
        index = next;
        json_skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            return Err("the member has no value".to_string());
        }
        index += 1;
        json_skip_ws(bytes, &mut index);
        let start = index;
        json_skip_value(bytes, &mut index)
            .ok_or_else(|| "the member has a value that cannot be read".to_string())?;
        let end = index;
        let value = raw
            .get(start..end)
            .ok_or_else(|| "the member has a value that cannot be read".to_string())?
            .trim();
        let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
        let number = digits
            .parse::<u64>()
            .map_err(|_| format!("the member {name} does not carry a release floor"))?;
        members.push((name, number));
        json_skip_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(&b',') => index += 1,
            Some(&b'}') => return Ok(members),
            _ => return Err("the member is not a flat object".to_string()),
        }
    }
}

// The per-package floors as the record carries them: one package per line,
// sorted by package id, so two writes of the same floors produce the same
// bytes and a reader sees the same order every time.
fn baselines_map_text(entries: &[(String, u64)], indent: usize) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }
    let mut sorted: Vec<&(String, u64)> = entries.iter().collect();
    sorted.sort_by(|left, right| left.0.cmp(&right.0));
    let inner = " ".repeat(indent + 2);
    let outer = " ".repeat(indent);
    let mut out = String::from("{\n");
    for (index, (package, value)) in sorted.iter().enumerate() {
        out.push_str(&inner);
        dump_string(&mut out, package);
        out.push_str(": ");
        out.push_str(&value.to_string());
        if index + 1 != sorted.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&outer);
    out.push('}');
    out
}

// The indentation the record's own members carry, so a member the record
// does not carry yet is added in the record's own layout.
fn record_indent(text: &str) -> usize {
    let bytes = text.as_bytes();
    let Some(open) = bytes.iter().position(|byte| *byte == b'{') else {
        return 0;
    };
    let mut cursor = open + 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let run = &text[open + 1..cursor];
    match run.rfind('\n') {
        Some(at) => run[at + 1..].chars().take_while(|c| *c == ' ').count(),
        None => 0,
    }
}

// The indentation of the line a member's value sits on.
fn member_indent(text: &str, at: usize) -> usize {
    let bytes = text.as_bytes();
    let mut line_start = at.min(bytes.len());
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    bytes[line_start..at]
        .iter()
        .take_while(|byte| **byte == b' ')
        .count()
}

// The byte span of a top-level member's value inside a JSON text. A member of
// the same name inside another object is not a top-level member, so a record
// that carries "distributed_baseline" under another key never stands in for
// the record's own member, whatever the record's layout.
fn json_top_level_member(text: &str, key: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    json_skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    loop {
        json_skip_ws(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            return None;
        }
        let (name, next) = json_string_at(bytes, index)?;
        index = next;
        json_skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            return None;
        }
        index += 1;
        json_skip_ws(bytes, &mut index);
        let start = index;
        json_skip_value(bytes, &mut index)?;
        let end = index;
        if name == key {
            return Some((start, end));
        }
        json_skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b',') {
            return None;
        }
        index += 1;
    }
}

fn json_skip_ws(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && bytes[*index].is_ascii_whitespace() {
        *index += 1;
    }
}

// The end of one JSON value, so a member's value span can be read or
// rewritten without reformatting the rest of the record.
fn json_skip_value(bytes: &[u8], index: &mut usize) -> Option<()> {
    match bytes.get(*index)? {
        b'"' => {
            let (_, next) = json_string_at(bytes, *index)?;
            *index = next;
            Some(())
        }
        b'{' | b'[' => {
            let mut depth = 0usize;
            loop {
                match bytes.get(*index)? {
                    b'"' => {
                        let (_, next) = json_string_at(bytes, *index)?;
                        *index = next;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        if depth == 0 {
                            return None;
                        }
                        depth -= 1;
                        if depth == 0 {
                            *index += 1;
                            return Some(());
                        }
                    }
                    _ => {}
                }
                *index += 1;
            }
        }
        _ => {
            while let Some(byte) = bytes.get(*index) {
                match byte {
                    b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E' | b't' | b'r' | b'u' | b'n' | b'a' | b'l' | b's' | b'f' => *index += 1,
                    _ => break,
                }
            }
            Some(())
        }
    }
}

struct ReleaseArtifact {
    path: PathBuf,
    code: Option<u64>,
    package: Option<String>,
    retained_release: bool,
}

struct ReleaseScan {
    floor: u64,
    artifacts: Vec<ReleaseArtifact>,
    recorded_version: Option<u64>,
    recorded_baseline: Option<u64>,
    recorded_package: Option<String>,
    recorded_baselines: Vec<(String, u64)>,
}

fn collect_apks(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        if kind.is_dir() {
            collect_apks(&path, out);
        } else if kind.is_file() && path.extension().map(|ext| ext == "apk").unwrap_or(false) {
            out.push(path);
        }
    }
}

// The derived floor: the highest compiled versionCode any retained artifact
// carries, joined with the version and the baseline the publication record
// holds. A missing record, or a record that names neither value, is a
// refusal: a floor that cannot be derived must not become a default.
fn scan_release(root: &Path) -> Result<ReleaseScan, String> {
    let mut paths = Vec::new();
    collect_apks(&root.join("APK"), &mut paths);
    collect_apks(&root.join("output"), &mut paths);
    paths.sort();
    let mut artifacts = Vec::new();
    let mut floor = 0u64;
    for path in paths {
        let archive = Archive::open(&path)
            .map_err(|err| format!("{}: {}", path.display(), err.traceback_line()))?;
        let manifest = archive
            .read_name("AndroidManifest.xml")
            .map_err(|err| format!("{}: {}", path.display(), err.traceback_line()))?;
        let facts = axml_manifest_facts(&manifest);
        let code = facts.version_code;
        if let Some(code) = code {
            if code > floor {
                floor = code;
            }
        }
        // APK/ is the retained release collection. In output/, only final
        // delivery files immediately below a build directory are releases;
        // javac, resource-linker, staging and alignment APKs are intermediates.
        let retained_release = path.starts_with(root.join("APK"))
            || (path.parent().and_then(Path::parent) == Some(root.join("output").as_path())
                && matches!(path.file_name().and_then(|name| name.to_str()),
                    Some("RLLM.apk" | "RLLM-unsigned.apk")));
        artifacts.push(ReleaseArtifact { path, code, package: facts.package, retained_release });
    }
    let record = root.join("PUBLICATION-MANIFEST.json");
    let text = fs::read_to_string(&record).map_err(|_| format!("{} is absent", record.display()))?;
    let recorded_version = json_top_level_number(&text, "version");
    let recorded_baseline = json_top_level_number(&text, "distributed_baseline");
    let recorded_package = json_top_level_string(&text, "package");
    // A record that carries no per-package floor yet is read as one that
    // holds none: the map is the record's per-package ledger and it is
    // written by the advance. A map that is present but unreadable is a
    // refusal, because a floor that cannot be read must not become a default.
    let recorded_baselines = json_top_level_number_map(&text, "distributed_baselines")?
        .unwrap_or_default();
    for value in [recorded_version, recorded_baseline].into_iter().flatten() {
        if value > floor {
            floor = value;
        }
    }
    for (_, value) in &recorded_baselines {
        if *value > floor {
            floor = *value;
        }
    }
    if recorded_version.is_none() && recorded_baseline.is_none() && recorded_baselines.is_empty() {
        return Err(format!(
            "{} records neither a version nor a distributed baseline",
            record.display()
        ));
    }
    Ok(ReleaseScan {
        floor,
        artifacts,
        recorded_version,
        recorded_baseline,
        recorded_package,
        recorded_baselines,
    })
}

// The release floor of one package: the floor the record holds for that
// package, joined with the release numbers of the artifacts that belong to
// it. An artifact whose manifest names no package cannot be attributed, so it
// counts for every package: a floor that ignored an artifact of unknown
// provenance would let a release number below it be issued.
//
// The record's version and scalar baseline describe its named package.
// Other packages use their own retained releases or ledger entries. When
// neither exists, the conservative tree floor applies, not a zero default
// or a placeholder code found in a build intermediate.
fn package_floor(scan: &ReleaseScan, package: &str) -> u64 {
    let mut floor = 0u64;
    let mut evidence = false;
    let mut consider = |value: u64| {
        evidence = true;
        if value > floor {
            floor = value;
        }
    };
    if scan.recorded_package.as_deref() == Some(package) {
        for value in [scan.recorded_version, scan.recorded_baseline].into_iter().flatten() {
            consider(value);
        }
    }
    for (name, value) in &scan.recorded_baselines {
        if name == package {
            consider(*value);
        }
    }
    for artifact in &scan.artifacts {
        if !artifact.retained_release {
            continue;
        }
        match artifact.package.as_deref() {
            Some(owner) if owner != package => {}
            _ => {
                if let Some(code) = artifact.code {
                    consider(code);
                }
            }
        }
    }
    // An unpublished package must retain the prior tree-wide protection.
    // Intermediate versionCode=1 manifests cannot establish a release floor.
    if evidence { floor } else { scan.floor }
}

// Every package the tree knows about: the package the record names, every
// package the record holds a floor for, and every package a retained
// artifact declares.
fn known_packages(scan: &ReleaseScan) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    let mut add = |package: &str| {
        if !packages.iter().any(|known| known == package) {
            packages.push(package.to_string());
        }
    };
    if let Some(package) = &scan.recorded_package {
        add(package);
    }
    for (package, _) in &scan.recorded_baselines {
        add(package);
    }
    for artifact in &scan.artifacts {
        if let Some(package) = &artifact.package {
            add(package);
        }
    }
    packages.sort();
    packages
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum ReleaseMode {
    Baseline,
    Next,
    Report,
    ReportJson,
    Advance,
}

fn release_report(root: &Path, scan: &ReleaseScan) {
    println!("root: {}", root.display());
    println!("recorded version: {}", scan.recorded_version.map(|v| v.to_string()).unwrap_or_else(|| "absent".to_string()));
    println!("recorded distributed baseline: {}", scan.recorded_baseline.map(|v| v.to_string()).unwrap_or_else(|| "absent".to_string()));
    println!("recorded package: {}", scan.recorded_package.clone().unwrap_or_else(|| "absent".to_string()));
    for artifact in &scan.artifacts {
        let code = artifact.code.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string());
        println!("artifact {} {}", code, artifact.path.display());
    }
    println!("release baseline: {}", scan.floor);
    for package in known_packages(scan) {
        println!("package release baseline {} {}", package, package_floor(scan, &package));
    }
    println!("next version code: {}", scan.floor + 1);
}

// The same measurement as the human report, in a shape a reader can parse
// instead of scrape: the release floor of the tree, the version code the
// allocator hands out next, the values the record holds, the release floor of
// every package the tree knows about and the compiled version code of every
// retained artifact with the package it declares. A member the record does
// not carry is absent from the document rather than present with a
// placeholder, so a reader cannot mistake an absent value for a number.
fn release_report_json(root: &Path, scan: &ReleaseScan, selected: Option<&str>) -> String {
    let mut members: Vec<(String, Json)> = vec![
        ("root".to_string(), Json::Str(root.display().to_string())),
        ("release_baseline".to_string(), Json::Number(scan.floor.to_string())),
        ("next_version_code".to_string(), Json::Number((scan.floor + 1).to_string())),
    ];
    if let Some(version) = scan.recorded_version {
        members.push(("recorded_version".to_string(), Json::Number(version.to_string())));
    }
    if let Some(baseline) = scan.recorded_baseline {
        members.push((
            "recorded_distributed_baseline".to_string(),
            Json::Number(baseline.to_string()),
        ));
    }
    if let Some(package) = &scan.recorded_package {
        members.push(("recorded_package".to_string(), Json::Str(package.clone())));
    }
    let floors: Vec<Json> = known_packages(scan)
        .iter()
        .map(|package| {
            Json::Obj(vec![
                ("package".to_string(), Json::Str(package.clone())),
                (
                    "release_baseline".to_string(),
                    Json::Number(package_floor(scan, package).to_string()),
                ),
            ])
        })
        .collect();
    members.push(("package_floors".to_string(), Json::Arr(floors)));
    if let Some(package) = selected {
        members.push(("selected_package".to_string(), Json::Str(package.to_string())));
        members.push((
            "selected_release_baseline".to_string(),
            Json::Number(package_floor(scan, package).to_string()),
        ));
        members.push((
            "selected_next_version_code".to_string(),
            Json::Number((package_floor(scan, package) + 1).to_string()),
        ));
    }
    let artifacts: Vec<Json> = scan
        .artifacts
        .iter()
        .map(|artifact| {
            let mut fields = vec![(
                "path".to_string(),
                Json::Str(artifact.path.display().to_string()),
            )];
            if let Some(code) = artifact.code {
                fields.push(("version_code".to_string(), Json::Number(code.to_string())));
            }
            if let Some(package) = &artifact.package {
                fields.push(("package".to_string(), Json::Str(package.clone())));
            }
            Json::Obj(fields)
        })
        .collect();
    members.push(("artifacts".to_string(), Json::Arr(artifacts)));
    dump(&Json::Obj(members))
}

// The release floor of one package, advanced to the code that was built. The
// advance never lowers a floor and it never records a code below the floor
// the tree already implies: the record and the allocator enforce one floor,
// not two.
//
// The advance rewrites the record's own top-level members only, so a member
// of the same name inside another object is left exactly as it was, and the
// record is replaced by a rename, so a reader never sees a half-written
// record.
//
// A code below the floor of its package is refused with both values and the
// package named rather than silently leaving the record where it is: a caller
// that believes it advanced the floor has to learn that it did not, instead of
// reading a success that changed nothing.
fn advance_baseline(root: &Path, code: u64, package: Option<&str>) -> Result<u64, String> {
    let record = root.join("PUBLICATION-MANIFEST.json");
    let text = fs::read_to_string(&record)
        .map_err(|err| format!("{} is unreadable: {}", record.display(), err))?;
    let recorded_package = json_top_level_string(&text, "package");
    let target = match package {
        Some(package) => package.to_string(),
        None => match recorded_package.clone() {
            Some(package) => package,
            None => {
                return Err(format!(
                    "{} names no package, so there is no package whose release floor could be advanced",
                    record.display()
                ))
            }
        },
    };
    // The floor this advance must not fall below: the floor of that package,
    // joined with the release numbers of the artifacts that belong to it.
    let scan = scan_release(root)?;
    let floor = package_floor(&scan, &target);
    if code < floor {
        return Err(format!(
            "version code {code} is below the release floor {floor} of package {target}; a release number may not move downward"
        ));
    }
    let mut entries = scan.recorded_baselines.clone();
    entries.retain(|(name, _)| name != &target);
    entries.push((target.clone(), code));
    let mut updated = text.clone();
    // The scalar baseline describes the package the record names, so it is
    // only written for that package: a second package's floor belongs to the
    // per-package ledger alone.
    if recorded_package.as_deref() == Some(target.as_str()) {
        let span = json_top_level_member(&updated, "distributed_baseline");
        let recorded = span.and_then(|(start, end)| updated.get(start..end)?.trim().parse::<u64>().ok());
        let next = recorded.map(|value| value.max(code)).unwrap_or(code);
        updated = match span {
            Some((start, end)) => {
                let mut out = String::with_capacity(updated.len());
                out.push_str(&updated[..start]);
                out.push_str(&next.to_string());
                out.push_str(&updated[end..]);
                out
            }
            None => insert_top_level_member(&updated, "distributed_baseline", &next.to_string())?,
        };
    }
    let span = json_top_level_member(&updated, "distributed_baselines");
    let indent = match span {
        Some((start, _)) => member_indent(&updated, start),
        None => record_indent(&updated),
    };
    let rendered = baselines_map_text(&entries, indent);
    updated = match span {
        Some((start, end)) => {
            let mut out = String::with_capacity(updated.len());
            out.push_str(&updated[..start]);
            out.push_str(&rendered);
            out.push_str(&updated[end..]);
            out
        }
        None => insert_top_level_member(&updated, "distributed_baselines", &rendered)?,
    };
    if updated == text {
        return Ok(code);
    }
    let temporary = record.with_extension("json.advance-tmp");
    fs::write(&temporary, updated.as_bytes())
        .map_err(|err| format!("{} is unwritable: {}", temporary.display(), err))?;
    fs::rename(&temporary, &record)
        .map_err(|err| format!("{} could not be replaced: {}", record.display(), err))?;
    Ok(code)
}

// A member the record does not carry yet is added as a top-level member, in
// the record's own layout: a record written one member per line keeps that
// layout, and a compact record keeps its compact form.
fn insert_top_level_member(text: &str, key: &str, value: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let open = bytes
        .iter()
        .position(|byte| *byte == b'{')
        .ok_or_else(|| "the publication record is not a JSON object".to_string())?;
    let run_start = open + 1;
    let mut cursor = run_start;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let run = &text[run_start..cursor];
    let inserted = if bytes.get(cursor) == Some(&b'}') {
        format!("\"{key}\": {value}")
    } else if let Some(at) = run.rfind('\n') {
        format!("\n{}\"{key}\": {value},", &run[at + 1..])
    } else {
        format!("\"{key}\": {value},")
    };
    let mut out = String::with_capacity(text.len() + inserted.len());
    out.push_str(&text[..open + 1]);
    out.push_str(&inserted);
    out.push_str(&text[open + 1..]);
    Ok(out)
}

fn release_command(args: &[String]) -> i32 {
    let mut mode: Option<ReleaseMode> = None;
    let mut advance_code: Option<u64> = None;
    let mut root_override: Option<PathBuf> = None;
    let mut package: Option<String> = None;
    let mut index = 0usize;
    while index < args.len() {
        let candidate = match args[index].as_str() {
            "--release-baseline" => Some(ReleaseMode::Baseline),
            "--next-version-code" => Some(ReleaseMode::Next),
            "--release-report" => Some(ReleaseMode::Report),
            "--release-report-json" => Some(ReleaseMode::ReportJson),
            "--advance-baseline" => Some(ReleaseMode::Advance),
            "--root" => {
                index += 1;
                let value = match args.get(index) {
                    Some(value) => value,
                    None => {
                        eprintln!("apk: --root needs a directory");
                        return 2;
                    }
                };
                root_override = Some(PathBuf::from(value));
                None
            }
            "--package" => {
                index += 1;
                let value = match args.get(index) {
                    Some(value) => value,
                    None => {
                        eprintln!("apk: --package needs a package id");
                        return 2;
                    }
                };
                package = Some(value.clone());
                None
            }
            other => {
                eprintln!("apk: unrecognized arguments: {other}");
                return 2;
            }
        };
        if candidate == Some(ReleaseMode::Advance) {
            index += 1;
            let value = match args.get(index) {
                Some(value) => value,
                None => {
                    eprintln!("apk: --advance-baseline needs a version code");
                    return 2;
                }
            };
            match value.parse::<u64>() {
                Ok(code) => advance_code = Some(code),
                Err(_) => {
                    eprintln!("apk: --advance-baseline needs a version code, not {value}");
                    return 2;
                }
            }
        }
        if let Some(candidate) = candidate {
            if mode.is_some() && mode != Some(candidate) {
                eprintln!("apk: --release-baseline, --next-version-code, --release-report, --release-report-json and --advance-baseline cannot be combined");
                return 2;
            }
            mode = Some(candidate);
        }
        index += 1;
    }
    let mode = match mode {
        Some(mode) => mode,
        None => return 2,
    };
    // The tree the release numbers are read from. The project root is the
    // default; a caller that measures a fixture tree names it, so the
    // measurement is of that tree and not of whichever tree holds the binary.
    let root = match root_override {
        Some(path) => {
            if !path.is_dir() {
                eprintln!("apk: --root {} is not a directory", path.display());
                return 2;
            }
            path
        }
        None => match project_root() {
            Ok(root) => root,
            Err(err) => {
                eprintln!("{}", err.traceback_line());
                return 1;
            }
        },
    };
    if mode == ReleaseMode::Advance {
        let code = match advance_code {
            Some(code) => code,
            None => {
                eprintln!("apk: --advance-baseline needs a version code");
                return 2;
            }
        };
        return match advance_baseline(&root, code, package.as_deref()) {
            Ok(value) => {
                println!("{value}");
                0
            }
            Err(message) => {
                eprintln!("apk: {message}");
                1
            }
        };
    }
    let scan = match scan_release(&root) {
        Ok(scan) => scan,
        Err(message) => {
            eprintln!("apk: {message}");
            return 1;
        }
    };
    match mode {
        // The floor of the named package, or the floor of the whole tree when
        // no package is named: a release number belongs to a package, and the
        // tree's floor is the highest release number the tree holds.
        ReleaseMode::Baseline => match package.as_deref() {
            Some(package) => println!("{}", package_floor(&scan, package)),
            None => println!("{}", scan.floor),
        },
        ReleaseMode::Next => match package.as_deref() {
            Some(package) => println!("{}", package_floor(&scan, package) + 1),
            None => println!("{}", scan.floor + 1),
        },
        ReleaseMode::Report => release_report(&root, &scan),
        ReleaseMode::ReportJson => println!("{}", release_report_json(&root, &scan, package.as_deref())),
        // The advance returned above: it needs the record, not a scan.
        ReleaseMode::Advance => {}
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let release = args.iter().any(|arg| {
        arg == "--release-baseline"
            || arg == "--next-version-code"
            || arg == "--release-report"
            || arg == "--release-report-json"
            || arg == "--advance-baseline"
    });
    let code = if release {
        release_command(&args)
    } else {
        match parse_cli(&args) {
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
        }
    };
    std::process::exit(code);
}

// A compiled manifest element carrying the given attributes. Each attribute
// is its namespace index, its name index, its value type and its value.

// A compiled manifest carrying one android:versionCode attribute, or none.

// The same compiled manifest with the release number declared by an attribute
// that belongs to no namespace. That attribute is named versionCode but it is
// not the android release number, so it must not be read as one.

// A compiled manifest that names its package and carries a release number.
// The package attribute is the unqualified one, as a manifest declares it.

// A compiled manifest whose release number is declared twice: once by an
// attribute that belongs to no namespace and once by the android-namespaced
// attribute. Only the latter is the release number.

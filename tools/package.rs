// Native port of tools/package.py. tools/package.py remains until a signed
// candidate is published with this binary. Built by tools/build-tools.sh.
// --dry-run repeats the reads and checks, prints the pinned censuses and the
// path and byte count of each artifact, and writes nothing except the
// destination directory.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
usage: package --destination DESTINATION [--candidate] [--dry-run]

--destination DIRECTORY   publication directory (created if needed)
--candidate               allow the single binary_flow gap
--dry-run                 check and print artifact sizes, write nothing inside the destination
";

fn refuse(message: &str) -> String {
    format!("refusing: {message}")
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
    // checkpoint.rs caps a member at 64 MiB. The publisher keeps that cap and
    // names the member when a source file is larger.
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

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn fit_u32(value: usize, what: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| refuse(&format!("source zip exceeds ZIP32 limits ({what})")))
}

fn archive_bytes(members: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if members.len() > u16::MAX as usize {
        return Err(refuse("source zip exceeds ZIP32 entry count"));
    }
    let mut body = Vec::new();
    let mut directory = Vec::new();
    for (name, plain) in members {
        if name.len() > u16::MAX as usize {
            return Err(refuse(&format!("{name} exceeds ZIP32 name length")));
        }
        let compressed = deflate_store(plain);
        let crc = crc32(plain);
        let plain_len = fit_u32(plain.len(), name)?;
        let comp_len = fit_u32(compressed.len(), name)?;
        let offset = fit_u32(body.len(), name)?;
        let name_bytes = name.as_bytes();
        push_u32(&mut body, 0x0403_4b50);
        push_u16(&mut body, 20);
        push_u16(&mut body, 0x0800);
        push_u16(&mut body, 8);
        push_u16(&mut body, 0);
        push_u16(&mut body, 0x0021);
        push_u32(&mut body, crc);
        push_u32(&mut body, comp_len);
        push_u32(&mut body, plain_len);
        push_u16(&mut body, name_bytes.len() as u16);
        push_u16(&mut body, 0);
        body.extend_from_slice(name_bytes);
        body.extend_from_slice(&compressed);
        push_u32(&mut directory, 0x0201_4b50);
        push_u16(&mut directory, 0x0314);
        push_u16(&mut directory, 20);
        push_u16(&mut directory, 0x0800);
        push_u16(&mut directory, 8);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0x0021);
        push_u32(&mut directory, crc);
        push_u32(&mut directory, comp_len);
        push_u32(&mut directory, plain_len);
        push_u16(&mut directory, name_bytes.len() as u16);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u32(&mut directory, 0);
        push_u32(&mut directory, offset);
        directory.extend_from_slice(name_bytes);
    }
    let directory_offset = fit_u32(body.len(), "central directory")?;
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
    Ok(body)
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data.get(offset..offset + 2).ok_or_else(|| "Truncated central directory".to_string())?;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data.get(offset..offset + 4).ok_or_else(|| "Truncated central directory".to_string())?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_archive(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    if data.len() < 22 {
        return Err("File is not a zip file".into());
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
    let eocd = eocd.ok_or("File is not a zip file")?;
    let entries = u16_at(data, eocd + 8)? as usize;
    let entries_total = u16_at(data, eocd + 10)? as usize;
    if u16_at(data, eocd + 4)? != 0 || u16_at(data, eocd + 6)? != 0 || entries != entries_total {
        return Err("zipfiles that span multiple disks are not supported".into());
    }
    let size_cd = u32_at(data, eocd + 12)? as usize;
    let offset_cd = u32_at(data, eocd + 16)? as usize;
    let end_dir = offset_cd.checked_add(size_cd).ok_or("Truncated central directory")?;
    if end_dir > data.len() {
        return Err("Truncated central directory".into());
    }
    let mut cursor = offset_cd;
    let mut described = Vec::with_capacity(entries);
    while cursor < end_dir {
        if cursor + 46 > end_dir || data[cursor..cursor + 4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err("Bad magic number for central directory".into());
        }
        let method = u16_at(data, cursor + 10)?;
        let crc = u32_at(data, cursor + 16)?;
        let compressed_size = u32_at(data, cursor + 20)? as usize;
        let uncompressed_size = u32_at(data, cursor + 24)? as usize;
        let name_len = u16_at(data, cursor + 28)? as usize;
        let extra_len = u16_at(data, cursor + 30)? as usize;
        let comment_len = u16_at(data, cursor + 32)? as usize;
        let local_offset = u32_at(data, cursor + 42)? as usize;
        let name_at = cursor + 46;
        let name_end = name_at + name_len;
        let next = name_end + extra_len + comment_len;
        if next > end_dir {
            return Err("Truncated central directory".into());
        }
        let name = std::str::from_utf8(&data[name_at..name_end]).map_err(|_| "zip filename is not valid UTF-8".to_string())?.to_string();
        described.push((name, method, crc, compressed_size, uncompressed_size, local_offset));
        cursor = next;
    }
    if described.len() != entries {
        return Err("Truncated central directory".into());
    }
    let mut out = Vec::with_capacity(described.len());
    for (name, method, crc, compressed_size, uncompressed_size, local_offset) in described {
        if local_offset + 30 > data.len() || data[local_offset..local_offset + 4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(format!("Bad magic number for file header {name}"));
        }
        let name_len = u16_at(data, local_offset + 26)? as usize;
        let extra_len = u16_at(data, local_offset + 28)? as usize;
        let data_at = local_offset + 30 + name_len + extra_len;
        let data_end = data_at.checked_add(compressed_size).ok_or_else(|| format!("Truncated member {name}"))?;
        if data_end > data.len() {
            return Err(format!("Truncated member {name}"));
        }
        let compressed = &data[data_at..data_end];
        let plain = match method {
            0 => compressed.to_vec(),
            8 => inflate(compressed, uncompressed_size).map_err(|err| format!("{name}: {err}"))?,
            other => return Err(format!("{name}: compression type {other}")),
        };
        if plain.len() != uncompressed_size {
            return Err(format!("{name}: length {uncompressed_size} does not match {}", plain.len()));
        }
        let actual = crc32(&plain);
        if actual != crc {
            return Err(format!("{name}: Bad CRC-32"));
        }
        out.push((name, plain));
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
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
    fn bump(&mut self) -> Result<u8, String> {
        let byte = self.peek().ok_or_else(|| "Expecting value".to_string())?;
        self.index += 1;
        Ok(byte)
    }
    fn parse_value(&mut self) -> Result<Json, String> {
        self.skip();
        match self.peek() {
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err("Expecting value".into()),
        }
    }
    fn literal(&mut self, text: &[u8], value: Json) -> Result<Json, String> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err("Expecting value".into())
        }
    }
    fn number(&mut self) -> Result<Json, String> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        if self.peek() == Some(b'0') {
            self.index += 1;
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("Expecting value".into());
            }
        } else if matches!(self.peek(), Some(b'1'..=b'9')) {
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        } else {
            return Err("Expecting value".into());
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let mark = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == mark {
                return Err("Expecting value".into());
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
                return Err("Expecting value".into());
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.index]).unwrap().to_string();
        Ok(Json::Number(token))
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.bump()?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err("Invalid \\uXXXX escape".into()),
            };
            value = (value << 4) | digit as u32;
        }
        Ok(value)
    }
    fn string(&mut self) -> Result<String, String> {
        if self.bump()? != b'"' {
            return Err("Expecting value".into());
        }
        let mut out = String::new();
        loop {
            let byte = self.peek().ok_or_else(|| "Unterminated string".to_string())?;
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
                                return Err("Invalid \\uXXXX escape".into());
                            }
                            let low = self.hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err("Invalid \\uXXXX escape".into());
                            }
                            0x10000 + (((unit - 0xD800) << 10) | (low - 0xDC00))
                        } else if (0xDC00..=0xDFFF).contains(&unit) {
                            return Err("Invalid \\uXXXX escape".into());
                        } else {
                            unit
                        };
                        out.push(char::from_u32(code).ok_or_else(|| "Invalid \\uXXXX escape".to_string())?);
                    }
                    _ => return Err("Invalid escape".into()),
                }
                continue;
            }
            if byte < 0x20 {
                return Err("Invalid control character".into());
            }
            let width = match byte {
                0x00..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => return Err("Invalid UTF-8".into()),
            };
            let end = self.index + width;
            let text = std::str::from_utf8(self.bytes.get(self.index..end).ok_or_else(|| "Invalid UTF-8".to_string())?)
                .map_err(|_| "Invalid UTF-8".to_string())?;
            out.push_str(text);
            self.index = end;
        }
    }
    fn object(&mut self) -> Result<Json, String> {
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
                    return Err("Expecting ',' delimiter".into());
                }
                self.skip();
            }
            if self.peek() != Some(b'"') {
                return Err("Expecting property name".into());
            }
            let key = self.string()?;
            self.skip();
            if self.bump()? != b':' {
                return Err("Expecting ':' delimiter".into());
            }
            let value = self.parse_value()?;
            if let Some((_, slot)) = pairs.iter_mut().find(|(name, _)| name == &key) {
                *slot = value;
            } else {
                pairs.push((key, value));
            }
        }
    }
    fn array(&mut self) -> Result<Json, String> {
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
                    return Err("Expecting ',' delimiter".into());
                }
                self.skip();
            }
            items.push(self.parse_value()?);
        }
    }
}

fn parse_json(text: &str) -> Result<Json, String> {
    let mut parser = Parser { bytes: text.as_bytes(), index: 0 };
    let value = parser.parse_value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err("Extra data".into());
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

fn json_text(value: &Json) -> String {
    format!("{}\n", dump(value, Some(2)))
}

fn get<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    let Json::Obj(pairs) = value else { return None };
    pairs.iter().rev().find(|(name, _)| name == key).map(|(_, item)| item)
}

fn upsert(value: &mut Json, key: &str, item: Json) -> Result<(), String> {
    let Json::Obj(pairs) = value else {
        return Err(refuse("expected an object"));
    };
    if let Some((_, slot)) = pairs.iter_mut().find(|(name, _)| name == key) {
        *slot = item;
    } else {
        pairs.push((key.to_string(), item));
    }
    Ok(())
}

fn as_i128(value: &Json) -> Option<i128> {
    let Json::Number(token) = value else { return None };
    let bare = token.strip_suffix(".0").unwrap_or(token.as_str());
    if bare.is_empty() || bare.contains(['.', 'e', 'E', '+']) {
        return None;
    }
    bare.parse().ok()
}

fn json_string(value: &Json) -> Option<String> {
    match value {
        Json::Str(text) => Some(text.clone()),
        _ => None,
    }
}

fn show_scalar(value: &Json) -> Result<String, String> {
    match value {
        Json::Number(token) => Ok(token.clone()),
        Json::Str(text) => Ok(text.clone()),
        _ => Err(refuse("expected a string or number")),
    }
}

fn obj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect())
}

fn j_int(value: i64) -> Json {
    Json::Number(value.to_string())
}

fn j_str(value: &str) -> Json {
    Json::Str(value.to_string())
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn skip_py_string(bytes: &[u8], start: usize) -> Result<usize, String> {
    let quote = bytes[start];
    if bytes.get(start..start + 3) == Some(&[quote, quote, quote]) {
        let mut index = start + 3;
        while index + 2 < bytes.len() {
            if bytes[index] == quote && bytes[index + 1] == quote && bytes[index + 2] == quote {
                return Ok(index + 3);
            }
            index += 1;
        }
        return Err(refuse("tools/checks.py has an unterminated string"));
    }
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == quote {
            return Ok(index + 1);
        }
        if bytes[index] == b'\n' {
            return Err(refuse("tools/checks.py has an unterminated string"));
        }
        index += 1;
    }
    Err(refuse("tools/checks.py has an unterminated string"))
}

fn parse_py_string(bytes: &[u8], start: usize) -> Result<(String, usize), String> {
    let quote = bytes[start];
    let mut index = start + 1;
    let mut out = String::new();
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            let next = *bytes.get(index + 1).ok_or_else(|| refuse("tools/checks.py names list"))?;
            match next {
                b'\\' => out.push('\\'),
                b'\'' => out.push('\''),
                b'"' => out.push('"'),
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                other => out.push(other as char),
            }
            index += 2;
            continue;
        }
        if byte == quote {
            return Ok((out, index + 1));
        }
        out.push(byte as char);
        index += 1;
    }
    Err(refuse("tools/checks.py names list"))
}

fn parse_names_list(source: &str) -> Result<Vec<String>, String> {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'\'' || bytes[index] == b'"' {
            index = skip_py_string(bytes, index)?;
            continue;
        }
        let boundary = index == 0 || !is_ident(bytes[index - 1]);
        if boundary && bytes.get(index..index + 5) == Some(b"names") && bytes.get(index + 5).map(|byte| !is_ident(*byte)).unwrap_or(true) {
            let mut cursor = index + 5;
            while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&b'=') {
                cursor += 1;
                while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\n')) {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(&b'[') {
                    return Err(refuse("tools/checks.py names list"));
                }
                cursor += 1;
                let mut names = Vec::new();
                loop {
                    while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\n' | b'\r' | b',')) {
                        cursor += 1;
                    }
                    if bytes.get(cursor) == Some(&b']') {
                        return Ok(names);
                    }
                    if !matches!(bytes.get(cursor), Some(b'\'' | b'"')) {
                        return Err(refuse("tools/checks.py names list"));
                    }
                    let (name, next) = parse_py_string(bytes, cursor)?;
                    names.push(name);
                    cursor = next;
                }
            }
        }
        index += 1;
    }
    Err(refuse("tools/checks.py has no names list"))
}

const EXPLICIT_PAIRS: &[(&str, &str)] = &[
    ("provider_process", "current-results"),
    ("network_drive", "network-results"),
    ("drive_ownership", "drive-ownership"),
    ("question_process", "process-results"),
    ("history", "history-results"),
    ("a2a", "a2a"),
    ("binary_flow", "binary-flow"),
    ("commands", "commands"),
    ("history_migration", "history-migration"),
    ("hosted_phases", "hosted-phases"),
    ("prompt_reference", "prompt-reference"),
    ("provider_evidence", "provider-evidence"),
    ("pe_declarations", "pe-declarations"),
    ("delegation_visibility", "delegation-visibility"),
    ("local_feedback", "local-feedback"),
    ("network_feedback", "network-feedback"),
    ("proposal_feedback", "proposal-feedback"),
    ("proposal_recovery", "proposal-recovery"),
    ("review_ownership", "review-ownership"),
    ("task_ownership", "task-ownership"),
    ("dav_listing", "dav-listing"),
    ("server_admission", "server-admission"),
    ("http_framing", "http-framing"),
    ("platform_resolution", "platform-resolution"),
    ("query_feedback", "query-feedback"),
    ("carry_flow", "carry-flow"),
    ("workspace_navigation", "workspace-navigation"),
    ("field_flow", "field-flow"),
    ("flags_flow", "flags-flow"),
    ("product_flow", "product-flow"),
    ("diagnostics", "diagnostics"),
    ("desktop_runtime", "desktop_runtime"),
    ("state_sealing", "state-sealing"),
    ("turn_background", "turn_background"),
    ("permission_modes", "permission_modes"),
];

const IDENTITY_PAIRS: &[&str] = &[
    "services",
    "media",
    "jobs",
    "voice",
    "media_stream",
    "media_upload",
    "voice_interruption",
    "voice_background",
    "voice_session",
    "capture_lifecycle",
    "scene",
    "apk_alignment",
    "audio_route",
    "document_providers",
    "snapshot_identity",
    "storage_auth",
    "storage_oauth",
    "credential_rotation",
    "voice_settings",
    "acp",
    "oauth",
    "device_authorization",
];

const HYPHEN_PAIRS: &[&str] = &[
    "gateway_expectation",
    "device_shortcut",
    "request_fields",
    "response_fields",
    "chunk_fields",
    "storage_etags",
    "storage_owner",
    "mcp_catalog",
    "mcp_catalog_events",
    "mcp_images",
    "mcp_continuation",
    "mcp_history",
    "self_rebuild",
];

fn report_pairs() -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (name, file) in EXPLICIT_PAIRS {
        pairs.push(((*name).to_string(), (*file).to_string()));
    }
    for name in IDENTITY_PAIRS {
        pairs.push(((*name).to_string(), (*name).to_string()));
    }
    for name in HYPHEN_PAIRS {
        pairs.push(((*name).to_string(), name.replace('_', "-")));
    }
    pairs
}

fn inventory_order(runs: &[String], expected: &[String]) -> Result<(), String> {
    if runs == expected {
        Ok(())
    } else {
        Err(refuse("Regression inventory differs from the current runner"))
    }
}

fn duplicate_groups(expected: &[String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for name in expected {
        if !seen.insert(name) {
            return Err(refuse(&format!("Duplicate regression group: {name}")));
        }
    }
    Ok(())
}

fn pairs_cover(expected: &[String]) -> Result<(), String> {
    let pairs: BTreeSet<String> = report_pairs().into_iter().map(|(name, _)| name).collect();
    let got: BTreeSet<String> = expected.iter().cloned().collect();
    if pairs == got {
        return Ok(());
    }
    let missing: Vec<&String> = pairs.difference(&got).collect();
    let extra: Vec<&String> = got.difference(&pairs).collect();
    Err(refuse(&format!(
        "Packaging report inventory differs from current checks (missing {missing:?}, extra {extra:?})"
    )))
}

#[derive(Clone)]
struct RunRow {
    name: String,
    code: i128,
    keys: BTreeSet<String>,
    value: Json,
}

fn exact_binary_flow(row: &RunRow) -> bool {
    row.name == "binary_flow"
        && row.code == 1
        && row.keys.len() == 2
        && row.keys.contains("name")
        && row.keys.contains("code")
}

fn candidate_gate(failed: &[RunRow], candidate: bool) -> Result<(), String> {
    if failed.is_empty() {
        return Ok(());
    }
    if candidate && failed.len() == 1 && exact_binary_flow(&failed[0]) {
        return Ok(());
    }
    let shown = failed.iter().map(|row| format!("{}={}", row.name, row.code)).collect::<Vec<_>>().join(", ");
    Err(refuse(&shown))
}

fn binary_flow_gap(value: &Json) -> Result<(), String> {
    let length = match get(value, "unavailable") {
        None => 0,
        Some(Json::Arr(items)) => items.len(),
        Some(_) => return Err(refuse("output/checks/binary-flow.json unavailable")),
    };
    let Some(retained) = get(value, "retained_compatibility") else {
        return Err(refuse("output/checks/binary-flow.json retained_compatibility"));
    };
    if length == 3 && matches!(retained, Json::Null) {
        Ok(())
    } else {
        Err(refuse("output/checks/binary-flow.json unavailable"))
    }
}

fn sequence_len(value: &Json) -> Option<usize> {
    match value {
        Json::Arr(items) => Some(items.len()),
        Json::Obj(pairs) => Some(pairs.len()),
        Json::Str(text) => Some(text.chars().count()),
        _ => None,
    }
}

fn executed_count(value: &Json) -> Result<u64, String> {
    let count = if let Some(passed) = get(value, "passed") {
        if let Some(length) = match passed {
            Json::Arr(_) => sequence_len(passed),
            _ => None,
        } {
            length as i128
        } else if let Some(number) = as_i128(passed) {
            number
        } else {
            return Err(refuse("executed count is not an integer"));
        }
    } else if let Some(number) = get(value, "count").and_then(as_i128) {
        number
    } else {
        return Err(refuse("executed count is missing"));
    };
    if count <= 0 {
        return Err(refuse("executed count is not a positive integer"));
    }
    let count = u64::try_from(count).map_err(|_| refuse("executed count is not a positive integer"))?;
    if let Some(checks) = get(value, "checks") {
        let Some(length) = sequence_len(checks) else {
            return Err(refuse("checks cross-check"));
        };
        if count != length as u64 {
            return Err(refuse("checks cross-check"));
        }
    }
    if let Some(declared) = get(value, "count") {
        let Some(number) = as_i128(declared) else {
            return Err(refuse("executed count is not an integer"));
        };
        if number != count as i128 {
            return Err(refuse("executed count differs from count"));
        }
    }
    Ok(count)
}

fn strict_lengths(checks: &[(String, Json)], counts: &[(String, u64)]) -> Result<(), String> {
    for name in IDENTITY_PAIRS {
        let Some(value) = checks.iter().find(|(key, _)| key == name).map(|(_, value)| value) else {
            return Err(refuse(&format!("{name} checks")));
        };
        let Some(items) = get(value, "checks") else {
            return Err(refuse(&format!("{name} checks")));
        };
        let Some(length) = sequence_len(items) else {
            return Err(refuse(&format!("{name} checks")));
        };
        let Some(count) = counts.iter().find(|(key, _)| key == name).map(|(_, count)| *count) else {
            return Err(refuse(&format!("{name} checks")));
        };
        if count != length as u64 {
            return Err(refuse(&format!("{name} checks")));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Match,
    Drift,
    Absent,
}

struct Census {
    pinned: usize,
    matched: usize,
    drift: usize,
    absent: usize,
}

fn classify_digest(root: &Path, relative: &str, expect: &str) -> Result<Class, String> {
    let path = root.join(relative);
    if !path.is_file() {
        return Ok(Class::Absent);
    }
    let bytes = fs::read(&path).map_err(|_| refuse(&format!("{relative} unreadable")))?;
    if hex_digest(&sha256(&bytes)) == expect {
        Ok(Class::Match)
    } else {
        Ok(Class::Drift)
    }
}

fn digest_census(root: &Path, entries: &[(String, String)]) -> Result<Census, String> {
    let mut census = Census { pinned: entries.len(), matched: 0, drift: 0, absent: 0 };
    for (relative, expect) in entries {
        match classify_digest(root, relative, expect)? {
            Class::Match => census.matched += 1,
            Class::Drift => census.drift += 1,
            Class::Absent => census.absent += 1,
        }
    }
    Ok(census)
}

fn render_census_line(label: &str, census: &Census) -> String {
    format!(
        "{label}: pinned {} match {} drift {} absent {}\n",
        census.pinned, census.matched, census.drift, census.absent
    )
}

fn manifest_pins(value: &Json) -> Result<(Vec<(String, String)>, Vec<(String, String)>), String> {
    let Some(Json::Obj(inputs)) = get(value, "build_inputs") else {
        return Err(refuse("PUBLICATION-MANIFEST.json build_inputs"));
    };
    let mut build_inputs = Vec::new();
    for (key, item) in inputs {
        let Some(digest) = json_string(item) else {
            return Err(refuse(&format!("PUBLICATION-MANIFEST.json build_inputs {key}")));
        };
        build_inputs.push((key.clone(), digest));
    }
    let Some(Json::Obj(files)) = get(value, "files") else {
        return Err(refuse("PUBLICATION-MANIFEST.json files"));
    };
    let mut file_pins = Vec::new();
    for (key, item) in files {
        let Some(digest) = get(item, "sha256").and_then(json_string) else {
            return Err(refuse(&format!("PUBLICATION-MANIFEST.json files {key}")));
        };
        file_pins.push((key.clone(), digest));
    }
    Ok((build_inputs, file_pins))
}

fn render_manifest_census(root: &Path) -> Result<String, String> {
    let path = root.join("PUBLICATION-MANIFEST.json");
    if !path.is_file() {
        return Err(refuse("PUBLICATION-MANIFEST.json absent"));
    }
    let text = fs::read_to_string(&path).map_err(|_| refuse("PUBLICATION-MANIFEST.json unreadable"))?;
    let value = parse_json(&text).map_err(|err| refuse(&format!("PUBLICATION-MANIFEST.json {err}")))?;
    let (inputs, files) = manifest_pins(&value)?;
    let inputs = digest_census(root, &inputs)?;
    let files = digest_census(root, &files)?;
    Ok(format!(
        "{}{}",
        render_census_line("build_inputs", &inputs),
        render_census_line("files", &files)
    ))
}

#[derive(Debug)]
struct UnitSuite {
    count: u64,
    label: String,
    runs: Json,
    full: Option<Json>,
}

fn passed_in_summary(summary: &str) -> Result<u64, String> {
    let bytes = summary.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if bytes.get(index..index + " passed".len()) == Some(b" passed") {
                let number = std::str::from_utf8(&bytes[start..index]).unwrap();
                return number.parse::<u64>().map_err(|_| refuse("selected-units summary"));
            }
        }
        index += 1;
    }
    Err(refuse("selected-units summary"))
}

fn unit_suite(full_units: Option<&Json>, selected: Option<&Json>) -> Result<UnitSuite, String> {
    if let Some(full) = full_units {
        let passed = as_i128(get(full, "passed").ok_or_else(|| refuse("output/checks/full-units.json passed"))?)
            .ok_or_else(|| refuse("output/checks/full-units.json passed"))?;
        let cases = as_i128(get(full, "cases").ok_or_else(|| refuse("output/checks/full-units.json cases"))?)
            .ok_or_else(|| refuse("output/checks/full-units.json cases"))?;
        let failures = get(full, "failures").ok_or_else(|| refuse("output/checks/full-units.json failures"))?;
        let empty = match failures {
            Json::Null | Json::Bool(false) => true,
            Json::Arr(items) => items.is_empty(),
            Json::Obj(pairs) => pairs.is_empty(),
            Json::Str(text) => text.is_empty(),
            Json::Number(token) => token == "0" || token == "0.0",
            Json::Bool(true) => false,
        };
        if passed != cases || cases <= 0 || !empty {
            return Err(refuse("output/checks/full-units.json failures"));
        }
        return Ok(UnitSuite {
            count: u64::try_from(passed).map_err(|_| refuse("output/checks/full-units.json passed"))?,
            label: "Rust cases from the full suite".into(),
            runs: Json::Arr(vec![]),
            full: Some(full.clone()),
        });
    }
    let Some(Json::Arr(runs)) = selected else {
        return Err(refuse("output/checks/selected-units.json absent"));
    };
    let mut total = 0u64;
    for run in runs {
        let code = as_i128(get(run, "code").ok_or_else(|| refuse("output/checks/selected-units.json code"))?)
            .ok_or_else(|| refuse("output/checks/selected-units.json code"))?;
        if code != 0 {
            return Err(refuse("output/checks/selected-units.json"));
        }
        let summary = json_string(get(run, "summary").ok_or_else(|| refuse("output/checks/selected-units.json summary"))?)
            .ok_or_else(|| refuse("output/checks/selected-units.json summary"))?;
        total += passed_in_summary(&summary)?;
    }
    if total < 199 {
        return Err(refuse(&format!("selected Rust cases {total} are below 199")));
    }
    Ok(UnitSuite {
        count: total,
        label: "selected Rust cases".into(),
        runs: Json::Arr(runs.clone()),
        full: None,
    })
}

fn cmp_rel(left: &str, right: &str) -> std::cmp::Ordering {
    left.split('/').cmp(right.split('/'))
}

const SKIP_PARTS: &[&str] = &["output", "__pycache__", ".git"];
const FORBIDDEN_SUFFIXES: &[&str] = &["p12", "jks", "password", "keystore", "zip", "tar", "xz"];
const BUILD_ROOTS: &[&str] = &["core", "platform", "res", "notices"];
const BUILD_FILES: &[&str] = &["AndroidManifest.xml", "package.sh", "tools/alignment.rs"];

fn extension_of(relative: &str) -> Option<&str> {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    Some(ext)
}

fn screen_relative(relative: &str, symlink: bool) -> Result<(), String> {
    if symlink {
        return Err(refuse(&format!("symlink {relative}")));
    }
    if extension_of(relative) == Some("apk") {
        return Err(refuse(&format!("The signed APK is distributed separately: {relative}")));
    }
    if let Some(ext) = extension_of(relative) {
        if FORBIDDEN_SUFFIXES.contains(&ext) {
            return Err(refuse(&format!("{relative} has a refused suffix .{ext}")));
        }
    }
    Ok(())
}

fn is_build_input(relative: &str) -> bool {
    let root = relative.split('/').next().unwrap_or(relative);
    BUILD_ROOTS.contains(&root) || BUILD_FILES.contains(&relative)
}

fn excluded_relative(relative: &str) -> bool {
    relative.split('/').any(|part| SKIP_PARTS.contains(&part)) || relative.rsplit('/').next() == Some("PUBLICATION-MANIFEST.json")
}

fn collect_tree(root: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    fn walk(dir: &Path, root: &Path, files: &mut Vec<String>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|_| refuse(&format!("{} unreadable", dir.display())))?;
        for entry in entries {
            let entry = entry.map_err(|_| refuse(&format!("{} unreadable", dir.display())))?;
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|_| refuse("path escaped the project"))?;
            let text = relative.to_str().ok_or_else(|| refuse("path is not UTF-8"))?.replace('\\', "/");
            if excluded_relative(&text) {
                continue;
            }
            let meta = fs::symlink_metadata(&path).map_err(|_| refuse(&format!("{text} unreadable")))?;
            if meta.file_type().is_symlink() {
                if path.is_file() {
                    files.push(text);
                }
                continue;
            }
            if meta.is_dir() {
                walk(&path, root, files)?;
            } else if meta.is_file() {
                files.push(text);
            }
        }
        Ok(())
    }
    walk(root, root, &mut files)?;
    files.sort_by(|left, right| cmp_rel(left, right));
    files.dedup();
    Ok(files)
}

fn screen_files(root: &Path, files: &[String]) -> Result<(), String> {
    for relative in files {
        let path = root.join(relative);
        let symlink = fs::symlink_metadata(&path).map(|meta| meta.file_type().is_symlink()).unwrap_or(false);
        screen_relative(relative, symlink)?;
    }
    Ok(())
}

const SUMMARY: &str = r#"Auto chooses a direct reply, a short current-fact lookup, or deeper investigation. The conversation has explicit mode choices. Simple local time and date questions use the device clock. Quick lookups keep full source and error details available on tap, and a verified DJIA daily close is labeled with its observation date. Automatic web access now includes ordinary public HTTPS pages and marketplace searches opened directly by primary or delegated agents. The app checks the resolved address before connecting and applies the same public scope to every redirect. Requests for private destinations, page scripts and captures follow existing review controls. Supported OpenAI, xAI, Anthropic and Gemini connections can use provider-hosted search on both the primary agent and delegated agents. Provider searches run on the selected connection, and RLLM keeps returned source metadata. One evidence-only follow-up can recover an empty answer after search without repeating the query. Automatic web search approval is on by default for configured search sources. A single valid search proposal in model prose runs under the same approval policy, and eligible ambiguous proposals get one bounded model correction. Web-search fallback and subsequent queries stay in the current turn. Chat and local streaming enforce declared lengths and encodings and validate chunk boundaries and trailers before adopting text or completion. Bodyless HTTP 204 responses cannot publish reply text. Owned responses enforce the shared 16 KiB trailer limit. Framed MCP control acknowledgments settle after informational headers without waiting for a close. An explicit search deferral requires fresh approval even when automatic or saved approval is enabled, and omits the provider-hosted search tool for that turn. RLLM offers a second bounded refinement round after a comparable partial gain, keeps the stronger tested instructions, and compares JSON values in separate evaluation. Shared response framing accepts split delimiters at the exact 8 KiB chunk-size and 64 KiB response-header limits. RLLM can evaluate the original and retained agent instructions on up to two separate tasks with local phrase, exact-text, JSON or digest checks. Both retained replies must pass comparable checks before the evaluation can contribute a measured lesson. Evaluation proposals, interrupted responses and incomparable contexts cannot promote a lesson. A completed training candidate remains reviewable after an interrupted evaluation. RLLM learns the default built-in search order from validated same-query fallbacks. Checked agent refinements replay the exact revised instructions and retain a passing candidate when a revision fails. User adoption preserves the original agent profile. Duplicate correlated MCP POST stream replies are refused before result adoption. RLLM retains multiple named MCP connections and uses one selected server for discovery, calls and streams. A refresh publishes only the complete tool catalog and refuses duplicate tool names across pages. Ambiguous names reported by the active connection require a successful refresh before use, including after switching away and back. New conversation leads the plus menu. An explicit request to search the web authorizes built-in queries for the current turn without repeated prompts, including a follow-up command that relies on the current topic. An unavailable built-in source can fail over, and raw search diagnostics stay in message details. Higher-priority destination review remains available. Conversation displays compact web results and keeps raw tool JSON in message details. Storage authorization uses the configured certificate hostname on the matching HTTPS route while separate origins retain their own identity. Shared Rust HTTP request and response handling validates field syntax, credential-header boundaries, gateway request lengths and expectations, informational responses and chunk extensions. Optional pre-filled device verification links retain the manual address and code and fall back on unusable links. Safe WebDAV updates also preserve valid UTF-8 opaque ETags in exact conditional writes while retaining weak, malformed and duplicate validator refusals. Shared token adoption refuses non-text refresh credentials before replacing saved grants and preserves scoped uncertain-refresh recovery. Existing conversation, intrinsic investigation, local storage and optional provider-neutral services remain integrated. The signed candidate matches the recorded cumulative source inputs. Recursive refinement can check individual JSON fields in training and held-out tasks. It keeps expected values local and only treats a partial score increase as progress when earlier passing checks still pass. WebDAV listings respect declared XML namespaces, preserving valid file and folder routing when extension elements use similar names. An owned media feed failure retires its ticket before it can mint a completion receipt, including when the HTTP framer has reached its last byte. Structured refinement can unwrap a single JSON code block for local verification and records the repair separately. A failed owned import cannot issue a receipt for its staged prefix. A failed upload read retires only its own ticket before another read can advance it. Media chunks reject malformed Base64 instead of accepting a decodable prefix."#;

const LIMITS_OPEN: &str = "Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and whole-product independent acceptance remain unverified. Physical background/screen-off and battery behavior remain unverified. Bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device. The historical revision-two comparator is unavailable; its three compatibility checks remain open.";
const LIMITS_CLOSED: &str = "Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and whole-product independent acceptance remain unverified. Physical background/screen-off and battery behavior remain unverified. Bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device.";
const LIMITS_SCENE: &str = " Scene preview displays static triangle geometry with base and vertex colors. Embedded PNG base-color textures and MASK cutouts are displayed. External images and other image codecs retain geometry-only viewing; animation and physical material effects remain open. Skins, morph targets, blended transparency and required unsupported extensions refuse only preview; export remains available. GPU rendering and wider scene formats remain open.";
const LIMITS_PARTIAL: &str = " Full Rust suite execution is recorded separately when available.";

fn limits_text(failed: bool, full_units: bool) -> String {
    let mut text = if failed { LIMITS_OPEN } else { LIMITS_CLOSED }.to_string();
    text.push_str(LIMITS_SCENE);
    if !full_units {
        text.push_str(LIMITS_PARTIAL);
    }
    text
}

fn splice_readme(text: &str, version: &str) -> Result<String, String> {
    let marker = "# RLLM\n\n";
    let start = text.find(marker).ok_or_else(|| refuse("README.md marker # RLLM"))? + marker.len();
    let end = text[start..].find("RLLM connects your Android device").ok_or_else(|| refuse("README.md marker RLLM connects your Android device"))? + start;
    let inserted = format!("Current signed candidate {version} includes the MCP connections, conversation menu, web search, transport and authorization changes described in the release notes. Owner validation, unavailable historical comparisons and pending device/independent acceptance are recorded separately.\n\n");
    let mut text = format!("{}{inserted}{}", &text[..start], &text[end..]);
    text = text.replace(
        "The source includes one matching signed RLLM APK in release/RLLM.apk.",
        "The separately supplied signed APK matches this source; its SHA-256 is recorded in PUBLICATION-MANIFEST.json.",
    );
    Ok(text)
}

fn notes_text(version: &str, passed: usize, groups: usize, total: u64, units: u64, label: &str, limits: &str, bytes: &str, sha: &str) -> String {
    format!(
        "# RLLM {version} — signed candidate\n\n{SUMMARY}\n\n{passed}/{groups} connected groups passed; {total} executed checks and {units} {label}. Incomplete groups are explicitly recorded in verification/RESULTS.json.\n\n{limits}\n\nPackage com.local.client.lab, Android 8.0+, ARM64 and x86-64; retained current signing identity, APK CRC, exact build inputs and 16 KiB alignment verified. APK {bytes} bytes, SHA256 {sha}.\n"
    )
}

fn status_text(passed: usize, groups: usize, total: u64, units: u64, label: &str, limits: &str) -> String {
    format!(
        "# Current implementation\n\n{SUMMARY}\n\nSetup, mappings, job behavior and limits: [SERVICES.md](SERVICES.md). The established text, investigation, web, file, storage, MCP/A2A and delegation paths remain part of the same app.\n\nValidation: {passed}/{groups} connected groups, {total} executed checks, {units} {label}. Reports describe owner execution, not independent acceptance.\n\n{limits}\n"
    )
}

fn progress_text(stamp: &str, version: &str, passed: usize, groups: usize, total: u64, units: u64, label: &str, bytes: &str, sha: &str, limits: &str) -> String {
    format!(
        "# RLLM checkpoint — {stamp}\n\nBuild {version}; signed candidate. {SUMMARY}\n\nValidation: {passed}/{groups} connected groups, {total} executed checks, {units} {label}. APK {bytes} bytes, SHA256 {sha}. Retained signer ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5. The matching signed APK is supplied separately.\n\n{limits}\n\nProtected baseline: version1793750802, immutable source SHA2562676d5476035cc32db9b85f652c94ecc5e21ca09f925ac2dc983cce932e94917, baseline commit36a5075caa58110ba33ef58990d91c1a102ae970. Immutable baseline source Drive1pBK9HP49rMqBuftLOhH_IMfgWIs0gIdB, APK1mFRkR3ubuMLrVE0Y0iDroRlWivpChtqi. Do not overwrite those baseline copies.\n\nNext: publish and verify this signed candidate. Broaden product-owned semantic repair and reusable deterministic verification. Continue the protocol, storage and resource roadmap with remaining integration gaps. Recover genuine revision-two source through the selected stable source's revision history if browser authentication becomes available; run all three historical checks without changing the reader fingerprint or current decoder. Complete current-device/live-endpoint and independent acceptance separately. Earlier broader accepted roadmap remains in the canonical cumulative engineering record, not cancelled by this upgrade: https://docs.google.com/document/d/1jyJ1ma1juoWacfJFpei5MdzPS05i7EiNy-FyWs9UPRI/edit . This build does not claim whole-project closure. The existing hourly build task Automation_787bc0676d388191bc52212c823ab48b and hourly delivery task Automation_1188b1045d7481919a5069a82a9def98 were updated and read back. Both are enabled. Their prompts prefer the newest verified project Drive source and reconcile later working changes. The scheduler did not expose the next-run timestamps at this checkpoint. Signing custody remains in the separate protected archive.\n"
    )
}

fn stamp_matches(stamp: &str) -> bool {
    let bytes = stamp.as_bytes();
    if bytes.len() != 21 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(|byte| byte.is_ascii_digit());
    digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && &bytes[10..12] == b"__"
        && digits(12..14)
        && bytes[14] == b'-'
        && digits(15..17)
        && bytes[17] == b'-'
        && digits(18..20)
        && bytes[20] == b'Z'
}

fn checkpoint_utc(now: SystemTime) -> String {
    let elapsed = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let unix = elapsed.as_secs() as i64;
    let micros = elapsed.subsec_micros();
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
    if micros == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}+00:00")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}+00:00")
    }
}

fn read_required(root: &Path, relative: &str) -> Result<Json, String> {
    let path = root.join(relative);
    if !path.is_file() {
        return Err(refuse(&format!("{relative} absent")));
    }
    let text = fs::read_to_string(&path).map_err(|_| refuse(&format!("{relative} unreadable")))?;
    parse_json(&text).map_err(|err| refuse(&format!("{relative} {err}")))
}

fn read_optional(root: &Path, relative: &str) -> Result<Option<Json>, String> {
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(refuse(&format!("{relative} unreadable"))),
        Ok(meta) if meta.is_dir() => Err(refuse(&format!("{relative} absent"))),
        Ok(_) if !path.is_file() => Ok(None),
        Ok(_) => Ok(Some(read_required(root, relative)?)),
    }
}

fn selected_filters(previous: &Json) -> Result<Json, String> {
    if let Some(value) = get(previous, "selected_rust_filters") {
        return Ok(value.clone());
    }
    let Some(runs) = get(previous, "selected_rust_runs") else {
        return Ok(Json::Arr(vec![]));
    };
    let Json::Arr(items) = runs else {
        return Err(refuse("verification/RESULTS.json selected_rust_runs"));
    };
    let mut filters = Vec::new();
    for item in items {
        let Some(filter) = get(item, "filter") else {
            return Err(refuse("verification/RESULTS.json filter"));
        };
        filters.push(filter.clone());
    }
    Ok(Json::Arr(filters))
}

fn load_runs(value: &Json) -> Result<Vec<RunRow>, String> {
    let Json::Arr(items) = value else {
        return Err(refuse("output/checks/current-regression.json"));
    };
    let mut runs = Vec::new();
    for item in items {
        let Json::Obj(pairs) = item else {
            return Err(refuse("output/checks/current-regression.json"));
        };
        let Some(name) = get(item, "name").and_then(json_string) else {
            return Err(refuse("output/checks/current-regression.json name"));
        };
        let Some(code) = get(item, "code").and_then(as_i128) else {
            return Err(refuse(&format!("output/checks/current-regression.json {name} code")));
        };
        runs.push(RunRow {
            name,
            code,
            keys: pairs.iter().map(|(key, _)| key.clone()).collect(),
            value: item.clone(),
        });
    }
    Ok(runs)
}

fn run_signature(program: &Path, apk: &Path) -> Result<(), String> {
    if !program.is_file() {
        return Err(refuse("output/tools/signature absent"));
    }
    let output = Command::new(program).arg(apk).output().map_err(|err| refuse(&format!("output/tools/signature {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&output.stderr);
    let err = err.trim();
    let tail: String = err.chars().rev().take(300).collect::<String>().chars().rev().collect();
    Err(refuse(&format!("output/tools/signature rejected {}: {tail}", apk.display())))
}

struct Artifact {
    path: PathBuf,
    bytes: Vec<u8>,
}

struct Plan {
    before: Vec<Artifact>,
    apk: Artifact,
    after: Vec<Artifact>,
    signature: PathBuf,
    preview: String,
    printed: String,
}

impl Plan {
    fn commit(self) -> Result<String, String> {
        for artifact in self.before.iter().chain(std::iter::once(&self.apk)) {
            if let Some(parent) = artifact.path.parent() {
                if parent.file_name().and_then(|name| name.to_str()) == Some("release") {
                    fs::create_dir_all(parent).map_err(|_| refuse(&format!("{} unwritable", parent.display())))?;
                }
            }
            fs::write(&artifact.path, &artifact.bytes).map_err(|_| refuse(&format!("{} unwritable", artifact.path.display())))?;
        }
        run_signature(&self.signature, &self.apk.path)?;
        for artifact in &self.after {
            if let Some(parent) = artifact.path.parent() {
                fs::create_dir_all(parent).map_err(|_| refuse(&format!("{} unwritable", parent.display())))?;
            }
            fs::write(&artifact.path, &artifact.bytes).map_err(|_| refuse(&format!("{} unwritable", artifact.path.display())))?;
        }
        Ok(self.printed)
    }
}

fn push_preview(preview: &mut String, path: &Path, bytes: &[u8]) {
    preview.push_str(&format!("would write: {} bytes {}\n", path.display(), bytes.len()));
}

struct PublishOptions<'a> {
    root: &'a Path,
    destination: &'a Path,
    candidate: bool,
    dry_run: bool,
}

fn prepare(opts: &PublishOptions<'_>) -> Result<Plan, String> {
    let root = opts.root;
    let apk = read_required(root, "output/checks/apk.json")?;
    let apk_path_text = json_string(get(&apk, "path").ok_or_else(|| refuse("output/checks/apk.json path"))?)
        .ok_or_else(|| refuse("output/checks/apk.json path"))?;
    let apk_path = PathBuf::from(&apk_path_text);
    if !apk_path.is_file() {
        return Err(refuse(&format!("{apk_path_text} absent")));
    }
    let apk_bytes = fs::read(&apk_path).map_err(|_| refuse(&format!("{apk_path_text} unreadable")))?;
    let declared_len = as_i128(get(&apk, "bytes").ok_or_else(|| refuse("output/checks/apk.json bytes"))?)
        .ok_or_else(|| refuse("output/checks/apk.json bytes"))?;
    let declared_sha = json_string(get(&apk, "sha256").ok_or_else(|| refuse("output/checks/apk.json sha256"))?)
        .ok_or_else(|| refuse("output/checks/apk.json sha256"))?;
    if declared_len != apk_bytes.len() as i128 || declared_sha != hex_digest(&sha256(&apk_bytes)) {
        return Err(refuse(&format!("{apk_path_text} bytes or sha256 mismatch")));
    }
    let Some(Json::Obj(input_pairs)) = get(&apk, "build_inputs") else {
        return Err(refuse("output/checks/apk.json build_inputs"));
    };
    let mut apk_inputs = BTreeMap::new();
    for (relative, digest) in input_pairs {
        let Some(digest) = json_string(digest) else {
            return Err(refuse(&format!("{relative} digest mismatch")));
        };
        let path = root.join(relative);
        if !path.is_file() {
            return Err(refuse(&format!("{relative} absent")));
        }
        let actual = hex_digest(&sha256(&fs::read(&path).map_err(|_| refuse(&format!("{relative} unreadable")))?));
        if actual != digest {
            return Err(refuse(&format!("{relative} digest mismatch")));
        }
        apk_inputs.insert(relative.clone(), digest);
    }
    let full_path = root.join("output/checks/full-units.json");
    let units = if full_path.is_file() {
        let full = read_required(root, "output/checks/full-units.json")?;
        unit_suite(Some(&full), None)?
    } else {
        let selected = read_required(root, "output/checks/selected-units.json")?;
        unit_suite(None, Some(&selected))?
    };
    let regression = read_required(root, "output/checks/current-regression.json")?;
    let runs = load_runs(&regression)?;
    let names_source = fs::read_to_string(root.join("tools/checks.py")).map_err(|_| refuse("tools/checks.py absent"))?;
    let expected = parse_names_list(&names_source)?;
    let run_names: Vec<String> = runs.iter().map(|row| row.name.clone()).collect();
    inventory_order(&run_names, &expected)?;
    duplicate_groups(&expected)?;
    let failed: Vec<RunRow> = runs.iter().filter(|row| row.code != 0).cloned().collect();
    candidate_gate(&failed, opts.candidate)?;
    pairs_cover(&expected)?;
    let pairs = report_pairs();
    let mut checks = Vec::new();
    for (name, file) in &pairs {
        let relative = format!("output/checks/{file}.json");
        checks.push((name.clone(), read_required(root, &relative)?));
    }
    if !failed.is_empty() {
        let Some((_, binary)) = checks.iter().find(|(name, _)| name == "binary_flow") else {
            return Err(refuse("output/checks/binary-flow.json absent"));
        };
        binary_flow_gap(binary)?;
    }
    let mut counts = Vec::new();
    for (name, value) in &checks {
        let count = executed_count(value).map_err(|err| {
            if err.contains(name) {
                err
            } else {
                refuse(&format!("{name}: {err}"))
            }
        })?;
        counts.push((name.clone(), count));
    }
    strict_lengths(&checks, &counts)?;
    let published = read_required(root, "output/checks/published-comparator.json")?;
    let source_path = json_string(get(&published, "source_path").ok_or_else(|| refuse("output/checks/published-comparator.json source_path"))?)
        .ok_or_else(|| refuse("output/checks/published-comparator.json source_path"))?;
    let source_sha = json_string(get(&published, "source_sha256").ok_or_else(|| refuse("output/checks/published-comparator.json source_sha256"))?)
        .ok_or_else(|| refuse("output/checks/published-comparator.json source_sha256"))?;
    let source_bytes = fs::read(Path::new(&source_path)).map_err(|_| refuse(&format!("{source_path} absent")))?;
    if hex_digest(&sha256(&source_bytes)) != source_sha {
        return Err(refuse("Published comparator source changed"));
    }
    let controller = root.join("output/published-controller");
    let controller_bytes = fs::read(&controller).map_err(|_| refuse("output/published-controller absent"))?;
    let controller_sha = json_string(get(&published, "controller_sha256").ok_or_else(|| refuse("output/checks/published-comparator.json controller_sha256"))?)
        .ok_or_else(|| refuse("output/checks/published-comparator.json controller_sha256"))?;
    if hex_digest(&sha256(&controller_bytes)) != controller_sha {
        return Err(refuse("Published comparator executable changed"));
    }
    let version_value = get(&apk, "version").ok_or_else(|| refuse("output/checks/apk.json version"))?.clone();
    let version = show_scalar(&version_value)?;
    let bytes_text = show_scalar(get(&apk, "bytes").ok_or_else(|| refuse("output/checks/apk.json bytes"))?)?;
    let sha_text = declared_sha.clone();
    let stamp = apk_path.parent().and_then(|path| path.file_name()).and_then(|name| name.to_str()).unwrap_or("").to_string();
    if !stamp_matches(&stamp) {
        return Err(refuse(&format!("{apk_path_text} parent {stamp} is not a publication stamp")));
    }
    let total: u64 = counts.iter().map(|(_, count)| *count).sum();
    let groups = runs.len();
    let passed = groups - failed.len();
    let limits = limits_text(!failed.is_empty(), units.full.is_some());
    let mut connected_counts = Vec::new();
    for (name, count) in &counts {
        connected_counts.push((name.clone(), j_int(i64::try_from(*count).map_err(|_| refuse("executed count"))?)));
    }
    let unavailable = Json::Arr(failed.iter().map(|row| row.value.clone()).collect());
    let connected = obj(vec![
        ("counts", Json::Obj(connected_counts)),
        ("total_executed", j_int(i64::try_from(total).unwrap_or(i64::MAX))),
        ("groups_passed", j_int(passed as i64)),
        ("groups", j_int(groups as i64)),
        ("unavailable", unavailable),
    ]);
    let mut validation = obj(vec![
        ("connected", connected.clone()),
        ("selected_rust_cases", if units.full.is_some() { Json::Null } else { j_int(units.count as i64) }),
        ("selected_rust_runs", units.runs),
        ("checks", Json::Obj(checks.iter().map(|(name, value)| (name.clone(), value.clone())).collect())),
        (
            "apk",
            match &apk {
                Json::Obj(pairs) => Json::Obj(pairs.iter().filter(|(key, _)| key != "path").cloned().collect()),
                _ => Json::Obj(vec![]),
            },
        ),
        ("evidence_owner", j_str("implementation owner; real local HTTP/file fixtures, compiled prior core and LLVM references")),
        ("published_comparator", published),
        (
            "flow_comparator",
            obj(vec![
                ("version", j_int(1793395702)),
                ("source_sha256", j_str("f9d0dac9e3e8ae02cf5e787ffa71871f7d1242020fa6c87a3740af5b2d827ab8")),
                ("method", j_str("Actual prior core with thin original JSON-lines envelope")),
            ]),
        ),
        (
            "authentication_comparator",
            obj(vec![
                ("version", j_int(1793593529)),
                ("source_sha256", j_str("af219d4d2269f8f9ef0b9b9656106a8eca5dedbcebc5ef95047216de0c26b289")),
                ("available", Json::Bool(failed.is_empty())),
                ("method", j_str("Genuine comparator required; no substitution or weakened historical reader fingerprint")),
            ]),
        ),
        ("independent_acceptance", Json::Bool(false)),
        ("installed_device_tested", Json::Bool(false)),
        ("live_service_endpoints_tested", Json::Bool(false)),
        ("live_model_NAS_tested", Json::Bool(false)),
    ]);
    if let Some(full) = &units.full {
        let previous = read_required(root, "verification/RESULTS.json")?;
        upsert(&mut validation, "full_rust_suite", full.clone())?;
        upsert(&mut validation, "selected_rust_filters", selected_filters(&previous)?)?;
    }
    let upgrade = read_required(root, "verification/UPGRADE-AUDIT.json")?;
    let historical_results = get(&upgrade, "full_rust_suite").cloned().ok_or_else(|| refuse("verification/UPGRADE-AUDIT.json full_rust_suite"))?;
    upsert(&mut validation, "upgrade_audit", upgrade)?;
    upsert(
        &mut validation,
        "historical_full_rust_suite",
        obj(vec![
            ("source_version", j_int(1793837555)),
            ("source_sha256", j_str("6a24fa55a6cd10e299c49655858772866fa849c3c37aa99bfd34772b49eabfdb")),
            ("results", historical_results),
            ("scope", j_str("Original historical run retained separately from current execution.")),
        ]),
    )?;
    const SLICES: &[(&str, &str)] = &[
        ("output/checks/answer-mode-flow.json", "answer_mode_flow"),
        ("output/checks/public-page-flow.json", "public_page_flow"),
        ("output/search-flow.json", "provider_search_flow"),
        ("output/checks/live-web.json", "live_public_web"),
        ("output/checks/independent-improvement.json", "independent_improvement_slice"),
        ("output/checks/independent-file-transfer.json", "independent_file_transfer_slice"),
        ("output/checks/independent-media.json", "independent_media_slice"),
        ("output/checks/independent-storage.json", "independent_storage_slice"),
        ("output/checks/independent-transport.json", "independent_transport_slice"),
        ("output/checks/independent-search.json", "independent_search_slice"),
    ];
    for (relative, key) in SLICES {
        if let Some(value) = read_optional(root, relative)? {
            upsert(&mut validation, key, value)?;
        }
    }
    let results_bytes = json_text(&validation).into_bytes();
    let notes = notes_text(&version, passed, groups, total, units.count, &units.label, &limits, &bytes_text, &sha_text).into_bytes();
    let status = status_text(passed, groups, total, units.count, &units.label, &limits).into_bytes();
    let progress = progress_text(&stamp, &version, passed, groups, total, units.count, &units.label, &bytes_text, &sha_text, &limits).into_bytes();
    let readme_text = fs::read_to_string(root.join("README.md")).map_err(|_| refuse("README.md absent"))?;
    let readme = splice_readme(&readme_text, &version)?.into_bytes();
    let mut overlay: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    overlay.insert("verification/RESULTS.json".into(), results_bytes.clone());
    overlay.insert("release/NOTES.md".into(), notes.clone());
    overlay.insert("docs/STATUS.md".into(), status.clone());
    overlay.insert("project-progress.md".into(), progress.clone());
    overlay.insert("README.md".into(), readme.clone());
    let mut files = collect_tree(root)?;
    for relative in overlay.keys() {
        if !files.iter().any(|have| have == relative) {
            files.push(relative.clone());
        }
    }
    files.sort_by(|left, right| cmp_rel(left, right));
    screen_files(root, &files)?;
    let mut file_records = Vec::new();
    let mut member_bytes = Vec::new();
    let mut walked_inputs = BTreeMap::new();
    for relative in &files {
        let bytes = if let Some(stored) = overlay.get(relative) {
            stored.clone()
        } else {
            fs::read(root.join(relative)).map_err(|_| refuse(&format!("{relative} unreadable")))?
        };
        if is_build_input(relative) {
            walked_inputs.insert(relative.clone(), hex_digest(&sha256(&bytes)));
        }
        file_records.push((
            relative.clone(),
            obj(vec![("bytes", j_int(bytes.len() as i64)), ("sha256", j_str(&hex_digest(&sha256(&bytes))))]),
        ));
        member_bytes.push((relative.clone(), bytes));
    }
    if walked_inputs != apk_inputs {
        let mut differ = String::new();
        let keys: BTreeSet<_> = walked_inputs.keys().chain(apk_inputs.keys()).collect();
        for key in keys {
            if walked_inputs.get(key) != apk_inputs.get(key) {
                differ = key.clone();
                break;
            }
        }
        return Err(refuse(&format!("Source differs from inspected APK inputs: {differ}")));
    }
    let manifest_path = root.join("PUBLICATION-MANIFEST.json");
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|_| refuse("PUBLICATION-MANIFEST.json absent"))?;
    let mut manifest = parse_json(&manifest_text).map_err(|err| refuse(&format!("PUBLICATION-MANIFEST.json {err}")))?;
    let published_name = format!("RLLM__Candidate-Application__{stamp}.apk");
    let source_name = format!("RLLM__Candidate-Source__{stamp}.zip");
    let sums_name = format!("RLLM__Candidate-Checksums__{stamp}.txt");
    upsert(&mut manifest, "state", j_str("in_progress; signed candidate"))?;
    upsert(&mut manifest, "version", version_value)?;
    upsert(
        &mut manifest,
        "signed_apk",
        obj(vec![
            ("location", j_str("companion")),
            ("path", j_str(&published_name)),
            ("bytes", j_int(apk_bytes.len() as i64)),
            ("sha256", j_str(&sha_text)),
        ]),
    )?;
    upsert(
        &mut manifest,
        "validation",
        obj(vec![
            ("path", j_str("verification/RESULTS.json")),
            ("connected", connected),
            ("rust_cases", j_int(units.count as i64)),
            ("rust_scope", j_str(&units.label)),
        ]),
    )?;
    upsert(&mut manifest, "whole_project_complete", Json::Bool(false))?;
    upsert(&mut manifest, "checkpoint_utc", j_str(&checkpoint_utc(SystemTime::now())))?;
    upsert(&mut manifest, "files", Json::Obj(file_records))?;
    let mut input_pairs: Vec<(String, String)> = walked_inputs.into_iter().collect();
    input_pairs.sort_by(|left, right| cmp_rel(&left.0, &right.0));
    let build_input_len = input_pairs.len();
    let build_input_json = Json::Obj(input_pairs.into_iter().map(|(key, digest)| (key, j_str(&digest))).collect());
    upsert(&mut manifest, "build_inputs", build_input_json)?;
    let manifest_bytes = json_text(&manifest).into_bytes();
    member_bytes.push(("PUBLICATION-MANIFEST.json".into(), manifest_bytes.clone()));
    member_bytes.sort_by(|left, right| cmp_rel(&left.0, &right.0));
    let zip_members: Vec<(String, Vec<u8>)> = member_bytes.iter().map(|(relative, bytes)| (format!("RLLM/{relative}"), bytes.clone())).collect();
    let zip_bytes = archive_bytes(&zip_members)?;
    let opened = read_archive(&zip_bytes).map_err(|err| refuse(&format!("{source_name} {err}")))?;
    if opened.len() != file_records_len(&manifest) {
        return Err(refuse(&format!("{source_name} member count")));
    }
    for (relative, record) in manifest_files(&manifest)? {
        let name = format!("RLLM/{relative}");
        let Some((_, data)) = opened.iter().find(|(member, _)| member == &name) else {
            return Err(refuse(&format!("{source_name} missing {relative}")));
        };
        let expect_len = as_i128(get(&record, "bytes").ok_or_else(|| refuse(&format!("{relative} bytes")))?)
            .ok_or_else(|| refuse(&format!("{relative} bytes")))?;
        let expect_sha = json_string(get(&record, "sha256").ok_or_else(|| refuse(&format!("{relative} sha256")))?)
            .ok_or_else(|| refuse(&format!("{relative} sha256")))?;
        if data.len() as i128 != expect_len || hex_digest(&sha256(data)) != expect_sha {
            return Err(refuse(&format!("{source_name} {relative}")));
        }
    }
    let destination = fs::canonicalize(opts.destination).unwrap_or_else(|_| opts.destination.to_path_buf());
    let published_path = destination.join(&published_name);
    let source_path_out = destination.join(&source_name);
    let sums_path = destination.join(&sums_name);
    let apk_sha = hex_digest(&sha256(&apk_bytes));
    let zip_sha = hex_digest(&sha256(&zip_bytes));
    let sums = format!("{apk_sha}  {published_name}\n{zip_sha}  {source_name}\n").into_bytes();
    let record = Json::Obj(vec![
        ("status".into(), j_str("signed candidate; acceptance incomplete")),
        ("version".into(), get(&manifest, "version").cloned().unwrap_or(Json::Null)),
        ("members".into(), j_int(opened.len() as i64)),
        ("build_inputs".into(), j_int(build_input_len as i64)),
        (
            "paths".into(),
            Json::Obj(vec![
                ("apk".into(), j_str(&published_path.display().to_string())),
                ("source".into(), j_str(&source_path_out.display().to_string())),
                ("checksums".into(), j_str(&sums_path.display().to_string())),
            ]),
        ),
        (
            "files".into(),
            Json::Obj(vec![
                (published_name.clone(), obj(vec![("bytes", j_int(apk_bytes.len() as i64)), ("sha256", j_str(&apk_sha))])),
                (source_name.clone(), obj(vec![("bytes", j_int(zip_bytes.len() as i64)), ("sha256", j_str(&zip_sha))])),
                (sums_name.clone(), obj(vec![("bytes", j_int(sums.len() as i64)), ("sha256", j_str(&hex_digest(&sha256(&sums))))])),
            ]),
        ),
    ]);
    let packaged = json_text(&record).into_bytes();
    let printed = dump(&record, Some(2));
    let signature = root.join("output/tools/signature");
    if opts.dry_run {
        run_signature(&signature, &apk_path)?;
    }
    let before = vec![
        Artifact { path: root.join("verification/RESULTS.json"), bytes: results_bytes },
        Artifact { path: root.join("release/NOTES.md"), bytes: notes },
        Artifact { path: root.join("docs/STATUS.md"), bytes: status },
        Artifact { path: root.join("project-progress.md"), bytes: progress },
        Artifact { path: root.join("README.md"), bytes: readme },
        Artifact { path: manifest_path, bytes: manifest_bytes },
        Artifact { path: source_path_out.clone(), bytes: zip_bytes },
    ];
    let apk_artifact = Artifact { path: published_path.clone(), bytes: apk_bytes };
    let after = vec![
        Artifact { path: sums_path, bytes: sums },
        Artifact { path: root.join("output/checks/packaged.json"), bytes: packaged },
    ];
    let mut preview = String::new();
    for artifact in before.iter().chain(std::iter::once(&apk_artifact)).chain(after.iter()) {
        push_preview(&mut preview, &artifact.path, &artifact.bytes);
    }
    preview.push_str(&format!("would run: {} {}\n", signature.display(), published_path.display()));
    Ok(Plan { before, apk: apk_artifact, after, signature, preview, printed })
}

fn file_records_len(manifest: &Json) -> usize {
    match get(manifest, "files") {
        Some(Json::Obj(pairs)) => pairs.len() + 1,
        _ => 0,
    }
}

fn manifest_files(manifest: &Json) -> Result<Vec<(String, Json)>, String> {
    let Some(Json::Obj(pairs)) = get(manifest, "files") else {
        return Err(refuse("PUBLICATION-MANIFEST.json files"));
    };
    Ok(pairs.iter().cloned().collect())
}

fn project_root() -> Result<PathBuf, String> {
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
            if dir.join("tools/package.rs").is_file() && dir.join("tools/checks.py").is_file() {
                return Ok(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    Err(refuse("cannot locate the RLLM project root"))
}

struct Cli {
    destination: PathBuf,
    candidate: bool,
    dry_run: bool,
}

fn parse_cli(args: &[String]) -> Result<Cli, i32> {
    let mut destination = None;
    let mut candidate = false;
    let mut dry_run = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-h" || arg == "--help" {
            print!("{USAGE}");
            let _ = io::stdout().flush();
            return Err(0);
        }
        if arg == "--candidate" {
            candidate = true;
            index += 1;
            continue;
        }
        if arg == "--dry-run" {
            dry_run = true;
            index += 1;
            continue;
        }
        if arg == "--destination" || arg.starts_with("--destination=") {
            let value = if let Some(inline) = arg.strip_prefix("--destination=") {
                inline.to_string()
            } else {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("{USAGE}package: error: --destination requires a directory");
                    return Err(2);
                };
                value.clone()
            };
            destination = Some(PathBuf::from(value));
            index += 1;
            continue;
        }
        eprintln!("{USAGE}package: error: unrecognized argument {arg}");
        return Err(2);
    }
    let Some(destination) = destination else {
        eprintln!("{USAGE}package: error: --destination is required");
        return Err(2);
    };
    Ok(Cli { destination, candidate, dry_run })
}

fn publish(root: &Path, destination: &Path, candidate: bool, dry_run: bool) -> Result<String, String> {
    fs::create_dir_all(destination).map_err(|_| refuse(&format!("{} unwritable", destination.display())))?;
    let census = if dry_run { Some(render_manifest_census(root)?) } else { None };
    let opts = PublishOptions { root, destination, candidate, dry_run };
    match prepare(&opts) {
        Ok(plan) => {
            if dry_run {
                Ok(format!("{}{}", census.unwrap_or_default(), plan.preview))
            } else {
                plan.commit()
            }
        }
        Err(err) => {
            if let Some(census) = census {
                Err(format!("{census}{err}"))
            } else {
                Err(err)
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match parse_cli(&args) {
        Err(code) => code,
        Ok(cli) => match project_root() {
            Err(err) => {
                println!("{err}");
                1
            }
            Ok(root) => match publish(&root, &cli.destination, cli.candidate, cli.dry_run) {
                Ok(text) => {
                    println!("{text}");
                    0
                }
                Err(err) => {
                    println!("{err}");
                    1
                }
            },
        },
    };
    std::process::exit(code);
}

#[cfg(test)]
mod checks {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = env::temp_dir().join(format!("rllm-package-{}-{}", std::process::id(), TEMP.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn runner_names_match_packaging_pairs() {
        let root = project_root().unwrap();
        let source = fs::read_to_string(root.join("tools/checks.py")).unwrap();
        let names = parse_names_list(&source).unwrap();
        assert_eq!(names.len(), 70, "{}", names.len());
        assert_eq!(names.first().map(String::as_str), Some("provider_process"));
        assert_eq!(names.last().map(String::as_str), Some("self_rebuild"));
        duplicate_groups(&names).unwrap();
        pairs_cover(&names).unwrap();
        assert_eq!(report_pairs().len(), 70);
    }

    #[test]
    fn matching_inventory_passes() {
        let parsed = parse_names_list("'''doc names = not this'''\nnames=['b', 'a']\n# names=['z']\n").unwrap();
        assert_eq!(parsed, vec!["b".to_string(), "a".to_string()]);
        inventory_order(&parsed, &parsed).unwrap();
        let escaped = parse_names_list("names=['a\\'b']\n").unwrap();
        assert_eq!(escaped, vec!["a'b".to_string()]);
    }

    #[test]
    fn reordered_inventory_refuses() {
        let expected = vec!["a".into(), "b".into()];
        let runs = vec!["b".into(), "a".into()];
        let err = inventory_order(&runs, &expected).unwrap_err();
        assert!(err.contains("Regression inventory differs from the current runner"), "{err}");
    }

    #[test]
    fn duplicate_group_refuses() {
        let expected = vec!["binary_flow".into(), "binary_flow".into()];
        inventory_order(&expected, &expected).unwrap();
        let err = duplicate_groups(&expected).unwrap_err();
        assert!(err.contains("Duplicate regression group"), "{err}");
    }

    #[test]
    fn pairs_cross_check_refuses_unknown_group() {
        let mut names = report_pairs().into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        names.pop();
        names.push("not_a_group".into());
        let err = pairs_cover(&names).unwrap_err();
        assert!(err.contains("Packaging report inventory differs from current checks"), "{err}");
        pairs_cover(&report_pairs().into_iter().map(|(name, _)| name).collect::<Vec<_>>()).unwrap();
    }

    #[test]
    fn executed_count_list_int_and_count_arms() {
        let list = obj(vec![("passed", Json::Arr(vec![j_str("a"), j_str("b")]))]);
        assert_eq!(executed_count(&list).unwrap(), 2);
        let integer = obj(vec![("passed", j_int(5))]);
        assert_eq!(executed_count(&integer).unwrap(), 5);
        let count = obj(vec![("count", j_int(4))]);
        assert_eq!(executed_count(&count).unwrap(), 4);
        let both = obj(vec![("passed", j_int(4)), ("count", j_int(4))]);
        assert_eq!(executed_count(&both).unwrap(), 4);
    }

    #[test]
    fn executed_count_checks_cross_check_refuses() {
        let bad = obj(vec![("passed", j_int(3)), ("checks", Json::Arr(vec![j_str("only")]))]);
        let err = executed_count(&bad).unwrap_err();
        assert!(err.contains("checks cross-check"), "{err}");
        let count_mismatch = obj(vec![("passed", j_int(3)), ("count", j_int(4))]);
        assert!(executed_count(&count_mismatch).is_err());
        let good = obj(vec![("passed", j_int(1)), ("checks", Json::Arr(vec![j_str("one")]))]);
        assert_eq!(executed_count(&good).unwrap(), 1);
    }

    #[test]
    fn executed_count_rejects_bool_and_zero() {
        let boolean = obj(vec![("passed", Json::Bool(true))]);
        assert!(executed_count(&boolean).unwrap_err().contains("not an integer"));
        let zero = obj(vec![("passed", j_int(0))]);
        assert!(executed_count(&zero).unwrap_err().contains("positive"));
        let empty = obj(vec![("passed", Json::Arr(vec![]))]);
        assert!(executed_count(&empty).is_err());
    }

    #[test]
    fn strict_groups_require_matching_checks_length() {
        let mut checks = Vec::new();
        let mut counts = Vec::new();
        for name in IDENTITY_PAIRS {
            checks.push(((*name).to_string(), obj(vec![("checks", Json::Arr(vec![j_str("one"), j_str("two")]))])));
            counts.push(((*name).to_string(), 2));
        }
        strict_lengths(&checks, &counts).unwrap();
        counts[0].1 = 1;
        let err = strict_lengths(&checks, &counts).unwrap_err();
        assert!(err.contains("services"), "{err}");
    }

    #[test]
    fn build_inputs_census_counts_match_drift_absent() {
        let dir = TempDir::new();
        fs::write(dir.0.join("keep.txt"), b"same").unwrap();
        fs::write(dir.0.join("moved.txt"), b"now").unwrap();
        let entries = vec![
            ("keep.txt".into(), hex_digest(&sha256(b"same"))),
            ("moved.txt".into(), hex_digest(&sha256(b"then"))),
            ("gone.txt".into(), hex_digest(&sha256(b"x"))),
        ];
        let census = digest_census(&dir.0, &entries).unwrap();
        assert_eq!((census.pinned, census.matched, census.drift, census.absent), (3, 1, 1, 1));
        let manifest = Json::Obj(vec![
            (
                "build_inputs".into(),
                Json::Obj(vec![
                    ("keep.txt".into(), j_str(&hex_digest(&sha256(b"same")))),
                    ("gone.txt".into(), j_str("ab")),
                ]),
            ),
            (
                "files".into(),
                Json::Obj(vec![("keep.txt".into(), obj(vec![("bytes", j_int(4)), ("sha256", j_str(&hex_digest(&sha256(b"same"))))]))]),
            ),
        ]);
        fs::write(dir.0.join("PUBLICATION-MANIFEST.json"), json_text(&manifest)).unwrap();
        let rendered = render_manifest_census(&dir.0).unwrap();
        assert!(rendered.contains("build_inputs: pinned 2 match 1 drift 0 absent 1"), "{rendered}");
        assert!(rendered.contains("files: pinned 1 match 1 drift 0 absent 0"), "{rendered}");
    }

    fn row(name: &str, code: i128, extra: bool) -> RunRow {
        let mut keys = BTreeSet::from(["name".into(), "code".into()]);
        if extra {
            keys.insert("extra".into());
        }
        RunRow { name: name.into(), code, keys, value: Json::Null }
    }

    #[test]
    fn candidate_accepts_only_one_binary_flow_failure() {
        candidate_gate(&[row("binary_flow", 1, false)], true).unwrap();
        let gap = obj(vec![
            ("unavailable", Json::Arr(vec![j_str("a"), j_str("b"), j_str("c")])),
            ("retained_compatibility", Json::Null),
        ]);
        binary_flow_gap(&gap).unwrap();
        let short = obj(vec![("unavailable", Json::Arr(vec![j_str("a")])), ("retained_compatibility", Json::Null)]);
        assert!(binary_flow_gap(&short).is_err());
        let retained = obj(vec![
            ("unavailable", Json::Arr(vec![j_str("a"), j_str("b"), j_str("c")])),
            ("retained_compatibility", Json::Bool(true)),
        ]);
        assert!(binary_flow_gap(&retained).is_err());
        assert!(candidate_gate(&[row("binary_flow", 1, true)], true).is_err());
        assert!(candidate_gate(&[row("binary_flow", 1, false)], false).is_err());
    }

    #[test]
    fn candidate_rejects_two_failures() {
        let err = candidate_gate(&[row("binary_flow", 1, false), row("history_migration", 1, false)], true).unwrap_err();
        assert!(err.contains("binary_flow") && err.contains("history_migration"), "{err}");
    }

    #[test]
    fn unit_suite_fork_and_floor() {
        let full = obj(vec![("passed", j_int(4)), ("cases", j_int(4)), ("failures", Json::Arr(vec![]))]);
        let suite = unit_suite(Some(&full), None).unwrap();
        assert_eq!(suite.count, 4);
        assert_eq!(suite.label, "Rust cases from the full suite");
        let broken = obj(vec![("passed", j_int(1)), ("cases", j_int(2)), ("failures", Json::Arr(vec![]))]);
        assert!(unit_suite(Some(&broken), None).is_err());
        let low = Json::Arr(vec![obj(vec![("code", j_int(0)), ("summary", j_str("3 passed; 0 failed"))])]);
        let err = unit_suite(None, Some(&low)).unwrap_err();
        assert!(err.contains("below 199"), "{err}");
        let mut runs = Vec::new();
        for _ in 0..199 {
            runs.push(obj(vec![("code", j_int(0)), ("summary", j_str("1 passed; 0 failed"))]));
        }
        let high = unit_suite(None, Some(&Json::Arr(runs))).unwrap();
        assert_eq!(high.count, 199);
        assert_eq!(high.label, "selected Rust cases");
    }

    #[test]
    fn suffix_symlink_and_apk_are_refused() {
        let dir = TempDir::new();
        fs::write(dir.0.join("notes.zip"), b"zip").unwrap();
        let err = screen_files(&dir.0, &collect_tree(&dir.0).unwrap()).unwrap_err();
        assert!(err.contains("notes.zip") && err.contains("suffix"), "{err}");
        let dir = TempDir::new();
        fs::write(dir.0.join("app.apk"), b"apk").unwrap();
        let err = screen_files(&dir.0, &collect_tree(&dir.0).unwrap()).unwrap_err();
        assert!(err.contains("The signed APK is distributed separately"), "{err}");
        let dir = TempDir::new();
        fs::write(dir.0.join("target.txt"), b"t").unwrap();
        std::os::unix::fs::symlink(dir.0.join("target.txt"), dir.0.join("link.txt")).unwrap();
        let err = screen_files(&dir.0, &collect_tree(&dir.0).unwrap()).unwrap_err();
        assert!(err.contains("symlink") && err.contains("link.txt"), "{err}");
    }

    #[test]
    fn zip_roundtrip_verifies_crc_and_length() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
        assert_eq!(inflate(&deflate_store(b""), 0).unwrap(), b"");
        let big = vec![b'a'; 70_000];
        assert_eq!(inflate(&deflate_store(&big), big.len()).unwrap(), big);
        let stored = deflate_store(b"hello");
        assert_eq!(stored.len(), b"hello".len() + 5);
        let archive = archive_bytes(&[("RLLM/a.txt".into(), b"hello".to_vec()), ("RLLM/empty.txt".into(), Vec::new())]).unwrap();
        let members = read_archive(&archive).unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].1, b"hello");
        assert_eq!(members[1].1.len(), 0);
        let mut corrupt = archive.clone();
        let at = corrupt.windows(5).position(|window| window == b"hello").unwrap();
        corrupt[at] ^= 0xff;
        let err = read_archive(&corrupt).unwrap_err();
        assert!(err.contains("CRC-32") || err.contains("mismatch") || err.contains("length"), "{err}");
    }

    fn write_exe(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn digest_file(path: &Path) -> String {
        hex_digest(&sha256(&fs::read(path).unwrap()))
    }

    fn snapshot(root: &Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                let meta = fs::symlink_metadata(&path).unwrap();
                let relative = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                if meta.is_dir() && !meta.file_type().is_symlink() {
                    walk(&path, root, out);
                } else if meta.is_file() || path.is_file() {
                    out.insert(relative, digest_file(&path));
                }
            }
        }
        walk(root, root, &mut out);
        out
    }

    fn fixture() -> (TempDir, PathBuf) {
        let dir = TempDir::new();
        let root = dir.0.clone();
        fs::write(root.join("AndroidManifest.xml"), b"manifest").unwrap();
        fs::write(root.join("package.sh"), b"pkg").unwrap();
        fs::create_dir_all(root.join("tools")).unwrap();
        fs::write(root.join("tools/alignment.rs"), b"align").unwrap();
        let names = report_pairs().into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        let quoted = names.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(",");
        fs::write(root.join("tools/checks.py"), format!("names=[{quoted}]\n")).unwrap();
        fs::write(root.join("README.md"), "# RLLM\n\nOld paragraph.\n\nRLLM connects your Android device to models.\n").unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/STATUS.md"), b"old status\n").unwrap();
        fs::create_dir_all(root.join("verification")).unwrap();
        fs::write(root.join("verification/RESULTS.json"), "{\"selected_rust_filters\":[\"network::one\"]}\n").unwrap();
        fs::write(root.join("verification/UPGRADE-AUDIT.json"), "{\"full_rust_suite\":{\"source\":\"historical\"}}\n").unwrap();
        fs::create_dir_all(root.join("output/checks")).unwrap();
        fs::create_dir_all(root.join("output/tools")).unwrap();
        write_exe(&root.join("output/tools/signature"), "#!/bin/sh\nexit 0\n");
        fs::write(root.join("output/published-controller"), b"controller").unwrap();
        let manifest_path = root.join("AndroidManifest.xml");
        let published = obj(vec![
            ("source_path", j_str(&manifest_path.display().to_string())),
            ("source_sha256", j_str(&digest_file(&manifest_path))),
            ("controller_sha256", j_str(&hex_digest(&sha256(b"controller")))),
        ]);
        fs::write(root.join("output/checks/published-comparator.json"), json_text(&published)).unwrap();
        let stamp = root.join("output/build/2026-01-02__03-04-05Z");
        fs::create_dir_all(&stamp).unwrap();
        let apk = stamp.join("app.apk");
        fs::write(&apk, b"apk-bytes").unwrap();
        let inputs = Json::Obj(vec![
            ("AndroidManifest.xml".into(), j_str(&hex_digest(&sha256(b"manifest")))),
            ("package.sh".into(), j_str(&hex_digest(&sha256(b"pkg")))),
            ("tools/alignment.rs".into(), j_str(&hex_digest(&sha256(b"align")))),
        ]);
        let apk_json = Json::Obj(vec![
            ("path".into(), j_str(&apk.display().to_string())),
            ("bytes".into(), j_int(9)),
            ("sha256".into(), j_str(&hex_digest(&sha256(b"apk-bytes")))),
            ("version".into(), j_int(7)),
            ("build_inputs".into(), inputs.clone()),
        ]);
        fs::write(root.join("output/checks/apk.json"), json_text(&apk_json)).unwrap();
        fs::write(root.join("output/checks/full-units.json"), "{\"passed\":4,\"cases\":4,\"failures\":[]}\n").unwrap();
        let regression = Json::Arr(names.iter().map(|name| obj(vec![("name", j_str(name)), ("code", j_int(0))])).collect());
        fs::write(root.join("output/checks/current-regression.json"), json_text(&regression)).unwrap();
        for (name, file) in report_pairs() {
            let body = if IDENTITY_PAIRS.contains(&name.as_str()) {
                "{\"passed\":1,\"checks\":[\"one\"]}\n"
            } else {
                "{\"passed\":1}\n"
            };
            fs::write(root.join(format!("output/checks/{file}.json")), body).unwrap();
        }
        let manifest = Json::Obj(vec![
            ("state".into(), j_str("old")),
            (
                "build_inputs".into(),
                Json::Obj(vec![
                    ("AndroidManifest.xml".into(), j_str(&hex_digest(&sha256(b"manifest")))),
                    ("package.sh".into(), j_str("ffff")),
                    ("missing.rs".into(), j_str("ab")),
                ]),
            ),
            (
                "files".into(),
                Json::Obj(vec![(
                    "AndroidManifest.xml".into(),
                    obj(vec![("bytes", j_int(8)), ("sha256", j_str(&hex_digest(&sha256(b"manifest"))))]),
                )]),
            ),
        ]);
        fs::write(root.join("PUBLICATION-MANIFEST.json"), json_text(&manifest)).unwrap();
        (dir, root)
    }

    #[test]
    fn dry_run_writes_nothing() {
        let (dir, root) = fixture();
        let before = snapshot(&root);
        let destination = root.join("destination");
        let report = publish(&root, &destination, false, true).unwrap();
        assert!(destination.is_dir(), "destination was not created");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0, "dry-run wrote inside the destination");
        assert!(!root.join("release").exists(), "dry-run created release/");
        assert_eq!(snapshot(&root), before, "dry-run changed a project file");
        assert!(report.contains("build_inputs: pinned 3 match 1 drift 1 absent 1"), "{report}");
        assert!(report.contains("would write:"), "{report}");
        assert!(report.contains("verification/RESULTS.json bytes "), "{report}");
        assert!(report.contains("PUBLICATION-MANIFEST.json bytes "), "{report}");
        assert!(report.contains("bytes "), "{report}");
        assert!(report.contains("Current signed candidate") == false);
        let _ = dir;
    }

    #[test]
    fn write_path_emits_the_publication_set() {
        let (_dir, root) = fixture();
        let destination = root.join("destination");
        let printed = publish(&root, &destination, false, false).unwrap();
        assert!(root.join("verification/RESULTS.json").is_file());
        assert!(root.join("release/NOTES.md").is_file());
        assert!(root.join("docs/STATUS.md").is_file());
        assert!(root.join("output/checks/packaged.json").is_file());
        let notes = fs::read_to_string(root.join("release/NOTES.md")).unwrap();
        assert!(notes.contains("Auto chooses a direct reply"), "{notes}");
        assert!(notes.contains("70/70 connected groups passed"), "{notes}");
        assert!(notes.contains("4 Rust cases from the full suite"), "{notes}");
        let readme = fs::read_to_string(root.join("README.md")).unwrap();
        assert!(readme.contains("Current signed candidate 7"), "{readme}");
        let names = fs::read_dir(&destination).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names.len(), 3, "{names:?}");
        let zip_name = names.iter().find(|name| name.ends_with(".zip")).unwrap();
        let opened = read_archive(&fs::read(destination.join(zip_name)).unwrap()).unwrap();
        assert!(opened.iter().any(|(name, data)| name == "RLLM/AndroidManifest.xml" && data == b"manifest"));
        assert!(printed.contains("signed candidate; acceptance incomplete"), "{printed}");
    }
}

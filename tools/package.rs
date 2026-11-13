// Native port of tools/package.py. tools/package.py remains until a signed
// candidate is published with this binary. Built by tools/build-tools.sh.
// --dry-run repeats the reads and checks, prints the pinned censuses, the
// Python area census, and the path and byte count of each artifact, and writes
// nothing except the destination directory. The Python census also runs on a
// real publish and refuses before any artifact is written.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
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

const WINDOW: usize = 32768;
const CHUNK: usize = 65536;
const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const MAX_MATCH: usize = 258;
const CHAIN_LIMIT: usize = 4096;

const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

#[derive(Clone, Copy)]
enum Tok { Lit(u8), Match { len: u16, dist: u16 } }

struct BitOut { bytes: Vec<u8>, acc: u64, n: u32 }
impl BitOut {
    fn new() -> Self { Self { bytes: Vec::new(), acc: 0, n: 0 } }
    fn put(&mut self, value: u32, count: u32) {
        if count == 0 { return; }
        let value = value & ((1u32 << count) - 1);
        self.acc |= (value as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.bytes.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    fn bit_len(&self) -> usize { self.bytes.len() * 8 + self.n as usize }
    fn append(&mut self, other: &BitOut) {
        for &byte in &other.bytes { self.put(byte as u32, 8); }
        if other.n > 0 { self.put(other.acc as u32, other.n); }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 { self.bytes.push(self.acc as u8); }
        self.bytes
    }
}

fn bit_reverse(code: u16, len: u8) -> u16 {
    let mut reversed = 0u16;
    let mut bits = code;
    for _ in 0..len {
        reversed = (reversed << 1) | (bits & 1);
        bits >>= 1;
    }
    reversed
}

fn put_huff(out: &mut BitOut, code: u16, len: u8) {
    out.put(bit_reverse(code, len) as u32, len as u32);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape { Complete, Single, Empty, Bad }

fn code_shape(lengths: &[u8]) -> Shape {
    let mut count = [0i32; 16];
    for &len in lengths {
        if len > 15 { return Shape::Bad; }
        if len > 0 { count[len as usize] += 1; }
    }
    let size: i32 = count.iter().sum();
    if size == 0 { return Shape::Empty; }
    if size == 1 && count[1] == 1 { return Shape::Single; }
    let mut left = 1i32;
    for bits in 1..16 {
        left = left * 2 - count[bits as usize];
        if left < 0 { return Shape::Bad; }
    }
    if left == 0 { Shape::Complete } else { Shape::Bad }
}

fn canonical(lengths: &[u8]) -> Vec<(u16, u8)> {
    let mut count = [0u32; 16];
    for &len in lengths {
        if len > 0 { count[len as usize] += 1; }
    }
    let mut next = [0u16; 16];
    let mut code = 0u32;
    for bits in 1..16 {
        code = (code + count[bits - 1]) << 1;
        next[bits] = code as u16;
    }
    let mut codes = vec![(0u16, 0u8); lengths.len()];
    for (symbol, &len) in lengths.iter().enumerate() {
        if len != 0 {
            codes[symbol] = (next[len as usize], len);
            next[len as usize] = next[len as usize].wrapping_add(1);
        }
    }
    codes
}

fn assign_balanced(lengths: &mut [u8], symbols: &[usize], max_bits: usize) -> bool {
    let n = symbols.len();
    if n == 0 { return true; }
    let mut bits = 1usize;
    while bits < 16 && (1usize << bits) < n { bits += 1; }
    if (1usize << bits) < n || bits > max_bits { return false; }
    if (1usize << bits) == n {
        for &symbol in symbols { lengths[symbol] = bits as u8; }
        return true;
    }
    let half = 1usize << (bits - 1);
    let k = n - half;
    let short = half - k;
    let mut ordered = symbols.to_vec();
    ordered.sort_unstable();
    for (index, &symbol) in ordered.iter().enumerate() {
        lengths[symbol] = if index < short { (bits - 1) as u8 } else { bits as u8 };
    }
    true
}

fn huffman_lengths(freq: &[u64], max_bits: usize) -> Vec<u8> {
    let mut lengths = vec![0u8; freq.len()];
    let symbols: Vec<usize> = freq.iter().enumerate().filter(|(_, weight)| **weight > 0).map(|(index, _)| index).collect();
    let count = symbols.len();
    if count == 0 { return lengths; }
    if count == 1 {
        lengths[symbols[0]] = 1;
        return lengths;
    }
    struct Node { left: i32, right: i32, symbol: i32 }
    let mut nodes: Vec<Node> = symbols.iter().map(|&symbol| Node { left: -1, right: -1, symbol: symbol as i32 }).collect();
    let mut weight: Vec<u64> = symbols.iter().map(|&symbol| freq[symbol]).collect();
    let mut heap: Vec<usize> = (0..count).collect();
    while heap.len() > 1 {
        heap.sort_by(|&left, &right| weight[left].cmp(&weight[right]).then(left.cmp(&right)));
        let left = heap.remove(0);
        let right = heap.remove(0);
        let node = nodes.len();
        nodes.push(Node { left: left as i32, right: right as i32, symbol: -1 });
        weight.push(weight[left] + weight[right]);
        heap.push(node);
    }
    let mut stack = vec![(heap[0], 0u32)];
    let mut max_depth = 0u32;
    while let Some((node, depth)) = stack.pop() {
        let item = &nodes[node];
        if item.symbol >= 0 {
            let bits = depth.max(1);
            lengths[item.symbol as usize] = bits as u8;
            max_depth = max_depth.max(bits);
        } else {
            stack.push((item.left as usize, depth + 1));
            stack.push((item.right as usize, depth + 1));
        }
    }
    if max_depth > max_bits as u32 {
        lengths.fill(0);
        if !assign_balanced(&mut lengths, &symbols, max_bits) { return lengths; }
    }
    lengths
}

fn length_symbol(len: usize) -> (usize, u8, usize) {
    let mut index = 0;
    while index + 1 < LENGTH_BASE.len() && LENGTH_BASE[index + 1] <= len { index += 1; }
    (257 + index, LENGTH_EXTRA[index], len - LENGTH_BASE[index])
}

fn distance_base(symbol: usize) -> (usize, u8) {
    let extra = if symbol < 4 { 0 } else { symbol / 2 - 1 };
    let base = if symbol < 4 { symbol + 1 } else { (2 + symbol % 2) * (1usize << extra) + 1 };
    (base, extra as u8)
}

fn distance_symbol(dist: usize) -> (usize, u8, usize) {
    let mut symbol = 29;
    while symbol > 0 && distance_base(symbol).0 > dist { symbol -= 1; }
    let (base, extra) = distance_base(symbol);
    (symbol, extra, dist - base)
}

struct Cl { symbol: u8, extra: u32, bits: u32 }

fn rle(lengths: &[u8]) -> Vec<Cl> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < lengths.len() {
        let value = lengths[index];
        let mut end = index + 1;
        while end < lengths.len() && lengths[end] == value { end += 1; }
        let mut count = end - index;
        if value == 0 {
            while count >= 11 {
                let take = count.min(138);
                out.push(Cl { symbol: 18, extra: (take - 11) as u32, bits: 7 });
                count -= take;
            }
            while count >= 3 {
                let take = count.min(10);
                out.push(Cl { symbol: 17, extra: (take - 3) as u32, bits: 3 });
                count -= take;
            }
            while count > 0 {
                out.push(Cl { symbol: 0, extra: 0, bits: 0 });
                count -= 1;
            }
        } else {
            out.push(Cl { symbol: value, extra: 0, bits: 0 });
            count -= 1;
            while count >= 3 {
                let take = count.min(6);
                out.push(Cl { symbol: 16, extra: (take - 3) as u32, bits: 2 });
                count -= take;
            }
            while count > 0 {
                out.push(Cl { symbol: value, extra: 0, bits: 0 });
                count -= 1;
            }
        }
        index = end;
    }
    out
}

fn rle_covers(runs: &[Cl], expected: usize) -> bool {
    let mut n = 0usize;
    for run in runs {
        n += match run.symbol {
            16 => 3 + run.extra as usize,
            17 => 3 + run.extra as usize,
            18 => 11 + run.extra as usize,
            _ => 1,
        };
    }
    n == expected
}

fn fixed_literal_codes() -> Vec<(u16, u8)> {
    let mut lengths = vec![8u8; 288];
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..288].fill(8);
    canonical(&lengths)
}

fn fixed_distance_codes() -> Vec<(u16, u8)> {
    canonical(&[5u8; 30])
}

fn write_tokens(out: &mut BitOut, tokens: &[Tok], lit: &[(u16, u8)], dist: &[(u16, u8)]) {
    for token in tokens {
        match *token {
            Tok::Lit(byte) => put_huff(out, lit[byte as usize].0, lit[byte as usize].1),
            Tok::Match { len, dist: distance } => {
                let (symbol, extra, value) = length_symbol(len as usize);
                put_huff(out, lit[symbol].0, lit[symbol].1);
                if extra > 0 { out.put(value as u32, extra as u32); }
                let (symbol, extra, value) = distance_symbol(distance as usize);
                put_huff(out, dist[symbol].0, dist[symbol].1);
                if extra > 0 { out.put(value as u32, extra as u32); }
            }
        }
    }
}

fn write_fixed(out: &mut BitOut, tokens: &[Tok], is_final: bool) {
    out.put(u32::from(is_final), 1);
    out.put(1, 2);
    let lit = fixed_literal_codes();
    let dist = fixed_distance_codes();
    write_tokens(out, tokens, &lit, &dist);
    put_huff(out, lit[256].0, lit[256].1);
}

fn write_dynamic(out: &mut BitOut, tokens: &[Tok], is_final: bool) -> bool {
    let mut lit_freq = [0u64; 286];
    let mut dist_freq = [0u64; 30];
    lit_freq[256] = 1;
    for token in tokens {
        match *token {
            Tok::Lit(byte) => lit_freq[byte as usize] += 1,
            Tok::Match { len, dist } => {
                lit_freq[length_symbol(len as usize).0] += 1;
                dist_freq[distance_symbol(dist as usize).0] += 1;
            }
        }
    }
    let lit_lens = huffman_lengths(&lit_freq, 15);
    let dist_lens = huffman_lengths(&dist_freq, 15);
    if lit_lens[256] == 0 { return false; }
    for token in tokens {
        match *token {
            Tok::Lit(byte) => { if lit_lens[byte as usize] == 0 { return false; } }
            Tok::Match { len, dist } => {
                if lit_lens[length_symbol(len as usize).0] == 0 || dist_lens[distance_symbol(dist as usize).0] == 0 {
                    return false;
                }
            }
        }
    }
    let mut lit_count = 257;
    for index in (0..286).rev() {
        if lit_lens[index] != 0 { lit_count = index + 1; break; }
    }
    if lit_count < 257 { lit_count = 257; }
    let mut dist_count = 1;
    for index in (0..30).rev() {
        if dist_lens[index] != 0 { dist_count = index + 1; break; }
    }
    let lit_shape = code_shape(&lit_lens[..lit_count]);
    let dist_shape = code_shape(&dist_lens[..dist_count]);
    if !matches!(lit_shape, Shape::Complete | Shape::Single) { return false; }
    if !matches!(dist_shape, Shape::Complete | Shape::Single | Shape::Empty) { return false; }
    let mut all = Vec::with_capacity(lit_count + dist_count);
    all.extend_from_slice(&lit_lens[..lit_count]);
    all.extend_from_slice(&dist_lens[..dist_count]);
    let runs = rle(&all);
    if !rle_covers(&runs, all.len()) { return false; }
    let mut cl_freq = [0u64; 19];
    for run in &runs { cl_freq[run.symbol as usize] += 1; }
    let mut cl_lens = huffman_lengths(&cl_freq, 7);
    if code_shape(&cl_lens) == Shape::Single {
        let existing = cl_lens.iter().position(|&len| len > 0).unwrap();
        let dummy = if existing == 0 { 1 } else { 0 };
        cl_lens[existing] = 1;
        cl_lens[dummy] = 1;
    }
    if code_shape(&cl_lens) != Shape::Complete { return false; }
    for run in &runs {
        if cl_lens[run.symbol as usize] == 0 { return false; }
    }
    let mut cl_send = 4;
    for index in (0..19).rev() {
        if cl_lens[CL_ORDER[index]] != 0 {
            cl_send = (index + 1).max(4);
            break;
        }
    }
    out.put(u32::from(is_final), 1);
    out.put(2, 2);
    out.put((lit_count - 257) as u32, 5);
    out.put((dist_count - 1) as u32, 5);
    out.put((cl_send - 4) as u32, 4);
    for index in 0..cl_send { out.put(cl_lens[CL_ORDER[index]] as u32, 3); }
    let cl_codes = canonical(&cl_lens);
    for run in &runs {
        let (code, len) = cl_codes[run.symbol as usize];
        put_huff(out, code, len);
        if run.bits > 0 { out.put(run.extra, run.bits); }
    }
    let lit_codes = canonical(&lit_lens);
    let dist_codes = canonical(&dist_lens);
    write_tokens(out, tokens, &lit_codes, &dist_codes);
    put_huff(out, lit_codes[256].0, lit_codes[256].1);
    true
}

fn hash3(data: &[u8], pos: usize) -> usize {
    let mixed = (data[pos] as u32).wrapping_mul(0x0100_0193)
        ^ (data[pos + 1] as u32).wrapping_mul(0x0100_0193)
        ^ (data[pos + 2] as u32);
    (mixed.wrapping_mul(0x1e35_a7bd) >> (32 - HASH_BITS)) as usize
}

fn insert(data: &[u8], pos: usize, head: &mut [u32], prev: &mut [u32]) {
    if pos + 2 >= data.len() { return; }
    let hash = hash3(data, pos);
    prev[pos % WINDOW] = head[hash];
    head[hash] = (pos as u32) + 1;
}

fn find_match(data: &[u8], pos: usize, head: &[u32], prev: &[u32]) -> Option<(usize, usize)> {
    if pos == 0 || pos + 3 > data.len() { return None; }
    let max_len = (data.len() - pos).min(MAX_MATCH);
    let oldest = pos.saturating_sub(WINDOW);
    let mut chain = head[hash3(data, pos)];
    let mut best_len = 2usize;
    let mut best_dist = 0usize;
    let mut steps = 0usize;
    let mut limit = CHAIN_LIMIT;
    while chain != 0 && steps < limit {
        let cand = (chain - 1) as usize;
        if cand < oldest || cand >= pos { break; }
        steps += 1;
        let dist = pos - cand;
        if dist <= WINDOW && data[cand] == data[pos] && data[cand + 1] == data[pos + 1] && data[cand + 2] == data[pos + 2] {
            let mut len = 3usize;
            while len < max_len && data[cand + len] == data[pos + len] { len += 1; }
            if len > best_len {
                best_len = len;
                best_dist = dist;
                if best_len >= MAX_MATCH { break; }
                if best_len >= 32 { limit = steps + 128; }
            }
        }
        let next = prev[cand % WINDOW];
        if next == chain { break; }
        chain = next;
    }
    if best_len >= 3 { Some((best_len, best_dist)) } else { None }
}

fn lz77(data: &[u8]) -> Vec<Tok> {
    let mut head = vec![0u32; HASH_SIZE];
    let mut prev = vec![0u32; WINDOW];
    let mut tokens = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let found = find_match(data, pos, &head, &prev);
        insert(data, pos, &mut head, &mut prev);
        if let Some((len, dist)) = found {
            let mut take = true;
            if len < MAX_MATCH && pos + 1 < data.len() {
                if let Some((next_len, _)) = find_match(data, pos + 1, &head, &prev) {
                    if next_len > len {
                        tokens.push(Tok::Lit(data[pos]));
                        pos += 1;
                        take = false;
                    }
                }
            }
            if take {
                tokens.push(Tok::Match { len: len as u16, dist: dist as u16 });
                let end = pos + len;
                pos += 1;
                while pos < end {
                    insert(data, pos, &mut head, &mut prev);
                    pos += 1;
                }
            }
        } else {
            tokens.push(Tok::Lit(data[pos]));
            pos += 1;
        }
    }
    tokens
}

fn stored_blocks(data: &[u8]) -> Vec<u8> {
    if data.is_empty() { return vec![0x01, 0x00, 0x00, 0xff, 0xff]; }
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

fn compress_huffman(data: &[u8]) -> Vec<u8> {
    let mut out = BitOut::new();
    if data.is_empty() {
        write_fixed(&mut out, &[], true);
        return out.finish();
    }
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + CHUNK).min(data.len());
        let tokens = lz77(&data[offset..end]);
        let is_final = end == data.len();
        let mut fixed = BitOut::new();
        write_fixed(&mut fixed, &tokens, is_final);
        let mut dynamic = BitOut::new();
        let dynamic_ok = write_dynamic(&mut dynamic, &tokens, is_final);
        if dynamic_ok && dynamic.bit_len() < fixed.bit_len() { out.append(&dynamic); }
        else { out.append(&fixed); }
        offset = end;
    }
    out.finish()
}

// Level-9 analogue, not zlib's bytes. Search uses a 32 KiB window, 3-byte
// hash chains, lazy matches, and a nice length of 258. The chain walk stops
// at 4096 steps, or 128 steps after a match of length 32. Each block covers
// at most 64 KiB of input and uses the smaller of fixed and dynamic Huffman.
// If that encoding is not strictly smaller than stored blocks, the member is
// stored. Match picks, block splits, and code-length runs differ from zlib,
// so the bytes are not compresslevel=9.
fn deflate(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() >= u32::MAX as usize { return stored_blocks(bytes); }
    let stored = stored_blocks(bytes);
    let compressed = compress_huffman(bytes);
    if compressed.len() < stored.len() { compressed } else { stored }
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

// glibc struct tm. localtime_r fills it; only the calendar fields are read.
#[repr(C)]
struct LocalTm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: i64,
    tm_zone: *const i8,
}

extern "C" {
    fn localtime_r(timep: *const i64, result: *mut LocalTm) -> *mut LocalTm;
}

struct MemberStamp {
    dos_time: u16,
    dos_date: u16,
    external_attr: u32,
}

fn zip_flags(name: &str) -> u16 {
    if name.is_ascii() { 0 } else { 0x0800 }
}

fn dos_from_parts(year: i32, month: i32, day: i32, hour: i32, minute: i32, second: i32) -> Result<(u16, u16), String> {
    if !(1980..=2107).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(refuse(&format!("ZIP does not support timestamp {year:04}-{month:02}-{day:02}")));
    }
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=61).contains(&second) {
        return Err(refuse(&format!("ZIP does not support timestamp {hour:02}:{minute:02}:{second:02}")));
    }
    let date = ((year - 1980) << 9) | (month << 5) | day;
    let time = (hour << 11) | (minute << 5) | (second / 2);
    Ok((time as u16, date as u16))
}

fn dos_stamp(when: SystemTime) -> Result<(u16, u16), String> {
    let secs = match when.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).map_err(|_| refuse("ZIP does not support timestamp"))?,
        Err(_) => return Err(refuse("ZIP does not support timestamps before 1980")),
    };
    let mut tm = LocalTm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
    };
    // POSIX localtime_r, same conversion Python's time.localtime uses for ZipInfo.
    let ptr = unsafe { localtime_r(&secs, &mut tm) };
    if ptr.is_null() {
        return Err(refuse("ZIP does not support timestamp"));
    }
    dos_from_parts(tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn created_file_mode() -> u32 {
    static MODE: OnceLock<u32> = OnceLock::new();
    *MODE.get_or_init(|| {
        let path = env::temp_dir().join(format!("rllm-package-mode-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        let mode = fs::File::create(&path).and_then(|_| fs::metadata(&path)).map(|meta| meta.permissions().mode()).unwrap_or(0o100644);
        let _ = fs::remove_file(&path);
        mode
    })
}

fn member_stamp(root: &Path, relative: &str, rewritten_at: Option<SystemTime>) -> Result<MemberStamp, String> {
    let path = root.join(relative);
    let existing = fs::metadata(&path).ok();
    let mode = existing.as_ref().map(|meta| meta.permissions().mode()).unwrap_or_else(created_file_mode);
    let when = if let Some(now) = rewritten_at {
        now
    } else if let Some(meta) = existing {
        meta.modified().map_err(|_| refuse(&format!("{relative} mtime")))?
    } else {
        return Err(refuse(&format!("{relative} absent")));
    };
    let (dos_time, dos_date) = dos_stamp(when)?;
    Ok(MemberStamp { dos_time, dos_date, external_attr: (mode & 0xFFFF) << 16 })
}

#[cfg(test)]
fn archive_bytes(members: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let stamped = members
        .iter()
        .map(|(name, bytes)| (name.clone(), bytes.clone(), MemberStamp { dos_time: 0, dos_date: 0x0021, external_attr: 0 }))
        .collect::<Vec<_>>();
    write_zip(&stamped)
}

fn write_zip(members: &[(String, Vec<u8>, MemberStamp)]) -> Result<Vec<u8>, String> {
    if members.len() > u16::MAX as usize {
        return Err(refuse("source zip exceeds ZIP32 entry count"));
    }
    let mut body = Vec::new();
    let mut directory = Vec::new();
    for (name, plain, stamp) in members {
        if name.len() > u16::MAX as usize {
            return Err(refuse(&format!("{name} exceeds ZIP32 name length")));
        }
        let compressed = deflate(plain);
        let crc = crc32(plain);
        let plain_len = fit_u32(plain.len(), name)?;
        let comp_len = fit_u32(compressed.len(), name)?;
        let offset = fit_u32(body.len(), name)?;
        let name_bytes = name.as_bytes();
        let flags = zip_flags(name);
        push_u32(&mut body, 0x0403_4b50);
        push_u16(&mut body, 20);
        push_u16(&mut body, flags);
        push_u16(&mut body, 8);
        push_u16(&mut body, stamp.dos_time);
        push_u16(&mut body, stamp.dos_date);
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
        push_u16(&mut directory, flags);
        push_u16(&mut directory, 8);
        push_u16(&mut directory, stamp.dos_time);
        push_u16(&mut directory, stamp.dos_date);
        push_u32(&mut directory, crc);
        push_u32(&mut directory, comp_len);
        push_u32(&mut directory, plain_len);
        push_u16(&mut directory, name_bytes.len() as u16);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u16(&mut directory, 0);
        push_u32(&mut directory, stamp.external_attr);
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

fn refuse_groups_absent() -> String {
    refuse("tools/harness.rs absent; expected const GROUPS")
}

fn refuse_groups_missing() -> String {
    refuse("tools/harness.rs has no GROUPS list")
}

fn refuse_groups_malformed() -> String {
    refuse("tools/harness.rs GROUPS list is malformed")
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn at_ident_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0 || !is_ident_byte(bytes[index - 1])
}

fn eat_ident(bytes: &[u8], index: usize) -> Option<(&str, usize)> {
    if index >= bytes.len() || !is_ident_start(bytes[index]) {
        return None;
    }
    let mut end = index + 1;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    let ident = std::str::from_utf8(&bytes[index..end]).ok()?;
    Some((ident, end))
}

fn skip_block_comment(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut index = start + 2;
    let mut depth = 1;
    while index + 1 < bytes.len() {
        if bytes[index] == b'/' && bytes[index + 1] == b'*' {
            depth += 1;
            index += 2;
            continue;
        }
        if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return Ok(index);
            }
            continue;
        }
        index += 1;
    }
    Err(refuse_groups_malformed())
}

fn skip_gap(bytes: &[u8], mut index: usize, commas: bool) -> Result<usize, String> {
    loop {
        if index >= bytes.len() {
            return Ok(index);
        }
        match bytes[index] {
            b' ' | b'\t' | b'\n' | b'\r' => index += 1,
            b',' if commas => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_block_comment(bytes, index)?;
            }
            _ => return Ok(index),
        }
    }
}

fn eat_byte(bytes: &[u8], index: usize, want: u8) -> Result<usize, String> {
    let index = skip_gap(bytes, index, false)?;
    if bytes.get(index) != Some(&want) {
        return Err(refuse_groups_malformed());
    }
    Ok(index + 1)
}

fn skip_rust_string(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if index + 1 >= bytes.len() {
                return Err(refuse_groups_malformed());
            }
            index += 2;
            continue;
        }
        if bytes[index] == b'"' {
            return Ok(index + 1);
        }
        index += 1;
    }
    Err(refuse_groups_malformed())
}

fn take_rust_string(bytes: &[u8], start: usize) -> Result<(String, usize), String> {
    let mut index = start + 1;
    let mut out = String::new();
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            let next = *bytes.get(index + 1).ok_or_else(refuse_groups_malformed)?;
            match next {
                b'\\' => out.push('\\'),
                b'"' => out.push('"'),
                b'\'' => out.push('\''),
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'0' => out.push('\0'),
                _ => return Err(refuse_groups_malformed()),
            }
            index += 2;
            continue;
        }
        if byte == b'"' {
            return Ok((out, index + 1));
        }
        if byte == b'\n' {
            return Err(refuse_groups_malformed());
        }
        out.push(byte as char);
        index += 1;
    }
    Err(refuse_groups_malformed())
}

fn raw_string_hashes(bytes: &[u8], index: usize) -> Option<usize> {
    if !at_ident_boundary(bytes, index) || bytes.get(index) != Some(&b'r') {
        return None;
    }
    let mut look = index + 1;
    let mut hashes = 0usize;
    while bytes.get(look) == Some(&b'#') {
        hashes += 1;
        look += 1;
    }
    if bytes.get(look) == Some(&b'"') {
        Some(hashes)
    } else {
        None
    }
}

fn skip_raw_string(bytes: &[u8], index: usize, hashes: usize) -> Result<usize, String> {
    let mut cursor = index + 1 + hashes + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"' {
            let tail = cursor + 1;
            let closed = bytes.get(tail..tail + hashes).map(|slice| slice.iter().all(|byte| *byte == b'#')).unwrap_or(false);
            if closed {
                return Ok(tail + hashes);
            }
        }
        cursor += 1;
    }
    Err(refuse_groups_malformed())
}

fn parse_group_entries(bytes: &[u8], mut index: usize) -> Result<(Vec<String>, usize), String> {
    let mut names = Vec::new();
    loop {
        index = skip_gap(bytes, index, true)?;
        if index >= bytes.len() {
            return Err(refuse_groups_malformed());
        }
        if bytes[index] == b']' {
            return Ok((names, index + 1));
        }
        if bytes[index] != b'"' {
            return Err(refuse_groups_malformed());
        }
        let (name, next) = take_rust_string(bytes, index)?;
        names.push(name);
        index = next;
    }
}

fn groups_after_const(bytes: &[u8], index: usize) -> Result<Option<(Vec<String>, usize)>, String> {
    let index = skip_gap(bytes, index, false)?;
    let Some((ident, end)) = eat_ident(bytes, index) else {
        return Ok(None);
    };
    if ident != "GROUPS" {
        return Ok(None);
    }
    let mut index = eat_byte(bytes, end, b':')?;
    index = eat_byte(bytes, index, b'&')?;
    index = eat_byte(bytes, index, b'[')?;
    index = eat_byte(bytes, index, b'&')?;
    let index = skip_gap(bytes, index, false)?;
    let Some((ty, end)) = eat_ident(bytes, index) else {
        return Err(refuse_groups_malformed());
    };
    if ty != "str" {
        return Err(refuse_groups_malformed());
    }
    let mut index = eat_byte(bytes, end, b']')?;
    index = eat_byte(bytes, index, b'=')?;
    index = eat_byte(bytes, index, b'&')?;
    index = eat_byte(bytes, index, b'[')?;
    Ok(Some(parse_group_entries(bytes, index)?))
}

// Packaging reads the runner group census from const GROUPS in tools/harness.rs.
fn parse_groups_list(source: &str) -> Result<Vec<String>, String> {
    let bytes = source.as_bytes();
    let (mut index, mut found) = (0usize, Vec::<Vec<String>>::new());
    while index < bytes.len() {
        let gap = skip_gap(bytes, index, false)?;
        if gap != index {
            index = gap;
            continue;
        }
        if bytes[index] == b'"' {
            index = skip_rust_string(bytes, index)?;
            continue;
        }
        if let Some(hashes) = raw_string_hashes(bytes, index) {
            index = skip_raw_string(bytes, index, hashes)?;
            continue;
        }
        if at_ident_boundary(bytes, index) {
            if let Some((ident, end)) = eat_ident(bytes, index) {
                if ident == "const" {
                    if let Some((names, after)) = groups_after_const(bytes, end)? {
                        found.push(names); index = after; continue;
                    }
                }
                index = end;
                continue;
            }
        }
        index += 1;
    }
    finish_groups_list(found)
}

fn load_runner_groups(root: &Path) -> Result<Vec<String>, String> {
    let source = fs::read_to_string(root.join("tools/harness.rs")).map_err(|_| refuse_groups_absent())?;
    parse_groups_list(&source)
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
    "rrsi_loop",
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
        "Packaging report inventory differs from the runner GROUPS list (groups {}, pairs {}, missing {missing:?}, extra {extra:?})",
        expected.len(),
        report_pairs().len()
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
// The publisher walk also sees vendored toolchains and agent scratch. Those
// roots are not product source, so a Python file there is not an RLLM area.
const NON_PRODUCT_ROOTS: &[&str] = &[".playwright-mcp", ".qwen", "toolchain", "tooling"];

struct ToolPython {
    name: &'static str,
    role: &'static str,
}

const TOOL_PYTHON: &[ToolPython] = &[
    ToolPython {
        name: "checks.py",
        role: "full-suite unit gate (--all-units, native twin gate <units>), selected-unit runner (--units, native twin gate --units), and connected-group runner (no-flag sweep; native orchestration and record writing is harness sweep over the Python group scripts)",
    },
    ToolPython {
        name: "package.py",
        role: "publisher until a signed candidate is published with the native package binary",
    },
];

// Both ceilings are the measured verification/ totals after every Python leg
// that a native twin replaced was deleted (23 legs so far, 50 files and 4407
// lines left). A larger file count or line count refuses; a smaller count still
// passes. The ceilings move down with every retired leg, never up.
const VERIFICATION_PY_FILE_CEILING: usize = 50;
const VERIFICATION_PY_LINE_CEILING: usize = 4407;
// A ceiling refusal names at most this many verification files, largest line
// count first. The rest of the count stays in the ceiling sentence.
const VERIFICATION_PY_LARGEST_SHOWN: usize = 8;
// A planted Python file is refused before its body is allocated past this cap.
// The largest counted product Python file is under 48 KiB.
const PYTHON_CENSUS_BYTE_CAP: u64 = 1024 * 1024;

// Zero declarations keep the missing-list refusal. One declaration is that list.
// Two or more, nested modules included, are a duplicate refusal.
fn finish_groups_list(mut found: Vec<Vec<String>>) -> Result<Vec<String>, String> {
    if found.is_empty() {
        return Err(refuse_groups_missing());
    }
    if found.len() != 1 {
        return Err(refuse("tools/harness.rs has more than one const GROUPS declaration"));
    }
    Ok(found.remove(0))
}

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

struct PyBucket {
    files: usize,
    lines: usize,
    paths: Vec<String>,
}

struct PythonCensus {
    core: PyBucket,
    platform: PyBucket,
    tools: PyBucket,
    verification: PyBucket,
    elsewhere: PyBucket,
    verification_largest: Vec<(String, usize)>,
}

fn empty_bucket() -> PyBucket {
    PyBucket { files: 0, lines: 0, paths: Vec::new() }
}

fn non_product_root(relative: &str) -> bool {
    let top = relative.split('/').next().unwrap_or(relative);
    NON_PRODUCT_ROOTS.contains(&top)
}

fn python_line_count(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let mut lines = bytes.iter().filter(|byte| **byte == b'\n').count();
    if !bytes.ends_with(b"\n") {
        lines += 1;
    }
    lines
}

fn tool_python_allowed(relative: &str) -> bool {
    let Some(name) = relative.strip_prefix("tools/") else {
        return false;
    };
    TOOL_PYTHON.iter().any(|item| item.name == name)
}

fn python_byte_bound_refusal(relative: &str) -> String {
    refuse(&format!("{relative} exceeds the Python census byte bound {PYTHON_CENSUS_BYTE_CAP}"))
}

fn read_python_body(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    let path = root.join(relative);
    let meta = fs::metadata(&path).map_err(|_| refuse(&format!("{relative} unreadable")))?;
    if meta.is_file() && meta.len() > PYTHON_CENSUS_BYTE_CAP {
        return Err(python_byte_bound_refusal(relative));
    }
    let file = fs::File::open(&path).map_err(|_| refuse(&format!("{relative} unreadable")))?;
    let mut bytes = Vec::new();
    file.take(PYTHON_CENSUS_BYTE_CAP.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| refuse(&format!("{relative} unreadable")))?;
    if bytes.len() as u64 > PYTHON_CENSUS_BYTE_CAP {
        return Err(python_byte_bound_refusal(relative));
    }
    Ok(bytes)
}

fn note_largest(largest: &mut Vec<(String, usize)>, relative: &str, lines: usize) {
    let mut index = 0;
    while index < largest.len() {
        let (path, have) = &largest[index];
        let before = lines > *have || (lines == *have && cmp_rel(relative, path).is_lt());
        if before {
            break;
        }
        index += 1;
    }
    if index == largest.len() && largest.len() == VERIFICATION_PY_LARGEST_SHOWN {
        return;
    }
    largest.insert(index, (relative.to_string(), lines));
    largest.truncate(VERIFICATION_PY_LARGEST_SHOWN);
}

fn tally_python(root: &Path, files: &[String]) -> Result<PythonCensus, String> {
    let mut census = PythonCensus {
        core: empty_bucket(),
        platform: empty_bucket(),
        tools: empty_bucket(),
        verification: empty_bucket(),
        elsewhere: empty_bucket(),
        verification_largest: Vec::new(),
    };
    for relative in files {
        if non_product_root(relative) || extension_of(relative) != Some("py") {
            continue;
        }
        let bytes = read_python_body(root, relative)?;
        let lines = python_line_count(&bytes);
        let top = relative.split('/').next().unwrap_or(relative);
        let bucket = match top {
            "core" => &mut census.core,
            "platform" => &mut census.platform,
            "tools" => &mut census.tools,
            "verification" => &mut census.verification,
            _ => &mut census.elsewhere,
        };
        bucket.files += 1;
        bucket.lines += lines;
        if top == "verification" {
            note_largest(&mut census.verification_largest, relative, lines);
        }
        let allowed_tool = top == "tools" && tool_python_allowed(relative);
        if top != "verification" && !allowed_tool {
            bucket.paths.push(relative.clone());
        }
    }
    Ok(census)
}

fn render_python_census(census: &PythonCensus) -> String {
    let allowlist = TOOL_PYTHON.iter().map(|item| format!("{} ({})", item.name, item.role)).collect::<Vec<_>>().join("; ");
    format!(
        "python_census: core files {} lines {}; platform files {} lines {}; tools files {} lines {} [{}]; verification files {} lines {} ceiling files {} lines {}; elsewhere files {} lines {}\n",
        census.core.files,
        census.core.lines,
        census.platform.files,
        census.platform.lines,
        census.tools.files,
        census.tools.lines,
        allowlist,
        census.verification.files,
        census.verification.lines,
        VERIFICATION_PY_FILE_CEILING,
        VERIFICATION_PY_LINE_CEILING,
        census.elsewhere.files,
        census.elsewhere.lines,
    )
}

fn reject_python(census: &PythonCensus) -> Result<(), String> {
    let mut messages = Vec::new();
    for path in &census.core.paths {
        messages.push(refuse(&format!("product-code violation: core/ contains Python {path}")));
    }
    for path in &census.platform.paths {
        messages.push(refuse(&format!("product-code violation: platform/ contains Python {path}")));
    }
    for path in &census.tools.paths {
        messages.push(refuse(&format!("tools/ Python {path} is outside the allowlist")));
    }
    if census.verification.files > VERIFICATION_PY_FILE_CEILING {
        messages.push(refuse(&format!(
            "verification/ Python files {} exceed the recorded ceiling {}",
            census.verification.files, VERIFICATION_PY_FILE_CEILING
        )));
    }
    if census.verification.lines > VERIFICATION_PY_LINE_CEILING {
        messages.push(refuse(&format!(
            "verification/ Python lines {} exceed the recorded ceiling {}",
            census.verification.lines, VERIFICATION_PY_LINE_CEILING
        )));
    }
    if census.verification.files > VERIFICATION_PY_FILE_CEILING || census.verification.lines > VERIFICATION_PY_LINE_CEILING {
        let names = census.verification_largest.iter().map(|(path, lines)| format!("{path} lines {lines}")).collect::<Vec<_>>().join(", ");
        messages.push(refuse(&format!(
            "verification/ Python largest {} of {}: {names}",
            census.verification_largest.len(),
            census.verification.files
        )));
    }
    for path in &census.elsewhere.paths {
        messages.push(refuse(&format!("Python outside core/, platform/, tools/, and verification/: {path}")));
    }
    if messages.is_empty() {
        Ok(())
    } else {
        Err(messages.join("\n"))
    }
}

fn enforce_python_census(root: &Path, files: &[String]) -> Result<String, String> {
    let census = tally_python(root, files)?;
    let line = render_python_census(&census);
    if let Err(err) = reject_python(&census) {
        return Err(format!("{line}{err}"));
    }
    Ok(line)
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
    rewritten_at: SystemTime,
}

fn rewritten_output(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("RESULTS.json" | "NOTES.md" | "STATUS.md" | "project-progress.md" | "README.md" | "PUBLICATION-MANIFEST.json")
    )
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
            if rewritten_output(&artifact.path) {
                let file = fs::File::options().write(true).open(&artifact.path).map_err(|_| refuse(&format!("{} mtime", artifact.path.display())))?;
                file.set_modified(self.rewritten_at).map_err(|_| refuse(&format!("{} mtime", artifact.path.display())))?;
            }
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
    let expected = load_runner_groups(root)?;
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
    let python_line = enforce_python_census(root, &files)?;
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
    let rewritten_at = SystemTime::now();
    let mut zip_members = Vec::with_capacity(member_bytes.len());
    for (relative, bytes) in &member_bytes {
        let rewritten = relative == "PUBLICATION-MANIFEST.json" || overlay.contains_key(relative);
        let stamp = member_stamp(root, relative, rewritten.then_some(rewritten_at))?;
        zip_members.push((format!("RLLM/{relative}"), bytes.clone(), stamp));
    }
    let zip_bytes = write_zip(&zip_members)?;
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
    let mut preview = python_line;
    for artifact in before.iter().chain(std::iter::once(&apk_artifact)).chain(after.iter()) {
        push_preview(&mut preview, &artifact.path, &artifact.bytes);
    }
    preview.push_str(&format!("would run: {} {}\n", signature.display(), published_path.display()));
    Ok(Plan { before, apk: apk_artifact, after, signature, preview, printed, rewritten_at })
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
        let names = load_runner_groups(&root).unwrap();
        duplicate_groups(&names).unwrap();
        pairs_cover(&names).unwrap();
        assert_eq!(EXPLICIT_PAIRS.len(), 35);
        assert_eq!(IDENTITY_PAIRS.len(), 22);
        assert_eq!(HYPHEN_PAIRS.len(), 14);
        assert_eq!(names.len(), 71, "{}", names.len());
        assert_eq!(report_pairs().len(), 71);
        assert_eq!(names.len(), EXPLICIT_PAIRS.len() + IDENTITY_PAIRS.len() + HYPHEN_PAIRS.len());
        assert_eq!(names.first().map(String::as_str), Some("provider_process"));
        assert_eq!(names.last().map(String::as_str), Some("rrsi_loop"));
    }

    #[test]
    fn runner_groups_list_refuses_absent_missing_and_malformed() {
        let dir = TempDir::new();
        let absent = load_runner_groups(&dir.0).unwrap_err();
        assert_eq!(absent, "refusing: tools/harness.rs absent; expected const GROUPS");
        let commented = parse_groups_list("// const GROUPS: &[&str] = &[\"hidden\"];\nfn main() {}\n").unwrap_err();
        assert_eq!(commented, "refusing: tools/harness.rs has no GROUPS list");
        let missing = parse_groups_list("fn main() {}\nconst OTHER: &[&str] = &[\"a\"];\n").unwrap_err();
        assert_eq!(missing, "refusing: tools/harness.rs has no GROUPS list");
        let malformed = parse_groups_list("const GROUPS: &[&str] = &[\"ok\", not_a_string];\n").unwrap_err();
        assert_eq!(malformed, "refusing: tools/harness.rs GROUPS list is malformed");
        let open = parse_groups_list("const GROUPS: &[&str] = &[\"unterminated\n").unwrap_err();
        assert_eq!(open, "refusing: tools/harness.rs GROUPS list is malformed");
    }

    #[test]
    fn matching_inventory_passes() {
        let source = r##"
const LABEL: &str = "const GROUPS: &[&str] = &[\"no\"];";
// const GROUPS: &[&str] = &["z"];
/* const GROUPS: &[&str] = &["z"]; */
const OTHER: &[&str] = &["skip"];
const GROUPS: &[&str] = &[
    "b",
    "a\"b",
];
"##;
        let parsed = parse_groups_list(source).unwrap();
        assert_eq!(parsed, vec!["b".to_string(), "a\"b".to_string()]);
        inventory_order(&parsed, &parsed).unwrap();
        let escaped = parse_groups_list("const GROUPS: &[&str] = &[\"a\\\"b\"];\n").unwrap();
        assert_eq!(escaped, vec!["a\"b".to_string()]);
    }

    fn real_runner_group_names() -> Vec<String> {
        [
            "provider_process",
            "network_drive",
            "drive_ownership",
            "question_process",
            "history",
            "a2a",
            "acp",
            "oauth",
            "device_authorization",
            "binary_flow",
            "pe_declarations",
            "commands",
            "history_migration",
            "hosted_phases",
            "prompt_reference",
            "provider_evidence",
            "delegation_visibility",
            "local_feedback",
            "network_feedback",
            "proposal_feedback",
            "proposal_recovery",
            "review_ownership",
            "task_ownership",
            "dav_listing",
            "server_admission",
            "http_framing",
            "gateway_expectation",
            "device_shortcut",
            "request_fields",
            "response_fields",
            "chunk_fields",
            "storage_etags",
            "platform_resolution",
            "query_feedback",
            "carry_flow",
            "workspace_navigation",
            "mcp_catalog",
            "mcp_catalog_events",
            "mcp_images",
            "mcp_continuation",
            "mcp_history",
            "document_providers",
            "snapshot_identity",
            "storage_auth",
            "storage_oauth",
            "credential_rotation",
            "storage_owner",
            "voice_settings",
            "field_flow",
            "flags_flow",
            "product_flow",
            "diagnostics",
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
            "state_sealing",
            "desktop_runtime",
            "turn_background",
            "permission_modes",
            "self_rebuild",
            "rrsi_loop",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn real_groups_declaration() -> String {
        let quoted = real_runner_group_names().iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(", ");
        format!("const GROUPS: &[&str] = &[{quoted}];\n")
    }

    #[test]
    fn nested_decoy_groups_before_the_real_const_refuses() {
        let source = format!("mod legacy_probe {{\n    pub const GROUPS: &[&str] = &[\"wrong\"];\n}}\n\n{}", real_groups_declaration());
        let err = parse_groups_list(&source).unwrap_err();
        assert_eq!(err, "refusing: tools/harness.rs has more than one const GROUPS declaration");
    }

    #[test]
    fn two_groups_declarations_refuse_in_either_order() {
        let sentence = "refusing: tools/harness.rs has more than one const GROUPS declaration";
        let alpha = "const GROUPS: &[&str] = &[\"alpha\"];\n";
        let beta = "const GROUPS: &[&str] = &[\"beta\"];\n";
        assert_eq!(parse_groups_list(&format!("{alpha}{beta}")).unwrap_err(), sentence);
        assert_eq!(parse_groups_list(&format!("{beta}{alpha}")).unwrap_err(), sentence);
    }

    #[test]
    fn single_groups_declaration_returns_the_real_71_names() {
        let expected = real_runner_group_names();
        assert_eq!(expected.len(), 71);
        let parsed = parse_groups_list(&real_groups_declaration()).unwrap();
        assert_eq!(parsed, expected);
        let runner = load_runner_groups(&project_root().unwrap()).unwrap();
        assert_eq!(parsed, runner);
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
        assert!(err.contains("Packaging report inventory differs from the runner GROUPS list"), "{err}");
        assert!(err.contains("groups 71, pairs 71"), "{err}");
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
        assert_eq!(inflate(&stored_blocks(b""), 0).unwrap(), b"");
        let big = vec![b'a'; 70_000];
        assert_eq!(inflate(&stored_blocks(&big), big.len()).unwrap(), big);
        let stored = stored_blocks(b"hello");
        assert_eq!(stored.len(), b"hello".len() + 5);
        let archive = archive_bytes(&[("RLLM/a.txt".into(), b"hello".to_vec()), ("RLLM/empty.txt".into(), Vec::new())]).unwrap();
        let members = read_archive(&archive).unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].1, b"hello");
        assert_eq!(members[1].1.len(), 0);
        let mut corrupt = archive.clone();
        let central = corrupt.windows(4).position(|window| window == [0x50, 0x4b, 0x01, 0x02]).unwrap();
        corrupt[central + 16] ^= 0xff;
        let err = read_archive(&corrupt).unwrap_err();
        assert!(err.contains("CRC-32"), "{err}");
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
        let quoted = names.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(", ");
        fs::write(root.join("tools/harness.rs"), format!("const GROUPS: &[&str] = &[{quoted}];\n")).unwrap();
        let py_quoted = names.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(",");
        fs::write(root.join("tools/checks.py"), format!("names=[{py_quoted}]\n")).unwrap();
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
        assert!(notes.contains("71/71 connected groups passed"), "{notes}");
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

    #[test]
    fn dos_parts_pack_the_zip_clock() {
        assert_eq!(dos_from_parts(2026, 11, 12, 8, 47, 2).unwrap(), (0x45e1, 0x5d6c));
        assert_eq!(dos_from_parts(2026, 11, 12, 8, 47, 3).unwrap().0, 0x45e1);
        assert!(dos_from_parts(1979, 1, 1, 0, 0, 0).unwrap_err().contains("before 1980") || dos_from_parts(1979, 1, 1, 0, 0, 0).unwrap_err().contains("ZIP does not support"));
    }

    #[test]
    fn zip_headers_record_stamp_mode_and_utf8_bit() {
        let members = vec![
            ("RLLM/a.txt".into(), b"hello".to_vec(), MemberStamp { dos_time: 0x45e1, dos_date: 0x5d6c, external_attr: 0x81b4_0000 }),
            ("RLLM/café.txt".into(), b"c".to_vec(), MemberStamp { dos_time: 0x45e1, dos_date: 0x5d6c, external_attr: 0x81a4_0000 }),
        ];
        let archive = write_zip(&members).unwrap();
        let ascii = zip_header(&archive, "RLLM/a.txt");
        assert_eq!(ascii.flag, 0, "ascii names leave the UTF-8 flag clear");
        assert_eq!((ascii.time, ascii.date, ascii.external_attr), (0x45e1, 0x5d6c, 0x81b4_0000));
        let utf = zip_header(&archive, "RLLM/café.txt");
        assert_eq!(utf.flag, 0x0800);
        assert_eq!(utf.external_attr, 0x81a4_0000);
    }

    struct ZipHeader {
        flag: u16,
        time: u16,
        date: u16,
        external_attr: u32,
    }

    fn zip_header(data: &[u8], name: &str) -> ZipHeader {
        let bytes = name.as_bytes();
        let mut flag = None;
        let mut time = 0;
        let mut date = 0;
        let mut external_attr = 0;
        let mut start = 0;
        while let Some(at) = data[start..].windows(bytes.len()).position(|window| window == bytes) {
            let pos = start + at;
            if pos >= 30 && data[pos - 30..pos - 26] == [0x50, 0x4b, 0x03, 0x04] {
                flag = Some(u16::from_le_bytes(data[pos - 24..pos - 22].try_into().unwrap()));
                time = u16::from_le_bytes(data[pos - 20..pos - 18].try_into().unwrap());
                date = u16::from_le_bytes(data[pos - 18..pos - 16].try_into().unwrap());
            }
            if pos >= 46 && data[pos - 46..pos - 42] == [0x50, 0x4b, 0x01, 0x02] {
                external_attr = u32::from_le_bytes(data[pos - 8..pos - 4].try_into().unwrap());
            }
            start = pos + 1;
        }
        ZipHeader { flag: flag.expect(name), time, date, external_attr }
    }

    #[test]
    fn published_zip_uses_source_mtime_and_mode() {
        let (_dir, root) = fixture();
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::write(root.join("notes/café.txt"), b"cafe\n").unwrap();
        let destination = root.join("destination");
        publish(&root, &destination, false, false).unwrap();
        let zip_path = fs::read_dir(&destination)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("zip"))
            .unwrap();
        let archive = fs::read(&zip_path).unwrap();
        let manifest = root.join("AndroidManifest.xml");
        let meta = fs::metadata(&manifest).unwrap();
        let (time, date) = dos_stamp(meta.modified().unwrap()).unwrap();
        let header = zip_header(&archive, "RLLM/AndroidManifest.xml");
        assert_eq!(header.flag, 0);
        assert_eq!((header.time, header.date), (time, date));
        assert_eq!(header.external_attr, (meta.permissions().mode() & 0xFFFF) << 16);
        let cafe = zip_header(&archive, "RLLM/notes/café.txt");
        assert_eq!(cafe.flag, 0x0800);
        let cafe_meta = fs::metadata(root.join("notes/café.txt")).unwrap();
        assert_eq!(cafe.external_attr, (cafe_meta.permissions().mode() & 0xFFFF) << 16);
        let notes = zip_header(&archive, "RLLM/release/NOTES.md");
        let notes_meta = fs::metadata(root.join("release/NOTES.md")).unwrap();
        assert_eq!(notes.external_attr, (notes_meta.permissions().mode() & 0xFFFF) << 16);
        let _ = fs::remove_dir_all(root.join("destination"));
        let readme = fs::File::options().write(true).open(root.join("README.md")).unwrap();
        let old = UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        readme.set_modified(old).unwrap();
        drop(readme);
        let outside = env::temp_dir().join(format!("rllm-pkg-readme-{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        publish(&root, &outside, false, false).unwrap();
        let zip_path = fs::read_dir(&outside)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("zip"))
            .unwrap();
        let archive = fs::read(&zip_path).unwrap();
        let _ = fs::remove_dir_all(&outside);
        let header = zip_header(&archive, "RLLM/README.md");
        let (old_time, old_date) = dos_stamp(old).unwrap();
        assert_ne!((header.time, header.date), (old_time, old_date), "rewritten README kept its pre-publish mtime");
    }

    fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..n).map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 24) as u8
        }).collect()
    }

    #[test]
    fn deflate_empty_is_fixed_block() {
        let out = deflate(b"");
        assert_eq!(out, [3, 0]);
        assert_eq!((out[0] >> 1) & 3, 1);
        assert_eq!(inflate(&out, 0).unwrap(), b"");
    }

    #[test]
    fn deflate_short_literals_match_fixed_block() {
        let out = deflate(b"abc");
        assert_eq!(out, [75, 76, 74, 6, 0]);
        assert_eq!((out[0] >> 1) & 3, 1);
        assert_eq!(inflate(&out, 3).unwrap(), b"abc");
    }

    #[test]
    fn deflate_long_run_is_dynamic_and_smaller() {
        let data = vec![b'a'; 8000];
        let out = deflate(&data);
        assert_eq!((out[0] >> 1) & 3, 2);
        assert!(out.len() < stored_blocks(&data).len());
        assert_eq!(inflate(&out, data.len()).unwrap(), data);
        let mut phrase = Vec::new();
        let piece = b"the quick brown fox jumps over the lazy dog. ";
        for _ in 0..80 { phrase.extend_from_slice(piece); }
        let compressed = deflate(&phrase);
        assert_eq!((compressed[0] >> 1) & 3, 2);
        assert!(compressed.len() < stored_blocks(&phrase).len());
        assert_eq!(inflate(&compressed, phrase.len()).unwrap(), phrase);
    }

    #[test]
    fn deflate_growth_uses_stored_blocks() {
        let data = noise(65536, 7);
        let out = deflate(&data);
        assert_eq!(out, stored_blocks(&data), "growth case uses stored blocks");
        assert_eq!(out.len(), 65546);
        assert_eq!(out[0], 0);
        assert_eq!(u16::from_le_bytes([out[1], out[2]]), 65535);
        assert_eq!(inflate(&out, data.len()).unwrap(), data);
    }

    #[test]
    fn deflate_roundtrip_at_stored_block_boundary() {
        for size in [65535usize, 65536, 70000] {
            let data = vec![b'b'; size];
            let out = deflate(&data);
            assert!(out.len() < stored_blocks(&data).len(), "{size}");
            assert_ne!((out[0] >> 1) & 3, 0, "{size}");
            assert_eq!(inflate(&out, data.len()).unwrap(), data);
        }
        let mut wide = noise(32768, 99);
        let head = wide[..1000].to_vec();
        wide.extend_from_slice(&head);
        assert_eq!(inflate(&deflate(&wide), wide.len()).unwrap(), wide);
    }

    #[test]
    fn truncated_deflate_stream_is_refused() {
        assert_eq!(inflate(&[3], 0).unwrap_err(), "Truncated compressed member");
    }

    #[test]
    fn length_limited_codes_stay_complete() {
        let mut freq = [0u64; 40];
        let mut a = 1u64;
        let mut b = 1u64;
        for slot in &mut freq {
            *slot = a;
            let next = a + b;
            a = b;
            b = next;
        }
        let open = huffman_lengths(&freq, 64);
        assert!(open.iter().any(|&len| len > 15));
        let limited = huffman_lengths(&freq, 15);
        assert!(limited.iter().all(|&len| (1..=15).contains(&len)));
        assert!(matches!(code_shape(&limited), Shape::Complete));
    }

    #[test]
    fn archive_writer_shrinks_a_compressible_member() {
        let plain = vec![b'a'; 4000];
        let archive = archive_bytes(&[("RLLM/a.txt".into(), plain.clone())]).unwrap();
        assert_eq!(read_archive(&archive).unwrap(), vec![("RLLM/a.txt".into(), plain.clone())]);
        assert_eq!(u16_at(&archive, 8).unwrap(), 8);
        let comp_len = u32_at(&archive, 18).unwrap() as usize;
        assert!(comp_len < plain.len() + 5, "compressible member is smaller than stored");
        let payload = 30 + "RLLM/a.txt".len();
        assert_eq!((archive[payload] >> 1) & 3, 2);
    }

    #[test]
    fn archive_writer_stores_a_growth_member() {
        let plain = noise(2048, 0xA5A5_1234);
        let archive = archive_bytes(&[("RLLM/r.bin".into(), plain.clone())]).unwrap();
        assert_eq!(read_archive(&archive).unwrap()[0].1, plain);
        let name_len = "RLLM/r.bin".len();
        let comp_len = u32_at(&archive, 18).unwrap() as usize;
        let payload = &archive[30 + name_len..30 + name_len + comp_len];
        assert_eq!(payload, stored_blocks(&plain).as_slice(), "growth case uses stored blocks");
    }

    fn plant(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn newline_body(lines: usize) -> String {
        let mut body = String::from("pass");
        for _ in 0..lines {
            body.push('\n');
        }
        body
    }

    fn refusal_lines(err: &str) -> String {
        err.lines().filter(|line| line.starts_with("refusing:")).collect::<Vec<_>>().join("\n")
    }

    fn census_line(report: &str) -> &str {
        report.lines().find(|line| line.starts_with("python_census:")).unwrap_or("")
    }

    fn assert_publish_refuses(root: &Path, sentence: &str) {
        let destination = root.join("destination");
        let before = snapshot(root);
        let err = publish(root, &destination, false, false).expect_err(sentence);
        assert_eq!(snapshot(root), before, "refused publish changed the tree: {err}");
        assert!(destination.is_dir(), "publish did not reach the destination");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0, "refused publish wrote an artifact");
        assert_eq!(refusal_lines(&err), sentence, "{err}");
        assert!(err.contains("python_census:"), "{err}");
        assert!(!err.contains("Source differs"), "{err}");
    }

    #[test]
    fn python_line_count_counts_a_final_partial_line() {
        assert_eq!(python_line_count(b""), 0);
        assert_eq!(python_line_count(b"a\n"), 1);
        assert_eq!(python_line_count(b"a\nb"), 2);
        assert_eq!(python_line_count(b"a\nb\n"), 2);
    }

    #[test]
    fn publish_refuses_core_python() {
        let (_dir, root) = fixture();
        plant(&root, "core/sub/plant.py", "print(1)\n");
        assert_publish_refuses(&root, "refusing: product-code violation: core/ contains Python core/sub/plant.py");
    }

    #[test]
    fn publish_refuses_platform_python() {
        let (_dir, root) = fixture();
        plant(&root, "platform/com/local/plant.py", "print(1)\n");
        assert_publish_refuses(&root, "refusing: product-code violation: platform/ contains Python platform/com/local/plant.py");
    }

    #[test]
    fn publish_refuses_tools_python_outside_allowlist() {
        let (_dir, root) = fixture();
        plant(&root, "tools/helper.py", "print(1)\n");
        plant(&root, "tools/sub/checks.py", "print(1)\n");
        let destination = root.join("destination");
        let before = snapshot(&root);
        let err = publish(&root, &destination, false, false).expect_err("tools python outside the allowlist must refuse");
        assert_eq!(snapshot(&root), before, "{err}");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0, "{err}");
        assert_eq!(
            refusal_lines(&err),
            "refusing: tools/ Python tools/helper.py is outside the allowlist\nrefusing: tools/ Python tools/sub/checks.py is outside the allowlist",
            "{err}"
        );
        assert!(census_line(&err).contains("tools files 3 lines 3"), "{err}");
        assert!(census_line(&err).contains("checks.py (full-suite unit gate (--all-units, native twin gate <units>), selected-unit runner (--units, native twin gate --units), and connected-group runner (no-flag sweep; native orchestration and record writing is harness sweep over the Python group scripts))"), "{err}");
        assert!(census_line(&err).contains("package.py (publisher until a signed candidate is published with the native package binary)"), "{err}");
    }

    #[test]
    fn checks_py_role_names_the_native_twin_and_refuses_a_python_only_label() {
        let role = TOOL_PYTHON.iter().find(|item| item.name == "checks.py").unwrap().role;
        let all_at = role.find("full-suite unit gate").unwrap();
        let selected_at = role.find("selected-unit runner").unwrap();
        let sweep_at = role.find("connected-group").unwrap();
        assert!(all_at < selected_at && selected_at < sweep_at, "{role}");
        assert!(!role[..all_at].contains("Python-only"), "{role}");
        let all_units = &role[all_at..selected_at];
        let selected = &role[selected_at..sweep_at];
        let sweep = &role[sweep_at..];
        assert!(all_units.contains("--all-units"), "{all_units}");
        assert!(all_units.contains("native twin gate <units>"), "{all_units}");
        assert!(!all_units.contains("Python-only"), "native-equivalent --all-units role is described as Python-only: {all_units}");
        assert!(selected.contains("native twin gate --units"), "{selected}");
        assert!(!selected.contains("--all-units"), "{selected}");
        assert!(!selected.contains("Python-only"), "native-equivalent --units role is described as Python-only: {selected}");
        assert!(sweep.contains("no-flag"), "{sweep}");
        assert!(sweep.contains("harness sweep"), "{sweep}");
        assert!(sweep.contains("native orchestration and record writing"), "{sweep}");
        assert!(sweep.contains("Python group scripts"), "{sweep}");
        assert!(!sweep.contains("native twin harness sweep"), "the no-flag sweep is orchestration over Python group scripts, not a native twin of those scripts: {sweep}");
        assert!(!sweep.contains("Python-only"), "the no-flag connected-group sweep is still labeled Python-only: {sweep}");
    }

    #[test]
    fn publish_refuses_verification_python_over_ceiling() {
        let (_dir, root) = fixture();
        assert_eq!(VERIFICATION_PY_FILE_CEILING, 50);
        assert_eq!(VERIFICATION_PY_LARGEST_SHOWN, 8);
        for index in 0..51 {
            let body = if index == 50 { "pass\npass\npass\n" } else { "pass\n" };
            plant(&root, &format!("verification/debt{index}.py"), body);
        }
        assert_publish_refuses(
            &root,
            "refusing: verification/ Python files 51 exceed the recorded ceiling 50\nrefusing: verification/ Python largest 8 of 51: verification/debt50.py lines 3, verification/debt0.py lines 1, verification/debt1.py lines 1, verification/debt10.py lines 1, verification/debt11.py lines 1, verification/debt12.py lines 1, verification/debt13.py lines 1, verification/debt14.py lines 1",
        );
    }

    #[test]
    fn publish_refuses_verification_python_lines_over_ceiling() {
        let (_dir, root) = fixture();
        assert_eq!(VERIFICATION_PY_LINE_CEILING, 4407);
        plant(&root, "verification/a.py", "pass\n");
        plant(&root, "verification/z.py", &newline_body(4407));
        assert_eq!(python_line_count(b"pass\n") + python_line_count(newline_body(4407).as_bytes()), 4408);
        assert_publish_refuses(
            &root,
            "refusing: verification/ Python lines 4408 exceed the recorded ceiling 4407\nrefusing: verification/ Python largest 2 of 2: verification/z.py lines 4407, verification/a.py lines 1",
        );
    }

    #[test]
    fn publish_passes_verification_python_lines_at_ceiling() {
        let (_dir, root) = fixture();
        assert_eq!(VERIFICATION_PY_LINE_CEILING, 4407);
        let body = newline_body(VERIFICATION_PY_LINE_CEILING);
        plant(&root, "verification/debt.py", &body);
        assert_eq!(python_line_count(body.as_bytes()), 4407);
        let destination = root.join("destination");
        let report = publish(&root, &destination, false, false).unwrap();
        assert!(report.contains("signed candidate; acceptance incomplete"), "{report}");
        assert!(!report.contains("would write:"), "{report}");
        assert!(!report.contains("refusing:"), "{report}");
        assert!(root.join("release/NOTES.md").is_file(), "real publish did not write release notes");
        assert!(root.join("verification/RESULTS.json").is_file());
        let names = fs::read_dir(&destination).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.iter().any(|name| name.ends_with(".zip")), "{names:?}");
    }

    #[test]
    fn publish_python_census_byte_bound_is_two_sided() {
        assert_eq!(PYTHON_CENSUS_BYTE_CAP, 1_048_576);
        let sentence = "refusing: verification/over.py exceeds the Python census byte bound 1048576";
        let (_over_dir, over_root) = fixture();
        fs::write(over_root.join("verification/over.py"), vec![b'x'; PYTHON_CENSUS_BYTE_CAP as usize + 1]).unwrap();
        let over_destination = over_root.join("destination");
        let before = snapshot(&over_root);
        let err = publish(&over_root, &over_destination, false, false).expect_err("over the byte bound");
        assert_eq!(snapshot(&over_root), before, "{err}");
        assert!(over_destination.is_dir(), "{err}");
        assert_eq!(fs::read_dir(&over_destination).unwrap().count(), 0, "{err}");
        assert_eq!(err, sentence);
        assert!(!err.contains("Source differs"), "{err}");
        let (_at_dir, at_root) = fixture();
        fs::write(at_root.join("verification/at.py"), vec![b'x'; PYTHON_CENSUS_BYTE_CAP as usize]).unwrap();
        let at_destination = at_root.join("destination");
        let report = publish(&at_root, &at_destination, false, true).unwrap();
        let line = census_line(&report);
        assert!(line.contains("verification files 1 lines 1 ceiling files 50 lines 4407"), "{line}");
        assert!(line.contains("core files 0 lines 0"), "{line}");
        assert!(!report.contains("refusing:"), "{report}");
        assert_eq!(fs::read_dir(&at_destination).unwrap().count(), 0, "at-bound twin wrote an artifact");
        assert!(!at_root.join("release").exists(), "at-bound dry-run created release/");
    }

    #[test]
    fn publish_refuses_python_elsewhere() {
        let (_dir, root) = fixture();
        plant(&root, "docs/notes/plant.py", "print(1)\n");
        assert_publish_refuses(&root, "refusing: Python outside core/, platform/, tools/, and verification/: docs/notes/plant.py");
    }

    #[test]
    fn publish_passes_compliant_python_census() {
        let (_dir, root) = fixture();
        plant(&root, "tools/package.py", "# pending native publish\n");
        plant(&root, "docs/keep.txt", "not python\n");
        plant(&root, "notes/keep.txt", "not python\n");
        for index in 0..VERIFICATION_PY_FILE_CEILING {
            plant(&root, &format!("verification/debt{index}.py"), "pass\n");
        }
        plant(&root, "tooling/ndk/helper.py", "print(1)\n");
        let report = publish(&root, &root.join("destination"), false, true).unwrap();
        let line = census_line(&report);
        assert!(report.contains("build_inputs:"), "{report}");
        assert!(report.contains("would write:"), "{report}");
        assert!(!report.contains("refusing:"), "{report}");
        assert!(line.contains("core files 0 lines 0"), "{line}");
        assert!(line.contains("platform files 0 lines 0"), "{line}");
        assert!(line.contains("tools files 2 lines 2"), "{line}");
        assert!(line.contains("verification files 50 lines 50 ceiling files 50 lines 4407"), "{line}");
        assert!(line.contains("elsewhere files 0 lines 0"), "{line}");
        assert!(line.contains("checks.py (full-suite unit gate (--all-units, native twin gate <units>), selected-unit runner (--units, native twin gate --units), and connected-group runner (no-flag sweep; native orchestration and record writing is harness sweep over the Python group scripts))"), "{line}");
        assert!(line.contains("package.py (publisher until a signed candidate is published with the native package binary)"), "{line}");
    }

    #[test]
    fn publish_python_census_counts_move_when_a_file_is_added() {
        let (_dir, root) = fixture();
        let destination = root.join("destination");
        let first = publish(&root, &destination, false, true).unwrap();
        assert!(first.contains("build_inputs:"), "{first}");
        assert!(first.contains("would write:"), "{first}");
        let before = census_line(&first).to_string();
        assert!(before.contains("core files 0 lines 0"), "{before}");
        assert!(before.contains("platform files 0 lines 0"), "{before}");
        assert!(before.contains("tools files 1 lines 1"), "{before}");
        assert!(before.contains("verification files 0 lines 0 ceiling files 50 lines 4407"), "{before}");
        assert!(before.contains("elsewhere files 0 lines 0"), "{before}");
        plant(&root, "output/hidden.py", "print(1)\n");
        plant(&root, "tooling/helper.py", "print(1)\n");
        plant(&root, ".qwen/helper.py", "print(1)\n");
        plant(&root, "toolchain/helper.py", "print(1)\n");
        plant(&root, ".playwright-mcp/helper.py", "print(1)\n");
        let hidden = publish(&root, &destination, false, true).unwrap();
        assert_eq!(census_line(&hidden), before, "non-product roots moved the census");
        plant(&root, "verification/debt.py", "print(1)\n");
        plant(&root, "tools/package.py", "# pending native publish\n");
        let second = publish(&root, &destination, false, true).unwrap();
        let after = census_line(&second);
        assert!(after.contains("tools files 2 lines 2"), "{after}");
        assert!(after.contains("verification files 1 lines 1"), "{after}");
        assert_ne!(before, after);
    }

    #[test]
    fn project_python_census_counts_this_tree() {
        let root = project_root().unwrap();
        let files = collect_tree(&root).unwrap();
        assert!(files.iter().any(|path| path == "core/lib.rs"), "walk missed core/");
        assert!(files.iter().any(|path| path == "tools/checks.py"), "walk missed tools/checks.py");
        assert!(files.iter().any(|path| path == "tools/package.py"), "walk missed tools/package.py");
        assert!(files.iter().any(|path| path.starts_with("verification/") && path.ends_with(".py")), "walk missed verification/");
        let line = enforce_python_census(&root, &files).unwrap();
        assert_eq!(VERIFICATION_PY_FILE_CEILING, 50);
        assert!(line.contains("core files 0 lines 0;"), "{line}");
        assert!(line.contains("platform files 0 lines 0;"), "{line}");
        assert!(line.contains("tools files 2 lines 191 ["), "{line}");
        assert!(line.contains("verification files 50 lines 4407 ceiling files 50 lines 4407;"), "{line}");
        assert_eq!(VERIFICATION_PY_LINE_CEILING, 4407);
        assert!(line.contains("elsewhere files 0 lines 0"), "{line}");
        println!("{line}");
    }
}

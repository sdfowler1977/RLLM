use super::Result;

struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Bits<'_> {
    fn take(&mut self, count: usize) -> Result<usize> {
        if count > 16 || self.at.checked_add(count).ok_or("Compressed offset overflow")? > self.bytes.len() * 8 {
            return Err("Truncated compressed member".into());
        }
        let mut value = 0;
        for shift in 0..count {
            value |= (((self.bytes[self.at / 8] >> (self.at % 8)) & 1) as usize) << shift;
            self.at += 1;
        }
        Ok(value)
    }
}

struct Codes {
    counts: [usize; 16],
    first: [usize; 16],
    offsets: [usize; 16],
    symbols: Vec<usize>,
}
impl Codes {
    fn new(lengths: &[usize], single: bool, empty: bool) -> Result<Self> {
        let mut counts = [0usize; 16];
        for &length in lengths {
            if length > 15 { return Err("Invalid compressed code length".into()); }
            if length != 0 { counts[length] += 1; }
        }
        let size: usize = counts.iter().sum();
        let mut left = 1isize;
        for &count in &counts[1..] {
            left = (left << 1) - count as isize;
            if left < 0 { return Err("Oversubscribed compressed codes".into()); }
        }
        if left != 0 && !(single && size == 1 && counts[1] == 1) && !(empty && size == 0) {
            return Err("Incomplete compressed codes".into());
        }
        let mut first = [0; 16];
        let mut offsets = [0; 16];
        for length in 1..16 {
            first[length] = (first[length - 1] + counts[length - 1]) << 1;
            offsets[length] = offsets[length - 1] + counts[length - 1];
        }
        let mut symbols = vec![0; size];
        let mut next = offsets;
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 { symbols[next[length]] = symbol; next[length] += 1; }
        }
        Ok(Self { counts, first, offsets, symbols })
    }
    fn symbol(&self, bits: &mut Bits<'_>) -> Result<usize> {
        let mut code = 0;
        for length in 1..16 {
            code = (code << 1) | bits.take(1)?;
            if code >= self.first[length] {
                let index = code - self.first[length];
                if index < self.counts[length] { return Ok(self.symbols[self.offsets[length] + index]); }
            }
        }
        Err("Invalid compressed symbol".into())
    }
}

fn tables(bits: &mut Bits<'_>) -> Result<(Codes, Codes)> {
    let literals = bits.take(5)? + 257;
    let distances = bits.take(5)? + 1;
    if literals > 286 { return Err("Reserved compressed literal count".into()); }
    let count = bits.take(4)? + 4;
    let order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut lengths = [0; 19];
    for &symbol in &order[..count] { lengths[symbol] = bits.take(3)?; }
    let code = Codes::new(&lengths, false, false)?;
    let mut lengths = Vec::with_capacity(literals + distances);
    while lengths.len() < literals + distances {
        let symbol = code.symbol(bits)?;
        let (value, repeat) = match symbol {
            0..=15 => (symbol, 1),
            16 => (*lengths.last().ok_or("Compressed repeat has no previous length")?, bits.take(2)? + 3),
            17 => (0, bits.take(3)? + 3),
            18 => (0, bits.take(7)? + 11),
            _ => return Err("Invalid compressed length symbol".into()),
        };
        if lengths.len() + repeat > literals + distances { return Err("Compressed length repeat overflow".into()); }
        lengths.resize(lengths.len() + repeat, value);
    }
    if lengths[256] == 0 { return Err("Compressed member lacks end symbol".into()); }
    Ok((Codes::new(&lengths[..literals], true, false)?, Codes::new(&lengths[literals..], true, true)?))
}

// RFC 1951 raw blocks. The caller supplies the already bounded directory size;
// output never grows past it, including overlapping back-references.
pub fn inflate(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
    if expected > 1024 * 1024 || bytes.len() > 5 * 1024 * 1024 {
        return Err("Compressed member exceeds resource limit".into());
    }
    let mut bits = Bits { bytes, at: 0 };
    let mut out = Vec::new();
    out.try_reserve_exact(expected).map_err(|_| "Compressed member allocation failed")?;
    let mut lengths = [8; 288];
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    let fixed = (Codes::new(&lengths, false, false)?, Codes::new(&[5; 32], false, false)?);
    let mut blocks = 0usize;
    loop {
        blocks += 1;
        if blocks > 65536 { return Err("Compressed member exceeds block-processing limit".into()); }
        let final_block = bits.take(1)? != 0;
        match bits.take(2)? {
            0 => {
                bits.at = (bits.at + 7) & !7;
                let length = bits.take(16)?;
                if (length ^ bits.take(16)?) != 0xffff { return Err("Compressed stored length mismatch".into()); }
                if out.len() + length > expected { return Err("Compressed member exceeds declared size".into()); }
                let start = bits.at / 8;
                let data = bytes.get(start..start + length).ok_or("Truncated compressed stored block")?;
                out.extend_from_slice(data);
                bits.at += length * 8;
            }
            kind @ (1 | 2) => {
                let dynamic;
                let (literal, distance) = if kind == 1 { (&fixed.0, &fixed.1) }
                    else { dynamic = tables(&mut bits)?; (&dynamic.0, &dynamic.1) };
                loop {
                    match literal.symbol(&mut bits)? {
                        symbol @ 0..=255 => {
                            if out.len() == expected { return Err("Compressed member exceeds declared size".into()); }
                            out.push(symbol as u8);
                        }
                        256 => break,
                        symbol @ 257..=285 => {
                            const BASE: [usize; 29] = [3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258];
                            const EXTRA: [usize; 29] = [0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0];
                            let length = BASE[symbol - 257] + bits.take(EXTRA[symbol - 257])?;
                            let symbol = distance.symbol(&mut bits)?;
                            if symbol > 29 { return Err("Reserved compressed distance".into()); }
                            let extra = if symbol < 4 { 0 } else { symbol / 2 - 1 };
                            let base = if symbol < 4 { symbol + 1 } else { (2 + symbol % 2) * (1 << extra) + 1 };
                            let offset = base + bits.take(extra)?;
                            if offset > out.len() || out.len() + length > expected {
                                return Err("Compressed back-reference outside member bounds".into());
                            }
                            for _ in 0..length { out.push(out[out.len() - offset]); }
                        }
                        _ => return Err("Reserved compressed literal".into()),
                    }
                }
            }
            _ => return Err("Reserved compressed block type".into()),
        }
        if final_block { break; }
    }
    if out.len() != expected || (bits.at + 7) / 8 != bytes.len() {
        return Err("Compressed member size or trailing data mismatch".into());
    }
    Ok(out)
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
pub fn deflate(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() >= u32::MAX as usize { return stored_blocks(bytes); }
    let stored = stored_blocks(bytes);
    let compressed = compress_huffman(bytes);
    if compressed.len() < stored.len() { compressed } else { stored }
}

#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn compressed_block_processing_is_bounded() {
        let mut bytes = vec![0u8; (65537 * 10 + 7) / 8];
        for block in 0..65537 {
            let offset = block * 10;
            bytes[(offset + 1) / 8] |= 1 << ((offset + 1) % 8);
            if block == 65536 { bytes[offset / 8] |= 1 << (offset % 8); }
        }
        assert!(inflate(&bytes, 0).unwrap_err().contains("block-processing limit"));
    }
    #[test]
    fn compressed_input_bounds_and_structure() {
        assert_eq!(inflate(&[1, 3, 0, 252, 255, 97, 98, 99], 3).unwrap(), b"abc");
        assert_eq!(inflate(&[75, 76, 74, 6, 0], 3).unwrap(), b"abc");
        assert_eq!(inflate(&[3, 0], 0).unwrap(), b"");
        assert!(inflate(&[75, 76, 74, 6, 0], 2).is_err());
        assert!(inflate(&[75, 76, 74, 6, 0], 4).is_err());
        assert!(inflate(&[75, 76, 74, 6, 0, 0], 3).is_err());
        assert!(inflate(&[1, 3, 0, 252, 254, 97, 98, 99], 3).is_err());
        assert!(inflate(&[7], 0).is_err());
        assert!(inflate(&[3], 0).is_err());
        assert!(Codes::new(&[1,1,1], false, false).is_err());
        assert!(Codes::new(&[2], true, false).is_err());
        assert!(inflate(&[], 1024 * 1024 + 1).is_err());
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
}

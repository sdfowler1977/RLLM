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
}

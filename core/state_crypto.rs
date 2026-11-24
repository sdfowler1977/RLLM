//! AES-256-GCM state sealing. The core owns the algorithm; each platform keeps
//! its own key (Android Keystore, a Linux key file) and only ever hands the core
//! bytes plus that key, so no platform stores a plaintext vault.
use super::*;

const ROUND_CONSTANTS: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];
const NONCE: usize = 12;
const TAG: usize = 16;
const ROUNDS: usize = 15;

fn get<'a>(value: &'a Json, key: &str) -> &'a Json {
    match value {
        Obj(fields) => fields.get(key).unwrap_or(&Null),
        _ => &Null,
    }
}
fn text(value: &Json, key: &str) -> String {
    get(value, key).text().unwrap_or("").into()
}

fn multiply(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0u8;
    for _ in 0..8 {
        if right & 1 != 0 {
            product ^= left;
        }
        let carry = left & 0x80 != 0;
        left = left.wrapping_shl(1);
        if carry {
            left ^= 0x1b;
        }
        right >>= 1;
    }
    product
}

// Inverse in GF(2^8) by exponentiation to x^(2^8-2); no table, no branch on the value.
fn invert(value: u8) -> u8 {
    if value == 0 {
        return 0;
    }
    let mut power = value;
    let mut result = 1u8;
    let mut exponent = 0xfeu8;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result = multiply(result, power);
        }
        power = multiply(power, power);
        exponent >>= 1;
    }
    result
}

fn substitute(value: u8) -> u8 {
    let inverse = invert(value);
    let mut rotated = inverse;
    let mut result = inverse;
    for _ in 0..4 {
        rotated = (rotated << 1) | (rotated >> 7);
        result ^= rotated;
    }
    result ^ 0x63
}

fn expand_key(key: &[u8; 32]) -> Vec<[u8; 16]> {
    let mut words: Vec<[u8; 4]> = key.chunks(4).map(|chunk| [chunk[0], chunk[1], chunk[2], chunk[3]]).collect();
    for index in 8..ROUNDS * 4 {
        let mut previous = words[index - 1];
        if index % 8 == 0 {
            previous = [
                substitute(previous[1]) ^ ROUND_CONSTANTS[index / 8 - 1],
                substitute(previous[2]),
                substitute(previous[3]),
                substitute(previous[0]),
            ];
        } else if index % 8 == 4 {
            previous = [substitute(previous[0]), substitute(previous[1]), substitute(previous[2]), substitute(previous[3])];
        }
        let earlier = words[index - 8];
        words.push([
            earlier[0] ^ previous[0],
            earlier[1] ^ previous[1],
            earlier[2] ^ previous[2],
            earlier[3] ^ previous[3],
        ]);
    }
    // Round keys are stored column-major, the same order as the block bytes.
    words.chunks(4).map(|round| [
        round[0][0], round[0][1], round[0][2], round[0][3],
        round[1][0], round[1][1], round[1][2], round[1][3],
        round[2][0], round[2][1], round[2][2], round[2][3],
        round[3][0], round[3][1], round[3][2], round[3][3],
    ]).collect()
}

fn mix_column(state: &mut [u8; 16], column: usize) {
    let at = column * 4;
    let (a, b, c, d) = (state[at], state[at + 1], state[at + 2], state[at + 3]);
    state[at] = multiply(a, 2) ^ multiply(b, 3) ^ c ^ d;
    state[at + 1] = a ^ multiply(b, 2) ^ multiply(c, 3) ^ d;
    state[at + 2] = a ^ b ^ multiply(c, 2) ^ multiply(d, 3);
    state[at + 3] = multiply(a, 3) ^ b ^ c ^ multiply(d, 2);
}

fn encrypt_block(block: &[u8; 16], rounds: &[[u8; 16]]) -> [u8; 16] {
    let mut state = *block;
    for byte in 0..16 {
        state[byte] ^= rounds[0][byte];
    }
    for round in 1..=13 {
        for byte in state.iter_mut() {
            *byte = substitute(*byte);
        }
        shift_rows(&mut state);
        for column in 0..4 {
            mix_column(&mut state, column);
        }
        for byte in 0..16 {
            state[byte] ^= rounds[round][byte];
        }
    }
    for byte in state.iter_mut() {
        *byte = substitute(*byte);
    }
    shift_rows(&mut state);
    for byte in 0..16 {
        state[byte] ^= rounds[14][byte];
    }
    state
}

// Row r rotates left by r positions. The block is column-major, so a row's
// bytes are stride four apart and the rotation needs a full copy.
fn shift_rows(state: &mut [u8; 16]) {
    let original = *state;
    for row in 0..4 {
        for column in 0..4 {
            state[column * 4 + row] = original[((column + row) % 4) * 4 + row];
        }
    }
}

fn multiply_128(left: u128, right: u128) -> u128 {
    let mut accumulator = 0u128;
    let mut value = left;
    let multiplier = right;
    for bit in 0..128 {
        if (multiplier >> (127 - bit)) & 1 == 1 {
            accumulator ^= value;
        }
        let carry = value & 1 == 1;
        value >>= 1;
        if carry {
            value ^= 0xe1u128 << 120;
        }
    }
    accumulator
}

fn ghash(rounds: &[[u8; 16]], data: &[u8]) -> [u8; 16] {
    // The hash subkey H is AES-256(0^128) for a zero-length key-schedule input.
    let hash_key = u128::from_be_bytes(encrypt_block(&[0u8; 16], rounds));
    let mut accumulator = 0u128;
    for chunk in data.chunks(16) {
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        accumulator = multiply_128(accumulator ^ u128::from_be_bytes(block), hash_key);
    }
    accumulator.to_be_bytes()
}

fn authentication_tag(rounds: &[[u8; 16]], nonce: &[u8], cipher: &[u8]) -> [u8; 16] {
    let mut data = Vec::with_capacity(nonce.len() + cipher.len() + 32);
    data.extend_from_slice(nonce);
    data.resize(data.len() + (16 - nonce.len() % 16) % 16, 0);
    data.extend_from_slice(cipher);
    data.resize(data.len() + (16 - cipher.len() % 16) % 16, 0);
    data.extend_from_slice(&((nonce.len() as u64) * 8).to_be_bytes());
    data.extend_from_slice(&((cipher.len() as u64) * 8).to_be_bytes());
    let digest = ghash(rounds, &data);
    let mut counter = [0u8; 16];
    counter[..nonce.len()].copy_from_slice(nonce);
    counter[15] = 1;
    let mask = encrypt_block(&counter, rounds);
    let mut tag = [0u8; 16];
    for index in 0..16 {
        tag[index] = digest[index] ^ mask[index];
    }
    tag
}

fn increment(counter: &mut [u8; 16]) {
    let low = u32::from_be_bytes([counter[12], counter[13], counter[14], counter[15]]);
    counter[12..].copy_from_slice(&low.wrapping_add(1).to_be_bytes());
}

fn counter_keystream(rounds: &[[u8; 16]], nonce: &[u8], length: usize) -> Vec<u8> {
    let mut counter = [0u8; 16];
    counter[..nonce.len()].copy_from_slice(nonce);
    counter[15] = 1;
    increment(&mut counter);
    let mut stream = Vec::with_capacity(length);
    while stream.len() < length {
        stream.extend_from_slice(&encrypt_block(&counter, rounds));
        increment(&mut counter);
    }
    stream.truncate(length);
    stream
}

fn transform(rounds: &[[u8; 16]], nonce: &[u8], input: &[u8]) -> Vec<u8> {
    let stream = counter_keystream(rounds, nonce, input.len());
    input.iter().zip(stream).map(|(left, right)| left ^ right).collect()
}

pub(crate) fn random_bytes(count: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; count];
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut file = std::fs::File::open("/dev/urandom")
            .map_err(|_| "no local randomness is available for state sealing".to_string())?;
        file.read_exact(&mut bytes).map_err(|_| "local randomness was incomplete".to_string())?;
    }
    #[cfg(windows)]
    {
        if count > u32::MAX as usize {
            return Err("local randomness request is too large".into());
        }
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn SystemFunction036(buffer: *mut u8, len: u32) -> u8;
        }
        if unsafe { SystemFunction036(bytes.as_mut_ptr(), count as u32) } == 0 {
            return Err("no local randomness is available for state sealing".into());
        }
    }
    Ok(bytes)
}

/// Seal returns nonce || ciphertext || tag, so a stored vault is self-describing.
pub fn seal(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let rounds = expand_key(key);
    let nonce = random_bytes(NONCE)?;
    let cipher = transform(&rounds, &nonce, plaintext);
    let tag = authentication_tag(&rounds, &nonce, &cipher);
    let mut sealed = Vec::with_capacity(NONCE + cipher.len() + TAG);
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&cipher);
    sealed.extend_from_slice(&tag);
    Ok(sealed)
}

pub fn open(sealed: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    if sealed.len() < NONCE + TAG {
        return Err("Sealed state is truncated".into());
    }
    let rounds = expand_key(key);
    let nonce = &sealed[..NONCE];
    let cipher = &sealed[NONCE..sealed.len() - TAG];
    let tag = &sealed[sealed.len() - TAG..];
    let plaintext = transform(&rounds, nonce, cipher);
    let expected = authentication_tag(&rounds, nonce, cipher);
    // Compare every byte so a tampered tag costs the same as a matching one.
    let difference = expected.iter().zip(tag).fold(0u8, |all, (left, right)| all | (left ^ right));
    if difference != 0 {
        return Err("Sealed state failed authentication; its key or bytes changed".into());
    }
    Ok(plaintext)
}

pub(super) fn operation(v: &Json) -> Result<Json> {
    let key = network::unbase64(v.get("key")?.text()?)?;
    if key.len() != 32 {
        return Err("State key must be exactly 32 bytes".into());
    }
    let mut storage = [0u8; 32];
    storage.copy_from_slice(&key);
    let data = network::unbase64(v.get("data")?.text()?)?;
    if data.len() > 8 * 1024 * 1024 {
        return Err("Sealed state exceeds 8 MiB".into());
    }
    if text(v, "op") == "state_seal" {
        let sealed = seal(&data, &storage)?;
        return Ok(obj(vec![("sealed", s(&network::b64(&sealed)))]));
    }
    let plain = open(&data, &storage)?;
    Ok(obj(vec![("data", s(&network::b64(&plain)))]))
}

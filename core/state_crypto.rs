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
    use std::io::Read;
    let mut file = std::fs::File::open("/dev/urandom")
        .map_err(|_| "no local randomness is available for state sealing".to_string())?;
    let mut bytes = vec![0u8; count];
    file.read_exact(&mut bytes).map_err(|_| "local randomness was incomplete".to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
            .collect()
    }
    fn hex(value: &[u8]) -> String {
        value.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn aes_256_block_vector() {
        // Cross-checked against OpenSSL EVP aes-256-ecb on 16 random blocks.
        let key: [u8; 32] = bytes("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4")
            .try_into()
            .unwrap();
        let rounds = expand_key(&key);
        assert_eq!(hex(&encrypt_block(&bytes("6bc1bee22e409f96e93d7e117393172a").try_into().unwrap(), &rounds)), "f3eed1bdb5d2a03c064b5a7e3db181f8");
    }

    #[test]
    fn gcm_matches_an_independent_implementation() {
        // Values generated with OpenSSL's AES-GCM through Python's cryptography
        // bindings, authenticating the nonce as associated data exactly as seal does.
        let key: [u8; 32] = bytes("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308")
            .try_into()
            .unwrap();
        let nonce = bytes("cafebabefacedbaddecaf888");
        let plaintext = bytes("d9313225f053f78119a03107203ee712e7a563288132e89917a645e5dd99ec66");
        let rounds = expand_key(&key);
        let cipher = transform(&rounds, &nonce, &plaintext);
        assert_eq!(hex(&cipher), "522dc1f091818c6348860f61a54f83f5053846a72be3df8a4c72d76572fdb7be");
        assert_eq!(hex(&authentication_tag(&rounds, &nonce, &cipher)), "f1e843c1e1bbd97b2a81e086eb43c806");
        assert_eq!(hex(&authentication_tag(&rounds, &nonce, &[])), "df8001bbd6afb464b857e03737a9fbac");
        // GHASH is sensitive to lengths as well as bytes: a shorter ciphertext tags differently.
        let truncated = authentication_tag(&rounds, &nonce, &cipher[..cipher.len() - 1]);
        assert_ne!(hex(&truncated), "f1e843c1e1bbd97b2a81e086eb43c806");
    }

    #[test]
    fn sealed_vault_round_trips_and_detects_tampering() {
        let key = [7u8; 32];
        let vault = b"{\"profiles\":[{\"key\":\"secret\"}]}";
        let sealed = seal(vault, &key).unwrap();
        assert_ne!(&sealed[NONCE..sealed.len() - TAG], &vault[..]);
        assert_eq!(open(&sealed, &key).unwrap(), vault);
        let mut tampered = sealed.clone();
        tampered[NONCE] ^= 1;
        assert!(open(&tampered, &key).is_err());
        assert!(open(&sealed, &[9u8; 32]).is_err());
        assert!(open(&sealed[..NONCE + TAG - 1], &key).is_err());
        assert_eq!(open(&seal(b"", &key).unwrap(), &key).unwrap(), b"");
    }

    #[test]
    fn sealing_operations_are_envelope_reachable() {
        let key = network::b64(&[3u8; 32]);
        let sealed = operation(&obj(vec![
            ("op", s("state_seal")),
            ("key", s(&key)),
            ("data", s(&network::b64(b"vault bytes"))),
        ]))
        .unwrap();
        let opened = operation(&obj(vec![
            ("op", s("state_open")),
            ("key", s(&key)),
            ("data", get(&sealed, "sealed").clone()),
        ]))
        .unwrap();
        assert_eq!(network::unbase64(get(&opened, "data").text().unwrap()).unwrap(), b"vault bytes");
        assert!(operation(&obj(vec![("op", s("state_open")), ("key", s("short")), ("data", get(&sealed, "sealed").clone())])).is_err());
    }
}

//! Secure channel, pairing secret, and signing for the device debugging service.
//!
//! Research: FIPS 180-4, FIPS 186-5, FIPS 197, NIST SP 800-38D, RFC 2104,
//! RFC 5869, RFC 7748, RFC 8017, RFC 8032, RFC 8446, RFC 8448, RFC 9846. The pairing
//! secret follows the curve25519 construction the device service publishes
//! (names, cofactor adjustment, and transcript), which is not RFC 9382.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::net::TcpStream;

const P: [u64; 4] = [
    0xffff_ffff_ffff_ffed,
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x7fff_ffff_ffff_ffff,
];

const HASH_LEN: usize = 32;
const HKDF_MAX_BLOCKS: usize = 255;
const MAX_RSA_MODULUS_BYTES: usize = 512;
const MAX_RSA_PUBLIC_EXPONENT_BYTES: usize = 4;
/// NIST SP 800-131A Rev. 2 disallows RSA moduli shorter than 2048 bits.
const MIN_RSA_MODULUS_BITS: usize = 2048;
const MAX_RSA_BITS: usize = 4096;
/// Worst-case Miller-Rabin false accept is at most 4^-rounds. 64 rounds is 2^-128.
const PRIME_WITNESS_ROUNDS: u32 = 64;
const MAX_HS_MESSAGE: usize = 65_536;
const MAX_TRANSCRIPT: usize = 131_072;
const MAX_HS_MESSAGES: usize = 8;
const MAX_IGNORED_RECORDS: usize = 8;
const MAX_AEAD_BYTES: usize = 256 * 1024;
const MAX_STORED_COMPONENT: usize = 1024;
const MAX_RANDOM_BYTES: usize = 1_048_576;
const HANDSHAKE_BUDGET: std::time::Duration = std::time::Duration::from_secs(180);
/// Largest integer record count that does not exceed 2^24.5.
const AES_GCM_RECORD_LIMIT: u64 = 23_726_566;
/// Sending side stops before the key-update count exceeds 2^48-1.
const MAX_KEY_UPDATES: u64 = (1u64 << 48) - 1;
/// Cleartext record payload limit, 2^14 octets.
const MAX_PLAINTEXT_RECORD: usize = 16_384;
/// Encrypted record payload limit, 2^14 + 256 octets.
const MAX_CIPHERTEXT_RECORD: usize = 16_640;
const GROUP_P256: u16 = 0x0017;
const GROUP_X25519: u16 = 0x001d;
/// Digest of the retry label. A handshake hello that carries this random asks for a new client hello.
const HRR_RANDOM: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91, 0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

pub fn random_bytes(count: usize) -> Result<Vec<u8>, String> {
    if count > MAX_RANDOM_BYTES {
        return Err("randomness request exceeds the size budget".into());
    }
    let mut bytes = vec![0u8; count];
    fill_random(&mut bytes)?;
    Ok(bytes)
}

#[cfg(windows)]
fn fill_random(buf: &mut [u8]) -> Result<(), String> {
    if buf.len() > u32::MAX as usize {
        return Err("randomness request is too large".into());
    }
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(alg: *mut core::ffi::c_void, buf: *mut u8, len: u32, flags: u32) -> i32;
    }
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, 2) };
    if status != 0 {
        return Err("the operating system did not supply randomness".into());
    }
    Ok(())
}

#[cfg(unix)]
fn fill_random(buf: &mut [u8]) -> Result<(), String> {
    let mut file = std::fs::File::open("/dev/urandom")
        .map_err(|_| "the operating system did not supply randomness".to_string())?;
    file.read_exact(buf)
        .map_err(|_| "the operating system randomness was incomplete".to_string())
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = if i < 20 {
                ((b & c) | (!b & d), 0x5a82_7999)
            } else if i < 40 {
                (b ^ c ^ d, 0x6ed9_eba1)
            } else if i < 60 {
                ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc)
            } else {
                (b ^ c ^ d, 0xca62_c1d6)
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for i in 0..5 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut a = h;
        for i in 0..64 {
            let s1 = a[4].rotate_right(6) ^ a[4].rotate_right(11) ^ a[4].rotate_right(25);
            let ch = (a[4] & a[5]) ^ (!a[4] & a[6]);
            let t1 = a[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a[0].rotate_right(2) ^ a[0].rotate_right(13) ^ a[0].rotate_right(22);
            let maj = (a[0] & a[1]) ^ (a[0] & a[2]) ^ (a[1] & a[2]);
            let t2 = s0.wrapping_add(maj);
            a[7] = a[6];
            a[6] = a[5];
            a[5] = a[4];
            a[4] = a[3].wrapping_add(t1);
            a[3] = a[2];
            a[2] = a[1];
            a[1] = a[0];
            a[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(a[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

pub fn sha512(data: &[u8]) -> [u8; 64] {
    let mut h: [u64; 8] = [
        0x6a09_e667_f3bc_c908,
        0xbb67_ae85_84ca_a73b,
        0x3c6e_f372_fe94_f82b,
        0xa54f_f53a_5f1d_36f1,
        0x510e_527f_ade6_82d1,
        0x9b05_688c_2b3e_6c1f,
        0x1f83_d9ab_fb41_bd6b,
        0x5be0_cd19_137e_2179,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u128).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks_exact(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            w[i] = u64::from_be_bytes(chunk[i * 8..i * 8 + 8].try_into().unwrap());
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut a = h;
        for i in 0..80 {
            let s1 = a[4].rotate_right(14) ^ a[4].rotate_right(18) ^ a[4].rotate_right(41);
            let ch = (a[4] & a[5]) ^ (!a[4] & a[6]);
            let t1 = a[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K512[i]).wrapping_add(w[i]);
            let s0 = a[0].rotate_right(28) ^ a[0].rotate_right(34) ^ a[0].rotate_right(39);
            let maj = (a[0] & a[1]) ^ (a[0] & a[2]) ^ (a[1] & a[2]);
            let t2 = s0.wrapping_add(maj);
            a[7] = a[6];
            a[6] = a[5];
            a[5] = a[4];
            a[4] = a[3].wrapping_add(t1);
            a[3] = a[2];
            a[2] = a[1];
            a[1] = a[0];
            a[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(a[i]);
        }
    }
    let mut out = [0u8; 64];
    for i in 0..8 {
        out[i * 8..i * 8 + 8].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0u8; 64];
    let mut opad = [0u8; 64];
    for i in 0..64 {
        ipad[i] = block[i] ^ 0x36;
        opad[i] = block[i] ^ 0x5c;
    }
    let mut inner = ipad.to_vec();
    inner.extend_from_slice(msg);
    let digest = sha256(&inner);
    let mut outer = opad.to_vec();
    outer.extend_from_slice(&digest);
    sha256(&outer)
}

pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    if salt.is_empty() {
        hmac_sha256(&[0u8; 32], ikm)
    } else {
        hmac_sha256(salt, ikm)
    }
}

pub fn hkdf_expand(prk: &[u8], info: &[u8], length: usize) -> Result<Vec<u8>, String> {
    if length > HKDF_MAX_BLOCKS * HASH_LEN {
        return Err("hkdf output is longer than 255 hash blocks".into());
    }
    let mut out = Vec::with_capacity(length);
    let mut prev = Vec::new();
    let mut counter = 1u32;
    while out.len() < length {
        if counter > HKDF_MAX_BLOCKS as u32 {
            return Err("hkdf output is longer than 255 hash blocks".into());
        }
        let mut msg = prev.clone();
        msg.extend_from_slice(info);
        msg.push(counter as u8);
        prev = hmac_sha256(prk, &msg).to_vec();
        out.extend_from_slice(&prev);
        counter += 1;
    }
    out.truncate(length);
    Ok(out)
}

pub fn hkdf_expand_label(secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Result<Vec<u8>, String> {
    if length > 65535 {
        return Err("label length does not fit the protocol field".into());
    }
    let mut full = b"tls13 ".to_vec();
    full.extend_from_slice(label);
    if full.len() > 255 || context.len() > 255 {
        return Err("label or context does not fit the protocol field".into());
    }
    let mut info = Vec::new();
    info.extend_from_slice(&(length as u16).to_be_bytes());
    info.push(full.len() as u8);
    info.extend_from_slice(&full);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    hkdf_expand(secret, &info, length)
}

pub fn derive_secret(secret: &[u8], label: &[u8], transcript_hash: &[u8]) -> Result<Vec<u8>, String> {
    hkdf_expand_label(secret, label, transcript_hash, 32)
}

/// Exported keying material. The label is taken as raw bytes, including a trailing zero when the peer counts it.
pub fn export_keying_material(exporter_master: &[u8], label: &[u8], length: usize) -> Result<Vec<u8>, String> {
    let empty = sha256(b"");
    let derived = hkdf_expand_label(exporter_master, label, &empty, 32)?;
    hkdf_expand_label(&derived, b"exporter", &empty, length)
}

fn tags_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..left.len() {
        diff |= left[i] ^ right[i];
    }
    diff == 0
}

const fn gf8_const(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0u8;
    let mut round = 0;
    while round < 8 {
        if right & 1 != 0 {
            product ^= left;
        }
        let carry = left & 0x80;
        left = left.wrapping_shl(1);
        if carry != 0 {
            left ^= 0x1b;
        }
        right >>= 1;
        round += 1;
    }
    product
}

const fn sbox_const(value: u8) -> u8 {
    if value == 0 {
        return 0x63;
    }
    let mut power = value;
    let mut inverse = 1u8;
    let mut exponent = 0xfeu8;
    while exponent != 0 {
        if exponent & 1 != 0 {
            inverse = gf8_const(inverse, power);
        }
        power = gf8_const(power, power);
        exponent >>= 1;
    }
    let mut rotated = inverse;
    let mut result = inverse;
    let mut turn = 0;
    while turn < 4 {
        rotated = rotated.rotate_left(1);
        result ^= rotated;
        turn += 1;
    }
    result ^ 0x63
}

const fn sbox_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut index = 0;
    while index < 256 {
        table[index] = sbox_const(index as u8);
        index += 1;
    }
    table
}

const SBOX: [u8; 256] = sbox_table();

fn ct_eq_mask(left: u8, right: u8) -> u8 {
    let mut mixed = left ^ right;
    mixed |= mixed >> 4;
    mixed |= mixed >> 2;
    mixed |= mixed >> 1;
    (mixed & 1).wrapping_sub(1)
}

fn sbox_at(value: u8) -> u8 {
    let mut out = 0u8;
    let mut index = 0usize;
    while index < 256 {
        out |= SBOX[index] & ct_eq_mask(value, index as u8);
        index += 1;
    }
    out
}

#[allow(dead_code)]
fn gf8(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0u8;
    for _ in 0..8 {
        let bit_mask = (right & 1).wrapping_neg();
        product ^= left & bit_mask;
        let carry_mask = ((left & 0x80) >> 7).wrapping_neg();
        left = left.wrapping_shl(1) ^ (0x1b & carry_mask);
        right >>= 1;
    }
    product
}

fn aes_encrypt_block(key: &[u8], block: &[u8; 16]) -> Result<[u8; 16], String> {
    if !matches!(key.len(), 16 | 24 | 32) {
        return Err("cipher key length is not supported".into());
    }
    let expanded = expand_key(key);
    let rounds = match key.len() {
        16 => 10,
        24 => 12,
        _ => 14,
    };
    let mut state = *block;
    add_round(&mut state, &expanded[..16]);
    for round in 1..rounds {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        add_round(&mut state, &expanded[round * 16..(round + 1) * 16]);
    }
    sub_bytes(&mut state);
    shift_rows(&mut state);
    add_round(&mut state, &expanded[rounds * 16..(rounds + 1) * 16]);
    Ok(state)
}

fn expand_key(key: &[u8]) -> Vec<u8> {
    let words_in = key.len() / 4;
    let rounds = match words_in {
        4 => 10,
        6 => 12,
        _ => 14,
    };
    let total = 4 * (rounds + 1);
    let mut words = vec![0u32; total];
    for i in 0..words_in {
        words[i] = u32::from_be_bytes(key[i * 4..i * 4 + 4].try_into().unwrap());
    }
    for i in words_in..total {
        let mut temp = words[i - 1];
        if i % words_in == 0 {
            temp = sub_word(temp.rotate_left(8)) ^ ((RCON[i / words_in - 1] as u32) << 24);
        } else if words_in > 6 && i % words_in == 4 {
            temp = sub_word(temp);
        }
        words[i] = words[i - words_in] ^ temp;
    }
    let mut out = Vec::with_capacity(total * 4);
    for word in words {
        out.extend_from_slice(&word.to_be_bytes());
    }
    out
}

fn sub_word(value: u32) -> u32 {
    let bytes = value.to_be_bytes();
    u32::from_be_bytes([
        sbox_at(bytes[0]),
        sbox_at(bytes[1]),
        sbox_at(bytes[2]),
        sbox_at(bytes[3]),
    ])
}

fn sub_bytes(state: &mut [u8; 16]) {
    for byte in state.iter_mut() {
        *byte = sbox_at(*byte);
    }
}

fn add_round(state: &mut [u8; 16], key: &[u8]) {
    for i in 0..16 {
        state[i] ^= key[i];
    }
}

fn shift_rows(state: &mut [u8; 16]) {
    let saved = *state;
    state[1] = saved[5];
    state[5] = saved[9];
    state[9] = saved[13];
    state[13] = saved[1];
    state[2] = saved[10];
    state[6] = saved[14];
    state[10] = saved[2];
    state[14] = saved[6];
    state[3] = saved[15];
    state[7] = saved[3];
    state[11] = saved[7];
    state[15] = saved[11];
}

fn xtime(value: u8) -> u8 {
    let mask = ((value & 0x80) >> 7).wrapping_neg();
    value.wrapping_shl(1) ^ (0x1b & mask)
}

fn mix_columns(state: &mut [u8; 16]) {
    for column in 0..4 {
        let i = column * 4;
        let a = [state[i], state[i + 1], state[i + 2], state[i + 3]];
        state[i] = xtime(a[0]) ^ xtime(a[1]) ^ a[1] ^ a[2] ^ a[3];
        state[i + 1] = a[0] ^ xtime(a[1]) ^ xtime(a[2]) ^ a[2] ^ a[3];
        state[i + 2] = a[0] ^ a[1] ^ xtime(a[2]) ^ xtime(a[3]) ^ a[3];
        state[i + 3] = xtime(a[0]) ^ a[0] ^ a[1] ^ a[2] ^ xtime(a[3]);
    }
}

pub fn aes_gcm_seal(key: &[u8], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String> {
    let (body, tag) = aes_gcm(key, nonce, aad, plain)?;
    let mut out = body;
    out.extend_from_slice(&tag);
    Ok(out)
}

pub fn aes_gcm_open(key: &[u8], nonce: &[u8; 12], aad: &[u8], boxed: &[u8]) -> Result<Vec<u8>, String> {
    if boxed.len() < 16 {
        return Err("authenticated ciphertext is truncated".into());
    }
    let (body, tag) = boxed.split_at(boxed.len() - 16);
    let mut tag_bytes = [0u8; 16];
    tag_bytes.copy_from_slice(tag);
    aes_gcm_decrypt(key, nonce, aad, body, &tag_bytes)
}

fn aes_gcm(key: &[u8], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Result<(Vec<u8>, [u8; 16]), String> {
    if aad.len() > MAX_AEAD_BYTES || plain.len() > MAX_AEAD_BYTES {
        return Err("cipher input exceeds the size budget".into());
    }
    let hash = aes_encrypt_block(key, &[0u8; 16])?;
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(nonce);
    j0[15] = 1;
    let mut counter = j0;
    inc32(&mut counter);
    let mut cipher = vec![0u8; plain.len()];
    let mut offset = 0;
    while offset < plain.len() {
        let stream = aes_encrypt_block(key, &counter)?;
        let n = (plain.len() - offset).min(16);
        for i in 0..n {
            cipher[offset + i] = plain[offset + i] ^ stream[i];
        }
        offset += n;
        inc32(&mut counter);
    }
    let digest = ghash(&hash, aad, &cipher);
    let mask = aes_encrypt_block(key, &j0)?;
    let mut tag = [0u8; 16];
    for i in 0..16 {
        tag[i] = digest[i] ^ mask[i];
    }
    Ok((cipher, tag))
}

fn aes_gcm_decrypt(key: &[u8], nonce: &[u8; 12], aad: &[u8], cipher: &[u8], tag: &[u8; 16]) -> Result<Vec<u8>, String> {
    if aad.len() > MAX_AEAD_BYTES || cipher.len() > MAX_AEAD_BYTES {
        return Err("cipher input exceeds the size budget".into());
    }
    let hash = aes_encrypt_block(key, &[0u8; 16])?;
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(nonce);
    j0[15] = 1;
    let digest = ghash(&hash, aad, cipher);
    let mask = aes_encrypt_block(key, &j0)?;
    let mut expect = [0u8; 16];
    for i in 0..16 {
        expect[i] = digest[i] ^ mask[i];
    }
    if !tags_equal(&expect, tag) {
        return Err("authentication tag did not match".into());
    }
    let mut counter = j0;
    inc32(&mut counter);
    let mut plain = vec![0u8; cipher.len()];
    let mut offset = 0;
    while offset < cipher.len() {
        let stream = aes_encrypt_block(key, &counter)?;
        let n = (cipher.len() - offset).min(16);
        for i in 0..n {
            plain[offset + i] = cipher[offset + i] ^ stream[i];
        }
        offset += n;
        inc32(&mut counter);
    }
    Ok(plain)
}

fn inc32(block: &mut [u8; 16]) {
    let n = u32::from_be_bytes(block[12..16].try_into().unwrap()).wrapping_add(1);
    block[12..16].copy_from_slice(&n.to_be_bytes());
}

fn ghash(h: &[u8; 16], aad: &[u8], cipher: &[u8]) -> [u8; 16] {
    let mut y = [0u8; 16];
    ghash_blocks(&mut y, h, aad);
    ghash_blocks(&mut y, h, cipher);
    let mut lengths = [0u8; 16];
    lengths[..8].copy_from_slice(&((aad.len() as u64).wrapping_mul(8)).to_be_bytes());
    lengths[8..].copy_from_slice(&((cipher.len() as u64).wrapping_mul(8)).to_be_bytes());
    for i in 0..16 {
        y[i] ^= lengths[i];
    }
    gf_mul(&y, h)
}

fn ghash_blocks(y: &mut [u8; 16], h: &[u8; 16], data: &[u8]) {
    let mut i = 0;
    while i < data.len() {
        let n = (data.len() - i).min(16);
        for b in 0..n {
            y[b] ^= data[i + b];
        }
        *y = gf_mul(y, h);
        i += n;
    }
}

fn gf_mul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *y;
    for bit in 0..128 {
        let mask = ((x[bit / 8] >> (7 - (bit % 8))) & 1).wrapping_neg();
        for i in 0..16 {
            z[i] ^= v[i] & mask;
        }
        let lsb_mask = (v[15] & 1).wrapping_neg();
        let mut carry = 0u8;
        for i in 0..16 {
            let next = (v[i] & 1) << 7;
            v[i] = (v[i] >> 1) | carry;
            carry = next;
        }
        v[0] ^= 0xe1 & lsb_mask;
    }
    z
}

#[derive(Clone, Copy)]
struct Fe(pub [u64; 4]);

fn limb_cmp(left: &[u64; 4], right: &[u64; 4]) -> Ordering {
    for i in (0..4).rev() {
        match left[i].cmp(&right[i]) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

#[allow(dead_code)]
fn fe_ge_p(value: &Fe) -> bool {
    limb_cmp(&value.0, &P) != Ordering::Less
}

#[allow(dead_code)]
fn sub_limbs(left: &[u64; 4], right: &[u64; 4]) -> [u64; 4] {
    let mut out = [0u64; 4];
    let mut borrow = 0i128;
    for i in 0..4 {
        let diff = left[i] as i128 - right[i] as i128 - borrow;
        if diff < 0 {
            out[i] = (diff + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            out[i] = diff as u64;
            borrow = 0;
        }
    }
    out
}

fn limb_add(left: &[u64; 4], right: &[u64; 4]) -> ([u64; 4], u64) {
    let mut out = [0u64; 4];
    let mut carry = 0u128;
    for i in 0..4 {
        let sum = left[i] as u128 + right[i] as u128 + carry;
        out[i] = sum as u64;
        carry = sum >> 64;
    }
    (out, carry as u64)
}

fn limb_sub(left: &[u64; 4], right: &[u64; 4]) -> ([u64; 4], u64) {
    let mut out = [0u64; 4];
    let mut borrow = 0u64;
    for i in 0..4 {
        let lhs = left[i] as u128;
        let rhs = (right[i] as u128) + u128::from(borrow);
        let diff = lhs.wrapping_sub(rhs);
        out[i] = diff as u64;
        borrow = (diff >> 127) as u64;
    }
    (out, borrow)
}

fn limb_select(mask: u64, left: &[u64; 4], right: &[u64; 4]) -> [u64; 4] {
    let bits = 0u64.wrapping_sub(mask & 1);
    let mut out = [0u64; 4];
    for i in 0..4 {
        out[i] = (left[i] & bits) | (right[i] & !bits);
    }
    out
}

fn fold_small(mut limbs: [u64; 4], mut extra: u128) -> ([u64; 4], u128) {
    for i in 0..4 {
        let sum = u128::from(limbs[i]) + extra;
        limbs[i] = sum as u64;
        extra = sum >> 64;
    }
    (limbs, extra)
}

/// Subtract p when the residue is at least p. Inputs below 3p become canonical.
fn ct_sub_p(value: [u64; 4]) -> [u64; 4] {
    let (reduced, borrow) = limb_sub(&value, &P);
    limb_select(borrow, &value, &reduced)
}

fn fe_add(left: &Fe, right: &Fe) -> Fe {
    let (sum, carry) = limb_add(&left.0, &right.0);
    let (sum, carry) = fold_small(sum, u128::from(carry) * 38);
    let (sum, _) = fold_small(sum, carry * 38);
    Fe(ct_sub_p(ct_sub_p(sum)))
}

fn fe_sub(left: &Fe, right: &Fe) -> Fe {
    let (diff, borrow) = limb_sub(&left.0, &right.0);
    let mask = 0u64.wrapping_sub(borrow);
    let mut addend = [0u64; 4];
    for i in 0..4 {
        addend[i] = P[i] & mask;
    }
    let (sum, _) = limb_add(&diff, &addend);
    Fe(ct_sub_p(ct_sub_p(sum)))
}

fn fe_neg(value: &Fe) -> Fe {
    fe_sub(&Fe([0, 0, 0, 0]), value)
}

fn fe_from_wide(wide: [u64; 8]) -> Fe {
    let mut limb = [0u64; 4];
    let mut carry = 0u128;
    for i in 0..4 {
        let sum = u128::from(wide[i]) + u128::from(wide[i + 4]) * 38 + carry;
        limb[i] = sum as u64;
        carry = sum >> 64;
    }
    let (limb, carry) = fold_small(limb, carry * 38);
    let (limb, _) = fold_small(limb, carry * 38);
    Fe(ct_sub_p(ct_sub_p(limb)))
}

fn fe_mul(left: &Fe, right: &Fe) -> Fe {
    // Four extra limbs keep every carry step in range. The trip count is the limb count, not the words.
    let mut acc = [0u64; 12];
    for i in 0..4 {
        let mut carry = 0u128;
        for j in 0..4 {
            let sum = u128::from(acc[i + j]) + u128::from(left.0[i]) * u128::from(right.0[j]) + carry;
            acc[i + j] = sum as u64;
            carry = sum >> 64;
        }
        for step in 0..4 {
            let sum = u128::from(acc[i + 4 + step]) + carry;
            acc[i + 4 + step] = sum as u64;
            carry = sum >> 64;
        }
    }
    let mut wide = [0u64; 8];
    wide.copy_from_slice(&acc[..8]);
    fe_from_wide(wide)
}

fn fe_sqr(value: &Fe) -> Fe {
    fe_mul(value, value)
}

fn fe_pow(base: &Fe, exp: &[u8; 32]) -> Fe {
    let mut acc = fe_one();
    for byte in exp.iter().rev() {
        for bit in (0..8).rev() {
            acc = fe_sqr(&acc);
            if (byte >> bit) & 1 == 1 {
                acc = fe_mul(&acc, base);
            }
        }
    }
    acc
}

fn fe_inv(value: &Fe) -> Fe {
    let mut exp = P;
    exp[0] = exp[0].wrapping_sub(2);
    let mut bytes = [0u8; 32];
    for i in 0..4 {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&exp[i].to_le_bytes());
    }
    fe_pow(value, &bytes)
}

fn fe_one() -> Fe {
    Fe([1, 0, 0, 0])
}

fn fe_from_u64(value: u64) -> Fe {
    Fe([value, 0, 0, 0])
}

fn fe_to_bytes(value: &Fe) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..4 {
        out[i * 8..i * 8 + 8].copy_from_slice(&value.0[i].to_le_bytes());
    }
    out
}

fn fe_from_bytes(bytes: &[u8; 32]) -> Result<Fe, String> {
    let mut limbs = [0u64; 4];
    for i in 0..4 {
        limbs[i] = u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
    }
    if limb_cmp(&limbs, &P) != Ordering::Less {
        return Err("field element is outside the prime field".into());
    }
    Ok(Fe(limbs))
}

/// Accept a 32-byte field encoding and reduce it modulo p.
/// The top bit is cleared by the caller before this runs. Values below 2^256 need at most two subtractions.
fn fe_accept(bytes: &[u8; 32]) -> Fe {
    let mut limbs = [0u64; 4];
    for i in 0..4 {
        limbs[i] = u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
    }
    Fe(ct_sub_p(ct_sub_p(limbs)))
}

fn fe_is_odd(value: &Fe) -> bool {
    value.0[0] & 1 == 1
}

fn fe_cswap(swap: u64, left: &mut Fe, right: &mut Fe) {
    let mask = 0u64.wrapping_sub(swap);
    for i in 0..4 {
        let mixed = mask & (left.0[i] ^ right.0[i]);
        left.0[i] ^= mixed;
        right.0[i] ^= mixed;
    }
}

pub fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let mut clamped = *scalar;
    clamped[0] &= 248;
    clamped[31] &= 127;
    clamped[31] |= 64;
    let mut u = *point;
    u[31] &= 127;
    let x1 = fe_accept(&u);
    let mut x2 = fe_one();
    let mut z2 = Fe([0, 0, 0, 0]);
    let mut x3 = x1;
    let mut z3 = fe_one();
    let mut swap = 0u64;
    let a24 = fe_from_u64(121665);
    for t in (0..255).rev() {
        let kt = ((clamped[t / 8] >> (t % 8)) & 1) as u64;
        swap ^= kt;
        fe_cswap(swap, &mut x2, &mut x3);
        fe_cswap(swap, &mut z2, &mut z3);
        swap = kt;
        let a = fe_add(&x2, &z2);
        let aa = fe_sqr(&a);
        let b = fe_sub(&x2, &z2);
        let bb = fe_sqr(&b);
        let e = fe_sub(&aa, &bb);
        let c = fe_add(&x3, &z3);
        let d = fe_sub(&x3, &z3);
        let da = fe_mul(&d, &a);
        let cb = fe_mul(&c, &b);
        x3 = fe_sqr(&fe_add(&da, &cb));
        z3 = fe_mul(&x1, &fe_sqr(&fe_sub(&da, &cb)));
        x2 = fe_mul(&aa, &bb);
        z2 = fe_mul(&e, &fe_add(&aa, &fe_mul(&a24, &e)));
    }
    fe_cswap(swap, &mut x2, &mut x3);
    fe_cswap(swap, &mut z2, &mut z3);
    fe_to_bytes(&fe_mul(&x2, &fe_inv(&z2)))
}

#[derive(Clone)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

fn point_identity() -> Point {
    Point { x: Fe([0, 0, 0, 0]), y: fe_one(), z: fe_one(), t: Fe([0, 0, 0, 0]) }
}

struct Curve {
    d: Fe,
    d2: Fe,
    base: Point,
}

fn curve() -> Result<&'static Curve, String> {
    use std::sync::OnceLock;
    static CURVE: OnceLock<Curve> = OnceLock::new();
    if let Some(existing) = CURVE.get() {
        return Ok(existing);
    }
    let built = build_curve()?;
    let _ = CURVE.set(built);
    Ok(CURVE.get().unwrap())
}

fn build_curve() -> Result<Curve, String> {
    let d_num = fe_from_u64(121665);
    let d_den = fe_inv(&fe_from_u64(121666));
    let d = fe_neg(&fe_mul(&d_num, &d_den));
    let d2 = fe_add(&d, &d);
    let base_bytes = [0x58u8, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66];
    let base = decompress_with(d, &base_bytes)?;
    Ok(Curve { d, d2, base })
}

fn point_add(curve_d2: Fe, left: &Point, right: &Point) -> Point {
    let a = fe_mul(&fe_sub(&left.y, &left.x), &fe_sub(&right.y, &right.x));
    let b = fe_mul(&fe_add(&left.y, &left.x), &fe_add(&right.y, &right.x));
    let c = fe_mul(&fe_mul(&left.t, &curve_d2), &right.t);
    let d = fe_mul(&fe_add(&left.z, &left.z), &right.z);
    let e = fe_sub(&b, &a);
    let f = fe_sub(&d, &c);
    let g = fe_add(&d, &c);
    let h = fe_add(&b, &a);
    Point {
        x: fe_mul(&e, &f),
        y: fe_mul(&g, &h),
        t: fe_mul(&e, &h),
        z: fe_mul(&f, &g),
    }
}

fn point_double(left: &Point) -> Point {
    let a = fe_sqr(&left.x);
    let b = fe_sqr(&left.y);
    let c = fe_add(&fe_sqr(&left.z), &fe_sqr(&left.z));
    let d = fe_neg(&a);
    let e = fe_sub(&fe_sub(&fe_sqr(&fe_add(&left.x, &left.y)), &a), &b);
    let g = fe_add(&d, &b);
    let f = fe_sub(&g, &c);
    let h = fe_sub(&d, &b);
    Point {
        x: fe_mul(&e, &f),
        y: fe_mul(&g, &h),
        t: fe_mul(&e, &h),
        z: fe_mul(&f, &g),
    }
}

fn fe_select(mask: u64, left: &Fe, right: &Fe) -> Fe {
    let bits = 0u64.wrapping_sub(mask & 1);
    let mut out = [0u64; 4];
    for i in 0..4 {
        out[i] = (left.0[i] & bits) | (right.0[i] & !bits);
    }
    Fe(out)
}

fn point_select(mask: u64, left: &Point, right: &Point) -> Point {
    Point {
        x: fe_select(mask, &left.x, &right.x),
        y: fe_select(mask, &left.y, &right.y),
        z: fe_select(mask, &left.z, &right.z),
        t: fe_select(mask, &left.t, &right.t),
    }
}

fn point_mul(value: &Point, scalar: &[u8; 32]) -> Result<Point, String> {
    let curve = curve()?;
    let mut acc = point_identity();
    for byte_index in (0..32).rev() {
        let byte = scalar[byte_index];
        for bit in (0..8).rev() {
            acc = point_double(&acc);
            let added = point_add(curve.d2, &acc, value);
            let mask = ((byte >> bit) & 1) as u64;
            acc = point_select(mask, &added, &acc);
        }
    }
    Ok(acc)
}

fn point_neg(value: &Point) -> Point {
    Point { x: fe_neg(&value.x), y: value.y, z: value.z, t: fe_neg(&value.t) }
}

fn compress(value: &Point) -> [u8; 32] {
    let inv = fe_inv(&value.z);
    let x = fe_mul(&value.x, &inv);
    let y = fe_mul(&value.y, &inv);
    let mut bytes = fe_to_bytes(&y);
    bytes[31] |= ((x.0[0] as u8) & 1) << 7;
    bytes
}

fn decompress(bytes: &[u8; 32]) -> Result<Point, String> {
    let curve = curve()?;
    decompress_with(curve.d, bytes)
}

fn decompress_with(d: Fe, bytes: &[u8; 32]) -> Result<Point, String> {
    let sign = (bytes[31] >> 7) & 1;
    let mut y_bytes = *bytes;
    y_bytes[31] &= 0x7f;
    let y = fe_from_bytes(&y_bytes)?;
    let y2 = fe_sqr(&y);
    let u = fe_sub(&y2, &fe_one());
    let v = fe_add(&fe_mul(&d, &y2), &fe_one());
    let x = fe_sqrt_ratio(&u, &v)?;
    let mut x = x;
    if x.0 == [0, 0, 0, 0] && sign == 1 {
        return Err("point sign does not match".into());
    }
    if (fe_is_odd(&x) as u8) != sign {
        x = fe_neg(&x);
    }
    let t = fe_mul(&x, &y);
    Ok(Point { x, y, z: fe_one(), t })
}

fn fe_sqrt_ratio(u: &Fe, v: &Fe) -> Result<Fe, String> {
    let v3 = fe_mul(&fe_sqr(v), v);
    let v7 = fe_mul(&fe_sqr(&v3), v);
    let mut exp = [0xffu8; 32];
    exp[0] = 0xfd;
    exp[31] = 0x0f;
    let pow = fe_pow(&fe_mul(u, &v7), &exp);
    let mut x = fe_mul(&fe_mul(u, &v3), &pow);
    let x2 = fe_mul(u, &fe_inv(v));
    if fe_sqr(&x).0 != x2.0 {
        let i = sqrt_minus_one();
        x = fe_mul(&x, &i);
    }
    if fe_sqr(&x).0 != x2.0 {
        return Err("value is not a square on the curve".into());
    }
    Ok(x)
}

fn sqrt_minus_one() -> Fe {
    use std::sync::OnceLock;
    static I: OnceLock<Fe> = OnceLock::new();
    *I.get_or_init(|| {
        let mut exp = [0xffu8; 32];
        exp[0] = 0xfb;
        exp[31] = 0x1f;
        fe_pow(&fe_from_u64(2), &exp)
    })
}

const ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

const MASK_M: [u8; 32] = [
    0x5a, 0xda, 0x7e, 0x4b, 0xf6, 0xdd, 0xd9, 0xad, 0xb6, 0x62, 0x6d, 0x32, 0x13, 0x1c, 0x6b, 0x5c, 0x51, 0xa1, 0xe3, 0x47, 0xa3, 0x47, 0x8f, 0x53, 0xcf, 0xcf, 0x44, 0x1b, 0x88, 0xee, 0xd1, 0x2e,
];

const MASK_N: [u8; 32] = [
    0x10, 0xe3, 0xdf, 0x0a, 0xe3, 0x7d, 0x8e, 0x7a, 0x99, 0xb5, 0xfe, 0x74, 0xb4, 0x46, 0x72, 0x10, 0x3d, 0xbd, 0xdc, 0xbd, 0x06, 0xaf, 0x68, 0x0d, 0x71, 0x32, 0x9a, 0x11, 0x69, 0x3b, 0xc7, 0x78,
];

fn order_limbs() -> [u32; 8] {
    let mut limbs = [0u32; 8];
    for i in 0..8 {
        let o = i * 4;
        limbs[i] = u32::from_le_bytes([ORDER[o], ORDER[o + 1], ORDER[o + 2], ORDER[o + 3]]);
    }
    limbs
}

/// Reduce a little-endian integer modulo the curve order.
/// Every input bit is shifted in. The value does not choose a branch.
fn sc_reduce(wide: &[u8]) -> [u8; 32] {
    let mut le = [0u8; 64];
    let n = wide.len().min(le.len());
    le[..n].copy_from_slice(&wide[..n]);
    let mut be = [0u8; 64];
    for i in 0..64 {
        be[i] = le[63 - i];
    }
    let modulus = order_limbs();
    let rem = ct_mod_bytes(&be, &modulus);
    let mut out = [0u8; 32];
    for i in 0..8 {
        let limb = rem.get(i).copied().unwrap_or(0);
        out[i * 4..i * 4 + 4].copy_from_slice(&limb.to_le_bytes());
    }
    out
}

fn left_shift_3(value: &mut [u8; 32]) {
    let mut carry = 0u8;
    for byte in value.iter_mut() {
        let next = *byte >> 5;
        *byte = (*byte << 3) | carry;
        carry = next;
    }
}

fn password_scalar(hash: &[u8; 64]) -> [u8; 32] {
    let mut scalar = sc_reduce(hash);
    let mut order = ORDER;
    for bit in 0..3u32 {
        let mut added = scalar;
        add_bytes(&mut added, &order);
        let select = 0u8.wrapping_sub((scalar[0] >> bit) & 1);
        for i in 0..32 {
            scalar[i] = (added[i] & select) | (scalar[i] & !select);
        }
        let current = order;
        add_bytes(&mut order, &current);
    }
    scalar
}

fn add_bytes(left: &mut [u8; 32], right: &[u8; 32]) {
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = u16::from(left[i]) + u16::from(right[i]) + carry;
        left[i] = sum as u8;
        carry = sum >> 8;
    }
}

pub struct Spake2 {
    role_alice: bool,
    private_key: [u8; 32],
    password_hash: [u8; 64],
    password_scalar: [u8; 32],
    message: [u8; 32],
}

impl Spake2 {
    pub fn alice(password: &[u8]) -> Result<Self, String> {
        Self::start(true, password)
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bob(password: &[u8]) -> Result<Self, String> {
        Self::start(false, password)
    }
    fn start(alice: bool, password: &[u8]) -> Result<Self, String> {
        let mut wide = [0u8; 64];
        wide.copy_from_slice(&random_bytes(64)?);
        let mut private_key = sc_reduce(&wide);
        left_shift_3(&mut private_key);
        let password_hash = sha512(password);
        let password_scalar = password_scalar(&password_hash);
        let mask_bytes = if alice { MASK_M } else { MASK_N };
        let mask = decompress(&mask_bytes)?;
        let curve = curve()?;
        let base = point_mul(&curve.base, &private_key)?;
        let hidden = point_mul(&mask, &password_scalar)?;
        let combined = point_add(curve.d2, &base, &hidden);
        Ok(Spake2 {
            role_alice: alice,
            private_key,
            password_hash,
            password_scalar,
            message: compress(&combined),
        })
    }
    pub fn message(&self) -> &[u8; 32] {
        &self.message
    }
    pub fn finish(self, their: &[u8]) -> Result<[u8; 64], String> {
        if their.len() != 32 {
            return Err("pairing message must be 32 bytes".into());
        }
        let mut their_msg = [0u8; 32];
        their_msg.copy_from_slice(their);
        let masked = decompress(&their_msg)?;
        let peer = if self.role_alice { MASK_N } else { MASK_M };
        let peer_point = decompress(&peer)?;
        let hidden = point_mul(&peer_point, &self.password_scalar)?;
        let curve = curve()?;
        let unmasked = point_add(curve.d2, &masked, &point_neg(&hidden));
        let shared_point = point_mul(&unmasked, &self.private_key)?;
        let shared = compress(&shared_point);
        let alice_name: &[u8] = b"adb pair client\0";
        let bob_name: &[u8] = b"adb pair server\0";
        let (alice_msg, bob_msg) = if self.role_alice {
            (&self.message, &their_msg)
        } else {
            (&their_msg, &self.message)
        };
        let mut hash = Vec::new();
        for part in [alice_name, bob_name, alice_msg, bob_msg, &shared, &self.password_hash] {
            hash.extend_from_slice(&(part.len() as u64).to_le_bytes());
            hash.extend_from_slice(part);
        }
        Ok(sha512(&hash))
    }
}

pub struct PairCipher {
    key: [u8; 16],
    enc: u64,
    dec: u64,
}

impl PairCipher {
    pub fn new(material: &[u8]) -> Self {
        let expanded = hkdf_expand(
            &hkdf_extract(&[], material),
            b"adb pairing_auth aes-128-gcm key",
            16,
        );
        let mut bytes = [0u8; 16];
        if let Ok(key) = expanded {
            if key.len() == 16 {
                bytes.copy_from_slice(&key);
            }
        }
        PairCipher { key: bytes, enc: 0, dec: 0 }
    }
    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>, String> {
        if self.enc >= AES_GCM_RECORD_LIMIT {
            return Err("pairing key record limit reached".into());
        }
        let nonce = pair_nonce(self.enc);
        self.enc += 1;
        aes_gcm_seal(&self.key, &nonce, &[], plain)
    }
    pub fn open(&mut self, boxed: &[u8]) -> Result<Vec<u8>, String> {
        if self.dec >= AES_GCM_RECORD_LIMIT {
            return Err("pairing key record limit reached".into());
        }
        let nonce = pair_nonce(self.dec);
        let plain = aes_gcm_open(&self.key, &nonce, &[], boxed)?;
        self.dec += 1;
        Ok(plain)
    }
}

fn pair_nonce(counter: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..8].copy_from_slice(&counter.to_le_bytes());
    nonce
}

#[derive(Clone, Debug)]
struct Nat {
    w: Vec<u32>,
}

impl Nat {
    fn zero() -> Self {
        Nat { w: Vec::new() }
    }
    fn is_zero(&self) -> bool {
        self.w.is_empty()
    }
    fn from_u32(value: u32) -> Self {
        if value == 0 {
            Self::zero()
        } else {
            Nat { w: vec![value] }
        }
    }
    fn norm(&mut self) {
        while self.w.last() == Some(&0) {
            self.w.pop();
        }
    }
    fn from_be(bytes: &[u8]) -> Self {
        let mut w = Vec::new();
        let mut end = bytes.len();
        while end > 0 {
            let start = end.saturating_sub(4);
            let mut value = 0u32;
            for byte in &bytes[start..end] {
                value = (value << 8) | u32::from(*byte);
            }
            w.push(value);
            end = start;
        }
        let mut n = Nat { w };
        n.norm();
        n
    }
    fn from_le(bytes: &[u8]) -> Self {
        let mut reversed = bytes.to_vec();
        reversed.reverse();
        Self::from_be(&reversed)
    }
    fn to_be(&self) -> Vec<u8> {
        if self.w.is_empty() {
            return vec![0];
        }
        let mut out = Vec::new();
        for (index, word) in self.w.iter().rev().enumerate() {
            let bytes = word.to_be_bytes();
            if index == 0 {
                let start = bytes.iter().position(|b| *b != 0).unwrap_or(3);
                out.extend_from_slice(&bytes[start..]);
            } else {
                out.extend_from_slice(&bytes);
            }
        }
        out
    }
    fn to_be_fixed(&self, len: usize) -> Vec<u8> {
        let raw = self.to_be();
        if raw == [0] && self.is_zero() {
            return vec![0u8; len];
        }
        if raw.len() >= len {
            raw[raw.len() - len..].to_vec()
        } else {
            let mut out = vec![0u8; len - raw.len()];
            out.extend_from_slice(&raw);
            out
        }
    }
    fn to_le_fixed(&self, len: usize) -> Vec<u8> {
        let mut be = self.to_be_fixed(len);
        be.reverse();
        be
    }
    fn cmp(&self, other: &Nat) -> Ordering {
        match self.w.len().cmp(&other.w.len()) {
            Ordering::Equal => {}
            other => return other,
        }
        for i in (0..self.w.len()).rev() {
            match self.w[i].cmp(&other.w[i]) {
                Ordering::Equal => {}
                other => return other,
            }
        }
        Ordering::Equal
    }
    fn bitlen(&self) -> usize {
        match self.w.last() {
            None => 0,
            Some(top) => (self.w.len() - 1) * 32 + (32 - top.leading_zeros() as usize),
        }
    }
    fn bit(&self, index: usize) -> bool {
        let word = index / 32;
        if word >= self.w.len() {
            false
        } else {
            (self.w[word] >> (index % 32)) & 1 == 1
        }
    }
    fn mod_u32(&self, divisor: u32) -> u32 {
        if divisor == 0 {
            return 0;
        }
        let mut rem = 0u64;
        for word in self.w.iter().rev() {
            rem = ((rem << 32) | u64::from(*word)) % u64::from(divisor);
        }
        rem as u32
    }
    fn to_width(&self, width: usize) -> Vec<u32> {
        let mut limbs = vec![0u32; width];
        for (index, word) in self.w.iter().enumerate() {
            if index >= width {
                break;
            }
            limbs[index] = *word;
        }
        limbs
    }
    fn add(&self, other: &Nat) -> Nat {
        let n = self.w.len().max(other.w.len());
        let mut w = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let left = u64::from(*self.w.get(i).unwrap_or(&0));
            let right = u64::from(*other.w.get(i).unwrap_or(&0));
            let sum = left + right + carry;
            w.push(sum as u32);
            carry = sum >> 32;
        }
        if carry != 0 {
            w.push(carry as u32);
        }
        Nat { w }
    }
    fn sub(&self, other: &Nat) -> Nat {
        let mut w = Vec::with_capacity(self.w.len());
        let mut borrow = 0i64;
        for i in 0..self.w.len() {
            let left = self.w[i] as i64;
            let right = i64::from(*other.w.get(i).unwrap_or(&0));
            let diff = left - right - borrow;
            if diff < 0 {
                w.push((diff + (1i64 << 32)) as u32);
                borrow = 1;
            } else {
                w.push(diff as u32);
                borrow = 0;
            }
        }
        let mut n = Nat { w };
        n.norm();
        n
    }
    fn shl1(&self) -> Nat {
        let mut w = Vec::with_capacity(self.w.len() + 1);
        let mut carry = 0u32;
        for word in &self.w {
            w.push((word << 1) | carry);
            carry = word >> 31;
        }
        if carry != 0 {
            w.push(carry);
        }
        Nat { w }
    }
    fn shr1(&self) -> Nat {
        if self.w.is_empty() {
            return Nat::zero();
        }
        let mut w = vec![0u32; self.w.len()];
        let mut carry = 0u32;
        for i in (0..self.w.len()).rev() {
            w[i] = (self.w[i] >> 1) | carry;
            carry = (self.w[i] & 1) << 31;
        }
        let mut n = Nat { w };
        n.norm();
        n
    }
    fn mul(&self, other: &Nat) -> Nat {
        if self.is_zero() || other.is_zero() {
            return Nat::zero();
        }
        let mut acc = vec![0u64; self.w.len() + other.w.len() + 1];
        for i in 0..self.w.len() {
            let mut carry = 0u64;
            for j in 0..other.w.len() {
                let sum = acc[i + j] + u64::from(self.w[i]) * u64::from(other.w[j]) + carry;
                acc[i + j] = sum & 0xffff_ffff;
                carry = sum >> 32;
            }
            acc[i + other.w.len()] += carry;
        }
        let mut w = Vec::with_capacity(acc.len());
        let mut extra = 0u64;
        for value in acc {
            let sum = value + extra;
            w.push(sum as u32);
            extra = sum >> 32;
        }
        if extra != 0 {
            w.push(extra as u32);
        }
        let mut n = Nat { w };
        n.norm();
        n
    }
    fn divmod(&self, divisor: &Nat) -> Result<(Nat, Nat), String> {
        if divisor.is_zero() {
            return Err("division by zero".into());
        }
        if self.cmp(divisor) == Ordering::Less {
            return Ok((Nat::zero(), self.clone()));
        }
        let mut quot = Nat { w: vec![0; self.w.len()] };
        let mut rem = Nat::zero();
        for index in (0..self.bitlen()).rev() {
            rem = rem.shl1();
            if self.bit(index) {
                rem = rem.add(&Nat::from_u32(1));
            }
            if rem.cmp(divisor) != Ordering::Less {
                rem = rem.sub(divisor);
                let word = index / 32;
                if quot.w.len() <= word {
                    quot.w.resize(word + 1, 0);
                }
                quot.w[word] |= 1 << (index % 32);
            }
        }
        quot.norm();
        rem.norm();
        Ok((quot, rem))
    }
    fn modulo(&self, modulus: &Nat) -> Nat {
        self.divmod(modulus).map(|(_, rem)| rem).unwrap_or_else(|_| Nat::zero())
    }
    fn modpow(&self, exp: &Nat, modulus: &Nat) -> Result<Nat, String> {
        monty_pow(self, exp, modulus, false)
    }
    fn modpow_secret(&self, exp: &Nat, modulus: &Nat) -> Result<Nat, String> {
        monty_pow(self, exp, modulus, true)
    }
    fn inv_mod(&self, modulus: &Nat) -> Result<Nat, String> {
        let mut r0 = modulus.clone();
        let mut r1 = self.divmod(modulus)?.1;
        let mut s0 = Signed::zero();
        let mut s1 = Signed::pos(Nat::from_u32(1));
        while !r1.is_zero() {
            let (quot, next) = r0.divmod(&r1)?;
            r0 = r1;
            r1 = next;
            let product = s1.mul_nat(&quot);
            let next_s = s0.sub(&product);
            s0 = s1;
            s1 = next_s;
        }
        if r0.cmp(&Nat::from_u32(1)) != Ordering::Equal {
            return Err("value has no inverse".into());
        }
        Ok(s0.residue(modulus))
    }
}

fn limb_width(value: &Nat) -> usize {
    let bits = value.bitlen().max(1);
    (bits + 31) / 32
}

fn nat_from_limbs(limbs: &[u32]) -> Nat {
    let mut n = Nat { w: limbs.to_vec() };
    n.norm();
    n
}

fn sub_limbs_u32(left: &[u32], right: &[u32]) -> (Vec<u32>, u32) {
    let mut out = vec![0u32; left.len()];
    let mut borrow = 0u64;
    for i in 0..left.len() {
        let lhs = u64::from(left[i]);
        let rhs = u64::from(*right.get(i).unwrap_or(&0)) + borrow;
        let diff = lhs.wrapping_sub(rhs);
        out[i] = diff as u32;
        borrow = u64::from(lhs < rhs);
    }
    (out, borrow as u32)
}

fn select_limbs(mask: u32, left: &[u32], right: &[u32]) -> Vec<u32> {
    let mut out = vec![0u32; left.len()];
    for i in 0..left.len() {
        let b = right.get(i).copied().unwrap_or(0);
        out[i] = (left[i] & mask) | (b & !mask);
    }
    out
}

fn ct_cond_sub(value: &[u32], extra: u32, modulus: &[u32]) -> Vec<u32> {
    let (diff, borrow) = sub_limbs_u32(value, modulus);
    let mut extra_bit = extra;
    extra_bit |= extra_bit >> 16;
    extra_bit |= extra_bit >> 8;
    extra_bit |= extra_bit >> 4;
    extra_bit |= extra_bit >> 2;
    extra_bit |= extra_bit >> 1;
    let subtract = (extra_bit & 1) | (borrow ^ 1);
    let mask = 0u32.wrapping_sub(subtract & 1);
    select_limbs(mask, &diff, value)
}

fn ct_shift_in(rem: &[u32], bit: u32, modulus: &[u32]) -> Vec<u32> {
    let mut shifted = vec![0u32; rem.len()];
    let mut carry = bit & 1;
    for i in 0..rem.len() {
        let next = rem[i] >> 31;
        shifted[i] = (rem[i] << 1) | carry;
        carry = next;
    }
    ct_cond_sub(&shifted, carry, modulus)
}

fn ct_mod_bytes(bytes: &[u8], modulus: &[u32]) -> Vec<u32> {
    let mut rem = vec![0u32; modulus.len()];
    for &byte in bytes {
        for shift in (0..8u32).rev() {
            let bit = u32::from((byte >> shift) & 1);
            rem = ct_shift_in(&rem, bit, modulus);
        }
    }
    rem
}

fn ct_double_mod(value: &[u32], modulus: &[u32]) -> Vec<u32> {
    ct_shift_in(value, 0, modulus)
}

fn ct_r2(modulus: &[u32]) -> Vec<u32> {
    let mut acc = vec![0u32; modulus.len()];
    if !modulus.is_empty() {
        acc[0] = 1;
    }
    for _ in 0..modulus.len().saturating_mul(64) {
        acc = ct_double_mod(&acc, modulus);
    }
    acc
}

fn mont_mul(left: &[u32], right: &[u32], modulus: &[u32], n0inv: u32) -> Vec<u32> {
    let k = modulus.len();
    let mut acc = vec![0u32; k];
    let mut overflow = 0u32;
    for i in 0..k {
        let mut carry = 0u128;
        for j in 0..k {
            let sum = u128::from(acc[j]) + u128::from(left[i]) * u128::from(right[j]) + carry;
            acc[j] = sum as u32;
            carry = sum >> 32;
        }
        let sum = u128::from(overflow) + carry;
        let hi = sum as u32;
        overflow = (sum >> 32) as u32;
        let m = acc[0].wrapping_mul(n0inv);
        let mut carry = 0u128;
        for j in 0..k {
            let sum = u128::from(acc[j]) + u128::from(m) * u128::from(modulus[j]) + carry;
            if j > 0 {
                acc[j - 1] = sum as u32;
            }
            carry = sum >> 32;
        }
        let sum = u128::from(hi) + carry;
        if k > 0 {
            acc[k - 1] = sum as u32;
        }
        overflow = overflow.wrapping_add((sum >> 32) as u32);
    }
    let once = ct_cond_sub(&acc, overflow, modulus);
    ct_cond_sub(&once, 0, modulus)
}

fn monty_pow(base: &Nat, exp: &Nat, modulus: &Nat, secret: bool) -> Result<Nat, String> {
    if modulus.cmp(&Nat::from_u32(1)) != Ordering::Greater || modulus.bitlen() > MAX_RSA_BITS || exp.bitlen() > MAX_RSA_BITS {
        return Err("integer size exceeds the working limit".into());
    }
    if !modulus.bit(0) {
        return Err("integer size exceeds the working limit".into());
    }
    let width = limb_width(modulus);
    if width == 0 || width > MAX_RSA_MODULUS_BYTES / 4 + 1 {
        return Err("integer size exceeds the working limit".into());
    }
    let n = modulus.to_width(width);
    let n0inv = invert_odd(n[0]).wrapping_neg();
    let r2 = ct_r2(&n);
    let reduced = if secret {
        ct_mod_bytes(&base.to_be(), &n)
    } else {
        base.modulo(modulus).to_width(width)
    };
    let mut window = mont_mul(&reduced, &r2, &n, n0inv);
    let one = {
        let mut limbs = vec![0u32; width];
        limbs[0] = 1;
        limbs
    };
    let mut result = mont_mul(&one, &r2, &n, n0inv);
    let bits = if secret { modulus.bitlen() } else { exp.bitlen() };
    let exp_limbs = exp.to_width(width.max((exp.bitlen() + 31) / 32));
    for index in 0..bits {
        if secret {
            let product = mont_mul(&result, &window, &n, n0inv);
            let bit = {
                let word = exp_limbs.get(index / 32).copied().unwrap_or(0);
                (word >> (index % 32)) & 1
            };
            let mask = 0u32.wrapping_sub(bit);
            result = select_limbs(mask, &product, &result);
        } else if exp.bit(index) {
            result = mont_mul(&result, &window, &n, n0inv);
        }
        window = mont_mul(&window, &window, &n, n0inv);
    }
    Ok(nat_from_limbs(&mont_mul(&result, &one, &n, n0inv)))
}

fn fit_width(limbs: &[u32], width: usize) -> Vec<u32> {
    let mut out = vec![0u32; width];
    for i in 0..width.min(limbs.len()) {
        out[i] = limbs[i];
    }
    out
}

fn modpow_limbs(base: &[u32], exp: &[u32], modulus: &[u32]) -> Vec<u32> {
    let width = modulus.len();
    if width == 0 || modulus[0] & 1 == 0 {
        return vec![0u32; width];
    }
    let n0inv = invert_odd(modulus[0]).wrapping_neg();
    let r2 = ct_r2(modulus);
    let base = fit_width(base, width);
    let exp = fit_width(exp, width);
    let mut window = mont_mul(&base, &r2, modulus, n0inv);
    let mut one = vec![0u32; width];
    one[0] = 1;
    let mut result = mont_mul(&one, &r2, modulus, n0inv);
    for index in 0..width.saturating_mul(32) {
        let product = mont_mul(&result, &window, modulus, n0inv);
        let word = exp.get(index / 32).copied().unwrap_or(0);
        let bit = (word >> (index % 32)) & 1;
        result = select_limbs(0u32.wrapping_sub(bit), &product, &result);
        window = mont_mul(&window, &window, modulus, n0inv);
    }
    mont_mul(&result, &one, modulus, n0inv)
}

fn limbs_to_be(limbs: &[u32]) -> Vec<u8> {
    let mut bytes = vec![0u8; limbs.len().saturating_mul(4)];
    for (index, limb) in limbs.iter().enumerate() {
        let at = index * 4;
        if at + 4 <= bytes.len() {
            bytes[at..at + 4].copy_from_slice(&limb.to_le_bytes());
        }
    }
    bytes.reverse();
    bytes
}

fn crt_finish(m2: &[u32], q: &[u32], h: &[u32], n: &[u32]) -> Vec<u32> {
    let prod = mul_limbs(q, h);
    let mut sum = vec![0u32; prod.len()];
    let mut carry = 0u64;
    for i in 0..prod.len() {
        let low = u64::from(*m2.get(i).unwrap_or(&0));
        let total = low + u64::from(prod[i]) + carry;
        sum[i] = total as u32;
        carry = total >> 32;
    }
    ct_mod_bytes(&limbs_to_be(&sum), n)
}

fn plain_mul_mod(left: &[u32], right: &[u32], modulus: &[u32]) -> Vec<u32> {
    let width = modulus.len();
    let n0inv = invert_odd(modulus[0]).wrapping_neg();
    let r2 = ct_r2(modulus);
    let a = mont_mul(left, &r2, modulus, n0inv);
    let b = mont_mul(right, &r2, modulus, n0inv);
    let product = mont_mul(&a, &b, modulus, n0inv);
    let one = {
        let mut limbs = vec![0u32; width];
        if width > 0 {
            limbs[0] = 1;
        }
        limbs
    };
    mont_mul(&product, &one, modulus, n0inv)
}

fn ct_sub_mod_limbs(left: &[u32], right: &[u32], modulus: &[u32]) -> Vec<u32> {
    let (diff, borrow) = sub_limbs_u32(left, right);
    let mask = 0u32.wrapping_sub(borrow);
    let mut added = vec![0u32; left.len()];
    let mut carry = 0u64;
    for i in 0..left.len() {
        let addend = u64::from(modulus.get(i).copied().unwrap_or(0) & mask);
        let sum = u64::from(diff[i]) + addend + carry;
        added[i] = sum as u32;
        carry = sum >> 32;
    }
    added
}

fn mul_limbs(left: &[u32], right: &[u32]) -> Vec<u32> {
    let mut acc = vec![0u64; left.len() + right.len() + 1];
    for i in 0..left.len() {
        let mut carry = 0u64;
        for j in 0..right.len() {
            let sum = acc[i + j] + u64::from(left[i]) * u64::from(right[j]) + carry;
            acc[i + j] = sum & 0xffff_ffff;
            carry = sum >> 32;
        }
        acc[i + right.len()] += carry;
    }
    let mut out = vec![0u32; acc.len()];
    let mut extra = 0u64;
    for i in 0..acc.len() {
        let sum = acc[i] + extra;
        out[i] = sum as u32;
        extra = sum >> 32;
    }
    out
}

fn random_unit(modulus: &[u32]) -> Result<Vec<u32>, String> {
    let bytes = modulus.len().saturating_mul(4);
    if bytes == 0 || bytes > MAX_RANDOM_BYTES {
        return Err("randomness request exceeds the size budget".into());
    }
    let raw = random_bytes(bytes)?;
    let mut reduced = ct_mod_bytes(&raw, modulus);
    let mut acc = 0u32;
    for limb in &reduced {
        acc |= *limb;
    }
    acc |= acc >> 16;
    acc |= acc >> 8;
    acc |= acc >> 4;
    acc |= acc >> 2;
    acc |= acc >> 1;
    let is_zero = (acc & 1) ^ 1;
    let mask = 0u32.wrapping_sub(is_zero);
    if !reduced.is_empty() {
        reduced[0] = (reduced[0] & !mask) | (1 & mask);
    }
    Ok(reduced)
}

fn sub_const_limbs(limbs: &[u32], value: u32) -> Vec<u32> {
    let mut out = vec![0u32; limbs.len()];
    let mut borrow = u64::from(value);
    for i in 0..limbs.len() {
        let lhs = u64::from(limbs[i]);
        let diff = lhs.wrapping_sub(borrow);
        out[i] = diff as u32;
        borrow = u64::from(lhs < borrow);
    }
    out
}

#[allow(dead_code)]
fn secret_bit_mask(exp: &Nat, index: usize) -> u32 {
    let word_index = index / 32;
    let mut limb = 0u32;
    let mut i = 0usize;
    while i < exp.w.len() {
        let select = 0u32.wrapping_sub(u32::from(i == word_index));
        limb |= exp.w[i] & select;
        i += 1;
    }
    let shift = index % 32;
    let bit = (limb >> shift) & 1;
    0u32.wrapping_sub(bit)
}

#[allow(dead_code)]
fn nat_select(mask: u32, left: &Nat, right: &Nat) -> Nat {
    let width = left.w.len().max(right.w.len());
    let mut w = Vec::with_capacity(width);
    for i in 0..width {
        let a = left.w.get(i).copied().unwrap_or(0);
        let b = right.w.get(i).copied().unwrap_or(0);
        w.push((a & mask) | (b & !mask));
    }
    let mut n = Nat { w };
    n.norm();
    n
}

#[derive(Clone)]
struct Signed {
    neg: bool,
    mag: Nat,
}

impl Signed {
    fn zero() -> Self {
        Signed { neg: false, mag: Nat::zero() }
    }
    fn pos(mag: Nat) -> Self {
        Signed { neg: false, mag }
    }
    fn mul_nat(&self, other: &Nat) -> Signed {
        if self.mag.is_zero() || other.is_zero() {
            Self::zero()
        } else {
            Signed { neg: self.neg, mag: self.mag.mul(other) }
        }
    }
    fn sub(&self, other: &Signed) -> Signed {
        self.add(&Signed { neg: !other.neg && !other.mag.is_zero(), mag: other.mag.clone() })
    }
    fn add(&self, other: &Signed) -> Signed {
        if self.neg == other.neg {
            let mag = self.mag.add(&other.mag);
            return Signed { neg: self.neg && !mag.is_zero(), mag };
        }
        match self.mag.cmp(&other.mag) {
            Ordering::Greater => Signed { neg: self.neg, mag: self.mag.sub(&other.mag) },
            Ordering::Less => Signed { neg: other.neg, mag: other.mag.sub(&self.mag) },
            Ordering::Equal => Self::zero(),
        }
    }
    fn residue(&self, modulus: &Nat) -> Nat {
        let mut mag = self.mag.modulo(modulus);
        if self.neg && !mag.is_zero() {
            mag = modulus.sub(&mag);
        }
        mag
    }
}

fn small_primes() -> &'static [u32] {
    use std::sync::OnceLock;
    static PRIMES: OnceLock<Vec<u32>> = OnceLock::new();
    PRIMES.get_or_init(|| {
        let mut sieve = vec![true; 5000];
        sieve[0] = false;
        sieve[1] = false;
        for i in 2..5000 {
            if sieve[i] {
                let mut j = i * i;
                while j < 5000 {
                    sieve[j] = false;
                    j += i;
                }
            }
        }
        sieve.iter().enumerate().filter(|(_, p)| **p).map(|(i, _)| i as u32).collect()
    })
}

fn is_probable_prime(n: &Nat) -> Result<bool, String> {
    if n.cmp(&Nat::from_u32(2)) == Ordering::Less {
        return Ok(false);
    }
    for prime in small_primes() {
        let p = Nat::from_u32(*prime);
        if n.cmp(&p) == Ordering::Equal {
            return Ok(true);
        }
        if n.mod_u32(*prime) == 0 {
            return Ok(false);
        }
    }
    let one = Nat::from_u32(1);
    let nm1 = n.sub(&one);
    let mut d = nm1.clone();
    let mut s = 0u32;
    while !d.is_zero() && !d.bit(0) {
        d = d.shr1();
        s += 1;
    }
    for _ in 0..PRIME_WITNESS_ROUNDS {
        let base = random_below(n)?;
        let mut x = base.modpow(&d, n)?;
        if x.cmp(&one) == Ordering::Equal || x.cmp(&nm1) == Ordering::Equal {
            continue;
        }
        let mut witnessed = false;
        for _ in 1..s {
            x = x.mul(&x).divmod(n)?.1;
            if x.cmp(&nm1) == Ordering::Equal {
                witnessed = true;
                break;
            }
            if x.cmp(&one) == Ordering::Equal {
                return Ok(false);
            }
        }
        if !witnessed {
            return Ok(false);
        }
    }
    Ok(true)
}

fn random_below(modulus: &Nat) -> Result<Nat, String> {
    let bytes = (modulus.bitlen() + 7) / 8 + 1;
    if bytes > MAX_RANDOM_BYTES {
        return Err("integer size exceeds the working limit".into());
    }
    for _ in 0..256 {
        let raw = random_bytes(bytes)?;
        let value = Nat::from_be(&raw).modulo(modulus);
        if value.cmp(&Nat::from_u32(2)) != Ordering::Less && value.cmp(&modulus.sub(&Nat::from_u32(2))) != Ordering::Greater {
            return Ok(value);
        }
    }
    Err("randomness did not yield a value in range".into())
}

fn random_odd(bits: usize) -> Result<Nat, String> {
    if bits < 16 || bits % 8 != 0 || bits / 8 > MAX_RSA_MODULUS_BYTES {
        return Err("prime size is not supported".into());
    }
    let bytes = bits / 8;
    let mut buf = random_bytes(bytes)?;
    buf[0] |= 0x80;
    let last = bytes - 1;
    buf[last] |= 1;
    Ok(Nat::from_be(&buf))
}

fn generate_prime(bits: usize) -> Result<Nat, String> {
    let e = Nat::from_u32(65537);
    for _ in 0..100_000 {
        let candidate = random_odd(bits)?;
        if !is_probable_prime(&candidate)? {
            continue;
        }
        if candidate.sub(&Nat::from_u32(1)).divmod(&e)?.1.is_zero() {
            continue;
        }
        return Ok(candidate);
    }
    Err("a prime was not found within the attempt budget".into())
}

#[derive(Debug)]
pub struct PrivateKey {
    n: Nat,
    e: Nat,
    d: Nat,
    p: Nat,
    q: Nat,
    dp: Nat,
    dq: Nat,
    qinv: Nat,
}

impl PrivateKey {
    pub fn generate(bits: usize) -> Result<Self, String> {
        if bits < 512 || bits > MAX_RSA_BITS || bits % 16 != 0 {
            return Err("key size must be a multiple of 16 and from 512 to 4096 bits".into());
        }
        for _ in 0..64 {
            let p = generate_prime(bits / 2)?;
            let q = generate_prime(bits / 2)?;
            if p.cmp(&q) == Ordering::Equal {
                continue;
            }
            let (p, q) = if p.cmp(&q) == Ordering::Greater { (p, q) } else { (q, p) };
            let n = p.mul(&q);
            if n.bitlen() != bits {
                continue;
            }
            let one = Nat::from_u32(1);
            let phi = p.sub(&one).mul(&q.sub(&one));
            let e = Nat::from_u32(65537);
            let d = e.inv_mod(&phi)?;
            return PrivateKey::assemble(n, e, d, p, q);
        }
        Err("a key was not found within the attempt budget".into())
    }

    fn assemble(n: Nat, e: Nat, d: Nat, p: Nat, q: Nat) -> Result<Self, String> {
        let one = Nat::from_u32(1);
        if p.cmp(&one) != Ordering::Greater || q.cmp(&one) != Ordering::Greater {
            return Err("stored key is not a signing key".into());
        }
        let dp = d.divmod(&p.sub(&one))?.1;
        let dq = d.divmod(&q.sub(&one))?.1;
        let qinv = q.inv_mod(&p)?;
        Ok(PrivateKey { n, e, d, p, q, dp, dq, qinv })
    }

    pub fn sign_sha1_pkcs1(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        self.pkcs1_sign(&sha1(message), b"\x30\x21\x30\x09\x06\x05\x2b\x0e\x03\x02\x1a\x05\x00\x04\x14")
    }

    pub fn sign_sha256_pkcs1(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        self.pkcs1_sign(&sha256(message), b"\x30\x31\x30\x0d\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x01\x05\x00\x04\x20")
    }

    pub fn sign_sha256_pss(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        let hash = sha256(message);
        let salt = random_bytes(32)?;
        let mut m_prime = vec![0u8; 8];
        m_prime.extend_from_slice(&hash);
        m_prime.extend_from_slice(&salt);
        let h = sha256(&m_prime);
        let k = (self.n.bitlen() + 7) / 8;
        let em_len = (self.n.bitlen() - 1 + 7) / 8;
        if em_len < 32 + salt.len() + 2 {
            return Err("key is too small for the signature".into());
        }
        let db_len = em_len - 32 - 1;
        let mut db = vec![0u8; db_len - salt.len() - 1];
        db.push(0x01);
        db.extend_from_slice(&salt);
        let mask = mgf1(&h, db_len)?;
        for i in 0..db_len {
            db[i] ^= mask[i];
        }
        let unused = 8 * em_len - (self.n.bitlen() - 1);
        if unused > 0 {
            db[0] &= 0xff >> unused;
        }
        let mut em = db;
        em.extend_from_slice(&h);
        em.push(0xbc);
        if em.len() < k {
            let mut padded = vec![0u8; k - em.len()];
            padded.extend(em);
            em = padded;
        }
        self.private_exp(&em)
    }

    fn pkcs1_sign(&self, hash: &[u8], prefix: &[u8]) -> Result<Vec<u8>, String> {
        let k = (self.n.bitlen() + 7) / 8;
        let t_len = prefix.len() + hash.len();
        if k < t_len + 11 {
            return Err("key is too small for the signature".into());
        }
        let mut em = vec![0xffu8; k];
        em[0] = 0x00;
        em[1] = 0x01;
        let marker = k - t_len;
        em[marker - 1] = 0x00;
        em[marker..marker + prefix.len()].copy_from_slice(prefix);
        em[k - hash.len()..].copy_from_slice(hash);
        self.private_exp(&em)
    }

    fn private_exp(&self, encoded: &[u8]) -> Result<Vec<u8>, String> {
        if encoded.len() > MAX_RSA_MODULUS_BYTES + 1 {
            return Err("signature integer is not smaller than the modulus".into());
        }
        let value = Nat::from_be(encoded);
        if value.cmp(&self.n) != Ordering::Less {
            return Err("signature integer is not smaller than the modulus".into());
        }
        let p = self.p.to_width(limb_width(&self.p));
        let q = self.q.to_width(limb_width(&self.q));
        let n = self.n.to_width(limb_width(&self.n));
        let rp = random_unit(&p)?;
        let rq = random_unit(&q)?;
        let rp_inv = modpow_limbs(&rp, &sub_const_limbs(&p, 2), &p);
        let rq_inv = modpow_limbs(&rq, &sub_const_limbs(&q, 2), &q);
        let rp_e = modpow_limbs(&rp, &self.e.to_width(p.len()), &p);
        let rq_e = modpow_limbs(&rq, &self.e.to_width(q.len()), &q);
        let mp = ct_mod_bytes(encoded, &p);
        let mq = ct_mod_bytes(encoded, &q);
        let m1 = plain_mul_mod(&modpow_limbs(&plain_mul_mod(&mp, &rp_e, &p), &self.dp.to_width(p.len()), &p), &rp_inv, &p);
        let m2 = plain_mul_mod(&modpow_limbs(&plain_mul_mod(&mq, &rq_e, &q), &self.dq.to_width(q.len()), &q), &rq_inv, &q);
        let m2_mod_p = ct_mod_bytes(&limbs_to_be(&m2), &p);
        let diff = ct_sub_mod_limbs(&m1, &m2_mod_p, &p);
        let h = plain_mul_mod(&self.qinv.to_width(p.len()), &diff, &p);
        let signed = crt_finish(&m2, &q, &h, &n);
        Ok(nat_from_limbs(&signed).to_be_fixed((self.n.bitlen() + 7) / 8))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn verify_sha1_pkcs1(&self, message: &[u8], signature: &[u8]) -> Result<(), String> {
        let encoded = self.public_exp(signature)?;
        let hash = sha1(message);
        let prefix: &[u8] = b"\x30\x21\x30\x09\x06\x05\x2b\x0e\x03\x02\x1a\x05\x00\x04\x14";
        check_pkcs1(&encoded, prefix, &hash)
    }

    pub fn verify_sha256_pss(n_be: &[u8], e_be: &[u8], message: &[u8], signature: &[u8]) -> Result<(), String> {
        if n_be.len() > MAX_RSA_MODULUS_BYTES || n_be.is_empty() || e_be.is_empty() || e_be.len() > MAX_RSA_PUBLIC_EXPONENT_BYTES || signature.len() > MAX_RSA_MODULUS_BYTES + 1 {
            return Err("rsa parameters are outside the supported size".into());
        }
        let n = Nat::from_be(n_be);
        let e = Nat::from_be(e_be);
        let bits = n.bitlen();
        if bits < MIN_RSA_MODULUS_BITS || bits > MAX_RSA_BITS || !n.bit(0) {
            return Err("rsa modulus is outside the supported size".into());
        }
        if e.cmp(&Nat::from_u32(3)) == Ordering::Less || !e.bit(0) || e.bitlen() > 32 {
            return Err("rsa public exponent is outside the supported size".into());
        }
        let em_len = (bits - 1 + 7) / 8;
        if em_len < 32 + 32 + 2 {
            return Err("rsa modulus is too small for the signature".into());
        }
        let k = (bits + 7) / 8;
        if signature.len() != k {
            return Err("signature length does not match the modulus".into());
        }
        let value = Nat::from_be(signature);
        if value.cmp(&n) != Ordering::Less {
            return Err("signature integer is not smaller than the modulus".into());
        }
        let encoded = value.modpow(&e, &n)?;
        if !encoded.is_zero() && encoded.to_be().len() > em_len {
            return Err("signature encoding does not fit".into());
        }
        let encoded = encoded.to_be_fixed(em_len);
        let em = encoded.as_slice();
        if em.last() != Some(&0xbc) || em.len() < 34 {
            return Err("signature trailer is not intact".into());
        }
        let db_len = em.len() - 32 - 1;
        let mut db = em[..db_len].to_vec();
        let h = &em[db_len..db_len + 32];
        let mask = mgf1(h, db_len)?;
        for i in 0..db_len {
            db[i] ^= mask[i];
        }
        let unused = 8 * em.len() - (bits - 1);
        if unused >= 8 {
            return Err("signature encoding is too short".into());
        }
        if unused > 0 {
            db[0] &= 0xff >> unused;
        }
        let mut index = 0;
        while index < db.len() && db[index] == 0 {
            index += 1;
        }
        if index >= db.len() || db[index] != 0x01 {
            return Err("signature padding is not intact".into());
        }
        let salt = &db[index + 1..];
        if salt.len() != 32 {
            return Err("signature salt length is not the digest length".into());
        }
        let mut m_prime = vec![0u8; 8];
        m_prime.extend_from_slice(&sha256(message));
        m_prime.extend_from_slice(salt);
        if !tags_equal(&sha256(&m_prime), h) {
            return Err("signature did not verify".into());
        }
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn public_exp(&self, signature: &[u8]) -> Result<Vec<u8>, String> {
        let k = (self.n.bitlen() + 7) / 8;
        if signature.len() != k {
            return Err("signature length does not match the modulus".into());
        }
        let value = Nat::from_be(signature);
        if value.cmp(&self.n) != Ordering::Less {
            return Err("signature integer is not smaller than the modulus".into());
        }
        let plain = value.modpow(&self.e, &self.n)?;
        Ok(plain.to_be_fixed(k))
    }

    pub fn device_public_line(&self, comment: &str) -> Result<String, String> {
        if self.n.bitlen() != 2048 {
            return Err("the device public record is 2048 bits".into());
        }
        let modulus = self.n.to_be_fixed(256);
        let mut little = modulus.clone();
        little.reverse();
        let n0 = u32::from_le_bytes(little[0..4].try_into().unwrap());
        let n0inv = invert_odd(n0).wrapping_neg();
        let rr = Nat::from_u32(2).modpow(&Nat::from_u32(4096), &self.n)?;
        let mut rr_le = rr.to_be_fixed(256);
        rr_le.reverse();
        let mut record = Vec::with_capacity(524);
        record.extend_from_slice(&64u32.to_le_bytes());
        record.extend_from_slice(&n0inv.to_le_bytes());
        record.extend_from_slice(&little);
        record.extend_from_slice(&rr_le);
        record.extend_from_slice(&65537u32.to_le_bytes());
        Ok(format!("{} {}", b64(&record), comment))
    }

    pub fn certificate(&self, name: &str) -> Result<Vec<u8>, String> {
        let tbs = tbs_certificate(self, name)?;
        let signature = self.sign_sha256_pkcs1(&tbs)?;
        let alg = sha256_rsa_alg();
        let mut sig = vec![0u8];
        sig.extend_from_slice(&signature);
        let mut body = tbs;
        body.extend(alg);
        body.extend(der(0x03, &sig));
        Ok(der(0x30, &body))
    }

    pub fn store_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for part in [&self.n, &self.e, &self.d, &self.p, &self.q] {
            let be = part.to_be();
            out.extend_from_slice(&(be.len() as u32).to_be_bytes());
            out.extend_from_slice(&be);
        }
        out
    }

    pub fn load_bytes(data: &[u8]) -> Result<Self, String> {
        let mut cursor = 0;
        let mut take = || -> Result<Nat, String> {
            if cursor + 4 > data.len() {
                return Err("stored key is truncated".into());
            }
            let len = u32::from_be_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            if len > MAX_STORED_COMPONENT {
                return Err("stored key component is too large".into());
            }
            if cursor + len > data.len() {
                return Err("stored key is truncated".into());
            }
            let n = Nat::from_be(&data[cursor..cursor + len]);
            cursor += len;
            Ok(n)
        };
        let key = PrivateKey::assemble(take()?, take()?, take()?, take()?, take()?)?;
        if cursor != data.len() {
            return Err("stored key has trailing bytes".into());
        }
        Ok(key)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn public_parts(&self) -> (Vec<u8>, Vec<u8>) {
        (self.n.to_be(), self.e.to_be())
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn check_pkcs1(encoded: &[u8], prefix: &[u8], hash: &[u8]) -> Result<(), String> {
    if encoded.len() < prefix.len() + hash.len() + 11 || encoded[0] != 0 || encoded[1] != 1 {
        return Err("signature block is not intact".into());
    }
    let mut i = 2;
    while i < encoded.len() && encoded[i] == 0xff {
        i += 1;
    }
    if i < 10 || i >= encoded.len() || encoded[i] != 0 {
        return Err("signature padding is not intact".into());
    }
    i += 1;
    if encoded.len() - i != prefix.len() + hash.len() {
        return Err("signature length does not match the digest".into());
    }
    if &encoded[i..i + prefix.len()] != prefix || &encoded[i + prefix.len()..] != hash {
        return Err("signature did not verify".into());
    }
    Ok(())
}

fn mgf1(seed: &[u8], length: usize) -> Result<Vec<u8>, String> {
    if length > HKDF_MAX_BLOCKS * HASH_LEN {
        return Err("mask length exceeds the hash limit".into());
    }
    let mut out = Vec::new();
    let mut counter = 0u32;
    while out.len() < length {
        if counter == u32::MAX {
            return Err("mask counter exhausted".into());
        }
        let mut block = seed.to_vec();
        block.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&sha256(&block));
        counter += 1;
    }
    out.truncate(length);
    Ok(out)
}

fn invert_odd(value: u32) -> u32 {
    let mut inv = 1u32;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(value.wrapping_mul(inv)));
    }
    inv
}

fn der(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = body.len();
    if n < 128 {
        out.push(n as u8);
    } else if n < 256 {
        out.push(0x81);
        out.push(n as u8);
    } else if n < 65536 {
        out.push(0x82);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0x83);
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
    }
    out.extend_from_slice(body);
    out
}

fn der_int(be: &[u8]) -> Vec<u8> {
    let mut v = be.to_vec();
    if v.is_empty() {
        v.push(0);
    }
    while v.len() > 1 && v[0] == 0 && v[1] & 0x80 == 0 {
        v.remove(0);
    }
    if v[0] & 0x80 != 0 {
        v.insert(0, 0);
    }
    der(0x02, &v)
}

fn sha256_rsa_alg() -> Vec<u8> {
    let oid = der(0x06, &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b]);
    let mut body = oid;
    body.extend_from_slice(&[0x05, 0x00]);
    der(0x30, &body)
}

fn rsa_alg() -> Vec<u8> {
    let oid = der(0x06, &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01]);
    let mut body = oid;
    body.extend_from_slice(&[0x05, 0x00]);
    der(0x30, &body)
}

fn name_dn(text: &str) -> Vec<u8> {
    let oid = der(0x06, &[0x55, 0x04, 0x03]);
    let value = der(0x0c, text.as_bytes());
    let mut pair = oid;
    pair.extend(value);
    let inner = der(0x30, &pair);
    let set = der(0x31, &inner);
    der(0x30, &set)
}

fn tbs_certificate(key: &PrivateKey, name: &str) -> Result<Vec<u8>, String> {
    let version = der(0xa0, &der_int(&[2]));
    let digest = sha256(&key.n.to_be());
    let mut serial_raw = digest[..8].to_vec();
    serial_raw[0] &= 0x7f;
    if serial_raw.iter().all(|b| *b == 0) {
        serial_raw[7] = 1;
    }
    let serial = der_int(&serial_raw);
    let alg = sha256_rsa_alg();
    let dn = name_dn(name);
    let time = der(0x18, b"20200101000000Z");
    let time2 = der(0x18, b"20991231235959Z");
    let mut validity = time;
    validity.extend(time2);
    let validity = der(0x30, &validity);
    let n = der_int(&key.n.to_be());
    let e = der_int(&key.e.to_be());
    let mut rsa = n;
    rsa.extend(e);
    let rsa = der(0x30, &rsa);
    let mut bits = vec![0u8];
    bits.extend_from_slice(&rsa);
    let mut spki = rsa_alg();
    spki.extend(der(0x03, &bits));
    let spki = der(0x30, &spki);
    let mut body = version;
    body.extend(serial);
    body.extend(alg);
    body.extend(dn.clone());
    body.extend(validity);
    body.extend(dn);
    body.extend(spki);
    Ok(der(0x30, &body))
}

pub fn b64(data: &[u8]) -> String {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(ALPHA[((n >> 18) & 63) as usize] as char);
        out.push(ALPHA[((n >> 12) & 63) as usize] as char);
        out.push(ALPHA[((n >> 6) & 63) as usize] as char);
        out.push(ALPHA[(n & 63) as usize] as char);
        i += 3;
    }
    if i < data.len() {
        let mut n = (data[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 63) as usize] as char);
        if i + 1 < data.len() {
            n |= (data[i + 1] as u32) << 8;
            out.push(ALPHA[((n >> 12) & 63) as usize] as char);
            out.push(ALPHA[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push(ALPHA[((n >> 12) & 63) as usize] as char);
            out.push('=');
            out.push('=');
        }
    }
    out
}

pub fn unb64(text: &str) -> Result<Vec<u8>, String> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err("encoded text is not a whole block".into());
    }
    fn val(b: u8) -> Result<u8, String> {
        match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("encoded text has a character outside the alphabet".into()),
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let last = i + 4 == bytes.len();
        let c_is_pad = bytes[i + 2] == b'=';
        let d_is_pad = bytes[i + 3] == b'=';
        if bytes[i] == b'=' || bytes[i + 1] == b'=' || (c_is_pad && !d_is_pad) || ((c_is_pad || d_is_pad) && !last) {
            return Err("encoded text has padding in the wrong place".into());
        }
        let a = val(bytes[i])?;
        let b = val(bytes[i + 1])?;
        let c = if c_is_pad { 0 } else { val(bytes[i + 2])? };
        let d = if d_is_pad { 0 } else { val(bytes[i + 3])? };
        if c_is_pad && (b & 0x0f) != 0 {
            return Err("encoded text has unused bits set".into());
        }
        if d_is_pad && !c_is_pad && (c & 0x03) != 0 {
            return Err("encoded text has unused bits set".into());
        }
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | d as u32;
        out.push((n >> 16) as u8);
        if !c_is_pad {
            out.push((n >> 8) as u8);
        }
        if !d_is_pad {
            out.push(n as u8);
        }
        i += 4;
    }
    Ok(out)
}

struct Der<'a> {
    data: &'a [u8],
    i: usize,
}

impl<'a> Der<'a> {
    fn tlv(&mut self) -> Result<(u8, &'a [u8]), String> {
        if self.i >= self.data.len() {
            return Err("certificate is truncated".into());
        }
        let tag = self.data[self.i];
        self.i += 1;
        if self.i >= self.data.len() {
            return Err("certificate is truncated".into());
        }
        let first = self.data[self.i];
        self.i += 1;
        let len = if first & 0x80 == 0 {
            first as usize
        } else {
            let n = (first & 0x7f) as usize;
            if n == 0 || n > 3 || self.i.checked_add(n).map(|end| end > self.data.len()).unwrap_or(true) {
                return Err("certificate length is invalid".into());
            }
            let mut v = 0usize;
            for _ in 0..n {
                v = (v << 8) | self.data[self.i] as usize;
                self.i += 1;
            }
            let min_wide = if n == 1 { 128usize } else { 1usize << (8 * (n - 1)) };
            if v < min_wide {
                return Err("certificate length is invalid".into());
            }
            v
        };
        let end = match self.i.checked_add(len) {
            Some(end) if end <= self.data.len() => end,
            _ => return Err("certificate is truncated".into()),
        };
        let body = &self.data[self.i..end];
        self.i = end;
        Ok((tag, body))
    }
    fn finish(&self) -> Result<(), String> {
        if self.i != self.data.len() {
            return Err("certificate has trailing bytes".into());
        }
        Ok(())
    }
    fn expect(&mut self, tag: u8) -> Result<&'a [u8], String> {
        let (got, body) = self.tlv()?;
        if got != tag {
            return Err("certificate field is not the expected kind".into());
        }
        Ok(body)
    }
}

fn der_integer_ok(body: &[u8]) -> Result<(), String> {
    if body.is_empty() {
        return Err("certificate integer is empty".into());
    }
    if body.len() > 1 && body[0] == 0 && body[1] & 0x80 == 0 {
        return Err("certificate integer is not minimally encoded".into());
    }
    Ok(())
}

pub fn certificate_public_rsa(der_bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if der_bytes.len() > MAX_HS_MESSAGE {
        return Err("certificate exceeds the size budget".into());
    }
    let mut top = Der { data: der_bytes, i: 0 };
    let cert_body = top.expect(0x30)?;
    top.finish()?;
    let mut cert = Der { data: cert_body, i: 0 };
    let tbs = cert.expect(0x30)?;
    cert.expect(0x30)?;
    let sig_bits = cert.expect(0x03)?;
    if sig_bits.is_empty() {
        return Err("certificate signature is empty".into());
    }
    cert.finish()?;
    let mut cursor = Der { data: tbs, i: 0 };
    let (tag, first_body) = cursor.tlv()?;
    if tag == 0xa0 {
        let mut ver = Der { data: first_body, i: 0 };
        der_integer_ok(ver.expect(0x02)?)?;
        ver.finish()?;
        der_integer_ok(cursor.expect(0x02)?)?;
    } else if tag == 0x02 {
        der_integer_ok(first_body)?;
    } else {
        return Err("certificate version is missing".into());
    }
    cursor.expect(0x30)?;
    cursor.expect(0x30)?;
    cursor.expect(0x30)?;
    cursor.expect(0x30)?;
    let spki = cursor.expect(0x30)?.to_vec();
    while cursor.i < tbs.len() {
        cursor.tlv()?;
    }
    cursor.finish()?;
    let mut sp = Der { data: &spki, i: 0 };
    sp.expect(0x30)?;
    let bits = sp.expect(0x03)?;
    sp.finish()?;
    if bits.is_empty() {
        return Err("certificate key is empty".into());
    }
    if bits[0] != 0 {
        return Err("certificate key is not byte aligned".into());
    }
    let mut rsa = Der { data: &bits[1..], i: 0 };
    let seq = rsa.expect(0x30)?;
    rsa.finish()?;
    let mut nums = Der { data: seq, i: 0 };
    let n_body = nums.expect(0x02)?;
    der_integer_ok(n_body)?;
    let e_body = nums.expect(0x02)?;
    der_integer_ok(e_body)?;
    nums.finish()?;
    let n = strip_int(n_body);
    let e = strip_int(e_body);
    if n.is_empty() || n.len() > MAX_RSA_MODULUS_BYTES || e.is_empty() || e.len() > MAX_RSA_PUBLIC_EXPONENT_BYTES {
        return Err("certificate rsa parameters are outside the supported size".into());
    }
    Ok((n.to_vec(), e.to_vec()))
}

fn strip_int(bytes: &[u8]) -> &[u8] {
    let mut s = bytes;
    while s.len() > 1 && s[0] == 0 {
        s = &s[1..];
    }
    s
}

pub struct Exported {
    outcome: Result<Vec<u8>, String>,
}

impl Exported {
    pub fn material(&self) -> Result<&[u8], &str> {
        match &self.outcome {
            Ok(bytes) => Ok(bytes.as_slice()),
            Err(err) => Err(err.as_str()),
        }
    }
}

impl IntoIterator for Exported {
    type Item = u8;
    type IntoIter = std::vec::IntoIter<u8>;
    fn into_iter(self) -> Self::IntoIter {
        match self.outcome {
            Ok(bytes) => bytes.into_iter(),
            Err(_) => Vec::new().into_iter(),
        }
    }
}

pub struct SecurePipe {
    stream: TcpStream,
    write_key: [u8; 16],
    write_iv: [u8; 12],
    write_seq: u64,
    read_key: [u8; 16],
    read_iv: [u8; 12],
    read_seq: u64,
    write_secret: Vec<u8>,
    read_secret: Vec<u8>,
    pending: Vec<u8>,
    post_hs: Vec<u8>,
    exporter: Vec<u8>,
    peer_cert: Vec<u8>,
    reply_key_update: bool,
    write_updates: u64,
}

impl SecurePipe {
    pub fn exporter(&self, label: &[u8], length: usize) -> Exported {
        Exported { outcome: export_keying_material(&self.exporter, label, length) }
    }
    pub fn peer_certificate(&self) -> &[u8] {
        &self.peer_cert
    }
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), String> {
        let mut offset = 0;
        while offset < data.len() {
            let n = (data.len() - offset).min(16384);
            self.write_record(23, &data[offset..offset + n])?;
            offset += n;
        }
        Ok(())
    }
    pub fn read_exact(&mut self, len: usize) -> Result<Vec<u8>, String> {
        let mut stalled = 0usize;
        while self.pending.len() < len {
            let before = self.pending.len();
            let (kind, payload) = self.read_record()?;
            if kind == 21 {
                return Err("the peer closed the secure connection".into());
            }
            if kind == 22 {
                self.accept_post_handshake(&payload)?;
                stalled += 1;
                if stalled > MAX_IGNORED_RECORDS {
                    return Err("too many post-handshake records".into());
                }
                continue;
            }
            if !self.post_hs.is_empty() {
                return Err("handshake message is interleaved with another record".into());
            }
            if kind != 23 {
                return Err("unexpected secure record".into());
            }
            if self.pending.len().saturating_add(payload.len()) > MAX_TRANSCRIPT.saturating_add(len) {
                return Err("secure record exceeded the memory budget".into());
            }
            self.pending.extend_from_slice(&payload);
            if self.pending.len() == before {
                stalled += 1;
                if stalled > MAX_IGNORED_RECORDS {
                    return Err("the peer sent empty records".into());
                }
            } else {
                stalled = 0;
            }
        }
        let out = self.pending[..len].to_vec();
        self.pending.drain(..len);
        Ok(out)
    }
    fn can_rotate_write_key(&self) -> bool {
        self.write_updates < MAX_KEY_UPDATES && self.write_seq < AES_GCM_RECORD_LIMIT
    }
    fn write_record(&mut self, kind: u8, plain: &[u8]) -> Result<(), String> {
        self.write_record_limited(kind, plain, true)
    }
    fn write_record_limited(&mut self, kind: u8, plain: &[u8], schedule: bool) -> Result<(), String> {
        if schedule {
            if self.reply_key_update {
                self.reply_key_update = false;
                if self.can_rotate_write_key() {
                    self.send_key_update(0)?;
                }
            }
            if self.write_seq.saturating_add(1) >= AES_GCM_RECORD_LIMIT {
                if self.can_rotate_write_key() {
                    self.send_key_update(0)?;
                } else {
                    return Err("traffic key record limit reached".into());
                }
            }
        }
        if self.write_seq >= AES_GCM_RECORD_LIMIT {
            return Err("traffic key record limit reached".into());
        }
        let mut inner = plain.to_vec();
        inner.push(kind);
        let length = match inner.len().checked_add(16) {
            Some(n) if n <= 16640 => n,
            _ => return Err("record exceeds the size budget".into()),
        };
        let header = [23u8, 3, 3, (length >> 8) as u8, length as u8];
        let nonce = traffic_nonce(&self.write_iv, self.write_seq);
        let boxed = aes_gcm_seal(&self.write_key, &nonce, &header, &inner)?;
        self.write_seq = self.write_seq.checked_add(1).ok_or_else(|| "traffic key record limit reached".to_string())?;
        self.stream.write_all(&header).map_err(|e| format!("secure write failed: {e}"))?;
        self.stream.write_all(&boxed).map_err(|e| format!("secure write failed: {e}"))?;
        Ok(())
    }
    fn read_record(&mut self) -> Result<(u8, Vec<u8>), String> {
        let mut ignored = 0usize;
        loop {
            let (typ, body) = read_raw(&mut self.stream, None)?;
            if typ == 20 {
                ignored += 1;
                if ignored > MAX_IGNORED_RECORDS {
                    return Err("too many change-cipher-spec records".into());
                }
                continue;
            }
            if typ != 23 {
                return Err("secure record type is not application data".into());
            }
            if body.len() < 16 {
                return Err("secure record is truncated".into());
            }
            let header = [typ, 3, 3, (body.len() >> 8) as u8, body.len() as u8];
            let nonce = traffic_nonce(&self.read_iv, self.read_seq);
            let plain = aes_gcm_open(&self.read_key, &nonce, &header, &body).map_err(|_| "secure record authentication failed".to_string())?;
            self.read_seq = self.read_seq.checked_add(1).ok_or_else(|| "secure record sequence is exhausted".to_string())?;
            let mut data = plain;
            while data.last() == Some(&0) {
                data.pop();
            }
            let kind = data.pop().ok_or_else(|| "secure record has no content type".to_string())?;
            if kind == 21 {
                let desc = data.last().copied().unwrap_or(0);
                return Err(format!("the peer sent a secure alert ({desc})"));
            }
            return Ok((kind, data));
        }
    }
    fn accept_post_handshake(&mut self, payload: &[u8]) -> Result<(), String> {
        let requests = absorb_handshake(&mut self.post_hs, payload)?;
        for request in requests {
            let (key, iv, secret) = roll_traffic_secret(&self.read_secret)?;
            self.read_key = key;
            self.read_iv = iv;
            self.read_secret = secret;
            self.read_seq = 0;
            if request == 1 {
                self.reply_key_update = true;
            }
        }
        Ok(())
    }
    fn send_key_update(&mut self, request: u8) -> Result<(), String> {
        if !self.can_rotate_write_key() {
            return Err("traffic key record limit reached".into());
        }
        self.write_record_limited(22, &[24, 0, 0, 1, request], false)?;
        let (key, iv, secret) = roll_traffic_secret(&self.write_secret)?;
        self.write_key = key;
        self.write_iv = iv;
        self.write_secret = secret;
        self.write_seq = 0;
        self.write_updates = self.write_updates.saturating_add(1);
        Ok(())
    }
    #[cfg(any(test, rllm_module_tests))]
    pub fn request_key_update(&mut self) -> Result<(), String> {
        self.send_key_update(1)
    }
    #[cfg(any(test, rllm_module_tests))]
    fn write_handshake_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.write_record(22, bytes)
    }
    #[cfg(any(test, rllm_module_tests))]
    fn install_next_write_key(&mut self) -> Result<(), String> {
        let (key, iv, secret) = roll_traffic_secret(&self.write_secret)?;
        self.write_key = key;
        self.write_iv = iv;
        self.write_secret = secret;
        self.write_seq = 0;
        self.write_updates = self.write_updates.saturating_add(1);
        Ok(())
    }
}

fn roll_traffic_secret(secret: &[u8]) -> Result<([u8; 16], [u8; 12], Vec<u8>), String> {
    let next = hkdf_expand_label(secret, b"traffic upd", b"", 32)?;
    let (key, iv) = keys_of(&next)?;
    Ok((key, iv, next))
}

fn parse_key_update(payload: &[u8]) -> Result<u8, String> {
    if payload.len() != 5 || payload[0] != 24 {
        return Err("unexpected post-handshake message".into());
    }
    let len = u24(&payload[1..4]);
    if len != 1 {
        return Err("key update is not the required length".into());
    }
    if payload[4] > 1 {
        return Err("key update request is not a known value".into());
    }
    Ok(payload[4])
}

fn traffic_nonce(iv: &[u8; 12], seq: u64) -> [u8; 12] {
    let mut nonce = *iv;
    let bytes = seq.to_be_bytes();
    for i in 0..8 {
        nonce[4 + i] ^= bytes[i];
    }
    nonce
}

fn read_until(stream: &mut TcpStream, buf: &mut [u8], deadline: Option<std::time::Instant>) -> Result<(), String> {
    let mut filled = 0;
    while filled < buf.len() {
        if let Some(deadline) = deadline {
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err("the handshake deadline passed".into());
            }
            stream
                .set_read_timeout(Some(deadline.saturating_duration_since(now)))
                .map_err(|err| format!("secure read failed: {err}"))?;
        }
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return Err("secure read failed: connection closed".into()),
            Ok(n) => filled += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut || err.kind() == std::io::ErrorKind::WouldBlock => {
                if deadline.is_some_and(|limit| std::time::Instant::now() >= limit) {
                    return Err("the handshake deadline passed".into());
                }
                if deadline.is_none() {
                    return Err(format!("secure read failed: {err}"));
                }
            }
            Err(err) => return Err(format!("secure read failed: {err}")),
        }
    }
    Ok(())
}

fn write_until(stream: &mut TcpStream, mut data: &[u8], deadline: Option<std::time::Instant>) -> Result<(), String> {
    while !data.is_empty() {
        if let Some(deadline) = deadline {
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err("the handshake deadline passed".into());
            }
            stream
                .set_write_timeout(Some(deadline.saturating_duration_since(now)))
                .map_err(|err| format!("secure write failed: {err}"))?;
        }
        match stream.write(data) {
            Ok(0) => return Err("secure write failed: connection closed".into()),
            Ok(n) => data = &data[n..],
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut || err.kind() == std::io::ErrorKind::WouldBlock => {
                if deadline.is_some_and(|limit| std::time::Instant::now() >= limit) {
                    return Err("the handshake deadline passed".into());
                }
                if deadline.is_none() {
                    return Err(format!("secure write failed: {err}"));
                }
            }
            Err(err) => return Err(format!("secure write failed: {err}")),
        }
    }
    Ok(())
}

fn record_limit(kind: u8) -> usize {
    if kind == 23 {
        MAX_CIPHERTEXT_RECORD
    } else {
        MAX_PLAINTEXT_RECORD
    }
}

fn read_raw(stream: &mut TcpStream, deadline: Option<std::time::Instant>) -> Result<(u8, Vec<u8>), String> {
    let mut header = [0u8; 5];
    read_until(stream, &mut header, deadline)?;
    let len = u16::from_be_bytes([header[3], header[4]]) as usize;
    if len > record_limit(header[0]) {
        return Err("secure record is too large".into());
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        read_until(stream, &mut body, deadline)?;
    }
    Ok((header[0], body))
}

fn write_raw(stream: &mut TcpStream, kind: u8, payload: &[u8], legacy: bool, deadline: Option<std::time::Instant>) -> Result<(), String> {
    if payload.len() > 16384 {
        return Err("record exceeds the size budget".into());
    }
    let version = if legacy { 0x01 } else { 0x03 };
    let len = payload.len() as u16;
    let mut bytes = vec![kind, 0x03, version, (len >> 8) as u8, len as u8];
    bytes.extend_from_slice(payload);
    write_until(stream, &bytes, deadline)
}

fn remember_timeouts(stream: &TcpStream) -> (Option<std::time::Duration>, Option<std::time::Duration>) {
    (stream.read_timeout().ok().flatten(), stream.write_timeout().ok().flatten())
}

fn restore_timeouts(stream: &TcpStream, saved: (Option<std::time::Duration>, Option<std::time::Duration>)) {
    let _ = stream.set_read_timeout(saved.0);
    let _ = stream.set_write_timeout(saved.1);
}

fn hs(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![kind, (body.len() >> 16) as u8, (body.len() >> 8) as u8, body.len() as u8];
    out.extend_from_slice(body);
    out
}

fn u24(bytes: &[u8]) -> usize {
    ((bytes[0] as usize) << 16) | ((bytes[1] as usize) << 8) | bytes[2] as usize
}

fn within(buf_len: usize, start: usize, len: usize) -> bool {
    start.checked_add(len).map(|end| end <= buf_len).unwrap_or(false)
}

fn take_hs(buf: &mut Vec<u8>) -> Result<Option<(u8, Vec<u8>, Vec<u8>)>, String> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u24(&buf[1..4]);
    if len > MAX_HS_MESSAGE {
        return Err("handshake message exceeds the size budget".into());
    }
    if !within(buf.len(), 4, len) {
        return Ok(None);
    }
    let full = buf[..4 + len].to_vec();
    let kind = buf[0];
    let body = buf[4..4 + len].to_vec();
    buf.drain(..4 + len);
    Ok(Some((kind, body, full)))
}

fn keys_of(secret: &[u8]) -> Result<([u8; 16], [u8; 12]), String> {
    let key = hkdf_expand_label(secret, b"key", b"", 16)?;
    let iv = hkdf_expand_label(secret, b"iv", b"", 12)?;
    if key.len() != 16 || iv.len() != 12 {
        return Err("traffic key length is not the cipher size".into());
    }
    let mut k = [0u8; 16];
    let mut v = [0u8; 12];
    k.copy_from_slice(&key);
    v.copy_from_slice(&iv);
    Ok((k, v))
}

fn finished_data(secret: &[u8], transcript: &[u8]) -> Result<[u8; 32], String> {
    let key = hkdf_expand_label(secret, b"finished", b"", 32)?;
    Ok(hmac_sha256(&key, &sha256(transcript)))
}

fn new_flight(stream: TcpStream) -> Flight {
    Flight {
        stream,
        transcript: Vec::new(),
        read_key: None,
        write_key: None,
        hs_buf: Vec::new(),
        messages: 0,
        deadline: std::time::Instant::now() + HANDSHAKE_BUDGET,
    }
}

struct Flight {
    stream: TcpStream,
    transcript: Vec<u8>,
    read_key: Option<([u8; 16], [u8; 12], u64)>,
    write_key: Option<([u8; 16], [u8; 12], u64)>,
    hs_buf: Vec<u8>,
    messages: usize,
    deadline: std::time::Instant,
}

impl Flight {
    fn read_hs(&mut self) -> Result<(u8, Vec<u8>), String> {
        let mut empty_records = 0usize;
        loop {
            if std::time::Instant::now() >= self.deadline {
                return Err("the handshake deadline passed".into());
            }
            if self.messages >= MAX_HS_MESSAGES {
                return Err("handshake exceeded the message budget".into());
            }
            if let Some((kind, body, full)) = take_hs(&mut self.hs_buf)? {
                if self.transcript.len().saturating_add(full.len()) > MAX_TRANSCRIPT {
                    return Err("handshake transcript exceeded the memory budget".into());
                }
                self.transcript.extend_from_slice(&full);
                self.messages += 1;
                return Ok((kind, body));
            }
            if self.hs_buf.len() > MAX_TRANSCRIPT {
                return Err("handshake buffer exceeded the memory budget".into());
            }
            if let Some((key, iv, seq)) = self.read_key.as_mut() {
                let (kind, payload) = open_one(&mut self.stream, key, iv, seq, self.deadline)?;
                if kind != 22 {
                    return Err("expected a handshake record".into());
                }
                if payload.is_empty() {
                    empty_records += 1;
                    if empty_records > MAX_IGNORED_RECORDS {
                        return Err("handshake record was empty".into());
                    }
                } else {
                    empty_records = 0;
                }
                if self.hs_buf.len().saturating_add(payload.len()) > MAX_TRANSCRIPT {
                    return Err("handshake buffer exceeded the memory budget".into());
                }
                self.hs_buf.extend_from_slice(&payload);
            } else {
                let (typ, body) = read_raw(&mut self.stream, Some(self.deadline))?;
                if typ == 20 {
                    empty_records += 1;
                    if empty_records > MAX_IGNORED_RECORDS {
                        return Err("too many change-cipher-spec records".into());
                    }
                    continue;
                }
                if typ != 22 {
                    return Err("expected a clear handshake record".into());
                }
                if body.is_empty() {
                    empty_records += 1;
                    if empty_records > MAX_IGNORED_RECORDS {
                        return Err("handshake record was empty".into());
                    }
                } else {
                    empty_records = 0;
                }
                if self.hs_buf.len().saturating_add(body.len()) > MAX_TRANSCRIPT {
                    return Err("handshake buffer exceeded the memory budget".into());
                }
                self.hs_buf.extend_from_slice(&body);
            }
        }
    }
    fn write_hs(&mut self, message: &[u8], encrypted: bool) -> Result<(), String> {
        if message.len() < 4 || message.len() > MAX_HS_MESSAGE || self.transcript.len().saturating_add(message.len()) > MAX_TRANSCRIPT {
            return Err("handshake transcript exceeded the memory budget".into());
        }
        let declared = ((message[1] as usize) << 16) | ((message[2] as usize) << 8) | (message[3] as usize);
        if declared.saturating_add(4) != message.len() {
            return Err("handshake message length does not match its body".into());
        }
        self.transcript.extend_from_slice(message);
        if encrypted {
            let (key, iv, seq) = self.write_key.as_mut().ok_or_else(|| "write keys are not ready".to_string())?;
            seal_one(&mut self.stream, key, iv, seq, 22, message, Some(self.deadline))
        } else {
            write_raw(&mut self.stream, 22, message, true, Some(self.deadline))
        }
    }
}

fn open_one(stream: &mut TcpStream, key: &[u8; 16], iv: &[u8; 12], seq: &mut u64, deadline: std::time::Instant) -> Result<(u8, Vec<u8>), String> {
    let mut ignored = 0usize;
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("the handshake deadline passed".into());
        }
        let (typ, body) = read_raw(stream, Some(deadline))?;
        if typ == 20 {
            ignored += 1;
            if ignored > MAX_IGNORED_RECORDS {
                return Err("too many change-cipher-spec records".into());
            }
            continue;
        }
        if typ != 23 {
            return Err("expected an encrypted record".into());
        }
        let header = [typ, 3, 3, (body.len() >> 8) as u8, body.len() as u8];
        let nonce = traffic_nonce(iv, *seq);
        let plain = aes_gcm_open(key, &nonce, &header, &body).map_err(|_| "secure record authentication failed".to_string())?;
        *seq = seq.checked_add(1).ok_or_else(|| "secure record sequence is exhausted".to_string())?;
        let mut data = plain;
        while data.last() == Some(&0) {
            data.pop();
        }
        let kind = data.pop().ok_or_else(|| "secure record has no content type".to_string())?;
        if kind == 21 {
            return Err("the peer sent a secure alert".into());
        }
        return Ok((kind, data));
    }
}

fn seal_one(stream: &mut TcpStream, key: &[u8; 16], iv: &[u8; 12], seq: &mut u64, kind: u8, plain: &[u8], deadline: Option<std::time::Instant>) -> Result<(), String> {
    let mut inner = plain.to_vec();
    inner.push(kind);
    let length = match inner.len().checked_add(16) {
        Some(n) if n <= 16640 => n,
        _ => return Err("record exceeds the size budget".into()),
    };
    let header = [23u8, 3, 3, (length >> 8) as u8, length as u8];
    let nonce = traffic_nonce(iv, *seq);
    if *seq >= AES_GCM_RECORD_LIMIT {
        return Err("traffic key record limit reached".into());
    }
    let boxed = aes_gcm_seal(key, &nonce, &header, &inner)?;
    *seq += 1;
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&boxed);
    write_until(stream, &bytes, deadline)
}

const P256_P: [u32; 8] = [
    0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0x0000_0000, 0x0000_0000, 0x0000_0000, 0x0000_0001, 0xffff_ffff,
];
/// 2^256 - p, used to fold a carry off the top limb.
const P256_DELTA: [u32; 8] = [
    0x0000_0001, 0x0000_0000, 0x0000_0000, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_fffe, 0x0000_0000,
];
const P256_N: [u32; 8] = [
    0xfc63_2551, 0xf3b9_cac2, 0xa717_9e84, 0xbce6_faad, 0xffff_ffff, 0xffff_ffff, 0x0000_0000, 0xffff_ffff,
];
const P256_B_BE: [u8; 32] = [
    0x5a, 0xc6, 0x35, 0xd8, 0xaa, 0x3a, 0x93, 0xe7, 0xb3, 0xeb, 0xbd, 0x55, 0x76, 0x98, 0x86, 0xbc, 0x65, 0x1d, 0x06, 0xb0, 0xcc, 0x53, 0xb0, 0xf6, 0x3b, 0xce, 0x3c, 0x3e, 0x27, 0xd2, 0x60, 0x4b,
];
const P256_GX_BE: [u8; 32] = [
    0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98, 0xc2, 0x96,
];
const P256_GY_BE: [u8; 32] = [
    0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e, 0x16, 0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf, 0x51, 0xf5,
];

fn be_limbs(bytes: &[u8; 32]) -> [u32; 8] {
    let mut limbs = [0u32; 8];
    for i in 0..8 {
        let o = (7 - i) * 4;
        limbs[i] = u32::from_be_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    }
    limbs
}

fn limbs_be(limbs: &[u32; 8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..8 {
        let o = (7 - i) * 4;
        out[o..o + 4].copy_from_slice(&limbs[i].to_be_bytes());
    }
    out
}

fn u32_add(left: &[u32; 8], right: &[u32; 8]) -> ([u32; 8], u32) {
    let mut out = [0u32; 8];
    let mut carry = 0u64;
    for i in 0..8 {
        let sum = u64::from(left[i]) + u64::from(right[i]) + carry;
        out[i] = sum as u32;
        carry = sum >> 32;
    }
    (out, carry as u32)
}

fn u32_sub(left: &[u32; 8], right: &[u32; 8]) -> ([u32; 8], u32) {
    let mut out = [0u32; 8];
    let mut borrow = 0u64;
    for i in 0..8 {
        let lhs = u64::from(left[i]);
        let rhs = u64::from(right[i]) + borrow;
        let diff = lhs.wrapping_sub(rhs);
        out[i] = diff as u32;
        borrow = u64::from(lhs < rhs);
    }
    (out, borrow as u32)
}

fn p256_select(mask: u32, left: &[u32; 8], right: &[u32; 8]) -> [u32; 8] {
    let bits = 0u32.wrapping_sub(mask & 1);
    let mut out = [0u32; 8];
    for i in 0..8 {
        out[i] = (left[i] & bits) | (right[i] & !bits);
    }
    out
}

fn p256_cond_sub(value: &[u32; 8]) -> [u32; 8] {
    let (diff, borrow) = u32_sub(value, &P256_P);
    p256_select(borrow ^ 1, &diff, value)
}

fn p256_add(left: &[u32; 8], right: &[u32; 8]) -> [u32; 8] {
    let (sum, carry) = u32_add(left, right);
    let (folded, _) = u32_add(&sum, &P256_DELTA);
    p256_select(carry, &folded, &p256_cond_sub(&sum))
}

fn p256_sub(left: &[u32; 8], right: &[u32; 8]) -> [u32; 8] {
    let (diff, borrow) = u32_sub(left, right);
    let (lifted, _) = u32_add(&diff, &P256_P);
    p256_select(borrow, &lifted, &diff)
}

fn p256_product(left: &[u32; 8], right: &[u32; 8]) -> [u32; 16] {
    // Eight extra limbs keep every carry step in range. Each row always takes eight carry steps.
    let mut acc = [0u64; 24];
    for i in 0..8 {
        let mut carry = 0u64;
        for j in 0..8 {
            let sum = acc[i + j] + u64::from(left[i]) * u64::from(right[j]) + carry;
            acc[i + j] = u64::from(sum as u32);
            carry = sum >> 32;
        }
        for step in 0..8 {
            let sum = acc[i + 8 + step] + carry;
            acc[i + 8 + step] = u64::from(sum as u32);
            carry = sum >> 32;
        }
    }
    let mut out = [0u32; 16];
    for i in 0..16 {
        out[i] = acc[i] as u32;
    }
    out
}

fn p256_fold(low: &[u32; 8], high: &[u32; 8]) -> ([u32; 8], [u32; 8]) {
    let prod = p256_product(high, &P256_DELTA);
    let mut next_low = [0u32; 8];
    let mut next_high = [0u32; 8];
    let mut carry = 0u64;
    for i in 0..8 {
        let sum = u64::from(prod[i]) + u64::from(low[i]) + carry;
        next_low[i] = sum as u32;
        carry = sum >> 32;
    }
    for i in 0..8 {
        let sum = u64::from(prod[i + 8]) + carry;
        next_high[i] = sum as u32;
        carry = sum >> 32;
    }
    let _ = carry;
    (next_low, next_high)
}

fn p256_reduce_wide(mut low: [u32; 8], mut high: [u32; 8]) -> [u32; 8] {
    for _ in 0..10 {
        let (next_low, next_high) = p256_fold(&low, &high);
        low = next_low;
        high = next_high;
    }
    let mut out = low;
    for _ in 0..4 {
        out = p256_cond_sub(&out);
    }
    out
}

fn p256_mul(left: &[u32; 8], right: &[u32; 8]) -> [u32; 8] {
    let prod = p256_product(left, right);
    let mut low = [0u32; 8];
    let mut high = [0u32; 8];
    low.copy_from_slice(&prod[..8]);
    high.copy_from_slice(&prod[8..]);
    p256_reduce_wide(low, high)
}

fn p256_sqr(value: &[u32; 8]) -> [u32; 8] {
    p256_mul(value, value)
}

fn p256_zero() -> [u32; 8] {
    [0; 8]
}

fn p256_one() -> [u32; 8] {
    let mut one = p256_zero();
    one[0] = 1;
    one
}

fn p256_three() -> [u32; 8] {
    let mut three = p256_zero();
    three[0] = 3;
    three
}

fn p256_eq(left: &[u32; 8], right: &[u32; 8]) -> u32 {
    let mut mixed = 0u32;
    for i in 0..8 {
        mixed |= left[i] ^ right[i];
    }
    mixed |= mixed >> 16;
    mixed |= mixed >> 8;
    mixed |= mixed >> 4;
    mixed |= mixed >> 2;
    mixed |= mixed >> 1;
    (mixed & 1) ^ 1
}

fn p256_inv(value: &[u32; 8]) -> [u32; 8] {
    let (exp, _) = u32_sub(&P256_P, &[2, 0, 0, 0, 0, 0, 0, 0]);
    let mut acc = p256_one();
    for index in (0..256).rev() {
        acc = p256_sqr(&acc);
        let bit = (exp[index / 32] >> (index % 32)) & 1;
        let product = p256_mul(&acc, value);
        acc = p256_select(bit, &product, &acc);
    }
    acc
}

fn p256_lt(value: &[u32; 8]) -> bool {
    let (_, borrow) = u32_sub(value, &P256_P);
    borrow == 1
}

struct P256Point {
    x: [u32; 8],
    y: [u32; 8],
    inf: u32,
}

fn p256_infinity() -> P256Point {
    P256Point { x: p256_zero(), y: p256_zero(), inf: 1 }
}

fn p256_select_point(mask: u32, left: &P256Point, right: &P256Point) -> P256Point {
    let bits = 0u32.wrapping_sub(mask & 1);
    P256Point {
        x: p256_select(mask, &left.x, &right.x),
        y: p256_select(mask, &left.y, &right.y),
        inf: (left.inf & bits) | (right.inf & !bits),
    }
}

struct P256Jac {
    x: [u32; 8],
    y: [u32; 8],
    z: [u32; 8],
}

fn p256_jac_select(mask: u32, left: &P256Jac, right: &P256Jac) -> P256Jac {
    P256Jac {
        x: p256_select(mask, &left.x, &right.x),
        y: p256_select(mask, &left.y, &right.y),
        z: p256_select(mask, &left.z, &right.z),
    }
}

fn p256_jac_double(point: &P256Jac) -> P256Jac {
    let delta = p256_sqr(&point.z);
    let gamma = p256_sqr(&point.y);
    let beta = p256_mul(&point.x, &gamma);
    let alpha = p256_mul(&p256_three(), &p256_mul(&p256_sub(&point.x, &delta), &p256_add(&point.x, &delta)));
    let beta8 = p256_double_limb(&p256_double_limb(&p256_double_limb(&beta)));
    let x3 = p256_sub(&p256_sqr(&alpha), &beta8);
    let z_sum = p256_add(&point.y, &point.z);
    let z3 = p256_sub(&p256_sub(&p256_sqr(&z_sum), &gamma), &delta);
    let four_beta = p256_double_limb(&p256_double_limb(&beta));
    let gamma2 = p256_sqr(&gamma);
    let eight_gamma2 = p256_double_limb(&p256_double_limb(&p256_double_limb(&gamma2)));
    let y3 = p256_sub(&p256_mul(&alpha, &p256_sub(&four_beta, &x3)), &eight_gamma2);
    P256Jac { x: x3, y: y3, z: z3 }
}

fn p256_double_limb(value: &[u32; 8]) -> [u32; 8] {
    p256_add(value, value)
}

fn p256_jac_add_affine(point: &P256Jac, affine: &P256Point) -> P256Jac {
    let z1z1 = p256_sqr(&point.z);
    let u2 = p256_mul(&affine.x, &z1z1);
    let z1cu = p256_mul(&point.z, &z1z1);
    let s2 = p256_mul(&affine.y, &z1cu);
    let h = p256_sub(&u2, &point.x);
    let hh = p256_sqr(&h);
    let i = p256_double_limb(&p256_double_limb(&hh));
    let j = p256_mul(&h, &i);
    let r = p256_double_limb(&p256_sub(&s2, &point.y));
    let v = p256_mul(&point.x, &i);
    let x3 = p256_sub(&p256_sub(&p256_sqr(&r), &j), &p256_double_limb(&v));
    let y3 = p256_sub(&p256_mul(&r, &p256_sub(&v, &x3)), &p256_double_limb(&p256_mul(&point.y, &j)));
    let z_sum = p256_add(&point.z, &h);
    let z3 = p256_sub(&p256_sub(&p256_sqr(&z_sum), &z1z1), &hh);
    let added = P256Jac { x: x3, y: y3, z: z3 };
    let doubled = p256_jac_double(point);
    let h_zero = p256_eq(&h, &p256_zero());
    let same_y = p256_eq(&s2, &point.y);
    let mut result = p256_jac_select(h_zero & same_y, &doubled, &added);
    result = p256_jac_select(h_zero & (same_y ^ 1), &P256Jac { x: p256_one(), y: p256_one(), z: p256_zero() }, &result);
    let from_affine = P256Jac { x: affine.x, y: affine.y, z: p256_one() };
    p256_jac_select(p256_eq(&point.z, &p256_zero()), &from_affine, &result)
}

fn p256_jac_affine(point: &P256Jac) -> P256Point {
    let z_zero = p256_eq(&point.z, &p256_zero());
    let zinv = p256_inv(&point.z);
    let zinv2 = p256_sqr(&zinv);
    let zinv3 = p256_mul(&zinv2, &zinv);
    let x = p256_mul(&point.x, &zinv2);
    let y = p256_mul(&point.y, &zinv3);
    p256_select_point(z_zero, &p256_infinity(), &P256Point { x, y, inf: 0 })
}

fn p256_base() -> P256Point {
    P256Point { x: be_limbs(&P256_GX_BE), y: be_limbs(&P256_GY_BE), inf: 0 }
}

fn p256_on_curve(x: &[u32; 8], y: &[u32; 8]) -> bool {
    let y2 = p256_sqr(y);
    let x2 = p256_sqr(x);
    let x3 = p256_mul(&x2, x);
    let three_x = p256_mul(x, &p256_three());
    let right = p256_add(&p256_sub(&x3, &three_x), &be_limbs(&P256_B_BE));
    p256_eq(&y2, &right) == 1
}

fn p256_mul_point(point: &P256Point, scalar: &[u8; 32]) -> P256Point {
    let mut acc = P256Jac { x: p256_one(), y: p256_one(), z: p256_zero() };
    for index in (0..256).rev() {
        acc = p256_jac_double(&acc);
        let added = p256_jac_add_affine(&acc, point);
        let bit = u32::from((scalar[31 - (index / 8)] >> (index % 8)) & 1);
        acc = p256_jac_select(bit, &added, &acc);
    }
    p256_jac_affine(&acc)
}

fn p256_reduce_scalar(seed: &[u8; 32]) -> Result<[u8; 32], String> {
    let rem = ct_mod_bytes(seed, &P256_N);
    let mut out = [0u8; 32];
    let mut nonzero = 0u32;
    for i in 0..8 {
        let limb = rem.get(i).copied().unwrap_or(0);
        nonzero |= limb;
        let o = (7 - i) * 4;
        out[o..o + 4].copy_from_slice(&limb.to_be_bytes());
    }
    if nonzero == 0 {
        return Err("the key agreement scalar was not usable".into());
    }
    Ok(out)
}

fn p256_keypair(seed: &[u8; 32]) -> Result<([u8; 32], [u8; 65]), String> {
    let scalar = p256_reduce_scalar(seed)?;
    let point = p256_mul_point(&p256_base(), &scalar);
    if point.inf == 1 {
        return Err("the key agreement scalar was not usable".into());
    }
    Ok((scalar, p256_encode(&point)?))
}

fn p256_encode(point: &P256Point) -> Result<[u8; 65], String> {
    if point.inf == 1 {
        return Err("the key agreement point is at infinity".into());
    }
    let mut out = [0u8; 65];
    out[0] = 4;
    out[1..33].copy_from_slice(&limbs_be(&point.x));
    out[33..65].copy_from_slice(&limbs_be(&point.y));
    Ok(out)
}

fn p256_decode(bytes: &[u8]) -> Result<P256Point, String> {
    if bytes.len() != 65 || bytes[0] != 4 {
        return Err("the peer key share is not an uncompressed point".into());
    }
    let mut xb = [0u8; 32];
    let mut yb = [0u8; 32];
    xb.copy_from_slice(&bytes[1..33]);
    yb.copy_from_slice(&bytes[33..65]);
    let x = be_limbs(&xb);
    let y = be_limbs(&yb);
    if !p256_lt(&x) || !p256_lt(&y) {
        return Err("the peer key share is outside the field".into());
    }
    if p256_eq(&x, &p256_zero()) == 1 && p256_eq(&y, &p256_zero()) == 1 {
        return Err("the peer key share is the point at infinity".into());
    }
    if !p256_on_curve(&x, &y) {
        return Err("the peer key share is not on the curve".into());
    }
    Ok(P256Point { x, y, inf: 0 })
}

fn p256_agree(scalar: &[u8; 32], peer: &[u8]) -> Result<[u8; 32], String> {
    let point = p256_decode(peer)?;
    let shared = p256_mul_point(&point, scalar);
    if shared.inf == 1 {
        return Err("the key agreement produced an invalid shared secret".into());
    }
    Ok(limbs_be(&shared.x))
}

fn p256_offer() -> Result<([u8; 32], [u8; 65]), String> {
    for _ in 0..4 {
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&random_bytes(32)?);
        if let Ok(pair) = p256_keypair(&seed) {
            return Ok(pair);
        }
    }
    Err("the key agreement scalar was not usable".into())
}

#[derive(Debug)]
enum PeerShare {
    X25519([u8; 32]),
    P256([u8; 65]),
}

fn agree_share(x_scalar: &[u8; 32], p_scalar: &[u8; 32], peer: PeerShare) -> Result<[u8; 32], String> {
    match peer {
        PeerShare::X25519(point) => {
            let shared = x25519(x_scalar, &point);
            if shared.iter().all(|byte| *byte == 0) {
                return Err("the key agreement produced an invalid shared secret".into());
            }
            Ok(shared)
        }
        PeerShare::P256(point) => p256_agree(p_scalar, &point),
    }
}

fn absorb_handshake(buf: &mut Vec<u8>, payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.is_empty() {
        return Err("handshake fragment is empty".into());
    }
    if buf.len().saturating_add(payload.len()) > MAX_HS_MESSAGE {
        return Err("post-handshake message exceeds the size budget".into());
    }
    buf.extend_from_slice(payload);
    let mut updates = Vec::new();
    loop {
        if buf.len() < 4 {
            return Ok(updates);
        }
        let len = u24(&buf[1..4]);
        if len > MAX_HS_MESSAGE {
            return Err("post-handshake message exceeds the size budget".into());
        }
        let total = match len.checked_add(4) {
            Some(n) => n,
            None => return Err("post-handshake message exceeds the size budget".into()),
        };
        if buf.len() < total {
            return Ok(updates);
        }
        let kind = buf[0];
        let aligned = buf.len() == total;
        let body = buf[4..total].to_vec();
        buf.drain(..total);
        match kind {
            4 => ignore_new_session_ticket(&body)?,
            24 => {
                if !aligned {
                    return Err("key update is not aligned to a record boundary".into());
                }
                if body.len() != 1 || body[0] > 1 {
                    return Err("key update request is not a known value".into());
                }
                updates.push(body[0]);
            }
            other => return Err(format!("unexpected post-handshake message {other}")),
        }
    }
}

fn ignore_new_session_ticket(body: &[u8]) -> Result<(), String> {
    if body.len() < 12 {
        return Err("session ticket is truncated".into());
    }
    let mut i = 8;
    let nonce_len = body[i] as usize;
    i += 1;
    if !within(body.len(), i, nonce_len) {
        return Err("session ticket is truncated".into());
    }
    i += nonce_len;
    if !within(body.len(), i, 2) {
        return Err("session ticket is truncated".into());
    }
    let ticket_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    i += 2;
    if ticket_len < 1 || !within(body.len(), i, ticket_len) {
        return Err("session ticket is truncated".into());
    }
    i += ticket_len;
    if !within(body.len(), i, 2) {
        return Err("session ticket is truncated".into());
    }
    let ext_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    i += 2;
    if i.checked_add(ext_len) != Some(body.len()) {
        return Err("session ticket length does not match".into());
    }
    extensions(&body[i..])?;
    Ok(())
}

pub fn client_handshake(stream: TcpStream, key: &PrivateKey, pin: Option<&[u8]>) -> Result<SecurePipe, String> {
    let saved = remember_timeouts(&stream);
    let mut flight = new_flight(stream);
    let mut random = [0u8; 32];
    random.copy_from_slice(&random_bytes(32)?);
    let mut session = [0u8; 32];
    session.copy_from_slice(&random_bytes(32)?);
    let mut x_scalar = [0u8; 32];
    x_scalar.copy_from_slice(&random_bytes(32)?);
    let mut base = [0u8; 32];
    base[0] = 9;
    let x_share = x25519(&x_scalar, &base);
    let (mut p_scalar, p_share) = p256_offer()?;
    let hello = client_hello(&random, &session, &x_share, &p_share);
    flight.write_hs(&hello, false)?;
    let first_hello_len = flight.transcript.len();
    write_raw(&mut flight.stream, 20, &[1], false, Some(flight.deadline))?;
    let mut x_public = x_share;
    let mut p_public = p_share;
    let mut had_x = true;
    let mut had_p = true;
    let mut retried = false;
    let mut required_group = None;
    let peer_share = loop {
        let (kind, server_hello) = flight.read_hs()?;
        if kind != 2 {
            return Err("the peer did not send a server hello".into());
        }
        if is_hello_retry(&server_hello) {
            if retried {
                return Err("the peer sent a second hello retry".into());
            }
            retried = true;
            let plan = parse_hello_retry(&server_hello, &session)?;
            fold_retry_transcript(&mut flight.transcript, first_hello_len)?;
            let updated = updated_client_hello(&random, &session, &mut x_scalar, &mut x_public, &mut p_scalar, &mut p_public, &mut had_x, &mut had_p, &plan)?;
            flight.write_hs(&updated, false)?;
            required_group = plan.selected;
            continue;
        }
        let share = parse_server_hello(&server_hello, &session)?;
        if let Some(group) = required_group {
            let actual = match &share {
                PeerShare::X25519(_) => GROUP_X25519,
                PeerShare::P256(_) => GROUP_P256,
            };
            if actual != group {
                return Err("the peer selected a different group after the retry".into());
            }
        }
        break share;
    };
    let shared = agree_share(&x_scalar, &p_scalar, peer_share)?;
    let (c_hs, s_hs, master) = handshake_secrets(&shared, &flight.transcript)?;
    let (rk, riv) = keys_of(&s_hs)?;
    let (wk, wiv) = keys_of(&c_hs)?;
    flight.read_key = Some((rk, riv, 0));
    flight.write_key = Some((wk, wiv, 0));
    let mut request_context = None;
    let mut peer_cert = Vec::new();
    let mut stage = 0u8;
    loop {
        let (kind, body) = flight.read_hs()?;
        match (stage, kind) {
            (0, 8) => {
                parse_encrypted_extensions(&body)?;
                stage = 1;
            }
            (1, 13) => {
                parse_certificate_request(&body)?;
                request_context = Some(body);
                stage = 2;
            }
            (1, 11) | (2, 11) => {
                peer_cert = first_cert(&body)?;
                if peer_cert.is_empty() {
                    return Err("the peer did not present a certificate".into());
                }
                stage = 3;
            }
            (3, 15) => {
                verify_cert_sig(&peer_cert, &body, &flight.transcript, b"TLS 1.3, server CertificateVerify\0")?;
                stage = 4;
            }
            (4, 20) => {
                if body.len() != 32 {
                    return Err("the peer finished value did not match".into());
                }
                let end = flight.transcript.len().checked_sub(body.len().saturating_add(4)).ok_or_else(|| "the peer finished value did not match".to_string())?;
                let expect = finished_data(&s_hs, &flight.transcript[..end])?;
                if !tags_equal(&expect, &body) {
                    return Err("the peer finished value did not match".into());
                }
                break;
            }
            (_, 8) => return Err("encrypted extensions were repeated".into()),
            (_, 15) | (_, 20) => return Err("the peer did not prove possession of the certificate key".into()),
            (_, other) => return Err(format!("unexpected handshake message {other}")),
        }
    }
    if stage != 4 || peer_cert.is_empty() {
        return Err("the peer did not prove possession of the certificate key".into());
    }
    // The pairing connection and the later transport connection present
    // certificates from different keys. A saved pairing certificate is not
    // required to be the same bytes as this peer certificate. Possession of
    // the peer certificate was already required above; a certificate that
    // does not prove possession is still refused there.
    let _saved_pairing_certificate = pin;
    let hash_at_server_finished = sha256(&flight.transcript);
    let exporter = derive_secret(&master, b"exp master", &hash_at_server_finished)?;
    let c_ap = derive_secret(&master, b"c ap traffic", &hash_at_server_finished)?;
    let s_ap = derive_secret(&master, b"s ap traffic", &hash_at_server_finished)?;
    if let Some(req) = request_context {
        let ctx_len = req.first().copied().unwrap_or(0) as usize;
        if !within(req.len(), 1, ctx_len) {
            return Err("certificate request is truncated".into());
        }
        let context = &req[1..1 + ctx_len];
        let cert = key.certificate("local")?;
        let message = certificate_message(context, &cert);
        flight.write_hs(&message, true)?;
        let verify = certificate_verify(key, b"TLS 1.3, client CertificateVerify\0", &flight.transcript)?;
        flight.write_hs(&verify, true)?;
    }
    let client_finished = finished_data(&c_hs, &flight.transcript)?;
    flight.write_hs(&hs(20, &client_finished), true)?;
    let (wk, wiv) = keys_of(&c_ap)?;
    let (rk, riv) = keys_of(&s_ap)?;
    restore_timeouts(&flight.stream, saved);
    Ok(SecurePipe {
        stream: flight.stream,
        write_key: wk,
        write_iv: wiv,
        write_seq: 0,
        read_key: rk,
        read_iv: riv,
        read_seq: 0,
        write_secret: c_ap,
        read_secret: s_ap,
        pending: Vec::new(),
        post_hs: Vec::new(),
        exporter,
        peer_cert,
        reply_key_update: false,
        write_updates: 0,
    })
}

#[cfg(any(test, rllm_module_tests))]
pub fn server_handshake(stream: TcpStream, key: &PrivateKey) -> Result<SecurePipe, String> {
    server_handshake_with(stream, key, false, None)
}

#[cfg(any(test, rllm_module_tests))]
pub fn server_handshake_p256(stream: TcpStream, key: &PrivateKey) -> Result<SecurePipe, String> {
    server_handshake_with(stream, key, true, None)
}

#[cfg(any(test, rllm_module_tests))]
fn server_handshake_cookie(stream: TcpStream, key: &PrivateKey, cookie: &[u8]) -> Result<SecurePipe, String> {
    server_handshake_with(stream, key, false, Some(cookie))
}

#[cfg(any(test, rllm_module_tests))]
fn server_handshake_with(stream: TcpStream, key: &PrivateKey, p256_only: bool, cookie: Option<&[u8]>) -> Result<SecurePipe, String> {
    let saved = remember_timeouts(&stream);
    let mut flight = new_flight(stream);
    let (kind, hello) = flight.read_hs()?;
    if kind != 1 {
        return Err("the peer did not send a client hello".into());
    }
    let mut offered = parse_client_hello(&hello)?;
    if let Some(cookie) = cookie {
        if cookie.is_empty() {
            return Err("hello retry cookie is empty".into());
        }
        let hrr = hello_retry(&offered.session, Some(cookie), None)?;
        let first_len = flight.transcript.len();
        flight.write_hs(&hrr, false)?;
        fold_retry_transcript(&mut flight.transcript, first_len)?;
        let (kind2, hello2) = flight.read_hs()?;
        if kind2 != 1 {
            return Err("the peer did not send an updated client hello".into());
        }
        let echoed = client_extension(&hello2, 0x002c)?;
        if echoed.as_deref() != Some(cookie) {
            return Err("updated client hello did not copy the cookie".into());
        }
        offered = parse_client_hello(&hello2)?;
    }
    let mut random = [0u8; 32];
    random.copy_from_slice(&random_bytes(32)?);
    let (server_hello, shared) = server_share(&offered, &random, p256_only)?;
    flight.write_hs(&server_hello, false)?;
    write_raw(&mut flight.stream, 20, &[1], false, Some(flight.deadline))?;
    let (c_hs, s_hs, master) = handshake_secrets(&shared, &flight.transcript)?;
    let (wk, wiv) = keys_of(&s_hs)?;
    let (rk, riv) = keys_of(&c_hs)?;
    flight.write_key = Some((wk, wiv, 0));
    flight.read_key = Some((rk, riv, 0));
    flight.write_hs(&hs(8, &0u16.to_be_bytes()), true)?;
    let mut req = vec![0u8];
    let mut ext = Vec::new();
    ext.extend_from_slice(&0x000du16.to_be_bytes());
    ext.extend_from_slice(&4u16.to_be_bytes());
    ext.extend_from_slice(&2u16.to_be_bytes());
    ext.extend_from_slice(&0x0804u16.to_be_bytes());
    req.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    req.extend_from_slice(&ext);
    flight.write_hs(&hs(13, &req), true)?;
    let cert = key.certificate("peer")?;
    flight.write_hs(&certificate_message(&[], &cert), true)?;
    let verify = certificate_verify(key, b"TLS 1.3, server CertificateVerify\0", &flight.transcript)?;
    flight.write_hs(&verify, true)?;
    let finished = finished_data(&s_hs, &flight.transcript)?;
    flight.write_hs(&hs(20, &finished), true)?;
    let hash_now = sha256(&flight.transcript);
    let exporter = derive_secret(&master, b"exp master", &hash_now)?;
    let c_ap = derive_secret(&master, b"c ap traffic", &hash_now)?;
    let s_ap = derive_secret(&master, b"s ap traffic", &hash_now)?;
    let mut client_cert = Vec::new();
    let mut stage = 0u8;
    loop {
        let (kind, body) = flight.read_hs()?;
        match (stage, kind) {
            (0, 11) => {
                client_cert = first_cert(&body)?;
                if client_cert.is_empty() {
                    return Err("the peer did not present a certificate".into());
                }
                stage = 1;
            }
            (1, 15) => {
                verify_cert_sig(&client_cert, &body, &flight.transcript, b"TLS 1.3, client CertificateVerify\0")?;
                stage = 2;
            }
            (2, 20) => {
                if body.len() != 32 {
                    return Err("the peer finished value did not match".into());
                }
                let end = flight.transcript.len().checked_sub(body.len().saturating_add(4)).ok_or_else(|| "the peer finished value did not match".to_string())?;
                let expect = finished_data(&c_hs, &flight.transcript[..end])?;
                if !tags_equal(&expect, &body) {
                    return Err("the peer finished value did not match".into());
                }
                break;
            }
            (_, 15) | (_, 20) => return Err("the peer did not prove possession of the certificate key".into()),
            (_, other) => return Err(format!("unexpected client handshake message {other}")),
        }
    }
    if stage != 2 {
        return Err("the peer did not prove possession of the certificate key".into());
    }
    let (wk, wiv) = keys_of(&s_ap)?;
    let (rk, riv) = keys_of(&c_ap)?;
    restore_timeouts(&flight.stream, saved);
    Ok(SecurePipe {
        stream: flight.stream,
        write_key: wk,
        write_iv: wiv,
        write_seq: 0,
        read_key: rk,
        read_iv: riv,
        read_seq: 0,
        write_secret: s_ap,
        read_secret: c_ap,
        pending: Vec::new(),
        post_hs: Vec::new(),
        exporter,
        peer_cert: client_cert,
        reply_key_update: false,
        write_updates: 0,
    })
}

fn handshake_secrets(shared: &[u8; 32], transcript: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let zeros = [0u8; 32];
    let early = hkdf_extract(&zeros, &zeros);
    let empty = sha256(b"");
    let derived = derive_secret(&early, b"derived", &empty)?;
    let handshake = hkdf_extract(&derived, shared);
    let th = sha256(transcript);
    let c_hs = derive_secret(&handshake, b"c hs traffic", &th)?;
    let s_hs = derive_secret(&handshake, b"s hs traffic", &th)?;
    let derived2 = derive_secret(&handshake, b"derived", &empty)?;
    let master = hkdf_extract(&derived2, &zeros);
    Ok((c_hs, s_hs, master.to_vec()))
}

fn client_hello(random: &[u8; 32], session: &[u8; 32], x_share: &[u8; 32], p_share: &[u8; 65]) -> Vec<u8> {
    match client_hello_ext(random, session, Some(x_share), Some(p_share), None) {
        Ok(hello) => hello,
        Err(err) => panic!("client hello: {err}"),
    }
}

fn client_hello_ext(random: &[u8; 32], session: &[u8; 32], x_share: Option<&[u8; 32]>, p_share: Option<&[u8; 65]>, cookie: Option<&[u8]>) -> Result<Vec<u8>, String> {
    if x_share.is_none() && p_share.is_none() {
        return Err("the client hello has no key share".into());
    }
    if cookie.is_some_and(|value| value.is_empty() || value.len() > 65_000) {
        return Err("hello retry cookie is outside the supported size".into());
    }
    let mut body = Vec::new();
    body.extend_from_slice(&0x0303u16.to_be_bytes());
    body.extend_from_slice(random);
    body.push(32);
    body.extend_from_slice(session);
    body.extend_from_slice(&2u16.to_be_bytes());
    body.extend_from_slice(&0x1301u16.to_be_bytes());
    body.push(1);
    body.push(0);
    let mut ext = Vec::new();
    if let Some(cookie) = cookie {
        ext.extend(extension(0x002c, cookie));
    }
    ext.extend(extension(0x002b, &[0x02, 0x03, 0x04]));
    let mut groups = Vec::new();
    groups.extend_from_slice(&4u16.to_be_bytes());
    groups.extend_from_slice(&GROUP_X25519.to_be_bytes());
    groups.extend_from_slice(&GROUP_P256.to_be_bytes());
    ext.extend(extension(0x000a, &groups));
    let mut sigs = Vec::new();
    sigs.extend_from_slice(&2u16.to_be_bytes());
    sigs.extend_from_slice(&0x0804u16.to_be_bytes());
    ext.extend(extension(0x000d, &sigs));
    let mut entries = Vec::new();
    if let Some(share) = x_share {
        entries.extend_from_slice(&GROUP_X25519.to_be_bytes());
        entries.extend_from_slice(&32u16.to_be_bytes());
        entries.extend_from_slice(share);
    }
    if let Some(share) = p_share {
        entries.extend_from_slice(&GROUP_P256.to_be_bytes());
        entries.extend_from_slice(&65u16.to_be_bytes());
        entries.extend_from_slice(share);
    }
    let mut ks = Vec::new();
    if entries.len() > u16::MAX as usize {
        return Err("client hello key share exceeds the size field".into());
    }
    ks.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    ks.extend_from_slice(&entries);
    ext.extend(extension(0x0033, &ks));
    if ext.len() > u16::MAX as usize {
        return Err("client hello extensions exceed the size field".into());
    }
    body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    body.extend_from_slice(&ext);
    Ok(hs(1, &body))
}

#[cfg_attr(not(test), allow(dead_code))]
fn server_hello(random: &[u8; 32], session: &[u8; 32], share: &[u8; 32]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0x0303u16.to_be_bytes());
    body.extend_from_slice(random);
    body.push(32);
    body.extend_from_slice(session);
    body.extend_from_slice(&0x1301u16.to_be_bytes());
    body.push(0);
    let mut ext = Vec::new();
    ext.extend(extension(0x002b, &[0x03, 0x04]));
    let mut ks = Vec::new();
    ks.extend_from_slice(&0x001du16.to_be_bytes());
    ks.extend_from_slice(&32u16.to_be_bytes());
    ks.extend_from_slice(share);
    ext.extend(extension(0x0033, &ks));
    body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    body.extend_from_slice(&ext);
    hs(2, &body)
}

fn extension(kind: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&(data.len() as u16).to_be_bytes());
    out.extend_from_slice(data);
    out
}

fn is_hello_retry(body: &[u8]) -> bool {
    body.len() >= 34 && body[2..34] == HRR_RANDOM
}

struct RetryPlan {
    cookie: Option<Vec<u8>>,
    selected: Option<u16>,
}

struct RetryChoice {
    send_x: bool,
    send_p: bool,
    fresh_x: bool,
    fresh_p: bool,
}

#[cfg_attr(not(any(test, rllm_module_tests)), allow(dead_code))]
fn hello_retry(session: &[u8; 32], cookie: Option<&[u8]>, group: Option<u16>) -> Result<Vec<u8>, String> {
    if cookie.is_some_and(|value| value.is_empty() || value.len() > 65_000) {
        return Err("hello retry cookie is outside the supported size".into());
    }
    if cookie.is_none() && group.is_none() {
        return Err("hello retry would not change the client hello".into());
    }
    let mut body = Vec::new();
    body.extend_from_slice(&0x0303u16.to_be_bytes());
    body.extend_from_slice(&HRR_RANDOM);
    body.push(session.len() as u8);
    body.extend_from_slice(session);
    body.extend_from_slice(&0x1301u16.to_be_bytes());
    body.push(0);
    let mut ext = Vec::new();
    ext.extend(extension(0x002b, &[0x03, 0x04]));
    if let Some(cookie) = cookie {
        ext.extend(extension(0x002c, cookie));
    }
    if let Some(group) = group {
        ext.extend(extension(0x0033, &group.to_be_bytes()));
    }
    if ext.len() > u16::MAX as usize {
        return Err("hello retry extensions exceed the size field".into());
    }
    body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    body.extend_from_slice(&ext);
    Ok(hs(2, &body))
}

fn parse_hello_retry(body: &[u8], session: &[u8; 32]) -> Result<RetryPlan, String> {
    if !is_hello_retry(body) {
        return Err("the peer hello is not a retry".into());
    }
    if body.len() < 34 + 1 + 32 + 3 {
        return Err("hello retry is truncated".into());
    }
    if u16::from_be_bytes([body[0], body[1]]) != 0x0303 {
        return Err("hello retry legacy version is not the required value".into());
    }
    let sid_len = body[34] as usize;
    if sid_len > 32 || !within(body.len(), 35, sid_len) || !within(body.len(), 35 + sid_len, 3) {
        return Err("hello retry is truncated".into());
    }
    if &body[35..35 + sid_len] != session {
        return Err("hello retry did not echo the session identifier".into());
    }
    let mut i = 35 + sid_len;
    let cipher = u16::from_be_bytes([body[i], body[i + 1]]);
    i += 2;
    if cipher != 0x1301 {
        return Err("the peer selected an unsupported cipher".into());
    }
    if body[i] != 0 {
        return Err("hello retry compression method is not zero".into());
    }
    i += 1;
    if !within(body.len(), i, 2) {
        return Err("hello retry is truncated".into());
    }
    let ext_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    i += 2;
    if ext_len < 6 || !within(body.len(), i, ext_len) || i + ext_len != body.len() {
        return Err("hello retry length does not match".into());
    }
    let exts = extensions(&body[i..i + ext_len])?;
    let mut saw_version = false;
    let mut cookie = None;
    let mut selected = None;
    for (kind, data) in exts {
        match kind {
            0x002b => {
                if data.as_slice() != [0x03, 0x04] {
                    return Err("the peer did not select the current protocol".into());
                }
                saw_version = true;
            }
            0x002c => {
                if data.is_empty() || data.len() > 65_000 {
                    return Err("hello retry cookie is outside the supported size".into());
                }
                cookie = Some(data);
            }
            0x0033 => {
                if data.len() != 2 {
                    return Err("hello retry key share is not a selected group".into());
                }
                selected = Some(u16::from_be_bytes([data[0], data[1]]));
            }
            _ => return Err("hello retry contains an extension that is not allowed".into()),
        }
    }
    if !saw_version {
        return Err("the peer did not select a protocol version".into());
    }
    if cookie.is_none() && selected.is_none() {
        return Err("hello retry would not change the client hello".into());
    }
    Ok(RetryPlan { cookie, selected })
}

fn retry_choice(plan: &RetryPlan, had_x: bool, had_p: bool) -> Result<RetryChoice, String> {
    if plan.cookie.is_none() && plan.selected.is_none() {
        return Err("hello retry would not change the client hello".into());
    }
    if let Some(group) = plan.selected {
        return match group {
            GROUP_X25519 => {
                if had_x {
                    Err("hello retry selected a group that was already shared".into())
                } else {
                    Ok(RetryChoice { send_x: true, send_p: false, fresh_x: true, fresh_p: false })
                }
            }
            GROUP_P256 => {
                if had_p {
                    Err("hello retry selected a group that was already shared".into())
                } else {
                    Ok(RetryChoice { send_x: false, send_p: true, fresh_x: false, fresh_p: true })
                }
            }
            _ => Err("hello retry selected a group that was not offered".into()),
        };
    }
    if !had_x && !had_p {
        return Err("the client hello has no key share".into());
    }
    Ok(RetryChoice { send_x: had_x, send_p: had_p, fresh_x: false, fresh_p: false })
}

fn updated_client_hello(
    random: &[u8; 32],
    session: &[u8; 32],
    x_scalar: &mut [u8; 32],
    x_public: &mut [u8; 32],
    p_scalar: &mut [u8; 32],
    p_public: &mut [u8; 65],
    had_x: &mut bool,
    had_p: &mut bool,
    plan: &RetryPlan,
) -> Result<Vec<u8>, String> {
    let choice = retry_choice(plan, *had_x, *had_p)?;
    if choice.fresh_x {
        let bytes = random_bytes(32)?;
        x_scalar.copy_from_slice(&bytes);
        let mut base = [0u8; 32];
        base[0] = 9;
        *x_public = x25519(x_scalar, &base);
    }
    if choice.fresh_p {
        let (scalar, share) = p256_offer()?;
        *p_scalar = scalar;
        *p_public = share;
    }
    let hello = client_hello_ext(
        random,
        session,
        if choice.send_x { Some(x_public) } else { None },
        if choice.send_p { Some(p_public) } else { None },
        plan.cookie.as_deref(),
    )?;
    *had_x = choice.send_x;
    *had_p = choice.send_p;
    Ok(hello)
}

fn fold_retry_transcript(transcript: &mut Vec<u8>, first_len: usize) -> Result<(), String> {
    if first_len > transcript.len() || first_len < 4 {
        return Err("hello retry transcript is truncated".into());
    }
    let digest = sha256(&transcript[..first_len]);
    let mut folded = Vec::with_capacity(36 + transcript.len() - first_len);
    folded.extend_from_slice(&[0xfe, 0x00, 0x00, 0x20]);
    folded.extend_from_slice(&digest);
    folded.extend_from_slice(&transcript[first_len..]);
    *transcript = folded;
    Ok(())
}

fn parse_server_hello(body: &[u8], session: &[u8; 32]) -> Result<PeerShare, String> {
    if body.len() < 34 + 1 + 32 + 3 {
        return Err("server hello is truncated".into());
    }
    if u16::from_be_bytes([body[0], body[1]]) != 0x0303 {
        return Err("server hello legacy version is not the required value".into());
    }
    let sid_len = body[34] as usize;
    if sid_len > 32 {
        return Err("server hello session identifier is too long".into());
    }
    if !within(body.len(), 35, sid_len) || !within(body.len(), 35 + sid_len, 3) {
        return Err("server hello is truncated".into());
    }
    if &body[35..35 + sid_len] != session {
        return Err("server hello did not echo the session identifier".into());
    }
    let mut i = 35 + sid_len;
    let cipher = u16::from_be_bytes([body[i], body[i + 1]]);
    i += 2;
    if cipher != 0x1301 {
        return Err("the peer selected an unsupported cipher".into());
    }
    if body[i] != 0 {
        return Err("server hello compression method is not zero".into());
    }
    i += 1;
    if !within(body.len(), i, 2) {
        return Err("server hello is truncated".into());
    }
    let ext_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    i += 2;
    if ext_len < 6 || !within(body.len(), i, ext_len) || i + ext_len != body.len() {
        return Err("server hello length does not match".into());
    }
    let exts = extensions(&body[i..i + ext_len])?;
    let version = exts.iter().find(|e| e.0 == 0x002b).ok_or("the peer did not select a protocol version")?;
    if version.1 != [0x03, 0x04] {
        return Err("the peer did not select the current protocol".into());
    }
    if is_hello_retry(body) {
        return Err("the peer sent a hello retry".into());
    }
    let share = exts.iter().find(|e| e.0 == 0x0033).ok_or("the peer did not send a key share")?;
    if share.1.len() < 4 {
        return Err("the peer key share is truncated".into());
    }
    let group = u16::from_be_bytes([share.1[0], share.1[1]]);
    let key_len = u16::from_be_bytes([share.1[2], share.1[3]]) as usize;
    if key_len.checked_add(4) != Some(share.1.len()) {
        return Err("the peer key share is truncated".into());
    }
    let key = &share.1[4..];
    if group == GROUP_X25519 && key_len == 32 {
        let mut out = [0u8; 32];
        out.copy_from_slice(key);
        return Ok(PeerShare::X25519(out));
    }
    if group == GROUP_P256 && key_len == 65 {
        let mut out = [0u8; 65];
        out.copy_from_slice(key);
        return Ok(PeerShare::P256(out));
    }
    Err("the peer key share is not a supported group".into())
}

#[cfg_attr(not(any(test, rllm_module_tests)), allow(dead_code))]
struct ClientOffer {
    session: [u8; 32],
    x25519: Option<[u8; 32]>,
    p256: Option<[u8; 65]>,
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_client_hello(body: &[u8]) -> Result<ClientOffer, String> {
    if body.len() < 35 {
        return Err("client hello is truncated".into());
    }
    if u16::from_be_bytes([body[0], body[1]]) != 0x0303 {
        return Err("client hello legacy version is not the required value".into());
    }
    let sid_len = body[34] as usize;
    if sid_len != 32 || body.len() < 35 + sid_len {
        return Err("client hello session identifier is not the expected size".into());
    }
    let mut session = [0u8; 32];
    session.copy_from_slice(&body[35..67]);
    let mut i = 35 + sid_len;
    if !within(body.len(), i, 2) {
        return Err("client hello is truncated".into());
    }
    let cipher_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    let after_ciphers = match i.checked_add(2).and_then(|v| v.checked_add(cipher_len)) {
        Some(end) if end < body.len() => end,
        _ => return Err("client hello is truncated".into()),
    };
    i = after_ciphers;
    let comp_len = body[i] as usize;
    let after_comp = match i.checked_add(1).and_then(|v| v.checked_add(comp_len)) {
        Some(end) if within(body.len(), end, 2) => end,
        _ => return Err("client hello is truncated".into()),
    };
    let comp = &body[i + 1..after_comp];
    if !comp.contains(&0) {
        return Err("client hello compression method is not zero".into());
    }
    i = after_comp;
    let ext_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    i += 2;
    if !within(body.len(), i, ext_len) || i + ext_len != body.len() {
        return Err("client hello length does not match".into());
    }
    let exts = extensions(&body[i..i + ext_len])?;
    let share = exts.iter().find(|e| e.0 == 0x0033).ok_or("the client did not send a key share")?;
    let (x25519, p256) = parse_client_shares(&share.1)?;
    Ok(ClientOffer { session, x25519, p256 })
}

#[cfg_attr(not(any(test, rllm_module_tests)), allow(dead_code))]
fn client_extension(body: &[u8], kind: u16) -> Result<Option<Vec<u8>>, String> {
    if body.len() < 35 {
        return Err("client hello is truncated".into());
    }
    let sid_len = body[34] as usize;
    if sid_len != 32 || !within(body.len(), 35, sid_len) {
        return Err("client hello is truncated".into());
    }
    let mut i = 35 + sid_len;
    if !within(body.len(), i, 2) {
        return Err("client hello is truncated".into());
    }
    let cipher_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    let after_ciphers = match i.checked_add(2).and_then(|value| value.checked_add(cipher_len)) {
        Some(end) if end < body.len() => end,
        _ => return Err("client hello is truncated".into()),
    };
    i = after_ciphers;
    let comp_len = body[i] as usize;
    let after_comp = match i.checked_add(1).and_then(|value| value.checked_add(comp_len)) {
        Some(end) if within(body.len(), end, 2) => end,
        _ => return Err("client hello is truncated".into()),
    };
    i = after_comp;
    let ext_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
    i += 2;
    if !within(body.len(), i, ext_len) || i + ext_len != body.len() {
        return Err("client hello length does not match".into());
    }
    let exts = extensions(&body[i..i + ext_len])?;
    Ok(exts.into_iter().find(|item| item.0 == kind).map(|item| item.1))
}

fn parse_client_shares(data: &[u8]) -> Result<(Option<[u8; 32]>, Option<[u8; 65]>), String> {
    if data.len() < 2 {
        return Err("the client key share is truncated".into());
    }
    let list = u16::from_be_bytes([data[0], data[1]]) as usize;
    if list.checked_add(2) != Some(data.len()) {
        return Err("the client key share is truncated".into());
    }
    let mut i = 2;
    let mut x25519 = None;
    let mut p256 = None;
    while i < data.len() {
        if !within(data.len(), i, 4) {
            return Err("the client key share is truncated".into());
        }
        let group = u16::from_be_bytes([data[i], data[i + 1]]);
        let key_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        i += 4;
        if key_len == 0 || !within(data.len(), i, key_len) {
            return Err("the client key share is truncated".into());
        }
        let key = &data[i..i + key_len];
        i += key_len;
        if group == GROUP_X25519 {
            if x25519.is_some() || key_len != 32 {
                return Err("the client key share is not a supported group".into());
            }
            let mut out = [0u8; 32];
            out.copy_from_slice(key);
            x25519 = Some(out);
        } else if group == GROUP_P256 {
            if p256.is_some() || key_len != 65 || key[0] != 4 {
                return Err("the client key share is not an uncompressed point".into());
            }
            let mut out = [0u8; 65];
            out.copy_from_slice(key);
            p256 = Some(out);
        }
    }
    if x25519.is_none() && p256.is_none() {
        return Err("the client did not offer a supported group".into());
    }
    Ok((x25519, p256))
}

#[cfg_attr(not(any(test, rllm_module_tests)), allow(dead_code))]
fn server_share(offer: &ClientOffer, random: &[u8; 32], p256_only: bool) -> Result<(Vec<u8>, [u8; 32]), String> {
    if !p256_only {
        if let Some(their) = offer.x25519 {
            let mut scalar = [0u8; 32];
            scalar.copy_from_slice(&random_bytes(32)?);
            let mut base = [0u8; 32];
            base[0] = 9;
            let share = x25519(&scalar, &base);
            let hello = server_hello(random, &offer.session, &share);
            let shared = x25519(&scalar, &their);
            if shared.iter().all(|byte| *byte == 0) {
                return Err("the key agreement produced an invalid shared secret".into());
            }
            return Ok((hello, shared));
        }
    }
    let their = offer.p256.ok_or("the client did not offer the required group")?;
    let (scalar, share) = p256_offer()?;
    let hello = server_hello_group(random, &offer.session, GROUP_P256, &share);
    let shared = p256_agree(&scalar, &their)?;
    Ok((hello, shared))
}

#[cfg_attr(not(any(test, rllm_module_tests)), allow(dead_code))]
fn server_hello_group(random: &[u8; 32], session: &[u8; 32], group: u16, share: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0x0303u16.to_be_bytes());
    body.extend_from_slice(random);
    body.push(32);
    body.extend_from_slice(session);
    body.extend_from_slice(&0x1301u16.to_be_bytes());
    body.push(0);
    let mut ext = Vec::new();
    ext.extend(extension(0x002b, &[0x03, 0x04]));
    let mut ks = Vec::new();
    ks.extend_from_slice(&group.to_be_bytes());
    ks.extend_from_slice(&(share.len() as u16).to_be_bytes());
    ks.extend_from_slice(share);
    ext.extend(extension(0x0033, &ks));
    body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    body.extend_from_slice(&ext);
    hs(2, &body)
}

fn extensions(data: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, String> {
    let mut out: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut i = 0;
    while within(data.len(), i, 4) {
        let kind = u16::from_be_bytes([data[i], data[i + 1]]);
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        i += 4;
        if !within(data.len(), i, len) {
            return Err("extension is truncated".into());
        }
        if out.iter().any(|item: &(u16, Vec<u8>)| item.0 == kind) {
            return Err("extensions repeat a type".into());
        }
        out.push((kind, data[i..i + len].to_vec()));
        i += len;
    }
    if i != data.len() {
        return Err("extensions are truncated".into());
    }
    Ok(out)
}

fn first_cert(body: &[u8]) -> Result<Vec<u8>, String> {
    if body.is_empty() {
        return Err("certificate message is empty".into());
    }
    if body[0] != 0 {
        return Err("certificate context is not empty".into());
    }
    let ctx = body[0] as usize;
    if !within(body.len(), 1, ctx) || !within(body.len(), 1 + ctx, 3) {
        return Err("certificate message is truncated".into());
    }
    let mut i = 1 + ctx;
    let list = u24(&body[i..i + 3]);
    i += 3;
    if i.checked_add(list) != Some(body.len()) || list < 5 {
        return Err("certificate list is empty".into());
    }
    let list_end = i + list;
    let mut first = None;
    while i < list_end {
        if !within(list_end, i, 3) {
            return Err("certificate is truncated".into());
        }
        let len = u24(&body[i..i + 3]);
        i += 3;
        if len < 1 || !within(list_end, i, len) {
            return Err("certificate is truncated".into());
        }
        let cert = body[i..i + len].to_vec();
        i += len;
        if !within(list_end, i, 2) {
            return Err("certificate extensions are truncated".into());
        }
        let ext_len = u16::from_be_bytes([body[i], body[i + 1]]) as usize;
        i += 2;
        if !within(list_end, i, ext_len) {
            return Err("certificate extensions are truncated".into());
        }
        extensions(&body[i..i + ext_len])?;
        i += ext_len;
        if first.is_none() {
            first = Some(cert);
        }
    }
    if i != list_end {
        return Err("certificate list is truncated".into());
    }
    first.ok_or_else(|| "certificate list is empty".to_string())
}

fn parse_encrypted_extensions(body: &[u8]) -> Result<(), String> {
    if body.len() < 2 {
        return Err("encrypted extensions are truncated".into());
    }
    let len = u16::from_be_bytes([body[0], body[1]]) as usize;
    if len.checked_add(2) != Some(body.len()) {
        return Err("encrypted extensions length does not match".into());
    }
    let exts = extensions(&body[2..])?;
    for (kind, data) in &exts {
        if matches!(*kind, 0x002b | 0x000d | 0x0029 | 0x002c | 0x002d | 0x0032 | 0x0033) {
            return Err("encrypted extensions contain an extension that is not allowed".into());
        }
        if *kind == 0x000a {
            if data.len() < 2 {
                return Err("encrypted extensions are truncated".into());
            }
            let list = u16::from_be_bytes([data[0], data[1]]) as usize;
            if list.checked_add(2) != Some(data.len()) || list % 2 != 0 {
                return Err("encrypted extensions are truncated".into());
            }
        }
    }
    Ok(())
}

fn signature_schemes(data: &[u8]) -> Result<Vec<u16>, String> {
    if data.len() < 2 {
        return Err("signature algorithms are truncated".into());
    }
    let list = u16::from_be_bytes([data[0], data[1]]) as usize;
    if list < 2 || list % 2 != 0 || list.checked_add(2) != Some(data.len()) {
        return Err("signature algorithms are truncated".into());
    }
    let mut schemes = Vec::new();
    let mut i = 2;
    while i < data.len() {
        schemes.push(u16::from_be_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    Ok(schemes)
}

fn parse_certificate_request(body: &[u8]) -> Result<(), String> {
    if body.is_empty() {
        return Err("certificate request is truncated".into());
    }
    if body[0] != 0 {
        return Err("certificate request context is not empty".into());
    }
    let ctx = body[0] as usize;
    if !within(body.len(), 1, ctx) || !within(body.len(), 1 + ctx, 2) {
        return Err("certificate request is truncated".into());
    }
    let ext_len = u16::from_be_bytes([body[1 + ctx], body[2 + ctx]]) as usize;
    let ext_at = 1 + ctx + 2;
    if ext_len < 2 || ext_at.checked_add(ext_len) != Some(body.len()) {
        return Err("certificate request length does not match".into());
    }
    let exts = extensions(&body[ext_at..])?;
    let sigs = exts
        .iter()
        .find(|item| item.0 == 0x000d)
        .ok_or_else(|| "certificate request did not list signature algorithms".to_string())?;
    let schemes = signature_schemes(&sigs.1)?;
    if !schemes.contains(&0x0804) {
        return Err("certificate request did not offer the client signature algorithm".into());
    }
    if let Some(cert_sigs) = exts.iter().find(|item| item.0 == 0x0032) {
        signature_schemes(&cert_sigs.1)?;
    }
    Ok(())
}

fn verify_cert_sig(cert: &[u8], body: &[u8], transcript: &[u8], label: &[u8]) -> Result<(), String> {
    if body.len() < 4 {
        return Err("certificate verify is truncated".into());
    }
    let scheme = u16::from_be_bytes([body[0], body[1]]);
    if scheme != 0x0804 {
        return Err("the peer used an unsupported signature scheme".into());
    }
    let len = u16::from_be_bytes([body[2], body[3]]) as usize;
    if body.len() != 4 + len {
        return Err("certificate verify length does not match".into());
    }
    let (n, e) = certificate_public_rsa(cert)?;
    let message_end = transcript
        .len()
        .checked_sub(body.len())
        .and_then(|value| value.checked_sub(4))
        .ok_or_else(|| "certificate verify transcript is truncated".to_string())?;
    let mut content = vec![0x20u8; 64];
    content.extend_from_slice(label);
    content.extend_from_slice(&sha256(&transcript[..message_end]));
    PrivateKey::verify_sha256_pss(&n, &e, &content, &body[4..])
}

fn certificate_message(context: &[u8], cert: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(context.len() as u8);
    body.extend_from_slice(context);
    let entry = 3 + cert.len() + 2;
    body.push((entry >> 16) as u8);
    body.push((entry >> 8) as u8);
    body.push(entry as u8);
    body.push((cert.len() >> 16) as u8);
    body.push((cert.len() >> 8) as u8);
    body.push(cert.len() as u8);
    body.extend_from_slice(cert);
    body.extend_from_slice(&0u16.to_be_bytes());
    hs(11, &body)
}

fn certificate_verify(key: &PrivateKey, label: &[u8], transcript: &[u8]) -> Result<Vec<u8>, String> {
    let mut content = vec![0x20u8; 64];
    content.extend_from_slice(label);
    content.extend_from_slice(&sha256(transcript));
    let sig = key.sign_sha256_pss(&content)?;
    let mut body = Vec::new();
    body.extend_from_slice(&0x0804u16.to_be_bytes());
    body.extend_from_slice(&(sig.len() as u16).to_be_bytes());
    body.extend_from_slice(&sig);
    Ok(hs(15, &body))
}

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2, 0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65, 0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4, 0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df, 0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec, 0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c, 0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

#[cfg(any(test, rllm_module_tests))]
mod tests {
    use super::*;

    fn hex(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn field_matches_integer_arithmetic() {
        let mut prime = Nat::from_u32(1);
        for _ in 0..255 {
            prime = prime.shl1();
        }
        prime = prime.sub(&Nat::from_u32(19));
        let mut a_bytes = [0u8; 32];
        let mut b_bytes = [0u8; 32];
        a_bytes[0] = 0xec;
        a_bytes[1] = 0xff;
        a_bytes[2] = 0xff;
        a_bytes[3] = 0xff;
        a_bytes[4] = 0xff;
        a_bytes[5] = 0xff;
        a_bytes[6] = 0xff;
        a_bytes[7] = 0xff;
        for i in 8..31 {
            a_bytes[i] = 0xff;
            b_bytes[i] = 0x5a;
        }
        a_bytes[31] = 0x7f;
        b_bytes[0] = 0x9b;
        b_bytes[31] = 0x3c;
        let a = fe_from_bytes(&a_bytes).unwrap();
        let b = fe_from_bytes(&b_bytes).unwrap();
        let prod = fe_to_bytes(&fe_mul(&a, &b));
        let expect = Nat::from_le(&a_bytes).mul(&Nat::from_le(&b_bytes)).modulo(&prime);
        assert_eq!(prod.to_vec(), expect.to_le_fixed(32), "product");
        let sum = fe_to_bytes(&fe_add(&a, &b));
        let expect_sum = Nat::from_le(&a_bytes).add(&Nat::from_le(&b_bytes)).modulo(&prime);
        assert_eq!(sum.to_vec(), expect_sum.to_le_fixed(32), "sum");
        let diff = fe_to_bytes(&fe_sub(&fe_from_u64(3), &b));
        let expect_diff = Nat::from_u32(3).add(&prime).sub(&Nat::from_le(&b_bytes)).modulo(&prime);
        assert_eq!(diff.to_vec(), expect_diff.to_le_fixed(32), "difference");
        let inv = fe_to_bytes(&fe_inv(&fe_from_u64(2)));
        let two = Nat::from_u32(2);
        let expect_inv = two.modpow(&prime.sub(&Nat::from_u32(2)), &prime).unwrap();
        assert_eq!(inv.to_vec(), expect_inv.to_le_fixed(32), "inverse");
        let wrapped = fe_to_bytes(&fe_add(&a, &fe_from_u64(1)));
        assert_eq!(wrapped, [0u8; 32], "p-1 plus one");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn published_hashes_and_key_schedule() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha512(b"abc")), "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
        let key = vec![0x0bu8; 20];
        assert_eq!(hex(&hmac_sha256(&key, b"Hi There")), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
        let ikm = unhex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let salt = unhex("000102030405060708090a0b0c");
        let info = unhex("f0f1f2f3f4f5f6f7f8f9");
        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(hex(&prk), "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5");
        assert_eq!(hex(&hkdf_expand(&prk, &info, 42).unwrap()), "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865");
        assert!(hkdf_expand(&prk, &info, 0).unwrap().is_empty());
        assert_eq!(hkdf_expand(&prk, &info, 255 * 32).unwrap().len(), 255 * 32);
        assert!(hkdf_expand(&prk, &info, 255 * 32 + 1).is_err());
        assert!(export_keying_material(&prk, b"label", 255 * 32 + 1).is_err());
        let master = unhex("18df06843d13a08bf2a449844c5f8a478001bc4d4c627984d5a41da8d0402919");
        let th = unhex("9608102a0f1ccc6db6250b7b7e417b1a000eaada3daae4777a7686c9ff83df13");
        assert_eq!(hex(&hkdf_expand_label(&master, b"c ap traffic", &th, 32).unwrap()), "9e40646ce79a7f9dc05af8889bce6552875afa0b06df0087f792ebb7c17504a5");
        assert_eq!(hex(&hkdf_expand_label(&master, b"exp master", &th, 32).unwrap()), "fe22f881176eda18eb8f44529e6792c50c9a3f89452f68d8ae311b4309d3cf50");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn published_aes_gcm_and_x25519() {
        let block = aes_encrypt_block(&unhex("000102030405060708090a0b0c0d0e0f"), &unhex("00112233445566778899aabbccddeeff").try_into().unwrap()).unwrap();
        assert_eq!(hex(&block), "69c4e0d86a7b0430d8cdb78070b4c55a");
        let key = unhex("7fddb57453c241d03efbed3ac44e371c");
        let nonce: [u8; 12] = unhex("ee283a3fc75575e33efd4887").try_into().unwrap();
        let pt = unhex("d5de42b461646c255c87bd2962d3b9a2");
        let boxed = aes_gcm_seal(&key, &nonce, b"", &pt).unwrap();
        assert_eq!(hex(&boxed), "2ccda4a5415cb91e135c2a0f78c9b2fdb36d1df9b9d5e596f83e8b7f52971cb3");
        assert_eq!(aes_gcm_open(&key, &nonce, b"", &boxed).unwrap(), pt);
        let scalar = unhex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let mut base = [0u8; 32];
        base[0] = 9;
        assert_eq!(hex(&x25519(&scalar.try_into().unwrap(), &base)), "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn curve_matches_published_point_and_both_pairing_sides_agree() {
        let seed = unhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let digest = sha512(&seed);
        let mut scalar = [0u8; 32];
        scalar.copy_from_slice(&digest[..32]);
        scalar[0] &= 248;
        scalar[31] &= 63;
        scalar[31] |= 64;
        let curve = curve().unwrap();
        assert_eq!(hex(&fe_to_bytes(&curve.d)), "a3785913ca4deb75abd841414d0a700098e879777940c78c73fe6f2bee6c0352");
        assert_eq!(hex(&compress(&curve.base)), "5866666666666666666666666666666666666666666666666666666666666666", "base");
        let added = point_add(curve.d2, &point_identity(), &curve.base);
        assert_eq!(hex(&compress(&added)), "5866666666666666666666666666666666666666666666666666666666666666", "add identity");
        let doubled = point_double(&point_identity());
        assert_eq!(hex(&compress(&doubled)), "0100000000000000000000000000000000000000000000000000000000000000", "double identity");
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(hex(&compress(&point_mul(&curve.base, &one).unwrap())), "5866666666666666666666666666666666666666666666666666666666666666", "times one");
        let point = point_mul(&curve.base, &scalar).unwrap();
        assert_eq!(hex(&compress(&point)), "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let password = b"123456-secret";
        let alice = Spake2::alice(password).unwrap();
        let bob = Spake2::bob(password).unwrap();
        let a = alice.message().clone();
        let b = bob.message().clone();
        let ak = alice.finish(&b).unwrap();
        let bk = bob.finish(&a).unwrap();
        assert_eq!(ak, bk);
        let mut ca = PairCipher::new(&ak);
        let mut cb = PairCipher::new(&bk);
        let sealed = ca.seal(b"peer-info").unwrap();
        assert_eq!(cb.open(&sealed).unwrap(), b"peer-info");
        let wrong = Spake2::bob(b"999999-secret").unwrap();
        let wk = wrong.finish(&a).unwrap();
        let mut cw = PairCipher::new(&wk);
        assert!(cw.open(&sealed).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn rsa_textbook_and_signature_round_trip() {
        let p = Nat::from_u32(61);
        let q = Nat::from_u32(53);
        let n = p.mul(&q);
        let e = Nat::from_u32(17);
        let phi = p.sub(&Nat::from_u32(1)).mul(&q.sub(&Nat::from_u32(1)));
        let d = e.inv_mod(&phi).unwrap();
        assert_eq!(d.to_be(), Nat::from_u32(2753).to_be());
        let message = Nat::from_u32(123);
        let cipher = message.modpow(&e, &n).unwrap();
        assert_eq!(cipher.modpow(&d, &n).unwrap().to_be(), message.to_be());
        assert_eq!(cipher.modpow_secret(&d, &n).unwrap().to_be(), message.to_be());
        let key = PrivateKey::generate(1024).unwrap();
        let sig = key.sign_sha1_pkcs1(b"token-bytes").unwrap();
        key.verify_sha1_pkcs1(b"token-bytes", &sig).unwrap();
        assert!(key.verify_sha1_pkcs1(b"other-bytes", &sig).is_err());
        let (n_bytes, e_bytes) = key.public_parts();
        let pss = key.sign_sha256_pss(b"handshake").unwrap();
        let weak = PrivateKey::verify_sha256_pss(&n_bytes, &e_bytes, b"handshake", &pss).unwrap_err();
        assert!(weak.contains("supported size"), "{weak}");
        assert!(MIN_RSA_MODULUS_BITS >= 2048);
        let mut modulus = vec![0xffu8; 128];
        modulus[0] |= 0x80;
        modulus[127] |= 1;
        assert!(PrivateKey::verify_sha256_pss(&modulus, &[0x01, 0x00, 0x01], b"peer", &vec![1u8; 128]).is_err());
        let stored = PrivateKey::load_bytes(&key.store_bytes()).unwrap();
        stored.verify_sha1_pkcs1(b"token-bytes", &sig).unwrap();
        let again = stored.sign_sha1_pkcs1(b"token-bytes").unwrap();
        stored.verify_sha1_pkcs1(b"token-bytes", &again).unwrap();
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn tls_handshake_exports_the_same_material_both_ways() {
        use std::net::TcpListener;
        use std::thread;
        let server_key = PrivateKey::generate(2048).unwrap();
        let client_key = PrivateKey::generate(2048).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nodelay(true).unwrap();
            server_handshake(stream, &server_key).unwrap()
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_nodelay(true).unwrap();
        let mut client = client_handshake(stream, &client_key, None).unwrap();
        let mut server = server.join().unwrap();
        let label = b"adb-label\0";
        assert_eq!(client.exporter(label, 64).material().unwrap(), server.exporter(label, 64).material().unwrap());
        assert!(client.exporter(label, 255 * 32 + 1).material().is_err());
        client.write_all(b"ping-frame").unwrap();
        assert_eq!(server.read_exact(10).unwrap(), b"ping-frame");
        server.write_all(b"pong-frame").unwrap();
        assert_eq!(client.read_exact(10).unwrap(), b"pong-frame");
        server.request_key_update().unwrap();
        server.write_all(b"after-update").unwrap();
        assert_eq!(client.read_exact(12).unwrap(), b"after-update");
        client.write_all(b"client-reply").unwrap();
        assert_eq!(server.read_exact(12).unwrap(), b"client-reply");
        assert!(!client.peer_certificate().is_empty());
        let ticket = sample_ticket();
        server.write_handshake_bytes(&ticket).unwrap();
        server.write_all(b"after-ticket").unwrap();
        assert_eq!(client.read_exact(12).unwrap(), b"after-ticket");
        let update = [24u8, 0, 0, 1, 0];
        server.write_handshake_bytes(&update[..3]).unwrap();
        server.write_handshake_bytes(&update[3..]).unwrap();
        server.install_next_write_key().unwrap();
        server.write_all(b"after-fragment").unwrap();
        assert_eq!(client.read_exact(14).unwrap(), b"after-fragment");
        client.write_seq = AES_GCM_RECORD_LIMIT - 1;
        server.read_seq = AES_GCM_RECORD_LIMIT - 1;
        client.write_all(b"rotated").unwrap();
        assert_eq!(server.read_exact(7).unwrap(), b"rotated");
        assert!(client.write_seq < AES_GCM_RECORD_LIMIT);
        client.write_seq = AES_GCM_RECORD_LIMIT;
        client.write_updates = MAX_KEY_UPDATES;
        let limited = client.write_all(b"x").unwrap_err();
        assert!(limited.contains("record limit"), "{limited}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn hostile_lengths_do_not_panic() {
        assert!(PRIME_WITNESS_ROUNDS >= 64);
        assert!(!is_probable_prime(&Nat::from_u32(5003).mul(&Nat::from_u32(5009))).unwrap());
        assert!(is_probable_prime(&Nat::from_u32(7919)).unwrap());
        assert_eq!(sbox_at(0), 0x63);
        assert_eq!(sbox_at(1), 0x7c);
        for value in 0..=255u16 {
            assert_eq!(sbox_at(value as u8), algebraic_sbox(value as u8));
        }
        assert!(PrivateKey::verify_sha256_pss(&[0x00], &[0x03], b"peer", &[0x01]).is_err());
        assert!(PrivateKey::verify_sha256_pss(&[0x0f], &[0x03], b"peer", &[0x02]).is_err());
        let huge_exponent = vec![0x01u8; 40];
        let mut modulus = vec![0xffu8; 128];
        modulus[0] &= 0x7f;
        modulus[127] |= 1;
        assert!(PrivateKey::verify_sha256_pss(&modulus, &huge_exponent, b"peer", &vec![1u8; 128]).is_err());
        let wide = Nat::from_be(&vec![0xffu8; 600]);
        assert!(Nat::from_u32(2).modpow(&wide, &Nat::from_be(&modulus)).is_err());
        assert!(PrivateKey::load_bytes(&10_000u32.to_be_bytes()).is_err());
        assert!(certificate_public_rsa(&vec![0x30; MAX_HS_MESSAGE + 1]).is_err());
        let mut server_hello = vec![0x03, 0x03];
        server_hello.extend_from_slice(&[9u8; 32]);
        server_hello.push(32);
        server_hello.extend_from_slice(&[7u8; 32]);
        server_hello.extend_from_slice(&0x1301u16.to_be_bytes());
        server_hello.push(0);
        server_hello.extend_from_slice(&0xffffu16.to_be_bytes());
        assert!(parse_server_hello(&server_hello, &[7u8; 32]).is_err());
        let mut client_hello = vec![0x03, 0x03];
        client_hello.extend_from_slice(&[0u8; 32]);
        client_hello.push(32);
        client_hello.extend_from_slice(&[1u8; 32]);
        client_hello.extend_from_slice(&2u16.to_be_bytes());
        client_hello.extend_from_slice(&0x1301u16.to_be_bytes());
        client_hello.push(1);
        client_hello.push(0);
        let mut share = Vec::new();
        share.extend_from_slice(&34u16.to_be_bytes());
        share.extend_from_slice(&0x001du16.to_be_bytes());
        share.extend_from_slice(&32u16.to_be_bytes());
        share.extend_from_slice(&[0u8; 30]);
        let mut ext = Vec::new();
        ext.extend_from_slice(&0x0033u16.to_be_bytes());
        ext.extend_from_slice(&(share.len() as u16).to_be_bytes());
        ext.extend_from_slice(&share);
        client_hello.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        client_hello.extend_from_slice(&ext);
        assert!(parse_client_hello(&client_hello).is_err());
        use std::io::Write;
        use std::net::TcpListener;
        use std::thread;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let oversized = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&[23, 3, 3, 0x48, 0x01]).unwrap();
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let err = read_raw(&mut client, None).unwrap_err();
        assert!(err.contains("too large"), "{err}");
        oversized.join().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let truncated = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&[23, 3, 3, 0x00, 20]).unwrap();
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        assert!(read_raw(&mut client, None).is_err());
        truncated.join().unwrap();
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn certificate_proof_is_required_before_the_session_is_usable() {
        let server_key = PrivateKey::generate(2048).unwrap();
        let client_key = PrivateKey::generate(2048).unwrap();
        let omitted = reject_peer(&server_key, &client_key, 0);
        assert!(omitted.contains("certificate key"), "{omitted}");
        let forged = reject_peer(&server_key, &client_key, 1);
        assert!(forged.contains("signature") || forged.contains("exponent") || forged.contains("modulus") || forged.contains("intact") || forged.contains("verify"), "{forged}");
        let repeated = reject_peer(&server_key, &client_key, 2);
        assert!(repeated.contains("encrypted extensions"), "{repeated}");
    }

    fn algebraic_sbox(value: u8) -> u8 {
        if value == 0 {
            return 0x63;
        }
        let mut power = value;
        let mut inverse = 1u8;
        let mut exponent = 0xfeu8;
        while exponent != 0 {
            if exponent & 1 != 0 {
                inverse = gf8(inverse, power);
            }
            power = gf8(power, power);
            exponent >>= 1;
        }
        let mut rotated = inverse;
        let mut result = inverse;
        for _ in 0..4 {
            rotated = rotated.rotate_left(1);
            result ^= rotated;
        }
        result ^ 0x63
    }

    fn reject_peer(server_key: &PrivateKey, client_key: &PrivateKey, mode: u8) -> String {
        reject_peer_pin(server_key, client_key, mode, None)
    }

    fn reject_peer_pin(server_key: &PrivateKey, client_key: &PrivateKey, mode: u8, pin: Option<&[u8]>) -> String {
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;
        let stored = server_key.store_bytes();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let message = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let (stream, _) = listener.accept().expect("accept");
                stream.set_nodelay(true).ok();
                stream.set_read_timeout(Some(Duration::from_secs(20))).ok();
                stream.set_write_timeout(Some(Duration::from_secs(20))).ok();
                let key = PrivateKey::load_bytes(&stored).expect("load key");
                match hostile_flight(stream, &key, mode) {
                    Ok(()) => "server finished".to_string(),
                    Err(err) => err,
                }
            }));
            let text = match message {
                Ok(text) => text,
                Err(_) => "server panicked".to_string(),
            };
            let _ = tx.send(text);
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(Duration::from_secs(20))).ok();
        let err = match client_handshake(stream, client_key, pin) {
            Err(err) => err,
            Ok(_) => "client accepted the session".to_string(),
        };
        let server_text = rx.recv().unwrap_or_else(|_| "server thread ended".to_string());
        let _ = server.join();
        format!("{err} | {server_text}")
    }

    fn hostile_flight(stream: TcpStream, key: &PrivateKey, mode: u8) -> Result<(), String> {
        let mut flight = new_flight(stream);
        let (kind, hello) = flight.read_hs()?;
        if kind != 1 {
            return Err("the peer did not send a client hello".into());
        }
        let offered = parse_client_hello(&hello)?;
        let their_share = offered.x25519.ok_or("the client did not offer a supported group")?;
        let mut scalar = [0u8; 32];
        scalar.copy_from_slice(&random_bytes(32)?);
        let mut base = [0u8; 32];
        base[0] = 9;
        let share = x25519(&scalar, &base);
        let mut random = [0u8; 32];
        random.copy_from_slice(&random_bytes(32)?);
        flight.write_hs(&server_hello(&random, &offered.session, &share), false)?;
        write_raw(&mut flight.stream, 20, &[1], false, Some(flight.deadline))?;
        let shared = x25519(&scalar, &their_share);
        let (_c_hs, s_hs, _master) = handshake_secrets(&shared, &flight.transcript)?;
        let (wk, wiv) = keys_of(&s_hs)?;
        flight.write_key = Some((wk, wiv, 0));
        flight.write_hs(&hs(8, &0u16.to_be_bytes()), true)?;
        if mode == 2 {
            for _ in 0..8 {
                if flight.write_hs(&hs(8, &0u16.to_be_bytes()), true).is_err() {
                    break;
                }
            }
            drain_until_close(&mut flight.stream);
            return Ok(());
        }
        let mut req = vec![0u8];
        let mut ext = Vec::new();
        ext.extend_from_slice(&0x000du16.to_be_bytes());
        ext.extend_from_slice(&4u16.to_be_bytes());
        ext.extend_from_slice(&2u16.to_be_bytes());
        ext.extend_from_slice(&0x0804u16.to_be_bytes());
        req.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        req.extend_from_slice(&ext);
        flight.write_hs(&hs(13, &req), true)?;
        let cert = key.certificate("peer")?;
        flight.write_hs(&certificate_message(&[], &cert), true)?;
        if mode == 1 {
            let mut body = vec![];
            body.extend_from_slice(&0x0804u16.to_be_bytes());
            body.extend_from_slice(&128u16.to_be_bytes());
            body.extend_from_slice(&[0u8; 128]);
            flight.write_hs(&hs(15, &body), true)?;
        }
        let finished = finished_data(&s_hs, &flight.transcript)?;
        flight.write_hs(&hs(20, &finished), true)?;
        drain_until_close(&mut flight.stream);
        Ok(())
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn handshake_limits_reject_hostile_records() {
        assert_eq!(fe_to_bytes(&fe_neg(&Fe([0, 0, 0, 0]))), [0u8; 32]);
        let one = fe_from_u64(1);
        let wrapped = fe_add(&fe_sub(&Fe(P), &one), &one);
        assert_eq!(fe_to_bytes(&wrapped), [0u8; 32]);
        assert!(parse_key_update(&[24, 0, 0, 1, 0]).is_ok());
        assert!(parse_key_update(&[24, 0, 0, 1, 1]).is_ok());
        assert!(parse_key_update(&[24, 0, 0, 1, 2]).is_err());
        assert!(parse_key_update(&[24, 0, 0, 2, 0, 0]).is_err());
        assert!(parse_key_update(&[22, 0, 0, 1, 0]).is_err());
        assert!(parse_encrypted_extensions(&[0, 0]).is_ok());
        assert!(parse_encrypted_extensions(&[0, 0, 1]).is_err());
        assert!(parse_encrypted_extensions(&[]).is_err());
        let mut repeated = vec![0, 8];
        repeated.extend_from_slice(&0u16.to_be_bytes());
        repeated.extend_from_slice(&0u16.to_be_bytes());
        repeated.extend_from_slice(&0u16.to_be_bytes());
        repeated.extend_from_slice(&0u16.to_be_bytes());
        assert!(parse_encrypted_extensions(&repeated).is_err());
        let mut forbidden = vec![0, 6];
        forbidden.extend_from_slice(&0x002bu16.to_be_bytes());
        forbidden.extend_from_slice(&2u16.to_be_bytes());
        forbidden.extend_from_slice(&[0x03, 0x04]);
        assert!(parse_encrypted_extensions(&forbidden).is_err());
        let session = [7u8; 32];
        let random = [9u8; 32];
        let mut share = [3u8; 32];
        share[0] = 9;
        let good = server_hello(&random, &session, &share);
        let body = &good[4..];
        assert!(parse_server_hello(body, &session).is_ok());
        let mut bad_version = body.to_vec();
        bad_version[0] = 0x03;
        bad_version[1] = 0x01;
        assert!(parse_server_hello(&bad_version, &session).unwrap_err().contains("legacy version"));
        let mut bad_compression = body.to_vec();
        let compression_at = 35 + 32 + 2;
        bad_compression[compression_at] = 1;
        assert!(parse_server_hello(&bad_compression, &session).unwrap_err().contains("compression"));
        let mut trailing = body.to_vec();
        trailing.push(0);
        assert!(parse_server_hello(&trailing, &session).unwrap_err().contains("length"));
        use std::io::Write;
        use std::net::TcpListener;
        use std::thread;
        use std::time::{Duration, Instant};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.set_nodelay(true);
            for _ in 0..64 {
                if stream.write_all(&[22]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let started = Instant::now();
        let err = read_raw(&mut client, Some(started + Duration::from_millis(300))).unwrap_err();
        let elapsed = started.elapsed();
        assert!(err.contains("deadline"), "{err}");
        assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
        drop(client);
        let _ = peer.join();
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn secret_scalar_reduction_is_the_residue_modulo_the_order() {
        let mut small = [0u8; 64];
        small[0] = 5;
        assert_eq!(sc_reduce(&small)[0], 5);
        assert_eq!(sc_reduce(&small)[1..], [0u8; 31]);
        let mut order_wide = [0u8; 64];
        order_wide[..32].copy_from_slice(&ORDER);
        assert_eq!(sc_reduce(&order_wide), [0u8; 32]);
        let mut plus = order_wide;
        plus[0] = plus[0].wrapping_add(3);
        let reduced = sc_reduce(&plus);
        assert_eq!(reduced[0], 3);
        assert_eq!(&reduced[1..], &[0u8; 31]);
        let sample = [0xabu8; 64];
        let expect = Nat::from_le(&sample).modulo(&Nat::from_le(&ORDER));
        assert_eq!(sc_reduce(&sample).to_vec(), expect.to_le_fixed(32));
        let above = [0xffu8; 64];
        let expect_above = Nat::from_le(&above).modulo(&Nat::from_le(&ORDER));
        assert_eq!(sc_reduce(&above).to_vec(), expect_above.to_le_fixed(32));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn x25519_accepts_noncanonical_coordinates() {
        let scalar = unhex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let scalar: [u8; 32] = scalar.try_into().unwrap();
        let mut base = [0u8; 32];
        base[0] = 9;
        let canonical = x25519(&scalar, &base);
        let mut plus_prime = [0xffu8; 32];
        plus_prime[0] = 0xf6;
        plus_prime[31] = 0x7f;
        assert_eq!(x25519(&scalar, &plus_prime), canonical);
        let mut eighteen = [0u8; 32];
        eighteen[0] = 18;
        let saturated = [0xffu8; 32];
        assert_eq!(x25519(&scalar, &saturated), x25519(&scalar, &eighteen));
        assert_ne!(x25519(&scalar, &saturated), x25519(&scalar, &[0u8; 32]));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn send_side_record_limit_matches_the_aes_gcm_bound() {
        let n = u128::from(AES_GCM_RECORD_LIMIT);
        let bound = 1u128 << 49;
        assert!(n * n <= bound);
        assert!((n + 1) * (n + 1) > bound);
        assert_eq!(MAX_KEY_UPDATES, (1u64 << 48) - 1);
        let mut cipher = PairCipher::new(&[7u8; 16]);
        cipher.enc = AES_GCM_RECORD_LIMIT;
        let err = cipher.seal(b"x").unwrap_err();
        assert!(err.contains("record limit"), "{err}");
        cipher.enc = AES_GCM_RECORD_LIMIT - 1;
        assert!(cipher.seal(b"x").is_ok());
    }

    fn sample_ticket() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&60u32.to_be_bytes());
        body.extend_from_slice(&7u32.to_be_bytes());
        body.push(1);
        body.push(9);
        body.extend_from_slice(&1u16.to_be_bytes());
        body.push(0xab);
        body.extend_from_slice(&0u16.to_be_bytes());
        hs(4, &body)
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn post_handshake_framing_accepts_tickets_and_fragments() {
        let ticket = sample_ticket();
        let update = vec![24u8, 0, 0, 1, 0];
        let mut buf = Vec::new();
        assert!(absorb_handshake(&mut buf, &ticket).unwrap().is_empty());
        assert!(buf.is_empty());
        let mut buf = Vec::new();
        assert!(absorb_handshake(&mut buf, &update[..3]).unwrap().is_empty());
        assert!(!buf.is_empty());
        assert_eq!(absorb_handshake(&mut buf, &update[3..]).unwrap(), vec![0]);
        assert!(buf.is_empty());
        let mut both = ticket.clone();
        both.extend_from_slice(&update);
        let mut buf = Vec::new();
        assert_eq!(absorb_handshake(&mut buf, &both).unwrap(), vec![0]);
        let mut trailing = update.clone();
        trailing.extend_from_slice(&ticket);
        assert!(absorb_handshake(&mut Vec::new(), &trailing).unwrap_err().contains("aligned"));
        assert!(absorb_handshake(&mut Vec::new(), &[]).unwrap_err().contains("empty"));
        assert!(absorb_handshake(&mut Vec::new(), &[24, 0xff, 0xff, 0xff]).is_err());
        assert!(absorb_handshake(&mut Vec::new(), &hs(1, &[0])).is_err());
        let mut empty_ticket = Vec::new();
        empty_ticket.extend_from_slice(&1u32.to_be_bytes());
        empty_ticket.extend_from_slice(&1u32.to_be_bytes());
        empty_ticket.push(0);
        empty_ticket.extend_from_slice(&0u16.to_be_bytes());
        empty_ticket.extend_from_slice(&0u16.to_be_bytes());
        assert!(absorb_handshake(&mut Vec::new(), &hs(4, &empty_ticket)).is_err());
        let mut requested = update.clone();
        requested[4] = 1;
        assert_eq!(absorb_handshake(&mut Vec::new(), &requested).unwrap(), vec![1]);
        let mut split = ticket.clone();
        split.extend_from_slice(&update[..2]);
        let mut buf = Vec::new();
        assert!(absorb_handshake(&mut buf, &split).unwrap().is_empty());
        assert_eq!(absorb_handshake(&mut buf, &update[2..]).unwrap(), vec![0]);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn prime_field_group_is_offered_and_completes() {
        let (p_minus, _) = u32_sub(&P256_P, &[1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(p256_add(&p_minus, &p256_one()), p256_zero());
        assert_eq!(p256_mul(&p_minus, &p_minus), p256_one());
        assert_eq!(p256_sub(&p256_zero(), &p256_one()), p_minus);
        assert!(p256_on_curve(&be_limbs(&P256_GX_BE), &be_limbs(&P256_GY_BE)));
        let mut one = [0u8; 32];
        one[31] = 1;
        let g = p256_mul_point(&p256_base(), &one);
        assert_eq!(g.inf, 0);
        assert_eq!(g.x, be_limbs(&P256_GX_BE));
        assert_eq!(g.y, be_limbs(&P256_GY_BE));
        let mut two = [0u8; 32];
        two[31] = 2;
        let mut three = [0u8; 32];
        three[31] = 3;
        let g2 = p256_mul_point(&p256_base(), &two);
        let g3 = p256_mul_point(&p256_base(), &three);
        let pub2 = p256_encode(&g2).unwrap();
        let pub3 = p256_encode(&g3).unwrap();
        assert_eq!(p256_agree(&three, &pub2).unwrap(), p256_agree(&two, &pub3).unwrap());
        let mut bad = pub2;
        bad[1] ^= 1;
        assert!(p256_agree(&three, &bad).is_err());
        let random = [9u8; 32];
        let session = [7u8; 32];
        let hello = client_hello(&random, &session, &[4u8; 32], &pub2);
        assert!(hello.windows(2).any(|item| item == GROUP_P256.to_be_bytes()));
        assert!(hello.windows(2).any(|item| item == GROUP_X25519.to_be_bytes()));
        let server = server_hello_group(&random, &session, GROUP_P256, &pub2);
        match parse_server_hello(&server[4..], &session).unwrap() {
            PeerShare::P256(point) => assert_eq!(point, pub2),
            PeerShare::X25519(_) => panic!("expected the prime-field share"),
        }
        use std::net::TcpListener;
        use std::thread;
        let server_key = PrivateKey::generate(2048).unwrap();
        let client_key = PrivateKey::generate(2048).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nodelay(true).unwrap();
            server_handshake_p256(stream, &server_key).unwrap()
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_nodelay(true).unwrap();
        let mut client = client_handshake(stream, &client_key, None).unwrap();
        let mut server = peer.join().unwrap();
        client.write_all(b"p256-frame").unwrap();
        assert_eq!(server.read_exact(10).unwrap(), b"p256-frame");
        server.write_all(b"p256-reply").unwrap();
        assert_eq!(client.read_exact(10).unwrap(), b"p256-reply");
    }

    fn wide_limbs_be(limbs: &[u32; 16]) -> [u8; 64] {
        let mut out = [0u8; 64];
        for i in 0..16 {
            let o = (15 - i) * 4;
            out[o..o + 4].copy_from_slice(&limbs[i].to_be_bytes());
        }
        out
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn p256_product_carry_matches_the_integer_product() {
        let cases = [
            ([0xffff_ffffu32; 8], [0xffff_ffffu32; 8]),
            ([0xffff_ffff, 0, 0, 0, 0, 0, 0, 0], [0xffff_ffff, 0, 0, 0, 0, 0, 0, 0]),
            ([1, 0, 0, 0, 0, 0, 0, 0], [1, 0, 0, 0, 0, 0, 0, 0]),
            ([0xffff_ffff; 8], [2, 0, 0, 0, 0, 0, 0, 0]),
        ];
        for (left, right) in cases {
            let prod = p256_product(&left, &right);
            let expect = Nat::from_be(&limbs_be(&left)).mul(&Nat::from_be(&limbs_be(&right)));
            assert_eq!(Nat::from_be(&wide_limbs_be(&prod)).to_be_fixed(64), expect.to_be_fixed(64));
        }
        let mut two = p256_zero();
        two[0] = 2;
        let pm2 = p256_sub(&P256_P, &two);
        let mut four = p256_zero();
        four[0] = 4;
        assert_eq!(p256_mul(&pm2, &pm2), four);
    }

    fn header_read_error(header: &[u8]) -> String {
        use std::io::Write;
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let header = header.to_vec();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.write_all(&header);
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let err = read_raw(&mut client, None).unwrap_err();
        let _ = peer.join();
        err
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn incoming_records_stop_at_the_protocol_limit() {
        assert_eq!(MAX_CIPHERTEXT_RECORD, 16_640);
        assert_eq!(MAX_PLAINTEXT_RECORD, 16_384);
        let old_limit = header_read_error(&[23, 3, 3, 0x48, 0x00]);
        assert!(old_limit.contains("too large"), "{old_limit}");
        let cipher_over = header_read_error(&[23, 3, 3, 0x41, 0x01]);
        assert!(cipher_over.contains("too large"), "{cipher_over}");
        let plain_over = header_read_error(&[22, 3, 1, 0x40, 0x01]);
        assert!(plain_over.contains("too large"), "{plain_over}");
        let cipher_max = header_read_error(&[23, 3, 3, 0x41, 0x00]);
        assert!(!cipher_max.contains("too large"), "{cipher_max}");
        let plain_max = header_read_error(&[22, 3, 1, 0x40, 0x00]);
        assert!(!plain_max.contains("too large"), "{plain_max}");
    }

    fn cert_request(context: &[u8], schemes: &[u16], extra_scheme_bytes: &[u8]) -> Vec<u8> {
        let mut list = Vec::new();
        let declared = schemes.len() * 2 + extra_scheme_bytes.len();
        list.extend_from_slice(&(declared as u16).to_be_bytes());
        for scheme in schemes {
            list.extend_from_slice(&scheme.to_be_bytes());
        }
        list.extend_from_slice(extra_scheme_bytes);
        let mut ext = Vec::new();
        ext.extend_from_slice(&0x000du16.to_be_bytes());
        ext.extend_from_slice(&(list.len() as u16).to_be_bytes());
        ext.extend_from_slice(&list);
        let mut body = Vec::new();
        body.push(context.len() as u8);
        body.extend_from_slice(context);
        body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        body
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn certificate_request_requires_an_empty_context_and_pss() {
        assert!(parse_certificate_request(&cert_request(&[], &[0x0804], &[])).is_ok());
        assert!(parse_certificate_request(&cert_request(&[], &[0x0401, 0x0804], &[])).is_ok());
        let nonempty = parse_certificate_request(&cert_request(&[0x01], &[0x0804], &[])).unwrap_err();
        assert!(nonempty.contains("context"), "{nonempty}");
        let missing = parse_certificate_request(&cert_request(&[], &[0x0401], &[])).unwrap_err();
        assert!(missing.contains("signature"), "{missing}");
        let short = parse_certificate_request(&cert_request(&[], &[0x0804], &[0xff])).unwrap_err();
        assert!(short.contains("truncated") || short.contains("signature"), "{short}");
        let mut no_list = vec![0u8];
        let mut ext = Vec::new();
        ext.extend_from_slice(&0x000au16.to_be_bytes());
        ext.extend_from_slice(&2u16.to_be_bytes());
        ext.extend_from_slice(&0u16.to_be_bytes());
        no_list.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        no_list.extend_from_slice(&ext);
        assert!(parse_certificate_request(&no_list).unwrap_err().contains("signature"));
        let mut bad_vector = vec![0u8];
        let mut raw = Vec::new();
        raw.extend_from_slice(&0x000du16.to_be_bytes());
        raw.extend_from_slice(&3u16.to_be_bytes());
        raw.extend_from_slice(&[0x00, 0x02, 0x08]);
        bad_vector.extend_from_slice(&(raw.len() as u16).to_be_bytes());
        bad_vector.extend_from_slice(&raw);
        assert!(parse_certificate_request(&bad_vector).is_err());
    }

    fn framed_certs(context: &[u8], certs: &[&[u8]], tail: &[u8], bad_ext: bool) -> Vec<u8> {
        let mut list = Vec::new();
        for cert in certs {
            list.push((cert.len() >> 16) as u8);
            list.push((cert.len() >> 8) as u8);
            list.push(cert.len() as u8);
            list.extend_from_slice(cert);
            if bad_ext {
                list.extend_from_slice(&5u16.to_be_bytes());
            } else {
                list.extend_from_slice(&0u16.to_be_bytes());
            }
        }
        list.extend_from_slice(tail);
        let mut body = Vec::new();
        body.push(context.len() as u8);
        body.extend_from_slice(context);
        body.push((list.len() >> 16) as u8);
        body.push((list.len() >> 8) as u8);
        body.push(list.len() as u8);
        body.extend_from_slice(&list);
        body
    }

    fn sample_rsa_cert(serial: &[u8], extra_key: &[u8]) -> Vec<u8> {
        let version = der(0xa0, &der_int(&[2]));
        let empty = der(0x30, &[]);
        let mut key_body = der_int(&[0x80]);
        key_body.extend(der_int(&[0x01, 0x00, 0x01]));
        key_body.extend_from_slice(extra_key);
        let key_seq = der(0x30, &key_body);
        let mut bits = vec![0u8];
        bits.extend_from_slice(&key_seq);
        let mut spki_body = empty.clone();
        spki_body.extend(der(0x03, &bits));
        let spki = der(0x30, &spki_body);
        let mut tbs_body = version;
        tbs_body.extend_from_slice(serial);
        tbs_body.extend(empty.clone());
        tbs_body.extend(empty.clone());
        tbs_body.extend(empty.clone());
        tbs_body.extend(empty);
        tbs_body.extend(spki);
        let tbs = der(0x30, &tbs_body);
        let mut cert_body = tbs;
        cert_body.extend(der(0x30, &[]));
        cert_body.extend(der(0x03, &[0]));
        der(0x30, &cert_body)
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn certificate_framing_and_der_are_fully_consumed() {
        let leaf = b"leaf-cert";
        let chain = b"next-cert";
        assert_eq!(first_cert(&framed_certs(&[], &[leaf], &[], false)).unwrap(), leaf);
        assert_eq!(first_cert(&framed_certs(&[], &[leaf, chain], &[], false)).unwrap(), leaf);
        assert!(first_cert(&framed_certs(&[0xab], &[leaf], &[], false)).unwrap_err().contains("context"));
        assert!(first_cert(&framed_certs(&[], &[leaf], &[], true)).is_err());
        assert!(first_cert(&framed_certs(&[], &[leaf], &[0xff, 0xff, 0xff], false)).is_err());
        let good = sample_rsa_cert(&der_int(&[1]), &[]);
        let (n, e) = certificate_public_rsa(&good).unwrap();
        assert_eq!(n, vec![0x80]);
        assert_eq!(e, vec![0x01, 0x00, 0x01]);
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(certificate_public_rsa(&trailing).unwrap_err().contains("trailing"));
        assert!(certificate_public_rsa(&sample_rsa_cert(&der_int(&[1]), &der_int(&[7]))).is_err());
        assert!(certificate_public_rsa(&sample_rsa_cert(&[0x02, 0x81, 0x01, 0x01], &[])).unwrap_err().contains("length"));
        let key = PrivateKey::generate(512).unwrap();
        let cert = key.certificate("peer").unwrap();
        let (got_n, got_e) = certificate_public_rsa(&cert).unwrap();
        let (expect_n, expect_e) = key.public_parts();
        assert_eq!(got_n, expect_n);
        assert_eq!(got_e, expect_e);
        let message = certificate_message(&[], &cert);
        assert_eq!(first_cert(&message[4..]).unwrap(), cert);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn rsa_pss_rejects_out_of_range_signatures() {
        let key = PrivateKey::generate(2048).unwrap();
        let message = b"pss-range";
        let sig = key.sign_sha256_pss(message).unwrap();
        let (n_be, e_be) = key.public_parts();
        assert_eq!(sig.len(), (key.n.bitlen() + 7) / 8);
        PrivateKey::verify_sha256_pss(&n_be, &e_be, message, &sig).unwrap();
        let mut short = sig.clone();
        short.pop();
        assert!(PrivateKey::verify_sha256_pss(&n_be, &e_be, message, &short).unwrap_err().contains("length"));
        let mut longer = vec![0u8];
        longer.extend_from_slice(&sig);
        assert!(PrivateKey::verify_sha256_pss(&n_be, &e_be, message, &longer).unwrap_err().contains("length"));
        assert!(PrivateKey::verify_sha256_pss(&n_be, &e_be, message, &n_be).unwrap_err().contains("modulus"));
        let congruent = Nat::from_be(&sig).add(&key.n).to_be();
        let rejected = PrivateKey::verify_sha256_pss(&n_be, &e_be, message, &congruent).unwrap_err();
        assert!(rejected.contains("length") || rejected.contains("modulus"), "{rejected}");
        let pkcs = key.sign_sha1_pkcs1(b"token-bytes").unwrap();
        key.verify_sha1_pkcs1(b"token-bytes", &pkcs).unwrap();
        let lifted = Nat::from_be(&pkcs).add(&key.n).to_be();
        assert!(key.verify_sha1_pkcs1(b"token-bytes", &lifted).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn base64_padding_only_in_the_final_quartet() {
        assert_eq!(unb64(&b64(b"")).unwrap(), b"");
        assert_eq!(unb64(&b64(b"a")).unwrap(), b"a");
        assert_eq!(unb64(&b64(b"ab")).unwrap(), b"ab");
        assert_eq!(unb64(&b64(b"abc")).unwrap(), b"abc");
        assert_eq!(unb64(&b64(b"hello")).unwrap(), b"hello");
        assert!(unb64("AA=A").unwrap_err().contains("padding"));
        assert!(unb64("AA==AAAA").unwrap_err().contains("padding"));
        assert!(unb64("A=AA").unwrap_err().contains("padding"));
        assert!(unb64("=AAA").unwrap_err().contains("padding"));
        assert!(unb64("====").unwrap_err().contains("padding"));
        assert!(unb64("AB==").unwrap_err().contains("unused"));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn hello_retry_sends_an_updated_client_hello() {
        assert_eq!(HRR_RANDOM, sha256(b"HelloRetryRequest"));
        let session = [7u8; 32];
        let random = [9u8; 32];
        let cookie = b"retry-cookie";
        let cookie_only = hello_retry(&session, Some(cookie), None).unwrap();
        let plan = parse_hello_retry(&cookie_only[4..], &session).unwrap();
        assert_eq!(plan.cookie.as_deref(), Some(cookie.as_slice()));
        assert!(plan.selected.is_none());
        let rejected = parse_server_hello(&cookie_only[4..], &session).unwrap_err();
        assert!(rejected.contains("hello retry"), "{rejected}");
        assert!(!rejected.contains("truncated"), "{rejected}");
        assert!(!rejected.contains("did not send a key share"), "{rejected}");
        let selected = hello_retry(&session, None, Some(GROUP_P256)).unwrap();
        let plan = parse_hello_retry(&selected[4..], &session).unwrap();
        assert_eq!(plan.selected, Some(GROUP_P256));
        let rejected = parse_server_hello(&selected[4..], &session).unwrap_err();
        assert!(rejected.contains("hello retry"), "{rejected}");
        assert!(!rejected.contains("truncated"), "{rejected}");
        let mut share = [0u8; 65];
        share[0] = 4;
        let full = server_hello_group(&HRR_RANDOM, &session, GROUP_P256, &share);
        let err = match parse_hello_retry(&full[4..], &session) {
            Err(err) => err,
            Ok(_) => panic!("a full key share is not a selected group"),
        };
        assert!(err.contains("selected group"), "{err}");
        let already = parse_hello_retry(&selected[4..], &session).unwrap();
        let err = match retry_choice(&already, true, true) {
            Err(err) => err,
            Ok(_) => panic!("a repeated share must be refused"),
        };
        assert!(err.contains("already shared"), "{err}");
        let unsupported = hello_retry(&session, None, Some(0x0018)).unwrap();
        let plan = parse_hello_retry(&unsupported[4..], &session).unwrap();
        let err = match retry_choice(&plan, false, false) {
            Err(err) => err,
            Ok(_) => panic!("an unknown group must be refused"),
        };
        assert!(err.contains("not offered"), "{err}");
        assert!(hello_retry(&session, None, None).is_err());
        assert!(hello_retry(&session, Some(b""), None).is_err());
        let mut x_scalar = [3u8; 32];
        let mut x_public = [4u8; 32];
        let mut p_scalar = [0u8; 32];
        let mut p_public = [0u8; 65];
        let mut had_x = true;
        let mut had_p = false;
        let plan = parse_hello_retry(&selected[4..], &session).unwrap();
        let updated = updated_client_hello(&random, &session, &mut x_scalar, &mut x_public, &mut p_scalar, &mut p_public, &mut had_x, &mut had_p, &plan).unwrap();
        let offer = parse_client_hello(&updated[4..]).unwrap();
        assert!(offer.x25519.is_none());
        assert!(offer.p256.unwrap()[0] == 4);
        assert!(!had_x && had_p);
        let mut had_x = true;
        let mut had_p = true;
        let mut x_scalar = [3u8; 32];
        let mut x_public = [4u8; 32];
        let saved_x = x_public;
        let mut p_scalar = [5u8; 32];
        let mut p_public = [6u8; 65];
        p_public[0] = 4;
        let saved_p = p_public;
        let plan = parse_hello_retry(&cookie_only[4..], &session).unwrap();
        let updated = updated_client_hello(&random, &session, &mut x_scalar, &mut x_public, &mut p_scalar, &mut p_public, &mut had_x, &mut had_p, &plan).unwrap();
        assert_eq!(x_public, saved_x);
        assert_eq!(p_public, saved_p);
        let offer = parse_client_hello(&updated[4..]).unwrap();
        assert_eq!(offer.x25519.unwrap(), saved_x);
        assert_eq!(offer.p256.unwrap(), saved_p);
        assert_eq!(client_extension(&updated[4..], 0x002c).unwrap().unwrap(), cookie);
        let ch = client_hello(&random, &session, &saved_x, &saved_p);
        let mut transcript = ch.clone();
        let first_len = transcript.len();
        transcript.extend_from_slice(&cookie_only);
        fold_retry_transcript(&mut transcript, first_len).unwrap();
        assert_eq!(&transcript[..4], &[0xfe, 0, 0, 32]);
        assert_eq!(&transcript[4..36], &sha256(&ch)[..]);
        assert_eq!(&transcript[36..], &cookie_only[..]);
        use std::net::{TcpListener, TcpStream};
        use std::thread;
        let server_key = PrivateKey::generate(2048).unwrap();
        let client_key = PrivateKey::generate(2048).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nodelay(true).unwrap();
            let mut flight = new_flight(stream);
            let (kind, hello) = flight.read_hs().unwrap();
            assert_eq!(kind, 1);
            let offered = parse_client_hello(&hello).unwrap();
            let hrr = hello_retry(&offered.session, None, Some(GROUP_P256)).unwrap();
            flight.write_hs(&hrr, false).unwrap();
            let _ = read_raw(&mut flight.stream, Some(flight.deadline));
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_nodelay(true).unwrap();
        let err = match client_handshake(stream, &client_key, None) {
            Err(err) => err,
            Ok(_) => panic!("a repeated share must be refused"),
        };
        assert!(err.contains("already shared"), "{err}");
        assert!(!err.contains("truncated"), "{err}");
        peer.join().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nodelay(true).unwrap();
            let mut flight = new_flight(stream);
            let (_, hello) = flight.read_hs().unwrap();
            let offered = parse_client_hello(&hello).unwrap();
            let hrr = hello_retry(&offered.session, Some(b"once"), None).unwrap();
            let first_len = flight.transcript.len();
            flight.write_hs(&hrr, false).unwrap();
            fold_retry_transcript(&mut flight.transcript, first_len).unwrap();
            let (kind2, hello2) = flight.read_hs().unwrap();
            assert_eq!(kind2, 1);
            assert_eq!(client_extension(&hello2, 0x002c).unwrap().unwrap(), b"once");
            let offered2 = parse_client_hello(&hello2).unwrap();
            let again = hello_retry(&offered2.session, Some(b"twice"), None).unwrap();
            flight.write_hs(&again, false).unwrap();
            let _ = read_raw(&mut flight.stream, Some(std::time::Instant::now() + std::time::Duration::from_secs(2)));
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_nodelay(true).unwrap();
        let err = match client_handshake(stream, &client_key, None) {
            Err(err) => err,
            Ok(_) => panic!("a second hello retry must be refused"),
        };
        assert!(err.contains("second hello retry"), "{err}");
        peer.join().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nodelay(true).unwrap();
            server_handshake_cookie(stream, &server_key, b"retry-cookie").unwrap()
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_nodelay(true).unwrap();
        let mut client = client_handshake(stream, &client_key, None).unwrap();
        let mut server = peer.join().unwrap();
        client.write_all(b"retry-frame").unwrap();
        assert_eq!(server.read_exact(11).unwrap(), b"retry-frame");
        server.write_all(b"retry-reply").unwrap();
        assert_eq!(client.read_exact(11).unwrap(), b"retry-reply");
        let left = client.exporter(b"pair\0", 32).material().unwrap().to_vec();
        let right = server.exporter(b"pair\0", 32).material().unwrap().to_vec();
        assert_eq!(left, right);
    }

    #[cfg(rllm_module_tests)]
    pub fn run() -> usize {
        let mut passed = 0usize;
        field_matches_integer_arithmetic();
        passed += 1;
        published_hashes_and_key_schedule();
        passed += 1;
        published_aes_gcm_and_x25519();
        passed += 1;
        curve_matches_published_point_and_both_pairing_sides_agree();
        passed += 1;
        rsa_textbook_and_signature_round_trip();
        passed += 1;
        tls_handshake_exports_the_same_material_both_ways();
        passed += 1;
        hostile_lengths_do_not_panic();
        passed += 1;
        certificate_proof_is_required_before_the_session_is_usable();
        passed += 1;
        handshake_limits_reject_hostile_records();
        passed += 1;
        secret_scalar_reduction_is_the_residue_modulo_the_order();
        passed += 1;
        x25519_accepts_noncanonical_coordinates();
        passed += 1;
        send_side_record_limit_matches_the_aes_gcm_bound();
        passed += 1;
        post_handshake_framing_accepts_tickets_and_fragments();
        passed += 1;
        prime_field_group_is_offered_and_completes();
        passed += 1;
        p256_product_carry_matches_the_integer_product();
        passed += 1;
        incoming_records_stop_at_the_protocol_limit();
        passed += 1;
        certificate_request_requires_an_empty_context_and_pss();
        passed += 1;
        certificate_framing_and_der_are_fully_consumed();
        passed += 1;
        rsa_pss_rejects_out_of_range_signatures();
        passed += 1;
        base64_padding_only_in_the_final_quartet();
        passed += 1;
        hello_retry_sends_an_updated_client_hello();
        passed += 1;
        transport_certificate_need_not_match_the_saved_pairing_certificate();
        passed += 1;
        passed
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn transport_certificate_need_not_match_the_saved_pairing_certificate() {
        use std::net::TcpListener;
        use std::thread;
        let pairing_key = PrivateKey::generate(2048).unwrap();
        let transport_key = PrivateKey::generate(2048).unwrap();
        let client_key = PrivateKey::generate(2048).unwrap();
        let saved_pin = pairing_key.certificate("peer").unwrap();
        let transport_cert = transport_key.certificate("peer").unwrap();
        assert_ne!(saved_pin, transport_cert);
        let stored_transport = transport_key.store_bytes();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nodelay(true).unwrap();
            let key = PrivateKey::load_bytes(&stored_transport).unwrap();
            server_handshake(stream, &key).unwrap()
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_nodelay(true).unwrap();
        let mut client = client_handshake(stream, &client_key, Some(&saved_pin)).unwrap();
        let mut server = server.join().unwrap();
        assert_eq!(client.peer_certificate(), transport_cert.as_slice());
        assert_ne!(client.peer_certificate(), saved_pin.as_slice());
        let label = b"adb-label\0";
        assert_eq!(client.exporter(label, 64).material().unwrap(), server.exporter(label, 64).material().unwrap());
        client.write_all(b"paired-ok").unwrap();
        assert_eq!(server.read_exact(9).unwrap(), b"paired-ok");
        let unrelated = reject_peer_pin(&transport_key, &client_key, 1, Some(&saved_pin));
        assert!(!unrelated.contains("paired identity"), "{unrelated}");
        assert!(
            unrelated.contains("signature")
                || unrelated.contains("modulus")
                || unrelated.contains("intact")
                || unrelated.contains("verify")
                || unrelated.contains("smaller")
                || unrelated.contains("certificate key"),
            "{unrelated}"
        );
    }

    fn drain_until_close(stream: &mut TcpStream) {
        use std::io::Read;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
        let mut buf = [0u8; 2048];
        while stream.read(&mut buf).unwrap_or(0) > 0 {}
    }
}

#[cfg(rllm_module_tests)]
pub fn a1_fix4_adb_tls_tests() -> usize {
    tests::run()
}

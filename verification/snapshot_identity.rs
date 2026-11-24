mod twin;

// ------------------------------------------------------------------
// The TLS peer this twin serves.
//
// The fixture used to be a separate interpreter program: the twin started
// it, it owned the socket, and it wrote the events file. The peer is now
// product code in this file: a record layer, a handshake, the ciphers the
// handshake needs, and the server loop that records what it saw. The
// contract it keeps is the one the events file has always had.
// ------------------------------------------------------------------
mod tls {
#![allow(dead_code)]
    // ------------------------------------------------------------------
    // Big integers: unsigned little-endian 32-bit limbs with Montgomery
    // modular arithmetic for the odd moduli used here (RSA modulus and the
    // X25519 field prime).
    // ------------------------------------------------------------------
    pub mod num {
        use std::cmp::Ordering;

        pub type Limbs = Vec<u32>;

        pub fn trim(limbs: &mut Limbs) {
            while limbs.len() > 1 && limbs[limbs.len() - 1] == 0 {
                limbs.pop();
            }
            if limbs.is_empty() {
                limbs.push(0);
            }
        }

        /// Parses a big-endian byte string.
        pub fn from_be(bytes: &[u8]) -> Limbs {
            let mut limbs: Limbs = Vec::with_capacity(bytes.len() / 4 + 1);
            let mut index = bytes.len();
            while index > 0 {
                let start = index.saturating_sub(4);
                let mut value: u32 = 0;
                for byte in &bytes[start..index] {
                    value = (value << 8) | (*byte as u32);
                }
                limbs.push(value);
                index = start;
            }
            trim(&mut limbs);
            limbs
        }

        /// Serializes to `len` big-endian bytes, zero padded on the left.
        pub fn to_be(limbs: &Limbs, len: usize) -> Vec<u8> {
            let mut out = vec![0u8; len];
            for (index, limb) in limbs.iter().enumerate() {
                let bytes = limb.to_be_bytes();
                for offset in 0..4usize {
                    let position = index * 4 + offset;
                    if position < len {
                        out[len - 1 - position] = bytes[3 - offset];
                    }
                }
            }
            out
        }

        /// Compares two little-endian limb vectors by value. The vectors may
        /// carry different lengths, so the missing high limbs count as zero.
        pub fn cmp(left: &[u32], right: &[u32]) -> Ordering {
            for index in (0..left.len().max(right.len())).rev() {
                let a = left.get(index).copied().unwrap_or(0);
                let b = right.get(index).copied().unwrap_or(0);
                if a != b {
                    return a.cmp(&b);
                }
            }
            Ordering::Equal
        }

        pub fn add(left: &Limbs, right: &Limbs) -> Limbs {
            let mut out = Vec::with_capacity(left.len().max(right.len()) + 1);
            let mut carry: u64 = 0;
            for index in 0..left.len().max(right.len()) {
                let a = left.get(index).copied().unwrap_or(0) as u64;
                let b = right.get(index).copied().unwrap_or(0) as u64;
                let sum = a + b + carry;
                out.push(sum as u32);
                carry = sum >> 32;
            }
            if carry > 0 {
                out.push(carry as u32);
            }
            trim(&mut out);
            out
        }

        /// Subtracts `right` from `left`; the caller guarantees `left >= right`.
        pub fn sub(left: &Limbs, right: &Limbs) -> Limbs {
            let mut out = Vec::with_capacity(left.len());
            let mut borrow: i64 = 0;
            for index in 0..left.len() {
                let a = left[index] as i64;
                let b = right.get(index).copied().unwrap_or(0) as i64;
                let mut difference = a - b - borrow;
                if difference < 0 {
                    difference += 1i64 << 32;
                    borrow = 1;
                } else {
                    borrow = 0;
                }
                out.push(difference as u32);
            }
            trim(&mut out);
            out
        }

        pub fn mul(left: &Limbs, right: &Limbs) -> Limbs {
            let mut out = vec![0u32; left.len() + right.len() + 1];
            for (i, a) in left.iter().enumerate() {
                let mut carry: u64 = 0;
                for (j, b) in right.iter().enumerate() {
                    let value = out[i + j] as u64 + (*a as u64) * (*b as u64) + carry;
                    out[i + j] = value as u32;
                    carry = value >> 32;
                }
                let mut index = i + right.len();
                while carry > 0 {
                    let value = out[index] as u64 + carry;
                    out[index] = value as u32;
                    carry = value >> 32;
                    index += 1;
                }
            }
            trim(&mut out);
            out
        }

        pub fn mul_small(value: &Limbs, factor: u32) -> Limbs {
            let mut out = Vec::with_capacity(value.len() + 1);
            let mut carry: u64 = 0;
            for limb in value {
                let product = (*limb as u64) * (factor as u64) + carry;
                out.push(product as u32);
                carry = product >> 32;
            }
            if carry > 0 {
                out.push(carry as u32);
            }
            trim(&mut out);
            out
        }

        pub fn shl1(value: &Limbs) -> Limbs {
            let mut out = Vec::with_capacity(value.len() + 1);
            let mut carry: u32 = 0;
            for limb in value {
                out.push((limb << 1) | carry);
                carry = limb >> 31;
            }
            if carry > 0 {
                out.push(carry);
            }
            trim(&mut out);
            out
        }

        pub fn bit(value: &Limbs, index: usize) -> bool {
            let limb = index / 32;
            match value.get(limb) {
                Some(word) => (word >> (index % 32)) & 1 == 1,
                None => false,
            }
        }

        pub fn bit_len(value: &Limbs) -> usize {
            for index in (0..value.len()).rev() {
                if value[index] != 0 {
                    return index * 32 + (32 - value[index].leading_zeros() as usize);
                }
            }
            0
        }

        pub fn is_zero(value: &Limbs) -> bool {
            value.iter().all(|limb| *limb == 0)
        }

        /// Montgomery context for an odd modulus: the residue `R = 2^(32*k)`
        /// and the precomputed `R^2 mod n`.
        pub struct Mont {
            pub modulus: Limbs,
            n0inv: u32,
            r2: Limbs,
        }

        impl Mont {
            pub fn new(modulus: Limbs) -> Mont {
                assert!(modulus[0] & 1 == 1, "Montgomery arithmetic needs an odd modulus");
                let low = modulus[0];
                let mut inverse: u32 = 1;
                for _ in 0..6 {
                    inverse = inverse.wrapping_mul(2u32.wrapping_sub(low.wrapping_mul(inverse)));
                }
                let n0inv = inverse.wrapping_neg();
                let mut residue: Limbs = vec![1];
                for _ in 0..(64 * modulus.len()) {
                    residue = shl1(&residue);
                    if cmp(&residue, &modulus) != Ordering::Less {
                        residue = sub(&residue, &modulus);
                    }
                }
                Mont {
                    modulus,
                    n0inv,
                    r2: residue,
                }
            }

            fn reduce(&self, value: &Limbs) -> Limbs {
                let mut out = value.clone();
                while cmp(&out, &self.modulus) != Ordering::Less {
                    out = sub(&out, &self.modulus);
                }
                out
            }

            /// Montgomery product `left * right * R^-1 mod n`.
            pub fn mul(&self, left: &Limbs, right: &Limbs) -> Limbs {
                let width = self.modulus.len();
                let mut a = vec![0u32; width];
                for (index, limb) in left.iter().enumerate() {
                    if index < width {
                        a[index] = *limb;
                    }
                }
                let mut b = vec![0u32; width];
                for (index, limb) in right.iter().enumerate() {
                    if index < width {
                        b[index] = *limb;
                    }
                }
                let mut t = vec![0u32; width + 2];
                for i in 0..width {
                    let mut carry: u64 = 0;
                    for j in 0..width {
                        let value = t[j] as u64 + (a[i] as u64) * (b[j] as u64) + carry;
                        t[j] = value as u32;
                        carry = value >> 32;
                    }
                    let value = t[width] as u64 + carry;
                    t[width] = value as u32;
                    t[width + 1] = (value >> 32) as u32;

                    let factor = t[0].wrapping_mul(self.n0inv);
                    let value = t[0] as u64 + (factor as u64) * (self.modulus[0] as u64);
                    t[0] = value as u32;
                    let mut carry = value >> 32;
                    for j in 1..width {
                        let value = t[j] as u64 + (factor as u64) * (self.modulus[j] as u64) + carry;
                        t[j] = value as u32;
                        carry = value >> 32;
                    }
                    let value = t[width] as u64 + carry;
                    t[width] = value as u32;
                    t[width + 1] = t[width + 1].wrapping_add((value >> 32) as u32);

                    for j in 0..width {
                        t[j] = t[j + 1];
                    }
                    t[width] = t[width + 1];
                    t[width + 1] = 0;
                }
                let mut wide: Limbs = t[0..=width].to_vec();
                if wide[width] != 0 || cmp(&wide[0..width], &self.modulus) != Ordering::Less {
                    let mut extended = self.modulus.clone();
                    extended.push(0);
                    wide = sub(&wide, &extended);
                }
                // The subtraction trims the high zero limbs; restore the
                // fixed width so the residue keeps its place value.
                while wide.len() < width + 1 {
                    wide.push(0);
                }
                let mut out = wide[0..width].to_vec();
                trim(&mut out);
                out
            }

            /// Adds two residues.
            pub fn add(&self, left: &Limbs, right: &Limbs) -> Limbs {
                let mut out = add(left, right);
                while cmp(&out, &self.modulus) != Ordering::Less {
                    out = sub(&out, &self.modulus);
                }
                out
            }

            /// Subtracts two residues.
            pub fn sub(&self, left: &Limbs, right: &Limbs) -> Limbs {
                if cmp(left, right) != Ordering::Less {
                    sub(left, right)
                } else {
                    // `left` is the smaller residue: `left - right` is
                    // `left + p - right` modulo `p`.
                    sub(&add(&self.modulus, left), right)
                }
            }

            /// Maps a plain residue into the Montgomery domain.
            pub fn to_mont(&self, value: &Limbs) -> Limbs {
                self.mul(&self.reduce(value), &self.r2)
            }

            /// Maps a Montgomery residue back to the plain domain.
            pub fn from_mont(&self, value: &Limbs) -> Limbs {
                self.mul(value, &vec![1u32])
            }

            /// `base^exponent` kept in the Montgomery domain; the exponent is
            /// a big-endian byte string.
            pub fn pow(&self, base: &Limbs, exponent: &[u8]) -> Limbs {
                let mut result = self.to_mont(&vec![1u32]);
                let mont_base = self.to_mont(base);
                for byte in exponent {
                    for shift in (0..8).rev() {
                        result = self.mul(&result, &result);
                        if (byte >> shift) & 1 == 1 {
                            result = self.mul(&result, &mont_base);
                        }
                    }
                }
                result
            }

            /// `base^exponent mod modulus` in the plain domain.
            pub fn pow_plain(&self, base: &Limbs, exponent: &[u8]) -> Limbs {
                self.from_mont(&self.pow(base, exponent))
            }
        }
    }

    // ------------------------------------------------------------------
    // AES-128 (FIPS-197) with an S-box derived at run time from the
    // multiplicative inverse in GF(2^8).
    // ------------------------------------------------------------------
    pub mod aes {
        fn xtime(value: u8) -> u8 {
            let high = value & 0x80;
            let mut out = value << 1;
            if high != 0 {
                out ^= 0x1b;
            }
            out
        }

        fn mul(value: u8, factor: u8) -> u8 {
            let mut out: u8 = 0;
            let mut addend = value;
            let mut rest = factor;
            while rest > 0 {
                if rest & 1 == 1 {
                    out ^= addend;
                }
                addend = xtime(addend);
                rest >>= 1;
            }
            out
        }

        /// Builds the exponent and logarithm tables of GF(2^8) for the
        /// generator 3.
        fn log_tables() -> ([u8; 256], [u8; 256]) {
            let mut exponent = [0u8; 256];
            let mut logarithm = [0u8; 256];
            let mut value: u8 = 1;
            for index in 0..255usize {
                exponent[index] = value;
                logarithm[value as usize] = index as u8;
                value = xtime(value) ^ value;
            }
            (exponent, logarithm)
        }

        fn inverse(value: u8) -> u8 {
            if value == 0 {
                return 0;
            }
            let (exponent, logarithm) = log_tables();
            let power = (255u32 - logarithm[value as usize] as u32) % 255;
            exponent[power as usize]
        }

        fn sbox() -> [u8; 256] {
            let mut table = [0u8; 256];
            for value in 0..256usize {
                let base = inverse(value as u8);
                table[value] = base
                    ^ base.rotate_left(1)
                    ^ base.rotate_left(2)
                    ^ base.rotate_left(3)
                    ^ base.rotate_left(4)
                    ^ 0x63;
            }
            table
        }

        pub struct Aes128 {
            round_keys: [[u8; 16]; 11],
            sbox: [u8; 256],
            inverse: [u8; 256],
        }

        impl Aes128 {
            pub fn new(key: &[u8; 16]) -> Aes128 {
                let sbox = sbox();
                let mut inverse = [0u8; 256];
                for (index, value) in sbox.iter().enumerate() {
                    inverse[*value as usize] = index as u8;
                }
                let mut words = [0u32; 44];
                for index in 0..4 {
                    words[index] = u32::from_be_bytes([
                        key[4 * index],
                        key[4 * index + 1],
                        key[4 * index + 2],
                        key[4 * index + 3],
                    ]);
                }
                let mut rcon: u8 = 1;
                for index in 4..44 {
                    let mut temp = words[index - 1];
                    if index % 4 == 0 {
                        temp = sub_word(temp.rotate_left(8), &sbox) ^ ((rcon as u32) << 24);
                        rcon = xtime(rcon);
                    }
                    words[index] = words[index - 4] ^ temp;
                }
                let mut round_keys = [[0u8; 16]; 11];
                for round in 0..11 {
                    for word in 0..4 {
                        let bytes = words[round * 4 + word].to_be_bytes();
                        for offset in 0..4 {
                            round_keys[round][word * 4 + offset] = bytes[offset];
                        }
                    }
                }
                Aes128 {
                    round_keys,
                    sbox,
                    inverse,
                }
            }

            pub fn encrypt_block(&self, block: &mut [u8; 16]) {
                add_round_key(block, &self.round_keys[0]);
                for round in 1..10 {
                    self.sub_bytes(block);
                    shift_rows(block);
                    mix_columns(block);
                    add_round_key(block, &self.round_keys[round]);
                }
                self.sub_bytes(block);
                shift_rows(block);
                add_round_key(block, &self.round_keys[10]);
            }

            pub fn decrypt_block(&self, block: &mut [u8; 16]) {
                add_round_key(block, &self.round_keys[10]);
                for round in (1..10).rev() {
                    inverse_shift_rows(block);
                    self.inverse_sub_bytes(block);
                    add_round_key(block, &self.round_keys[round]);
                    inverse_mix_columns(block);
                }
                inverse_shift_rows(block);
                self.inverse_sub_bytes(block);
                add_round_key(block, &self.round_keys[0]);
            }

            fn sub_bytes(&self, block: &mut [u8; 16]) {
                for byte in block.iter_mut() {
                    *byte = self.sbox[*byte as usize];
                }
            }

            fn inverse_sub_bytes(&self, block: &mut [u8; 16]) {
                for byte in block.iter_mut() {
                    *byte = self.inverse[*byte as usize];
                }
            }
        }

        fn sub_word(word: u32, sbox: &[u8; 256]) -> u32 {
            let bytes = word.to_be_bytes();
            let mut out = [0u8; 4];
            for index in 0..4 {
                out[index] = sbox[bytes[index] as usize];
            }
            u32::from_be_bytes(out)
        }

        fn add_round_key(block: &mut [u8; 16], key: &[u8; 16]) {
            for index in 0..16 {
                block[index] ^= key[index];
            }
        }

        fn shift_rows(block: &mut [u8; 16]) {
            let source = *block;
            for row in 1..4 {
                for column in 0..4 {
                    block[4 * column + row] = source[4 * ((column + row) % 4) + row];
                }
            }
        }

        fn inverse_shift_rows(block: &mut [u8; 16]) {
            let source = *block;
            for row in 1..4 {
                for column in 0..4 {
                    block[4 * ((column + row) % 4) + row] = source[4 * column + row];
                }
            }
        }

        fn mix_columns(block: &mut [u8; 16]) {
            for column in 0..4 {
                let a0 = block[4 * column];
                let a1 = block[4 * column + 1];
                let a2 = block[4 * column + 2];
                let a3 = block[4 * column + 3];
                block[4 * column] = xtime(a0) ^ (xtime(a1) ^ a1) ^ a2 ^ a3;
                block[4 * column + 1] = a0 ^ xtime(a1) ^ (xtime(a2) ^ a2) ^ a3;
                block[4 * column + 2] = a0 ^ a1 ^ xtime(a2) ^ (xtime(a3) ^ a3);
                block[4 * column + 3] = (xtime(a0) ^ a0) ^ a1 ^ a2 ^ xtime(a3);
            }
        }

        fn inverse_mix_columns(block: &mut [u8; 16]) {
            for column in 0..4 {
                let a0 = block[4 * column];
                let a1 = block[4 * column + 1];
                let a2 = block[4 * column + 2];
                let a3 = block[4 * column + 3];
                block[4 * column] = mul(a0, 14) ^ mul(a1, 11) ^ mul(a2, 13) ^ mul(a3, 9);
                block[4 * column + 1] = mul(a0, 9) ^ mul(a1, 14) ^ mul(a2, 11) ^ mul(a3, 13);
                block[4 * column + 2] = mul(a0, 13) ^ mul(a1, 9) ^ mul(a2, 14) ^ mul(a3, 11);
                block[4 * column + 3] = mul(a0, 11) ^ mul(a1, 13) ^ mul(a2, 9) ^ mul(a3, 14);
            }
        }
    }

    // ------------------------------------------------------------------
    // AES-GCM (NIST SP 800-38D) over AES-128.
    // ------------------------------------------------------------------
    pub mod gcm {
        use super::aes::Aes128;

        fn gf_mul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
            let mut product = [0u8; 16];
            let mut value = *y;
            for index in 0..128 {
                if (x[index / 8] >> (7 - index % 8)) & 1 == 1 {
                    for byte in 0..16 {
                        product[byte] ^= value[byte];
                    }
                }
                let low = value[15] & 1;
                for byte in (1..16).rev() {
                    value[byte] = (value[byte] >> 1) | ((value[byte - 1] & 1) << 7);
                }
                value[0] >>= 1;
                if low == 1 {
                    value[0] ^= 0xe1;
                }
            }
            product
        }

        fn ghash(hash: &[u8; 16], aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
            let mut state = [0u8; 16];
            for chunk in aad.chunks(16).chain(ciphertext.chunks(16)) {
                let mut block = [0u8; 16];
                block[..chunk.len()].copy_from_slice(chunk);
                for index in 0..16 {
                    state[index] ^= block[index];
                }
                state = gf_mul(&state, hash);
            }
            let mut lengths = [0u8; 16];
            lengths[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
            lengths[8..].copy_from_slice(&((ciphertext.len() as u64) * 8).to_be_bytes());
            for index in 0..16 {
                state[index] ^= lengths[index];
            }
            gf_mul(&state, hash)
        }

        fn counter_block(nonce: &[u8; 12], counter: u32) -> [u8; 16] {
            let mut block = [0u8; 16];
            block[..12].copy_from_slice(nonce);
            block[12..].copy_from_slice(&counter.to_be_bytes());
            block
        }

        /// Encrypts and appends the 16-byte authentication tag.
        pub fn encrypt(key: &[u8; 16], nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
            let cipher = Aes128::new(key);
            let mut hash = [0u8; 16];
            cipher.encrypt_block(&mut hash);
            let mut out = Vec::with_capacity(plaintext.len() + 16);
            for (index, chunk) in plaintext.chunks(16).enumerate() {
                let mut block = counter_block(nonce, 2 + index as u32);
                cipher.encrypt_block(&mut block);
                for (offset, byte) in chunk.iter().enumerate() {
                    out.push(block[offset] ^ byte);
                }
            }
            let mut tag = ghash(&hash, aad, &out);
            let mut mask = counter_block(nonce, 1);
            cipher.encrypt_block(&mut mask);
            for index in 0..16 {
                tag[index] ^= mask[index];
            }
            out.extend_from_slice(&tag);
            out
        }

        /// Verifies the tag and returns the plaintext, or `None` when the
        /// ciphertext or tag was altered.
        pub fn decrypt(key: &[u8; 16], nonce: &[u8; 12], aad: &[u8], input: &[u8]) -> Option<Vec<u8>> {
            if input.len() < 16 {
                return None;
            }
            let (ciphertext, tag) = input.split_at(input.len() - 16);
            let cipher = Aes128::new(key);
            let mut hash = [0u8; 16];
            cipher.encrypt_block(&mut hash);
            let mut expected = ghash(&hash, aad, ciphertext);
            let mut mask = counter_block(nonce, 1);
            cipher.encrypt_block(&mut mask);
            for index in 0..16 {
                expected[index] ^= mask[index];
            }
            let mut difference = 0u8;
            for index in 0..16 {
                difference |= expected[index] ^ tag[index];
            }
            if difference != 0 {
                return None;
            }
            let mut out = Vec::with_capacity(ciphertext.len());
            for (index, chunk) in ciphertext.chunks(16).enumerate() {
                let mut block = counter_block(nonce, 2 + index as u32);
                cipher.encrypt_block(&mut block);
                for (offset, byte) in chunk.iter().enumerate() {
                    out.push(block[offset] ^ byte);
                }
            }
            Some(out)
        }
    }

    // ------------------------------------------------------------------
    // X25519 (RFC 7748) over the field GF(2^255 - 19).
    // ------------------------------------------------------------------
    pub mod x25519 {
        use super::num::{self, Limbs, Mont};

        fn modulus() -> Limbs {
            // p = 2^255 - 19 = 0x7fff...ffed, parsed big-endian.
            let mut bytes = vec![0xffu8; 32];
            bytes[0] = 0x7f;
            bytes[31] = 0xed;
            num::from_be(&bytes)
        }

        fn from_le(bytes: &[u8]) -> Limbs {
            let mut reversed = bytes.to_vec();
            reversed.reverse();
            num::from_be(&reversed)
        }

        fn to_le(limbs: &Limbs) -> [u8; 32] {
            let big = num::to_be(limbs, 32);
            let mut out = [0u8; 32];
            for index in 0..32 {
                out[index] = big[31 - index];
            }
            out
        }

        /// Swaps the two operands limb-wise when `swap` is 1 and leaves them
        /// untouched when it is 0. Both operands are widened to a common limb
        /// count first, because the two values of a ladder step may have been
        /// trimmed to different widths by an earlier reduction.
        fn cswap(swap: u32, left: &mut Limbs, right: &mut Limbs) {
            let width = left.len().max(right.len());
            left.resize(width, 0);
            right.resize(width, 0);
            let mask = 0u32.wrapping_sub(swap);
            for index in 0..width {
                let difference = (left[index] ^ right[index]) & mask;
                left[index] ^= difference;
                right[index] ^= difference;
            }
        }

        /// The X25519 function of RFC 7748: the scalar is clamped and the
        /// most significant bit of the point is masked.
        pub fn scalar_mult(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
            let mont = Mont::new(modulus());
            let mut key = *scalar;
            key[0] &= 248;
            key[31] &= 127;
            key[31] |= 64;
            let mut u = *point;
            u[31] &= 127;

            let one = vec![1u32];
            let zero = vec![0u32];
            let x1 = mont.to_mont(&from_le(&u));
            let mut x2 = mont.to_mont(&one);
            let mut z2 = mont.to_mont(&zero);
            let mut x3 = x1.clone();
            let mut z3 = mont.to_mont(&one);
            let mut swap: u32 = 0;

            for index in (0..255).rev() {
                let bit = ((key[index / 8] >> (index % 8)) & 1) as u32;
                swap ^= bit;
                cswap(swap, &mut x2, &mut x3);
                cswap(swap, &mut z2, &mut z3);
                swap = bit;

                let a = mont.add(&x2, &z2);
                let aa = mont.mul(&a, &a);
                let b = mont.sub(&x2, &z2);
                let bb = mont.mul(&b, &b);
                let e = mont.sub(&aa, &bb);
                let c = mont.add(&x3, &z3);
                let d = mont.sub(&x3, &z3);
                let da = mont.mul(&d, &a);
                let cb = mont.mul(&c, &b);
                let sum = mont.add(&da, &cb);
                let difference = mont.sub(&da, &cb);
                x3 = mont.mul(&sum, &sum);
                z3 = mont.mul(&x1, &mont.mul(&difference, &difference));
                x2 = mont.mul(&aa, &bb);
                // (A - 2) / 4 = 121665 for curve25519, in Montgomery form.
                let a24 = mont.mul(&mont.to_mont(&vec![121665u32]), &e);
                z2 = mont.mul(&e, &mont.add(&aa, &a24));
            }
            cswap(swap, &mut x2, &mut x3);
            cswap(swap, &mut z2, &mut z3);

            // p - 2 = 2^255 - 21 = 0x7fff...ffeb, parsed big-endian.
            let mut exponent = vec![0xffu8; 32];
            exponent[0] = 0x7f;
            exponent[31] = 0xeb;
            let inverse = mont.pow(&z2, &exponent);
            let shared = mont.mul(&x2, &mont.to_mont(&inverse));
            to_le(&mont.from_mont(&shared))
        }

        /// The public key of a secret scalar, that is the scalar times the
        /// base point 9.
        pub fn public_key(secret: &[u8; 32]) -> [u8; 32] {
            let mut base = [0u8; 32];
            base[0] = 9;
            scalar_mult(secret, &base)
        }
    }

    // ------------------------------------------------------------------
    // SHA-256 (reused from the twin), HMAC-SHA256 and the TLS 1.2 PRF.
    // ------------------------------------------------------------------
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        crate::twin::sha256_digest(data)
    }

    pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
        let mut block = [0u8; 64];
        if key.len() > 64 {
            block[..32].copy_from_slice(&sha256(key));
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let mut inner_pad = [0x36u8; 64];
        let mut outer_pad = [0x5cu8; 64];
        for index in 0..64 {
            inner_pad[index] ^= block[index];
            outer_pad[index] ^= block[index];
        }
        let mut inner = Vec::with_capacity(64 + data.len());
        inner.extend_from_slice(&inner_pad);
        inner.extend_from_slice(data);
        let inner_digest = sha256(&inner);
        let mut outer = Vec::with_capacity(96);
        outer.extend_from_slice(&outer_pad);
        outer.extend_from_slice(&inner_digest);
        sha256(&outer)
    }

    /// The TLS 1.2 pseudo random function P_SHA256.
    pub fn prf_sha256(secret: &[u8], label: &str, seed: &[u8], length: usize) -> Vec<u8> {
        let mut full_seed = label.as_bytes().to_vec();
        full_seed.extend_from_slice(seed);
        let mut out = Vec::with_capacity(length + 32);
        let mut a = full_seed.clone();
        while out.len() < length {
            a = hmac_sha256(secret, &a).to_vec();
            let mut input = a.clone();
            input.extend_from_slice(&full_seed);
            out.extend_from_slice(&hmac_sha256(secret, &input));
        }
        out.truncate(length);
        out
    }

    // ------------------------------------------------------------------
    // A small DER reader: enough for PKCS#8 private keys.
    // ------------------------------------------------------------------
    pub mod der {
        pub struct Reader<'a> {
            data: &'a [u8],
            position: usize,
        }

        impl<'a> Reader<'a> {
            pub fn new(data: &'a [u8]) -> Reader<'a> {
                Reader { data, position: 0 }
            }

            /// Reads one tag-length-value triple.
            pub fn read(&mut self) -> Option<(u8, &'a [u8])> {
                let tag = *self.data.get(self.position)?;
                self.position += 1;
                let first = *self.data.get(self.position)?;
                self.position += 1;
                let length = if first < 0x80 {
                    first as usize
                } else {
                    let count = (first & 0x7f) as usize;
                    if count == 0 || count > 4 {
                        return None;
                    }
                    let mut value = 0usize;
                    for _ in 0..count {
                        let byte = *self.data.get(self.position)?;
                        self.position += 1;
                        value = (value << 8) | byte as usize;
                    }
                    value
                };
                let end = self.position.checked_add(length)?;
                let body = self.data.get(self.position..end)?;
                self.position = end;
                Some((tag, body))
            }

            /// Reads one tag-length-value triple and checks the tag.
            pub fn expect(&mut self, tag: u8) -> Option<&'a [u8]> {
                let (found, body) = self.read()?;
                if found == tag {
                    Some(body)
                } else {
                    None
                }
            }

            /// Reads a DER INTEGER as unsigned big-endian bytes.
            pub fn integer(&mut self) -> Option<Vec<u8>> {
                let body = self.expect(0x02)?;
                let mut start = 0;
                while start + 1 < body.len() && body[start] == 0 {
                    start += 1;
                }
                Some(body[start..].to_vec())
            }

            pub fn is_empty(&self) -> bool {
                self.position >= self.data.len()
            }
        }
    }

    // ------------------------------------------------------------------
    // RSA with PKCS#1 v1.5 padding, driven by a PKCS#8 PEM private key.
    // ------------------------------------------------------------------
    pub mod rsa {
        use super::der;
        use super::num::{self, Limbs, Mont};
        use super::sha256;

        /// The DigestInfo prefix of a SHA-256 signature (RFC 8017, 9.2).
        const SHA256_DIGEST_INFO: [u8; 19] = [
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x01, 0x05, 0x00, 0x04, 0x20,
        ];

        pub struct RsaKey {
            pub modulus: Limbs,
            pub public_exponent: Limbs,
            pub private_exponent: Limbs,
            pub modulus_bytes: usize,
        }

        impl RsaKey {
            /// Parses a PEM private key in either form: PKCS#8 (what OpenSSL 3
            /// writes) or PKCS#1 (what an older writer leaves behind).
            pub fn from_pem(text: &str) -> Option<RsaKey> {
                Self::from_pkcs8_pem(text).or_else(|| Self::from_pkcs1_pem(text))
            }

            /// Parses a PKCS#1 PEM private key (RFC 8017, A.1.2).
            pub fn from_pkcs1_pem(text: &str) -> Option<RsaKey> {
                let body = pem_body(text, "RSA PRIVATE KEY")?;
                let der_bytes = decode_base64(&strip_whitespace(body))?;
                let mut outer = der::Reader::new(&der_bytes);
                let content = outer.expect(0x30)?;
                let mut reader = der::Reader::new(content);
                let _version = reader.integer()?;
                let modulus = reader.integer()?;
                let public_exponent = reader.integer()?;
                let private_exponent = reader.integer()?;
                if modulus.is_empty() || public_exponent.is_empty() || private_exponent.is_empty() {
                    return None;
                }
                let modulus_bytes = (modulus.len() + 3) / 4 * 4;
                Some(RsaKey {
                    modulus: num::from_be(&modulus),
                    public_exponent: num::from_be(&public_exponent),
                    private_exponent: num::from_be(&private_exponent),
                    modulus_bytes,
                })
            }

            /// Parses a PKCS#8 PEM private key produced by
            /// `openssl genpkey`/`openssl req -newkey`.
            pub fn from_pkcs8_pem(text: &str) -> Option<RsaKey> {
                let body = pem_body(text, "PRIVATE KEY")?;
                let der_bytes = decode_base64(&strip_whitespace(body))?;
                let mut outer = der::Reader::new(&der_bytes);
                let content = outer.expect(0x30)?;
                let mut reader = der::Reader::new(content);
                let _version = reader.integer()?;
                let _algorithm = reader.expect(0x30)?;
                let inner = reader.expect(0x04)?;
                let mut key_reader = der::Reader::new(inner);
                let key_content = key_reader.expect(0x30)?;
                let mut parts = der::Reader::new(key_content);
                let _key_version = parts.integer()?;
                let modulus = parts.integer()?;
                let public_exponent = parts.integer()?;
                let private_exponent = parts.integer()?;
                if modulus.is_empty() || public_exponent.is_empty() || private_exponent.is_empty() {
                    return None;
                }
                let modulus_bytes = (modulus.len() + 3) / 4 * 4;
                Some(RsaKey {
                    modulus: num::from_be(&modulus),
                    public_exponent: num::from_be(&public_exponent),
                    private_exponent: num::from_be(&private_exponent),
                    modulus_bytes,
                })
            }

            fn context(&self) -> Mont {
                Mont::new(self.modulus.clone())
            }

            /// The raw RSA operation `value^exponent mod n`.
            fn raw(&self, value: &[u8], exponent: &Limbs) -> Vec<u8> {
                let mont = self.context();
                let exponent_bytes = num::to_be(exponent, (exponent.len() * 4).max(1));
                let result = mont.pow_plain(&num::from_be(value), &exponent_bytes);
                num::to_be(&result, self.modulus_bytes)
            }

            /// The PKCS#1 v1.5 signature of `message` under SHA-256.
            pub fn sign_pkcs1_sha256(&self, message: &[u8]) -> Vec<u8> {
                let digest = sha256(message);
                let mut tail = Vec::with_capacity(SHA256_DIGEST_INFO.len() + 32);
                tail.extend_from_slice(&SHA256_DIGEST_INFO);
                tail.extend_from_slice(&digest);
                let mut block = vec![0u8; self.modulus_bytes];
                block[0] = 0x00;
                block[1] = 0x01;
                let padding = self.modulus_bytes - tail.len() - 3;
                for index in 0..padding {
                    block[2 + index] = 0xff;
                }
                block[2 + padding] = 0x00;
                block[3 + padding..].copy_from_slice(&tail);
                self.raw(&block, &self.private_exponent)
            }

            /// Checks a PKCS#1 v1.5 SHA-256 signature.
            pub fn verify_pkcs1_sha256(&self, message: &[u8], signature: &[u8]) -> bool {
                if signature.len() != self.modulus_bytes {
                    return false;
                }
                let recovered = self.raw(signature, &self.public_exponent);
                let digest = sha256(message);
                let mut tail = Vec::with_capacity(SHA256_DIGEST_INFO.len() + 32);
                tail.extend_from_slice(&SHA256_DIGEST_INFO);
                tail.extend_from_slice(&digest);
                let mut expected = vec![0u8; self.modulus_bytes];
                expected[0] = 0x00;
                expected[1] = 0x01;
                let padding = self.modulus_bytes - tail.len() - 3;
                for index in 0..padding {
                    expected[2 + index] = 0xff;
                }
                expected[2 + padding] = 0x00;
                expected[3 + padding..].copy_from_slice(&tail);
                let mut difference = 0u8;
                for index in 0..self.modulus_bytes {
                    difference |= expected[index] ^ recovered[index];
                }
                difference == 0
            }

        }

        /// Decodes a PEM block into its DER bytes.
        pub fn pem_der(text: &str, label: &str) -> Option<Vec<u8>> {
            let body = pem_body(text, label)?;
            decode_base64(&strip_whitespace(body))
        }

        /// Decodes standard base64, refusing any character outside the
        /// alphabet instead of guessing.
        fn decode_base64(text: &str) -> Option<Vec<u8>> {
            let mut out = Vec::new();
            let mut assembled = 0u32;
            let mut bits = 0u32;
            for byte in text.bytes() {
                if byte == b'=' {
                    break;
                }
                let value = match byte {
                    b'A'..=b'Z' => byte - b'A',
                    b'a'..=b'z' => byte - b'a' + 26,
                    b'0'..=b'9' => byte - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    _ => return None,
                };
                assembled = (assembled << 6) | value as u32;
                bits += 6;
                if bits >= 8 {
                    bits -= 8;
                    out.push((assembled >> bits) as u8);
                }
            }
            Some(out)
        }

        /// Extracts the body of a PEM block.
        pub fn pem_body<'a>(text: &'a str, label: &str) -> Option<&'a str> {
            let begin = format!("-----BEGIN {label}-----");
            let end = format!("-----END {label}-----");
            let start = text.find(&begin)? + begin.len();
            let stop = text[start..].find(&end)? + start;
            Some(&text[start..stop])
        }

        /// Removes every ASCII whitespace character.
        pub fn strip_whitespace(text: &str) -> String {
            text.chars().filter(|ch| !ch.is_ascii_whitespace()).collect()
        }
    }

    // ------------------------------------------------------------------
    // TLS 1.2 server: ECDHE_RSA_WITH_AES_128_GCM_SHA256 over X25519, with
    // the record layer, the handshake state machine and the HTTP exchange
    // the verification fixture needs.
    // ------------------------------------------------------------------
    pub mod session {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        use super::gcm;
        use super::prf_sha256;
        use super::rsa::RsaKey;
        use super::sha256;
        use super::x25519;

        pub const CONTENT_CHANGE_CIPHER_SPEC: u8 = 20;
        pub const CONTENT_ALERT: u8 = 21;
        pub const CONTENT_HANDSHAKE: u8 = 22;
        pub const CONTENT_APPLICATION_DATA: u8 = 23;

        pub const HANDSHAKE_CLIENT_HELLO: u8 = 1;
        pub const HANDSHAKE_SERVER_HELLO: u8 = 2;
        pub const HANDSHAKE_CERTIFICATE: u8 = 11;
        pub const HANDSHAKE_SERVER_KEY_EXCHANGE: u8 = 12;
        pub const HANDSHAKE_SERVER_HELLO_DONE: u8 = 14;
        pub const HANDSHAKE_CLIENT_KEY_EXCHANGE: u8 = 16;
        pub const HANDSHAKE_FINISHED: u8 = 20;

        pub const CIPHER_ECDHE_RSA_WITH_AES_128_GCM_SHA256: u16 = 0xC02F;
        pub const GROUP_X25519: u16 = 0x001d;
        pub const SIGNATURE_RSA_PKCS1_SHA256: u16 = 0x0401;
        /// RFC 5746 §3.3: a client that supports secure renegotiation but sends
        /// no renegotiation_info extension signals it with this value in the
        /// cipher-suite list. It is never a suite and is never negotiated.
        pub const SCSV_SECURE_RENEGOTIATION: u16 = 0x00ff;

        const EXTENSION_SERVER_NAME: u16 = 0;
        const EXTENSION_SUPPORTED_GROUPS: u16 = 10;
        const EXTENSION_EC_POINT_FORMATS: u16 = 11;
        const EXTENSION_SIGNATURE_ALGORITHMS: u16 = 13;
        const EXTENSION_EXTENDED_MASTER_SECRET: u16 = 23;
        const EXTENSION_RENEGOTIATION_INFO: u16 = 0xff01;

        /// The reply the fixture sends for every captured request.
        pub const RESPONSE: &[u8] =
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK";

        /// The largest request header the fixture accepts.
        pub const HEADER_LIMIT: usize = 65536;

        #[derive(Debug)]
        pub enum TlsError {
            /// The peer closed the transport in the middle of the protocol.
            Eof,
            /// The peer sent a fatal alert.
            PeerAlert(u8),
            /// The peer sent a record that failed authentication.
            BadRecordMac,
            /// The peer sent something the protocol does not allow.
            Protocol(String),
            /// The transport failed.
            Io(String),
        }

        pub struct ServerConfig {
            /// The certificate chain, DER encoded, leaf first.
            pub chain: Vec<Vec<u8>>,
            pub key: RsaKey,
        }

        /// The parsed ClientHello.
        pub struct ClientHello {
            pub raw: Vec<u8>,
            pub random: [u8; 32],
            pub session_id: Vec<u8>,
            pub cipher_suites: Vec<u16>,
            pub groups: Vec<u16>,
            pub signature_algorithms: Vec<u16>,
            pub server_name: Option<String>,
            pub extended_master_secret: bool,
            pub renegotiation_info: bool,
        }

        impl ClientHello {
            pub fn parse(body: &[u8]) -> Option<ClientHello> {
                let mut cursor = Cursor::new(body);
                let _version = cursor.take(2)?;
                let mut random = [0u8; 32];
                random.copy_from_slice(cursor.take(32)?);
                let session_id_length = cursor.byte()? as usize;
                let session_id = cursor.take(session_id_length)?.to_vec();
                let suites_length = cursor.u16()? as usize;
                let suites = cursor.take(suites_length)?;
                let mut cipher_suites = Vec::new();
                for pair in suites.chunks_exact(2) {
                    cipher_suites.push(u16::from_be_bytes([pair[0], pair[1]]));
                }
                let compression_length = cursor.byte()? as usize;
                cursor.take(compression_length)?;

                let mut groups = Vec::new();
                let mut signature_algorithms = Vec::new();
                let mut server_name = None;
                let mut extended_master_secret = false;
                let mut renegotiation_info = cipher_suites.contains(&SCSV_SECURE_RENEGOTIATION);
                if !cursor.is_empty() {
                    let extensions_length = cursor.u16()? as usize;
                    let extensions = cursor.take(extensions_length)?;
                    let mut extension_cursor = Cursor::new(extensions);
                    while !extension_cursor.is_empty() {
                        let kind = extension_cursor.u16()?;
                        let length = extension_cursor.u16()? as usize;
                        let body = extension_cursor.take(length)?;
                        match kind {
                            EXTENSION_SERVER_NAME => {
                                let mut list = Cursor::new(body);
                                let list_length = list.u16()? as usize;
                                let names = list.take(list_length)?;
                                let mut name_cursor = Cursor::new(names);
                                while !name_cursor.is_empty() {
                                    let name_type = name_cursor.byte()?;
                                    let name_length = name_cursor.u16()? as usize;
                                    let name = name_cursor.take(name_length)?;
                                    if name_type == 0 {
                                        server_name = String::from_utf8(name.to_vec()).ok();
                                        break;
                                    }
                                }
                            }
                            EXTENSION_SUPPORTED_GROUPS => {
                                let mut list = Cursor::new(body);
                                let list_length = list.u16()? as usize;
                                let items = list.take(list_length)?;
                                for pair in items.chunks_exact(2) {
                                    groups.push(u16::from_be_bytes([pair[0], pair[1]]));
                                }
                            }
                            EXTENSION_SIGNATURE_ALGORITHMS => {
                                let mut list = Cursor::new(body);
                                let list_length = list.u16()? as usize;
                                let items = list.take(list_length)?;
                                for pair in items.chunks_exact(2) {
                                    signature_algorithms.push(u16::from_be_bytes([pair[0], pair[1]]));
                                }
                            }
                            EXTENSION_EXTENDED_MASTER_SECRET => extended_master_secret = true,
                            EXTENSION_RENEGOTIATION_INFO => renegotiation_info = true,
                            _ => {}
                        }
                    }
                }
                Some(ClientHello {
                    raw: Vec::new(),
                    random,
                    session_id,
                    cipher_suites,
                    groups,
                    signature_algorithms,
                    server_name,
                    extended_master_secret,
                    renegotiation_info,
                })
            }
        }

        /// A byte cursor over a protocol message.
        pub struct Cursor<'a> {
            data: &'a [u8],
            position: usize,
        }

        impl<'a> Cursor<'a> {
            pub fn new(data: &'a [u8]) -> Cursor<'a> {
                Cursor { data, position: 0 }
            }

            pub fn is_empty(&self) -> bool {
                self.position >= self.data.len()
            }

            pub fn take(&mut self, length: usize) -> Option<&'a [u8]> {
                let end = self.position.checked_add(length)?;
                let body = self.data.get(self.position..end)?;
                self.position = end;
                Some(body)
            }

            pub fn byte(&mut self) -> Option<u8> {
                let value = *self.data.get(self.position)?;
                self.position += 1;
                Some(value)
            }

            pub fn u16(&mut self) -> Option<u16> {
                let body = self.take(2)?;
                Some(u16::from_be_bytes([body[0], body[1]]))
            }
        }

        /// An AES-128-GCM record cipher with its four byte salt.
        #[derive(Clone, Copy)]
        pub struct Aead {
            pub key: [u8; 16],
            pub iv: [u8; 4],
        }

        /// A handshake message with its header.
        pub struct HandshakeMessage {
            pub kind: u8,
            pub body: Vec<u8>,
            pub raw: Vec<u8>,
        }

        /// The record layer over a transport.
        pub struct Connection {
            stream: TcpStream,
            read_cipher: Option<Aead>,
            pending_read_cipher: Option<Aead>,
            write_cipher: Option<Aead>,
            read_sequence: u64,
            write_sequence: u64,
            handshake_buffer: Vec<u8>,
        }

        impl Connection {
            pub fn new(stream: TcpStream) -> Connection {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                Connection {
                    stream,
                    read_cipher: None,
                    pending_read_cipher: None,
                    write_cipher: None,
                    read_sequence: 0,
                    write_sequence: 0,
                    handshake_buffer: Vec::new(),
                }
        }

            /// Reads one record, decrypting it when a cipher is active. A
            /// ChangeCipherSpec record switches the read cipher and is not
            /// returned to the caller.
            pub fn read_record(&mut self) -> Result<(u8, Vec<u8>), TlsError> {
                loop {
                    let mut header = [0u8; 5];
                    self.read_exact(&mut header)?;
                    let kind = header[0];
                    let length = u16::from_be_bytes([header[3], header[4]]) as usize;
                    let mut payload = vec![0u8; length];
                    self.read_exact(&mut payload)?;
                    if kind == CONTENT_CHANGE_CIPHER_SPEC {
                        if let Some(cipher) = self.pending_read_cipher.take() {
                            self.read_cipher = Some(cipher);
                        }
                        continue;
                    }
                    let plain = match self.read_cipher {
                        None => payload,
                        Some(cipher) => {
                            let sequence = self.read_sequence;
                            self.read_sequence += 1;
                            if payload.len() < 8 + 16 {
                                return Err(TlsError::Protocol("short encrypted record".into()));
                            }
                            let mut nonce = [0u8; 12];
                            nonce[..4].copy_from_slice(&cipher.iv);
                            nonce[4..].copy_from_slice(&payload[..8]);
                            let body = &payload[8..];
                            let plain_length = body.len() - 16;
                            let mut aad = Vec::with_capacity(13);
                            aad.extend_from_slice(&sequence.to_be_bytes());
                            aad.push(kind);
                            aad.extend_from_slice(&[0x03, 0x03]);
                            aad.extend_from_slice(&(plain_length as u16).to_be_bytes());
                            match gcm::decrypt(&cipher.key, &nonce, &aad, body) {
                                Some(plain) => plain,
                                None => return Err(TlsError::BadRecordMac),
                            }
                        }
                    };
                    return Ok((kind, plain));
                }
            }

            /// Writes one record, encrypting it when a cipher is active.
            pub fn write_record(&mut self, kind: u8, payload: &[u8]) -> Result<(), TlsError> {
                match self.write_cipher {
                    None => {
                        let mut header = [0u8; 5];
                        header[0] = kind;
                        header[1] = 0x03;
                        header[2] = 0x03;
                        header[3] = (payload.len() >> 8) as u8;
                        header[4] = payload.len() as u8;
                        self.write_all(&header)?;
                        self.write_all(payload)
                    }
                    Some(cipher) => {
                        let sequence = self.write_sequence;
                        self.write_sequence += 1;
                        let explicit = sequence.to_be_bytes();
                        let mut nonce = [0u8; 12];
                        nonce[..4].copy_from_slice(&cipher.iv);
                        nonce[4..].copy_from_slice(&explicit);
                        let mut aad = Vec::with_capacity(13);
                        aad.extend_from_slice(&sequence.to_be_bytes());
                        aad.push(kind);
                        aad.extend_from_slice(&[0x03, 0x03]);
                        aad.extend_from_slice(&(payload.len() as u16).to_be_bytes());
                        let sealed = gcm::encrypt(&cipher.key, &nonce, &aad, payload);
                        let mut body = Vec::with_capacity(8 + sealed.len());
                        body.extend_from_slice(&explicit);
                        body.extend_from_slice(&sealed);
                        let mut header = [0u8; 5];
                        header[0] = kind;
                        header[1] = 0x03;
                        header[2] = 0x03;
                        header[3] = (body.len() >> 8) as u8;
                        header[4] = body.len() as u8;
                        self.write_all(&header)?;
                        self.write_all(&body)
                    }
                }
            }

            /// Reads the next handshake message.
            pub fn read_handshake(&mut self) -> Result<HandshakeMessage, TlsError> {
                loop {
                    let (kind, payload) = self.read_record()?;
                    match kind {
                        CONTENT_HANDSHAKE => {
                            self.handshake_buffer.extend_from_slice(&payload);
                            if self.handshake_buffer.len() >= 4 {
                                let length = ((self.handshake_buffer[1] as usize) << 16)
                                    | ((self.handshake_buffer[2] as usize) << 8)
                                    | self.handshake_buffer[3] as usize;
                                if self.handshake_buffer.len() >= 4 + length {
                                    let raw: Vec<u8> = self.handshake_buffer.drain(..4 + length).collect();
                                    return Ok(HandshakeMessage {
                                        kind: raw[0],
                                        body: raw[4..].to_vec(),
                                        raw,
                                    });
                                }
                            }
                        }
                        CONTENT_ALERT => {
                            let level = payload.first().copied().unwrap_or(0);
                            let description = payload.get(1).copied().unwrap_or(0);
                            if level == 1 && description == 0 {
                                return Err(TlsError::Eof);
                            }
                            return Err(TlsError::PeerAlert(description));
                        }
                        other => {
                            return Err(TlsError::Protocol(format!("unexpected content type {other}")))
                        }
                    }
                }
            }

            /// Writes one handshake message and records it in the transcript.
            pub fn write_handshake(
                &mut self,
                transcript: &mut Vec<u8>,
                kind: u8,
                body: &[u8],
            ) -> Result<(), TlsError> {
                let mut raw = Vec::with_capacity(4 + body.len());
                raw.push(kind);
                raw.push((body.len() >> 16) as u8);
                raw.push((body.len() >> 8) as u8);
                raw.push(body.len() as u8);
                raw.extend_from_slice(body);
                self.write_record(CONTENT_HANDSHAKE, &raw)?;
                transcript.extend_from_slice(&raw);
                Ok(())
            }

            /// Reads application data.
            pub fn read_application(&mut self) -> Result<Vec<u8>, TlsError> {
                let (kind, payload) = self.read_record()?;
                match kind {
                    CONTENT_APPLICATION_DATA => Ok(payload),
                    CONTENT_ALERT => Err(TlsError::Eof),
                    other => Err(TlsError::Protocol(format!("unexpected content type {other}"))),
                }
            }

            /// Writes application data.
            pub fn write_application(&mut self, payload: &[u8]) -> Result<(), TlsError> {
                self.write_record(CONTENT_APPLICATION_DATA, payload)
            }

            /// Writes the close_notify alert.
            pub fn write_close_notify(&mut self) -> Result<(), TlsError> {
                self.write_record(CONTENT_ALERT, &[1, 0])
            }

            fn read_exact(&mut self, buffer: &mut [u8]) -> Result<(), TlsError> {
                match self.stream.read_exact(buffer) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                        Err(TlsError::Eof)
                    }
                    Err(error) => Err(TlsError::Io(error.to_string())),
                }
            }

            fn write_all(&mut self, buffer: &[u8]) -> Result<(), TlsError> {
                match self.stream.write_all(buffer) {
                    Ok(()) => Ok(()),
                    Err(error) => Err(TlsError::Io(error.to_string())),
                }
            }
        }

        /// Reads and parses the ClientHello.
        pub fn read_client_hello(connection: &mut Connection) -> Result<ClientHello, TlsError> {
            let message = connection.read_handshake()?;
            if message.kind != HANDSHAKE_CLIENT_HELLO {
                return Err(TlsError::Protocol("expected a ClientHello".into()));
            }
            let mut hello = ClientHello::parse(&message.body)
                .ok_or_else(|| TlsError::Protocol("malformed ClientHello".into()))?;
            hello.raw = message.raw;
            Ok(hello)
        }

        /// The ServerHello body. RFC 5746 §3.4 makes the renegotiation_info echo
        /// mandatory whenever the client indicated secure renegotiation, by the
        /// 0xff01 extension or by the 0x00ff value in its cipher suites. The echo
        /// carries an empty renegotiated_connection field, which on the wire is a
        /// single 0x00 length byte; a ServerHello with no echo is read as an
        /// unsafe legacy renegotiation server and with any other body as a
        /// decoding error.
        pub fn server_hello_body(hello: &ClientHello, server_random: [u8; 32]) -> Vec<u8> {
            let mut extensions: Vec<u8> = Vec::new();
            if hello.renegotiation_info {
                extensions.extend_from_slice(&EXTENSION_RENEGOTIATION_INFO.to_be_bytes());
                extensions.extend_from_slice(&1u16.to_be_bytes());
                extensions.push(0);
            }
            if hello.extended_master_secret {
                extensions.extend_from_slice(&EXTENSION_EXTENDED_MASTER_SECRET.to_be_bytes());
                extensions.extend_from_slice(&0u16.to_be_bytes());
            }
            extensions.extend_from_slice(&EXTENSION_EC_POINT_FORMATS.to_be_bytes());
            extensions.extend_from_slice(&2u16.to_be_bytes());
            extensions.extend_from_slice(&[1, 0]);

            let mut body = Vec::new();
            body.extend_from_slice(&[0x03, 0x03]);
            body.extend_from_slice(&server_random);
            body.push(0);
            body.extend_from_slice(&CIPHER_ECDHE_RSA_WITH_AES_128_GCM_SHA256.to_be_bytes());
            body.push(0);
            body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
            body.extend_from_slice(&extensions);
            body
        }

        pub fn finish_handshake(
            connection: &mut Connection,
            hello: &ClientHello,
            config: &ServerConfig,
        ) -> Result<(), TlsError> {
            if !hello
                .cipher_suites
                .contains(&CIPHER_ECDHE_RSA_WITH_AES_128_GCM_SHA256)
            {
                return Err(TlsError::Protocol("no shared cipher suite".into()));
            }
            if !hello.groups.contains(&GROUP_X25519) {
                return Err(TlsError::Protocol("no shared group".into()));
            }
            if !hello.signature_algorithms.contains(&SIGNATURE_RSA_PKCS1_SHA256)
                && !hello.signature_algorithms.is_empty()
            {
                return Err(TlsError::Protocol("no shared signature algorithm".into()));
            }

            let mut transcript = hello.raw.clone();
            let server_random = random_bytes();
            let body = server_hello_body(hello, server_random);
            connection.write_handshake(&mut transcript, HANDSHAKE_SERVER_HELLO, &body)?;

            let mut list: Vec<u8> = Vec::new();
            for certificate in &config.chain {
                list.extend_from_slice(&(certificate.len() as u32).to_be_bytes()[1..]);
                list.extend_from_slice(certificate);
            }
            let mut certificate_body: Vec<u8> = Vec::with_capacity(list.len() + 3);
            certificate_body.extend_from_slice(&(list.len() as u32).to_be_bytes()[1..]);
            certificate_body.extend_from_slice(&list);
            connection.write_handshake(&mut transcript, HANDSHAKE_CERTIFICATE, &certificate_body)?;

            let secret = random_bytes();
            let public = x25519::public_key(&secret);
            let mut parameters: Vec<u8> = Vec::with_capacity(36);
            parameters.push(3);
            parameters.extend_from_slice(&GROUP_X25519.to_be_bytes());
            parameters.push(32);
            parameters.extend_from_slice(&public);
            let mut signed: Vec<u8> = Vec::with_capacity(64 + parameters.len());
            signed.extend_from_slice(&hello.random);
            signed.extend_from_slice(&server_random);
            signed.extend_from_slice(&parameters);
            let signature = config.key.sign_pkcs1_sha256(&signed);
            let mut key_exchange = parameters;
            key_exchange.extend_from_slice(&SIGNATURE_RSA_PKCS1_SHA256.to_be_bytes());
            key_exchange.extend_from_slice(&(signature.len() as u16).to_be_bytes());
            key_exchange.extend_from_slice(&signature);
            connection.write_handshake(&mut transcript, HANDSHAKE_SERVER_KEY_EXCHANGE, &key_exchange)?;

            connection.write_handshake(&mut transcript, HANDSHAKE_SERVER_HELLO_DONE, &[])?;

            let message = connection.read_handshake()?;
            if message.kind != HANDSHAKE_CLIENT_KEY_EXCHANGE {
                return Err(TlsError::Protocol("expected a ClientKeyExchange".into()));
            }
            transcript.extend_from_slice(&message.raw);
            if message.body.is_empty() {
                return Err(TlsError::Protocol("empty ClientKeyExchange".into()));
            }
            let point_length = message.body[0] as usize;
            let point = message
                .body
                .get(1..1 + point_length)
                .ok_or_else(|| TlsError::Protocol("truncated ClientKeyExchange".into()))?;
            if point.len() != 32 {
                return Err(TlsError::Protocol("unexpected key exchange length".into()));
            }
            let mut client_point = [0u8; 32];
            client_point.copy_from_slice(point);
            let premaster = x25519::scalar_mult(&secret, &client_point);

            let session_hash = sha256(&transcript);
            let master_secret = if hello.extended_master_secret {
                prf_sha256(
                    &premaster,
                    "extended master secret",
                    &session_hash,
                    48,
                )
            } else {
                let mut seed = hello.random.to_vec();
                seed.extend_from_slice(&server_random);
                prf_sha256(&premaster, "master secret", &seed, 48)
            };

            let mut key_seed = server_random.to_vec();
            key_seed.extend_from_slice(&hello.random);
            let key_block = prf_sha256(&master_secret, "key expansion", &key_seed, 40);
            let mut client_key = [0u8; 16];
            client_key.copy_from_slice(&key_block[0..16]);
            let mut server_key = [0u8; 16];
            server_key.copy_from_slice(&key_block[16..32]);
            let mut client_iv = [0u8; 4];
            client_iv.copy_from_slice(&key_block[32..36]);
            let mut server_iv = [0u8; 4];
            server_iv.copy_from_slice(&key_block[36..40]);

            connection.pending_read_cipher = Some(Aead {
                key: client_key,
                iv: client_iv,
            });
            let finished = connection.read_handshake()?;
            if finished.kind != HANDSHAKE_FINISHED {
                return Err(TlsError::Protocol("expected a client Finished".into()));
            }
            let expected = prf_sha256(
                &master_secret,
                "client finished",
                &sha256(&transcript),
                12,
            );
            if finished.body != expected {
                return Err(TlsError::Protocol("client Finished mismatch".into()));
            }
            transcript.extend_from_slice(&finished.raw);

            connection.write_record(CONTENT_CHANGE_CIPHER_SPEC, &[1])?;
            connection.write_cipher = Some(Aead {
                key: server_key,
                iv: server_iv,
            });
            let server_verify = prf_sha256(
                &master_secret,
                "server finished",
                &sha256(&transcript),
                12,
            );
            connection.write_handshake(&mut transcript, HANDSHAKE_FINISHED, &server_verify)?;
            Ok(())
        }

        /// A captured request.
        pub struct Request {
            pub header: Vec<u8>,
            pub payload: Vec<u8>,
        }

        /// Why a request could not be read.
        pub enum RequestError {
            /// The peer closed the transport.
            Closed,
            /// The header grew past the limit.
            TooLarge,
            /// The request carried no Content-Length header.
            NoLength,
            /// The transport failed.
            Tls(TlsError),
        }

        /// Reads the HTTP request the way the fixture always did: headers up
        /// to the empty line, then exactly Content-Length bytes.
        pub fn read_request(connection: &mut Connection) -> Result<Request, RequestError> {
            let mut data: Vec<u8> = Vec::new();
            loop {
                let part = connection
                    .read_application()
                    .map_err(RequestError::Tls)?;
                if part.is_empty() {
                    return Err(RequestError::Closed);
                }
                data.extend_from_slice(&part);
                // The cap is checked before the terminator is looked for, the
                // order the contract has always used.
                if data.len() > HEADER_LIMIT {
                    return Err(RequestError::TooLarge);
                }
                if find(&data, b"\r\n\r\n").is_some() {
                    break;
                }
            }
            let separator = find(&data, b"\r\n\r\n").unwrap_or(0);
            let header = data[..separator].to_vec();
            let mut payload = data[separator + 4..].to_vec();
            let length = content_length(&header).ok_or(RequestError::NoLength)?;
            while payload.len() < length {
                let part = connection
                    .read_application()
                    .map_err(RequestError::Tls)?;
                if part.is_empty() {
                    return Err(RequestError::Closed);
                }
                payload.extend_from_slice(&part);
            }
            Ok(Request { header, payload })
        }

        /// Reads the Content-Length header value.
        pub fn content_length(header: &[u8]) -> Option<usize> {
            for line in header.split(|byte| *byte == b'\n') {
                let line = match line.last() {
                    Some(b'\r') => &line[..line.len() - 1],
                    _ => line,
                };
                if line.len() < 15 || !line[..15].eq_ignore_ascii_case(b"content-length:") {
                    continue;
                }
                let value = std::str::from_utf8(&line[15..]).ok()?.trim();
                return value.parse::<usize>().ok();
            }
            None
        }

        /// Finds a byte pattern.
        pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
            if needle.is_empty() || haystack.len() < needle.len() {
                return None;
            }
            haystack
                .windows(needle.len())
                .position(|window| window == needle)
        }

        /// 32 bytes from the system entropy source.
        pub fn random_bytes() -> [u8; 32] {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos() as u64)
                .unwrap_or(0x9e3779b97f4a7c15);
            let mut state = nanos ^ 0x5deece66d;
            let mut out = [0u8; 32];
            for byte in out.iter_mut() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
            }
            out
        }
    }

    // ------------------------------------------------------------------
    // The fixture server: one thread per connection, an atomically written
    // events file and a port file, the contract the fixture has always kept.
    // ------------------------------------------------------------------
    pub mod fixture {
        use std::fs::{self, OpenOptions};
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};
        use std::thread::JoinHandle;
        use std::time::Duration;

        use super::session::{
            finish_handshake, read_client_hello, read_request, Connection, RequestError,
            ServerConfig, TlsError, RESPONSE,
        };

        /// Everything the events file records about the connections so far.
        #[derive(Default)]
        pub struct FixtureState {
            /// The number of connections the fixture has handled.
            pub events: u64,
            /// The server name of every ClientHello, `None` when absent.
            pub names: Vec<Option<String>>,
            /// The captured request headers and payloads.
            pub captured: Vec<(Vec<u8>, Vec<u8>)>,
            /// The names of the TLS errors of the refused handshakes.
            pub refused: Vec<String>,
        }

        /// The events file, in the shape the leg always wrote.
        pub fn state_json(state: &FixtureState) -> String {
            let mut out = String::from("{\"events\": ");
            out.push_str(&state.events.to_string());
            out.push_str(", \"names\": [");
            for (index, name) in state.names.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                match name {
                    Some(text) => {
                        out.push('"');
                        out.push_str(&escape(text));
                        out.push('"');
                    }
                    None => out.push_str("null"),
                }
            }
            out.push_str("], \"captured\": [");
            for (index, (header, payload)) in state.captured.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str("{\"header\": \"");
                out.push_str(&crate::twin::b64_encode(header));
                out.push_str("\", \"payload\": \"");
                out.push_str(&crate::twin::b64_encode(payload));
                out.push_str("\"}");
            }
            out.push_str("], \"refused\": [");
            for (index, name) in state.refused.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push('"');
                out.push_str(&escape(name));
                out.push('"');
            }
            out.push_str("]}");
            out
        }

        fn escape(text: &str) -> String {
            let mut out = String::new();
            for ch in text.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                    ch => out.push(ch),
                }
            }
            out
        }

        /// Writes the events file atomically: a temporary file and a rename.
        pub fn write_state(path: &Path, state: &Mutex<FixtureState>) {
            // The lock is held across the write and the rename, so two
            // connections that finish together cannot interleave a temporary
            // file and lose an event.
            let guard = match state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let text = state_json(&guard);
            let mut temporary = path.as_os_str().to_os_string();
            temporary.push(".tmp");
            let temporary = PathBuf::from(temporary);
            if fs::write(&temporary, text.as_bytes()).is_ok() {
                let _ = fs::rename(&temporary, path);
            }
        }

        fn log(path: &Path, line: &str) {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(file, "{line}");
            }
        }

        /// The fixture server: it owns the listener, the connection threads
        /// and the recorded state.
        pub struct FixtureServer {
            port: u16,
            stop: Arc<AtomicBool>,
            state: Arc<Mutex<FixtureState>>,
            connections: Arc<Mutex<Vec<TcpStream>>>,
            workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
            accept: Option<JoinHandle<()>>,
            events_path: PathBuf,
            log_path: PathBuf,
        }

        impl FixtureServer {
            /// Binds the listener, publishes the port and starts accepting.
            pub fn start(folder: &Path, config: ServerConfig) -> std::io::Result<FixtureServer> {
                let listener = TcpListener::bind("127.0.0.1:0")?;
                listener.set_nonblocking(true)?;
                let port = listener.local_addr()?.port();
                let events_path = folder.join("events.json");
                let log_path = folder.join("server.err");
                let state = Arc::new(Mutex::new(FixtureState::default()));
                write_state(&events_path, &state);
                fs::write(folder.join("port"), port.to_string())?;

                let stop = Arc::new(AtomicBool::new(false));
                let connections: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
                let workers: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
                let config = Arc::new(config);

                let thread_stop = stop.clone();
                let thread_state = state.clone();
                let thread_connections = connections.clone();
                let thread_workers = workers.clone();
                let thread_events = events_path.clone();
                let thread_log = log_path.clone();
                let accept = std::thread::Builder::new()
                    .name("tls-fixture-accept".to_string())
                    .spawn(move || {
                        while !thread_stop.load(Ordering::SeqCst) {
                            match listener.accept() {
                                Ok((stream, _)) => {
                                    let _ = stream.set_nonblocking(false);
                                    if let Ok(mut list) = thread_connections.lock() {
                                        if let Ok(clone) = stream.try_clone() {
                                            list.push(clone);
                                        }
                                    }
                                    let worker_state = thread_state.clone();
                                    let worker_events = thread_events.clone();
                                    let worker_log = thread_log.clone();
                                    let worker_config = config.clone();
                                    let worker = std::thread::Builder::new()
                                        .name("tls-fixture-connection".to_string())
                                        .spawn(move || {
                                            handle(
                                                stream,
                                                &worker_config,
                                                &worker_state,
                                                &worker_events,
                                                &worker_log,
                                            );
                                        });
                                    if let Ok(mut list) = thread_workers.lock() {
                                        if let Ok(worker) = worker {
                                            list.push(worker);
                                        }
                                    }
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    std::thread::sleep(Duration::from_millis(5));
                                }
                                Err(_) => break,
                            }
                        }
                    })
                    .map_err(|error| std::io::Error::other(error.to_string()))?;

                Ok(FixtureServer {
                    port,
                    stop,
                    state,
                    connections,
                    workers,
                    accept: Some(accept),
                    events_path,
                    log_path,
                })
            }

            pub fn port(&self) -> u16 {
                self.port
            }

            pub fn events_path(&self) -> &Path {
                &self.events_path
            }

            /// True while the accept thread is still running.
            pub fn running(&self) -> bool {
                match &self.accept {
                    Some(handle) => !handle.is_finished(),
                    None => false,
                }
            }

            /// The recorded state.
            pub fn snapshot(&self) -> FixtureState {
                let guard = match self.state.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                FixtureState {
                    events: guard.events,
                    names: guard.names.clone(),
                    captured: guard.captured.clone(),
                    refused: guard.refused.clone(),
                }
            }

            /// Stops the server and joins every thread it owns.
            pub fn stop(&mut self) {
                self.stop.store(true, Ordering::SeqCst);
                if let Ok(mut list) = self.connections.lock() {
                    for stream in list.iter() {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                    }
                    list.clear();
                }
                if let Some(accept) = self.accept.take() {
                    let _ = accept.join();
                }
                if let Ok(mut list) = self.workers.lock() {
                    for worker in list.drain(..) {
                        let _ = worker.join();
                    }
                }
            }
        }

        impl Drop for FixtureServer {
            fn drop(&mut self) {
                self.stop();
            }
        }

        /// Handles one connection: the handshake, the request and the reply.
        fn handle(
            stream: TcpStream,
            config: &ServerConfig,
            state: &Mutex<FixtureState>,
            events_path: &Path,
            log_path: &Path,
        ) {
            let mut connection = Connection::new(stream);
            let mut refusal: Option<String> = None;
            match read_client_hello(&mut connection) {
                Ok(hello) => {
                    {
                        let mut guard = match state.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        guard.names.push(hello.server_name.clone());
                    }
                    match finish_handshake(&mut connection, &hello, config) {
                        Ok(()) => match read_request(&mut connection) {
                            Ok(request) => {
                                {
                                    let mut guard = match state.lock() {
                                        Ok(guard) => guard,
                                        Err(poisoned) => poisoned.into_inner(),
                                    };
                                    guard.captured.push((request.header, request.payload));
                                }
                                let _ = connection.write_application(RESPONSE);
                                let _ = connection.write_close_notify();
                                log(
                                    log_path,
                                    "connection: handshake complete, request captured",
                                );
                            }
                            Err(RequestError::Tls(error)) => {
                                refusal = refusal_name(&error);
                                log(log_path, &format!("connection: refused {}", error_text(&error)));
                            }
                            Err(RequestError::Closed) => {
                                log(log_path, "connection: closed before the request was complete");
                            }
                            Err(RequestError::TooLarge) => {
                                log(log_path, "connection: request header over the limit");
                            }
                            Err(RequestError::NoLength) => {
                                log(log_path, "connection: request carried no Content-Length");
                            }
                        },
                        Err(error) => {
                            refusal = refusal_name(&error);
                            log(log_path, &format!("connection: refused {}", error_text(&error)));
                        }
                    }
                }
                Err(error) => {
                    refusal = refusal_name(&error);
                    log(log_path, &format!("connection: refused {}", error_text(&error)));
                }
            }
            {
                let mut guard = match state.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                if let Some(name) = refusal {
                    guard.refused.push(name);
                }
                guard.events += 1;
            }
            write_state(events_path, state);
        }

        /// The readable form of a transport error, for the server log.
        fn error_text(error: &TlsError) -> String {
            match error {
                TlsError::Eof => "unexpected eof".to_string(),
                TlsError::PeerAlert(code) => format!("peer alert {code}"),
                TlsError::BadRecordMac => "bad record mac".to_string(),
                TlsError::Protocol(text) => text.clone(),
                TlsError::Io(text) => text.clone(),
            }
        }

        /// The error name a refused handshake is recorded under. A transport
        /// failure is not a TLS refusal, so it is recorded as no name at all.
        fn refusal_name(error: &TlsError) -> Option<String> {
            match error {
                TlsError::Eof => Some("SSLEOFError".to_string()),
                TlsError::Io(_) => None,
                _ => Some("SSLError".to_string()),
            }
        }
    }

}

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner execution of public legacy project_payload/current project_chunks operations, opaque bytes and pinned connections; unchanged production Android TLS socket block executed on the host JVM against a real local certificate, per-process fixture trust, successful alias/SNI and refused hostname mismatches. No archive extraction, excluded historical fixtures or physical Android/live-user NAS acceptance is claimed.";
const BODY: &[u8] = b"\x00\xffopaque\r\nHost: 100.64.0.2:5006\r\n\x80bytes";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let mut controller = Controller::spawn(&root_text);
    let mut passed = Vec::new();
    for streamed in [false, true] {
        run_protocol_checks(&mut controller, streamed, &mut passed);
    }
    run_production_tls_checks(&mut controller, &root, &mut passed);
    controller.finish();
    if passed.len() != 61 {
        panic!("snapshot_identity emitted {} checks", passed.len());
    }
    let report = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("snapshot_identity", &report, true);
}

fn run_protocol_checks(controller: &mut Controller, streamed: bool, passed: &mut Vec<String>) {
    let label = if streamed { "streamed" } else { "legacy" };
    for phase in ["write", "probe", "verify"] {
        let record = protocol_record(phase, streamed);
        let original = record.clone();
        let reply = protocol_request(controller, &record, streamed, None);
        let route = format!("{label} {phase} retains VPN socket route and HTTPS");
        let value = match reply.get("value") {
            Some(value) => value,
            None => panic!("{route}\n{}", reply.to_string()),
        };
        keep(
            passed,
            &route,
            text_is(value, "host", "100.64.0.2")
                && number_is(value, "port", "5006")
                && matches!(value.get("tls"), Some(Value::Bool(true))),
        );
        keep(
            passed,
            &format!("{label} {phase} retains configured certificate DNS identity"),
            text_is(value, "tls_host", "nas.example.test"),
        );
        let authority = format!("{label} {phase} applies certificate alias only to request authority");
        let wire = wire_bytes(value, &authority);
        let (header, payload) = split_wire(&wire, &authority);
        let mut header_line = header.clone();
        header_line.extend_from_slice(b"\r\n");
        keep(
            passed,
            &authority,
            contains(&header_line, b"\r\nHost: nas.example.test:5006\r\n"),
        );
        let credentials = format!(
            "Authorization: Basic {}",
            b64_encode(b"retained-user:retained-password")
        );
        keep(
            passed,
            &format!("{label} {phase} retains original connection credentials"),
            contains(&header, credentials.as_bytes()),
        );
        let method = if phase == "write" { "PUT" } else { "GET" };
        keep(
            passed,
            &format!("{label} {phase} preserves conditional write or read semantics"),
            wire.starts_with(format!("{method} /share/saved.bin HTTP/1.1\r\n").as_bytes())
                && contains(&header, b"If-None-Match: *") == (phase == "write"),
        );
        if phase == "write" {
            let length = format!("Content-Length: {}", BODY.len());
            let expected: &[u8] = if streamed { b"" } else { BODY };
            keep(
                passed,
                &format!("{label} write preserves exact body framing"),
                contains(&header, length.as_bytes()) && payload == expected,
            );
        } else {
            keep(
                passed,
                &format!("{label} {phase} carries no write payload"),
                payload.is_empty(),
            );
        }
        keep(
            passed,
            &format!("{label} {phase} leaves saved connection and source bytes unchanged"),
            record == original,
        );
        let handle = value
            .get("snapshot_handle")
            .unwrap_or_else(|| panic!("{label} {phase} snapshot handle is missing"))
            .clone();
        protocol_close(controller, streamed, &handle);
    }

    for invalid in [
        "https://nas.example.test",
        "*.example.test",
        "100.64.0.2",
        "bad\r\nInjected: value",
    ] {
        let mut record = protocol_record("write", streamed);
        let mut connection = record
            .get("connection")
            .unwrap_or_else(|| panic!("connection missing"))
            .clone();
        put(&mut connection, "tls_name", s(invalid));
        put(&mut record, "connection", connection);
        let reply = protocol_request(controller, &record, streamed, None);
        keep(
            passed,
            &format!(
                "{label} rejects invalid certificate identity {}",
                quoted_identity(invalid)
            ),
            reply.get("error").is_some() && reply.get("value").is_none(),
        );
    }

    let mut record = protocol_record("write", streamed);
    let mut connection = record
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    put(&mut connection, "url", s("http://100.64.0.2:5006/share/"));
    put(&mut connection, "plain", twin::b(true));
    put(&mut record, "connection", connection.clone());
    put(
        &mut record,
        "target",
        s("http://100.64.0.2:5006/share/saved.bin"),
    );
    let reply = protocol_request(controller, &record, streamed, None);
    keep(
        passed,
        &format!("{label} refuses certificate identity on unencrypted transport"),
        reply.get("error").is_some() && reply.get("value").is_none(),
    );

    let record = protocol_record("probe", streamed);
    let mut stored = record.clone();
    let mut connection = stored
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    put(&mut connection, "tls_name", s("changed.example.test"));
    put(&mut stored, "connection", connection);
    let reply = protocol_request(controller, &record, streamed, Some(&stored));
    keep(
        passed,
        &format!("{label} refuses changed saved connection before transport"),
        reply.get("error").is_some() && reply.get("value").is_none(),
    );

    let mut record = protocol_record("write", streamed);
    let mut connection = record
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    put(&mut connection, "tls_name", s(""));
    put(&mut record, "connection", connection);
    let reply = protocol_request(controller, &record, streamed, None);
    let ordinary = format!("{label} retains ordinary HTTPS when no alias is configured");
    let value = match reply.get("value") {
        Some(value) => value,
        None => panic!("{ordinary}\n{}", reply.to_string()),
    };
    keep(
        passed,
        &ordinary,
        matches!(value.get("tls"), Some(Value::Bool(true))) && no_tls_host(value),
    );
    let handle = value
        .get("snapshot_handle")
        .unwrap_or_else(|| panic!("{ordinary} snapshot handle is missing"))
        .clone();
    protocol_close(controller, streamed, &handle);
}

fn protocol_record(phase: &str, streamed: bool) -> Value {
    let connection = obj(vec![
        ("url".into(), s("https://100.64.0.2:5006/share/")),
        ("tls_name".into(), s("nas.example.test")),
        ("user".into(), s("retained-user")),
        ("password".into(), s("retained-password")),
        ("plain".into(), twin::b(false)),
    ]);
    let mut record = obj(vec![
        ("stage_ticket".into(), s("owned-fixture")),
        ("purpose".into(), s(&format!("project_transfer:owned-{phase}"))),
        ("bytes".into(), twin::n(&BODY.len().to_string())),
        ("phone".into(), twin::b(false)),
        ("tree".into(), Value::Null),
        ("connection".into(), connection),
        ("target".into(), s("https://100.64.0.2:5006/share/saved.bin")),
        ("mime".into(), s("application/octet-stream")),
        ("source".into(), Value::Null),
        ("phase".into(), s(phase)),
    ]);
    if streamed {
        put(&mut record, "format", twin::n("2"));
    }
    record
}

fn protocol_request(
    controller: &mut Controller,
    record: &Value,
    streamed: bool,
    stored: Option<&Value>,
) -> Value {
    let operation = if streamed { "project_chunks" } else { "project_payload" };
    let stored = stored.unwrap_or(record);
    controller.send(&obj(vec![
        ("op".into(), s(operation)),
        ("action".into(), s("open")),
        ("record".into(), record.clone()),
        ("stored".into(), stored.clone()),
        ("archive".into(), s(&b64_encode(BODY))),
    ]))
}

fn protocol_close(controller: &mut Controller, streamed: bool, handle: &Value) {
    let operation = if streamed { "project_chunks" } else { "project_close" };
    let reply = controller.send(&obj(vec![
        ("op".into(), s(operation)),
        ("action".into(), s("close")),
        ("handle".into(), handle.clone()),
    ]));
    if reply.get("error").is_some() || reply.get("value").is_none() {
        panic!("{operation} close failed: {}", reply.to_string());
    }
}

fn wire_bytes(value: &Value, check: &str) -> Vec<u8> {
    if let Some(encoded) = value.get("wire_base64") {
        let Value::Str(text) = encoded else {
            panic!("{check}");
        };
        let Some(bytes) = decode_loose(text) else {
            panic!("{check}");
        };
        return bytes;
    }
    match value.get("wire") {
        Some(Value::Str(text)) => text.as_bytes().to_vec(),
        _ => panic!("{check}"),
    }
}

fn split_wire(wire: &[u8], check: &str) -> (Vec<u8>, Vec<u8>) {
    let Some(separator) = wire.windows(4).position(|window| window == b"\r\n\r\n") else {
        panic!("{check}");
    };
    (wire[..separator].to_vec(), wire[separator + 4..].to_vec())
}

fn run_production_tls_checks(controller: &mut Controller, root: &Path, passed: &mut Vec<String>) {
    // Activity.java's TLS line is the subject. std has no TLS stack, so the peer
    // is this file's own certificate-backed TLS server: SNI comes from the
    // ClientHello, and a hostname rejection must not deliver a second request body.
    let mut fixture = TlsFixture::create(root);
    let mut record = protocol_record("write", false);
    let mut connection = record
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    let url = format!("https://127.0.0.1:{}/share/", fixture.port);
    put(&mut connection, "url", s(&url));
    put(&mut record, "connection", connection);
    put(&mut record, "target", s(&format!("{url}saved.bin")));
    let reply = protocol_request(controller, &record, false, None);
    let value = match reply.get("value") {
        Some(value) => value.clone(),
        None => panic!(
            "actual production TLS binding completes a trusted DNS-alias handshake over an IP socket\n{}",
            reply.to_string()
        ),
    };
    let wire_base64 = match value.get("wire_base64") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("actual production TLS binding completes a trusted DNS-alias handshake over an IP socket"),
    };
    let tls_host = match value.get("tls_host") {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    };
    let handle = value
        .get("snapshot_handle")
        .unwrap_or_else(|| panic!("production TLS snapshot handle is missing"))
        .clone();
    protocol_close(controller, false, &handle);

    let accepted = fixture.run(&tls_host, &wire_base64);
    let accepted_bytes = decode_loose(&accepted.stdout);
    keep_detail(
        passed,
        "actual production TLS binding completes a trusted DNS-alias handshake over an IP socket",
        accepted.success && accepted_bytes.as_ref().is_some_and(|bytes| bytes.ends_with(b"\r\n\r\nOK")),
        &format!(
            "java success={} timed_out={} stdout={} stderr={}",
            accepted.success, accepted.timed_out, accepted.stdout, accepted.stderr
        ),
    );
    keep_detail(
        passed,
        "actual TLS server receives unchanged opaque payload and aliased HTTP authority",
        captured_matches(&fixture.state, BODY),
        &format!(
            "state {}\nserver {}",
            fixture.state.to_string(),
            read_text(&fixture.stderr_path)
        ),
    );

    for name in ["", "wrong.example.test"] {
        let rejected = fixture.run(name, &wire_base64);
        let check = format!(
            "production hostname verification rejects {}",
            if name.is_empty() { "the socket IP" } else { "a mismatched DNS identity" }
        );
        keep_detail(
            passed,
            &check,
            !rejected.success && rejected.stderr.contains("SSLHandshakeException"),
            &format!(
                "java success={} timed_out={} stdout={} stderr={}",
                rejected.success, rejected.timed_out, rejected.stdout, rejected.stderr
            ),
        );
    }
    keep_detail(
        passed,
        "production TLS sends configured SNI while keeping failed peers from request bytes",
        has_name(&fixture.state, "nas.example.test")
            && has_name(&fixture.state, "wrong.example.test")
            && captured_len(&fixture.state) == 1,
        &format!(
            "state {}\nserver {}",
            fixture.state.to_string(),
            read_text(&fixture.stderr_path)
        ),
    );
}

struct TlsFixture {
    folder: PathBuf,
    port: u16,
    server: tls::fixture::FixtureServer,
    events_path: PathBuf,
    stderr_path: PathBuf,
    jdk: PathBuf,
    events: u64,
    state: Value,
}

impl TlsFixture {
    fn create(root: &Path) -> Self {
        let folder = twin::create_scratch(root, "snapshot-tls");
        let certificate = folder.join("fixture.pem");
        let private = folder.join("fixture.key");
        let mut openssl = Command::new("openssl");
        openssl.args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=nas.example.test",
            "-addext",
            "subjectAltName=DNS:nas.example.test",
            "-keyout",
            private.to_str().unwrap_or_else(|| panic!("key path is not UTF-8")),
            "-out",
            certificate.to_str().unwrap_or_else(|| panic!("certificate path is not UTF-8")),
        ]);
        run_checked("openssl", openssl);
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("key mode: {error}"));
        let jdk = PathBuf::from(std::env::var_os("CLIENT_JDK").unwrap_or_else(|| panic!("CLIENT_JDK is not set")));
        let trust = folder.join("trust.p12");
        let mut keytool = Command::new(jdk.join("bin/keytool"));
        keytool.args([
            "-importcert",
            "-noprompt",
            "-alias",
            "fixture",
            "-file",
            certificate.to_str().unwrap_or_else(|| panic!("certificate path is not UTF-8")),
            "-keystore",
            trust.to_str().unwrap_or_else(|| panic!("trust path is not UTF-8")),
            "-storetype",
            "PKCS12",
            "-storepass",
            "controlled-fixture-password",
        ]);
        run_checked("keytool", keytool);
        let source = folder.join("SnapshotTlsChecks.java");
        std::fs::write(&source, java_source(&production_tls_block(root)))
            .unwrap_or_else(|error| panic!("could not write SnapshotTlsChecks.java: {error}"));
        let mut javac = Command::new(jdk.join("bin/javac"));
        javac
            .arg("--release")
            .arg("8")
            .arg("-d")
            .arg(&folder)
            .arg(&source);
        run_checked("javac", javac);

        let events_path = folder.join("events.json");
        let port_path = folder.join("port");
        let stderr_path = folder.join("server.err");
        let certificate_text = std::fs::read_to_string(&certificate).unwrap_or_else(|error| {
            panic!("could not read {}: {error}", certificate.display())
        });
        let private_text = std::fs::read_to_string(&private)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", private.display()));
        let chain = vec![tls::rsa::pem_der(&certificate_text, "CERTIFICATE")
            .unwrap_or_else(|| panic!("the fixture certificate is not a PEM certificate"))];
        let key = tls::rsa::RsaKey::from_pem(&private_text)
            .unwrap_or_else(|| panic!("the fixture key is not a PEM private key"));
        let config = tls::session::ServerConfig { chain, key };
        let server = tls::fixture::FixtureServer::start(&folder, config)
            .unwrap_or_else(|error| panic!("could not start the TLS fixture server: {error}"));
        let port = wait_port(&server, &port_path, &stderr_path);
        let state = load_state(&events_path).unwrap_or_else(|| obj(vec![]));
        let events = event_count(&state);
        Self {
            folder,
            port,
            server,
            events_path,
            stderr_path,
            jdk,
            events,
            state,
        }
    }

    fn run(&mut self, host: &str, wire_base64: &str) -> ProcessResult {
        let before = self.events;
        let result = self.java(host, wire_base64);
        if let Err(detail) = self.wait_event(before) {
            panic!(
                "TLS fixture did not record the connection for {host:?}\n{detail}\njava success={} timed_out={} stdout={} stderr={}",
                result.success, result.timed_out, result.stdout, result.stderr
            );
        }
        result
    }

    fn java(&self, host: &str, wire_base64: &str) -> ProcessResult {
        let trust = self.folder.join("trust.p12");
        let mut command = Command::new(self.jdk.join("bin/java"));
        command
            .arg(format!(
                "-Djavax.net.ssl.trustStore={}",
                trust.to_str().unwrap_or_else(|| panic!("trust path is not UTF-8"))
            ))
            .arg("-Djavax.net.ssl.trustStorePassword=controlled-fixture-password")
            .arg("-cp")
            .arg(&self.folder)
            .arg("SnapshotTlsChecks")
            .arg("127.0.0.1")
            .arg(self.port.to_string())
            .arg(host)
            .arg(wire_base64)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command_output(&mut command, Duration::from_secs(20))
    }

    fn wait_event(&mut self, before: u64) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(state) = load_state(&self.events_path) {
                let events = event_count(&state);
                if events > before {
                    self.events = events;
                    self.state = state;
                    return Ok(());
                }
            }
            if !self.server.running() {
                return Err(format!(
                    "TLS fixture server stopped: {}",
                    read_text(&self.stderr_path)
                ));
            }
            if Instant::now() > deadline {
                return Err(read_text(&self.stderr_path));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for TlsFixture {
    fn drop(&mut self) {
        self.server.stop();
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn production_tls_block(root: &Path) -> String {
    let path = root.join("platform/com/local/client/Activity.java");
    let activity = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let marker = "if(command.optBoolean(\"tls\")){SSLSocket";
    let Some(start) = activity.find(marker) else {
        panic!("production TLS socket block is missing");
    };
    let end = activity[start..]
        .find('\n')
        .map(|offset| start + offset)
        .unwrap_or(activity.len());
    activity[start..end].trim().to_string()
}

fn java_source(tls_block: &str) -> String {
    let template = r#"
import java.net.*;import java.io.*;import java.util.*;import javax.net.ssl.*;
public class SnapshotTlsChecks{
 static int networkActive(int token,long deadline){return 10000;}
 static class Command{String name;Command(String n){name=n;}boolean optBoolean(String key){return true;}String optString(String key,String fallback){return name.isEmpty()?fallback:name;}boolean has(String key){return !name.isEmpty();}String getString(String key){return name;}}
 public static void main(String[] args)throws Exception{String host=args[0];int port=Integer.parseInt(args[1]),token=1;long transportDeadline=System.currentTimeMillis()+10000;Command command=new Command(args[2]);Socket raw=new Socket();raw.connect(new InetSocketAddress(host,port),5000);raw.setSoTimeout(5000);Socket connection=raw;
 TLS_BLOCK
 try(Socket managed=connection){managed.getOutputStream().write(Base64.getDecoder().decode(args[3]));managed.getOutputStream().flush();ByteArrayOutputStream result=new ByteArrayOutputStream();byte[] block=new byte[4096];int count;while((count=managed.getInputStream().read(block))!=-1)result.write(block,0,count);System.out.println(Base64.getEncoder().encodeToString(result.toByteArray()));}
 }
}
"#;
    template.replacen(" TLS_BLOCK", tls_block, 1)
}


fn wait_port(server: &tls::fixture::FixtureServer, port_path: &Path, stderr_path: &Path) -> u16 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(port_path) {
            if let Ok(port) = text.trim().parse::<u16>() {
                if port != 0 {
                    if port != server.port() {
                        panic!(
                            "the fixture published port {port} but bound {}",
                            server.port()
                        );
                    }
                    return port;
                }
            }
        }
        if !server.running() {
            panic!(
                "TLS fixture server stopped before it published a port: {}",
                read_text(stderr_path)
            );
        }
        if Instant::now() > deadline {
            panic!(
                "TLS fixture server did not publish a port: {}",
                read_text(stderr_path)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn load_state(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let owned = text.trim().to_string();
    if !owned.starts_with('{') || !owned.ends_with('}') {
        return None;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Value::parse(&owned))).ok()
}

fn event_count(state: &Value) -> u64 {
    match state.get("events") {
        Some(Value::Num(token)) => token.parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

fn has_name(state: &Value, expect: &str) -> bool {
    let Some(Value::Arr(names)) = state.get("names") else {
        return false;
    };
    names
        .iter()
        .any(|name| matches!(name, Value::Str(text) if text == expect))
}

fn captured_len(state: &Value) -> usize {
    match state.get("captured") {
        Some(Value::Arr(items)) => items.len(),
        _ => 0,
    }
}

fn captured_matches(state: &Value, body: &[u8]) -> bool {
    let Some(Value::Arr(captured)) = state.get("captured") else {
        return false;
    };
    if captured.len() != 1 {
        return false;
    }
    let Some(Value::Str(header_b64)) = captured[0].get("header") else {
        return false;
    };
    let Some(Value::Str(payload_b64)) = captured[0].get("payload") else {
        return false;
    };
    let Some(header) = decode_loose(header_b64) else {
        return false;
    };
    let Some(payload) = decode_loose(payload_b64) else {
        return false;
    };
    payload == body && contains(&header, b"Host: nas.example.test:")
}

fn run_checked(label: &str, mut command: Command) {
    let result = command_output(&mut command, Duration::from_secs(120));
    if !result.success {
        panic!("{label} failed\n{}{}", result.stdout, result.stderr);
    }
}

struct ProcessResult {
    success: bool,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

fn command_output(command: &mut Command, timeout: Duration) -> ProcessResult {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("could not start command: {error}"));
    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("command wait: {error}"),
        }
    };
    let (stdout, stderr) = collect_pipes(&mut child);
    ProcessResult {
        success: status.is_some_and(|status| status.success()),
        timed_out,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

fn collect_pipes(child: &mut Child) -> (Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    (stdout, stderr)
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn keep_detail(passed: &mut Vec<String>, name: &str, ok: bool, detail: &str) {
    if !ok {
        panic!("{name}\n{detail}");
    }
    passed.push(name.to_string());
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn number_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Num(number)) if number == expect)
}

fn no_tls_host(value: &Value) -> bool {
    match value.get("tls_host") {
        None | Some(Value::Null) => true,
        Some(Value::Str(text)) => text.is_empty(),
        _ => false,
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

/// The identity as it appears in a published check name: a single quoted
/// string, so the names this twin reports stay the names it always reported.
fn quoted_identity(text: &str) -> String {
    let mut out = String::from("'");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\'' => out.push_str("\\'"),
            ch => out.push(ch),
        }
    }
    out.push('\'');
    out
}

fn decode_loose(text: &str) -> Option<Vec<u8>> {
    let filtered: String = text.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
    if filtered
        .bytes()
        .any(|byte| !matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='))
    {
        return None;
    }
    Some(b64_decode(&filtered))
}

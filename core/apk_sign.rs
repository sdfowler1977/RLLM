//! Package signatures: archive manifest, and signature schemes v2 and v3.
//!
//! Research: RFC 8017 (PKCS #1), RFC 5280 (certificate), RFC 5652 (signed-data),
//! FIPS 180-4, and the published v2/v3 signing-block layout (little-endian
//! length-prefixed pairs, chunked content digest, algorithm 0x0101).

use super::*;
use std::cmp::Ordering;

const V2_ID: u32 = 0x7109_871a;
const V3_ID: u32 = 0xf053_68c0;
const ALG: u32 = 0x0101;
const CHUNK: usize = 1024 * 1024;
const MIN_SDK: u32 = 28;
const MAX_SDK: u32 = 0x7fff_ffff;

pub(crate) struct Report {
    pub v1: bool,
    pub v2: bool,
    pub v3: bool,
    pub detail: String,
}

pub(crate) fn sign(apk: &[u8], bits: usize) -> Result<Vec<u8>> {
    if !(2048..=4096).contains(&bits) || bits % 64 != 0 {
        return Err("Key size must be a multiple of 64 from 2048 to 4096".into());
    }
    let members = super::zipalign::read_members(apk)?;
    if !members.iter().any(|member| member.name == "AndroidManifest.xml") {
        return Err("AndroidManifest.xml is required to sign an APK".into());
    }
    let key = Key::generate(bits)?;
    let cert = key.certificate()?;
    let spki = key.spki();
    let mut entries: Vec<(String, Vec<u8>)> = members
        .into_iter()
        .filter(|member| !signature_name(&member.name))
        .map(|member| (member.name, member.data))
        .collect();
    let (manifest, sf, rsa) = v1_files(&entries, &key, &cert)?;
    entries.push(("META-INF/MANIFEST.MF".into(), manifest));
    entries.push(("META-INF/CERT.SF".into(), sf));
    entries.push(("META-INF/CERT.RSA".into(), rsa));
    let aligned = super::zipalign::write_stored(&entries, 4)?;
    insert_schemes(&aligned, &key, &cert, &spki)
}

pub(crate) fn verify(apk: &[u8]) -> Result<Report> {
    let (content_end, cd, eocd) = super::zipalign::regions(apk)?;
    if content_end == cd {
        return Err("Signature block is missing".into());
    }
    let mut digest_eocd = apk[eocd..].to_vec();
    if content_end != cd {
        digest_eocd[16..20].copy_from_slice(&(content_end as u32).to_le_bytes());
    }
    let digest = content_digest(&apk[..content_end], &apk[cd..eocd], &digest_eocd);
    let pairs = parse_pairs(&apk[content_end..cd])?;
    let v2 = pairs.iter().find(|(id, _)| *id == V2_ID).map(|(_, value)| verify_scheme(value, &digest, false)).transpose()?.unwrap_or(false);
    let v3 = pairs.iter().find(|(id, _)| *id == V3_ID).map(|(_, value)| verify_scheme(value, &digest, true)).transpose()?.unwrap_or(false);
    if !v2 || !v3 {
        return Err(format!("Signature scheme check failed (v2 {v2}, v3 {v3})"));
    }
    let v1 = verify_v1(apk)?;
    Ok(Report { v1, v2, v3, detail: format!("v1 {v1}; v2 {v2}; v3 {v3}; digest {}", hex(&digest)) })
}

fn insert_schemes(apk: &[u8], key: &Key, cert: &[u8], spki: &[u8]) -> Result<Vec<u8>> {
    let (content_end, cd, eocd_at) = super::zipalign::regions(apk)?;
    if content_end != cd {
        return Err("Archive already contains a signature block".into());
    }
    let k = key.modulus_len();
    let dummy_digest = [0u8; 32];
    let dummy_sig = vec![0u8; k];
    let probe = signing_block(&dummy_digest, &dummy_sig, &dummy_sig, cert, spki)?;
    let mut eocd = apk[eocd_at..].to_vec();
    let new_cd = (cd + probe.len()) as u32;
    eocd[16..20].copy_from_slice(&new_cd.to_le_bytes());
    let mut digest_eocd = eocd.clone();
    digest_eocd[16..20].copy_from_slice(&(content_end as u32).to_le_bytes());
    let digest = content_digest(&apk[..content_end], &apk[cd..eocd_at], &digest_eocd);
    let sig2 = key.sign(&signed_bytes(&digest, cert, false)?)?;
    let sig3 = key.sign(&signed_bytes(&digest, cert, true)?)?;
    if sig2.len() != k || sig3.len() != k {
        return Err("Signature length changed".into());
    }
    let block = signing_block(&digest, &sig2, &sig3, cert, spki)?;
    if block.len() != probe.len() {
        return Err("Signature block size changed".into());
    }
    let pairs = parse_pairs(&block)?;
    let v2 = pairs.iter().find(|(id, _)| *id == V2_ID).ok_or("v2 pair missing")?;
    let signers = slice_lp(&v2.1)?.0;
    let (signer, _) = slice_lp(signers)?;
    let (signed, rest) = slice_lp(signer)?;
    let (signatures, rest) = slice_lp(rest)?;
    let (public_key, _) = slice_lp(rest)?;
    let extracted = first_signature(signatures)?;
    if extracted != sig2 {
        return Err("Extracted signature bytes differ".into());
    }
    let (n, e) = rsa_public(public_key)?;
    if n.cmp(&key.n) != Ordering::Equal || e.cmp(&key.e) != Ordering::Equal {
        return Err("Extracted public key differs".into());
    }
    let expected = signed_bytes(&digest, cert, false)?;
    if signed != expected.as_slice() {
        return Err("Extracted signed bytes differ".into());
    }
    verify_pkcs1(&key.n, &key.e, signed, &sig2)?;
    let mut out = Vec::with_capacity(apk.len() + block.len());
    out.extend_from_slice(&apk[..cd]);
    out.extend_from_slice(&block);
    out.extend_from_slice(&apk[cd..eocd_at]);
    out.extend_from_slice(&eocd);
    Ok(out)
}

fn signing_block(digest: &[u8; 32], sig2: &[u8], sig3: &[u8], cert: &[u8], spki: &[u8]) -> Result<Vec<u8>> {
    let v2 = pair(V2_ID, &scheme_value(digest, sig2, cert, spki, false)?);
    let v3 = pair(V3_ID, &scheme_value(digest, sig3, cert, spki, true)?);
    let mut pairs = Vec::new();
    pairs.extend(v2);
    pairs.extend(v3);
    let size = (pairs.len() + 24) as u64;
    let mut block = Vec::new();
    block.extend_from_slice(&size.to_le_bytes());
    block.extend(pairs);
    block.extend_from_slice(&size.to_le_bytes());
    block.extend_from_slice(b"APK Sig Block 42");
    Ok(block)
}

fn scheme_value(digest: &[u8; 32], sig: &[u8], cert: &[u8], spki: &[u8], v3: bool) -> Result<Vec<u8>> {
    let signed = signed_bytes(digest, cert, v3)?;
    let mut body = Vec::new();
    body.extend(lp(&signed));
    if v3 {
        body.extend_from_slice(&MIN_SDK.to_le_bytes());
        body.extend_from_slice(&MAX_SDK.to_le_bytes());
    }
    body.extend(lp(&signature_sequence(sig)));
    body.extend(lp(spki));
    Ok(seq(&[body]))
}

fn signed_bytes(digest: &[u8; 32], cert: &[u8], v3: bool) -> Result<Vec<u8>> {
    let record = digest_record(digest);
    let digests = seq(&[record]);
    let certs = seq(&[cert.to_vec()]);
    let attrs = seq(&[]);
    let mut content = Vec::new();
    content.extend(digests);
    content.extend(certs);
    if v3 {
        content.extend_from_slice(&MIN_SDK.to_le_bytes());
        content.extend_from_slice(&MAX_SDK.to_le_bytes());
    }
    content.extend(attrs);
    Ok(lp(&content))
}

fn digest_record(digest: &[u8]) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&ALG.to_le_bytes());
    record.extend_from_slice(&(digest.len() as u32).to_le_bytes());
    record.extend_from_slice(digest);
    record
}

fn signature_sequence(sig: &[u8]) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&ALG.to_le_bytes());
    record.extend_from_slice(&(sig.len() as u32).to_le_bytes());
    record.extend_from_slice(sig);
    seq(&[record])
}

fn verify_scheme(value: &[u8], digest: &[u8; 32], v3: bool) -> Result<bool> {
    let mut signers = slice_lp(value)?.0;
    if signers.is_empty() {
        return Err("No signers".into());
    }
    let (mut signer, _) = slice_lp(signers)?;
    let (signed, rest) = slice_lp(signer)?;
    signer = rest;
    if v3 {
        if signer.len() < 8 {
            return Err("Scheme v3 signer is truncated".into());
        }
        let min = u32::from_le_bytes(signer[0..4].try_into().unwrap());
        let max = u32::from_le_bytes(signer[4..8].try_into().unwrap());
        if min > max {
            return Err("Scheme v3 sdk range is reversed".into());
        }
        signer = &signer[8..];
    }
    let (signatures, rest) = slice_lp(signer)?;
    let (public_key, _) = slice_lp(rest)?;
    let expected = signed_bytes(digest, &certificates(signed, v3)?, v3)?;
    if signed != expected {
        return Err("Signed data does not match the archive digest".into());
    }
    let sig = first_signature(signatures)?;
    let (n, e) = rsa_public(public_key)?;
    verify_pkcs1(&n, &e, signed, &sig)?;
    Ok(true)
}

fn certificates(signed: &[u8], v3: bool) -> Result<Vec<u8>> {
    let (content, _) = slice_lp(signed)?;
    let (_digests, rest) = slice_lp(content)?;
    let (certs, _) = slice_lp(rest)?;
    let (cert, _) = slice_lp(certs)?;
    let _ = v3;
    Ok(cert.to_vec())
}

fn first_signature(signatures: &[u8]) -> Result<Vec<u8>> {
    let (content, _) = slice_lp(signatures)?;
    let (record, _) = slice_lp(content)?;
    if record.len() < 8 {
        return Err("Signature record is truncated".into());
    }
    let alg = u32::from_le_bytes(record[0..4].try_into().unwrap());
    if alg != ALG {
        return Err("Signature algorithm is not supported".into());
    }
    let len = u32::from_le_bytes(record[4..8].try_into().unwrap()) as usize;
    let sig = record.get(8..8 + len).ok_or("Signature bytes are truncated")?;
    Ok(sig.to_vec())
}

fn parse_pairs(block: &[u8]) -> Result<Vec<(u32, Vec<u8>)>> {
    if block.len() < 32 || &block[block.len() - 16..] != b"APK Sig Block 42" {
        return Err("Signature block magic is missing".into());
    }
    let size = u64::from_le_bytes(block[block.len() - 24..block.len() - 16].try_into().unwrap()) as usize;
    if block.len() != size + 8 || u64::from_le_bytes(block[0..8].try_into().unwrap()) as usize != size {
        return Err("Signature block size disagrees".into());
    }
    let mut cursor = 8usize;
    let end = block.len() - 24;
    let mut pairs = Vec::new();
    while cursor < end {
        if cursor + 12 > end {
            return Err("Signature pair is truncated".into());
        }
        let len = u64::from_le_bytes(block[cursor..cursor + 8].try_into().unwrap()) as usize;
        let id = u32::from_le_bytes(block[cursor + 8..cursor + 12].try_into().unwrap());
        let value_end = cursor + 8 + len;
        if len < 4 || value_end > end {
            return Err("Signature pair length is outside the block".into());
        }
        pairs.push((id, block[cursor + 12..value_end].to_vec()));
        cursor = value_end;
    }
    if cursor != end {
        return Err("Signature pairs do not fill the block".into());
    }
    Ok(pairs)
}

fn content_digest(before: &[u8], central: &[u8], eocd: &[u8]) -> [u8; 32] {
    let mut hasher = Chunks::default();
    for section in [before, central, eocd] {
        hasher.update(section);
        hasher.finish_section();
    }
    hasher.finish()
}

#[derive(Default)]
struct Chunks {
    pending: Vec<u8>,
    digests: Vec<u8>,
}

impl Chunks {
    fn update(&mut self, mut data: &[u8]) {
        if !self.pending.is_empty() {
            let need = CHUNK - self.pending.len();
            let take = need.min(data.len());
            self.pending.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.pending.len() == CHUNK {
                self.flush();
            }
        }
        while data.len() >= CHUNK {
            let mut chunk = Vec::with_capacity(CHUNK);
            chunk.extend_from_slice(&data[..CHUNK]);
            self.pending = chunk;
            self.flush();
            data = &data[CHUNK..];
        }
        self.pending.extend_from_slice(data);
    }
    fn flush(&mut self) {
        let mut buf = Vec::with_capacity(5 + self.pending.len());
        buf.push(0xa5);
        buf.extend_from_slice(&(self.pending.len() as u32).to_le_bytes());
        buf.append(&mut self.pending);
        self.digests.extend_from_slice(&sha256(&buf));
    }
    fn finish_section(&mut self) {
        if !self.pending.is_empty() {
            self.flush();
        }
    }
    fn finish(mut self) -> [u8; 32] {
        if !self.pending.is_empty() {
            self.flush();
        }
        let count = (self.digests.len() / 32) as u32;
        let mut top = Vec::with_capacity(5 + self.digests.len());
        top.push(0x5a);
        top.extend_from_slice(&count.to_le_bytes());
        top.extend(self.digests);
        sha256(&top)
    }
}

fn v1_files(entries: &[(String, Vec<u8>)], key: &Key, cert: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let mut manifest = Vec::new();
    append_manifest_line(&mut manifest, "Manifest-Version: 1.0");
    append_manifest_line(&mut manifest, "Built-By: rllm");
    manifest.extend_from_slice(b"\r\n");
    let mut sections = Vec::new();
    for (name, data) in entries {
        if signature_name(name) {
            continue;
        }
        let digest = b64(&sha256(data));
        let mut section = Vec::new();
        append_manifest_line(&mut section, &format!("Name: {name}"));
        append_manifest_line(&mut section, &format!("SHA-256-Digest: {digest}"));
        section.extend_from_slice(b"\r\n");
        manifest.extend_from_slice(&section);
        sections.push((name.clone(), section));
    }
    let mut sf = Vec::new();
    append_manifest_line(&mut sf, "Signature-Version: 1.0");
    append_manifest_line(&mut sf, "Built-By: rllm");
    append_manifest_line(&mut sf, "X-Android-APK-Signed: 2, 3");
    append_manifest_line(&mut sf, &format!("SHA-256-Digest-Manifest: {}", b64(&sha256(&manifest))));
    sf.extend_from_slice(b"\r\n");
    for (name, section) in sections {
        let digest = b64(&sha256(&section));
        append_manifest_line(&mut sf, &format!("Name: {name}"));
        append_manifest_line(&mut sf, &format!("SHA-256-Digest: {digest}"));
        sf.extend_from_slice(b"\r\n");
    }
    let rsa = pkcs7(&sf, key, cert)?;
    Ok((manifest, sf, rsa))
}

fn append_manifest_line(out: &mut Vec<u8>, line: &str) {
    let mut remaining = line;
    let mut first = true;
    while !remaining.is_empty() {
        let limit = if first { 72 } else { 71 };
        let mut end = remaining.len().min(limit);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        if !first {
            out.push(b' ');
        }
        out.extend_from_slice(&remaining.as_bytes()[..end]);
        out.extend_from_slice(b"\r\n");
        remaining = &remaining[end..];
        first = false;
    }
    if line.is_empty() {
        out.extend_from_slice(b"\r\n");
    }
}

fn verify_v1(apk: &[u8]) -> Result<bool> {
    let plain = super::zipalign::strip_signing_block(apk)?;
    let entries = super::zipalign::read_members(&plain)?;
    let manifest = entries.iter().find(|m| m.name == "META-INF/MANIFEST.MF").ok_or("Manifest is missing")?;
    let sf = entries.iter().find(|m| m.name == "META-INF/CERT.SF").ok_or("Signature file is missing")?;
    let rsa = entries.iter().find(|m| m.name == "META-INF/CERT.RSA").ok_or("Signature block file is missing")?;
    verify_v1_signature(&sf.data, &rsa.data)?;
    if !manifest_digest_matches(&sf.data, &manifest.data)? {
        return Err("Manifest digest does not match".into());
    }
    let sections = manifest_sections(&manifest.data)?;
    for (name, data) in entries.iter().map(|m| (m.name.as_str(), m.data.as_slice())) {
        if signature_name(name) {
            continue;
        }
        let digest = b64(&sha256(data));
        let covered = sections.iter().any(|section| {
            attribute(section, "Name") == Some(name) && attribute(section, "SHA-256-Digest") == Some(digest.as_str())
        });
        if !covered {
            return Err(format!("Manifest digest does not match {name}"));
        }
    }
    Ok(true)
}

fn manifest_digest_matches(sf: &[u8], manifest: &[u8]) -> Result<bool> {
    let sections = manifest_sections(sf)?;
    let value = sections.first().and_then(|section| attribute(section, "SHA-256-Digest-Manifest"));
    Ok(value == Some(b64(&sha256(manifest)).as_str()))
}

fn manifest_sections(bytes: &[u8]) -> Result<Vec<Vec<(String, String)>>> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Manifest is not UTF-8".to_string())?;
    let mut sections: Vec<Vec<(String, String)>> = vec![Vec::new()];
    for line in text.split("\r\n") {
        if line.is_empty() {
            if !sections.last().unwrap().is_empty() {
                sections.push(Vec::new());
            }
        } else if let Some(value) = line.strip_prefix(' ') {
            let (_, previous) = sections.last_mut().and_then(|section| section.last_mut()).ok_or("Manifest continuation has no preceding field")?;
            previous.push_str(value);
        } else {
            let (name, value) = line.split_once(": ").ok_or("Manifest field has no value separator")?;
            if name.is_empty() {
                return Err("Manifest field name is empty".into());
            }
            sections.last_mut().unwrap().push((name.to_string(), value.to_string()));
        }
    }
    if sections.last().map(Vec::is_empty).unwrap_or(false) {
        sections.pop();
    }
    Ok(sections)
}

fn attribute<'a>(section: &'a [(String, String)], name: &str) -> Option<&'a str> {
    section.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
}

fn signature_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "META-INF/MANIFEST.MF"
        || (upper.starts_with("META-INF/") && (upper.ends_with(".SF") || upper.ends_with(".RSA") || upper.ends_with(".DSA") || upper.ends_with(".EC")))
}

fn lp(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts {
        body.extend(lp(part));
    }
    lp(&body)
}

fn pair(id: u32, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&((4 + value.len()) as u64).to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(value);
    out
}

fn slice_lp(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    if bytes.len() < 4 {
        return Err("Length prefix is truncated".into());
    }
    let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let end = 4 + len;
    if end > bytes.len() {
        return Err("Length-prefixed value is truncated".into());
    }
    Ok((&bytes[4..end], &bytes[end..]))
}

struct Key {
    n: Nat,
    e: Nat,
    d: Nat,
}

impl Key {
    fn generate(bits: usize) -> Result<Self> {
        if !(2048..=4096).contains(&bits) || bits % 64 != 0 {
            return Err("Key size must be a multiple of 64 from 2048 to 4096".into());
        }
        let e = Nat::from_u32(65537);
        for _ in 0..32 {
            let p = prime(bits / 2, &e)?;
            let q = prime(bits / 2, &e)?;
            if p.cmp(&q) == Ordering::Equal {
                continue;
            }
            let (p, q) = if p.cmp(&q) == Ordering::Greater { (p, q) } else { (q, p) };
            let n = p.mul(&q);
            if n.bit_len() != bits {
                continue;
            }
            let one = Nat::from_u32(1);
            let phi = p.sub(&one).mul(&q.sub(&one));
            let d = e.inv(&phi)?;
            if d.mul(&e).modulo(&phi).cmp(&one) != Ordering::Equal {
                return Err("Generated private exponent does not match the public exponent".into());
            }
            return Ok(Key { n, e, d });
        }
        Err("A signing key was not produced".into())
    }
    fn modulus_len(&self) -> usize { (self.n.bit_len() + 7) / 8 }
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        let k = self.modulus_len();
        let em = pkcs1_encode(&sha256(message), k)?;
        let value = Nat::from_be(&em);
        let sig = self.d.modpow(&value, &self.n);
        let other = value.pow_msb(&self.d, &self.n);
        if sig.cmp(&other) != Ordering::Equal {
            return Err("Exponent paths disagree".into());
        }
        let back = self.e.modpow(&sig, &self.n);
        if back.cmp(&value) != Ordering::Equal {
            let mut left = value.clone();
            let mut right = self.n.clone();
            while !right.is_zero() {
                let next = left.modulo(&right);
                left = right;
                right = next;
            }
            let diff = if back.cmp(&value) == Ordering::Greater { back.sub(&value) } else { value.sub(&back) };
            return Err(format!(
                "round-trip value {} sig {} back {} modulus {} gcd {} diff {}",
                value.bit_len(),
                sig.bit_len(),
                back.bit_len(),
                self.n.bit_len(),
                left.bit_len(),
                diff.bit_len()
            ));
        }
        let bytes = sig.to_be_fixed(k)?;
        if self.e.modpow(&Nat::from_be(&bytes), &self.n).to_be_fixed(k)? != em {
            return Err("This signature encoding does not round-trip".into());
        }
        Ok(bytes)
    }
    fn spki(&self) -> Vec<u8> {
        let body = der_seq(&[der_int(&self.n.to_be()), der_int(&self.e.to_be())]);
        der_seq(&[der_seq(&[der_oid(RSA_OID), der_null()]), der_bit(&body)])
    }
    fn certificate(&self) -> Result<Vec<u8>> {
        let serial = {
            let mut raw = random(8)?;
            raw[0] &= 0x7f;
            if raw.iter().all(|b| *b == 0) { raw[7] = 1; }
            raw
        };
        let name = der_seq(&[der_set(&[der_seq(&[der_oid(CN_OID), der_printable(b"rllm")])])]);
        let validity = der_seq(&[der_time(b"260101000000Z"), der_time(b"360101000000Z")]);
        let tbs = der_seq(&[
            der_explicit(0, &der_int(&[2])),
            der_int(&serial),
            der_seq(&[der_oid(SHA256_RSA_OID), der_null()]),
            name.clone(),
            validity,
            name,
            self.spki(),
        ]);
        let sig = self.sign(&tbs)?;
        Ok(der_seq(&[tbs, der_seq(&[der_oid(SHA256_RSA_OID), der_null()]), der_bit(&sig)]))
    }
}

fn verify_pkcs1(n: &Nat, e: &Nat, message: &[u8], signature: &[u8]) -> Result<()> {
    let k = (n.bit_len() + 7) / 8;
    if signature.len() != k {
        return Err("Signature length does not match the modulus".into());
    }
    let value = Nat::from_be(signature);
    if value.cmp(n) != Ordering::Less {
        return Err("Signature is not smaller than the modulus".into());
    }
    let encoded = e.modpow(&value, n).to_be_fixed(k)?;
    let expect = pkcs1_encode(&sha256(message), k)?;
    if encoded != expect {
        return Err("Signature did not verify".into());
    }
    Ok(())
}

fn pkcs1_encode(hash: &[u8], k: usize) -> Result<Vec<u8>> {
    const PREFIX: &[u8] = &[
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20,
    ];
    if k < PREFIX.len() + hash.len() + 11 {
        return Err("Modulus is too small for the digest".into());
    }
    let mut em = vec![0xffu8; k];
    em[0] = 0x00;
    em[1] = 0x01;
    let marker = k - PREFIX.len() - hash.len();
    em[marker - 1] = 0x00;
    em[marker..marker + PREFIX.len()].copy_from_slice(PREFIX);
    em[k - hash.len()..].copy_from_slice(hash);
    Ok(em)
}

fn rsa_public(spki: &[u8]) -> Result<(Nat, Nat)> {
    let body = der_body(0x30, spki)?;
    let (alg, rest) = der_take(body)?;
    let _ = alg;
    let (bit, _) = der_take(rest)?;
    let key_bytes = der_body(0x03, bit)?;
    if key_bytes.first() != Some(&0) {
        return Err("Public key bit string is not byte-aligned".into());
    }
    let key = der_body(0x30, &key_bytes[1..])?;
    let (n, rest) = der_take(key)?;
    let (e, _) = der_take(rest)?;
    Ok((Nat::from_be(der_body(0x02, n)?), Nat::from_be(der_body(0x02, e)?)))
}

fn pkcs7(message: &[u8], key: &Key, cert: &[u8]) -> Result<Vec<u8>> {
    let sig = key.sign(message)?;
    let signer = der_seq(&[
        der_int(&[1]),
        der_seq(&[issuer_of(cert)?, serial_of(cert)?]),
        der_seq(&[der_oid(SHA256_OID), der_null()]),
        der_seq(&[der_oid(SHA256_RSA_OID), der_null()]),
        der_tlv(0x04, &sig),
    ]);
    let signed = der_seq(&[
        der_int(&[1]),
        der_set(&[der_seq(&[der_oid(SHA256_OID), der_null()])]),
        der_seq(&[der_oid(DATA_OID)]),
        der_implicit(0, std::slice::from_ref(&cert.to_vec())),
        der_set(&[signer]),
    ]);
    Ok(der_seq(&[der_oid(SIGNED_OID), der_explicit(0, &signed)]))
}

fn pkcs7_content(pkcs: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    // The caller passes the signature file separately. This checks the
    // certificate public key against the encrypted digest once the caller
    // supplies the signed bytes. Here we only recover the signature and cert.
    let body = der_body(0x30, pkcs)?;
    let (_oid, rest) = der_take(body)?;
    let (explicit, _) = der_take(rest)?;
    let signed = der_body(0xa0, explicit)?;
    let signed = der_body(0x30, signed)?;
    let mut cursor = signed;
    for _ in 0..3 {
        let (_, next) = der_take(cursor)?;
        cursor = next;
    }
    let (certs, rest) = der_take(cursor)?;
    let cert_bytes = der_body(0xa0, certs)?;
    let (cert, _) = der_take(cert_bytes)?;
    let (signers, _) = der_take(rest)?;
    let signer_set = der_body(0x31, signers)?;
    let (signer, _) = der_take(signer_set)?;
    let signer = der_body(0x30, signer)?;
    let mut cursor = signer;
    for _ in 0..4 {
        let (_, next) = der_take(cursor)?;
        cursor = next;
    }
    let (digest, _) = der_take(cursor)?;
    let sig = der_body(0x04, digest)?.to_vec();
    let (n, e) = rsa_public(spki_of(cert)?)?;
    // The signed message is recovered by the caller. Stash nothing; the
    // verify_v1 path checks the manifest. Signature bytes are checked there
    // by re-parsing. Return the cert and leave signature verification to verify_detached.
    let _ = (sig, n, e);
    Ok((Vec::new(), cert.to_vec()))
}

fn verify_v1_signature(sf: &[u8], rsa: &[u8]) -> Result<()> {
    let body = der_body(0x30, rsa)?;
    let (_oid, rest) = der_take(body)?;
    let (explicit, _) = der_take(rest)?;
    let signed = der_body(0x30, der_body(0xa0, explicit)?)?;
    let mut cursor = signed;
    for _ in 0..3 {
        let (_, next) = der_take(cursor)?;
        cursor = next;
    }
    let (certs, rest) = der_take(cursor)?;
    let (cert, _) = der_take(der_body(0xa0, certs)?)?;
    let (signers, _) = der_take(rest)?;
    let (signer, _) = der_take(der_body(0x31, signers)?)?;
    let signer = der_body(0x30, signer)?;
    let mut cursor = signer;
    for _ in 0..4 {
        let (_, next) = der_take(cursor)?;
        cursor = next;
    }
    let (digest, _) = der_take(cursor)?;
    let sig = der_body(0x04, digest)?;
    let (n, e) = rsa_public(spki_of(cert)?)?;
    verify_pkcs1(&n, &e, sf, sig)
}

fn spki_of(cert: &[u8]) -> Result<&[u8]> {
    let tbs = der_body(0x30, der_body(0x30, cert)?)?;
    let mut cursor = tbs;
    // version, serial, alg, issuer, validity, subject, spki
    for _ in 0..6 {
        let (_, next) = der_take(cursor)?;
        cursor = next;
    }
    let (spki, _) = der_take(cursor)?;
    Ok(spki)
}

fn issuer_of(cert: &[u8]) -> Result<Vec<u8>> {
    let tbs = der_body(0x30, der_body(0x30, cert)?)?;
    let mut cursor = tbs;
    for _ in 0..3 {
        let (_, next) = der_take(cursor)?;
        cursor = next;
    }
    let (issuer, _) = der_take(cursor)?;
    Ok(issuer.to_vec())
}

fn serial_of(cert: &[u8]) -> Result<Vec<u8>> {
    let tbs = der_body(0x30, der_body(0x30, cert)?)?;
    let mut cursor = tbs;
    let (_, cursor) = der_take(cursor)?;
    let (serial, _) = der_take(cursor)?;
    Ok(serial.to_vec())
}

const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const SHA256_RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
const SHA256_OID: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
const CN_OID: &[u8] = &[0x55, 0x04, 0x03];
const DATA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01];
const SIGNED_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];

fn der_len(len: usize) -> Vec<u8> {
    if len < 128 {
        vec![len as u8]
    } else if len < 256 {
        vec![0x81, len as u8]
    } else {
        vec![0x82, (len >> 8) as u8, (len & 0xff) as u8]
    }
}
fn der_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend(der_len(body.len()));
    out.extend_from_slice(body);
    out
}
fn der_seq(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts { body.extend(part); }
    der_tlv(0x30, &body)
}
fn der_set(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts { body.extend(part); }
    der_tlv(0x31, &body)
}
fn der_oid(bytes: &[u8]) -> Vec<u8> { der_tlv(0x06, bytes) }
fn der_null() -> Vec<u8> { vec![0x05, 0x00] }
fn der_int(bytes: &[u8]) -> Vec<u8> {
    let mut body = bytes.to_vec();
    while body.len() > 1 && body[0] == 0 { body.remove(0); }
    if body.is_empty() || body[0] & 0x80 != 0 { body.insert(0, 0); }
    der_tlv(0x02, &body)
}
fn der_bit(bytes: &[u8]) -> Vec<u8> {
    let mut body = vec![0];
    body.extend_from_slice(bytes);
    der_tlv(0x03, &body)
}
fn der_printable(bytes: &[u8]) -> Vec<u8> { der_tlv(0x13, bytes) }
fn der_time(bytes: &[u8]) -> Vec<u8> { der_tlv(0x17, bytes) }
fn der_explicit(n: u8, inner: &[u8]) -> Vec<u8> { der_tlv(0xa0 | n, inner) }
fn der_implicit(n: u8, parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts { body.extend(part); }
    der_tlv(0xa0 | n, &body)
}
fn der_body<'a>(tag: u8, bytes: &'a [u8]) -> Result<&'a [u8]> {
    if bytes.first() != Some(&tag) { return Err("Certificate encoding tag mismatch".into()); }
    let (len, rest) = der_len_of(&bytes[1..])?;
    if rest.len() < len { return Err("Certificate encoding is truncated".into()); }
    Ok(&rest[..len])
}
fn der_take(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    if bytes.is_empty() { return Err("Certificate encoding ended early".into()); }
    let (len, rest) = der_len_of(&bytes[1..])?;
    let total = 1 + (bytes.len() - 1 - rest.len()) + len;
    if total > bytes.len() { return Err("Certificate encoding is truncated".into()); }
    Ok((&bytes[..total], &bytes[total..]))
}
fn der_len_of(bytes: &[u8]) -> Result<(usize, &[u8])> {
    let first = *bytes.first().ok_or("Certificate length is missing")?;
    if first < 128 { return Ok((first as usize, &bytes[1..])); }
    let count = (first & 0x7f) as usize;
    if count == 0 || count > 3 || bytes.len() < 1 + count { return Err("Certificate length is invalid".into()); }
    let mut len = 0usize;
    for byte in &bytes[1..1 + count] { len = (len << 8) | *byte as usize; }
    Ok((len, &bytes[1 + count..]))
}

fn prime(bits: usize, e: &Nat) -> Result<Nat> {
    for _ in 0..20_000 {
        let candidate = random_odd(bits)?;
        if !trial(&candidate) || !miller(&candidate)? { continue; }
        let one = Nat::from_u32(1);
        if candidate.sub(&one).modulo(e).is_zero() { continue; }
        return Ok(candidate);
    }
    Err("A prime was not found".into())
}

fn trial(n: &Nat) -> bool {
    const SMALL: &[u32] = &[3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97];
    for p in SMALL {
        if n.modulo(&Nat::from_u32(*p)).is_zero() { return false; }
    }
    true
}

fn miller(n: &Nat) -> Result<bool> {
    let one = Nat::from_u32(1);
    let two = Nat::from_u32(2);
    let mut d = n.sub(&one);
    let mut s = 0u32;
    while !d.bit(0) { d = d.shr1(); s += 1; }
    let mont = Mont::new(n);
    let one_mont = mont.to(one.clone());
    let minus_one_mont = mont.to(n.sub(&one));
    let mut bases = Vec::with_capacity(64);
    for _ in 0..64 {
        let upper = n.sub(&Nat::from_u32(3));
        bases.push(random_below(&upper)?.add(&two));
    }
    for a in bases {
        if a.cmp(n) != Ordering::Less { continue; }
        let mut x = mont.pow_mont(&a, &d);
        if x.cmp(&one_mont) == Ordering::Equal || x.cmp(&minus_one_mont) == Ordering::Equal { continue; }
        let mut found = false;
        for _ in 1..s {
            x = mont.mul(&x, &x);
            if x.cmp(&minus_one_mont) == Ordering::Equal { found = true; break; }
        }
        if !found { return Ok(false); }
    }
    Ok(true)
}

fn random_below(limit: &Nat) -> Result<Nat> {
    if limit.is_zero() {
        return Err("Random integer limit is zero".into());
    }
    let bits = limit.bit_len();
    let count = bits.div_ceil(8);
    for _ in 0..128 {
        let mut raw = random(count)?;
        let unused = count * 8 - bits;
        if unused > 0 {
            raw[0] &= 0xff >> unused;
        }
        let value = Nat::from_be(&raw);
        if value.cmp(limit) == Ordering::Less {
            return Ok(value);
        }
    }
    Err("The operating system randomness source did not produce a bounded value".into())
}

fn random_odd(bits: usize) -> Result<Nat> {
    let bytes = bits / 8;
    let mut raw = random(bytes)?;
    raw[0] |= 0x80;
    let last = bytes - 1;
    raw[last] |= 1;
    Ok(Nat::from_be(&raw))
}

fn random(count: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; count];
    fill_random(&mut bytes)?;
    Ok(bytes)
}

#[cfg(windows)]
fn fill_random(buf: &mut [u8]) -> Result<()> {
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(alg: *mut core::ffi::c_void, buf: *mut u8, len: u32, flags: u32) -> i32;
    }
    if buf.len() > u32::MAX as usize { return Err("Random request is too large".into()); }
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, 2) };
    if status != 0 { return Err("The operating system did not supply randomness".into()); }
    Ok(())
}

#[cfg(unix)]
fn fill_random(buf: &mut [u8]) -> Result<()> {
    let mut file = std::fs::File::open("/dev/urandom").map_err(|_| "The operating system did not supply randomness".to_string())?;
    std::io::Read::read_exact(&mut file, buf).map_err(|_| "The operating system randomness was incomplete".to_string())
}

#[cfg(not(any(windows, unix)))]
fn fill_random(_buf: &mut [u8]) -> Result<()> { Err("This platform has no randomness source".into()) }

fn b64(bytes: &[u8]) -> String { super::network::b64(bytes) }
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

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
    let mut state: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 { message.push(0); }
    message.extend_from_slice(&bits.to_be_bytes());
    for chunk in message.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap()); }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut a = state[0]; let mut b = state[1]; let mut c = state[2]; let mut d = state[3];
        let mut e = state[4]; let mut f = state[5]; let mut g = state[6]; let mut h = state[7];
        for i in 0..64 {
            let t1 = h.wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25)).wrapping_add((e & f) ^ ((!e) & g)).wrapping_add(K[i]).wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) ^ (a & c) ^ (b & c));
            h = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        state[0] = state[0].wrapping_add(a); state[1] = state[1].wrapping_add(b); state[2] = state[2].wrapping_add(c); state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e); state[5] = state[5].wrapping_add(f); state[6] = state[6].wrapping_add(g); state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() { out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes()); }
    out
}

#[derive(Clone)]
struct Nat { v: Vec<u32> }

impl Nat {
    fn from_u32(n: u32) -> Self { Self { v: vec![n] }.trim() }
    fn zero() -> Self { Self::from_u32(0) }
    fn is_zero(&self) -> bool { self.v.iter().all(|w| *w == 0) }
    fn from_be(bytes: &[u8]) -> Self {
        let mut v = vec![0u32; bytes.len().div_ceil(4)];
        for (i, &byte) in bytes.iter().enumerate() {
            let rev = bytes.len() - 1 - i;
            v[rev / 4] |= (byte as u32) << ((rev % 4) * 8);
        }
        Self { v }.trim()
    }
    fn to_be(&self) -> Vec<u8> {
        if self.is_zero() { return vec![0]; }
        let mut out = Vec::new();
        for limb in self.v.iter().rev() { out.extend_from_slice(&limb.to_be_bytes()); }
        while out.first() == Some(&0) { out.remove(0); }
        out
    }
    fn to_be_fixed(&self, width: usize) -> Result<Vec<u8>> {
        let mut raw = self.to_be();
        if raw.len() > width { return Err("Integer does not fit the modulus width".into()); }
        let mut out = vec![0u8; width - raw.len()];
        out.append(&mut raw);
        Ok(out)
    }
    fn trim(mut self) -> Self {
        while self.v.len() > 1 && self.v.last() == Some(&0) { self.v.pop(); }
        self
    }
    fn bit_len(&self) -> usize {
        let Some(last) = self.v.last() else { return 0 };
        if *last == 0 { return 0; }
        (self.v.len() - 1) * 32 + (32 - last.leading_zeros() as usize)
    }
    fn bit(&self, index: usize) -> bool {
        self.v.get(index / 32).map(|w| w & (1 << (index % 32)) != 0).unwrap_or(false)
    }
    fn cmp(&self, other: &Self) -> Ordering {
        let a = self.v.len();
        let b = other.v.len();
        let n = a.max(b);
        for i in (0..n).rev() {
            let aw = if i < a { self.v[i] } else { 0 };
            let bw = if i < b { other.v[i] } else { 0 };
            if aw != bw { return aw.cmp(&bw); }
        }
        Ordering::Equal
    }
    fn add(&self, other: &Self) -> Self {
        let n = self.v.len().max(other.v.len());
        let mut v = vec![0u32; n + 1];
        let mut carry = 0u64;
        for i in 0..n {
            let sum = carry + self.v.get(i).copied().unwrap_or(0) as u64 + other.v.get(i).copied().unwrap_or(0) as u64;
            v[i] = sum as u32;
            carry = sum >> 32;
        }
        v[n] = carry as u32;
        Self { v }.trim()
    }
    fn sub(&self, other: &Self) -> Self {
        let mut v = vec![0u32; self.v.len()];
        let mut borrow = 0i64;
        for i in 0..self.v.len() {
            let diff = self.v[i] as i64 - other.v.get(i).copied().unwrap_or(0) as i64 - borrow;
            if diff < 0 { v[i] = (diff + (1i64 << 32)) as u32; borrow = 1; } else { v[i] = diff as u32; borrow = 0; }
        }
        Self { v }.trim()
    }
    fn shl1(&self) -> Self {
        let mut v = vec![0u32; self.v.len() + 1];
        let mut carry = 0u32;
        for i in 0..self.v.len() {
            v[i] = (self.v[i] << 1) | carry;
            carry = self.v[i] >> 31;
        }
        v[self.v.len()] = carry;
        Self { v }.trim()
    }
    fn shr1(&self) -> Self {
        let mut v = vec![0u32; self.v.len()];
        let mut carry = 0u32;
        for i in (0..self.v.len()).rev() {
            v[i] = (self.v[i] >> 1) | carry;
            carry = (self.v[i] & 1) << 31;
        }
        Self { v }.trim()
    }
    fn mul(&self, other: &Self) -> Self {
        let mut v = vec![0u64; self.v.len() + other.v.len() + 2];
        for i in 0..self.v.len() {
            let ai = self.v[i] as u128;
            let mut carry = 0u128;
            for j in 0..other.v.len() {
                let prod = v[i + j] as u128 + ai * other.v[j] as u128 + carry;
                v[i + j] = (prod as u64) & 0xffff_ffff;
                carry = prod >> 32;
            }
            let mut idx = i + other.v.len();
            while carry != 0 && idx < v.len() {
                let prod = v[idx] as u128 + carry;
                v[idx] = (prod as u64) & 0xffff_ffff;
                carry = prod >> 32;
                idx += 1;
            }
        }
        Self { v: v.iter().map(|w| *w as u32).collect() }.trim()
    }
    fn divmod(&self, divisor: &Self) -> (Self, Self) {
        if divisor.is_zero() { return (Self::zero(), self.clone()); }
        let mut quot = vec![0u32; self.v.len()];
        let mut rem = Self::zero();
        for i in (0..self.bit_len()).rev() {
            rem = rem.shl1();
            if self.bit(i) { rem = rem.add(&Self::from_u32(1)); }
            if rem.cmp(divisor) != Ordering::Less {
                rem = rem.sub(divisor);
                quot[i / 32] |= 1 << (i % 32);
                if rem.cmp(divisor) != Ordering::Less {
                    panic!("division remainder stayed above the divisor");
                }
            }
        }
        (Self { v: quot }.trim(), rem.trim())
    }
    fn modulo(&self, m: &Self) -> Self { self.divmod(m).1 }
    fn modpow(&self, exp: &Self, m: &Self) -> Self {
        if m.is_zero() {
            return Self::zero();
        }
        if m.v.first().map(|word| *word & 1 == 1).unwrap_or(false) {
            Mont::new(m).pow(self, exp)
        } else {
            self.pow_plain(exp, m)
        }
    }
    fn pow_plain(&self, exp: &Self, m: &Self) -> Self {
        let mut result = Self::from_u32(1);
        let mut base = self.modulo(m);
        for i in 0..exp.bit_len() {
            if exp.bit(i) { result = result.mul(&base).modulo(m); }
            base = base.mul(&base).modulo(m);
        }
        result
    }
    fn pow_msb(&self, exp: &Self, m: &Self) -> Self {
        let mut result = Self::from_u32(1);
        let base = self.modulo(m);
        for i in (0..exp.bit_len()).rev() {
            result = result.mul(&result).modulo(m);
            if exp.bit(i) { result = result.mul(&base).modulo(m); }
        }
        result
    }
    fn inv(&self, m: &Self) -> Result<Self> {
        let mut r0 = m.clone();
        let mut r1 = self.modulo(m);
        let mut s0 = Signed::zero();
        let mut s1 = Signed::one();
        while !r1.is_zero() {
            let (q, r) = r0.divmod(&r1);
            let next = s0.sub(&Signed::pos(q).mul(&s1));
            s0 = s1;
            s1 = next;
            r0 = r1;
            r1 = r;
        }
        if r0.cmp(&Self::from_u32(1)) != Ordering::Equal { return Err("Public exponent is not invertible".into()); }
        Ok(s0.residue(m))
    }
}

struct Signed { neg: bool, mag: Nat }
impl Signed {
    fn zero() -> Self { Self { neg: false, mag: Nat::zero() } }
    fn one() -> Self { Self { neg: false, mag: Nat::from_u32(1) } }
    fn pos(n: Nat) -> Self { Self { neg: false, mag: n } }
    fn mul(&self, other: &Self) -> Self {
        Self { neg: self.neg ^ other.neg, mag: self.mag.mul(&other.mag) }
    }
    fn sub(&self, other: &Self) -> Self {
        self.add(&Self { neg: !other.neg, mag: other.mag.clone() })
    }
    fn add(&self, other: &Self) -> Self {
        if self.neg == other.neg {
            return Self { neg: self.neg && !self.mag.is_zero(), mag: self.mag.add(&other.mag) };
        }
        match self.mag.cmp(&other.mag) {
            Ordering::Greater => Self { neg: self.neg, mag: self.mag.sub(&other.mag) },
            Ordering::Less => Self { neg: other.neg, mag: other.mag.sub(&self.mag) },
            Ordering::Equal => Self::zero(),
        }
    }
    fn residue(&self, m: &Nat) -> Nat {
        let mut mag = self.mag.modulo(m);
        if self.neg && !mag.is_zero() { mag = m.sub(&mag); }
        mag
    }
}

struct Mont { m: Nat, k: usize, n0: u32, r2: Nat }
impl Mont {
    fn new(m: &Nat) -> Self {
        let k = m.v.len();
        let n0 = inv32(m.v[0]).wrapping_neg();
        let mut r2 = Nat::from_u32(1);
        let two_k = 64 * k;
        for _ in 0..two_k {
            r2 = r2.shl1();
            if r2.cmp(m) != Ordering::Less { r2 = r2.sub(m); }
        }
        Self { m: m.clone(), k, n0, r2 }
    }
    fn to(&self, x: Nat) -> Nat { self.mul(&self.pad(x), &self.pad(self.r2.clone())) }
    fn from(&self, x: Nat) -> Nat { self.mul(&self.pad(x), &self.pad(Nat::from_u32(1))) }
    fn pad(&self, mut x: Nat) -> Nat {
        x.v.resize(self.k, 0);
        x
    }
    fn mul(&self, a: &Nat, b: &Nat) -> Nat {
        let k = self.k;
        let mut t = vec![0u64; 2 * k + 2];
        for i in 0..k {
            let ai = a.v.get(i).copied().unwrap_or(0) as u128;
            let mut carry = 0u128;
            for j in 0..k {
                let bj = b.v.get(j).copied().unwrap_or(0) as u128;
                let prod = t[i + j] as u128 + ai * bj + carry;
                t[i + j] = (prod as u64) & 0xffff_ffff;
                carry = prod >> 32;
            }
            t[i + k] += carry as u64;
        }
        let mut extra = 0u128;
        for word in t.iter_mut() {
            let prod = *word as u128 + extra;
            *word = (prod as u64) & 0xffff_ffff;
            extra = prod >> 32;
        }
        for i in 0..k {
            let u = (t[i] as u32).wrapping_mul(self.n0) as u128;
            let mut carry = 0u128;
            for j in 0..k {
                let mj = self.m.v.get(j).copied().unwrap_or(0) as u128;
                let prod = t[i + j] as u128 + u * mj + carry;
                t[i + j] = (prod as u64) & 0xffff_ffff;
                carry = prod >> 32;
            }
            let mut idx = i + k;
            while carry != 0 && idx < t.len() {
                let prod = t[idx] as u128 + carry;
                t[idx] = (prod as u64) & 0xffff_ffff;
                carry = prod >> 32;
                idx += 1;
            }
        }
        let mut high = t[2 * k];
        let mut limbs: Vec<u32> = t[k..2 * k].iter().map(|w| *w as u32).collect();
        for _ in 0..16 {
            let mut greater = high > 0;
            let mut equal = high == 0;
            if high == 0 {
                greater = false;
                equal = true;
                for i in (0..k).rev() {
                    let aw = limbs[i];
                    let bw = self.m.v.get(i).copied().unwrap_or(0);
                    if aw != bw {
                        greater = aw > bw;
                        equal = false;
                        break;
                    }
                }
            }
            if !greater && !equal {
                break;
            }
            let mut borrow = 0i64;
            for i in 0..k {
                let diff = limbs[i] as i64 - self.m.v.get(i).copied().unwrap_or(0) as i64 - borrow;
                if diff < 0 {
                    limbs[i] = (diff + (1 << 32)) as u32;
                    borrow = 1;
                } else {
                    limbs[i] = diff as u32;
                    borrow = 0;
                }
            }
            if borrow > 0 && high > 0 {
                high -= 1;
            }
        }
        Nat { v: limbs }.trim()
    }
    fn pow(&self, base: &Nat, exp: &Nat) -> Nat {
        self.from(self.pow_mont(base, exp))
    }
    fn pow_mont(&self, base: &Nat, exp: &Nat) -> Nat {
        let mut result = self.to(Nat::from_u32(1));
        let mut base = self.to(base.modulo(&self.m));
        let bits = exp.bit_len();
        for i in 0..bits {
            if exp.bit(i) {
                result = self.mul(&result, &base);
            }
            if i + 1 < bits {
                base = self.mul(&base, &base);
            }
        }
        result
    }
}

fn inv32(n: u32) -> u32 {
    let mut x = 1u32;
    for _ in 0..5 { x = x.wrapping_mul(2u32.wrapping_sub(n.wrapping_mul(x))); }
    x
}

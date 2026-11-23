//! In-process TLS 1.3 and TLS 1.2 for desktop hosts that do not use the
//! operating system's handshake API. Trust anchors are the system certificate
//! bundle files. The peer name is checked against the certificate. The core
//! still builds every request byte and validates every response byte.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const RECORD_LIMIT: usize = 16 * 1024;
const HANDSHAKE_LIMIT: usize = 256 * 1024;
const CHAIN_LIMIT: usize = 8;
const HANDSHAKE_RECORD_BUDGET: usize = 64;
const HANDSHAKE_BUDGET: Duration = Duration::from_secs(30);
const RSA_EXPONENT_BITS: usize = 32;
const EXTENSION_LIMIT: usize = 32;
const NAME_LIMIT: usize = 64;
const KU_DIGITAL_SIGNATURE: u16 = 1 << 0;
const KU_KEY_CERT_SIGN: u16 = 1 << 5;
const TLS12_VERSION: u16 = 0x0303;
const TLS13_VERSION: u16 = 0x0304;
const HELLO_RETRY_RANDOM: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91, 0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

#[derive(Clone)]
struct HelloRetry {
    suite: u16,
    version: u16,
    group: u16,
    cookie: Vec<u8>,
}

pub struct Client {
    stream: TcpStream,
    incoming: Vec<u8>,
    incoming_at: usize,
    write: Traffic,
    read: Traffic,
    /// TLS 1.3 application traffic secrets. Empty for TLS 1.2.
    read_secret: Vec<u8>,
    write_secret: Vec<u8>,
    hash: HashKind,
    tls13: bool,
    pending: Vec<u8>,
    key_updates: u16,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Client")
    }
}

struct Traffic {
    kind: CipherKind,
    key: [u8; 32],
    key_len: usize,
    iv: [u8; 12],
    seq: u64,
    /// TLS 1.2 records carry the content type in the header and an explicit nonce.
    legacy: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CipherKind {
    Aes128,
    Aes256,
    ChaCha,
}

impl Client {
    /// Completes a verified handshake for `host` over `stream`.
    pub(crate) fn primitive_self_check() -> Result<(), String> {
    crypto_self_check()
}

pub fn open(stream: TcpStream, host: &str) -> Result<Client, String> {
        crypto_self_check()?;
        if host.is_empty() || host.len() > 253 || host.as_bytes().iter().any(|byte| *byte <= 32 || *byte >= 127) {
            return Err("TLS host name is not usable".into());
        }
        let anchors = trust_anchors()?;
        if anchors.is_empty() {
            return Err("No system trust anchors were found".into());
        }
        handshake(stream, host, &anchors)
    }

    pub fn set_read_timeout(&self, timeout: Option<std::time::Duration>) {
        self.stream.set_read_timeout(timeout).ok();
    }
}

fn handshake(mut stream: TcpStream, host: &str, anchors: &[Certificate]) -> Result<Client, String> {
    let previous = stream.read_timeout().unwrap_or(None);
    let deadline = Instant::now() + HANDSHAKE_BUDGET;
    stream.set_read_timeout(Some(HANDSHAKE_BUDGET)).ok();
    let result = handshake_deadline(&mut stream, host, anchors, deadline, previous);
    if result.is_err() {
        stream.set_read_timeout(previous).ok();
    }
    result
}

fn handshake_deadline(stream: &mut TcpStream, host: &str, anchors: &[Certificate], deadline: Instant, previous: Option<Duration>) -> Result<Client, String> {
    let mut random = [0u8; 32];
    random.copy_from_slice(&crate::state_crypto::random_bytes(32)?);
    let x25519_secret = clamp_scalar(&crate::state_crypto::random_bytes(32)?);
    let x25519_public = x25519(&x25519_secret, &X25519_BASE);
    let p256_secret = fresh_p256_secret()?;
    let curve_public = p256_public(&p256_secret)?;
    let mut session = [0u8; 32];
    session.copy_from_slice(&crate::state_crypto::random_bytes(32)?);
    let hello = client_hello(host, &random, &session, &x25519_public, &curve_public, &[], None)?;
    write_record(stream, 22, &hello, None)?;
    let mut transcript = hello.clone();
    let mut pending = Vec::new();
    let mut records = 0usize;
    let mut offered_x = x25519_secret;
    let mut offered_p = p256_secret;
    let mut offered_xp = x25519_public;
    let mut offered_pp = curve_public;
    let server_hello = next_handshake(stream, &mut transcript, &mut pending, &mut records, None, deadline)?;
    if server_hello.is_empty() || server_hello[0] != 2 {
        return Err("TLS server hello was not received".into());
    }
    let body = message_body(&server_hello)?;
    if body.len() < 34 {
        return Err("TLS server hello is truncated".into());
    }
    let server_random = &body[2..34];
    if server_random == HELLO_RETRY_RANDOM {
        let retry = hello_retry_selection(body)?;
        let hash = hash_for_suite(retry.suite)?;
        if retry.group == 0x001d {
            offered_x = clamp_scalar(&crate::state_crypto::random_bytes(32)?);
            offered_xp = x25519(&offered_x, &X25519_BASE);
        } else if retry.group == 0x0017 {
            offered_p = fresh_p256_secret()?;
            offered_pp = p256_public(&offered_p)?;
        } else {
            return Err("TLS server asked for an unsupported group".into());
        }
        transcript = message_hash_prefix(hash, &hello);
        transcript.extend_from_slice(&server_hello);
        let second = client_hello(host, &random, &session, &offered_xp, &offered_pp, &retry.cookie, Some(retry.group))?;
        write_record(stream, 22, &second, None)?;
        if transcript.len().saturating_add(second.len()) > HANDSHAKE_LIMIT {
            return Err("TLS handshake exceeds the memory budget".into());
        }
        transcript.extend_from_slice(&second);
        let again = next_handshake(stream, &mut transcript, &mut pending, &mut records, None, deadline)?;
        return finish_server_hello(stream, host, anchors, &transcript, &again, Some(retry), &offered_x, &offered_p, &mut pending, &mut records, deadline, previous);
    }
    finish_server_hello(stream, host, anchors, &transcript, &server_hello, None, &offered_x, &offered_p, &mut pending, &mut records, deadline, previous)
}

fn fresh_p256_secret() -> Result<[u8; 32], String> {
    for _ in 0..16 {
        let bytes = crate::state_crypto::random_bytes(32)?;
        if let Ok(scalar) = p256_scalar(&bytes) {
            return Ok(scalar);
        }
    }
    Err("TLS elliptic-curve secret was rejected".into())
}

fn message_hash_prefix(hash: HashKind, client_hello: &[u8]) -> Vec<u8> {
    let digest = hash_bytes(hash, client_hello);
    let mut out = vec![0xfe, 0, 0, digest.len() as u8];
    out.extend_from_slice(&digest);
    out
}

fn hash_for_suite(suite: u16) -> Result<HashKind, String> {
    match suite {
        0x1301 | 0x1303 => Ok(HashKind::Sha256),
        0x1302 => Ok(HashKind::Sha384),
        _ => Err("TLS server selected no offered cipher".into()),
    }
}

fn finish_server_hello(stream: &mut TcpStream, host: &str, anchors: &[Certificate], transcript_in: &[u8], server_hello: &[u8], hrr: Option<HelloRetry>, x_secret: &[u8; 32], p_secret: &[u8; 32], pending: &mut Vec<u8>, records: &mut usize, deadline: Instant, previous: Option<Duration>) -> Result<Client, String> {
    let mut transcript = transcript_in.to_vec();
    if server_hello.len() < 4 || server_hello[0] != 2 {
        return Err("TLS server hello was not received".into());
    }
    // next_handshake already appended server_hello. Do not append it again.
    let body = message_body(server_hello)?;
    if body.len() < 34 {
        return Err("TLS server hello is truncated".into());
    }
    let server_random = &body[2..34];
    let (legacy, suite, key_share, version) = parse_server_hello(body)?;
    if version == TLS12_VERSION {
        if downgrade_sentinel(server_random) {
            return Err("TLS server random contains a downgrade marker".into());
        }
        if hrr.is_some() {
            return Err("TLS server hello does not match the hello retry".into());
        }
        if legacy == TLS12_VERSION {
            return handshake_12(stream, host, anchors, &mut transcript, suite, x_secret, p_secret, pending, records, deadline, previous);
        }
        return Err("TLS server selected no supported version".into());
    }
    if version != TLS13_VERSION || legacy != TLS12_VERSION {
        return Err("TLS server selected no supported version".into());
    }
    if let Some(retry) = &hrr {
        if suite != retry.suite || version != retry.version {
            return Err("TLS server hello does not match the hello retry".into());
        }
    }
    let key_share = key_share.ok_or("TLS server sent no key share")?;
    if let Some(retry) = &hrr {
        if key_share.0 != retry.group {
            return Err("TLS server hello does not match the hello retry".into());
        }
    }
    let (kind, hash) = suite_13(suite)?;
    let shared = match key_share.0 {
        0x001d => x25519_shared(x_secret, &key_share_point(&key_share.1, 32)?)?.to_vec(),
        0x0017 => p256_shared(p_secret, &key_share.1)?,
        _ => return Err("TLS server selected no offered group".into()),
    };
    let early = hkdf_extract(hash, &zeros(hash_len(hash)), &zeros(hash_len(hash)));
    let derived = hkdf_expand_label(hash, &early, b"derived", &hash_empty(hash), hash_len(hash));
    let handshake_secret = hkdf_extract(hash, &derived, &shared);
    let client_hs = derive_secret(hash, &handshake_secret, b"c hs traffic", &transcript);
    let server_hs = derive_secret(hash, &handshake_secret, b"s hs traffic", &transcript);
    let mut read_keys = traffic_keys(hash, kind, &server_hs);
    let mut write_keys = traffic_keys(hash, kind, &client_hs);
    let mut messages = Vec::new();
    loop {
        let message = next_handshake(stream, &mut transcript, pending, records, Some(&mut read_keys), deadline)?;
        if message.is_empty() {
            return Err("TLS handshake ended early".into());
        }
        messages.push(message);
        if messages.last().is_some_and(|item| item[0] == 20) {
            break;
        }
        if messages.len() > 8 {
            return Err("TLS handshake did not finish".into());
        }
    }
    let mut extensions = None;
    let mut certificate = None;
    let mut verify = None;
    let mut finished = None;
    for message in &messages {
        match message[0] {
            8 => {
                if extensions.is_some() {
                    return Err("Unexpected TLS handshake message".into());
                }
                extensions = Some(message.clone());
            }
            11 => {
                if certificate.is_some() {
                    return Err("Unexpected TLS handshake message".into());
                }
                certificate = Some(message.clone());
            }
            15 => {
                if verify.is_some() {
                    return Err("Unexpected TLS handshake message".into());
                }
                verify = Some(message.clone());
            }
            20 => {
                if finished.is_some() {
                    return Err("Unexpected TLS handshake message".into());
                }
                finished = Some(message.clone());
            }
            0 => {}
            _ => return Err("Unexpected TLS handshake message".into()),
        }
    }
    let extensions = extensions.ok_or("TLS server sent no extensions")?;
    parse_tls_extension_list(message_body(&extensions)?)?;
    let certificate = certificate.ok_or("TLS server sent no certificate")?;
    let verify = verify.ok_or("TLS server sent no certificate signature")?;
    let finished = finished.ok_or("TLS server sent no handshake finish")?;
    let chain = parse_certificate_13(&certificate)?;
    let leaf = verify_chain(host, anchors, &chain)?;
    // Finished covers every handshake message before itself, including CertificateVerify.
    // CertificateVerify covers the transcript only through Certificate.
    let finished_transcript = transcript_excluding_suffix(&transcript, &finished)?;
    let certificate_transcript = transcript_excluding_suffix(&finished_transcript, &verify)?;
    let signed_hash = hash_bytes(hash, &certificate_transcript);
    verify_certificate_signature_13(&leaf, &verify, &signed_hash)?;
    let expect = finished_verify(hash, &server_hs, &finished_transcript);
    let got = message_body(&finished)?;
    if !equal(got, &expect) {
        return Err("TLS server handshake finish did not match".into());
    }
    let client_verify = finished_verify(hash, &client_hs, &transcript);
    let mut client_finished = vec![20, 0, 0, 0];
    let length = u32::try_from(client_verify.len()).map_err(|_| "TLS handshake finish is too large".to_string())?;
    client_finished[1] = (length >> 16) as u8;
    client_finished[2] = (length >> 8) as u8;
    client_finished[3] = length as u8;
    client_finished.extend_from_slice(&client_verify);
    let (client_app, server_app, _resumption) = tls13_traffic(hash, &handshake_secret, &transcript, &client_finished);
    write_record(stream, 22, &client_finished, Some(&mut write_keys))?;
    let stream = stream.try_clone().map_err(|error| format!("TLS socket: {error}"))?;
    stream.set_read_timeout(previous).ok();
    Ok(Client {
        stream,
        incoming: Vec::new(),
        incoming_at: 0,
        write: traffic_keys(hash, kind, &client_app),
        read: traffic_keys(hash, kind, &server_app),
        read_secret: server_app,
        write_secret: client_app,
        hash,
        tls13: true,
        pending: Vec::new(),
        key_updates: 0,
    })
}

fn tls13_traffic(hash: HashKind, handshake_secret: &[u8], transcript_through_server_finished: &[u8], client_finished: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let master = {
        let derived = hkdf_expand_label(hash, handshake_secret, b"derived", &hash_empty(hash), hash_len(hash));
        hkdf_extract(hash, &derived, &zeros(hash_len(hash)))
    };
    let client_app = derive_secret(hash, &master, b"c ap traffic", transcript_through_server_finished);
    let server_app = derive_secret(hash, &master, b"s ap traffic", transcript_through_server_finished);
    let mut full = transcript_through_server_finished.to_vec();
    full.extend_from_slice(client_finished);
    let resumption = derive_secret(hash, &master, b"res master", &full);
    (client_app, server_app, resumption)
}

fn handshake_12(stream: &mut TcpStream, host: &str, anchors: &[Certificate], transcript: &mut Vec<u8>, suite: u16, x_secret: &[u8; 32], p_secret: &[u8; 32], pending: &mut Vec<u8>, records: &mut usize, deadline: Instant, previous: Option<Duration>) -> Result<Client, String> {
    let (kind, hash) = suite_12(suite)?;
    let mut certificate = None;
    let mut key_exchange = None;
    let mut flight = 0usize;
    loop {
        let message = next_handshake(stream, transcript, pending, records, None, deadline)?;
        if message.is_empty() {
            return Err("TLS 1.2 handshake ended early".into());
        }
        flight += 1;
        if flight > 8 {
            return Err("TLS 1.2 handshake did not finish".into());
        }
        match message[0] {
            11 => certificate = Some(message),
            12 => key_exchange = Some(message),
            14 => break,
            _ => return Err("Unexpected TLS 1.2 handshake message".into()),
        }
    }
    let certificate = certificate.ok_or("TLS server sent no certificate")?;
    let key_exchange = key_exchange.ok_or("TLS server sent no key exchange")?;
    let chain = parse_certificate_12(&certificate)?;
    let leaf = verify_chain(host, anchors, &chain)?;
    let (group, server_public) = verify_server_key_exchange(&leaf, transcript, &key_exchange)?;
    let shared = match group {
        0x001d => {
            let public = key_share_point(&server_public, 32)?;
            x25519_shared(x_secret, &public)?.to_vec()
        }
        0x0017 => p256_shared(p_secret, &server_public)?,
        _ => return Err("TLS server selected no offered group".into()),
    };
    let client_public = if group == 0x001d { x25519(x_secret, &X25519_BASE).to_vec() } else { p256_public(p_secret)? };
    let mut exchange = vec![16, 0, 0, 0];
    let point = if group == 0x001d {
        let mut body = vec![32];
        body.extend_from_slice(&client_public);
        body
    } else {
        let point_len = u8::try_from(client_public.len()).map_err(|_| "TLS key exchange is too large")?;
        let mut body = vec![point_len];
        body.extend_from_slice(&client_public);
        body
    };
    let length = u32::try_from(point.len()).map_err(|_| "TLS key exchange is too large".to_string())?;
    exchange[1] = (length >> 16) as u8;
    exchange[2] = (length >> 8) as u8;
    exchange[3] = length as u8;
    exchange.extend_from_slice(&point);
    write_record(stream, 22, &exchange, None)?;
    transcript.extend_from_slice(&exchange);
    write_record(stream, 20, &[1], None)?;
    let (client_key, server_key, client_iv, server_iv) = key_block_12(hash, &shared, transcript, kind)?;
    let mut client_traffic = Traffic { kind, key: pad_key(&client_key), key_len: client_key.len(), iv: client_iv, seq: 0, legacy: true };
    let mut server_traffic = Traffic { kind, key: pad_key(&server_key), key_len: server_key.len(), iv: server_iv, seq: 0, legacy: true };
    let verify = finished_prf(hash, &client_key_from_master(hash, &shared, transcript)?, transcript)?;
    // The finished message uses the master secret, not the traffic key. Recompute explicitly.
    let _ = verify;
    let master = master_12(hash, &shared, transcript)?;
    let finished_body = prf(hash, &master, b"client finished", &hash_bytes(hash, transcript), 12)?;
    let mut finished = vec![20, 0, 0, 12];
    finished.extend_from_slice(&finished_body);
    write_record(stream, 22, &finished, Some(&mut client_traffic))?;
    transcript.extend_from_slice(&finished);
    // Server change-cipher-spec then encrypted finished.
    *records = records.saturating_add(1);
    if *records > HANDSHAKE_RECORD_BUDGET {
        return Err("TLS handshake exceeds the record budget".into());
    }
    read_cipher_change(stream, deadline)?;
    let server_finished = next_handshake(stream, transcript, pending, records, Some(&mut server_traffic), deadline)?;
    let prior = transcript_excluding_suffix(transcript, &server_finished)?;
    let expect = prf(hash, &master, b"server finished", &hash_bytes(hash, &prior), 12)?;
    let got = message_body(&server_finished)?;
    if !equal(got, &expect) {
        return Err("TLS server handshake finish did not match".into());
    }
    let stream = stream.try_clone().map_err(|error| format!("TLS socket: {error}"))?;
    stream.set_read_timeout(previous).ok();
    Ok(Client {
        stream,
        incoming: Vec::new(),
        incoming_at: 0,
        write: client_traffic,
        read: server_traffic,
        read_secret: Vec::new(),
        write_secret: Vec::new(),
        hash,
        tls13: false,
        pending: Vec::new(),
        key_updates: 0,
    })
}

fn client_key_from_master(hash: HashKind, shared: &[u8], transcript: &[u8]) -> Result<Vec<u8>, String> {
    master_12(hash, shared, transcript)
}

fn master_12(hash: HashKind, shared: &[u8], transcript: &[u8]) -> Result<Vec<u8>, String> {
    let randoms = client_server_random(transcript)?;
    prf(hash, shared, b"master secret", &randoms, 48)
}

fn key_block_12(hash: HashKind, shared: &[u8], transcript: &[u8], kind: CipherKind) -> Result<(Vec<u8>, Vec<u8>, [u8; 12], [u8; 12]), String> {
    let master = master_12(hash, shared, transcript)?;
    let randoms = server_client_random(transcript)?;
    let key_len = if kind == CipherKind::Aes256 { 32 } else { 16 };
    let block = prf(hash, &master, b"key expansion", &randoms, key_len * 2 + 4 * 2)?;
    let client_key = block[..key_len].to_vec();
    let server_key = block[key_len..key_len * 2].to_vec();
    let mut client_iv = [0u8; 12];
    let mut server_iv = [0u8; 12];
    client_iv[..4].copy_from_slice(&block[key_len * 2..key_len * 2 + 4]);
    server_iv[..4].copy_from_slice(&block[key_len * 2 + 4..key_len * 2 + 8]);
    Ok((client_key, server_key, client_iv, server_iv))
}

fn client_server_random(transcript: &[u8]) -> Result<Vec<u8>, String> {
    let client = hello_random(transcript, true)?;
    let server = hello_random(transcript, false)?;
    let mut out = client.to_vec();
    out.extend_from_slice(server);
    Ok(out)
}

fn server_client_random(transcript: &[u8]) -> Result<Vec<u8>, String> {
    let client = hello_random(transcript, true)?;
    let server = hello_random(transcript, false)?;
    let mut out = server.to_vec();
    out.extend_from_slice(client);
    Ok(out)
}

fn hello_random<'a>(transcript: &'a [u8], client: bool) -> Result<&'a [u8], String> {
    let mut at = 0usize;
    let mut seen = 0usize;
    while at + 4 <= transcript.len() {
        seen += 1;
        if seen > 64 {
            return Err("TLS transcript is too long".into());
        }
        let kind = transcript[at];
        let length = u32::from_be_bytes([0, transcript[at + 1], transcript[at + 2], transcript[at + 3]]) as usize;
        let start = at.checked_add(4).ok_or("TLS transcript is truncated")?;
        let end = start.checked_add(length).ok_or("TLS transcript is truncated")?;
        let body = transcript.get(start..end).ok_or("TLS transcript is truncated")?;
        if (client && kind == 1) || (!client && kind == 2) {
            if body.len() < 34 {
                return Err("TLS hello is truncated".into());
            }
            return Ok(&body[2..34]);
        }
        at = end;
    }
    Err("TLS random was not in the transcript".into())
}

fn finished_prf(hash: HashKind, master: &[u8], transcript: &[u8]) -> Result<Vec<u8>, String> {
    prf(hash, master, b"client finished", &hash_bytes(hash, transcript), hash_len(hash))
}

fn prf(hash: HashKind, secret: &[u8], label: &[u8], seed: &[u8], length: usize) -> Result<Vec<u8>, String> {
    if length > 1024 {
        return Err("TLS key material is too large".into());
    }
    let mut material = label.to_vec();
    material.extend_from_slice(seed);
    let mut out = Vec::new();
    let mut a = material.clone();
    let mut rounds = 0usize;
    while out.len() < length {
        rounds += 1;
        if rounds > 64 {
            return Err("TLS key material is too large".into());
        }
        a = hmac(hash, secret, &a)?;
        let mut input = a.clone();
        input.extend_from_slice(&material);
        out.extend_from_slice(&hmac(hash, secret, &input)?);
    }
    out.truncate(length);
    Ok(out)
}

fn suite_13(suite: u16) -> Result<(CipherKind, HashKind), String> {
    match suite {
        0x1301 => Ok((CipherKind::Aes128, HashKind::Sha256)),
        0x1302 => Ok((CipherKind::Aes256, HashKind::Sha384)),
        0x1303 => Ok((CipherKind::ChaCha, HashKind::Sha256)),
        _ => Err("TLS server selected no offered cipher".into()),
    }
}

fn suite_12(suite: u16) -> Result<(CipherKind, HashKind), String> {
    match suite {
        0xc02f | 0xc02b => Ok((CipherKind::Aes128, HashKind::Sha256)),
        0xc030 | 0xc02c => Ok((CipherKind::Aes256, HashKind::Sha384)),
        _ => Err("TLS server selected no offered cipher".into()),
    }
}

fn downgrade_sentinel(random: &[u8]) -> bool {
    if random.len() < 32 {
        return false;
    }
    let tail = &random[24..32];
    tail == b"DOWNGRD\x01" || tail == b"DOWNGRD\x00"
}

fn server_hello_suite(body: &[u8]) -> Result<u16, String> {
    let (_, suite, _, _) = parse_server_hello(body)?;
    Ok(suite)
}

fn parse_server_hello(body: &[u8]) -> Result<(u16, u16, Option<(u16, Vec<u8>)>, u16), String> {
    if body.len() < 35 {
        return Err("TLS server hello is truncated".into());
    }
    let legacy = u16::from_be_bytes([body[0], body[1]]);
    let mut at = 34usize;
    let session = *body.get(at).ok_or("TLS server hello is truncated")? as usize;
    at = at.checked_add(1 + session).ok_or("TLS server hello is truncated")?;
    if at.checked_add(3).ok_or("TLS server hello is truncated")? > body.len() {
        return Err("TLS server hello is truncated".into());
    }
    let suite = u16::from_be_bytes([body[at], body[at + 1]]);
    at += 3;
    if at == body.len() {
        return Ok((legacy, suite, None, legacy));
    }
    if at + 2 > body.len() {
        return Err("TLS server hello is truncated".into());
    }
    let ext_len = u16::from_be_bytes([body[at], body[at + 1]]) as usize;
    at += 2;
    let end = at.checked_add(ext_len).ok_or("TLS server hello is truncated")?;
    if end > body.len() {
        return Err("TLS server hello is truncated".into());
    }
    let mut version = legacy;
    let mut share = None;
    let mut seen = Vec::new();
    let mut extensions = 0usize;
    while at + 4 <= end {
        extensions += 1;
        if extensions > EXTENSION_LIMIT {
            return Err("TLS server hello has too many extensions".into());
        }
        let kind = u16::from_be_bytes([body[at], body[at + 1]]);
        let len = u16::from_be_bytes([body[at + 2], body[at + 3]]) as usize;
        note_server_extension(&mut seen, kind)?;
        let data_at = at.checked_add(4).ok_or("TLS extension is truncated")?;
        let data_end = data_at.checked_add(len).ok_or("TLS extension is truncated")?;
        let data = body.get(data_at..data_end).ok_or("TLS extension is truncated")?;
        if kind == 43 {
            if data.len() != 2 {
                return Err("TLS server selected no supported version".into());
            }
            version = u16::from_be_bytes([data[0], data[1]]);
        }
        if kind == 51 {
            if data.len() == 2 {
                // A hello retry names the group. The final server hello carries the key share.
            } else if data.len() >= 4 {
                let group = u16::from_be_bytes([data[0], data[1]]);
                let point_len = u16::from_be_bytes([data[2], data[3]]) as usize;
                let point_end = 4usize.checked_add(point_len).ok_or("TLS key share is truncated")?;
                if point_end != data.len() {
                    return Err("TLS key share is truncated".into());
                }
                share = Some((group, data[4..point_end].to_vec()));
            } else {
                return Err("TLS key share is truncated".into());
            }
        }
        at = data_end;
    }
    if at != end {
        return Err("TLS server hello is truncated".into());
    }
    Ok((legacy, suite, share, version))
}

fn hello_retry_cookie(body: &[u8]) -> Result<Vec<u8>, String> {
    extension_bytes(body, 44).unwrap_or(Ok(Vec::new()))
}

fn hello_retry_group(body: &[u8]) -> Result<u16, String> {
    let data = extension_bytes(body, 51).ok_or_else(|| "TLS retry did not name a group".to_string())??;
    if data.len() != 2 {
        return Err("TLS retry did not name a group".into());
    }
    Ok(u16::from_be_bytes([data[0], data[1]]))
}

fn hello_retry_selection(body: &[u8]) -> Result<HelloRetry, String> {
    let cookie = hello_retry_cookie(body)?;
    let group = hello_retry_group(body)?;
    if group != 0x001d && group != 0x0017 {
        return Err("TLS server asked for an unsupported group".into());
    }
    let (legacy, suite, share, version) = parse_server_hello(body)?;
    if legacy != TLS12_VERSION || version != TLS13_VERSION {
        return Err("TLS server selected no supported version".into());
    }
    if share.is_some() {
        return Err("TLS hello retry is not valid".into());
    }
    hash_for_suite(suite)?;
    Ok(HelloRetry { suite, version, group, cookie })
}

fn note_server_extension(seen: &mut Vec<u16>, kind: u16) -> Result<(), String> {
    if seen.contains(&kind) {
        return Err("TLS server hello repeats an extension".into());
    }
    seen.push(kind);
    Ok(())
}

fn extension_bytes(body: &[u8], want: u16) -> Option<Result<Vec<u8>, String>> {
    if body.len() < 35 {
        return Some(Err("TLS server hello is truncated".into()));
    }
    let session = body[34] as usize;
    let mut at = match 35usize.checked_add(session).and_then(|value| value.checked_add(3)) {
        Some(value) => value,
        None => return Some(Err("TLS server hello is truncated".into())),
    };
    if at + 2 > body.len() {
        return None;
    }
    let ext_len = u16::from_be_bytes([body[at], body[at + 1]]) as usize;
    at += 2;
    let end = match at.checked_add(ext_len) {
        Some(value) if value <= body.len() => value,
        _ => return Some(Err("TLS server hello is truncated".into())),
    };
    let mut seen_count = 0usize;
    let mut seen = Vec::new();
    let mut found = None;
    while at + 4 <= end {
        seen_count += 1;
        if seen_count > EXTENSION_LIMIT {
            return Some(Err("TLS server hello has too many extensions".into()));
        }
        let kind = u16::from_be_bytes([body[at], body[at + 1]]);
        let len = u16::from_be_bytes([body[at + 2], body[at + 3]]) as usize;
        if let Err(error) = note_server_extension(&mut seen, kind) {
            return Some(Err(error));
        }
        let data_at = match at.checked_add(4) {
            Some(value) => value,
            None => return Some(Err("TLS extension is truncated".into())),
        };
        let data_end = match data_at.checked_add(len) {
            Some(value) => value,
            None => return Some(Err("TLS extension is truncated".into())),
        };
        let data = match body.get(data_at..data_end) {
            Some(data) => data,
            None => return Some(Err("TLS extension is truncated".into())),
        };
        if kind == want {
            found = Some(data.to_vec());
        }
        at = data_end;
    }
    if at != end {
        return Some(Err("TLS server hello is truncated".into()));
    }
    found.map(Ok)
}

fn host_for_server_name(host: &str) -> Option<&str> {
    let bare = host.trim_matches(|byte| byte == '[' || byte == ']');
    if bare.is_empty() || parse_ip(bare).is_some() {
        return None;
    }
    Some(host)
}

fn client_hello(host: &str, random: &[u8; 32], session: &[u8; 32], x_public: &[u8; 32], p_public: &[u8], cookie: &[u8], only_group: Option<u16>) -> Result<Vec<u8>, String> {
    let mut extensions = Vec::new();
    if let Some(name) = host_for_server_name(host) {
        ext(&mut extensions, 0, name.as_bytes())?;
    }
    let versions = [4u8, (TLS13_VERSION >> 8) as u8, (TLS13_VERSION & 0xff) as u8, (TLS12_VERSION >> 8) as u8, (TLS12_VERSION & 0xff) as u8];
    ext(&mut extensions, 43, &versions)?;
    let groups = [4u8, 0x00, 0x1d, 0x00, 0x17];
    ext(&mut extensions, 10, &groups)?;
    let mut sigs = Vec::new();
    for algo in [0x0403u16, 0x0804, 0x0805, 0x0401, 0x0501, 0x0601] {
        sigs.extend_from_slice(&algo.to_be_bytes());
    }
    let mut sig_body = (sigs.len() as u16).to_be_bytes().to_vec();
    sig_body.extend_from_slice(&sigs);
    ext(&mut extensions, 13, &sig_body)?;
    let mut shares = Vec::new();
    let emit_x = only_group.is_none_or(|group| group == 0x001d);
    let emit_p = only_group.is_none_or(|group| group == 0x0017);
    if !emit_x && !emit_p {
        return Err("TLS server asked for an unsupported group".into());
    }
    if emit_x {
        shares.extend_from_slice(&0x001du16.to_be_bytes());
        shares.extend_from_slice(&32u16.to_be_bytes());
        shares.extend_from_slice(x_public);
    }
    if emit_p {
        let point_len = u16::try_from(p_public.len()).map_err(|_| "TLS key share is too large")?;
        shares.extend_from_slice(&0x0017u16.to_be_bytes());
        shares.extend_from_slice(&point_len.to_be_bytes());
        shares.extend_from_slice(p_public);
    }
    let share_len = u16::try_from(shares.len()).map_err(|_| "TLS key share is too large")?;
    let mut share_body = share_len.to_be_bytes().to_vec();
    share_body.extend_from_slice(&shares);
    ext(&mut extensions, 51, &share_body)?;
    if !cookie.is_empty() {
        ext(&mut extensions, 44, cookie)?;
    }
    let mut suites = Vec::new();
    for suite in [0x1301u16, 0x1302, 0x1303, 0xc02f, 0xc02b, 0xc030, 0xc02c] {
        suites.extend_from_slice(&suite.to_be_bytes());
    }
    let mut body = Vec::new();
    body.extend_from_slice(&0x0303u16.to_be_bytes());
    body.extend_from_slice(random);
    body.push(u8::try_from(session.len()).map_err(|_| "TLS client hello is too large")?);
    body.extend_from_slice(session);
    body.extend_from_slice(&u16::try_from(suites.len()).map_err(|_| "TLS client hello is too large")?.to_be_bytes());
    body.extend_from_slice(&suites);
    body.extend_from_slice(&[1, 0]);
    body.extend_from_slice(&u16::try_from(extensions.len()).map_err(|_| "TLS client hello is too large")?.to_be_bytes());
    body.extend_from_slice(&extensions);
    let mut message = vec![1, 0, 0, 0];
    let length = u32::try_from(body.len()).map_err(|_| "TLS client hello is too large")?;
    message[1] = (length >> 16) as u8;
    message[2] = (length >> 8) as u8;
    message[3] = length as u8;
    message.extend_from_slice(&body);
    Ok(message)
}

fn ext(out: &mut Vec<u8>, kind: u16, body: &[u8]) -> Result<(), String> {
    if kind == 0 {
        let name_len = u16::try_from(body.len()).map_err(|_| "TLS extension is too large")?;
        let mut named = vec![0];
        named.extend_from_slice(&name_len.to_be_bytes());
        named.extend_from_slice(body);
        let list_len = u16::try_from(named.len()).map_err(|_| "TLS extension is too large")?;
        let mut list = list_len.to_be_bytes().to_vec();
        list.extend_from_slice(&named);
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&u16::try_from(list.len()).map_err(|_| "TLS extension is too large")?.to_be_bytes());
        out.extend_from_slice(&list);
        return Ok(());
    }
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&u16::try_from(body.len()).map_err(|_| "TLS extension is too large")?.to_be_bytes());
    out.extend_from_slice(body);
    Ok(())
}

fn next_handshake(stream: &mut TcpStream, transcript: &mut Vec<u8>, pending: &mut Vec<u8>, records: &mut usize, mut keys: Option<&mut Traffic>, deadline: Instant) -> Result<Vec<u8>, String> {
    loop {
        if Instant::now() >= deadline {
            return Err("TLS handshake timed out".into());
        }
        if let Some(message) = take_handshake(pending)? {
            if transcript.len().saturating_add(message.len()) > HANDSHAKE_LIMIT {
                return Err("TLS handshake exceeds the memory budget".into());
            }
            transcript.extend_from_slice(&message);
            return Ok(message);
        }
        if *records >= HANDSHAKE_RECORD_BUDGET {
            return Err("TLS handshake exceeds the record budget".into());
        }
        let (kind, payload) = read_record(stream, keys.as_deref_mut(), Some(deadline))?;
        *records += 1;
        if kind == 21 {
            return Err(alert_text(&payload));
        }
        if kind == 20 {
            require_change_cipher_spec(&payload)?;
            continue;
        }
        if kind != 22 {
            return Err("TLS handshake record was expected".into());
        }
        if payload.len() > RECORD_LIMIT || pending.len().saturating_add(payload.len()) > HANDSHAKE_LIMIT {
            return Err("TLS handshake exceeds the memory budget".into());
        }
        pending.extend_from_slice(&payload);
    }
}

fn take_handshake(pending: &mut Vec<u8>) -> Result<Option<Vec<u8>>, String> {
    if pending.len() < 4 {
        return Ok(None);
    }
    let length = u32::from_be_bytes([0, pending[1], pending[2], pending[3]]) as usize;
    if length > HANDSHAKE_LIMIT {
        return Err("TLS handshake message is too large".into());
    }
    let total = 4usize.checked_add(length).ok_or("TLS handshake message is too large")?;
    if pending.len() < total {
        return Ok(None);
    }
    let message = pending[..total].to_vec();
    pending.drain(..total);
    Ok(Some(message))
}

fn transcript_excluding_suffix(transcript: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    if message.is_empty() || !transcript.ends_with(message) {
        return Err("TLS transcript does not end with the expected handshake message".into());
    }
    Ok(transcript[..transcript.len() - message.len()].to_vec())
}

fn u16_len(len: usize) -> Result<u16, String> {
    u16::try_from(len).map_err(|_| "TLS record exceeds the memory budget".into())
}

fn reject_record_seq_wrap(keys: &Traffic) -> Result<(), String> {
    if keys.seq == u64::MAX {
        return Err("TLS record sequence would wrap under the same keys".into());
    }
    Ok(())
}

fn write_record(stream: &mut TcpStream, kind: u8, payload: &[u8], keys: Option<&mut Traffic>) -> Result<(), String> {
    if payload.len() > RECORD_LIMIT {
        return Err("TLS record exceeds the memory budget".into());
    }
    if let Some(keys) = keys {
        reject_record_seq_wrap(keys)?;
        let sealed = if keys.legacy { seal_legacy(keys, kind, payload)? } else { seal(keys, kind, payload)? };
        keys.seq += 1;
        stream.write_all(&sealed).map_err(|error| format!("TLS write: {error}"))?;
    } else {
        let len = u16::try_from(payload.len()).map_err(|_| "TLS record exceeds the memory budget")?;
        let mut record = Vec::new();
        record.push(kind);
        record.extend_from_slice(&0x0303u16.to_be_bytes());
        record.extend_from_slice(&len.to_be_bytes());
        record.extend_from_slice(payload);
        stream.write_all(&record).map_err(|error| format!("TLS write: {error}"))?;
    }
    stream.flush().ok();
    Ok(())
}

fn read_record(stream: &mut TcpStream, keys: Option<&mut Traffic>, deadline: Option<Instant>) -> Result<(u8, Vec<u8>), String> {
    if let Some(keys) = keys.as_ref() {
        reject_record_seq_wrap(keys)?;
    }
    let mut header = [0u8; 5];
    read_exact(stream, &mut header, deadline)?;
    let kind = header[0];
    let len = u16::from_be_bytes([header[3], header[4]]) as usize;
    if len > RECORD_LIMIT + 256 {
        return Err("TLS record exceeds the memory budget".into());
    }
    // Ciphertext may be 256 bytes over the fragment cap. Plaintext, including an
    // unencrypted change-cipher-spec, stops at the fragment cap.
    let unencrypted = match keys.as_ref() {
        None => true,
        Some(keys) => !keys.legacy && kind == 20,
    };
    if unencrypted && len > RECORD_LIMIT {
        return Err("TLS record exceeds the memory budget".into());
    }
    let mut payload = vec![0u8; len];
    read_exact(stream, &mut payload, deadline)?;
    if let Some(keys) = keys {
        // A compatibility change-cipher-spec is the single unencrypted byte 0x01.
        if !keys.legacy && kind == 20 {
            require_change_cipher_spec(&payload)?;
            return Ok((20, payload));
        }
        let plain = if keys.legacy {
            open_legacy(keys, &header, &payload)?
        } else {
            if kind != 23 {
                return Err("TLS encrypted record was expected".into());
            }
            open(keys, &header, &payload)?
        };
        keys.seq += 1;
        if keys.legacy {
            if plain.len() > RECORD_LIMIT {
                return Err("TLS record exceeds the memory budget".into());
            }
            return Ok((kind, plain));
        }
        if plain.len() > RECORD_LIMIT + 1 {
            return Err("TLS record exceeds the memory budget".into());
        }
        let (inner, body) = tls13_content(&plain)?;
        if body.len() > RECORD_LIMIT {
            return Err("TLS record exceeds the memory budget".into());
        }
        if inner == 20 {
            return Err("TLS protected change cipher spec is not valid".into());
        }
        return Ok((inner, body));
    }
    if payload.len() > RECORD_LIMIT {
        return Err("TLS record exceeds the memory budget".into());
    }
    Ok((kind, payload))
}

fn require_change_cipher_spec(payload: &[u8]) -> Result<(), String> {
    if payload == [1] {
        Ok(())
    } else {
        Err("TLS change cipher spec is not valid".into())
    }
}

fn read_cipher_change(stream: &mut TcpStream, deadline: Instant) -> Result<(), String> {
    let ccs = read_record(stream, None, Some(deadline))?;
    if ccs.0 == 21 {
        return Err(alert_text(&ccs.1));
    }
    if ccs.0 != 20 {
        return Err("TLS server did not switch ciphers".into());
    }
    require_change_cipher_spec(&ccs.1)
}

fn tls13_content(plain: &[u8]) -> Result<(u8, Vec<u8>), String> {
    // Inner plaintext is content, content type, and padding, and stops at 2^14 + 1.
    if plain.len() > RECORD_LIMIT + 1 {
        return Err("TLS record exceeds the memory budget".into());
    }
    let mut end = plain.len();
    while end > 0 && plain[end - 1] == 0 {
        end -= 1;
    }
    if end == 0 {
        return Err("TLS record has no content type".into());
    }
    let body = &plain[..end - 1];
    if body.len() > RECORD_LIMIT {
        return Err("TLS record exceeds the memory budget".into());
    }
    Ok((plain[end - 1], body.to_vec()))
}

fn seal_legacy(keys: &Traffic, kind: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&keys.iv[..4]);
    nonce[4..].copy_from_slice(&keys.seq.to_be_bytes());
    let mut aad = keys.seq.to_be_bytes().to_vec();
    let plain_len = u16_len(payload.len())?;
    let header = [kind, 3, 3, (plain_len >> 8) as u8, (plain_len & 0xff) as u8];
    aad.extend_from_slice(&header);
    let encrypted = aead_seal(keys, &nonce, &aad, payload)?;
    let length = u16_len(8usize.checked_add(encrypted.len()).ok_or("TLS record exceeds the memory budget")?)?;
    let mut record = vec![kind, 3, 3, (length >> 8) as u8, (length & 0xff) as u8];
    record.extend_from_slice(&keys.seq.to_be_bytes());
    record.extend_from_slice(&encrypted);
    Ok(record)
}

fn open_legacy(keys: &Traffic, header: &[u8], payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.len() < 8 + 16 {
        return Err("TLS record is truncated".into());
    }
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&keys.iv[..4]);
    nonce[4..].copy_from_slice(&payload[..8]);
    let plain_len = u16_len(payload.len() - 8 - 16)?;
    let mut aad = keys.seq.to_be_bytes().to_vec();
    aad.extend_from_slice(&[header[0], header[1], header[2], (plain_len >> 8) as u8, (plain_len & 0xff) as u8]);
    aead_open(keys, &nonce, &aad, &payload[8..])
}

fn read_exact(stream: &mut TcpStream, buf: &mut [u8], deadline: Option<Instant>) -> Result<(), String> {
    let mut at = 0;
    while at < buf.len() {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("TLS handshake timed out".into());
        }
        match stream.read(&mut buf[at..]) {
            Ok(0) => return Err("TLS peer closed during the handshake".into()),
            Ok(read) => at += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::TimedOut || error.kind() == io::ErrorKind::WouldBlock => {
                return Err("TLS handshake timed out".into());
            }
            Err(error) => return Err(format!("TLS read: {error}")),
        }
    }
    Ok(())
}

fn alert_description(payload: &[u8]) -> Result<u8, String> {
    // An alert is exactly two bytes: level warning(1) or fatal(2), then the description.
    if payload.len() != 2 || (payload[0] != 1 && payload[0] != 2) {
        return Err("TLS peer sent a malformed alert".into());
    }
    Ok(payload[1])
}

fn alert_text(payload: &[u8]) -> String {
    match alert_description(payload) {
        Ok(code) => format!("TLS peer sent alert {code}"),
        Err(error) => error,
    }
}

fn seal(keys: &Traffic, kind: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut plain = payload.to_vec();
    plain.push(kind);
    let mut nonce = keys.iv;
    let seq = keys.seq.to_be_bytes();
    for (slot, byte) in nonce[4..].iter_mut().zip(seq) {
        *slot ^= byte;
    }
    let length = u16_len(plain.len().checked_add(16).ok_or("TLS record exceeds the memory budget")?)?;
    let header = [23, 3, 3, (length >> 8) as u8, (length & 0xff) as u8];
    let encrypted = aead_seal(keys, &nonce, &header, &plain)?;
    let mut record = header.to_vec();
    record.extend_from_slice(&encrypted);
    Ok(record)
}

fn open(keys: &Traffic, header: &[u8], payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.len() < 16 {
        return Err("TLS record is truncated".into());
    }
    let mut nonce = keys.iv;
    let seq = keys.seq.to_be_bytes();
    for (slot, byte) in nonce[4..].iter_mut().zip(seq) {
        *slot ^= byte;
    }
    aead_open(keys, &nonce, header, payload)
}

fn aead_seal(keys: &Traffic, nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String> {
    match keys.kind {
        CipherKind::Aes128 | CipherKind::Aes256 => aes_gcm_seal(&keys.key[..keys.key_len], nonce, aad, plain),
        CipherKind::ChaCha => chacha_seal(&keys.key, nonce, aad, plain),
    }
}

fn aead_open(keys: &Traffic, nonce: &[u8; 12], aad: &[u8], body: &[u8]) -> Result<Vec<u8>, String> {
    match keys.kind {
        CipherKind::Aes128 | CipherKind::Aes256 => aes_gcm_open(&keys.key[..keys.key_len], nonce, aad, body),
        CipherKind::ChaCha => chacha_open(&keys.key, nonce, aad, body),
    }
}

impl Read for Client {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.incoming_at < self.incoming.len() {
            let n = (self.incoming.len() - self.incoming_at).min(buf.len());
            buf[..n].copy_from_slice(&self.incoming[self.incoming_at..self.incoming_at + n]);
            self.incoming_at += n;
            if self.incoming_at == self.incoming.len() {
                self.incoming.clear();
                self.incoming_at = 0;
            }
            return Ok(n);
        }
        let mut skipped = 0usize;
        loop {
            let (kind, payload) = read_record(&mut self.stream, Some(&mut self.read), None).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if kind == 21 {
                match alert_description(&payload) {
                    Ok(0) => return Ok(0),
                    Ok(code) => return Err(io::Error::new(io::ErrorKind::ConnectionAborted, format!("TLS peer sent alert {code}"))),
                    Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
                }
            }
            if kind == 20 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS change cipher spec is not expected"));
            }
            if kind == 22 && self.tls13 {
                skipped += 1;
                if skipped > 8 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS record was not application data"));
                }
                self.accept_post_handshake(&payload)?;
                continue;
            }
            if kind == 23 {
                if !self.pending.is_empty() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS handshake message is incomplete"));
                }
                if payload.is_empty() {
                    skipped += 1;
                    if skipped > 8 {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS record was not application data"));
                    }
                    continue;
                }
                if payload.len() > RECORD_LIMIT {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS record exceeds the memory budget"));
                }
                self.incoming = payload;
                self.incoming_at = 0;
                return self.read(buf);
            }
            skipped += 1;
            if skipped > 8 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS record was not application data"));
            }
        }
    }
}

impl Client {
    fn accept_post_handshake(&mut self, payload: &[u8]) -> io::Result<()> {
        if payload.len() > RECORD_LIMIT || self.pending.len().saturating_add(payload.len()) > HANDSHAKE_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS handshake exceeds the memory budget"));
        }
        self.pending.extend_from_slice(payload);
        let mut handled = 0usize;
        loop {
            if self.pending.len() < 4 {
                return Ok(());
            }
            let length = u32::from_be_bytes([0, self.pending[1], self.pending[2], self.pending[3]]) as usize;
            if length > HANDSHAKE_LIMIT {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS handshake message is too large"));
            }
            let total = 4usize.checked_add(length).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TLS handshake message is too large"))?;
            if self.pending.len() < total {
                return Ok(());
            }
            handled += 1;
            if handled > 8 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS handshake exceeds the record budget"));
            }
            let message: Vec<u8> = self.pending.drain(..total).collect();
            self.dispatch_post_handshake(&message)?;
        }
    }

    fn dispatch_post_handshake(&mut self, message: &[u8]) -> io::Result<()> {
        match message.first().copied().unwrap_or(255) {
            4 => Ok(()),
            24 => self.apply_key_update(message),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected TLS handshake message")),
        }
    }

    fn apply_key_update(&mut self, message: &[u8]) -> io::Result<()> {
        let body = message_body(message).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if body.len() != 1 || body[0] > 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS key update is not valid"));
        }
        if self.key_updates >= 256 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "TLS key update limit reached"));
        }
        self.key_updates += 1;
        self.rotate_read().map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if body[0] == 1 {
            let reply = [24u8, 0, 0, 1, 0];
            write_record(&mut self.stream, 22, &reply, Some(&mut self.write)).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            self.rotate_write().map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        Ok(())
    }

    fn rotate_read(&mut self) -> Result<(), String> {
        let next = next_traffic_secret(self.hash, &self.read_secret)?;
        self.read = traffic_keys(self.hash, self.read.kind, &next);
        self.read_secret = next;
        Ok(())
    }

    fn rotate_write(&mut self) -> Result<(), String> {
        let next = next_traffic_secret(self.hash, &self.write_secret)?;
        self.write = traffic_keys(self.hash, self.write.kind, &next);
        self.write_secret = next;
        Ok(())
    }
}

fn next_traffic_secret(hash: HashKind, secret: &[u8]) -> Result<Vec<u8>, String> {
    if secret.len() != hash_len(hash) {
        return Err("TLS key update is not valid".into());
    }
    Ok(hkdf_expand_label(hash, secret, b"traffic upd", b"", hash_len(hash)))
}

impl Write for Client {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut at = 0;
        while at < buf.len() {
            let end = at.saturating_add(RECORD_LIMIT).min(buf.len());
            write_record(&mut self.stream, 23, &buf[at..end], Some(&mut self.write)).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            at = end;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn traffic_keys(hash: HashKind, kind: CipherKind, secret: &[u8]) -> Traffic {
    let key_len = match kind {
        CipherKind::Aes128 => 16,
        CipherKind::Aes256 | CipherKind::ChaCha => 32,
    };
    let key_bytes = hkdf_expand_label(hash, secret, b"key", b"", key_len);
    let iv_bytes = hkdf_expand_label(hash, secret, b"iv", b"", 12);
    let mut key = [0u8; 32];
    key[..key_bytes.len()].copy_from_slice(&key_bytes);
    let mut iv = [0u8; 12];
    iv.copy_from_slice(&iv_bytes);
    Traffic { kind, key, key_len, iv, seq: 0, legacy: false }
}

fn pad_key(key: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..key.len()].copy_from_slice(key);
    out
}

fn derive_secret(hash: HashKind, secret: &[u8], label: &[u8], transcript: &[u8]) -> Vec<u8> {
    hkdf_expand_label(hash, secret, label, &hash_bytes(hash, transcript), hash_len(hash))
}

fn finished_verify(hash: HashKind, secret: &[u8], transcript: &[u8]) -> Vec<u8> {
    let key = hkdf_expand_label(hash, secret, b"finished", b"", hash_len(hash));
    hmac(hash, &key, &hash_bytes(hash, transcript)).unwrap_or_default()
}

fn hkdf_extract(hash: HashKind, salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    hmac(hash, salt, ikm).unwrap_or_default()
}

fn hkdf_expand_label(hash: HashKind, secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Vec<u8> {
    let mut hkdf_label = Vec::new();
    hkdf_label.extend_from_slice(&(length as u16).to_be_bytes());
    let full = {
        let mut name = b"tls13 ".to_vec();
        name.extend_from_slice(label);
        name
    };
    hkdf_label.push(full.len() as u8);
    hkdf_label.extend_from_slice(&full);
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);
    hkdf_expand(hash, secret, &hkdf_label, length)
}

fn hkdf_expand(hash: HashKind, secret: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    if length == 0 || length > 1024 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut previous = Vec::new();
    let mut counter = 1u8;
    while out.len() < length && counter != 0 {
        let mut input = previous.clone();
        input.extend_from_slice(info);
        input.push(counter);
        previous = hmac(hash, secret, &input).unwrap_or_default();
        if previous.is_empty() {
            break;
        }
        out.extend_from_slice(&previous);
        counter = counter.wrapping_add(1);
    }
    out.truncate(length);
    out
}

fn zeros(n: usize) -> Vec<u8> {
    vec![0u8; n]
}

fn hash_empty(hash: HashKind) -> Vec<u8> {
    hash_bytes(hash, b"")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HashKind {
    Sha256,
    Sha384,
    Sha512,
}

fn hash_len(hash: HashKind) -> usize {
    match hash {
        HashKind::Sha256 => 32,
        HashKind::Sha384 => 48,
        HashKind::Sha512 => 64,
    }
}

fn hash_bytes(hash: HashKind, data: &[u8]) -> Vec<u8> {
    match hash {
        HashKind::Sha256 => sha256(data).to_vec(),
        HashKind::Sha384 => sha384(data).to_vec(),
        HashKind::Sha512 => sha512(data, false).to_vec(),
    }
}

fn hmac(hash: HashKind, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let block = match hash {
        HashKind::Sha256 => 64,
        HashKind::Sha384 | HashKind::Sha512 => 128,
    };
    let mut key_block = if key.len() > block { hash_bytes(hash, key) } else { key.to_vec() };
    key_block.resize(block, 0);
    let mut inner = vec![0x36u8; block];
    let mut outer = vec![0x5cu8; block];
    for index in 0..block {
        inner[index] ^= key_block[index];
        outer[index] ^= key_block[index];
    }
    inner.extend_from_slice(data);
    outer.extend_from_slice(&hash_bytes(hash, &inner));
    Ok(hash_bytes(hash, &outer))
}

fn sha256(message: &[u8]) -> [u8; 32] {
    let mut state = [0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let blocks = padded(message, 64, true);
    for block in blocks.chunks(64) {
        let mut words = [0u32; 64];
        for index in 0..16 {
            words[index] = u32::from_be_bytes(block[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut a = state;
        for index in 0..64 {
            let s1 = a[4].rotate_right(6) ^ a[4].rotate_right(11) ^ a[4].rotate_right(25);
            let ch = (a[4] & a[5]) ^ (!a[4] & a[6]);
            let t1 = a[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[index]).wrapping_add(words[index]);
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
        for index in 0..8 {
            state[index] = state[index].wrapping_add(a[index]);
        }
    }
    let mut out = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn sha384(message: &[u8]) -> [u8; 48] {
    let full = sha512(message, true);
    let mut out = [0u8; 48];
    out.copy_from_slice(&full[..48]);
    out
}

fn sha512(message: &[u8], sha384_iv: bool) -> [u8; 64] {
    let mut state = if sha384_iv {
        [0xcbbb9d5dc1059ed8u64, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939, 0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4]
    } else {
        [0x6a09e667f3bcc908u64, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1, 0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179]
    };
    let blocks = padded(message, 128, false);
    for block in blocks.chunks(128) {
        let mut words = [0u64; 80];
        for index in 0..16 {
            words[index] = u64::from_be_bytes(block[index * 8..index * 8 + 8].try_into().unwrap());
        }
        for index in 16..80 {
            let s0 = words[index - 15].rotate_right(1) ^ words[index - 15].rotate_right(8) ^ (words[index - 15] >> 7);
            let s1 = words[index - 2].rotate_right(19) ^ words[index - 2].rotate_right(61) ^ (words[index - 2] >> 6);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut a = state;
        for index in 0..80 {
            let s1 = a[4].rotate_right(14) ^ a[4].rotate_right(18) ^ a[4].rotate_right(41);
            let ch = (a[4] & a[5]) ^ (!a[4] & a[6]);
            let t1 = a[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA512_K[index]).wrapping_add(words[index]);
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
        for index in 0..8 {
            state[index] = state[index].wrapping_add(a[index]);
        }
    }
    let mut out = [0u8; 64];
    for (index, word) in state.iter().enumerate() {
        out[index * 8..index * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn padded(message: &[u8], block: usize, bits32: bool) -> Vec<u8> {
    let mut out = message.to_vec();
    out.push(0x80);
    let length_size = if bits32 { 8 } else { 16 };
    while (out.len() + length_size) % block != 0 {
        out.push(0);
    }
    let bits = (message.len() as u128) * 8;
    if !bits32 {
        out.extend_from_slice(&0u64.to_be_bytes());
    }
    out.extend_from_slice(&(bits as u64).to_be_bytes());
    out
}

fn aes_encrypt_block(key: &[u8], block: &[u8; 16]) -> Result<[u8; 16], String> {
    let rounds = match key.len() {
        16 => 10,
        32 => 14,
        _ => return Err("Unsupported AES key".into()),
    };
    let expanded = expand_key(key, rounds)?;
    let mut state = *block;
    add_round(&mut state, &expanded[0..16]);
    for round in 1..rounds {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        add_round(&mut state, &expanded[round * 16..round * 16 + 16]);
    }
    sub_bytes(&mut state);
    shift_rows(&mut state);
    add_round(&mut state, &expanded[rounds * 16..rounds * 16 + 16]);
    Ok(state)
}

fn expand_key(key: &[u8], rounds: usize) -> Result<Vec<u8>, String> {
    let nk = key.len() / 4;
    let words = 4 * (rounds + 1);
    let mut out = key.to_vec();
    let mut index = nk;
    while index < words {
        let mut temp = [out[(index - 1) * 4], out[(index - 1) * 4 + 1], out[(index - 1) * 4 + 2], out[(index - 1) * 4 + 3]];
        if index % nk == 0 {
            temp = [temp[1], temp[2], temp[3], temp[0]];
            for byte in &mut temp {
                *byte = sbox(*byte);
            }
            temp[0] ^= RCON[index / nk];
        } else if nk > 6 && index % nk == 4 {
            for byte in &mut temp {
                *byte = sbox(*byte);
            }
        }
        for byte in 0..4 {
            let value = out[(index - nk) * 4 + byte] ^ temp[byte];
            out.push(value);
        }
        index += 1;
    }
    Ok(out)
}

fn add_round(state: &mut [u8; 16], round: &[u8]) {
    for index in 0..16 {
        state[index] ^= round[index];
    }
}

fn sub_bytes(state: &mut [u8; 16]) {
    for byte in state {
        *byte = sbox(*byte);
    }
}

fn sbox(byte: u8) -> u8 {
    let byte = std::hint::black_box(byte);
    let mut out = 0u8;
    let mut index = 0usize;
    while index < 256 {
        let candidate = unsafe { std::ptr::read_volatile(SBOX.as_ptr().add(index)) };
        out |= candidate & ct_eq_mask(byte, index as u8);
        index += 1;
    }
    std::hint::black_box(out)
}

fn ct_eq_mask(left: u8, right: u8) -> u8 {
    let diff = (left ^ right) as u32;
    let equal = diff.wrapping_sub(1) >> 31;
    (equal as u8).wrapping_neg()
}

fn shift_rows(state: &mut [u8; 16]) {
    let original = *state;
    for row in 1..4 {
        for column in 0..4 {
            state[row + 4 * column] = original[row + 4 * ((column + row) % 4)];
        }
    }
}

fn mix_columns(state: &mut [u8; 16]) {
    for column in 0..4 {
        let i = column * 4;
        let a = [state[i], state[i + 1], state[i + 2], state[i + 3]];
        state[i] = xtime(a[0]) ^ (xtime(a[1]) ^ a[1]) ^ a[2] ^ a[3];
        state[i + 1] = a[0] ^ xtime(a[1]) ^ (xtime(a[2]) ^ a[2]) ^ a[3];
        state[i + 2] = a[0] ^ a[1] ^ xtime(a[2]) ^ (xtime(a[3]) ^ a[3]);
        state[i + 3] = (xtime(a[0]) ^ a[0]) ^ a[1] ^ a[2] ^ xtime(a[3]);
    }
}

fn xtime(value: u8) -> u8 {
    let mask = (value >> 7).wrapping_neg();
    (value << 1) ^ (0x1b & mask)
}

fn aes_gcm_seal(key: &[u8], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut counter = [0u8; 16];
    counter[..12].copy_from_slice(nonce);
    counter[15] = 1;
    let tag_mask = aes_encrypt_block(key, &inc(&mut counter))?;
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < plain.len() {
        let block = aes_encrypt_block(key, &inc(&mut counter))?;
        let end = (offset + 16).min(plain.len());
        for (left, right) in plain[offset..end].iter().zip(block) {
            out.push(left ^ right);
        }
        offset = end;
    }
    let tag = ghash_tag(key, aad, &out, &tag_mask)?;
    out.extend_from_slice(&tag);
    Ok(out)
}

fn aes_gcm_open(key: &[u8], nonce: &[u8; 12], aad: &[u8], body: &[u8]) -> Result<Vec<u8>, String> {
    if body.len() < 16 {
        return Err("TLS record authentication failed".into());
    }
    let (cipher, tag) = body.split_at(body.len() - 16);
    let mut counter = [0u8; 16];
    counter[..12].copy_from_slice(nonce);
    counter[15] = 1;
    let tag_mask = aes_encrypt_block(key, &inc(&mut counter))?;
    let expect = ghash_tag(key, aad, cipher, &tag_mask)?;
    if !equal(&expect, tag) {
        return Err("TLS record authentication failed".into());
    }
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < cipher.len() {
        let block = aes_encrypt_block(key, &inc(&mut counter))?;
        let end = (offset + 16).min(cipher.len());
        for (left, right) in cipher[offset..end].iter().zip(block) {
            out.push(left ^ right);
        }
        offset = end;
    }
    Ok(out)
}

fn inc(counter: &mut [u8; 16]) -> [u8; 16] {
    let current = *counter;
    let mut value = u32::from_be_bytes(counter[12..16].try_into().unwrap());
    value = value.wrapping_add(1);
    counter[12..16].copy_from_slice(&value.to_be_bytes());
    current
}

fn ghash_tag(key: &[u8], aad: &[u8], cipher: &[u8], mask: &[u8; 16]) -> Result<[u8; 16], String> {
    let h = aes_encrypt_block(key, &[0u8; 16])?;
    let mut y = [0u8; 16];
    ghash_update(&mut y, &h, aad);
    ghash_update(&mut y, &h, cipher);
    let mut lengths = [0u8; 16];
    let aad_bits = (aad.len() as u64).checked_mul(8).ok_or("TLS record exceeds the memory budget")?;
    let cipher_bits = (cipher.len() as u64).checked_mul(8).ok_or("TLS record exceeds the memory budget")?;
    lengths[..8].copy_from_slice(&aad_bits.to_be_bytes());
    lengths[8..].copy_from_slice(&cipher_bits.to_be_bytes());
    for index in 0..16 {
        y[index] ^= lengths[index];
    }
    y = ghash_mul(&y, &h);
    for index in 0..16 {
        y[index] ^= mask[index];
    }
    Ok(y)
}

fn ghash_update(y: &mut [u8; 16], h: &[u8; 16], data: &[u8]) {
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + 16).min(data.len());
        for (slot, byte) in y.iter_mut().zip(data[offset..end].iter()) {
            *slot ^= byte;
        }
        *y = ghash_mul(y, h);
        offset = end;
    }
}

fn ghash_mul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *y;
    for bit_index in 0..128 {
        let bit = (x[bit_index / 8] >> (7 - (bit_index % 8))) & 1;
        let mask = bit.wrapping_neg();
        for index in 0..16 {
            z[index] ^= v[index] & mask;
        }
        let lsb = v[15] & 1;
        let reduce = lsb.wrapping_neg();
        let mut next = [0u8; 16];
        for index in (0..16).rev() {
            let mut value = v[index] >> 1;
            if index > 0 {
                value |= (v[index - 1] & 1) << 7;
            }
            next[index] = value;
        }
        next[0] ^= 0xe1 & reduce;
        v = next;
    }
    z
}

fn chacha_seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut counter = [0u8; 16];
    counter[4..].copy_from_slice(nonce);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&chacha_block(key, &counter)[..32]);
    counter[0] = 1;
    let mut out = chacha_xor(key, &counter, plain);
    let tag = poly1305(&poly_key, aad, &out);
    out.extend_from_slice(&tag);
    Ok(out)
}

fn chacha_open(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], body: &[u8]) -> Result<Vec<u8>, String> {
    if body.len() < 16 {
        return Err("TLS record authentication failed".into());
    }
    let (cipher, tag) = body.split_at(body.len() - 16);
    let mut counter = [0u8; 16];
    counter[4..].copy_from_slice(nonce);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&chacha_block(key, &counter)[..32]);
    if !equal(&poly1305(&poly_key, aad, cipher), tag) {
        return Err("TLS record authentication failed".into());
    }
    counter[0] = 1;
    Ok(chacha_xor(key, &counter, cipher))
}

fn chacha_xor(key: &[u8; 32], counter: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut block_counter = u32::from_le_bytes(counter[..4].try_into().unwrap());
    let mut offset = 0;
    while offset < data.len() {
        let mut nonce = *counter;
        nonce[..4].copy_from_slice(&block_counter.to_le_bytes());
        let block = chacha_block(key, &nonce);
        let end = (offset + 64).min(data.len());
        for (left, right) in data[offset..end].iter().zip(block) {
            out.push(left ^ right);
        }
        block_counter = block_counter.wrapping_add(1);
        offset = end;
    }
    out
}

fn chacha_block(key: &[u8; 32], counter: &[u8; 16]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;
    for index in 0..8 {
        state[4 + index] = u32::from_le_bytes(key[index * 4..index * 4 + 4].try_into().unwrap());
    }
    for index in 0..4 {
        state[12 + index] = u32::from_le_bytes(counter[index * 4..index * 4 + 4].try_into().unwrap());
    }
    let original = state;
    for _ in 0..10 {
        quarter(&mut state, 0, 4, 8, 12);
        quarter(&mut state, 1, 5, 9, 13);
        quarter(&mut state, 2, 6, 10, 14);
        quarter(&mut state, 3, 7, 11, 15);
        quarter(&mut state, 0, 5, 10, 15);
        quarter(&mut state, 1, 6, 11, 12);
        quarter(&mut state, 2, 7, 8, 13);
        quarter(&mut state, 3, 4, 9, 14);
    }
    for index in 0..16 {
        state[index] = state[index].wrapping_add(original[index]);
    }
    let mut out = [0u8; 64];
    for index in 0..16 {
        out[index * 4..index * 4 + 4].copy_from_slice(&state[index].to_le_bytes());
    }
    out
}

fn quarter(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

fn poly1305(key: &[u8; 32], aad: &[u8], cipher: &[u8]) -> [u8; 16] {
    let mut mac = Vec::new();
    mac.extend_from_slice(aad);
    mac.extend_from_slice(&vec![0u8; (16 - aad.len() % 16) % 16]);
    mac.extend_from_slice(cipher);
    mac.extend_from_slice(&vec![0u8; (16 - cipher.len() % 16) % 16]);
    mac.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac.extend_from_slice(&(cipher.len() as u64).to_le_bytes());
    poly1305_mac(key, &mac)
}

fn poly1305_mac(key: &[u8], message: &[u8]) -> [u8; 16] {
    let mut r = [0u8; 16];
    r.copy_from_slice(&key[..16]);
    r[3] &= 15;
    r[7] &= 15;
    r[11] &= 15;
    r[15] &= 15;
    r[4] &= 252;
    r[8] &= 252;
    r[12] &= 252;
    let r_limbs = poly_limbs(&r, false);
    let mut acc = [0u64; 5];
    let mut offset = 0usize;
    while offset < message.len() {
        let end = (offset + 16).min(message.len());
        let mut block = [0u8; 17];
        block[..end - offset].copy_from_slice(&message[offset..end]);
        block[end - offset] = 1;
        let n = poly_limbs(&block, true);
        for index in 0..5 {
            acc[index] = acc[index].wrapping_add(n[index]);
        }
        acc = poly_mul(acc, r_limbs);
        offset = end;
    }
    acc = poly_freeze(acc);
    let bytes = poly_pack(acc);
    let mut carry = 0u16;
    let mut tag = [0u8; 16];
    for index in 0..16 {
        let sum = bytes[index] as u16 + key[16 + index] as u16 + carry;
        tag[index] = sum as u8;
        carry = sum >> 8;
    }
    let _ = bytes;
    tag
}

fn poly_limbs(bytes: &[u8], wide: bool) -> [u64; 5] {
    let mut n = 0u128;
    let limit = bytes.len().min(16);
    for index in 0..limit {
        n |= (bytes[index] as u128) << (8 * index);
    }
    let mask = (1u128 << 26) - 1;
    let mut limbs = [
        (n & mask) as u64,
        ((n >> 26) & mask) as u64,
        ((n >> 52) & mask) as u64,
        ((n >> 78) & mask) as u64,
        ((n >> 104) & mask) as u64,
    ];
    if wide && bytes.len() > 16 {
        limbs[4] |= (bytes[16] as u64) << 24;
    }
    limbs
}

fn poly_mul(a: [u64; 5], b: [u64; 5]) -> [u64; 5] {
    let mut c = [0u128; 5];
    for i in 0..5 {
        for j in 0..5 {
            let prod = a[i] as u128 * b[j] as u128;
            if i + j < 5 {
                c[i + j] += prod;
            } else {
                c[i + j - 5] += prod * 5;
            }
        }
    }
    poly_carry(c)
}

fn poly_carry(mut c: [u128; 5]) -> [u64; 5] {
    let mask = (1u128 << 26) - 1;
    for _ in 0..3 {
        let mut carry = 0u128;
        for index in 0..5 {
            let value = c[index] + carry;
            c[index] = value & mask;
            carry = value >> 26;
        }
        c[0] += carry * 5;
    }
    let mut carry = 0u128;
    for index in 0..5 {
        let value = c[index] + carry;
        c[index] = value & mask;
        carry = value >> 26;
    }
    c[0] += carry * 5;
    c[0] &= mask;
    [c[0] as u64, c[1] as u64, c[2] as u64, c[3] as u64, c[4] as u64]
}

fn poly_freeze(a: [u64; 5]) -> [u64; 5] {
    let mask = (1u128 << 26) - 1;
    let mut c = [0u128; 5];
    c[0] = a[0] as u128 + 5;
    for index in 1..5 {
        c[index] = a[index] as u128;
    }
    for index in 0..4 {
        c[index + 1] += c[index] >> 26;
        c[index] &= mask;
    }
    let hi = (c[4] >> 26) as u64;
    c[4] &= mask;
    let select = 0u64.wrapping_sub(hi & 1);
    let mut out = [0u64; 5];
    for index in 0..5 {
        out[index] = (a[index] & !select) | ((c[index] as u64) & select);
    }
    out
}

fn poly_pack(a: [u64; 5]) -> [u8; 16] {
    let mut n = a[0] as u128;
    n |= (a[1] as u128) << 26;
    n |= (a[2] as u128) << 52;
    n |= (a[3] as u128) << 78;
    n |= (a[4] as u128) << 104;
    let mut out = [0u8; 16];
    for index in 0..16 {
        out[index] = (n >> (8 * index)) as u8;
    }
    out
}

const X25519_BASE: [u8; 32] = [9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn clamp_scalar(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = bytes.len().min(32);
    out[..n].copy_from_slice(&bytes[..n]);
    out[0] &= 248;
    out[31] &= 127;
    out[31] |= 64;
    out
}

fn x25519_shared(scalar: &[u8; 32], point: &[u8; 32]) -> Result<[u8; 32], String> {
    let shared = x25519(scalar, point);
    let mut acc = 0u8;
    for byte in shared {
        acc |= byte;
    }
    if acc == 0 {
        return Err("TLS key share was rejected".into());
    }
    Ok(shared)
}

fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let mut e = *scalar;
    e[0] &= 248;
    e[31] &= 127;
    e[31] |= 64;
    let x1 = Fe::from_bytes(point);
    let mut x2 = Fe::one();
    let mut z2 = Fe::zero();
    let mut x3 = x1;
    let mut z3 = Fe::one();
    let a24 = Fe::from_bytes(&{
        let mut bytes = [0u8; 32];
        bytes[..4].copy_from_slice(&121665u32.to_le_bytes());
        bytes
    });
    let mut swap = 0u64;
    for pos in (0..255).rev() {
        let bit = ((e[pos / 8] >> (pos % 8)) & 1) as u64;
        swap ^= bit;
        Fe::cswap(&mut x2, &mut x3, swap);
        Fe::cswap(&mut z2, &mut z3, swap);
        swap = bit;
        let a = x2.add(&z2);
        let aa = a.square();
        let b = x2.sub(&z2);
        let bb = b.square();
        let difference = aa.sub(&bb);
        let c = x3.add(&z3);
        let d = x3.sub(&z3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        let da_cb = da.add(&cb);
        x3 = da_cb.square();
        let da_sub = da.sub(&cb);
        z3 = x1.mul(&da_sub.square());
        x2 = aa.mul(&bb);
        z2 = difference.mul(&aa.add(&a24.mul(&difference)));
    }
    Fe::cswap(&mut x2, &mut x3, swap);
    Fe::cswap(&mut z2, &mut z3, swap);
    x2.mul(&z2.invert()).to_bytes()
}

fn x25519_prime() -> Nat {
    let mut bytes = [0xffu8; 32];
    bytes[0] = 0xed;
    bytes[31] = 0x7f;
    Nat::from_le(&bytes)
}

fn x25519_pm2() -> Nat {
    let mut bytes = [0xffu8; 32];
    bytes[0] = 0xeb;
    bytes[31] = 0x7f;
    Nat::from_le(&bytes)
}

fn field_add(left: &Nat, right: &Nat, prime: &Nat) -> Nat {
    left.add(right).divmod(prime).1
}

fn field_sub(left: &Nat, right: &Nat, prime: &Nat) -> Nat {
    let left = left.divmod(prime).1;
    let right = right.divmod(prime).1;
    if left.cmp(&right) == std::cmp::Ordering::Less {
        prime.sub(&right).add(&left).divmod(prime).1
    } else {
        left.sub(&right)
    }
}

fn field_mul(left: &Nat, right: &Nat, prime: &Nat) -> Nat {
    left.mod_mul(right, prime)
}

#[derive(Clone, Copy)]
struct Fe([u64; 5]);

impl Fe {
    fn zero() -> Fe { Fe([0; 5]) }
    fn one() -> Fe { Fe([1, 0, 0, 0, 0]) }
    fn mask() -> u64 { (1u64 << 51) - 1 }
    fn from_bytes(bytes: &[u8; 32]) -> Fe {
        let load = |offset: usize| -> u64 {
            let mut value = 0u64;
            for index in 0..8 {
                if offset + index < 32 {
                    value |= (bytes[offset + index] as u64) << (8 * index);
                }
            }
            value
        };
        let mask = Fe::mask();
        Fe([
            load(0) & mask,
            (load(6) >> 3) & mask,
            (load(12) >> 6) & mask,
            (load(19) >> 1) & mask,
            (load(24) >> 12) & mask,
        ])
    }
    fn to_bytes(mut self) -> [u8; 32] {
        self.carry();
        self.carry();
        let mut q = (self.0[0].wrapping_add(19)) >> 51;
        q = (self.0[1].wrapping_add(q)) >> 51;
        q = (self.0[2].wrapping_add(q)) >> 51;
        q = (self.0[3].wrapping_add(q)) >> 51;
        q = (self.0[4].wrapping_add(q)) >> 51;
        self.0[0] = self.0[0].wrapping_add(19 * q);
        let mask = Fe::mask();
        let mut carry = self.0[0] >> 51;
        self.0[0] &= mask;
        for index in 1..5 {
            self.0[index] = self.0[index].wrapping_add(carry);
            carry = self.0[index] >> 51;
            self.0[index] &= mask;
        }
        let mut out = [0u8; 32];
        let mut acc = 0u128;
        let mut bits = 0u32;
        let mut at = 0usize;
        for limb in self.0 {
            acc |= (limb as u128) << bits;
            bits += 51;
            while bits >= 8 && at < 32 {
                out[at] = acc as u8;
                acc >>= 8;
                bits -= 8;
                at += 1;
            }
        }
        if at < 32 {
            out[at] = acc as u8;
        }
        out
    }
    fn carry(&mut self) {
        let mask = Fe::mask();
        let mut carry = self.0[0] >> 51;
        self.0[0] &= mask;
        self.0[1] = self.0[1].wrapping_add(carry);
        carry = self.0[1] >> 51;
        self.0[1] &= mask;
        self.0[2] = self.0[2].wrapping_add(carry);
        carry = self.0[2] >> 51;
        self.0[2] &= mask;
        self.0[3] = self.0[3].wrapping_add(carry);
        carry = self.0[3] >> 51;
        self.0[3] &= mask;
        self.0[4] = self.0[4].wrapping_add(carry);
        carry = self.0[4] >> 51;
        self.0[4] &= mask;
        self.0[0] = self.0[0].wrapping_add(carry.wrapping_mul(19));
        carry = self.0[0] >> 51;
        self.0[0] &= mask;
        self.0[1] = self.0[1].wrapping_add(carry);
        carry = self.0[1] >> 51;
        self.0[1] &= mask;
        self.0[2] = self.0[2].wrapping_add(carry);
    }
    fn add(self, other: &Fe) -> Fe {
        let mut out = Fe([0; 5]);
        for index in 0..5 {
            out.0[index] = self.0[index].wrapping_add(other.0[index]);
        }
        out.carry();
        out
    }
    fn sub(self, other: &Fe) -> Fe {
        let mask = Fe::mask();
        let prime = [mask - 18, mask, mask, mask, mask];
        let mut out = self.0;
        for _ in 0..8 {
            for index in 0..5 {
                out[index] = out[index].wrapping_add(prime[index]);
            }
        }
        for index in 0..5 {
            out[index] = out[index].wrapping_sub(other.0[index]);
        }
        let mut value = Fe(out);
        value.carry();
        value.carry();
        value
    }
    fn mul(self, other: &Fe) -> Fe {
        let a = self.0;
        let b = other.0;
        let mut c = [0u128; 5];
        for i in 0..5 {
            for j in 0..5 {
                let prod = a[i] as u128 * b[j] as u128;
                if i + j < 5 {
                    c[i + j] += prod;
                } else {
                    c[i + j - 5] += prod * 19;
                }
            }
        }
        let mask = (1u128 << 51) - 1;
        let mut carry = 0u128;
        let mut limbs = [0u64; 5];
        for index in 0..4 {
            let value = c[index] + carry;
            limbs[index] = (value & mask) as u64;
            carry = value >> 51;
        }
        let value = c[4] + carry;
        limbs[4] = (value & mask) as u64;
        let wrapped = (value >> 51) * 19;
        let low = limbs[0] as u128 + wrapped;
        limbs[0] = (low & mask) as u64;
        limbs[1] = limbs[1].wrapping_add((low >> 51) as u64);
        let mut out = Fe(limbs);
        out.carry();
        out.carry();
        out
    }
    fn square(self) -> Fe { self.mul(&self) }
    fn invert(self) -> Fe {
        let mut result = Fe::one();
        let mut exp = [0xffu8; 32];
        exp[0] = 0x7f;
        exp[31] = 0xeb;
        for byte in exp {
            for bit in (0..8).rev() {
                result = result.square();
                if byte & (1 << bit) != 0 {
                    result = result.mul(&self);
                }
            }
        }
        result
    }
    fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
        let mask = 0u64.wrapping_sub(swap & 1);
        for index in 0..5 {
            let diff = mask & (a.0[index] ^ b.0[index]);
            a.0[index] ^= diff;
            b.0[index] ^= diff;
        }
    }
}

fn p256_scalar(bytes: &[u8]) -> Result<[u8; 32], String> {
    if bytes.len() < 32 {
        return Err("TLS elliptic-curve secret was rejected".into());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes[..32]);
    let scalar = Nat::from_be(&out);
    let order = Nat::from_be(&P256_N);
    if scalar.is_zero() || scalar.cmp(&order) != std::cmp::Ordering::Less {
        return Err("TLS elliptic-curve secret was rejected".into());
    }
    Ok(out)
}

fn p256_public(secret: &[u8; 32]) -> Result<Vec<u8>, String> {
    let point = p256_mul(secret, &P256_G)?;
    Ok(p256_uncompressed(&point))
}

fn p256_shared(secret: &[u8; 32], public: &[u8]) -> Result<Vec<u8>, String> {
    let point = p256_decode(public)?;
    let shared = p256_mul(secret, &point)?;
    if shared.inf {
        return Err("TLS key share was rejected".into());
    }
    Ok(shared.x.to_be(32))
}

fn p256_uncompressed(point: &Point) -> Vec<u8> {
    let mut out = vec![4];
    out.extend_from_slice(&point.x.to_be(32));
    out.extend_from_slice(&point.y.to_be(32));
    out
}

#[derive(Clone)]
struct Point {
    x: Nat,
    y: Nat,
    inf: bool,
}

fn p256_decode(bytes: &[u8]) -> Result<Point, String> {
    if bytes.len() == 65 && bytes[0] == 4 {
        let x = Nat::from_be(&bytes[1..33]);
        let y = Nat::from_be(&bytes[33..65]);
        let point = Point { x, y, inf: false };
        if !p256_on_curve(&point) {
            return Err("TLS key share is not on the curve".into());
        }
        return Ok(point);
    }
    Err("TLS key share is not an uncompressed point".into())
}

fn p256_on_curve(point: &Point) -> bool {
    if point.inf { return false; }
    let p = Nat::from_be(&P256_P);
    let y2 = point.y.mod_mul(&point.y, &p);
    let x2 = point.x.mod_mul(&point.x, &p);
    let x3 = x2.mod_mul(&point.x, &p);
    let three_x = point.x.mod_mul(&Nat::from_be(&[3]), &p);
    let right = x3.add(&p).sub(&three_x).mod_mul(&Nat::from_be(&[1]), &p);
    let b = Nat::from_be(&P256_B);
    let right = right.add(&b);
    let right = if right.cmp(&p) != std::cmp::Ordering::Less { right.sub(&p) } else { right };
    y2.cmp(&right) == std::cmp::Ordering::Equal
}

fn p256_mul(scalar: &[u8; 32], point: &Point) -> Result<Point, String> {
    if point.inf {
        return Ok(point.clone());
    }
    let base = Proj::from_affine(point)?;
    let mut result = Proj::infinity();
    for byte in scalar {
        for bit in (0..8).rev() {
            result = result.double();
            let added = result.add(&base);
            let choose = ((byte >> bit) & 1) as u64;
            result = Proj::select(&result, &added, choose);
        }
    }
    result.to_affine()
}

struct Proj {
    x: [u64; 4],
    y: [u64; 4],
    z: [u64; 4],
}

impl Proj {
    fn infinity() -> Proj {
        Proj { x: [0; 4], y: p256_to_mont(&[1, 0, 0, 0]), z: [0; 4] }
    }
    fn from_affine(point: &Point) -> Result<Proj, String> {
        let x = nat_to_limbs(&point.x);
        let y = nat_to_limbs(&point.y);
        Ok(Proj { x: p256_to_mont(&x), y: p256_to_mont(&y), z: p256_to_mont(&[1, 0, 0, 0]) })
    }
    fn double(&self) -> Proj { self.add(self) }
    fn add(&self, other: &Proj) -> Proj {
        let (x1, y1, z1) = (self.x, self.y, self.z);
        let (x2, y2, z2) = (other.x, other.y, other.z);
        let t0 = p256_mul_m(&x1, &x2);
        let t1 = p256_mul_m(&y1, &y2);
        let t2 = p256_mul_m(&z1, &z2);
        let t3 = p256_mul_m(&p256_add_m(&x1, &y1), &p256_add_m(&x2, &y2));
        let t4 = p256_add_m(&t0, &t1);
        let t3 = p256_sub_m(&t3, &t4);
        let t4 = p256_mul_m(&p256_add_m(&x1, &z1), &p256_add_m(&x2, &z2));
        let t5 = p256_add_m(&t0, &t2);
        let t4 = p256_sub_m(&t4, &t5);
        let t5 = p256_mul_m(&p256_add_m(&y1, &z1), &p256_add_m(&y2, &z2));
        let x3 = p256_add_m(&t1, &t2);
        let t5 = p256_sub_m(&t5, &x3);
        let z3 = p256_mul_m(&p256_a(), &t4);
        let x3 = p256_mul_m(&p256_b3(), &t2);
        let z3 = p256_add_m(&x3, &z3);
        let x3 = p256_sub_m(&t1, &z3);
        let z3 = p256_add_m(&t1, &z3);
        let mut y3 = p256_mul_m(&x3, &z3);
        let t1 = p256_add_m(&p256_add_m(&t0, &t0), &t0);
        let t2 = p256_mul_m(&p256_a(), &t2);
        let t4 = p256_mul_m(&p256_b3(), &t4);
        let t1 = p256_add_m(&t1, &t2);
        let t2 = p256_mul_m(&p256_a(), &p256_sub_m(&t0, &t2));
        let t4 = p256_add_m(&t4, &t2);
        let t0 = p256_mul_m(&t1, &t4);
        y3 = p256_add_m(&y3, &t0);
        let t0 = p256_mul_m(&t5, &t4);
        let mut x3 = p256_mul_m(&t3, &x3);
        x3 = p256_sub_m(&x3, &t0);
        let t0 = p256_mul_m(&t3, &t1);
        let mut z3 = p256_mul_m(&t5, &z3);
        z3 = p256_add_m(&z3, &t0);
        Proj { x: x3, y: y3, z: z3 }
    }
    fn select(a: &Proj, b: &Proj, choose_b: u64) -> Proj {
        Proj {
            x: p256_select(&a.x, &b.x, choose_b),
            y: p256_select(&a.y, &b.y, choose_b),
            z: p256_select(&a.z, &b.z, choose_b),
        }
    }
    fn to_affine(&self) -> Result<Point, String> {
        if p256_is_zero(&self.z) {
            return Ok(Point { x: Nat::zero(), y: Nat::zero(), inf: true });
        }
        let inv = p256_inv(&self.z);
        let x = p256_from_mont(&p256_mul_m(&self.x, &inv));
        let y = p256_from_mont(&p256_mul_m(&self.y, &inv));
        Ok(Point { x: limbs_to_nat(&x), y: limbs_to_nat(&y), inf: false })
    }
}

fn p256_limbs() -> [u64; 4] {
    [
        0xffff_ffff_ffff_ffff,
        0x0000_0000_ffff_ffff,
        0x0000_0000_0000_0000,
        0xffff_ffff_0000_0001,
    ]
}

fn p256_rr() -> [u64; 4] {
    static RR: std::sync::OnceLock<[u64; 4]> = std::sync::OnceLock::new();
    *RR.get_or_init(|| {
        let p = Nat::from_be(&P256_P);
        let mut wide = vec![0u8; 33];
        wide[0] = 1;
        let r_mod = Nat::from_be(&wide).divmod(&p).1;
        let rr = r_mod.mod_mul(&r_mod, &p);
        nat_to_limbs(&rr)
    })
}

fn nat_to_limbs(value: &Nat) -> [u64; 4] {
    let bytes = value.to_be(32);
    let mut out = [0u64; 4];
    for index in 0..4 {
        let mut limb = [0u8; 8];
        limb.copy_from_slice(&bytes[index * 8..index * 8 + 8]);
        out[3 - index] = u64::from_be_bytes(limb);
    }
    out
}

fn limbs_to_nat(value: &[u64; 4]) -> Nat {
    let mut bytes = [0u8; 32];
    for index in 0..4 {
        bytes[index * 8..index * 8 + 8].copy_from_slice(&value[3 - index].to_be_bytes());
    }
    Nat::from_be(&bytes)
}

fn p256_to_mont(value: &[u64; 4]) -> [u64; 4] {
    p256_mont_mul(value, &p256_rr())
}

fn p256_from_mont(value: &[u64; 4]) -> [u64; 4] {
    p256_mont_mul(value, &[1, 0, 0, 0])
}

fn p256_a() -> [u64; 4] {
    static A: std::sync::OnceLock<[u64; 4]> = std::sync::OnceLock::new();
    *A.get_or_init(|| {
        let p = Nat::from_be(&P256_P);
        let three = Nat::from_be(&[3]);
        p256_to_mont(&nat_to_limbs(&p.sub(&three)))
    })
}

fn p256_b3() -> [u64; 4] {
    static B: std::sync::OnceLock<[u64; 4]> = std::sync::OnceLock::new();
    *B.get_or_init(|| {
        let b = Nat::from_be(&P256_B);
        let three = Nat::from_be(&[3]);
        let p = Nat::from_be(&P256_P);
        p256_to_mont(&nat_to_limbs(&b.mul(&three).divmod(&p).1))
    })
}

fn p256_mul_m(left: &[u64; 4], right: &[u64; 4]) -> [u64; 4] {
    p256_mont_mul(left, right)
}

fn p256_add_m(left: &[u64; 4], right: &[u64; 4]) -> [u64; 4] {
    let mut out = [0u64; 4];
    let mut carry = 0u128;
    for index in 0..4 {
        let sum = left[index] as u128 + right[index] as u128 + carry;
        out[index] = sum as u64;
        carry = sum >> 64;
    }
    p256_cond_sub(out, carry as u64)
}

fn p256_sub_m(left: &[u64; 4], right: &[u64; 4]) -> [u64; 4] {
    let (diff, borrow) = p256_sub_limbs(left, right);
    let added = p256_add_raw(&diff, &p256_limbs());
    p256_select(&diff, &added, borrow)
}

fn p256_add_raw(left: &[u64; 4], right: &[u64; 4]) -> [u64; 4] {
    let mut out = [0u64; 4];
    let mut carry = 0u128;
    for index in 0..4 {
        let sum = left[index] as u128 + right[index] as u128 + carry;
        out[index] = sum as u64;
        carry = sum >> 64;
    }
    let _ = carry;
    out
}

fn p256_sub_limbs(left: &[u64; 4], right: &[u64; 4]) -> ([u64; 4], u64) {
    let mut out = [0u64; 4];
    let mut borrow = 0u64;
    for index in 0..4 {
        let diff = (left[index] as u128).wrapping_sub(right[index] as u128).wrapping_sub(borrow as u128);
        out[index] = diff as u64;
        borrow = (diff >> 127) as u64;
    }
    (out, borrow)
}

fn p256_cond_sub(value: [u64; 4], extra: u64) -> [u64; 4] {
    let mut current = value;
    let mut extra = extra;
    for _ in 0..4 {
        let (diff, borrow) = p256_sub_limbs(&current, &p256_limbs());
        let underflow = extra.wrapping_sub(borrow) >> 63;
        let mask = 0u64.wrapping_sub(underflow & 1);
        let new_extra = extra.wrapping_sub(borrow);
        current = p256_select(&diff, &current, mask);
        extra = (new_extra & !mask) | (extra & mask);
    }
    current
}

fn p256_select(a: &[u64; 4], b: &[u64; 4], choose_b: u64) -> [u64; 4] {
    let mask = 0u64.wrapping_sub(choose_b & 1);
    let mut out = [0u64; 4];
    for index in 0..4 {
        out[index] = (a[index] & !mask) | (b[index] & mask);
    }
    out
}

fn p256_is_zero(value: &[u64; 4]) -> bool {
    let mut acc = 0u64;
    for limb in value {
        acc |= limb;
    }
    acc == 0
}

fn p256_mont_mul(a: &[u64; 4], b: &[u64; 4]) -> [u64; 4] {
    let p = p256_limbs();
    let mask = 0xffff_ffff_ffff_ffffu128;
    let mut acc = [0u128; 9];
    for i in 0..4 {
        let mut carry = 0u128;
        for j in 0..4 {
            let (sum, over) = p256_add3(acc[i + j], a[i] as u128 * b[j] as u128, carry);
            acc[i + j] = sum & mask;
            carry = (sum >> 64) + (over << 64);
        }
        acc[i + 4] = acc[i + 4].wrapping_add(carry);
    }
    for i in 0..4 {
        let m = acc[i] as u64;
        let mut carry = 0u128;
        for j in 0..4 {
            let (sum, over) = p256_add3(acc[i + j], m as u128 * p[j] as u128, carry);
            acc[i + j] = sum & mask;
            carry = (sum >> 64) + (over << 64);
        }
        let mut at = i + 4;
        while carry != 0 && at < acc.len() {
            let sum = acc[at].wrapping_add(carry);
            acc[at] = sum & mask;
            carry = sum >> 64;
            at += 1;
        }
    }
    p256_cond_sub([acc[4] as u64, acc[5] as u64, acc[6] as u64, acc[7] as u64], acc[8] as u64)
}

fn p256_add3(left: u128, mid: u128, right: u128) -> (u128, u128) {
    let (sum, first) = left.overflowing_add(mid);
    let (sum, second) = sum.overflowing_add(right);
    (sum, u128::from(first) + u128::from(second))
}

fn p256_inv(value: &[u64; 4]) -> [u64; 4] {
    let mut result = p256_to_mont(&[1, 0, 0, 0]);
    let mut exp = p256_limbs();
    exp[0] = exp[0].wrapping_sub(2);
    for limb_index in (0..4).rev() {
        for bit in (0..64).rev() {
            result = p256_mont_mul(&result, &result);
            if exp[limb_index] & (1u64 << bit) != 0 {
                result = p256_mont_mul(&result, value);
            }
        }
    }
    result
}

fn p256_add(a: &Point, b: &Point) -> Result<Point, String> {
    if a.inf { return Ok(b.clone()); }
    if b.inf { return Ok(a.clone()); }
    let p = Nat::from_be(&P256_P);
    if a.x.cmp(&b.x) == std::cmp::Ordering::Equal {
        let sum = a.y.add(&b.y);
        let sum = if sum.cmp(&p) != std::cmp::Ordering::Less { sum.sub(&p) } else { sum };
        if sum.is_zero() {
            return Ok(Point { x: Nat::zero(), y: Nat::zero(), inf: true });
        }
        let three_x2 = a.x.mod_mul(&a.x, &p).mod_mul(&Nat::from_be(&[3]), &p);
        let mut num = three_x2.add(&p).sub(&Nat::from_be(&[3]));
        if num.cmp(&p) != std::cmp::Ordering::Less { num = num.sub(&p); }
        let den = a.y.mod_mul(&Nat::from_be(&[2]), &p);
        let slope = num.mod_mul(&den.mod_inv(&p)?, &p);
        return p256_slope(a, b, &slope, &p);
    }
    let mut num = b.y.add(&p).sub(&a.y);
    if num.cmp(&p) != std::cmp::Ordering::Less { num = num.sub(&p); }
    let mut den = b.x.add(&p).sub(&a.x);
    if den.cmp(&p) != std::cmp::Ordering::Less { den = den.sub(&p); }
    let slope = num.mod_mul(&den.mod_inv(&p)?, &p);
    p256_slope(a, b, &slope, &p)
}

fn p256_slope(a: &Point, b: &Point, slope: &Nat, p: &Nat) -> Result<Point, String> {
    let mut x = slope.mod_mul(slope, p).add(p).add(p).sub(&a.x).sub(&b.x);
    while x.cmp(p) != std::cmp::Ordering::Less { x = x.sub(p); }
    let mut y = a.x.add(p).sub(&x);
    if y.cmp(p) != std::cmp::Ordering::Less { y = y.sub(p); }
    y = slope.mod_mul(&y, p).add(p).sub(&a.y);
    if y.cmp(p) != std::cmp::Ordering::Less { y = y.sub(p); }
    Ok(Point { x, y, inf: false })
}

#[derive(Clone)]
struct Nat {
    limb: Vec<u32>,
}

impl Nat {
    fn zero() -> Nat { Nat { limb: vec![0] } }
    fn is_zero(&self) -> bool { self.limb.iter().all(|limb| *limb == 0) }
    fn from_be(bytes: &[u8]) -> Nat {
        let mut limb = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0;
        for byte in bytes.iter().rev() {
            acc |= (*byte as u32) << bits;
            bits += 8;
            if bits >= 32 {
                limb.push(acc);
                acc = if bits > 32 { (*byte as u32) >> (8 - (bits - 32)) } else { 0 };
                bits -= 32;
                if bits == 0 { acc = 0; }
            }
        }
        // The bit accumulation above is easy to get wrong for partial limbs. Rebuild directly.
        let _ = (limb, acc, bits);
        let mut out = vec![0u32; bytes.len().div_ceil(4)];
        for (index, byte) in bytes.iter().rev().enumerate() {
            out[index / 4] |= (*byte as u32) << ((index % 4) * 8);
        }
        if out.is_empty() { out.push(0); }
        Nat { limb: out }.trim()
    }
    fn from_le(bytes: &[u8]) -> Nat {
        let mut reversed = bytes.to_vec();
        reversed.reverse();
        Nat::from_be(&reversed)
    }
    fn to_be(&self, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        for (index, limb) in self.limb.iter().enumerate() {
            for byte in 0..4 {
                let at = index * 4 + byte;
                if at < len {
                    out[len - 1 - at] = (limb >> (8 * byte)) as u8;
                }
            }
        }
        out
    }
    fn to_le(&self, len: usize) -> Vec<u8> {
        let mut out = self.to_be(len);
        out.reverse();
        out
    }
    fn trim(mut self) -> Nat {
        while self.limb.len() > 1 && self.limb.last() == Some(&0) { self.limb.pop(); }
        self
    }
    fn bit_len(&self) -> usize {
        let top = *self.limb.last().unwrap_or(&0);
        self.limb.len().saturating_sub(1) * 32 + (32 - top.leading_zeros() as usize)
    }
    fn bit(&self, index: usize) -> bool {
        self.limb.get(index / 32).is_some_and(|limb| limb & (1 << (index % 32)) != 0)
    }
    fn cmp(&self, other: &Nat) -> std::cmp::Ordering {
        let len = self.limb.len().max(other.limb.len());
        for index in (0..len).rev() {
            let left = *self.limb.get(index).unwrap_or(&0);
            let right = *other.limb.get(index).unwrap_or(&0);
            if left != right { return left.cmp(&right); }
        }
        std::cmp::Ordering::Equal
    }
    fn add(&self, other: &Nat) -> Nat {
        let mut limb = Vec::new();
        let mut carry = 0u64;
        let len = self.limb.len().max(other.limb.len());
        for index in 0..len {
            carry += *self.limb.get(index).unwrap_or(&0) as u64 + *other.limb.get(index).unwrap_or(&0) as u64;
            limb.push(carry as u32);
            carry >>= 32;
        }
        if carry != 0 { limb.push(carry as u32); }
        Nat { limb }.trim()
    }
    fn sub(&self, other: &Nat) -> Nat {
        let mut limb = Vec::new();
        let mut borrow = 0i64;
        for index in 0..self.limb.len() {
            let value = self.limb[index] as i64 - *other.limb.get(index).unwrap_or(&0) as i64 - borrow;
            if value < 0 {
                limb.push((value + (1i64 << 32)) as u32);
                borrow = 1;
            } else {
                limb.push(value as u32);
                borrow = 0;
            }
        }
        Nat { limb }.trim()
    }
    fn mul(&self, other: &Nat) -> Nat {
        let mut limb = vec![0u32; self.limb.len() + other.limb.len() + 1];
        for i in 0..self.limb.len() {
            let mut carry = 0u64;
            for j in 0..other.limb.len() {
                carry += limb[i + j] as u64 + self.limb[i] as u64 * other.limb[j] as u64;
                limb[i + j] = carry as u32;
                carry >>= 32;
            }
            limb[i + other.limb.len()] = carry as u32;
        }
        Nat { limb }.trim()
    }
    fn divmod(&self, divisor: &Nat) -> (Nat, Nat) {
        if divisor.is_zero() { return (Nat::zero(), self.clone()); }
        let mut quotient = Nat { limb: vec![0u32; self.limb.len()] };
        let mut remainder = Nat::zero();
        for index in (0..self.bit_len()).rev() {
            remainder = remainder.add(&remainder);
            if self.bit(index) {
                remainder = remainder.add(&Nat { limb: vec![1] });
            }
            if remainder.cmp(divisor) != std::cmp::Ordering::Less {
                remainder = remainder.sub(divisor);
                let limb = index / 32;
                if limb >= quotient.limb.len() { quotient.limb.resize(limb + 1, 0); }
                quotient.limb[limb] |= 1 << (index % 32);
            }
        }
        (quotient.trim(), remainder.trim())
    }
    fn mod_mul(&self, other: &Nat, modulus: &Nat) -> Nat {
        self.mul(other).divmod(modulus).1
    }
    fn mod_inv(&self, modulus: &Nat) -> Result<Nat, String> {
        // Extended Euclid.
        let mut t = Nat::zero();
        let mut new_t = Nat { limb: vec![1] };
        let mut r = modulus.clone();
        let mut new_r = self.divmod(modulus).1;
        let mut t_neg = false;
        let mut new_t_neg = false;
        if new_r.is_zero() && self.cmp(modulus) != std::cmp::Ordering::Less {
            return Err("Value has no inverse".into());
        }
        while !new_r.is_zero() {
            let (q, rem) = r.divmod(&new_r);
            r = new_r;
            new_r = rem;
            let product = q.mul(if new_t_neg { &new_t } else { &new_t });
            let (next, next_neg) = if t_neg == new_t_neg {
                if t.cmp(&product) != std::cmp::Ordering::Less { (t.sub(&product), t_neg) } else { (product.sub(&t), !t_neg) }
            } else {
                (t.add(&product), t_neg)
            };
            t = new_t;
            t_neg = new_t_neg;
            new_t = next;
            new_t_neg = next_neg;
        }
        if r.cmp(&Nat { limb: vec![1] }) != std::cmp::Ordering::Equal {
            return Err("Value has no inverse".into());
        }
        if t_neg {
            Ok(modulus.sub(&t.divmod(modulus).1))
        } else {
            Ok(t.divmod(modulus).1)
        }
    }
    fn modpow(&self, exponent: &Nat, modulus: &Nat) -> Nat {
        let mut result = Nat { limb: vec![1] };
        let mut base = self.divmod(modulus).1;
        for index in 0..exponent.bit_len() {
            if exponent.bit(index) {
                result = result.mod_mul(&base, modulus);
            }
            base = base.mod_mul(&base, modulus);
        }
        result
    }
}

fn key_share_point(bytes: &[u8], len: usize) -> Result<[u8; 32], String> {
    if bytes.len() != len || len != 32 {
        return Err("TLS key share has an unexpected length".into());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes);
    Ok(out)
}

#[derive(Clone)]
struct Certificate {
    raw: Vec<u8>,
    tbs: Vec<u8>,
    issuer: Vec<u8>,
    subject: Vec<u8>,
    not_before: i64,
    not_after: i64,
    public: PublicKey,
    signature_alg: SigAlg,
    signature: Vec<u8>,
    ca: bool,
    path_len: Option<u32>,
    key_usage: Option<u16>,
    eku: Option<Vec<Vec<u8>>>,
    names: Vec<Name>,
    constraints: NameConstraints,
}

impl std::fmt::Debug for Certificate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Certificate")
    }
}

#[derive(Clone, Default)]
struct NameConstraints {
    permitted_dns: Vec<String>,
    excluded_dns: Vec<String>,
    permitted_ip: Vec<(Vec<u8>, Vec<u8>)>,
    excluded_ip: Vec<(Vec<u8>, Vec<u8>)>,
    permitted_dn: Vec<Vec<u8>>,
    excluded_dn: Vec<Vec<u8>>,
}

#[derive(Clone)]
enum PublicKey {
    Rsa { n: Nat, e: Nat },
    P256 { point: Point },
}

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PublicKey")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PssParams {
    hash: HashKind,
    mgf: HashKind,
    salt_len: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SigAlg {
    RsaSha256,
    RsaSha384,
    RsaSha512,
    EcdsaSha256,
    EcdsaSha384,
    Pss(PssParams),
}

fn pss_scheme(hash: HashKind) -> SigAlg {
    SigAlg::Pss(PssParams { hash, mgf: hash, salt_len: hash_len(hash) })
}

#[derive(Clone)]
enum Name {
    Dns(String),
    Ip(Vec<u8>),
}

fn parse_certificate_13(message: &[u8]) -> Result<Vec<Certificate>, String> {
    let body = message_body(message)?;
    if body.len() < 3 {
        return Err("TLS certificate message is truncated".into());
    }
    let context = body[0] as usize;
    let mut at = 1usize.checked_add(context).ok_or("TLS certificate list is truncated")?;
    if at.checked_add(3).ok_or("TLS certificate list is truncated")? > body.len() {
        return Err("TLS certificate list is truncated".into());
    }
    let list = u32::from_be_bytes([0, body[at], body[at + 1], body[at + 2]]) as usize;
    at += 3;
    let end = at.checked_add(list).ok_or("TLS certificate list is truncated")?;
    parse_cert_list(body.get(at..end).ok_or("TLS certificate list is truncated")?, true)
}

fn parse_certificate_12(message: &[u8]) -> Result<Vec<Certificate>, String> {
    let body = message_body(message)?;
    if body.len() < 3 {
        return Err("TLS certificate message is truncated".into());
    }
    let list = u32::from_be_bytes([0, body[0], body[1], body[2]]) as usize;
    let end = 3usize.checked_add(list).ok_or("TLS certificate list is truncated")?;
    parse_cert_list(body.get(3..end).ok_or("TLS certificate list is truncated")?, false)
}

fn message_body(message: &[u8]) -> Result<&[u8], String> {
    if message.len() < 4 {
        return Err("TLS handshake message is truncated".into());
    }
    let length = u32::from_be_bytes([0, message[1], message[2], message[3]]) as usize;
    let end = 4usize.checked_add(length).ok_or("TLS handshake message is truncated")?;
    message.get(4..end).ok_or_else(|| "TLS handshake message is truncated".into())
}

fn parse_tls_extension_list(body: &[u8]) -> Result<(), String> {
    if body.len() < 2 {
        return Err("TLS extension is truncated".into());
    }
    let len = u16::from_be_bytes([body[0], body[1]]) as usize;
    let end = 2usize.checked_add(len).ok_or("TLS extension is truncated")?;
    if end != body.len() {
        return Err("TLS extension is truncated".into());
    }
    walk_tls_extensions(&body[2..])
}

fn walk_tls_extensions(bytes: &[u8]) -> Result<(), String> {
    let mut at = 0usize;
    let mut seen = Vec::new();
    let mut count = 0usize;
    while at < bytes.len() {
        count += 1;
        if count > EXTENSION_LIMIT {
            return Err("TLS extensions exceed the memory budget".into());
        }
        if at.checked_add(4).map(|end| end > bytes.len()).unwrap_or(true) {
            return Err("TLS extension is truncated".into());
        }
        let kind = u16::from_be_bytes([bytes[at], bytes[at + 1]]);
        let len = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        if seen.contains(&kind) {
            return Err("TLS message repeats an extension".into());
        }
        seen.push(kind);
        let data_at = at.checked_add(4).ok_or("TLS extension is truncated")?;
        let data_end = data_at.checked_add(len).ok_or("TLS extension is truncated")?;
        if data_end > bytes.len() {
            return Err("TLS extension is truncated".into());
        }
        at = data_end;
    }
    Ok(())
}

fn parse_cert_list(bytes: &[u8], tls13: bool) -> Result<Vec<Certificate>, String> {
    let mut at = 0usize;
    let mut chain = Vec::new();
    while at + 3 <= bytes.len() {
        let len = u32::from_be_bytes([0, bytes[at], bytes[at + 1], bytes[at + 2]]) as usize;
        at = at.checked_add(3).ok_or("Certificate is truncated")?;
        let end = at.checked_add(len).ok_or("Certificate is truncated")?;
        let der = bytes.get(at..end).ok_or("Certificate is truncated")?;
        chain.push(parse_certificate(der)?);
        at = end;
        if tls13 {
            if at + 2 > bytes.len() {
                return Err("Certificate extensions are truncated".into());
            }
            let ext = u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize;
            let ext_at = at.checked_add(2).ok_or("Certificate extensions are truncated")?;
            let ext_end = ext_at.checked_add(ext).ok_or("Certificate extensions are truncated")?;
            let block = bytes.get(ext_at..ext_end).ok_or("Certificate extensions are truncated")?;
            walk_tls_extensions(block)?;
            at = ext_end;
        }
        if chain.len() > CHAIN_LIMIT {
            return Err("Certificate chain is too long".into());
        }
    }
    if at != bytes.len() {
        return Err("TLS certificate list is truncated".into());
    }
    if chain.is_empty() {
        return Err("TLS server sent no certificate".into());
    }
    Ok(chain)
}

fn parse_certificate(der: &[u8]) -> Result<Certificate, String> {
    let mut cursor = Der::new(der);
    let raw = cursor.sequence_bytes()?;
    let mut seq = Der::new(raw);
    let tbs = seq.sequence_tlv()?.to_vec();
    let alg = signature_algorithm(&seq.sequence_bytes()?)?;
    let signature_bits = seq.bit_string()?;
    let mut tbs_outer = Der::new(&tbs);
    let tbs_inner = tbs_outer.sequence_bytes()?;
    let mut tbs_cursor = Der::new(tbs_inner);
    if tbs_cursor.peek_tag() == 0xa0 {
        let _ = tbs_cursor.explicit(0)?;
    }
    let _serial = tbs_cursor.any()?;
    let inner_alg = signature_algorithm(&tbs_cursor.sequence_bytes()?)?;
    if inner_alg != alg && !compatible_sig(inner_alg, alg) {
        return Err("Certificate signature algorithm differs from its body".into());
    }
    let issuer = tbs_cursor.sequence_bytes()?.to_vec();
    let validity = tbs_cursor.sequence_bytes()?;
    let (not_before, not_after) = validity_window(validity)?;
    let subject = tbs_cursor.sequence_bytes()?.to_vec();
    let spki = tbs_cursor.sequence_bytes()?;
    let public = public_key(spki)?;
    let mut extensions = CertExt::default();
    let mut saw_extensions = false;
    while tbs_cursor.remaining() > 0 {
        let tag = tbs_cursor.peek_tag();
        if tag == 0xa3 {
            if saw_extensions {
                return Err("Certificate repeats an extension".into());
            }
            saw_extensions = true;
            let ext_bytes = tbs_cursor.explicit(3)?;
            let mut ext_seq = Der::new(ext_bytes);
            let list = ext_seq.sequence_bytes()?;
            if ext_seq.remaining() > 0 {
                return Err("Certificate repeats an extension".into());
            }
            extensions = parse_extensions(list)?;
        } else {
            let _ = tbs_cursor.any()?;
        }
    }
    Ok(Certificate {
        raw: der.to_vec(),
        tbs,
        issuer,
        subject,
        not_before,
        not_after,
        public,
        signature_alg: alg,
        signature: signature_bits,
        ca: extensions.ca,
        path_len: extensions.path_len,
        key_usage: extensions.key_usage,
        eku: extensions.eku,
        names: extensions.names,
        constraints: extensions.constraints,
    })
}

#[derive(Default)]
struct CertExt {
    ca: bool,
    path_len: Option<u32>,
    key_usage: Option<u16>,
    eku: Option<Vec<Vec<u8>>>,
    names: Vec<Name>,
    constraints: NameConstraints,
}

fn compatible_sig(inner: SigAlg, outer: SigAlg) -> bool {
    inner == outer
}

fn parse_extensions(bytes: &[u8]) -> Result<CertExt, String> {
    let mut cursor = Der::new(bytes);
    let mut out = CertExt::default();
    let mut count = 0usize;
    let mut seen = Vec::new();
    while cursor.remaining() > 0 {
        count += 1;
        if count > EXTENSION_LIMIT {
            return Err("Certificate has too many extensions".into());
        }
        let ext = cursor.sequence_bytes()?;
        let mut item = Der::new(ext);
        let oid = item.oid()?;
        if seen.iter().any(|prior| prior == &oid) {
            return Err("Certificate repeats an extension".into());
        }
        seen.push(oid.clone());
        let mut critical = false;
        if item.peek_tag() == 0x01 {
            critical = item.boolean()?;
        }
        let value = item.octet()?;
        let known = match oid.as_slice() {
            [0x55, 0x1d, 0x13] => {
                parse_basic_constraints(value, &mut out)?;
                true
            }
            [0x55, 0x1d, 0x0f] => {
                out.key_usage = Some(parse_key_usage(value)?);
                true
            }
            [0x55, 0x1d, 0x25] => {
                out.eku = Some(parse_eku(value)?);
                true
            }
            [0x55, 0x1d, 0x11] => {
                parse_san(value, &mut out.names)?;
                true
            }
            [0x55, 0x1d, 0x1e] => {
                out.constraints = parse_name_constraints(value)?;
                true
            }
            [0x55, 0x1d, 0x0e] | [0x55, 0x1d, 0x23] => {
                let _ = value;
                true
            }
            [0x55, 0x1d, 0x20] => {
                parse_policies(value)?;
                true
            }
            _ => false,
        };
        if critical && !known {
            return Err("Certificate has an unsupported critical extension".into());
        }
    }
    Ok(out)
}

fn parse_basic_constraints(value: &[u8], out: &mut CertExt) -> Result<(), String> {
    let mut body = Der::new(value);
    let seq = body.sequence_bytes()?;
    if body.remaining() > 0 {
        return Err("Certificate basic constraints are not valid".into());
    }
    let mut fields = Der::new(seq);
    if fields.remaining() > 0 && fields.peek_tag() == 0x01 {
        out.ca = fields.boolean()?;
    }
    if fields.remaining() > 0 && fields.peek_tag() == 0x02 {
        let raw = fields.integer()?;
        if raw.len() > 4 {
            return Err("Certificate path length is not usable".into());
        }
        let mut path = 0u32;
        for byte in raw {
            path = (path << 8) | byte as u32;
        }
        if !out.ca {
            return Err("Certificate path length is not usable".into());
        }
        out.path_len = Some(path);
    }
    if fields.remaining() > 0 {
        return Err("Certificate basic constraints are not valid".into());
    }
    Ok(())
}

fn parse_key_usage(value: &[u8]) -> Result<u16, String> {
    let mut der = Der::new(value);
    let (tag, len) = der.header()?;
    if tag != 0x03 || len < 2 {
        return Err("Certificate key usage is invalid".into());
    }
    let unused = *der.data.get(der.at).ok_or("Certificate key usage is invalid")?;
    if unused > 7 {
        return Err("Certificate key usage is invalid".into());
    }
    let bits_at = der.at.checked_add(1).ok_or("Certificate key usage is invalid")?;
    let bits_end = der.at.checked_add(len).ok_or("Certificate key usage is invalid")?;
    if bits_end != der.data.len() {
        return Err("Certificate key usage is invalid".into());
    }
    let bits = der.data.get(bits_at..bits_end).ok_or("Certificate key usage is invalid")?;
    let last = *bits.last().ok_or("Certificate key usage is invalid")?;
    let unused_mask = if unused == 0 { 0 } else { 0xffu8 >> (8 - unused) };
    if last & unused_mask != 0 {
        return Err("Certificate key usage is invalid".into());
    }
    let significant = bits.len().saturating_mul(8).saturating_sub(unused as usize);
    let mut usage = 0u16;
    for (index, byte) in bits.iter().enumerate() {
        for bit in 0..8 {
            let pos = index * 8 + bit;
            if pos >= significant || pos >= 16 {
                break;
            }
            if byte & (1 << (7 - bit)) != 0 {
                usage |= 1 << pos;
            }
        }
    }
    Ok(usage)
}

fn parse_eku(value: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut body = Der::new(value);
    let seq = body.sequence_bytes()?;
    let mut items = Der::new(seq);
    let mut oids = Vec::new();
    while items.remaining() > 0 {
        if oids.len() >= NAME_LIMIT {
            return Err("Certificate extended key usage is too large".into());
        }
        oids.push(items.oid()?);
    }
    if oids.is_empty() {
        return Err("Certificate extended key usage is empty".into());
    }
    Ok(oids)
}

fn parse_san(value: &[u8], names: &mut Vec<Name>) -> Result<(), String> {
    let mut body = Der::new(value);
    let seq = body.sequence_bytes()?;
    let mut names_der = Der::new(seq);
    while names_der.remaining() > 0 {
        if names.len() >= NAME_LIMIT {
            return Err("Certificate has too many names".into());
        }
        let (tag, data) = names_der.tagged()?;
        if tag == 0x82 {
            if !data.is_ascii() {
                return Err("Certificate name is not text".into());
            }
            let text = std::str::from_utf8(data).map_err(|_| "Certificate name is not text")?;
            names.push(Name::Dns(text.to_ascii_lowercase()));
        } else if tag == 0x87 {
            if data.len() != 4 && data.len() != 16 {
                return Err("Certificate address is invalid".into());
            }
            names.push(Name::Ip(data.to_vec()));
        }
    }
    Ok(())
}

fn parse_policies(value: &[u8]) -> Result<(), String> {
    let mut body = Der::new(value);
    let seq = body.sequence_bytes()?;
    let mut items = Der::new(seq);
    let mut count = 0usize;
    while items.remaining() > 0 {
        count += 1;
        if count > EXTENSION_LIMIT {
            return Err("Certificate policies are too large".into());
        }
        let policy = items.sequence_bytes()?;
        let mut policy_der = Der::new(policy);
        let _oid = policy_der.oid()?;
        while policy_der.remaining() > 0 {
            let _ = policy_der.any()?;
        }
    }
    if count == 0 {
        return Err("Certificate policies are empty".into());
    }
    Ok(())
}

fn parse_name_constraints(value: &[u8]) -> Result<NameConstraints, String> {
    let mut body = Der::new(value);
    let seq = body.sequence_bytes()?;
    if body.remaining() > 0 {
        return Err("Certificate name constraint is not supported".into());
    }
    let mut cursor = Der::new(seq);
    let mut out = NameConstraints::default();
    let mut saw_permit = false;
    let mut saw_exclude = false;
    while cursor.remaining() > 0 {
        let (tag, data) = cursor.tagged()?;
        match tag {
            0xa0 => {
                if saw_permit {
                    return Err("Certificate repeats an extension".into());
                }
                saw_permit = true;
                parse_subtrees(data, &mut out.permitted_dns, &mut out.permitted_ip, &mut out.permitted_dn)?;
            }
            0xa1 => {
                if saw_exclude {
                    return Err("Certificate repeats an extension".into());
                }
                saw_exclude = true;
                parse_subtrees(data, &mut out.excluded_dns, &mut out.excluded_ip, &mut out.excluded_dn)?;
            }
            _ => return Err("Certificate name constraint is not supported".into()),
        }
    }
    Ok(out)
}

fn parse_subtrees(data: &[u8], dns: &mut Vec<String>, ip: &mut Vec<(Vec<u8>, Vec<u8>)>, dn: &mut Vec<Vec<u8>>) -> Result<(), String> {
    let mut list = data;
    if data.first() == Some(&0x30) {
        let mut cursor = Der::new(data);
        let inner = cursor.sequence_bytes()?;
        if cursor.remaining() == 0 && inner.first() == Some(&0x30) {
            list = inner;
        }
    }
    let mut cursor = Der::new(list);
    while cursor.remaining() > 0 {
        if dns.len() + ip.len() + dn.len() >= NAME_LIMIT {
            return Err("Certificate name constraints are too large".into());
        }
        let subtree = cursor.sequence_bytes()?;
        let mut sub = Der::new(subtree);
        let (tag, name) = sub.tagged()?;
        match tag {
            0x82 => {
                if !name.is_ascii() {
                    return Err("Certificate name constraint is not supported".into());
                }
                let text = std::str::from_utf8(name).map_err(|_| "Certificate name constraint is not supported")?;
                dns.push(dns_constraint_name(text)?);
            }
            0x87 => {
                if name.len() != 8 && name.len() != 32 {
                    return Err("Certificate name constraint is not supported".into());
                }
                let (addr, mask) = name.split_at(name.len() / 2);
                ip.push((addr.to_vec(), mask.to_vec()));
            }
            0xa4 => dn.push(name.to_vec()),
            _ => return Err("Certificate name constraint is not supported".into()),
        }
        if sub.remaining() > 0 {
            let (extra, extra_value) = sub.tagged()?;
            if extra != 0x80 || !is_zero_integer(extra_value) || sub.remaining() > 0 {
                return Err("Certificate name constraint is not supported".into());
            }
        }
    }
    Ok(())
}

fn is_zero_integer(bytes: &[u8]) -> bool {
    bytes == [0x00] || bytes == [0x02, 0x01, 0x00]
}

fn rsa_exponent_ok(e: &Nat) -> Result<(), String> {
    if !e.bit(0) || e.bit_len() < 2 || e.bit_len() > RSA_EXPONENT_BITS {
        return Err("Certificate RSA exponent is not usable".into());
    }
    Ok(())
}

fn public_key(spki: &[u8]) -> Result<PublicKey, String> {
    let mut seq = Der::new(spki);
    let alg = seq.sequence_bytes()?;
    let bits = seq.bit_string()?;
    let mut alg_der = Der::new(alg);
    let oid = alg_der.oid()?;
    if oid == [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01] {
        let mut key = Der::new(&bits);
        let pair = key.sequence_bytes()?;
        let mut pair_der = Der::new(pair);
        let n = Nat::from_be(&pair_der.integer()?);
        let e = Nat::from_be(&pair_der.integer()?);
        if n.bit_len() < 2048 || n.bit_len() > 4096 {
            return Err("Certificate RSA key is outside the supported range".into());
        }
        rsa_exponent_ok(&e)?;
        return Ok(PublicKey::Rsa { n, e });
    }
    if oid == [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01] {
        let curve = alg_der.oid()?;
        if curve != [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07] {
            return Err("Certificate uses an unsupported curve".into());
        }
        return Ok(PublicKey::P256 { point: p256_decode(&bits)? });
    }
    Err("Certificate uses an unsupported public key".into())
}

fn signature_algorithm(bytes: &[u8]) -> Result<SigAlg, String> {
    let mut der = Der::new(bytes);
    let oid = der.oid()?;
    let alg = match oid.as_slice() {
        [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b] => SigAlg::RsaSha256,
        [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0c] => SigAlg::RsaSha384,
        [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0d] => SigAlg::RsaSha512,
        [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02] => SigAlg::EcdsaSha256,
        [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x03] => SigAlg::EcdsaSha384,
        [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a] => SigAlg::Pss(pss_params(&mut der)?),
        _ => return Err("Certificate uses an unsupported signature".into()),
    };
    Ok(alg)
}

fn pss_params(der: &mut Der<'_>) -> Result<PssParams, String> {
    if der.remaining() == 0 {
        return Err("Certificate uses an unsupported signature".into());
    }
    let params = der.sequence_bytes().map_err(|_| "Certificate uses an unsupported signature".to_string())?;
    if der.remaining() != 0 {
        return Err("Certificate uses an unsupported signature".into());
    }
    let mut seq = Der::new(params);
    let mut hash = None;
    let mut mgf = None;
    let mut salt = None;
    let mut trailer = 1usize;
    let mut saw_trailer = false;
    while seq.remaining() > 0 {
        match seq.peek_tag() {
            0xa0 if hash.is_none() && mgf.is_none() && salt.is_none() && !saw_trailer => {
                hash = Some(hash_kind_identifier(seq.explicit(0)?)?);
            }
            0xa1 if mgf.is_none() && salt.is_none() && !saw_trailer => {
                mgf = Some(mgf_hash_identifier(seq.explicit(1)?)?);
            }
            0xa2 if salt.is_none() && !saw_trailer => {
                salt = Some(pss_uint(seq.explicit(2)?)?);
            }
            0xa3 if !saw_trailer => {
                trailer = pss_uint(seq.explicit(3)?)?;
                saw_trailer = true;
            }
            _ => return Err("Certificate uses an unsupported signature".into()),
        }
    }
    if trailer != 1 {
        return Err("Certificate uses an unsupported signature".into());
    }
    let hash = hash.ok_or_else(|| "Certificate uses an unsupported signature".to_string())?;
    let mgf = mgf.ok_or_else(|| "Certificate uses an unsupported signature".to_string())?;
    Ok(PssParams { hash, mgf, salt_len: salt.unwrap_or(20) })
}

fn hash_kind_identifier(bytes: &[u8]) -> Result<HashKind, String> {
    let mut der = Der::new(bytes);
    let seq = der.sequence_bytes().map_err(|_| "Certificate uses an unsupported signature".to_string())?;
    if der.remaining() != 0 {
        return Err("Certificate uses an unsupported signature".into());
    }
    let mut body = Der::new(seq);
    hash_kind_oid(&body.oid()?)
}

fn hash_kind_oid(oid: &[u8]) -> Result<HashKind, String> {
    match oid {
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01] => Ok(HashKind::Sha256),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02] => Ok(HashKind::Sha384),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03] => Ok(HashKind::Sha512),
        _ => Err("Certificate uses an unsupported signature".into()),
    }
}

fn mgf_hash_identifier(bytes: &[u8]) -> Result<HashKind, String> {
    let mut der = Der::new(bytes);
    let seq = der.sequence_bytes().map_err(|_| "Certificate uses an unsupported signature".to_string())?;
    if der.remaining() != 0 {
        return Err("Certificate uses an unsupported signature".into());
    }
    let mut body = Der::new(seq);
    let oid = body.oid()?;
    if oid != [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x08] || body.remaining() == 0 {
        return Err("Certificate uses an unsupported signature".into());
    }
    hash_kind_identifier(&seq[body.at..])
}

fn pss_uint(bytes: &[u8]) -> Result<usize, String> {
    let mut der = Der::new(bytes);
    let value = der.integer().map_err(|_| "Certificate uses an unsupported signature".to_string())?;
    if der.remaining() != 0 || value.len() > 4 {
        return Err("Certificate uses an unsupported signature".into());
    }
    let mut number = 0usize;
    for byte in value {
        number = (number << 8) | byte as usize;
    }
    Ok(number)
}

fn validity_window(bytes: &[u8]) -> Result<(i64, i64), String> {
    let mut der = Der::new(bytes);
    let start = der.time()?;
    let end = der.time()?;
    if end < start {
        return Err("Certificate validity is reversed".into());
    }
    Ok((start, end))
}

struct Der<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Der<'a> {
    fn new(data: &'a [u8]) -> Der<'a> { Der { data, at: 0 } }
    fn remaining(&self) -> usize { self.data.len().saturating_sub(self.at) }
    fn peek_tag(&self) -> u8 { self.data.get(self.at).copied().unwrap_or(0) }
    fn header(&mut self) -> Result<(u8, usize), String> {
        let tag = *self.data.get(self.at).ok_or("Certificate is truncated")?;
        self.at += 1;
        let first = *self.data.get(self.at).ok_or("Certificate is truncated")?;
        self.at += 1;
        let len = if first < 0x80 {
            first as usize
        } else {
            let count = (first & 0x7f) as usize;
            if count == 0 || count > 3 || self.at.checked_add(count).map(|end| end > self.data.len()).unwrap_or(true) {
                return Err("Certificate length is invalid".into());
            }
            if self.data[self.at] == 0 {
                return Err("Certificate length is invalid".into());
            }
            let mut value = 0usize;
            for _ in 0..count {
                value = (value << 8) | self.data[self.at] as usize;
                self.at += 1;
            }
            if value < 128 || (count > 1 && value < 1usize << (8 * (count - 1))) {
                return Err("Certificate length is invalid".into());
            }
            value
        };
        let end = self.at.checked_add(len).ok_or("Certificate is truncated")?;
        if end > self.data.len() {
            return Err("Certificate is truncated".into());
        }
        Ok((tag, len))
    }
    fn sequence_bytes(&mut self) -> Result<&'a [u8], String> {
        let (tag, len) = self.header()?;
        if tag != 0x30 {
            return Err("Certificate structure is not a sequence".into());
        }
        let bytes = &self.data[self.at..self.at + len];
        self.at += len;
        Ok(bytes)
    }
    fn sequence_tlv(&mut self) -> Result<&'a [u8], String> {
        let start = self.at;
        let (tag, len) = self.header()?;
        if tag != 0x30 {
            return Err("Certificate structure is not a sequence".into());
        }
        self.at = self.at.checked_add(len).ok_or("Certificate is truncated")?;
        Ok(&self.data[start..self.at])
    }
    fn integer(&mut self) -> Result<Vec<u8>, String> {
        let (tag, len) = self.header()?;
        if tag != 0x02 || len == 0 {
            return Err("Certificate integer is invalid".into());
        }
        let bytes = &self.data[self.at..self.at + len];
        self.at += len;
        if bytes[0] & 0x80 != 0 {
            return Err("Certificate integer is negative".into());
        }
        if bytes.len() > 1 && bytes[0] == 0 && bytes[1] & 0x80 == 0 {
            return Err("Certificate integer is invalid".into());
        }
        Ok(bytes.iter().skip_while(|byte| **byte == 0).copied().collect())
    }
    fn oid(&mut self) -> Result<Vec<u8>, String> {
        let (tag, len) = self.header()?;
        if tag != 0x06 {
            return Err("Certificate object identifier is invalid".into());
        }
        let bytes = self.data[self.at..self.at + len].to_vec();
        self.at += len;
        Ok(bytes)
    }
    fn bit_string(&mut self) -> Result<Vec<u8>, String> {
        let (tag, len) = self.header()?;
        if tag != 0x03 || len < 1 {
            return Err("Certificate bit string is invalid".into());
        }
        let unused = self.data[self.at];
        if unused != 0 {
            return Err("Certificate bit string is not byte-aligned".into());
        }
        let bytes = self.data[self.at + 1..self.at + len].to_vec();
        self.at += len;
        Ok(bytes)
    }
    fn octet(&mut self) -> Result<&'a [u8], String> {
        let (tag, len) = self.header()?;
        if tag != 0x04 {
            return Err("Certificate octet string is invalid".into());
        }
        let bytes = &self.data[self.at..self.at + len];
        self.at += len;
        Ok(bytes)
    }
    fn boolean(&mut self) -> Result<bool, String> {
        let (tag, len) = self.header()?;
        if tag != 0x01 || len != 1 {
            return Err("Certificate boolean is invalid".into());
        }
        let value = match self.data[self.at] {
            0xff => true,
            0x00 => false,
            _ => return Err("Certificate boolean is invalid".into()),
        };
        self.at += 1;
        Ok(value)
    }
    fn time(&mut self) -> Result<i64, String> {
        let (tag, len) = self.header()?;
        let bytes = &self.data[self.at..self.at + len];
        self.at += len;
        parse_time(tag, bytes)
    }
    fn explicit(&mut self, number: u8) -> Result<&'a [u8], String> {
        let (tag, len) = self.header()?;
        if tag != 0xa0 | number {
            return Err("Certificate field is missing".into());
        }
        let bytes = &self.data[self.at..self.at + len];
        self.at += len;
        Ok(bytes)
    }
    fn tagged(&mut self) -> Result<(u8, &'a [u8]), String> {
        let (tag, len) = self.header()?;
        let bytes = &self.data[self.at..self.at + len];
        self.at += len;
        Ok((tag, bytes))
    }
    fn any(&mut self) -> Result<&'a [u8], String> {
        let start = self.at;
        let (_, len) = self.header()?;
        self.at += len;
        Ok(&self.data[start..self.at])
    }
}

fn parse_time(tag: u8, bytes: &[u8]) -> Result<i64, String> {
    let (year, rest) = if tag == 0x17 {
        if bytes.len() != 13 || bytes[12] != b'Z' {
            return Err("Certificate time is invalid".into());
        }
        let yy = time_digits(&bytes[..2])?;
        (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &bytes[2..12])
    } else if tag == 0x18 {
        if bytes.len() != 15 || bytes[14] != b'Z' {
            return Err("Certificate time is invalid".into());
        }
        (time_digits(&bytes[..4])?, &bytes[4..14])
    } else {
        return Err("Certificate time is invalid".into());
    };
    if rest.len() != 10 {
        return Err("Certificate time is invalid".into());
    }
    let month = time_digits(&rest[..2])?;
    let day = time_digits(&rest[2..4])?;
    let hour = time_digits(&rest[4..6])?;
    let minute = time_digits(&rest[6..8])?;
    let second = time_digits(&rest[8..10])?;
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 60 || !(1..=9999).contains(&year) {
        return Err("Certificate time is invalid".into());
    }
    let mdays = [31, if leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if day < 1 || day > mdays[(month - 1) as usize] {
        return Err("Certificate time is invalid".into());
    }
    let days = if year >= 1970 {
        let mut days = 0i64;
        for past in 1970..year {
            days += if leap(past) { 366 } else { 365 };
        }
        for past in 1..month {
            days += mdays[(past - 1) as usize];
        }
        days + day - 1
    } else {
        let mut days = 0i64;
        for past in year..1970 {
            days += if leap(past) { 366 } else { 365 };
        }
        let mut into = 0i64;
        for past in 1..month {
            into += mdays[(past - 1) as usize];
        }
        into + day - 1 - days
    };
    Ok(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn time_digits(bytes: &[u8]) -> Result<i64, String> {
    if bytes.is_empty() {
        return Err("Certificate time is invalid".into());
    }
    let mut value = 0i64;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err("Certificate time is invalid".into());
        }
        value = value * 10 + (byte - b'0') as i64;
    }
    Ok(value)
}

fn leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn verify_chain(host: &str, anchors: &[Certificate], chain: &[Certificate]) -> Result<Certificate, String> {
    if chain.is_empty() || chain.len() > CHAIN_LIMIT {
        return Err("Certificate chain is too long".into());
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_secs() as i64).unwrap_or(0);
    for cert in chain {
        if now < cert.not_before || now > cert.not_after {
            return Err("Certificate is outside its validity window".into());
        }
    }
    let top = chain.last().unwrap();
    let anchor = anchors.iter().find(|anchor| anchor.subject == top.issuer || anchor.subject == top.subject).ok_or("Certificate chain is not signed by a system trust anchor")?;
    if now < anchor.not_before || now > anchor.not_after {
        return Err("Trust anchor is outside its validity window".into());
    }
    let mut path: Vec<&Certificate> = Vec::new();
    path.push(anchor);
    if anchor.subject == top.subject {
        if !same_public_key(anchor, top) {
            return Err("Certificate chain is not signed by a system trust anchor".into());
        }
        for cert in chain[..chain.len() - 1].iter().rev() {
            path.push(cert);
        }
    } else {
        for cert in chain.iter().rev() {
            path.push(cert);
        }
    }
    if path.len() < 2 && !hostname_matches(host, path[0]) {
        return Err("Certificate is not valid for this host name".into());
    }
    for index in 0..path.len() - 1 {
        if path[index + 1].issuer != path[index].subject {
            return Err("Certificate issuer name does not match the signer".into());
        }
    }
    enforce_path_policy(&path)?;
    for index in 0..path.len() - 1 {
        verify_signed(path[index], path[index + 1])?;
    }
    let leaf = *path.last().unwrap();
    if !hostname_matches(host, leaf) {
        return Err("Certificate is not valid for this host name".into());
    }
    Ok(leaf.clone())
}

fn eku_allows_server(eku: &[Vec<u8>]) -> bool {
    eku.iter().any(|oid| oid.as_slice() == [0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01] || oid.as_slice() == [0x55, 0x1d, 0x25, 0x00])
}

fn same_public_key(left: &Certificate, right: &Certificate) -> bool {
    match (&left.public, &right.public) {
        (PublicKey::Rsa { n: ln, e: le }, PublicKey::Rsa { n: rn, e: re }) => ln.cmp(rn) == std::cmp::Ordering::Equal && le.cmp(re) == std::cmp::Ordering::Equal,
        (PublicKey::P256 { point: left }, PublicKey::P256 { point: right }) => left.x.cmp(&right.x) == std::cmp::Ordering::Equal && left.y.cmp(&right.y) == std::cmp::Ordering::Equal && left.inf == right.inf,
        _ => false,
    }
}

fn enforce_path_policy(path: &[&Certificate]) -> Result<(), String> {
    if let Some(eku) = &path[0].eku {
        if !eku_allows_server(eku) {
            return Err("Certificate is not allowed for this service".into());
        }
    }
    let mut max_path = path[0].path_len.unwrap_or(CHAIN_LIMIT as u32);
    let mut constraints: Vec<&NameConstraints> = Vec::new();
    if path.len() > 1 && path[0].constraints.has_any() {
        constraints.push(&path[0].constraints);
    }
    if path.len() > 1 {
        if let Some(usage) = path[0].key_usage {
            if usage & KU_KEY_CERT_SIGN == 0 {
                return Err("Certificate issuer is not allowed to sign certificates".into());
            }
        }
    }
    for (index, cert) in path.iter().enumerate().skip(1) {
        for constraint in &constraints {
            constraint.allows(cert)?;
        }
        let leaf = index + 1 == path.len();
        if !leaf {
            if !cert.ca {
                return Err("Certificate issuer is not a certificate authority".into());
            }
            if let Some(usage) = cert.key_usage {
                if usage & KU_KEY_CERT_SIGN == 0 {
                    return Err("Certificate issuer is not allowed to sign certificates".into());
                }
            }
            let self_issued = cert.subject == cert.issuer;
            if !self_issued {
                if max_path == 0 {
                    return Err("Certificate path is longer than allowed".into());
                }
                max_path -= 1;
            }
            if let Some(limit) = cert.path_len {
                max_path = max_path.min(limit);
            }
            if cert.constraints.has_any() {
                constraints.push(&cert.constraints);
            }
        } else if let Some(usage) = cert.key_usage {
            if usage & KU_DIGITAL_SIGNATURE == 0 {
                return Err("Certificate is not allowed to sign the handshake".into());
            }
        }
        if let Some(eku) = &cert.eku {
            if !eku_allows_server(eku) {
                return Err("Certificate is not allowed for this service".into());
            }
        }
    }
    Ok(())
}

impl NameConstraints {
    fn has_any(&self) -> bool {
        !self.permitted_dns.is_empty() || !self.excluded_dns.is_empty() || !self.permitted_ip.is_empty() || !self.excluded_ip.is_empty() || !self.permitted_dn.is_empty() || !self.excluded_dn.is_empty()
    }
    fn allows(&self, cert: &Certificate) -> Result<(), String> {
        for name in &cert.names {
            match name {
                Name::Dns(dns) => {
                    let dns_rules = !self.permitted_dns.is_empty() || !self.excluded_dns.is_empty();
                    if dns_rules && presented_dns(dns).is_none() {
                        return Err("Certificate name is not permitted".into());
                    }
                    if self.excluded_dns.iter().any(|base| dns_constrained(dns, base)) {
                        return Err("Certificate name is excluded".into());
                    }
                    if !self.permitted_dns.is_empty() && !self.permitted_dns.iter().any(|base| dns_constrained(dns, base)) {
                        return Err("Certificate name is not permitted".into());
                    }
                }
                Name::Ip(addr) => {
                    if self.excluded_ip.iter().any(|(base, mask)| ip_constrained(addr, base, mask)) {
                        return Err("Certificate address is excluded".into());
                    }
                    if !self.permitted_ip.is_empty() && !self.permitted_ip.iter().any(|(base, mask)| ip_constrained(addr, base, mask)) {
                        return Err("Certificate address is not permitted".into());
                    }
                }
            }
        }
        if self.excluded_dn.iter().any(|base| dn_within(&cert.subject, base)) || self.excluded_dn.iter().any(|base| dn_within(&cert.issuer, base)) {
            return Err("Certificate name is excluded".into());
        }
        if !self.permitted_dn.is_empty() && !self.permitted_dn.iter().any(|base| dn_within(&cert.subject, base)) {
            return Err("Certificate name is not permitted".into());
        }
        Ok(())
    }
}

fn dns_constraint_name(text: &str) -> Result<String, String> {
    let lower = text.to_ascii_lowercase();
    let (subdomains_only, rest) = if let Some(rest) = lower.strip_prefix('.') {
        (true, rest)
    } else {
        (false, lower.as_str())
    };
    let rest = rest.strip_suffix('.').unwrap_or(rest);
    if !dns_labels_ok(rest) {
        return Err("Certificate name constraint is not supported".into());
    }
    if subdomains_only {
        Ok(format!(".{rest}"))
    } else {
        Ok(rest.to_string())
    }
}

fn dns_labels_ok(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.ends_with('.') && !name.split('.').any(|label| label.is_empty()) && name.is_ascii()
}

fn presented_dns(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let trimmed = lower.strip_suffix('.').unwrap_or(&lower);
    if dns_labels_ok(trimmed) { Some(trimmed.to_string()) } else { None }
}

fn dns_constrained(name: &str, base: &str) -> bool {
    let name = match presented_dns(name) {
        Some(name) => name,
        None => return false,
    };
    let subdomains_only = base.starts_with('.');
    let base = base.strip_prefix('.').unwrap_or(base);
    let base = base.strip_suffix('.').unwrap_or(base);
    if !dns_labels_ok(base) {
        return false;
    }
    if name == base {
        return !subdomains_only;
    }
    let name_bytes = name.as_bytes();
    let base_bytes = base.as_bytes();
    name_bytes.len() > base_bytes.len() && name_bytes[name_bytes.len() - base_bytes.len() - 1] == b'.' && &name_bytes[name_bytes.len() - base_bytes.len()..] == base_bytes
}

fn ip_constrained(addr: &[u8], base: &[u8], mask: &[u8]) -> bool {
    addr.len() == base.len() && addr.len() == mask.len() && addr.iter().zip(base).zip(mask).all(|((left, right), bit)| left & bit == right & bit)
}

fn dn_within(subject: &[u8], constraint: &[u8]) -> bool {
    let subject_sets = rdn_sets(subject);
    let constraint_body = if constraint.first() == Some(&0x30) {
        Der::new(constraint).sequence_bytes().unwrap_or(constraint)
    } else {
        constraint
    };
    let constraint_sets = rdn_sets(constraint_body);
    !constraint_sets.is_empty() && constraint_sets.len() <= subject_sets.len() && constraint_sets.iter().zip(subject_sets.iter()).all(|(left, right)| left == right)
}

fn rdn_sets(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut cursor = Der::new(bytes);
    let mut sets = Vec::new();
    while cursor.remaining() > 0 {
        let start = cursor.at;
        if cursor.any().is_err() {
            break;
        }
        sets.push(cursor.data[start..cursor.at].to_vec());
    }
    sets
}

fn verify_signed(issuer: &Certificate, subject: &Certificate) -> Result<(), String> {
    let hash = match subject.signature_alg {
        SigAlg::RsaSha256 | SigAlg::EcdsaSha256 | SigAlg::Pss(PssParams { hash: HashKind::Sha256, .. }) => hash_bytes(HashKind::Sha256, &subject.tbs),
        SigAlg::RsaSha384 | SigAlg::EcdsaSha384 | SigAlg::Pss(PssParams { hash: HashKind::Sha384, .. }) => hash_bytes(HashKind::Sha384, &subject.tbs),
        SigAlg::RsaSha512 | SigAlg::Pss(PssParams { hash: HashKind::Sha512, .. }) => hash_bytes(HashKind::Sha512, &subject.tbs),
    };
    match (&issuer.public, subject.signature_alg) {
        (PublicKey::Rsa { n, e }, SigAlg::RsaSha256 | SigAlg::RsaSha384 | SigAlg::RsaSha512) => verify_pkcs1(n, e, subject.signature_alg, &hash, &subject.signature),
        (PublicKey::Rsa { n, e }, SigAlg::Pss(params)) => verify_pss(n, e, params, &subject.tbs, &subject.signature),
        (PublicKey::P256 { point }, SigAlg::EcdsaSha256 | SigAlg::EcdsaSha384) => verify_ecdsa(point, &hash, &subject.signature),
        _ => Err("Certificate signature does not match the issuer key".into()),
    }
}

fn verify_pkcs1(n: &Nat, e: &Nat, alg: SigAlg, hash: &[u8], signature: &[u8]) -> Result<(), String> {
    rsa_exponent_ok(e)?;
    let decoded = Nat::from_be(signature).modpow(e, n).to_be(n.to_be(0).len().max(signature.len()));
    let encoded = pkcs1_digest(alg, hash, decoded.len())?;
    if !equal(&decoded, &encoded) {
        return Err("Certificate signature is not valid".into());
    }
    Ok(())
}

fn pkcs1_digest(alg: SigAlg, hash: &[u8], em_len: usize) -> Result<Vec<u8>, String> {
    let prefix: &[u8] = match alg {
        SigAlg::RsaSha256 => &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20],
        SigAlg::RsaSha384 => &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30],
        SigAlg::RsaSha512 => &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40],
        _ => return Err("Certificate signature is not valid".into()),
    };
    let mut digest_info = prefix.to_vec();
    digest_info.extend_from_slice(hash);
    if em_len < digest_info.len() + 11 {
        return Err("Certificate signature is not valid".into());
    }
    let mut em = vec![0xffu8; em_len];
    em[0] = 0x00;
    em[1] = 0x01;
    let fill = em_len - digest_info.len() - 3;
    for byte in &mut em[2..2 + fill] { *byte = 0xff; }
    em[2 + fill] = 0x00;
    em[3 + fill..].copy_from_slice(&digest_info);
    Ok(em)
}

fn verify_pss(n: &Nat, e: &Nat, params: PssParams, message: &[u8], signature: &[u8]) -> Result<(), String> {
    rsa_exponent_ok(e)?;
    let hash = params.hash;
    let hlen = hash_len(hash);
    let modulus_bits = n.bit_len();
    if modulus_bits < 2 {
        return Err("Certificate signature is not valid".into());
    }
    let em_bits = modulus_bits - 1;
    let em_len = (em_bits + 7) / 8;
    let modulus_octets = (modulus_bits + 7) / 8;
    let wide = Nat::from_be(signature).modpow(e, n).to_be(modulus_octets);
    let leading = modulus_octets.checked_sub(em_len).ok_or("Certificate signature is not valid")?;
    if wide[..leading].iter().any(|byte| *byte != 0) {
        return Err("Certificate signature is not valid".into());
    }
    let em = wide[leading..].to_vec();
    let minimum = hlen.checked_add(params.salt_len).and_then(|sum| sum.checked_add(2)).ok_or("Certificate signature is not valid")?;
    if em.len() < minimum || em.last() != Some(&0xbc) {
        return Err("Certificate signature is not valid".into());
    }
    let (masked, rest) = em.split_at(em.len() - hlen - 1);
    let hash_value = &rest[..hlen];
    let unused = pss_leading_bits(em.len(), modulus_bits)?;
    pss_leading_zeros(masked, unused)?;
    let mut db = mgf1(params.mgf, hash_value, masked.len())?;
    for index in 0..masked.len() { db[index] ^= masked[index]; }
    pss_clear_leading(&mut db, unused)?;
    let salt = pss_salt(&db, params.salt_len)?;
    let mut prefix = vec![0u8; 8];
    prefix.extend_from_slice(&hash_bytes(hash, message));
    prefix.extend_from_slice(salt);
    if !equal(hash_value, &hash_bytes(hash, &prefix)) {
        return Err("Certificate signature is not valid".into());
    }
    Ok(())
}

fn pss_leading_bits(em_len: usize, modulus_bits: usize) -> Result<usize, String> {
    let em_bits = modulus_bits.checked_sub(1).ok_or("Certificate signature is not valid")?;
    let width = em_len.checked_mul(8).ok_or("Certificate signature is not valid")?;
    if width < em_bits {
        return Err("Certificate signature is not valid".into());
    }
    Ok(width - em_bits)
}

fn pss_leading_zeros(bytes: &[u8], unused: usize) -> Result<(), String> {
    let full = unused / 8;
    let rest = unused % 8;
    if full > bytes.len() || (rest > 0 && full >= bytes.len()) {
        return Err("Certificate signature is not valid".into());
    }
    if bytes[..full].iter().any(|byte| *byte != 0) || (rest > 0 && bytes[full] & !(0xffu8 >> rest) != 0) {
        return Err("Certificate signature is not valid".into());
    }
    Ok(())
}

fn pss_clear_leading(bytes: &mut [u8], unused: usize) -> Result<(), String> {
    let full = unused / 8;
    let rest = unused % 8;
    if full > bytes.len() || (rest > 0 && full >= bytes.len()) {
        return Err("Certificate signature is not valid".into());
    }
    for byte in &mut bytes[..full] {
        *byte = 0;
    }
    if rest > 0 {
        bytes[full] &= 0xff >> rest;
    }
    Ok(())
}

fn pss_salt(db: &[u8], salt_len: usize) -> Result<&[u8], String> {
    let separator = db.len().checked_sub(salt_len).and_then(|end| end.checked_sub(1)).ok_or("Certificate signature is not valid")?;
    if db.get(separator) != Some(&0x01) || db[..separator].iter().any(|byte| *byte != 0) {
        return Err("Certificate signature is not valid".into());
    }
    db.get(separator + 1..).ok_or_else(|| "Certificate signature is not valid".to_string())
}

fn mgf1(hash: HashKind, seed: &[u8], length: usize) -> Result<Vec<u8>, String> {
    if length > 512 {
        return Err("Certificate signature is not valid".into());
    }
    let mut out = Vec::new();
    let mut counter = 0u32;
    while out.len() < length {
        let mut input = seed.to_vec();
        input.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&hash_bytes(hash, &input));
        counter += 1;
    }
    out.truncate(length);
    Ok(out)
}

fn verify_ecdsa(point: &Point, hash: &[u8], signature: &[u8]) -> Result<(), String> {
    let (r, s) = if signature.len() == 64 {
        (Nat::from_be(&signature[..32]), Nat::from_be(&signature[32..]))
    } else {
        let mut der = Der::new(signature);
        let seq = der.sequence_bytes()?;
        let mut body = Der::new(seq);
        (Nat::from_be(&body.integer()?), Nat::from_be(&body.integer()?))
    };
    let order = Nat::from_be(&P256_N);
    if r.is_zero() || s.is_zero() || r.cmp(&order) != std::cmp::Ordering::Less || s.cmp(&order) != std::cmp::Ordering::Less {
        return Err("Certificate signature is not valid".into());
    }
    let e = Nat::from_be(hash);
    let w = s.mod_inv(&order)?;
    let u1 = e.mod_mul(&w, &order);
    let u2 = r.mod_mul(&w, &order);
    let p1 = p256_mul(&u1_bytes(&u1), &P256_G_POINT)?;
    let p2 = p256_mul(&u1_bytes(&u2), point)?;
    let point = p256_add(&p1, &p2)?;
    if point.inf || point.x.divmod(&order).1.cmp(&r) != std::cmp::Ordering::Equal {
        return Err("Certificate signature is not valid".into());
    }
    Ok(())
}

fn u1_bytes(value: &Nat) -> [u8; 32] {
    value.to_be(32).try_into().unwrap_or([0u8; 32])
}

fn hostname_matches(host: &str, cert: &Certificate) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    let ip = parse_ip(&host);
    let mut saw_name = false;
    for name in &cert.names {
        saw_name = true;
        match name {
            Name::Dns(dns) if ip.is_none() && dns_match(dns, &host) => return true,
            Name::Ip(bytes) if ip.as_ref() == Some(bytes) => return true,
            _ => {}
        }
    }
    let _ = saw_name;
    false
}

fn dns_match(pattern: &str, host: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix("*.") {
        let Some((first, tail)) = host.split_once('.') else { return false };
        return !first.is_empty() && !first.contains('*') && tail == rest && !rest.is_empty();
    }
    pattern == host
}

fn parse_ip(host: &str) -> Option<Vec<u8>> {
    if let Ok(addr) = host.parse::<std::net::Ipv4Addr>() {
        return Some(addr.octets().to_vec());
    }
    if let Ok(addr) = host.parse::<std::net::Ipv6Addr>() {
        return Some(addr.octets().to_vec());
    }
    None
}

#[allow(dead_code)]
fn subject_cn(subject: &[u8]) -> Option<String> {
    let mut seq = Der::new(subject);
    while seq.remaining() > 0 {
        let set = seq.tagged().ok()?;
        if set.0 != 0x31 { continue; }
        let mut item = Der::new(set.1);
        let pair = item.sequence_bytes().ok()?;
        let mut pair_der = Der::new(pair);
        let oid = pair_der.oid().ok()?;
        if oid == [0x55, 0x04, 0x03] {
            let (tag, data) = pair_der.tagged().ok()?;
            if tag == 0x13 || tag == 0x0c || tag == 0x16 {
                return std::str::from_utf8(data).ok().map(str::to_owned);
            }
        }
    }
    None
}

fn verify_certificate_signature_13(leaf: &Certificate, message: &[u8], transcript_hash: &[u8]) -> Result<(), String> {
    let body = message_body(message)?;
    if body.len() < 4 {
        return Err("TLS certificate signature is truncated".into());
    }
    let scheme = u16::from_be_bytes([body[0], body[1]]);
    let len = u16::from_be_bytes([body[2], body[3]]) as usize;
    let sig_end = 4usize.checked_add(len).ok_or("TLS certificate signature is truncated")?;
    if sig_end != body.len() {
        return Err("TLS certificate signature is truncated".into());
    }
    let signature = body.get(4..sig_end).ok_or("TLS certificate signature is truncated")?;
    let mut content = vec![0x20u8; 64];
    content.extend_from_slice(b"TLS 1.3, server CertificateVerify");
    content.push(0);
    content.extend_from_slice(transcript_hash);
    let alg = match scheme {
        0x0403 => SigAlg::EcdsaSha256,
        0x0503 => SigAlg::EcdsaSha384,
        0x0804 => pss_scheme(HashKind::Sha256),
        0x0805 => pss_scheme(HashKind::Sha384),
        _ => return Err("TLS certificate signature scheme is not supported".into()),
    };
    let fake = Certificate {
        signature_alg: alg,
        signature: signature.to_vec(),
        tbs: content,
        ..leaf.clone()
    };
    // verify_signed hashes tbs. For PSS, verify_pss hashes the message itself, so tbs must be the content.
    let issuer_key = Certificate { public: leaf.public.clone(), ..leaf.clone() };
    verify_signed(&issuer_key, &fake)
}

fn verify_server_key_exchange(leaf: &Certificate, transcript: &[u8], message: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let body = message_body(message)?;
    if body.len() < 4 {
        return Err("TLS key exchange is truncated".into());
    }
    let curve = u8::from(body[0]);
    if curve != 3 {
        return Err("TLS key exchange is not elliptic-curve".into());
    }
    let group = u16::from_be_bytes([body[1], body[2]]);
    if group != 0x001d && group != 0x0017 {
        return Err("TLS server selected no offered group".into());
    }
    let point_len = body[3] as usize;
    let point_end = 4usize.checked_add(point_len).ok_or("TLS key exchange is truncated")?;
    let point = body.get(4..point_end).ok_or("TLS key exchange is truncated")?.to_vec();
    let mut at = point_end;
    let scheme_or_hash = *body.get(at).ok_or("TLS key exchange signature is truncated")?;
    let sig_alg = *body.get(at + 1).ok_or("TLS key exchange signature is truncated")?;
    at += 2;
    let sig_len = u16::from_be_bytes([*body.get(at).ok_or("TLS key exchange signature is truncated")?, *body.get(at + 1).ok_or("TLS key exchange signature is truncated")?]) as usize;
    at += 2;
    let sig_end = at.checked_add(sig_len).ok_or("TLS key exchange signature is truncated")?;
    let signature = body.get(at..sig_end).ok_or("TLS key exchange signature is truncated")?;
    let mut signed = hello_random(transcript, true)?.to_vec();
    signed.extend_from_slice(hello_random(transcript, false)?);
    signed.extend_from_slice(&body[..point_end]);
    let alg = match (scheme_or_hash, sig_alg) {
        (4, 1) => SigAlg::RsaSha256,
        (5, 1) => SigAlg::RsaSha384,
        (6, 1) => SigAlg::RsaSha512,
        (4, 3) => SigAlg::EcdsaSha256,
        (5, 3) => SigAlg::EcdsaSha384,
        _ => return Err("TLS key exchange signature is not supported".into()),
    };
    let subject = Certificate { signature_alg: alg, signature: signature.to_vec(), tbs: signed, ..leaf.clone() };
    verify_signed(leaf, &subject)?;
    Ok((group, point))
}

fn trust_anchors() -> Result<Vec<Certificate>, String> {
    let mut bytes = Vec::new();
    for path in [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        "/etc/ssl/cert.pem",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
        "/private/etc/ssl/cert.pem",
        "/etc/ssl/certs/ca-bundle.crt",
    ] {
        if let Ok(file) = std::fs::read(path) {
            if bytes.len().saturating_add(file.len()) > 8 * 1024 * 1024 {
                break;
            }
            bytes.extend_from_slice(&file);
            bytes.extend_from_slice(b"\n");
        }
    }
    if let Ok(entries) = std::fs::read_dir("/etc/ssl/certs") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("pem") || path.extension().and_then(|ext| ext.to_str()) == Some("crt") {
                if let Ok(file) = std::fs::read(&path) {
                    if bytes.len().saturating_add(file.len()) > 8 * 1024 * 1024 { break; }
                    bytes.extend_from_slice(&file);
                    bytes.extend_from_slice(b"\n");
                }
            }
        }
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut anchors = Vec::new();
    for part in text.split("-----END CERTIFICATE-----") {
        let Some(start) = part.find("-----BEGIN CERTIFICATE-----") else { continue };
        let body = part[start + "-----BEGIN CERTIFICATE-----".len()..].chars().filter(|c| !c.is_whitespace()).collect::<String>();
        if body.is_empty() { continue; }
        let der = b64_decode(&body)?;
        if let Ok(cert) = parse_certificate(&der) {
            anchors.push(cert);
        }
        if anchors.len() >= 600 { break; }
    }
    Ok(anchors)
}

fn b64_decode(text: &str) -> Result<Vec<u8>, String> {
    fn value(byte: u8) -> Result<u8, String> {
        match byte {
            b'A'..=b'Z' => Ok(byte - b'A'),
            b'a'..=b'z' => Ok(byte - b'a' + 26),
            b'0'..=b'9' => Ok(byte - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("Trust anchor encoding is invalid".into()),
        }
    }
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        let a = value(bytes[index])?;
        let b = value(bytes[index + 1])?;
        let c = if bytes[index + 2] == b'=' { 0 } else { value(bytes[index + 2])? };
        let d = if bytes[index + 3] == b'=' { 0 } else { value(bytes[index + 3])? };
        out.push((a << 2) | (b >> 4));
        if bytes[index + 2] != b'=' { out.push((b << 4) | (c >> 2)); }
        if bytes[index + 3] != b'=' { out.push((c << 6) | d); }
        index += 4;
    }
    Ok(out)
}

fn equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() { return false; }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) { diff |= a ^ b; }
    diff == 0
}

fn crypto_self_check() -> Result<(), String> {
    static READY: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    READY.get_or_init(run_crypto_self_check).clone()
}

fn run_crypto_self_check() -> Result<(), String> {
    let sha = sha256(b"abc");
    if sha != [0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad] {
        return Err("TLS hash self-check failed".into());
    }
    let key = [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f];
    let block = [0x00u8, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
    let cipher = aes_encrypt_block(&key, &block)?;
    if cipher != [0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4, 0xc5, 0x5a] {
        return Err("TLS cipher self-check failed".into());
    }
    let zeros = [0u8; 16];
    let nonce = [0u8; 12];
    let sealed = aes_gcm_seal(&zeros, &nonce, b"", b"")?;
    if sealed != [0x58, 0xe2, 0xfc, 0xce, 0xfa, 0x7e, 0x30, 0x61, 0x36, 0x7f, 0x1d, 0x57, 0xa4, 0xe7, 0x45, 0x5a] {
        return Err("TLS authentication self-check failed".into());
    }
    let scalar: [u8; 32] = hex_bytes("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")?.try_into().map_err(|_| "TLS key agreement self-check failed".to_string())?;
    let point: [u8; 32] = hex_bytes("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")?.try_into().map_err(|_| "TLS key agreement self-check failed".to_string())?;
    let shared = x25519(&scalar, &point);
    let expect = hex_bytes("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742")?;
    if shared.as_slice() != expect.as_slice() {
        return Err("TLS key agreement self-check failed".into());
    }
    if Nat::from_be(&[2]).modpow(&Nat::from_be(&[10]), &Nat::from_be(&1000u32.to_be_bytes())).cmp(&Nat::from_be(&24u32.to_be_bytes())) != std::cmp::Ordering::Equal {
        return Err("TLS signature arithmetic self-check failed".into());
    }
    let g = p256_decode(&p256_uncompressed(&P256_G_POINT))?;
    if g.x.cmp(&P256_G_POINT.x) != std::cmp::Ordering::Equal {
        return Err("TLS curve self-check failed".into());
    }
    let mut one = [0u8; 32];
    one[31] = 1;
    if p256_public(&one)? != p256_uncompressed(&P256_G_POINT) {
        return Err("TLS curve scalar self-check failed".into());
    }
    let mut two = [0u8; 32];
    two[31] = 2;
    let doubled = p256_mul(&two, &P256_G_POINT)?;
    if doubled.inf || !p256_on_curve(&doubled) || doubled.x.cmp(&P256_G_POINT.x) == std::cmp::Ordering::Equal {
        return Err("TLS curve double self-check failed".into());
    }
    Ok(())
}

fn hex_bytes(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 { return Err("bad hex".into()); }
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let value = std::str::from_utf8(&bytes[index..index + 2]).ok().and_then(|s| u8::from_str_radix(s, 16).ok()).ok_or("bad hex")?;
        out.push(value);
        index += 2;
    }
    Ok(out)
}

const P256_P: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];
const P256_N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];
const P256_B: [u8; 32] = [
    0x5a, 0xc6, 0x35, 0xd8, 0xaa, 0x3a, 0x93, 0xe7, 0xb3, 0xeb, 0xbd, 0x55, 0x76, 0x98, 0x86, 0xbc, 0x65, 0x1d, 0x06, 0xb0, 0xcc, 0x53, 0xb0, 0xf6, 0x3b, 0xce, 0x3c, 0x3e, 0x27, 0xd2, 0x60, 0x4b,
];
const P256_GX: [u8; 32] = [
    0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98, 0xc2, 0x96,
];
const P256_GY: [u8; 32] = [
    0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e, 0x16, 0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf, 0x51, 0xf5,
];

fn p256_g_point() -> Point {
    Point { x: Nat::from_be(&P256_GX), y: Nat::from_be(&P256_GY), inf: false }
}

// Lazy point so the constant tables stay simple.
use std::sync::OnceLock;
fn p256_g() -> &'static Point {
    static G: OnceLock<Point> = OnceLock::new();
    G.get_or_init(p256_g_point)
}

// Alias used above before the helper existed in source order. The const path uses this name.
const P256_G: PointRef = PointRef;
struct PointRef;
impl std::ops::Deref for PointRef {
    type Target = Point;
    fn deref(&self) -> &Point { p256_g() }
}

// The curve self-check and multiplication refer to P256_G_POINT.
fn p256_g_static() -> Point { p256_g().clone() }

// Provide the name used in the handshake.
static P256_G_POINT_CELL: std::sync::OnceLock<Point> = OnceLock::new();
fn p256_g_point_cell() -> &'static Point {
    P256_G_POINT_CELL.get_or_init(p256_g_point)
}

// Macro-like alias via a function used by the code. The identifiers P256_G and P256_G_POINT
// are referenced above. Bind them with type aliases that don't work for values.
// The references in this file use P256_G and P256_G_POINT. Define them as functions is invalid
// in expression position. Use a static.

static P256_G_POINT: OncePoint = OncePoint;
struct OncePoint;
impl std::ops::Deref for OncePoint {
    type Target = Point;
    fn deref(&self) -> &Point { p256_g_point_cell() }
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const SHA512_K: [u64; 80] = [
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

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76, 0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15, 0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84, 0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8, 0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73, 0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79, 0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a, 0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf, 0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const RCON: [u8; 11] = [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

#[cfg(any(test, rllm_module_tests))]
pub mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    #[cfg(rllm_module_tests)]
    pub fn g1_tls_client_tests() {
        a_crypto_self_check_still_passes();
        x25519_rejects_all_zero_shared_secret();
        p256_scalar_rejects_zero_and_order_and_does_not_replace_them();
        sbox_matches_every_entry();
        poly1305_and_chacha_match_published_vectors();
        finished_compare_rejects_one_bit_difference();
        tls13_padding_scans_through_zeros();
        tls12_server_hello_needs_no_key_share_and_downgrade_is_rejected();
        hello_retry_hashes_the_first_hello_and_sends_one_group();
        handshake_messages_reassemble_and_reject_oversized_lengths();
        application_secrets_use_the_transcript_through_server_finished();
        certificate_time_rejects_impossible_days_without_panicking();
        rsa_exponent_is_bounded_before_exponentiation();
        signatures_cover_the_full_certificate_encoding();
        trust_anchor_is_selected_before_signatures();
        path_constraints_key_use_and_names_are_enforced();
        certificate_verify_stops_at_the_certificate();
        key_usage_rejects_bits_declared_unused();
        tls13_compatibility_change_cipher_spec_is_not_decrypted();
        key_update_rotates_the_next_read_generation();
        pss_accepts_a_salt_that_contains_the_separator();
        pss_sha384_certificate_verifies_and_pss_sha256_still_verifies();
        ip_literals_are_omitted_from_the_server_name();
        duplicate_certificate_extensions_are_rejected();
        server_hello_selections_stay_inside_the_offer_and_hello_retry();
        record_sequence_does_not_wrap_under_the_same_keys();
        empty_read_returns_without_waiting();
        leading_dot_dns_constraints_are_subdomains_only();
        plaintext_records_keep_the_fragment_limit();
        change_cipher_spec_requires_the_single_byte_one();
        close_notify_is_exactly_two_bytes();
    }

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

    fn der_seq(body: &[u8]) -> Vec<u8> { der_tlv(0x30, body) }

    fn der_oid(body: &[u8]) -> Vec<u8> { der_tlv(0x06, body) }

    fn der_int(bytes: &[u8]) -> Vec<u8> {
        let mut start = 0;
        while start + 1 < bytes.len() && bytes[start] == 0 && bytes[start + 1] & 0x80 == 0 {
            start += 1;
        }
        let raw: &[u8] = if bytes.is_empty() { &[0] } else { &bytes[start..] };
        let mut body = Vec::new();
        if raw[0] & 0x80 != 0 {
            body.push(0);
        }
        body.extend_from_slice(raw);
        der_tlv(0x02, &body)
    }

    fn der_nat(value: &Nat) -> Vec<u8> { der_int(&value.to_be(32)) }

    fn ecdsa_sign(message: &[u8]) -> Vec<u8> {
        let order = Nat::from_be(&P256_N);
        let mut k = [0u8; 32];
        k[31] = 7;
        let point = p256_mul(&k, &P256_G_POINT).expect("scalar");
        let r = point.x.divmod(&order).1;
        let e = Nat::from_be(&sha256(message)).divmod(&order).1;
        let d = Nat::from_be(&[1]);
        let s = Nat::from_be(&k).mod_inv(&order).expect("inverse").mod_mul(&e.add(&r.mod_mul(&d, &order)), &order);
        der_seq(&[der_nat(&r), der_nat(&s)].concat())
    }

    fn ecdsa_alg() -> Vec<u8> {
        der_seq(&der_oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02]))
    }

    fn p256_spki() -> Vec<u8> {
        let mut alg = der_oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01]);
        alg.extend(der_oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]));
        let alg = der_seq(&alg);
        let mut bits = vec![0];
        bits.extend(p256_uncompressed(&P256_G_POINT));
        let mut body = alg;
        body.extend(der_tlv(0x03, &bits));
        der_seq(&body)
    }

    fn dn(cn: &str) -> Vec<u8> {
        let mut pair = der_oid(&[0x55, 0x04, 0x03]);
        pair.extend(der_tlv(0x0c, cn.as_bytes()));
        let set = der_tlv(0x31, &der_seq(&pair));
        der_seq(&set)
    }

    fn validity() -> Vec<u8> {
        let mut body = der_tlv(0x18, b"20200101000000Z");
        body.extend(der_tlv(0x18, b"20300101000000Z"));
        der_seq(&body)
    }

    fn extension(oid: &[u8], critical: bool, value: &[u8]) -> Vec<u8> {
        let mut body = der_oid(oid);
        if critical {
            body.extend(der_tlv(0x01, &[0xff]));
        }
        body.extend(der_tlv(0x04, value));
        der_seq(&body)
    }

    fn extensions(items: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for item in items {
            body.extend(item);
        }
        der_tlv(0xa3, &der_seq(&body))
    }

    fn bc(ca: bool, path: Option<u8>) -> Vec<u8> {
        let mut body = der_tlv(0x01, &[if ca { 0xff } else { 0x00 }]);
        if let Some(path) = path {
            body.extend(der_int(&[path]));
        }
        extension(&[0x55, 0x1d, 0x13], true, &der_seq(&body))
    }

    fn ku(bits: u8, unused: u8) -> Vec<u8> {
        extension(&[0x55, 0x1d, 0x0f], true, &der_tlv(0x03, &[unused, bits]))
    }

    fn eku(oid: &[u8]) -> Vec<u8> {
        extension(&[0x55, 0x1d, 0x25], false, &der_seq(&der_oid(oid)))
    }

    fn server_eku() -> Vec<u8> { eku(&[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01]) }

    fn san_dns(name: &str) -> Vec<u8> {
        extension(&[0x55, 0x1d, 0x11], false, &der_seq(&der_tlv(0x82, name.as_bytes())))
    }

    fn nc_dns(permit: bool, name: &str, minimum_zero: bool) -> Vec<u8> {
        let mut sub = der_tlv(0x82, name.as_bytes());
        if minimum_zero {
            sub.extend(der_tlv(0x80, &[0x00]));
        }
        let list = der_seq(&der_seq(&sub));
        let tagged = der_tlv(if permit { 0xa0 } else { 0xa1 }, &list);
        extension(&[0x55, 0x1d, 0x1e], true, &der_seq(&tagged))
    }

    fn certify(issuer: &[u8], subject: &[u8], exts: &[Vec<u8>]) -> Vec<u8> {
        let mut body = der_tlv(0xa0, &der_int(&[2]));
        body.extend(der_int(&[1]));
        body.extend(ecdsa_alg());
        body.extend_from_slice(issuer);
        body.extend(validity());
        body.extend_from_slice(subject);
        body.extend(p256_spki());
        if !exts.is_empty() {
            body.extend(extensions(exts));
        }
        let tbs = der_seq(&body);
        let sig = ecdsa_sign(&tbs);
        let mut bits = vec![0];
        bits.extend(sig);
        let mut cert = tbs;
        cert.extend(ecdsa_alg());
        cert.extend(der_tlv(0x03, &bits));
        der_seq(&cert)
    }

    fn leaf_exts(name: &str) -> Vec<Vec<u8>> {
        vec![san_dns(name), ku(0x80, 7), server_eku()]
    }

    fn ca_exts(path: Option<u8>) -> Vec<Vec<u8>> {
        vec![bc(true, path), ku(0x04, 2)]
    }

    fn parsed(issuer: &[u8], subject: &[u8], exts: &[Vec<u8>]) -> Certificate {
        let der = certify(issuer, subject, exts);
        parse_certificate(&der).expect("certificate")
    }

    fn root_anchor() -> (Vec<u8>, Certificate) {
        let root = dn("Root");
        let anchor = parsed(&root, &root, &ca_exts(None));
        (root, anchor)
    }

    fn peer() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("port").port();
        let client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let (server, _) = listener.accept().expect("accept");
        (client, server)
    }

    fn server_hello_12(random: &[u8; 32]) -> Vec<u8> {
        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(random);
        body.push(0);
        body.extend_from_slice(&0xc02fu16.to_be_bytes());
        body.push(0);
        let mut message = vec![2, 0, 0, body.len() as u8];
        message.extend_from_slice(&body);
        message
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn a_crypto_self_check_still_passes() {
        run_crypto_self_check().expect("self-check");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn x25519_rejects_all_zero_shared_secret() {
        let mut scalar = [0u8; 32];
        scalar[0] = 9;
        let zero_point = x25519(&scalar, &[0u8; 32]);
        assert!(x25519_shared(&scalar, &[0u8; 32]).is_err(), "shared {zero_point:?}");
        let mut one = [0u8; 32];
        one[0] = 1;
        assert!(x25519_shared(&scalar, &one).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn p256_scalar_rejects_zero_and_order_and_does_not_replace_them() {
        assert!(p256_scalar(&[0u8; 32]).is_err());
        assert!(p256_scalar(&P256_N).is_err());
        let mut one = [0u8; 32];
        one[31] = 1;
        assert_eq!(p256_scalar(&one).expect("one"), one);
        let one_m = p256_to_mont(&[1, 0, 0, 0]);
        let back = p256_from_mont(&one_m);
        assert_eq!(back, [1, 0, 0, 0], "mont1 {:?} rr {:?}", one_m, p256_rr());
        let gx = nat_to_limbs(&P256_G_POINT.x);
        let gx_back = p256_from_mont(&p256_to_mont(&gx));
        assert_eq!(gx_back, gx, "Gx mont");
        let inv = p256_inv(&one_m);
        assert_eq!(inv, one_m, "inv one");
        let round = Proj::from_affine(&P256_G_POINT).expect("affine").to_affine().expect("back");
        assert_eq!(round.x.to_be(32), P256_GX.to_vec(), "roundtrip x");
        let doubled_inf = Proj::infinity().double().to_affine().expect("double inf");
        assert!(doubled_inf.inf, "infinity doubled x {:?} z-check", doubled_inf.x.to_be(8));
        let sum = Proj::infinity().add(&Proj::from_affine(&P256_G_POINT).expect("g")).to_affine().expect("sum");
        assert_eq!(sum.x.to_be(32), P256_GX.to_vec(), "infinity plus generator");
        let mut two = [0u8; 32];
        two[31] = 2;
        let doubled = p256_mul(&two, &P256_G_POINT).expect("double");
        let x = hex_bytes("7cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc47669978").expect("x");
        let y = hex_bytes("07775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1").expect("y");
        assert_eq!(doubled.x.to_be(32), x);
        assert_eq!(doubled.y.to_be(32), y);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn sbox_matches_every_entry() {
        for index in 0..256 {
            assert_eq!(sbox(index as u8), SBOX[index]);
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn poly1305_and_chacha_match_published_vectors() {
        let key = hex_bytes("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b").expect("key");
        let tag = poly1305_mac(&key, b"Cryptographic Forum Research Group");
        assert_eq!(hex_bytes("a8061dc1305136c6c22b8baf0c0127a9").expect("tag"), tag);
        let aead_key: [u8; 32] = hex_bytes("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f").expect("aead").try_into().expect("32");
        let nonce: [u8; 12] = hex_bytes("070000004041424344454647").expect("nonce").try_into().expect("12");
        let aad = hex_bytes("50515253c0c1c2c3c4c5c6c7").expect("aad");
        let plain = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let sealed = chacha_seal(&aead_key, &nonce, &aad, plain).expect("seal");
        assert_eq!(sealed.len(), plain.len() + 16);
        assert_eq!(&sealed[sealed.len() - 16..], hex_bytes("1ae10b594f09e26a7e902ecbd0600691").expect("aead tag").as_slice());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn finished_compare_rejects_one_bit_difference() {
        assert!(equal(b"abc", b"abc"));
        assert!(!equal(b"abc", b"abd"));
        assert!(!equal(b"ab", b"abc"));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn tls13_padding_scans_through_zeros() {
        let (kind, payload) = tls13_content(b"hello\x16\x00\x00").expect("padding");
        assert_eq!(kind, 0x16);
        assert_eq!(payload, b"hello");
        assert!(tls13_content(&[0, 0, 0]).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn tls12_server_hello_needs_no_key_share_and_downgrade_is_rejected() {
        let mut random = [0x11u8; 32];
        let (legacy, suite, share, version) = parse_server_hello(&server_hello_12(&random)[4..]).expect("hello");
        assert_eq!(legacy, 0x0303);
        assert_eq!(suite, 0xc02f);
        assert!(share.is_none());
        assert_eq!(version, 0x0303);
        random[24..].copy_from_slice(b"DOWNGRD\x01");
        assert!(downgrade_sentinel(&random));
        let mut below = [0x11u8; 32];
        below[24..].copy_from_slice(b"DOWNGRD\x00");
        assert!(downgrade_sentinel(&below));
        assert!(!downgrade_sentinel(&[0x11u8; 32]));
        let (mut client, server) = peer();
        drop(server);
        let hello = server_hello_12(&random);
        let err = finish_server_hello(&mut client, "example.com", &[], &[], &hello, None, &[9u8; 32], &[9u8; 32], &mut Vec::new(), &mut 0, Instant::now() + Duration::from_secs(2), None).expect_err("downgrade");
        assert!(err.contains("downgrade"), "{err}");
        let (mut client, server) = peer();
        drop(server);
        let plain = server_hello_12(&[0x11u8; 32]);
        let err = finish_server_hello(&mut client, "example.com", &[], &[], &plain, None, &[9u8; 32], &[9u8; 32], &mut Vec::new(), &mut 0, Instant::now() + Duration::from_secs(2), None).expect_err("closed");
        assert!(!err.contains("key share"), "{err}");
        assert!(err.contains("closed") || err.contains("read"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn hello_retry_hashes_the_first_hello_and_sends_one_group() {
        let hello = client_hello("example.com", &[1u8; 32], &[2u8; 32], &[3u8; 32], &[4u8; 65], &[], None).expect("hello");
        let only = client_hello("example.com", &[1u8; 32], &[2u8; 32], &[3u8; 32], &[4u8; 65], &[], Some(0x001d)).expect("only");
        assert!(hello.windows(4).any(|window| window == [0x00, 0x17, 0x00, 0x41]));
        assert!(!only.windows(4).any(|window| window == [0x00, 0x17, 0x00, 0x41]));
        assert!(only.windows(4).any(|window| window == [0x00, 0x1d, 0x00, 0x20]));
        assert!(!hello.windows(2).any(|window| window == [0x05, 0x03]));
        let prefix = message_hash_prefix(HashKind::Sha256, &hello);
        assert_eq!(prefix[0], 254);
        assert_eq!(&prefix[1..4], &[0, 0, 32]);
        assert_eq!(&prefix[4..], sha256(&hello).as_slice());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn handshake_messages_reassemble_and_reject_oversized_lengths() {
        let mut pending = vec![1, 0, 0, 5, 1, 2];
        assert!(take_handshake(&mut pending).expect("partial").is_none());
        pending.extend_from_slice(&[3, 4, 5, 2, 0, 0, 1, 9]);
        assert_eq!(take_handshake(&mut pending).expect("first").expect("message"), vec![1, 0, 0, 5, 1, 2, 3, 4, 5]);
        assert_eq!(take_handshake(&mut pending).expect("second").expect("message"), vec![2, 0, 0, 1, 9]);
        let mut huge = vec![1, 0x10, 0x00, 0x01];
        assert!(take_handshake(&mut huge).is_err());
        let mut body = vec![3, 3];
        body.extend(std::iter::repeat(0).take(32));
        body.push(0xff);
        assert!(parse_server_hello(&body).is_err());
        let (mut client, _server) = peer();
        let mut records = HANDSHAKE_RECORD_BUDGET;
        let err = next_handshake(&mut client, &mut Vec::new(), &mut Vec::new(), &mut records, None, Instant::now() + Duration::from_secs(2)).expect_err("budget");
        assert!(err.contains("record budget"), "{err}");
        let err = next_handshake(&mut client, &mut Vec::new(), &mut Vec::new(), &mut 0, None, Instant::now() - Duration::from_secs(1)).expect_err("deadline");
        assert!(err.contains("timed out"), "{err}");
        let err = write_record(&mut client, 23, &vec![0u8; RECORD_LIMIT + 1], None).expect_err("record");
        assert!(err.contains("budget"), "{err}");
        let (root, _) = root_anchor();
        let der = certify(&root, &dn("Leaf"), &leaf_exts("example.com"));
        let mut list = vec![(der.len() >> 16) as u8, (der.len() >> 8) as u8, der.len() as u8];
        list.extend_from_slice(&der);
        assert_eq!(parse_cert_list(&list, false).expect("list").len(), 1);
        list.push(0);
        assert!(parse_cert_list(&list, false).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn application_secrets_use_the_transcript_through_server_finished() {
        let hash = HashKind::Sha256;
        let secret = [7u8; 32];
        let short = b"transcript-through-server-finished";
        let finished = b"client-finished-message";
        let (client_app, _server_app, resumption) = tls13_traffic(hash, &secret, short, finished);
        let derived = hkdf_expand_label(hash, &secret, b"derived", &hash_empty(hash), hash_len(hash));
        let master = hkdf_extract(hash, &derived, &zeros(hash_len(hash)));
        assert_eq!(client_app, derive_secret(hash, &master, b"c ap traffic", short));
        let mut full = short.to_vec();
        full.extend_from_slice(finished);
        assert_ne!(client_app, derive_secret(hash, &master, b"c ap traffic", &full));
        assert_eq!(resumption, derive_secret(hash, &master, b"res master", &full));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn certificate_time_rejects_impossible_days_without_panicking() {
        let mut raw = [b'0'; 13];
        raw[0] = 0xe2;
        raw[1] = 0x82;
        raw[2] = 0xac;
        raw[12] = b'Z';
        let caught = std::panic::catch_unwind(|| parse_time(0x17, &raw));
        assert!(caught.expect("panic").is_err());
        assert!(parse_time(0x18, b"20260230000000Z").is_err());
        assert!(parse_time(0x18, b"20260229000000Z").is_err());
        assert!(parse_time(0x18, b"20260431000000Z").is_err());
        assert!(parse_time(0x18, b"21000229000000Z").is_err());
        let start = parse_time(0x18, b"20200101000000Z").expect("start");
        let leap = parse_time(0x18, b"20240229000000Z").expect("leap");
        assert!(leap > start);
        assert!(parse_time(0x18, b"20200229000000Z").expect("2020") > start);
        let mut der = Der::new(&[0x30, 0x81, 0x01, 0x00]);
        assert!(der.sequence_bytes().is_err());
        let mut der = Der::new(&[0x02, 0x02, 0x00, 0x01]);
        assert!(der.integer().is_err());
        let mut der = Der::new(&[0x01, 0x01, 0x01]);
        assert!(der.boolean().is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn rsa_exponent_is_bounded_before_exponentiation() {
        let mut modulus = vec![0x80u8];
        modulus.extend(std::iter::repeat(0x11).take(255));
        let mut huge = vec![0x01u8; 64];
        let mut alg = der_oid(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01]);
        let pair = der_seq(&[der_int(&modulus), der_int(&huge)].concat());
        let mut bits = vec![0];
        bits.extend(pair);
        let spki = [der_seq(&alg), der_tlv(0x03, &bits)].concat();
        let err = public_key(&spki).expect_err("huge");
        assert!(err.contains("exponent"), "{err}");
        huge = vec![0x01, 0x00, 0x01];
        let pair = der_seq(&[der_int(&modulus), der_int(&huge)].concat());
        let mut bits = vec![0];
        bits.extend(pair);
        alg = der_oid(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01]);
        let spki = [der_seq(&alg), der_tlv(0x03, &bits)].concat();
        assert!(public_key(&spki).is_ok());
        let n = Nat::from_be(&modulus);
        let e = Nat::from_be(&[0x01, 0x00, 0x00, 0x00, 0x01]);
        let err = verify_pkcs1(&n, &e, SigAlg::RsaSha256, &[0u8; 32], &vec![1u8; 256]).expect_err("wide");
        assert!(err.contains("exponent"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn signatures_cover_the_full_certificate_encoding() {
        let (root, anchor) = root_anchor();
        let der = certify(&root, &dn("Leaf"), &leaf_exts("example.com"));
        let cert = parse_certificate(&der).expect("leaf");
        assert_eq!(cert.tbs[0], 0x30);
        let inner = Der::new(&cert.tbs).sequence_bytes().expect("inner");
        assert_ne!(sha256(&cert.tbs).as_slice(), sha256(inner).as_slice());
        verify_signed(&anchor, &cert).expect("signature");
        verify_chain("example.com", &[anchor], &[cert]).expect("chain");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn trust_anchor_is_selected_before_signatures() {
        let (root, anchor) = root_anchor();
        let mid = parsed(&dn("Nobody"), &dn("Mid"), &ca_exts(None));
        let mut leaf = parsed(&dn("Mid"), &dn("Leaf"), &leaf_exts("example.com"));
        leaf.signature[10] ^= 0xff;
        let err = verify_chain("example.com", &[anchor.clone()], &[leaf, mid]).expect_err("untrusted");
        assert!(err.contains("trust anchor"), "{err}");
        assert!(!err.contains("signature"), "{err}");
        let mut leaf = parsed(&root, &dn("Leaf"), &leaf_exts("example.com"));
        leaf.signature[10] ^= 0xff;
        let err = verify_chain("example.com", &[anchor], &[leaf]).expect_err("bad signature");
        assert!(err.contains("signature"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn path_constraints_key_use_and_names_are_enforced() {
        let (root, _) = root_anchor();
        let limited = parsed(&root, &root, &ca_exts(Some(0)));
        let mid = parsed(&root, &dn("Mid"), &ca_exts(None));
        let leaf = parsed(&dn("Mid"), &dn("Leaf"), &leaf_exts("example.com"));
        let err = verify_chain("example.com", &[limited.clone()], &[leaf.clone(), mid, limited.clone()]).expect_err("path");
        assert!(err.contains("longer than allowed"), "{err}");
        let direct = parsed(&root, &dn("Leaf"), &leaf_exts("example.com"));
        verify_chain("example.com", &[limited], &[direct]).expect("direct");
        let weak_anchor = parsed(&root, &root, &[bc(true, None), ku(0x80, 7)]);
        let err = verify_chain("example.com", &[weak_anchor], &[parsed(&root, &dn("Leaf"), &leaf_exts("example.com"))]).expect_err("usage");
        assert!(err.contains("not allowed to sign certificates"), "{err}");
        let anchor = parsed(&root, &root, &ca_exts(None));
        let err = verify_chain("example.com", &[anchor.clone()], &[parsed(&root, &dn("Leaf"), &[san_dns("example.com"), ku(0x04, 2), server_eku()])]).expect_err("leaf usage");
        assert!(err.contains("not allowed to sign the handshake"), "{err}");
        let err = verify_chain("example.com", &[anchor.clone()], &[parsed(&root, &dn("Leaf"), &[san_dns("example.com"), ku(0x80, 7), eku(&[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02])])]).expect_err("eku");
        assert!(err.contains("not allowed for this service"), "{err}");
        let constrained = parsed(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(true, "example.com", true)]);
        verify_chain("www.example.com", &[constrained.clone()], &[parsed(&root, &dn("Leaf"), &leaf_exts("www.example.com"))]).expect("permitted");
        let err = verify_chain("other.com", &[constrained], &[parsed(&root, &dn("Leaf"), &leaf_exts("other.com"))]).expect_err("outside");
        assert!(err.contains("not permitted"), "{err}");
        let excluded = parsed(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(false, "example.com", false)]);
        let err = verify_chain("www.example.com", &[excluded], &[parsed(&root, &dn("Leaf"), &leaf_exts("www.example.com"))]).expect_err("excluded");
        assert!(err.contains("excluded"), "{err}");
        let unknown = extension(&[0x2a, 0x03], true, &[0x05]);
        let err = parse_certificate(&certify(&root, &dn("Leaf"), &[unknown])).expect_err("critical");
        assert!(err.contains("unsupported critical"), "{err}");
        let tolerated = extension(&[0x2a, 0x03], false, &[0x05]);
        parse_certificate(&certify(&root, &dn("Leaf"), &[san_dns("example.com"), tolerated])).expect("noncritical");
        let cn_only = parsed(&root, &dn("*.example.com"), &[ku(0x80, 7), server_eku()]);
        assert!(!hostname_matches("www.example.com", &cn_only));
        let wild = parsed(&root, &dn("Leaf"), &leaf_exts("*.example.com"));
        assert!(hostname_matches("www.example.com", &wild));
        assert!(!hostname_matches("example.com", &wild));
        let _ = anchor;
    }

    fn hello_of(host: &str) -> Vec<u8> {
        client_hello(host, &[1u8; 32], &[2u8; 32], &[3u8; 32], &[4u8; 65], &[], None).expect("hello")
    }

    fn paired_tls13() -> (Client, Client) {
        let (left, right) = peer();
        let hash = HashKind::Sha256;
        let to_right = [3u8; 32];
        let to_left = [5u8; 32];
        let make = |stream: TcpStream, write_secret: [u8; 32], read_secret: [u8; 32]| Client {
            stream,
            incoming: Vec::new(),
            incoming_at: 0,
            write: traffic_keys(hash, CipherKind::Aes128, &write_secret),
            read: traffic_keys(hash, CipherKind::Aes128, &read_secret),
            write_secret: write_secret.to_vec(),
            read_secret: read_secret.to_vec(),
            hash,
            tls13: true,
            pending: Vec::new(),
            key_updates: 0,
        };
        (make(left, to_right, to_left), make(right, to_left, to_right))
    }

    fn pss_encode(message: &[u8], salt: &[u8], em_len: usize) -> Vec<u8> {
        let hash = HashKind::Sha256;
        let hlen = hash_len(hash);
        let mut prefix = vec![0u8; 8];
        prefix.extend(hash_bytes(hash, message));
        prefix.extend_from_slice(salt);
        let digest = hash_bytes(hash, &prefix);
        let db_len = em_len - hlen - 1;
        let mut db = vec![0u8; db_len - salt.len() - 1];
        db.push(0x01);
        db.extend_from_slice(salt);
        let mut masked = mgf1(hash, &digest, db_len).expect("mask");
        for (slot, byte) in masked.iter_mut().zip(&db) {
            *slot ^= byte;
        }
        let mut em = masked;
        em.extend(digest);
        em.push(0xbc);
        em
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn certificate_verify_stops_at_the_certificate() {
        let certificate = vec![11u8, 0, 0, 3, 1, 2, 3];
        let verify = vec![15u8, 0, 0, 3, 4, 5, 6];
        let finished = vec![20u8, 0, 0, 3, 7, 8, 9];
        let mut transcript = certificate.clone();
        transcript.extend_from_slice(&verify);
        transcript.extend_from_slice(&finished);
        let for_finished = transcript_excluding_suffix(&transcript, &finished).expect("finished");
        let for_certificate = transcript_excluding_suffix(&for_finished, &verify).expect("certificate");
        assert_eq!(for_certificate, certificate);
        let mut through_verify = certificate.clone();
        through_verify.extend_from_slice(&verify);
        assert_eq!(for_finished, through_verify);
        assert_ne!(hash_bytes(HashKind::Sha256, &for_certificate), hash_bytes(HashKind::Sha256, &for_finished));
        assert!(transcript_excluding_suffix(&transcript, &verify).is_err());
        let (root, anchor) = root_anchor();
        let leaf = parsed(&root, &dn("Leaf"), &leaf_exts("example.com"));
        let exact = vec![15u8, 0, 0, 5, 0x04, 0x03, 0, 1, 0xaa];
        let err = verify_certificate_signature_13(&leaf, &exact, &[0u8; 32]).expect_err("signature");
        assert!(!err.contains("TLS certificate signature is truncated"), "{err}");
        let trailing = vec![15u8, 0, 0, 6, 0x04, 0x03, 0, 1, 0xaa, 0xff];
        let err = verify_certificate_signature_13(&leaf, &trailing, &[0u8; 32]).expect_err("trailing");
        assert!(err.contains("TLS certificate signature is truncated"), "{err}");
        let _ = anchor;
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn key_usage_rejects_bits_declared_unused() {
        assert!(parse_key_usage(&[0x03, 0x02, 0x07, 0x04]).is_err());
        let usage = parse_key_usage(&[0x03, 0x02, 0x02, 0x04]).expect("cert sign");
        assert_eq!(usage & KU_KEY_CERT_SIGN, KU_KEY_CERT_SIGN);
        assert_eq!(parse_key_usage(&[0x03, 0x02, 0x07, 0x80]).expect("digital"), KU_DIGITAL_SIGNATURE);
        let (root, _) = root_anchor();
        let hostile = extension(&[0x55, 0x1d, 0x0f], true, &der_tlv(0x03, &[0x07, 0x04]));
        let err = parse_certificate(&certify(&root, &root, &[bc(true, None), hostile])).expect_err("unused");
        assert!(err.contains("key usage"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn tls13_compatibility_change_cipher_spec_is_not_decrypted() {
        let hash = HashKind::Sha256;
        let secret = [9u8; 32];
        let mut keys = traffic_keys(hash, CipherKind::Aes128, &secret);
        let (mut client, mut server) = peer();
        let deadline = Instant::now() + Duration::from_secs(2);
        server.write_all(&[20, 3, 1, 0, 1, 1]).expect("ccs");
        let (kind, payload) = read_record(&mut client, Some(&mut keys), Some(deadline)).expect("discard");
        assert_eq!(kind, 20);
        assert_eq!(payload, vec![1]);
        assert_eq!(keys.seq, 0);
        let mut sealing = traffic_keys(hash, CipherKind::Aes128, &secret);
        write_record(&mut server, 23, b"ok", Some(&mut sealing)).expect("seal");
        let (kind, payload) = read_record(&mut client, Some(&mut keys), Some(deadline)).expect("app");
        assert_eq!(kind, 23);
        assert_eq!(payload, b"ok");
        assert_eq!(keys.seq, 1);
        server.write_all(&[20, 3, 3, 0, 1, 0]).expect("bad");
        let err = read_record(&mut client, Some(&mut keys), Some(deadline)).expect_err("reject");
        assert!(err.contains("change cipher spec"), "{err}");
        assert_eq!(keys.seq, 1);
        let mut sealing = traffic_keys(hash, CipherKind::Aes128, &secret);
        sealing.seq = 1;
        write_record(&mut server, 20, &[1], Some(&mut sealing)).expect("protected");
        let err = read_record(&mut client, Some(&mut keys), Some(deadline)).expect_err("protected");
        assert!(err.contains("protected"), "{err}");
        let (mut reader, mut writer) = paired_tls13();
        writer.stream.write_all(&[20, 3, 3, 0, 1, 1]).expect("late");
        let err = reader.read(&mut [0u8; 4]).expect_err("after handshake");
        assert!(err.to_string().contains("not expected"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn key_update_rotates_the_next_read_generation() {
        let (mut reader, mut writer) = paired_tls13();
        let generation = writer.write_secret.clone();
        let mut sending = traffic_keys(writer.hash, writer.write.kind, &generation);
        let update = [24u8, 0, 0, 1, 0];
        write_record(&mut writer.stream, 22, &update[..2], Some(&mut sending)).expect("part");
        write_record(&mut writer.stream, 22, &update[2..], Some(&mut sending)).expect("rest");
        let next = next_traffic_secret(writer.hash, &generation).expect("next");
        let mut sending = traffic_keys(writer.hash, writer.write.kind, &next);
        write_record(&mut writer.stream, 23, b"payload", Some(&mut sending)).expect("app");
        let mut buf = [0u8; 16];
        let n = reader.read(&mut buf).expect("read");
        assert_eq!(&buf[..n], b"payload");
        assert_eq!(reader.read_secret, next);

        let (mut reader, mut writer) = paired_tls13();
        let generation = writer.write_secret.clone();
        let mut sending = traffic_keys(writer.hash, writer.write.kind, &generation);
        write_record(&mut writer.stream, 22, &[4, 0, 0, 1, 0], Some(&mut sending)).expect("ticket");
        write_record(&mut writer.stream, 22, &[24, 0, 0, 1, 1], Some(&mut sending)).expect("request");
        let next = next_traffic_secret(writer.hash, &generation).expect("next");
        let mut sending = traffic_keys(writer.hash, writer.write.kind, &next);
        write_record(&mut writer.stream, 23, b"after", Some(&mut sending)).expect("after");
        let n = reader.read(&mut buf).expect("requested");
        assert_eq!(&buf[..n], b"after");
        let reply_secret = [3u8; 32];
        let mut reading = traffic_keys(writer.hash, writer.read.kind, &reply_secret);
        let deadline = Instant::now() + Duration::from_secs(2);
        let (kind, body) = read_record(&mut writer.stream, Some(&mut reading), Some(deadline)).expect("reply");
        assert_eq!(kind, 22);
        assert_eq!(body, vec![24, 0, 0, 1, 0]);
        assert_eq!(reader.write_secret, next_traffic_secret(reader.hash, &reply_secret).expect("write"));

        let (mut reader, mut writer) = paired_tls13();
        let generation = writer.write_secret.clone();
        let mut sending = traffic_keys(writer.hash, writer.write.kind, &generation);
        write_record(&mut writer.stream, 22, &[24, 0, 0, 1, 2], Some(&mut sending)).expect("bad update");
        let err = reader.read(&mut buf).expect_err("rejected");
        assert!(err.to_string().contains("key update"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn pss_accepts_a_salt_that_contains_the_separator() {
        let mut salt = vec![0x01u8; 32];
        salt[3] = 0x11;
        salt[31] = 0x01;
        let mut db = vec![0u8; 8];
        db.push(0x01);
        db.extend_from_slice(&salt);
        let split = pss_salt(&db, salt.len()).expect("salt");
        assert_eq!(split, salt.as_slice());
        assert_ne!(db.iter().rposition(|byte| *byte == 0x01).expect("marker"), 8);
        let message = b"pss-message";
        let em = pss_encode(message, &salt, 96);
        let (modulus, signature) = pss_modulus_and_signature(&em, em.len() * 8 + 1);
        assert_eq!(modulus.bit_len(), em.len() * 8 + 1);
        let exponent = Nat::from_be(&[3]);
        verify_pss(&modulus, &exponent, PssParams { hash: HashKind::Sha256, mgf: HashKind::Sha256, salt_len: salt.len() }, message, &signature).expect("separator inside salt");
        let mut broken = signature;
        let last = broken.len() - 1;
        broken[last - 1] ^= 0xff;
        assert!(verify_pss(&modulus, &exponent, PssParams { hash: HashKind::Sha256, mgf: HashKind::Sha256, salt_len: salt.len() }, message, &broken).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn pss_sha384_certificate_verifies_and_pss_sha256_still_verifies() {
        assert_pss_certificate(HashKind::Sha384, HashKind::Sha384, 48);
        assert_pss_certificate(HashKind::Sha256, HashKind::Sha256, 32);
        assert_pss_certificate(HashKind::Sha512, HashKind::Sha512, 64);
        assert_pss_certificate(HashKind::Sha256, HashKind::Sha384, 20);
    }

    fn assert_pss_certificate(hash: HashKind, mgf: HashKind, salt_len: usize) {
        assert_pss_certificate_bits(hash, mgf, salt_len, 2049);
        assert_pss_certificate_bits(hash, mgf, salt_len, 2048);
    }

    fn assert_pss_certificate_bits(hash: HashKind, mgf: HashKind, salt_len: usize, modulus_bits: usize) {
        let issuer_name = dn("PssRoot");
        let subject_name = dn("PssLeaf");
        let alg = pss_identifier(hash, mgf, salt_len);
        let tbs = pss_tbs(&issuer_name, &subject_name, &alg, &p256_spki());
        let salt = vec![0x11u8; salt_len];
        let em_bits = modulus_bits - 1;
        let em_len = (em_bits + 7) / 8;
        let em = pss_encoded(hash, mgf, &tbs, &salt, em_len, em_bits);
        assert_eq!(em.len(), 256);
        assert_eq!(em.len(), em_len);
        let (modulus, signature) = pss_modulus_and_signature(&em, modulus_bits);
        assert_eq!(modulus.bit_len(), modulus_bits);
        let modulus_octets = (modulus_bits + 7) / 8;
        let recovered = Nat::from_be(&signature).modpow(&Nat::from_be(&[3]), &modulus).to_be(modulus_octets);
        assert_eq!(recovered.len(), modulus_octets);
        if modulus_octets > em_len {
            assert_eq!(recovered[0], 0, "leading representative octet");
            assert_eq!(&recovered[1..], em.as_slice());
        } else {
            assert_eq!(recovered[0] & 0x80, 0, "top encoded bit");
            assert_eq!(recovered, em);
        }
        let subject = parse_certificate(&wrap_cert(&tbs, &alg, &signature)).expect("subject");
        let issuer = parse_certificate(&wrap_cert(&ecdsa_tbs(&issuer_name, &rsa_spki(&modulus)), &ecdsa_alg(), &ecdsa_sign(&ecdsa_tbs(&issuer_name, &rsa_spki(&modulus))))).expect("issuer");
        let parsed = match subject.signature_alg {
            SigAlg::Pss(params) => params,
            _ => panic!("signature parameters were not kept"),
        };
        assert!(parsed.hash == hash && parsed.mgf == mgf && parsed.salt_len == salt_len);
        verify_signed(&issuer, &subject).expect("pss certificate");
        let mut tampered = signature;
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        let broken = parse_certificate(&wrap_cert(&tbs, &alg, &tampered)).expect("tampered");
        assert!(verify_signed(&issuer, &broken).is_err());
    }

    fn pss_identifier(hash: HashKind, mgf: HashKind, salt_len: usize) -> Vec<u8> {
        let hash_id = hash_identifier(hash);
        let mgf_id = hash_identifier(mgf);
        let mut mgf_body = der_oid(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x08]);
        mgf_body.extend_from_slice(&mgf_id);
        let mgf_alg = der_seq(&mgf_body);
        let mut salt = Vec::new();
        let mut rest = salt_len;
        let mut digits = Vec::new();
        if rest == 0 {
            digits.push(0);
        }
        while rest > 0 {
            digits.push((rest & 0xff) as u8);
            rest >>= 8;
        }
        digits.reverse();
        salt.extend(der_int(&digits));
        let mut params = der_tlv(0xa0, &hash_id);
        params.extend(der_tlv(0xa1, &mgf_alg));
        params.extend(der_tlv(0xa2, &salt));
        params.extend(der_tlv(0xa3, &der_int(&[1])));
        let mut body = der_oid(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a]);
        body.extend(der_seq(&params));
        der_seq(&body)
    }

    fn hash_identifier(hash: HashKind) -> Vec<u8> {
        let oid: &[u8] = match hash {
            HashKind::Sha256 => &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01],
            HashKind::Sha384 => &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02],
            HashKind::Sha512 => &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03],
        };
        let mut body = der_oid(oid);
        body.extend(der_tlv(0x05, &[]));
        der_seq(&body)
    }

    fn pss_tbs(issuer: &[u8], subject: &[u8], alg: &[u8], spki: &[u8]) -> Vec<u8> {
        let mut body = der_tlv(0xa0, &der_int(&[2]));
        body.extend(der_int(&[1]));
        body.extend_from_slice(alg);
        body.extend_from_slice(issuer);
        body.extend(validity());
        body.extend_from_slice(subject);
        body.extend_from_slice(spki);
        der_seq(&body)
    }

    fn ecdsa_tbs(name: &[u8], spki: &[u8]) -> Vec<u8> {
        pss_tbs(name, name, &ecdsa_alg(), spki)
    }

    fn wrap_cert(tbs: &[u8], alg: &[u8], signature: &[u8]) -> Vec<u8> {
        let mut bits = vec![0u8];
        bits.extend_from_slice(signature);
        let mut cert = tbs.to_vec();
        cert.extend_from_slice(alg);
        cert.extend(der_tlv(0x03, &bits));
        der_seq(&cert)
    }

    fn rsa_spki(modulus: &Nat) -> Vec<u8> {
        let mut alg = der_oid(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01]);
        alg.extend(der_tlv(0x05, &[]));
        let alg = der_seq(&alg);
        let length = modulus.bit_len().div_ceil(8).max(1);
        let mut pair = der_int(&modulus.to_be(length));
        pair.extend(der_int(&[3]));
        let mut bits = vec![0u8];
        bits.extend(der_seq(&pair));
        let mut body = alg;
        body.extend(der_tlv(0x03, &bits));
        der_seq(&body)
    }

    fn nat_pow2(bit: usize) -> Nat {
        Nat::from_be(&bytes_pow2(bit))
    }

    fn bytes_pow2(bit: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; bit / 8 + 1];
        bytes[0] = 1 << (bit % 8);
        bytes
    }

    fn pss_modulus_and_signature(em: &[u8], modulus_bits: usize) -> (Nat, Vec<u8>) {
        let encoded = Nat::from_be(em);
        if modulus_bits == 2049 {
            return (nat_pow2(2049).sub(&encoded), bytes_pow2(683));
        }
        let lower = nat_pow2(modulus_bits - 1).add(&encoded);
        let mut high = nat_pow2(modulus_bits / 3 + 2);
        while cube(&high).cmp(&lower) == std::cmp::Ordering::Less {
            high = high.add(&high);
        }
        let root = smallest_cube_at_least(&lower, &high);
        let modulus = cube(&root).sub(&encoded);
        let length = root.bit_len().div_ceil(8).max(1);
        (modulus, root.to_be(length))
    }

    fn cube(value: &Nat) -> Nat {
        value.mul(value).mul(value)
    }

    fn smallest_cube_at_least(target: &Nat, high_bound: &Nat) -> Nat {
        let mut low = Nat { limb: vec![0] };
        let mut high = high_bound.clone();
        while low.cmp(&high) == std::cmp::Ordering::Less {
            let mid = nat_mid(&low, &high);
            if mid.cmp(&low) != std::cmp::Ordering::Greater {
                break;
            }
            if cube(&mid).cmp(target) == std::cmp::Ordering::Less {
                low = mid.add(&Nat { limb: vec![1] });
            } else {
                high = mid;
            }
        }
        if cube(&low).cmp(target) == std::cmp::Ordering::Less { high } else { low }
    }

    fn nat_mid(low: &Nat, high: &Nat) -> Nat {
        let mut limb = low.add(high).limb;
        let mut carry = 0u32;
        for word in limb.iter_mut().rev() {
            let next = *word & 1;
            *word = (*word >> 1) | (carry << 31);
            carry = next;
        }
        while limb.len() > 1 && limb.last() == Some(&0) {
            limb.pop();
        }
        if limb.is_empty() {
            limb.push(0);
        }
        Nat { limb }
    }

    fn pss_encoded(hash: HashKind, mgf: HashKind, message: &[u8], salt: &[u8], em_len: usize, em_bits: usize) -> Vec<u8> {
        let hlen = hash_len(hash);
        let mut prefix = vec![0u8; 8];
        prefix.extend(hash_bytes(hash, message));
        prefix.extend_from_slice(salt);
        let digest = hash_bytes(hash, &prefix);
        let db_len = em_len - hlen - 1;
        let mut db = vec![0u8; db_len - salt.len() - 1];
        db.push(0x01);
        db.extend_from_slice(salt);
        let mut masked = mgf1(mgf, &digest, db_len).expect("mask");
        for (slot, byte) in masked.iter_mut().zip(&db) {
            *slot ^= *byte;
        }
        let mut unused = em_len * 8 - em_bits;
        let mut index = 0;
        while unused >= 8 && index < masked.len() {
            masked[index] = 0;
            index += 1;
            unused -= 8;
        }
        if unused > 0 && index < masked.len() {
            masked[index] &= 0xff >> unused;
        }
        let mut em = masked;
        em.extend(digest);
        em.push(0xbc);
        em
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn ip_literals_are_omitted_from_the_server_name() {
        let dns = hello_of("example.com");
        assert!(dns.windows(11).any(|window| window == b"example.com"));
        let v4 = hello_of("192.0.2.10");
        assert!(!v4.windows(10).any(|window| window == b"192.0.2.10"));
        let v6 = hello_of("2001:db8::1");
        assert!(!v6.windows(11).any(|window| window == b"2001:db8::1"));
        let bracket = hello_of("[2001:db8::2]");
        assert!(!bracket.windows(12).any(|window| window == b"[2001:db8::2]"));
        assert!(!bracket.windows(11).any(|window| window == b"2001:db8::2"));
    }

    fn certify_with_blocks(issuer: &[u8], subject: &[u8], blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = der_tlv(0xa0, &der_int(&[2]));
        body.extend(der_int(&[1]));
        body.extend(ecdsa_alg());
        body.extend_from_slice(issuer);
        body.extend(validity());
        body.extend_from_slice(subject);
        body.extend(p256_spki());
        for block in blocks {
            body.extend_from_slice(block);
        }
        let tbs = der_seq(&body);
        let sig = ecdsa_sign(&tbs);
        let mut bits = vec![0];
        bits.extend(sig);
        let mut cert = tbs;
        cert.extend(ecdsa_alg());
        cert.extend(der_tlv(0x03, &bits));
        der_seq(&cert)
    }

    fn handshake_message(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut message = vec![kind, 0, 0, 0];
        let length = u32::try_from(body.len()).expect("handshake length");
        message[1] = (length >> 16) as u8;
        message[2] = (length >> 8) as u8;
        message[3] = length as u8;
        message.extend_from_slice(body);
        message
    }

    fn server_hello_body(legacy: u16, random: &[u8; 32], suite: u16, extensions: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&legacy.to_be_bytes());
        body.extend_from_slice(random);
        body.push(0);
        body.extend_from_slice(&suite.to_be_bytes());
        body.push(0);
        if !extensions.is_empty() {
            body.extend_from_slice(&u16::try_from(extensions.len()).expect("extension length").to_be_bytes());
            body.extend_from_slice(extensions);
        }
        body
    }

    fn tls_ext(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut out = kind.to_be_bytes().to_vec();
        out.extend_from_slice(&u16::try_from(data.len()).expect("extension data").to_be_bytes());
        out.extend_from_slice(data);
        out
    }

    fn version_ext(version: u16) -> Vec<u8> { tls_ext(43, &version.to_be_bytes()) }

    fn key_share_ext(group: u16, point: &[u8]) -> Vec<u8> {
        let mut entry = group.to_be_bytes().to_vec();
        entry.extend_from_slice(&u16::try_from(point.len()).expect("point").to_be_bytes());
        entry.extend_from_slice(point);
        tls_ext(51, &entry)
    }

    fn named_group_ext(group: u16) -> Vec<u8> { tls_ext(51, &group.to_be_bytes()) }

    fn retry_extensions(version: u16, group: u16) -> Vec<u8> {
        let mut extensions = version_ext(version);
        extensions.extend(named_group_ext(group));
        extensions
    }

    fn err_of<T>(result: Result<T, String>, label: &str) -> String {
        match result {
            Ok(_) => panic!("{label}"),
            Err(error) => error,
        }
    }

    fn finish_at(message: &[u8], hrr: Option<HelloRetry>) -> Result<Client, String> {
        let (mut client, server) = peer();
        drop(server);
        finish_server_hello(&mut client, "example.com", &[], &[], message, hrr, &[9u8; 32], &[9u8; 32], &mut Vec::new(), &mut 0, Instant::now() + Duration::from_secs(2), None)
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn duplicate_certificate_extensions_are_rejected() {
        let (root, anchor) = root_anchor();
        let single = parsed(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(true, "example.com", false)]);
        verify_chain("www.example.com", &[single], &[parsed(&root, &dn("Leaf"), &leaf_exts("www.example.com"))]).expect("one constraint");
        let replaced = certify(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(true, "good.example", false), nc_dns(true, "example.com", false)]);
        let err = parse_certificate(&replaced).expect_err("second name constraint");
        assert!(err.contains("repeats an extension"), "{err}");
        let usage = certify(&root, &root, &[bc(true, None), ku(0x80, 7), ku(0x04, 2)]);
        let err = parse_certificate(&usage).expect_err("second key usage");
        assert!(err.contains("repeats an extension"), "{err}");
        let merged = certify(&root, &dn("Leaf"), &[san_dns("evil.com"), san_dns("example.com"), ku(0x80, 7), server_eku()]);
        let err = parse_certificate(&merged).expect_err("merged names");
        assert!(err.contains("repeats an extension"), "{err}");
        let ski = extension(&[0x55, 0x1d, 0x0e], false, &[0x00]);
        let ignored = certify(&root, &dn("Leaf"), &[san_dns("example.com"), ku(0x80, 7), server_eku(), ski.clone(), ski]);
        let err = parse_certificate(&ignored).expect_err("repeated ignored extension");
        assert!(err.contains("repeats an extension"), "{err}");
        let unknown = extension(&[0x2a, 0x03], false, &[0x05]);
        let err = parse_certificate(&certify(&root, &dn("Leaf"), &[san_dns("example.com"), ku(0x80, 7), server_eku(), unknown.clone(), unknown])).expect_err("repeated unknown");
        assert!(err.contains("repeats an extension"), "{err}");
        let mut permit = der_tlv(0xa0, &der_seq(&der_seq(&der_tlv(0x82, b"good.example"))));
        permit.extend(der_tlv(0xa0, &der_seq(&der_seq(&der_tlv(0x82, b"example.com")))));
        let widened = extension(&[0x55, 0x1d, 0x1e], true, &der_seq(&permit));
        let err = parse_certificate(&certify(&root, &root, &[bc(true, None), ku(0x04, 2), widened])).expect_err("two permitted subtrees");
        assert!(err.contains("repeats an extension"), "{err}");
        let first = extensions(&[nc_dns(true, "good.example", false)]);
        let second = extensions(&[nc_dns(true, "example.com", false)]);
        let err = parse_certificate(&certify_with_blocks(&root, &root, &[first, second])).expect_err("two extension lists");
        assert!(err.contains("repeats an extension"), "{err}");
        let err = parse_certificate(&certify(&root, &root, &[bc(false, Some(0)), ku(0x04, 2)])).expect_err("path without ca");
        assert!(err.contains("path length"), "{err}");
        let mut fields = der_tlv(0x01, &[0xff]);
        fields.extend(der_tlv(0x01, &[0x00]));
        let err = parse_certificate(&certify(&root, &root, &[extension(&[0x55, 0x1d, 0x13], true, &der_seq(&fields)), ku(0x04, 2)])).expect_err("trailing constraints");
        assert!(err.contains("basic constraints"), "{err}");
        let der = certify(&root, &dn("Leaf"), &leaf_exts("example.com"));
        let prefix = [(der.len() >> 16) as u8, (der.len() >> 8) as u8, der.len() as u8];
        let mut list = prefix.to_vec();
        list.extend_from_slice(&der);
        list.extend_from_slice(&[0x00, 0x00]);
        assert_eq!(parse_cert_list(&list, true).expect("empty extensions").len(), 1);
        let mut hostile = prefix.to_vec();
        hostile.extend_from_slice(&der);
        hostile.extend_from_slice(&[0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let err = parse_cert_list(&hostile, true).expect_err("duplicate certificate extensions");
        assert!(err.contains("repeats an extension"), "{err}");
        let mut truncated = prefix.to_vec();
        truncated.extend_from_slice(&der);
        truncated.extend_from_slice(&[0x00, 0x03, 0x00, 0x00, 0x00]);
        assert!(parse_cert_list(&truncated, true).is_err());
        assert!(parse_tls_extension_list(&[0x00, 0x04, 0x00, 0x2b, 0x00, 0x00]).is_ok());
        let err = parse_tls_extension_list(&[0x00, 0x08, 0x00, 0x2b, 0x00, 0x00, 0x00, 0x2b, 0x00, 0x00]).expect_err("duplicate encrypted extensions");
        assert!(err.contains("repeats an extension"), "{err}");
        let _ = anchor;
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn server_hello_selections_stay_inside_the_offer_and_hello_retry() {
        let hello = hello_of("example.com");
        assert!(hello.windows(2).any(|window| window == [0x03, 0x04]));
        assert!(hello.windows(2).any(|window| window == [0x03, 0x03]));
        assert!(!hello.windows(2).any(|window| window == [0x03, 0x05]));
        let offered = hello_retry_selection(&server_hello_body(TLS12_VERSION, &HELLO_RETRY_RANDOM, 0x1301, &retry_extensions(TLS13_VERSION, 0x001d))).expect("retry");
        assert_eq!(offered.suite, 0x1301);
        assert_eq!(offered.version, TLS13_VERSION);
        assert_eq!(offered.group, 0x001d);
        let err = err_of(hello_retry_selection(&server_hello_body(TLS12_VERSION, &HELLO_RETRY_RANDOM, 0x1301, &retry_extensions(0x0305, 0x001d))), "unoffered retry version");
        assert!(err.contains("supported version"), "{err}");
        let err = err_of(hello_retry_selection(&server_hello_body(0x0301, &HELLO_RETRY_RANDOM, 0x1301, &retry_extensions(TLS13_VERSION, 0x001d))), "legacy");
        assert!(err.contains("supported version"), "{err}");
        let err = err_of(hello_retry_selection(&server_hello_body(TLS12_VERSION, &HELLO_RETRY_RANDOM, 0x00ff, &retry_extensions(TLS13_VERSION, 0x001d))), "suite");
        assert!(err.contains("cipher"), "{err}");
        let err = err_of(hello_retry_selection(&server_hello_body(TLS12_VERSION, &HELLO_RETRY_RANDOM, 0x1301, &retry_extensions(TLS13_VERSION, 0x0018))), "group");
        assert!(err.contains("unsupported group"), "{err}");
        let mut repeated = version_ext(TLS13_VERSION);
        repeated.extend(version_ext(TLS12_VERSION));
        repeated.extend(named_group_ext(0x001d));
        let err = err_of(hello_retry_selection(&server_hello_body(TLS12_VERSION, &HELLO_RETRY_RANDOM, 0x1301, &repeated)), "repeated retry extension");
        assert!(err.contains("repeats an extension"), "{err}");
        let err = parse_server_hello(&server_hello_body(TLS12_VERSION, &[0x11; 32], 0x1301, &{
            let mut extensions = version_ext(0x0304);
            extensions.extend(version_ext(0x0303));
            extensions
        })).expect_err("repeated version");
        assert!(err.contains("repeats an extension"), "{err}");
        let err = parse_server_hello(&server_hello_body(TLS12_VERSION, &[0x11; 32], 0x1301, &tls_ext(43, &[0x03]))).expect_err("short version");
        assert!(err.contains("supported version"), "{err}");
        let mut share = 0x001du16.to_be_bytes().to_vec();
        share.extend_from_slice(&32u16.to_be_bytes());
        share.extend_from_slice(&[0x02; 32]);
        share.push(0xff);
        let err = parse_server_hello(&server_hello_body(TLS12_VERSION, &[0x11; 32], 0x1301, &tls_ext(51, &share))).expect_err("trailing share");
        assert!(err.contains("key share"), "{err}");
        let err = parse_server_hello(&server_hello_body(TLS12_VERSION, &[0x11; 32], 0x1301, &tls_ext(51, &[0x00]))).expect_err("short share");
        assert!(err.contains("key share"), "{err}");
        let bare = handshake_message(2, &server_hello_body(TLS12_VERSION, &[0x11; 32], 0x1301, &version_ext(0x0305)));
        let err = finish_at(&bare, None).expect_err("unoffered without share");
        assert!(err.contains("supported version"), "{err}");
        assert!(!err.contains("key share"), "{err}");
        let mut offered_share = version_ext(0x0305);
        offered_share.extend(key_share_ext(0x001d, &[0x02; 32]));
        let with_share = handshake_message(2, &server_hello_body(TLS12_VERSION, &[0x11; 32], 0x1301, &offered_share));
        let err = finish_at(&with_share, None).expect_err("unoffered with share");
        assert!(err.contains("supported version"), "{err}");
        let mut legacy = version_ext(TLS13_VERSION);
        legacy.extend(key_share_ext(0x001d, &[0x02; 32]));
        let err = finish_at(&handshake_message(2, &server_hello_body(0x0301, &[0x11; 32], 0x1301, &legacy)), None).expect_err("legacy");
        assert!(err.contains("supported version"), "{err}");
        let retry = HelloRetry { suite: 0x1301, version: TLS13_VERSION, group: 0x001d, cookie: Vec::new() };
        let mut other_suite = version_ext(TLS13_VERSION);
        other_suite.extend(key_share_ext(0x001d, &[0x02; 32]));
        let err = finish_at(&handshake_message(2, &server_hello_body(TLS12_VERSION, &[0x22; 32], 0x1302, &other_suite)), Some(retry.clone())).expect_err("suite changed");
        assert!(err.contains("hello retry"), "{err}");
        let err = finish_at(&server_hello_12(&[0x11; 32]), Some(retry.clone())).expect_err("version changed");
        assert!(err.contains("hello retry"), "{err}");
        let mut other_group = version_ext(TLS13_VERSION);
        other_group.extend(key_share_ext(0x0017, &[0x04; 65]));
        let err = finish_at(&handshake_message(2, &server_hello_body(TLS12_VERSION, &[0x22; 32], 0x1301, &other_group)), Some(retry.clone())).expect_err("group changed");
        assert!(err.contains("hello retry"), "{err}");
        let mut same = version_ext(TLS13_VERSION);
        same.extend(key_share_ext(0x001d, &[0x02; 32]));
        let err = finish_at(&handshake_message(2, &server_hello_body(TLS12_VERSION, &[0x22; 32], 0x1301, &same)), Some(retry)).expect_err("continues");
        assert!(!err.contains("hello retry"), "{err}");
        assert!(!err.contains("supported version"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn record_sequence_does_not_wrap_under_the_same_keys() {
        let (mut client, _server) = peer();
        for legacy in [false, true] {
            let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
            keys.legacy = legacy;
            keys.seq = u64::MAX;
            let err = write_record(&mut client, 23, b"x", Some(&mut keys)).expect_err("write wrap");
            assert!(err.contains("wrap"), "{err}");
            assert_eq!(keys.seq, u64::MAX);
            let err = read_record(&mut client, Some(&mut keys), Some(Instant::now() + Duration::from_millis(50))).expect_err("read wrap");
            assert!(err.contains("wrap"), "{err}");
            assert_eq!(keys.seq, u64::MAX);
        }
        let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
        keys.seq = u64::MAX - 1;
        write_record(&mut client, 23, b"last", Some(&mut keys)).expect("last record");
        assert_eq!(keys.seq, u64::MAX);
        let err = write_record(&mut client, 23, b"again", Some(&mut keys)).expect_err("after last");
        assert!(err.contains("wrap"), "{err}");
        assert_eq!(keys.seq, u64::MAX);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn empty_read_returns_without_waiting() {
        let (mut reader, _writer) = paired_tls13();
        reader.stream.set_read_timeout(Some(Duration::from_millis(200))).expect("timeout");
        let started = Instant::now();
        assert_eq!(reader.read(&mut []).expect("empty"), 0);
        assert!(started.elapsed() < Duration::from_millis(150), "empty read waited {:?}", started.elapsed());
        reader.incoming = b"ab".to_vec();
        reader.incoming_at = 0;
        assert_eq!(reader.read(&mut []).expect("buffered empty"), 0);
        let mut buf = [0u8; 2];
        assert_eq!(reader.read(&mut buf).expect("preserved"), 2);
        assert_eq!(&buf, b"ab");
    }

    fn write_plain(stream: &mut TcpStream, kind: u8, payload: &[u8]) {
        let len = u16::try_from(payload.len()).expect("record length");
        let mut record = vec![kind, 3, 3, (len >> 8) as u8, (len & 0xff) as u8];
        record.extend_from_slice(payload);
        stream.write_all(&record).expect("record");
    }

    fn seal_raw_inner(keys: &Traffic, inner: &[u8]) -> Vec<u8> {
        let mut nonce = keys.iv;
        let seq = keys.seq.to_be_bytes();
        for (slot, byte) in nonce[4..].iter_mut().zip(seq) {
            *slot ^= byte;
        }
        let length = u16::try_from(inner.len() + 16).expect("inner length");
        let header = [23u8, 3, 3, (length >> 8) as u8, (length & 0xff) as u8];
        let encrypted = aead_seal(keys, &nonce, &header, inner).expect("seal");
        let mut record = header.to_vec();
        record.extend_from_slice(&encrypted);
        record
    }

    fn deadline() -> Instant { Instant::now() + Duration::from_secs(2) }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn leading_dot_dns_constraints_are_subdomains_only() {
        let (root, _) = root_anchor();
        let sub = parsed(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(true, ".example.com", false)]);
        assert_eq!(sub.constraints.permitted_dns, vec![".example.com".to_string()]);
        let leaf = |name: &str| parsed(&root, &dn("Leaf"), &leaf_exts(name));
        let err = verify_chain("example.com", &[sub.clone()], &[leaf("example.com")]).expect_err("apex");
        assert!(err.contains("not permitted"), "{err}");
        verify_chain("www.example.com", &[sub.clone()], &[leaf("www.example.com")]).expect("one label");
        verify_chain("a.b.example.com", &[sub.clone()], &[leaf("a.b.example.com")]).expect("two labels");
        let err = verify_chain("notexample.com", &[sub.clone()], &[leaf("notexample.com")]).expect_err("partial label");
        assert!(err.contains("not permitted"), "{err}");
        let err = verify_chain("example.com.evil.net", &[sub.clone()], &[leaf("example.com.evil.net")]).expect_err("different parent");
        assert!(err.contains("not permitted"), "{err}");
        let dotted = parsed(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(true, ".Example.COM.", true)]);
        assert_eq!(dotted.constraints.permitted_dns, vec![".example.com".to_string()]);
        let err = verify_chain("example.com", &[dotted.clone()], &[leaf("example.com")]).expect_err("trailing apex");
        assert!(err.contains("not permitted"), "{err}");
        verify_chain("www.example.com", &[dotted], &[leaf("www.example.com")]).expect("folded subdomain");
        let apex = parsed(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(true, "example.com.", false)]);
        assert_eq!(apex.constraints.permitted_dns, vec!["example.com".to_string()]);
        verify_chain("example.com", &[apex.clone()], &[leaf("example.com")]).expect("apex still permitted");
        verify_chain("www.example.com", &[apex], &[leaf("www.example.com")]).expect("apex constraint includes subdomains");
        let excluded = parsed(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(false, ".example.com", false)]);
        verify_chain("example.com", &[excluded.clone()], &[leaf("example.com")]).expect("exclusion leaves the apex");
        let err = verify_chain("www.example.com", &[excluded.clone()], &[leaf("www.example.com")]).expect_err("exclusion");
        assert!(err.contains("excluded"), "{err}");
        for hostile in [".", "..", "..example.com", "example..com", ".example..com", "example.com.."] {
            let err = parse_certificate(&certify(&root, &root, &[bc(true, None), ku(0x04, 2), nc_dns(true, hostile, false)])).expect_err(hostile);
            assert!(err.contains("name constraint"), "{hostile}: {err}");
        }
        assert!(dns_constrained("WWW.Example.COM.", ".example.com"));
        assert!(!dns_constrained("example.com.", ".example.com"));
        assert!(dns_constrained("example.com.", "example.com"));
        assert!(!dns_constrained("notexample.com", ".example.com"));
        assert!(!dns_constrained(".example.com", "example.com"));
        assert!(!dns_constrained("example.com", "."));
        assert!(!dns_constrained("example.com", ""));
        assert!(sub.constraints.allows(&leaf(".example.com")).expect_err("leading dot on the name").contains("not permitted"));
        assert!(excluded.constraints.allows(&leaf("example..com")).is_err());
        sub.constraints.allows(&leaf("www.example.com.")).expect("trailing dot on a subdomain");
        assert!(sub.constraints.allows(&leaf("example.com.")).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn plaintext_records_keep_the_fragment_limit() {
        let mut payload = vec![0u8; RECORD_LIMIT];
        let body_len = RECORD_LIMIT - 4;
        payload[0] = 11;
        payload[1] = (body_len >> 16) as u8;
        payload[2] = (body_len >> 8) as u8;
        payload[3] = body_len as u8;
        let (mut client, mut server) = peer();
        write_plain(&mut server, 22, &payload);
        let message = next_handshake(&mut client, &mut Vec::new(), &mut Vec::new(), &mut 0, None, deadline()).expect("maximum fragment");
        assert_eq!(message.len(), RECORD_LIMIT);
        for extra in [1usize, 256] {
            let (mut client, mut server) = peer();
            let illegal = RECORD_LIMIT + extra;
            let mut record = vec![22, 3, 3, (illegal >> 8) as u8, (illegal & 0xff) as u8];
            record.extend(std::iter::repeat(0).take(illegal));
            server.write_all(&record).expect("oversized");
            let mut pending = Vec::new();
            let mut transcript = Vec::new();
            let err = next_handshake(&mut client, &mut transcript, &mut pending, &mut 0, None, deadline()).expect_err("plaintext cap");
            assert!(err.contains("budget"), "{extra}: {err}");
            assert!(pending.is_empty(), "{extra}");
            assert!(transcript.is_empty(), "{extra}");
        }
        let mut legal = vec![0x61u8; RECORD_LIMIT];
        legal.push(22);
        let (kind, body) = tls13_content(&legal).expect("maximum inner");
        assert_eq!(kind, 22);
        assert_eq!(body.len(), RECORD_LIMIT);
        let mut padded = b"hi".to_vec();
        padded.push(22);
        padded.extend(std::iter::repeat(0).take(RECORD_LIMIT));
        assert!(tls13_content(&padded).expect_err("padding past the inner cap").contains("budget"));
        let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
        let (mut client, mut server) = peer();
        server.write_all(&seal(&keys, 22, &vec![0x61; RECORD_LIMIT]).expect("seal max")).expect("write max");
        let (kind, body) = read_record(&mut client, Some(&mut keys), Some(deadline())).expect("read max");
        assert_eq!(kind, 22);
        assert_eq!(body.len(), RECORD_LIMIT);
        let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
        let (mut client, mut server) = peer();
        server.write_all(&seal(&keys, 22, &vec![0x61; RECORD_LIMIT + 1]).expect("seal over")).expect("write over");
        let err = read_record(&mut client, Some(&mut keys), Some(deadline())).expect_err("inner cap");
        assert!(err.contains("budget"), "{err}");
        let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
        let (mut client, mut server) = peer();
        server.write_all(&seal_raw_inner(&keys, &padded)).expect("padded");
        let err = read_record(&mut client, Some(&mut keys), Some(deadline())).expect_err("padded inner");
        assert!(err.contains("budget"), "{err}");
        let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
        keys.legacy = true;
        let (mut client, mut server) = peer();
        server.write_all(&seal_legacy(&keys, 22, &vec![0x61; RECORD_LIMIT]).expect("legacy max")).expect("write legacy");
        let (kind, body) = read_record(&mut client, Some(&mut keys), Some(deadline())).expect("legacy fragment");
        assert_eq!(kind, 22);
        assert_eq!(body.len(), RECORD_LIMIT);
        let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
        keys.legacy = true;
        let (mut client, mut server) = peer();
        server.write_all(&seal_legacy(&keys, 22, &vec![0x61; RECORD_LIMIT + 1]).expect("legacy over")).expect("write legacy over");
        let err = read_record(&mut client, Some(&mut keys), Some(deadline())).expect_err("legacy plaintext cap");
        assert!(err.contains("budget"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn change_cipher_spec_requires_the_single_byte_one() {
        for payload in [Vec::new(), vec![0], vec![2], vec![1, 0], vec![1, 1], vec![0, 1], vec![1, 1, 1]] {
            let (mut client, mut server) = peer();
            write_plain(&mut server, 20, &payload);
            let err = next_handshake(&mut client, &mut Vec::new(), &mut Vec::new(), &mut 0, None, deadline()).expect_err("handshake ccs");
            assert!(err.contains("change cipher spec"), "{payload:?}: {err}");
            let (mut client, mut server) = peer();
            write_plain(&mut server, 20, &payload);
            let err = read_cipher_change(&mut client, deadline()).expect_err("cipher change");
            assert!(err.contains("change cipher spec"), "{payload:?}: {err}");
            let mut keys = traffic_keys(HashKind::Sha256, CipherKind::Aes128, &[9u8; 32]);
            let (mut client, mut server) = peer();
            write_plain(&mut server, 20, &payload);
            let err = read_record(&mut client, Some(&mut keys), Some(deadline())).expect_err("protected reader");
            assert!(err.contains("change cipher spec"), "{payload:?}: {err}");
            assert_eq!(keys.seq, 0);
        }
        let (mut client, mut server) = peer();
        write_plain(&mut server, 20, &[1]);
        write_plain(&mut server, 22, &[11, 0, 0, 0]);
        let message = next_handshake(&mut client, &mut Vec::new(), &mut Vec::new(), &mut 0, None, deadline()).expect("skip a real ccs");
        assert_eq!(message, vec![11, 0, 0, 0]);
        let (mut client, mut server) = peer();
        write_plain(&mut server, 20, &[1]);
        read_cipher_change(&mut client, deadline()).expect("one byte");
        let (mut client, mut server) = peer();
        write_plain(&mut server, 22, &[11, 0, 0, 0]);
        let err = read_cipher_change(&mut client, deadline()).expect_err("not a ccs");
        assert!(err.contains("did not switch"), "{err}");
        let (mut client, mut server) = peer();
        write_plain(&mut server, 21, &[1, 0, 0]);
        let err = read_cipher_change(&mut client, deadline()).expect_err("alert instead");
        assert!(err.contains("malformed"), "{err}");
        assert!(!err.contains("alert 0"), "{err}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn close_notify_is_exactly_two_bytes() {
        for payload in [vec![1, 0], vec![2, 0]] {
            let (mut reader, mut writer) = paired_tls13();
            let mut sending = traffic_keys(writer.hash, writer.write.kind, &writer.write_secret);
            write_record(&mut writer.stream, 21, &payload, Some(&mut sending)).expect("close");
            let mut buf = [0u8; 4];
            assert_eq!(reader.read(&mut buf).expect("eof"), 0, "{payload:?}");
        }
        for payload in [vec![1, 0, 0], vec![2, 0, 0], vec![1, 0, 1], vec![0, 0], vec![1], Vec::new(), vec![1, 0, 0, 0], vec![255, 0]] {
            let (mut reader, mut writer) = paired_tls13();
            let mut sending = traffic_keys(writer.hash, writer.write.kind, &writer.write_secret);
            write_record(&mut writer.stream, 21, &payload, Some(&mut sending)).expect("malformed");
            let mut buf = [0u8; 4];
            let err = reader.read(&mut buf).expect_err("not eof");
            assert!(err.to_string().contains("malformed"), "{payload:?}: {err}");
            let (mut client, mut server) = peer();
            write_plain(&mut server, 21, &payload);
            let err = next_handshake(&mut client, &mut Vec::new(), &mut Vec::new(), &mut 0, None, deadline()).expect_err("handshake alert");
            assert!(err.contains("malformed"), "{payload:?}: {err}");
            assert!(!err.contains("alert 0"), "{payload:?}: {err}");
        }
        let (mut reader, writer) = paired_tls13();
        let keys = traffic_keys(writer.hash, writer.write.kind, &writer.write_secret);
        writer.stream.try_clone().expect("clone").write_all(&seal_raw_inner(&keys, &[1, 0, 21, 0, 0])).expect("padded close");
        let mut buf = [0u8; 4];
        assert_eq!(reader.read(&mut buf).expect("padded eof"), 0);
        let (mut reader, writer) = paired_tls13();
        let keys = traffic_keys(writer.hash, writer.write.kind, &writer.write_secret);
        writer.stream.try_clone().expect("clone").write_all(&seal_raw_inner(&keys, &[1, 0, 0, 21, 0])).expect("trailing zero");
        let err = reader.read(&mut buf).expect_err("trailing content");
        assert!(err.to_string().contains("malformed"), "{err}");
        let (mut client, mut server) = peer();
        write_plain(&mut server, 21, &[1, 0]);
        let err = next_handshake(&mut client, &mut Vec::new(), &mut Vec::new(), &mut 0, None, deadline()).expect_err("real close");
        assert!(err.contains("alert 0"), "{err}");
    }
}

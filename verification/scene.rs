//! Actual returned GLB files, Rust geometry, rasterization, and owned viewport lifetime.
mod twin;

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use twin::{b64_decode, b64_encode, create_scratch, n, obj, pretty, report_value, s, Controller, Value};

const SCOPE: &str = "Owner execution with actual HTTP-returned GLB files, Rust accessor/node/material validation and software rasterization, owned frame/close operations and filesystem mutations. Installed Android controls, gesture/coalescing/lifecycle and display acceptance remain unverified. Static triangle geometry with embedded PNG base-color sampling and alpha cutouts; animation and physical materials are outside this execution.";
const CENTER: usize = (64 * 128 + 64) * 4;

#[derive(Clone, Copy)]
enum P {
    K(&'static str),
    I(usize),
}

struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    used: usize,
    count: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            block: [0; 64],
            used: 0,
            count: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.count = self.count.checked_add(data.len() as u64).unwrap_or_else(|| panic!("digest length"));
        while !data.is_empty() {
            let n = (64 - self.used).min(data.len());
            self.block[self.used..self.used + n].copy_from_slice(&data[..n]);
            self.used += n;
            data = &data[n..];
            if self.used == 64 {
                self.compress();
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> String {
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 56 {
            self.block[self.used..].fill(0);
            self.compress();
            self.block.fill(0);
        } else {
            self.block[self.used..56].fill(0);
        }
        self.block[56..].copy_from_slice(&(self.count * 8).to_be_bytes());
        self.compress();
        self.state.iter().map(|n| format!("{n:08x}")).collect()
    }

    fn compress(&mut self) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
            0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
            0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
            0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
            0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
            0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
            0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(self.block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut x] = self.state;
        for i in 0..64 {
            let t1 = x
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) ^ (a & c) ^ (b & c));
            x = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (i, v) in [a, b, c, d, e, f, g, x].iter().enumerate() {
            self.state[i] = self.state[i].wrapping_add(*v);
        }
    }
}

fn sha256(data: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(data);
    digest.finish()
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for byte in data {
        c ^= u32::from(*byte);
        for _ in 0..8 {
            c = (c >> 1) ^ if c & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    !c
}

fn le_f32(value: f64) -> [u8; 4] {
    (value as f32).to_le_bytes()
}

fn pack_f32s(values: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend_from_slice(&le_f32(*value));
    }
    out
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if data.is_empty() {
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(!0u16).to_le_bytes());
    } else {
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + 65535).min(data.len());
            out.push(if end == data.len() { 1 } else { 0 });
            let len = (end - offset) as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(&data[offset..end]);
            offset = end;
        }
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(kind);
    crc_data.extend_from_slice(data);
    let mut out = Vec::with_capacity(12 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
    out
}

fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row = width as usize * 4;
    if rgba.len() != row * height as usize {
        panic!("png rgba length {} for {width}x{height}", rgba.len());
    }
    let mut scan = Vec::with_capacity(rgba.len() + height as usize);
    for y in 0..height as usize {
        scan.push(0);
        scan.extend_from_slice(&rgba[y * row..(y + 1) * row]);
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &zlib_store(&scan)));
    out.extend(chunk(b"IEND", b""));
    out
}

fn glb(doc: &Value, binary: &[u8]) -> Vec<u8> {
    let mut raw = doc.to_string().into_bytes();
    raw.extend(std::iter::repeat(b' ').take((4 - raw.len() % 4) % 4));
    let mut body = binary.to_vec();
    body.extend(std::iter::repeat(0).take((4 - body.len() % 4) % 4));
    let mut chunks = Vec::new();
    chunks.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    chunks.extend_from_slice(&raw);
    chunks.extend_from_slice(&(body.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&0x004e_4942u32.to_le_bytes());
    chunks.extend_from_slice(&body);
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((chunks.len() + 12) as u32).to_le_bytes());
    out.extend_from_slice(&chunks);
    out
}

fn o(fields: Vec<(&str, Value)>) -> Value {
    obj(fields.into_iter().map(|(key, value)| (key.to_string(), value)).collect())
}

fn a(items: Vec<Value>) -> Value {
    Value::Arr(items)
}

fn ni(number: i64) -> Value {
    n(&number.to_string())
}

fn ns(token: &str) -> Value {
    n(token)
}

fn sv(text: &str) -> Value {
    s(text)
}

fn ints(values: &[i64]) -> Value {
    a(values.iter().copied().map(ni).collect())
}

fn walk_mut<'a>(value: &'a mut Value, path: &[P]) -> &'a mut Value {
    if let Some((step, rest)) = path.split_first() {
        match step {
            P::K(key) => {
                let Value::Obj(fields) = value else { panic!("expected object for {key}") };
                let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
                walk_mut(&mut fields[index].1, rest)
            }
            P::I(index) => {
                let Value::Arr(items) = value else { panic!("expected array at {index}") };
                walk_mut(&mut items[*index], rest)
            }
        }
    } else {
        value
    }
}

fn set_path(value: &mut Value, path: &[P], next: Value) {
    *walk_mut(value, path) = next;
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("put on non-object") };
    if let Some(slot) = fields.iter_mut().find(|(name, _)| name == key) {
        slot.1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn put_at(value: &mut Value, path: &[P], key: &str, next: Value) {
    put(walk_mut(value, path), key, next);
}

fn pop_path(value: &mut Value, path: &[P], key: &str) {
    let Value::Obj(fields) = walk_mut(value, path) else { panic!("pop on non-object") };
    let before = fields.len();
    fields.retain(|(name, _)| name != key);
    if fields.len() == before {
        panic!("missing {key}");
    }
}

fn push_path(value: &mut Value, path: &[P], item: Value) {
    let Value::Arr(items) = walk_mut(value, path) else { panic!("push on non-array") };
    items.push(item);
}

fn clone_at(value: &Value, path: &[P]) -> Value {
    let mut current = value;
    for step in path {
        current = match step {
            P::K(key) => current.get(key).unwrap_or_else(|| panic!("missing {key}")),
            P::I(index) => current.arr().get(*index).unwrap_or_else(|| panic!("missing index {index}")),
        };
    }
    current.clone()
}

fn triangle() -> (Value, Vec<u8>) {
    let mut data = pack_f32s(&[-1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.0, 0.0]);
    data.extend_from_slice(&[0, 1, 2]);
    let doc = o(vec![
        ("asset", o(vec![("version", sv("2.0"))])),
        ("buffers", a(vec![o(vec![("byteLength", ni(data.len() as i64))])])),
        (
            "bufferViews",
            a(vec![
                o(vec![("buffer", ni(0)), ("byteOffset", ni(0)), ("byteLength", ni(36))]),
                o(vec![("buffer", ni(0)), ("byteOffset", ni(36)), ("byteLength", ni(3))]),
            ]),
        ),
        (
            "accessors",
            a(vec![
                o(vec![("bufferView", ni(0)), ("componentType", ni(5126)), ("count", ni(3)), ("type", sv("VEC3"))]),
                o(vec![("bufferView", ni(1)), ("componentType", ni(5121)), ("count", ni(3)), ("type", sv("SCALAR"))]),
            ]),
        ),
        (
            "meshes",
            a(vec![o(vec![(
                "primitives",
                a(vec![o(vec![
                    ("attributes", o(vec![("POSITION", ni(0))])),
                    ("indices", ni(1)),
                    ("material", ni(0)),
                ])]),
            )])]),
        ),
        (
            "materials",
            a(vec![o(vec![(
                "pbrMetallicRoughness",
                o(vec![("baseColorFactor", ints(&[1, 0, 0, 1]))]),
            )])]),
        ),
        ("nodes", a(vec![o(vec![("mesh", ni(0))])])),
        ("scenes", a(vec![o(vec![("nodes", ints(&[0]))])])),
        ("scene", ni(0)),
    ]);
    (doc, data)
}

fn image_rgba(alpha: u8) -> Vec<u8> {
    vec![0, 255, 0, alpha, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 255, 255]
}

fn textured_triangle(uv: (f64, f64), alpha: u8, kind: i64, coord: i64) -> (Value, Vec<u8>) {
    let (mut doc, data) = triangle();
    let mut binary = data[..36].to_vec();
    let coords = if kind == 5126 {
        let mut packed = Vec::new();
        for _ in 0..3 {
            packed.extend_from_slice(&le_f32(uv.0));
            packed.extend_from_slice(&le_f32(uv.1));
        }
        packed
    } else {
        let u = (uv.0 * 255.0).round() as u8;
        let v = (uv.1 * 255.0).round() as u8;
        vec![u, v, u, v, u, v]
    };
    binary.extend_from_slice(&coords);
    let image = png(2, 2, &image_rgba(alpha));
    let offset = binary.len() as i64;
    binary.extend_from_slice(&image);
    set_path(&mut doc, &[P::K("buffers"), P::I(0), P::K("byteLength")], ni(binary.len() as i64));
    set_path(
        &mut doc,
        &[P::K("bufferViews"), P::I(1)],
        o(vec![("buffer", ni(0)), ("byteOffset", ni(36)), ("byteLength", ni(coords.len() as i64))]),
    );
    push_path(
        &mut doc,
        &[P::K("bufferViews")],
        o(vec![("buffer", ni(0)), ("byteOffset", ni(offset)), ("byteLength", ni(image.len() as i64))]),
    );
    let mut accessor = o(vec![
        ("bufferView", ni(1)),
        ("componentType", ni(kind)),
        ("count", ni(3)),
        ("type", sv("VEC2")),
    ]);
    if kind != 5126 {
        put(&mut accessor, "normalized", Value::Bool(true));
    }
    set_path(&mut doc, &[P::K("accessors"), P::I(1)], accessor);
    let tex = format!("TEXCOORD_{coord}");
    set_path(
        &mut doc,
        &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0)],
        o(vec![
            ("attributes", obj(vec![("POSITION".into(), ni(0)), (tex, ni(1))])),
            ("material", ni(0)),
        ]),
    );
    set_path(
        &mut doc,
        &[P::K("materials"), P::I(0), P::K("pbrMetallicRoughness")],
        o(vec![
            ("baseColorFactor", ints(&[1, 1, 1, 1])),
            ("baseColorTexture", o(vec![("index", ni(0)), ("texCoord", ni(coord))])),
        ]),
    );
    put(&mut doc, "images", a(vec![o(vec![("bufferView", ni(2)), ("mimeType", sv("image/png"))])]));
    put(&mut doc, "textures", a(vec![o(vec![("source", ni(0)), ("sampler", ni(0))])]));
    put(
        &mut doc,
        "samplers",
        a(vec![o(vec![
            ("magFilter", ni(9728)),
            ("minFilter", ni(9728)),
            ("wrapS", ni(33071)),
            ("wrapT", ni(33071)),
        ])]),
    );
    (doc, binary)
}

fn set_buffer_len(doc: &mut Value, len: usize) {
    set_path(doc, &[P::K("buffers"), P::I(0), P::K("byteLength")], ni(len as i64));
}

fn self_check() {
    assert_eq!(crc32(b"IEND"), 0xae42_6082, "png crc");
    assert_eq!(sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(f32::from_bits(0x7fc0_0000).to_le_bytes(), [0x00, 0x00, 0xc0, 0x7f]);
    let (doc, binary) = triangle();
    let blob = glb(&doc, &binary);
    assert_eq!(&blob[..4], b"glTF");
    let total = u32::from_le_bytes(blob[8..12].try_into().unwrap()) as usize;
    assert_eq!(total, blob.len());
    assert!(png(1, 1, &[1, 2, 3, 4]).windows(2).any(|pair| pair == [0x78, 0x01]));
}

fn pass(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn pass_msg(passed: &mut Vec<String>, name: &str, ok: bool, detail: &str) {
    if !ok {
        panic!("{name}: {detail}");
    }
    passed.push(name.to_string());
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).unwrap_or_else(|| panic!("missing {key} in {}", value.to_string())).str_()
}

fn num(value: &Value, key: &str) -> i64 {
    match value.get(key) {
        Some(Value::Num(token)) => token.parse().unwrap_or_else(|_| panic!("{key} number {token}")),
        Some(other) => panic!("{key} is {}", other.to_string()),
        None => panic!("missing {key}"),
    }
}

fn notice(state: &Value) -> String {
    state.get("notice").map(|value| value.to_string()).unwrap_or_else(|| "no notice".into())
}

fn pixels(frame: &Value) -> Vec<u8> {
    b64_decode(field(frame, "rgba"))
}

fn red_count(frame: &Value) -> usize {
    pixels(frame).chunks_exact(4).filter(|px| px[0] > 200 && px[1] < 10 && px[2] < 10 && px[3] == 255).count()
}

fn center4(px: &[u8]) -> [u8; 4] {
    [px[CENTER], px[CENTER + 1], px[CENTER + 2], px[CENTER + 3]]
}

fn merge(owner: &Value, extra: Vec<(String, Value)>) -> Value {
    let Value::Obj(fields) = owner else { panic!("owner is not an object") };
    let mut out = fields.clone();
    for (key, value) in extra {
        if let Some(slot) = out.iter_mut().find(|(name, _)| name == &key) {
            slot.1 = value;
        } else {
            out.push((key, value));
        }
    }
    Value::Obj(out)
}

fn device(pairs: &[(&str, u64)]) -> Value {
    obj(pairs.iter().map(|(key, value)| ((*key).to_string(), n(&value.to_string()))).collect())
}

fn text_present(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if !text.is_empty())
}

fn port_of(value: &Value) -> u16 {
    match value.get("port") {
        Some(Value::Num(token)) => token.parse().unwrap_or_else(|_| panic!("port {token}")),
        other => panic!("missing port {}", other.map(|v| v.to_string()).unwrap_or_else(|| "absent".into())),
    }
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    if let Some(Value::Str(text)) = effect.get("wire_base64") {
        if !text.is_empty() {
            return b64_decode(text);
        }
    }
    if let Some(Value::Str(text)) = effect.get("wire") {
        return text.as_bytes().to_vec();
    }
    panic!("effect has no request wire: {}", effect.to_string());
}

struct CancelOp<'a> {
    ctrl: &'a mut Controller,
    op_name: &'static str,
    purpose: String,
    ticket: String,
    armed: bool,
}

impl Drop for CancelOp<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let envelope = obj(vec![
            ("op".into(), s(self.op_name)),
            ("purpose".into(), s(&self.purpose)),
            ("ticket".into(), s(&self.ticket)),
        ]);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.ctrl.send(&envelope);
        }));
    }
}

impl CancelOp<'_> {
    fn op(&mut self, envelope: &Value) -> Value {
        self.ctrl.op(envelope)
    }

    fn disarm_and_cancel(&mut self) {
        self.armed = false;
        let envelope = obj(vec![
            ("op".into(), s(self.op_name)),
            ("purpose".into(), s(&self.purpose)),
            ("ticket".into(), s(&self.ticket)),
        ]);
        let _ = self.ctrl.op(&envelope);
    }
}

fn read_request(sock: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = sock.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 1024 * 1024 {
            return None;
        }
    };
    let header = std::str::from_utf8(&buf[..header_end]).ok()?;
    let mut length: Option<usize> = None;
    for line in header.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length?;
    while buf.len() < header_end + length {
        let n = sock.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    Some(buf[header_end..header_end + length].to_vec())
}

fn serve(listener: TcpListener, payload: Arc<Mutex<Vec<u8>>>, requests: Arc<Mutex<Vec<Vec<u8>>>>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut sock, _)) => {
                let _ = sock.set_nonblocking(false);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = sock.set_write_timeout(Some(Duration::from_secs(5)));
                if let Some(body) = read_request(&mut sock) {
                    if let Ok(mut list) = requests.lock() {
                        list.push(body);
                    }
                    let bytes = payload.lock().map(|guard| guard.clone()).unwrap_or_default();
                    let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
                    let _ = sock.write_all(header.as_bytes());
                    let _ = sock.write_all(&bytes);
                    let _ = sock.flush();
                    let _ = sock.shutdown(Shutdown::Write);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock || error.kind() == std::io::ErrorKind::Interrupted => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

struct PriorController {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<BufReader<std::process::ChildStdout>>,
}

impl PriorController {
    fn spawn(path: &Path, cwd: &Path) -> Self {
        let binary = std::fs::canonicalize(path).unwrap_or_else(|error| {
            panic!("previous scene controller {}: {error}", path.display())
        });
        let mut child = Command::new(&binary)
            .arg("--jsonl")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", binary.display()));
        let stdin = child.stdin.take().unwrap_or_else(|| panic!("previous scene controller stdin was not piped"));
        let stdout = child.stdout.take().unwrap_or_else(|| panic!("previous scene controller stdout was not piped"));
        Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: Some(BufReader::new(stdout)),
        }
    }

    fn send(&mut self, envelope: &Value) -> Value {
        let stdin = self.stdin.as_mut().unwrap_or_else(|| panic!("previous scene controller stdin is closed"));
        let line = envelope.to_string();
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .unwrap_or_else(|error| panic!("previous scene controller write: {error}"));
        let stdout = self.stdout.as_mut().unwrap_or_else(|| panic!("previous scene controller stdout is closed"));
        let mut reply = String::new();
        loop {
            match stdout.read_line(&mut reply) {
                Ok(0) => panic!("previous scene controller closed stdout"),
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("previous scene controller read: {error}"),
            }
        }
        let text = reply.trim_end_matches(['\n', '\r']);
        match Value::parse(text) {
            Value::Obj(fields) => Value::Obj(fields),
            _ => panic!("{text}"),
        }
    }

    fn op(&mut self, envelope: &Value) -> Value {
        let reply = self.send(envelope);
        match reply.get("value") {
            Some(value) => value.clone(),
            None => panic!("{}", reply.to_string()),
        }
    }

    fn shutdown(&mut self) {
        self.stdin.take();
        let Some(mut child) = self.child.take() else {
            self.stdout.take();
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        let timed_out = loop {
            match child.try_wait() {
                Ok(Some(_)) => break false,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break true;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    self.stdout.take();
                    if !std::thread::panicking() {
                        panic!("previous scene controller wait: {error}");
                    }
                    return;
                }
            }
        };
        self.stdout.take();
        if timed_out && !std::thread::panicking() {
            panic!("previous scene controller did not exit within 15s");
        }
    }
}

impl Drop for PriorController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Session {
    ctrl: Controller,
    root: PathBuf,
    root_text: String,
    project: PathBuf,
    payload: Arc<Mutex<Vec<u8>>>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    returned: Vec<u8>,
    port: u16,
    previous: Option<PathBuf>,
}

impl Session {
    fn request_len(&self) -> usize {
        self.requests.lock().unwrap_or_else(|error| panic!("request lock: {error}")).len()
    }

    fn serve_bytes(&mut self, bytes: Vec<u8>) {
        *self.payload.lock().unwrap_or_else(|error| panic!("payload lock: {error}")) = bytes.clone();
        self.returned = bytes;
    }

    fn config(&self) -> String {
        let url = format!("http://127.0.0.1:{}", self.port);
        o(vec![
            ("version", ni(1)),
            (
                "hosts",
                a(vec![o(vec![
                    ("id", sv("scene-host")),
                    ("url", sv(&url)),
                    ("plain", Value::Bool(true)),
                    ("enabled", Value::Bool(true)),
                ])]),
            ),
            (
                "routes",
                a(vec![o(vec![
                    ("id", sv("scene")),
                    ("host", sv("scene-host")),
                    ("capability", sv("scene_generation")),
                    ("path", sv("/generate")),
                    ("method", sv("POST")),
                    ("request", o(vec![("prompt", o(vec![("$input", sv("/prompt"))]))])),
                    ("response", sv("binary")),
                    ("artifact_kind", sv("scene")),
                    ("enabled", Value::Bool(true)),
                ])]),
            ),
            ("defaults", o(vec![("scene_generation", o(vec![("routes", a(vec![sv("scene")]))]))])),
        ])
        .to_string()
    }

    fn next(&mut self, event: &Value, action: &str, inputs: Value) -> Value {
        let state = event.get("state").cloned().unwrap_or(Value::Null);
        self.ctrl.event(Some(&state), action, inputs, &self.root_text)
    }

    fn upload(&mut self, effect: &Value, sock: &mut TcpStream) {
        if !text_present(effect, "media_upload") {
            return;
        }
        let purpose = field(effect, "purpose").to_string();
        let ticket = field(effect, "media_upload").to_string();
        let limit = num(effect, "upload_bytes") as usize;
        let mut guard = CancelOp {
            ctrl: &mut self.ctrl,
            op_name: "media_upload_cancel",
            purpose,
            ticket,
            armed: true,
        };
        let mut count = 0usize;
        loop {
            let part = guard.op(&obj(vec![
                ("op".into(), s("media_upload_read")),
                ("purpose".into(), s(&guard.purpose)),
                ("ticket".into(), s(&guard.ticket)),
            ]));
            let data = b64_decode(field(&part, "data"));
            if data.len() > 8192 {
                panic!("upload chunk exceeds 8192");
            }
            count += data.len();
            if count > limit {
                panic!("upload exceeds declared length");
            }
            sock.write_all(&data).unwrap_or_else(|error| panic!("upload write: {error}"));
            let done = part.get("done").map(|value| value.bool_()).unwrap_or(false);
            if done {
                if count != limit {
                    panic!("upload length mismatch");
                }
                break;
            }
            if data.is_empty() {
                panic!("empty upload chunk before completion");
            }
        }
        guard.disarm_and_cancel();
    }

    fn response(&mut self, effect: &Value, sock: &mut TcpStream) -> Value {
        if !text_present(effect, "media_download") {
            let mut data = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match sock.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => data.extend_from_slice(&buf[..n]),
                    Err(error) => panic!("response read: {error}"),
                }
            }
            return obj(vec![("data".into(), s(&b64_encode(&data)))]);
        }
        let purpose = field(effect, "purpose").to_string();
        let ticket = field(effect, "media_download").to_string();
        let mut guard = CancelOp {
            ctrl: &mut self.ctrl,
            op_name: "media_stream_cancel",
            purpose,
            ticket,
            armed: true,
        };
        let mut ready = false;
        let mut buf = [0u8; 8192];
        loop {
            let n = sock.read(&mut buf).unwrap_or_else(|error| panic!("media response read: {error}"));
            if n == 0 {
                break;
            }
            let data = b64_encode(&buf[..n]);
            let reply = guard.op(&obj(vec![
                ("op".into(), s("media_stream_feed")),
                ("purpose".into(), s(&guard.purpose)),
                ("ticket".into(), s(&guard.ticket)),
                ("data".into(), s(&data)),
            ]));
            ready = reply.get("ready").map(|value| value.bool_()).unwrap_or(false);
            if ready {
                break;
            }
        }
        let token = guard.op(&obj(vec![
            ("op".into(), s("media_stream_finish")),
            ("purpose".into(), s(&guard.purpose)),
            ("ticket".into(), s(&guard.ticket)),
            ("eof".into(), Value::Bool(!ready)),
        ]));
        guard.disarm_and_cancel();
        obj(vec![("media_receipt".into(), token)])
    }

    fn exchange(&mut self, effect: &Value) -> Value {
        let host = field(effect, "host").to_string();
        let port = port_of(effect);
        let wire = wire_bytes(effect);
        let addr: SocketAddr = format!("{host}:{port}").parse().unwrap_or_else(|error| panic!("socket {host}:{port}: {error}"));
        let mut sock = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).unwrap_or_else(|error| panic!("connect {addr}: {error}"));
        let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = sock.set_write_timeout(Some(Duration::from_secs(5)));
        sock.write_all(&wire).unwrap_or_else(|error| panic!("send wire: {error}"));
        sock.flush().unwrap_or_else(|error| panic!("flush wire: {error}"));
        self.upload(effect, &mut sock);
        self.response(effect, &mut sock)
    }

    fn received_blob(&mut self, blob: Vec<u8>) -> (Value, Value) {
        self.serve_bytes(blob);
        let saved = self.config();
        let mut event = self.ctrl.event(None, "resume", obj(vec![]), &self.root_text);
        event = self.next(&event, "service_connections", obj(vec![]));
        event = self.next(&event, "service_save", obj(vec![("service_config".into(), s(&saved))]));
        event = self.next(&event, "service_request", obj(vec![]));
        event = self.next(
            &event,
            "service_review",
            obj(vec![
                ("service_capability".into(), s("scene_generation")),
                ("service_input".into(), s("{\"prompt\":\"A triangle\"}")),
                ("service_file".into(), Value::Bool(false)),
            ]),
        );
        let reviewed = event.get("state").cloned().unwrap_or(Value::Null);
        let dialog = reviewed.get("dialog").and_then(|value| match value {
            Value::Str(text) => Some(text.as_str()),
            _ => None,
        });
        if dialog != Some("approval") {
            panic!("approval dialog missing: {}", notice(&reviewed));
        }
        event = self.next(&event, "approve", obj(vec![]));
        let effect = event.get("effect").cloned().unwrap_or(Value::Null);
        if matches!(effect, Value::Null) {
            panic!("approve returned no effect: {}", notice(event.get("state").unwrap_or(&Value::Null)));
        }
        let purpose = field(&effect, "purpose").to_string();
        let mut inputs = self.exchange(&effect);
        put(&mut inputs, "purpose", s(&purpose));
        event = self.next(&event, "network_result", inputs);
        let state = event.get("state").cloned().unwrap_or(Value::Null);
        if matches!(state.get("service_artifact"), None | Some(Value::Null)) {
            panic!("service_artifact missing: {}", notice(&state));
        }
        let preview = self.ctrl.event(Some(&state), "media_preview", obj(vec![]), &self.root_text);
        let preview_effect = preview.get("effect").cloned().unwrap_or(Value::Null);
        if field(&preview_effect, "media_kind") != "scene" {
            panic!("media preview kind: {}", preview_effect.to_string());
        }
        let owner = obj(vec![
            ("root".into(), preview_effect.get("root").cloned().unwrap_or(Value::Null)),
            ("root_identity".into(), preview_effect.get("root_identity").cloned().unwrap_or(Value::Null)),
            ("chat".into(), preview_effect.get("chat").cloned().unwrap_or(Value::Null)),
            ("record".into(), preview_effect.get("record").cloned().unwrap_or(Value::Null)),
        ]);
        (state, owner)
    }

    fn received_doc(&mut self, doc: &Value, binary: &[u8]) -> (Value, Value) {
        let blob = glb(doc, binary);
        self.received_blob(blob)
    }

    fn received_triangle(&mut self) -> (Value, Value) {
        let (doc, binary) = triangle();
        self.received_doc(&doc, &binary)
    }

    fn artifact_path(&self, owner: &Value) -> PathBuf {
        let record = owner.get("record").unwrap_or_else(|| panic!("owner record missing"));
        let path = field(record, "path");
        let candidate = Path::new(path);
        if candidate.is_absolute() { candidate.to_path_buf() } else { self.root.join(candidate) }
    }

    fn artifact(&self, owner: &Value) -> Vec<u8> {
        let path = self.artifact_path(owner);
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    fn open_env(owner: &Value, device_value: Option<Value>) -> Value {
        let mut extra = vec![("op".into(), s("scene_open"))];
        if let Some(device_value) = device_value {
            extra.push(("device".into(), device_value));
        }
        merge(owner, extra)
    }

    fn open_value(&mut self, owner: &Value, device_value: Option<Value>) -> Value {
        let reply = self.ctrl.send(&Self::open_env(owner, device_value));
        match reply.get("value") {
            Some(value) => value.clone(),
            None => panic!("scene_open failed: {}", reply.to_string()),
        }
    }

    fn must_open(&mut self, owner: &Value, device_value: Option<Value>) -> String {
        field(&self.open_value(owner, device_value), "scene_ticket").to_string()
    }

    fn close_scene(&mut self, owner: &Value, ticket: &str) {
        let _ = self.ctrl.op(&merge(
            owner,
            vec![("op".into(), s("scene_close")), ("scene_ticket".into(), s(ticket))],
        ));
    }

    fn frame_at(&mut self, owner: &Value, ticket: &str, camera: &[(&str, &str)]) -> Value {
        self.ctrl.op(&frame_env(owner, ticket, camera))
    }

    fn frame_bare(&mut self, owner: &Value, ticket: &str) -> Value {
        self.ctrl.op(&merge(owner, vec![("op".into(), s("scene_frame")), ("scene_ticket".into(), s(ticket))]))
    }

    fn rejected(&mut self, envelope: Value) -> bool {
        self.ctrl.send(&envelope).get("error").is_some()
    }

    fn bad(&mut self, passed: &mut Vec<String>, name: &str, doc: &Value, binary: &[u8]) {
        let (_state, owner) = self.received_doc(doc, binary);
        let reply = self.ctrl.send(&Self::open_env(&owner, None));
        let err = reply.get("error").is_some();
        let same = self.artifact(&owner) == self.returned;
        pass_msg(passed, name, err && same, &format!("error={err} bytes_match={same} reply={}", reply.to_string()));
    }

    fn run(&mut self, passed: &mut Vec<String>) {
        let (state, owner) = self.received_triangle();
        let digest = sha256(&self.returned);
        let record_sha = field(owner.get("record").unwrap(), "sha256").to_string();
        let same_file = self.artifact(&owner) == self.returned;
        pass_msg(
            passed,
            "actual HTTP scene bytes retain their returned digest",
            digest == record_sha && same_file,
            &format!("hash_match={} bytes_match={same_file}", digest == record_sha),
        );
        let menu = self.ctrl.event(Some(&state), "media_menu", obj(vec![]), &self.root_text);
        let preview_button = menu
            .get("ui")
            .and_then(|ui| ui.get("modal"))
            .and_then(|modal| modal.get("buttons"))
            .map(|buttons| buttons.arr().iter().any(|button| button.get("event").map(|event| event.str_()) == Some("media_preview")))
            .unwrap_or(false);
        pass(passed, "returned scene has a real preview action in the media menu", preview_button);
        let ticket = self.must_open(&owner, None);
        let frame0 = self.frame_at(&owner, &ticket, &[]);
        let px = pixels(&frame0);
        let geometry_ok = num(&frame0, "width") == 128
            && num(&frame0, "height") == 128
            && px.len() == 128 * 128 * 4
            && num(&frame0, "triangles") == 1
            && red_count(&frame0) > 1000;
        pass_msg(
            passed,
            "Rust renders actual indexed geometry into complete RGBA pixels",
            geometry_ok,
            &format!("width {} height {} bytes {} triangles {} red {}", num(&frame0, "width"), num(&frame0, "height"), px.len(), num(&frame0, "triangles"), red_count(&frame0)),
        );
        let description = field(&frame0, "description");
        pass(
            passed,
            "frame retains exact source provenance and explicit geometry limits",
            field(&frame0, "source_sha256") == record_sha && description.contains("embedded PNG textures") && description.contains("alpha cutouts"),
        );
        let again = self.frame_at(&owner, &ticket, &[]);
        pass(passed, "software frame is deterministic for the same source and camera", field(&again, "rgba") == field(&frame0, "rgba"));
        let workers = num(&frame0, "cpu_workers");
        pass(passed, "default frame uses bounded actual host workers", (1..=4).contains(&workers));
        let one = self.must_open(&owner, Some(device(&[("cores", 1)])));
        let single = self.frame_at(&owner, &one, &[]);
        let single_again = self.frame_at(&owner, &one, &[]);
        pass(
            passed,
            "measured single-core route retains identical actual pixels",
            field(&single, "rgba") == field(&frame0, "rgba") && num(&single_again, "cpu_workers") == 1,
        );
        self.close_scene(&owner, &one);
        self.previous_pixels(passed, &owner, &ticket);
        let rotated = self.frame_at(&owner, &ticket, &[("yaw", "0.7")]);
        let zoomed = self.frame_at(&owner, &ticket, &[("zoom", "1.5")]);
        pass(passed, "orbit changes the actual scene pixels", field(&rotated, "rgba") != field(&frame0, "rgba"));
        pass(passed, "zoom enlarges actual geometry coverage", red_count(&zoomed) > red_count(&frame0));
        pass(passed, "background is rendered outside the geometry", px[..4] == [23, 27, 34, 255]);
        let wrong = merge(&owner, vec![("chat".into(), s("different"))]);
        let frame_denied = self.rejected(merge(&wrong, vec![("op".into(), s("scene_frame")), ("scene_ticket".into(), s(&ticket))]));
        let close_denied = self.rejected(merge(&wrong, vec![("op".into(), s("scene_close")), ("scene_ticket".into(), s(&ticket))]));
        pass(passed, "another conversation cannot render or close this viewport", frame_denied && close_denied);
        let kept = self.frame_at(&owner, &ticket, &[]);
        pass(passed, "failed ownership leaves the actual viewport available", field(&kept, "rgba") == field(&frame0, "rgba"));
        for (shown, key, token) in [
            ("{\"size\": 127}", "size", "127"),
            ("{\"size\": 769}", "size", "769"),
            ("{\"zoom\": 0}", "zoom", "0"),
            ("{\"zoom\": 9}", "zoom", "9"),
            ("{\"yaw\": 10001}", "yaw", "10001"),
            ("{\"pitch\": -10001}", "pitch", "-10001"),
        ] {
            let name = format!("invalid camera is scoped {shown}");
            let denied = self.rejected(merge(
                &owner,
                vec![("op".into(), s("scene_frame")), ("scene_ticket".into(), s(&ticket)), (key.to_string(), n(token))],
            ));
            pass(passed, &name, denied);
        }
        let limited = self.open_value(
            &owner,
            Some(device(&[("heap_bytes", 8 * 1024 * 1024), ("memory_bytes", 128 * 1024 * 1024), ("cores", 1)])),
        );
        let max_size = num(&limited, "max_size");
        pass(passed, "measured small memory admits a smaller real viewport", (128..384).contains(&max_size));
        let small = field(&limited, "scene_ticket").to_string();
        let small_frame = self.frame_at(&owner, &small, &[]);
        pass(passed, "small-memory frame remains functional", num(&small_frame, "triangles") == 1);
        let omitted = self.frame_bare(&owner, &small);
        pass(passed, "omitted frame size uses the admitted small-memory resolution", num(&omitted, "width") == max_size);
        let oversized = self.rejected(merge(
            &owner,
            vec![("op".into(), s("scene_frame")), ("scene_ticket".into(), s(&small)), ("size".into(), n("384"))],
        ));
        pass(passed, "oversized frame is refused within its own memory budget", oversized);
        self.close_scene(&owner, &small);
        let low_memory = self.rejected(merge(
            &owner,
            vec![(
                "op".into(),
                s("scene_open"),
            ), (
                "device".into(),
                device(&[("heap_bytes", 1024 * 1024), ("memory_bytes", 128 * 1024 * 1024), ("cores", 1)]),
            )],
        ));
        pass(passed, "insufficient measured memory refuses only this optional viewport", low_memory);
        let second = self.must_open(&owner, None);
        let third = self.rejected(merge(&owner, vec![("op".into(), s("scene_open"))]));
        pass(passed, "viewport admission refuses a third live scene", third);
        self.close_scene(&owner, &second);
        let reply = self.ctrl.send(&Self::open_env(&owner, None));
        let opened = match reply.get("value").and_then(|value| value.get("scene_ticket")) {
            Some(Value::Str(text)) => text.clone(),
            _ => String::new(),
        };
        pass(passed, "closing releases its scene slot", !opened.is_empty());
        self.close_scene(&owner, &opened);
        self.close_scene(&owner, &ticket);
        let stale = self.rejected(merge(&owner, vec![("op".into(), s("scene_frame")), ("scene_ticket".into(), s(&ticket))]));
        pass(passed, "closed viewport cannot render stale frames", stale);

        let alpha_cases = [
            ("0.49", "None", false, ns("0.49"), None),
            ("0.5", "None", true, ns("0.5"), None),
            ("0.75", "0.75", true, ns("0.75"), Some(ns("0.75"))),
            ("1", "1.1", false, ni(1), Some(ns("1.1"))),
            ("0", "0", true, ni(0), Some(ni(0))),
        ];
        for (alpha_text, cutoff_text, visible, alpha, cutoff) in alpha_cases {
            let (mut doc, data) = triangle();
            put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("MASK"));
            set_path(&mut doc, &[P::K("materials"), P::I(0), P::K("pbrMetallicRoughness"), P::K("baseColorFactor"), P::I(3)], alpha);
            if let Some(cutoff) = cutoff {
                put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaCutoff", cutoff);
            }
            let (_state, masked) = self.received_doc(&doc, &data);
            let held = self.must_open(&masked, None);
            let image = self.frame_at(&masked, &held, &[]);
            let name = format!("alpha mask threshold uses actual rendered coverage ({alpha_text}, {cutoff_text})");
            let ok = if visible { field(&image, "rgba") == field(&frame0, "rgba") } else { red_count(&image) == 0 };
            pass(passed, &name, ok);
            self.close_scene(&masked, &held);
        }

        let (mut doc, data) = triangle();
        put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("OPAQUE"));
        put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaCutoff", ni(2));
        set_path(&mut doc, &[P::K("materials"), P::I(0), P::K("pbrMetallicRoughness"), P::K("baseColorFactor"), P::I(3)], ni(0));
        let (_state, opaque) = self.received_doc(&doc, &data);
        let held = self.must_open(&opaque, None);
        let opaque_frame = self.frame_at(&opaque, &held, &[]);
        pass(passed, "opaque mode ignores alpha and cutoff", field(&opaque_frame, "rgba") == field(&frame0, "rgba"));
        self.close_scene(&opaque, &held);

        for reverse in [false, true] {
            let (mut doc, data) = triangle();
            put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("MASK"));
            set_path(&mut doc, &[P::K("materials"), P::I(0), P::K("pbrMetallicRoughness"), P::K("baseColorFactor"), P::I(3)], ns("0.1"));
            push_path(&mut doc, &[P::K("materials")], o(vec![("pbrMetallicRoughness", o(vec![("baseColorFactor", ints(&[0, 0, 1, 1]))]))]));
            let mesh = clone_at(&doc, &[P::K("meshes"), P::I(0)]);
            push_path(&mut doc, &[P::K("meshes")], mesh);
            set_path(&mut doc, &[P::K("meshes"), P::I(1), P::K("primitives"), P::I(0), P::K("material")], ni(1));
            set_path(
                &mut doc,
                &[P::K("nodes")],
                a(vec![
                    o(vec![("mesh", ni(0)), ("translation", a(vec![ni(0), ni(0), ns("0.5")]))]),
                    o(vec![("mesh", ni(1)), ("translation", a(vec![ni(0), ni(0), ns("-0.5")]))]),
                ]),
            );
            let order = if reverse { ints(&[1, 0]) } else { ints(&[0, 1]) };
            set_path(&mut doc, &[P::K("scenes"), P::I(0), P::K("nodes")], order);
            let (_state, layered) = self.received_doc(&doc, &data);
            let held = self.must_open(&layered, None);
            let color = pixels(&self.frame_at(&layered, &held, &[]));
            let name = format!("discarded mask does not occlude far geometry {}", if reverse { "True" } else { "False" });
            pass(passed, &name, color[CENTER + 2] > 200 && color[CENTER] < 10);
            self.close_scene(&layered, &held);
        }

        let (mut doc, data) = triangle();
        let mut bin = data[..36].to_vec();
        bin.extend(pack_f32s(&[1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0]));
        set_buffer_len(&mut doc, bin.len());
        set_path(&mut doc, &[P::K("bufferViews"), P::I(1)], o(vec![("buffer", ni(0)), ("byteOffset", ni(36)), ("byteLength", ni(48))]));
        set_path(
            &mut doc,
            &[P::K("accessors"), P::I(1)],
            o(vec![("bufferView", ni(1)), ("componentType", ni(5126)), ("count", ni(3)), ("type", sv("VEC4"))]),
        );
        set_path(
            &mut doc,
            &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0)],
            o(vec![
                ("attributes", o(vec![("POSITION", ni(0)), ("COLOR_0", ni(1))])),
                ("material", ni(0)),
            ]),
        );
        put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("MASK"));
        let (_state, gradient) = self.received_doc(&doc, &bin);
        let held = self.must_open(&gradient, None);
        let image = self.frame_at(&gradient, &held, &[]);
        let partial = red_count(&image);
        let full = red_count(&frame0);
        pass(passed, "interpolated vertex alpha produces a partial cutout", partial > 0 && partial < full);
        let cores = self.must_open(&gradient, Some(device(&[("cores", 1)])));
        let core_frame = self.frame_at(&gradient, &cores, &[]);
        pass(passed, "alpha cutout matches exactly across CPU worker counts", field(&core_frame, "rgba") == field(&image, "rgba"));
        self.close_scene(&gradient, &cores);
        self.close_scene(&gradient, &held);

        for (label, cutoff) in [("negative", ns("-0.1")), ("string", sv("0.5"))] {
            let (mut doc, data) = triangle();
            put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("MASK"));
            put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaCutoff", cutoff);
            self.bad(passed, &format!("{label} alpha cutoff refuses only its scene preview"), &doc, &data);
        }

        for (kind, coord) in [(5126_i64, 0_i64), (5121, 1)] {
            let (doc, data) = textured_triangle((0.25, 0.25), 255, kind, coord);
            let before = self.request_len();
            let (_state, texture_owner) = self.received_doc(&doc, &data);
            let held = self.must_open(&texture_owner, None);
            let image = self.frame_at(&texture_owner, &held, &[]);
            let color = center4(&pixels(&image));
            pass_msg(
                passed,
                &format!("embedded PNG and selected UV accessor reach actual pixels ({kind}, {coord})"),
                color[1] > 240 && color[0] < 10 && color[2] < 10 && self.request_len() == before + 1,
                &format!("rgba={color:?} requests={}", self.request_len() - before),
            );
            let cores = self.must_open(&texture_owner, Some(device(&[("cores", 1)])));
            let core_frame = self.frame_at(&texture_owner, &cores, &[]);
            pass(passed, &format!("textured frame matches exactly across CPU counts {kind}"), field(&core_frame, "rgba") == field(&image, "rgba"));
            self.close_scene(&texture_owner, &cores);
            self.close_scene(&texture_owner, &held);
        }

        for (u, v, shown_u, shown_v, channel) in [(-0.1, 0.25, "-0.1", "0.25", 2usize), (1.1, 0.25, "1.1", "0.25", 1), (0.25, 1.1, "0.25", "1.1", 1)] {
            let (mut doc, data) = textured_triangle((u, v), 255, 5126, 0);
            put_at(&mut doc, &[P::K("samplers"), P::I(0)], "wrapS", ni(10497));
            put_at(&mut doc, &[P::K("samplers"), P::I(0)], "wrapT", ni(10497));
            let (_state, texture_owner) = self.received_doc(&doc, &data);
            let held = self.must_open(&texture_owner, None);
            let color = center4(&pixels(&self.frame_at(&texture_owner, &held, &[])));
            pass(passed, &format!("wrapped texture reaches actual pixel ({shown_u}, {shown_v})"), color[channel] > 240);
            self.close_scene(&texture_owner, &held);
        }

        let (mut doc, data) = textured_triangle((0.5, 0.25), 255, 5126, 0);
        put_at(&mut doc, &[P::K("samplers"), P::I(0)], "magFilter", ni(9729));
        let (_state, texture_owner) = self.received_doc(&doc, &data);
        let held = self.must_open(&texture_owner, None);
        let color = center4(&pixels(&self.frame_at(&texture_owner, &held, &[])));
        pass(
            passed,
            "linear texture filtering combines adjacent linear color samples",
            color[0] < 10 && (170..200).contains(&color[1]) && (170..200).contains(&color[2]),
        );
        self.close_scene(&texture_owner, &held);

        let (doc, data) = textured_triangle((0.25, 0.25), 255, 5126, 0);
        let mut varied = data[..36].to_vec();
        varied.extend(pack_f32s(&[0.0, 1.0, 1.0, 1.0, 0.5, 0.0]));
        varied.extend_from_slice(&data[60..]);
        let (_state, texture_owner) = self.received_doc(&doc, &varied);
        let held = self.must_open(&texture_owner, None);
        let image = self.frame_at(&texture_owner, &held, &[]);
        let colored = pixels(&image);
        let mut unique = BTreeSet::new();
        for px in colored.chunks_exact(4) {
            unique.insert([px[0], px[1], px[2], px[3]]);
        }
        pass(passed, "varying UVs shade the actual triangle with all four image colors", unique.len() >= 5);
        let png_path = self.project.join("output/checks/scene-texture.png");
        if let Some(parent) = png_path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("png dir: {error}"));
        }
        std::fs::write(&png_path, png(128, 128, &colored)).unwrap_or_else(|error| panic!("png write: {error}"));
        self.close_scene(&texture_owner, &held);

        let (mut doc, data) = textured_triangle((0.25, 0.25), 0, 5126, 0);
        put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("MASK"));
        let (_state, texture_owner) = self.received_doc(&doc, &data);
        let held = self.must_open(&texture_owner, None);
        let color = center4(&pixels(&self.frame_at(&texture_owner, &held, &[])));
        pass(passed, "sampled texture alpha discards covered pixels before depth", color == [23, 27, 34, 255]);
        self.close_scene(&texture_owner, &held);

        for label in ["missing UV", "invalid wrap", "invalid filter", "image escapes buffer", "mismatched UV count"] {
            let (mut doc, data) = textured_triangle((0.25, 0.25), 255, 5126, 0);
            match label {
                "missing UV" => pop_path(&mut doc, &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0), P::K("attributes")], "TEXCOORD_0"),
                "invalid wrap" => put_at(&mut doc, &[P::K("samplers"), P::I(0)], "wrapS", ni(0)),
                "invalid filter" => put_at(&mut doc, &[P::K("samplers"), P::I(0)], "magFilter", ni(0)),
                "image escapes buffer" => put_at(&mut doc, &[P::K("bufferViews"), P::I(2)], "byteLength", ni(999999)),
                "mismatched UV count" => put_at(&mut doc, &[P::K("accessors"), P::I(1)], "count", ni(2)),
                _ => panic!("unknown texture fault {label}"),
            }
            self.bad(passed, &format!("{label} refuses only the requested textured preview"), &doc, &data);
        }

        let (doc, mut data) = textured_triangle((0.25, 0.25), 255, 5126, 0);
        let last = data.len() - 1;
        data[last] ^= 1;
        self.bad(passed, "corrupt embedded PNG refuses without replacing original artifact", &doc, &data);

        let (mut doc, data) = triangle();
        put_at(&mut doc, &[P::K("nodes"), P::I(0)], "translation", ints(&[100, 200, 300]));
        let (_state, translated) = self.received_doc(&doc, &data);
        let held = self.must_open(&translated, None);
        let moved = self.frame_at(&translated, &held, &[]);
        pass(passed, "node translation is applied before automatic framing", field(&moved, "rgba") == field(&frame0, "rgba"));
        self.close_scene(&translated, &held);

        let (mut doc, data) = triangle();
        put_at(&mut doc, &[P::K("nodes"), P::I(0)], "scale", ints(&[2, 1, 1]));
        let (_state, scaled) = self.received_doc(&doc, &data);
        let held = self.must_open(&scaled, None);
        let scaled_frame = self.frame_at(&scaled, &held, &[]);
        pass(passed, "node scale changes actual geometry proportions", field(&scaled_frame, "rgba") != field(&frame0, "rgba"));
        self.close_scene(&scaled, &held);

        let (mut doc, data) = triangle();
        push_path(&mut doc, &[P::K("materials")], o(vec![("pbrMetallicRoughness", o(vec![("baseColorFactor", ints(&[0, 0, 1, 1]))]))]));
        let mesh = clone_at(&doc, &[P::K("meshes"), P::I(0)]);
        push_path(&mut doc, &[P::K("meshes")], mesh);
        set_path(&mut doc, &[P::K("meshes"), P::I(1), P::K("primitives"), P::I(0), P::K("material")], ni(1));
        set_path(
            &mut doc,
            &[P::K("nodes")],
            a(vec![
                o(vec![("mesh", ni(1)), ("translation", a(vec![ni(0), ni(0), ns("0.5")]))]),
                o(vec![("mesh", ni(0)), ("translation", a(vec![ni(0), ni(0), ns("-0.5")]))]),
            ]),
        );
        set_path(&mut doc, &[P::K("scenes"), P::I(0), P::K("nodes")], ints(&[0, 1]));
        let (_state, depth) = self.received_doc(&doc, &data);
        let held = self.must_open(&depth, None);
        let visible = pixels(&self.frame_at(&depth, &held, &[]));
        pass(passed, "near geometry occludes later-drawn far geometry by actual depth", visible[CENTER + 2] > 200 && visible[CENTER] < 10);
        self.close_scene(&depth, &held);

        for kind in [5123_i64, 5125] {
            let (mut doc, data) = triangle();
            let mut bin = data[..36].to_vec();
            if kind == 5123 {
                for value in [0u16, 1, 2] {
                    bin.extend_from_slice(&value.to_le_bytes());
                }
            } else {
                for value in [0u32, 1, 2] {
                    bin.extend_from_slice(&value.to_le_bytes());
                }
            }
            set_buffer_len(&mut doc, bin.len());
            put_at(&mut doc, &[P::K("bufferViews"), P::I(1)], "byteLength", ni((bin.len() - 36) as i64));
            put_at(&mut doc, &[P::K("accessors"), P::I(1)], "componentType", ni(kind));
            let (_state, indices) = self.received_doc(&doc, &bin);
            let held = self.must_open(&indices, None);
            let indexed = self.frame_at(&indices, &held, &[]);
            pass(passed, &format!("unsigned index component decodes exact geometry {kind}"), field(&indexed, "rgba") == field(&frame0, "rgba"));
            self.close_scene(&indices, &held);
        }

        let (mut doc, data) = triangle();
        pop_path(&mut doc, &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0)], "indices");
        let (_state, nonindexed) = self.received_doc(&doc, &data);
        let held = self.must_open(&nonindexed, None);
        let plain = self.frame_at(&nonindexed, &held, &[]);
        pass(passed, "non-indexed triangle geometry renders its real vertices", field(&plain, "rgba") == field(&frame0, "rgba"));
        self.close_scene(&nonindexed, &held);

        for mode in [5_i64, 6] {
            let (mut doc, data) = triangle();
            put_at(&mut doc, &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0)], "mode", ni(mode));
            let (_state, mode_owner) = self.received_doc(&doc, &data);
            let held = self.must_open(&mode_owner, None);
            let mode_frame = self.frame_at(&mode_owner, &held, &[]);
            pass(passed, &format!("triangle topology renders {mode}"), num(&mode_frame, "triangles") == 1);
            self.close_scene(&mode_owner, &held);
        }

        let (mut doc, _data) = triangle();
        let mut sparse = vec![1, 2, 0, 0];
        sparse.extend(pack_f32s(&[1.0, -1.0, 0.0, 0.0, 1.0, 0.0]));
        set_buffer_len(&mut doc, sparse.len());
        set_path(
            &mut doc,
            &[P::K("bufferViews")],
            a(vec![
                o(vec![("buffer", ni(0)), ("byteOffset", ni(0)), ("byteLength", ni(2))]),
                o(vec![("buffer", ni(0)), ("byteOffset", ni(4)), ("byteLength", ni(24))]),
            ]),
        );
        set_path(
            &mut doc,
            &[P::K("accessors")],
            a(vec![o(vec![
                ("componentType", ni(5126)),
                ("count", ni(3)),
                ("type", sv("VEC3")),
                (
                    "sparse",
                    o(vec![
                        ("count", ni(2)),
                        ("indices", o(vec![("bufferView", ni(0)), ("componentType", ni(5121))])),
                        ("values", o(vec![("bufferView", ni(1))])),
                    ]),
                ),
            ])]),
        );
        pop_path(&mut doc, &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0)], "indices");
        let (_state, sparse_owner) = self.received_doc(&doc, &sparse);
        let held = self.must_open(&sparse_owner, None);
        let sparse_frame = self.frame_at(&sparse_owner, &held, &[]);
        let sparse_again = self.frame_at(&sparse_owner, &held, &[]);
        pass(
            passed,
            "sparse accessors replace original zero values with actual geometry",
            num(&sparse_frame, "triangles") == 1 && red_count(&sparse_again) > 0,
        );
        self.close_scene(&sparse_owner, &held);

        let (mut doc, _data) = triangle();
        let mut rows = Vec::new();
        for row in [[-1.0, -1.0, 0.0, 1.0, 0.0, 0.0], [1.0, -1.0, 0.0, 0.0, 1.0, 0.0], [0.0, 1.0, 0.0, 0.0, 0.0, 1.0]] {
            rows.extend(pack_f32s(&row));
        }
        set_buffer_len(&mut doc, rows.len());
        set_path(
            &mut doc,
            &[P::K("bufferViews")],
            a(vec![o(vec![("buffer", ni(0)), ("byteOffset", ni(0)), ("byteLength", ni(rows.len() as i64)), ("byteStride", ni(24))])]),
        );
        set_path(
            &mut doc,
            &[P::K("accessors")],
            a(vec![
                o(vec![("bufferView", ni(0)), ("componentType", ni(5126)), ("count", ni(3)), ("type", sv("VEC3"))]),
                o(vec![("bufferView", ni(0)), ("byteOffset", ni(12)), ("componentType", ni(5126)), ("count", ni(3)), ("type", sv("VEC3"))]),
            ]),
        );
        set_path(
            &mut doc,
            &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0)],
            o(vec![("attributes", o(vec![("POSITION", ni(0)), ("COLOR_0", ni(1))]))]),
        );
        let (_state, colored_owner) = self.received_doc(&doc, &rows);
        let held = self.must_open(&colored_owner, None);
        let colored_frame = self.frame_at(&colored_owner, &held, &[]);
        let mixed = pixels(&colored_frame).chunks_exact(4).any(|px| px[1] > 100 && px[2] > 50);
        pass(passed, "interleaved vertex colors reach actual rasterized pixels", mixed);
        self.close_scene(&colored_owner, &held);

        let (mut doc, data) = triangle();
        put_at(&mut doc, &[P::K("buffers"), P::I(0)], "uri", sv("https://outside.invalid/secret"));
        let before = self.request_len();
        self.bad(passed, "external buffer references refuse without fetching", &doc, &data);
        pass(passed, "scene decoding does not make a second network request", self.request_len() == before + 1);

        let (mut doc, data) = triangle();
        put(&mut doc, "images", a(vec![o(vec![("uri", sv("https://outside.invalid/texture"))])]));
        put(&mut doc, "textures", a(vec![o(vec![("source", ni(0))])]));
        put_at(
            &mut doc,
            &[P::K("materials"), P::I(0), P::K("pbrMetallicRoughness")],
            "baseColorTexture",
            o(vec![("index", ni(0))]),
        );
        let before = self.request_len();
        let (_state, textured) = self.received_doc(&doc, &data);
        let held = self.must_open(&textured, None);
        let described = self.frame_at(&textured, &held, &[]);
        pass(
            passed,
            "geometry preview explicitly omits unfetched external textures",
            field(&described, "description").contains("textures") && self.request_len() == before + 1,
        );
        self.close_scene(&textured, &held);

        let (doc, data) = triangle();
        let mut blob = glb(&doc, &data);
        let declared = u32::try_from(blob.len() * 2).unwrap_or_else(|_| panic!("chunk length"));
        blob[12..16].copy_from_slice(&declared.to_le_bytes());
        let (_state, broken) = self.received_blob(blob.clone());
        let reply = self.ctrl.send(&Self::open_env(&broken, None));
        let same = self.artifact(&broken) == blob;
        pass_msg(
            passed,
            "GLB chunk overflow is refused after actual artifact receipt",
            reply.get("error").is_some() && same,
            &format!("error={} bytes_match={same} reply={}", reply.get("error").is_some(), reply.to_string()),
        );

        for (name, change) in [("future minimum version", "min"), ("unknown material mode", "mode")] {
            let (mut doc, data) = triangle();
            if change == "min" {
                put_at(&mut doc, &[P::K("asset")], "minVersion", sv("9.0"));
            } else {
                put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("unknown"));
            }
            self.bad(passed, &format!("{name} is refused explicitly"), &doc, &data);
        }

        let (mut doc, data) = triangle();
        put(&mut doc, "extras", o(vec![("dense", a(vec![ni(0); 100_000]))]));
        let (_state, dense) = self.received_doc(&doc, &data);
        let failure = self.ctrl.send(&Self::open_env(&dense, None));
        let message = match failure.get("error") {
            Some(Value::Str(text)) => text.clone(),
            _ => String::new(),
        };
        let same = self.artifact(&dense) == self.returned;
        pass_msg(
            passed,
            "dense metadata refuses before allocating JSON nodes",
            message.contains("Scene metadata exceeds its current memory budget") && same,
            &format!("bytes_match={same} error={message}"),
        );

        let (mut doc, data) = triangle();
        put(&mut doc, "extras", o(vec![("text", sv(&":,".repeat(50_000)))]));
        let (_state, textual) = self.received_doc(&doc, &data);
        let held = self.must_open(&textual, None);
        let textual_frame = self.frame_at(&textual, &held, &[]);
        pass(passed, "metadata punctuation inside strings does not consume structural allocation count", field(&textual_frame, "rgba") == field(&frame0, "rgba"));
        self.close_scene(&textual, &held);

        for name in [
            "required rendering extension",
            "unsupported line topology",
            "index count outside preview bound",
            "accessor escapes buffer view",
            "misaligned geometry stride",
            "invalid quaternion",
            "mixed transform authority",
            "cyclic scene hierarchy",
            "repeated root node",
            "skinned geometry",
            "transparent material",
            "invalid base color",
        ] {
            let (mut doc, data) = triangle();
            match name {
                "required rendering extension" => put(&mut doc, "extensionsRequired", a(vec![sv("unknown_required")])),
                "unsupported line topology" => put_at(&mut doc, &[P::K("meshes"), P::I(0), P::K("primitives"), P::I(0)], "mode", ni(1)),
                "index count outside preview bound" => put_at(&mut doc, &[P::K("accessors"), P::I(0)], "count", ni(300_001)),
                "accessor escapes buffer view" => put_at(&mut doc, &[P::K("accessors"), P::I(0)], "byteOffset", ni(32)),
                "misaligned geometry stride" => put_at(&mut doc, &[P::K("bufferViews"), P::I(0)], "byteStride", ni(13)),
                "invalid quaternion" => put_at(&mut doc, &[P::K("nodes"), P::I(0)], "rotation", ints(&[0, 0, 0, 0])),
                "mixed transform authority" => {
                    put_at(&mut doc, &[P::K("nodes"), P::I(0)], "matrix", ints(&[1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]));
                    put_at(&mut doc, &[P::K("nodes"), P::I(0)], "translation", ints(&[0, 0, 0]));
                }
                "cyclic scene hierarchy" => put_at(&mut doc, &[P::K("nodes"), P::I(0)], "children", ints(&[0])),
                "repeated root node" => set_path(&mut doc, &[P::K("scenes"), P::I(0), P::K("nodes")], ints(&[0, 0])),
                "skinned geometry" => put_at(&mut doc, &[P::K("nodes"), P::I(0)], "skin", ni(0)),
                "transparent material" => put_at(&mut doc, &[P::K("materials"), P::I(0)], "alphaMode", sv("BLEND")),
                "invalid base color" => set_path(&mut doc, &[P::K("materials"), P::I(0), P::K("pbrMetallicRoughness"), P::K("baseColorFactor")], ints(&[-1, 0, 0, 1])),
                _ => panic!("unknown mutation {name}"),
            }
            self.bad(passed, &format!("{name} is a scoped preview refusal"), &doc, &data);
        }

        let (doc, data) = triangle();
        let mut nan = f32::from_bits(0x7fc0_0000).to_le_bytes().to_vec();
        nan.extend_from_slice(&data[4..]);
        self.bad(passed, "non-finite binary positions cannot become a frame", &doc, &nan);
        let (doc, data) = triangle();
        let mut outside = data[..data.len() - 1].to_vec();
        outside.push(0xff);
        self.bad(passed, "triangle index outside the position array is rejected", &doc, &outside);

        let (state, owner) = self.received_triangle();
        let _ = state;
        let held = self.must_open(&owner, None);
        std::fs::write(self.artifact_path(&owner), b"changed").unwrap_or_else(|error| panic!("mutate source: {error}"));
        let mutated = self.rejected(merge(&owner, vec![("op".into(), s("scene_frame")), ("scene_ticket".into(), s(&held))]));
        pass(passed, "source mutation invalidates an existing scene viewport", mutated);
        self.close_scene(&owner, &held);

        let (_state, owner) = self.received_triangle();
        let held = self.must_open(&owner, None);
        let path = self.artifact_path(&owner);
        let saved = std::fs::read(&path).unwrap_or_else(|error| panic!("read source: {error}"));
        std::fs::remove_file(&path).unwrap_or_else(|error| panic!("unlink source: {error}"));
        let other = self.root.join("outside.glb");
        std::fs::write(&other, &saved).unwrap_or_else(|error| panic!("write outside: {error}"));
        symlink(&other, &path).unwrap_or_else(|error| panic!("symlink: {error}"));
        let substituted = self.rejected(Self::open_env(&owner, None));
        pass(passed, "source symlink substitution cannot enter a new viewport", substituted);
        self.close_scene(&owner, &held);

        let (_state, owner) = self.received_triangle();
        let held = self.must_open(&owner, None);
        let later = self.frame_at(&owner, &held, &[]);
        pass(passed, "refusals release decode admission for a later valid scene", num(&later, "triangles") == 1);
        self.close_scene(&owner, &held);
        std::fs::write(self.root.join("local.txt"), "local work").unwrap_or_else(|error| panic!("local file: {error}"));
        let read = self.ctrl.op(&obj(vec![
            ("op".into(), s("tool")),
            ("tool".into(), s("read")),
            ("root".into(), s(&self.root_text)),
            ("args".into(), obj(vec![("path".into(), s("local.txt"))])),
        ]));
        let local_ok = matches!(&read, Value::Str(text) if text == "local work");
        pass_msg(passed, "scene failures leave model-free local operations working", local_ok, &read.to_string());
    }

    fn previous_pixels(&mut self, passed: &mut Vec<String>, owner: &Value, ticket: &str) {
        let Some(path) = self.previous.clone() else { return };
        let mut prior = PriorController::spawn(&path, &self.project);
        let opened = prior.op(&Self::open_env(owner, None));
        let prior_ticket = field(&opened, "scene_ticket").to_string();
        // These names are Python's str(dict) for each camera, insertion order included.
        let cameras: [(&str, &[(&str, &str)]); 4] = [
            (
                "actual published renderer pixels match at {'size': 128, 'yaw': 0, 'pitch': 0, 'zoom': 1}",
                &[("size", "128"), ("yaw", "0"), ("pitch", "0"), ("zoom", "1")],
            ),
            (
                "actual published renderer pixels match at {'size': 193, 'yaw': 0.7, 'pitch': -0.3, 'zoom': 1.5}",
                &[("size", "193"), ("yaw", "0.7"), ("pitch", "-0.3"), ("zoom", "1.5")],
            ),
            (
                "actual published renderer pixels match at {'size': 384, 'yaw': -0.6, 'pitch': 0.8, 'zoom': 0.25}",
                &[("size", "384"), ("yaw", "-0.6"), ("pitch", "0.8"), ("zoom", "0.25")],
            ),
            (
                "actual published renderer pixels match at {'size': 128, 'yaw': 1.8, 'pitch': 1.1, 'zoom': 3}",
                &[("size", "128"), ("yaw", "1.8"), ("pitch", "1.1"), ("zoom", "3")],
            ),
        ];
        for (name, camera) in cameras {
            let baseline = prior.op(&frame_env(owner, &prior_ticket, camera));
            let current = self.frame_at(owner, ticket, camera);
            let matched = same_field(&baseline, &current, "rgba")
                && same_field(&baseline, &current, "triangles")
                && same_field(&baseline, &current, "source_sha256");
            pass(passed, name, matched);
        }
        let _closed = prior.op(&merge(
            owner,
            vec![("op".into(), s("scene_close")), ("scene_ticket".into(), s(&prior_ticket))],
        ));
    }
}

fn frame_env(owner: &Value, ticket: &str, camera: &[(&str, &str)]) -> Value {
    let mut extra = vec![
        ("op".into(), s("scene_frame")),
        ("scene_ticket".into(), s(ticket)),
        ("size".into(), n("128")),
        ("yaw".into(), n("0")),
        ("pitch".into(), n("0")),
        ("zoom".into(), n("1")),
    ];
    for (key, token) in camera {
        if let Some(slot) = extra.iter_mut().find(|(name, _)| name == key) {
            slot.1 = n(token);
        } else {
            extra.push(((*key).to_string(), n(token)));
        }
    }
    merge(owner, extra)
}

fn same_field(left: &Value, right: &Value, key: &str) -> bool {
    left.get(key).is_some() && left.get(key) == right.get(key)
}

fn parse_previous_scene_controller() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    let mut previous = None;
    while let Some(arg) = args.next() {
        if arg == "--previous-scene-controller" {
            let Some(path) = args.next() else {
                eprintln!("scene: --previous-scene-controller requires a path");
                std::process::exit(2);
            };
            if previous.is_some() {
                eprintln!("scene: --previous-scene-controller was given more than once");
                std::process::exit(2);
            }
            previous = Some(PathBuf::from(path));
        } else {
            eprintln!("scene: unrecognised argument {arg}");
            std::process::exit(2);
        }
    }
    previous
}

fn main() {
    let previous = parse_previous_scene_controller();
    self_check();
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project path is not UTF-8")).to_string();
    let tmp = create_scratch(&project, "scene");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
    let port = listener.local_addr().unwrap_or_else(|error| panic!("port: {error}")).port();
    listener.set_nonblocking(true).unwrap_or_else(|error| panic!("nonblocking: {error}"));
    let payload = Arc::new(Mutex::new(Vec::new()));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let shutdown = Arc::new(AtomicBool::new(false));
    let server = {
        let payload = Arc::clone(&payload);
        let requests = Arc::clone(&requests);
        let shutdown = Arc::clone(&shutdown);
        std::thread::spawn(move || serve(listener, payload, requests, shutdown))
    };
    let session_root = tmp.clone();
    let previous_for_session = previous.clone();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut session = Session {
            ctrl: Controller::spawn(&project_text),
            root: session_root,
            root_text: tmp.to_str().unwrap_or_else(|| panic!("temp path is not UTF-8")).to_string(),
            project,
            payload,
            requests,
            returned: Vec::new(),
            port,
            previous: previous_for_session,
        };
        let mut passed = Vec::new();
        session.run(&mut passed);
        passed
    }));
    shutdown.store(true, Ordering::Relaxed);
    let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(200));
    let _ = server.join();
    let passed = match outcome {
        Ok(passed) => passed,
        Err(payload) => std::panic::resume_unwind(payload),
    };
    let _ = std::fs::remove_dir_all(&tmp);
    let expected = if previous.is_some() { 94 } else { 90 };
    if passed.len() != expected {
        panic!("scene emitted {} checks", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    report_value("scene", &body, true);
    println!("{}", pretty(&body));
}

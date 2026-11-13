//! Shared controller, JSON, hash and compression helpers for native verification twins.
//! A twin is `verification/<name>.rs` with `mod twin;`. It talks to
//! `output/headless/RLLM --jsonl` and writes the report the harness already reads.
//! Each twin binary uses a subset of this API, so unused items are not dead code.
//!
//! Compression encode writes stored DEFLATE blocks (BTYPE 00). `zlib_compress`
//! wraps those blocks in an RFC 1950 header plus Adler-32. `gzip_store` wraps
//! the same blocks in an RFC 1952 header plus CRC-32 and ISIZE. Decode is one
//! inflater: stored blocks, fixed Huffman blocks, and dynamic Huffman blocks.
//! `inflate_deflate` is the raw member; `inflate_zlib` and `gzip_decode` add
//! their own wrappers.
#![allow(dead_code)]
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(left), Value::Bool(right)) => left == right,
            (Value::Num(left), Value::Num(right)) => left == right,
            (Value::Str(left), Value::Str(right)) => left == right,
            (Value::Arr(left), Value::Arr(right)) => left == right,
            (Value::Obj(left), Value::Obj(right)) => {
                // Both directions: a one-sided match lets a repeated key in one object
                // stand in for a different key in the other, so `{"a":1,"a":1}` would
                // compare equal to `{"a":1,"b":1}`.
                left.len() == right.len()
                    && left.iter().all(|(key, value)| {
                        right.iter().any(|(other_key, other_value)| key == other_key && value == other_value)
                    })
                    && right.iter().all(|(key, value)| {
                        left.iter().any(|(other_key, other_value)| key == other_key && value == other_value)
                    })
            }
            _ => false,
        }
    }
}

impl Value {
    pub fn parse(text: &str) -> Value {
        match parse_json(text) {
            Ok(value) => value,
            Err(error) => panic!("invalid JSON ({error}): {text}"),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        let Value::Obj(fields) = self else {
            return None;
        };
        fields.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn str_(&self) -> &str {
        let Value::Str(text) = self else {
            panic!("expected string, got {}", self.to_string());
        };
        text
    }

    pub fn bool_(&self) -> bool {
        let Value::Bool(value) = self else {
            panic!("expected bool, got {}", self.to_string());
        };
        *value
    }

    pub fn arr(&self) -> &Vec<Value> {
        let Value::Arr(items) = self else {
            panic!("expected array, got {}", self.to_string());
        };
        items
    }

    pub fn to_string(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(value) => value.to_string(),
            Value::Num(token) => token.clone(),
            Value::Str(text) => quote(text),
            Value::Arr(items) => {
                let body = items.iter().map(Value::to_string).collect::<Vec<_>>().join(",");
                format!("[{body}]")
            }
            Value::Obj(fields) => {
                let body = fields
                    .iter()
                    .map(|(key, value)| format!("{}:{}", quote(key), value.to_string()))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
        }
    }
}

// The Python legs wrote their reports with json.dumps(indent=2) and the delivered
// record splices that text, so a twin's report has to be laid out the same way.
fn write_pretty(value: &Value, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match value {
        Value::Arr(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                write_pretty(item, depth + 1, out);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        Value::Obj(fields) if !fields.is_empty() => {
            out.push_str("{\n");
            for (index, (key, item)) in fields.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&quote(key));
                out.push_str(": ");
                write_pretty(item, depth + 1, out);
                if index + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push('}');
        }
        other => out.push_str(&other.to_string()),
    }
}

pub fn pretty(value: &Value) -> String {
    let mut out = String::new();
    write_pretty(value, 0, &mut out);
    out
}

pub fn obj(fields: Vec<(String, Value)>) -> Value {
    Value::Obj(fields)
}

pub fn s(text: &str) -> Value {
    Value::Str(text.to_string())
}

pub fn n(number: &str) -> Value {
    Value::Num(number.to_string())
}

pub fn b(flag: bool) -> Value {
    Value::Bool(flag)
}

// Legs assert advertised fields with Python truthiness (`not cap['promptCapabilities']`).
// A twin that compares against Null instead rejects the empty object the product emits.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Num(text) => text.parse::<f64>().map(|number| number != 0.0).unwrap_or(true),
        Value::Str(text) => !text.is_empty(),
        Value::Arr(items) => !items.is_empty(),
        Value::Obj(fields) => !fields.is_empty(),
    }
}

pub fn b64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let assembled = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | (chunk.get(2).copied().unwrap_or(0) as u32);
        for index in 0..4 {
            if index > chunk.len() {
                out.push('=');
            } else {
                out.push(TABLE[((assembled >> (18 - 6 * index)) & 63) as usize] as char);
            }
        }
    }
    out
}

pub fn b64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut assembled = 0u32;
    let mut bits = 0;
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
            _ => panic!("invalid base64"),
        };
        assembled = (assembled << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((assembled >> bits) as u8);
        }
    }
    out
}

pub struct Controller {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<std::process::ChildStdout>>,
}

impl Controller {
    pub fn spawn(root: &str) -> Controller {
        Self::spawn_at(&Path::new(root).join("output/headless/RLLM"), root)
    }

    // The same JSONL operation protocol, at another controller binary: the published
    // controller speaks the identical envelope framing, so a twin that drives both the
    // current subject and the published one must not grow a second transport.
    pub fn spawn_at(binary: &Path, root: &str) -> Controller {
        let mut child = Command::new(binary)
            .arg("--jsonl")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", binary.display()));
        let stdin = child.stdin.take().unwrap_or_else(|| panic!("controller stdin was not piped"));
        let stdout = child.stdout.take().unwrap_or_else(|| panic!("controller stdout was not piped"));
        Controller {
            child: Some(child),
            stdin: Some(stdin),
            stdout: Some(BufReader::new(stdout)),
        }
    }

    pub fn send(&mut self, envelope: &Value) -> Value {
        let stdin = self.stdin.as_mut().unwrap_or_else(|| panic!("controller stdin is closed"));
        let line = envelope.to_string();
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .unwrap_or_else(|error| panic!("controller write: {error}"));
        let stdout = self.stdout.as_mut().unwrap_or_else(|| panic!("controller stdout is closed"));
        let mut reply = String::new();
        loop {
            match stdout.read_line(&mut reply) {
                Ok(0) => panic!("controller closed stdout"),
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("controller read: {error}"),
            }
        }
        let text = reply.trim_end_matches(['\n', '\r']);
        match parse_json(text) {
            Ok(Value::Obj(fields)) => Value::Obj(fields),
            _ => panic!("{text}"),
        }
    }

    pub fn op(&mut self, envelope: &Value) -> Value {
        let reply = self.send(envelope);
        match reply.get("value") {
            Some(value) => value.clone(),
            None => panic!("{}", reply.to_string()),
        }
    }

    pub fn failed(&mut self, envelope: &Value) -> String {
        let reply = self.send(envelope);
        match reply.get("error") {
            Some(Value::Str(text)) => text.clone(),
            _ => panic!("{}", reply.to_string()),
        }
    }

    pub fn event(&mut self, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
        let envelope = obj(vec![
            ("op".into(), s("event")),
            ("state".into(), state.cloned().unwrap_or(Value::Null)),
            ("action".into(), s(action)),
            ("inputs".into(), inputs),
            ("root".into(), s(root)),
        ]);
        self.op(&envelope)
    }

    pub fn finish(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stdin.take();
        self.stdout.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn report(group: &str, passed: &[String], scope: &str) {
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(scope)),
    ]);
    report_value(group, &body, false);
    println!("{} {} checks passed", passed.len(), group);
}

// The delivered record splices this file's text verbatim, so a twin that needs a
// different report shape (a count instead of a name list, extra keys like
// `http_requests`, a trailing newline) builds that body itself and writes it here.
pub fn report_value(group: &str, body: &Value, trailing_newline: bool) {
    std::fs::write(&report_path(group), report_text(body, trailing_newline))
        .unwrap_or_else(|error| panic!("could not write {}: {error}", report_path(group).display()));
}

fn report_path(group: &str) -> PathBuf {
    let path = Path::new("output/checks").join(group_report_file(group));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    }
    path
}

fn report_text(body: &Value, trailing_newline: bool) -> String {
    let mut text = pretty(body);
    if trailing_newline {
        text.push('\n');
    }
    text
}

// Some legs print their report instead of writing a file, and the runner counts
// and persists those printed bytes. Their twins must print the same bytes they
// write: a file alone is invisible to that path, which then reports the group as
// having absent checks even though the group exited 0.
pub fn report_value_printed(group: &str, body: &Value, trailing_newline: bool) {
    let text = report_text(body, trailing_newline);
    report_value(group, body, trailing_newline);
    print!("{text}");
}

// A Python leg gets its scratch root from tempfile.mkdtemp, which reserves the
// directory name atomically. create_dir_all would silently share a name another
// process already reserved, so reservation goes through create_dir and retries
// on the collision.
pub fn create_scratch(root: &Path, prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("system clock is before the unix epoch: {error}"))
        .as_nanos();
    let parent = root.join("output");
    std::fs::create_dir_all(&parent)
        .unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    for attempt in 0..64u32 {
        let path = parent.join(format!("{prefix}-{}-{nanos}-{attempt}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("could not create {}: {error}", path.display()),
        }
    }
    panic!("could not reserve a unique scratch directory under {}", parent.display());
}

fn group_report_file(name: &str) -> String {
    let stem = match name {
        "provider_process" => "current-results".to_string(),
        "network_drive" => "network-results".to_string(),
        "question_process" => "process-results".to_string(),
        "history" => "history-results".to_string(),
        "desktop_runtime" | "turn_background" | "permission_modes" | "services" | "media" | "jobs" | "voice"
        | "media_stream" | "media_upload" | "voice_interruption" | "voice_background" | "voice_session"
        | "capture_lifecycle" | "scene" | "apk_alignment" | "audio_route" | "document_providers"
        | "snapshot_identity" | "storage_auth" | "storage_oauth" | "credential_rotation" | "voice_settings"
        | "acp" | "oauth" | "device_authorization" => name.to_string(),
        other => other.replace('_', "-"),
    };
    format!("{stem}.json")
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch if (ch as u32) <= 0x7f => out.push(ch),
            // Python's json.dumps keeps ensure_ascii=True, so a report string's
            // non-ASCII characters go out as UTF-16 code units; a non-BMP
            // character becomes a surrogate pair.
            ch => {
                let code = ch as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let surrogate = code - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (surrogate >> 10),
                        0xdc00 + (surrogate & 0x3ff)
                    ));
                }
            }
        }
    }
    out.push('"');
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

fn parse_json(text: &str) -> Result<Value, String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("JSON exceeds 8 MiB".into());
    }
    let mut parser = Parser { bytes: text.as_bytes(), index: 0 };
    let value = parser.value(0)?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err("trailing data".into());
    }
    Ok(value)
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while self.index < self.bytes.len() && matches!(self.bytes[self.index], b' ' | b'\n' | b'\r' | b'\t') {
            self.index += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.skip();
        if self.bytes.get(self.index) == Some(&byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 128 {
            return Err("JSON nesting limit exceeded".into());
        }
        self.skip();
        match self.bytes.get(self.index).copied() {
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err("invalid JSON value".into()),
        }
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Result<Value, String> {
        if self.bytes.get(self.index..self.index + word.len()) != Some(word) {
            return Err("invalid literal".into());
        }
        self.index += word.len();
        Ok(value)
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        self.index += 1;
        let mut fields = Vec::new();
        if self.eat(b'}') {
            return Ok(Value::Obj(fields));
        }
        loop {
            let key = self.string()?;
            if !self.eat(b':') {
                return Err("missing colon".into());
            }
            let value = self.value(depth + 1)?;
            if fields.iter().any(|(name, _)| name == &key) {
                return Err("duplicate JSON field".into());
            }
            fields.push((key, value));
            if self.eat(b'}') {
                return Ok(Value::Obj(fields));
            }
            if !self.eat(b',') {
                return Err("missing comma".into());
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        self.index += 1;
        let mut items = Vec::new();
        if self.eat(b']') {
            return Ok(Value::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            if self.eat(b']') {
                return Ok(Value::Arr(items));
            }
            if !self.eat(b',') {
                return Err("missing comma".into());
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.index;
        if self.bytes[self.index] == b'-' {
            self.index += 1;
        }
        match self.bytes.get(self.index) {
            Some(b'0') => self.index += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err("invalid number".into()),
        }
        if self.bytes.get(self.index) == Some(&b'.') {
            self.index += 1;
            let fraction = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == fraction {
                return Err("missing fraction".into());
            }
        }
        if matches!(self.bytes.get(self.index), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.bytes.get(self.index), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exponent = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exponent {
                return Err("missing exponent".into());
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "invalid number")?;
        Ok(Value::Num(token.to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat(b'"') {
            return Err("expected string".into());
        }
        let mut out = String::new();
        loop {
            let byte = *self.bytes.get(self.index).ok_or("unterminated string")?;
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = *self.bytes.get(self.index).ok_or("truncated escape")?;
                    self.index += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => out.push(escaped as char),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err("invalid escape".into()),
                    }
                }
                0..=31 => return Err("unescaped control character".into()),
                _ => {
                    let width = utf8_width(byte).ok_or("invalid UTF-8")?;
                    let start = self.index - 1;
                    let end = start + width;
                    if end > self.bytes.len() {
                        return Err("truncated UTF-8".into());
                    }
                    let text = std::str::from_utf8(&self.bytes[start..end]).map_err(|_| "invalid UTF-8")?;
                    out.push_str(text);
                    self.index = end;
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let mut unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.bytes.get(self.index..self.index + 2) != Some(br"\u") {
                return Err("missing surrogate".into());
            }
            self.index += 2;
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err("invalid surrogate".into());
            }
            unit = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            return Err("invalid surrogate".into());
        }
        char::from_u32(unit).ok_or_else(|| "invalid unicode scalar".into())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.index + 4 > self.bytes.len() {
            return Err("short unicode escape".into());
        }
        let text = std::str::from_utf8(&self.bytes[self.index..self.index + 4]).map_err(|_| "invalid unicode")?;
        self.index += 4;
        u32::from_str_radix(text, 16).map_err(|_| "invalid unicode escape".into())
    }
}

fn utf8_width(byte: u8) -> Option<usize> {
    if byte < 0x80 {
        Some(1)
    } else if byte & 0xE0 == 0xC0 {
        Some(2)
    } else if byte & 0xF0 == 0xE0 {
        Some(3)
    } else if byte & 0xF8 == 0xF0 {
        Some(4)
    } else {
        None
    }
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    sha256_digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
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
    let mut padded = bytes.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((bytes.len() as u64) * 8).to_be_bytes());
    for block in padded.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, word) in words[..16].iter_mut().enumerate() {
            *word = u32::from_be_bytes(block[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
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
    let mut digest = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

pub fn sha1_digest(bytes: &[u8]) -> [u8; 20] {
    let mut padded = bytes.to_vec();
    let bit_length = (bytes.len() as u64) * 8;
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());
    let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
    for block in padded.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (index, word) in words[..16].iter_mut().enumerate() {
            *word = u32::from_be_bytes(block[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..80 {
            words[index] = (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a827999),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut digest = [0u8; 20];
    for (index, word) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

pub fn adler32(data: &[u8]) -> u32 {
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &byte in data {
        s1 = (s1 + u32::from(byte)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

// One raw DEFLATE member of stored blocks. An empty member is a single final
// empty stored block, so a wrapper can still carry a checksum.
pub fn deflate_stored(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = input;
    if rest.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
        return out;
    }
    while !rest.is_empty() {
        let take = rest.len().min(65535);
        let last = take == rest.len();
        out.push(if last { 0x01 } else { 0x00 });
        let size = take as u16;
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(!size).to_le_bytes());
        out.extend_from_slice(&rest[..take]);
        rest = &rest[take..];
    }
    out
}

pub fn zlib_compress(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend(deflate_stored(bytes));
    out.extend_from_slice(&adler32(bytes).to_be_bytes());
    out
}

pub fn gzip_store(input: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff];
    out.extend(deflate_stored(input));
    out.extend_from_slice(&crc32(input).to_le_bytes());
    out.extend_from_slice(&(input.len() as u32).to_le_bytes());
    out
}

pub fn zlib_decompress(bytes: &[u8]) -> Vec<u8> {
    inflate_zlib(bytes).unwrap_or_else(|error| panic!("invalid zlib stream: {error}"))
}

const ZLIB_LIMIT: usize = 8 * 1024 * 1024;

pub fn inflate_zlib(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > ZLIB_LIMIT {
        return Err("zlib input limit".into());
    }
    let mut input = ZIn { bytes, index: 0, bit_buf: 0, bit_count: 0 };
    let cmf = input.byte()?;
    let flg = input.byte()?;
    if cmf & 15 != 8 || cmf >> 4 > 7 {
        return Err("unsupported zlib method".into());
    }
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err("zlib header check".into());
    }
    if flg & 32 != 0 {
        return Err("zlib dictionary".into());
    }
    let window = 1usize << ((cmf >> 4) as usize + 8);
    let mut out = Vec::new();
    inflate_blocks(&mut input, &mut out, window)?;
    input.align();
    let checksum = input.big32()?;
    if checksum != adler32(&out) {
        return Err("adler32".into());
    }
    if input.index != input.bytes.len() {
        return Err("trailing zlib data".into());
    }
    Ok(out)
}

pub fn inflate_deflate(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > ZLIB_LIMIT {
        return Err("deflate input limit".into());
    }
    let mut input = ZIn { bytes, index: 0, bit_buf: 0, bit_count: 0 };
    let mut out = Vec::new();
    inflate_blocks(&mut input, &mut out, 1 << 15)?;
    input.align();
    if input.index != input.bytes.len() {
        return Err("trailing deflate data".into());
    }
    Ok(out)
}

pub fn gzip_decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > ZLIB_LIMIT {
        return Err("gzip input limit".into());
    }
    if bytes.len() < 10 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Err("gzip magic".into());
    }
    if bytes[2] != 8 {
        return Err("unsupported gzip method".into());
    }
    let flags = bytes[3];
    if flags & 0xe0 != 0 {
        return Err("reserved gzip flags".into());
    }
    let mut index = 10usize;
    if flags & 4 != 0 {
        if index + 2 > bytes.len() {
            return Err("gzip truncated".into());
        }
        let extra = u16::from_le_bytes([bytes[index], bytes[index + 1]]) as usize;
        index += 2 + extra;
    }
    if flags & 8 != 0 {
        skip_gzip_text(bytes, &mut index)?;
    }
    if flags & 16 != 0 {
        skip_gzip_text(bytes, &mut index)?;
    }
    if flags & 2 != 0 {
        index += 2;
    }
    if index > bytes.len() {
        return Err("gzip truncated".into());
    }
    let mut input = ZIn { bytes, index, bit_buf: 0, bit_count: 0 };
    let mut out = Vec::new();
    inflate_blocks(&mut input, &mut out, 1 << 15)?;
    input.align();
    if input.index + 8 > bytes.len() {
        return Err("gzip truncated".into());
    }
    let checksum = u32::from_le_bytes(bytes[input.index..input.index + 4].try_into().unwrap());
    let size = u32::from_le_bytes(bytes[input.index + 4..input.index + 8].try_into().unwrap());
    if checksum != crc32(&out) || size != (out.len() as u32) {
        return Err("gzip trailer".into());
    }
    if input.index + 8 != bytes.len() {
        return Err("trailing gzip data".into());
    }
    Ok(out)
}

fn skip_gzip_text(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    while *index < bytes.len() && bytes[*index] != 0 {
        *index += 1;
    }
    if *index >= bytes.len() {
        return Err("gzip truncated".into());
    }
    *index += 1;
    Ok(())
}

struct ZIn<'a> {
    bytes: &'a [u8],
    index: usize,
    bit_buf: u8,
    bit_count: u8,
}

impl<'a> ZIn<'a> {
    fn byte(&mut self) -> Result<u8, String> {
        let value = *self.bytes.get(self.index).ok_or("zlib truncated")?;
        self.index += 1;
        Ok(value)
    }

    fn bits(&mut self, count: u8) -> Result<u32, String> {
        let mut value = 0u32;
        for shift in 0..count {
            if self.bit_count == 0 {
                self.bit_buf = self.byte()?;
                self.bit_count = 8;
            }
            value |= u32::from(self.bit_buf & 1) << shift;
            self.bit_buf >>= 1;
            self.bit_count -= 1;
        }
        Ok(value)
    }

    fn align(&mut self) {
        self.bit_count = 0;
        self.bit_buf = 0;
    }

    fn little16(&mut self) -> Result<u16, String> {
        let low = self.byte()?;
        let high = self.byte()?;
        Ok(u16::from_le_bytes([low, high]))
    }

    fn big32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes([self.byte()?, self.byte()?, self.byte()?, self.byte()?]))
    }
}

struct ZTree {
    counts: [u16; 16],
    first: [u16; 16],
    positions: [u16; 16],
    symbols: Vec<u16>,
    max: u8,
}

impl ZTree {
    fn build(lengths: &[u8]) -> Result<Self, String> {
        if lengths.len() > 288 {
            return Err("huffman alphabet bound".into());
        }
        let mut counts = [0u16; 16];
        let mut max = 0u8;
        for &length in lengths {
            if length > 15 {
                return Err("huffman code length".into());
            }
            if length != 0 {
                counts[length as usize] += 1;
                max = max.max(length);
            }
        }
        let mut left = 1i32;
        for count in counts.iter().skip(1) {
            left = left * 2 - i32::from(*count);
            if left < 0 {
                return Err("huffman oversubscribed".into());
            }
        }
        let mut first = [0u16; 16];
        let mut positions = [0u16; 16];
        let mut code = 0u32;
        let mut position = 0u16;
        for length in 1..=15 {
            code = (code + u32::from(counts[length - 1])) << 1;
            first[length] = code as u16;
            positions[length] = position;
            position += counts[length];
        }
        let used = position as usize;
        let mut symbols = vec![0u16; used];
        let mut next = positions;
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let at = next[length as usize] as usize;
                symbols[at] = symbol as u16;
                next[length as usize] += 1;
            }
        }
        Ok(Self { counts, first, positions, symbols, max })
    }

    fn decode(&self, input: &mut ZIn<'_>) -> Result<u16, String> {
        if self.max == 0 {
            return Err("empty huffman tree".into());
        }
        let mut code = 0u32;
        for length in 1..=self.max as usize {
            code = (code << 1) | input.bits(1)?;
            let first = u32::from(self.first[length]);
            if code >= first {
                let delta = code - first;
                if delta < u32::from(self.counts[length]) {
                    return Ok(self.symbols[self.positions[length] as usize + delta as usize]);
                }
            }
        }
        Err("invalid huffman code".into())
    }
}

fn fixed_trees() -> Result<(ZTree, ZTree), String> {
    let mut lengths = [0u8; 288];
    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..].fill(8);
    Ok((ZTree::build(&lengths)?, ZTree::build(&[5u8; 32])?))
}

fn dynamic_trees(input: &mut ZIn<'_>) -> Result<(ZTree, ZTree), String> {
    let literals = input.bits(5)? as usize + 257;
    let distances = input.bits(5)? as usize + 1;
    let codes = input.bits(4)? as usize + 4;
    if literals > 286 || distances > 32 {
        return Err("dynamic code counts".into());
    }
    let order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut code_lengths = [0u8; 19];
    for &index in &order[..codes] {
        code_lengths[index] = input.bits(3)? as u8;
    }
    let alphabet = ZTree::build(&code_lengths)?;
    let wanted = literals + distances;
    let mut lengths = Vec::with_capacity(wanted);
    while lengths.len() < wanted {
        let symbol = alphabet.decode(input)?;
        let (length, count) = match symbol {
            0..=15 => (symbol as u8, 1usize),
            16 => {
                let prior = *lengths.last().ok_or("repeat without prior length")?;
                (prior, input.bits(2)? as usize + 3)
            }
            17 => (0, input.bits(3)? as usize + 3),
            18 => (0, input.bits(7)? as usize + 11),
            _ => return Err("invalid code length symbol".into()),
        };
        if count > wanted - lengths.len() {
            return Err("code length repeat overflow".into());
        }
        lengths.extend(std::iter::repeat_n(length, count));
    }
    if lengths.get(256).copied().unwrap_or(0) == 0 {
        return Err("missing end symbol".into());
    }
    Ok((ZTree::build(&lengths[..literals])?, ZTree::build(&lengths[literals..])?))
}

fn inflate_block(input: &mut ZIn<'_>, out: &mut Vec<u8>, window: usize, lit: &ZTree, dist: &ZTree) -> Result<(), String> {
    const LENGTH_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227,
        258,
    ];
    const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DIST_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097,
        6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
    loop {
        match lit.decode(input)? {
            symbol @ 0..=255 => {
                if out.len() >= ZLIB_LIMIT {
                    return Err("zlib output limit".into());
                }
                out.push(symbol as u8);
            }
            256 => return Ok(()),
            symbol @ 257..=285 => {
                let index = (symbol - 257) as usize;
                let length = LENGTH_BASE[index] + input.bits(LENGTH_EXTRA[index])? as usize;
                let distance_symbol = dist.decode(input)? as usize;
                if distance_symbol >= 30 {
                    return Err("reserved distance".into());
                }
                let distance = DIST_BASE[distance_symbol] + input.bits(DIST_EXTRA[distance_symbol])? as usize;
                if distance == 0 || distance > out.len() || distance > window || out.len() + length > ZLIB_LIMIT {
                    return Err("bad distance".into());
                }
                for _ in 0..length {
                    let byte = out[out.len() - distance];
                    out.push(byte);
                }
            }
            _ => return Err("reserved length".into()),
        }
    }
}

fn inflate_blocks(input: &mut ZIn<'_>, out: &mut Vec<u8>, window: usize) -> Result<(), String> {
    loop {
        let final_block = input.bits(1)? != 0;
        match input.bits(2)? {
            0 => {
                input.align();
                let length = input.little16()?;
                let inverse = input.little16()?;
                if length != !inverse {
                    return Err("stored length".into());
                }
                for _ in 0..length {
                    if out.len() >= ZLIB_LIMIT {
                        return Err("zlib output limit".into());
                    }
                    out.push(input.byte()?);
                }
            }
            1 => {
                let (lit, dist) = fixed_trees()?;
                inflate_block(input, out, window, &lit, &dist)?;
            }
            2 => {
                let (lit, dist) = dynamic_trees(input)?;
                inflate_block(input, out, window, &lit, &dist)?;
            }
            _ => return Err("reserved block".into()),
        }
        if final_block {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn escapes_round_trip_including_u0000() {
        let text = "quote \" newline\n tab\t backslash\\ \0\u{001f} \u{1D11E}";
        assert_eq!(Value::parse(&s(text).to_string()).str_(), text);
        assert_eq!(Value::parse("\"\\u0000\"").str_(), "\0");
        assert_eq!(Value::parse("\"\\ud834\\udd1e\"").str_(), "\u{1D11E}");
    }

    #[test]
    fn objects_keep_insertion_order_and_compare_by_key() {
        let value = obj(vec![("b".into(), n("1")), ("a".into(), s("x"))]);
        assert_eq!(value.to_string(), "{\"b\":1,\"a\":\"x\"}");
        assert_eq!(value, Value::parse("{\"a\":\"x\",\"b\":1}"));
        // A repeated key must not stand in for a different key on the other side.
        let repeated = obj(vec![("a".into(), n("1")), ("a".into(), n("1"))]);
        let other_key = obj(vec![("a".into(), n("1")), ("b".into(), n("1"))]);
        assert_eq!(repeated, obj(vec![("a".into(), n("1")), ("a".into(), n("1"))]));
        assert_ne!(repeated, other_key);
        assert_ne!(other_key, repeated);
    }

    #[test]
    fn truthy_matches_the_falsy_values_the_legs_test() {
        for value in [
            Value::Null,
            b(false),
            n("0"),
            n("0.0"),
            s(""),
            Value::Arr(vec![]),
            obj(vec![]),
        ] {
            assert!(!truthy(&value), "expected falsy: {}", value.to_string());
        }
        for value in [
            b(true),
            n("2"),
            n("-1"),
            s("x"),
            Value::Arr(vec![Value::Null]),
            obj(vec![("prompt".into(), Value::Null)]),
        ] {
            assert!(truthy(&value), "expected truthy: {}", value.to_string());
        }
    }

    #[test]
    fn base64_round_trip() {
        let bytes: Vec<u8> = (0..50).collect();
        assert_eq!(b64_decode(&b64_encode(&bytes)), bytes);
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(&[0xff]), "/w==");
    }

    #[test]
    fn history_report_file_matches_the_runner() {
        assert_eq!(group_report_file("history"), "history-results.json");
        assert_eq!(group_report_file("state_sealing"), "state-sealing.json");
    }

    #[test]
    fn printed_report_bytes_are_the_json_a_printing_leg_emits() {
        // The legs that report on stdout emit json.dumps(body, indent=2) plus
        // print()'s newline, and the runner counts and persists those printed
        // bytes, so the indent style and trailing newline are part of the
        // contract, not formatting.
        let body = obj(vec![
            ("passed".into(), Value::Arr(vec![s("a")])),
            ("checks".into(), Value::Arr(vec![s("a")])),
        ]);
        let printed = report_text(&body, true);
        assert_eq!(printed, "{\n  \"passed\": [\n    \"a\"\n  ],\n  \"checks\": [\n    \"a\"\n  ]\n}\n");
        assert_eq!(report_text(&body, false), format!("{printed}").trim_end_matches('\n'));
    }
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len() / 2)
            .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn hashes_and_checksums_match_known_vectors() {
        assert_eq!(sha256_bytes(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_bytes(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let sha1: String = sha1_digest(b"").iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(sha1, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        let sha1_abc: String = sha1_digest(b"abc").iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(sha1_abc, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e60398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn stored_blocks_round_trip_through_each_wrapper() {
        let long = vec![b'x'; 70_000];
        let samples: [&[u8]; 5] = [b"", b"abc", b"Wikipedia", &[0, 7, 0, 0], long.as_slice()];
        for sample in samples {
            assert_eq!(inflate_deflate(&deflate_stored(sample)).unwrap(), sample);
            assert_eq!(zlib_decompress(&zlib_compress(sample)), sample);
            assert_eq!(gzip_decode(&gzip_store(sample)).unwrap(), sample);
        }
    }

    #[test]
    fn inflater_reads_fixed_and_dynamic_streams() {
        // zlib.compress / gzip.compress bytes: empty and "abc" are fixed Huffman,
        // the long run is dynamic. The stored encoder is a different block type.
        assert_eq!(zlib_decompress(&hex("789c030000000001")), b"");
        assert_eq!(zlib_decompress(&hex("789c4b4c4a0600024d0127")), b"abc");
        assert_eq!(
            zlib_decompress(&hex(
                "789cb3c928c9cdb1b349ca4fa9b4738ff20c88770b72f475b5d1070bd8e8836501e1a30c11"
            )),
            b"<html><body>GZIP_FRAME</body></html>"
        );
        let xs = zlib_decompress(&hex(
            "789cedc13101000000c2a0da8b6f094fa00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000080b7012c283401",
        ));
        assert_eq!(xs, vec![b'x'; 70_000]);
        assert_eq!(gzip_decode(&hex("1f8b080000000000020303000000000000000000")).unwrap(), b"");
        assert_eq!(gzip_decode(&hex("1f8b08000000000002034b4c4a0600c241243503000000")).unwrap(), b"abc");
        assert_eq!(inflate_deflate(&hex("4b4c4a0600")).unwrap(), b"abc");
    }
}

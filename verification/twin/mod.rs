//! Shared controller and JSON helpers for native verification twins.
//! A twin is `verification/<name>.rs` with `mod twin;`. It talks to
//! `output/headless/RLLM --jsonl` and writes the report the harness already reads.
//! Each twin binary uses a subset of this API, so unused items are not dead code.
#![allow(dead_code)]
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
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
                left.len() == right.len()
                    && left.iter().all(|(key, value)| {
                        right.iter().any(|(other_key, other_value)| key == other_key && value == other_value)
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

pub fn obj(fields: Vec<(String, Value)>) -> Value {
    Value::Obj(fields)
}

pub fn s(text: &str) -> Value {
    Value::Str(text.to_string())
}

pub fn n(number: &str) -> Value {
    Value::Num(number.to_string())
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
        let binary = Path::new(root).join("output/headless/RLLM");
        let mut child = Command::new(&binary)
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
    let path = Path::new("output/checks").join(group_report_file(group));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    }
    std::fs::write(&path, body.to_string()).unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
    println!("{} {} checks passed", passed.len(), group);
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
            ch => out.push(ch),
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
}

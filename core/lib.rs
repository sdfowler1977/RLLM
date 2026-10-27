mod application;
mod archive;
mod compression;
mod layout;
mod execution;
mod gateway;
mod network;
mod provider;
mod generation;
mod resources;
mod stream;
mod document;
use std::{collections::BTreeMap, ffi::CString, os::raw::c_char};

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}
use Json::*;
type Result<T> = std::result::Result<T, String>;
impl Json {
    fn get(&self, k: &str) -> Result<&Json> {
        if let Obj(o) = self {
            o.get(k).ok_or_else(|| format!("Missing {k}"))
        } else {
            Err("Expected object".into())
        }
    }
    fn text(&self) -> Result<&str> {
        if let Str(s) = self {
            Ok(s)
        } else {
            Err("Expected text".into())
        }
    }
    fn array(&self) -> Result<&Vec<Json>> {
        if let Arr(v) = self {
            Ok(v)
        } else {
            Err("Expected array".into())
        }
    }
    fn string(&self) -> String {
        match self {
            Null => "null".into(),
            Bool(b) => b.to_string(),
            Num(s) => s.clone(),
            Str(s) => quote(s),
            Arr(v) => format!(
                "[{}]",
                v.iter().map(Json::string).collect::<Vec<_>>().join(",")
            ),
            Obj(o) => format!(
                "{{{}}}",
                o.iter()
                    .map(|(k, v)| format!("{}:{}", quote(k), v.string()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}
fn obj(fields: Vec<(&str, Json)>) -> Json {
    Obj(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn s(v: &str) -> Json {
    Str(v.into())
}
fn quote(v: &str) -> String {
    let mut out = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}
impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.ws();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn hex(&mut self) -> Result<u32> {
        if self.i + 4 > self.b.len() {
            return Err("Short unicode escape".into());
        }
        let v = std::str::from_utf8(&self.b[self.i..self.i + 4]).map_err(|_| "Invalid unicode")?;
        self.i += 4;
        u32::from_str_radix(v, 16).map_err(|_| "Invalid unicode escape".into())
    }
    fn str(&mut self) -> Result<String> {
        if !self.take(b'"') {
            return Err("Expected quoted text".into());
        }
        let mut out = Vec::new();
        loop {
            let c = *self.b.get(self.i).ok_or("Unterminated text")?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| "Invalid UTF-8".into()),
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or("Missing escape")?;
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(10),
                        b'r' => out.push(13),
                        b't' => out.push(9),
                        b'u' => {
                            let mut n = self.hex()?;
                            if (0xd800..=0xdbff).contains(&n) {
                                if self.b.get(self.i..self.i + 2) != Some(b"\\u") {
                                    return Err("Missing surrogate".into());
                                }
                                self.i += 2;
                                let low = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err("Invalid surrogate".into());
                                }
                                n = 0x10000 + ((n - 0xd800) << 10) + (low - 0xdc00);
                            }
                            let c = char::from_u32(n).ok_or("Invalid unicode scalar")?;
                            let mut buf = [0; 4];
                            out.extend(c.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err("Invalid escape".into()),
                    }
                }
                0..=31 => return Err("Unescaped control character".into()),
                _ => out.push(c),
            }
        }
    }
    fn value(&mut self, depth: usize) -> Result<Json> {
        if depth > 64 {
            return Err("JSON nesting limit exceeded".into());
        }
        self.ws();
        match self.b.get(self.i).copied() {
            Some(b'"') => Ok(Str(self.str()?)),
            Some(b'{') => {
                self.i += 1;
                let mut m = BTreeMap::new();
                if self.take(b'}') {
                    return Ok(Obj(m));
                }
                loop {
                    let k = self.str()?;
                    if !self.take(b':') {
                        return Err("Missing colon".into());
                    }
                    let v = self.value(depth + 1)?;
                    if m.insert(k, v).is_some() {
                        return Err("Duplicate JSON field".into());
                    }
                    if self.take(b'}') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err("Missing comma".into());
                    }
                }
                Ok(Obj(m))
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                if self.take(b']') {
                    return Ok(Arr(v));
                }
                loop {
                    v.push(self.value(depth + 1)?);
                    if self.take(b']') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err("Missing comma".into());
                    }
                }
                Ok(Arr(v))
            }
            Some(b't') => self.literal(b"true", Bool(true)),
            Some(b'f') => self.literal(b"false", Bool(false)),
            Some(b'n') => self.literal(b"null", Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                if self.b[self.i] == b'-' {
                    self.i += 1;
                }
                match self.b.get(self.i) {
                    Some(b'0') => self.i += 1,
                    Some(b'1'..=b'9') => {
                        while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                            self.i += 1
                        }
                    }
                    _ => return Err("Invalid number".into()),
                }
                if self.b.get(self.i) == Some(&b'.') {
                    self.i += 1;
                    let n = self.i;
                    while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                        self.i += 1
                    }
                    if self.i == n {
                        return Err("Missing fraction".into());
                    }
                }
                if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
                    self.i += 1;
                    if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                        self.i += 1
                    }
                    let n = self.i;
                    while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                        self.i += 1
                    }
                    if self.i == n {
                        return Err("Missing exponent".into());
                    }
                }
                Ok(Num(std::str::from_utf8(&self.b[start..self.i])
                    .unwrap()
                    .into()))
            }
            _ => Err("Invalid JSON value".into()),
        }
    }
    fn literal(&mut self, w: &[u8], v: Json) -> Result<Json> {
        if self.b.get(self.i..self.i + w.len()) != Some(w) {
            return Err("Invalid literal".into());
        }
        self.i += w.len();
        Ok(v)
    }
}
fn parse(text: &str) -> Result<Json> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("JSON exceeds 8 MiB".into());
    }
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err("Trailing JSON data".into());
    }
    Ok(v)
}
fn field<'a>(j: &'a Json, k: &str) -> Result<&'a str> {
    j.get(k)?.text()
}
fn request(v: &Json) -> Result<Json> {
    let protocol = field(v, "protocol")?;
    let model = field(v, "model")?;
    if model.trim().is_empty() {
        return Err("Enter a model name".into());
    }
    let source = v.get("messages")?.array()?;
    if source.is_empty() || source.len() > 2000 {
        return Err("Conversation must contain 1–2000 messages".into());
    }
    let mut messages = Vec::new();
    let mut system = Vec::new();
    let mut contents = Vec::new();
    for m in source {
        let role = field(m, "role")?;
        let content = field(m, "content")?;
        if !["user", "assistant", "system"].contains(&role) {
            return Err("Unsupported message role".into());
        }
        if content.len() > 1024 * 1024 {
            return Err("Message exceeds 1 MiB".into());
        }
        if role == "system" {
            system.push(content.to_string());
        }
        if role != "system" || protocol == "chat" || protocol == "local" {
            messages.push(obj(vec![("role", s(role)), ("content", s(content))]));
        }
        if role != "system" {
            contents.push(obj(vec![
                (
                    "role",
                    s(if role == "assistant" { "model" } else { "user" }),
                ),
                ("parts", Arr(vec![obj(vec![("text", s(content))])])),
            ]));
        }
    }
    let body_result: Result<Json> = match protocol {
        "chat" | "local" => Ok(obj(vec![
            ("model", s(model)),
            ("messages", Arr(messages)),
            ("stream", Bool(false)),
        ])),
        "messages" => {
            let mut fields = vec![
                ("model", s(model)),
                ("messages", Arr(messages)),
                ("max_tokens", Num("4096".into())),
                ("stream", Bool(false)),
            ];
            if !system.is_empty() {
                fields.push(("system", s(&system.join("\n\n"))));
            }
            Ok(obj(fields))
        }
        "content" => {
            let mut fields = vec![("contents", Arr(contents))];
            if !system.is_empty() {
                fields.push((
                    "systemInstruction",
                    obj(vec![(
                        "parts",
                        Arr(vec![obj(vec![("text", s(&system.join("\n\n")))])]),
                    )]),
                ));
            }
            Ok(obj(fields))
        }
        _ => Err("Unknown API format".into()),
    };
    let mut body = body_result?;
    let options = v.get("generation").unwrap_or(&Null);
    generation::apply(&mut body, options, protocol, source)?;
    Ok(body)
}
fn response(v: &Json) -> Result<Json> {
    let protocol = field(v, "protocol")?;
    let body = parse(field(v, "body")?)?;
    if let Ok(e) = body.get("error") {
        return Err(format!("Server error: {}", e.string()));
    }
    let text = match protocol {
        "chat" => {
            let choice = body
                .get("choices")?
                .array()?
                .first()
                .ok_or("No response choices")?;
            let msg = choice.get("message")?;
            msg.get("content")?.text()?.to_string()
        }
        "local" => field(body.get("message")?, "content")?.to_string(),
        "messages" => {
            let mut parts = Vec::new();
            for p in body.get("content")?.array()? {
                if field(p, "type") == Ok("text") {
                    parts.push(field(p, "text")?.to_string());
                }
            }
            parts.join("\n")
        }
        "content" => stream::content_text(&body)?,

        _ => return Err("Unknown API format".into()),
    };
    if text.trim().is_empty() {
        return Err("Server returned no text response".into());
    }
    Ok(s(&text))
}
fn operation(input: &str) -> Result<Json> {
    let v = parse(input)?;
    match field(&v, "op")? {
        "stream_open" | "stream_feed" | "stream_finish" | "stream_close" => stream::operation(&v),
        "phone_revision_check" => document::operation(&v),
        "layout" => layout::plan(&v),
        "script" => execution::run(&v),
        "project_archive" => archive::build(&v),
        "artifact_snapshot" => application::artifact_snapshot(&v),
        "project_chunks" => application::project_chunks(&v),
        "project_payload" => application::project_payload(&v),
        "project_record" => application::project_record(&v),
        "project_readback" => application::project_readback(&v),
        "project_close" => application::project_close(&v),
        "archive_preview" => archive::preview(&v),
        "archive_import" => archive::import(&v),
        "network_binary" => {
            let raw = network::unbase64(v.get("data")?.text()?)?;
            let (_, body) = network::decoded_bytes(&raw)?;
            if body.len() > 5 * 1024 * 1024 {
                return Err("Project ZIP exceeds 5 MiB".into());
            }
            Ok(obj(vec![("archive", s(&network::b64(&body)))]))
        }
        "gateway_size" => gateway::size(&v),
        "gateway_incoming" => gateway::incoming(&v),
        "gateway_outgoing" => gateway::outgoing(&v),
        "gateway_failure" => gateway::failure(&v),
        "cancel_tool" => {
            application::cancel();
            Ok(Null)
        }
        "event" => application::event(&v),
        "tool" => application::tool(&v),
        "request" => request(&v),
        "response" => response(&v),
        "validate" => {
            let value = v.get("value")?;
            Ok(value.clone())
        }
        _ => Err("Unknown operation".into()),
    }
}
fn envelope(input: &str) -> String {
    match operation(input) {
        Ok(value) => obj(vec![("value", value)]).string(),
        Err(e) => obj(vec![("error", s(&e))]).string(),
    }
}
#[no_mangle]
pub unsafe extern "C" fn client_process(data: *const u8, len: usize) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        if data.is_null() || len > 8 * 1024 * 1024 {
            return "{\"error\":\"Invalid input size\"}".to_string();
        }
        match std::str::from_utf8(std::slice::from_raw_parts(data, len)) {
            Ok(v) => envelope(v),
            Err(_) => "{\"error\":\"Invalid UTF-8\"}".into(),
        }
    })
    .unwrap_or_else(|_| "{\"error\":\"Core operation failed\"}".into());
    CString::new(result).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn client_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn json_roundtrip() {
        for v in [
            "quote \" newline\n tab\t backslash\\",
            "中文 café \u{1D11E}",
            "\0\u{001f}",
        ] {
            assert_eq!(parse(&quote(v)).unwrap(), s(v));
        }
        assert_eq!(parse("\"\\ud834\\udd1e\"").unwrap(), s("\u{1D11E}"));
    }
    #[test]
    fn bad_json() {
        for v in [
            "{\"x\":1,\"x\":2}",
            "[1,]",
            "01",
            "1e",
            "\"\\ud800\"",
            "true x",
            "\"\n\"",
        ] {
            assert!(parse(v).is_err(), "{v}");
        }
    }
    #[test]
    fn formats() {
        for protocol in ["chat", "local", "messages", "content"] {
            let j = obj(vec![
                ("protocol", s(protocol)),
                ("model", s("custom/model")),
                (
                    "messages",
                    Arr(vec![
                        obj(vec![("role", s("system")), ("content", s("Instructions"))]),
                        obj(vec![("role", s("user")), ("content", s("one"))]),
                        obj(vec![("role", s("assistant")), ("content", s("two"))]),
                        obj(vec![("role", s("user")), ("content", s("three"))]),
                    ]),
                ),
            ]);
            let r = request(&j).unwrap();
            assert!(r.string().contains("three"));
            assert!(r.string().contains("Instructions"));
        }
    }
    #[test]
    fn outputs() {
        for (protocol, body) in [
            (
                "chat",
                r#"{"choices":[{"message":{"content":"a\"b\n中文"}}]}"#,
            ),
            ("local", r#"{"message":{"content":"a\"b\n中文"}}"#),
            (
                "messages",
                r#"{"content":[{"type":"text","text":"a\"b\n中文"}]}"#,
            ),
            (
                "content",
                r#"{"candidates":[{"content":{"parts":[{"text":"a\"b\n中文"}]}}]}"#,
            ),
        ] {
            assert_eq!(
                response(&obj(vec![("protocol", s(protocol)), ("body", s(body))])).unwrap(),
                s("a\"b\n中文")
            );
        }
    }
    #[test]
    fn scoped_failure() {
        assert!(envelope(r#"{"op":"request"}"#).contains("error"));
        assert_eq!(
            envelope(r#"{"op":"validate","value":{"ready":true}}"#),
            r#"{"value":{"ready":true}}"#
        );
    }
    #[test]
    fn depth_limit() {
        assert!(parse(&("[".repeat(66) + "0" + &"]".repeat(66))).is_err());
    }
    #[test]
    fn binary_network_zip_reaches_import_preview() {
        let root = std::env::current_dir()
            .unwrap()
            .join("toolchain")
            .join(format!(
                "client-network-zip-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("binary.dat"), [0, 255, 1]).unwrap();
        let built = operation(
            &obj(vec![
                ("op", s("project_archive")),
                ("root", s(root.to_str().unwrap())),
            ])
            .string(),
        )
        .unwrap();
        let zip = network::unbase64(built.get("archive").unwrap().text().unwrap()).unwrap();
        let mut response =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", zip.len()).into_bytes();
        response.extend_from_slice(&zip);
        let extracted = operation(
            &obj(vec![
                ("op", s("network_binary")),
                ("data", s(&network::b64(&response))),
            ])
            .string(),
        )
        .unwrap();
        let preview = operation(
            &obj(vec![
                ("op", s("archive_preview")),
                ("archive", extracted.get("archive").unwrap().clone()),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(preview.get("files").unwrap(), &Num("1".into()));
        assert!(preview.string().contains("binary.dat"));
        assert_eq!(
            network::unbase64(extracted.get("archive").unwrap().text().unwrap()).unwrap(),
            zip
        );
        // Preserve this owned verification fixture for review.
    }
}

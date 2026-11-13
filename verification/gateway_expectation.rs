//! Native verification twin for current gateway header admission.
//! Drives the real loopback listener: interim 100-continue, final decisions
//! before a body upload, both A2A versions, and the selected file's digest.
mod twin;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use twin::{obj, s, Value};

const SCOPE: &str = "Owner execution: real loopback HTTP, no body sent before continue/final decisions, both A2A versions and actual file/output digests. No installed Android or independent acceptance.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&project);
    let payload = "Actual selected file for expectation handling.\n";
    let fixture = scratch.path().join("文.txt");
    std::fs::write(&fixture, payload).unwrap_or_else(|error| panic!("could not write the fixture file: {error}"));
    let token = format!("gateway-expectation-{}", "z".repeat(40));
    let key = scratch.path().join("token");
    std::fs::write(&key, &token).unwrap_or_else(|error| panic!("could not write the token file: {error}"));
    restrict(&key);

    let mut passed = Vec::new();
    let mut gateway = Gateway::start(&project, scratch.path(), &key);
    let banner = gateway.banner();
    check(&mut passed, "listener starts", !banner.is_empty());
    let port = parse_port(&banner).unwrap_or_else(|| panic!("listener starts"));
    let digest = twin::sha256_bytes(std::fs::read(&fixture).unwrap_or_else(|error| panic!("could not read the fixture file: {error}")).as_slice());

    for (version, expect, early_body) in [
        ("1.0", "100-continue", false),
        ("0.3", "100-CoNtInUe", false),
        ("1.0", "100-continue, 100-CONTINUE", false),
        ("1.0", "100-continue", true),
    ] {
        let current = version == "1.0";
        let mid = format!("observed-{}", passed.len());
        let body = request_body(current, &mid);
        let mut connection = Connection::open(port);
        let mut framed = header("POST", "/a2a", body.len(), Some(expect), &token, &format!("A2A-Version: {version}\r\n"));
        if early_body {
            framed.extend_from_slice(&body);
        }
        connection.send(&framed);
        let (code, raw, _) = connection.response();
        check(
            &mut passed,
            &format!("{version} receives interim before body wait {mid}"),
            code == 100 && raw == b"HTTP/1.1 100 Continue\r\n\r\n",
        );
        if !early_body {
            connection.send(&body);
        }
        let (code, _, reply) = connection.response();
        check(
            &mut passed,
            &format!("{version} final response returns actual selected file {mid}"),
            final_file(code, &reply, current, &digest),
        );
    }

    for (name, expect, auth, length, extra, wanted) in [
        ("wrong bearer with expectation", Some("100-continue"), "wrong", 2usize, "", 401u16),
        ("wrong bearer without expectation", None, "wrong", 2, "", 401),
        ("unknown expectation", Some("fancy-upload"), token.as_str(), 2, "", 417),
        ("mixed expectation", Some("100-continue, fancy-upload"), token.as_str(), 2, "", 417),
        ("authentication before expectation disclosure", Some("fancy-upload"), "wrong", 2, "", 401),
        ("duplicate expectation", Some("100-continue"), token.as_str(), 2, "Expect: 100-continue\r\n", 400),
        ("oversized framed body", Some("100-continue"), token.as_str(), 1_048_577, "", 400),
        ("empty body has no interim", Some("100-continue"), token.as_str(), 0, "", 200),
    ] {
        let mut connection = Connection::open(port);
        connection.send(&header("GET", "/health", length, expect, auth, extra));
        let (code, raw, _) = connection.response();
        check(
            &mut passed,
            &format!("{name} returns final before any body upload"),
            code == wanted && raw.windows(b"Connection: close\r\n".len()).any(|window| window == b"Connection: close\r\n"),
        );
        check(&mut passed, &format!("{name} closes only this connection"), connection.eof());
    }

    let mut connection = Connection::open(port);
    connection.send(&header("GET", "/health", 0, None, &token, ""));
    let (code, _, _) = connection.response();
    check(&mut passed, "ordinary authenticated local health remains usable", code == 200);

    gateway.stop();
    let report = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("count".into(), twin::n(&passed.len().to_string())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("gateway_expectation", &report, true);
    println!("{} gateway_expectation checks passed", passed.len());
}

fn check(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn final_file(code: u16, reply: &[u8], current: bool, digest: &str) -> bool {
    if code != 200 {
        return false;
    }
    let Ok(text) = std::str::from_utf8(reply) else {
        return false;
    };
    let value = Value::parse(text);
    let Some(output) = artifact_data(&value, current) else {
        return false;
    };
    let Some(body) = output.get("output").and_then(Value::as_str) else {
        return false;
    };
    let Some(returned) = output.get("returnedSha256").and_then(Value::as_str) else {
        return false;
    };
    body.contains(digest) && returned == twin::sha256_bytes(body.as_bytes())
}

fn artifact_data<'a>(value: &'a Value, current: bool) -> Option<&'a Value> {
    let result = value.get("result")?;
    let task = if current { result.get("task")? } else { result };
    let artifact = nth(task.get("artifacts")?, 0)?;
    let part = nth(artifact.get("parts")?, 0)?;
    part.get("data")
}

fn nth(value: &Value, index: usize) -> Option<&Value> {
    match value {
        Value::Arr(items) => items.get(index),
        _ => None,
    }
}

trait AsStr {
    fn as_str(&self) -> Option<&str>;
}

impl AsStr for Value {
    fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }
}

fn request_body(current: bool, mid: &str) -> Vec<u8> {
    let mut part = vec![(
        "data".to_string(),
        obj(vec![
            ("tool".into(), s("inspect")),
            (
                "args".into(),
                obj(vec![("path".into(), s("文.txt")), ("content".into(), s("complete_digest"))]),
            ),
        ]),
    )];
    let mut message = vec![
        ("messageId".into(), s(mid)),
        ("role".into(), s(if current { "ROLE_USER" } else { "user" })),
    ];
    if !current {
        part.push(("kind".into(), s("data")));
    }
    message.push(("parts".into(), Value::Arr(vec![obj(part)])));
    if !current {
        message.push(("kind".into(), s("message")));
    }
    let request = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), s(mid)),
        ("method".into(), s(if current { "SendMessage" } else { "message/send" })),
        ("params".into(), obj(vec![("message".into(), obj(message))])),
    ]);
    // Same bytes as json.dumps(..., ensure_ascii=False): spaced separators, raw UTF-8.
    python_dumps(&request).into_bytes()
}

fn python_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Num(token) => token.clone(),
        Value::Str(text) => python_quote(text),
        Value::Arr(items) => {
            let body = items.iter().map(python_dumps).collect::<Vec<_>>().join(", ");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let body = fields
                .iter()
                .map(|(key, item)| format!("{}: {}", python_quote(key), python_dumps(item)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{body}}}")
        }
    }
}

fn python_quote(text: &str) -> String {
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

fn header(method: &str, path: &str, length: usize, expect: Option<&str>, auth: &str, extra: &str) -> Vec<u8> {
    let mut text = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {auth}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n"
    );
    if let Some(expect) = expect {
        text.push_str(&format!("Expect: {expect}\r\n"));
    }
    text.push_str(extra);
    text.push_str("\r\n");
    text.into_bytes()
}

fn parse_port(line: &str) -> Option<u16> {
    let tail = line.trim_end().rsplit(':').next()?;
    tail.split('/').next()?.trim().parse().ok()
}

struct Connection {
    stream: TcpStream,
}

impl Connection {
    fn open(port: u16) -> Connection {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|error| panic!("could not connect: {error}"));
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap_or_else(|error| panic!("could not set the read timeout: {error}"));
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap_or_else(|error| panic!("could not set the write timeout: {error}"));
        let _ = stream.set_nodelay(true);
        Connection { stream }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.stream
            .write_all(bytes)
            .unwrap_or_else(|error| panic!("could not send the request: {error}"));
        self.stream.flush().unwrap_or_else(|error| panic!("could not flush the request: {error}"));
    }

    fn response(&mut self) -> (u16, Vec<u8>, Vec<u8>) {
        let mut raw = Vec::new();
        while !raw.ends_with(b"\r\n\r\n") {
            let mut byte = [0u8; 1];
            let read = self.stream.read(&mut byte).unwrap_or_else(|error| panic!("could not read the response: {error}"));
            if read == 0 {
                panic!("Peer closed before a complete response");
            }
            raw.push(byte[0]);
            if raw.len() >= 16384 {
                panic!("the response header grew past 16384 bytes");
            }
        }
        let code = parse_status(&raw);
        let size = header_field(&raw, b"content-length").unwrap_or(0);
        let mut body = vec![0u8; size];
        if size > 0 {
            self.stream
                .read_exact(&mut body)
                .unwrap_or_else(|error| panic!("could not read the response body: {error}"));
        }
        (code, raw, body)
    }

    fn eof(&mut self) -> bool {
        let mut byte = [0u8; 1];
        match self.stream.read(&mut byte) {
            Ok(0) => true,
            // The leg's f.read(1)==b'' only accepts a zero-byte read; any read error is a failure.
            Ok(_) | Err(_) => false,
        }
    }
}

fn parse_status(raw: &[u8]) -> u16 {
    let line = raw.split(|byte| *byte == b'\r').next().unwrap_or_default();
    let mut parts = line.split(|byte| *byte == b' ');
    let _version = parts.next();
    let code = parts.next().unwrap_or_default();
    String::from_utf8(code.to_vec())
        .unwrap_or_default()
        .parse::<u16>()
        .unwrap_or_else(|_| panic!("could not parse the status code"))
}

fn header_field(raw: &[u8], name: &[u8]) -> Option<usize> {
    for line in raw.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break;
        }
        let Some(split) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        let key = &line[..split];
        if key.eq_ignore_ascii_case(name) {
            let mut value = &line[split + 1..];
            while value.first().is_some_and(|byte| byte.is_ascii_whitespace()) {
                value = &value[1..];
            }
            let text = String::from_utf8_lossy(value);
            // A present field that will not parse must not read as "absent": the leg's
            // `int(fields.get(b'content-length', b'0'))` defaults only a missing field.
            return Some(text.trim().parse::<usize>().unwrap_or_else(|error| {
                panic!("{} is present but unparsable ({error}): {text}", String::from_utf8_lossy(name))
            }));
        }
    }
    None
}

struct Gateway {
    child: Child,
    banner_line: Option<String>,
}

impl Gateway {
    fn start(project: &Path, root: &Path, key: &Path) -> Gateway {
        let binary = project.join("output/headless/RLLM");
        let mut child = Command::new(&binary)
            .arg("--a2a")
            .arg("--root")
            .arg(root)
            .arg("--token-file")
            .arg(key)
            .arg("--port")
            .arg("0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", binary.display()));
        let stderr = child.stderr.take().unwrap_or_else(|| panic!("gateway stderr was not piped"));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let _ = tx.send(line);
            let mut sink = Vec::new();
            let _ = reader.read_to_end(&mut sink);
        });
        let banner_line = rx.recv_timeout(Duration::from_secs(10)).ok();
        Gateway { child, banner_line }
    }

    fn banner(&mut self) -> String {
        self.banner_line.clone().unwrap_or_default()
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop();
    }
}

fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("could not read the token permissions: {error}"))
            .permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(path, permissions).unwrap_or_else(|error| panic!("could not set the token permissions: {error}"));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(project: &Path) -> Scratch {
        Scratch(twin::create_scratch(project, "gateway-expectation"))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

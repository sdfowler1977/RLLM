mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner real HTTP peer and reviewed shared native-core events; no model/services configured; exact backup and conditional-write/readback bytes. No installed-device or independent acceptance.";
const REPORT: &str = "output/checks/storage-etags.json";

const VALID_TAGS: &[&str] = &[
    "\"caf\u{e9}-\u{7248}\"",
    "\"e\u{301}\"",
    "\"\u{a0}opaque\u{a0}\"",
    "\"ascii\"",
];

const INVALID_TAGS: &[(&str, &str)] = &[
    ("\u{a0}\"caf\u{e9}\"\u{a0}", ""),
    ("W/\"caf\u{e9}\"", ""),
    ("\"bad space\"", ""),
    ("\"bad\u{85}\"", ""),
    ("\"caf\u{e9}\"", "ETag: \"other\"\r\n"),
];

struct PeerState {
    files: Vec<(Vec<u8>, Vec<u8>)>,
    requests: Vec<Request>,
    tag: Vec<u8>,
    extra: Vec<u8>,
}

#[derive(Clone)]
struct Request {
    method: Vec<u8>,
    path: Vec<u8>,
    fields: Vec<(Vec<u8>, Vec<u8>)>,
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path(), &peer);
    controller.finish();
    peer.stop();

    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("storage_etags", &body, true);
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains(SCOPE) {
        panic!("{REPORT} missing scope");
    }
}

fn run(controller: &mut Controller, root: &str, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let mut session = state(&resumed).clone();
    let port = peer.port();
    put(
        &mut session,
        "drive",
        obj(vec![
            ("url".into(), s(&format!("http://127.0.0.1:{port}/share/"))),
            ("plain".into(), twin::b(true)),
        ]),
    );

    for value in VALID_TAGS {
        peer.reset(value.as_bytes(), b"");
        let mut update = review_update(controller, &session, root, &mut passed);
        while !effect_absent(&update) {
            update = exchange(controller, &update, root);
        }
        let label = python_repr(value);
        keep(
            &mut passed,
            &format!("valid opaque tag completes exact update {label}"),
            peer.body(b"/share/file.txt").as_deref() == Some(&b"new text"[..])
                && revision_absent(state(&update)),
        );
        keep(
            &mut passed,
            &format!("prior bytes survive in one separate revision {label}"),
            peer.file_count() == 2 && peer.bodies().iter().any(|body| body == b"old text"),
        );
        let puts = peer.puts_for(b"/share/file.txt");
        keep(
            &mut passed,
            &format!("conditional bytes are unchanged {label}"),
            puts.len() == 1 && puts[0].field(b"If-Match").as_deref() == Some(value.as_bytes()),
        );
        keep(
            &mut passed,
            &format!("backup readback precedes target write {label}"),
            peer.methods()
                == [
                    b"GET".to_vec(),
                    b"PUT".to_vec(),
                    b"GET".to_vec(),
                    b"PUT".to_vec(),
                    b"GET".to_vec(),
                ],
        );
    }

    let mut refused = None;
    for (value, duplicate) in INVALID_TAGS {
        peer.reset(value.as_bytes(), duplicate.as_bytes());
        let update = review_update(controller, &session, root, &mut passed);
        let outcome = exchange(controller, &update, root);
        let label = python_tuple2(value, duplicate);
        let notice = state(&outcome).get("notice_error").is_some_and(twin::truthy);
        keep(
            &mut passed,
            &format!("unsafe validator refuses before any write {label}"),
            effect_absent(&outcome) && notice && peer.requests().len() == 1 && peer.only_original(),
        );
        refused = Some(outcome);
    }
    let refused = refused.unwrap_or_else(|| panic!("unsafe validator refuses before any write"));
    let edited = controller.event(Some(state(&refused)), "drive_edit", obj(vec![]), root);
    keep(
        &mut passed,
        "ordinary local settings remain available after scoped refusal",
        text_is(state(&edited), "dialog", "drive"),
    );
    passed
}

fn review_update(controller: &mut Controller, session: &Value, root: &str, passed: &mut Vec<String>) -> Value {
    let opened = controller.event(Some(session), "drive_update_editor", obj(vec![]), root);
    let reviewed = controller.event(
        Some(state(&opened)),
        "drive_update_review",
        obj(vec![
            ("path".into(), s("file.txt")),
            ("content".into(), s("new text")),
        ]),
        root,
    );
    keep(
        passed,
        "manual update remains explicitly reviewed",
        effect_absent(&reviewed) && text_is(state(&reviewed), "dialog", "approval"),
    );
    controller.event(Some(state(&reviewed)), "approve", obj(vec![]), root)
}

fn exchange(controller: &mut Controller, update: &Value, root: &str) -> Value {
    let network = match update.get("effect") {
        Some(Value::Null) | None => panic!("expected network effect"),
        Some(value) => value,
    };
    let kind = network.get("kind").map(Value::str_).unwrap_or_else(|| panic!("network effect has no kind"));
    if kind != "network" {
        panic!("expected network effect");
    }
    let host = network.get("host").map(Value::str_).unwrap_or_else(|| panic!("network effect has no host"));
    let port = match network.get("port") {
        Some(Value::Num(text)) => text.parse::<u16>().unwrap_or_else(|_| panic!("invalid port")),
        _ => panic!("network effect has no port"),
    };
    let wire = network.get("wire").map(Value::str_).unwrap_or_else(|| panic!("network effect has no wire"));
    let mut stream = connect(host, port);
    stream.set_nodelay(true).unwrap_or_else(|error| panic!("nodelay: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("write timeout: {error}"));
    stream.write_all(wire.as_bytes()).unwrap_or_else(|error| panic!("write: {error}"));
    let _ = stream.shutdown(Shutdown::Write);
    let raw = read_to_end(&mut stream);
    let mut inputs = obj(vec![]);
    for key in ["purpose", "drive_ticket", "model_ticket", "child_ticket", "child_owner"] {
        if let Some(value) = network.get(key) {
            put(&mut inputs, key, value.clone());
        }
    }
    put(&mut inputs, "data", s(&b64_encode(&raw)));
    controller.event(Some(state(update)), "network_result", inputs, root)
}

fn connect(host: &str, port: u16) -> TcpStream {
    let address = format!("{host}:{port}");
    let connected = if let Ok(socket) = address.parse::<std::net::SocketAddr>() {
        TcpStream::connect_timeout(&socket, Duration::from_secs(5))
    } else {
        TcpStream::connect((host, port))
    };
    connected.unwrap_or_else(|error| panic!("connect: {error}"))
}

fn read_to_end(stream: &mut TcpStream) -> Vec<u8> {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => raw.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("read: {error}"),
        }
    }
    raw
}

fn effect_absent(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn revision_absent(value: &Value) -> bool {
    matches!(value.get("drive_revision"), None | Some(Value::Null))
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
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

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

// Python's repr() is the check label. Printable non-ASCII stays as itself;
// the non-printable bytes in this leg's tags use the same \x/\u form.
fn python_repr(text: &str) -> String {
    let mut out = String::from("'");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if python_nonprintable(ch) => {
                let code = ch as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('\'');
    out
}

fn python_nonprintable(ch: char) -> bool {
    let code = ch as u32;
    code != 0x20 && (code < 0x20 || code == 0x7f || ch == '\u{85}' || ch == '\u{a0}')
}

fn python_tuple2(left: &str, right: &str) -> String {
    format!("({}, {})", python_repr(left), python_repr(right))
}

struct Peer {
    port: u16,
    state: Arc<Mutex<PeerState>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("local addr: {error}")).port();
        let state = Arc::new(Mutex::new(PeerState {
            files: Vec::new(),
            requests: Vec::new(),
            tag: Vec::new(),
            extra: Vec::new(),
        }));
        let thread_state = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let state = Arc::clone(&thread_state);
                std::thread::spawn(move || handle_peer(stream, state));
            }
        });
        Self { port, state }
    }

    fn stop(self) {
        drop(self);
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn reset(&self, tag: &[u8], extra: &[u8]) {
        let mut state = self.state.lock().unwrap_or_else(|error| panic!("peer lock: {error}"));
        state.tag = tag.to_vec();
        state.extra = extra.to_vec();
        state.files = vec![(b"/share/file.txt".to_vec(), b"old text".to_vec())];
        state.requests.clear();
    }

    fn requests(&self) -> Vec<Request> {
        self.state.lock().unwrap_or_else(|error| panic!("peer lock: {error}")).requests.clone()
    }

    fn bodies(&self) -> Vec<Vec<u8>> {
        self.state
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .files
            .iter()
            .map(|(_, body)| body.clone())
            .collect()
    }

    fn file_count(&self) -> usize {
        self.state.lock().unwrap_or_else(|error| panic!("peer lock: {error}")).files.len()
    }

    fn body(&self, path: &[u8]) -> Option<Vec<u8>> {
        self.state
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .files
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, body)| body.clone())
    }

    fn only_original(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| panic!("peer lock: {error}"));
        state.files.len() == 1
            && state.files[0].0 == b"/share/file.txt"
            && state.files[0].1 == b"old text"
    }

    fn puts_for(&self, path: &[u8]) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|request| request.method == b"PUT" && request.path == path)
            .collect()
    }

    fn methods(&self) -> Vec<Vec<u8>> {
        self.requests().into_iter().map(|request| request.method).collect()
    }
}

impl Request {
    fn field(&self, name: &[u8]) -> Option<Vec<u8>> {
        self.fields.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
    }
}

fn handle_peer(mut stream: TcpStream, shared: Arc<Mutex<PeerState>>) {
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("peer timeout: {error}"));
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = read_count(&mut stream, &mut buffer);
        if count == 0 {
            return;
        }
        raw.extend_from_slice(&buffer[..count]);
        if raw.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let separator = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("peer request has no header terminator"));
    let header = raw[..separator].to_vec();
    let mut body = raw[separator + 4..].to_vec();
    let lines: Vec<Vec<u8>> = header.split(|byte| *byte == b'\n').map(strip_cr).map(|line| line.to_vec()).collect();
    let request_line = lines.first().map(Vec::as_slice).unwrap_or_else(|| panic!("peer request has no request line"));
    let mut pieces = request_line.split(|byte| *byte == b' ');
    let method = pieces.next().unwrap_or_default().to_vec();
    let path = pieces.next().unwrap_or_default().to_vec();
    if pieces.next().is_none() {
        panic!("peer request line is incomplete");
    }
    let mut fields = Vec::new();
    for line in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }
        let position = line
            .windows(2)
            .position(|window| window == b": ")
            .unwrap_or_else(|| panic!("peer header has no separator"));
        fields.push((line[..position].to_vec(), line[position + 2..].to_vec()));
    }
    let length = fields
        .iter()
        .find(|(name, _)| name == b"Content-Length")
        .and_then(|(_, value)| std::str::from_utf8(value).ok())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    while body.len() < length {
        let count = read_count(&mut stream, &mut buffer);
        if count == 0 {
            panic!("peer request ended before Content-Length");
        }
        body.extend_from_slice(&buffer[..count]);
    }
    body.truncate(length);

    let (status, headers, output) = {
        let mut state = shared.lock().unwrap_or_else(|error| panic!("peer lock: {error}"));
        state.requests.push(Request {
            method: method.clone(),
            path: path.clone(),
            fields: fields.clone(),
        });
        if method == b"GET" {
            let output = state
                .files
                .iter()
                .find(|(name, _)| name == &path)
                .map(|(_, stored)| stored.clone())
                .unwrap_or_else(|| panic!("peer has no {}", String::from_utf8_lossy(&path)));
            let headers = if path == b"/share/file.txt" {
                // Match the Python leg: `ETag: {tag}\r\n` + extra, then the blank line below.
                let mut headers = b"ETag: ".to_vec();
                headers.extend_from_slice(&state.tag);
                headers.extend_from_slice(b"\r\n");
                headers.extend_from_slice(&state.extra);
                headers
            } else {
                Vec::new()
            };
            (200u16, headers, output)
        } else if method == b"PUT" {
            if path == b"/share/file.txt" {
                let condition = fields.iter().find(|(name, _)| name == b"If-Match").map(|(_, value)| value.as_slice());
                if condition != Some(state.tag.as_slice()) {
                    panic!("peer If-Match mismatch");
                }
                if let Some(slot) = state.files.iter_mut().find(|(name, _)| name == &path) {
                    slot.1 = body;
                } else {
                    state.files.push((path, body));
                }
                (204, Vec::new(), Vec::new())
            } else {
                let condition = fields
                    .iter()
                    .find(|(name, _)| name == b"If-None-Match")
                    .map(|(_, value)| value.as_slice());
                if condition != Some(b"*".as_slice()) {
                    panic!("peer If-None-Match mismatch");
                }
                if state.files.iter().any(|(name, _)| name == &path) {
                    panic!("peer backup path already exists");
                }
                state.files.push((path, body));
                (201, Vec::new(), Vec::new())
            }
        } else {
            panic!("unexpected peer method");
        }
    };
    let mut reply = format!(
        "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n",
        output.len()
    )
    .into_bytes();
    reply.extend_from_slice(&headers);
    reply.extend_from_slice(b"\r\n");
    reply.extend_from_slice(&output);
    stream.write_all(&reply).unwrap_or_else(|error| panic!("peer write: {error}"));
    let _ = stream.shutdown(Shutdown::Both);
}

fn strip_cr(line: &[u8]) -> &[u8] {
    if line.ends_with(b"\r") { &line[..line.len() - 1] } else { line }
}

fn read_count(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    loop {
        match stream.read(buffer) {
            Ok(count) => return count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "storage-etags"))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

//! Chunk-extension integrity checks through actual peers and public storage events.
mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use twin::{b64_encode, obj, s, Controller, Value};

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = twin::create_scratch(&root, "chunk-fields");
    let scratch_text = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let mut peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &mut peer, &scratch_text);
    controller.finish();
    peer.finish();
    let _ = std::fs::remove_dir_all(&scratch);
    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
    ]);
    twin::report_value_printed("chunk_fields", &body, true);
}

fn run(controller: &mut Controller, peer: &mut Peer, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let body = multistatus();
    let valid = content_length_response(body);
    let chunked: &[u8] = b"HTTP/1.1 207 Multi-Status\r\nTransfer-Encoding: chunked\r\n\r\n";
    peer.set_reply(valid.clone());

    let mut state = state_of(&controller.event(None, "resume", obj(vec![]), root));
    state = state_of(&controller.event(Some(&state), "drive_edit", obj(vec![]), root));
    let url = format!("http://127.0.0.1:{}/share/", peer.port());
    state = state_of(&controller.event(
        Some(&state),
        "drive_save",
        obj(vec![("url".into(), s(&url)), ("plain".into(), twin::b(true))]),
        root,
    ));
    state = listing(controller, &state, root);
    let name = "ordinary WebDAV works without a model or extra services";
    keep(&mut passed, name, arr_len(&state, "remote", name) == 1);

    let extensions: [&[u8]; 14] = [
        b";",
        b";=x",
        b";name=",
        b";name=\"open",
        b";name=\"closed\"tail",
        b";name=two words",
        b";na(me=x",
        b";name=x\0",
        b";name=\"x\rY\"",
        b";name=\"x\nY\"",
        b";name=\"x\x7fY\"",
        b";name=x;",
        b" ",
        b";name ",
    ];
    let mut malformed = Vec::new();
    for extension in extensions {
        for last in [false, true] {
            malformed.push(chunk(chunked, body, extension, last));
        }
    }
    let expected = "Invalid chunk extension";

    for (index, raw) in malformed.iter().enumerate() {
        let retain = format!("refusal {index} retains destination and credentials");
        let old = state.get("drive").cloned().unwrap_or_else(|| panic!("{retain}: missing drive"));
        peer.set_reply(raw.clone());
        state = listing(controller, &state, root);
        let refused = format!("malformed response {index} refused by actual storage event");
        keep(
            &mut passed,
            &refused,
            !bool_field(&state, "busy", &refused) && str_field(&state, "notice", &refused) == expected,
        );
        keep(&mut passed, &retain, drive_is(&state, &old, &retain));
        peer.set_reply(valid.clone());
        state = listing(controller, &state, root);
        let recovered = format!("ordinary storage recovers after refusal {index}");
        keep(
            &mut passed,
            &recovered,
            arr_len(&state, "remote", &recovered) == 1 && !notice_has_error(&state, &recovered),
        );
    }

    for extension in [
        b";flag".as_slice(),
        b";key=value;other=token",
        b" \t; \tkey \t= \t\"quoted; value\";flag",
        b";key=\"\"",
    ] {
        for last in [false, true] {
            peer.set_reply(chunk(chunked, body, extension, last));
            state = listing(controller, &state, root);
            let name = format!("valid extension remains available {}", python_pair(extension, last));
            keep(
                &mut passed,
                &name,
                arr_len(&state, "remote", &name) == 1 && !notice_has_error(&state, &name),
            );
        }
    }

    for (index, raw) in malformed.iter().enumerate() {
        let ident = frame_open(controller, "chunks");
        let error = frame_error(controller, "chunks", &ident, raw);
        let refused = format!("stream refusal {index}");
        keep(&mut passed, &refused, error == expected);
        let closed = frame_close(controller, "chunks", &ident);
        let retired = format!("failed stream retires its own session {index}");
        keep(&mut passed, &retired, closed == Value::Bool(false));
    }

    let ident = frame_open(controller, "chunks");
    let raw = chunk(chunked, body, b" \t; flag; key=\"value\"", false);
    let mut final_reply = Value::Null;
    for byte in &raw {
        final_reply = frame_feed(controller, "chunks", &ident, &[*byte]);
    }
    let name = "fragmented valid stream recovers after malformed peers";
    let finished = frame_finish(controller, "chunks", &ident);
    keep(&mut passed, name, ready(&final_reply, name) && status_code(&finished, name) == 207);

    for status in [b"200 OK".as_slice(), b"207 Multi-Status".as_slice()] {
        let owner = format!("split-chunk-{}", std::str::from_utf8(&status[..3]).unwrap_or_else(|_| panic!("status label")));
        let mut head = b"HTTP/1.1 ".to_vec();
        head.extend_from_slice(status);
        head.extend_from_slice(b"\r\nTransfer-Encoding: chunked\r\n\r\n");
        let expected_status: i64 = std::str::from_utf8(&status[..3])
            .unwrap_or_else(|_| panic!("status code"))
            .parse()
            .unwrap_or_else(|_| panic!("status code"));
        for last in [false, true] {
            let mut line = if last { b"0;flag=".to_vec() } else { b"1;flag=".to_vec() };
            line.resize(8192, b'x');
            let mut prefix = head.clone();
            if last {
                prefix.extend_from_slice(b"1\r\nA\r\n");
            }
            let label = python_pair(status, last);
            let ident = frame_open(controller, &owner);
            let mut split = prefix.clone();
            split.extend_from_slice(&line);
            split.push(b'\r');
            let fed = frame_feed(controller, &owner, &ident, &split);
            let accepted = format!("exact-size chunk line accepts a split delimiter {label}");
            keep(&mut passed, &accepted, !ready(&fed, &accepted));
            let rest: &[u8] = if last { b"\n\r\n" } else { b"\nA\r\n0\r\n\r\n" };
            let end = frame_feed(controller, &owner, &ident, rest);
            let finished = frame_finish(controller, &owner, &ident);
            let completed = format!("split delimiter completes the owned response {label}");
            keep(&mut passed, &completed, ready(&end, &completed) && status_code(&finished, &completed) == expected_status);
            let ident = frame_open(controller, &owner);
            let mut extra = prefix;
            extra.extend_from_slice(&line);
            extra.extend_from_slice(b"x\r");
            let error = frame_error(controller, &owner, &ident, &extra);
            let bounded = format!("one extra chunk-header byte fails before LF {label}");
            keep(&mut passed, &bounded, error == "Chunk header exceeds bound");
        }
    }

    let requests = peer.requests();
    let name = "every exchange used actual configured storage peer";
    keep(
        &mut passed,
        name,
        requests.len() == 65 && requests.iter().all(|request| request.starts_with(b"PROPFIND /share/")),
    );
    passed
}

fn multistatus() -> &'static [u8] {
    br#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/share/ok.txt</d:href><d:propstat><d:prop><d:displayname>ok.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>"#
}

fn content_length_response(body: &[u8]) -> Vec<u8> {
    let mut out = b"HTTP/1.1 207 Multi-Status\r\nContent-Length: ".to_vec();
    out.extend_from_slice(body.len().to_string().as_bytes());
    out.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    out.extend_from_slice(body);
    out
}

// Python `format(len(body), 'x')`: lowercase hex size, then the extension on the
// first chunk line or on the terminating 0-chunk when `last` is set.
fn chunk(head: &[u8], body: &[u8], extension: &[u8], last: bool) -> Vec<u8> {
    let mut out = head.to_vec();
    out.extend_from_slice(format!("{:x}", body.len()).as_bytes());
    if !last {
        out.extend_from_slice(extension);
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out.extend_from_slice(b"\r\n0");
    if last {
        out.extend_from_slice(extension);
    }
    out.extend_from_slice(b"\r\n\r\n");
    out
}

fn listing(controller: &mut Controller, state: &Value, root: &str) -> Value {
    let result = controller.event(Some(state), "drive_list", obj(vec![]), root);
    let effect = result
        .get("effect")
        .cloned()
        .unwrap_or_else(|| panic!("drive_list returned no effect: {}", result.to_string()));
    if effect.get("kind").and_then(as_str) != Some("network") {
        panic!("drive_list did not request network storage: {}", effect.to_string());
    }
    let host = effect
        .get("host")
        .and_then(as_str)
        .unwrap_or_else(|| panic!("drive_list effect has no host: {}", effect.to_string()))
        .to_string();
    let port = match effect.get("port") {
        Some(Value::Num(text)) => text.parse::<u16>().unwrap_or_else(|_| panic!("drive_list port {text}")),
        _ => panic!("drive_list effect has no port: {}", effect.to_string()),
    };
    let wire = effect
        .get("wire")
        .and_then(as_str)
        .unwrap_or_else(|| panic!("drive_list effect has no wire: {}", effect.to_string()))
        .to_string();
    let prior = state_of(&result);
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("storage peer: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(wire.as_bytes()).unwrap_or_else(|error| panic!("storage peer write: {error}"));
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap_or_else(|error| panic!("storage peer read: {error}"));
    let mut values = Vec::new();
    for key in ["purpose", "drive_ticket", "child_ticket", "child_owner", "model_ticket"] {
        if let Some(value) = effect.get(key) {
            values.push((key.to_string(), value.clone()));
        }
    }
    values.push(("data".into(), s(&b64_encode(&bytes))));
    state_of(&controller.event(Some(&prior), "network_result", obj(values), root))
}

fn frame_open(controller: &mut Controller, owner: &str) -> Value {
    controller.op(&obj(vec![
        ("op".into(), s("network_frame_open")),
        ("owner".into(), s(owner)),
        ("limit".into(), twin::n("65536")),
    ]))
}

fn frame_feed(controller: &mut Controller, owner: &str, id: &Value, data: &[u8]) -> Value {
    controller.op(&feed_envelope(owner, id, data))
}

fn frame_error(controller: &mut Controller, owner: &str, id: &Value, data: &[u8]) -> String {
    let reply = controller.send(&feed_envelope(owner, id, data));
    match reply.get("error") {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    }
}

fn feed_envelope(owner: &str, id: &Value, data: &[u8]) -> Value {
    obj(vec![
        ("op".into(), s("network_frame_feed")),
        ("owner".into(), s(owner)),
        ("id".into(), id.clone()),
        ("data".into(), s(&b64_encode(data))),
    ])
}

fn frame_close(controller: &mut Controller, owner: &str, id: &Value) -> Value {
    controller.op(&obj(vec![
        ("op".into(), s("network_frame_close")),
        ("owner".into(), s(owner)),
        ("id".into(), id.clone()),
    ]))
}

fn frame_finish(controller: &mut Controller, owner: &str, id: &Value) -> Value {
    controller.op(&obj(vec![
        ("op".into(), s("network_frame_finish")),
        ("owner".into(), s(owner)),
        ("id".into(), id.clone()),
        ("eof".into(), twin::b(false)),
    ]))
}

fn state_of(value: &Value) -> Value {
    value.get("state").cloned().unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

fn str_field<'a>(value: &'a Value, key: &str, name: &str) -> &'a str {
    match value.get(key) {
        Some(Value::Str(text)) => text,
        Some(other) => panic!("{name}: {key} is {}", other.to_string()),
        None => panic!("{name}: missing {key}"),
    }
}

fn bool_field(value: &Value, key: &str, name: &str) -> bool {
    match value.get(key) {
        Some(Value::Bool(flag)) => *flag,
        Some(other) => panic!("{name}: {key} is {}", other.to_string()),
        None => panic!("{name}: missing {key}"),
    }
}

fn arr_len(value: &Value, key: &str, name: &str) -> usize {
    match value.get(key) {
        Some(Value::Arr(items)) => items.len(),
        Some(other) => panic!("{name}: {key} is {}", other.to_string()),
        None => panic!("{name}: missing {key}"),
    }
}

fn drive_is(state: &Value, old: &Value, name: &str) -> bool {
    match state.get("drive") {
        Some(drive) => drive == old,
        None => panic!("{name}: missing drive"),
    }
}

fn notice_has_error(state: &Value, name: &str) -> bool {
    str_field(state, "notice", name).to_lowercase().contains("error")
}

fn ready(value: &Value, name: &str) -> bool {
    bool_field(value, "ready", name)
}

fn status_code(value: &Value, name: &str) -> i64 {
    match value.get("status") {
        Some(Value::Num(text)) => text.parse().unwrap_or_else(|_| panic!("{name}: status {text}")),
        Some(other) => panic!("{name}: status is {}", other.to_string()),
        None => panic!("{name}: missing status"),
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

// Check names concatenate Python's repr((bytes, bool)): b'...' plus True/False.
fn python_bytes(bytes: &[u8]) -> String {
    let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') { '"' } else { '\'' };
    let mut out = String::from("b");
    out.push(quote);
    for &byte in bytes {
        if byte == b'\\' || byte == quote as u8 {
            out.push('\\');
            out.push(byte as char);
        } else if byte == b'\t' {
            out.push_str("\\t");
        } else if byte == b'\n' {
            out.push_str("\\n");
        } else if byte == b'\r' {
            out.push_str("\\r");
        } else if (0x20..0x7f).contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("\\x{byte:02x}"));
        }
    }
    out.push(quote);
    out
}

fn python_pair(bytes: &[u8], last: bool) -> String {
    format!("({}, {})", python_bytes(bytes), if last { "True" } else { "False" })
}

struct Peer {
    port: u16,
    reply: Arc<Mutex<Vec<u8>>>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")).port();
        let reply = Arc::new(Mutex::new(Vec::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reply_thread = Arc::clone(&reply);
        let requests_thread = Arc::clone(&requests);
        let stop_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || serve(listener, reply_thread, requests_thread, stop_thread));
        Self { port, reply, requests, stop, thread: Some(thread) }
    }

    fn set_reply(&self, bytes: Vec<u8>) {
        *self.reply.lock().unwrap() = bytes;
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn requests(&self) -> Vec<Vec<u8>> {
        self.requests.lock().unwrap().clone()
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        TcpStream::connect(("127.0.0.1", self.port)).unwrap_or_else(|error| panic!("peer shutdown: {error}"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// The leg's handler reads through the header terminator, records that byte
// string, sends the current reply, and closes. The caller reads until EOF.
fn serve(listener: TcpListener, reply: Arc<Mutex<Vec<u8>>>, requests: Arc<Mutex<Vec<Vec<u8>>>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("storage peer accept: {error}"),
        };
        if stop.load(Ordering::SeqCst) {
            break;
        }
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let Some(request) = read_headers(&mut stream) else {
            continue;
        };
        requests.lock().unwrap().push(request);
        let body = reply.lock().unwrap().clone();
        stream.write_all(&body).unwrap_or_else(|error| panic!("storage peer write: {error}"));
        let _ = stream.flush();
    }
}

fn read_headers(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if data.windows(4).any(|window| window == b"\r\n\r\n") {
            return Some(data);
        }
        match stream.read(&mut buf) {
            Ok(0) => {
                if data.is_empty() {
                    return None;
                }
                panic!("storage peer closed before the request headers finished");
            }
            Ok(count) => {
                data.extend_from_slice(&buf[..count]);
                if data.len() >= 65536 {
                    panic!("storage peer request exceeded 65536 bytes");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("storage peer read: {error}"),
        }
    }
}

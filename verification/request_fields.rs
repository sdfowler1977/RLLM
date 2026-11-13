mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner shared native settings/ordinary model and reviewed service events; actual local HTTP model, storage and service peers; no installed Android/live user endpoint/independent acceptance.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    // A failed run must not leave the Python leg's report looking like a twin pass.
    let _ = std::fs::remove_file(root.join("output/checks/request-fields.json"));
    let scratch_dir = twin::create_scratch(&root, "request-fields");
    let scratch = scratch_dir.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let _cleanup = Cleanup(scratch_dir);
    let mut peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch, &peer);
    controller.finish();
    peer.stop();

    let body = twin::obj(vec![
        ("count".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("request_fields", &body, true);
    println!("{} request_fields checks passed", passed.len());
}

fn run(controller: &mut Controller, scratch: &str, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    let origin = peer.origin();

    for fmt in ["chat", "local", "messages", "content"] {
        let resumed = controller.event(None, "resume", obj(vec![]), scratch);
        let created = controller.event(Some(state(&resumed)), "profile_new", obj(vec![]), scratch);
        let endpoint = format!("{origin}/{fmt}");
        let saved = controller.event(
            Some(state(&created)),
            "profile_save",
            obj(vec![
                ("name".into(), s("Header model")),
                ("protocol".into(), s(fmt)),
                ("endpoint".into(), s(&endpoint)),
                ("model".into(), s("controlled")),
                ("key".into(), s("secret-model")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
            ]),
            scratch,
        );
        let mut saved_state = state(&saved).clone();
        {
            let profiles = items_mut(field_mut(&mut saved_state, "profiles"));
            if profiles.is_empty() {
                panic!("{fmt} profile_save returned no profile");
            }
            put(&mut profiles[0], "auth_header", s("X_API_Key"));
            put(&mut profiles[0], "auth_prefix", s(""));
        }
        let sent = controller.event(
            Some(&saved_state),
            "send",
            obj(vec![("draft".into(), s("Use this configured model."))]),
            scratch,
        );
        let done = transfer(controller, &sent, scratch, &mut passed, fmt);
        let headers = peer.last_headers();
        keep_detail(
            &mut passed,
            &format!("{fmt} real peer receives the exact custom field"),
            header_is(&headers, "X_API_Key", "secret-model"),
            &format!("{headers:?}"),
        );
        keep(
            &mut passed,
            &format!("{fmt} ordinary response completes"),
            not_indexed(state(&done), "busy"),
        );
    }

    let share = format!("{origin}/share/");
    let resumed = controller.event(None, "resume", obj(vec![]), scratch);
    let edited = controller.event(Some(state(&resumed)), "drive_edit", obj(vec![]), scratch);
    let saved = controller.event(
        Some(state(&edited)),
        "drive_save",
        obj(vec![
            ("url".into(), s(&share)),
            ("plain".into(), twin::b(true)),
            ("key".into(), s("secret-storage")),
            ("auth_header".into(), s("X.Storage_Key")),
            ("auth_prefix".into(), s("")),
        ]),
        scratch,
    );
    keep(
        &mut passed,
        "native storage settings accept token field syntax",
        falsy(state(&saved).get("notice_error")),
    );
    let storage_state = state(&saved).clone();
    let listed = controller.event(Some(&storage_state), "drive_list", obj(vec![]), scratch);
    transfer(controller, &listed, scratch, &mut passed, "storage");
    let headers = peer.last_headers();
    keep_detail(
        &mut passed,
        "actual storage peer receives exact credential field",
        header_is(&headers, "X.Storage_Key", "secret-storage"),
        &format!("{headers:?}"),
    );

    for name in [
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Accept-Encoding",
        "TE",
        "Trailer",
        "Expect",
        "Upgrade",
        "Proxy-Connection",
        "bad name",
        "Bad:Key",
    ] {
        let edited = controller.event(Some(&storage_state), "drive_edit", obj(vec![]), scratch);
        let refused = controller.event(
            Some(state(&edited)),
            "drive_save",
            obj(vec![
                ("auth_header".into(), s(name)),
                ("url".into(), s(&share)),
                ("plain".into(), twin::b(true)),
                ("key".into(), s("secret-storage")),
                ("auth_prefix".into(), s("")),
            ]),
            scratch,
        );
        keep(
            &mut passed,
            &format!("{name} refuses before sending or replacing saved storage settings"),
            effect_is_null(&refused)
                && !falsy(state(&refused).get("notice_error"))
                && same_field(state(&refused), &storage_state, "drive"),
        );
    }

    let listed = controller.event(Some(&storage_state), "drive_list", obj(vec![]), scratch);
    let done = transfer(controller, &listed, scratch, &mut passed, "after-refusal");
    keep(
        &mut passed,
        "refusal leaves ordinary storage available",
        not_indexed(state(&done), "busy"),
    );

    let config = service_config(&origin);
    let resumed = controller.event(None, "resume", obj(vec![]), scratch);
    let connections = controller.event(Some(state(&resumed)), "service_connections", obj(vec![]), scratch);
    let config_json = config.to_string();
    let saved = controller.event(
        Some(state(&connections)),
        "service_save",
        obj(vec![("service_config".into(), s(&config_json))]),
        scratch,
    );
    let notice = notice_label(state(&saved));
    keep(
        &mut passed,
        &format!("service settings accept the same token syntax: {notice}"),
        falsy(state(&saved).get("notice_error")),
    );
    let service_state = state(&saved).clone();
    let requested = controller.event(Some(&service_state), "service_request", obj(vec![]), scratch);
    let reviewed = controller.event(
        Some(state(&requested)),
        "service_review",
        obj(vec![
            ("service_route".into(), s("main")),
            ("service_capability".into(), s("")),
            ("service_input".into(), s(r#"{"prompt": "actual input"}"#)),
        ]),
        scratch,
    );
    let approved = controller.event(Some(state(&reviewed)), "approve", obj(vec![]), scratch);
    transfer(controller, &approved, scratch, &mut passed, "service");
    let headers = peer.last_headers();
    keep_detail(
        &mut passed,
        "reviewed service sends exact credential and metadata names",
        header_is(&headers, "X_Service_Key", "secret-service") && header_is(&headers, "X.Meta_Field", "metadata"),
        &format!("{headers:?}"),
    );

    for name in ["TE", "Proxy-Connection"] {
        let mut bad_config = config.clone();
        put(
            items_mut(field_mut(&mut bad_config, "hosts")).first_mut().unwrap_or_else(|| panic!("service config has no host")),
            "auth_header",
            s(name),
        );
        let edited = controller.event(Some(&service_state), "service_connections", obj(vec![]), scratch);
        let bad_json = bad_config.to_string();
        let refused = controller.event(
            Some(state(&edited)),
            "service_save",
            obj(vec![("service_config".into(), s(&bad_json))]),
            scratch,
        );
        keep(
            &mut passed,
            &format!("service {name} cannot replace current transport settings"),
            effect_is_null(&refused)
                && !falsy(state(&refused).get("notice_error"))
                && notice_contains(state(&refused), "transport framing")
                && same_field(state(&refused), &service_state, "service_connections"),
        );
    }

    passed
}

fn service_config(origin: &str) -> Value {
    obj(vec![
        ("version".into(), twin::n("1")),
        (
            "hosts".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("peer")),
                ("url".into(), s(origin)),
                ("plain".into(), twin::b(true)),
                ("enabled".into(), twin::b(true)),
                ("auth_header".into(), s("X_Service_Key")),
                ("auth_prefix".into(), s("")),
                ("key".into(), s("secret-service")),
                ("headers".into(), obj(vec![("X.Meta_Field".into(), s("metadata"))])),
            ])]),
        ),
        (
            "routes".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("main")),
                ("host".into(), s("peer")),
                ("capability".into(), s("image_generation")),
                ("path".into(), s("/service")),
                ("method".into(), s("POST")),
                ("request".into(), obj(vec![("prompt".into(), obj(vec![("$input".into(), s("/prompt"))]))])),
                ("response".into(), s("json")),
                ("response_path".into(), s("")),
                ("enabled".into(), twin::b(true)),
            ])]),
        ),
        ("defaults".into(), obj(vec![])),
    ])
}

/// Send the subject's request to the local peer and return that response through network_result.
/// Header-ownership checks count the request the subject actually built.
fn transfer(controller: &mut Controller, effect: &Value, scratch: &str, passed: &mut Vec<String>, label: &str) -> Value {
    let network = match effect.get("effect") {
        Some(value) if !matches!(value, Value::Null) => value,
        _ => panic!("{label} produced no network effect: {}", notice_label(state(effect))),
    };
    if !matches!(network.get("kind"), Some(Value::Str(kind)) if kind == "network") {
        panic!("{label} effect is not network: {}", network.to_string());
    }
    let raw = wire_bytes(network, label);
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n").unwrap_or_else(|| panic!("{label} wire has no header terminator"));
    let head = std::str::from_utf8(&raw[..split]).unwrap_or_else(|_| panic!("{label} request headers are not UTF-8"));
    let mut lines = head.split("\r\n");
    let _request = lines.next();
    let rest: Vec<&str> = lines.collect();
    for managed in ["host:", "connection:", "accept-encoding:", "content-length:"] {
        let count = rest.iter().filter(|line| line.to_ascii_lowercase().starts_with(managed)).count();
        let name = format!("{label} owns exactly one {managed}");
        if count != 1 {
            panic!("{name}: {head}");
        }
        passed.push(name);
    }

    let host = network.get("host").unwrap_or_else(|| panic!("{label} missing host")).str_().to_string();
    let port = port_of(network.get("port"), label);
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("{label} connect {host}:{port}: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("{label} read timeout: {error}"));
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("{label} write timeout: {error}"));
    stream.write_all(&raw).unwrap_or_else(|error| panic!("{label} write {host}:{port}: {error}"));
    let mut reply = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(count) => reply.extend_from_slice(&buf[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("{label} read {host}:{port}: {error}"),
        }
    }
    controller.event(Some(state(effect)), "network_result", callback_fields(network, &reply), scratch)
}

fn wire_bytes(network: &Value, label: &str) -> Vec<u8> {
    // Python decodes wire_base64 when that key is present, and otherwise encodes wire as text.
    match network.get("wire_base64") {
        Some(Value::Str(encoded)) => twin::b64_decode(encoded),
        Some(Value::Null) => panic!("{label} wire_base64 is null"),
        Some(other) => panic!("{label} wire_base64 is not text: {}", other.to_string()),
        None => network.get("wire").unwrap_or_else(|| panic!("{label} missing wire")).str_().as_bytes().to_vec(),
    }
}

fn callback_fields(network: &Value, reply: &[u8]) -> Value {
    let mut fields = Vec::new();
    for key in ["purpose", "model_ticket", "child_ticket", "child_owner", "drive_ticket"] {
        if let Some(value) = network.get(key) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    let encoded = twin::b64_encode(reply);
    fields.push(("data".into(), s(&encoded)));
    obj(fields)
}

fn port_of(value: Option<&Value>, label: &str) -> u16 {
    match value {
        Some(Value::Num(text)) | Some(Value::Str(text)) => text.parse().unwrap_or_else(|error| panic!("{label} bad port {text}: {error}")),
        _ => panic!("{label} missing port"),
    }
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    fields.push((key.to_string(), Value::Null));
    &mut fields.last_mut().unwrap().1
}

fn items_mut(value: &mut Value) -> &mut Vec<Value> {
    let Value::Arr(items) = value else { panic!("expected array") };
    items
}

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
}

fn falsy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(value) => !twin::truthy(value),
    }
}

fn not_indexed(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(item) => !twin::truthy(item),
        None => false,
    }
}

fn effect_is_null(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn same_field(left: &Value, right: &Value, key: &str) -> bool {
    match (left.get(key), right.get(key)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn notice_label(state: &Value) -> String {
    match state.get("notice") {
        Some(Value::Str(text)) => text.clone(),
        Some(Value::Null) | None => "None".to_string(),
        Some(Value::Bool(flag)) => if *flag { "True".to_string() } else { "False".to_string() },
        Some(Value::Num(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn notice_contains(state: &Value, needle: &str) -> bool {
    match state.get("notice") {
        Some(Value::Str(text)) => text.contains(needle),
        _ => false,
    }
}

fn header_is(headers: &[(String, String)], name: &str, value: &str) -> bool {
    headers.iter().any(|(key, got)| key == name && got == value)
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn keep_detail(passed: &mut Vec<String>, name: &str, ok: bool, detail: &str) {
    if !ok {
        panic!("{name}: {detail}");
    }
    passed.push(name.to_string());
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Peer {
    port: u16,
    seen: Arc<Mutex<Vec<Vec<(String, String)>>>>,
    shutdown: Arc<Mutex<bool>>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("bind peer: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")).port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(Mutex::new(false));
        let thread_seen = Arc::clone(&seen);
        let thread_shutdown = Arc::clone(&shutdown);
        let thread = thread::spawn(move || peer_loop(listener, thread_seen, thread_shutdown));
        Self { port, seen, shutdown, thread: Some(thread) }
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn last_headers(&self) -> Vec<(String, String)> {
        self.seen.lock().unwrap_or_else(|error| panic!("peer state: {error}")).last().cloned().unwrap_or_default()
    }

    fn stop(&mut self) {
        if self.thread.is_none() {
            return;
        }
        *self.shutdown.lock().unwrap_or_else(|error| panic!("peer stop: {error}")) = true;
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn peer_loop(listener: TcpListener, seen: Arc<Mutex<Vec<Vec<(String, String)>>>>, shutdown: Arc<Mutex<bool>>) {
    loop {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) => panic!("peer accept: {error}"),
        };
        if *shutdown.lock().unwrap_or_else(|error| panic!("peer stop: {error}")) {
            break;
        }
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        serve_one(&mut stream, &seen).unwrap_or_else(|error| panic!("peer request: {error}"));
    }
}

fn serve_one(stream: &mut TcpStream, seen: &Mutex<Vec<Vec<(String, String)>>>) -> std::io::Result<()> {
    let (method, path, headers, _body) = read_request(stream)?;
    seen.lock().unwrap_or_else(|error| panic!("peer state: {error}")).push(headers);
    let (status, reason, body) = peer_response(&method, &path);
    write_response(stream, status, reason, body)
}

/// The Python peer answers POST with the unwrapped provider document for that
/// path, and PROPFIND with an empty WebDAV multistatus. Other paths, including
/// the reviewed service, get the generic JSON document.
fn peer_response(method: &str, path: &str) -> (u16, &'static str, &'static str) {
    if method == "PROPFIND" {
        return (207, "Multi-Status", "<d:multistatus xmlns:d=\"DAV:\"/>");
    }
    if method != "POST" {
        return (501, "Unsupported method", "");
    }
    let body = match path.trim_matches('/') {
        "chat" => r#"{"choices": [{"message": {"content": "Actual header-bound reply."}, "finish_reason": "stop"}]}"#,
        "local" => r#"{"message": {"content": "Actual header-bound reply."}, "done": true}"#,
        "messages" => r#"{"content": [{"type": "text", "text": "Actual header-bound reply."}], "stop_reason": "end_turn"}"#,
        "content" => r#"{"candidates": [{"content": {"parts": [{"text": "Actual header-bound reply."}]}, "finishReason": "STOP"}]}"#,
        _ => r#"{"actual": "service reply"}"#,
    };
    (200, "OK", body)
}

fn write_response(stream: &mut TcpStream, status: u16, reason: &str, body: &str) -> std::io::Result<()> {
    let payload = body.as_bytes();
    let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
    stream.write_all(head.as_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let split = loop {
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 1024 * 1024 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request headers too large"));
        }
        let count = read_some(stream, &mut tmp)?;
        if count == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "truncated request"));
        }
        buf.extend_from_slice(&tmp[..count]);
    };
    let head = std::str::from_utf8(&buf[..split]).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    let mut length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }
    let mut body = buf[split + 4..].to_vec();
    while body.len() < length {
        let count = read_some(stream, &mut tmp)?;
        if count == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..count]);
        if body.len() > 4 * 1024 * 1024 + 65536 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request body too large"));
        }
    }
    body.truncate(length);
    Ok((method, path, headers, body))
}

fn read_some(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<usize> {
    loop {
        match stream.read(buf) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

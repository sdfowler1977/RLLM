//! Real local grant HTTP peer and native shared-core device shortcut checks.
mod twin;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use twin::{b64_encode, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner actual local grant HTTP, native shared-core modal/effects and controlled GET destination across four model formats and storage. No physical Android browser/visual acceptance or live user endpoint.";

struct PeerState {
    origin: String,
    mode: String,
    issued: Vec<(String, Value)>,
    opened: Vec<String>,
}

struct Peer {
    shared: Arc<Mutex<PeerState>>,
}

impl Peer {
    fn origin(&self) -> String {
        self.shared.lock().unwrap().origin.clone()
    }
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("could not start peer: {error}"));
        let port = listener.local_addr().unwrap().port();
        let origin = format!("http://127.0.0.1:{port}");
        let shared = Arc::new(Mutex::new(PeerState {
            origin: origin.clone(),
            mode: "valid".to_string(),
            issued: Vec::new(),
            opened: Vec::new(),
        }));
        let worker = Arc::clone(&shared);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(_) => continue,
                };
                let state = Arc::clone(&worker);
                std::thread::spawn(move || serve(stream, state));
            }
        });
        Self { shared }
    }

    fn set_mode(&self, mode: &str) {
        self.shared.lock().unwrap().mode = mode.to_string();
    }

    fn issued(&self) -> Vec<(String, Value)> {
        self.shared.lock().unwrap().issued.clone()
    }

    fn last_opened(&self) -> Option<String> {
        self.shared.lock().unwrap().opened.last().cloned()
    }
}

fn serve(mut stream: TcpStream, shared: Arc<Mutex<PeerState>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    if method == "POST" {
        let form = String::from_utf8_lossy(&body);
        if !form.contains("client_id=public-shortcut") {
            panic!("peer received invalid client_id: {form}");
        }
        let mut state = shared.lock().unwrap();
        let mode = state.mode.clone();
        let code = format!("private-device-{}", state.issued.len());
        let uri = format!("{}/verify", peer_origin(&state));
        let mut value = obj(vec![
            ("device_code".into(), s(&code)),
            ("user_code".into(), s("CODE-7392")),
            ("verification_uri".into(), s(&uri)),
            ("expires_in".into(), n("120")),
            ("interval".into(), n("5")),
        ]);
        let complete = match mode.as_str() {
            "off-origin" => "https://other.invalid/verify?user_code=CODE-7392".to_string(),
            "private" => format!("{uri}?x={code}"),
            "encoded-private" => format!(
                "{uri}?x={}",
                code.bytes().map(|byte| format!("%{byte:02X}")).collect::<String>()
            ),
            "private-field" => format!("{uri}?%2564evice_code=hidden"),
            "malformed" => "file:///private".to_string(),
            "wrong-type" => "true".to_string(),
            "oversized" => format!("{uri}?x={}", "x".repeat(2048)),
            "omitted" => String::new(),
            _ => format!("{uri}?user_code=CODE-7392&session=%2F"),
        };
        if mode != "omitted" {
            value = obj(
                match value {
                    Value::Obj(mut fields) => {
                        fields.push(("verification_uri_complete".into(), s(&complete)));
                        fields
                    }
                    _ => unreachable!(),
                },
            );
        }
        state.issued.push((code, value.clone()));
        let raw = value.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{raw}",
            raw.len()
        );
        let _ = stream.write_all(response.as_bytes());
    } else {
        shared.lock().unwrap().opened.push(path);
        let body = b"Controlled verification page";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    }
}

// Each connection handler holds only the shared state, so the origin it must
// so the shared state keeps only the fields the peer actually publishes.
fn peer_origin(state: &PeerState) -> String {
    state.origin.clone()
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = twin::create_scratch(&root, "device-shortcut");
    let peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch, &peer);
    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);

    let body = obj(vec![
        ("count".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("device_shortcut", &body, true);
    println!("{}", twin::pretty(&body));
}

fn run(controller: &mut Controller, root: &std::path::Path, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    let root_text = root.to_str().unwrap().to_string();
    let targets = ["chat", "local", "messages", "content", "storage"];
    let modes = [
        "valid",
        "omitted",
        "off-origin",
        "private",
        "encoded-private",
        "private-field",
        "malformed",
        "wrong-type",
        "oversized",
    ];
    for target in targets {
        for mode in modes {
            peer.set_mode(mode);
            let waiting = grant(controller, &root_text, peer, target);
            let issued = peer.issued();
            let (_, value) = issued.last().unwrap();
            let code = value.get("device_code").unwrap().str_().to_string();
            let ui = waiting.get("ui").unwrap().to_string();
            let state_text = waiting.get("state").unwrap().to_string();
            let wanted = if mode == "valid" {
                value
                    .get("verification_uri_complete")
                    .unwrap()
                    .str_()
                    .to_string()
            } else {
                format!("{}/verify", peer.origin())
            };
            let opened = controller.event(
                waiting.get("state"),
                "oauth_device_open",
                obj(vec![]),
                &root_text,
            );
            let effect = opened.get("effect").unwrap();

            keep(
                &mut passed,
                &format!("{target}/{mode} keeps manual address and user code"),
                ui.contains(&format!("{}/verify", peer.origin())) && ui.contains("CODE-7392"),
            );
            keep(
                &mut passed,
                &format!("{target}/{mode} private credential absent from UI/state"),
                !ui.contains(&code) && !state_text.contains(&code),
            );
            keep(
                &mut passed,
                &format!("{target}/{mode} emits no automatic browser or polling effect"),
                matches!(waiting.get("effect"), Some(Value::Null))
                    && waiting
                        .get("state")
                        .and_then(|state| state.get("dialog"))
                        .map(Value::str_)
                        == Some("oauth_device"),
            );
            keep(
                &mut passed,
                &format!("{target}/{mode} explicit action chooses safe destination"),
                effect
                    == &obj(vec![
                        ("kind".into(), s("oauth_browser")),
                        ("url".into(), s(&wanted)),
                    ]),
            );
            if mode == "valid" {
                keep(
                    &mut passed,
                    &format!("{target} does not persist optional shortcut"),
                    !state_text.contains(&wanted),
                );
                keep(
                    &mut passed,
                    &format!("{target} review displays exact shortcut destination"),
                    ui.contains(&wanted),
                );
                let address = split_url(&wanted);
                let response = http_get(
                    &address.host,
                    address.port,
                    &address.target,
                );
                assert_status(&response);
                keep(
                    &mut passed,
                    &format!("{target} actual controlled browser-target request preserves URI spelling"),
                    peer.last_opened().as_deref() == Some("/verify?user_code=CODE-7392&session=%2F"),
                );
            }
            let cancelled = controller.event(waiting.get("state"), "oauth_cancel", obj(vec![]), &root_text);
            let late = controller.event(
                cancelled.get("state"),
                "oauth_device_open",
                obj(vec![]),
                &root_text,
            );
            keep(
                &mut passed,
                &format!("{target}/{mode} cancelled grant cannot reopen shortcut"),
                matches!(late.get("effect"), Some(Value::Null)),
            );
        }
    }
    passed
}

fn setup(controller: &mut Controller, root: &str, peer: &Peer, target: &str) -> Value {
    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let state = resumed.get("state").unwrap();
    if target == "storage" {
        let edited = controller.event(Some(state), "drive_edit", obj(vec![]), root);
        let saved = controller.event(
            edited.get("state"),
            "drive_save",
            obj(vec![
                ("url".into(), s(&format!("{}/share/", peer.origin()))),
                ("plain".into(), twin::b(true)),
                ("key".into(), s("retained-key")),
            ]),
            root,
        );
        let settings = controller.event(saved.get("state"), "storage_oauth_settings", obj(vec![]), root);
        controller.event(
            settings.get("state"),
            "oauth_save",
            obj(vec![
                ("device_endpoint".into(), s(&format!("{}/device", peer.origin()))),
                ("token_endpoint".into(), s(&format!("{}/token", peer.origin()))),
                ("client_id".into(), s("public-shortcut")),
                ("plain".into(), twin::b(true)),
            ]),
            root,
        )
    } else {
        let profile = controller.event(Some(state), "profile_new", obj(vec![]), root);
        let saved = controller.event(
            profile.get("state"),
            "profile_save",
            obj(vec![
                ("name".into(), s("Shortcut model")),
                ("protocol".into(), s(target)),
                ("endpoint".into(), s(&format!("{}/model", peer.origin()))),
                ("model".into(), s("controlled")),
                ("key".into(), s("retained-key")),
            ]),
            root,
        );
        let settings = controller.event(saved.get("state"), "oauth_settings", obj(vec![]), root);
        controller.event(
            settings.get("state"),
            "oauth_save",
            obj(vec![
                ("device_endpoint".into(), s(&format!("{}/device", peer.origin()))),
                ("token_endpoint".into(), s(&format!("{}/token", peer.origin()))),
                ("client_id".into(), s("public-shortcut")),
                ("plain".into(), twin::b(true)),
            ]),
            root,
        )
    }
}

fn grant(controller: &mut Controller, root: &str, peer: &Peer, target: &str) -> Value {
    let state = setup(controller, root, peer, target);
    let started = controller.event(state.get("state"), "oauth_device_start", obj(vec![]), root);
    let effect = started.get("effect").unwrap();
    if effect.get("kind").map(Value::str_) != Some("network") {
        panic!("oauth_device_start did not produce a network effect");
    }
    let host = effect.get("host").unwrap().str_().to_string();
    let port = effect.get("port").unwrap().to_string();
    let wire = effect.get("wire").unwrap().str_().to_string();
    let body = wire.split_once("\r\n\r\n").unwrap().1.to_string();
    let response = http_post(&host, &port, "/device", &body);
    assert_status(&response);
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        response.body.len(),
        response.body
    );
    controller.event(
        started.get("state"),
        "network_result",
        obj(vec![
            ("purpose".into(), effect.get("purpose").unwrap().clone()),
            ("data".into(), s(&b64_encode(raw.as_bytes()))),
        ]),
        root,
    )
}

struct HttpResponse {
    status: String,
    body: String,
}

fn assert_status(response: &HttpResponse) {
    if !response.status.contains("200") {
        panic!("peer request failed: {}", response.status);
    }
}

fn http_post(host: &str, port: &str, path: &str, body: &str) -> HttpResponse {
    let mut stream = TcpStream::connect((host, port.parse().unwrap())).unwrap();
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    read_response(stream)
}

fn http_get(host: &str, port: u16, path: &str) -> HttpResponse {
    let mut stream = TcpStream::connect((host, port)).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    read_response(stream)
}

fn read_response(mut stream: TcpStream) -> HttpResponse {
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (headers, body) = text.split_once("\r\n\r\n").unwrap();
    let status = headers.lines().next().unwrap_or("").to_string();
    HttpResponse {
        status,
        body: body.to_string(),
    }
}

struct Url {
    host: String,
    port: u16,
    // The request target the peer must observe, kept verbatim from the advertised
    // shortcut URI so percent-escapes cannot be normalised by a hand-written parse.
    target: String,
}

fn split_url(url: &str) -> Url {
    let rest = url.strip_prefix("http://").unwrap_or_else(|| panic!("advertised uri is not http: {url}"));
    let (authority, target) = match rest.split_once('/') {
        Some((authority, target)) => (authority, format!("/{target}")),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().unwrap_or_else(|error| panic!("uri port: {error}"))),
        None => (authority.to_string(), 80),
    };
    Url { host, port, target }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}
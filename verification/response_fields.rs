mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{obj, s, Controller, Value};

const BODY: &[u8] = b"<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/share/ok.txt</d:href><d:propstat><d:prop><d:displayname>ok.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = twin::create_scratch(&project, "response-fields");
    let root_text = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
        .to_string();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("local address: {error}"));
    let replies = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let requests = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    spawn_peer(listener, replies.clone(), requests.clone());
    let mut controller = Controller::spawn(&project_text);
    let passed = run(&mut controller, &root_text, address, replies, requests);
    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);

    let body = obj(vec![
        ("passed".into(), twin::Value::Num(passed.len().to_string())),
        (
            "checks".into(),
            twin::Value::Arr(passed.iter().map(|name| twin::s(name)).collect()),
        ),
    ]);
    // This leg reports on stdout, not as a file, and the runner counts the
    // printed bytes, so the human summary line its twin printed instead left
    // the sweep with a group that exited 0 and had absent checks.
    twin::report_value_printed("response_fields", &body, true);
}

fn run(
    controller: &mut Controller,
    root: &str,
    address: SocketAddr,
    replies: Arc<Mutex<Vec<Vec<u8>>>>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
) -> Vec<String> {
    let mut passed = Vec::new();
    let valid = valid_response();
    set_reply(&replies, valid.clone());

    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let edited = controller.event(Some(&owned_state(&resumed)), "drive_edit", obj(vec![]), root);
    let saved = controller.event(
        Some(&owned_state(&edited)),
        "drive_save",
        obj(vec![
            ("url".into(), s(&format!("http://{address}/share/"))),
            ("plain".into(), twin::b(true)),
        ]),
        root,
    );
    let mut current = owned_state(&saved);
    current = owned_state(&listing(controller, &current, root));
    keep(
        &mut passed,
        "ordinary WebDAV works without a model or extra services",
        remote_len(&current, 1),
    );

    let malformed = malformed_responses(&valid);
    let expected = [
        "Invalid HTTP status line",
        "Invalid HTTP status line",
        "Invalid response length",
        "Invalid response header",
        "Invalid response header",
        "Invalid response header",
        "Invalid response header",
        "Unsupported informational response",
        "Invalid response header",
        "Unsupported informational response",
        "Unsupported informational response",
        "Invalid chunk size",
        "Invalid response header",
        "Framing trailer forbidden",
    ];
    if malformed.len() != expected.len() {
        panic!("malformed response count {} != {}", malformed.len(), expected.len());
    }

    for (index, raw) in malformed.into_iter().enumerate() {
        let Some(old) = current.get("drive").cloned() else {
            panic!("refusal {index} retains destination and credentials");
        };
        set_reply(&replies, raw);
        current = owned_state(&listing(controller, &current, root));
        keep(
            &mut passed,
            &format!("malformed response {index} refused by actual storage event"),
            not_busy(&current) && notice_eq(&current, expected[index]),
        );
        keep(
            &mut passed,
            &format!("refusal {index} retains destination and credentials"),
            current.get("drive") == Some(&old),
        );

        set_reply(&replies, valid.clone());
        current = owned_state(&listing(controller, &current, root));
        keep(
            &mut passed,
            &format!("ordinary storage recovers after refusal {index}"),
            remote_len(&current, 1) && notice_clean(&current),
        );
    }

    let interim = {
        let mut bytes = b"HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n".to_vec();
        bytes.extend_from_slice(&replace(&valid, b"Connection: close", b"X_Custom: \tvalue\t"));
        bytes
    };
    set_reply(&replies, interim);
    current = owned_state(&listing(controller, &current, root));
    keep(
        &mut passed,
        "valid interim and token field names remain supported",
        remote_len(&current, 1) && notice_clean(&current),
    );

    let observed = requests
        .lock()
        .unwrap_or_else(|error| panic!("request lock: {error}"));
    keep(
        &mut passed,
        "every exchange used actual configured storage peer",
        observed.len() == 30 && observed.iter().all(|request| request.starts_with(b"PROPFIND /share/")),
    );
    passed
}

fn listing(controller: &mut Controller, state: &Value, root: &str) -> Value {
    let event = controller.event(Some(state), "drive_list", obj(vec![]), root);
    let effect = event.get("effect").cloned().unwrap_or_else(|| {
        let notice = event
            .get("state")
            .and_then(|state| state.get("notice"))
            .map(Value::to_string)
            .unwrap_or_else(|| "<missing>".into());
        panic!("drive_list returned no effect: {notice}")
    });
    if effect.get("kind").map(Value::str_) != Some("network") {
        panic!("drive_list effect is not network: {}", effect.to_string());
    }
    let host = effect
        .get("host")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("missing host"))
        .to_string();
    let port = port_of(effect.get("port").unwrap_or_else(|| panic!("missing port")));
    let wire = effect
        .get("wire")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("missing wire"))
        .to_string();

    let mut stream = std::net::TcpStream::connect((host.as_str(), port))
        .unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream
        .write_all(wire.as_bytes())
        .unwrap_or_else(|error| panic!("write: {error}"));
    let mut data = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => data.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("read: {error}"),
        }
    }

    let mut values = obj(vec![]);
    for key in [
        "purpose",
        "drive_ticket",
        "child_ticket",
        "child_owner",
        "model_ticket",
    ] {
        if let Some(value) = effect.get(key) {
            put(&mut values, key, value.clone());
        }
    }
    put(&mut values, "data", s(&twin::b64_encode(&data)));
    let listed = event
        .get("state")
        .cloned()
        .unwrap_or_else(|| panic!("missing listing state"));
    controller.event(Some(&listed), "network_result", values, root)
}

fn malformed_responses(valid: &[u8]) -> Vec<Vec<u8>> {
    let mut malformed = vec![
        replace(valid, b"HTTP/1.1", b"HTTP/1.x"),
        replace(valid, b"207 ", b"+207 "),
        replace(valid, b"Content-Length: ", b"Content-Length: +"),
    ];
    for field in [
        &b"Bad Name: x"[..],
        &b"X: bad\rInjected: yes"[..],
        &b"X: bad\nInjected: yes"[..],
        &b"X: bad\0value"[..],
    ] {
        malformed.push(replace(valid, b"Connection: close", field));
    }
    for prefix in [
        &b"HTTP/1.1 101 Switching Protocols\r\n\r\n"[..],
        &b"HTTP/1.1 100 Continue\r\nBad Name: x\r\n\r\n"[..],
        &b"HTTP/1.1 103 Early Hints\r\nContent-Length: 2\r\n\r\n"[..],
        &b"HTTP/1.1 100 Continue\r\nTransfer-Encoding: chunked\r\n\r\n"[..],
    ] {
        malformed.push([prefix, valid].concat());
    }
    let chunked = b"HTTP/1.1 207 Multi-Status\r\nTransfer-Encoding: chunked\r\n\r\n";
    // Chunk sizes are hexadecimal, matching format(len(body), 'x').
    let size = format!("{:x}", BODY.len());
    malformed.push(
        [
            chunked.as_slice(),
            b"+".as_slice(),
            size.as_bytes(),
            b"\r\n".as_slice(),
            BODY,
            b"\r\n0\r\n\r\n".as_slice(),
        ]
        .concat(),
    );
    malformed.push(
        [
            chunked.as_slice(),
            size.as_bytes(),
            b"\r\n".as_slice(),
            BODY,
            b"\r\n0\r\nBad Name: x\r\n\r\n".as_slice(),
        ]
        .concat(),
    );
    malformed.push(
        [
            chunked.as_slice(),
            size.as_bytes(),
            b"\r\n".as_slice(),
            BODY,
            b"\r\n0\r\nContent-Length: 0\r\n\r\n".as_slice(),
        ]
        .concat(),
    );
    malformed
}

fn valid_response() -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        BODY.len()
    )
    .into_bytes();
    response.extend_from_slice(BODY);
    response
}

fn replace(input: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let position = input
        .windows(from.len())
        .position(|window| window == from)
        .unwrap_or_else(|| panic!("replacement target not found"));
    let mut output = input[..position].to_vec();
    output.extend_from_slice(to);
    output.extend_from_slice(&input[position + from.len()..]);
    output
}

fn spawn_peer(listener: TcpListener, replies: Arc<Mutex<Vec<Vec<u8>>>>, requests: Arc<Mutex<Vec<Vec<u8>>>>) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap_or_else(|error| panic!("accept: {error}"));
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap_or_else(|error| panic!("read timeout: {error}"));
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut complete = false;
            while request.len() < 65536 {
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    complete = true;
                    break;
                }
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(count) => request.extend_from_slice(&chunk[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => panic!("peer read: {error}"),
                }
            }
            if !complete {
                continue;
            }
            requests
                .lock()
                .unwrap_or_else(|error| panic!("request lock: {error}"))
                .push(request);
            let reply = replies
                .lock()
                .unwrap_or_else(|error| panic!("reply lock: {error}"))
                .last()
                .cloned()
                .unwrap_or_else(valid_response);
            stream
                .write_all(&reply)
                .unwrap_or_else(|error| panic!("peer write: {error}"));
            stream.flush().unwrap_or_else(|error| panic!("peer flush: {error}"));
            let _ = stream.shutdown(Shutdown::Write);
        }
    });
}

fn set_reply(replies: &Arc<Mutex<Vec<Vec<u8>>>>, reply: Vec<u8>) {
    let mut replies = replies
        .lock()
        .unwrap_or_else(|error| panic!("reply lock: {error}"));
    replies.clear();
    replies.push(reply);
}

fn owned_state(value: &Value) -> Value {
    value
        .get("state")
        .cloned()
        .unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn port_of(value: &Value) -> u16 {
    let token = match value {
        Value::Num(token) | Value::Str(token) => token.as_str(),
        other => panic!("invalid port {}", other.to_string()),
    };
    token
        .parse()
        .unwrap_or_else(|error| panic!("invalid port {token}: {error}"))
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

fn remote_len(state: &Value, expected: usize) -> bool {
    match state.get("remote") {
        Some(Value::Arr(items)) => items.len() == expected,
        _ => false,
    }
}

fn not_busy(state: &Value) -> bool {
    match state.get("busy") {
        Some(busy) => !twin::truthy(busy),
        None => false,
    }
}

fn notice_eq(state: &Value, expected: &str) -> bool {
    match state.get("notice") {
        Some(Value::Str(text)) => text == expected,
        _ => false,
    }
}

fn notice_clean(state: &Value) -> bool {
    match state.get("notice") {
        Some(Value::Str(text)) => !text.to_ascii_lowercase().contains("error"),
        _ => false,
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

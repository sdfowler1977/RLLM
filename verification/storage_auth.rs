mod twin;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{obj, Controller, Value};

const SCOPE: &str = "Owner public core events and actual loopback WebDAV sockets for no-auth/Basic/Bearer/custom headers, conditional create/readback/folder operations, and legacy/current opaque snapshot writes/probes/readback. Model callbacks and stored records are controlled inputs. Live user NAS/cloud account and Android secure-state acceptance remains separate; no application-owned cloud OAuth or SMB is claimed.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    // One root for every event, like the leg's single TemporaryDirectory: the
    // subject keys pending network results to the event's approval root.
    let passed = run(&mut controller, scratch.path(), scratch.path(), &peer);
    controller.finish();
    peer.stop();

    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), twin::Value::Arr(passed.into_iter().map(|name| twin::s(&name)).collect())),
        ("scope".into(), twin::s(SCOPE)),
    ]);
    twin::report_value_printed("storage_auth", &body, true);
}

fn run(controller: &mut Controller, root: &str, scratch: &str, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    let url = peer.url();
    let mut d = event(controller, None, "resume", obj(vec![]), scratch);
    d = save(controller, &d, root, &url, vec![]);
    d = listing(controller, &d, root);

    keep(
        &mut passed,
        "empty credential settings retain real unauthenticated WebDAV",
        peer.last_call().headers.is_empty() && remote_len(&d) == 1,
    );

    d = save(
        controller,
        &d,
        root,
        &url,
        vec![("user", twin::s("saved-user")), ("password", twin::s("saved-password"))],
    );
    let basic = format!("Basic {}", twin::b64_encode(b"saved-user:saved-password"));
    peer.set_expected(&[("authorization", &basic)]);
    d = listing(controller, &d, root);
    keep(
        &mut passed,
        "legacy username/password settings retain exact Basic authentication",
        peer.last_call().headers == vec![("authorization".into(), basic.clone())] && remote_len(&d) == 1,
    );

    d = save(controller, &d, root, &url, vec![("key", twin::s("controlled-storage-token"))]);
    let bearer = "Bearer controlled-storage-token".to_string();
    peer.set_expected(&[("authorization", &bearer)]);
    d = listing(controller, &d, root);
    keep(
        &mut passed,
        "default key uses one Bearer header without Basic fallback",
        peer.last_call().headers == vec![("authorization".into(), bearer.clone())] && remote_len(&d) == 1,
    );
    keep(
        &mut passed,
        "key mode preserves existing username/password credentials",
        drive_field(&d, "user").str_() == "saved-user" && drive_field(&d, "password").str_() == "saved-password",
    );

    let opened = event(controller, Some(&d), "drive_edit", obj(vec![]), scratch);
    let fields = modal_fields(&opened);
    keep(
        &mut passed,
        "storage key has a secret control and configurable protocol fields",
        field(&fields, "key").get("kind").is_some_and(|value| value.str_() == "secret")
            && field(&fields, "auth_header").get("value").is_some_and(|value| value.str_() == "Authorization")
            && field(&fields, "auth_prefix").get("value").is_some_and(|value| value.str_() == "Bearer"),
    );
    let status = field(&fields, "auth_status").get("value").unwrap().str_();
    keep(
        &mut passed,
        "saved credential mode is explicit without printing the key",
        status.contains("Key/token header active") && !status.contains("controlled-storage-token"),
    );

    d = save(
        controller,
        &d,
        root,
        &url,
        vec![("auth_header", twin::s("X-Storage-Key")), ("auth_prefix", twin::s("Token"))],
    );
    let custom = "Token controlled-storage-token".to_string();
    peer.set_expected(&[("x-storage-key", &custom)]);
    d = listing(controller, &d, root);
    keep(
        &mut passed,
        "custom credential header and prefix reach the real server",
        peer.last_call().headers == vec![("x-storage-key".into(), custom.clone())] && remote_len(&d) == 1,
    );

    d = save(controller, &d, root, &url, vec![("auth_prefix", twin::s(""))]);
    let raw_key = "controlled-storage-token".to_string();
    peer.set_expected(&[("x-storage-key", &raw_key)]);
    d = listing(controller, &d, root);
    keep(
        &mut passed,
        "explicit empty prefix sends the exact raw key",
        peer.last_call().headers == vec![("x-storage-key".into(), raw_key.clone())],
    );

    d = event(controller, Some(&d), "remote:0", obj(vec![]), scratch);
    d = complete(controller, &d, root);
    keep(
        &mut passed,
        "file reads retain the same configured credential method",
        peer.last_call().method == "GET"
            && peer.last_call().headers == vec![("x-storage-key".into(), raw_key.clone())]
            && ui_text(&d).contains("Observed storage text."),
    );

    d = event(controller, Some(&d), "drive_editor", obj(vec![]), scratch);
    d = event(
        controller,
        Some(&d),
        "drive_create_review",
        obj(vec![("path".into(), twin::s("created.txt")), ("content".into(), twin::s("Actual saved content."))]),
        scratch,
    );
    keep(
        &mut passed,
        "custom-key write remains behind ordinary review",
        d.get("effect").is_some_and(|value| *value == Value::Null) && state_text(&d, "dialog") == "approval",
    );
    d = event(controller, Some(&d), "approve", obj(vec![]), scratch);
    d = complete(controller, &d, root);
    d = complete(controller, &d, root);
    let calls = peer.calls();
    keep(
        &mut passed,
        "conditional create and exact readback use the configured key",
        peer.file("/share/created.txt") == Some(b"Actual saved content.".to_vec())
            && calls.len() >= 2
            && calls[calls.len() - 2].method == "PUT"
            && calls[calls.len() - 1].method == "GET"
            && calls[calls.len() - 2].headers == vec![("x-storage-key".into(), raw_key.clone())]
            && calls[calls.len() - 1].headers == vec![("x-storage-key".into(), raw_key.clone())]
            && ui_text(&d).contains("readback verified"),
    );

    d = event(controller, Some(&d), "drive_folder", obj(vec![]), scratch);
    d = event(
        controller,
        Some(&d),
        "drive_folder_review",
        obj(vec![("path".into(), twin::s("created-folder"))]),
        scratch,
    );
    let approved = event(controller, Some(&d), "approve", obj(vec![]), scratch);
    d = complete(controller, &approved, root);
    keep(
        &mut passed,
        "folder creation uses the same credential boundary",
        peer.last_call().method == "MKCOL"
            && peer.last_call().headers == vec![("x-storage-key".into(), raw_key.clone())]
            && state(&d).get("busy").is_some_and(|value| !twin::truthy(value)),
    );

    let exported = event(controller, Some(&d), "configuration_export", obj(vec![]), scratch);
    let content = exported.get("effect").unwrap().get("content").unwrap().str_().to_string();
    let parsed = Value::parse(&content);
    keep(
        &mut passed,
        "configuration export omits key and retained credentials",
        !content.contains("controlled-storage-token")
            && !content.contains("saved-user")
            && !content.contains("saved-password")
            && parsed.get("credentials_omitted").is_some_and(Value::bool_),
    );
    let ticket = exported.get("effect").unwrap().get("ticket").unwrap().clone();
    d = event(
        controller,
        Some(&exported),
        "export_cancel",
        obj(vec![("ticket".into(), ticket)]),
        scratch,
    );

    let stable = drive_value(&d);
    let invalid = vec![
        vec![("auth_header", twin::s("Host"))],
        vec![("auth_header", twin::s("Connection"))],
        vec![("auth_header", twin::s("Content-Length"))],
        vec![("auth_header", twin::s("Transfer-Encoding"))],
        vec![("auth_header", twin::s("Content-Type"))],
        vec![("auth_header", twin::s("Accept-Encoding"))],
        vec![("auth_header", twin::s("X-Key: injected"))],
        vec![("auth_prefix", twin::s("Bearer\r\nInjected"))],
        vec![("key", twin::s("secret\nInjected"))],
        vec![("key", twin::s(&"x".repeat(8193)))],
        vec![("key", twin::b(true))],
        vec![("auth_prefix", twin::b(true))],
        vec![("auth_header", twin::b(true))],
    ];
    for change in invalid {
        let before = peer.calls().len();
        let rejected = save(controller, &d, root, &url, change.clone());
        let name = format!("invalid active credential refuses atomically {}", changed_names(&change));
        keep(
            &mut passed,
            &name,
            rejected.get("effect").is_some_and(|value| *value == Value::Null)
                && drive_value(&rejected) == stable
                && state(&rejected).get("notice_error").is_some_and(twin::truthy)
                && peer.calls().len() == before,
        );
    }

    d = save(controller, &d, root, &url, vec![("key", twin::s(""))]);
    peer.set_expected(&[("authorization", &basic)]);
    d = listing(controller, &d, root);
    keep(
        &mut passed,
        "clearing the key explicitly restores retained Basic mode",
        peer.last_call().headers == vec![("authorization".into(), basic.clone())],
    );

    d = save(
        controller,
        &d,
        root,
        &url,
        vec![("key", twin::s("controlled-storage-token")), ("auth_header", twin::s("Authorization")), ("auth_prefix", twin::s("Bearer"))],
    );
    peer.set_expected(&[("authorization", &bearer)]);
    peer.set_refuse(true);
    let before = peer.calls().len();
    d = listing(controller, &d, root);
    keep(
        &mut passed,
        "401 refusal does not replay with another credential",
        peer.calls().len() == before + 1
            && d.get("effect").is_some_and(|value| *value == Value::Null)
            && state(&d).get("busy").is_some_and(|value| !twin::truthy(value))
            && peer.last_call().headers == vec![("authorization".into(), bearer.clone())],
    );
    peer.set_refuse(false);

    d = listing(controller, &d, root);
    keep(
        &mut passed,
        "a scoped authentication refusal preserves later storage use",
        remote_len(&d) == 1 && peer.last_call().headers == vec![("authorization".into(), bearer.clone())],
    );

    // Python assigns profiles in place on the resumed state object.
    let mut model_state = state(&d).clone();
    put(
        &mut model_state,
        "profiles",
        twin::Value::Arr(vec![obj(vec![
            ("name".into(), twin::s("Primary")),
            ("endpoint".into(), twin::s("http://model.test/model")),
            ("protocol".into(), twin::s("chat")),
            ("model".into(), twin::s("controlled")),
            ("plain".into(), twin::b(true)),
            ("stream".into(), twin::b(false)),
        ])]),
    );
    let sent = event(
        controller,
        Some(&model_state),
        "send",
        obj(vec![("draft".into(), twin::s("Keep ordinary text usable."))]),
        scratch,
    );
    let wire = sent.get("effect").unwrap().get("wire").unwrap().str_();
    keep(
        &mut passed,
        "storage credentials never enter the primary model request",
        !wire.contains("controlled-storage-token") && !wire.contains("saved-password"),
    );
    let ticket = state(&sent).get("model_expected").unwrap().clone();
    let blocked = save(controller, &sent, root, &url, vec![("key", twin::s("replacement"))]);
    keep(
        &mut passed,
        "editing storage during a model request preserves its current owner",
        state(&blocked).get("busy").is_some_and(Value::bool_)
            && state(&blocked).get("model_expected").is_some_and(|value| *value == ticket)
            && blocked.get("effect").is_some_and(|value| *value == Value::Null),
    );
    d = event(controller, Some(&sent), "cancel", obj(vec![]), scratch);

    let payload = b"\x00\xffopaque saved payload\x80".to_vec();
    for streamed in [false, true] {
        let label = if streamed { "streamed" } else { "legacy" };
        for phase in ["write", "probe", "verify"] {
            let target = format!("/share/{label}-snapshot.bin");
            let mut record = obj(vec![
                ("stage_ticket".into(), twin::s(&format!("owned-{label}"))),
                ("purpose".into(), twin::s(&format!("project_transfer:owned-{phase}"))),
                ("bytes".into(), twin::n(&payload.len().to_string())),
                ("phone".into(), twin::b(false)),
                ("tree".into(), Value::Null),
                ("connection".into(), drive_value(&d)),
                ("target".into(), twin::s(&format!("{url}{}", target.trim_start_matches("/share/")))),
                ("mime".into(), twin::s("application/octet-stream")),
                ("source".into(), Value::Null),
                ("phase".into(), twin::s(phase)),
            ]);
            if streamed {
                put(&mut record, "format", twin::n("2"));
            }
            let stored = record.clone();
            let operation = if streamed { "project_chunks" } else { "project_payload" };
            let envelope = obj(vec![
                ("op".into(), twin::s(operation)),
                ("action".into(), twin::s("open")),
                ("record".into(), record.clone()),
                ("stored".into(), stored.clone()),
                ("archive".into(), twin::s(&twin::b64_encode(&payload))),
            ]);
            let opened = controller.op(&envelope);
            let handle = opened.get("snapshot_handle").unwrap().clone();
            let wire_bytes = opened
                .get("wire_base64")
                .map(|value| twin::b64_decode(value.str_()))
                .unwrap_or_else(|| opened.get("wire").unwrap().str_().as_bytes().to_vec());
            keep(
                &mut passed,
                &format!("{label} {phase} retains its pinned key and excludes Basic credentials"),
                contains_bytes(&wire_bytes, b"Authorization: Bearer controlled-storage-token\r\n")
                    && !contains_bytes(&wire_bytes, b"Authorization: Basic "),
            );
            let received = if streamed && phase == "write" {
                exchange_with_payload(&opened, &payload)
            } else {
                exchange(&opened)
            };
            let status_prefix: &[u8] = if phase == "write" { b"HTTP/1.0 201" } else { b"HTTP/1.0 200" };
            keep(
                &mut passed,
                &format!("{label} {phase} reaches the actual authenticated storage peer"),
                received.starts_with(status_prefix) && peer.last_call().headers == vec![("authorization".into(), bearer.clone())],
            );
            keep(
                &mut passed,
                &format!("{label} {phase} preserves exact source payload or readback"),
                peer.last_call().body == if phase == "write" { payload.clone() } else { Vec::new() }
                    && peer.file(&target) == Some(payload.clone()),
            );
            controller.op(&obj(vec![
                ("op".into(), twin::s(if streamed { "project_chunks" } else { "project_close" })),
                ("action".into(), twin::s("close")),
                ("handle".into(), handle),
            ]));
            let mut changed = stored.clone();
            let connection = field_mut(&mut changed, "connection");
            put(connection, "key", twin::s("another-key"));
            let changed_envelope = obj(vec![
                ("op".into(), twin::s(operation)),
                ("action".into(), twin::s("open")),
                ("record".into(), record),
                ("stored".into(), changed),
                ("archive".into(), twin::s(&twin::b64_encode(&payload))),
            ]);
            let rejected = controller.send(&changed_envelope);
            keep(
                &mut passed,
                &format!("{label} {phase} refuses changed pinned credentials before transport"),
                rejected.get("error").is_some() && rejected.get("value").is_none(),
            );
        }
    }
    passed
}

fn listing(controller: &mut Controller, d: &Value, root: &str) -> Value {
    let listed = event(controller, Some(d), "drive_list", obj(vec![]), root);
    complete(controller, &listed, root)
}

fn complete(controller: &mut Controller, d: &Value, root: &str) -> Value {
    let effect = d.get("effect").unwrap();
    let mut values = Vec::new();
    for key in ["purpose", "drive_ticket", "child_ticket", "child_owner", "model_ticket"] {
        if let Some(value) = effect.get(key) {
            values.push((key.into(), value.clone()));
        }
    }
    values.push(("data".into(), twin::s(&twin::b64_encode(&exchange(effect)))));
    event(controller, Some(d), "network_result", obj(values), root)
}

fn save(controller: &mut Controller, d: &Value, root: &str, url: &str, changes: Vec<(&str, Value)>) -> Value {
    let edited = event(controller, Some(d), "drive_edit", obj(vec![]), root);
    let mut fields = vec![("url".into(), twin::s(url)), ("plain".into(), twin::b(true))];
    fields.extend(changes.into_iter().map(|(key, value)| (key.into(), value)));
    event(controller, Some(state(&edited)), "drive_save", obj(fields), root)
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    // Callers hold the whole event result. Python passes that result's state
    // into the next event; an application state has no effect field.
    controller.event(state.map(as_state), action, inputs, root)
}

fn as_state(value: &Value) -> &Value {
    if value.get("effect").is_some() {
        value.get("state").unwrap_or_else(|| panic!("event reply carried an effect without a state"))
    } else {
        value
    }
}

fn exchange(effect: &Value) -> Vec<u8> {
    exchange_with_payload(effect, &[])
}

fn exchange_with_payload(effect: &Value, payload: &[u8]) -> Vec<u8> {
    let host = effect.get("host").unwrap().str_();
    let port = effect.get("port").unwrap().to_string();
    let mut wire = effect
        .get("wire_base64")
        .map(|value| twin::b64_decode(value.str_()))
        .unwrap_or_else(|| effect.get("wire").unwrap().str_().as_bytes().to_vec());
    wire.extend_from_slice(payload);
    let mut stream = TcpStream::connect((host, port.parse::<u16>().unwrap())).unwrap_or_else(|error| panic!("connect: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(&wire).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    response
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

// Python names each refusal with str(list(change)), which is the dict keys in
// repr form: ['auth_header']. Debug formatting would emit double quotes.
fn changed_names(changes: &[(&str, Value)]) -> String {
    let names = changes.iter().map(|(key, _)| format!("'{key}'")).collect::<Vec<_>>().join(", ");
    format!("[{names}]")
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn ui_text(value: &Value) -> String {
    // The leg searches json.dumps(d['ui']) only, never the whole event result.
    value.get("ui").map(Value::to_string).unwrap_or_default()
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn state_text<'a>(value: &'a Value, key: &str) -> &'a str {
    state(value).get(key).unwrap().str_()
}

fn drive_value(value: &Value) -> Value {
    state(value).get("drive").unwrap().clone()
}

fn drive_field<'a>(value: &'a Value, key: &str) -> &'a Value {
    state(value).get("drive").and_then(|drive| drive.get(key)).unwrap_or_else(|| panic!("missing drive.{key}"))
}

fn remote_len(value: &Value) -> usize {
    state(value).get("remote").unwrap().arr().len()
}

fn modal_fields(value: &Value) -> HashMap<String, Value> {
    value
        .get("ui")
        .and_then(|ui| ui.get("modal"))
        .and_then(|modal| modal.get("fields"))
        .unwrap()
        .arr()
        .iter()
        .filter_map(|field| Some((field.get("key")?.str_().to_string(), field.clone())))
        .collect()
}

fn field<'a>(fields: &'a HashMap<String, Value>, key: &str) -> &'a Value {
    fields.get(key).unwrap_or_else(|| panic!("missing field {key}"))
}

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    fields.push((key.to_string(), Value::Null));
    &mut fields.last_mut().unwrap().1
}

struct Call {
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct PeerState {
    calls: Vec<Call>,
    files: HashMap<String, Vec<u8>>,
    expected: Vec<(String, String)>,
    refuse: bool,
}

struct Peer {
    address: String,
    port: u16,
    state: Arc<Mutex<PeerState>>,
    stop: Arc<Mutex<bool>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(PeerState {
            calls: Vec::new(),
            files: HashMap::from([("/share/alpha.txt".into(), b"Observed storage text.".to_vec())]),
            expected: Vec::new(),
            refuse: false,
        }));
        let stop = Arc::new(Mutex::new(false));
        let thread_state = state.clone();
        let thread_stop = stop.clone();
        let thread = thread::spawn(move || {
            for stream in listener.incoming() {
                if *thread_stop.lock().unwrap() {
                    break;
                }
                let mut stream = stream.unwrap();
                serve(&mut stream, &thread_state);
            }
        });
        Self { address: "127.0.0.1".into(), port, state, stop, thread: Some(thread) }
    }

    fn url(&self) -> String {
        format!("http://{}:{}/share/", self.address, self.port)
    }

    fn set_expected(&self, expected: &[(&str, &str)]) {
        let mut state = self.state.lock().unwrap();
        state.expected = expected.iter().map(|(key, value)| (key.to_ascii_lowercase(), value.to_string())).collect();
    }

    fn set_refuse(&self, refuse: bool) {
        self.state.lock().unwrap().refuse = refuse;
    }

    fn calls(&self) -> Vec<CallSnapshot> {
        self.state
            .lock()
            .unwrap()
            .calls
            .iter()
            .map(|call| CallSnapshot {
                method: call.method.clone(),
                headers: call.headers.clone(),
                body: call.body.clone(),
            })
            .collect()
    }

    fn last_call(&self) -> CallSnapshot {
        self.calls().last().cloned().unwrap_or_else(|| panic!("no peer calls"))
    }

    fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.state.lock().unwrap().files.get(path).cloned()
    }

    fn stop(mut self) {
        *self.stop.lock().unwrap() = true;
        let _ = TcpStream::connect((self.address, self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone)]
struct CallSnapshot {
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn serve(stream: &mut TcpStream, state: &Arc<Mutex<PeerState>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let (method, path, headers, body) = read_request(stream);
    // Python records only Authorization and X-Storage-Key, lowercased, in arrival order.
    let credential_headers = headers
        .iter()
        .filter(|(key, _)| matches!(key.as_str(), "authorization" | "x-storage-key"))
        .cloned()
        .collect::<Vec<_>>();
    let mut state = state.lock().unwrap_or_else(|error| panic!("peer lock: {error}"));
    state.calls.push(Call { method: method.clone(), headers: credential_headers.clone(), body: body.clone() });

    let mut status = 200u16;
    let mut data = Vec::new();
    if state.refuse || !same_credentials(&credential_headers, &state.expected) {
        status = 401;
    } else if method == "PROPFIND" {
        status = 207;
        data = b"<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/share/alpha.txt</d:href><d:propstat><d:prop><d:displayname>alpha.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>".to_vec();
    } else if method == "PUT" {
        let exists = state.files.contains_key(&path);
        let conditional = headers.iter().any(|(key, value)| key == "if-none-match" && value == "*");
        if conditional && exists {
            status = 412;
        } else {
            state.files.insert(path.clone(), body);
            status = 201;
        }
    } else if method == "MKCOL" {
        status = 201;
    } else if let Some(file) = state.files.get(&path) {
        data = file.clone();
    } else {
        status = 404;
    }
    drop(state);

    // BaseHTTPRequestHandler answers HTTP/1.0 even when the subject sent HTTP/1.1.
    // The snapshot checks require that exact status-line prefix.
    let reason = match status {
        200 => "OK",
        201 => "Created",
        207 => "Multi-Status",
        401 => "Unauthorized",
        404 => "Not Found",
        412 => "Precondition Failed",
        _ => "Unknown",
    };
    write!(stream, "HTTP/1.0 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())
        .unwrap_or_else(|error| panic!("response head: {error}"));
    stream.write_all(&data).unwrap_or_else(|error| panic!("response body: {error}"));
    stream.flush().unwrap_or_else(|error| panic!("response flush: {error}"));
    let _ = stream.shutdown(Shutdown::Both);
}

fn read_request(stream: &mut TcpStream) -> (String, String, Vec<(String, String)>, Vec<u8>) {
    let mut buffer = Vec::new();
    let mut temporary = [0u8; 8192];
    let header_end = loop {
        let count = match stream.read(&mut temporary) {
            Ok(0) => panic!("request ended before headers"),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("request read: {error}"),
        };
        buffer.extend_from_slice(&temporary[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > 1_048_576 {
            panic!("request headers exceed 1 MiB");
        }
    };
    let header = std::str::from_utf8(&buffer[..header_end]).unwrap_or_else(|error| panic!("request headers are not UTF-8: {error}"));
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let length = headers
        .iter()
        .find(|(key, _)| key == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let count = match stream.read(&mut temporary) {
            Ok(0) => panic!("request ended before the body"),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("request body read: {error}"),
        };
        body.extend_from_slice(&temporary[..count]);
        if body.len() > 8 * 1024 * 1024 {
            panic!("request body exceeds 8 MiB");
        }
    }
    body.truncate(length);
    (method, path, headers, body)
}

// Python compares credential dicts, so the last repeated header wins and order does not.
fn same_credentials(actual: &[(String, String)], expected: &[(String, String)]) -> bool {
    fn fold(headers: &[(String, String)]) -> Vec<(String, String)> {
        let mut folded: Vec<(String, String)> = Vec::new();
        for (key, value) in headers {
            if let Some(slot) = folded.iter_mut().find(|(name, _)| name == key) {
                slot.1.clone_from(value);
            } else {
                folded.push((key.clone(), value.clone()));
            }
        }
        folded.sort();
        folded
    }
    fold(actual) == fold(expected)
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|error| panic!("clock: {error}"))
            .as_nanos();
        let path = root.join("output").join(format!("storage-auth-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("could not create {}: {error}", path.display()));
        Self(path)
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
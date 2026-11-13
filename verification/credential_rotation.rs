mod twin;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{b, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner execution through actual local HTTP authorization/token/storage/model peers. Malformed grant and rotation across all four model formats, typed storage owner, manual recovery and single-operation uncertainty; no independent or installed-device acceptance.";
const REPORT: &str = "output/checks/credential_rotation.json";

static SCRATCH_ROOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[derive(Default)]
struct PeerState {
    calls: Vec<(String, String, HashMap<String, String>, Vec<u8>)>,
    codes: HashMap<String, Query>,
    refreshes: HashMap<String, String>,
    accesses: HashMap<String, String>,
    devices: HashMap<String, String>,
    files: HashMap<String, Vec<u8>>,
    override_refresh: Option<Option<Value>>,
    origin: String,
}

#[derive(Clone, Default)]
struct Query {
    values: HashMap<String, String>,
}

struct Peer {
    state: Arc<Mutex<PeerState>>,
    origin: String,
    listener: TcpListener,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("peer address: {error}"))
            .port();
        let origin = format!("http://127.0.0.1:{port}");
        let peer = Self {
            state: Arc::new(Mutex::new(PeerState {
                files: HashMap::from([(
                    "/share/alpha.txt".to_string(),
                    b"Actual authorized storage text.".to_vec(),
                )]),
                origin: origin.clone(),
                ..PeerState::default()
            })),
            origin,
            listener,
        };
        let thread_state = Arc::clone(&peer.state);
        let listener = peer
            .listener
            .try_clone()
            .unwrap_or_else(|error| panic!("peer listener clone: {error}"));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let state = Arc::clone(&thread_state);
                thread::spawn(move || handle_client(stream, state));
            }
        });
        peer
    }

    fn set_override(&self, value: Option<Value>) {
        self.state
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .override_refresh = value.map(Some);
    }

    fn clear_override(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .override_refresh = None;
    }

    fn allow_refresh(&self, token: &str, scope: &str) {
        self.state
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .refreshes
            .insert(token.to_string(), scope.to_string());
    }
}

fn handle_client(mut stream: TcpStream, state: Arc<Mutex<PeerState>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .unwrap_or_else(|error| panic!("peer clone: {error}")),
    );
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if length != 0 {
        reader
            .read_exact(&mut body)
            .unwrap_or_else(|error| panic!("peer body: {error}"));
    }
    let (path, query) = split_target(&target);
    let reply = {
        let mut peer = state
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"));
        route(&mut peer, &method, &path, &query, &headers, body)
    };
    let _ = stream.write_all(&reply);
    let _ = stream.flush();
}

fn split_target(target: &str) -> (String, Query) {
    let (path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let mut values = HashMap::new();
    for pair in raw_query.split('&').filter(|part| !part.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key), percent_decode(value));
    }
    (percent_decode(path), Query { values })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'%'));
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn route(
    peer: &mut PeerState,
    method: &str,
    path: &str,
    query: &Query,
    headers: &HashMap<String, String>,
    body: Vec<u8>,
) -> Vec<u8> {
    if method == "GET" && path == "/authorize" {
        let code = random_hex(12);
        peer.codes.insert(code.clone(), query.clone());
        let redirect = query
            .values
            .get("redirect_uri")
            .cloned()
            .unwrap_or_default();
        let state_text = query.values.get("state").cloned().unwrap_or_default();
        let issuer = peer.origin.clone();
        let location = format!(
            "{redirect}?{}",
            [
                ("code", code.as_str()),
                ("state", state_text.as_str()),
                ("iss", issuer.as_str()),
            ]
            .into_iter()
            .map(|(key, value)| format!("{key}={}", percent_encode(value)))
            .collect::<Vec<_>>()
            .join("&")
        );
        return http_reply(302, &[], &[("Location", &location)]);
    }
    if method == "POST" && path.starts_with("/model/") {
        return model_reply(peer, path, headers, body);
    }
    if method == "POST" && path == "/device" {
        let values = form_values(&body);
        peer.calls.push((
            "POST".into(),
            path.to_string(),
            headers.clone(),
            body.clone(),
        ));
        let code = random_hex(12);
        let scope = values
            .get("scope")
            .cloned()
            .unwrap_or_else(|| "storage".to_string());
        peer.devices.insert(code.clone(), scope);
        return json_reply(
            200,
            &obj(vec![
                ("device_code".into(), s(&code)),
                ("user_code".into(), s("OWNED-CODE")),
                ("verification_uri".into(), s(&format!("{}/verify", peer.origin))),
                ("expires_in".into(), n("120")),
                ("interval".into(), n("1")),
            ]),
        );
    }
    if method == "POST" && path == "/token" {
        let values = form_values(&body);
        peer.calls.push((
            "POST".into(),
            path.to_string(),
            headers.clone(),
            body.clone(),
        ));
        return token_reply(peer, &values);
    }
    if path.starts_with("/model/") {
        return model_reply(peer, path, headers, body);
    }
    storage_reply(peer, method, path, headers, body)
}

fn token_reply(peer: &mut PeerState, values: &HashMap<String, String>) -> Vec<u8> {
    let grant = values.get("grant_type").map(String::as_str).unwrap_or("");
    match grant {
        "authorization_code" => {
            let Some(config) = peer
                .codes
                .remove(values.get("code").map(String::as_str).unwrap_or(""))
            else {
                return json_reply(
                    400,
                    &obj(vec![("error".into(), s("invalid_grant"))]),
                );
            };
            let verifier = values
                .get("code_verifier")
                .cloned()
                .unwrap_or_default();
            // The leg recomputes the challenge with base64.urlsafe_b64encode(...).rstrip(b'='), so
            // the proof must use the URL-safe alphabet, not the standard one.
            let proof = twin::b64_encode(&twin::sha256_digest(verifier.as_bytes()))
                .trim_end_matches('=')
                .replace('+', "-")
                .replace('/', "_");
            let expected = config
                .values
                .get("code_challenge")
                .cloned()
                .unwrap_or_default();
            if proof != expected
                || values.get("redirect_uri") != config.values.get("redirect_uri")
                || values.get("client_id") != config.values.get("client_id")
            {
                return json_reply(
                    400,
                    &obj(vec![("error".into(), s("invalid_grant"))]),
                );
            }
            let scope = config
                .values
                .get("scope")
                .cloned()
                .unwrap_or_else(|| "storage".to_string());
            issue(peer, &scope)
        }
        "refresh_token" => {
            let token = values
                .get("refresh_token")
                .map(String::as_str)
                .unwrap_or("");
            let Some(scope) = peer.refreshes.remove(token) else {
                return json_reply(
                    400,
                    &obj(vec![("error".into(), s("invalid_grant"))]),
                );
            };
            issue(peer, &scope)
        }
        "urn:ietf:params:oauth:grant-type:device_code" => {
            let token = values
                .get("device_code")
                .map(String::as_str)
                .unwrap_or("");
            let Some(scope) = peer.devices.remove(token) else {
                return json_reply(
                    400,
                    &obj(vec![("error".into(), s("invalid_grant"))]),
                );
            };
            issue(peer, &scope)
        }
        _ => json_reply(
            400,
            &obj(vec![("error".into(), s("unsupported_grant_type"))]),
        ),
    }
}

fn issue(peer: &mut PeerState, scope: &str) -> Vec<u8> {
    // The leg monkeypatches this shared handler, so the replacement body answers
    // every grant route (authorization_code, refresh_token, device_code) and
    // registers no access or refresh token with the peer.
    if let Some(replacement) = peer.override_refresh.clone() {
        let mut fields = vec![
            ("token_type".to_string(), s("Bearer")),
            (
                "access_token".to_string(),
                s("replacement-access"),
            ),
            ("expires_in".to_string(), n("3600")),
        ];
        if let Some(token) = replacement {
            fields.push(("refresh_token".to_string(), token));
        }
        return json_reply(200, &Value::Obj(fields));
    }
    let access = format!("{scope}-access-{}", random_hex(8));
    let refresh = format!("{scope}-refresh-{}", random_hex(8));
    peer.accesses.insert(access.clone(), scope.to_string());
    peer.refreshes.insert(refresh.clone(), scope.to_string());
    json_reply(
        200,
        &obj(vec![
            ("token_type".into(), s("Bearer")),
            ("access_token".into(), s(&access)),
            ("refresh_token".into(), s(&refresh)),
            ("expires_in".into(), n("3600")),
        ]),
    )
}

fn model_reply(
    peer: &mut PeerState,
    path: &str,
    headers: &HashMap<String, String>,
    body: Vec<u8>,
) -> Vec<u8> {
    peer.calls.push((
        "MODEL".into(),
        path.to_string(),
        headers.clone(),
        body.clone(),
    ));
    let authorization = headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let scope = peer
        .accesses
        .get(
            authorization
                .strip_prefix("Bearer ")
                .unwrap_or(authorization),
        )
        .map(String::as_str);
    let allowed = matches!(authorization, "Bearer primary-key" | "Bearer child-key")
        || headers.get("x-api-key").map(String::as_str) == Some("primary-key")
        || headers.get("x-goog-api-key").map(String::as_str) == Some("primary-key")
        || scope == Some("conversation");
    if !allowed {
        return http_reply(401, &[], &[]);
    }
    let format = path.rsplit('/').next().unwrap_or("");
    let payload = match format {
        "chat" => obj(vec![(
            "choices".into(),
            Value::Arr(vec![obj(vec![
                (
                    "message".into(),
                    obj(vec![
                        ("role".into(), s("assistant")),
                        ("content".into(), s("Actual primary response.")),
                    ]),
                ),
                ("finish_reason".into(), s("stop")),
            ])]),
        )]),
        "local" => obj(vec![
            (
                "message".into(),
                obj(vec![("content".into(), s("Actual primary response."))]),
            ),
            ("done".into(), b(true)),
        ]),
        "messages" => obj(vec![
            ("role".into(), s("assistant")),
            (
                "content".into(),
                Value::Arr(vec![obj(vec![
                    ("type".into(), s("text")),
                    ("text".into(), s("Actual primary response.")),
                ])]),
            ),
            ("stop_reason".into(), s("end_turn")),
        ]),
        "content" => obj(vec![(
            "candidates".into(),
            Value::Arr(vec![obj(vec![(
                "content".into(),
                obj(vec![(
                    "parts".into(),
                    Value::Arr(vec![obj(vec![("text".into(), s("Actual primary response."))])]),
                )]),
            )])]),
        )]),
        _ => obj(vec![]),
    };
    json_reply(200, &payload)
}

fn storage_reply(
    peer: &mut PeerState,
    method: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: Vec<u8>,
) -> Vec<u8> {
    peer.calls.push((
        method.to_string(),
        path.to_string(),
        headers.clone(),
        body.clone(),
    ));
    let authorization = headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let scope = peer
        .accesses
        .get(
            authorization
                .strip_prefix("Bearer ")
                .unwrap_or(authorization),
        )
        .map(String::as_str);
    if scope != Some("storage")
        && headers.get("x-storage-key").map(String::as_str) != Some("Manual retained-key")
    {
        return http_reply(401, &[], &[]);
    }
    if method == "PROPFIND" {
        return http_reply(
            207,
            b"<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/share/alpha.txt</d:href><d:propstat><d:prop><d:displayname>alpha.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>",
            &[],
        );
    }
    if method == "PUT" {
        if headers.get("if-none-match").map(String::as_str) == Some("*")
            && peer.files.contains_key(path)
        {
            return http_reply(412, &[], &[]);
        }
        peer.files.insert(path.to_string(), body);
        return http_reply(201, &[], &[]);
    }
    if method == "MKCOL" {
        return http_reply(201, &[], &[]);
    }
    if let Some(content) = peer.files.get(path) {
        return http_reply(200, content, &[]);
    }
    http_reply(404, &[], &[])
}

fn form_values(body: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(body);
    let mut values = HashMap::new();
    for pair in text.split('&').filter(|part| !part.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key), percent_decode(value));
    }
    values
}

fn json_reply(status: u16, value: &Value) -> Vec<u8> {
    http_reply(
        status,
        value.to_string().as_bytes(),
        &[("Content-Type", "application/json")],
    )
}

fn http_reply(status: u16, body: &[u8], headers: &[(&str, &str)]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        207 => "Multi-Status",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        412 => "Precondition Failed",
        _ => "Response",
    };
    let mut response = format!("HTTP/1.1 {status} {reason}\r\n").into_bytes();
    for (key, value) in headers {
        response.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    response.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    response.extend_from_slice(b"Connection: close\r\n\r\n");
    response.extend_from_slice(body);
    response
}

fn random_hex(bytes: usize) -> String {
    let mut data = vec![0u8; bytes];
    let mut file = std::fs::File::open("/dev/urandom")
        .unwrap_or_else(|error| panic!("urandom: {error}"));
    file.read_exact(&mut data)
        .unwrap_or_else(|error| panic!("urandom: {error}"));
    data.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() {
    let root = std::env::current_dir()
        .unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let peer = Peer::start();
    let scratch = Scratch::create(&root);
    SCRATCH_ROOT
        .set(scratch.path().to_string())
        .unwrap_or_else(|_| panic!("scratch root set twice"));
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &peer);
    controller.finish();
    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value_printed("credential_rotation", &body, true);
    let written = std::fs::read_to_string(REPORT)
        .unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    let expected = twin::pretty(&body);
    if written != format!("{expected}\n") {
        panic!("{REPORT} has the wrong shape");
    }
    if passed.len() != 82 {
        panic!("expected 82 checks, got {}", passed.len());
    }
}

fn run(controller: &mut Controller, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();

    let setup = setup_storage(controller, peer);
    let authorized = authorize(controller, peer, &setup);
    let saved = tokens(&authorized, "drive").clone();

    for value in [
        Value::Num("42".into()),
        Value::Bool(true),
        Value::Arr(vec![]),
        Value::Obj(vec![]),
        Value::Arr(vec![s("rotated-secret")]),
    ] {
        peer.set_override(Some(value.clone()));
        let settings = event(
            controller,
            Some(&authorized),
            "storage_oauth_settings",
            Value::Null,
        );
        let fresh = authorize(controller, peer, &settings);
        keep(
            &mut passed,
            &format!(
                "{} malformed initial grant retains exact prior tokens",
                type_name(&value)
            ),
            tokens(&fresh, "drive") == &saved && notice_error(&fresh),
        );

        peer.allow_refresh(
            saved.get("refresh_token").map(Value::str_).unwrap_or(""),
            "storage",
        );
        let pending = event(
            controller,
            Some(&authorized),
            "oauth_refresh",
            Value::Null,
        );
        let done = complete(controller, peer, pending);
        let actual = tokens(&done, "drive").clone();
        let uncertain = actual.get("refresh_uncertain").is_some();
        let without_uncertain = remove_field(&actual, "refresh_uncertain");
        keep(
            &mut passed,
            &format!(
                "{} malformed refresh retains original tokens and uncertainty",
                type_name(&value)
            ),
            without_uncertain == saved && uncertain && notice_error(&done),
        );

        let again = event(controller, Some(&done), "oauth_refresh", Value::Null);
        keep(
            &mut passed,
            &format!(
                "uncertain rotation is never replayed {}",
                type_name(&value)
            ),
            again.get("effect") == Some(&Value::Null),
        );

        let manual = event(controller, Some(&done), "oauth_key_mode", Value::Null);
        let listed_event = event(
            controller,
            Some(&manual),
            "drive_list",
            Value::Null,
        );
        let listed = complete(controller, peer, listed_event);
        let count = listed
            .get("state")
            .and_then(|state| state.get("remote"))
            .map(Value::arr)
            .map(Vec::len)
            .unwrap_or(0);
        keep(
            &mut passed,
            &format!("manual storage remains available {}", type_name(&value)),
            count == 1,
        );
    }

    peer.clear_override();
    peer.allow_refresh(
        saved.get("refresh_token").map(Value::str_).unwrap_or(""),
        "storage",
    );
    let pending = event(
        controller,
        Some(&authorized),
        "oauth_refresh",
        Value::Null,
    );
    let renewed = complete(controller, peer, pending);
    let prior_refresh = tokens(&renewed, "drive")
        .get("refresh_token")
        .cloned();
    keep(
        &mut passed,
        "valid rotation still replaces refresh credential",
        prior_refresh != saved.get("refresh_token").cloned(),
    );

    peer.set_override(Some(Value::Null));
    let pending = event(
        controller,
        Some(&renewed),
        "oauth_refresh",
        Value::Null,
    );
    let omitted = complete(controller, peer, pending);
    keep(
        &mut passed,
        "null omission retains prior refresh compatibility",
        tokens(&omitted, "drive").get("refresh_token")
            == tokens(&renewed, "drive").get("refresh_token")
            && !notice_error(&omitted),
    );

    for format in ["chat", "local", "messages", "content"] {
        peer.clear_override();
        let model = setup_model(controller, peer, format);
        let prior = tokens_in(&model, "profiles", 0).clone();

        for value in [
            Value::Num("42".into()),
            Value::Bool(true),
            Value::Arr(vec![]),
            Value::Obj(vec![]),
            Value::Arr(vec![s("rotated-secret")]),
        ] {
            peer.set_override(Some(value.clone()));
            let settings = event(
                controller,
                Some(&model),
                "oauth_settings",
                Value::Null,
            );
            let fresh = authorize(controller, peer, &settings);
            keep(
                &mut passed,
                &format!(
                    "{format} malformed new grant preserves credentials {}",
                    type_name(&value)
                ),
                tokens_in(&fresh, "profiles", 0) == &prior && notice_error(&fresh),
            );

            peer.allow_refresh(
                prior.get("refresh_token").map(Value::str_).unwrap_or(""),
                "conversation",
            );
            let pending = event(
                controller,
                Some(&model),
                "oauth_refresh",
                Value::Null,
            );
            let done = complete(controller, peer, pending);
            let actual = tokens_in(&done, "profiles", 0).clone();
            let uncertain = actual.get("refresh_uncertain").is_some();
            let without_uncertain = remove_field(&actual, "refresh_uncertain");
            keep(
                &mut passed,
                &format!(
                    "{format} malformed rotation remains uncertain {}",
                    type_name(&value)
                ),
                without_uncertain == prior && uncertain && notice_error(&done),
            );

            let manual = event(
                controller,
                Some(&done),
                "oauth_key_mode",
                Value::Null,
            );
            let sent = event(
                controller,
                Some(&manual),
                "send",
                obj(vec![(
                    "draft".into(),
                    s("Continue with retained manual credentials."),
                )]),
            );
            let answered = complete(controller, peer, sent);
            let chats = answered
                .get("state")
                .and_then(|state| state.get("chats"))
                .map(Value::to_string)
                .unwrap_or_default();
            keep(
                &mut passed,
                &format!(
                    "{format} scoped failure preserves primary request {}",
                    type_name(&value)
                ),
                chats.contains("Actual primary response."),
            );
        }
    }

    passed
}

fn setup_storage(controller: &mut Controller, peer: &Peer) -> Value {
    let resumed = event(controller, None, "resume", Value::Null);
    let edited = event(controller, Some(&resumed), "drive_edit", Value::Null);
    let saved = event(
        controller,
        Some(&edited),
        "drive_save",
        obj(vec![
            ("url".into(), s(&format!("{}/share/", peer.origin))),
            ("plain".into(), b(true)),
            ("user".into(), s("retained-user")),
            ("password".into(), s("retained-password")),
            ("key".into(), s("retained-key")),
            ("auth_header".into(), s("X-Storage-Key")),
            ("auth_prefix".into(), s("Manual")),
        ]),
    );
    let settings = event(
        controller,
        Some(&saved),
        "storage_oauth_settings",
        Value::Null,
    );
    event(
        controller,
        Some(&settings),
        "oauth_save",
        obj(vec![
            (
                "authorization_endpoint".into(),
                s(&format!("{}/authorize", peer.origin)),
            ),
            (
                "device_endpoint".into(),
                s(&format!("{}/device", peer.origin)),
            ),
            (
                "token_endpoint".into(),
                s(&format!("{}/token", peer.origin)),
            ),
            ("client_id".into(), s("public-client")),
            ("scope".into(), s("storage")),
            ("issuer".into(), s(&peer.origin)),
            ("plain".into(), b(true)),
        ]),
    )
}

fn setup_model(controller: &mut Controller, peer: &Peer, format: &str) -> Value {
    let resumed = event(controller, None, "resume", Value::Null);
    let profile = event(controller, Some(&resumed), "profile_new", Value::Null);
    let saved = event(
        controller,
        Some(&profile),
        "profile_save",
        obj(vec![
            ("name".into(), s("Rotation peer")),
            ("protocol".into(), s(format)),
            (
                "endpoint".into(),
                s(&format!("{}/model/{format}", peer.origin)),
            ),
            ("model".into(), s("controlled")),
            ("key".into(), s("primary-key")),
            ("plain".into(), b(true)),
            ("stream".into(), b(false)),
        ]),
    );
    let settings = event(controller, Some(&saved), "oauth_settings", Value::Null);
    let configured = event(
        controller,
        Some(&settings),
        "oauth_save",
        obj(vec![
            (
                "authorization_endpoint".into(),
                s(&format!("{}/authorize", peer.origin)),
            ),
            (
                "token_endpoint".into(),
                s(&format!("{}/token", peer.origin)),
            ),
            ("client_id".into(), s("public-client")),
            ("scope".into(), s("conversation")),
            ("issuer".into(), s(&peer.origin)),
            ("plain".into(), b(true)),
        ]),
    );
    authorize(controller, peer, &configured)
}

fn authorize(controller: &mut Controller, peer: &Peer, state: &Value) -> Value {
    let started = event(controller, Some(state), "oauth_sign_in", Value::Null);
    let effect = started
        .get("effect")
        .unwrap_or_else(|| panic!("oauth browser did not start: {}", started.to_string()));
    let kind = effect.get("kind").map(Value::str_).unwrap_or("");
    if kind != "oauth_browser" {
        panic!("oauth browser did not start: {}", started.to_string());
    }
    let url = effect
        .get("url")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("oauth browser has no URL"));
    let path_and_query = url
        .split_once("://")
        .map(|(_, rest)| {
            rest.split_once('/')
                .map(|(_, tail)| tail)
                .unwrap_or("")
        })
        .unwrap_or(url);
    let (base, query) = split_target(path_and_query);
    let response = http_get(peer, base.trim_start_matches('/'), &query.values);
    let location = response
        .get("location")
        .cloned()
        .unwrap_or_else(|| panic!("authorization did not redirect"));
    let returned = event(
        controller,
        Some(&started),
        "oauth_return",
        obj(vec![("uri".into(), s(&location))]),
    );
    complete(controller, peer, returned)
}

fn http_get(
    peer: &Peer,
    path: &str,
    query: &HashMap<String, String>,
) -> HashMap<String, String> {
    let address = peer.origin.trim_start_matches("http://");
    let (host, port) = address.split_once(':').unwrap_or((address, "80"));
    let port = port
        .parse::<u16>()
        .unwrap_or_else(|error| panic!("peer port: {error}"));
    let mut target = path.to_string();
    if !query.is_empty() {
        target.push('?');
        let mut first = true;
        for (key, value) in query {
            if !first {
                target.push('&');
            }
            first = false;
            target.push_str(&percent_encode(key));
            target.push('=');
            target.push_str(&percent_encode(value));
        }
    }
    let request = format!(
        "GET /{target} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    );
    let mut stream = TcpStream::connect((host, port))
        .unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("timeout: {error}"));
    stream
        .write_all(request.as_bytes())
        .unwrap_or_else(|error| panic!("write: {error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("read: {error}"));
    let text = String::from_utf8_lossy(&response);
    let mut lines = text.split("\r\n");
    let status = lines.next().unwrap_or("");
    if !status.contains(" 302") {
        panic!("authorization did not redirect: {status}");
    }
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    headers
}

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => out.push(*byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn event(
    controller: &mut Controller,
    envelope: Option<&Value>,
    action: &str,
    inputs: Value,
) -> Value {
    let state = envelope.map(|value| value.get("state").unwrap_or(value));
    let inputs = match inputs {
        Value::Null => obj(vec![]),
        other => other,
    };
    controller.event(
        state,
        action,
        inputs,
        SCRATCH_ROOT.get().expect("scratch root set in main"),
    )
}

fn complete(controller: &mut Controller, _peer: &Peer, value: Value) -> Value {
    let effect = value
        .get("effect")
        .unwrap_or_else(|| panic!("operation had no effect: {}", value.to_string()));
    let kind = effect.get("kind").map(Value::str_).unwrap_or("");
    if kind != "network" {
        panic!("operation was not a network request: {}", value.to_string());
    }
    let mut inputs = Value::Obj(Vec::new());
    for key in [
        "purpose",
        "drive_ticket",
        "model_ticket",
        "child_ticket",
        "child_owner",
    ] {
        if let Some(field) = effect.get(key) {
            inputs = put(inputs, key, field.clone());
        }
    }
    let wire = match (effect.get("wire_base64"), effect.get("wire")) {
        (Some(Value::Str(text)), _) => twin::b64_decode(text),
        (_, Some(Value::Str(text))) => text.as_bytes().to_vec(),
        (_, Some(Value::Arr(items))) => items
            .iter()
            .map(|item| item.str_().as_bytes().to_vec())
            .flatten()
            .collect(),
        _ => panic!("network effect had no wire data: {}", effect.to_string()),
    };
    let host = effect
        .get("host")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("network effect had no host"));
    let port = effect
        .get("port")
        .map(|value| match value {
            Value::Num(text) => text.parse::<u16>().unwrap_or(0),
            Value::Str(text) => text.parse::<u16>().unwrap_or(0),
            _ => 0,
        })
        .unwrap_or_else(|| panic!("network effect had no port"));
    let response = exchange(host, port, &wire);
    inputs = put(inputs, "data", s(&twin::b64_encode(&response)));
    event(
        controller,
        Some(value.get("state").unwrap_or(&Value::Null)),
        "network_result",
        inputs,
    )
}

fn exchange(host: &str, port: u16, wire: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect((host, port))
        .unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("timeout: {error}"));
    stream
        .write_all(wire)
        .unwrap_or_else(|error| panic!("write: {error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("read: {error}"));
    response
}

fn tokens<'a>(value: &'a Value, section: &str) -> &'a Value {
    value
        .get("state")
        .and_then(|state| state.get(section))
        .and_then(|owner| owner.get("oauth_tokens"))
        .unwrap_or_else(|| panic!("missing drive {section} tokens: {}", value.to_string()))
}

fn tokens_in<'a>(value: &'a Value, section: &str, index: usize) -> &'a Value {
    value
        .get("state")
        .and_then(|state| state.get(section))
        .map(Value::arr)
        .and_then(|items| items.get(index))
        .and_then(|profile| profile.get("oauth_tokens"))
        .unwrap_or_else(|| panic!("missing profile tokens: {}", value.to_string()))
}

fn notice_error(value: &Value) -> bool {
    value
        .get("state")
        .and_then(|state| state.get("notice_error"))
        .is_some_and(twin::truthy)
}

fn type_name(value: &Value) -> String {
    match value {
        Value::Null => "<class 'NoneType'>".to_string(),
        Value::Bool(_) => "<class 'bool'>".to_string(),
        Value::Num(_) => "<class 'int'>".to_string(),
        Value::Str(_) => "<class 'str'>".to_string(),
        Value::Arr(_) => "<class 'list'>".to_string(),
        Value::Obj(_) => "<class 'dict'>".to_string(),
    }
}

fn remove_field(value: &Value, key: &str) -> Value {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    obj(
        fields
            .iter()
            .filter(|(name, _)| name != key)
            .cloned()
            .collect(),
    )
}

fn put(value: Value, key: &str, next: Value) -> Value {
    let Value::Obj(mut fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
    Value::Obj(fields)
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = root.join("output").join(format!(
            "storage-oauth-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("could not create {}: {error}", path.display()));
        Self(path)
    }

    fn path(&self) -> &str {
        self.0
            .to_str()
            .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

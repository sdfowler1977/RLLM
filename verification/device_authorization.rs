mod twin;

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use twin::{b, b64_decode, b64_encode, create_scratch, n, obj, pretty, report_value, s, truthy, Controller, Value};

const SCOPE: &str = "Owner execution through actual local HTTP device/token/model peers and shared event interface across four formats. Explicit rate-limited status checks; no autonomous polling or physical Android/browser/live server acceptance claimed.";
const FORMATS: [&str; 4] = ["chat", "local", "messages", "content"];

#[derive(Clone)]
struct PeerOptions {
    status: u16,
    response: Vec<(String, Value)>,
    error: String,
    token_type: String,
}

impl Default for PeerOptions {
    fn default() -> Self {
        Self {
            status: 200,
            response: Vec::new(),
            error: "authorization_pending".to_string(),
            token_type: "Bearer".to_string(),
        }
    }
}

struct Shared {
    origin: String,
    options: Mutex<PeerOptions>,
    sessions: Mutex<Vec<String>>,
    refreshes: Mutex<Vec<String>>,
    failure: Mutex<Option<String>>,
}

struct Peer {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind peer: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")).port();
        let shared = Arc::new(Shared {
            origin: format!("http://127.0.0.1:{port}"),
            options: Mutex::new(PeerOptions::default()),
            sessions: Mutex::new(Vec::new()),
            refreshes: Mutex::new(Vec::new()),
            failure: Mutex::new(None),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread_shared = Arc::clone(&shared);
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || accept_loop(listener, thread_shared, thread_stop));
        Self { shared, stop, thread: Some(thread) }
    }

    fn origin(&self) -> &str {
        &self.shared.origin
    }

    fn reset(&self) {
        *self.lock_options() = PeerOptions::default();
    }

    fn set_error(&self, error: &str) {
        self.lock_options().error = error.to_string();
    }

    fn set_response(&self, response: Vec<(String, Value)>) {
        self.lock_options().response = response;
    }

    fn last_session(&self) -> String {
        self.shared
            .sessions
            .lock()
            .unwrap_or_else(|error| panic!("sessions lock: {error}"))
            .last()
            .cloned()
            .unwrap_or_else(|| panic!("no device session"))
    }

    fn take_failure(&self) -> Option<String> {
        self.shared.failure.lock().unwrap_or_else(|error| panic!("failure lock: {error}")).take()
    }

    fn lock_options(&self) -> std::sync::MutexGuard<'_, PeerOptions> {
        self.shared.options.lock().unwrap_or_else(|error| panic!("options lock: {error}"))
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    listener.set_nonblocking(true).unwrap_or_else(|error| panic!("peer nonblocking: {error}"));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => serve(&shared, stream),
            Err(error) if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::Interrupted => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("peer accept: {error}"),
        }
    }
}

fn serve(shared: &Shared, mut stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let outcome = catch_unwind(AssertUnwindSafe(|| handle(shared, &mut stream)));
    if let Err(payload) = outcome {
        *shared.failure.lock().unwrap_or_else(|error| panic!("failure lock: {error}")) = Some(panic_message(payload));
    }
    let _ = stream.shutdown(Shutdown::Both);
}

fn handle(shared: &Shared, stream: &mut TcpStream) {
    let (path, headers, body) = read_http(stream);
    let (status, payload) = respond(shared, &path, &headers, &body);
    let bytes = http_bytes(status, &payload);
    stream.write_all(&bytes).unwrap_or_else(|error| panic!("peer write: {error}"));
    stream.flush().unwrap_or_else(|error| panic!("peer flush: {error}"));
}

fn respond(shared: &Shared, path: &str, headers: &[(String, String)], body: &str) -> (u16, String) {
    let options = shared.options.lock().unwrap_or_else(|error| panic!("options lock: {error}")).clone();
    if path == "/device" || path == "/token" {
        if header(headers, "authorization").is_some() {
            panic!("{path} request included Authorization");
        }
        if form_value(body, "client_id") != "public-device" {
            panic!("{path} client_id was not the public client");
        }
    }
    if path == "/device" {
        return (options.status, device_payload(shared, &options));
    }
    if path == "/token" {
        return token_payload(shared, &options, body);
    }
    let format = path.rsplit('/').next().unwrap_or("");
    let authorization = header(headers, "authorization").unwrap_or("");
    if !authorization.starts_with("Bearer ") {
        panic!("model request missing bearer: {path}");
    }
    (200, model_payload(format))
}

fn device_payload(shared: &Shared, options: &PeerOptions) -> String {
    let code = token();
    shared.sessions.lock().unwrap_or_else(|error| panic!("sessions lock: {error}")).push(code.clone());
    let mut value = obj(vec![
        ("device_code".into(), s(&code)),
        ("user_code".into(), s("CODE-7392")),
        ("verification_uri".into(), s(&format!("{}/verify", shared.origin))),
        ("expires_in".into(), n("120")),
        ("interval".into(), n("1")),
    ]);
    for (key, item) in &options.response {
        put(&mut value, key, item.clone());
    }
    value.to_string()
}

fn token_payload(shared: &Shared, options: &PeerOptions, body: &str) -> (u16, String) {
    let grant = form_value(body, "grant_type");
    if grant == "urn:ietf:params:oauth:grant-type:device_code" {
        let code = form_value(body, "device_code");
        let known = shared
            .sessions
            .lock()
            .unwrap_or_else(|error| panic!("sessions lock: {error}"))
            .iter()
            .any(|item| item == &code);
        if !known {
            panic!("unknown device code");
        }
        if !options.error.is_empty() {
            return (
                400,
                obj(vec![
                    ("error".into(), s(&options.error)),
                    ("error_description".into(), s("PRIVATE_SERVER_DETAIL")),
                ])
                .to_string(),
            );
        }
    } else if grant == "refresh_token" {
        let refresh = form_value(body, "refresh_token");
        let mut refreshes = shared.refreshes.lock().unwrap_or_else(|error| panic!("refresh lock: {error}"));
        let Some(index) = refreshes.iter().position(|item| item == &refresh) else {
            panic!("unknown refresh token");
        };
        refreshes.remove(index);
    } else {
        panic!("unexpected grant {grant}");
    }
    let access = token();
    let refresh = token();
    shared.refreshes.lock().unwrap_or_else(|error| panic!("refresh lock: {error}")).push(refresh.clone());
    (
        200,
        obj(vec![
            ("token_type".into(), s(&options.token_type)),
            ("access_token".into(), s(&access)),
            ("refresh_token".into(), s(&refresh)),
            ("expires_in".into(), n("3600")),
        ])
        .to_string(),
    )
}

fn model_payload(format: &str) -> String {
    let text = "Device-authorized response.";
    match format {
        "chat" => obj(vec![(
            "choices".into(),
            Value::Arr(vec![obj(vec![
                ("message".into(), obj(vec![("content".into(), s(text))])),
                ("finish_reason".into(), s("stop")),
            ])]),
        )]),
        "local" => obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("done".into(), b(true)),
        ]),
        "messages" => obj(vec![
            ("role".into(), s("assistant")),
            (
                "content".into(),
                Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])]),
            ),
            ("stop_reason".into(), s("end_turn")),
        ]),
        "content" => obj(vec![(
            "candidates".into(),
            Value::Arr(vec![obj(vec![
                (
                    "content".into(),
                    obj(vec![(
                        "parts".into(),
                        Value::Arr(vec![obj(vec![("text".into(), s(text))])]),
                    )]),
                ),
                ("finishReason".into(), s("STOP")),
            ])]),
        )]),
        _ => panic!("unexpected model format {format}"),
    }
    .to_string()
}

fn http_bytes(status: u16, body: &str) -> Vec<u8> {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&project);
    let _ = std::fs::remove_file("output/checks/device_authorization.json");
    let peer = Peer::start();
    let passed = run(&project_text, &peer, scratch.path());
    let count = passed.len().to_string();
    let checks = passed.iter().map(|name| s(name)).collect::<Vec<_>>();
    let body = obj(vec![
        ("passed".into(), n(&count)),
        ("checks".into(), Value::Arr(checks)),
        ("scope".into(), s(SCOPE)),
    ]);
    report_value("device_authorization", &body, true);
    println!("{}", pretty(&body));
}

fn run(project: &str, peer: &Peer, root: &str) -> Vec<String> {
    let mut controller = Controller::spawn(project);
    let mut passed = Vec::new();
    for format in FORMATS {
        let saved = setup(&mut controller, peer, root, format, &[]);
        keep(
            &mut passed,
            &format!("{format} accepts a device-only saved public-client configuration"),
            !has_notice_error(&saved),
        );
        keep(
            &mut passed,
            &format!("{format} native authorization form exposes device method"),
            ui_contains(&saved, "oauth_device_start"),
        );
        let (waiting, initial) = start(&mut controller, peer, root, &saved);
        let code = peer.last_session();
        let verify = format!("{}/verify", peer.origin());
        keep(
            &mut passed,
            &format!("{format} private device credential remains out of UI and saved state"),
            !waiting.to_string().contains(&code) && dialog_eq(&waiting, "oauth_device") && effect_null(&waiting),
        );
        keep(
            &mut passed,
            &format!("{format} actual user code and verification address are displayed"),
            ui_contains(&waiting, "CODE-7392") && ui_contains(&waiting, &verify),
        );
        let immediate = event(&mut controller, Some(state_of(&waiting)), "oauth_device_poll", blank(), root);
        keep(
            &mut passed,
            &format!("{format} early polling emits no network request"),
            effect_null(&immediate) && not_busy(&immediate),
        );
        let opened = event(&mut controller, Some(state_of(&waiting)), "oauth_device_open", blank(), root);
        keep(
            &mut passed,
            &format!("{format} browser opens only by explicit action"),
            browser_effect(&opened, &verify),
        );
        let recovered = event(&mut controller, Some(state_of(&opened)), "resume", blank(), root);
        keep(
            &mut passed,
            &format!("{format} same-process activity return preserves waiting flow without network"),
            effect_null(&recovered) && dialog_eq(&recovered, "oauth_device"),
        );
        let pending = poll(&mut controller, state_of(&recovered), 1.05, root);
        let late = event(&mut controller, Some(state_of(&pending)), "network_result", initial, root);
        keep(
            &mut passed,
            &format!("{format} previous phase callback cannot satisfy a new poll"),
            states_equal(&late, &pending) && effect_null(&late),
        );
        let (waiting, pending_result) = exchange(&mut controller, peer, root, &pending);
        keep(
            &mut passed,
            &format!("{format} actual pending response preserves user instructions"),
            effect_null(&waiting) && dialog_eq(&waiting, "oauth_device") && not_busy(&waiting),
        );
        peer.set_error("");
        let pending = poll(&mut controller, state_of(&waiting), 1.05, root);
        let late = event(&mut controller, Some(state_of(&pending)), "network_result", pending_result, root);
        keep(
            &mut passed,
            &format!("{format} prior poll callback cannot satisfy the next request"),
            states_equal(&late, &pending) && effect_null(&late),
        );
        let (done, callback) = exchange(&mut controller, peer, root, &pending);
        let granted = profile(&done).clone();
        keep(
            &mut passed,
            &format!("{format} actual grant adopts bearer and retains key"),
            adopted(&granted),
        );
        let duplicate = event(&mut controller, Some(state_of(&done)), "network_result", callback, root);
        keep(
            &mut passed,
            &format!("{format} successful grant callback remains single-use"),
            states_equal(&duplicate, &done) && effect_null(&duplicate),
        );
        let sent = event(
            &mut controller,
            Some(state_of(&done)),
            "send",
            obj(vec![("draft".into(), s("Use the device-authorized model."))]),
            root,
        );
        let access = access_token(&granted);
        keep(
            &mut passed,
            &format!("{format} grant reaches original configured model"),
            !access.is_empty() && wire_has_bearer(&sent, &access),
        );
        let (answered, _) = exchange(&mut controller, peer, root, &sent);
        keep(
            &mut passed,
            &format!("{format} actual model response completes normally"),
            not_busy(&answered),
        );
        let refreshed = event(&mut controller, Some(state_of(&answered)), "oauth_refresh", blank(), root);
        let (done, _) = exchange(&mut controller, peer, root, &refreshed);
        let old_refresh = refresh_token(&granted);
        let new_refresh = refresh_token(profile(&done));
        keep(
            &mut passed,
            &format!("{format} device grant uses existing atomic refresh lifecycle"),
            matches!((old_refresh, new_refresh), (Some(old), Some(new)) if old != new),
        );
        let exported = event(&mut controller, Some(state_of(&done)), "configuration_export", blank(), root);
        let secrets = [
            access_token(profile(&done)),
            refresh_token(profile(&done)).unwrap_or_default(),
            "retained-api-key".to_string(),
            code,
        ];
        keep(
            &mut passed,
            &format!("{format} export excludes credentials"),
            export_hides(&exported, &secrets),
        );
    }

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    let (waiting, _) = start(&mut controller, peer, root, &saved);
    peer.set_error("slow_down");
    let polled = poll(&mut controller, state_of(&waiting), 1.05, root);
    let (waiting, _) = exchange(&mut controller, peer, root, &polled);
    thread::sleep(Duration::from_secs_f64(1.05));
    let early = event(&mut controller, Some(state_of(&waiting)), "oauth_device_poll", blank(), root);
    keep(
        &mut passed,
        "slow_down permanently adds five seconds before another poll",
        effect_null(&early) && notice_has(&early, "Wait"),
    );
    peer.set_error("");
    let polled = poll(&mut controller, state_of(&waiting), 5.05, root);
    let (done, _) = exchange(&mut controller, peer, root, &polled);
    keep(&mut passed, "poll succeeds after increased interval", profile(&done).get("oauth_enabled").is_some_and(truthy));

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    peer.set_response(vec![("interval".into(), Value::Null)]);
    let (waiting, _) = start(&mut controller, peer, root, &saved);
    keep(&mut passed, "null interval is treated as omitted default", dialog_eq(&waiting, "oauth_device"));
    let early = event(&mut controller, Some(state_of(&waiting)), "oauth_device_poll", blank(), root);
    keep(&mut passed, "default five-second interval is enforced", effect_null(&early));
    event(&mut controller, Some(state_of(&waiting)), "oauth_cancel", blank(), root);

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    peer.set_response(vec![("expires_in".into(), n("1"))]);
    let (waiting, _) = start(&mut controller, peer, root, &saved);
    thread::sleep(Duration::from_secs_f64(1.05));
    let retired = event(&mut controller, Some(state_of(&waiting)), "status", blank(), root);
    let late = event(&mut controller, Some(state_of(&retired)), "oauth_device_poll", blank(), root);
    keep(
        &mut passed,
        "expired local grant cannot send another request",
        effect_null(&late) && not_busy(&late),
    );

    for error in ["access_denied", "expired_token", "invalid_client", "unknown_error"] {
        let saved = setup(&mut controller, peer, root, "chat", &[]);
        let (waiting, _) = start(&mut controller, peer, root, &saved);
        peer.set_error(error);
        let polled = poll(&mut controller, state_of(&waiting), 1.05, root);
        let (done, _) = exchange(&mut controller, peer, root, &polled);
        keep(
            &mut passed,
            &format!("{error} retires the flow without exposing server details"),
            effect_null(&done) && has_notice_error(&done) && notice_lacks(&done, "PRIVATE_SERVER_DETAIL"),
        );
        keep(
            &mut passed,
            &format!("{error} leaves existing API key available"),
            profile(&done).get("key") == Some(&s("retained-api-key")),
        );
    }

    let origin = peer.origin().to_string();
    let cases = [
        ("empty code", vec![("device_code".into(), s(""))]),
        ("control code", vec![("user_code".into(), s("bad\ncode"))]),
        ("non-http URI", vec![("verification_uri".into(), s("file:///private"))]),
        ("private URI fields", vec![("verification_uri".into(), s(&format!("{origin}/verify?device_code=secret")))]),
        ("negative lifetime", vec![("expires_in".into(), n("-1"))]),
        ("string timing", vec![("interval".into(), s("1"))]),
        ("zero interval", vec![("interval".into(), n("0"))]),
        ("large timing", vec![("expires_in".into(), n("86401"))]),
    ];
    for (label, response) in cases {
        let saved = setup(&mut controller, peer, root, "chat", &[]);
        peer.set_response(response);
        let (done, _) = start(&mut controller, peer, root, &saved);
        keep(
            &mut passed,
            &format!("{label} is refused without adopting credentials"),
            effect_null(&done) && has_notice_error(&done) && !profile(&done).get("oauth_enabled").is_some_and(truthy),
        );
    }

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    let (waiting, initial) = start(&mut controller, peer, root, &saved);
    let cancelled = event(&mut controller, Some(state_of(&waiting)), "oauth_cancel", blank(), root);
    let late = event(&mut controller, Some(state_of(&cancelled)), "network_result", initial, root);
    keep(
        &mut passed,
        "cancel retains key and rejects late response exactly",
        states_equal(&late, &cancelled) && effect_null(&late) && profile(&cancelled).get("key") == Some(&s("retained-api-key")),
    );

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    let started = event(&mut controller, Some(state_of(&saved)), "oauth_device_start", blank(), root);
    let cancelled = event(&mut controller, Some(state_of(&started)), "oauth_cancel", blank(), root);
    let (_done, callback) = exchange(&mut controller, peer, root, &started);
    let late = event(&mut controller, Some(state_of(&cancelled)), "network_result", callback, root);
    keep(
        &mut passed,
        "cancelled in-flight initialization cannot adopt or reopen code",
        states_equal(&late, &cancelled) && effect_null(&late),
    );

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    let (waiting, _) = start(&mut controller, peer, root, &saved);
    let request = poll(&mut controller, state_of(&waiting), 1.05, root);
    let purpose = request
        .get("effect")
        .and_then(|effect| effect.get("purpose"))
        .cloned()
        .filter(|value| value != &Value::Null)
        .unwrap_or_else(|| panic!("uncertain token exchange retires without replay"));
    let failed = event(
        &mut controller,
        Some(state_of(&request)),
        "failure",
        obj(vec![
            ("purpose".into(), purpose),
            ("sent".into(), b(true)),
            ("error".into(), s("controlled uncertain result")),
        ]),
        root,
    );
    let again = event(&mut controller, Some(state_of(&failed)), "oauth_device_poll", blank(), root);
    keep(
        &mut passed,
        "uncertain token exchange retires without replay",
        effect_null(&again) && not_busy(&again),
    );

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    let (waiting, _) = start(&mut controller, peer, root, &saved);
    let mut changed = state_of(&waiting).clone();
    set_endpoint(&mut changed, &format!("{}/other", peer.origin()));
    let refused = event(&mut controller, Some(&changed), "oauth_device_poll", blank(), root);
    keep(
        &mut passed,
        "changed model destination cannot use held device code",
        effect_null(&refused) && not_busy(&refused),
    );

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    let refused = event(
        &mut controller,
        Some(state_of(&saved)),
        "oauth_device_start",
        obj(vec![("device_endpoint".into(), s(&format!("{}/unsaved", peer.origin())))]),
        root,
    );
    keep(
        &mut passed,
        "unsaved device endpoint cannot receive a request",
        effect_null(&refused) && has_notice_error(&refused),
    );

    let saved = setup(
        &mut controller,
        peer,
        root,
        "chat",
        &[("device_endpoint", s(&format!("{}/device?device_code=override", peer.origin())))],
    );
    keep(
        &mut passed,
        "configured endpoints cannot override protocol parameters",
        has_notice_error(&saved),
    );

    let saved = setup(&mut controller, peer, root, "chat", &[]);
    let (waiting, _) = start(&mut controller, peer, root, &saved);
    let saved_state = state_of(&waiting).clone();
    controller.finish();
    let mut controller = Controller::spawn(project);
    let resumed = event(&mut controller, Some(&saved_state), "resume", blank(), root);
    keep(
        &mut passed,
        "new process never reconstructs or replays a private device code",
        effect_null(&resumed) && dialog_eq(&resumed, "oauth_settings") && !state_of(&resumed).get("oauth_expected").is_some_and(truthy),
    );
    controller.finish();
    passed
}

fn setup(controller: &mut Controller, peer: &Peer, root: &str, format: &str, overrides: &[(&str, Value)]) -> Value {
    peer.reset();
    let resumed = event(controller, None, "resume", blank(), root);
    let created = event(controller, Some(state_of(&resumed)), "profile_new", blank(), root);
    let saved = event(
        controller,
        Some(state_of(&created)),
        "profile_save",
        obj(vec![
            ("name".into(), s("Private model")),
            ("protocol".into(), s(format)),
            ("endpoint".into(), s(&format!("{}/model/{format}", peer.origin()))),
            ("model".into(), s("controlled")),
            ("key".into(), s("retained-api-key")),
            ("stream".into(), b(false)),
        ]),
        root,
    );
    let oauth = event(controller, Some(state_of(&saved)), "oauth_settings", blank(), root);
    let mut config = vec![
        ("device_endpoint".into(), s(&format!("{}/device", peer.origin()))),
        ("token_endpoint".into(), s(&format!("{}/token", peer.origin()))),
        ("client_id".into(), s("public-device")),
        ("scope".into(), s("model offline")),
        ("plain".into(), b(true)),
    ];
    for (key, value) in overrides {
        if let Some((_, slot)) = config.iter_mut().find(|(name, _)| name == key) {
            *slot = value.clone();
        } else {
            config.push(((*key).to_string(), value.clone()));
        }
    }
    event(controller, Some(state_of(&oauth)), "oauth_save", obj(config), root)
}

fn start(controller: &mut Controller, peer: &Peer, root: &str, saved: &Value) -> (Value, Value) {
    let started = event(controller, Some(state_of(saved)), "oauth_device_start", blank(), root);
    exchange(controller, peer, root, &started)
}

fn poll(controller: &mut Controller, state: &Value, delay: f64, root: &str) -> Value {
    thread::sleep(Duration::from_secs_f64(delay));
    event(controller, Some(state), "oauth_device_poll", blank(), root)
}

fn exchange(controller: &mut Controller, peer: &Peer, root: &str, request: &Value) -> (Value, Value) {
    if let Some(message) = peer.take_failure() {
        panic!("peer: {message}");
    }
    let effect = network_effect(request);
    let mut stream = connect(effect);
    stream.write_all(&wire_bytes(effect)).unwrap_or_else(|error| panic!("peer write: {error}"));
    upload(controller, effect, &mut stream);
    let mut fields = response(controller, effect, &mut stream);
    if let Some(message) = peer.take_failure() {
        panic!("peer: {message}");
    }
    put(&mut fields, "purpose", effect.get("purpose").cloned().unwrap_or_else(|| panic!("missing purpose")));
    for key in ["model_ticket", "child_ticket", "child_owner"] {
        if let Some(value) = effect.get(key) {
            put(&mut fields, key, value.clone());
        }
    }
    let state = state_of(request).clone();
    let result = event(controller, Some(&state), "network_result", fields.clone(), root);
    (result, fields)
}

fn upload(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) {
    let Some(ticket) = nonempty(effect.get("media_upload")) else { return };
    let purpose = require_str(effect, "purpose");
    let limit = as_usize(effect.get("upload_bytes").unwrap_or_else(|| panic!("missing upload_bytes")));
    let owner = obj(vec![("purpose".into(), s(&purpose)), ("ticket".into(), s(&ticket))]);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut count = 0usize;
        loop {
            let mut envelope = owner.clone();
            put(&mut envelope, "op", s("media_upload_read"));
            let part = controller.op(&envelope);
            let data = b64_decode(require_str(&part, "data").as_str());
            if data.len() > 8192 {
                panic!("upload chunk exceeds 8192");
            }
            count += data.len();
            if count > limit {
                panic!("upload exceeded declared length");
            }
            stream.write_all(&data).unwrap_or_else(|error| panic!("upload write: {error}"));
            if flag(&part, "done") == Some(true) {
                if count != limit {
                    panic!("upload length mismatch {count} != {limit}");
                }
                return;
            }
            if data.is_empty() {
                panic!("upload returned an empty chunk");
            }
        }
    }));
    cancel_op(controller, &owner, "media_upload_cancel");
    if let Err(error) = outcome {
        resume_unwind(error);
    }
}

fn response(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) -> Value {
    let Some(ticket) = nonempty(effect.get("media_download")) else {
        return obj(vec![("data".into(), s(&b64_encode(&read_all(stream))))]);
    };
    let purpose = require_str(effect, "purpose");
    let owner = obj(vec![("purpose".into(), s(&purpose)), ("ticket".into(), s(&ticket))]);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut ready = false;
        loop {
            let chunk = read_chunk(stream);
            if chunk.is_empty() {
                break;
            }
            let mut envelope = owner.clone();
            put(&mut envelope, "op", s("media_stream_feed"));
            put(&mut envelope, "data", s(&b64_encode(&chunk)));
            let value = controller.op(&envelope);
            ready = flag(&value, "ready") == Some(true);
            if ready {
                break;
            }
        }
        let mut envelope = owner.clone();
        put(&mut envelope, "op", s("media_stream_finish"));
        put(&mut envelope, "eof", b(!ready));
        obj(vec![("media_receipt".into(), controller.op(&envelope))])
    }));
    cancel_op(controller, &owner, "media_stream_cancel");
    match outcome {
        Ok(value) => value,
        Err(error) => resume_unwind(error),
    }
}

fn cancel_op(controller: &mut Controller, owner: &Value, name: &str) {
    let mut envelope = owner.clone();
    put(&mut envelope, "op", s(name));
    let _ = controller.op(&envelope);
}

fn connect(effect: &Value) -> TcpStream {
    let host = require_str(effect, "host");
    let port = as_u16(effect.get("port").unwrap_or_else(|| panic!("missing port")));
    let stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("peer connect {host}:{port}: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("peer timeout: {error}"));
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("peer timeout: {error}"));
    let _ = stream.set_nodelay(true);
    stream
}

fn network_effect(request: &Value) -> &Value {
    match request.get("effect") {
        Some(effect) if !matches!(effect, Value::Null) && field_str(effect, "kind") == Some("network") => effect,
        _ => panic!(
            "expected network effect ({})",
            state_of(request).get("notice").map(Value::to_string).unwrap_or_else(|| "no notice".into())
        ),
    }
}

fn read_http(stream: &mut TcpStream) -> (String, Vec<(String, String)>, String) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let split = loop {
        if let Some(end) = header_end(&buffer) {
            break end;
        }
        let count = read_some(stream, &mut chunk);
        if count == 0 {
            panic!("peer request closed before headers");
        }
        buffer.extend_from_slice(&chunk[..count]);
        if buffer.len() > 1_048_576 {
            panic!("peer request exceeds 1 MiB");
        }
    };
    let header = std::str::from_utf8(&buffer[..split - 4]).unwrap_or_else(|error| panic!("peer headers are not UTF-8: {error}"));
    let mut lines = header.split("\r\n");
    let request = lines.next().unwrap_or("");
    let path = request.split(' ').nth(1).unwrap_or("").to_string();
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            panic!("malformed peer header: {line}");
        };
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.parse::<usize>().unwrap_or_else(|error| panic!("bad content-length: {error}")))
        .unwrap_or_else(|| panic!("missing content-length"));
    let mut body = buffer[split..].to_vec();
    while body.len() < length {
        let count = read_some(stream, &mut chunk);
        if count == 0 {
            panic!("peer body truncated");
        }
        body.extend_from_slice(&chunk[..count]);
        if body.len() > 1_048_576 {
            panic!("peer body exceeds 1 MiB");
        }
    }
    body.truncate(length);
    let body = String::from_utf8(body).unwrap_or_else(|error| panic!("peer body is not UTF-8: {error}"));
    (path, headers, body)
}

fn read_some(stream: &mut TcpStream, chunk: &mut [u8]) -> usize {
    loop {
        match stream.read(chunk) {
            Ok(count) => return count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
        }
    }
}

fn read_all(stream: &mut TcpStream) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => data.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut => {
                if !data.is_empty() {
                    break;
                }
                panic!("peer response timed out");
            }
            Err(error) => panic!("peer response read: {error}"),
        }
        if data.len() > 8 * 1024 * 1024 {
            panic!("peer response exceeds 8 MiB");
        }
    }
    data
}

fn read_chunk(stream: &mut TcpStream) -> Vec<u8> {
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Vec::new(),
            Ok(count) => return buffer[..count].to_vec(),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
        }
    }
}

fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
}

fn form_value(body: &str, key: &str) -> String {
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if decode(name) == key {
            return decode(value);
        }
    }
    panic!("missing form field {key}")
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    panic!("truncated percent-escape");
                }
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or_else(|error| panic!("percent-escape: {error}"));
                out.push(u8::from_str_radix(hex, 16).unwrap_or_else(|error| panic!("percent-escape: {error}")));
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|error| panic!("form value is not UTF-8: {error}"))
}

fn token() -> String {
    let mut bytes = [0u8; 32];
    let mut file = std::fs::File::open("/dev/urandom").unwrap_or_else(|error| panic!("urandom: {error}"));
    file.read_exact(&mut bytes).unwrap_or_else(|error| panic!("urandom: {error}"));
    b64_encode(&bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    match effect.get("wire") {
        Some(Value::Str(text)) => text.as_bytes().to_vec(),
        _ => panic!("missing wire {}", effect.to_string()),
    }
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state, action, inputs, root)
}

fn blank() -> Value {
    obj(vec![])
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn profile(value: &Value) -> &Value {
    let profiles = state_of(value).get("profiles").unwrap_or_else(|| panic!("event returned no profiles"));
    profiles.arr().first().unwrap_or_else(|| panic!("event returned no profile"))
}

fn effect_null(value: &Value) -> bool {
    value.get("effect") == Some(&Value::Null)
}

fn dialog_eq(value: &Value, name: &str) -> bool {
    field_str(state_of(value), "dialog") == Some(name)
}

fn not_busy(value: &Value) -> bool {
    state_of(value).get("busy") == Some(&Value::Bool(false))
}

fn has_notice_error(value: &Value) -> bool {
    state_of(value).get("notice_error").is_some_and(truthy)
}

fn notice_has(value: &Value, needle: &str) -> bool {
    field_str(state_of(value), "notice").is_some_and(|text| text.contains(needle))
}

fn notice_lacks(value: &Value, needle: &str) -> bool {
    field_str(state_of(value), "notice").is_some_and(|text| !text.contains(needle))
}

fn ui_contains(value: &Value, needle: &str) -> bool {
    value.get("ui").is_some_and(|ui| ui.to_string().contains(needle))
}

fn states_equal(left: &Value, right: &Value) -> bool {
    state_of(left) == state_of(right)
}

fn browser_effect(value: &Value, url: &str) -> bool {
    value.get("effect")
        == Some(&obj(vec![("kind".into(), s("oauth_browser")), ("url".into(), s(url))]))
}

fn adopted(profile: &Value) -> bool {
    profile.get("oauth_enabled").is_some_and(truthy)
        && profile.get("key") == Some(&s("retained-api-key"))
        && profile.get("oauth_tokens").and_then(|tokens| tokens.get("access_token")).is_some_and(truthy)
}

fn access_token(profile: &Value) -> String {
    match profile.get("oauth_tokens").and_then(|tokens| tokens.get("access_token")) {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    }
}

fn refresh_token(profile: &Value) -> Option<String> {
    match profile.get("oauth_tokens").and_then(|tokens| tokens.get("refresh_token")) {
        Some(Value::Str(text)) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn wire_has_bearer(sent: &Value, access: &str) -> bool {
    match sent.get("effect").and_then(|effect| effect.get("wire")) {
        Some(Value::Str(text)) => text.contains(&format!("Bearer {access}")),
        _ => false,
    }
}

fn export_hides(exported: &Value, secrets: &[String]) -> bool {
    let Some(effect) = exported.get("effect") else { return false };
    let text = effect.to_string();
    secrets.iter().all(|secret| !secret.is_empty() && !text.contains(secret))
}

fn field_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value.get(key) {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    }
}

fn set_endpoint(state: &mut Value, endpoint: &str) {
    let current = state.get("profiles").cloned().unwrap_or_else(|| panic!("missing profiles"));
    let mut profiles = current.arr().clone();
    let profile = profiles.first_mut().unwrap_or_else(|| panic!("missing profile"));
    put(profile, "endpoint", s(endpoint));
    put(state, "profiles", Value::Arr(profiles));
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
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

fn flag(value: &Value, key: &str) -> Option<bool> {
    match value.get(key) {
        Some(Value::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

fn require_str(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        _ => panic!("missing {key} in {}", value.to_string()),
    }
}

fn nonempty(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Str(text)) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn as_u16(value: &Value) -> u16 {
    match value {
        Value::Num(token) | Value::Str(token) => token.parse().unwrap_or_else(|_| panic!("bad port {token}")),
        other => panic!("bad port {}", other.to_string()),
    }
}

fn as_usize(value: &Value) -> usize {
    match value {
        Value::Num(token) | Value::Str(token) => token.parse().unwrap_or_else(|_| panic!("bad size {token}")),
        other => panic!("bad size {}", other.to_string()),
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    "peer handler panicked".to_string()
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(create_scratch(root, "device-authorization"))
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

//! Real HTTP artifact URLs, bounded streaming, ownership and output integrity.
mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-core and real local HTTP checks; Android storage/decoder and live configured endpoints need device acceptance.";

struct Spec {
    response: &'static str,
    path: &'static str,
    kind: &'static str,
    limit: usize,
    method: Option<&'static str>,
    fallback: bool,
    timeout: u64,
    file: bool,
}

struct Peer {
    port: u16,
}

struct World {
    peers: Vec<Peer>,
    calls: Arc<Mutex<Vec<(u16, String, Option<String>)>>>,
    posts: Arc<Mutex<Vec<Vec<u8>>>>,
    target: Arc<Mutex<String>>,
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path());
    controller.finish();
    if passed.len() != 41 {
        panic!("expected 41 checks, emitted {}", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("media_stream", &body, true);
    println!("{} media_stream checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let audio = wave_bytes(4096);
    let large_audio = wave_bytes(5 * 1024 * 1024 / 2);
    let world = World::start(audio.clone(), large_audio.clone());
    let mut passed = Vec::new();
    let mut opened = Vec::new();

    world.set_target(&format!("http://127.0.0.1:{}/file", world.peers[1].port));
    let mut request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let downloaded = wire(effect(&request));
    pass(
        &mut passed,
        "artifact URL creates a separate bounded GET",
        has_download(&request) && port_of(&request) == world.peers[1].port as i64 && downloaded.starts_with(b"GET /file "),
    );
    pass(
        &mut passed,
        "download credentials belong only to artifact host",
        contains_bytes(&downloaded, b"key1") && !contains_bytes(&downloaded, b"key0"),
    );

    let active = owner(effect(&request));
    let _ = controller.op(&obj(vec![
        ("op".into(), s("media_stream_cancel")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s("wrong")),
    ]));
    let receipt = must(feed(controller, &mut opened, &request), "wrong ticket cancel cannot stop active stream");
    let done = settle(controller, root, &request, &receipt);
    let record = field(state(&done), "service_artifact");
    pass(&mut passed, "wrong ticket cancel cannot stop active stream", text(record, "sha256") == sha256(&audio));
    let encoded = state(&done).to_string();
    let adopted = artifact_bytes(root, record);
    pass(
        &mut passed,
        "stream adopts complete actual bytes outside conversation state",
        adopted == audio && encoded.len() < 100_000 && !encoded.contains(&b64_encode(&audio)),
    );
    let duplicate = settle(controller, root, &with_state(&request, state(&done)), &receipt);
    pass(&mut passed, "duplicate completed callback does not alter state", state(&duplicate) == state(&done));

    let before = call_count(&world);
    world.set_target("http://outside.invalid/file");
    let blocked = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    pass(
        &mut passed,
        "unconfigured artifact origin is refused before download",
        effect_is_null(&blocked) && text(field(state(&blocked), "service_result"), "status") == "failed" && call_count(&world) == before + 1,
    );

    world.set_target(&format!("http://127.0.0.1:{}/file", world.peers[1].port));
    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let active = owner(effect(&request));
    let bad = controller.send(&obj(vec![
        ("op".into(), s("media_stream_feed")),
        ("purpose".into(), s("wrong")),
        ("ticket".into(), s(&active.1)),
        ("data".into(), s(&b64_encode(b"x"))),
    ]));
    pass(&mut passed, "wrong stream purpose cannot feed bytes", bad.get("value").is_none());
    let premature = controller.send(&obj(vec![
        ("op".into(), s("media_stream_finish")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s(&active.1)),
        ("eof".into(), Value::Bool(false)),
    ]));
    pass(&mut passed, "a premature finish without EOF leaves the rightful download active", premature.get("value").is_none());
    let done = adopt(controller, root, &mut opened, &request, "invalid ownership and premature finish preserve ordinary retrieval");
    pass(
        &mut passed,
        "invalid ownership and premature finish preserve ordinary retrieval",
        text(field(state(&done), "service_artifact"), "sha256") == sha256(&audio),
    );

    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let active = owner(effect(&request));
    let _ = controller.op(&feed_envelope(&active, b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n"));
    let bad = controller.send(&obj(vec![
        ("op".into(), s("media_stream_feed")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s(&active.1)),
        ("data".into(), s("Zg==!invalid")),
    ]));
    let finish = controller.send(&finish_envelope(&active, true));
    let error = bad.get("error").and_then(as_str).unwrap_or("");
    pass(
        &mut passed,
        "garbage after Base64 padding cannot finish a download",
        error.contains("Invalid Base64 media chunk") && finish.get("value").is_none(),
    );
    cancel(controller, root, &request);

    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let active = owner(effect(&request));
    let bad = controller.send(&feed_envelope(&active, &vec![b'x'; 8193]));
    pass(&mut passed, "oversized native chunks are refused", bad.get("value").is_none());
    cancel(controller, root, &request);

    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let active = owner(effect(&request));
    let _ = controller.op(&feed_envelope(&active, b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort"));
    let bad = controller.send(&finish_envelope(&active, true));
    pass(&mut passed, "truncated response cannot become a receipt", bad.get("value").is_none());
    cancel(controller, root, &request);

    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let active = owner(effect(&request));
    let bad = controller.send(&feed_envelope(&active, b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n"));
    pass(&mut passed, "stream never follows redirects or replays generation", bad.get("value").is_none());
    cancel(controller, root, &request);

    world.set_target(&format!("http://127.0.0.1:{}/large", world.peers[1].port));
    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "data", 1, None, false, 120, false));
    let size_error = match feed(controller, &mut opened, &request) {
        Err(error) => error,
        Ok(_) => "large stream was accepted".to_string(),
    };
    pass(&mut passed, "configured file size bound is enforced", size_error.contains("bound"));
    let active = owner(effect(&request));
    let result = controller.send(&finish_envelope(&active, false));
    if result.get("value").is_some() {
        panic!("a size-rejected feed minted a receipt and returned {}", result.to_string());
    }
    pass(
        &mut passed,
        "size-rejected feed cannot be finalized even if the wire had completed",
        result.get("error").and_then(as_str).unwrap_or("").contains("ticket"),
    );
    let revived = controller.send(&obj(vec![
        ("op".into(), s("media_stream_feed")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s(&active.1)),
        ("data".into(), s("")),
    ]));
    pass(&mut passed, "another feed cannot revive the failed owner", revived.get("value").is_none());
    cancel(controller, root, &request);

    world.set_target(&format!("http://127.0.0.1:{}/file", world.peers[1].port));
    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let receipt = must(feed(controller, &mut opened, &request), "same length disk mutation invalidates ready receipt");
    let path = newest_file(&Path::new(root).join("service-files"));
    let mut original = std::fs::read(&path).unwrap_or_else(|error| panic!("artifact file: {error}"));
    if original.is_empty() {
        panic!("same length disk mutation invalidates ready receipt: artifact file is empty");
    }
    original[0] ^= 1;
    std::fs::write(&path, &original).unwrap_or_else(|error| panic!("artifact mutation: {error}"));
    let done = settle(controller, root, &request, &receipt);
    pass(
        &mut passed,
        "same length disk mutation invalidates ready receipt",
        text(field(state(&done), "service_result"), "status") == "failed" && absent(state(&done), "service_artifact"),
    );

    request = launch(controller, root, &world, &mut opened, &audio, spec("artifact_url", "/generate", "audio", 1, None, false, 120, false));
    let receipt = must(feed(controller, &mut opened, &request), "cancelled artifact callback is an exact state no-op");
    let stopped = controller.event(Some(state(&request)), "cancel", obj(vec![]), root);
    let late = settle(controller, root, &with_state(&request, state(&stopped)), &receipt);
    pass(&mut passed, "cancelled artifact callback is an exact state no-op", state(&late) == state(&stopped));

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-file", "audio", 8, Some("POST"), false, 120, false));
    pass(&mut passed, "POST binary response has an owned streaming destination", has_download(&request));
    let done = adopt(controller, root, &mut opened, &request, "POST created response retains complete media above inline limit");
    let record = field(state(&done), "service_artifact");
    pass(
        &mut passed,
        "POST created response retains complete media above inline limit",
        number(field(state(&done), "service_result"), "http_status") == 201
            && artifact_bytes(root, record) == large_audio
            && text(record, "sha256") == sha256(&large_audio),
    );

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-chunked", "audio", 8, Some("POST"), false, 120, false));
    let done = adopt(controller, root, &mut opened, &request, "chunked POST created response preserves exact decoded file");
    let record = field(state(&done), "service_artifact");
    pass(
        &mut passed,
        "chunked POST created response preserves exact decoded file",
        artifact_bytes(root, record) == large_audio && text(record, "sha256") == sha256(&large_audio),
    );

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-file", "audio", 8, Some("PUT"), false, 120, false));
    let done = adopt(controller, root, &mut opened, &request, "PUT created response retains complete generated media");
    pass(
        &mut passed,
        "PUT created response retains complete generated media",
        number(field(state(&done), "service_result"), "http_status") == 201
            && artifact_bytes(root, field(state(&done), "service_artifact")) == large_audio,
    );

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-reject", "audio", 8, Some("POST"), true, 120, true));
    let next_request = adopt(controller, root, &mut opened, &request, "reviewed input reaches only the initial explicitly rejected endpoint");
    pass(
        &mut passed,
        "reviewed input reaches only the initial explicitly rejected endpoint",
        last_post_is(&world, &audio) && last_key(&world).as_deref() == Some("key0"),
    );
    let done = adopt(controller, root, &mut opened, &next_request, "owned streamed input and output survive configured fallback");
    pass(
        &mut passed,
        "owned streamed input and output survive configured fallback",
        last_post_is(&world, &audio) && last_key(&world).as_deref() == Some("key1") && artifact_bytes(root, field(state(&done), "service_artifact")) == large_audio,
    );

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-reject", "audio", 8, Some("POST"), true, 120, false));
    let before = call_count(&world);
    let next_request = adopt(controller, root, &mut opened, &request, "complete configured503 permits exactly one initial submission fallback");
    pass(
        &mut passed,
        "complete configured503 permits exactly one initial submission fallback",
        !effect_is_null(&next_request) && port_of(&next_request) == world.peers[1].port as i64 && call_count(&world) == before + 1,
    );
    let done = adopt(controller, root, &mut opened, &next_request, "streamed fallback retains its own credentials and actual file");
    pass(
        &mut passed,
        "streamed fallback retains its own credentials and actual file",
        last_key(&world).as_deref() == Some("key1")
            && text(field(state(&done), "service_result"), "route") == "fallback"
            && artifact_bytes(root, field(state(&done), "service_artifact")) == large_audio,
    );

    for (path, status) in [("/post-auth", 401_i64), ("/post-accepted", 202), ("/post-redirect", 302), ("/post-partial", 206)] {
        request = start(controller, root, &world, &mut opened, &audio, spec("binary", path, "audio", 8, Some("POST"), true, 120, false));
        let before = call_count(&world);
        let done = adopt(controller, root, &mut opened, &request, "streamed status stays scoped without replay");
        let expected = if status == 202 { "accepted_unconfirmed" } else { "failed" };
        pass(
            &mut passed,
            &format!("streamed status stays scoped without replay {status}"),
            effect_is_null(&done)
                && call_count(&world) == before + 1
                && number(field(state(&done), "service_result"), "http_status") == status
                && text(field(state(&done), "service_result"), "status") == expected
                && absent(state(&done), "service_artifact"),
        );
    }

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-truncated-error", "audio", 8, Some("POST"), true, 120, false));
    let before = call_count(&world);
    let error = match feed(controller, &mut opened, &request) {
        Err(error) => error,
        Ok(_) => "truncated error was adopted".to_string(),
    };
    pass(&mut passed, "truncated status body cannot authorize fallback", error.contains("framing"));
    let active = owner(effect(&request));
    let _ = controller.op(&obj(vec![
        ("op".into(), s("media_stream_cancel")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s(&active.1)),
    ]));
    let failed = controller.event(
        Some(state(&request)),
        "failure",
        obj(vec![
            ("purpose".into(), s(&active.0)),
            ("sent".into(), Value::Bool(true)),
            ("error".into(), s("truncated response")),
        ]),
        root,
    );
    pass(
        &mut passed,
        "truncated submission remains uncertain without replay",
        effect_is_null(&failed) && call_count(&world) == before + 1,
    );

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-file", "audio", 8, Some("POST"), true, 120, false));
    let reserved = total_size(&Path::new(root).join("service-files"));
    let active = owner(effect(&request));
    let next_request = controller.event(
        Some(state(&request)),
        "failure",
        obj(vec![
            ("purpose".into(), s(&active.0)),
            ("sent".into(), Value::Bool(false)),
            ("error".into(), s("controlled pre-send refusal")),
        ]),
        root,
    );
    pass(
        &mut passed,
        "pre-send streamed fallback releases old reservation before allocating next",
        !effect_is_null(&next_request) && total_size(&Path::new(root).join("service-files")) == reserved,
    );
    let done = adopt(controller, root, &mut opened, &next_request, "pre-send fallback completes actual remaining endpoint");
    pass(&mut passed, "pre-send fallback completes actual remaining endpoint", text(field(state(&done), "service_result"), "route") == "fallback");

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-file", "audio", 8, Some("POST"), true, 120, false));
    let active = owner(effect(&request));
    let failed = controller.event(
        Some(state(&request)),
        "failure",
        obj(vec![
            ("purpose".into(), s(&active.0)),
            ("sent".into(), Value::Bool(true)),
            ("error".into(), s("controlled uncertain send")),
        ]),
        root,
    );
    pass(
        &mut passed,
        "uncertain initial binary submission never retries",
        effect_is_null(&failed) && text(field(state(&failed), "service_result"), "status") == "failed",
    );

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/large-file", "audio", 8, None, false, 120, false));
    let done = adopt(controller, root, &mut opened, &request, "large GET retrieval remains exact");
    pass(&mut passed, "large GET retrieval remains exact", artifact_bytes(root, field(state(&done), "service_artifact")) == large_audio);

    request = start(controller, root, &world, &mut opened, &audio, spec("binary", "/created-file", "audio", 8, None, false, 120, false));
    let error = match feed(controller, &mut opened, &request) {
        Err(error) => error,
        Ok(_) => "retrieval accepted submission-only status".to_string(),
    };
    pass(&mut passed, "created-file allowance stays scoped to initial submissions", error.contains("HTTP 201"));
    cancel(controller, root, &request);

    for stream in &opened {
        if stream.get("media_download").and_then(as_str).is_some_and(|ticket| !ticket.is_empty()) {
            let active = owner(stream);
            let _ = controller.op(&obj(vec![
                ("op".into(), s("media_stream_cancel")),
                ("purpose".into(), s(&active.0)),
                ("ticket".into(), s(&active.1)),
            ]));
        }
    }
    let files = Path::new(root).join("service-files");
    let before = total_size(&files);
    let expired = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-file", "audio", 8, Some("POST"), false, 5, false));
    pass(&mut passed, "expiry read begins with one exact reserved allowance", total_size(&files) == before + 8 * 1024 * 1024);
    thread::sleep(Duration::from_millis(5100));
    let active = owner(effect(&expired));
    let bad = controller.send(&feed_envelope(&active, b"x"));
    pass(&mut passed, "expired stream read releases its unwritten reservation", bad.get("value").is_none() && total_size(&files) == before);

    let _abandoned = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-file", "audio", 8, Some("POST"), false, 5, false));
    pass(&mut passed, "admission expiry begins with one exact reserved allowance", total_size(&files) == before + 8 * 1024 * 1024);
    thread::sleep(Duration::from_millis(5100));
    let current = start(controller, root, &world, &mut opened, &audio, spec("binary", "/post-file", "audio", 8, Some("POST"), false, 120, false));
    pass(&mut passed, "new stream prunes abandoned expired reservations", total_size(&files) == before + 8 * 1024 * 1024);
    cancel(controller, root, &current);
    passed
}

fn spec(response: &'static str, path: &'static str, kind: &'static str, limit: usize, method: Option<&'static str>, fallback: bool, timeout: u64, file: bool) -> Spec {
    Spec { response, path, kind, limit, method, fallback, timeout, file }
}

fn start(controller: &mut Controller, root: &str, world: &World, opened: &mut Vec<Value>, audio: &[u8], spec: Spec) -> Value {
    let hosts = Value::Arr(
        world
            .peers
            .iter()
            .enumerate()
            .map(|(index, peer)| {
                obj(vec![
                    ("id".into(), s(&format!("h{index}"))),
                    ("url".into(), s(&format!("http://127.0.0.1:{}", peer.port))),
                    ("enabled".into(), Value::Bool(true)),
                    ("plain".into(), Value::Bool(true)),
                    ("auth_header".into(), s("X-Key")),
                    ("key".into(), s(&format!("key{index}"))),
                ])
            })
            .collect(),
    );
    let mut route = obj(vec![
        ("id".into(), s("generate")),
        ("capability".into(), s("speech_synthesis")),
        ("host".into(), s("h0")),
        ("path".into(), s(spec.path)),
        ("method".into(), s(if spec.response == "artifact_url" { "POST" } else { "GET" })),
        ("response".into(), s(spec.response)),
        ("response_path".into(), s(if spec.response == "artifact_url" { "/url" } else { "" })),
        ("artifact_kind".into(), s(spec.kind)),
        ("artifact_host".into(), s("h1")),
        ("artifact_limit_mb".into(), twin::n(&spec.limit.to_string())),
        ("enabled".into(), Value::Bool(true)),
    ]);
    if let Some(method) = spec.method {
        set_key(&mut route, "method", s(method));
    }
    if spec.file {
        set_key(&mut route, "request_format", s("binary"));
    }
    set_key(&mut route, "timeout_seconds", twin::n(&spec.timeout.to_string()));
    let mut routes = vec![route.clone()];
    let mut defaults = obj(vec![]);
    if spec.fallback {
        let mut fallback = route;
        set_key(&mut fallback, "id", s("fallback"));
        set_key(&mut fallback, "host", s("h1"));
        set_key(&mut fallback, "path", s("/post-file"));
        set_key(&mut fallback, "method", s("POST"));
        routes.push(fallback);
        defaults = obj(vec![(
            "speech_synthesis".into(),
            obj(vec![
                ("routes".into(), Value::Arr(vec![s("generate"), s("fallback")])),
                ("fallback_codes".into(), Value::Arr(vec![twin::n("503")])),
                ("connect_fallback".into(), Value::Bool(true)),
            ]),
        )]);
    }
    let config = obj(vec![
        ("version".into(), twin::n("1")),
        ("hosts".into(), hosts),
        ("routes".into(), Value::Arr(routes)),
        ("defaults".into(), defaults),
    ]);
    let mut current = state(&controller.event(None, "resume", obj(vec![]), root)).clone();
    current = state(&controller.event(Some(&current), "service_connections", obj(vec![]), root)).clone();
    let saved = controller.event(Some(&current), "service_save", obj(vec![("service_config".into(), s(&config.to_string()))]), root);
    current = state(&saved).clone();
    if flag(state(&saved), "notice_error") {
        panic!("service configuration rejected: {}", text_or(state(&saved), "notice"));
    }
    if spec.file {
        let picked = controller.event(Some(&current), "media_pick", obj(vec![]), root);
        let ticket = text(effect(&picked), "media_ticket").to_string();
        current = state(&picked).clone();
        let stored = controller.event(
            Some(&current),
            "media_input_result",
            obj(vec![
                ("media_ticket".into(), s(&ticket)),
                ("name".into(), s("audio.wav")),
                ("data".into(), s(&b64_encode(audio))),
            ]),
            root,
        );
        current = state(&stored).clone();
    }
    current = state(&controller.event(Some(&current), "service_request", obj(vec![]), root)).clone();
    let reviewed = controller.event(
        Some(&current),
        "service_review",
        obj(vec![
            ("service_route".into(), s(if spec.fallback { "" } else { "generate" })),
            ("service_capability".into(), s(if spec.fallback { "speech_synthesis" } else { "" })),
            ("service_file".into(), Value::Bool(spec.file)),
            ("service_input".into(), s("{}")),
        ]),
        root,
    );
    current = state(&reviewed).clone();
    let approved = controller.event(Some(&current), "approve", obj(vec![]), root);
    opened.push(effect(&approved).clone());
    approved
}

fn launch(controller: &mut Controller, root: &str, world: &World, opened: &mut Vec<Value>, audio: &[u8], spec: Spec) -> Value {
    let started = start(controller, root, world, opened, audio, spec);
    exchange(controller, root, &started)
}

fn adopt(controller: &mut Controller, root: &str, opened: &mut Vec<Value>, request: &Value, name: &str) -> Value {
    let receipt = must(feed(controller, opened, request), name);
    settle(controller, root, request, &receipt)
}

fn exchange(controller: &mut Controller, root: &str, request: &Value) -> Value {
    let current = effect(request).clone();
    let mut socket = connect(&current);
    socket.write_all(&wire(&current)).unwrap_or_else(|error| panic!("request write: {error}"));
    upload(controller, &current, &mut socket);
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = read_socket(&mut socket, &mut buffer);
        if count == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..count]);
    }
    let purpose = text(&current, "purpose").to_string();
    let saved = state(request).clone();
    controller.event(
        Some(&saved),
        "network_result",
        obj(vec![("purpose".into(), s(&purpose)), ("data".into(), s(&b64_encode(&output)))]),
        root,
    )
}

fn feed(controller: &mut Controller, opened: &mut Vec<Value>, request: &Value) -> Result<Value, String> {
    let current = effect(request).clone();
    opened.push(current.clone());
    let active = owner(&current);
    let mut ready = false;
    let mut socket = connect(&current);
    socket.write_all(&wire(&current)).map_err(|error| format!("request write: {error}"))?;
    upload(controller, &current, &mut socket);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = read_socket(&mut socket, &mut buffer);
        if count == 0 {
            break;
        }
        match feed_part(controller, &active, &buffer[..count]) {
            Ok(is_ready) => {
                ready = is_ready;
                if ready {
                    break;
                }
            }
            Err(error) => return Err(error),
        }
    }
    finish_stream(controller, &active, !ready)
}

fn feed_part(controller: &mut Controller, active: &(String, String), bytes: &[u8]) -> Result<bool, String> {
    let reply = controller.send(&feed_envelope(active, bytes));
    let Some(value) = reply.get("value") else {
        return Err(reply.get("error").and_then(as_str).unwrap_or("missing media feed value").to_string());
    };
    match value.get("ready") {
        Some(Value::Bool(ready)) => Ok(*ready),
        _ => Err(format!("media feed missing ready: {}", value.to_string())),
    }
}

fn finish_stream(controller: &mut Controller, active: &(String, String), eof: bool) -> Result<Value, String> {
    let reply = controller.send(&finish_envelope(active, eof));
    match reply.get("value") {
        Some(value) => Ok(value.clone()),
        None => Err(reply.get("error").and_then(as_str).unwrap_or("missing media finish value").to_string()),
    }
}

fn upload(controller: &mut Controller, effect: &Value, socket: &mut TcpStream) {
    let Some(ticket) = effect.get("media_upload").and_then(as_str) else { return };
    if ticket.is_empty() {
        return;
    }
    let purpose = text(effect, "purpose").to_string();
    let expected = number(effect, "upload_bytes") as usize;
    let mut count = 0_usize;
    let mut failed = None;
    loop {
        let reply = controller.send(&obj(vec![
            ("op".into(), s("media_upload_read")),
            ("purpose".into(), s(&purpose)),
            ("ticket".into(), s(ticket)),
        ]));
        let Some(part) = reply.get("value") else {
            failed = Some(reply.to_string());
            break;
        };
        let data = b64_decode(text(part, "data"));
        if data.len() > 8192 {
            failed = Some(format!("upload chunk {} exceeds 8192", data.len()));
            break;
        }
        count += data.len();
        if count > expected {
            failed = Some(format!("upload count {count} exceeds {expected}"));
            break;
        }
        if let Err(error) = socket.write_all(&data) {
            failed = Some(format!("upload write: {error}"));
            break;
        }
        let done = match part.get("done") {
            Some(Value::Bool(done)) => *done,
            _ => {
                failed = Some(format!("upload part missing done: {}", part.to_string()));
                break;
            }
        };
        if done {
            if count != expected {
                failed = Some(format!("upload ended at {count}, expected {expected}"));
            }
            break;
        }
        if data.is_empty() {
            failed = Some("empty upload chunk before done".to_string());
            break;
        }
    }
    let cancel = controller.send(&obj(vec![
        ("op".into(), s("media_upload_cancel")),
        ("purpose".into(), s(&purpose)),
        ("ticket".into(), s(ticket)),
    ]));
    if cancel.get("value").is_none() {
        panic!("media_upload_cancel failed: {}", cancel.to_string());
    }
    if let Some(error) = failed {
        panic!("upload: {error}");
    }
}

fn settle(controller: &mut Controller, root: &str, request: &Value, receipt: &Value) -> Value {
    let purpose = text(effect(request), "purpose").to_string();
    let saved = state(request).clone();
    controller.event(
        Some(&saved),
        "network_result",
        obj(vec![("purpose".into(), s(&purpose)), ("media_receipt".into(), receipt.clone())]),
        root,
    )
}

fn cancel(controller: &mut Controller, root: &str, request: &Value) -> Value {
    let active = owner(effect(request));
    let _ = controller.op(&obj(vec![
        ("op".into(), s("media_stream_cancel")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s(&active.1)),
    ]));
    let saved = state(request).clone();
    controller.event(Some(&saved), "cancel", obj(vec![]), root)
}

fn feed_envelope(active: &(String, String), bytes: &[u8]) -> Value {
    obj(vec![
        ("op".into(), s("media_stream_feed")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s(&active.1)),
        ("data".into(), s(&b64_encode(bytes))),
    ])
}

fn finish_envelope(active: &(String, String), eof: bool) -> Value {
    obj(vec![
        ("op".into(), s("media_stream_finish")),
        ("purpose".into(), s(&active.0)),
        ("ticket".into(), s(&active.1)),
        ("eof".into(), Value::Bool(eof)),
    ])
}

fn connect(effect: &Value) -> TcpStream {
    let host = text(effect, "host").to_string();
    let port = number(effect, "port") as u16;
    let stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream.set_write_timeout(Some(Duration::from_secs(20))).unwrap_or_else(|error| panic!("write timeout: {error}"));
    let _ = stream.set_nodelay(true);
    stream
}

fn read_socket(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    loop {
        match stream.read(buffer) {
            Ok(count) => return count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("socket read: {error}"),
        }
    }
}

fn with_state(request: &Value, replacement: &Value) -> Value {
    let Value::Obj(fields) = request else { panic!("expected object") };
    let mut out = Vec::with_capacity(fields.len());
    let mut found = false;
    for (key, value) in fields {
        if key == "state" {
            out.push(("state".into(), replacement.clone()));
            found = true;
        } else {
            out.push((key.clone(), value.clone()));
        }
    }
    if !found {
        panic!("event has no state");
    }
    Value::Obj(out)
}

fn set_key(value: &mut Value, key: &str, replacement: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(slot) = fields.iter_mut().find(|(name, _)| name == key) {
        slot.1 = replacement;
    } else {
        fields.push((key.to_string(), replacement));
    }
}

fn effect<'a>(value: &'a Value) -> &'a Value {
    value.get("effect").unwrap_or_else(|| panic!("missing effect"))
}

fn state<'a>(value: &'a Value) -> &'a Value {
    value.get("state").unwrap_or_else(|| panic!("missing state"))
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    field(value, key).str_()
}

fn text_or<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(as_str).unwrap_or("")
}

fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

fn number(value: &Value, key: &str) -> i64 {
    match field(value, key) {
        Value::Num(number) | Value::Str(number) => number.parse().unwrap_or_else(|error| panic!("invalid number {number}: {error}")),
        other => panic!("expected number, got {}", other.to_string()),
    }
}

fn flag(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(Value::Bool(bit)) => *bit,
        Some(Value::Str(text)) => text == "true",
        _ => false,
    }
}

fn owner(effect: &Value) -> (String, String) {
    (text(effect, "purpose").to_string(), text(effect, "media_download").to_string())
}

fn wire(effect: &Value) -> Vec<u8> {
    match effect.get("wire_base64") {
        Some(Value::Str(encoded)) => b64_decode(encoded),
        _ => text(effect, "wire").as_bytes().to_vec(),
    }
}

fn has_download(value: &Value) -> bool {
    value.get("effect").and_then(|effect| effect.get("media_download")).and_then(as_str).is_some_and(|ticket| !ticket.is_empty())
}

fn effect_is_null(value: &Value) -> bool {
    matches!(value.get("effect"), None | Some(Value::Null))
}

fn port_of(value: &Value) -> i64 {
    number(effect(value), "port")
}

fn absent(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|part| part == needle)
}

fn artifact_bytes(root: &str, record: &Value) -> Vec<u8> {
    let path = Path::new(root).join(text(record, "path"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("artifact {}: {error}", path.display()))
}

fn pass(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn must(result: Result<Value, String>, name: &str) -> Value {
    result.unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn call_count(world: &World) -> usize {
    world.calls.lock().unwrap_or_else(|error| panic!("calls lock: {error}")).len()
}

fn last_key(world: &World) -> Option<String> {
    world.calls.lock().unwrap_or_else(|error| panic!("calls lock: {error}")).last().and_then(|call| call.2.clone())
}

fn last_post_is(world: &World, body: &[u8]) -> bool {
    world.posts.lock().unwrap_or_else(|error| panic!("posts lock: {error}")).last().is_some_and(|post| post == body)
}

fn wave_bytes(samples: usize) -> Vec<u8> {
    let data_length = samples * 2;
    let mut out = Vec::with_capacity(44 + data_length);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_length as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&16_000_u32.to_le_bytes());
    out.extend_from_slice(&32_000_u32.to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_length as u32).to_le_bytes());
    out.resize(out.len() + data_length, 0);
    out
}

fn sha256(bytes: &[u8]) -> String {
    let mut hash = [0x6a09e667_u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = bytes.to_vec();
    let bit_length = (bytes.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    for block in message.chunks(64) {
        let mut words = [0_u32; 64];
        for index in 0..16 {
            words[index] = u32::from_be_bytes(block[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut working = hash;
        for index in 0..64 {
            let s1 = working[4].rotate_right(6) ^ working[4].rotate_right(11) ^ working[4].rotate_right(25);
            let choose = (working[4] & working[5]) ^ (!working[4] & working[6]);
            let temp1 = working[7].wrapping_add(s1).wrapping_add(choose).wrapping_add(SHA256_K[index]).wrapping_add(words[index]);
            let s0 = working[0].rotate_right(2) ^ working[0].rotate_right(13) ^ working[0].rotate_right(22);
            let majority = (working[0] & working[1]) ^ (working[0] & working[2]) ^ (working[1] & working[2]);
            let temp2 = s0.wrapping_add(majority);
            working[7] = working[6];
            working[6] = working[5];
            working[5] = working[4];
            working[4] = working[3].wrapping_add(temp1);
            working[3] = working[2];
            working[2] = working[1];
            working[1] = working[0];
            working[0] = temp1.wrapping_add(temp2);
        }
        for index in 0..8 {
            hash[index] = hash[index].wrapping_add(working[index]);
        }
    }
    hash.iter().map(|word| format!("{word:08x}")).collect()
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn newest_file(directory: &Path) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let mut best: Option<(i64, i64, PathBuf)> = None;
    for entry in std::fs::read_dir(directory).unwrap_or_else(|error| panic!("read dir: {error}")) {
        let entry = entry.unwrap_or_else(|error| panic!("dir entry: {error}"));
        let meta = entry.metadata().unwrap_or_else(|error| panic!("metadata: {error}"));
        let key = (meta.mtime(), meta.mtime_nsec());
        if best.as_ref().is_none_or(|current| key > (current.0, current.1)) {
            best = Some((key.0, key.1, entry.path()));
        }
    }
    best.unwrap_or_else(|| panic!("service-files is empty")).2
}

fn total_size(directory: &Path) -> u64 {
    std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read dir: {error}"))
        .map(|entry| entry.unwrap_or_else(|error| panic!("dir entry: {error}")).metadata().unwrap_or_else(|error| panic!("metadata: {error}")).len())
        .sum()
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or(0);
        let path = root.join("output").join(format!("stream-{nanos}"));
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

impl World {
    fn start(audio: Vec<u8>, large: Vec<u8>) -> World {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let posts = Arc::new(Mutex::new(Vec::new()));
        let target = Arc::new(Mutex::new(String::new()));
        let peers = (0..2)
            .map(|_| spawn_peer(audio.clone(), large.clone(), calls.clone(), posts.clone(), target.clone()))
            .collect();
        World { peers, calls, posts, target }
    }

    fn set_target(&self, value: &str) {
        *self.target.lock().unwrap_or_else(|error| panic!("target lock: {error}")) = value.to_string();
    }
}

fn spawn_peer(audio: Vec<u8>, large: Vec<u8>, calls: Arc<Mutex<Vec<(u16, String, Option<String>)>>>, posts: Arc<Mutex<Vec<Vec<u8>>>>, target: Arc<Mutex<String>>) -> Peer {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("bind: {error}"));
    let port = listener.local_addr().unwrap_or_else(|error| panic!("local addr: {error}")).port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
            handle(&mut stream, port, &audio, &large, &calls, &posts, &target);
        }
    });
    Peer { port }
}

fn handle(stream: &mut TcpStream, port: u16, audio: &[u8], large: &[u8], calls: &Mutex<Vec<(u16, String, Option<String>)>>, posts: &Mutex<Vec<Vec<u8>>>, target: &Mutex<String>) {
    let Some((method, path, headers, body)) = read_request(stream) else { return };
    let key = headers.iter().find(|(name, _)| name == "x-key").map(|(_, value)| value.clone());
    if method == "POST" || method == "PUT" {
        posts.lock().unwrap_or_else(|error| panic!("posts lock: {error}")).push(body);
    }
    calls.lock().unwrap_or_else(|error| panic!("calls lock: {error}")).push((port, path.clone(), key));
    if method == "POST" || method == "PUT" {
        if path == "/post-file" || path == "/post-chunked" {
            if path == "/post-chunked" {
                if write_head(stream, 201, &[("Transfer-Encoding", "chunked")]).is_err() {
                    return;
                }
                for chunk in large.chunks(8192) {
                    if stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes()).is_err() || stream.write_all(chunk).is_err() || stream.write_all(b"\r\n").is_err() {
                        return;
                    }
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            } else if write_body(stream, 201, large).is_err() {
                return;
            }
            return;
        }
        let statuses = [("/post-reject", 503_u16), ("/post-auth", 401), ("/post-accepted", 202), ("/post-redirect", 302), ("/post-partial", 206), ("/post-truncated-error", 503)];
        if let Some((_, status)) = statuses.iter().find(|(name, _)| *name == path) {
            let response_body: &[u8] = if path == "/post-partial" { audio } else { b"controlled status" };
            let declared = if path == "/post-truncated-error" { response_body.len() + 100 } else { response_body.len() };
            let declared = declared.to_string();
            if write_head(stream, *status, &[("Content-Length", &declared)]).is_err() {
                return;
            }
            let _ = stream.write_all(response_body);
            return;
        }
        let url = target.lock().unwrap_or_else(|error| panic!("target lock: {error}")).clone();
        let body = format!("{{\"url\":\"{url}\"}}");
        let _ = write_body(stream, 200, body.as_bytes());
        return;
    }
    let status = if path == "/created-file" { 201 } else { 200 };
    if path == "/large-file" || path == "/created-file" {
        let _ = write_body(stream, status, large);
    } else if path == "/file" {
        let _ = write_body(stream, status, audio);
    } else {
        let filler = vec![b'x'; 1024 * 1024 + 1];
        let _ = write_body(stream, status, &filler);
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        if buffer.len() > 1024 * 1024 {
            return None;
        }
        let count = match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
    };
    let head = std::str::from_utf8(&buffer[..header_end]).ok()?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?;
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let content_length = headers.iter().find(|(name, _)| name == "content-length").and_then(|(_, value)| value.parse::<usize>().ok()).unwrap_or(0);
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length {
        let count = match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        body.extend_from_slice(&chunk[..count]);
    }
    body.truncate(content_length);
    Some((method, path, headers, body))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        206 => "Partial Content",
        302 => "Found",
        401 => "Unauthorized",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

fn write_head(stream: &mut TcpStream, status: u16, headers: &[(&str, &str)]) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())
}

fn write_body(stream: &mut TcpStream, status: u16, body: &[u8]) -> std::io::Result<()> {
    let length = body.len().to_string();
    write_head(stream, status, &[("Content-Length", &length)])?;
    stream.write_all(body)
}

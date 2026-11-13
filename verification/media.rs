mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner file/HTTP fixture checks. No physical microphone, installed Android decoding, live provider or independent acceptance claimed.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&project, "media");
    let project_text = project.to_str().unwrap_or_else(|| panic!("project path is not UTF-8")).to_string();
    let scratch_text = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let audio = wav_fixture();
    let mut server = Peer::start(audio.clone());
    let mut controller = Controller::spawn(&project_text);
    let passed = run(&mut controller, &project_text, &scratch_text, &server, &audio);
    // The leg's TemporaryDirectory is deleted at scope exit, and a cleanup error raises
    // there, so the twin must fail the same way instead of swallowing it: leaving the tree
    // behind would let a later run observe files this run created.
    std::fs::remove_dir_all(&scratch)
        .unwrap_or_else(|error| panic!("could not remove {}: {error}", scratch.display()));
    controller.finish();
    server.stop();

    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("media", &body, true);
    println!("{} media checks passed", passed.len());
}

fn run(controller: &mut Controller, project: &str, root: &str, server: &Peer, audio: &[u8]) -> Vec<String> {
    let mut passed = Vec::new();
    let png = png_fixture();

    let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, audio, "recording.wav");
    let record = field(state_of(&picked), "media_input");
    let stored = owned_path(root, record);
    let digest = text(record, "sha256").to_string();
    keep(
        &mut passed,
        "selected audio stored as actual owned file",
        read_bytes(&stored) == audio && digest == twin::sha256_bytes(audio),
    );
    keep(
        &mut passed,
        "input bytes excluded from saved conversation JSON",
        !state_of(&picked).to_string().contains(&b64_encode(audio)),
    );

    let pending = review(controller, root, state_of(&picked), true);
    keep(
        &mut passed,
        "file disclosure is explicit before dispatch",
        text(field(field(state_of(&pending), "pending"), "file_disclosure"), "sha256") == digest,
    );
    let done = complete(controller, root, &pending);
    let hit = server.last();
    keep(
        &mut passed,
        "multipart transfer contains exact selected bytes",
        contains_bytes(&hit.body, audio) && header(&hit, "Content-Type").contains("multipart/form-data; boundary="),
    );
    keep(
        &mut passed,
        "multipart fields mapped from typed input",
        contains_bytes(&hit.body, b"name=\"language\"\r\n\r\nen") && contains_bytes(&hit.body, b"name=\"audio\""),
    );
    keep(&mut passed, "actual mapped transcription returned", result_text(&done) == "fixture transcription");

    let state = configure(controller, root, server, "binary", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, audio, "recording.wav");
    let pending = review(controller, root, state_of(&picked), true);
    complete(controller, root, &pending);
    keep(&mut passed, "raw transfer preserves all bytes", server.last().body == audio);

    let state = configure(controller, root, server, "binary", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, audio, "get-source.wav");
    let mut get_state = state_of(&picked).clone();
    update_route(&mut get_state, vec![("method", s("GET"))]);
    let stored = owned_path(root, field(&get_state, "media_input"));
    let before = server.len();
    let requested = event(controller, root, &get_state, "service_request", obj(vec![]));
    let refused = event(
        controller,
        root,
        state_of(&requested),
        "service_review",
        obj(vec![
            ("service_route".into(), s("media")),
            ("service_file".into(), twin::b(true)),
            ("service_input".into(), s("{\"language\": \"en\"}")),
        ]),
    );
    keep(
        &mut passed,
        "GET file request refuses before approval or transport",
        effect_is_null(&refused) && truthy_key(state_of(&refused), "notice_error") && falsy_key(state_of(&refused), "pending"),
    );
    keep(
        &mut passed,
        "GET refusal retains selected file and local bytes",
        field(state_of(&refused), "media_input") == field(&get_state, "media_input") && read_bytes(&stored) == audio && server.len() == before,
    );
    write_bytes(&Path::new(root).join("local.txt"), b"local operation retained");
    let local = controller.op(&obj(vec![
        ("op".into(), s("tool")),
        ("tool".into(), s("read")),
        ("root".into(), s(root)),
        ("args".into(), obj(vec![("path".into(), s("local.txt"))])),
    ]));
    keep(&mut passed, "GET refusal preserves unrelated local operation", local.str_() == "local operation retained");

    update_route(&mut get_state, vec![("request_format", s("json")), ("path", s("/status/{language}"))]);
    let pending = review(controller, root, &get_state, false);
    let done = complete(controller, root, &pending);
    let hit = server.last();
    keep(
        &mut passed,
        "GET without file retains path variables and sends no body",
        hit.path == "/status/en" && hit.body.is_empty() && result_text(&done) == "fixture transcription",
    );

    update_route(
        &mut get_state,
        vec![("method", s("PUT")), ("request_format", s("binary")), ("path", s("/transcribe"))],
    );
    let pending = review(controller, root, &get_state, true);
    complete(controller, root, &pending);
    keep(&mut passed, "PUT retains actual reviewed file upload", server.last().body == audio);

    let mapping = obj(vec![
        ("audio".into(), obj(vec![("$file".into(), s("base64"))])),
        ("name".into(), obj(vec![("$file".into(), s("name"))])),
    ]);
    let state = configure(controller, root, server, "json", "/transcribe", "json", "/text", Some(mapping), "data");
    let picked = pick(controller, root, &state, audio, "selected.wav");
    let pending = review(controller, root, state_of(&picked), true);
    complete(controller, root, &pending);
    let parsed = Value::parse(&String::from_utf8(server.last().body.clone()).unwrap_or_else(|error| panic!("JSON upload is not UTF-8: {error}")));
    let mapped_audio = parsed.get("audio").and_then(as_str).map(b64_decode);
    let mapped_name = parsed.get("name").and_then(as_str);
    keep(
        &mut passed,
        "explicit JSON file mapping preserves all bytes",
        mapped_audio.as_deref() == Some(audio) && mapped_name == Some("selected.wav"),
    );

    let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, audio, "original.wav");
    let pending = review(controller, root, state_of(&picked), true);
    write_bytes(&owned_path(root, field(state_of(&picked), "media_input")), b"changed");
    let refused = event(controller, root, state_of(&pending), "approve", obj(vec![]));
    keep(
        &mut passed,
        "changed file is withheld before upload",
        effect_is_null(&refused) && truthy_key(state_of(&refused), "notice_error"),
    );

    let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
    let picking = event(controller, root, &state, "media_pick", obj(vec![]));
    let ticket = text(field(&picking, "effect"), "media_ticket").to_string();
    let cancelled = event(controller, root, state_of(&picking), "cancel", obj(vec![]));
    let late = event(
        controller,
        root,
        state_of(&cancelled),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), s(&ticket)),
            ("data".into(), s(&b64_encode(audio))),
            ("name".into(), s("late.wav")),
            ("draft".into(), s("late injected draft")),
        ]),
    );
    keep(&mut passed, "cancelled native input callback is exact state no-op", state_of(&late) == state_of(&cancelled));

    let picking = event(controller, root, &state, "media_pick", obj(vec![]));
    let wrong = event(
        controller,
        root,
        state_of(&picking),
        "media_input_result",
        obj(vec![("media_ticket".into(), s("wrong")), ("draft".into(), s("injected"))]),
    );
    keep(&mut passed, "wrong native capture owner is exact state no-op", state_of(&wrong) == state_of(&picking));
    event(controller, root, state_of(&picking), "cancel", obj(vec![]));

    let request = obj(vec![("text".into(), s("fixture audio"))]);
    let state = configure(controller, root, server, "json", "/audio", "binary", "", Some(request), "audio");
    let picked = pick(controller, root, &state, audio, "source.wav");
    let pending = review(controller, root, state_of(&picked), false);
    let done = complete(controller, root, &pending);
    let artifact = field(state_of(&done), "service_artifact").clone();
    let artifact_path = owned_path(root, &artifact);
    let artifact_rel = text(&artifact, "path").to_string();
    keep(
        &mut passed,
        "binary result stored outside encrypted JSON",
        twin::truthy(&artifact) && read_bytes(&artifact_path) == audio && !state_of(&done).to_string().contains(&b64_encode(audio)),
    );
    keep(&mut passed, "returned artifact SHA matches actual bytes", text(&artifact, "sha256") == twin::sha256_bytes(audio));
    keep(
        &mut passed,
        "returned file appears in current conversation",
        listed_paths(&done).iter().any(|path| path == &artifact_rel),
    );

    let mut export_state = state_of(&done).clone();
    if !matches!(export_state.get("drive"), Some(Value::Obj(_))) {
        set_key(&mut export_state, "drive", obj(vec![]));
    }
    set_key(object_mut(&mut export_state, "drive"), "phone_default", twin::b(true));
    set_key(&mut export_state, "phone_uri", s("content://fixture/tree/owned"));
    let exported = event(controller, root, &export_state, "media_export", obj(vec![]));
    let prepare = field(field(&exported, "effect"), "prepare").clone();
    keep(
        &mut passed,
        "media export keeps actual detected extension and MIME",
        text(&prepare, "name").ends_with(".wav") && text(&prepare, "mime") == "audio/wav",
    );
    let record = controller.op(&obj(vec![
        ("op".into(), s("project_chunks")),
        ("action".into(), s("record")),
        ("prepare".into(), prepare),
        ("bytes".into(), s(&scalar_at(&artifact, "bytes"))),
    ]));
    keep(
        &mut passed,
        "media export metadata passes existing chunk transfer contract",
        record.get("mime").and_then(as_str) == Some("audio/wav"),
    );
    let preview = event(controller, root, state_of(&done), "media_preview", obj(vec![]));
    keep(
        &mut passed,
        "validated audio hands off to native decoder",
        field(&preview, "effect").get("kind").and_then(as_str) == Some("media_preview")
            && field(&preview, "effect").get("media_kind").and_then(as_str) == Some("audio"),
    );
    write_bytes(&artifact_path, b"changed");
    let blocked = event(controller, root, state_of(&done), "media_preview", obj(vec![]));
    keep(
        &mut passed,
        "changed returned file cannot reach decoder",
        effect_is_null(&blocked) && truthy_key(state_of(&blocked), "notice_error"),
    );

    for kind in ["audio", "video"] {
        let fixture = std::fs::read(Path::new(project).join(format!("verification/fixtures/track-{kind}.mp4")))
            .unwrap_or_else(|error| panic!("could not read track-{kind}.mp4: {error}"));
        let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
        let picked = pick(controller, root, &state, &fixture, "generic.mp4");
        let record = field(state_of(&picked), "media_input");
        keep(
            &mut passed,
            &format!("generic MP4 {kind} classification uses actual track handler"),
            text(record, "kind") == kind && read_bytes(&owned_path(root, record)) == fixture,
        );
    }

    let fake = b"\0\0\0\x18ftypisom\0\0\0\0isommp42\0\0\0\x40mdatvide";
    let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, fake, "fake.mp4");
    keep(
        &mut passed,
        "malformed MP4 and embedded handler text cannot assert video",
        text(field(state_of(&picked), "media_input"), "kind") == "data",
    );

    let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, audio, "audio.wav");
    let pending = review(controller, root, state_of(&picked), false);
    let refused = event(controller, root, state_of(&pending), "approve", obj(vec![]));
    keep(
        &mut passed,
        "multipart cannot silently upload without file consent",
        effect_is_null(&refused) && truthy_key(state_of(&refused), "notice_error"),
    );

    let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, audio, "keep.wav");
    let stored = owned_path(root, field(state_of(&picked), "media_input"));
    let name = file_name(&stored);
    let manager = event(controller, root, state_of(&picked), "media_files", obj(vec![]));
    keep(
        &mut passed,
        "media manager lists actual retained file",
        modal_text(&manager).contains(&name),
    );
    let selected = event(controller, root, state_of(&manager), &format!("media_file:{name}"), obj(vec![]));
    let refused = event(controller, root, state_of(&selected), "media_file_remove_confirm", obj(vec![]));
    keep(
        &mut passed,
        "file cannot be removed without its review",
        stored.exists() && truthy_key(state_of(&refused), "notice_error"),
    );
    let removal = event(controller, root, state_of(&selected), "media_file_remove", obj(vec![]));
    let _kept = event(controller, root, state_of(&removal), "media_files", obj(vec![]));
    keep(&mut passed, "leaving removal review retains file", stored.exists());
    let changed = event(controller, root, state_of(&removal), "media_files", obj(vec![]));
    let changed = event(controller, root, state_of(&changed), &format!("media_file:{name}"), obj(vec![]));
    let changed = event(controller, root, state_of(&changed), "media_file_remove", obj(vec![]));
    write_bytes(&stored, &[audio, b"changed"].concat());
    let blocked = event(controller, root, state_of(&changed), "media_file_remove_confirm", obj(vec![]));
    keep(
        &mut passed,
        "changed file is withheld from old removal approval",
        stored.exists() && truthy_key(state_of(&blocked), "notice_error"),
    );
    let selected = event(controller, root, state_of(&blocked), &format!("media_file:{name}"), obj(vec![]));
    let removal = event(controller, root, state_of(&selected), "media_file_remove", obj(vec![]));
    let done = event(controller, root, state_of(&removal), "media_file_remove_confirm", obj(vec![]));
    keep(
        &mut passed,
        "specific reviewed file removal clears selected input only",
        !stored.exists() && is_null(state_of(&done), "media_input") && dir_count(&Path::new(root).join("service-files")) > 0,
    );

    let state = configure(controller, root, server, "multipart", "/transcribe", "json", "/text", None, "data");
    let picked = pick(controller, root, &state, &png, "image.png");
    let attached = event(controller, root, state_of(&picked), "media_attach_input", obj(vec![]));
    let chat = active_chat(state_of(&attached));
    keep(
        &mut passed,
        "selected image attaches exact bytes to draft without sending",
        first_attachment_data(chat) == Some(b64_encode(&png).as_str()) && falsy_key(chat, "messages") && effect_is_null(&attached),
    );
    passed
}

fn configure(
    controller: &mut Controller,
    root: &str,
    server: &Peer,
    format_name: &str,
    path: &str,
    response: &str,
    pointer: &str,
    request: Option<Value>,
    kind: &str,
) -> Value {
    // The leg writes `request or {default}`, so every falsey value — an empty map,
    // an empty list, an empty string, zero, false — is replaced, not just an empty map.
    let fallback = || obj(vec![("language".into(), obj(vec![("$input".into(), s("/language"))]))]);
    let request = match request {
        Some(value) if twin::truthy(&value) => value,
        _ => fallback(),
    };
    let config = obj(vec![
        ("version".into(), twin::n("1")),
        (
            "hosts".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("local")),
                ("url".into(), s(&format!("http://127.0.0.1:{}", server.port()))),
                ("plain".into(), twin::b(true)),
                ("enabled".into(), twin::b(true)),
            ])]),
        ),
        (
            "routes".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("media")),
                ("host".into(), s("local")),
                ("capability".into(), s("speech_recognition")),
                ("path".into(), s(path)),
                ("method".into(), s("POST")),
                ("request_format".into(), s(format_name)),
                ("request".into(), request),
                ("file_field".into(), s("audio")),
                ("response".into(), s(response)),
                ("response_path".into(), s(pointer)),
                ("artifact_kind".into(), s(kind)),
                ("enabled".into(), twin::b(true)),
            ])]),
        ),
        ("defaults".into(), obj(vec![])),
    ]);
    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let connections = event(controller, root, state_of(&resumed), "service_connections", obj(vec![]));
    let saved = event(
        controller,
        root,
        state_of(&connections),
        "service_save",
        obj(vec![("service_config".into(), s(&config.to_string()))]),
    );
    state_of(&saved).clone()
}

fn pick(controller: &mut Controller, root: &str, state: &Value, content: &[u8], name: &str) -> Value {
    let picking = event(controller, root, state, "media_pick", obj(vec![]));
    let ticket = text(field(&picking, "effect"), "media_ticket").to_string();
    let picked = event(
        controller,
        root,
        state_of(&picking),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), s(&ticket)),
            ("name".into(), s(name)),
            ("data".into(), s(&b64_encode(content))),
        ]),
    );
    if state_of(&picked).get("media_input").filter(|value| !matches!(value, Value::Null)).is_none() {
        panic!("media input was not stored: {}", picked.to_string());
    }
    picked
}

fn review(controller: &mut Controller, root: &str, state: &Value, file: bool) -> Value {
    let requested = event(controller, root, state, "service_request", obj(vec![]));
    let reviewed = event(
        controller,
        root,
        state_of(&requested),
        "service_review",
        obj(vec![
            ("service_route".into(), s("media")),
            ("service_file".into(), twin::b(file)),
            ("service_input".into(), s("{\"language\": \"en\"}")),
        ]),
    );
    if falsy_key(state_of(&reviewed), "pending") {
        panic!("service review did not create pending state: {}", reviewed.to_string());
    }
    reviewed
}

fn complete(controller: &mut Controller, root: &str, pending: &Value) -> Value {
    let approved = event(controller, root, state_of(pending), "approve", obj(vec![]));
    let effect = field(&approved, "effect").clone();
    if matches!(effect, Value::Null) {
        panic!("network effect is missing: {}", approved.to_string());
    }
    let wire = wire_bytes(&effect);
    let host = text(&effect, "host").to_string();
    let port = as_u16(field(&effect, "port"));
    let purpose = text(&effect, "purpose").to_string();
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("could not connect to {host}:{port}: {error}"));
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    stream.write_all(&wire).and_then(|_| stream.flush()).unwrap_or_else(|error| panic!("could not send request: {error}"));
    upload(controller, &effect, &mut stream);
    let mut fields = response(controller, &effect, &mut stream);
    set_key(&mut fields, "purpose", s(&purpose));
    event(controller, root, state_of(&approved), "network_result", fields)
}

/// Same bounded chunk API as verification/transport.py. The wire is headers only
/// when the subject owns the body; those bytes are read back and written here.
fn upload(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) {
    let Some(ticket) = nonempty(effect.get("media_upload")) else { return };
    let purpose = text(effect, "purpose").to_string();
    let ticket = ticket.to_string();
    let limit = as_usize(field(effect, "upload_bytes"));
    let mut failure = None;
    let mut count = 0usize;
    loop {
        let part = controller.op(&obj(vec![
            ("op".into(), s("media_upload_read")),
            ("purpose".into(), s(&purpose)),
            ("ticket".into(), s(&ticket)),
        ]));
        let data = b64_decode(part.get("data").and_then(as_str).unwrap_or(""));
        if data.len() > 8192 {
            failure = Some(format!("upload chunk {} exceeds 8192", data.len()));
            break;
        }
        count += data.len();
        if count > limit {
            failure = Some(format!("upload count {count} exceeds {limit}"));
            break;
        }
        if let Err(error) = stream.write_all(&data) {
            failure = Some(format!("upload write: {error}"));
            break;
        }
        let done = match part.get("done") {
            Some(Value::Bool(done)) => *done,
            _ => {
                failure = Some(format!("upload part missing done: {}", part.to_string()));
                break;
            }
        };
        if done {
            if count != limit {
                failure = Some(format!("upload ended at {count}, expected {limit}"));
            }
            break;
        }
        if data.is_empty() {
            failure = Some("empty upload chunk before done".to_string());
            break;
        }
    }
    let _ = stream.flush();
    let _ = controller.op(&obj(vec![
        ("op".into(), s("media_upload_cancel")),
        ("purpose".into(), s(&purpose)),
        ("ticket".into(), s(&ticket)),
    ]));
    if let Some(error) = failure {
        panic!("upload: {error}");
    }
}

/// A binary route returns `media_receipt`. Every other route returns the raw
/// HTTP response in `data`, which is what transport.response does.
fn response(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) -> Value {
    let Some(ticket) = nonempty(effect.get("media_download")) else {
        return obj(vec![("data".into(), s(&b64_encode(&read_all(stream))))]);
    };
    let purpose = text(effect, "purpose").to_string();
    let ticket = ticket.to_string();
    let mut ready = false;
    let mut failure = None;
    loop {
        let chunk = match read_some(stream) {
            Ok(chunk) => chunk,
            Err(error) => {
                failure = Some(error);
                break;
            }
        };
        if chunk.is_empty() {
            break;
        }
        let fed = controller.op(&obj(vec![
            ("op".into(), s("media_stream_feed")),
            ("purpose".into(), s(&purpose)),
            ("ticket".into(), s(&ticket)),
            ("data".into(), s(&b64_encode(&chunk))),
        ]));
        ready = matches!(fed.get("ready"), Some(Value::Bool(true)));
        if ready {
            break;
        }
    }
    let receipt = if failure.is_none() {
        Some(controller.op(&obj(vec![
            ("op".into(), s("media_stream_finish")),
            ("purpose".into(), s(&purpose)),
            ("ticket".into(), s(&ticket)),
            ("eof".into(), twin::b(!ready)),
        ])))
    } else {
        None
    };
    let _ = controller.op(&obj(vec![
        ("op".into(), s("media_stream_cancel")),
        ("purpose".into(), s(&purpose)),
        ("ticket".into(), s(&ticket)),
    ]));
    if let Some(error) = failure {
        panic!("media response: {error}");
    }
    obj(vec![("media_receipt".into(), receipt.unwrap_or(Value::Null))])
}

fn event(controller: &mut Controller, root: &str, state: &Value, action: &str, inputs: Value) -> Value {
    controller.event(Some(state), action, inputs, root)
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    if let Some(encoded) = nonempty(effect.get("wire_base64")) {
        return b64_decode(encoded);
    }
    if let Some(text) = effect.get("wire").and_then(as_str) {
        return text.as_bytes().to_vec();
    }
    panic!("missing network wire: {}", effect.to_string());
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}: {}", value.to_string()))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    field(value, key).str_()
}

fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

fn nonempty<'a>(value: Option<&'a Value>) -> Option<&'a str> {
    match value {
        Some(Value::Str(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::Str(text) | Value::Num(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        other => panic!("expected scalar, got {}", other.to_string()),
    }
}

fn scalar_at(value: &Value, key: &str) -> String {
    scalar(field(value, key))
}

fn as_u16(value: &Value) -> u16 {
    scalar(value).parse().unwrap_or_else(|error| panic!("invalid port {}: {error}", scalar(value)))
}

fn as_usize(value: &Value) -> usize {
    scalar(value).parse().unwrap_or_else(|error| panic!("invalid size {}: {error}", scalar(value)))
}

fn truthy_key(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(twin::truthy)
}

fn falsy_key(value: &Value, key: &str) -> bool {
    !truthy_key(value, key)
}

fn effect_is_null(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn is_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Null))
}

fn result_text(value: &Value) -> &str {
    field(field(state_of(value), "service_result"), "value").str_()
}

fn listed_paths(value: &Value) -> Vec<String> {
    let Some(Value::Arr(items)) = field(value, "ui").get("service_artifacts") else { return Vec::new() };
    items.iter().filter_map(|item| item.get("path").and_then(as_str).map(str::to_string)).collect()
}

fn modal_text(value: &Value) -> String {
    field(value, "ui").get("modal").map(Value::to_string).unwrap_or_default()
}

fn active_chat(state: &Value) -> &Value {
    let active = text(state, "active");
    field(state, "chats")
        .arr()
        .iter()
        .find(|chat| chat.get("id").and_then(as_str) == Some(active))
        .unwrap_or_else(|| panic!("active chat is missing"))
}

fn first_attachment_data(chat: &Value) -> Option<&str> {
    let Value::Arr(items) = chat.get("draft_attachments")? else { return None };
    let item = items.first()?;
    item.get("data").and_then(as_str)
}

fn object_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else { panic!("expected object") };
    fields.iter_mut().find(|(name, _)| name == key).map(|(_, item)| item).unwrap_or_else(|| panic!("missing {key}"))
}

fn set_key(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some((_, slot)) = fields.iter_mut().find(|(name, _)| name == key) {
        *slot = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn route_mut(state: &mut Value) -> &mut Value {
    let connections = object_mut(state, "service_connections");
    let routes = object_mut(connections, "routes");
    let Value::Arr(items) = routes else { panic!("expected routes array") };
    items.first_mut().unwrap_or_else(|| panic!("missing route"))
}

fn update_route(state: &mut Value, updates: Vec<(&str, Value)>) {
    let route = route_mut(state);
    for (key, value) in updates {
        set_key(route, key, value);
    }
}

fn owned_path(root: &str, record: &Value) -> PathBuf {
    Path::new(root).join(text(record, "path"))
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).unwrap_or_else(|| panic!("unnamed file {}", path.display())).to_string()
}

fn read_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    }
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
}

fn dir_count(path: &Path) -> usize {
    std::fs::read_dir(path).unwrap_or_else(|error| panic!("could not read {}: {error}", path.display())).count()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
    println!("{name}");
    let _ = std::io::stdout().flush();
}

fn wav_fixture() -> Vec<u8> {
    let data_len = 1600 * 2;
    let mut wav = Vec::with_capacity(44 + data_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());
    wav.resize(44 + data_len, 0);
    wav
}

fn png_fixture() -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], body: &[u8]| {
        png.extend_from_slice(&(body.len() as u32).to_be_bytes());
        png.extend_from_slice(kind);
        png.extend_from_slice(body);
        let mut checked = kind.to_vec();
        checked.extend_from_slice(body);
        png.extend_from_slice(&twin::crc32(&checked).to_be_bytes());
    };
    chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
    chunk(b"IDAT", &twin::zlib_compress(&[0, 0]));
    chunk(b"IEND", &[]);
    png
}

struct Hit {
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Clone for Hit {
    fn clone(&self) -> Self {
        Self { path: self.path.clone(), headers: self.headers.clone(), body: self.body.clone() }
    }
}

fn header<'a>(hit: &'a Hit, name: &str) -> &'a str {
    hit.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str()).unwrap_or("")
}

struct Peer {
    port: u16,
    stop: Arc<AtomicBool>,
    received: Arc<Mutex<Vec<Hit>>>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn start(audio: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("could not bind peer: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")).port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_received = Arc::clone(&received);
        let thread_stop = Arc::clone(&stop);
        let audio = Arc::new(audio);
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) => {
                        if thread_stop.load(Ordering::SeqCst) {
                            break;
                        }
                        panic!("peer accept: {error}");
                    }
                };
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                if let Err(error) = serve(&mut stream, &thread_received, &audio) {
                    panic!("peer request: {error}");
                }
            }
        });
        Self { port, stop, received, thread: Some(thread) }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn len(&self) -> usize {
        self.received.lock().unwrap_or_else(|error| panic!("peer lock: {error}")).len()
    }

    fn last(&self) -> Hit {
        self.received
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .last()
            .cloned()
            .unwrap_or_else(|| panic!("peer received no request"))
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                panic!("peer thread panicked");
            }
        }
    }
}

fn serve(stream: &mut TcpStream, received: &Mutex<Vec<Hit>>, audio: &[u8]) -> Result<(), String> {
    let request = read_request(stream)?;
    let body = if (request.method == "POST" || request.method == "PUT") && request.path == "/audio" {
        audio.to_vec()
    } else if request.method == "POST" || request.method == "PUT" {
        format!("{{\"text\": \"fixture transcription\", \"length\": {}}}", request.body.len()).into_bytes()
    } else {
        b"{\"text\": \"fixture transcription\"}".to_vec()
    };
    received.lock().map_err(|error| format!("peer lock: {error}"))?.push(Hit {
        path: request.path,
        headers: request.headers,
        body: request.body,
    });
    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&body)).and_then(|_| stream.flush()).map_err(|error| format!("response write: {error}"))
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    let header_end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if bytes.len() > 65536 {
            return Err("request headers exceed 64 KiB".into());
        }
        let count = read_some_into(stream, &mut buffer)?;
        if count == 0 {
            return Err("connection closed before request headers".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    };
    let head = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "request headers are not UTF-8".to_string())?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or("").split_whitespace();
    let method = request_line.next().unwrap_or("").to_string();
    let path = request_line.next().unwrap_or("").to_string();
    if method.is_empty() || path.is_empty() {
        return Err(format!("invalid request line in {head}"));
    }
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(format!("invalid request header: {line}"));
        };
        headers.push((name.to_string(), value.trim().to_string()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    if length > 8 * 1024 * 1024 {
        return Err("request body exceeds 8 MiB".into());
    }
    let mut body = bytes[header_end + 4..].to_vec();
    while body.len() < length {
        let count = read_some_into(stream, &mut buffer)?;
        if count == 0 {
            return Err(format!("connection closed with {} of {length} body bytes", body.len()));
        }
        body.extend_from_slice(&buffer[..count]);
    }
    body.truncate(length);
    Ok(Request { method, path, headers, body })
}

fn read_some(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut buffer = [0u8; 8192];
    let count = read_some_into(stream, &mut buffer)?;
    Ok(buffer[..count].to_vec())
}

fn read_all(stream: &mut TcpStream) -> Vec<u8> {
    let mut data = Vec::new();
    loop {
        let chunk = read_some(stream).unwrap_or_else(|error| panic!("socket read: {error}"));
        if chunk.is_empty() {
            break;
        }
        data.extend(chunk);
    }
    data
}

fn read_some_into(stream: &mut TcpStream, buffer: &mut [u8]) -> Result<usize, String> {
    loop {
        match stream.read(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("socket read: {error}")),
        }
    }
}

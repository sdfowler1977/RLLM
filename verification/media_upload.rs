mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner checks using actual disk bytes, public shared-core events, chunk APIs and local HTTP sockets. Android SDK compilation is separate; physical picker/socket/lifecycle and live user servers remain unverified.";
const MIB: u64 = 1024 * 1024;
const MAXIMUM: u64 = 64 * MIB;
// The peer must accept the 64 MiB upload and the repeated base64 JSON body.
const BODY_LIMIT: usize = 80 * 1024 * 1024;

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = twin::create_scratch(&project, "upload");
    let workspace = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch directory is not UTF-8"))
        .to_string();
    let peer = Peer::start();
    let mut controller = Controller::spawn(&project_text);
    let mut passed = Vec::new();
    run(&mut controller, &project_text, &workspace, &peer, &mut passed);
    controller.finish();
    drop(peer);
    if passed.len() != 65 {
        panic!("emitted {} checks, expected 65", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.into_iter().map(|name| s(&name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    // The leg writes json.dumps(record, indent=2) plus a trailing newline and prints the
    // same pretty record on stdout, so both channels carry identical bytes.
    twin::report_value_printed("media_upload", &body, true);
    let written = std::fs::read_to_string("output/checks/media_upload.json")
        .unwrap_or_else(|error| panic!("output/checks/media_upload.json: {error}"));
    if written != format!("{}\n", twin::pretty(&body)) {
        panic!("output/checks/media_upload.json has the wrong shape");
    }
    std::fs::remove_dir_all(&scratch).unwrap_or_else(|error| panic!("could not remove {}: {error}", scratch.display()));
}

fn run(controller: &mut Controller, project: &str, workspace: &str, peer: &Peer, passed: &mut Vec<String>) {
    let data = pdf_fixture();
    let configured = config(controller, None, "binary", None, workspace, peer.port);
    let (done, owner, receipt) = selected(controller, &configured, &data, "source.pdf", workspace);
    let record = state_of(&done).get("media_input").unwrap_or_else(|| panic!("missing media_input"));
    let stored_path = Path::new(workspace).join(must_str(record, "path"));
    let stored = read_bytes(&stored_path);
    let digest = twin::sha256_bytes(&data);
    keep(
        passed,
        "PDF larger than inline limit imports exact bytes",
        data.len() > 2 * 1024 * 1024
            && stored == data
            && same_str(record, "sha256", &digest)
            && same_str(record, "name", "source.pdf")
            && same_str(record, "kind", "document"),
    );
    let dumped = python_dumps(state_of(&done));
    let needle = b64_encode(&data[..100_000.min(data.len())]);
    keep(
        passed,
        "large file bytes stay outside saved state",
        dumped.len() < 100_000 && !dumped.contains(&needle),
    );
    let repeated = controller.event(
        Some(state_of(&done)),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), must_clone(&owner, "media_ticket")),
            ("media_receipt".into(), receipt.clone()),
            ("draft".into(), s("injected")),
        ]),
        workspace,
    );
    keep(passed, "input completion is single-use exact no-op", same_state(&repeated, &done));

    let approved_reply = approved(controller, state_of(&done), None, workspace);
    let uploaded = exchange(controller, &approved_reply, workspace, passed);
    let binary_ok = peer.with_last(|hit| hit.body == data && header_exact(&hit.headers, "Content-Type") == "application/pdf");
    keep(
        passed,
        "binary HTTP upload retains complete large PDF",
        binary_ok && service_received(&uploaded),
    );

    let json_request = obj(vec![
        ("source".into(), file_map("base64")),
        ("url".into(), file_map("data_url")),
        ("name".into(), file_map("name")),
        ("mime".into(), file_map("mime")),
        ("opaque".into(), obj(vec![("$input".into(), s("/opaque"))])),
    ]);
    let json_state = config(controller, Some(state_of(&done)), "json", Some(json_request), workspace, peer.port);
    let json_input = obj(vec![("opaque".into(), obj(vec![("$file".into(), s("base64"))]))]);
    let json_approved = approved(controller, &json_state, Some(json_input), workspace);
    exchange(controller, &json_approved, workspace, passed);
    let json_value = peer.with_last(|hit| parse_json_unlimited(&hit.body));
    let source_ok = b64_decode(must_str(&json_value, "source")) == data;
    let url = must_str(&json_value, "url");
    let url_body = url.split_once(',').map(|(_, rest)| rest).unwrap_or_else(|| panic!("data url has no comma"));
    keep(
        passed,
        "JSON streams repeated base64 and data URL without padding gaps",
        source_ok && b64_decode(url_body) == data && same_str(&json_value, "name", "source.pdf") && same_str(&json_value, "mime", "application/pdf"),
    );
    let opaque_expected = obj(vec![("$file".into(), s("base64"))]);
    keep(
        passed,
        "input data cannot acquire template file authority",
        json_value.get("opaque").unwrap_or_else(|| panic!("missing opaque")) == &opaque_expected,
    );

    let multipart_request = obj(vec![
        ("language".into(), obj(vec![("$input".into(), s("/language"))])),
        ("encoded".into(), file_map("base64")),
        ("detail".into(), obj(vec![("name".into(), file_map("name"))])),
    ]);
    let multipart_state = config(controller, Some(state_of(&done)), "multipart", Some(multipart_request), workspace, peer.port);
    let multipart_approved = approved(
        controller,
        &multipart_state,
        Some(obj(vec![("language".into(), s("en"))])),
        workspace,
    );
    exchange(controller, &multipart_approved, workspace, passed);
    let multipart_ok = peer.with_last(|hit| multipart_matches(&hit.headers, &hit.body, &data));
    keep(passed, "multipart streams actual file and mapped fields", multipart_ok);

    for length in [1usize, 2, 3, 8191, 8192, 8193] {
        let content: Vec<u8> = (0..length).map(|index| (index % 251) as u8).collect();
        let small_state = config(
            controller,
            None,
            "json",
            Some(obj(vec![("source".into(), file_map("base64"))])),
            workspace,
            peer.port,
        );
        let (small, _, _) = selected(controller, &small_state, &content, "bytes.bin", workspace);
        let small_approved = approved(controller, state_of(&small), None, workspace);
        exchange(controller, &small_approved, workspace, passed);
        let round_trip = peer.with_last(|hit| {
            let value = parse_json_unlimited(&hit.body);
            b64_decode(must_str(&value, "source")) == content
        });
        keep(passed, &format!("base64 chunk boundary {length}"), round_trip);
    }

    let samples = [
        ("invalid Base64", "%%%".to_string()),
        ("garbage after padding", "Zg==!invalid".to_string()),
        ("partial Base64 quantum", "Zm9vY".to_string()),
        ("nonzero padding bits", "Zh==".to_string()),
        ("oversized input", b64_encode(&vec![b'x'; 8193])),
    ];
    for (label, encoded) in samples {
        let fresh = config(controller, None, "binary", None, workspace, peer.port);
        let (opened, feed_owner, _) = opening(controller, Some(&fresh), "source.pdf", workspace);
        controller.op(&overlay(
            &feed_owner,
            vec![("op", s("media_input_feed")), ("data", s(&b64_encode(b"valid prefix")))],
        ));
        let failed = controller.send(&overlay(
            &feed_owner,
            vec![("op", s("media_input_feed")), ("data", s(&encoded))],
        ));
        keep(passed, &format!("{label} refuses the owned chunk"), value_absent(&failed));
        let finish = controller.send(&overlay(&feed_owner, vec![("op", s("media_input_finish"))]));
        keep(passed, &format!("{label} cannot issue a receipt for the valid prefix"), value_absent(&finish));
        let retired = controller.send(&overlay(
            &feed_owner,
            vec![("op", s("media_input_feed")), ("data", s("eA=="))],
        ));
        keep(
            passed,
            &format!("{label} retires the failed input before any further feed"),
            error_contains(&retired, "Input selection is absent"),
        );
        controller.event(Some(state_of(&opened)), "cancel", obj(vec![]), workspace);
    }

    let unpadded_state = config(controller, None, "binary", None, workspace, peer.port);
    let (unpadded, unpadded_owner, _) = opening(controller, Some(&unpadded_state), "unpadded.bin", workspace);
    controller.op(&overlay(&unpadded_owner, vec![("op", s("media_input_feed")), ("data", s("Zg"))]));
    let unpadded_receipt = controller.op(&overlay(&unpadded_owner, vec![("op", s("media_input_finish"))]));
    let unpadded_done = controller.event(
        Some(state_of(&unpadded)),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), must_clone(&unpadded_owner, "media_ticket")),
            ("media_receipt".into(), unpadded_receipt),
        ]),
        workspace,
    );
    let unpadded_path = Path::new(workspace).join(must_str(must_get(state_of(&unpadded_done), "media_input"), "path"));
    keep(passed, "canonical unpadded Base64 retains the exact file", read_bytes(&unpadded_path) == b"f");

    let wrong_state = config(controller, None, "binary", None, workspace, peer.port);
    let (wrong_open, wrong_owner, _) = opening(controller, Some(&wrong_state), "source.pdf", workspace);
    let wrong = controller.send(&overlay(
        &wrong_owner,
        vec![
            ("op", s("media_input_feed")),
            ("ticket", s("wrong")),
            ("data", s(&b64_encode(b"wrong"))),
        ],
    ));
    keep(passed, "wrong import ticket cannot write", value_absent(&wrong));
    controller.op(&overlay(
        &wrong_owner,
        vec![("op", s("media_input_feed")), ("data", s(&b64_encode(b"partial retained")))],
    ));
    let cancelled = controller.event(Some(state_of(&wrong_open)), "cancel", obj(vec![]), workspace);
    let cancelled_finish = controller.send(&overlay(&wrong_owner, vec![("op", s("media_input_finish"))]));
    keep(
        passed,
        "cancelled import cannot finish and retains partial file",
        value_absent(&cancelled_finish) && any_small_file_eq(&Path::new(workspace).join("service-files"), b"partial retained"),
    );
    let late = controller.event(
        Some(state_of(&cancelled)),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), must_clone(&wrong_owner, "media_ticket")),
            ("media_receipt".into(), s("late")),
        ]),
        workspace,
    );
    keep(passed, "cancelled import callback cannot change state", same_state(&late, &cancelled));

    let fresh_state = config(controller, None, "binary", None, workspace, peer.port);
    let (fresh, _, _) = selected(controller, &fresh_state, &data, "source.pdf", workspace);
    let upload_reply = approved(controller, state_of(&fresh), None, workspace);
    let upload_effect = must_get(&upload_reply, "effect");
    let upload_owner = obj(vec![
        ("purpose".into(), must_clone(upload_effect, "purpose")),
        ("ticket".into(), must_clone(upload_effect, "media_upload")),
    ]);
    let wrong_purpose = controller.send(&overlay(
        &upload_owner,
        vec![("op", s("media_upload_read")), ("purpose", s("wrong"))],
    ));
    keep(passed, "wrong purpose cannot consume upload", value_absent(&wrong_purpose));
    let part = controller.op(&overlay(&upload_owner, vec![("op", s("media_upload_read"))]));
    keep(
        passed,
        "upload first chunk is bounded actual data",
        b64_decode(must_str(&part, "data")) == data[..8192.min(data.len())],
    );
    let media_path = Path::new(workspace).join(must_str(must_get(state_of(&fresh), "media_input"), "path"));
    let original = read_bytes(&media_path);
    if original.len() < 7 {
        panic!("imported file is shorter than the mutation prefix");
    }
    let mut mutated = b"changed".to_vec();
    mutated.extend_from_slice(&original[7..]);
    write_bytes(&media_path, &mutated);
    let mutated_read = controller.send(&overlay(&upload_owner, vec![("op", s("media_upload_read"))]));
    keep(passed, "same-size file mutation interrupts further upload", value_absent(&mutated_read));
    let retired_upload = controller.send(&overlay(&upload_owner, vec![("op", s("media_upload_read"))]));
    keep(
        passed,
        "owned upload error retires the ticket before any retry",
        error_contains(&retired_upload, "Upload ticket or purpose differs"),
    );
    controller.event(Some(state_of(&upload_reply)), "cancel", obj(vec![]), workspace);
    let resumed = controller.send(&overlay(&upload_owner, vec![("op", s("media_upload_read"))]));
    keep(passed, "cancelled upload cannot resume", value_absent(&resumed));

    let quota = Path::new(workspace).join("quota");
    std::fs::create_dir(&quota).unwrap_or_else(|error| panic!("could not create {}: {error}", quota.display()));
    let quota_text = quota.to_str().unwrap_or_else(|| panic!("quota path is not UTF-8")).to_string();
    let mut openings = Vec::new();
    for index in 0..4 {
        let (pending, feed_owner, stream) = opening(controller, None, "source.pdf", &quota_text);
        keep(passed, &format!("concurrent reservation {index}"), same_u64(&stream, "limit", 64 * MIB));
        openings.push((pending, feed_owner));
    }
    let fifth = controller.event(None, "media_pick", obj(vec![]), &quota_text);
    let fifth_ticket = must_clone(must_get(&fifth, "effect"), "media_ticket");
    let fifth_open = controller.send(&obj(vec![
        ("op".into(), s("media_input_open")),
        ("media_ticket".into(), fifth_ticket.clone()),
        ("name".into(), s("fifth.bin")),
    ]));
    let reserved = dir_size_sum(&quota.join("service-files"));
    keep(
        passed,
        "fifth stream cannot overbook shared storage",
        value_absent(&fifth_open) && reserved == 256 * MIB,
    );
    refuse_other_process(project, &quota_text, passed);

    let (pending, feed_owner) = openings.remove(0);
    controller.op(&overlay(
        &feed_owner,
        vec![("op", s("media_input_feed")), ("data", s(&b64_encode(b"quota partial")))],
    ));
    controller.event(Some(state_of(&pending)), "cancel", obj(vec![]), &quota_text);
    let quota_files = quota.join("service-files");
    let released = dir_size_sum(&quota_files);
    keep(
        passed,
        "cancellation releases only unwritten reservation",
        released == 3 * 64 * MIB + b"quota partial".len() as u64 && any_small_file_eq(&quota_files, b"quota partial"),
    );
    let reused = controller.op(&obj(vec![
        ("op".into(), s("media_input_open")),
        ("media_ticket".into(), fifth_ticket),
        ("name".into(), s("fifth.bin")),
    ]));
    keep(
        passed,
        "released quota can be used by next selection",
        same_u64(&reused, "limit", 64 * MIB - b"quota partial".len() as u64),
    );
    for (pending, _) in &openings {
        controller.event(Some(state_of(pending)), "cancel", obj(vec![]), &quota_text);
    }
    controller.event(Some(state_of(&fifth)), "cancel", obj(vec![]), &quota_text);

    let capture_root = Path::new(workspace).join("capture-quota");
    std::fs::create_dir(&capture_root).unwrap_or_else(|error| panic!("could not create {}: {error}", capture_root.display()));
    let capture_text = capture_root
        .to_str()
        .unwrap_or_else(|| panic!("capture path is not UTF-8"))
        .to_string();
    let recording = controller.event(None, "media_record", obj(vec![]), &capture_text);
    let capture_path = PathBuf::from(must_str(must_get(&recording, "effect"), "capture_path"));
    let mut captures = Vec::new();
    for index in 0..4 {
        let (pending, _, stream) = opening(controller, None, "source.pdf", &capture_text);
        let expect = if index < 3 { 64 * MIB } else { 62 * MIB };
        keep(passed, &format!("recording reservation is respected {index}"), same_u64(&stream, "limit", expect));
        captures.push(pending);
    }
    write_bytes(&capture_path, &vec![b'r'; (2 * MIB) as usize]);
    keep(
        passed,
        "native recording can fill its reserved bound without overbooking",
        dir_size_sum(&capture_root.join("service-files")) == 256 * MIB,
    );
    controller.event(Some(state_of(&recording)), "cancel", obj(vec![]), &capture_text);
    for pending in &captures {
        controller.event(Some(state_of(pending)), "cancel", obj(vec![]), &capture_text);
    }
    let recorded = std::fs::metadata(&capture_path)
        .unwrap_or_else(|error| panic!("could not stat {}: {error}", capture_path.display()))
        .len();
    keep(passed, "recording cancellation retains recorded bytes", recorded == 2 * MIB);

    let chunk: Vec<u8> = (0..8192).map(|index| (index % 251) as u8).collect();
    let encoded_chunk = b64_encode(&chunk);
    let times = (MAXIMUM as usize) / chunk.len();
    let maximum_digest = repeated_digest(&chunk, times);
    let maximum_state = config(controller, None, "binary", None, workspace, peer.port);
    let (maximum_open, maximum_owner, _) = opening(controller, Some(&maximum_state), "maximum.bin", workspace);
    for _ in 0..times {
        controller.op(&overlay(
            &maximum_owner,
            vec![("op", s("media_input_feed")), ("data", s(&encoded_chunk))],
        ));
    }
    let maximum_receipt = controller.op(&overlay(&maximum_owner, vec![("op", s("media_input_finish"))]));
    let ready = controller.event(
        Some(state_of(&maximum_open)),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), must_clone(&maximum_owner, "media_ticket")),
            ("media_receipt".into(), maximum_receipt),
        ]),
        workspace,
    );
    let adopted = must_get(state_of(&ready), "media_input");
    keep(
        passed,
        "exact 64 MiB input is fully adopted",
        same_u64(adopted, "bytes", MAXIMUM) && same_str(adopted, "sha256", &maximum_digest),
    );
    let maximum_approved = approved(controller, state_of(&ready), None, workspace);
    exchange(controller, &maximum_approved, workspace, passed);
    let maximum_body = peer.with_last(|hit| {
        hit.body.len() == MAXIMUM as usize && twin::sha256_bytes(&hit.body) == maximum_digest
    });
    keep(passed, "maximum binary HTTP upload has exact length and digest", maximum_body);

    let overflow_state = config(controller, None, "binary", None, workspace, peer.port);
    let (overflow_open, overflow_owner, _) = opening(controller, Some(&overflow_state), "overflow.bin", workspace);
    for _ in 0..times {
        controller.op(&overlay(
            &overflow_owner,
            vec![("op", s("media_input_feed")), ("data", s(&encoded_chunk))],
        ));
    }
    let refused = controller.send(&overlay(
        &overflow_owner,
        vec![("op", s("media_input_feed")), ("data", s(&b64_encode(b"x")))],
    ));
    // Python `and` does not call finish when the extra byte was accepted.
    let overflow_ok = if value_absent(&refused) {
        value_absent(&controller.send(&overlay(&overflow_owner, vec![("op", s("media_input_finish"))])))
    } else {
        false
    };
    keep(passed, "one byte beyond maximum cannot become a truncated success", overflow_ok);
    controller.event(Some(state_of(&overflow_open)), "cancel", obj(vec![]), workspace);

    write_bytes(&Path::new(workspace).join("local.txt"), b"ordinary local result");
    let local = controller.op(&obj(vec![
        ("op".into(), s("tool")),
        ("tool".into(), s("read")),
        ("root".into(), s(workspace)),
        ("args".into(), obj(vec![("path".into(), s("local.txt"))])),
    ]));
    keep(passed, "artifact-free local operation survives scoped upload failure", local == s("ordinary local result"));
}

fn refuse_other_process(project: &str, quota: &str, passed: &mut Vec<String>) {
    let mut child = Controller::spawn(project);
    let pick = child.send(&obj(vec![
        ("op".into(), s("event")),
        ("action".into(), s("media_pick")),
        ("root".into(), s(quota)),
        ("inputs".into(), obj(vec![])),
    ]));
    let picked = pick.get("value").cloned().unwrap_or_else(|| panic!("child media_pick has no value: {}", pick.to_string()));
    let refused = child.send(&obj(vec![
        ("op".into(), s("media_input_open")),
        ("media_ticket".into(), must_clone(must_get(&picked, "effect"), "media_ticket")),
        ("name".into(), s("other-process.bin")),
    ]));
    let ok = value_absent(&refused);
    child.finish();
    keep(passed, "shared quota also refuses an independent process", ok);
}

fn config(
    controller: &mut Controller,
    state: Option<&Value>,
    format_name: &str,
    request: Option<Value>,
    workspace: &str,
    port: u16,
) -> Value {
    let cfg = obj(vec![
        ("version".into(), twin::n("1")),
        (
            "hosts".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("server")),
                ("url".into(), s(&format!("http://127.0.0.1:{port}"))),
                ("plain".into(), twin::b(true)),
                ("enabled".into(), twin::b(true)),
            ])]),
        ),
        (
            "routes".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("upload")),
                ("host".into(), s("server")),
                ("capability".into(), s("document_processing")),
                ("method".into(), s("POST")),
                ("path".into(), s("/upload")),
                ("request_format".into(), s(format_name)),
                ("request".into(), request.unwrap_or_else(|| obj(vec![]))),
                ("file_field".into(), s("source")),
                ("response".into(), s("json")),
                ("response_path".into(), s("")),
                ("enabled".into(), twin::b(true)),
            ])]),
        ),
        ("defaults".into(), obj(vec![])),
    ]);
    let current = match state {
        Some(state) => state.clone(),
        None => state_of(&controller.event(None, "resume", obj(vec![]), workspace)).clone(),
    };
    let connections = controller.event(Some(&current), "service_connections", obj(vec![]), workspace);
    let saved = controller.event(
        Some(state_of(&connections)),
        "service_save",
        obj(vec![("service_config".into(), s(&python_dumps(&cfg)))]),
        workspace,
    );
    let saved_state = state_of(&saved);
    if saved_state.get("notice_error").is_some_and(twin::truthy) {
        panic!("service_save notice_error: {}", saved_state.get("notice").map(Value::to_string).unwrap_or_default());
    }
    saved_state.clone()
}

fn opening(controller: &mut Controller, state: Option<&Value>, name: &str, workspace: &str) -> (Value, Value, Value) {
    let reply = controller.event(state, "media_pick", obj(vec![]), workspace);
    let media_ticket = must_clone(must_get(&reply, "effect"), "media_ticket");
    let stream = controller.op(&obj(vec![
        ("op".into(), s("media_input_open")),
        ("media_ticket".into(), media_ticket.clone()),
        ("name".into(), s(name)),
    ]));
    let owner = obj(vec![
        ("media_ticket".into(), media_ticket),
        ("ticket".into(), must_clone(&stream, "ticket")),
    ]);
    (reply, owner, stream)
}

fn selected(controller: &mut Controller, state: &Value, data: &[u8], name: &str, workspace: &str) -> (Value, Value, Value) {
    let (reply, owner, _) = opening(controller, Some(state), name, workspace);
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + 8192).min(data.len());
        let encoded = b64_encode(&data[offset..end]);
        controller.op(&overlay(&owner, vec![("op", s("media_input_feed")), ("data", s(&encoded))]));
        offset = end;
    }
    let receipt = controller.op(&overlay(&owner, vec![("op", s("media_input_finish"))]));
    let done = controller.event(
        Some(state_of(&reply)),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), must_clone(&owner, "media_ticket")),
            ("media_receipt".into(), receipt.clone()),
        ]),
        workspace,
    );
    if !state_of(&done).get("media_input").is_some_and(twin::truthy) {
        panic!(
            "media input was not stored: {}",
            state_of(&done).get("notice").map(Value::to_string).unwrap_or_default()
        );
    }
    (done, owner, receipt)
}

fn approved(controller: &mut Controller, state: &Value, input: Option<Value>, workspace: &str) -> Value {
    let requested = controller.event(Some(state), "service_request", obj(vec![]), workspace);
    let payload = input.unwrap_or_else(|| obj(vec![]));
    let reviewed = controller.event(
        Some(state_of(&requested)),
        "service_review",
        obj(vec![
            ("service_route".into(), s("upload")),
            ("service_file".into(), twin::b(true)),
            ("service_input".into(), s(&python_dumps(&payload))),
        ]),
        workspace,
    );
    if state_of(&reviewed).get("dialog").and_then(as_str) != Some("approval") {
        panic!(
            "expected approval dialog: {}",
            state_of(&reviewed).get("notice").map(Value::to_string).unwrap_or_default()
        );
    }
    let approved = controller.event(Some(state_of(&reviewed)), "approve", obj(vec![]), workspace);
    if !approved.get("effect").is_some_and(twin::truthy) {
        panic!(
            "approve produced no effect: {}",
            state_of(&approved).get("notice").map(Value::to_string).unwrap_or_default()
        );
    }
    approved
}

fn exchange(controller: &mut Controller, reply: &Value, workspace: &str, passed: &mut Vec<String>) -> Value {
    let effect = must_get(reply, "effect");
    let name = format!("file request envelope stays bounded {}", passed.len());
    let bounded = python_dumps(effect).len() < 65536 && effect.get("media_upload").is_some() && effect.get("wire_base64").is_none();
    keep(passed, &name, bounded);
    // The leg sends plain `wire` text. wire_base64 is a different carrier and is absent here.
    let wire = must_str(effect, "wire").as_bytes().to_vec();
    let host = must_str(effect, "host").to_string();
    let port = as_port(must_get(effect, "port"));
    let purpose = must_clone(effect, "purpose");
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("could not connect to {host}:{port}: {error}"));
    stream.set_nodelay(true).unwrap_or_else(|error| panic!("nodelay: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(180))).unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream.set_write_timeout(Some(Duration::from_secs(180))).unwrap_or_else(|error| panic!("write timeout: {error}"));
    stream.write_all(&wire).and_then(|_| stream.flush()).unwrap_or_else(|error| panic!("could not send request: {error}"));
    upload(controller, effect, &mut stream);
    let response = read_to_end(&mut stream);
    controller.event(
        Some(state_of(reply)),
        "network_result",
        obj(vec![("purpose".into(), purpose), ("data".into(), s(&b64_encode(&response)))]),
        workspace,
    )
}

fn upload(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) {
    if !effect.get("media_upload").is_some_and(twin::truthy) {
        return;
    }
    let owner = obj(vec![
        ("purpose".into(), must_clone(effect, "purpose")),
        ("ticket".into(), must_clone(effect, "media_upload")),
    ]);
    let limit = as_u64(must_get(effect, "upload_bytes")).unwrap_or_else(|| panic!("upload_bytes is not an integer"));
    let mut failure = None;
    let mut count = 0u64;
    loop {
        let reply = controller.send(&overlay(&owner, vec![("op", s("media_upload_read"))]));
        let Some(part) = reply.get("value").cloned() else {
            failure = Some(format!("media_upload_read failed: {}", reply.to_string()));
            break;
        };
        let Some(Value::Str(encoded)) = part.get("data") else {
            failure = Some(format!("media_upload_read data is missing: {}", part.to_string()));
            break;
        };
        let data = b64_decode(encoded);
        if data.len() > 8192 {
            failure = Some(format!("upload chunk {} exceeds 8192", data.len()));
            break;
        }
        count += data.len() as u64;
        if count > limit {
            failure = Some(format!("upload count {count} exceeds {limit}"));
            break;
        }
        if let Err(error) = stream.write_all(&data) {
            failure = Some(format!("upload write: {error}"));
            break;
        }
        let Some(done) = part.get("done") else {
            failure = Some(format!("upload part missing done: {}", part.to_string()));
            break;
        };
        if twin::truthy(done) {
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
    let cancel = controller.send(&overlay(&owner, vec![("op", s("media_upload_cancel"))]));
    if cancel.get("value").is_none() {
        panic!("media_upload_cancel failed: {}", cancel.to_string());
    }
    if let Some(error) = failure {
        panic!("upload: {error}");
    }
}

fn file_map(kind: &str) -> Value {
    obj(vec![("$file".into(), s(kind))])
}

fn pdf_fixture() -> Vec<u8> {
    let stream = "% padding\n".repeat(320_000).into_bytes();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_vec(),
    ];
    let mut fourth = Vec::new();
    fourth.extend_from_slice(b"<< /Length ");
    fourth.extend_from_slice(stream.len().to_string().as_bytes());
    fourth.extend_from_slice(b" >>\nstream\n");
    fourth.extend_from_slice(&stream);
    fourth.extend_from_slice(b"endstream");
    objects.push(fourth);
    let mut data = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, body) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice((index + 1).to_string().as_bytes());
        data.extend_from_slice(b" 0 obj\n");
        data.extend_from_slice(body);
        data.extend_from_slice(b"\nendobj\n");
    }
    let xref = data.len();
    data.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in &offsets[1..] {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n");
    data.extend_from_slice(xref.to_string().as_bytes());
    data.extend_from_slice(b"\n%%EOF\n");
    data
}

fn repeated_digest(chunk: &[u8], times: usize) -> String {
    let mut bytes = Vec::with_capacity(chunk.len().saturating_mul(times));
    for _ in 0..times {
        bytes.extend_from_slice(chunk);
    }
    twin::sha256_bytes(&bytes)
}

fn multipart_matches(headers: &[(String, String)], body: &[u8], data: &[u8]) -> bool {
    let content_type = header_exact(headers, "Content-Type");
    let fields = multipart_fields(content_type, body);
    let source = field_bytes(&fields, "source");
    let language = field_bytes(&fields, "language");
    let encoded = field_bytes(&fields, "encoded");
    let detail = field_bytes(&fields, "detail");
    let encoded_text = std::str::from_utf8(encoded).unwrap_or_else(|_| panic!("encoded part is not UTF-8"));
    let detail_text = std::str::from_utf8(detail).unwrap_or_else(|_| panic!("detail part is not UTF-8"));
    let detail_value = Value::parse(detail_text.trim());
    let expected_detail = obj(vec![("name".into(), s("source.pdf"))]);
    source == data && language == b"en" && b64_decode(encoded_text) == data && detail_value == expected_detail
}

fn multipart_fields(content_type: &str, body: &[u8]) -> Vec<(String, Vec<u8>)> {
    let boundary = header_param(content_type, "boundary").unwrap_or_else(|| panic!("multipart content type has no boundary: {content_type}"));
    let marker = format!("--{boundary}").into_bytes();
    if !body.starts_with(&marker) {
        panic!("multipart body does not start with its boundary");
    }
    let mut rest = body;
    let mut fields = Vec::new();
    loop {
        // Each part, including the first, begins at `--boundary`. The epilogue is `--boundary--`.
        if !rest.starts_with(&marker) {
            panic!("multipart parser lost the boundary");
        }
        rest = &rest[marker.len()..];
        if rest.starts_with(b"--") {
            break;
        }
        if rest.starts_with(b"\r\n") {
            rest = &rest[2..];
        } else if rest.starts_with(b"\n") {
            rest = &rest[1..];
        } else {
            panic!("multipart boundary is not followed by a line ending");
        }
        let (separator, separator_len) = if let Some(index) = find_bytes(rest, b"\r\n\r\n") {
            (index, 4)
        } else if let Some(index) = find_bytes(rest, b"\n\n") {
            (index, 2)
        } else {
            panic!("multipart part has no header terminator");
        };
        let header_text = std::str::from_utf8(&rest[..separator]).unwrap_or_else(|_| panic!("multipart headers are not UTF-8"));
        let headers = parse_header_lines(header_text);
        rest = &rest[separator + separator_len..];
        let crlf_marker = join_bytes(b"\r\n", &marker);
        let lf_marker = join_bytes(b"\n", &marker);
        let (end, marker_len) = if let Some(index) = find_bytes(rest, &crlf_marker) {
            (index, crlf_marker.len())
        } else if let Some(index) = find_bytes(rest, &lf_marker) {
            (index, lf_marker.len())
        } else {
            panic!("multipart part has no closing boundary");
        };
        let payload = rest[..end].to_vec();
        // Leave the parser sitting on the next `--boundary` marker.
        let line_len = if marker_len == crlf_marker.len() { 2 } else { 1 };
        rest = &rest[end + line_len..];
        let disposition = header_lookup(&headers, "content-disposition").unwrap_or_else(|| panic!("multipart part has no content-disposition"));
        let name = header_param(disposition, "name").unwrap_or_else(|| panic!("content-disposition has no name: {disposition}"));
        fields.push((name, decode_part(&headers, payload)));
    }
    fields
}

fn decode_part(headers: &[(String, String)], payload: Vec<u8>) -> Vec<u8> {
    let encoding = header_lookup(headers, "content-transfer-encoding").unwrap_or("");
    if encoding.eq_ignore_ascii_case("base64") {
        let text = std::str::from_utf8(&payload).unwrap_or_else(|_| panic!("base64 part is not UTF-8"));
        return b64_decode(text);
    }
    if encoding.eq_ignore_ascii_case("quoted-printable") {
        panic!("quoted-printable part was not produced by the leg's decoder path");
    }
    payload
}

fn parse_header_lines(text: &str) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            panic!("invalid multipart header: {line}");
        };
        headers.push((name.to_string(), value.trim().to_string()));
    }
    headers
}

fn header_lookup<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
}

fn header_param(value: &str, wanted: &str) -> Option<String> {
    for part in split_semicolons(value).skip(1) {
        let part = part.trim();
        let Some((name, raw)) = part.split_once('=') else { continue };
        if name.trim().eq_ignore_ascii_case(wanted) {
            return Some(unquote(raw.trim()));
        }
    }
    None
}

fn split_semicolons(value: &str) -> impl Iterator<Item = &str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => quoted = !quoted,
            b'\\' if quoted => index += 1,
            b';' if !quoted => {
                items.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    items.push(&value[start..]);
    items.into_iter()
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            } else {
                out.push(ch);
            }
        }
        out
    } else {
        value.to_string()
    }
}

fn field_bytes<'a>(fields: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_slice())
        .unwrap_or_else(|| {
            let have = fields.iter().map(|(key, value)| format!("{key}={}", value.len())).collect::<Vec<_>>().join(", ");
            panic!("missing multipart field {name}; have {have}")
        })
}

fn join_bytes(prefix: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(suffix);
    out
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn value_absent(reply: &Value) -> bool {
    reply.get("value").is_none()
}

fn error_contains(reply: &Value, needle: &str) -> bool {
    match reply.get("error") {
        Some(Value::Str(text)) => text.contains(needle),
        Some(other) => panic!("error is not text: {}", other.to_string()),
        None => false,
    }
}

fn service_received(reply: &Value) -> bool {
    let value = must_get(must_get(state_of(reply), "service_result"), "value");
    value == &obj(vec![("received".into(), twin::b(true))])
}

fn same_state(left: &Value, right: &Value) -> bool {
    state_of(left) == state_of(right)
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn must_get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}: {}", value.to_string()))
}

fn must_clone(value: &Value, key: &str) -> Value {
    must_get(value, key).clone()
}

fn must_str<'a>(value: &'a Value, key: &str) -> &'a str {
    let item = must_get(value, key);
    match item {
        Value::Str(text) => text,
        other => panic!("{key} is not text: {}", other.to_string()),
    }
}

fn same_str(value: &Value, key: &str, expected: &str) -> bool {
    match value.get(key) {
        Some(Value::Str(text)) => text == expected,
        Some(_) => false,
        None => panic!("missing {key}"),
    }
}

fn same_u64(value: &Value, key: &str, expected: u64) -> bool {
    match value.get(key) {
        Some(item) => as_u64(item) == Some(expected),
        None => panic!("missing {key}"),
    }
}

fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

fn as_u64(value: &Value) -> Option<u64> {
    let Value::Num(text) = value else { return None };
    if let Ok(number) = text.parse::<u64>() {
        return Some(number);
    }
    let Ok(number) = text.parse::<f64>() else { return None };
    if number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= u64::MAX as f64 {
        Some(number as u64)
    } else {
        None
    }
}

fn as_port(value: &Value) -> u16 {
    let number = as_u64(value).unwrap_or_else(|| panic!("invalid port {}", value.to_string()));
    u16::try_from(number).unwrap_or_else(|_| panic!("port out of range {number}"))
}

fn overlay(base: &Value, updates: Vec<(&str, Value)>) -> Value {
    let Value::Obj(fields) = base else { panic!("expected object, got {}", base.to_string()) };
    let mut fields = fields.clone();
    for (key, value) in updates {
        if let Some((_, slot)) = fields.iter_mut().find(|(name, _)| name == key) {
            *slot = value;
        } else {
            fields.push((key.to_string(), value));
        }
    }
    Value::Obj(fields)
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

fn dir_size_sum(path: &Path) -> u64 {
    let entries = std::fs::read_dir(path).unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
    let mut total = 0u64;
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
        let meta = entry.metadata().unwrap_or_else(|error| panic!("could not stat {}: {error}", entry.path().display()));
        total += meta.len();
    }
    total
}

fn any_small_file_eq(path: &Path, expected: &[u8]) -> bool {
    let entries = std::fs::read_dir(path).unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
        let meta = entry.metadata().unwrap_or_else(|error| panic!("could not stat {}: {error}", entry.path().display()));
        if meta.len() >= 100 {
            continue;
        }
        if read_bytes(&entry.path()) == expected {
            return true;
        }
    }
    false
}

fn read_to_end(stream: &mut TcpStream) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => data.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("socket read: {error}"),
        }
    }
    data
}

struct Hit {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct Peer {
    port: u16,
    received: Arc<Mutex<Vec<Hit>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("could not bind peer: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")).port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_received = Arc::clone(&received);
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                let (stream, _) = match listener.accept() {
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
                let connection_received = Arc::clone(&thread_received);
                std::thread::spawn(move || {
                    if let Err(error) = serve(stream, &connection_received) {
                        panic!("peer request: {error}");
                    }
                });
            }
        });
        Self { port, received, stop, thread: Some(thread) }
    }

    fn with_last<T>(&self, body: impl FnOnce(&Hit) -> T) -> T {
        let guard = self.received.lock().unwrap_or_else(|error| panic!("peer lock: {error}"));
        let hit = guard.last().unwrap_or_else(|| panic!("peer received no request"));
        body(hit)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                panic!("peer thread panicked");
            }
        }
    }
}

fn serve(mut stream: TcpStream, received: &Mutex<Vec<Hit>>) -> Result<(), String> {
    stream.set_nodelay(true).map_err(|error| format!("nodelay: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(300))).map_err(|error| format!("read timeout: {error}"))?;
    stream.set_write_timeout(Some(Duration::from_secs(60))).map_err(|error| format!("write timeout: {error}"))?;
    let request = read_request(&mut stream)?;
    received.lock().map_err(|error| format!("peer lock: {error}"))?.push(request);
    let body = b"{\"received\":true}";
    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body)).and_then(|_| stream.flush()).map_err(|error| format!("response write: {error}"))
}

fn read_request(stream: &mut TcpStream) -> Result<Hit, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    let header_end = loop {
        if let Some(position) = find_bytes(&bytes, b"\r\n\r\n") {
            break position;
        }
        if bytes.len() > 65536 {
            return Err("request headers exceed 64 KiB".into());
        }
        let count = read_some(stream, &mut buffer)?;
        if count == 0 {
            return Err("connection closed before request headers".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    };
    let head = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "request headers are not UTF-8".to_string())?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    if request_line.split_whitespace().nth(1).is_none() {
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
    if length > BODY_LIMIT {
        return Err(format!("request body {length} exceeds {BODY_LIMIT}"));
    }
    let mut body = bytes[header_end + 4..].to_vec();
    while body.len() < length {
        let count = read_some(stream, &mut buffer)?;
        if count == 0 {
            return Err(format!("connection closed with {} of {length} body bytes", body.len()));
        }
        body.extend_from_slice(&buffer[..count]);
    }
    body.truncate(length);
    Ok(Hit { headers, body })
}

fn read_some(stream: &mut TcpStream, buffer: &mut [u8]) -> Result<usize, String> {
    loop {
        match stream.read(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("socket read: {error}")),
        }
    }
}

fn header_exact<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()).unwrap_or_else(|| panic!("missing header {name}"))
}

fn python_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Num(token) => token.clone(),
        Value::Str(text) => python_quote(text),
        Value::Arr(items) => {
            let body = items.iter().map(python_dumps).collect::<Vec<_>>().join(", ");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let body = fields.iter().map(|(key, item)| format!("{}: {}", python_quote(key), python_dumps(item))).collect::<Vec<_>>().join(", ");
            format!("{{{body}}}")
        }
    }
}

fn python_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch if (ch as u32) <= 0x7f => out.push(ch),
            ch => {
                let code = ch as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let surrogate = code - 0x10000;
                    out.push_str(&format!("\\u{:04x}\\u{:04x}", 0xd800 + (surrogate >> 10), 0xdc00 + (surrogate & 0x3ff)));
                }
            }
        }
    }
    out.push('"');
    out
}

fn parse_json_unlimited(bytes: &[u8]) -> Value {
    let mut parser = JsonParser { bytes, index: 0 };
    let value = parser.value(0).unwrap_or_else(|error| panic!("invalid JSON ({error})"));
    parser.skip();
    if parser.index != parser.bytes.len() {
        panic!("invalid JSON (trailing data)");
    }
    value
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> JsonParser<'a> {
    fn skip(&mut self) {
        while self.index < self.bytes.len() && matches!(self.bytes[self.index], b' ' | b'\n' | b'\r' | b'\t') {
            self.index += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.skip();
        if self.bytes.get(self.index) == Some(&byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 128 {
            return Err("JSON nesting limit exceeded".into());
        }
        self.skip();
        match self.bytes.get(self.index).copied() {
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err("invalid JSON value".into()),
        }
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Result<Value, String> {
        if self.bytes.get(self.index..self.index + word.len()) != Some(word) {
            return Err("invalid literal".into());
        }
        self.index += word.len();
        Ok(value)
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        self.index += 1;
        let mut fields = Vec::new();
        if self.eat(b'}') {
            return Ok(Value::Obj(fields));
        }
        loop {
            let key = self.string()?;
            if !self.eat(b':') {
                return Err("missing colon".into());
            }
            let value = self.value(depth + 1)?;
            if fields.iter().any(|(name, _)| name == &key) {
                return Err("duplicate JSON field".into());
            }
            fields.push((key, value));
            if self.eat(b'}') {
                return Ok(Value::Obj(fields));
            }
            if !self.eat(b',') {
                return Err("missing comma".into());
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        self.index += 1;
        let mut items = Vec::new();
        if self.eat(b']') {
            return Ok(Value::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            if self.eat(b']') {
                return Ok(Value::Arr(items));
            }
            if !self.eat(b',') {
                return Err("missing comma".into());
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.index;
        if self.bytes[self.index] == b'-' {
            self.index += 1;
        }
        match self.bytes.get(self.index) {
            Some(b'0') => self.index += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err("invalid number".into()),
        }
        if self.bytes.get(self.index) == Some(&b'.') {
            self.index += 1;
            let fraction = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == fraction {
                return Err("missing fraction".into());
            }
        }
        if matches!(self.bytes.get(self.index), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.bytes.get(self.index), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exponent = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exponent {
                return Err("missing exponent".into());
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "invalid number")?;
        Ok(Value::Num(token.to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat(b'"') {
            return Err("expected string".into());
        }
        let mut out = String::new();
        loop {
            let byte = *self.bytes.get(self.index).ok_or("unterminated string")?;
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = *self.bytes.get(self.index).ok_or("truncated escape")?;
                    self.index += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => out.push(escaped as char),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err("invalid escape".into()),
                    }
                }
                0..=31 => return Err("unescaped control character".into()),
                _ => {
                    let width = utf8_width(byte).ok_or("invalid UTF-8")?;
                    let start = self.index - 1;
                    let end = start + width;
                    if end > self.bytes.len() {
                        return Err("truncated UTF-8".into());
                    }
                    let text = std::str::from_utf8(&self.bytes[start..end]).map_err(|_| "invalid UTF-8")?;
                    out.push_str(text);
                    self.index = end;
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let mut unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.bytes.get(self.index..self.index + 2) != Some(br"\u") {
                return Err("missing surrogate".into());
            }
            self.index += 2;
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err("invalid surrogate".into());
            }
            unit = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            return Err("invalid surrogate".into());
        }
        char::from_u32(unit).ok_or_else(|| "invalid unicode scalar".into())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.index + 4 > self.bytes.len() {
            return Err("short unicode escape".into());
        }
        let text = std::str::from_utf8(&self.bytes[self.index..self.index + 4]).map_err(|_| "invalid unicode")?;
        self.index += 4;
        u32::from_str_radix(text, 16).map_err(|_| "invalid unicode escape".into())
    }
}

fn utf8_width(byte: u8) -> Option<usize> {
    if byte < 0x80 {
        Some(1)
    } else if byte & 0xE0 == 0xC0 {
        Some(2)
    } else if byte & 0xF0 == 0xE0 {
        Some(3)
    } else if byte & 0xF8 == 0xF0 {
        Some(4)
    } else {
        None
    }
}

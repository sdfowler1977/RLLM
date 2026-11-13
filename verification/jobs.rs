//! Native verification twin for asynchronous HTTP jobs.
mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner asynchronous HTTP fixture tests. Explicit polling; no background scheduler or live user generation service acceptance.";
const REPORT: &str = "output/checks/jobs.json";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch_path = twin::create_scratch(&root, "jobs");
    let scratch = Scratch(scratch_path);
    let root_text = scratch.path().to_string();
    let mut fixture = Fixture::start();
    let mut controller = Controller::spawn(root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")));
    let passed = run(&mut controller, &root_text, &mut fixture);
    controller.finish();
    fixture.stop();

    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("jobs", &body, true);
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if written != format!("{}\n", twin::pretty(&body)) {
        panic!("{REPORT} has unexpected contents");
    }
    println!("{} jobs checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str, fixture: &mut Fixture) -> Vec<String> {
    let mut passed = Vec::new();
    let mapping = obj(vec![
        ("id".into(), s("/id")),
        ("state".into(), s("/state")),
        ("success".into(), Value::Arr(vec![s("done")])),
        ("failure".into(), Value::Arr(vec![s("failed")])),
        ("status_route".into(), s("status")),
        ("result_route".into(), s("result")),
        ("cancel_route".into(), s("cancel")),
    ]);
    let mut routes = vec![obj(vec![
        ("id".into(), s("submit")),
        ("host".into(), s("peer")),
        ("capability".into(), s("video_generation")),
        ("method".into(), s("POST")),
        ("path".into(), s("/generate")),
        ("response".into(), s("json")),
        ("response_path".into(), s("")),
        ("enabled".into(), twin::b(true)),
        ("job".into(), mapping),
    ])];
    for action in ["status", "result", "cancel"] {
        routes.push(obj(vec![
            ("id".into(), s(action)),
            ("host".into(), s("peer")),
            ("capability".into(), s(&format!("job_{action}"))),
            ("method".into(), s(if action == "cancel" { "POST" } else { "GET" })),
            ("path".into(), s(&format!("/{action}/{{job}}"))),
            ("response".into(), s(if action == "result" { "binary" } else { "json" })),
            ("response_path".into(), s("")),
            ("artifact_kind".into(), s(if action == "result" { "audio" } else { "data" })),
            ("enabled".into(), twin::b(true)),
        ]));
    }
    let config = obj(vec![
        ("version".into(), twin::n("1")),
        (
            "hosts".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("peer")),
                ("url".into(), s(&format!("http://127.0.0.1:{}", fixture.port()))),
                ("plain".into(), twin::b(true)),
                ("enabled".into(), twin::b(true)),
            ])]),
        ),
        ("routes".into(), Value::Arr(routes)),
        ("defaults".into(), obj(vec![])),
    ]);

    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let connected = controller.event(Some(state(&resumed)), "service_connections", obj(vec![]), root);
    let saved = controller.event(
        Some(state(&connected)),
        "service_save",
        obj(vec![("service_config".into(), s(&config.to_string()))]),
        root,
    );
    assert_saved(&saved, "initial service_save");
    let requested = controller.event(Some(state(&saved)), "service_request", obj(vec![]), root);
    let reviewed = controller.event(
        Some(state(&requested)),
        "service_review",
        obj(vec![
            ("service_route".into(), s("submit")),
            ("service_input".into(), s("{\"prompt\":\"fixture generation\"}")),
        ]),
        root,
    );
    let approved = controller.event(Some(state(&reviewed)), "approve", obj(vec![]), root);
    let accepted = transport(controller, &approved, root);
    let job = job_at(&accepted, 0);
    keep(
        &mut passed,
        "202 tracks actual server job ID",
        text(&job, "server_id") == "server-job" && text(&job, "state") == "accepted",
        &format!("server_id={} state={}", field_debug(&job, "server_id"), field_debug(&job, "state")),
    );
    keep(
        &mut passed,
        "acceptance does not claim artifact completion",
        nested(&accepted, "service_result", "status") == "accepted" && !py_true(job.get("artifact")),
        &format!(
            "status={} artifact={}",
            field_debug(state(&accepted).get("service_result").unwrap_or(&Value::Null), "status"),
            field_debug(&job, "artifact")
        ),
    );

    let count = fixture.calls().len();
    let resumed = controller.event(Some(state(&accepted)), "resume", obj(vec![]), root);
    keep(
        &mut passed,
        "resume preserves job without resubmission",
        job_rows(&resumed) == vec![job.clone()] && fixture.calls().len() == count && effect_is_none(&resumed),
        &format!(
            "jobs={} calls={} effect={}",
            job_rows(&resumed).len(),
            fixture.calls().len(),
            resumed.get("effect").map(|value| value.to_string()).unwrap_or_else(|| "absent".into())
        ),
    );

    let viewed = controller.event(
        Some(state(&resumed)),
        &format!("job_view:{}", text(&job, "id")),
        obj(vec![]),
        root,
    );
    let status_request = request(controller, &viewed, "status", root);
    let status = transport(controller, &status_request, root);
    let status_job = job_at(&status, 0);
    keep(
        &mut passed,
        "status uses captured ID and separate route",
        fixture.last_path() == "/status/server-job" && text(&status_job, "state") == "running",
        &format!("path={} state={}", fixture.last_path(), field_debug(&status_job, "state")),
    );

    fixture.set_phase("done");
    let status_request = request(controller, &status, "status", root);
    let ready = transport(controller, &status_request, root);
    let ready_job = job_at(&ready, 0);
    keep(
        &mut passed,
        "server success remains ready until actual file exists",
        text(&ready_job, "state") == "ready" && !py_true(ready_job.get("artifact")),
        &format!("state={} artifact={}", field_debug(&ready_job, "state"), field_debug(&ready_job, "artifact")),
    );

    let result_request = request(controller, &ready, "result", root);
    let completed_event = transport(controller, &result_request, root);
    let record = job_at(&completed_event, 0);
    let artifact = record.get("artifact").cloned().unwrap_or(Value::Null);
    let artifact_path = Path::new(root).join(text(&artifact, "path"));
    let bytes = std::fs::read(&artifact_path).unwrap_or_else(|error| panic!("artifact {}: {error}", artifact_path.display()));
    keep(
        &mut passed,
        "only validated returned file completes job",
        text(&record, "state") == "completed"
            && text(&artifact, "sha256") == twin::sha256_bytes(&bytes)
            && bytes == fixture.artifact(),
        &format!(
            "state={} sha256={} bytes={} artifact={}",
            field_debug(&record, "state"),
            field_debug(&artifact, "sha256"),
            bytes.len(),
            fixture.artifact().len()
        ),
    );
    let generations = fixture.calls().iter().filter(|(_, path, _)| path == "/generate").count();
    keep(
        &mut passed,
        "status and result never repeat generation",
        generations == 1,
        &format!("generations={generations}"),
    );

    let cancel_request = request(controller, &completed_event, "cancel", root);
    let cancelled = transport(controller, &cancel_request, root);
    let cancelled_job = job_at(&cancelled, 0);
    keep(
        &mut passed,
        "202 cancellation is a request rather than proof",
        text(&cancelled_job, "state") == "cancel_requested",
        &format!("state={}", field_debug(&cancelled_job, "state")),
    );

    let changed = changed_origin(state(&cancelled));
    let blocked = controller.event(Some(&changed), "job_status", obj(vec![]), root);
    keep(
        &mut passed,
        "changed origin cannot receive tracked server ID",
        effect_is_none(&blocked) && py_true(state(&blocked).get("notice_error")),
        &format!(
            "effect={} notice_error={} notice={}",
            blocked.get("effect").map(|value| value.to_string()).unwrap_or_else(|| "absent".into()),
            field_debug(state(&blocked), "notice_error"),
            field_debug(state(&blocked), "notice")
        ),
    );

    let before = job_at(&cancelled, 0);
    let started = request(controller, &cancelled, "status", root);
    let failed = controller.event(
        Some(state(&started)),
        "failure",
        obj(vec![
            ("purpose".into(), s(&require_str(effect_of(&started), "purpose"))),
            ("sent".into(), twin::b(true)),
            ("error".into(), s("fixture interruption")),
        ]),
        root,
    );
    let after = job_at(&failed, 0);
    keep(
        &mut passed,
        "failed poll preserves known server state",
        text(&after, "state") == text(&before, "state")
            && text(&after, "server_id") == text(&before, "server_id")
            && py_true(after.get("last_error")),
        &format!(
            "before={} after={} server_id={} last_error={}",
            field_debug(&before, "state"),
            field_debug(&after, "state"),
            field_debug(&after, "server_id"),
            field_debug(&after, "last_error")
        ),
    );

    let archived = controller.event(Some(state(&failed)), "job_archive", obj(vec![]), root);
    let archived_job = job_at(&archived, 0);
    keep(
        &mut passed,
        "job archive is reversible",
        present_and_true(&archived_job, "archived"),
        &format!("archived={}", field_debug(&archived_job, "archived")),
    );
    let restored = controller.event(Some(state(&archived)), "job_restore", obj(vec![]), root);
    let restored_job = job_at(&restored, 0);
    keep(
        &mut passed,
        "job history restore preserves identity",
        present_and_false(&restored_job, "archived") && text(&restored_job, "server_id") == "server-job",
        &format!(
            "archived={} server_id={}",
            field_debug(&restored_job, "archived"),
            field_debug(&restored_job, "server_id")
        ),
    );

    fixture.set_acceptance(200);
    fixture.set_phase("queued");
    let mapped = mapped_config(&config);
    let connected = controller.event(Some(state(&restored)), "service_connections", obj(vec![]), root);
    let saved = controller.event(
        Some(state(&connected)),
        "service_save",
        obj(vec![("service_config".into(), s(&mapped.to_string()))]),
        root,
    );
    assert_saved(&saved, "mapped service_save");
    let requested = controller.event(Some(state(&saved)), "service_request", obj(vec![]), root);
    let reviewed = controller.event(
        Some(state(&requested)),
        "service_review",
        obj(vec![
            ("service_route".into(), s("submit")),
            ("service_input".into(), s("{}")),
        ]),
        root,
    );
    let approved = controller.event(Some(state(&reviewed)), "approve", obj(vec![]), root);
    let accepted = transport(controller, &approved, root);
    let mapped_job = job_last(&accepted);
    keep(
        &mut passed,
        "configured HTTP 200 submission tracks acceptance",
        text(&mapped_job, "state") == "accepted" && nested(&accepted, "service_result", "status") == "accepted",
        &format!(
            "state={} status={}",
            field_debug(&mapped_job, "state"),
            field_debug(state(&accepted).get("service_result").unwrap_or(&Value::Null), "status")
        ),
    );

    let viewed = controller.event(
        Some(state(&accepted)),
        &format!("job_view:{}", text(&mapped_job, "id")),
        obj(vec![]),
        root,
    );
    fixture.set_phase_bool(true);
    let status_request = request(controller, &viewed, "status", root);
    let boolean_ready = transport(controller, &status_request, root);
    let boolean_job = job_last(&boolean_ready);
    keep(
        &mut passed,
        "boolean terminal status retains its type",
        text(&boolean_job, "state") == "ready" && boolean_job.get("server_state") == Some(&twin::b(true)),
        &format!(
            "state={} server_state={}",
            field_debug(&boolean_job, "state"),
            field_debug(&boolean_job, "server_state")
        ),
    );

    let result_request = request(controller, &boolean_ready, "result", root);
    let mapped_result = transport(controller, &result_request, root);
    let mapped_record = job_last(&mapped_result);
    keep(
        &mut passed,
        "result path maps actual status response field",
        fixture.last_path() == "/result/server-job" && text(&mapped_record, "state") == "completed",
        &format!("path={} state={}", fixture.last_path(), field_debug(&mapped_record, "state")),
    );

    let untouched = job_at(&mapped_result, 0);
    let before_calls = fixture.calls().len();
    let refused = controller.event(Some(state(&mapped_result)), "job_forget_confirm", obj(vec![]), root);
    keep(
        &mut passed,
        "job record removal requires selected review",
        py_true(state(&refused).get("notice_error")) && job_rows(&refused).len() == 2,
        &format!(
            "notice_error={} jobs={} notice={}",
            field_debug(state(&refused), "notice_error"),
            job_rows(&refused).len(),
            field_debug(state(&refused), "notice")
        ),
    );

    let forgotten = controller.event(Some(state(&mapped_result)), "job_forget", obj(vec![]), root);
    let confirmed = controller.event(Some(state(&forgotten)), "job_forget_confirm", obj(vec![]), root);
    let retained = if mapped_job.get("artifact").is_some() {
        mapped_job.get("artifact").cloned().unwrap_or(Value::Null)
    } else {
        record.get("artifact").cloned().unwrap_or_else(|| panic!("missing retained artifact"))
    };
    let retained_path = Path::new(root).join(text(&retained, "path"));
    keep(
        &mut passed,
        "reviewed job removal preserves other records and local artifact",
        job_rows(&confirmed) == vec![untouched.clone()]
            && fixture.calls().len() == before_calls
            && retained_path.exists(),
        &format!(
            "jobs={} calls={} exists={} path={}",
            job_rows(&confirmed).len(),
            fixture.calls().len(),
            retained_path.exists(),
            retained_path.display()
        ),
    );

    let malformed = missing_identifier(&mapped);
    let connected = controller.event(Some(state(&confirmed)), "service_connections", obj(vec![]), root);
    let saved = controller.event(
        Some(state(&connected)),
        "service_save",
        obj(vec![("service_config".into(), s(&malformed.to_string()))]),
        root,
    );
    let requested = controller.event(Some(state(&saved)), "service_request", obj(vec![]), root);
    let reviewed = controller.event(
        Some(state(&requested)),
        "service_review",
        obj(vec![
            ("service_route".into(), s("submit")),
            ("service_input".into(), s("{}")),
        ]),
        root,
    );
    let before_calls = fixture.calls().len();
    let submitted = controller.event(Some(state(&reviewed)), "approve", obj(vec![]), root);
    let unconfirmed = transport(controller, &submitted, root);
    keep(
        &mut passed,
        "HTTP 200 acceptance without usable identifier remains unconfirmed without replay",
        nested(&unconfirmed, "service_result", "status") == "accepted_unconfirmed"
            && fixture.calls().len() == before_calls + 1,
        &format!(
            "status={} calls={} before={}",
            field_debug(state(&unconfirmed).get("service_result").unwrap_or(&Value::Null), "status"),
            fixture.calls().len(),
            before_calls
        ),
    );

    passed
}

fn request(controller: &mut Controller, event: &Value, action: &str, root: &str) -> Value {
    let requested = controller.event(Some(state(event)), &format!("job_{action}"), obj(vec![]), root);
    let dialog = state(&requested).get("dialog").map(Value::to_string).unwrap_or_else(|| "absent".into());
    if state(&requested).get("dialog") != Some(&s("approval")) {
        panic!(
            "job_{action} did not open approval ({dialog}): {}",
            field_debug(state(&requested), "notice")
        );
    }
    controller.event(Some(state(&requested)), "approve", obj(vec![]), root)
}

fn transport(controller: &mut Controller, event: &Value, root: &str) -> Value {
    let effect = effect_of(event).clone();
    if !twin::truthy(&effect) {
        panic!("missing transport effect: {}", field_debug(state(event), "notice"));
    }
    let host = require_str(&effect, "host");
    let port = as_u16(effect.get("port").unwrap_or_else(|| panic!("missing port")));
    let wire = wire_bytes(&effect);
    let address = format!("{host}:{port}")
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .unwrap_or_else(|| panic!("unresolved transport address {host}:{port}"));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .unwrap_or_else(|error| panic!("connect {address}: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(&wire).unwrap_or_else(|error| panic!("write: {error}"));
    upload(controller, &effect, &mut stream);
    let inputs = response_inputs(controller, &effect, &mut stream);
    controller.event(Some(state(event)), "network_result", inputs, root)
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
            let data = twin::b64_decode(require_str(&part, "data").as_str());
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
    let mut cancel = owner;
    put(&mut cancel, "op", s("media_upload_cancel"));
    let _ = controller.op(&cancel);
    if let Err(error) = outcome {
        resume_unwind(error);
    }
}

fn response_inputs(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) -> Value {
    let purpose = require_str(effect, "purpose");
    let Some(ticket) = nonempty(effect.get("media_download")) else {
        let data = read_all(stream);
        return obj(vec![
            ("purpose".into(), s(&purpose)),
            ("data".into(), s(&twin::b64_encode(&data))),
        ]);
    };
    let _ = controller.op(&obj(vec![
        ("purpose".into(), s(&purpose)),
        ("ticket".into(), s("wrong-ticket")),
        ("op".into(), s("media_stream_cancel")),
    ]));
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
            put(&mut envelope, "data", s(&twin::b64_encode(&chunk)));
            let value = controller.op(&envelope);
            let ready_now = match value.get("ready") {
                Some(item) => twin::truthy(item),
                None => panic!("media_stream_feed missing ready: {}", value.to_string()),
            };
            if ready_now {
                ready = true;
                break;
            }
        }
        let mut envelope = owner.clone();
        put(&mut envelope, "op", s("media_stream_finish"));
        put(&mut envelope, "eof", twin::b(!ready));
        controller.op(&envelope)
    }));
    let mut cancel = owner;
    put(&mut cancel, "op", s("media_stream_cancel"));
    let _ = controller.op(&cancel);
    let receipt = match outcome {
        Ok(value) => value,
        Err(error) => resume_unwind(error),
    };
    obj(vec![
        ("purpose".into(), s(&purpose)),
        ("media_receipt".into(), receipt),
    ])
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool, detail: &str) {
    if !ok {
        panic!("{name}: {detail}");
    }
    passed.push(name.to_string());
}

fn assert_saved(event: &Value, context: &str) {
    if py_true(state(event).get("notice_error")) {
        panic!("{context}: {}", field_debug(state(event), "notice"));
    }
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn effect_of(value: &Value) -> &Value {
    value.get("effect").unwrap_or(&Value::Null)
}

fn effect_is_none(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn job_rows(value: &Value) -> Vec<Value> {
    match state(value).get("service_jobs") {
        Some(Value::Arr(items)) => items.clone(),
        other => panic!("missing service_jobs: {}", other.map(Value::to_string).unwrap_or_else(|| "absent".into())),
    }
}

fn job_at(value: &Value, index: usize) -> Value {
    job_rows(value).get(index).cloned().unwrap_or_else(|| panic!("missing service_jobs[{index}]"))
}

fn job_last(value: &Value) -> Value {
    job_rows(value).last().cloned().unwrap_or_else(|| panic!("missing service_jobs"))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    match value.get(key) {
        Some(Value::Str(text)) => text.as_str(),
        other => panic!("missing string {key}: {}", other.map(Value::to_string).unwrap_or_else(|| "absent".into())),
    }
}

fn nested<'a>(value: &'a Value, object: &str, key: &str) -> &'a str {
    let child = state(value).get(object).unwrap_or_else(|| panic!("missing {object}"));
    text(child, key)
}

fn require_str(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        other => panic!("missing {key}: {}", other.map(Value::to_string).unwrap_or_else(|| "absent".into())),
    }
}

fn nonempty(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Str(text)) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn flag(value: &Value, key: &str) -> Option<bool> {
    match value.get(key) {
        Some(Value::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

fn py_true(value: Option<&Value>) -> bool {
    value.is_some_and(twin::truthy)
}

fn present_and_true(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(item) if twin::truthy(item))
}

fn present_and_false(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(item) if !twin::truthy(item))
}

fn field_debug(value: &Value, key: &str) -> String {
    value.get(key).map(Value::to_string).unwrap_or_else(|| "absent".into())
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

fn wire_bytes(effect: &Value) -> Vec<u8> {
    if let Some(Value::Str(text)) = effect.get("wire_base64") {
        return twin::b64_decode(text);
    }
    if let Some(Value::Str(text)) = effect.get("wire") {
        return text.as_bytes().to_vec();
    }
    panic!("missing wire in {}", effect.to_string());
}

fn clone_value(value: &Value) -> Value {
    Value::parse(&value.to_string())
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    fields.push((key.to_string(), Value::Null));
    &mut fields.last_mut().unwrap().1
}

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
}

fn changed_origin(state: &Value) -> Value {
    let mut changed = clone_value(state);
    let connections = field_mut(&mut changed, "service_connections");
    let hosts = field_mut(connections, "hosts");
    let Value::Arr(items) = hosts else { panic!("service hosts are not an array") };
    put(&mut items[0], "url", s("http://different.invalid"));
    changed
}

fn mapped_config(config: &Value) -> Value {
    let mut mapped = clone_value(config);
    let routes = field_mut(&mut mapped, "routes");
    let Value::Arr(items) = routes else { panic!("routes are not an array") };
    {
        let job = field_mut(&mut items[0], "job");
        put(job, "accept_codes", Value::Arr(vec![twin::n("200")]));
        put(job, "success", Value::Arr(vec![twin::b(true)]));
        put(job, "failure", Value::Arr(vec![twin::b(false)]));
        put(
            job,
            "result_input",
            obj(vec![(
                "asset".into(),
                obj(vec![("$input".into(), s("/asset"))]),
            )]),
        );
    }
    put(&mut items[2], "path", s("/result/{asset}"));
    mapped
}

fn missing_identifier(config: &Value) -> Value {
    let mut malformed = clone_value(config);
    let routes = field_mut(&mut malformed, "routes");
    let Value::Arr(items) = routes else { panic!("routes are not an array") };
    let job = field_mut(&mut items[0], "job");
    put(job, "id", s("/missing"));
    malformed
}

fn read_all(stream: &mut TcpStream) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => data.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
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
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn path(&self) -> &str {
        self.0.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    port: u16,
    artifact: Arc<Vec<u8>>,
    state: Arc<Mutex<FixtureState>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

struct FixtureState {
    acceptance: u16,
    phase: String,
    calls: Vec<(String, String, Vec<u8>)>,
}

impl Fixture {
    fn start() -> Self {
        let artifact = Arc::new(make_artifact());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("local addr: {error}")).port();
        listener.set_nonblocking(true).unwrap_or_else(|error| panic!("nonblocking: {error}"));
        let state = Arc::new(Mutex::new(FixtureState {
            acceptance: 202,
            phase: "\"running\"".to_string(),
            calls: Vec::new(),
        }));
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_artifact = Arc::clone(&artifact);
        let thread = thread::spawn(move || serve(listener, thread_artifact, thread_state, thread_shutdown));
        Self { port, artifact, state, shutdown, thread: Some(thread) }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn artifact(&self) -> Vec<u8> {
        self.artifact.as_ref().clone()
    }

    fn calls(&self) -> Vec<(String, String, Vec<u8>)> {
        self.state.lock().unwrap_or_else(|error| panic!("fixture lock: {error}")).calls.clone()
    }

    fn last_path(&self) -> String {
        self.calls().last().map(|(_, path, _)| path.clone()).unwrap_or_else(|| panic!("fixture received no call"))
    }

    fn set_acceptance(&self, status: u16) {
        self.state.lock().unwrap_or_else(|error| panic!("fixture lock: {error}")).acceptance = status;
    }

    fn set_phase(&self, phase: &str) {
        self.state.lock().unwrap_or_else(|error| panic!("fixture lock: {error}")).phase = json_string(phase);
    }

    fn set_phase_bool(&self, phase: bool) {
        self.state.lock().unwrap_or_else(|error| panic!("fixture lock: {error}")).phase = phase.to_string();
    }

    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(listener: TcpListener, artifact: Arc<Vec<u8>>, state: Arc<Mutex<FixtureState>>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                let Some((method, path, body)) = read_request(&mut stream) else { continue };
                let (status, payload) = {
                    let mut lock = state.lock().unwrap_or_else(|error| panic!("fixture lock: {error}"));
                    lock.calls.push((method, path.clone(), body));
                    let status = if path == "/generate" {
                        lock.acceptance
                    } else if path == "/cancel/server-job" {
                        202
                    } else {
                        200
                    };
                    let payload = if path == "/result/server-job" {
                        artifact.as_ref().clone()
                    } else {
                        format!(
                            "{{\"id\":\"server-job\",\"state\":{},\"asset\":\"server-job\"}}",
                            lock.phase
                        )
                        .into_bytes()
                    };
                    (status, payload)
                };
                let reason = if status == 202 { "Accepted" } else { "OK" };
                let header = format!(
                    "HTTP/1.0 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&payload);
                let _ = stream.flush();
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    let header_end = loop {
        match stream.read(&mut buffer) {
            Ok(0) => return None,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
                    break index + 4;
                }
                if bytes.len() > 1024 * 1024 {
                    return None;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    while bytes.len() < header_end + length {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    let end = header_end.saturating_add(length).min(bytes.len());
    let body = bytes.get(header_end..end).unwrap_or_default().to_vec();
    Some((method, path, body))
}

fn find_bytes(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|window| window == needle)
}

fn json_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn make_artifact() -> Vec<u8> {
    let frames = 3 * 1024 * 1024;
    let data_len = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data_len);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes());
    bytes.extend_from_slice(&32_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data_len as u32).to_le_bytes());
    bytes.resize(bytes.len() + data_len, 0);
    bytes
}

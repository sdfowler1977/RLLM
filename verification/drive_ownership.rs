mod twin;

use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-event checks: actual controller transitions and controlled HTTP/provider responses.";

static SCRATCH_ROOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn scratch_root() -> &'static str {
    SCRATCH_ROOT.get().map(String::as_str).unwrap_or("")
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch(twin::create_scratch(&root, "drive-ownership-"));
    let scratch_text = scratch
        .0
        .to_str()
        .unwrap_or_else(|| panic!("scratch root is not UTF-8"))
        .to_string();
    SCRATCH_ROOT
        .set(scratch_text.clone())
        .unwrap_or_else(|_| panic!("scratch root already set"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();
    let names = Value::Arr(passed.iter().map(|name| s(name)).collect());
    // The leg prints one record and writes another: the runner counts the printed bytes and
    // the publisher splices the written file, so both shapes must exist and must agree.
    println!(
        "{}",
        twin::pretty(&obj(vec![
            ("passed".into(), twin::n(&passed.len().to_string())),
            ("checks".into(), names.clone()),
            ("scope".into(), s(SCOPE)),
        ]))
    );
    twin::report_value("drive_ownership", &obj(vec![("passed".into(), names)]), true);
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let first_state = state(controller, root);
    let a = create(controller, &first_state, "first.txt", root);
    let cancelled_a = event(controller, Some(get(&a, "state")), "cancel", obj(vec![]), root);
    let b = create(controller, get(&cancelled_a, "state"), "second.txt", root);
    keep(
        &mut passed,
        "each create receives a distinct exact owner",
        get(get(&a, "effect"), "drive_ticket") != get(get(&b, "effect"), "drive_ticket"),
    );
    unchanged(
        &mut passed,
        "cancelled acknowledgement cannot start another create readback",
        &b,
        &response(controller, &b, 201, "", None, Some(&a)),
    );
    for (label, extra) in [
        ("missing", vec![]),
        ("wrong", vec![("drive_ticket".to_string(), s("drive-forged"))]),
        (
            "different-purpose",
            vec![
                ("drive_ticket".to_string(), get(get(&b, "effect"), "drive_ticket").clone()),
                ("purpose".to_string(), s("listing")),
            ],
        ),
    ] {
        let mut inputs = obj(vec![
            ("purpose".into(), get(get(&b, "effect"), "purpose").clone()),
            (
                "data".into(),
                s(&b64_encode(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n",
                )),
            ),
        ]);
        for (key, value) in extra {
            set(&mut inputs, &key, value);
        }
        unchanged(
            &mut passed,
            &format!("{label} acknowledgement owner is inert"),
            &b,
            &event(controller, Some(get(&b, "state")), "network_result", inputs, root),
        );
    }
    let verify = response(controller, &b, 201, "", None, None);
    keep(
        &mut passed,
        "authentic acknowledgement starts only its own target",
        get(get(&verify, "effect"), "wire")
            .str_()
            .starts_with("GET /share/second.txt ")
            && get(get(&verify, "effect"), "drive_ticket") != get(get(&b, "effect"), "drive_ticket"),
    );
    unchanged(
        &mut passed,
        "write owner cannot settle its readback phase",
        &verify,
        &response(controller, &verify, 201, "", None, Some(&b)),
    );
    let before = verify.clone();
    let done = response(controller, &verify, 200, "Same bytes", None, None);
    keep(
        &mut passed,
        "authentic current readback verifies exact contents",
        !get(get(&done, "state"), "busy").bool_()
            && is_null(get(get(&done, "state"), "drive_creation"))
            && get(&done, "ui").to_string().contains("readback verified"),
    );
    unchanged(
        &mut passed,
        "duplicate readback cannot replay completed state",
        &done,
        &response(controller, &done, 200, "Same bytes", None, Some(&before)),
    );
    unchanged(
        &mut passed,
        "restored pre-completion state cannot revive consumed owner",
        &before,
        &response(controller, &before, 200, "Same bytes", None, None),
    );

    let third_state = state(controller, root);
    let a = create(controller, &third_state, "third.txt", root);
    let old_verify = response(controller, &a, 201, "", None, None);
    let cancelled_old = event(controller, Some(get(&old_verify, "state")), "cancel", obj(vec![]), root);
    let b = create(controller, get(&cancelled_old, "state"), "fourth.txt", root);
    let new_verify = response(controller, &b, 201, "", None, None);
    unchanged(
        &mut passed,
        "cancelled same-phase same-content readback cannot certify a newer file",
        &new_verify,
        &response(controller, &new_verify, 200, "Same bytes", None, Some(&old_verify)),
    );
    let done = response(controller, &new_verify, 200, "Same bytes", None, None);
    keep(
        &mut passed,
        "newer file remains verifiable after stale readback",
        get(&done, "ui").to_string().contains("readback verified"),
    );

    let list_state_140 = state(controller, root);
    let a = event(controller, Some(&list_state_140), "drive_list", obj(vec![]), root);
    let cancelled_a = event(controller, Some(get(&a, "state")), "cancel", obj(vec![]), root);
    let b = event(controller, Some(get(&cancelled_a, "state")), "drive_list", obj(vec![]), root);
    unchanged(
        &mut passed,
        "cancelled listing cannot replace current rows",
        &b,
        &response(controller, &b, 207, &xml("old.txt"), None, Some(&a)),
    );
    let listed = response(controller, &b, 207, &xml("current.txt"), None, None);
    let remote = get(get(&listed, "state"), "remote").arr();
    keep(
        &mut passed,
        "current listing remains available",
        remote.len() == 1 && get(&remote[0], "url").str_().ends_with("/current.txt"),
    );
    for (key, value) in [
        ("drive", obj(vec![("url".into(), s("https://other.test/share/"))])),
        ("active", s("another-chat")),
        ("drive_creation", obj(vec![("phase".into(), s("changed"))])),
    ] {
        let list_state_161 = state(controller, root);
        let d = event(controller, Some(&list_state_161), "drive_list", obj(vec![]), root);
        let mut changed = d.clone();
        set(get_mut(&mut changed, "state"), key, value);
        unchanged(
            &mut passed,
            &format!("changed {key} cannot adopt bytes under prior owner"),
            &changed,
            &response(controller, &changed, 207, &xml("hidden.txt"), None, None),
        );
        event(controller, Some(get(&d, "state")), "cancel", obj(vec![]), root);
    }
    let list_state_172 = state(controller, root);
    let d = event(controller, Some(&list_state_172), "drive_list", obj(vec![]), root);
    let saved = d.clone();
    event(controller, Some(get(&d, "state")), "resume", obj(vec![]), root);
    unchanged(
        &mut passed,
        "resume revokes the process-held owner even for restored saved state",
        &saved,
        &response(controller, &saved, 207, &xml("restored.txt"), None, None),
    );

    let list_state_182 = state(controller, root);
    let a = event(controller, Some(&list_state_182), "drive_list", obj(vec![]), root);
    let cancelled_a = event(controller, Some(get(&a, "state")), "cancel", obj(vec![]), root);
    let b = event(controller, Some(get(&cancelled_a, "state")), "drive_list", obj(vec![]), root);
    let mut inputs = obj(vec![
        ("purpose".into(), get(get(&a, "effect"), "purpose").clone()),
        ("drive_ticket".into(), get(get(&a, "effect"), "drive_ticket").clone()),
        ("error".into(), s("Old failure")),
        ("sent".into(), twin::b(false)),
    ]);
    unchanged(
        &mut passed,
        "old failure cannot clear a newer busy request",
        &b,
        &event(controller, Some(get(&b, "state")), "failure", inputs.clone(), root),
    );
    set(&mut inputs, "drive_ticket", get(get(&b, "effect"), "drive_ticket").clone());
    set(&mut inputs, "error", s("Current controlled failure"));
    let failed = event(controller, Some(get(&b, "state")), "failure", inputs, root);
    keep(
        &mut passed,
        "current failure settles its own request",
        !get(get(&failed, "state"), "busy").bool_()
            && get(get(&failed, "state"), "notice")
                .str_()
                .contains("Current controlled failure"),
    );

    let mut st = state(controller, root);
    set(&mut st, "dialog", s("drive_update_editor"));
    set(&mut st, "edit", obj(vec![]));
    let d = event(
        controller,
        Some(&st),
        "drive_update_review",
        obj(vec![
            ("path".into(), s("existing.txt")),
            ("content".into(), s("New bytes")),
        ]),
        root,
    );
    let d = event(controller, Some(get(&d, "state")), "approve", obj(vec![]), root);
    keep(
        &mut passed,
        "reviewed update begins with an owned current read",
        get(get(&d, "effect"), "purpose") == &Value::Str("drive_update_read".into())
            && truthy(get(get(&d, "effect"), "drive_ticket")),
    );
    let d = response(
        controller,
        &d,
        200,
        "Previous bytes",
        Some(vec![("ETag", "\"source-etag\"")]),
        None,
    );
    keep(
        &mut passed,
        "current read prepares a conditional separate backup",
        get(get(&d, "effect"), "purpose") == &Value::Str("drive_update_backup".into())
            && get(get(&d, "effect"), "wire")
                .str_()
                .contains("If-None-Match: *"),
    );
    let d = response(controller, &d, 201, "", None, None);
    keep(
        &mut passed,
        "backup acknowledgement starts owned backup verification",
        get(get(&d, "effect"), "purpose") == &Value::Str("drive_update_verify".into()),
    );
    let old_backup = d.clone();
    let d = response(controller, &d, 200, "Previous bytes", None, None);
    keep(
        &mut passed,
        "only verified backup enables guarded target update",
        get(get(&d, "effect"), "purpose") == &Value::Str("drive_update_write".into())
            && get(get(&d, "effect"), "wire")
                .str_()
                .contains("If-Match: \"source-etag\""),
    );
    let d = response(controller, &d, 204, "", None, None);
    keep(
        &mut passed,
        "target update acknowledgement starts owned confirmation",
        get(get(&d, "effect"), "purpose") == &Value::Str("drive_update_confirm".into()),
    );
    unchanged(
        &mut passed,
        "backup verification cannot settle target confirmation",
        &d,
        &response(controller, &d, 200, "New bytes", None, Some(&old_backup)),
    );
    let d = response(controller, &d, 200, "New bytes", None, None);
    keep(
        &mut passed,
        "owned revision completes only after matching target readback",
        get(&d, "ui").to_string().contains("Network text file updated")
            && is_null(get(get(&d, "state"), "drive_revision")),
    );

    let snapshot_state = state(controller, root);
    let a = event(controller, Some(&snapshot_state), "save_drive", obj(vec![]), root);
    let cancelled_a = event(controller, Some(get(&a, "state")), "cancel", obj(vec![]), root);
    let discarded = event(
        controller,
        Some(get(&cancelled_a, "state")),
        "remote_save_discard",
        obj(vec![]),
        root,
    );
    let b = event(controller, Some(get(&discarded, "state")), "save_drive", obj(vec![]), root);
    unchanged(
        &mut passed,
        "cancelled snapshot acknowledgement cannot advance a newer snapshot",
        &b,
        &response(controller, &b, 201, "", None, Some(&a)),
    );
    let d = response(controller, &b, 201, "", None, None);
    keep(
        &mut passed,
        "current snapshot acknowledgement waits for owned verification",
        get(get(&d, "effect"), "purpose") == &Value::Str("saved_verify".into()),
    );
    let content = get(get(get(&d, "state"), "remote_save"), "content").clone();
    let d = response(controller, &d, 200, content.str_(), None, None);
    keep(
        &mut passed,
        "snapshot saved success requires exact current readback",
        is_null(get(get(&d, "state"), "remote_save"))
            && get(get(&d, "state"), "notice")
                .str_()
                .contains("exact bytes verified"),
    );

    let mut st = state(controller, root);
    set(&mut st, "dialog", s("drive_update_editor"));
    set(&mut st, "edit", obj(vec![]));
    let d = event(
        controller,
        Some(&st),
        "drive_update_load",
        obj(vec![("path".into(), s("existing.txt"))]),
        root,
    );
    keep(
        &mut passed,
        "editor load receives an exact owner",
        get(get(&d, "effect"), "purpose") == &Value::Str("drive_editor_read".into())
            && truthy(get(get(&d, "effect"), "drive_ticket")),
    );
    let mut changed = d.clone();
    set(get_mut(get_mut(&mut changed, "state"), "edit"), "path", s("different.txt"));
    unchanged(
        &mut passed,
        "changed editor path cannot receive older file contents",
        &changed,
        &response(controller, &changed, 200, "Previous bytes", None, None),
    );
    let d = response(controller, &d, 200, "Previous bytes", None, None);
    keep(
        &mut passed,
        "current editor loads exact returned text",
        get(get(get(&d, "state"), "edit"), "content") == &Value::Str("Previous bytes".into()),
    );

    let mut st = state(controller, root);
    set(
        &mut st,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("protocol".into(), s("chat")),
            ("endpoint".into(), s("http://model.test/v1/chat/completions")),
            ("model".into(), s("controlled")),
            ("plain".into(), twin::b(true)),
            ("stream".into(), twin::b(false)),
        ])]),
    );
    set(
        &mut st,
        "agents",
        Value::Arr(vec![
            obj(vec![
                ("name".into(), s("Parent")),
                ("tools".into(), s("delegate")),
                ("subagents".into(), Value::Arr(vec![twin::n("1")])),  // the leg passes a LIST of subagent indexes
            ]),
            obj(vec![
                ("name".into(), s("Child")),
                ("tools".into(), s("drive_list")),
                ("model".into(), s("0")),  // the leg passes the string "0"
            ]),
        ]),
    );
    let d = event(
        controller,
        Some(&st),
        "send",
        obj(vec![("draft".into(), s("Ask the child to list the folder."))]),
        root,
    );
    let d = model(controller, &d, &delegate_json());
    let d = event(controller, Some(get(&d, "state")), "approve", obj(vec![]), root);
    let d = model(controller, &d, &drive_json());
    let d = event(controller, Some(get(&d, "state")), "approve", obj(vec![]), root);
    keep(
        &mut passed,
        "child network effect has both exact owners",
        truthy(get(get(&d, "effect"), "drive_ticket"))
            && truthy(get(get(&d, "effect"), "child_ticket")),
    );
    let mut wrong = d.clone();
    if let Value::Obj(fields) = get_mut(&mut wrong, "effect") {
        fields.retain(|(key, _)| key != "child_ticket");
    }
    unchanged(
        &mut passed,
        "missing child owner cannot consume the current transport owner",
        &d,
        &response(controller, &d, 207, &xml("child.txt"), None, Some(&wrong)),
    );
    let d = response(controller, &d, 207, &xml("child.txt"), None, None);
    keep(
        &mut passed,
        "authentic child response reaches its actual model",
        get(get(&d, "effect"), "wire").str_().contains("child.txt"),
    );
    let d = model(controller, &d, "Actual child listing completed.");
    keep(
        &mut passed,
        "actual child returns to captured parent",
        get(get(&d, "effect"), "wire")
            .str_()
            .contains("Actual child listing completed."),
    );
    let d = model(controller, &d, "Parent answer from the actual child result.");
    keep(
        &mut passed,
        "parent can complete after owned storage result",
        !get(get(&d, "state"), "busy").bool_(),
    );
    passed
}

fn state(controller: &mut Controller, root: &str) -> Value {
    let resumed = event(controller, None, "resume", obj(vec![]), root);
    let mut st = get(&resumed, "state").clone();
    set(
        &mut st,
        "drive",
        obj(vec![
            ("url".into(), s("https://nas.test/share/")),
            ("user".into(), s("fixture")),
            ("password".into(), s("fixture")),
        ]),
    );
    st
}

fn create(controller: &mut Controller, st: &Value, path: &str, root: &str) -> Value {
    let mut st = st.clone();
    set(&mut st, "dialog", s("drive_editor"));
    set(&mut st, "edit", obj(vec![]));
    let d = event(
        controller,
        Some(&st),
        "drive_create_review",
        obj(vec![
            ("path".into(), s(path)),
            ("content".into(), s("Same bytes")),
        ]),
        root,
    );
    assert_eq!(get(get(&d, "state"), "dialog"), &Value::Str("approval".into()));
    event(
        controller,
        Some(get(&d, "state")),
        "approve",
        obj(vec![]),
        root,
    )
}

fn event(
    controller: &mut Controller,
    st: Option<&Value>,
    action: &str,
    inputs: Value,
    root: &str,
) -> Value {
    controller.event(st, action, inputs, root)
}

fn response(
    controller: &mut Controller,
    d: &Value,
    status: u16,
    body: &str,
    headers: Option<Vec<(&str, &str)>>,
    source: Option<&Value>,
) -> Value {
    let effect = get(source.unwrap_or(d), "effect");
    let data = body.as_bytes();
    let mut fields = vec![("Content-Length".to_string(), data.len().to_string())];
    for (key, value) in headers.unwrap_or_default() {
        fields.push((key.to_string(), value.to_string()));
    }
    let mut wire = format!("HTTP/1.1 {status} Result\r\n");
    for (key, value) in fields {
        wire.push_str(&format!("{key}: {value}\r\n"));
    }
    wire.push_str("\r\n");
    let mut bytes = wire.into_bytes();
    bytes.extend_from_slice(data);
    let mut inputs = obj(vec![]);
    for key in ["purpose", "drive_ticket", "child_ticket"] {
        if let Some(value) = effect.get(key) {
            set(&mut inputs, key, value.clone());
        }
    }
    set(&mut inputs, "data", s(&b64_encode(&bytes)));
    event(
        controller,
        Some(get(d, "state")),
        "network_result",
        inputs,
        scratch_root(),
    )
}

fn unchanged(passed: &mut Vec<String>, name: &str, current: &Value, result: &Value) {
    keep(
        passed,
        name,
        matches!(result.get("effect"), None | Some(Value::Null))
            && get(result, "state") == get(current, "state"),
    );
}

fn model(controller: &mut Controller, d: &Value, text: &str) -> Value {
    let body = format!(
        "{{\"choices\":[{{\"message\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}",
        json_string(text)
    );
    let mut wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    wire.extend_from_slice(body.as_bytes());
    let inputs = obj(vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), get(get(d, "state"), "model_expected").clone()),
        ("data".into(), s(&b64_encode(&wire))),
    ]);
    event(
        controller,
        Some(get(d, "state")),
        "network_result",
        inputs,
        scratch_root(),
    )
}

fn delegate_json() -> String {
    "{\"tool\":\"delegate\",\"args\":{\"agent\":1,\"task\":\"List the folder.\"}}".into()
}

fn drive_json() -> String {
    "{\"tool\":\"drive_list\",\"args\":{\"path\":\"\"}}".into()
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn xml(name: &str) -> String {
    format!(
        "<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/share/{name}</d:href></d:response></d:multistatus>"
    )
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Num(value) => value.parse::<f64>().map(|number| number != 0.0).unwrap_or(true),
        Value::Str(value) => !value.is_empty(),
        Value::Arr(value) => !value.is_empty(),
        Value::Obj(value) => !value.is_empty(),
    }
}

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing key: {key}"))
}

fn get_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    match value {
        Value::Obj(fields) => fields
            .iter_mut()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("missing key: {key}")),
        _ => panic!("expected object"),
    }
}

fn is_null(value: &Value) -> bool {
    matches!(value, Value::Null)
}

fn set(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
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

// The scratch tree is this twin's own working area. It must not survive the
// twin on any exit path, including a panic, so the removal rides on Drop the
// way the other twins that clean up after themselves do.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

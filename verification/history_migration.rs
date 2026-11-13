//! Native verification twin of verification/history_migration.py.
mod twin;

use std::path::{Path, PathBuf};

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner compatibility checks: actual prior and current controllers with controlled peer replies.";

fn main() {
    let previous = parse_args();
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&project, "history-migration");
    let root = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8"));
    std::fs::write(Path::new(root).join("input.txt"), "Ordinary local input remains available.")
        .unwrap_or_else(|error| panic!("could not write input.txt: {error}"));

    let mut passed = Vec::new();
    let mut prior = Controller::spawn_at(
        &previous,
        project
            .to_str()
            .unwrap_or_else(|| panic!("project root is not UTF-8")),
    );
    let saved = drive_prior(&mut prior, root, &mut passed);
    prior.finish();

    let mut current = Controller::spawn(
        project
            .to_str()
            .unwrap_or_else(|| panic!("project root is not UTF-8")),
    );
    drive_current(&mut current, root, &saved, &mut passed);
    current.finish();

    let body = obj(vec![
        ("passed".into(), Value::Num(passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value_printed("history_migration", &body, false);

    let written = obj(vec![(
        "passed".into(),
        Value::Arr(passed.iter().map(|name| s(name)).collect()),
    )]);
    twin::report_value("history_migration", &written, true);

    std::fs::remove_dir_all(&scratch)
        .unwrap_or_else(|error| panic!("could not remove {}: {error}", scratch.display()));
}

fn parse_args() -> PathBuf {
    let mut arguments = std::env::args().skip(1);
    let mut previous = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--previous-controller" => {
                if previous.is_some() {
                    panic!("--previous-controller was provided more than once");
                }
                previous = Some(PathBuf::from(arguments.next().unwrap_or_else(|| {
                    panic!("--previous-controller requires a path")
                })));
            }
            _ => panic!("unrecognized arguments: {argument}"),
        }
    }
    previous.unwrap_or_else(|| {
        panic!("the following arguments are required: --previous-controller")
    })
}

fn drive_prior(prior: &mut Controller, root: &str, passed: &mut Vec<String>) -> Value {
    let old = String::from_utf8(vec![114, 101, 97]).unwrap();
    let resumed = prior.event(None, "resume", obj(vec![]), root);
    let mut state = field(&resumed, "state").clone();

    put(
        &mut state,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://model.test/v1/chat/completions")),
            ("protocol".into(), s("chat")),
            ("model".into(), s("controlled")),
            ("plain".into(), Value::Bool(true)),
            ("stream".into(), Value::Bool(false)),
        ])]),
    );
    put(
        &mut state,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s(&format!("read,{old}"))),
        ])]),
    );
    put(
        &mut state,
        &old,
        obj(vec![
            ("endpoint".into(), s("http://127.0.0.1:9127/mcp")),
            ("key".into(), s("fixture-key")),
        ]),
    );

    let sent = prior.event(
        Some(&state),
        "send",
        obj(vec![("draft".into(), s("Obtain an actual peer observation."))]),
        root,
    );
    let proposal = format!(
        "{{\"tool\":\"{old}\",\"args\":{{\"name\":\"inspect_product\",\"arguments\":{{}}}}}}"
    );
    let mut dialogue = sent;
    let model_ticket = field(field(&dialogue, "state"), "model_expected").clone();
    let response = Some(obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(&proposal))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]));
    let data = wire(&response, "200 OK");
    dialogue = prior.event(
        Some(field(&dialogue, "state")),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), model_ticket),
            ("data".into(), s(&data)),
        ]),
        root,
    );
    if value_str(field(field(&dialogue, "state"), "dialog")) != Some("approval") {
        panic!("prior controller did not request approval");
    }
    dialogue = prior.event(
        Some(field(&dialogue, "state")),
        "approve",
        obj(vec![]),
        root,
    );

    let mut settled = false;
    for _ in 0..8 {
        let effect = field(&dialogue, "effect").clone();
        if value_str(&effect) != Some("network") {
            panic!("Peer operation did not produce a network effect");
        }
        let purpose = field(&effect, "purpose").clone();
        if value_str(&purpose) == Some("model") {
            settled = true;
            break;
        }
        let wire_text = value_str(field(&effect, "wire"))
            .unwrap_or_else(|| panic!("network effect has no wire"));
        let body = wire_text
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("network effect has no HTTP body"))
            .1;
        let request = Value::parse(body);
        let method = value_str(field(&request, "method"))
            .unwrap_or_else(|| panic!("network request has no method"));
        let response = match method {
            "initialize" => Some(obj(vec![
                ("jsonrpc".into(), s("2.0")),
                ("id".into(), field(&request, "id").clone()),
                (
                    "result".into(),
                    obj(vec![
                        (
                            "protocolVersion".into(),
                            field(field(&request, "params"), "protocolVersion").clone(),
                        ),
                        (
                            "capabilities".into(),
                            obj(vec![("tools".into(), obj(vec![]))]),
                        ),
                        (
                            "serverInfo".into(),
                            obj(vec![
                                ("name".into(), s("Fixture")),
                                ("version".into(), s("one")),
                            ]),
                        ),
                    ]),
                ),
            ])),
            "notifications/initialized" => None,
            "tools/call" => Some(obj(vec![
                ("jsonrpc".into(), s("2.0")),
                ("id".into(), field(&request, "id").clone()),
                (
                    "result".into(),
                    obj(vec![(
                        "content".into(),
                        Value::Arr(vec![obj(vec![
                            ("type".into(), s("text")),
                            ("text".into(), s("Original controlled peer observation.")),
                        ])]),
                    )]),
                ),
            ])),
            other => panic!("{other}"),
        };
        let status = if response.is_none() {
            "202 Accepted"
        } else {
            "200 OK"
        };
        let data = wire(&response, status);
        dialogue = prior.event(
            Some(field(&dialogue, "state")),
            "network_result",
            obj(vec![
                ("purpose".into(), purpose),
                ("data".into(), s(&data)),
            ]),
            root,
        );
    }
    if !settled {
        panic!("Peer operation did not settle");
    }
    if value_str(field(field(&dialogue, "effect"), "purpose")) != Some("model") {
        panic!("Peer operation did not settle on the model request");
    }

    let ticket = field(field(&dialogue, "state"), "model_expected").clone();
    let response = Some(obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            (
                "message".into(),
                obj(vec![(
                    "content".into(),
                    s("Answer following the actual peer result."),
                )]),
            ),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]));
    let data = wire(&response, "200 OK");
    let saved = prior.event(
        Some(field(&dialogue, "state")),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&data)),
        ]),
        root,
    );
    let saved = field(&saved, "state").clone();
    let case = inquiry_with_evidence(&saved);
    let receipt = field(case, "evidence").arr()[0].clone();
    let output = value_str(field(&receipt, "output")).unwrap_or_default();
    check(
        passed,
        "reference controller creates actual owned historical receipt",
        value_str(field(&receipt, "tool")) == Some("rea")
            && output.contains("Original controlled peer observation."),
    );
    saved
}

fn drive_current(current: &mut Controller, root: &str, saved: &Value, passed: &mut Vec<String>) {
    let case = inquiry_with_evidence(saved);
    let case_id = field(case, "id").clone();
    let receipt = field(case, "evidence").arr()[0].clone();

    let resumed = current.event(Some(saved), "resume", obj(vec![]), root);
    let migrated = inquiry_by_id(field(&resumed, "state"), &case_id);
    let actual = field(migrated, "evidence").arr()[0].clone();
    check(
        passed,
        "historical receipt uses current operation identifier",
        value_str(field(&actual, "tool")) == Some("mcp"),
    );
    check(
        passed,
        "captured output bytes and digests are unchanged",
        ["output", "sha256", "prefix_sha256", "bytes", "source_guard", "args"]
            .iter()
            .all(|key| field(&actual, key) == field(&receipt, key)),
    );

    let case_id_text = value_str(&case_id).unwrap_or_default();
    let selected = current.event(
        Some(field(&resumed, "state")),
        &format!("inquiry_select:{case_id_text}"),
        obj(vec![]),
        root,
    );
    let selected_state = field(&selected, "state");
    check(
        passed,
        "migrated historical question remains readable",
        !matches!(selected_state.get("notice_error"), Some(Value::Bool(true)))
            && value_str(selected_state.get("dialog").unwrap_or(&Value::Null)) == Some("inquiry_view"),
    );

    let evidence = current.event(
        Some(selected_state),
        "inquiry_section:evidence",
        obj(vec![]),
        root,
    );
    let evidence_state = field(&evidence, "state");
    check(
        passed,
        "historical evidence is still available",
        value_str(evidence_state.get("dialog").unwrap_or(&Value::Null)) == Some("inquiry_view")
            && field(evidence_state, "ui").to_string().contains("Original controlled peer observation."),
    );

    let mut corrupted = saved.clone();
    let corrupted_case = inquiry_by_id_mut(&mut corrupted, &case_id);
    let corrupted_receipt = field_mut(field_mut(corrupted_case, "evidence"), "0");
    put(
        corrupted_receipt,
        "output",
        s("Tampered input without a matching original receipt"),
    );
    let corrupted_resumed = current.event(Some(&corrupted), "resume", obj(vec![]), root);
    let corrupted_selected = current.event(
        Some(field(&corrupted_resumed, "state")),
        &format!("inquiry_select:{case_id_text}"),
        obj(vec![]),
        root,
    );
    check(
        passed,
        "migration refuses to bless altered receipt data",
        matches!(
            field(&corrupted_selected, "state").get("notice_error"),
            Some(Value::Bool(true))
        ),
    );

    let read = current.op(&obj(vec![
        ("op".into(), s("tool")),
        ("root".into(), s(root)),
        ("tool".into(), s("read")),
        (
            "args".into(),
            obj(vec![("path".into(), s("input.txt"))]),
        ),
    ]));
    check(
        passed,
        "corrupt optional history does not block local operations",
        value_str(&read) == Some("Ordinary local input remains available."),
    );
}

fn wire(value: &Option<Value>, status: &str) -> String {
    let bytes = value.as_ref().map(|value| value.to_string()).unwrap_or_default();
    let encoded = twin::b64_encode(bytes.as_bytes());
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{encoded}",
        bytes.len()
    )
}

fn inquiry_with_evidence(state: &Value) -> &Value {
    field(field(state, "inquiry_records"), "items")
        .arr()
        .iter()
        .find(|case| !field(case, "evidence").arr().is_empty())
        .unwrap_or_else(|| panic!("no inquiry record with evidence"))
}

fn inquiry_by_id<'a>(state: &'a Value, id: &Value) -> &'a Value {
    field(field(state, "inquiry_records"), "items")
        .arr()
        .iter()
        .find(|case| field(case, "id") == id)
        .unwrap_or_else(|| panic!("no inquiry record with the requested id"))
}

fn inquiry_by_id_mut<'a>(state: &'a mut Value, id: &Value) -> &'a mut Value {
    let items = field_mut(field_mut(state, "inquiry_records"), "items");
    let Value::Arr(items) = items else {
        panic!("expected array");
    };
    items
        .iter_mut()
        .find(|case| field(case, "id") == id)
        .unwrap_or_else(|| panic!("no inquiry record with the requested id"))
}

fn value_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
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

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
}

fn check(passed: &mut Vec<String>, name: &str, value: bool) {
    if !value {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

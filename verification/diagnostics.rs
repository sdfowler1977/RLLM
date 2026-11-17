mod twin;

use twin::{b64_encode, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner connected core execution with actual completed requests, saved-state round trip, active callback preservation, selected model/range, ordinary artifact-free local worker and scoped range refusal. Geometry checks describe drawing bounds; installed Android visual acceptance is separate.";

static SCRATCH_ROOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn scratch_root() -> &'static str {
    SCRATCH_ROOT.get().map(String::as_str).unwrap_or("")
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch(twin::create_scratch(&root, "diagnostics-"));
    std::fs::write(scratch.0.join("input.txt"), "Local work after range validation.")
        .unwrap_or_else(|error| panic!("could not write input.txt: {error}"));
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
    let printed = obj(vec![
        ("passed".into(), names.clone()),
        (
            "scope".into(),
            s(SCOPE),
        ),
        ("count".into(), n(&passed.len().to_string())),
    ]);
    println!("{}", twin::pretty(&printed));

    let written = obj(vec![
        ("passed".into(), names),
        (
            "scope".into(),
            s(SCOPE),
        ),
    ]);
    twin::report_value("diagnostics", &written, true);
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();

    let d = event(controller, None, "resume", obj(vec![]), root);
    keep(
        &mut passed,
        "startup keeps phone permissions under explicit Setup control",
        matches!(d.get("effect"), None | Some(Value::Null))
            && matches!(get(&d, "ui").get("modal"), None | Some(Value::Null)),
    );

    let mut st = get(&d, "state").clone();
    set(
        &mut st,
        "profiles",
        Value::Arr(vec![
            obj(vec![
                ("name".into(), s("First")),
                ("model".into(), s("first")),
                ("endpoint".into(), s("http://first.test/v1/chat/completions")),
                ("protocol".into(), s("chat")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
            ]),
            obj(vec![
                ("name".into(), s("Second")),
                ("model".into(), s("second")),
                ("endpoint".into(), s("http://second.test/v1/chat/completions")),
                ("protocol".into(), s("chat")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
            ]),
        ]),
    );
    set(
        &mut st,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("list,read")),
        ])]),
    );
    set(&mut st, "phone_access_requested", twin::b(true));

    let d = event(
        controller,
        Some(&st),
        "send",
        obj(vec![("draft".into(), s("First measurement"))]),
        root,
    );
    let d = finish(controller, &d, &"a".repeat(400), 100, Some(20), root);
    let d = event(
        controller,
        Some(get(&d, "state")),
        "send",
        obj(vec![("draft".into(), s("Second measurement"))]),
        root,
    );
    let d = finish(controller, &d, &"b".repeat(400), 900, Some(180), root);

    let d = event(controller, Some(get(&d, "state")), "monitor:window:2", obj(vec![]), root);
    let gauges = get(get(get(&d, "ui"), "monitor"), "gauges").arr();
    keep(
        &mut passed,
        "actual completed requests produce weighted token average",
        get(&gauges[0], "value").str_() == "≈200.0 tok/s",
    );
    keep(
        &mut passed,
        "response and first-text gauges use selected means",
        get(&gauges[1], "value").str_() == "500 ms" && get(&gauges[2], "value").str_() == "100 ms",
    );
    keep(
        &mut passed,
        "selected gauge sample count is visible",
        get(&gauges[0], "caption")
            .str_()
            .contains("2 completed runs"),
    );

    let d = event(controller, Some(get(&d, "state")), "monitor:range", obj(vec![]), root);
    keep(
        &mut passed,
        "exact count control is exposed",
        get(get(get(&d, "ui"), "modal"), "title").str_() == "Averaging range",
    );

    let d = event(
        controller,
        Some(get(&d, "state")),
        "capture",
        obj(vec![("runs".into(), s("7"))]),
        root,
    );
    let d = event(controller, Some(get(&d, "state")), "resume", obj(vec![]), root);
    keep(
        &mut passed,
        "unapplied count survives form capture and recovery",
        get(get(&d, "state"), "dialog").str_() == "monitor_range"
            && get(get(get(&d, "ui"), "modal"), "fields")
                .arr()
                .iter()
                .find(|field| get(field, "key").str_() == "runs")
                .map(|field| get(field, "value").str_().to_string())
                .unwrap_or_else(|| {
                    panic!("unapplied count survives form capture and recovery: the capture form has no runs field")
                })
                == "7",
    );

    let snapshot = get(&d, "state").clone();
    let d = event(
        controller,
        Some(&snapshot),
        "monitor:range_save",
        obj(vec![("runs".into(), s("0"))]),
        root,
    );
    keep(
        &mut passed,
        "invalid count retains prior range and review",
        get(get(&d, "state"), "monitor_window") == &twin::n("2")
            && get(get(&d, "state"), "dialog").str_() == "monitor_range"
            && matches!(d.get("effect"), None | Some(Value::Null)),
    );

    let d = event(
        controller,
        Some(get(&d, "state")),
        "monitor:range_save",
        obj(vec![("runs".into(), s("1"))]),
        root,
    );
    keep(
        &mut passed,
        "exact count applies to actual completed history",
        get(&get(get(get(&d, "ui"), "monitor"), "gauges").arr()[1], "value").str_() == "900 ms",
    );

    let saved_state = clone_value(get(&d, "state"));
    let d = event(controller, Some(&saved_state), "resume", obj(vec![]), root);
    let d = event(controller, Some(get(&d, "state")), "diagnostics", obj(vec![]), root);
    keep(
        &mut passed,
        "range survives saved-state recovery",
        get(get(&d, "ui"), "monitor").get("window") == Some(&twin::n("1")),
    );

    let d = event(
        controller,
        Some(get(&d, "state")),
        "send",
        obj(vec![("draft".into(), s("Keep live separate"))]),
        root,
    );
    let ticket = get(get(&d, "state"), "model_expected").clone();
    let d = event(
        controller,
        Some(get(&d, "state")),
        "model_progress",
        obj(vec![
            ("model_ticket".into(), ticket.clone()),
            ("stream_text".into(), s("abcd")),
            ("elapsed_ms".into(), twin::n("1000")),
        ]),
        root,
    );
    let d = event(
        controller,
        Some(get(&d, "state")),
        "diagnostics",
        obj(vec![("draft".into(), s("Retained draft"))]),
        root,
    );
    keep(
        &mut passed,
        "live estimate has its own card",
        get(get(get(get(&d, "ui"), "monitor"), "live"), "value").str_() == "≈1.0 tok/s",
    );
    keep(
        &mut passed,
        "live request preserves completed-run average",
        get(&get(get(get(&d, "ui"), "monitor"), "gauges").arr()[1], "value").str_() == "900 ms",
    );

    let d = event(
        controller,
        Some(get(&d, "state")),
        "monitor:range_save",
        obj(vec![("runs".into(), s("2"))]),
        root,
    );
    keep(
        &mut passed,
        "range changes preserve active callback and draft",
        get(get(&d, "state"), "model_expected") == &ticket
            && get(get(&d, "state"), "busy").bool_()
            && get(get(&d, "state"), "draft").str_() == "Retained draft",
    );

    let d = event(controller, Some(get(&d, "state")), "cancel", obj(vec![]), root);
    let before = clone_value(get(&d, "state"));
    for (action, inputs) in [
        (
            "rate_tick",
            obj(vec![
                ("model_ticket".into(), ticket.clone()),
                ("elapsed_ms".into(), twin::n("9999")),
                ("draft".into(), s("STALE")),
            ]),
        ),
        (
            "model_progress",
            obj(vec![
                ("model_ticket".into(), ticket.clone()),
                ("elapsed_ms".into(), twin::n("9999")),
                ("stream_text".into(), s("STALE")),
            ]),
        ),
        (
            "failure",
            obj(vec![
                ("purpose".into(), s("model")),
                ("model_ticket".into(), ticket.clone()),
                ("error".into(), s("STALE")),
            ]),
        ),
    ] {
        let late = event(controller, Some(&before), action, inputs, root);
        keep(
            &mut passed,
            &format!("stale model delivery preserves exact state: {action}"),
            get(&late, "state") == &before && matches!(late.get("effect"), None | Some(Value::Null)),
        );
    }

    std::fs::write(
        std::path::Path::new(scratch_root()).join("AGENTS.md"),
        "User project instructions changed after the cancelled request.\n",
    )
    .unwrap_or_else(|error| panic!("could not write AGENTS.md: {error}"));

    for (action, inputs) in [
        (
            "document_result",
            obj(vec![
                ("document_ticket".into(), s("retired")),
                ("content".into(), s("STALE_DOCUMENT")),
            ]),
        ),
        (
            "image_result",
            obj(vec![("document_ticket".into(), s("retired"))]),
        ),
        (
            "document_cancel",
            obj(vec![("document_ticket".into(), s("retired"))]),
        ),
        (
            "document_failure",
            obj(vec![
                ("document_ticket".into(), s("retired")),
                ("error".into(), s("STALE_DOCUMENT_ERROR")),
            ]),
        ),
        (
            "apps_result",
            obj(vec![
                ("ticket".into(), s("retired")),
                ("package".into(), s("app.retired")),
                ("data".into(), Value::Arr(vec![])),
            ]),
        ),
    ] {
        let late = event(controller, Some(&before), action, inputs, root);
        keep(
            &mut passed,
            &format!("stale platform delivery preserves exact state before project refresh: {action}"),
            get(&late, "state") == &before && matches!(late.get("effect"), None | Some(Value::Null)),
        );
    }

    let refreshed = event(controller, Some(&before), "diagnostics", obj(vec![]), root);
    keep(
        &mut passed,
        "explicit navigation still refreshes current project instructions",
        get(get(&refreshed, "state"), "harness_project")
            .to_string()
            .contains("User project instructions changed"),
    );

    let d = event(
        controller,
        Some(get(&d, "state")),
        "monitor:window:2",
        obj(vec![]),
        root,
    );
    let gauges = get(get(get(&d, "ui"), "monitor"), "gauges").arr();
    keep(
        &mut passed,
        "cancelled run is excluded from successful averages",
        get(&gauges[1], "value").str_() == "900 ms" && get(&gauges[3], "value").str_() == "0.0 %",
    );

    let mut st = get(&d, "state").clone();
    set(&mut st, "selected", twin::n("1"));
    set(
        &mut st,
        "routing",
        obj(vec![("primary".into(), s("1"))]),
    );
    let d = event(
        controller,
        Some(&st),
        "send",
        obj(vec![("draft".into(), s("Other model"))]),
        root,
    );
    let d = finish(controller, &d, &"c".repeat(40), 400, Some(80), root);
    let record = get(get(&d, "state"), "requests").arr().last().unwrap().clone();
    let d = event(
        controller,
        Some(get(&d, "state")),
        &format!("monitor:model:{}", get(&record, "id").str_()),
        obj(vec![]),
        root,
    );
    keep(
        &mut passed,
        "model filter precedes selected count",
        get(&get(get(get(&d, "ui"), "monitor"), "gauges").arr()[1], "value").str_() == "400 ms"
            && get(get(get(&d, "ui"), "monitor"), "subtitle")
                .str_()
                .contains("1 recorded requests"),
    );

    let d = event(controller, Some(get(&d, "state")), "monitor:models", obj(vec![]), root);
    keep(
        &mut passed,
        "model statistics share selected history",
        get(get(get(&d, "ui"), "modal"), "fields")
            .arr()
            .iter()
            .any(|field| {
                field.get("value")
                    .and_then(|value| matches!(value, Value::Str(_)).then(|| value.str_()))
                    .map(|value| value.contains("1 requests"))
                    .unwrap_or(false)
            }),
    );

    let d = event(controller, Some(get(&d, "state")), "monitor:details", obj(vec![]), root);
    keep(
        &mut passed,
        "request list shares selected history",
        get(get(get(&d, "ui"), "modal"), "buttons").arr().len() == 1,
    );

    let d = event(controller, Some(get(&d, "state")), "monitor:all", obj(vec![]), root);
    let d = event(
        controller,
        Some(get(&d, "state")),
        "monitor:window:128",
        obj(vec![]),
        root,
    );
    keep(
        &mut passed,
        "retention ceiling remains explicit",
        get(get(&d, "ui"), "monitor").get("window") == Some(&twin::n("128"))
            && get(get(get(&d, "ui"), "monitor"), "range_label")
                .str_()
                .contains("128 runs"),
    );

    for card in get(get(get(&d, "ui"), "monitor"), "gauges").arr() {
        let visual = get(card, "visual");
        let ok = get(visual, "shapes").arr().iter().all(|shape| {
            shape.get("kind").and_then(|value| matches!(value, Value::Str(_)).then(|| value.str_())) != Some("arc")
                || {
                    let x = shape.get("x").and_then(number).unwrap_or_else(|| panic!("missing shape x"));
                    let x2 = shape.get("x2").and_then(number).unwrap_or_else(|| panic!("missing shape x2"));
                    let y = shape.get("y").and_then(number).unwrap_or_else(|| panic!("missing shape y"));
                    let y2 = shape.get("y2").and_then(number).unwrap_or_else(|| panic!("missing shape y2"));
                    let width = shape
                        .get("width")
                        .and_then(number)
                        .unwrap_or_else(|| panic!("missing shape width"));
                    let height = visual
                        .get("height")
                        .and_then(number)
                        .unwrap_or_else(|| panic!("missing visual height"));
                    let visual_width = visual
                        .get("width")
                        .and_then(number)
                        .unwrap_or_else(|| panic!("missing visual width"));
                    0.0 <= x - width / 2.0
                        && x2 + width / 2.0 <= visual_width
                        && 0.0 <= y - width / 2.0
                        && y2 + width / 2.0 <= height
                }
        });
        keep(
            &mut passed,
            &format!(
                "gauge arcs stay within their drawing bounds: {}",
                get(card, "title").str_()
            ),
            ok,
        );
    }

    let d = event(
        controller,
        Some(get(&d, "state")),
        "monitor:range_save",
        obj(vec![("runs".into(), s("129"))]),
        root,
    );
    let d = event(controller, Some(get(&d, "state")), "files", obj(vec![]), root);
    let effect = get(&d, "effect").clone();
    let mut tool = effect;
    set(&mut tool, "op", s("tool"));
    set(&mut tool, "root", s(root));
    let result = match controller.op(&tool) {
        Value::Str(text) => text,
        // The leg asks whether 'input.txt' is inside the tool reply's own string value, so a
        // non-string reply must fail the check rather than be stringified.
        other => panic!(
            "range failure leaves ordinary local tools available: tool reply was not a string: {}",
            other.to_string()
        ),
    };
    keep(
        &mut passed,
        "range failure leaves ordinary local tools available",
        result.contains("input.txt"),
    );

    std::fs::create_dir_all("output/checks")
        .unwrap_or_else(|error| panic!("could not create output/checks: {error}"));
    let mut visual_report = twin::pretty(get(&d, "ui"));
    visual_report.push('\n');
    std::fs::write("output/checks/diagnostic-view.json", visual_report)
        .unwrap_or_else(|error| panic!("could not write diagnostic-view.json: {error}"));

    passed
}

fn finish(
    controller: &mut Controller,
    d: &Value,
    content: &str,
    elapsed_ms: u64,
    first_text_ms: Option<u64>,
    root: &str,
) -> Value {
    let body = format!(
        "{{\"choices\":[{{\"message\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}",
        json_string(content)
    );
    let mut wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    wire.extend_from_slice(body.as_bytes());
    let mut inputs = obj(vec![
        ("purpose".into(), s("model")),
        (
            "model_ticket".into(),
            get(get(d, "state"), "model_expected").clone(),
        ),
        ("data".into(), s(&b64_encode(&wire))),
        ("elapsed_ms".into(), n(&elapsed_ms.to_string())),
    ]);
    if let Some(first) = first_text_ms {
        set(&mut inputs, "first_text_ms", n(&first.to_string()));
    }
    event(
        controller,
        Some(get(d, "state")),
        "network_result",
        inputs,
        root,
    )
}

fn event(
    controller: &mut Controller,
    state: Option<&Value>,
    action: &str,
    inputs: Value,
    root: &str,
) -> Value {
    controller.event(state, action, inputs, root)
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing key: {key}"))
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

fn clone_value(value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        Value::Num(value) => Value::Num(value.clone()),
        Value::Str(value) => Value::Str(value.clone()),
        Value::Arr(values) => Value::Arr(values.iter().map(clone_value).collect()),
        Value::Obj(fields) => Value::Obj(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), clone_value(value)))
                .collect(),
        ),
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Num(text) => text.parse::<f64>().ok(),
        _ => None,
    }
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

// The scratch tree is this twin's own working area. The removal rides on this
// twin's own Drop guard, so the tree goes away when the twin returns normally
// and when a panic unwinds out of it. An uncatchable termination such as
// SIGKILL leaves the tree behind; that ceiling belongs to the runner's
// timeout path, which kills the twin's process group, not to this twin.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

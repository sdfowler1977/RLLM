mod twin;

use twin::{b, b64_encode, create_scratch, n, obj, report_value, s, sha256_bytes, truthy, Controller, Value};

const SCOPE: &str = "controlled fixtures; actual file worker and process execution";
const EVIDENCE: &str = "Observed evidence from an actual held file.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = create_scratch(&root, "provider-process");
    std::fs::write(scratch.join("evidence.txt"), EVIDENCE)
        .unwrap_or_else(|error| panic!("could not write evidence.txt: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch_text = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
        .to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();
    publish(&passed);
    let _ = std::fs::remove_dir_all(&scratch);
}

fn publish(passed: &[String]) {
    if passed.len() != 60 {
        panic!("provider_process emitted {} checks", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("provider_replies".into(), s(SCOPE)),
    ]);
    // Python writes output/checks/current-results.json with json.dumps(indent=2)
    // and no trailing newline. group_report_file maps this group to that name.
    report_value("provider_process", &body, false);
    println!("{} fresh connected checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    for fmt in ["chat", "local", "messages", "content"] {
        let resumed = controller.event(None, "resume", obj(vec![]), root);
        let mut st = state(&resumed).clone();
        put(&mut st, "dialog", Value::Null);
        put(
            &mut st,
            "profiles",
            Value::Arr(
                (0..2)
                    .map(|index| {
                        obj(vec![
                            ("name".into(), s(&format!("Connection {index}"))),
                            ("protocol".into(), s(fmt)),
                            (
                                "endpoint".into(),
                                s(&format!("http://model{index}.test/v1/chat/completions")),
                            ),
                            ("model".into(), s(&format!("model-{index}"))),
                            ("plain".into(), b(true)),
                            ("stream".into(), b(false)),
                            ("tool_format".into(), s("structured")),
                        ])
                    })
                    .collect(),
            ),
        );
        put(
            &mut st,
            "agents",
            Value::Arr(vec![
                obj(vec![
                    ("name".into(), s("Parent")),
                    ("model".into(), s("0")),
                    ("tools".into(), s("read,hypotheses,delegate")),
                    ("subagents".into(), Value::Arr(vec![n("1")])),
                ]),
                obj(vec![
                    ("name".into(), s("Child")),
                    ("model".into(), s("1")),
                    ("tools".into(), s("read,hypotheses")),
                    ("subagents".into(), Value::Arr(vec![])),
                ]),
            ]),
        );

        let mut d = controller.event(
            Some(&st),
            "send",
            obj(vec![(
                "draft".into(),
                s("Investigate the evidence file and design an original answer."),
            )]),
            root,
        );
        let name = format!("{fmt} declaration");
        keep(&mut passed, &name, text_contains(effect_of(&d, &name), "wire", "\"tools\""));

        let name = format!("{fmt} normal conversation starts a question without manual forms");
        let current = last_inquiry(&d, &name);
        keep(
            &mut passed,
            &name,
            matches!(current.get("plan"), Some(Value::Null))
                && matches!(current.get("question"), Some(Value::Str(text)) if text.starts_with("Investigate the evidence file")),
        );

        let name = format!("{fmt} review");
        d = reply(
            controller,
            &d,
            fmt,
            "read",
            vec![("path".into(), s("evidence.txt"))],
            "",
            root,
        );
        keep(&mut passed, &name, text_is(state_of(&d, &name), "dialog", "approval"));

        let name = format!("{fmt} held worker and retained evidence");
        let approved = controller.event(Some(state_of(&d, &name)), "approve", obj(vec![]), root);
        let tool_effect = effect_of(&approved, &name).clone();
        let output = tool_op(controller, &tool_effect, root, &name);
        demand(&name, output == EVIDENCE);
        let mut inputs = project(
            &tool_effect,
            &["purpose", "tool_ticket", "tool_binding", "child_ticket", "child_owner"],
            &name,
        );
        put(&mut inputs, "output", s(&output));
        d = controller.event(Some(state_of(&approved, &name)), "tool_result", inputs, root);
        let retained = last_inquiry(&d, &name)
            .get("evidence")
            .and_then(last_of)
            .and_then(|item| item.get("output"));
        keep(&mut passed, &name, matches!(retained, Some(Value::Str(text)) if text == &output));

        let actions = [
            obj(vec![
                ("action".into(), s("plan")),
                ("questions".into(), Value::Arr(vec![s("What was observed?")])),
                ("requirements".into(), Value::Arr(vec![s("Use original observations")])),
                (
                    "constraints".into(),
                    Value::Arr(vec![
                        s("Do not copy an implementation"),
                        s(&format!("Implementation language: {}", language(fmt))),
                    ]),
                ),
            ]),
            obj(vec![
                ("action".into(), s("note")),
                ("kind".into(), s("design")),
                ("text".into(), s("Author an original design from the observed interface.")),
                ("citations".into(), Value::Arr(vec![n("1")])),
            ]),
            obj(vec![
                ("action".into(), s("check")),
                ("evidence".into(), n("1")),
                ("method".into(), s("equals")),
                ("expected".into(), s(&output)),
                ("requirement".into(), s("Use original observations")),
            ]),
        ];
        for action in actions {
            let action_name = match action.get("action") {
                Some(Value::Str(text)) => text.clone(),
                _ => panic!("{fmt} process action"),
            };
            let name = format!("{fmt} {action_name}");
            let arguments = obj(vec![("process".into(), action)]).to_string();
            d = reply(
                controller,
                &d,
                fmt,
                "workspace",
                vec![
                    ("tool".into(), s("hypotheses")),
                    ("arguments_json".into(), s(&arguments)),
                ],
                "",
                root,
            );
            demand(&name, text_is(state_of(&d, &name), "dialog", "approval"));
            d = controller.event(Some(state_of(&d, &name)), "approve", obj(vec![]), root);
            keep(&mut passed, &name, text_is(effect_of(&d, &name), "purpose", "model"));
        }

        let name = format!("{fmt} ordinary model plans produce a brief without raw returned source");
        d = reply(
            controller,
            &d,
            fmt,
            "hypotheses",
            vec![("process".into(), obj(vec![("action".into(), s("brief"))]))],
            "",
            root,
        );
        demand(&name, text_is(state_of(&d, &name), "dialog", "approval"));
        d = controller.event(Some(state_of(&d, &name)), "approve", obj(vec![]), root);
        demand(&name, text_is(effect_of(&d, &name), "purpose", "model"));
        let request = wire_body(effect_of(&d, &name), &name);
        let brief = find_text(&request, "\"kind\":\"design_brief\"").unwrap_or_else(|| panic!("{name}"));
        keep(
            &mut passed,
            &name,
            !brief.contains(&output)
                && brief.contains("Implementation language:")
                && brief.contains("Author an original design from the observed interface."),
        );

        let name = format!("{fmt} assessment");
        let plan = last_inquiry(&d, &name).get("plan").cloned().unwrap_or_else(|| panic!("{name}"));
        let digest = sha256_bytes(canonical_json(&plan).as_bytes());
        let assessment = obj(vec![
            ("action".into(), s("assess")),
            ("plan_sha256".into(), s(&digest)),
            ("item".into(), s("requirements")),
            ("index".into(), n("1")),
            ("status".into(), s("answered")),
            ("explanation".into(), s("The actual worker result was retained and compared.")),
            ("citations".into(), Value::Arr(vec![n("1")])),
            ("checks".into(), Value::Arr(vec![n("1")])),
        ]);
        d = reply(
            controller,
            &d,
            fmt,
            "hypotheses",
            vec![("process".into(), assessment)],
            "",
            root,
        );
        d = controller.event(Some(state_of(&d, &name)), "approve", obj(vec![]), root);
        let status = last_inquiry(&d, &name)
            .get("assessments")
            .and_then(last_of)
            .and_then(|item| item.get("status"));
        keep(&mut passed, &name, matches!(status, Some(Value::Str(text)) if text == "answered"));

        let name = format!("{fmt} captured child return");
        d = reply(
            controller,
            &d,
            fmt,
            "delegate",
            vec![
                ("agent".into(), n("1")),
                ("task".into(), s("Reconcile the observation")),
            ],
            "",
            root,
        );
        d = controller.event(Some(state_of(&d, &name)), "approve", obj(vec![]), root);
        demand(&name, number_is(state_of(&d, &name), "request_index", "1"));
        d = reply(controller, &d, fmt, "", vec![], "Actual child reply", root);
        keep(
            &mut passed,
            &name,
            number_is(state_of(&d, &name), "request_index", "0")
                && text_contains(effect_of(&d, &name), "wire", "Actual child reply"),
        );

        let name = format!("{fmt} final answer");
        d = reply(
            controller,
            &d,
            fmt,
            "",
            vec![],
            "Original answer based on the retained observations.",
            root,
        );
        keep(
            &mut passed,
            &name,
            is_falsy(state_of(&d, &name), "busy") && is_null(&d, "effect"),
        );

        let name = format!("{fmt} child failure returns to captured parent");
        let mut second = controller.event(
            Some(&st),
            "send",
            obj(vec![("draft".into(), s("Have the child inspect the problem."))]),
            root,
        );
        second = reply(
            controller,
            &second,
            fmt,
            "delegate",
            vec![("agent".into(), n("1")), ("task".into(), s("Investigate"))],
            "",
            root,
        );
        second = controller.event(Some(state_of(&second, &name)), "approve", obj(vec![]), root);
        let child_effect = effect_of(&second, &name).clone();
        let child_ticket = field(state_of(&second, &name), "model_expected", &name).clone();
        let mut failure_inputs = pick(&child_effect, &["child_ticket", "child_owner"]);
        put(&mut failure_inputs, "purpose", s("model"));
        put(&mut failure_inputs, "model_ticket", child_ticket);
        put(&mut failure_inputs, "error", s("Controlled child transport failure"));
        put(&mut failure_inputs, "sent", b(false));
        let failed = controller.event(
            Some(state_of(&second, &name)),
            "failure",
            failure_inputs.clone(),
            root,
        );
        let failed_effect = effect_of(&failed, &name);
        keep(
            &mut passed,
            &name,
            number_is(state_of(&failed, &name), "request_index", "0")
                && text_is(failed_effect, "purpose", "model")
                && text_contains(failed_effect, "wire", "Controlled child transport failure"),
        );

        let name = format!("{fmt} duplicate child failure cannot disturb parent request");
        let duplicate = controller.event(Some(state_of(&failed, &name)), "failure", failure_inputs, root);
        keep(
            &mut passed,
            &name,
            state_of(&duplicate, &name) == state_of(&failed, &name) && is_null(&duplicate, "effect"),
        );

        let name = format!("{fmt} parent can answer after child failure");
        d = reply(
            controller,
            &failed,
            fmt,
            "",
            vec![],
            "The child could not complete its request.",
            root,
        );
        keep(&mut passed, &name, is_falsy(state_of(&d, &name), "busy"));

        let name = format!("{fmt} cancelled callback refusal");
        let mut cancelled = controller.event(
            Some(&st),
            "send",
            obj(vec![("draft".into(), s("Investigate"))]),
            root,
        );
        cancelled = controller.event(Some(state_of(&cancelled, &name)), "cancel", obj(vec![]), root);
        // The leg replies against the cancelled state and an empty effect, so a
        // late read is not attributed to a child ticket from the cancel effect.
        let carrier = obj(vec![
            ("state".into(), state_of(&cancelled, &name).clone()),
            ("effect".into(), obj(vec![])),
        ]);
        let late = reply(
            controller,
            &carrier,
            fmt,
            "read",
            vec![("path".into(), s("evidence.txt"))],
            "",
            root,
        );
        let late_state = state_of(&late, &name);
        keep(
            &mut passed,
            &name,
            is_falsy(late_state, "busy") && matches!(late_state.get("pending"), None | Some(Value::Null)),
        );
    }
    passed
}

fn reply(
    controller: &mut Controller,
    d: &Value,
    fmt: &str,
    tool: &str,
    args: Vec<(String, Value)>,
    text: &str,
    root: &str,
) -> Value {
    let effect = d.get("effect").unwrap_or_else(|| panic!("event returned no effect"));
    let body = body(fmt, tool, args, text);
    let mut inputs = obj(vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), field(state(d), "model_expected", "model_expected").clone()),
        ("data".into(), s(&raw(&body))),
    ]);
    copy_present(effect, &mut inputs, "child_ticket");
    copy_present(effect, &mut inputs, "child_owner");
    controller.event(Some(state(d)), "network_result", inputs, root)
}

fn body(fmt: &str, tool: &str, args: Vec<(String, Value)>, text: &str) -> Value {
    let args = obj(args);
    match fmt {
        "chat" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if !tool.is_empty() {
                put(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![
                        ("id".into(), s("call_one")),
                        ("type".into(), s("function")),
                        (
                            "function".into(),
                            obj(vec![
                                ("name".into(), s(tool)),
                                ("arguments".into(), s(&args.to_string())),
                            ]),
                        ),
                    ])]),
                );
            }
            obj(vec![(
                "choices".into(),
                Value::Arr(vec![obj(vec![
                    ("index".into(), n("0")),
                    ("message".into(), message),
                    (
                        "finish_reason".into(),
                        s(if tool.is_empty() { "stop" } else { "tool_calls" }),
                    ),
                ])]),
            )])
        }
        "local" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if !tool.is_empty() {
                put(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![(
                        "function".into(),
                        obj(vec![("name".into(), s(tool)), ("arguments".into(), args)]),
                    )])]),
                );
            }
            obj(vec![
                ("message".into(), message),
                ("done".into(), b(true)),
                ("done_reason".into(), s("stop")),
            ])
        }
        "messages" => {
            let mut parts = vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])];
            let stop = if tool.is_empty() {
                "end_turn"
            } else {
                parts.push(obj(vec![
                    ("type".into(), s("tool_use")),
                    ("id".into(), s("call_one")),
                    ("name".into(), s(tool)),
                    ("input".into(), args),
                ]));
                "tool_use"
            };
            obj(vec![
                ("content".into(), Value::Arr(parts)),
                ("stop_reason".into(), s(stop)),
            ])
        }
        _ => {
            let mut parts = vec![obj(vec![("text".into(), s(text))])];
            if !tool.is_empty() {
                parts.push(obj(vec![(
                    "functionCall".into(),
                    obj(vec![("name".into(), s(tool)), ("args".into(), args)]),
                )]));
            }
            obj(vec![(
                "candidates".into(),
                Value::Arr(vec![obj(vec![
                    ("index".into(), n("0")),
                    (
                        "content".into(),
                        obj(vec![("role".into(), s("model")), ("parts".into(), Value::Arr(parts))]),
                    ),
                    ("finishReason".into(), s("STOP")),
                ])]),
            )])
        }
    }
}

fn raw(body: &Value) -> String {
    let bytes = body.to_string().into_bytes();
    let mut response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", bytes.len()).into_bytes();
    response.extend_from_slice(&bytes);
    b64_encode(&response)
}

fn tool_op(controller: &mut Controller, effect: &Value, root: &str, name: &str) -> String {
    let mut envelope = effect.clone();
    put(&mut envelope, "op", s("tool"));
    put(&mut envelope, "root", s(root));
    let value = controller.op(&envelope);
    match value {
        Value::Str(text) => text,
        _ => panic!("{name}"),
    }
}

fn language(fmt: &str) -> &'static str {
    match fmt {
        "chat" => "Rust",
        "local" => "C++",
        "messages" => "C#",
        _ => "Swift",
    }
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn state_of<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get("state").unwrap_or_else(|| panic!("{name}"))
}

fn effect_of<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get("effect").unwrap_or_else(|| panic!("{name}"))
}

fn field<'a>(value: &'a Value, key: &str, name: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("{name}"))
}

fn last_of(value: &Value) -> Option<&Value> {
    match value {
        Value::Arr(items) => items.last(),
        _ => None,
    }
}

fn last_inquiry<'a>(value: &'a Value, name: &str) -> &'a Value {
    state_of(value, name)
        .get("inquiry_records")
        .and_then(|records| records.get("items"))
        .and_then(last_of)
        .unwrap_or_else(|| panic!("{name}"))
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn text_contains(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text.contains(expect))
}

fn is_falsy(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(item) if !truthy(item))
}

fn is_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Null))
}

fn number_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Num(number)) if number == expect)
}

fn copy_present(source: &Value, target: &mut Value, key: &str) {
    if let Some(value) = source.get(key) {
        put(target, key, value.clone());
    }
}

fn project(source: &Value, keys: &[&str], name: &str) -> Value {
    let Value::Obj(fields) = source else {
        panic!("{name}");
    };
    let mut out = Vec::new();
    for (key, value) in fields {
        if keys.iter().any(|candidate| candidate == key) {
            out.push((key.clone(), value.clone()));
        }
    }
    Value::Obj(out)
}

fn pick(source: &Value, keys: &[&str]) -> Value {
    let mut out = Vec::new();
    for key in keys {
        if let Some(value) = source.get(key) {
            out.push(((*key).to_string(), value.clone()));
        }
    }
    Value::Obj(out)
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn wire_body(effect: &Value, name: &str) -> Value {
    let wire = match effect.get("wire") {
        Some(Value::Str(text)) => text.as_str(),
        _ => panic!("{name}"),
    };
    let body = wire.split_once("\r\n\r\n").unwrap_or_else(|| panic!("{name}")).1;
    Value::parse(body)
}

fn find_text(value: &Value, needle: &str) -> Option<String> {
    match value {
        Value::Str(text) if text.contains(needle) => Some(text.clone()),
        Value::Arr(items) => items.iter().find_map(|item| find_text(item, needle)),
        Value::Obj(fields) => fields.iter().find_map(|(_, item)| find_text(item, needle)),
        _ => None,
    }
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Num(number) => number.clone(),
        Value::Str(text) => canonical_string(text),
        Value::Arr(items) => {
            let body = items.iter().map(canonical_json).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let mut sorted = fields.clone();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            let body = sorted
                .iter()
                .map(|(key, value)| format!("{}:{}", canonical_string(key), canonical_json(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

fn canonical_string(text: &str) -> String {
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

fn demand(name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    demand(name, ok);
    passed.push(name.to_string());
}

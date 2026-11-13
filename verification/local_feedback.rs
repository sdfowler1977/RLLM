mod twin;

use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-interface checks using actual missing-file workers, four provider envelopes, native review, exact callback ownership, current permission identity and response admission.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = twin::create_scratch(&root, "delegation-visibility");
    let scratch_text = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch directory is not UTF-8"))
        .to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();
    // The leg writes json.dumps(result, indent=2) + '\n', so the report body is built here
    // instead of via twin::report (which writes no trailing newline).
    let body = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("local_feedback", &body, true);
    println!("{} local_feedback checks passed", passed.len());
    let _ = std::fs::remove_dir_all(&scratch);
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();

    for fmt in ["chat", "local", "messages", "content"] {
        let initial_state = state(controller, root, fmt);
        let mut delegated = proposed(controller, root, &initial_state);
        let original = delegated.clone();
        let error = worker_error(controller, root, &delegated);
        delegated = failed(controller, root, &delegated, &error);

        keep(
            &mut passed,
            &format!("{fmt} actual failed worker returns to its requesting model"),
            delegated.get("effect").is_some_and(|effect| {
                effect.get("purpose").is_some_and(|purpose| purpose.str_() == "model")
            }) && number(&delegated, "state", "request_index") == 0
                && prompt(&delegated).contains(&error)
                && prompt(&delegated).contains("\"status\":\"failed\"")
                && !truthy_at(&delegated, "state", "operation_expected")
                && !truthy_at(&delegated, "state", "inquiry_capture"),
        );
        keep(
            &mut passed,
            &format!("{fmt} actual error is retained with an explicit failed outcome"),
            messages(&delegated).iter().any(|message| {
                let content = message.get("content").map(Value::str_).unwrap_or("");
                content.contains(&error) && content.contains("\"completed_result\":false")
            }),
        );

        let duplicate = failed(controller, root, &original, &error);
        keep(
            &mut passed,
            &format!("{fmt} consumed worker callback is inert on replay"),
            matches!(duplicate.get("effect"), Some(Value::Null))
                && duplicate.get("state") == original.get("state"),
        );

        delegated = reply(
            controller,
            root,
            &delegated,
            None,
            None,
            "The file read returned its actual missing-file error.",
        );
        keep(
            &mut passed,
            &format!("{fmt} model can settle using the actual operation failure"),
            !truthy_at(&delegated, "state", "busy")
                && !truthy_at(&delegated, "state", "agent_running"),
        );
    }

    let delegated_state = state(controller, root, "chat");
    let delegated = proposed(controller, root, &delegated_state);
    let effect = delegated
        .get("effect")
        .unwrap_or_else(|| panic!("delegation produced no effect"));
    let forged = event(
        controller,
        root,
        delegated.get("state"),
        "failure",
        obj(vec![
            ("purpose".into(), s("local_tool")),
            ("tool_ticket".into(), s("wrong-ticket")),
            ("tool_binding".into(), twin::n("1")),
            ("error".into(), s("FORGED_ERROR")),
        ]),
    );
    keep(
        &mut passed,
        "a wrong callback owner cannot enter the model conversation",
        matches!(forged.get("effect"), Some(Value::Null)) && forged.get("state") == delegated.get("state"),
    );

    let mut changed = delegated.get("state").cloned().unwrap_or_else(|| panic!("no state"));
    put_nested(&mut changed, &["agents", "0", "tools"], s("hypotheses"));
    let revoked = event(
        controller,
        root,
        Some(&changed),
        "failure",
        obj(vec![
            ("purpose".into(), s("local_tool")),
            (
                "tool_ticket".into(),
                effect.get("tool_ticket").cloned().unwrap_or_else(|| panic!("no tool ticket")),
            ),
            ("tool_binding".into(), twin::n("1")),
            ("error".into(), s("REVOKED_ERROR")),
        ]),
    );
    keep(
        &mut passed,
        "permission change preserves callback rejection",
        matches!(revoked.get("effect"), Some(Value::Null)) && revoked.get("state") == Some(&changed),
    );

    let cancelled_state = state(controller, root, "chat");
    // The leg fails the PRE-cancellation snapshot, so the held ownership is still in the state it
    // feeds; feeding the post-cancel reply would test a different (already revoked) callback.
    let original = proposed(controller, root, &cancelled_state);
    event(controller, root, original.get("state"), "cancel", obj(vec![]));
    let late = failed(controller, root, &original, "LATE_CANCELLED_ERROR");
    keep(
        &mut passed,
        "native cancellation revokes held local failure ownership",
        matches!(late.get("effect"), Some(Value::Null)) && late.get("state") == original.get("state"),
    );

    let mut manual = event(controller, root, None, "investigation", obj(vec![]));
    manual = event(
        controller,
        root,
        manual.get("state"),
        "inspect_file",
        obj(vec![("path".into(), s("missing.txt"))]),
    );
    manual = event(controller, root, manual.get("state"), "approve", obj(vec![]));
    let manual_error = worker_error(controller, root, &manual);
    manual = failed(controller, root, &manual, &manual_error);
    keep(
        &mut passed,
        "native manual failure settles locally",
        matches!(manual.get("effect"), Some(Value::Null))
            && !truthy_at(&manual, "state", "busy")
            && text_at(&manual, "state", "notice").contains(&manual_error),
    );

    let refused_state = state(controller, root, "chat");
    let mut refused = proposed(controller, root, &refused_state);
    let refusal_error = worker_error(controller, root, &refused);
    let mut small = refused.clone();
    put_nested(
        &mut small,
        &["state", "device"],
        obj(vec![
            ("heap_bytes".into(), twin::n("2097152")),
            ("memory_bytes".into(), twin::n("4194304")),
            ("cores".into(), twin::n("1")),
        ]),
    );
    refused = failed(controller, root, &small, &refusal_error);
    keep(
        &mut passed,
        "response admission refusal retains the actual error and settles the request",
        matches!(refused.get("effect"), Some(Value::Null))
            && !truthy_at(&refused, "state", "busy")
            && !truthy_at(&refused, "state", "model_expected")
            && !truthy_at(&refused, "state", "agent_running")
            && messages(&refused).iter().any(|message| {
                message
                    .get("content")
                    .map(Value::str_)
                    .unwrap_or("")
                    .contains(&refusal_error)
            })
            && text_at(&refused, "state", "notice").contains("Model response allowance"),
    );

    let reviewed_state = state(controller, root, "chat");
    let mut reviewed = proposed(controller, root, &reviewed_state);
    let reviewed_error = worker_error(controller, root, &reviewed);
    reviewed = failed(controller, root, &reviewed, &reviewed_error);
    for _ in 0..2 {
        reviewed = reply(
            controller,
            root,
            &reviewed,
            Some("read"),
            Some(obj(vec![("path".into(), s("missing.txt"))])),
            "Answer",
        );
        reviewed = event(controller, root, reviewed.get("state"), "approve", obj(vec![]));
        let error = worker_error(controller, root, &reviewed);
        reviewed = failed(controller, root, &reviewed, &error);
    }
    keep(
        &mut passed,
        "three new reviewed failed operations return three actual model results",
        reviewed.get("effect").is_some()
            && number(&reviewed, "state", "local_failure_followups") == 3
            && number(&reviewed, "state", "steps") == 3,
    );

    reviewed = reply(
        controller,
        root,
        &reviewed,
        Some("read"),
        Some(obj(vec![("path".into(), s("missing.txt"))])),
        "Answer",
    );
    reviewed = event(controller, root, reviewed.get("state"), "approve", obj(vec![]));
    let repeated_error = worker_error(controller, root, &reviewed);
    reviewed = failed(controller, root, &reviewed, &repeated_error);
    keep(
        &mut passed,
        "repeated worker failures settle at the bounded feedback limit",
        matches!(reviewed.get("effect"), Some(Value::Null))
            && !truthy_at(&reviewed, "state", "busy")
            && !truthy_at(&reviewed, "state", "agent_running")
            && text_at(&reviewed, "state", "notice").contains("Three operation error results")
            && messages(&reviewed)
                .iter()
                .filter(|message| {
                    message
                        .get("content")
                        .map(Value::str_)
                        .unwrap_or("")
                        .contains("\"status\":\"failed\"")
                })
                .count()
                == 4,
    );

    let sent = event(
        controller,
        root,
        reviewed.get("state"),
        "send",
        obj(vec![("draft".into(), s("Start a fresh read of missing.txt."))]),
    );
    let mut fresh = reply(
        controller,
        root,
        &sent,
        Some("read"),
        Some(obj(vec![("path".into(), s("missing.txt"))])),
        "Answer",
    );
    fresh = event(controller, root, fresh.get("state"), "approve", obj(vec![]));
    let fresh_error = worker_error(controller, root, &fresh);
    fresh = failed(controller, root, &fresh, &fresh_error);
    keep(
        &mut passed,
        "fresh native instruction resets bounded local error feedback",
        fresh.get("effect").is_some()
            && number(&fresh, "state", "local_failure_followups") == 1,
    );

    let mut selected_state = state(controller, root, "chat");
    put_nested(&mut selected_state, &["agents", "0", "model"], s("2"));
    let mut selected = proposed(controller, root, &selected_state);
    let selected_error = worker_error(controller, root, &selected);
    selected = failed(controller, root, &selected, &selected_error);
    keep(
        &mut passed,
        "local failure resumes the actual selected connection",
        selected.get("effect").is_some()
            && number(&selected, "state", "request_index") == 2
            && text_at(&selected, "state", "request_model") == "CHILD_MODEL_TWO",
    );

    let mut standalone_state = state(controller, root, "chat");
    put_nested(
        &mut standalone_state,
        &["routing"],
        obj(vec![("subagent".into(), s("2"))]),
    );
    let mut standalone = event(
        controller,
        root,
        Some(&standalone_state),
        "delegate",
        obj(vec![(
            "task".into(),
            s("Read missing.txt and explain the actual result."),
        )]),
    );
    standalone = reply(
        controller,
        root,
        &standalone,
        Some("read"),
        Some(obj(vec![("path".into(), s("missing.txt"))])),
        "Answer",
    );
    standalone = event(controller, root, standalone.get("state"), "approve", obj(vec![]));
    let standalone_error = worker_error(controller, root, &standalone);
    standalone = failed(controller, root, &standalone, &standalone_error);
    keep(
        &mut passed,
        "standalone task failure resumes its assigned connection",
        standalone.get("effect").is_some()
            && number(&standalone, "state", "request_index") == 2
            && prompt(&standalone).contains(&standalone_error)
            && last_tool_output(&standalone).contains(&standalone_error),
    );
    standalone = reply(
        controller,
        root,
        &standalone,
        None,
        None,
        "Standalone result from the failed file read.",
    );
    keep(
        &mut passed,
        "standalone task saves its final answer after the actual failure",
        matches!(standalone.get("effect"), Some(Value::Null))
            && !truthy_at(&standalone, "state", "busy")
            && messages(&standalone).iter().any(|message| {
                message
                    .get("content")
                    .map(Value::str_)
                    .unwrap_or("")
                    .contains("Standalone result from the failed file read.")
            }),
    );

    let paused_state = state(controller, root, "chat");
    let mut paused = proposed(controller, root, &paused_state);
    let paused_error = worker_error(controller, root, &paused);
    put_nested(&mut paused, &["state", "capability_mode"], s("local"));
    paused = failed(controller, root, &paused, &paused_error);
    keep(
        &mut passed,
        "paused remote mode retains the operation error and settles locally",
        matches!(paused.get("effect"), Some(Value::Null))
            && !truthy_at(&paused, "state", "busy")
            && !truthy_at(&paused, "state", "agent_running")
            && messages(&paused).iter().any(|message| {
                message
                    .get("content")
                    .map(Value::str_)
                    .unwrap_or("")
                    .contains(&paused_error)
            }),
    );

    let child_state = state(controller, root, "chat");
    let mut child = proposal(
        controller,
        root,
        &child_state,
        obj(vec![
            ("agent".into(), twin::n("1")),
            (
                "task".into(),
                s("Read missing.txt and return the actual result."),
            ),
        ]),
    );
    child = event(controller, root, child.get("state"), "approve", obj(vec![]));
    child = reply(
        controller,
        root,
        &child,
        Some("read"),
        Some(obj(vec![("path".into(), s("missing.txt"))])),
        "Answer",
    );
    child = event(controller, root, child.get("state"), "approve", obj(vec![]));
    let child_error = worker_error(controller, root, &child);
    child = failed(controller, root, &child, &child_error);
    keep(
        &mut passed,
        "actual child worker failure retains its captured parent return",
        child.get("effect").is_some()
            && number(&child, "state", "request_index") == 0
            && !truthy_at(&child, "state", "delegation_stack")
            && prompt(&child).contains(&child_error),
    );

    passed
}

fn state(controller: &mut Controller, root: &str, fmt: &str) -> Value {
    let mut state = event(controller, root, None, "resume", obj(vec![]))
        .get("state")
        .cloned()
        .unwrap_or_else(|| panic!("resume produced no state"));
    let profiles = ["Primary connection", "Exact child one", "Exact child two", "Unlinked connection"]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            obj(vec![
                ("name".into(), s(name)),
                (
                    "model".into(),
                    s(match index {
                        0 => "PRIMARY_MODEL",
                        1 => "CHILD_MODEL_ONE",
                        2 => "CHILD_MODEL_TWO",
                        _ => "UNLINKED_MODEL",
                    }),
                ),
                ("protocol".into(), s(fmt)),
                (
                    "endpoint".into(),
                    s(&format!(
                        "http://PRIVATE_{index}.test/v1/chat/completions?token=ENDPOINT_SECRET"
                    )),
                ),
                ("key".into(), s(&format!("KEY_SECRET_{index}"))),
                ("auth_header".into(), s("X-PRIVATE-AUTH")),
                ("auth_prefix".into(), s("PREFIX_SECRET")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
                ("tool_format".into(), s("structured")),
            ])
        })
        .collect();
    put(&mut state, "profiles", Value::Arr(profiles));
    put(
        &mut state,
        "agents",
        Value::Arr(vec![
            obj(vec![
                ("name".into(), s("Parent")),
                ("model".into(), s("0")),
                ("tools".into(), s("read,hypotheses,delegate")),
                (
                    "subagents".into(),
                    Value::Arr(vec![twin::n("1"), twin::n("2")]),
                ),
            ]),
            obj(vec![
                ("name".into(), s("Research child")),
                ("role".into(), s("coding")),
                ("specialty".into(), s("Research")),
                ("model".into(), s("1")),
                ("tools".into(), s("read,hypotheses")),
            ]),
            obj(vec![
                ("name".into(), s("Review child")),
                ("role".into(), s("nonwrite")),
                ("specialty".into(), s("Review")),
                ("model".into(), s("2")),
                ("tools".into(), s("read,hypotheses")),
            ]),
        ]),
    );
    state
}

fn proposed(controller: &mut Controller, root: &str, state: &Value) -> Value {
    let sent = event(
        controller,
        root,
        Some(state),
        "send",
        obj(vec![(
            "draft".into(),
            s("Read missing.txt and explain the actual file result."),
        )]),
    );
    let delegated = reply(
        controller,
        root,
        &sent,
        Some("read"),
        Some(obj(vec![("path".into(), s("missing.txt"))])),
        "Answer",
    );
    event(controller, root, delegated.get("state"), "approve", obj(vec![]))
}

fn proposal(
    controller: &mut Controller,
    root: &str,
    state: &Value,
    args: Value,
) -> Value {
    let sent = event(
        controller,
        root,
        Some(state),
        "send",
        obj(vec![(
            "draft".into(),
            s("Use a configured permitted child for this task."),
        )]),
    );
    let delegated = reply(controller, root, &sent, Some("delegate"), Some(args), "Answer");
    event(controller, root, delegated.get("state"), "approve", obj(vec![]))
}

fn reply(
    controller: &mut Controller,
    root: &str,
    value: &Value,
    tool: Option<&str>,
    args: Option<Value>,
    text: &str,
) -> Value {
    let protocol = text_at(value, "state", "request_protocol");
    let body = provider_body(&protocol, tool, args.as_ref(), text);
    let body = serde_json(&body);
    let wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let model_ticket = text_at(value, "state", "model_expected");
    let encoded = b64_encode(wire.as_bytes());
    event(
        controller,
        root,
        value.get("state"),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), s(&model_ticket)),
            ("data".into(), s(&encoded)),
        ]),
    )
}

fn provider_body(fmt: &str, tool: Option<&str>, args: Option<&Value>, text: &str) -> Value {
    let arguments = args.cloned().unwrap_or_else(|| obj(vec![]));
    match fmt {
        "chat" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if let Some(name) = tool {
                let arguments = serde_json(&arguments);
                put(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![
                        ("id".into(), s("call_one")),
                        ("type".into(), s("function")),
                        (
                            "function".into(),
                            obj(vec![
                                ("name".into(), s(name)),
                                ("arguments".into(), s(&arguments)),
                            ]),
                        ),
                    ])]),
                );
            }
            obj(vec![(
                "choices".into(),
                Value::Arr(vec![obj(vec![
                    ("index".into(), twin::n("0")),
                    ("message".into(), message),
                    (
                        "finish_reason".into(),
                        s(if tool.is_some() { "tool_calls" } else { "stop" }),
                    ),
                ])]),
            )])
        }
        "local" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if let Some(name) = tool {
                put(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![(
                        "function".into(),
                        obj(vec![("name".into(), s(name)), ("arguments".into(), arguments)]),
                    )])]),
                );
            }
            obj(vec![
                ("message".into(), message),
                ("done".into(), twin::b(true)),
                ("done_reason".into(), s("stop")),
            ])
        }
        "messages" => {
            let mut parts = vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])];
            if let Some(name) = tool {
                parts.push(obj(vec![
                    ("type".into(), s("tool_use")),
                    ("id".into(), s("call_one")),
                    ("name".into(), s(name)),
                    ("input".into(), arguments),
                ]));
            }
            obj(vec![
                ("content".into(), Value::Arr(parts)),
                (
                    "stop_reason".into(),
                    s(if tool.is_some() { "tool_use" } else { "end_turn" }),
                ),
            ])
        }
        "content" => {
            let mut parts = vec![obj(vec![("text".into(), s(text))])];
            if let Some(name) = tool {
                parts.push(obj(vec![(
                    "functionCall".into(),
                    obj(vec![("name".into(), s(name)), ("args".into(), arguments)]),
                )]));
            }
            obj(vec![(
                "candidates".into(),
                Value::Arr(vec![obj(vec![
                    ("index".into(), twin::n("0")),
                    (
                        "content".into(),
                        obj(vec![
                            ("role".into(), s("model")),
                            ("parts".into(), Value::Arr(parts)),
                        ]),
                    ),
                    ("finishReason".into(), s("STOP")),
                ])]),
            )])
        }
        other => panic!("unknown request protocol: {other}"),
    }
}

fn worker_error(controller: &mut Controller, root: &str, value: &Value) -> String {
    let effect = value
        .get("effect")
        .unwrap_or_else(|| panic!("worker proposal produced no effect"));
    let mut envelope = match effect {
        Value::Obj(fields) => fields.clone(),
        _ => panic!("effect is not an object"),
    };
    envelope.retain(|(key, _)| key != "op");
    envelope.push(("op".into(), s("tool")));
    envelope.push(("root".into(), s(root)));
    controller.failed(&Value::Obj(envelope))
}

fn failed(
    controller: &mut Controller,
    root: &str,
    value: &Value,
    error: &str,
) -> Value {
    let effect = value
        .get("effect")
        .unwrap_or_else(|| panic!("no effect to fail"));
    let mut inputs = obj(vec![]);
    for key in ["purpose", "tool_ticket", "tool_binding"] {
        if let Some(field) = effect.get(key) {
            inputs = Value::Obj(match inputs {
                Value::Obj(mut fields) => {
                    fields.push((key.to_string(), field.clone()));
                    fields
                }
                _ => unreachable!(),
            });
        }
    }
    inputs = Value::Obj(match inputs {
        Value::Obj(mut fields) => {
            fields.push(("error".into(), s(error)));
            fields
        }
        _ => unreachable!(),
    });
    event(controller, root, value.get("state"), "failure", inputs)
}

fn event(
    controller: &mut Controller,
    root: &str,
    state: Option<&Value>,
    action: &str,
    inputs: Value,
) -> Value {
    controller.event(state, action, inputs, root)
}

fn messages(value: &Value) -> Vec<Value> {
    let state = value
        .get("state")
        .unwrap_or_else(|| panic!("value has no state"));
    let chats = state
        .get("chats")
        .unwrap_or_else(|| panic!("state has no chats"))
        .arr();
    let active = state
        .get("active")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("state has no active chat"));
    chats
        .iter()
        .find(|chat| chat.get("id").map(Value::str_) == Some(active))
        .and_then(|chat| chat.get("messages"))
        .unwrap_or_else(|| panic!("active chat has no messages"))
        .arr()
        .clone()
}

fn prompt(value: &Value) -> String {
    let effect = value
        .get("effect")
        .unwrap_or_else(|| panic!("value has no effect"));
    let wire = effect
        .get("wire")
        .unwrap_or_else(|| panic!("effect has no wire"))
        .str_();
    let body = wire
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("wire has no HTTP body"))
        .1;
    // The request wire is plain text; the leg parses its body directly, no base64 layer.
    let parsed = Value::parse(body);
    strings(&parsed).join("\n")
}

fn strings(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_strings(value, &mut out);
    out
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Str(text) => out.push(text.clone()),
        Value::Arr(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Obj(fields) => {
            for (_, item) in fields {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

fn last_tool_output(value: &Value) -> String {
    let state = value
        .get("state")
        .unwrap_or_else(|| panic!("value has no state"));
    let evidence = state
        .get("task_run")
        .and_then(|task| task.get("tool_evidence"))
        .unwrap_or_else(|| panic!("task has no tool evidence"))
        .arr();
    evidence
        .last()
        .and_then(|item| item.get("output"))
        .map(Value::str_)
        .unwrap_or_else(|| panic!("tool evidence has no output"))
        .to_string()
}

fn serde_json(value: &Value) -> String {
    value.to_string()
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn text_at(value: &Value, outer: &str, key: &str) -> String {
    value
        .get(outer)
        .and_then(|inner| inner.get(key))
        .map(Value::str_)
        .unwrap_or_else(|| panic!("missing {outer}.{key}"))
        .to_string()
}

fn truthy_at(value: &Value, outer: &str, key: &str) -> bool {
    value
        .get(outer)
        .and_then(|inner| inner.get(key))
        .is_some_and(twin::truthy)
}

fn number(value: &Value, outer: &str, key: &str) -> i64 {
    value
        .get(outer)
        .and_then(|inner| inner.get(key))
        .unwrap_or_else(|| panic!("missing {outer}.{key}"))
        .to_string()
        .parse()
        .unwrap_or_else(|error| panic!("invalid {outer}.{key}: {error}"))
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

fn put_nested(value: &mut Value, path: &[&str], next: Value) {
    let (key, rest) = match path.split_first() {
        Some(pair) => pair,
        None => unreachable!(),
    };
    if rest.is_empty() {
        put(value, key, next);
        return;
    }
    match value {
        // The leg indexes fixture lists positionally (`state['agents'][0]['tools']`), so a numeric
        // path component walks a Value::Arr while a named one walks a Value::Obj.
        Value::Obj(fields) => {
            let index = fields
                .iter()
                .position(|(name, _)| name == key)
                .unwrap_or_else(|| panic!("missing {key}"));
            put_nested(&mut fields[index].1, rest, next);
        }
        Value::Arr(items) => {
            let index = key
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("array index expected, got {key}"));
            put_nested(&mut items[index], rest, next);
        }
        other => panic!("expected container at {key}, found {}", other.to_string()),
    }
}
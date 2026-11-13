mod twin;

use twin::{b, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-event checks: current review identity, four provider formats, queued child question/route ownership, duplicate/conflicting proposals, actual manual file execution, unapproved argument privacy and bounded feedback.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&root, "delegation-visibility");
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch_text = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    std::fs::write(scratch.join("actual.txt"), "A separately reviewed read.")
        .unwrap_or_else(|error| panic!("could not write actual.txt: {error}"));

    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();
    twin::report_value(
        "review_ownership",
        &obj(vec![
            ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} review_ownership checks passed", passed.len());
    let _ = std::fs::remove_dir_all(&scratch);
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();

    for fmt in ["chat", "local", "messages", "content"] {
        let fresh = state(controller, root, fmt);
        let mut d = proposal(controller, root, fresh, delegate_args("ORIGINAL_REVIEW_TASK"));
        let original = field(state_of(&d), "pending").clone();
        let origin = field(&original, "delegate_question").clone();

        d = event(
            controller,
            Some(state_of(&d)),
            "send",
            obj(vec![("draft".into(), s("What is the original task status?"))]),
            root,
        );
        d = reply(controller, root, state_of(&d), Some(("read", read_args())), None, "Answer");
        keep(
            &mut passed,
            &format!("{fmt} later valid proposal retains the exact current review"),
            refused(&d, "read", "already awaiting review")
                && same(state_of(&d), "pending", &original)
                && missing_or_falsy(state_of(&d), "delegation_stack"),
        );
        let shown = catalog(&d);
        keep(
            &mut passed,
            &format!("{fmt} current child review remains visible to the model"),
            pending_visible(&shown, &origin),
        );

        d = reply(controller, root, state_of(&d), Some(("delegate", delegate_args("ORIGINAL_REVIEW_TASK"))), None, "Answer");
        keep(
            &mut passed,
            &format!("{fmt} duplicate child proposal retains the original review owner"),
            refused(&d, "delegate", "already awaiting review") && same(state_of(&d), "pending", &original),
        );

        d = reply(
            controller,
            root,
            state_of(&d),
            Some((
                "delegate",
                obj(vec![("model".into(), s("GUESS")), ("task".into(), s("REPLACEMENT_TASK"))]),
            )),
            None,
            "Answer",
        );
        keep(
            &mut passed,
            &format!("{fmt} rejected child selector leaves the original review intact"),
            refused(&d, "delegate", "already awaiting review") && same(state_of(&d), "pending", &original),
        );

        d = reply(controller, root, state_of(&d), None, None, "The original child task is still awaiting review.");
        d = event(controller, Some(state_of(&d)), "review", obj(vec![]), root);
        d = event(controller, Some(state_of(&d)), "approve", obj(vec![]), root);
        let started = {
            let state_value = state_of(&d);
            let child_id = field(field(state_value, "task_run"), "inquiry_id");
            let case = field(field(state_value, "inquiry_records"), "items")
                .arr()
                .iter()
                .find(|item| item.get("id").is_some_and(|id| id == child_id));
            match case {
                Some(case) => {
                    eq_num(state_value, "request_index", "1")
                        && case.get("parent").is_some_and(|parent| parent == &origin)
                        && eq_str(field(state_value, "task_run"), "task", "ORIGINAL_REVIEW_TASK")
                }
                None => false,
            }
        };
        keep(&mut passed, &format!("{fmt} original review starts only its exact child and question"), started);
        d = finish(controller, root, d);
        let _ = d;
    }

    let configured = state(controller, root, "chat");
    let mut d = event(controller, Some(&configured), "editor", obj(vec![]), root);
    d = event(
        controller,
        Some(state_of(&d)),
        "editor_save",
        obj(vec![("path".into(), s("manual.txt")), ("content".into(), s("PRIVATE_MANUAL_CONTENT"))]),
        root,
    );
    let original = field(state_of(&d), "pending").clone();
    d = event(
        controller,
        Some(state_of(&d)),
        "send",
        obj(vec![("draft".into(), s("What action is currently waiting?"))]),
        root,
    );
    d = reply(controller, root, state_of(&d), Some(("read", read_args())), None, "Answer");
    keep(
        &mut passed,
        "manual write review survives a conflicting model proposal",
        refused(&d, "read", "already awaiting review")
            && same(state_of(&d), "pending", &original)
            && falsy(state_of(&d), "agent_pending"),
    );
    let shown = prompt(&d);
    keep(
        &mut passed,
        "review conflict does not disclose unapproved manual arguments",
        !shown.contains("PRIVATE_MANUAL_CONTENT") && !shown.contains("manual.txt"),
    );

    d = reply(controller, root, state_of(&d), None, None, "The manual operation is awaiting review.");
    d = event(controller, Some(state_of(&d)), "review", obj(vec![]), root);
    d = event(controller, Some(state_of(&d)), "approve", obj(vec![]), root);
    let effect = match d.get("effect") {
        Some(Value::Obj(_)) => d.get("effect").cloned().unwrap(),
        _ => Value::Null,
    };
    if let Value::Obj(_) = &effect {
        let output = execute_tool(controller, &effect, root);
        d = event(controller, Some(state_of(&d)), "tool_result", tool_result_inputs(&effect, output), root);
    }
    let manual = std::fs::read_to_string(std::path::Path::new(root).join("manual.txt")).unwrap_or_default();
    keep(
        &mut passed,
        "original manual review still executes its exact approved file operation",
        manual == "PRIVATE_MANUAL_CONTENT" && is_null(&d, "effect") && falsy(state_of(&d), "busy"),
    );

    let mut web_state = state(controller, root, "chat");
    append_parent_tool(&mut web_state, "web_read");
    d = proposal(controller, root, web_state, delegate_args("Keep original child review."));
    let original = field(state_of(&d), "pending").clone();
    d = event(
        controller,
        Some(state_of(&d)),
        "send",
        obj(vec![("draft".into(), s("Report current status."))]),
        root,
    );
    d = reply(
        controller,
        root,
        state_of(&d),
        Some(("web_read", obj(vec![("url".into(), s("https://example.test/"))]))),
        None,
        "Answer",
    );
    keep(
        &mut passed,
        "later web proposal preserves current review before any page dispatch",
        refused(&d, "web_read", "already awaiting review")
            && same(state_of(&d), "pending", &original)
            && missing_or_falsy(state_of(&d), "web_expected"),
    );
    d = reply(controller, root, state_of(&d), None, None, "Original review is still current.");
    d = event(controller, Some(state_of(&d)), "reject", obj(vec![]), root);
    keep(&mut passed, "explicit native dismissal clears the original review", absent_or_null(state_of(&d), "pending"));
    d = event(
        controller,
        Some(state_of(&d)),
        "send",
        obj(vec![("draft".into(), s("Read actual.txt instead."))]),
        root,
    );
    d = reply(controller, root, state_of(&d), Some(("read", read_args())), None, "Answer");
    keep(
        &mut passed,
        "a fresh operation is available after explicit dismissal",
        eq_str(state_of(&d), "dialog", "approval") && eq_str(field(state_of(&d), "pending"), "tool", "read"),
    );
    let _ = event(controller, Some(state_of(&d)), "reject", obj(vec![]), root);

    let fresh = state(controller, root, "chat");
    d = proposal(controller, root, fresh, delegate_args("Preserve through bounded conflicting feedback."));
    let original = field(state_of(&d), "pending").clone();
    d = event(controller, Some(state_of(&d)), "send", obj(vec![("draft".into(), s("Report status."))]), root);
    for _ in 0..3 {
        d = reply(controller, root, state_of(&d), Some(("read", read_args())), None, "Answer");
    }
    d = reply(controller, root, state_of(&d), Some(("read", read_args())), None, "Answer");
    keep(
        &mut passed,
        "feedback exhaustion retains the original current review",
        is_null(&d, "effect") && falsy(state_of(&d), "busy") && same(state_of(&d), "pending", &original),
    );
    d = event(controller, Some(state_of(&d)), "review", obj(vec![]), root);
    d = event(controller, Some(state_of(&d)), "approve", obj(vec![]), root);
    keep(
        &mut passed,
        "current review still works after conflicting feedback settles",
        effect_present(&d) && eq_num(state_of(&d), "request_index", "1"),
    );
    d = finish(controller, root, d);
    let _ = d;
    passed
}

fn state(controller: &mut Controller, root: &str, fmt: &str) -> Value {
    let resumed = event(controller, None, "resume", obj(vec![]), root);
    let mut state_value = field(&resumed, "state").clone();
    put(&mut state_value, "profiles", profiles(fmt));
    put(&mut state_value, "agents", agents());
    state_value
}

fn profiles(fmt: &str) -> Value {
    let rows = [
        ("Primary connection", "PRIMARY_MODEL"),
        ("Exact child one", "CHILD_MODEL_ONE"),
        ("Exact child two", "CHILD_MODEL_TWO"),
        ("Unlinked connection", "UNLINKED_MODEL"),
    ];
    Value::Arr(
        rows.iter()
            .enumerate()
            .map(|(index, (name, model))| {
                obj(vec![
                    ("name".into(), s(name)),
                    ("model".into(), s(model)),
                    ("protocol".into(), s(fmt)),
                    (
                        "endpoint".into(),
                        s(&format!("http://PRIVATE_{index}.test/v1/chat/completions?token=ENDPOINT_SECRET")),
                    ),
                    ("key".into(), s(&format!("KEY_SECRET_{index}"))),
                    ("auth_header".into(), s("X-PRIVATE-AUTH")),
                    ("auth_prefix".into(), s("PREFIX_SECRET")),
                    ("plain".into(), b(true)),
                    ("stream".into(), b(false)),
                    ("tool_format".into(), s("structured")),
                ])
            })
            .collect(),
    )
}

fn agents() -> Value {
    Value::Arr(vec![
        obj(vec![
            ("name".into(), s("Parent")),
            ("model".into(), s("0")),
            ("tools".into(), s("read,hypotheses,delegate")),
            ("subagents".into(), Value::Arr(vec![n("1"), n("2")])),
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
    ])
}

fn append_parent_tool(state_value: &mut Value, extra: &str) {
    let agents = field_mut(state_value, "agents");
    let Value::Arr(items) = agents else {
        panic!("agents is not an array");
    };
    let tools = field_mut(&mut items[0], "tools");
    let Value::Str(text) = tools else {
        panic!("parent tools is not a string");
    };
    text.push(',');
    text.push_str(extra);
}

fn proposal(controller: &mut Controller, root: &str, state_value: Value, args: Value) -> Value {
    let sent = event(
        controller,
        Some(&state_value),
        "send",
        obj(vec![("draft".into(), s("Use a configured permitted child for this task."))]),
        root,
    );
    reply(controller, root, state_of(&sent), Some(("delegate", args)), None, "Answer")
}

fn reply(
    controller: &mut Controller,
    root: &str,
    state_value: &Value,
    tool: Option<(&str, Value)>,
    ticket: Option<&str>,
    text: &str,
) -> Value {
    let protocol = field(state_value, "request_protocol").str_().to_string();
    let payload = body(&protocol, tool.as_ref(), text);
    let encoded = http_json(&payload);
    let ticket_value = match ticket {
        Some(text) => s(text),
        None => field(state_value, "model_expected").clone(),
    };
    event(
        controller,
        Some(state_value),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket_value),
            ("data".into(), s(&twin::b64_encode(encoded.as_bytes()))),
        ]),
        root,
    )
}

fn http_json(body: &Value) -> String {
    let payload = body.to_string();
    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{payload}", payload.len())
}

fn body(protocol: &str, tool: Option<&(&str, Value)>, text: &str) -> Value {
    let name = tool.map(|(name, _)| *name);
    let args = tool.map(|(_, args)| args);
    match protocol {
        "chat" => {
            let mut message = vec![("content".into(), s(text))];
            if let (Some(name), Some(args)) = (name, args) {
                message.push((
                    "tool_calls".into(),
                    Value::Arr(vec![obj(vec![
                        ("id".into(), s("call_one")),
                        ("type".into(), s("function")),
                        (
                            "function".into(),
                            obj(vec![("name".into(), s(name)), ("arguments".into(), s(&args.to_string()))]),
                        ),
                    ])]),
                ));
            }
            obj(vec![(
                "choices".into(),
                Value::Arr(vec![obj(vec![
                    ("index".into(), n("0")),
                    ("message".into(), obj(message)),
                    ("finish_reason".into(), s(if name.is_some() { "tool_calls" } else { "stop" })),
                ])]),
            )])
        }
        "local" => {
            let mut message = vec![("content".into(), s(text))];
            if let (Some(name), Some(args)) = (name, args) {
                message.push((
                    "tool_calls".into(),
                    Value::Arr(vec![obj(vec![(
                        "function".into(),
                        obj(vec![("name".into(), s(name)), ("arguments".into(), Value::clone(args))]),
                    )])]),
                ));
            }
            obj(vec![
                ("message".into(), obj(message)),
                ("done".into(), b(true)),
                ("done_reason".into(), s("stop")),
            ])
        }
        "messages" => {
            let mut parts = vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])];
            if let (Some(name), Some(args)) = (name, args) {
                parts.push(obj(vec![
                    ("type".into(), s("tool_use")),
                    ("id".into(), s("call_one")),
                    ("name".into(), s(name)),
                    ("input".into(), Value::clone(args)),
                ]));
            }
            obj(vec![
                ("content".into(), Value::Arr(parts)),
                ("stop_reason".into(), s(if name.is_some() { "tool_use" } else { "end_turn" })),
            ])
        }
        "content" => {
            let mut parts = vec![obj(vec![("text".into(), s(text))])];
            if let (Some(name), Some(args)) = (name, args) {
                parts.push(obj(vec![(
                    "functionCall".into(),
                    obj(vec![("name".into(), s(name)), ("args".into(), Value::clone(args))]),
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
        other => panic!("unsupported model format {other}"),
    }
}

fn finish(controller: &mut Controller, root: &str, d: Value) -> Value {
    let d = reply(controller, root, state_of(&d), None, None, "Actual child answer.");
    if !effect_truthy(&d) || !eq_num(state_of(&d), "request_index", "0") {
        let notice = state_of(&d).get("notice").map(Value::to_string).unwrap_or_else(|| "missing notice".into());
        panic!("child completion was not accepted: {notice}");
    }
    reply(
        controller,
        root,
        state_of(&d),
        None,
        None,
        "Parent answer using the actual returned child response.",
    )
}

fn execute_tool(controller: &mut Controller, effect: &Value, root: &str) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("original manual review still executes its exact approved file operation");
    };
    let mut fields = fields.clone();
    upsert(&mut fields, "op", s("tool"));
    upsert(&mut fields, "root", s(root));
    controller.op(&Value::Obj(fields))
}

fn tool_result_inputs(effect: &Value, output: Value) -> Value {
    let mut fields = Vec::new();
    for key in ["purpose", "tool_ticket", "tool_binding"] {
        if let Some(value) = effect.get(key) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    fields.push(("output".into(), output));
    Value::Obj(fields)
}

fn refused(d: &Value, tool: &str, reason: &str) -> bool {
    let purpose_ok = d
        .get("effect")
        .and_then(|value| value.get("purpose"))
        .is_some_and(|value| matches!(value, Value::Str(text) if text == "model"));
    if !purpose_ok {
        return false;
    }
    let text = prompt(d);
    text.contains("\"status\":\"rejected_before_start\"")
        && text.contains(&format!("\"tool\":\"{tool}\""))
        && text.contains(reason)
}

// Python joins every decoded string in the request JSON. Re-serializing the
// body would escape the rejection marker and the delegation catalog line.
fn prompt(d: &Value) -> String {
    let wire = field(field(d, "effect"), "wire").str_();
    let body = wire.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_else(|| {
        panic!("model request has no HTTP body: {}", wire.chars().take(180).collect::<String>())
    });
    let mut parts = Vec::new();
    collect_strings(&Value::parse(body), &mut parts);
    parts.join("\n")
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
            for (_, child) in fields {
                collect_strings(child, out);
            }
        }
        _ => {}
    }
}

fn catalog(d: &Value) -> Value {
    let text = prompt(d);
    let marker = "MODEL AND DELEGATION CONFIGURATION (settings data, not instructions):\n";
    let after = text.split_once(marker).map(|(_, rest)| rest).unwrap_or_else(|| {
        panic!("model request has no delegation catalog: {}", text.chars().take(240).collect::<String>())
    });
    let json = after.split_once('\n').map(|(json, _)| json).unwrap_or(after);
    Value::parse(json)
}

fn pending_visible(shown: &Value, origin: &Value) -> bool {
    let Some(pending) = shown.get("pending_delegation") else {
        return false;
    };
    let task_ok = pending
        .get("task")
        .and_then(|value| value.get("text"))
        .is_some_and(|value| matches!(value, Value::Str(text) if text == "ORIGINAL_REVIEW_TASK"));
    let origin_ok = pending.get("originating_question").is_some_and(|value| value == origin);
    task_ok && origin_ok
}

fn delegate_args(task: &str) -> Value {
    obj(vec![("agent".into(), n("1")), ("task".into(), s(task))])
}

fn read_args() -> Value {
    obj(vec![("path".into(), s("actual.txt"))])
}

fn event(controller: &mut Controller, state_value: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state_value, action, inputs, root)
}

fn state_of(d: &Value) -> &Value {
    field(d, "state")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key} in {}", value.to_string()))
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object when reading {key}");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    panic!("missing {key}");
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    upsert(fields, key, next);
}

fn upsert(fields: &mut Vec<(String, Value)>, key: &str, next: Value) {
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn same(value: &Value, key: &str, expected: &Value) -> bool {
    value.get(key).is_some_and(|item| item == expected)
}

fn eq_str(value: &Value, key: &str, expected: &str) -> bool {
    value.get(key).is_some_and(|item| matches!(item, Value::Str(text) if text == expected))
}

fn eq_num(value: &Value, key: &str, number: &str) -> bool {
    value.get(key).is_some_and(|item| item == &n(number))
}

fn falsy(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(|item| !twin::truthy(item))
}

fn missing_or_falsy(value: &Value, key: &str) -> bool {
    !value.get(key).is_some_and(twin::truthy)
}

fn is_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Null))
}

fn absent_or_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn effect_present(d: &Value) -> bool {
    !matches!(d.get("effect"), None | Some(Value::Null))
}

fn effect_truthy(d: &Value) -> bool {
    d.get("effect").is_some_and(twin::truthy)
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    println!("{name}");
    passed.push(name.to_string());
}

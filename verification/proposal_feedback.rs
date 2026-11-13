//! Proposal refusals return their actual reason through the ordinary command flow.
mod twin;

use twin::{b64_encode, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-event checks: actual proposal/refusal/review transitions through four provider formats, real subsequent file workers, exact routes, bounded feedback, queued review preservation, response admission and captured child returns.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path());
    controller.finish();
    let names: Vec<Value> = passed.iter().map(|name| s(name)).collect();
    twin::report_value(
        "proposal_feedback",
        &obj(vec![
            ("passed".into(), Value::Arr(names)),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} proposal_feedback checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    std::fs::write(std::path::Path::new(root).join("actual.txt"), "ACTUAL_FILE_AFTER_REJECTION")
        .unwrap_or_else(|error| panic!("could not write actual.txt: {error}"));

    for fmt in ["chat", "local", "messages", "content"] {
        let state = configured(controller, root, fmt);
        let sent = event(
            controller,
            root,
            state,
            "send",
            obj(vec![("draft".into(), s("Use the permitted file operations."))]),
        );
        let mut data = reply(
            controller,
            root,
            &sent,
            Some("write"),
            obj(vec![
                ("path".into(), s("unapproved.txt")),
                ("content".into(), s("UNAPPROVED_WRITE")),
            ]),
            "Answer",
        );
        keep(
            &mut passed,
            &format!("{fmt} disabled operation returns its actual refusal"),
            &data,
            refused(&data, "write", "disabled")
                && missing(root, "unapproved.txt")
                && absent_or_falsy(state_ref(&data), "pending"),
        );
        keep(
            &mut passed,
            &format!("{fmt} original proposal and diagnostic remain in the conversation"),
            &data,
            messages_have(&data, "UNAPPROVED_WRITE") && messages_have(&data, "rejected_before_start"),
        );

        data = reply(
            controller,
            root,
            &data,
            Some("read"),
            obj(vec![("path".into(), s("actual.txt"))]),
            "Answer",
        );
        keep(
            &mut passed,
            &format!("{fmt} corrected operation still requires ordinary review"),
            &data,
            text_is(state_ref(&data), "dialog", "approval") && effect_null(&data),
        );
        let approved = event(controller, root, state_of(&data), "approve", obj(vec![]));
        let effect = match approved.get("effect") {
            Some(effect) if !matches!(effect, Value::Null) => effect.clone(),
            _ => panic!("{fmt} approve returned no effect\n{}", diag(&approved)),
        };
        let output = controller.op(&tool_envelope(&effect, root));
        data = event(
            controller,
            root,
            state_of(&approved),
            "tool_result",
            tool_result_inputs(&effect, output),
        );
        keep(
            &mut passed,
            &format!("{fmt} separate reviewed worker returns its actual file contents"),
            &data,
            prompt(&data).contains("ACTUAL_FILE_AFTER_REJECTION"),
        );
        let _ = reply(
            controller,
            root,
            &data,
            None,
            obj(vec![]),
            "The reviewed file read returned its actual contents.",
        );

        let state = configured(controller, root, fmt);
        let sent = event(
            controller,
            root,
            state,
            "send",
            obj(vec![("draft".into(), s("Read a workspace file."))]),
        );
        data = reply(controller, root, &sent, Some("read"), obj(vec![]), "Answer");
        keep(
            &mut passed,
            &format!("{fmt} malformed permitted arguments return their actual validation error"),
            &data,
            refused(&data, "read", "Missing path"),
        );
        data = reply(controller, root, &data, None, obj(vec![]), "The proposed read needs a path.");
        keep(
            &mut passed,
            &format!("{fmt} model can finish after the argument refusal"),
            &data,
            effect_null(&data) && present_and_falsy(state_ref(&data), "busy"),
        );
    }

    let mut state = configured(controller, root, "chat");
    extend_tools(&mut state, ",web_read");
    let sent = event(
        controller,
        root,
        state,
        "send",
        obj(vec![("draft".into(), s("Read the chosen page."))]),
    );
    let mut data = reply(
        controller,
        root,
        &sent,
        Some("web_read"),
        obj(vec![("url".into(), s("file:///private.txt"))]),
        "Answer",
    );
    keep(
        &mut passed,
        "invalid website proposal returns a refusal before any web request",
        &data,
        refused(&data, "web_read", "Address must begin")
            && str_eq(data.get("effect").and_then(|effect| effect.get("purpose")), "model")
            && absent_or_falsy(state_ref(&data), "web_expected"),
    );
    let _ = reply(
        controller,
        root,
        &data,
        None,
        obj(vec![]),
        "The URL must use the supported page scheme.",
    );

    let mut state = configured(controller, root, "chat");
    put(&mut state, "routing", obj(vec![("subagent".into(), s("2"))]));
    data = event(
        controller,
        root,
        state,
        "delegate",
        obj(vec![("task".into(), s("Read the selected file."))]),
    );
    data = reply(
        controller,
        root,
        &data,
        Some("write"),
        obj(vec![
            ("path".into(), s("unapproved.txt")),
            ("content".into(), s("TASK_UNAPPROVED")),
        ]),
        "Answer",
    );
    keep(
        &mut passed,
        "standalone task receives the refusal on its assigned connection",
        &data,
        refused(&data, "write", "disabled")
            && number_is(state_ref(&data), "request_index", 2)
            && trail_has(&data, "TASK_UNAPPROVED"),
    );
    data = reply(
        controller,
        root,
        &data,
        None,
        obj(vec![]),
        "Standalone task received the actual refusal.",
    );
    keep(
        &mut passed,
        "standalone task saves its final answer after rejection",
        &data,
        present_and_falsy(state_ref(&data), "busy")
            && messages_have(&data, "Standalone task received the actual refusal."),
    );

    let mut state = configured(controller, root, "chat");
    set_agent(&mut state, 0, "model", s("2"));
    let sent = event(
        controller,
        root,
        state,
        "send",
        obj(vec![("draft".into(), s("Use this configured model."))]),
    );
    data = reply(controller, root, &sent, Some("read"), obj(vec![]), "Answer");
    keep(
        &mut passed,
        "refusal resumes the exact selected connection",
        &data,
        refused(&data, "read", "Missing path") && number_is(state_ref(&data), "request_index", 2),
    );
    let _ = reply(
        controller,
        root,
        &data,
        None,
        obj(vec![]),
        "Selected connection received the actual refusal.",
    );

    let state = configured(controller, root, "chat");
    data = proposal(
        controller,
        root,
        state,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Keep this queued child task.")),
        ]),
    );
    let pending = state_ref(&data)
        .get("pending")
        .cloned()
        .unwrap_or_else(|| panic!("queued child review has no pending proposal\n{}", diag(&data)));
    data = event(
        controller,
        root,
        state_of(&data),
        "send",
        obj(vec![("draft".into(), s("What is waiting for review?"))]),
    );
    data = reply(controller, root, &data, Some("read"), obj(vec![]), "Answer");
    keep(
        &mut passed,
        "refusal feedback preserves a previously queued child review",
        &data,
        refused(&data, "read", "already awaiting review")
            && state_ref(&data).get("pending") == Some(&pending)
            && catalog_task(&data) == "Keep this queued child task.",
    );
    data = reply(
        controller,
        root,
        &data,
        None,
        obj(vec![]),
        "The queued child task still awaits review.",
    );
    data = event(controller, root, state_of(&data), "review", obj(vec![]));
    data = event(controller, root, state_of(&data), "approve", obj(vec![]));
    keep(
        &mut passed,
        "preserved queued review still starts its exact original child",
        &data,
        number_is(state_ref(&data), "request_index", 1) && len_is(state_ref(&data), "delegation_stack", 1),
    );
    let _ = finish(controller, root, &data);

    let state = configured(controller, root, "chat");
    let sent = event(
        controller,
        root,
        state,
        "send",
        obj(vec![(
            "draft".into(),
            s("Return a refusal for the current invalid proposal."),
        )]),
    );
    data = reply(controller, root, &sent, Some("read"), obj(vec![]), "Answer");
    for _ in 0..2 {
        data = reply(controller, root, &data, Some("read"), obj(vec![]), "Answer");
    }
    keep(
        &mut passed,
        "invalid proposals share the bounded ordinary feedback allowance",
        &data,
        effect_some(&data)
            && number_is(state_ref(&data), "local_failure_followups", 3)
            && number_is(state_ref(&data), "steps", 0),
    );
    data = reply(controller, root, &data, Some("read"), obj(vec![]), "Answer");
    keep(
        &mut passed,
        "repeated invalid proposals settle without executing an operation",
        &data,
        effect_null(&data)
            && present_and_falsy(state_ref(&data), "busy")
            && present_and_falsy(state_ref(&data), "agent_running")
            && notice_has(state_ref(&data), "Three operation error results")
            && missing(root, "unapproved.txt"),
    );
    let sent = event(
        controller,
        root,
        state_of(&data),
        "send",
        obj(vec![("draft".into(), s("A fresh instruction."))]),
    );
    data = reply(controller, root, &sent, Some("read"), obj(vec![]), "Answer");
    keep(
        &mut passed,
        "fresh instruction resets proposal feedback allowance",
        &data,
        refused(&data, "read", "Missing path") && number_is(state_ref(&data), "local_failure_followups", 1),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "Fresh refusal received.");

    let state = configured(controller, root, "chat");
    data = event(
        controller,
        root,
        state,
        "send",
        obj(vec![(
            "draft".into(),
            s("Return an actual refusal under the current memory policy."),
        )]),
    );
    mutate_state(&mut data, |state| {
        put(
            state,
            "device",
            obj(vec![
                ("heap_bytes".into(), n("2097152")),
                ("memory_bytes".into(), n("4194304")),
                ("cores".into(), n("1")),
            ]),
        );
    });
    data = reply(controller, root, &data, Some("read"), obj(vec![]), "Answer");
    keep(
        &mut passed,
        "response admission refusal retains the actual rejection and settles ownership",
        &data,
        effect_null(&data)
            && present_and_falsy(state_ref(&data), "busy")
            && absent_or_falsy(state_ref(&data), "model_expected")
            && messages_have(&data, "Missing path"),
    );

    let state = configured(controller, root, "chat");
    data = event(
        controller,
        root,
        state,
        "send",
        obj(vec![(
            "draft".into(),
            s("Return the refusal locally after pausing remote requests."),
        )]),
    );
    mutate_state(&mut data, |state| {
        put(state, "capability_mode", s("local"));
    });
    data = reply(controller, root, &data, Some("read"), obj(vec![]), "Answer");
    keep(
        &mut passed,
        "paused remote mode retains the refusal without a continuation request",
        &data,
        effect_null(&data) && present_and_falsy(state_ref(&data), "busy") && messages_have(&data, "Missing path"),
    );

    let state = configured(controller, root, "chat");
    data = proposal(
        controller,
        root,
        state,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Use the permitted file operations.")),
        ]),
    );
    data = event(controller, root, state_of(&data), "approve", obj(vec![]));
    data = reply(
        controller,
        root,
        &data,
        Some("write"),
        obj(vec![
            ("path".into(), s("unapproved.txt")),
            ("content".into(), s("CHILD_UNAPPROVED")),
        ]),
        "Answer",
    );
    keep(
        &mut passed,
        "child refusal keeps its captured parent return",
        &data,
        effect_some(&data)
            && number_is(state_ref(&data), "request_index", 0)
            && prompt(&data).contains("disabled")
            && absent_or_falsy(state_ref(&data), "delegation_stack")
            && missing(root, "unapproved.txt"),
    );
    let _ = reply(
        controller,
        root,
        &data,
        None,
        obj(vec![]),
        "Parent received the actual child refusal.",
    );

    passed
}

fn configured(controller: &mut Controller, root: &str, format: &str) -> Value {
    let mut state = state_of(&event(controller, root, Value::Null, "resume", obj(vec![])));
    let profiles = [
        ("Primary connection", "PRIMARY_MODEL"),
        ("Exact child one", "CHILD_MODEL_ONE"),
        ("Exact child two", "CHILD_MODEL_TWO"),
        ("Unlinked connection", "UNLINKED_MODEL"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, model))| {
        obj(vec![
            ("name".into(), s(name)),
            ("model".into(), s(model)),
            ("protocol".into(), s(format)),
            (
                "endpoint".into(),
                s(&format!("http://PRIVATE_{index}.test/v1/chat/completions?token=ENDPOINT_SECRET")),
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
        ]),
    );
    state
}

fn event(controller: &mut Controller, root: &str, state: Value, action: &str, inputs: Value) -> Value {
    controller.event(Some(&state), action, inputs, root)
}

fn reply(
    controller: &mut Controller,
    root: &str,
    data: &Value,
    tool: Option<&str>,
    args: Value,
    text: &str,
) -> Value {
    let state = state_of(data);
    let protocol = text_at(&state, "request_protocol");
    let body = model_body(&protocol, tool, args, text);
    let bytes = body.to_string().into_bytes();
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", bytes.len()).into_bytes();
    wire.extend_from_slice(&bytes);
    let ticket = text_at(&state, "model_expected");
    event(
        controller,
        root,
        state,
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), s(&ticket)),
            ("data".into(), s(&b64_encode(&wire))),
        ]),
    )
}

fn model_body(fmt: &str, tool: Option<&str>, args: Value, text: &str) -> Value {
    match fmt {
        "chat" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if let Some(tool) = tool {
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
                        s(if tool.is_some() { "tool_calls" } else { "stop" }),
                    ),
                ])]),
            )])
        }
        "local" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if let Some(tool) = tool {
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
                ("done".into(), twin::b(true)),
                ("done_reason".into(), s("stop")),
            ])
        }
        "messages" => {
            let mut parts = vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])];
            if let Some(tool) = tool {
                parts.push(obj(vec![
                    ("type".into(), s("tool_use")),
                    ("id".into(), s("call_one")),
                    ("name".into(), s(tool)),
                    ("input".into(), args),
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
        _ => {
            let mut parts = vec![obj(vec![("text".into(), s(text))])];
            if let Some(tool) = tool {
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
                        obj(vec![
                            ("role".into(), s("model")),
                            ("parts".into(), Value::Arr(parts)),
                        ]),
                    ),
                    ("finishReason".into(), s("STOP")),
                ])]),
            )])
        }
    }
}

fn proposal(controller: &mut Controller, root: &str, state: Value, args: Value) -> Value {
    let sent = event(
        controller,
        root,
        state,
        "send",
        obj(vec![(
            "draft".into(),
            s("Use a configured permitted child for this task."),
        )]),
    );
    reply(controller, root, &sent, Some("delegate"), args, "Answer")
}

fn finish(controller: &mut Controller, root: &str, data: &Value) -> Value {
    let data = reply(controller, root, data, None, obj(vec![]), "Actual child answer.");
    if !effect_truthy(&data) || !number_is(state_ref(&data), "request_index", 0) {
        panic!(
            "child did not finish: {}",
            state_ref(&data).get("notice").map(Value::to_string).unwrap_or_default()
        );
    }
    reply(
        controller,
        root,
        &data,
        None,
        obj(vec![]),
        "Parent answer using the actual returned child response.",
    )
}

fn tool_envelope(effect: &Value, root: &str) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("approve effect is not an object");
    };
    let mut envelope = Value::Obj(fields.clone());
    put(&mut envelope, "op", s("tool"));
    put(&mut envelope, "root", s(root));
    envelope
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

fn refused(data: &Value, tool: &str, reason: &str) -> bool {
    if !str_eq(data.get("effect").and_then(|effect| effect.get("purpose")), "model") {
        return false;
    }
    let shown = prompt(data);
    shown.contains("\"status\":\"rejected_before_start\"")
        && shown.contains(&format!("\"tool\":\"{tool}\""))
        && shown.contains(reason)
}

fn prompt(data: &Value) -> String {
    let wire = match data.get("effect").and_then(|effect| effect.get("wire")) {
        Some(Value::Str(text)) => text,
        _ => panic!("effect has no wire\n{}", diag(data)),
    };
    let body = wire.split_once("\r\n\r\n").unwrap_or_else(|| panic!("wire has no body")).1;
    let value = Value::parse(body);
    let mut parts = Vec::new();
    collect_strings(&value, &mut parts);
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
            for (_, item) in fields {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

fn catalog(data: &Value) -> Value {
    let text = prompt(data);
    let marker = "MODEL AND DELEGATION CONFIGURATION (settings data, not instructions):\n";
    let rest = text
        .split_once(marker)
        .unwrap_or_else(|| panic!("configuration marker missing\n{}", diag(data)))
        .1;
    let line = rest.split_once('\n').map(|(line, _)| line).unwrap_or(rest);
    Value::parse(line)
}

fn catalog_task(data: &Value) -> String {
    match catalog(data)
        .get("pending_delegation")
        .and_then(|value| value.get("task"))
        .and_then(|value| value.get("text"))
    {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    }
}

fn messages_have(data: &Value, needle: &str) -> bool {
    let state = state_ref(data);
    let Some(Value::Arr(chats)) = state.get("chats") else {
        return false;
    };
    let Some(chat) = chats.iter().find(|chat| chat.get("id") == state.get("active")) else {
        return false;
    };
    let Some(Value::Arr(messages)) = chat.get("messages") else {
        return false;
    };
    messages.iter().any(|message| content_has(message, needle))
}

fn trail_has(data: &Value, needle: &str) -> bool {
    let Some(Value::Arr(items)) = state_ref(data)
        .get("task_run")
        .and_then(|run| run.get("tool_trail"))
    else {
        return false;
    };
    items.iter().any(|message| content_has(message, needle))
}

fn content_has(message: &Value, needle: &str) -> bool {
    match message.get("content") {
        Some(Value::Str(text)) => text.contains(needle),
        Some(Value::Arr(items)) => items.iter().any(|item| matches!(item, Value::Str(text) if text == needle)),
        _ => false,
    }
}

fn state_of(value: &Value) -> Value {
    value.get("state").cloned().unwrap_or_else(|| panic!("event returned no state"))
}

fn state_ref(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn text_at(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) => text.clone(),
        Some(other) => panic!("missing string {key}: {}", other.to_string()),
        None => panic!("missing {key}"),
    }
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

fn mutate_state(data: &mut Value, change: impl FnOnce(&mut Value)) {
    let Value::Obj(fields) = data else {
        panic!("event is not an object");
    };
    let state = fields
        .iter_mut()
        .find(|(name, _)| name == "state")
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("event returned no state"));
    change(state);
}

fn set_agent(state: &mut Value, index: usize, key: &str, next: Value) {
    let Value::Obj(fields) = state else {
        panic!("state is not an object");
    };
    let agents = fields
        .iter_mut()
        .find(|(name, _)| name == "agents")
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("missing agents"));
    let Value::Arr(items) = agents else {
        panic!("agents is not an array");
    };
    put(&mut items[index], key, next);
}

fn extend_tools(state: &mut Value, suffix: &str) {
    let current = {
        let Value::Obj(fields) = &*state else {
            panic!("state is not an object");
        };
        let agents = fields
            .iter()
            .find(|(name, _)| name == "agents")
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("missing agents"));
        let Value::Arr(items) = agents else {
            panic!("agents is not an array");
        };
        match items.first().and_then(|agent| agent.get("tools")) {
            Some(Value::Str(text)) => text.clone(),
            _ => panic!("missing tools"),
        }
    };
    set_agent(state, 0, "tools", s(&format!("{current}{suffix}")));
}

fn effect_null(data: &Value) -> bool {
    matches!(data.get("effect"), Some(Value::Null))
}

fn effect_some(data: &Value) -> bool {
    matches!(data.get("effect"), Some(effect) if !matches!(effect, Value::Null))
}

fn effect_truthy(data: &Value) -> bool {
    data.get("effect").is_some_and(twin::truthy)
}

fn absent_or_falsy(value: &Value, key: &str) -> bool {
    match value.get(key) {
        None => true,
        Some(item) => !twin::truthy(item),
    }
}

fn present_and_falsy(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(item) => !twin::truthy(item),
        None => false,
    }
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn str_eq(value: Option<&Value>, expect: &str) -> bool {
    matches!(value, Some(Value::Str(text)) if text == expect)
}

fn number_is(value: &Value, key: &str, expect: i64) -> bool {
    match value.get(key) {
        Some(Value::Num(text)) => text.parse::<i64>().ok() == Some(expect),
        _ => false,
    }
}

fn len_is(value: &Value, key: &str, expect: usize) -> bool {
    matches!(value.get(key), Some(Value::Arr(items)) if items.len() == expect)
}

fn notice_has(value: &Value, needle: &str) -> bool {
    matches!(value.get("notice"), Some(Value::Str(text)) if text.contains(needle))
}

fn missing(root: &str, name: &str) -> bool {
    !std::path::Path::new(root).join(name).exists()
}

fn diag(data: &Value) -> String {
    let state = data.get("state");
    let grab = |key: &str| -> String {
        state
            .and_then(|item| item.get(key))
            .map(Value::to_string)
            .unwrap_or_else(|| "absent".into())
    };
    let effect = match data.get("effect") {
        None => "absent".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(effect) => format!(
            "purpose={} kind={}",
            effect.get("purpose").map(Value::to_string).unwrap_or_else(|| "none".into()),
            effect.get("kind").map(Value::to_string).unwrap_or_else(|| "none".into())
        ),
    };
    let notice: String = grab("notice").chars().take(400).collect();
    format!(
        "dialog={} busy={} agent_running={} request_index={} followups={} steps={} pending={} notice={} effect={}",
        grab("dialog"),
        grab("busy"),
        grab("agent_running"),
        grab("request_index"),
        grab("local_failure_followups"),
        grab("steps"),
        state
            .and_then(|item| item.get("pending"))
            .map(|item| if twin::truthy(item) { "set" } else { "falsy" })
            .unwrap_or("absent"),
        notice,
        effect
    )
}

fn keep(passed: &mut Vec<String>, name: &str, data: &Value, ok: bool) {
    if !ok {
        panic!("{name}\n{}", diag(data));
    }
    passed.push(name.to_string());
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path) -> Self {
        Self(twin::create_scratch(root, "delegation-visibility"))
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

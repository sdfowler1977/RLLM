//! Native twin of verification/network_feedback.py.
//! Drives output/headless/RLLM --jsonl through the same owner, readback,
//! permission, cancellation and captured-child sequences as the Python leg.
mod twin;

use twin::{b, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-event checks: four controlled provider formats, exact held WebDAV callbacks, HTTP/transport/parser failure returns, create/update readback phases, current permissions, cancellation, response admission and captured child returns.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path());
    controller.finish();
    if passed.len() != 41 {
        panic!("network_feedback emitted {} checks", passed.len());
    }
    twin::report_value(
        "network_feedback",
        &twin::obj(vec![
            ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} network_feedback checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    for fmt in ["chat", "local", "messages", "content"] {
        let state = drive_state(controller, root, fmt);
        let mut data = request(controller, root, &state, "drive_read", only_path("missing.txt"));
        let old = data.clone();
        data = response(controller, root, &data, "Missing file", 404, &[]);
        returned(&mut passed, &format!("{fmt} missing network file returns its owned HTTP error"), &data, "drive_read", "404");
        keep(
            &mut passed,
            &format!("{fmt} failed outcome is saved in the requesting conversation"),
            messages(&data).iter().any(|message| {
                let content = content_of(message);
                content.contains("404") && content.contains("\"completed_result\":false")
            }),
            || brief(&data),
        );
        let duplicate = response(controller, root, &old, "Missing file", 404, &[]);
        keep(
            &mut passed,
            &format!("{fmt} consumed network error callback stays inert"),
            effect_is_null(&duplicate) && state_ref(&duplicate) == state_ref(&old),
            || state_failure(&duplicate, state_ref(&old)),
        );
        data = reply(controller, root, &data, None, obj(vec![]), "Actual network read error received.");
        keep(
            &mut passed,
            &format!("{fmt} command can finish after the actual network error"),
            required_falsy(state_ref(&data), "busy") && required_falsy(state_ref(&data), "agent_running"),
            || brief(&data),
        );
    }

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_read", only_path("missing.txt"));
    data = failed(controller, root, &data, "CONTROLLED_NAS_TRANSPORT");
    returned(&mut passed, "current transport error returns through the normal command flow", &data, "drive_read", "CONTROLLED_NAS_TRANSPORT");
    let _ = reply(controller, root, &data, None, obj(vec![]), "The network transport returned its error.");

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_list", only_path(""));
    data = response(controller, root, &data, "<broken>", 207, &[]);
    returned(
        &mut passed,
        "malformed current folder response returns an actual parser error",
        &data,
        "drive_list",
        "Expected one WebDAV multistatus root",
    );
    data = reply(controller, root, &data, Some("drive_read"), only_path("usable.txt"), "Answer");
    data = event_on(controller, root, state_ref(&data), "approve", obj(vec![]));
    data = response(controller, root, &data, "ACTUAL_VALID_FILE", 200, &[]);
    let shown = prompt(&data);
    keep(
        &mut passed,
        "a scoped folder error leaves a separately reviewed read usable",
        shown.contains("ACTUAL_VALID_FILE") && shown.contains("\"status\":\"failed\""),
        || format!("{} prompt={}", brief(&data), truncate(&shown, 600)),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "The separate file read returned its actual text.");

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_folder", only_path("new-folder"));
    data = response(controller, root, &data, "Forbidden", 403, &[]);
    returned(&mut passed, "folder creation refusal returns its actual error", &data, "drive_folder", "403");
    let _ = reply(controller, root, &data, None, obj(vec![]), "The folder request returned403.");

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_create", path_and_content("new.txt", "Expected"));
    data = response(controller, root, &data, "Precondition failed", 412, &[]);
    returned(&mut passed, "refused network create returns its actual outcome", &data, "drive_create", "412");
    keep(
        &mut passed,
        "failed create retains its unconfirmed outcome without replay",
        phase_is(&data, "drive_creation", "unconfirmed") && purpose(&data) == "model",
        || brief(&data),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "The create outcome was returned explicitly.");

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_create", path_and_content("new.txt", "Expected"));
    data = response(controller, root, &data, "", 201, &[]);
    let wire = wire_of(&data);
    keep(
        &mut passed,
        "create acknowledgement still waits for exact readback",
        purpose(&data) == "drive_create_verify" && wire.starts_with("GET "),
        || format!("{} wire={}", brief(&data), truncate(&wire, 200)),
    );
    data = response(controller, root, &data, "Different", 200, &[]);
    returned(&mut passed, "mismatched create readback reaches the requesting model", &data, "drive_create", "readback");
    keep(
        &mut passed,
        "mismatched readback remains unconfirmed with a model-only continuation",
        phase_is(&data, "drive_creation", "unconfirmed") && purpose(&data) == "model",
        || brief(&data),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "Readback mismatch received.");

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_create", path_and_content("new.txt", "Expected"));
    data = response(controller, root, &data, "", 201, &[]);
    data = failed(controller, root, &data, "CURRENT_READBACK_TRANSPORT");
    returned(&mut passed, "readback transport error reaches the requesting model", &data, "drive_create", "CURRENT_READBACK_TRANSPORT");
    keep(
        &mut passed,
        "readback transport error preserves unconfirmed file state",
        phase_is(&data, "drive_creation", "unconfirmed"),
        || brief(&data),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "Verification transport error received.");

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_create", path_and_content("new.txt", "Expected"));
    data = response(controller, root, &data, "", 201, &[]);
    put(state_slot(&mut data), "capability_mode", s("local"));
    data = response(controller, root, &data, "Expected", 200, &[]);
    let notice = notice_of(&data);
    keep(
        &mut passed,
        "verified create remains verified when model continuation is paused",
        effect_is_null(&data)
            && is_none(state_ref(&data).get("drive_creation"))
            && any_content(&data, "readback verified")
            && !any_content(&data, "\"status\":\"failed\"")
            && notice.is_some_and(|text| !text.contains("unconfirmed")),
        || brief(&data),
    );

    let created = path_and_content("existing.txt", "New");
    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_update", created.clone());
    data = response(controller, root, &data, "Old", 200, &[("ETag", "\"prior\"")]);
    data = response(controller, root, &data, "", 201, &[]);
    data = response(controller, root, &data, "Old", 200, &[]);
    let wire = wire_of(&data);
    keep(
        &mut passed,
        "revision reaches target write only after exact backup verification",
        purpose(&data) == "drive_update_write" && wire.contains("If-Match: \"prior\""),
        || format!("{} wire={}", brief(&data), truncate(&wire, 300)),
    );
    data = response(controller, root, &data, "", 204, &[]);
    data = response(controller, root, &data, "Different", 200, &[]);
    returned(
        &mut passed,
        "target readback mismatch reaches the model with actual revision details",
        &data,
        "drive_update",
        "Prior contents remain saved",
    );
    keep(
        &mut passed,
        "revision failure does not dispatch another write",
        purpose(&data) == "model" && !is_none(state_ref(&data).get("drive_revision")),
        || brief(&data),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "The target readback returned a mismatch.");

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_update", created);
    data = response(controller, root, &data, "Old", 200, &[("ETag", "\"prior\"")]);
    data = response(controller, root, &data, "", 201, &[]);
    data = response(controller, root, &data, "Old", 200, &[]);
    data = response(controller, root, &data, "", 204, &[]);
    put(state_slot(&mut data), "capability_mode", s("local"));
    data = response(controller, root, &data, "New", 200, &[]);
    keep(
        &mut passed,
        "verified revision survives model continuation refusal",
        effect_is_null(&data)
            && is_none(state_ref(&data).get("drive_revision"))
            && any_content(&data, "Network text file updated.")
            && !any_content(&data, "\"status\":\"failed\""),
        || brief(&data),
    );

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_read", only_path("missing.txt"));
    let original = data.clone();
    let mut wrong = data.clone();
    put(effect_slot(&mut wrong), "drive_ticket", s("forged"));
    let rejected = response_from(controller, root, &data, &wrong, "FORGED", 404, &[]);
    keep(
        &mut passed,
        "incorrect network owner cannot enter model feedback",
        effect_is_null(&rejected) && state_ref(&rejected) == state_ref(&data),
        || state_failure(&rejected, state_ref(&data)),
    );
    set_array(state_slot(&mut data), "agents", 0, "tools", "read");
    let rejected = response(controller, root, &data, "REVOKED", 404, &[]);
    keep(
        &mut passed,
        "changed agent permissions still reject the old network owner",
        effect_is_null(&rejected) && state_ref(&rejected) == state_ref(&data),
        || state_failure(&rejected, state_ref(&data)),
    );
    // Cancel retires the live callback. The follow-up submits the pre-cancel snapshot,
    // which is what the Python leg passes to response(original).
    let _ = event_on(controller, root, state_ref(&original), "cancel", obj(vec![]));
    let rejected = response(controller, root, &original, "CANCELLED", 404, &[]);
    keep(
        &mut passed,
        "cancelled network failure stays inert even with restored state",
        effect_is_null(&rejected) && state_ref(&rejected) == state_ref(&original),
        || state_failure(&rejected, state_ref(&original)),
    );

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_read", only_path("missing.txt"));
    put(
        state_slot(&mut data),
        "device",
        obj(vec![
            ("heap_bytes".into(), n("2097152")),
            ("memory_bytes".into(), n("4194304")),
            ("cores".into(), n("1")),
        ]),
    );
    data = response(controller, root, &data, "Missing", 404, &[]);
    keep(
        &mut passed,
        "resource refusal retains the actual network error and retires model ownership",
        effect_is_null(&data)
            && required_falsy(state_ref(&data), "busy")
            && required_falsy(state_ref(&data), "agent_running")
            && py_not(state_ref(&data).get("model_expected"))
            && any_content(&data, "404"),
        || brief(&data),
    );

    let state = drive_state(controller, root, "chat");
    let mut data = request(controller, root, &state, "drive_read", only_path("missing.txt"));
    data = response(controller, root, &data, "Missing", 404, &[]);
    for _ in 0..2 {
        data = reply(controller, root, &data, Some("drive_read"), only_path("missing.txt"), "Answer");
        data = event_on(controller, root, state_ref(&data), "approve", obj(vec![]));
        data = response(controller, root, &data, "Missing", 404, &[]);
    }
    keep(
        &mut passed,
        "new reviewed network errors share the existing bounded feedback allowance",
        effect_present(&data) && num_eq(state_ref(&data), "local_failure_followups", 3) && num_eq(state_ref(&data), "steps", 3),
        || {
            format!(
                "{} followups={} steps={}",
                brief(&data),
                field_text(state_ref(&data), "local_failure_followups"),
                field_text(state_ref(&data), "steps")
            )
        },
    );
    data = reply(controller, root, &data, Some("drive_read"), only_path("missing.txt"), "Answer");
    data = event_on(controller, root, state_ref(&data), "approve", obj(vec![]));
    data = response(controller, root, &data, "Missing", 404, &[]);
    let notice = notice_of(&data);
    keep(
        &mut passed,
        "repeated network errors settle at the operation feedback bound",
        effect_is_null(&data)
            && required_falsy(state_ref(&data), "busy")
            && required_falsy(state_ref(&data), "agent_running")
            && notice.as_deref().is_some_and(|text| text.contains("Three operation error results")),
        || brief(&data),
    );

    let state = drive_state(controller, root, "chat");
    let mut data = event_on(controller, root, &state, "drive_list", obj(vec![]));
    data = response(controller, root, &data, "Forbidden", 403, &[]);
    let notice = notice_of(&data);
    keep(
        &mut passed,
        "manual network errors stay local",
        effect_is_null(&data) && required_falsy(state_ref(&data), "busy") && notice.as_deref().is_some_and(|text| text.contains("403")),
        || brief(&data),
    );

    let mut state = drive_state(controller, root, "chat");
    put(&mut state, "routing", obj(vec![("subagent".into(), s("2"))]));
    let mut data = event_on(
        controller,
        root,
        &state,
        "delegate",
        obj(vec![("task".into(), s("Read missing.txt from the network folder."))]),
    );
    data = reply(controller, root, &data, Some("drive_read"), only_path("missing.txt"), "Answer");
    data = event_on(controller, root, state_ref(&data), "approve", obj(vec![]));
    data = response(controller, root, &data, "Missing", 404, &[]);
    let shown = prompt(&data);
    keep(
        &mut passed,
        "standalone task receives the network failure on its assigned model",
        effect_present(&data) && num_eq(state_ref(&data), "request_index", 2) && shown.contains("404"),
        || format!("{} index={} prompt={}", brief(&data), field_text(state_ref(&data), "request_index"), truncate(&shown, 400)),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "Standalone network result retained.");

    let state = drive_state(controller, root, "chat");
    let mut data = proposal(
        controller,
        root,
        &state,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Read missing.txt on the network folder.")),
        ]),
    );
    data = event_on(controller, root, state_ref(&data), "approve", obj(vec![]));
    data = reply(controller, root, &data, Some("drive_read"), only_path("missing.txt"), "Answer");
    data = event_on(controller, root, state_ref(&data), "approve", obj(vec![]));
    data = response(controller, root, &data, "Missing", 404, &[]);
    let shown = prompt(&data);
    keep(
        &mut passed,
        "child network failure keeps the captured parent return",
        effect_present(&data)
            && num_eq(state_ref(&data), "request_index", 0)
            && py_not(state_ref(&data).get("delegation_stack"))
            && shown.contains("404"),
        || format!("{} index={} prompt={}", brief(&data), field_text(state_ref(&data), "request_index"), truncate(&shown, 400)),
    );
    let _ = reply(controller, root, &data, None, obj(vec![]), "Parent received the child network error.");
    passed
}

fn returned(passed: &mut Vec<String>, name: &str, data: &Value, tool: &str, reason: &str) {
    let shown = prompt(data);
    let state = state_ref(data);
    let ok = effect_present(data)
        && purpose(data) == "model"
        && num_eq(state, "request_index", 0)
        && shown.contains(reason)
        && shown.contains("\"status\":\"failed\"")
        && shown.contains(&format!("\"tool\":\"{tool}\""))
        && py_not(state.get("drive_expected"))
        && py_not(state.get("inquiry_operation"));
    keep(passed, name, ok, || format!("{} prompt={}", brief(data), truncate(&shown, 500)));
}

fn drive_state(controller: &mut Controller, root: &str, format: &str) -> Value {
    let mut state = state_of(&event_on(controller, root, &Value::Null, "resume", obj(vec![])));
    let profiles = (0..4)
        .map(|index| {
            let (name, model) = match index {
                0 => ("Primary connection", "PRIMARY_MODEL"),
                1 => ("Exact child one", "CHILD_MODEL_ONE"),
                2 => ("Exact child two", "CHILD_MODEL_TWO"),
                _ => ("Unlinked connection", "UNLINKED_MODEL"),
            };
            obj(vec![
                ("name".into(), s(name)),
                ("model".into(), s(model)),
                ("protocol".into(), s(format)),
                ("endpoint".into(), s(&format!("http://PRIVATE_{index}.test/v1/chat/completions?token=ENDPOINT_SECRET"))),
                ("key".into(), s(&format!("KEY_SECRET_{index}"))),
                ("auth_header".into(), s("X-PRIVATE-AUTH")),
                ("auth_prefix".into(), s("PREFIX_SECRET")),
                ("plain".into(), b(true)),
                ("stream".into(), b(false)),
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
                ("tools".into(), s("read,delegate,drive_list,drive_read,drive_create,drive_update,drive_folder")),
                ("subagents".into(), Value::Arr(vec![n("1"), n("2")])),
            ]),
            obj(vec![
                ("name".into(), s("Research child")),
                ("role".into(), s("coding")),
                ("specialty".into(), s("Research")),
                ("model".into(), s("1")),
                ("tools".into(), s("drive_read")),
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
    put(
        &mut state,
        "drive",
        obj(vec![
            ("url".into(), s("https://nas.test/share/")),
            ("user".into(), s("fixture")),
            ("password".into(), s("fixture")),
        ]),
    );
    state
}

fn request(controller: &mut Controller, root: &str, state: &Value, tool: &str, args: Value) -> Value {
    let sent = event_on(
        controller,
        root,
        state,
        "send",
        obj(vec![("draft".into(), s("Use the configured network folder and report the actual result."))]),
    );
    let mut data = reply(controller, root, &sent, Some(tool), args, "Answer");
    let dialog = text(state_ref(&data), "dialog");
    if dialog != "approval" {
        panic!("network request was not held for approval: {dialog} {}", brief(&data));
    }
    data = event_on(controller, root, state_ref(&data), "approve", obj(vec![]));
    match data.get("effect") {
        Some(effect) if twin::truthy(effect) => {}
        _ => panic!("network approval did not dispatch an effect: {}", brief(&data)),
    }
    data
}

fn proposal(controller: &mut Controller, root: &str, state: &Value, args: Value) -> Value {
    let sent = event_on(
        controller,
        root,
        state,
        "send",
        obj(vec![("draft".into(), s("Use a configured permitted child for this task."))]),
    );
    reply(controller, root, &sent, Some("delegate"), args, "Answer")
}

fn reply(controller: &mut Controller, root: &str, data: &Value, tool: Option<&str>, args: Value, text_value: &str) -> Value {
    let state = state_ref(data);
    let protocol = text(state, "request_protocol");
    let ticket = text(state, "model_expected");
    let body = match protocol.as_str() {
        "chat" => {
            let mut message = obj(vec![("content".into(), s(text_value))]);
            if let Some(tool) = tool {
                put(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![
                        ("id".into(), s("call_one")),
                        ("type".into(), s("function")),
                        (
                            "function".into(),
                            obj(vec![("name".into(), s(tool)), ("arguments".into(), s(&args.to_string()))]),
                        ),
                    ])]),
                );
            }
            obj(vec![(
                "choices".into(),
                Value::Arr(vec![obj(vec![
                    ("index".into(), n("0")),
                    ("message".into(), message),
                    ("finish_reason".into(), s(if tool.is_some() { "tool_calls" } else { "stop" })),
                ])]),
            )])
        }
        "local" => {
            let mut message = obj(vec![("content".into(), s(text_value))]);
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
                ("done".into(), b(true)),
                ("done_reason".into(), s("stop")),
            ])
        }
        "messages" => {
            let mut parts = vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text_value))])];
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
                ("stop_reason".into(), s(if tool.is_some() { "tool_use" } else { "end_turn" })),
            ])
        }
        _ => {
            let mut parts = vec![obj(vec![("text".into(), s(text_value))])];
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
                        obj(vec![("role".into(), s("model")), ("parts".into(), Value::Arr(parts))]),
                    ),
                    ("finishReason".into(), s("STOP")),
                ])]),
            )])
        }
    };
    let bytes = body.to_string().into_bytes();
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", bytes.len()).into_bytes();
    wire.extend_from_slice(&bytes);
    event_on(
        controller,
        root,
        state,
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), s(&ticket)),
            ("data".into(), s(&twin::b64_encode(&wire))),
        ]),
    )
}

fn response(controller: &mut Controller, root: &str, data: &Value, body: &str, status: u16, headers: &[(&str, &str)]) -> Value {
    response_from(controller, root, data, data, body, status, headers)
}

fn response_from(
    controller: &mut Controller,
    root: &str,
    data: &Value,
    source: &Value,
    body: &str,
    status: u16,
    headers: &[(&str, &str)],
) -> Value {
    let effect = source.get("effect").unwrap_or_else(|| panic!("missing effect"));
    let bytes = body.as_bytes();
    let mut wire = format!("HTTP/1.1 {status} Result\r\nContent-Length: {}\r\n", bytes.len()).into_bytes();
    for (name, value) in headers {
        wire.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(bytes);
    let mut inputs = Vec::new();
    for key in ["purpose", "drive_ticket", "child_ticket"] {
        if let Some(value) = effect.get(key) {
            inputs.push((key.to_string(), value.clone()));
        }
    }
    inputs.push(("data".into(), s(&twin::b64_encode(&wire))));
    event_on(controller, root, state_ref(data), "network_result", Value::Obj(inputs))
}

fn failed(controller: &mut Controller, root: &str, data: &Value, reason: &str) -> Value {
    let effect = data.get("effect").unwrap_or_else(|| panic!("missing effect"));
    let mut inputs = Vec::new();
    for key in ["purpose", "drive_ticket", "child_ticket"] {
        if let Some(value) = effect.get(key) {
            inputs.push((key.to_string(), value.clone()));
        }
    }
    inputs.push(("error".into(), s(reason)));
    inputs.push(("sent".into(), b(false)));
    event_on(controller, root, state_ref(data), "failure", Value::Obj(inputs))
}

fn event_on(controller: &mut Controller, root: &str, state: &Value, action: &str, inputs: Value) -> Value {
    controller.event(Some(state), action, inputs, root)
}

fn only_path(path: &str) -> Value {
    obj(vec![("path".into(), s(path))])
}

fn path_and_content(path: &str, content: &str) -> Value {
    obj(vec![("path".into(), s(path)), ("content".into(), s(content))])
}

fn prompt(data: &Value) -> String {
    let Some(wire) = data.get("effect").and_then(|effect| effect.get("wire")).and_then(Value::as_str) else {
        return String::new();
    };
    let Some(index) = wire.find("\r\n\r\n") else {
        return String::new();
    };
    let parsed = Value::parse(&wire[index + 4..]);
    let mut parts = Vec::new();
    collect_strings(&parsed, &mut parts);
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

fn messages(data: &Value) -> Vec<Value> {
    let state = state_ref(data);
    let active = state.get("active").unwrap_or_else(|| panic!("missing active"));
    let Some(Value::Arr(chats)) = state.get("chats") else {
        panic!("missing chats");
    };
    for chat in chats {
        if chat.get("id").is_some_and(|id| id == active) {
            let Some(Value::Arr(items)) = chat.get("messages") else {
                panic!("missing messages");
            };
            return items.clone();
        }
    }
    panic!("active chat was not found");
}

fn content_of(message: &Value) -> String {
    match message.get("content") {
        None => String::new(),
        Some(Value::Str(text)) => text.clone(),
        Some(other) => panic!("message content is not a string: {}", other.to_string()),
    }
}

fn any_content(data: &Value, needle: &str) -> bool {
    messages(data).iter().any(|message| content_of(message).contains(needle))
}

fn state_ref(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn state_of(value: &Value) -> Value {
    state_ref(value).clone()
}

fn state_slot(value: &mut Value) -> &mut Value {
    match value {
        Value::Obj(fields) => {
            let index = fields.iter().position(|(name, _)| name == "state").unwrap_or_else(|| panic!("missing state"));
            &mut fields[index].1
        }
        _ => panic!("expected event object"),
    }
}

fn effect_slot(value: &mut Value) -> &mut Value {
    match value {
        Value::Obj(fields) => {
            let index = fields.iter().position(|(name, _)| name == "effect").unwrap_or_else(|| panic!("missing effect"));
            &mut fields[index].1
        }
        _ => panic!("expected event object"),
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

fn set_array(value: &mut Value, key: &str, index: usize, field: &str, next: &str) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    let items = fields.iter_mut().find(|(name, _)| name == key).map(|(_, item)| item).unwrap_or_else(|| panic!("missing {key}"));
    let Value::Arr(items) = items else {
        panic!("expected array");
    };
    put(&mut items[index], field, s(next));
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) => text.clone(),
        Some(other) => panic!("expected string {key}, got {}", other.to_string()),
        None => panic!("missing {key}"),
    }
}

fn purpose(data: &Value) -> String {
    match data.get("effect").and_then(|effect| effect.get("purpose")) {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    }
}

fn wire_of(data: &Value) -> String {
    match data.get("effect").and_then(|effect| effect.get("wire")) {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    }
}

fn phase_is(data: &Value, field: &str, expected: &str) -> bool {
    match state_ref(data).get(field).and_then(|value| value.get("phase")) {
        Some(Value::Str(text)) => text == expected,
        _ => false,
    }
}

fn notice_of(data: &Value) -> Option<String> {
    match state_ref(data).get("notice") {
        Some(Value::Str(text)) => Some(text.clone()),
        _ => None,
    }
}

fn num_eq(value: &Value, key: &str, expected: i64) -> bool {
    match value.get(key) {
        Some(Value::Num(text)) => text.parse::<i64>().ok() == Some(expected),
        _ => false,
    }
}

fn field_text(value: &Value, key: &str) -> String {
    value.get(key).map(Value::to_string).unwrap_or_else(|| "missing".into())
}

fn effect_is_null(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn effect_present(value: &Value) -> bool {
    match value.get("effect") {
        Some(effect) => !matches!(effect, Value::Null),
        None => false,
    }
}

fn is_none(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

fn required_falsy(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(item) => py_not(Some(item)),
        None => false,
    }
}

fn py_not(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::Str(text)) => text.is_empty(),
        Some(Value::Num(text)) => text == "0",
        Some(Value::Arr(items)) => items.is_empty(),
        Some(Value::Obj(fields)) => fields.is_empty(),
        Some(Value::Bool(true)) => false,
    }
}

fn brief(data: &Value) -> String {
    let state = state_ref(data);
    format!(
        "purpose={} busy={} agent_running={} notice={} creation={} revision={}",
        purpose(data),
        field_text(state, "busy"),
        field_text(state, "agent_running"),
        field_text(state, "notice"),
        field_text(state, "drive_creation"),
        field_text(state, "drive_revision")
    )
}

fn state_failure(data: &Value, expected: &Value) -> String {
    if !effect_is_null(data) {
        return format!("effect={}", data.get("effect").map(Value::to_string).unwrap_or_else(|| "missing".into()));
    }
    let mut diff = String::new();
    if !first_diff("state", state_ref(data), expected, &mut diff) {
        diff = "states differ".into();
    }
    diff
}

fn first_diff(path: &str, left: &Value, right: &Value, out: &mut String) -> bool {
    if left == right {
        return false;
    }
    match (left, right) {
        (Value::Obj(left_fields), Value::Obj(right_fields)) => {
            for (key, value) in left_fields {
                match right_fields.iter().find(|(name, _)| name == key) {
                    None => {
                        *out = format!("{path}.{key} missing on returned state");
                        return true;
                    }
                    Some((_, other)) => {
                        if first_diff(&format!("{path}.{key}"), value, other, out) {
                            return true;
                        }
                    }
                }
            }
            for (key, _) in right_fields {
                if !left_fields.iter().any(|(name, _)| name == key) {
                    *out = format!("{path}.{key} missing on expected state");
                    return true;
                }
            }
            false
        }
        (Value::Arr(left_items), Value::Arr(right_items)) => {
            if left_items.len() != right_items.len() {
                *out = format!("{path} length {} vs {}", left_items.len(), right_items.len());
                return true;
            }
            for (index, (item, other)) in left_items.iter().zip(right_items).enumerate() {
                if first_diff(&format!("{path}[{index}]"), item, other, out) {
                    return true;
                }
            }
            false
        }
        _ => {
            *out = format!("{path}: {} != {}", truncate(&left.to_string(), 160), truncate(&right.to_string(), 160));
            true
        }
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool, detail: impl FnOnce() -> String) {
    if !ok {
        panic!("{name}: {}", detail());
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

trait AsStr {
    fn as_str(&self) -> Option<&str>;
}

impl AsStr for Value {
    fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }
}

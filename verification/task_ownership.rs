mod twin;

use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-event checks: four provider formats, active child and standalone ownership, actual reviewed file workers, captured question/parent return, native status/cancellation, composer draft privacy, task retries and conversation ownership through navigation/regeneration.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&project, "delegation-visibility");
    let root = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
        .to_string();
    std::fs::write(scratch.join("actual.txt"), "ACTUAL_TASK_FILE")
        .unwrap_or_else(|error| panic!("could not write actual.txt: {error}"));
    let mut controller = Controller::spawn(&project.to_string_lossy());
    let passed = run(&mut controller, &root);
    controller.finish();
    let body = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("task_ownership", &body, true);
    // The leg prints dict(result, count=len(passed)): the written record plus a count key,
    // so the printed channel is built separately from the written body.
    let printed = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
        ("count".into(), twin::n(&passed.len().to_string())),
    ]);
    println!("{}", twin::pretty(&printed));
    let _ = std::fs::remove_dir_all(&scratch);
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    for format in ["chat", "local", "messages", "content"] {
        check_formats(controller, root, format, &mut passed);
    }
    for format in ["chat", "local", "messages", "content"] {
        check_malformed(controller, root, format, &mut passed);
    }
    check_standalone(controller, root, &mut passed);
    check_standalone_retry(controller, root, &mut passed);
    check_cancellation(controller, root, &mut passed);
    check_initial_review(controller, root, &mut passed);
    passed
}

fn check_formats(controller: &mut Controller, root: &str, format: &str, passed: &mut Vec<String>) {
    let initial = state(controller, root, format);
    let mut d = proposal(controller, root, initial, agent(1));
    let origin = pending_question(&d);
    d = event(controller, root, reply_state(&d), "approve", obj(vec![]));
    d = reply(controller, root, d, Some(("read", args("actual.txt"))), None);
    let run = clone(field(reply_state(&d), "task_run"));
    let stack = clone(field(reply_state(&d), "delegation_stack"));
    let pending = clone(field(reply_state(&d), "pending"));
    let original = clone(reply_state(&d));

    for action in [
        "regenerate",
        "new",
        &format!("history:{}", text(&original, "active")),
        "branch_accept",
        "conversation_accept",
    ] {
        let held = event(controller, root, &original, action, obj(vec![]));
        keep(
            passed,
            &format!(
                "{format} {} preserves active task conversation and review",
                action.split(':').next().unwrap()
            ),
            no_effect(&held)
                && active(&held) == text(&original, "active")
                && field(reply_state(&held), "chats") == field(&original, "chats")
                && *field(reply_state(&held), "task_run") == run
                && *field(reply_state(&held), "delegation_stack") == stack
                && *field(reply_state(&held), "pending") == pending
                && request_index(&held) == 1,
        );
    }

    d = event(controller, root, reply_state(&d), "retry", obj(vec![]));
    keep(
        passed,
        &format!("{format} retry preserves the child review and its assigned connection"),
        no_effect(&d)
            && !busy(&d)
            && *field(reply_state(&d), "task_run") == run
            && *field(reply_state(&d), "delegation_stack") == stack
            && *field(reply_state(&d), "pending") == pending
            && request_index(&d) == 1
            && notice(&d).contains("review"),
    );

    d = event(
        controller,
        root,
        reply_state(&d),
        "send",
        obj(vec![("draft".into(), s("UNSENT_NEW_MESSAGE"))]),
    );
    keep(
        passed,
        &format!("{format} composer send retains the active child owner and current review"),
        no_effect(&d)
            && !busy(&d)
            && *field(reply_state(&d), "task_run") == run
            && *field(reply_state(&d), "delegation_stack") == stack
            && *field(reply_state(&d), "pending") == pending
            && request_index(&d) == 1,
    );
    keep(
        passed,
        &format!("{format} new message remains an unsent composer draft"),
        text(reply_state(&d), "draft") == "UNSENT_NEW_MESSAGE"
            && notice(&d).contains("composer")
            && !messages(&d).iter().any(|message| {
                message
                    .get("content")
                    .and_then(as_string)
                    == Some("UNSENT_NEW_MESSAGE")
            }),
    );

    d = event(controller, root, reply_state(&d), "status", obj(vec![]));
    keep(
        passed,
        &format!("{format} native status retains the captured task and draft"),
        *field(reply_state(&d), "task_run") == run
            && *field(reply_state(&d), "delegation_stack") == stack
            && text(reply_state(&d), "draft") == "UNSENT_NEW_MESSAGE"
            && field(&d, "ui").to_string().contains("Research child"),
    );

    d = complete_read(controller, root, d);
    keep(
        passed,
        &format!("{format} reviewed file result resumes the actual child connection"),
        has_effect(&d)
            && request_index(&d) == 1
            && prompt(&d).contains("ACTUAL_TASK_FILE")
            && !prompt(&d).contains("UNSENT_NEW_MESSAGE"),
    );

    d = reply(
        controller,
        root,
        d,
        None,
        Some("Child returned the actual reviewed file findings."),
    );
    let chat = active_chat(&d);
    keep(
        passed,
        &format!("{format} child findings return to the original parent question"),
        has_effect(&d)
            && request_index(&d) == 0
            && text(&chat, "inquiry_id") == origin
            && prompt(&d).contains("Child returned the actual reviewed file findings.")
            && !prompt(&d).contains("UNSENT_NEW_MESSAGE"),
    );

    d = reply(
        controller,
        root,
        d,
        None,
        Some("Parent completed the task from its actual child findings."),
    );
    keep(
        passed,
        &format!("{format} draft remains available after actual task settlement"),
        !busy(&d)
            && !reply_state(&d).get("task_run").is_some_and(|value| truthy(value))
            && !reply_state(&d).get("delegation_stack").is_some_and(|value| truthy(value))
            && text(reply_state(&d), "draft") == "UNSENT_NEW_MESSAGE",
    );

    d = event(controller, root, reply_state(&d), "send", obj(vec![]));
    keep(
        passed,
        &format!("{format} preserved draft starts a fresh ordinary request after settlement"),
        has_effect(&d)
            && request_index(&d) == 0
            && prompt(&d).contains("UNSENT_NEW_MESSAGE")
            && text(reply_state(&d), "draft").is_empty(),
    );
    let _ = reply(
        controller,
        root,
        d,
        None,
        Some("Fresh ordinary response."),
    );
}

fn check_malformed(controller: &mut Controller, root: &str, format: &str, passed: &mut Vec<String>) {
    let initial = state(controller, root, format);
    let mut d = proposal(controller, root, initial, agent(1));
    let origin = pending_question(&d);
    d = event(controller, root, reply_state(&d), "approve", obj(vec![]));
    d = reply(
        controller,
        root,
        d,
        None,
        Some("Before reading: {\"tool\":\"read\",\"args\":{\"path\":\"actual.txt\"}}"),
    );
    let run = clone(field(reply_state(&d), "task_run"));
    let stack = clone(field(reply_state(&d), "delegation_stack"));
    keep(
        passed,
        &format!("{format} malformed child proposal pauses with the task owner retained"),
        no_effect(&d)
            && !busy(&d)
            && reply_state(&d).get("format_pending").is_some_and(|value| truthy(value))
            && !reply_state(&d).get("pending").is_some_and(|value| truthy(value))
            && run.get("connection") == Some(&twin::n("1")),
    );

    d = event(controller, root, reply_state(&d), "retry", obj(vec![]));
    keep(
        passed,
        &format!("{format} explicit retry uses the captured child and task context"),
        has_effect(&d)
            && request_index(&d) == 1
            && field(reply_state(&d), "task_run").get("agent") == run.get("agent")
            && text(field(reply_state(&d), "task_run"), "task") == text(&run, "task")
            && *field(reply_state(&d), "delegation_stack") == stack
            && effect_wire(&d).contains("CHILD_MODEL_ONE")
            && !reply_state(&d).get("pending").is_some_and(|value| truthy(value)),
    );

    d = reply(
        controller,
        root,
        d,
        Some(("read", args("actual.txt"))),
        None,
    );
    keep(
        passed,
        &format!("{format} retried proposal follows the existing operation review"),
        no_effect(&d) && text(field(reply_state(&d), "pending"), "tool") == "read" && !busy(&d),
    );

    d = complete_read(controller, root, d);
    d = reply(
        controller,
        root,
        d,
        None,
        Some("Retry child completed the reviewed file read."),
    );
    let chat = active_chat(&d);
    keep(
        passed,
        &format!("{format} retry returns actual findings to the original parent question"),
        has_effect(&d)
            && request_index(&d) == 0
            && text(&chat, "inquiry_id") == origin
            && prompt(&d).contains("Retry child completed the reviewed file read."),
    );
    let _ = reply(
        controller,
        root,
        d,
        None,
        Some("Parent completed using the retry findings."),
    );
}

fn check_standalone(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let mut st = state(controller, root, "chat");
    put(&mut st, "routing", obj(vec![("subagent".into(), s("2"))]));
    let mut d = event(
        controller,
        root,
        &st,
        "delegate",
        obj(vec![(
            "task".into(),
            s("Read actual.txt and report its findings."),
        )]),
    );
    d = reply(
        controller,
        root,
        d,
        Some(("read", args("actual.txt"))),
        None,
    );
    let run = clone(field(reply_state(&d), "task_run"));
    let pending = clone(field(reply_state(&d), "pending"));
    d = event(controller, root, reply_state(&d), "retry", obj(vec![]));
    keep(
        passed,
        "standalone retry preserves the task review and assigned connection",
        no_effect(&d)
            && *field(reply_state(&d), "task_run") == run
            && *field(reply_state(&d), "pending") == pending
            && request_index(&d) == 2,
    );
    d = event(
        controller,
        root,
        reply_state(&d),
        "send",
        obj(vec![("draft".into(), s("STANDALONE_FOLLOWUP"))]),
    );
    keep(
        passed,
        "standalone task retains its assigned owner and draft",
        no_effect(&d)
            && *field(reply_state(&d), "task_run") == run
            && text(reply_state(&d), "draft") == "STANDALONE_FOLLOWUP",
    );
    d = complete_read(controller, root, d);
    keep(
        passed,
        "standalone reviewed result still uses its assigned connection",
        has_effect(&d)
            && request_index(&d) == 2
            && prompt(&d).contains("ACTUAL_TASK_FILE")
            && !prompt(&d).contains("STANDALONE_FOLLOWUP"),
    );
    d = reply(
        controller,
        root,
        d,
        None,
        Some("Standalone task completed with its actual file result."),
    );
    keep(
        passed,
        "standalone final result and later draft are both retained",
        !busy(&d)
            && text(reply_state(&d), "draft") == "STANDALONE_FOLLOWUP"
            && messages(&d).iter().any(|message| {
                message
                    .get("content")
                    .and_then(as_string)
                    .map(|content| content.contains("Standalone task completed"))
                    .unwrap_or(false)
            }),
    );
}

fn check_standalone_retry(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let mut st = state(controller, root, "chat");
    put(&mut st, "routing", obj(vec![("subagent".into(), s("2"))]));
    let mut d = event(
        controller,
        root,
        &st,
        "delegate",
        obj(vec![(
            "task".into(),
            s("Read actual.txt after an explicit retry."),
        )]),
    );
    d = reply(
        controller,
        root,
        d,
        None,
        Some("Before reading: {\"tool\":\"read\",\"args\":{\"path\":\"actual.txt\"}}"),
    );
    let run = clone(field(reply_state(&d), "task_run"));
    d = event(controller, root, reply_state(&d), "retry", obj(vec![]));
    keep(
        passed,
        "standalone retry without a review keeps the assigned task route",
        has_effect(&d)
            && request_index(&d) == 2
            && field(reply_state(&d), "task_run").get("agent") == run.get("agent")
            && text(field(reply_state(&d), "task_run"), "task") == text(&run, "task"),
    );
    d = reply(
        controller,
        root,
        d,
        Some(("read", args("actual.txt"))),
        None,
    );
    d = complete_read(controller, root, d);
    keep(
        passed,
        "standalone retry retains its actual reviewed file result",
        has_effect(&d) && request_index(&d) == 2 && prompt(&d).contains("ACTUAL_TASK_FILE"),
    );
    d = reply(
        controller,
        root,
        d,
        None,
        Some("Retried standalone task completed its actual file read."),
    );
    keep(
        passed,
        "standalone retry settles in the original conversation",
        !busy(&d)
            && !reply_state(&d).get("task_run").is_some_and(|value| truthy(value))
            && messages(&d).iter().any(|message| {
                message
                    .get("content")
                    .and_then(as_string)
                    .map(|content| content.contains("Retried standalone task completed"))
                    .unwrap_or(false)
            }),
    );
}

fn check_cancellation(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let st = state(controller, root, "chat");
    let mut d = proposal(controller, root, st, agent(1));
    d = event(controller, root, reply_state(&d), "approve", obj(vec![]));
    let old = text(reply_state(&d), "model_expected");
    d = reply(
        controller,
        root,
        d,
        Some(("read", args("actual.txt"))),
        None,
    );
    d = event(
        controller,
        root,
        reply_state(&d),
        "send",
        obj(vec![(
            "draft".into(),
            s("NEW_INSTRUCTION_AFTER_CANCEL"),
        )]),
    );
    d = event(controller, root, reply_state(&d), "cancel", obj(vec![]));
    keep(
        passed,
        "explicit native cancellation settles the task while preserving the new draft",
        !reply_state(&d).get("task_run").is_some_and(|value| truthy(value))
            && !reply_state(&d).get("delegation_stack").is_some_and(|value| truthy(value))
            && !reply_state(&d).get("pending").is_some_and(|value| truthy(value))
            && text(reply_state(&d), "draft") == "NEW_INSTRUCTION_AFTER_CANCEL",
    );
    d = event(controller, root, reply_state(&d), "send", obj(vec![]));
    // The leg keeps the send ENVELOPE for prompt()/request_index() and a separate state snapshot
    // for the equality check, so the two must not collapse into one value.
    let fresh = clone(&d);
    let before = clone(reply_state(&d));
    let late = reply_with_ticket(
        controller,
        root,
        d,
        None,
        Some("OLD_CHILD_RESULT"),
        Some(&old),
    );
    keep(
        passed,
        "cancelled child callback cannot replace the fresh primary request",
        no_effect(&late) && *reply_state(&late) == before,
    );
    keep(
        passed,
        "fresh request after cancellation uses the preserved message",
        request_index(&fresh) == 0 && prompt(&fresh).contains("NEW_INSTRUCTION_AFTER_CANCEL"),
    );
    let _ = reply(
        controller,
        root,
        fresh,
        None,
        Some("Fresh primary answer."),
    );
}

fn check_initial_review(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let st = state(controller, root, "chat");
    let mut d = proposal(controller, root, st, agent(1));
    let pending = clone(field(reply_state(&d), "pending"));
    d = event(
        controller,
        root,
        reply_state(&d),
        "send",
        obj(vec![(
            "draft".into(),
            s("What is waiting for initial review?"),
        )]),
    );
    keep(
        passed,
        "status question before a child starts retains ordinary model access",
        has_effect(&d)
            && request_index(&d) == 0
            && *field(reply_state(&d), "pending") == pending
            && !reply_state(&d).get("task_run").is_some_and(|value| truthy(value)),
    );
    d = reply(
        controller,
        root,
        d,
        None,
        Some("The child has not yet started and awaits review."),
    );
    let _ = event(controller, root, reply_state(&d), "reject", obj(vec![]));
}

fn state(controller: &mut Controller, root: &str, format: &str) -> Value {
    let resume = event(controller, root, &Value::Null, "resume", obj(vec![]));
    let mut st = state_value(&resume);
    let profiles = [
        "Primary connection",
        "Exact child one",
        "Exact child two",
        "Unlinked connection",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, name)| {
        let endpoint = format!(
            "http://PRIVATE_{index}.test/v1/chat/completions?token=ENDPOINT_SECRET"
        );
        let key = format!("KEY_SECRET_{index}");
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
            ("protocol".into(), s(format)),
            ("endpoint".into(), s(&endpoint)),
            ("key".into(), s(&key)),
            ("auth_header".into(), s("X-PRIVATE-AUTH")),
            ("auth_prefix".into(), s("PREFIX_SECRET")),
            ("plain".into(), twin::b(true)),
            ("stream".into(), twin::b(false)),
            ("tool_format".into(), s("structured")),
        ])
    })
    .collect();
    put(&mut st, "profiles", Value::Arr(profiles));
    put(
        &mut st,
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
    st
}

fn proposal(controller: &mut Controller, root: &str, st: Value, args: Value) -> Value {
    let draft = obj(vec![(
        "draft".into(),
        s("Use a configured permitted child for this task."),
    )]);
    let sent = event(controller, root, &st, "send", draft);
    let ticket = text(field(&sent, "effect"), "model_ticket");
    let response = body(
        &text(field(&sent, "state"), "request_protocol"),
        Some(("delegate", args)),
        "Answer",
    );
    let json = response.to_string();
    let wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        json.len(),
        json
    );
    event(
        controller,
        root,
        field(&sent, "state"),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), s(&ticket)),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
    )
}

fn agent(index: i64) -> Value {
    obj(vec![
        ("agent".into(), twin::n(&index.to_string())),
        (
            "task".into(),
            s("Read actual.txt and return the findings."),
        ),
    ])
}

fn args(path: &str) -> Value {
    obj(vec![("path".into(), s(path))])
}

fn event(controller: &mut Controller, root: &str, st: &Value, action: &str, inputs: Value) -> Value {
    controller.event(Some(st), action, inputs, root)
}

fn reply(
    controller: &mut Controller,
    root: &str,
    d: Value,
    tool: Option<(&str, Value)>,
    response_text: Option<&str>,
) -> Value {
    reply_with_ticket(controller, root, d, tool, response_text, None)
}

fn reply_with_ticket(
    controller: &mut Controller,
    root: &str,
    d: Value,
    tool: Option<(&str, Value)>,
    response_text: Option<&str>,
    ticket: Option<&str>,
) -> Value {
    let st = state_value(&d);
    let format = text(&st, "request_protocol");
    let response = body(&format, tool, response_text.unwrap_or("Answer"));
    let json = response.to_string();
    let wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        json.len(),
        json
    );
    let expected = text(&st, "model_expected");
    let model_ticket = ticket.unwrap_or(&expected);
    event(
        controller,
        root,
        &st,
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), s(model_ticket)),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
    )
}

fn body(format: &str, tool: Option<(&str, Value)>, text: &str) -> Value {
    match format {
        "chat" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if let Some((name, arguments)) = &tool {
                let arguments = Value::parse(&arguments.to_string());
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
                                ("arguments".into(), s(&arguments.to_string())),
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
            if let Some((name, arguments)) = &tool {
                put(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![(
                        "function".into(),
                        obj(vec![
                            ("name".into(), s(name)),
                            ("arguments".into(), arguments.clone()),
                        ]),
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
            let mut parts = vec![obj(vec![
                ("type".into(), s("text")),
                ("text".into(), s(text)),
            ])];
            if let Some((name, arguments)) = &tool {
                parts.push(obj(vec![
                    ("type".into(), s("tool_use")),
                    ("id".into(), s("call_one")),
                    ("name".into(), s(name)),
                    ("input".into(), arguments.clone()),
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
            if let Some((name, arguments)) = &tool {
                parts.push(obj(vec![(
                    "functionCall".into(),
                    obj(vec![
                        ("name".into(), s(name)),
                        ("args".into(), arguments.clone()),
                    ]),
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
    }
}

fn complete_read(controller: &mut Controller, root: &str, d: Value) -> Value {
    let st = state_value(&d);
    let mut reviewed = event(controller, root, &st, "review", obj(vec![]));
    let reviewed_st = state_value(&reviewed);
    reviewed = event(controller, root, &reviewed_st, "approve", obj(vec![]));
    let reviewed_st = state_value(&reviewed);
    let effect = field(&reviewed, "effect");
    // The leg forwards the WHOLE effect into the tool op (op(dict(e, op='tool', root=...))), so the
    // approved dispatch carries its original args; forwarding a subset fails the dispatch check.
    let Value::Obj(effect_fields) = effect else {
        panic!("tool review emitted no object effect");
    };
    let mut envelope = Value::Obj(effect_fields.clone());
    envelope = put_owned(envelope, "op", s("tool"));
    envelope = put_owned(envelope, "root", s(root));
    let output = controller.op(&envelope);
    let purpose = text(effect, "purpose");
    let tool_ticket = text(effect, "tool_ticket");
    event(
        controller,
        root,
        &reviewed_st,
        "tool_result",
        obj(vec![
            ("purpose".into(), s(&purpose)),
            ("tool_ticket".into(), s(&tool_ticket)),
            (
                "tool_binding".into(),
                effect.get("tool_binding").cloned().unwrap_or(Value::Null),
            ),
            ("output".into(), output),
        ]),
    )
}

fn prompt(d: &Value) -> String {
    let effect = field(d, "effect");
    let wire = text(effect, "wire");
    let body = wire
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    let text = Value::parse(body);
    let mut result = Vec::new();
    strings(&text, &mut result);
    result.join("\n")
}

fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Str(text) => out.push(text.clone()),
        Value::Obj(fields) => {
            for (_, value) in fields {
                strings(value, out);
            }
        }
        Value::Arr(items) => {
            for value in items {
                strings(value, out);
            }
        }
        _ => {}
    }
}

fn messages(d: &Value) -> Vec<Value> {
    let state = state_value(d);
    state
        .get("chats")
        .and_then(as_array)
        .and_then(|chats| chats.iter().find(|chat| text(chat, "id") == active(d)))
        .and_then(|chat| chat.get("messages"))
        .and_then(as_array)
        .cloned()
        .unwrap_or_default()
}

fn active_chat(d: &Value) -> Value {
    let state = state_value(d);
    state
        .get("chats")
        .and_then(as_array)
        .and_then(|chats| {
            chats
                .iter()
                .find(|chat| text(chat, "id") == active(d))
        })
        .cloned()
        .unwrap_or_else(|| panic!("active chat is missing"))
}

fn pending_question(d: &Value) -> String {
    text(field(&state_value(d), "pending"), "delegate_question")
}

fn effect_wire(d: &Value) -> String {
    text(field(d, "effect"), "wire")
}

fn active(d: &Value) -> String {
    text(&state_value(d), "active")
}

fn request_index(d: &Value) -> i64 {
    match field(&state_value(d), "request_index") {
        Value::Num(value) => value
            .parse::<i64>()
            .unwrap_or_else(|_| panic!("request_index is not an integer")),
        _ => panic!("request_index is not an integer"),
    }
}

fn busy(d: &Value) -> bool {
    matches!(field(&state_value(d), "busy"), Value::Bool(true))
}

fn no_effect(d: &Value) -> bool {
    matches!(field(d, "effect"), Value::Null)
}

fn has_effect(d: &Value) -> bool {
    !no_effect(d)
}

fn notice(d: &Value) -> String {
    text(&state_value(d), "notice")
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

// A controller reply is an envelope; the leg reads and feeds the state sub-object, never the
// envelope itself.
fn reply_state<'a>(value: &'a Value) -> &'a Value {
    field(value, "state")
}

fn state_value(value: &Value) -> Value {
    value
        .get("state")
        .cloned()
        .unwrap_or_else(|| panic!("event returned no state"))
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(as_string)
        .unwrap_or_else(|| panic!("missing string {key}"))
        .to_string()
}

fn as_string(value: &Value) -> Option<&str> {
    match value {
        Value::Str(value) => Some(value),
        _ => None,
    }
}

fn as_array(value: &Value) -> Option<&Vec<Value>> {
    match value {
        Value::Arr(value) => Some(value),
        _ => None,
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Num(value) => value
            .parse::<f64>()
            .map(|value| value != 0.0)
            .unwrap_or(true),
        Value::Str(value) => !value.is_empty(),
        Value::Arr(value) => !value.is_empty(),
        Value::Obj(value) => !value.is_empty(),
    }
}

fn clone(value: &Value) -> Value {
    Value::parse(&value.to_string())
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

fn put_owned(mut value: Value, key: &str, next: Value) -> Value {
    put(&mut value, key, next);
    value
}

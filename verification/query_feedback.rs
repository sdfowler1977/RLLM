mod twin;

use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner public core-event checks through four controlled provider envelopes, actual journal failures/successes, current permission and approval ownership, bounded feedback, paused continuation/retry, real child file inspection and parent queries.";
const DRAFT: &str = "Use the selected journal operation and its actual result.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&project, "query-feedback");
    let root = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
        .to_string();
    std::fs::write(
        scratch.join("source.json"),
        "{\"value\":42,\"origin\":\"actual child source\"}\n",
    )
    .unwrap_or_else(|error| panic!("could not write source.json: {error}"));
    let mut controller = Controller::spawn(&project.to_string_lossy());
    let passed = run(&mut controller, &root);
    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);
    if passed.len() != 74 {
        panic!("emitted {} checks, expected 74", passed.len());
    }
    let body = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("query_feedback", &body, true);
    let written = std::fs::read_to_string("output/checks/query-feedback.json")
        .unwrap_or_else(|error| panic!("output/checks/query-feedback.json: {error}"));
    if written != format!("{}\n", twin::pretty(&body)) {
        panic!("output/checks/query-feedback.json has the wrong shape");
    }
    // The leg prints dict(result, count=len(passed)): the written record plus a count key.
    let printed = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
        ("count".into(), twin::n(&passed.len().to_string())),
    ]);
    println!("{}", twin::pretty(&printed));
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    provider_errors(controller, root, &mut passed);
    assigned_owners(controller, root, &mut passed);
    paused_designs(controller, root, &mut passed);
    feedback_bounds(controller, root, &mut passed);
    captured_reviews(controller, root, &mut passed);
    paused_task(controller, root, &mut passed);
    paused_child(controller, root, &mut passed);
    child_inspect(controller, root, &mut passed);
    passed
}

fn provider_errors(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    for format_name in ["chat", "local", "messages", "content"] {
        for (tool, args, reason) in failure_cases() {
            let st = fresh(controller, root, format_name);
            let mut turn = proposal(controller, root, &st, tool, &args);
            expect_dialog(&turn, "approval");
            turn = act(controller, root, &turn, "approve");
            let state = state_of(&turn);
            keep(
                passed,
                &format!("{format_name} {tool} runtime error returns to its current model"),
                effect_set(&turn)
                    && num_is(state, "request_index", "0")
                    && prompt(&turn).contains(reason)
                    && prompt(&turn).contains("\"status\":\"failed\"")
                    && falsy_get(state, "pending")
                    && num_is(state, "local_failure_followups", "1"),
            );
            turn = reply(controller, root, &turn, Some(tool), Some(&obj(vec![])), "Answer");
            turn = act(controller, root, &turn, "approve");
            expect_effect(&turn);
            turn = reply(controller, root, &turn, None, None, "Current query result received.");
            let state = state_of(&turn);
            keep(
                passed,
                &format!("{format_name} {tool} corrected query remains available and settles"),
                not_flag(state, "busy") && falsy_get(state, "pending") && not_flag(state, "agent_running"),
            );
        }
    }
}

fn assigned_owners(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    for (tool, args, reason) in failure_cases() {
        let mut st = fresh(controller, root, "chat");
        put(&mut st, "routing", obj(vec![("subagent".into(), s("1"))]));
        let mut turn = event(
            controller,
            root,
            &st,
            "delegate",
            obj(vec![("task".into(), s("Read the selected journal record."))]),
        );
        turn = reply(controller, root, &turn, Some(tool), Some(&args), "Answer");
        turn = act(controller, root, &turn, "approve");
        let state = state_of(&turn);
        keep(
            passed,
            &format!("{tool} standalone failure keeps its assigned model"),
            effect_set(&turn)
                && num_is(state, "request_index", "1")
                && prompt(&turn).contains(reason)
                && present_value(state, "task_run"),
        );
        turn = reply(
            controller,
            root,
            &turn,
            None,
            None,
            "Standalone received the current diagnostic.",
        );
        let state = state_of(&turn);
        keep(
            passed,
            &format!("{tool} standalone failure can settle its task"),
            not_flag(state, "busy") && falsy_get(state, "task_run"),
        );

        let child = fresh(controller, root, "chat");
        let mut turn = proposal(
            controller,
            root,
            &child,
            "delegate",
            &obj(vec![
                ("agent".into(), twin::n("1")),
                ("task".into(), s("Read the journal record.")),
            ]),
        );
        turn = act(controller, root, &turn, "approve");
        turn = reply(controller, root, &turn, Some(tool), Some(&args), "Answer");
        turn = act(controller, root, &turn, "approve");
        let returned = {
            let state = state_of(&turn);
            effect_set(&turn)
                && num_is(state, "request_index", "0")
                && prompt(&turn).contains(reason)
                && falsy_get(state, "delegation_stack")
        };
        turn = reply(
            controller,
            root,
            &turn,
            None,
            None,
            "Parent received the actual child query error.",
        );
        keep(
            passed,
            &format!("{tool} child failure returns to its captured parent and settles"),
            returned && not_flag(state_of(&turn), "busy"),
        );

        let mut st = fresh(controller, root, "chat");
        put(
            &mut st,
            "pending",
            obj(vec![("tool".into(), s(tool)), ("args".into(), args)]),
        );
        put(&mut st, "agent_pending", twin::b(false));
        put(&mut st, "agent_running", twin::b(false));
        put(&mut st, "dialog", s("approval"));
        put(&mut st, "busy", twin::b(false));
        let turn = event(controller, root, &st, "approve", empty());
        keep(
            passed,
            &format!("{tool} manual failure remains local"),
            effect_null(&turn) && field_contains(state_of(&turn), "notice", reason),
        );
    }
}

fn paused_designs(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    for format_name in ["chat", "local", "messages", "content"] {
        let marker = format!("OWNED_DESIGN_{format_name}");
        let st = fresh(controller, root, format_name);
        let mut turn = proposal(
            controller,
            root,
            &st,
            "hypotheses",
            &obj(vec![(
                "process".into(),
                obj(vec![
                    ("action".into(), s("note")),
                    ("kind".into(), s("design")),
                    ("text".into(), s(&marker)),
                ]),
            )]),
        );
        let mut st = state_of(&turn).clone();
        put(&mut st, "capability_mode", s("local"));
        turn = event(controller, root, &st, "approve", empty());
        let question = inquiry_item(state_of(&turn), &marker);
        let state = state_of(&turn);
        keep(
            passed,
            &format!("{format_name} approved design survives paused model continuation"),
            effect_null(&turn)
                && question.to_string().contains(&marker)
                && falsy_get(state, "pending")
                && not_flag(state, "busy")
                && question
                    .get("id")
                    .is_some_and(|id| value_in(state.get("output"), id)),
        );
        let mut st = state_of(&turn).clone();
        put(&mut st, "capability_mode", s("assisted"));
        turn = event(
            controller,
            root,
            &st,
            "send",
            obj(vec![("draft".into(), s("Read the approved saved design."))]),
        );
        let id = question.get("id").cloned().unwrap_or(Value::Null);
        turn = reply(
            controller,
            root,
            &turn,
            Some("hypotheses"),
            Some(&obj(vec![(
                "process".into(),
                obj(vec![
                    ("action".into(), s("current")),
                    ("id".into(), id),
                    ("section".into(), s("notes")),
                ]),
            )])),
            "Answer",
        );
        turn = act(controller, root, &turn, "approve");
        keep(
            passed,
            &format!("{format_name} fresh instruction reads the actual preserved design"),
            effect_set(&turn) && prompt(&turn).contains(&marker),
        );
        reply(controller, root, &turn, None, None, "Actual saved design received.");
    }
}

fn feedback_bounds(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let st = fresh(controller, root, "chat");
    let mut turn = proposal(controller, root, &st, "observations", &missing_record());
    turn = act(controller, root, &turn, "approve_always");
    keep(
        passed,
        "failed core query leaves no new standing approval",
        effect_set(&turn) && falsy_get(state_of(&turn), "approval_rules"),
    );
    reply(controller, root, &turn, None, None, "Actual query failure received.");

    let st = fresh(controller, root, "chat");
    let mut turn = proposal(controller, root, &st, "observations", &missing_record());
    turn = act(controller, root, &turn, "approve");
    let expected = state_of(&turn)
        .get("model_expected")
        .cloned()
        .unwrap_or(Value::Null);
    let again = act(controller, root, &turn, "approve");
    let again_state = state_of(&again);
    keep(
        passed,
        "repeated approval preserves the current error-feedback request",
        effect_null(&again)
            && again_state.get("model_expected") == Some(&expected)
            && num_is(again_state, "local_failure_followups", "1")
            && again_state.get("busy").is_some_and(twin::truthy),
    );
    reply(controller, root, &turn, None, None, "Actual diagnostic received.");

    let st = fresh(controller, root, "chat");
    let mut turn = proposal(controller, root, &st, "observations", &missing_record());
    turn = act(controller, root, &turn, "approve");
    for _ in 0..2 {
        turn = reply(
            controller,
            root,
            &turn,
            Some("observations"),
            Some(&missing_record()),
            "Answer",
        );
        turn = act(controller, root, &turn, "approve");
    }
    keep(
        passed,
        "core query errors share the bounded feedback allowance",
        effect_set(&turn) && num_is(state_of(&turn), "local_failure_followups", "3"),
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some("observations"),
        Some(&missing_record()),
        "Answer",
    );
    turn = act(controller, root, &turn, "approve");
    let state = state_of(&turn);
    keep(
        passed,
        "repeated query errors settle at the existing feedback bound",
        effect_null(&turn)
            && not_flag(state, "busy")
            && falsy_get(state, "pending")
            && field_contains(state, "notice", "Three operation error results"),
    );
    let st = state_of(&turn).clone();
    turn = proposal(controller, root, &st, "observations", &missing_record());
    turn = act(controller, root, &turn, "approve");
    keep(
        passed,
        "fresh instruction resets the core-query feedback allowance",
        effect_set(&turn) && num_is(state_of(&turn), "local_failure_followups", "1"),
    );
    reply(controller, root, &turn, None, None, "Fresh query diagnostic received.");

    let opened = fresh(controller, root, "chat");
    let mut turn = proposal(controller, root, &opened, "observations", &missing_record());
    let pending = state_of(&turn).get("pending").cloned().unwrap_or(Value::Null);
    let mut st = state_of(&turn).clone();
    put_path(&mut st, &["agents", "0", "tools"], s("read"));
    turn = event(controller, root, &st, "approve", empty());
    let state = state_of(&turn);
    keep(
        passed,
        "permission change still preserves the unexecuted review",
        effect_null(&turn)
            && state.get("pending") == Some(&pending)
            && field_contains(state, "notice", "disabled"),
    );
}

fn captured_reviews(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    for (tool, args) in authored() {
        let key = first_key(&args);
        for change in ["profile", "connection"] {
            let opened = fresh(controller, root, "chat");
            let mut turn = proposal(controller, root, &opened, tool, &args);
            expect_dialog(&turn, "approval");
            let pending = state_of(&turn).get("pending").cloned().unwrap_or(Value::Null);
            let mut st = state_of(&turn).clone();
            if change == "profile" {
                put_path(&mut st, &["profiles", "0", "model"], s("Changed after review"));
            } else {
                put(&mut st, "request_index", twin::n("1"));
            }
            turn = event(controller, root, &st, "approve_always", empty());
            let state = state_of(&turn);
            keep(
                passed,
                &format!("{tool} {key} changed {change} preserves captured review"),
                effect_null(&turn)
                    && state.get("pending") == Some(&pending)
                    && falsy_get(state, "approval_rules")
                    && notice_lower_has(state, "author"),
            );
            keep(
                passed,
                &format!("{tool} {key} changed {change} has no feedback or mutation"),
                falsy_get(state, "local_failure_followups")
                    && !dumps(state.get("inquiry_records")).contains("CAPTURED_AUTHOR")
                    && falsy_get(state, "investigation_claims")
                    && falsy_get(state, "investigation_experiments"),
            );
        }
    }
}

fn paused_task(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let mut st = fresh(controller, root, "chat");
    put(&mut st, "routing", obj(vec![("subagent".into(), s("1"))]));
    let mut turn = event(
        controller,
        root,
        &st,
        "delegate",
        obj(vec![("task".into(), s("Record a design note and return it."))]),
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some("hypotheses"),
        Some(&design_note("PAUSED_TASK_RESULT")),
        "Answer",
    );
    let mut st = state_of(&turn).clone();
    put(&mut st, "capability_mode", s("local"));
    turn = event(controller, root, &st, "approve", empty());
    let state = state_of(&turn);
    keep(
        passed,
        "paused task retains its successful local result and owner",
        effect_null(&turn)
            && present_value(state, "task_run")
            && dumps(state.get("inquiry_records")).contains("PAUSED_TASK_RESULT")
            && dumps(Some(state.get("task_run").unwrap_or(&Value::Null))).contains("PAUSED_TASK_RESULT"),
    );
    let mut st = state_of(&turn).clone();
    put(&mut st, "capability_mode", s("assisted"));
    turn = event(controller, root, &st, "retry", empty());
    keep(
        passed,
        "paused task resumes its assigned model with the actual saved result",
        effect_set(&turn) && num_is(state_of(&turn), "request_index", "1") && prompt(&turn).contains("PAUSED_TASK_RESULT"),
    );
    turn = reply(controller, root, &turn, None, None, "Actual saved task result received.");
    let state = state_of(&turn);
    keep(
        passed,
        "resumed successful task settles without replaying its journal operation",
        not_flag(state, "busy") && falsy_get(state, "task_run"),
    );
}

fn paused_child(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let opened = fresh(controller, root, "chat");
    let mut turn = proposal(
        controller,
        root,
        &opened,
        "delegate",
        &obj(vec![
            ("agent".into(), twin::n("1")),
            ("task".into(), s("Record the actual child design and return it.")),
        ]),
    );
    turn = act(controller, root, &turn, "approve");
    turn = reply(
        controller,
        root,
        &turn,
        Some("hypotheses"),
        Some(&design_note("PAUSED_CHILD_DESIGN")),
        "Answer",
    );
    let mut st = state_of(&turn).clone();
    put(&mut st, "capability_mode", s("local"));
    turn = event(controller, root, &st, "approve", empty());
    let state = state_of(&turn);
    keep(
        passed,
        "paused child design survives the captured parent return",
        effect_null(&turn)
            && falsy_get(state, "delegation_stack")
            && dumps(state.get("inquiry_records")).contains("PAUSED_CHILD_DESIGN"),
    );
    let before = state.get("inquiry_records").cloned().unwrap_or(Value::Null);
    let mut st = state_of(&turn).clone();
    put(&mut st, "capability_mode", s("assisted"));
    turn = event(controller, root, &st, "retry", empty());
    let state = state_of(&turn);
    keep(
        passed,
        "paused parent retry uses its original primary connection",
        effect_set(&turn) && num_is(state, "request_index", "0") && state.get("busy").is_some_and(twin::truthy),
    );
    turn = reply(controller, root, &turn, None, None, "Actual child design result retained.");
    let state = state_of(&turn);
    let after = state.get("inquiry_records").cloned().unwrap_or(Value::Null);
    keep(
        passed,
        "paused child design is retained once across parent retry",
        marker_count(&after, "PAUSED_CHILD_DESIGN") == marker_count(&before, "PAUSED_CHILD_DESIGN")
            && marker_count(&before, "PAUSED_CHILD_DESIGN").is_some()
            && not_flag(state, "busy"),
    );
}

fn child_inspect(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let opened = fresh(controller, root, "chat");
    let mut turn = proposal(
        controller,
        root,
        &opened,
        "delegate",
        &obj(vec![
            ("agent".into(), twin::n("1")),
            ("task".into(), s("Inspect source.json and return its actual observation ID.")),
        ]),
    );
    turn = act(controller, root, &turn, "approve");
    keep(
        passed,
        "review starts the configured child connection",
        num_is(state_of(&turn), "request_index", "1"),
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some("inspect"),
        Some(&obj(vec![("path".into(), s("source.json"))])),
        "Answer",
    );
    turn = act(controller, root, &turn, "approve");
    let effect = turn.get("effect").cloned().unwrap_or(Value::Null);
    let output = tool_output(controller, &effect, root);
    turn = act_inputs(
        controller,
        root,
        &turn,
        "tool_result",
        tool_result_inputs(&effect, output),
    );
    let record = last_item(state_of(&turn), "investigation_records");
    keep(
        passed,
        "actual child worker retains its own observation identity",
        eq_str(&record, "agent", "Research child") && eq_str(&record, "tool", "inspect"),
    );
    keep(
        passed,
        "child observation retains its task and depth",
        eq_str(&record, "mode", "delegate")
            && record.get("depth") == Some(&twin::n("1"))
            && field_contains(&record, "task", "source.json"),
    );
    if num_is(state_of(&turn), "request_index", "1") {
        let id = string_field(&record, "id");
        turn = reply(
            controller,
            root,
            &turn,
            None,
            None,
            &format!("Actual child observation {id}"),
        );
    }
    let id_text = string_field(&record, "id");
    let id_value = record.get("id").cloned().unwrap_or(Value::Null);
    keep(
        passed,
        "actual child result returns to the captured primary",
        effect_set(&turn)
            && num_is(state_of(&turn), "request_index", "0")
            && prompt(&turn).contains(&id_text)
            && falsy_get(state_of(&turn), "delegation_stack"),
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some("observations"),
        Some(&obj(vec![("id".into(), id_value)])),
        "Answer",
    );
    turn = act(controller, root, &turn, "approve");
    keep(
        passed,
        "primary queries the actual retained child report",
        effect_set(&turn) && prompt(&turn).contains(&id_text) && prompt(&turn).contains("Research child"),
    );
    turn = reply(controller, root, &turn, None, None, "Owned child report received.");
    keep(
        passed,
        "primary settles after consuming the child report",
        not_flag(state_of(&turn), "busy"),
    );
}

fn failure_cases() -> Vec<(&'static str, Value, &'static str)> {
    vec![
        ("observations", missing_record(), "Record unavailable"),
        ("hypotheses", obj(vec![("id".into(), twin::n("999"))]), "Claim unavailable"),
        ("experiments", obj(vec![("id".into(), twin::n("999"))]), "Experiment unavailable"),
    ]
}

fn missing_record() -> Value {
    obj(vec![("id".into(), s("missing-record"))])
}

fn design_note(text: &str) -> Value {
    obj(vec![(
        "process".into(),
        obj(vec![
            ("action".into(), s("note")),
            ("kind".into(), s("design")),
            ("text".into(), s(text)),
        ]),
    )])
}

fn authored() -> Vec<(&'static str, Value)> {
    vec![
        ("hypotheses", design_note("CAPTURED_AUTHOR")),
        (
            "hypotheses",
            obj(vec![(
                "create".into(),
                obj(vec![
                    ("question".into(), s("Source kind?")),
                    ("hypothesis".into(), s("JSON source")),
                ]),
            )]),
        ),
        (
            "experiments",
            obj(vec![(
                "create".into(),
                obj(vec![
                    ("claim".into(), twin::n("1")),
                    (
                        "operation".into(),
                        obj(vec![
                            ("tool".into(), s("inspect")),
                            ("args".into(), obj(vec![("path".into(), s("source.json"))])),
                        ]),
                    ),
                    (
                        "mapping".into(),
                        obj(vec![(
                            "source".into(),
                            obj(vec![
                                ("snapshot".into(), twin::n("0")),
                                ("path_hex".into(), s("")),
                            ]),
                        )]),
                    ),
                ]),
            )]),
        ),
    ]
}

fn fresh(controller: &mut Controller, root: &str, format_name: &str) -> Value {
    let resumed = controller.event(None, "resume", empty(), root);
    let mut st = state_of(&resumed).clone();
    put(
        &mut st,
        "profiles",
        Value::Arr(vec![profile("Primary", format_name), profile("Child", format_name)]),
    );
    put(&mut st, "agents", agents());
    st
}

fn profile(name: &str, format_name: &str) -> Value {
    obj(vec![
        ("name".into(), s(name)),
        ("model".into(), s(name)),
        ("endpoint".into(), s("http://controlled.test/model")),
        ("plain".into(), twin::b(true)),
        ("stream".into(), twin::b(false)),
        ("protocol".into(), s(format_name)),
    ])
}

fn agents() -> Value {
    Value::Arr(vec![
        obj(vec![
            ("name".into(), s("Parent")),
            ("model".into(), s("0")),
            ("tools".into(), s("list,read,inspect,observations,hypotheses,experiments,delegate")),
            ("subagents".into(), Value::Arr(vec![twin::n("1")])),
        ]),
        obj(vec![
            ("name".into(), s("Research child")),
            ("model".into(), s("1")),
            ("tools".into(), s("list,read,inspect,observations,hypotheses,experiments")),
        ]),
    ])
}

fn proposal(controller: &mut Controller, root: &str, state: &Value, tool: &str, args: &Value) -> Value {
    let sent = event(controller, root, state, "send", obj(vec![("draft".into(), s(DRAFT))]));
    reply(controller, root, &sent, Some(tool), Some(args), "Answer")
}

fn reply(
    controller: &mut Controller,
    root: &str,
    data: &Value,
    tool: Option<&str>,
    args: Option<&Value>,
    text: &str,
) -> Value {
    let state = state_of(data).clone();
    let protocol = match state.get("request_protocol") {
        Some(Value::Str(value)) => value.clone(),
        _ => panic!("state has no request_protocol"),
    };
    let content = if let Some(name) = tool {
        obj(vec![
            ("tool".into(), s(name)),
            ("args".into(), args.cloned().unwrap_or_else(empty)),
        ])
        .to_string()
    } else {
        text.to_string()
    };
    let body = response(&protocol, &content).to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let ticket = state
        .get("model_expected")
        .cloned()
        .unwrap_or_else(|| panic!("state has no model_expected"));
    event(
        controller,
        root,
        &state,
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
    )
}

fn response(format_name: &str, content: &str) -> Value {
    match format_name {
        "chat" => obj(vec![(
            "choices".into(),
            Value::Arr(vec![obj(vec![
                ("message".into(), obj(vec![("content".into(), s(content))])),
                ("finish_reason".into(), s("stop")),
            ])]),
        )]),
        "local" => obj(vec![
            ("message".into(), obj(vec![("content".into(), s(content))])),
            ("done".into(), twin::b(true)),
            ("done_reason".into(), s("stop")),
        ]),
        "messages" => obj(vec![
            (
                "content".into(),
                Value::Arr(vec![obj(vec![
                    ("type".into(), s("text")),
                    ("text".into(), s(content)),
                ])]),
            ),
            ("stop_reason".into(), s("end_turn")),
        ]),
        _ => obj(vec![(
            "candidates".into(),
            Value::Arr(vec![obj(vec![
                ("index".into(), twin::n("0")),
                (
                    "content".into(),
                    obj(vec![
                        ("role".into(), s("model")),
                        (
                            "parts".into(),
                            Value::Arr(vec![obj(vec![("text".into(), s(content))])]),
                        ),
                    ]),
                ),
                ("finishReason".into(), s("STOP")),
            ])]),
        )]),
    }
}

fn act(controller: &mut Controller, root: &str, data: &Value, action: &str) -> Value {
    let state = state_of(data).clone();
    event(controller, root, &state, action, empty())
}

fn act_inputs(controller: &mut Controller, root: &str, data: &Value, action: &str, inputs: Value) -> Value {
    let state = state_of(data).clone();
    event(controller, root, &state, action, inputs)
}

fn event(controller: &mut Controller, root: &str, state: &Value, action: &str, inputs: Value) -> Value {
    controller.event(Some(state), action, inputs, root)
}

fn tool_output(controller: &mut Controller, effect: &Value, root: &str) -> Value {
    // op(dict(e, op='tool', root=...)) clones every effect field, then overlays op and root.
    let Value::Obj(fields) = effect else {
        panic!("tool effect is not an object: {}", effect.to_string());
    };
    let mut envelope = Value::Obj(fields.clone());
    put(&mut envelope, "op", s("tool"));
    put(&mut envelope, "root", s(root));
    controller.op(&envelope)
}

fn tool_result_inputs(effect: &Value, output: Value) -> Value {
    let mut fields = Vec::new();
    for key in ["tool_ticket", "tool_binding", "editor", "editor_dialog"] {
        if effect.get(key).is_some() {
            fields.push((key.to_string(), effect.get(key).unwrap().clone()));
        }
    }
    fields.push(("purpose".into(), s("local_tool")));
    fields.push(("output".into(), output));
    Value::Obj(fields)
}

fn prompt(data: &Value) -> String {
    let effect = data.get("effect").unwrap_or_else(|| panic!("event returned no effect"));
    let wire = wire_text(effect);
    let body = wire
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("wire has no HTTP body"))
        .1;
    let value = Value::parse(body);
    let mut parts = Vec::new();
    collect_strings(&value, &mut parts);
    parts.join("\n")
}

fn wire_text(effect: &Value) -> String {
    match effect.get("wire") {
        Some(Value::Str(text)) => text.clone(),
        Some(Value::Arr(items)) => items
            .iter()
            .map(|item| match item {
                Value::Str(text) => text.clone(),
                _ => String::new(),
            })
            .collect(),
        Some(other) => panic!("wire is not text: {}", other.to_string()),
        None => panic!("effect has no wire"),
    }
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

fn inquiry_item(state: &Value, marker: &str) -> Value {
    let items = state
        .get("inquiry_records")
        .and_then(|records| records.get("items"))
        .unwrap_or_else(|| panic!("inquiry records are missing"));
    let Value::Arr(list) = items else {
        panic!("inquiry items are not a list");
    };
    list.iter()
        .find(|item| item.to_string().contains(marker))
        .cloned()
        .unwrap_or_else(|| panic!("inquiry item {marker} is missing"))
}

fn last_item(state: &Value, key: &str) -> Value {
    let items = state
        .get(key)
        .and_then(|records| records.get("items"))
        .unwrap_or_else(|| panic!("{key} items are missing"));
    let Value::Arr(list) = items else {
        panic!("{key} items are not a list");
    };
    list.last().cloned().unwrap_or_else(|| panic!("{key} items are empty"))
}

fn marker_count(records: &Value, marker: &str) -> Option<usize> {
    let items = records.get("items")?;
    let Value::Arr(list) = items else {
        return None;
    };
    Some(list.iter().map(|item| item.to_string().matches(marker).count()).sum())
}

fn dumps(value: Option<&Value>) -> String {
    match value {
        None => "null".to_string(),
        Some(item) => item.to_string(),
    }
}

fn value_in(container: Option<&Value>, needle: &Value) -> bool {
    match (container, needle) {
        (Some(Value::Str(text)), Value::Str(part)) => text.contains(part),
        (Some(Value::Arr(items)), item) => items.iter().any(|candidate| candidate == item),
        (Some(Value::Obj(fields)), Value::Str(key)) => fields.iter().any(|(name, _)| name == key),
        _ => false,
    }
}

fn field_contains(value: &Value, key: &str, needle: &str) -> bool {
    value_in(value.get(key), &s(needle)) || matches!(value.get(key), Some(Value::Str(text)) if text.contains(needle))
}

fn notice_lower_has(state: &Value, needle: &str) -> bool {
    matches!(state.get("notice"), Some(Value::Str(text)) if text.to_lowercase().contains(needle))
}

fn effect_null(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn effect_set(value: &Value) -> bool {
    value.get("effect").is_some_and(|item| !matches!(item, Value::Null))
}

fn present_value(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(|item| !matches!(item, Value::Null))
}

fn falsy_get(value: &Value, key: &str) -> bool {
    !value.get(key).is_some_and(|item| twin::truthy(item))
}

fn not_flag(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(item) => !twin::truthy(item),
        None => false,
    }
}

fn num_is(value: &Value, key: &str, number: &str) -> bool {
    value.get(key) == Some(&twin::n(number))
}

fn eq_str(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn string_field(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) => text.clone(),
        Some(other) => panic!("expected string {key}, got {}", other.to_string()),
        None => panic!("missing {key}"),
    }
}

fn first_key(value: &Value) -> String {
    match value {
        Value::Obj(fields) => fields
            .first()
            .map(|(key, _)| key.clone())
            .unwrap_or_else(|| panic!("args are empty")),
        _ => panic!("args are not an object"),
    }
}

fn expect_dialog(reply: &Value, dialog: &str) {
    let state = state_of(reply);
    if matches!(state.get("dialog"), Some(Value::Str(text)) if text == dialog) {
        return;
    }
    let notice = state.get("notice").map(Value::to_string).unwrap_or_else(|| "missing".into());
    let actual = state.get("dialog").map(Value::to_string).unwrap_or_else(|| "missing".into());
    panic!("expected dialog {dialog}, got {actual}, notice {notice}");
}

fn expect_effect(reply: &Value) {
    if effect_set(reply) {
        return;
    }
    let notice = state_of(reply)
        .get("notice")
        .map(Value::to_string)
        .unwrap_or_else(|| "missing".into());
    panic!("expected an effect, notice {notice}");
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn empty() -> Value {
    obj(vec![])
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

fn put_path(value: &mut Value, path: &[&str], next: Value) {
    let (key, rest) = path.split_first().unwrap_or_else(|| panic!("empty path"));
    if rest.is_empty() {
        put(value, key, next);
        return;
    }
    match value {
        Value::Obj(fields) => {
            let index = fields
                .iter()
                .position(|(name, _)| name == key)
                .unwrap_or_else(|| panic!("missing {key}"));
            put_path(&mut fields[index].1, rest, next);
        }
        Value::Arr(items) => {
            let index = key.parse::<usize>().unwrap_or_else(|_| panic!("array index expected, got {key}"));
            put_path(&mut items[index], rest, next);
        }
        other => panic!("expected container at {key}, found {}", other.to_string()),
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

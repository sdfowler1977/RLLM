mod twin;

use twin::{b, b64_encode, n, obj, s, truthy, Controller, Value};

const SCOPE: &str = "Owner public-interface checks: four controlled provider envelopes, actual configuration catalogs, controller routing, review identity, owned child callbacks and retained outcomes.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = twin::create_scratch(&root, "delegation-visibility");
    let scratch_text = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();
    if passed.len() != 94 {
        panic!("expected 94 checks, emitted {}", passed.len());
    }
    let names: Vec<Value> = passed.iter().map(|name| s(name)).collect();
    twin::report_value(
        "delegation_visibility",
        &obj(vec![
            ("passed".into(), Value::Arr(names)),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} delegation_visibility checks passed", passed.len());
    let _ = std::fs::remove_dir_all(&scratch);
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    for fmt in ["chat", "local", "messages", "content"] {
        let state = fresh_state(controller, root, fmt);
        let mut turn = event(
            controller,
            Some(&state),
            "send",
            obj(vec![("draft".into(), s("Which actual models and callable agents are configured?"))]),
            root,
        );
        let listed = catalog(&turn);
        keep(
            &mut passed,
            &format!("{fmt} request identifies the actual active primary model"),
            num_is(listed.get("current_model").and_then(|row| row.get("connection")), 0)
                && str_is(listed.get("current_model").and_then(|row| row.get("model")).and_then(|row| row.get("text")), "PRIMARY_MODEL"),
        );
        keep(
            &mut passed,
            &format!("{fmt} primary receives every configured model identity"),
            connection_models(&listed)
                == ["PRIMARY_MODEL", "CHILD_MODEL_ONE", "CHILD_MODEL_TWO", "UNLINKED_MODEL"],
        );
        keep(
            &mut passed,
            &format!("{fmt} callable agent indexes expose exact effective connection and role"),
            child_table(&listed)
                == vec![
                    (1, 1, "CHILD_MODEL_ONE".to_string(), "coding".to_string()),
                    (2, 2, "CHILD_MODEL_TWO".to_string(), "nonwrite".to_string()),
                ]
                && num_is(listed.get("default_agent"), 1),
        );
        let shown = prompt(&turn);
        keep(
            &mut passed,
            &format!("{fmt} model catalog exposes permission data without credentials"),
            str_is(listed.get("declared_parent_tools"), "read,hypotheses,delegate")
                && ["KEY_SECRET", "ENDPOINT_SECRET", "PRIVATE_0", "X-PRIVATE-AUTH", "PREFIX_SECRET"]
                    .iter()
                    .all(|secret| !shown.contains(secret)),
        );

        turn = reply(
            controller,
            root,
            &turn,
            Some(("delegate", &obj(vec![("task".into(), s("Reply with ready using the configured default child."))]))),
            None,
            "Answer",
        );
        let pending = state_of(&turn).get("pending");
        keep(
            &mut passed,
            &format!("{fmt} omitted selector uses declared default through real review"),
            str_is(state_of(&turn).get("dialog"), "approval")
                && num_is(pending.and_then(|row| row.get("child_connection")), 1)
                && str_is(pending.and_then(|row| row.get("child_model")), "CHILD_MODEL_ONE"),
        );
        turn = event_on(controller, root, &turn, "approve", empty());
        keep(
            &mut passed,
            &format!("{fmt} approved default starts the actual selected child"),
            effect_non_null(&turn) && num_is(state_of(&turn).get("request_index"), 1) && arr_len(state_of(&turn), "delegation_stack") == Some(1),
        );
        let child_catalog = catalog(&turn);
        keep(
            &mut passed,
            &format!("{fmt} child request identifies its actual configured model"),
            num_is(child_catalog.get("current_model").and_then(|row| row.get("connection")), 1)
                && str_is(
                    child_catalog.get("current_model").and_then(|row| row.get("model")).and_then(|row| row.get("text")),
                    "CHILD_MODEL_ONE",
                ),
        );

        let old_ticket = state_of(&turn).get("model_expected").cloned().unwrap_or_else(|| panic!("{fmt} child request has no model ticket"));
        let child_state = state_of(&turn).clone();
        turn = reply(controller, root, &turn, None, None, "READY_FROM_ACTUAL_CHILD");
        let returned = catalog(&turn);
        let last = returned.get("last_delegation");
        keep(
            &mut passed,
            &format!("{fmt} actual owned child return is visible to its captured parent"),
            num_is(state_of(&turn).get("request_index"), 0)
                && prompt(&turn).contains("READY_FROM_ACTUAL_CHILD")
                && str_is(last.and_then(|row| row.get("status")), "response_received")
                && last.and_then(|row| row.get("question")).is_some_and(truthy),
        );
        let before = state_of(&turn).clone();
        let duplicate = reply(controller, root, &turn, None, Some(&old_ticket), "FORGED_DUPLICATE");
        keep(
            &mut passed,
            &format!("{fmt} old child callback cannot settle the parent request"),
            state_of(&duplicate) == &before && effect_is_null(&duplicate),
        );
        turn = reply(controller, root, &turn, None, None, "Parent received the actual ready response.");
        assert_settled(&turn, &format!("{fmt} parent after owned child return"));
        let failed = event(
            controller,
            Some(&child_state),
            "failure",
            obj(vec![
                ("purpose".into(), s("model")),
                ("model_ticket".into(), old_ticket),
                ("error".into(), s("Controlled child transport failure")),
                ("sent".into(), b(false)),
            ]),
            root,
        );
        keep(
            &mut passed,
            &format!("{fmt} child transport failure returns an explicit parent result"),
            effect_non_null(&failed)
                && num_is(state_of(&failed).get("request_index"), 0)
                && prompt(&failed).contains("Controlled child transport failure")
                && str_is(catalog(&failed).get("last_delegation").and_then(|row| row.get("status")), "failed"),
        );
        let failed = reply(controller, root, &failed, None, None, "The child request failed.");
        assert_settled(&failed, &format!("{fmt} parent after child transport failure"));
    }

    for fmt in ["chat", "local", "messages", "content"] {
        let state = fresh_state(controller, root, fmt);
        let opened = proposal(
            controller,
            root,
            &state,
            obj(vec![
                ("agent".into(), n("1")),
                ("task".into(), s("Read actual.txt through a reviewed child operation.")),
            ]),
        );
        let approved = event_on(controller, root, &opened, "approve", empty());
        let reading = reply(
            controller,
            root,
            &approved,
            Some(("read", &obj(vec![("path".into(), s("actual.txt"))]))),
            None,
            "Answer",
        );
        let rejected = event_on(controller, root, &reading, "reject", empty());
        let last = state_of(&rejected).get("delegate_status").cloned().unwrap_or(Value::Null);
        keep(
            &mut passed,
            &format!("{fmt} rejected child operation records actual interruption and settles ownership"),
            effect_is_null(&rejected)
                && is_false(state_of(&rejected), "busy")
                && absent(state_of(&rejected), "task_run")
                && absent(state_of(&rejected), "delegation_stack")
                && absent(state_of(&rejected), "pending")
                && str_is(last.get("status"), "interrupted")
                && contains_val(last.get("detail"), "child's read proposal")
                && last.get("question").is_some_and(truthy),
        );
        let status = event_on(controller, root, &rejected, "status", empty());
        let ui = ui_text(&status);
        keep(
            &mut passed,
            &format!("{fmt} native status shows the actual stopped child transition"),
            ui.contains("Last child transition: interrupted") && ui.contains("child's read proposal"),
        );
        let followed = event(
            controller,
            Some(state_of(&status)),
            "send",
            obj(vec![("draft".into(), s("Explain the actual child task status."))]),
            root,
        );
        keep(
            &mut passed,
            &format!("{fmt} following primary request receives the actual interruption"),
            effect_non_null(&followed)
                && num_is(state_of(&followed).get("request_index"), 0)
                && catalog(&followed).get("last_delegation") == Some(&last),
        );
        let answered = reply(
            controller,
            root,
            &followed,
            None,
            None,
            "The user rejected its reviewed operation and stopped the child task.",
        );
        let fresh = event_on(controller, root, &answered, "new", empty());
        let fresh_status = event_on(controller, root, &fresh, "status", empty());
        keep(
            &mut passed,
            &format!("{fmt} a new conversation excludes another conversation child transition"),
            !ui_text(&fresh_status).contains("Last child transition:"),
        );
    }

    chat_cases(controller, root, &mut passed);
    passed
}

fn chat_cases(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let mut nested = fresh_state(controller, root, "chat");
    set_indexed(&mut nested, "agents", 1, "tools", s("read,hypotheses,delegate"));
    set_indexed(&mut nested, "agents", 1, "subagents", Value::Arr(vec![n("2")]));
    let mut turn = proposal(
        controller,
        root,
        &nested,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Ask the linked review child to check the task.")),
        ]),
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    turn = reply(
        controller,
        root,
        &turn,
        Some((
            "delegate",
            &obj(vec![("agent".into(), n("2")), ("task".into(), s("Review this child task."))]),
        )),
        None,
        "Answer",
    );
    if !str_is(state_of(&turn).get("pending").and_then(|row| row.get("tool")), "delegate") {
        panic!("rejected nested delegation records the already running child interruption");
    }
    turn = event_on(controller, root, &turn, "reject", empty());
    keep(
        passed,
        "rejected nested delegation records the already running child interruption",
        effect_is_null(&turn)
            && absent(state_of(&turn), "delegation_stack")
            && str_is(state_of(&turn).get("delegate_status").and_then(|row| row.get("status")), "interrupted")
            && contains_val(
                state_of(&turn).get("delegate_status").and_then(|row| row.get("detail")),
                "child's delegate proposal",
            ),
    );
    turn = event(
        controller,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("What happened to the running child?"))]),
        root,
    );
    keep(
        passed,
        "next primary request identifies the stopped child after nested review rejection",
        num_is(state_of(&turn).get("request_index"), 0)
            && str_is(catalog(&turn).get("last_delegation").and_then(|row| row.get("status")), "interrupted"),
    );
    let _answered = reply(controller, root, &turn, None, None, "The running child was stopped by the rejected review.");

    let st = fresh_state(controller, root, "chat");
    let selectors = [
        (obj(vec![("model".into(), s("CHILD_MODEL_TWO"))]), 2, r#"{"model": "CHILD_MODEL_TWO"}"#),
        (obj(vec![("model".into(), s("Exact child one"))]), 1, r#"{"model": "Exact child one"}"#),
        (obj(vec![("connection".into(), n("2"))]), 2, r#"{"connection": 2}"#),
        (obj(vec![("agent".into(), n("1"))]), 1, r#"{"agent": 1}"#),
        (obj(vec![("role".into(), s("nonwrite"))]), 2, r#"{"role": "nonwrite"}"#),
        (obj(vec![("specialty".into(), s("Review"))]), 2, r#"{"specialty": "Review"}"#),
    ];
    for (selector, index, rendered) in selectors {
        let proposed = proposal(controller, root, &st, with_task(&selector, "Use the exact configured selection."));
        keep(
            passed,
            &format!("selector {rendered} resolves the actual reviewed destination"),
            str_is(state_of(&proposed).get("dialog"), "approval")
                && num_is(state_of(&proposed).get("pending").and_then(|row| row.get("child_connection")), index),
        );
        let approved = event_on(controller, root, &proposed, "approve", empty());
        if !num_is(state_of(&approved).get("request_index"), index) {
            panic!("selector {rendered} resolves the actual reviewed destination");
        }
        let done = finish(controller, root, &approved);
        assert_settled(&done, rendered);
    }

    let rejected_selectors = [
        (obj(vec![("model".into(), s("guessed-provider-token"))]), r#"{"model": "guessed-provider-token"}"#),
        (obj(vec![("connection".into(), n("3"))]), r#"{"connection": 3}"#),
        (obj(vec![("agent".into(), n("0"))]), r#"{"agent": 0}"#),
        (obj(vec![("agent".into(), s("bad-index"))]), r#"{"agent": "bad-index"}"#),
        (obj(vec![("provider".into(), s("minimax"))]), r#"{"provider": "minimax"}"#),
    ];
    for (selector, rendered) in rejected_selectors {
        let proposed = proposal(controller, root, &st, with_task(&selector, "A bad selector must return an actual error."));
        keep(
            passed,
            &format!("rejected {rendered} returns feedback without a child"),
            effect_non_null(&proposed)
                && num_is(state_of(&proposed).get("request_index"), 0)
                && absent(state_of(&proposed), "delegation_stack")
                && prompt(&proposed).contains("Delegation result: blocked before start.")
                && str_is(catalog(&proposed).get("last_delegation").and_then(|row| row.get("status")), "blocked_before_start"),
        );
        let settled = reply(controller, root, &proposed, None, None, "The delegation was refused before execution.");
        assert_settled(&settled, rendered);
    }

    let mut turn = proposal(
        controller,
        root,
        &st,
        obj(vec![("model".into(), s("unknown")), ("task".into(), s("Do not loop."))]),
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some((
            "delegate",
            &obj(vec![
                ("model".into(), s("still-unknown")),
                ("task".into(), s("A second invalid selector.")),
            ]),
        )),
        None,
        "Answer",
    );
    keep(
        passed,
        "repeated invalid delegation cannot create an automatic feedback loop",
        effect_is_null(&turn)
            && is_true(state_of(&turn), "notice_error")
            && contains_val(state_of(&turn).get("notice"), "already returned once")
            && absent(state_of(&turn), "delegation_stack"),
    );
    turn = event(
        controller,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("A fresh instruction may receive one explicit rejection again."))]),
        root,
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some((
            "delegate",
            &obj(vec![
                ("model".into(), s("unknown-again")),
                ("task".into(), s("Return an explicit error.")),
            ]),
        )),
        None,
        "Answer",
    );
    keep(
        passed,
        "fresh native instruction resets bounded failure feedback",
        effect_non_null(&turn) && prompt(&turn).contains("Delegation result: blocked before start."),
    );
    let _settled = reply(controller, root, &turn, None, None, "Fresh explicit rejection received.");

    let mut duplicate = st.clone();
    set_indexed(&mut duplicate, "agents", 2, "model", s("1"));
    turn = proposal(
        controller,
        root,
        &duplicate,
        obj(vec![
            ("model".into(), s("CHILD_MODEL_ONE")),
            ("task".into(), s("Do not guess between two profiles.")),
        ]),
    );
    keep(
        passed,
        "ambiguous model identity requires an exact permitted agent",
        effect_non_null(&turn)
            && prompt(&turn).contains("Multiple permitted child profiles")
            && absent(state_of(&turn), "delegation_stack"),
    );
    let _settled = reply(controller, root, &turn, None, None, "Use an exact agent index.");
    turn = proposal(
        controller,
        root,
        &duplicate,
        obj(vec![
            ("agent".into(), n("2")),
            ("task".into(), s("An exact profile resolves ambiguity.")),
        ]),
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    keep(
        passed,
        "exact agent selector resolves shared-model ambiguity",
        task_agent_name(state_of(&turn)) == Some("Review child") && num_is(state_of(&turn).get("request_index"), 1),
    );
    let _done = finish(controller, root, &turn);

    let mut broken = st.clone();
    set_indexed(&mut broken, "agents", 1, "model", s("63"));
    turn = event(
        controller,
        Some(&broken),
        "send",
        obj(vec![("draft".into(), s("Show the actual routable default."))]),
        root,
    );
    let listed = catalog(&turn);
    keep(
        passed,
        "catalog marks a missing route and selects the next routable permitted default",
        child_callable(&listed, 0) == Some(false) && child_callable(&listed, 1) == Some(true) && num_is(listed.get("default_agent"), 2),
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some(("delegate", &obj(vec![("task".into(), s("Use the shown default."))]))),
        None,
        "Answer",
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    keep(
        passed,
        "default execution matches the catalog after a broken child route",
        num_is(state_of(&turn).get("request_index"), 2),
    );
    let _done = finish(controller, root, &turn);

    let mut disabled = st.clone();
    set_indexed(&mut disabled, "agents", 0, "tools", s("read"));
    turn = event(
        controller,
        Some(&disabled),
        "send",
        obj(vec![("draft".into(), s("Models remain visible without delegation authority."))]),
        root,
    );
    let listed = catalog(&turn);
    keep(
        passed,
        "disabled delegation still exposes configured identities without inventing callability",
        arr_len_value(listed.get("connections")) == Some(4)
            && matches!(listed.get("default_agent"), Some(Value::Null))
            && children_uncallable(&listed),
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some(("delegate", &obj(vec![("task".into(), s("A disabled operation must not execute."))]))),
        None,
        "Answer",
    );
    keep(
        passed,
        "disabled tool returns the actual permission refusal to its model",
        effect_non_null(&turn) && prompt(&turn).contains("Tool delegate is disabled") && absent(state_of(&turn), "delegation_stack"),
    );
    let _settled = reply(controller, root, &turn, None, None, "Delegation is disabled for this profile.");

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Native rejection should remain explicit.")),
        ]),
    );
    turn = event_on(controller, root, &turn, "reject", empty());
    keep(
        passed,
        "native rejection records that no child was started",
        effect_is_null(&turn)
            && str_is(state_of(&turn).get("delegate_status").and_then(|row| row.get("status")), "rejected_before_start"),
    );
    turn = event(
        controller,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("What happened to that proposed child?"))]),
        root,
    );
    keep(
        passed,
        "the next primary request sees the actual native rejection",
        str_is(catalog(&turn).get("last_delegation").and_then(|row| row.get("status")), "rejected_before_start"),
    );
    let _settled = reply(controller, root, &turn, None, None, "It was rejected before a child started.");

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![("agent".into(), n("1")), ("task".into(), s("Cancel this actual child."))]),
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    let ticket = state_of(&turn).get("model_expected").cloned().unwrap_or_else(|| panic!("cancellation records interruption and rejects the late result"));
    turn = event_on(controller, root, &turn, "cancel", empty());
    let late = reply(controller, root, &turn, None, Some(&ticket), "LATE_CHILD");
    keep(
        passed,
        "cancellation records interruption and rejects the late result",
        str_is(state_of(&late).get("delegate_status").and_then(|row| row.get("status")), "interrupted")
            && absent(state_of(&late), "delegation_stack")
            && effect_is_null(&late),
    );

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Keep an actual child return if parent preparation fails.")),
        ]),
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    let mut changed = turn.clone();
    set_indexed(state_mut(&mut changed), "profiles", 0, "endpoint", s("unsupported://parent"));
    turn = reply(controller, root, &changed, None, None, "ACTUAL_CHILD_RETURN_TO_RETAIN");
    keep(
        passed,
        "failed parent continuation preserves the actual child return and exact failure",
        effect_is_null(&turn)
            && absent(state_of(&turn), "delegation_stack")
            && messages_contain(state_of(&turn), "ACTUAL_CHILD_RETURN_TO_RETAIN")
            && str_is(
                state_of(&turn).get("delegate_status").and_then(|row| row.get("status")),
                "response_received_parent_unavailable",
            )
            && contains_val(state_of(&turn).get("notice"), "parent continuation failed"),
    );

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Do not silently change the reviewed model.")),
        ]),
    );
    let mut changed_state = state_of(&turn).clone();
    set_indexed(&mut changed_state, "agents", 1, "model", s("2"));
    turn = event(controller, Some(&changed_state), "approve", empty(), root);
    keep(
        passed,
        "changed reviewed child destination cannot start a different model",
        absent(state_of(&turn), "delegation_stack")
            && str_is(state_of(&turn).get("delegate_status").and_then(|row| row.get("status")), "blocked_before_start"),
    );
    if !(effect_is_null(&turn) || num_is(state_of(&turn).get("request_index"), 0)) {
        panic!("changed reviewed child destination cannot start a different model");
    }
    if effect_truthy(&turn) {
        let _answered = reply(controller, root, &turn, None, None, "A fresh review is required.");
    }

    let mut broken = st.clone();
    set_indexed(&mut broken, "agents", 1, "model", s("63"));
    turn = proposal(
        controller,
        root,
        &broken,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Return an explicit route error.")),
        ]),
    );
    keep(
        passed,
        "explicit unroutable agent returns the actual route error to its requester",
        effect_non_null(&turn)
            && num_is(state_of(&turn).get("request_index"), 0)
            && prompt(&turn).contains("Subagent connection not found")
            && str_is(catalog(&turn).get("last_delegation").and_then(|row| row.get("status")), "blocked_before_start"),
    );
    let _settled = reply(controller, root, &turn, None, None, "The selected route needs correction.");

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Check admission before starting a child.")),
        ]),
    );
    set(state_mut(&mut turn), "device", tight_device());
    turn = event_on(controller, root, &turn, "approve", empty());
    keep(
        passed,
        "response admission refusal leaves the captured parent settled",
        effect_is_null(&turn)
            && is_false(state_of(&turn), "busy")
            && absent(state_of(&turn), "agent_running")
            && absent(state_of(&turn), "task_run")
            && absent(state_of(&turn), "delegation_stack")
            && absent(state_of(&turn), "model_expected")
            && str_is(state_of(&turn).get("delegate_status").and_then(|row| row.get("status")), "blocked_before_start"),
    );
    keep(
        passed,
        "actual admission reason remains in the parent conversation",
        messages_contain(state_of(&turn), "Model response allowance")
            && contains_val(state_of(&turn).get("notice"), "Model response allowance"),
    );
    let file_path = std::path::Path::new(root).join("budget-result.txt");
    std::fs::write(&file_path, "Actual local file after response admission.").unwrap_or_else(|error| panic!("could not write {}: {error}", file_path.display()));
    let read = controller.op(&obj(vec![
        ("op".into(), s("tool")),
        ("tool".into(), s("read")),
        ("root".into(), s(root)),
        ("args".into(), obj(vec![("path".into(), s("budget-result.txt"))])),
    ]));
    keep(
        passed,
        "local file work remains usable after model admission refusal",
        matches!(read, Value::Str(text) if text == "Actual local file after response admission."),
    );
    let mut manual = event_on(controller, root, &turn, "investigation", empty());
    manual = event(
        controller,
        Some(state_of(&manual)),
        "inspect_file",
        obj(vec![("path".into(), s("budget-result.txt"))]),
        root,
    );
    manual = event_on(controller, root, &manual, "approve", empty());
    let effect = manual.get("effect").cloned().unwrap_or(Value::Null);
    if !matches!(effect, Value::Obj(_)) {
        panic!("native reviewed file work settles locally after model admission refusal");
    }
    let mut request = effect.clone();
    set(&mut request, "op", s("tool"));
    set(&mut request, "root", s(root));
    let output = controller.op(&request);
    manual = event(controller, Some(state_of(&manual)), "tool_result", tool_callback(&effect, output), root);
    keep(
        passed,
        "native reviewed file work settles locally after model admission refusal",
        effect_is_null(&manual) && is_false(state_of(&manual), "busy") && ui_text(&manual).contains("budget-result.txt"),
    );
    set(state_mut(&mut turn), "device", empty());
    turn = event(
        controller,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("Continue with the selected configured child."))]),
        root,
    );
    turn = reply(
        controller,
        root,
        &turn,
        Some((
            "delegate",
            &obj(vec![
                ("agent".into(), n("2")),
                ("task".into(), s("Return ready after admission recovery.")),
            ]),
        )),
        None,
        "Answer",
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    keep(
        passed,
        "fresh request after resource recovery starts the exact selected route",
        effect_non_null(&turn) && num_is(state_of(&turn).get("request_index"), 2),
    );
    let _done = finish(controller, root, &turn);

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Keep an approval-time route refusal visible.")),
        ]),
    );
    set_indexed(state_mut(&mut turn), "agents", 1, "model", s("63"));
    turn = event_on(controller, root, &turn, "approve", empty());
    keep(
        passed,
        "approval-time route refusal records its actual controller outcome",
        effect_is_null(&turn)
            && absent(state_of(&turn), "delegation_stack")
            && str_is(state_of(&turn).get("delegate_status").and_then(|row| row.get("status")), "blocked_before_start")
            && contains_val(
                state_of(&turn).get("delegate_status").and_then(|row| row.get("detail")),
                "Subagent connection not found",
            ),
    );

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Retain a failed child when its parent route fails.")),
        ]),
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    set_indexed(state_mut(&mut turn), "profiles", 0, "endpoint", s("unsupported://parent"));
    let ticket = state_of(&turn).get("model_expected").cloned().unwrap_or_else(|| panic!("failed parent preparation retains the actual child failure and retires its frame"));
    turn = event(
        controller,
        Some(state_of(&turn)),
        "failure",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("error".into(), s("OWNED_CHILD_FAILURE_TO_RETAIN")),
            ("sent".into(), b(false)),
        ]),
        root,
    );
    keep(
        passed,
        "failed parent preparation retains the actual child failure and retires its frame",
        absent(state_of(&turn), "delegation_stack")
            && absent(state_of(&turn), "model_expected")
            && effect_is_null(&turn)
            && messages_contain(state_of(&turn), "OWNED_CHILD_FAILURE_TO_RETAIN")
            && str_is(state_of(&turn).get("delegate_status").and_then(|row| row.get("status")), "failed_parent_unavailable"),
    );

    let mut nested = st.clone();
    set_indexed(&mut nested, "agents", 1, "tools", s("read,hypotheses,delegate"));
    set_indexed(&mut nested, "agents", 1, "subagents", Value::Arr(vec![n("2")]));
    turn = proposal(
        controller,
        root,
        &nested,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Run an owned child with permitted nested delegation.")),
        ]),
    );
    turn = event_on(controller, root, &turn, "approve", empty());
    turn = reply(
        controller,
        root,
        &turn,
        Some((
            "delegate",
            &obj(vec![
                ("agent".into(), n("2")),
                ("task".into(), s("A nested request must settle if admission fails.")),
            ]),
        )),
        None,
        "Answer",
    );
    set(state_mut(&mut turn), "device", tight_device());
    turn = event_on(controller, root, &turn, "approve", empty());
    keep(
        passed,
        "nested admission failure unwinds through captured parents with its actual reason",
        effect_is_null(&turn)
            && absent(state_of(&turn), "delegation_stack")
            && absent(state_of(&turn), "model_expected")
            && is_false(state_of(&turn), "busy")
            && messages_contain(state_of(&turn), "Model response allowance"),
    );

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("An original queued task stays associated with its question.")),
        ]),
    );
    let origin = state_of(&turn)
        .get("pending")
        .and_then(|row| row.get("delegate_question"))
        .cloned()
        .unwrap_or_else(|| panic!("primary sees the exact queued review and its originating question"));
    turn = event(
        controller,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("What is waiting for my review?"))]),
        root,
    );
    let pending = catalog(&turn).get("pending_delegation").cloned().unwrap_or(Value::Null);
    keep(
        passed,
        "primary sees the exact queued review and its originating question",
        str_is(pending.get("status"), "review_pending")
            && str_is(pending.get("model"), "CHILD_MODEL_ONE")
            && pending.get("originating_question") == Some(&origin)
            && str_is(
                pending.get("task").and_then(|row| row.get("text")),
                "An original queued task stays associated with its question.",
            ),
    );
    turn = reply(controller, root, &turn, None, None, "The configured child task is awaiting review.");
    turn = event_on(controller, root, &turn, "review", empty());
    turn = event_on(controller, root, &turn, "approve", empty());
    let child_id = state_of(&turn)
        .get("task_run")
        .and_then(|row| row.get("inquiry_id"))
        .cloned()
        .unwrap_or_else(|| panic!("queued child and resumed parent preserve the original question after a status turn"));
    let child_parent = inquiry_parent(state_of(&turn), &child_id)
        .unwrap_or_else(|| panic!("queued child and resumed parent preserve the original question after a status turn"));
    turn = reply(controller, root, &turn, None, None, "ORIGINAL_QUESTION_CHILD_RETURN");
    let chat_inquiry = active_chat(state_of(&turn)).and_then(|chat| chat.get("inquiry_id")).cloned();
    let returned_catalog = catalog(&turn);
    let last_question = returned_catalog.get("last_delegation").and_then(|row| row.get("question")).cloned();
    keep(
        passed,
        "queued child and resumed parent preserve the original question after a status turn",
        child_parent == origin && chat_inquiry.as_ref() == Some(&origin) && last_question.as_ref() == Some(&child_id),
    );
    turn = reply(controller, root, &turn, None, None, "Original queued task completed with its actual child return.");
    assert_settled(&turn, "queued child completion");

    turn = proposal(
        controller,
        root,
        &st,
        obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("A copied review cannot choose an earlier question.")),
        ]),
    );
    let old = state_of(&turn).get("pending").cloned().unwrap_or_else(|| panic!("model-supplied review metadata is replaced by the actual current question"));
    turn = event_on(controller, root, &turn, "reject", empty());
    turn = event(
        controller,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("Start this as a new native instruction."))]),
        root,
    );
    let current = active_chat(state_of(&turn))
        .and_then(|chat| chat.get("inquiry_id"))
        .cloned()
        .unwrap_or_else(|| panic!("model-supplied review metadata is replaced by the actual current question"));
    turn = reply(controller, root, &turn, None, None, &py_dumps(&old));
    let pending_question = state_of(&turn).get("pending").and_then(|row| row.get("delegate_question"));
    keep(
        passed,
        "model-supplied review metadata is replaced by the actual current question",
        str_is(state_of(&turn).get("dialog"), "approval")
            && pending_question == Some(&current)
            && pending_question != old.get("delegate_question"),
    );
    let _rejected = event_on(controller, root, &turn, "reject", empty());
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn assert_settled(turn: &Value, label: &str) {
    if !is_false(state_of(turn), "busy") {
        let notice = state_of(turn).get("notice").map(Value::to_string).unwrap_or_else(|| "none".into());
        panic!("{label} did not settle: {notice}");
    }
}

fn fresh_state(controller: &mut Controller, root: &str, fmt: &str) -> Value {
    let resumed = event(controller, None, "resume", empty(), root);
    let mut state = state_of(&resumed).clone();
    set(
        &mut state,
        "profiles",
        Value::Arr(vec![
            profile("Primary connection", "PRIMARY_MODEL", fmt, 0),
            profile("Exact child one", "CHILD_MODEL_ONE", fmt, 1),
            profile("Exact child two", "CHILD_MODEL_TWO", fmt, 2),
            profile("Unlinked connection", "UNLINKED_MODEL", fmt, 3),
        ]),
    );
    set(
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

fn profile(name: &str, model: &str, fmt: &str, index: usize) -> Value {
    obj(vec![
        ("name".into(), s(name)),
        ("model".into(), s(model)),
        ("protocol".into(), s(fmt)),
        ("endpoint".into(), s(&format!("http://PRIVATE_{index}.test/v1/chat/completions?token=ENDPOINT_SECRET"))),
        ("key".into(), s(&format!("KEY_SECRET_{index}"))),
        ("auth_header".into(), s("X-PRIVATE-AUTH")),
        ("auth_prefix".into(), s("PREFIX_SECRET")),
        ("plain".into(), b(true)),
        ("stream".into(), b(false)),
        ("tool_format".into(), s("structured")),
    ])
}

fn tight_device() -> Value {
    obj(vec![
        ("heap_bytes".into(), n("2097152")),
        ("memory_bytes".into(), n("4194304")),
        ("cores".into(), n("1")),
    ])
}

fn empty() -> Value {
    obj(vec![])
}

fn with_task(selector: &Value, task: &str) -> Value {
    let mut args = selector.clone();
    set(&mut args, "task", s(task));
    args
}

fn proposal(controller: &mut Controller, root: &str, state: &Value, args: Value) -> Value {
    let sent = event(
        controller,
        Some(state),
        "send",
        obj(vec![("draft".into(), s("Use a configured permitted child for this task."))]),
        root,
    );
    reply(controller, root, &sent, Some(("delegate", &args)), None, "Answer")
}

fn finish(controller: &mut Controller, root: &str, turn: &Value) -> Value {
    let returned = reply(controller, root, turn, None, None, "Actual child answer.");
    if !(effect_truthy(&returned) && num_is(state_of(&returned).get("request_index"), 0)) {
        let notice = state_of(&returned).get("notice").map(Value::to_string).unwrap_or_else(|| "none".into());
        panic!("child return did not resume its parent: {notice}");
    }
    reply(
        controller,
        root,
        &returned,
        None,
        None,
        "Parent answer using the actual returned child response.",
    )
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state, action, inputs, root)
}

fn event_on(controller: &mut Controller, root: &str, turn: &Value, action: &str, inputs: Value) -> Value {
    let state = state_of(turn).clone();
    event(controller, Some(&state), action, inputs, root)
}

fn reply(
    controller: &mut Controller,
    root: &str,
    turn: &Value,
    tool: Option<(&str, &Value)>,
    ticket: Option<&Value>,
    text: &str,
) -> Value {
    let state = state_of(turn).clone();
    let fmt = match state.get("request_protocol") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("model reply has no request protocol"),
    };
    let payload = body(&fmt, tool, text);
    let encoded = payload.to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{encoded}", encoded.len());
    let model_ticket = match ticket {
        Some(value) if truthy(value) => value.clone(),
        _ => match state.get("model_expected") {
            Some(value) => value.clone(),
            None => panic!("model reply has no model ticket"),
        },
    };
    event(
        controller,
        Some(&state),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), model_ticket),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
        root,
    )
}

fn body(fmt: &str, tool: Option<(&str, &Value)>, text: &str) -> Value {
    match fmt {
        "chat" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if let Some((name, args)) = tool {
                set(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![
                        ("id".into(), s("call_one")),
                        ("type".into(), s("function")),
                        (
                            "function".into(),
                            obj(vec![("name".into(), s(name)), ("arguments".into(), s(&py_dumps(args)))]),
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
            let mut message = obj(vec![("content".into(), s(text))]);
            if let Some((name, args)) = tool {
                set(
                    &mut message,
                    "tool_calls",
                    Value::Arr(vec![obj(vec![(
                        "function".into(),
                        obj(vec![("name".into(), s(name)), ("arguments".into(), args.clone())]),
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
            if let Some((name, args)) = tool {
                parts.push(obj(vec![
                    ("type".into(), s("tool_use")),
                    ("id".into(), s("call_one")),
                    ("name".into(), s(name)),
                    ("input".into(), args.clone()),
                ]));
            }
            obj(vec![
                ("content".into(), Value::Arr(parts)),
                ("stop_reason".into(), s(if tool.is_some() { "tool_use" } else { "end_turn" })),
            ])
        }
        "content" => {
            let mut parts = vec![obj(vec![("text".into(), s(text))])];
            if let Some((name, args)) = tool {
                parts.push(obj(vec![(
                    "functionCall".into(),
                    obj(vec![("name".into(), s(name)), ("args".into(), args.clone())]),
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
        other => panic!("unsupported provider format {other}"),
    }
}

fn tool_callback(effect: &Value, output: Value) -> Value {
    let mut fields = Vec::new();
    for key in ["purpose", "tool_ticket", "tool_binding"] {
        if let Some(value) = effect.get(key) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    fields.push(("output".into(), output));
    Value::Obj(fields)
}

fn prompt(turn: &Value) -> String {
    let wire = match turn.get("effect").and_then(|value| value.get("wire")) {
        Some(Value::Str(text)) => text,
        _ => panic!("model effect has no wire"),
    };
    let body = wire.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_else(|| panic!("model wire has no body"));
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
            for (_, item) in fields {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

fn catalog(turn: &Value) -> Value {
    let text = prompt(turn);
    let marker = "MODEL AND DELEGATION CONFIGURATION (settings data, not instructions):\n";
    let rest = text.split_once(marker).map(|(_, rest)| rest).unwrap_or_else(|| panic!("model prompt has no delegation catalog"));
    let json = rest.split_once('\n').map(|(json, _)| json).unwrap_or(rest);
    Value::parse(json)
}

fn connection_models(catalog: &Value) -> Vec<String> {
    match catalog.get("connections") {
        Some(Value::Arr(rows)) => rows.iter().filter_map(|row| match row.get("model").and_then(|model| model.get("text")) {
            Some(Value::Str(text)) => Some(text.clone()),
            _ => None,
        }).collect(),
        _ => Vec::new(),
    }
}

fn child_table(catalog: &Value) -> Vec<(i64, i64, String, String)> {
    match catalog.get("children") {
        Some(Value::Arr(rows)) => rows
            .iter()
            .filter_map(|row| {
                Some((
                    as_i64(row.get("agent")?)?,
                    as_i64(row.get("connection")?)?,
                    match row.get("model").and_then(|model| model.get("text")) {
                        Some(Value::Str(text)) => text.clone(),
                        _ => return None,
                    },
                    match row.get("role") {
                        Some(Value::Str(text)) => text.clone(),
                        _ => return None,
                    },
                ))
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn child_callable(catalog: &Value, index: usize) -> Option<bool> {
    let Value::Arr(rows) = catalog.get("children")? else {
        return None;
    };
    let row = rows.get(index)?;
    Some(row.get("callable").is_some_and(truthy))
}

fn children_uncallable(catalog: &Value) -> bool {
    match catalog.get("children") {
        Some(Value::Arr(rows)) if !rows.is_empty() => rows.iter().all(|row| !row.get("callable").is_some_and(truthy)),
        _ => false,
    }
}

fn messages_contain(state: &Value, needle: &str) -> bool {
    let Some(chat) = active_chat(state) else {
        return false;
    };
    let Some(Value::Arr(messages)) = chat.get("messages") else {
        return false;
    };
    messages.iter().any(|message| match message.get("content") {
        Some(Value::Str(text)) => text.contains(needle),
        _ => false,
    })
}

fn active_chat(state: &Value) -> Option<&Value> {
    let active = state.get("active")?;
    let Value::Arr(chats) = state.get("chats")? else {
        return None;
    };
    chats.iter().find(|chat| chat.get("id") == Some(active))
}

fn inquiry_parent(state: &Value, id: &Value) -> Option<Value> {
    let Value::Arr(items) = state.get("inquiry_records")?.get("items")? else {
        return None;
    };
    items.iter().find(|item| item.get("id") == Some(id)).and_then(|item| item.get("parent").cloned())
}

fn task_agent_name(state: &Value) -> Option<&str> {
    match state.get("task_run").and_then(|run| run.get("agent")).and_then(|agent| agent.get("name")) {
        Some(Value::Str(text)) => Some(text),
        _ => None,
    }
}

fn ui_text(turn: &Value) -> String {
    turn.get("ui").map(Value::to_string).unwrap_or_default()
}

fn py_dumps(value: &Value) -> String {
    match value {
        Value::Arr(items) => {
            let body = items.iter().map(py_dumps).collect::<Vec<_>>().join(", ");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let body = fields
                .iter()
                .map(|(key, item)| format!("{}: {}", Value::Str(key.clone()).to_string(), py_dumps(item)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{body}}}")
        }
        other => other.to_string(),
    }
}

fn as_i64(value: &Value) -> Option<i64> {
    let Value::Num(text) = value else {
        return None;
    };
    if let Ok(number) = text.parse::<i64>() {
        return Some(number);
    }
    text.parse::<f64>().ok().and_then(|number| (number.fract() == 0.0).then_some(number as i64))
}

fn num_is(value: Option<&Value>, expected: i64) -> bool {
    value.and_then(as_i64) == Some(expected)
}

fn str_is(value: Option<&Value>, expected: &str) -> bool {
    matches!(value, Some(Value::Str(text)) if text == expected)
}

fn contains_val(value: Option<&Value>, needle: &str) -> bool {
    matches!(value, Some(Value::Str(text)) if text.contains(needle))
}

fn is_true(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(truthy)
}

fn is_false(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(|item| !truthy(item))
}

fn absent(value: &Value, key: &str) -> bool {
    match value.get(key) {
        None | Some(Value::Null) => true,
        Some(item) => !truthy(item),
    }
}

fn arr_len(value: &Value, key: &str) -> Option<usize> {
    match value.get(key) {
        Some(Value::Arr(items)) => Some(items.len()),
        _ => None,
    }
}

fn arr_len_value(value: Option<&Value>) -> Option<usize> {
    match value {
        Some(Value::Arr(items)) => Some(items.len()),
        _ => None,
    }
}

fn effect_is_null(turn: &Value) -> bool {
    matches!(turn.get("effect"), Some(Value::Null))
}

fn effect_non_null(turn: &Value) -> bool {
    matches!(turn.get("effect"), Some(value) if !matches!(value, Value::Null))
}

fn effect_truthy(turn: &Value) -> bool {
    turn.get("effect").is_some_and(truthy)
}

fn state_of(turn: &Value) -> &Value {
    turn.get("state").unwrap_or_else(|| panic!("event returned no state: {}", turn.to_string()))
}

fn state_mut(turn: &mut Value) -> &mut Value {
    get_mut(turn, "state")
}

fn get_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object when reading {key}");
    };
    let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
    &mut fields[index].1
}

fn set(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object when setting {key}");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn set_indexed(value: &mut Value, list_key: &str, index: usize, field: &str, next: Value) {
    let list = get_mut(value, list_key);
    let Value::Arr(items) = list else {
        panic!("expected {list_key} array");
    };
    let item = items.get_mut(index).unwrap_or_else(|| panic!("{list_key}[{index}] missing"));
    set(item, field, next);
}

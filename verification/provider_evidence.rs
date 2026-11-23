//! Owner public-interface checks for controlled provider replies.
//! Same sequence as verification/provider_evidence.py.
mod twin;

use twin::{b, b64_encode, n, obj, s, sha256_bytes, truthy, Controller, Value};

const SCOPE: &str = "Owner public-interface checks: controlled provider replies, native question records, finite comparisons, guarded file export and refinement transitions.";
const GENERAL_TOOLS: &str = "read,hypotheses,web_search";
const SECRETS: &[&str] = &[
    "PROFILE_SECRET",
    "ENDPOINT_SECRET",
    "ENVELOPE_SECRET",
    "INPUT_SECRET",
    "OPAQUE_SECRET",
    "UNFETCHED_PAGE_SECRET",
    "THINKING_SECRET",
    "SIGNATURE_SECRET",
    "REDACTED_SECRET",
];

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = twin::create_scratch(&root, "provider-evidence");
    let scratch_text = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);
    if passed.len() != 40 {
        panic!("provider_evidence emitted {} checks", passed.len());
    }
    twin::report_value(
        "provider_evidence",
        &obj(vec![
            ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} provider_evidence checks passed", passed.len());
}

fn run(controller: &mut Controller, scratch: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let st = fresh_state(controller, scratch, GENERAL_TOOLS);
    let sent = controller.event(Some(&st), "send", draft("Investigate the reported source."), scratch);
    let original_ticket = sent
        .get("state")
        .and_then(|state| state.get("model_expected"))
        .cloned()
        .unwrap_or_else(|| panic!("send did not assign model_expected"));
    let blocks = search_blocks("one", "observed source", "Reported source", "https://example.test/source", None, true);
    let held = reply(controller, scratch, &sent, blocks.clone(), "end_turn", None);
    let receipt = case_receipt(&held);

    keep(
        &mut passed,
        "ordinary owned search automatically retains one evidence receipt",
        evidence_len(&held) == Some(1)
            && receipt.as_ref().is_some_and(|item| text_eq(item, "source_kind", "provider_search"))
            && not_busy(&held),
    );
    let report = receipt.as_ref().and_then(first_report);
    let metadata = receipt.as_ref().and_then(output_json);
    keep(
        &mut passed,
        "receipt correlates observed query and reported source",
        report.as_ref().is_some_and(reported_source),
    );
    keep(
        &mut passed,
        "provider metadata is not presented as fetched pages or verified truth",
        metadata.as_ref().is_some_and(|value| text_eq(value, "type", "provider_reported_search_metadata"))
            && metadata.as_ref().is_some_and(|value| text_contains(value, "meaning", "Digests identify this exact metadata projection"))
            && receipt.as_ref().is_some_and(|item| text_eq(item, "source_kind", "provider_search")),
    );
    keep(
        &mut passed,
        "projection excludes opaque blocks and profile credentials",
        case_last(&held).is_some_and(|case| SECRETS.iter().all(|secret| !case.to_string().contains(secret))),
    );
    keep(
        &mut passed,
        "same-profile private protocol history retains the actual original blocks",
        hosted_matches(&held, &blocks),
    );
    keep(
        &mut passed,
        "receipt digests measure exact metadata bytes",
        receipt.as_ref().is_some_and(digest_matches),
    );

    let duplicate = reply(controller, scratch, &held, blocks, "end_turn", Some(&original_ticket));
    keep(
        &mut passed,
        "completed callback cannot create a duplicate receipt",
        same_records(&duplicate, &held),
    );

    let mut view = viewed(controller, scratch, state_of(&held));
    keep(
        &mut passed,
        "native returned-evidence page shows the same provider receipt",
        ui_has(&view, "provider_search") && ui_has(&view, "Reported source"),
    );
    let view_state = state_of(&view).clone();
    view = controller.event(Some(&view_state), "inquiry_check", obj(vec![]), scratch);
    let view_state = state_of(&view).clone();
    view = controller.event(
        Some(&view_state),
        "inquiry_check_save",
        obj(vec![
            ("evidence".into(), s("1")),
            ("method".into(), s("contains")),
            ("expected".into(), s("https://example.test/source")),
            ("requirement".into(), s("The metadata reports this source")),
        ]),
        scratch,
    );
    keep(
        &mut passed,
        "finite check uses actual retained metadata",
        last_verdict(&view) == Some("Supported"),
    );
    let view_state = state_of(&view).clone();
    view = controller.event(Some(&view_state), "inquiry_export", obj(vec![]), scratch);
    let view_state = state_of(&view).clone();
    view = controller.event(
        Some(&view_state),
        "inquiry_export_review",
        obj(vec![("output".into(), s("provider-evidence.json"))]),
        scratch,
    );
    let view_state = state_of(&view).clone();
    view = controller.event(Some(&view_state), "approve", obj(vec![]), scratch);
    let effect = view.get("effect").cloned().unwrap_or(Value::Null);
    let output = controller.op(&tool_envelope(&effect, scratch));
    keep(
        &mut passed,
        "reviewed real file export retains exact receipt and comparison",
        receipt.as_ref().is_some_and(|saved| export_matches(scratch, saved)),
    );
    let view_state = state_of(&view).clone();
    view = controller.event(Some(&view_state), "tool_result", tool_result_inputs(&effect, output), scratch);
    settle_quiet(&view, "export tool_result");

    for (key, value) in [
        ("key", s("CHANGED_SECRET")),
        ("endpoint", s("http://changed.test/v1/messages")),
        ("hosted_identity", s("changed-identity")),
        ("hosted_search", s("off")),
    ] {
        let mut changed = state_of(&held).clone();
        set_profile_field(&mut changed, key, value);
        let hidden = viewed(controller, scratch, &changed);
        let modal = modal_dump(&hidden);
        keep(
            &mut passed,
            &format!("changed {key} hides historical provider evidence"),
            modal.contains("Question unavailable") && !modal.contains("Reported source"),
        );
    }

    let mut changed = state_of(&held).clone();
    put(
        &mut changed,
        "harness_policy",
        obj(vec![
            ("schema".into(), n("1")),
            (
                "rules".into(),
                Value::Arr(vec![obj(vec![
                    ("id".into(), s("denied")),
                    ("scope".into(), s("tool")),
                    ("tool".into(), s("web_search")),
                    ("decision".into(), s("deny")),
                    ("text".into(), s("No search evidence")),
                ])]),
            ),
        ]),
    );
    controller.op(&obj(vec![("op".into(), s("harness_policy_commit")), ("state".into(), changed.clone())]));
    let hidden = viewed(controller, scratch, &changed);
    keep(
        &mut passed,
        "current policy change hides old provider receipts",
        modal_dump(&hidden).contains("Question unavailable"),
    );
    let mut restored = state_of(&hidden).clone();
    put(&mut restored, "harness_policy", Value::Null);
    controller.op(&obj(vec![("op".into(), s("harness_policy_commit")), ("state".into(), restored)]));

    let mut forged = state_of(&held).clone();
    forge_receipt(&mut forged);
    let hidden = viewed(controller, scratch, &forged);
    keep(
        &mut passed,
        "modified receipt output fails journal integrity",
        modal_dump(&hidden).contains("Question unavailable"),
    );

    let qid = case_last(&held)
        .and_then(|case| case.get("id"))
        .cloned()
        .unwrap_or_else(|| panic!("question id"));
    let read_state = state_of(&held).clone();
    let mut model = controller.event(
        Some(&read_state),
        "send",
        draft("Read the actual earlier source receipt."),
        scratch,
    );
    model = model_process(
        controller,
        scratch,
        &model,
        obj(vec![
            ("action".into(), s("current")),
            ("id".into(), qid.clone()),
            ("section".into(), s("evidence")),
        ]),
    );
    keep(
        &mut passed,
        "model reads provider evidence without enabling a local web fallback",
        effect_some(&model) && wire_has(&model, "Reported source") && field_truthy(state_of(&model), "request_hosted_mode"),
    );
    model = reply(controller, scratch, &model, text_block("Answered from the retained metadata."), "end_turn", None);
    settle_quiet(&model, "provider evidence read");

    let mut revoked = state_of(&held).clone();
    set_agent_tools(&mut revoked, "read,hypotheses");
    model = controller.event(Some(&revoked), "send", draft("The old source needs current permission."), scratch);
    model = model_process(
        controller,
        scratch,
        &model,
        obj(vec![
            ("action".into(), s("current")),
            ("id".into(), qid),
            ("section".into(), s("evidence")),
        ]),
    );
    let feedback = feedback_dump(&model);
    keep(
        &mut passed,
        "model permission revocation returns an error without historical source disclosure",
        feedback.contains("Question unavailable")
            && !feedback.contains("Reported source")
            && feedback.contains("completed_result")
            && no_pending(&model),
    );
    let cancel_state = state_of(&model).clone();
    controller.event(Some(&cancel_state), "cancel", obj(vec![]), scratch);

    let intrinsic_state = fresh_state(controller, scratch, "read,web_search");
    let mut intrinsic = controller.event(
        Some(&intrinsic_state),
        "send",
        draft("Investigate with no journal tool or selected skill."),
        scratch,
    );
    keep(
        &mut passed,
        "intrinsic method needs no journal tool invocation",
        wire_has(&intrinsic, "Investigation is intrinsic") && !wire_has(&intrinsic, "process:{action"),
    );
    intrinsic = reply(controller, scratch, &intrinsic, search_named("one"), "end_turn", None);
    keep(
        &mut passed,
        "automatic source capture needs no journal tool permission",
        evidence_len(&intrinsic) == Some(1) && not_busy(&intrinsic),
    );

    let task_state = fresh_state(controller, scratch, GENERAL_TOOLS);
    let mut standalone = controller.event(
        Some(&task_state),
        "delegate",
        obj(vec![("task".into(), s("Investigate as an ordinary assigned task."))]),
        scratch,
    );
    standalone = reply(controller, scratch, &standalone, search_named("one"), "end_turn", None);
    keep(
        &mut passed,
        "standalone task also captures its actual provider source",
        evidence_len(&standalone) == Some(1) && no_stack(&standalone) && not_busy(&standalone),
    );

    let mut child_state = fresh_state(controller, scratch, GENERAL_TOOLS);
    add_child_profile(&mut child_state);
    put(
        &mut child_state,
        "agents",
        Value::Arr(vec![
            obj(vec![
                ("name".into(), s("Parent")),
                ("model".into(), s("0")),
                ("tools".into(), s("hypotheses,delegate,web_search")),
                ("subagents".into(), Value::Arr(vec![n("1")])),
            ]),
            obj(vec![
                ("name".into(), s("Child")),
                ("model".into(), s("1")),
                ("tools".into(), s("read,web_search")),
                ("subagents".into(), Value::Arr(vec![])),
            ]),
        ]),
    );
    let mut child = controller.event(
        Some(&child_state),
        "send",
        draft("Have the actual child investigate the reported source."),
        scratch,
    );
    let parent_id = case_last(&child)
        .and_then(|case| case.get("id"))
        .cloned()
        .unwrap_or_else(|| panic!("parent question id"));
    child = reply(
        controller,
        scratch,
        &child,
        text_block(&delegate_text("1", "Inspect provider-reported sources.")),
        "end_turn",
        None,
    );
    let child_state = state_of(&child).clone();
    child = controller.event(Some(&child_state), "approve", obj(vec![]), scratch);
    keep(
        &mut passed,
        "reviewed child retains intrinsic method and its actual owner",
        index_is(&child, 1) && wire_has(&child, "Investigation is intrinsic") && ticket_matches(&child),
    );
    let child_id = case_last(&child)
        .and_then(|case| case.get("id"))
        .cloned()
        .unwrap_or_else(|| panic!("child question id"));
    child = reply(controller, scratch, &child, search_named("child"), "end_turn", None);
    let child_case = find_case(&child, &child_id);
    let child_call = child_case.and_then(|case| match case.get("evidence") {
        Some(Value::Arr(items)) => items.first().and_then(first_report),
        _ => None,
    });
    keep(
        &mut passed,
        "actual child return retains evidence in its own linked question",
        child_case.and_then(|case| case.get("parent")) == Some(&parent_id)
            && child_case.and_then(|case| match case.get("evidence") {
                Some(Value::Arr(items)) => Some(items.len()),
                _ => None,
            }) == Some(1)
            && child_call.as_ref().is_some_and(|report| text_eq(report, "call_id", "search_child")),
    );
    keep(
        &mut passed,
        "owned child source response resumes the captured parent",
        index_is(&child, 0) && no_stack(&child) && wire_has(&child, "Answer using the reported metadata."),
    );
    child = reply(controller, scratch, &child, text_block("Parent answer from actual child work."), "end_turn", None);
    settle_quiet(&child, "child return");

    let pause_state = fresh_state(controller, scratch, GENERAL_TOOLS);
    let mut pending = controller.event(Some(&pause_state), "send", draft("A provider query pauses before results."), scratch);
    pending = reply(controller, scratch, &pending, query_only("search_later", "continuation query"), "pause_turn", None);
    keep(
        &mut passed,
        "query-only paused response does not invent completed evidence",
        evidence_is_empty(&pending) && dialog_is(&pending, "hosted_pause"),
    );
    let pending_state = state_of(&pending).clone();
    pending = controller.event(Some(&pending_state), "hosted_continue", obj(vec![]), scratch);
    pending = reply(
        controller,
        scratch,
        &pending,
        search_blocks("later", "observed source", "Reported source", "https://example.test/source", None, false),
        "end_turn",
        None,
    );
    keep(
        &mut passed,
        "explicit continuation correlates the actual prior query",
        case_receipt(&pending)
            .as_ref()
            .and_then(first_report)
            .is_some_and(|report| text_eq(&report, "query", "continuation query")),
    );

    let fail_state = fresh_state(controller, scratch, GENERAL_TOOLS);
    let mut failed = controller.event(Some(&fail_state), "send", draft("Report actual provider search failure."), scratch);
    failed = reply(
        controller,
        scratch,
        &failed,
        search_blocks("one", "observed source", "Reported source", "https://example.test/source", Some("rate_limited"), true),
        "end_turn",
        None,
    );
    let failed_receipt = case_receipt(&failed);
    keep(
        &mut passed,
        "reported provider errors remain labelled observed errors",
        failed_receipt
            .as_ref()
            .and_then(first_report)
            .is_some_and(|report| text_eq(&report, "provider_reported_error", "rate_limited"))
            && failed_receipt.as_ref().is_some_and(|item| match item.get("output") {
                Some(Value::Str(text)) => !text.contains("ERROR_SECRET"),
                _ => false,
            })
            && not_busy(&failed),
    );

    let large_title = "X".repeat(12000);
    let large_state = fresh_state(controller, scratch, GENERAL_TOOLS);
    let mut oversized = controller.event(Some(&large_state), "send", draft("Retain a bounded prefix."), scratch);
    oversized = reply(
        controller,
        scratch,
        &oversized,
        search_blocks("one", "observed source", &large_title, "https://example.test/source", None, true),
        "end_turn",
        None,
    );
    keep(
        &mut passed,
        "large result preserves an explicit bounded metadata prefix",
        case_receipt(&oversized).as_ref().is_some_and(prefix_bounded),
    );
    let oversized_state = state_of(&oversized).clone();
    oversized = controller.event(Some(&oversized_state), "inquiry", obj(vec![]), scratch);
    let oversized_state = state_of(&oversized).clone();
    oversized = controller.event(Some(&oversized_state), "inquiry_check", obj(vec![]), scratch);
    let oversized_state = state_of(&oversized).clone();
    oversized = controller.event(
        Some(&oversized_state),
        "inquiry_check_save",
        obj(vec![
            ("evidence".into(), s("1")),
            ("method".into(), s("contains")),
            ("expected".into(), s("ABSENT_FROM_PREFIX")),
            ("requirement".into(), s("Missing content cannot be declared false from a prefix")),
        ]),
        scratch,
    );
    keep(
        &mut passed,
        "missing partial-output match stays unresolved",
        last_verdict(&oversized) == Some("Unresolved"),
    );

    let cancel_before = fresh_state(controller, scratch, GENERAL_TOOLS);
    let before = controller.event(Some(&cancel_before), "send", draft("Cancel this in-flight search."), scratch);
    let ticket = before
        .get("state")
        .and_then(|state| state.get("model_expected"))
        .cloned()
        .unwrap_or_else(|| panic!("cancel send did not assign model_expected"));
    let before_state = state_of(&before).clone();
    let mut cancel = controller.event(Some(&before_state), "cancel", obj(vec![]), scratch);
    cancel = reply(controller, scratch, &cancel, search_named("one"), "end_turn", Some(&ticket));
    keep(&mut passed, "cancelled model callback cannot retain evidence", evidence_is_empty(&cancel));

    let changed_before = fresh_state(controller, scratch, GENERAL_TOOLS);
    let before = controller.event(
        Some(&changed_before),
        "send",
        draft("Do not accept a changed connection callback."),
        scratch,
    );
    let mut changed_flight = before.clone();
    set_profile_field(state_mut(&mut changed_flight), "key", s("CHANGED"));
    changed_flight = reply(controller, scratch, &changed_flight, search_named("one"), "end_turn", None);
    keep(
        &mut passed,
        "changed in-flight connection cannot capture returned sources",
        evidence_is_empty(&changed_flight) && effect_null(&changed_flight),
    );

    let saved_case = case_last(&held).cloned().unwrap_or_else(|| panic!("saved question"));
    for mode in ["off", "malformed", "full"] {
        let mut mode_state = fresh_state(controller, scratch, GENERAL_TOOLS);
        if mode == "off" {
            put(&mut mode_state, "investigation_retention_off", b(true));
        }
        if mode == "malformed" {
            put(
                &mut mode_state,
                "inquiry_records",
                obj(vec![("schema".into(), n("1")), ("items".into(), s("broken"))]),
            );
        }
        if mode == "full" {
            put(
                &mut mode_state,
                "inquiry_records",
                obj(vec![
                    ("schema".into(), n("1")),
                    ("items".into(), Value::Arr(vec![saved_case.clone(); 32])),
                ]),
            );
        }
        let mut answer = controller.event(
            Some(&mode_state),
            "send",
            draft(&format!("Answer normally when optional retention is {mode}")),
            scratch,
        );
        answer = reply(controller, scratch, &answer, search_named("one"), "end_turn", None);
        keep(
            &mut passed,
            &format!("{mode} optional journal does not block actual answer"),
            not_busy(&answer) && effect_null(&answer) && ui_has(&answer, "Answer using the reported metadata."),
        );
    }

    let capacity_state = fresh_state(controller, scratch, GENERAL_TOOLS);
    let mut full = controller.event(
        Some(&capacity_state),
        "send",
        draft("Keep actual results until this question capacity is full."),
        scratch,
    );
    for n in 0..17 {
        let stop = if n == 16 { "end_turn" } else { "pause_turn" };
        full = reply(controller, scratch, &full, search_named(&n.to_string()), stop, None);
        if n < 16 {
            let full_state = state_of(&full).clone();
            full = controller.event(Some(&full_state), "hosted_continue", obj(vec![]), scratch);
        }
    }
    keep(
        &mut passed,
        "receipt capacity retains earlier results and reports omission without gating",
        evidence_len(&full) == Some(16) && omitted_is(&full, 1) && not_busy(&full) && status_has(&full, "not retained"),
    );

    let refine_state = fresh_state(controller, scratch, GENERAL_TOOLS);
    let mut refine = controller.event(
        Some(&refine_state),
        "refine",
        obj(vec![("task".into(), s("Refine using actual reported sources."))]),
        scratch,
    );
    refine = reply(controller, scratch, &refine, search_named("one"), "end_turn", None);
    let body = wire_json(&refine);
    keep(
        &mut passed,
        "proposal automatically receives existing journal metadata",
        phase_is(&refine, "propose") && wire_has(&refine, "provider_reports") && wire_has(&refine, "Reported source"),
    );
    keep(
        &mut passed,
        "synthesis does not receive private protocol state",
        body.as_ref().is_some_and(body_hides_protocol),
    );
    keep(
        &mut passed,
        "metadata read does not re-enable server search in proposal",
        body.as_ref().is_some_and(proposal_without_server_search),
    );
    refine = reply(controller, scratch, &refine, text_block("Candidate from the actual metadata."), "end_turn", None);
    refine = reply(controller, scratch, &refine, search_named("probe"), "end_turn", None);
    keep(
        &mut passed,
        "composition includes both actual baseline and probe metadata",
        phase_is(&refine, "compose") && wire_has(&refine, "search_one") && wire_has(&refine, "search_probe"),
    );
    refine = reply(controller, scratch, &refine, text_block("Composed from actual returned sources."), "end_turn", None);
    keep(
        &mut passed,
        "refinement finishes its existing review after automatic evidence capture",
        dialog_is(&refine, "refinement_review") && not_busy(&refine),
    );
    passed
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
    println!("{name}");
}

fn settle_quiet(data: &Value, step: &str) {
    if not_busy(data) {
        return;
    }
    let notice = data
        .get("state")
        .and_then(|state| state.get("notice"))
        .map(Value::to_string)
        .unwrap_or_else(|| "none".into());
    panic!("{step} left the session busy: {notice}");
}

fn fresh_state(controller: &mut Controller, scratch: &str, tools: &str) -> Value {
    let resumed = controller.event(None, "resume", obj(vec![]), scratch);
    let mut state = state_of(&resumed).clone();
    put(&mut state, "profiles", Value::Arr(vec![controlled_profile()]));
    put(
        &mut state,
        "agents",
        Value::Arr(vec![obj(vec![("name".into(), s("General")), ("tools".into(), s(tools))])]),
    );
    state
}

fn controlled_profile() -> Value {
    obj(vec![
        ("name".into(), s("Controlled")),
        ("endpoint".into(), s("http://provider.test/v1/messages?private=ENDPOINT_SECRET")),
        ("key".into(), s("PROFILE_SECRET")),
        ("protocol".into(), s("messages")),
        ("model".into(), s("controlled")),
        ("plain".into(), b(true)),
        ("stream".into(), b(false)),
        ("hosted_search".into(), s("messages_search")),
        ("tool_format".into(), s("structured")),
    ])
}

fn draft(text: &str) -> Value {
    obj(vec![("draft".into(), s(text))])
}

fn search_named(name: &str) -> Value {
    search_blocks(name, "observed source", "Reported source", "https://example.test/source", None, true)
}

fn search_blocks(name: &str, query: &str, title: &str, url: &str, error: Option<&str>, include_call: bool) -> Value {
    let mut rows = Vec::new();
    if include_call {
        rows.push(obj(vec![
            ("type".into(), s("server_tool_use")),
            ("id".into(), s(&format!("search_{name}"))),
            ("name".into(), s("web_search")),
            (
                "input".into(),
                obj(vec![("query".into(), s(query)), ("private".into(), s("INPUT_SECRET"))]),
            ),
        ]));
    }
    let result = if let Some(error) = error {
        obj(vec![
            ("type".into(), s("web_search_tool_result_error")),
            ("error_code".into(), s(error)),
            ("private".into(), s("ERROR_SECRET")),
        ])
    } else {
        Value::Arr(vec![obj(vec![
            ("type".into(), s("web_search_result")),
            ("title".into(), s(title)),
            ("url".into(), s(url)),
            ("encrypted_content".into(), s("OPAQUE_SECRET")),
            ("page".into(), s("UNFETCHED_PAGE_SECRET")),
        ])])
    };
    rows.push(obj(vec![
        ("type".into(), s("web_search_tool_result")),
        ("tool_use_id".into(), s(&format!("search_{name}"))),
        ("content".into(), result),
    ]));
    rows.push(obj(vec![
        ("type".into(), s("thinking")),
        ("thinking".into(), s("THINKING_SECRET")),
        ("signature".into(), s("SIGNATURE_SECRET")),
    ]));
    rows.push(obj(vec![
        ("type".into(), s("redacted_thinking")),
        ("data".into(), s("REDACTED_SECRET")),
    ]));
    rows.push(obj(vec![
        ("type".into(), s("text")),
        ("text".into(), s("Answer using the reported metadata.")),
    ]));
    Value::Arr(rows)
}

fn query_only(id: &str, query: &str) -> Value {
    Value::Arr(vec![obj(vec![
        ("type".into(), s("server_tool_use")),
        ("id".into(), s(id)),
        ("name".into(), s("web_search")),
        ("input".into(), obj(vec![("query".into(), s(query))])),
    ])])
}

fn text_block(text: &str) -> Value {
    Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])])
}

fn delegate_text(agent: &str, task: &str) -> String {
    obj(vec![
        ("tool".into(), s("delegate")),
        (
            "args".into(),
            obj(vec![("agent".into(), n(agent)), ("task".into(), s(task))]),
        ),
    ])
    .to_string()
}

fn hypotheses_text(args: Value) -> String {
    obj(vec![
        ("tool".into(), s("hypotheses")),
        ("args".into(), obj(vec![("process".into(), args)])),
    ])
    .to_string()
}

fn wire_b64(blocks: &Value, stop: &str) -> String {
    let body = obj(vec![
        ("role".into(), s("assistant")),
        ("content".into(), blocks.clone()),
        ("stop_reason".into(), s(stop)),
        ("private_envelope".into(), s("ENVELOPE_SECRET")),
    ])
    .to_string();
    let raw = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    b64_encode(raw.as_bytes())
}

fn reply(controller: &mut Controller, scratch: &str, data: &Value, blocks: Value, stop: &str, ticket: Option<&Value>) -> Value {
    let state = state_of(data).clone();
    let model_ticket = ticket.cloned().unwrap_or_else(|| field(&state, "model_expected").clone());
    let mut inputs = vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), model_ticket),
        ("data".into(), s(&wire_b64(&blocks, stop))),
    ];
    if let Some(effect) = data.get("effect") {
        if truthy(effect) {
            for key in ["child_ticket", "child_owner"] {
                if let Some(value) = effect.get(key) {
                    inputs.push((key.into(), value.clone()));
                }
            }
        }
    }
    controller.event(Some(&state), "network_result", obj(inputs), scratch)
}

fn model_process(controller: &mut Controller, scratch: &str, data: &Value, args: Value) -> Value {
    let replied = reply(controller, scratch, data, text_block(&hypotheses_text(args)), "end_turn", None);
    if !dialog_is(&replied, "approval") {
        let notice = replied
            .get("state")
            .and_then(|state| state.get("notice"))
            .map(Value::to_string)
            .unwrap_or_else(|| "none".into());
        panic!("model process did not open approval: {notice}");
    }
    let state = state_of(&replied).clone();
    controller.event(Some(&state), "approve", obj(vec![]), scratch)
}

fn viewed(controller: &mut Controller, scratch: &str, state: &Value) -> Value {
    let opened = controller.event(Some(state), "inquiry", obj(vec![]), scratch);
    let next = state_of(&opened).clone();
    controller.event(Some(&next), "inquiry_section:evidence", obj(vec![]), scratch)
}

fn tool_envelope(effect: &Value, scratch: &str) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("export effect is not an object: {}", effect.to_string());
    };
    let mut out = Vec::new();
    let mut saw_op = false;
    let mut saw_root = false;
    for (key, value) in fields {
        match key.as_str() {
            "op" => {
                out.push(("op".into(), s("tool")));
                saw_op = true;
            }
            "root" => {
                out.push(("root".into(), s(scratch)));
                saw_root = true;
            }
            _ => out.push((key.clone(), value.clone())),
        }
    }
    if !saw_op {
        out.push(("op".into(), s("tool")));
    }
    if !saw_root {
        out.push(("root".into(), s(scratch)));
    }
    obj(out)
}

fn tool_result_inputs(effect: &Value, output: Value) -> Value {
    let mut fields = Vec::new();
    for key in ["purpose", "tool_ticket", "tool_binding"] {
        if let Some(value) = effect.get(key) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    fields.push(("output".into(), output));
    obj(fields)
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn state_mut(value: &mut Value) -> &mut Value {
    field_mut(value, "state")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
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

fn items_mut(value: &mut Value) -> &mut Vec<Value> {
    let Value::Arr(items) = value else {
        panic!("expected array");
    };
    items
}

fn set_profile_field(state: &mut Value, key: &str, value: Value) {
    let profiles = items_mut(field_mut(state, "profiles"));
    put(&mut profiles[0], key, value);
}

fn set_agent_tools(state: &mut Value, tools: &str) {
    let agents = items_mut(field_mut(state, "agents"));
    put(&mut agents[0], "tools", s(tools));
}

fn add_child_profile(state: &mut Value) {
    let profiles = items_mut(field_mut(state, "profiles"));
    let mut child = profiles[0].clone();
    put(&mut child, "name", s("Child connection"));
    put(&mut child, "endpoint", s("http://child.test/v1/messages"));
    profiles.push(child);
}

fn forge_receipt(state: &mut Value) {
    let items = items_mut(field_mut(field_mut(state, "inquiry_records"), "items"));
    let last = items.last_mut().unwrap_or_else(|| panic!("no question to forge"));
    let evidence = items_mut(field_mut(last, "evidence"));
    put(&mut evidence[0], "output", s("Forged output"));
}

fn case_last(data: &Value) -> Option<&Value> {
    match data.get("state").and_then(|state| state.get("inquiry_records")).and_then(|records| records.get("items")) {
        Some(Value::Arr(items)) => items.last(),
        _ => None,
    }
}

fn case_receipt(data: &Value) -> Option<Value> {
    match case_last(data).and_then(|case| case.get("evidence")) {
        Some(Value::Arr(items)) => items.last().cloned(),
        _ => None,
    }
}

fn find_case<'a>(data: &'a Value, id: &Value) -> Option<&'a Value> {
    let items = data.get("state")?.get("inquiry_records")?.get("items")?;
    let Value::Arr(items) = items else {
        return None;
    };
    items.iter().find(|case| case.get("id") == Some(id))
}

fn evidence_len(data: &Value) -> Option<usize> {
    match case_last(data).and_then(|case| case.get("evidence")) {
        Some(Value::Arr(items)) => Some(items.len()),
        _ => None,
    }
}

fn evidence_is_empty(data: &Value) -> bool {
    evidence_len(data) == Some(0)
}

fn omitted_is(data: &Value, expect: u64) -> bool {
    match case_last(data).and_then(|case| case.get("omitted_evidence")) {
        Some(value) => num_is(value, expect),
        None => false,
    }
}

fn last_verdict(data: &Value) -> Option<&str> {
    match case_last(data).and_then(|case| case.get("checks")) {
        Some(Value::Arr(items)) => match items.last().and_then(|item| item.get("verdict")) {
            Some(Value::Str(text)) => Some(text.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn text_eq(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn text_contains(value: &Value, key: &str, needle: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text.contains(needle))
}

fn num_is(value: &Value, expect: u64) -> bool {
    match value {
        Value::Num(token) => token.parse::<u64>().ok() == Some(expect),
        _ => false,
    }
}

fn num_gt(value: &Value, expect: u64) -> bool {
    match value {
        Value::Num(token) => token.parse::<u64>().ok().is_some_and(|number| number > expect),
        _ => false,
    }
}

fn not_busy(data: &Value) -> bool {
    match data.get("state").and_then(|state| state.get("busy")) {
        Some(value) => !truthy(value),
        None => false,
    }
}

fn field_truthy(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(truthy)
}

fn field_falsy(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(item) => !truthy(item),
        None => false,
    }
}

fn no_pending(data: &Value) -> bool {
    match data.get("state") {
        Some(state) => match state.get("pending") {
            None => true,
            Some(value) => !truthy(value),
        },
        None => false,
    }
}

fn no_stack(data: &Value) -> bool {
    match data.get("state") {
        Some(state) => match state.get("delegation_stack") {
            None => true,
            Some(value) => !truthy(value),
        },
        None => false,
    }
}

fn dialog_is(data: &Value, expect: &str) -> bool {
    data.get("state").is_some_and(|state| text_eq(state, "dialog", expect))
}

fn index_is(data: &Value, expect: u64) -> bool {
    data.get("state").and_then(|state| state.get("request_index")).is_some_and(|value| num_is(value, expect))
}

fn phase_is(data: &Value, expect: &str) -> bool {
    data.get("state")
        .and_then(|state| state.get("task_run"))
        .and_then(|run| run.get("phase"))
        .is_some_and(|phase| matches!(phase, Value::Str(text) if text == expect))
}

fn effect_some(data: &Value) -> bool {
    match data.get("effect") {
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

fn effect_null(data: &Value) -> bool {
    matches!(data.get("effect"), Some(Value::Null))
}

fn effect_wire(data: &Value) -> Option<&str> {
    match data.get("effect").and_then(|effect| effect.get("wire")) {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    }
}

fn wire_has(data: &Value, needle: &str) -> bool {
    effect_wire(data).is_some_and(|wire| wire.contains(needle))
}

fn wire_json(data: &Value) -> Option<Value> {
    let body = effect_wire(data)?.split_once("\r\n\r\n")?.1;
    Some(Value::parse(body))
}

fn ui_has(data: &Value, needle: &str) -> bool {
    data.get("ui").is_some_and(|ui| ui.to_string().contains(needle))
}

fn modal_dump(data: &Value) -> String {
    match data.get("ui").and_then(|ui| ui.get("modal")) {
        Some(modal) => modal.to_string(),
        None => "null".into(),
    }
}

fn status_has(data: &Value, needle: &str) -> bool {
    match data.get("state").and_then(|state| state.get("inquiry_status")) {
        Some(Value::Str(text)) => text.contains(needle),
        _ => false,
    }
}

fn output_json(receipt: &Value) -> Option<Value> {
    match receipt.get("output") {
        Some(Value::Str(text)) => Some(Value::parse(text)),
        _ => None,
    }
}

fn first_report(receipt: &Value) -> Option<Value> {
    match output_json(receipt).as_ref().and_then(|metadata| metadata.get("reports")) {
        Some(Value::Arr(items)) => items.first().cloned(),
        _ => None,
    }
}

fn reported_source(report: &Value) -> bool {
    let expected = Value::Arr(vec![obj(vec![
        ("title".into(), s("Reported source")),
        ("url".into(), s("https://example.test/source")),
    ])]);
    text_eq(report, "query", "observed source") && text_eq(report, "call_id", "search_one")
        && report.get("results") == Some(&expected)
}

fn digest_matches(receipt: &Value) -> bool {
    let Some(Value::Str(output)) = receipt.get("output") else {
        return false;
    };
    let measured = output.as_bytes();
    let digest = sha256_bytes(measured);
    field_truthy(receipt, "complete")
        && receipt.get("bytes").is_some_and(|value| num_is(value, measured.len() as u64))
        && text_eq(receipt, "sha256", &digest)
        && text_eq(receipt, "prefix_sha256", &digest)
}

fn prefix_bounded(receipt: &Value) -> bool {
    let Some(Value::Str(output)) = receipt.get("output") else {
        return false;
    };
    let measured = output.as_bytes();
    field_falsy(receipt, "complete")
        && receipt.get("bytes").is_some_and(|value| num_gt(value, 8192))
        && measured.len() <= 8192
        && text_eq(receipt, "prefix_sha256", &sha256_bytes(measured))
}

fn hosted_matches(data: &Value, blocks: &Value) -> bool {
    let Some(state) = data.get("state") else {
        return false;
    };
    let Some(active) = state.get("active") else {
        return false;
    };
    let Some(Value::Arr(chats)) = state.get("chats") else {
        return false;
    };
    let Some(chat) = chats.iter().find(|chat| chat.get("id") == Some(active)) else {
        return false;
    };
    let Some(Value::Arr(hosted)) = chat.get("hosted_responses") else {
        return false;
    };
    let Some(raw) = hosted.last() else {
        return false;
    };
    raw.get("content") == Some(blocks)
        && matches!(raw.get("raw").and_then(|item| item.get("private_envelope")), Some(Value::Str(text)) if text == "ENVELOPE_SECRET")
}

fn same_records(left: &Value, right: &Value) -> bool {
    let left_records = left.get("state").and_then(|state| state.get("inquiry_records"));
    let right_records = right.get("state").and_then(|state| state.get("inquiry_records"));
    left_records.is_some() && left_records == right_records
}

fn export_matches(scratch: &str, receipt: &Value) -> bool {
    let path = format!("{scratch}/provider-evidence.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let exported = Value::parse(&text);
    let Some(data) = exported.get("data") else {
        return false;
    };
    let Some(Value::Arr(evidence)) = data.get("evidence") else {
        return false;
    };
    let Some(Value::Arr(checks)) = data.get("checks") else {
        return false;
    };
    evidence.first() == Some(receipt) && checks.last().is_some_and(|item| text_eq(item, "verdict", "Supported"))
}

fn ticket_matches(data: &Value) -> bool {
    let Some(expected) = data.get("state").and_then(|state| state.get("model_expected")) else {
        return false;
    };
    data.get("effect").and_then(|effect| effect.get("model_ticket")) == Some(expected)
}

fn feedback_dump(data: &Value) -> String {
    let Some(effect) = data.get("effect") else {
        return "{}".into();
    };
    if !truthy(effect) {
        return "{}".into();
    }
    let Some(body) = wire_json(data) else {
        return String::new();
    };
    match body.get("messages") {
        Some(Value::Arr(messages)) => messages.last().map(Value::to_string).unwrap_or_else(|| "null".into()),
        _ => String::new(),
    }
}

fn body_hides_protocol(body: &Value) -> bool {
    let encoded = body.to_string();
    ["THINKING_SECRET", "OPAQUE_SECRET", "SIGNATURE_SECRET", "PROFILE_SECRET", "ENVELOPE_SECRET"]
        .iter()
        .all(|secret| !encoded.contains(secret))
}

fn proposal_without_server_search(body: &Value) -> bool {
    match body.get("tools") {
        None => true,
        Some(Value::Arr(tools)) => !tools.iter().any(|tool| matches!(tool.get("name"), Some(Value::Str(name)) if name == "web_search")),
        Some(_) => false,
    }
}

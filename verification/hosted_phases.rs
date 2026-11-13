mod twin;

use std::path::Path;

use twin::{b, b64_encode, n, obj, pretty, report_value, s, sha256_bytes, truthy, Controller, Value};

const SCOPE: &str = "Owner connected phase checks: controlled Messages replies, actual core file work and lifecycle transitions.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = twin::create_scratch(&project, "hosted-phases");
    let root = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    std::fs::write(scratch.join("evidence.txt"), "Actual supplied file evidence.")
        .unwrap_or_else(|error| panic!("could not write evidence.txt: {error}"));
    let mut controller = Controller::spawn(&project_text);
    let passed = run(&mut controller, &root, &scratch);
    controller.finish();
    let names = Value::Arr(passed.iter().map(|name| s(name)).collect());
    report_value("hosted_phases", &obj(vec![("passed".into(), names.clone())]), true);
    let printed = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), names),
        ("scope".into(), s(SCOPE)),
    ]);
    println!("{}", pretty(&printed));
    std::fs::remove_dir_all(&scratch).unwrap_or_else(|error| panic!("could not remove scratch: {error}"));
}

fn run(controller: &mut Controller, root: &str, scratch: &Path) -> Vec<String> {
    let mut passed = Vec::new();
    let criteria = "REQUIRED-ALPHA\nREQUIRED-BETA";
    let criteria_lines = lines_of(criteria);
    let report_task = "Produce the requested bounded report.";

    let st = fresh_state(controller, root);
    let mut d = event(controller, root, Some(&st), "send", draft("Search the web only after I confirm."));
    check(
        &mut passed,
        "explicit deferral suppresses the configured provider search declaration and keeps normal model requests",
        !searches(&d) && wire_has(&d, "current user request defers web search"),
    );
    d = reply(controller, root, &d, "I will wait for confirmation.", "end_turn", None);
    check(
        &mut passed,
        "ordinary response completes while provider search is deferred",
        not_flag(state_of(&d), "busy") && effect_null(&d),
    );

    let st = fresh_state(controller, root);
    d = event(
        controller,
        root,
        Some(&st),
        "refine",
        obj(vec![("task".into(), s("Refine instructions from observed evidence."))]),
    );
    check(
        &mut passed,
        "baseline retains explicitly enabled provider search",
        searches(&d) && flag(state_of(&d), "request_hosted_search"),
    );
    d = reply(controller, root, &d, "Observed baseline.", "end_turn", None);
    check(
        &mut passed,
        "proposal phase uses captured evidence without a provider search declaration",
        phase_is(state_of(&d), "propose") && !searches(&d) && not_flag(state_of(&d), "request_hosted_search"),
    );
    check(
        &mut passed,
        "proposal prompt states the actual phase reason",
        wire_has(&d, "phase or exhausted operation allowance uses captured evidence only")
            && wire_lacks(&d, "agent has not permitted"),
    );
    check(
        &mut passed,
        "proposal does not leak a provider-search execution instruction",
        wire_lacks(&d, "Use the provider's declared web_search server tool"),
    );
    d = reply(controller, root, &d, "Candidate instructions based on the observed baseline.", "end_turn", None);
    check(
        &mut passed,
        "probe retains explicitly enabled evidence operations",
        phase_is(state_of(&d), "probe") && searches(&d) && flag(state_of(&d), "request_hosted_search"),
    );
    d = reply(controller, root, &d, "Observed probe.", "end_turn", None);
    check(
        &mut passed,
        "composition uses captured evidence without provider search",
        phase_is(state_of(&d), "compose") && !searches(&d) && not_flag(state_of(&d), "request_hosted_search"),
    );
    d = reply(controller, root, &d, "Composed instructions from actual baseline and probe.", "end_turn", None);
    check(
        &mut passed,
        "refinement still reaches its existing adoption review",
        not_flag(state_of(&d), "busy")
            && str_eq(state_of(&d), "dialog", "refinement_review")
            && str_starts(dig(state_of(&d), &["refinement_pending"]), "composition", "Composed instructions"),
    );

    let st = fresh_state(controller, root);
    d = event(
        controller,
        root,
        Some(&st),
        "refine",
        obj(vec![
            ("task".into(), s(report_task)),
            ("response_checks".into(), s(criteria)),
        ]),
    );
    check(
        &mut passed,
        "user checks are captured before the first call and withheld from the model",
        string_list_eq(dig(state_of(&d), &["task_run", "response_checks"]), &criteria_lines) && wire_lacks(&d, "REQUIRED-"),
    );
    d = reply(controller, root, &d, "Partial report REQUIRED-ALPHA.", "end_turn", None);
    check(
        &mut passed,
        "baseline records partial progress without strict completion",
        num_eq(dig(state_of(&d), &["task_run", "baseline_score"]), "passed", 1)
            && not_flag(dig(state_of(&d), &["task_run", "baseline_score"]), "strict")
            && wire_has(&d, "1/2 required phrases"),
    );
    d = reply(controller, root, &d, "Candidate rules for the bounded report.", "end_turn", None);
    d = reply(controller, root, &d, "Completed report REQUIRED-ALPHA REQUIRED-BETA.", "end_turn", None);
    check(
        &mut passed,
        "probe records a strict result independently of the model wording",
        flag(dig(state_of(&d), &["task_run", "probe_score"]), "strict")
            && num_eq(dig(state_of(&d), &["task_run", "probe_score"]), "passed", 2),
    );
    d = reply(controller, root, &d, "Revision rules for the bounded report.", "end_turn", None);
    check(
        &mut passed,
        "exact composed rules run in a new owned replay before review",
        phase_is(state_of(&d), "verify")
            && get_none(state_of(&d), "refinement_pending")
            && wire_has(&d, "Revision rules for the bounded report.")
            && wire_lacks(&d, "Candidate rules for the bounded report.")
            && wire_lacks(&d, "REQUIRED-"),
    );
    d = reply(controller, root, &d, "Revised report missed the second phrase. REQUIRED-ALPHA.", "end_turn", None);
    let candidate = dig(state_of(&d), &["refinement_pending"]);
    check(
        &mut passed,
        "failed revision retains the earlier exact passing instructions",
        flag(candidate, "retained_probe")
            && not_flag(dig(candidate, &["replay_score"]), "strict")
            && str_eq(candidate, "composition", "Candidate rules for the bounded report.")
            && str_eq(candidate, "rejected_revision", "Revision rules for the bounded report."),
    );
    check(
        &mut passed,
        "outcome comparison is visible and original remains active before approval",
        modal_has(&d, "1/2 required phrases") && len_of(state_of(&d), "agents") == 1,
    );
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "explicit adoption retains the original and records the measured gain",
        str_eq(at(state_of(&adopted), "agents", 1), "instructions", "Candidate rules for the bounded report.")
            && lesson_verified(state_of(&adopted)),
    );
    let again = event(
        controller,
        root,
        Some(state_of(&adopted)),
        "refine",
        obj(vec![("task".into(), s("Another bounded report."))]),
    );
    check(
        &mut passed,
        "a later refinement reuses the adopted measured lesson as historical data",
        text_of(arr_first(dig(state_of(&again), &["task_run", "lesson_context"]), "items"), "provenance")
            .starts_with("user-adopted profile; retained instructions met"),
    );
    let _ = event(controller, root, Some(state_of(&again)), "cancel", blank());

    let st = fresh_state(controller, root);
    d = event(
        controller,
        root,
        Some(&st),
        "refine",
        obj(vec![
            ("task".into(), s(report_task)),
            ("response_checks".into(), s(criteria)),
        ]),
    );
    d = replies(
        controller,
        root,
        d,
        &[
            "Baseline with no required text.",
            "Candidate rules.",
            "Partial REQUIRED-ALPHA.",
            "Revised rules.",
            "<think>REQUIRED-ALPHA REQUIRED-BETA</think>Partial REQUIRED-ALPHA.",
        ],
    );
    check(
        &mut passed,
        "hidden reasoning cannot satisfy a response check",
        num_eq(dig(state_of(&d), &["refinement_pending", "replay_score"]), "passed", 1)
            && not_flag(dig(state_of(&d), &["refinement_pending", "replay_score"]), "strict"),
    );
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    let again = event(
        controller,
        root,
        Some(state_of(&adopted)),
        "refine",
        obj(vec![("task".into(), s("Another bounded report."))]),
    );
    check(
        &mut passed,
        "an explicitly saved partial candidate does not enter the next learning context",
        !lesson_verified(state_of(&adopted))
            && not_flag(dig(state_of(&again), &["task_run", "lesson_context"]), "items"),
    );
    let _ = event(controller, root, Some(state_of(&again)), "cancel", blank());

    let training = "Produce a short training report.";
    let evaluation = "Separate evaluation question, outside the rewrite.";
    let expected = "EVALUATION_EXPECTED_SECRET\nexact second line";
    let expected_line = lines_of(expected).into_iter().next().unwrap_or_else(|| panic!("expected text has no line"));
    let options = obj(vec![
        ("task".into(), s(training)),
        ("evaluation_task".into(), s(evaluation)),
        ("evaluation_method".into(), s("equals")),
        ("evaluation_checks".into(), s(expected)),
    ]);
    let task_state = fresh_state(controller, root);
    let task_turn = event(controller, root, Some(&task_state), "task", blank());
    let task_ui = dig(&task_turn, &["ui", "modal"]).clone();
    check(
        &mut passed,
        "the task modal exposes the separate task, comparison method and local expected output",
        keys_cover(&task_ui, &["evaluation_task", "evaluation_method", "evaluation_checks"]),
    );
    let st = fresh_state(controller, root);
    let bad = event(
        controller,
        root,
        Some(&st),
        "refine",
        with_fields(&options, vec![("evaluation_task".into(), s(training))]),
    );
    check(
        &mut passed,
        "reusing the training task is refused before any request",
        effect_null(&bad) && flag(state_of(&bad), "notice_error"),
    );
    let st = fresh_state(controller, root);
    let bad = event(
        controller,
        root,
        Some(&st),
        "refine",
        with_fields(
            &options,
            vec![
                ("evaluation_method".into(), s("sha256")),
                ("evaluation_checks".into(), s("invalid")),
            ],
        ),
    );
    check(
        &mut passed,
        "invalid SHA-256 input is refused before any request",
        effect_null(&bad) && flag(state_of(&bad), "notice_error"),
    );

    d = start_evaluation(controller, root, &options, evaluation, &expected_line);
    check(
        &mut passed,
        "only after the rewrite does the separate task reach the original instructions",
        phase_is(state_of(&d), "evaluation_base")
            && wire_has(&d, evaluation)
            && wire_has(&d, "ORIGINAL_RULE_MARKER")
            && wire_lacks(&d, "RETAINED_RULE_MARKER"),
    );
    check(
        &mut passed,
        "the evaluation omits the verifier, prior task and tool declarations",
        wire_lacks(&d, &expected_line)
            && wire_lacks(&d, training)
            && wire_lacks(&d, "Training probe answer.")
            && body_lacks_tools(&d)
            && not_flag(effect(&d), "stream"),
    );
    let chat_before = chat_snapshot(state_of(&d));
    let stale = event(
        controller,
        root,
        Some(state_of(&d)),
        "failure",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), s("stale")),
            ("error".into(), s("stale")),
        ]),
    );
    check(
        &mut passed,
        "stale callback cannot score or settle the current evaluation",
        effect_null(&stale)
            && phase_is(state_of(&stale), "evaluation_base")
            && field(state_of(&stale), "model_expected") == field(state_of(&d), "model_expected"),
    );
    d = reply(controller, root, &d, &hammer_text(), "end_turn", None);
    check(
        &mut passed,
        "ordinary JSON can be evaluated without invoking a tool",
        str_eq(effect(&d), "purpose", "model")
            && phase_is(state_of(&d), "evaluation_candidate")
            && not_get(state_of(&d), "pending")
            && not_flag(dig(state_of(&d), &["task_run", "evaluation_base_score"]), "strict"),
    );
    check(
        &mut passed,
        "the retained instructions use the same task with no first answer or expected output",
        wire_has(&d, evaluation)
            && wire_has(&d, "RETAINED_RULE_MARKER")
            && wire_lacks(&d, "ORIGINAL_RULE_MARKER")
            && wire_lacks(&d, "Ordinary structured answer")
            && wire_lacks(&d, &expected_line)
            && body_lacks_tools(&d),
    );
    d = reply(controller, root, &d, expected, "end_turn", None);
    let held = dig(state_of(&d), &["refinement_pending"]);
    check(
        &mut passed,
        "exact comparison passes with visible matching multiline text",
        flag(dig(held, &["evaluation_candidate_score"]), "strict"),
    );
    check(
        &mut passed,
        "evaluation does not retain the task, answer or verifier in the candidate",
        get_none(held, "evaluation") && !dumps_contains(held, &expected_line) && !dumps_contains(held, evaluation),
    );
    check(
        &mut passed,
        "evaluation does not append answers to ordinary chat",
        same_snapshot(state_of(&d), &chat_before),
    );
    check(
        &mut passed,
        "the native review exposes both comparison outcomes",
        modal_has(&d, "0/1 exact text checks") && modal_has(&d, "1/1 exact text checks"),
    );
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "separate evaluation gain becomes a lesson only after explicit adoption",
        num_eq(last(state_of(&adopted), "lessons"), "schema", 3)
            && lesson_verified(state_of(&adopted))
            && len_of(state_of(&adopted), "agents") == 2,
    );

    let proposals = [
        write_proposal(expected),
        search_proposal(expected),
        memory_proposal(expected),
    ];
    for proposal in proposals {
        let shaped = with_fields(
            &options,
            vec![
                ("evaluation_method".into(), s("phrases")),
                ("evaluation_checks".into(), s(&expected_line)),
            ],
        );
        d = start_evaluation(controller, root, &shaped, evaluation, &expected_line);
        d = reply(controller, root, &d, "Original answer.", "end_turn", None);
        d = reply(controller, root, &d, &proposal, "end_turn", None);
        let held = dig(state_of(&d), &["refinement_pending"]);
        check(
            &mut passed,
            "operation-shaped evaluation output cannot score, execute, or stage review",
            effect_null(&d)
                && not_get(state_of(&d), "pending")
                && not_get(held, "evaluation_candidate_score")
                && str_has(held, "evaluation_error", "proposed operation or report")
                && !scratch.join("should-not-exist").exists(),
        );
        let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
        check(
            &mut passed,
            "operation-shaped output cannot promote a lesson",
            !lesson_verified(state_of(&adopted)),
        );
    }

    let hash_value = sha256_bytes(expected.as_bytes()).to_ascii_uppercase();
    d = start_evaluation(
        controller,
        root,
        &with_fields(
            &options,
            vec![
                ("evaluation_method".into(), s("sha256")),
                ("evaluation_checks".into(), s(&hash_value)),
            ],
        ),
        evaluation,
        &expected_line,
    );
    d = reply(controller, root, &d, "Original answer.", "end_turn", None);
    check(
        &mut passed,
        "expected digest stays local in both requests",
        wire_lacks(&d, &hash_value),
    );
    d = reply(
        controller,
        root,
        &d,
        &format!("<think>{expected}</think>Visible answer differs."),
        "end_turn",
        None,
    );
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "hidden reasoning cannot satisfy a digest check or promote the lesson",
        !lesson_verified(state_of(&adopted)),
    );
    let again = event(
        controller,
        root,
        Some(state_of(&adopted)),
        "refine",
        obj(vec![("task".into(), s("Another task."))]),
    );
    check(
        &mut passed,
        "ineligible evaluation does not enter later learning prompts",
        not_flag(dig(state_of(&again), &["task_run", "lesson_context"]), "items"),
    );
    let _ = event(controller, root, Some(state_of(&again)), "cancel", blank());

    d = start_evaluation(controller, root, &options, evaluation, &expected_line);
    d = reply(controller, root, &d, "Original answer.", "end_turn", None);
    let ticket = field(state_of(&d), "model_expected").clone();
    d = event(
        controller,
        root,
        Some(state_of(&d)),
        "failure",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("error".into(), s("fixture connection lost")),
            ("sent".into(), b(true)),
        ]),
    );
    check(
        &mut passed,
        "evaluation transport failure preserves a candidate and reports an incomplete evaluation",
        effect_null(&d)
            && str_eq(state_of(&d), "dialog", "refinement_review")
            && str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "RETAINED_RULE_MARKER")
            && modal_has(&d, "Evaluation incomplete"),
    );

    d = start_evaluation(controller, root, &options, evaluation, &expected_line);
    d = event(controller, root, Some(state_of(&d)), "cancel", blank());
    check(
        &mut passed,
        "cancelling evaluation retains the candidate and clears the callback ticket",
        str_eq(effect(&d), "kind", "cancel")
            && str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "RETAINED_RULE_MARKER")
            && get_none(state_of(&d), "model_expected"),
    );

    d = start_evaluation(controller, root, &options, evaluation, &expected_line);
    d = event(controller, root, Some(state_of(&d)), "resume", blank());
    check(
        &mut passed,
        "resuming after interruption retains the candidate without replay",
        not_get(state_of(&d), "model_expected")
            && str_eq(state_of(&d), "dialog", "refinement_review")
            && str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "RETAINED_RULE_MARKER")
            && resume_without_network(&d),
    );

    d = start_evaluation(controller, root, &options, evaluation, &expected_line);
    put(
        state_mut(&mut d),
        "instructions",
        s("Changed global instructions before the second observation."),
    );
    d = reply(controller, root, &d, "Original answer.", "end_turn", None);
    d = reply(controller, root, &d, expected, "end_turn", None);
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "changed evaluation context prevents classifying the two outcomes as a gain",
        !lesson_verified(state_of(&adopted)),
    );

    let structured = with_fields(
        &options,
        vec![
            ("evaluation_method".into(), s("json")),
            ("evaluation_checks".into(), s("{\"secret_expected\":true,\"items\":[1,2]}")),
        ],
    );
    d = start_evaluation(controller, root, &structured, evaluation, &expected_line);
    check(
        &mut passed,
        "the structured expected result stays local",
        wire_lacks(&d, "secret_expected"),
    );
    d = reply(controller, root, &d, "{\"secret_expected\":true,\"items\":[2,1]}", "end_turn", None);
    d = reply(
        controller,
        root,
        &d,
        "{ \"items\": [1, 2], \"secret_expected\": true }",
        "end_turn",
        None,
    );
    let held = dig(state_of(&d), &["refinement_pending"]);
    check(
        &mut passed,
        "JSON verification ignores object layout and preserves array order",
        not_flag(dig(held, &["evaluation_base_score"]), "strict")
            && flag(dig(held, &["evaluation_candidate_score"]), "strict")
            && !dumps_contains(held, "secret_expected"),
    );
    let st = fresh_state(controller, root);
    let bad = event(
        controller,
        root,
        Some(&st),
        "refine",
        with_fields(&structured, vec![("evaluation_checks".into(), s("{invalid"))]),
    );
    check(
        &mut passed,
        "malformed expected JSON fails before a model call",
        effect_null(&bad) && flag(state_of(&bad), "notice_error"),
    );

    let second = "Independent question for the second evaluation.";
    let expected_two = "EVALUATION_SECOND_SECRET";
    let pair = with_fields(
        &options,
        vec![
            ("evaluation_task_2".into(), s(second)),
            ("evaluation_method_2".into(), s("phrases")),
            ("evaluation_checks_2".into(), s(expected_two)),
        ],
    );
    check(
        &mut passed,
        "the native task modal exposes the second local check group",
        keys_cover(&task_ui, &["evaluation_task_2", "evaluation_method_2", "evaluation_checks_2"]),
    );
    let duplicated = format!("  {}  ", evaluation.replace(' ', "  "));
    let st = fresh_state(controller, root);
    let duplicate = event(
        controller,
        root,
        Some(&st),
        "refine",
        with_fields(&pair, vec![("evaluation_task_2".into(), s(&duplicated))]),
    );
    check(
        &mut passed,
        "a duplicate held-out task is rejected before the first model request",
        effect_null(&duplicate) && flag(state_of(&duplicate), "notice_error"),
    );

    d = pair_second(
        controller,
        root,
        &pair,
        evaluation,
        second,
        expected,
        &expected_line,
        expected_two,
        training,
        &mut passed,
    );
    d = reply(controller, root, &d, expected_two, "end_turn", None);
    check(
        &mut passed,
        "second candidate request contains only its own task and retained instructions",
        phase_is(state_of(&d), "evaluation_candidate")
            && wire_has(&d, second)
            && wire_has(&d, "RETAINED_RULE_MARKER")
            && !searches(&d)
            && wire_lacks_all(&d, &[evaluation, &expected_line, expected_two, training]),
    );
    d = reply(controller, root, &d, expected_two, "end_turn", None);
    let held = dig(state_of(&d), &["refinement_pending"]);
    check(
        &mut passed,
        "paired evaluation shows both results and retains only scores",
        modal_has(&d, "Evaluation task 1")
            && modal_has(&d, "Evaluation task 2")
            && flag(dig(held, &["evaluation_first", "retained"]), "strict")
            && flag(dig(held, &["evaluation_candidate_score"]), "strict")
            && dumps_lacks_all(held, &[evaluation, second, &expected_line, expected_two]),
    );
    d = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "both checks passing with a held-out improvement becomes reusable after adoption",
        lesson_verified(state_of(&d)),
    );

    d = pair_second(
        controller,
        root,
        &pair,
        evaluation,
        second,
        expected,
        &expected_line,
        expected_two,
        training,
        &mut passed,
    );
    d = reply(controller, root, &d, expected_two, "end_turn", None);
    d = reply(controller, root, &d, "The second check regressed.", "end_turn", None);
    d = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "a regression on the second task cannot be hidden by a first-task gain",
        !lesson_verified(state_of(&d)),
    );

    d = pair_second(
        controller,
        root,
        &pair,
        evaluation,
        second,
        expected,
        &expected_line,
        expected_two,
        training,
        &mut passed,
    );
    d = event(controller, root, Some(state_of(&d)), "cancel", blank());
    check(
        &mut passed,
        "interrupting the second task preserves candidate and first scores",
        str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "RETAINED_RULE_MARKER")
            && flag(dig(state_of(&d), &["refinement_pending", "evaluation_first", "retained"]), "strict")
            && modal_has(&d, "Evaluation incomplete"),
    );
    d = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "an interrupted second task cannot promote a first-task gain",
        !lesson_verified(state_of(&d)),
    );

    d = pair_second(
        controller,
        root,
        &pair,
        evaluation,
        second,
        expected,
        &expected_line,
        expected_two,
        training,
        &mut passed,
    );
    put(state_mut(&mut d), "instructions", s("New context between the two evaluation tasks."));
    d = reply(controller, root, &d, expected_two, "end_turn", None);
    d = reply(controller, root, &d, expected_two, "end_turn", None);
    d = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "a changed context between evaluation tasks cannot promote a lesson",
        !lesson_verified(state_of(&d)),
    );

    let checks = obj(vec![
        ("/nested/0/a~1b~0c".into(), s("TRAIN_PRIVATE_VALUE")),
        ("/nullable".into(), Value::Null),
    ]);
    let held_checks = obj(vec![
        ("/held/0".into(), s("HELD_PRIVATE_VALUE")),
        ("/nullable".into(), Value::Null),
    ]);
    let field_inputs = obj(vec![
        ("task".into(), s(training)),
        ("response_check_method".into(), s("json_fields")),
        ("response_checks".into(), s(&checks.to_string())),
        ("refinement_rounds".into(), s("2")),
        ("evaluation_task".into(), s(evaluation)),
        ("evaluation_method".into(), s("json_fields")),
        ("evaluation_checks".into(), s(&held_checks.to_string())),
    ]);
    check(
        &mut passed,
        "native form offers JSON field checks in training and held-out tasks",
        keys_cover(&task_ui, &["response_check_method", "evaluation_method", "evaluation_method_2"])
            && dumps_count(&task_ui, "json_fields") == 3,
    );
    for invalid in ["{}", "{\"bad\":0}", "{\"/bad~2escape\":0}", "{\"/repeat\":0,\"/repeat\":1}"] {
        let st = fresh_state(controller, root);
        d = event(
            controller,
            root,
            Some(&st),
            "refine",
            with_fields(&field_inputs, vec![("response_checks".into(), s(invalid))]),
        );
        check(
            &mut passed,
            &format!("malformed field verifier is rejected before dispatch {invalid}"),
            effect_null(&d) && flag(state_of(&d), "notice_error"),
        );
    }
    let private = ["/nested/0/a~1b~0c", "/nullable", "/held/0", "HELD_PRIVATE_VALUE"];
    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "refine", field_inputs.clone());
    ensure(
        wire_lacks_all(&d, &["/nested/0/a~1b~0c", "/nullable", "/held/0", "HELD_PRIVATE_VALUE", "TRAIN_PRIVATE_VALUE", evaluation]),
        "field verifier leaked into the training request",
    );
    d = reply(controller, root, &d, "{\"nested\":[{\"a/b~c\":\"wrong\"}]}", "end_turn", None);
    check(
        &mut passed,
        "missing null and mismatched nested field score zero",
        num_eq(dig(state_of(&d), &["task_run", "baseline_score"]), "passed", 0),
    );
    d = reply(controller, root, &d, "JSON candidate instructions.", "end_turn", None);
    d = reply(controller, root, &d, "{\"nullable\":null,\"extra\":true}", "end_turn", None);
    check(
        &mut passed,
        "optional extra fields do not prevent partial deterministic gain",
        num_eq(dig(state_of(&d), &["task_run", "probe_score"]), "passed", 1) && wire_has(&d, "1/2 JSON field checks"),
    );
    ensure(
        wire_lacks_all(&d, &["/nested/0/a~1b~0c", "/nullable", "/held/0", "HELD_PRIVATE_VALUE", "TRAIN_PRIVATE_VALUE", evaluation]),
        "field verifier leaked into the probe request",
    );
    d = reply(controller, root, &d, "First JSON rewrite.", "end_turn", None);
    d = reply(controller, root, &d, "{\"nullable\":null}", "end_turn", None);
    check(
        &mut passed,
        "partial JSON gain enters another rewrite with measured counts",
        phase_is(state_of(&d), "propose") && wire_has(&d, "First round retained instructions"),
    );
    d = reply(controller, root, &d, "Second JSON candidate.", "end_turn", None);
    d = reply(
        controller,
        root,
        &d,
        "{\"nullable\":null,\"nested\":[{\"a/b~c\":\"wrong\"}]}",
        "end_turn",
        None,
    );
    d = reply(controller, root, &d, "Second JSON rewrite.", "end_turn", None);
    d = reply(
        controller,
        root,
        &d,
        "{\"nullable\":null,\"nested\":[{\"a/b~c\":\"TRAIN_PRIVATE_VALUE\"}],\"extra\":true}",
        "end_turn",
        None,
    );
    check(
        &mut passed,
        "strict checked replay reaches the separate held-out task",
        phase_is(state_of(&d), "evaluation_base")
            && wire_has(&d, evaluation)
            && wire_lacks_all(&d, &["/nested/0/a~1b~0c", "/nullable", "/held/0", "HELD_PRIVATE_VALUE", "TRAIN_PRIVATE_VALUE"]),
    );
    d = reply(controller, root, &d, "{\"held\":[\"wrong\"],\"nullable\":null}", "end_turn", None);
    d = reply(
        controller,
        root,
        &d,
        "{\"held\":[\"HELD_PRIVATE_VALUE\"],\"nullable\":null,\"extra\":1}",
        "end_turn",
        None,
    );
    let held = dig(state_of(&d), &["refinement_pending"]);
    check(
        &mut passed,
        "separate JSON field checks distinguish partial and strict results and clear verifiers",
        num_eq(dig(held, &["evaluation_base_score"]), "passed", 1)
            && flag(dig(held, &["evaluation_candidate_score"]), "strict")
            && get_none(held, "evaluation")
            && get_none(held, "response_checks")
            && dumps_lacks_all(held, &private),
    );
    d = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "strict field gains become reusable after adoption",
        lesson_verified(state_of(&d)),
    );
    let st = fresh_state(controller, root);
    d = event(
        controller,
        root,
        Some(&st),
        "refine",
        with_fields(
            &field_inputs,
            vec![
                ("response_checks".into(), s("{\"/held/00\":null}")),
                ("refinement_rounds".into(), s("1")),
            ],
        ),
    );
    d = reply(controller, root, &d, "{\"held\":[null]}", "end_turn", None);
    check(
        &mut passed,
        "a leading-zero JSON Pointer array index never matches",
        num_eq(dig(state_of(&d), &["task_run", "baseline_score"]), "passed", 0),
    );

    let json_repair = obj(vec![
        ("task".into(), s(training)),
        ("response_check_method".into(), s("json_fields")),
        ("response_checks".into(), s("{\"/ok\":true}")),
        ("evaluation_task".into(), s(evaluation)),
        ("evaluation_method".into(), s("json")),
        ("evaluation_checks".into(), s("{\"result\":true}")),
    ]);
    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "refine", json_repair);
    d = reply(controller, root, &d, "{\"ok\":false}", "end_turn", None);
    d = reply(controller, root, &d, "Candidate JSON instructions.", "end_turn", None);
    d = reply(controller, root, &d, "```JSON\r\n{\"ok\":true}\r\n```", "end_turn", None);
    check(
        &mut passed,
        "a single code wrapper is removed before deterministic training checks",
        flag(dig(state_of(&d), &["task_run", "probe_score"]), "strict")
            && flag(dig(state_of(&d), &["task_run", "probe_score"]), "json_unwrapped")
            && wire_has(&d, "JSON code block unwrapped locally"),
    );
    d = reply(controller, root, &d, "Retained JSON instructions.", "end_turn", None);
    d = reply(controller, root, &d, "{\"ok\":true}", "end_turn", None);
    d = reply(
        controller,
        root,
        &d,
        "Introductory text.\n```json\n{\"result\":true}\n```",
        "end_turn",
        None,
    );
    check(
        &mut passed,
        "prose surrounding a JSON code block is not silently discarded",
        num_eq(dig(state_of(&d), &["task_run", "evaluation_base_score"]), "passed", 0),
    );
    d = reply(controller, root, &d, "```json\n{\"result\":true}\n```", "end_turn", None);
    let held = dig(state_of(&d), &["refinement_pending"]);
    check(
        &mut passed,
        "the held-out verifier reports the local repair and drops the expected answer",
        flag(dig(held, &["evaluation_candidate_score"]), "strict")
            && flag(dig(held, &["evaluation_candidate_score"]), "json_unwrapped")
            && modal_has(&d, "JSON code block unwrapped locally")
            && is_null(held, "evaluation"),
    );

    d = start_evaluation(
        controller,
        root,
        &with_fields(
            &options,
            vec![
                ("evaluation_method".into(), s("json")),
                ("evaluation_checks".into(), s("{\"tool\":\"write\",\"args\":{\"path\":\"forbidden\"}}")),
            ],
        ),
        evaluation,
        &expected_line,
    );
    d = reply(
        controller,
        root,
        &d,
        "```json\n{\"tool\":\"write\",\"args\":{\"path\":\"forbidden\"}}\n```",
        "end_turn",
        None,
    );
    check(
        &mut passed,
        "a wrapped operation cannot execute or score as a held-out JSON answer",
        effect_null(&d)
            && str_has(dig(state_of(&d), &["refinement_pending"]), "evaluation_error", "proposed operation")
            && !scratch.join("forbidden").exists(),
    );

    d = start_evaluation(
        controller,
        root,
        &with_fields(
            &options,
            vec![
                ("evaluation_method".into(), s("json")),
                ("evaluation_checks".into(), s("{\"result\":true}")),
            ],
        ),
        evaluation,
        &expected_line,
    );
    d = reply(
        controller,
        root,
        &d,
        "```json\n{\"result\":true}\n```\n```json\n{\"result\":true}\n```",
        "end_turn",
        None,
    );
    check(
        &mut passed,
        "multiple JSON code blocks never qualify as a repaired value",
        num_eq(dig(state_of(&d), &["task_run", "evaluation_base_score"]), "passed", 0),
    );

    let strict_bits = obj(vec![
        ("task".into(), s(training)),
        ("response_check_method".into(), s("json_fields")),
        ("refinement_rounds".into(), s("2")),
        ("response_checks".into(), s("{\"/first\":true,\"/second\":true,\"/third\":true}")),
    ]);
    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "refine", strict_bits.clone());
    d = replies(
        controller,
        root,
        d,
        &[
            "{\"first\":true}",
            "Initial field instructions.",
            "{\"second\":true,\"third\":true}",
            "Rewritten field instructions.",
            "{\"second\":true,\"third\":true}",
        ],
    );
    check(
        &mut passed,
        "a higher count that drops a previously passing field does not seed another round",
        str_eq(state_of(&d), "dialog", "refinement_review")
            && not_get(dig(state_of(&d), &["refinement_pending"]), "first_refinement"),
    );
    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "refine", strict_bits);
    d = replies(
        controller,
        root,
        d,
        &[
            "{}",
            "Initial field instructions.",
            "{\"first\":true}",
            "First verified instructions.",
            "{\"first\":true}",
            "Second field instructions.",
            "{\"first\":true}",
            "Second rewritten instructions.",
            "{\"second\":true,\"third\":true}",
        ],
    );
    check(
        &mut passed,
        "a higher second-round count that drops an earlier pass retains the first exact instructions",
        str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "First verified instructions.")
            && num_eq(dig(state_of(&d), &["refinement_pending", "selected_score"]), "passed", 1),
    );

    let rounds = obj(vec![
        ("task".into(), s(training)),
        ("refinement_rounds".into(), s("2")),
        ("response_checks".into(), s(criteria)),
    ]);
    let st = fresh_state(controller, root);
    let bad = event(
        controller,
        root,
        Some(&st),
        "refine",
        with_fields(&rounds, vec![("response_checks".into(), s(""))]),
    );
    check(
        &mut passed,
        "a second refinement round requires deterministic checks",
        effect_null(&bad) && flag(state_of(&bad), "notice_error"),
    );

    d = first_round(controller, root, &rounds, Some(&options), evaluation, &expected_line);
    let run = dig(state_of(&d), &["task_run"]);
    check(
        &mut passed,
        "the second proposal starts from the exact better checked attempt",
        str_eq(run, "phase", "propose")
            && str_eq(dig(run, &["first_refinement"]), "instructions", "First probe rules.")
            && wire_has(&d, "First partial answer")
            && wire_has(&d, "First probe rules.")
            && wire_lacks(&d, evaluation),
    );
    check(
        &mut passed,
        "the original comparison and operation allowance remain intact",
        str_eq(run, "baseline", "Original reply.")
            && num_eq(dig(run, &["baseline_score"]), "passed", 0)
            && num_eq(state_of(&d), "steps", 0),
    );
    d = replies(
        controller,
        root,
        d,
        &[
            "Second probe rules.",
            "Second partial REQUIRED-ALPHA.",
            "Second composed rules.",
            "Second successful REQUIRED-ALPHA REQUIRED-BETA.",
        ],
    );
    check(
        &mut passed,
        "a strict second round enters only the captured separate evaluation",
        phase_is(state_of(&d), "evaluation_base")
            && str_eq(dig(state_of(&d), &["task_run"]), "composition", "Second composed rules.")
            && wire_has(&d, evaluation)
            && wire_lacks(&d, &expected_line)
            && wire_lacks(&d, "First partial answer"),
    );
    d = reply(controller, root, &d, "Original evaluation missed the answer.", "end_turn", None);
    check(
        &mut passed,
        "separate evaluation uses the exact second-round instructions",
        wire_has(&d, "Second composed rules.") && wire_lacks(&d, "First probe rules."),
    );
    d = reply(controller, root, &d, expected, "end_turn", None);
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "the strictly improved second round can contribute after adoption",
        lesson_verified(state_of(&adopted))
            && str_eq(at(state_of(&adopted), "agents", 1), "instructions", "Second composed rules."),
    );

    d = first_round(controller, root, &rounds, None, evaluation, &expected_line);
    d = replies(
        controller,
        root,
        d,
        &[
            "Second probe rules.",
            "Second answer missed both.",
            "Second composed rules.",
            "Second replay missed both.",
        ],
    );
    check(
        &mut passed,
        "a regressing second round retains the earlier exact instructions and score",
        effect_null(&d)
            && str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "First probe rules.")
            && num_eq(dig(state_of(&d), &["refinement_pending", "selected_score"]), "passed", 1)
            && modal_has(&d, "first round was retained"),
    );
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "a partial retained candidate cannot contribute a verified lesson",
        !lesson_verified(state_of(&adopted)),
    );

    for action in ["resume", "cancel", "failure"] {
        d = first_round(controller, root, &rounds, None, evaluation, &expected_line);
        let inputs = if action == "failure" {
            let ticket = field(state_of(&d), "model_expected").clone();
            obj(vec![
                ("purpose".into(), s("model")),
                ("model_ticket".into(), ticket),
                ("sent".into(), b(true)),
                ("error".into(), s("Second round interrupted")),
            ])
        } else {
            blank()
        };
        d = event(controller, root, Some(state_of(&d)), action, inputs);
        check(
            &mut passed,
            &format!("interrupting the second round preserves the tested first candidate {action}"),
            str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "First probe rules.")
                && get_none(state_of(&d), "model_expected"),
        );
    }

    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "refine", rounds.clone());
    d = replies(
        controller,
        root,
        d,
        &[
            "Original reply.",
            "First probe rules.",
            "Complete REQUIRED-ALPHA REQUIRED-BETA.",
            "First composed rules.",
            "Complete REQUIRED-ALPHA REQUIRED-BETA.",
        ],
    );
    check(
        &mut passed,
        "first-round strict completion avoids unnecessary second requests",
        effect_null(&d)
            && str_eq(state_of(&d), "dialog", "refinement_review")
            && not_get(dig(state_of(&d), &["refinement_pending"]), "first_refinement"),
    );

    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "refine", rounds.clone());
    d = replies(
        controller,
        root,
        d,
        &[
            "Original REQUIRED-ALPHA REQUIRED-BETA.",
            "First probe rules.",
            "Complete REQUIRED-ALPHA REQUIRED-BETA.",
            "First composed rules.",
            "Replay kept only REQUIRED-ALPHA.",
        ],
    );
    check(
        &mut passed,
        "a strict original baseline cannot cause a passing probe to be replaced by a regression",
        effect_null(&d)
            && str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "First probe rules.")
            && flag(dig(state_of(&d), &["refinement_pending"]), "retained_probe"),
    );

    d = first_round(controller, root, &rounds, None, evaluation, &expected_line);
    d = reply(controller, root, &d, "Second read-only probe instructions.", "end_turn", None);
    d = reply(controller, root, &d, &read_proposal(), "end_turn", None);
    check(
        &mut passed,
        "second-round tools still require the existing review",
        effect_null(&d) && str_eq(state_of(&d), "dialog", "approval"),
    );
    d = event(controller, root, Some(state_of(&d)), "reject", blank());
    check(
        &mut passed,
        "rejecting a proposed operation retains the first tested candidate and its displayed reply",
        str_eq(dig(state_of(&d), &["refinement_pending"]), "composition", "First probe rules.")
            && modal_has(&d, "First partial answer")
            && effect_null(&d),
    );

    let mut plain = fresh_state(controller, root);
    pop_field(obj_at_mut(&mut plain, "profiles", 0), "hosted_search");
    d = event(controller, root, Some(&plain), "refine", rounds.clone());
    write_project(scratch, "Actual changed project instructions after the original request.");
    d = replies(
        controller,
        root,
        d,
        &[
            "Original reply.",
            "Probe rules.",
            "Complete REQUIRED-ALPHA REQUIRED-BETA.",
            "Composed rules.",
            "Complete REQUIRED-ALPHA REQUIRED-BETA.",
        ],
    );
    let contexts_differ = dig(state_of(&d), &["refinement_pending", "baseline_score", "context"])
        != dig(state_of(&d), &["refinement_pending", "replay_score", "context"]);
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "a real change to project instructions prevents a comparable training gain",
        contexts_differ && !lesson_verified(state_of(&adopted)),
    );
    remove_project(scratch);

    d = event(controller, root, Some(&plain), "refine", rounds.clone());
    d = event(controller, root, Some(state_of(&d)), "workspace_permissions", blank());
    let permissions = access_permissions(state_of(&d));
    d = event(controller, root, Some(state_of(&d)), "workspace_access_save", permissions);
    d = replies(
        controller,
        root,
        d,
        &[
            "Original reply.",
            "Probe rules.",
            "Complete REQUIRED-ALPHA REQUIRED-BETA.",
            "Composed rules.",
            "Complete REQUIRED-ALPHA REQUIRED-BETA.",
        ],
    );
    let contexts_differ = dig(state_of(&d), &["refinement_pending", "baseline_score", "context"])
        != dig(state_of(&d), &["refinement_pending", "replay_score", "context"]);
    let adopted = event(controller, root, Some(state_of(&d)), "refinement_adopt", blank());
    check(
        &mut passed,
        "changing workspace access through the native permission action invalidates training comparison",
        contexts_differ && !lesson_verified(state_of(&adopted)),
    );

    let mut plain = fresh_state(controller, root);
    pop_field(obj_at_mut(&mut plain, "profiles", 0), "hosted_search");
    d = event(controller, root, Some(&plain), "refine", rounds);
    d = reply(controller, root, &d, "Original reply.", "end_turn", None);
    write_project(scratch, "A different project context for the probe only.");
    d = reply(controller, root, &d, "Incomparable probe rules.", "end_turn", None);
    remove_project(scratch);
    d = replies(
        controller,
        root,
        d,
        &[
            "Incomparable REQUIRED-ALPHA REQUIRED-BETA.",
            "Comparable replay rules.",
            "Comparable REQUIRED-ALPHA.",
        ],
    );
    check(
        &mut passed,
        "a comparable partial replay outranks an incomparable strict probe",
        str_eq(dig(state_of(&d), &["task_run", "first_refinement"]), "instructions", "Comparable replay rules.")
            && phase_is(state_of(&d), "propose"),
    );
    let _ = event(controller, root, Some(state_of(&d)), "cancel", blank());

    let mut st = fresh_state(controller, root);
    for n in 0..3 {
        let question = format!("Controlled ordinary question {n}");
        d = event(controller, root, Some(&st), "send", draft(&question));
        ensure(searches(&d), "ordinary question did not declare provider search");
        let answer = format!("Observed ordinary answer {n}");
        d = reply(controller, root, &d, &answer, "end_turn", None);
        st = state_of(&d).clone();
    }
    let original = field(active_chat(&st), "messages").clone();
    d = event(controller, root, Some(&st), "compact", blank());
    check(
        &mut passed,
        "actual compaction declares no provider search",
        phase_is(state_of(&d), "compact") && !searches(&d) && not_flag(state_of(&d), "request_hosted_search"),
    );
    check(
        &mut passed,
        "compaction keeps intrinsic method and supplied-evidence-only instructions",
        wire_has(&d, "Investigation is intrinsic") && wire_has(&d, "This phase summarizes supplied evidence"),
    );
    d = reply(controller, root, &d, "Summary of supplied conversation evidence.", "end_turn", None);
    let chat = active_chat(state_of(&d));
    check(
        &mut passed,
        "actual compaction preserves original history and its summary",
        not_flag(state_of(&d), "busy")
            && field(chat, "messages") == &original
            && str_eq(chat, "context_summary", "Summary of supplied conversation evidence."),
    );

    let mut st = fresh_state(controller, root);
    put(obj_at_mut(&mut st, "agents", 0), "tools", s("read"));
    d = event(controller, root, Some(&st), "send", draft("Answer without web permission."));
    check(
        &mut passed,
        "missing web permission keeps provider search disabled",
        !searches(&d) && wire_has(&d, "selected agent has not permitted web_search"),
    );
    d = reply(controller, root, &d, "Answer from supplied context.", "end_turn", None);
    check(
        &mut passed,
        "permission-disabled search still permits an ordinary answer",
        not_flag(state_of(&d), "busy"),
    );

    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "send", draft("Read the actual evidence within this turn."));
    for _ in 0..8 {
        ensure(searches(&d), "read turn lost provider search before the allowance");
        d = reply(controller, root, &d, &read_proposal(), "end_turn", None);
        ensure(
            str_eq(state_of(&d), "dialog", "approval"),
            &format!(
                "read proposal was not reviewed (dialog {})",
                state_of(&d).get("dialog").map(Value::to_string).unwrap_or_else(|| "missing".into())
            ),
        );
        d = event(controller, root, Some(state_of(&d)), "approve", blank());
        let tool_effect = effect(&d).clone();
        let output = run_tool(controller, &tool_effect, root);
        ensure(output == "Actual supplied file evidence.", &format!("read returned {output}"));
        let mut inputs = Vec::new();
        for key in ["purpose", "tool_ticket", "tool_binding", "child_ticket", "child_owner"] {
            if let Some(value) = tool_effect.get(key) {
                inputs.push((key.to_string(), value.clone()));
            }
        }
        inputs.push(("output".into(), s(&output)));
        d = event(controller, root, Some(state_of(&d)), "tool_result", Value::Obj(inputs));
    }
    check(
        &mut passed,
        "eight actual reads suppress provider search at the existing allowance",
        num_eq(state_of(&d), "steps", 8) && !searches(&d) && not_flag(state_of(&d), "request_hosted_search"),
    );
    check(
        &mut passed,
        "exhausted hosted profile cannot substitute local web tools",
        body_lacks_tools(&d)
            && wire_has(&d, "Available operations for this agent: .")
            && wire_has(&d, "Actual supplied file evidence."),
    );
    d = reply(controller, root, &d, "Answer from the eight actual observations.", "end_turn", None);
    check(
        &mut passed,
        "exhausted provider-search turn can finish from actual evidence",
        not_flag(state_of(&d), "busy") && effect_null(&d),
    );
    d = event(controller, root, Some(state_of(&d)), "send", draft("A fresh ordinary question."));
    check(
        &mut passed,
        "fresh user request restores explicitly enabled provider search",
        num_eq(state_of(&d), "steps", 0) && searches(&d),
    );
    let _ = reply(controller, root, &d, "Fresh answer.", "end_turn", None);

    let st = fresh_state(controller, root);
    let mut task = event(controller, root, Some(&st), "task", blank());
    check(
        &mut passed,
        "manual task controls describe standalone execution",
        run_agent_button(field(&task, "ui")) && dumps_contains(field(&task, "ui"), "its answer is saved in this conversation"),
    );
    task = event(
        controller,
        root,
        Some(state_of(&task)),
        "delegate",
        obj(vec![("task".into(), s("Answer this standalone task."))]),
    );
    let blocks = Value::Arr(vec![
        obj(vec![
            ("type".into(), s("thinking")),
            ("thinking".into(), s("CONTROLLED_HIDDEN_THINKING")),
            ("signature".into(), s("exact-signature")),
        ]),
        obj(vec![
            ("type".into(), s("text")),
            ("text".into(), s("Actual standalone answer.")),
        ]),
    ]);
    task = reply(controller, root, &task, "", "end_turn", Some(blocks.clone()));
    let standalone_state = state_of(&task);
    let last = last_message(active_chat(standalone_state));
    check(
        &mut passed,
        "standalone task result is not represented as a spawned child",
        str_eq(last, "content", "Agent task result:\nActual standalone answer.")
            && not_get(standalone_state, "delegation_stack")
            && str_has(standalone_state, "notice", "Agent task result saved"),
    );
    check(
        &mut passed,
        "new result presentation retains exact hosted protocol context",
        last.get("hosted_content") == Some(&blocks) && ui_hides_thinking(field(&task, "ui")),
    );
    task = event(
        controller,
        root,
        Some(state_of(&task)),
        "send",
        draft("Continue from the actual saved task answer."),
    );
    check(
        &mut passed,
        "same-connection continuation retains original hosted blocks",
        request_has_blocks(&task, &blocks),
    );
    task = reply(controller, root, &task, "Continued from the actual task.", "end_turn", None);
    ensure(not_flag(state_of(&task), "busy"), "standalone continuation stayed busy");

    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "send", draft("Use current provider search if needed."));
    d = reply(controller, root, &d, "Provider paused.", "pause_turn", None);
    check(
        &mut passed,
        "eligible provider pause retains explicit continuation",
        str_eq(state_of(&d), "dialog", "hosted_pause") && not_flag(state_of(&d), "busy"),
    );
    let mut resumed = event(controller, root, Some(state_of(&d)), "hosted_continue", blank());
    check(
        &mut passed,
        "eligible explicit continuation retains its declared server tool",
        effect_present(&resumed) && searches(&resumed),
    );
    resumed = reply(controller, root, &resumed, "Actual continued answer.", "end_turn", None);
    check(
        &mut passed,
        "explicit continuation still settles normally",
        not_flag(state_of(&resumed), "busy"),
    );

    let st = fresh_state(controller, root);
    d = event(controller, root, Some(&st), "send", draft("Bounded search turn."));
    d = reply(controller, root, &d, "Provider paused.", "pause_turn", None);
    let mut changed = state_of(&d).clone();
    put(&mut changed, "steps", n("8"));
    let refused = event(controller, root, Some(&changed), "hosted_continue", blank());
    check(
        &mut passed,
        "exhausted restored pause cannot re-enable provider search",
        effect_null(&refused)
            && flag(state_of(&refused), "notice_error")
            && str_has(state_of(&refused), "notice", "cannot continue provider search"),
    );

    let mut legacy = fresh_state(controller, root);
    put(obj_at_mut(&mut legacy, "profiles", 0), "hosted_search", s("minimax_messages_v1"));
    put(
        obj_at_mut(&mut legacy, "profiles", 0),
        "endpoint",
        s("http://proxy.test/custom-search-route"),
    );
    d = event(controller, root, Some(&legacy), "send", draft("Preserve imported hosted settings."));
    check(
        &mut passed,
        "imported search mode migrates while custom server routes remain usable",
        str_eq(at(state_of(&d), "profiles", 0), "hosted_search", "messages_search_v1")
            && searches(&d)
            && wire_starts(effect(&d), "POST /custom-search-route "),
    );
    d = reply(controller, root, &d, "Preserved hosted pause.", "pause_turn", None);
    let mut legacy = state_of(&d).clone();
    put(obj_at_mut(&mut legacy, "profiles", 0), "hosted_search", s("minimax_messages_v1"));
    {
        let profile = field_mut(field_mut(&mut legacy, "hosted_request"), "profile");
        put(profile, "hosted_search", s("minimax_messages_v1"));
    }
    resumed = event(controller, root, Some(&legacy), "hosted_continue", blank());
    check(
        &mut passed,
        "imported paused continuation preserves captured route and server tools",
        effect_present(&resumed) && searches(&resumed) && wire_starts(effect(&resumed), "POST /custom-search-route "),
    );
    resumed = reply(controller, root, &resumed, "Actual migrated continuation.", "end_turn", None);
    check(
        &mut passed,
        "migrated hosted continuation completes without replay",
        not_flag(state_of(&resumed), "busy") && effect_null(&resumed),
    );
    passed
}

fn start_evaluation(
    controller: &mut Controller,
    root: &str,
    options: &Value,
    evaluation: &str,
    expected_line: &str,
) -> Value {
    let mut st = fresh_state(controller, root);
    put(obj_at_mut(&mut st, "agents", 0), "instructions", s("ORIGINAL_RULE_MARKER"));
    let mut turn = event(controller, root, Some(&st), "refine", options.clone());
    for response in [
        "Training baseline answer.",
        "Training candidate instructions.",
        "Training probe answer.",
        "RETAINED_RULE_MARKER",
    ] {
        ensure(
            wire_lacks_all(&turn, &[evaluation, expected_line]),
            "training wire leaked the held-out evaluation",
        );
        turn = reply(controller, root, &turn, response, "end_turn", None);
    }
    turn
}

fn pair_start(controller: &mut Controller, root: &str, pair: &Value, hidden: &[&str]) -> Value {
    let mut st = fresh_state(controller, root);
    put(obj_at_mut(&mut st, "agents", 0), "instructions", s("ORIGINAL_RULE_MARKER"));
    let mut turn = event(controller, root, Some(&st), "refine", pair.clone());
    for response in [
        "Training baseline answer.",
        "Training candidate instructions.",
        "Training probe answer.",
        "RETAINED_RULE_MARKER",
    ] {
        ensure(wire_lacks_all(&turn, hidden), "paired training wire leaked a held-out value");
        turn = reply(controller, root, &turn, response, "end_turn", None);
    }
    turn
}

fn pair_second(
    controller: &mut Controller,
    root: &str,
    pair: &Value,
    evaluation: &str,
    second: &str,
    expected: &str,
    expected_line: &str,
    expected_two: &str,
    training: &str,
    passed: &mut Vec<String>,
) -> Value {
    let original_one = "Original missed the first check.";
    let mut turn = pair_start(controller, root, pair, &[evaluation, second, expected_line, expected_two]);
    ensure(
        wire_has(&turn, evaluation) && wire_lacks(&turn, second),
        "first paired evaluation did not open on its own task",
    );
    turn = reply(controller, root, &turn, original_one, "end_turn", None);
    ensure(
        wire_has(&turn, evaluation) && wire_lacks(&turn, second),
        "first paired candidate request changed tasks",
    );
    turn = reply(controller, root, &turn, expected, "end_turn", None);
    let state = state_of(&turn);
    check(
        passed,
        "the second task starts with original instructions in its own request",
        str_eq(dig(state, &["task_run"]), "phase", "evaluation_base")
            && get_none(state, "refinement_pending")
            && wire_has(&turn, second)
            && wire_has(&turn, "ORIGINAL_RULE_MARKER")
            && wire_lacks_all(&turn, &[evaluation, expected_line, expected_two, training, original_one]),
    );
    turn
}

fn first_round(
    controller: &mut Controller,
    root: &str,
    rounds: &Value,
    extra: Option<&Value>,
    evaluation: &str,
    expected_line: &str,
) -> Value {
    let inputs = match extra {
        Some(extra) => merge_obj(rounds, extra),
        None => rounds.clone(),
    };
    let st = fresh_state(controller, root);
    let mut turn = event(controller, root, Some(&st), "refine", inputs);
    for response in [
        "Original reply.",
        "First probe rules.",
        "First partial answer REQUIRED-ALPHA.",
        "First composed rules.",
        "First replay missed everything.",
    ] {
        ensure(
            wire_lacks_all(&turn, &[evaluation, expected_line]),
            "refinement wire leaked the held-out evaluation",
        );
        turn = reply(controller, root, &turn, response, "end_turn", None);
    }
    turn
}

fn fresh_state(controller: &mut Controller, root: &str) -> Value {
    let resumed = controller.event(None, "resume", blank(), root);
    let mut st = state_of(&resumed).clone();
    put(
        &mut st,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://provider.test/anthropic/v1/messages")),
            ("protocol".into(), s("messages")),
            ("model".into(), s("controlled")),
            ("plain".into(), b(true)),
            ("stream".into(), b(false)),
            ("hosted_search".into(), s("messages_search_v1")),
            ("tool_format".into(), s("structured")),
        ])]),
    );
    put(
        &mut st,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("read,web_search,web_read")),
        ])]),
    );
    st
}

fn reply(
    controller: &mut Controller,
    root: &str,
    turn: &Value,
    text: &str,
    stop: &str,
    blocks: Option<Value>,
) -> Value {
    let state = state_of(turn);
    let content = match blocks {
        Some(value) if truthy(&value) => value,
        _ => Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])]),
    };
    let payload = obj(vec![
        ("role".into(), s("assistant")),
        ("content".into(), content),
        ("stop_reason".into(), s(stop)),
    ]);
    let encoded = payload.to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{encoded}", encoded.len());
    let ticket = state
        .get("model_expected")
        .cloned()
        .unwrap_or_else(|| panic!("state has no model_expected"));
    controller.event(
        Some(state),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
        root,
    )
}

fn replies(controller: &mut Controller, root: &str, mut turn: Value, texts: &[&str]) -> Value {
    for text in texts {
        turn = reply(controller, root, &turn, text, "end_turn", None);
    }
    turn
}

fn event(controller: &mut Controller, root: &str, state: Option<&Value>, action: &str, inputs: Value) -> Value {
    controller.event(state, action, inputs, root)
}

fn run_tool(controller: &mut Controller, effect: &Value, root: &str) -> String {
    let mut envelope = effect.clone();
    put(&mut envelope, "op", s("tool"));
    put(&mut envelope, "root", s(root));
    match controller.op(&envelope) {
        Value::Str(text) => text,
        other => panic!("tool output is not text: {}", other.to_string()),
    }
}

fn check(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn ensure(ok: bool, message: &str) {
    if !ok {
        panic!("{message}");
    }
}

fn blank() -> Value {
    obj(vec![])
}

fn draft(text: &str) -> Value {
    obj(vec![("draft".into(), s(text))])
}

fn read_proposal() -> String {
    obj(vec![
        ("tool".into(), s("read")),
        ("args".into(), obj(vec![("path".into(), s("evidence.txt"))])),
    ])
    .to_string()
}

fn hammer_text() -> String {
    obj(vec![
        ("tool".into(), s("hammer")),
        ("description".into(), s("Ordinary structured answer")),
    ])
    .to_string()
}

fn write_proposal(expected: &str) -> String {
    obj(vec![
        ("tool".into(), s("write")),
        (
            "args".into(),
            obj(vec![
                ("path".into(), s("should-not-exist")),
                ("content".into(), s(expected)),
            ]),
        ),
    ])
    .to_string()
}

fn search_proposal(expected: &str) -> String {
    format!(
        "Tool request follows:\n{}",
        obj(vec![
            ("tool".into(), s("web_search")),
            ("args".into(), obj(vec![("query".into(), s(expected))])),
        ])
        .to_string()
    )
}

fn memory_proposal(expected: &str) -> String {
    obj(vec![(
        "memory_proposal".into(),
        obj(vec![("observation".into(), s(expected))]),
    )])
    .to_string()
}

fn write_project(scratch: &Path, text: &str) {
    std::fs::write(scratch.join("AGENTS.md"), text).unwrap_or_else(|error| panic!("could not write AGENTS.md: {error}"));
}

fn remove_project(scratch: &Path) {
    std::fs::remove_file(scratch.join("AGENTS.md")).unwrap_or_else(|error| panic!("could not remove AGENTS.md: {error}"));
}

fn access_permissions(state: &Value) -> Value {
    let mut fields = vec![
        ("phone".to_string(), b(true)),
        ("network".to_string(), b(true)),
        ("cloud".to_string(), b(true)),
        ("mcp".to_string(), b(true)),
    ];
    match state.get("storage_access") {
        None => {}
        Some(Value::Obj(extra)) => {
            for (key, value) in extra {
                if let Some(index) = fields.iter().position(|(name, _)| name == key) {
                    fields[index].1 = value.clone();
                } else {
                    fields.push((key.clone(), value.clone()));
                }
            }
        }
        Some(Value::Null) => panic!("storage_access is null"),
        Some(other) => panic!("storage_access is not an object: {}", other.to_string()),
    }
    if let Some(index) = fields.iter().position(|(name, _)| name == "phone") {
        fields[index].1 = b(false);
    }
    Value::Obj(fields)
}

fn state_of(turn: &Value) -> &Value {
    turn.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn state_mut(turn: &mut Value) -> &mut Value {
    field_mut(turn, "state")
}

fn effect(turn: &Value) -> &Value {
    match turn.get("effect") {
        Some(value) if !matches!(value, Value::Null) => value,
        Some(_) => panic!("effect is null"),
        None => panic!("event returned no effect"),
    }
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object when reading {key}");
    };
    let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
    &mut fields[index].1
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

fn pop_field(value: &mut Value, key: &str) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
    fields.remove(index);
}

fn arr_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
    match &mut fields[index].1 {
        Value::Arr(items) => items,
        _ => panic!("{key} is not a list"),
    }
}

fn obj_at_mut<'a>(value: &'a mut Value, key: &str, index: usize) -> &'a mut Value {
    let items = arr_mut(value, key);
    if index >= items.len() {
        panic!("missing {key}[{index}]");
    }
    &mut items[index]
}

fn dig<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    let mut current = value;
    for key in path {
        current = current.get(key).unwrap_or_else(|| panic!("missing {key}"));
    }
    current
}

fn at<'a>(value: &'a Value, key: &str, index: usize) -> &'a Value {
    match field(value, key) {
        Value::Arr(items) => items.get(index).unwrap_or_else(|| panic!("missing {key}[{index}]")),
        other => panic!("{key} is not a list: {}", other.to_string()),
    }
}

fn last<'a>(value: &'a Value, key: &str) -> &'a Value {
    match field(value, key) {
        Value::Arr(items) => items.last().unwrap_or_else(|| panic!("empty {key}")),
        other => panic!("{key} is not a list: {}", other.to_string()),
    }
}

fn arr_first<'a>(value: &'a Value, key: &str) -> &'a Value {
    match field(value, key) {
        Value::Arr(items) => items.first().unwrap_or_else(|| panic!("empty {key}")),
        other => panic!("{key} is not a list: {}", other.to_string()),
    }
}

fn len_of(value: &Value, key: &str) -> usize {
    match field(value, key) {
        Value::Arr(items) => items.len(),
        other => panic!("{key} is not a list: {}", other.to_string()),
    }
}

fn text_of<'a>(value: &'a Value, key: &str) -> &'a str {
    match value.get(key) {
        Some(Value::Str(text)) => text,
        Some(other) => panic!("{key} is not text: {}", other.to_string()),
        None => panic!("missing {key}"),
    }
}

fn str_eq(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn str_has(value: &Value, key: &str, needle: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text.contains(needle))
}

fn str_starts(value: &Value, key: &str, prefix: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text.starts_with(prefix))
}

fn eq_get_str(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn flag(value: &Value, key: &str) -> bool {
    truthy(field(value, key))
}

fn not_flag(value: &Value, key: &str) -> bool {
    !flag(value, key)
}

fn is_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Null))
}

fn get_none(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn not_get(value: &Value, key: &str) -> bool {
    !value.get(key).is_some_and(truthy)
}

fn effect_null(turn: &Value) -> bool {
    is_null(turn, "effect")
}

fn effect_present(turn: &Value) -> bool {
    match turn.get("effect") {
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

fn resume_without_network(turn: &Value) -> bool {
    match turn.get("effect") {
        None => panic!("missing effect"),
        Some(effect) if !truthy(effect) => true,
        Some(effect) => match effect.get("kind") {
            Some(Value::Str(kind)) => kind != "network",
            Some(_) => true,
            None => panic!("missing effect kind"),
        },
    }
}

fn num_eq(value: &Value, key: &str, expect: i64) -> bool {
    match value.get(key) {
        Some(Value::Num(token)) => token.parse::<i64>().ok() == Some(expect),
        _ => false,
    }
}

fn phase_is(state: &Value, phase: &str) -> bool {
    str_eq(dig(state, &["task_run"]), "phase", phase)
}

fn lesson_verified(state: &Value) -> bool {
    flag(last(state, "lessons"), "verified_improvement")
}

fn wire_contains(effect: &Value, needle: &str) -> bool {
    match effect.get("wire") {
        Some(Value::Str(text)) => text.contains(needle),
        Some(Value::Arr(items)) => items.iter().any(|item| matches!(item, Value::Str(text) if text == needle)),
        Some(other) => panic!("wire is not text or a list: {}", other.to_string()),
        None => panic!("effect has no wire"),
    }
}

fn wire_has(turn: &Value, needle: &str) -> bool {
    wire_contains(effect(turn), needle)
}

fn wire_lacks(turn: &Value, needle: &str) -> bool {
    !wire_has(turn, needle)
}

fn wire_lacks_all(turn: &Value, needles: &[&str]) -> bool {
    let wire = effect(turn);
    needles.iter().all(|needle| !wire_contains(wire, needle))
}

fn wire_starts(effect: &Value, prefix: &str) -> bool {
    match effect.get("wire") {
        Some(Value::Str(text)) => text.starts_with(prefix),
        Some(other) => panic!("wire is not text: {}", other.to_string()),
        None => panic!("effect has no wire"),
    }
}

fn request_body(turn: &Value) -> Value {
    let wire = match effect(turn).get("wire") {
        Some(Value::Str(text)) => text,
        Some(other) => panic!("request wire is not text: {}", other.to_string()),
        None => panic!("request has no wire"),
    };
    let body = wire.split_once("\r\n\r\n").unwrap_or_else(|| panic!("request wire has no body")).1;
    Value::parse(body)
}

fn searches(turn: &Value) -> bool {
    match request_body(turn).get("tools") {
        None => false,
        Some(Value::Arr(tools)) => tools.iter().any(|tool| eq_get_str(tool, "name", "web_search")),
        Some(other) => panic!("tools is not a list: {}", other.to_string()),
    }
}

fn body_lacks_tools(turn: &Value) -> bool {
    request_body(turn).get("tools").is_none()
}

fn request_has_blocks(turn: &Value, blocks: &Value) -> bool {
    match request_body(turn).get("messages") {
        Some(Value::Arr(items)) => items
            .iter()
            .any(|message| eq_get_str(message, "role", "assistant") && message.get("content") == Some(blocks)),
        Some(other) => panic!("messages is not a list: {}", other.to_string()),
        None => false,
    }
}

fn dumps_contains(value: &Value, needle: &str) -> bool {
    value.to_string().contains(needle)
}

fn dumps_count(value: &Value, needle: &str) -> usize {
    value.to_string().matches(needle).count()
}

fn dumps_lacks_all(value: &Value, needles: &[&str]) -> bool {
    let text = value.to_string();
    needles.iter().all(|needle| !text.contains(needle))
}

fn modal_has(turn: &Value, needle: &str) -> bool {
    dumps_contains(dig(turn, &["ui", "modal"]), needle)
}

fn lines_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' || bytes[index] == b'\r' {
            out.push(text[start..index].to_string());
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 2;
            } else {
                index += 1;
            }
            start = index;
        } else {
            index += 1;
        }
    }
    if start < text.len() {
        out.push(text[start..].to_string());
    }
    out
}

fn string_list_eq(value: &Value, lines: &[String]) -> bool {
    match value {
        Value::Arr(items) if items.len() == lines.len() => {
            items.iter().zip(lines).all(|(item, line)| matches!(item, Value::Str(text) if text == line))
        }
        _ => false,
    }
}

fn keys_cover(modal: &Value, needed: &[&str]) -> bool {
    let Value::Arr(fields) = field(modal, "fields") else {
        panic!("modal fields are not a list");
    };
    let keys: Vec<&str> = fields
        .iter()
        .map(|item| match item.get("key") {
            Some(Value::Str(text)) => text.as_str(),
            _ => panic!("modal field has no key"),
        })
        .collect();
    needed.iter().all(|key| keys.iter().any(|have| have == key))
}

fn merge_obj(base: &Value, extra: &Value) -> Value {
    let mut out = base.clone();
    let Value::Obj(fields) = extra else {
        panic!("expected object");
    };
    for (key, value) in fields {
        put(&mut out, key, value.clone());
    }
    out
}

fn with_fields(base: &Value, extra: Vec<(String, Value)>) -> Value {
    let mut out = base.clone();
    for (key, value) in extra {
        put(&mut out, &key, value);
    }
    out
}

fn chat_snapshot(state: &Value) -> Vec<(Value, Value)> {
    let mut out = Vec::new();
    let Value::Arr(chats) = field(state, "chats") else {
        panic!("chats is not a list");
    };
    for chat in chats {
        let id = field(chat, "id").clone();
        let messages = field(chat, "messages").clone();
        if let Some(slot) = out.iter_mut().find(|(key, _)| key == &id) {
            slot.1 = messages;
        } else {
            out.push((id, messages));
        }
    }
    out
}

fn same_snapshot(state: &Value, before: &[(Value, Value)]) -> bool {
    let now = chat_snapshot(state);
    now.len() == before.len()
        && before
            .iter()
            .all(|(id, messages)| now.iter().any(|(other, other_messages)| other == id && other_messages == messages))
}

fn active_chat(state: &Value) -> &Value {
    let Value::Arr(chats) = field(state, "chats") else {
        panic!("chats is not a list");
    };
    let active = field(state, "active");
    chats
        .iter()
        .find(|chat| chat.get("id") == Some(active))
        .unwrap_or_else(|| panic!("active chat is missing"))
}

fn last_message(chat: &Value) -> &Value {
    match field(chat, "messages") {
        Value::Arr(items) => items.last().unwrap_or_else(|| panic!("chat has no messages")),
        other => panic!("messages is not a list: {}", other.to_string()),
    }
}

fn run_agent_button(ui: &Value) -> bool {
    let Some(Value::Arr(buttons)) = ui.get("modal").and_then(|modal| modal.get("buttons")) else {
        return false;
    };
    buttons
        .iter()
        .any(|button| eq_get_str(button, "text", "Run agent task") || eq_get_str(button, "label", "Run agent task"))
}

fn ui_hides_thinking(ui: &Value) -> bool {
    match ui.get("messages") {
        None => true,
        Some(Value::Arr(items)) => items.iter().all(|message| match message.get("content") {
            None => true,
            Some(Value::Str(text)) => !text.contains("CONTROLLED_HIDDEN_THINKING"),
            Some(other) => panic!("ui message content is not text: {}", other.to_string()),
        }),
        Some(other) => panic!("ui messages are not a list: {}", other.to_string()),
    }
}

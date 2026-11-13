mod twin;

use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner connected checks: controlled provider responses and actual shared-core command/file operations.";
const EVIDENCE: &str = "Actual evidence from a normal command.";
const STOP_REASON: &str = "Server returned no text response: content_filter";
const SAVED_NOTICE: &str = "Search evidence saved. Asking the same model to answer from those results…";
const REFUSAL: &str = "Search evidence is still in this conversation. The model returned an empty answer after one recovery attempt.";
const EMPTY_STREAM: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\ndata: [DONE]\n\n";
const HIDDEN_CONTRACTS: &[&str] = &[
    "phone_read(path)",
    "phone_create(path,content)",
    "drive_read(path)",
    "drive_create(path,content)",
    "An MCP connection is configured",
];
const SOURCE_OPS: &[&str] = &["read", "write", "web_read", "web_search"];
const READONLY_OPS: &[&str] = &["read", "web_read", "web_search", "phone_read", "drive_read"];
const TOOL_KEYS: &[&str] = &["purpose", "tool_ticket", "tool_binding", "child_ticket", "child_owner"];

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = Scratch::create(&project);
    std::fs::write(scratch.0.join("evidence.txt"), EVIDENCE)
        .unwrap_or_else(|error| panic!("could not write evidence: {error}"));
    let mut controller = Controller::spawn(&project_text);
    let passed = run(&mut controller, scratch.path());
    controller.finish();
    if passed.len() != 86 {
        panic!("emitted {} checks, expected 86", passed.len());
    }
    let names = obj(vec![(
        "passed".into(),
        Value::Arr(passed.iter().map(|name| s(name)).collect()),
    )]);
    // The file is the name list only. stdout is the counted record, so the two
    // channels are written and printed separately.
    twin::report_value("commands", &names, true);
    let written = std::fs::read_to_string("output/checks/commands.json")
        .unwrap_or_else(|error| panic!("output/checks/commands.json: {error}"));
    if written != format!("{}\n", twin::pretty(&names)) {
        panic!("output/checks/commands.json has the wrong shape");
    }
    let record = obj(vec![
        ("passed".into(), twin::n("86")),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    println!("{}", twin::pretty(&record));
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let old_bytes = [114u8, 101, 97];
    let old = std::str::from_utf8(&old_bytes).unwrap_or_else(|error| panic!("legacy token: {error}"));

    let resumed = event(controller, None, "resume", empty(), root);
    let mut st = state_of(&resumed).clone();
    put(
        &mut st,
        old,
        obj(vec![
            ("endpoint".into(), s("http://127.0.0.1:9127/mcp")),
            ("key".into(), s("migration-fixture")),
        ]),
    );
    put(&mut st, &format!("{old}_expected"), s(&format!("{old}_server:expired")));
    put(&mut st, "busy", twin::b(true));
    put(
        &mut st,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s(&format!("read,{old}"))),
            ("instructions".into(), s("User-authored instructions")),
            ("investigation_skill".into(), twin::b(false)),
        ])]),
    );
    put(
        &mut st,
        "investigation_skill",
        obj(vec![
            ("enabled".into(), twin::b(false)),
            ("instructions".into(), s("Preserve my additional guidance")),
        ]),
    );
    let migrated = event(controller, Some(&st), "resume", empty(), root);
    let st = state_of(&migrated).clone();
    let agent = agent_at(&st, 0);
    keep(
        &mut passed,
        "saved connection migrates into protocol settings",
        nested_str(&st, "mcp", "endpoint", "http://127.0.0.1:9127/mcp")
            && nested_str(&st, "mcp", "key", "migration-fixture")
            && st.get(old).is_none(),
    );
    keep(
        &mut passed,
        "saved tool permission migrates without an alias",
        eq_str(agent, "tools", "read,mcp"),
    );
    keep(
        &mut passed,
        "old in-flight owner cannot resume",
        absent(st.get("mcp_client_expected")) && present_falsy(&st, "busy"),
    );
    keep(
        &mut passed,
        "optional preferences and user instructions are preserved",
        nested_str(&st, "investigation_preferences", "instructions", "Preserve my additional guidance")
            && agent.get("investigation_preferences") == Some(&twin::b(false))
            && eq_str(agent, "instructions", "User-authored instructions"),
    );

    let settings = event(controller, Some(&st), "instruction_settings", empty(), root);
    keep(
        &mut passed,
        "preferences are in agent settings",
        any_button(&settings, "investigation_preferences"),
    );
    let skills = event(controller, Some(&st), "skills", empty(), root);
    keep(
        &mut passed,
        "core process is not presented as a selectable skill",
        no_button(&skills, "investigation_preferences"),
    );
    let called = event(controller, Some(&st), "mcp_client_call", empty(), root);
    keep(
        &mut passed,
        "remote call UI uses actual protocol naming",
        modal_title(&called, "MCP tool call"),
    );
    let unknown = event(controller, Some(&st), &format!("{old}_list"), empty(), root);
    keep(&mut passed, "removed action is not an active alias", effect_null(&unknown));
    let listed = event(controller, Some(&st), "mcp_client_list", empty(), root);
    let listed_effect = effect_of(&listed);
    keep(
        &mut passed,
        "migrated connection emits an actual protocol initialization",
        eq_str(listed_effect, "kind", "network")
            && starts_with(listed_effect, "purpose", "mcp_client_init:")
            && py_in(listed_effect.get("wire"), "initialize"),
    );
    event(controller, Some(state_of(&listed)), "cancel", empty(), root);

    let fresh = event(controller, None, "resume", empty(), root);
    let mut command = state_of(&fresh).clone();
    put(
        &mut command,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://model.test/v1/chat/completions")),
            ("protocol".into(), s("chat")),
            ("model".into(), s("controlled")),
            ("plain".into(), twin::b(true)),
            ("stream".into(), twin::b(false)),
        ])]),
    );
    put(
        &mut command,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("read")),
            ("investigation_preferences".into(), twin::b(false)),
        ])]),
    );
    put(
        &mut command,
        "investigation_preferences",
        obj(vec![("enabled".into(), twin::b(false))]),
    );
    put(
        &mut command,
        "inquiry_records",
        obj(vec![("schema".into(), twin::n("999")), ("items".into(), Value::Arr(vec![]))]),
    );
    let mut turn = event(
        controller,
        Some(&command),
        "send",
        draft("Read the evidence file and answer from what it says."),
        root,
    );
    keep(
        &mut passed,
        "ordinary command runs without a peer or journal",
        eq_str(effect_of(&turn), "purpose", "model"),
    );
    let wire = wire_str(effect_of(&turn));
    keep(
        &mut passed,
        "ordinary mode preserves evidence access with additional guidance off",
        wire.contains("Choose the smallest useful response") && !wire.contains("Investigation preferences:"),
    );
    keep(
        &mut passed,
        "no branded workflow or remote activation is required",
        !legacy_marker(wire, old) && !wire.contains("request mcp args"),
    );
    turn = reply(controller, root, &turn, &tool_text("read", &[("path", "evidence.txt")]));
    keep(
        &mut passed,
        "normal evidence operation uses existing review",
        eq_str(state_of(&turn), "dialog", "approval"),
    );
    turn = event(controller, Some(state_of(&turn)), "approve", empty(), root);
    let effect = effect_of(&turn).clone();
    keep(
        &mut passed,
        "normal command dispatches the actual file operation",
        eq_str(&effect, "kind", "tool") && eq_str(&effect, "tool", "read"),
    );
    let actual = controller.op(&tool_envelope(&effect, root));
    let output = match &actual {
        Value::Str(text) => text.clone(),
        // The leg compares the tool reply's value against the evidence text directly, so a
        // non-string reply must fail the check rather than become an invented empty string.
        other => panic!(
            "file operation executes despite unavailable bookkeeping: tool reply was not a string: {}",
            other.to_string()
        ),
    };
    keep(
        &mut passed,
        "file operation executes despite unavailable bookkeeping",
        output == EVIDENCE,
    );
    let inputs = tool_result_inputs(&effect, &actual);
    turn = event(controller, Some(state_of(&turn)), "tool_result", inputs, root);
    let continued_wire = wire_str(effect_of(&turn));
    keep(
        &mut passed,
        "actual evidence feeds directly back into command continuation",
        continued_wire.contains(&output) && continued_wire.contains("Choose the smallest useful response"),
    );
    turn = reply(controller, root, &turn, "The answer follows the actual file observation.");
    keep(
        &mut passed,
        "normal command completes through the existing answer path",
        present_falsy(state_of(&turn), "busy") && effect_null(&turn),
    );

    for fmt in ["chat", "local", "messages", "content"] {
        let opened = event(controller, None, "resume", empty(), root);
        let mut candidate = state_of(&opened).clone();
        put(
            &mut candidate,
            "profiles",
            Value::Arr(vec![obj(vec![
                ("name".into(), s("Controlled")),
                ("protocol".into(), s(fmt)),
                ("endpoint".into(), s("http://model.test/v1/chat/completions")),
                ("model".into(), s("controlled")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
                ("tool_format".into(), s("structured")),
            ])]),
        );
        put(
            &mut candidate,
            "agents",
            Value::Arr(vec![obj(vec![
                ("name".into(), s("General")),
                ("tools".into(), s("read,mcp,delegate")),
                ("subagents".into(), Value::Arr(vec![])),
                ("investigation_preferences".into(), twin::b(false)),
            ])]),
        );
        let outgoing = event(controller, Some(&candidate), "send", draft("Understand the evidence."), root);
        let allowed = enums_of(&request_body(wire_str(effect_of(&outgoing))));
        keep(
            &mut passed,
            &format!("{fmt} structured catalog excludes absent connections and children"),
            enums_are_read(&allowed),
        );
        put(
            &mut candidate,
            "mcp",
            obj(vec![
                ("endpoint".into(), s("http://peer.test/mcp")),
                ("key".into(), s("MCP_SECRET_FIXTURE")),
            ]),
        );
        let outgoing = event(controller, Some(&candidate), "send", draft("Use permitted evidence."), root);
        let allowed = enums_of(&request_body(wire_str(effect_of(&outgoing))));
        keep(
            &mut passed,
            &format!("{fmt} configured peer is explicitly available without leaking credentials"),
            !allowed.is_empty()
                && allowed.iter().all(|item| arr_has_str(item, "mcp") && !arr_has_str(item, "delegate"))
                && !wire_str(effect_of(&outgoing)).contains("MCP_SECRET_FIXTURE"),
        );

        let mut scoped = candidate.clone();
        put_agent(&mut scoped, 0, "tools", s("read,write,web_read,web_search,mcp,phone_read,phone_create,drive_read,drive_create"));
        put(&mut scoped, "phone_access_mode", s("limited"));
        put(&mut scoped, "phone_uri", s("content://fixture/tree/selected"));
        put(
            &mut scoped,
            "drive",
            obj(vec![
                ("url".into(), s("http://nas.test/share/")),
                ("password".into(), s("SOURCE_SECRET_FIXTURE")),
            ]),
        );
        for form in ["structured", "json"] {
            put_profile(&mut scoped, "tool_format", s(form));
            put(&mut scoped, "storage_access", storage(false, false, false));
            let outgoing = event(
                controller,
                Some(&scoped),
                "send",
                draft("Follow the current source access switches."),
                root,
            );
            let wire = wire_str(effect_of(&outgoing));
            let allowed = enums_of(&request_body(wire));
            keep(
                &mut passed,
                &format!("{fmt} {form} disabled source access is reflected in the text catalog"),
                wire.contains("Available operations for this agent: read,write,web_read,web_search.")
                    && HIDDEN_CONTRACTS.iter().all(|item| !wire.contains(item)),
            );
            keep(
                &mut passed,
                &format!("{fmt} {form} structured source catalog agrees with actual access"),
                allowed.iter().all(|item| enum_set(item, SOURCE_OPS)) && (!allowed.is_empty() == (form == "structured")),
            );
            keep(
                &mut passed,
                &format!("{fmt} {form} source revocation preserves adaptive mode and independent web access"),
                wire.contains("Choose the smallest useful response")
                    && wire.contains("web_search args {query}")
                    && !wire.contains("SOURCE_SECRET_FIXTURE")
                    && !wire.contains("MCP_SECRET_FIXTURE"),
            );
            event(controller, Some(state_of(&outgoing)), "cancel", empty(), root);
            put(&mut scoped, "storage_access", storage(true, true, true));
            let outgoing = event(
                controller,
                Some(&scoped),
                "send",
                draft("Use the re-enabled configured sources."),
                root,
            );
            let wire = wire_str(effect_of(&outgoing));
            keep(
                &mut passed,
                &format!("{fmt} {form} re-enabled sources regain their existing contracts"),
                HIDDEN_CONTRACTS.iter().all(|item| wire.contains(item)),
            );
            event(controller, Some(state_of(&outgoing)), "cancel", empty(), root);
        }

        put_profile(&mut scoped, "tool_format", s("structured"));
        put_agent(&mut scoped, 0, "role", s("nonwrite"));
        let outgoing = event(
            controller,
            Some(&scoped),
            "send",
            draft("Inspect the evidence with this read-only role."),
            root,
        );
        let wire = wire_str(effect_of(&outgoing));
        let allowed = enums_of(&request_body(wire));
        let readonly_text = format!("Available operations for this agent: {}.", READONLY_OPS.join(","));
        keep(
            &mut passed,
            &format!("{fmt} non-writing role offers only executable operation families"),
            !allowed.is_empty()
                && allowed.iter().all(|item| enum_set(item, READONLY_OPS))
                && wire.contains(&readonly_text),
        );
        event(controller, Some(state_of(&outgoing)), "cancel", empty(), root);

        put_agent(&mut candidate, 0, "tools", s("read,write,mcp,delegate"));
        put_agent(&mut candidate, 0, "subagents", Value::Arr(vec![twin::n("1")]));
        push_agent(
            &mut candidate,
            obj(vec![
                ("name".into(), s("Child")),
                ("tools".into(), s("read")),
                ("model".into(), s("0")),
            ]),
        );
        let refined = event(
            controller,
            Some(&candidate),
            "refine",
            task("Read evidence and improve the instructions."),
            root,
        );
        let wire = wire_str(effect_of(&refined));
        let allowed = enums_of(&request_body(wire));
        keep(
            &mut passed,
            &format!("{fmt} refinement structured catalog matches executor restrictions"),
            enums_are_read(&allowed),
        );
        keep(
            &mut passed,
            &format!("{fmt} refinement text catalog and child guidance match"),
            wire.contains("Available operations for this agent: read.")
                && !wire.contains("Callable subagents:")
                && !wire.contains("request mcp args"),
        );
        put_profile(&mut candidate, "tool_format", s("json"));
        let refined = event(controller, Some(&candidate), "refine", task("Read the same evidence."), root);
        if !effect_live(&refined) {
            let notice = state_of(&refined).get("notice").map(Value::to_string).unwrap_or_else(|| "missing".into());
            let dialog = state_of(&refined).get("dialog").map(Value::to_string).unwrap_or_else(|| "missing".into());
            panic!("{fmt} refinement has no effect notice={notice} dialog={dialog}");
        }
        let wire = wire_str(effect_of(&refined));
        keep(
            &mut passed,
            &format!("{fmt} text-only refinement has the same real availability"),
            wire.contains("Available operations for this agent: read.") && !wire.contains("Callable subagents:"),
        );
    }

    put_profile(&mut command, "tool_format", s("structured"));
    let mut turn = event(
        controller,
        Some(&command),
        "send",
        draft("Read the evidence within the current operation allowance."),
        root,
    );
    for _attempt in 0..8 {
        turn = reply(controller, root, &turn, &tool_text("read", &[("path", "evidence.txt")]));
        require_approval(&turn);
        turn = event(controller, Some(state_of(&turn)), "approve", empty(), root);
        let effect = effect_of(&turn).clone();
        let actual = controller.op(&tool_envelope(&effect, root));
        if actual != s(&output) {
            panic!("reviewed read returned {}", actual.to_string());
        }
        let inputs = tool_result_inputs(&effect, &actual);
        turn = event(controller, Some(state_of(&turn)), "tool_result", inputs, root);
    }
    keep(
        &mut passed,
        "eight actual reviewed operations reach their existing allowance",
        eq_num(state_of(&turn), "steps", "8") && eq_str(effect_of(&turn), "purpose", "model"),
    );
    let wire = wire_str(effect_of(&turn));
    let request = request_body(wire);
    keep(
        &mut passed,
        "exhausted request offers an evidence-based answer without unusable tools",
        request.get("tools").is_none() && wire.contains("operation allowance is exhausted") && wire.contains(&output),
    );
    let refused = reply(controller, root, &turn, &tool_text("read", &[("path", "evidence.txt")]));
    keep(
        &mut passed,
        "ninth proposal is refused before an impossible review",
        effect_null(&refused)
            && absent(state_of(&refused).get("pending"))
            && dialog_not(state_of(&refused), "approval")
            && state_of(&refused).get("notice_error").is_some_and(twin::truthy),
    );
    let continued = event(
        controller,
        Some(state_of(&refused)),
        "send",
        draft("Answer from the returned evidence."),
        root,
    );
    let wire = wire_str(effect_of(&continued));
    keep(
        &mut passed,
        "a new user instruction restores its ordinary operation allowance",
        eq_num(state_of(&continued), "steps", "0") && request_body(wire).get("tools").is_some(),
    );
    let continued = reply(controller, root, &continued, "Answer from the actual evidence.");
    if !present_falsy(state_of(&continued), "busy") {
        panic!("continued command stayed busy");
    }

    let failed = controller.send(&read_tool(root, "missing.txt"));
    keep(&mut passed, "legitimate missing input stays scoped", failed.get("error").is_some());
    let again = controller.op(&read_tool(root, "evidence.txt"));
    keep(
        &mut passed,
        "unrelated work remains available after scoped failure",
        again == s(&output),
    );

    let opened = event(controller, None, "resume", empty(), root);
    let mut st = state_of(&opened).clone();
    put(
        &mut st,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://127.0.0.1:9/v1/chat/completions")),
            ("protocol".into(), s("chat")),
            ("model".into(), s("controlled")),
            ("plain".into(), twin::b(true)),
            ("hosted_search".into(), s("off")),
        ])]),
    );
    put(
        &mut st,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("web_search")),
            ("investigation_preferences".into(), twin::b(false)),
        ])]),
    );
    put(
        &mut st,
        "investigation_preferences",
        obj(vec![("enabled".into(), twin::b(false))]),
    );
    let mut turn = event(
        controller,
        Some(&st),
        "send",
        draft("Gather the fixture topic from permitted sources."),
        root,
    );
    keep(
        &mut passed,
        "search evidence request streams through the chat provider",
        eq_str(effect_of(&turn), "purpose", "model")
            && effect_of(&turn).get("stream") == Some(&twin::b(true))
            && eq_str(effect_of(&turn), "stream_protocol", "chat"),
    );
    let done = finish_stream(controller, "chat", &search_offer());
    let encoded = done
        .get("value")
        .and_then(|value| value.get("data"))
        .cloned()
        .unwrap_or_else(|| panic!("stream finish has no data: {}", done.to_string()));
    turn = event(
        controller,
        Some(state_of(&turn)),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket_of(&turn)),
            ("data".into(), encoded),
            ("sent".into(), twin::b(true)),
        ]),
        root,
    );
    keep(
        &mut passed,
        "streamed search proposal dispatches without a review dialog",
        effect_live(&turn)
            && eq_str(effect_of(&turn), "purpose", "web_tool")
            && wire_str(effect_of(&turn)).contains("q=fixture%20topic"),
    );
    let web_ticket = effect_of(&turn)
        .get("web_ticket")
        .cloned()
        .unwrap_or_else(|| panic!("web effect has no web_ticket"));
    turn = event(
        controller,
        Some(state_of(&turn)),
        "network_result",
        obj(vec![
            ("purpose".into(), s("web_tool")),
            ("web_ticket".into(), web_ticket),
            ("data".into(), s(&b64_encode(&http_bytes(&search_payload())))),
        ]),
        root,
    );
    keep(
        &mut passed,
        "search evidence reaches the streamed follow-up prompt",
        effect_live(&turn)
            && eq_str(effect_of(&turn), "purpose", "model")
            && effect_of(&turn).get("stream") == Some(&twin::b(true))
            && wire_str(effect_of(&turn)).contains("Search results for: fixture topic")
            && wire_str(effect_of(&turn)).contains("Returned results")
            && matches!(state_of(&turn).get("web_answer_binding"), Some(Value::Obj(_))),
    );
    let protocol = effect_protocol(effect_of(&turn));
    let stopped = finish_stream(controller, &protocol, EMPTY_STREAM);
    let reason = error_text(&stopped);
    keep(
        &mut passed,
        "provider stop reason survives an empty streamed completion",
        reason == STOP_REASON,
    );
    turn = event(
        controller,
        Some(state_of(&turn)),
        "failure",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket_of(&turn)),
            ("error".into(), s(&reason)),
            ("sent".into(), twin::b(true)),
        ]),
        root,
    );
    keep(
        &mut passed,
        "suffixed empty reply asks for an answer from the saved search",
        eq_str(state_of(&turn), "notice", SAVED_NOTICE)
            && effect_live(&turn)
            && eq_str(effect_of(&turn), "purpose", "model")
            && state_of(&turn).get("web_answer_only") == Some(&twin::b(true))
            && wire_str(effect_of(&turn)).contains("Search results for: fixture topic"),
    );
    let follow_protocol = follow_protocol(effect_of(&turn));
    let stopped = finish_stream(controller, &follow_protocol, EMPTY_STREAM);
    let reason = error_text(&stopped);
    keep(
        &mut passed,
        "recovery completion stop reason is produced by the product parser",
        reason == STOP_REASON,
    );
    turn = event(
        controller,
        Some(state_of(&turn)),
        "failure",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket_of(&turn)),
            ("error".into(), s(&reason)),
            ("sent".into(), twin::b(true)),
        ]),
        root,
    );
    keep(
        &mut passed,
        "visible refusal is the one recovery attempt",
        eq_str(state_of(&turn), "notice", REFUSAL)
            && eq_str(ui_of(&turn), "status", REFUSAL)
            && eq_str(ui_of(&turn), "visible_status", REFUSAL)
            && absent(turn.get("effect")),
    );
    passed
}

fn reply(controller: &mut Controller, root: &str, data: &Value, text: &str) -> Value {
    let body = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )])
    .to_string();
    event(
        controller,
        Some(state_of(data)),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket_of(data)),
            ("data".into(), s(&b64_encode(&http_bytes(body.as_bytes())))),
        ]),
        root,
    )
}

fn finish_stream(controller: &mut Controller, protocol: &str, text: &str) -> Value {
    let wire = http_bytes(text.as_bytes());
    let sid = controller.op(&obj(vec![
        ("op".into(), s("stream_open")),
        ("protocol".into(), s(protocol)),
    ]));
    controller.op(&obj(vec![
        ("op".into(), s("stream_feed")),
        ("id".into(), sid.clone()),
        ("data".into(), s(&b64_encode(&wire))),
    ]));
    controller.send(&obj(vec![
        ("op".into(), s("stream_finish")),
        ("id".into(), sid),
        ("eof".into(), twin::b(false)),
    ]))
}

fn search_offer() -> String {
    let proposal = obj(vec![
        ("tool".into(), s("web_search")),
        ("args".into(), obj(vec![("query".into(), s("fixture topic"))])),
    ])
    .to_string();
    let first = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("index".into(), twin::n("0")),
            ("delta".into(), obj(vec![("content".into(), s(&proposal))])),
            ("finish_reason".into(), Value::Null),
        ])]),
    )])
    .to_string();
    let second = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("index".into(), twin::n("0")),
            ("delta".into(), obj(vec![])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )])
    .to_string();
    format!("data: {first}\n\ndata: {second}\n\ndata: [DONE]\n\n")
}

fn search_payload() -> Vec<u8> {
    obj(vec![
        ("query".into(), s("fixture topic")),
        ("number_of_results".into(), twin::n("1")),
        (
            "results".into(),
            Value::Arr(vec![obj(vec![
                ("url".into(), s("https://example.test/page")),
                ("title".into(), s("A title")),
                ("content".into(), s("Snippet")),
                ("engine".into(), s("user")),
            ])]),
        ),
    ])
    .to_string()
    .into_bytes()
}

fn tool_text(tool: &str, args: &[(&str, &str)]) -> String {
    obj(vec![
        ("tool".into(), s(tool)),
        (
            "args".into(),
            obj(args.iter().map(|(key, value)| ((*key).to_string(), s(value))).collect()),
        ),
    ])
    .to_string()
}

fn http_bytes(body: &[u8]) -> Vec<u8> {
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body);
    wire
}

fn tool_envelope(effect: &Value, root: &str) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("tool effect is not an object: {}", effect.to_string());
    };
    let mut fields = fields.clone();
    overlay(&mut fields, "op", s("tool"));
    overlay(&mut fields, "root", s(root));
    Value::Obj(fields)
}

fn tool_result_inputs(effect: &Value, output: &Value) -> Value {
    let mut fields = Vec::new();
    for key in TOOL_KEYS {
        if let Some(value) = effect.get(key) {
            fields.push(((*key).to_string(), value.clone()));
        }
    }
    fields.push(("output".into(), output.clone()));
    Value::Obj(fields)
}

fn read_tool(root: &str, path: &str) -> Value {
    obj(vec![
        ("op".into(), s("tool")),
        ("root".into(), s(root)),
        ("tool".into(), s("read")),
        ("args".into(), obj(vec![("path".into(), s(path))])),
    ])
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state, action, inputs, root)
}

fn empty() -> Value {
    obj(vec![])
}

fn draft(text: &str) -> Value {
    obj(vec![("draft".into(), s(text))])
}

fn task(text: &str) -> Value {
    obj(vec![("task".into(), s(text))])
}

fn storage(phone: bool, network: bool, mcp: bool) -> Value {
    obj(vec![
        ("phone".into(), twin::b(phone)),
        ("network".into(), twin::b(network)),
        ("mcp".into(), twin::b(mcp)),
    ])
}

fn request_body(wire: &str) -> Value {
    let body = wire
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("wire has no header break"));
    Value::parse(body)
}

fn enums_of(value: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    collect_enums(value, &mut out);
    out
}

fn collect_enums(value: &Value, out: &mut Vec<Value>) {
    let children: Vec<&Value> = match value {
        Value::Obj(fields) => fields.iter().map(|(_, item)| item).collect(),
        Value::Arr(items) => items.iter().collect(),
        _ => Vec::new(),
    };
    if let Some(properties) = value.get("properties") {
        if matches!(properties, Value::Obj(_)) {
            if let Some(tool) = properties.get("tool") {
                if matches!(tool, Value::Obj(_)) {
                    if let Some(entries) = tool.get("enum") {
                        out.push(entries.clone());
                    }
                }
            }
        }
    }
    for child in children {
        collect_enums(child, out);
    }
}

fn enums_are_read(allowed: &[Value]) -> bool {
    !allowed.is_empty() && allowed.iter().all(|item| item == &Value::Arr(vec![s("read")]))
}

fn enum_set(item: &Value, expected: &[&str]) -> bool {
    let Value::Arr(values) = item else {
        return false;
    };
    let mut seen = Vec::new();
    for value in values {
        let Value::Str(text) = value else {
            return false;
        };
        if !seen.iter().any(|got: &String| got == text) {
            seen.push(text.clone());
        }
    }
    seen.len() == expected.len() && expected.iter().all(|name| seen.iter().any(|got| got == name))
}

fn arr_has_str(item: &Value, needle: &str) -> bool {
    match item {
        Value::Arr(values) => values.iter().any(|value| matches!(value, Value::Str(text) if text == needle)),
        _ => false,
    }
}

fn legacy_marker(wire: &str, token: &str) -> bool {
    let chars: Vec<char> = wire.chars().collect();
    let token: Vec<char> = token.chars().collect();
    let width = token.len();
    if width == 0 || chars.len() < width {
        return false;
    }
    for index in 0..=chars.len() - width {
        let same = chars[index..index + width]
            .iter()
            .zip(token.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right));
        if !same {
            continue;
        }
        let left_bound = index == 0 || !word_char(chars[index - 1]);
        let after = index + width;
        let right_bound = after == chars.len() || !word_char(chars[after]);
        let underscore = after < chars.len() && chars[after] == '_';
        if left_bound && (right_bound || underscore) {
            return true;
        }
    }
    false
}

fn word_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn py_in(container: Option<&Value>, needle: &str) -> bool {
    match container {
        Some(Value::Str(text)) => text.contains(needle),
        Some(Value::Arr(items)) => items.iter().any(|item| matches!(item, Value::Str(text) if text == needle)),
        _ => false,
    }
}

// The leg reads the protocol two ways: `d['effect']['stream_protocol']` for the first streamed
// request (a missing or non-string protocol is a KeyError there), and
// `follow.get('stream_protocol') or 'chat'` for the follow-up, which keeps the default.
fn effect_protocol(effect: &Value) -> String {
    match effect.get("stream_protocol") {
        Some(Value::Str(text)) => text.clone(),
        other => panic!(
            "provider stop reason survives an empty streamed completion: effect has no string stream_protocol: {}",
            match other {
                Some(value) => value.to_string(),
                None => "absent".to_string(),
            }
        ),
    }
}

fn follow_protocol(effect: &Value) -> String {
    match effect.get("stream_protocol") {
        Some(Value::Str(text)) if twin::truthy(&s(text)) => text.clone(),
        _ => "chat".to_string(),
    }
}

fn error_text(reply: &Value) -> String {
    match reply.get("error") {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    }
}

fn ticket_of(value: &Value) -> Value {
    state_of(value)
        .get("model_expected")
        .cloned()
        .unwrap_or_else(|| panic!("state has no model_expected"))
}

fn require_approval(value: &Value) {
    if eq_str(state_of(value), "dialog", "approval") {
        return;
    }
    let dialog = state_of(value).get("dialog").map(Value::to_string).unwrap_or_else(|| "missing".into());
    let notice = state_of(value).get("notice").map(Value::to_string).unwrap_or_else(|| "missing".into());
    panic!("expected approval dialog, got {dialog} notice {notice}");
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn effect_of(value: &Value) -> &Value {
    value.get("effect").unwrap_or_else(|| panic!("event returned no effect: {}", value.to_string()))
}

fn ui_of(value: &Value) -> &Value {
    value.get("ui").unwrap_or_else(|| panic!("event returned no ui: {}", value.to_string()))
}

fn wire_str(effect: &Value) -> &str {
    match effect.get("wire") {
        Some(Value::Str(text)) => text,
        Some(other) => panic!("effect wire is not a string: {}", other.to_string()),
        None => panic!("effect has no wire: {}", effect.to_string()),
    }
}

fn effect_null(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn effect_live(value: &Value) -> bool {
    value.get("effect").is_some_and(twin::truthy)
}

fn absent(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

fn present_falsy(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(|item| !twin::truthy(item))
}

fn eq_str(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn eq_num(value: &Value, key: &str, number: &str) -> bool {
    value.get(key) == Some(&twin::n(number))
}

fn starts_with(value: &Value, key: &str, prefix: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text.starts_with(prefix))
}

fn nested_str(value: &Value, outer: &str, key: &str, expect: &str) -> bool {
    matches!(value.get(outer).and_then(|item| item.get(key)), Some(Value::Str(text)) if text == expect)
}

fn dialog_not(state: &Value, expect: &str) -> bool {
    match state.get("dialog") {
        Some(Value::Str(text)) => text != expect,
        Some(_) => true,
        None => false,
    }
}

fn any_button(value: &Value, event_name: &str) -> bool {
    buttons(value).is_some_and(|items| items.iter().any(|button| eq_str(button, "event", event_name)))
}

fn no_button(value: &Value, event_name: &str) -> bool {
    buttons(value).is_some_and(|items| items.iter().all(|button| !eq_str(button, "event", event_name)))
}

fn buttons(value: &Value) -> Option<&Vec<Value>> {
    match value.get("ui").and_then(|ui| ui.get("modal")).and_then(|modal| modal.get("buttons")) {
        Some(Value::Arr(items)) => Some(items),
        _ => None,
    }
}

fn modal_title(value: &Value, title: &str) -> bool {
    matches!(
        value.get("ui").and_then(|ui| ui.get("modal")).and_then(|modal| modal.get("title")),
        Some(Value::Str(text)) if text == title
    )
}

fn agent_at(state: &Value, index: usize) -> &Value {
    state
        .get("agents")
        .and_then(|agents| match agents {
            Value::Arr(items) => items.get(index),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing agent {index}"))
}

fn put_agent(state: &mut Value, index: usize, key: &str, next: Value) {
    let agents = arr_mut(state, "agents");
    put(&mut agents[index], key, next);
}

fn push_agent(state: &mut Value, agent: Value) {
    arr_mut(state, "agents").push(agent);
}

fn put_profile(state: &mut Value, key: &str, next: Value) {
    let profiles = arr_mut(state, "profiles");
    put(&mut profiles[0], key, next);
}

fn arr_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
    match &mut fields[index].1 {
        Value::Arr(items) => items,
        _ => panic!("{key} is not an array"),
    }
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    overlay(fields, key, next);
}

fn overlay(fields: &mut Vec<(String, Value)>, key: &str, next: Value) {
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path) -> Self {
        Self(twin::create_scratch(root, "commands"))
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

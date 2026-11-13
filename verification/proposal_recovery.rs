mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{b64_encode, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-event checks across four formats; explicit review, current ownership, examples/ambiguity refusal, actual local HTTP and controlled model continuation. Physical Android and actual user server not exercised.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &root_text, scratch.path(), &peer);
    controller.finish();
    let names: Vec<Value> = passed.iter().map(|name| s(name)).collect();
    twin::report_value(
        "proposal_recovery",
        &obj(vec![
            ("passed".into(), n(&names.len().to_string())),
            ("checks".into(), Value::Arr(names)),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} proposal_recovery checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str, scratch: &str, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    let formats = ["chat", "local", "messages", "content"];
    let proposal = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![("query".into(), s("Tampa, Florida current weather"))]),
        ),
    ]);
    let proposal_text = proposal.to_string();

    for fmt in formats {
        let raw = format!(
            "Observation: the retained search has no current weather reading.\n\nProposal for review:\n{proposal_text}"
        );
        let data = answer(controller, scratch, fmt, &raw);
        write_observation(root, fmt, &data, &raw);
        let display = ui_message(&data, -1);
        let history = message_content(&data, -1);
        keep(
            &mut passed,
            &format!("{fmt} chat keeps prose and raw details stay in history"),
            display.contains(raw.split("\n\n").next().unwrap_or_default())
                && !display.contains(&proposal_text)
                && history == raw,
        );
        keep(
            &mut passed,
            &format!("{fmt} malformed reply itself runs no operation"),
            none(&data, "effect") && none(state(&data), "pending"),
        );

        let shown = event(controller, Some(state(&data)), "chat_actions", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} current reply exposes a direct review control"),
            actions(&shown).contains("format_review"),
        );

        let review = event(controller, Some(state(&shown)), "format_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} explicit recovery opens exact normal review without execution"),
            none(&review, "effect")
                && text_is(state(&review), "dialog", "approval")
                && field(state(&review), "pending").get("args") == proposal.get("args")
                && truthy(state(&review), "agent_pending"),
        );
        keep(
            &mut passed,
            &format!("{fmt} recovered request exposes one-time approval only"),
            !actions(&review).contains("approve_session")
                && !actions(&review).contains("approve_always")
                && actions(&review).contains("Approve once"),
        );

        let matching = event(controller, Some(state(&review)), "approve_matching_rule", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} automatic approval action cannot dispatch recovered work"),
            none(&matching, "effect")
                && field(state(&matching), "pending") == field(state(&review), "pending"),
        );

        let persistence = event(controller, Some(state(&review)), "approve_session", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} recovered request cannot create a persistent approval"),
            none(&persistence, "effect")
                && field(state(&persistence), "pending") == field(state(&review), "pending")
                && state(&persistence).get("approval_rules") == state(&review).get("approval_rules"),
        );

        let approved = event(controller, Some(state(&review)), "approve", obj(vec![]), scratch);
        let approved_effect = effect(&approved);
        keep(
            &mut passed,
            &format!("{fmt} only approval emits the exact search query"),
            approved_effect.get("purpose") == Some(&s("web_tool"))
                && approved_effect.get("wire").map(Value::str_).unwrap_or_default()
                    .contains("Tampa%2C%20Florida%20current%20weather"),
        );
        event(controller, Some(state(&approved)), "cancel", obj(vec![]), scratch);

        let raw = format!(
            "Observed context retained.\n{}",
            obj(vec![
                ("tool".into(), s("web_read")),
                (
                    "args".into(),
                    obj(vec![(
                        "url".into(),
                        s(&format!("http://127.0.0.1:{}/observed", peer.port)),
                    )]),
                ),
            ])
            .to_string()
        );
        let data = answer(controller, scratch, fmt, &raw);
        let before = peer.request_count();
        let review = event(controller, Some(state(&data)), "format_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} reviewing a recovered page performs no network I/O"),
            none(&review, "effect") && peer.request_count() == before,
        );
        let approved = event(controller, Some(state(&review)), "approve", obj(vec![]), scratch);
        let done = request(controller, scratch, &approved);
        let done_effect = effect(&done);
        keep(
            &mut passed,
            &format!("{fmt} actual approved page result returns to requesting model"),
            done_effect.get("purpose") == Some(&s("model"))
                && done_effect.get("wire").map(Value::str_).unwrap_or_default()
                    .contains("Actual separately reviewed page bytes.")
                && peer.request_count() == before + 1,
        );
        reply(controller, scratch, fmt, &done, "Actual page evidence received.");

        let inspected = event(
            controller,
            Some(state(&data)),
            &format!("branch:{}", message_count(&data) - 1), obj(vec![]),
            scratch);
        let modal_fields = inspected
            .get("ui")
            .and_then(|ui| ui.get("modal"))
            .and_then(|modal| modal.get("fields"))
            .map(Value::arr)
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|field| field.get("value"))
            .map(Value::str_)
            .collect::<Vec<_>>()
            .join("\n");
        keep(
            &mut passed,
            &format!("{fmt} selected malformed reply shows response details and review"),
            actions(&inspected).contains("Response details")
                && actions(&inspected).contains("format_review")
                && modal_fields.contains(&raw),
        );

        let cases = [
            (
                "multiple",
                format!("First:\n{proposal_text}\nSecond:\n{proposal_text}"),
            ),
            (
                "fenced",
                format!("Example:\n```json\n{proposal_text}\n```"),
            ),
            ("quoted", format!("Example: `{proposal_text}`")),
            (
                "nested",
                format!(
                    "Configuration: {}",
                    obj(vec![("example".into(), proposal.clone())]).to_string()
                ),
            ),
        ];
        for (label, text) in cases {
            let sample = answer(controller, scratch, fmt, &text);
            let menu = event(controller, Some(state(&sample)), "chat_actions", obj(vec![]), scratch);
            keep(
                &mut passed,
                &format!("{fmt}{label} data or ambiguous requests cannot become recovered review"),
                !actions(&menu).contains("format_review"),
            );
            let blocked = event(controller, Some(state(&sample)), "format_review", obj(vec![]), scratch);
            keep(
                &mut passed,
                &format!("{fmt}{label} direct action still emits no operation"),
                none(&blocked, "effect") && none(state(&blocked), "pending"),
            );
        }

        let data = answer(controller, scratch, fmt, &format!("Proposal:\n{proposal_text}"));
        let mut changed = state(&data).clone();
        let profile = field_mut(&mut changed, "profiles");
        let Value::Arr(profiles) = profile else {
            panic!("profiles is not an array");
        };
        put(&mut profiles[0], "model", s("changed"));
        let blocked = event(controller, Some(&changed), "format_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} changed originating connection cannot recover the request"),
            none(&blocked, "effect")
                && none(state(&blocked), "pending")
                && truthy(state(&blocked), "notice_error"),
        );

        for (label, text) in [
            (
                "reasoning_only",
                format!("<think>{proposal_text}</think>Ordinary answer."),
            ),
            (
                "unfinished_reasoning",
                format!("<think>{proposal_text}"),
            ),
        ] {
            let data = answer(controller, scratch, fmt, &text);
            let menu = event(controller, Some(state(&data)), "chat_actions", obj(vec![]), scratch);
            let embedded = message_field(&data, -1)
                .get("embedded_tool_json")
                .map(Value::bool_)
                .unwrap_or(false);
            keep(
                &mut passed,
                &format!("{fmt}{label} hidden reasoning cannot become a recovered request"),
                !actions(&menu).contains("format_review") && !embedded,
            );
        }

        let text = format!(
            "<think>{}</think>Visible proposal:\n{proposal_text}",
            obj(vec![
                ("tool".into(), s("write")),
                (
                    "args".into(),
                    obj(vec![
                        ("path".into(), s("hidden.txt")),
                        ("content".into(), s("not authorized")),
                    ]),
                ),
            ])
            .to_string()
        );
        let data = answer(controller, scratch, fmt, &text);
        let recovered = event(controller, Some(state(&data)), "format_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} only visible request is staged after leading reasoning"),
            none(&recovered, "effect")
                && field(state(&recovered), "pending").get("tool") == Some(&s("web_search"))
                && !Path::new(scratch).join("hidden.txt").exists(),
        );
        event(controller, Some(state(&recovered)), "reject", obj(vec![]), scratch);

        let mut configured = configured_state(controller, scratch, fmt);
        let agents = field_mut(&mut configured, "agents");
        let Value::Arr(agents) = agents else {
            panic!("agents is not an array");
        };
        put(&mut agents[0], "tools", s("read"));
        let sent_0 = event(
                controller,
                Some(&configured),
                "send",
                obj(vec![("draft".into(), s("Return a response."))]), scratch);
        let data = reply(
            controller,
            scratch,
            fmt,
            &sent_0,
            &format!("Proposal:\n{proposal_text}"));
        let blocked = event(controller, Some(state(&data)), "format_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} disabled tool remains unavailable through request recovery"),
            none(&blocked, "effect") && none(state(&blocked), "pending"),
        );

        let alternative = obj(vec![
            ("tool".into(), s("web_search")),
            (
                "args".into(),
                obj(vec![
                    ("query".into(), s("Tampa, Florida current weather")),
                    ("source".into(), s("alternative")),
                ]),
            ),
        ]);
        let data = answer(controller, scratch, fmt, &format!("Proposal:\n{}", alternative.to_string()));
        let recovered = event(controller, Some(state(&data)), "format_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} alternative source is disclosed by the actual review destination"),
            field(state(&recovered), "pending")
                .get("destination")
                .map(Value::str_)
                .unwrap_or_default()
                .contains("duckduckgo.com"),
        );
        let approved = event(controller, Some(state(&recovered)), "approve", obj(vec![]), scratch);
        let approved_effect = effect(&approved);
        keep(
            &mut passed,
            &format!("{fmt} explicit alternative emits the exact query to the other source once"),
            approved_effect.get("host") == Some(&s("html.duckduckgo.com"))
                && approved_effect.get("wire").map(Value::str_).unwrap_or_default()
                    .contains("Tampa%2C%20Florida%20current%20weather")
                && remaining(state(&approved)).is_empty(),
        );
        event(controller, Some(state(&approved)), "cancel", obj(vec![]), scratch);

        let exact = obj(vec![
            ("tool".into(), s("web_read")),
            (
                "args".into(),
                obj(vec![(
                    "url".into(),
                    s(&format!("http://127.0.0.1:{}/saved-rule", peer.port)),
                )]),
            ),
        ]);
        let ordinary = answer(controller, scratch, fmt, &exact.to_string());
        let grant = event(controller, Some(state(&ordinary)), "approve_session", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} ordinary request still supports an actual saved session approval"),
            effect(&grant).get("purpose") == Some(&s("web_tool")),
        );
        event(controller, Some(state(&grant)), "cancel", obj(vec![]), scratch);

        let control = answer(controller, scratch, fmt, &exact.to_string());
        keep(
            &mut passed,
            &format!("{fmt} saved session approval actually matches the same normal request"),
            effect(&control).get("purpose") == Some(&s("web_tool")),
        );
        event(controller, Some(state(&control)), "cancel", obj(vec![]), scratch);

        let wrapped = answer(
            controller,
            scratch,
            fmt,
            &format!("Review this request:\n{}", exact.to_string()),
        );
        let review = event(controller, Some(state(&wrapped)), "format_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} matching saved rule cannot automatically dispatch recovered prose"),
            none(&review, "effect")
                && field(state(&review), "pending").get("format_review") == Some(&Value::Bool(true)),
        );
        let blocked = event(controller, Some(state(&review)), "approve_matching_rule", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} matching-rule action also preserves the current recovered review"),
            none(&blocked, "effect")
                && field(state(&blocked), "pending") == field(state(&review), "pending"),
        );
        let sent_1 = event(controller, Some(state(&review)), "approve", obj(vec![]), scratch);
        let done = request(controller, scratch, &sent_1);
        keep(
            &mut passed,
            &format!("{fmt} explicit one-time approval still completes actual HTTP under a saved rule"),
            effect(&done).get("purpose") == Some(&s("model")),
        );
        reply(controller, scratch, fmt, &done, "Actual reviewed page received.");
    }

    restart(controller, root);

    let shopping = "I would like to know if you can find any cmp 170hx that have damaged pcb's for sale";
    let item = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![
                ("query".into(), s("CMP 170HX damaged PCB for sale")),
                ("source".into(), s("alternative")),
            ]),
        ),
    ]);
    let prose = "I'll search the web for CMP 170HX devices with damaged PCBs for sale. Let me start by looking for this product.";
    let mut default = configured_state(controller, scratch, "chat");
    remove(&mut default, "web");
    let sent_2 = event(
            controller,
            Some(&default),
            "send",
            obj(vec![("draft".into(), s(shopping))]), scratch);
    let request_started = reply(
        controller,
        scratch,
        "chat",
        &sent_2,
        &format!("{prose}\n{}", item.to_string()));
    let started_effect = effect(&request_started);
    keep(
        &mut passed,
        "the reported shopping wording with wrapped model JSON runs directly on the default setting",
        started_effect.get("purpose") == Some(&s("web_tool"))
            && started_effect.get("wire").map(Value::str_).unwrap_or_default()
                .contains("CMP%20170HX%20damaged%20PCB%20for%20sale")
            && none(state(&request_started), "pending")
            && ui_message(&request_started, -1).contains("Search request recovered"),
    );

    let request_fallback = search_response(controller, scratch, &request_started, 202, "<html>Challenge</html>", "text/html");
    let fallback_effect = effect(&request_fallback);
    keep(
        &mut passed,
        "the default policy also covers a same-query fallback without review",
        fallback_effect.get("host") == Some(&s("api.mwmbl.org"))
            && fallback_effect.get("wire").map(Value::str_).unwrap_or_default()
                .contains("CMP%20170HX%20damaged%20PCB%20for%20sale")
            && none(state(&request_fallback), "pending"),
    );

    let listings = obj(vec![
        ("query".into(), s("CMP 170HX damaged PCB for sale")),
        ("number_of_results".into(), n("1")),
        (
            "results".into(),
            Value::Arr(vec![obj(vec![
                ("url".into(), s("https://example.test/listing")),
                ("title".into(), s("Controlled listing evidence")),
            ])]),
        ),
    ])
    .to_string();
    let continuation = search_response(controller, scratch, &request_fallback, 200, &listings, "application/json");
    keep(
        &mut passed,
        "the recovered search returns parsed evidence to the original model",
        effect(&continuation).get("wire").map(Value::str_).unwrap_or_default()
            .contains("Controlled listing evidence"),
    );

    let followup_item = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![
                ("query".into(), s("CMP 170HX parts only")),
                ("source".into(), s("alternative")),
            ]),
        ),
    ]);
    let followup = reply(controller, scratch, "chat", &continuation, &followup_item.to_string());
    keep(
        &mut passed,
        "the model can run a further search on its own without review",
        effect(&followup).get("purpose") == Some(&s("web_tool"))
            && none(state(&followup), "pending"),
    );
    event(controller, Some(state(&followup)), "cancel", obj(vec![]), scratch);

    for fmt in ["local", "messages", "content"] {
        let mut blank = configured_state(controller, scratch, fmt);
        remove(&mut blank, "web");
        let sent_3 = event(
                controller,
                Some(&blank),
                "send",
                obj(vec![("draft".into(), s(shopping))]), scratch);
        let recovered = reply(
            controller,
            scratch,
            fmt,
            &sent_3,
            &format!("{prose}\n{}", item.to_string()));
        keep(
            &mut passed,
            &format!("{fmt} response with a single unambiguous wrapped search runs by default"),
            effect(&recovered).get("purpose") == Some(&s("web_tool")),
        );
        event(controller, Some(state(&recovered)), "cancel", obj(vec![]), scratch);
    }

    let labels = [
        "Example:\n",
        "Example:\n- ",
        "For illustration only:\n",
        "Here is how the search payload should look:\n",
        "Documentation payload follows:\n",
        "Do not execute the example search below:\n",
        "This is only an example:\n",
        "Here is sample JSON:\n",
        "This is a sample search request:\n",
        "I will not search.\n",
        "I am not going to search.\n",
        "I will show a sample search request.\n",
        "Looking at this example is enough.\n",
    ];
    for label in labels {
        let sent_4 = event(
                controller,
                Some(&default),
                "send",
                obj(vec![(
                    "draft".into(),
                    s("Explain a JSON example without running it."),
                )]), scratch);
        let example = reply(
            controller,
            scratch,
            "chat",
            &sent_4,
            &format!("{label}{}", item.to_string()));
        keep(
            &mut passed,
            &format!(
                "an explicitly labeled example cannot trigger automatic search or correction: {}",
                label.trim()
            ),
            none(&example, "effect") && none(state(&example), "pending"),
        );
    }

    let sent_5 = event(
            controller,
            Some(&default),
            "send",
            obj(vec![("draft".into(), s(shopping))]), scratch);
    let unclear = reply(
        controller,
        scratch,
        "chat",
        &sent_5,
        &format!("Current information would help.\n{}", item.to_string()));
    keep(
        &mut passed,
        "an unclear wrapped proposal asks the model for one correction without sending a search",
        effect(&unclear).get("purpose") == Some(&s("model"))
            && effect(&unclear).get("wire").map(Value::str_).unwrap_or_default()
                .contains("Proposal format review")
            && none(state(&unclear), "pending"),
    );
    let recovered = reply(controller, scratch, "chat", &unclear, &item.to_string());
    keep(
        &mut passed,
        "a clean corrected search still runs without approval",
        effect(&recovered).get("purpose") == Some(&s("web_tool")),
    );
    event(controller, Some(state(&recovered)), "cancel", obj(vec![]), scratch);

    let mut custom = configured_state(controller, scratch, "chat");
    put(
        &mut custom,
        "web",
        obj(vec![(
            "search_url".into(),
            s(&format!(
                "http://127.0.0.1:{}/search?q={{query}}",
                peer.port
            )),
        )]),
    );
    let custom_item = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![("query".into(), s("controlled custom search"))]),
        ),
    ]);
    let sent_6 = event(
            controller,
            Some(&custom),
            "send",
            obj(vec![("draft".into(), s(shopping))]), scratch);
    let custom_reply = reply(
        controller,
        scratch,
        "chat",
        &sent_6,
        &format!("{prose}\n{}", custom_item.to_string()));
    let custom_effect = effect(&custom_reply);
    keep(
        &mut passed,
        "automatic search sends only to the saved custom endpoint",
        custom_effect.get("port") == Some(&n(&peer.port.to_string()))
            && custom_effect.get("wire").map(Value::str_).unwrap_or_default()
                .contains("GET /search?q=controlled%20custom%20search")
            && none(state(&custom_reply), "pending"),
    );
    let custom_done = request(controller, scratch, &custom_reply);
    keep(
        &mut passed,
        "actual custom response returns to the requesting model",
        effect(&custom_done).get("purpose") == Some(&s("model"))
            && effect(&custom_done).get("wire").map(Value::str_).unwrap_or_default()
                .contains("Actual separately reviewed page bytes."),
    );
    reply(controller, scratch, "chat", &custom_done, "Received actual custom evidence.");

    restart(controller, root);
    let mut off = configured_state(controller, scratch, "chat");
    put(
        &mut off,
        "web",
        obj(vec![("search_without_review".into(), Value::Bool(false))]),
    );
    let prompt = "Please search the internet for current public information.";
    let query = "bounded current information";
    let proposal = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![
                ("query".into(), s(query)),
                ("source".into(), s("alternative")),
            ]),
        ),
    ]);
    let sent_7 = event(
            controller,
            Some(&off),
            "send",
            obj(vec![("draft".into(), s(prompt))]), scratch);
    let search = reply(
        controller,
        scratch,
        "chat",
        &sent_7,
        &proposal.to_string());
    keep(
        &mut passed,
        "explicit user request authorizes built-in alternative search with the setting off",
        effect(&search).get("host") == Some(&s("html.duckduckgo.com"))
            && none(state(&search), "pending"),
    );
    keep(
        &mut passed,
        "turn authorization adds no saved approval rule",
        state(&search).get("approval_rules") == off.get("approval_rules"),
    );

    let fallback = search_response(controller, scratch, &search, 202, "<html>source did not return results</html>", "text/html");
    keep(
        &mut passed,
        "HTTP 202 automatically falls back to the independent primary without review",
        effect(&fallback).get("host") == Some(&s("api.mwmbl.org"))
            && none(state(&fallback), "pending")
            && effect(&fallback).get("wire").map(Value::str_).unwrap_or_default()
                .contains("bounded%20current%20information"),
    );

    let body = obj(vec![
        ("query".into(), s(query)),
        ("number_of_results".into(), n("1")),
        (
            "results".into(),
            Value::Arr(vec![obj(vec![
                ("url".into(), s("https://example.test/public")),
                ("title".into(), s("Current source evidence")),
                ("content".into(), s("Verified result fixture")),
                ("engine".into(), s("test")),
            ])]),
        ),
    ])
    .to_string();
    let completed = search_response(controller, scratch, &fallback, 200, &body, "application/json");
    keep(
        &mut passed,
        "the actual parsed fallback result returns to the requesting model",
        effect(&completed).get("purpose") == Some(&s("model"))
            && effect(&completed).get("wire").map(Value::str_).unwrap_or_default()
                .contains("Current source evidence"),
    );

    let revised = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![("query".into(), s("a second focused query"))]),
        ),
    ]);
    let next_search = reply(controller, scratch, "chat", &completed, &revised.to_string());
    keep(
        &mut passed,
        "a second search in the same turn dispatches without another approval",
        effect(&next_search).get("purpose") == Some(&s("web_tool"))
            && effect(&next_search).get("host") == Some(&s("api.mwmbl.org"))
            && none(state(&next_search), "pending"),
    );
    keep(
        &mut passed,
        "rapid repeated requests queue the primary source within the rate bound",
        match effect(&next_search).get("search_delay_ms") {
            Some(Value::Num(token)) => token.parse::<i64>().is_ok_and(|value| value > 0 && value <= 4000),
            _ => false,
        },
    );

    let fallback = search_response(controller, scratch, &next_search, 503, "temporary primary failure", "text/plain");
    let failed = search_response(controller, scratch, &fallback, 202, "<html>no results</html>", "text/html");
    keep(
        &mut passed,
        "both-source failure resumes the same model and hides raw status in ordinary chat",
        effect(&failed).get("purpose") == Some(&s("model"))
            && effect(&failed).get("wire").map(Value::str_).unwrap_or_default().contains("HTTP 202")
            && ui_message(&failed, -1).contains("Tap for details.")
            && !ui_message(&failed, -1).contains("failure_kind"),
    );
    let details = event(
        controller,
        Some(state(&failed)),
        &format!("branch:{}", message_count(&failed) - 1), obj(vec![]),
        scratch);
    keep(
        &mut passed,
        "failure details retain the exact source outcome for inspection",
        actions(&details).contains("HTTP 202"),
    );
    let answer_done = reply(
        controller,
        scratch,
        "chat",
        &failed,
        "The search did not return a verified answer.",
    );
    keep(
        &mut passed,
        "the final reply completes after a scoped search failure",
        !truthy(state(&answer_done), "busy")
            && ui_message(&answer_done, -1).contains("verified answer"),
    );
    let sent_8 = event(
            controller,
            Some(state(&answer_done)),
            "send",
            obj(vec![(
                "draft".into(),
                s("Summarize the notes in this conversation."),
            )]), scratch);
    let neutral = reply(
        controller,
        scratch,
        "chat",
        &sent_8,
        &proposal.to_string());
    keep(
        &mut passed,
        "a fresh unrelated message does not inherit turn authorization",
        none(&neutral, "effect") && text_is(state(&neutral), "dialog", "approval"),
    );
    event(controller, Some(state(&neutral)), "reject", obj(vec![]), scratch);

    let wrapped = format!("I will look for sources.\n{}", proposal.to_string());
    let sent_9 = event(
            controller,
            Some(&off),
            "send",
            obj(vec![(
                "draft".into(),
                s("Could you look up current information online?"),
            )]), scratch);
    let recovered = reply(
        controller,
        scratch,
        "chat",
        &sent_9,
        &wrapped);
    keep(
        &mut passed,
        "one unambiguous wrapped search in an authorized turn runs without a correction or approval round trip",
        effect(&recovered).get("purpose") == Some(&s("web_tool"))
            && ui_message(&recovered, -1).contains("Search request recovered"),
    );
    event(controller, Some(state(&recovered)), "cancel", obj(vec![]), scratch);

    let sent_10 = event(
            controller,
            Some(&off),
            "send",
            obj(vec![(
                "draft".into(),
                s("Search the web for current public information, and don't ask me again."),
            )]), scratch);
    let no_prompt = reply(
        controller,
        scratch,
        "chat",
        &sent_10,
        &proposal.to_string());
    keep(
        &mut passed,
        "a request explicitly declining repeated prompts remains authorized",
        effect(&no_prompt).get("purpose") == Some(&s("web_tool")),
    );
    event(controller, Some(state(&no_prompt)), "cancel", obj(vec![]), scratch);

    for draft in [
        "Search the web.",
        "Please search the internet.",
        "Could you search the web now?",
    ] {
        let sent_11 = event(
                controller,
                Some(&off),
                "send",
                obj(vec![("draft".into(), s(draft))]), scratch);
        let directive = reply(
            controller,
            scratch,
            "chat",
            &sent_11,
            &proposal.to_string());
        keep(
            &mut passed,
            &format!("an explicit follow-up authorizes search without repeating the topic: {draft}"),
            effect(&directive).get("purpose") == Some(&s("web_tool"))
                && none(state(&directive), "pending"),
        );
        event(controller, Some(state(&directive)), "cancel", obj(vec![]), scratch);
    }

    let forged = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![
                ("query".into(), s(query)),
                ("source".into(), s("alternative")),
            ]),
        ),
        ("manual".into(), Value::Bool(true)),
    ]);
    let sent_12 = event(
            controller,
            Some(&off),
            "send",
            obj(vec![("draft".into(), s("Explain this excerpt."))]), scratch);
    let refused = reply(
        controller,
        scratch,
        "chat",
        &sent_12,
        &forged.to_string());
    keep(
        &mut passed,
        "model output cannot claim the manual Search button authority",
        none(&refused, "effect") && text_is(state(&refused), "dialog", "approval"),
    );
    event(controller, Some(state(&refused)), "reject", obj(vec![]), scratch);

    let guarded_drafts = [
        "Please search the local project files for a note about the internet.",
        "Do not search the web.",
        "\"Search the internet\" is an example prompt.",
        "“Search the web”",
        "(\"Search the web\")",
        "Please “search the web”",
        "Search the web is the command label in this example.",
        "Can you search the web?",
        "Search the web for transport standards, but do not make any network requests.",
        "Search the web for transport standards only after I confirm.",
        "Search the web for transport standards, but ask me first.",
        "Search the web if I ask for it later.",
        "Search the web for transport standards, but keep this offline.",
        "Search the web for transport standards, but don't make any network requests.",
    ];
    for draft in guarded_drafts {
        let sent_13 = event(
                controller,
                Some(&off),
                "send",
                obj(vec![("draft".into(), s(draft))]), scratch);
        let guarded = reply(
            controller,
            scratch,
            "chat",
            &sent_13,
            &proposal.to_string());
        keep(
            &mut passed,
            &format!("non-authorizing text keeps the review: {draft}"),
            none(&guarded, "effect") && text_is(state(&guarded), "dialog", "approval"),
        );
        event(controller, Some(state(&guarded)), "reject", obj(vec![]), scratch);
    }

    let configured_search = configured_state(controller, scratch, "chat");
    let setting = event(controller, Some(&configured_search), "web_settings", obj(vec![]), scratch);
    let setting = event(
        controller,
        Some(state(&setting)),
        "web_save",
        obj(vec![("search_without_review".into(), Value::Bool(true))]), scratch);
    let standing = state(&setting).clone();
    keep(
        &mut passed,
        "built-in standing approval survives state restoration",
        field(state(&event(controller, Some(&standing), "resume", obj(vec![]), scratch)), "web")
            .get("search_without_review")
            == Some(&Value::Bool(true)),
    );

    let base_proposal = obj(vec![
        ("tool".into(), s("web_search")),
        (
            "args".into(),
            obj(vec![
                ("query".into(), s("bounded search check")),
                ("source".into(), s("alternative")),
            ]),
        ),
    ]);
    let wrapped = format!("I will search for evidence.\n{}", base_proposal.to_string());
    let deferred_prompts = [
        "Do not search the web.",
        "Search the web only after I confirm.",
        "Please check online, but ask me first.",
        "Keep this offline.",
        "Search the web later.",
        "Hold off on web search.",
        "Please delay web search.",
        "No internet access.",
        "Don't go online.",
        "- Do not search the web.",
        "1. Do not search the web.",
        "Search the web when I confirm.",
        "Search the web once I confirm.",
        "Ask me before searching the web.",
        "Wait for my confirmation before searching the web.",
        "Summarize the developers' notes. Do not search the web.",
    ];
    for draft in deferred_prompts {
        let sent_14 = event(
                controller,
                Some(&standing),
                "send",
                obj(vec![("draft".into(), s(draft))]), scratch);
        let held = reply(
            controller,
            scratch,
            "chat",
            &sent_14,
            &base_proposal.to_string());
        keep(
            &mut passed,
            &format!("an explicit deferral overrides automatic approval: {draft}"),
            none(&held, "effect") && text_is(state(&held), "dialog", "approval"),
        );
        let repeated = event(controller, Some(state(&held)), "approve_matching_rule", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("matching-rule action cannot bypass an explicit deferral: {draft}"),
            none(&repeated, "effect")
                && field(state(&repeated), "pending") == field(state(&held), "pending"),
        );
        event(controller, Some(state(&repeated)), "reject", obj(vec![]), scratch);
    }

    for draft in [
        "Search the web for when I need to renew my passport.",
        "Search the web for how long to wait for the train.",
        "Search the web for no-network deployment patterns.",
    ] {
        let sent_15 = event(
                controller,
                Some(&off),
                "send",
                obj(vec![("draft".into(), s(draft))]), scratch);
        let allowed = reply(
            controller,
            scratch,
            "chat",
            &sent_15,
            &base_proposal.to_string());
        keep(
            &mut passed,
            &format!("topic wording still authorizes a direct search: {draft}"),
            effect(&allowed).get("purpose") == Some(&s("web_tool")),
        );
        event(controller, Some(state(&allowed)), "cancel", obj(vec![]), scratch);
    }

    let sent_16 = event(
            controller,
            Some(&standing),
            "send",
            obj(vec![(
                "draft".into(),
                s("Explain the sentence “Do not search the web.”"),
            )]), scratch);
    let quoted = reply(
        controller,
        scratch,
        "chat",
        &sent_16,
        &base_proposal.to_string());
    keep(
        &mut passed,
        "quoted example does not change the standing setting",
        effect(&quoted).get("purpose") == Some(&s("web_tool")),
    );
    event(controller, Some(state(&quoted)), "cancel", obj(vec![]), scratch);

    let sent_17 = event(
            controller,
            Some(&standing),
            "send",
            obj(vec![("draft".into(), s(deferred_prompts[1]))]), scratch);
    let held = reply(
        controller,
        scratch,
        "chat",
        &sent_17,
        &wrapped);
    let menu = event(controller, Some(state(&held)), "chat_actions", obj(vec![]), scratch);
    keep(
        &mut passed,
        "explicit deferral also suppresses the automatic web format correction",
        none(&held, "effect") && actions(&menu).contains("format_review"),
    );

    let mut scope = configured_state(controller, scratch, "chat");
    let profiles = field_mut(&mut scope, "profiles");
    let Value::Arr(profiles) = profiles else {
        panic!("profiles is not an array");
    };
    put(&mut profiles[0], "name", s("Saved approval control"));
    let sent_18 = event(
            controller,
            Some(&scope),
            "send",
            obj(vec![("draft".into(), s("Answer the ordinary question."))]), scratch);
    let rule = reply(
        controller,
        scratch,
        "chat",
        &sent_18,
        &base_proposal.to_string());
    let rule = event(controller, Some(state(&rule)), "approve_session", obj(vec![]), scratch);
    if effect(&rule).get("purpose") != Some(&s("web_tool")) {
        panic!("saved-session approval did not emit a web tool effect");
    }
    let scope = state(&event(controller, Some(state(&rule)), "cancel", obj(vec![]), scratch)).clone();
    let sent_19 = event(
            controller,
            Some(&scope),
            "send",
            obj(vec![("draft".into(), s("Answer another ordinary question."))]), scratch);
    let allowed = reply(
        controller,
        scratch,
        "chat",
        &sent_19,
        &base_proposal.to_string());
    keep(
        &mut passed,
        "control dispatches under the matching saved session rule",
        effect(&allowed).get("purpose") == Some(&s("web_tool")),
    );
    let scope = state(&event(controller, Some(state(&allowed)), "cancel", obj(vec![]), scratch)).clone();
    let sent_20 = event(
            controller,
            Some(&scope),
            "send",
            obj(vec![("draft".into(), s(deferred_prompts[0]))]), scratch);
    let held = reply(
        controller,
        scratch,
        "chat",
        &sent_20,
        &base_proposal.to_string());
    keep(
        &mut passed,
        "explicit deferral also overrides a matching saved session rule",
        none(&held, "effect") && text_is(state(&held), "dialog", "approval"),
    );
    let authorized = event(controller, Some(state(&held)), "approve", obj(vec![]), scratch);
    keep(
        &mut passed,
        "later explicit approval still authorizes the pending search",
        effect(&authorized).get("purpose") == Some(&s("web_tool")),
    );
    event(controller, Some(state(&authorized)), "cancel", obj(vec![]), scratch);

    let draft = event(
        controller,
        Some(&standing),
        "send",
        obj(vec![("draft".into(), s("Search the web."))]), scratch);
    let preview = event(
        controller,
        Some(state(&draft)),
        "model_progress",
        obj(vec![
            (
                "model_ticket".into(),
                field(state(&draft), "model_expected").clone(),
            ),
            (
                "stream_text".into(),
                s(&wrapped[..wrapped.len() - 5]),
            ),
        ]), scratch);
    keep(
        &mut passed,
        "a partial streamed envelope remains hidden while prose is visible",
        ui_message(&preview, -1).contains("I will search")
            && !ui_message(&preview, -1).contains("\"tool\""),
    );
    let stopped = event(controller, Some(state(&preview)), "cancel", obj(vec![]), scratch);
    keep(
        &mut passed,
        "cancelled stream keeps raw history but hides the request envelope",
        !ui_message(&stopped, -1).contains("\"tool\"")
            && message_content(&stopped, -1) == wrapped[..wrapped.len() - 5],
    );

    let ambiguous = format!(
        "{wrapped}\n{}",
        obj(vec![
            ("tool".into(), s("web_search")),
            (
                "args".into(),
                obj(vec![("query".into(), s("a second proposal"))]),
            ),
        ])
        .to_string()
    );
    let sent_21 = event(
            controller,
            Some(&standing),
            "send",
            obj(vec![(
                "draft".into(),
                s("Investigate the returned evidence."),
            )]), scratch);
    let correction = reply(
        controller,
        scratch,
        "chat",
        &sent_21,
        &ambiguous);
    keep(
        &mut passed,
        "one malformed web proposal gets a model correction only",
        effect(&correction).get("purpose") == Some(&s("model"))
            && none(state(&correction), "pending"),
    );
    let repeated = reply(controller, scratch, "chat", &correction, &ambiguous);
    let repeated_menu = event(controller, Some(state(&repeated)), "chat_actions", obj(vec![]), scratch);
    keep(
        &mut passed,
        "a second ambiguous reply stays in history and stops automatic correction",
        none(&repeated, "effect")
            && message_content(&repeated, -1) == ambiguous
            && !actions(&repeated_menu).contains("format_correction"),
    );
    let sent_22 = event(
            controller,
            Some(&standing),
            "send",
            obj(vec![(
                "draft".into(),
                s("Investigate the returned evidence."),
            )]), scratch);
    let correction = reply(
        controller,
        scratch,
        "chat",
        &sent_22,
        &ambiguous);
    let approved = reply(controller, scratch, "chat", &correction, &base_proposal.to_string());
    keep(
        &mut passed,
        "corrected valid built-in search uses the saved approval",
        effect(&approved).get("purpose") == Some(&s("web_tool"))
            && effect(&approved).get("host") == Some(&s("html.duckduckgo.com")),
    );
    keep(
        &mut passed,
        "valid bare proposal stays in history and its envelope is hidden",
        !ui_message(&approved, -1).contains("\"tool\"")
            && Value::parse(&message_content(&approved, -1)) == base_proposal,
    );
    event(controller, Some(state(&approved)), "cancel", obj(vec![]), scratch);

    let manual = event(controller, None, "resume", obj(vec![]), scratch);
    let manual = event(controller, Some(state(&manual)), "web_search", obj(vec![]), scratch);
    keep(
        &mut passed,
        "manual search exposes the same generic alternative-source choice",
        actions(&manual).contains("Alternative source only"),
    );
    let manual = event(
        controller,
        Some(state(&manual)),
        "web_search_review",
        obj(vec![
            ("query".into(), s("Tampa current weather")),
            ("source".into(), s("alternative")),
        ]), scratch);
    let manual_effect = effect(&manual);
    keep(
        &mut passed,
        "manual search submission authorizes the exact query and source immediately",
        manual_effect.get("host") == Some(&s("html.duckduckgo.com"))
            && manual_effect.get("wire").map(Value::str_).unwrap_or_default()
                .contains("Tampa%20current%20weather"),
    );
    keep(
        &mut passed,
        "manual alternate-only choice keeps its one-source scope",
        remaining(state(&manual)).is_empty(),
    );
    event(controller, Some(state(&manual)), "cancel", obj(vec![]), scratch);

    let mut learned = state(&event(controller, None, "resume", obj(vec![]), scratch)).clone();
    let html = "<div class=\"result\"><a class=\"result__a\" href=\"https://example.test/evidence\">Structured result</a></div>";
    for number in 0..3 {
        thread::sleep(Duration::from_millis(1050));
        let query = format!("Exact comparison {number}");
        let first = manual_search(controller, scratch, &learned, &query);
        if none(&first, "effect") {
            panic!(
                "manual search did not emit an effect: {}",
                state(&first).get("notice").map(Value::to_string).unwrap_or_default()
            );
        }
        keep(
            &mut passed,
            "default order remains stable below the evidence margin",
            effect(&first).get("host") == Some(&s("api.mwmbl.org")),
        );
        let empty = obj(vec![
            ("query".into(), s(&query)),
            ("number_of_results".into(), n("0")),
            ("results".into(), Value::Arr(vec![])),
        ])
        .to_string();
        let second = search_response(controller, scratch, &first, 200, &empty, "application/json");
        keep(
            &mut passed,
            "a valid empty first source does not earn a comparison vote",
            effect(&second).get("host") == Some(&s("html.duckduckgo.com"))
                && number_or(state(&second), "search_preference", 0) == number as i64,
        );
        let returned = search_response(controller, scratch, &second, 200, html, "text/html");
        learned = state(&returned).clone();
        keep(
            &mut passed,
            "a completed fallback votes once only after validated results",
            number_field(&learned, "search_preference") == number as i64 + 1
                && learned.get("output").map(Value::str_).unwrap_or_default()
                    .contains("Structured result"),
        );
        let late = search_response(
            controller,
            scratch,
            &with_state(&second, &learned),
            200,
            html,
            "text/html",
        );
        keep(
            &mut passed,
            "the consumed callback cannot train twice",
            number_field(state(&late), "search_preference") == number as i64 + 1,
        );
    }

    learned = state(&event(controller, Some(&learned), "resume", obj(vec![]), scratch)).clone();
    let ordering = manual_search(controller, scratch, &learned, "Future search after reload");
    keep(
        &mut passed,
        "learned order changes an ordinary search after state restoration",
        effect(&ordering).get("host") == Some(&s("html.duckduckgo.com"))
            && remaining(state(&ordering)) == vec![n("0")],
    );
    let ordering = event(controller, Some(state(&ordering)), "cancel", obj(vec![]), scratch);
    keep(
        &mut passed,
        "cancelled search has no effect on learned order",
        number_field(state(&ordering), "search_preference") == 3,
    );
    let settings = event(controller, Some(state(&ordering)), "web_settings", obj(vec![]), scratch);
    keep(
        &mut passed,
        "settings expose the current default and a working opt-out",
        actions(&settings).contains("automatic order starts with DuckDuckGo"),
    );
    let disabled = event(
        controller,
        Some(state(&settings)),
        "web_save",
        obj(vec![("search_learning".into(), Value::Bool(false))]), scratch);
    thread::sleep(Duration::from_millis(1050));
    let disabled_search = manual_search(controller, scratch, state(&disabled), "Learning paused");
    keep(
        &mut passed,
        "opt-out restores the original order without erasing the saved observations",
        effect(&disabled_search).get("host") == Some(&s("api.mwmbl.org"))
            && number_field(state(&disabled_search), "search_preference") == 3,
    );
    event(controller, Some(state(&disabled_search)), "cancel", obj(vec![]), scratch);
    let saved = event(
        controller,
        Some(state(&settings)),
        "web_save",
        obj(vec![
            ("search_learning".into(), Value::Bool(true)),
            (
                "search_url".into(),
                s("https://api.mwmbl.org/api/v2/search/?q={query}"),
            ),
        ]), scratch);
    thread::sleep(Duration::from_millis(1050));
    let pinned = manual_search(controller, scratch, state(&saved), "Pinned source");
    keep(
        &mut passed,
        "explicitly configured order outranks the learned default",
        effect(&pinned).get("host") == Some(&s("api.mwmbl.org"))
            && field(
                field(field(state(&pinned), "web_expected"), "search_chain"),
                "adaptive",
            ) == &Value::Bool(false),
    );
    event(controller, Some(state(&pinned)), "cancel", obj(vec![]), scratch);

    let query = "Comparison after learning";
    let first = manual_search(controller, scratch, &learned, query);
    thread::sleep(Duration::from_millis(1050));
    let fallback = event(
        controller,
        Some(state(&first)),
        "failure",
        obj(vec![
            ("purpose".into(), s("web_tool")),
            (
                "web_ticket".into(),
                effect(&first).get("web_ticket").cloned().unwrap_or(Value::Null),
            ),
            ("error".into(), s("Local transport unavailable")),
            ("sent".into(), Value::Bool(false)),
        ]), scratch);
    let record = obj(vec![
        ("query".into(), s(query)),
        ("number_of_results".into(), n("1")),
        (
            "results".into(),
            Value::Arr(vec![obj(vec![
                ("url".into(), s("https://example.test/data")),
                ("title".into(), s("Validated primary result")),
            ])]),
        ),
    ])
    .to_string();
    let transport_result = search_response(controller, scratch, &fallback, 200, &record, "application/json");
    keep(
        &mut passed,
        "local transport failure cannot train a source preference",
        number_field(state(&transport_result), "search_preference") == 3
            && state(&transport_result).get("output").map(Value::str_).unwrap_or_default()
                .contains("Validated primary result"),
    );
    let first = manual_search(controller, scratch, state(&transport_result), query);
    thread::sleep(Duration::from_millis(1050));
    let fallback = search_response(controller, scratch, &first, 202, "<html>no completed result</html>", "text/html");
    let learned_again = search_response(controller, scratch, &fallback, 200, &record, "application/json");
    keep(
        &mut passed,
        "a measured result in the opposite direction corrects the learned order",
        number_field(state(&learned_again), "search_preference") == 2
            && state(&learned_again).get("output").map(Value::str_).unwrap_or_default()
                .contains("Validated primary result"),
    );

    let targeted = event(controller, Some(&standing), "harness_rules", obj(vec![]), scratch);
    let targeted = event(controller, Some(state(&targeted)), "harness_item_new", obj(vec![]), scratch);
    let targeted = event(
        controller,
        Some(state(&targeted)),
        "harness_item_save",
        obj(vec![
            ("scope".into(), s("tool")),
            ("tool".into(), s("web_search")),
            ("decision".into(), s("ask")),
            (
                "target".into(),
                s("https://html.duckduckgo.com/html/"),
            ),
            ("text".into(), s("Review this search destination")),
            ("enabled".into(), Value::Bool(true)),
        ]), scratch);
    let sent_23 = event(
            controller,
            Some(state(&targeted)),
            "send",
            obj(vec![("draft".into(), s("Search the web."))]), scratch);
    let pending = reply(
        controller,
        scratch,
        "chat",
        &sent_23,
        &obj(vec![
            ("tool".into(), s("web_search")),
            (
                "args".into(),
                obj(vec![("query".into(), s("bounded default source check"))]),
            ),
        ])
        .to_string());
    keep(
        &mut passed,
        "an explicit fallback-destination review outranks standing permission",
        none(&pending, "effect")
            && text_is(state(&pending), "dialog", "approval")
            && actions(&pending).contains("Review this search destination"),
    );
    event(controller, Some(state(&pending)), "reject", obj(vec![]), scratch);

    let mut st = configured_state(controller, scratch, "chat");
    put(
        &mut st,
        "web",
        obj(vec![
            ("search_url".into(), s("http://custom.test/search?q={query}")),
            ("key".into(), s("PRIVATE_CUSTOM_KEY")),
            ("header".into(), s("X-Key")),
        ]),
    );
    let _data = answer(controller, scratch, "chat", "Ordinary reply.");
    let sent_24 = event(
            controller,
            Some(&st),
            "send",
            obj(vec![("draft".into(), s("Use search."))]), scratch);
    let data = reply(
        controller,
        scratch,
        "chat",
        &sent_24,
        &format!(
            "Proposal:\n{}",
            obj(vec![
                ("tool".into(), s("web_search")),
                (
                    "args".into(),
                    obj(vec![
                        ("query".into(), s("Tampa")),
                        ("source".into(), s("alternative")),
                    ]),
                ),
            ])
            .to_string()
        ));
    let refused = event(controller, Some(state(&data)), "format_review", obj(vec![]), scratch);
    keep(
        &mut passed,
        "custom search alternative refuses before any endpoint or credential use",
        none(&refused, "effect")
            && none(state(&refused), "pending")
            && field(state(&refused), "web").get("key") == Some(&s("PRIVATE_CUSTOM_KEY")),
    );

    passed
}

fn configured_state(controller: &mut Controller, root: &str, fmt: &str) -> Value {
    let mut state = state(&event(controller, None, "resume", obj(vec![]), root)).clone();
    put(
        &mut state,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("model".into(), s("controlled")),
            ("protocol".into(), s(fmt)),
            ("endpoint".into(), s("http://model.test/model")),
            ("plain".into(), Value::Bool(true)),
            ("stream".into(), Value::Bool(false)),
        ])]),
    );
    put(
        &mut state,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("read,write,web_search,web_read")),
            ("investigation_preferences".into(), Value::Bool(false)),
        ])]),
    );
    put(
        &mut state,
        "web",
        obj(vec![("search_without_review".into(), Value::Bool(false))]),
    );
    state
}

fn answer(controller: &mut Controller, root: &str, fmt: &str, text: &str) -> Value {
    let state = configured_state(controller, root, fmt);
    let sent = event(
        controller,
        Some(&state),
        "send",
        obj(vec![("draft".into(), s("Use actual returned evidence."))]),
        root,
    );
    reply(controller, root, fmt, &sent, text)
}

fn reply(controller: &mut Controller, root: &str, _fmt: &str, data: &Value, text: &str) -> Value {
    // Python reads the live request protocol. The body is that protocol's object,
    // not an extra wrapper keyed by the format name.
    let fmt = field(state(data), "request_protocol").str_();
    let payload = match fmt {
        "chat" => obj(vec![(
            "choices".into(),
            Value::Arr(vec![obj(vec![
                ("index".into(), n("0")),
                ("message".into(), obj(vec![("content".into(), s(text))])),
                ("finish_reason".into(), s("stop")),
            ])]),
        )]),
        "local" => obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("done".into(), Value::Bool(true)),
            ("done_reason".into(), s("stop")),
        ]),
        "messages" => obj(vec![
            (
                "content".into(),
                Value::Arr(vec![obj(vec![
                    ("type".into(), s("text")),
                    ("text".into(), s(text)),
                ])]),
            ),
            ("stop_reason".into(), s("end_turn")),
        ]),
        "content" => obj(vec![(
            "candidates".into(),
            Value::Arr(vec![obj(vec![
                ("index".into(), n("0")),
                (
                    "content".into(),
                    obj(vec![(
                        "parts".into(),
                        Value::Arr(vec![obj(vec![("text".into(), s(text))])]),
                    )]),
                ),
                ("finishReason".into(), s("STOP")),
            ])]),
        )]),
        _ => panic!("unsupported model format"),
    };
    let body = payload.to_string();
    let wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let inputs = obj(vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), field(state(data), "model_expected").clone()),
        ("data".into(), s(&b64_encode(wire.as_bytes()))),
    ]);
    event(controller, Some(state(data)), "network_result", inputs, root)
}

fn search_response(
    controller: &mut Controller,
    root: &str,
    data: &Value,
    status: u16,
    body: &str,
    kind: &str,
) -> Value {
    let raw = format!(
        "HTTP/1.1 {status} Test\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let inputs = obj(vec![
        ("purpose".into(), s("web_tool")),
        ("web_ticket".into(), field(effect(data), "web_ticket").clone()),
        ("data".into(), s(&b64_encode(raw.as_bytes()))),
    ]);
    event(controller, Some(state(data)), "network_result", inputs, root)
}

fn request(controller: &mut Controller, root: &str, data: &Value) -> Value {
    let network = effect(data);
    let host = network
        .get("host")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("network effect has no host"))
        .to_string();
    let port = match network.get("port") {
        Some(Value::Num(token)) => token.parse::<u16>().unwrap_or_else(|error| panic!("port: {error}")),
        other => panic!(
            "network effect has no port: {}",
            other.map(Value::to_string).unwrap_or_else(|| "missing".into())
        ),
    };
    let wire = network
        .get("wire")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("network effect has no wire"))
        .to_string();
    let purpose = network
        .get("purpose")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("network effect has no purpose"))
        .to_string();
    let ticket = field(network, "web_ticket").clone();
    let mut stream = TcpStream::connect((host.as_str(), port))
        .unwrap_or_else(|error| panic!("could not connect to {host}:{port}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream
        .write_all(wire.as_bytes())
        .unwrap_or_else(|error| panic!("request write: {error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("request read: {error}"));
    let inputs = obj(vec![
        ("purpose".into(), s(&purpose)),
        ("web_ticket".into(), ticket),
        ("data".into(), s(&b64_encode(&response))),
    ]);
    event(controller, Some(state(data)), "network_result", inputs, root)
}

fn manual_search(controller: &mut Controller, root: &str, current: &Value, query: &str) -> Value {
    let opened = event(controller, Some(current), "web_search", obj(vec![]), root);
    event(
        controller,
        Some(state(&opened)),
        "web_search_review",
        obj(vec![("query".into(), s(query))]),
        root,
    )
}

fn event(
    controller: &mut Controller,
    state: Option<&Value>,
    action: &str,
    inputs: Value,
    root: &str,
) -> Value {
    controller.event(state, action, inputs, root)
}

fn restart(controller: &mut Controller, root: &str) {
    let previous = std::mem::replace(controller, Controller::spawn(root));
    previous.finish();
}

fn state(value: &Value) -> &Value {
    value
        .get("state")
        .unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn effect(value: &Value) -> &Value {
    value
        .get("effect")
        .unwrap_or_else(|| panic!("event returned no effect: {}", value.to_string()))
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
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

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
}

fn remove(value: &mut Value, key: &str) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    fields.retain(|(name, _)| name != key);
}

fn none(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn with_state(value: &Value, next: &Value) -> Value {
    let mut copy = value.clone();
    put(&mut copy, "state", next.clone());
    copy
}

fn truthy(value: &Value, key: &str) -> bool {
    value.get(key).map(Value::bool_).unwrap_or(false)
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn actions(value: &Value) -> String {
    value
        .get("ui")
        .and_then(|ui| ui.get("modal"))
        .map(Value::to_string)
        .unwrap_or_default()
}

fn chat_messages<'a>(value: &'a Value) -> &'a [Value] {
    let current = state(value);
    let chats = match current.get("chats") {
        Some(Value::Arr(items)) => items.as_slice(),
        _ => return &[],
    };
    let Some(chat) = chats.iter().find(|chat| chat.get("id") == current.get("active")) else {
        return &[];
    };
    match chat.get("messages") {
        Some(Value::Arr(items)) => items.as_slice(),
        _ => &[],
    }
}

fn message_count(value: &Value) -> usize {
    chat_messages(value).len()
}

fn message_field<'a>(value: &'a Value, index: isize) -> &'a Value {
    let messages = chat_messages(value);
    let len = messages.len() as isize;
    let index = if index < 0 { len + index } else { index };
    messages
        .get(index as usize)
        .unwrap_or_else(|| panic!("missing message {index}"))
}

fn message_content(value: &Value, index: isize) -> String {
    message_field(value, index)
        .get("content")
        .map(Value::str_)
        .unwrap_or_default()
        .to_string()
}

fn ui_messages<'a>(value: &'a Value) -> &'a [Value] {
    match value.get("ui").and_then(|ui| ui.get("messages")) {
        Some(Value::Arr(items)) => items.as_slice(),
        _ => &[],
    }
}

fn ui_message(value: &Value, index: isize) -> String {
    let messages = ui_messages(value);
    let len = messages.len() as isize;
    let index = if index < 0 { len + index } else { index };
    messages
        .get(index as usize)
        .and_then(|message| message.get("content"))
        .map(Value::str_)
        .unwrap_or_default()
        .to_string()
}

fn number_field(value: &Value, key: &str) -> i64 {
    match value.get(key) {
        Some(Value::Num(token)) => token.parse::<i64>().unwrap_or_else(|error| panic!("{key}: {error}")),
        other => panic!(
            "missing numeric {key}: {}",
            other.map(Value::to_string).unwrap_or_else(|| "missing".into())
        ),
    }
}

fn number_or(value: &Value, key: &str, default: i64) -> i64 {
    match value.get(key) {
        None | Some(Value::Null) => default,
        Some(Value::Num(token)) => token.parse::<i64>().unwrap_or_else(|error| panic!("{key}: {error}")),
        Some(other) => panic!("missing numeric {key}: {}", other.to_string()),
    }
}

fn remaining(value: &Value) -> Vec<Value> {
    value
        .get("web_expected")
        .and_then(|expected| expected.get("search_chain"))
        .and_then(|chain| chain.get("remaining"))
        .map(Value::arr)
        .cloned()
        .unwrap_or_default()
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn write_observation(root: &str, fmt: &str, data: &Value, raw: &str) {
    let record = obj(vec![
        ("format".into(), s(fmt)),
        ("raw".into(), s(raw)),
        ("display".into(), s(&ui_message(data, -1))),
        ("effect".into(), data.get("effect").cloned().unwrap_or(Value::Null)),
        (
            "pending".into(),
            state(data).get("pending").cloned().unwrap_or(Value::Null),
        ),
    ]);
    let path = if fmt == "chat" {
        Path::new(root)
            .join("output")
            .join("checks")
            .join("proposal-recovery-current-observation.json")
    } else {
        Path::new(root).join("output").join("checks").join(format!(
            "proposal-recovery-current-observation-{fmt}.json"
        ))
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    }
    std::fs::write(&path, format!("{}\n", record.to_string()))
        .unwrap_or_else(|error| panic!("could not write observation: {error}"));
}

struct Peer {
    port: u16,
    requests: Arc<Mutex<usize>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .unwrap_or_else(|error| panic!("could not bind peer: {error}"));
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(0usize));
        let observed = requests.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    continue;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buffer = Vec::new();
                let mut tmp = [0u8; 2048];
                while !buffer.windows(4).any(|window| window == b"\r\n\r\n") && buffer.len() < 65536 {
                    match stream.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => buffer.extend_from_slice(&tmp[..count]),
                    }
                }
                if buffer.is_empty() {
                    continue;
                }
                *observed.lock().unwrap() += 1;
                let body = b"<html><body>Actual separately reviewed page bytes.</body></html>";
                let response = format!(
                    "HTTP/1.0 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        });
        Self { port, requests }
    }

    fn request_count(&self) -> usize {
        *self.requests.lock().unwrap()
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "proposal-recovery"))
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

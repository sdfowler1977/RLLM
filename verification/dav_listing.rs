mod twin;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner public-event checks: current WebDAV resource/property/status scopes, actual row/open routing, mixed and absent statuses, nested metadata, XML field text, scoped errors, stale-view prevention, four provider formats and captured child return. Primary protocol reference: RFC4918 sections13and14.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path());
    controller.finish();
    twin::report_value(
        "dav_listing",
        &obj(vec![
            ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} dav_listing checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let nested = prop(
        "<d:displayname>File</d:displayname>",
        "<d:resourcetype/>",
        Some("HTTP/1.1 200 OK"),
        "<d:lockdiscovery><d:lockroot><d:href>/share/WRONG</d:href></d:lockroot><d:collection/></d:lockdiscovery>",
    );
    let mut data = listed(controller, root, "chat", &item("/share/file.txt", &nested, ""));
    let row = first_row(&data);
    keep(&mut passed, "nested metadata href preserves the exact listed file route", text(&row, "url") == "https://nas.test:443/share/file.txt");
    keep(&mut passed, "nested collection metadata does not change a file into a folder", bool_at(&row, "folder") == false && text(&row, "name") == "File");

    data = event(controller, root, state_of(&data), "remote:0", obj(vec![]));
    keep(&mut passed, "manual file open uses the actual resource href", wire(&data).starts_with("GET /share/file.txt HTTP/1.1"));
    data = response(controller, root, &data, "ACTUAL_FILE", 200, &[]);
    keep(&mut passed, "manual file open displays the fresh returned bytes", ui_contains(&data, "ACTUAL_FILE"));

    let failed = prop(
        "<d:displayname>WRONG_NAME</d:displayname>",
        "<d:resourcetype><d:collection/></d:resourcetype>",
        Some("HTTP/1.1 404 Not Found"),
        "",
    );
    let good = prop("<d:displayname>Actual name</d:displayname>", "<d:resourcetype/>", Some("HTTP/1.1 200 OK"), "");
    data = listed(controller, root, "chat", &item("/share/file.txt", &failed, ""));
    let row = first_row(&data);
    let errors = row.get("property_errors").unwrap_or_else(|| panic!("missing property_errors"));
    keep(
        &mut passed,
        "failed property values are reported as errors instead of file metadata",
        text(&row, "name") == "/share/file.txt" && bool_at(&row, "folder") == false && exact_errors(errors, &[("displayname", 404), ("resourcetype", 404)]),
    );

    for (sequence, first) in [(format!("{failed}{good}"), true), (format!("{good}{failed}"), false)] {
        data = listed(controller, root, "chat", &item("/share/file.txt", &sequence, ""));
        let row = first_row(&data);
        let errors = row.get("property_errors").unwrap_or_else(|| panic!("missing property_errors"));
        let name = if first {
            "successful requested properties survive mixed property statuses True"
        } else {
            "successful requested properties survive mixed property statuses False"
        };
        keep(&mut passed, name, text(&row, "name") == "Actual name" && bool_at(&row, "folder") == false && property_error(errors, "displayname") == 404);
    }

    let folder = prop(
        "<d:displayname>Actual folder</d:displayname>",
        "<d:resourcetype><d:collection/></d:resourcetype>",
        Some("HTTP/1.1 200 OK"),
        "",
    );
    data = listed(controller, root, "chat", &item("/share/folder/", &folder, ""));
    keep(&mut passed, "successful resource type still produces a usable folder row", bool_at(&first_row(&data), "folder"));
    data = event(controller, root, state_of(&data), "remote:0", obj(vec![]));
    keep(&mut passed, "manual folder open retains PROPFIND routing", wire(&data).starts_with("PROPFIND /share/folder/ HTTP/1.1"));
    let _ = response(controller, root, &data, &doc(""), 207, &[]);

    data = listed(controller, root, "chat", &item("/share/missing.txt", "", "<d:status>HTTP/1.1 404 Not Found</d:status>"));
    let row = first_row(&data);
    keep(&mut passed, "whole-resource status remains explicit in data and the row label", number(&row, "status") == 404 && text(&row, "name").contains("HTTP 404"));
    data = event(controller, root, state_of(&data), "remote:0", obj(vec![]));
    keep(&mut passed, "a historical listing status does not prevent a new explicit read", wire(&data).starts_with("GET /share/missing.txt HTTP/1.1"));
    data = response(controller, root, &data, "NOW_AVAILABLE", 200, &[]);
    keep(&mut passed, "fresh read can succeed after an earlier resource status", ui_contains(&data, "NOW_AVAILABLE"));

    data = listed(controller, root, "chat", &item("/share/legacy.txt", &prop("<d:displayname>Legacy</d:displayname>", "<d:resourcetype/>", None, ""), ""));
    let row = first_row(&data);
    keep(&mut passed, "legacy absent-status property listings retain their prior behavior", text(&row, "name") == "Legacy" && py_not(row.get("property_errors")));

    data = listed(
        controller,
        root,
        "chat",
        &item(
            "/share/escaped&amp;name.txt",
            &prop(
                "<d:displayname xml:lang=\"en&gt;fr\" extra=\"a>b\">Café &gt; 台灣</d:displayname>",
                "<d:resourcetype/>",
                Some("<![CDATA[HTTP/1.1 200 OK]]>"),
                "",
            ),
            "",
        ),
    );
    let row = first_row(&data);
    keep(&mut passed, "quoted tag delimiters preserve Unicode and entity-decoded property values", text(&row, "name") == "Café > 台灣" && text(&row, "url").ends_with("/escaped&name.txt"));

    data = listed(controller, root, "chat", &item("/share/fi<!--middle-->le.txt", &prop("<d:displayname>File <!--middle--> name</d:displayname>", "<d:resourcetype/>", Some("HTTP/1.1 200 OK"), ""), ""));
    let row = first_row(&data);
    keep(&mut passed, "comments preserve surrounding field text and resource coordinates", text(&row, "name") == "File  name" && text(&row, "url").ends_with("/file.txt"));

    let scoped = "<d:multistatus xmlns:d=\"DAV:\" xmlns:x=\"urn:extension\"><d:response><d:href>/share/scoped.txt</d:href><x:href>/share/decoy/</x:href><x:status>HTTP/1.1 404 Decoy</x:status><d:propstat><d:prop><d:displayname>Actual</d:displayname><d:resourcetype><x:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>";
    data = list_xml(controller, root, scoped);
    let row = first_row(&data);
    keep(&mut passed, "foreign href, collection and status do not override the DAV resource", text(&row, "url").ends_with("/scoped.txt") && text(&row, "name") == "Actual" && bool_at(&row, "folder") == false && row.get("status").is_none());
    data = event(controller, root, state_of(&data), "remote:0", obj(vec![]));
    keep(&mut passed, "extension collection cannot turn an actual file open into PROPFIND", wire(&data).starts_with("GET /share/scoped.txt "));
    data = response(controller, root, &data, "Namespace-safe file read", 200, &[]);
    keep(&mut passed, "the file open still displays the exact returned text", ui_contains(&data, "Namespace-safe file read"));

    let default_xml = "<multistatus xmlns=\"DAV:\"><response><href>/share/sub/</href><propstat><prop><resourcetype><alias:collection xmlns:alias=\"DAV:\"/></resourcetype><displayname>Folder</displayname></prop><status>HTTP/1.1 200 OK</status></propstat></response><response xmlns=\"urn:extension\"><href>/share/wrong</href></response></multistatus>";
    let invalid_default = default_xml.replacen("xmlns=\"DAV:\"", "xmlns=\"DAV&#58;\"", 1);
    data = list_xml(controller, root, default_xml);
    let invalid = list_xml(controller, root, &invalid_default);
    let invalid_notice = state_of(&invalid).get("notice_error").cloned();
    assert!(invalid_notice.is_some() && py_not(invalid_notice.as_ref()), "entity-decoded DAV namespace was rejected");
    let default_rows = rows(&data);
    keep(&mut passed, "default DAV namespace and locally declared aliases preserve valid folders", default_rows.len() == 1 && bool_at(&default_rows[0], "folder") && text(&default_rows[0], "url").ends_with("/sub/"));

    let rebound = "<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/share/file.txt</d:href><d:resourcetype><d:collection xmlns:d=\"urn:extension\"/></d:resourcetype></d:response><d:response><d:href>/share/folder/</d:href><d:resourcetype><d:collection/></d:resourcetype></d:response></d:multistatus>";
    data = list_xml(controller, root, rebound);
    let rebound_rows = rows(&data);
    keep(&mut passed, "prefix rebinding is scoped to its own element and does not affect siblings", rebound_rows.len() == 2 && bool_at(&rebound_rows[0], "folder") == false && bool_at(&rebound_rows[1], "folder"));

    let unknown_prefix = doc("<x:response/>");
    let unknown_attribute_prefix = doc("<d:response x:extra=\"1\"/>");
    let duplicate_expanded = doc("<d:response a:x=\"1\" b:x=\"2\" xmlns:a=\"urn:same\" xmlns:b=\"urn:same\"/>");
    let invalid_prefix_name = doc("<d:response xmlns:1=\"DAV:\"/>");
    let raw_delimiter = doc("<d:response xmlns:x=\"urn:<extension\"/>");
    let space_before = doc("< d:response/>");
    let malformed = [
        ("foreign root", "<x:multistatus xmlns:x=\"urn:extension\"/>"),
        ("unknown prefix", unknown_prefix.as_str()),
        ("unknown attribute prefix", unknown_attribute_prefix.as_str()),
        ("duplicate expanded attributes", duplicate_expanded.as_str()),
        ("invalid namespace prefix name", invalid_prefix_name.as_str()),
        ("raw attribute delimiter", raw_delimiter.as_str()),
        ("malformed decimal reference in binding", "<d:multistatus xmlns:d=\"DAV&#+58;\"/>"),
        ("malformed hex reference in binding", "<d:multistatus xmlns:d=\"DAV&#x+3A;\"/>"),
        ("space before tag name", space_before.as_str()),
        ("mismatched qualified close", "<d:multistatus xmlns:d=\"DAV:\" xmlns:x=\"DAV:\"></x:multistatus>"),
    ];
    for (label, xml) in malformed {
        data = list_xml(controller, root, xml);
        let state = state_of(&data);
        keep(&mut passed, &format!("malformed listing fails only its current operation {label}"), is_true(&state, "notice_error") && is_false(&state, "busy"));
    }

    let current_malformed = [
        ("empty property status", item("/share/f.txt", &prop("<d:displayname>False</d:displayname>", "<d:resourcetype/>", Some(""), ""), "")),
        ("malformed property status", item("/share/f.txt", &prop("<d:displayname>False</d:displayname>", "<d:resourcetype/>", Some("HTTP/1.1 BAD"), ""), "")),
        ("empty resource status", item("/share/f.txt", "", "<d:status/>")),
        ("duplicate resource status", item("/share/f.txt", "", "<d:status>HTTP/1.1 200 OK</d:status><d:status>HTTP/1.1 404 Missing</d:status>")),
        ("duplicate resource href", item("/share/f.txt", "", "<d:href>/share/other.txt</d:href>")),
    ];
    for (label, inner) in &current_malformed {
        data = listed(controller, root, "chat", inner);
        let state = state_of(&data);
        keep(
            &mut passed,
            &format!("current malformed listing is scoped to its request: {label}"),
            effect_is_null(&data) && is_false(&state, "busy") && is_true(&state, "notice_error") && py_not(state.get("remote")),
        );
    }

    let trailing = format!("{}extra", doc(""));
    let invalid_envelopes = [
        ("plain text", "not XML"),
        ("wrong root", "<other/>"),
        ("multiple roots", "<d:multistatus xmlns:d=\"DAV:\"/><other/>"),
        ("trailing data", trailing.as_str()),
    ];
    for (label, xml) in invalid_envelopes {
        data = list_xml(controller, root, xml);
        let state = state_of(&data);
        keep(&mut passed, &format!("invalid listing envelope is an actual scoped error: {label}"), effect_is_null(&data) && is_true(&state, "notice_error") && is_false(&state, "busy"));
    }

    data = listed(controller, root, "chat", &item("/share/sub/", &folder, ""));
    data = event(controller, root, state_of(&data), "remote:0", obj(vec![]));
    keep(&mut passed, "a new folder request clears rows from the earlier location", rows(&data).is_empty() && text(&state_of(&data), "remote_url").ends_with("/sub/"));
    data = response(controller, root, &data, "Unavailable", 503, &[]);
    let state = state_of(&data);
    keep(&mut passed, "failed navigation does not present old-folder rows under the new location", rows(&data).is_empty() && is_true(&state, "notice_error") && is_false(&state, "busy"));

    for fmt in ["chat", "local", "messages", "content"] {
        let state = drive_state(controller, root, fmt);
        let mut data = request(controller, root, &state, "drive_list", obj(vec![("path".into(), s(""))]));
        let listing = format!("{}{}{}", item("/share/file.txt", &nested, ""), item("/share/failed.txt", &failed, ""), item("/share/missing.txt", "", "<d:status>HTTP/1.1 404 Missing</d:status>"));
        data = response(controller, root, &data, &doc(&listing), 207, &[]);
        let shown = prompt(&data);
        keep(
            &mut passed,
            &format!("{fmt} model receives exact resource routes and scoped property/status data"),
            shown.contains("\"url\":\"https://nas.test:443/share/file.txt\"") && shown.contains("property_errors") && shown.contains("\"status\":404") && !shown.contains("WRONG_NAME") && !shown.contains("file.txt/share/WRONG"),
        );
        let _ = reply(controller, root, &data, None, obj(vec![]), "Reported the actual listing entries and their statuses.");
    }

    let mut state = drive_state(controller, root, "chat");
    set_array(&mut state, "agents", 1, "tools", "drive_list");
    let mut data = proposal(controller, root, &state, obj(vec![("agent".into(), twin::n("1")), ("task".into(), s("Inspect the actual folder listing."))]));
    data = event(controller, root, state_of(&data), "approve", obj(vec![]));
    data = reply(controller, root, &data, Some("drive_list"), obj(vec![("path".into(), s(""))]), "Answer");
    data = event(controller, root, state_of(&data), "approve", obj(vec![]));
    data = response(controller, root, &data, &doc(&item("/share/file.txt", &nested, "")), 207, &[]);
    let shown = prompt(&data);
    keep(&mut passed, "child listing uses the same source-correct parser and actual route", number(&state_of(&data), "request_index") == 1 && !shown.contains("file.txt/share/WRONG") && shown.contains("\"url\":\"https://nas.test:443/share/file.txt\""));
    let _ = finish(controller, root, &data);
    passed
}

fn drive_state(controller: &mut Controller, root: &str, format: &str) -> Value {
    let mut state = state_of(&event(controller, root, Value::Null, "resume", obj(vec![])));
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
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
                ("tool_format".into(), s("structured")),
            ])
        })
        .collect();
    put(&mut state, "profiles", Value::Arr(profiles));
    let agents = vec![
        obj(vec![
            ("name".into(), s("Parent")),
            ("model".into(), s("0")),
            ("tools".into(), s("read,delegate,drive_list,drive_read,drive_create,drive_update,drive_folder")),
            ("subagents".into(), Value::Arr(vec![twin::n("1"), twin::n("2")])),
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
    ];
    put(&mut state, "agents", Value::Arr(agents));
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
    let sent = event(controller, root, state.clone(), "send", obj(vec![("draft".into(), s("Use the configured network folder and report the actual result."))]));
    let mut data = reply(controller, root, &sent, Some(tool), args, "Answer");
    assert_eq!(text(&state_of(&data), "dialog"), "approval");
    data = event(controller, root, state_of(&data), "approve", obj(vec![]));
    assert!(effect_present(&data), "approved drive request produced no effect");
    data
}

fn proposal(controller: &mut Controller, root: &str, state: &Value, args: Value) -> Value {
    let sent = event(controller, root, state.clone(), "send", obj(vec![("draft".into(), s("Use a configured permitted child for this task."))]));
    reply(controller, root, &sent, Some("delegate"), args, "Answer")
}

fn finish(controller: &mut Controller, root: &str, data: &Value) -> Value {
    let data = reply(controller, root, data, None, obj(vec![]), "Actual child answer.");
    assert!(effect_present(&data) && number(&state_of(&data), "request_index") == 0, "child listing did not return to the parent");
    reply(controller, root, &data, None, obj(vec![]), "Parent answer using the actual returned child response.")
}

fn reply(controller: &mut Controller, root: &str, data: &Value, tool: Option<&str>, args: Value, text: &str) -> Value {
    let state = state_of(data);
    let protocol = text_of(&state, "request_protocol");
    let body = match protocol.as_str() {
        "chat" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if tool.is_some() {
                put(&mut message, "tool_calls", Value::Arr(vec![obj(vec![
                    ("id".into(), s("call_one")),
                    ("type".into(), s("function")),
                    ("function".into(), obj(vec![("name".into(), s(tool.unwrap())), ("arguments".into(), s(&args.to_string()))])),
                ])]));
            }
            obj(vec![("choices".into(), Value::Arr(vec![obj(vec![
                ("index".into(), twin::n("0")),
                ("message".into(), message),
                ("finish_reason".into(), s(if tool.is_some() { "tool_calls" } else { "stop" })),
            ])]))])
        }
        "local" => {
            let mut message = obj(vec![("content".into(), s(text))]);
            if tool.is_some() {
                put(&mut message, "tool_calls", Value::Arr(vec![obj(vec![
                    ("function".into(), obj(vec![("name".into(), s(tool.unwrap())), ("arguments".into(), args)])),
                ])]));
            }
            obj(vec![
                ("message".into(), message),
                ("done".into(), twin::b(true)),
                ("done_reason".into(), s("stop")),
            ])
        }
        "messages" => {
            let mut parts = vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])];
            if tool.is_some() {
                parts.push(obj(vec![
                    ("type".into(), s("tool_use")),
                    ("id".into(), s("call_one")),
                    ("name".into(), s(tool.unwrap())),
                    ("input".into(), args),
                ]));
            }
            obj(vec![
                ("content".into(), Value::Arr(parts)),
                ("stop_reason".into(), s(if tool.is_some() { "tool_use" } else { "end_turn" })),
            ])
        }
        _ => {
            let mut parts = vec![obj(vec![("text".into(), s(text))])];
            if tool.is_some() {
                parts.push(obj(vec![("functionCall".into(), obj(vec![("name".into(), s(tool.unwrap())), ("args".into(), args)]))]));
            }
            obj(vec![("candidates".into(), Value::Arr(vec![obj(vec![
                ("index".into(), twin::n("0")),
                ("content".into(), obj(vec![("role".into(), s("model")), ("parts".into(), Value::Arr(parts))])),
                ("finishReason".into(), s("STOP")),
            ])]))])
        }
    };
    let bytes = body.to_string().into_bytes();
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", bytes.len()).into_bytes();
    wire.extend_from_slice(&bytes);
    let ticket = text_of(&state, "model_expected");
    let encoded = twin::b64_encode(&wire);
    event(controller, root, state, "network_result", obj(vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), s(&ticket)),
        ("data".into(), s(&encoded)),
    ]))
}

fn listed(controller: &mut Controller, root: &str, format: &str, inner: &str) -> Value {
    let state = drive_state(controller, root, format);
    let opened = event(controller, root, state, "drive_list", obj(vec![]));
    response(controller, root, &opened, &doc(inner), 207, &[])
}

fn list_xml(controller: &mut Controller, root: &str, xml: &str) -> Value {
    let state = drive_state(controller, root, "chat");
    let opened = event(controller, root, state, "drive_list", obj(vec![]));
    response(controller, root, &opened, xml, 207, &[])
}

fn response(controller: &mut Controller, root: &str, data: &Value, body: &str, status: u16, headers: &[(&str, &str)]) -> Value {
    let effect = data.get("effect").unwrap_or_else(|| panic!("missing effect"));
    let mut wire = format!("HTTP/1.1 {status} Result\r\nContent-Length: {}\r\n", body.len()).into_bytes();
    for (name, value) in headers {
        wire.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(body.as_bytes());
    let mut inputs = Vec::new();
    for key in ["purpose", "drive_ticket", "child_ticket"] {
        if let Some(value) = effect.get(key) {
            inputs.push((key.to_string(), value.clone()));
        }
    }
    inputs.push(("data".into(), s(&twin::b64_encode(&wire))));
    event(controller, root, state_of(data), "network_result", Value::Obj(inputs))
}

fn event(controller: &mut Controller, root: &str, state: Value, action: &str, inputs: Value) -> Value {
    controller.event(Some(&state), action, inputs, root)
}

fn state_of(value: &Value) -> Value {
    value.get("state").cloned().unwrap_or_else(|| panic!("event returned no state"))
}

fn rows(value: &Value) -> Vec<Value> {
    match state_of(value).get("remote") {
        Some(Value::Arr(items)) => items.clone(),
        _ => panic!("missing remote"),
    }
}

fn first_row(value: &Value) -> Value {
    rows(value).into_iter().next().unwrap_or_else(|| panic!("missing remote row"))
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).map(Value::str_).unwrap_or_else(|| panic!("missing {key}")).to_string()
}

fn text_of(value: &Value, key: &str) -> String {
    text(value, key)
}

fn bool_at(value: &Value, key: &str) -> bool {
    value.get(key).map(Value::bool_).unwrap_or_else(|| panic!("missing {key}"))
}

fn number(value: &Value, key: &str) -> i64 {
    match value.get(key) {
        Some(Value::Num(text)) => text.parse().unwrap_or_else(|_| panic!("invalid number {text}")),
        _ => panic!("missing number {key}"),
    }
}

fn property_error(value: &Value, key: &str) -> i64 {
    number(value, key)
}

fn exact_errors(value: &Value, expected: &[(&str, i64)]) -> bool {
    let Value::Obj(fields) = value else {
        return false;
    };
    fields.len() == expected.len()
        && expected.iter().all(|(key, status)| matches!(value.get(key), Some(Value::Num(text)) if text == &status.to_string()))
}

fn wire(value: &Value) -> String {
    let effect = value.get("effect").unwrap_or_else(|| panic!("missing effect"));
    text(effect, "wire")
}

fn prompt(value: &Value) -> String {
    let effect = value.get("effect").unwrap_or_else(|| panic!("missing effect"));
    let wire = text(effect, "wire");
    let index = wire.find("\r\n\r\n").unwrap_or_else(|| panic!("wire has no body"));
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

fn ui_contains(value: &Value, needle: &str) -> bool {
    value.get("ui").map(|ui| ui.to_string().contains(needle)).unwrap_or(false)
}

fn effect_is_null(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn effect_present(value: &Value) -> bool {
    matches!(value.get("effect"), Some(effect) if !matches!(effect, Value::Null))
}

fn is_true(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Bool(true)))
}

fn is_false(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Bool(false)))
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

fn doc(inner: &str) -> String {
    format!("<d:multistatus xmlns:d=\"DAV:\">{inner}</d:multistatus>")
}

fn prop(name: &str, kind: &str, status: Option<&str>, extra: &str) -> String {
    let status_xml = match status {
        Some(status) => format!("<d:status>{status}</d:status>"),
        None => String::new(),
    };
    format!("<d:propstat><d:prop>{name}{kind}{extra}</d:prop>{status_xml}</d:propstat>")
}

fn item(href: &str, properties: &str, extra: &str) -> String {
    format!("<d:response><d:href>{href}</d:href>{properties}{extra}</d:response>")
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
    let items = fields.iter_mut().find(|(name, _)| name == key).map(|(_, value)| value).unwrap();
    let Value::Arr(items) = items else {
        panic!("expected array");
    };
    put(&mut items[index], field, s(next));
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

//! Native verification twin of verification/history_migration.py.
mod twin;

use std::path::{Path, PathBuf};

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner compatibility checks: actual prior and current controllers with controlled peer replies.";

fn main() {
    let previous = parse_args();
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch(twin::create_scratch(&project, "history-migration"));
    let root = scratch.0.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8"));
    std::fs::write(Path::new(root).join("input.txt"), "Ordinary local input remains available.")
        .unwrap_or_else(|error| panic!("could not write input.txt: {error}"));

    let mut passed = Vec::new();
    let mut prior = Controller::spawn_at(
        &previous,
        project
            .to_str()
            .unwrap_or_else(|| panic!("project root is not UTF-8")),
    );
    let saved = drive_prior(&mut prior, root, &mut passed);
    prior.finish();

    let mut current = Controller::spawn(
        project
            .to_str()
            .unwrap_or_else(|| panic!("project root is not UTF-8")),
    );
    drive_current(&mut current, root, &saved, &mut passed);
    current.finish();

    let body = obj(vec![
        ("passed".into(), Value::Num(passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value_printed("history_migration", &body, PRINTED_REPORT_TRAILING_NEWLINE);

    let written = obj(vec![(
        "passed".into(),
        Value::Arr(passed.iter().map(|name| s(name)).collect()),
    )]);
    twin::report_value("history_migration", &written, true);

    std::fs::remove_dir_all(&scratch.0)
        .unwrap_or_else(|error| panic!("could not remove {}: {error}", scratch.0.display()));
}

fn parse_args() -> PathBuf {
    let mut arguments = std::env::args().skip(1);
    let mut previous = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--previous-controller" => {
                if previous.is_some() {
                    panic!("--previous-controller was provided more than once");
                }
                previous = Some(PathBuf::from(arguments.next().unwrap_or_else(|| {
                    panic!("--previous-controller requires a path")
                })));
            }
            _ => panic!("unrecognized arguments: {argument}"),
        }
    }
    previous.unwrap_or_else(|| {
        panic!("the following arguments are required: --previous-controller")
    })
}

fn drive_prior(prior: &mut Controller, root: &str, passed: &mut Vec<String>) -> Value {
    let old = String::from_utf8(vec![114, 101, 97]).unwrap();
    let endpoint = "http://127.0.0.1:9127/mcp";
    let key = "fixture-key";
    let resumed = prior.event(None, "resume", obj(vec![]), root);
    let mut state = field(&resumed, "state").clone();

    put(
        &mut state,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://model.test/v1/chat/completions")),
            ("protocol".into(), s("chat")),
            ("model".into(), s("controlled")),
            ("plain".into(), Value::Bool(true)),
            ("stream".into(), Value::Bool(false)),
        ])]),
    );
    put(
        &mut state,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s(&format!("read,{old}"))),
        ])]),
    );
    put(
        &mut state,
        &old,
        obj(vec![
            ("endpoint".into(), s(endpoint)),
            ("key".into(), s(key)),
        ]),
    );

    let sent = prior.event(
        Some(&state),
        "send",
        obj(vec![("draft".into(), s("Obtain an actual peer observation."))]),
        root,
    );
    // The operation the controller answers under is the name it kept for the
    // seeded peer connection: a controller built before the operation
    // identifier was renamed keeps the legacy name, and one that has already
    // imported the rename keeps the current one. The identifier is therefore
    // read from the controller's own state rather than assumed, so the legacy
    // seeding above still describes the historical scenario while the proposal
    // names an operation this controller accepts. The historical receipt is
    // still required to carry the legacy identifier below.
    let identifier = operation_identifier(field(&sent, "state"), endpoint, key);
    let proposal = format!(
        "{{\"tool\":\"{identifier}\",\"args\":{{\"name\":\"inspect_product\",\"arguments\":{{}}}}}}"
    );
    let mut dialogue = sent;
    let model_ticket = field(field(&dialogue, "state"), "model_expected").clone();
    let response = Some(obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(&proposal))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]));
    let data = wire(&response, "200 OK");
    dialogue = prior.event(
        Some(field(&dialogue, "state")),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), model_ticket),
            ("data".into(), s(&data)),
        ]),
        root,
    );
    if value_str(field(field(&dialogue, "state"), "dialog")) != Some("approval") {
        panic!("prior controller did not request approval");
    }
    dialogue = prior.event(
        Some(field(&dialogue, "state")),
        "approve",
        obj(vec![]),
        root,
    );

    let mut settled = false;
    for _ in 0..8 {
        let effect = field(&dialogue, "effect").clone();
        // The peer effect is an object, not a string: `network::wire` builds it
        // with a `kind` field (`core/network.rs`), so the kind is read from that
        // field. A missing kind is the same failure as a wrong kind.
        if value_str(effect.get("kind").unwrap_or(&Value::Null)) != Some("network") {
            panic!("Peer operation did not produce a network effect");
        }
        let purpose = field(&effect, "purpose").clone();
        if value_str(&purpose) == Some("model") {
            settled = true;
            break;
        }
        let wire_text = value_str(field(&effect, "wire"))
            .unwrap_or_else(|| panic!("network effect has no wire"));
        let body = wire_text
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("network effect has no HTTP body"))
            .1;
        let request = Value::parse(body);
        let method = value_str(field(&request, "method"))
            .unwrap_or_else(|| panic!("network request has no method"));
        let response = match method {
            "initialize" => Some(obj(vec![
                ("jsonrpc".into(), s("2.0")),
                ("id".into(), field(&request, "id").clone()),
                (
                    "result".into(),
                    obj(vec![
                        (
                            "protocolVersion".into(),
                            field(field(&request, "params"), "protocolVersion").clone(),
                        ),
                        (
                            "capabilities".into(),
                            obj(vec![("tools".into(), obj(vec![]))]),
                        ),
                        (
                            "serverInfo".into(),
                            obj(vec![
                                ("name".into(), s("Fixture")),
                                ("version".into(), s("one")),
                            ]),
                        ),
                    ]),
                ),
            ])),
            "notifications/initialized" => None,
            "tools/call" => Some(obj(vec![
                ("jsonrpc".into(), s("2.0")),
                ("id".into(), field(&request, "id").clone()),
                (
                    "result".into(),
                    obj(vec![(
                        "content".into(),
                        Value::Arr(vec![obj(vec![
                            ("type".into(), s("text")),
                            ("text".into(), s("Original controlled peer observation.")),
                        ])]),
                    )]),
                ),
            ])),
            other => panic!("{other}"),
        };
        let status = if response.is_none() {
            "202 Accepted"
        } else {
            "200 OK"
        };
        let data = wire(&response, status);
        dialogue = prior.event(
            Some(field(&dialogue, "state")),
            "network_result",
            obj(vec![
                ("purpose".into(), purpose),
                ("data".into(), s(&data)),
            ]),
            root,
        );
    }
    if !settled {
        panic!("Peer operation did not settle");
    }
    if value_str(field(field(&dialogue, "effect"), "purpose")) != Some("model") {
        panic!("Peer operation did not settle on the model request");
    }

    let ticket = field(field(&dialogue, "state"), "model_expected").clone();
    let response = Some(obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            (
                "message".into(),
                obj(vec![(
                    "content".into(),
                    s("Answer following the actual peer result."),
                )]),
            ),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]));
    let data = wire(&response, "200 OK");
    let saved = prior.event(
        Some(field(&dialogue, "state")),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&data)),
        ]),
        root,
    );
    let saved = field(&saved, "state").clone();
    let case = inquiry_with_evidence(&saved);
    let receipt = field(case, "evidence").arr()[0].clone();
    let output = value_str(field(&receipt, "output")).unwrap_or_default();
    check(
        passed,
        "reference controller creates actual owned historical receipt",
        value_str(field(&receipt, "tool")) == Some("rea")
            && output.contains("Original controlled peer observation."),
    );
    saved
}

fn drive_current(current: &mut Controller, root: &str, saved: &Value, passed: &mut Vec<String>) {
    let case = inquiry_with_evidence(saved);
    let case_id = field(case, "id").clone();
    let receipt = field(case, "evidence").arr()[0].clone();

    let resumed = current.event(Some(saved), "resume", obj(vec![]), root);
    let migrated = inquiry_by_id(field(&resumed, "state"), &case_id);
    let actual = field(migrated, "evidence").arr()[0].clone();
    check(
        passed,
        "historical receipt uses current operation identifier",
        value_str(field(&actual, "tool")) == Some("mcp"),
    );
    check(
        passed,
        "captured output bytes and digests are unchanged",
        ["output", "sha256", "prefix_sha256", "bytes", "source_guard", "args"]
            .iter()
            .all(|key| field(&actual, key) == field(&receipt, key)),
    );

    let case_id_text = value_str(&case_id).unwrap_or_default();
    let selected = current.event(
        Some(field(&resumed, "state")),
        &format!("inquiry_select:{case_id_text}"),
        obj(vec![]),
        root,
    );
    let selected_state = field(&selected, "state");
    check(
        passed,
        "migrated historical question remains readable",
        !matches!(selected_state.get("notice_error"), Some(Value::Bool(true)))
            && value_str(selected_state.get("dialog").unwrap_or(&Value::Null)) == Some("inquiry_view"),
    );

    let evidence = current.event(
        Some(selected_state),
        "inquiry_section:evidence",
        obj(vec![]),
        root,
    );
    let evidence_state = field(&evidence, "state");
    // The rendered screen is the event reply's `ui`, a sibling of `state`
    // (`core/application.rs`); the state itself never carries a `ui` key.
    check(
        passed,
        "historical evidence is still available",
        value_str(evidence_state.get("dialog").unwrap_or(&Value::Null)) == Some("inquiry_view")
            && field(&evidence, "ui").to_string().contains("Original controlled peer observation."),
    );

    let mut corrupted = saved.clone();
    let corrupted_case = inquiry_by_id_mut(&mut corrupted, &case_id);
    // The evidence of a record is a list of receipts, so the tampered receipt
    // is reached by its position in that list.
    let corrupted_receipt = element_mut(field_mut(corrupted_case, "evidence"), 0);
    put(
        corrupted_receipt,
        "output",
        s("Tampered input without a matching original receipt"),
    );
    let corrupted_resumed = current.event(Some(&corrupted), "resume", obj(vec![]), root);
    let corrupted_selected = current.event(
        Some(field(&corrupted_resumed, "state")),
        &format!("inquiry_select:{case_id_text}"),
        obj(vec![]),
        root,
    );
    check(
        passed,
        "migration refuses to bless altered receipt data",
        matches!(
            field(&corrupted_selected, "state").get("notice_error"),
            Some(Value::Bool(true))
        ),
    );

    let read = current.op(&obj(vec![
        ("op".into(), s("tool")),
        ("root".into(), s(root)),
        ("tool".into(), s("read")),
        (
            "args".into(),
            obj(vec![("path".into(), s("input.txt"))]),
        ),
    ]));
    check(
        passed,
        "corrupt optional history does not block local operations",
        value_str(&read) == Some("Ordinary local input remains available."),
    );
}

/// Builds the `network_result` payload for one HTTP response.
///
/// The product reads `inputs["data"]` and decodes it with
/// `network::unbase64`, which refuses every byte outside the base64 alphabet
/// (`core/network.rs`), so `data` is the base64 of the whole HTTP response
/// text, not of the body alone. The framing the product then parses requires
/// the `Content-Length` header to equal the byte count of the raw body that
/// follows `\r\n\r\n`.
fn wire(value: &Option<Value>, status: &str) -> String {
    let bytes = value.as_ref().map(|value| value.to_string()).unwrap_or_default();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        bytes.len(),
        bytes
    );
    twin::b64_encode(response.as_bytes())
}

/// The operation identifier the prior controller kept for the seeded peer.
///
/// The twin seeds one peer connection under the legacy operation name. The
/// product answers an operation by the name it holds that connection under, so
/// the identifier is read from the controller's own state: the key the seeded
/// connection was retained under. A controller that has not yet imported the
/// rename keeps the legacy name and one that has keeps the current one, and
/// the instrument follows the controller instead of assuming. When several
/// records carry the seeded connection the operation the controller accepts is
/// the one an agent profile advertises (`core/harness.rs`).
fn operation_identifier(state: &Value, endpoint: &str, key: &str) -> String {
    let Value::Obj(fields) = state else {
        panic!("prior controller state is not an object");
    };
    let candidates: Vec<String> = fields
        .iter()
        .filter(|(_, value)| {
            value.get("endpoint").and_then(value_str) == Some(endpoint)
                && value.get("key").and_then(value_str) == Some(key)
        })
        .map(|(name, _)| name.clone())
        .collect();
    let Some(first) = candidates.first().cloned() else {
        panic!("prior controller retained no peer connection under an operation name");
    };
    if candidates.len() == 1 {
        return first;
    }
    let advertised = advertised_tools(state);
    candidates
        .iter()
        .find(|name| advertised.iter().any(|tool| tool == *name))
        .cloned()
        .unwrap_or_else(|| {
            panic!("prior controller advertised no operation name for the seeded peer connection")
        })
}

/// The tools the active agent profile advertises.
///
/// The product accepts a proposed operation only when the active agent's tools
/// list names it (`core/harness.rs`), and the active agent is the profile the
/// current chat selects, else the globally selected profile
/// (`core/interaction.rs`, `core/harness.rs`).
fn advertised_tools(state: &Value) -> Vec<String> {
    let agents = match state.get("agents") {
        Some(Value::Arr(agents)) => agents,
        _ => return Vec::new(),
    };
    let Some(agent) = agents.get(active_agent_index(state)) else {
        return Vec::new();
    };
    agent
        .get("tools")
        .and_then(value_str)
        .map(|tools| {
            tools
                .split(',')
                .map(|tool| tool.trim().to_string())
                .filter(|tool| !tool.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The index of the active agent profile.
///
/// The current chat selects the profile and the globally selected profile is
/// the fallback (`core/interaction.rs`). The current chat is the one whose
/// identity the state marks active (`core/application.rs`).
fn active_agent_index(state: &Value) -> usize {
    let active = state.get("active").and_then(value_str).unwrap_or_default();
    let chats = match state.get("chats") {
        Some(Value::Arr(chats)) => chats,
        _ => return state.get("agent_selected").and_then(index_value).unwrap_or(0),
    };
    let chat_profile = chats
        .iter()
        .find(|chat| chat.get("id").and_then(value_str) == Some(active))
        .and_then(|chat| chat.get("agent_profile"))
        .filter(|profile| !matches!(profile, Value::Null))
        .and_then(index_value);
    chat_profile.unwrap_or_else(|| {
        state
            .get("agent_selected")
            .and_then(index_value)
            .unwrap_or(0)
    })
}

/// Reads one index the way the product reads an index: a number or a numeric
/// text, and zero for anything else (`core/application.rs`).
fn index_value(value: &Value) -> Option<usize> {
    match value {
        Value::Num(text) | Value::Str(text) => Some(text.parse::<usize>().unwrap_or(0)),
        _ => None,
    }
}

fn inquiry_with_evidence(state: &Value) -> &Value {
    field(field(state, "inquiry_records"), "items")
        .arr()
        .iter()
        .find(|case| !field(case, "evidence").arr().is_empty())
        .unwrap_or_else(|| panic!("no inquiry record with evidence"))
}

fn inquiry_by_id<'a>(state: &'a Value, id: &Value) -> &'a Value {
    field(field(state, "inquiry_records"), "items")
        .arr()
        .iter()
        .find(|case| field(case, "id") == id)
        .unwrap_or_else(|| panic!("no inquiry record with the requested id"))
}

fn inquiry_by_id_mut<'a>(state: &'a mut Value, id: &Value) -> &'a mut Value {
    let items = field_mut(field_mut(state, "inquiry_records"), "items");
    let Value::Arr(items) = items else {
        panic!("expected array");
    };
    items
        .iter_mut()
        .find(|case| field(case, "id") == id)
        .unwrap_or_else(|| panic!("no inquiry record with the requested id"))
}

fn value_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
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

fn element_mut<'a>(value: &'a mut Value, index: usize) -> &'a mut Value {
    let Value::Arr(items) = value else {
        panic!("expected array");
    };
    &mut items[index]
}

fn check(passed: &mut Vec<String>, name: &str, value: bool) {
    if !value {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

/// The printed report body ends with the newline the retired leg's print() wrote.
///
/// The retired leg prints its report with `print(json.dumps(body, indent=2))`, and
/// `print` appends the trailing newline, so the leg's printed report body carries
/// that newline. The twin asks the shared reporter for the same trailing newline,
/// and the report file it writes afterwards is the leg's own file shape.
const PRINTED_REPORT_TRAILING_NEWLINE: bool = true;

/// The scratch directory this leg owns, removed on every exit path.
///
/// The retired leg holds its temporary directory in a context manager, so the
/// directory is removed when the leg ends, including when it aborts. The twin's own
/// scratch directory was removed on the success path only, so an aborted run left it
/// behind in the project's output tree. This guard owns the directory for the whole
/// run and removes it on the way out, whichever way the leg ends. It changes nothing
/// the leg reports: an abort still aborts with the same diagnostic.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

mod twin;

use std::path::PathBuf;
use std::process::Command;

use twin::{obj, s, Controller, Value};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mut live_seen = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--live" => {
                if live_seen {
                    refuse("unrecognized arguments");
                }
                live_seen = true;
            }
            _ => refuse("unrecognized arguments"),
        }
    }
    if !live_seen {
        refuse("the following arguments are required: --live");
    }

    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = Scratch(twin::create_scratch(&project, "live-web"));
    let root = scratch
        .0
        .to_str()
        .unwrap_or_else(|| panic!("scratch directory is not UTF-8"))
        .to_string();

    let mut controller = Controller::spawn(&project_text);
    let mut passed = Vec::new();
    let mut observations = Vec::new();

    let resumed = event(&mut controller, None, "resume", obj(vec![]), &root);
    let mut state = state_of(&resumed).clone();
    set_profiles(&mut state);
    set_agents(&mut state);
    set_investigation_preferences(&mut state);

    let mut result = event(
        &mut controller,
        Some(&state),
        "send",
        obj(vec![(
            "draft".into(),
            s("Use the callable child to search for the Rust programming language."),
        )]),
        &root,
    );
    check(
        &mut passed,
        "adaptive answer mode remains available with optional guidance off",
        wire_contains(must_get(must_get(&result, "effect"), "wire"), "Choose the smallest useful response"),
    );

    result = reply(
        &mut controller,
        &result,
        &root,
        &dumps(&obj(vec![
            ("tool".into(), s("delegate")),
            (
                "args".into(),
                obj(vec![
                    ("agent".into(), twin::n("1")),
                    (
                        "task".into(),
                        s("Search for Rust programming language and return actual source evidence."),
                    ),
                ]),
            ),
        ])),
    );
    check(
        &mut passed,
        "child selection enters review",
        must_str(state_of(&result), "dialog") == "approval" && present_null(&result, "effect"),
    );

    result = event(
        &mut controller,
        Some(state_of(&result)),
        "approve",
        obj(vec![]),
        &root,
    );
    check(
        &mut passed,
        "approved child uses its assigned model",
        number(must_get(state_of(&result), "request_index")) == 1,
    );

    result = reply(
        &mut controller,
        &result,
        &root,
        &format!(
            "I will look up the language.\n{}",
            dumps(&obj(vec![
                ("tool".into(), s("web_search")),
                (
                    "args".into(),
                    obj(vec![("query".into(), s("Rust programming language"))]),
                ),
            ]))
        ),
    );
    check(
        &mut passed,
        "default automatic approval dispatches a wrapped child search without a review",
        result.get("effect").is_some_and(|effect| {
            must_str(effect, "purpose") == "web_tool"
                && !state_of(&result).get("pending").is_some_and(twin::truthy)
        }),
    );

    for _attempt in 0..2 {
        let effect = must_get(&result, "effect").clone();
        assert_eq!(
            must_str(&effect, "purpose"),
            "web_tool",
            "{}",
            state_of(&result)
                .get("notice")
                .map(Value::to_string)
                .unwrap_or_default()
        );
        let wire = must_str(&effect, "wire");
        let target = wire
            .split(' ')
            .nth(1)
            .unwrap_or_else(|| panic!("web tool wire has no target"));
        let tls = match effect.get("tls") {
            Some(Value::Bool(value)) => *value,
            Some(other) => panic!("tls is not boolean: {}", other.to_string()),
            None => panic!("missing tls"),
        };
        let host = must_str(&effect, "host");
        let url = format!("{}://{}{}", if tls { "https" } else { "http" }, host, target);
        assert!(matches!(host, "api.mwmbl.org" | "html.duckduckgo.com"));
        let limit = number(must_get(&effect, "limit"));
        let raw = curl(&url, limit);
        observations.push(obj(vec![
            ("url".into(), s(&url)),
            ("bytes".into(), twin::n(&raw.len().to_string())),
            ("sha256".into(), s(&twin::sha256_bytes(&raw))),
        ]));
        let mut inputs = vec![
            ("purpose".into(), s("web_tool")),
            ("web_ticket".into(), must_clone(&effect, "web_ticket")),
            ("data".into(), s(&twin::b64_encode(&raw))),
        ];
        for key in ["child_ticket", "child_owner"] {
            if let Some(value) = effect.get(key) {
                inputs.push((key.into(), value.clone()));
            }
        }
        result = event(
            &mut controller,
            Some(state_of(&result)),
            "network_result",
            obj(inputs),
            &root,
        );
        if result
            .get("effect")
            .and_then(|effect| effect.get("purpose"))
            .and_then(as_str)
            == Some("model")
        {
            break;
        }
    }

    check(
        &mut passed,
        "actual live search evidence returns to the assigned child",
        result.get("effect").is_some_and(|effect| {
            must_str(effect, "purpose") == "model"
                && number(must_get(state_of(&result), "request_index")) == 1
                && wire_contains(must_get(effect, "wire"), "Search results for: Rust programming language")
        }),
    );
    check(
        &mut passed,
        "retained results include source URLs and actual snippets",
        result.get("effect").is_some_and(|effect| {
            let wire = must_str(effect, "wire");
            wire.contains("Returned results")
                && wire.contains("snippet")
                && wire.contains("http")
        }),
    );
    let records = must_get(must_get(state_of(&result), "inquiry_records"), "items");
    let records = match records {
        Value::Arr(items) => items.clone(),
        _ => panic!("inquiry_records items is not an array: {}", records.to_string()),
    };
    let journal_contains = records.iter().any(|record| {
        match record.get("evidence") {
            Some(Value::Arr(evidence)) => evidence.iter().any(|entry| {
                entry
                    .get("output")
                    .and_then(as_str)
                    .is_some_and(|output| output.contains("Search results for: Rust programming language"))
            }),
            _ => false,
        }
    });
    check(
        &mut passed,
        "live returned evidence reaches the integrated journal",
        journal_contains,
    );

    result = reply(
        &mut controller,
        &result,
        &root,
        "Child received the live source results for Rust programming language.",
    );
    check(
        &mut passed,
        "actual child completion returns to the captured parent",
        result.get("effect").is_some_and(|effect| {
            number(must_get(state_of(&result), "request_index")) == 0
                && wire_contains(must_get(effect, "wire"), "Child received the live source results")
        }),
    );

    result = reply(
        &mut controller,
        &result,
        &root,
        "The child returned the search evidence.",
    );
    check(
        &mut passed,
        "parent completes the ordinary command",
        !state_of(&result).get("busy").is_some_and(twin::truthy) && present_null(&result, "effect"),
    );

    controller.finish();

    let body = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.into_iter().map(|name| s(&name)).collect()),
        ),
        ("transport_observations".into(), Value::Arr(observations)),
        (
            "scope".into(),
            s("Live public HTTPS search responses transported by curl through actual automatic search approval, owned callbacks, evidence retention and child/parent continuation. Model replies are controlled protocol fixtures; configured model inference and installed Android transport require their own execution."),
        ),
    ]);
    twin::report_value_printed("live_web", &body, true);

    std::fs::remove_dir_all(&scratch.0)
        .unwrap_or_else(|error| panic!("could not remove {}: {error}", scratch.0.display()));
}

// The leg sends its model payloads with json.dumps defaults (", " and ": "), so the
// twin builds the same single-line text instead of the indented report layout.
fn dumps(value: &Value) -> String {
    let mut out = String::new();
    write_dumps(value, &mut out);
    out
}

fn write_dumps(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(&flag.to_string()),
        Value::Num(token) => out.push_str(token),
        Value::Str(_) => out.push_str(&value.to_string()),
        Value::Arr(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_dumps(item, out);
            }
            out.push(']');
        }
        Value::Obj(fields) => {
            out.push('{');
            for (index, (key, item)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&Value::Str(key.clone()).to_string());
                out.push_str(": ");
                write_dumps(item, out);
            }
            out.push('}');
        }
    }
}

fn refuse(message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(2);
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

fn reply(controller: &mut Controller, result: &Value, root: &str, content: &str) -> Value {
    let body = dumps(&obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            (
                "message".into(),
                obj(vec![("content".into(), s(content))]),
            ),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]));
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes();
    let mut inputs = vec![
        ("purpose".into(), s("model")),
        (
            "model_ticket".into(),
            must_clone(state_of(result), "model_expected"),
        ),
        ("data".into(), s(&twin::b64_encode(&raw))),
    ];
    let effect = must_get(result, "effect");
    for key in ["child_ticket", "child_owner"] {
        if let Some(value) = effect.get(key) {
            inputs.push((key.into(), value.clone()));
        }
    }
    event(
        controller,
        Some(state_of(result)),
        "network_result",
        obj(inputs),
        root,
    )
}

fn set_profiles(state: &mut Value) {
    let profiles = ["parent", "child"]
        .into_iter()
        .map(|name| {
            obj(vec![
                ("name".into(), s(name)),
                ("model".into(), s(name)),
                ("protocol".into(), s("chat")),
                (
                    "endpoint".into(),
                    s(&format!("http://{name}.test/v1/chat/completions")),
                ),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
            ])
        })
        .collect();
    *state = overlay(state, vec![("profiles".into(), Value::Arr(profiles))]);
}

fn set_agents(state: &mut Value) {
    let agents = Value::Arr(vec![
        obj(vec![
            ("name".into(), s("Parent")),
            ("tools".into(), s("delegate,web_search,web_read")),
            ("subagents".into(), Value::Arr(vec![twin::n("1")])),
            ("model".into(), s("0")),
            ("investigation_preferences".into(), twin::b(false)),
        ]),
        obj(vec![
            ("name".into(), s("Child")),
            ("tools".into(), s("web_search,web_read")),
            ("model".into(), s("1")),
            ("investigation_preferences".into(), twin::b(false)),
        ]),
    ]);
    *state = overlay(state, vec![("agents".into(), agents)]);
}

fn set_investigation_preferences(state: &mut Value) {
    *state = overlay(
        state,
        vec![(
            "investigation_preferences",
            obj(vec![("enabled".into(), twin::b(false))]),
        )],
    );
}

fn overlay(base: &Value, updates: Vec<(&str, Value)>) -> Value {
    let Value::Obj(fields) = base else {
        panic!("expected object, got {}", base.to_string());
    };
    let mut fields = fields.clone();
    for (key, value) in updates {
        if let Some((_, slot)) = fields.iter_mut().find(|(name, _)| name == key) {
            *slot = value;
        } else {
            fields.push((key.to_string(), value));
        }
    }
    Value::Obj(fields)
}

fn curl(url: &str, limit: i128) -> Vec<u8> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--http1.1",
            "--include",
            "--suppress-connect-headers",
            "--connect-timeout",
            "10",
            "--max-time",
            "30",
            "--max-filesize",
            &limit.to_string(),
            "--header",
            "Accept-Encoding: identity",
            url,
        ])
        .output()
        .unwrap_or_else(|error| panic!("could not run curl: {error}"));
    if !output.status.success() {
        panic!(
            "curl failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output.stdout
}

fn number(value: &Value) -> i128 {
    let Value::Num(token) = value else {
        panic!("expected number, got {}", value.to_string());
    };
    if let Ok(number) = token.parse::<i128>() {
        return number;
    }
    let number = token
        .parse::<f64>()
        .unwrap_or_else(|error| panic!("invalid number {token}: {error}"));
    if !number.is_finite() {
        panic!("non-finite number: {token}");
    }
    number as i128
}

fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

// The leg reads result['effect'] directly, so a reply that omits the key raises
// KeyError there; only a present key carrying an explicit null counts as no effect.
fn present_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Null))
}

fn wire_contains(value: &Value, needle: &str) -> bool {
    match value {
        Value::Str(text) => text.contains(needle),
        _ => false,
    }
}

fn state_of(value: &Value) -> &Value {
    value
        .get("state")
        .unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn must_get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {}", value.to_string()))
}

fn must_clone(value: &Value, key: &str) -> Value {
    must_get(value, key).clone()
}

fn must_str<'a>(value: &'a Value, key: &str) -> &'a str {
    match must_get(value, key) {
        Value::Str(text) => text,
        other => panic!("{key} is not text: {}", other.to_string()),
    }
}

fn check(passed: &mut Vec<String>, name: &str, value: bool) {
    if !value {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

/// The scratch directory this leg owns, removed when the leg unwinds.
///
/// The retired leg holds its temporary directory in a context manager, so the
/// directory is removed when the leg ends, including when it aborts. The twin's
/// own scratch directory was removed on the success path only, so an aborted run
/// left it behind in the project's output tree. This guard owns the directory
/// for the whole run and removes it on the way out, and it changes nothing the
/// leg reports: an abort still aborts with the same diagnostic.
///
/// The removal rides on a destructor, so it reaches the exits the process
/// observes: the normal return, where the leg has already removed the directory
/// itself, and the unwinding of a panic, where the destructor is what removes
/// it. A termination the process cannot catch reaches no destructor, so a kill
/// of the process group with SIGKILL leaves the directory behind. That ceiling
/// is the runner's and not this leg's: the runner ends a leg that outruns its
/// deadline by killing the process group, and the directory of a leg killed
/// that way stays in the output tree.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

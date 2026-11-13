//! Native twin of the authenticated server-admission verification.
mod twin;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner public-interface checks: authenticated current/legacy local-agent protocol,256immediate sequential real file-inspection tasks with changed source bytes, complete source/output hashes, distinct identities and cached-message idempotency. No retries or sleeps between fresh requests.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path());
    controller.finish();
    // The leg writes json.dumps(result, indent=2) + "\n", so the report file needs the
    // trailing newline; the count line stays on stdout only.
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("server_admission", &body, true);
    println!("{} server_admission checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let source = std::path::Path::new(root).join("actual.txt");
    std::fs::write(&source, "Actual sequential observation.")
        .unwrap_or_else(|error| panic!("could not write {}: {error}", source.display()));

    let token = format!("fixture-{}", "z".repeat(40));
    let launched = controller.event(None, "launch", obj(vec![]), root);
    let state = state(&launched).clone();
    let started = controller.event(
        Some(&state),
        "gateway_start",
        obj(vec![
            ("listen".into(), s("127.0.0.1")),
            ("port".into(), s("8080")),
            ("token".into(), s(&token)),
            ("a2a".into(), Value::Bool(true)),
        ]),
        root,
    );
    let config = field(&started, "effect")
        .get("config")
        .unwrap_or_else(|| panic!("gateway start returned no config: {}", started.to_string()))
        .clone();

    let mut passed = Vec::new();
    for version in ["1.0", "0.3"] {
        let mut ids = Vec::new();
        let mut completed = Vec::new();
        let mut fresh = Vec::new();
        let mut last_task = Value::Null;

        for index in 0..128 {
            let payload = format!("Actual source revision {version}/{index}.\n");
            std::fs::write(&source, &payload)
                .unwrap_or_else(|error| panic!("could not write {}: {error}", source.display()));
            let request = request(version, index);
            let task = send(controller, &config, &token, version, &request);
            ids.push(task.get("id").unwrap_or_else(|| panic!("task has no id")).str_().to_string());
            completed.push(
                field(&task, "status")
                    .get("state")
                    .unwrap_or_else(|| panic!("task has no status state"))
                    .str_()
                    == if version == "1.0" {
                        "TASK_STATE_COMPLETED"
                    } else {
                        "completed"
                    },
            );
            let output = field(&task, "artifacts")
                .arr()
                .first()
                .unwrap_or_else(|| panic!("task has no artifacts"))
                .get("parts")
                .unwrap_or_else(|| panic!("artifact has no parts"))
                .arr()
                .first()
                .unwrap_or_else(|| panic!("artifact has no parts"))
                .get("data")
                .unwrap_or_else(|| panic!("artifact part has no data"));
            let raw = output
                .get("output")
                .unwrap_or_else(|| panic!("output has no output text"))
                .str_();
            let digest = twin::sha256_bytes(payload.as_bytes());
            fresh.push(
                raw.contains(&digest)
                    && output
                        .get("complete")
                        .unwrap_or_else(|| panic!("output has no complete flag"))
                        .bool_()
                    && output
                        .get("returnedSha256")
                        .unwrap_or_else(|| panic!("output has no returnedSha256"))
                        .str_()
                        == twin::sha256_bytes(raw.as_bytes()),
            );
            last_task = task;
        }

        keep(
            &mut passed,
            &format!(
                "{version} sequential task acknowledgements leave capacity ready for the next task"
            ),
            completed.len() == 128 && completed.iter().all(|value| *value),
        );
        keep(
            &mut passed,
            &format!("{version} every admitted task returns its actual fresh source digest"),
            fresh.iter().all(|value| *value),
        );
        keep(
            &mut passed,
            &format!("{version} each fresh message owns a distinct actual task"),
            ids.len() == 128 && ids.iter().collect::<std::collections::BTreeSet<_>>().len() == 128,
        );

        std::fs::write(&source, "Changed after the last completed task.")
            .unwrap_or_else(|error| panic!("could not write {}: {error}", source.display()));
        let retry_request = request(version, 127);
        let cached = send(controller, &config, &token, version, &retry_request);
        keep(
            &mut passed,
            &format!(
                "{version} identical message retry preserves its historical result without replay"
            ),
            cached.get("id") == last_task.get("id")
                && cached.get("artifacts") == last_task.get("artifacts"),
        );
    }

    passed
}

fn request(version: &str, index: usize) -> Value {
    let current = version == "1.0";
    let mut message = obj(vec![
        ("messageId".into(), s(&format!("{version}-{index}"))),
        (
            "role".into(),
            s(if current { "ROLE_USER" } else { "user" }),
        ),
        (
            "parts".into(),
            Value::Arr(vec![obj(vec![(
                "data".into(),
                obj(vec![
                    ("tool".into(), s("inspect")),
                    (
                        "args".into(),
                        obj(vec![
                            ("path".into(), s("actual.txt")),
                            ("content".into(), s("complete_digest")),
                        ]),
                    ),
                ]),
            )])]),
        ),
    ]);
    if !current {
        put(&mut message, "kind", s("message"));
        let parts = field_mut(&mut message, "parts");
        let Value::Arr(items) = parts else {
            panic!("parts is not an array");
        };
        put(&mut items[0], "kind", s("data"));
    }
    obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), Value::Num(index.to_string())),
        (
            "method".into(),
            s(if current { "SendMessage" } else { "message/send" }),
        ),
        ("params".into(), obj(vec![("message".into(), message)])),
    ])
}

fn send(
    controller: &mut Controller,
    config: &Value,
    token: &str,
    version: &str,
    request: &Value,
) -> Value {
    let body = request.to_string();
    let header = format!(
        "POST /a2a HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nA2A-Version: {version}\r\nAuthorization: Bearer {token}\r\n\r\n",
        body.as_bytes().len()
    );
    let reply = controller.op(&obj(vec![
        ("op".into(), s("gateway_incoming")),
        ("config".into(), config.clone()),
        ("header".into(), s(&header)),
        ("body".into(), s(&body)),
    ]));
    let wire = reply
        .get("reply")
        .unwrap_or_else(|| panic!("gateway incoming returned no reply: {}", reply.to_string()))
        .str_();
    let response_text = wire
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("gateway reply has no HTTP body"))
        .1;
    let response = Value::parse(response_text);
    let result = response
        .get("result")
        .unwrap_or_else(|| panic!("gateway response has no result: {}", response.to_string()));
    if version == "1.0" {
        result
            .get("task")
            .unwrap_or_else(|| panic!("current gateway result has no task"))
            .clone()
    } else {
        result.clone()
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn state(value: &Value) -> &Value {
    value
        .get("state")
        .unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {}", value.to_string()))
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

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path) -> Self {
        let scratch = twin::create_scratch(root, "server-admission-");
        Self(scratch)
    }

    fn path(&self) -> &str {
        self.0
            .to_str()
            .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
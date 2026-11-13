//! Native ACP standard-stream verification twin.
mod twin;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use twin::{obj, s, Value};

const SCOPE: &str = "Owner checks through actual ACP standard streams and shared production workspace engine; no model, external server, developer report or journal in the selected workspace. Read-only ACP v1 capability scope, not full coding-agent or Android ACP client acceptance.";
const REPORT: &str = "output/checks/acp.json";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let workspace = scratch.path().join("workspace");
    let outside = scratch.path().join("outside");
    std::fs::create_dir_all(&workspace).unwrap_or_else(|error| panic!("workspace: {error}"));
    std::fs::create_dir_all(&outside).unwrap_or_else(|error| panic!("outside: {error}"));
    let sample = workspace.join("sample.txt");
    let other = workspace.join("other.txt");
    let secret = outside.join("private.txt");
    std::fs::write(&sample, "actual ACP workspace content\n").unwrap_or_else(|error| panic!("sample: {error}"));
    std::fs::write(&other, "different bytes\n").unwrap_or_else(|error| panic!("other: {error}"));
    std::fs::write(&secret, "outside selected root\n").unwrap_or_else(|error| panic!("secret: {error}"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, workspace.join("escape")).unwrap_or_else(|error| panic!("symlink: {error}"));
    let large = workspace.join("large.bin");
    let large_file = std::fs::File::create(&large).unwrap_or_else(|error| panic!("large: {error}"));
    large_file.set_len(32 * 1024 * 1024).unwrap_or_else(|error| panic!("large size: {error}"));

    let mut client = Client::spawn(&root, &workspace);
    let mut passed = Vec::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &mut client,
            &mut passed,
            &workspace,
            &outside,
            &sample,
            &other,
            &secret,
            &scratch,
        );
    }));
    client.close();
    if result.is_err() {
        std::panic::resume_unwind(result.unwrap_err());
    }
    scratch.cleanup();
    let record = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("acp", &record, true);
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains(SCOPE) {
        panic!("{REPORT} missing scope");
    }
}

fn run(
    client: &mut Client,
    passed: &mut Vec<String>,
    workspace: &Path,
    outside: &Path,
    sample: &Path,
    other: &Path,
    secret: &Path,
    scratch: &Scratch,
) {
    let root = workspace.to_string_lossy().into_owned();
    let outside_text = outside.to_string_lossy().into_owned();

    let response = client.call(1, "session/new", obj(vec![
        ("cwd".into(), s(&root)),
        ("mcpServers".into(), Value::Arr(vec![])),
    ]));
    keep(passed, "session creation requires initialization", error_code(&response) == Some(-32600));

    let response = client.call(2, "initialize", obj(vec![
        ("protocolVersion".into(), twin::n("2")),
        ("clientCapabilities".into(), obj(vec![])),
        ("clientInfo".into(), obj(vec![("name".into(), s("Fixture")), ("version".into(), s("1"))])),
    ]));
    let result = result_of(&response);
    keep(passed, "unknown requested version negotiates supported stable version", result.get("protocolVersion").is_some_and(|value| value == &twin::n("1")));
    let capabilities = result.get("agentCapabilities").unwrap_or_else(|| panic!("missing agent capabilities"));
    let not_advertised = |key: &str| {
        let value = capabilities.get(key).unwrap_or_else(|| panic!("agentCapabilities omits {key}"));
        !twin::truthy(value)
    };
    let no_prompt = not_advertised("promptCapabilities");
    let no_mcp = not_advertised("mcpCapabilities");
    let no_load = not_advertised("loadSession");
    keep(passed, "unsupported media and peer launch capabilities are not advertised", no_prompt && no_mcp && no_load);
    let session_capabilities = capabilities.get("sessionCapabilities").unwrap_or_else(|| panic!("missing session capabilities"));
    let extensions = ["list", "resume", "close"].iter().all(|key| session_capabilities.get(key).is_some());
    keep(passed, "implemented session extensions are advertised", extensions);

    let response = client.call(3, "initialize", obj(vec![("protocolVersion".into(), twin::n("1"))]));
    keep(passed, "duplicate initialization cannot replace connection state", response.get("error").is_some());

    let response = client.call(4, "session/new", obj(vec![
        ("cwd".into(), s(&outside_text)),
        ("mcpServers".into(), Value::Arr(vec![])),
    ]));
    keep(passed, "client cannot widen startup workspace", error_code(&response) == Some(-32602));

    let response = client.call(5, "session/new", obj(vec![
        ("cwd".into(), s(&root)),
        ("mcpServers".into(), Value::Arr(vec![obj(vec![
            ("name".into(), s("Peer")),
            ("command".into(), s("/bin/false")),
            ("args".into(), Value::Arr(vec![])),
            ("env".into(), Value::Arr(vec![])),
        ])])),
    ]));
    keep(passed, "unadvertised external server launch is refused", response.get("error").is_some());

    let response = client.call(6, "session/new", obj(vec![
        ("cwd".into(), s(&root)),
        ("mcpServers".into(), Value::Arr(vec![])),
        ("additionalDirectories".into(), Value::Arr(vec![s(&outside_text)])),
    ]));
    keep(passed, "additional roots cannot be silently accepted", response.get("error").is_some());

    let session = client.session(7, workspace);
    let (response, frames) = client.prompt(8, &session, "/inspect sample.txt", vec![]);
    let updates: Vec<&Value> = frames.iter().filter_map(|frame| {
        frame.get("params").and_then(|params| params.get("update"))
    }).collect();
    let complete = updates.iter().find(|update| update.get("sessionUpdate") == Some(&s("tool_call_update"))).unwrap_or_else(|| panic!("missing tool call completion"));
    let raw = complete.get("rawOutput").unwrap_or_else(|| panic!("missing raw output"));
    let body: String = updates.iter().filter_map(|update| {
        if update.get("sessionUpdate") == Some(&s("agent_message_chunk")) {
            update.get("content").and_then(|content| content.get("text")).map(Value::str_)
        } else {
            None
        }
    }).collect();
    // The leg indexes complete["rawOutput"]["output"], so a missing field is a KeyError there;
    // a defaulted "" would let an absent field read as a real empty output.
    let raw_output = raw.get("output").unwrap_or_else(|| panic!("missing raw output text")).str_();
    keep(passed, "real engine output reaches ACP agent messages", complete.get("status") == Some(&s("completed")) && body == raw_output && body.contains("sample.txt"));
    let digest = twin::sha256_bytes(body.as_bytes());
    keep(passed, "returned output digest describes actual returned bytes", raw.get("complete") == Some(&twin::b(true)) && raw.get("returnedBytes") == Some(&twin::n(&body.as_bytes().len().to_string())) && raw.get("returnedSha256") == Some(&s(&digest)));
    let started = updates.iter().find(|update| update.get("sessionUpdate") == Some(&s("tool_call"))).unwrap_or_else(|| panic!("missing tool start"));
    keep(passed, "tool start and completion precede prompt completion", started.get("status") == Some(&s("in_progress")) && result_of(&response).get("stopReason") == Some(&s("end_turn")));

    let response = client.call(9, "session/list", obj(vec![]));
    let expected = obj(vec![
        ("sessionId".into(), s(&session)),
        ("cwd".into(), s(&root)),
        ("title".into(), s("Workspace observations")),
    ]);
    keep(passed, "session list returns owned selected-root session", result_of(&response).get("sessions") == Some(&Value::Arr(vec![expected])));

    let response = client.call(10, "session/resume", obj(vec![
        ("sessionId".into(), s(&session)),
        ("cwd".into(), s(&root)),
        ("mcpServers".into(), Value::Arr(vec![])),
    ]));
    keep(passed, "existing in-process session resumes without fabricated history", result_of(&response) == &obj(vec![]));

    let links = vec![
        obj(vec![("type".into(), s("resource_link")), ("name".into(), s("sample.txt")), ("uri".into(), s(&file_uri(sample)))]),
        obj(vec![("type".into(), s("resource_link")), ("name".into(), s("other.txt")), ("uri".into(), s(&file_uri(other)))]),
    ];
    let (_, frames) = client.prompt(11, &session, "/compare", links);
    keep(passed, "local resource links execute actual comparison", frames.iter().any(|frame| frame.get("params").and_then(|params| params.get("update")).and_then(|update| update.get("status")) == Some(&s("completed"))));

    let command = serde_json(&obj(vec![
        ("tool".into(), s("inspect")),
        ("args".into(), obj(vec![("path".into(), s("sample.txt")), ("content".into(), s("complete_digest"))])),
    ]));
    let (_, frames) = client.prompt(12, &session, &command, vec![]);
    let sample_digest = twin::sha256_bytes(&std::fs::read(sample).unwrap_or_else(|error| panic!("read sample: {error}")));
    keep(passed, "JSON command uses actual whole-file digest", frames.iter().any(|frame| frame.to_string().contains(&sample_digest)));

    let refusals = [
        "/inspect ../outside/private.txt",
        "/inspect escape",
        "/inspect missing.txt",
    ];
    for (offset, text) in refusals.iter().enumerate() {
        let number = 20 + offset as i64;
        let (response, frames) = client.prompt(number, &session, text, vec![]);
        let failed = frames.iter().any(|frame| frame.get("params").and_then(|params| params.get("update")).and_then(|update| update.get("status")) == Some(&s("failed"))) || response.get("error").is_some();
        keep(passed, &format!("scoped read refusal {text}"), failed);
        keep(passed, &format!("refusal exposes no outside content {text}"), frames.iter().all(|frame| !frame.to_string().contains("outside selected root")));
    }

    let invalid_commands = [
        serde_json(&obj(vec![("tool".into(), s("write")), ("args".into(), obj(vec![("path".into(), s("created.txt")), ("content".into(), s("bad"))]))])),
        serde_json(&obj(vec![("tool".into(), s("inspect")), ("args".into(), obj(vec![("path".into(), s("sample.txt")), ("root".into(), s(&outside_text))]))])),
        "/compare not-json".to_string(),
    ];
    for (offset, text) in invalid_commands.iter().enumerate() {
        let number = 30 + offset as i64;
        let (response, _) = client.prompt(number, &session, text, vec![]);
        keep(passed, &format!("unsupported authority or malformed command refused {}", number), response.get("error").is_some());
    }
    keep(passed, "refused write did not create a file", !workspace.join("created.txt").exists());

    let (response, _) = client.prompt(34, &session, "/inspect", vec![obj(vec![
        ("type".into(), s("resource_link")),
        ("name".into(), s("private")),
        ("uri".into(), s(&file_uri(secret))),
    ])]);
    keep(passed, "outside resource links are refused", response.get("error").is_some());

    let response = client.call(35, "session/prompt", obj(vec![
        ("sessionId".into(), s(&session)),
        ("prompt".into(), Value::Arr(vec![obj(vec![
            ("type".into(), s("audio")),
            ("mimeType".into(), s("audio/wav")),
            ("data".into(), s("AAAA")),
        ])])),
    ]));
    keep(passed, "unadvertised audio is not interpreted as text", response.get("error").is_some());

    let (response, _) = client.prompt(36, &session, "/inspect sample.txt", vec![]);
    keep(passed, "unrelated valid operation still works after scoped failures", result_of(&response).get("stopReason") == Some(&s("end_turn")));

    client.raw(b"{not-json}\n");
    let response = client.receive();
    keep(passed, "malformed JSON receives a parse error", error_code(&response) == Some(-32700));

    let oversized = vec![b'x'; 64 * 1024 + 1];
    client.raw(&oversized);
    client.raw(b"\n");
    let response = client.receive();
    keep(passed, "oversized frame is drained within declared bound", error_code(&response) == Some(-32700));

    let response = client.call(37, "session/list", obj(vec![]));
    keep(passed, "valid request after oversized frame remains usable", response.to_string().contains(&session));

    let session2 = client.session(38, workspace);
    let command = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), twin::n("40")),
        ("method".into(), s("session/prompt")),
        ("params".into(), obj(vec![
            ("sessionId".into(), s(&session)),
            ("prompt".into(), Value::Arr(vec![obj(vec![
                ("type".into(), s("text")),
                ("text".into(), s("/inspect {\"path\":\"large.bin\",\"content\":\"complete_digest\"}")),
            ])])),
        ])),
    ]);
    let cancel = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("method".into(), s("session/cancel")),
        ("params".into(), obj(vec![("sessionId".into(), s(&session))])),
    ]);
    // Leg writes the prompt and the cancel as one newline-delimited buffer
    // (acp.py: the raw() of json.dumps(command)+'\n'+json.dumps(cancel)+'\n').
    // Separate writes with no newline never complete a frame, so the child
    // waits in its line reader and this side times out in receive().
    client.raw_frames(&[&command, &cancel]);
    let (response, frames) = client.until(40);
    keep(passed, "actual in-flight work cancels with stable prompt stop reason", result_of(&response).get("stopReason") == Some(&s("cancelled")));
    keep(passed, "cancelled work publishes no agent result", !frames.iter().any(|frame| frame.get("params").and_then(|params| params.get("update")).and_then(|update| update.get("sessionUpdate")) == Some(&s("agent_message_chunk"))));

    let (response, _) = client.prompt(41, &session2, "/inspect sample.txt", vec![]);
    keep(passed, "one session cancellation leaves another session usable", result_of(&response).get("stopReason") == Some(&s("end_turn")));

    let mut command = command;
    if let Value::Obj(fields) = &mut command {
        if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "id") {
            *value = twin::n("42");
        }
    }
    let cancel = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("method".into(), s("$/cancel_request")),
        ("params".into(), obj(vec![("requestId".into(), twin::n("42"))])),
    ]);
    client.raw_frames(&[&command, &cancel]);
    let (response, _) = client.until(42);
    keep(passed, "request cancellation retains original request identity", result_of(&response).get("stopReason") == Some(&s("cancelled")));

    let mut command = command;
    if let Value::Obj(fields) = &mut command {
        if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "id") {
            *value = twin::n("43");
        }
    }
    let closing = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), twin::n("44")),
        ("method".into(), s("session/close")),
        ("params".into(), obj(vec![("sessionId".into(), s(&session))])),
    ]);
    client.raw_frames(&[&command, &closing]);
    let (response, frames) = client.until(44);
    keep(passed, "session close waits for owned turn cancellation", result_of(&response) == &obj(vec![]) && frames.iter().any(|frame| frame.get("id") == Some(&twin::n("43")) && result_of(frame).get("stopReason") == Some(&s("cancelled"))));

    let response = client.call(45, "session/resume", obj(vec![
        ("sessionId".into(), s(&session)),
        ("cwd".into(), s(&root)),
        ("mcpServers".into(), Value::Arr(vec![])),
    ]));
    keep(passed, "closed session cannot revive", response.get("error").is_some());

    let response = client.call(46, "session/close", obj(vec![("sessionId".into(), s(&session2))]));
    keep(passed, "idle close releases session capacity", result_of(&response) == &obj(vec![]));

    let mut owned = Vec::new();
    for index in 0..8 {
        owned.push(client.session(50 + index as i64, workspace));
    }
    let response = client.call(60, "session/new", obj(vec![
        ("cwd".into(), s(&root)),
        ("mcpServers".into(), Value::Arr(vec![])),
    ]));
    keep(passed, "bounded session admission reports actual capacity", response.get("error").is_some());

    let original = scratch.path().join("original");
    std::fs::rename(workspace, &original).unwrap_or_else(|error| panic!("rename workspace: {error}"));
    std::fs::create_dir_all(workspace).unwrap_or_else(|error| panic!("replacement workspace: {error}"));
    std::fs::write(sample, "replacement root\n").unwrap_or_else(|error| panic!("replacement sample: {error}"));
    let (response, _) = client.prompt(61, owned.last().unwrap(), "/inspect sample.txt", vec![]);
    let message = response.get("error").and_then(|error| error.get("message")).map(Value::str_).unwrap_or("");
    keep(passed, "replaced startup root cannot feed stale session", message.contains("identity changed"));
    keep(passed, "replacement root contents are not adopted", client.frames.iter().all(|frame| !frame.to_string().contains("replacement root")));
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn result_of(value: &Value) -> &Value {
    value.get("result").unwrap_or_else(|| panic!("missing result: {}", value.to_string()))
}

fn error_code(value: &Value) -> Option<i64> {
    value.get("error").and_then(|error| error.get("code")).and_then(|code| match code {
        Value::Num(number) => number.parse().ok(),
        _ => None,
    })
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy())
}

fn serde_json(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Num(number) => out.push_str(number),
        Value::Str(text) => write_json_string(text, out),
        Value::Arr(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Obj(fields) => {
            out.push('{');
            for (index, (key, item)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_string(key, out);
                out.push(':');
                write_json(item, out);
            }
            out.push('}');
        }
    }
}

fn write_json_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    receiver: Receiver<Value>,
    frames: Vec<Value>,
}

impl Client {
    fn spawn(root: &Path, workspace: &Path) -> Self {
        let binary = root.join("output/headless/RLLM");
        let mut child = Command::new(&binary)
            .arg("--acp")
            .arg("--root")
            .arg(workspace)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", binary.display()));
        let stdin = child.stdin.take().unwrap_or_else(|| panic!("ACP stdin was not piped"));
        let stdout = child.stdout.take().unwrap_or_else(|| panic!("ACP stdout was not piped"));
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = line.unwrap_or_else(|error| panic!("ACP read: {error}"));
                let value = Value::parse(&line);
                if sender.send(value).is_err() {
                    return;
                }
            }
        });
        Self { child, stdin: Some(stdin), receiver, frames: Vec::new() }
    }

    fn raw(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap_or_else(|| panic!("ACP stdin is closed"));
        stdin.write_all(bytes).unwrap_or_else(|error| panic!("ACP write: {error}"));
        stdin.flush().unwrap_or_else(|error| panic!("ACP flush: {error}"));
    }

    // One write, newline after each object, matching Client.raw in the leg.
    fn raw_frames(&mut self, frames: &[&Value]) {
        let mut bytes = Vec::new();
        for frame in frames {
            bytes.extend(serde_json(frame).into_bytes());
            bytes.push(b'\n');
        }
        self.raw(&bytes);
    }

    fn send(&mut self, id: Option<i64>, method: &str, params: Value) {
        let mut fields = vec![
            ("jsonrpc".into(), s("2.0")),
            ("method".into(), s(method)),
            ("params".into(), params),
        ];
        if let Some(id) = id {
            fields.push(("id".into(), twin::n(&id.to_string())));
        }
        let mut bytes = serde_json(&obj(fields)).into_bytes();
        bytes.push(b'\n');
        self.raw(&bytes);
    }

    fn receive(&mut self) -> Value {
        let value = self.receiver.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|error| match error {
            RecvTimeoutError::Timeout => panic!("ACP exited before replying"),
            RecvTimeoutError::Disconnected => panic!("ACP exited before replying"),
        });
        self.frames.push(value.clone());
        value
    }

    fn until(&mut self, id: i64) -> (Value, Vec<Value>) {
        let start = self.frames.len();
        loop {
            let value = self.receive();
            if value.get("id") == Some(&twin::n(&id.to_string())) {
                return (value, self.frames[start..].to_vec());
            }
        }
    }

    fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(Some(id), method, params);
        self.until(id).0
    }

    fn session(&mut self, id: i64, root: &Path) -> String {
        let response = self.call(id, "session/new", obj(vec![
            ("cwd".into(), s(&root.to_string_lossy())),
            ("mcpServers".into(), Value::Arr(vec![])),
        ]));
        result_of(&response).get("sessionId").unwrap_or_else(|| panic!("missing session id")).str_().to_string()
    }

    fn prompt(&mut self, id: i64, session: &str, text: &str, links: Vec<Value>) -> (Value, Vec<Value>) {
        let mut prompt = vec![obj(vec![("type".into(), s("text")), ("text".into(), s(text))])];
        prompt.extend(links);
        self.send(Some(id), "session/prompt", obj(vec![
            ("sessionId".into(), s(session)),
            ("prompt".into(), Value::Arr(prompt)),
        ]));
        self.until(id)
    }

    fn close(&mut self) {
        self.stdin.take();
        let status = self.child.wait().unwrap_or_else(|error| panic!("ACP wait: {error}"));
        if !status.success() {
            let mut stderr = String::new();
            if let Some(mut pipe) = self.child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!("ACP exited unsuccessfully: {stderr}");
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = root.join("output").join(format!("acp-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("could not create {}: {error}", path.display()));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.cleanup();
    }
}
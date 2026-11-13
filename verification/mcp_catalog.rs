mod twin;

use twin::{b64_encode, obj, s, Controller, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const REPORT: &str = "output/checks/mcp-catalog.json";

struct Peer {
    address: SocketAddr,
    mode: Arc<Mutex<String>>,
    requests: Arc<Mutex<usize>>,
}

impl Peer {
    fn start() -> (Self, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind MCP peer: {error}"));
        let address = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}"));
        let mode = Arc::new(Mutex::new("original".to_string()));
        let requests = Arc::new(Mutex::new(0usize));
        let worker_mode = Arc::clone(&mode);
        let worker_requests = Arc::clone(&requests);
        let worker = thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(_) => break,
                };
                let mode = Arc::clone(&worker_mode);
                let requests = Arc::clone(&worker_requests);
                thread::spawn(move || {
                    if let Err(error) = serve(stream, mode, requests) {
                        panic!("MCP peer request failed: {error}");
                    }
                });
            }
        });
        (Self { address, mode, requests }, worker)
    }

    fn set_mode(&self, mode: &str) {
        *self.mode.lock().unwrap_or_else(|error| panic!("peer mode lock: {error}")) = mode.to_string();
    }

    fn request_count(&self) -> usize {
        *self.requests
            .lock()
            .unwrap_or_else(|error| panic!("peer requests lock: {error}"))
    }
}

fn serve(mut stream: TcpStream, mode: Arc<Mutex<String>>, requests: Arc<Mutex<usize>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let request = read_http_request(&mut stream)?;
    let mut headers = HashMap::new();
    for line in request.headers.split("\r\n") {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
        }
    }
    if request.path != "/mcp" || headers.get("authorization").map(String::as_str) != Some("Bearer catalog-test") {
        panic!("invalid MCP request {} {:?}", request.path, headers.get("authorization"));
    }
    let message = Value::parse(&String::from_utf8(request.body).expect("UTF-8 request"));
    let method = message.get("method").unwrap_or_else(|| panic!("missing method")).str_().to_string();
    let params = message.get("params").cloned().unwrap_or(Value::Obj(vec![]));
    *requests
        .lock()
        .unwrap_or_else(|error| panic!("peer requests lock: {error}")) += 1;

    let mode = mode.lock().unwrap_or_else(|error| panic!("peer mode lock: {error}")).clone();
    let (status, reason, body) = if method == "notifications/initialized" {
        (202, "Accepted", Vec::new())
    } else if method == "initialize" {
        let result = obj(vec![
            ("protocolVersion".into(), s("2025-11-25")),
            ("capabilities".into(), obj(vec![("tools".into(), obj(vec![]))])),
            ("serverInfo".into(), obj(vec![("name".into(), s("Catalog peer")), ("version".into(), s("1"))])),
        ]);
        (200, "OK", rpc(&message, result))
    } else if method == "tools/list" {
        (200, "OK", rpc(&message, tools_list(&mode, &params)))
    } else {
        panic!("unexpected MCP method: {method}");
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn rpc(message: &Value, result: Value) -> Vec<u8> {
    obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), message.get("id").cloned().unwrap_or(Value::Null)),
        ("result".into(), result),
    ])
    .to_string()
    .into_bytes()
}

fn tools_list(mode: &str, params: &Value) -> Value {
    let empty_params = matches!(params, Value::Obj(fields) if fields.is_empty()) || matches!(params, Value::Null);
    let second_page = params == &obj(vec![("cursor".into(), s("second-page"))]);
    match mode {
        "empty" => page(vec![]),
        "original" => page(vec![tool("original")]),
        "duplicate-page" => page(vec![tool("first"), tool("first")]),
        _ if empty_params => obj(vec![
            ("tools".into(), Value::Arr(vec![tool(if mode == "duplicate-original" { "original" } else { "first" })])),
            ("nextCursor".into(), s("second-page")),
        ]),
        _ if second_page => match mode {
            "failure" => page(vec![obj(vec![("name".into(), s("invalid")), ("inputSchema".into(), Value::Bool(false))])]),
            "duplicate-chain" | "duplicate-original" => {
                let name = if mode == "duplicate-original" { "original" } else { "first" };
                page(vec![obj(vec![
                    ("name".into(), s(name)),
                    ("inputSchema".into(), obj(vec![("type".into(), s("object")), ("required".into(), Value::Arr(vec![s("different")]))])),
                ])])
            }
            "case-sensitive" => page(vec![tool("First")]),
            _ => page(vec![tool("second")]),
        },
        _ => panic!("unexpected MCP list params: {}", params.to_string()),
    }
}

fn page(tools: Vec<Value>) -> Value {
    obj(vec![("tools".into(), Value::Arr(tools))])
}

fn tool(name: &str) -> Value {
    obj(vec![("name".into(), s(name)), ("inputSchema".into(), obj(vec![("type".into(), s("object"))]))])
}

struct HttpRequest {
    path: String,
    headers: String,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<HttpRequest> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF before headers"));
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header_text = String::from_utf8(bytes[..header_end].to_vec()).expect("UTF-8 headers");
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let path = request_line.split_whitespace().nth(1).unwrap_or_default().to_string();
    let headers = lines.collect::<Vec<_>>().join("\r\n");
    let length = headers
        .split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF before body"));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(HttpRequest {
        path,
        headers,
        body: bytes[header_end..header_end + length].to_vec(),
    })
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let (peer, _worker) = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path(), &peer);
    controller.finish();
    let names: Vec<Value> = passed.iter().map(|name| s(name)).collect();
    let requests = peer.request_count();
    // The Python leg writes json.dumps(..., indent=2) plus a newline. The target file ends with that newline.
    twin::report_value(
        "mcp_catalog",
        &obj(vec![
            ("passed".into(), twin::n(&names.len().to_string())),
            ("checks".into(), Value::Arr(names)),
            ("requests".into(), twin::n(&requests.to_string())),
        ]),
        true,
    );
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains("\"passed\": ") || !written.contains("\"requests\": ") {
        panic!("{REPORT} missing report fields");
    }
    println!("{requests} requests, {} checks", passed.len());
}

fn run(controller: &mut Controller, root: &str, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    let endpoint = format!("http://{}/mcp", peer.address);
    let mut step = event(controller, None, "resume", obj(vec![]), root);
    step = event(controller, Some(state_of(&step)), "mcp_client_settings", obj(vec![]), root);
    step = event(
        controller,
        Some(state_of(&step)),
        "mcp_client_save",
        obj(vec![
            ("endpoint".into(), s(&endpoint)),
            ("key".into(), s("catalog-test")),
            ("access".into(), Value::Bool(true)),
        ]),
        root,
    );
    let (step, _) = finish_page(controller, state_of(&step), root);
    let prior = state_of(&step).clone();
    keep(&mut passed, "original catalog published after successful discovery", names(&prior) == ["original"]);

    peer.set_mode("failure");
    let (pending, _) = finish_page(controller, &prior, root);
    let (failed, late) = exchange(controller, &pending, root);
    let failed_state = state_of(&failed).clone();
    keep(&mut passed, "invalid later page cannot replace the last complete catalog", names(&failed_state) == ["original"]);
    keep(
        &mut passed,
        "refused page reports scoped failure and settles discovery",
        truthy(failed_state.get("notice_error")) && !truthy(failed_state.get("busy")),
    );
    keep(
        &mut passed,
        "failed refresh preserves exact published catalog text",
        failed_state.get("mcp_client_tools") == prior.get("mcp_client_tools"),
    );
    keep(&mut passed, "failed refresh discards staged pages", absent(failed_state.get("mcp_client_listing")));
    keep(&mut passed, "pending first page never appeared in the published list", names(state_of(&pending)) == ["original"]);

    let legacy = with_catalog(&prior, vec![tool("legacy"), tool("legacy"), tool("other")]);
    let catalog = event(controller, Some(&legacy), "mcp_tools", obj(vec![]), root);
    let buttons = modal_buttons(&catalog);
    keep(
        &mut passed,
        "ambiguous legacy rows are disabled and unique rows remain selectable",
        button_enabled(&buttons, "mcp_tool:0") == Some(false)
            && button_enabled(&buttons, "mcp_tool:1") == Some(false)
            && button_enabled(&buttons, "mcp_tool:2") != Some(false),
    );
    let rejected = event(controller, Some(state_of(&catalog)), "mcp_tool:1", obj(vec![]), root);
    keep(
        &mut passed,
        "an explicit legacy selection is also refused",
        absent(rejected.get("effect"))
            && truthy(state_of(&rejected).get("notice_error"))
            && state_of(&rejected).get("notice").is_some_and(|notice| notice.str_().contains("duplicate tool name")),
    );
    let call = event(controller, Some(state_of(&catalog)), "mcp_client_call", obj(vec![]), root);
    let rejected = event(
        controller,
        Some(state_of(&call)),
        "mcp_client_review",
        obj(vec![("tool".into(), s("legacy")), ("arguments".into(), s("{}"))]),
        root,
    );
    keep(
        &mut passed,
        "manual entry cannot bypass the legacy duplicate refusal",
        absent(rejected.get("effect"))
            && truthy(state_of(&rejected).get("notice_error"))
            && absent(state_of(&rejected).get("mcp_client_pending")),
    );
    let selected = event(controller, Some(state_of(&catalog)), "mcp_tool:2", obj(vec![]), root);
    let review = event(
        controller,
        Some(state_of(&selected)),
        "mcp_client_review",
        obj(vec![("arguments".into(), s("{}"))]),
        root,
    );
    keep(
        &mut passed,
        "a distinct tool in older state can still be reviewed",
        state_of(&review).get("dialog").is_some_and(|value| value.str_() == "mcp_client_review"),
    );

    peer.set_mode("duplicate-original");
    let (pending, _) = finish_page(controller, &failed_state, root);
    let (newly_ambiguous, _) = exchange(controller, &pending, root);
    keep(
        &mut passed,
        "a newly duplicated name keeps the complete earlier catalog visible",
        names(state_of(&newly_ambiguous)) == ["original"],
    );
    let refreshed = event(controller, Some(state_of(&newly_ambiguous)), "mcp_tools", obj(vec![]), root);
    keep(
        &mut passed,
        "the retained row for that name is now disabled",
        button_enabled(&modal_buttons(&refreshed), "mcp_tool:0") == Some(false),
    );
    let manual = event(controller, Some(state_of(&refreshed)), "mcp_client_call", obj(vec![]), root);
    let blocked = event(
        controller,
        Some(state_of(&manual)),
        "mcp_client_review",
        obj(vec![("tool".into(), s("original")), ("arguments".into(), s("{}"))]),
        root,
    );
    keep(
        &mut passed,
        "manual calls cannot bypass a newly observed duplicate",
        absent(blocked.get("effect")) && truthy(state_of(&blocked).get("notice_error")),
    );
    let ordinary = event(
        controller,
        Some(state_of(&manual)),
        "mcp_client_review",
        obj(vec![("tool".into(), s("other")), ("arguments".into(), s("{}"))]),
        root,
    );
    keep(
        &mut passed,
        "a different exact tool remains available on the same server",
        state_of(&ordinary).get("dialog").is_some_and(|value| value.str_() == "mcp_client_review"),
    );

    let server_list = event(controller, Some(state_of(&newly_ambiguous)), "mcp_client_connections", obj(vec![]), root);
    let adding = event(controller, Some(state_of(&server_list)), "mcp_client_new", obj(vec![]), root);
    let second = event(
        controller,
        Some(state_of(&adding)),
        "mcp_client_save",
        obj(vec![
            ("endpoint".into(), s(&format!("http://{}/other", peer.address))),
            ("key".into(), s("second-test")),
            ("access".into(), Value::Bool(true)),
        ]),
        root,
    );
    let manual = event(controller, Some(state_of(&second)), "mcp_client_call", obj(vec![]), root);
    let usable = event(
        controller,
        Some(state_of(&manual)),
        "mcp_client_review",
        obj(vec![("tool".into(), s("original")), ("arguments".into(), s("{}"))]),
        root,
    );
    keep(
        &mut passed,
        "a different saved connection can independently use the same exact name",
        state_of(&usable).get("dialog").is_some_and(|value| value.str_() == "mcp_client_review"),
    );
    let server_list = event(controller, Some(state_of(&second)), "mcp_client_connections", obj(vec![]), root);
    let selected_server = event(controller, Some(state_of(&server_list)), "mcp_client_select:0", obj(vec![]), root);
    let restored = event(controller, Some(state_of(&selected_server)), "resume", obj(vec![]), root);
    let manual = event(controller, Some(state_of(&restored)), "mcp_client_call", obj(vec![]), root);
    let blocked = event(
        controller,
        Some(state_of(&manual)),
        "mcp_client_review",
        obj(vec![("tool".into(), s("original")), ("arguments".into(), s("{}"))]),
        root,
    );
    keep(
        &mut passed,
        "switching away and back and restarting retain the duplicate protection",
        absent(blocked.get("effect")) && truthy(state_of(&blocked).get("notice_error")),
    );

    peer.set_mode("original");
    let (clean, _) = finish_page(controller, state_of(&blocked), root);
    let selected = event(controller, Some(state_of(&clean)), "mcp_tool:0", obj(vec![]), root);
    let review = event(
        controller,
        Some(state_of(&selected)),
        "mcp_client_review",
        obj(vec![("arguments".into(), s("{}"))]),
        root,
    );
    keep(
        &mut passed,
        "a complete valid refresh makes the name available again",
        state_of(&review).get("dialog").is_some_and(|value| value.str_() == "mcp_client_review"),
    );

    peer.set_mode("duplicate-chain");
    let (pending, _) = finish_page(controller, &failed_state, root);
    let (collision, _) = exchange(controller, &pending, root);
    let collision_state = state_of(&collision).clone();
    keep(
        &mut passed,
        "repeated names with different schemas cannot replace the last complete catalog",
        names(&collision_state) == ["original"],
    );
    keep(
        &mut passed,
        "cross-page duplicate settles only the discovery with an explicit diagnostic",
        truthy(collision_state.get("notice_error"))
            && collision_state.get("notice").is_some_and(|notice| notice.str_().to_ascii_lowercase().contains("tool name"))
            && !truthy(collision_state.get("busy"))
            && absent(collision_state.get("mcp_client_listing")),
    );

    peer.set_mode("duplicate-page");
    let (collision, _) = finish_page(controller, &collision_state, root);
    keep(
        &mut passed,
        "repeated identical entries within one page are also refused",
        names(state_of(&collision)) == ["original"] && truthy(state_of(&collision).get("notice_error")),
    );

    peer.set_mode("case-sensitive");
    let (first, _) = finish_page(controller, state_of(&collision), root);
    let (distinct, _) = exchange(controller, &first, root);
    keep(&mut passed, "case-distinct names have separate wire identities", names(state_of(&distinct)) == ["first", "First"]);

    peer.set_mode("success");
    let (pending, first_reply) = finish_page(controller, &prior, root);
    let request = {
        let effect = pending.get("effect").unwrap_or_else(|| panic!("missing network effect"));
        let wire = effect.get("wire").unwrap_or_else(|| panic!("missing wire")).str_();
        let body = wire.split("\r\n\r\n").nth(1).unwrap_or_else(|| panic!("missing HTTP body"));
        Value::parse(body)
    };
    keep(
        &mut passed,
        "cursor sent on the actual continuation request",
        request.get("params") == Some(&obj(vec![("cursor".into(), s("second-page"))])),
    );
    let stale = event(controller, Some(state_of(&pending)), "network_result", first_reply, root);
    keep(
        &mut passed,
        "stale first callback does not disturb the active continuation",
        state_of(&stale) == state_of(&pending) && absent(stale.get("effect")),
    );
    let (done, _) = exchange(controller, &pending, root);
    let done_state = state_of(&done).clone();
    keep(
        &mut passed,
        "both pages replace the prior catalog together",
        names(&done_state) == ["first", "second"] && !truthy(done_state.get("busy")),
    );
    let tools_text = done_state.get("mcp_client_tools").map(|value| value.str_().to_string()).unwrap_or_default();
    let modal_text = done.get("ui").and_then(|ui| ui.get("modal")).map(Value::to_string).unwrap_or_default();
    keep(
        &mut passed,
        "the finished list and UI expose both current tools",
        ["first", "second"].iter().all(|name| tools_text.contains(name) && modal_text.contains(name)),
    );
    let (pending, _) = finish_page(controller, &done_state, root);
    let cancelled = event(controller, Some(state_of(&pending)), "cancel", obj(vec![]), root);
    keep(
        &mut passed,
        "cancel keeps published list and drops the pending catalog",
        names(state_of(&cancelled)) == ["first", "second"] && absent(state_of(&cancelled).get("mcp_client_listing")),
    );
    let resumed = event(controller, Some(state_of(&pending)), "resume", obj(vec![]), root);
    keep(
        &mut passed,
        "restart keeps published list and drops the pending catalog",
        names(state_of(&resumed)) == ["first", "second"] && absent(state_of(&resumed).get("mcp_client_listing")),
    );
    let stale = event(controller, Some(state_of(&resumed)), "network_result", late, root);
    keep(
        &mut passed,
        "old response after restart cannot publish or dispatch",
        names(state_of(&stale)) == ["first", "second"] && absent(stale.get("effect")),
    );

    peer.set_mode("empty");
    let (empty, _) = finish_page(controller, state_of(&resumed), root);
    keep(
        &mut passed,
        "a successful empty catalog replaces the last complete list",
        names(state_of(&empty)).is_empty() && !truthy(state_of(&empty).get("busy")),
    );
    keep(&mut passed, "empty discovery leaves no staged pages", absent(state_of(&empty).get("mcp_client_listing")));
    if peer.request_count() == 0 {
        panic!("MCP peer received no requests");
    }
    passed
}

fn finish_page(controller: &mut Controller, state: &Value, root: &str) -> (Value, Value) {
    let opened = start(controller, state, root);
    exchange(controller, &opened, root)
}

fn start(controller: &mut Controller, state: &Value, root: &str) -> Value {
    let mut step = event(controller, Some(state), "mcp_client_list", obj(vec![]), root);
    for _ in 0..2 {
        step = exchange(controller, &step, root).0;
    }
    step
}

fn exchange(controller: &mut Controller, step: &Value, root: &str) -> (Value, Value) {
    let effect = step.get("effect").unwrap_or_else(|| panic!("missing network effect: {}", step.to_string()));
    if effect.get("kind").is_none_or(|kind| kind.str_() != "network") {
        panic!("expected network effect: {}", step.to_string());
    }
    let host = effect.get("host").unwrap_or_else(|| panic!("missing host")).str_().to_string();
    let port = port_of(effect.get("port").unwrap_or_else(|| panic!("missing port")));
    let wire = effect.get("wire").unwrap_or_else(|| panic!("missing wire")).str_().to_string();
    let purpose = effect.get("purpose").cloned().unwrap_or(Value::Null);
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("connect MCP peer: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("timeout: {error}"));
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("timeout: {error}"));
    stream.write_all(wire.as_bytes()).unwrap_or_else(|error| panic!("write MCP request: {error}"));
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap_or_else(|error| panic!("read MCP response: {error}"));
    let inputs = obj(vec![
        ("purpose".into(), purpose),
        ("data".into(), s(&b64_encode(&raw))),
    ]);
    let result = event(controller, Some(state_of(step)), "network_result", inputs.clone(), root);
    (result, inputs)
}

fn port_of(value: &Value) -> u16 {
    let text = match value {
        Value::Num(text) | Value::Str(text) => text.as_str(),
        other => panic!("invalid port: {}", other.to_string()),
    };
    text.parse::<u16>().unwrap_or_else(|error| panic!("invalid port: {error}"))
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state, action, inputs, root)
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn names(state: &Value) -> Vec<String> {
    let catalog = state.get("mcp_client_catalog");
    if absent(catalog) {
        return Vec::new();
    }
    catalog
        .unwrap_or_else(|| panic!("catalog missing"))
        .arr()
        .iter()
        .map(|entry| entry.get("name").unwrap_or_else(|| panic!("catalog entry has no name")).str_().to_string())
        .collect()
}

fn with_catalog(state: &Value, catalog: Vec<Value>) -> Value {
    let Value::Obj(fields) = state else {
        panic!("expected state object");
    };
    let mut fields = fields.clone();
    if let Some(entry) = fields.iter_mut().find(|(name, _)| name == "mcp_client_catalog") {
        entry.1 = Value::Arr(catalog);
    } else {
        fields.push(("mcp_client_catalog".into(), Value::Arr(catalog)));
    }
    Value::Obj(fields)
}

fn modal_buttons(value: &Value) -> Vec<Value> {
    value
        .get("ui")
        .and_then(|ui| ui.get("modal"))
        .and_then(|modal| modal.get("buttons"))
        .map(Value::arr)
        .unwrap_or_else(|| panic!("missing modal buttons"))
        .clone()
}

fn button_enabled(buttons: &[Value], event_name: &str) -> Option<bool> {
    buttons
        .iter()
        .find(|button| button.get("event").is_some_and(|value| value.str_() == event_name))
        .and_then(|button| button.get("enabled"))
        .map(Value::bool_)
}

fn absent(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Null) => false,
        Some(Value::Str(value)) => !value.is_empty(),
        Some(Value::Num(value)) => value != "0",
        Some(Value::Arr(value)) => !value.is_empty(),
        Some(Value::Obj(value)) => !value.is_empty(),
        None => false,
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
        Self(twin::create_scratch(root, "mcp-catalog-checks"))
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

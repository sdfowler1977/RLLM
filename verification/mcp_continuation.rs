//! Model-directed image evidence through the local HTTP and media paths.
mod twin;

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{n, obj, s, Controller, Value};

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create();
    let workspace = scratch.path().to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let peer = Peer::start();
    let mut controller = Controller::spawn(&project_text);
    let (passed, mcp_requests, model_requests) = run(&mut controller, &peer, &workspace);
    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("mcp_requests".into(), n(&mcp_requests.to_string())),
        ("model_requests".into(), n(&model_requests.to_string())),
    ]);
    twin::report_value("mcp_continuation", &body, true);
    println!("passed {mcp_count} mcp {mcp_requests} model {model_requests}", mcp_count = passed.len());
    controller.finish();
}

fn run(controller: &mut Controller, peer: &Peer, workspace: &str) -> (Vec<String>, usize, usize) {
    let mut passed = Vec::new();
    let png = png_bytes();
    let encoded = twin::b64_encode(&png);
    let text = text_block();
    let image = image_value(&encoded, "image/png");
    let mixed = mixed_result(&encoded);
    peer.set_result(&mixed);

    let d = followup(controller, peer, workspace, &encoded, false, &mut passed);
    let original = active(&d).get("id").unwrap_or_else(|| panic!("another conversation does not expose the first result")).str_().to_string();
    let switched = event(controller, Some(state(&d)), "new", obj(vec![]), workspace);
    let other = event(controller, Some(state(&switched)), "chat_actions", obj(vec![]), workspace);
    pass(
        &mut passed,
        "another conversation does not expose the first result",
        !controls(&other).iter().any(|action| action == "mcp_image_review"),
    );
    let moved = Path::new(workspace).join("moved");
    std::fs::create_dir(&moved).unwrap_or_else(|error| panic!("could not create {}: {error}", moved.display()));
    let moved_text = moved.to_str().unwrap_or_else(|| panic!("moved path is not UTF-8")).to_string();
    let foreign = event(controller, Some(state(&d)), "chat_actions", obj(vec![]), &moved_text);
    pass(
        &mut passed,
        "another workspace cannot expose the result",
        !controls(&foreign).iter().any(|action| action == "mcp_image_review"),
    );
    let opened = event(controller, Some(state(&switched)), &format!("history:{original}"), obj(vec![]), workspace);
    let reviewed = review(controller, &opened, workspace);
    let raw = event(controller, Some(state(&reviewed)), "mcp_image_raw", obj(vec![]), workspace);
    let raw_text = field_at(&raw, &["ui", "modal", "fields"]).arr().first().and_then(|item| item.get("value")).map(Value::str_).unwrap_or_else(|| panic!("local details preserve the exact typed and structured result"));
    pass(
        &mut passed,
        "local details preserve the exact typed and structured result",
        Value::parse(raw_text) == mixed,
    );
    let back = event(controller, Some(state(&raw)), "mcp_image_back", obj(vec![]), workspace);
    let offered: Vec<String> = controls(&back).into_iter().filter(|action| action.starts_with("mcp_image_save:")).collect();
    pass(
        &mut passed,
        "only a supported image offers an explicit save",
        offered.len() == 1 && offered[0].starts_with("mcp_image_save:1:"),
    );

    let child = followup(controller, peer, workspace, &encoded, true, &mut passed);
    let child_review = review(controller, &child, workspace);
    pass(
        &mut passed,
        "child result is visible in the originating conversation",
        controls(&child_review).iter().any(|action| action.starts_with("mcp_image_save:1:")),
    );

    let save = event(controller, Some(state(&back)), &offered[0], obj(vec![]), workspace);
    let record = field(state(&save), "service_artifact");
    let saved = std::fs::read(Path::new(workspace).join(field(record, "path").str_())).unwrap_or_else(|error| panic!("explicit save uses exact image and existing conversation artifact: {error}"));
    let last = active(&save).get("service_artifacts").and_then(as_arr).and_then(|items| items.last());
    pass(
        &mut passed,
        "explicit save uses exact image and existing conversation artifact",
        saved == png && field(record, "sha256").str_() == sha256_hex(&png) && last == Some(record),
    );
    let preview_event = event(controller, Some(state(&save)), "media_preview", obj(vec![]), workspace);
    let preview = field(&preview_event, "effect");
    let verified = controller.op(&with_op(preview, "media_verify"));
    pass(
        &mut passed,
        "native preview verifier reads the same saved bytes",
        field(&verified, "path").str_() == field(preview, "path").str_(),
    );

    peer.set_result(&obj(vec![
        ("content".into(), Value::Arr(vec![text.clone(), image.clone()])),
        ("isError".into(), Value::Bool(true)),
    ]));
    let started = start(controller, peer, workspace, false, &mut passed);
    let d = exchange(controller, peer, &started, workspace, None);
    let error_wire = effect_text(&d, "wire", "tool-error text continues without image base64");
    pass(
        &mut passed,
        "tool-error text continues without image base64",
        error_wire.contains("isError") && !error_wire.contains(&encoded),
    );
    let answered = exchange(controller, peer, &d, workspace, Some("The tool reported an error."));
    let error = review(controller, &answered, workspace);
    pass(
        &mut passed,
        "tool errors allow raw inspection without an image save",
        !controls(&error).iter().any(|action| action.starts_with("mcp_image_save:")),
    );

    peer.set_result(&obj(vec![("content".into(), Value::Arr(vec![text.clone()]))]));
    let started = start(controller, peer, workspace, false, &mut passed);
    let d = exchange(controller, peer, &started, workspace, None);
    let plain_wire = effect_text(&d, "wire", "plain MCP output keeps its original text and creates no image review");
    let answered = exchange(controller, peer, &d, workspace, Some("Text received."));
    let plain_actions = event(controller, Some(state(&answered)), "chat_actions", obj(vec![]), workspace);
    pass(
        &mut passed,
        "plain MCP output keeps its original text and creates no image review",
        plain_wire.contains("Actual image peer observation") && !controls(&plain_actions).iter().any(|action| action == "mcp_image_review"),
    );

    let large_png = large_png(&png);
    let large_encoded = twin::b64_encode(&large_png);
    let large_image = image_value(&large_encoded, "image/png");
    peer.set_result(&obj(vec![("content".into(), Value::Arr(vec![text.clone(), large_image.clone()]))]));
    let pending = start(controller, peer, workspace, false, &mut passed);
    let callback = socket_result(peer, &pending, None);
    let mut limited_state = state(&pending).clone();
    put(&mut limited_state, "capacity_fixture", s(&"x".repeat(6 * 1024 * 1024 - 200_000)));
    let limited = event(controller, Some(&limited_state), "network_result", callback, workspace);
    let limited_wire = effect_text(&limited, "wire", "local retention capacity cannot block the text continuation");
    pass(
        &mut passed,
        "local retention capacity cannot block the text continuation",
        effect_text(&limited, "purpose", "local retention capacity cannot block the text continuation") == "model"
            && limited_wire.contains("Actual image peer observation")
            && !limited_wire.contains(&large_encoded)
            && absent(active(&limited), "mcp_image_evidence")
            && notice_has(&limited, "could not be retained"),
    );

    peer.set_result(&mixed);
    let pending = start(controller, peer, workspace, false, &mut passed);
    let callback = socket_result(peer, &pending, None);
    let mut local_state = state(&pending).clone();
    put(&mut local_state, "capability_mode", s("local"));
    let local = event(controller, Some(&local_state), "network_result", callback, workspace);
    let local_review = review(controller, &local, workspace);
    pass(
        &mut passed,
        "paused model leaves the already received image available locally",
        effect_is_null(&local) && notice_error(&local) && notice_has(&local, "Remote model requests are paused") && controls(&local_review).iter().any(|action| action.starts_with("mcp_image_save:1:")),
    );
    let listed = event(controller, Some(state(&local)), "mcp_client_list", obj(vec![]), workspace);
    let listed = exchange(controller, peer, &listed, workspace, None);
    let listed = exchange(controller, peer, &listed, workspace, None);
    let discovered = exchange(controller, peer, &listed, workspace, None);
    let names = state(&discovered).get("mcp_client_catalog").and_then(as_arr).map(|items| items.iter().map(|entry| field(entry, "name").str_().to_string()).collect::<Vec<_>>()).unwrap_or_default();
    pass(
        &mut passed,
        "a stopped model continuation cannot gate ordinary MCP discovery",
        text_is(state(&discovered), "dialog", "mcp_tools") && names == ["image"],
    );

    let pending = start(controller, peer, workspace, false, &mut passed);
    let late = socket_result(peer, &pending, None);
    let wrong_root = event(controller, Some(state(&pending)), "network_result", late.clone(), &moved_text);
    pass(
        &mut passed,
        "in-flight MCP result is refused in a different workspace",
        effect_is_null(&wrong_root) && absent(active(&wrong_root), "mcp_image_evidence"),
    );
    let stopped = event(controller, Some(state(&pending)), "cancel", obj(vec![]), workspace);
    let cancelled = event(controller, Some(state(&stopped)), "network_result", late, workspace);
    let cancelled_actions = event(controller, Some(state(&cancelled)), "chat_actions", obj(vec![]), workspace);
    pass(
        &mut passed,
        "cancelled callbacks cannot continue or install image evidence",
        effect_is_null(&cancelled) && !controls(&cancelled_actions).iter().any(|action| action == "mcp_image_review"),
    );

    let mcp = peer.mcp_log();
    let models = peer.model_log();
    let mut expected = Vec::new();
    for _ in 0..6 {
        expected.extend(["initialize", "notifications/initialized", "tools/call"]);
    }
    expected.extend(["initialize", "notifications/initialized", "tools/list", "initialize", "notifications/initialized", "tools/call"]);
    pass(
        &mut passed,
        "exact calls occurred through the loopback MCP and both model routes",
        mcp == expected && models.iter().any(|route| route == "/child"),
    );
    (passed, mcp.len(), models.len())
}

fn followup(controller: &mut Controller, peer: &Peer, workspace: &str, encoded: &str, child: bool, passed: &mut Vec<String>) -> Value {
    let pending = start(controller, peer, workspace, child, passed);
    let callback = socket_result(peer, &pending, None);
    let d = event(controller, Some(state(&pending)), "network_result", callback.clone(), workspace);
    let wire = effect_text(&d, "wire", "typed image base64 never enters model continuation").to_string();
    pass(passed, "typed image base64 never enters model continuation", !wire.contains(encoded) && wire.contains("dataOmitted"));
    let repeated = event(controller, Some(state(&d)), "network_result", callback, workspace);
    let before = active(&d).get("mcp_image_evidence").cloned();
    let after = active(&repeated).get("mcp_image_evidence").cloned();
    pass(
        passed,
        "duplicate result does not install a new continuation or result",
        effect_is_null(&repeated) && before.is_some() && before == after,
    );
    pass(
        passed,
        "text and structured evidence continue on the owning model route",
        wire.contains("Actual image peer observation") && wire.contains("frame") && is_num(state(&d), "request_index", if child { "1" } else { "0" }),
    );
    pass(passed, "callback offers review without creating a file", service_files_empty(workspace));
    let d = exchange(controller, peer, &d, workspace, Some("Image result received as evidence."));
    if child {
        let parent_wire = effect_text(&d, "wire", "child continuation returns to the original parent");
        pass(
            passed,
            "child continuation returns to the original parent",
            is_num(state(&d), "request_index", "0") && !parent_wire.contains(encoded),
        );
        return exchange(controller, peer, &d, workspace, Some("Parent received the child finding."));
    }
    d
}

fn start(controller: &mut Controller, peer: &Peer, workspace: &str, child: bool, passed: &mut Vec<String>) -> Value {
    let base = peer.base();
    let st = configured(controller, workspace, &base);
    let mut d = event(
        controller,
        Some(&st),
        "send",
        obj(vec![("draft".into(), s("Request the reviewed MCP image."))]),
        workspace,
    );
    if child {
        d = proposal(controller, peer, &d, workspace, "delegate", &obj(vec![
            ("agent".into(), n("1")),
            ("task".into(), s("Retrieve the reviewed image result.")),
        ]));
        d = event(controller, Some(state(&d)), "approve", obj(vec![]), workspace);
    }
    d = proposal(controller, peer, &d, workspace, "mcp", &obj(vec![
        ("name".into(), s("image")),
        ("arguments".into(), obj(vec![])),
    ]));
    pass(passed, "model proposal waits for approval", effect_is_null(&d));
    d = event(controller, Some(state(&d)), "approve", obj(vec![]), workspace);
    let d = exchange(controller, peer, &d, workspace, None);
    exchange(controller, peer, &d, workspace, None)
}

fn configured(controller: &mut Controller, workspace: &str, base: &str) -> Value {
    let d = event(controller, None, "resume", obj(vec![]), workspace);
    let d = event(controller, Some(state(&d)), "mcp_client_settings", obj(vec![]), workspace);
    let d = event(
        controller,
        Some(state(&d)),
        "mcp_client_save",
        obj(vec![
            ("endpoint".into(), s(&format!("{base}/mcp"))),
            ("key".into(), s("image-test")),
            ("access".into(), Value::Bool(true)),
        ]),
        workspace,
    );
    let mut st = state(&d).clone();
    put(&mut st, "profiles", Value::Arr(vec![profile("primary", base), profile("child", base)]));
    put(
        &mut st,
        "agents",
        Value::Arr(vec![
            obj(vec![
                ("name".into(), s("Parent")),
                ("model".into(), s("0")),
                ("tools".into(), s("mcp,delegate")),
                ("subagents".into(), Value::Arr(vec![n("1")])),
            ]),
            obj(vec![
                ("name".into(), s("Child")),
                ("model".into(), s("1")),
                ("tools".into(), s("mcp")),
            ]),
        ]),
    );
    st
}

fn profile(name: &str, base: &str) -> Value {
    obj(vec![
        ("name".into(), s(name)),
        ("model".into(), s(name)),
        ("endpoint".into(), s(&format!("{base}/{name}"))),
        ("plain".into(), Value::Bool(true)),
        ("protocol".into(), s("chat")),
        ("stream".into(), Value::Bool(false)),
        ("tool_format".into(), s("json")),
    ])
}

fn proposal(controller: &mut Controller, peer: &Peer, d: &Value, workspace: &str, tool: &str, args: &Value) -> Value {
    let response = obj(vec![("tool".into(), s(tool)), ("args".into(), args.clone())]).to_string();
    let d = exchange(controller, peer, d, workspace, Some(&response));
    let pending = state(&d).get("pending").and_then(|item| item.get("tool")).map(Value::str_);
    if pending != Some(tool) || !effect_is_null(&d) {
        panic!("proposal {tool} was not held for approval; notice={} pending={}", notice(&d), pending.unwrap_or(""));
    }
    d
}

fn exchange(controller: &mut Controller, peer: &Peer, d: &Value, workspace: &str, response: Option<&str>) -> Value {
    let inputs = socket_result(peer, d, response);
    event(controller, Some(state(d)), "network_result", inputs, workspace)
}

fn socket_result(peer: &Peer, d: &Value, response: Option<&str>) -> Value {
    if let Some(response) = response {
        peer.push_reply(response);
    }
    let effect = field(d, "effect");
    if effect.get("kind").map(Value::str_) != Some("network") {
        panic!("expected network effect; notice={}", notice(d));
    }
    let host = field(effect, "host").str_();
    let port = match field(effect, "port") {
        Value::Num(token) => token.parse::<u16>().unwrap_or_else(|error| panic!("port {token}: {error}")),
        other => panic!("port {}", other.to_string()),
    };
    let mut stream = TcpStream::connect((host, port)).unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap_or_else(|error| panic!("timeout: {error}"));
    stream.set_write_timeout(Some(Duration::from_secs(60))).unwrap_or_else(|error| panic!("timeout: {error}"));
    stream.write_all(field(effect, "wire").str_().as_bytes()).unwrap_or_else(|error| panic!("write wire: {error}"));
    stream.flush().unwrap_or_else(|error| panic!("flush wire: {error}"));
    let _ = stream.shutdown(Shutdown::Write);
    let mut raw = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => raw.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("read response: {error}"),
        }
    }
    if let Some(error) = peer.take_error() {
        panic!("peer failed: {error}");
    }
    if raw.is_empty() {
        panic!("empty HTTP response for {}", field(effect, "purpose").str_());
    }
    let mut inputs = obj(vec![
        ("purpose".into(), s(field(effect, "purpose").str_())),
        ("data".into(), s(&twin::b64_encode(&raw))),
    ]);
    if field(effect, "purpose").str_() == "model" {
        put(&mut inputs, "model_ticket", s(state(d).get("model_expected").unwrap_or_else(|| panic!("missing model_expected")).str_()));
    }
    inputs
}

fn review(controller: &mut Controller, d: &Value, workspace: &str) -> Value {
    let d = event(controller, Some(state(d)), "chat_actions", obj(vec![]), workspace);
    if !controls(&d).iter().any(|action| action == "mcp_image_review") {
        panic!("mcp_image_review not offered: {}", controls(&d).join(","));
    }
    event(controller, Some(state(&d)), "mcp_image_review", obj(vec![]), workspace)
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, workspace: &str) -> Value {
    controller.event(state, action, inputs, workspace)
}

fn controls(d: &Value) -> Vec<String> {
    let buttons = d.get("ui").and_then(|ui| ui.get("modal")).and_then(|modal| modal.get("buttons")).and_then(as_arr).unwrap_or_else(|| panic!("actions have no buttons"));
    buttons.iter().map(|button| field(button, "event").str_().to_string()).collect()
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn as_arr(value: &Value) -> Option<&Vec<Value>> {
    match value {
        Value::Arr(items) => Some(items),
        _ => None,
    }
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn field_at<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    let mut current = value;
    for key in path {
        current = field(current, key);
    }
    current
}

fn effect_text<'a>(d: &'a Value, key: &str, name: &str) -> &'a str {
    let effect = d.get("effect").unwrap_or_else(|| panic!("{name}"));
    effect.get(key).unwrap_or_else(|| panic!("{name}")).str_()
}

fn effect_is_null(d: &Value) -> bool {
    matches!(d.get("effect"), Some(Value::Null))
}

fn is_num(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Num(token)) if token == expect)
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn absent(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn notice(d: &Value) -> String {
    state(d).get("notice").map(Value::to_string).unwrap_or_else(|| "none".into())
}

fn notice_has(d: &Value, text: &str) -> bool {
    matches!(state(d).get("notice"), Some(Value::Str(value)) if value.contains(text))
}

fn notice_error(d: &Value) -> bool {
    matches!(state(d).get("notice_error"), Some(Value::Bool(true)))
}

fn pass(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
    println!("{name}");
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn with_op(effect: &Value, op_name: &str) -> Value {
    let Value::Obj(fields) = effect else { panic!("media preview effect is not an object") };
    let mut fields = fields.clone();
    if let Some(index) = fields.iter().position(|(name, _)| name == "op") {
        fields[index].1 = s(op_name);
    } else {
        fields.push(("op".into(), s(op_name)));
    }
    Value::Obj(fields)
}

fn service_files_empty(workspace: &str) -> bool {
    match std::fs::read_dir(Path::new(workspace).join("service-files")) {
        Ok(entries) => entries.flatten().all(|entry| entry.file_name().to_string_lossy().starts_with('.')),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => panic!("callback offers review without creating a file: {error}"),
    }
}

fn active(d: &Value) -> &Value {
    let st = state(d);
    let id = st.get("active").unwrap_or_else(|| panic!("missing active"));
    st.get("chats").and_then(as_arr).and_then(|chats| chats.iter().find(|chat| chat.get("id") == Some(id))).unwrap_or_else(|| panic!("missing active chat"))
}

fn text_block() -> Value {
    obj(vec![("type".into(), s("text")), ("text".into(), s("Actual image peer observation"))])
}

fn image_value(encoded: &str, mime: &str) -> Value {
    obj(vec![("type".into(), s("image")), ("data".into(), s(encoded)), ("mimeType".into(), s(mime))])
}

fn mixed_result(encoded: &str) -> Value {
    obj(vec![
        ("content".into(), Value::Arr(vec![text_block(), image_value(encoded, "image/png"), image_value(encoded, "image/gif")])),
        ("structuredContent".into(), obj(vec![("frame".into(), n("1"))])),
    ])
}

fn png_bytes() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    bytes.extend(png_chunk(b"IDAT", &zlib_store(&[0x00, 0x11, 0x22, 0x33])));
    bytes.extend(png_chunk(b"IEND", &[]));
    bytes
}

fn large_png(png: &[u8]) -> Vec<u8> {
    let mut note = b"Note\0".to_vec();
    note.extend(std::iter::repeat(b'x').take(240_000));
    let mut bytes = png[..33].to_vec();
    bytes.extend(png_chunk(b"tEXt", &note));
    bytes.extend_from_slice(&png[33..]);
    bytes
}

fn png_chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01, 0x01];
    let len = u16::try_from(data.len()).unwrap_or_else(|_| panic!("stored deflate block is too large"));
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut hash = [0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks(64) {
        let mut words = [0u32; 64];
        for index in 0..16 {
            words[index] = u32::from_be_bytes(block[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut v = hash;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[index]).wrapping_add(words[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for index in 0..8 {
            hash[index] = hash[index].wrapping_add(v[index]);
        }
    }
    hash.iter().map(|word| format!("{word:08x}")).collect()
}

struct Shared {
    mcp: Mutex<Vec<String>>,
    models: Mutex<Vec<String>>,
    replies: Mutex<VecDeque<String>>,
    result: Mutex<Value>,
    error: Mutex<Option<String>>,
    shutdown: AtomicBool,
}

struct Peer {
    port: u16,
    shared: Arc<Shared>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("port: {error}")).port();
        let shared = Arc::new(Shared {
            mcp: Mutex::new(Vec::new()),
            models: Mutex::new(Vec::new()),
            replies: Mutex::new(VecDeque::new()),
            result: Mutex::new(Value::Null),
            error: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });
        let thread_shared = Arc::clone(&shared);
        let handle = thread::spawn(move || serve(listener, &thread_shared));
        Self { port, shared, handle: Some(handle) }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn set_result(&self, value: &Value) {
        *self.shared.result.lock().unwrap_or_else(|error| panic!("result lock: {error}")) = value.clone();
    }

    fn push_reply(&self, response: &str) {
        self.shared.replies.lock().unwrap_or_else(|error| panic!("reply lock: {error}")).push_back(response.to_string());
    }

    fn mcp_log(&self) -> Vec<String> {
        self.shared.mcp.lock().unwrap_or_else(|error| panic!("mcp lock: {error}")).clone()
    }

    fn model_log(&self) -> Vec<String> {
        self.shared.models.lock().unwrap_or_else(|error| panic!("model lock: {error}")).clone()
    }

    fn take_error(&self) -> Option<String> {
        self.shared.error.lock().unwrap_or_else(|error| panic!("error lock: {error}")).clone()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve(listener: TcpListener, shared: &Shared) {
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if shared.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(error) = handle(&mut stream, shared) {
                    *shared.error.lock().unwrap_or_else(|poison| panic!("error lock: {poison}")) = Some(error);
                    break;
                }
            }
            Err(error) => {
                if shared.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                *shared.error.lock().unwrap_or_else(|poison| panic!("error lock: {poison}")) = Some(format!("accept: {error}"));
                break;
            }
        }
    }
}

fn handle(stream: &mut TcpStream, shared: &Shared) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let request = read_http(stream)?;
    if request.path == "/mcp" {
        if request.authorization.as_deref() != Some("Bearer image-test") {
            return Err(format!("authorization {:?}", request.authorization));
        }
        let body = Value::parse(&request.body);
        let method = body.get("method").map(Value::str_).ok_or("missing MCP method")?.to_string();
        shared.mcp.lock().map_err(|_| "mcp lock".to_string())?.push(method.clone());
        match method.as_str() {
            "initialize" => {
                let result = obj(vec![
                    ("protocolVersion".into(), s("2025-11-25")),
                    ("capabilities".into(), obj(vec![("tools".into(), obj(vec![]))])),
                    ("serverInfo".into(), obj(vec![("name".into(), s("Image peer")), ("version".into(), s("1"))])),
                ]);
                write_http(stream, 200, "OK", &rpc(body.get("id").ok_or("missing id")?, &result))
            }
            "notifications/initialized" => write_http(stream, 202, "Accepted", ""),
            "tools/list" => {
                let result = obj(vec![("tools".into(), Value::Arr(vec![obj(vec![
                    ("name".into(), s("image")),
                    ("inputSchema".into(), obj(vec![("type".into(), s("object"))])),
                ])]))]);
                write_http(stream, 200, "OK", &rpc(body.get("id").ok_or("missing id")?, &result))
            }
            "tools/call" => {
                let expected = obj(vec![("name".into(), s("image")), ("arguments".into(), obj(vec![]))]);
                if body.get("params") != Some(&expected) {
                    return Err(format!("unexpected tools/call params {}", body.get("params").map(Value::to_string).unwrap_or_default()));
                }
                let result = shared.result.lock().map_err(|_| "result lock".to_string())?.clone();
                write_http(stream, 200, "OK", &rpc(body.get("id").ok_or("missing id")?, &result))
            }
            other => Err(format!("unexpected MCP method {other}")),
        }
    } else if request.path == "/primary" || request.path == "/child" {
        shared.models.lock().map_err(|_| "model lock".to_string())?.push(request.path);
        let reply = shared.replies.lock().map_err(|_| "reply lock".to_string())?.pop_front().ok_or("missing model reply")?;
        let body = obj(vec![("choices".into(), Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(&reply))])),
            ("finish_reason".into(), s("stop")),
        ])]))]);
        write_http(stream, 200, "OK", &body.to_string())
    } else {
        Err(format!("unexpected path {}", request.path))
    }
}

fn rpc(id: &Value, result: &Value) -> String {
    obj(vec![("jsonrpc".into(), s("2.0")), ("id".into(), id.clone()), ("result".into(), result.clone())]).to_string()
}

struct HttpRequest {
    path: String,
    body: String,
    authorization: Option<String>,
}

fn read_http(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("HTTP request exceeds 8 MiB".into());
        }
        let count = stream.read(&mut buffer).map_err(|error| format!("read request: {error}"))?;
        if count == 0 {
            return Err("incomplete HTTP request".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(header_end) = find_subslice(&bytes, b"\r\n\r\n") else { continue };
        let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "request headers are not UTF-8".to_string())?;
        let mut lines = header.split("\r\n");
        let request_line = lines.next().ok_or("missing request line")?;
        let path = request_line.split_whitespace().nth(1).ok_or("missing request path")?.to_string();
        let mut length = None;
        let mut authorization = None;
        for line in lines {
            let (name, value) = line.split_once(':').ok_or_else(|| format!("bad header {line}"))?;
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().map_err(|_| "bad content-length".to_string())?);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_string());
            }
        }
        let length = length.ok_or("missing content-length")?;
        let start = header_end + 4;
        if bytes.len() < start + length {
            continue;
        }
        if bytes.len() != start + length {
            return Err("trailing bytes after HTTP request".into());
        }
        let body = std::str::from_utf8(&bytes[start..]).map_err(|_| "request body is not UTF-8".to_string())?.to_string();
        return Ok(HttpRequest { path, body, authorization });
    }
}

fn write_http(stream: &mut TcpStream, status: u16, reason: &str, body: &str) -> Result<(), String> {
    let header = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(header.as_bytes()).and_then(|_| stream.write_all(body.as_bytes())).and_then(|_| stream.flush()).map_err(|error| format!("write response: {error}"))?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("mcp-continuation-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&path).unwrap_or_else(|error| panic!("could not create {}: {error}", path.display()));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

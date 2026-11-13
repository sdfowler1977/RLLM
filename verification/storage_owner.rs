mod twin;

use twin::{b, b64_decode, b64_encode, n, obj, s, sha256_digest, truthy, Controller, Value};

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCOPE: &str = "Actual shared-controller events and local HTTP peers; no installed-device or independent acceptance.";
const LISTING: &[u8] = br#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/share/alpha.txt</d:href><d:propstat><d:prop><d:displayname>alpha.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>"#;

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let binary = std::env::var("RLLM_CONTROLLER")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("output/headless/RLLM"));
    let peer = Peer::start();
    let scratch = Scratch::create(&root);
    let mut controller = Controller::spawn_at(&binary, &root_text);
    let passed = run(&mut controller, &peer, scratch.path());
    let exchanges = peer.call_count();
    let call_log = peer.call_log();
    controller.finish();
    drop(peer);

    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("http_exchanges".into(), n(&exchanges.to_string())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("storage_owner", &body, true);
    if passed.len() != 43 || exchanges != 21 {
        panic!("storage-owner report is {} checks and {exchanges} exchanges: {call_log}", passed.len());
    }
}

fn run(controller: &mut Controller, peer: &Peer, scratch: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let origin = peer.origin();

    let configured = setup(controller, &origin, scratch);
    let first = authorize(controller, &configured, scratch);
    let first_access = text(tokens(state(&first)), "access_token").to_string();
    let configured = setup(controller, &origin, scratch);
    let second = authorize(controller, &configured, scratch);
    let newer = tokens(state(&second)).clone();

    let settings = event(controller, Some(state(&first)), "storage_oauth_settings", obj(vec![]), scratch);
    let pending = event(controller, Some(state(&settings)), "oauth_refresh", obj(vec![]), scratch);
    let effect = network_effect(&pending);
    let response = exchange(&effect);
    let callback = callback_input(text(&effect, "purpose"), &response);
    let failure = failure_input(text(&effect, "purpose"));
    let expires_at = now_secs() + 8000;
    for (key, value) in [
        ("identity", s("replacement-owner")),
        ("access_token", s(text(&newer, "access_token"))),
        ("refresh_token", s(text(&newer, "refresh_token"))),
        ("expires_at", n(&expires_at.to_string())),
        ("binding", s("other-binding")),
        ("storage_connection", obj(vec![])),
    ] {
        let changed = token_field(state(&pending), key, value);
        inert(
            controller,
            &mut passed,
            peer,
            &format!("old refresh cannot adopt after changed {key}"),
            &changed,
            "network_result",
            callback.clone(),
            scratch,
        );
        inert(
            controller,
            &mut passed,
            peer,
            &format!("old pre-send failure cannot clear changed {key}"),
            &changed,
            "failure",
            failure.clone(),
            scratch,
        );
    }

    let replaced = changed_credentials(state(&pending), &newer);
    inert(
        controller,
        &mut passed,
        peer,
        "old reply cannot replace complete newer grant",
        &replaced,
        "network_result",
        callback.clone(),
        scratch,
    );
    let current = event(controller, Some(state(&pending)), "network_result", callback.clone(), scratch);
    keep(
        &mut passed,
        "unchanged current refresh still adopts exact new generation",
        text(tokens(state(&current)), "access_token") != first_access && absent_or_false(tokens(state(&current)), "refresh_uncertain"),
    );
    inert(
        controller,
        &mut passed,
        peer,
        "consumed current reply cannot revive prior credential generation",
        state(&pending),
        "network_result",
        callback,
        scratch,
    );

    let settings = event(controller, Some(state(&current)), "storage_oauth_settings", obj(vec![]), scratch);
    let (started, uri) = browser(controller, Some(state(&settings)), scratch);
    inert(
        controller,
        &mut passed,
        peer,
        "old browser return cannot exchange code after credentials change",
        &changed_credentials(state(&started), &newer),
        "oauth_return",
        obj(vec![("uri".into(), s(&uri))]),
        scratch,
    );
    let pending = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), scratch);
    let effect = network_effect(&pending);
    let response = exchange(&effect);
    let callback = callback_input(text(&effect, "purpose"), &response);
    inert(
        controller,
        &mut passed,
        peer,
        "old browser token reply cannot replace newer grant",
        &changed_credentials(state(&pending), &newer),
        "network_result",
        callback.clone(),
        scratch,
    );
    let completed = event(controller, Some(state(&pending)), "network_result", callback, scratch);
    keep(
        &mut passed,
        "current browser grant still completes",
        drive_enabled(state(&completed)) && not_busy(state(&completed)),
    );

    let fresh = setup(controller, &origin, scratch);
    let pending = event(controller, Some(state(&fresh)), "oauth_device_start", obj(vec![]), scratch);
    let effect = network_effect(&pending);
    let response = exchange(&effect);
    let callback = callback_input(text(&effect, "purpose"), &response);
    inert(
        controller,
        &mut passed,
        peer,
        "old device initiation cannot adopt after credentials change",
        &changed_credentials(state(&pending), &newer),
        "network_result",
        callback.clone(),
        scratch,
    );
    let wait = event(controller, Some(state(&pending)), "network_result", callback, scratch);
    keep(
        &mut passed,
        "current device initiation retains its code review",
        matches!(state(&wait).get("dialog"), Some(Value::Str(text)) if text == "oauth_device"),
    );
    thread::sleep(Duration::from_millis(1100));

    let pending = event(controller, Some(state(&wait)), "oauth_device_poll", obj(vec![]), scratch);
    let effect = network_effect(&pending);
    let response = exchange(&effect);
    let callback = callback_input(text(&effect, "purpose"), &response);
    inert(
        controller,
        &mut passed,
        peer,
        "old device token reply cannot replace newer grant",
        &changed_credentials(state(&pending), &newer),
        "network_result",
        callback.clone(),
        scratch,
    );
    let completed = event(controller, Some(state(&pending)), "network_result", callback, scratch);
    keep(
        &mut passed,
        "current device poll still adopts tokens",
        drive_enabled(state(&completed)) && not_busy(state(&completed)),
    );

    let settings = event(controller, Some(state(&completed)), "storage_oauth_settings", obj(vec![]), scratch);
    let pending = event(controller, Some(state(&settings)), "oauth_refresh", obj(vec![]), scratch);
    let effect = network_effect(&pending);
    let failed = event(controller, Some(state(&pending)), "failure", failure_input(text(&effect, "purpose")), scratch);
    keep(
        &mut passed,
        "current pre-send failure clears only its own uncertainty",
        tokens(state(&failed)).get("refresh_uncertain").is_some_and(|value| !truthy(value)),
    );
    let retry = event(controller, Some(state(&failed)), "oauth_refresh", obj(vec![]), scratch);
    let completed = complete(controller, &retry, scratch);
    keep(
        &mut passed,
        "explicit current retry still completes",
        drive_enabled(state(&completed)) && not_busy(state(&completed)),
    );

    let changed = changed_credentials(state(&completed), &newer);
    let settings = event(controller, Some(&changed), "storage_oauth_settings", obj(vec![]), scratch);
    let manual = event(controller, Some(state(&settings)), "oauth_key_mode", obj(vec![]), scratch);
    let listed = event(controller, Some(state(&manual)), "drive_list", obj(vec![]), scratch);
    let listed = complete(controller, &listed, scratch);
    let remote_ok = matches!(state(&listed).get("remote"), Some(Value::Arr(items)) if items.len() == 1);
    let profiles_ok = state(&listed).get("profiles").is_some_and(|value| !truthy(value));
    let header = peer.last_header("x-storage-key");
    if !(remote_ok && profiles_ok && header.as_deref() == Some("Manual retained-key")) {
        panic!(
            "explicit manual storage remains usable without models or reports: remote={remote_ok} profiles={profiles_ok} header={header:?} calls={}",
            peer.call_log()
        );
    }
    passed.push("explicit manual storage remains usable without models or reports".to_string());

    let settings = event(controller, Some(state(&listed)), "storage_oauth_settings", obj(vec![]), scratch);
    let (started, uri) = browser(controller, Some(state(&settings)), scratch);
    let resumed = event(controller, Some(state(&started)), "resume", obj(vec![]), scratch);
    let pending_owner = matches!(state(&resumed).get("dialog"), Some(Value::Str(text)) if text == "oauth_wait")
        && matches!(state(&resumed).get("oauth_pending").and_then(|item| item.get("target")), Some(Value::Str(text)) if text == "storage");
    keep(&mut passed, "unchanged pending browser recovery retains typed storage owner", pending_owner);
    let returned = event(controller, Some(state(&resumed)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), scratch);
    let completed = complete(controller, &returned, scratch);
    keep(&mut passed, "recovered current browser return completes", drive_enabled(state(&completed)));

    for protocol in ["chat", "local", "messages", "content"] {
        let mut model = event(controller, None, "resume", obj(vec![]), scratch);
        model = event(controller, Some(state(&model)), "profile_new", obj(vec![]), scratch);
        model = event(
            controller,
            Some(state(&model)),
            "profile_save",
            obj(vec![
                ("name".into(), s("Ownership peer")),
                ("protocol".into(), s(protocol)),
                ("endpoint".into(), s(&format!("{origin}/model/{protocol}"))),
                ("model".into(), s("controlled")),
                ("key".into(), s("primary-key")),
                ("plain".into(), b(true)),
                ("stream".into(), b(false)),
            ]),
            scratch,
        );
        model = event(controller, Some(state(&model)), "oauth_settings", obj(vec![]), scratch);
        model = event(
            controller,
            Some(state(&model)),
            "oauth_save",
            obj(vec![
                ("authorization_endpoint".into(), s(&format!("{origin}/authorize"))),
                ("token_endpoint".into(), s(&format!("{origin}/token"))),
                ("client_id".into(), s("public-client")),
                ("scope".into(), s("conversation")),
                ("issuer".into(), s(&origin)),
                ("plain".into(), b(true)),
            ]),
            scratch,
        );
        model = authorize(controller, &model, scratch);
        let mut model_state = state(&model).clone();
        set_profile_token(&mut model_state, "expires_at", n(&(now_secs() + 20).to_string()));
        let pending = event(
            controller,
            Some(&model_state),
            "send",
            obj(vec![("draft".into(), s("Resume only this authorized primary request."))]),
            scratch,
        );
        let effect = pending.get("effect").cloned().unwrap_or(Value::Null);
        let purpose = effect.get("purpose").map(Value::str_).unwrap_or("").to_string();
        if !truthy(&effect) || !purpose.starts_with("oauth_token:") {
            let notice = state(&pending).get("notice").map(Value::to_string).unwrap_or_default();
            panic!("{protocol} send did not start token refresh: {} notice={notice}", effect.to_string());
        }
        let response = exchange(&effect);
        let callback = callback_input(&purpose, &response);
        let mut changed = state(&pending).clone();
        set_profile_token(&mut changed, "identity", s("new-model-generation"));
        inert(
            controller,
            &mut passed,
            peer,
            &format!("{protocol} old automatic refresh cannot adopt a newer model generation"),
            &changed,
            "network_result",
            callback.clone(),
            scratch,
        );
        inert(
            controller,
            &mut passed,
            peer,
            &format!("{protocol} old automatic pre-send failure cannot clear a newer model generation"),
            &changed,
            "failure",
            failure_input(&purpose),
            scratch,
        );
        let resumed = event(controller, Some(state(&pending)), "network_result", callback, scratch);
        let resumed_effect = resumed.get("effect").cloned().unwrap_or(Value::Null);
        let resumes = truthy(&resumed_effect)
            && resumed_effect.get("purpose").map(Value::str_) == Some("model")
            && state(&resumed).get("busy").is_some_and(truthy);
        if !resumes {
            panic!(
                "{protocol} current automatic refresh resumes actual primary operation: effect={} busy={}",
                resumed_effect.to_string(),
                state(&resumed).get("busy").map(Value::to_string).unwrap_or_else(|| "missing".to_string())
            );
        }
        passed.push(format!("{protocol} current automatic refresh resumes actual primary operation"));
        let done = complete(controller, &resumed, scratch);
        let chats = state(&done).get("chats").map(Value::to_string).unwrap_or_default();
        if !(chats.contains("Actual primary response.") && state(&done).get("busy").is_some_and(|item| !truthy(item))) {
            let notice = state(&done).get("notice").map(Value::to_string).unwrap_or_default();
            panic!(
                "{protocol} actual primary reply completes after current refresh: notice={notice} busy={}",
                state(&done).get("busy").map(Value::to_string).unwrap_or_else(|| "missing".to_string())
            );
        }
        passed.push(format!("{protocol} actual primary reply completes after current refresh"));
    }

    passed
}

fn setup(controller: &mut Controller, origin: &str, root: &str) -> Value {
    let resumed = event(controller, None, "resume", obj(vec![]), root);
    let edited = event(controller, Some(state(&resumed)), "drive_edit", obj(vec![]), root);
    let saved = event(
        controller,
        Some(state(&edited)),
        "drive_save",
        obj(vec![
            ("url".into(), s(&format!("{origin}/share/"))),
            ("plain".into(), b(true)),
            ("user".into(), s("retained-user")),
            ("password".into(), s("retained-password")),
            ("key".into(), s("retained-key")),
            ("auth_header".into(), s("X-Storage-Key")),
            ("auth_prefix".into(), s("Manual")),
        ]),
        root,
    );
    configure(controller, Some(state(&saved)), origin, root)
}

fn configure(controller: &mut Controller, current: Option<&Value>, origin: &str, root: &str) -> Value {
    let settings = event(controller, current, "storage_oauth_settings", obj(vec![]), root);
    event(
        controller,
        Some(state(&settings)),
        "oauth_save",
        obj(vec![
            ("authorization_endpoint".into(), s(&format!("{origin}/authorize"))),
            ("device_endpoint".into(), s(&format!("{origin}/device"))),
            ("token_endpoint".into(), s(&format!("{origin}/token"))),
            ("client_id".into(), s("public-client")),
            ("scope".into(), s("storage")),
            ("issuer".into(), s(origin)),
            ("plain".into(), b(true)),
        ]),
        root,
    )
}

fn authorize(controller: &mut Controller, configured: &Value, root: &str) -> Value {
    let (started, uri) = browser(controller, Some(state(configured)), root);
    let returned = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    complete(controller, &returned, root)
}

fn browser(controller: &mut Controller, current: Option<&Value>, root: &str) -> (Value, String) {
    let started = event(controller, current, "oauth_sign_in", obj(vec![]), root);
    let effect = require_kind(&started, "oauth_browser");
    let url = text(effect, "url").to_string();
    (started, fetch_redirect(&url))
}

fn complete(controller: &mut Controller, pending: &Value, root: &str) -> Value {
    let effect = network_effect(pending);
    let response = exchange(&effect);
    let mut inputs = obj(vec![]);
    for key in ["purpose", "drive_ticket", "model_ticket", "child_ticket", "child_owner"] {
        if let Some(value) = effect.get(key) {
            set_field(&mut inputs, key, value.clone());
        }
    }
    set_field(&mut inputs, "data", s(&b64_encode(&response)));
    event(controller, Some(state(pending)), "network_result", inputs, root)
}

fn inert(
    controller: &mut Controller,
    passed: &mut Vec<String>,
    peer: &Peer,
    name: &str,
    sent: &Value,
    action: &str,
    inputs: Value,
    root: &str,
) {
    let before = peer.call_count();
    let reply = event(controller, Some(sent), action, inputs, root);
    let same = state(&reply) == sent;
    let no_effect = matches!(reply.get("effect"), Some(Value::Null));
    let quiet = peer.call_count() == before;
    if !(same && no_effect && quiet) {
        let diff = if same {
            "state matched".to_string()
        } else {
            first_diff(state(&reply), sent, "state").unwrap_or_else(|| "state differed".to_string())
        };
        let effect = reply.get("effect").map(Value::to_string).unwrap_or_else(|| "missing".to_string());
        panic!("{name}: effect={effect} calls {before}->{} {diff}", peer.call_count());
    }
    passed.push(name.to_string());
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn event(controller: &mut Controller, current: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(current, action, inputs, root)
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn tokens(value: &Value) -> &Value {
    value
        .get("drive")
        .and_then(|drive| drive.get("oauth_tokens"))
        .unwrap_or_else(|| panic!("missing drive.oauth_tokens in {}", value.to_string()))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).unwrap_or_else(|| panic!("missing {key} in {}", value.to_string())).str_()
}

fn network_effect(value: &Value) -> Value {
    require_kind(value, "network").clone()
}

fn require_kind<'a>(value: &'a Value, kind: &str) -> &'a Value {
    let effect = value.get("effect").unwrap_or_else(|| panic!("missing effect for {kind}"));
    if !truthy(effect) || effect.get("kind").map(Value::str_) != Some(kind) {
        let notice = value.get("state").and_then(|item| item.get("notice")).map(Value::to_string).unwrap_or_default();
        panic!("expected {kind} effect, got {} notice={notice}", effect.to_string());
    }
    effect
}

fn absent_or_false(value: &Value, key: &str) -> bool {
    match value.get(key) {
        None | Some(Value::Null) => true,
        Some(item) => !truthy(item),
    }
}

fn set_field(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object when setting {key}, got {}", value.to_string());
    };
    if let Some((_, slot)) = fields.iter_mut().find(|(name, _)| name == key) {
        *slot = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn get_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object when reading {key}, got {}", value.to_string());
    };
    let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
    &mut fields[index].1
}

fn set_path(value: &mut Value, keys: &[&str], next: Value) {
    let mut cursor = value;
    let last = keys.len() - 1;
    for key in &keys[..last] {
        cursor = get_mut(cursor, key);
    }
    set_field(cursor, keys[last], next);
}

fn token_field(current: &Value, key: &str, next: Value) -> Value {
    let mut changed = current.clone();
    set_path(&mut changed, &["drive", "oauth_tokens", key], next);
    changed
}

fn changed_credentials(current: &Value, newer: &Value) -> Value {
    let mut changed = current.clone();
    set_path(&mut changed, &["drive", "oauth_tokens"], newer.clone());
    changed
}

fn set_profile_token(current: &mut Value, key: &str, next: Value) {
    let profiles = get_mut(current, "profiles");
    let Value::Arr(items) = profiles else {
        panic!("profiles is not an array: {}", profiles.to_string());
    };
    let profile = items.first_mut().unwrap_or_else(|| panic!("profiles is empty"));
    set_field(get_mut(profile, "oauth_tokens"), key, next);
}

fn drive_enabled(current: &Value) -> bool {
    current.get("drive").and_then(|drive| drive.get("oauth_enabled")).is_some_and(truthy)
}

fn not_busy(current: &Value) -> bool {
    current.get("busy").is_some_and(|item| !truthy(item))
}

fn callback_input(purpose: &str, response: &[u8]) -> Value {
    obj(vec![("purpose".into(), s(purpose)), ("data".into(), s(&b64_encode(response)))])
}

fn failure_input(purpose: &str) -> Value {
    obj(vec![
        ("purpose".into(), s(purpose)),
        ("sent".into(), b(false)),
        ("error".into(), s("controlled refusal")),
    ])
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("system clock is before the unix epoch: {error}"))
        .as_secs()
}

fn first_diff(left: &Value, right: &Value, path: &str) -> Option<String> {
    if left == right {
        return None;
    }
    match (left, right) {
        (Value::Obj(left_fields), Value::Obj(right_fields)) => {
            let mut keys: Vec<&str> = left_fields.iter().map(|(key, _)| key.as_str()).collect();
            for (key, _) in right_fields {
                if !keys.contains(&key.as_str()) {
                    keys.push(key.as_str());
                }
            }
            for key in keys {
                let left_value = left_fields.iter().find(|(name, _)| name == key).map(|(_, value)| value);
                let right_value = right_fields.iter().find(|(name, _)| name == key).map(|(_, value)| value);
                match (left_value, right_value) {
                    (None, Some(_)) => return Some(format!("{path}.{key} missing on returned state")),
                    (Some(_), None) => return Some(format!("{path}.{key} missing on sent state")),
                    (Some(left_item), Some(right_item)) => {
                        if let Some(diff) = first_diff(left_item, right_item, &format!("{path}.{key}")) {
                            return Some(diff);
                        }
                    }
                    (None, None) => {}
                }
            }
            Some(format!("{path} objects differ"))
        }
        (Value::Arr(left_items), Value::Arr(right_items)) => {
            if left_items.len() != right_items.len() {
                return Some(format!("{path} len {} vs {}", left_items.len(), right_items.len()));
            }
            for (index, (left_item, right_item)) in left_items.iter().zip(right_items).enumerate() {
                if let Some(diff) = first_diff(left_item, right_item, &format!("{path}[{index}]")) {
                    return Some(diff);
                }
            }
            Some(format!("{path} arrays differ"))
        }
        _ => {
            let left_text = left.to_string();
            let right_text = right.to_string();
            let left_clip: String = left_text.chars().take(180).collect();
            let right_clip: String = right_text.chars().take(180).collect();
            Some(format!("{path} {left_clip} vs {right_clip}"))
        }
    }
}

fn exchange(effect: &Value) -> Vec<u8> {
    let host = text(effect, "host");
    let port = match effect.get("port") {
        Some(Value::Num(value)) | Some(Value::Str(value)) => value.parse::<u16>().unwrap_or_else(|_| panic!("invalid port {value}")),
        other => panic!("missing port: {}", other.map(Value::to_string).unwrap_or_else(|| "absent".to_string())),
    };
    let wire = match effect.get("wire_base64") {
        Some(Value::Str(value)) => b64_decode(value),
        Some(other) => panic!("wire_base64 is not text: {}", other.to_string()),
        None => match effect.get("wire") {
            Some(Value::Str(value)) => value.as_bytes().to_vec(),
            _ => panic!("missing wire"),
        },
    };
    let mut stream = TcpStream::connect((host, port)).unwrap_or_else(|error| panic!("connect peer: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("write timeout: {error}"));
    stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
    stream.flush().unwrap_or_else(|error| panic!("peer flush: {error}"));
    let mut response = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => response.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
        }
    }
    response
}

fn fetch_redirect(url: &str) -> String {
    let rest = url.strip_prefix("http://").unwrap_or_else(|| panic!("authorization URL is not http: {url}"));
    let (authority, path) = rest.split_once('/').unwrap_or_else(|| panic!("authorization URL has no path: {url}"));
    let mut stream = TcpStream::connect(authority).unwrap_or_else(|error| panic!("authorize connect: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("authorize timeout: {error}"));
    let request = format!("GET /{path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap_or_else(|error| panic!("authorize write: {error}"));
    let mut response = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => response.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("authorize read: {error}"),
        }
    }
    let text = String::from_utf8_lossy(&response);
    let status = text.lines().next().unwrap_or("");
    if !status.contains(" 302 ") {
        panic!("authorization redirect status was {status}");
    }
    text.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("location") {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("authorization redirect has no Location"))
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "storage-oauth"))
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

struct Authorization {
    redirect_uri: String,
    code_challenge: String,
    client_id: String,
    scope: String,
}

struct Call {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct PeerData {
    origin: String,
    codes: HashMap<String, Authorization>,
    refreshes: HashMap<String, String>,
    accesses: HashMap<String, String>,
    devices: HashMap<String, String>,
    files: HashMap<String, Vec<u8>>,
    calls: Vec<Call>,
}

struct Peer {
    origin: String,
    data: Arc<Mutex<PeerData>>,
    running: Arc<Mutex<bool>>,
    port: Option<u16>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")).port();
        let origin = format!("http://127.0.0.1:{port}");
        let mut files = HashMap::new();
        files.insert("/share/alpha.txt".to_string(), b"Actual authorized storage text.".to_vec());
        let data = Arc::new(Mutex::new(PeerData {
            origin: origin.clone(),
            codes: HashMap::new(),
            refreshes: HashMap::new(),
            accesses: HashMap::new(),
            devices: HashMap::new(),
            files,
            calls: Vec::new(),
        }));
        let running = Arc::new(Mutex::new(true));
        let thread_data = Arc::clone(&data);
        let thread_running = Arc::clone(&running);
        let thread = thread::spawn(move || {
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if !*thread_running.lock().unwrap_or_else(|error| panic!("peer stop lock: {error}")) {
                            break;
                        }
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                        if let Err(error) = handle_request(&mut stream, &thread_data) {
                            eprintln!("storage peer: {error}");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });
        Self { origin, data, running, port: Some(port), thread: Some(thread) }
    }

    fn origin(&self) -> String {
        self.origin.clone()
    }

    fn call_count(&self) -> usize {
        self.data.lock().unwrap_or_else(|error| panic!("peer lock: {error}")).calls.len()
    }

    fn last_header(&self, name: &str) -> Option<String> {
        self.data
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .calls
            .last()
            .and_then(|call| call.headers.get(name).cloned())
    }

    fn call_log(&self) -> String {
        self.data
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .calls
            .iter()
            .map(|call| format!("{} {} body={}", call.method, call.path, call.body.len()))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn shutdown(&mut self) {
        if let Some(port) = self.port.take() {
            *self.running.lock().unwrap_or_else(|error| panic!("peer stop lock: {error}")) = false;
            let _ = TcpStream::connect(("127.0.0.1", port));
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn handle_request(stream: &mut TcpStream, shared: &Arc<Mutex<PeerData>>) -> Result<(), String> {
    let (method, target, headers, body) = read_request(stream)?;
    let path = target.split('?').next().unwrap_or("/");
    let query = target.split_once('?').map(|(_, value)| value).unwrap_or("");
    if method == "GET" && path == "/authorize" {
        let location = {
            let mut data = shared.lock().map_err(|_| "peer lock".to_string())?;
            issue_redirect(&mut data, query)?
        };
        write_redirect(stream, &location)
    } else {
        let reply = {
            let mut data = shared.lock().map_err(|_| "peer lock".to_string())?;
            data.calls.push(Call { method: method.clone(), path: target.clone(), headers: headers.clone(), body: body.clone() });
            dispatch(&mut data, &method, &target, path, &headers, &body)
        };
        write_bytes(stream, reply.0, reply.1, &reply.2)
    }
}

fn dispatch(
    data: &mut PeerData,
    method: &str,
    target: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> (u16, &'static str, Vec<u8>) {
    if path.starts_with("/model/") {
        return model_reply(data, headers, path);
    }
    if method == "POST" && path == "/device" {
        return (200, "OK", device_reply(data, body));
    }
    if method == "POST" && path == "/token" {
        return token_reply(data, body);
    }
    if method == "POST" {
        return (404, "Not Found", Vec::new());
    }
    if matches!(method, "GET" | "PROPFIND" | "PUT" | "MKCOL") {
        return storage_reply(data, method, target, headers, body);
    }
    (501, "Not Implemented", Vec::new())
}

fn issue_redirect(data: &mut PeerData, query: &str) -> Result<String, String> {
    let params = parse_form(query.as_bytes());
    let redirect = params.get("redirect_uri").cloned().ok_or_else(|| "authorization request omits redirect_uri".to_string())?;
    let state_value = params.get("state").cloned().ok_or_else(|| "authorization request omits state".to_string())?;
    let challenge = params.get("code_challenge").cloned().ok_or_else(|| "authorization request omits code_challenge".to_string())?;
    let client = params.get("client_id").cloned().ok_or_else(|| "authorization request omits client_id".to_string())?;
    let scope = params.get("scope").cloned().unwrap_or_else(|| "storage".to_string());
    let code = token_hex(12);
    data.codes.insert(
        code.clone(),
        Authorization {
            redirect_uri: redirect.clone(),
            code_challenge: challenge,
            client_id: client,
            scope,
        },
    );
    Ok(format!(
        "{redirect}?code={}&state={}&iss={}",
        urlencode(&code),
        urlencode(&state_value),
        urlencode(&data.origin)
    ))
}

fn token_reply(data: &mut PeerData, body: &[u8]) -> (u16, &'static str, Vec<u8>) {
    let params = parse_form(body);
    match params.get("grant_type").map(String::as_str).unwrap_or("") {
        "authorization_code" => {
            let code = params.get("code").cloned().unwrap_or_default();
            let Some(config) = data.codes.remove(&code) else {
                return (400, "Bad Request", error_json("invalid_grant"));
            };
            let proof = s256(params.get("code_verifier").map(String::as_str).unwrap_or(""));
            if proof != config.code_challenge
                || params.get("redirect_uri").map(String::as_str) != Some(config.redirect_uri.as_str())
                || params.get("client_id").map(String::as_str) != Some(config.client_id.as_str())
            {
                return (400, "Bad Request", error_json("invalid_grant"));
            }
            (200, "OK", issue(data, &config.scope))
        }
        "refresh_token" => {
            let refresh = params.get("refresh_token").cloned().unwrap_or_default();
            let Some(scope) = data.refreshes.remove(&refresh) else {
                return (400, "Bad Request", error_json("invalid_grant"));
            };
            (200, "OK", issue(data, &scope))
        }
        "urn:ietf:params:oauth:grant-type:device_code" => {
            let code = params.get("device_code").cloned().unwrap_or_default();
            let Some(scope) = data.devices.remove(&code) else {
                return (400, "Bad Request", error_json("invalid_grant"));
            };
            (200, "OK", issue(data, &scope))
        }
        _ => (400, "Bad Request", error_json("unsupported_grant_type")),
    }
}

fn device_reply(data: &mut PeerData, body: &[u8]) -> Vec<u8> {
    let params = parse_form(body);
    let code = token_hex(12);
    let scope = params.get("scope").cloned().unwrap_or_else(|| "storage".to_string());
    data.devices.insert(code.clone(), scope);
    obj(vec![
        ("device_code".into(), s(&code)),
        ("user_code".into(), s("OWNED-CODE")),
        ("verification_uri".into(), s(&format!("{}/verify", data.origin))),
        ("expires_in".into(), n("120")),
        ("interval".into(), n("1")),
    ])
    .to_string()
    .into_bytes()
}

fn issue(data: &mut PeerData, scope: &str) -> Vec<u8> {
    let access = format!("{scope}-access-{}", token_hex(8));
    let refresh = format!("{scope}-refresh-{}", token_hex(8));
    data.accesses.insert(access.clone(), scope.to_string());
    data.refreshes.insert(refresh.clone(), scope.to_string());
    obj(vec![
        ("token_type".into(), s("Bearer")),
        ("access_token".into(), s(&access)),
        ("refresh_token".into(), s(&refresh)),
        ("expires_in".into(), n("3600")),
    ])
    .to_string()
    .into_bytes()
}

fn model_reply(data: &PeerData, headers: &HashMap<String, String>, path: &str) -> (u16, &'static str, Vec<u8>) {
    let auth = headers.get("authorization").cloned().unwrap_or_default();
    let token = auth.strip_prefix("Bearer ").unwrap_or(auth.as_str());
    let scope = data.accesses.get(token).map(String::as_str);
    if auth != "Bearer primary-key" && auth != "Bearer child-key" && scope != Some("conversation") {
        return (401, "Unauthorized", Vec::new());
    }
    let format = path.rsplit('/').next().unwrap_or("");
    match format {
        "chat" => (
            200,
            "OK",
            obj(vec![(
                "choices".into(),
                Value::Arr(vec![obj(vec![
                    (
                        "message".into(),
                        obj(vec![("role".into(), s("assistant")), ("content".into(), s("Actual primary response."))]),
                    ),
                    ("finish_reason".into(), s("stop")),
                ])]),
            )])
            .to_string()
            .into_bytes(),
        ),
        "local" => (
            200,
            "OK",
            obj(vec![
                ("message".into(), obj(vec![("content".into(), s("Actual primary response."))])),
                ("done".into(), b(true)),
            ])
            .to_string()
            .into_bytes(),
        ),
        "messages" => (
            200,
            "OK",
            obj(vec![
                ("role".into(), s("assistant")),
                (
                    "content".into(),
                    Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s("Actual primary response."))])]),
                ),
                ("stop_reason".into(), s("end_turn")),
            ])
            .to_string()
            .into_bytes(),
        ),
        "content" => (
            200,
            "OK",
            obj(vec![(
                "candidates".into(),
                Value::Arr(vec![obj(vec![
                    (
                        "content".into(),
                        obj(vec![(
                            "parts".into(),
                            Value::Arr(vec![obj(vec![("text".into(), s("Actual primary response."))])]),
                        )]),
                    ),
                    ("finishReason".into(), s("STOP")),
                ])]),
            )])
            .to_string()
            .into_bytes(),
        ),
        _ => (404, "Not Found", Vec::new()),
    }
}

fn storage_reply(
    data: &mut PeerData,
    method: &str,
    target: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> (u16, &'static str, Vec<u8>) {
    let auth = headers.get("authorization").cloned().unwrap_or_default();
    let token = auth.strip_prefix("Bearer ").unwrap_or(auth.as_str());
    let scope = data.accesses.get(token).map(String::as_str);
    if scope != Some("storage") && headers.get("x-storage-key").map(String::as_str) != Some("Manual retained-key") {
        return (401, "Unauthorized", Vec::new());
    }
    if method == "PROPFIND" {
        return (207, "Multi-Status", LISTING.to_vec());
    }
    if method == "PUT" {
        if headers.get("if-none-match").map(String::as_str) == Some("*") && data.files.contains_key(target) {
            return (412, "Precondition Failed", Vec::new());
        }
        data.files.insert(target.to_string(), body.to_vec());
        return (201, "Created", Vec::new());
    }
    if method == "MKCOL" {
        return (201, "Created", Vec::new());
    }
    if let Some(file) = data.files.get(target) {
        return (200, "OK", file.clone());
    }
    (404, "Not Found", Vec::new())
}

fn error_json(error: &str) -> Vec<u8> {
    obj(vec![("error".into(), s(error))]).to_string().into_bytes()
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String, HashMap<String, String>, Vec<u8>), String> {
    let mut buffer = Vec::new();
    let mut temporary = [0u8; 8192];
    let header_end = loop {
        let count = read_chunk(stream, &mut temporary)?;
        if count == 0 {
            return Err("request ended before headers".to_string());
        }
        buffer.extend_from_slice(&temporary[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > 1_048_576 {
            return Err("request headers are too large".to_string());
        }
    };
    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let length = content_length(&header);
    if length > 8 * 1024 * 1024 {
        return Err("request body is too large".to_string());
    }
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let count = read_chunk(stream, &mut temporary)?;
        if count == 0 {
            return Err("request ended before the body".to_string());
        }
        body.extend_from_slice(&temporary[..count]);
    }
    body.truncate(length);
    let mut lines = header.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok((method, target, headers, body))
}

fn read_chunk(stream: &mut TcpStream, buffer: &mut [u8]) -> Result<usize, String> {
    loop {
        match stream.read(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("request read: {error}")),
        }
    }
}

fn content_length(header: &str) -> usize {
    header
        .split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn write_redirect(stream: &mut TcpStream, location: &str) -> Result<(), String> {
    let header = format!("HTTP/1.0 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(header.as_bytes()).map_err(|error| format!("redirect write: {error}"))?;
    stream.flush().map_err(|error| format!("redirect flush: {error}"))?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn write_bytes(stream: &mut TcpStream, status: u16, reason: &str, body: &[u8]) -> Result<(), String> {
    let header = format!("HTTP/1.0 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(header.as_bytes()).map_err(|error| format!("response write: {error}"))?;
    stream.write_all(body).map_err(|error| format!("response body: {error}"))?;
    stream.flush().map_err(|error| format!("response flush: {error}"))?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn parse_form(body: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(body);
    let mut values = HashMap::new();
    for pair in text.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = url_decode(value);
        if value.is_empty() {
            continue;
        }
        values.entry(url_decode(key)).or_insert(value);
    }
    values
}

fn url_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    index += 3;
                } else {
                    out.push(b'%');
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn urlencode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else if byte == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn s256(verifier: &str) -> String {
    b64_encode(&sha256_digest(verifier.as_bytes()))
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

fn token_hex(nbytes: usize) -> String {
    let mut bytes = vec![0u8; nbytes];
    let mut file = std::fs::File::open("/dev/urandom").unwrap_or_else(|error| panic!("urandom: {error}"));
    file.read_exact(&mut bytes).unwrap_or_else(|error| panic!("urandom read: {error}"));
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(nbytes * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

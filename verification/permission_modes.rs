mod twin;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner and manual operations through current public events against a real workspace and a local search endpoint: mode menu, per-connection dropdowns, never-reviewed web search, review paths, auto-dispatch and named refusals. Model-proposed operations reuse the same automatic_approval and harness_rules gates; physical Android rendering of the footer control remains separate.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let server = SearchServer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.workspace(), &server.url);
    controller.finish();
    write_report(&passed);
}

fn run(controller: &mut Controller, root: &str, search_url: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let initial = state(&resumed).clone();
    keep(
        &mut passed,
        "the chat surface carries the default harness permission mode",
        text_is(ui_of(&resumed), "permission_mode", "review") && text_is(ui_of(&resumed), "permission_label", "Ask"),
    );

    let configured = controller.event(Some(&initial), "config", obj(vec![]), root);
    keep(
        &mut passed,
        "app settings exposes the permissions screen",
        events(&configured).iter().any(|event| event == "permissions"),
    );

    let menu = controller.event(Some(&initial), "permission_modes", obj(vec![]), root);
    keep(
        &mut passed,
        "the permission menu offers exactly the three modes",
        events(&menu) == ["permission_mode:review", "permission_mode:limited", "permission_mode:yolo"]
            && text_is(state(&menu), "dialog", "permission_modes"),
    );

    let actions = controller.event(Some(&initial), "chat_actions", obj(vec![]), root);
    keep(
        &mut passed,
        "the conversation menu names the current mode",
        buttons(&actions).iter().any(|button| text_is(button, "label", "Permission mode · Ask")),
    );

    let mut connection = controller.event(Some(&initial), "profile_new", obj(vec![]), root);
    connection = controller.event(
        Some(state(&connection)),
        "profile_save",
        obj(vec![
            ("name".into(), s("Probe")),
            ("endpoint".into(), s("http://127.0.0.1:9311/v1")),
            ("model".into(), s("probe-model")),
            ("protocol".into(), s("chat")),
        ]),
        root,
    );
    keep(
        &mut passed,
        "a connection is selected before permissions are configured",
        profile_name(&connection) == Some("Probe"),
    );

    let permissions = controller.event(Some(state(&connection)), "permissions", obj(vec![]), root);
    keep(
        &mut passed,
        "permissions carries one dropdown per limit class for each connection",
        field_keys(&permissions) == ["mode", "mode_0", "web_0", "file_0"] && all_choices(&permissions),
    );
    keep(
        &mut passed,
        "every limit dropdown offers the inherit arm",
        offers_inherit(&permissions, "web_0") && offers_inherit(&permissions, "file_0"),
    );
    keep(
        &mut passed,
        "the mode menu states that web search runs without a review",
        field_value_contains(&menu, "description", "never reviewed"),
    );

    let closed = controller.event(Some(state(&permissions)), "close", obj(vec![]), root);
    let ran = searched(controller, state(&closed), root, search_url, "permission modes", None);
    keep(
        &mut passed,
        "Ask mode runs a web search with no review dialog",
        !absent(&ran, "effect")
            && effect_has(&ran, "host", "127.0.0.1")
            && effect_has(&ran, "purpose", "web_tool")
            && !absent(state(&ran), "web_expected")
            && absent(state(&ran), "pending")
            && absent(ui_of(&ran), "modal"),
    );

    let canceled = controller.event(Some(state(&ran)), "cancel", obj(vec![]), root);
    let held = searched(controller, state(&canceled), root, search_url, "reviewed search", Some(false));
    keep(
        &mut passed,
        "the app-wide search review setting still asks",
        absent(&held, "effect") && object_text(state(&held), "pending", "tool") == Some("web_search"),
    );

    let rejected = controller.event(Some(state(&held)), "reject", obj(vec![]), root);
    let reviewed = reviewed_read(controller, state(&rejected), root);
    keep(
        &mut passed,
        "the default mode reviews a manual workspace read",
        absent(&reviewed, "effect")
            && text_is(state(&reviewed), "dialog", "approval")
            && object_text(state(&reviewed), "pending", "tool") == Some("read"),
    );
    let rejected_read = controller.event(Some(state(&reviewed)), "reject", obj(vec![]), root);
    keep(
        &mut passed,
        "rejecting a reviewed read executes nothing",
        absent(&rejected_read, "effect") && absent(state(&rejected_read), "pending"),
    );

    let saved = controller.event(
        Some(state(&rejected_read)),
        "permissions_save",
        obj(vec![("mode".into(), s("yolo")), ("mode_0".into(), s("yolo"))]),
        root,
    );
    keep(
        &mut passed,
        "saving permissions reports and applies the harness mode",
        text_is(state(&saved), "permission_mode", "yolo")
            && text_is(ui_of(&saved), "permission_label", "YOLO")
            && notice(&saved).contains("YOLO"),
    );
    let yolo_read = reviewed_read(controller, state(&saved), root);
    keep(
        &mut passed,
        "YOLO dispatches the same read with no review dialog",
        !absent(&yolo_read, "effect")
            && effect_has(&yolo_read, "kind", "tool")
            && effect_has(&yolo_read, "tool", "read")
            && absent(state(&yolo_read), "pending")
            && absent(ui_of(&yolo_read), "modal"),
    );

    let canceled = controller.event(Some(state(&yolo_read)), "cancel", obj(vec![]), root);
    let tighter = controller.event(
        Some(state(&canceled)),
        "permissions_save",
        obj(vec![("mode".into(), s("yolo")), ("mode_0".into(), s("review"))]),
        root,
    );
    let tighter_read = reviewed_read(controller, state(&tighter), root);
    keep(
        &mut passed,
        "a tighter per-connection mode restores review while the harness stays YOLO",
        text_is(state(&tighter_read), "dialog", "approval")
            && object_text(state(&tighter_read), "pending", "tool") == Some("read")
            && text_is(state(&tighter_read), "permission_mode", "yolo"),
    );
    let blocked = controller.event(
        Some(state(&tighter_read)),
        "permissions_save",
        obj(vec![("mode".into(), s("yolo")), ("mode_0".into(), s("yolo"))]),
        root,
    );
    keep(
        &mut passed,
        "permissions cannot be saved over an open review",
        notice(&blocked).contains("Finish or cancel the current task or tool review first") && notice_error(&blocked),
    );

    let rejected = controller.event(Some(state(&blocked)), "reject", obj(vec![]), root);
    let refused = controller.event(
        Some(state(&rejected)),
        "permissions_save",
        obj(vec![("mode".into(), s("reckless"))]),
        root,
    );
    keep(
        &mut passed,
        "an unsupported permission mode is refused",
        notice(&refused).contains("Choose a supported permission mode") && notice_error(&refused),
    );

    let file_off = controller.event(
        Some(state(&refused)),
        "permissions_save",
        obj(vec![
            ("mode".into(), s("limited")),
            ("mode_0".into(), s("review")),
            ("file_0".into(), s("off")),
        ]),
        root,
    );
    keep(
        &mut passed,
        "the file limit is stored per connection",
        permissions_are(
            &file_off,
            &obj(vec![(
                "0".into(),
                obj(vec![("mode".into(), s("review")), ("file".into(), s("off"))]),
            )]),
        ),
    );
    let staged = reviewed_read(controller, state(&file_off), root);
    keep(
        &mut passed,
        "the file-off connection stages its read like any other review",
        absent(&staged, "effect") && object_text(state(&staged), "pending", "tool") == Some("read"),
    );
    let approved = controller.event(Some(state(&staged)), "approve", obj(vec![]), root);
    keep(
        &mut passed,
        "file access off refuses dispatch and names the connection",
        absent(&approved, "effect")
            && notice_error(&approved)
            && notice(&approved).contains("File access is off for Probe (App settings → Permissions)"),
    );

    let rejected = controller.event(Some(state(&approved)), "reject", obj(vec![]), root);
    let web_off = controller.event(
        Some(state(&rejected)),
        "permissions_save",
        obj(vec![
            ("mode".into(), s("limited")),
            ("mode_0".into(), s("yolo")),
            ("web_0".into(), s("off")),
        ]),
        root,
    );
    let held = searched(controller, state(&web_off), root, search_url, "blocked search", None);
    keep(
        &mut passed,
        "web access off blocks that connection search before execution",
        absent(&held, "effect")
            && notice_error(&held)
            && notice(&held).contains("Web access is off for Probe (App settings → Permissions)")
            && absent(state(&held), "pending"),
    );

    let web_on = controller.event(
        Some(state(&held)),
        "permissions_save",
        obj(vec![("mode".into(), s("limited")), ("mode_0".into(), s("yolo"))]),
        root,
    );
    let ran = searched(controller, state(&web_on), root, search_url, "allowed search", None);
    keep(
        &mut passed,
        "the same connection searches without review once web access is back on",
        !absent(&ran, "effect") && effect_has(&ran, "host", "127.0.0.1") && absent(state(&ran), "pending"),
    );

    let canceled = controller.event(Some(state(&ran)), "cancel", obj(vec![]), root);
    let limited = controller.event(
        Some(state(&canceled)),
        "permissions_save",
        obj(vec![("mode".into(), s("limited")), ("mode_0".into(), s("limited"))]),
        root,
    );
    let write = reviewed_write(controller, state(&limited), root, "Written under Limited\n");
    keep(
        &mut passed,
        "Limited reviews a write",
        absent(&write, "effect")
            && text_is(state(&write), "dialog", "approval")
            && object_text(state(&write), "pending", "tool") == Some("write"),
    );

    let rejected = controller.event(Some(state(&write)), "reject", obj(vec![]), root);
    let yolo = controller.event(
        Some(state(&rejected)),
        "permissions_save",
        obj(vec![("mode".into(), s("limited")), ("mode_0".into(), s("yolo"))]),
        root,
    );
    let wrote = reviewed_write(controller, state(&yolo), root, "Written by YOLO\n");
    keep(
        &mut passed,
        "a YOLO connection writes without a review",
        !absent(&wrote, "effect")
            && effect_has(&wrote, "kind", "tool")
            && effect_has(&wrote, "tool", "write")
            && absent(state(&wrote), "pending"),
    );

    let canceled = controller.event(Some(state(&wrote)), "cancel", obj(vec![]), root);
    let limited_again = controller.event(
        Some(state(&canceled)),
        "permissions_save",
        obj(vec![("mode".into(), s("limited")), ("mode_0".into(), s("limited"))]),
        root,
    );
    let limited_read = reviewed_read(controller, state(&limited_again), root);
    keep(
        &mut passed,
        "Limited runs a read-only operation without a dialog",
        !absent(&limited_read, "effect")
            && effect_has(&limited_read, "kind", "tool")
            && effect_has(&limited_read, "tool", "read")
            && absent(state(&limited_read), "pending"),
    );

    let canceled = controller.event(Some(state(&limited_read)), "cancel", obj(vec![]), root);
    let back = controller.event(
        Some(state(&canceled)),
        "permissions_save",
        obj(vec![("mode".into(), s("review")), ("mode_0".into(), s("inherit"))]),
        root,
    );
    keep(
        &mut passed,
        "clearing the per-connection limits leaves only the harness mode",
        permissions_are(&back, &obj(vec![])) && text_is(ui_of(&back), "permission_mode", "review"),
    );
    let cleared = reviewed_read(controller, state(&back), root);
    keep(
        &mut passed,
        "cleared limits put an ordinary operation back on the review path",
        absent(&cleared, "effect")
            && text_is(state(&cleared), "dialog", "approval")
            && object_text(state(&cleared), "pending", "tool") == Some("read"),
    );
    passed
}

fn searched(
    controller: &mut Controller,
    current: &Value,
    root: &str,
    search_url: &str,
    query: &str,
    without_review: Option<bool>,
) -> Value {
    let prepared = with_web(current, search_url, without_review);
    let opened = controller.event(Some(&prepared), "web_search", obj(vec![]), root);
    controller.event(
        Some(state(&opened)),
        "web_search_review",
        obj(vec![("query".into(), s(query))]),
        root,
    )
}

fn with_web(current: &Value, search_url: &str, without_review: Option<bool>) -> Value {
    let mut next = current.clone();
    let mut settings = vec![("search_url".into(), s(search_url))];
    if let Some(flag) = without_review {
        settings.push(("search_without_review".into(), Value::Bool(flag)));
    }
    replace_key(&mut next, "web", obj(settings));
    next
}

fn replace_key(value: &mut Value, key: &str, item: Value) {
    let Value::Obj(fields) = value else {
        panic!("cannot set {key} on a non-object");
    };
    if let Some(slot) = fields.iter_mut().find(|(name, _)| name == key) {
        slot.1 = item;
    } else {
        fields.push((key.to_string(), item));
    }
}

fn reviewed_read(controller: &mut Controller, current: &Value, root: &str) -> Value {
    let opened = controller.event(Some(current), "reader", obj(vec![]), root);
    controller.event(
        Some(state(&opened)),
        "manual_tool",
        obj(vec![("path".into(), s("keep.txt"))]),
        root,
    )
}

fn reviewed_write(controller: &mut Controller, current: &Value, root: &str, content: &str) -> Value {
    let opened = controller.event(
        Some(current),
        "editor",
        obj(vec![("path".into(), s("new.txt")), ("content".into(), s(content))]),
        root,
    );
    controller.event(
        Some(state(&opened)),
        "editor_save",
        obj(vec![("path".into(), s("new.txt")), ("content".into(), s(content))]),
        root,
    )
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn ui_of(value: &Value) -> &Value {
    value.get("ui").unwrap_or_else(|| panic!("event returned no ui: {}", value.to_string()))
}

fn absent(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn text_is(value: &Value, key: &str, expected: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expected)
}

fn object_text<'a>(value: &'a Value, key: &str, child: &str) -> Option<&'a str> {
    let parent = value.get(key)?;
    match parent {
        Value::Obj(_) => match parent.get(child)? {
            Value::Str(text) => Some(text.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn profile_name(value: &Value) -> Option<&str> {
    let profiles = state(value).get("profiles")?;
    let Value::Arr(items) = profiles else {
        return None;
    };
    match items.first().and_then(|profile| profile.get("name")) {
        Some(Value::Str(name)) => Some(name.as_str()),
        _ => None,
    }
}

fn buttons(value: &Value) -> Vec<&Value> {
    let ui = ui_of(value);
    if let Some(modal) = ui.get("modal") {
        if !matches!(modal, Value::Null) {
            if let Some(Value::Arr(items)) = modal.get("buttons") {
                return items.iter().collect();
            }
        }
    }
    match ui.get("rows") {
        Some(Value::Arr(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

fn events(value: &Value) -> Vec<String> {
    buttons(value)
        .into_iter()
        .filter_map(|button| match button.get("event") {
            Some(Value::Str(event)) => Some(event.clone()),
            _ => None,
        })
        .collect()
}

fn modal_fields(value: &Value) -> Vec<&Value> {
    let Some(modal) = ui_of(value).get("modal") else {
        return Vec::new();
    };
    if matches!(modal, Value::Null) {
        return Vec::new();
    }
    match modal.get("fields") {
        Some(Value::Arr(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

fn modal_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    modal_fields(value).into_iter().find(|field| text_is(field, "key", key))
}

fn field_keys(value: &Value) -> Vec<String> {
    modal_fields(value)
        .into_iter()
        .filter_map(|field| match field.get("key") {
            Some(Value::Str(key)) => Some(key.clone()),
            _ => None,
        })
        .collect()
}

fn all_choices(value: &Value) -> bool {
    let fields = modal_fields(value);
    !fields.is_empty() && fields.iter().all(|field| text_is(field, "kind", "choice"))
}

fn offers_inherit(value: &Value, key: &str) -> bool {
    let Some(field) = modal_field(value, key) else {
        return false;
    };
    match field.get("options") {
        Some(Value::Arr(options)) => options.iter().any(|option| text_is(option, "value", "inherit")),
        _ => false,
    }
}

fn field_value_contains(value: &Value, key: &str, needle: &str) -> bool {
    match modal_field(value, key).and_then(|field| field.get("value")) {
        Some(Value::Str(text)) => text.contains(needle),
        _ => false,
    }
}

fn notice(value: &Value) -> &str {
    match state(value).get("notice") {
        Some(Value::Str(text)) => text,
        _ => "",
    }
}

fn notice_error(value: &Value) -> bool {
    matches!(state(value).get("notice_error"), Some(Value::Bool(true)))
}

fn permissions_are(value: &Value, expected: &Value) -> bool {
    matches!(state(value).get("permissions"), Some(found) if found == expected)
}

fn effect_has(value: &Value, key: &str, expected: &str) -> bool {
    match value.get("effect") {
        Some(effect) => text_is(effect, key, expected),
        None => false,
    }
}

fn write_report(passed: &[String]) {
    let checks = Value::Arr(passed.iter().map(|name| s(name)).collect());
    let body = obj(vec![
        ("passed".into(), Value::Num(passed.len().to_string())),
        ("checks".into(), checks),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value_printed("permission_modes", &body, true);
}

struct Scratch {
    root: PathBuf,
    workspace: PathBuf,
}

impl Scratch {
    fn create(root: &Path) -> Self {
        let scratch = twin::create_scratch(root, "permission-modes");
        let workspace = scratch.join("workspace");
        std::fs::create_dir(&workspace)
            .unwrap_or_else(|error| panic!("could not create {}: {error}", workspace.display()));
        std::fs::write(workspace.join("keep.txt"), "Original saved work\n")
            .unwrap_or_else(|error| panic!("could not write fixture: {error}"));
        Self { root: scratch, workspace }
    }

    fn workspace(&self) -> &str {
        self.workspace.to_str().unwrap_or_else(|| panic!("workspace path is not UTF-8"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct SearchServer {
    listener: TcpListener,
    url: String,
}

impl SearchServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("could not bind search server: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("search listener has no address: {error}")).port();
        let server = Self {
            listener,
            url: format!("http://127.0.0.1:{port}/search?q={{query}}"),
        };
        let listener = server
            .listener
            .try_clone()
            .unwrap_or_else(|error| panic!("could not clone listener: {error}"));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                thread::spawn(move || serve(stream));
            }
        });
        server
    }
}

impl Drop for SearchServer {
    fn drop(&mut self) {
        let _ = self.listener.local_addr();
    }
}

fn serve(mut stream: std::net::TcpStream) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let mut reader = match stream.try_clone() {
        Ok(cloned) => BufReader::new(cloned),
        Err(_) => return,
    };
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let query = request
        .split_whitespace()
        .nth(1)
        .and_then(|target| target.split('?').nth(1))
        .and_then(|query| query.split('&').find_map(|part| part.strip_prefix("q=")))
        .map(percent_decode)
        .unwrap_or_default();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line == "\n" || line == "\r\n" => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    let body = format!(
        "{{\"query\": \"{}\", \"number_of_results\": 1, \"results\": [{{\"url\": \"https://example.test/permission-modes\", \"title\": \"Permission modes\", \"content\": \"Harness permission modes gate the review dialog.\", \"engine\": \"probe\"}}]}}",
        json_escape(&query)
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'+' {
            result.push(b' ');
            index += 1;
        } else if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                result.push(value);
                index += 3;
            } else {
                result.push(bytes[index]);
                index += 1;
            }
        } else {
            result.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(result).unwrap_or_default()
}

fn json_escape(text: &str) -> String {
    let mut out = String::new();
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
    out
}

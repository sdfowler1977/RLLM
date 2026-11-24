//! On-demand MCP for saved connections, agent tasks, and package operations.
use super::*;
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex};

#[derive(Default)]
struct TaskRequests {
    queued: Vec<(Json, bool)>,
    active: Option<Json>,
}

fn task_request_id(request: &Json) -> Option<&Json> {
    let id = get(request, "id");
    (id != &Null && text(request, "method") == "tools/call"
        && text(get(request, "params"), "name") == "agent_task").then_some(id)
}

pub(super) fn serve(state: &mut Json, vault: &Vault, root: &str) -> Result<()> {
    let mut writer = std::io::BufWriter::new(std::io::stdout());
    let mut initialized = false;
    let mut ready = false;
    let tasks = Arc::new(Mutex::new(TaskRequests::default()));
    let reading = Arc::clone(&tasks);
    let (send, receive) = mpsc::sync_channel(8);
    // Keep reading cancellation notifications while a provider request is in flight.
    std::thread::Builder::new().spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        loop {
            let line = match next_line_bounded(&mut reader, 8 * 1024 * 1024) {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(error) => {
                    let _ = send.send(Err(error));
                    break;
                }
            };
            if line.trim().is_empty() { continue; }
            let request = parse(line.trim());
            if let Ok(request) = &request {
                if get(request, "id") == &Null && text(request, "method") == "notifications/cancelled" {
                    let request_id = get(get(request, "params"), "requestId");
                    let mut tasks = reading.lock().unwrap();
                    if tasks.active.as_ref() == Some(request_id) {
                        CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
                    } else if let Some((_, cancelled)) = tasks.queued.iter_mut().find(|(id, _)| id == request_id) {
                        *cancelled = true;
                    }
                    continue;
                }
                if let Some(id) = task_request_id(request) {
                    reading.lock().unwrap().queued.push((id.clone(), false));
                }
            }
            if send.send(Ok(request)).is_err() { break; }
        }
    }).map_err(|error| format!("MCP input: {error}"))?;
    for item in receive {
        let response = match item? {
            Ok(request) => {
                let params = get(&request, "params");
                let tracking = task_request_id(&request).is_some_and(|id| {
                    let mut tasks = tasks.lock().unwrap();
                    let cancelled = tasks.queued.iter().position(|(queued, _)| queued == id)
                        .is_some_and(|index| tasks.queued.remove(index).1);
                    if ready {
                        tasks.active = Some(id.clone());
                        CANCEL.store(cancelled, std::sync::atomic::Ordering::Relaxed);
                    }
                    ready
                });
                let token = get(get(params, "_meta"), "progressToken");
                let mut progress = 0u64;
                let mut progress_error = None;
                let response = handle(state, vault, root, &request, &mut initialized, &mut ready, &mut |message| {
                    if !matches!(token, Str(_) | Num(_)) || progress_error.is_some() { return; }
                    progress += 1;
                    let update = obj(vec![
                        ("jsonrpc", s("2.0")),
                        ("method", s("notifications/progress")),
                        ("params", obj(vec![("progressToken", token.clone()),
                            ("progress", Num(progress.to_string())), ("message", s(message))])),
                    ]);
                    progress_error = write_message(&mut writer, &update).err();
                });
                if tracking {
                    let mut tasks = tasks.lock().unwrap();
                    tasks.active = None;
                    CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
                }
                response
            }
            Err(error) => Some(rpc_error(&Null, -32700, &format!("Parse error: {error}"))),
        };
        if let Some(response) = response {
            write_message(&mut writer, &response)?;
        }
    }
    Ok(())
}

fn write_message(writer: &mut impl Write, message: &Json) -> Result<()> {
    writeln!(writer, "{}", message.string()).and_then(|_| writer.flush()).map_err(|error| error.to_string())
}

fn handle(state: &mut Json, vault: &Vault, root: &str, request: &Json, initialized: &mut bool, ready: &mut bool, report: &mut dyn FnMut(&str)) -> Option<Json> {
    let method = text(request, "method");
    let id = get(request, "id");
    if id == &Null {
        if method == "notifications/initialized" && *initialized {
            *ready = true;
        }
        return None;
    }
    let result = if method == "initialize" {
        *initialized = true;
        Ok(obj(vec![
            ("protocolVersion", s("2025-03-26")),
            ("capabilities", obj(vec![("tools", obj(vec![]))])),
            ("serverInfo", obj(vec![("name", s("RLLM")), ("version", s("1"))])),
            ("instructions", s("List saved connections, send a task to a named connection, or operate on supplied package data. Credentials are not exposed.")),
        ]))
    } else if !*ready {
        Err("Initialize the session and send notifications/initialized first".into())
    } else {
        match method.as_str() {
            "ping" => Ok(obj(vec![])),
            "tools/list" => Ok(obj(vec![("tools", Arr(tool_definitions()))])),
            "tools/call" => call(state, vault, root, get(request, "params"), report),
            _ => Err(format!("Method {method} is not supported")),
        }
    };
    Some(match result {
        Ok(value) => obj(vec![("jsonrpc", s("2.0")), ("id", id.clone()), ("result", value)]),
        Err(error) => rpc_error(id, -32601, &error),
    })
}

fn tool_definitions() -> Vec<Json> {
    let mut tools = vec![
        tool("connections", "List saved connection names and model ids. Credentials and endpoints are not included.", &[], vec![]),
        tool("agent_task", "Start a new conversation on the named saved connection and return the answer or pending review.", &[("name", "Exact saved connection name"), ("task", "The task text")], vec![
            ("mode", obj(vec![("type", s("string")), ("enum", Arr(MODES.iter().map(|mode| s(mode)).collect())), ("description", s("Optional permission mode for this task. Saved tool limits and review rules still apply."))])),
        ]),
    ];
    tools.extend(super::apk_build::TOOLS.iter().map(|name| package_tool(name)));
    tools
}

fn package_tool(name: &str) -> Json {
    const APK: (&str, &str) = ("apk_b64", "Base64-encoded APK bytes");
    const REPLACEMENT: &[(&str, &str)] = &[APK, ("old", "Exact existing text"), ("new", "Replacement text")];
    let dex_entry = || ("dex_entry", obj(vec![("type", s("string")), ("default", s("classes.dex"))]));
    let optional_apk = || (APK.0, obj(vec![("type", s("string")), ("description", s(APK.1))]));
    let alignment = || ("alignment", obj(vec![("type", s("integer")), ("default", Num("4".into()))]));
    match name {
        "apk_unpack" => tool(name, "Return archive members and their bytes.", &[APK], vec![]),
        "apk_read_strings" => tool(name, "Read manifest and resource strings.", &[APK], vec![]),
        "apk_edit_string" => tool(name, "Replace an exact resource string.", REPLACEMENT, vec![]),
        "apk_edit_xml" => tool(name, "Replace an exact binary manifest string.", REPLACEMENT, vec![]),
        "apk_smali" => {
            let mut definition = tool(name, "Disassemble supplied DEX bytes or a DEX member of an APK.", &[], vec![
                optional_apk(), ("dex_b64", obj(vec![("type", s("string"))])), dex_entry(),
            ]);
            let mut schema = get(&definition, "inputSchema").clone();
            put(&mut schema, "anyOf", Arr(["dex_b64", "apk_b64"].iter()
                .map(|field| obj(vec![("required", Arr(vec![s(field)]))])).collect()));
            put(&mut definition, "inputSchema", schema);
            definition
        }
        "apk_assemble" => tool(name, "Assemble smali into DEX, optionally replacing an APK member.", &[("smali", "Smali source text")], vec![optional_apk(), dex_entry()]),
        "apk_rebuild" => {
            let mut definition = tool(name, "Build an APK from archive entries.", &[], vec![
                ("entries", obj(vec![("type", s("array")), ("items", obj(vec![
                    ("type", s("object")),
                    ("properties", obj(vec![("name", obj(vec![("type", s("string"))])), ("data_b64", obj(vec![("type", s("string"))]))])),
                    ("required", Arr(vec![s("name"), s("data_b64")])),
                ]))])),
            ]);
            let mut schema = get(&definition, "inputSchema").clone();
            put(&mut schema, "required", Arr(vec![s("entries")]));
            put(&mut definition, "inputSchema", schema);
            definition
        }
        "apk_align" => tool(name, "Align APK member data and verify the alignment.", &[APK], vec![alignment()]),
        "apk_sign" => tool(name, "Sign the APK.", &[APK], vec![("bits", obj(vec![("type", s("integer")), ("default", Num("2048".into()))]))]),
        "apk_verify" => tool(name, "Verify APK signatures and alignment.", &[APK], vec![alignment()]),
        _ => tool(name, "Perform a package operation on supplied APK bytes.", &[APK], vec![]),
    }
}

fn tool(name: &str, description: &str, fields: &[(&'static str, &str)], mut properties: Vec<(&'static str, Json)>) -> Json {
    let mut required = Vec::new();
    for (name, description) in fields {
        properties.push((*name, obj(vec![("type", s("string")), ("description", s(description))])));
        required.push(s(name));
    }
    obj(vec![
        ("name", s(name)),
        ("description", s(description)),
        ("inputSchema", obj(vec![("type", s("object")), ("properties", obj(properties)), ("required", Arr(required)), ("additionalProperties", Bool(false))])),
    ])
}

fn call(state: &mut Json, vault: &Vault, root: &str, params: &Json, report: &mut dyn FnMut(&str)) -> Result<Json> {
    let name = text(params, "name");
    let args = get(params, "arguments");
    if super::apk_build::TOOLS.contains(&name.as_str()) {
        return Ok(match super::apk_build::run(&name, args) {
            Ok(result) => obj(vec![
                ("content", Arr(vec![text_content(&result.string())])),
                ("isError", Bool(false)),
            ]),
            Err(error) => obj(vec![
                ("content", Arr(vec![text_content(&error)])),
                ("isError", Bool(true)),
            ]),
        });
    }
    let content = match name.as_str() {
        "connections" => vec![text_content(&connections(state))],
        "agent_task" => {
            let (answer, transcript) = match agent_task(state, vault, root, args, report) {
                Ok(result) => result,
                Err(error) => return Ok(obj(vec![
                    ("content", Arr(vec![text_content(&error)])),
                    ("isError", Bool(true)),
                ])),
            };
            let mut content = vec![text_content(&answer)];
            if let Some(transcript) = transcript { content.push(text_content(&transcript)); }
            content
        }
        _ => return Err(format!("Unknown tool {name}")),
    };
    Ok(obj(vec![
        ("content", Arr(content)),
        ("isError", Bool(false)),
    ]))
}

fn text_content(text: &str) -> Json {
    obj(vec![("type", s("text")), ("text", s(text))])
}

fn connections(state: &Json) -> String {
    let Arr(profiles) = get(state, "profiles") else {
        return "No saved connections.".into();
    };
    if profiles.is_empty() {
        return "No saved connections.".into();
    }
    let mut lines = Vec::new();
    for (index, profile) in profiles.iter().enumerate() {
        lines.push(format!("{index} {} model {}", text(profile, "name"), text(profile, "model")));
    }
    lines.join("\n")
}

const MODES: [&str; 3] = ["review", "limited", "yolo"];

fn agent_task(state: &mut Json, vault: &Vault, root: &str, args: &Json, report: &mut dyn FnMut(&str)) -> Result<(String, Option<String>)> {
    if transport_cancelled() {
        return Ok(("Task cancelled before starting.".into(), None));
    }
    let name = text(args, "name");
    let task = text(args, "task");
    if name.is_empty() || task.is_empty() {
        return Err("agent_task requires name and task".into());
    }
    let mode = match args.get("mode") {
        Err(_) => None,
        Ok(Str(mode)) if MODES.contains(&mode.as_str()) => Some(mode.as_str()),
        Ok(_) => return Err("agent_task mode must be review, limited, or yolo".into()),
    };
    let Arr(profiles) = get(state, "profiles").clone() else {
        return Err("No saved connections".into());
    };
    let Some(index) = profiles.iter().position(|profile| text(profile, "name") == name) else {
        return Err(format!("No saved connection is named {name}"));
    };
    if get(state, "pending") != &Null || boolean(state, "busy") || get(state, "task_run") != &Null {
        return Err("The active conversation has unfinished work. Resolve or cancel it before starting a new MCP task.".into());
    }
    let mut next = state.clone();
    advance(&mut next, root, "new", Null)?;
    let active = text(&next, "active");
    let chat = active_chat(&next);
    if active.is_empty() || active == text(state, "active") || !matches!(get(chat, "messages"), Arr(messages) if messages.is_empty()) {
        return Err(format!("Could not start a new MCP conversation. {}", stop_reason(&next)));
    }
    advance(&mut next, root, &format!("chat_model:{index}"), Null)?;
    if get(active_chat(&next), "primary_connection") != &Num(index.to_string()) {
        return Err(stop_reason(&next));
    }
    super::application::mark_agent_task(&mut next);
    *state = next;
    let permissions = mode.map(|mode| SavedPermissions::override_for_task(state, index, mode));
    let mut progress = Progress::new(state, report);
    let sent = advance_tracking(state, root, "send", obj(vec![("draft", s(&task))]), &mut progress);
    let outcome = sent.and_then(|_| settle_tools(state, root, &mut progress));
    super::application::clear_caller_feedback();
    if let Some(permissions) = permissions { permissions.restore(state); }
    vault.save(state)?;
    let outcome = outcome?;
    if text(state, "active") != active || !active_messages(state).first().is_some_and(|message| text(message, "role") == "user" && text(message, "content") == task) {
        return Err(stop_reason(state));
    }
    let status = match outcome {
        TaskStop::Complete => return Ok((final_reply(state)?, Some(transcript(state)))),
        TaskStop::Review => "Task paused for review. To continue, open the saved conversation in the app and review the pending item.",
        TaskStop::Stalled => "Task stalled after repeated or prolonged lack of progress. Work so far was saved.",
        TaskStop::Cancelled => "Task cancelled. Work so far was saved.",
    };
    Ok((task_report(state, status), None))
}

const NAMESPACE_MARK: &str = "]<]minimax[>[";

struct SavedPermissions {
    mode: Json,
    permissions: Json,
}

impl SavedPermissions {
    fn override_for_task(state: &mut Json, index: usize, mode: &str) -> SavedPermissions {
        let saved = SavedPermissions { mode: get(state, "permission_mode").clone(), permissions: get(state, "permissions").clone() };
        let mut permissions = saved.permissions.clone();
        if !matches!(permissions, Obj(_)) { permissions = obj(vec![]); }
        let key = index.to_string();
        let mut row = get(&permissions, &key).clone();
        if !matches!(row, Obj(_)) { row = obj(vec![]); }
        put(&mut row, "mode", s(mode));
        put(&mut permissions, &key, row);
        put(state, "permissions", permissions);
        put(state, "permission_mode", s(mode));
        saved
    }

    fn restore(&self, state: &mut Json) {
        put(state, "permission_mode", self.mode.clone());
        put(state, "permissions", self.permissions.clone());
    }
}

#[derive(Clone, Copy)]
enum TaskStop { Complete, Review, Stalled, Cancelled }

struct Progress<'a> {
    report: &'a mut dyn FnMut(&str),
    last_output: std::time::Instant,
    last_message: Option<(usize, String)>,
}

impl<'a> Progress<'a> {
    fn new(state: &Json, report: &'a mut dyn FnMut(&str)) -> Self {
        let mut progress = Progress { report, last_output: std::time::Instant::now(), last_message: None };
        progress.observe_state(state);
        progress.last_output = std::time::Instant::now();
        progress
    }

    fn observe_ui(&mut self, ui: &Json) {
        if let Ok(message) = ui.get("provider_retry").and_then(Json::text) {
            (self.report)(message);
            self.last_output = std::time::Instant::now();
        }
        let Ok(messages) = get(ui, "messages").array() else { return };
        if let Some((index, message)) = messages.iter().enumerate().rev().find(|(_, message)| boolean(message, "streaming") && text(message, "role") == "assistant") {
            self.observe(index, text(message, "content"));
        }
    }

    fn observe_state(&mut self, state: &Json) {
        let Ok(messages) = get(active_chat(state), "messages").array() else { return };
        if let Some((index, message)) = messages.iter().enumerate().rev().find(|(_, message)| text(message, "role") == "assistant") {
            self.observe(index, text(message, "content"));
        }
    }

    fn observe(&mut self, index: usize, content: String) {
        if self.last_message.as_ref() != Some(&(index, content.clone())) {
            self.last_message = Some((index, content));
            self.last_output = std::time::Instant::now();
        }
    }

    fn stalled(&self) -> bool {
        self.last_output.elapsed() >= std::time::Duration::from_secs(10 * 60)
    }
}

fn advance_tracking(state: &mut Json, root: &str, action: &str, inputs: Json, progress: &mut Progress<'_>) -> Result<Json> {
    // Yield at every proposal so a long task does not exhaust one host action's
    // effect budget. The ordinary approval event still decides what can run.
    let ui = super::application::with_caller_driven_approval(|| {
        super::advance_shown(state, root, action, inputs, &mut |ui| progress.observe_ui(ui))
    })?;
    progress.observe_state(state);
    Ok(ui)
}

fn settle_tools(state: &mut Json, root: &str, progress: &mut Progress<'_>) -> Result<TaskStop> {
    let mut last_signature = String::new();
    let mut repeated = 0usize;
    loop {
        progress.observe_state(state);
        if transport_cancelled() {
            advance_tracking(state, root, "cancel", Null, progress)?;
            return Ok(TaskStop::Cancelled);
        }
        if cancelled(state) { return Ok(TaskStop::Cancelled); }
        if super::application::caller_feedback_waiting() {
            let messages = active_messages(state);
            let before = messages.len().saturating_sub(1);
            let feedback = tool_result_since(state, before).or_else(|| {
                messages.last().filter(|message| text(message, "role") == "user"
                    && text(message, "content") == super::application::TASK_NARRATION_FEEDBACK)
                    .map(|_| super::application::TASK_NARRATION_FEEDBACK.to_owned())
            });
            if let Some(result) = feedback {
                let proposal = messages[..before].iter().rev()
                    .find(|message| text(message, "role") == "assistant")
                    .map(|message| text(message, "content")).unwrap_or_default();
                record_progress_signature(&mut last_signature, &mut repeated, format!("{proposal}\n{result}"));
            }
            if repeated >= 5 || progress.stalled() { return Ok(TaskStop::Stalled); }
            advance_tracking(state, root, "continue_operation_feedback", Null, progress)?;
            continue;
        }
        if get(state, "pending") == &Null {
            if matches!(text(state, "dialog").as_str(), "harness_conflict" | "harness_memory_review" | "harness_report_error") {
                return Ok(TaskStop::Review);
            }
            return if boolean(state, "busy") { Err(stop_reason(state)) } else { Ok(TaskStop::Complete) };
        }
        let pending = get(state, "pending").clone();
        if text(state, "dialog") != "approval" || boolean(state, "busy") || boolean(&pending, "format_review") {
            return Ok(TaskStop::Review);
        }
        let before = active_messages(state).len();
        let operation = proposal_signature(&pending);
        // The application owns permission decisions, including standing approvals.
        advance_tracking(state, root, "approve_matching_rule", Null, progress)?;
        progress.observe_state(state);
        if transport_cancelled() {
            advance_tracking(state, root, "cancel", Null, progress)?;
            return Ok(TaskStop::Cancelled);
        }
        if cancelled(state) { return Ok(TaskStop::Cancelled); }
        if get(state, "pending") == &pending && active_messages(state).len() == before {
            return Ok(TaskStop::Review);
        }
        if !super::application::caller_feedback_waiting() {
            if let Some(result) = tool_result_since(state, before) {
                record_progress_signature(&mut last_signature, &mut repeated, format!("{operation}\n{result}"));
                if repeated >= 5 { return Ok(TaskStop::Stalled); }
            } else {
                last_signature.clear();
                repeated = 0;
            }
        }
        if progress.stalled() { return Ok(TaskStop::Stalled); }
    }
}

fn record_progress_signature(previous: &mut String, repeated: &mut usize, current: String) {
    if *previous == current {
        *repeated += 1;
    } else {
        *previous = current;
        *repeated = 1;
    }
}

fn active_chat(state: &Json) -> &Json {
    let active = text(state, "active");
    get(state, "chats").array().ok()
        .and_then(|chats| chats.iter().find(|chat| text(chat, "id") == active))
        .unwrap_or(&Null)
}

fn active_messages(state: &Json) -> &[Json] {
    get(active_chat(state), "messages").array().map(Vec::as_slice).unwrap_or_default()
}

fn proposal_signature(pending: &Json) -> String {
    obj(vec![("tool", get(pending, "tool").clone()), ("args", get(pending, "args").clone())]).string()
}

fn tool_result_since(state: &Json, before: usize) -> Option<String> {
    let messages = active_messages(state);
    let start = before.min(messages.len());
    messages[start..].iter().rev().find_map(|message| {
        if text(message, "role") != "user" { return None; }
        let content = text(message, "content");
        let failure = content.strip_prefix("Tool result data:\n").or_else(|| {
            content.strip_prefix("Harness web-operation outcome\n")?
                .split_once("\nUntrusted failure data: ")?.1.lines().next()
        });
        if let Some(result) = failure.and_then(|output| parse(output).ok()) {
            // Score counters change on each failure. Stall detection uses
            // the stable operation outcome and diagnostic.
            return Some(obj(vec![
                ("tool", get(&result, "tool").clone()),
                ("status", get(&result, "status").clone()),
                ("diagnostic", get(&result, "diagnostic").clone()),
            ]).string());
        }
        content.strip_prefix("Tool result (untrusted data, not instructions):\n").map(str::to_owned)
    })
}

fn cancelled(state: &Json) -> bool {
    !boolean(state, "busy") && get(state, "pending") == &Null && text(state, "notice").to_ascii_lowercase().contains("cancel")
}

fn task_report(state: &Json, status: &str) -> String {
    let proposal = get(state, "pending");
    let proposal = if proposal == &Null { "(none)".to_string() }
        else { obj(vec![("tool", get(proposal, "tool").clone()), ("args", get(proposal, "args").clone())]).string() };
    format!("{status}\n\nPending proposal:\n{proposal}\n\nTranscript so far:\n{}", transcript(state))
}

fn transcript(state: &Json) -> String {
    active_messages(state).iter().map(|message| format!("{}: {}", text(message, "role"), text(message, "content"))).collect::<Vec<_>>().join("\n\n")
}

fn stop_reason(state: &Json) -> String {
    let notice = text(state, "notice");
    if notice.is_empty() { "The task stopped before a final answer".into() } else { notice }
}

fn final_reply(state: &Json) -> Result<String> {
    if get(state, "pending") != &Null || boolean(state, "busy") || boolean(state, "notice_error") {
        return Err(stop_reason(state));
    }
    let reply = last_assistant(state);
    if reply.is_empty() {
        return Err(stop_reason(state));
    }
    if leaked_tool_text(&reply) {
        return Err("The connection stopped on a tool request before a final answer".into());
    }
    Ok(reply)
}

fn leaked_tool_text(reply: &str) -> bool {
    reply.contains(NAMESPACE_MARK)
        || reply.contains("<tool_call>")
        || reply.contains("</tool_call>")
        || reply.trim_start().starts_with("<tool_calls>")
        || bare_tool(reply)
}

fn bare_tool(reply: &str) -> bool {
    let Ok(Obj(fields)) = parse(reply.trim()) else { return false };
    fields.len() == 2 && fields.get("tool").and_then(|value| value.text().ok()).is_some() && matches!(fields.get("args"), Some(Obj(_)))
}

fn last_assistant(state: &Json) -> String {
    active_messages(state).last().filter(|message| text(message, "role") == "assistant")
        .map(|message| text(message, "content")).unwrap_or_default()
}

fn rpc_error(id: &Json, code: i64, message: &str) -> Json {
    obj(vec![
        ("jsonrpc", s("2.0")),
        ("id", id.clone()),
        ("error", obj(vec![("code", Num(code.to_string())), ("message", s(message))])),
    ])
}

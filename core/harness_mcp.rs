//! On-demand MCP for the saved harness. Cursor starts this process, asks it to
//! list connections, or sends one task to a named connection. It does not serve
//! the local file-inspection tools.
use super::*;
use std::io::{BufRead, Write};

pub(super) fn serve(state: &mut Json, vault: &Vault, root: &str) -> Result<()> {
    let mut reader = std::io::BufReader::new(std::io::stdin());
    let mut writer = std::io::BufWriter::new(std::io::stdout());
    let mut initialized = false;
    let mut ready = false;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(|error| format!("MCP input: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = match parse(trimmed) {
            Ok(request) => handle(state, vault, root, &request, &mut initialized, &mut ready),
            Err(error) => Some(rpc_error(&Null, -32700, &format!("Parse error: {error}"))),
        };
        if let Some(response) = response {
            writer.write_all(response.string().as_bytes()).map_err(|error| error.to_string())?;
            writer.write_all(b"\n").map_err(|error| error.to_string())?;
            writer.flush().map_err(|error| error.to_string())?;
        }
    }
}

fn handle(state: &mut Json, vault: &Vault, root: &str, request: &Json, initialized: &mut bool, ready: &mut bool) -> Option<Json> {
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
            ("instructions", s("List saved connections or send one task to a connection by its exact name. This does not inspect binaries and does not reveal credentials.")),
        ]))
    } else if !*ready {
        Err("Initialize the session and send notifications/initialized first".into())
    } else {
        match method.as_str() {
            "ping" => Ok(obj(vec![])),
            "tools/list" => Ok(obj(vec![("tools", Arr(tool_definitions()))])),
            "tools/call" => call(state, vault, root, get(request, "params")),
            _ => Err(format!("Method {method} is not supported")),
        }
    };
    Some(match result {
        Ok(value) => obj(vec![("jsonrpc", s("2.0")), ("id", id.clone()), ("result", value)]),
        Err(error) => rpc_error(id, -32601, &error),
    })
}

fn tool_definitions() -> Vec<Json> {
    vec![
        tool("connections", "List saved connection names and model ids. Credentials and endpoints are not included.", &[]),
        tool("agent_task", "Send one task to the saved connection whose name matches exactly. The reply is that connection's assistant text.", &[("name", "Exact connection name, such as Queen or MiniMax"), ("task", "The task text")]),
    ]
}

fn tool(name: &str, description: &str, fields: &[(&str, &str)]) -> Json {
    let mut properties = Vec::new();
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

fn call(state: &mut Json, vault: &Vault, root: &str, params: &Json) -> Result<Json> {
    let name = text(params, "name");
    let args = get(params, "arguments");
    let text = match name.as_str() {
        "connections" => connections(state),
        "agent_task" => agent_task(state, vault, root, args)?,
        _ => return Err(format!("Unknown tool {name}")),
    };
    Ok(obj(vec![
        ("content", Arr(vec![obj(vec![("type", s("text")), ("text", s(&text))])])),
        ("isError", Bool(false)),
    ]))
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

fn agent_task(state: &mut Json, vault: &Vault, root: &str, args: &Json) -> Result<String> {
    let name = text(args, "name");
    let task = text(args, "task");
    if name.is_empty() || task.is_empty() {
        return Err("agent_task requires name and task".into());
    }
    let Arr(profiles) = get(state, "profiles").clone() else {
        return Err("No saved connections".into());
    };
    let Some(index) = profiles.iter().position(|profile| text(profile, "name") == name) else {
        return Err(format!("No saved connection is named {name}"));
    };
    advance(state, root, &format!("chat_model:{index}"), Null)?;
    advance(state, root, "send", obj(vec![("draft", s(&task))]))?;
    vault.save(state)?;
    let reply = last_assistant(state);
    if reply.is_empty() {
        let notice = text(state, "notice");
        if notice.is_empty() { Err("The connection returned no assistant text".into()) } else { Err(notice) }
    } else {
        Ok(reply)
    }
}

fn last_assistant(state: &Json) -> String {
    let active = text(state, "active");
    let Arr(chats) = get(state, "chats") else { return String::new(); };
    for chat in chats {
        if !active.is_empty() && text(chat, "id") != active {
            continue;
        }
        let Arr(messages) = get(chat, "messages") else { continue; };
        for message in messages.iter().rev() {
            if text(message, "role") == "assistant" {
                return text(message, "content");
            }
        }
    }
    String::new()
}

fn rpc_error(id: &Json, code: i64, message: &str) -> Json {
    obj(vec![
        ("jsonrpc", s("2.0")),
        ("id", id.clone()),
        ("error", obj(vec![("code", Num(code.to_string())), ("message", s(message))])),
    ])
}

//! Assembled field/extraction values through inspection, query, UI and exact export.
mod twin;

use std::collections::HashMap;

use twin::{b, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner current shared-core checks with original LLVM-assembled AArch64 field/extraction instructions, independent disassembly, retained observations, native UI descriptions, controlled provider query, exact export and genuine immediately-published core capture compatibility.";

const ASSEMBLY: &str = r#".text
.global entry
entry:
unsigned_field: ubfm x1,x0,#8,#15
unsigned_zero: and x2,x1,#0xff00
 mov x5,#0x5500
merged_field: bfm x5,x0,#0,#7
merged_constant: lsr x6,x5,#8
 mov x7,#0xaa
inserted_field: bfm x8,x7,#0,#7
inserted_constant: and x9,x8,#0xff
 and x10,x0,#0x7f
positive_field: sbfm x11,x10,#0,#7
positive_zero: lsr x12,x11,#7
 orr x13,x0,#0x80
negative_field: sbfm x14,x13,#0,#7
negative_constant: lsr x15,x14,#7
signed_wrap: sbfm x16,x0,#60,#3
signed_wrap_zero: and x17,x16,#0xf
unsigned_wrap: ubfm x18,x0,#60,#3
unsigned_wrap_zero: and x19,x18,#0xffffffffffffff0f
 mov x20,#0
extract_low: extr x21,x0,x20,#8
extract_low_zero: and x22,x21,#0x00ffffffffffffff
extract_high: extr x23,x20,x0,#8
extract_high_zero: lsr x24,x23,#56
 and x25,x0,#0xff
extract_rotation: extr x26,x25,x25,#8
rotation_zero: and x27,x26,#0x00ffffffffffffff
 ret
word_entry:
word_field: ubfm w1,w0,#4,#7
word_zero: and x2,x1,#0xfffffffffffffff0
 mov w3,#0x5500
word_merge: bfm w3,w0,#0,#7
word_constant: lsr x4,x3,#8
word_extract: extr w5,w0,wzr,#8
word_extract_zero: and x6,x5,#0x00ffffff
 ret
coordinate_entry:
 adr x10,entry
coordinate_unsigned: ubfm x11,x10,#0,#63
coordinate_signed: sbfm x12,x10,#0,#63
coordinate_merge: bfm x13,x10,#0,#63
coordinate_alias: bfm x10,x10,#0,#7
coordinate_extract: extr x14,xzr,x10,#0
 ret
"#;

const FIELD_NEEDLE: &str = "X5 <- partial integer bits0x5500, known mask0xffffffffffffff00";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project = project.to_str().unwrap_or_else(|| panic!("project path is not UTF-8")).to_string();
    let scratch = twin::create_scratch(std::path::Path::new(&project), "field-flow-");
    let root = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let (nm, disassembly) = assemble(&root);
    let mut controller = Controller::spawn(&project);
    let passed = run(&mut controller, &project, &root, &nm, &disassembly);
    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);
    if passed.len() != 37 {
        panic!("field_flow emitted {} checks", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value_printed("field_flow", &body, true);
}

fn run(controller: &mut Controller, project: &str, root: &str, nm: &str, disassembly: &str) -> Vec<String> {
    let symbols = symbols(nm);
    let (inspected, graph) = inspect(controller, root);
    let nodes = encoding_nodes(&graph);
    let mut passed = Vec::new();

    keep(
        &mut passed,
        "independent assembler emits field and extraction instructions",
        disassembly.contains("ubfx") && disassembly.contains("bfxil") && disassembly.contains("extr"),
    );

    let field_cases = [
        ("unsigned_field", "X1", 0x0u64, 0xffffffffffffff00u64),
        ("merged_field", "X5", 0x5500, 0xffffffffffffff00),
        ("inserted_field", "X8", 0xaa, 0xff),
        ("positive_field", "X11", 0, 0xffffffffffffff80),
        ("negative_field", "X14", 0xffffffffffffff80, 0xffffffffffffff80),
        ("signed_wrap", "X16", 0, 0xf),
        ("unsigned_wrap", "X18", 0, 0xffffffffffffff0f),
        ("extract_low", "X21", 0, 0x00ffffffffffffff),
        ("extract_high", "X23", 0, 0xff00000000000000),
        ("extract_rotation", "X26", 0, 0x00ffffffffffffff),
        ("word_field", "W1", 0, 0xfffffffffffffff0),
        ("word_merge", "W3", 0x5500, 0xffffffffffffff00),
        ("word_extract", "W5", 0, 0xffffffff00ffffff),
    ];
    for (name, register, bits, known) in field_cases {
        let check = format!("{name} retains the selected known bits");
        let needle = format!("{register} <- partial integer bits0x{bits:x}, known mask0x{known:x}");
        keep(&mut passed, &check, node_contains(&nodes, &symbols, name, &needle));
    }

    let constant_cases = [
        ("unsigned_zero", "X2", 0x0u64),
        ("merged_constant", "X6", 0x55),
        ("inserted_constant", "X9", 0xaa),
        ("positive_zero", "X12", 0),
        ("negative_constant", "X15", 0x1ffffffffffffff),
        ("signed_wrap_zero", "X17", 0),
        ("unsigned_wrap_zero", "X19", 0),
        ("extract_low_zero", "X22", 0),
        ("extract_high_zero", "X24", 0),
        ("rotation_zero", "X27", 0),
        ("word_zero", "X2", 0),
        ("word_constant", "X4", 0x55),
        ("word_extract_zero", "X6", 0),
    ];
    for (name, register, value) in constant_cases {
        let check = format!("{name} derives its constant from the preceding field");
        let needle = format!("{register} <- encoded integer 0x{value:x}");
        keep(&mut passed, &check, node_contains(&nodes, &symbols, name, &needle));
    }

    for name in ["coordinate_unsigned", "coordinate_signed", "coordinate_merge", "coordinate_alias", "coordinate_extract"] {
        let check = format!("{name} preserves source-coordinate identity");
        keep(&mut passed, &check, node_contains(&nodes, &symbols, name, "encoded source coordinate declared VA "));
    }

    let record = last_record(state_of(&inspected)).clone();
    let record_id = record.get("id").cloned().unwrap_or_else(|| panic!("native view retains actual partial field value"));
    let merged = node_named(&nodes, &symbols, "merged_field").cloned().unwrap_or_else(|| panic!("native view retains actual partial field value"));
    let merged_id = merged.get("id").cloned().unwrap_or_else(|| panic!("native view retains actual partial field value"));
    let merged_content = match merged.get("content") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("native view retains actual partial field value"),
    };

    let mut view = event(controller, Some(state_of(&inspected)), &format!("investigation_record:{}", py_str(&record_id)), obj(vec![]), root);
    view = event(controller, Some(state_of(&view)), "investigation_graph", obj(vec![]), root);
    view = event(controller, Some(state_of(&view)), &format!("investigation_graph_node:{}", py_str(&merged_id)), obj(vec![]), root);
    let ui = view.get("ui").cloned().unwrap_or(Value::Null);
    keep(&mut passed, "native view retains actual partial field value", joined_strings(&ui).contains(FIELD_NEEDLE));

    let mut state = state_of(&view).clone();
    set(
        &mut state,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("model".into(), s("controlled")),
            ("endpoint".into(), s("http://controlled.test/model")),
            ("plain".into(), b(true)),
            ("stream".into(), b(false)),
            ("protocol".into(), s("chat")),
        ])]),
    );
    let sent = event(controller, Some(&state), "send", obj(vec![("draft".into(), s("Read the retained field value."))]), root);
    let call = obj(vec![
        ("tool".into(), s("observations")),
        (
            "args".into(),
            obj(vec![("id".into(), record_id.clone()), ("snapshot".into(), n("0")), ("node".into(), merged_id.clone())]),
        ),
    ])
    .to_string();
    let proposed = model_reply(controller, &sent, root, &call);
    let approved = event(controller, Some(state_of(&proposed)), "approve", obj(vec![]), root);
    keep(
        &mut passed,
        "ordinary model query receives the same partial field value",
        effect_wire(&approved).is_some_and(|wire| wire.contains(FIELD_NEEDLE)),
    );
    let answered = model_reply(controller, &approved, root, "Field value received.");

    let mut export = event(controller, Some(state_of(&answered)), &format!("investigation_record:{}", py_str(&record_id)), obj(vec![]), root);
    export = event(controller, Some(state_of(&export)), "investigation_graph", obj(vec![]), root);
    export = event(controller, Some(state_of(&export)), &format!("investigation_graph_node:{}", py_str(&merged_id)), obj(vec![]), root);
    export = event(controller, Some(state_of(&export)), "reconstruction_new", obj(vec![]), root);
    export = event(
        controller,
        Some(state_of(&export)),
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), record_id.clone()),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&py_str(&merged_id))),
            ("mode".into(), s("representation")),
            ("output".into(), s("field-result.txt")),
        ]),
        root,
    );
    export = event(controller, Some(state_of(&export)), "approve", obj(vec![]), root);
    let effect = match export.get("effect") {
        Some(value) if !matches!(value, Value::Null) => value.clone(),
        _ => panic!("reviewed export retains the exact field representation"),
    };
    let output = tool(controller, &effect, root);
    // The leg calls (root/'field-result.txt').read_bytes(), which raises when the
    // export wrote nothing; a fabricated empty read would unfail the check.
    let written = match std::fs::read(std::path::Path::new(root).join("field-result.txt")) {
        Ok(bytes) => bytes,
        Err(error) => panic!("reviewed export retains the exact field representation: could not read field-result.txt: {error}"),
    };
    keep(
        &mut passed,
        "reviewed export retains the exact field representation",
        written == merged_content.as_bytes() && value_contains(&output, "read back exactly"),
    );

    let published = std::path::Path::new(project).join("output/published-controller");
    let mut previous = Controller::spawn_at(&published, project);
    let (prior, prior_graph) = inspect(&mut previous, root);
    previous.finish();
    let prior_nodes = encoding_nodes(&prior_graph);
    let merged_address = symbols.get("merged_field").copied().unwrap_or_else(|| panic!("actual published capture retains its encoded field facts"));
    let prior_node = prior_nodes
        .iter()
        .find(|node| address_of(node) == Some(merged_address))
        .cloned()
        .unwrap_or_else(|| panic!("actual published capture retains its encoded field facts"));
    keep(&mut passed, "actual published capture retains its encoded field facts", content_contains(&prior_node, FIELD_NEEDLE));

    let resumed = event(controller, None, "resume", obj(vec![]), root);
    let mut current = state_of(&resumed).clone();
    let records = state_of(&prior)
        .get("investigation_records")
        .cloned()
        .unwrap_or_else(|| panic!("current view preserves the actual prior field capture"));
    set(&mut current, "investigation_records", records);
    let prior_record = last_record(&current).clone();
    let prior_record_id = prior_record.get("id").cloned().unwrap_or_else(|| panic!("current view preserves the actual prior field capture"));
    let prior_id = prior_node.get("id").cloned().unwrap_or_else(|| panic!("current view preserves the actual prior field capture"));
    let prior_content = match prior_node.get("content") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("current view preserves the actual prior field capture"),
    };
    let mut prior_view = event(controller, Some(&current), &format!("investigation_record:{}", py_str(&prior_record_id)), obj(vec![]), root);
    prior_view = event(controller, Some(state_of(&prior_view)), "investigation_graph", obj(vec![]), root);
    prior_view = event(
        controller,
        Some(state_of(&prior_view)),
        &format!("investigation_graph_node:{}", py_str(&prior_id)),
        obj(vec![]),
        root,
    );
    let prior_ui = prior_view.get("ui").cloned().unwrap_or(Value::Null);
    keep(
        &mut passed,
        "current view preserves the actual prior field capture",
        joined_strings(&prior_ui).contains(prior_content.trim()),
    );
    passed
}

fn assemble(root: &str) -> (String, String) {
    let ndk = std::env::var("CLIENT_NDK").unwrap_or_else(|error| panic!("CLIENT_NDK: {error}"));
    let bin = std::path::Path::new(&ndk).join("toolchains/llvm/prebuilt/linux-x86_64/bin");
    let asm = std::path::Path::new(root).join("fields.S");
    let elf = std::path::Path::new(root).join("fields.elf");
    std::fs::write(&asm, ASSEMBLY).unwrap_or_else(|error| panic!("{}: {error}", asm.display()));
    let compiled = std::process::Command::new(bin.join("clang"))
        .args(["--target=aarch64-linux-android26", "-nostdlib", "-static", "-fuse-ld=lld", "-Wl,-e,entry", "-Wl,--emit-relocs"])
        .arg(&asm)
        .arg("-o")
        .arg(&elf)
        .env("TMPDIR", root)
        .output()
        .unwrap_or_else(|error| panic!("clang: {error}"));
    if !compiled.status.success() {
        panic!("clang failed: {}", String::from_utf8_lossy(&compiled.stderr));
    }
    let nm = command(&bin.join("llvm-nm"), "-n", &elf);
    let disassembly = command(&bin.join("llvm-objdump"), "-d", &elf);
    (nm, disassembly)
}

fn command(program: &std::path::Path, flag: &str, file: &std::path::Path) -> String {
    let output = std::process::Command::new(program).arg(flag).arg(file).output().unwrap_or_else(|error| panic!("{}: {error}", program.display()));
    if !output.status.success() {
        panic!("{} failed: {}", program.display(), String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).unwrap_or_else(|error| panic!("{} output: {error}", program.display()))
}

fn symbols(nm: &str) -> HashMap<String, u64> {
    let mut symbols = HashMap::new();
    for line in nm.lines() {
        let Some((address, name)) = parse_nm(line.trim_end_matches('\r')) else { continue };
        symbols.insert(name, address);
    }
    symbols
}

// llvm-nm -n lines match the Python leg's `([0-9a-fA-F]+)\s+(\w)\s+(\S+)$`.
fn parse_nm(line: &str) -> Option<(u64, String)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
        index += 1;
    }
    if index == 0 {
        return None;
    }
    let address = u64::from_str_radix(&line[..index], 16).ok()?;
    if index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
        return None;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let type_byte = *bytes.get(index)?;
    if !type_byte.is_ascii_alphanumeric() && type_byte != b'_' {
        return None;
    }
    index += 1;
    if index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
        return None;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if index >= bytes.len() || line[index..].bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    Some((address, line[index..].to_string()))
}

fn inspect(controller: &mut Controller, root: &str) -> (Value, Value) {
    let mut step = event(controller, None, "resume", obj(vec![]), root);
    step = event(controller, Some(state_of(&step)), "investigation", obj(vec![]), root);
    step = event(controller, Some(state_of(&step)), "inspect_file", obj(vec![("path".into(), s("fields.elf"))]), root);
    step = event(controller, Some(state_of(&step)), "approve", obj(vec![]), root);
    let effect = match step.get("effect") {
        Some(value) if !matches!(value, Value::Null) => value.clone(),
        _ => panic!("inspection approval has no effect"),
    };
    let output = tool(controller, &effect, root);
    let text = match &output {
        Value::Str(text) => text.clone(),
        other => panic!("inspection tool output is not text: {}", other.to_string()),
    };
    let parsed = Value::parse(&text);
    let graph = match parsed.get("snapshots") {
        Some(Value::Arr(items)) => items.first().cloned().unwrap_or_else(|| panic!("inspection snapshot is empty")),
        _ => panic!("inspection tool output has no snapshots"),
    };
    let settled = event(controller, Some(state_of(&step)), "tool_result", tool_inputs(&effect, &output), root);
    (settled, graph)
}

fn tool(controller: &mut Controller, effect: &Value, root: &str) -> Value {
    let mut envelope = effect.clone();
    set(&mut envelope, "op", s("tool"));
    set(&mut envelope, "root", s(root));
    controller.op(&envelope)
}

fn tool_inputs(effect: &Value, output: &Value) -> Value {
    let mut fields = Vec::new();
    for key in ["tool_ticket", "tool_binding", "editor", "editor_dialog"] {
        if let Some(value) = effect.get(key) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    fields.push(("purpose".into(), s("local_tool")));
    fields.push(("output".into(), output.clone()));
    obj(fields)
}

fn model_reply(controller: &mut Controller, step: &Value, root: &str, content: &str) -> Value {
    let body = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(content))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )])
    .to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let ticket = state_of(step).get("model_expected").cloned().unwrap_or_else(|| panic!("model reply has no model_expected"));
    event(
        controller,
        Some(state_of(step)),
        "network_result",
        obj(vec![("purpose".into(), s("model")), ("model_ticket".into(), ticket), ("data".into(), s(&twin::b64_encode(wire.as_bytes())))]),
        root,
    )
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state, action, inputs, root)
}

fn state_of(step: &Value) -> &Value {
    step.get("state").unwrap_or_else(|| panic!("event returned no state"))
}

fn encoding_nodes(graph: &Value) -> Vec<Value> {
    match graph.get("nodes") {
        Some(Value::Arr(items)) if !items.is_empty() => items[1..].to_vec(),
        _ => panic!("observation graph has no encoding nodes"),
    }
}

fn address_of(node: &Value) -> Option<u64> {
    match node.get("encoding").and_then(|value| value.get("address")) {
        Some(Value::Num(token)) => token.parse().ok(),
        _ => None,
    }
}

fn node_named<'a>(nodes: &'a [Value], symbols: &HashMap<String, u64>, name: &str) -> Option<&'a Value> {
    let address = symbols.get(name).copied()?;
    nodes.iter().rev().find(|node| address_of(node) == Some(address))
}

fn node_contains(nodes: &[Value], symbols: &HashMap<String, u64>, name: &str, needle: &str) -> bool {
    node_named(nodes, symbols, name).is_some_and(|node| content_contains(node, needle))
}

fn content_contains(node: &Value, needle: &str) -> bool {
    match node.get("content") {
        Some(Value::Str(text)) => text.contains(needle),
        _ => false,
    }
}

fn last_record(state: &Value) -> &Value {
    state
        .get("investigation_records")
        .and_then(|value| value.get("items"))
        .and_then(|value| match value {
            Value::Arr(items) => items.last(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no investigation record"))
}

fn effect_wire(step: &Value) -> Option<&str> {
    match step.get("effect") {
        Some(Value::Obj(_)) => match step.get("effect").unwrap().get("wire") {
            Some(Value::Str(text)) => Some(text),
            _ => None,
        },
        _ => None,
    }
}

fn value_contains(value: &Value, needle: &str) -> bool {
    match value {
        Value::Str(text) => text.contains(needle),
        Value::Obj(fields) => fields.iter().any(|(key, _)| key == needle),
        _ => false,
    }
}

fn joined_strings(value: &Value) -> String {
    let mut parts = Vec::new();
    collect_strings(value, &mut parts);
    parts.join("\n")
}

fn collect_strings(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::Str(text) => parts.push(text.clone()),
        Value::Obj(fields) => {
            for (_, child) in fields {
                collect_strings(child, parts);
            }
        }
        Value::Arr(items) => {
            for child in items {
                collect_strings(child, parts);
            }
        }
        _ => {}
    }
}

fn py_str(value: &Value) -> String {
    match value {
        Value::Str(text) => text.clone(),
        Value::Num(token) => token.clone(),
        Value::Bool(flag) => if *flag { "True".into() } else { "False".into() },
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn set(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

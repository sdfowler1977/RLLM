mod twin;

use std::collections::HashMap;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner shared-core checks using original LLVM-assembled AArch64 carry instructions, actual retained observations, UI descriptions, controlled provider query and exact file export.";

const ASSEMBLY: &str = r#".text
.global entry,adc64,sbc64,adcs64,sbcs64,selected,adc32,sbc32,adcs32,sbcs32,unknown_adc,masked,coordinate_add,coordinate_sub,identity_sub,identity_flags,identity_sbc,identity_bic,identity_orn,identity_eon,identity_selected,identity_bics
entry:
 mov x0,#-1
 adds x1,x0,#1
 mov x2,#41
adc64: adc x3,x2,xzr
sbc64: sbc x4,x3,x2
adcs64: adcs x5,x0,xzr
sbcs64: sbcs x6,x3,x2
selected: csel x7,x3,x2,hi
 mov w8,#42
 subs w9,w8,#43
adc32: adc w10,w8,wzr
sbc32: sbc w11,w8,wzr
adcs32: adcs w12,w9,wzr
sbcs32: sbcs w13,w8,wzr
 ret
unknown_entry:
 mov x14,#40
unknown_adc: adc x15,x14,xzr
masked: and x16,x15,#0xfffffffffffffffe
 ret
coordinate_entry:
 adr x17,entry
 subs xzr,xzr,xzr
coordinate_add: adc x18,x17,xzr
coordinate_sub: sbc x19,x17,xzr
 ret
identity_entry:
identity_sub: sub x20,x21,x21
identity_flags: subs x22,x21,x21
identity_sbc: sbc x23,x21,x21
identity_bic: bic x24,x21,x21
identity_orn: orn x25,x21,x21
identity_eon: eon x26,x21,x21
identity_selected: csel x27,x25,x24,eq
identity_bics: bics x28,x21,x21
 ret
"#;

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch(twin::create_scratch(&root, "carry-flow-"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch_text = scratch.0.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();
    let names: Vec<Value> = passed.iter().map(|name| s(name)).collect();
    twin::report_value(
        "carry_flow",
        &twin::obj(vec![
            ("passed".into(), Value::Arr(names)),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} carry_flow checks passed", passed.len());
}

fn run(controller: &mut Controller, scratch: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let asm = format!("{scratch}/carry.S");
    let elf = format!("{scratch}/carry.elf");
    std::fs::write(&asm, ASSEMBLY).unwrap_or_else(|error| panic!("could not write {asm}: {error}"));

    let ndk = std::env::var("CLIENT_NDK").unwrap_or_else(|error| panic!("CLIENT_NDK: {error}"));
    let bin = format!("{ndk}/toolchains/llvm/prebuilt/linux-x86_64/bin");
    command(
        &format!("{bin}/clang"),
        &[
            "--target=aarch64-linux-android26",
            "-nostdlib",
            "-static",
            "-fuse-ld=lld",
            "-Wl,-e,entry",
            "-Wl,--emit-relocs",
            &asm,
            "-o",
            &elf,
        ],
        scratch,
    );
    let symbols = parse_symbols(&command(&format!("{bin}/llvm-nm"), &["-n", &elf], scratch));
    let disassembly = command(&format!("{bin}/llvm-objdump"), &["-d", &elf], scratch);

    let mut event = controller.event(None, "resume", obj(vec![]), scratch);
    event = drive(controller, &event, "investigation", obj(vec![]), scratch);
    event = drive(
        controller,
        &event,
        "inspect_file",
        obj(vec![("path".into(), s("carry.elf"))]),
        scratch,
    );
    event = drive(controller, &event, "approve", obj(vec![]), scratch);
    let effect = event.get("effect").cloned().unwrap_or_else(|| panic!("approve returned no effect"));
    if matches!(effect, Value::Null) {
        panic!("approve returned no effect: {}", notice(&event));
    }
    let output = tool(controller, &effect, scratch);
    let graph = first_snapshot(&output);
    let nodes = index_nodes(&graph);

    let cases = [
        ("adc64", "ADC", "X3", "0x2a"),
        ("sbc64", "SBC", "X4", "0x1"),
        ("adcs64", "ADCS", "X5", "0x0"),
        ("sbcs64", "SBCS", "X6", "0x1"),
        ("adc32", "ADC", "W10", "0x2a"),
        ("sbc32", "SBC", "W11", "0x29"),
        ("adcs32", "ADCS", "W12", "0xffffffff"),
        ("sbcs32", "SBCS", "W13", "0x29"),
    ];
    for (name, mnemonic, reg, value) in cases {
        let check = format!("{name} matches independent encoding and carry result");
        let node = take_node(&nodes, &symbols, name, &check);
        let found = node.get("encoding").and_then(|encoding| encoding.get("mnemonic")).map(Value::str_);
        let text = content(&node);
        let dump_ok = disassembly.contains(&mnemonic.to_ascii_lowercase());
        if found != Some(mnemonic) || !dump_ok || !text.contains(&format!("{reg} <- encoded integer {value}")) {
            panic!("{check}: mnemonic={found:?} dump_ok={dump_ok} content={text}");
        }
        passed.push(check);
    }

    let selected = take_node(&nodes, &symbols, "selected", "carry-updated flags drive the following conditional select");
    keep(
        &mut passed,
        "carry-updated flags drive the following conditional select",
        content(&selected).contains("X7 <- encoded integer 0x2a"),
        &content(&selected),
    );
    let unknown = take_node(&nodes, &symbols, "unknown_adc", "unknown carry retains common result bits");
    keep(
        &mut passed,
        "unknown carry retains common result bits",
        content(&unknown).contains("X15 <- partial integer bits0x28, known mask0xfffffffffffffffe"),
        &content(&unknown),
    );
    let masked = take_node(&nodes, &symbols, "masked", "following mask derives a constant from unknown carry");
    keep(
        &mut passed,
        "following mask derives a constant from unknown carry",
        content(&masked).contains("X16 <- encoded integer 0x28"),
        &content(&masked),
    );
    let coordinate_ok = ["coordinate_add", "coordinate_sub"].iter().all(|name| {
        let node = take_node(&nodes, &symbols, name, "carry arithmetic preserves coordinate provenance");
        let text = content(&node);
        text.contains("ELF declared VA") && !text.contains("encoded integer")
    });
    keep(
        &mut passed,
        "carry arithmetic preserves coordinate provenance",
        coordinate_ok,
        "coordinate nodes lost declared VA provenance or collapsed to an encoded integer",
    );

    let identities = [
        ("identity_sub", "X20", "0x0"),
        ("identity_flags", "X22", "0x0"),
        ("identity_sbc", "X23", "0x0"),
        ("identity_bic", "X24", "0x0"),
        ("identity_orn", "X25", "0xffffffffffffffff"),
        ("identity_eon", "X26", "0xffffffffffffffff"),
        ("identity_selected", "X27", "0xffffffffffffffff"),
        ("identity_bics", "X28", "0x0"),
    ];
    for (name, reg, value) in identities {
        let check = format!("{name} retains its register identity and flags");
        let node = take_node(&nodes, &symbols, name, &check);
        let text = content(&node);
        if !text.contains(&format!("{reg} <- encoded integer {value}")) {
            panic!("{check}: {text}");
        }
        passed.push(check);
    }

    let carry = take_node(&nodes, &symbols, "adc64", "native view retains actual carry result");
    let carry_content = content(&carry);
    let node_id = carry.get("id").cloned().unwrap_or_else(|| panic!("native view retains actual carry result"));
    let node_text = scalar_text(&node_id);

    event = drive(controller, &event, "tool_result", tool_result_inputs(&effect, &output), scratch);
    let record_id = last_record_id(&event);
    let record_text = scalar_text(&record_id);

    event = drive(controller, &event, &format!("investigation_record:{record_text}"), obj(vec![]), scratch);
    event = drive(controller, &event, "investigation_graph", obj(vec![]), scratch);
    event = drive(
        controller,
        &event,
        &format!("investigation_graph_node:{node_text}"),
        obj(vec![]),
        scratch,
    );
    let ui = event.get("ui").cloned().unwrap_or(Value::Null);
    keep(
        &mut passed,
        "native view retains actual carry result",
        joined_strings(&ui).contains("X3 <- encoded integer 0x2a"),
        &joined_strings(&ui),
    );

    let mut controlled = session(&event).clone();
    put(
        &mut controlled,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("model".into(), s("controlled")),
            ("endpoint".into(), s("http://controlled.test/model")),
            ("plain".into(), Value::Bool(true)),
            ("stream".into(), Value::Bool(false)),
            ("protocol".into(), s("chat")),
        ])]),
    );
    event = controller.event(
        Some(&controlled),
        "send",
        obj(vec![("draft".into(), s("Read the actual retained carry arithmetic."))]),
        scratch,
    );
    let call = obj(vec![
        ("tool".into(), s("observations")),
        (
            "args".into(),
            obj(vec![
                ("id".into(), record_id),
                ("snapshot".into(), twin::n("0")),
                ("node".into(), node_id),
            ]),
        ),
    ]);
    event = reply(controller, &event, &call.to_string(), scratch);
    event = drive(controller, &event, "approve", obj(vec![]), scratch);
    keep(
        &mut passed,
        "ordinary model query receives actual carry result",
        wire_has(&event, "X3 <- encoded integer 0x2a"),
        &format!("effect={} notice={}", effect_text(&event), notice(&event)),
    );
    event = reply(controller, &event, "Actual carry result received.", scratch);

    event = drive(controller, &event, &format!("investigation_record:{record_text}"), obj(vec![]), scratch);
    event = drive(controller, &event, "investigation_graph", obj(vec![]), scratch);
    event = drive(
        controller,
        &event,
        &format!("investigation_graph_node:{node_text}"),
        obj(vec![]),
        scratch,
    );
    event = drive(controller, &event, "reconstruction_new", obj(vec![]), scratch);
    event = drive(
        controller,
        &event,
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(&record_text)),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&node_text)),
            ("mode".into(), s("representation")),
            ("output".into(), s("carry-result.txt")),
        ]),
        scratch,
    );
    event = drive(controller, &event, "approve", obj(vec![]), scratch);
    let export_effect = event.get("effect").cloned().unwrap_or(Value::Null);
    if matches!(export_effect, Value::Null) {
        panic!("guarded export retains exact carry representation: {}", notice(&event));
    }
    let exported = tool(controller, &export_effect, scratch);
    let bytes = std::fs::read(format!("{scratch}/carry-result.txt")).unwrap_or_else(|error| {
        panic!("guarded export retains exact carry representation: {error}")
    });
    keep(
        &mut passed,
        "guarded export retains exact carry representation",
        bytes == carry_content.as_bytes() && exported.contains("read back exactly"),
        &format!("file_len={} content_len={} output={exported}", bytes.len(), carry_content.len()),
    );
    passed
}

fn drive(controller: &mut Controller, event: &Value, action: &str, inputs: Value, root: &str) -> Value {
    let state = session(event).clone();
    controller.event(Some(&state), action, inputs, root)
}

fn session(event: &Value) -> &Value {
    event.get("state").unwrap_or_else(|| panic!("event returned no state: {}", event.to_string()))
}

fn notice(event: &Value) -> String {
    session(event).get("notice").map(Value::to_string).unwrap_or_default()
}

fn effect_text(event: &Value) -> String {
    event.get("effect").map(Value::to_string).unwrap_or_else(|| "missing".to_string())
}

fn reply(controller: &mut Controller, event: &Value, text: &str, root: &str) -> Value {
    let state = session(event).clone();
    let expected = state.get("model_expected").cloned().unwrap_or_else(|| panic!("reply has no model_expected: {}", notice(event)));
    let body = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )])
    .to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    controller.event(
        Some(&state),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), expected),
            ("data".into(), s(&twin::b64_encode(wire.as_bytes()))),
        ]),
        root,
    )
}

fn tool(controller: &mut Controller, effect: &Value, root: &str) -> String {
    let Value::Obj(fields) = effect else {
        panic!("effect is not an object");
    };
    let mut envelope = Vec::new();
    for (key, value) in fields {
        if key != "op" && key != "root" {
            envelope.push((key.clone(), value.clone()));
        }
    }
    envelope.push(("op".into(), s("tool")));
    envelope.push(("root".into(), s(root)));
    match controller.op(&Value::Obj(envelope)) {
        Value::Str(output) => output,
        other => panic!("tool did not return a string: {}", other.to_string()),
    }
}

fn tool_result_inputs(effect: &Value, output: &str) -> Value {
    let mut fields = Vec::new();
    for key in ["tool_ticket", "tool_binding", "editor", "editor_dialog"] {
        if let Some(value) = effect.get(key) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    fields.push(("purpose".into(), s("local_tool")));
    fields.push(("output".into(), s(output)));
    Value::Obj(fields)
}

fn first_snapshot(output: &str) -> Value {
    let parsed = Value::parse(output);
    let snapshots = parsed.get("snapshots").unwrap_or_else(|| panic!("tool returned no snapshots"));
    let items = snapshots.arr();
    items.first().cloned().unwrap_or_else(|| panic!("tool returned no snapshots"))
}

fn index_nodes(graph: &Value) -> HashMap<u64, Value> {
    let mut nodes = HashMap::new();
    let items = graph.get("nodes").unwrap_or_else(|| panic!("graph has no nodes")).arr();
    for node in items.iter().skip(1) {
        let address = node
            .get("encoding")
            .and_then(|encoding| encoding.get("address"))
            .unwrap_or_else(|| panic!("encoding node has no address"));
        let Value::Num(token) = address else {
            panic!("encoding address is not a number: {}", address.to_string());
        };
        let address = token.parse::<u64>().unwrap_or_else(|_| panic!("encoding address is not a u64: {token}"));
        nodes.insert(address, node.clone());
    }
    nodes
}

fn take_node(nodes: &HashMap<u64, Value>, symbols: &HashMap<String, u64>, name: &str, check: &str) -> Value {
    let Some(address) = symbols.get(name) else {
        panic!("{check}: missing symbol {name}");
    };
    nodes.get(address).cloned().unwrap_or_else(|| panic!("{check}: missing node {name} at {address:#x}"))
}

fn content(node: &Value) -> String {
    node.get("content").map(Value::str_).unwrap_or_else(|| panic!("node has no content")).to_string()
}

fn last_record_id(event: &Value) -> Value {
    let state = session(event);
    let records = state.get("investigation_records").unwrap_or_else(|| panic!("no investigation_records: {}", notice(event)));
    let items = records.get("items").unwrap_or_else(|| panic!("investigation records have no items")).arr();
    let record = items.last().unwrap_or_else(|| panic!("no investigation record"));
    record.get("id").cloned().unwrap_or_else(|| panic!("investigation record has no id"))
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::Str(text) => text.clone(),
        Value::Num(token) => token.clone(),
        other => panic!("expected a string or number id, got {}", other.to_string()),
    }
}

fn wire_has(event: &Value, needle: &str) -> bool {
    match event.get("effect") {
        Some(effect) if !matches!(effect, Value::Null) => match effect.get("wire") {
            Some(Value::Str(text)) => text.contains(needle),
            _ => false,
        },
        _ => false,
    }
}

fn joined_strings(value: &Value) -> String {
    let mut parts = Vec::new();
    collect_strings(value, &mut parts);
    parts.join("\n")
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Str(text) => out.push(text.clone()),
        Value::Arr(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Obj(fields) => {
            for (_, item) in fields {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn parse_symbols(nm: &str) -> HashMap<String, u64> {
    let mut symbols = HashMap::new();
    for line in nm.lines() {
        let bytes = line.as_bytes();
        let mut index = 0;
        while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
            index += 1;
        }
        if index == 0 || index == bytes.len() || !bytes[index].is_ascii_whitespace() {
            continue;
        }
        let address = &line[..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            continue;
        }
        let kind = bytes[index];
        if !(kind.is_ascii_alphanumeric() || kind == b'_') {
            continue;
        }
        index += 1;
        if index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
            continue;
        }
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || line[index..].chars().any(char::is_whitespace) {
            continue;
        }
        let Ok(address) = u64::from_str_radix(address, 16) else {
            continue;
        };
        symbols.insert(line[index..].to_string(), address);
    }
    symbols
}

fn command(program: &str, args: &[&str], scratch: &str) -> String {
    let output = std::process::Command::new(program)
        .args(args)
        .env("TMPDIR", scratch)
        .output()
        .unwrap_or_else(|error| panic!("could not start {program}: {error}"));
    if !output.status.success() {
        panic!("{program} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).unwrap_or_else(|error| panic!("{program} output was not UTF-8: {error}"))
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool, detail: &str) {
    if !ok {
        panic!("{name}: {detail}");
    }
    passed.push(name.to_string());
}

// The scratch tree is this twin's own working area. It must not survive the
// twin on any exit path, including a panic, so the removal rides on Drop the
// way the other twins that clean up after themselves do.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

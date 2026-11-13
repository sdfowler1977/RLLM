//! Individually known flags through assembled flow, queries, UI and exact export.
mod twin;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use twin::{b, b64_encode, n, obj, pretty, s, Controller, Value};

const SCOPE: &str = "Owner current shared-core checks with original LLVM-assembled AArch64 flag-producing and consuming instructions, independent disassembly, retained observations, native UI descriptions, controlled provider query, exact export and genuine immediately-published core capture compatibility.";

const ASM: &str = r#".text
.global entry
entry:
 mov x4,#42
 mov x5,#7
 ands xzr,x0,#0xff
flag_adc: adc x1,xzr,xzr
flag_sbc: sbc x2,xzr,xzr
flag_cc: csel x6,x4,x5,cc
flag_vc: csel x7,x4,x5,vc
flag_pl: csel x8,x4,x5,pl
flag_eq: csel x9,x4,x5,eq
flag_hi: csel x10,x4,x5,hi
flag_ls: csel x11,x4,x5,ls
 ret
unknown_sign_entry:
 mov x4,#42
 mov x5,#7
 ands xzr,x0,x0
unknown_sign_vc: csel x12,x4,x5,vc
unknown_sign_pl: csel x13,x4,x5,pl
 ret
arithmetic_entry:
 mov x4,#42
 mov x5,#7
 and x12,x0,#0xfffffffffffffff0
 adds x13,x12,#1
arithmetic_adc: adc x14,xzr,xzr
arithmetic_ne: csel x15,x4,x5,ne
arithmetic_eq: csel x16,x4,x5,eq
arithmetic_vc: csel x17,x4,x5,vc
arithmetic_pl: csel x18,x4,x5,pl
 ret
word_entry:
 mov w4,#42
 mov w5,#7
 ands wzr,w0,#0xff
word_adc: adc w1,wzr,wzr
word_cc: csel w2,w4,w5,cc
 ret
join_entry:
 cbz x0,join_left
 mov x4,#42
 mov x5,#7
 mov x1,#0
 ands xzr,x1,x1
 b join_target
join_left:
 mov x4,#42
 mov x5,#7
 mov x1,#1
 ands xzr,x1,x1
 b join_target
join_target:
join_adc: adc x2,xzr,xzr
join_vc: csel x3,x4,x5,vc
 ret
"#;

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(Path::new(&project));
    let passed = run(&project, scratch.path());
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("flags_flow", &body, true);
    let shown = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
        ("count".into(), n(&passed.len().to_string())),
    ]);
    println!("{}", pretty(&shown));
}

fn run(project: &str, scratch: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let work = Path::new(scratch);
    let asm = work.join("flags.S");
    let elf = work.join("flags.elf");
    std::fs::write(&asm, ASM).unwrap_or_else(|error| panic!("could not write {}: {error}", asm.display()));

    let ndk = PathBuf::from(std::env::var("CLIENT_NDK").unwrap_or_else(|_| panic!("CLIENT_NDK is not set")))
        .join("toolchains/llvm/prebuilt/linux-x86_64/bin");
    run_checked(
        Command::new(ndk.join("clang"))
            .arg("--target=aarch64-linux-android26")
            .arg("-nostdlib")
            .arg("-static")
            .arg("-fuse-ld=lld")
            .arg("-Wl,-e,entry")
            .arg("-Wl,--emit-relocs")
            .arg(&asm)
            .arg("-o")
            .arg(&elf)
            .env("TMPDIR", work),
        "clang",
    );
    let nm = command_text(Command::new(ndk.join("llvm-nm")).arg("-n").arg(&elf), "llvm-nm");
    let mut symbols = HashMap::new();
    for line in nm.lines() {
        if let Some((address, name)) = parse_symbol(line) {
            symbols.insert(name, address);
        }
    }
    let disassembly = command_text(Command::new(ndk.join("llvm-objdump")).arg("-d").arg(&elf), "llvm-objdump");

    let mut controller = Controller::spawn(project);
    let (mut d, graph) = inspect(&mut controller, scratch, "flags.elf");
    let nodes = index_nodes(&graph);
    keep(
        &mut passed,
        "independent assembler retains actual flag consumers",
        disassembly.contains("adc") && disassembly.contains("csel") && disassembly.contains("tst"),
    );

    for (name, register, value) in [
        ("flag_adc", "X1", 0u64),
        ("flag_sbc", "X2", u64::MAX),
        ("flag_cc", "X6", 42),
        ("flag_vc", "X7", 42),
        ("flag_pl", "X8", 42),
        ("flag_hi", "X10", 7),
        ("flag_ls", "X11", 42),
        ("unknown_sign_vc", "X12", 42),
        ("arithmetic_adc", "X14", 0),
        ("arithmetic_ne", "X15", 42),
        ("arithmetic_eq", "X16", 7),
        ("arithmetic_vc", "X17", 42),
        ("word_adc", "W1", 0),
        ("word_cc", "W2", 42),
        ("join_adc", "X2", 0),
        ("join_vc", "X3", 42),
    ] {
        let check = format!("{name} uses the determining known flags");
        let expected = format!("{register} <- encoded integer 0x{value:x}");
        keep(&mut passed, &check, content_of(&nodes, &symbols, name, &check).contains(&expected));
    }

    for (name, register) in [("flag_eq", "X9"), ("unknown_sign_pl", "X13"), ("arithmetic_pl", "X18")] {
        let check = format!("{name} retains its actual unresolved condition");
        let expected = format!("{register} <- partial integer bits0x2, known mask0xffffffffffffffd2");
        keep(&mut passed, &check, content_of(&nodes, &symbols, name, &check).contains(&expected));
    }

    keep(
        &mut passed,
        "logical carry is independently available to ADC",
        content_of(&nodes, &symbols, "flag_adc", "logical carry is independently available to ADC").contains("incoming carry 0"),
    );
    keep(
        &mut passed,
        "arithmetic carry survives an unknown result sign",
        content_of(&nodes, &symbols, "arithmetic_adc", "arithmetic carry survives an unknown result sign").contains("incoming carry 0"),
    );
    keep(
        &mut passed,
        "joined flag display labels its unknown Z directly",
        content_of(&nodes, &symbols, "join_adc", "joined flag display labels its unknown Z directly")
            .contains("NZCV N=0 Z=unknown C=0 V=0"),
    );

    let record = last_record(state(&d), "native view retains the actual known-carry result");
    let record_id = shown(record.get("id").unwrap_or(&Value::Null), "native view retains the actual known-carry result");
    let flag_adc = node_of(&nodes, &symbols, "flag_adc", "native view retains the actual known-carry result");
    let node_id = flag_adc.get("id").cloned().unwrap_or(Value::Null);
    let node_label = shown(&node_id, "native view retains the actual known-carry result");
    d = step(&mut controller, &d, &format!("investigation_record:{record_id}"), obj(vec![]), scratch);
    d = step(&mut controller, &d, "investigation_graph", obj(vec![]), scratch);
    d = step(&mut controller, &d, &format!("investigation_graph_node:{node_label}"), obj(vec![]), scratch);
    keep(
        &mut passed,
        "native view retains the actual known-carry result",
        joined(d.get("ui").unwrap_or(&Value::Null)).contains("incoming carry 0; X1 <- encoded integer 0x0"),
    );

    let mut st = state(&d).clone();
    st = put(
        st,
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
    d = event(
        &mut controller,
        Some(&st),
        "send",
        obj(vec![("draft".into(), s("Read the retained independently known carry."))]),
        scratch,
    );
    d = reply(&mut controller, &d, &observations_text(&record_id, &node_id), scratch);
    d = step(&mut controller, &d, "approve", obj(vec![]), scratch);
    let wire_ok = match d.get("effect") {
        Some(effect) if !matches!(effect, Value::Null) => match effect.get("wire") {
            Some(Value::Str(wire)) => wire.contains("incoming carry 0; X1 <- encoded integer 0x0"),
            _ => false,
        },
        _ => false,
    };
    keep(&mut passed, "ordinary model query receives the actual known-carry result", wire_ok);
    d = reply(&mut controller, &d, "Known-carry result received.", scratch);

    d = step(&mut controller, &d, &format!("investigation_record:{record_id}"), obj(vec![]), scratch);
    d = step(&mut controller, &d, "investigation_graph", obj(vec![]), scratch);
    d = step(&mut controller, &d, &format!("investigation_graph_node:{node_label}"), obj(vec![]), scratch);
    d = step(&mut controller, &d, "reconstruction_new", obj(vec![]), scratch);
    d = step(
        &mut controller,
        &d,
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(&record_id)),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&node_label)),
            ("mode".into(), s("representation")),
            ("output".into(), s("flags-result.txt")),
        ]),
        scratch,
    );
    d = step(&mut controller, &d, "approve", obj(vec![]), scratch);
    let export_check = "reviewed export retains the exact flag-derived representation";
    let effect = d.get("effect").cloned().unwrap_or_else(|| panic!("{export_check}"));
    if matches!(effect, Value::Null) {
        panic!("{export_check}");
    }
    let output = tool(&mut controller, &effect, scratch, export_check);
    let exported = std::fs::read(work.join("flags-result.txt")).unwrap_or_else(|_| panic!("{export_check}"));
    let content = match flag_adc.get("content") {
        Some(Value::Str(text)) => text.as_bytes().to_vec(),
        _ => panic!("{export_check}"),
    };
    keep(&mut passed, export_check, exported == content && output.contains("read back exactly"));

    let mut published = Controller::spawn_at(&Path::new(project).join("output/published-controller"), project);
    let (prior, prior_graph) = inspect(&mut published, scratch, "flags.elf");
    published.finish();

    let published_check = "actual published capture retains the same flag-consumer encoding";
    let flag_address = symbols.get("flag_adc").copied().unwrap_or_else(|| panic!("{published_check}"));
    let prior_node = child_nodes(&prior_graph)
        .into_iter()
        .find(|candidate| address_of(candidate) == Some(flag_address))
        .unwrap_or_else(|| panic!("{published_check}"));
    let bits_match = match (
        prior_node.get("encoding").and_then(|encoding| encoding.get("bits")),
        flag_adc.get("encoding").and_then(|encoding| encoding.get("bits")),
    ) {
        (Some(left), Some(right)) => numbers_equal(left, right),
        _ => false,
    };
    let mnemonic_match = matches!(
        prior_node.get("encoding").and_then(|encoding| encoding.get("mnemonic")),
        Some(Value::Str(text)) if text == "ADC"
    );
    keep(&mut passed, published_check, bits_match && mnemonic_match);

    let view_check = "current view preserves the actual prior flag capture";
    let resumed = event(&mut controller, None, "resume", obj(vec![]), scratch);
    let mut st = state(&resumed).clone();
    let records = prior
        .get("state")
        .and_then(|state| state.get("investigation_records"))
        .cloned()
        .unwrap_or_else(|| panic!("{view_check}"));
    st = put(st, "investigation_records", records);
    let prior_record = last_record(&st, view_check);
    let prior_id = shown(prior_record.get("id").unwrap_or(&Value::Null), view_check);
    let prior_label = shown(prior_node.get("id").unwrap_or(&Value::Null), view_check);
    let mut current = event(&mut controller, Some(&st), &format!("investigation_record:{prior_id}"), obj(vec![]), scratch);
    current = step(&mut controller, &current, "investigation_graph", obj(vec![]), scratch);
    current = step(&mut controller, &current, &format!("investigation_graph_node:{prior_label}"), obj(vec![]), scratch);
    let needle = match prior_node.get("content") {
        Some(Value::Str(text)) => text.trim().to_string(),
        _ => panic!("{view_check}"),
    };
    keep(&mut passed, view_check, joined(current.get("ui").unwrap_or(&Value::Null)).contains(&needle));

    controller.finish();
    passed
}

trait Drive {
    fn call(&mut self, envelope: &Value) -> Value;
}

impl Drive for Controller {
    fn call(&mut self, envelope: &Value) -> Value {
        self.op(envelope)
    }
}

fn inspect<D: Drive>(drive: &mut D, root: &str, elf_name: &str) -> (Value, Value) {
    let mut d = event(drive, None, "resume", obj(vec![]), root);
    d = step(drive, &d, "investigation", obj(vec![]), root);
    d = step(drive, &d, "inspect_file", obj(vec![("path".into(), s(elf_name))]), root);
    d = step(drive, &d, "approve", obj(vec![]), root);
    let effect = d.get("effect").cloned().unwrap_or_else(|| panic!("inspection was not approved"));
    if matches!(effect, Value::Null) {
        panic!("inspection was not approved");
    }
    let output = tool(drive, &effect, root, "inspection tool");
    let graph = first_snapshot(&output);
    let d = step(drive, &d, "tool_result", tool_inputs(&effect, &output), root);
    (d, graph)
}

fn tool<D: Drive>(drive: &mut D, effect: &Value, root: &str, check: &str) -> String {
    let value = drive.call(&tool_envelope(effect, root));
    match value {
        Value::Str(text) => text,
        other => panic!("{check}: tool result was not a string: {}", other.to_string()),
    }
}

fn tool_envelope(effect: &Value, root: &str) -> Value {
    let mut envelope = match effect {
        Value::Obj(fields) => Value::Obj(fields.clone()),
        other => panic!("effect is not an object: {}", other.to_string()),
    };
    envelope = put(envelope, "op", s("tool"));
    envelope = put(envelope, "root", s(root));
    envelope
}

fn tool_inputs(effect: &Value, output: &str) -> Value {
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
    match parsed.get("snapshots") {
        Some(Value::Arr(items)) => items.first().cloned().unwrap_or_else(|| panic!("tool output snapshots is empty")),
        _ => panic!("tool output has no snapshots: {output}"),
    }
}

fn observations_text(record_id: &str, node_id: &Value) -> String {
    obj(vec![
        ("tool".into(), s("observations")),
        (
            "args".into(),
            obj(vec![
                ("id".into(), s(record_id)),
                ("snapshot".into(), n("0")),
                ("node".into(), node_id.clone()),
            ]),
        ),
    ])
    .to_string()
}

fn reply<D: Drive>(drive: &mut D, current: &Value, text: &str, root: &str) -> Value {
    let st = state(current).clone();
    let ticket = st.get("model_expected").cloned().unwrap_or_else(|| panic!("ordinary model query receives the actual known-carry result"));
    let body = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]);
    let encoded = body.to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{encoded}", encoded.len());
    event(
        drive,
        Some(&st),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
        root,
    )
}

fn event<D: Drive>(drive: &mut D, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    drive.call(&obj(vec![
        ("op".into(), s("event")),
        ("state".into(), state.cloned().unwrap_or(Value::Null)),
        ("action".into(), s(action)),
        ("inputs".into(), inputs),
        ("root".into(), s(root)),
    ]))
}

fn step<D: Drive>(drive: &mut D, current: &Value, action: &str, inputs: Value, root: &str) -> Value {
    let st = state(current).clone();
    event(drive, Some(&st), action, inputs, root)
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn last_record<'a>(state: &'a Value, check: &str) -> &'a Value {
    match state.get("investigation_records").and_then(|records| records.get("items")) {
        Some(Value::Arr(items)) => items.last().unwrap_or_else(|| panic!("{check}")),
        _ => panic!("{check}"),
    }
}

fn index_nodes(graph: &Value) -> HashMap<u64, Value> {
    let items = match graph.get("nodes") {
        Some(Value::Arr(items)) => items,
        _ => panic!("observation graph has no nodes"),
    };
    let mut nodes = HashMap::new();
    for node in items.iter().skip(1) {
        let address = address_of(node).unwrap_or_else(|| panic!("observation node has no numeric address"));
        nodes.insert(address, node.clone());
    }
    nodes
}

fn child_nodes(graph: &Value) -> Vec<Value> {
    match graph.get("nodes") {
        Some(Value::Arr(items)) => items.iter().skip(1).cloned().collect(),
        _ => panic!("actual published capture retains the same flag-consumer encoding"),
    }
}

fn address_of(node: &Value) -> Option<u64> {
    match node.get("encoding").and_then(|encoding| encoding.get("address")) {
        Some(Value::Num(token)) => token.parse::<u64>().ok(),
        _ => None,
    }
}

fn node_of<'a>(nodes: &'a HashMap<u64, Value>, symbols: &HashMap<String, u64>, name: &str, check: &str) -> &'a Value {
    let address = symbols.get(name).copied().unwrap_or_else(|| panic!("{check}"));
    nodes.get(&address).unwrap_or_else(|| panic!("{check}"))
}

fn content_of(nodes: &HashMap<u64, Value>, symbols: &HashMap<String, u64>, name: &str, check: &str) -> String {
    match node_of(nodes, symbols, name, check).get("content") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("{check}"),
    }
}

fn numbers_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Num(a), Value::Num(b)) => match (a.parse::<u128>(), b.parse::<u128>()) {
            (Ok(left), Ok(right)) => left == right,
            _ => a == b,
        },
        _ => false,
    }
}

fn shown(value: &Value, check: &str) -> String {
    match value {
        Value::Str(text) => text.clone(),
        Value::Num(token) => token.clone(),
        _ => panic!("{check}"),
    }
}

fn joined(value: &Value) -> String {
    let mut parts = Vec::new();
    collect_strings(value, &mut parts);
    parts.join("\n")
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Str(text) => out.push(text.clone()),
        Value::Obj(fields) => {
            for (_, value) in fields {
                collect_strings(value, out);
            }
        }
        Value::Arr(items) => {
            for value in items {
                collect_strings(value, out);
            }
        }
        _ => {}
    }
}

fn put(value: Value, key: &str, next: Value) -> Value {
    let mut value = value;
    let Value::Obj(fields) = &mut value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
    value
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn parse_symbol(line: &str) -> Option<(u64, String)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
        index += 1;
    }
    if index == 0 || index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
        return None;
    }
    let address = u64::from_str_radix(&line[..index], 16).ok()?;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let kind = *bytes.get(index)?;
    if !(kind.is_ascii_alphanumeric() || kind == b'_') {
        return None;
    }
    index += 1;
    if index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
        return None;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let name = &line[index..];
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return None;
    }
    Some((address, name.to_string()))
}

fn run_checked(command: &mut Command, what: &str) {
    let output = command.output().unwrap_or_else(|error| panic!("{what}: {error}"));
    if !output.status.success() {
        panic!("{what} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

fn command_text(command: &mut Command, what: &str) -> String {
    let output = command.output().unwrap_or_else(|error| panic!("{what}: {error}"));
    if !output.status.success() {
        panic!("{what} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).unwrap_or_else(|error| panic!("{what} output is not UTF-8: {error}"))
}


struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "flags-flow"))
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

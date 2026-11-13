//! Product identities and known bits through assembled flow, queries, UI and exact export.
mod twin;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use twin::{b, b64_encode, n, obj, pretty, s, Controller, Value};

const SCOPE: &str = "Owner current shared-core checks with original LLVM-assembled AArch64 product instruction encodings, independent disassembly, retained observations, native UI descriptions, controlled provider query, exact export and genuine immediately-published core capture compatibility. Connected clean-room reconstruction of one retained complete JavaScript file, the exclusive-create refusal, and named refusals for an incomplete file observation and a subject over the 131072-byte cap. The generated file is a non-empty skeleton whose digest differs from the retained subject observation and whose statements are the reported spec items. A fresh inspect of that file must observe the same spec items. An incomplete observation is refused by notice before any tool effect, and the output file is absent.";

const ASM: &str = r#".text
.global entry
entry:
zero_product: madd x1,x0,xzr,xzr
 mov x2,#42
zero_addend: madd x3,x0,xzr,x2
zero_subtract: msub x4,x0,xzr,x2
 movk x0,#0x1234
 mov x6,#1
unit_product: madd x7,x0,x6,xzr
same_register_cancel: msub x8,x0,x6,x0
 mov x9,#2
shift_product: madd x10,x0,x9,xzr
commuted_product: madd x11,x9,x0,xzr
 movk x12,#0xabcd
low_product: madd x13,x0,x12,xzr
masked_product: and x14,x13,#0xffff
 and x15,x0,#0xffffffff00000000
forced_zero: madd x16,x15,x15,xzr
forced_accumulator: madd x17,x15,x15,x2
 mov x18,#3
 mov x19,#2
 movk x20,#0x1000
partial_addend: madd x21,x18,x19,x20
partial_subtract: msub x22,x18,x19,x20
masked_addend: and x23,x21,#0xffff
 ands xzr,xzr,xzr
flag_product: madd x24,x0,xzr,x2
preserved_flags: csel x25,x2,xzr,eq
 ret
word_entry:
 mov w2,#42
word_zero: madd w1,w0,wzr,wzr
word_accumulator: msub w3,w0,wzr,w2
 movk w0,#0x1234
 mov w6,#2
word_shift: madd w7,w0,w6,wzr
word_mask: and x8,x7,#0xffff
 ret
coordinate_entry:
 adr x1,entry
 mov x2,#1
coordinate_unit: madd x3,x1,x2,xzr
coordinate_accumulator: madd x4,x0,xzr,x1
coordinate_cancel: msub x5,x1,x2,x1
 mov x6,#2
coordinate_shift: madd x7,x1,x6,xzr
 ret
"#;

const FEATURE_JS: &str = "const {app} = require(\"electron\");\nconst helper = require(\"./helper.js\");\nipcMain.on(\"window:open\", function () {});\nrouter.get(\"/status\", function () {});\nexport function openWindow() {}\n";
const DIGEST_METHOD: &str = "SHA-256 of every byte read from the complete held regular file";
const INCOMPLETE_NOTICE: &str = "Clean-room reconstruction requires a complete unchanged observed file; missing or prefix content is not reconstructed";
const NO_APPROVAL_NOTICE: &str = "No current operation is awaiting approval";
const EXISTS_ERROR: &str = "Cannot create new streaming output: File exists (os error 17)";
const CAP_ERROR: &str = "Refusal: clean-room subject-byte cap. 131073 exceeds 131072. No output was written.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(Path::new(&project));
    let passed = run(&project, scratch.path());
    if passed.len() != 37 {
        panic!("emitted {} checks, expected 37", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("product_flow", &body, true);
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
    let asm = work.join("product.S");
    let elf = work.join("product.elf");
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
    let (mut d, graph) = inspect(&mut controller, scratch, "product.elf");
    let nodes = index_nodes(&graph);
    keep(
        &mut passed,
        "independent assembler retains actual product instruction encodings",
        disassembly.contains("madd") && disassembly.contains("msub") && disassembly.contains("mul"),
    );

    for (name, register, value) in [
        ("zero_product", "X1", 0u64),
        ("zero_addend", "X3", 42),
        ("zero_subtract", "X4", 42),
        ("same_register_cancel", "X8", 0),
        ("masked_product", "X14", 0x1234 * 0xabcd & 0xffff),
        ("forced_zero", "X16", 0),
        ("forced_accumulator", "X17", 42),
        ("masked_addend", "X23", 0x1006),
        ("flag_product", "X24", 42),
        ("preserved_flags", "X25", 42),
        ("word_zero", "W1", 0),
        ("word_accumulator", "W3", 42),
        ("word_mask", "X8", 0x2468),
        ("coordinate_cancel", "X5", 0),
    ] {
        let check = format!("{name} retains its determined product or accumulator");
        let expected = format!("{register} <- encoded integer 0x{value:x}");
        keep(&mut passed, &check, content_of(&nodes, &symbols, name, &check).contains(&expected));
    }

    for (name, register, bits, known) in [
        ("unit_product", "X7", 0x1234u64, 0xffffu64),
        ("shift_product", "X10", 0x2468, 0x1ffff),
        ("commuted_product", "X11", 0x2468, 0x1ffff),
        ("low_product", "X13", 0x1234 * 0xabcd & 0xffff, 0xffff),
        ("partial_addend", "X21", 0x1006, 0xffff),
        ("partial_subtract", "X22", 0xffa, 0xffff),
        ("word_shift", "W7", 0x2468, 0xffffffff0001ffff),
        ("coordinate_shift", "X7", 0, 1),
    ] {
        let check = format!("{name} retains each proved product bit");
        let expected = format!("{register} <- partial integer bits0x{bits:x}, known mask0x{known:x}");
        keep(&mut passed, &check, content_of(&nodes, &symbols, name, &check).contains(&expected));
    }

    for name in ["coordinate_unit", "coordinate_accumulator"] {
        let check = format!("{name} preserves source-coordinate identity");
        keep(
            &mut passed,
            &check,
            content_of(&nodes, &symbols, name, &check).contains("encoded source coordinate declared VA "),
        );
    }

    let view_check = "native view retains actual partial product bits";
    let record = last_record(state(&d), view_check).clone();
    let record_id = shown(record.get("id").unwrap_or(&Value::Null), view_check);
    let shift = node_of(&nodes, &symbols, "shift_product", view_check).clone();
    let node_id = shift.get("id").cloned().unwrap_or_else(|| panic!("{view_check}"));
    let node_label = shown(&node_id, view_check);
    d = step(&mut controller, &d, &format!("investigation_record:{record_id}"), obj(vec![]), scratch);
    d = step(&mut controller, &d, "investigation_graph", obj(vec![]), scratch);
    d = step(&mut controller, &d, &format!("investigation_graph_node:{node_label}"), obj(vec![]), scratch);
    keep(
        &mut passed,
        view_check,
        joined(d.get("ui").unwrap_or(&Value::Null)).contains("X10 <- partial integer bits0x2468, known mask0x1ffff"),
    );

    let query_check = "ordinary model query receives the actual product bits";
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
    d = controller.event(
        Some(&st),
        "send",
        obj(vec![("draft".into(), s("Read the retained product bits."))]),
        scratch,
    );
    d = reply(&mut controller, &d, &observations_text(record.get("id").unwrap_or(&Value::Null), &node_id), scratch, query_check);
    d = step(&mut controller, &d, "approve", obj(vec![]), scratch);
    let wire_ok = match d.get("effect") {
        Some(effect) if !matches!(effect, Value::Null) => match effect.get("wire") {
            Some(Value::Str(wire)) => wire.contains("X10 <- partial integer bits0x2468, known mask0x1ffff"),
            _ => false,
        },
        _ => false,
    };
    keep(&mut passed, query_check, wire_ok);
    d = reply(&mut controller, &d, "Product bits received.", scratch, query_check);

    let export_check = "reviewed export retains the exact product representation";
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
            ("output".into(), s("product-result.txt")),
        ]),
        scratch,
    );
    d = step(&mut controller, &d, "approve", obj(vec![]), scratch);
    let effect = d.get("effect").cloned().unwrap_or_else(|| panic!("{export_check}"));
    if matches!(effect, Value::Null) {
        panic!("{export_check}");
    }
    let output = tool(&mut controller, &effect, scratch, export_check);
    let exported = std::fs::read(work.join("product-result.txt")).unwrap_or_else(|_| panic!("{export_check}"));
    let content = match shift.get("content") {
        Some(Value::Str(text)) => text.as_bytes().to_vec(),
        _ => panic!("{export_check}"),
    };
    keep(&mut passed, export_check, exported == content && output.contains("read back exactly"));

    let mut published = Controller::spawn_at(&Path::new(project).join("output/published-controller"), project);
    let (prior, prior_graph) = inspect(&mut published, scratch, "product.elf");
    published.finish();

    let published_check = "actual published capture retains the same product encoding";
    let shift_address = symbols.get("shift_product").copied().unwrap_or_else(|| panic!("{published_check}"));
    let prior_node = child_nodes(&prior_graph)
        .into_iter()
        .find(|candidate| address_of(candidate) == Some(shift_address))
        .unwrap_or_else(|| panic!("{published_check}"));
    let bits_match = match (
        prior_node.get("encoding").and_then(|encoding| encoding.get("bits")),
        shift.get("encoding").and_then(|encoding| encoding.get("bits")),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    };
    let mnemonic_match = matches!(
        prior_node.get("encoding").and_then(|encoding| encoding.get("mnemonic")),
        Some(Value::Str(text)) if text == "MADD"
    );
    keep(&mut passed, published_check, bits_match && mnemonic_match);

    let capture_check = "current view preserves the actual prior product capture";
    let resumed = controller.event(None, "resume", obj(vec![]), scratch);
    let mut st = state(&resumed).clone();
    let records = prior
        .get("state")
        .and_then(|state| state.get("investigation_records"))
        .cloned()
        .unwrap_or_else(|| panic!("{capture_check}"));
    st = put(st, "investigation_records", records);
    let prior_record = last_record(&st, capture_check);
    let prior_id = shown(prior_record.get("id").unwrap_or(&Value::Null), capture_check);
    let prior_label = shown(prior_node.get("id").unwrap_or(&Value::Null), capture_check);
    let mut current = controller.event(Some(&st), &format!("investigation_record:{prior_id}"), obj(vec![]), scratch);
    current = step(&mut controller, &current, "investigation_graph", obj(vec![]), scratch);
    current = step(&mut controller, &current, &format!("investigation_graph_node:{prior_label}"), obj(vec![]), scratch);
    let needle = match prior_node.get("content") {
        Some(Value::Str(text)) => text.trim().to_string(),
        _ => panic!("{capture_check}"),
    };
    keep(&mut passed, capture_check, joined(current.get("ui").unwrap_or(&Value::Null)).contains(&needle));
    controller.finish();

    std::fs::write(work.join("feature.js"), FEATURE_JS).unwrap_or_else(|error| panic!("could not write feature.js: {error}"));
    std::fs::create_dir(work.join("held")).unwrap_or_else(|error| panic!("could not create held: {error}"));
    let mut wide = b"const x = require(\"electron\");\n".to_vec();
    wide.extend(std::iter::repeat(b'x').take(5000));
    std::fs::write(work.join("held").join("wide.js"), wide).unwrap_or_else(|error| panic!("could not write wide.js: {error}"));
    std::fs::write(work.join("oversized.js"), vec![b'x'; 131073]).unwrap_or_else(|error| panic!("could not write oversized.js: {error}"));

    let mut room = Controller::spawn(project);
    clean_room(&mut room, scratch, &mut passed);
    room.finish();
    passed
}

fn clean_room(room: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let work = Path::new(root);
    let subject_check = "clean-room subject retains the complete JavaScript file observation";
    let (mut d, graph) = inspect_content(room, root, "feature.js", Some("complete_digest"));
    let record = last_record(state(&d), subject_check).clone();
    let record_id = shown(record.get("id").unwrap_or(&Value::Null), subject_check);
    let file_node = node_at(&graph, 0, subject_check);
    let sample = file_node.get("sample").unwrap_or(&Value::Null);
    keep(
        passed,
        subject_check,
        text_is(&graph, "kind", "complete_file")
            && text_is(&graph, "coverage", "Complete")
            && text_is(file_node, "kind", "File")
            && matches!(sample.get("complete_file"), Some(Value::Bool(true)))
            && text_is(record.get("args").unwrap_or(&Value::Null), "path", "feature.js")
            && text_is(record.get("args").unwrap_or(&Value::Null), "content", "complete_digest"),
    );

    let report_check = "clean-room reconstruction reports the spec, the verification and the generated file";
    let bytes_check = "clean-room generated file exists and its bytes differ from the reference";
    d = step(room, &d, &format!("investigation_record:{record_id}"), obj(vec![]), root);
    d = step(room, &d, "investigation_graph", obj(vec![]), root);
    d = step(room, &d, "investigation_graph_node:0", obj(vec![]), root);
    d = step(room, &d, "reconstruction_new", obj(vec![]), root);
    d = step(
        room,
        &d,
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(&record_id)),
            ("snapshot".into(), s("0")),
            ("node".into(), s("0")),
            ("mode".into(), s("clean_room")),
            ("output".into(), s("feature-rebuild.js")),
        ]),
        root,
    );
    d = step(room, &d, "approve", obj(vec![]), root);
    let effect = d.get("effect").cloned().unwrap_or_else(|| panic!("{report_check}"));
    if matches!(effect, Value::Null) {
        panic!("{report_check}");
    }
    let out = tool(room, &effect, root, report_check);
    let generated = work.join("feature-rebuild.js");
    keep(
        passed,
        report_check,
        out.contains("Spec items:")
            && out.contains("Verification matched")
            && out.contains("Generated file: feature-rebuild.js")
            && out.contains("matched edge require electron")
            && out.contains("matched edge require ./helper.js")
            && out.contains("matched site ipcMain.on window:open")
            && out.contains("matched site router.get /status")
            && out.contains("matched edge export local"),
    );
    let body = std::fs::read(&generated).unwrap_or_default();
    let digest = twin::sha256_bytes(&body);
    let subject = match sample.get("sha256") {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    };
    let ids = spec_ids(&out).unwrap_or_default();
    let named = statements_for(spec_ids(&out).as_deref());
    let file_lines = std::str::from_utf8(&body).map(|text| text.lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let statements_ok = match &named {
        Some(lines) => lines.len() == ids.len() && lines.iter().all(|line| file_lines.iter().any(|actual| actual == line)),
        None => false,
    };
    keep(
        passed,
        bytes_check,
        generated.is_file()
            && !body.is_empty()
            && !subject.is_empty()
            && digest != subject
            && matches!(sample.get("complete_file"), Some(Value::Bool(true)))
            && matches!(sample.get("method"), Some(Value::Str(text)) if text == DIGEST_METHOD)
            && out.contains(&format!("Output SHA-256: {digest}"))
            && out.contains(&format!("Reference SHA-256 {subject}"))
            && named.is_some()
            && statements_ok,
    );
    let kept = body;
    d = step(room, &d, "tool_result", tool_inputs(&effect, &out), root);

    let reinspected = "clean-room re-inspection of the generated file matches the reported spec items";
    d = step(room, &d, "investigation", obj(vec![]), root);
    d = step(room, &d, "inspect_file", obj(vec![("path".into(), s("feature-rebuild.js"))]), root);
    d = step(room, &d, "approve", obj(vec![]), root);
    let seen_effect = d.get("effect").cloned().unwrap_or_else(|| panic!("{reinspected}"));
    if matches!(seen_effect, Value::Null) {
        panic!("{reinspected}");
    }
    let seen = tool(room, &seen_effect, root, reinspected);
    let found = observed_ids(&seen);
    let reported = spec_count(&out);
    let found_len = found.as_ref().map(Vec::len);
    keep(
        passed,
        reinspected,
        reported.is_some()
            && found.is_some()
            && reported == Some(ids.len())
            && found_len == Some(ids.len())
            && found.as_ref() == Some(&ids),
    );
    d = step(room, &d, "tool_result", tool_inputs(&seen_effect, &seen), root);

    let exclusive = "exclusive create refuses an output name that already exists";
    d = step(room, &d, &format!("investigation_record:{record_id}"), obj(vec![]), root);
    d = step(room, &d, "investigation_graph", obj(vec![]), root);
    d = step(room, &d, "investigation_graph_node:0", obj(vec![]), root);
    d = step(room, &d, "reconstruction_new", obj(vec![]), root);
    d = step(
        room,
        &d,
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(&record_id)),
            ("snapshot".into(), s("0")),
            ("node".into(), s("0")),
            ("mode".into(), s("clean_room")),
            ("output".into(), s("feature-rebuild.js")),
        ]),
        root,
    );
    d = step(room, &d, "approve", obj(vec![]), root);
    let duplicate = d.get("effect").cloned().unwrap_or_else(|| panic!("{exclusive}"));
    if matches!(duplicate, Value::Null) {
        panic!("{exclusive}");
    }
    let answer = room.send(&tool_envelope(&duplicate, root, exclusive));
    let refused_error = match answer.get("error") {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    };
    let still_kept = std::fs::read(&generated).ok().as_deref() == Some(kept.as_slice());
    keep(passed, exclusive, refused_error == EXISTS_ERROR && still_kept);
    d = step(
        room,
        &d,
        "failure",
        obj(vec![
            ("purpose".into(), s("local_tool")),
            ("tool_ticket".into(), copied_or_null(&duplicate, "tool_ticket")),
            ("tool_binding".into(), copied_or_null(&duplicate, "tool_binding")),
            ("error".into(), s(&refused_error)),
        ]),
        root,
    );

    let incomplete = "clean-room reconstruction refuses an observation that is not a complete file";
    d = step(room, &d, "investigation", obj(vec![]), root);
    d = step(room, &d, "inspect_file", obj(vec![("path".into(), s("held"))]), root);
    d = step(room, &d, "approve", obj(vec![]), root);
    let held_effect = d.get("effect").cloned().unwrap_or_else(|| panic!("{incomplete}"));
    if matches!(held_effect, Value::Null) {
        panic!("{incomplete}");
    }
    let held_output = tool(room, &held_effect, root, incomplete);
    let held_graph = first_snapshot(&held_output);
    let held_nodes = match held_graph.get("nodes") {
        Some(Value::Arr(items)) => items.clone(),
        _ => panic!("{incomplete}"),
    };
    let held_file = held_nodes
        .iter()
        .find(|node| text_is(node, "kind", "File"))
        .cloned()
        .unwrap_or_else(|| panic!("{incomplete}"));
    d = step(room, &d, "tool_result", tool_inputs(&held_effect, &held_output), root);
    let partial = last_record(state(&d), incomplete).clone();
    let partial_id = shown(partial.get("id").unwrap_or(&Value::Null), incomplete);
    let held_label = shown(held_file.get("id").unwrap_or(&Value::Null), incomplete);
    d = step(room, &d, &format!("investigation_record:{partial_id}"), obj(vec![]), root);
    d = step(room, &d, "investigation_graph", obj(vec![]), root);
    d = step(room, &d, &format!("investigation_graph_node:{held_label}"), obj(vec![]), root);
    d = step(room, &d, "reconstruction_new", obj(vec![]), root);
    d = step(
        room,
        &d,
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(&partial_id)),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&held_label)),
            ("mode".into(), s("clean_room")),
            ("output".into(), s("wide-rebuild.js")),
        ]),
        root,
    );
    let refused = state(&d).get("notice").cloned();
    let prepared_effect_absent = get_is_none(&d, "effect");
    let prepared_pending_absent = get_is_none(state(&d), "pending");
    d = step(room, &d, "approve", obj(vec![]), root);
    let held_sample = held_file.get("sample").unwrap_or(&Value::Null);
    keep(
        passed,
        incomplete,
        text_is(&held_file, "kind", "File")
            && matches!(held_sample.get("complete_file"), Some(Value::Bool(false)))
            && text_is(partial.get("args").unwrap_or(&Value::Null), "path", "held")
            && matches!(refused, Some(Value::Str(text)) if text == INCOMPLETE_NOTICE)
            && prepared_effect_absent
            && prepared_pending_absent
            && get_is_none(&d, "effect")
            && text_is(state(&d), "notice", NO_APPROVAL_NOTICE)
            && !work.join("wide-rebuild.js").exists(),
    );

    let cap_check = "clean-room reconstruction refuses a subject over the subject-byte cap";
    d = step(room, &d, "investigation", obj(vec![]), root);
    d = step(
        room,
        &d,
        "inspect_file",
        obj(vec![("path".into(), s("oversized.js")), ("content".into(), s("complete_digest"))]),
        root,
    );
    d = step(room, &d, "approve", obj(vec![]), root);
    let cap_effect = d.get("effect").cloned().unwrap_or_else(|| panic!("{cap_check}"));
    if matches!(cap_effect, Value::Null) {
        panic!("{cap_check}");
    }
    let cap_output = tool(room, &cap_effect, root, cap_check);
    let cap_graph = first_snapshot(&cap_output);
    d = step(room, &d, "tool_result", tool_inputs(&cap_effect, &cap_output), root);
    let cap = last_record(state(&d), cap_check).clone();
    let cap_id = shown(cap.get("id").unwrap_or(&Value::Null), cap_check);
    d = step(room, &d, &format!("investigation_record:{cap_id}"), obj(vec![]), root);
    d = step(room, &d, "investigation_graph", obj(vec![]), root);
    d = step(room, &d, "investigation_graph_node:0", obj(vec![]), root);
    d = step(room, &d, "reconstruction_new", obj(vec![]), root);
    d = step(
        room,
        &d,
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(&cap_id)),
            ("snapshot".into(), s("0")),
            ("node".into(), s("0")),
            ("mode".into(), s("clean_room")),
            ("output".into(), s("oversized-rebuild.js")),
        ]),
        root,
    );
    d = step(room, &d, "approve", obj(vec![]), root);
    let cap_final = d.get("effect").cloned().unwrap_or_else(|| panic!("{cap_check}"));
    if matches!(cap_final, Value::Null) {
        panic!("{cap_check}");
    }
    let cap_answer = room.send(&tool_envelope(&cap_final, root, cap_check));
    let cap_error = match cap_answer.get("error") {
        Some(Value::Str(text)) => text.as_str(),
        _ => "",
    };
    keep(
        passed,
        cap_check,
        cap_graph.get("sampled_bytes") == Some(&n("131073"))
            && text_is(cap.get("args").unwrap_or(&Value::Null), "path", "oversized.js")
            && text_is(cap.get("args").unwrap_or(&Value::Null), "content", "complete_digest")
            && cap_error == CAP_ERROR
            && !work.join("oversized-rebuild.js").exists(),
    );
}

fn inspect(controller: &mut Controller, root: &str, elf_name: &str) -> (Value, Value) {
    inspect_content(controller, root, elf_name, None)
}

fn inspect_content(controller: &mut Controller, root: &str, path: &str, content: Option<&str>) -> (Value, Value) {
    let mut d = controller.event(None, "resume", obj(vec![]), root);
    d = step(controller, &d, "investigation", obj(vec![]), root);
    let mut inputs = vec![("path".into(), s(path))];
    if let Some(content) = content {
        inputs.push(("content".into(), s(content)));
    }
    d = step(controller, &d, "inspect_file", obj(inputs), root);
    d = step(controller, &d, "approve", obj(vec![]), root);
    let effect = d.get("effect").cloned().unwrap_or_else(|| panic!("inspection was not approved"));
    if matches!(effect, Value::Null) {
        panic!("inspection was not approved");
    }
    let output = tool(controller, &effect, root, "inspection tool");
    let graph = first_snapshot(&output);
    let d = step(controller, &d, "tool_result", tool_inputs(&effect, &output), root);
    (d, graph)
}

fn tool(controller: &mut Controller, effect: &Value, root: &str, check: &str) -> String {
    let value = controller.op(&tool_envelope(effect, root, check));
    match value {
        Value::Str(text) => text,
        _ => panic!("{check}"),
    }
}

fn tool_envelope(effect: &Value, root: &str, check: &str) -> Value {
    let mut envelope = match effect {
        Value::Obj(fields) => Value::Obj(fields.clone()),
        _ => panic!("{check}"),
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
        _ => panic!("tool output has no snapshots"),
    }
}

fn observations_text(record_id: &Value, node_id: &Value) -> String {
    obj(vec![
        ("tool".into(), s("observations")),
        (
            "args".into(),
            obj(vec![
                ("id".into(), record_id.clone()),
                ("snapshot".into(), n("0")),
                ("node".into(), node_id.clone()),
            ]),
        ),
    ])
    .to_string()
}

fn reply(controller: &mut Controller, current: &Value, text: &str, root: &str, check: &str) -> Value {
    let st = state(current).clone();
    let ticket = st.get("model_expected").cloned().unwrap_or_else(|| panic!("{check}"));
    let body = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]);
    let encoded = body.to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{encoded}", encoded.len());
    controller.event(
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

fn step(controller: &mut Controller, current: &Value, action: &str, inputs: Value, root: &str) -> Value {
    let st = state(current).clone();
    controller.event(Some(&st), action, inputs, root)
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
        _ => panic!("actual published capture retains the same product encoding"),
    }
}

fn node_at<'a>(graph: &'a Value, index: usize, check: &str) -> &'a Value {
    match graph.get("nodes") {
        Some(Value::Arr(items)) => items.get(index).unwrap_or_else(|| panic!("{check}")),
        _ => panic!("{check}"),
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

fn shown(value: &Value, check: &str) -> String {
    match value {
        Value::Str(text) => text.clone(),
        Value::Num(token) => token.clone(),
        _ => panic!("{check}"),
    }
}

fn text_is(value: &Value, key: &str, expected: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expected)
}

fn get_is_none(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn copied_or_null(value: &Value, key: &str) -> Value {
    value.get(key).cloned().unwrap_or(Value::Null)
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

fn spec_ids(report: &str) -> Option<Vec<String>> {
    let mut ids = Vec::new();
    for line in report.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with("spec ") {
            if let Some(end) = line.find(": known:") {
                ids.push(line[5..end].to_string());
            }
        }
    }
    Some(ids)
}

fn statements_for(ids: Option<&[String]>) -> Option<Vec<String>> {
    let ids = ids?;
    if ids.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for id in ids {
        if let Some(rest) = id.strip_prefix("edge ") {
            let (form, spec) = rest.split_once(' ')?;
            match form {
                "require" => out.push(format!("require(\"{spec}\");")),
                "import" => out.push(format!("import \"{spec}\";")),
                "import()" => out.push(format!("import(\"{spec}\");")),
                "export" if spec == "local" => out.push("export function rebuilt() {}".to_string()),
                "export" => out.push(format!("export {{}} from \"{spec}\";")),
                _ => return None,
            }
        } else if let Some(rest) = id.strip_prefix("site ") {
            let (kind, value) = rest.split_once(' ')?;
            out.push(format!("{kind}(\"{value}\", function () {{}});"));
        } else {
            return None;
        }
    }
    Some(out)
}

fn observed_ids(text: &str) -> Option<Vec<String>> {
    let marker = "JavaScript/Electron observation\n";
    let tail = text.split_once(marker)?.1;
    let mut ids = Vec::new();
    let mut section = "";
    for line in tail.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with("Module graph: ") && line.ends_with(" edges.") {
            section = "graph";
            continue;
        }
        if line.starts_with("Unknowns: ") {
            section = "";
            continue;
        }
        if line.starts_with("String census: ") && line.ends_with(" sites.") {
            section = "census";
            continue;
        }
        if line.starts_with("Source maps:") {
            section = "";
            continue;
        }
        if section == "graph" {
            let (form, rest) = line.split_once(" at byte ")?;
            let tail = rest.split_once(' ')?.1;
            if tail == "local form, no external specifier." {
                ids.push(format!("edge {form} local"));
            } else if let Some(value) = tail.strip_prefix("specifier \"").and_then(|value| value.strip_suffix("\".")) {
                ids.push(format!("edge {form} {value}"));
            } else {
                return None;
            }
        } else if section == "census" {
            let (kind, rest) = line.split_once(" at byte ")?;
            let tail = rest.split_once(' ')?.1;
            if let Some(value) = tail.strip_prefix("literal \"").and_then(|value| value.strip_suffix("\".")) {
                ids.push(format!("site {kind} {value}"));
            } else {
                return None;
            }
        }
    }
    Some(ids)
}

fn spec_count(report: &str) -> Option<usize> {
    let marker = "Spec items: ";
    let suffix = ". Generated file: feature-rebuild.js";
    let mut offset = 0;
    while let Some(relative) = report[offset..].find(marker) {
        let start = offset + relative + marker.len();
        let rest = &report[start..];
        let digits = rest.bytes().take_while(|byte| byte.is_ascii_digit()).count();
        if digits > 0 && rest[digits..].starts_with(suffix) {
            return rest[..digits].parse().ok();
        }
        offset = start;
    }
    None
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
        Self(twin::create_scratch(root, "product-flow"))
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

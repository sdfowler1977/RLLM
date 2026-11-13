mod twin;

use std::path::{Path, PathBuf};
use std::process::Command;
use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner connected checks: original assembled/linked PE, LLVM metadata comparison, guarded file inspection, callbacks, retained queries, native views, export/readback, cited behaviour documents, clean-room implementation, saved-capture rows on the plain observations route, saved-capture rows read from workspace har and saz files, and saved-capture member, section, and file-size boundaries read from workspace files.";
const ASM: &str = ".text\n.global entry\nentry:\n mov x0,#7\n ret\n.data\n.global retained_pointer\nretained_pointer:\n .quad entry\n";
const POISON_ASM: &str = ".text\n.global entry\nentry:\n mov x0,#7\n ret\n.data\n.ascii \"The subject was e\"\n";
const PHRASE: &[u8] = b"The subject was e";
const MODE_REFUSAL: &str = "Choose file reconstruction, retained representation, structural specification or cited behavioural specification";
const DISPATCH_REFUSAL: &str = "Local execution arguments, workspace or current user rules differ from the approved dispatch";
const FILE_LIMIT: usize = 5 * 1024 * 1024;

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&project, "pe-declarations");
    let outside = scratch
        .parent()
        .unwrap_or_else(|| panic!("scratch has no parent"))
        .join("pe-declarations-outside.txt");
    let guard = Cleanup {
        scratch: scratch.clone(),
        outside: outside.clone(),
    };
    let passed = run(&project, &scratch, &outside);
    drop(guard);
    if passed.len() != 43 {
        panic!("emitted {} checks, expected 43", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("pe_declarations", &body, true);
    let printed = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
        ("count".into(), twin::n(&passed.len().to_string())),
    ]);
    println!("{}", twin::pretty(&printed));
}

struct Cleanup {
    scratch: PathBuf,
    outside: PathBuf,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.outside.exists() {
            let _ = std::fs::remove_file(&self.outside);
        }
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn run(project: &Path, scratch: &Path, outside: &Path) -> Vec<String> {
    let root = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project path is not UTF-8"));
    must_write(scratch.join("original.S"), ASM.as_bytes());
    let ndk = PathBuf::from(std::env::var("CLIENT_NDK").unwrap_or_else(|_| panic!("CLIENT_NDK is not set")))
        .join("toolchains/llvm/prebuilt/linux-x86_64/bin");
    run_checked(
        Command::new(ndk.join("clang"))
            .arg("--target=aarch64-pc-windows-msvc")
            .arg("-c")
            .arg(scratch.join("original.S"))
            .arg("-o")
            .arg(scratch.join("original.obj"))
            .env("TMPDIR", root),
        "clang",
    );
    run_checked(
        Command::new(ndk.join("lld-link"))
            .arg("/entry:entry")
            .arg("/subsystem:console")
            .arg("/nodefaultlib")
            .arg("/machine:arm64")
            .arg(format!("/out:{}", scratch.join("original.exe").display()))
            .arg(scratch.join("original.obj"))
            .env("TMPDIR", root),
        "lld-link",
    );
    let ref_text = command_stdout(
        Command::new(ndk.join("llvm-readobj"))
            .arg("--coff-basereloc")
            .arg(scratch.join("original.exe")),
        "llvm-readobj",
    );
    let addresses = dir64_addresses(&ref_text);
    let mut passed = Vec::new();
    keep(
        &mut passed,
        "independent linked PE contains an actual DIR64 base relocation",
        addresses.len() == 1,
    );
    let address = addresses[0];
    let mut controller = Controller::spawn(project_text);
    let mut turn = event(&mut controller, root, None, "resume", empty());
    let mut st = state_of(&turn).clone();
    put(
        &mut st,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://model.test/v1/chat/completions")),
            ("model".into(), s("controlled")),
            ("protocol".into(), s("chat")),
            ("stream".into(), twin::b(false)),
            ("plain".into(), twin::b(true)),
        ])]),
    );
    put(
        &mut st,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("inspect,list,read,observations,reconstruct,write")),
        ])]),
    );
    turn = event(
        &mut controller,
        root,
        Some(&st),
        "send",
        obj(vec![("draft".into(), s("Investigate original.exe using the existing command process."))]),
    );
    keep(
        &mut passed,
        "ordinary command uses intrinsic investigation without a separate skill",
        wire_of(&turn).contains("Investigation is intrinsic"),
    );
    turn = reply(
        &mut controller,
        root,
        &turn,
        &tool_json("inspect", obj(vec![("path".into(), s("original.exe"))])),
    );
    keep(
        &mut passed,
        "normal binary inspection uses the existing approval path",
        str_is(state_of(&turn), "dialog", "approval"),
    );
    turn = act(&mut controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    let out_text = as_string(&out);
    let parsed = Value::parse(&out_text);
    let graph = dig(&parsed, &["snapshots", "0"]).clone();
    let decl = index(&graph, "declarations").clone();
    let rows = dig(&decl, &["relocations", "rows"]).clone();
    let target = rows
        .arr()
        .iter()
        .find(|row| num_is(index(row, "code"), 10))
        .cloned()
        .unwrap_or_else(|| panic!("no DIR64 relocation row"));
    keep(
        &mut passed,
        "typed source declarations agree with independent relocation coordinates",
        num_is(index(&target, "target_rva"), address as i128)
            && str_is(&target, "name", "DIR64")
            && str_is(&target, "format", "PE base relocation"),
    );
    let original = std::fs::read(scratch.join("original.exe")).unwrap_or_else(|error| panic!("original.exe: {error}"));
    let file_offset = must_num(index(&target, "file_offset"), "file_offset");
    if file_offset < 0 || file_offset as usize + 2 > original.len() {
        panic!("file_offset {file_offset} is outside original.exe");
    }
    let word = u16::from_le_bytes([original[file_offset as usize], original[file_offset as usize + 1]]) as i128;
    keep(
        &mut passed,
        "typed record retains the actual held source word location",
        num_is(index(&target, "code"), word >> 12)
            && num_is(index(&target, "offset"), word & 4095)
            && must_num(index(&target, "page_rva"), "page_rva") + must_num(index(&target, "offset"), "offset")
                == must_num(index(&target, "target_rva"), "target_rva"),
    );
    let offset_value = index(&target, "target_file_offset");
    let offset_known = !matches!(offset_value, Value::Null);
    keep(
        &mut passed,
        "mapping reports only captured field bytes without adding an image base",
        num_is(index(&target, "target_field_bytes"), 8)
            && offset_known
            && must_num(offset_value, "target_file_offset") + 8 <= original.len() as i128
            && must_num(index(&target, "target_rva"), "target_rva") < 0x1_4000_0000,
    );
    keep(
        &mut passed,
        "declared inventory and unsupported classifications stay distinct",
        matches!(index(&decl, "relocation_inventory_complete"), Value::Bool(true))
            && matches!(index(&decl, "classification_inventory_complete"), Value::Null)
            && num_is(dig(&decl, &["relocations", "observed"]), rows.arr().len() as i128),
    );
    let padding_ok = rows.arr().iter().filter(|row| num_is(index(row, "code"), 0)).all(|row| {
        matches!(index(row, "target_rva"), Value::Null)
            && matches!(index(row, "target_field_bytes"), Value::Null)
            && matches!(index(row, "target_file_offset"), Value::Null)
    });
    keep(&mut passed, "padding records never become loaded targets", padding_ok);
    keep(
        &mut passed,
        "observations never claim applied relocations or loaded values",
        out_text.contains("Observation mode: held-source declarations.")
            && index(&target, "meaning").str_().contains("Coordinates are source RVAs"),
    );
    turn = settle(&mut controller, root, &turn, &effect, out.clone());
    let record = last_item(dig(state_of(&turn), &["investigation_records", "items"]));
    let record_id = index(&record, "id").clone();
    keep(
        &mut passed,
        "actual owned callback retains exact typed declarations",
        py_eq(dig(&record, &["snapshots", "0", "declarations"]), &decl),
    );
    let continuation = wire_of(&turn);
    keep(
        &mut passed,
        "ordinary model continuation receives the actual relocation evidence",
        continuation.contains("base-relocation") && continuation.contains("DIR64"),
    );
    turn = reply(
        &mut controller,
        root,
        &turn,
        &tool_json(
            "observations",
            obj(vec![
                ("id".into(), record_id.clone()),
                ("snapshot".into(), twin::n("0")),
                ("node".into(), twin::n("0")),
            ]),
        ),
    );
    turn = act(&mut controller, root, &turn, "approve");
    let query_wire = wire_of(&turn).replace("\\\"", "\"");
    keep(
        &mut passed,
        "ordinary retained query uses the same source declaration data",
        wire_of(&turn).contains("PE base relocation") && query_wire.contains("\"target_rva\""),
    );
    turn = reply(
        &mut controller,
        root,
        &turn,
        "Answer from actual declared relocation data, without executing the image.",
    );
    assert_settled(&turn);
    turn = event(
        &mut controller,
        root,
        Some(state_of(&turn)),
        &format!("investigation_record:{}", id_text(&record_id)),
        empty(),
    );
    turn = event(&mut controller, root, Some(state_of(&turn)), "investigation_graph", empty());
    turn = event(
        &mut controller,
        root,
        Some(state_of(&turn)),
        "investigation_graph_node:0",
        empty(),
    );
    keep(
        &mut passed,
        "native evidence view uses actual retained PE data",
        index(&turn, "ui").to_string().contains("DIR64"),
    );
    turn = event(&mut controller, root, Some(state_of(&turn)), "reconstruction_new", empty());
    turn = event(
        &mut controller,
        root,
        Some(state_of(&turn)),
        "reconstruction_prepare",
        prepare_inputs(&record_id, "specification", "pe-structure.json"),
    );
    keep(
        &mut passed,
        "structural export enters the existing authority review",
        str_is(state_of(&turn), "dialog", "approval"),
    );
    turn = act(&mut controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    let out_text = as_string(&out);
    let export = Value::parse(&read_text(&scratch.join("pe-structure.json")));
    keep(
        &mut passed,
        "actual structural export preserves exact typed PE declarations",
        py_eq(dig(&export, &["source", "structure", "declarations"]), &decl) && out_text.contains("read back exactly"),
    );
    turn = settle(&mut controller, root, &turn, &effect, out);
    turn = drive(&mut controller, root, state_of(&turn), &record_id, "representation", "pe-report.txt");
    turn = act(&mut controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    let out_text = as_string(&out);
    let report = std::fs::read(scratch.join("pe-report.txt")).unwrap_or_else(|error| panic!("pe-report.txt: {error}"));
    let rep = dig(&graph, &["nodes", "0", "representation"]);
    keep(
        &mut passed,
        "retained source report exports exact hashed representation bytes",
        report.len() as i128 == must_num(index(rep, "bytes"), "bytes")
            && twin::sha256_bytes(&report) == index(rep, "sha256").str_()
            && out_text.contains("read back exactly"),
    );
    turn = settle(&mut controller, root, &turn, &effect, out);
    let mut tampered = state_of(&turn).clone();
    {
        let row = descend(
            record_mut(&mut tampered, &record_id),
            &["snapshots", "0", "declarations", "relocations", "rows", "0"],
        );
        let current = index(row, "target_rva").clone();
        put(row, "target_rva", num_add(&current, 4));
    }
    let denied = event(
        &mut controller,
        root,
        Some(&tampered),
        &format!("investigation_record:{}", id_text(&record_id)),
        empty(),
    );
    let denied = event(&mut controller, root, Some(state_of(&denied)), "investigation_graph", empty());
    let denied_state = state_of(&denied);
    keep(
        &mut passed,
        "altered typed declaration is not adopted as historical evidence",
        twin::truthy(index(denied_state, "notice_error"))
            && !matches!(index(denied_state, "dialog"), Value::Str(text) if text == "investigation_graph"),
    );
    let plain_tool = obj(vec![
        ("op".into(), s("tool")),
        ("tool".into(), s("inspect")),
        ("root".into(), s(root)),
        ("args".into(), obj(vec![("path".into(), s("original.S"))])),
    ]);
    keep(
        &mut passed,
        "ordinary file work remains available after optional history corruption",
        matches!(controller.op(&plain_tool), Value::Str(_)),
    );
    turn = drive(&mut controller, root, state_of(&turn), &record_id, "behaviour", "pe-behaviour.json");
    assert_dialog(&turn, "approval");
    turn = act(&mut controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    let out_text = as_string(&out);
    let behaviour = Value::parse(&read_text(&scratch.join("pe-behaviour.json")));
    let claims = index(&behaviour, "claims").clone();
    let unknown = index(&behaviour, "unknown").clone();
    let digest = falsy_string(&behaviour, "digest");
    let citations_ok = claims.arr().iter().all(|claim| {
        matches!(claim.get("citation"), Some(Value::Obj(_)))
            && index(claim, "citation").get("record").is_some_and(twin::truthy)
            && get_present(index(claim, "citation"), "snapshot")
            && get_present(index(claim, "citation"), "node")
    });
    keep(
        &mut passed,
        "cited behaviour document carries citations and a 64-hex self digest",
        get_str(&behaviour, "type") == Some("clean_room_behaviour")
            && num_is_opt(behaviour.get("schema"), 1)
            && is_hex64(&digest)
            && digest == seal(&behaviour)
            && twin::truthy(&claims)
            && citations_ok
            && twin::truthy(&unknown)
            && out_text.contains("cited clean-room behavioural specification")
            && out_text.contains("read back exactly"),
    );
    turn = settle(&mut controller, root, &turn, &effect, out);
    turn = drive(&mut controller, root, state_of(&turn), &record_id, "implement", "pe-implement.txt");
    assert_dialog(&turn, "approval");
    turn = act(&mut controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    let out_text = as_string(&out);
    let artifact = read_text(&scratch.join("pe-implement.txt"));
    let (provenance, units, refusals) = markers(&artifact);
    let citation_list = Value::Arr(claims.arr().iter().map(|claim| index(claim, "citation").clone()).collect());
    keep(
        &mut passed,
        "clean-room artifact provenance names the behaviour document digest",
        provenance.as_ref().is_some_and(|item| get_str(item, "behaviour_digest") == Some(digest.as_str()))
            && provenance.as_ref().is_some_and(|item| py_eq_get(item.get("citations"), Some(&citation_list)))
            && out_text.contains("clean-room implementation generated from cited behavioural claims")
            && out_text.contains("read back exactly"),
    );
    let provenance = provenance.unwrap_or_else(|| panic!("clean-room artifact provenance names the behaviour document digest"));
    let units_match = units.len() == claims.arr().len()
        && refusals.len() == unknown.arr().len()
        && units.iter().zip(claims.arr()).all(|(unit, claim)| {
            get_str(unit, "kind") == Some("behaviour")
                && py_eq_get(unit.get("statement"), claim.get("statement"))
                && py_eq_get(unit.get("citation"), claim.get("citation"))
        })
        && refusals.iter().zip(unknown.arr()).all(|(item, row)| {
            get_str(item, "marker") == Some("refused")
                && py_eq_get(item.get("statement"), row.get("statement"))
                && py_eq_get(item.get("reason"), row.get("reason"))
        });
    keep(
        &mut passed,
        "artifact unit and refused lines match cited claims and unknown items",
        units_match,
    );
    let (matched, missing, conflicting) = verdicts(&behaviour, &provenance, &units, &refusals);
    let receipt = format!("Verification matched {matched}, missing {missing}, conflicting {conflicting}.");
    keep(
        &mut passed,
        "implement receipt names the same verify counts as the artifact",
        matched == claims.arr().len() && missing == 0 && conflicting == 0 && out_text.contains(&receipt),
    );
    turn = settle(&mut controller, root, &turn, &effect, out);
    for (mode, name) in [("verify", "pe-verify-mode.txt"), ("behavior", "pe-behavior-mode.txt")] {
        let opened = drive(&mut controller, root, state_of(&turn), &record_id, mode, name);
        let opened_state = state_of(&opened);
        keep(
            &mut passed,
            &format!("mode {mode} on the behaviour document is still refused"),
            opened_state.get("notice_error").is_some_and(twin::truthy)
                && get_str(opened_state, "notice") == Some(MODE_REFUSAL)
                && !scratch.join(name).exists(),
        );
        turn = obj(vec![("state".into(), opened_state.clone())]);
    }
    let opened = drive(
        &mut controller,
        root,
        state_of(&turn),
        &record_id,
        "implement",
        &format!("../{}", outside.file_name().and_then(|text| text.to_str()).unwrap_or("pe-declarations-outside.txt")),
    );
    let opened_state = state_of(&opened);
    let outside_notice = get_str(opened_state, "notice");
    keep(
        &mut passed,
        "output path outside the workspace is refused and the artifact is absent",
        opened_state.get("notice_error").is_some_and(twin::truthy)
            && matches!(outside_notice, Some("Choose a new workspace-root output filename" | "Implementation artifact path is outside the workspace"))
            && !outside.exists()
            && !scratch.join(outside.file_name().unwrap_or_default()).exists(),
    );
    turn = obj(vec![("state".into(), opened_state.clone())]);
    turn = drive(&mut controller, root, state_of(&turn), &record_id, "implement", "pe-digest-mismatch.txt");
    assert_dialog(&turn, "approval");
    let approved = act(&mut controller, root, &turn, "approve");
    let effect = index(&approved, "effect").clone();
    let mut tampered = effect.clone();
    {
        let statement = descend(
            &mut tampered,
            &["reconstruction_source", "behaviour", "document", "claims", "0", "statement"],
        );
        let Value::Str(text) = statement else {
            panic!("statement is not a string: {}", statement.to_string());
        };
        *text = format!("{text} digest mismatch");
    }
    let answer = controller.send(&tool_call(&tampered, root));
    let mismatch_path = scratch.join("pe-digest-mismatch.txt");
    let absent = !mismatch_path.exists();
    let (cleared_state, cleared) = refused_dispatch(&mut controller, root, state_of(&approved), &effect, &answer);
    keep(
        &mut passed,
        "digest-mismatched document is refused and the artifact is absent",
        error_in(&answer, &[DISPATCH_REFUSAL, "Behaviour document digest does not match the handed document"])
            && absent
            && !mismatch_path.exists()
            && cleared,
    );
    turn = obj(vec![("state".into(), cleared_state)]);
    turn = drive(&mut controller, root, state_of(&turn), &record_id, "implement", "pe-citation-missing.txt");
    assert_dialog(&turn, "approval");
    let approved = act(&mut controller, root, &turn, "approve");
    let effect = index(&approved, "effect").clone();
    let mut tampered = effect.clone();
    {
        let document = descend(&mut tampered, &["reconstruction_source", "behaviour", "document"]);
        let removed = {
            let claim = descend(document, &["claims", "0"]);
            pop(claim, "citation")
        };
        if !twin::truthy(&removed) {
            panic!("citation was empty");
        }
        let sealed = seal(document);
        put(document, "digest", s(&sealed));
    }
    let answer = controller.send(&tool_call(&tampered, root));
    let citation_path = scratch.join("pe-citation-missing.txt");
    let absent = !citation_path.exists();
    let (cleared_state, cleared) = refused_dispatch(&mut controller, root, state_of(&approved), &effect, &answer);
    let citation_error = error_or_empty(&answer);
    keep(
        &mut passed,
        "citation-less claim is refused and the artifact is absent",
        (citation_error == DISPATCH_REFUSAL || citation_error.starts_with("Cited claim has no citation:"))
            && absent
            && !citation_path.exists()
            && cleared,
    );
    turn = obj(vec![("state".into(), cleared_state)]);
    if PHRASE.len() != 17 {
        panic!("phrase length");
    }
    must_write(scratch.join("poison.S"), POISON_ASM.as_bytes());
    run_checked(
        Command::new(ndk.join("clang"))
            .arg("--target=aarch64-pc-windows-msvc")
            .arg("-c")
            .arg(scratch.join("poison.S"))
            .arg("-o")
            .arg(scratch.join("poison.obj"))
            .env("TMPDIR", root),
        "clang poison",
    );
    run_checked(
        Command::new(ndk.join("lld-link"))
            .arg("/entry:entry")
            .arg("/subsystem:console")
            .arg("/nodefaultlib")
            .arg("/machine:arm64")
            .arg(format!("/out:{}", scratch.join("poison.exe").display()))
            .arg(scratch.join("poison.obj"))
            .env("TMPDIR", root),
        "lld-link poison",
    );
    let poison_bytes = std::fs::read(scratch.join("poison.exe")).unwrap_or_else(|error| panic!("poison.exe: {error}"));
    if !contains_slice(&poison_bytes, PHRASE) {
        panic!("poison phrase missing");
    }
    turn = event(
        &mut controller,
        root,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("Investigate poison.exe using the existing command process."))]),
    );
    turn = reply(
        &mut controller,
        root,
        &turn,
        &tool_json("inspect", obj(vec![("path".into(), s("poison.exe"))])),
    );
    assert_dialog(&turn, "approval");
    turn = act(&mut controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    turn = settle(&mut controller, root, &turn, &effect, out);
    turn = maybe_model(&mut controller, root, turn, "Answer from the retained observation only.");
    assert_settled(&turn);
    let poison = last_item(dig(state_of(&turn), &["investigation_records", "items"]));
    if index(index(&poison, "args"), "path").str_() != "poison.exe" {
        panic!("poison record path");
    }
    let poison_id = index(&poison, "id").clone();
    let opened = drive(&mut controller, root, state_of(&turn), &poison_id, "implement", "stolen.txt");
    assert_dialog(&opened, "approval");
    let approved = act(&mut controller, root, &opened, "approve");
    let effect = index(&approved, "effect").clone();
    let answer = controller.send(&tool_call(&effect, root));
    let stolen = scratch.join("stolen.txt");
    let absent = !stolen.exists();
    let (_cleared_state, cleared) = refused_dispatch(&mut controller, root, state_of(&approved), &effect, &answer);
    keep(
        &mut passed,
        "seventeen contiguous subject bytes are refused and the artifact is absent",
        get_str(&answer, "error")
            == Some("Behavioural specification copies 17 or more contiguous subject bytes; quoted fragments are limited to 16 bytes")
            && absent
            && !stolen.exists()
            && cleared,
    );
    let pages = scratch.join("saved-pages");
    std::fs::create_dir(&pages).unwrap_or_else(|error| panic!("saved-pages: {error}"));
    must_write(pages.join("page.html"), b"<html><p>not a capture</p></html>\n");
    turn = event(
        &mut controller,
        root,
        Some(state_of(&turn)),
        "send",
        obj(vec![("draft".into(), s("Inspect saved-pages using the existing command process."))]),
    );
    turn = reply(
        &mut controller,
        root,
        &turn,
        &tool_json("inspect", obj(vec![("path".into(), s("saved-pages"))])),
    );
    assert_dialog(&turn, "approval");
    turn = act(&mut controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    turn = settle(&mut controller, root, &turn, &effect, out);
    turn = maybe_model(&mut controller, root, turn, "Answer from the retained folder observation only.");
    assert_settled(&turn);
    let folder = last_item(dig(state_of(&turn), &["investigation_records", "items"]));
    if index(index(&folder, "args"), "path").str_() != "saved-pages" || !folder.get("snapshots").is_some_and(twin::truthy) {
        panic!("{}", folder.get("graph_note").map(Value::to_string).unwrap_or_else(|| "None".into()));
    }
    let base = state_of(&turn).clone();
    let folder_id = index(&folder, "id").clone();
    let held_report = index(&folder, "report").clone();
    let valid_capture = capture_document(&[
        capture_entry("POST", "https://example.test/submit", 201, "application/json", 40),
        capture_entry("GET", "https://example.test/item", 200, "text/html", 12),
    ]);
    let shown = ask_saved(
        &mut controller,
        root,
        &with_report(&base, &folder_id, s(&valid_capture)),
        obj(vec![("id".into(), folder_id.clone())]),
    );
    let capture = shown.get("saved_capture");
    let rows_value = match capture {
        Some(Value::Obj(_)) => capture.and_then(|item| item.get("entries")),
        _ => None,
    };
    let expected_rows = Value::Arr(vec![
        entry_row(0, "POST", "https://example.test/submit", 201, "application/json", 40),
        entry_row(1, "GET", "https://example.test/item", 200, "text/html", 12),
    ]);
    keep(
        &mut passed,
        "retained capture record returns typed saved_capture rows and no refusal",
        matches!(capture, Some(Value::Obj(_)))
            && capture.is_some_and(|item| get_is_none(item, "refusal"))
            && capture.is_some_and(|item| is_empty_arr(item, "caps"))
            && rows_value.is_some_and(|item| py_eq(item, &expected_rows))
            && shown.get("report").is_some_and(|item| py_eq(item, &s(&valid_capture)))
            && !substring_in(capture.unwrap_or(&Value::Null), "report", "99999")
            && !substring_in(capture.unwrap_or(&Value::Null), "report", "text/plain"),
    );
    let page_text = "<html><p>not a capture</p></html>";
    let plain = ask_saved(&mut controller, root, &base, obj(vec![("id".into(), folder_id.clone())]));
    let paged = ask_saved(
        &mut controller,
        root,
        &with_report(&base, &folder_id, s(page_text)),
        obj(vec![("id".into(), folder_id.clone())]),
    );
    keep(
        &mut passed,
        "ordinary page text gains no saved_capture key and the other shown fields stay unchanged",
        plain.get("saved_capture").is_none()
            && paged.get("saved_capture").is_none()
            && plain.get("report").is_some_and(|item| py_eq(item, &held_report))
            && paged.get("report").is_some_and(|item| py_eq(item, &s(page_text)))
            && eq_except(&plain, &paged, "report"),
    );
    let graphed = ask_saved(
        &mut controller,
        root,
        &with_report(&base, &folder_id, s(&valid_capture)),
        obj(vec![("id".into(), folder_id.clone()), ("snapshot".into(), twin::n("0"))]),
    );
    keep(
        &mut passed,
        "snapshot query of the same capture record stays on the typed graph without saved_capture",
        graphed.get("saved_capture").is_none()
            && get_str(&graphed, "kind") == Some("directory")
            && matches!(graphed.get("nodes"), Some(Value::Arr(items)) if !items.is_empty())
            && graphed.get("record_id").is_some_and(|item| py_eq(item, &folder_id))
            && graphed.get("entries").is_none(),
    );
    let cut = "{\"log\":{\"version\":\"1.2\",\"entries\":[{\"request\":{\"method\":\"GET\",\"url\":\"https://example.test/cut\"";
    let shown = ask_saved(
        &mut controller,
        root,
        &with_report(&base, &folder_id, s(cut)),
        obj(vec![("id".into(), folder_id.clone())]),
    );
    let capture = match &shown {
        Value::Obj(_) => shown.get("saved_capture"),
        _ => None,
    };
    let refusal = capture.and_then(|item| match item {
        Value::Obj(_) => item.get("refusal"),
        _ => None,
    });
    keep(
        &mut passed,
        "truncated capture is refused by name and returns no rows",
        matches!(capture, Some(Value::Obj(_)))
            && capture.is_some_and(|item| is_empty_arr(item, "entries"))
            && matches!(refusal, Some(Value::Str(_)))
            && refusal.is_some_and(|item| item.str_().contains("truncated"))
            && refusal.is_some_and(|item| !item.str_().contains("unusable"))
            && capture.is_some_and(|item| !item.to_string().contains("https://example.test/cut")),
    );
    let broken = "{\"request\":{\"method\":\"GET\",\"url\":\"https://example.test/broken\"},\"response\":{\"content\":{\"mimeType\":\"text/plain\",\"size\":4}}}";
    let shown = ask_saved(
        &mut controller,
        root,
        &with_report(
            &base,
            &folder_id,
            s(&capture_document(&[
                capture_entry("GET", "https://example.test/ok", 200, "text/html", 8),
                broken.to_string(),
            ])),
        ),
        obj(vec![("id".into(), folder_id.clone())]),
    );
    let capture = match &shown {
        Value::Obj(_) => shown.get("saved_capture"),
        _ => None,
    };
    let refusal = capture.and_then(|item| match item {
        Value::Obj(_) => item.get("refusal"),
        _ => None,
    });
    keep(
        &mut passed,
        "malformed entry refuses the whole capture by index and drops the valid sibling",
        matches!(capture, Some(Value::Obj(_)))
            && capture.is_some_and(|item| is_empty_arr(item, "entries"))
            && matches!(refusal, Some(Value::Str(text)) if text.contains("entry 1") && text.contains("response.status") && !text.contains("entry 0"))
            && capture.is_some_and(|item| !item.to_string().contains("https://example.test/ok")),
    );
    let many = capture_document(
        &(0..65)
            .map(|i| capture_entry("GET", &format!("https://e.test/{i}"), 200, "text/plain", 1))
            .collect::<Vec<_>>(),
    );
    let shown = ask_saved(
        &mut controller,
        root,
        &with_report(&base, &folder_id, s(&many)),
        obj(vec![("id".into(), folder_id.clone())]),
    );
    let capture = match &shown {
        Value::Obj(_) => shown.get("saved_capture"),
        _ => None,
    };
    let capped_rows = capture.and_then(|item| match item {
        Value::Obj(_) => item.get("entries"),
        _ => None,
    });
    keep(
        &mut passed,
        "capture over the entry cap returns the capped rows and names the entry cap",
        matches!(capture, Some(Value::Obj(_)))
            && capture.is_some_and(|item| get_is_none(item, "refusal"))
            && matches!(capped_rows, Some(Value::Arr(items)) if items.len() == 64)
            && capped_rows.is_some_and(|item| num_is_opt(item.arr().first().and_then(|row| row.get("index")), 0))
            && capped_rows.is_some_and(|item| num_is_opt(item.arr().last().and_then(|row| row.get("index")), 63))
            && capped_rows.is_some_and(|item| item.arr().iter().all(|row| !num_is_opt(row.get("index"), 64)))
            && capture.is_some_and(|item| membership(item, "caps", "saved-capture entry cap"))
            && capture.is_some_and(|item| substring_field(item, "report", "saved-capture entry cap"))
            && capture.is_some_and(|item| !membership(item, "caps", "saved-capture report cap")),
    );
    let har = b"{\"log\":{\"version\":\"1.2\",\"entries\":[{\"request\":{\"method\":\"GET\",\"url\":\"https://example.test/from-file\"},\"response\":{\"status\":200,\"content\":{\"mimeType\":\"text/html\",\"size\":12}}}]}}";
    must_write(scratch.join("traffic.har"), har);
    let (st, hid, hout) = file_inspect(&mut controller, root, &base, "traffic.har");
    let cap = section_rows(&hout);
    let har_rows = cap.get("entries");
    let shown = ask_saved(&mut controller, root, &st, obj(vec![("id".into(), hid.clone())]));
    let lifted = shown.get("saved_capture");
    keep(
        &mut passed,
        "a workspace har file reaches typed rows through inspect and the unpatched observations route",
        get_is_none(&cap, "refusal")
            && is_empty_arr(&cap, "caps")
            && har_rows.is_some_and(|item| {
                py_eq(
                    item,
                    &Value::Arr(vec![entry_row(0, "GET", "https://example.test/from-file", 200, "text/html", 12)]),
                )
            })
            && lifted.is_some_and(|item| py_eq(item, &cap))
            && shown.get("report").is_some_and(|item| py_eq(item, &s(&hout)))
            && std::fs::read(scratch.join("traffic.har")).ok().as_deref() == Some(har.as_slice()),
    );
    let post = b"POST https://example.test/submit HTTP/1.1\r\nHost: example.test\r\n\r\n";
    let created = b"HTTP/1.1 201 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":1}";
    let getb = b"GET https://example.test/item HTTP/1.1\r\nHost: example.test\r\n\r\n";
    let page = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\nhello";
    let saz = zip_store(
        &[
            ("index.dat".into(), b"session".to_vec()),
            ("raw/2_c.txt".into(), post.to_vec()),
            ("raw/2_s.txt".into(), created.to_vec()),
            ("raw/10_c.txt".into(), getb.to_vec()),
            ("raw/10_s.txt".into(), page.to_vec()),
        ],
        None,
    );
    must_write(scratch.join("traffic.saz"), &saz);
    let (st, sid, sout) = file_inspect(&mut controller, root, &st, "traffic.saz");
    let cap = section_rows(&sout);
    let saz_rows = cap.get("entries");
    let shown = ask_saved(&mut controller, root, &st, obj(vec![("id".into(), sid)]));
    let lifted = shown.get("saved_capture");
    keep(
        &mut passed,
        "a workspace saz whose header is not utf-8 reaches typed session rows through inspect and the unpatched observations route",
        saz.get(11).is_some_and(|byte| *byte >= 0x80)
            && utf8_ignore(&saz) != saz
            && get_is_none(&cap, "refusal")
            && is_empty_arr(&cap, "caps")
            && saz_rows.is_some_and(|item| {
                py_eq(
                    item,
                    &Value::Arr(vec![
                        entry_row(2, "POST", "https://example.test/submit", 201, "application/json", 8),
                        entry_row(10, "GET", "https://example.test/item", 200, "text/html", 5),
                    ]),
                )
            })
            && lifted.is_some_and(|item| py_eq(item, &cap))
            && shown.get("report").is_some_and(|item| py_eq(item, &s(&sout))),
    );
    must_write(scratch.join("notes.har"), b"hello from a plain file\n");
    let (st, nid, nout) = file_inspect(&mut controller, root, &st, "notes.har");
    let cap = section_rows(&nout);
    let shown = ask_saved(&mut controller, root, &st, obj(vec![("id".into(), nid)]));
    keep(
        &mut passed,
        "a plain-text file named har is refused by name from the bytes on disk",
        is_empty_arr(&cap, "entries")
            && get_str(&cap, "refusal") == Some("Refusal: file is not a saved capture.")
            && shown.get("saved_capture").is_some_and(|item| py_eq(item, &cap))
            && read_text(&scratch.join("notes.har")) == "hello from a plain file\n",
    );
    let m320 = capture_members(320, b"ok");
    let (st, cap, shown) = saved_disk(&mut controller, root, scratch, &st, "cap320.saz", &zip_store(&m320, None));
    keep(
        &mut passed,
        "saved capture of exactly 320 members returns the session row and names no member cap",
        m320.len() == 320
            && get_is_none(&cap, "refusal")
            && is_empty_arr(&cap, "caps")
            && cap.get("entries").is_some_and(|item| py_eq(item, &Value::Arr(vec![entry_row(1, "GET", "https://e.test/cap", 200, "text/plain", 2)])))
            && shown.get("saved_capture").is_some_and(|item| py_eq(item, &cap)),
    );
    let m321 = capture_members(321, b"ok");
    let (st, cap, shown) = saved_disk(&mut controller, root, scratch, &st, "cap321.saz", &zip_store(&m321, None));
    keep(
        &mut passed,
        "saved capture of 321 members is refused by the member cap and returns no rows",
        m321.len() == 321
            && is_empty_arr(&cap, "entries")
            && is_empty_arr(&cap, "caps")
            && get_str(&cap, "refusal") == Some("Refusal: saved-capture member cap.")
            && shown.get("saved_capture").is_some_and(|item| py_eq(item, &cap))
            && !cap.to_string().contains("https://e.test/cap"),
    );
    let head = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n";
    let body = vec![b'B'; 64 * 1024 - head.len()];
    let sect = capture_members(3, &body);
    let (st, cap, shown) = saved_disk(&mut controller, root, scratch, &st, "byte64.saz", &zip_store(&sect, None));
    keep(
        &mut passed,
        "a response section of exactly 65536 bytes returns that body size and the member byte cap stays silent",
        sect[2].1.len() == 64 * 1024
            && get_is_none(&cap, "refusal")
            && is_empty_arr(&cap, "caps")
            && cap.get("entries").is_some_and(|item| {
                py_eq(item, &Value::Arr(vec![entry_row(1, "GET", "https://e.test/cap", 200, "text/plain", body.len() as i128)]))
            })
            && shown.get("saved_capture").is_some_and(|item| py_eq(item, &cap)),
    );
    let mut over_body = body.clone();
    over_body.push(b'B');
    let overm = capture_members(3, &over_body);
    let (st, cap, shown) = saved_disk(&mut controller, root, scratch, &st, "byte65.saz", &zip_store(&overm, None));
    keep(
        &mut passed,
        "a response section of 65537 bytes is refused by the member byte cap and drops the session row",
        overm[2].1.len() == 64 * 1024 + 1
            && is_empty_arr(&cap, "entries")
            && is_empty_arr(&cap, "caps")
            && get_str(&cap, "refusal") == Some("Refusal: saved-capture member byte cap.")
            && shown.get("saved_capture").is_some_and(|item| py_eq(item, &cap))
            && !cap.to_string().contains("https://e.test/cap"),
    );
    let wide = wide_members();
    let exact = zip_store(&wide, Some(FILE_LIMIT));
    let (st, cap, shown) = saved_disk(&mut controller, root, scratch, &st, "file5.saz", &exact);
    let wide_bytes: usize = wide.iter().map(|(_, data)| data.len()).sum();
    let wide_max = wide.iter().map(|(_, data)| data.len()).max().unwrap_or(0);
    keep(
        &mut passed,
        "a saved capture of exactly 5242880 bytes returns the session row and is inside the reader limit",
        exact.len() == FILE_LIMIT
            && wide.len() <= 320
            && wide_max <= 64 * 1024
            && wide_bytes <= 4 * 1024 * 1024
            && get_is_none(&cap, "refusal")
            && is_empty_arr(&cap, "caps")
            && cap.get("entries").is_some_and(|item| py_eq(item, &Value::Arr(vec![entry_row(1, "GET", "https://e.test/wide", 200, "text/plain", 2)])))
            && shown.get("saved_capture").is_some_and(|item| py_eq(item, &cap)),
    );
    let mut plus = exact;
    plus.push(b'Z');
    let (_st, cap, shown) = saved_disk(&mut controller, root, scratch, &st, "file5plus.saz", &plus);
    keep(
        &mut passed,
        "a saved capture of 5242881 bytes is refused by the reader limit and returns no rows",
        file_len(&scratch.join("file5plus.saz")) == (FILE_LIMIT as u64) + 1
            && is_empty_arr(&cap, "entries")
            && is_empty_arr(&cap, "caps")
            && get_str(&cap, "refusal") == Some("Refusal: saved capture file exceeds the reader limit.")
            && shown.get("saved_capture").is_some_and(|item| py_eq(item, &cap)),
    );
    controller.finish();
    passed
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn empty() -> Value {
    obj(vec![])
}

fn event(controller: &mut Controller, root: &str, state: Option<&Value>, action: &str, inputs: Value) -> Value {
    let inputs = if twin::truthy(&inputs) { inputs } else { empty() };
    controller.event(state, action, inputs, root)
}

fn act(controller: &mut Controller, root: &str, data: &Value, action: &str) -> Value {
    let state = state_of(data).clone();
    event(controller, root, Some(&state), action, empty())
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn index<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key} in {}", value.to_string()))
}

fn index_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    match value {
        Value::Obj(fields) => {
            let pos = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
            &mut fields[pos].1
        }
        Value::Arr(items) => {
            let pos = key.parse::<usize>().unwrap_or_else(|_| panic!("array index expected, got {key}"));
            items.get_mut(pos).unwrap_or_else(|| panic!("index {key} out of range"))
        }
        other => panic!("expected container at {key}, got {}", other.to_string()),
    }
}

fn descend<'a>(value: &'a mut Value, path: &[&str]) -> &'a mut Value {
    if path.is_empty() {
        value
    } else {
        descend(index_mut(value, path[0]), &path[1..])
    }
}

fn dig<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    let mut cursor = value;
    for key in path {
        cursor = match cursor {
            Value::Arr(items) => {
                let pos = key.parse::<usize>().unwrap_or_else(|_| panic!("array index expected, got {key}"));
                items.get(pos).unwrap_or_else(|| panic!("index {key} out of range"))
            }
            Value::Obj(_) => index(cursor, key),
            other => panic!("expected container at {key}, got {}", other.to_string()),
        };
    }
    cursor
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object, got {}", value.to_string());
    };
    if let Some(pos) = fields.iter().position(|(name, _)| name == key) {
        fields[pos].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn pop(value: &mut Value, key: &str) -> Value {
    let Value::Obj(fields) = value else {
        panic!("expected object, got {}", value.to_string());
    };
    let pos = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
    fields.remove(pos).1
}

fn record_mut<'a>(state: &'a mut Value, id: &Value) -> &'a mut Value {
    let items = index_mut(index_mut(state, "investigation_records"), "items");
    let Value::Arr(list) = items else {
        panic!("investigation items are not a list");
    };
    let pos = list
        .iter()
        .position(|item| py_eq(index(item, "id"), id))
        .unwrap_or_else(|| panic!("record {} is missing", id.to_string()));
    &mut list[pos]
}

fn last_item(value: &Value) -> Value {
    value.arr().last().cloned().unwrap_or_else(|| panic!("items are empty: {}", value.to_string()))
}

fn id_text(value: &Value) -> String {
    match value {
        Value::Str(text) => text.clone(),
        other => panic!("record id is not a string: {}", other.to_string()),
    }
}

fn as_string(value: &Value) -> String {
    match value {
        Value::Str(text) => text.clone(),
        other => panic!("expected string, got {}", other.to_string()),
    }
}

fn str_is(value: &Value, key: &str, expected: &str) -> bool {
    matches!(index(value, key), Value::Str(text) if text == expected)
}

fn get_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value.get(key) {
        Some(Value::Str(text)) => Some(text),
        _ => None,
    }
}

fn get_is_none(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn get_present(value: &Value, key: &str) -> bool {
    !matches!(value.get(key), None | Some(Value::Null))
}

fn is_empty_arr(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Arr(items)) if items.is_empty())
}

fn falsy_string(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        Some(other) if twin::truthy(other) => panic!("{key} is not a string: {}", other.to_string()),
        _ => String::new(),
    }
}

fn num_i128(value: &Value) -> Option<i128> {
    match value {
        Value::Num(text) => parse_i128(text),
        Value::Bool(true) => Some(1),
        Value::Bool(false) => Some(0),
        _ => None,
    }
}

fn parse_i128(text: &str) -> Option<i128> {
    if let Ok(value) = text.parse::<i128>() {
        if !text.contains(['.', 'e', 'E']) {
            return Some(value);
        }
    }
    let float: f64 = text.parse().ok()?;
    if !float.is_finite() || float.fract() != 0.0 {
        return None;
    }
    let value = float as i128;
    (value as f64 == float).then_some(value)
}

fn num_is(value: &Value, expected: i128) -> bool {
    num_i128(value) == Some(expected)
}

fn num_is_opt(value: Option<&Value>, expected: i128) -> bool {
    value.is_some_and(|item| num_is(item, expected))
}

fn must_num(value: &Value, name: &str) -> i128 {
    num_i128(value).unwrap_or_else(|| panic!("{name} is not a number: {}", value.to_string()))
}

fn num_add(value: &Value, delta: i64) -> Value {
    let Value::Num(text) = value else {
        panic!("expected number, got {}", value.to_string());
    };
    if !text.contains(['.', 'e', 'E']) {
        let parsed = text.parse::<i128>().unwrap_or_else(|_| panic!("bad number {text}"));
        return twin::n(&(parsed + i128::from(delta)).to_string());
    }
    let float = text.parse::<f64>().unwrap_or_else(|_| panic!("bad number {text}"));
    let sum = float + delta as f64;
    if sum.fract() == 0.0 && sum.abs() < i64::MAX as f64 {
        twin::n(&(sum as i64).to_string())
    } else {
        panic!("non-integral number {text}");
    }
}

fn py_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Num(_), Value::Num(_)) | (Value::Bool(_), Value::Num(_)) | (Value::Num(_), Value::Bool(_)) => {
            match (num_i128(left), num_i128(right)) {
                (Some(a), Some(b)) => a == b,
                _ => match (left, right) {
                    (Value::Num(a), Value::Num(b)) => a.parse::<f64>().ok() == b.parse::<f64>().ok() && a.parse::<f64>().is_ok(),
                    _ => false,
                },
            }
        }
        (Value::Arr(a), Value::Arr(b)) => a.len() == b.len() && a.iter().zip(b).all(|(left, right)| py_eq(left, right)),
        (Value::Obj(a), Value::Obj(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| b.iter().any(|(other, other_value)| key == other && py_eq(value, other_value)))
                && b.iter().all(|(key, value)| a.iter().any(|(other, other_value)| key == other && py_eq(value, other_value)))
        }
        _ => false,
    }
}

fn py_eq_get(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (none_if_null(left), none_if_null(right)) {
        (None, None) => true,
        (Some(left), Some(right)) => py_eq(left, right),
        _ => false,
    }
}

fn none_if_null(value: Option<&Value>) -> Option<&Value> {
    match value {
        None | Some(Value::Null) => None,
        Some(other) => Some(other),
    }
}

fn eq_except(left: &Value, right: &Value, skip: &str) -> bool {
    let filter = |value: &Value| {
        let Value::Obj(fields) = value else {
            panic!("expected object, got {}", value.to_string());
        };
        Value::Obj(fields.iter().filter(|(key, _)| key != skip).cloned().collect())
    };
    py_eq(&filter(left), &filter(right))
}

fn wire_of(data: &Value) -> String {
    match index(index(data, "effect"), "wire") {
        Value::Str(text) => text.clone(),
        other => panic!("wire is not a string: {}", other.to_string()),
    }
}

fn reply(controller: &mut Controller, root: &str, data: &Value, text: &str) -> Value {
    let state = state_of(data).clone();
    let body = obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )]);
    let encoded = body.to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{encoded}", encoded.len());
    let ticket = index(&state, "model_expected").clone();
    event(
        controller,
        root,
        Some(&state),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
    )
}

fn tool_json(tool: &str, args: Value) -> String {
    obj(vec![("tool".into(), s(tool)), ("args".into(), args)]).to_string()
}

fn tool_call(effect: &Value, root: &str) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("effect is not an object: {}", effect.to_string());
    };
    let mut envelope = Value::Obj(fields.clone());
    put(&mut envelope, "op", s("tool"));
    put(&mut envelope, "root", s(root));
    envelope
}

fn settle(controller: &mut Controller, root: &str, data: &Value, effect: &Value, output: Value) -> Value {
    let mut fields = Vec::new();
    for key in ["purpose", "tool_ticket", "tool_binding", "child_ticket", "child_owner"] {
        if let Some(value) = effect.get(key) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    fields.push(("output".into(), output));
    let state = state_of(data).clone();
    event(controller, root, Some(&state), "tool_result", Value::Obj(fields))
}

fn assert_dialog(data: &Value, expected: &str) {
    let state = state_of(data);
    if matches!(state.get("dialog"), Some(Value::Str(text)) if text == expected) {
        return;
    }
    panic!("{}", state.get("notice").map(Value::to_string).unwrap_or_else(|| "None".into()));
}

fn assert_settled(data: &Value) {
    let state = state_of(data);
    if !twin::truthy(index(state, "busy")) {
        return;
    }
    panic!("{}", state.get("notice").map(Value::to_string).unwrap_or_else(|| "None".into()));
}

fn model_effect(data: &Value) -> bool {
    match data.get("effect") {
        Some(effect) if twin::truthy(effect) => matches!(effect.get("purpose"), Some(Value::Str(text)) if text == "model"),
        _ => false,
    }
}

fn maybe_model(controller: &mut Controller, root: &str, data: Value, text: &str) -> Value {
    if model_effect(&data) {
        reply(controller, root, &data, text)
    } else {
        data
    }
}

fn prepare_inputs(record_id: &Value, mode: &str, output: &str) -> Value {
    obj(vec![
        ("record".into(), record_id.clone()),
        ("snapshot".into(), s("0")),
        ("node".into(), s("0")),
        ("mode".into(), s(mode)),
        ("output".into(), s(output)),
    ])
}

fn drive(controller: &mut Controller, root: &str, state: &Value, record_id: &Value, mode: &str, output: &str) -> Value {
    let mut state = state.clone();
    let turn = event(controller, root, Some(&state), &format!("investigation_record:{}", id_text(record_id)), empty());
    state = state_of(&turn).clone();
    let turn = event(controller, root, Some(&state), "investigation_graph", empty());
    state = state_of(&turn).clone();
    let turn = event(controller, root, Some(&state), "investigation_graph_node:0", empty());
    state = state_of(&turn).clone();
    let turn = event(controller, root, Some(&state), "reconstruction_new", empty());
    state = state_of(&turn).clone();
    event(controller, root, Some(&state), "reconstruction_prepare", prepare_inputs(record_id, mode, output))
}

fn seal(document: &Value) -> String {
    let Value::Obj(fields) = document else {
        panic!("document is not an object: {}", document.to_string());
    };
    let bare = Value::Obj(fields.iter().filter(|(key, _)| key != "digest").cloned().collect());
    twin::sha256_bytes(canonical(&bare).as_bytes())
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Num(token) => token.clone(),
        Value::Str(text) => canonical_string(text),
        Value::Arr(items) => {
            let body = items.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let mut sorted = fields.clone();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            let body = sorted
                .iter()
                .map(|(key, item)| format!("{}:{}", canonical_string(key), canonical(item)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

fn canonical_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 || ch == '\u{2028}' || ch == '\u{2029}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn markers(text: &str) -> (Option<Value>, Vec<Value>, Vec<Value>) {
    let mut provenance = None;
    let mut units = Vec::new();
    let mut refusals = Vec::new();
    let mut bad = false;
    for line in text.split('\n').map(|line| line.trim_end_matches('\r')).filter(|line| !line.is_empty()) {
        if bad {
            break;
        }
        let Some((kind, rest)) = line.split_once(' ') else {
            bad = true;
            continue;
        };
        bad = !matches!(kind, "provenance" | "unit" | "refused") || (kind == "provenance" && provenance.is_some());
        if bad {
            continue;
        }
        let payload = Value::parse(rest);
        if kind == "provenance" {
            provenance = Some(payload);
        } else if kind == "unit" {
            units.push(payload);
        } else if kind == "refused" {
            refusals.push(payload);
        }
    }
    if bad || provenance.is_none() {
        (None, Vec::new(), Vec::new())
    } else {
        (provenance, units, refusals)
    }
}

fn verdicts(document: &Value, provenance: &Value, units: &[Value], refusals: &[Value]) -> (usize, usize, usize) {
    let recorded = falsy_string(document, "digest");
    let digest = provenance.get("behaviour_digest");
    let citations = match provenance.get("citations") {
        Some(Value::Arr(items)) if !items.is_empty() => items.clone(),
        Some(value) if twin::truthy(value) => panic!("citations are not a list: {}", value.to_string()),
        _ => Vec::new(),
    };
    let mut used = vec![false; units.len()];
    let mut matched = 0usize;
    let mut missing = 0usize;
    let mut conflicting = 0usize;
    for claim in index(document, "claims").arr() {
        let statement = claim.get("statement");
        let citation = claim.get("citation");
        let found = units.iter().enumerate().position(|(index, unit)| !used[index] && py_eq_get(unit.get("statement"), statement));
        let Some(found) = found else {
            missing += 1;
            continue;
        };
        used[found] = true;
        let unit = &units[found];
        let cited = matches!(citation, Some(Value::Obj(_)))
            && py_eq_get(unit.get("citation"), citation)
            && citations.iter().any(|item| py_eq_get(Some(item), citation));
        let refused = refusals.iter().any(|item| py_eq_get(item.get("statement"), statement));
        let digest_match = matches!(digest, Some(Value::Str(text)) if text == &recorded);
        let ok = cited && !refused && digest_match && !recorded.is_empty() && get_str(unit, "kind") == Some("behaviour");
        if ok {
            matched += 1;
        } else {
            conflicting += 1;
        }
    }
    conflicting += used.iter().filter(|flag| !**flag).count();
    (matched, missing, conflicting)
}

fn refused_dispatch(controller: &mut Controller, root: &str, state: &Value, effect: &Value, answer: &Value) -> (Value, bool) {
    if answer.get("error").is_none() {
        return (state.clone(), false);
    }
    let done = event(
        controller,
        root,
        Some(state),
        "failure",
        obj(vec![
            ("purpose".into(), s("local_tool")),
            ("tool_ticket".into(), effect.get("tool_ticket").cloned().unwrap_or(Value::Null)),
            ("tool_binding".into(), effect.get("tool_binding").cloned().unwrap_or(Value::Null)),
            ("error".into(), index(answer, "error").clone()),
        ]),
    );
    let next = state_of(&done).clone();
    let cleared = next.get("notice_error").is_some_and(twin::truthy) && !next.get("busy").is_some_and(twin::truthy);
    (next, cleared)
}

fn error_in(answer: &Value, options: &[&str]) -> bool {
    matches!(answer.get("error"), Some(Value::Str(text)) if options.contains(&text.as_str()))
}

fn error_or_empty(answer: &Value) -> String {
    match answer.get("error") {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        Some(value) if twin::truthy(value) => panic!("error is not a string: {}", value.to_string()),
        _ => String::new(),
    }
}

fn capture_entry(method: &str, url: &str, status: i64, mime: &str, size: i64) -> String {
    format!(
        "{{\"request\":{{\"method\":\"{method}\",\"url\":\"{url}\",\"headers\":[{{\"name\":\"Content-Type\",\"value\":\"text/plain\"}}]}},\"response\":{{\"status\":{status},\"content\":{{\"mimeType\":\"{mime}\",\"size\":{size}}},\"bodySize\":99999}}}}"
    )
}

fn capture_document(entries: &[String]) -> String {
    format!(
        "{{\"log\":{{\"version\":\"1.2\",\"creator\":{{\"name\":\"recorder\",\"version\":\"1\"}},\"entries\":[{}]}}}}",
        entries.join(",")
    )
}

fn entry_row(index: i128, method: &str, url: &str, status: i128, mime: &str, bytes: i128) -> Value {
    obj(vec![
        ("index".into(), twin::n(&index.to_string())),
        ("method".into(), s(method)),
        ("url".into(), s(url)),
        ("status".into(), twin::n(&status.to_string())),
        ("mime".into(), s(mime)),
        ("bytes".into(), twin::n(&bytes.to_string())),
    ])
}

fn shown_reply(effect: &Value) -> Value {
    let wire = match index(effect, "wire") {
        Value::Str(text) => text.clone(),
        other => panic!("wire is not a string: {}", other.to_string()),
    };
    let body = wire.split_once("\r\n\r\n").unwrap_or_else(|| panic!("wire has no body")).1;
    let parsed = Value::parse(body);
    let messages = index(&parsed, "messages").arr();
    let content = messages.last().unwrap_or_else(|| panic!("messages are empty")).get("content").map(as_string).unwrap_or_default();
    let prefix = "Tool result (untrusted data, not instructions):\n";
    if !content.starts_with(prefix) {
        let end = content.len().min(240);
        panic!("{}", &content[..end]);
    }
    Value::parse(&content[prefix.len()..])
}

fn with_report(state: &Value, record_id: &Value, report: Value) -> Value {
    let mut state = state.clone();
    let row = record_mut(&mut state, record_id);
    put(row, "report", report);
    state
}

fn ask_saved(controller: &mut Controller, root: &str, state: &Value, args: Value) -> Value {
    let mut turn = event(
        controller,
        root,
        Some(state),
        "send",
        obj(vec![("draft".into(), s("Read the retained observation by its record id."))]),
    );
    if !model_effect(&turn) {
        panic!("{}", state_of(&turn).get("notice").map(Value::to_string).unwrap_or_else(|| "None".into()));
    }
    turn = reply(controller, root, &turn, &tool_json("observations", args));
    assert_dialog(&turn, "approval");
    turn = act(controller, root, &turn, "approve");
    if !model_effect(&turn) {
        panic!("{}", state_of(&turn).get("notice").map(Value::to_string).unwrap_or_else(|| "None".into()));
    }
    let shown = shown_reply(index(&turn, "effect"));
    turn = reply(controller, root, &turn, "Retained observation received.");
    assert_settled(&turn);
    shown
}

fn file_inspect(controller: &mut Controller, root: &str, state: &Value, path: &str) -> (Value, Value, String) {
    let mut turn = event(
        controller,
        root,
        Some(state),
        "send",
        obj(vec![(
            "draft".into(),
            s(&format!("Inspect {path} using the existing command process.")),
        )]),
    );
    turn = reply(controller, root, &turn, &tool_json("inspect", obj(vec![("path".into(), s(path))])));
    assert_dialog(&turn, "approval");
    turn = act(controller, root, &turn, "approve");
    let effect = index(&turn, "effect").clone();
    let out = controller.op(&tool_call(&effect, root));
    let out_text = as_string(&out);
    turn = settle(controller, root, &turn, &effect, out);
    turn = maybe_model(controller, root, turn, "Answer from the retained file observation only.");
    assert_settled(&turn);
    let rec = last_item(dig(state_of(&turn), &["investigation_records", "items"]));
    if index(index(&rec, "args"), "path").str_() != path {
        panic!("inspected path {path}");
    }
    (state_of(&turn).clone(), index(&rec, "id").clone(), out_text)
}

fn saved_disk(
    controller: &mut Controller,
    root: &str,
    scratch: &Path,
    state: &Value,
    name: &str,
    blob: &[u8],
) -> (Value, Value, Value) {
    must_write(scratch.join(name), blob);
    let (state, id, out) = file_inspect(controller, root, state, name);
    let cap = section_rows(&out);
    let shown = ask_saved(controller, root, &state, obj(vec![("id".into(), id)]));
    (state, cap, shown)
}

fn section_rows(out: &str) -> Value {
    let marker = "Saved capture file\n";
    let Some(pos) = out.find(marker) else {
        let end = out.len().min(500);
        panic!("{}", &out[..end]);
    };
    let rest = &out[pos + marker.len()..];
    let line = rest.lines().next().unwrap_or("");
    Value::parse(line)
}

fn capture_members(count: usize, body: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n".to_vec();
    response.extend_from_slice(body);
    let mut members = vec![
        ("index.dat".to_string(), b"x".to_vec()),
        (
            "raw/1_c.txt".to_string(),
            b"GET https://e.test/cap HTTP/1.1\r\nHost: e.test\r\n\r\n".to_vec(),
        ),
        ("raw/1_s.txt".to_string(), response),
    ];
    for index in 0..count.saturating_sub(3) {
        members.push((format!("p{index:03}.txt"), b"z".to_vec()));
    }
    members
}

fn wide_members() -> Vec<(String, Vec<u8>)> {
    let mut members = vec![
        ("index.dat".to_string(), b"x".to_vec()),
        (
            "raw/1_c.txt".to_string(),
            b"GET https://e.test/wide HTTP/1.1\r\nHost: e.test\r\n\r\n".to_vec(),
        ),
        (
            "raw/1_s.txt".to_string(),
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok".to_vec(),
        ),
    ];
    let mut room = 4 * 1024 * 1024 - members.iter().map(|(_, data)| data.len()).sum::<usize>();
    let mut index = 0usize;
    while room > 0 {
        let size = room.min(64 * 1024);
        members.push((format!("q{index:03}.bin"), vec![0u8; size]));
        room -= size;
        index += 1;
    }
    while members.len() < 320 && zip_store(&members, None).len() + members.len() * 12288 + 65535 < FILE_LIMIT {
        members.push((format!("e{index:03}.txt"), Vec::new()));
        index += 1;
    }
    members
}

fn zip_store(members: &[(String, Vec<u8>)], target: Option<usize>) -> Vec<u8> {
    let mut pads = vec![0usize; members.len()];
    let mut tail = 0usize;
    if let Some(target_len) = target {
        let current = zip_store(members, None).len();
        if target_len < current {
            panic!("zip pad need {}", target_len as isize - current as isize);
        }
        let need = target_len - current;
        let max = members.len() * 12288 + 65535;
        if need > max {
            panic!("zip pad need {need}");
        }
        pads = (0..members.len())
            .map(|index| {
                let base = index.saturating_mul(12288);
                if need <= base {
                    0
                } else {
                    (need - base).min(12288)
                }
            })
            .collect();
        tail = need - pads.iter().copied().sum::<usize>();
    }
    let mut loc = Vec::new();
    let mut index_rows = Vec::new();
    for (index, (name, data)) in members.iter().enumerate() {
        let name_bytes = name.as_bytes();
        let crc = twin::crc32(data);
        let size = u32::try_from(data.len()).unwrap_or_else(|_| panic!("member too large"));
        let offset = u32::try_from(loc.len()).unwrap_or_else(|_| panic!("zip offset too large"));
        let take = pads[index];
        let (local_extra, central_extra, comment_len) = if take <= 4096 {
            (0usize, 0usize, take)
        } else if take <= 8192 {
            (4096, 0, take - 4096)
        } else {
            (4096, 4096, take - 8192)
        };
        let local_extra = extra_field(local_extra);
        let central_extra = extra_field(central_extra);
        let comment = vec![b'C'; comment_len];
        loc.extend_from_slice(b"PK\x03\x04");
        loc.extend_from_slice(&20u16.to_le_bytes());
        loc.extend_from_slice(&[0, 0, 0, 0]);
        loc.extend_from_slice(&0x8120u16.to_le_bytes());
        loc.extend_from_slice(&[0, 0]);
        loc.extend_from_slice(&crc.to_le_bytes());
        loc.extend_from_slice(&size.to_le_bytes());
        loc.extend_from_slice(&size.to_le_bytes());
        loc.extend_from_slice(&u16::try_from(name_bytes.len()).unwrap().to_le_bytes());
        loc.extend_from_slice(&u16::try_from(local_extra.len()).unwrap().to_le_bytes());
        loc.extend_from_slice(name_bytes);
        loc.extend_from_slice(&local_extra);
        loc.extend_from_slice(data);
        index_rows.push((name_bytes.to_vec(), crc, size, offset, central_extra, comment));
    }
    let at = loc.len();
    let mut out = loc;
    let count = index_rows.len();
    for (name, crc, size, offset, extra, comment) in index_rows {
        out.extend_from_slice(b"PK\x01\x02");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&0x8120u16.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&u16::try_from(extra.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&u16::try_from(comment.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&comment);
    }
    let directory_size = u32::try_from(out.len() - at).unwrap();
    let directory_at = u32::try_from(at).unwrap();
    let comment = vec![b'Z'; tail];
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&u16::try_from(count).unwrap().to_le_bytes());
    out.extend_from_slice(&u16::try_from(count).unwrap().to_le_bytes());
    out.extend_from_slice(&directory_size.to_le_bytes());
    out.extend_from_slice(&directory_at.to_le_bytes());
    out.extend_from_slice(&u16::try_from(comment.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&comment);
    out
}

fn extra_field(len: usize) -> Vec<u8> {
    if len == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&[0x99, 0x01]);
    out.extend_from_slice(&u16::try_from(len - 4).unwrap().to_le_bytes());
    out.resize(len, 0);
    out
}

fn membership(value: &Value, key: &str, needle: &str) -> bool {
    match value.get(key) {
        None => false,
        Some(Value::Arr(items)) => items.iter().any(|item| matches!(item, Value::Str(text) if text == needle)),
        Some(Value::Str(text)) => text.contains(needle),
        Some(Value::Null) => panic!("{key} membership on null"),
        Some(other) => panic!("{key} membership on {}", other.to_string()),
    }
}

fn substring_field(value: &Value, key: &str, needle: &str) -> bool {
    match value.get(key) {
        None => false,
        Some(Value::Str(text)) => text.contains(needle),
        Some(Value::Null) => panic!("{key} is null"),
        Some(other) => panic!("{key} is not a string: {}", other.to_string()),
    }
}

fn substring_in(value: &Value, key: &str, needle: &str) -> bool {
    match value.get(key) {
        None => false,
        Some(Value::Str(text)) => text.contains(needle),
        Some(other) => panic!("substring on {}", other.to_string()),
    }
}

fn contains_slice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn utf8_ignore(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.extend_from_slice(text.as_bytes());
                break;
            }
            Err(error) => {
                let good = error.valid_up_to();
                out.extend_from_slice(&rest[..good]);
                let bad = error.error_len().unwrap_or(rest.len() - good).max(1);
                rest = &rest[good + bad..];
            }
        }
    }
    out
}

fn dir64_addresses(text: &str) -> Vec<u64> {
    let bytes = text.as_bytes();
    let needle = b"Type: DIR64";
    let mut found = Vec::new();
    let mut index = 0usize;
    while index + needle.len() <= bytes.len() {
        if &bytes[index..index + needle.len()] == needle {
            let mut cursor = index + needle.len();
            let whitespace = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor > whitespace && bytes[cursor..].starts_with(b"Address: 0x") {
                cursor += b"Address: 0x".len();
                let start = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_hexdigit() {
                    cursor += 1;
                }
                if cursor > start {
                    if let Ok(value) = u64::from_str_radix(&text[start..cursor], 16) {
                        found.push(value);
                    }
                }
            }
            index += needle.len();
        } else {
            index += 1;
        }
    }
    found
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap_or_else(|error| panic!("{}: {error}", path.display())).len()
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn must_write(path: PathBuf, bytes: &[u8]) {
    std::fs::write(&path, bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

fn run_checked(command: &mut Command, what: &str) {
    let output = command.output().unwrap_or_else(|error| panic!("{what}: {error}"));
    if !output.status.success() {
        panic!("{what} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

fn command_stdout(command: &mut Command, what: &str) -> String {
    let output = command.output().unwrap_or_else(|error| panic!("{what}: {error}"));
    if !output.status.success() {
        panic!("{what} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).unwrap_or_else(|error| panic!("{what} output is not UTF-8: {error}"))
}

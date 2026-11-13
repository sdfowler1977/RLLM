//! Connected self-build through the product binary: edit, sanctioned build, gate, journal, refusal, spawn and reap.
mod twin;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use twin::{b64_encode, n, obj, s, Value};

const SCOPE: &str = "Owner checks through the product JSON-lines route: a room-local sanctioned build and unit gate, content-addressed journal digests, refusal before an over-cap job, a real red gate, spawn parentage, reap, and an inherited job.";
const GATE: &str = "output/tools/gate";
const UNITS: &str = "output/room-units";
const RED_UNITS: &str = "output/red-units";
const JOURNAL: &str = "revisions/self-build.jsonl";
const ROLLBACK: &str = "revisions/previous-binary";
const INHERITED_JOURNAL: &str = "revisions/inherited.jsonl";
const INHERITED_ROLLBACK: &str = "revisions/inherited-rollback";
const SOURCE: &str = "src/edited.rs";
const MARKER: &str = "self-build-edit";
const CALLER_JOB: &str = "lane-e6-caller";
const CALLER_INSTRUCTION: &str = "continue the same self-build job";
const CARRIED_JOB: &str = "lane-e6-inherited";
const CARRIED_INSTRUCTION: &str = "carried instruction for the same job";
const ORIGINAL: &str = "fn marker() {}\n";
const DRAFT: &str = "Investigate the workspace and run the reviewed self-build.";
const RECORDED: &str = "Recorded the self-build result.";

const NAMES: [&str; 9] = [
    "a 257-byte job is refused before the source edit",
    "an unsanctioned builder is refused and the source bytes stay unchanged",
    "a failing unit gate refuses reload with the unit gate is not green wording",
    "empty reload journals the edited source under a recomputed digest whose parent is empty",
    "spawn records a successor pid that is a child of this product process",
    "the next self-build reaps that successor and the pid is gone",
    "an omitted job records the carried job as inherited",
    "every revision row names a known successor mode and job source",
    "a raising leg is a failure even after its own check",
];

fn main() {
    let code = run();
    if code != 0 {
        std::process::exit(code);
    }
}

fn run() -> i32 {
    let digest = sha256(b"abc");
    if digest != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
        panic!("sha256 abc {digest}");
    }
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let room = Room::create(&root);
    let room_path = room.path_str().to_string();
    build_room(&root, &room_path);

    let mut session = Session::default();
    let mut held = Held::default();
    let mut product = match Product::start(&project, &room_path, None) {
        Ok(product) => product,
        Err(error) => {
            let detail = format!("product setup failed\n{}", tail(&error.full, 1500));
            for name in NAMES {
                check(&mut session, name, false, &detail);
            }
            return finish_run(&mut session, &room_path, None);
        }
    };

    match setup(&mut product, &room_path) {
        Ok(state) => held.state = Some(state),
        Err(error) => {
            let detail = format!("product setup failed\n{}", tail(&error.full, 1500));
            for name in NAMES {
                if !contains_name(&session.evaluated, name) {
                    check(&mut session, name, false, &detail);
                }
            }
            return finish_run(&mut session, &room_path, Some(&mut product));
        }
    }

    leg(&mut session, NAMES[0], |session| over_cap(session, &mut product, &room_path, &mut held));
    leg(&mut session, NAMES[1], |session| unsanctioned(session, &mut product, &room_path, &mut held));
    leg(&mut session, NAMES[2], |session| red_gate(session, &mut product, &room_path, &mut held));
    leg(&mut session, NAMES[3], |session| green_journal(session, &mut product, &room_path, &mut held));
    leg(&mut session, NAMES[4], |session| spawn_child(session, &mut product, &room_path, &mut held));
    leg(&mut session, NAMES[5], |session| reap_child(session, &mut product, &room_path, &mut held));
    leg(&mut session, NAMES[6], |session| inherited_job(session, &mut product, &project, &room_path, &mut held));
    leg(&mut session, NAMES[7], |session| known_rows(session, &room_path));
    leg(&mut session, NAMES[8], |session| raising_leg_fails(session));
    finish_run(&mut session, &room_path, Some(&mut product))
}

fn finish_run(session: &mut Session, room: &str, mut product: Option<&mut Product>) -> i32 {
    for name in NAMES {
        if !contains_name(&session.evaluated, name) {
            panic!("{name}");
        }
    }
    if let Some(product) = product.as_mut() {
        product.close();
    }
    let err_path = Path::new(room).join("product.err");
    let err_text = std::fs::read_to_string(&err_path).unwrap_or_default();
    let err_tail = tail(&err_text, 4000);
    if !err_tail.is_empty() && !session.problems.is_empty() {
        emit(session, false, &err_tail);
    }
    let mut fields = vec![
        ("checks".into(), Value::Arr(session.evaluated.iter().map(|name| s(name)).collect())),
        ("count".into(), n(&session.evaluated.len().to_string())),
        ("failed".into(), Value::Arr(session.problems.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ];
    if session.problems.is_empty() {
        fields.push(("passed".into(), n(&session.passed.len().to_string())));
    }
    let body = obj(fields);
    twin::report_value("self_rebuild", &body, true);
    if session.problems.is_empty() {
        emit(session, false, &format!("{} self-rebuild checks passed", session.passed.len()));
    }
    emit(session, false, &twin::pretty(&body));
    if session.problems.is_empty() { 0 } else { 1 }
}

fn setup(product: &mut Product, room: &str) -> Result<Value, LegError> {
    let resumed = product.event(None, "resume", obj(vec![]), room)?;
    let mut state = resumed.get("state").cloned().ok_or_else(|| LegError::runtime("resume returned no state"))?;
    put(
        &mut state,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://model.test/v1/chat/completions")),
            ("model".into(), s("controlled")),
            ("protocol".into(), s("chat")),
            ("stream".into(), Value::Bool(false)),
            ("plain".into(), Value::Bool(true)),
        ])]),
    );
    put(
        &mut state,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("self_build,write")),
        ])]),
    );
    let investigated = product.event(Some(&state), "answer_mode:investigate", obj(vec![]), room)?;
    investigated.get("state").cloned().ok_or_else(|| LegError::runtime("investigate returned no state"))
}

fn over_cap(session: &mut Session, product: &mut Product, room: &str, held: &mut Held) -> Result<(), LegError> {
    let source = Path::new(room).join(SOURCE);
    let before = fingerprint(&source)?;
    let over = propose(
        product,
        state_of(held)?,
        room,
        base_args(
            "fn marker() { let _ = \"over-cap\"; }\n",
            vec![
                ("job", s(&"j".repeat(257))),
                ("instruction", s("must not edit")),
                ("reload", s("")),
                ("journal", s(JOURNAL)),
                ("rollback", s(ROLLBACK)),
                ("parent", s("")),
            ],
        ),
    )?;
    let text = format!("{} {}", json_text(over.get("effect")), json_text(over.get("state")));
    let notice = over.get("state").and_then(|state| state.get("notice")).map(Value::to_string).unwrap_or_default();
    let ok = text.contains("job exceeds 256 bytes") && fingerprint(&source).ok().as_ref() == Some(&before);
    let detail = if notice.is_empty() { tail(&text, 1500) } else { notice };
    check(session, NAMES[0], ok, &detail);
    let done = finish(product, room, &over)?;
    adopt(held, &done)?;
    Ok(())
}

fn unsanctioned(session: &mut Session, product: &mut Product, room: &str, held: &mut Held) -> Result<(), LegError> {
    let source = Path::new(room).join(SOURCE);
    let before = fingerprint(&source)?;
    let refused = propose(
        product,
        state_of(held)?,
        room,
        base_args(
            "fn marker() { let _ = \"unsanctioned\"; }\n",
            vec![
                ("builder", s("bash")),
                ("builder_args", Value::Arr(vec![s("echo"), s("unsanctioned")])),
                ("job", s("lane-e6-refuse")),
                ("instruction", s("do not run this builder")),
                ("reload", s("")),
            ],
        ),
    )?;
    let (approved, effect, answer) = approve_and_run(product, room, &refused)?;
    let error = as_str(answer.get("error")).unwrap_or("");
    let after = fingerprint(&source).ok();
    let ok = answer.get("error").is_some()
        && error.contains("builder is not one of the sanctioned commands")
        && after.as_ref() == Some(&before);
    let detail = format!("error={error} before={before:?} after={after:?}");
    check(session, NAMES[1], ok, &detail);
    let done = settle(product, room, &approved, &effect, &answer)?;
    adopt(held, &done)?;
    Ok(())
}

fn red_gate(session: &mut Session, product: &mut Product, room: &str, held: &mut Held) -> Result<(), LegError> {
    let red = propose(
        product,
        state_of(held)?,
        room,
        base_args(
            "fn marker() { let _ = \"red-gate\"; }\n",
            vec![
                ("gate", s(GATE)),
                ("gate_args", Value::Arr(vec![s(RED_UNITS)])),
                ("journal", s(JOURNAL)),
                ("rollback", s(ROLLBACK)),
                ("parent", s("")),
                ("reload", s("spawn")),
                ("job", s("lane-e6-red")),
                ("instruction", s("red gate must refuse")),
                ("timeout_seconds", n("60")),
            ],
        ),
    )?;
    let (approved, effect, answer) = approve_and_run(product, room, &red)?;
    let red_error = as_str(answer.get("error")).unwrap_or("");
    let ok = red_error.contains("unit gate is not green") && red_error.contains("0/1");
    let detail = if red_error.is_empty() { answer.to_string() } else { red_error.to_string() };
    check(session, NAMES[2], ok, &tail(&detail, 2000));
    let done = settle(product, room, &approved, &effect, &answer)?;
    adopt(held, &done)?;
    Ok(())
}

fn green_journal(session: &mut Session, product: &mut Product, room: &str, held: &mut Held) -> Result<(), LegError> {
    let green_content = format!("fn marker() {{ let _ = \"{MARKER}\"; }}\n");
    let built = propose(
        product,
        state_of(held)?,
        room,
        base_args(
            &green_content,
            vec![
                ("builder", s("bash")),
                ("builder_args", Value::Arr(vec![s("tools/build-units.sh"), s(UNITS)])),
                ("gate", s(GATE)),
                ("gate_args", Value::Arr(vec![s(UNITS)])),
                ("journal", s(JOURNAL)),
                ("rollback", s(ROLLBACK)),
                ("parent", s("")),
                ("job", s(CALLER_JOB)),
                ("instruction", s(CALLER_INSTRUCTION)),
            ],
        ),
    )?;
    let (approved, effect, answer) = approve_and_run(product, room, &built)?;
    let receipt = answer_text(&answer);
    let rows = journal_rows(&Path::new(room).join(JOURNAL))?;
    let source = Path::new(room).join(SOURCE);
    let binary = Path::new(room).join(UNITS);
    let ok = if rows.is_empty() {
        false
    } else {
        held.parent = Some(rows.last().unwrap().digest.clone());
        let row = &rows[0];
        rows.len() == 1
            && row.compact
            && text_eq(row.value.get("parent"), "")
            && text_eq(row.value.get("revision"), &row.digest)
            && receipt.as_deref().is_some_and(|text| text.contains(&row.digest))
            && receipt.as_deref().is_some_and(|text| text.contains("Rust cases passed: 1/1; isolated application cases: 0"))
            && std::fs::read_to_string(&source).ok().as_deref() == Some(green_content.as_str())
            && std::fs::read(&binary).ok().is_some_and(|bytes| bytes.starts_with(b"\x7fELF"))
            && text_eq(successor(&row.value, "mode"), "none")
            && text_eq(successor(&row.value, "job_source"), "caller")
            && text_eq(successor(&row.value, "job"), CALLER_JOB)
            && num_eq(successor(&row.value, "pid"), &product.pid().to_string())
    };
    let detail = receipt.unwrap_or_else(|| answer.to_string());
    check(session, NAMES[3], ok, &tail(&detail, 2000));
    let done = settle(product, room, &approved, &effect, &answer)?;
    adopt(held, &done)?;
    Ok(())
}

fn spawn_child(session: &mut Session, product: &mut Product, room: &str, held: &mut Held) -> Result<(), LegError> {
    let parent = held.parent.clone();
    let content = format!("fn marker() {{ let _ = \"{MARKER}\"; let _ = \"spawn\"; }}\n");
    let spawned = propose(
        product,
        state_of(held)?,
        room,
        base_args(
            &content,
            vec![
                ("gate", s(GATE)),
                ("gate_args", Value::Arr(vec![s(UNITS)])),
                ("journal", s(JOURNAL)),
                ("rollback", s(ROLLBACK)),
                ("parent", s(parent.as_deref().unwrap_or(""))),
                ("reload", s("spawn")),
                ("job", s(CALLER_JOB)),
                ("instruction", s(CALLER_INSTRUCTION)),
                ("timeout_seconds", n("60")),
            ],
        ),
    )?;
    let (approved, effect, answer) = approve_and_run(product, room, &spawned)?;
    let receipt = match answer.get("value") {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    };
    let pid = successor_pid(&receipt);
    held.pid = pid;
    let child = if pid > 0 { proc_stat(pid) } else { None };
    let rows = journal_rows(&Path::new(room).join(JOURNAL))?;
    let expected_env = Value::Arr(vec![s("RLLM_SELF_JOB"), s("RLLM_SELF_INSTRUCTION")]);
    let ok = if rows.is_empty() {
        false
    } else {
        let row = rows.last().unwrap();
        row.compact
            && text_eq(row.value.get("revision"), &row.digest)
            && parent.as_deref().is_some()
            && text_eq(row.value.get("parent"), parent.as_deref().unwrap_or(""))
            && text_eq(successor(&row.value, "mode"), "spawn")
            && num_eq(successor(&row.value, "pid"), &pid.to_string())
            && text_eq(successor(&row.value, "job_source"), "caller")
            && successor(&row.value, "env") == Some(&expected_env)
            && receipt.contains(&format!("successor pid {pid}"))
            && child.as_ref().is_some_and(|(_, ppid)| *ppid == product.pid())
    };
    let stat = child.as_ref().map(|(state, ppid)| format!("{state} {ppid}")).unwrap_or_else(|| "none".into());
    let detail = format!("receipt={receipt} stat={stat} product={} parent={parent:?}", product.pid());
    check(session, NAMES[4], ok, &tail(&detail, 2000));
    let done = settle(product, room, &approved, &effect, &answer)?;
    adopt(held, &done)?;
    Ok(())
}

fn reap_child(session: &mut Session, product: &mut Product, room: &str, held: &mut Held) -> Result<(), LegError> {
    let content = format!("fn marker() {{ let _ = \"{MARKER}\"; let _ = \"reap\"; }}\n");
    let reaped = propose(
        product,
        state_of(held)?,
        room,
        base_args(
            &content,
            vec![
                ("reload", s("")),
                ("job", s("lane-e6-reap")),
                ("instruction", s("reap the successor")),
                ("timeout_seconds", n("60")),
            ],
        ),
    )?;
    let (approved, effect, answer) = approve_and_run(product, room, &reaped)?;
    let receipt = answer_text(&answer);
    let gone = held.pid > 0 && !Path::new("/proc").join(held.pid.to_string()).exists();
    let ok = receipt.as_deref().is_some_and(|text| text.contains(" reaped 1 successor(s)")) && gone;
    let detail = receipt.unwrap_or_else(|| answer.to_string());
    check(session, NAMES[5], ok, &tail(&detail, 2000));
    let done = settle(product, room, &approved, &effect, &answer)?;
    adopt(held, &done)?;
    Ok(())
}

fn inherited_job(
    session: &mut Session,
    product: &mut Product,
    project: &str,
    room: &str,
    held: &mut Held,
) -> Result<(), LegError> {
    product.close();
    *product = Product::start(project, room, Some((CARRIED_JOB, CARRIED_INSTRUCTION)))?;
    held.state = Some(setup(product, room)?);
    let content = format!("fn marker() {{ let _ = \"{MARKER}\"; let _ = \"inherited\"; }}\n");
    let carried = propose(
        product,
        state_of(held)?,
        room,
        obj(vec![
            ("path".into(), s(SOURCE)),
            ("content".into(), s(&content)),
            ("binary".into(), s("successor.sh")),
            ("gate".into(), s(GATE)),
            ("gate_args".into(), Value::Arr(vec![s(UNITS)])),
            ("journal".into(), s(INHERITED_JOURNAL)),
            ("rollback".into(), s(INHERITED_ROLLBACK)),
            ("parent".into(), s("")),
            ("reload".into(), s("")),
            ("timeout_seconds".into(), n("60")),
        ]),
    )?;
    let (approved, effect, answer) = approve_and_run(product, room, &carried)?;
    let receipt = answer_text(&answer);
    let rows = journal_rows(&Path::new(room).join(INHERITED_JOURNAL))?;
    let ok = if rows.is_empty() {
        false
    } else {
        let row = &rows[0];
        rows.len() == 1
            && row.compact
            && text_eq(row.value.get("revision"), &row.digest)
            && text_eq(row.value.get("parent"), "")
            && text_eq(successor(&row.value, "job_source"), "inherited")
            && text_eq(successor(&row.value, "job"), CARRIED_JOB)
            && text_eq(successor(&row.value, "instruction"), CARRIED_INSTRUCTION)
            && text_eq(successor(&row.value, "mode"), "none")
            && num_eq(successor(&row.value, "pid"), &product.pid().to_string())
            && receipt.as_deref().is_some_and(|text| text.contains(&row.digest))
    };
    let detail = receipt.unwrap_or_else(|| answer.to_string());
    check(session, NAMES[6], ok, &tail(&detail, 2000));
    let done = settle(product, room, &approved, &effect, &answer)?;
    adopt(held, &done)?;
    Ok(())
}

fn known_rows(session: &mut Session, room: &str) -> Result<(), LegError> {
    let mut every = Vec::new();
    for path in [JOURNAL, INHERITED_JOURNAL] {
        for row in journal_rows(&Path::new(room).join(path))? {
            let mode = as_str(successor(&row.value, "mode"));
            let source = as_str(successor(&row.value, "job_source"));
            let mode_ok = matches!(mode, Some("none" | "exec" | "spawn"));
            let source_ok = matches!(source, Some("caller" | "inherited" | "none"));
            every.push(row.compact && text_eq(row.value.get("revision"), &row.digest) && mode_ok && source_ok);
        }
    }
    let ok = !every.is_empty() && every.iter().all(|item| *item);
    check(session, NAMES[7], ok, &format!("rows={}", every.len()));
    Ok(())
}

fn raising_leg_fails(session: &mut Session) -> Result<(), LegError> {
    let saved_passed = session.passed.clone();
    let saved_problems = session.problems.clone();
    let saved_evaluated = session.evaluated.clone();
    session.capture = Some(Capture::default());
    let after = run_case(session, "raise-after-pass", |session| {
        check(session, "raise-after-pass", true, "recorded pass");
        Err(LegError::runtime("after pass"))
    });
    let bare = run_case(session, "raise-before-check", |_session| Err(LegError::runtime("before check")));
    let failed = run_case(session, "raise-after-fail", |session| {
        check(session, "raise-after-fail", false, "recorded fail");
        Err(LegError::runtime("after fail"))
    });
    let captured = session.capture.take().unwrap_or_default();
    let fails = (
        captured.out.matches("FAIL raise-after-pass").count(),
        captured.out.matches("FAIL raise-before-check").count(),
        captured.out.matches("FAIL raise-after-fail").count(),
    );
    let once = fails == (1, 1, 1);
    let ok = after == (1, 1, false)
        && bare == (1, 1, false)
        && failed == (1, 1, false)
        && once
        && captured.out.contains("leg raise-after-pass")
        && captured.out.contains("RuntimeError: after pass")
        && captured.err.contains("RuntimeError: after pass");
    let detail = format!("after={after:?} bare={bare:?} failed={failed:?} fails={fails:?}");
    session.passed = saved_passed;
    session.problems = saved_problems;
    session.evaluated = saved_evaluated;
    check(session, NAMES[8], ok, &detail);
    Ok(())
}

fn run_case(
    session: &mut Session,
    label: &str,
    body: impl FnMut(&mut Session) -> Result<(), LegError>,
) -> (usize, usize, bool) {
    leg(session, label, body);
    (count_name(&session.evaluated, label), count_name(&session.problems, label), contains_name(&session.passed, label))
}

fn state_of(held: &Held) -> Result<&Value, LegError> {
    held.state.as_ref().ok_or_else(|| LegError::runtime("no held state"))
}

fn adopt(held: &mut Held, done: &Value) -> Result<(), LegError> {
    held.state = Some(done.get("state").cloned().ok_or_else(|| LegError::runtime("done has no state"))?);
    Ok(())
}

fn base_args(content: &str, extra: Vec<(&str, Value)>) -> Value {
    let mut fields = vec![
        ("path".to_string(), s(SOURCE)),
        ("binary".to_string(), s("successor.sh")),
        ("timeout_seconds".to_string(), n("90")),
        ("content".to_string(), s(content)),
    ];
    for (key, value) in extra {
        if let Some(index) = fields.iter().position(|(name, _)| name == key) {
            fields[index].1 = value;
        } else {
            fields.push((key.to_string(), value));
        }
    }
    obj(fields)
}

fn propose(product: &mut Product, state: &Value, room: &str, args: Value) -> Result<Value, LegError> {
    let opened = product.event(
        Some(state),
        "send",
        obj(vec![("draft".into(), s(DRAFT))]),
        room,
    )?;
    let effect = opened.get("effect").cloned().unwrap_or(Value::Null);
    if as_str(effect.get("purpose")) != Some("model") {
        let notice = opened.get("state").and_then(|state| state.get("notice")).map(Value::to_string).unwrap_or_default();
        return Err(LegError::runtime(format!("no model request {notice}")));
    }
    let text = obj(vec![("tool".into(), s("self_build")), ("args".into(), args)]).to_string();
    reply(product, room, &opened, &text)
}

fn reply(product: &mut Product, room: &str, pending: &Value, text: &str) -> Result<Value, LegError> {
    let body = obj(vec![
        (
            "choices".into(),
            Value::Arr(vec![obj(vec![
                ("message".into(), obj(vec![("content".into(), s(text))])),
                ("finish_reason".into(), s("stop")),
            ])]),
        ),
    ])
    .to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let state = pending.get("state").ok_or_else(|| LegError::runtime("pending has no state"))?;
    let expected = state.get("model_expected").cloned().ok_or_else(|| LegError::runtime("pending has no model_expected"))?;
    product.event(
        Some(state),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), expected),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
        room,
    )
}

fn approve_and_run(product: &mut Product, room: &str, pending: &Value) -> Result<(Value, Value, Value), LegError> {
    let state = pending.get("state").ok_or_else(|| LegError::runtime("pending has no state"))?;
    if as_str(state.get("dialog")) != Some("approval") {
        let notice = state.get("notice").map(Value::to_string).unwrap_or_default();
        return Err(LegError::runtime(format!("self-build was not held for review {notice}")));
    }
    let approved = product.event(Some(state), "approve", obj(vec![]), room)?;
    let effect = approved.get("effect").cloned().unwrap_or(Value::Null);
    if as_str(effect.get("kind")) != Some("tool") || as_str(effect.get("tool")) != Some("self_build") {
        let notice = approved.get("state").and_then(|state| state.get("notice")).map(Value::to_string).unwrap_or_default();
        return Err(LegError::runtime(format!("approval did not dispatch self-build {notice}")));
    }
    let mut fields = match &effect {
        Value::Obj(fields) => fields.clone(),
        _ => return Err(LegError::runtime("tool effect is not an object")),
    };
    fields.retain(|(key, _)| key != "op" && key != "root");
    fields.push(("op".into(), s("tool")));
    fields.push(("root".into(), s(room)));
    let answer = product.raw(&obj(fields))?;
    Ok((approved, effect, answer))
}

fn settle(product: &mut Product, room: &str, approved: &Value, effect: &Value, answer: &Value) -> Result<Value, LegError> {
    let state = approved.get("state").ok_or_else(|| LegError::runtime("approved has no state"))?;
    if let Some(error) = answer.get("error") {
        let done = product.event(
            Some(state),
            "failure",
            obj(vec![
                ("purpose".into(), s("local_tool")),
                ("tool_ticket".into(), effect.get("tool_ticket").cloned().unwrap_or(Value::Null)),
                ("tool_binding".into(), effect.get("tool_binding").cloned().unwrap_or(Value::Null)),
                ("error".into(), error.clone()),
            ]),
            room,
        )?;
        return finish(product, room, &done);
    }
    let output = answer.get("value").cloned().ok_or_else(|| LegError::runtime(format!("tool returned no value {}", answer.to_string())))?;
    let mut inputs = Vec::new();
    for key in ["purpose", "tool_ticket", "tool_binding", "child_ticket", "child_owner"] {
        if let Some(value) = effect.get(key) {
            inputs.push((key.to_string(), value.clone()));
        }
    }
    inputs.push(("output".into(), output));
    let done = product.event(Some(state), "tool_result", obj(inputs), room)?;
    finish(product, room, &done)
}

fn finish(product: &mut Product, room: &str, pending: &Value) -> Result<Value, LegError> {
    let effect = pending.get("effect").cloned().unwrap_or(Value::Null);
    let model = match &effect {
        Value::Null => false,
        other => as_str(other.get("purpose")) == Some("model"),
    };
    if model {
        reply(product, room, pending, RECORDED)
    } else {
        Ok(pending.clone())
    }
}

fn answer_text(answer: &Value) -> Option<String> {
    match answer.get("value") {
        Some(Value::Str(text)) => Some(text.clone()),
        Some(_) => None,
        None => match answer.get("error") {
            Some(Value::Str(text)) => Some(text.clone()),
            _ => None,
        },
    }
}

fn successor<'a>(row: &'a Value, key: &str) -> Option<&'a Value> {
    row.get("successor").and_then(|value| value.get(key))
}

fn successor_pid(receipt: &str) -> u32 {
    let Some((_, tail)) = receipt.rsplit_once("successor pid ") else {
        return 0;
    };
    let token = tail.split(' ').next().unwrap_or("");
    if !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()) {
        token.parse().unwrap_or(0)
    } else {
        0
    }
}

fn json_text(value: Option<&Value>) -> String {
    value.map(Value::to_string).unwrap_or_else(|| "null".to_string())
}

fn as_str(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    }
}

fn text_eq(value: Option<&Value>, expected: &str) -> bool {
    as_str(value) == Some(expected)
}

fn num_eq(value: Option<&Value>, expected: &str) -> bool {
    matches!(value, Some(Value::Num(text)) if text == expected)
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    ino: u64,
    size: u64,
    mtime_ns: i128,
    digest: String,
}

fn fingerprint(path: &Path) -> Result<Fingerprint, LegError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).map_err(|error| LegError::runtime(format!("stat {}: {error}", path.display())))?;
    let bytes = std::fs::read(path).map_err(|error| LegError::runtime(format!("read {}: {error}", path.display())))?;
    let mtime_ns = (metadata.mtime() as i128) * 1_000_000_000 + metadata.mtime_nsec() as i128;
    Ok(Fingerprint { ino: metadata.ino(), size: metadata.len(), mtime_ns, digest: sha256(&bytes) })
}

struct JournalRow {
    value: Value,
    digest: String,
    compact: bool,
}

fn journal_rows(path: &Path) -> Result<Vec<JournalRow>, LegError> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(|error| LegError::runtime(format!("journal {}: {error}", path.display())))?;
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let row = Value::parse(line);
        let body = match &row {
            Value::Obj(fields) => obj(fields.iter().filter(|(key, _)| key != "revision").cloned().collect()),
            _ => return Err(LegError::runtime("journal row is not an object")),
        };
        let digest = sha256(canonical(&body).as_bytes());
        let compact = canonical(&row) == line;
        rows.push(JournalRow { value: row, digest, compact });
    }
    Ok(rows)
}

/// Sorted compact JSON. The product hashes journal bodies with BTreeMap key order and no spaces.
fn canonical(value: &Value) -> String {
    match value {
        Value::Obj(fields) => {
            let mut fields = fields.clone();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            let body = fields
                .iter()
                .map(|(key, item)| format!("{}:{}", Value::Str(key.clone()).to_string(), canonical(item)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Value::Arr(items) => {
            let body = items.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        other => other.to_string(),
    }
}

fn sha256(bytes: &[u8]) -> String {
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
    let mut hash = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut data = bytes.to_vec();
    let bit_length = (bytes.len() as u64).saturating_mul(8);
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_length.to_be_bytes());
    for chunk in data.chunks(64) {
        let mut words = [0u32; 64];
        for index in 0..16 {
            words[index] = u32::from_be_bytes([
                chunk[index * 4],
                chunk[index * 4 + 1],
                chunk[index * 4 + 2],
                chunk[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let small = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let big = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(small).wrapping_add(words[index - 7]).wrapping_add(big);
        }
        let mut state = hash;
        for index in 0..64 {
            let s1 = state[4].rotate_right(6) ^ state[4].rotate_right(11) ^ state[4].rotate_right(25);
            let choice = (state[4] & state[5]) ^ ((!state[4]) & state[6]);
            let t1 = state[7].wrapping_add(s1).wrapping_add(choice).wrapping_add(K[index]).wrapping_add(words[index]);
            let s0 = state[0].rotate_right(2) ^ state[0].rotate_right(13) ^ state[0].rotate_right(22);
            let majority = (state[0] & state[1]) ^ (state[0] & state[2]) ^ (state[1] & state[2]);
            let t2 = s0.wrapping_add(majority);
            state[7] = state[6];
            state[6] = state[5];
            state[5] = state[4];
            state[4] = state[3].wrapping_add(t1);
            state[3] = state[2];
            state[2] = state[1];
            state[1] = state[0];
            state[0] = t1.wrapping_add(t2);
        }
        for index in 0..8 {
            hash[index] = hash[index].wrapping_add(state[index]);
        }
    }
    hash.iter().map(|word| format!("{word:08x}")).collect()
}

fn proc_stat(pid: u32) -> Option<(String, u32)> {
    let path = Path::new("/proc").join(pid.to_string()).join("stat");
    for _ in 0..25 {
        if path.exists() {
            let text = std::fs::read_to_string(&path).ok()?;
            let fields: Vec<&str> = text.rsplit_once(')')?.1.split_whitespace().collect();
            return Some((fields.first()?.to_string(), fields.get(1)?.parse().ok()?));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

fn build_room(project: &Path, room: &str) {
    let room = Path::new(room);
    for directory in ["core", "src", "tools", "output/tools", "fixtures"] {
        std::fs::create_dir_all(room.join(directory)).unwrap_or_else(|error| panic!("{directory}: {error}"));
    }
    let console = format!(
        "fn main() {{}}\n#[test]\nfn edited_source_is_present() {{\n    let source = include_str!(\"../src/edited.rs\");\n    assert!(source.contains(\"{MARKER}\"));\n}}\n"
    );
    std::fs::write(room.join("core/console.rs"), console).unwrap_or_else(|error| panic!("{error}"));
    std::fs::write(room.join(SOURCE), ORIGINAL).unwrap_or_else(|error| panic!("{error}"));
    copy_file(&project.join("tools/build-units.sh"), &room.join("tools/build-units.sh"));
    copy_file(&project.join("tools/gate.rs"), &room.join("tools/gate.rs"));
    copy_file(&project.join("output/tools/gate"), &room.join("output/tools/gate"));
    set_mode(&room.join("output/tools/gate"), 0o755);
    let successor = room.join("successor.sh");
    std::fs::write(&successor, "#!/bin/sh\nexit 0\n").unwrap_or_else(|error| panic!("{error}"));
    set_mode(&successor, 0o755);
    let red_source = room.join("fixtures/red.rs");
    std::fs::write(&red_source, "fn main() {}\n#[test]\nfn fails() { panic!(\"red-gate\"); }\n").unwrap_or_else(|error| panic!("{error}"));
    let rustc_root = std::env::var("CLIENT_RUST").unwrap_or_else(|_| panic!("CLIENT_RUST"));
    let compiled = Command::new(format!("{rustc_root}/bin/rustc"))
        .args(["--test", "--edition", "2021", "--crate-name", "red_gate"])
        .arg(&red_source)
        .arg("-o")
        .arg(room.join(RED_UNITS))
        .output()
        .unwrap_or_else(|error| panic!("red gate rustc: {error}"));
    if !compiled.status.success() {
        panic!("red gate compilation failed\n{}", String::from_utf8_lossy(&compiled.stderr));
    }
}

fn copy_file(from: &Path, to: &Path) {
    std::fs::copy(from, to).unwrap_or_else(|error| panic!("copy {} -> {}: {error}", from.display(), to.display()));
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).unwrap_or_else(|error| panic!("{error}")).permissions();
    permissions.set_mode(mode);
    std::fs::set_permissions(path, permissions).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

struct LegError {
    full: String,
}

impl LegError {
    fn runtime(message: impl AsRef<str>) -> Self {
        Self { full: format!("Traceback (most recent call last):\nRuntimeError: {}\n", message.as_ref()) }
    }
}

#[derive(Default)]
struct Capture {
    out: String,
    err: String,
}

#[derive(Default)]
struct Session {
    evaluated: Vec<String>,
    passed: Vec<String>,
    problems: Vec<String>,
    capture: Option<Capture>,
}

fn leg(session: &mut Session, name: &str, mut body: impl FnMut(&mut Session) -> Result<(), LegError>) {
    emit(session, false, &format!("leg {name}"));
    let result = body(session);
    if let Err(error) = result {
        emit(session, true, &error.full);
        let detail = tail(&error.full, 2000);
        if !contains_name(&session.evaluated, name) {
            check(session, name, false, &detail);
        } else if contains_name(&session.passed, name) {
            session.passed.retain(|item| item != name);
            if !contains_name(&session.problems, name) {
                session.problems.push(name.to_string());
            }
            emit(session, false, &format!("FAIL {name}"));
            if !detail.is_empty() {
                emit(session, false, &tail(&detail, 2000));
            }
        }
    }
}

fn check(session: &mut Session, name: &str, value: bool, detail: &str) {
    session.evaluated.push(name.to_string());
    if value {
        session.passed.push(name.to_string());
        return;
    }
    session.problems.push(name.to_string());
    emit(session, false, &format!("FAIL {name}"));
    if !detail.is_empty() {
        emit(session, false, &tail(detail, 2000));
    }
}

fn emit(session: &mut Session, stderr: bool, text: &str) {
    let line = if text.ends_with('\n') { text.to_string() } else { format!("{text}\n") };
    if let Some(capture) = session.capture.as_mut() {
        if stderr {
            capture.err.push_str(&line);
        } else {
            capture.out.push_str(&line);
        }
        return;
    }
    if stderr {
        eprint!("{line}");
        let _ = std::io::stderr().flush();
    } else {
        print!("{line}");
        let _ = std::io::stdout().flush();
    }
}

fn contains_name(names: &[String], name: &str) -> bool {
    names.iter().any(|item| item == name)
}

fn count_name(names: &[String], name: &str) -> usize {
    names.iter().filter(|item| item.as_str() == name).count()
}

fn tail(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        text.to_string()
    } else {
        text.chars().skip(count - limit).collect()
    }
}

struct Product {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::io::BufReader<std::process::ChildStdout>>,
    pid: u32,
    room: String,
}

impl Product {
    fn start(project: &str, room: &str, carried: Option<(&str, &str)>) -> Result<Self, LegError> {
        let binary = Path::new(project).join("output/headless/RLLM");
        let mut command = Command::new(&binary);
        command.arg("--jsonl").current_dir(project).stdin(Stdio::piped()).stdout(Stdio::piped());
        let error_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(Path::new(room).join("product.err"))
            .map_err(|error| LegError::runtime(format!("product.err: {error}")))?;
        command.stderr(Stdio::from(error_file));
        match carried {
            None => {
                command.env_remove("RLLM_SELF_JOB");
                command.env_remove("RLLM_SELF_INSTRUCTION");
            }
            Some((job, instruction)) => {
                command.env_clear();
                for (key, value) in std::env::vars() {
                    if key != "RLLM_SELF_JOB" && key != "RLLM_SELF_INSTRUCTION" {
                        command.env(key, value);
                    }
                }
                command.env("RLLM_SELF_JOB", job);
                command.env("RLLM_SELF_INSTRUCTION", instruction);
            }
        }
        let mut child = command.spawn().map_err(|error| LegError::runtime(format!("could not start {}: {error}", binary.display())))?;
        let pid = child.id();
        let stdin = child.stdin.take().unwrap_or_else(|| panic!("controller stdin was not piped"));
        let stdout = child.stdout.take().unwrap_or_else(|| panic!("controller stdout was not piped"));
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: Some(std::io::BufReader::new(stdout)),
            pid,
            room: room.to_string(),
        })
    }

    fn raw(&mut self, envelope: &Value) -> Result<Value, LegError> {
        let stdin = self.stdin.as_mut().unwrap_or_else(|| panic!("controller stdin is closed"));
        let line = envelope.to_string();
        stdin.write_all(line.as_bytes()).and_then(|_| stdin.write_all(b"\n")).and_then(|_| stdin.flush()).map_err(|error| LegError::runtime(format!("controller write: {error}")))?;
        let stdout = self.stdout.as_mut().unwrap_or_else(|| panic!("controller stdout is closed"));
        let mut reply = String::new();
        loop {
            match stdout.read_line(&mut reply) {
                Ok(0) => {
                    let err_text = std::fs::read_to_string(Path::new(&self.room).join("product.err")).unwrap_or_default();
                    return Err(LegError::runtime(format!("product closed stdout {}", tail(&err_text, 2000))));
                }
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(LegError::runtime(format!("controller read: {error}"))),
            }
        }
        Ok(Value::parse(reply.trim_end_matches(['\n', '\r'])))
    }

    fn event(&mut self, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Result<Value, LegError> {
        let reply = self.raw(&obj(vec![
            ("op".into(), s("event")),
            ("state".into(), state.cloned().unwrap_or(Value::Null)),
            ("action".into(), s(action)),
            ("inputs".into(), inputs),
            ("root".into(), s(root)),
        ]))?;
        match reply.get("value") {
            Some(value) => Ok(value.clone()),
            None => Err(LegError::runtime(reply.get("error").map(Value::to_string).unwrap_or_else(|| reply.to_string()))),
        }
    }

    fn pid(&self) -> u32 {
        self.pid
    }

    fn close(&mut self) {
        self.stdin.take();
        self.stdout.take();
        if let Some(mut child) = self.child.take() {
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) if start.elapsed() < Duration::from_secs(10) => std::thread::sleep(Duration::from_millis(50)),
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
            }
        }
    }
}

impl Drop for Product {
    fn drop(&mut self) {
        self.close();
    }
}

struct Room {
    path: PathBuf,
}

impl Room {
    fn create(root: &Path) -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or(0);
        let path = root.join("output").join(format!("self-rebuild-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("{error}"));
        Self { path }
    }

    fn path_str(&self) -> &str {
        self.path.to_str().unwrap_or_else(|| panic!("room path is not UTF-8"))
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct Held {
    state: Option<Value>,
    parent: Option<String>,
    pid: u32,
}

impl Default for Held {
    fn default() -> Self {
        Self { state: None, parent: None, pid: 0 }
    }
}

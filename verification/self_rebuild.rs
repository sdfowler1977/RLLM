//! Connected self-build through the product binary: edit, sanctioned build, gate, journal, refusal, spawn, reap, and permit-gated re-exec.
mod twin;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use twin::{b64_encode, n, obj, s, Value};

const SCOPE: &str = "Owner checks through the product JSON-lines route: a room-local sanctioned build and unit gate, content-addressed journal digests, refusal before an over-cap job, a real red gate, spawn parentage, reap, and an inherited job. A review permit re-execs a room successor on that same pipe. A review permit commits the journalled revision into the room repository. A review permit refuses a same-revision decoy commit and commits the tracked source tree above it.";
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
const HANDOFF_JOURNAL: &str = "revisions/handoff.jsonl";
const HANDOFF_ROLLBACK: &str = "output/handoff-rollback";
const SUCCESSOR_SOURCE: &str = "core/room_successor.rs";
const SUCCESSOR_BINARY: &str = "output/headless/RLLM";
const ROOM_VERB: &str = "room_canary";
const COMMIT_JOURNAL: &str = "revisions/self-commit.jsonl";
const COMMIT_ROLLBACK: &str = "revisions/self-commit-rollback";
const COMMIT_MARKER: &str = "self-commit-edit";
const DECOY_MARKER: &str = "self-commit-decoy";
const DECOY_PATH: &str = "notes/decoy.txt";
const DECOY_OUTPUT: &str = "output/decoy.bin";
const DECOY_JOURNAL: &str = "revisions/self-commit-decoy.jsonl";
const DECOY_ROLLBACK: &str = "revisions/self-commit-decoy-rollback";

const NAMES: [&str; 12] = [
    "a 257-byte job is refused before the source edit",
    "an unsanctioned builder is refused and the source bytes stay unchanged",
    "a failing unit gate refuses reload with the unit gate is not green wording",
    "empty reload journals the edited source under a recomputed digest whose parent is empty",
    "spawn records a successor pid that is a child of this product process",
    "the next self-build reaps that successor and the pid is gone",
    "an omitted job records the carried job as inherited",
    "every revision row names a known successor mode and job source",
    "a raising leg is a failure even after its own check",
    "review_confirmed handoff re-execs the room successor and the same pipe answers the room verb",
    "a review permit commits the journalled revision and the commit tree carries the edited source bytes",
    "a review permit refuses a same-revision decoy commit and commits the tracked source tree above it",
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
    let mut product = match Product::start(&project, &room_path, None, "product.err") {
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
    leg(&mut session, NAMES[9], |session| room_handoff(session, &mut product, &project, &room_path, &mut held));
    leg(&mut session, NAMES[10], |session| review_commit(session, &project, &room_path));
    leg(&mut session, NAMES[11], |session| decoy_commit(session, &project, &room_path));
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
    *product = Product::start(project, room, Some((CARRIED_JOB, CARRIED_INSTRUCTION)), "product.err")?;
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

fn room_handoff(
    session: &mut Session,
    product: &mut Product,
    project: &str,
    room: &str,
    held: &mut Held,
) -> Result<(), LegError> {
    let (ok, detail) = match drive_room_handoff(product, project, room, held) {
        Ok(outcome) => outcome,
        Err(error) => (false, error.full),
    };
    if ok {
        emit(session, false, &detail);
    }
    check(session, NAMES[9], ok, &detail);
    Ok(())
}

fn drive_room_handoff(
    product: &mut Product,
    project: &str,
    room: &str,
    held: &mut Held,
) -> Result<(bool, String), LegError> {
    let mut fact = Vec::new();
    let before = product.raw(&obj(vec![("op".into(), s(ROOM_VERB))]))?;
    let before_error = as_str(before.get("error")).unwrap_or("").to_string();
    fact.push(format!("before={before_error}"));
    if before_error != "Unknown operation" || before.get("value").is_some() {
        return Ok((false, fact.join(" ")));
    }

    let canary = sha256(format!("room-canary\n{room}\n{ROOM_VERB}").as_bytes());
    let source = successor_source(ROOM_VERB, &canary);
    std::fs::write(Path::new(room).join("headless.sh"), ROOM_HEADLESS)
        .map_err(|error| LegError::runtime(format!("headless.sh: {error}")))?;
    let built = propose(
        product,
        state_of(held)?,
        room,
        base_args(
            &source,
            vec![
                ("path", s(SUCCESSOR_SOURCE)),
                ("builder", s("bash")),
                ("builder_args", Value::Arr(vec![s("headless.sh")])),
                ("gate", s(GATE)),
                ("gate_args", Value::Arr(vec![s(UNITS)])),
                ("job", s("lane-handoff")),
                ("instruction", s("build the room successor")),
                ("timeout_seconds", n("180")),
                ("reload", s("")),
            ],
        ),
    )?;
    let (approved, effect, answer) = approve_and_run(product, room, &built)?;
    if let Some(error) = answer.get("error") {
        let _ = settle(product, room, &approved, &effect, &answer);
        fact.push(format!("build={}", tail(&error.to_string(), 800)));
        return Ok((false, fact.join(" ")));
    }
    let receipt = answer_text(&answer).unwrap_or_default();
    let Some(gate_line) = gate_sentence(&receipt) else {
        let _ = settle(product, room, &approved, &effect, &answer);
        fact.push(format!("build={}", tail(&receipt, 800)));
        return Ok((false, fact.join(" ")));
    };
    let done = settle(product, room, &approved, &effect, &answer)?;
    adopt(held, &done)?;

    let pre_edit = Path::new(project).join("output/headless/RLLM");
    let pre_edit_path = pre_edit.to_str().unwrap_or("").to_string();
    let successor_path = Path::new(room).join(SUCCESSOR_BINARY);
    let pre_file = file_digest(&pre_edit)?;
    let successor_file = file_digest(&successor_path)?;
    let finger = product.raw(&obj(vec![
        ("op".into(), s("self_fingerprint")),
        ("root".into(), s(room)),
        ("binary".into(), s(&pre_edit_path)),
        ("journal".into(), s(HANDOFF_JOURNAL)),
        ("rollback".into(), s(HANDOFF_ROLLBACK)),
    ]))?;
    if let Some(error) = finger.get("error") {
        fact.push(format!("fingerprint={}", tail(&error.to_string(), 400)));
        return Ok((false, fact.join(" ")));
    }
    let pre_digest = as_str(finger.get("value").and_then(|value| value.get("binary_digest"))).unwrap_or("").to_string();
    let source_digest = as_str(finger.get("value").and_then(|value| value.get("source_digest"))).unwrap_or("").to_string();
    fact.push(format!("pre_edit={pre_digest}"));

    let successor_abs = successor_path.to_str().unwrap_or("").to_string();
    let journaled = product.raw(&obj(vec![
        ("op".into(), s("self_journal")),
        ("root".into(), s(room)),
        ("binary".into(), s(&pre_edit_path)),
        ("previous_binary".into(), s(&successor_abs)),
        ("successor_binary".into(), s(SUCCESSOR_BINARY)),
        ("journal".into(), s(HANDOFF_JOURNAL)),
        ("rollback".into(), s(HANDOFF_ROLLBACK)),
        ("parent".into(), s("")),
        ("gate_output".into(), s(&gate_line)),
        ("review_confirmed".into(), Value::Bool(true)),
        ("source_digest".into(), s(&source_digest)),
    ]))?;
    if let Some(error) = journaled.get("error") {
        fact.push(format!("journal={}", tail(&error.to_string(), 400)));
        return Ok((false, fact.join(" ")));
    }
    let recorded = as_str(
        journaled
            .get("value")
            .and_then(|value| value.get("successor"))
            .and_then(|value| value.get("binary_digest")),
    )
    .unwrap_or("")
    .to_string();
    let journal_recorded = journaled.get("value").and_then(|value| value.get("recorded")) == Some(&Value::Bool(true));
    fact.push(format!("successor={recorded}"));
    fact.push(format!("recorded={journal_recorded}"));

    let refused = product.raw(&handoff_request(room, &source_digest, false))?;
    if let Some(error) = refused.get("error") {
        fact.push(format!("unconfirmed={}", tail(&error.to_string(), 400)));
        return Ok((false, fact.join(" ")));
    }
    let refusal_digest = as_str(refused.get("value").and_then(|value| value.get("binary_digest"))).unwrap_or("").to_string();
    let rows = refused.get("value").and_then(|value| value.get("successor_rows")).map(Value::to_string).unwrap_or_else(|| "missing".into());
    let tip = as_str(refused.get("value").and_then(|value| value.get("successor_tip"))).unwrap_or("").to_string();
    let successors = match refused.get("value").and_then(|value| value.get("successors")) {
        Some(Value::Arr(items)) if items.is_empty() => "[]".to_string(),
        Some(Value::Arr(items)) => format!("array:{}", items.len()),
        _ => "missing".to_string(),
    };
    let executed = refused
        .get("value")
        .and_then(|value| value.get("executed"))
        .map(Value::to_string)
        .unwrap_or_else(|| "missing".into());
    fact.push(format!("executed={executed}"));
    fact.push(format!("successor_rows={rows}"));
    fact.push(format!("successor_tip={tip:?}"));
    fact.push(format!("successors={successors}"));
    fact.push(format!("refusal_digest={refusal_digest}"));
    let still = product.raw(&obj(vec![("op".into(), s(ROOM_VERB))]))?;
    let still_error = as_str(still.get("error")).unwrap_or("").to_string();
    fact.push(format!("after_unconfirmed={still_error}"));

    // The permit arm execs over this pipe. The old image consumes the handoff
    // line and writes no reply. The next line is sent only after the process
    // image changes, so the old buffered reader cannot take it.
    let Some(before_image) = process_image(product.pid()) else {
        fact.push("image=unreadable".into());
        return Ok((false, fact.join(" ")));
    };
    product.send_line(&handoff_request(room, &source_digest, true))?;
    let replaced = image_replaced(product.pid(), &before_image);
    fact.push(format!("replaced={replaced}"));
    if !replaced {
        return Ok((false, fact.join(" ")));
    }
    let answered = product.raw(&obj(vec![("op".into(), s(ROOM_VERB))]))?;
    let answered_canary = as_str(answered.get("value").and_then(|value| value.get("canary"))).unwrap_or("").to_string();
    let answered_error = as_str(answered.get("error")).unwrap_or("").to_string();
    fact.push(format!("canary={canary}"));
    fact.push(format!("answered={answered_canary}"));
    if !answered_error.is_empty() {
        fact.push(format!("answered_error={answered_error}"));
    }
    let post = product.raw(&obj(vec![
        ("op".into(), s("self_fingerprint")),
        ("root".into(), s(room)),
        ("binary".into(), s(&successor_abs)),
        ("journal".into(), s(HANDOFF_JOURNAL)),
        ("rollback".into(), s(HANDOFF_ROLLBACK)),
    ]))?;
    if let Some(error) = post.get("error") {
        fact.push(format!("post_error={}", tail(&error.to_string(), 400)));
        return Ok((false, fact.join(" ")));
    }
    let post_digest = as_str(post.get("value").and_then(|value| value.get("binary_digest"))).unwrap_or("").to_string();
    fact.push(format!("post={post_digest}"));

    let state_carried = matches!(refused.get("value").and_then(|value| value.get("successors")), Some(Value::Arr(_)))
        && matches!(refused.get("value").and_then(|value| value.get("successor_rows")), Some(Value::Num(_)))
        && matches!(refused.get("value").and_then(|value| value.get("successor_tip")), Some(Value::Str(_)));
    let ok = before_error == "Unknown operation"
        && pre_digest == pre_file
        && pre_digest.len() == 64
        && journal_recorded
        && recorded == successor_file
        && recorded != pre_digest
        && refusal_digest == recorded
        && executed == "false"
        && state_carried
        && still_error == "Unknown operation"
        && still.get("value").is_none()
        && replaced
        && answered_canary == canary
        && answered_error.is_empty()
        && post_digest == recorded
        && post_digest != pre_digest;
    Ok((ok, fact.join(" ")))
}

fn handoff_request(room: &str, source_digest: &str, permit: bool) -> Value {
    obj(vec![
        ("op".into(), s("self_handoff")),
        ("root".into(), s(room)),
        ("binary".into(), s(SUCCESSOR_BINARY)),
        ("args".into(), Value::Arr(vec![s("--jsonl")])),
        ("source_digest".into(), s(source_digest)),
        ("review_confirmed".into(), Value::Bool(permit)),
        ("journal".into(), s(HANDOFF_JOURNAL)),
        ("rollback".into(), s(HANDOFF_ROLLBACK)),
    ])
}

fn review_commit(session: &mut Session, project: &str, room: &str) -> Result<(), LegError> {
    let mut product = match Product::start(project, room, None, "output/commit-product.err") {
        Ok(product) => product,
        Err(error) => {
            check(session, NAMES[10], false, &error.full);
            return Ok(());
        }
    };
    let (ok, detail) = match drive_review_commit(&mut product, project, room) {
        Ok(outcome) => outcome,
        Err(error) => (false, error.full),
    };
    product.close();
    if ok {
        emit(session, false, &detail);
    }
    check(session, NAMES[10], ok, &detail);
    Ok(())
}

fn drive_review_commit(product: &mut Product, project: &str, room: &str) -> Result<(bool, String), LegError> {
    let mut fact = Vec::new();
    let room_path = Path::new(room);
    git_room(room_path, &["init", "-b", "main"])?;
    git_room(room_path, &["add", "--", SOURCE])?;
    git_room(
        room_path,
        &[
            "-c",
            "user.name=RLLM",
            "-c",
            "user.email=rllm@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "commit",
            "-m",
            "seed",
        ],
    )?;
    let edited = format!("fn marker() {{ let _ = \"{COMMIT_MARKER}\"; }}\n");
    std::fs::write(room_path.join(SOURCE), &edited).map_err(|error| LegError::runtime(format!("edited source: {error}")))?;
    let binary = Path::new(project).join("output/headless/RLLM");
    let binary = binary.to_str().ok_or_else(|| LegError::runtime("binary path is not UTF-8"))?.to_string();
    let finger = product.raw(&obj(vec![
        ("op".into(), s("self_fingerprint")),
        ("root".into(), s(room)),
        ("binary".into(), s(&binary)),
        ("journal".into(), s(COMMIT_JOURNAL)),
        ("rollback".into(), s(COMMIT_ROLLBACK)),
    ]))?;
    if let Some(error) = finger.get("error") {
        fact.push(format!("fingerprint={}", tail(&error.to_string(), 400)));
        return observe(false, &fact);
    }
    let source_digest = as_str(finger.get("value").and_then(|value| value.get("source_digest"))).unwrap_or("").to_string();
    let journaled = product.raw(&obj(vec![
        ("op".into(), s("self_journal")),
        ("root".into(), s(room)),
        ("binary".into(), s(&binary)),
        ("previous_binary".into(), s("successor.sh")),
        ("journal".into(), s(COMMIT_JOURNAL)),
        ("rollback".into(), s(COMMIT_ROLLBACK)),
        ("parent".into(), s("")),
        ("gate_output".into(), s("Rust cases passed: 1/1; isolated application cases: 1")),
        ("review_confirmed".into(), Value::Bool(true)),
        ("source_digest".into(), s(&source_digest)),
    ]))?;
    if let Some(error) = journaled.get("error") {
        fact.push(format!("journal={}", tail(&error.to_string(), 400)));
        return observe(false, &fact);
    }
    let revision = as_str(journaled.get("value").and_then(|value| value.get("revision"))).unwrap_or("").to_string();
    let journal_source = as_str(journaled.get("value").and_then(|value| value.get("source_digest"))).unwrap_or("").to_string();
    let recorded = journaled.get("value").and_then(|value| value.get("recorded")) == Some(&Value::Bool(true));
    fact.push(format!("journal_revision={revision}"));
    fact.push(format!("journal_source={journal_source}"));
    fact.push(format!("recorded={recorded}"));

    let refs_before = snapshot_refs(room_path)?;
    let head_before = room_ref(room_path)?;
    let denied = product.raw(&commit_request(room, &binary, &revision, false))?;
    if let Some(error) = denied.get("error") {
        fact.push(format!("unpermitted={}", tail(&error.to_string(), 400)));
        return observe(false, &fact);
    }
    let denied_value = denied.get("value").cloned().unwrap_or(Value::Null);
    let refs_after_denial = snapshot_refs(room_path)?;
    let head_after_denial = room_ref(room_path)?;
    let denied_paths = path_list(&denied_value);
    let denied_revision = as_str(denied_value.get("revision")).unwrap_or("").to_string();
    let denied_parent = as_str(denied_value.get("parent_commit")).unwrap_or("").to_string();
    let denied_source = as_str(denied_value.get("source_digest")).unwrap_or("").to_string();
    let denied_names = denied_paths.clone().unwrap_or_default();
    fact.push(format!("unpermitted_commit={}", if commit_absent(&denied_value) { "absent" } else { "present" }));
    fact.push(format!("unpermitted_revision={denied_revision}"));
    fact.push(format!("unpermitted_parent_commit={denied_parent}"));
    fact.push(format!("unpermitted_source_digest={denied_source}"));
    fact.push(format!("unpermitted_committed_paths={} {}", denied_names.len(), denied_names.join(",")));
    fact.push(format!("refs_equal={}", refs_before == refs_after_denial));
    fact.push(format!("ref_before={head_before}"));
    fact.push(format!("ref_after_unpermitted={head_after_denial}"));
    let intent = commit_intent(room_path, &revision)?;
    fact.push(format!("intent={intent}"));

    let permitted = product.raw(&commit_request(room, &binary, &revision, true))?;
    if let Some(error) = permitted.get("error") {
        fact.push(format!("permitted={}", tail(&error.to_string(), 400)));
        return observe(false, &fact);
    }
    let permitted_value = permitted.get("value").cloned().unwrap_or(Value::Null);
    let commit = commit_text(&permitted_value);
    let parent_commit = as_str(permitted_value.get("parent_commit")).unwrap_or("").to_string();
    let committed_revision = as_str(permitted_value.get("revision")).unwrap_or("").to_string();
    let committed_source = as_str(permitted_value.get("source_digest")).unwrap_or("").to_string();
    let committed_paths = path_list(&permitted_value).unwrap_or_default();
    let confirmed = permitted_value.get("review_confirmed") == Some(&Value::Bool(true));
    let room_head = room_ref(room_path)?;
    let rev_head = git_room(room_path, &["rev-parse", "HEAD"])?.trim().to_string();
    fact.push(format!("commit={commit}"));
    fact.push(format!("revision={committed_revision}"));
    fact.push(format!("parent_commit={parent_commit}"));
    fact.push(format!("source_digest={committed_source}"));
    fact.push(format!("committed_paths={} {}", committed_paths.len(), committed_paths.join(",")));
    fact.push(format!("room_ref={room_head}"));
    fact.push(format!("rev_parse={rev_head}"));
    fact.push(format!("review_confirmed={confirmed}"));

    let commit_object = if is_hex(&commit, 40) {
        git_room(room_path, &["cat-file", "-p", &commit]).unwrap_or_default()
    } else {
        String::new()
    };
    let tree = tree_hash(&commit_object).unwrap_or_default();
    let tree_object = if is_hex(&tree, 40) {
        git_room(room_path, &["cat-file", "-p", &tree]).unwrap_or_default()
    } else {
        String::new()
    };
    let blob = if is_hex(&tree, 40) {
        blob_at_path(room_path, &tree, SOURCE).unwrap_or_default()
    } else {
        String::new()
    };
    let blob_bytes = if is_hex(&blob, 40) {
        git_room(room_path, &["cat-file", "-p", &blob]).unwrap_or_default()
    } else {
        String::new()
    };
    let linked = if is_hex(&commit, 40) {
        git_room(room_path, &["rev-parse", &format!("{commit}:{SOURCE}")]).unwrap_or_default().trim().to_string()
    } else {
        String::new()
    };
    let parent_blob = if is_hex(&parent_commit, 40) {
        git_room(room_path, &["cat-file", "-p", &format!("{parent_commit}:{SOURCE}")]).unwrap_or_default()
    } else {
        String::new()
    };
    let marker_in_blob = blob_bytes.contains(COMMIT_MARKER);
    let marker_in_parent = parent_blob.contains(COMMIT_MARKER);
    fact.push(format!("tree={tree}"));
    fact.push(format!("blob={blob}"));
    fact.push(format!("linked_blob={linked}"));
    fact.push(format!("marker_in_blob={marker_in_blob}"));
    fact.push(format!("marker_in_parent_blob={marker_in_parent}"));
    fact.push(format!("blob_equals_edit={}", blob_bytes == edited));

    let refs_after_commit = snapshot_refs(room_path)?;
    let again = product.raw(&commit_request(room, &binary, &revision, true))?;
    let again_error = as_str(again.get("error")).unwrap_or("").to_string();
    let again_commit = commit_text(again.get("value").unwrap_or(&Value::Null));
    let head_after_second = room_ref(room_path)?;
    let refs_after_second = snapshot_refs(room_path)?;
    fact.push(format!("second_commit={again_commit}"));
    fact.push(format!("second_error={again_error}"));
    fact.push(format!("second_same={}", again_commit == commit));
    fact.push(format!("ref_after_second={head_after_second}"));
    fact.push(format!("refs_stable_after_second={}", refs_after_commit == refs_after_second));

    let ok = recorded
        && is_hex(&revision, 64)
        && journal_source == source_digest
        && is_hex(&source_digest, 64)
        && commit_absent(&denied_value)
        && denied_value.get("review_confirmed") == Some(&Value::Bool(false))
        && denied_revision == revision
        && denied_parent == head_before
        && denied_source == source_digest
        && denied_paths.is_some()
        && refs_before == refs_after_denial
        && head_before == head_after_denial
        && is_hex(&head_before, 40)
        && intent
        && is_hex(&commit, 40)
        && parent_commit == head_before
        && is_hex(&parent_commit, 40)
        && committed_revision == revision
        && committed_source == source_digest
        && confirmed
        && committed_paths == vec![SOURCE.to_string()]
        && room_head == commit
        && rev_head == commit
        && is_hex(&tree, 40)
        && tree_has_entry(&tree_object, "src", "tree")
        && blob == linked
        && is_hex(&blob, 40)
        && marker_in_blob
        && blob_bytes == edited
        && !marker_in_parent
        && again_error.is_empty()
        && again_commit == commit
        && head_after_second == commit
        && refs_after_commit == refs_after_second;
    observe(ok, &fact)
}

fn commit_request(room: &str, binary: &str, revision: &str, permit: bool) -> Value {
    obj(vec![
        ("op".into(), s("self_commit")),
        ("root".into(), s(room)),
        ("binary".into(), s(binary)),
        ("revision".into(), s(revision)),
        ("journal".into(), s(COMMIT_JOURNAL)),
        ("parent".into(), s("")),
        ("review_confirmed".into(), Value::Bool(permit)),
    ])
}

fn decoy_commit(session: &mut Session, project: &str, room: &str) -> Result<(), LegError> {
    let mut product = match Product::start(project, room, None, "output/decoy-product.err") {
        Ok(product) => product,
        Err(error) => {
            check(session, NAMES[11], false, &error.full);
            return Ok(());
        }
    };
    let (ok, detail) = match drive_decoy_commit(&mut product, project, room) {
        Ok(outcome) => outcome,
        Err(error) => (false, error.full),
    };
    product.close();
    if ok {
        emit(session, false, &detail);
    }
    check(session, NAMES[11], ok, &detail);
    Ok(())
}

/// The room HEAD carries the subject of the journalled revision but not the room's
/// tracked source path set. The permit must refuse that commit and make its own
/// commit above it, so the head tree path check carries the refusal.
fn drive_decoy_commit(product: &mut Product, project: &str, room: &str) -> Result<(bool, String), LegError> {
    let mut fact = Vec::new();
    let room_path = Path::new(room);
    git_room(room_path, &["init", "-b", "main"])?;
    let edited = format!("fn marker() {{ let _ = \"{DECOY_MARKER}\"; }}\n");
    std::fs::write(room_path.join(SOURCE), &edited).map_err(|error| LegError::runtime(format!("edited source: {error}")))?;
    git_room(room_path, &["add", "--", SOURCE])?;
    // A tracked path the worktree no longer carries. The index keeps the entry, so the
    // tracked path set and the head tree path set differ while the worktree stays clean.
    let notes = room_path.join("notes");
    std::fs::create_dir_all(&notes).map_err(|error| LegError::runtime(format!("notes directory: {error}")))?;
    std::fs::write(notes.join("decoy.txt"), "decoy\n").map_err(|error| LegError::runtime(format!("decoy path: {error}")))?;
    git_room(room_path, &["add", "--", DECOY_PATH])?;
    std::fs::remove_file(notes.join("decoy.txt")).map_err(|error| LegError::runtime(format!("decoy removal: {error}")))?;
    // A second staged path under the skipped output tree keeps the room's own commit
    // from being empty, so the refusal is answered with a commit and not with a failure.
    std::fs::write(room_path.join(DECOY_OUTPUT), "decoy\n").map_err(|error| LegError::runtime(format!("decoy output: {error}")))?;
    git_room(room_path, &["add", "-f", "--", DECOY_OUTPUT])?;
    std::fs::remove_file(room_path.join(DECOY_OUTPUT)).map_err(|error| LegError::runtime(format!("decoy output removal: {error}")))?;
    let binary = Path::new(project).join("output/headless/RLLM");
    let binary = binary.to_str().ok_or_else(|| LegError::runtime("binary path is not UTF-8"))?.to_string();
    let finger = product.raw(&obj(vec![
        ("op".into(), s("self_fingerprint")),
        ("root".into(), s(room)),
        ("binary".into(), s(&binary)),
        ("journal".into(), s(DECOY_JOURNAL)),
        ("rollback".into(), s(DECOY_ROLLBACK)),
    ]))?;
    if let Some(error) = finger.get("error") {
        fact.push(format!("fingerprint={}", tail(&error.to_string(), 400)));
        return observe(false, &fact);
    }
    let source_digest = as_str(finger.get("value").and_then(|value| value.get("source_digest"))).unwrap_or("").to_string();
    let journaled = product.raw(&obj(vec![
        ("op".into(), s("self_journal")),
        ("root".into(), s(room)),
        ("binary".into(), s(&binary)),
        ("previous_binary".into(), s("successor.sh")),
        ("journal".into(), s(DECOY_JOURNAL)),
        ("rollback".into(), s(DECOY_ROLLBACK)),
        ("parent".into(), s("")),
        ("gate_output".into(), s("Rust cases passed: 1/1; isolated application cases: 1")),
        ("review_confirmed".into(), Value::Bool(true)),
        ("source_digest".into(), s(&source_digest)),
    ]))?;
    if let Some(error) = journaled.get("error") {
        fact.push(format!("journal={}", tail(&error.to_string(), 400)));
        return observe(false, &fact);
    }
    let revision = as_str(journaled.get("value").and_then(|value| value.get("revision"))).unwrap_or("").to_string();
    let journal_source = as_str(journaled.get("value").and_then(|value| value.get("source_digest"))).unwrap_or("").to_string();
    fact.push(format!("revision={revision}"));
    fact.push(format!("source_digest={source_digest}"));
    fact.push(format!("journal_source={journal_source}"));
    let subject = format!("self-commit {revision}");
    let head_before = room_ref(room_path)?;
    git_room(
        room_path,
        &[
            "-c",
            "user.name=RLLM",
            "-c",
            "user.email=rllm@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "commit",
            "-m",
            subject.as_str(),
            "--",
            SOURCE,
        ],
    )?;
    let decoy = git_room(room_path, &["rev-parse", "HEAD"])?.trim().to_string();
    let decoy_subject = git_room(room_path, &["log", "-1", "--format=%s"])?.trim().to_string();
    let decoy_tree = git_room(room_path, &["ls-tree", "-r", "--name-only", "HEAD"])?;
    let index_paths = git_room(room_path, &["ls-files"])?;
    let clean = git_room(room_path, &["diff", "--quiet", "HEAD"]).is_ok();
    let tracked_paths: Vec<&str> = index_paths.lines().collect();
    let tree_paths: Vec<&str> = decoy_tree.lines().collect();
    fact.push(format!("head_before={head_before}"));
    fact.push(format!("decoy={decoy}"));
    fact.push(format!("decoy_subject={decoy_subject}"));
    fact.push(format!("decoy_tree={}", tree_paths.join(",")));
    fact.push(format!("tracked_paths={}", tracked_paths.join(",")));
    fact.push(format!("worktree_clean={clean}"));
    let permitted = product.raw(&decoy_commit_request(room, &binary, &revision))?;
    if let Some(error) = permitted.get("error") {
        fact.push(format!("permitted={}", tail(&error.to_string(), 400)));
        return observe(false, &fact);
    }
    let permitted_value = permitted.get("value").cloned().unwrap_or(Value::Null);
    let commit = commit_text(&permitted_value);
    let parent_commit = as_str(permitted_value.get("parent_commit")).unwrap_or("").to_string();
    let committed_source = as_str(permitted_value.get("source_digest")).unwrap_or("").to_string();
    let committed_paths = path_list(&permitted_value).unwrap_or_default();
    let confirmed = permitted_value.get("review_confirmed") == Some(&Value::Bool(true));
    let head_after = room_ref(room_path)?;
    let head_subject = git_room(room_path, &["log", "-1", "--format=%s"])?.trim().to_string();
    let commit_object = git_room(room_path, &["cat-file", "-p", commit.as_str()]).unwrap_or_default();
    let tree = tree_hash(&commit_object).unwrap_or_default();
    let blob = blob_at_path(room_path, &tree, SOURCE).unwrap_or_default();
    let blob_bytes = git_room(room_path, &["cat-file", "-p", blob.as_str()]).unwrap_or_default();
    let parent_ref = format!("{commit}^");
    let commit_parent = git_room(room_path, &["rev-parse", parent_ref.as_str()]).unwrap_or_default().trim().to_string();
    fact.push(format!("commit={commit}"));
    fact.push(format!("parent_commit={parent_commit}"));
    fact.push(format!("commit_parent={commit_parent}"));
    fact.push(format!("committed_paths={} {}", committed_paths.len(), committed_paths.join(",")));
    fact.push(format!("committed_source={committed_source}"));
    fact.push(format!("review_confirmed={confirmed}"));
    fact.push(format!("head_after={head_after}"));
    fact.push(format!("head_subject={head_subject}"));
    fact.push(format!("blob_equals_edit={}", blob_bytes == edited));
    let ok = is_hex(&revision, 64)
        && is_hex(&source_digest, 64)
        && journal_source == source_digest
        && decoy != head_before
        && decoy_subject == subject
        && clean
        && tracked_paths.contains(&DECOY_PATH)
        && !tree_paths.contains(&DECOY_PATH)
        && tree_paths.contains(&SOURCE)
        && is_hex(&decoy, 40)
        && is_hex(&commit, 40)
        && commit != decoy
        && parent_commit == decoy
        && commit_parent == decoy
        && head_after == commit
        && head_subject == subject
        && committed_paths == vec![SOURCE.to_string()]
        && committed_source == source_digest
        && confirmed
        && blob_bytes == edited
        && blob_bytes.contains(DECOY_MARKER);
    observe(ok, &fact)
}

fn decoy_commit_request(room: &str, binary: &str, revision: &str) -> Value {
    obj(vec![
        ("op".into(), s("self_commit")),
        ("root".into(), s(room)),
        ("binary".into(), s(binary)),
        ("revision".into(), s(revision)),
        ("journal".into(), s(DECOY_JOURNAL)),
        ("parent".into(), s("")),
        ("review_confirmed".into(), Value::Bool(true)),
    ])
}

fn observe(ok: bool, fact: &[String]) -> Result<(bool, String), LegError> {
    let report = fact.join(" ");
    evidence(&report);
    Ok((ok, report))
}

fn evidence(body: &str) {
    let flat: String = body.chars().map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch }).collect();
    println!("evidence {flat}");
    let _ = std::io::stdout().flush();
}

fn git_room(room: &Path, args: &[&str]) -> Result<String, LegError> {
    let mut full = vec!["-c", "safe.directory=*"];
    full.extend_from_slice(args);
    let output = Command::new("git")
        .args(&full)
        .current_dir(room)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_NAMESPACE")
        .output()
        .map_err(|error| LegError::runtime(format!("git {}: {error}", args.first().copied().unwrap_or("git"))))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().map(|code| code.to_string()).unwrap_or_else(|| "signal".into());
        return Err(LegError::runtime(format!("git {} exited {code}: {}", args.join(" "), tail(detail.trim(), 500))));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn snapshot_refs(room: &Path) -> Result<Vec<u8>, LegError> {
    let git = room.join(".git");
    let mut files = Vec::new();
    let head = std::fs::read(git.join("HEAD")).map_err(|error| LegError::runtime(format!("HEAD: {error}")))?;
    files.push(("HEAD".to_string(), head));
    let packed = git.join("packed-refs");
    if packed.is_file() {
        let bytes = std::fs::read(&packed).map_err(|error| LegError::runtime(format!("packed-refs: {error}")))?;
        files.push(("packed-refs".to_string(), bytes));
    }
    let mut stack = vec![git.join("refs")];
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        let entries = std::fs::read_dir(&dir).map_err(|error| LegError::runtime(format!("refs {}: {error}", dir.display())))?;
        for entry in entries {
            let entry = entry.map_err(|error| LegError::runtime(format!("refs entry: {error}")))?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|error| LegError::runtime(format!("refs stat: {error}")))?;
            if meta.file_type().is_symlink() {
                return Err(LegError::runtime(format!("git ref is a symlink {}", path.display())));
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let rel = path.strip_prefix(&git).map_err(|_| LegError::runtime("ref path escaped .git"))?;
            let name = rel.to_str().ok_or_else(|| LegError::runtime("ref path is not UTF-8"))?.to_string();
            let bytes = std::fs::read(&path).map_err(|error| LegError::runtime(format!("ref read: {error}")))?;
            files.push((name, bytes));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut out = Vec::new();
    for (name, bytes) in files {
        out.extend(name.as_bytes());
        out.push(0);
        out.extend(bytes);
        out.push(0);
    }
    Ok(out)
}

fn room_ref(room: &Path) -> Result<String, LegError> {
    let git = room.join(".git");
    let head = std::fs::read_to_string(git.join("HEAD")).map_err(|error| LegError::runtime(format!("HEAD: {error}")))?;
    let head = head.trim();
    let name = head.strip_prefix("ref: ").ok_or_else(|| LegError::runtime(format!("HEAD is not a branch ref {head}")))?;
    if name.split('/').any(|part| part.is_empty() || part == "." || part == "..") || name.starts_with('/') {
        return Err(LegError::runtime("HEAD ref escapes the repository"));
    }
    let text = std::fs::read_to_string(git.join(name)).map_err(|error| LegError::runtime(format!("ref {name}: {error}")))?;
    Ok(text.trim().to_string())
}

fn commit_intent(room: &Path, revision: &str) -> Result<bool, LegError> {
    let text = std::fs::read_to_string(room.join(COMMIT_JOURNAL)).map_err(|error| LegError::runtime(format!("commit journal: {error}")))?;
    let mut found = false;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let row = Value::parse(line);
        if as_str(row.get("intent")) == Some("self-commit")
            && as_str(row.get("target")) == Some(revision)
            && row.get("review_confirmed") == Some(&Value::Bool(false))
        {
            found = true;
        }
    }
    Ok(found)
}

fn commit_text(value: &Value) -> String {
    match value.get("commit") {
        None | Some(Value::Null) => String::new(),
        Some(Value::Str(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn commit_absent(value: &Value) -> bool {
    match value.get("commit") {
        None | Some(Value::Null) => true,
        Some(Value::Str(text)) => text.is_empty(),
        Some(_) => false,
    }
}

fn path_list(value: &Value) -> Option<Vec<String>> {
    let Value::Arr(items) = value.get("committed_paths")? else {
        return None;
    };
    let mut paths = Vec::new();
    for item in items {
        let Value::Str(path) = item else {
            return None;
        };
        paths.push(path.clone());
    }
    Some(paths)
}

fn tree_hash(commit_object: &str) -> Option<String> {
    for line in commit_object.lines() {
        let hash = line.strip_prefix("tree ")?.trim();
        if is_hex(hash, 40) {
            return Some(hash.to_string());
        }
    }
    None
}

fn tree_entry(tree_object: &str, name: &str) -> Option<(String, String)> {
    for line in tree_object.lines() {
        let (meta, entry) = line.split_once('\t')?;
        if entry != name {
            continue;
        }
        let mut words = meta.split_whitespace();
        let _mode = words.next()?;
        let kind = words.next()?;
        let hash = words.next()?;
        if is_hex(hash, 40) {
            return Some((kind.to_string(), hash.to_string()));
        }
    }
    None
}

fn tree_has_entry(tree_object: &str, name: &str, kind: &str) -> bool {
    tree_entry(tree_object, name).is_some_and(|(entry_kind, _)| entry_kind == kind)
}

/// Walk `commit`'s tree with `cat-file -p` until `path` resolves to a blob.
fn blob_at_path(room: &Path, tree: &str, path: &str) -> Result<String, LegError> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return Ok(String::new());
    }
    let mut cursor = tree.to_string();
    for (index, part) in parts.iter().enumerate() {
        let listing = git_room(room, &["cat-file", "-p", &cursor])?;
        let last = index + 1 == parts.len();
        let Some((kind, hash)) = tree_entry(&listing, part) else {
            return Ok(String::new());
        };
        if last {
            if kind != "blob" {
                return Ok(String::new());
            }
            return Ok(hash);
        }
        if kind != "tree" {
            return Ok(String::new());
        }
        cursor = hash;
    }
    Ok(String::new())
}

fn is_hex(value: &str, width: usize) -> bool {
    value.len() == width && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn gate_sentence(text: &str) -> Option<String> {
    let start = text.find("Rust cases passed:")?;
    let rest = &text[start..];
    let end = rest.find(['\n', '\r']).unwrap_or(rest.len());
    let line = rest[..end].trim();
    if line.contains("isolated application cases:") {
        Some(line.to_string())
    } else {
        None
    }
}

fn file_digest(path: &Path) -> Result<String, LegError> {
    let bytes = std::fs::read(path).map_err(|error| LegError::runtime(format!("read {}: {error}", path.display())))?;
    Ok(sha256(&bytes))
}

fn process_image(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(Path::new("/proc").join(pid.to_string()).join("exe")).ok()
}

fn image_replaced(pid: u32, before: &Path) -> bool {
    let proc_dir = Path::new("/proc").join(pid.to_string());
    for _ in 0..800 {
        if !proc_dir.exists() {
            return false;
        }
        if let Some(path) = process_image(pid) {
            if path != before {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn successor_source(verb: &str, canary: &str) -> String {
    let source = SUCCESSOR_TEMPLATE.replace("@@VERB@@", verb).replace("@@CANARY@@", canary);
    if source.contains("@@") {
        panic!("successor template placeholder left in place");
    }
    source
}

const ROOM_HEADLESS: &str = r#"#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")" && pwd)
: "${CLIENT_RUST:?Set CLIENT_RUST to the Rust toolchain directory}"
mkdir -p "$project_dir/output/headless"
LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
  "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name room_successor --crate-type bin \
  --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
  -C linker-features=-lld "$project_dir/core/room_successor.rs" -o "$project_dir/output/headless/RLLM"
printf '%s\n' "$project_dir/output/headless/RLLM"
"#;

const SUCCESSOR_TEMPLATE: &str = r##"use std::io::{BufRead, Write};

fn main() {
    if sha256(b"abc") != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
        eprintln!("successor sha256 abc mismatch");
        std::process::exit(2);
    }
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut writer = std::io::BufWriter::new(std::io::stdout().lock());
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {}
            Err(_) => return,
        }
        let op = field(&line, "op").unwrap_or("");
        let reply = if op == "@@VERB@@" {
            "{\"value\":{\"canary\":\"@@CANARY@@\"}}".to_string()
        } else if op == "self_fingerprint" {
            match field(&line, "binary") {
                Some(path) => match digest_file(path) {
                    Some(digest) => format!("{{\"value\":{{\"binary_digest\":\"{digest}\"}}}}"),
                    None => "{\"error\":\"binary unreadable\"}".to_string(),
                },
                None => "{\"error\":\"binary path is required\"}".to_string(),
            }
        } else {
            "{\"error\":\"Unknown operation\"}".to_string()
        };
        if writer.write_all(reply.as_bytes()).and_then(|_| writer.write_all(b"\n")).and_then(|_| writer.flush()).is_err() {
            return;
        }
    }
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    let start = line.find(&needle)? + needle.len();
    let bytes = line.as_bytes();
    let mut index = start;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = index.saturating_add(2);
            continue;
        }
        if bytes[index] == b'"' {
            return Some(&line[start..index]);
        }
        index += 1;
    }
    None
}

fn digest_file(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(sha256(&bytes))
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
"##;

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
    stderr_path: PathBuf,
}

impl Product {
    fn start(project: &str, room: &str, carried: Option<(&str, &str)>, stderr_name: &str) -> Result<Self, LegError> {
        let binary = Path::new(project).join("output/headless/RLLM");
        let mut command = Command::new(&binary);
        command.arg("--jsonl").current_dir(project).stdin(Stdio::piped()).stdout(Stdio::piped());
        let stderr_path = Path::new(room).join(stderr_name);
        if let Some(parent) = stderr_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| LegError::runtime(format!("product stderr: {error}")))?;
        }
        let error_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&stderr_path)
            .map_err(|error| LegError::runtime(format!("product stderr: {error}")))?;
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
            stderr_path,
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
                    let err_text = std::fs::read_to_string(&self.stderr_path).unwrap_or_default();
                    return Err(LegError::runtime(format!("product closed stdout in {} {}", self.room, tail(&err_text, 2000))));
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

    fn send_line(&mut self, envelope: &Value) -> Result<(), LegError> {
        let stdin = self.stdin.as_mut().unwrap_or_else(|| panic!("controller stdin is closed"));
        let line = envelope.to_string();
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|error| LegError::runtime(format!("controller write: {error}")))
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

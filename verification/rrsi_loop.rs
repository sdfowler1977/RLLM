//! Connected self-improvement loop checks through the product JSON-lines route.
mod twin;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use twin::{b64_encode, n, obj, s, Value};

const SCOPE: &str = "Owner checks through the product JSON-lines route: refine copies approval_root onto task_run.workspace and records source_digest, the digest-named ledger chains revisions for that run and experience.jsonl seals the settled row, a second round reads a recorded rejection, resume rebuilds the ledger prompt and decision from those files alone, a tampered library entry is refused without a rewrite, and delegate or compact carries no workspace and does not run the pool. A resumed refine records propose budgets 4/4, 3/3 and 3/1, refuses a repeated skills hypothesis by the round 1 name, records noise floor 0 and decision cost not recorded when a round gains nothing, leaves two flat skills rounds unpruned, stores the accepted incumbent as the rllm-evolve worktree commit, and keeps the separate evaluation score off the held-out keys. A three-round refine records budgets 4, 3 and 2, a leakage refusal before scoring, noise floor 2, pruned skills, the evolve commit, and the four held-out keys. A stalled ledger refuses an already-tried component while an untried component is not refused. A flat memory shape is accepted for structural novelty while its cost is unknown. The next equal score, one byte shorter, is accepted because the cost fell. The same shape one byte longer is refused because the cost rose. A later refine task reads the earlier task's sealed experience entry out of experience.jsonl in its own propose prompt, keyed to the earlier task rather than to itself, and the same later task over a library that holds no sealed row shows no historical experience section at all. The run also prints the values behind those claims — the annealed budget sentence quoted out of the wire of each of the six propose requests this group drives, labelled with the request it came from, the noise floor each ledger row of all three runs recorded, the pruned set the settled run reports and the absence the flat-skills run reports, the incumbent commit as the ledger row, the rllm-evolve ref and the worktree HEAD, the held-out task and verifier digests, and the sealed library row's key, revision and digests next to the empty library before it — and a final check certifies that this run's own log carries each of them on the line of the request or run that produced it.";

const TASK: &str = "Write the bounded note.";
const CHECKS: &str = "REQUIRED-ALPHA\nREQUIRED-BETA";
const CONTROL: &str = "CONTROL-RULES";
const REJECTED: &str = "REJECTED-REVISION";
const REFUSAL: &str = "refused because component prompt with that hypothesis was already rejected in round 1";
const STALL_CHECK: &str = "a stalled ledger refuses an already-tried component and does not refuse one that was never tried";
const SHAPE_CHECK: &str = "a flat memory shape is accepted for structural novelty, a shorter equal score is accepted for lower cost, and the same shape is refused when the cost rose";
const CROSS_TASK: &str = "Write the second bounded note.";
const CROSS_CHECK: &str = "a later refine task reads the earlier task's sealed experience entry from the propose prompt";
const EMPTY_CHECK: &str = "a refine task whose library holds no sealed row shows no historical experience section";
const LOG_CHECK: &str = "the run log prints the annealed budgets, the evolve commit three ways, the noise floors, the pruned set, the held-out digests and the sealed library row";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let room = scratch.path().canonicalize().unwrap_or_else(|error| panic!("room: {error}"));
    let mut product = Product::spawn(&root_text, &scratch.path().join("xdg"), &scratch.path().join("product.err"));
    let passed = run(&mut product, &room);
    product.finish();
    let names: Vec<Value> = passed.iter().map(|name| s(name)).collect();
    // Python json.dumps writes a trailing newline. The target report has one.
    twin::report_value(
        "rrsi_loop",
        &obj(vec![
            ("checks".into(), Value::Arr(names.clone())),
            ("count".into(), n(&names.len().to_string())),
            ("failed".into(), Value::Arr(Vec::new())),
            ("scope".into(), s(SCOPE)),
            ("passed".into(), Value::Arr(names)),
        ]),
        true,
    );
    println!("{} rrsi-loop checks passed", passed.len());
}

fn run(product: &mut Product, room: &Path) -> Vec<String> {
    let mut passed = Vec::new();
    let mut held = Held::default();
    let default_root = product.data_home.join("rllm");

    let prepared = fresh(product, room);
    let opened = event(product, Some(&prepared), "refine", obj(vec![("task".into(), s(TASK))]), room);
    let opened_state = state(&opened);
    let approval = opened_state.get("approval_root").and_then(as_str).unwrap_or("");
    let run = field_or_empty(opened_state, "task_run");
    let workspace = run.get("workspace").and_then(as_str).unwrap_or("");
    let source = run.get("source_digest").and_then(as_str).unwrap_or("");
    let expected = source_digest_of(Path::new(approval));
    held.after_entry = opened.clone();
    keep(
        &mut passed,
        "refine entry copies approval_root onto task_run.workspace and records source_digest",
        approval == room.to_str().unwrap_or("") && workspace == approval && source == expected && !expected.is_empty(),
    );

    event(product, Some(state(&held.after_entry)), "cancel", obj(vec![]), room);
    // No run has reached a review yet, so the library holds no sealed row and
    // the later task must be given no history at all.
    let empty_read = empty_library_read(product, room, &default_root);
    let before = files_here(&default_root);
    let prepared = fresh(product, room);
    let delegated = event(product, Some(&prepared), "delegate", obj(vec![("task".into(), s("Read the notes."))]), room);
    let delegate_state = state(&delegated);
    let delegate_run = field_or_empty(delegate_state, "task_run");
    let delegate_workspace = delegate_run.get("workspace").and_then(as_str).unwrap_or("");
    let pooled = reply(product, &delegated, "candidates: 1\nrewritten harness");
    let pooled_state = state(&pooled);
    let delegate_note = pooled_state.get("notice").and_then(as_str).unwrap_or("");
    let delegate_left = pooled_state.get("task_run");
    let mut chat_state = pooled_state.clone();
    let mut messages = active_messages(&chat_state);
    while messages.len() < 5 {
        let sent = event(
            product,
            Some(&chat_state),
            "send",
            obj(vec![("draft".into(), s(&format!("Extra line {}", messages.len())))]),
            room,
        );
        chat_state = state(&reply(product, &sent, &format!("Extra answer {}", messages.len()))).clone();
        messages = active_messages(&chat_state);
    }
    let compacted = event(product, Some(&chat_state), "compact", obj(vec![]), room);
    let compact_run = field_or_empty(state(&compacted), "task_run");
    let compact_workspace = compact_run.get("workspace").and_then(as_str).unwrap_or("");
    let done = reply(product, &compacted, "candidates: 2\nrewritten harness\n---\nanother rewrite");
    let done_state = state(&done);
    let summary = active_chat(done_state).get("context_summary").and_then(as_str).unwrap_or("");
    let after = files_here(&default_root);
    let grew: Vec<&String> = after.iter().filter(|name| !before.contains(*name)).collect();
    let mut pool_text = String::new();
    for name in &after {
        pool_text.push_str(&read_text(&default_root.join(name)));
    }
    let worktrees = room.join(".rllm-worktrees").exists() || default_root.join(".rllm-worktrees").exists();
    keep(
        &mut passed,
        "a non-refine run carries no workspace and does not attempt the pool",
        delegate_state.get("approval_root").and_then(as_str) == Some(room.to_str().unwrap_or(""))
            && delegate_run.get("mode").and_then(as_str) == Some("delegate")
            && delegate_workspace.is_empty()
            && is_nullish(delegate_left)
            && delegate_note == "Agent task result saved"
            && compact_run.get("mode").and_then(as_str) == Some("compact")
            && (compact_workspace.is_empty() || compact_run.get("workspace").is_none())
            && is_nullish(done_state.get("task_run"))
            && summary == "candidates: 2\nrewritten harness\n---\nanother rewrite"
            && grew.is_empty()
            && !pool_text.contains("pool_status")
            && !worktrees,
    );

    let prepared = fresh(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s(TASK)),
            ("response_checks".into(), s(CHECKS)),
            ("refinement_rounds".into(), s("2")),
        ]),
        room,
    );
    pending = reply(product, &pending, "Original reply.");
    let control = reply(product, &pending, CONTROL);
    let control_text = outcome_text(&control);
    pending = reply(product, &control, "First partial answer REQUIRED-ALPHA.");
    pending = reply(product, &pending, REJECTED);
    pending = reply(product, &pending, "First replay missed everything.");
    let treated = reply(product, &pending, REJECTED);
    let treated_text = outcome_text(&treated);
    held.refusal = treated.clone();
    keep(
        &mut passed,
        "a second round decision changes when the ledger already rejected that component",
        state(&control)
            .get("task_run")
            .and_then(|run| run.get("phase"))
            .and_then(as_str)
            == Some("probe")
            && !control_text.contains(REFUSAL)
            && treated_text.contains(REFUSAL)
            && control_text != treated_text
            && state(&treated).get("task_run").and_then(|run| run.get("phase")).and_then(as_str) != Some("probe"),
    );

    if held.refusal.get("state").is_some() {
        event(product, Some(state(&held.refusal)), "cancel", obj(vec![]), room);
    }
    let prepared = fresh(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s(TASK)),
            ("response_checks".into(), s(CHECKS)),
        ]),
        room,
    );
    for text in [
        "Partial report REQUIRED-ALPHA.",
        "Candidate rules for the bounded report.",
        "Completed report REQUIRED-ALPHA REQUIRED-BETA.",
        "Revision rules for the bounded report.",
        "Revised report missed the second phrase. REQUIRED-ALPHA.",
    ] {
        pending = reply(product, &pending, text);
    }
    let settled = field_or_empty(state(&pending), "refinement_pending");
    held.settled = settled.clone();
    let digest = settled.get("run_digest").and_then(as_str).unwrap_or("").to_string();
    let ledger_name = if digest.len() == 64 { format!("{digest}.jsonl") } else { String::new() };
    let ledger_rows = if ledger_name.is_empty() { Vec::new() } else { read_rows(&default_root.join(&ledger_name)) };
    let library_rows = read_rows(&default_root.join("experience.jsonl"));
    let linked = !ledger_rows.is_empty()
        && ledger_rows.iter().all(|row| row.get("run_digest").and_then(as_str) == Some(digest.as_str()) && row_seal(row));
    let chained = !ledger_rows.is_empty()
        && ledger_rows[0].get("previous").and_then(as_str) == Some("")
        && ledger_rows.iter().enumerate().skip(1).all(|(index, row)| {
            row.get("previous") == ledger_rows[index - 1].get("revision")
        });
    let this_library: Vec<&Value> = library_rows
        .iter()
        .filter(|row| row.get("run_digest").and_then(as_str) == Some(digest.as_str()))
        .collect();
    let library_ok = !this_library.is_empty()
        && this_library.iter().all(|row| row_seal(row))
        && this_library.iter().any(|row| row.get("harness") == settled.get("composition"));
    let named = settled.get("edit_ledger_root").and_then(as_str) == default_root.to_str()
        && settled.get("edit_ledger_path").and_then(as_str) == Some(ledger_name.as_str())
        && !ledger_name.is_empty()
        && ledger_name != "edits.jsonl";
    let no_fixed_name = !default_root.join("edits.jsonl").is_file();
    keep(
        &mut passed,
        "the digest-named ledger chains each revision to this run and experience.jsonl seals the settled row",
        named
            && linked
            && chained
            && library_ok
            && no_fixed_name
            && settled.get("phase").and_then(as_str) == Some("review"),
    );

    // The settled row is on disk now, so a later task with a different task
    // text must read that entry out of the library in its own propose prompt.
    let cross_read = cross_task_read(product, room, &default_root, &held.settled);

    product.restart(room).unwrap_or_else(|error| panic!("could not restart product: {error}"));
    let settled = held.settled.clone();
    let digest = settled.get("run_digest").and_then(as_str).unwrap_or("");
    let ledger_name = if digest.len() == 64 { format!("{digest}.jsonl") } else { String::new() };
    let rows = if ledger_name.is_empty() { Vec::new() } else { read_rows(&default_root.join(&ledger_name)) };
    let chosen = rows.iter().find(|row| {
        row.get("verdict").and_then(as_str) == Some("rejected")
            && row.get("run_digest").and_then(as_str) == Some(digest)
            && row.get("hypothesis").and_then(as_str).is_some_and(|text| !text.is_empty())
    });
    let hypothesis = chosen.and_then(|row| row.get("hypothesis")).and_then(as_str).unwrap_or("");
    let component = chosen.and_then(|row| row.get("component")).and_then(as_str).unwrap_or("");
    let round_no = chosen.and_then(|row| row.get("round"));
    let round_text = match round_no {
        Some(Value::Num(token)) => token.parse::<i64>().map(|value| value.to_string()).unwrap_or_default(),
        Some(Value::Str(token)) if token.chars().all(|ch| ch.is_ascii_digit()) => token.clone(),
        _ => String::new(),
    };
    let refusal = format!(
        "refused because component {component} with that hypothesis was already rejected in round {round_text}"
    );
    let harness = settled.get("composition").and_then(as_str).unwrap_or("");
    let prepared = fresh(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s(TASK)),
            ("response_checks".into(), s(CHECKS)),
            ("refinement_rounds".into(), s("2")),
        ]),
        room,
    );
    pending = reply(product, &pending, "Original reply.");
    let wire = effect_wire(&pending);
    let history_at = wire.find("EDIT HISTORY");
    let library_at = wire.find("HISTORICAL EXPERIENCE");
    let history = if let Some(history_at) = history_at {
        let end = match library_at {
            Some(library_at) if library_at > history_at => library_at,
            _ => wire.len(),
        };
        &wire[history_at..end]
    } else {
        ""
    };
    let library = if let Some(library_at) = library_at { &wire[library_at..] } else { "" };
    let library_read = library.contains("Adopted harness (historical)") && !harness.is_empty() && library.contains(&harness);
    let ledger_rebuilt = history.contains("Rejected hypotheses (do not propose these again)")
        && !hypothesis.is_empty()
        && history.contains(&hypothesis);
    let proposed = reply(product, &pending, if hypothesis.is_empty() { REJECTED } else { &hypothesis });
    let proposed_run = field_or_empty(state(&proposed), "task_run");
    let decision = proposed_run
        .get("round_note")
        .and_then(as_str)
        .or_else(|| state(&proposed).get("notice").and_then(as_str))
        .unwrap_or("");
    let phase = proposed_run.get("phase").and_then(as_str).unwrap_or("None");
    let decision_resumed = !hypothesis.is_empty()
        && !component.is_empty()
        && !round_text.is_empty()
        && decision.contains(&refusal)
        && phase != "probe";
    let files_alone = !ledger_name.is_empty()
        && default_root.join(&ledger_name).is_file()
        && !default_root.join("edits.jsonl").is_file();
    keep(
        &mut passed,
        "files alone rebuild the resumed phase and decision",
        library_read && ledger_rebuilt && decision_resumed && files_alone,
    );

    let recursion = run_recursion(product, room, &default_root);
    keep(
        &mut passed,
        "three propose prompts anneal remaining edits from 4 to 3 to 1",
        recursion.wire1
            && recursion.wire2
            && recursion.wire3
            && recursion.rows == vec![
                RowFacts {
                    round: Some(1),
                    component: Some("skills".into()),
                    hypothesis: Some("flat one".into()),
                    verdict: Some("rejected".into()),
                    edit_budget: Some(4),
                    noise_floor: Some(0),
                    decision: Some("rejected revision".into()),
                    score_before: Some(0),
                    score_after: Some(0),
                },
                RowFacts {
                    round: Some(1),
                    component: Some("plugins".into()),
                    hypothesis: Some("keep the index".into()),
                    verdict: Some("accepted".into()),
                    edit_budget: Some(4),
                    noise_floor: Some(0),
                    decision: Some("cost not recorded".into()),
                    score_before: Some(0),
                    score_after: Some(1),
                },
                RowFacts {
                    round: Some(2),
                    component: Some("skills".into()),
                    hypothesis: Some("flat three".into()),
                    verdict: Some("rejected".into()),
                    edit_budget: Some(3),
                    noise_floor: Some(0),
                    decision: Some("score".into()),
                    score_before: Some(1),
                    score_after: Some(0),
                },
            ]
            && row_string(&recursion.raw_rows[2], "accept_reason") == Some("score")
            && row_bool(&recursion.raw_rows[2], "cost_known") == Some(true)
            && row_int(&recursion.raw_rows[2], "cost_before") == Some(68)
            && row_int(&recursion.raw_rows[2], "cost_after") == Some(56)
            && row_bool(&recursion.raw_rows[1], "cost_known") == Some(false)
            && row_int(&recursion.raw_rows[1], "cost_after") == Some(68)
            && recursion.resumed.as_deref() == Some(recursion.digest.as_str())
            && recursion.digest.len() == 64,
    );

    let drive_rows = &recursion.drive_rows;
    let budgets = round_budgets(drive_rows);
    let leaks: Vec<&Value> = drive_rows
        .iter()
        .filter(|row| row.get("leakage").and_then(as_str) == Some("task_text"))
        .collect();
    let floors: Vec<&Value> = drive_rows
        .iter()
        .filter(|row| row.get("component").and_then(as_str) == Some("plugins"))
        .collect();
    let skills: Vec<i64> = drive_rows
        .iter()
        .filter(|row| row.get("component").and_then(as_str) == Some("skills"))
        .filter_map(|row| row.get("round").and_then(number_as_i64))
        .collect();
    let floor_row = if floors.len() == 1 { floors[0] } else { &Value::Null };
    let commit = drive_rows.iter().find_map(|row| {
        if row.get("branch").and_then(as_str) == Some("rllm-evolve") {
            row.get("commit").and_then(as_str).map(str::to_string)
        } else {
            None
        }
    });
    keep(
        &mut passed,
        "three refinement rounds record budgets 4, 3 and 2, a leakage refusal, noise floor 2, pruned skills, the evolve commit, and held-out keys",
        recursion.propose1
            && recursion.propose2
            && recursion.propose3
            && budgets == BTreeMap::from([(1, BTreeSet::from([4])), (2, BTreeSet::from([3])), (3, BTreeSet::from([2]))])
            && leaks.len() == 1
            && row_int(leaks[0], "round") == Some(1)
            && row_string(leaks[0], "decision") == Some("refused because the candidate contains a recorded task")
            && leaks[0].get("score_before").is_none()
            && leaks[0].get("score_after").is_none()
            && row_string(leaks[0], "verdict") == Some("rejected")
            && row_string(floor_row, "decision") == Some("refused because the gain 1 does not exceed the noise floor 2")
            && row_int(floor_row, "noise_floor") == Some(2)
            && row_int(floor_row, "round") == Some(2)
            && row_int(floor_row, "score_before") == Some(1)
            && row_int(floor_row, "score_after") == Some(2)
            && skills == vec![1, 2, 3]
            && recursion.pruned == vec!["skills".to_string()]
            && recursion.evidence.contains("Pruned: skills.")
            && recursion.evidence.contains("The annealed budget is 1.")
            && commit.as_deref().is_some_and(commit_hex)
            && commit.as_deref().map(str::len) == Some(40)
            && commit.as_deref() == Some(recursion.tip.as_str())
            && commit.as_deref() == Some(recursion.head.as_str())
            && recursion.harness == "{\"a\":\"\"}"
            && recursion.main == recursion.seed
            && row_bool(floor_row, "held_out_known") == Some(true)
            && row_int(floor_row, "held_out_passed") == Some(1)
            && row_string(floor_row, "held_out_task_key").map(str::to_string)
                == Some(twin::sha256_bytes(json_string("Held out drive task.").as_bytes()))
            && row_string(floor_row, "held_out_verifier").map(str::to_string)
                == Some(twin::sha256_bytes(b"[\"HELD-TOKEN\"]"))
            && ["held_out_known", "held_out_passed", "held_out_task_key", "held_out_verifier"]
                .iter()
                .all(|key| floor_row.get(key).is_some())
            && recursion.phase == "review",
    );

    keep(
        &mut passed,
        "a re-proposed skills hypothesis is refused by name for round 1",
        recursion.refusal
            == "refused because component skills with that hypothesis was already rejected in round 1"
            && recursion.refusal_notice == recursion.refusal
            && recursion.refusal_phase == "propose",
    );

    let quiet_rows = &recursion.quiet_rows;
    let quiet_row = if quiet_rows.len() == 1 { &quiet_rows[0] } else { &Value::Null };
    keep(
        &mut passed,
        "a round that gains nothing records noise floor 0 and decision cost not recorded",
        quiet_rows.len() == 1
            && row_int(quiet_row, "noise_floor") == Some(0)
            && row_string(quiet_row, "decision") == Some("cost not recorded")
            && row_int(quiet_row, "score_before") == Some(0)
            && row_int(quiet_row, "score_after") == Some(0)
            && recursion.quiet_note
                == "The first round ended the refinement. A second round requires a comparable partial gain.",
    );

    keep(
        &mut passed,
        "two flat skills rounds leave the budget line unpruned",
        recursion.wire1
            && recursion.wire2
            && !recursion.wire3_pruned
            && py_falsy(recursion.pruned_run.as_ref())
            && recursion.rows == vec![
                RowFacts {
                    round: Some(1),
                    component: Some("skills".into()),
                    hypothesis: Some("flat one".into()),
                    verdict: Some("rejected".into()),
                    edit_budget: Some(4),
                    noise_floor: Some(0),
                    decision: Some("rejected revision".into()),
                    score_before: Some(0),
                    score_after: Some(0),
                },
                RowFacts {
                    round: Some(1),
                    component: Some("plugins".into()),
                    hypothesis: Some("keep the index".into()),
                    verdict: Some("accepted".into()),
                    edit_budget: Some(4),
                    noise_floor: Some(0),
                    decision: Some("cost not recorded".into()),
                    score_before: Some(0),
                    score_after: Some(1),
                },
                RowFacts {
                    round: Some(2),
                    component: Some("skills".into()),
                    hypothesis: Some("flat three".into()),
                    verdict: Some("rejected".into()),
                    edit_budget: Some(3),
                    noise_floor: Some(0),
                    decision: Some("score".into()),
                    score_before: Some(1),
                    score_after: Some(0),
                },
            ]
            && row_string(&recursion.raw_rows[2], "decision") == Some("score")
            && row_int(&recursion.raw_rows[2], "cost_before") == Some(68)
            && row_int(&recursion.raw_rows[2], "cost_after") == Some(56)
            && recursion
                .raw_rows
                .iter()
                .all(|row| !row_string(row, "decision").unwrap_or("").contains("was pruned")),
    );

    let pool = &recursion.pool;
    let pool_commit = pool.get("commit").and_then(as_str).unwrap_or("");
    keep(
        &mut passed,
        "the incumbent commit in .rllm-worktrees is the evolve tip",
        commit_hex(pool_commit)
            && pool_commit.len() == 40
            && pool_commit != pool.get("seed").and_then(as_str).unwrap_or("")
            && pool_commit == pool.get("tip").and_then(as_str).unwrap_or("")
            && pool_commit == pool.get("head").and_then(as_str).unwrap_or("")
            && pool.get("branch").and_then(as_str) == Some("rllm-evolve")
            && pool.get("status").and_then(as_str) == Some("admissible")
            && pool.get("harness").and_then(as_str) == Some("{\"flag\":\"\"}")
            && pool.get("parent").and_then(as_bool) == Some(true)
            && pool.get("main").and_then(as_str) == pool.get("seed").and_then(as_str)
            && row_int(pool, "before") == Some(0)
            && row_int(pool, "after") == Some(1)
            && pool.get("note").and_then(as_str) == Some("kept candidate 0"),
    );

    keep(
        &mut passed,
        "the separate evaluation score 0 differs from the in-round score 1 and is not a held-out key",
        matches!(pool.get("held"), Some(Value::Arr(items)) if items.is_empty())
            && row_int(pool, "eval_base") == Some(1)
            && row_int(pool, "eval_cand") == Some(0)
            && row_int(pool, "after") == Some(1)
            && row_int(pool, "eval_cand") != row_int(pool, "after"),
    );

    run_stall(product, room, &default_root, &mut passed);
    run_shape(product, room, &default_root, &mut passed);
    run_tamper(product, room, &default_root, &mut passed);
    keep(&mut passed, CROSS_CHECK, cross_read.ok());
    keep(&mut passed, EMPTY_CHECK, empty_read.ok());
    keep(&mut passed, LOG_CHECK, printed_log_carries_values());
    passed
}

fn run_stall(product: &mut Product, room: &Path, default_root: &Path, passed: &mut Vec<String>) {
    let task = "Stall band task.";
    let checks = "BAND-A\nBAND-B";
    let sentence = "refused because component skills was already tried while progress stayed inside the noise band for 3 rounds";
    drive_gap(product, room, task, checks, &[
        "baseline says nothing",
        &edit_text("skills", "stall one", "skills body one"),
        "nothing here",
        &edit_text("skills", "stall one", "skills body one"),
        "nothing here",
        "BAND-A is the second base",
        &edit_text("memory", "stall two", "memory body two"),
        "nothing here",
        &edit_text("memory", "stall two", "memory body two"),
        "nothing here",
        &edit_text("tools", "stall three", "tools body three"),
        "nothing here",
        &edit_text("tools", "stall three", "tools body three"),
        "nothing here",
    ]);
    let pending = drive_gap(product, room, task, checks, &[
        "baseline says nothing",
        &edit_text("subagents", "stall extra", "subagents body extra"),
        "nothing here",
        &edit_text("subagents", "stall extra", "subagents body extra"),
        "nothing here",
        "BAND-A opens the live floor",
        &edit_text("skills", "stall again", "skills body again"),
    ]);
    let run = field_or_empty(state(&pending), "task_run");
    let rows = ledger_of(state(&pending), default_root);
    let (best, floor, components) = scored_band(&rows);
    let tried = rows
        .iter()
        .find(|row| row.get("hypothesis").and_then(as_str) == Some("stall again"));
    let refused = run.get("round_note").and_then(as_str) == Some(sentence)
        && run.get("phase").and_then(as_str) == Some("propose")
        && tried.and_then(|row| row.get("decision")).and_then(as_str) == Some(sentence)
        && best.len() >= 3
        && floor >= 1
        && best.values().all(|gain| *gain <= floor)
        && !components.contains("config");
    let retried = event(product, Some(state(&pending)), "retry", obj(vec![]), room);
    let control = reply(product, &retried, &edit_text("config", "never tried", "config body fresh"));
    let control_run = field_or_empty(state(&control), "task_run");
    let control_rows = ledger_of(state(&control), default_root);
    let notice = state(&control).get("notice").and_then(as_str).unwrap_or("");
    let same = control_rows.len() == rows.len()
        && !control_rows.iter().any(|row| row.get("component").and_then(as_str) == Some("config"))
        && control_run.get("phase").and_then(as_str) == Some("probe")
        && !notice.contains(sentence);
    keep(passed, STALL_CHECK, refused && same);
}

fn run_shape(product: &mut Product, room: &Path, default_root: &Path, passed: &mut Vec<String>) {
    let task = "Novelty dup task.";
    let checks = "MARK-A\nMARK-B";
    let novel = "accepted because of structural novelty";
    let rose = "refused because the cost rose while the score did not";
    let lower = "accepted because the equal score costs less";
    let one = edit_text("memory", "flat one", "body alpha");
    let two = edit_text("memory", "flat two", "body beta");
    drive_gap(product, room, task, checks, &[
        "baseline says nothing",
        &edit_text("skills", "opener", "body opener"),
        "nothing here",
        &edit_text("skills", "opener", "body opener"),
        "nothing here",
        "MARK-A is the second base",
        &one,
        "nothing here",
        &one,
        "nothing here",
        &two,
        "nothing here",
        &two,
        "nothing here",
    ]);
    let prepared = settle_gap(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s(task)),
            ("response_checks".into(), s(checks)),
            ("refinement_rounds".into(), s("3")),
        ]),
        room,
    );
    pending = reply(product, &pending, "baseline says nothing");
    for component in [
        "prompt",
        "control flow",
        "config",
        "context management",
        "tools",
        "subagents",
        "skills",
    ] {
        pending = reply(
            product,
            &pending,
            &format!("component: {component}\nhypothesis: leak {component}\nLeak task {task}"),
        );
        pending = event(product, Some(state(&pending)), "retry", obj(vec![]), room);
    }
    for text in [
        &edit_text("prompt", "filler", "body filler"),
        "nothing here",
        &edit_text("prompt", "filler", "body filler"),
        "nothing here",
        "MARK-A opens the floor",
        &one,
        "nothing here",
        &one,
        "nothing here",
    ] {
        pending = reply(product, &pending, text);
    }
    let rows = ledger_of(state(&pending), default_root);
    let first = rows.iter().find(|row| {
        row.get("hypothesis").and_then(as_str) == Some("flat one")
            && row.get("decision").and_then(as_str) == Some(novel)
    });
    let again = rows.iter().find(|row| {
        row.get("hypothesis").and_then(as_str) == Some("flat one")
            && row.get("decision").and_then(as_str) == Some(rose)
    });
    let other = rows.iter().find(|row| {
        row.get("hypothesis").and_then(as_str) == Some("flat two")
            && row.get("decision").and_then(as_str) == Some(lower)
    });
    let triple = |row: &Value| {
        (
            row.get("component").cloned(),
            row.get("hypothesis").cloned(),
            row.get("diff_digest").cloned(),
        )
    };
    let ok = first.is_some()
        && again.is_some()
        && other.is_some()
        && triple(first.unwrap()) == triple(again.unwrap())
        && other.unwrap().get("diff_digest") != first.unwrap().get("diff_digest")
        && row_string(first.unwrap(), "accept_reason") == Some("structural novelty")
        && row_bool(first.unwrap(), "cost_known") == Some(false)
        && row_int(first.unwrap(), "cost_before") == Some(15)
        && row_int(first.unwrap(), "cost_after") == Some(49)
        && row_string(again.unwrap(), "accept_reason") == Some("cost rose")
        && row_bool(again.unwrap(), "cost_known") == Some(true)
        && row_int(again.unwrap(), "cost_before") == Some(48)
        && row_int(again.unwrap(), "cost_after") == Some(49)
        && row_string(other.unwrap(), "accept_reason") == Some("lower cost")
        && row_bool(other.unwrap(), "cost_known") == Some(true)
        && row_int(other.unwrap(), "cost_before") == Some(49)
        && row_int(other.unwrap(), "cost_after") == Some(48)
        && row_int(first.unwrap(), "beta0") == Some(0)
        && row_int(again.unwrap(), "beta0") == Some(0)
        && row_int(other.unwrap(), "beta0") == Some(0)
        && row_int(first.unwrap(), "beta1") == Some(64)
        && row_int(again.unwrap(), "beta1") == Some(64)
        && row_int(other.unwrap(), "beta1") == Some(64)
        && row_string(first.unwrap(), "verdict") == Some("accepted")
        && row_string(again.unwrap(), "verdict") == Some("rejected")
        && row_string(other.unwrap(), "verdict") == Some("accepted")
        && row_int(first.unwrap(), "score_before") == Some(0)
        && row_int(again.unwrap(), "score_before") == Some(0)
        && row_int(other.unwrap(), "score_before") == Some(0)
        && row_int(first.unwrap(), "score_after") == Some(0)
        && row_int(again.unwrap(), "score_after") == Some(0)
        && row_int(other.unwrap(), "score_after") == Some(0)
        && row_int(first.unwrap(), "noise_floor") == Some(1)
        && row_int(again.unwrap(), "noise_floor") == Some(1)
        && row_int(other.unwrap(), "noise_floor") == Some(1)
        && row_string(first.unwrap(), "decision") != row_string(again.unwrap(), "decision");
    keep(passed, SHAPE_CHECK, ok);
}

fn run_tamper(product: &mut Product, room: &Path, default_root: &Path, passed: &mut Vec<String>) {
    let library = default_root.join("experience.jsonl");
    let original = std::fs::read(&library).unwrap_or_else(|error| panic!("a tampered library entry is refused and its bytes stay unchanged: {error}"));
    let token = b"Candidate rules for the bounded report.";
    let at = original
        .windows(token.len())
        .position(|window| window == token)
        .unwrap_or_else(|| panic!("a tampered library entry is refused and its bytes stay unchanged: sealed harness text was not in experience.jsonl"));
    let mut tampered = original.clone();
    tampered[at] = if tampered[at] == b'X' { b'Y' } else { b'X' };
    std::fs::write(&library, &tampered).unwrap_or_else(|error| panic!("a tampered library entry is refused and its bytes stay unchanged: {error}"));
    let before = twin::sha256_bytes(&tampered);
    product.restart(room).unwrap_or_else(|error| panic!("a tampered library entry is refused and its bytes stay unchanged: {error}"));
    let prepared = fresh(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s(TASK)),
            ("response_checks".into(), s(CHECKS)),
        ]),
        room,
    );
    pending = reply(product, &pending, "Original reply.");
    let wire = effect_wire(&pending);
    let after = twin::sha256_bytes(&std::fs::read(&library).unwrap());
    let refused = wire.contains("library record digest does not match its body; existing history was left unchanged");
    let adopted = wire.contains("Adopted harness (historical)");
    keep(
        passed,
        "a tampered library entry is refused and its bytes stay unchanged",
        refused && !adopted && before == after && std::fs::read(&library).unwrap() == tampered,
    );
}

/// The library key the product derives from a task text is the digest of the
/// task bytes, so the twin computes the key a sealed record carries.
fn task_key(task: &str) -> String {
    twin::sha256_bytes(task.as_bytes())
}

/// The product renders a harness or a lesson clipped to 480 characters and
/// marks the cut with a trailing ellipsis.
fn clipped(value: &str) -> String {
    let mut end = value.len().min(480);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = value[..end].to_string();
    if end < value.len() {
        out.push_str("...");
    }
    out
}

/// A rendered string reaches the wire inside a JSON string, so it appears
/// either verbatim or in its JSON-escaped form.
fn wire_has(wire: &str, text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let quoted = json_string(text);
    wire.contains(text) || wire.contains(&quoted[1..quoted.len() - 1])
}

/// The task keys a historical section names, in the order it names them.
fn named_keys(section: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let mut rest = section;
    while let Some(at) = rest.find("key ") {
        rest = &rest[at + 4..];
        if rest.len() < 64 || !rest.is_char_boundary(64) {
            continue;
        }
        let candidate = &rest[..64];
        if !candidate.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            continue;
        }
        if !keys.iter().any(|key| key == candidate) {
            keys.push(candidate.to_string());
        }
        rest = &rest[64..];
    }
    keys
}

/// One refine task driven through the product JSON-lines route. The wire is
/// the request the product sent to the model and the phase says where the run
/// stood when it sent it.
struct ProposeRun {
    wire: String,
    phase: String,
}

fn propose_run(product: &mut Product, room: &Path, task: &str, checks: &str) -> ProposeRun {
    let prepared = fresh(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![("task".into(), s(task)), ("response_checks".into(), s(checks))]),
        room,
    );
    pending = reply(product, &pending, "Original reply.");
    let run = field_or_empty(state(&pending), "task_run");
    let phase = run.get("phase").and_then(as_str).unwrap_or("").to_string();
    let wire = effect_wire(&pending).to_string();
    let cancelled = event(product, Some(state(&pending)), "cancel", obj(vec![]), room);
    if !is_nullish(state(&cancelled).get("task_run")) {
        panic!("a later refine task left an active task behind");
    }
    ProposeRun { wire, phase }
}

/// What a later task's propose request carried out of the library.
struct CrossRead {
    rows: usize,
    heading: bool,
    relation_other: bool,
    names_earlier_key: bool,
    names_earlier_harness: bool,
    names_earlier_lesson: bool,
    names_own_key: bool,
    keys: Vec<String>,
    earlier_key: String,
    own_key: String,
    earlier_key_is_task_key: bool,
    earlier_row_is_first_task: bool,
    phase: String,
    proposed: bool,
}

impl CrossRead {
    fn ok(&self) -> bool {
        self.rows >= 1
            && self.heading
            && self.relation_other
            && self.earlier_key_is_task_key
            && self.earlier_row_is_first_task
            && self.names_earlier_key
            && self.names_earlier_harness
            && self.names_earlier_lesson
            && !self.names_own_key
            && !self.keys.iter().any(|key| key == &self.own_key)
            && self.keys.contains(&self.earlier_key)
            && self.phase == "propose"
            && self.proposed
    }
}

fn cross_task_read(product: &mut Product, room: &Path, default_root: &Path, settled: &Value) -> CrossRead {
    let rows = read_rows(&default_root.join("experience.jsonl"));
    let digest = settled.get("run_digest").and_then(as_str).unwrap_or("");
    // The row the settled first task sealed, named by that run's digest.
    let earlier = rows
        .iter()
        .find(|row| digest.len() == 64 && row.get("run_digest").and_then(as_str) == Some(digest))
        .cloned()
        .unwrap_or(Value::Null);
    let earlier_key = earlier.get("key").and_then(as_str).unwrap_or("").to_string();
    let earlier_harness = earlier.get("harness").and_then(as_str).unwrap_or("").to_string();
    let earlier_lesson = earlier.get("lesson").and_then(as_str).unwrap_or("").to_string();
    let own_key = task_key(CROSS_TASK);
    let run = propose_run(product, room, CROSS_TASK, CHECKS);
    let at = run.wire.find("HISTORICAL EXPERIENCE");
    let section = at.map(|at| &run.wire[at..]).unwrap_or("");
    let keys = named_keys(section);
    let names_own_key = wire_has(section, &own_key);
    for key in &keys {
        println!("section heading HISTORICAL EXPERIENCE names key {key}");
    }
    println!(
        "section heading HISTORICAL EXPERIENCE names the earlier task key {earlier_key} and the current task key {own_key} is {} from that section",
        yes_no(names_own_key)
    );
    print_library_row("earlier sealed library row", &earlier);
    CrossRead {
        rows: rows.len(),
        heading: at.is_some(),
        relation_other: section.contains("indexed key is from another task"),
        names_earlier_key: !earlier_key.is_empty() && wire_has(section, &earlier_key),
        names_earlier_harness: !earlier_harness.is_empty() && wire_has(section, &clipped(&earlier_harness)),
        names_earlier_lesson: !earlier_lesson.is_empty() && wire_has(section, &clipped(&earlier_lesson)),
        names_own_key,
        keys,
        earlier_key_is_task_key: !earlier_key.is_empty() && earlier_key == task_key(TASK),
        earlier_row_is_first_task: earlier.get("task").and_then(as_str) == Some(TASK),
        earlier_key,
        own_key,
        phase: run.phase,
        proposed: run.wire.contains("Propose improved agent instructions"),
    }
}

/// What a later task's propose request carried when the library held nothing.
struct EmptyRead {
    rows: usize,
    heading: bool,
    harness_line: bool,
    phase: String,
    proposed: bool,
}

impl EmptyRead {
    fn ok(&self) -> bool {
        self.rows == 0 && !self.heading && !self.harness_line && self.phase == "propose" && self.proposed
    }
}

fn empty_library_read(product: &mut Product, room: &Path, default_root: &Path) -> EmptyRead {
    let rows = read_rows(&default_root.join("experience.jsonl"));
    let run = propose_run(product, room, CROSS_TASK, CHECKS);
    let heading = run.wire.contains("HISTORICAL EXPERIENCE");
    println!(
        "section heading HISTORICAL EXPERIENCE is {} in a propose wire over a library with {} sealed rows",
        yes_no(heading),
        rows.len()
    );
    if rows.is_empty() {
        print_library_row("library before a sealed row", &Value::Null);
    } else {
        for row in &rows {
            print_library_row("library before a sealed row", row);
        }
    }
    EmptyRead {
        rows: rows.len(),
        heading,
        harness_line: run.wire.contains("Adopted harness (historical)"),
        phase: run.phase,
        proposed: run.wire.contains("Propose improved agent instructions"),
    }
}

/// A measured fact reads as a word, so a printed line never states the
/// opposite of what the run showed.
fn yes_no(present: bool) -> &'static str {
    if present {
        "present"
    } else {
        "absent"
    }
}

/// One greppable line. The text is a value just read from a wire, a ledger
/// row, or a git file. A newline in that value stays on this one line.
fn evidence(body: &str) {
    let flat: String = body
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect();
    println!("evidence {flat}");
    PRINTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(flat);
}

/// The evidence lines this run printed. The log is what a reader of a run-log
/// document sees, so the run certifies its own log instead of trusting that
/// the prints happened.
static PRINTED: std::sync::LazyLock<std::sync::Mutex<Vec<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

/// A lowercase hex value of exactly `width` characters.
fn is_hex(value: &str, width: usize) -> bool {
    value.len() == width
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// The value printed after `key` on this line, up to the next space.
fn value_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(key)? + key.len();
    line.get(at..)?.split_whitespace().next()
}

/// A printed commit line carries the same 40-hex commit three times: the ledger
/// row's commit, the `rllm-evolve` ref, and the linked worktree's HEAD.
fn printed_commit_triple(line: &str) -> bool {
    let Some(at) = line.find(" ledger commit ") else {
        return false;
    };
    let tokens: Vec<&str> = line[at + " ledger commit ".len()..].split_whitespace().collect();
    tokens.len() >= 6
        && is_hex(tokens[0], 40)
        && tokens[1] == "refs/heads/rllm-evolve"
        && tokens[2] == tokens[0]
        && tokens[3] == "worktree"
        && tokens[4] == "HEAD"
        && tokens[5] == tokens[0]
}

fn printed_log_carries_values() -> bool {
    let lines = PRINTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // A label and its value share one printed line, so a budget value cannot be
    // satisfied by a sentence the run printed for a different request.
    let budget_rows = [
        (
            "anneal propose round 1 ",
            "Edits remaining this round: 4. The annealed budget is 4.",
        ),
        (
            "anneal propose round 2 ",
            "Edits remaining this round: 3. The annealed budget is 3.",
        ),
        ("anneal resumed propose ", "Edits remaining this round: 1. The annealed budget is 3."),
        (
            "drive propose round 1 ",
            "Edits remaining this round: 4. The annealed budget is 4.",
        ),
        (
            "drive propose round 2 ",
            "Edits remaining this round: 3. The annealed budget is 3.",
        ),
        (
            "drive propose round 3 ",
            "Edits remaining this round: 2. The annealed budget is 2.",
        ),
    ];
    let has_budgets = budget_rows.iter().all(|(label, sentence)| {
        lines
            .iter()
            .any(|line| line.contains(label) && line.contains(sentence))
    });
    let no_missing_budget = !lines
        .iter()
        .any(|line| line.contains("propose budget sentence is absent"));
    let has_commit_triple = lines.iter().any(|line| printed_commit_triple(line));
    let has_noise_floors = ["anneal ledger ", "quiet ledger ", "drive ledger "]
        .iter()
        .all(|label| lines.iter().any(|line| line.contains(label) && line.ends_with("noise_floor 0")))
        && lines
            .iter()
            .any(|line| line.contains("drive ledger ") && line.contains("round 3") && line.ends_with("noise_floor 2"));
    let has_pruned = lines
        .iter()
        .any(|line| line.contains("drive settled pruned_components [\"skills\"]"));
    // The flat-skills arm claims no pruning, so its line has to say so rather
    // than leave the word "absent" in the log unattributed.
    let asserts_flat_pruned_absent = lines
        .iter()
        .any(|line| line.contains("flat skills settled pruned_components is absent"));
    let asserts_empty_library = lines.iter().any(|line| {
        line.contains("library before a sealed row key absent revision absent harness_digest absent run_digest absent")
    });
    let has_held_out = lines.iter().any(|line| {
        line.contains("drive held-out ")
            && line.contains("held_out_known true")
            && value_after(line, "held_out_task_key ").is_some_and(|key| is_hex(key, 64))
            && value_after(line, "held_out_verifier ").is_some_and(|key| is_hex(key, 64))
    });
    let has_library_row = lines.iter().any(|line| {
        let Some(at) = line.find(" sealed library row ") else {
            return false;
        };
        let rest = &line[at + " sealed library row ".len()..];
        value_after(rest, "key ").is_some_and(|key| is_hex(key, 64))
            && value_after(rest, "revision ").is_some_and(|value| is_hex(value, 64))
            && value_after(rest, "harness_digest ").is_some_and(|value| is_hex(value, 64))
            && value_after(rest, "run_digest ").is_some_and(|value| is_hex(value, 64))
    });
    // The separate evaluation and the in-round score are printed as one line so
    // the log shows they are different measurements of the same round.
    let has_separate_evaluation = lines.iter().any(|line| {
        line.contains("separate evaluation base passed 1 separate evaluation candidate passed 0")
            && line.contains("in-round score_after 1")
    });
    has_budgets
        && no_missing_budget
        && has_commit_triple
        && has_noise_floors
        && has_pruned
        && asserts_flat_pruned_absent
        && asserts_empty_library
        && has_held_out
        && has_library_row
        && has_separate_evaluation
}

/// The budget sentence the product wrote on this request, through the period
/// that closes the annealed budget. Anything short of that sentence is absent.
fn budget_sentence(wire: &str) -> Option<String> {
    const START: &str = "Edits remaining this round:";
    const ANNEALED: &str = "The annealed budget is ";
    let rest = &wire[wire.find(START)?..];
    let number_at = rest.find(ANNEALED)? + ANNEALED.len();
    let tail = rest.get(number_at..)?;
    let split = tail.find(|ch: char| ch == '.' || ch == '\n' || ch == '\r').unwrap_or(tail.len());
    if split == 0 {
        return None;
    }
    let stop = number_at + split + usize::from(tail.as_bytes().get(split) == Some(&b'.'));
    Some(rest[..stop].to_string())
}

fn print_budget(label: &str, wire: &str) {
    match budget_sentence(wire) {
        Some(sentence) => evidence(&format!("{label} {sentence}")),
        None => evidence(&format!("{label} propose budget sentence is {}", yes_no(false))),
    }
}

fn stored_word(row: &Value, key: &str) -> String {
    match row.get(key) {
        None | Some(Value::Null) => "absent".to_string(),
        Some(Value::Str(text)) => text.clone(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Num(token)) => token.clone(),
        Some(other) => canonical(other),
    }
}

fn file_text(path: &Path) -> String {
    if !path.is_file() {
        return "absent".to_string();
    }
    let text = read_text(path).trim().to_string();
    if text.is_empty() {
        "absent".to_string()
    } else {
        text
    }
}

fn print_noise(label: &str, rows: &[Value]) {
    let mut carried = 0usize;
    for (index, row) in rows.iter().enumerate() {
        if matches!(row.get("noise_floor"), None | Some(Value::Null)) {
            continue;
        }
        carried += 1;
        evidence(&format!(
            "{label} row {index} round {} component {} noise_floor {}",
            stored_word(row, "round"),
            stored_word(row, "component"),
            stored_word(row, "noise_floor"),
        ));
    }
    if carried == 0 {
        evidence(&format!("{label} noise_floor is {}", yes_no(false)));
    }
}

fn print_pruned(label: &str, value: Option<&Value>) {
    match value {
        None | Some(Value::Null) => evidence(&format!("{label} pruned_components is {}", yes_no(false))),
        Some(value) => evidence(&format!("{label} pruned_components {}", canonical(value))),
    }
}

fn print_incumbent(label: &str, ledger_commit: &str, room: &Path, seed: &str) {
    let evolve = file_text(&room.join(".git/refs/heads/rllm-evolve"));
    let head = file_text(&room.join(".git/worktrees/c0/HEAD"));
    let main = file_text(&room.join(".git/refs/heads/main"));
    let planted = {
        let text = seed.trim();
        if text.is_empty() {
            "absent".to_string()
        } else {
            text.to_string()
        }
    };
    let equals = main != "absent" && planted != "absent" && main == planted;
    evidence(&format!(
        "{label} ledger commit {ledger_commit} refs/heads/rllm-evolve {evolve} worktree HEAD {head} refs/heads/main {main} planted seed {planted} main equals planted seed is {}",
        yes_no(equals)
    ));
}

fn evolve_commit(rows: &[Value]) -> String {
    rows.iter()
        .find(|row| row_string(row, "branch") == Some("rllm-evolve"))
        .map(|row| stored_word(row, "commit"))
        .unwrap_or_else(|| "absent".to_string())
}

fn passed_word(score: &Value) -> String {
    match score {
        Value::Null => "absent".to_string(),
        other => stored_word(other, "passed"),
    }
}

fn print_eval(label: &str, base: &Value, candidate: &Value, in_round: &Value) {
    evidence(&format!(
        "{label} separate evaluation base passed {} separate evaluation candidate passed {} in-round score_after {}",
        passed_word(base),
        passed_word(candidate),
        stored_word(in_round, "score_after"),
    ));
}

fn print_held_rows(label: &str, rows: &[Value]) {
    const KEYS: [&str; 4] = ["held_out_known", "held_out_passed", "held_out_task_key", "held_out_verifier"];
    let mut found = false;
    for (index, row) in rows.iter().enumerate() {
        let carries = KEYS.iter().any(|key| matches!(row.get(*key), Some(value) if !matches!(value, Value::Null)));
        if !carries {
            continue;
        }
        found = true;
        let body = KEYS
            .iter()
            .map(|key| format!("{key} {}", stored_word(row, key)))
            .collect::<Vec<_>>()
            .join(" ");
        evidence(&format!(
            "{label} row {index} round {} component {} {body}",
            stored_word(row, "round"),
            stored_word(row, "component"),
        ));
    }
    if !found {
        evidence(&format!(
            "{label} held_out_known held_out_passed held_out_task_key held_out_verifier are {}",
            yes_no(false)
        ));
    }
}

fn print_library_row(label: &str, row: &Value) {
    evidence(&format!(
        "{label} key {} revision {} harness_digest {} run_digest {}",
        stored_word(row, "key"),
        stored_word(row, "revision"),
        stored_word(row, "harness_digest"),
        stored_word(row, "run_digest"),
    ));
}

fn run_recursion(product: &mut Product, room: &Path, default_root: &Path) -> Recursion {
    let phrases = "PHRASE-ONE\nPHRASE-TWO";
    let seed = plant_git(room).unwrap();
    let mut observed = Recursion::default();

    let prepared = settle_gap(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s("Flat skills task.")),
            ("response_checks".into(), s(phrases)),
            ("refinement_rounds".into(), s("2")),
        ]),
        room,
    );
    pending = reply(product, &pending, "baseline says nothing");
    let wire1 = effect_wire(&pending).to_string();
    print_budget("anneal propose round 1", &wire1);
    pending = reply(product, &pending, "component: plugins\nhypothesis: keep the index\nindex body stays local");
    pending = reply(product, &pending, "PHRASE-ONE only");
    pending = reply(product, &pending, "component: skills\nhypothesis: flat one\nflat body one");
    pending = reply(product, &pending, "replay says nothing");
    let wire2 = effect_wire(&pending).to_string();
    print_budget("anneal propose round 2", &wire2);
    pending = reply(product, &pending, "component: skills\nhypothesis: flat two\nflat body two");
    pending = reply(product, &pending, "still nothing");
    pending = reply(product, &pending, "component: skills\nhypothesis: flat three\nflat body three");
    pending = reply(product, &pending, "replay still nothing");
    let settled = field_or_empty(state(&pending), "refinement_pending");
    let digest = settled.get("run_digest").and_then(as_str).unwrap_or("").to_string();
    let rows = read_rows(&default_root.join(format!("{digest}.jsonl")));
    observed.digest = digest.clone();
    observed.rows = rows.iter().map(RowFacts::from).collect();
    print_noise("anneal ledger", &rows);
    observed.raw_rows = rows;
    observed.wire1 = wire1.contains("Edits remaining this round: 4. The annealed budget is 4.") && !wire1.contains("Pruned:");
    observed.wire2 = wire2.contains("Edits remaining this round: 3. The annealed budget is 3.") && !wire2.contains("Pruned:");
    observed.pruned_run = settled.get("pruned_components").cloned();
    print_pruned("flat skills settled", settled.get("pruned_components"));

    let prepared = settle_gap(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s("Flat skills task.")),
            ("response_checks".into(), s(phrases)),
            ("refinement_rounds".into(), s("2")),
        ]),
        room,
    );
    pending = reply(product, &pending, "baseline again");
    let wire3 = effect_wire(&pending);
    print_budget("anneal resumed propose", wire3);
    observed.wire3 = wire3.contains("Edits remaining this round: 1. The annealed budget is 3.");
    observed.wire3_pruned = wire3.contains("Pruned:");
    observed.resumed = state(&pending).get("task_run").and_then(|run| run.get("run_digest")).and_then(as_str).map(str::to_string);
    pending = reply(product, &pending, "component: skills\nhypothesis: flat one\nflat body proposed again");
    let run = field_or_empty(state(&pending), "task_run");
    observed.refusal = run.get("round_note").and_then(as_str).unwrap_or("").to_string();
    observed.refusal_notice = state(&pending).get("notice").and_then(as_str).unwrap_or("").to_string();
    observed.refusal_phase = run.get("phase").and_then(as_str).unwrap_or("").to_string();

    let prepared = settle_gap(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s("No gain task.")),
            ("response_checks".into(), s(phrases)),
            ("refinement_rounds".into(), s("2")),
        ]),
        room,
    );
    pending = reply(product, &pending, "baseline says nothing");
    pending = reply(product, &pending, "component: memory\nhypothesis: leave the floor\nmemory body stays local");
    pending = reply(product, &pending, "still nothing");
    pending = reply(product, &pending, "component: memory\nhypothesis: leave the floor again\nmemory body rewritten");
    pending = reply(product, &pending, "replay still nothing");
    let quiet = field_or_empty(state(&pending), "refinement_pending");
    let quiet_digest = quiet.get("run_digest").and_then(as_str).unwrap_or("");
    observed.quiet_note = quiet.get("round_note").and_then(as_str).unwrap_or("").to_string();
    observed.quiet_rows = read_rows(&default_root.join(format!("{quiet_digest}.jsonl")));
    print_noise("quiet ledger", &observed.quiet_rows);

    let prepared = settle_gap(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s("Pool commit task.")),
            ("response_check_method".into(), s("json_fields")),
            ("response_checks".into(), s("{\"/flag\":\"\"}")),
            ("refinement_rounds".into(), s("1")),
            ("evaluation_task".into(), s("Held out pool task.")),
            ("evaluation_method".into(), s("phrases")),
            ("evaluation_checks".into(), s("HELD-TOKEN")),
        ]),
        room,
    );
    pending = reply(product, &pending, "baseline is not json");
    pending = reply(product, &pending, "candidates: 1\n{\"flag\":\"\"}");
    let pool_run = field_or_empty(state(&pending), "task_run");
    let pool_digest = pool_run.get("run_digest").and_then(as_str).unwrap_or("");
    let pool_rows = read_rows(&default_root.join(format!("{pool_digest}.jsonl")));
    let pool_row = pool_rows.last().cloned().unwrap_or(Value::Null);
    pending = reply(product, &pending, "HELD-TOKEN present");
    let done = reply(product, &pending, "no token here");
    let reviewed = field_or_empty(state(&done), "refinement_pending");
    let base = reviewed.get("evaluation_base_score").cloned().unwrap_or(Value::Null);
    let cand = reviewed.get("evaluation_candidate_score").cloned().unwrap_or(Value::Null);
    let head_file = room.join(".git/worktrees/c0/HEAD");
    let harness_file = room.join(".rllm-worktrees/c0/rllm-harness.txt");
    let pool = obj(vec![
        ("note".into(), pool_run.get("round_note").cloned().unwrap_or(Value::Null)),
        ("commit".into(), pool_row.get("commit").cloned().unwrap_or(Value::Null)),
        ("branch".into(), pool_row.get("branch").cloned().unwrap_or(Value::Null)),
        ("status".into(), pool_row.get("pool_status").cloned().unwrap_or(Value::Null)),
        ("before".into(), pool_row.get("score_before").cloned().unwrap_or(Value::Null)),
        ("after".into(), pool_row.get("score_after").cloned().unwrap_or(Value::Null)),
        (
            "held".into(),
            Value::Arr(
                ["held_out_known", "held_out_passed", "held_out_task_key", "held_out_verifier"]
                    .iter()
                    .filter_map(|key| pool_row.get(key).cloned())
                    .collect(),
            ),
        ),
        ("tip".into(), s(&incumbent_tip(room))),
        ("head".into(), s(read_text_if_file(&head_file).trim())),
        ("harness".into(), s(&read_text_if_file(&harness_file))),
        (
            "parent".into(),
            Value::Bool(
                commit_text(room, pool_row.get("commit").and_then(as_str).unwrap_or(""))
                    .contains(&format!("parent {seed}")),
            ),
        ),
        ("seed".into(), s(&seed)),
        ("main".into(), s(read_text_if_file(&room.join(".git/refs/heads/main")).trim())),
        ("eval_base".into(), base.get("passed").cloned().unwrap_or(Value::Null)),
        ("eval_cand".into(), cand.get("passed").cloned().unwrap_or(Value::Null)),
    ]);
    observed.pool = pool;
    print_incumbent("pool", &stored_word(&pool_row, "commit"), room, &seed);
    print_eval("pool", &base, &cand, &pool_row);

    let prepared = settle_gap(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s("Drive three rounds.")),
            ("response_check_method".into(), s("json_fields")),
            ("response_checks".into(), s("{\"/a\":\"\",\"/b\":\"\"}")),
            ("refinement_rounds".into(), s("3")),
            ("evaluation_task".into(), s("Held out drive task.")),
            ("evaluation_method".into(), s("phrases")),
            ("evaluation_checks".into(), s("HELD-TOKEN")),
        ]),
        room,
    );
    pending = reply(product, &pending, "baseline says nothing");
    pending = reply(product, &pending, "HELD-TOKEN is the separate answer");
    let drive_wire_1 = effect_wire(&pending).to_string();
    print_budget("drive propose round 1", &drive_wire_1);
    observed.propose1 = drive_wire_1.contains("Edits remaining this round: 4. The annealed budget is 4.")
        && !drive_wire_1.contains("Pruned:");
    pending = reply(
        product,
        &pending,
        "candidates: 3\n{\"a\":\"\"}\n---\nCopy the task Drive three rounds. into the harness\n---\ncomponent: skills\nhypothesis: flat one\nflat body one",
    );
    pending = reply(product, &pending, "{\"a\":\"\",\"b\":\"\"}");
    let drive_wire_2 = effect_wire(&pending).to_string();
    print_budget("drive propose round 2", &drive_wire_2);
    observed.propose2 = drive_wire_2.contains("Edits remaining this round: 3. The annealed budget is 3.")
        && !drive_wire_2.contains("Pruned:");
    pending = reply(product, &pending, "component: plugins\nhypothesis: raise the flag\nplugin body stays local");
    pending = reply(product, &pending, "{\"a\":\"\",\"b\":\"\"}");
    pending = reply(product, &pending, "component: skills\nhypothesis: flat two\nflat body two");
    pending = reply(product, &pending, "replay says nothing");
    let drive_wire_3 = effect_wire(&pending).to_string();
    print_budget("drive propose round 3", &drive_wire_3);
    observed.propose3 = drive_wire_3.contains("Edits remaining this round: 2. The annealed budget is 2.")
        && !drive_wire_3.contains("Pruned:");
    pending = reply(product, &pending, "component: skills\nhypothesis: flat three\nround three body");
    pending = reply(product, &pending, "still nothing");
    pending = reply(product, &pending, "component: skills\nhypothesis: flat four\nround three composed");
    pending = reply(product, &pending, "still nothing");
    pending = reply(product, &pending, "HELD-TOKEN is here");
    let done = reply(product, &pending, "no token");
    let reviewed = field_or_empty(state(&done), "refinement_pending");
    let digest = reviewed.get("run_digest").and_then(as_str).unwrap_or("");
    observed.drive_rows = read_rows(&default_root.join(format!("{digest}.jsonl")));
    observed.evidence = reviewed.get("evidence").and_then(as_str).unwrap_or("").to_string();
    observed.pruned = match reviewed.get("pruned_components") {
        Some(Value::Arr(items)) => items.iter().filter_map(as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    observed.phase = reviewed.get("phase").and_then(as_str).unwrap_or("").to_string();
    observed.tip = incumbent_tip(room);
    observed.head = read_text_if_file(&room.join(".git/worktrees/c0/HEAD")).trim().to_string();
    observed.harness = read_text_if_file(&room.join(".rllm-worktrees/c0/rllm-harness.txt"));
    observed.main = read_text_if_file(&room.join(".git/refs/heads/main")).trim().to_string();
    observed.seed = seed;
    print_noise("drive ledger", &observed.drive_rows);
    print_pruned("drive settled", reviewed.get("pruned_components"));
    print_incumbent("drive", &evolve_commit(&observed.drive_rows), room, &observed.seed);
    print_held_rows("drive held-out", &observed.drive_rows);
    settle_gap(product, room);
    observed
}

fn drive_gap(product: &mut Product, room: &Path, task: &str, checks: &str, steps: &[&str]) -> Value {
    let prepared = settle_gap(product, room);
    let mut pending = event(
        product,
        Some(&prepared),
        "refine",
        obj(vec![
            ("task".into(), s(task)),
            ("response_checks".into(), s(checks)),
            ("refinement_rounds".into(), s("3")),
        ]),
        room,
    );
    for step in steps {
        pending = reply(product, &pending, step);
    }
    pending
}

fn fresh(product: &mut Product, room: &Path) -> Value {
    let resumed = event(product, None, "resume", obj(vec![]), room);
    let mut value = state(&resumed).clone();
    put(&mut value, "profiles", Value::Arr(vec![profile()]));
    put(&mut value, "agents", Value::Arr(vec![agent()]));
    value
}

fn settle_gap(product: &mut Product, room: &Path) -> Value {
    let resumed = event(product, None, "resume", obj(vec![]), room);
    let mut value = state(&resumed).clone();
    if present(&value, "task_run") {
        let cancelled = event(product, Some(&value), "cancel", obj(vec![]), room);
        value = state(&cancelled).clone();
    }
    if present(&value, "refinement_pending") {
        let review = event(product, Some(&value), "refinement_discard_review", obj(vec![]), room);
        value = state(&review).clone();
        let discarded = event(product, Some(&value), "refinement_discard", obj(vec![]), room);
        value = state(&discarded).clone();
    }
    put(&mut value, "profiles", Value::Arr(vec![profile()]));
    put(&mut value, "agents", Value::Arr(vec![agent()]));
    value
}

fn profile() -> Value {
    obj(vec![
        ("name".into(), s("Controlled")),
        ("endpoint".into(), s("http://model.test/v1/chat/completions")),
        ("model".into(), s("controlled")),
        ("protocol".into(), s("chat")),
        ("stream".into(), Value::Bool(false)),
        ("plain".into(), Value::Bool(true)),
        ("tool_format".into(), s("structured")),
    ])
}

fn agent() -> Value {
    obj(vec![
        ("name".into(), s("General")),
        ("tools".into(), s("read")),
        ("instructions".into(), s("INCUMBENT RULES")),
    ])
}

fn event(product: &mut Product, state: Option<&Value>, action: &str, inputs: Value, room: &Path) -> Value {
    product.event(state, action, inputs, room)
}

fn reply(product: &mut Product, pending: &Value, text: &str) -> Value {
    let body = format!(
        "{{\"choices\":[{{\"message\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}",
        json_string(text)
    );
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let pending_state = state(pending);
    let model_ticket = pending_state.get("model_expected").cloned().unwrap_or(Value::Null);
    let room = product.room.clone();
    event(
        product,
        Some(pending_state),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), model_ticket),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
        &room,
    )
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn field_or_empty(value: &Value, key: &str) -> Value {
    value.get(key).cloned().unwrap_or(Value::Obj(Vec::new()))
}

fn effect_wire(value: &Value) -> &str {
    value
        .get("effect")
        .and_then(|effect| effect.get("wire"))
        .and_then(as_str)
        .unwrap_or("")
}

fn active_chat(value: &Value) -> &Value {
    let active = value.get("active").and_then(as_str).unwrap_or("");
    let chats = match value.get("chats") {
        Some(Value::Arr(items)) => items,
        _ => panic!("active chat not found"),
    };
    chats
        .iter()
        .find(|chat| chat.get("id").and_then(as_str) == Some(active))
        .unwrap_or_else(|| panic!("active chat not found"))
}

fn active_messages(value: &Value) -> &Vec<Value> {
    match active_chat(value).get("messages") {
        Some(Value::Arr(items)) => items,
        _ => panic!("active chat has no messages"),
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

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    println!("leg {name}");
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text.as_str()),
        _ => None,
    }
}

fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        _ => None,
    }
}

fn present(value: &Value, key: &str) -> bool {
    !is_nullish(value.get(key))
}

fn is_nullish(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

fn py_falsy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::Num(token)) => token == "0",
        Some(Value::Str(text)) => text.is_empty(),
        Some(Value::Arr(items)) => items.is_empty(),
        Some(_) => false,
    }
}

fn source_digest_of(workspace: &Path) -> String {
    let path = workspace.join("rllm-harness.txt");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => "absent".to_string(),
        Ok(metadata) if metadata.is_file() => twin::sha256_bytes(&std::fs::read(&path).unwrap()),
        _ => "absent".to_string(),
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Num(token) => token.clone(),
        Value::Str(value) => json_string(value),
        Value::Arr(items) => {
            let body = items.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let mut sorted = fields.clone();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            let body = sorted
                .iter()
                .map(|(key, value)| format!("{}:{}", json_string(key), canonical(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

fn row_seal(row: &Value) -> bool {
    let Value::Obj(fields) = row else {
        return false;
    };
    let body = Value::Obj(fields.iter().filter(|(key, _)| key != "revision").cloned().collect());
    let digest = twin::sha256_bytes(canonical(&body).as_bytes());
    let revision = row.get("revision");
    let run_digest = row.get("run_digest");
    matches!(revision, Some(Value::Str(text)) if text == &digest)
        && matches!(run_digest, Some(Value::Str(text)) if text.len() == 64)
}

fn read_rows(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(Value::parse)
        .collect()
}

fn files_here(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn read_text_if_file(path: &Path) -> String {
    if path.is_file() {
        read_text(path)
    } else {
        String::new()
    }
}


fn git_store(git: &Path, kind: &str, payload: &[u8]) -> String {
    let mut raw = format!("{kind} {}\0", payload.len()).into_bytes();
    raw.extend_from_slice(payload);
    let digest = twin::sha1_digest(&raw);
    let hex = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let path = git.join("objects").join(&hex[..2]).join(&hex[2..]);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, twin::zlib_compress(&raw)).unwrap();
    hex
}

fn plant_git(workspace: &Path) -> Option<String> {
    let git = workspace.join(".git");
    let blob = git_store(&git, "blob", b"seed\n");
    let tree_payload = [b"100644 seed.txt\0".as_slice(), &hex_bytes(&blob)[..]].concat();
    let tree = git_store(&git, "tree", &tree_payload);
    let body = format!(
        "tree {tree}\nauthor RLLM <rllm@localhost> 1000000000 +0000\ncommitter RLLM <rllm@localhost> 1000000000 +0000\n\nseed\n"
    );
    let commit = git_store(&git, "commit", body.as_bytes());
    std::fs::create_dir_all(git.join("refs/heads")).unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(git.join("refs/heads/main"), format!("{commit}\n")).unwrap();
    std::fs::write(
        git.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n",
    )
    .unwrap();
    std::fs::write(workspace.join("seed.txt"), "seed\n").unwrap();
    Some(commit)
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|index| u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap())
        .collect()
}

fn commit_text(workspace: &Path, sha: &str) -> String {
    if sha.len() != 40 {
        return String::new();
    }
    let path = workspace.join(".git/objects").join(&sha[..2]).join(&sha[2..]);
    let Ok(compressed) = std::fs::read(path) else {
        return String::new();
    };
    let raw = twin::zlib_decompress(&compressed);
    let Some(separator) = raw.iter().position(|byte| *byte == 0) else {
        return String::new();
    };
    if !raw[..separator].starts_with(b"commit ") {
        return String::new();
    }
    String::from_utf8_lossy(&raw[separator + 1..]).to_string()
}

fn commit_hex(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn incumbent_tip(workspace: &Path) -> String {
    let evolve = workspace.join(".git/refs/heads/rllm-evolve");
    if evolve.is_file() {
        let tip = read_text(&evolve).trim().to_string();
        if commit_hex(&tip) {
            return tip;
        }
    }
    read_text(&workspace.join(".git/refs/heads/main")).trim().to_string()
}

fn outcome_text(pending: &Value) -> String {
    let value = state(pending);
    let run = value.get("task_run").or_else(|| value.get("refinement_pending"));
    let phase = match run.and_then(|item| item.get("phase")) {
        None | Some(Value::Null) => "None".to_string(),
        Some(Value::Str(text)) => text.clone(),
        Some(other) => other.to_string(),
    };
    let notice = match value.get("notice") {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        _ => String::new(),
    };
    let decision = match run.and_then(|item| item.get("round_note")) {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        _ => String::new(),
    };
    format!("phase={phase} notice={notice} decision={decision}")
}

fn edit_text(name: &str, hypothesis: &str, body: &str) -> String {
    format!("component: {name}\nhypothesis: {hypothesis}\n{body}")
}

fn ledger_of(value: &Value, default_root: &Path) -> Vec<Value> {
    let run = value.get("refinement_pending").or_else(|| value.get("task_run"));
    let digest = run.and_then(|item| item.get("run_digest")).and_then(as_str).unwrap_or("");
    if digest.len() == 64 {
        read_rows(&default_root.join(format!("{digest}.jsonl")))
    } else {
        Vec::new()
    }
}

fn scored_band(rows: &[Value]) -> (BTreeMap<i64, i64>, i64, BTreeSet<String>) {
    let mut best = BTreeMap::new();
    let mut floor = 0i64;
    let mut components = BTreeSet::new();
    for row in rows {
        if let Some(component) = row.get("component").and_then(as_str) {
            components.insert(component.to_string());
        }
        if let (Some(before), Some(after)) = (row.get("score_before").and_then(number_as_i64), row.get("score_after").and_then(number_as_i64)) {
            let gain = after - before;
            if let Some(round) = row.get("round").and_then(number_as_i64) {
                best.entry(round).and_modify(|value: &mut i64| *value = (*value).max(gain)).or_insert(gain);
            }
            if let Some(value) = row.get("noise_floor").and_then(number_as_i64) {
                floor = floor.max(value);
            }
        }
    }
    (best, floor, components)
}

fn round_budgets(rows: &[Value]) -> BTreeMap<i64, BTreeSet<i64>> {
    let mut budgets = BTreeMap::new();
    for row in rows {
        let (Some(round), Some(budget)) = (
            row.get("round").and_then(number_as_i64),
            row.get("edit_budget").and_then(number_as_i64),
        ) else {
            continue;
        };
        budgets.entry(round).or_insert_with(BTreeSet::new).insert(budget);
    }
    budgets
}

fn row_string<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(as_str)
}

fn row_bool(row: &Value, key: &str) -> Option<bool> {
    row.get(key).and_then(as_bool)
}

fn row_int(row: &Value, key: &str) -> Option<i64> {
    row.get(key).and_then(number_as_i64)
}

fn number_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Num(token) => token.parse().ok(),
        Value::Str(token) => token.parse().ok(),
        _ => None,
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
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
    out.push('"');
    out
}

struct Held {
    after_entry: Value,
    refusal: Value,
    settled: Value,
}

impl Default for Held {
    fn default() -> Self {
        Self {
            after_entry: Value::Null,
            refusal: Value::Null,
            settled: Value::Null,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RowFacts {
    round: Option<i64>,
    component: Option<String>,
    hypothesis: Option<String>,
    verdict: Option<String>,
    edit_budget: Option<i64>,
    noise_floor: Option<i64>,
    decision: Option<String>,
    score_before: Option<i64>,
    score_after: Option<i64>,
}

impl From<&Value> for RowFacts {
    fn from(row: &Value) -> Self {
        Self {
            round: row.get("round").and_then(number_as_i64),
            component: row_string(row, "component").map(str::to_string),
            hypothesis: row_string(row, "hypothesis").map(str::to_string),
            verdict: row_string(row, "verdict").map(str::to_string),
            edit_budget: row.get("edit_budget").and_then(number_as_i64),
            noise_floor: row.get("noise_floor").and_then(number_as_i64),
            decision: row_string(row, "decision").map(str::to_string),
            score_before: row.get("score_before").and_then(number_as_i64),
            score_after: row.get("score_after").and_then(number_as_i64),
        }
    }
}

struct Recursion {
    digest: String,
    rows: Vec<RowFacts>,
    raw_rows: Vec<Value>,
    wire1: bool,
    wire2: bool,
    wire3: bool,
    wire3_pruned: bool,
    pruned_run: Option<Value>,
    resumed: Option<String>,
    refusal: String,
    refusal_notice: String,
    refusal_phase: String,
    quiet_note: String,
    quiet_rows: Vec<Value>,
    pool: Value,
    propose1: bool,
    propose2: bool,
    propose3: bool,
    drive_rows: Vec<Value>,
    evidence: String,
    pruned: Vec<String>,
    phase: String,
    tip: String,
    head: String,
    harness: String,
    main: String,
    seed: String,
}

impl Default for Recursion {
    fn default() -> Self {
        Self {
            digest: String::new(),
            rows: Vec::new(),
            raw_rows: Vec::new(),
            wire1: false,
            wire2: false,
            wire3: false,
            wire3_pruned: false,
            pruned_run: None,
            resumed: None,
            refusal: String::new(),
            refusal_notice: String::new(),
            refusal_phase: String::new(),
            quiet_note: String::new(),
            quiet_rows: Vec::new(),
            pool: Value::Null,
            propose1: false,
            propose2: false,
            propose3: false,
            drive_rows: Vec::new(),
            evidence: String::new(),
            pruned: Vec::new(),
            phase: String::new(),
            tip: String::new(),
            head: String::new(),
            harness: String::new(),
            main: String::new(),
            seed: String::new(),
        }
    }
}

struct Product {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<std::io::BufReader<ChildStdout>>,
    room: PathBuf,
    data_home: PathBuf,
    error_file: PathBuf,
    root: PathBuf,
}

impl Product {
    fn spawn(root: &str, data_home: &Path, error_file: &Path) -> Product {
        std::fs::create_dir_all(data_home).unwrap();
        let (child, stdin, stdout) = Self::start(Path::new(root), data_home, error_file)
            .unwrap_or_else(|error| panic!("could not start product: {error}"));
        Product {
            child: Some(child),
            stdin: Some(stdin),
            stdout: Some(stdout),
            room: PathBuf::new(),
            data_home: data_home.to_path_buf(),
            error_file: error_file.to_path_buf(),
            root: PathBuf::from(root),
        }
    }

    fn start(root: &Path, data_home: &Path, error_file: &Path) -> Result<(Child, ChildStdin, std::io::BufReader<ChildStdout>), String> {
        let err = std::fs::File::create(error_file).map_err(|error| error.to_string())?;
        let mut child = Command::new(root.join("output/headless/RLLM"))
            .arg("--jsonl")
            .current_dir(root)
            .env("XDG_DATA_HOME", data_home)
            .env_remove("RLLM_SELF_JOB")
            .env_remove("RLLM_SELF_INSTRUCTION")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|error| error.to_string())?;
        let stdin = child.stdin.take().ok_or("controller stdin was not piped")?;
        let stdout = child.stdout.take().ok_or("controller stdout was not piped")?;
        Ok((child, stdin, std::io::BufReader::new(stdout)))
    }

    fn restart(&mut self, room: &Path) -> Result<(), String> {
        self.finish();
        let (child, stdin, stdout) = Self::start(&self.root, &self.data_home, &self.error_file)?;
        self.stdin = Some(stdin);
        self.stdout = Some(stdout);
        self.child = Some(child);
        self.room = room.to_path_buf();
        Ok(())
    }

    fn event(&mut self, state: Option<&Value>, action: &str, inputs: Value, room: &Path) -> Value {
        self.room = room.to_path_buf();
        let envelope = obj(vec![
            ("op".into(), s("event")),
            ("state".into(), state.cloned().unwrap_or(Value::Null)),
            ("action".into(), s(action)),
            ("inputs".into(), inputs),
            ("root".into(), s(room.to_str().unwrap_or(""))),
        ]);
        let reply = self.send(&envelope);
        match reply.get("value") {
            Some(value) => value.clone(),
            None => panic!("{}", reply.to_string()),
        }
    }

    fn send(&mut self, envelope: &Value) -> Value {
        use std::io::{BufRead, Write};
        let stdin = self.stdin.as_mut().unwrap_or_else(|| panic!("controller stdin is closed"));
        let line = envelope.to_string();
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .unwrap_or_else(|error| panic!("controller write: {error}"));
        let stdout = self.stdout.as_mut().unwrap_or_else(|| panic!("controller stdout is closed"));
        let mut response = String::new();
        loop {
            match stdout.read_line(&mut response) {
                Ok(0) => {
                    let tail = std::fs::read_to_string(&self.error_file).unwrap_or_default();
                    let start = tail.len().saturating_sub(2000);
                    panic!("product closed stdout {}", &tail[start..]);
                }
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("controller read: {error}"),
            }
        }
        Value::parse(response.trim_end_matches(['\n', '\r']))
    }

    fn finish(&mut self) {
        self.stdin.take();
        self.stdout.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

impl Drop for Product {
    fn drop(&mut self) {
        self.finish();
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "rrsi-loop"))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("scratch kept at {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

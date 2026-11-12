//! Append-only edit ledger for one refinement run, and the durable improvement
//! library that later tasks read from the same store.
//! The ledger records the verdict the refine loop already reached. It does not choose that verdict.
//! Cost is the UTF-8 length of the instructions being replaced and of the proposal.
//! A full line cap refuses the new record and leaves every existing line in place.
//! The library file `experience.jsonl` is sealed with the same content digest and previous link.
//! Each line is the index entry for the adopted harness and the lessons the ledger supports.
use super::*;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const NOFOLLOW: i32 = 131072;
const CLOEXEC: i32 = 524288;
const PATH_MAX: usize = 1024;
const NAME_MAX: usize = 128;
const FILE_MAX: u64 = 4 * 1024 * 1024;
const LINE_CAP: usize = 64;
const RECORD_MAX: usize = 48 * 1024;
const HYPOTHESIS_MAX: usize = 32768;
const PROMPT_REJECTED: usize = 4;
const PROMPT_HYPOTHESIS: usize = 240;
const PROMPT_MAX: usize = 2048;
const COMPONENTS: &[&str] = &[
    "prompt",
    "control flow",
    "config",
    "context management",
    "tools",
    "skills",
    "memory",
    "subagents",
];
// Gap 2 names the eight editable harness parts. The older stall list above stays
// so an existing ledger still renders the same tried and untried lines.
const EDITABLE: &[&str] = &[
    "system prompt",
    "guardrails",
    "memory",
    "tool guidance",
    "subagents",
    "skills",
    "scripts",
    "plugins",
];
const LEAKAGE: &[&str] = &["clean", "task_text", "answer_text", "inert"];
const ROW_EXTRA: &[&str] = &["cost_change", "accepted", "leakage"];
/// Rounds of a component with no strictly positive score gain before it is pruned.
pub(crate) const PRUNE_WINDOW: usize = 3;
const EDIT_KEYS: &[&str] = &[
    "component",
    "hypothesis",
    "diff_digest",
    "score_before",
    "score_after",
    "cost_before",
    "cost_after",
    "verdict",
    "round",
    "run_digest",
];
const POLICY_KEYS: &[&str] = &["edit_budget", "noise_floor", "cost_known", "decision"];
// Optional fields for a candidate pool, its worktree commit, and the split scores.
// An older row omits them and still seals.
const SPLIT_KEYS: &[&str] = &[
    "pool_status",
    "branch",
    "commit",
    "held_out_known",
    "ood_known",
    "evolve_passed",
    "held_out_passed",
    "ood_passed",
    "evolve_task_key",
    "held_out_task_key",
    "ood_task_key",
    "evolve_verifier",
    "held_out_verifier",
    "ood_verifier",
    "harness_digest",
    "evolve_shape",
    "ood_shape",
];
// Three candidates are a pool, and three refusals still leave one edit of the
// annealed cap of four. The slots are judged in order. There is no thread schedule
// in the ledger. One worktree per slot: a fourth live worktree is refused.
pub(crate) const CANDIDATE_MAX: usize = 3;
pub(crate) const WORKTREE_MAX: usize = 3;
pub(crate) const EVOLVE_BRANCH: &str = "rllm-evolve";
const HARNESS_FILE: &str = "rllm-harness.txt";
const SOURCE_ABSENT: &str = "absent";
const GIT_WALL_SECS: u64 = 15;
const GIT_OUTPUT_MAX: usize = 64 * 1024;
// The product has no fixed round horizon, so each round already in the ledger
// spends one step of the cap. Early rounds may attempt more edits than later ones.
const BUDGET_MAX: u64 = 4;
const BUDGET_MIN: u64 = 1;

pub(crate) fn digest(bytes: &[u8]) -> String {
    super::application::workspace_digest(bytes)
}
fn get<'a>(value: &'a Json, key: &str) -> &'a Json {
    match value {
        Obj(fields) => fields.get(key).unwrap_or(&Null),
        _ => &Null,
    }
}
fn text(value: &Json, key: &str) -> String {
    get(value, key).text().unwrap_or("").to_string()
}
fn put(value: &mut Json, key: &str, item: Json) {
    if let Obj(fields) = value {
        fields.insert(key.into(), item);
    }
}
fn n(value: u64) -> Json {
    Num(value.to_string())
}
fn hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn number_field(value: &Json, key: &str) -> Result<u64> {
    match get(value, key) {
        Num(text) => text.parse::<u64>().map_err(|_| format!("invalid {key}")),
        _ => Err(format!("{key} must be a number")),
    }
}
fn only_keys(value: &Json, keys: &[&str]) -> Result<()> {
    let Obj(fields) = value else {
        return Err("edit ledger record must be an object".into());
    };
    if let Some(key) = fields.keys().find(|key| !keys.contains(&key.as_str())) {
        return Err(format!("unknown edit ledger field {key}"));
    }
    Ok(())
}
fn component_index(name: &str) -> Option<usize> {
    COMPONENTS.iter().position(|item| *item == name)
}
pub(crate) fn known_component(name: &str) -> bool {
    component_index(name).is_some() || EDITABLE.iter().any(|item| *item == name)
}
fn signed_field(value: &Json, key: &str) -> Result<i64> {
    match get(value, key) {
        Num(text) => text.parse::<i64>().map_err(|_| format!("invalid {key}")),
        _ => Err(format!("{key} must be a number")),
    }
}
fn score_missing(value: &Json) -> bool {
    get(value, "score_before") == &Null && get(value, "score_after") == &Null
}
fn validate_fields(value: &Json, expected_run: &str) -> Result<()> {
    let component = text(value, "component");
    if !known_component(&component) {
        return Err("unknown edit component".into());
    }
    let hypothesis = text(value, "hypothesis");
    if hypothesis.is_empty() || hypothesis.len() > HYPOTHESIS_MAX || hypothesis.contains('\0') {
        return Err("hypothesis must be non-empty text within the record cap".into());
    }
    if !hex_digest(&text(value, "diff_digest")) {
        return Err("diff_digest must be 64 lowercase hexadecimal characters".into());
    }
    let verdict = text(value, "verdict");
    if verdict != "accepted" && verdict != "rejected" {
        return Err("verdict must be accepted or rejected".into());
    }
    let round = number_field(value, "round")?;
    if !(1..=1024).contains(&round) {
        return Err("round is outside 1..=1024".into());
    }
    for key in ["cost_before", "cost_after"] {
        number_field(value, key)?;
    }
    let leakage = text(value, "leakage");
    if get(value, "leakage") != &Null && !LEAKAGE.contains(&leakage.as_str()) {
        return Err("leakage must be clean, task_text, answer_text or inert".into());
    }
    let refused = matches!(leakage.as_str(), "task_text" | "answer_text" | "inert");
    let decision_text = text(value, "decision");
    let unscored = refused
        || (score_missing(value)
            && (decision_text.starts_with("refused because component ")
                || decision_text.starts_with("workspace is not")
                // Distinct from the work-tree sentence, and still written before any score exists.
                || decision_text == "workspace identity does not match the approved root"));
    if refused && !score_missing(value) {
        return Err("a leakage refusal must not record a score".into());
    }
    if unscored {
        if verdict != "rejected" {
            return Err("an unscored refusal is rejected".into());
        }
    } else {
        for key in ["score_before", "score_after"] {
            number_field(value, key)?;
        }
    }
    if get(value, "cost_change") != &Null {
        let change = signed_field(value, "cost_change")?;
        let before = number_field(value, "cost_before")? as i64;
        let after = number_field(value, "cost_after")? as i64;
        if change != after - before {
            return Err("cost_change does not match the recorded costs".into());
        }
    }
    if get(value, "accepted") != &Null {
        let accepted = match get(value, "accepted") {
            Bool(bit) => *bit,
            _ => return Err("accepted must be a boolean".into()),
        };
        if accepted != (verdict == "accepted") {
            return Err("accepted does not match the verdict".into());
        }
    }
    let run_digest = text(value, "run_digest");
    if !hex_digest(&run_digest) || run_digest != expected_run {
        return Err("run_digest does not match the run being recorded".into());
    }
    validate_policy(value)?;
    validate_split(value)
}
fn commit_hex(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn bounded_row_text(value: &Json, key: &str, max: usize) -> Result<()> {
    if get(value, key) == &Null {
        return Ok(());
    }
    let text = text(value, key);
    if text.is_empty() || text.len() > max || text.contains('\0') {
        return Err(format!("{key} must be bounded text"));
    }
    Ok(())
}
fn known_leg(value: &Json, known_key: &str, passed_key: &str, task_key: &str, verifier_key: &str) -> Result<()> {
    if get(value, known_key) == &Null {
        for key in [passed_key, task_key, verifier_key] {
            if get(value, key) != &Null {
                return Err(format!("{key} requires {known_key}"));
            }
        }
        return Ok(());
    }
    let known = match get(value, known_key) {
        Bool(bit) => *bit,
        _ => return Err(format!("{known_key} must be a boolean")),
    };
    if !known {
        for key in [passed_key, task_key, verifier_key] {
            if get(value, key) != &Null {
                return Err(format!("{known_key} false cannot carry a score"));
            }
        }
        return Ok(());
    }
    let passed = number_field(value, passed_key)?;
    if passed > 1024 {
        return Err(format!("{passed_key} is outside the record cap"));
    }
    if !hex_digest(&text(value, task_key)) || !hex_digest(&text(value, verifier_key)) {
        return Err(format!("{task_key} and {verifier_key} must be 64 lowercase hexadecimal characters"));
    }
    Ok(())
}
fn validate_split(value: &Json) -> Result<()> {
    if get(value, "pool_status") != &Null {
        let status = text(value, "pool_status");
        let refused = status.strip_prefix("refused:").is_some_and(|cause| !cause.is_empty());
        if (status != "admissible" && !refused) || status.len() > 200 || status.contains('\0') {
            return Err("pool_status must be admissible or refused with a cause".into());
        }
    }
    if get(value, "branch") != &Null && text(value, "branch") != EVOLVE_BRANCH {
        return Err("branch must be the evolve branch".into());
    }
    if get(value, "commit") != &Null && !commit_hex(&text(value, "commit")) {
        return Err("commit must be a lowercase hexadecimal digest".into());
    }
    known_leg(value, "held_out_known", "held_out_passed", "held_out_task_key", "held_out_verifier")?;
    known_leg(value, "ood_known", "ood_passed", "ood_task_key", "ood_verifier")?;
    if get(value, "evolve_passed") != &Null {
        let passed = number_field(value, "evolve_passed")?;
        if passed > 1024 {
            return Err("evolve_passed is outside the record cap".into());
        }
    }
    for key in ["evolve_task_key", "evolve_verifier", "harness_digest"] {
        if get(value, key) != &Null && !hex_digest(&text(value, key)) {
            return Err(format!("{key} must be 64 lowercase hexadecimal characters"));
        }
    }
    bounded_row_text(value, "evolve_shape", 80)?;
    bounded_row_text(value, "ood_shape", 80)
}
fn edit_keys(sealed: bool) -> Vec<&'static str> {
    let mut keys = EDIT_KEYS.to_vec();
    keys.extend_from_slice(POLICY_KEYS);
    keys.extend_from_slice(ROW_EXTRA);
    keys.extend_from_slice(SPLIT_KEYS);
    if sealed {
        keys.extend_from_slice(&["schema", "previous", "revision"]);
    }
    keys
}
fn validate_policy(value: &Json) -> Result<()> {
    if get(value, "edit_budget") != &Null {
        let budget = number_field(value, "edit_budget")?;
        if budget > 1024 {
            return Err("edit budget is outside the record cap".into());
        }
    }
    if get(value, "noise_floor") != &Null {
        let floor = number_field(value, "noise_floor")?;
        if floor > 1024 {
            return Err("noise floor is outside the record cap".into());
        }
    }
    if get(value, "cost_known") != &Null && !matches!(get(value, "cost_known"), Bool(_)) {
        return Err("cost_known must be a boolean".into());
    }
    if get(value, "decision") != &Null {
        let decision = text(value, "decision");
        if decision.is_empty() || decision.len() > 240 || decision.contains('\0') {
            return Err("decision must be bounded text".into());
        }
    }
    Ok(())
}
fn validate_edit(value: &Json, expected_run: &str) -> Result<()> {
    only_keys(value, &edit_keys(false))?;
    validate_fields(value, expected_run)
}
fn body_without_revision(value: &Json) -> Result<Json> {
    let Obj(fields) = value else {
        return Err("edit ledger record must be an object".into());
    };
    let mut body = fields.clone();
    body.remove("revision");
    Ok(Obj(body))
}
fn linked_previous(value: &Json, expected: &str) -> Result<String> {
    only_keys(value, &edit_keys(true))?;
    if text(value, "schema") != "1" {
        return Err("edit ledger record schema is not 1".into());
    }
    if text(value, "previous") != expected {
        return Err("ledger previous link does not match the preceding record".into());
    }
    let revision = text(value, "revision");
    if !hex_digest(&revision) {
        return Err("ledger record is missing its content digest".into());
    }
    let actual = digest(body_without_revision(value)?.string().as_bytes());
    if revision != actual {
        return Err("ledger record digest does not match its body".into());
    }
    validate_fields(value, &text(value, "run_digest"))?;
    Ok(revision)
}
fn parse_chain(lines: &[String]) -> Result<Vec<Json>> {
    if lines.len() > LINE_CAP {
        return Err("edit ledger exceeds its line cap; history was kept and no line was dropped".into());
    }
    let mut records = Vec::with_capacity(lines.len());
    let mut previous = String::new();
    for line in lines {
        if line.is_empty() {
            return Err("ledger contains a blank line; history was not rewritten".into());
        }
        if line.len() > RECORD_MAX {
            return Err("ledger record exceeds the record cap".into());
        }
        let value = parse(line).map_err(|_| "ledger record is not one JSON object".to_string())?;
        previous = linked_previous(&value, &previous)?;
        records.push(value);
    }
    Ok(records)
}
fn load_text(document: &str) -> Result<Vec<Json>> {
    if document.len() as u64 > FILE_MAX {
        return Err("ledger exceeds its size cap".into());
    }
    if !document.is_empty() && !document.ends_with('\n') {
        return Err("ledger document is torn; history was not rewritten".into());
    }
    let lines: Vec<String> = document.lines().map(str::to_string).collect();
    parse_chain(&lines)
}
fn seal(lines: &[Json], edit: &Json, expected_run: &str) -> Result<String> {
    validate_edit(edit, expected_run)?;
    if lines.len() >= LINE_CAP {
        return Err("edit ledger line cap reached; history was kept and no line was dropped".into());
    }
    if let Some(existing) = lines.first() {
        if text(existing, "run_digest") != expected_run {
            return Err("run_digest does not match the run being recorded".into());
        }
    }
    let previous = match lines.last() {
        Some(tail) => text(tail, "revision"),
        None => String::new(),
    };
    let mut record = edit.clone();
    put(&mut record, "schema", s("1"));
    put(&mut record, "previous", s(&previous));
    let revision = digest(body_without_revision(&record)?.string().as_bytes());
    put(&mut record, "revision", s(&revision));
    let line = record.string();
    if line.len() > RECORD_MAX || line.contains('\n') {
        return Err("edit ledger record exceeds the record cap".into());
    }
    Ok(line)
}
fn file_name(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > NAME_MAX
        || value.contains(['\0', '/', '\\'])
        || value == "."
        || value == ".."
    {
        return Err("ledger path must be a single bounded file name".into());
    }
    Ok(value.to_string())
}
fn root_dir(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.len() > PATH_MAX || value.contains('\0') {
        return Err("ledger root is empty or over the path cap".into());
    }
    let path = PathBuf::from(value);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            Err("ledger root must be a real directory".into())
        }
        Ok(_) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
                }
            }
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
            let created = fs::symlink_metadata(&path).map_err(|err| err.to_string())?;
            if created.file_type().is_symlink() || !created.is_dir() {
                return Err("ledger root must be a real directory".into());
            }
            Ok(path)
        }
        Err(error) => Err(error.to_string()),
    }
}
fn ledger_path(root: &str, name: &str) -> Result<PathBuf> {
    let root = root_dir(root)?;
    let name = file_name(name)?;
    let path = root.join(&name);
    if path.parent() != Some(root.as_path()) {
        return Err("ledger path must stay inside its root".into());
    }
    Ok(path)
}
fn read_bounded(path: &Path) -> Result<String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error.to_string()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err("ledger must be a regular non-symlink file".into());
        }
        Ok(meta) if meta.len() > FILE_MAX => return Err("ledger exceeds its size cap".into()),
        Ok(_) => {}
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | CLOEXEC)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > FILE_MAX {
        return Err("ledger exceeds its size cap".into());
    }
    String::from_utf8(bytes).map_err(|_| "ledger is not UTF-8".into())
}
fn append_line(path: &Path, line: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err("ledger must be a regular non-symlink file".into());
        }
        Ok(meta) if meta.len().saturating_add(line.len() as u64).saturating_add(1) > FILE_MAX => {
            return Err("ledger exceeds its size cap; history was kept and no line was dropped".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(NOFOLLOW | CLOEXEC)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(line.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}
fn outcome(lines: usize, revision: &str, document: Option<String>) -> Json {
    let mut fields = vec![
        ("recorded", Bool(true)),
        ("evicted", Bool(false)),
        ("lines", n(lines as u64)),
        ("revision", s(revision)),
    ];
    if let Some(document) = document {
        fields.push(("document", s(&document)));
    }
    obj(fields)
}
fn revision_of(line: &str) -> Result<String> {
    let value = parse(line)?;
    let revision = text(&value, "revision");
    if revision.is_empty() {
        return Err("ledger record is missing its content digest".into());
    }
    Ok(revision)
}

pub(crate) fn diff_digest(before: &str, after: &str) -> String {
    let mut bytes = Vec::with_capacity(before.len().saturating_add(after.len()).saturating_add(1));
    bytes.extend_from_slice(before.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(after.as_bytes());
    digest(&bytes)
}
pub(crate) fn default_root() -> String {
    // Same directory the desktop host uses for saved state (`state.bin` and `key`).
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.local/share")
    });
    PathBuf::from(base).join("rllm").to_string_lossy().into_owned()
}
pub(crate) fn edit(
    component: &str,
    hypothesis: &str,
    diff_digest: &str,
    score_before: u64,
    score_after: u64,
    cost_before: u64,
    cost_after: u64,
    verdict: &str,
    round: u64,
    run_digest: &str,
) -> Result<Json> {
    let value = obj(vec![
        ("component", s(component)),
        ("hypothesis", s(hypothesis)),
        ("diff_digest", s(diff_digest)),
        ("score_before", n(score_before)),
        ("score_after", n(score_after)),
        ("cost_before", n(cost_before)),
        ("cost_after", n(cost_after)),
        ("verdict", s(verdict)),
        ("round", n(round)),
        ("run_digest", s(run_digest)),
    ]);
    validate_edit(&value, run_digest)?;
    Ok(value)
}
/// Attach the history fields a scored edit carries. An old row omits them and still seals.
pub(crate) fn annotate(mut edit: Json, leakage: &str) -> Result<Json> {
    let before = number_field(&edit, "cost_before")? as i64;
    let after = number_field(&edit, "cost_after")? as i64;
    put(&mut edit, "cost_change", Num((after - before).to_string()));
    let accepted = text(&edit, "verdict") == "accepted";
    put(&mut edit, "accepted", Bool(accepted));
    if !leakage.is_empty() {
        put(&mut edit, "leakage", s(leakage));
    }
    validate_edit(&edit, &text(&edit, "run_digest"))?;
    Ok(edit)
}
/// A refusal written before scoring. The row keeps costs and the verdict, and omits both scores.
pub(crate) fn unscored(
    component: &str,
    hypothesis: &str,
    diff_digest: &str,
    cost_before: u64,
    cost_after: u64,
    round: u64,
    run_digest: &str,
    leakage: &str,
    decision: &str,
) -> Result<Json> {
    let mut fields = vec![
        ("component", s(component)),
        ("hypothesis", s(hypothesis)),
        ("diff_digest", s(diff_digest)),
        ("cost_before", n(cost_before)),
        ("cost_after", n(cost_after)),
        ("cost_change", Num((cost_after as i64 - cost_before as i64).to_string())),
        ("verdict", s("rejected")),
        ("accepted", Bool(false)),
        ("round", n(round)),
        ("run_digest", s(run_digest)),
        ("decision", s(decision)),
    ];
    if !leakage.is_empty() {
        fields.push(("leakage", s(leakage)));
    }
    let value = obj(fields);
    validate_edit(&value, run_digest)?;
    Ok(value)
}
pub(crate) fn append_document(document: &str, edit: &Json, run_digest: &str) -> Result<Json> {
    let existing = load_text(document)?;
    let line = seal(&existing, edit, run_digest)?;
    let mut next = document.to_string();
    next.push_str(&line);
    next.push('\n');
    if next.len() as u64 > FILE_MAX {
        return Err("ledger exceeds its size cap; history was kept and no line was dropped".into());
    }
    Ok(outcome(existing.len() + 1, &revision_of(&line)?, Some(next)))
}
pub(crate) fn load_file(root: &str, name: &str) -> Result<String> {
    if root.is_empty() && name.is_empty() {
        return Ok(String::new());
    }
    if root.is_empty() || name.is_empty() {
        return Err("ledger path is incomplete".into());
    }
    read_bounded(&ledger_path(root, name)?)
}
pub(crate) fn append_file(root: &str, name: &str, edit: &Json, run_digest: &str) -> Result<Json> {
    let path = ledger_path(root, name)?;
    let document = read_bounded(&path)?;
    let existing = load_text(&document)?;
    let line = seal(&existing, edit, run_digest)?;
    if document.len().saturating_add(line.len()).saturating_add(1) as u64 > FILE_MAX {
        return Err("ledger exceeds its size cap; history was kept and no line was dropped".into());
    }
    let before = document.clone();
    append_line(&path, &line)?;
    let after = read_bounded(&path)?;
    if !after.starts_with(&before) || after.len() != before.len() + line.len() + 1 {
        return Err("ledger append did not preserve existing history".into());
    }
    Ok(outcome(existing.len() + 1, &revision_of(&line)?, None))
}
pub(crate) fn records_of(document: &str) -> Result<Vec<Json>> {
    load_text(document)
}
pub(crate) fn falsified(records: &[Json], run_digest: &str) -> Vec<Json> {
    records
        .iter()
        .filter(|record| text(record, "run_digest") == run_digest && text(record, "verdict") == "rejected")
        .cloned()
        .collect()
}
pub(crate) fn component_counts(records: &[Json], run_digest: &str) -> Vec<(String, usize)> {
    let mut counts = [0usize; COMPONENTS.len()];
    for record in records {
        if text(record, "run_digest") != run_digest {
            continue;
        }
        if let Some(index) = component_index(&text(record, "component")) {
            counts[index] += 1;
        }
    }
    COMPONENTS
        .iter()
        .enumerate()
        .filter(|(index, _)| counts[*index] > 0)
        .map(|(index, name)| ((*name).to_string(), counts[index]))
        .collect()
}
pub(crate) fn annealed_budget(rounds_recorded: u64) -> u64 {
    BUDGET_MIN.max(BUDGET_MAX.saturating_sub(rounds_recorded))
}
pub(crate) fn budget_for(records: &[Json], run_digest: &str, round: u64) -> (u64, u64) {
    let mut prior = Vec::new();
    let mut spent = 0u64;
    for record in records {
        if text(record, "run_digest") != run_digest {
            continue;
        }
        let Ok(id) = number_field(record, "round") else { continue };
        if id == round {
            spent += 1;
        } else if !prior.contains(&id) {
            prior.push(id);
        }
    }
    let budget = annealed_budget(prior.len() as u64);
    (budget, budget.saturating_sub(spent))
}
pub(crate) fn budget_line(records: &[Json], run_digest: &str, round: u64) -> String {
    let (budget, remaining) = budget_for(records, run_digest, round);
    let mut line = format!("\nEDIT BUDGET\nEdits remaining this round: {remaining}. The annealed budget is {budget}.");
    let pruned = pruned_components(records, run_digest);
    if !pruned.is_empty() {
        line.push_str(" Pruned: ");
        line.push_str(&pruned.join(", "));
        line.push('.');
    }
    line
}
pub(crate) fn rejection_refusal(component: &str, round: u64) -> String {
    format!("refused because component {component} with that hypothesis was already rejected in round {round}")
}
pub(crate) fn prune_refusal(component: &str) -> String {
    format!(
        "refused because component {component} was pruned after {PRUNE_WINDOW} rounds without a strictly positive gain"
    )
}
pub(crate) fn inert_refusal(component: &str) -> String {
    format!("refused because the candidate adds no change to component {component}")
}
pub(crate) fn prior_rejection(records: &[Json], run_digest: &str, component: &str, hypothesis: &str) -> Option<u64> {
    let mut found = None;
    for record in records {
        if text(record, "run_digest") != run_digest
            || text(record, "component") != component
            || text(record, "verdict") != "rejected"
            || text(record, "hypothesis") != hypothesis
        {
            continue;
        }
        if let Ok(round) = number_field(record, "round") {
            if found.is_none() {
                found = Some(round);
            }
        }
    }
    found
}
pub(crate) fn incumbent_diff(records: &[Json], run_digest: &str, component: &str) -> Option<String> {
    records.iter().rev().find_map(|record| {
        if text(record, "run_digest") != run_digest
            || text(record, "component") != component
            || text(record, "verdict") != "accepted"
        {
            return None;
        }
        let diff = text(record, "diff_digest");
        hex_digest(&diff).then_some(diff)
    })
}
fn edit_gain(record: &Json) -> Option<i64> {
    let before = number_field(record, "score_before").ok()? as i64;
    let after = number_field(record, "score_after").ok()? as i64;
    Some(after - before)
}
fn component_rounds(records: &[Json], run_digest: &str, component: &str) -> Vec<(u64, i64)> {
    let mut rounds = Vec::new();
    for record in records {
        if text(record, "run_digest") != run_digest || text(record, "component") != component {
            continue;
        }
        let (Ok(round), Some(gain)) = (number_field(record, "round"), edit_gain(record)) else {
            continue;
        };
        if let Some(existing) = rounds.iter_mut().find(|(id, _)| *id == round) {
            if gain > existing.1 {
                existing.1 = gain;
            }
        } else {
            rounds.push((round, gain));
        }
    }
    rounds.sort_by_key(|(round, _)| *round);
    rounds
}
fn editable_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = EDITABLE.to_vec();
    for name in COMPONENTS {
        if !names.contains(name) {
            names.push(*name);
        }
    }
    names
}
pub(crate) fn pruned_components(records: &[Json], run_digest: &str) -> Vec<String> {
    let mut pruned = Vec::new();
    for name in editable_names() {
        let rounds = component_rounds(records, run_digest, name);
        if rounds.len() < PRUNE_WINDOW {
            continue;
        }
        let window = &rounds[rounds.len() - PRUNE_WINDOW..];
        if window.iter().all(|(_, gain)| *gain <= 0) {
            pruned.push((*name).to_string());
        }
    }
    pruned
}
pub(crate) fn live_components(records: &[Json], run_digest: &str) -> Vec<&'static str> {
    let pruned = pruned_components(records, run_digest);
    EDITABLE
        .iter()
        .copied()
        .filter(|name| pruned.iter().all(|pruned_name| pruned_name != name))
        .collect()
}
// Every row stores measured lengths. cost_known is false when a row had no pair
// of costs to compare, and those lengths are not a later comparison.
pub(crate) fn incumbent_cost(records: &[Json], run_digest: &str) -> Option<u64> {
    records.iter().rev().find_map(|record| {
        if text(record, "run_digest") != run_digest || text(record, "verdict") != "accepted" {
            return None;
        }
        if get(record, "cost_known") != &Bool(true) {
            return None;
        }
        match get(record, "cost_after") {
            Num(value) => value.parse().ok(),
            _ => None,
        }
    })
}
pub(crate) fn untried_components(records: &[Json], run_digest: &str) -> Vec<&'static str> {
    let tried = component_counts(records, run_digest);
    COMPONENTS
        .iter()
        .copied()
        .filter(|name| tried.iter().all(|(tried_name, _)| tried_name != name))
        .collect()
}
fn clip(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = value[..end].to_string();
    if end < value.len() {
        out.push_str("...");
    }
    out
}
pub(crate) fn prompt_document(document: &str, run_digest: &str) -> Result<String> {
    if document.is_empty() {
        return Ok(String::new());
    }
    if !hex_digest(run_digest) {
        return Err("run_digest does not match the run being recorded".into());
    }
    let records = records_of(document)?;
    if records.is_empty() {
        return Ok(String::new());
    }
    let rejected = falsified(&records, run_digest);
    let shown: Vec<&Json> = rejected.iter().rev().take(PROMPT_REJECTED).collect();
    let mut lines = Vec::new();
    if rejected.is_empty() {
        lines.push("Rejected hypotheses (do not propose these again): none".to_string());
    } else {
        lines.push("Rejected hypotheses (do not propose these again):".to_string());
        for record in shown.into_iter().rev() {
            let decision = if get(record, "accepted") == &Null {
                String::new()
            } else {
                format!(" · {}", text(record, "verdict"))
            };
            lines.push(format!(
                "- round {} · {} · {}{}",
                number_field(record, "round")?,
                text(record, "component"),
                clip(&text(record, "hypothesis"), PROMPT_HYPOTHESIS),
                decision
            ));
        }
        let hidden = rejected.len().saturating_sub(PROMPT_REJECTED);
        if hidden > 0 {
            lines.push(format!("Earlier rejected entries retained in the ledger: {hidden}"));
        }
    }
    let tried = component_counts(&records, run_digest)
        .into_iter()
        .map(|(name, count)| format!("{name} ({count})"))
        .collect::<Vec<_>>()
        .join(", ");
    let untried = untried_components(&records, run_digest).join(", ");
    lines.push(format!("Tried: {tried}"));
    lines.push(format!(
        "Not yet tried: {}",
        if untried.is_empty() { "none".to_string() } else { untried }
    ));
    let pruned = pruned_components(&records, run_digest);
    if !pruned.is_empty() {
        lines.push(format!(
            "Pruned components (do not propose these): {}",
            pruned.join(", ")
        ));
        let live = live_components(&records, run_digest).join(", ");
        lines.push(format!(
            "Live components: {}",
            if live.is_empty() { "none".to_string() } else { live }
        ));
    }
    let mut section = format!("\nEDIT HISTORY\n{}", lines.join("\n"));
    if section.len() > PROMPT_MAX {
        section.truncate(PROMPT_MAX);
        while !section.is_char_boundary(section.len()) {
            section.pop();
        }
    }
    Ok(section)
}

const LIBRARY_FILE: &str = "experience.jsonl";
const LIBRARY_SCHEMA: &str = "library-1";
const LIBRARY_PROMPT_MAX: usize = 4096;
const LIBRARY_TASK_MAX: usize = 4096;
const LIBRARY_HARNESS_MAX: usize = 12288;
const LIBRARY_LESSON_MAX: usize = 3072;
const LIBRARY_FALSIFIED_MAX: usize = 8;
const LIBRARY_SHOWN: usize = 4;
const LIBRARY_KEYS: &[&str] = &[
    "kind",
    "key",
    "task",
    "task_complete",
    "harness",
    "harness_digest",
    "run_digest",
    "falsified",
    "untried",
    "omitted_falsified",
    "lesson",
];
const FALSIFIED_KEYS: &[&str] = &["component", "diff_digest", "hypothesis", "revision", "round"];

fn library_gate() -> std::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GATE.lock().unwrap_or_else(|error| error.into_inner())
}
fn cut(value: &str, limit: usize) -> (String, bool) {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), end < value.len())
}
fn num_text(value: &Json, key: &str) -> String {
    match get(value, key) {
        Num(text) => text.clone(),
        _ => String::new(),
    }
}
fn library_lesson(falsified: &[Json], untried: &[String], omitted: usize) -> String {
    let mut out = String::from("Lessons the sealed edit ledger supports: ");
    if falsified.is_empty() {
        out.push_str("no hypothesis was falsified. ");
    } else {
        out.push_str("falsified hypotheses were rejected by the measured checks: ");
        for (index, item) in falsified.iter().enumerate() {
            if index > 0 {
                out.push_str("; ");
            }
            out.push_str(&format!(
                "round {} {} revision {} diff {} ({})",
                num_text(item, "round"),
                text(item, "component"),
                text(item, "revision"),
                text(item, "diff_digest"),
                text(item, "hypothesis")
            ));
        }
        out.push_str(". ");
        if omitted > 0 {
            out.push_str(&format!(
                "Earlier falsified entries remain in the edit ledger: {omitted}. "
            ));
        }
    }
    if untried.is_empty() {
        out.push_str("Every listed component was tried.");
    } else {
        out.push_str("Components with no trial in this run: ");
        out.push_str(&untried.join(", "));
        out.push('.');
    }
    out
}
fn library_body(task: &str, harness: &str, ledger: &str, run_digest: &str) -> Result<Json> {
    if task.is_empty() || task.contains('\0') {
        return Err("improvement library task must be non-empty text".into());
    }
    if harness.trim().is_empty() || harness.contains('\0') {
        return Err("improvement library harness must be non-empty text".into());
    }
    if !hex_digest(run_digest) {
        return Err("improvement library requires the sealed run digest".into());
    }
    let records = if ledger.is_empty() {
        Vec::new()
    } else {
        records_of(ledger)?
    };
    let rejected = falsified(&records, run_digest);
    let omitted = rejected.len().saturating_sub(LIBRARY_FALSIFIED_MAX);
    let mut shown = Vec::new();
    for record in rejected.iter().rev().take(LIBRARY_FALSIFIED_MAX).rev() {
        shown.push(obj(vec![
            ("hypothesis", s(&clip(&text(record, "hypothesis"), PROMPT_HYPOTHESIS))),
            ("revision", s(&text(record, "revision"))),
            ("diff_digest", s(&text(record, "diff_digest"))),
            ("component", s(&text(record, "component"))),
            ("round", get(record, "round").clone()),
        ]));
    }
    let untried: Vec<String> = untried_components(&records, run_digest)
        .into_iter()
        .map(str::to_string)
        .collect();
    let (task_text, task_cut) = cut(task, LIBRARY_TASK_MAX);
    let (harness_text, _) = cut(harness, LIBRARY_HARNESS_MAX);
    let (lesson, _) = cut(&library_lesson(&shown, &untried, omitted), LIBRARY_LESSON_MAX);
    Ok(obj(vec![
        ("kind", s("experience")),
        ("key", s(&digest(task.as_bytes()))),
        ("task", s(&task_text)),
        ("task_complete", Bool(!task_cut)),
        ("harness", s(&harness_text)),
        ("harness_digest", s(&digest(harness_text.as_bytes()))),
        ("run_digest", s(run_digest)),
        ("falsified", Arr(shown)),
        ("untried", Arr(untried.into_iter().map(|name| s(&name)).collect())),
        ("omitted_falsified", n(omitted as u64)),
        ("lesson", s(&lesson)),
    ]))
}
fn validate_library(value: &Json) -> Result<()> {
    if text(value, "kind") != "experience" {
        return Err("improvement library record kind must be experience".into());
    }
    let key = text(value, "key");
    if !hex_digest(&key) {
        return Err("library index key must be 64 lowercase hexadecimal characters".into());
    }
    let task = text(value, "task");
    if task.is_empty() || task.len() > LIBRARY_TASK_MAX || task.contains('\0') {
        return Err("library task must be non-empty text within the record cap".into());
    }
    match get(value, "task_complete") {
        Bool(true) if key != digest(task.as_bytes()) => {
            return Err("library index key does not match the stored task".into());
        }
        Bool(_) => {}
        _ => return Err("library task_complete must be a boolean".into()),
    }
    let harness = text(value, "harness");
    if harness.is_empty() || harness.len() > LIBRARY_HARNESS_MAX || harness.contains('\0') {
        return Err("library harness must be non-empty text within the record cap".into());
    }
    let harness_digest = text(value, "harness_digest");
    if !hex_digest(&harness_digest) || harness_digest != digest(harness.as_bytes()) {
        return Err("library harness digest does not match the stored harness".into());
    }
    if !hex_digest(&text(value, "run_digest")) {
        return Err("library run_digest must be 64 lowercase hexadecimal characters".into());
    }
    let falsified = match get(value, "falsified") {
        Arr(items) => items,
        _ => return Err("library falsified must be an array".into()),
    };
    if falsified.len() > LIBRARY_FALSIFIED_MAX {
        return Err("library falsified list exceeds its cap".into());
    }
    for item in falsified {
        only_keys(item, FALSIFIED_KEYS)?;
        let hypothesis = text(item, "hypothesis");
        if hypothesis.is_empty() || hypothesis.len() > PROMPT_HYPOTHESIS + 3 || hypothesis.contains('\0') {
            return Err("falsified hypothesis must be bounded text".into());
        }
        if !hex_digest(&text(item, "revision")) || !hex_digest(&text(item, "diff_digest")) {
            return Err("falsified entry digest must be 64 lowercase hexadecimal characters".into());
        }
        let component = text(item, "component");
        if component.is_empty() || component.len() > 64 || component.contains('\0') {
            return Err("falsified component must be bounded text".into());
        }
        let round = number_field(item, "round")?;
        if !(1..=1024).contains(&round) {
            return Err("falsified round is outside 1..=1024".into());
        }
    }
    let untried = match get(value, "untried") {
        Arr(items) => items,
        _ => return Err("library untried must be an array".into()),
    };
    if untried.len() > COMPONENTS.len() {
        return Err("library untried list exceeds its cap".into());
    }
    let mut seen = Vec::new();
    for item in untried {
        let name = item.text().map_err(|_| "untried component must be text".to_string())?;
        if name.is_empty() || name.len() > 64 || seen.iter().any(|seen: &String| seen == name) {
            return Err("untried component must be bounded unique text".into());
        }
        seen.push(name.to_string());
    }
    let omitted = number_field(value, "omitted_falsified")?;
    if omitted > 1024 {
        return Err("omitted falsified count is outside the record cap".into());
    }
    let lesson = text(value, "lesson");
    if lesson.is_empty() || lesson.len() > LIBRARY_LESSON_MAX || lesson.contains('\0') {
        return Err("library lesson must be non-empty text within the record cap".into());
    }
    Ok(())
}
fn seal_library(lines: &[Json], entry: &Json) -> Result<String> {
    only_keys(entry, LIBRARY_KEYS)?;
    validate_library(entry)?;
    if lines.len() >= LINE_CAP {
        return Err("improvement library line cap reached; history was kept and no line was dropped".into());
    }
    let previous = match lines.last() {
        Some(tail) => text(tail, "revision"),
        None => String::new(),
    };
    let mut record = entry.clone();
    put(&mut record, "schema", s(LIBRARY_SCHEMA));
    put(&mut record, "previous", s(&previous));
    let revision = digest(body_without_revision(&record)?.string().as_bytes());
    put(&mut record, "revision", s(&revision));
    let line = record.string();
    if line.len() > RECORD_MAX || line.contains('\n') {
        return Err("improvement library record exceeds the record cap; history was kept and no line was dropped".into());
    }
    Ok(line)
}
fn linked_library(value: &Json, expected: &str) -> Result<String> {
    let mut keys = LIBRARY_KEYS.to_vec();
    keys.extend(["schema", "previous", "revision"]);
    only_keys(value, &keys)?;
    if text(value, "schema") != LIBRARY_SCHEMA {
        return Err("improvement library record schema is not library-1; existing history was left unchanged".into());
    }
    if text(value, "previous") != expected {
        return Err("library previous link does not match the preceding record; existing history was left unchanged".into());
    }
    let revision = text(value, "revision");
    if !hex_digest(&revision) {
        return Err("library record is missing its content digest; existing history was left unchanged".into());
    }
    let actual = digest(body_without_revision(value)?.string().as_bytes());
    if revision != actual {
        return Err("library record digest does not match its body; existing history was left unchanged".into());
    }
    validate_library(value)?;
    Ok(revision)
}
fn parse_library(lines: &[String]) -> Result<Vec<Json>> {
    if lines.len() > LINE_CAP {
        return Err("improvement library exceeds its line cap; history was kept and no line was dropped".into());
    }
    let mut records = Vec::with_capacity(lines.len());
    let mut previous = String::new();
    for line in lines {
        if line.is_empty() {
            return Err("library contains a blank line; existing history was left unchanged".into());
        }
        if line.len() > RECORD_MAX {
            return Err("library record exceeds the record cap; existing history was left unchanged".into());
        }
        let value = parse(line).map_err(|_| {
            "library record is not one JSON object; existing history was left unchanged".to_string()
        })?;
        previous = linked_library(&value, &previous)?;
        records.push(value);
    }
    Ok(records)
}
fn load_library_text(document: &str) -> Result<Vec<Json>> {
    if document.len() as u64 > FILE_MAX {
        return Err("improvement library exceeds its size cap; existing history was left unchanged".into());
    }
    if !document.is_empty() && !document.ends_with('\n') {
        return Err("improvement library document is torn; existing history was left unchanged".into());
    }
    parse_library(&document.lines().map(str::to_string).collect::<Vec<_>>())
}
fn index_of(record: &Json) -> Json {
    obj(vec![
        ("key", s(&text(record, "key"))),
        ("revision", s(&text(record, "revision"))),
        ("run_digest", s(&text(record, "run_digest"))),
    ])
}
fn load_experience_unlocked(root: &str) -> Result<Vec<Json>> {
    load_library_text(&load_file(root, LIBRARY_FILE)?)
}
pub(crate) fn library_index(records: &[Json]) -> Vec<Json> {
    records.iter().map(index_of).collect()
}
pub(crate) fn load_experience(root: &str) -> Result<Vec<Json>> {
    let _gate = library_gate();
    load_experience_unlocked(root)
}
pub(crate) fn append_experience(root: &str, task: &str, harness: &str, ledger: &str, run_digest: &str) -> Result<Json> {
    let entry = library_body(task, harness, ledger, run_digest)?;
    let _gate = library_gate();
    let path = ledger_path(root, LIBRARY_FILE)?;
    let document = read_bounded(&path)?;
    let existing = load_library_text(&document)?;
    if let Some(prior) = existing.iter().rev().find(|record| {
        text(record, "run_digest") == run_digest
            && text(record, "key") == text(&entry, "key")
            && text(record, "harness_digest") == text(&entry, "harness_digest")
    }) {
        let mut saved = outcome(existing.len(), &text(prior, "revision"), None);
        put(&mut saved, "key", s(&text(prior, "key")));
        put(&mut saved, "index", index_of(prior));
        return Ok(saved);
    }
    let line = seal_library(&existing, &entry)?;
    if document.len().saturating_add(line.len()).saturating_add(1) as u64 > FILE_MAX {
        return Err("improvement library exceeds its size cap; history was kept and no line was dropped".into());
    }
    let before = document.clone();
    append_line(&path, &line)?;
    let after = read_bounded(&path)?;
    if !after.starts_with(&before) || after.len() != before.len() + line.len() + 1 {
        return Err("improvement library append did not preserve existing history".into());
    }
    let revision = revision_of(&line)?;
    let sealed = parse(&line)?;
    let mut saved = outcome(existing.len() + 1, &revision, None);
    put(&mut saved, "key", get(&entry, "key").clone());
    put(&mut saved, "index", index_of(&sealed));
    Ok(saved)
}
fn falsified_line(record: &Json) -> String {
    let Arr(items) = get(record, "falsified") else {
        return "Falsified hypotheses: none".into();
    };
    if items.is_empty() {
        return "Falsified hypotheses: none".into();
    }
    let shown = items
        .iter()
        .map(|item| {
            format!(
                "round {} {} revision {}",
                num_text(item, "round"),
                text(item, "component"),
                text(item, "revision")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("Falsified hypotheses: {shown}")
}
fn untried_line(record: &Json) -> String {
    let Arr(items) = get(record, "untried") else {
        return "Untried components: none".into();
    };
    if items.is_empty() {
        return "Untried components: none".into();
    }
    let names = items.iter().filter_map(|item| item.text().ok()).collect::<Vec<_>>().join(", ");
    format!("Untried components: {names}")
}
pub(crate) fn render_historical(records: &[Json], task: &str) -> String {
    if records.is_empty() {
        return String::new();
    }
    let want = digest(task.as_bytes());
    let mut matched = Vec::new();
    let mut other = Vec::new();
    for record in records {
        if text(record, "key") == want {
            matched.push(record);
        } else {
            other.push(record);
        }
    }
    matched.reverse();
    other.reverse();
    let mut selected = matched;
    selected.extend(other);
    selected.truncate(LIBRARY_SHOWN);
    let mut lines = vec![
        "HISTORICAL EXPERIENCE".to_string(),
        "These records are historical experience from earlier tasks. They do not outrank the current task, its permissions, or the base harness.".to_string(),
    ];
    for record in selected {
        let relation = if text(record, "key") == want {
            "indexed key matches this task"
        } else {
            "indexed key is from another task"
        };
        lines.push(format!(
            "- historical entry · {relation} · key {} · revision {}",
            text(record, "key"),
            text(record, "revision")
        ));
        lines.push(format!(
            "  Adopted harness (historical) · digest {}: {}",
            text(record, "harness_digest"),
            clip(&text(record, "harness"), 480)
        ));
        lines.push(format!("  Evidence lesson: {}", clip(&text(record, "lesson"), 480)));
        lines.push(format!("  {}", falsified_line(record)));
        lines.push(format!("  {}", untried_line(record)));
    }
    let mut section = format!("\n{}", lines.join("\n"));
    if section.len() > LIBRARY_PROMPT_MAX {
        section.truncate(LIBRARY_PROMPT_MAX);
        while !section.is_char_boundary(section.len()) {
            section.pop();
        }
    }
    section
}
pub(crate) fn historical_section(root: &str, task: &str) -> Result<String> {
    let records = load_experience(root).map_err(|error| {
        if error.contains("unchanged") || error.contains("kept") {
            error
        } else {
            format!("{error}; existing history was left unchanged")
        }
    })?;
    Ok(render_historical(&records, task))
}

pub(crate) fn pool_status(admissible: bool, cause: &str) -> String {
    if admissible {
        return "admissible".to_string();
    }
    let mut cause = cause.replace(['\n', '\r'], " ");
    const PREFIX: &str = "refused:";
    let room = 200usize.saturating_sub(PREFIX.len());
    if cause.len() > room {
        cause.truncate(room);
        while !cause.is_char_boundary(cause.len()) {
            cause.pop();
        }
    }
    if cause.is_empty() {
        cause = "unspecified".to_string();
    }
    format!("{PREFIX}{cause}")
}

/// Copy the optional pool and split fields onto a row that already validates.
pub(crate) fn attach(mut edit: Json, extra: &Json) -> Result<Json> {
    for key in SPLIT_KEYS {
        if get(extra, key) != &Null {
            put(&mut edit, key, get(extra, key).clone());
        }
    }
    validate_edit(&edit, &text(&edit, "run_digest"))?;
    Ok(edit)
}

fn library_error(error: String) -> String {
    if error.contains("unchanged") || error.contains("kept") {
        error
    } else {
        format!("{error}; existing history was left unchanged")
    }
}

/// Load the sealed ledger and library into a new process. This reads both files
/// and does not rewrite them. The budget is `budget_for` on the recorded rounds.
pub(crate) fn resume_run(root: &str, ledger_name: &str, task: &str) -> Result<Json> {
    if task.is_empty() || task.contains('\0') || task.len() > LIBRARY_TASK_MAX {
        return Err("resume task must be non-empty text within the library cap".into());
    }
    let library = load_experience(root).map_err(library_error)?;
    let document = load_file(root, ledger_name).map_err(library_error)?;
    let records = records_of(&document).map_err(library_error)?;
    if records.is_empty() {
        return Err("resume needs the sealed ledger; existing history was left unchanged".into());
    }
    let run_digest = text(&records[0], "run_digest");
    if records.iter().any(|row| text(row, "run_digest") != run_digest) {
        return Err("resume ledger mixes runs; existing history was left unchanged".into());
    }
    let mut round = 0u64;
    for record in &records {
        if let Ok(value) = number_field(record, "round") {
            round = round.max(value);
        }
    }
    let next = round.saturating_add(1).min(1024);
    let (budget, remaining) = budget_for(&records, &run_digest, next);
    let history = prompt_document(&document, &run_digest).map_err(library_error)?;
    let prompt = format!("{history}{}{}", budget_line(&records, &run_digest, next), render_historical(&library, task));
    let adopted = library.iter().rev().find(|record| text(record, "run_digest") == run_digest);
    let harness = adopted.map(|record| text(record, "harness")).unwrap_or_default();
    let harness_digest = adopted.map(|record| text(record, "harness_digest")).unwrap_or_default();
    let pruned = pruned_components(&records, &run_digest);
    let rejected = falsified(&records, &run_digest);
    Ok(obj(vec![
        ("run_digest", s(&run_digest)),
        ("round", n(round)),
        ("next_round", n(next)),
        ("edit_budget", n(budget)),
        ("edits_remaining", n(remaining)),
        ("adopted_harness", s(&harness)),
        ("harness_digest", s(&harness_digest)),
        ("prompt", s(&prompt)),
        ("pruned", Arr(pruned.into_iter().map(|name| s(&name)).collect())),
        ("rejected", n(rejected.len() as u64)),
        ("edit_ledger", s(&document)),
    ]))
}

pub(crate) struct GitRoot {
    workspace: PathBuf,
    git_dir: PathBuf,
    head: String,
}

fn git_not_cancelled() -> bool {
    false
}

fn path_text(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("path is not UTF-8")?;
    if text.is_empty() || text.len() > PATH_MAX || text.contains('\0') {
        return Err("path is outside the path cap".into());
    }
    Ok(text.to_string())
}

fn git_run(git_dir: &Path, work_tree: &Path, args: &[&str]) -> Result<super::execution::BoundedOutput> {
    let hooks = git_dir.join("rllm-empty-hooks");
    fs::create_dir_all(&hooks).map_err(|error| error.to_string())?;
    let mut full = vec![
        "--git-dir".to_string(),
        path_text(git_dir)?,
        "--work-tree".to_string(),
        path_text(work_tree)?,
        "-c".into(),
        "safe.directory=*".into(),
        "-c".into(),
        "user.name=RLLM".into(),
        "-c".into(),
        "user.email=rllm@localhost".into(),
        "-c".into(),
        "commit.gpgsign=false".into(),
        "-c".into(),
        format!("core.hooksPath={}", path_text(&hooks)?),
    ];
    full.extend(args.iter().copied().map(str::to_string));
    let output = super::execution::bounded_process(
        "git",
        &full,
        work_tree,
        std::time::Duration::from_secs(GIT_WALL_SECS),
        &git_not_cancelled,
    )?;
    if output.stdout.len() > GIT_OUTPUT_MAX || output.stderr.len() > GIT_OUTPUT_MAX {
        return Err("git output exceeded its bound".into());
    }
    Ok(output)
}

fn git_detail(args: &[&str], output: &super::execution::BoundedOutput) -> String {
    let mut detail = output.stderr.trim().to_string();
    if detail.is_empty() {
        detail = output.stdout.trim().to_string();
    }
    if detail.len() > 240 {
        detail.truncate(240);
    }
    format!("git {} exited {:?}: {detail}", args.first().unwrap_or(&"git"), output.code)
}

fn git_ok(git_dir: &Path, work_tree: &Path, args: &[&str]) -> Result<super::execution::BoundedOutput> {
    let output = git_run(git_dir, work_tree, args)?;
    if output.code != Some(0) {
        return Err(git_detail(args, &output));
    }
    Ok(output)
}

fn ignore_worktrees(git_dir: &Path) -> Result<()> {
    let info = git_dir.join("info");
    fs::create_dir_all(&info).map_err(|error| error.to_string())?;
    let exclude = info.join("exclude");
    let mut body = if exclude.exists() {
        fs::read_to_string(&exclude).map_err(|error| error.to_string())?
    } else {
        String::new()
    };
    // The slot checkouts stay out of status. The caller copy of the harness file is
    // excluded too, so applying it does not stage the caller's index. The slot commit
    // force-adds that same name.
    let mut changed = false;
    for line in [".rllm-worktrees/", HARNESS_FILE] {
        if body.lines().any(|existing| existing == line) {
            continue;
        }
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(line);
        body.push('\n');
        changed = true;
    }
    if changed {
        fs::write(&exclude, body).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn path_has_slot(path: &Path) -> bool {
    path.components().any(|component| component.as_os_str() == ".rllm-worktrees")
}

fn locate_worktree(workspace: &Path) -> Result<PathBuf> {
    let meta = match fs::symlink_metadata(workspace) {
        Ok(meta) if !meta.file_type().is_symlink() && meta.is_dir() => meta,
        _ => return Err("workspace is not the root of a git work tree".into()),
    };
    let _ = meta;
    if path_has_slot(workspace) {
        return Err("workspace is not the root of a git work tree".into());
    }
    // A relative --git-dir is resolved from the work-tree directory, so git has
    // to see the canonical directory rather than the spelling the caller used.
    let wanted = fs::canonicalize(workspace).map_err(|error| error.to_string())?;
    if path_has_slot(&wanted) {
        return Err("workspace is not the root of a git work tree".into());
    }
    let git_dir = wanted.join(".git");
    match fs::symlink_metadata(&git_dir) {
        Ok(meta) if !meta.file_type().is_symlink() && meta.is_dir() => {}
        _ => return Err("workspace is not the root of a git work tree".into()),
    }
    Ok(wanted)
}

fn finish_worktree(wanted: PathBuf) -> Result<GitRoot> {
    let git_dir = wanted.join(".git");
    let inside = git_ok(&git_dir, &wanted, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.stdout.trim() != "true" {
        return Err("workspace is not the root of a git work tree".into());
    }
    let top = git_ok(&git_dir, &wanted, &["rev-parse", "--show-toplevel"])?;
    let got = fs::canonicalize(Path::new(top.stdout.trim())).map_err(|error| error.to_string())?;
    if wanted != got || path_has_slot(&wanted) {
        return Err("workspace is not the root of a git work tree".into());
    }
    let head = git_ok(&git_dir, &wanted, &["rev-parse", "HEAD"])?;
    let head = head.stdout.trim().to_string();
    if !commit_hex(&head) {
        return Err("workspace HEAD is not a commit".into());
    }
    ignore_worktrees(&git_dir)?;
    Ok(GitRoot { workspace: wanted, git_dir, head })
}

pub(crate) fn workspace_text(root: &GitRoot) -> Result<String> {
    path_text(&root.workspace)
}

pub(crate) fn workspace_identity(path: &Path) -> Result<String> {
    let meta = fs::metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_dir() {
        return Err("workspace is not the root of a git work tree".into());
    }
    Ok(format!("{}:{}", meta.dev(), meta.ino()))
}

/// Resolve `workspace` and, when `recorded` is non-empty, require that inode.
/// An identity mismatch returns before git runs, so the other tree is not touched.
pub(crate) fn require_recorded_worktree(workspace: &Path, recorded: &str) -> Result<GitRoot> {
    let wanted = locate_worktree(workspace)?;
    if !recorded.is_empty() {
        let identity = workspace_identity(&wanted)?;
        if identity != recorded {
            return Err("workspace identity does not match the approved root".into());
        }
    }
    finish_worktree(wanted)
}

pub(crate) fn require_worktree_root(workspace: &Path) -> Result<GitRoot> {
    require_recorded_worktree(workspace, "")
}

fn slot_path(workspace: &Path, slot: usize) -> PathBuf {
    workspace.join(".rllm-worktrees").join(format!("c{slot}"))
}

fn linked_git_dir(worktree: &Path) -> Result<PathBuf> {
    let dot = worktree.join(".git");
    let meta = fs::symlink_metadata(&dot).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() {
        return Err("worktree gitdir is a symlink".into());
    }
    if meta.is_file() {
        let body = fs::read_to_string(&dot).map_err(|error| error.to_string())?;
        let line = body.lines().next().unwrap_or("");
        let rest = line.strip_prefix("gitdir: ").ok_or("worktree gitdir is missing")?;
        return Ok(PathBuf::from(rest.trim()));
    }
    if meta.is_dir() {
        return Ok(dot);
    }
    Err("worktree gitdir is not a file".into())
}

fn rllm_worktree_count(root: &GitRoot) -> Result<usize> {
    let output = git_ok(&root.git_dir, &root.workspace, &["worktree", "list", "--porcelain"])?;
    let mut count = 0usize;
    for line in output.stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if path.contains(".rllm-worktrees/") {
                count += 1;
            }
        }
    }
    Ok(count)
}

fn remove_slot(root: &GitRoot, path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err("candidate worktree path is a symlink".into()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    }
    let shown = path_text(path)?;
    let _ = git_run(&root.git_dir, &root.workspace, &["worktree", "remove", "--force", &shown])?;
    if path.exists() {
        fs::remove_dir_all(path).map_err(|error| error.to_string())?;
        let _ = git_run(&root.git_dir, &root.workspace, &["worktree", "prune"]);
    }
    if path.exists() {
        return Err("candidate slot was left in place".into());
    }
    Ok(())
}

/// Drop every slot directory from an aborted round. A symlink is left in place.
pub(crate) fn discard_round_slots(root: &GitRoot) {
    for slot in 0..WORKTREE_MAX {
        let path = slot_path(&root.workspace, slot);
        if matches!(fs::symlink_metadata(&path), Ok(meta) if meta.file_type().is_symlink()) {
            continue;
        }
        let _ = remove_slot(root, &path);
    }
}

/// The caller HEAD is the incumbent commit unless an evolve branch already exists.
pub(crate) fn incumbent_base(root: &GitRoot) -> Result<String> {
    let output = git_run(
        &root.git_dir,
        &root.workspace,
        &["rev-parse", "-q", "--verify", &format!("refs/heads/{EVOLVE_BRANCH}")],
    )?;
    if output.code == Some(0) {
        let tip = output.stdout.trim().to_string();
        if commit_hex(&tip) {
            return Ok(tip);
        }
    }
    Ok(root.head.clone())
}

/// Commit `body` in slot `slot`, starting from `base`. Does not check out the caller's branch.
pub(crate) fn commit_candidate(root: &GitRoot, slot: usize, body: &str, base: &str) -> Result<String> {
    if slot >= WORKTREE_MAX {
        return Err(format!("refusing to create more than {WORKTREE_MAX} worktrees"));
    }
    if !commit_hex(base) {
        return Err("worktree base is not a commit".into());
    }
    if body.contains('\0') || body.len() > 32768 {
        return Err("candidate harness exceeds 32 KiB".into());
    }
    let path = slot_path(&root.workspace, slot);
    remove_slot(root, &path)?;
    if path.exists() {
        return Err("candidate slot was left in place".into());
    }
    let live = rllm_worktree_count(root)?;
    if live >= WORKTREE_MAX {
        return Err(format!("refusing to create more than {WORKTREE_MAX} worktrees"));
    }
    fs::create_dir_all(root.workspace.join(".rllm-worktrees")).map_err(|error| error.to_string())?;
    let shown = path_text(&path)?;
    let created = commit_in_slot(root, &path, &shown, body, base);
    if created.is_err() {
        let _ = remove_slot(root, &path);
    }
    created
}

fn commit_in_slot(root: &GitRoot, path: &Path, shown: &str, body: &str, base: &str) -> Result<String> {
    git_ok(&root.git_dir, &root.workspace, &["worktree", "add", "--detach", shown, base])?;
    let file = path.join(HARNESS_FILE);
    fs::write(&file, body).map_err(|error| error.to_string())?;
    let work_git = linked_git_dir(path)?;
    git_ok(&work_git, path, &["add", "-f", "--", HARNESS_FILE])?;
    git_ok(
        &work_git,
        path,
        &["commit", "-m", "rllm candidate", "--", HARNESS_FILE],
    )?;
    let head = git_ok(&work_git, path, &["rev-parse", "HEAD"])?;
    let digest = head.stdout.trim().to_string();
    if !commit_hex(&digest) {
        return Err("candidate commit digest is not hexadecimal".into());
    }
    Ok(digest)
}

/// Move `rllm-evolve` to `commit` only when that is a fast-forward and the caller is not on that branch.
pub(crate) fn fast_forward_evolve(root: &GitRoot, commit: &str) -> Result<()> {
    if !commit_hex(commit) {
        return Err("evolve target is not a commit".into());
    }
    let symbolic = git_run(&root.git_dir, &root.workspace, &["symbolic-ref", "-q", "HEAD"])?;
    if symbolic.code == Some(0) && symbolic.stdout.trim() == format!("refs/heads/{EVOLVE_BRANCH}") {
        return Err("refusing to move the caller's checked-out branch".into());
    }
    let current = git_run(
        &root.git_dir,
        &root.workspace,
        &["rev-parse", "-q", "--verify", &format!("refs/heads/{EVOLVE_BRANCH}")],
    )?;
    if current.code == Some(0) {
        let old = current.stdout.trim().to_string();
        if !commit_hex(&old) {
            return Err("evolve branch tip is not a commit".into());
        }
        let ancestor = git_run(&root.git_dir, &root.workspace, &["merge-base", "--is-ancestor", &old, commit])?;
        if ancestor.code != Some(0) {
            return Err("evolve branch cannot fast-forward to the candidate commit".into());
        }
        git_ok(
            &root.git_dir,
            &root.workspace,
            &["update-ref", &format!("refs/heads/{EVOLVE_BRANCH}"), commit, &old],
        )?;
    } else {
        git_ok(
            &root.git_dir,
            &root.workspace,
            &["update-ref", &format!("refs/heads/{EVOLVE_BRANCH}"), commit],
        )?;
    }
    Ok(())
}

fn harness_path(workspace: &Path) -> Result<PathBuf> {
    if path_has_slot(workspace) {
        return Err("candidate target is outside the workspace".into());
    }
    let path = workspace.join(HARNESS_FILE);
    if path.parent() != Some(workspace) {
        return Err("candidate target is outside the workspace".into());
    }
    Ok(path)
}

fn read_harness(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("candidate target is not a regular file".into());
    }
    if meta.len() > 32768 {
        return Err("candidate harness exceeds 32 KiB".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | CLOEXEC)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > 32768 {
        return Err("candidate harness exceeds 32 KiB".into());
    }
    Ok(bytes)
}

fn write_harness(path: &Path, body: &[u8]) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err("candidate target is not a regular file".into());
        }
        Ok(_) => {
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .custom_flags(NOFOLLOW | CLOEXEC)
                .open(path)
                .map_err(|error| error.to_string())?;
            file.write_all(body).map_err(|error| error.to_string())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .custom_flags(NOFOLLOW | CLOEXEC)
                .open(path)
                .map_err(|error| error.to_string())?;
            file.write_all(body).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}

fn harness_marker(path: &Path) -> Result<String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SOURCE_ABSENT.into()),
        Err(error) => Err(error.to_string()),
        Ok(_) => Ok(digest(&read_harness(path)?)),
    }
}

pub(crate) fn recorded_source(root: &GitRoot) -> Result<String> {
    if path_has_slot(&root.workspace) {
        return Err("workspace is not the root of a git work tree".into());
    }
    harness_marker(&harness_path(&root.workspace)?)
}

/// Digest of the caller harness file. `absent` means the file was not there.
pub(crate) fn source_digest(workspace: &Path) -> Result<String> {
    let meta = fs::symlink_metadata(workspace).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("workspace is not the root of a git work tree".into());
    }
    let workspace = fs::canonicalize(workspace).map_err(|error| error.to_string())?;
    if path_has_slot(&workspace) {
        return Err("workspace is not the root of a git work tree".into());
    }
    harness_marker(&harness_path(&workspace)?)
}

/// Write `body` onto the caller worktree after a fast-forward.
/// A digest mismatch leaves the file byte-identical.
pub(crate) fn apply_accepted(root: &GitRoot, body: &str, recorded: &str) -> Result<()> {
    if body.contains('\0') || body.len() > 32768 {
        return Err("candidate harness exceeds 32 KiB".into());
    }
    if path_has_slot(&root.workspace) {
        return Err("candidate target is outside the workspace".into());
    }
    let path = harness_path(&root.workspace)?;
    let current = harness_marker(&path)?;
    if current != recorded {
        return Err("target file changed since the candidate was made".into());
    }
    write_harness(&path, body.as_bytes())?;
    let landed = read_harness(&path)?;
    if landed != body.as_bytes() {
        return Err("candidate target write did not land".into());
    }
    Ok(())
}

#[cfg(test)]
mod checks {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const RUN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    static TEMP: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rllm-rrsi-test-{}-{}",
                std::process::id(),
                TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample(hypothesis: &str, verdict: &str, run_digest: &str) -> Json {
        edit(
            "prompt",
            hypothesis,
            &diff_digest("before", hypothesis),
            1,
            2,
            6,
            hypothesis.len() as u64,
            verdict,
            1,
            run_digest,
        )
        .unwrap()
    }

    #[test]
    fn append_read_back_links_previous_and_keeps_order() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let first = sample("假设\nnext", "accepted", RUN);
        let saved = append_file(root, "ledger.jsonl", &first, RUN).unwrap();
        assert_eq!(get(&saved, "evicted"), &Bool(false));
        assert_eq!(get(&saved, "lines"), &Num("1".into()));
        let second = sample("second change", "rejected", RUN);
        let again = append_file(root, "ledger.jsonl", &second, RUN).unwrap();
        assert_eq!(get(&again, "recorded"), &Bool(true));
        let document = fs::read_to_string(scratch.0.join("ledger.jsonl")).unwrap();
        assert_eq!(document, load_file(root, "ledger.jsonl").unwrap());
        let records = records_of(&document).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(text(&records[0], "hypothesis"), "假设\nnext");
        assert_eq!(text(&records[0], "previous"), "");
        assert_eq!(text(&records[1], "previous"), text(&records[0], "revision"));
        assert_eq!(text(&records[1], "verdict"), "rejected");
        assert_eq!(text(&records[0], "diff_digest"), diff_digest("before", "假设\nnext"));
        assert_eq!(diff_digest("before", "假设\nnext"), digest(b"before\n\xe5\x81\x87\xe8\xae\xbe\nnext"));
        let kept = document.lines().next().unwrap().to_string();
        assert!(document.lines().nth(1).unwrap().starts_with('{'));
        assert!(document.starts_with(&kept));
    }

    #[test]
    fn malformed_or_over_cap_record_is_refused() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let good = sample("kept", "accepted", RUN);
        append_file(root, "ledger.jsonl", &good, RUN).unwrap();
        let before = fs::read_to_string(scratch.0.join("ledger.jsonl")).unwrap();
        let mut bad = good.clone();
        put(&mut bad, "verdict", s("maybe"));
        let error = append_file(root, "ledger.jsonl", &bad, RUN).unwrap_err();
        assert!(error.contains("verdict"), "{error}");
        let mut huge = good.clone();
        put(&mut huge, "hypothesis", s(&"h".repeat(HYPOTHESIS_MAX + 1)));
        let error = append_file(root, "ledger.jsonl", &huge, RUN).unwrap_err();
        assert!(error.contains("record cap"), "{error}");
        assert!(edit("nope", "x", &diff_digest("", "x"), 0, 0, 0, 1, "accepted", 1, RUN).is_err());
        assert_eq!(fs::read_to_string(scratch.0.join("ledger.jsonl")).unwrap(), before);
    }

    #[test]
    fn stale_run_digest_is_refused() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let good = sample("owned by this run", "rejected", RUN);
        append_file(root, "ledger.jsonl", &good, RUN).unwrap();
        let before = fs::read_to_string(scratch.0.join("ledger.jsonl")).unwrap();
        let other = sample("different run", "rejected", OTHER);
        let error = append_file(root, "ledger.jsonl", &other, OTHER).unwrap_err();
        assert!(error.contains("run_digest"), "{error}");
        let mismatch = append_file(root, "ledger.jsonl", &good, OTHER).unwrap_err();
        assert!(mismatch.contains("run_digest"), "{mismatch}");
        assert_eq!(fs::read_to_string(scratch.0.join("ledger.jsonl")).unwrap(), before);
    }

    #[test]
    fn rejected_hypothesis_is_listed_and_shown_in_the_prompt() {
        let rejected = sample("do not repeat this hypothesis", "rejected", RUN);
        let accepted = sample("keep this other wording", "accepted", RUN);
        let long = sample(&format!("LONG-{}", "x".repeat(300)), "rejected", RUN);
        let mut document = String::new();
        for edit in [&rejected, &accepted, &long] {
            let saved = append_document(&document, edit, RUN).unwrap();
            assert_eq!(get(&saved, "evicted"), &Bool(false));
            document = text(&saved, "document");
        }
        let records = records_of(&document).unwrap();
        let falsified = falsified(&records, RUN);
        assert_eq!(falsified.len(), 2);
        assert!(falsified.iter().any(|record| text(record, "hypothesis") == "do not repeat this hypothesis"));
        assert!(falsified.iter().all(|record| text(record, "hypothesis") != "keep this other wording"));
        let section = prompt_document(&document, RUN).unwrap();
        assert!(section.contains("do not repeat this hypothesis"), "{section}");
        assert!(section.contains("LONG-"));
        assert!(section.contains("..."));
        assert!(!section.contains(&"x".repeat(300)));
        assert!(!section.contains("keep this other wording"), "{section}");
        assert!(section.len() <= PROMPT_MAX);
    }

    #[test]
    fn tried_component_is_counted_and_untried_component_is_listed() {
        let edit = edit(
            "prompt",
            "prompt was edited",
            &diff_digest("old", "prompt was edited"),
            0,
            1,
            3,
            18,
            "accepted",
            2,
            RUN,
        )
        .unwrap();
        let saved = append_document("", &edit, RUN).unwrap();
        let records = records_of(&text(&saved, "document")).unwrap();
        let tried = component_counts(&records, RUN);
        assert_eq!(tried, vec![("prompt".into(), 1)]);
        let untried = untried_components(&records, RUN);
        assert!(!untried.contains(&"prompt"));
        assert!(untried.contains(&"tools"));
        assert!(untried.contains(&"control flow"));
        assert!(untried.contains(&"context management"));
        assert_eq!(untried.len(), COMPONENTS.len() - 1);
        let section = prompt_document(&text(&saved, "document"), RUN).unwrap();
        assert!(section.contains("Tried: prompt (1)"), "{section}");
        assert!(section.contains("control flow"), "{section}");
        assert!(section.contains("subagents"), "{section}");
    }

    #[test]
    fn line_cap_refuses_without_dropping_history() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let path = scratch.0.join("ledger.jsonl");
        for index in 0..LINE_CAP {
            let edit = sample(&format!("entry {index}"), "accepted", RUN);
            append_file(root, "ledger.jsonl", &edit, RUN).unwrap();
        }
        let before = fs::read_to_string(&path).unwrap();
        assert_eq!(before.lines().count(), LINE_CAP);
        let extra = sample("one past the cap", "rejected", RUN);
        let error = append_file(root, "ledger.jsonl", &extra, RUN).unwrap_err();
        assert!(error.contains("no line was dropped"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        let document_error = append_document(&before, &extra, RUN).unwrap_err();
        assert!(document_error.contains("no line was dropped"), "{document_error}");
        assert_eq!(records_of(&before).unwrap().len(), LINE_CAP);
    }

    #[test]
    fn reordered_or_torn_ledger_is_refused() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let path = scratch.0.join("ledger.jsonl");
        append_file(root, "ledger.jsonl", &sample("first", "accepted", RUN), RUN).unwrap();
        append_file(root, "ledger.jsonl", &sample("second", "rejected", RUN), RUN).unwrap();
        let original = fs::read_to_string(&path).unwrap();
        let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
        lines.swap(0, 1);
        let swapped = format!("{}\n", lines.join("\n"));
        fs::write(&path, &swapped).unwrap();
        let error = append_file(root, "ledger.jsonl", &sample("third", "rejected", RUN), RUN).unwrap_err();
        assert!(error.contains("previous"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), swapped);
        assert!(records_of(&swapped).is_err());
        fs::write(&path, &original[..original.len() - 8]).unwrap();
        let torn = fs::read_to_string(&path).unwrap();
        assert!(append_file(root, "ledger.jsonl", &sample("third", "rejected", RUN), RUN).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), torn);
    }

    fn sealed_ledger() -> (String, String, String) {
        let rejected = sample("alpha hypothesis", "rejected", RUN);
        let accepted = sample("kept wording", "accepted", RUN);
        let mut ledger = String::new();
        for edit in [rejected, accepted] {
            ledger = text(&append_document(&ledger, &edit, RUN).unwrap(), "document");
        }
        let records = records_of(&ledger).unwrap();
        (ledger, text(&records[0], "revision"), text(&records[0], "diff_digest"))
    }

    #[test]
    fn library_entry_round_trips_with_the_sealed_digest() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let (ledger, rejected_revision, rejected_diff) = sealed_ledger();
        let task = "task t";
        let harness = "adopted harness";
        let saved = append_experience(root, task, harness, &ledger, RUN).unwrap();
        let revision = text(&saved, "revision");
        let later = append_experience(root, "task t+1", "next harness", "", OTHER).unwrap();
        let loaded = load_experience(root).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(text(&loaded[0], "revision"), revision);
        assert_eq!(text(&loaded[0], "harness"), harness);
        assert_eq!(text(&loaded[0], "harness_digest"), digest(harness.as_bytes()));
        assert_eq!(text(&loaded[1], "revision"), text(&later, "revision"));
        assert_eq!(text(&loaded[1], "harness_digest"), digest(b"next harness"));
        let falsified = match get(&loaded[0], "falsified") {
            Arr(items) => items.clone(),
            other => panic!("falsified list missing: {other:?}"),
        };
        assert_eq!(falsified.len(), 1);
        assert_eq!(text(&falsified[0], "hypothesis"), "alpha hypothesis");
        assert_eq!(text(&falsified[0], "revision"), rejected_revision);
        assert_eq!(text(&falsified[0], "diff_digest"), rejected_diff);
        let untried = match get(&loaded[0], "untried") {
            Arr(items) => items.iter().map(|item| item.text().unwrap().to_string()).collect::<Vec<_>>(),
            other => panic!("untried list missing: {other:?}"),
        };
        assert!(untried.iter().any(|name| name == "tools"));
        assert!(untried.iter().all(|name| name != "prompt"));
        let index = library_index(&loaded);
        assert_eq!(text(&index[0], "key"), library_key_of(task));
        assert_eq!(text(&index[0], "revision"), revision);
        assert_eq!(text(&index[1], "key"), digest(b"task t+1"));
        assert_eq!(text(&index[1], "revision"), text(&later, "revision"));
        assert_eq!(text(get(&saved, "index"), "revision"), revision);
        let again = load_experience(root).unwrap();
        assert_eq!(text(&again[0], "revision"), revision);
        assert_eq!(text(&again[0], "harness_digest"), digest(harness.as_bytes()));
        let section = historical_section(root, task).unwrap();
        assert!(section.contains(&revision), "{section}");
        assert!(section.contains(&rejected_revision), "{section}");
    }

    fn library_key_of(task: &str) -> String {
        digest(task.as_bytes())
    }

    #[test]
    fn tampered_library_line_is_refused() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        append_experience(root, "task t", "adopted harness", "", RUN).unwrap();
        let path = scratch.0.join("experience.jsonl");
        let original = fs::read_to_string(&path).unwrap();
        let mut tampered = original.clone();
        let at = tampered.find("adopted").unwrap();
        tampered.replace_range(at..at + 1, "x");
        fs::write(&path, &tampered).unwrap();
        let error = load_experience(root).unwrap_err();
        assert!(error.contains("digest"), "{error}");
        assert!(error.contains("unchanged"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), tampered);
        let refused = historical_section(root, "task t").unwrap_err();
        assert!(refused.contains("digest"), "{refused}");
        assert_eq!(fs::read_to_string(&path).unwrap(), tampered);
        let append_error = append_experience(root, "task t+1", "next harness", "", OTHER).unwrap_err();
        assert!(append_error.contains("unchanged"), "{append_error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), tampered);
        assert!(load_experience(root).is_err());
    }

    #[test]
    fn library_line_cap_keeps_existing_history() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let path = scratch.0.join("experience.jsonl");
        for index in 0..LINE_CAP {
            append_experience(root, &format!("task {index}"), &format!("harness {index}"), "", RUN).unwrap();
        }
        let before = fs::read_to_string(&path).unwrap();
        assert_eq!(before.lines().count(), LINE_CAP);
        let error = append_experience(root, "one past the cap", "extra harness", "", RUN).unwrap_err();
        assert!(error.contains("history was kept and no line was dropped"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(load_experience(root).unwrap().len(), LINE_CAP);
        let kept = historical_section(root, "task 0").unwrap();
        assert!(kept.contains("HISTORICAL EXPERIENCE"), "{kept}");
        assert!(kept.len() <= LIBRARY_PROMPT_MAX);
    }

    #[test]
    fn historical_section_is_bounded_and_labelled_as_experience() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let harness = format!("{}ENDHARNESS", "H".repeat(2000));
        for index in 0..4 {
            append_experience(root, "same task", &format!("{harness}-{index}"), "", RUN).unwrap();
        }
        let section = historical_section(root, "same task").unwrap();
        assert!(section.contains("HISTORICAL EXPERIENCE"), "{section}");
        assert!(section.contains("historical experience"), "{section}");
        assert!(section.contains("do not outrank the current task"), "{section}");
        assert!(section.contains("indexed key matches this task"), "{section}");
        assert!(!section.contains("do not propose these again"), "{section}");
        assert!(!section.contains("ENDHARNESS"), "{section}");
        assert!(!section.contains(&"H".repeat(1000)), "{section}");
        assert!(section.len() <= LIBRARY_PROMPT_MAX, "{}", section.len());
        let later = historical_section(root, "a different later task").unwrap();
        assert!(later.contains("HISTORICAL EXPERIENCE"), "{later}");
        assert!(later.contains("do not outrank the current task"), "{later}");
        assert!(later.contains("indexed key is from another task"), "{later}");
        assert!(later.len() <= LIBRARY_PROMPT_MAX);
        assert!(!later.contains("ENDHARNESS"), "{later}");
    }

    #[test]
    fn annealed_budget_shrinks_as_recorded_rounds_grow() {
        let mut seen = Vec::new();
        for rounds in 0..6 {
            let mut records = Vec::new();
            for round in 1..=rounds {
                let row = edit("prompt", &format!("round {round}"), &diff_digest("a", "b"), 0, 1, 1, 2, "accepted", round, RUN).unwrap();
                records.push(row);
            }
            let (budget, remaining) = budget_for(&records, RUN, rounds + 1);
            assert_eq!(remaining, budget, "{rounds}");
            seen.push(budget);
        }
        assert!(seen.windows(2).all(|pair| pair[0] >= pair[1]), "{seen:?}");
        assert!(seen[0] > seen[3], "{seen:?}");
        assert_eq!(seen[0], BUDGET_MAX);
        assert_eq!(*seen.last().unwrap(), BUDGET_MIN);
        let spent: Vec<Json> = (0..BUDGET_MAX).map(|index| {
            edit("prompt", &format!("spent {index}"), &diff_digest("a", "b"), 0, 1, 1, 2, "accepted", 1, RUN).unwrap()
        }).collect();
        let (budget, remaining) = budget_for(&spent, RUN, 1);
        assert_eq!(budget, BUDGET_MAX);
        assert_eq!(remaining, 0);
        let line = budget_line(&spent, RUN, 1);
        assert!(line.contains("Edits remaining this round: 0"), "{line}");
        assert!(line.contains(&format!("The annealed budget is {budget}")), "{line}");
    }

    #[test]
    fn policy_fields_round_trip_and_legacy_rows_still_seal() {
        let legacy = sample("legacy row", "accepted", RUN);
        let mut document = text(&append_document("", &legacy, RUN).unwrap(), "document");
        let mut annotated = edit("tools", "budget refusal", &diff_digest("a", "b"), 1, 1, 8, 9, "rejected", 2, RUN).unwrap();
        put(&mut annotated, "edit_budget", n(2));
        put(&mut annotated, "noise_floor", n(1));
        put(&mut annotated, "cost_known", Bool(false));
        put(&mut annotated, "decision", s("cost not recorded"));
        document = text(&append_document(&document, &annotated, RUN).unwrap(), "document");
        let records = records_of(&document).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(get(&records[0], "edit_budget"), &Null);
        assert_eq!(get(&records[1], "edit_budget"), &Num("2".into()));
        assert_eq!(get(&records[1], "noise_floor"), &Num("1".into()));
        assert_eq!(get(&records[1], "cost_known"), &Bool(false));
        assert_eq!(text(&records[1], "decision"), "cost not recorded");
        assert_eq!(text(&records[1], "verdict"), "rejected");
        assert!(incumbent_cost(&records, RUN).is_none());
        let measured = edit("prompt", "lengths only", &diff_digest("a", "b"), 1, 2, 4, 11, "accepted", 3, RUN).unwrap();
        document = text(&append_document(&document, &measured, RUN).unwrap(), "document");
        assert!(incumbent_cost(&records_of(&document).unwrap(), RUN).is_none());
        let mut known = edit("prompt", "kept", &diff_digest("a", "b"), 1, 2, 4, 11, "accepted", 4, RUN).unwrap();
        put(&mut known, "cost_known", Bool(true));
        document = text(&append_document(&document, &known, RUN).unwrap(), "document");
        assert_eq!(incumbent_cost(&records_of(&document).unwrap(), RUN), Some(11));
    }

    fn flat(component: &str, hypothesis: &str, round: u64, before: u64, after: u64) -> Json {
        edit(component, hypothesis, &diff_digest("a", hypothesis), before, after, 4, 4, "rejected", round, RUN).unwrap()
    }

    #[test]
    fn rejected_component_hypothesis_names_the_round() {
        let row = flat("skills", "cache the listing", 2, 1, 1);
        let saved = append_document("", &row, RUN).unwrap();
        let records = records_of(&text(&saved, "document")).unwrap();
        assert_eq!(prior_rejection(&records, RUN, "skills", "cache the listing"), Some(2));
        assert_eq!(prior_rejection(&records, RUN, "skills", "a different hypothesis"), None);
        assert_eq!(prior_rejection(&records, RUN, "plugins", "cache the listing"), None);
        assert_eq!(
            rejection_refusal("skills", 2),
            "refused because component skills with that hypothesis was already rejected in round 2"
        );
    }

    #[test]
    fn prune_window_drops_a_flat_component_and_keeps_a_gain() {
        let mut flat_doc = String::new();
        for round in 1..=PRUNE_WINDOW as u64 {
            let row = flat("skills", &format!("flat {round}"), round, 2, 2);
            flat_doc = text(&append_document(&flat_doc, &row, RUN).unwrap(), "document");
        }
        let flat_records = records_of(&flat_doc).unwrap();
        assert_eq!(pruned_components(&flat_records, RUN), vec!["skills".to_string()]);
        assert!(!live_components(&flat_records, RUN).contains(&"skills"));
        assert!(live_components(&flat_records, RUN).contains(&"plugins"));
        assert_eq!(
            prune_refusal("skills"),
            format!("refused because component skills was pruned after {PRUNE_WINDOW} rounds without a strictly positive gain")
        );
        let mut gained = String::new();
        for (round, after) in [(1, 2), (2, 3), (3, 2)] {
            let row = flat("skills", &format!("gain {round}"), round, 2, after);
            gained = text(&append_document(&gained, &row, RUN).unwrap(), "document");
        }
        let gained_records = records_of(&gained).unwrap();
        assert!(pruned_components(&gained_records, RUN).is_empty(), "{gained_records:?}");
        assert!(live_components(&gained_records, RUN).contains(&"skills"));
        let line = budget_line(&flat_records, RUN, 4);
        assert!(line.contains("Pruned: skills."), "{line}");
        assert!(line.starts_with("\nEDIT BUDGET\nEdits remaining this round:"));
        let section = prompt_document(&flat_doc, RUN).unwrap();
        assert!(section.contains("Pruned components (do not propose these): skills"), "{section}");
        let live = section.lines().find(|line| line.starts_with("Live components:")).unwrap();
        assert!(!live.contains("skills"), "{live}");
        assert!(live.contains("plugins"), "{live}");
    }

    #[test]
    fn legacy_row_keeps_today_prompt_and_budget_line() {
        let legacy = edit(
            "prompt",
            "legacy hypothesis",
            &diff_digest("before", "legacy hypothesis"),
            1,
            2,
            6,
            17,
            "rejected",
            1,
            RUN,
        )
        .unwrap();
        let saved = append_document("", &legacy, RUN).unwrap();
        let document = text(&saved, "document");
        let records = records_of(&document).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(get(&records[0], "leakage"), &Null);
        assert_eq!(get(&records[0], "accepted"), &Null);
        assert_eq!(get(&records[0], "cost_change"), &Null);
        assert_eq!(get(&records[0], "score_before"), &Num("1".into()));
        let section = prompt_document(&document, RUN).unwrap();
        assert_eq!(
            section,
            "\nEDIT HISTORY\nRejected hypotheses (do not propose these again):\n- round 1 · prompt · legacy hypothesis\nTried: prompt (1)\nNot yet tried: control flow, config, context management, tools, skills, memory, subagents"
        );
        assert_eq!(
            budget_line(&records, RUN, 1),
            "\nEDIT BUDGET\nEdits remaining this round: 3. The annealed budget is 4."
        );
        assert!(!section.contains("Pruned"));
        assert!(!section.contains("Live components"));
    }

    #[test]
    fn scored_history_fields_round_trip_without_task_text() {
        let task = "TASK_BODY_must_stay_out_of_the_row";
        let row = annotate(
            edit("system prompt", "shorten the tool list", &diff_digest("old", "shorten the tool list"), 1, 3, 10, 7, "accepted", 1, RUN).unwrap(),
            "clean",
        )
        .unwrap();
        let saved = append_document("", &row, RUN).unwrap();
        let records = records_of(&text(&saved, "document")).unwrap();
        let serialised = records[0].string();
        assert_eq!(text(&records[0], "component"), "system prompt");
        assert_eq!(text(&records[0], "hypothesis"), "shorten the tool list");
        assert_eq!(text(&records[0], "diff_digest").len(), 64);
        assert_eq!(get(&records[0], "score_before"), &Num("1".into()));
        assert_eq!(get(&records[0], "score_after"), &Num("3".into()));
        assert_eq!(get(&records[0], "cost_before"), &Num("10".into()));
        assert_eq!(get(&records[0], "cost_after"), &Num("7".into()));
        assert_eq!(get(&records[0], "cost_change"), &Num("-3".into()));
        assert_eq!(get(&records[0], "accepted"), &Bool(true));
        assert_eq!(text(&records[0], "leakage"), "clean");
        assert!(!serialised.contains(task), "{serialised}");
        let refused = unscored(
            "skills",
            &digest(task.as_bytes()),
            &diff_digest("old", "new"),
            4,
            4,
            2,
            RUN,
            "task_text",
            "refused because the candidate contains a recorded task",
        )
        .unwrap();
        let saved = append_document("", &refused, RUN).unwrap();
        let row = records_of(&text(&saved, "document")).unwrap().pop().unwrap();
        assert_eq!(text(&row, "leakage"), "task_text");
        assert_eq!(get(&row, "score_before"), &Null);
        assert_eq!(get(&row, "score_after"), &Null);
        assert_eq!(get(&row, "accepted"), &Bool(false));
        assert!(!row.string().contains(task), "{}", row.string());
    }

    fn git_scratch() -> (Scratch, PathBuf) {
        let scratch = Scratch::new();
        let repo = scratch.0.join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} {}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            );
        };
        git(&["init", "-b", "main"]);
        fs::write(repo.join("seed.txt"), "seed\n").unwrap();
        git(&["-c", "user.name=RLLM", "-c", "user.email=rllm@localhost", "add", "--", "seed.txt"]);
        git(&[
            "-c",
            "user.name=RLLM",
            "-c",
            "user.email=rllm@localhost",
            "commit",
            "-m",
            "seed",
        ]);
        (scratch, repo)
    }

    fn git_text(repo: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn worktree_max_is_enforced() {
        let (_scratch, repo) = git_scratch();
        let root = require_worktree_root(&repo).unwrap();
        let before = git_text(&repo, &["rev-parse", "HEAD"]);
        assert_eq!(before, root.head);
        let base = incumbent_base(&root).unwrap();
        for slot in 0..WORKTREE_MAX {
            let digest = commit_candidate(&root, slot, &format!("body {slot}"), &base).unwrap();
            assert!(commit_hex(&digest), "{digest}");
        }
        let again = commit_candidate(&root, 0, "reused body", &base).unwrap();
        assert!(commit_hex(&again));
        assert_eq!(rllm_worktree_count(&root).unwrap(), WORKTREE_MAX);
        let refused = commit_candidate(&root, WORKTREE_MAX, "one past the cap", &base).unwrap_err();
        assert!(refused.contains(&format!("{WORKTREE_MAX}")), "{refused}");
        assert!(refused.contains("worktree"), "{refused}");
        assert_eq!(rllm_worktree_count(&root).unwrap(), WORKTREE_MAX);
        assert_eq!(git_text(&repo, &["rev-parse", "HEAD"]), before);
        assert_eq!(git_text(&repo, &["status", "--porcelain"]), "");
    }

    fn git_at(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn stale_slot_is_reinitialised_not_reused() {
        let (_scratch, repo) = git_scratch();
        let root = require_worktree_root(&repo).unwrap();
        let stale = repo.join(".rllm-worktrees").join("c0");
        fs::create_dir_all(&stale).unwrap();
        git_at(&stale, &["init", "-b", "main"]);
        fs::write(stale.join("junk.txt"), "junk").unwrap();
        git_at(&stale, &["add", "--", "junk.txt"]);
        git_at(
            &stale,
            &["-c", "user.name=RLLM", "-c", "user.email=rllm@localhost", "commit", "-m", "stale"],
        );
        let refused = match require_worktree_root(&stale) {
            Err(error) => error,
            Ok(_) => panic!("stale slot was accepted as a workspace"),
        };
        assert!(refused.contains("not the root of a git work tree"), "{refused}");
        let base = incumbent_base(&root).unwrap();
        let digest = commit_candidate(&root, 0, "fresh body", &base).unwrap();
        assert!(commit_hex(&digest), "{digest}");
        assert!(!stale.join("junk.txt").exists(), "stale file was reused");
        assert_eq!(fs::read_to_string(stale.join("rllm-harness.txt")).unwrap(), "fresh body");
        let meta = fs::symlink_metadata(stale.join(".git")).unwrap();
        assert!(meta.is_file(), "stale git directory was reused");
        let again = match require_worktree_root(&stale) {
            Err(error) => error,
            Ok(_) => panic!("reinitialised slot was accepted as a workspace"),
        };
        assert!(again.contains("not the root of a git work tree"), "{again}");
    }

    #[test]
    fn aborted_commit_removes_the_slot_directory() {
        let (_scratch, repo) = git_scratch();
        let blocked = repo.join("rllm-harness.txt");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("blocked"), "x\n").unwrap();
        git_at(&repo, &["add", "--", "rllm-harness.txt"]);
        git_at(
            &repo,
            &["-c", "user.name=RLLM", "-c", "user.email=rllm@localhost", "commit", "-m", "block"],
        );
        let root = require_worktree_root(&repo).unwrap();
        let before = git_text(&repo, &["rev-parse", "HEAD"]);
        let base = incumbent_base(&root).unwrap();
        let refused = commit_candidate(&root, 0, "fresh body", &base).unwrap_err();
        assert!(refused.contains("directory"), "{refused}");
        assert!(!repo.join(".rllm-worktrees").join("c0").exists());
        assert_eq!(git_text(&repo, &["rev-parse", "HEAD"]), before);
        let listed = git_text(&repo, &["worktree", "list", "--porcelain"]);
        assert!(!listed.contains(".rllm-worktrees"), "{listed}");
    }

    fn relative_to_cwd(target: &Path) -> String {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let target = target.canonicalize().unwrap();
        let cwd_parts: Vec<_> = cwd.components().collect();
        let target_parts: Vec<_> = target.components().collect();
        let mut shared = 0;
        while shared < cwd_parts.len() && shared < target_parts.len() && cwd_parts[shared] == target_parts[shared] {
            shared += 1;
        }
        let mut relative = PathBuf::new();
        for _ in shared..cwd_parts.len() {
            relative.push("..");
        }
        for part in &target_parts[shared..] {
            relative.push(part.as_os_str());
        }
        let text = relative.to_str().unwrap().to_string();
        assert!(relative.is_relative(), "{text}");
        assert!(!text.starts_with('/'), "{text}");
        text
    }

    fn exclude_body(repo: &Path) -> String {
        let exclude = repo.join(".git").join("info").join("exclude");
        if exclude.exists() {
            fs::read_to_string(exclude).unwrap()
        } else {
            String::new()
        }
    }

    #[test]
    fn relative_worktree_spelling_resolves_to_the_canonical_root() {
        let (_scratch, repo) = git_scratch();
        let canonical = repo.canonicalize().unwrap();
        let identity = workspace_identity(&canonical).unwrap();
        let relative = PathBuf::from(relative_to_cwd(&canonical));
        assert_ne!(relative, canonical);
        let bound = require_recorded_worktree(&relative, &identity).unwrap();
        assert_eq!(bound.workspace, canonical);
        assert_eq!(workspace_identity(&bound.workspace).unwrap(), identity);
        let unbound = require_recorded_worktree(&relative, "").unwrap();
        assert_eq!(unbound.workspace, canonical);
    }

    #[test]
    fn lexical_worktree_spellings_resolve_to_the_same_root() {
        let (_scratch, repo) = git_scratch();
        let canonical = repo.canonicalize().unwrap();
        let identity = workspace_identity(&canonical).unwrap();
        let name = canonical.file_name().unwrap().to_str().unwrap();
        let spellings = [
            PathBuf::from(format!("{}/", canonical.display())),
            PathBuf::from(format!("{}/./.", canonical.display())),
            PathBuf::from(format!("{canonical}/../{name}", canonical = canonical.display())),
        ];
        for spelling in spellings {
            let spelling_text = spelling.to_str().unwrap();
            assert_ne!(spelling_text, canonical.to_str().unwrap(), "{spelling_text}");
            let bound = require_recorded_worktree(Path::new(spelling_text), &identity).unwrap();
            assert_eq!(bound.workspace, canonical, "{spelling_text}");
            assert_eq!(workspace_identity(&bound.workspace).unwrap(), identity);
        }
    }

    #[test]
    fn symlinked_parent_resolves_to_the_real_worktree() {
        let (scratch, repo) = git_scratch();
        let canonical = repo.canonicalize().unwrap();
        let identity = workspace_identity(&canonical).unwrap();
        let link = scratch.0.join("link-parent");
        std::os::unix::fs::symlink(repo.parent().unwrap(), &link).unwrap();
        let via = link.join(repo.file_name().unwrap());
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(!fs::symlink_metadata(&via).unwrap().file_type().is_symlink());
        assert_ne!(via.canonicalize().unwrap(), via);
        let bound = require_recorded_worktree(&via, &identity).unwrap();
        assert_eq!(bound.workspace, canonical);
        assert_eq!(workspace_identity(&via).unwrap(), identity);
        assert!(!bound.workspace.components().any(|component| component.as_os_str() == "link-parent"));
    }

    #[test]
    fn recorded_identity_refuses_a_different_worktree_without_writing_exclude() {
        let (_keep_a, repo_a) = git_scratch();
        let (_keep_b, repo_b) = git_scratch();
        let identity_a = workspace_identity(&repo_a.canonicalize().unwrap()).unwrap();
        let identity_b = workspace_identity(&repo_b.canonicalize().unwrap()).unwrap();
        assert_ne!(identity_a, identity_b);
        let refused = match require_recorded_worktree(&repo_b, &identity_a) {
            Err(error) => error,
            Ok(_) => panic!("a different work tree was accepted"),
        };
        assert_eq!(refused, "workspace identity does not match the approved root");
        assert!(!exclude_body(&repo_b).contains(".rllm-worktrees/"), "{}", exclude_body(&repo_b));
        assert!(!repo_b.join(".git").join("rllm-empty-hooks").exists());
        assert!(!repo_b.join("rllm-harness.txt").exists());
        let bound = require_recorded_worktree(&repo_b, &identity_b).unwrap();
        assert_eq!(bound.workspace, repo_b.canonicalize().unwrap());
    }

    #[test]
    fn identity_mismatch_seals_without_scores_and_a_longer_sentence_does_not() {
        let decision = "workspace identity does not match the approved root";
        let row = unscored("skills", "bind the approved root", &diff_digest("old", "new"), 4, 4, 1, RUN, "", decision).unwrap();
        assert_eq!(text(&row, "decision"), decision);
        assert_eq!(text(&row, "verdict"), "rejected");
        assert_eq!(get(&row, "score_before"), &Null);
        assert_eq!(get(&row, "score_after"), &Null);
        let widened = match unscored("skills", "bind the approved root", &diff_digest("old", "new"), 4, 4, 1, RUN, "", "workspace identity does not match the approved root extra") {
            Err(error) => error,
            Ok(_) => panic!("a longer identity sentence sealed without scores"),
        };
        assert_eq!(widened, "score_before must be a number");
    }

    #[test]
    fn a_file_is_not_a_worktree_root() {
        let scratch = Scratch::new();
        let file = scratch.0.join("not-a-root");
        fs::write(&file, b"kept-bytes").unwrap();
        let refused = match require_recorded_worktree(&file, "9:9") {
            Err(error) => error,
            Ok(_) => panic!("a file was accepted as a work tree"),
        };
        assert_eq!(refused, "workspace is not the root of a git work tree");
        assert_eq!(fs::read(&file).unwrap(), b"kept-bytes");
    }

    #[test]
    fn a_missing_path_is_not_a_worktree_root() {
        let scratch = Scratch::new();
        let missing = scratch.0.join("missing-root");
        assert!(!missing.exists());
        let refused = match require_recorded_worktree(&missing, "9:9") {
            Err(error) => error,
            Ok(_) => panic!("a missing path was accepted as a work tree"),
        };
        assert_eq!(refused, "workspace is not the root of a git work tree");
        assert!(!missing.exists());
    }

    #[test]
    fn resumed_run_prompt_and_budget_reflect_prior_rounds() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let mut ledger = String::new();
        for round in 1..=3 {
            let row = edit(
                "skills",
                &format!("do-not-repeat-{round}"),
                &diff_digest("a", &format!("h{round}")),
                2,
                2,
                4,
                4,
                "rejected",
                round,
                RUN,
            )
            .unwrap();
            ledger = text(&append_document(&ledger, &row, RUN).unwrap(), "document");
            append_file(root, "edits.jsonl", &row, RUN).unwrap();
        }
        let harness = "ADOPTED-HARNESS-MARKER";
        append_experience(root, "resume task", harness, &ledger, RUN).unwrap();
        let ledger_bytes = fs::read(scratch.0.join("edits.jsonl")).unwrap();
        let library_bytes = fs::read(scratch.0.join("experience.jsonl")).unwrap();
        let resumed = resume_run(root, "edits.jsonl", "resume task").unwrap();
        assert_eq!(fs::read(scratch.0.join("edits.jsonl")).unwrap(), ledger_bytes);
        assert_eq!(fs::read(scratch.0.join("experience.jsonl")).unwrap(), library_bytes);
        assert_eq!(number_field(&resumed, "round").unwrap(), 3);
        assert_eq!(number_field(&resumed, "edit_budget").unwrap(), annealed_budget(3));
        assert!(annealed_budget(3) < annealed_budget(0));
        let prompt = text(&resumed, "prompt");
        assert!(prompt.contains("do-not-repeat-1"), "{prompt}");
        assert!(prompt.contains("do-not-repeat-3"), "{prompt}");
        assert!(prompt.contains("Rejected hypotheses"), "{prompt}");
        assert!(prompt.contains("skills"), "{prompt}");
        assert!(prompt.contains("Pruned"), "{prompt}");
        assert!(prompt.contains(&format!("The annealed budget is {}", annealed_budget(3))), "{prompt}");
        assert!(prompt.contains("ADOPTED-HARNESS-MARKER"), "{prompt}");
        assert!(prompt.contains("Adopted harness (historical)"), "{prompt}");
        assert_eq!(text(&resumed, "adopted_harness"), harness);
        let pruned = match get(&resumed, "pruned") {
            Arr(items) => items.clone(),
            other => panic!("pruned list missing: {other:?}"),
        };
        assert!(pruned.iter().any(|item| item.text().unwrap() == "skills"));
        assert_eq!(number_field(&resumed, "rejected").unwrap(), 3);
    }

    #[test]
    fn resume_over_tampered_library_leaves_bytes_unchanged() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let row = edit("prompt", "kept hypothesis", &diff_digest("a", "b"), 1, 2, 4, 4, "accepted", 1, RUN).unwrap();
        append_file(root, "edits.jsonl", &row, RUN).unwrap();
        append_experience(root, "resume task", "adopted harness", "", RUN).unwrap();
        let path = scratch.0.join("experience.jsonl");
        let ledger_path = scratch.0.join("edits.jsonl");
        let original = fs::read(&path).unwrap();
        let mut tampered = original.clone();
        let at = tampered.windows(7).position(|window| window == b"adopted").unwrap();
        tampered[at] = b'x';
        assert_ne!(tampered, original);
        fs::write(&path, &tampered).unwrap();
        let ledger_before = fs::read(&ledger_path).unwrap();
        let refused = resume_run(root, "edits.jsonl", "resume task").unwrap_err();
        assert!(refused.contains("existing history was left unchanged"), "{refused}");
        assert!(refused.contains("digest"), "{refused}");
        assert_eq!(fs::read(&path).unwrap(), tampered);
        assert_eq!(fs::read(&ledger_path).unwrap(), ledger_before);
    }

    #[test]
    fn digest_named_ledger_resumes_rows_without_rewriting() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let name = format!("{RUN}.jsonl");
        let hypothesis = "do-not-repeat";
        let mut ledger = String::new();
        let row = edit("prompt", hypothesis, &diff_digest("before", hypothesis), 1, 1, 4, 4, "rejected", 1, RUN).unwrap();
        ledger = text(&append_document(&ledger, &row, RUN).unwrap(), "document");
        append_file(root, &name, &row, RUN).unwrap();
        append_experience(root, "resume task", "ADOPTED-HARNESS-MARKER", &ledger, RUN).unwrap();
        let ledger_bytes = fs::read(scratch.0.join(&name)).unwrap();
        let library_bytes = fs::read(scratch.0.join("experience.jsonl")).unwrap();
        let resumed = resume_run(root, &name, "resume task").unwrap();
        // Fails if the digest file name is rejected, the row is dropped, or either file is rewritten.
        assert_eq!(fs::read(scratch.0.join(&name)).unwrap(), ledger_bytes);
        assert_eq!(fs::read(scratch.0.join("experience.jsonl")).unwrap(), library_bytes);
        assert_eq!(text(&resumed, "edit_ledger").as_bytes(), ledger_bytes.as_slice());
        assert_eq!(text(&resumed, "run_digest"), RUN);
        let rows = records_of(&text(&resumed, "edit_ledger")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(text(&rows[0], "verdict"), "rejected");
        assert_eq!(text(&rows[0], "hypothesis"), hypothesis);
        let prompt = text(&resumed, "prompt");
        assert!(prompt.contains("EDIT HISTORY"), "{prompt}");
        assert!(prompt.contains("Rejected hypotheses"), "{prompt}");
        assert!(prompt.contains(hypothesis), "{prompt}");
        assert!(prompt.contains("ADOPTED-HARNESS-MARKER"), "{prompt}");
    }

    #[test]
    fn tampered_ledger_digest_is_refused_and_bytes_stay() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let row = edit("prompt", "kept-hypothesis", &diff_digest("a", "b"), 1, 1, 4, 4, "rejected", 1, RUN).unwrap();
        append_file(root, "edits.jsonl", &row, RUN).unwrap();
        append_experience(root, "resume task", "adopted harness", "", RUN).unwrap();
        let path = scratch.0.join("edits.jsonl");
        let original = fs::read(&path).unwrap();
        let needle = b"kept-hypothesis";
        let at = original.windows(needle.len()).position(|window| window == needle).unwrap();
        let mut tampered = original.clone();
        tampered[at] = b'x';
        fs::write(&path, &tampered).unwrap();
        let library_before = fs::read(scratch.0.join("experience.jsonl")).unwrap();
        let refused = resume_run(root, "edits.jsonl", "resume task").unwrap_err();
        // Fails if a broken digest is returned as an empty ledger, or if either file is rewritten.
        assert!(refused.contains("ledger record digest does not match its body"), "{refused}");
        assert!(refused.contains("existing history was left unchanged"), "{refused}");
        assert_eq!(fs::read(&path).unwrap(), tampered);
        assert_eq!(fs::read(scratch.0.join("experience.jsonl")).unwrap(), library_before);
    }

    #[test]
    fn unreadable_ledger_is_refused_not_empty() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let path = scratch.0.join("edits.jsonl");
        fs::write(&path, [0xff, 0xfe]).unwrap();
        let before = fs::read(&path).unwrap();
        let refused = resume_run(root, "edits.jsonl", "resume task").unwrap_err();
        // Fails if bad bytes are treated as a successful empty resume, or if the file is rewritten.
        assert!(refused.contains("ledger is not UTF-8"), "{refused}");
        assert!(refused.contains("existing history was left unchanged"), "{refused}");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(resume_run(root, "edits.jsonl", "resume task").is_err());
    }

    #[test]
    fn empty_or_absent_ledger_is_not_a_successful_resume() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let missing = scratch.0.join("missing.jsonl");
        let absent = resume_run(root, "missing.jsonl", "resume task").unwrap_err();
        // Fails if the reader is weakened to Ok(empty) for a file that is not there.
        assert!(absent.contains("resume needs the sealed ledger"), "{absent}");
        assert!(!missing.exists());
        let empty_path = scratch.0.join("empty.jsonl");
        fs::write(&empty_path, b"").unwrap();
        let empty = resume_run(root, "empty.jsonl", "resume task").unwrap_err();
        // Fails if a zero-length file is returned as an empty successful ledger.
        assert!(empty.contains("resume needs the sealed ledger"), "{empty}");
        assert_eq!(fs::read(&empty_path).unwrap(), b"");
    }
}

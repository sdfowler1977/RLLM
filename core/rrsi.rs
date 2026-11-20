//! Append-only edit ledger for one refinement run, and the durable improvement
//! library that later tasks read from the same store.
//! The ledger records the verdict the refine loop already reached. It does not choose that verdict.
//! Cost is the UTF-8 length of the instructions being replaced and of the proposal.
//! A full edit-ledger segment rolls into a new sealed segment whose first record
//! links to the last digest of the previous one. No line is dropped, none is
//! rewritten, and the digest chain stays verifiable across the seam.
//! The library file `experience.jsonl` is sealed with the same content digest and previous link.
//! Each line is the index entry for the adopted harness and the lessons the ledger supports.
use super::*;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use crate::host_fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const NOFOLLOW: i32 = 131072;
const CLOEXEC: i32 = 524288;
const PATH_MAX: usize = 1024;
const NAME_MAX: usize = 128;
const FILE_MAX: u64 = 4 * 1024 * 1024;
/// Sealed records in one edit-ledger segment. A segment that is full rolls
/// into a new sealed segment instead of refusing the next record, so the
/// ledger keeps its history and the run keeps its memory.
pub(crate) const LINE_CAP: usize = 64;
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
    "guardrails",
    "tool guidance",
    "scripts",
    "plugins",
];
const EDITABLE: &[&str] = &[
    "system prompt", "guardrails", "memory", "tool guidance",
    "subagents", "skills", "scripts", "plugins",
];
const LEAKAGE: &[&str] = &["clean", "task_text", "answer_text", "inert"];
const ROW_EXTRA: &[&str] = &["cost_change", "accepted", "leakage"];
/// Rounds of a component with no strictly positive score gain before it is pruned.
pub(crate) const PRUNE_WINDOW: usize = 3;
/// Scored rounds whose best gain must stay inside δ before a slot is held
/// for a component that has not been tried.
/// Three matches the evidence `PRUNE_WINDOW` already requires before a
/// component is pruned. Here the gain is the best score change of the round
/// across every component. A run with fewer scored rounds than this has not
/// shown a stall.
pub(crate) const STALL_WINDOW: usize = 3;
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
// Optional fields for a candidate pool, its worktree commit, the split scores,
// and the cost bound recorded with the decision. An older row omits them and still seals.
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
    "beta0",
    "beta1",
    "accept_reason",
    "reward",
    "budget_penalty",
];
/// A control row is sealed with the ledger line and is not a trial.
/// `stop` is the only marker. The record schema allows it on every row and requires nothing when it is absent.
const CONTROL_KEYS: &[&str] = &["control"];
// Three candidates are a pool, and three refusals still leave one edit of the
// annealed cap of four. The slots are judged in order. There is no thread schedule
// in the ledger. One worktree per slot: a fourth live worktree is refused.
pub(crate) const CANDIDATE_MAX: usize = 3;
pub(crate) const WORKTREE_MAX: usize = 3;
pub(crate) const EVOLVE_BRANCH: &str = "rllm-evolve";
const HARNESS_FILE: &str = "rllm-harness.txt";
/// One file in the run store. The refine loop reads it and never deletes it.
pub(crate) const STOP_FILE: &str = "STOP";
/// Notice text and the durable row when a regular STOP file is present.
pub(crate) const STOP_SENTENCE: &str =
    "refused because the STOP file is present; the incumbent was kept";
/// A symlink named STOP is refused with this sentence and left in place.
pub(crate) const STOP_SYMLINK: &str = "STOP file must be a regular non-symlink file";
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
    let key = component_key(name);
    COMPONENTS.iter().position(|item| *item == key)
}
pub(crate) fn known_component(name: &str) -> bool {
    component_index(name).is_some()
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
    // A STOP control row may record round 0. That means the caller closed no
    // round. Writing 1 there would name a round that was never opened.
    // Trial rows need a positive representable round, with no policy horizon.
    let control_stop = if get(value, "control") == &Null {
        false
    } else if text(value, "control") == "stop" {
        true
    } else {
        return Err("control must be stop".into());
    };
    if !control_stop && round == 0 {
        return Err("round must be positive".into());
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
    // The STOP sentence is accepted only on the control row that names hypothesis STOP.
    // Any other row that copies the sentence is an ordinary record and must carry scores.
    // `control_stop` was parsed with the round check above.
    if control_stop && !score_missing(value) {
        return Err("a control row must not record a score".into());
    }
    let stop_row = control_stop && hypothesis == "STOP" && decision_text == STOP_SENTENCE;
    if control_stop && !stop_row {
        return Err("a control row must pair the STOP hypothesis with the STOP decision".into());
    }
    let unscored = refused
        || (score_missing(value)
            && (decision_text.starts_with("refused because component ")
                || decision_text.starts_with("workspace is not")
                // Distinct from the work-tree sentence, and still written before any score exists.
                || decision_text == "workspace identity does not match the approved root"
                || stop_row));
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
    keys.extend_from_slice(CONTROL_KEYS);
    if sealed {
        keys.extend_from_slice(&["schema", "previous", "revision"]);
    }
    keys
}
fn validate_policy(value: &Json) -> Result<()> {
    if get(value, "reward") != &Null {
        let reward = signed_field(value, "reward")?;
        if (text(value, "verdict") == "accepted") != (reward > 0) {
            return Err("reward sign does not match the verdict".into());
        }
    }
    if get(value, "budget_penalty") != &Null && signed_field(value, "budget_penalty")? > 0 {
        return Err("budget penalty must not be positive".into());
    }
    if get(value, "edit_budget") != &Null {
        let budget = number_field(value, "edit_budget")?;
        if budget > 1024 {
            return Err("edit budget is outside the record cap".into());
        }
    }
    if get(value, "noise_floor") != &Null {
        number_field(value, "noise_floor")?;
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
    for key in ["beta0", "beta1"] {
        if get(value, key) != &Null {
            let number = number_field(value, key)?;
            if number > 1_000_000 {
                return Err(format!("{key} is outside the record cap"));
            }
        }
    }
    bounded_row_text(value, "accept_reason", 80)?;
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
/// Parse the sealed chain in order. The line count is not a gate: a full
/// segment rolls into a new sealed segment, so the chain is read whole and the
/// digest link is checked across the seam. The per-record cap, the blank-line
/// check and the link check stay.
fn parse_chain(lines: &[String]) -> Result<Vec<Json>> {
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
/// Seal one record onto the end of the chain. `chain` holds every sealed
/// record so far, in order, across every segment. The new record links to the
/// last digest of the chain, so a segment boundary keeps the chain verifiable.
fn seal(chain: &[Json], edit: &Json, expected_run: &str) -> Result<String> {
    validate_edit(edit, expected_run)?;
    if let Some(existing) = chain.first() {
        if text(existing, "run_digest") != expected_run {
            return Err("run_digest does not match the run being recorded".into());
        }
    }
    let previous = match chain.last() {
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
/// One sealed segment of the edit ledger. The base file is the first segment
/// and every later segment is `<base>.<n>`, so the chain is read in order and
/// the active segment is the last one.
fn segment_path(root: &Path, name: &str, index: usize) -> Result<PathBuf> {
    let file = if index == 0 { name.to_string() } else { format!("{name}.{index}") };
    let file = file_name(&file)?;
    let path = root.join(&file);
    if path.parent() != Some(root) {
        return Err("ledger path must stay inside its root".into());
    }
    Ok(path)
}
/// Every sealed segment of one ledger, in read order. The base file comes
/// first and the numbered segments follow it. A gap in the numbering is a
/// missing segment, and reading past one would drop history, so it is refused.
fn segment_paths(root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    let prefix = format!("{name}.");
    let mut indices: Vec<usize> = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if let Some(rest) = file.strip_prefix(prefix.as_str()) {
            if let Ok(index) = rest.parse::<usize>() {
                // `01` is not the first segment, and a second spelling of one
                // index is not a second segment.
                if index > 0 && rest == index.to_string() {
                    indices.push(index);
                }
            }
        }
    }
    indices.sort_unstable();
    indices.dedup();
    let mut chain = Vec::new();
    let base = segment_path(root, name, 0)?;
    if fs::symlink_metadata(&base).is_ok() {
        chain.push(base);
    }
    for (position, index) in indices.iter().enumerate() {
        if *index != position + 1 {
            return Err(
                "edit ledger segment chain is not contiguous; history was kept and no line was dropped".into(),
            );
        }
        chain.push(segment_path(root, name, *index)?);
    }
    Ok(chain)
}
/// The sealed segments of one ledger and their contents, in read order.
fn chain_documents(root: &Path, name: &str) -> Result<(Vec<PathBuf>, Vec<String>)> {
    let chain = segment_paths(root, name)?;
    let mut documents = Vec::with_capacity(chain.len());
    for path in &chain {
        documents.push(read_bounded(path)?);
    }
    Ok((chain, documents))
}
/// The whole chain as one document, in segment order. Every segment is checked
/// on its own so a torn segment is refused, and the chain is parsed as one
/// linked document so the digest chain is verified across the seam.
fn chain_document(documents: &[String]) -> Result<String> {
    let mut chain = String::new();
    for document in documents {
        if !document.is_empty() && !document.ends_with('\n') {
            return Err("ledger document is torn; history was not rewritten".into());
        }
        chain.push_str(document);
    }
    Ok(chain)
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
    let gain = i64::try_from(score_after).unwrap_or(i64::MAX)
        .saturating_sub(i64::try_from(score_before).unwrap_or(i64::MAX));
    let reward = if verdict == "accepted" && gain > 0 { gain } else { gain.min(0).saturating_sub(1) };
    let mut value = obj(vec![
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
    // Older callers can reconstruct historical accepted ties without inventing
    // positive reward. Live policy always attaches its measured signed result.
    if verdict == "rejected" || gain > 0 { put(&mut value, "reward", Num(reward.to_string())); }
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
fn refusal_row(
    component: &str,
    hypothesis: &str,
    diff_digest: &str,
    cost_before: u64,
    cost_after: u64,
    round: u64,
    run_digest: &str,
    leakage: &str,
    decision: &str,
) -> Json {
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
        ("reward", Num("-1".into())),
    ];
    if !leakage.is_empty() {
        fields.push(("leakage", s(leakage)));
    }
    obj(fields)
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
    let value = refusal_row(
        component,
        hypothesis,
        diff_digest,
        cost_before,
        cost_after,
        round,
        run_digest,
        leakage,
        decision,
    );
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
/// Read the whole sealed chain of one ledger, in segment order. The active
/// segment is the last one, so the reader sees every record the run wrote.
pub(crate) fn load_file(root: &str, name: &str) -> Result<String> {
    if root.is_empty() && name.is_empty() {
        return Ok(String::new());
    }
    if root.is_empty() || name.is_empty() {
        return Err("ledger path is incomplete".into());
    }
    let root_path = root_dir(root)?;
    let name = file_name(name)?;
    let (_, documents) = chain_documents(&root_path, &name)?;
    chain_document(&documents)
}
/// Append one record to the sealed chain. A full segment rolls into a new
/// sealed segment whose first record links to the last digest of the previous
/// one. Nothing is dropped, nothing is rewritten, and the chain stays
/// verifiable across the seam.
pub(crate) fn append_file(root: &str, name: &str, edit: &Json, run_digest: &str) -> Result<Json> {
    let root_path = root_dir(root)?;
    let name = file_name(name)?;
    let (chain, documents) = chain_documents(&root_path, &name)?;
    let before = chain_document(&documents)?;
    let existing = load_text(&before)?;
    let line = seal(&existing, edit, run_digest)?;
    let full = documents
        .last()
        .map(|document| document.lines().count() >= LINE_CAP)
        .unwrap_or(false);
    let target = if full {
        segment_path(&root_path, &name, chain.len())?
    } else {
        match chain.last() {
            Some(path) => path.clone(),
            None => segment_path(&root_path, &name, 0)?,
        }
    };
    if before.len().saturating_add(line.len()).saturating_add(1) as u64 > FILE_MAX {
        return Err("ledger exceeds its size cap; history was kept and no line was dropped".into());
    }
    append_line(&target, &line)?;
    let (_, after_documents) = chain_documents(&root_path, &name)?;
    let after = chain_document(&after_documents)?;
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
        .filter(|record| {
            text(record, "run_digest") == run_digest
                && text(record, "verdict") == "rejected"
                // A STOP control row is rejected, and it was never a trial.
                && text(record, "control") != "stop"
        })
        .cloned()
        .collect()
}
pub(crate) fn component_counts(records: &[Json], run_digest: &str) -> Vec<(String, usize)> {
    let mut counts = [0usize; COMPONENTS.len()];
    for record in records {
        if text(record, "run_digest") != run_digest {
            continue;
        }
        // A STOP control row names a component but it was never a trial.
        if text(record, "control") == "stop" {
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
        // A STOP control row names a round but it was never a trial.
        if text(record, "control") == "stop" {
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
    let mut line = format!("\nEDIT BUDGET\nEdits remaining this round: {remaining}. The annealed budget is {budget}. This is an effort score, not an execution limit; continue toward the goal.");
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
    prior_shape_rejection(records, run_digest, component, hypothesis, None)
}

pub(crate) fn normalized_key(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub(crate) fn component_key(value: &str) -> String {
    let key = normalized_key(value);
    if key == "system prompt" { "prompt".into() } else { key }
}

pub(crate) fn prior_shape_rejection(records: &[Json], run_digest: &str, component: &str, hypothesis: &str, diff: Option<&str>) -> Option<u64> {
    let component = component_key(component);
    let key = normalized_key(hypothesis);
    // Older redacted rows stored an exact digest. New redacted rows normalize
    // before hashing, and every consumer uses this same comparison.
    let old_digest = super::application::workspace_digest(s(hypothesis).string().as_bytes());
    let key_digest = super::application::workspace_digest(s(&key).string().as_bytes());
    let mut found = None;
    for record in records {
        let recorded = text(record, "hypothesis");
        let same_hypothesis = normalized_key(&recorded) == key || recorded == old_digest || recorded == key_digest;
        let same_shape = diff.is_some_and(|diff| hex_digest(diff) && text(record, "diff_digest") == diff);
        if text(record, "run_digest") != run_digest
            || component_key(&text(record, "component")) != component
            || text(record, "verdict") != "rejected"
            || !(same_hypothesis || same_shape)
        {
            continue;
        }
        // A STOP control row reuses a component and hypothesis, and it was never a trial.
        if text(record, "control") == "stop" {
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
// Incumbent cost is the run-global incumbent length: `cost_after` on the latest
// accepted row for this run, whichever component that row names. `incumbent_text`
// is one body for the run (`first_refinement.instructions`, else the active
// agent's `instructions`). The component label is not the text being replaced,
// so a plugins accept is the length a later prompt or skills candidate is scored
// against. That length is stored even when the accepted row had no earlier pair
// (`cost_known` false). A missing `cost_after` is unknown and is not borrowed
// from an older accepted row.
pub(crate) fn incumbent_cost(records: &[Json], run_digest: &str) -> Option<u64> {
    let record = records.iter().rev().find(|record| {
        text(record, "run_digest") == run_digest && text(record, "verdict") == "accepted"
    })?;
    match get(record, "cost_after") {
        Num(value) => value.parse().ok(),
        _ => None,
    }
}
pub(crate) fn untried_components(records: &[Json], run_digest: &str) -> Vec<&'static str> {
    let tried = component_counts(records, run_digest);
    COMPONENTS
        .iter()
        .copied()
        .filter(|name| tried.iter().all(|(tried_name, _)| tried_name != name))
        .collect()
}
fn round_best_gains(records: &[Json], run_digest: &str) -> Vec<i64> {
    let mut rounds = Vec::new();
    for record in records {
        if text(record, "run_digest") != run_digest {
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
    rounds.into_iter().map(|(_, gain)| gain).collect()
}
fn gain_inside_band(gain: i64, delta: u64) -> bool {
    if gain <= 0 {
        return true;
    }
    match u64::try_from(gain) {
        Ok(gain) => gain <= delta,
        Err(_) => false,
    }
}
/// Zero spread still permits detecting an actual flat or regressing window.
/// A positive gain clears a zero-width band; missing scores never count.
pub(crate) fn progress_stalled(records: &[Json], run_digest: &str, delta: u64) -> bool {
    let gains = round_best_gains(records, run_digest);
    if gains.len() < STALL_WINDOW {
        return false;
    }
    gains[gains.len() - STALL_WINDOW..].iter().all(|gain| gain_inside_band(*gain, delta))
}
pub(crate) fn stall_refusal(component: &str) -> String {
    format!(
        "refused because component {component} was already tried while progress stayed inside the noise band for {STALL_WINDOW} rounds"
    )
}
pub(crate) fn reserved_slot_refusal(
    records: &[Json],
    run_digest: &str,
    component: &str,
    delta: u64,
) -> Option<String> {
    if !progress_stalled(records, run_digest, delta) || untried_components(records, run_digest).is_empty() {
        return None;
    }
    let tried = component_counts(records, run_digest);
    tried.iter().any(|(name, _)| name == component).then(|| stall_refusal(component))
}

/// A plateau raises the bar instead of only announcing itself. The required
/// gain rises by the noise floor, so the next round must beat the band it
/// stalled in. A measured noise floor of zero still rises the bar, one point
/// per stalled window, so an exhausted search area keeps demanding more. This
/// is a score: a stalled round is never refused.
pub(crate) fn acceptance_floor(records: &[Json], run_digest: &str, floor: u64) -> u64 {
    let gains = round_best_gains(records, run_digest);
    let stalled = gains.iter().rev().take_while(|gain| gain_inside_band(**gain, floor)).count();
    if stalled < STALL_WINDOW { return floor; }
    floor.saturating_add(floor.max((stalled / STALL_WINDOW) as u64))
}

pub(crate) fn budget_penalty(records: &[Json], run_digest: &str, round: u64) -> i64 {
    let (budget, _) = budget_for(records, run_digest, round);
    let spent = records.iter().filter(|row| text(row, "run_digest") == run_digest
        && text(row, "control") != "stop" && number_field(row, "round").ok() == Some(round)).count() as u64;
    -i64::try_from(spent.saturating_add(1).saturating_sub(budget)).unwrap_or(i64::MAX)
}
fn recorded_noise_floor(records: &[Json], run_digest: &str) -> u64 {
    records.iter().rev().find_map(|record| {
        if text(record, "run_digest") != run_digest {
            return None;
        }
        match get(record, "noise_floor") {
            Num(value) => value.parse().ok(),
            _ => None,
        }
    }).unwrap_or(0)
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
/// The change a recorded reason allows. The vocabulary is the one the ledger
/// writes into `accept_reason`; the fix says what a proposal may do about the
/// reason. It is advice in the prompt text, never a refusal: a reason the
/// ledger does not name keeps a generic fix, and no reason stops a proposal.
fn allowed_fix(reason: &str) -> &'static str {
    match reason {
        "no gain" => "change behaviour that a check measures",
        "regression" => "restore the behaviour the measured checks saw before",
        "floor" => "raise the measured gain above the noise floor",
        "cost rose" => "keep the cost flat or lower it while the score rises",
        "cost bound" => "keep the cost inside the bound the measured gain pays for",
        "duplicate shape" => "propose a shape the ledger has not recorded",
        "rejected revision" => "propose a revision the ledger has not rejected",
        _ => "propose a change the measured checks can see",
    }
}
/// The rejections that share one recorded reason, newest first. The reason is
/// the value the ledger stored and it is kept verbatim: grouping never
/// rewrites, folds or normalises it.
struct ReasonGroup<'a> {
    reason: String,
    members: Vec<&'a Json>,
}
/// Group the run's rejections by the reason the ledger recorded. A row that
/// recorded no reason is not grouped: it keeps the plain row it has always
/// had. The groups come back with the biggest reason first and, for equal
/// counts, the most recent first.
fn rejection_groups(rejected: &[Json]) -> Vec<ReasonGroup<'_>> {
    let mut groups: Vec<ReasonGroup<'_>> = Vec::new();
    for record in rejected.iter().rev() {
        let reason = text(record, "accept_reason");
        if reason.is_empty() {
            continue;
        }
        match groups.iter_mut().find(|group| group.reason == reason) {
            Some(group) => group.members.push(record),
            None => groups.push(ReasonGroup {
                reason,
                members: vec![record],
            }),
        }
    }
    groups.sort_by_key(|group| std::cmp::Reverse(group.members.len()));
    groups
}
/// One rejection, rendered the way the prompt has always rendered it.
fn rejection_row(record: &Json) -> Result<String> {
    let decision = if get(record, "accepted") == &Null {
        String::new()
    } else {
        format!(" · {}", text(record, "verdict"))
    };
    Ok(format!(
        "round {} · {} · {}{}",
        number_field(record, "round")?,
        text(record, "component"),
        clip(&text(record, "hypothesis"), PROMPT_HYPOTHESIS),
        decision
    ))
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
    let mut lines = Vec::new();
    if rejected.is_empty() {
        lines.push("Rejected hypotheses (do not propose these again): none".to_string());
    } else {
        lines.push("Rejected hypotheses (do not propose these again):".to_string());
        // The rejections are grouped by the reason the ledger recorded, so the
        // proposer reads the pattern ("no gain ×3: change behaviour that a
        // check measures") instead of a wall of rows that say the same thing.
        // The prompt budget is the one the prompt has always spent: every
        // reason shows its most recent rejection first and the budget that is
        // left goes round robin over the reasons, so one dominant reason
        // cannot crowd the other lessons out of the prompt.
        let groups = rejection_groups(&rejected);
        let plain: Vec<&Json> = rejected
            .iter()
            .rev()
            .filter(|record| text(record, "accept_reason").is_empty())
            .collect();
        let mut examples: Vec<Vec<&Json>> = groups.iter().map(|_| Vec::new()).collect();
        let mut plain_rows: Vec<&Json> = Vec::new();
        let mut budget = PROMPT_REJECTED;
        // Every reason shows one example before any reason shows a second, so
        // the reason the run hit most often cannot crowd the other lessons
        // out of the prompt.
        for index in 0..groups.len() {
            if budget == 0 {
                break;
            }
            if let Some(record) = groups[index].members.first() {
                examples[index].push(record);
                budget -= 1;
            }
        }
        // A row that recorded no reason keeps the plain row it always had. It
        // takes the budget that is left after every reason has shown one
        // example, so a row the ledger recorded no reason for is never pushed
        // out of the prompt by a grouped one.
        for record in plain.iter() {
            if budget == 0 {
                break;
            }
            plain_rows.push(record);
            budget -= 1;
        }
        plain_rows.reverse();
        // The budget that is left goes back to the reasons, one more example
        // each in turn, so the reason the run hit most often is the one the
        // proposer reads most often.
        let mut depth = 1usize;
        loop {
            let mut progressed = false;
            for index in 0..groups.len() {
                if budget == 0 {
                    break;
                }
                if let Some(record) = groups[index].members.get(depth) {
                    examples[index].push(record);
                    budget -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
            depth += 1;
        }
        let mut shown = 0usize;
        for (index, group) in groups.iter().enumerate() {
            let mut line = format!(
                "- {} ×{}: {}",
                group.reason,
                group.members.len(),
                allowed_fix(&group.reason)
            );
            for record in examples[index].iter().rev() {
                line.push_str(" | ");
                line.push_str(&rejection_row(record)?);
                shown += 1;
            }
            lines.push(line);
        }
        for record in plain_rows.iter() {
            lines.push(format!("- {}", rejection_row(record)?));
            shown += 1;
        }
        let hidden = rejected.len().saturating_sub(shown);
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
    let delta = recorded_noise_floor(&records, run_digest);
    // The stall notice is a bar change, not a refusal: the next round must
    // gain more, and the proposer is told the work is not finished.
    let mut notice = String::new();
    if progress_stalled(&records, run_digest, delta) {
        let reserved = untried_components(&records, run_digest).join(", ");
        notice = vec![
            format!("Progress stalled inside the noise band for {STALL_WINDOW} rounds."),
            format!(
                "Reserved slots for components not yet tried: {}",
                if reserved.is_empty() { "none".to_string() } else { reserved }
            ),
            format!(
                "The bar moved, so switch component or raise the bar; the work is not finished. \
                 The required gain for the next round rises by the noise floor ({delta})."
            ),
        ].join("\n");
    }
    Ok(compose_section(&lines.join("\n"), &notice))
}

/// Compose the EDIT HISTORY section under the prompt cap. The stall notice is
/// a bar change the proposer has to read, so the history is clipped to leave
/// room for it and the notice is appended last: a truncation of the history
/// can never drop the sentence that says the work is not finished.
fn compose_section(history: &str, notice: &str) -> String {
    let header = "\nEDIT HISTORY\n";
    let budget = PROMPT_MAX.saturating_sub(header.len() + notice.len() + 1);
    let mut history = history.to_string();
    if history.len() > budget {
        history.truncate(budget);
        while !history.is_char_boundary(history.len()) {
            history.pop();
        }
    }
    let mut section = String::from(header);
    section.push_str(&history);
    if !notice.is_empty() {
        if !section.ends_with('\n') {
            section.push('\n');
        }
        section.push_str(notice);
    }
    section
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
        if round == 0 {
            return Err("falsified round must be positive".into());
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
    // load_experience and resume_run both parse here. Exactly LINE_CAP sealed
    // lines load. One more line is this refusal, before any link or digest check.
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
/// Read `experience.jsonl`. `resume_run` and the propose prompt use this
/// function, then `library_error` when they show the refusal. There is no
/// second library loader and no second line cap. Exactly `LINE_CAP` sealed
/// lines load. One more line returns
/// "improvement library exceeds its line cap; history was kept and no line was dropped".
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
pub(crate) const LIBRARY_INDEX_MISMATCH: &str =
    "stored library index does not match the library on disk; history was kept and no line was dropped";
pub(crate) const LIBRARY_INDEX_NOT_A_LIST: &str =
    "stored library index is not a list; history was kept and no line was dropped";

fn index_row_agrees(stored: &Json, disk: &Json) -> bool {
    text(stored, "key") == text(disk, "key")
        && text(stored, "revision") == text(disk, "revision")
        && text(stored, "run_digest") == text(disk, "run_digest")
}

// Null is the first round, before resume stored an index, and is not a
// disagreement. An array disagrees when its length differs or a key,
// revision, or run_digest is not the loaded row at that position. Any other
// shape cannot be compared and is a disagreement, so it cannot pass as
// agreement. historical_section_checked reports that shape with
// LIBRARY_INDEX_NOT_A_LIST. An array disagreement stays LIBRARY_INDEX_MISMATCH.
fn index_disagrees(stored: &Json, records: &[Json]) -> bool {
    let Arr(rows) = stored else {
        return !matches!(stored, Null);
    };
    let fresh = library_index(records);
    rows.len() != fresh.len()
        || rows
            .iter()
            .zip(fresh.iter())
            .any(|(row, disk)| !index_row_agrees(row, disk))
}

#[cfg(test)]
pub(crate) fn historical_section(root: &str, task: &str) -> Result<String> {
    historical_section_checked(root, task, &Null)
}

/// The propose prompt calls this with the run's stored `library_index`.
/// `load_experience` is the only loader and the only line cap. `Null` means
/// no stored index and renders the library. An array whose key, revision, or
/// run_digest is not the loaded row at that position, or a library that
/// gained or lost a row, returns `LIBRARY_INDEX_MISMATCH`. A stored value
/// that is present and not an array returns `LIBRARY_INDEX_NOT_A_LIST`.
/// This reads the library and does not write it.
pub(crate) fn historical_section_checked(root: &str, task: &str, stored: &Json) -> Result<String> {
    let records = load_experience(root).map_err(library_error)?;
    match stored {
        Null => {}
        Arr(_) if index_disagrees(stored, &records) => return Err(LIBRARY_INDEX_MISMATCH.into()),
        Arr(_) => {}
        _ => return Err(LIBRARY_INDEX_NOT_A_LIST.into()),
    }
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
            let value = if *key == "reward" && text(&edit, "verdict") == "rejected" {
                let mut reward = -signed_field(extra, key)?.saturating_abs().max(1);
                if let (Ok(before), Ok(after)) = (number_field(&edit, "score_before"), number_field(&edit, "score_after")) {
                    if after < before {
                        let loss = i64::try_from(before - after).unwrap_or(i64::MAX);
                        reward = reward.min((-loss).saturating_sub(1));
                    }
                }
                Num(reward.to_string())
            } else { get(extra, key).clone() };
            put(&mut edit, key, value);
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

/// `true` when the run store holds a regular `STOP` file. Missing is `false`.
/// A symlink or other non-file is `STOP_SYMLINK` and is not removed. An absent
/// store is not created. The file is opened with `O_NOFOLLOW` and not read as a ledger.
pub(crate) fn stop_requested(root: &str) -> Result<bool> {
    if root.is_empty() {
        return Ok(false);
    }
    if root.len() > PATH_MAX || root.contains('\0') {
        return Err("ledger root is empty or over the path cap".into());
    }
    let root_path = Path::new(root);
    match fs::symlink_metadata(root_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err("ledger root must be a real directory".into());
        }
        Ok(_) => {}
    }
    let name = file_name(STOP_FILE)?;
    let path = root_path.join(&name);
    if path.parent() != Some(root_path) {
        return Err("ledger path must stay inside its root".into());
    }
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => return Err(STOP_SYMLINK.into()),
        Ok(meta) if meta.len() > FILE_MAX => return Err("STOP file exceeds its size cap".into()),
        Ok(_) => {}
    }
    match OpenOptions::new().read(true).custom_flags(NOFOLLOW | CLOEXEC).open(&path) {
        Ok(file) => {
            drop(file);
            Ok(true)
        }
        Err(error) if error.raw_os_error() == Some(40) => Err(STOP_SYMLINK.into()),
        Err(error) => Err(error.to_string()),
    }
}

/// Unscored control row that names `STOP_SENTENCE`. The closed round keeps its own row.
/// `round` is that closed round. `0` means the caller closed nothing; the row keeps 0
/// so it does not claim round 1. A trial row cannot use 0.
/// `control` is `stop`, so the component census does not treat this row as a trial.
pub(crate) fn stop_edit(run_digest: &str, round: u64, incumbent: &str) -> Result<Json> {
    let mut value = refusal_row(
        "prompt",
        "STOP",
        &diff_digest(incumbent, STOP_FILE),
        incumbent.len() as u64,
        incumbent.len() as u64,
        round,
        run_digest,
        "",
        STOP_SENTENCE,
    );
    put(&mut value, "control", s("stop"));
    validate_edit(&value, run_digest)?;
    Ok(value)
}

/// Load the sealed ledger file and the experience library into a new process.
/// This reads both files and does not rewrite them. The library is
/// `load_experience`: the same file, the same line cap, and the same refusals.
/// The budget is `budget_for` on the round after the highest recorded round.
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
    let next = round.checked_add(1).ok_or("refinement round index overflow")?;
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
        ("library_index", Arr(library_index(&library))),
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

#[cfg(test)]
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

    fn rejected_with_reason(hypothesis: &str, reason: &str) -> Json {
        let mut row = sample(hypothesis, "rejected", RUN);
        put(&mut row, "accept_reason", s(reason));
        row
    }

    #[test]
    fn rejections_are_grouped_by_reason_with_a_count_and_the_allowed_fix() {
        // Three rejections share one reason. The prompt carries that pattern
        // once, with the count and the change the reason allows, and it keeps
        // the examples the proposer must not repeat.
        let mut document = String::new();
        for hypothesis in ["first flat attempt", "second flat attempt", "third flat attempt"] {
            let row = rejected_with_reason(hypothesis, "no gain");
            document = text(&append_document(&document, &row, RUN).unwrap(), "document");
        }
        let costly = rejected_with_reason("a costlier attempt", "cost rose");
        document = text(&append_document(&document, &costly, RUN).unwrap(), "document");
        let section = prompt_document(&document, RUN).unwrap();
        // The reason is grouped: it is named once, and the count says how
        // many rejections carried it.
        assert_eq!(section.matches("no gain").count(), 1, "{section}");
        assert!(section.contains("no gain ×3"), "{section}");
        assert!(section.contains("change behaviour that a check measures"), "{section}");
        // The examples the proposer must not repeat are still in the prompt.
        for hypothesis in ["first flat attempt", "second flat attempt", "third flat attempt"] {
            assert!(section.contains(hypothesis), "{section}");
        }
        // The other reason is its own group, with its own count and its own fix.
        assert_eq!(section.matches("cost rose").count(), 1, "{section}");
        assert!(section.contains("cost rose ×1"), "{section}");
        assert!(
            section.contains("keep the cost flat or lower it while the score rises"),
            "{section}"
        );
        assert!(section.contains("a costlier attempt"), "{section}");
        // One line per reason: the wall of same-reason rows is gone.
        let grouped = section.lines().filter(|line| line.starts_with("- ")).count();
        assert_eq!(grouped, 2, "{section}");
        assert!(section.len() <= PROMPT_MAX, "{}", section.len());
    }

    #[test]
    fn a_single_rejection_still_renders_its_reason_and_its_example() {
        let row = rejected_with_reason("one flat attempt", "no gain");
        let document = text(&append_document("", &row, RUN).unwrap(), "document");
        let section = prompt_document(&document, RUN).unwrap();
        assert!(section.contains("no gain ×1"), "{section}");
        assert!(section.contains("change behaviour that a check measures"), "{section}");
        assert!(section.contains("one flat attempt"), "{section}");
    }

    #[test]
    fn a_row_with_no_recorded_reason_keeps_its_place_next_to_the_groups() {
        // A row the ledger recorded no reason for is still a rejection the
        // proposer must not repeat, so the grouped rows may not push it out of
        // the prompt.
        let mut document = String::new();
        for index in 0..5 {
            let row = rejected_with_reason(&format!("flat attempt {index}"), "no gain");
            document = text(&append_document(&document, &row, RUN).unwrap(), "document");
        }
        let legacy = sample("an old row with no recorded reason", "rejected", RUN);
        document = text(&append_document(&document, &legacy, RUN).unwrap(), "document");
        let section = prompt_document(&document, RUN).unwrap();
        assert!(section.contains("no gain ×5"), "{section}");
        assert!(section.contains("an old row with no recorded reason"), "{section}");
    }

    #[test]
    fn grouping_changes_the_prompt_text_only_and_writes_every_row() {
        // The grouping is a reading of the ledger, never a gate: every row is
        // written, no append is refused, and no segment is skipped.
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let mut document = String::new();
        for index in 0..6 {
            let reason = if index % 2 == 0 { "no gain" } else { "cost rose" };
            let row = rejected_with_reason(&format!("attempt {index}"), reason);
            let saved = append_file(root, "ledger.jsonl", &row, RUN).unwrap();
            assert_eq!(get(&saved, "recorded"), &Bool(true));
            assert_eq!(get(&saved, "evicted"), &Bool(false));
            document = text(&append_document(&document, &row, RUN).unwrap(), "document");
        }
        let on_disk = fs::read_to_string(scratch.0.join("ledger.jsonl")).unwrap();
        assert_eq!(on_disk.lines().count(), 6);
        assert_eq!(records_of(&on_disk).unwrap().len(), 6);
        assert_eq!(records_of(&document).unwrap().len(), 6);
        // The prompt reads the ledger and writes none of it.
        let before = document.clone();
        let section = prompt_document(&document, RUN).unwrap();
        assert_eq!(document, before, "the grouping changed no ledger byte");
        assert!(section.contains("no gain ×3"), "{section}");
        assert!(section.contains("cost rose ×3"), "{section}");
        assert!(
            section.contains("Earlier rejected entries retained in the ledger: 2"),
            "{section}"
        );
    }

    #[test]
    fn the_recorded_reason_is_quoted_verbatim_and_keeps_its_bound() {
        // The reason the ledger stored is the reason the proposer reads. The
        // grouping never folds it, trims it or rewrites its case.
        let row = rejected_with_reason("an odd reason", "Cost Bound");
        let document = text(&append_document("", &row, RUN).unwrap(), "document");
        let records = records_of(&document).unwrap();
        assert_eq!(text(&records[0], "accept_reason"), "Cost Bound");
        let section = prompt_document(&document, RUN).unwrap();
        assert!(section.contains("Cost Bound ×1"), "{section}");
        // A reason past the row bound is refused by the append path, and the
        // ledger keeps the rows it already holds.
        let long = rejected_with_reason("an over long reason", &"r".repeat(81));
        let error = append_document(&document, &long, RUN).unwrap_err();
        assert!(error.contains("accept_reason"), "{error}");
        assert_eq!(records_of(&document).unwrap().len(), 1);
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
    fn line_cap_rolls_into_a_new_segment_and_keeps_every_line() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let path = scratch.0.join("ledger.jsonl");
        for index in 0..LINE_CAP {
            let edit = sample(&format!("entry {index}"), "accepted", RUN);
            append_file(root, "ledger.jsonl", &edit, RUN).unwrap();
        }
        let before = fs::read_to_string(&path).unwrap();
        assert_eq!(before.lines().count(), LINE_CAP);
        // A full segment is not a refusal: the next record continues into a new
        // sealed segment, so no line is dropped and nothing is rewritten.
        let extra = sample("one past the cap", "rejected", RUN);
        let rolled = append_file(root, "ledger.jsonl", &extra, RUN).unwrap();
        assert_eq!(get(&rolled, "recorded"), &Bool(true));
        assert_eq!(get(&rolled, "evicted"), &Bool(false));
        assert_eq!(get(&rolled, "lines"), &Num((LINE_CAP + 1).to_string()));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        // The new segment holds the new record and links to the last digest of
        // the previous segment.
        let next = scratch.0.join("ledger.jsonl.1");
        assert!(next.exists(), "a full segment rolls into a new sealed segment");
        let rolled_text = fs::read_to_string(&next).unwrap();
        assert_eq!(rolled_text.lines().count(), 1);
        let head = parse(rolled_text.lines().next().unwrap()).unwrap();
        let tail = records_of(&before).unwrap().pop().unwrap();
        assert_eq!(text(&head, "previous"), text(&tail, "revision"));
        // The reader reads the whole chain in order, the active segment last.
        let chain = load_file(root, "ledger.jsonl").unwrap();
        assert_eq!(chain.lines().count(), LINE_CAP + 1);
        assert!(chain.starts_with(&before), "the chain keeps every earlier line");
        assert_eq!(records_of(&chain).unwrap().len(), LINE_CAP + 1);
        // The in-memory mirror carries the same lines, so the flat document and
        // the chain agree byte for byte.
        let flat = append_document(&before, &extra, RUN).unwrap();
        assert_eq!(text(&flat, "document"), chain);
    }

    #[test]
    fn every_seam_links_and_no_line_is_dropped_across_the_chain() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let total = LINE_CAP * 2 + 3;
        for index in 0..total {
            let edit = sample(&format!("entry {index}"), "accepted", RUN);
            append_file(root, "ledger.jsonl", &edit, RUN).unwrap();
        }
        // Three sealed segments: the base file and two numbered ones.
        for index in 0..3 {
            let name = if index == 0 {
                "ledger.jsonl".to_string()
            } else {
                format!("ledger.jsonl.{index}")
            };
            let segment = scratch.0.join(&name);
            assert!(segment.exists(), "{name} is a sealed segment");
            let count = fs::read_to_string(&segment).unwrap().lines().count();
            assert!(count <= LINE_CAP, "{name} holds a sealed segment");
        }
        // The reader reads the whole chain in order and drops no line.
        let chain = load_file(root, "ledger.jsonl").unwrap();
        assert_eq!(chain.lines().count(), total, "no line was dropped");
        let records = records_of(&chain).unwrap();
        assert_eq!(records.len(), total);
        for index in 1..records.len() {
            assert_eq!(
                text(&records[index], "previous"),
                text(&records[index - 1], "revision"),
                "record {index} links to the record before it across the seam"
            );
        }
        for index in 0..total {
            assert_eq!(text(&records[index], "hypothesis"), format!("entry {index}"));
        }
        // Evidence across the seam: the line count of every segment, the
        // digest of the last record of the first segment and the link the
        // first record of the second segment carries.
        let first = fs::read_to_string(scratch.0.join("ledger.jsonl")).unwrap();
        let second = fs::read_to_string(scratch.0.join("ledger.jsonl.1")).unwrap();
        let first_tail = records_of(&first).unwrap().pop().unwrap();
        let second_head = parse(second.lines().next().unwrap()).unwrap();
        println!(
            "SEAM segment1_lines={} segment2_lines={} chain_lines={} records={} \
             segment1_last_digest={} segment2_first_link={} links_equal={}",
            first.lines().count(),
            second.lines().count(),
            chain.lines().count(),
            records.len(),
            text(&first_tail, "revision"),
            text(&second_head, "previous"),
            text(&first_tail, "revision") == text(&second_head, "previous")
        );
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
    fn stored_index_refuses_a_replaced_row_and_keeps_the_file() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        append_experience(root, "task t", "adopted harness", "", RUN).unwrap();
        append_experience(root, "task u", "other harness", "", OTHER).unwrap();
        let loaded = load_experience(root).unwrap();
        let stored = Arr(library_index(&loaded));
        let section = historical_section_checked(root, "task t", &stored).unwrap();
        assert_eq!(section, historical_section(root, "task t").unwrap());
        assert!(section.contains(&text(&loaded[0], "revision")), "{section}");
        assert!(!section.contains(LIBRARY_INDEX_MISMATCH), "{section}");
        let path = scratch.0.join("experience.jsonl");
        let before = fs::read(&path).unwrap();
        let alt = scratch.0.join("alt");
        let alt_root = alt.to_str().unwrap();
        append_experience(alt_root, "task t", "adopted harness", "", RUN).unwrap();
        append_experience(alt_root, "task u", "replaced harness", "", OTHER).unwrap();
        let replaced = fs::read(alt.join("experience.jsonl")).unwrap();
        assert_ne!(replaced, before);
        fs::write(&path, &replaced).unwrap();
        let parked = fs::read(&path).unwrap();
        assert_eq!(
            LIBRARY_INDEX_MISMATCH,
            "stored library index does not match the library on disk; history was kept and no line was dropped"
        );
        match historical_section_checked(root, "task t", &stored) {
            Err(error) => assert_eq!(error, LIBRARY_INDEX_MISMATCH),
            Ok(section) => panic!(
                "replaced row was accepted: {}",
                section.chars().take(180).collect::<String>()
            ),
        }
        assert_eq!(fs::read(&path).unwrap(), parked);
        assert_eq!(text(&load_experience(root).unwrap()[1], "harness"), "replaced harness");
    }

    #[test]
    fn stored_index_that_is_not_a_list_is_refused_and_history_stays() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        append_experience(root, "task t", "adopted harness", "", RUN).unwrap();
        append_experience(root, "task u", "other harness", "", OTHER).unwrap();
        let loaded = load_experience(root).unwrap();
        let path = scratch.0.join("experience.jsonl");
        let before = fs::read(&path).unwrap();
        assert_eq!(
            LIBRARY_INDEX_NOT_A_LIST,
            "stored library index is not a list; history was kept and no line was dropped"
        );
        assert_ne!(LIBRARY_INDEX_NOT_A_LIST, LIBRARY_INDEX_MISMATCH);
        let history = historical_section(root, "task t").unwrap();
        let first_revision = text(&loaded[0], "revision");
        let second_revision = text(&loaded[1], "revision");
        assert!(history.contains(&first_revision), "{history}");
        assert!(history.contains("historical entry"), "{history}");
        assert!(!history.contains(LIBRARY_INDEX_NOT_A_LIST), "{history}");
        // Null is the historical_section delegate: no stored index, so the
        // same history renders. A present string, object, or number cannot
        // be compared. The route returns LIBRARY_INDEX_NOT_A_LIST on the
        // Err path that LIBRARY_INDEX_MISMATCH already uses.
        assert_eq!(
            historical_section_checked(root, "task t", &Null).unwrap(),
            history
        );
        let shapes = [
            ("string", s("not-a-list")),
            ("object", obj(vec![("key", s("x")), ("revision", s("1"))])),
            ("number", n(4)),
        ];
        for (name, stored) in shapes {
            match historical_section_checked(root, "task t", &stored) {
                Err(error) => assert_eq!(error, LIBRARY_INDEX_NOT_A_LIST, "{name}: {error}"),
                Ok(section) => panic!(
                    "expected the refusal sentence {LIBRARY_INDEX_NOT_A_LIST:?}, got the rendered history: {section}"
                ),
            }
            assert_eq!(fs::read(&path).unwrap(), before, "{name}");
            let kept = fs::read_to_string(&path).unwrap();
            assert_eq!(kept.lines().count(), loaded.len(), "{name}");
            assert!(kept.contains(&first_revision), "{name}: {kept}");
            assert!(kept.contains(&second_revision), "{name}: {kept}");
            assert_eq!(historical_section(root, "task t").unwrap(), history, "{name}");
        }
    }

    #[test]
    fn stored_index_refuses_an_appended_inserted_or_reordered_list() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        append_experience(root, "task t", "adopted harness", "", RUN).unwrap();
        append_experience(root, "task u", "other harness", "", OTHER).unwrap();
        let loaded = load_experience(root).unwrap();
        let rows = library_index(&loaded);
        assert!(rows.len() >= 2, "{}", rows.len());
        let path = scratch.0.join("experience.jsonl");
        let before = fs::read(&path).unwrap();
        let agreed = historical_section_checked(root, "task t", &Arr(rows.clone())).unwrap();
        assert!(agreed.contains(&text(&loaded[0], "revision")), "{agreed}");
        assert!(!agreed.contains(LIBRARY_INDEX_MISMATCH), "{agreed}");
        let mut appended = rows.clone();
        appended.push(rows[0].clone());
        let mut inserted = rows.clone();
        inserted.insert(0, rows[1].clone());
        let mut reordered = rows.clone();
        reordered.swap(0, 1);
        for (name, stored_rows) in [
            ("append", appended),
            ("insert-at-0", inserted),
            ("reorder", reordered),
        ] {
            match historical_section_checked(root, "task t", &Arr(stored_rows)) {
                Err(error) => assert_eq!(error, LIBRARY_INDEX_MISMATCH, "{name}"),
                Ok(section) => panic!("{name} was accepted: {section}"),
            }
        }
        assert_eq!(fs::read(&path).unwrap(), before);
        let kept = load_experience(root).unwrap();
        assert_eq!(text(&kept[0], "revision"), text(&loaded[0], "revision"));
        assert_eq!(text(&kept[1], "harness"), "other harness");
        assert_eq!(kept.len(), loaded.len());
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
    fn sr1_rejection_identity_normalizes_aliases_whitespace_and_shape() {
        let diff = diff_digest("incumbent", "candidate");
        let row = edit("prompt", "tighten the greeting", &diff, 2, 1, 40, 10, "rejected", 1, RUN).unwrap();
        let records = vec![row];
        for component in ["prompt", "system prompt", "Prompt", "  SYSTEM  \n PROMPT "] {
            for hypothesis in ["tighten the greeting", " TIGHTEN the greeting ", "tighten\n  the\t greeting"] {
                assert_eq!(prior_rejection(&records, RUN, component, hypothesis), Some(1));
            }
        }
        assert_eq!(prior_shape_rejection(&records, RUN, "Prompt", "new label", Some(&diff)), Some(1));
        assert_eq!(prior_shape_rejection(&records, RUN, "Prompt", "new label", Some(&diff_digest("a", "different"))), None);
        assert_eq!(prior_shape_rejection(&records, RUN, "skills", "new label", Some(&diff)), None);
        assert_eq!(prior_rejection(&records, &"0".repeat(64), "prompt", "tighten the greeting"), None);
        let mut accepted = records[0].clone();
        put(&mut accepted, "verdict", s("accepted"));
        assert_eq!(prior_rejection(&[accepted], RUN, "prompt", "tighten the greeting"), None);
        println!("SR1-P4 aliases/case/wrapped whitespace and same diff rejected; changed shape+claim, other component/run and accepted controls remain new");
    }

    #[test]
    fn sr1_plateau_changes_area_or_raises_floor_and_budget_only_scores() {
        let mut records = Vec::new();
        for (index, component) in COMPONENTS.iter().enumerate() {
            let row = edit(component, &format!("trial {index}"), &diff_digest("old", &format!("new {index}")),
                1, 1, 40, 10, "rejected", index as u64 + 1, RUN).unwrap();
            records.push(row);
        }
        assert!(untried_components(&records, RUN).is_empty());
        assert!(acceptance_floor(&records, RUN, 0) > 0);
        let subset = &records[..STALL_WINDOW];
        assert!(!untried_components(subset, RUN).is_empty());
        assert!(reserved_slot_refusal(subset, RUN, "prompt", 1).is_some());
        let untried = untried_components(subset, RUN)[0];
        assert!(reserved_slot_refusal(subset, RUN, untried, 1).is_none());
        let plugin_plateau = window("plugins", 1, 1);
        assert!(progress_stalled(&plugin_plateau, RUN, 0));
        assert!(reserved_slot_refusal(&plugin_plateau, RUN, "plugins", 0).is_some());
        let mut spent = Vec::new();
        for index in 0..5 {
            spent.push(edit("prompt", &format!("effort {index}"), &diff_digest("a", "b"), 1, 1, 40, 10, "rejected", 1, RUN).unwrap());
        }
        assert_eq!(budget_for(&spent, RUN, 1).1, 0);
        assert_eq!(budget_penalty(&spent, RUN, 1), -2);
        assert!(budget_line(&spent, RUN, 1).contains("not an execution limit"));
        let far_round = edit("skills", "later trial", &diff_digest("a", "later"), 1, 2, 10, 12, "accepted", 10000, RUN).unwrap();
        let sealed = append_document("", &far_round, RUN).unwrap();
        assert_eq!(get(&records_of(&text(&sealed, "document")).unwrap()[0], "round"), &n(10000));
        println!("SR1-P3 plateau: untried area available; exhausted areas raise floor; spent budget penalty=-2; round 10000 seals");
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
        // The rejected row is not the incumbent. The accepted legacy row's measured
        // cost_after is, even though that row never set cost_known.
        assert_eq!(incumbent_cost(&records, RUN), Some("legacy row".len() as u64));
        let measured = edit("prompt", "lengths only", &diff_digest("a", "b"), 1, 2, 4, 11, "accepted", 3, RUN).unwrap();
        document = text(&append_document(&document, &measured, RUN).unwrap(), "document");
        assert_eq!(incumbent_cost(&records_of(&document).unwrap(), RUN), Some(11));
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
            "\nEDIT HISTORY\nRejected hypotheses (do not propose these again):\n- round 1 · prompt · legacy hypothesis\nTried: prompt (1)\nNot yet tried: control flow, config, context management, tools, skills, memory, subagents, guardrails, tool guidance, scripts, plugins"
        );
        assert_eq!(
            budget_line(&records, RUN, 1),
            "\nEDIT BUDGET\nEdits remaining this round: 3. The annealed budget is 4. This is an effort score, not an execution limit; continue toward the goal."
        );
        assert!(!section.contains("Pruned"));
        assert!(!section.contains("Live components"));
    }

    fn window(component: &str, before: u64, after: u64) -> Vec<Json> {
        (1..=STALL_WINDOW as u64)
            .map(|round| flat(component, &format!("{component} {round}"), round, before, after))
            .collect()
    }

    #[test]
    fn stalled_run_refuses_a_tried_component_and_admits_it_when_the_run_is_not_stalled() {
        let stalled = window("prompt", 4, 5);
        let delta = 1u64;
        assert_eq!(
            reserved_slot_refusal(&stalled, RUN, "prompt", delta),
            Some(stall_refusal("prompt")),
            "a stalled ledger with an untried component refuses a tried component"
        );
        assert!(progress_stalled(&stalled, RUN, delta));
        assert!(untried_components(&stalled, RUN).contains(&"tools"));
        assert_eq!(
            stall_refusal("prompt"),
            format!("refused because component prompt was already tried while progress stayed inside the noise band for {STALL_WINDOW} rounds")
        );
        let moving = &stalled[..STALL_WINDOW - 1];
        assert!(!progress_stalled(moving, RUN, delta));
        assert_eq!(reserved_slot_refusal(moving, RUN, "prompt", delta), None);
        assert_eq!(reserved_slot_refusal(&stalled, RUN, "tools", delta), None);
        let mut other = stalled.clone();
        other.push(
            edit("prompt", "other run", &diff_digest("a", "other"), 0, 50, 1, 1, "accepted", 1, OTHER).unwrap(),
        );
        assert_eq!(
            reserved_slot_refusal(&other, RUN, "prompt", delta),
            Some(stall_refusal("prompt"))
        );
    }

    #[test]
    fn gain_equal_to_delta_is_inside_the_band_and_one_above_clears_the_stall() {
        let delta = 2u64;
        let inside = window("prompt", 5, 5 + delta);
        assert!(
            progress_stalled(&inside, RUN, delta),
            "gain equal to delta stays inside the band"
        );
        let mut cleared = inside.clone();
        cleared.pop();
        cleared.push(flat("prompt", "above", STALL_WINDOW as u64, 5, 5 + delta + 1));
        assert!(
            !progress_stalled(&cleared, RUN, delta),
            "a gain strictly above delta clears the stall"
        );
        assert_eq!(reserved_slot_refusal(&cleared, RUN, "prompt", delta), None);
        let mut best = inside.clone();
        best.push(flat("skills", "best of the round", STALL_WINDOW as u64, 5, 5 + delta + 1));
        assert!(
            !progress_stalled(&best, RUN, delta),
            "the best gain of the round clears the stall"
        );
        let mut older = vec![flat("prompt", "old gain", 1, 0, 40)];
        for round in 2..=STALL_WINDOW as u64 + 1 {
            older.push(flat("prompt", &format!("later {round}"), round, 4, 4));
        }
        assert!(progress_stalled(&older, RUN, delta), "only the last window is tested");
        assert!(!progress_stalled(&inside[..STALL_WINDOW - 1], RUN, delta));
        let flat_band = window("prompt", 4, 4);
        assert!(progress_stalled(&flat_band, RUN, 0), "a flat measured window is a plateau even with zero spread");
        assert_eq!(reserved_slot_refusal(&flat_band, RUN, "prompt", 0), Some(stall_refusal("prompt")));
        let dropped = window("prompt", 8, 6);
        assert!(progress_stalled(&dropped, RUN, delta), "a loss stays inside the band");
        let mut unscored_rows = inside[..STALL_WINDOW - 1].to_vec();
        unscored_rows.push(obj(vec![
            ("component", s("prompt")),
            ("hypothesis", s("no score")),
            ("round", n(STALL_WINDOW as u64)),
            ("run_digest", s(RUN)),
            ("verdict", s("rejected")),
        ]));
        assert!(
            !progress_stalled(&unscored_rows, RUN, delta),
            "a round with no score is not a scored round"
        );
    }

    #[test]
    fn every_tried_component_leaves_nothing_to_reserve() {
        let mut records = Vec::new();
        for (index, name) in COMPONENTS.iter().enumerate() {
            let round = (index as u64 % STALL_WINDOW as u64) + 1;
            records.push(flat(name, &format!("tried {name}"), round, 3, 3));
        }
        let delta = 1u64;
        assert!(progress_stalled(&records, RUN, delta));
        assert!(untried_components(&records, RUN).is_empty());
        assert_eq!(reserved_slot_refusal(&records, RUN, "prompt", delta), None);
        assert_eq!(reserved_slot_refusal(&records, RUN, "subagents", delta), None);
    }

    fn budget_if_control_rows_count(records: &[Json], run_digest: &str, round: u64) -> (u64, u64) {
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

    fn falsified_if_control_rows_count(records: &[Json], run_digest: &str) -> Vec<Json> {
        records
            .iter()
            .filter(|record| text(record, "run_digest") == run_digest && text(record, "verdict") == "rejected")
            .cloned()
            .collect()
    }

    fn prior_rejection_if_control_rows_count(
        records: &[Json],
        run_digest: &str,
        component: &str,
        hypothesis: &str,
    ) -> Option<u64> {
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

    #[test]
    fn stop_control_row_is_not_a_trial_for_budget_falsified_or_prior_rejection() {
        let round = 2u64;
        let trial = edit("skills", "real attempt", &diff_digest("a", "real attempt"), 1, 1, 4, 4, "rejected", round, RUN).unwrap();
        let stop = stop_edit(RUN, round, "incumbent").unwrap();
        let genuine_stop = edit("prompt", "STOP", &diff_digest("a", "STOP"), 1, 1, 4, 4, "rejected", round, RUN).unwrap();
        let genuine_other = edit(
            "prompt",
            "already tried",
            &diff_digest("a", "already tried"),
            1,
            1,
            4,
            4,
            "rejected",
            3,
            RUN,
        )
        .unwrap();
        let without = vec![trial.clone()];
        let mut with_stop = without.clone();
        with_stop.push(stop.clone());
        let counted_without = budget_if_control_rows_count(&without, RUN, round);
        let counted_with = budget_if_control_rows_count(&with_stop, RUN, round);
        assert_ne!(
            counted_without, counted_with,
            "counting the STOP row changes the budget tuple"
        );
        assert_eq!(budget_for(&without, RUN, round), counted_without);
        assert_eq!(
            budget_for(&with_stop, RUN, round),
            budget_for(&without, RUN, round),
            "excluding the STOP row leaves the budget tuple unchanged"
        );
        assert_eq!(budget_for(&with_stop, RUN, round).1, counted_without.1);
        let mut with_genuine = without.clone();
        with_genuine.push(genuine_other.clone());
        assert_ne!(
            budget_for(&with_genuine, RUN, 3),
            budget_for(&without, RUN, 3),
            "a real rejected trial still spends a slot"
        );
        let stop_only = vec![stop_edit(RUN, round, "incumbent").unwrap()];
        let empty: Vec<Json> = Vec::new();
        assert_ne!(
            budget_if_control_rows_count(&stop_only, RUN, 5),
            budget_if_control_rows_count(&empty, RUN, 5),
            "a STOP row in another round anneals the budget while it is counted"
        );
        assert_eq!(budget_for(&stop_only, RUN, 5), budget_for(&empty, RUN, 5));
        let listed = vec![genuine_stop.clone(), stop.clone(), genuine_other.clone()];
        let before = falsified_if_control_rows_count(&listed, RUN);
        assert!(before.iter().any(|record| text(record, "control") == "stop"));
        assert!(before.iter().any(|record| text(record, "hypothesis") == "already tried"));
        assert!(before.iter().any(|record| text(record, "hypothesis") == "STOP" && text(record, "control") != "stop"));
        let after = falsified(&listed, RUN);
        assert!(after.iter().all(|record| text(record, "control") != "stop"));
        assert!(after.iter().any(|record| text(record, "hypothesis") == "already tried"));
        assert!(after.iter().any(|record| text(record, "hypothesis") == "STOP"));
        assert_eq!(after.len(), 2);
        let stop_ledger = vec![stop.clone()];
        assert_eq!(
            prior_rejection_if_control_rows_count(&stop_ledger, RUN, "prompt", "STOP"),
            Some(round),
            "the control row refuses hypothesis STOP while it is counted"
        );
        assert_eq!(prior_rejection(&stop_ledger, RUN, "prompt", "STOP"), None);
        let between = vec![trial.clone(), stop.clone()];
        assert_eq!(prior_rejection(&between, RUN, "prompt", "STOP"), None);
        assert_eq!(
            prior_rejection_if_control_rows_count(&between, RUN, "prompt", "STOP"),
            Some(round)
        );
        let real = vec![genuine_stop.clone()];
        assert_eq!(prior_rejection(&real, RUN, "prompt", "STOP"), Some(round));
        assert_eq!(prior_rejection_if_control_rows_count(&real, RUN, "prompt", "STOP"), Some(round));
        assert_eq!(
            rejection_refusal("prompt", round),
            "refused because component prompt with that hypothesis was already rejected in round 2"
        );
    }

    #[test]
    fn stop_control_round_zero_means_nothing_was_closed() {
        let row = stop_edit(RUN, 0, "kept").unwrap();
        assert_eq!(number_field(&row, "round").unwrap(), 0);
        assert_eq!(text(&row, "control"), "stop");
        let saved = append_document("", &row, RUN).unwrap();
        let records = records_of(&text(&saved, "document")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(number_field(&records[0], "round").unwrap(), 0);
        let trial = edit("prompt", "a real round", &diff_digest("a", "b"), 0, 1, 1, 1, "accepted", 0, RUN);
        let error = trial.unwrap_err();
        assert_eq!(error, "round must be positive");
        let closed = stop_edit(RUN, 4, "kept").unwrap();
        assert_eq!(number_field(&closed, "round").unwrap(), 4);
    }

    #[test]
    fn stop_control_row_stays_in_the_library_index_and_historical_render() {
        // Round 0 is legal on a control row and refused on a trial (`validate_fields`).
        // `index_of` copies key, revision, and run_digest and does not read `control`,
        // and `library_index` maps every record through that function. Dropping the
        // control row from a stored index then disagrees with the map.
        let stop = stop_edit(RUN, 0, "kept").unwrap();
        let mut document = text(&append_document("", &stop, RUN).unwrap(), "document");
        let trial = edit(
            "skills",
            "real attempt",
            &diff_digest("a", "real attempt"),
            1,
            1,
            4,
            4,
            "rejected",
            1,
            RUN,
        )
        .unwrap();
        document = text(&append_document(&document, &trial, RUN).unwrap(), "document");
        let records = records_of(&document).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(number_field(&records[0], "round").unwrap(), 0);
        assert_eq!(text(&records[0], "control"), "stop");
        assert_eq!(text(&records[0], "hypothesis"), "STOP");
        let mapped: Vec<Json> = records
            .iter()
            .map(|record| {
                obj(vec![
                    ("key", s(&text(record, "key"))),
                    ("revision", s(&text(record, "revision"))),
                    ("run_digest", s(&text(record, "run_digest"))),
                ])
            })
            .collect();
        assert_eq!(library_index(&records), mapped);
        assert_eq!(mapped.len(), records.len());
        assert_eq!(text(&mapped[0], "run_digest"), RUN);
        assert_eq!(text(&mapped[0], "revision"), text(&records[0], "revision"));
        assert_eq!(text(&mapped[0], "key"), "");
        assert!(!index_disagrees(&Arr(mapped.clone()), &records));
        assert!(index_disagrees(&Arr(mapped[1..].to_vec()), &records));
        let section = render_historical(&records, "task t");
        assert!(section.contains("HISTORICAL EXPERIENCE"), "{section}");
        assert!(section.contains(&text(&records[0], "revision")), "{section}");
        assert!(section.contains(&text(&records[1], "revision")), "{section}");
        let rejected = falsified(&records, RUN);
        assert_eq!(rejected.len(), 1);
        assert_eq!(text(&rejected[0], "hypothesis"), "real attempt");
        assert!(rejected.iter().all(|row| text(row, "control") != "stop"));
        assert_eq!(component_counts(&records, RUN), vec![("skills".to_string(), 1)]);
        assert_eq!(budget_for(&records, RUN, 2), (3, 3));
        assert_eq!(prior_rejection(&records, RUN, "prompt", "STOP"), None);
        assert_eq!(prior_rejection(&records, RUN, "skills", "real attempt"), Some(1));
    }

    #[test]
    fn stop_row_does_not_count_as_a_tried_component() {
        let stalled: Vec<Json> = (1..=STALL_WINDOW as u64)
            .map(|round| flat("tools", &format!("tools {round}"), round, 4, 5))
            .collect();
        let delta = 1u64;
        assert!(progress_stalled(&stalled, RUN, delta));
        let before_counts = component_counts(&stalled, RUN);
        let before_untried = untried_components(&stalled, RUN);
        assert!(before_untried.contains(&"prompt"));
        assert_eq!(reserved_slot_refusal(&stalled, RUN, "prompt", delta), None);
        let stop = stop_edit(RUN, STALL_WINDOW as u64, "incumbent").unwrap();
        let mut with_stop = stalled.clone();
        with_stop.push(stop);
        assert_eq!(component_counts(&with_stop, RUN), before_counts);
        assert_eq!(untried_components(&with_stop, RUN), before_untried);
        assert_eq!(
            reserved_slot_refusal(&with_stop, RUN, "prompt", delta),
            None,
            "a STOP row must not refuse a prompt candidate that was never tried"
        );
    }

    #[test]
    fn stop_control_row_seals_and_a_scored_control_row_is_refused() {
        let row = stop_edit(RUN, 1, "kept").unwrap();
        assert_eq!(text(&row, "control"), "stop");
        assert_eq!(text(&row, "hypothesis"), "STOP");
        let saved = append_document("", &row, RUN).unwrap();
        let records = records_of(&text(&saved, "document")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(text(&records[0], "control"), "stop");
        assert_eq!(text(&records[0], "decision"), STOP_SENTENCE);
        assert_eq!(get(&records[0], "score_before"), &Null);
        assert_eq!(get(&records[0], "score_after"), &Null);
        assert_eq!(text(&records[0], "verdict"), "rejected");
        let mut scored = edit("prompt", "STOP", &diff_digest("a", "b"), 0, 1, 1, 1, "rejected", 1, RUN).unwrap();
        put(&mut scored, "control", s("stop"));
        put(&mut scored, "decision", s(STOP_SENTENCE));
        let error = append_document("", &scored, RUN).unwrap_err();
        assert!(error.contains("must not record a score"), "{error}");
    }

    #[test]
    fn stop_sentence_on_an_unrelated_row_fails_validation() {
        let row = obj(vec![
            ("component", s("skills")),
            ("hypothesis", s("unrelated hypothesis")),
            ("diff_digest", s(&diff_digest("a", "b"))),
            ("cost_before", n(1)),
            ("cost_after", n(1)),
            ("cost_change", Num("0".into())),
            ("verdict", s("rejected")),
            ("accepted", Bool(false)),
            ("round", n(1)),
            ("run_digest", s(RUN)),
            ("decision", s(STOP_SENTENCE)),
        ]);
        let error = append_document("", &row, RUN).unwrap_err();
        assert!(
            error.contains("score_before"),
            "an unrelated row carrying the STOP sentence must fail validation, got {error}"
        );
    }

    #[test]
    fn stalled_prompt_names_the_reserved_slots_and_a_quiet_ledger_stays_unchanged() {
        let mut quiet = String::new();
        let mut stalled = String::new();
        let mut cleared = String::new();
        for round in 1..=STALL_WINDOW as u64 {
            let row = flat("prompt", &format!("flat {round}"), round, 4, 5);
            quiet = text(&append_document(&quiet, &row, RUN).unwrap(), "document");
            let mut marked = row.clone();
            put(&mut marked, "noise_floor", n(1));
            stalled = text(&append_document(&stalled, &marked, RUN).unwrap(), "document");
            let score = if round == STALL_WINDOW as u64 { 6 } else { 4 };
            let mut moved = flat("prompt", &format!("flat {round}"), round, 4, score);
            put(&mut moved, "noise_floor", n(1));
            cleared = text(&append_document(&cleared, &moved, RUN).unwrap(), "document");
        }
        let quiet_section = prompt_document(&quiet, RUN).unwrap();
        let stalled_section = prompt_document(&stalled, RUN).unwrap();
        let cleared_section = prompt_document(&cleared, RUN).unwrap();
        assert!(!quiet_section.contains("Progress stalled"), "{quiet_section}");
        assert!(!quiet_section.contains("Reserved slots"), "{quiet_section}");
        assert!(stalled_section.starts_with(&quiet_section), "{stalled_section}");
        assert!(
            stalled_section.contains(&format!(
                "Progress stalled inside the noise band for {STALL_WINDOW} rounds."
            )),
            "{stalled_section}"
        );
        let reserved = untried_components(&records_of(&stalled).unwrap(), RUN).join(", ");
        assert!(
            stalled_section.contains(&format!("Reserved slots for components not yet tried: {reserved}")),
            "{stalled_section}"
        );
        assert!(stalled_section.contains("tools"), "{stalled_section}");
        assert!(!reserved.split(", ").any(|name| name == "prompt"));
        assert!(stalled_section.len() <= PROMPT_MAX);
        assert_eq!(cleared_section, quiet_section);
    }

    #[test]
    fn a_stalled_plateau_raises_the_required_gain_and_says_the_work_is_not_finished() {
        // Three scored rounds inside the noise band: a plateau.
        let mut stalled = Vec::new();
        for round in 1..=STALL_WINDOW as u64 {
            stalled.push(flat("prompt", &format!("flat {round}"), round, 4, 5));
        }
        // The bar rises by the noise floor: a required gain of 4 becomes 8.
        assert_eq!(acceptance_floor(&stalled, RUN, 4), 8);
        // A plateau with no measured spread still raises the bar, so the next
        // round must gain more than nothing.
        let mut level = Vec::new();
        for round in 1..=STALL_WINDOW as u64 {
            level.push(flat("prompt", &format!("level {round}"), round, 4, 4));
        }
        assert_eq!(acceptance_floor(&level, RUN, 0), 1);
        // A ledger that is not stalled keeps its bar.
        let mut moving = Vec::new();
        for round in 1..=STALL_WINDOW as u64 {
            moving.push(flat("prompt", &format!("gain {round}"), round, 4, 12));
        }
        assert_eq!(acceptance_floor(&moving, RUN, 4), 4);
        // The proposal prompt says in plain words that the work is not finished.
        let mut document = String::new();
        for round in 1..=STALL_WINDOW as u64 {
            let mut row = flat("prompt", &format!("flat {round}"), round, 4, 5);
            put(&mut row, "noise_floor", n(4));
            document = text(&append_document(&document, &row, RUN).unwrap(), "document");
        }
        let section = prompt_document(&document, RUN).unwrap();
        assert!(
            section.contains("switch component or raise the bar; the work is not finished"),
            "{section}"
        );
        assert!(
            section.contains("The required gain for the next round rises by the noise floor (4)."),
            "{section}"
        );
        assert!(section.len() <= PROMPT_MAX, "the notice survives the cut");
        // A stalled round is scored, never refused.
        let saved = append_document(&document, &flat("prompt", "another flat", 9, 4, 5), RUN).unwrap();
        assert_eq!(get(&saved, "recorded"), &Bool(true));
        assert_eq!(get(&saved, "evicted"), &Bool(false));
    }

    #[test]
    fn the_stall_notice_survives_a_history_longer_than_the_prompt_cap() {
        // The cap cuts the history, never the notice: the sentence that says
        // the work is not finished is what the proposer must read.
        let notice = "The bar moved, so switch component or raise the bar; the work is not finished. \
             The required gain for the next round rises by the noise floor (4).";
        let history = format!("{}\nTAIL-MARKER", "h".repeat(PROMPT_MAX * 2));
        let section = compose_section(&history, notice);
        assert!(
            section.len() <= PROMPT_MAX,
            "the section stays inside the prompt cap: {}",
            section.len()
        );
        assert!(
            section.contains("switch component or raise the bar; the work is not finished"),
            "the notice survives the cut: {section}"
        );
        assert!(
            !section.contains("TAIL-MARKER"),
            "the cut really bit the history: {section}"
        );
        assert!(
            section.ends_with(notice),
            "the notice is the last thing the proposer reads"
        );
        assert_eq!(section.len(), PROMPT_MAX, "the cut lands exactly on the cap");
        println!(
            "INTERLOCK prompt cap={PROMPT_MAX} section={} history={} notice={} tail_dropped={}",
            section.len(),
            history.len(),
            notice.len(),
            !section.contains("TAIL-MARKER")
        );
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

    #[test]
    fn resume_receipt_carries_library_index_in_load_order() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let name = format!("{RUN}.jsonl");
        let row = edit(
            "prompt",
            "kept wording",
            &diff_digest("before", "kept wording"),
            1,
            2,
            4,
            5,
            "accepted",
            1,
            RUN,
        )
        .unwrap();
        let ledger = text(&append_document("", &row, RUN).unwrap(), "document");
        append_file(root, &name, &row, RUN).unwrap();
        let first = append_experience(root, "task alpha", "harness-alpha", &ledger, RUN).unwrap();
        let second = append_experience(root, "task beta", "harness-beta", "", OTHER).unwrap();
        let library_path = scratch.0.join("experience.jsonl");
        let ledger_path = scratch.0.join(&name);
        let library_bytes = fs::read(&library_path).unwrap();
        let ledger_bytes = fs::read(&ledger_path).unwrap();
        let resumed = resume_run(root, &name, "task alpha").unwrap();
        assert_eq!(fs::read(&library_path).unwrap(), library_bytes);
        assert_eq!(fs::read(&ledger_path).unwrap(), ledger_bytes);
        assert_eq!(text(&resumed, "run_digest"), RUN);
        assert_eq!(text(&resumed, "adopted_harness"), "harness-alpha");
        assert_eq!(text(&resumed, "harness_digest"), digest(b"harness-alpha"));
        assert_eq!(text(&resumed, "edit_ledger").as_bytes(), ledger_bytes.as_slice());
        assert_eq!(number_field(&resumed, "round").unwrap(), 1);
        let loaded = load_experience(root).unwrap();
        let rows = match get(&resumed, "library_index") {
            Arr(items) => items.clone(),
            other => panic!("library_index missing from the resume receipt: {other:?}"),
        };
        assert_eq!(rows, library_index(&loaded));
        assert_eq!(rows.len(), loaded.len());
        assert_eq!(rows.len(), 2);
        assert_eq!(text(&rows[0], "key"), library_key_of("task alpha"));
        assert_eq!(text(&rows[0], "key"), text(&loaded[0], "key"));
        assert_eq!(text(&rows[0], "revision"), text(&first, "revision"));
        assert_eq!(text(&rows[0], "revision"), text(&loaded[0], "revision"));
        assert_eq!(text(&rows[0], "run_digest"), RUN);
        assert_eq!(text(&rows[1], "key"), library_key_of("task beta"));
        assert_eq!(text(&rows[1], "key"), text(&loaded[1], "key"));
        assert_eq!(text(&rows[1], "revision"), text(&second, "revision"));
        assert_eq!(text(&rows[1], "revision"), text(&loaded[1], "revision"));
        assert_eq!(text(&rows[1], "run_digest"), OTHER);
        let ledger_rows = records_of(&text(&resumed, "edit_ledger")).unwrap();
        assert_eq!(ledger_rows.len(), 1);
        assert_ne!(rows.len(), ledger_rows.len());
    }

    #[test]
    fn resume_library_accepts_line_cap_and_refuses_one_past() {
        let scratch = Scratch::new();
        let root = scratch.0.to_str().unwrap();
        let name = format!("{RUN}.jsonl");
        let row = edit(
            "prompt",
            "kept wording",
            &diff_digest("before", "kept wording"),
            1,
            2,
            4,
            5,
            "accepted",
            1,
            RUN,
        )
        .unwrap();
        append_file(root, &name, &row, RUN).unwrap();
        for index in 0..LINE_CAP {
            append_experience(root, &format!("task {index}"), &format!("harness {index}"), "", RUN).unwrap();
        }
        let library_path = scratch.0.join("experience.jsonl");
        let ledger_path = scratch.0.join(&name);
        let library_bytes = fs::read(&library_path).unwrap();
        let ledger_bytes = fs::read(&ledger_path).unwrap();
        assert_eq!(String::from_utf8(library_bytes.clone()).unwrap().lines().count(), LINE_CAP);
        let resumed = resume_run(root, &name, "task 0").unwrap();
        assert_eq!(fs::read(&library_path).unwrap(), library_bytes);
        assert_eq!(fs::read(&ledger_path).unwrap(), ledger_bytes);
        let loaded = load_experience(root).unwrap();
        assert_eq!(loaded.len(), LINE_CAP);
        let rows = match get(&resumed, "library_index") {
            Arr(items) => items.clone(),
            other => panic!("library index missing: {other:?}"),
        };
        assert_eq!(rows, library_index(&loaded));
        assert_eq!(rows.len(), LINE_CAP);
        assert_eq!(text(&rows[0], "key"), library_key_of("task 0"));
        assert_eq!(text(&rows[0], "run_digest"), RUN);
        assert_eq!(text(&resumed, "run_digest"), RUN);
        assert_eq!(text(&resumed, "edit_ledger").as_bytes(), ledger_bytes.as_slice());
        assert_eq!(number_field(&resumed, "round").unwrap(), 1);
        assert_eq!(text(&resumed, "adopted_harness"), format!("harness {}", LINE_CAP - 1));
        let cap = "improvement library exceeds its line cap; history was kept and no line was dropped";
        let mut over = library_bytes.clone();
        let end = library_bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        over.extend_from_slice(&library_bytes[..end]);
        assert_eq!(String::from_utf8(over.clone()).unwrap().lines().count(), LINE_CAP + 1);
        fs::write(&library_path, &over).unwrap();
        let library_before = fs::read(&library_path).unwrap();
        let ledger_before = fs::read(&ledger_path).unwrap();
        let refused = resume_run(root, &name, "task 0").unwrap_err();
        let loaded_error = load_experience(root).unwrap_err();
        assert_eq!(refused, cap);
        assert_eq!(loaded_error, cap);
        assert_eq!(fs::read(&library_path).unwrap(), library_before);
        assert_eq!(fs::read(&ledger_path).unwrap(), ledger_before);
    }
}

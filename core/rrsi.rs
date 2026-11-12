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
use std::os::unix::fs::OpenOptionsExt;
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
fn validate_fields(value: &Json, expected_run: &str) -> Result<()> {
    let component = text(value, "component");
    if component_index(&component).is_none() {
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
    for key in ["score_before", "score_after", "cost_before", "cost_after"] {
        number_field(value, key)?;
    }
    let run_digest = text(value, "run_digest");
    if !hex_digest(&run_digest) || run_digest != expected_run {
        return Err("run_digest does not match the run being recorded".into());
    }
    validate_policy(value)
}
fn edit_keys(sealed: bool) -> Vec<&'static str> {
    let mut keys = EDIT_KEYS.to_vec();
    keys.extend_from_slice(POLICY_KEYS);
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
    format!("\nEDIT BUDGET\nEdits remaining this round: {remaining}. The annealed budget is {budget}.")
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
            lines.push(format!(
                "- round {} · {} · {}",
                number_field(record, "round")?,
                text(record, "component"),
                clip(&text(record, "hypothesis"), PROMPT_HYPOTHESIS)
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
}

//! Append-only edit ledger for one refinement run.
//! Records the verdict the refine loop already reached. It does not choose that verdict.
//! Cost is the UTF-8 length of the instructions being replaced and of the proposal.
//! A full line cap refuses the new record and leaves every existing line in place.
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
    Ok(())
}
fn validate_edit(value: &Json, expected_run: &str) -> Result<()> {
    only_keys(value, EDIT_KEYS)?;
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
    let stored = EDIT_KEYS.iter().copied().chain(["schema", "previous", "revision"]);
    only_keys(value, &stored.collect::<Vec<_>>())?;
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
    std::env::temp_dir().join("rllm-rrsi").to_string_lossy().into_owned()
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
}

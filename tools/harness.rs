// Connected-check runner. Replaces the Python driver's role and reports the
// difference between a product failure and a missing development input.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime};

const GROUPS: &[&str] = &[
    "provider_process", "network_drive", "drive_ownership", "question_process", "history", "a2a", "acp",
    "oauth", "device_authorization", "binary_flow", "pe_declarations", "commands", "history_migration",
    "hosted_phases", "prompt_reference", "provider_evidence", "delegation_visibility", "local_feedback",
    "network_feedback", "proposal_feedback", "proposal_recovery", "review_ownership", "task_ownership",
    "dav_listing", "server_admission", "http_framing", "gateway_expectation", "device_shortcut",
    "request_fields", "response_fields", "chunk_fields", "storage_etags", "platform_resolution",
    "query_feedback", "carry_flow", "workspace_navigation", "mcp_catalog", "mcp_catalog_events",
    "mcp_images", "mcp_continuation", "mcp_history", "document_providers", "snapshot_identity",
    "storage_auth", "storage_oauth", "credential_rotation", "storage_owner", "voice_settings",
    "field_flow", "flags_flow", "product_flow", "diagnostics", "services", "media", "jobs", "voice",
    "media_stream", "media_upload", "voice_interruption", "voice_background", "voice_session",
    "capture_lifecycle", "scene", "apk_alignment", "audio_route", "state_sealing", "desktop_runtime",
    "turn_background", "permission_modes",
];

// Scripts under verification/ that are not runner groups. A file outside these
// sets is an unwired check and stops the census.
const HELPERS: &[&str] = &["transport"];
const OPTIONAL: &[(&str, &str)] = &[("live_web", "requires --live: performs real public HTTPS searches")];

const ENV_TOKENS: &[(&str, &str)] = &[
    ("os.environ['CLIENT_NDK']", "CLIENT_NDK"),
    ("os.environ[\"CLIENT_NDK\"]", "CLIENT_NDK"),
    ("os.environ['CLIENT_JDK']", "CLIENT_JDK"),
    ("os.environ[\"CLIENT_JDK\"]", "CLIENT_JDK"),
    ("os.environ['CLIENT_ANDROID']", "CLIENT_ANDROID"),
    ("os.environ['CLIENT_TOOLS']", "CLIENT_TOOLS"),
    ("os.environ['CLIENT_RUST']", "CLIENT_RUST"),
    ("os.environ['RLLM_CHECK_APK']", "RLLM_CHECK_APK"),
    ("os.environ['CLIENT_CAPTURE_TEST_FONT']", "CLIENT_CAPTURE_TEST_FONT"),
];

// Reference binaries a group can consume. Whether one is required comes from the
// group's own source: required=True, or a default path the group reads without a
// guard, means the script will open it, so a missing input is reported instead of
// crashing the group. A group that reads the value only inside a guard runs
// without it and records the comparison as an unexercised leg.
const INPUT_FLAGS: &[(&str, &str, &str)] = &[
    ("--previous-controller", "output/previous-controller", "RLLM_PREVIOUS_CONTROLLER"),
    ("--previous-flow-controller", "output/pre-branch-controller", "RLLM_PREVIOUS_FLOW_CONTROLLER"),
    ("--previous-auth-controller", "output/pre-auth-controller", "RLLM_PREVIOUS_AUTH_CONTROLLER"),
    ("--previous-scene-controller", "", "RLLM_PREVIOUS_SCENE_CONTROLLER"),
    ("--published-controller", "output/published-controller", "RLLM_PUBLISHED_CONTROLLER"),
    ("--auth-baseline", "", "RLLM_AUTH_BASELINE"),
];

const SECONDS: u64 = 180;

struct Input {
    kind: &'static str,
    detail: String,
    present: bool,
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// A group's inputs come from its own source, so a newly required toolchain is
// discovered instead of being assumed by a hand-maintained table.
fn required_inputs(root: &Path, source: &str) -> Vec<Input> {
    let mut inputs = Vec::new();
    let mut seen = BTreeSet::new();
    for (token, name) in ENV_TOKENS {
        if source.contains(token) && seen.insert((*name).to_string()) {
            inputs.push(Input {
                kind: "env",
                detail: format!("{name} environment variable"),
                present: match std::env::var(name) {
                    Ok(value) => !value.trim().is_empty(),
                    Err(_) => false,
                },
            });
        }
    }
    for (flag, default, _) in INPUT_FLAGS {
        let declared = format!("add_argument('{flag}'");
        if !source.contains(&declared) {
            continue;
        }
        let detail = if default.is_empty() {
            format!("{flag} reference (supply it with the matching environment variable)")
        } else {
            format!("{flag} reference at {default}")
        };
        let path = flag_path(root, flag).or_else(|| {
            (!default.is_empty()).then(|| root.join(default))
        });
        let required = flag_required(source, flag, default);
        if required && !path.map(|path| path.exists()).unwrap_or(false) {
            inputs.push(Input { kind: "path", detail, present: false });
        }
    }
    // A group can also open a reference build without declaring a flag, by
    // hardcoding the path. Detect that too, so a missing development input is
    // reported as unavailable instead of being charged to the product as a FAIL.
    for (_, default, _) in INPUT_FLAGS {
        if default.is_empty() || !source.contains(default) {
            continue;
        }
        if inputs.iter().any(|input| input.detail.contains(default)) {
            continue;
        }
        inputs.push(Input {
            kind: "path",
            detail: format!("{default} reference build (the group opens this path itself)"),
            present: root.join(default).exists(),
        });
    }
    // A group can also shell a native tool that tools/build-tools.sh compiles.
    // That is a build artifact rather than a source file, so a missing one is an
    // unavailable development input, not a product failure.
    for tool in [
        "alignment",
        "readback",
        "checkpoint",
        "signature",
        "apk",
        "toolchain",
        "gate",
    ] {
        let reference = format!("output/tools/{tool}");
        if source.contains(&reference) {
            inputs.push(Input {
                kind: "path",
                detail: format!("{reference} (built by tools/build-tools.sh)"),
                present: root.join(&reference).exists(),
            });
        }
    }
    if source.contains("RLLM_CHECK_APK") || source.contains("release/RLLM.apk") {
        if seen.insert("apk".into()) {
            let configured = std::env::var("RLLM_CHECK_APK").unwrap_or_default();
            let (path, detail) = if configured.is_empty() {
                published_apk(root)
            } else {
                let path = PathBuf::from(&configured);
                (path.clone(), path.display().to_string())
            };
            inputs.push(Input {
                kind: "path",
                detail,
                present: path.exists(),
            });
        }
    }
    inputs
}

// PUBLICATION-MANIFEST.json is the only place that says where the signed APK is
// delivered. A companion delivery means the bytes are not in the tree, so a
// group's subject is the record's file or an explicit RLLM_CHECK_APK, never an
// invented default path that then reads as a product failure.
fn published_apk(root: &Path) -> (PathBuf, String) {
    let text = std::fs::read_to_string(root.join("PUBLICATION-MANIFEST.json")).unwrap_or_default();
    let record = match text.find("\"signed_apk\"") {
        Some(at) => {
            let end = text[at..].find('}').map_or(text.len(), |offset| at + offset);
            text[at..end].to_string()
        }
        None => String::new(),
    };
    let field = |key: &str| -> Option<String> {
        let needle = format!("\"{key}\": \"");
        let start = record.find(&needle)? + needle.len();
        let rest = &record[start..];
        Some(rest[..rest.find('"')?].to_string())
    };
    let name = field("path").unwrap_or_default();
    let name = name.rsplit('/').next().unwrap_or("").to_string();
    if !name.is_empty() && field("location").as_deref() == Some("companion") {
        let mut candidates = vec![root.join(&name)];
        if let Ok(entries) = std::fs::read_dir(root.join("output")) {
            candidates.extend(entries.flatten().map(|entry| entry.path().join(&name)));
        }
        if let Some(found) = candidates.iter().find(|path| path.is_file()) {
            return (
                found.clone(),
                format!("{} (companion APK named by PUBLICATION-MANIFEST.json)", found.display()),
            );
        }
        return (
            root.join(&name),
            format!("companion APK {name} named by PUBLICATION-MANIFEST.json (not on this machine)"),
        );
    }
    (root.join("release/RLLM.apk"), "release/RLLM.apk".into())
}

fn declares_required(source: &str, flag: &str) -> bool {
    let declared = format!("add_argument('{flag}'");
    let Some(at) = source.find(&declared) else { return false };
    let line_end = source[at..].find('\n').map(|offset| at + offset).unwrap_or(source.len());
    let declaration = &source[at..line_end];
    // A single line can declare several arguments, so stop at the next one.
    let declaration = match declaration.find("add_argument(") {
        Some(first) => {
            match declaration[first + 13..].find("add_argument(") {
                Some(next) => &declaration[..first + 13 + next],
                None => declaration,
            }
        }
        None => declaration,
    };
    declaration.contains("required=True")
}

// A group that reads a reference only inside a guard runs without it; one that
// reads it unconditionally cannot. Both signals live in the group's own source,
// so the pre-flight and the argument builder must read the same authority here.
fn declares_optional_guard(source: &str, flag: &str) -> bool {
    let name = format!("args.{}", flag.trim_start_matches('-').replace('-', "_"));
    source.contains(&format!("if {name}")) || source.contains(&format!("if !{name}"))
}

fn flag_required(source: &str, flag: &str, default: &str) -> bool {
    !declares_optional_guard(source, flag) && (declares_required(source, flag) || !default.is_empty())
}

fn flag_path(root: &Path, flag: &str) -> Option<PathBuf> {
    let (_, default, variable) = INPUT_FLAGS.iter().find(|(name, _, _)| *name == flag)?;
    let configured = std::env::var(variable).unwrap_or_default();
    if !configured.trim().is_empty() {
        let path = PathBuf::from(&configured);
        return path.exists().then_some(path);
    }
    if default.is_empty() {
        return None;
    }
    let path = root.join(default);
    path.exists().then_some(path)
}

// A required reference is validated before dispatch; an optional one is passed
// only when it actually exists, and its absence is reported as an unexercised
// comparison leg rather than a silent narrowing.
fn group_arguments(root: &Path, source: &str) -> (Vec<String>, Vec<String>) {
    let mut arguments = Vec::new();
    let mut unexercised = Vec::new();
    for (flag, default, _) in INPUT_FLAGS {
        if !source.contains(&format!("add_argument('{flag}'")) {
            continue;
        }
        let supplied = flag_path(root, flag).or_else(|| {
            (!default.is_empty()).then(|| root.join(default)).filter(|path| path.exists())
        });
        match supplied {
            Some(path) => {
                arguments.push((*flag).to_string());
                arguments.push(path.display().to_string());
            }
            None => {
                if !flag_required(source, flag, default) {
                    unexercised.push((*flag).to_string());
                }
            }
        }
    }
    (arguments, unexercised)
}

fn census(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let mut scripts = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("verification")).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("py") {
            if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                scripts.insert(stem.to_string());
            }
        }
    }
    for name in GROUPS {
        if !scripts.contains(*name) {
            problems.push(format!("group {name} has no verification/{name}.py"));
        }
    }
    let declared: BTreeSet<String> = GROUPS.iter().map(|value| value.to_string())
        .chain(HELPERS.iter().map(|value| value.to_string()))
        .chain(OPTIONAL.iter().map(|(value, _)| value.to_string()))
        .collect();
    for script in &scripts {
        if !declared.contains(script) {
            problems.push(format!("verification/{script}.py is not a runner group, a declared helper or a declared optional group"));
        }
    }
    let duplicates: BTreeSet<&str> = GROUPS.iter().copied().collect();
    if duplicates.len() != GROUPS.len() {
        problems.push("the runner inventory contains a duplicate group name".into());
    }
    problems
}

// RESULTS.json holds the counts recorded for a delivered candidate. A differing
// inventory or count is reported instead of silently restated.
fn counts_in(text: &str) -> Option<(usize, usize)> {
    let start = text.find("\"counts\"")?;
    let body = &text[start..];
    let open = body.find('{')?;
    let close = body.find('}')?;
    let inner = &body[open + 1..close];
    let mut groups = 0;
    let mut total = 0;
    for part in inner.split(',') {
        let mut pieces = part.split(':');
        let key = pieces.next()?.trim().trim_matches('"');
        let value = pieces.next()?.trim();
        if !key.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            groups += 1;
            total += value.parse::<usize>().unwrap_or(0);
        }
    }
    Some((groups, total))
}

fn recorded(root: &Path) -> Option<(usize, usize)> {
    counts_in(&read(&root.join("verification/RESULTS.json")))
}

// A group reports its check count on stdout, as a final JSON object with an
// integer count, an integer passed, or a passed/checks list. A group may also
// write that object under output/checks. Those shapes have to agree. A missing
// or disagreeing count stays unparsed so it cannot be stored as a silent zero.
enum Counted {
    Absent,
    Value(usize),
    Conflict,
}

#[derive(Default)]
struct Measure {
    count: Option<usize>,
    passed_int: Option<usize>,
    passed_len: Option<usize>,
    checks_len: Option<usize>,
}

impl Measure {
    fn finish(self) -> Counted {
        let mut values = Vec::new();
        for value in [self.count, self.passed_int, self.passed_len, self.checks_len] {
            if let Some(value) = value {
                values.push(value);
            }
        }
        if values.is_empty() {
            Counted::Absent
        } else if values.iter().all(|value| *value == values[0]) {
            Counted::Value(values[0])
        } else {
            Counted::Conflict
        }
    }
}

struct Scan<'a> {
    text: &'a [u8],
    i: usize,
}

impl<'a> Scan<'a> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.i).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }

    fn starts_number(&self) -> bool {
        matches!(self.peek(), Some(b'-' | b'0'..=b'9'))
    }

    fn skip_string(&mut self) -> Option<()> {
        if !self.eat(b'"') {
            return None;
        }
        while let Some(byte) = self.peek() {
            self.i += 1;
            if byte == b'"' {
                return Some(());
            }
            if byte == b'\\' {
                let escaped = self.peek()?;
                self.i += 1;
                if escaped == b'u' {
                    for _ in 0..4 {
                        if !self.peek()?.is_ascii_hexdigit() {
                            return None;
                        }
                        self.i += 1;
                    }
                }
            }
        }
        None
    }

    fn key(&mut self) -> Option<String> {
        let start = self.i;
        self.skip_string()?;
        if self.i < start + 2 {
            return None;
        }
        let raw = &self.text[start + 1..self.i - 1];
        std::str::from_utf8(raw).ok().map(str::to_string)
    }

    // Some(Some(n)) is a plain non-negative integer. Some(None) is some other
    // JSON number, already consumed. None means this was not a number.
    fn take_json_int(&mut self) -> Option<Option<usize>> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        let digits_at = self.i;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.i += 1;
        }
        if self.i == digits_at {
            self.i = start;
            return None;
        }
        let digit_end = self.i;
        let fractional = matches!(self.peek(), Some(b'.' | b'e' | b'E'));
        if self.eat(b'.') {
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if start != digits_at || fractional {
            return Some(None);
        }
        let text = std::str::from_utf8(&self.text[digits_at..digit_end]).ok()?;
        Some(text.parse::<usize>().ok())
    }

    fn skip_number(&mut self) -> Option<()> {
        self.take_json_int()?;
        Some(())
    }

    fn skip_value(&mut self) -> Option<()> {
        self.skip_ws();
        match self.peek()? {
            b'"' => self.skip_string(),
            b'{' => self.skip_object(),
            b'[' => self.array_len().map(|_| ()),
            b't' => self.eat_lit(b"true"),
            b'f' => self.eat_lit(b"false"),
            b'n' => self.eat_lit(b"null"),
            b'-' | b'0'..=b'9' => self.skip_number(),
            _ => None,
        }
    }

    fn eat_lit(&mut self, literal: &[u8]) -> Option<()> {
        if self.text[self.i..].starts_with(literal) {
            self.i += literal.len();
            Some(())
        } else {
            None
        }
    }

    fn array_len(&mut self) -> Option<usize> {
        if !self.eat(b'[') {
            return None;
        }
        let mut count = 0;
        loop {
            self.skip_ws();
            if self.eat(b']') {
                return Some(count);
            }
            count += 1;
            self.skip_value()?;
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                return Some(count);
            }
            return None;
        }
    }

    fn skip_object(&mut self) -> Option<()> {
        if !self.eat(b'{') {
            return None;
        }
        loop {
            self.skip_ws();
            if self.eat(b'}') {
                return Some(());
            }
            self.skip_string()?;
            self.skip_ws();
            if !self.eat(b':') {
                return None;
            }
            self.skip_value()?;
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Some(());
            }
            return None;
        }
    }
}

fn measure_object(text: &str, start: usize) -> Option<(Counted, usize)> {
    let mut scan = Scan { text: text.as_bytes(), i: start };
    if !scan.eat(b'{') {
        return None;
    }
    let mut measure = Measure::default();
    loop {
        scan.skip_ws();
        if scan.eat(b'}') {
            return Some((measure.finish(), scan.i));
        }
        let key = scan.key()?;
        scan.skip_ws();
        if !scan.eat(b':') {
            return None;
        }
        scan.skip_ws();
        let watched = matches!(key.as_str(), "count" | "passed" | "checks");
        if watched && scan.starts_number() {
            if let Some(value) = scan.take_json_int()? {
                match key.as_str() {
                    "count" => measure.count = Some(value),
                    "passed" => measure.passed_int = Some(value),
                    _ => {}
                }
            }
        } else if matches!(key.as_str(), "passed" | "checks") && scan.peek() == Some(b'[') {
            let length = scan.array_len()?;
            match key.as_str() {
                "passed" => measure.passed_len = Some(length),
                "checks" => measure.checks_len = Some(length),
                _ => {}
            }
        } else {
            scan.skip_value()?;
        }
        scan.skip_ws();
        if scan.eat(b',') {
            continue;
        }
        if scan.eat(b'}') {
            return Some((measure.finish(), scan.i));
        }
        return None;
    }
}

fn last_measure(text: &str) -> Counted {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut last = Counted::Absent;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            if let Some((counted, end)) = measure_object(text, index) {
                if !matches!(counted, Counted::Absent) {
                    last = counted;
                }
                index = end;
                continue;
            }
        }
        index += 1;
    }
    last
}

fn printed_line_count(text: &str) -> Counted {
    let mut found = Counted::Absent;
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_suffix("checks passed") else { continue };
        let Some((number, words)) = rest.trim_end().split_once(' ') else { continue };
        if words.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(value) = number.parse::<usize>() else { continue };
        match found {
            Counted::Absent => found = Counted::Value(value),
            Counted::Value(existing) if existing == value => {}
            _ => return Counted::Conflict,
        }
    }
    found
}

fn quoted_json_names(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut names = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let quote = bytes[index];
        if quote != b'\'' && quote != b'"' {
            index += 1;
            continue;
        }
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index] != quote && bytes[index] != b'\n' {
            if bytes[index] == b'\\' {
                index += 1;
                if index < bytes.len() {
                    index += 1;
                }
                continue;
            }
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != quote {
            continue;
        }
        let raw = &source[start..index];
        index += 1;
        let Some(name) = raw.rsplit(['/', '\\']).next() else { continue };
        if name.ends_with(".json") && !name.is_empty() && !name.contains('{') && !name.contains(' ') && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    }
    names
}

fn file_is_fresh(path: &Path, started: SystemTime) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|meta| meta.modified()) else {
        return false;
    };
    match modified.duration_since(started) {
        Ok(_) => true,
        Err(_) => started.duration_since(modified).map(|age| age <= std::time::Duration::from_secs(2)).unwrap_or(false),
    }
}

fn same_count(found: &mut Counted, value: usize) -> bool {
    match *found {
        Counted::Absent => {
            *found = Counted::Value(value);
            true
        }
        Counted::Value(existing) if existing == value => true,
        _ => false,
    }
}

fn file_measure(root: &Path, source: &str, started: SystemTime) -> Counted {
    let mut found = Counted::Absent;
    for name in quoted_json_names(source) {
        let path = root.join("output/checks").join(name);
        if !file_is_fresh(&path, started) {
            continue;
        }
        match last_measure(&read(&path)) {
            Counted::Absent => {}
            Counted::Conflict => return Counted::Conflict,
            Counted::Value(value) => {
                if !same_count(&mut found, value) {
                    return Counted::Conflict;
                }
            }
        }
    }
    found
}

fn combine_counts(stdout: Counted, file: Counted, line: Counted) -> (Option<usize>, &'static str) {
    let mut value = None;
    let mut labels = Vec::new();
    for (label, counted) in [("stdout", stdout), ("file", file), ("stdout-line", line)] {
        match counted {
            Counted::Absent => {}
            Counted::Conflict => return (None, "conflict"),
            Counted::Value(number) => {
                if let Some(existing) = value {
                    if existing != number {
                        return (None, "conflict");
                    }
                } else {
                    value = Some(number);
                }
                labels.push(label);
            }
        }
    }
    let source = match labels.as_slice() {
        [] => "unparsed",
        ["stdout"] => "stdout",
        ["file"] => "file",
        ["stdout-line"] => "stdout-line",
        ["stdout", "file"] | ["stdout", "file", "stdout-line"] => "stdout+file",
        ["file", "stdout-line"] => "stdout-line+file",
        _ => "stdout+file",
    };
    (value, source)
}

fn harvest(root: &Path, source: &str, text: &str, started: SystemTime) -> (Option<usize>, &'static str) {
    combine_counts(last_measure(text), file_measure(root, source, started), printed_line_count(text))
}

struct Row {
    name: &'static str,
    verdict: &'static str,
    code: Option<i32>,
    seconds: u64,
    missing: Vec<String>,
    unexercised: Vec<String>,
    checks: Option<usize>,
    count_source: &'static str,
}

fn row_json(row: &Row) -> String {
    let checks = match row.checks {
        Some(value) => value.to_string(),
        None => "null".into(),
    };
    let missing = row.missing.iter().map(|item| format!("\"{}\"", json_escape(item))).collect::<Vec<_>>().join(",");
    if row.verdict == "unavailable" {
        return format!(
            "    {{\"name\":\"{}\",\"verdict\":\"unavailable\",\"code\":null,\"seconds\":0,\"missing\":[{missing}],\"checks\":{checks},\"count_source\":\"{}\"}}",
            row.name, row.count_source
        );
    }
    let flags = row.unexercised.iter().map(|flag| format!("\"{flag}\"")).collect::<Vec<_>>().join(",");
    format!(
        "    {{\"name\":\"{}\",\"verdict\":\"{}\",\"code\":{},\"seconds\":{},\"missing\":[{missing}],\"unexercised\":[{flags}],\"checks\":{checks},\"count_source\":\"{}\"}}",
        row.name,
        row.verdict,
        row.code.unwrap_or(-1),
        row.seconds,
        row.count_source
    )
}

// The delivered record keeps every existing comparator. Only the connected
// census is replaced, and only after the same reader that enforces inventory
// drift accepts the new counts object.
fn publish_record(root: &Path, rows: &[Row]) -> bool {
    let path = root.join("verification/RESULTS.json");
    let original = read(&path);
    if original.is_empty() {
        eprintln!("delivered record left untouched: verification/RESULTS.json is empty");
        return false;
    }
    let mut counts = String::new();
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut unavailable = String::new();
    let mut zero_names = Vec::new();
    for name in GROUPS {
        let Some(row) = rows.iter().find(|row| row.name == *name) else {
            eprintln!("delivered record left untouched: group {name} has no row");
            return false;
        };
        let value = if row.verdict == "unavailable" {
            zero_names.push(*name);
            0
        } else if let Some(value) = row.checks {
            if row.verdict == "pass" {
                passed += 1;
            }
            value
        } else {
            eprintln!("delivered record left untouched: {name} has no harvested count");
            return false;
        };
        total += value;
        if !counts.is_empty() {
            counts.push_str(",\n");
        }
        counts.push_str(&format!("      \"{name}\": {value}"));
        if row.verdict == "unavailable" {
            if !unavailable.is_empty() {
                unavailable.push_str(",\n");
            }
            let reasons = row.missing.iter().map(|item| format!("          \"{}\"", json_escape(item))).collect::<Vec<_>>().join(",\n");
            unavailable.push_str(&format!(
                "      {{\n        \"name\": \"{name}\",\n        \"missing\": [\n{reasons}\n        ],\n        \"recorded_as\": 0\n      }}"
            ));
        }
    }
    let disclosure = if zero_names.len() == 1 {
        format!(
            "{} is recorded as 0 because that group was not executed. A 0 is not a pass and not a harvested check total.",
            zero_names[0]
        )
    } else {
        format!(
            "{} are recorded as 0 because those groups were not executed. A 0 is not a pass and not a harvested check total.",
            zero_names.join(", ")
        )
    };
    let body = format!(
        "{{\n    \"counts\": {{\n{counts}\n    }},\n    \"total_executed\": {total},\n    \"groups_passed\": {passed},\n    \"groups\": {},\n    \"unavailable\": [\n{unavailable}\n    ],\n    \"zero_count_disclosure\": \"{}\"\n  }}",
        GROUPS.len(),
        json_escape(&disclosure)
    );
    let Some(updated) = replace_connected(&original, &body) else {
        eprintln!("delivered record left untouched: connected object was not found");
        return false;
    };
    if !updated.contains("\"published_comparator\"") || !updated.contains("\"flow_comparator\"") || !updated.contains("\"authentication_comparator\"") {
        eprintln!("delivered record left untouched: a comparator field would have been dropped");
        return false;
    }
    let Some((groups, recorded_total)) = counts_in(&updated) else {
        eprintln!("delivered record left untouched: the new counts object is not readable");
        return false;
    };
    if groups != GROUPS.len() || recorded_total != total {
        eprintln!("delivered record left untouched: writer produced {groups} groups and {recorded_total} checks");
        return false;
    }
    if std::fs::write(&path, &updated).is_err() {
        eprintln!("delivered record left untouched: verification/RESULTS.json could not be written");
        return false;
    }
    println!("delivered record written: {groups} groups, {recorded_total} checks");
    true
}

fn replace_connected(text: &str, replacement: &str) -> Option<String> {
    let key = text.find("\"connected\"")?;
    let after = key + "\"connected\"".len();
    let open = after + text[after..].find('{')?;
    let mut scan = Scan { text: text.as_bytes(), i: open };
    scan.skip_object()?;
    let mut updated = String::with_capacity(text.len() + replacement.len());
    updated.push_str(&text[..open]);
    updated.push_str(replacement);
    updated.push_str(&text[scan.i..]);
    Some(updated)
}

// Child output goes straight to the log file so a chatty group cannot fill a
// pipe buffer and block while the runner waits for its deadline.
fn run(root: &Path, name: &str, arguments: &[String]) -> (i32, String, u64) {
    let started = Instant::now();
    let log = root.join(format!("output/checks/{name}.log"));
    let file = match std::fs::File::create(&log) {
        Ok(file) => file,
        Err(error) => return (-1, format!("runner could not write the log: {error}"), 0),
    };
    let spawned = Command::new("python3")
        .arg(root.join(format!("verification/{name}.py")))
        .args(arguments)
        .current_dir(root)
        .stdout(Stdio::from(file.try_clone().expect("log handle")))
        .stderr(Stdio::from(file))
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return (-1, format!("runner could not start the group: {error}"), 0),
    };
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {
                if started.elapsed().as_secs() >= SECONDS {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break 124;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("  {name}: runner could not wait for the group: {error}");
                break -1;
            }
        }
    };
    let elapsed = started.elapsed().as_secs();
    let mut text = read(&log);
    if timed_out {
        text.push_str(&format!("\nDevelopment check exceeded {SECONDS} seconds.\n"));
    }
    (status, text, elapsed)
}

fn main() {
    let root = PathBuf::from(std::env::args().next().unwrap_or_default())
        .parent()
        .and_then(|value| value.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let root = std::env::current_dir().unwrap_or(root);
    let only: Vec<String> = std::env::args().skip(1).collect();
    let output = root.join("output/checks");
    let _ = std::fs::create_dir_all(&output);
    let controller = root.join("output/headless/RLLM");

    let problems = census(&root);
    for problem in &problems {
        eprintln!("CENSUS: {problem}");
    }
    let recorded_counts = recorded(&root);

    let mut rows = Vec::new();
    let mut passed = 0;
    let mut failed = 0;
    let mut unavailable = 0;
    for name in GROUPS {
        if !only.is_empty() && !only.iter().any(|wanted| wanted == name) {
            continue;
        }
        let script = root.join(format!("verification/{name}.py"));
        let source = read(&script);
        let missing: Vec<Input> = required_inputs(&root, &source)
            .into_iter()
            .filter(|input| !input.present)
            .collect();
        let (arguments, unexercised) = group_arguments(&root, &source);
        let mut blocked = missing;
        if !controller.exists() {
            blocked.push(Input {
                kind: "path",
                detail: "output/headless/RLLM (bash headless.sh)".into(),
                present: false,
            });
        }
        if !blocked.is_empty() {
            unavailable += 1;
            let list = blocked.iter().map(|input| format!("{} {}", input.kind, input.detail)).collect::<Vec<_>>();
            println!("{name} UNAVAILABLE (not executed, missing: {})", list.join("; "));
            rows.push(Row {
                name,
                verdict: "unavailable",
                code: None,
                seconds: 0,
                missing: blocked.iter().map(|input| input.detail.clone()).collect(),
                unexercised: Vec::new(),
                checks: None,
                count_source: "not-executed",
            });
            continue;
        }
        let started_at = SystemTime::now();
        let (code, text, elapsed) = run(&root, name, &arguments);
        std::fs::write(output.join(format!("{name}.log")), &text).ok();
        let (checks, count_source) = harvest(&root, &source, &text, started_at);
        let verdict = if code == 0 { "pass" } else { "fail" };
        if code == 0 {
            passed += 1;
        } else {
            failed += 1;
        }
        let shown = match checks {
            Some(count) => count.to_string(),
            None => "unparsed".into(),
        };
        println!("{name} {} (exit {code}, {elapsed}s, {shown} checks)", verdict.to_uppercase());
        if !unexercised.is_empty() {
            println!("  {name}: comparison leg not exercised: {}", unexercised.join(", "));
        }
        if code != 0 {
            let tail: String = text.lines().rev().take(4).collect::<Vec<_>>().iter().rev().cloned().collect::<Vec<_>>().join(" | ");
            eprintln!("  {name}: {}", json_escape(&tail).replace("\\\"", "\""));
        }
        rows.push(Row {
            name,
            verdict,
            code: Some(code),
            seconds: elapsed,
            missing: Vec::new(),
            unexercised,
            checks,
            count_source,
        });
    }

    let unparsed: Vec<&str> = rows.iter().filter(|row| row.verdict != "unavailable" && row.checks.is_none()).map(|row| row.name).collect();
    let ledger = format!(
        "{{\n  \"groups\":{},\n  \"passed\":{},\n  \"failed\":{},\n  \"unavailable\":{},\n  \"optional\":[{}],\n  \"census\":[{}],\n  \"recorded\":{},\n  \"groups_executed\":{},\n  \"results\":[\n{}\n  ]\n}}\n",
        GROUPS.len(),
        passed,
        failed,
        unavailable,
        OPTIONAL.iter().map(|(name, why)| format!("{{\"name\":\"{name}\",\"reason\":\"{}\"}}", json_escape(why))).collect::<Vec<_>>().join(","),
        problems.iter().map(|problem| format!("\"{}\"", json_escape(problem))).collect::<Vec<_>>().join(","),
        match recorded_counts {
            Some((groups, total)) => format!("{{\"groups\":{groups},\"checks\":{total}}}"),
            None => "null".into(),
        },
        passed + failed,
        rows.iter().map(row_json).collect::<Vec<_>>().join(",\n")
    );
    std::fs::write(output.join("harness-ledger.json"), &ledger).ok();
    if only.is_empty() && problems.is_empty() && failed == 0 && unparsed.is_empty() {
        publish_record(&root, &rows);
    }
    println!("summary: {passed} pass, {failed} fail, {unavailable} unavailable of {} groups", GROUPS.len());
    if let Some((groups, total)) = recorded_counts {
        println!("recorded in verification/RESULTS.json: {groups} groups, {total} checks");
        if groups != GROUPS.len() {
            println!("INVENTORY DRIFT: the delivered report counts {groups} groups while the runner has {}", GROUPS.len());
        }
    }
    for name in &unparsed {
        eprintln!("COUNT UNPARSED: {name}");
    }
    if !problems.is_empty() || failed > 0 || !unparsed.is_empty() {
        std::process::exit(1);
    }
}

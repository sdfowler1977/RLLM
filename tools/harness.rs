// Connected-check runner.
// `harness sweep` is native orchestration and record writing over the Python
// group scripts. It runs a group only when that group's development inputs are
// present, reads each group's JSON, and writes the connected object plus one
// checks row per group in verification/RESULTS.json. A missing development
// input is unavailable and is not charged as a product failure. A product
// failure leaves that record untouched. The bare run and `harness <group>`
// do not write the record.
use std::collections::BTreeSet;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
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
    "turn_background", "permission_modes", "self_rebuild", "rrsi_loop",
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

    // A leading '-' is a signed integer here. Fractional and exponent forms stay
    // Some(None) so the caller can reject them as non-integers.
    fn take_executed_int(&mut self) -> Option<Option<i64>> {
        let start = self.i;
        let negative = self.peek() == Some(b'-');
        match self.take_json_int() {
            Some(Some(value)) => Some(Some(value as i64)),
            Some(None) if negative => {
                let token = std::str::from_utf8(self.text.get(start..self.i)?).ok()?;
                match token.parse::<i64>() {
                    Ok(value) => Some(Some(value)),
                    Err(_) => Some(None),
                }
            }
            Some(None) => Some(None),
            None => None,
        }
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

// The report file package reads for each group. Four names are not the
// hyphenated group name, and the identity groups keep their underscores.
fn group_report_file(name: &str) -> String {
    let stem = match name {
        "provider_process" => "current-results".to_string(),
        "network_drive" => "network-results".to_string(),
        "question_process" => "process-results".to_string(),
        "history" => "history-results".to_string(),
        "desktop_runtime" | "turn_background" | "permission_modes" | "services" | "media" | "jobs" | "voice"
        | "media_stream" | "media_upload" | "voice_interruption" | "voice_background" | "voice_session"
        | "capture_lifecycle" | "scene" | "apk_alignment" | "audio_route" | "document_providers"
        | "snapshot_identity" | "storage_auth" | "storage_oauth" | "credential_rotation" | "voice_settings"
        | "acp" | "oauth" | "device_authorization" => name.to_string(),
        other => other.replace('_', "-"),
    };
    format!("{stem}.json")
}

struct CountedJson {
    count: usize,
    passed_int: bool,
    saw_checks: bool,
}

// Same count the publisher uses: `passed` when that key is present (list length
// or integer), otherwise `count`. A `checks` list and a `count` field have to
// agree. Zero, negatives, and non-integers are rejected so they cannot be
// stored as a pass.
fn executed_count_of(text: &str) -> Result<CountedJson, &'static str> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while start < bytes.len() && matches!(bytes[start], b' ' | b'\n' | b'\r' | b'\t') {
        start += 1;
    }
    if bytes.get(start) != Some(&b'{') {
        return Err("group json is not an object");
    }
    let mut scan = Scan { text: bytes, i: start };
    if !scan.eat(b'{') {
        return Err("group json is not an object");
    }
    let mut passed_int: Option<i64> = None;
    let mut passed_len = None;
    let mut passed_bad = false;
    let mut count_int: Option<i64> = None;
    let mut count_bad = false;
    let mut checks_len = None;
    let mut checks_bad = false;
    let mut saw_passed = false;
    let mut saw_count = false;
    let mut saw_checks = false;
    loop {
        scan.skip_ws();
        if scan.eat(b'}') {
            break;
        }
        let key = scan.key().ok_or("group json")?;
        scan.skip_ws();
        if !scan.eat(b':') {
            return Err("group json");
        }
        scan.skip_ws();
        match key.as_str() {
            "passed" => {
                saw_passed = true;
                if scan.starts_number() {
                    match scan.take_executed_int() {
                        Some(Some(value)) => passed_int = Some(value),
                        _ => passed_bad = true,
                    }
                } else if scan.peek() == Some(b'[') {
                    passed_len = Some(scan.array_len().ok_or("group json")?);
                } else {
                    passed_bad = true;
                    scan.skip_value().ok_or("group json")?;
                }
            }
            "count" => {
                saw_count = true;
                if scan.starts_number() {
                    match scan.take_executed_int() {
                        Some(Some(value)) => count_int = Some(value),
                        _ => count_bad = true,
                    }
                } else {
                    count_bad = true;
                    scan.skip_value().ok_or("group json")?;
                }
            }
            "checks" => {
                saw_checks = true;
                if scan.peek() == Some(b'[') {
                    checks_len = Some(scan.array_len().ok_or("group json")?);
                } else {
                    checks_bad = true;
                    scan.skip_value().ok_or("group json")?;
                }
            }
            _ => {
                scan.skip_value().ok_or("group json")?;
            }
        }
        scan.skip_ws();
        if scan.eat(b',') {
            continue;
        }
        if scan.eat(b'}') {
            break;
        }
        return Err("group json");
    }
    if passed_bad || count_bad {
        return Err("executed count is not an integer");
    }
    if checks_bad {
        return Err("checks cross-check");
    }
    let (count, passed_is_int) = if saw_passed {
        match (passed_len, passed_int) {
            (Some(len), None) => (len as i64, false),
            (None, Some(value)) => (value, true),
            _ => return Err("executed count is not an integer"),
        }
    } else if let Some(value) = count_int {
        (value, false)
    } else if saw_count {
        return Err("executed count is not an integer");
    } else {
        return Err("executed count is missing");
    };
    if count <= 0 {
        return Err("executed count is not a positive integer");
    }
    let count = count as usize;
    if saw_checks {
        let len = checks_len.ok_or("checks cross-check")?;
        if len != count {
            return Err("checks cross-check");
        }
    }
    if saw_passed && saw_count {
        let declared = count_int.ok_or("executed count is not an integer")?;
        if declared != count as i64 {
            return Err("executed count differs from count");
        }
    }
    Ok(CountedJson { count, passed_int: passed_is_int, saw_checks })
}

fn sweep_usage() -> &'static str {
    "usage: harness sweep [--previous-scene-controller PATH] [--previous-controller PATH] [--previous-flow-controller PATH] [--previous-auth-controller PATH] [--auth-baseline PATH] [--auth-baseline-sha256 HEX]\n       harness [group ...]"
}

struct SweepOpts {
    previous_scene: Option<PathBuf>,
    previous_controller: Option<PathBuf>,
    previous_flow: Option<PathBuf>,
    previous_auth: Option<PathBuf>,
    auth_baseline: Option<PathBuf>,
    auth_baseline_sha256: Option<String>,
}

fn parse_sweep(args: &[String]) -> Result<SweepOpts, String> {
    let mut opts = SweepOpts {
        previous_scene: None,
        previous_controller: None,
        previous_flow: None,
        previous_auth: None,
        auth_baseline: None,
        auth_baseline_sha256: None,
    };
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        let Some(value) = args.get(index + 1) else {
            return Err(format!("missing value for {flag}"));
        };
        if value.starts_with("--") {
            return Err(format!("missing value for {flag}"));
        }
        match flag.as_str() {
            "--previous-scene-controller" => opts.previous_scene = Some(PathBuf::from(value)),
            "--previous-controller" => opts.previous_controller = Some(PathBuf::from(value)),
            "--previous-flow-controller" => opts.previous_flow = Some(PathBuf::from(value)),
            "--previous-auth-controller" => opts.previous_auth = Some(PathBuf::from(value)),
            "--auth-baseline" => opts.auth_baseline = Some(PathBuf::from(value)),
            "--auth-baseline-sha256" => opts.auth_baseline_sha256 = Some(value.clone()),
            other => return Err(format!("unknown sweep argument {other}")),
        }
        index += 2;
    }
    Ok(opts)
}

fn resolved_path(path: &Path) -> String {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical.display().to_string();
    }
    if path.is_absolute() {
        return path.display().to_string();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

// Argument rules copied from the no-flag path in tools/checks.py.
fn sweep_arguments(name: &str, root: &Path, opts: &SweepOpts) -> Vec<String> {
    let mut arguments = Vec::new();
    if name == "scene" {
        if let Some(path) = &opts.previous_scene {
            arguments.push("--previous-scene-controller".into());
            arguments.push(resolved_path(path));
        }
    }
    let controller = opts.previous_controller.clone().unwrap_or_else(|| root.join("output/previous-controller"));
    if name == "history_migration" || (name == "prompt_reference" && controller.is_file()) {
        arguments.push("--previous-controller".into());
        arguments.push(resolved_path(&controller));
    }
    if name == "binary_flow" {
        let flow = opts.previous_flow.clone().unwrap_or_else(|| root.join("output/pre-branch-controller"));
        let auth = opts.previous_auth.clone().unwrap_or_else(|| root.join("output/pre-auth-controller"));
        arguments.push("--previous-flow-controller".into());
        arguments.push(resolved_path(&flow));
        arguments.push("--previous-auth-controller".into());
        arguments.push(resolved_path(&auth));
        if let Some(path) = &opts.auth_baseline {
            arguments.push("--auth-baseline".into());
            arguments.push(resolved_path(path));
        }
        if let Some(digest) = &opts.auth_baseline_sha256 {
            arguments.push("--auth-baseline-sha256".into());
            arguments.push(digest.clone());
        }
    }
    arguments
}

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
}

const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const O_NONBLOCK: i32 = 2048;
const POLLIN: i16 = 1;

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

fn python_group(root: &Path, name: &str, arguments: &[String]) -> Command {
    let mut command = Command::new("python3");
    command
        .arg(root.join(format!("verification/{name}.py")))
        .args(arguments)
        .current_dir(root)
        .process_group(0);
    command
}

// Signal the child's process group. process_group(0) made that group id the
// child's pid, so grandchildren that inherited it die with the deadline.
fn kill_child_group(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    unsafe {
        kill(-pid, 9);
    }
    let _ = child.kill();
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) if Instant::now() >= deadline => break,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
}

fn set_nonblocking(fd: i32) {
    let flags = unsafe { fcntl(fd, F_GETFL, 0) };
    if flags >= 0 {
        unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK); }
    }
}

// Returns true while some writer still holds the pipe. Bytes already read stay
// in `buf`. The fd is nonblocking, so this returns at WouldBlock instead of
// waiting for EOF.
fn pipe_still_open(pipe: &mut impl Read, buf: &mut Vec<u8>, chunk: &mut [u8]) -> bool {
    loop {
        match pipe.read(chunk) {
            Ok(0) => return false,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return true,
            Err(_) => return false,
        }
    }
}

fn poll_wait_ms(deadline: Instant) -> Option<i32> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let millis = remaining.as_millis();
    if millis == 0 {
        return None;
    }
    Some(millis.min(200) as i32)
}

// The parent reads both pipes. There is no reader thread and no join. A setsid
// grandchild is outside kill(-pid) and can hold the write end; nonblocking
// poll stops at the drain deadline and the bytes already read are kept.
fn sweep_run(root: &Path, name: &str, arguments: &[String], seconds: u64) -> (i32, Vec<u8>, u64) {
    let started = Instant::now();
    let drain_deadline = started + std::time::Duration::from_secs(seconds.saturating_add(5));
    let spawned = python_group(root, name, arguments).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return (-1, format!("runner could not start the group: {error}").into_bytes(), 0),
    };
    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");
    set_nonblocking(stdout.as_raw_fd());
    set_nonblocking(stderr.as_raw_fd());
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut chunk = [0u8; 8192];
    let mut timed_out = false;
    let mut status = None;
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(finished)) => status = Some(finished.code().unwrap_or(-1)),
                Ok(None) if started.elapsed().as_secs() >= seconds => {
                    kill_child_group(&mut child);
                    timed_out = true;
                    status = Some(124);
                }
                Ok(None) => {}
                Err(error) => {
                    kill_child_group(&mut child);
                    let elapsed = started.elapsed().as_secs();
                    return (-1, format!("runner could not wait for the group: {error}").into_bytes(), elapsed);
                }
            }
        }
        if status.is_some() && !stdout_open && !stderr_open {
            break;
        }
        let Some(timeout_ms) = poll_wait_ms(drain_deadline) else {
            break;
        };
        let mut fds = [
            PollFd { fd: if stdout_open { stdout.as_raw_fd() } else { -1 }, events: POLLIN, revents: 0 },
            PollFd { fd: if stderr_open { stderr.as_raw_fd() } else { -1 }, events: POLLIN, revents: 0 },
        ];
        let ready = unsafe { poll(fds.as_mut_ptr(), fds.len(), timeout_ms) };
        if ready < 0 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        if stdout_open && fds[0].revents != 0 {
            stdout_open = pipe_still_open(&mut stdout, &mut stdout_bytes, &mut chunk);
        }
        if stderr_open && fds[1].revents != 0 {
            stderr_open = pipe_still_open(&mut stderr, &mut stderr_bytes, &mut chunk);
        }
    }
    if stdout_open {
        stdout_open = pipe_still_open(&mut stdout, &mut stdout_bytes, &mut chunk);
    }
    if stderr_open {
        stderr_open = pipe_still_open(&mut stderr, &mut stderr_bytes, &mut chunk);
    }
    let status = status.unwrap_or(-1);
    let mut log = stdout_bytes.clone();
    log.extend_from_slice(&stderr_bytes);
    if timed_out {
        log.extend(format!("\nDevelopment check exceeded {seconds} seconds.\n").into_bytes());
    }
    if stdout_open {
        log.extend(b"abandoned stdout\n");
    }
    if stderr_open {
        log.extend(b"abandoned stderr\n");
    }
    std::fs::write(root.join(format!("output/checks/{name}.log")), &log).ok();
    (status, stdout_bytes, started.elapsed().as_secs())
}

// response_fields and chunk_fields print their JSON and do not write it.
// checks.py persists that object; the sweep stores the printed bytes unchanged.
fn persist_printed_json(root: &Path, name: &str, stdout: &[u8]) -> Result<usize, String> {
    let counted = executed_count_of(&String::from_utf8_lossy(stdout)).map_err(|err| err.to_string())?;
    if !counted.passed_int || !counted.saw_checks {
        return Err("printed json needs an integer passed and a checks list".into());
    }
    let path = root.join("output/checks").join(group_report_file(name));
    std::fs::write(&path, stdout).map_err(|err| format!("could not write {}: {err}", path.display()))?;
    Ok(counted.count)
}

struct SweepRow {
    name: &'static str,
    code: Option<i32>,
    checks: Option<usize>,
    missing: Vec<String>,
    reason: String,
    // Object text the runner read after a spawn. None when the group was not
    // spawned or the bytes were not a JSON object. Never a reason to publish.
    report: Option<String>,
}

// The first JSON object in `bytes`, exactly as the group wrote it. Leading
// whitespace is skipped; check names inside the object are not rewritten.
fn report_object_text(bytes: &[u8]) -> Option<String> {
    let owned = String::from_utf8_lossy(bytes);
    let text = owned.as_ref();
    let raw = text.as_bytes();
    let mut start = 0;
    while start < raw.len() && matches!(raw[start], b' ' | b'\n' | b'\r' | b'\t') {
        start += 1;
    }
    let mut scan = Scan { text: raw, i: start };
    let begin = scan.i;
    scan.skip_object()?;
    Some(text[begin..scan.i].to_string())
}

fn row_failed(row: &SweepRow) -> bool {
    match row.code {
        None => false,
        Some(0) => !matches!(row.checks, Some(count) if count > 0),
        Some(_) => true,
    }
}

fn reason_for(code: i32, count_error: Option<&str>, report: &Path, log: &Path, seconds: u64) -> String {
    let (sentence, path): (String, &Path) = if code == 124 {
        (format!("timed out after {seconds} seconds"), log)
    } else if let Some(error) = count_error {
        let sentence: String = error.lines().next().unwrap_or(error).chars().take(240).collect();
        (sentence, report)
    } else {
        (format!("exited {code}"), log)
    };
    format!("{sentence} ({})", path.display())
}

fn unavailable_line(name: &str, missing: &[Input]) -> String {
    let list = missing.iter().map(|input| format!("{} {}", input.kind, input.detail)).collect::<Vec<_>>().join("; ");
    format!("{name} UNAVAILABLE (not executed, missing: {list})")
}

// Shape is the runner's own fact: spawned or not, the exit code, and whether
// the report bytes parsed as a positive executed count. A field the group
// wrote cannot choose it. A stale report left by a group that was not spawned,
// and a passing-looking report left by a non-zero exit, stay out of `checks`.
fn checks_shape(row: &SweepRow) -> &'static str {
    let spawned = row.code.is_some();
    if !spawned {
        return "unavailable";
    }
    let succeeded = row.code == Some(0);
    let report_counts = row.report.as_deref().is_some_and(|text| executed_count_of(text).is_ok());
    if succeeded && report_counts {
        return "executed";
    }
    "failed"
}

fn unavailable_reason(name: &str, missing: &[String]) -> String {
    if missing.len() == 1 {
        format!("{name} was not executed because a development input is missing: {}.", missing[0])
    } else if missing.is_empty() {
        format!("{name} was not executed because a development input is missing.")
    } else {
        format!("{name} was not executed because development inputs are missing: {}.", missing.join("; "))
    }
}

fn json_string_array(items: &[String]) -> String {
    items.iter().map(|item| format!("\"{}\"", json_escape(item))).collect::<Vec<_>>().join(", ")
}

fn unavailable_checks(row: &SweepRow) -> String {
    let reason = if row.reason.is_empty() { unavailable_reason(row.name, &row.missing) } else { row.reason.clone() };
    format!(
        "{{\n      \"name\": \"{}\",\n      \"code\": null,\n      \"missing\": [{}],\n      \"recorded_as\": 0,\n      \"reason\": \"{}\"\n    }}",
        json_escape(row.name),
        json_string_array(&row.missing),
        json_escape(&reason)
    )
}

fn failed_checks(row: &SweepRow) -> String {
    let code = row.code.unwrap_or(-1);
    let reason = if !row.reason.is_empty() {
        row.reason.clone()
    } else if row.report.is_none() {
        format!("The group report could not be read after exit {code}.")
    } else {
        format!("The group failed with exit {code}.")
    };
    format!(
        "{{\n      \"name\": \"{}\",\n      \"code\": {code},\n      \"reason\": \"{}\",\n      \"recorded_as\": 0\n    }}",
        json_escape(row.name),
        json_escape(&reason)
    )
}

fn checks_member(row: &SweepRow) -> String {
    match checks_shape(row) {
        "unavailable" => unavailable_checks(row),
        "executed" => row.report.clone().unwrap_or_else(|| failed_checks(row)),
        _ => failed_checks(row),
    }
}

fn checks_document(rows: &[SweepRow]) -> Result<String, String> {
    let mut body = String::from("{\n");
    for (index, name) in GROUPS.iter().enumerate() {
        let row = rows.iter().find(|row| row.name == *name).ok_or_else(|| format!("group {name} has no row"))?;
        if index > 0 {
            body.push_str(",\n");
        }
        body.push_str("    \"");
        body.push_str(name);
        body.push_str("\": ");
        body.push_str(&checks_member(row));
    }
    body.push_str("\n  }");
    let count = object_key_count(&body).ok_or("checks object is not readable")?;
    if count != GROUPS.len() {
        return Err(format!("checks object has {count} groups"));
    }
    Ok(body)
}

fn object_key_count(object: &str) -> Option<usize> {
    let mut scan = Scan { text: object.as_bytes(), i: 0 };
    if !scan.eat(b'{') {
        return None;
    }
    let mut count = 0;
    loop {
        scan.skip_ws();
        if scan.eat(b'}') {
            return Some(count);
        }
        scan.skip_string()?;
        count += 1;
        scan.skip_ws();
        if !scan.eat(b':') {
            return None;
        }
        scan.skip_value()?;
        scan.skip_ws();
        if scan.eat(b',') {
            continue;
        }
        if scan.eat(b'}') {
            return Some(count);
        }
        return None;
    }
}

// The first `"key": { ... }` object. A later object with the same key, inside
// a group report, is not the top-level value.
fn find_object_value(text: &str, key: &str) -> Option<(usize, usize)> {
    let needle = format!("\"{key}\"");
    let bytes = text.as_bytes();
    let mut search = 0;
    while let Some(rel) = text[search..].find(&needle) {
        let at = search + rel;
        let after = at + needle.len();
        let mut scan = Scan { text: bytes, i: after };
        scan.skip_ws();
        if !scan.eat(b':') {
            search = after;
            continue;
        }
        scan.skip_ws();
        if scan.peek() != Some(b'{') {
            search = after;
            continue;
        }
        let begin = scan.i;
        scan.skip_object()?;
        return Some((begin, scan.i));
    }
    None
}

fn replace_json_object(text: &str, key: &str, replacement: &str) -> Option<String> {
    let (begin, end) = find_object_value(text, key)?;
    let mut updated = String::with_capacity(text.len() + replacement.len());
    updated.push_str(&text[..begin]);
    updated.push_str(replacement);
    updated.push_str(&text[end..]);
    Some(updated)
}

fn insert_after_object(text: &str, key: &str, inserted: &str) -> Option<String> {
    let (_, end) = find_object_value(text, key)?;
    let mut updated = String::with_capacity(text.len() + inserted.len());
    updated.push_str(&text[..end]);
    updated.push_str(inserted);
    updated.push_str(&text[end..]);
    Some(updated)
}

fn unavailable_object(row: &SweepRow) -> String {
    match row.code {
        None => {
            let missing = row.missing.iter().map(|item| format!("\"{}\"", json_escape(item))).collect::<Vec<_>>().join(", ");
            format!(
                "      {{\n        \"name\": \"{}\",\n        \"code\": null,\n        \"missing\": [{missing}],\n        \"recorded_as\": 0\n      }}",
                json_escape(row.name)
            )
        }
        Some(code) => format!(
            "      {{\n        \"name\": \"{}\",\n        \"code\": {code},\n        \"reason\": \"{}\"\n      }}",
            json_escape(row.name),
            json_escape(&row.reason)
        ),
    }
}

// Replaces the connected object and the checks object. The serialized text must still
// contain the names published_comparator, flow_comparator and authentication_comparator;
// selected_rust_* fields stay because this writer never touches them, not because a
// guard checks them. On the commit_sweep route, a row that row_failed classifies as
// failed causes an early return, so publish_sweep is not reached with that row; a test
// may call publish_sweep directly with a failing row.
fn publish_sweep(root: &Path, rows: &[SweepRow]) -> Result<(), String> {
    let path = root.join("verification/RESULTS.json");
    let original = read(&path);
    if original.is_empty() {
        return Err("verification/RESULTS.json is empty".into());
    }
    let mut counts = String::new();
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut unavailable = String::new();
    let mut zero_names = Vec::new();
    for name in GROUPS {
        let row = rows.iter().find(|row| row.name == *name).ok_or_else(|| format!("group {name} has no row"))?;
        let value = if row.code == Some(0) { row.checks.unwrap_or(0) } else { 0 };
        if value == 0 {
            zero_names.push(*name);
        }
        if row.code == Some(0) && value > 0 {
            passed += 1;
        }
        total += value;
        if !counts.is_empty() {
            counts.push_str(",\n");
        }
        counts.push_str(&format!("      \"{name}\": {value}"));
        if row.code != Some(0) {
            if !unavailable.is_empty() {
                unavailable.push_str(",\n");
            }
            unavailable.push_str(&unavailable_object(row));
        }
    }
    let unavailable_field = if unavailable.is_empty() {
        "[]".to_string()
    } else {
        format!("[\n{unavailable}\n    ]")
    };
    let disclosure = if zero_names.is_empty() {
        ",\n    \"zero_count_disclosure\": \"No group was recorded as 0.\"".to_string()
    } else if zero_names.len() == 1 {
        format!(
            ",\n    \"zero_count_disclosure\": \"{} is recorded as 0 because that group produced no positive check count. A 0 is not a pass and not a harvested check total.\"",
            json_escape(zero_names[0])
        )
    } else {
        format!(
            ",\n    \"zero_count_disclosure\": \"{} are recorded as 0 because those groups produced no positive check count. A 0 is not a pass and not a harvested check total.\"",
            json_escape(&zero_names.join(", "))
        )
    };
    let body = format!(
        "{{\n    \"counts\": {{\n{counts}\n    }},\n    \"total_executed\": {total},\n    \"groups_passed\": {passed},\n    \"groups\": {},\n    \"unavailable\": {unavailable_field}{disclosure}\n  }}",
        GROUPS.len()
    );
    let Some(mut updated) = replace_connected(&original, &body) else {
        return Err("connected object was not found".into());
    };
    let checks = checks_document(rows)?;
    updated = match replace_json_object(&updated, "checks", &checks) {
        Some(text) => text,
        None => {
            let member = format!(",\n  \"checks\": {checks}");
            insert_after_object(&updated, "connected", &member).ok_or("checks object was not written")?
        }
    };
    if !updated.contains("\"published_comparator\"") || !updated.contains("\"flow_comparator\"") || !updated.contains("\"authentication_comparator\"") {
        return Err("a comparator field would have been dropped".into());
    }
    let Some((groups, recorded_total)) = counts_in(&updated) else {
        return Err("the new counts object is not readable".into());
    };
    if groups != GROUPS.len() || recorded_total != total {
        return Err(format!("writer produced {groups} groups and {recorded_total} checks"));
    }
    let temporary = path.with_extension("json.new");
    if std::fs::write(&temporary, &updated).is_err() {
        return Err("verification/RESULTS.json could not be written".into());
    }
    if std::fs::rename(&temporary, &path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err("verification/RESULTS.json could not be written".into());
    }
    println!("delivered record written: {groups} groups, {recorded_total} checks");
    Ok(())
}

// Failures block publication. Unavailable groups do not.
fn commit_sweep(root: &Path, rows: &[SweepRow]) -> bool {
    if rows.iter().any(row_failed) {
        eprintln!("delivered record left untouched: a group failed");
        return false;
    }
    match publish_sweep(root, rows) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("delivered record left untouched: {error}");
            false
        }
    }
}

fn project_root() -> PathBuf {
    let root = PathBuf::from(std::env::args().next().unwrap_or_default())
        .parent()
        .and_then(|value| value.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    std::env::current_dir().unwrap_or(root)
}

fn sweep_one(root: &Path, name: &'static str, opts: &SweepOpts, seconds: u64) -> SweepRow {
    let source = read(&root.join(format!("verification/{name}.py")));
    let missing: Vec<Input> = required_inputs(root, &source).into_iter().filter(|input| !input.present).collect();
    if !missing.is_empty() {
        println!("{}", unavailable_line(name, &missing));
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let details: Vec<String> = missing.iter().map(|input| input.detail.clone()).collect();
        let reason = unavailable_reason(name, &details);
        return SweepRow {
            name,
            code: None,
            checks: Some(0),
            missing: details,
            reason,
            report: None,
        };
    }
    let arguments = sweep_arguments(name, root, opts);
    let output = root.join("output/checks");
    let report = output.join(group_report_file(name));
    let log = output.join(format!("{name}.log"));
    let started_at = SystemTime::now();
    let (code, stdout, elapsed) = sweep_run(root, name, &arguments, seconds);
    let printed = name == "response_fields" || name == "chunk_fields";
    let fresh = file_is_fresh(&report, started_at);
    let counted = if printed {
        if code == 0 {
            persist_printed_json(root, name, &stdout)
        } else {
            Err("group did not exit 0, so its printed json was not stored".into())
        }
    } else if fresh {
        executed_count_of(&read(&report)).map(|counted| counted.count).map_err(|err| err.to_string())
    } else {
        Err(format!("no fresh {}", group_report_file(name)))
    };
    // Same bytes the count was taken from. A non-zero exit still keeps them so
    // the checks row can say the group failed instead of copying the report.
    let report_text = if printed {
        if code == 0 { report_object_text(&stdout) } else { None }
    } else if fresh {
        match std::fs::read(&report) {
            Ok(bytes) => report_object_text(&bytes),
            Err(_) => None,
        }
    } else {
        None
    };
    let checks = match &counted {
        Ok(count) => Some(*count),
        Err(error) => {
            eprintln!("  {name}: {error}");
            None
        }
    };
    let failed = code != 0 || checks.is_none();
    let reason = if failed {
        reason_for(code, counted.as_ref().err().map(String::as_str), &report, &log, seconds)
    } else {
        String::new()
    };
    let verdict = if failed { "fail" } else { "pass" };
    let shown = match checks {
        Some(count) => count.to_string(),
        None => "absent".into(),
    };
    println!("{name} {} (exit {code}, {elapsed}s, {shown} checks)", verdict.to_uppercase());
    let _ = std::io::Write::flush(&mut std::io::stdout());
    if code != 0 {
        let text = read(&log);
        let tail: String = text.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
        if !tail.is_empty() {
            eprintln!("  {name}: {tail}");
        }
    }
    SweepRow { name, code: Some(code), checks, missing: Vec::new(), reason, report: report_text }
}

fn run_sweep(args: &[String]) -> Result<i32, String> {
    let opts = parse_sweep(args)?;
    let root = project_root();
    let output = root.join("output/checks");
    if std::fs::create_dir_all(&output).is_err() {
        return Err(format!("could not create {}", output.display()));
    }
    println!("harness sweep: {} groups", GROUPS.len());
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let rows: Vec<SweepRow> = GROUPS.iter().map(|name| sweep_one(&root, name, &opts, SECONDS)).collect();
    let passed = rows.iter().filter(|row| !row_failed(row) && row.code.is_some()).count();
    let failed = rows.iter().filter(|row| row_failed(row)).count();
    let unavailable = rows.iter().filter(|row| row.code.is_none()).count();
    let absent = rows.iter().filter(|row| row.code.is_some() && row.checks.is_none()).count();
    let total: usize = rows.iter().map(|row| if row.code == Some(0) { row.checks.unwrap_or(0) } else { 0 }).sum();
    let wrote = commit_sweep(&root, &rows);
    println!(
        "summary: {passed} pass, {failed} fail, {unavailable} unavailable, {absent} absent of {} groups, {total} checks",
        GROUPS.len()
    );
    if !wrote || failed > 0 || absent > 0 {
        return Ok(1);
    }
    Ok(0)
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
    let spawned = python_group(root, name, arguments)
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
                    kill_child_group(&mut child);
                    timed_out = true;
                    break 124;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(error) => {
                kill_child_group(&mut child);
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("sweep") {
        let code = match run_sweep(&args[1..]) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("{error}");
                eprintln!("{}", sweep_usage());
                2
            }
        };
        std::process::exit(code);
    }
    let root = project_root();
    let only = args;
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

#[cfg(test)]
mod checks {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("rllm-harness-{}-{}", std::process::id(), TEMP.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn bare_opts() -> SweepOpts {
        SweepOpts {
            previous_scene: None,
            previous_controller: None,
            previous_flow: None,
            previous_auth: None,
            auth_baseline: None,
            auth_baseline_sha256: None,
        }
    }

    fn pass_rows() -> Vec<SweepRow> {
        let report = r#"{"passed":["a","b","c"]}"#.to_string();
        GROUPS.iter().map(|name| SweepRow {
            name,
            code: Some(0),
            checks: Some(3),
            missing: Vec::new(),
            reason: String::new(),
            report: Some(report.clone()),
        }).collect()
    }

    fn json_object_after<'a>(text: &'a str, key: &str) -> &'a str {
        let (begin, end) = find_object_value(text, key).unwrap_or_else(|| panic!("no object for {key}"));
        &text[begin..end]
    }

    fn json_field<'a>(object: &'a str, key: &str) -> &'a str {
        let mut scan = Scan { text: object.as_bytes(), i: 0 };
        assert!(scan.eat(b'{'), "field scan start");
        loop {
            scan.skip_ws();
            if scan.eat(b'}') {
                break;
            }
            let found = scan.key().unwrap_or_else(|| panic!("key"));
            scan.skip_ws();
            assert!(scan.eat(b':'), "colon");
            scan.skip_ws();
            let start = scan.i;
            scan.skip_value().unwrap_or_else(|| panic!("value"));
            if found == key {
                return &object[start..scan.i];
            }
            scan.skip_ws();
            if scan.eat(b',') {
                continue;
            }
            if scan.eat(b'}') {
                break;
            }
            panic!("field scan");
        }
        panic!("missing field {key}");
    }

    fn object_keys(object: &str) -> Vec<String> {
        let mut scan = Scan { text: object.as_bytes(), i: 0 };
        assert!(scan.eat(b'{'), "keys start");
        let mut keys = Vec::new();
        loop {
            scan.skip_ws();
            if scan.eat(b'}') {
                break;
            }
            keys.push(scan.key().unwrap_or_else(|| panic!("key")));
            scan.skip_ws();
            assert!(scan.eat(b':'), "colon");
            scan.skip_value().unwrap_or_else(|| panic!("value"));
            scan.skip_ws();
            if scan.eat(b',') {
                continue;
            }
            if scan.eat(b'}') {
                break;
            }
            panic!("keys scan");
        }
        keys
    }

    fn delivered_record() -> String {
        "{\n  \"published_comparator\": {\"note\": \"keep-me\"},\n  \"connected\": {\"counts\": {\"old\": 1}, \"total_executed\": 1, \"groups_passed\": 1, \"groups\": 1, \"unavailable\": []},\n  \"flow_comparator\": {\"note\": \"flow\"},\n  \"authentication_comparator\": {\"note\": \"auth\"}\n}\n".to_string()
    }

    fn plant_record(root: &Path) -> PathBuf {
        std::fs::create_dir_all(root.join("verification")).unwrap();
        let record = root.join("verification/RESULTS.json");
        std::fs::write(&record, delivered_record()).unwrap();
        record
    }

    fn repo_root() -> PathBuf {
        let mut starts = Vec::new();
        if let Ok(path) = std::env::current_exe() {
            starts.push(path);
        }
        if let Ok(path) = std::env::current_dir() {
            starts.push(path);
        }
        for start in starts {
            let mut dir = std::fs::canonicalize(&start).unwrap_or(start);
            loop {
                if dir.join("tools/package.rs").is_file() && dir.join("tools/harness.rs").is_file() {
                    return dir;
                }
                if !dir.pop() {
                    break;
                }
            }
        }
        panic!("cannot locate the RLLM project root");
    }

    fn rust_quoted(source: &str, marker: &str) -> Vec<String> {
        let at = source.find(marker).unwrap_or_else(|| panic!("package.rs has no {marker}"));
        let rest = &source[at..];
        let end = rest.find("];").unwrap_or_else(|| panic!("{marker} does not end"));
        let body = &rest[..end];
        let bytes = body.as_bytes();
        let mut names = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            if bytes[index] == b'"' {
                index += 1;
                let start = index;
                while index < bytes.len() && bytes[index] != b'"' {
                    index += 1;
                }
                names.push(body[start..index].to_string());
                index += 1;
                continue;
            }
            index += 1;
        }
        names
    }

    fn process_running(pid: i32) -> bool {
        if pid <= 0 {
            return false;
        }
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        status.lines().find_map(|line| line.strip_prefix("State:")).map(|rest| rest.trim().starts_with('S') || rest.trim().starts_with('R') || rest.trim().starts_with('D')).unwrap_or(false)
    }

    #[test]
    fn executed_count_rejects_a_negative_count() {
        assert_eq!(executed_count_of(r#"{"passed": -4}"#).err(), Some("executed count is not a positive integer"), "negative count");
    }

    #[test]
    fn executed_count_rejects_zero() {
        assert_eq!(executed_count_of(r#"{"passed": 0}"#).err(), Some("executed count is not a positive integer"), "zero count");
        assert_eq!(executed_count_of(r#"{"count": 0}"#).err(), Some("executed count is not a positive integer"), "zero count field");
    }

    #[test]
    fn executed_count_rejects_passed_and_count_disagreeing() {
        assert_eq!(executed_count_of(r#"{"passed": 2, "count": 3}"#).err(), Some("executed count differs from count"), "passed and count differ");
    }

    #[test]
    fn executed_count_rejects_checks_that_are_not_an_array_or_disagree() {
        assert_eq!(executed_count_of(r#"{"passed": 1, "checks": {"n": 1}}"#).err(), Some("checks cross-check"), "checks present but not an array");
        assert_eq!(executed_count_of(r#"{"passed": 2, "checks": [1]}"#).err(), Some("checks cross-check"), "checks length disagrees");
    }

    #[test]
    fn executed_count_rejects_when_neither_passed_nor_count_is_present() {
        assert_eq!(executed_count_of(r#"{"scope": "only"}"#).err(), Some("executed count is missing"), "neither passed nor count");
    }

    #[test]
    fn executed_count_rejects_a_non_object() {
        assert_eq!(executed_count_of("[1, 2]").err(), Some("group json is not an object"), "top level not an object");
    }

    #[test]
    fn executed_count_rejects_passed_string_or_bool() {
        assert_eq!(executed_count_of(r#"{"passed": "4"}"#).err(), Some("executed count is not an integer"), "passed string");
        assert_eq!(executed_count_of(r#"{"passed": true}"#).err(), Some("executed count is not an integer"), "passed bool");
    }

    #[test]
    fn executed_count_uses_passed_list_length() {
        let counted = executed_count_of(r#"{"passed": ["a", "b", "c"]}"#);
        assert_eq!(counted.as_ref().map(|item| item.count).ok(), Some(3), "passed list length");
        assert_eq!(counted.as_ref().map(|item| item.passed_int).ok(), Some(false), "passed list is not an integer");
    }

    #[test]
    fn executed_count_accepts_a_count_only_report() {
        let counted = executed_count_of(r#"{"count": 4}"#);
        assert_eq!(counted.as_ref().map(|item| item.count).ok(), Some(4), "count-only report");
        assert_eq!(counted.as_ref().map(|item| item.passed_int).ok(), Some(false), "count-only report is not a passed integer");
    }

    #[test]
    fn group_report_file_matches_the_publisher_for_every_group() {
        let source = std::fs::read_to_string(repo_root().join("tools/package.rs")).unwrap();
        let explicit = rust_quoted(&source, "const EXPLICIT_PAIRS:");
        let identity = rust_quoted(&source, "const IDENTITY_PAIRS:");
        let hyphen = rust_quoted(&source, "const HYPHEN_PAIRS:");
        assert_eq!(explicit.len() % 2, 0, "explicit pairs are not paired");
        assert_eq!(GROUPS.len(), 71, "runner group count");
        assert_eq!(explicit.len() / 2 + identity.len() + hyphen.len(), 71, "publisher pair count");
        for name in GROUPS {
            let expected = if let Some(chunk) = explicit.chunks(2).find(|chunk| chunk[0] == *name) {
                format!("{}.json", chunk[1])
            } else if identity.iter().any(|group| group == name) {
                format!("{name}.json")
            } else if hyphen.iter().any(|group| group == name) {
                format!("{}.json", name.replace('_', "-"))
            } else {
                panic!("publisher has no report for {name}");
            };
            assert_eq!(group_report_file(name), expected, "report file diverges for {name}");
        }
    }

    #[test]
    fn publish_sweep_records_a_failing_group_with_reason() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        let report = PathBuf::from("output/checks/history.json");
        let log = PathBuf::from("output/checks/history.log");
        let reason = reason_for(1, None, &report, &log, SECONDS);
        assert_eq!(reason, "exited 1 (output/checks/history.log)", "exit classification");
        assert_eq!(
            reason_for(124, Some("no fresh history.json"), &report, &log, SECONDS),
            "timed out after 180 seconds (output/checks/history.log)",
            "timeout reason"
        );
        assert_eq!(
            reason_for(1, Some("executed count is not a positive integer"), &report, &log, SECONDS),
            "executed count is not a positive integer (output/checks/history.json)",
            "count refusal reason"
        );
        let mut rows = pass_rows();
        let slot = rows.iter_mut().find(|row| row.name == "history").unwrap();
        slot.code = Some(1);
        slot.checks = Some(9);
        slot.reason = reason.clone();
        publish_sweep(root, &rows).unwrap_or_else(|err| panic!("publish_sweep refused a direct failing row: {err}"));
        let text = std::fs::read_to_string(&record).unwrap();
        let prefix = "{\n  \"published_comparator\": {\"note\": \"keep-me\"},\n  \"connected\": ";
        let suffix = ",\n  \"flow_comparator\": {\"note\": \"flow\"},\n  \"authentication_comparator\": {\"note\": \"auth\"}\n}\n";
        assert!(text.starts_with(prefix), "published_comparator bytes changed");
        assert!(text.ends_with(suffix), "comparator bytes changed");
        assert!(text.contains("\"history\": 0"), "failing group recorded as 0");
        assert!(text.contains("\"code\": 1"), "failing group code");
        assert!(text.contains(&format!("\"reason\": \"{reason}\"")), "failing group reason");
        assert!(text.contains("history is recorded as 0 because that group produced no positive check count"), "zero_count_disclosure");
        let (groups, total) = counts_in(&text).expect("counts");
        assert_eq!(groups, 71, "counts groups");
        assert_eq!(total, 70 * 3, "totals equal the sum of counts");
        assert!(text.contains("\"groups_passed\": 70"), "groups_passed");
    }

    #[test]
    fn publish_sweep_refuses_when_a_group_has_no_row() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        let original = std::fs::read(&record).unwrap();
        let rows: Vec<_> = pass_rows().into_iter().filter(|row| row.name != "history").collect();
        assert_eq!(publish_sweep(root, &rows).err().as_deref(), Some("group history has no row"), "writer refused without naming the missing group");
        assert_eq!(std::fs::read(&record).unwrap(), original, "record changed when a group has no row");
    }

    #[test]
    fn publish_sweep_keeps_zero_count_disclosure_when_nothing_is_zero() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        publish_sweep(root, &pass_rows()).unwrap_or_else(|err| panic!("{err}"));
        let text = std::fs::read_to_string(&record).unwrap();
        assert!(text.contains("\"zero_count_disclosure\": \"No group was recorded as 0.\""), "no group was recorded as 0");
    }

    #[test]
    fn publish_sweep_checks_names_every_group_and_keeps_the_executed_report() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        let report = "{\n  \"scope\": \"kept\",\n  \"passed\": [\n    \"chat declaration\",\n    \"chat review\"\n  ]\n}";
        let mut rows = pass_rows();
        let slot = rows.iter_mut().find(|row| row.name == "provider_process").unwrap();
        slot.checks = Some(2);
        slot.report = Some(report.to_string());
        publish_sweep(root, &rows).unwrap_or_else(|err| panic!("{err}"));
        let text = std::fs::read_to_string(&record).unwrap();
        let checks = json_object_after(&text, "checks");
        let keys = object_keys(checks);
        assert_eq!(keys.len(), 71, "checks rows");
        assert_eq!(keys, GROUPS.iter().copied().map(str::to_string).collect::<Vec<_>>(), "checks group order");
        let provider = json_field(checks, "provider_process");
        assert_eq!(provider, report, "executed report was reformatted");
        assert_eq!(object_keys(provider), vec!["scope".to_string(), "passed".to_string()], "executed checks keys");
        let (groups, total) = counts_in(&text).expect("counts");
        assert_eq!(groups, 71, "counts groups");
        assert_eq!(total, 70 * 3 + 2, "executed report did not change the other counts");
    }

    #[test]
    fn publish_sweep_records_unavailable_group_as_unavailable_not_executed() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        let mut rows = pass_rows();
        let slot = rows.iter_mut().find(|row| row.name == "binary_flow").unwrap();
        slot.code = None;
        slot.checks = Some(0);
        slot.missing = vec!["--previous-flow-controller reference at output/pre-branch-controller".into()];
        slot.reason = String::new();
        slot.report = Some(r#"{"passed":["stale executed name"]}"#.to_string());
        publish_sweep(root, &rows).unwrap_or_else(|err| panic!("{err}"));
        let text = std::fs::read_to_string(&record).unwrap();
        let value = json_field(json_object_after(&text, "checks"), "binary_flow");
        assert!(executed_count_of(value).is_err(), "unavailable group was treated as an executed report");
        assert_eq!(
            object_keys(value),
            vec!["name".to_string(), "code".to_string(), "missing".to_string(), "recorded_as".to_string(), "reason".to_string()],
            "unavailable checks keys"
        );
        assert_eq!(json_field(value, "code").trim(), "null", "unavailable code");
        assert_eq!(json_field(value, "recorded_as").trim(), "0", "unavailable recorded_as");
        assert!(json_field(value, "missing").contains("--previous-flow-controller reference at output/pre-branch-controller"), "missing list");
        assert!(
            json_field(value, "reason").contains("was not executed because a development input is missing: --previous-flow-controller reference at output/pre-branch-controller."),
            "unavailable reason"
        );
        assert!(!value.contains("stale executed name"), "unavailable group copied a report it did not run");
        assert!(text.contains("\"binary_flow\": 0"), "counts record 0");
        assert!(text.contains("\"code\": null"), "connected unavailable code");
        assert!(text.contains("\"recorded_as\": 0"), "connected recorded_as");
    }

    #[test]
    fn publish_sweep_records_failed_group_as_failed_not_executed() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        let mut rows = pass_rows();
        let report = PathBuf::from("output/checks/history.json");
        let log = PathBuf::from("output/checks/history.log");
        let reason = reason_for(1, None, &report, &log, SECONDS);
        let slot = rows.iter_mut().find(|row| row.name == "history").unwrap();
        slot.code = Some(1);
        slot.checks = Some(1);
        slot.reason = reason.clone();
        slot.report = Some(r#"{"passed":["lied about passing"]}"#.to_string());
        publish_sweep(root, &rows).unwrap_or_else(|err| panic!("{err}"));
        let text = std::fs::read_to_string(&record).unwrap();
        let value = json_field(json_object_after(&text, "checks"), "history");
        assert!(executed_count_of(value).is_err(), "failed group was treated as an executed report");
        assert_eq!(
            object_keys(value),
            vec!["name".to_string(), "code".to_string(), "reason".to_string(), "recorded_as".to_string()],
            "failed checks keys"
        );
        assert_eq!(json_field(value, "code").trim(), "1", "failed code is the exit code");
        assert_eq!(json_field(value, "recorded_as").trim(), "0", "failed recorded_as");
        assert!(json_field(value, "reason").contains(&reason), "failed reason");
        assert!(!value.contains("lied about passing"), "failed group copied a report that looks like a pass");
        assert!(text.contains("\"history\": 0"), "failing group recorded as 0");
    }

    #[test]
    fn publish_sweep_records_an_unreadable_report_as_failed() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        let mut rows = pass_rows();
        let report = PathBuf::from("output/checks/history.json");
        let log = PathBuf::from("output/checks/history.log");
        let reason = reason_for(0, Some("no fresh history.json"), &report, &log, SECONDS);
        let slot = rows.iter_mut().find(|row| row.name == "history").unwrap();
        slot.code = Some(0);
        slot.checks = None;
        slot.reason = reason.clone();
        slot.report = None;
        publish_sweep(root, &rows).unwrap_or_else(|err| panic!("{err}"));
        let text = std::fs::read_to_string(&record).unwrap();
        let value = json_field(json_object_after(&text, "checks"), "history");
        assert!(executed_count_of(value).is_err(), "unreadable report was treated as an executed report");
        assert_eq!(
            object_keys(value),
            vec!["name".to_string(), "code".to_string(), "reason".to_string(), "recorded_as".to_string()],
            "unreadable checks keys"
        );
        assert!(json_field(value, "reason").contains("no fresh history.json"), "unreadable reason");
        assert_eq!(json_field(value, "recorded_as").trim(), "0", "unreadable recorded_as");
    }

    #[test]
    fn sweep_arguments_pass_controllers_only_when_the_file_exists() {
        let dir = TempDir::new();
        let root = dir.path();
        let opts = bare_opts();
        let absent = sweep_arguments("prompt_reference", root, &opts);
        assert!(absent.is_empty(), "prompt_reference must not receive --previous-controller when that file is absent");
        std::fs::create_dir_all(root.join("output")).unwrap();
        std::fs::write(root.join("output/previous-controller"), b"controller").unwrap();
        let present = sweep_arguments("prompt_reference", root, &opts);
        assert_eq!(present.first().map(String::as_str), Some("--previous-controller"), "prompt_reference must receive --previous-controller when that file exists");
        assert_eq!(present.len(), 2, "prompt_reference arguments");
        let flow = sweep_arguments("binary_flow", root, &opts);
        assert_eq!(flow.first().map(String::as_str), Some("--previous-flow-controller"), "binary_flow flow argument");
        assert!(flow.get(1).is_some_and(|path| path.ends_with("output/pre-branch-controller")), "binary_flow flow default: {flow:?}");
        assert_eq!(flow.get(2).map(String::as_str), Some("--previous-auth-controller"), "binary_flow auth argument");
        assert!(flow.get(3).is_some_and(|path| path.ends_with("output/pre-auth-controller")), "binary_flow auth default: {flow:?}");
    }

    #[test]
    fn missing_input_is_unavailable_and_the_record_carries_missing() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("verification")).unwrap();
        let script = root.join("verification/apk_alignment.py");
        std::fs::write(&script, "raise SystemExit('output/tools/alignment')\n").unwrap();
        let row = sweep_one(root, "apk_alignment", &bare_opts(), SECONDS);
        assert!(row.code.is_none(), "missing input is unavailable, not a failure");
        assert_eq!(row.checks, Some(0), "missing input is recorded as 0");
        assert!(row.missing.iter().any(|item| item.contains("output/tools/alignment")), "missing input names the absent development input: {:?}", row.missing);
        let blocked: Vec<Input> = required_inputs(root, &read(&script)).into_iter().filter(|input| !input.present).collect();
        let line = unavailable_line("apk_alignment", &blocked);
        assert!(line.contains("UNAVAILABLE (not executed, missing:"), "unavailable line");
        assert!(!line.contains("FAIL"), "unavailable line charged a failure");
        let mut rows = pass_rows();
        *rows.iter_mut().find(|item| item.name == "apk_alignment").unwrap() = row;
        let record = plant_record(root);
        assert!(commit_sweep(root, &rows), "unavailable sweep must still publish");
        let text = std::fs::read_to_string(&record).unwrap();
        assert!(text.contains("\"name\": \"apk_alignment\""), "unavailable name");
        assert!(text.contains("\"code\": null"), "unexecuted code is null");
        assert!(text.contains("\"recorded_as\": 0"), "recorded_as 0");
        assert!(text.contains("output/tools/alignment"), "record entry carries missing");
        assert!(text.contains("\"apk_alignment\": 0"), "counts record 0");
        assert!(text.contains("zero_count_disclosure"), "zero_count_disclosure");
    }

    #[test]
    fn failing_sweep_leaves_the_record_byte_identical() {
        let dir = TempDir::new();
        let root = dir.path();
        let record = plant_record(root);
        let original = std::fs::read(&record).unwrap();
        let mut rows = pass_rows();
        let slot = rows.iter_mut().find(|row| row.name == "history").unwrap();
        slot.code = Some(1);
        slot.checks = None;
        slot.reason = reason_for(1, None, &root.join("output/checks/history.json"), &root.join("output/checks/history.log"), SECONDS);
        assert!(!commit_sweep(root, &rows), "a failing sweep published");
        assert_eq!(std::fs::read(&record).unwrap(), original, "failing sweep changed the record");
    }

    #[test]
    fn sweep_timeout_kills_the_grandchild_holding_stdout() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("verification")).unwrap();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        std::fs::write(root.join("verification/scene.py"), "import os, sys, time\nchild = os.fork()\nif child == 0:\n    open('output/checks/grandchild', 'w').write(str(os.getpid()))\n    sys.stdout.write('held\\n')\n    sys.stdout.flush()\n    time.sleep(120)\n    sys.exit(0)\ntime.sleep(120)\n").unwrap();
        let started = Instant::now();
        let (code, _stdout, _elapsed) = sweep_run(root, "scene", &[], 2);
        let wall = started.elapsed().as_secs();
        let pid = std::fs::read_to_string(root.join("output/checks/grandchild")).unwrap_or_default().trim().parse::<i32>().unwrap_or(0);
        let alive = process_running(pid);
        if pid > 0 {
            unsafe { kill(pid, 9); }
        }
        assert_eq!(code, 124, "grandchild holding stdout must be a 124 timeout");
        assert!(wall < 15, "timeout wall time {wall}s was not bounded");
        assert!(pid > 0, "grandchild pid was not recorded");
        assert!(!alive, "grandchild was still alive after the timeout");
    }

    #[test]
    fn sweep_timeout_abandons_a_setsid_grandchild_holding_stdout() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("verification")).unwrap();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        std::fs::write(
            root.join("verification/scene.py"),
            "import os, sys, time\nsys.stdout.write('{\"passed\": 1, \"checks\": [1]}\\n')\nsys.stdout.flush()\nchild = os.fork()\nif child == 0:\n    os.setsid()\n    open('output/checks/grandchild', 'w').write(str(os.getpid()))\n    time.sleep(30)\n    sys.exit(0)\ntime.sleep(120)\n",
        )
        .unwrap();
        let started = Instant::now();
        let (code, stdout, _elapsed) = sweep_run(root, "scene", &[], 2);
        let wall = started.elapsed().as_secs();
        let pid = std::fs::read_to_string(root.join("output/checks/grandchild")).unwrap_or_default().trim().parse::<i32>().unwrap_or(0);
        let alive = process_running(pid);
        if pid > 0 {
            unsafe { kill(pid, 9); }
        }
        assert_eq!(code, 124, "setsid grandchild holding stdout must be a 124 timeout");
        assert!(wall < 15, "setsid drain wall time {wall}s was not bounded");
        assert!(pid > 0, "setsid grandchild pid was not recorded");
        assert!(alive, "setsid grandchild died with the process group");
        let counted = executed_count_of(&String::from_utf8_lossy(&stdout)).ok().map(|item| item.count);
        assert_eq!(counted, Some(1), "setsid drain dropped the executed count already read");
        let log = std::fs::read_to_string(root.join("output/checks/scene.log")).unwrap_or_default();
        assert!(log.contains("abandoned stdout"), "setsid drain did not say stdout was abandoned: {log}");
        assert!(log.contains("abandoned stderr"), "setsid drain did not say stderr was abandoned: {log}");
        assert!(log.contains("{\"passed\": 1"), "setsid drain log lost the bytes already read: {log}");
    }

    #[test]
    fn persist_printed_json_keeps_the_original_bytes() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        let mut body = br#"{"passed": 1, "checks": [1]}"#.to_vec();
        body.push(0xff);
        let count = persist_printed_json(root, "response_fields", &body).unwrap_or_else(|err| panic!("printed json count: {err}"));
        assert_eq!(count, 1, "printed json count");
        let written = std::fs::read(root.join("output/checks").join(group_report_file("response_fields"))).unwrap();
        assert_eq!(written, body, "persist_printed_json altered the printed bytes");
    }
}

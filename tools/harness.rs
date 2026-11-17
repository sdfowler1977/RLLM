// Connected-check runner.
// `harness sweep` is native orchestration and record writing over the Python
// group scripts. It runs a group only when that group's development inputs are
// present, reads each group's JSON, and writes the connected object plus one
// checks row per group in verification/RESULTS.json. A missing development
// input is unavailable and is not charged as a product failure. A product
// failure leaves that record untouched. The same sweep writes
// output/checks/current-regression.json, one name and code per group, including
// when a group fails. Code 0 is written only for a group that passed. The bare
// run and `harness <group>` do not write either record.
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
    "capture_lifecycle", "scene", "apk_alignment", "release_numbering", "audio_route", "state_sealing",
    "desktop_runtime", "turn_background", "permission_modes", "self_rebuild", "rrsi_loop",
    "turn_runtime",
];

// Scripts under verification/ that are not runner groups. A file outside these
// sets is an unwired check and stops the census.
const HELPERS: &[&str] = &[];
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

// The native spellings of the same read. A native twin reads its environment
// through std::env, so the Python tokens above never match its source and the
// runner executes a group whose toolchain is missing instead of reporting the
// missing input. The spelling alone does not decide: the consumer of the
// value does, so a guarded or defaulted read stays optional.
const RUST_ENV_READS: &[&str] = &["env::var_os", "env::var"];

// The sanctioned resource toolchain. tooling/env.sh exports its directory as
// CLIENT_TOOLS and the tree ships the same copy, so a native twin may read the
// variable or fall back to the in-project path. What the group needs is the
// tool itself, and its absence is an unavailable development input rather than
// a product failure, so the runner resolves the tool exactly as the twin does.
const RESOURCE_TOOLS: &str = "tooling/buildtools/android-15";
const RESOURCE_TOOL: &str = "aapt2";

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// Find the next needle that is not inside a comment or a string literal, so a
// token written in prose never reads as code.
// The language a group's source is written in. A decision read that reads a
// group's own text has to read that language's comments and that language's
// literals, so the language travels with the source whose contract it decides.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceLanguage {
    Rust,
    Python,
}

// A Rust block comment nests: the comment that opens here closes only at the
// match that closes it, so the text between an outer open and its own close is
// comment even when it holds another comment. An unclosed comment runs to the
// end of the source.
fn block_comment_end(chars: &[char], at: usize) -> usize {
    let mut depth = 0i32;
    let mut index = at;
    while index < chars.len() {
        if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
            depth += 1;
            index += 2;
        } else if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
            depth -= 1;
            index += 2;
            if depth == 0 {
                break;
            }
        } else {
            index += 1;
        }
    }
    index
}

fn find_outside(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    let mut at = from;
    while at < chars.len() {
        match chars[at] {
            '/' if chars.get(at + 1) == Some(&'/') => {
                at = match chars[at..].iter().position(|c| *c == '\n') {
                    Some(offset) => at + offset + 1,
                    None => chars.len(),
                };
            }
            '/' if chars.get(at + 1) == Some(&'*') => {
                at = block_comment_end(chars, at);
            }
            '"' => {
                at = match string_end(chars, at) {
                    Some(end) => end + 1,
                    None => chars.len(),
                };
            }
            '\'' => {
                at = match char_end(chars, at) {
                    Some(end) => end + 1,
                    None => at + 1,
                };
            }
            _ => {
                if chars[at..].starts_with(&needle[..]) {
                    return Some(at);
                }
                at += 1;
            }
        }
    }
    None
}

// Find the next needle that is not inside a comment, keeping string literals.
// The set of arguments a group accepts is a property of that group's parser,
// so a token written in prose must never decide a dispatch: a comment is not
// the group's grammar. A parser arm, on the other hand, lives inside a string
// literal, so this scan keeps literals and skips comments - the opposite of
// find_outside, which skips literals too and so cannot see a parser arm.
fn find_outside_comments(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return None;
    }
    let mut at = from;
    while at < chars.len() {
        match chars[at] {
            '/' if chars.get(at + 1) == Some(&'/') => {
                at = match chars[at..].iter().position(|c| *c == '\n') {
                    Some(offset) => at + offset + 1,
                    None => chars.len(),
                };
            }
            '/' if chars.get(at + 1) == Some(&'*') => {
                at = block_comment_end(chars, at);
            }
            '"' => {
                // A literal is code: the whole extent is searched, quotes
                // included, so a quoted token is found where the group writes
                // it and a phrase inside a diagnostic is found as well.
                let end = string_end(chars, at).unwrap_or(chars.len() - 1);
                if let Some(offset) = chars[at..=end]
                    .windows(needle.len())
                    .position(|window| *window == needle[..])
                {
                    return Some(at + offset);
                }
                at = end + 1;
            }
            '\'' => {
                // A lifetime only looks like a character literal.
                let Some(end) = char_end(chars, at) else {
                    if chars[at..].starts_with(&needle[..]) {
                        return Some(at);
                    }
                    at += 1;
                    continue;
                };
                if let Some(offset) = chars[at..=end]
                    .windows(needle.len())
                    .position(|window| *window == needle[..])
                {
                    return Some(at + offset);
                }
                at = end + 1;
            }
            _ => {
                if chars[at..].starts_with(&needle[..]) {
                    return Some(at);
                }
                at += 1;
            }
        }
    }
    None
}

// Whether a needle appears in a group's source outside its comments. The
// group's own tokens are read here and not from the prose around them.
fn contains_outside_comments(source: &str, needle: &str) -> bool {
    let chars: Vec<char> = source.chars().collect();
    find_outside_comments(&chars, 0, needle).is_some()
}

// The bytes of a Python source that are code. A comment is prose and a string
// literal is the group's own text, so neither decides an argument contract.
// The mask is over-inclusive where the extent of a literal cannot be resolved:
// an extent that never closes runs to the end of the source.
fn python_code_mask(source: &str) -> Vec<bool> {
    let bytes = source.as_bytes();
    let mut code = vec![false; bytes.len()];
    let mut at = 0usize;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'#' {
            // A line comment runs to the end of the line.
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            let quote = byte;
            let triple = bytes.get(at + 1) == Some(&quote) && bytes.get(at + 2) == Some(&quote);
            let mut end = at + if triple { 3 } else { 1 };
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end += 2;
                    continue;
                }
                if triple {
                    if bytes[end] == quote
                        && bytes.get(end + 1) == Some(&quote)
                        && bytes.get(end + 2) == Some(&quote)
                    {
                        end += 3;
                        break;
                    }
                } else if bytes[end] == quote {
                    end += 1;
                    break;
                }
                end += 1;
            }
            at = end.min(bytes.len());
            continue;
        }
        code[at] = true;
        at += 1;
    }
    code
}

// The first place the needle appears in code. A needle that runs from code
// into a literal - the spelling of an argument is one - is found by its start.
fn find_in_code(source: &str, code: &[bool], needle: &str, from: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let wanted = needle.as_bytes();
    if wanted.is_empty() || wanted.len() > bytes.len() {
        return None;
    }
    let start = from.min(bytes.len());
    if start + wanted.len() > bytes.len() {
        return None;
    }
    (start..=bytes.len() - wanted.len())
        .find(|at| code[*at] && &bytes[*at..*at + wanted.len()] == wanted)
}

// The place a Python group declares the flag: the parser's own call in the
// group's code, followed by the flag's own spelling. The spelling is a string
// literal, so the call's text decides the match and not the literal's content.
fn python_declaration_at(source: &str, code: &[bool], flag: &str) -> Option<usize> {
    let single = format!("'{flag}'");
    let double = format!("\"{flag}\"");
    let mut from = 0usize;
    while let Some(at) = find_in_code(source, code, "add_argument(", from) {
        let text = &source[at + "add_argument(".len()..];
        if text.starts_with(&single) || text.starts_with(&double) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

// The text of the declaration a group wrote. A single line can declare several
// arguments, so the text stops at the next declaration.
fn declaration_text(source: &str, at: usize) -> &str {
    let line_end = source[at..].find('\n').map(|offset| at + offset).unwrap_or(source.len());
    let line = &source[at..line_end];
    match line.find("add_argument(") {
        Some(first) => match line[first + 13..].find("add_argument(") {
            Some(next) => &line[..first + 13 + next],
            None => line,
        },
        None => line,
    }
}

// Whether a group's own code declares the flag as an argument. The spelling is
// read from the group's own code in that language, so a comment that names the
// argument declares nothing.
fn declares_argument(source: &str, flag: &str, language: SourceLanguage) -> bool {
    match language {
        SourceLanguage::Rust => contains_outside_comments(source, &format!("add_argument('{flag}'")),
        SourceLanguage::Python => {
            let code = python_code_mask(source);
            python_declaration_at(source, &code, flag).is_some()
        }
    }
}

// Whether a group's own code requires the flag. A Python group requires it
// with the parser's own keyword on the declaration it wrote, so the keyword is
// read from that declaration and a keyword in a comment requires nothing.
// The scan reads a source in characters, so a position it returns is turned
// back into the byte offset the rest of the runner slices with.
fn byte_offset(source: &str, char_index: usize) -> usize {
    source
        .char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(source.len())
}

fn declares_required(source: &str, flag: &str, language: SourceLanguage) -> bool {
    match language {
        SourceLanguage::Rust => {
            let declared = format!("add_argument('{flag}'");
            // The declaration is located where the group's own code writes it,
            // so a declaration that only a comment writes is not the
            // declaration the required keyword is read from.
            let chars: Vec<char> = source.chars().collect();
            let Some(at) = find_outside_comments(&chars, 0, &declared) else { return false };
            let at = byte_offset(source, at);
            contains_outside_comments(declaration_text(source, at), "required=True")
        }
        SourceLanguage::Python => {
            let code = python_code_mask(source);
            let Some(at) = python_declaration_at(source, &code, flag) else { return false };
            let declaration = declaration_text(source, at);
            let declaration_code = python_code_mask(declaration);
            find_in_code(declaration, &declaration_code, "required=True", 0).is_some()
        }
    }
}

fn string_end(chars: &[char], at: usize) -> Option<usize> {
    let mut index = at + 1;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            '"' => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

// A character literal, or a lifetime that only looks like one.
fn char_end(chars: &[char], at: usize) -> Option<usize> {
    match chars.get(at + 1) {
        Some('\\') => {
            let mut index = at + 2;
            while index < chars.len() && chars[index] != '\'' {
                index += 1;
            }
            if index < chars.len() {
                Some(index)
            } else {
                None
            }
        }
        Some(_) => {
            if chars.get(at + 2) == Some(&'\'') {
                Some(at + 2)
            } else {
                None
            }
        }
        None => None,
    }
}

fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut at = open;
    while at < chars.len() {
        match chars[at] {
            '"' => {
                at = match string_end(chars, at) {
                    Some(end) => end + 1,
                    None => return None,
                };
            }
            '\'' => {
                at = match char_end(chars, at) {
                    Some(end) => end + 1,
                    None => at + 1,
                };
            }
            '(' => {
                depth += 1;
                at += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
                at += 1;
            }
            _ => at += 1,
        }
    }
    None
}

// The literal name of an environment variable, when the read names one.
fn literal_name(chars: &[char]) -> Option<String> {
    let mut start = 0;
    while start < chars.len() && chars[start].is_whitespace() {
        start += 1;
    }
    let mut end = chars.len();
    while end > start && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    if end - start < 3 || chars[start] != '"' || chars[end - 1] != '"' {
        return None;
    }
    let inner = &chars[start + 1..end - 1];
    if inner.contains(&'"') {
        return None;
    }
    Some(inner.iter().collect())
}

fn skip_space(chars: &[char], mut at: usize) -> usize {
    while at < chars.len() && chars[at].is_whitespace() {
        at += 1;
    }
    at
}

// The chain that consumes a read decides whether the group needs the value:
// expect and unwrap abort the group, and unwrap_or_else aborts it only when
// its fallback panics. A read whose chain ends supplies its own alternative.
fn chain_requires_value(chars: &[char], from: usize) -> bool {
    let mut at = from;
    loop {
        at = skip_space(chars, at);
        if chars.get(at) != Some(&'.') {
            return false;
        }
        at += 1;
        let start = at;
        while at < chars.len() && is_ident_char(chars[at]) {
            at += 1;
        }
        if at == start {
            return false;
        }
        let name: String = chars[start..at].iter().collect();
        at = skip_space(chars, at);
        if chars.get(at) != Some(&'(') {
            continue;
        }
        let Some(close) = matching_paren(chars, at) else {
            return false;
        };
        let args: String = chars[at + 1..close].iter().collect();
        match name.as_str() {
            "expect" | "unwrap" => return true,
            "unwrap_or_else" => {
                if args.contains("panic!") {
                    return true;
                }
            }
            _ => {}
        }
        at = close + 1;
    }
}

// The environment variables a native twin reads, with whether the consumer of
// the value makes the read required. The read is located in the group's own
// source and the consumer decides, so a guarded read or a read with a default
// stays optional and never blocks the group.
fn rust_env_reads(source: &str) -> Vec<(String, bool)> {
    let chars: Vec<char> = source.chars().collect();
    let mut reads: Vec<(String, bool)> = Vec::new();
    for needle in RUST_ENV_READS {
        let mut from = 0;
        while let Some(found) = find_outside(&chars, from, needle) {
            from = found + needle.len();
            // A longer name such as myenv::var is not this read.
            if found > 0 && is_ident_char(chars[found - 1]) {
                continue;
            }
            if chars.get(from) != Some(&'(') {
                continue;
            }
            let Some(close) = matching_paren(&chars, from) else {
                break;
            };
            if let Some(name) = literal_name(&chars[from + 1..close]) {
                if !reads.iter().any(|(existing, _)| *existing == name) {
                    reads.push((name, chain_requires_value(&chars, close + 1)));
                }
            }
            from = close + 1;
        }
    }
    reads
}

// The resource tools a group needs, resolved the way the twin resolves them:
// the exported directory first, the in-project copy when the group's own
// source names it. The dependency is satisfied by whichever copy actually
// carries the tool, so a twin that names the in-project path runs without an
// ambient variable and a group with neither copy is reported as unavailable
// instead of failing on a missing executable.
fn resource_tool_input(
    root: &Path,
    source: &str,
    reads: &[(String, bool)],
    configured: Option<&str>,
) -> Option<Input> {
    let reads_the_tools = reads.iter().any(|(name, _)| name.as_str() == "CLIENT_TOOLS");
    if !reads_the_tools && !source.contains(RESOURCE_TOOLS) {
        return None;
    }
    let mut present = false;
    if let Some(directory) = configured.map(str::trim).filter(|value| !value.is_empty()) {
        present = Path::new(directory).join(RESOURCE_TOOL).is_file();
    }
    if !present && source.contains(RESOURCE_TOOLS) {
        present = root.join(RESOURCE_TOOLS).join(RESOURCE_TOOL).is_file();
    }
    Some(Input {
        kind: "env",
        detail: format!(
            "CLIENT_TOOLS environment variable or the in-project resource tools at {RESOURCE_TOOLS}/{RESOURCE_TOOL}"
        ),
        present,
    })
}

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
fn required_inputs(root: &Path, source: &str, language: SourceLanguage) -> Vec<Input> {
    let mut inputs = Vec::new();
    let mut seen = BTreeSet::new();
    // A native twin reads the same variables through std::env, so the Python
    // spellings below never match its source. The read is required only when
    // its value is consumed by a panic, so a missing toolchain is reported as
    // an unavailable input instead of failing the group.
    let native_reads = rust_env_reads(source);
    // The resource tools are the one dependency a twin may resolve two ways, so
    // the runner resolves the tool the way the twin resolves it and names the
    // tool that is absent instead of the variable that would have pointed at
    // it.
    let configured = std::env::var("CLIENT_TOOLS").ok();
    if let Some(input) = resource_tool_input(root, source, &native_reads, configured.as_deref()) {
        seen.insert("CLIENT_TOOLS".to_string());
        inputs.push(input);
    }
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
    for (name, required) in native_reads {
        if !required {
            continue;
        }
        if seen.insert(name.clone()) {
            inputs.push(Input {
                kind: "env",
                detail: format!("{name} environment variable"),
                present: match std::env::var(&name) {
                    Ok(value) => !value.trim().is_empty(),
                    Err(_) => false,
                },
            });
        }
    }
    for (flag, default, _) in INPUT_FLAGS {
        // A script declares an argument; a native twin states its own requirement
        // in the parse error it prints when the argument is missing. A twin that
        // only mentions the flag guards the comparison itself and stays runnable.
        // The phrase is read from the group's own literal, so a comment that
        // mentions it is prose and decides nothing.
        let declared = declares_argument(source, flag, language);
        let native_required = contains_outside_comments(source, &format!("arguments are required: {flag}"));
        if !declared && !native_required {
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
        let required = if declared {
            flag_required(source, flag, default, language)
        } else {
            native_required && !declares_optional_guard(source, flag, language)
        };
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

// A group that reads a reference only inside a guard runs without it; one that
// reads it unconditionally cannot. Both signals live in the group's own source,
// so the pre-flight and the argument builder must read the same authority here.
fn declares_optional_guard(source: &str, flag: &str, language: SourceLanguage) -> bool {
    let name = format!("args.{}", flag.trim_start_matches('-').replace('-', "_"));
    let guard = format!("if {name}");
    let negated = format!("if !{name}");
    match language {
        SourceLanguage::Rust => {
            contains_outside_comments(source, &guard) || contains_outside_comments(source, &negated)
        }
        SourceLanguage::Python => {
            let code = python_code_mask(source);
            find_in_code(source, &code, &guard, 0).is_some()
                || find_in_code(source, &code, &negated, 0).is_some()
        }
    }
}

fn flag_required(source: &str, flag: &str, default: &str, language: SourceLanguage) -> bool {
    !declares_optional_guard(source, flag, language)
        && (declares_required(source, flag, language) || !default.is_empty())
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

// A native twin parses the runner's reference flags itself, so the Python
// declaration never appears in its source and the group would be dispatched
// without the reference its comparison leg is gated on. The spelling that says
// a group accepts a flag is a quoted token outside the group's comments, and
// the group is native when it reads its own arguments; a Python group never
// does, so its own path through this function is untouched.
fn native_accepts_flag(source: &str, flag: &str) -> bool {
    if !contains_outside_comments(source, "env::args") {
        return false;
    }
    contains_outside_comments(source, &format!("\"{flag}\""))
        || contains_outside_comments(source, &format!("\"{flag}=\""))
        || contains_outside_comments(source, &format!("arguments are required: {flag}"))
}

// A required reference is validated before dispatch; an optional one is passed
// only when it actually exists, and its absence is reported as an unexercised
// comparison leg rather than a silent narrowing.
fn group_arguments(root: &Path, source: &str, language: SourceLanguage) -> (Vec<String>, Vec<String>) {
    let mut arguments = Vec::new();
    let mut unexercised = Vec::new();
    for (flag, default, _) in INPUT_FLAGS {
        let declared = declares_argument(source, flag, language);
        if !declared && !native_accepts_flag(source, flag) {
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
                // A Python group states its own requirement with required=True; a
                // native group's reference is a comparison input, so its absence
                // leaves the group runnable and is reported as an unexercised
                // comparison leg instead of a silent narrowing.
                let required = declared && flag_required(source, flag, default, language);
                if !required {
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
    let mut natives = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("verification")).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        match path.extension().and_then(|value| value.to_str()) {
            Some("py") => {
                scripts.insert(stem.to_string());
            }
            Some("rs") => {
                natives.insert(stem.to_string());
            }
            _ => {}
        }
    }
    for name in GROUPS {
        if !scripts.contains(*name) && !natives.contains(*name) {
            problems.push(format!("group {name} has no verification/{name}.py or verification/{name}.rs"));
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
    // A top-level twin is built into a runnable group binary, so an undeclared
    // one would sit in output/verification/ unaccounted for.
    for native in &natives {
        if !declared.contains(native) {
            problems.push(format!("verification/{native}.rs is not a runner group, a declared helper or a declared optional group"));
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
#[derive(Debug)]
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

// The caller also passes the harness's own mapping for the group: a native twin
// derives its report filename from the shared helper, so that name appears
// nowhere in its source, and a leg that quotes no report would otherwise have no
// file-side count source at all. The caller must have removed that file before
// the group ran (`clear_report`), because a harness-derived name cannot be
// attributed to this run by its timestamp alone: modification times are too coarse.
fn file_measure(root: &Path, source: &str, canonical: &str, started: SystemTime) -> Counted {
    let mut found = Counted::Absent;
    let mut names = quoted_json_names(source);
    if !names.iter().any(|existing| existing == canonical) {
        names.push(canonical.to_string());
    }
    for name in names {
        let path = root.join("output/checks").join(&name);
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

// Presence after the run is the proof of execution, so the report this run is
// expected to publish is removed before the run. Returns whether a report was
// removed, which is how a leftover from an earlier run stops standing in for an
// execution that never happened.
fn clear_report(root: &Path, name: &str) -> bool {
    let path = root.join("output/checks").join(group_report_file(name));
    match std::fs::remove_file(&path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            eprintln!("{name}: could not remove stale {name} report: {error}");
            false
        }
    }
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

fn harvest(root: &Path, name: &str, source: &str, text: &str, started: SystemTime) -> (Option<usize>, &'static str) {
    combine_counts(last_measure(text), file_measure(root, source, &group_report_file(name), started), printed_line_count(text))
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

// The sweep's per-group argument rules, inherited from the retired Python runner.
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

fn leg_source(root: &Path, name: &str) -> (String, SourceLanguage) {
    let native = root.join(format!("verification/{name}.rs"));
    if native.is_file() {
        (read(&native), SourceLanguage::Rust)
    } else {
        (
            read(&root.join(format!("verification/{name}.py"))),
            SourceLanguage::Python,
        )
    }
}

// A built twin at output/verification/<name> is the group. The python3 script
// remains the fallback when that binary is absent.
fn group_command(root: &Path, name: &str, arguments: &[String]) -> Command {
    let twin = root.join("output/verification").join(name);
    if twin.is_file() {
        let mut command = Command::new(&twin);
        command.args(arguments).current_dir(root).process_group(0);
        command
    } else {
        python_group(root, name, arguments)
    }
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
    let spawned = group_command(root, name, arguments).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
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
// The group owns that object; the sweep stores the printed bytes unchanged.
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

// Publisher code 0 is a pass. The one gap a candidate publication tolerates is
// binary_flow with code 1 and only the keys name and code. A group that did not
// run, or that exited 0 without a positive check count, has no measured
// non-zero exit. Recording 0 would say it passed, and recording 1 on
// binary_flow would look like that tolerated gap, so the code is 2.
const REGRESSION_NOT_PASSED: i32 = 2;

fn regression_code(row: &SweepRow) -> i32 {
    if row.code == Some(0) && matches!(row.checks, Some(count) if count > 0) {
        return 0;
    }
    match row.code {
        Some(code) if code != 0 => code,
        _ => REGRESSION_NOT_PASSED,
    }
}

fn regression_document(rows: &[SweepRow]) -> Result<String, String> {
    let mut body = String::from("[\n");
    for (index, name) in GROUPS.iter().enumerate() {
        let row = rows.iter().find(|row| row.name == *name).ok_or_else(|| {
            format!("current-regression.json refused: group {name} has no row")
        })?;
        let code = regression_code(row);
        if index > 0 {
            body.push_str(",\n");
        }
        body.push_str("  {\n    \"name\": \"");
        body.push_str(&json_escape(name));
        body.push_str("\",\n    \"code\": ");
        body.push_str(&code.to_string());
        body.push_str("\n  }");
    }
    body.push_str("\n]\n");
    Ok(body)
}

// The whole array is built before the file is opened. A missing group returns
// here and the previous output/checks/current-regression.json stays as it was.
fn publish_regression(root: &Path, rows: &[SweepRow]) -> Result<(), String> {
    let document = regression_document(rows)?;
    let dir = root.join("output/checks");
    if std::fs::create_dir_all(&dir).is_err() {
        return Err("current-regression.json could not be written".into());
    }
    let path = dir.join("current-regression.json");
    let temporary = dir.join("current-regression.json.new");
    if std::fs::write(&temporary, &document).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err("current-regression.json could not be written".into());
    }
    if std::fs::rename(&temporary, &path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err("current-regression.json could not be written".into());
    }
    Ok(())
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
    let (source, language) = leg_source(root, name);
    let missing: Vec<Input> = required_inputs(root, &source, language)
        .into_iter()
        .filter(|input| !input.present)
        .collect();
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
    // The report this run is expected to publish is removed before the group
    // runs, so a report left by an earlier run cannot stand in for a group
    // that published nothing. Freshness alone cannot carry that provenance:
    // the kernel stamps a file with a clock that trails the real time clock
    // the freshness check reads, so a report written after the run started
    // still reads older than the run start, and the backward tolerance that
    // absorbs that trail also accepts a leftover.
    clear_report(root, name);
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

fn finish_sweep(root: &Path, rows: &[SweepRow]) -> Result<i32, String> {
    let passed = rows.iter().filter(|row| !row_failed(row) && row.code.is_some()).count();
    let failed = rows.iter().filter(|row| row_failed(row)).count();
    let unavailable = rows.iter().filter(|row| row.code.is_none()).count();
    let absent = rows.iter().filter(|row| row.code.is_some() && row.checks.is_none()).count();
    let total: usize = rows.iter().map(|row| if row.code == Some(0) { row.checks.unwrap_or(0) } else { 0 }).sum();
    let regression = publish_regression(root, rows);
    let wrote = if regression.is_ok() { commit_sweep(root, rows) } else { false };
    println!(
        "summary: {passed} pass, {failed} fail, {unavailable} unavailable, {absent} absent of {} groups, {total} checks",
        GROUPS.len()
    );
    if let Err(error) = regression {
        return Err(error);
    }
    if !wrote || failed > 0 || absent > 0 {
        return Ok(1);
    }
    Ok(0)
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
    match finish_sweep(&root, &rows) {
        Ok(code) => Ok(code),
        Err(error) => {
            eprintln!("regression record left untouched: {error}");
            Ok(1)
        }
    }
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
    let spawned = group_command(root, name, arguments)
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

// Verdict for one name passed to `harness <name>`. GROUPS run. A declared
// helper or optional group is real but this route does not execute it. Anything
// else is an unknown name and must not exit green.
enum RequestedGroup {
    Executable,
    DeclaredNotExecutable { kind: &'static str, reason: &'static str },
    Unknown,
}

fn classify_requested_group(name: &str) -> RequestedGroup {
    if GROUPS.iter().any(|group| *group == name) {
        return RequestedGroup::Executable;
    }
    if let Some((_, reason)) = OPTIONAL.iter().find(|(group, _)| *group == name) {
        return RequestedGroup::DeclaredNotExecutable { kind: "optional group", reason: *reason };
    }
    if HELPERS.iter().any(|group| *group == name) {
        return RequestedGroup::DeclaredNotExecutable { kind: "helper group", reason: "not a runner group" };
    }
    RequestedGroup::Unknown
}

fn requested_group_refusal(name: &str) -> Option<String> {
    match classify_requested_group(name) {
        RequestedGroup::Executable => None,
        RequestedGroup::Unknown => Some(format!(
            "unknown group {name} (not a runner group, a declared helper or a declared optional group)"
        )),
        RequestedGroup::DeclaredNotExecutable { kind, reason } => {
            Some(format!("{name} NOT EXECUTED ({kind}: {reason})"))
        }
    }
}

fn requested_group_refusals(names: &[String]) -> Vec<String> {
    names.iter().filter_map(|name| requested_group_refusal(name)).collect()
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
    let refusals = requested_group_refusals(&args);
    if !refusals.is_empty() {
        for line in &refusals {
            eprintln!("{line}");
        }
        eprintln!("{}", sweep_usage());
        std::process::exit(2);
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
        let (source, language) = leg_source(&root, name);
        let missing: Vec<Input> = required_inputs(&root, &source, language)
            .into_iter()
            .filter(|input| !input.present)
            .collect();
        let (arguments, unexercised) = group_arguments(&root, &source, language);
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
        // The count for a native twin comes from the report name the harness
        // derives, so the stale report goes away before the group runs.
        clear_report(&root, name);
        let started_at = SystemTime::now();
        let (code, text, elapsed) = run(&root, name, &arguments);
        std::fs::write(output.join(format!("{name}.log")), &text).ok();
        let (checks, count_source) = harvest(&root, name, &source, &text, started_at);
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
        assert_eq!(GROUPS.len(), 73, "runner group count");
        assert_eq!(explicit.len() / 2 + identity.len() + hyphen.len(), 73, "publisher pair count");
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
    fn file_measure_counts_the_canonical_report_a_twin_never_names_in_its_source() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        let started = SystemTime::now();
        std::fs::write(root.join("output/checks/acp.json"), r#"{"passed": ["a", "b", "c"]}"#).unwrap();
        // The twin writes through the shared helper, so its source quotes no
        // report name; only the harness's own mapping can find this file.
        let counted = file_measure(root, "twin::report_value(\"acp\", &body, true);", "acp.json", started);
        assert!(matches!(counted, Counted::Value(3)), "twin report not counted: {counted:?}");
    }

    #[test]
    fn file_measure_refutes_a_canonical_report_disagreeing_with_a_quoted_one() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        let started = SystemTime::now();
        std::fs::write(root.join("output/checks/history-results.json"), r#"{"count": 7}"#).unwrap();
        std::fs::write(root.join("output/checks/leg-notes.json"), r#"{"count": 9}"#).unwrap();
        let counted = file_measure(root, "\"leg-notes.json\"", "history-results.json", started);
        assert!(matches!(counted, Counted::Conflict), "disagreeing count sources must conflict: {counted:?}");
    }

    #[test]
    fn clear_report_takes_the_stale_canonical_report_out_of_the_count() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        std::fs::write(root.join("output/checks/history-results.json"), r#"{"count": 5}"#).unwrap();
        assert!(clear_report(root, "history"), "stale report removed");
        assert!(!clear_report(root, "history"), "nothing left to remove");
        // A group that publishes nothing now has no file-side count at all,
        // instead of inheriting the count of an earlier run.
        let counted = file_measure(root, "\"history-results.json\"", "history-results.json", SystemTime::now());
        assert!(matches!(counted, Counted::Absent), "leftover still counted: {counted:?}");
    }

    #[test]
    fn harvest_counts_a_twin_from_the_report_file_alone() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        let started = SystemTime::now();
        std::fs::write(root.join("output/checks/acp.json"), r#"{"passed": ["a", "b", "c"]}"#).unwrap();
        // The witnessed defect: a twin quotes no report name and prints nothing,
        // so the row used to read "acp PASS (exit 0, 0s, unparsed checks)".
        let (checks, source) = harvest(root, "acp", "twin::report_value(\"acp\", &body, true);", "", started);
        assert_eq!(checks, Some(3), "twin count harvested");
        assert_eq!(source, "file", "count came from the published report");
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
        assert_eq!(groups, 73, "counts groups");
        assert_eq!(total, 72 * 3, "totals equal the sum of counts");
        assert!(text.contains("\"groups_passed\": 72"), "groups_passed");
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
        assert_eq!(keys.len(), 73, "checks rows");
        assert_eq!(keys, GROUPS.iter().copied().map(str::to_string).collect::<Vec<_>>(), "checks group order");
        let provider = json_field(checks, "provider_process");
        assert_eq!(provider, report, "executed report was reformatted");
        assert_eq!(object_keys(provider), vec!["scope".to_string(), "passed".to_string()], "executed checks keys");
        let (groups, total) = counts_in(&text).expect("counts");
        assert_eq!(groups, 73, "counts groups");
        assert_eq!(total, 72 * 3 + 2, "executed report did not change the other counts");
    }

    #[test]
    fn delivered_record_selects_exactly_the_harness_group_list() {
        // The retired Python runner cross-checked its own group list against GROUPS, so the
        // two-sided equality needs a native home: the delivered record is the second source.
        let path = project_root().join("verification/RESULTS.json");
        let text = read(&path);
        assert_eq!(
            object_keys(json_object_after(&text, "counts")),
            GROUPS.iter().copied().map(str::to_string).collect::<Vec<_>>(),
            "delivered record groups differ from the harness GROUPS list"
        );
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
    fn a_native_group_that_names_its_required_reference_is_unavailable_without_it() {
        // The reference this row measures is the one in the tree; when the
        // environment names another one the pre-flight would answer for the
        // environment, so the row reports that it did not measure.
        if !no_environment_reference_is_configured(&["--previous-controller"]) {
            return;
        }
        let dir = TempDir::new();
        let root = dir.path();
        let native = "fn main(){match flag{--previous-controller=>{}}panic!(\"the following arguments are required: --previous-controller\");}";
        let inputs = required_inputs(root, native, SourceLanguage::Rust);
        assert!(inputs.iter().any(|input| input.kind == "path" && !input.present && input.detail.contains("--previous-controller reference at output/previous-controller")), "native required input: {:?}", inputs.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
        let guarded = "fn main(){let value=if seen{\"--previous-controller\"}else{\"\"};}";
        assert!(!required_inputs(root, guarded, SourceLanguage::Rust).iter().any(|input| input.detail.contains("previous-controller")), "a native mention without the required-argument phrase stays runnable");
        let guard_shaped = "fn main(){if !args.previous_controller{panic!(\"the following arguments are required: --previous-controller\");}}";
        assert!(!required_inputs(root, guard_shaped, SourceLanguage::Rust).iter().any(|input| input.detail.contains("previous-controller")), "the optional-guard pattern, not the panic phrase, decides a guarded native reference");
        assert!(declares_optional_guard(guard_shaped, "--previous-controller", SourceLanguage::Rust), "the guard-shaped requirement is read as optional: a known limit of the lexical rule");
        std::fs::create_dir_all(root.join("output")).unwrap();
        std::fs::write(root.join("output/previous-controller"), b"controller").unwrap();
        assert!(!required_inputs(root, native, SourceLanguage::Rust).iter().any(|input| !input.present && input.detail.contains("--previous-controller")), "the reference build satisfies the input");
    }

    // The rows that measure a reference measure the reference that sits in the
    // tree; an environment reference would answer for it and would turn their
    // absence cases into a measurement of the environment instead.
    fn environment_reference_is_configured(value: Option<&str>) -> bool {
        value.map(|value| !value.trim().is_empty()).unwrap_or(false)
    }

    // A row whose premise is absent reports that it did not measure and
    // returns, so an ambient value outside the product leaves the rows
    // unmeasured instead of failing the sanctioned build of the runner. Each
    // row names the references it measures, because the environment answers
    // for one reference at a time: the variable that names a flag is the one
    // that would take that flag's place in the tree.
    fn no_environment_reference_is_configured(flags: &[&str]) -> bool {
        for flag in flags {
            let Some((_, _, variable)) = INPUT_FLAGS.iter().find(|(name, _, _)| *name == *flag) else {
                continue;
            };
            let configured = std::env::var(*variable).ok();
            if environment_reference_is_configured(configured.as_deref()) {
                eprintln!(
                    "not measured: {variable} names a reference outside the tree, so this row would measure the environment"
                );
                return false;
            }
        }
        true
    }

    #[test]
    fn a_native_group_receives_the_reference_flag_its_own_source_names() {
        if !no_environment_reference_is_configured(&["--previous-controller"]) {
            return;
        }
        let dir = TempDir::new();
        let root = dir.path();
        // The shape of verification/history_migration.rs: the group reads its
        // own arguments, matches the flag itself and states its requirement in
        // the parse error it prints when the flag is missing.
        let native = "fn main(){let mut previous=None;for argument in std::env::args().skip(1){match argument.as_str(){\"--previous-controller\"=>{previous=Some(argument);}}}if previous.is_none(){panic!(\"the following arguments are required: --previous-controller\");}}";
        assert!(!native.contains("add_argument("), "this row measures the native spelling, not the Python one");
        // The pre-flight names the reference it needs and the builder names the
        // reference it passes; both must be the same file, or the comparison
        // leg is exercised against a reference the group never saw.
        let absent = required_inputs(root, native, SourceLanguage::Rust);
        assert!(absent.iter().any(|input| input.kind == "path" && !input.present && input.detail.contains("--previous-controller reference at output/previous-controller")), "the pre-flight names the reference the argument builder passes: {:?}", absent.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
        std::fs::create_dir_all(root.join("output")).unwrap();
        std::fs::write(root.join("output/previous-controller"), b"controller").unwrap();
        let present = required_inputs(root, native, SourceLanguage::Rust);
        assert!(!present.iter().any(|input| input.detail.contains("--previous-controller")), "the reference the tree holds satisfies the pre-flight: {:?}", present.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
        let (arguments, unexercised) = group_arguments(root, native, SourceLanguage::Rust);
        assert_eq!(arguments.first().map(String::as_str), Some("--previous-controller"), "a native group is dispatched with the reference flag its own source names: {arguments:?}");
        assert_eq!(arguments.len(), 2, "the native reference flag and its path, and nothing else: {arguments:?}");
        assert!(arguments[1].ends_with("output/previous-controller"), "the native reference path is the one the tree holds: {arguments:?}");
        assert!(unexercised.is_empty(), "the comparison leg is exercised when the reference exists: {unexercised:?}");
    }

    #[test]
    fn a_native_group_whose_reference_is_absent_stays_runnable_and_names_the_unexercised_leg() {
        if !no_environment_reference_is_configured(&["--previous-controller"]) {
            return;
        }
        let dir = TempDir::new();
        let root = dir.path();
        // The shape of verification/prompt_reference.rs: the group reads its own
        // arguments, takes the reference when one is offered and compares only
        // when one was given.
        let native = "fn main(){let mut previous=None;for argument in std::env::args().skip(1){if argument==\"--previous-controller\"{previous=Some(argument);}}if previous.is_none(){println!(\"no previous controller\");}}";
        assert!(!native.contains("arguments are required:"), "this row measures the optional native reference, not the required one");
        let (arguments, unexercised) = group_arguments(root, native, SourceLanguage::Rust);
        assert!(arguments.is_empty(), "an absent reference is not passed to the group: {arguments:?}");
        assert_eq!(unexercised, vec!["--previous-controller".to_string()], "the absent reference is reported as an unexercised comparison leg");
        let inputs = required_inputs(root, native, SourceLanguage::Rust);
        assert!(inputs.iter().all(|input| input.present), "an absent optional reference leaves the group runnable: {:?}", inputs.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
        assert!(!inputs.iter().any(|input| input.detail.contains("--previous-controller")), "an optional native reference is not a required input: {:?}", inputs.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
        std::fs::create_dir_all(root.join("output")).unwrap();
        std::fs::write(root.join("output/previous-controller"), b"controller").unwrap();
        let (arguments, unexercised) = group_arguments(root, native, SourceLanguage::Rust);
        assert_eq!(arguments.first().map(String::as_str), Some("--previous-controller"), "the native group receives the reference when the tree holds one: {arguments:?}");
        assert_eq!(arguments.len(), 2, "the native reference flag and its path, and nothing else: {arguments:?}");
        assert!(unexercised.is_empty(), "the comparison leg is exercised when the reference exists: {unexercised:?}");
    }

    #[test]
    fn the_python_spelling_of_a_group_is_unchanged() {
        if !no_environment_reference_is_configured(&["--previous-controller"]) {
            return;
        }
        let dir = TempDir::new();
        let root = dir.path();
        let required = "import argparse\nargs=argparse.ArgumentParser();args.add_argument('--previous-controller',required=True)\nargs=args.parse_args()";
        let (arguments, unexercised) = group_arguments(root, required, SourceLanguage::Python);
        assert!(arguments.is_empty(), "a required Python reference is not passed when it is absent: {arguments:?}");
        assert!(unexercised.is_empty(), "a required Python reference is a missing input, not an unexercised leg: {unexercised:?}");
        let inputs = required_inputs(root, required, SourceLanguage::Python);
        assert!(inputs.iter().any(|input| input.kind == "path" && !input.present && input.detail.contains("--previous-controller reference at output/previous-controller")), "the pre-flight still reports the required Python reference as missing: {:?}", inputs.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
        let optional = "import argparse\nargs=argparse.ArgumentParser();args.add_argument('--previous-controller')\nargs=args.parse_args()\nif args.previous_controller:\n    print(args.previous_controller)";
        let (arguments, unexercised) = group_arguments(root, optional, SourceLanguage::Python);
        assert!(arguments.is_empty(), "an absent optional Python reference is not passed: {arguments:?}");
        assert_eq!(unexercised, vec!["--previous-controller".to_string()], "an absent optional Python reference is an unexercised comparison leg");
        std::fs::create_dir_all(root.join("output")).unwrap();
        std::fs::write(root.join("output/previous-controller"), b"controller").unwrap();
        let (arguments, unexercised) = group_arguments(root, required, SourceLanguage::Python);
        assert_eq!(arguments.first().map(String::as_str), Some("--previous-controller"), "the Python group receives its reference: {arguments:?}");
        assert_eq!(arguments.len(), 2, "the Python reference flag and its path, and nothing else: {arguments:?}");
        assert!(unexercised.is_empty(), "the Python comparison leg is exercised when the reference exists: {unexercised:?}");
    }

    // A Python comment is prose: it names no argument and requires none. The
    // declaration is read from the group's own code, so a comment that spells
    // the declaration decides nothing.
    #[test]
    fn a_python_comment_does_not_declare_an_argument() {
        let dir = TempDir::new();
        let root = dir.path();
        let commented = "import argparse\n# args.add_argument('--previous-controller',required=True)\n# if args.previous_controller:\nargs=argparse.ArgumentParser();args=args.parse_args()";
        assert!(commented.contains("# args.add_argument("), "the fixture writes the declaration in a comment");
        assert!(!declares_argument(commented, "--previous-controller", SourceLanguage::Python), "a comment names no argument");
        assert!(!declares_required(commented, "--previous-controller", SourceLanguage::Python), "a comment requires nothing");
        assert!(!declares_optional_guard(commented, "--previous-controller", SourceLanguage::Python), "a comment guards nothing");
        let (arguments, unexercised) = group_arguments(root, commented, SourceLanguage::Python);
        assert!(arguments.is_empty(), "a group whose only mention is a comment is dispatched nothing: {arguments:?}");
        assert!(unexercised.is_empty(), "a group whose only mention is a comment has no reference leg to report: {unexercised:?}");
        assert!(!required_inputs(root, commented, SourceLanguage::Python).iter().any(|input| input.detail.contains("previous-controller")), "a comment demands nothing");
        // The group's own code still declares the argument it declares.
        let declared = "import argparse\nargs=argparse.ArgumentParser();args.add_argument('--previous-controller',required=True)\nargs=args.parse_args()";
        assert!(declares_argument(declared, "--previous-controller", SourceLanguage::Python), "the group's own code declares the argument");
        assert!(declares_required(declared, "--previous-controller", SourceLanguage::Python), "the group's own code requires it");
        assert!(!declares_optional_guard(declared, "--previous-controller", SourceLanguage::Python), "the group's own code does not guard it");
        // The Python script the tree still carries declares its argument after
        // this change, so the tree's own groups keep their contract.
        let script = project_root().join("verification/history_migration.py");
        if !script.is_file() {
            eprintln!(
                "not measured: the tree holds no verification/history_migration.py, so this row would measure a fixture instead of the tree's own script"
            );
            return;
        }
        let text = read(&script);
        assert!(declares_argument(&text, "--previous-controller", SourceLanguage::Python), "the tree's own Python script still declares the argument: {script:?}");
        assert!(declares_required(&text, "--previous-controller", SourceLanguage::Python), "the tree's own Python script still requires it: {script:?}");
    }

    // A token written in prose is not the group's grammar. The set of
    // arguments a group accepts is a property of that group's parser, so a
    // comment that names a flag must not change what the runner dispatches,
    // while a parser arm - which lives inside a string literal - is still
    // found. The known limit of this rule is a quoted token in a literal that
    // is not a parser arm: it is read as an accepted argument.
    #[test]
    fn a_quoted_token_in_a_comment_never_decides_a_native_dispatch() {
        let dir = TempDir::new();
        let root = dir.path();
        // The shape that was measured: the group reads its own arguments and
        // the only mention of the flag is a comment that names it.
        let commented = "// the runner also knows the older \"--previous-controller\" reference.\nfn main(){for argument in std::env::args().skip(1){let _=argument;}}";
        assert!(commented.contains("\"--previous-controller\""), "the comment names the token as a quoted token");
        assert!(!native_accepts_flag(commented, "--previous-controller"), "a quoted token in a comment is prose, not the group's grammar");
        let (arguments, unexercised) = group_arguments(root, commented, SourceLanguage::Rust);
        assert!(arguments.is_empty(), "a group whose only mention is a comment is dispatched nothing: {arguments:?}");
        assert!(unexercised.is_empty(), "a group whose only mention is a comment has no reference leg to report: {unexercised:?}");
        // A block comment is prose as well.
        let blocked = "/* \"--previous-controller\" */\nfn main(){for argument in std::env::args().skip(1){let _=argument;}}";
        assert!(!native_accepts_flag(blocked, "--previous-controller"), "a quoted token in a block comment is prose as well");
        // The group's own parser arm is a string literal, so the scan keeps
        // literals and the group is dispatched the flag it accepts.
        let armed = "fn main(){for argument in std::env::args().skip(1){match argument.as_str(){\"--previous-controller\"=>{}}}}";
        assert!(native_accepts_flag(armed, "--previous-controller"), "a parser arm is a string literal and is found");
        let assigned = "fn main(){for argument in std::env::args().skip(1){if argument.starts_with(\"--previous-controller=\"){}}}";
        assert!(native_accepts_flag(assigned, "--previous-controller"), "the assignment spelling is a literal too");
        // A group that reads its arguments only inside a comment is not a
        // native group at all.
        let prose_only = "// fn main(){for argument in std::env::args().skip(1){let _=argument;}}\nfn main(){}";
        assert!(!native_accepts_flag(prose_only, "--previous-controller"), "arguments named only in a comment are not the group's own arguments");
    }

    // The pre-flight and the argument builder read the same authority, so the
    // sentence a twin prints when the argument is missing is read from the
    // group's own literal and the same sentence in a comment demands nothing.
    #[test]
    fn a_required_phrase_in_a_comment_never_decides_a_native_required_input() {
        // The missing reference this row measures is the one in the tree; when
        // the environment names another one the pre-flight would answer for
        // the environment, so the row reports that it did not measure.
        if !no_environment_reference_is_configured(&["--previous-controller"]) {
            return;
        }
        let dir = TempDir::new();
        let root = dir.path();
        let stated = "fn main(){panic!(\"the following arguments are required: --previous-controller\");}";
        let inputs = required_inputs(root, stated, SourceLanguage::Rust);
        assert!(inputs.iter().any(|input| input.kind == "path" && !input.present && input.detail.contains("--previous-controller reference at output/previous-controller")), "a native twin's own diagnostic is a required input: {:?}", inputs.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
        let commented = "// the following arguments are required: --previous-controller\nfn main(){for argument in std::env::args().skip(1){let _=argument;}}";
        let inputs = required_inputs(root, commented, SourceLanguage::Rust);
        assert!(!inputs.iter().any(|input| input.detail.contains("--previous-controller")), "the same sentence in a comment is prose and demands nothing: {:?}", inputs.iter().map(|input| input.detail.clone()).collect::<Vec<String>>());
    }

    // A Rust block comment nests, so the comment that opens here closes only
    // at the match that closes it. The text between an outer open and its own
    // close is comment even when it holds another comment, so a flag token or
    // a required-input phrase written there is prose and decides nothing.
    #[test]
    fn a_token_inside_two_nested_block_comments_decides_nothing() {
        let dir = TempDir::new();
        let root = dir.path();
        // The shape that was measured: the group reads its own arguments and
        // the only mention of the flag sits in the outer of two nested block
        // comments.
        let nested = "/* outer /* inner */ \"--previous-controller\" and the following arguments are required: --previous-controller */\nfn main(){for argument in std::env::args().skip(1){let _=argument;}}";
        assert!(nested.contains("/* inner */"), "the fixture nests a second comment inside the first");
        assert!(!native_accepts_flag(nested, "--previous-controller"), "a quoted token in the outer of two nested block comments is prose, not the group's grammar");
        let (arguments, unexercised) = group_arguments(root, nested, SourceLanguage::Rust);
        assert!(arguments.is_empty(), "a group whose only mention is a nested comment is dispatched nothing: {arguments:?}");
        assert!(unexercised.is_empty(), "a group whose only mention is a nested comment has no reference leg to report: {unexercised:?}");
        assert!(!required_inputs(root, nested, SourceLanguage::Rust).iter().any(|input| input.detail.contains("previous-controller")), "a required-input phrase in the outer of two nested block comments demands nothing");
        // The group's own parser arm is a string literal and is found.
        let armed = "fn main(){for argument in std::env::args().skip(1){match argument.as_str(){\"--previous-controller\"=>{}}}}";
        assert!(native_accepts_flag(armed, "--previous-controller"), "a parser arm is a string literal and is found");
        // The same phrase in the group's own literal is read.
        let stated = "fn main(){panic!(\"the following arguments are required: --previous-controller\");}";
        assert!(required_inputs(root, stated, SourceLanguage::Rust).iter().any(|input| input.detail.contains("--previous-controller")), "the same phrase in the group's own literal is read");
    }

    // A row whose premise is absent says that it did not measure and returns.
    // An ambient value outside the product therefore leaves the rows
    // unmeasured instead of failing the sanctioned build of the runner.
    #[test]
    fn a_row_whose_premise_is_absent_reports_that_it_did_not_measure() {
        assert!(!environment_reference_is_configured(None), "an unset reference is not a configured one");
        assert!(!environment_reference_is_configured(Some("")), "a blank reference is not a configured one");
        assert!(!environment_reference_is_configured(Some("  \t\n")), "a blank reference is not a configured one");
        assert!(environment_reference_is_configured(Some("/tmp/other-tree/previous-controller")), "a reference outside the tree is a configured one");
    }

    #[test]
    fn a_native_environment_read_is_required_only_when_its_value_is_consumed_by_a_panic() {
        let dir = TempDir::new();
        let root = dir.path();
        let hard = "fn main(){let tools = std::env::var_os(\"CLIENT_NDK\")\n    .expect(\"CLIENT_NDK must name the sanctioned toolchain\");}";
        let inputs = required_inputs(root, hard, SourceLanguage::Rust);
        let input = inputs
            .iter()
            .find(|input| input.detail.contains("CLIENT_NDK"))
            .expect("a native read of a required variable is an input");
        assert_eq!(input.kind, "env", "the input names the environment");
        let expected = std::env::var("CLIENT_NDK").map(|value| !value.trim().is_empty()).unwrap_or(false);
        assert_eq!(input.present, expected, "the input tracks the variable the group reads");
        let chained = "fn main(){let tools = std::env::var_os(\"CLIENT_NDK\").map(std::path::PathBuf::from).unwrap_or_else(|| panic!(\"CLIENT_NDK is not set\"));}";
        assert!(required_inputs(root, chained, SourceLanguage::Rust).iter().any(|input| input.detail.contains("CLIENT_NDK")), "a panicking fallback is a required read");
        let guarded = "fn main(){if let Some(root) = std::env::var_os(\"CLIENT_NDK\") { run(root); }}";
        assert!(!required_inputs(root, guarded, SourceLanguage::Rust).iter().any(|input| input.detail.contains("CLIENT_NDK")), "a guarded read stays runnable");
        let defaulted = "fn main(){let tools = std::env::var(\"CLIENT_NDK\").unwrap_or_default();}";
        assert!(!required_inputs(root, defaulted, SourceLanguage::Rust).iter().any(|input| input.detail.contains("CLIENT_NDK")), "a defaulted read stays runnable");
        let prose = "fn main(){/* std::env::var_os(\"CLIENT_NDK\") */ let tools = \"\";}";
        assert!(!required_inputs(root, prose, SourceLanguage::Rust).iter().any(|input| input.detail.contains("CLIENT_NDK")), "a read written in a comment is not a read");
    }

    // The read of an environment variable is read from the group's own code,
    // so a read written in the outer of two nested block comments is prose and
    // is not a read.
    #[test]
    fn a_read_inside_two_nested_block_comments_is_not_a_read() {
        let dir = TempDir::new();
        let root = dir.path();
        let nested = "fn main(){/* outer /* inner */ std::env::var_os(\"CLIENT_NDK\").expect(\"CLIENT_NDK must name the sanctioned toolchain\"); */ }";
        assert!(!required_inputs(root, nested, SourceLanguage::Rust).iter().any(|input| input.detail.contains("CLIENT_NDK")), "a read written in the outer of two nested block comments is not a read");
        // The same read in the group's own code is a read.
        let code = "fn main(){let tools = std::env::var_os(\"CLIENT_NDK\").expect(\"CLIENT_NDK must name the sanctioned toolchain\");}";
        assert!(required_inputs(root, code, SourceLanguage::Rust).iter().any(|input| input.detail.contains("CLIENT_NDK")), "the same read in the group's own code is a read");
    }

    // A declaration written in a comment is prose: it declares nothing and
    // requires nothing. The declaration is located where the group's own code
    // writes it, so a comment that mentions the declaration first does not
    // become the declaration the required keyword is read from.
    #[test]
    fn a_declaration_in_a_comment_is_not_a_declaration() {
        let dir = TempDir::new();
        let root = dir.path();
        // A flat block comment.
        let commented = "/* args.add_argument('--previous-controller', required=True) */\nfn main(){}";
        assert!(!declares_argument(commented, "--previous-controller", SourceLanguage::Rust), "a declaration in a block comment declares no argument");
        assert!(!declares_required(commented, "--previous-controller", SourceLanguage::Rust), "a declaration in a block comment requires nothing");
        assert!(!required_inputs(root, commented, SourceLanguage::Rust).iter().any(|input| input.detail.contains("previous-controller")), "a declaration in a block comment demands nothing");
        // The outer of two nested block comments.
        let nested = "/* outer /* inner */ args.add_argument('--previous-controller', required=True) */\nfn main(){}";
        assert!(!declares_argument(nested, "--previous-controller", SourceLanguage::Rust), "a declaration in the outer of two nested block comments declares no argument");
        assert!(!declares_required(nested, "--previous-controller", SourceLanguage::Rust), "a declaration in the outer of two nested block comments requires nothing");
        assert!(!required_inputs(root, nested, SourceLanguage::Rust).iter().any(|input| input.detail.contains("previous-controller")), "a declaration in the outer of two nested block comments demands nothing");
        // The group's own code declares the argument it declares.
        let declared = "fn main(){ add_argument('--previous-controller', required=True); }";
        assert!(declares_argument(declared, "--previous-controller", SourceLanguage::Rust), "the group's own code declares the argument");
        assert!(declares_required(declared, "--previous-controller", SourceLanguage::Rust), "the group's own code requires it");
        // A comment that mentions the declaration first does not become the
        // declaration the required keyword is read from.
        let mentioned = "// args.add_argument('--previous-controller', required=True)\nfn main(){ add_argument('--previous-controller', required=True); }";
        assert!(declares_argument(mentioned, "--previous-controller", SourceLanguage::Rust), "the group's own code declares the argument");
        assert!(declares_required(mentioned, "--previous-controller", SourceLanguage::Rust), "the required keyword is read from the declaration in the group's own code");
    }

    #[test]
    fn the_resource_tool_is_satisfied_by_the_copy_that_carries_it() {
        let dir = TempDir::new();
        let root = dir.path();
        let source = "fn main(){ let tools = std::env::var_os(\"CLIENT_TOOLS\").map(std::path::PathBuf::from).unwrap_or_else(|| project.join(\"tooling/buildtools/android-15\")); }";
        let reads = rust_env_reads(source);
        let missing = resource_tool_input(root, source, &reads, None)
            .expect("the resource tools are an input of the group");
        assert!(!missing.present, "without a copy the tool is missing");
        assert!(missing.detail.contains("CLIENT_TOOLS"), "the input names the variable");
        assert!(missing.detail.contains(RESOURCE_TOOLS), "the input names the in-project copy");
        std::fs::create_dir_all(root.join(RESOURCE_TOOLS)).unwrap();
        std::fs::write(root.join(RESOURCE_TOOLS).join(RESOURCE_TOOL), b"tool").unwrap();
        let in_project = resource_tool_input(root, source, &reads, None)
            .expect("the resource tools are an input of the group");
        assert!(in_project.present, "the in-project copy satisfies the dependency");
        let empty = TempDir::new();
        let exported = resource_tool_input(root, source, &reads, Some(empty.path().to_str().unwrap()))
            .expect("the resource tools are an input of the group");
        assert!(exported.present, "the in-project copy still satisfies the dependency");
        let without = resource_tool_input(empty.path(), source, &reads, None)
            .expect("the resource tools are an input of the group");
        assert!(!without.present, "a tree without the tool leaves the dependency missing");
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
        let blocked: Vec<Input> = required_inputs(root, &read(&script), SourceLanguage::Python).into_iter().filter(|input| !input.present).collect();
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

    // The sweep credits the report the group publishes. A report left behind by
    // an earlier run must never stand in for a group that published nothing, so
    // the sweep removes the expected report before the group runs.
    #[test]
    fn a_leftover_report_is_not_credited_as_this_runs_measurement() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        std::fs::create_dir_all(root.join("output/verification")).unwrap();
        // The group runs, exits 0 and publishes no report of its own.
        let twin = root.join("output/verification/scene");
        std::fs::write(&twin, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&twin, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The leftover of an earlier run carries a count of its own, and its
        // modification time is older than the run that follows it.
        let report = root.join("output/checks").join(group_report_file("scene"));
        std::fs::write(&report, "{\"passed\": 4242}\n").unwrap();
        let earlier = SystemTime::now() - std::time::Duration::from_secs(1);
        std::fs::OpenOptions::new().write(true).open(&report).unwrap().set_modified(earlier).unwrap();
        let row = sweep_one(root, "scene", &bare_opts(), SECONDS);
        assert_ne!(row.checks, Some(4242), "the sweep credited a leftover report");
        assert!(row.checks.is_none(), "a group that published nothing has no count");
        assert!(row.report.is_none(), "no report text is carried over from a leftover");
        assert!(!report.exists(), "the leftover report outlived the run that had to clear it");
    }

    // The other half of the same rule: a group that publishes its own report
    // while it runs is credited with that report, and the report it published
    // is still on disk after the run.
    #[test]
    fn a_report_published_during_the_run_is_credited_and_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        std::fs::create_dir_all(root.join("output/verification")).unwrap();
        // The group publishes its own report while it runs, and exits 0.
        let report_name = group_report_file("scene");
        let twin = root.join("output/verification/scene");
        std::fs::write(&twin, format!("#!/bin/sh\nmkdir -p output/checks\nprintf '{{\"passed\": 7}}\\n' > output/checks/{report_name}\nexit 0\n")).unwrap();
        std::fs::set_permissions(&twin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let report = root.join("output/checks").join(&report_name);
        let row = sweep_one(root, "scene", &bare_opts(), SECONDS);
        assert_eq!(row.code, Some(0), "the group ran and exited 0");
        assert_eq!(row.checks, Some(7), "the count came from the report the group published");
        assert!(report.exists(), "the published report is still on disk after the run");
        assert_eq!(std::fs::read_to_string(&report).unwrap(), "{\"passed\": 7}\n", "the report on disk is the one the group published");
        assert!(row.report.is_some(), "the published report is carried into the row");
    }

    // A group whose development input is missing is never executed, so it
    // cannot have published a report. A report left by an earlier run must not
    // be credited to it either.
    #[test]
    fn an_unavailable_group_is_not_credited_a_stale_report() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        std::fs::create_dir_all(root.join("verification")).unwrap();
        // The development input the group needs is absent, so it cannot run.
        std::fs::write(root.join("verification/scene.py"), "raise SystemExit('output/tools/gate')\n").unwrap();
        // A report of an earlier run is on disk and carries a count of its own.
        let report = root.join("output/checks").join(group_report_file("scene"));
        std::fs::write(&report, "{\"passed\": 4242}\n").unwrap();
        let row = sweep_one(root, "scene", &bare_opts(), SECONDS);
        assert!(row.code.is_none(), "a group with a missing development input is not executed");
        assert_eq!(row.checks, Some(0), "a group that did not run is recorded as 0");
        assert!(row.report.is_none(), "no report text is carried for a group that did not run");
        // The delivered record counts the group as 0, not with the count of the
        // report that was left behind.
        let mut rows = pass_rows();
        *rows.iter_mut().find(|item| item.name == "scene").unwrap() = row;
        let record = plant_record(root);
        assert!(commit_sweep(root, &rows), "an unavailable sweep still publishes");
        let text = std::fs::read_to_string(&record).unwrap();
        assert!(text.contains("\"scene\": 0"), "the record counts the unexecuted group as 0");
        assert!(!text.contains("4242"), "the record did not take the count of the leftover report");
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

    #[test]
    fn unknown_group_is_refused_and_names_the_three_declared_categories() {
        let line = requested_group_refusal("zzz_nogroup").unwrap_or_else(|| panic!("undeclared name zzz_nogroup was accepted"));
        assert_eq!(line, "unknown group zzz_nogroup (not a runner group, a declared helper or a declared optional group)");
        assert!(line.contains("runner group"), "{line}");
        assert!(line.contains("declared helper"), "{line}");
        assert!(line.contains("declared optional group"), "{line}");
    }

    #[test]
    fn live_web_is_refused_with_the_optional_reason() {
        let reason = OPTIONAL.iter().find(|(name, _)| *name == "live_web").map(|(_, reason)| *reason).unwrap_or_else(|| panic!("live_web missing from OPTIONAL"));
        match classify_requested_group("live_web") {
            RequestedGroup::DeclaredNotExecutable { kind, reason: got } => {
                assert_eq!(kind, "optional group");
                assert!(std::ptr::eq(got, reason), "reason must be the OPTIONAL string, not a second copy");
                assert_eq!(got, reason);
            }
            RequestedGroup::Executable => panic!("live_web was accepted as executable"),
            RequestedGroup::Unknown => panic!("live_web was treated as unknown"),
        }
        assert_eq!(
            requested_group_refusal("live_web").as_deref(),
            Some(format!("live_web NOT EXECUTED (optional group: {reason})")).as_deref()
        );
    }

    #[test]
    fn gateway_expectation_stays_executable() {
        assert!(matches!(classify_requested_group("gateway_expectation"), RequestedGroup::Executable), "a runner group must stay executable");
        assert!(requested_group_refusal("gateway_expectation").is_none(), "a runner group must not be refused");
    }

    #[test]
    fn gateway_prefix_is_refused() {
        assert!(!GROUPS.iter().any(|name| *name == "gateway"), "gateway itself must not be a runner group");
        assert!(GROUPS.iter().any(|name| *name == "gateway_expectation" && name.starts_with("gateway") && *name != "gateway"));
        let line = requested_group_refusal("gateway").unwrap_or_else(|| panic!("prefix gateway was accepted"));
        assert_eq!(line, "unknown group gateway (not a runner group, a declared helper or a declared optional group)");
    }

    fn rendered_regression(codes: &[i32]) -> String {
        assert_eq!(codes.len(), GROUPS.len(), "rendered row count");
        let mut body = String::from("[\n");
        for (index, (name, code)) in GROUPS.iter().zip(codes.iter()).enumerate() {
            if index > 0 {
                body.push_str(",\n");
            }
            body.push_str("  {\n    \"name\": \"");
            body.push_str(&json_escape(name));
            body.push_str("\",\n    \"code\": ");
            body.push_str(&code.to_string());
            body.push_str("\n  }");
        }
        body.push_str("\n]\n");
        body
    }

    fn regression_path(root: &Path) -> PathBuf {
        root.join("output/checks/current-regression.json")
    }

    // tools/package.rs::load_runs is a separate binary, so this crate cannot call
    // it. The parser applies that reader's shape: an array of objects, a name
    // string, and an integer code. It also requires the name to be non-empty and
    // the keys to be exactly name then code, which is the object exact_binary_flow
    // counts. A non-array, a non-object row, a missing name, a non-integer code,
    // or any other key fails.
    fn publisher_regression_rows(text: &str) -> Result<Vec<(String, i64)>, String> {
        let mut scan = Scan { text: text.as_bytes(), i: 0 };
        scan.skip_ws();
        if !scan.eat(b'[') {
            return Err("not an array".into());
        }
        let mut rows = Vec::new();
        loop {
            scan.skip_ws();
            if scan.eat(b']') {
                break;
            }
            if !rows.is_empty() && !scan.eat(b',') {
                return Err("row was not an object".into());
            }
            scan.skip_ws();
            if !scan.eat(b'{') {
                return Err("row was not an object".into());
            }
            let mut name: Option<String> = None;
            let mut code: Option<i64> = None;
            let mut keys = Vec::new();
            loop {
                scan.skip_ws();
                if scan.eat(b'}') {
                    if keys.is_empty() {
                        break;
                    }
                    return Err("trailing comma".into());
                }
                let key = scan.key().ok_or_else(|| "missing name".to_string())?;
                scan.skip_ws();
                if !scan.eat(b':') {
                    return Err("row was not an object".into());
                }
                scan.skip_ws();
                match key.as_str() {
                    "name" => {
                        if scan.peek() != Some(b'"') {
                            return Err("name was not a string".into());
                        }
                        let start = scan.i;
                        scan.skip_string().ok_or_else(|| "name was not a string".to_string())?;
                        let end = scan.i;
                        let raw = std::str::from_utf8(&scan.text[start + 1..end - 1]).map_err(|_| "name was not a string".to_string())?;
                        if raw.is_empty() {
                            return Err("name was empty".into());
                        }
                        name = Some(raw.to_string());
                    }
                    "code" => match scan.take_executed_int() {
                        Some(Some(value)) => code = Some(value),
                        _ => return Err("code was not an integer".into()),
                    },
                    other => return Err(format!("unexpected key {other}")),
                }
                keys.push(key);
                scan.skip_ws();
                if scan.eat(b',') {
                    continue;
                }
                if scan.eat(b'}') {
                    break;
                }
                return Err("row was not an object".into());
            }
            if keys.len() != 2 || keys[0] != "name" || keys[1] != "code" {
                return Err(format!("keys were {keys:?}"));
            }
            let name = name.ok_or_else(|| "missing name".to_string())?;
            let code = code.ok_or_else(|| "code was not an integer".to_string())?;
            rows.push((name, code));
        }
        scan.skip_ws();
        if scan.i != scan.text.len() {
            return Err("trailing bytes".into());
        }
        Ok(rows)
    }

    #[test]
    fn regression_record_names_every_group_once_in_groups_order() {
        let dir = TempDir::new();
        let root = dir.path();
        plant_record(root);
        let code = finish_sweep(root, &pass_rows()).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(code, 0, "a passing sweep still exits 0");
        let path = regression_path(root);
        let first = std::fs::read(&path).unwrap();
        let expected = rendered_regression(&vec![0; GROUPS.len()]);
        assert_eq!(String::from_utf8(first.clone()).unwrap(), expected, "regression bytes");
        assert_eq!(finish_sweep(root, &pass_rows()).unwrap(), 0, "second publish");
        assert_eq!(std::fs::read(&path).unwrap(), first, "same rows rewrote the bytes");
        let mut reversed = pass_rows();
        reversed.reverse();
        assert_eq!(finish_sweep(root, &reversed).unwrap(), 0, "reversed input");
        assert_eq!(std::fs::read(&path).unwrap(), first, "row order followed the input instead of GROUPS");
        let parsed = publisher_regression_rows(&expected).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(parsed.len(), GROUPS.len(), "row count");
        let mut seen = BTreeSet::new();
        for (index, (name, code)) in parsed.iter().enumerate() {
            assert_eq!(name, GROUPS[index], "group order");
            assert_eq!(*code, 0, "{name} code");
            assert!(seen.insert(name.clone()), "duplicate {name}");
        }
    }

    #[test]
    fn regression_record_maps_pass_failure_and_absent_codes() {
        let dir = TempDir::new();
        let root = dir.path();
        let mut rows = pass_rows();
        let slot = rows.iter_mut().find(|row| row.name == "history").unwrap();
        slot.code = Some(9);
        slot.checks = Some(4);
        let slot = rows.iter_mut().find(|row| row.name == "binary_flow").unwrap();
        slot.code = None;
        slot.checks = Some(0);
        slot.report = None;
        let slot = rows.iter_mut().find(|row| row.name == "apk_alignment").unwrap();
        slot.code = Some(0);
        slot.checks = None;
        slot.report = None;
        let slot = rows.iter_mut().find(|row| row.name == "turn_background").unwrap();
        slot.code = Some(0);
        slot.checks = Some(0);
        let slot = rows.iter_mut().find(|row| row.name == "history_migration").unwrap();
        slot.code = Some(-1);
        slot.checks = None;
        let slot = rows.iter_mut().find(|row| row.name == "scene").unwrap();
        slot.code = Some(124);
        slot.checks = None;
        let code = finish_sweep(root, &rows).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(code, 1, "a failing sweep still exits 1");
        let text = std::fs::read_to_string(regression_path(root)).unwrap();
        let parsed = publisher_regression_rows(&text).unwrap_or_else(|err| panic!("{err}"));
        let code_of = |name: &str| parsed.iter().find(|(got, _)| got == name).map(|(_, code)| *code).unwrap();
        assert_eq!(code_of("provider_process"), 0, "a pass row");
        assert_eq!(code_of("history"), 9, "measured non-zero exit");
        assert_eq!(code_of("binary_flow"), i64::from(REGRESSION_NOT_PASSED), "unavailable code None");
        assert_ne!(code_of("binary_flow"), 0, "unavailable was recorded as a pass");
        assert_ne!(code_of("binary_flow"), 1, "unavailable binary_flow looked like the tolerated gap");
        assert_eq!(code_of("apk_alignment"), i64::from(REGRESSION_NOT_PASSED), "exit 0 with absent checks");
        assert_ne!(code_of("apk_alignment"), 0, "absent checks were recorded as a pass");
        assert_eq!(code_of("turn_background"), i64::from(REGRESSION_NOT_PASSED), "exit 0 with zero checks");
        assert_ne!(code_of("turn_background"), 0, "zero checks were recorded as a pass");
        assert_eq!(code_of("history_migration"), -1, "measured negative exit");
        assert_eq!(code_of("scene"), 124, "measured timeout exit");
        assert_eq!(REGRESSION_NOT_PASSED, 2, "not-passed sentinel");
        let mut codes = vec![0i32; GROUPS.len()];
        let set = |codes: &mut [i32], name: &str, value: i32| {
            codes[GROUPS.iter().position(|group| *group == name).unwrap()] = value;
        };
        set(&mut codes, "history", 9);
        set(&mut codes, "binary_flow", REGRESSION_NOT_PASSED);
        set(&mut codes, "apk_alignment", REGRESSION_NOT_PASSED);
        set(&mut codes, "turn_background", REGRESSION_NOT_PASSED);
        set(&mut codes, "history_migration", -1);
        set(&mut codes, "scene", 124);
        assert_eq!(text, rendered_regression(&codes), "verdict bytes");
    }

    #[test]
    fn regression_record_matches_the_publisher_row_shape() {
        let dir = TempDir::new();
        let root = dir.path();
        let mut rows = pass_rows();
        let slot = rows.iter_mut().find(|row| row.name == "history").unwrap();
        slot.code = Some(-3);
        slot.checks = None;
        finish_sweep(root, &rows).unwrap_or_else(|err| panic!("{err}"));
        let text = std::fs::read_to_string(regression_path(root)).unwrap();
        let parsed = publisher_regression_rows(&text).unwrap_or_else(|err| panic!("publisher shape refused the harness bytes: {err}"));
        assert_eq!(parsed.len(), GROUPS.len(), "row count");
        let mut seen = BTreeSet::new();
        for (index, (name, code)) in parsed.iter().enumerate() {
            assert_eq!(name, GROUPS[index], "group order");
            assert!(!name.is_empty(), "empty name");
            assert!(seen.insert(name.clone()), "duplicate {name}");
            if *name == "history" {
                assert_eq!(*code, -3, "integer code");
            }
        }
        assert!(publisher_regression_rows(r#"{"name":"history","code":0}"#).is_err(), "a non-array was accepted");
        assert!(publisher_regression_rows("[1]\n").is_err(), "a non-object row was accepted");
        assert!(publisher_regression_rows("[{\"code\":0}]\n").is_err(), "a missing name was accepted");
        assert!(publisher_regression_rows("[{\"name\":\"\",\"code\":0}]\n").is_err(), "an empty name was accepted");
        assert!(publisher_regression_rows("[{\"name\":\"history\",\"code\":1.5}]\n").is_err(), "a non-integer code was accepted");
        assert!(publisher_regression_rows("[{\"name\":\"history\",\"code\":0,\"reason\":\"x\"}]\n").is_err(), "an extra key was accepted");
    }

    #[test]
    fn regression_record_refuses_a_missing_group_and_keeps_the_prior_file() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::create_dir_all(root.join("output/checks")).unwrap();
        let path = regression_path(root);
        let prior = b"[\n  {\n    \"name\": \"prior\",\n    \"code\": 9\n  }\n]\n";
        std::fs::write(&path, prior).unwrap();
        let before = std::fs::read(&path).unwrap();
        let rows: Vec<_> = pass_rows().into_iter().filter(|row| row.name != "history").collect();
        let err = finish_sweep(root, &rows).err();
        assert_eq!(
            err.as_deref(),
            Some("current-regression.json refused: group history has no row"),
            "regression writer refused without naming the missing group"
        );
        let after = std::fs::read(&path).unwrap();
        assert_eq!(after, before, "prior regression record changed");
        assert_eq!(after, prior, "prior bytes");
        assert!(!root.join("output/checks/current-regression.json.new").exists(), "temporary regression file was left behind");
        assert!(!root.join("verification/RESULTS.json").exists(), "a refused regression write still published the delivered record");
    }
}

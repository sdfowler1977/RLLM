// Native unit gate.
// `gate <unit-binary>` is the full-suite role. Its gate line is what
// core/self_build.rs::parse_gate_line reads, so that argv stays one path.
// `gate --units <unit-binary>` is the selected-unit role: verification/RESULTS.json,
// the pinned filter merge, one child per filter, output/checks/selected-units.json.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Captured {
    code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
struct Failure {
    name: String,
    code: i32,
    details: String,
}

#[derive(Debug)]
struct GateRun {
    cases: usize,
    passed: u64,
    isolated: usize,
    font: String,
    failures: Vec<Failure>,
    binary_sha256: String,
}

impl GateRun {
    fn line(&self) -> String {
        format!(
            "Rust cases passed: {}/{}; isolated application cases: {}",
            self.passed, self.cases, self.isolated
        )
    }
    fn exit_code(&self) -> i32 {
        if self.failures.is_empty() { 0 } else { 1 }
    }
    fn font_label(&self) -> String {
        if self.font.is_empty() { "unavailable".to_string() } else { self.font.clone() }
    }
}

fn sha256(data: &[u8]) -> [u8; 32] {
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
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut message = Vec::with_capacity(data.len() + 72);
    message.extend_from_slice(data);
    let bits = (data.len() as u64).saturating_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for chunk in message.chunks_exact(64) {
        let mut words = [0u32; 64];
        for index in 0..16 {
            words[index] = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let t1 = h.wrapping_add(s1).wrapping_add(choice).wrapping_add(K[index]).wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) >= 0x7f => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn sort_fonts(paths: &mut [PathBuf]) {
    paths.sort_by(|left, right| left.components().cmp(right.components()));
}

fn collect_ttf(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            if name.ends_with(".ttf") {
                found.push(path);
            }
            continue;
        }
        if meta.is_dir() {
            collect_ttf(&path, found);
        } else if name.ends_with(".ttf") {
            found.push(path);
        }
    }
}

fn select_font(configured: &str, found: &[PathBuf]) -> String {
    if !configured.is_empty() && Path::new(configured).is_file() {
        return configured.to_string();
    }
    found.first().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
}

fn capture_font() -> String {
    let configured = env::var("CLIENT_CAPTURE_TEST_FONT").unwrap_or_default();
    let mut found = Vec::new();
    if configured.is_empty() || !Path::new(&configured).is_file() {
        collect_ttf(Path::new("/usr/share/fonts"), &mut found);
        sort_fonts(&mut found);
    }
    select_font(&configured, &found)
}

fn take_count<'a>(text: &'a str, suffix: &str) -> Option<(u64, &'a str)> {
    let end = text.find(suffix)?;
    if end == 0 || !text[..end].bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let count = text[..end].parse().ok()?;
    Some((count, &text[end + suffix.len()..]))
}

fn summary_counts(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.strip_prefix("ok. ").or_else(|| text.strip_prefix("FAILED. "))?;
    let (passed, text) = take_count(text, " passed; ")?;
    let (failed, text) = take_count(text, " failed; ")?;
    let (ignored, _) = take_count(text, " ignored;")?;
    Some((passed, failed, ignored))
}

fn parse_summary(stdout: &str) -> Option<(u64, u64, u64)> {
    let mut rest = stdout;
    while let Some(index) = rest.find("test result: ") {
        let after = &rest[index + "test result: ".len()..];
        if let Some(counts) = summary_counts(after) {
            return Some(counts);
        }
        rest = &rest[index + "test result: ".len()..];
    }
    None
}

fn tail_chars(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        text.to_string()
    } else {
        text.chars().skip(count - limit).collect()
    }
}

fn read_tail(mut pipe: impl Read, cap: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > cap {
                    let extra = buf.len() - cap;
                    buf.drain(..extra);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    buf
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        code
    } else {
        -status.signal().unwrap_or(1)
    }
}

// execve can return ETXTBSY, os error 26, for a file that was just closed.
// Retry only that error, at most five times, with a few milliseconds between tries.
fn executable_file_busy(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::ExecutableFileBusy || error.raw_os_error() == Some(26)
}

fn retry_executable_file_busy<T>(mut attempt: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    const ATTEMPTS: u32 = 5;
    let mut tried = 0u32;
    loop {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => {
                tried += 1;
                if tried >= ATTEMPTS || !executable_file_busy(&error) {
                    return Err(error);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn run_cmd(binary: &Path, args: &[String], cwd: &Path, font: Option<&str>, timeout: Duration) -> Result<Captured, String> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(font) = font {
        if !font.is_empty() {
            command.env("CLIENT_CAPTURE_TEST_FONT", font);
        }
    }
    let mut child = retry_executable_file_busy(|| command.spawn())
        .map_err(|error| format!("cannot start {}: {error}", binary.display()))?;
    let stdout = child.stdout.take().ok_or("unit gate stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("unit gate stderr is unavailable")?;
    let out_handle = std::thread::spawn(move || read_tail(stdout, 32 * 1024 * 1024));
    let err_handle = std::thread::spawn(move || read_tail(stderr, 32 * 1024 * 1024));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(format!("{} exceeded {}s", binary.display(), timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    };
    let stdout = String::from_utf8_lossy(&out_handle.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_handle.join().unwrap_or_default()).into_owned();
    Ok(Captured { code: exit_code(status), stdout, stderr })
}

fn resolve_binary(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().map_err(|error| error.to_string())?.join(path)
    };
    absolute.canonicalize().map_err(|error| format!("unit binary {}: {error}", absolute.display()))
}

fn inventory(binary: &Path) -> Result<Vec<String>, String> {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let captured = run_cmd(
        binary,
        &["--list".to_string(), "--format".to_string(), "terse".to_string()],
        &cwd,
        None,
        Duration::from_secs(30),
    )?;
    if captured.code != 0 {
        return Err(format!("unit inventory exited {}", captured.code));
    }
    let mut cases = Vec::new();
    for line in captured.stdout.lines() {
        if let Some(case) = line.strip_suffix(": test") {
            cases.push(case.to_string());
        }
    }
    if cases.is_empty() {
        return Err("unit inventory is empty".into());
    }
    let mut seen = BTreeSet::new();
    for case in &cases {
        if !seen.insert(case.as_str()) {
            return Err("unit inventory contains a duplicate case".into());
        }
    }
    Ok(cases)
}

struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch_dir(out: &Path) -> Result<Remove, String> {
    fs::create_dir_all(out).map_err(|error| error.to_string())?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    for salt in 0..1000 {
        let path = out.join(format!("rust-{}-{nanos}-{salt}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(Remove(path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("could not create a unit-gate scratch directory".into())
}

// These filters are the selected-unit authority. merge_pinned appends any the
// prior record omitted. checks::pinned_filters_are_the_selected_unit_set
// asserts this list; a drift fails the tools suite instead of diverging quietly.
const PINNED_FILTERS: &[&str] = &[
    "network::etag_checks::",
    "network::chunk_checks::",
    "network::response_checks::",
    "network::field_checks::",
    "network::authorization_response_checks::",
    "application::scene::checks::",
    "application::scene::scene_texture::checks::",
    "application::tool_format::tests::",
    "application::search::checks::",
    "application::oauth::checks::",
    "application::oauth::credential_checks::",
    "application::inquiry::design_brief::checks::",
    "application::oauth::device_authorization::checks::",
    "application::voice_audio::checks::",
    "application::monitor::checks::",
    "application::request_metrics::",
    "application::throughput::",
    "application::checks::live_rate_opens_analytics_without_losing_stream_or_draft",
    "application::checks::tls_guidance_preserves_explicit_endpoint",
];

struct SelectedRow {
    filter: String,
    code: i32,
    summary: String,
}

struct SelectedReport {
    rows: Vec<SelectedRow>,
    passed: Option<u64>,
}

fn universal_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}

fn python_line_break(ch: char) -> bool {
    matches!(
        ch,
        '\n' | '\r' | '\u{000b}' | '\u{000c}' | '\u{001c}' | '\u{001d}' | '\u{001e}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

fn first_test_result_line(text: &str) -> Option<&str> {
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if !python_line_break(ch) {
            continue;
        }
        let line = &text[start..index];
        if line.starts_with("test result:") {
            return Some(line);
        }
        if ch == '\r' && chars.peek().map(|(_, next)| *next) == Some('\n') {
            let (next_index, _) = chars.next().unwrap();
            start = next_index + 1;
        } else {
            start = index + ch.len_utf8();
        }
    }
    if start < text.len() {
        let line = &text[start..];
        if line.starts_with("test result:") {
            return Some(line);
        }
    }
    None
}

fn summary_of(stdout: &str, stderr: &str) -> String {
    let stdout = universal_newlines(stdout);
    let stderr = universal_newlines(stderr);
    if let Some(line) = first_test_result_line(&stdout) {
        return line.to_string();
    }
    let mut summary = tail_chars(&stdout, 700);
    summary.push_str(&tail_chars(&stderr, 700));
    summary
}

fn passed_in_summary(summary: &str) -> Option<u64> {
    let bytes = summary.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if bytes.get(index..index + " passed".len()) == Some(b" passed") {
                return std::str::from_utf8(&bytes[start..index]).ok()?.parse().ok();
            }
        } else {
            index += 1;
        }
    }
    None
}

fn selected_total(rows: &[SelectedRow]) -> Option<u64> {
    if rows.iter().any(|row| row.code != 0) {
        return None;
    }
    let mut total = 0u64;
    for row in rows {
        total = total.checked_add(passed_in_summary(&row.summary)?)?;
    }
    Some(total)
}

fn selected_cases_line(passed: u64) -> String {
    format!("Selected cases: {passed}")
}

fn merge_pinned(mut prior: Vec<String>) -> Vec<String> {
    for name in PINNED_FILTERS {
        if !prior.iter().any(|existing| existing == name) {
            prior.push((*name).to_string());
        }
    }
    prior
}

fn render_selected(rows: &[SelectedRow]) -> String {
    if rows.is_empty() {
        return "[]\n".to_string();
    }
    let mut out = String::from("[\n");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("  {\n    \"filter\": ");
        out.push_str(&json_string(&row.filter));
        out.push_str(",\n    \"code\": ");
        out.push_str(&row.code.to_string());
        out.push_str(",\n    \"summary\": ");
        out.push_str(&json_string(&row.summary));
        out.push_str("\n  }");
    }
    out.push_str("\n]\n");
    out
}

struct Scan<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Scan<'a> {
    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }
    fn bump(&mut self) -> Result<u8, String> {
        let byte = self.peek().ok_or_else(|| "verification/RESULTS.json".to_string())?;
        self.index += 1;
        Ok(byte)
    }
    fn eat(&mut self, text: &[u8]) -> Result<(), String> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(())
        } else {
            Err("verification/RESULTS.json".into())
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.bump()?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err("verification/RESULTS.json".into()),
            };
            value = (value << 4) | u32::from(digit);
        }
        Ok(value)
    }
    fn string(&mut self) -> Result<String, String> {
        if self.bump()? != b'"' {
            return Err("verification/RESULTS.json".into());
        }
        let mut out = String::new();
        loop {
            let byte = self.peek().ok_or_else(|| "verification/RESULTS.json".to_string())?;
            if byte == b'"' {
                self.index += 1;
                return Ok(out);
            }
            if byte == b'\\' {
                self.index += 1;
                match self.bump()? {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let unit = self.hex4()?;
                        let code = if (0xD800..=0xDBFF).contains(&unit) {
                            if self.bump()? != b'\\' || self.bump()? != b'u' {
                                return Err("verification/RESULTS.json".into());
                            }
                            let low = self.hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err("verification/RESULTS.json".into());
                            }
                            0x10000 + (((unit - 0xD800) << 10) | (low - 0xDC00))
                        } else if (0xDC00..=0xDFFF).contains(&unit) {
                            return Err("verification/RESULTS.json".into());
                        } else {
                            unit
                        };
                        out.push(char::from_u32(code).ok_or_else(|| "verification/RESULTS.json".to_string())?);
                    }
                    _ => return Err("verification/RESULTS.json".into()),
                }
                continue;
            }
            if byte < 0x20 {
                return Err("verification/RESULTS.json".into());
            }
            let width = match byte {
                0x00..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => return Err("verification/RESULTS.json".into()),
            };
            let end = self.index + width;
            let text = std::str::from_utf8(self.bytes.get(self.index..end).ok_or_else(|| "verification/RESULTS.json".to_string())?)
                .map_err(|_| "verification/RESULTS.json".to_string())?;
            out.push_str(text);
            self.index = end;
        }
    }
    fn skip_number(&mut self) -> Result<(), String> {
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        if self.peek() == Some(b'0') {
            self.index += 1;
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("verification/RESULTS.json".into());
            }
        } else if matches!(self.peek(), Some(b'1'..=b'9')) {
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        } else {
            return Err("verification/RESULTS.json".into());
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let mark = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == mark {
                return Err("verification/RESULTS.json".into());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let mark = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == mark {
                return Err("verification/RESULTS.json".into());
            }
        }
        Ok(())
    }
    fn skip_value(&mut self) -> Result<(), String> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => self.eat(b"null"),
            Some(b't') => self.eat(b"true"),
            Some(b'f') => self.eat(b"false"),
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(b'{') => self.skip_object(),
            Some(b'[') => self.skip_array(),
            Some(b'-' | b'0'..=b'9') => self.skip_number(),
            _ => Err("verification/RESULTS.json".into()),
        }
    }
    fn skip_object(&mut self) -> Result<(), String> {
        self.index += 1;
        let mut started = false;
        loop {
            self.skip_ws();
            if self.peek() == Some(b'}') {
                self.index += 1;
                return Ok(());
            }
            if started && self.bump()? != b',' {
                return Err("verification/RESULTS.json".into());
            }
            started = true;
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err("verification/RESULTS.json".into());
            }
            self.string()?;
            self.skip_ws();
            if self.bump()? != b':' {
                return Err("verification/RESULTS.json".into());
            }
            self.skip_value()?;
        }
    }
    fn skip_array(&mut self) -> Result<(), String> {
        self.index += 1;
        let mut started = false;
        loop {
            self.skip_ws();
            if self.peek() == Some(b']') {
                self.index += 1;
                return Ok(());
            }
            if started && self.bump()? != b',' {
                return Err("verification/RESULTS.json".into());
            }
            started = true;
            self.skip_value()?;
        }
    }
    fn parse_string_array(&mut self) -> Result<Vec<String>, String> {
        self.skip_ws();
        if self.bump()? != b'[' {
            return Err("verification/RESULTS.json selected_rust_filters".into());
        }
        let mut names = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b']') {
                self.index += 1;
                return Ok(names);
            }
            if !names.is_empty() && self.bump()? != b',' {
                return Err("verification/RESULTS.json selected_rust_filters".into());
            }
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err("verification/RESULTS.json selected_rust_filters".into());
            }
            names.push(self.string()?);
        }
    }
    fn parse_run_object(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.bump()? != b'{' {
            return Err("verification/RESULTS.json selected_rust_runs".into());
        }
        let mut filter = None;
        let mut started = false;
        loop {
            self.skip_ws();
            if self.peek() == Some(b'}') {
                self.index += 1;
                return filter.ok_or_else(|| "verification/RESULTS.json filter".to_string());
            }
            if started && self.bump()? != b',' {
                return Err("verification/RESULTS.json selected_rust_runs".into());
            }
            started = true;
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err("verification/RESULTS.json selected_rust_runs".into());
            }
            let key = self.string()?;
            self.skip_ws();
            if self.bump()? != b':' {
                return Err("verification/RESULTS.json selected_rust_runs".into());
            }
            if key == "filter" {
                self.skip_ws();
                if self.peek() != Some(b'"') {
                    return Err("verification/RESULTS.json filter".into());
                }
                filter = Some(self.string()?);
            } else {
                self.skip_value()?;
            }
        }
    }
    fn parse_run_array(&mut self) -> Result<Vec<String>, String> {
        self.skip_ws();
        if self.bump()? != b'[' {
            return Err("verification/RESULTS.json selected_rust_runs".into());
        }
        let mut names = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b']') {
                self.index += 1;
                return Ok(names);
            }
            if !names.is_empty() && self.bump()? != b',' {
                return Err("verification/RESULTS.json selected_rust_runs".into());
            }
            names.push(self.parse_run_object()?);
        }
    }
}

fn prior_filters(text: &str) -> Result<Vec<String>, String> {
    let mut scan = Scan { bytes: text.as_bytes(), index: 0 };
    scan.skip_ws();
    if scan.peek() != Some(b'{') {
        return Err("verification/RESULTS.json".into());
    }
    scan.index += 1;
    let mut filters = None;
    let mut runs = None;
    let mut started = false;
    loop {
        scan.skip_ws();
        if scan.peek() == Some(b'}') {
            scan.index += 1;
            break;
        }
        if started && scan.bump()? != b',' {
            return Err("verification/RESULTS.json".into());
        }
        started = true;
        scan.skip_ws();
        if scan.peek() != Some(b'"') {
            return Err("verification/RESULTS.json".into());
        }
        let key = scan.string()?;
        scan.skip_ws();
        if scan.bump()? != b':' {
            return Err("verification/RESULTS.json".into());
        }
        match key.as_str() {
            "selected_rust_filters" => filters = Some(scan.parse_string_array()?),
            "selected_rust_runs" => runs = Some(scan.parse_run_array()?),
            _ => scan.skip_value()?,
        }
    }
    scan.skip_ws();
    if scan.index != scan.bytes.len() {
        return Err("verification/RESULTS.json extra data".into());
    }
    if let Some(filters) = filters {
        Ok(filters)
    } else if let Some(runs) = runs {
        Ok(runs)
    } else {
        Err("verification/RESULTS.json has no selected_rust_filters or selected_rust_runs".into())
    }
}

fn read_pipe(mut pipe: impl Read) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(buf),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn decode_pipe(bytes: Vec<u8>, filter: &str, which: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| format!("unit filter {filter} {which} is not utf-8"))
}

// The selected-unit run inherits the working directory and the environment.
// It does not use the full-suite scratch directory or the capture-font override.
fn capture_filter(binary: &Path, filter: &str) -> Result<Captured, String> {
    let mut command = Command::new(binary);
    command
        .arg(filter)
        .arg("--test-threads=1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = retry_executable_file_busy(|| command.spawn())
        .map_err(|error| format!("cannot start {}: {error}", binary.display()))?;
    let stdout = child.stdout.take().ok_or("selected unit stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("selected unit stderr is unavailable")?;
    let out_handle = std::thread::spawn(move || read_pipe(stdout));
    let err_handle = std::thread::spawn(move || read_pipe(stderr));
    let status = child.wait().map_err(|error| error.to_string())?;
    let stdout = decode_pipe(out_handle.join().map_err(|_| format!("unit filter {filter} stdout failed"))??, filter, "stdout")?;
    let stderr = decode_pipe(err_handle.join().map_err(|_| format!("unit filter {filter} stderr failed"))??, filter, "stderr")?;
    Ok(Captured { code: exit_code(status), stdout, stderr })
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, text).map_err(|error| error.to_string())
}

fn run_selected(root: &Path, binary: &Path) -> Result<SelectedReport, String> {
    let binary = resolve_binary(binary)?;
    let recorded = fs::read_to_string(root.join("verification/RESULTS.json"))
        .map_err(|error| format!("verification/RESULTS.json: {error}"))?;
    let prior = merge_pinned(prior_filters(&recorded)?);
    let mut rows = Vec::with_capacity(prior.len());
    for filter in &prior {
        let captured = capture_filter(&binary, filter)?;
        rows.push(SelectedRow {
            filter: filter.clone(),
            code: captured.code,
            summary: summary_of(&captured.stdout, &captured.stderr),
        });
    }
    let text = render_selected(&rows);
    write_text(&root.join("output/checks/selected-units.json"), &text)?;
    let passed = selected_total(&rows);
    Ok(SelectedReport { rows, passed })
}

fn finish_selected(report: &SelectedReport) -> i32 {
    for row in &report.rows {
        println!("{} {}", row.filter, row.code);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
    if let Some(passed) = report.passed {
        println!("{}", selected_cases_line(passed));
        0
    } else {
        1
    }
}

fn write_report(path: &Path, run: &GateRun) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let failures = if run.failures.is_empty() {
        "[]".to_string()
    } else {
        let mut body = String::from("[\n");
        for (index, failure) in run.failures.iter().enumerate() {
            if index > 0 {
                body.push_str(",\n");
            }
            body.push_str("    {\n");
            body.push_str(&format!("      \"name\": {},\n", json_string(&failure.name)));
            body.push_str(&format!("      \"code\": {},\n", failure.code));
            body.push_str(&format!("      \"details\": {}\n", json_string(&failure.details)));
            body.push_str("    }");
        }
        body.push_str("\n  ]");
        body
    };
    let text = format!(
        "{{\n  \"cases\": {cases},\n  \"passed\": {passed},\n  \"isolated_application_cases\": {isolated},\n  \"capture_font\": {font},\n  \"failures\": {failures},\n  \"binary_sha256\": {sha}\n}}\n",
        cases = run.cases,
        passed = run.passed,
        isolated = run.isolated,
        font = json_string(&run.font_label()),
        sha = json_string(&run.binary_sha256),
    );
    fs::write(path, text).map_err(|error| error.to_string())
}

fn run_gate(root: &Path, binary: &Path) -> Result<GateRun, String> {
    let binary = resolve_binary(binary)?;
    let cases = inventory(&binary)?;
    let prefix = "application::checks::";
    let isolated: Vec<&String> = cases.iter().filter(|case| case.starts_with(prefix)).collect();
    let mut batches: Vec<(String, Vec<String>, u64)> = isolated
        .iter()
        .map(|case| ((*case).clone(), vec![(*case).clone(), "--exact".to_string()], 1))
        .collect();
    let remaining = (cases.len() - isolated.len()) as u64;
    batches.push((
        "remaining modules".to_string(),
        vec!["--skip".to_string(), prefix.to_string()],
        remaining,
    ));
    let font = capture_font();
    let font_arg = if font.is_empty() { None } else { Some(font.as_str()) };
    let out = root.join("output/checks");
    let scratch = scratch_dir(&out)?;
    let mut passed = 0u64;
    let mut failures = Vec::new();
    for (index, (name, selection, expected)) in batches.iter().enumerate() {
        let work = scratch.0.join(index.to_string());
        fs::create_dir_all(work.join("toolchain/check-workspace")).map_err(|error| error.to_string())?;
        let mut args = selection.clone();
        args.push("--test-threads=1".to_string());
        let captured = run_cmd(&binary, &args, &work, font_arg, Duration::from_secs(120))?;
        let summary = parse_summary(&captured.stdout);
        if let Some((count, _, _)) = summary {
            passed += count;
        }
        let counts = summary.map(|(ok, failed, ignored)| (ok, failed, ignored));
        if captured.code != 0 || counts != Some((*expected, 0, 0)) {
            failures.push(Failure {
                name: name.clone(),
                code: captured.code,
                details: tail_chars(&format!("{}{}", captured.stdout, captured.stderr), 5000),
            });
        }
    }
    let binary_sha256 = hex_digest(&sha256(&fs::read(&binary).map_err(|error| error.to_string())?));
    let run = GateRun {
        cases: cases.len(),
        passed,
        isolated: isolated.len(),
        font,
        failures,
        binary_sha256,
    };
    write_report(&out.join("full-units.json"), &run)?;
    Ok(run)
}

fn project_root() -> PathBuf {
    let mut starts = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    for start in starts {
        let mut dir = start.as_path();
        loop {
            if dir.join("tools/gate.rs").is_file() {
                return dir.to_path_buf();
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = project_root();
    if args.len() == 1 {
        match run_gate(&root, Path::new(&args[0])) {
            Ok(run) => {
                println!("{}", run.line());
                std::process::exit(run.exit_code());
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    } else if args.len() == 2 && args[0] == "--units" {
        match run_selected(&root, Path::new(&args[1])) {
            Ok(report) => std::process::exit(finish_selected(&report)),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("usage: gate <unit-binary>\n       gate --units <unit-binary>");
        std::process::exit(2);
    }
}

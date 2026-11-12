// Native unit gate for the --all-units role of tools/checks.py.
// Run the binary from tools/build-tools.sh. One argument: the unit binary.
// The gate line is what core/self_build.rs::parse_gate_line reads.

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
            c if (c as u32) < 0x20 || (c as u32) > 0x7f => {
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
    let mut child = command.spawn().map_err(|error| format!("cannot start {}: {error}", binary.display()))?;
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
    if args.len() != 1 {
        eprintln!("usage: gate <unit-binary>");
        std::process::exit(2);
    }
    match run_gate(&project_root(), Path::new(&args[0])) {
        Ok(run) => {
            println!("{}", run.line());
            std::process::exit(run.exit_code());
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "rllm-gate-{}-{}",
                std::process::id(),
                TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_script(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn font_paths_sort_like_path_parts() {
        let mut paths = vec![
            PathBuf::from("/usr/share/fonts/a.ttf"),
            PathBuf::from("/usr/share/fonts/a/b.ttf"),
            PathBuf::from("/usr/share/fonts/b.ttf"),
        ];
        sort_fonts(&mut paths);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/usr/share/fonts/a/b.ttf"),
                PathBuf::from("/usr/share/fonts/a.ttf"),
                PathBuf::from("/usr/share/fonts/b.ttf"),
            ]
        );
    }

    #[test]
    fn configured_font_file_wins() {
        let dir = TempDir::new();
        let font = dir.0.join("sample.ttf");
        fs::write(&font, b"font").unwrap();
        let found = vec![PathBuf::from("/usr/share/fonts/other.ttf")];
        assert_eq!(select_font(&font.display().to_string(), &found), font.display().to_string());
        assert_eq!(select_font("", &found), "/usr/share/fonts/other.ttf");
        assert_eq!(select_font("/no/such.ttf", &found), "/usr/share/fonts/other.ttf");
        assert_eq!(select_font("", &[]), "");
    }

    #[test]
    fn green_run_writes_report_and_gate_line() {
        let dir = TempDir::new();
        let binary = dir.0.join("units");
        write_script(
            &binary,
            "#!/bin/sh\n\
if [ \"$1\" = \"--list\" ]; then\n\
  printf '%s\\n' 'application::checks::one: test' 'module::two: test' 'not a case'\n\
  exit 0\n\
fi\n\
if [ ! -d toolchain/check-workspace ]; then\n\
  echo 'missing toolchain/check-workspace' >&2\n\
  exit 1\n\
fi\n\
printf '%s\\n' 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'\n\
exit 0\n",
        );
        let run = run_gate(&dir.0, &binary).unwrap();
        assert_eq!(run.line(), "Rust cases passed: 2/2; isolated application cases: 1");
        assert_eq!(run.exit_code(), 0);
        assert!(run.failures.is_empty());
        let report = fs::read_to_string(dir.0.join("output/checks/full-units.json")).unwrap();
        let label = if run.font.is_empty() { "unavailable".to_string() } else { run.font.clone() };
        let expected = format!(
            "{{\n  \"cases\": 2,\n  \"passed\": 2,\n  \"isolated_application_cases\": 1,\n  \"capture_font\": {font},\n  \"failures\": [],\n  \"binary_sha256\": {sha}\n}}\n",
            font = json_string(&label),
            sha = json_string(&run.binary_sha256),
        );
        assert_eq!(report, expected);
        assert_eq!(run.binary_sha256, hex_digest(&sha256(&fs::read(&binary).unwrap())));
        let scratches = fs::read_dir(dir.0.join("output/checks"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("rust-"))
            .count();
        assert_eq!(scratches, 0);
    }

    #[test]
    fn failed_batch_records_the_output_tail() {
        let dir = TempDir::new();
        let binary = dir.0.join("units");
        let pad = "Z".repeat(6000);
        write_script(
            &binary,
            &format!(
                "#!/bin/sh\n\
if [ \"$1\" = \"--list\" ]; then\n\
  printf '%s\\n' 'application::checks::bad: test' 'module::ok: test'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"--skip\" ]; then\n\
  printf '%s\\n' 'test result: ok. 1 passed; 0 failed; 0 ignored;'\n\
  exit 0\n\
fi\n\
printf '%s\\n' 'test result: FAILED. 0 passed; 1 failed; 0 ignored;'\n\
printf '%s' '{pad}'\n\
exit 1\n",
                pad = pad,
            ),
        );
        let run = run_gate(&dir.0, &binary).unwrap();
        assert_eq!(run.exit_code(), 1);
        assert_eq!(run.line(), "Rust cases passed: 1/2; isolated application cases: 1");
        assert_eq!(run.failures.len(), 1);
        assert_eq!(run.failures[0].name, "application::checks::bad");
        assert_eq!(run.failures[0].code, 1);
        assert_eq!(run.failures[0].details, "Z".repeat(5000));
        let report = fs::read_to_string(dir.0.join("output/checks/full-units.json")).unwrap();
        assert!(report.contains("\"passed\": 1"), "{report}");
        assert!(report.contains(&json_string(&"Z".repeat(5000))), "tail was not stored");
    }

    #[test]
    fn empty_and_duplicate_inventories_are_refused() {
        let dir = TempDir::new();
        let empty = dir.0.join("empty");
        write_script(&empty, "#!/bin/sh\nif [ \"$1\" = \"--list\" ]; then exit 0; fi\nexit 1\n");
        let error = run_gate(&dir.0, &empty).unwrap_err();
        assert!(error.contains("empty"), "{error}");
        assert!(!dir.0.join("output/checks/full-units.json").exists());
        let duplicate = dir.0.join("duplicate");
        write_script(
            &duplicate,
            "#!/bin/sh\nprintf '%s\\n' 'application::checks::one: test' 'application::checks::one: test'\n",
        );
        let error = run_gate(&dir.0, &duplicate).unwrap_err();
        assert!(error.contains("duplicate"), "{error}");
    }
}

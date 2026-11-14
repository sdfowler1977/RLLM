//! Fingerprint, bounded rebuild plan, content-addressed revision journal, re-exec, commit, and resume.
//! Reads and previews do not write or spawn. A journal append, a git commit, or a successor
//! exec runs only when the caller sets review_confirmed.
use super::*;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const NOFOLLOW: i32 = 131072;
const DIRECTORY: i32 = 65536;
const CLOEXEC: i32 = 524288;
const NONBLOCK: i32 = 2048;
const ELOOP: i32 = 40;
const ENOTDIR: i32 = 20;
const MAX_DEPTH: usize = 16;
const MAX_ENTRIES: usize = 8192;
const MAX_PER_DIR: usize = 1024;
const MAX_FILE: u64 = 4 * 1024 * 1024;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
const BINARY_MAX: u64 = 64 * 1024 * 1024;
const OUTPUT_MAX: usize = 64 * 1024;
const TAIL_MAX: usize = 400;
const JOURNAL_MAX: u64 = 1024 * 1024;
const JOURNAL_LINES: usize = 256;
const JOB_MAX: usize = 256;
const INSTRUCTION_MAX: usize = 16384;
const BUILDER_WALL: u64 = 900;
const GATE_WALL: u64 = 300;
const PROBE_WALL: u64 = 5;
const GATE_REL: &str = "output/tools/gate";
const SKIP_DIRS: &[&str] = &[".git", "output", "target", "tooling", ".qwen"];

fn sha256(bytes: &[u8]) -> String {
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
fn flag(value: &Json, key: &str) -> bool {
    get(value, key) == &Bool(true)
}
fn put(value: &mut Json, key: &str, item: Json) {
    if let Obj(fields) = value {
        fields.insert(key.into(), item);
    }
}
fn n(value: u64) -> Json {
    Num(value.to_string())
}
fn only(value: &Json, keys: &[&str]) -> Result<()> {
    let Obj(fields) = value else {
        return Err("self-build request must be an object".into());
    };
    if let Some(key) = fields.keys().find(|key| *key != "op" && !keys.contains(&key.as_str())) {
        return Err(format!("unknown self-build field {key}"));
    }
    Ok(())
}
fn relative(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.len() > 1024 || value.contains(['\0', '\\']) || value.starts_with('/') {
        return Err("path must stay inside the project".into());
    }
    let path = Path::new(value);
    if path.components().any(|component| !matches!(component, std::path::Component::Normal(_))) {
        return Err("path must stay inside the project".into());
    }
    Ok(path.to_path_buf())
}
fn root_of(value: &Json) -> Result<PathBuf> {
    let text = field(value, "root")?;
    if text.is_empty() || text.contains('\0') {
        return Err("project root is required".into());
    }
    let path = PathBuf::from(text);
    let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("project root must be a real directory".into());
    }
    Ok(path)
}
fn resolve_file(root: &Path, value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err("binary path is required".into());
    }
    if Path::new(value).is_absolute() {
        Ok(PathBuf::from(value))
    } else {
        Ok(root.join(relative(value)?))
    }
}
fn digest_text(value: &str) -> Result<()> {
    if value.is_empty() || (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())) {
        return Ok(());
    }
    Err("revision digest must be empty or 64 hexadecimal characters".into())
}
fn strings(value: &Json, key: &str) -> Result<Vec<String>> {
    match get(value, key) {
        Null => Ok(Vec::new()),
        Arr(items) if items.len() <= 32 => items
            .iter()
            .map(|item| {
                let text = item.text()?;
                if text.is_empty() || text.len() > 1024 || text.contains('\0') {
                    return Err(format!("invalid {key}"));
                }
                Ok(text.to_string())
            })
            .collect(),
        Arr(_) => Err(format!("{key} exceeds 32 arguments")),
        _ => Err(format!("{key} must be an array")),
    }
}
fn errno() -> i32 {
    unsafe extern "C" {
        #[cfg(target_os = "linux")]
        fn __errno_location() -> *mut i32;
        #[cfg(target_os = "android")]
        fn __errno() -> *mut i32;
    }
    #[cfg(target_os = "linux")]
    unsafe {
        *__errno_location()
    }
    #[cfg(target_os = "android")]
    unsafe {
        *__errno()
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        0
    }
}
fn open_at(parent: &File, name: &str, flags: i32) -> std::result::Result<File, i32> {
    let c_name = std::ffi::CString::new(name).map_err(|_| 22)?;
    // SAFETY: parent is an open directory fd. O_NOFOLLOW is always included by callers.
    // A negative return owns no descriptor; a non-negative return is owned by File.
    let fd = unsafe { openat(parent.as_raw_fd(), c_name.as_ptr(), flags) };
    if fd < 0 {
        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(5))
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
unsafe extern "C" {
    fn openat(fd: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    fn fdopendir(fd: i32) -> *mut std::ffi::c_void;
    fn readdir(dir: *mut std::ffi::c_void) -> *mut u8;
    fn closedir(dir: *mut std::ffi::c_void) -> i32;
    fn close(fd: i32) -> i32;
}
struct Listed(*mut std::ffi::c_void);
impl Drop for Listed {
    fn drop(&mut self) {
        unsafe { closedir(self.0) };
    }
}
fn list_dir(dir: &File) -> Result<Vec<String>> {
    let dot = std::ffi::CString::new(".").map_err(|_| "invalid directory name")?;
    // SAFETY: "." is relative to the already-opened directory. fdopendir owns this fd.
    let raw = unsafe { openat(dir.as_raw_fd(), dot.as_ptr(), DIRECTORY | NOFOLLOW | CLOEXEC) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let handle = unsafe { fdopendir(raw) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error().to_string();
        unsafe { close(raw) };
        return Err(error);
    }
    let listed = Listed(handle);
    let mut names = Vec::new();
    loop {
        errno_clear();
        // SAFETY: listed is a live DIR from fdopendir. The name bytes live only for this record.
        let entry = unsafe { readdir(listed.0) };
        if entry.is_null() {
            if errno() != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            break;
        }
        // 64-bit Linux/Android dirent: ino64, off64, reclen16, type8, then d_name.
        let length = unsafe { std::ptr::read_unaligned(entry.add(16).cast::<u16>()) } as usize;
        let bound = length.checked_sub(19).ok_or("invalid directory record")?.min(256);
        let name_bytes = unsafe { std::slice::from_raw_parts(entry.add(19), bound) };
        let end = name_bytes.iter().position(|byte| *byte == 0).ok_or("unterminated directory name")?;
        let bytes = &name_bytes[..end];
        if bytes == b"." || bytes == b".." {
            continue;
        }
        if names.len() >= MAX_PER_DIR {
            return Err("directory entry bound exceeded".into());
        }
        let name = std::str::from_utf8(bytes).map_err(|_| "source filename is not UTF-8")?;
        if name.contains(['/', '\\', '\0']) {
            return Err("source filename is not a single path segment".into());
        }
        names.push(name.to_string());
    }
    names.sort();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("directory changed during enumeration".into());
    }
    Ok(names)
}
fn errno_clear() {
    unsafe extern "C" {
        #[cfg(target_os = "linux")]
        fn __errno_location() -> *mut i32;
        #[cfg(target_os = "android")]
        fn __errno() -> *mut i32;
    }
    #[cfg(target_os = "linux")]
    unsafe {
        *__errno_location() = 0;
    }
    #[cfg(target_os = "android")]
    unsafe {
        *__errno() = 0;
    }
}
enum Opened {
    Dir(File),
    File(File),
    Symlink,
    Other,
}
fn open_entry(parent: &File, name: &str) -> Result<Opened> {
    match open_at(parent, name, DIRECTORY | NOFOLLOW | CLOEXEC) {
        Ok(file) => Ok(Opened::Dir(file)),
        Err(ELOOP) => Ok(Opened::Symlink),
        Err(ENOTDIR) => match open_at(parent, name, NOFOLLOW | CLOEXEC | NONBLOCK) {
            Ok(file) => {
                let meta = file.metadata().map_err(|error| error.to_string())?;
                if meta.is_file() {
                    Ok(Opened::File(file))
                } else {
                    Ok(Opened::Other)
                }
            }
            Err(ELOOP) => Ok(Opened::Symlink),
            Err(code) => Err(format!("source entry {name} failed ({code})")),
        },
        Err(code) => Err(format!("source entry {name} failed ({code})")),
    }
}
fn read_fd(file: File, max: u64) -> Result<Vec<u8>> {
    let len = file.metadata().map_err(|error| error.to_string())?.len();
    if len > max {
        return Err(format!("file exceeds {max} bytes"));
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() as u64 != len {
        return Err("file changed while reading".into());
    }
    Ok(bytes)
}
struct Walk {
    files: Vec<(String, String)>,
    entries: usize,
    bytes: u64,
    symlinks: usize,
}
fn walk(dir: &File, rel: &str, depth: usize, skip: &[String], state: &mut Walk) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err("source walk depth bound exceeded".into());
    }
    for name in list_dir(dir)? {
        state.entries += 1;
        if state.entries > MAX_ENTRIES {
            return Err("source walk entry bound exceeded".into());
        }
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        match open_entry(dir, &name)? {
            Opened::Symlink => state.symlinks += 1,
            Opened::Other => {}
            Opened::Dir(child) => walk(&child, &child_rel, depth + 1, skip, state)?,
            Opened::File(file) => {
                if skip.iter().any(|item| item == &child_rel) {
                    continue;
                }
                let len = file.metadata().map_err(|error| error.to_string())?.len();
                state.bytes = state.bytes.checked_add(len).ok_or("source walk overflow")?;
                if state.bytes > MAX_TOTAL {
                    return Err("source walk byte bound exceeded".into());
                }
                let bytes = read_fd(file, MAX_FILE)?;
                state.files.push((child_rel, sha256(&bytes)));
            }
        }
    }
    Ok(())
}
fn tree(root: &Path, skip: &[String]) -> Result<(String, usize, usize)> {
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(DIRECTORY | NOFOLLOW | CLOEXEC)
        .open(root)
        .map_err(|error| error.to_string())?;
    let mut state = Walk { files: Vec::new(), entries: 0, bytes: 0, symlinks: 0 };
    walk(&dir, "", 0, skip, &mut state)?;
    state.files.sort();
    let mut manifest = String::new();
    for (path, digest) in &state.files {
        manifest.push_str(path);
        manifest.push('\n');
        manifest.push_str(digest);
        manifest.push('\n');
    }
    Ok((sha256(manifest.as_bytes()), state.files.len(), state.symlinks))
}
fn read_regular(path: &Path, max: u64) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!("{} must be a regular non-symlink file", path.display()));
    }
    if meta.len() > max {
        return Err(format!("{} exceeds {max} bytes", path.display()));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | CLOEXEC | NONBLOCK)
        .open(path)
        .map_err(|error| error.to_string())?;
    read_fd(file, max)
}
/// Successor rows sit beside the revision journal. They use the same line
/// digest and parent link, and they are not source: a witness must not move
/// the source digest or the revision tail the next build parent has to match.
fn successor_journal_rel(journal_rel: &str) -> String {
    match journal_rel.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.successors.{ext}"),
        None => format!("{journal_rel}.successors"),
    }
}
fn successor_journal_path(journal: &Path) -> PathBuf {
    let name = journal.file_name().and_then(|name| name.to_str()).unwrap_or("journal");
    journal.with_file_name(successor_journal_rel(name))
}
fn successor_mark_rel(journal_rel: &str) -> String {
    match journal_rel.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.successor-mark.{ext}"),
        None => format!("{journal_rel}.successor-mark"),
    }
}
fn successor_mark_path(journal: &Path) -> PathBuf {
    let name = journal.file_name().and_then(|name| name.to_str()).unwrap_or("journal");
    journal.with_file_name(successor_mark_rel(name))
}
fn skip_paths(value: &Json) -> Result<Vec<String>> {
    let mut skip = Vec::new();
    for key in ["journal", "rollback"] {
        let path = text(value, key);
        if path.is_empty() {
            continue;
        }
        relative(&path)?;
        skip.push(path.clone());
        if key == "journal" {
            let witness = successor_journal_rel(&path);
            relative(&witness)?;
            skip.push(witness);
            // The mark is the durable high-water for that witness. It is not source.
            let mark = successor_mark_rel(&path);
            relative(&mark)?;
            skip.push(mark);
        }
    }
    Ok(skip)
}
fn units_dir(value: &Json) -> Result<String> {
    let units = text(value, "units");
    let units = if units.is_empty() { "output/units".to_string() } else { units };
    relative(&units)?;
    Ok(units)
}

struct Gate {
    passed: u64,
    cases: u64,
    isolated: u64,
    green: bool,
}
fn parse_gate_line(line: &str) -> Option<Gate> {
    let rest = line.trim().strip_prefix("Rust cases passed: ")?;
    let (counts, tail) = rest.split_once("; isolated application cases: ")?;
    let (passed, cases) = counts.split_once('/')?;
    let passed = passed.parse::<u64>().ok()?;
    let cases = cases.parse::<u64>().ok()?;
    let isolated = tail.trim().parse::<u64>().ok()?;
    Some(Gate { passed, cases, isolated, green: cases > 0 && passed == cases })
}
fn parse_gate(output: &str) -> Result<Gate> {
    let mut found = None;
    for line in output.lines() {
        if let Some(gate) = parse_gate_line(line) {
            found = Some(gate);
        }
    }
    found.ok_or_else(|| "unit gate did not print case counts".into())
}
fn command_step(name: &str, role: &str, program: &str, args: &[&str], wall: u64) -> Json {
    let mut argv = vec![s(program)];
    argv.extend(args.iter().map(|arg| s(arg)));
    obj(vec![
        ("name", s(name)),
        ("role", s(role)),
        ("program", s(program)),
        ("args", Arr(args.iter().map(|arg| s(arg)).collect())),
        ("argv", Arr(argv)),
        ("wall_seconds", n(wall)),
    ])
}

pub(crate) fn fingerprint(value: &Json) -> Result<Json> {
    only(value, &["root", "binary", "source_digest", "journal", "rollback", "review_confirmed"])?;
    let root = root_of(value)?;
    let binary = resolve_file(&root, field(value, "binary")?)?;
    let binary_digest = sha256(&read_regular(&binary, BINARY_MAX)?);
    let (source_digest, files, symlinks) = tree(&root, &skip_paths(value)?)?;
    let claimed = text(value, "source_digest");
    digest_text(&claimed)?;
    let stale = !claimed.is_empty() && claimed != source_digest;
    let mut result = obj(vec![
        ("binary_digest", s(&binary_digest)),
        ("source_digest", s(&source_digest)),
        ("fingerprint", s(&sha256(format!("{binary_digest}\n{source_digest}").as_bytes()))),
        ("files", n(files as u64)),
        ("symlinks_skipped", n(symlinks as u64)),
        ("stale", Bool(stale)),
    ]);
    if !claimed.is_empty() {
        put(&mut result, "claimed_source_digest", s(&claimed));
    }
    Ok(result)
}

pub(crate) fn plan(value: &Json) -> Result<Json> {
    only(value, &["units", "review_confirmed"])?;
    let units = units_dir(value)?;
    Ok(obj(vec![
        ("schema", s("1")),
        ("ordered", Bool(true)),
        ("bounded", Bool(true)),
        (
            "steps",
            Arr(vec![
                command_step("headless", "builder", "bash", &["headless.sh"], BUILDER_WALL),
                command_step("units", "builder", "bash", &["tools/build-units.sh", &units], BUILDER_WALL),
                command_step(
                    "unit_gate",
                    "gate",
                    GATE_REL,
                    &[&units],
                    GATE_WALL,
                ),
            ]),
        ),
        (
            "green",
            obj(vec![
                ("source", s("unit-gate-stdout")),
                ("line", s("Rust cases passed: <passed>/<cases>; isolated application cases: <isolated>")),
                ("passed_equals_cases", Bool(true)),
                ("cases_positive", Bool(true)),
                ("exit_code_is_verdict", Bool(false)),
            ]),
        ),
    ]))
}

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}
struct ChildGroup(i32);
impl Drop for ChildGroup {
    fn drop(&mut self) {
        unsafe {
            kill(-self.0, 9);
        }
    }
}
fn set_nonblock(fd: i32) -> bool {
    let flags = unsafe { fcntl(fd, 3) };
    flags >= 0 && unsafe { fcntl(fd, 4, flags | NONBLOCK) } >= 0
}
/// The carried identity belongs to the successor product. Builder and gate children
/// keep the rest of the environment and lose only these two names.
fn scrub_carried(command: &mut Command) {
    command.env_remove("RLLM_SELF_JOB");
    command.env_remove("RLLM_SELF_INSTRUCTION");
}
struct Captured {
    body: Vec<u8>,
    tail: Vec<u8>,
    total: usize,
}
impl Captured {
    fn push(&mut self, chunk: &[u8]) {
        self.total = self.total.saturating_add(chunk.len());
        let room = OUTPUT_MAX.saturating_sub(self.body.len());
        if room > 0 {
            let take = room.min(chunk.len());
            self.body.extend_from_slice(&chunk[..take]);
        }
        self.tail.extend_from_slice(chunk);
        if self.tail.len() > TAIL_MAX {
            let extra = self.tail.len() - TAIL_MAX;
            self.tail.drain(..extra);
        }
    }
    fn over(&self) -> bool {
        self.total >= OUTPUT_MAX
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    fn tail_text(&self) -> String {
        String::from_utf8_lossy(&self.tail).into_owned()
    }
}
fn drain_captured(input: &mut impl Read, stream: &mut Captured) -> Result<()> {
    let mut buffer = [0u8; 4096];
    for _ in 0..32 {
        match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => stream.push(&buffer[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
// Callers match the leading sentence. The exit and the tail make a compile failure readable.
fn bound_refusal(code: Option<i32>, out: &Captured, err: &Captured) -> String {
    let tail = if err.over() { err.tail_text() } else { out.tail_text() };
    format!("builder output exceeded its bound: exit {code:?}; tail {tail}")
}
/// One bounded child runner for every self-build leg. The sanctioned plan steps, a
/// probe and the reviewed builder/gate all run through this runner, which owns the
/// child process group and stops it on timeout or cancellation.
fn bounded_child(
    root: &Path,
    program: &str,
    args: &[String],
    wall: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<super::execution::BoundedOutput> {
    if wall.is_zero() || wall > Duration::from_secs(900) {
        return Err("Child process limit must be from 1 millisecond through 900 seconds".into());
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .process_group(0)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    scrub_carried(&mut command);
    if program == "git" {
        command.env_remove("GIT_DIR");
        command.env_remove("GIT_WORK_TREE");
        command.env_remove("GIT_INDEX_FILE");
        command.env_remove("GIT_NAMESPACE");
    }
    let mut child = command.spawn().map_err(|error| format!("Cannot start {program}: {error}"))?;
    let group = ChildGroup(child.id() as i32);
    let mut stdout = child.stdout.take().ok_or("Child output is unavailable")?;
    let mut stderr = child.stderr.take().ok_or("Child error output is unavailable")?;
    for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
        if !set_nonblock(fd) {
            let _ = child.kill();
            let _ = child.wait();
            drop(group);
            return Err("Unable to configure bounded output".into());
        }
    }
    let mut out = Captured { body: Vec::new(), tail: Vec::new(), total: 0 };
    let mut err = Captured { body: Vec::new(), tail: Vec::new(), total: 0 };
    let start = std::time::Instant::now();
    let status = loop {
        drain_captured(&mut stdout, &mut out)?;
        drain_captured(&mut stderr, &mut err)?;
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if cancelled() || start.elapsed() > wall {
            let _ = child.kill();
            let _ = child.wait();
            drop(group);
            return Err("Child process exceeded its time limit or was cancelled".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(group);
    drain_captured(&mut stdout, &mut out)?;
    drain_captured(&mut stderr, &mut err)?;
    let code = status.code();
    if out.over() || err.over() {
        return Err(bound_refusal(code, &out, &err));
    }
    Ok(super::execution::BoundedOutput { code, stdout: out.text(), stderr: err.text() })
}
/// The journal verb has no caller-held cancellation handle, so its own legs stay
/// wall-bound exactly as they were under the runner this seam replaced.
fn uncancelled() -> bool {
    false
}
fn run_probe(root: &Path, rel: &str, wall: Duration) -> Result<super::execution::BoundedOutput> {
    let path = root.join(relative(rel)?);
    if path.file_name().and_then(|name| name.to_str()) != Some("self-build-probe.sh") {
        return Err("probe must be self-build-probe.sh inside the project".into());
    }
    let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("probe must be a regular non-symlink file".into());
    }
    bounded_child(root, "bash", &[path.to_string_lossy().into_owned()], wall, &uncancelled)
}
fn run_sanctioned(root: &Path, units: &str) -> Result<super::execution::BoundedOutput> {
    let steps = [
        ("headless", "builder", "bash", vec!["headless.sh".to_string()], BUILDER_WALL),
        (
            "units",
            "builder",
            "bash",
            vec!["tools/build-units.sh".to_string(), units.to_string()],
            BUILDER_WALL,
        ),
        (
            "unit_gate",
            "gate",
            GATE_REL,
            vec![units.to_string()],
            GATE_WALL,
        ),
    ];
    let mut last = None;
    for (name, role, program, args, wall) in steps {
        let captured = if role == "gate" {
            let path = gate_tool_path(root)?;
            let path = path.to_string_lossy().into_owned();
            bounded_child(root, &path, &args, Duration::from_secs(wall), &uncancelled)?
        } else {
            bounded_child(root, program, &args, Duration::from_secs(wall), &uncancelled)?
        };
        if role == "builder" && captured.code != Some(0) {
            return Err(format!("builder failed: {name} exited {:?}", captured.code));
        }
        last = Some(captured);
    }
    last.ok_or_else(|| "sanctioned plan was empty".into())
}
fn probe_wall(value: &Json) -> Result<Duration> {
    match get(value, "wall_seconds") {
        Null => Ok(Duration::from_secs(PROBE_WALL)),
        Num(text) => text
            .parse::<u64>()
            .ok()
            .filter(|seconds| (1..=30).contains(seconds))
            .map(Duration::from_secs)
            .ok_or_else(|| "probe wall_seconds must be 1–30".into()),
        _ => Err("probe wall_seconds must be a number".into()),
    }
}
enum GateText {
    Ready(String, Option<i32>),
    BuilderFailed(String),
}
fn gate_text(value: &Json, root: &Path) -> Result<GateText> {
    if flag(value, "sanctioned") {
        if !flag(value, "review_confirmed") {
            return Err("sanctioned rebuild requires review_confirmed".into());
        }
        return match run_sanctioned(root, &units_dir(value)?) {
            Ok(captured) => Ok(GateText::Ready(captured.stdout, captured.code)),
            Err(error) if error.starts_with("builder failed:") => Ok(GateText::BuilderFailed(error)),
            Err(error) => Err(error),
        };
    }
    if !text(value, "probe").is_empty() {
        if !flag(value, "review_confirmed") {
            return Err("running a builder requires review_confirmed".into());
        }
        let captured = run_probe(root, &text(value, "probe"), probe_wall(value)?)?;
        return Ok(GateText::Ready(captured.stdout, captured.code));
    }
    if !text(value, "gate_output").is_empty() {
        return Ok(GateText::Ready(text(value, "gate_output"), None));
    }
    Err("unit gate output is required".into())
}

fn journal_lines(path: &Path) -> Result<Vec<String>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err("journal must be a regular non-symlink file".into());
        }
        Ok(meta) if meta.len() > JOURNAL_MAX => return Err("journal exceeds 1 MiB".into()),
        Ok(_) => {}
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | CLOEXEC | NONBLOCK)
        .open(path)
        .map_err(|error| error.to_string())?;
    let bytes = read_fd(file, JOURNAL_MAX)?;
    let text = String::from_utf8(bytes).map_err(|_| "journal is not UTF-8")?;
    let lines: Vec<String> = text.lines().filter(|line| !line.is_empty()).map(str::to_string).collect();
    if lines.len() > JOURNAL_LINES {
        return Err("journal exceeds 256 revisions".into());
    }
    Ok(lines)
}
fn without_revision(value: &Json) -> Result<Json> {
    let Obj(fields) = value else {
        return Err("revision record must be an object".into());
    };
    let mut body = fields.clone();
    body.remove("revision");
    Ok(Obj(body))
}
fn tail_revision(lines: &[String]) -> Result<Option<String>> {
    let Some(last) = lines.last() else {
        return Ok(None);
    };
    let value = parse(last)?;
    let revision = text(&value, "revision");
    digest_text(&revision)?;
    if revision.is_empty() || revision != sha256(without_revision(&value)?.string().as_bytes()) {
        return Err("journal tail is not a content-addressed record".into());
    }
    Ok(Some(revision))
}
fn locate(root: &Path, rel: &str, create_parents: bool) -> Result<PathBuf> {
    let rel_path = relative(rel)?;
    let mut cursor = root.to_path_buf();
    let parts: Vec<_> = rel_path.components().collect();
    for (index, component) in parts.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return Err("path must stay inside the project".into());
        };
        cursor.push(name);
        let last = index + 1 == parts.len();
        match fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.file_type().is_symlink() => return Err("path meets a symlink".into()),
            Ok(meta) if !last && !meta.is_dir() => return Err("path parent is not a directory".into()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && last => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create_parents => {
                fs::create_dir(&cursor).map_err(|err| err.to_string())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(cursor)
}
fn preserve_rollback(dest: &Path, previous: &Path) -> Result<String> {
    match fs::symlink_metadata(dest) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            Err("rollback artifact must stay a regular file".into())
        }
        Ok(_) => Ok(sha256(&read_regular(dest, BINARY_MAX)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let bytes = read_regular(previous, BINARY_MAX)?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|err| err.to_string())?;
                let parent_meta = fs::symlink_metadata(parent).map_err(|err| err.to_string())?;
                if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
                    return Err("rollback parent must be a real directory".into());
                }
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(dest)
                .map_err(|err| err.to_string())?;
            file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|err| err.to_string())?;
            let written = read_regular(dest, BINARY_MAX)?;
            if written != bytes {
                return Err("rollback copy did not read back".into());
            }
            Ok(sha256(&written))
        }
        Err(error) => Err(error.to_string()),
    }
}
fn rollback_name(dest: &Path, previous: &Path, create: bool) -> Result<String> {
    if fs::symlink_metadata(dest).is_ok() || !create {
        if fs::symlink_metadata(dest).is_ok() {
            return Ok(sha256(&read_regular(dest, BINARY_MAX)?));
        }
        return Ok(sha256(&read_regular(previous, BINARY_MAX)?));
    }
    preserve_rollback(dest, previous)
}
fn successor_of(root: &Path, value: &Json, build: &Path) -> Result<Json> {
    let path = if text(value, "successor_binary").is_empty() {
        build.to_path_buf()
    } else {
        resolve_file(root, &text(value, "successor_binary"))?
    };
    let digest = sha256(&read_regular(&path, BINARY_MAX)?);
    let mut argv = vec![path.to_string_lossy().into_owned()];
    argv.extend(strings(value, "successor_args")?);
    Ok(obj(vec![
        ("pid", n(u64::from(std::process::id()))),
        ("argv", Arr(argv.iter().map(|arg| s(arg)).collect())),
        ("binary_digest", s(&digest)),
    ]))
}
fn refuse(reason: &str, rollback_digest: &str) -> Result<Json> {
    Err(format!("{reason}; rollback {rollback_digest}"))
}
fn append_record(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(line.as_bytes()).and_then(|_| file.write_all(b"\n")).and_then(|_| file.sync_all()).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn journal(value: &Json) -> Result<Json> {
    only(
        value,
        &[
            "root",
            "binary",
            "previous_binary",
            "successor_binary",
            "successor_args",
            "job",
            "job_source",
            "instruction",
            "source_digest",
            "journal",
            "rollback",
            "parent",
            "probe",
            "gate_output",
            "review_confirmed",
            "sanctioned",
            "units",
            "wall_seconds",
        ],
    )?;
    let root = root_of(value)?;
    let journal_rel = text(value, "journal");
    let rollback_rel = text(value, "rollback");
    relative(&journal_rel)?;
    relative(&rollback_rel)?;
    if journal_rel == rollback_rel {
        return Err("journal and rollback must be different paths".into());
    }
    let parent = text(value, "parent");
    digest_text(&parent)?;
    let build = resolve_file(&root, field(value, "binary")?)?;
    let previous = if text(value, "previous_binary").is_empty() {
        build.clone()
    } else {
        resolve_file(&root, &text(value, "previous_binary"))?
    };
    let (source_digest, _, _) = tree(&root, &skip_paths(value)?)?;
    let claimed = text(value, "source_digest");
    digest_text(&claimed)?;
    let journal_path = locate(&root, &journal_rel, false)?;
    let rollback_path = locate(&root, &rollback_rel, false)?;
    if !claimed.is_empty() && claimed != source_digest {
        let rollback_digest = rollback_name(&rollback_path, &previous, false)?;
        return refuse("stale source digest", &rollback_digest);
    }
    let lines = journal_lines(&journal_path)?;
    match tail_revision(&lines)? {
        Some(tail) if parent != tail => return Err("parent revision does not match the journal tail".into()),
        None if parent.is_empty() && !lines.is_empty() => {
            return Err("parent revision does not match the journal tail".into());
        }
        _ => {}
    }
    let confirmed = flag(value, "review_confirmed");
    let run = gate_text(value, &root)?;
    let (stdout, builder_exit) = match run {
        GateText::BuilderFailed(reason) => {
            let rollback_digest = rollback_name(&rollback_path, &previous, confirmed)?;
            return refuse(&reason, &rollback_digest);
        }
        GateText::Ready(stdout, exit) => (stdout, exit),
    };
    let gate = match parse_gate(&stdout) {
        Ok(gate) => gate,
        Err(error) => {
            let rollback_digest = rollback_name(&rollback_path, &previous, confirmed)?;
            return refuse(&error, &rollback_digest);
        }
    };
    let successor = successor_of(&root, value, &build)?;
    let build_digest = sha256(&read_regular(&build, BINARY_MAX)?);
    if !gate.green {
        let rollback_digest = rollback_name(&rollback_path, &previous, confirmed)?;
        return refuse(
            &format!("reload refused: unit gate is not green ({}/{})", gate.passed, gate.cases),
            &rollback_digest,
        );
    }
    let rollback_digest = rollback_name(&rollback_path, &previous, confirmed)?;
    let body = obj(vec![
        ("schema", s("1")),
        ("source_digest", s(&source_digest)),
        ("build_digest", s(&build_digest)),
        ("parent", s(&parent)),
        (
            "gate",
            obj(vec![
                ("passed", n(gate.passed)),
                ("cases", n(gate.cases)),
                ("isolated", n(gate.isolated)),
                ("green", Bool(true)),
            ]),
        ),
        ("verdicts", Arr(vec![s("gate-green")])),
        ("successor", successor),
        ("rollback", s(&rollback_rel)),
        ("rollback_digest", s(&rollback_digest)),
    ]);
    let revision = sha256(body.string().as_bytes());
    let mut record = body.clone();
    put(&mut record, "revision", s(&revision));
    if !confirmed {
        let mut preview = record;
        put(&mut preview, "recorded", Bool(false));
        if let Some(exit) = builder_exit {
            put(&mut preview, "builder_exit", Num(exit.to_string()));
        }
        return Ok(preview);
    }
    let rollback_digest = preserve_rollback(&locate(&root, &rollback_rel, true)?, &previous)?;
    if text(&record, "rollback_digest") != rollback_digest {
        put(&mut record, "rollback_digest", s(&rollback_digest));
        let body = without_revision(&record)?;
        put(&mut record, "revision", s(&sha256(body.string().as_bytes())));
    }
    append_record(&locate(&root, &journal_rel, true)?, &record.string())?;
    put(&mut record, "recorded", Bool(true));
    if let Some(exit) = builder_exit {
        put(&mut record, "builder_exit", Num(exit.to_string()));
    }
    Ok(record)
}

fn run_log(pid: u64, argv: &[String], binary_digest: &str, source_digest: &str) -> Json {
    let argv_json = Arr(argv.iter().map(|arg| s(arg)).collect());
    obj(vec![
        ("pid", n(pid)),
        ("argv", argv_json.clone()),
        ("binary_digest", s(binary_digest)),
        ("source_digest", s(source_digest)),
        ("executed", Bool(false)),
        (
            "log",
            s(&format!("pid={pid} argv={} binary_digest={binary_digest}", argv_json.string())),
        ),
    ])
}
/// The successor file beside `journal`. An empty journal value, a missing
/// journal, and a journal with no recorded successor chain are the empty
/// witness. A missing companion is refused when that journal has a row and
/// this process has a tracked entry whose witness path is the companion.
/// A missing, shorter, replaced, or forked companion is refused when the
/// mark file beside the journal records a chain. Each append replaces that
/// file with one sealed row for the current chain. The mark survives exec
/// and restart. This read does not create the companion and does not append
/// a row.
fn successor_witness(root: &Path, value: &Json) -> Result<(u64, String)> {
    let journal_rel = text(value, "journal");
    if journal_rel.is_empty() {
        return Ok((0, String::new()));
    }
    let journal = locate(root, &journal_rel, false)?;
    let companion = successor_journal_path(&journal);
    if companion_missing(&companion)? {
        let journal_has_rows = !journal_lines(&journal)?.is_empty();
        if journal_has_rows && companion_tracked(&companion) {
            return Err("successor journal is missing beside a journal that has rows; the companion was not created".into());
        }
        let recorded = recorded_successor_revisions(&journal)?;
        if !recorded.is_empty() {
            return Err("successor journal is missing after a recorded chain".into());
        }
        return Ok((0, String::new()));
    }
    let rows = durable_successor_rows(&journal)?;
    let current: Vec<String> = rows.iter().map(|row| text(row, "revision")).collect();
    let recorded = recorded_successor_revisions(&journal)?;
    if let Some(reason) = successor_chain_break(&recorded, &current) {
        return Err(reason.into());
    }
    let tip = current.last().cloned().unwrap_or_default();
    Ok((current.len() as u64, tip))
}
fn companion_tracked(companion: &Path) -> bool {
    successors().values().any(|tracked| tracked.journal.as_deref() == Some(companion))
}

pub(crate) fn handoff(value: &Json) -> Result<Json> {
    only(
        value,
        &[
            "root",
            "binary",
            "args",
            "source_digest",
            "review_confirmed",
            "parent",
            "revision",
            "journal",
            "rollback",
        ],
    )?;
    let root = root_of(value)?;
    let binary = resolve_file(&root, field(value, "binary")?)?;
    let binary_bytes = read_regular(&binary, BINARY_MAX)?;
    let binary_digest = sha256(&binary_bytes);
    let (source_digest, _, _) = tree(&root, &skip_paths(value)?)?;
    let claimed = text(value, "source_digest");
    digest_text(&claimed)?;
    if !claimed.is_empty() && claimed != source_digest {
        return Err("stale source digest".into());
    }
    digest_text(&text(value, "parent"))?;
    digest_text(&text(value, "revision"))?;
    // The chain tip is the file beside `journal`, checked against the mark
    // written with each successor row. The live table does not supply the tip.
    // A missing companion is refused when that table has an entry for the companion,
    // and when the mark records a chain the file no longer holds.
    let (successor_rows, successor_tip) = successor_witness(&root, value)?;
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(strings(value, "args")?);
    let pid = u64::from(std::process::id());
    let mut log = run_log(pid, &argv, &binary_digest, &source_digest);
    put(&mut log, "successor_rows", n(successor_rows));
    put(&mut log, "successor_tip", s(&successor_tip));
    put(&mut log, "successors", Arr(successor_states()));
    if !flag(value, "review_confirmed") {
        return Ok(log);
    }
    let mut command = Command::new(&binary);
    command.args(&argv[1..]);
    let error = CommandExt::exec(&mut command);
    Err(format!("successor exec failed: {error}"))
}

fn hex_width(value: &str, width: usize) -> bool {
    value.len() == width && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn hex40(value: &str) -> bool {
    hex_width(value, 40)
}
fn hex64(value: &str) -> bool {
    hex_width(value, 64)
}
fn utf8_path(path: &Path) -> Result<String> {
    path.to_str()
        .filter(|text| !text.is_empty() && !text.contains('\0'))
        .map(str::to_string)
        .ok_or_else(|| "path is not UTF-8".into())
}
fn source_rel(path: &str, skip: &[String]) -> bool {
    if path.is_empty() || path.len() > 1024 || path.contains('\0') || path.starts_with('/') {
        return false;
    }
    if path.split('/').any(|part| part.is_empty() || part == "." || part == ".." || SKIP_DIRS.contains(&part)) {
        return false;
    }
    !skip.iter().any(|item| item == path)
}
fn require_git_dir(root: &Path) -> Result<PathBuf> {
    let git_dir = root.join(".git");
    let present = match fs::symlink_metadata(&git_dir) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => true,
        _ => false,
    };
    let head = git_dir.join("HEAD");
    let head_ok = match fs::symlink_metadata(&head) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => true,
        _ => false,
    };
    if present && head_ok {
        Ok(git_dir)
    } else {
        Err(format!("missing git repository {}", git_dir.display()))
    }
}
fn read_head_commit(git_dir: &Path) -> Result<String> {
    let head_text = fs::read_to_string(git_dir.join("HEAD")).map_err(|error| error.to_string())?;
    let head_text = head_text.trim();
    if let Some(name) = head_text.strip_prefix("ref: ") {
        if name.is_empty() || name.contains('\0') || name.starts_with('/') || name.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
            return Err("git HEAD ref is outside the repository".into());
        }
        let ref_path = git_dir.join(name);
        match fs::symlink_metadata(&ref_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(error) => return Err(error.to_string()),
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                return Err("git HEAD ref is not a regular file".into());
            }
            Ok(_) => {}
        }
        let text = fs::read_to_string(&ref_path).map_err(|error| error.to_string())?;
        let text = text.trim();
        if text.is_empty() {
            return Ok(String::new());
        }
        if !hex40(text) {
            return Err("git HEAD is not a commit".into());
        }
        return Ok(text.to_string());
    }
    if hex40(head_text) {
        return Ok(head_text.to_string());
    }
    Err("git HEAD is not a commit".into())
}
fn git_failure(args: &[String], output: &super::execution::BoundedOutput) -> String {
    let mut detail = output.stderr.trim().to_string();
    if detail.is_empty() {
        detail = output.stdout.trim().to_string();
    }
    if detail.chars().count() > 240 {
        detail = detail.chars().take(240).collect();
    }
    format!("git {} exited {:?}: {detail}", args.first().map(String::as_str).unwrap_or("git"), output.code)
}
fn git_command(root: &Path, git_dir: &Path, args: &[String]) -> Result<super::execution::BoundedOutput> {
    let hooks = git_dir.join("rllm-empty-hooks");
    fs::create_dir_all(&hooks).map_err(|error| error.to_string())?;
    let mut full = vec![
        "--git-dir".to_string(),
        utf8_path(git_dir)?,
        "--work-tree".to_string(),
        utf8_path(root)?,
        "-c".into(),
        "safe.directory=*".into(),
        "-c".into(),
        "user.name=RLLM".into(),
        "-c".into(),
        "user.email=rllm@localhost".into(),
        "-c".into(),
        "commit.gpgsign=false".into(),
        "-c".into(),
        "core.autocrlf=false".into(),
        "-c".into(),
        format!("core.hooksPath={}", utf8_path(&hooks)?),
    ];
    full.extend(args.iter().cloned());
    bounded_child(root, "git", &full, Duration::from_secs(15), &uncancelled)
}
fn git_ok(root: &Path, git_dir: &Path, args: &[String]) -> Result<super::execution::BoundedOutput> {
    let output = git_command(root, git_dir, args)?;
    if output.code != Some(0) {
        return Err(git_failure(args, &output));
    }
    Ok(output)
}
fn head_commit_id(root: &Path, git_dir: &Path) -> Result<String> {
    let output = git_ok(root, git_dir, &["rev-parse".to_string(), "HEAD".to_string()])?;
    let digest = output.stdout.trim().to_string();
    if !hex40(&digest) {
        return Err("commit digest is not hexadecimal".into());
    }
    Ok(digest)
}
fn commit_parent_id(root: &Path, git_dir: &Path, commit: &str) -> Result<String> {
    let output = git_command(
        root,
        git_dir,
        &["rev-parse".to_string(), "-q".to_string(), "--verify".to_string(), format!("{commit}^")],
    )?;
    if output.code != Some(0) {
        return Ok(String::new());
    }
    let parent = output.stdout.trim().to_string();
    if !hex40(&parent) {
        return Err("parent commit is not hexadecimal".into());
    }
    Ok(parent)
}
/// The room HEAD already records this revision and the tracked worktree matches it.
/// The subject alone is not enough: an unrelated same-subject commit is refused unless
/// its own tree carries exactly the source path set this room would commit.
fn matching_commit(
    root: &Path,
    git_dir: &Path,
    revision: &str,
    paths: &[String],
    skip: &[String],
) -> Result<Option<String>> {
    let listed = git_command(root, git_dir, &["log".to_string(), "-1".to_string(), "--format=%s".to_string()])?;
    if listed.code != Some(0) || listed.stdout.trim() != format!("self-commit {revision}") {
        return Ok(None);
    }
    let head = head_commit_id(root, git_dir)?;
    if commit_tree_paths(root, git_dir, &head, skip)? != paths {
        return Ok(None);
    }
    let diff = git_command(root, git_dir, &["diff".to_string(), "--quiet".to_string(), "HEAD".to_string()])?;
    match diff.code {
        Some(0) => Ok(Some(head)),
        Some(1) => Ok(None),
        _ => Err(git_failure(&["diff".to_string()], &diff)),
    }
}
fn tracked_source_paths(root: &Path, git_dir: &Path, skip: &[String]) -> Result<Vec<String>> {
    let output = git_ok(root, git_dir, &["ls-files".to_string(), "-z".to_string()])?;
    let mut paths = Vec::new();
    for path in output.stdout.split('\0') {
        if path.is_empty() || !source_rel(path, skip) {
            continue;
        }
        paths.push(path.to_string());
    }
    if paths.len() > 1024 {
        return Err("tracked source tree exceeds 1024 paths".into());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}
fn commit_tree_paths(root: &Path, git_dir: &Path, commit: &str, skip: &[String]) -> Result<Vec<String>> {
    let output = git_ok(
        root,
        git_dir,
        &[
            "ls-tree".to_string(),
            "-r".to_string(),
            "-z".to_string(),
            "--name-only".to_string(),
            commit.to_string(),
        ],
    )?;
    let mut paths = Vec::new();
    for path in output.stdout.split('\0') {
        if path.is_empty() || !source_rel(path, skip) {
            continue;
        }
        paths.push(path.to_string());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}
fn write_source_commit(
    root: &Path,
    git_dir: &Path,
    revision: &str,
    paths: &[String],
    skip: &[String],
) -> Result<String> {
    if let Some(existing) = matching_commit(root, git_dir, revision, paths, skip)? {
        return Ok(existing);
    }
    let mut add = vec!["add".to_string(), "--".to_string()];
    add.extend(paths.iter().cloned());
    git_ok(root, git_dir, &add)?;
    let committed = git_command(
        root,
        git_dir,
        &["commit".to_string(), "-m".to_string(), format!("self-commit {revision}")],
    )?;
    if committed.code == Some(0) {
        return head_commit_id(root, git_dir);
    }
    if let Some(existing) = matching_commit(root, git_dir, revision, paths, skip)? {
        return Ok(existing);
    }
    Err(git_failure(&["commit".to_string()], &committed))
}
fn journalled_revision(path: &Path, revision: &str) -> Result<Json> {
    for line in journal_lines(path)? {
        let Ok(value) = parse(&line) else {
            continue;
        };
        if text(&value, "intent") == "self-commit" || text(&value, "revision") != revision {
            continue;
        }
        let body = without_revision(&value)?;
        if sha256(body.string().as_bytes()) != revision {
            continue;
        }
        if !hex64(&text(&value, "source_digest")) || !hex64(&text(&value, "build_digest")) {
            continue;
        }
        return Ok(value);
    }
    Err("revision is not a journalled revision of this room".into())
}
fn append_commit_intent(path: &Path, revision: &str, source_digest: &str, parent: &str) -> Result<()> {
    let body = obj(vec![
        ("schema", s("1")),
        ("intent", s("self-commit")),
        ("target", s(revision)),
        ("source_digest", s(source_digest)),
        ("parent", s(parent)),
        ("review_confirmed", Bool(false)),
    ]);
    let digest = sha256(body.string().as_bytes());
    let mut record = body;
    put(&mut record, "revision", s(&digest));
    append_record(path, &record.string())
}
fn commit_receipt(
    commit: Option<&str>,
    revision: &str,
    parent_commit: &str,
    source_digest: &str,
    paths: &[String],
    confirmed: bool,
) -> Json {
    let mut fields = vec![
        ("revision", s(revision)),
        ("parent_commit", s(parent_commit)),
        ("source_digest", s(source_digest)),
        ("committed_paths", Arr(paths.iter().map(|path| s(path)).collect())),
        ("review_confirmed", Bool(confirmed)),
    ];
    if let Some(commit) = commit {
        fields.push(("commit", s(commit)));
    }
    obj(fields)
}

/// Commit the room's tracked source tree for one journalled revision.
/// `review_confirmed` false journals the intent and leaves every git ref untouched.
/// A later permit for the same revision returns the commit already created for it.
pub(crate) fn commit(value: &Json) -> Result<Json> {
    only(value, &["root", "binary", "revision", "journal", "review_confirmed", "parent"])?;
    let root = root_of(value)?;
    let binary = resolve_file(&root, field(value, "binary")?)?;
    let journal_rel = text(value, "journal");
    relative(&journal_rel)?;
    if journal_rel.is_empty() {
        return Err("journal path is required".into());
    }
    let parent = text(value, "parent");
    digest_text(&parent)?;
    let revision = text(value, "revision");
    if !hex64(&revision) {
        return Err("revision is not a journalled revision of this room".into());
    }
    let journal_path = locate(&root, &journal_rel, false)?;
    let record = journalled_revision(&journal_path, &revision)?;
    if text(&record, "parent") != parent {
        return Err("parent revision does not match the journalled revision".into());
    }
    let binary_digest = sha256(&read_regular(&binary, BINARY_MAX)?);
    if text(&record, "build_digest") != binary_digest {
        return Err("binary digest does not match the journalled revision".into());
    }
    let rollback = text(&record, "rollback");
    let skip = skip_paths(&obj(vec![("journal", s(&journal_rel)), ("rollback", s(&rollback))]))?;
    let (source_digest, _, _) = tree(&root, &skip)?;
    if text(&record, "source_digest") != source_digest {
        return Err("stale source digest".into());
    }
    let git_dir = require_git_dir(&root)?;
    let parent_commit = read_head_commit(&git_dir)?;
    if !flag(value, "review_confirmed") {
        append_commit_intent(&journal_path, &revision, &source_digest, &parent)?;
        return Ok(commit_receipt(None, &revision, &parent_commit, &source_digest, &[], false));
    }
    let paths = tracked_source_paths(&root, &git_dir, &skip)?;
    if paths.is_empty() {
        return Err("tracked source tree is empty".into());
    }
    let digest = write_source_commit(&root, &git_dir, &revision, &paths, &skip)?;
    let parent_of = commit_parent_id(&root, &git_dir, &digest)?;
    let listed = commit_tree_paths(&root, &git_dir, &digest, &skip)?;
    Ok(commit_receipt(Some(&digest), &revision, &parent_of, &source_digest, &listed, true))
}

/// Resume one journalled revision this room has already committed, then re-exec its binary.
/// `review_confirmed` false returns the commit receipt and leaves the process in place.
/// The journal row, the commit parent, and the commit tree are the checks `commit` uses.
/// The successor chain is the witness `handoff` reads, so a tampered, truncated, replaced,
/// or rolled-back companion returns that witness's sentence and does not exec.
pub(crate) fn resume(value: &Json) -> Result<Json> {
    only(value, &["root", "binary", "args", "revision", "journal", "review_confirmed", "parent"])?;
    let root = root_of(value)?;
    let binary = resolve_file(&root, field(value, "binary")?)?;
    let journal_rel = text(value, "journal");
    relative(&journal_rel)?;
    if journal_rel.is_empty() {
        return Err("journal path is required".into());
    }
    let parent = text(value, "parent");
    digest_text(&parent)?;
    let revision = text(value, "revision");
    if !hex64(&revision) {
        return Err("revision is not a journalled revision of this room".into());
    }
    let journal_path = locate(&root, &journal_rel, false)?;
    let record = journalled_revision(&journal_path, &revision)?;
    if text(&record, "parent") != parent {
        return Err("parent revision does not match the journalled revision".into());
    }
    let binary_digest = sha256(&read_regular(&binary, BINARY_MAX)?);
    if text(&record, "build_digest") != binary_digest {
        return Err("binary digest does not match the journalled revision".into());
    }
    let rollback = text(&record, "rollback");
    let skip = skip_paths(&obj(vec![("journal", s(&journal_rel)), ("rollback", s(&rollback))]))?;
    let (source_digest, _, _) = tree(&root, &skip)?;
    if text(&record, "source_digest") != source_digest {
        return Err("stale source digest".into());
    }
    let git_dir = require_git_dir(&root)?;
    let paths = tracked_source_paths(&root, &git_dir, &skip)?;
    if paths.is_empty() {
        return Err("tracked source tree is empty".into());
    }
    let Some(digest) = matching_commit(&root, &git_dir, &revision, &paths, &skip)? else {
        return Err("revision is not the committed head of this room".into());
    };
    let parent_of = commit_parent_id(&root, &git_dir, &digest)?;
    let listed = commit_tree_paths(&root, &git_dir, &digest, &skip)?;
    if listed != paths {
        return Err("revision is not the committed head of this room".into());
    }
    successor_witness(&root, value)?;
    if !flag(value, "review_confirmed") {
        return Ok(commit_receipt(Some(&digest), &revision, &parent_of, &source_digest, &listed, false));
    }
    let mut command = Command::new(&binary);
    command.args(strings(value, "args")?);
    let error = CommandExt::exec(&mut command);
    Err(format!("successor exec failed: {error}"))
}

pub(crate) fn write_args(args: &Json) -> Json {
    obj(vec![
        ("path", get(args, "path").clone()),
        ("content", get(args, "content").clone()),
    ])
}
pub(crate) fn permission_subject(pending: &Json) -> Json {
    let mut subject = pending.clone();
    put(&mut subject, "tool", s("write"));
    put(&mut subject, "args", write_args(get(pending, "args")));
    subject
}
fn validate_with(args: &Json, carried_job: &str, carried_instruction: &str) -> Result<()> {
    let path = text(args, "path");
    relative(&path)?;
    if text(args, "content").len() > MAX_FILE as usize {
        return Err("self-build content exceeds 4 MiB".into());
    }
    if text(args, "job").len() > JOB_MAX {
        return Err(format!("job exceeds {JOB_MAX} bytes"));
    }
    if text(args, "instruction").len() > INSTRUCTION_MAX {
        return Err(format!("instruction exceeds {INSTRUCTION_MAX} bytes"));
    }
    bound_carried(&text(args, "job"), carried_job, JOB_MAX, "job")?;
    bound_carried(&text(args, "instruction"), carried_instruction, INSTRUCTION_MAX, "instruction")?;
    Ok(())
}
pub(crate) fn validate(args: &Json) -> Result<()> {
    let (carried_job, carried_instruction) = inherited();
    validate_with(args, &carried_job, &carried_instruction)
}
pub(crate) fn approve(pending: &Json) -> Result<()> {
    validate(get(pending, "args"))
}
pub(crate) fn prepare(root: &Path, pending: &mut Json) -> Result<()> {
    let mut args = get(pending, "args").clone();
    validate(&args)?;
    let file = root.join(relative(&text(&args, "path"))?);
    match fs::symlink_metadata(&file) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            Err("self-build path must be a regular file".into())
        }
        Ok(_) => {
            let old = String::from_utf8(read_regular(&file, MAX_FILE)?).map_err(|_| "self-build source must be UTF-8")?;
            put(&mut args, "approved_previous", s(&old));
            put(&mut args, "previous_exists", Bool(true));
            put(pending, "args", args);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            put(&mut args, "previous_exists", Bool(false));
            put(pending, "args", args);
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}
fn pending_stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
/// The edit leg of the seam. The reviewed content replaces the workspace source with
/// the same replacement policy as the write operation: parent directories are created,
/// the bytes go into a new temporary file that is synced, and the rename is what puts
/// them at the target. The target must already be a regular non-symlink file, or absent.
fn apply_reviewed_edit(root: &Path, args: &Json) -> Result<(String, usize)> {
    let path = text(args, "path");
    let content = field(args, "content")?;
    if content.len() > MAX_FILE as usize {
        return Err("self-build content exceeds 4 MiB".into());
    }
    if get(args, "previous_exists") == &Null {
        return Err("self-build operation has not been reviewed".into());
    }
    let target = locate(root, &path, true)?;
    let previous = match fs::symlink_metadata(&target) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err("self-build path must be a regular file".into());
        }
        Ok(_) => {
            let bytes = read_regular(&target, MAX_FILE)?;
            Some(String::from_utf8(bytes).map_err(|_| "self-build source must be UTF-8")?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let reviewed_exists = get(args, "previous_exists") == &Bool(true) || text(args, "previous_exists") == "true";
    let unchanged = match (&previous, field(args, "approved_previous")) {
        (Some(old), Ok(approved)) => old == approved,
        (Some(_), Err(error)) => return Err(error),
        (None, _) => true,
    };
    if reviewed_exists != previous.is_some() || !unchanged {
        return Err("File changed since review; review the operation again".into());
    }
    let parent = target.parent().ok_or("Invalid parent")?;
    let temp = parent.join(format!(".pending-{}-{}", std::process::id(), pending_stamp()));
    let written = content.len();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| error.to_string())?;
    let applied = file.write_all(content.as_bytes()).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = applied.and_then(|_| fs::rename(&temp, &target)) {
        let _ = fs::remove_file(&temp);
        return Err(format!("Cannot apply the reviewed edit: {error}"));
    }
    Ok((path, written))
}
fn reviewed_timeout(args: &Json) -> Result<Duration> {
    match get(args, "timeout_seconds") {
        Null => Ok(Duration::from_secs(30)),
        Num(text) => text
            .parse::<u64>()
            .ok()
            .filter(|seconds| (1..=600).contains(seconds))
            .map(Duration::from_secs)
            .ok_or_else(|| "timeout_seconds must be 1–600".into()),
        _ => Err("timeout_seconds must be a number".into()),
    }
}
fn gate_tool_path(root: &Path) -> Result<PathBuf> {
    let path = root.join(GATE_REL);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(path),
        _ => Err("unit gate tool is absent; build it with bash tools/build-tools.sh".into()),
    }
}
/// The parent is settled before the reviewed edit and before any builder. A
/// broken chain then leaves the review stamp valid and does not spend a rebuild.
fn require_journal_parent(root: &Path, args: &Json) -> Result<()> {
    let journal_rel = text(args, "journal");
    let rollback_rel = text(args, "rollback");
    relative(&journal_rel)?;
    relative(&rollback_rel)?;
    if journal_rel == rollback_rel {
        return Err("journal and rollback must be different paths".into());
    }
    let parent = text(args, "parent");
    digest_text(&parent)?;
    let lines = journal_lines(&locate(root, &journal_rel, false)?)?;
    match tail_revision(&lines)? {
        Some(tail) if parent != tail => Err("parent revision does not match the journal tail".into()),
        None if parent.is_empty() && !lines.is_empty() => Err("parent revision does not match the journal tail".into()),
        _ => Ok(()),
    }
}
fn append_journal_revision(
    root: &Path,
    args: &Json,
    source_digest: &str,
    rollback_digest: &str,
    gate_output: &str,
    spawned: Option<(u64, Vec<String>)>,
    job: &str,
    instruction: &str,
    job_source: &str,
) -> Result<String> {
    let gate = parse_gate(gate_output)?;
    if !gate.green {
        return Err(format!("reload refused: unit gate is not green ({}/{})", gate.passed, gate.cases));
    }
    let binary = resolve_file(root, &text(args, "binary"))?;
    let build_digest = sha256(&read_regular(&binary, BINARY_MAX)?);
    let mut argv = vec![s(&binary.to_string_lossy())];
    for arg in strings(args, "successor_args")? {
        argv.push(s(&arg));
    }
    let reload_mode = text(args, "reload");
    let mode = if reload_mode == "exec" || reload_mode == "spawn" {
        reload_mode
    } else {
        "none".to_string()
    };
    let (pid, handed) = match spawned {
        Some((pid, names)) => (pid, names),
        None => (u64::from(std::process::id()), Vec::new()),
    };
    let mut successor_fields = vec![
        ("pid", n(pid)),
        ("mode", s(&mode)),
        ("argv", Arr(argv)),
        ("binary_digest", s(&build_digest)),
        ("job", s(job)),
        ("job_source", s(job_source)),
        ("instruction", s(instruction)),
    ];
    if mode == "spawn" {
        successor_fields.push(("env", Arr(handed.iter().map(|name| s(name)).collect())));
    }
    let successor = obj(successor_fields);
    let body = obj(vec![
        ("schema", s("1")),
        ("source_digest", s(source_digest)),
        ("build_digest", s(&build_digest)),
        ("parent", s(&text(args, "parent"))),
        (
            "gate",
            obj(vec![
                ("passed", n(gate.passed)),
                ("cases", n(gate.cases)),
                ("isolated", n(gate.isolated)),
                ("green", Bool(true)),
            ]),
        ),
        ("verdicts", Arr(vec![s("gate-green")])),
        ("successor", successor),
        ("rollback", s(&text(args, "rollback"))),
        ("rollback_digest", s(rollback_digest)),
    ]);
    let revision = sha256(body.string().as_bytes());
    let mut record = body;
    put(&mut record, "revision", s(&revision));
    append_record(&locate(root, &text(args, "journal"), true)?, &record.string())?;
    Ok(revision)
}
enum SanctionedKind {
    Direct,
    GateTool,
}
/// The only sanctioned-argv match. Preflight and the spawn site both go through
/// `require_sanctioned`, so an arm cannot be accepted in one place and refused in the other.
fn classify_sanctioned(program: &str, args: &[String]) -> Option<SanctionedKind> {
    match (program, args) {
        ("bash", [only]) if only == "headless.sh" => Some(SanctionedKind::Direct),
        ("bash", [script, units]) if script == "tools/build-units.sh" && relative(units).is_ok() => {
            Some(SanctionedKind::Direct)
        }
        (GATE_REL, [units]) if relative(units).is_ok() => Some(SanctionedKind::GateTool),
        _ => None,
    }
}
fn require_sanctioned(role: &str, program: &str, args: &[String]) -> Result<SanctionedKind> {
    classify_sanctioned(program, args).ok_or_else(|| format!("{role} is not one of the sanctioned commands"))
}
/// Reload, timeout, builder argv, gate argv, and the binary path are settled
/// before any workspace write. A shape the seam will not run leaves the source,
/// the rollback copy, and the journal untouched.
fn require_command_shape(root: &Path, args: &Json) -> Result<Duration> {
    let reload = text(args, "reload");
    if !reload.is_empty() && reload != "exec" && reload != "spawn" {
        return Err("reload must be exec, spawn, or empty".into());
    }
    let timeout = reviewed_timeout(args)?;
    let builder = text(args, "builder");
    if !builder.is_empty() {
        require_sanctioned("builder", &builder, &strings(args, "builder_args")?)?;
    }
    let gate = text(args, "gate");
    if !gate.is_empty() {
        require_sanctioned("gate", &gate, &strings(args, "gate_args")?)?;
    }
    let binary = text(args, "binary");
    if !text(args, "journal").is_empty() || reload == "exec" || reload == "spawn" || !binary.is_empty() {
        resolve_file(root, &binary)?;
    }
    Ok(timeout)
}
fn sanctioned_child(
    root: &Path,
    role: &str,
    program: &str,
    args: &[String],
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<super::execution::BoundedOutput> {
    match require_sanctioned(role, program, args)? {
        SanctionedKind::Direct => bounded_child(root, program, args, timeout, cancelled),
        SanctionedKind::GateTool => {
            let path = gate_tool_path(root)?;
            let path = path.to_string_lossy().into_owned();
            bounded_child(root, &path, args, timeout, cancelled)
        }
    }
}
fn inherited() -> (String, String) {
    (
        std::env::var("RLLM_SELF_JOB").unwrap_or_default(),
        std::env::var("RLLM_SELF_INSTRUCTION").unwrap_or_default(),
    )
}
fn effective(supplied: &str, carried: &str) -> (String, &'static str) {
    if !supplied.is_empty() {
        (supplied.to_string(), "caller")
    } else if !carried.is_empty() {
        (carried.to_string(), "inherited")
    } else {
        (String::new(), "none")
    }
}
fn bound_carried(supplied: &str, carried: &str, limit: usize, noun: &str) -> Result<(String, &'static str)> {
    let (value, source) = effective(supplied, carried);
    if source == "inherited" && value.len() > limit {
        return Err(format!("inherited {noun} exceeds {limit} bytes"));
    }
    Ok((value, source))
}

struct Tracked {
    child: Option<std::process::Child>,
    job: String,
    job_source: String,
    mode: String,
    exit: Option<i32>,
    signal: Option<i32>,
    /// `try_wait` has already observed an exit. Later reads and `reap` use `exit`
    /// and `signal`. They do not wait again.
    exit_observed: bool,
    journal: Option<PathBuf>,
}
static SUCCESSORS: OnceLock<Mutex<BTreeMap<u64, Tracked>>> = OnceLock::new();
fn successors() -> std::sync::MutexGuard<'static, BTreeMap<u64, Tracked>> {
    SUCCESSORS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}
fn track(child: std::process::Child, job: &str, job_source: &str, mode: &str) -> u64 {
    track_with(child, job, job_source, mode, None)
}
fn track_with(
    child: std::process::Child,
    job: &str,
    job_source: &str,
    mode: &str,
    journal: Option<PathBuf>,
) -> u64 {
    let pid = u64::from(child.id());
    successors().insert(
        pid,
        Tracked {
            child: Some(child),
            job: job.to_string(),
            job_source: job_source.to_string(),
            mode: mode.to_string(),
            exit: None,
            signal: None,
            exit_observed: false,
            journal,
        },
    );
    pid
}
fn unix_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn witnessed_code(value: Option<i32>) -> Json {
    match value {
        Some(code) if code >= 0 => Num(code.to_string()),
        _ => Null,
    }
}
fn primary_journal_path(companion: &Path) -> PathBuf {
    let name = companion.file_name().and_then(|name| name.to_str()).unwrap_or("journal.successors");
    let primary = match name.rsplit_once(".successors") {
        Some((stem, ext)) => format!("{stem}{ext}"),
        None => name.to_string(),
    };
    companion.with_file_name(primary)
}
fn companion_missing(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.to_string()),
        Ok(_) => Ok(false),
    }
}
fn seal_with_parent(path: &Path, mut body: Json, parent: &str) -> Result<String> {
    put(&mut body, "parent", s(parent));
    let revision = sha256(body.string().as_bytes());
    put(&mut body, "revision", s(&revision));
    append_record(path, &body.string())?;
    Ok(revision)
}
/// The latest mark is the successor chain this process has already appended.
/// A later read refuses a companion that is missing, shorter, or not that chain.
/// The mark is an anchor, not a history. Each append seals a new row whose
/// `revisions` are the whole companion, then replaces the file with that row.
/// The row's parent is empty because it is the only link in the mark chain.
/// One row stays under the companion's digest and `journal_lines` bounds
/// (the companion itself stops at 256 revisions). The mark is not the companion,
/// so truncating or deleting the companion leaves the mark in place.
fn append_mark(path: &Path, revisions: &[String]) -> Result<()> {
    let mut body = obj(vec![
        ("schema", s("1")),
        ("event", s("mark")),
        ("revisions", Arr(revisions.iter().map(|revision| s(revision)).collect())),
    ]);
    put(&mut body, "parent", s(""));
    let revision = sha256(body.string().as_bytes());
    put(&mut body, "revision", s(&revision));
    let line = body.string();
    if line.len() as u64 + 1 > JOURNAL_MAX {
        return Err("journal exceeds 1 MiB".into());
    }
    rewrite_mark(path, &line)
}
/// Publish one mark line by renaming a synced temporary file over `path`.
/// The temporary file is opened with `O_NOFOLLOW` so a symlink is not followed.
/// An existing symlink mark is refused. The byte bound matches `journal_lines`.
/// The parent directory is synced after the rename so the new directory entry
/// survives a power loss. The file bytes were already synced. If the directory
/// sync fails, the renamed anchor stays and this function still returns
/// success. The companion row is already durable and the live mark matches it,
/// so a later handoff still reads the chain.
fn rewrite_mark(path: &Path, line: &str) -> Result<()> {
    let mut payload = Vec::with_capacity(line.len() + 1);
    payload.extend_from_slice(line.as_bytes());
    payload.push(b'\n');
    if payload.len() as u64 > JOURNAL_MAX {
        return Err("journal exceeds 1 MiB".into());
    }
    let parent = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let parent_meta = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err("journal must be a regular non-symlink file".into());
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err("journal must be a regular non-symlink file".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let file_name = path.file_name().and_then(|name| name.to_str()).ok_or("invalid mark name")?;
    let temp_name = format!(".{file_name}.{}.{}.tmp", std::process::id(), pending_stamp());
    if temp_name.len() > 255 || temp_name.contains(['/', '\\', '\0']) {
        return Err("invalid mark name".into());
    }
    let temp = parent.join(&temp_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(NOFOLLOW | CLOEXEC)
        .open(&temp)
        .map_err(|error| error.to_string())?;
    let applied = file.write_all(&payload).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = applied {
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }
    // The rename has published the anchor. A directory-sync error must not
    // unpublish it or fail the append. The companion row is already durable,
    // and this name already points at the synced file, so the two still match.
    // Returning the error would fail `append_linked` and abort the caller.
    // A caller that treated that as "not published" and removed the mark would
    // make every later handoff refuse a chain that is intact.
    let _ = sync_mark_directory(parent);
    Ok(())
}
fn sync_mark_directory(dir: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(DIRECTORY | NOFOLLOW | CLOEXEC)
        .open(dir)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}
fn mark_revisions(value: &Json) -> Result<Vec<String>> {
    let items = match get(value, "revisions") {
        Arr(items) => items,
        _ => return Err("successor mark row is not a recorded chain".into()),
    };
    if items.is_empty() {
        return Err("successor mark row is not a recorded chain".into());
    }
    let mut revisions = Vec::new();
    for item in items {
        let Str(revision) = item else {
            return Err("successor mark row is not a recorded chain".into());
        };
        if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("successor mark row is not a recorded chain".into());
        }
        revisions.push(revision.clone());
    }
    Ok(revisions)
}
fn recorded_successor_revisions(journal: &Path) -> Result<Vec<String>> {
    let lines = journal_lines(&successor_mark_path(journal))?;
    let mut previous = String::new();
    let mut recorded = Vec::new();
    for line in &lines {
        let value = parse(line)?;
        let revision = text(&value, "revision");
        digest_text(&revision)?;
        let body = without_revision(&value)?;
        if revision.is_empty() || revision != sha256(body.string().as_bytes()) {
            return Err("successor mark row is not content-addressed".into());
        }
        let parent = text(&value, "parent");
        digest_text(&parent)?;
        if parent != previous {
            return Err("successor mark parent does not match the previous row".into());
        }
        previous = revision;
        recorded = mark_revisions(&value)?;
    }
    Ok(recorded)
}
fn successor_chain_break(recorded: &[String], current: &[String]) -> Option<&'static str> {
    if recorded == current {
        return None;
    }
    let common = recorded.iter().zip(current.iter()).take_while(|(left, right)| left == right).count();
    if !recorded.is_empty() && common == current.len() && current.len() < recorded.len() {
        return Some("successor journal was truncated below the recorded chain");
    }
    if common > 0 && common < recorded.len() {
        return Some("successor journal rolled back onto a revision already observed");
    }
    Some("successor journal was replaced with a different chain")
}
fn append_linked(path: &Path, body: Json) -> Result<()> {
    let journal = primary_journal_path(path);
    let recorded = recorded_successor_revisions(&journal)?;
    if companion_missing(path)? && !recorded.is_empty() {
        return Err("successor journal is missing after a recorded chain".into());
    }
    let existing = durable_successor_rows(&journal)?;
    let mut revisions: Vec<String> = existing.iter().map(|row| text(row, "revision")).collect();
    if let Some(reason) = successor_chain_break(&recorded, &revisions) {
        return Err(reason.into());
    }
    let parent = revisions.last().cloned().unwrap_or_default();
    let revision = seal_with_parent(path, body, &parent)?;
    revisions.push(revision);
    append_mark(&successor_mark_path(&journal), &revisions)?;
    Ok(())
}
fn append_spawn_row(path: &Path, pid: u64, job: &str, source_digest: &str, mode: &str) -> Result<()> {
    append_linked(
        path,
        obj(vec![
            ("schema", s("1")),
            ("event", s("spawn")),
            ("pid", n(pid)),
            ("job", s(job)),
            ("source_digest", s(source_digest)),
            ("mode", s(mode)),
            ("timestamp", n(unix_secs())),
        ]),
    )
}
fn append_reap_row(path: &Path, pid: u64, job: &str, exit: Option<i32>, signal: Option<i32>) -> Result<()> {
    append_linked(
        path,
        obj(vec![
            ("schema", s("1")),
            ("event", s("reap")),
            ("pid", n(pid)),
            ("job", s(job)),
            ("exit", witnessed_code(exit)),
            ("signal", witnessed_code(signal)),
        ]),
    )
}
/// Rows in the successor journal next to `journal`. Missing file is an empty
/// witness. The in-process table is not consulted.
pub(crate) fn durable_successor_rows(journal: &Path) -> Result<Vec<Json>> {
    let lines = journal_lines(&successor_journal_path(journal))?;
    let mut rows = Vec::new();
    let mut previous = String::new();
    for line in &lines {
        let value = parse(line)?;
        let revision = text(&value, "revision");
        digest_text(&revision)?;
        let body = without_revision(&value)?;
        if revision.is_empty() || revision != sha256(body.string().as_bytes()) {
            return Err("successor journal row is not content-addressed".into());
        }
        let parent = text(&value, "parent");
        digest_text(&parent)?;
        if parent != previous {
            return Err("successor journal parent does not match the previous row".into());
        }
        previous = revision;
        rows.push(value);
    }
    Ok(rows)
}
fn remember_exit(tracked: &mut Tracked, status: std::process::ExitStatus) {
    tracked.exit = status.code();
    tracked.signal = std::os::unix::process::ExitStatusExt::signal(&status);
    tracked.exit_observed = true;
}
// A spawn that carried a journal path appends its exit beside that journal.
// A spawn with no journal stays in memory, and a refused command writes nothing.
// When the view has already stored an exit, that stored exit is the one recorded.
// `Child::try_wait` promises only the result of the call that observed the exit.
fn reap() -> Result<usize> {
    let mut table = successors();
    let mut ready = Vec::new();
    for (pid, tracked) in table.iter_mut() {
        if tracked.mode != "spawn" || tracked.child.is_none() {
            continue;
        }
        if !tracked.exit_observed {
            let status = {
                let Some(child) = tracked.child.as_mut() else { continue };
                match child.try_wait() {
                    Ok(status) => status,
                    Err(_) => continue,
                }
            };
            let Some(status) = status else { continue };
            remember_exit(tracked, status);
        }
        ready.push((*pid, tracked.job.clone(), tracked.journal.clone(), tracked.exit, tracked.signal));
    }
    let mut exited = 0usize;
    for (pid, job, journal, code, signal) in ready {
        if let Some(tracked) = table.get_mut(&pid) {
            if tracked.child.is_none() {
                continue;
            }
            tracked.child = None;
            tracked.exit = code;
            exited += 1;
        } else {
            continue;
        }
        if let Some(path) = journal {
            // A failed append costs a missing witness row, not a lost transition.
            append_reap_row(&path, pid, &job, code, signal)?;
        }
    }
    Ok(exited)
}
/// Stores an exit the first time `try_wait` observes one. The wait does not
/// append a journal row and leaves the child in the table. A later read uses
/// the stored exit and does not wait again.
fn observe_exit(tracked: &mut Tracked) -> bool {
    if tracked.exit_observed {
        return true;
    }
    if tracked.mode != "spawn" {
        return false;
    }
    let Some(child) = tracked.child.as_mut() else {
        return false;
    };
    match child.try_wait() {
        Ok(Some(status)) => {
            remember_exit(tracked, status);
            true
        }
        _ => false,
    }
}
pub(crate) fn successor_states() -> Vec<Json> {
    successors()
        .iter_mut()
        .map(|(pid, tracked)| {
            let exited = observe_exit(tracked);
            let running = tracked.child.is_some() && tracked.mode == "spawn" && !exited;
            let exit = if running {
                Null
            } else {
                match tracked.exit {
                    Some(code) => Num(code.to_string()),
                    None => Null,
                }
            };
            // `exit` stays the wait status' exit code. A signal has no exit code,
            // so the view names it here, and only once `exit_observed` is set.
            // Until then `signal` is null, which is how an unobserved death stays
            // distinct from a signal death. The durable reap row is not this object.
            let signal = if tracked.exit_observed {
                match tracked.signal {
                    Some(number) => Num(number.to_string()),
                    None => Null,
                }
            } else {
                Null
            };
            obj(vec![
                ("pid", n(*pid)),
                ("job", s(&tracked.job)),
                ("job_source", s(&tracked.job_source)),
                ("running", Bool(running)),
                ("exit", exit),
                ("signal", signal),
            ])
        })
        .collect()
}
fn with_reaped(receipt: String, reaped: usize) -> String {
    if reaped == 0 {
        receipt
    } else {
        format!("{receipt} reaped {reaped} successor(s)")
    }
}

pub(crate) fn run(root: &Path, value: &Json, cancelled: &dyn Fn() -> bool) -> Result<String> {
    let (carried_job, carried_instruction) = inherited();
    run_with(root, value, cancelled, &carried_job, &carried_instruction)
}
fn run_with(
    root: &Path,
    value: &Json,
    cancelled: &dyn Fn() -> bool,
    carried_job: &str,
    carried_instruction: &str,
) -> Result<String> {
    let reaped = reap()?;
    if cancelled() {
        return Err("self-build cancelled".into());
    }
    let args = get(value, "args");
    validate_with(args, carried_job, carried_instruction)?;
    let (job, job_source) = bound_carried(&text(args, "job"), carried_job, JOB_MAX, "job")?;
    let (instruction, _) = bound_carried(
        &text(args, "instruction"),
        carried_instruction,
        INSTRUCTION_MAX,
        "instruction",
    )?;
    // Command shape, including sanctioned argv, is refused before the reviewed edit.
    let timeout = require_command_shape(root, args)?;
    let reload = text(args, "reload");
    let journal_rel = text(args, "journal");
    if !journal_rel.is_empty() {
        require_journal_parent(root, args)?;
    }
    let (edited, written) = apply_reviewed_edit(root, args)?;
    let saved = format!("Saved {written} bytes to {edited}.");
    let mut source_digest = String::new();
    let mut rollback_digest = String::new();
    if !journal_rel.is_empty() {
        let (digest, _, _) = tree(root, &skip_paths(args)?)?;
        source_digest = digest;
        let binary = resolve_file(root, &text(args, "binary"))?;
        rollback_digest = preserve_rollback(&locate(root, &text(args, "rollback"), true)?, &binary)?;
    }
    let mut gate_text = String::new();
    if !text(args, "builder").is_empty() {
        let built = sanctioned_child(root, "builder", &text(args, "builder"), &strings(args, "builder_args")?, timeout, cancelled)?;
        if built.code != Some(0) {
            return Err(format!(
                "builder failed: exit {:?} {}",
                built.code,
                built.stderr.chars().take(400).collect::<String>()
            ));
        }
        gate_text = built.stdout;
    }
    if !text(args, "gate").is_empty() {
        let gate = sanctioned_child(root, "gate", &text(args, "gate"), &strings(args, "gate_args")?, timeout, cancelled)?;
        let parsed = parse_gate(&gate.stdout).map_err(|error| format!("{error}; stderr {}", gate.stderr.chars().take(400).collect::<String>()))?;
        if !parsed.green {
            return Err(format!("reload refused: unit gate is not green ({}/{})", parsed.passed, parsed.cases));
        }
        gate_text = gate.stdout;
    }
    if reload == "spawn" && !journal_rel.is_empty() {
        let gate = parse_gate(&gate_text)?;
        if !gate.green {
            return Err(format!("reload refused: unit gate is not green ({}/{})", gate.passed, gate.cases));
        }
    }
    let revision = if journal_rel.is_empty() || reload == "spawn" {
        String::new()
    } else {
        append_journal_revision(
            root,
            args,
            &source_digest,
            &rollback_digest,
            &gate_text,
            None,
            &job,
            &instruction,
            job_source,
        )?
    };
    if reload == "exec" || reload == "spawn" {
        if parse_gate(&gate_text).map(|gate| gate.green).unwrap_or(false) == false {
            return Err("reload refused unless the unit gate is green".into());
        }
        let binary = resolve_file(root, &text(args, "binary"))?;
        let mut carried_env = Vec::new();
        if !job.is_empty() {
            carried_env.push(("RLLM_SELF_JOB", job.as_str()));
        }
        if !instruction.is_empty() {
            carried_env.push(("RLLM_SELF_INSTRUCTION", instruction.as_str()));
        }
        if reload == "exec" {
            super::execution::exec_replace(&binary, &[], root, &carried_env)?;
        }
        let successor_args = strings(args, "successor_args")?;
        let mut child = Command::new(&binary);
        child.current_dir(root).stdin(Stdio::null()).args(&successor_args);
        scrub_carried(&mut child);
        let mut handed = Vec::new();
        if !job.is_empty() {
            child.env("RLLM_SELF_JOB", &job);
            handed.push("RLLM_SELF_JOB".to_string());
        }
        if !instruction.is_empty() {
            child.env("RLLM_SELF_INSTRUCTION", &instruction);
            handed.push("RLLM_SELF_INSTRUCTION".to_string());
        }
        let witnessed = if journal_rel.is_empty() {
            None
        } else {
            Some(successor_journal_path(&locate(root, &journal_rel, true)?))
        };
        let child = child.spawn().map_err(|error| format!("successor spawn failed: {error}"))?;
        let pid = match &witnessed {
            None => track(child, &job, job_source, "spawn"),
            Some(path) => track_with(child, &job, job_source, "spawn", Some(path.clone())),
        };
        // Spawn succeeded, so the pid is real. The row lands before the revision handoff.
        if let Some(path) = &witnessed {
            append_spawn_row(path, pid, &job, &source_digest, &reload)?;
        }
        let revision = if journal_rel.is_empty() {
            String::new()
        } else {
            append_journal_revision(
                root,
                args,
                &source_digest,
                &rollback_digest,
                &gate_text,
                Some((pid, handed)),
                &job,
                &instruction,
                job_source,
            )?
        };
        if revision.is_empty() {
            return Ok(with_reaped(format!("{saved} successor pid {pid}"), reaped));
        }
        return Ok(with_reaped(
            format!("{saved} recorded revision {revision} in {journal_rel} successor pid {pid}"),
            reaped,
        ));
    }
    if !revision.is_empty() {
        let mut receipt = format!("{saved} recorded revision {revision} in {journal_rel}");
        let gate = gate_text.trim_end_matches(['\r', '\n']);
        if !gate.is_empty() {
            receipt.push(' ');
            receipt.push_str(gate);
        }
        Ok(with_reaped(receipt, reaped))
    } else if gate_text.is_empty() {
        Ok(with_reaped(
            format!("{saved} self-build reviewed; no builder was requested"),
            reaped,
        ))
    } else {
        Ok(with_reaped(format!("{saved} {gate_text}"), reaped))
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rllm-self-build-{name}-{}-{}",
                std::process::id(),
                TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }
    fn project(name: &str, source: &str, probe: &str) -> Scratch {
        let scratch = Scratch::new(name);
        write(&scratch.path().join("src/lib.rs"), source.as_bytes());
        write(&scratch.path().join("self-build-probe.sh"), probe.as_bytes());
        scratch
    }
    fn rows(path: &Path) -> Vec<Json> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| parse(line).unwrap())
            .collect()
    }
    fn addressed(line: &Json) {
        let body = without_revision(line).unwrap();
        assert_eq!(text(line, "revision"), sha256(body.string().as_bytes()));
    }
    fn isolate() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        // Parallel tests must not reap a successor that belongs to another test.
        successors().clear();
        guard
    }

    #[test]
    fn green_gate_journals_and_links_parent() {
        let _isolate = isolate();
        let scratch = project(
            "green",
            "pub fn built() {}\n",
            "#!/bin/sh\nprintf '%s\\n' 'Rust cases passed: 2/2; isolated application cases: 1'\nexit 3\n",
        );
        let root = scratch.path();
        write(&root.join("old-bin"), b"previous-binary");
        write(&root.join("new-bin"), b"built-binary");
        let root_text = root.to_str().unwrap();
        let seen = fingerprint(&obj(vec![
            ("root", s(root_text)),
            ("binary", s(root.join("new-bin").to_str().unwrap())),
            ("journal", s("revisions/self-build.jsonl")),
            ("rollback", s("revisions/previous-binary")),
        ]))
        .unwrap();
        assert_eq!(get(&seen, "stale"), &Bool(false));
        let source_digest = text(&seen, "source_digest");
        let genesis = sha256(b"genesis-parent");
        let base = vec![
            ("root", s(root_text)),
            ("binary", s(root.join("new-bin").to_str().unwrap())),
            ("previous_binary", s(root.join("old-bin").to_str().unwrap())),
            ("successor_binary", s(root.join("new-bin").to_str().unwrap())),
            ("successor_args", Arr(vec![s("continue")])),
            ("source_digest", s(&source_digest)),
            ("journal", s("revisions/self-build.jsonl")),
            ("rollback", s("revisions/previous-binary")),
            ("parent", s(&genesis)),
        ];
        let mut preview_fields = base.clone();
        preview_fields.push(("gate_output", s("Rust cases passed: 2/2; isolated application cases: 1")));
        let preview = journal(&obj(preview_fields)).unwrap();
        assert_eq!(get(&preview, "recorded"), &Bool(false));
        assert!(!root.join("revisions/self-build.jsonl").exists());
        assert!(!root.join("revisions/previous-binary").exists());
        let mut confirmed = base.clone();
        confirmed.push(("review_confirmed", Bool(true)));
        confirmed.push(("probe", s("self-build-probe.sh")));
        let first = journal(&obj(confirmed.clone())).unwrap();
        assert_eq!(get(&first, "recorded"), &Bool(true));
        assert_eq!(get(&first, "builder_exit"), &Num("3".into()));
        assert_eq!(text(&first, "parent"), genesis);
        let verdicts = get(&first, "verdicts").array().unwrap();
        assert_eq!(verdicts[0], s("gate-green"));
        assert_eq!(fs::read(root.join("revisions/previous-binary")).unwrap(), b"previous-binary");
        let mut second_fields = base;
        second_fields.push(("review_confirmed", Bool(true)));
        second_fields.push(("probe", s("self-build-probe.sh")));
        second_fields.retain(|(key, _)| *key != "parent");
        second_fields.push(("parent", s(&text(&first, "revision"))));
        let second = journal(&obj(second_fields)).unwrap();
        assert_eq!(text(&second, "parent"), text(&first, "revision"));
        let stored = rows(&root.join("revisions/self-build.jsonl"));
        assert_eq!(stored.len(), 2);
        addressed(&stored[0]);
        addressed(&stored[1]);
        assert_eq!(text(&stored[0], "parent"), genesis);
        assert_eq!(text(&stored[1], "parent"), text(&stored[0], "revision"));
        assert_eq!(fs::read(root.join("revisions/previous-binary")).unwrap(), b"previous-binary");
        assert_eq!(fs::read(root.join("old-bin")).unwrap(), b"previous-binary");
    }

    #[test]
    fn red_gate_refuses_and_names_rollback_digest() {
        let _isolate = isolate();
        let scratch = project(
            "red",
            "pub fn built() {}\n",
            "#!/bin/sh\nprintf '%s\\n' 'Rust cases passed: 1/2; isolated application cases: 0'\nexit 0\n",
        );
        let root = scratch.path();
        write(&root.join("old-bin"), b"running");
        write(&root.join("revisions/previous-binary"), b"keep-rollback");
        let rollback_digest = sha256(b"keep-rollback");
        let seen = fingerprint(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("old-bin").to_str().unwrap())),
            ("journal", s("revisions/self-build.jsonl")),
            ("rollback", s("revisions/previous-binary")),
        ]))
        .unwrap();
        let error = journal(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("old-bin").to_str().unwrap())),
            ("previous_binary", s(root.join("old-bin").to_str().unwrap())),
            ("source_digest", s(&text(&seen, "source_digest"))),
            ("journal", s("revisions/self-build.jsonl")),
            ("rollback", s("revisions/previous-binary")),
            ("parent", s(&sha256(b"genesis-parent"))),
            ("probe", s("self-build-probe.sh")),
            ("review_confirmed", Bool(true)),
        ]))
        .unwrap_err();
        assert!(error.contains(&rollback_digest), "{error}");
        assert!(error.contains("rollback"), "{error}");
        assert!(!root.join("revisions/self-build.jsonl").exists());
        assert_eq!(fs::read(root.join("revisions/previous-binary")).unwrap(), b"keep-rollback");
    }

    #[test]
    fn stale_source_digest_is_detected() {
        let _isolate = isolate();
        let scratch = Scratch::new("stale");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"bin-v1");
        let first = fingerprint(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
        ]))
        .unwrap();
        assert_eq!(get(&first, "stale"), &Bool(false));
        let digest = text(&first, "source_digest");
        write(&root.join("src/lib.rs"), b"fn a() { let _ = 1; }\n");
        let second = fingerprint(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("source_digest", s(&digest)),
        ]))
        .unwrap();
        assert_eq!(get(&second, "stale"), &Bool(true));
        assert_eq!(text(&second, "claimed_source_digest"), digest);
        assert_ne!(text(&second, "source_digest"), digest);
        let current = text(&second, "source_digest");
        let third = fingerprint(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("source_digest", s(&current)),
        ]))
        .unwrap();
        assert_eq!(get(&third, "stale"), &Bool(false));
        let error = journal(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("source_digest", s(&digest)),
            ("journal", s("revisions/self-build.jsonl")),
            ("rollback", s("revisions/previous-binary")),
            ("parent", s(&sha256(b"genesis-parent"))),
            ("gate_output", s("Rust cases passed: 1/1; isolated application cases: 0")),
            ("review_confirmed", Bool(true)),
        ]))
        .unwrap_err();
        assert!(error.contains("stale source digest"), "{error}");
        assert!(!root.join("revisions/self-build.jsonl").exists());
    }

    #[test]
    fn successor_record_carries_pid_argv_digest() {
        let _isolate = isolate();
        let scratch = Scratch::new("successor");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn ready() {}\n");
        write(&root.join("next-bin"), b"successor-bytes");
        write(
            &root.join("self-build-probe.sh"),
            b"#!/bin/sh\nprintf '%s\\n' 'Rust cases passed: 1/1; isolated application cases: 0'\n",
        );
        let binary = root.join("next-bin");
        let digest = sha256(b"successor-bytes");
        let pid = Num(std::process::id().to_string());
        let result = handoff(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(binary.to_str().unwrap())),
            ("args", Arr(vec![s("continue")])),
            ("review_confirmed", Bool(false)),
        ]))
        .unwrap();
        assert_eq!(get(&result, "pid"), &pid);
        assert_eq!(get(&result, "executed"), &Bool(false));
        assert_eq!(text(&result, "binary_digest"), digest);
        let argv = get(&result, "argv").array().unwrap();
        assert_eq!(argv[0], s(binary.to_str().unwrap()));
        assert_eq!(argv[1], s("continue"));
        assert!(text(&result, "log").contains(&digest));
        assert!(text(&result, "log").contains(&std::process::id().to_string()));
        let recorded = journal(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(binary.to_str().unwrap())),
            ("successor_binary", s(binary.to_str().unwrap())),
            ("successor_args", Arr(vec![s("continue")])),
            ("journal", s("revisions/self-build.jsonl")),
            ("rollback", s("revisions/previous-binary")),
            ("parent", s(&sha256(b"genesis-parent"))),
            ("probe", s("self-build-probe.sh")),
            ("review_confirmed", Bool(true)),
        ]))
        .unwrap();
        let successor = get(&recorded, "successor");
        assert_eq!(get(successor, "pid"), &pid);
        assert_eq!(text(successor, "binary_digest"), digest);
        let stored_argv = get(successor, "argv").array().unwrap();
        assert_eq!(stored_argv[0], s(binary.to_str().unwrap()));
        assert_eq!(stored_argv[1], s("continue"));
        let line = &rows(&root.join("revisions/self-build.jsonl"))[0];
        let stored = get(line, "successor");
        assert_eq!(get(stored, "pid"), &pid);
        assert_eq!(text(stored, "binary_digest"), digest);
        assert_eq!(get(stored, "argv").array().unwrap()[0], s(binary.to_str().unwrap()));
        assert_eq!(get(stored, "argv").array().unwrap()[1], s("continue"));
    }

    #[test]
    fn plan_is_ordered_and_green_is_parsed_counts() {
        let _isolate = isolate();
        let recipe = plan(&obj(vec![("units", s("output/units"))])).unwrap();
        assert_eq!(get(&recipe, "ordered"), &Bool(true));
        assert_eq!(get(&recipe, "bounded"), &Bool(true));
        let steps = get(&recipe, "steps").array().unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(text(&steps[0], "program"), "bash");
        assert_eq!(get(&steps[0], "argv").array().unwrap(), &vec![s("bash"), s("headless.sh")]);
        assert_eq!(text(&steps[1], "program"), "bash");
        assert_eq!(
            get(&steps[1], "argv").array().unwrap(),
            &vec![s("bash"), s("tools/build-units.sh"), s("output/units")]
        );
        assert_eq!(text(&steps[2], "role"), "gate");
        assert!(matches!(get(&steps[0], "wall_seconds"), Num(value) if value.parse::<u64>().unwrap() >= 60));
        assert!(matches!(get(&steps[1], "wall_seconds"), Num(value) if value.parse::<u64>().unwrap() >= 60));
        assert_eq!(get(get(&recipe, "green"), "exit_code_is_verdict"), &Bool(false));
        assert!(parse_gate("Rust cases passed: 4/4; isolated application cases: 1\n").unwrap().green);
        assert!(!parse_gate("Rust cases passed: 3/4; isolated application cases: 1\n").unwrap().green);
        assert!(parse_gate("exit 0\n").is_err());
    }

    #[test]
    fn fingerprint_does_not_follow_symlinks_out() {
        let _isolate = isolate();
        let scratch = Scratch::new("symlink");
        let outside = Scratch::new("outside");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn inside() {}\n");
        write(&root.join("bin"), b"binary");
        write(&outside.path().join("secret"), b"secret-v1");
        write(&outside.path().join("hidden.rs"), b"hidden-v1");
        std::os::unix::fs::symlink(outside.path().join("secret"), root.join("leak")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("outdir")).unwrap();
        let request = |claimed: &str| {
            let mut fields = vec![
                ("root", s(root.to_str().unwrap())),
                ("binary", s(root.join("bin").to_str().unwrap())),
            ];
            if !claimed.is_empty() {
                fields.push(("source_digest", s(claimed)));
            }
            fingerprint(&obj(fields)).unwrap()
        };
        let first = request("");
        let Num(skipped) = get(&first, "symlinks_skipped") else { panic!("symlink count missing") };
        assert!(skipped.parse::<u64>().unwrap() >= 2);
        let digest = text(&first, "source_digest");
        write(&outside.path().join("secret"), b"secret-v2");
        write(&outside.path().join("hidden.rs"), b"hidden-v2");
        let second = request(&digest);
        assert_eq!(get(&second, "stale"), &Bool(false));
        assert_eq!(text(&second, "source_digest"), digest);
    }

    #[test]
    fn reviewed_edit_lands_on_disk_and_the_receipt_names_the_write() {
        let _isolate = isolate();
        let scratch = Scratch::new("edit");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        let content = "fn a() { let _ = 1; }\n";
        let request = |approved: &str| {
            obj(vec![(
                "args",
                obj(vec![
                    ("path", s("src/lib.rs")),
                    ("content", s(content)),
                    ("previous_exists", Bool(true)),
                    ("approved_previous", s(approved)),
                ]),
            )])
        };
        let live = || false;
        let receipt = run(root, &request("fn a() {}\n"), &live).unwrap();
        assert!(
            receipt.contains(&format!("Saved {} bytes to src/lib.rs.", content.len())),
            "{receipt}"
        );
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), content);
        let error = run(root, &request("fn a() {}\n"), &live).unwrap_err();
        assert!(error.contains("File changed since review"), "{error}");
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), content);
        let unreviewed = obj(vec![(
            "args",
            obj(vec![("path", s("src/lib.rs")), ("content", s(content))]),
        )]);
        let error = run(root, &unreviewed, &live).unwrap_err();
        assert!(error.contains("has not been reviewed"), "{error}");
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), content);
        let pending = fs::read_dir(root.join("src"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".pending-"))
            .count();
        assert_eq!(pending, 0);
    }

    #[test]
    fn reviewed_edit_refuses_a_symlink_a_directory_and_a_non_utf8_source() {
        let _isolate = isolate();
        let scratch = Scratch::new("edit-refusal");
        let outside = Scratch::new("edit-outside");
        let root = scratch.path();
        write(&outside.path().join("target.rs"), b"fn outside() {}\n");
        std::os::unix::fs::symlink(outside.path().join("target.rs"), root.join("link.rs")).unwrap();
        fs::create_dir_all(root.join("nest")).unwrap();
        write(&root.join("raw.rs"), &[0xff, 0xfe, 0x00, 0x80]);
        let live = || false;
        let request = |path: &str, approved: &str| {
            obj(vec![(
                "args",
                obj(vec![
                    ("path", s(path)),
                    ("content", s("fn replaced() {}\n")),
                    ("previous_exists", Bool(true)),
                    ("approved_previous", s(approved)),
                ]),
            )])
        };
        let error = run(root, &request("link.rs", "fn outside() {}\n"), &live).unwrap_err();
        assert!(error.contains("symlink"), "{error}");
        assert_eq!(fs::read_to_string(outside.path().join("target.rs")).unwrap(), "fn outside() {}\n");
        let error = run(root, &request("nest", ""), &live).unwrap_err();
        assert!(error.contains("regular file"), "{error}");
        let error = run(root, &request("raw.rs", ""), &live).unwrap_err();
        assert!(error.contains("UTF-8"), "{error}");
        assert_eq!(fs::read(root.join("raw.rs")).unwrap(), vec![0xff, 0xfe, 0x00, 0x80]);
    }

    #[test]
    fn handoff_skips_the_journal_and_the_over_cap_rollback_store() {
        let _isolate = isolate();
        let scratch = Scratch::new("handoff-skip");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn shipped() {}\n");
        write(&root.join("next-bin"), b"successor-image");
        write(&root.join("self/journal.jsonl"), b"{\"schema\":\"1\"}\n");
        // Present and empty: this handoff is a digest skip, not a vanished chain.
        write(&successor_journal_path(&root.join("self/journal.jsonl")), b"");
        let mut rollback = vec![7u8; 4 * 1024 * 1024 + 4096];
        for (index, byte) in rollback.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        write(&root.join("self/rollback"), &rollback);
        let base = vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("next-bin").to_str().unwrap())),
            ("args", Arr(vec![s("continue")])),
            ("review_confirmed", Bool(false)),
        ];
        let error = handoff(&obj(base.clone())).unwrap_err();
        assert!(error.contains("file exceeds"), "{error}");
        let seen = fingerprint(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("next-bin").to_str().unwrap())),
            ("journal", s("self/journal.jsonl")),
            ("rollback", s("self/rollback")),
        ]))
        .unwrap();
        let mut stored = base;
        stored.push(("journal", s("self/journal.jsonl")));
        stored.push(("rollback", s("self/rollback")));
        stored.push(("source_digest", s(&text(&seen, "source_digest"))));
        let logged = handoff(&obj(stored)).unwrap();
        assert_eq!(get(&logged, "executed"), &Bool(false));
        assert_eq!(text(&logged, "source_digest"), text(&seen, "source_digest"));
        assert_eq!(text(&logged, "binary_digest"), sha256(b"successor-image"));
        assert!(text(&logged, "log").contains("continue"));
    }

    fn marker_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rllm-self-build-{name}-{}", std::process::id()))
    }
    fn journal_request<'a>(
        content: &'a str,
        parent: &'a str,
        extra: Vec<(&'a str, Json)>,
    ) -> Json {
        let mut fields = vec![
            ("path", s("src/lib.rs")),
            ("content", s(content)),
            ("previous_exists", Bool(true)),
            ("approved_previous", s("fn a() {}\n")),
            ("builder", s("bash")),
            ("builder_args", Arr(vec![s("headless.sh")])),
            ("binary", s("bin")),
            ("journal", s("self/journal.jsonl")),
            ("rollback", s("self/rollback")),
            ("parent", s(parent)),
        ];
        fields.extend(extra);
        obj(vec![("args", obj(fields))])
    }

    #[test]
    fn wrong_parent_refuses_before_the_builder_runs() {
        let _isolate = isolate();
        let scratch = Scratch::new("wrong-parent");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"binary-v1");
        let marker = marker_path("wrong-parent");
        let _ = fs::remove_file(&marker);
        write(
            &root.join("headless.sh"),
            format!(
                "#!/bin/sh\ntouch {}\nprintf '%s\\n' 'Rust cases passed: 2/2; isolated application cases: 1'\n",
                marker.display()
            )
            .as_bytes(),
        );
        let body = obj(vec![("schema", s("1")), ("parent", s(""))]);
        let revision = sha256(body.string().as_bytes());
        let mut record = body;
        put(&mut record, "revision", s(&revision));
        write(&root.join("self/journal.jsonl"), format!("{}\n", record.string()).as_bytes());
        let content = "fn a() { let _ = 1; }\n";
        let live = || false;
        let error = run(root, &journal_request(content, "", Vec::new()), &live).unwrap_err();
        assert_eq!(error, "parent revision does not match the journal tail");
        assert!(!marker.exists(), "builder ran for an empty parent");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), b"fn a() {}\n");
        let error = run(root, &journal_request(content, &sha256(b"wrong-parent"), Vec::new()), &live).unwrap_err();
        assert_eq!(error, "parent revision does not match the journal tail");
        assert!(!marker.exists(), "builder ran for a wrong parent");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), b"fn a() {}\n");
        assert_eq!(rows(&root.join("self/journal.jsonl")).len(), 1);
        let _ = fs::remove_file(&marker);
    }

    #[test]
    fn journal_commit_skips_over_cap_rollback_and_names_the_revision() {
        let _isolate = isolate();
        let scratch = Scratch::new("journal-commit");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"binary-v1");
        let mut rollback = vec![7u8; 4 * 1024 * 1024 + 4096];
        for (index, byte) in rollback.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        write(&root.join("self/rollback"), &rollback);
        let blocked = tree(root, &Vec::<String>::new()).unwrap_err();
        assert!(blocked.contains("exceeds"), "{blocked}");
        let marker = marker_path("journal-commit");
        let _ = fs::remove_file(&marker);
        write(
            &root.join("headless.sh"),
            format!(
                "#!/bin/sh\ntouch {}\nprintf '%s\\n' 'Rust cases passed: 2/2; isolated application cases: 1'\n",
                marker.display()
            )
            .as_bytes(),
        );
        let before = tree(root, &["self/journal.jsonl".into(), "self/rollback".into()]).unwrap().0;
        let content = "fn a() { let _ = 1; }\n";
        let live = || false;
        let receipt = run(root, &journal_request(content, "", Vec::new()), &live).unwrap();
        assert!(marker.exists(), "builder did not run");
        let _ = fs::remove_file(&marker);
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), content);
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 1);
        addressed(&stored[0]);
        let revision = text(&stored[0], "revision");
        assert!(
            receipt.contains(&format!("Saved {} bytes to src/lib.rs.", content.len())),
            "{receipt}"
        );
        assert!(
            receipt.contains(&format!("recorded revision {revision} in self/journal.jsonl")),
            "{receipt}"
        );
        let after = tree(root, &["self/journal.jsonl".into(), "self/rollback".into()]).unwrap().0;
        assert_ne!(before, after);
        assert_eq!(text(&stored[0], "source_digest"), after);
        assert_eq!(text(&stored[0], "build_digest"), sha256(b"binary-v1"));
        assert_eq!(text(&stored[0], "parent"), "");
        assert_eq!(text(&stored[0], "rollback_digest"), sha256(&rollback));
        assert_eq!(get(get(&stored[0], "gate"), "passed"), &Num("2".into()));
        assert_eq!(get(get(&stored[0], "gate"), "cases"), &Num("2".into()));
        assert_eq!(get(get(&stored[0], "gate"), "isolated"), &Num("1".into()));
        assert_eq!(get(get(&stored[0], "gate"), "green"), &Bool(true));
        assert_eq!(get(&stored[0], "verdicts").array().unwrap()[0], s("gate-green"));
        assert_eq!(fs::read(root.join("self/rollback")).unwrap(), rollback);
    }

    #[test]
    fn missing_gate_tool_names_the_build_script() {
        let _isolate = isolate();
        let scratch = Scratch::new("missing-gate");
        let root = scratch.path();
        let error = match sanctioned_child(
            root,
            "gate",
            "output/tools/gate",
            &["output/units".to_string()],
            Duration::from_secs(5),
            &|| false,
        ) {
            Err(error) => error,
            Ok(_) => panic!("absent gate tool was started"),
        };
        assert!(error.contains("bash tools/build-tools.sh"), "{error}");
        assert!(!error.contains("No such file"), "{error}");
        assert!(!error.contains("Cannot start"), "{error}");
    }

    const SUCCESSOR_SCRIPT: &str = r#"#!/bin/sh
tmp="$1.part"
{
printf '%s\0' "$0"
printf '%s\0' "$2"
printf '%s\0' "$RLLM_SELF_JOB"
printf '%s\0' "$RLLM_SELF_INSTRUCTION"
if [ -n "${RLLM_SELF_JOB+x}" ]; then printf 'job-set\0'; else printf 'job-unset\0'; fi
if [ -n "${RLLM_SELF_INSTRUCTION+x}" ]; then printf 'instruction-set\0'; else printf 'instruction-unset\0'; fi
} > "$tmp"
mv "$tmp" "$1.done"
"#;

    fn executable(path: &Path, bytes: &[u8]) {
        write(path, bytes);
        let mut permissions = fs::metadata(path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
    fn green_builder(root: &Path) {
        write(
            &root.join("headless.sh"),
            b"#!/bin/sh\nprintf '%s\\n' 'Rust cases passed: 2/2; isolated application cases: 1'\n",
        );
    }
    fn wait_bytes(path: &Path) -> Vec<u8> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(bytes) = fs::read(path) {
                if !bytes.is_empty() {
                    return bytes;
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!("successor did not write {}", path.display());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn spawn_fields(content: &str, approved: &str, parent: &str, extra: Vec<(&str, Json)>) -> Json {
        let mut fields = vec![
            ("path", s("src/lib.rs")),
            ("content", s(content)),
            ("previous_exists", Bool(true)),
            ("approved_previous", s(approved)),
            ("builder", s("bash")),
            ("builder_args", Arr(vec![s("headless.sh")])),
            ("binary", s("bin")),
            ("journal", s("self/journal.jsonl")),
            ("rollback", s("self/rollback")),
            ("parent", s(parent)),
            ("reload", s("spawn")),
        ];
        fields.extend(extra);
        obj(vec![("args", obj(fields))])
    }

    #[test]
    fn spawn_journal_records_the_child_pid_argv_and_env_names() {
        let _isolate = isolate();
        let scratch = Scratch::new("spawn-pid");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        green_builder(root);
        let proof = root.join("proof");
        let proof_text = proof.to_str().unwrap();
        let binary = root.join("bin");
        let binary_text = binary.to_str().unwrap();
        let live = || false;
        let content = "fn a() { let _ = 1; }\n";
        let receipt = run(
            root,
            &spawn_fields(
                content,
                "fn a() {}\n",
                "",
                vec![
                    ("job", s("alpha-job")),
                    ("instruction", s("keep going")),
                    ("successor_args", Arr(vec![s(proof_text), s("continue")])),
                ],
            ),
            &live,
        )
        .unwrap();
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 1);
        addressed(&stored[0]);
        let successor = get(&stored[0], "successor");
        assert_eq!(text(successor, "mode"), "spawn");
        let Num(recorded) = get(successor, "pid") else { panic!("successor pid is not a number") };
        let recorded_pid = recorded.parse::<u64>().unwrap();
        assert!(recorded_pid > 0, "{recorded}");
        assert_ne!(recorded_pid, u64::from(std::process::id()));
        let suffix = format!(
            "recorded revision {} in self/journal.jsonl successor pid {recorded}",
            text(&stored[0], "revision")
        );
        assert!(receipt.ends_with(&suffix), "{receipt}");
        assert_eq!(
            get(successor, "argv").array().unwrap(),
            &vec![s(binary_text), s(proof_text), s("continue")]
        );
        assert_eq!(
            get(successor, "env").array().unwrap(),
            &vec![s("RLLM_SELF_JOB"), s("RLLM_SELF_INSTRUCTION")]
        );
        let dumped = wait_bytes(&PathBuf::from(format!("{proof_text}.done")));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[0], binary_text.as_bytes());
        assert_eq!(parts[1], b"continue");
        assert_eq!(parts[2], b"alpha-job");
        assert_eq!(parts[3], b"keep going");
        assert_eq!(parts[4], b"job-set");
        assert_eq!(parts[5], b"instruction-set");
    }

    #[test]
    fn spawn_journal_stores_job_and_instruction_verbatim_and_absent_job_is_empty() {
        let _isolate = isolate();
        let scratch = Scratch::new("spawn-job");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        green_builder(root);
        let job = "job-9";
        let instruction = "keep \"both\"\nlines";
        let first_content = "fn a() { let _ = 1; }\n";
        let first_proof = root.join("proof-set");
        let receipt = run_with(
            root,
            &spawn_fields(
                first_content,
                "fn a() {}\n",
                "",
                vec![
                    ("job", s(job)),
                    ("instruction", s(instruction)),
                    ("successor_args", Arr(vec![s(first_proof.to_str().unwrap()), s("continue")])),
                ],
            ),
            &|| false,
            "",
            "",
        )
        .unwrap();
        assert!(receipt.contains("successor pid "), "{receipt}");
        let stored = rows(&root.join("self/journal.jsonl"));
        let successor = get(&stored[0], "successor");
        assert_eq!(text(successor, "job"), job);
        assert_eq!(text(successor, "instruction"), instruction);
        let dumped = wait_bytes(&PathBuf::from(format!("{}.done", first_proof.display())));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[2], job.as_bytes());
        assert_eq!(parts[3], instruction.as_bytes());
        let second_content = "fn a() { let _ = 2; }\n";
        let second_proof = root.join("proof-unset");
        let absent = run_with(
            root,
            &spawn_fields(
                second_content,
                first_content,
                &text(&stored[0], "revision"),
                vec![("successor_args", Arr(vec![s(second_proof.to_str().unwrap()), s("continue")]))],
            ),
            &|| false,
            "",
            "",
        )
        .unwrap();
        assert!(absent.contains("successor pid "), "{absent}");
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 2);
        let successor = get(&stored[1], "successor");
        assert_eq!(text(successor, "mode"), "spawn");
        assert_eq!(get(successor, "job"), &s(""));
        assert_eq!(get(successor, "instruction"), &s(""));
        assert!(get(successor, "env").array().unwrap().is_empty());
        let dumped = wait_bytes(&PathBuf::from(format!("{}.done", second_proof.display())));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[2], b"");
        assert_eq!(parts[3], b"");
        assert_eq!(parts[4], b"job-unset");
        assert_eq!(parts[5], b"instruction-unset");
    }

    #[test]
    fn overbound_job_and_instruction_refuse_before_the_edit() {
        let _isolate = isolate();
        let scratch = Scratch::new("bounds");
        let root = scratch.path();
        let original = b"fn a() {}\n";
        write(&root.join("src/lib.rs"), original);
        let live = || false;
        let request = |key: &str, value: &str| {
            obj(vec![(
                "args",
                obj(vec![
                    ("path", s("src/lib.rs")),
                    ("content", s("fn a() { let _ = 1; }\n")),
                    ("previous_exists", Bool(true)),
                    ("approved_previous", s("fn a() {}\n")),
                    (key, s(value)),
                ]),
            )])
        };
        let error = run(root, &request("job", &"j".repeat(257)), &live).unwrap_err();
        assert_eq!(error, "job exceeds 256 bytes");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), original);
        let error = run(root, &request("instruction", &"i".repeat(16385)), &live).unwrap_err();
        assert_eq!(error, "instruction exceeds 16384 bytes");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), original);
    }

    #[test]
    fn journal_mode_is_none_without_reload_and_exec_records_exec_only_when_it_writes() {
        let _isolate = isolate();
        let scratch = Scratch::new("mode");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"not a program\n");
        green_builder(root);
        let first = "fn a() { let _ = 1; }\n";
        let receipt = run_with(root, &journal_request(first, "", Vec::new()), &|| false, "", "").unwrap();
        assert!(receipt.contains("recorded revision "), "{receipt}");
        assert!(
            !receipt.contains("successor pid"),
            "{receipt}; a journal leg with no reload must not report a spawned pid"
        );
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 1);
        assert_eq!(text(get(&stored[0], "successor"), "mode"), "none");
        assert_eq!(get(get(&stored[0], "successor"), "job"), &s(""));
        assert_eq!(get(get(&stored[0], "successor"), "instruction"), &s(""));
        assert_eq!(get(get(&stored[0], "successor"), "env"), &Null);
        let error = run_with(
            root,
            &journal_request(first, &sha256(b"wrong-parent"), vec![("reload", s("exec"))]),
            &|| false,
            "",
            "",
        )
        .unwrap_err();
        assert_eq!(
            error, "parent revision does not match the journal tail",
            "{error}; exec was refused before a revision was recorded, so there is no successor row to read"
        );
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), first);
        assert_eq!(rows(&root.join("self/journal.jsonl")).len(), 1);
        let second = "fn a() { let _ = 2; }\n";
        let error = run_with(
            root,
            &obj(vec![(
                "args",
                obj(vec![
                    ("path", s("src/lib.rs")),
                    ("content", s(second)),
                    ("previous_exists", Bool(true)),
                    ("approved_previous", s(first)),
                    ("builder", s("bash")),
                    ("builder_args", Arr(vec![s("headless.sh")])),
                    ("binary", s("bin")),
                    ("journal", s("self/journal.jsonl")),
                    ("rollback", s("self/rollback")),
                    ("parent", s(&text(&stored[0], "revision"))),
                    ("reload", s("exec")),
                    ("job", s("exec-job")),
                    ("instruction", s("exec-instruction")),
                ]),
            )]),
            &|| false,
            "",
            "",
        )
        .unwrap_err();
        assert!(
            error.starts_with("Successor exec failed"),
            "{error}; exec was refused and this process was not replaced"
        );
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), second);
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 2);
        assert_eq!(text(get(&stored[0], "successor"), "mode"), "none");
        let successor = get(&stored[1], "successor");
        assert_eq!(text(successor, "mode"), "exec");
        assert_eq!(get(successor, "pid"), &Num(std::process::id().to_string()));
        assert_eq!(text(successor, "job"), "exec-job");
        assert_eq!(text(successor, "instruction"), "exec-instruction");
        assert_eq!(
            get(successor, "env"), &Null,
            "exec must not claim it handed environment variables to a spawned child"
        );
    }

    fn wait_dead(pid: u64) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let path = format!("/proc/{pid}/stat");
        loop {
            match fs::read_to_string(&path) {
                Err(_) => return,
                Ok(text) => {
                    if let Some(end) = text.rfind(')') {
                        let state = text[end + 1..].split_whitespace().next().unwrap_or("");
                        if state == "Z" || state == "X" {
                            return;
                        }
                    }
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!("successor {pid} did not exit");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn tracked_state(pid: u64) -> Json {
        let want = Num(pid.to_string());
        successor_states()
            .into_iter()
            .find(|row| get(row, "pid") == &want)
            .unwrap_or_else(|| panic!("missing successor {pid}"))
    }

    #[test]
    fn effective_identity_prefers_the_caller_then_the_carried_value() {
        let _isolate = isolate();
        assert_eq!(effective("caller-job", "carried-job"), ("caller-job".into(), "caller"));
        assert_eq!(effective("", "carried-job"), ("carried-job".into(), "inherited"));
        assert_eq!(effective("", ""), (String::new(), "none"));
        assert_eq!(
            bound_carried("", &"j".repeat(JOB_MAX + 1), JOB_MAX, "job").unwrap_err(),
            "inherited job exceeds 256 bytes"
        );
        assert_eq!(
            bound_carried("", &"i".repeat(INSTRUCTION_MAX + 1), INSTRUCTION_MAX, "instruction").unwrap_err(),
            "inherited instruction exceeds 16384 bytes"
        );
        assert_eq!(bound_carried("kept", &"j".repeat(JOB_MAX + 1), JOB_MAX, "job").unwrap().1, "caller");
        assert!(bound_carried("", &"j".repeat(JOB_MAX), JOB_MAX, "job").is_ok());
    }

    #[test]
    fn tracked_quiet_row_records_job_source_none() {
        let _isolate = isolate();
        let _ = reap();
        let scratch = Scratch::new("job-source-none");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"not a program\n");
        green_builder(root);
        let content = "fn a() { let _ = 1; }\n";
        let receipt = run_with(root, &journal_request(content, "", Vec::new()), &|| false, "", "").unwrap();
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 1);
        addressed(&stored[0]);
        let successor = get(&stored[0], "successor");
        assert_eq!(text(successor, "mode"), "none");
        assert_eq!(text(successor, "job_source"), "none");
        assert_eq!(get(successor, "job"), &s(""));
        assert_eq!(get(successor, "instruction"), &s(""));
        assert_eq!(get(successor, "env"), &Null);
        let expected = format!(
            "Saved {} bytes to src/lib.rs. recorded revision {} in self/journal.jsonl Rust cases passed: 2/2; isolated application cases: 1",
            content.len(),
            text(&stored[0], "revision")
        );
        assert_eq!(receipt, expected);
    }

    #[test]
    fn tracked_carried_identity_is_recorded_and_handed_to_the_successor() {
        let _isolate = isolate();
        let _ = reap();
        let scratch = Scratch::new("carried");
        let root = scratch.path();
        let original = "fn a() {}\n";
        write(&root.join("src/lib.rs"), original.as_bytes());
        // The running check is after the spawn row and its mark are appended.
        // Hold the child open across that write so the check does not race the exit.
        let hold = root.join("hold-running");
        let body = SUCCESSOR_SCRIPT.strip_prefix("#!/bin/sh\n").unwrap();
        executable(
            &root.join("bin"),
            format!("#!/bin/sh\nwhile [ ! -f '{}' ]; do sleep 0.05; done\n{body}", hold.display()).as_bytes(),
        );
        green_builder(root);
        let first = "fn a() { let _ = 1; }\n";
        let first_proof = root.join("proof-inherited");
        let receipt = run_with(
            root,
            &spawn_fields(
                first,
                original,
                "",
                vec![("successor_args", Arr(vec![s(first_proof.to_str().unwrap()), s("continue")]))],
            ),
            &|| false,
            "carried-job",
            "carried-instruction",
        )
        .unwrap();
        assert!(!receipt.contains("reaped"), "{receipt}");
        let stored = rows(&root.join("self/journal.jsonl"));
        let successor = get(&stored[0], "successor");
        assert_eq!(text(successor, "job_source"), "inherited");
        assert_eq!(text(successor, "job"), "carried-job");
        assert_eq!(text(successor, "instruction"), "carried-instruction");
        assert_eq!(
            get(successor, "env").array().unwrap(),
            &vec![s("RLLM_SELF_JOB"), s("RLLM_SELF_INSTRUCTION")]
        );
        let Num(first_pid) = get(successor, "pid") else { panic!("successor pid is not a number") };
        let first_pid_num = first_pid.parse::<u64>().unwrap();
        let running = tracked_state(first_pid_num);
        assert_eq!(get(&running, "running"), &Bool(true));
        assert_eq!(get(&running, "exit"), &Null);
        assert_eq!(text(&running, "job"), "carried-job");
        assert_eq!(text(&running, "job_source"), "inherited");
        write(&hold, b"go");
        let dumped = wait_bytes(&PathBuf::from(format!("{}.done", first_proof.display())));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[2], b"carried-job");
        assert_eq!(parts[3], b"carried-instruction");
        assert_eq!(parts[4], b"job-set");
        assert_eq!(parts[5], b"instruction-set");
        wait_dead(first_pid_num);
        let second = "fn a() { let _ = 2; }\n";
        let second_proof = root.join("proof-caller");
        let receipt = run_with(
            root,
            &spawn_fields(
                second,
                first,
                &text(&stored[0], "revision"),
                vec![
                    ("job", s("caller-job")),
                    ("instruction", s("caller-instruction")),
                    ("successor_args", Arr(vec![s(second_proof.to_str().unwrap()), s("continue")])),
                ],
            ),
            &|| false,
            "carried-job",
            "carried-instruction",
        )
        .unwrap();
        assert!(receipt.ends_with(" reaped 1 successor(s)"), "{receipt}");
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 2);
        let successor = get(&stored[1], "successor");
        assert_eq!(text(successor, "job_source"), "caller");
        assert_eq!(text(successor, "job"), "caller-job");
        assert_eq!(text(successor, "instruction"), "caller-instruction");
        assert_eq!(
            get(successor, "env").array().unwrap(),
            &vec![s("RLLM_SELF_JOB"), s("RLLM_SELF_INSTRUCTION")]
        );
        let dumped = wait_bytes(&PathBuf::from(format!("{}.done", second_proof.display())));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[2], b"caller-job");
        assert_eq!(parts[3], b"caller-instruction");
        let Num(second_pid) = get(successor, "pid") else { panic!("successor pid is not a number") };
        wait_dead(second_pid.parse::<u64>().unwrap());
        let finished = tracked_state(first_pid_num);
        assert_eq!(get(&finished, "running"), &Bool(false));
        assert_eq!(get(&finished, "exit"), &Num("0".into()));
        let before = fs::read(root.join("src/lib.rs")).unwrap();
        assert_eq!(before, second.as_bytes());
        let refused = obj(vec![(
            "args",
            obj(vec![
                ("path", s("src/lib.rs")),
                ("content", s("fn a() { let _ = 3; }\n")),
                ("previous_exists", Bool(true)),
                ("approved_previous", s(second)),
            ]),
        )]);
        let error = run_with(root, &refused, &|| false, &"j".repeat(JOB_MAX + 1), "short").unwrap_err();
        assert_eq!(error, "inherited job exceeds 256 bytes");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), before);
        assert_eq!(rows(&root.join("self/journal.jsonl")).len(), 2);
        let caller_over = obj(vec![(
            "args",
            obj(vec![
                ("path", s("src/lib.rs")),
                ("content", s("fn a() { let _ = 3; }\n")),
                ("previous_exists", Bool(true)),
                ("approved_previous", s(second)),
                ("job", s(&"j".repeat(JOB_MAX + 1))),
            ]),
        )]);
        let error = run_with(root, &caller_over, &|| false, "ok", "short").unwrap_err();
        assert_eq!(error, "job exceeds 256 bytes");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), before);
        let error = run_with(root, &refused, &|| false, "ok", &"i".repeat(INSTRUCTION_MAX + 1)).unwrap_err();
        assert_eq!(error, "inherited instruction exceeds 16384 bytes");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), before);
        assert_eq!(rows(&root.join("self/journal.jsonl")).len(), 2);
    }

    #[test]
    fn tracked_reap_reports_an_exited_successor_once() {
        let _isolate = isolate();
        let _ = reap();
        let scratch = Scratch::new("reap");
        let hold_path = scratch.path().join("hold");
        let quick_pid = track(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("exit 5")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "done-job",
            "inherited",
            "spawn",
        );
        let hold_pid = track(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("while [ ! -f \"$1\" ]; do sleep 0.05; done; exit 4")
                .arg("sh")
                .arg(&hold_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "held-job",
            "caller",
            "spawn",
        );
        wait_dead(quick_pid);
        assert_eq!(get(&tracked_state(quick_pid), "running"), &Bool(false));
        assert_eq!(get(&tracked_state(quick_pid), "exit"), &Num("5".into()));
        assert_eq!(get(&tracked_state(hold_pid), "running"), &Bool(true));
        assert_eq!(get(&tracked_state(hold_pid), "exit"), &Null);
        assert_eq!(text(&tracked_state(hold_pid), "job"), "held-job");
        assert_eq!(text(&tracked_state(hold_pid), "job_source"), "caller");
        assert_eq!(reap().unwrap(), 1);
        assert_eq!(reap().unwrap(), 0);
        assert_eq!(get(&tracked_state(quick_pid), "running"), &Bool(false));
        assert_eq!(get(&tracked_state(quick_pid), "exit"), &Num("5".into()));
        assert_eq!(text(&tracked_state(quick_pid), "job"), "done-job");
        assert_eq!(text(&tracked_state(quick_pid), "job_source"), "inherited");
        assert_eq!(get(&tracked_state(hold_pid), "running"), &Bool(true));
        assert_eq!(get(&tracked_state(hold_pid), "exit"), &Null);
        write(&hold_path, b"go");
        wait_dead(hold_pid);
        assert_eq!(reap().unwrap(), 1);
        assert_eq!(reap().unwrap(), 0);
        assert_eq!(get(&tracked_state(hold_pid), "running"), &Bool(false));
        assert_eq!(get(&tracked_state(hold_pid), "exit"), &Num("4".into()));
    }

    #[test]
    fn tracked_reaped_text_appears_only_after_a_successor_exits() {
        let _isolate = isolate();
        let _ = reap();
        let scratch = Scratch::new("reaped-text");
        let root = scratch.path();
        let original = "fn a() {}\n";
        write(&root.join("src/lib.rs"), original.as_bytes());
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        green_builder(root);
        let live = || false;
        let first = "fn a() { let _ = 1; }\n";
        let quiet = run(
            root,
            &obj(vec![(
                "args",
                obj(vec![
                    ("path", s("src/lib.rs")),
                    ("content", s(first)),
                    ("previous_exists", Bool(true)),
                    ("approved_previous", s(original)),
                ]),
            )]),
            &live,
        )
        .unwrap();
        let quiet_expected = format!(
            "Saved {} bytes to src/lib.rs. self-build reviewed; no builder was requested",
            first.len()
        );
        assert_eq!(quiet, quiet_expected);
        let second = "fn a() { let _ = 2; }\n";
        let proof = root.join("proof-reap");
        let spawned = run(
            root,
            &spawn_fields(
                second,
                first,
                "",
                vec![("successor_args", Arr(vec![s(proof.to_str().unwrap()), s("continue")]))],
            ),
            &live,
        )
        .unwrap();
        let stored = rows(&root.join("self/journal.jsonl"));
        let Num(pid) = get(get(&stored[0], "successor"), "pid") else { panic!("successor pid is not a number") };
        let spawned_expected = format!(
            "Saved {} bytes to src/lib.rs. recorded revision {} in self/journal.jsonl successor pid {pid}",
            second.len(),
            text(&stored[0], "revision")
        );
        assert_eq!(spawned, spawned_expected);
        assert!(!spawned.contains("reaped"), "{spawned}");
        wait_bytes(&PathBuf::from(format!("{}.done", proof.display())));
        wait_dead(pid.parse::<u64>().unwrap());
        let third = "fn a() { let _ = 3; }\n";
        let again = run(
            root,
            &obj(vec![(
                "args",
                obj(vec![
                    ("path", s("src/lib.rs")),
                    ("content", s(third)),
                    ("previous_exists", Bool(true)),
                    ("approved_previous", s(second)),
                ]),
            )]),
            &live,
        )
        .unwrap();
        let again_base = format!(
            "Saved {} bytes to src/lib.rs. self-build reviewed; no builder was requested",
            third.len()
        );
        assert_eq!(again, format!("{again_base} reaped 1 successor(s)"));
    }

    fn carried_probe() -> String {
        let job = "if [ -n \"${RLLM_SELF_JOB+x}\" ]; then printf 'job-set:%s\\n' \"$RLLM_SELF_JOB\"; else printf 'job-unset\\n'; fi";
        let instruction = "if [ -n \"${RLLM_SELF_INSTRUCTION+x}\" ]; then printf 'instruction-set:%s\\n' \"$RLLM_SELF_INSTRUCTION\"; else printf 'instruction-unset\\n'; fi";
        format!("{job}\n{instruction}\n")
    }
    fn child_text(output: &std::process::Output) -> String {
        format!(
            "status {:?}\nstdout {}\nstderr {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }
    fn test_child(extra: &[(&str, &str)], exact: &str) -> std::process::Output {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.stdin(Stdio::null()).args(["--test-threads=1", "--exact", exact]);
        for (key, value) in extra {
            child.env(key, value);
        }
        child.output().unwrap()
    }
    fn assert_builder_and_gate_are_scrubbed() {
        let probe = carried_probe();
        let inherited = Command::new("/bin/sh").arg("-c").arg(&probe).output().unwrap();
        assert!(inherited.status.success(), "{}", String::from_utf8_lossy(&inherited.stderr));
        assert_eq!(
            String::from_utf8(inherited.stdout).unwrap(),
            "job-set:gap1-chain-carried\ninstruction-set:carried-instruction\n"
        );
        let mut scrubbed = Command::new("/bin/sh");
        scrubbed.arg("-c").arg(&probe);
        scrub_carried(&mut scrubbed);
        let scrubbed = scrubbed.output().unwrap();
        assert!(scrubbed.status.success(), "{}", String::from_utf8_lossy(&scrubbed.stderr));
        assert_eq!(String::from_utf8(scrubbed.stdout).unwrap(), "job-unset\ninstruction-unset\n");
        let scratch = Scratch::new("scrub-children");
        let root = scratch.path();
        write(&root.join("headless.sh"), format!("#!/bin/sh\n{probe}").as_bytes());
        fs::create_dir_all(root.join("output/tools")).unwrap();
        executable(&root.join("output/tools/gate"), format!("#!/bin/sh\n{probe}").as_bytes());
        let built = sanctioned_child(root, "builder", "bash", &["headless.sh".to_string()], Duration::from_secs(5), &|| false).unwrap();
        assert_eq!(built.code, Some(0), "{}", built.stderr);
        assert_eq!(built.stdout, "job-unset\ninstruction-unset\n");
        let gated = sanctioned_child(
            root,
            "gate",
            "output/tools/gate",
            &["units".to_string()],
            Duration::from_secs(5),
            &|| false,
        )
        .unwrap();
        assert_eq!(gated.code, Some(0), "{}", gated.stderr);
        assert_eq!(gated.stdout, "job-unset\ninstruction-unset\n");
        let seen = root.join("builder-saw");
        write(
            &root.join("headless.sh"),
            format!(
                "#!/bin/sh\n{{\n{probe}}} > '{}'\nprintf '%s\\n' 'Rust cases passed: 2/2; isolated application cases: 1'\n",
                seen.display()
            )
            .as_bytes(),
        );
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"binary-v1");
        run(root, &journal_request("fn a() { let _ = 1; }\n", "", Vec::new()), &|| false).unwrap();
        let stored = rows(&root.join("self/journal.jsonl"));
        let successor = get(&stored[0], "successor");
        assert_eq!(text(successor, "job"), "gap1-chain-carried");
        assert_eq!(text(successor, "instruction"), "carried-instruction");
        assert_eq!(text(successor, "job_source"), "inherited");
        assert_eq!(fs::read_to_string(&seen).unwrap(), "job-unset\ninstruction-unset\n");
    }

    #[test]
    fn builder_and_gate_children_do_not_inherit_the_carried_names() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("scrub") {
            assert_builder_and_gate_are_scrubbed();
            return;
        }
        let output = test_child(
            &[
                ("RLLM_SELF_JOB", "gap1-chain-carried"),
                ("RLLM_SELF_INSTRUCTION", "carried-instruction"),
                ("RLLM_SELF_BUILD_PROBE", "scrub"),
            ],
            "self_build::checks::builder_and_gate_children_do_not_inherit_the_carried_names",
        );
        assert!(output.status.success(), "{}", child_text(&output));
    }

    fn exec_handoff() {
        let root = PathBuf::from(std::env::var("RLLM_EXEC_ROOT").unwrap());
        let proof = std::env::var("RLLM_EXEC_PROOF").unwrap();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        green_builder(&root);
        let script = format!(
            "#!/bin/sh\n{{\nprintf '%s\\0' \"$RLLM_SELF_JOB\"\nprintf '%s\\0' \"$RLLM_SELF_INSTRUCTION\"\nif [ -n \"${{RLLM_SELF_JOB+x}}\" ]; then printf 'job-set\\0'; else printf 'job-unset\\0'; fi\nif [ -n \"${{RLLM_SELF_INSTRUCTION+x}}\" ]; then printf 'instruction-set\\0'; else printf 'instruction-unset\\0'; fi\n}} > '{proof}.part'\nmv '{proof}.part' '{proof}'\n"
        );
        executable(&root.join("bin"), script.as_bytes());
        let result = run_with(
            &root,
            &journal_request("fn a() { let _ = 1; }\n", "", vec![("reload", s("exec"))]),
            &|| false,
            "exec-carried-job",
            "exec-carried-instruction",
        );
        panic!("exec returned {result:?}");
    }

    #[test]
    fn reload_arms_hand_both_carried_names() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("exec-handoff") {
            exec_handoff();
            return;
        }
        let scratch = Scratch::new("spawn-handoff");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        green_builder(root);
        let proof = root.join("spawn-proof");
        run_with(
            root,
            &spawn_fields(
                "fn a() { let _ = 1; }\n",
                "fn a() {}\n",
                "",
                vec![("successor_args", Arr(vec![s(proof.to_str().unwrap()), s("continue")]))],
            ),
            &|| false,
            "spawn-carried-job",
            "spawn-carried-instruction",
        )
        .unwrap();
        let dumped = wait_bytes(&PathBuf::from(format!("{}.done", proof.display())));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[2], b"spawn-carried-job");
        assert_eq!(parts[3], b"spawn-carried-instruction");
        assert_eq!(parts[4], b"job-set");
        assert_eq!(parts[5], b"instruction-set");
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(text(get(&stored[0], "successor"), "job_source"), "inherited");
        let exec_root = Scratch::new("exec-handoff");
        let proof_file = exec_root.path().join("proof");
        let output = test_child(
            &[
                ("RLLM_SELF_BUILD_PROBE", "exec-handoff"),
                ("RLLM_EXEC_ROOT", exec_root.path().to_str().unwrap()),
                ("RLLM_EXEC_PROOF", proof_file.to_str().unwrap()),
            ],
            "self_build::checks::reload_arms_hand_both_carried_names",
        );
        let dumped = fs::read(&proof_file).unwrap_or_else(|error| panic!("{error}\n{}", child_text(&output)));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[0], b"exec-carried-job", "{}", child_text(&output));
        assert_eq!(parts[1], b"exec-carried-instruction");
        assert_eq!(parts[2], b"job-set");
        assert_eq!(parts[3], b"instruction-set");
        assert!(output.status.success(), "{}", child_text(&output));
        let stored = rows(&exec_root.path().join("self/journal.jsonl"));
        let successor = get(&stored[0], "successor");
        assert_eq!(text(successor, "mode"), "exec");
        assert_eq!(text(successor, "job"), "exec-carried-job");
        assert_eq!(text(successor, "instruction"), "exec-carried-instruction");
        assert_eq!(text(successor, "job_source"), "inherited");
    }

    #[test]
    fn short_builder_output_matches_the_runner_and_a_bound_names_the_exit_and_tail() {
        let _isolate = isolate();
        let scratch = Scratch::new("bound-output");
        let root = scratch.path();
        let small = root.join("small.sh");
        write(&small, b"#!/bin/sh\nprintf 'stdout-marker-17\\n'\nprintf 'stderr-marker-19\\n' >&2\nexit 4\n");
        let args = vec![small.to_string_lossy().into_owned()];
        let seam = bounded_child(root, "bash", &args, Duration::from_secs(5), &|| false).unwrap();
        let runner = super::super::execution::bounded_process("bash", &args, root, Duration::from_secs(5), &|| false).unwrap();
        assert_eq!(seam.code, runner.code);
        assert_eq!(seam.stdout, runner.stdout);
        assert_eq!(seam.stderr, runner.stderr);
        assert_eq!(seam.code, Some(4));
        assert_eq!(seam.stdout, "stdout-marker-17\n");
        assert_eq!(seam.stderr, "stderr-marker-19\n");
        let mut bytes = vec![b'a'; OUTPUT_MAX + 8192];
        let marker = b"END-OF-STDERR-MARKER";
        let marker_at = bytes.len() - marker.len();
        bytes[marker_at..].copy_from_slice(marker);
        let payload = root.join("payload");
        write(&payload, &bytes);
        let spill = root.join("spill.sh");
        write(&spill, b"#!/bin/sh\ncat \"$1\" >&2\nexit 1\n");
        let error = match bounded_child(
            root,
            "bash",
            &[spill.to_string_lossy().into_owned(), payload.to_string_lossy().into_owned()],
            Duration::from_secs(10),
            &|| false,
        ) {
            Err(error) => error,
            Ok(_) => panic!("stderr over the bound was accepted"),
        };
        assert!(error.starts_with("builder output exceeded its bound: exit Some(1); tail "), "{error}");
        assert!(error.ends_with("END-OF-STDERR-MARKER"), "{error}");
        assert!(error.len() < 600, "{}", error.len());
        let stdout_spill = root.join("stdout-spill.sh");
        write(&stdout_spill, b"#!/bin/sh\ncat \"$1\"\nexit 2\n");
        let error = match bounded_child(
            root,
            "bash",
            &[stdout_spill.to_string_lossy().into_owned(), payload.to_string_lossy().into_owned()],
            Duration::from_secs(10),
            &|| false,
        ) {
            Err(error) => error,
            Ok(_) => panic!("stdout over the bound was accepted"),
        };
        assert!(error.starts_with("builder output exceeded its bound: exit Some(2); tail "), "{error}");
        assert!(error.ends_with("END-OF-STDERR-MARKER"), "{error}");
    }

    #[test]
    fn carried_environment_does_not_fail_the_journal_cases() {
        let _isolate = isolate();
        for name in [
            "self_build::checks::journal_mode_is_none_without_reload_and_exec_records_exec_only_when_it_writes",
            "self_build::checks::spawn_journal_stores_job_and_instruction_verbatim_and_absent_job_is_empty",
        ] {
            let output = test_child(
                &[("RLLM_SELF_JOB", "gap1-chain-carried"), ("RLLM_SELF_INSTRUCTION", "carried-instruction")],
                name,
            );
            assert!(output.status.success(), "{name}\n{}", child_text(&output));
        }
    }

    fn refusal<T>(result: Result<T>) -> String {
        match result {
            Err(error) => error,
            Ok(_) => panic!("expected a refusal"),
        }
    }
    fn subject_stamp(path: &Path) -> (u64, u64, String) {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::symlink_metadata(path).unwrap();
        let bytes = fs::read(path).unwrap();
        (meta.ino(), meta.len(), sha256(&bytes))
    }
    fn refused_shape(extra: Vec<(&str, Json)>) -> Json {
        let mut fields = vec![
            ("path", s("src/lib.rs")),
            ("content", s("fn marker() { let _ = \"unsanctioned\"; }\n")),
            ("previous_exists", Bool(true)),
            ("approved_previous", s("fn a() {}\n")),
            ("binary", s("bin")),
            ("journal", s("self/journal.jsonl")),
            ("rollback", s("self/rollback")),
            ("parent", s("")),
            ("reload", s("spawn")),
            ("job", s("lane-h-refuse")),
            ("instruction", s("do not run this command")),
        ];
        fields.extend(extra);
        obj(vec![("args", obj(fields))])
    }
    fn assert_refused_before_write(root: &Path, request: &Json, reason: &str) {
        let source = root.join("src/lib.rs");
        let before = subject_stamp(&source);
        let before_bytes = fs::read(&source).unwrap();
        let journal = root.join("self/journal.jsonl");
        let rollback = root.join("self/rollback");
        assert!(!journal.exists());
        assert!(!rollback.exists());
        let error = run(root, request, &|| false).unwrap_err();
        assert_eq!(error, reason);
        assert_eq!(subject_stamp(&source), before, "subject inode, size, or sha256 changed");
        assert_eq!(fs::read(&source).unwrap(), before_bytes);
        assert!(!journal.exists(), "a refused command appended a journal revision");
        assert!(!rollback.exists(), "a refused command created a rollback copy");
        assert!(successor_states().is_empty(), "a refused command tracked a successor");
        let pending = fs::read_dir(root.join("src"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name().to_string_lossy().starts_with(".pending-"));
        assert!(!pending, "a refused command left a pending file");
    }

    #[test]
    fn unsanctioned_builder_refuses_before_the_source_bytes_change() {
        let _isolate = isolate();
        let scratch = Scratch::new("unsanctioned-builder");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"binary-v1");
        assert_refused_before_write(
            root,
            &refused_shape(vec![
                ("builder", s("bash")),
                ("builder_args", Arr(vec![s("echo"), s("unsanctioned")])),
            ]),
            "builder is not one of the sanctioned commands",
        );
    }

    #[test]
    fn unsanctioned_gate_refuses_before_the_source_bytes_change() {
        let _isolate = isolate();
        let scratch = Scratch::new("unsanctioned-gate");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"binary-v1");
        assert_refused_before_write(
            root,
            &refused_shape(vec![
                ("gate", s("bash")),
                ("gate_args", Arr(vec![s("echo"), s("unsanctioned")])),
            ]),
            "gate is not one of the sanctioned commands",
        );
    }

    #[test]
    fn builder_with_an_extra_argument_refuses_before_the_source_bytes_change() {
        let _isolate = isolate();
        let scratch = Scratch::new("extra-builder-arg");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"binary-v1");
        assert_refused_before_write(
            root,
            &refused_shape(vec![
                ("builder", s("bash")),
                (
                    "builder_args",
                    Arr(vec![s("tools/build-units.sh"), s("output/units"), s("extra")]),
                ),
            ]),
            "builder is not one of the sanctioned commands",
        );
    }

    #[test]
    fn sanctioned_spawn_still_edits_rolls_back_journals_and_hands_both_carried_names() {
        let _isolate = isolate();
        let scratch = Scratch::new("sanctioned-spawn");
        let root = scratch.path();
        let original = "fn a() {}\n";
        write(&root.join("src/lib.rs"), original.as_bytes());
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        let binary_before = fs::read(root.join("bin")).unwrap();
        green_builder(root);
        let content = "fn a() { let _ = 1; }\n";
        let proof = root.join("proof-shape");
        let source = root.join("src/lib.rs");
        let source_before = subject_stamp(&source);
        let receipt = run_with(
            root,
            &spawn_fields(
                content,
                original,
                "",
                vec![("successor_args", Arr(vec![s(proof.to_str().unwrap()), s("continue")]))],
            ),
            &|| false,
            "carried-job",
            "carried-instruction",
        )
        .unwrap();
        assert_ne!(subject_stamp(&source), source_before);
        assert_eq!(fs::read_to_string(&source).unwrap(), content);
        assert_eq!(fs::read(root.join("self/rollback")).unwrap(), binary_before);
        let stored = rows(&root.join("self/journal.jsonl"));
        assert_eq!(stored.len(), 1);
        addressed(&stored[0]);
        let successor = get(&stored[0], "successor");
        assert_eq!(text(successor, "mode"), "spawn");
        assert_eq!(text(successor, "job_source"), "inherited");
        assert_eq!(text(successor, "job"), "carried-job");
        assert_eq!(text(successor, "instruction"), "carried-instruction");
        assert_eq!(
            get(successor, "env").array().unwrap(),
            &vec![s("RLLM_SELF_JOB"), s("RLLM_SELF_INSTRUCTION")]
        );
        assert!(receipt.contains("successor pid "), "{receipt}");
        let dumped = wait_bytes(&PathBuf::from(format!("{}.done", proof.display())));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[2], b"carried-job");
        assert_eq!(parts[3], b"carried-instruction");
        assert_eq!(parts[4], b"job-set");
        assert_eq!(parts[5], b"instruction-set");
        let Num(pid) = get(successor, "pid") else { panic!("successor pid is not a number") };
        wait_dead(pid.parse::<u64>().unwrap());
    }

    /// A local copy with the build-units arm removed. The production sites must
    /// keep agreeing with `require_sanctioned`, not with this copy.
    fn copy_without_build_units(program: &str, args: &[String]) -> bool {
        match (program, args) {
            ("bash", [only]) if only == "headless.sh" => true,
            (GATE_REL, [units]) if relative(units).is_ok() => true,
            _ => false,
        }
    }

    #[test]
    fn preflight_and_spawn_use_the_same_sanctioned_classifier() {
        let _isolate = isolate();
        let scratch = Scratch::new("same-classifier");
        let root = scratch.path();
        let units = vec!["tools/build-units.sh".to_string(), "output/units".to_string()];
        assert!(classify_sanctioned("bash", &units).is_some());
        assert!(
            !copy_without_build_units("bash", &units),
            "the local copy still contains the build-units arm"
        );
        let accepted = sanctioned_child(root, "builder", "bash", &units, Duration::from_secs(5), &|| false);
        let accepted = match accepted {
            Ok(output) => output,
            Err(error) => panic!("spawn site refused a sanctioned arm: {error}"),
        };
        assert_ne!(accepted.code, Some(0), "a missing build script was treated as success");
        let shape = obj(vec![
            ("builder", s("bash")),
            ("builder_args", Arr(vec![s("tools/build-units.sh"), s("output/units")])),
            ("reload", s("")),
        ]);
        assert!(require_command_shape(root, &shape).is_ok());
        let extra = vec![
            "tools/build-units.sh".to_string(),
            "output/units".to_string(),
            "extra".to_string(),
        ];
        assert!(classify_sanctioned("bash", &extra).is_none());
        assert!(!copy_without_build_units("bash", &extra));
        let from_classifier = refusal(require_sanctioned("builder", "bash", &extra));
        let from_spawn = refusal(sanctioned_child(root, "builder", "bash", &extra, Duration::from_secs(5), &|| false));
        let from_preflight = require_command_shape(
            root,
            &obj(vec![
                ("builder", s("bash")),
                (
                    "builder_args",
                    Arr(vec![s("tools/build-units.sh"), s("output/units"), s("extra")]),
                ),
                ("reload", s("")),
            ]),
        )
        .unwrap_err();
        assert_eq!(from_classifier, "builder is not one of the sanctioned commands");
        assert_eq!(from_spawn, from_classifier);
        assert_eq!(from_preflight, from_classifier);
        let bad_gate = vec!["echo".to_string(), "unsanctioned".to_string()];
        let gate_classifier = refusal(require_sanctioned("gate", "bash", &bad_gate));
        let gate_spawn = refusal(sanctioned_child(root, "gate", "bash", &bad_gate, Duration::from_secs(5), &|| false));
        let gate_preflight = require_command_shape(
            root,
            &obj(vec![
                ("gate", s("bash")),
                ("gate_args", Arr(vec![s("echo"), s("unsanctioned")])),
                ("reload", s("")),
            ]),
        )
        .unwrap_err();
        assert_eq!(gate_classifier, "gate is not one of the sanctioned commands");
        assert_eq!(gate_spawn, gate_classifier);
        assert_eq!(gate_preflight, gate_classifier);
    }

    fn unix_now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
    }

    #[test]
    fn spawn_handoff_appends_one_durable_successor_row() {
        let _isolate = isolate();
        let scratch = Scratch::new("durable-spawn");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        green_builder(root);
        let proof = root.join("proof-durable");
        let before = unix_now();
        let content = "fn a() { let _ = 1; }\n";
        let job = "durable-job";
        let receipt = run(
            root,
            &spawn_fields(
                content,
                "fn a() {}\n",
                "",
                vec![
                    ("job", s(job)),
                    ("successor_args", Arr(vec![s(proof.to_str().unwrap()), s("continue")])),
                ],
            ),
            &|| false,
        )
        .unwrap();
        let journal = root.join("self/journal.jsonl");
        let stored = rows(&journal);
        assert_eq!(stored.len(), 1, "the revision journal stays one build row");
        let witness = successor_journal_path(&journal);
        assert_eq!(witness, root.join("self/journal.successors.jsonl"));
        let lines = rows(&witness);
        assert_eq!(lines.len(), 1);
        let row = &lines[0];
        addressed(row);
        assert_eq!(text(row, "event"), "spawn");
        assert_eq!(text(row, "job"), job);
        assert_eq!(text(row, "mode"), "spawn");
        assert_eq!(text(row, "source_digest"), text(&stored[0], "source_digest"));
        assert_eq!(text(row, "parent"), "");
        assert_eq!(get(row, "exit"), &Null);
        assert_eq!(get(row, "pid"), get(get(&stored[0], "successor"), "pid"));
        let Num(stamp) = get(row, "timestamp") else { panic!("timestamp is not a number") };
        let stamp = stamp.parse::<u64>().unwrap();
        assert!(stamp >= before && stamp <= unix_now() + 1, "{stamp} outside {before}..");
        let read = durable_successor_rows(&journal).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(text(&read[0], "job"), job);
        assert_eq!(text(&read[0], "mode"), "spawn");
        assert_eq!(text(&read[0], "source_digest"), text(&stored[0], "source_digest"));
        assert!(receipt.contains("successor pid "), "{receipt}");
        wait_bytes(&PathBuf::from(format!("{}.done", proof.display())));
        let Num(pid) = get(row, "pid") else { panic!("successor pid is not a number") };
        wait_dead(pid.parse::<u64>().unwrap());
        assert_eq!(reap().unwrap(), 1);
    }

    #[test]
    fn reap_appends_the_child_exit_code_durably() {
        let _isolate = isolate();
        let scratch = Scratch::new("durable-reap");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        green_builder(root);
        // /bin/sh is a symlink. The seam only reads a regular file, so use the
        // program that symlink names. The child is still `sh -c exit 9`.
        let shell = fs::canonicalize("/bin/sh").unwrap();
        let content = "fn a() { let _ = 1; }\n";
        let job = "reap-job";
        let receipt = run(
            root,
            &spawn_fields(
                content,
                "fn a() {}\n",
                "",
                vec![
                    ("binary", s(shell.to_str().unwrap())),
                    ("job", s(job)),
                    ("successor_args", Arr(vec![s("-c"), s("exit 9")])),
                ],
            ),
            &|| false,
        )
        .unwrap();
        assert!(receipt.contains("successor pid "), "{receipt}");
        let journal = root.join("self/journal.jsonl");
        let spawned = durable_successor_rows(&journal).unwrap();
        assert_eq!(spawned.len(), 1);
        assert_eq!(text(&spawned[0], "event"), "spawn");
        let Num(pid_text) = get(&spawned[0], "pid") else { panic!("successor pid is not a number") };
        let pid = pid_text.parse::<u64>().unwrap();
        wait_dead(pid);
        assert_eq!(reap().unwrap(), 1);
        assert_eq!(reap().unwrap(), 0);
        let rows = durable_successor_rows(&journal).unwrap();
        assert_eq!(rows.len(), 2);
        addressed(&rows[0]);
        addressed(&rows[1]);
        assert_eq!(text(&rows[1], "event"), "reap");
        assert_eq!(text(&rows[1], "job"), job);
        assert_eq!(get(&rows[1], "pid"), &Num(pid.to_string()));
        assert_eq!(get(&rows[1], "exit"), &Num("9".into()));
        assert_eq!(get(&rows[1], "signal"), &Null);
        assert_eq!(text(&rows[1], "parent"), text(&rows[0], "revision"));
        let state = tracked_state(pid);
        assert_eq!(get(&state, "running"), &Bool(false));
        assert_eq!(get(&state, "exit"), &Num("9".into()));
        assert_eq!(text(&state, "job"), job);
    }

    #[test]
    fn durable_successor_rows_read_from_the_file_without_the_process_table() {
        let _isolate = isolate();
        let scratch = Scratch::new("durable-fresh");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        executable(&root.join("bin"), b"#!/bin/sh\nexit 3\n");
        green_builder(root);
        let content = "fn a() { let _ = 1; }\n";
        run(
            root,
            &spawn_fields(content, "fn a() {}\n", "", vec![("job", s("fresh-job"))]),
            &|| false,
        )
        .unwrap();
        let journal = root.join("self/journal.jsonl");
        let spawned = durable_successor_rows(&journal).unwrap();
        assert_eq!(spawned.len(), 1);
        let Num(pid_text) = get(&spawned[0], "pid") else { panic!("successor pid is not a number") };
        wait_dead(pid_text.parse::<u64>().unwrap());
        assert_eq!(reap().unwrap(), 1);
        let expected = durable_successor_rows(&journal).unwrap();
        assert_eq!(expected.len(), 2);
        assert_eq!(text(&expected[0], "event"), "spawn");
        assert_eq!(text(&expected[0], "job"), "fresh-job");
        assert_eq!(text(&expected[1], "event"), "reap");
        assert_eq!(get(&expected[1], "exit"), &Num("3".into()));
        successors().clear();
        assert!(successor_states().is_empty(), "clearing the table left an in-process successor");
        assert_eq!(durable_successor_rows(&journal).unwrap(), expected);
    }

    #[test]
    fn refused_self_build_writes_no_durable_successor_file() {
        let _isolate = isolate();
        let scratch = Scratch::new("durable-refuse");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"binary-v1");
        let journal = root.join("self/journal.jsonl");
        let companion = successor_journal_path(&journal);
        assert!(!companion.exists());
        assert_refused_before_write(
            root,
            &refused_shape(vec![
                ("builder", s("bash")),
                ("builder_args", Arr(vec![s("echo"), s("unsanctioned")])),
            ]),
            "builder is not one of the sanctioned commands",
        );
        assert!(!journal.exists(), "a refused command appended a journal revision");
        assert!(!companion.exists(), "a refused command created the successor journal");
        assert!(!successor_mark_path(&journal).exists(), "a refused command created the successor mark");
        assert!(durable_successor_rows(&journal).unwrap().is_empty());
        assert!(!companion.exists(), "reading successors created a file");
        assert!(successor_states().is_empty(), "a refused command tracked a successor");
    }

    fn successor_lines(journal: &Path) -> Vec<String> {
        fs::read_to_string(successor_journal_path(journal))
            .unwrap()
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }
    fn store_successor_lines(journal: &Path, lines: &[String]) {
        let mut text = lines.join("\n");
        text.push('\n');
        fs::write(successor_journal_path(journal), text).unwrap();
    }
    fn three_linked_successor_rows(name: &str) -> (Scratch, PathBuf) {
        let scratch = Scratch::new(name);
        let journal = scratch.path().join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        append_spawn_row(&witness, 41, "job-a", &sha256(b"source-a"), "spawn").unwrap();
        append_reap_row(&witness, 41, "job-a", Some(9), None).unwrap();
        append_spawn_row(&witness, 42, "job-b", &sha256(b"source-b"), "spawn").unwrap();
        let read = durable_successor_rows(&journal).unwrap();
        assert_eq!(read.len(), 3);
        assert_eq!(text(&read[1], "event"), "reap");
        assert_eq!(text(&read[1], "parent"), text(&read[0], "revision"));
        assert_eq!(text(&read[2], "parent"), text(&read[1], "revision"));
        (scratch, journal)
    }

    #[test]
    fn failed_reap_append_keeps_the_exit_and_does_not_journal_it_twice() {
        let _isolate = isolate();
        let scratch = Scratch::new("reap-append-fail");
        let journal = scratch.path().join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        append_spawn_row(&witness, 7, "seed-job", &sha256(b"seed-source"), "spawn").unwrap();
        let saved = fs::read(&witness).unwrap();
        let pid = track_with(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("exit 9")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "reap-job",
            "caller",
            "spawn",
            Some(witness.clone()),
        );
        wait_dead(pid);
        fs::remove_file(&witness).unwrap();
        fs::create_dir(&witness).unwrap();
        assert_eq!(reap().unwrap_err(), "journal must be a regular non-symlink file");
        let state = tracked_state(pid);
        assert_eq!(get(&state, "running"), &Bool(false));
        assert_eq!(get(&state, "exit"), &Num("9".into()));
        assert_eq!(text(&state, "job"), "reap-job");
        fs::remove_dir(&witness).unwrap();
        fs::write(&witness, &saved).unwrap();
        assert_eq!(reap().unwrap(), 0);
        assert_eq!(fs::read(&witness).unwrap(), saved);
        let rows = durable_successor_rows(&journal).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(text(&rows[0], "event"), "spawn");
        assert_eq!(get(&tracked_state(pid), "exit"), &Num("9".into()));
    }

    #[test]
    fn durable_successor_rows_refuse_a_flipped_reap_payload_on_the_digest() {
        let _isolate = isolate();
        let (_scratch, journal) = three_linked_successor_rows("reap-digest");
        let mut lines = successor_lines(&journal);
        let before = parse(&lines[1]).unwrap();
        assert_eq!(text(&before, "event"), "reap");
        let revision = text(&before, "revision");
        let parent = text(&before, "parent");
        let marker = "\"job\":\"job-a\"";
        let at = lines[1].find(marker).expect("reap payload job");
        let mut bytes = lines[1].clone().into_bytes();
        let index = at + marker.len() - 2;
        assert_eq!(bytes[index], b'a');
        bytes[index] = b'b';
        lines[1] = String::from_utf8(bytes).unwrap();
        let after = parse(&lines[1]).unwrap();
        assert_eq!(text(&after, "revision"), revision);
        assert_eq!(text(&after, "parent"), parent);
        assert_eq!(parent, text(&parse(&lines[0]).unwrap(), "revision"));
        assert_ne!(revision, sha256(without_revision(&after).unwrap().string().as_bytes()));
        assert_ne!(text(&after, "job"), "job-a");
        store_successor_lines(&journal, &lines);
        assert_eq!(
            durable_successor_rows(&journal).unwrap_err(),
            "successor journal row is not content-addressed"
        );
    }

    #[test]
    fn durable_successor_rows_refuse_a_rewritten_parent_on_the_link() {
        let _isolate = isolate();
        let (_scratch, journal) = three_linked_successor_rows("reap-parent");
        let mut lines = successor_lines(&journal);
        let previous = text(&parse(&lines[0]).unwrap(), "revision");
        let mut row = parse(&lines[1]).unwrap();
        assert_eq!(text(&row, "event"), "reap");
        assert_eq!(text(&row, "parent"), previous);
        let wrong = sha256(b"wrong-parent");
        assert_ne!(wrong, previous);
        put(&mut row, "parent", s(&wrong));
        let revision = sha256(without_revision(&row).unwrap().string().as_bytes());
        put(&mut row, "revision", s(&revision));
        assert_eq!(text(&row, "revision"), sha256(without_revision(&row).unwrap().string().as_bytes()));
        lines[1] = row.string();
        store_successor_lines(&journal, &lines);
        assert_eq!(
            durable_successor_rows(&journal).unwrap_err(),
            "successor journal parent does not match the previous row"
        );
    }

    #[test]
    fn durable_successor_rows_refuse_a_dropped_middle_row() {
        let _isolate = isolate();
        let (_scratch, journal) = three_linked_successor_rows("reap-chain");
        let mut lines = successor_lines(&journal);
        assert_eq!(lines.len(), 3);
        let removed = parse(&lines.remove(1)).unwrap();
        let first = parse(&lines[0]).unwrap();
        let last = parse(&lines[1]).unwrap();
        assert_eq!(text(&removed, "event"), "reap");
        assert_eq!(text(&last, "parent"), text(&removed, "revision"));
        assert_ne!(text(&last, "parent"), text(&first, "revision"));
        assert_eq!(text(&first, "revision"), sha256(without_revision(&first).unwrap().string().as_bytes()));
        assert_eq!(text(&last, "revision"), sha256(without_revision(&last).unwrap().string().as_bytes()));
        store_successor_lines(&journal, &lines);
        assert_eq!(
            durable_successor_rows(&journal).unwrap_err(),
            "successor journal parent does not match the previous row"
        );
    }

    fn two_successor_rows(name: &str) -> (Scratch, PathBuf) {
        let scratch = Scratch::new(name);
        let journal = scratch.path().join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        append_spawn_row(&witness, 11, "job-a", &sha256(b"source-a"), "spawn").unwrap();
        append_reap_row(&witness, 11, "job-a", Some(0), None).unwrap();
        (scratch, journal)
    }
    fn handoff_at(root: &Path, confirmed: bool) -> Result<Json> {
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"successor-image");
        handoff(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("journal", s("self/journal.jsonl")),
            ("review_confirmed", Bool(confirmed)),
        ]))
    }
    fn viewed_state(seen: &Json, pid: u64) -> Json {
        let want = n(pid);
        get(seen, "successors")
            .array()
            .unwrap()
            .iter()
            .find(|row| get(row, "pid") == &want)
            .unwrap_or_else(|| panic!("handoff omitted successor {pid}"))
            .clone()
    }

    #[test]
    fn handoff_reports_the_tip_of_a_valid_two_row_successor_chain() {
        let _isolate = isolate();
        let (scratch, journal) = two_successor_rows("handoff-tip");
        successors().clear();
        let file = durable_successor_rows(&journal).unwrap();
        assert_eq!(file.len(), 2);
        addressed(&file[0]);
        addressed(&file[1]);
        assert_eq!(text(&file[0], "event"), "spawn");
        assert_eq!(text(&file[1], "event"), "reap");
        assert_eq!(text(&file[1], "parent"), text(&file[0], "revision"));
        let seen = handoff_at(scratch.path(), false).unwrap();
        assert_eq!(get(&seen, "successor_rows"), &n(2));
        assert_eq!(text(&seen, "successor_tip"), text(&file[1], "revision"));
        assert_ne!(text(&seen, "successor_tip"), text(&file[0], "revision"));
        assert_eq!(text(&seen, "binary_digest"), sha256(b"successor-image"));
        assert_eq!(get(&seen, "executed"), &Bool(false));
        assert!(get(&seen, "successors").array().unwrap().is_empty());
        let Obj(fields) = &seen else { panic!("handoff result is not an object") };
        let keys: Vec<_> = fields.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "argv",
                "binary_digest",
                "executed",
                "log",
                "pid",
                "source_digest",
                "successor_rows",
                "successor_tip",
                "successors",
            ]
        );
    }

    #[test]
    fn handoff_refuses_a_successor_row_whose_revision_is_not_the_body_digest() {
        let _isolate = isolate();
        let (scratch, journal) = two_successor_rows("handoff-digest");
        let mut lines = successor_lines(&journal);
        assert_eq!(lines.len(), 2);
        let before = parse(&lines[1]).unwrap();
        assert_eq!(text(&before, "event"), "reap");
        let revision = text(&before, "revision");
        let marker = "\"job\":\"job-a\"";
        let at = lines[1].find(marker).expect("reap payload job");
        let mut bytes = lines[1].clone().into_bytes();
        let index = at + marker.len() - 2;
        assert_eq!(bytes[index], b'a');
        bytes[index] = b'b';
        lines[1] = String::from_utf8(bytes).unwrap();
        let after = parse(&lines[1]).unwrap();
        assert_eq!(text(&after, "revision"), revision);
        assert_ne!(revision, sha256(without_revision(&after).unwrap().string().as_bytes()));
        store_successor_lines(&journal, &lines);
        let root = scratch.path();
        assert_eq!(
            handoff_at(root, false).unwrap_err(),
            "successor journal row is not content-addressed"
        );
        assert_eq!(
            handoff_at(root, true).unwrap_err(),
            "successor journal row is not content-addressed"
        );
    }

    #[test]
    fn handoff_refuses_a_successor_parent_that_misses_the_previous_row() {
        let _isolate = isolate();
        let (scratch, journal) = two_successor_rows("handoff-parent");
        let mut lines = successor_lines(&journal);
        let previous = text(&parse(&lines[0]).unwrap(), "revision");
        let mut row = parse(&lines[1]).unwrap();
        assert_eq!(text(&row, "event"), "reap");
        assert_eq!(text(&row, "parent"), previous);
        let wrong = sha256(b"wrong-parent");
        assert_ne!(wrong, previous);
        put(&mut row, "parent", s(&wrong));
        let revision = sha256(without_revision(&row).unwrap().string().as_bytes());
        put(&mut row, "revision", s(&revision));
        assert_eq!(text(&row, "revision"), sha256(without_revision(&row).unwrap().string().as_bytes()));
        lines[1] = row.string();
        store_successor_lines(&journal, &lines);
        assert_eq!(
            handoff_at(scratch.path(), false).unwrap_err(),
            "successor journal parent does not match the previous row"
        );
    }

    #[test]
    fn handoff_reads_a_missing_successor_journal_as_an_empty_witness() {
        let _isolate = isolate();
        let scratch = Scratch::new("handoff-missing");
        let root = scratch.path();
        let companion = successor_journal_path(&root.join("self/journal.jsonl"));
        assert!(!companion.exists());
        let seen = handoff_at(root, false).unwrap();
        assert_eq!(get(&seen, "successor_rows"), &n(0));
        assert_eq!(text(&seen, "successor_tip"), "");
        assert!(get(&seen, "successors").array().unwrap().is_empty());
        assert_eq!(get(&seen, "executed"), &Bool(false));
        assert!(!companion.exists(), "reading a missing successor journal created a file");
        assert!(
            !successor_mark_path(&root.join("self/journal.jsonl")).exists(),
            "reading a missing successor journal created a mark"
        );
    }

    #[test]
    fn handoff_reads_an_empty_journal_file_with_no_successor_journal_as_an_empty_witness() {
        let _isolate = isolate();
        let scratch = Scratch::new("handoff-empty-journal");
        let root = scratch.path();
        let journal = root.join("self/journal.jsonl");
        let companion = successor_journal_path(&journal);
        write(&journal, b"\n");
        assert!(!companion.exists());
        let seen = handoff_at(root, false).unwrap();
        assert_eq!(get(&seen, "successor_rows"), &n(0));
        assert_eq!(text(&seen, "successor_tip"), "");
        assert_eq!(get(&seen, "executed"), &Bool(false));
        assert!(!companion.exists(), "reading an empty journal created a successor journal");
        assert!(!successor_mark_path(&journal).exists(), "reading an empty journal created a mark");
        assert_eq!(fs::read(&journal).unwrap(), b"\n");
    }

    #[test]
    fn handoff_refuses_a_vanished_successor_journal_beside_a_journal_with_rows() {
        let _isolate = isolate();
        let scratch = Scratch::new("handoff-vanished");
        let root = scratch.path();
        let journal = root.join("self/journal.jsonl");
        let companion = successor_journal_path(&journal);
        let journal_bytes = b"{\"schema\":\"1\"}\n";
        write(&journal, journal_bytes);
        write(&companion, b"{\"schema\":\"1\",\"event\":\"spawn\"}\n");
        let pid = track_with(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("exit 0")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "vanished-job",
            "caller",
            "spawn",
            Some(companion.clone()),
        );
        wait_dead(pid);
        fs::remove_file(&companion).unwrap();
        assert!(!companion.exists());
        assert!(
            successors().values().any(|tracked| tracked.journal.as_deref() == Some(companion.as_path())),
            "the refusal fixture has no tracked companion"
        );
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        let proof = root.join("not-executed");
        executable(
            &root.join("bin"),
            format!("#!/bin/sh\nprintf x > '{}'\n", proof.display()).as_bytes(),
        );
        let missing = "successor journal is missing beside a journal that has rows; the companion was not created";
        let refused = |confirmed: bool| {
            match handoff(&obj(vec![
                ("root", s(root.to_str().unwrap())),
                ("binary", s(root.join("bin").to_str().unwrap())),
                ("journal", s("self/journal.jsonl")),
                ("review_confirmed", Bool(confirmed)),
            ])) {
                Err(error) => error,
                Ok(_) => String::new(),
            }
        };
        assert_eq!(refused(false), missing);
        assert_eq!(refused(true), missing);
        assert!(!companion.exists(), "a vanished successor journal was recreated");
        assert!(!proof.exists(), "a vanished successor journal still exec'd the successor");
        assert_eq!(fs::read(&journal).unwrap(), journal_bytes);
    }

    fn handoff_exec_with_unrelated_journal_rows() {
        let root = PathBuf::from(std::env::var("RLLM_EXEC_ROOT").unwrap());
        let proof = std::env::var("RLLM_EXEC_PROOF").unwrap();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        let script = format!("#!/bin/sh\nprintf '%s\\n' \"$1\" > '{proof}'\n");
        executable(&root.join("bin"), script.as_bytes());
        let journal = root.join("self/journal.jsonl");
        write(&journal, b"{\"schema\":\"1\",\"note\":\"unrelated\"}\n");
        assert!(!successor_journal_path(&journal).exists());
        assert!(successors().is_empty(), "a fresh process tracked a successor");
        let result = handoff(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("args", Arr(vec![s("continue")])),
            ("journal", s("self/journal.jsonl")),
            ("review_confirmed", Bool(true)),
        ]));
        panic!("handoff exec returned {result:?}");
    }

    #[test]
    fn handoff_review_confirmed_execs_when_unrelated_journal_rows_have_no_tracked_successor() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("handoff-exec-unrelated") {
            handoff_exec_with_unrelated_journal_rows();
            return;
        }
        let scratch = Scratch::new("handoff-unrelated");
        let root = scratch.path();
        let journal = root.join("self/journal.jsonl");
        let companion = successor_journal_path(&journal);
        write(&journal, b"{\"schema\":\"1\",\"note\":\"unrelated\"}\n");
        assert!(!companion.exists());
        assert!(successors().is_empty());
        let seen = handoff_at(root, false).unwrap();
        assert_eq!(get(&seen, "successor_rows"), &n(0));
        assert_eq!(text(&seen, "successor_tip"), "");
        assert_eq!(get(&seen, "executed"), &Bool(false));
        assert!(get(&seen, "successors").array().unwrap().is_empty());
        assert!(!companion.exists(), "reading unrelated journal rows created a successor journal");
        assert_eq!(fs::read(&journal).unwrap(), b"{\"schema\":\"1\",\"note\":\"unrelated\"}\n");
        let proof = root.join("unrelated-proof");
        let output = test_child(
            &[
                ("RLLM_SELF_BUILD_PROBE", "handoff-exec-unrelated"),
                ("RLLM_EXEC_ROOT", root.to_str().unwrap()),
                ("RLLM_EXEC_PROOF", proof.to_str().unwrap()),
            ],
            "self_build::checks::handoff_review_confirmed_execs_when_unrelated_journal_rows_have_no_tracked_successor",
        );
        let body = fs::read_to_string(&proof).unwrap_or_else(|error| panic!("{error}\n{}", child_text(&output)));
        assert_eq!(body, "continue\n", "{}", child_text(&output));
        assert!(output.status.success(), "{}", child_text(&output));
        assert!(!companion.exists(), "an untracked journal created a successor journal");
    }

    #[test]
    fn handoff_successor_view_separates_a_running_spawn_from_a_reaped_exit() {
        let _isolate = isolate();
        let scratch = Scratch::new("handoff-state");
        let root = scratch.path();
        let hold = root.join("hold");
        let pid = track(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("while [ ! -f \"$1\" ]; do sleep 0.05; done; exit 4")
                .arg("sh")
                .arg(&hold)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "held-job",
            "caller",
            "spawn",
        );
        let seen = handoff_at(root, false).unwrap();
        let running = viewed_state(&seen, pid);
        assert_eq!(get(&running, "running"), &Bool(true));
        assert_eq!(get(&running, "exit"), &Null);
        assert_eq!(text(&running, "job"), "held-job");
        assert_eq!(text(&running, "job_source"), "caller");
        assert_eq!(get(&seen, "successor_rows"), &n(0));
        write(&hold, b"go");
        wait_dead(pid);
        assert_eq!(reap().unwrap(), 1);
        let seen = handoff_at(root, false).unwrap();
        let finished = viewed_state(&seen, pid);
        assert_eq!(get(&finished, "running"), &Bool(false));
        assert_eq!(get(&finished, "exit"), &n(4));
        assert_eq!(text(&finished, "job"), "held-job");
        assert_eq!(text(&finished, "job_source"), "caller");
        assert_eq!(get(&seen, "successor_rows"), &n(0));
    }

    #[test]
    fn handoff_reports_a_killed_successor_as_not_running_before_reap() {
        let _isolate = isolate();
        let scratch = Scratch::new("handoff-killed");
        let root = scratch.path();
        let journal = root.join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        assert!(!witness.exists());
        let pid = track_with(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("trap 'exit 9' TERM; while :; do sleep 0.05; done")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "killed-job",
            "caller",
            "spawn",
            Some(witness.clone()),
        );
        assert!(
            Command::new("/bin/kill").arg("-TERM").arg(pid.to_string()).status().unwrap().success(),
            "kill did not signal the successor"
        );
        wait_dead(pid);
        let seen = handoff_at(root, false).unwrap();
        let state = viewed_state(&seen, pid);
        assert_eq!(get(&state, "running"), &Bool(false));
        assert_eq!(get(&state, "exit"), &n(9));
        assert_eq!(text(&state, "job"), "killed-job");
        assert_eq!(text(&state, "job_source"), "caller");
        assert_eq!(get(&seen, "successor_rows"), &n(0));
        assert_eq!(text(&seen, "successor_tip"), "");
        assert!(!witness.exists(), "reading the handoff appended a successor journal row");
        assert_eq!(reap().unwrap(), 1);
        let rows = durable_successor_rows(&journal).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(text(&rows[0], "event"), "reap");
        assert_eq!(get(&rows[0], "exit"), &n(9));
        assert_eq!(get(&rows[0], "signal"), &Null);
        assert_eq!(text(&rows[0], "job"), "killed-job");
        let finished = tracked_state(pid);
        assert_eq!(get(&finished, "running"), &Bool(false));
        assert_eq!(get(&finished, "exit"), &n(9));
        let again = handoff_at(root, false).unwrap();
        let reread = viewed_state(&again, pid);
        assert_eq!(get(&reread, "running"), &Bool(false));
        assert_eq!(get(&reread, "exit"), &n(9));
        assert_eq!(text(&reread, "job"), "killed-job");
    }

    #[test]
    fn reap_uses_the_exit_stored_by_the_view() {
        let _isolate = isolate();
        let scratch = Scratch::new("stored-exit");
        let root = scratch.path();
        let journal = root.join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        let pid = track_with(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("exit 9")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "stored-job",
            "caller",
            "spawn",
            Some(witness.clone()),
        );
        wait_dead(pid);
        let state = tracked_state(pid);
        assert_eq!(get(&state, "running"), &Bool(false));
        assert_eq!(get(&state, "exit"), &n(9));
        assert!(!witness.exists(), "the view appended a successor journal row");
        let replacement = Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 4")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let replacement_pid = u64::from(replacement.id());
        wait_dead(replacement_pid);
        {
            let mut table = successors();
            let tracked = table.get_mut(&pid).unwrap();
            assert!(tracked.exit_observed, "the view did not store the exit");
            assert_eq!(tracked.exit, Some(9));
            assert_eq!(tracked.signal, None);
            tracked.child = Some(replacement);
        }
        assert_eq!(reap().unwrap(), 1);
        assert_eq!(reap().unwrap(), 0);
        let rows = durable_successor_rows(&journal).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(text(&rows[0], "event"), "reap");
        assert_eq!(get(&rows[0], "exit"), &n(9));
        assert_eq!(get(&rows[0], "signal"), &Null);
        assert_eq!(text(&rows[0], "job"), "stored-job");
        assert_eq!(get(&rows[0], "pid"), &n(pid));
        let finished = tracked_state(pid);
        assert_eq!(get(&finished, "running"), &Bool(false));
        assert_eq!(get(&finished, "exit"), &n(9));
    }

    #[test]
    fn handoff_view_names_a_signal_death_while_exit_stays_null() {
        let _isolate = isolate();
        let scratch = Scratch::new("signal-view");
        let root = scratch.path();
        let journal = root.join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        let pid = track_with(
            Command::new("/bin/sleep")
                .arg("30")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "signaled-job",
            "caller",
            "spawn",
            Some(witness.clone()),
        );
        assert!(
            Command::new("/bin/kill").arg("-KILL").arg(pid.to_string()).status().unwrap().success(),
            "kill did not signal the successor"
        );
        wait_dead(pid);
        let seen = handoff_at(root, false).unwrap();
        let state = viewed_state(&seen, pid);
        assert_eq!(get(&state, "running"), &Bool(false));
        assert_eq!(get(&state, "exit"), &Null, "signal death rendered {}", state.string());
        assert_eq!(
            get(&state, "signal"),
            &n(9),
            "signal death rendered {}",
            state.string()
        );
        assert!(
            state.string().contains("\"exit\":null") && state.string().contains("\"signal\":9"),
            "signal death rendered {}",
            state.string()
        );
        assert_eq!(get(&seen, "successor_rows"), &n(0));
        assert_eq!(text(&seen, "successor_tip"), "");
        assert!(!witness.exists(), "reading the handoff appended a successor journal row");
        assert!(
            !successor_mark_path(&journal).exists(),
            "reading the handoff wrote a successor mark"
        );
        let stored_signal = {
            let table = successors();
            let tracked = table.get(&pid).unwrap();
            assert!(tracked.exit_observed, "the view did not store the death");
            assert_eq!(tracked.exit, None);
            assert_eq!(tracked.signal, Some(9));
            tracked.signal
        };
        assert_eq!(reap().unwrap(), 1);
        assert_eq!(reap().unwrap(), 0);
        let rows = durable_successor_rows(&journal).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(text(&rows[0], "event"), "reap");
        assert_eq!(get(&rows[0], "exit"), &Null);
        assert_eq!(get(&rows[0], "signal"), &Num(stored_signal.unwrap().to_string()));
        assert_eq!(text(&rows[0], "job"), "signaled-job");
        assert_eq!(get(&rows[0], "pid"), &n(pid));
        let Obj(fields) = &rows[0] else { panic!("reap row is not an object") };
        let keys: Vec<_> = fields.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["event", "exit", "job", "parent", "pid", "revision", "schema", "signal"]
        );
        let line = rows[0].string();
        let exit_at = line.find("\"exit\":null").expect("reap row exit");
        let signal_at = line.find("\"signal\":9").expect("reap row signal");
        assert!(exit_at < signal_at, "{line}");
        assert!(!line.contains("exit_observed"), "{line}");
        let again = handoff_at(root, false).unwrap();
        let reread = viewed_state(&again, pid);
        assert_eq!(get(&reread, "running"), &Bool(false));
        assert_eq!(get(&reread, "exit"), &Null);
        assert_eq!(get(&reread, "signal"), &n(9));
        assert_eq!(durable_successor_rows(&journal).unwrap().len(), 1);
    }

    #[test]
    fn successor_view_distinguishes_a_normal_exit_a_signal_death_and_an_unobserved_death() {
        let _isolate = isolate();
        let scratch = Scratch::new("signal-three");
        let root = scratch.path();
        let normal = track(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("exit 4")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "normal-job",
            "caller",
            "spawn",
        );
        let signaled = track(
            Command::new("/bin/sleep")
                .arg("30")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "signal-job",
            "caller",
            "spawn",
        );
        let pending = track(
            Command::new("/bin/sleep")
                .arg("30")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            "unobserved-job",
            "caller",
            "spawn",
        );
        struct StopSleep(u64);
        impl Drop for StopSleep {
            fn drop(&mut self) {
                let _ = Command::new("/bin/kill").arg("-KILL").arg(self.0.to_string()).status();
            }
        }
        // Only the still-running child. The signaled pid is reaped by the view,
        // and killing it again could hit a reused pid.
        let _stop_pending = StopSleep(pending);
        wait_dead(normal);
        assert!(
            Command::new("/bin/kill").arg("-KILL").arg(signaled.to_string()).status().unwrap().success(),
            "kill did not signal the successor"
        );
        wait_dead(signaled);
        let seen = handoff_at(root, false).unwrap();
        let normal_view = viewed_state(&seen, normal);
        let signal_view = viewed_state(&seen, signaled);
        let pending_view = viewed_state(&seen, pending);
        assert_eq!(get(&normal_view, "running"), &Bool(false));
        assert_eq!(get(&normal_view, "exit"), &n(4));
        assert_eq!(get(&normal_view, "signal"), &Null);
        assert_eq!(get(&signal_view, "running"), &Bool(false));
        assert_eq!(get(&signal_view, "exit"), &Null);
        assert_eq!(get(&signal_view, "signal"), &n(9));
        assert_eq!(get(&pending_view, "running"), &Bool(true));
        assert_eq!(get(&pending_view, "exit"), &Null);
        assert_eq!(get(&pending_view, "signal"), &Null);
        {
            let table = successors();
            let normal_tracked = table.get(&normal).unwrap();
            let signal_tracked = table.get(&signaled).unwrap();
            let pending_tracked = table.get(&pending).unwrap();
            assert!(normal_tracked.exit_observed);
            assert_eq!(normal_tracked.exit, Some(4));
            assert_eq!(normal_tracked.signal, None);
            assert!(signal_tracked.exit_observed);
            assert_eq!(signal_tracked.exit, None);
            assert_eq!(signal_tracked.signal, Some(9));
            assert!(!pending_tracked.exit_observed);
            assert_eq!(pending_tracked.exit, None);
            assert_eq!(pending_tracked.signal, None);
        }
        let normal_text = normal_view.string();
        let signal_text = signal_view.string();
        let pending_text = pending_view.string();
        assert_ne!(normal_text, signal_text, "normal and signal views match\n{normal_text}");
        assert_ne!(signal_text, pending_text, "signal and unobserved views match\n{signal_text}");
        assert_ne!(normal_text, pending_text, "normal and unobserved views match\n{pending_text}");
        assert!(normal_text.contains("\"exit\":4") && normal_text.contains("\"signal\":null"), "{normal_text}");
        assert!(signal_text.contains("\"exit\":null") && signal_text.contains("\"signal\":9"), "{signal_text}");
        assert!(
            pending_text.contains("\"exit\":null") && pending_text.contains("\"signal\":null") && pending_text.contains("\"running\":true"),
            "{pending_text}"
        );
    }

    fn handoff_exec_reaches_the_successor() {
        let root = PathBuf::from(std::env::var("RLLM_EXEC_ROOT").unwrap());
        let proof = std::env::var("RLLM_EXEC_PROOF").unwrap();
        let tampered = std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("handoff-exec-tampered");
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        let script = format!("#!/bin/sh\nprintf '%s\\n' \"$1\" > '{proof}'\n");
        executable(&root.join("bin"), script.as_bytes());
        let journal = root.join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        append_spawn_row(&witness, 11, "job-a", &sha256(b"source-a"), "spawn").unwrap();
        append_reap_row(&witness, 11, "job-a", Some(0), None).unwrap();
        if tampered {
            let mut lines = successor_lines(&journal);
            let before = parse(&lines[1]).unwrap();
            let revision = text(&before, "revision");
            let marker = "\"job\":\"job-a\"";
            let at = lines[1].find(marker).expect("reap payload job");
            let mut bytes = lines[1].clone().into_bytes();
            let index = at + marker.len() - 2;
            assert_eq!(bytes[index], b'a');
            bytes[index] = b'b';
            lines[1] = String::from_utf8(bytes).unwrap();
            let after = parse(&lines[1]).unwrap();
            assert_eq!(text(&after, "revision"), revision);
            assert_ne!(revision, sha256(without_revision(&after).unwrap().string().as_bytes()));
            store_successor_lines(&journal, &lines);
        }
        let result = handoff(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("args", Arr(vec![s("continue")])),
            ("journal", s("self/journal.jsonl")),
            ("review_confirmed", Bool(true)),
        ]));
        if tampered {
            match result {
                Err(error) if error == "successor journal row is not content-addressed" => {
                    panic!("refused before exec: {error}");
                }
                other => panic!("handoff exec returned {other:?}"),
            }
        }
        panic!("handoff exec returned {result:?}");
    }
    fn assert_confirmed_exec_refuses_a_tampered_journal(name: &str, exact: &str) {
        let scratch = Scratch::new(name);
        let proof = scratch.path().join("tampered-proof");
        let output = test_child(
            &[
                ("RLLM_SELF_BUILD_PROBE", "handoff-exec-tampered"),
                ("RLLM_EXEC_ROOT", scratch.path().to_str().unwrap()),
                ("RLLM_EXEC_PROOF", proof.to_str().unwrap()),
            ],
            exact,
        );
        let detail = child_text(&output);
        assert!(!proof.exists(), "tampered chain still exec'd the successor\n{detail}");
        assert!(detail.contains("successor journal row is not content-addressed"), "{detail}");
        assert!(!detail.contains("successor exec failed"), "{detail}");
        assert!(!output.status.success(), "{detail}");
    }

    #[test]
    fn handoff_review_confirmed_execs_the_binary_after_a_valid_chain() {
        let _isolate = isolate();
        match std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() {
            Some("handoff-exec") | Some("handoff-exec-tampered") => {
                handoff_exec_reaches_the_successor();
                return;
            }
            _ => {}
        }
        let scratch = Scratch::new("handoff-exec");
        let proof = scratch.path().join("valid-proof");
        let output = test_child(
            &[
                ("RLLM_SELF_BUILD_PROBE", "handoff-exec"),
                ("RLLM_EXEC_ROOT", scratch.path().to_str().unwrap()),
                ("RLLM_EXEC_PROOF", proof.to_str().unwrap()),
            ],
            "self_build::checks::handoff_review_confirmed_execs_the_binary_after_a_valid_chain",
        );
        let body = fs::read_to_string(&proof).unwrap_or_else(|error| panic!("{error}\n{}", child_text(&output)));
        assert_eq!(body, "continue\n", "{}", child_text(&output));
        assert!(output.status.success(), "{}", child_text(&output));
        assert_confirmed_exec_refuses_a_tampered_journal(
            "handoff-exec-tampered",
            "self_build::checks::handoff_review_confirmed_execs_the_binary_after_a_valid_chain",
        );
    }

    #[test]
    fn handoff_review_confirmed_refuses_exec_on_a_tampered_successor_journal() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("handoff-exec-tampered") {
            handoff_exec_reaches_the_successor();
            return;
        }
        assert_confirmed_exec_refuses_a_tampered_journal(
            "handoff-exec-refuse",
            "self_build::checks::handoff_review_confirmed_refuses_exec_on_a_tampered_successor_journal",
        );
    }

    fn assert_handoff_refuses(root: &Path, sentence: &str) {
        assert_eq!(handoff_at(root, false).unwrap_err(), sentence);
        assert_eq!(handoff_at(root, true).unwrap_err(), sentence);
    }
    fn side_chain(name: &str, count: u64) -> Vec<String> {
        let scratch = Scratch::new(name);
        let journal = scratch.path().join("side.jsonl");
        let witness = successor_journal_path(&journal);
        for index in 0..count {
            append_spawn_row(
                &witness,
                1000 + index,
                &format!("other-{index}"),
                &sha256(format!("source-{index}").as_bytes()),
                "spawn",
            )
            .unwrap();
        }
        let lines = successor_lines(&journal);
        assert_eq!(lines.len(), count as usize);
        lines
    }
    fn mark_lines(journal: &Path) -> Vec<String> {
        fs::read_to_string(successor_mark_path(journal))
            .unwrap()
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }
    fn store_mark_lines(journal: &Path, lines: &[String]) {
        let mut text = lines.join("\n");
        text.push('\n');
        fs::write(successor_mark_path(journal), text).unwrap();
    }
    fn fork_companion(journal: &Path) {
        let lines = successor_lines(journal);
        assert!(lines.len() >= 3, "a rollback fork needs a recorded tail to abandon");
        let saved = mark_lines(journal);
        let recorded = recorded_successor_revisions(journal).unwrap();
        assert_eq!(recorded.len(), lines.len(), "the mark does not record the whole companion");
        let scratch = Scratch::new("fork-side");
        let side_journal = scratch.path().join("side.jsonl");
        let side_witness = successor_journal_path(&side_journal);
        let mut prefix = lines[..2].join("\n");
        prefix.push('\n');
        fs::create_dir_all(side_witness.parent().unwrap()).unwrap();
        fs::write(&side_witness, prefix).unwrap();
        // The anchor is one row, so the side chain gets a sealed prefix mark
        // instead of a copied history of earlier anchors.
        append_mark(&successor_mark_path(&side_journal), &recorded[..2]).unwrap();
        append_spawn_row(&side_witness, 99, "rolled", &sha256(b"rolled-source"), "spawn").unwrap();
        let rolled = successor_lines(&side_journal);
        assert_eq!(rolled.len(), 3);
        let new_row = parse(rolled.last().unwrap()).unwrap();
        assert_eq!(text(&new_row, "parent"), text(&parse(&lines[1]).unwrap(), "revision"));
        assert_ne!(text(&new_row, "revision"), text(&parse(&lines[2]).unwrap(), "revision"));
        store_successor_lines(journal, &rolled);
        assert_eq!(mark_lines(journal), saved, "sealing the fork row rewrote the recorded chain");
    }
    fn handoff_exec_refuses_a_rewound_companion() {
        let root = PathBuf::from(std::env::var("RLLM_EXEC_ROOT").unwrap());
        let proof = std::env::var("RLLM_EXEC_PROOF").unwrap();
        let attack = std::env::var("RLLM_SELF_BUILD_PROBE").unwrap();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        let script = format!("#!/bin/sh\nprintf '%s\\n' \"$1\" > '{proof}'\n");
        executable(&root.join("bin"), script.as_bytes());
        let journal = root.join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        append_spawn_row(&witness, 11, "job-a", &sha256(b"source-a"), "spawn").unwrap();
        append_reap_row(&witness, 11, "job-a", Some(0), None).unwrap();
        append_spawn_row(&witness, 12, "job-b", &sha256(b"source-b"), "spawn").unwrap();
        let sentence = match attack.as_str() {
            "handoff-exec-truncated" => {
                let lines = successor_lines(&journal);
                store_successor_lines(&journal, &lines[..2]);
                "successor journal was truncated below the recorded chain"
            }
            "handoff-exec-replaced" => {
                let replacement = side_chain("exec-replace-side", 2);
                assert_ne!(replacement, successor_lines(&journal));
                store_successor_lines(&journal, &replacement);
                "successor journal was replaced with a different chain"
            }
            "handoff-exec-deleted" => {
                fs::remove_file(&witness).unwrap();
                assert!(successors().is_empty(), "a fresh process tracked a successor");
                "successor journal is missing after a recorded chain"
            }
            "handoff-exec-rolled" => {
                fork_companion(&journal);
                "successor journal rolled back onto a revision already observed"
            }
            other => panic!("unknown successor probe {other}"),
        };
        let result = handoff(&obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("args", Arr(vec![s("continue")])),
            ("journal", s("self/journal.jsonl")),
            ("review_confirmed", Bool(true)),
        ]));
        match result {
            Err(error) if error == sentence => panic!("refused before exec: {error}"),
            other => panic!("handoff exec returned {other:?}"),
        }
    }
    fn assert_rewound_exec_refuses(name: &str, probe: &str, sentence: &str, exact: &str) {
        let scratch = Scratch::new(name);
        let proof = scratch.path().join("rewind-proof");
        let output = test_child(
            &[
                ("RLLM_SELF_BUILD_PROBE", probe),
                ("RLLM_EXEC_ROOT", scratch.path().to_str().unwrap()),
                ("RLLM_EXEC_PROOF", proof.to_str().unwrap()),
            ],
            exact,
        );
        let detail = child_text(&output);
        assert!(!proof.exists(), "rewound chain still exec'd the successor\n{detail}");
        assert!(detail.contains(sentence), "{detail}");
        assert!(detail.contains("refused before exec"), "{detail}");
        assert!(!detail.contains("successor exec failed"), "{detail}");
        assert!(!output.status.success(), "{detail}");
    }

    #[test]
    fn handoff_refuses_a_truncated_successor_chain() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("handoff-exec-truncated") {
            handoff_exec_refuses_a_rewound_companion();
            return;
        }
        let sentence = "successor journal was truncated below the recorded chain";
        {
            let (scratch, journal) = three_linked_successor_rows("trunc-prefix");
            let lines = successor_lines(&journal);
            let full_tip = text(&parse(lines.last().unwrap()).unwrap(), "revision");
            let kept_tip = text(&parse(&lines[1]).unwrap(), "revision");
            assert_ne!(full_tip, kept_tip);
            store_successor_lines(&journal, &lines[..2]);
            let read = durable_successor_rows(&journal).unwrap();
            assert_eq!(read.len(), 2, "the chain reader rejected a valid prefix");
            assert_eq!(text(&read[1], "revision"), kept_tip);
            assert_handoff_refuses(scratch.path(), sentence);
            let witness = successor_journal_path(&journal);
            assert_eq!(
                append_spawn_row(&witness, 99, "nope", &sha256(b"nope"), "spawn").unwrap_err(),
                sentence
            );
            assert_eq!(successor_lines(&journal).len(), 2, "a refused append extended the truncated chain");
        }
        {
            // Rewriting to a prefix and appending a row that is already in the chain
            // leaves that same prefix. The row's revision was observed on the longer chain.
            let (scratch, journal) = three_linked_successor_rows("trunc-reappend");
            let lines = successor_lines(&journal);
            let reused = text(&parse(&lines[1]).unwrap(), "revision");
            store_successor_lines(&journal, &[lines[0].clone(), lines[1].clone()]);
            let read = durable_successor_rows(&journal).unwrap();
            assert_eq!(text(&read[1], "revision"), reused);
            assert_handoff_refuses(scratch.path(), sentence);
        }
        assert_rewound_exec_refuses(
            "trunc-exec",
            "handoff-exec-truncated",
            sentence,
            "self_build::checks::handoff_refuses_a_truncated_successor_chain",
        );
    }

    #[test]
    fn handoff_refuses_a_replaced_successor_chain() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("handoff-exec-replaced") {
            handoff_exec_refuses_a_rewound_companion();
            return;
        }
        let sentence = "successor journal was replaced with a different chain";
        let (scratch, journal) = three_linked_successor_rows("replace-chain");
        let original = successor_lines(&journal);
        let replacement = side_chain("replace-side", 2);
        assert!(replacement.len() < original.len());
        assert_ne!(replacement[0], original[0]);
        store_successor_lines(&journal, &replacement);
        let read = durable_successor_rows(&journal).unwrap();
        assert_eq!(read.len(), 2, "the chain reader rejected an internally valid replacement");
        assert_eq!(text(&read[1], "revision"), text(&parse(replacement.last().unwrap()).unwrap(), "revision"));
        assert_handoff_refuses(scratch.path(), sentence);
        let witness = successor_journal_path(&journal);
        assert_eq!(
            append_spawn_row(&witness, 99, "nope", &sha256(b"nope"), "spawn").unwrap_err(),
            sentence
        );
        assert_eq!(successor_lines(&journal), replacement, "a refused append adopted the replacement");
        assert_rewound_exec_refuses(
            "replace-exec",
            "handoff-exec-replaced",
            sentence,
            "self_build::checks::handoff_refuses_a_replaced_successor_chain",
        );
    }

    #[test]
    fn handoff_refuses_a_deleted_successor_chain_after_a_recorded_chain() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("handoff-exec-deleted") {
            handoff_exec_refuses_a_rewound_companion();
            return;
        }
        let sentence = "successor journal is missing after a recorded chain";
        let tracked = "successor journal is missing beside a journal that has rows; the companion was not created";
        {
            let (scratch, journal) = three_linked_successor_rows("delete-tracked");
            let companion = successor_journal_path(&journal);
            let journal_bytes = b"{\"schema\":\"1\",\"note\":\"primary\"}\n";
            write(&journal, journal_bytes);
            assert!(successor_mark_path(&journal).exists(), "the tracked refusal has no recorded chain");
            let pid = track_with(
                Command::new("/bin/sh")
                    .arg("-c")
                    .arg("exit 0")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
                "tracked-delete",
                "caller",
                "spawn",
                Some(companion.clone()),
            );
            wait_dead(pid);
            fs::remove_file(&companion).unwrap();
            assert_handoff_refuses(scratch.path(), tracked);
            assert!(!companion.exists(), "a tracked refusal recreated the companion");
            assert_eq!(fs::read(&journal).unwrap(), journal_bytes);
        }
        {
            let (scratch, journal) = three_linked_successor_rows("delete-restart");
            let companion = successor_journal_path(&journal);
            let mark = successor_mark_path(&journal);
            write(&journal, b"{\"schema\":\"1\",\"note\":\"primary\"}\n");
            let saved_mark = fs::read(&mark).unwrap();
            fs::remove_file(&companion).unwrap();
            successors().clear();
            assert!(successors().is_empty(), "clearing the table left a tracked companion");
            assert!(durable_successor_rows(&journal).unwrap().is_empty(), "a missing file is not an empty chain read");
            assert_handoff_refuses(scratch.path(), sentence);
            assert_eq!(
                append_spawn_row(&companion, 99, "nope", &sha256(b"nope"), "spawn").unwrap_err(),
                sentence
            );
            assert!(!companion.exists(), "a refused append recreated the deleted companion");
            assert_eq!(fs::read(&mark).unwrap(), saved_mark, "the refusal rewrote the recorded chain");
        }
        assert_rewound_exec_refuses(
            "delete-exec",
            "handoff-exec-deleted",
            sentence,
            "self_build::checks::handoff_refuses_a_deleted_successor_chain_after_a_recorded_chain",
        );
    }

    #[test]
    fn handoff_refuses_a_rolled_back_successor_chain() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("handoff-exec-rolled") {
            handoff_exec_refuses_a_rewound_companion();
            return;
        }
        let sentence = "successor journal rolled back onto a revision already observed";
        let (scratch, journal) = three_linked_successor_rows("rollback-fork");
        let recorded: Vec<String> = successor_lines(&journal)
            .iter()
            .map(|line| text(&parse(line).unwrap(), "revision"))
            .collect();
        fork_companion(&journal);
        let read = durable_successor_rows(&journal).unwrap();
        assert_eq!(read.len(), 3, "the chain reader rejected an internally valid fork");
        assert_eq!(text(&read[1], "revision"), recorded[1]);
        assert_ne!(text(&read[2], "revision"), recorded[2]);
        assert_eq!(text(&read[2], "parent"), recorded[1], "the new row does not reuse the observed parent");
        assert_handoff_refuses(scratch.path(), sentence);
        let witness = successor_journal_path(&journal);
        assert_eq!(
            append_spawn_row(&witness, 100, "nope", &sha256(b"nope"), "spawn").unwrap_err(),
            sentence
        );
        assert_eq!(durable_successor_rows(&journal).unwrap().len(), 3, "a refused append extended the fork");
        assert_rewound_exec_refuses(
            "rollback-exec",
            "handoff-exec-rolled",
            sentence,
            "self_build::checks::handoff_refuses_a_rolled_back_successor_chain",
        );
    }

    fn newest_mark(lines: &[String]) -> usize {
        assert!(!lines.is_empty(), "tamper target has no mark row");
        lines.len() - 1
    }
    #[test]
    fn handoff_refuses_a_tampered_successor_mark() {
        let _isolate = isolate();
        {
            let (scratch, journal) = three_linked_successor_rows("mark-digest");
            let mut lines = mark_lines(&journal);
            let newest = newest_mark(&lines);
            let marker = "\"event\":\"mark\"";
            let at = lines[newest].find(marker).expect("mark event");
            let mut bytes = lines[newest].clone().into_bytes();
            let index = at + marker.len() - 2;
            assert_eq!(bytes[index], b'k');
            bytes[index] = b'q';
            lines[newest] = String::from_utf8(bytes).unwrap();
            let flipped = parse(&lines[newest]).unwrap();
            assert_ne!(
                text(&flipped, "revision"),
                sha256(without_revision(&flipped).unwrap().string().as_bytes())
            );
            store_mark_lines(&journal, &lines);
            assert_handoff_refuses(scratch.path(), "successor mark row is not content-addressed");
        }
        {
            let (scratch, journal) = three_linked_successor_rows("mark-parent");
            let mut lines = mark_lines(&journal);
            let newest = newest_mark(&lines);
            let previous = if newest == 0 {
                String::new()
            } else {
                text(&parse(&lines[newest - 1]).unwrap(), "revision")
            };
            let mut row = parse(&lines[newest]).unwrap();
            assert_eq!(text(&row, "parent"), previous);
            let wrong = sha256(b"wrong-mark-parent");
            assert_ne!(wrong, previous);
            put(&mut row, "parent", s(&wrong));
            let revision = sha256(without_revision(&row).unwrap().string().as_bytes());
            put(&mut row, "revision", s(&revision));
            lines[newest] = row.string();
            store_mark_lines(&journal, &lines);
            assert_handoff_refuses(scratch.path(), "successor mark parent does not match the previous row");
        }
        {
            let (scratch, journal) = three_linked_successor_rows("mark-shape");
            let mut lines = mark_lines(&journal);
            let newest = newest_mark(&lines);
            let mut row = parse(&lines[newest]).unwrap();
            put(&mut row, "revisions", s("not-a-list"));
            let revision = sha256(without_revision(&row).unwrap().string().as_bytes());
            put(&mut row, "revision", s(&revision));
            assert_eq!(text(&row, "revision"), sha256(without_revision(&row).unwrap().string().as_bytes()));
            lines[newest] = row.string();
            store_mark_lines(&journal, &lines);
            assert_handoff_refuses(scratch.path(), "successor mark row is not a recorded chain");
        }
    }

    #[test]
    fn successor_mark_stays_bounded_past_append_174() {
        let _isolate = isolate();
        let scratch = Scratch::new("mark-soak");
        let journal = scratch.path().join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        let mark = successor_mark_path(&journal);
        const APPENDED: u64 = 200;
        for index in 0..APPENDED {
            append_spawn_row(
                &witness,
                index,
                &format!("soak-{index}"),
                &sha256(format!("soak-source-{index}").as_bytes()),
                "spawn",
            )
            .unwrap();
            let size = fs::symlink_metadata(&mark).unwrap().len();
            assert!(
                size <= JOURNAL_MAX,
                "successor mark exceeded its bound at append {} ({size} bytes)",
                index + 1
            );
        }
        let lines = mark_lines(&journal);
        assert_eq!(lines.len(), 1, "pruned successor mark is one content-addressed row");
        let row = parse(&lines[0]).unwrap();
        addressed(&row);
        assert_eq!(text(&row, "parent"), "", "pruned successor mark is not chained");
        assert_eq!(text(&row, "event"), "mark");
        let marked = mark_revisions(&row).unwrap();
        let companion = successor_lines(&journal);
        let current: Vec<String> = companion
            .iter()
            .map(|line| text(&parse(line).unwrap(), "revision"))
            .collect();
        assert_eq!(marked, current, "pruned successor mark does not record the companion");
        assert_eq!(current.len(), APPENDED as usize);
        let seen = handoff_at(scratch.path(), false).unwrap();
        assert_eq!(get(&seen, "successor_rows"), &n(APPENDED));
        assert_eq!(text(&seen, "successor_tip"), current.last().cloned().unwrap());
        let saved_mark = fs::read(&mark).unwrap();
        store_successor_lines(&journal, &companion[..companion.len() / 2]);
        assert_handoff_refuses(scratch.path(), "successor journal was truncated below the recorded chain");
        assert_eq!(fs::read(&mark).unwrap(), saved_mark, "truncation refusal rewrote the pruned mark");
        store_successor_lines(&journal, &companion);
        let replacement = side_chain("soak-replace", 2);
        assert_ne!(replacement, companion);
        store_successor_lines(&journal, &replacement);
        assert_handoff_refuses(scratch.path(), "successor journal was replaced with a different chain");
        assert_eq!(fs::read(&mark).unwrap(), saved_mark, "replacement refusal rewrote the pruned mark");
        store_successor_lines(&journal, &companion);
        fork_companion(&journal);
        assert_handoff_refuses(scratch.path(), "successor journal rolled back onto a revision already observed");
        assert_eq!(fs::read(&mark).unwrap(), saved_mark, "fork refusal rewrote the pruned mark");
    }

    #[test]
    fn directory_synced_mark_stays_one_anchor_and_handoff_reports_the_rows() {
        let _isolate = isolate();
        let scratch = Scratch::new("mark-dir-sync");
        let journal = scratch.path().join("self/journal.jsonl");
        let witness = successor_journal_path(&journal);
        append_spawn_row(&witness, 11, "job-a", &sha256(b"source-a"), "spawn").unwrap();
        append_reap_row(&witness, 11, "job-a", Some(0), None).unwrap();
        let lines = mark_lines(&journal);
        assert_eq!(lines.len(), 1, "directory sync changed the sealed anchor into more than one row");
        let row = parse(&lines[0]).unwrap();
        addressed(&row);
        assert_eq!(text(&row, "parent"), "", "sealed anchor parent is not empty");
        assert_eq!(text(&row, "event"), "mark");
        let marked = mark_revisions(&row).unwrap();
        let companion = successor_lines(&journal);
        let current: Vec<String> = companion
            .iter()
            .map(|line| text(&parse(line).unwrap(), "revision"))
            .collect();
        assert_eq!(marked, current, "sealed anchor does not record the companion");
        assert_eq!(current.len(), 2);
        let seen = handoff_at(scratch.path(), false).unwrap();
        assert_eq!(get(&seen, "successor_rows"), &n(2));
        assert_eq!(text(&seen, "successor_tip"), current.last().cloned().unwrap());
    }

    #[test]
    fn mark_directory_sync_fails_when_the_directory_cannot_be_opened() {
        let scratch = Scratch::new("mark-dir-open");
        let file = scratch.path().join("not-a-directory");
        fs::write(&file, b"regular-file").unwrap();
        assert_eq!(
            sync_mark_directory(&file).unwrap_err(),
            "Not a directory (os error 20)",
            "opening a file with O_DIRECTORY did not fail"
        );
        let missing = scratch.path().join("missing-directory");
        assert_eq!(
            sync_mark_directory(&missing).unwrap_err(),
            "No such file or directory (os error 2)"
        );
        let real = scratch.path().join("real-dir");
        fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, scratch.path().join("linked-dir")).unwrap();
        assert_eq!(
            sync_mark_directory(&scratch.path().join("linked-dir")).unwrap_err(),
            "Not a directory (os error 20)",
            "O_NOFOLLOW followed a symlink directory"
        );
        assert!(sync_mark_directory(&real).is_ok(), "a real directory did not sync");
    }

    fn git_room(root: &Path, args: &[&str]) {
        let mut full = vec![
            "-c",
            "safe.directory=*",
            "-c",
            "user.name=RLLM",
            "-c",
            "user.email=rllm@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ];
        let hooks = root.join(".git/rllm-empty-hooks");
        if hooks.is_dir() {
            full.push("-c");
            full.push("core.hooksPath=.git/rllm-empty-hooks");
        }
        full.extend(args);
        let output = Command::new("git")
            .args(&full)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_NAMESPACE")
            .output()
            .unwrap_or_else(|error| panic!("git {} failed to start: {error}", args.join(" ")));
        assert!(
            output.status.success(),
            "git {} exited {:?}\n{}",
            args.join(" "),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn prepare_committed(root: &Path) -> (String, String) {
        write(&root.join("src/lib.rs"), b"fn marker() {}\n");
        write(&root.join("notes/base.txt"), b"base\n");
        executable(&root.join("bin"), b"#!/bin/sh\nprintf '%s\\n' \"$1\" > \"$2\"\n");
        let root_text = root.to_str().unwrap();
        let binary = root.join("bin").to_str().unwrap().to_string();
        let recorded = journal(&obj(vec![
            ("root", s(root_text)),
            ("binary", s(&binary)),
            ("previous_binary", s(&binary)),
            ("journal", s("self/journal.jsonl")),
            ("rollback", s("self/rollback")),
            ("parent", s("")),
            ("gate_output", s("Rust cases passed: 1/1; isolated application cases: 1")),
            ("review_confirmed", Bool(true)),
        ]))
        .unwrap_or_else(|error| panic!("journal: {error}"));
        assert_eq!(get(&recorded, "recorded"), &Bool(true));
        let revision = text(&recorded, "revision");
        git_room(root, &["init", "-b", "main"]);
        fs::create_dir_all(root.join(".git/rllm-empty-hooks")).unwrap();
        git_room(root, &["add", "--", "notes/base.txt"]);
        git_room(root, &["commit", "-m", "base", "--", "notes/base.txt"]);
        git_room(root, &["add", "--", "src/lib.rs"]);
        let committed = commit(&obj(vec![
            ("root", s(root_text)),
            ("binary", s(&binary)),
            ("revision", s(&revision)),
            ("journal", s("self/journal.jsonl")),
            ("parent", s("")),
            ("review_confirmed", Bool(true)),
        ]))
        .unwrap_or_else(|error| panic!("commit: {error}"));
        let digest = text(&committed, "commit");
        assert!(hex40(&digest), "{digest}");
        assert_eq!(text(&committed, "revision"), revision);
        (revision, digest)
    }
    fn resume_request(root: &Path, revision: &str, confirmed: bool) -> Json {
        obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("binary", s(root.join("bin").to_str().unwrap())),
            ("revision", s(revision)),
            ("journal", s("self/journal.jsonl")),
            ("parent", s("")),
            ("review_confirmed", Bool(confirmed)),
        ])
    }
    fn link_closed_successor(root: &Path) {
        let witness = successor_journal_path(&root.join("self/journal.jsonl"));
        append_spawn_row(&witness, 11, "job-a", &sha256(b"source-a"), "spawn").unwrap();
        append_reap_row(&witness, 11, "job-a", Some(0), None).unwrap();
    }
    fn link_three_successors(root: &Path) {
        link_closed_successor(root);
        let witness = successor_journal_path(&root.join("self/journal.jsonl"));
        append_spawn_row(&witness, 12, "job-b", &sha256(b"source-b"), "spawn").unwrap();
    }
    fn flip_successor_job(journal: &Path) {
        let mut lines = successor_lines(journal);
        assert!(lines.len() >= 2, "tamper needs a successor row");
        let before = parse(&lines[1]).unwrap();
        let revision = text(&before, "revision");
        let marker = "\"job\":\"job-a\"";
        let at = lines[1].find(marker).expect("reap payload job");
        let mut bytes = lines[1].clone().into_bytes();
        let index = at + marker.len() - 2;
        assert_eq!(bytes[index], b'a');
        bytes[index] = b'b';
        lines[1] = String::from_utf8(bytes).unwrap();
        let after = parse(&lines[1]).unwrap();
        assert_eq!(text(&after, "revision"), revision);
        assert_ne!(revision, sha256(without_revision(&after).unwrap().string().as_bytes()));
        store_successor_lines(journal, &lines);
    }
    fn resume_child(probe: &str, root: &Path, proof: &Path, revision: &str, exact: &str) -> std::process::Output {
        test_child(
            &[
                ("RLLM_SELF_BUILD_PROBE", probe),
                ("RLLM_EXEC_ROOT", root.to_str().unwrap()),
                ("RLLM_EXEC_PROOF", proof.to_str().unwrap()),
                ("RLLM_RESUME_REVISION", revision),
            ],
            exact,
        )
    }

    #[test]
    fn resume_reaches_the_successor() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("resume-exec") {
            let root = PathBuf::from(std::env::var("RLLM_EXEC_ROOT").unwrap());
            let proof = std::env::var("RLLM_EXEC_PROOF").unwrap();
            let revision = std::env::var("RLLM_RESUME_REVISION").unwrap();
            let mut request = resume_request(&root, &revision, true);
            put(&mut request, "args", Arr(vec![s("continue"), s(&proof)]));
            let result = resume(&request);
            panic!("resume exec returned {result:?}");
        }
        let scratch = Scratch::new("resume-exec");
        let root = scratch.path();
        let (revision, digest) = prepare_committed(root);
        link_closed_successor(root);
        let journal = fs::read(root.join("self/journal.jsonl")).unwrap();
        let head = fs::read(root.join(".git/refs/heads/main")).unwrap();
        let preview = resume(&resume_request(root, &revision, false)).unwrap();
        assert_eq!(text(&preview, "commit"), digest);
        assert_eq!(text(&preview, "revision"), revision);
        assert_eq!(get(&preview, "review_confirmed"), &Bool(false));
        let parent_commit = text(&preview, "parent_commit");
        assert!(hex40(&parent_commit), "{parent_commit}");
        assert_ne!(parent_commit, digest);
        assert_eq!(
            get(&preview, "committed_paths"),
            &Arr(vec![s("notes/base.txt"), s("src/lib.rs")])
        );
        assert_eq!(text(&preview, "source_digest"), text(&rows(&root.join("self/journal.jsonl"))[0], "source_digest"));
        assert_eq!(fs::read(root.join("self/journal.jsonl")).unwrap(), journal);
        assert_eq!(fs::read(root.join(".git/refs/heads/main")).unwrap(), head);
        let proof = root.join("valid-proof");
        let output = resume_child(
            "resume-exec",
            root,
            &proof,
            &revision,
            "self_build::checks::resume_reaches_the_successor",
        );
        let body = fs::read_to_string(&proof).unwrap_or_else(|error| panic!("{error}\n{}", child_text(&output)));
        assert_eq!(body, "continue\n", "{}", child_text(&output));
        assert!(output.status.success(), "{}", child_text(&output));
    }

    #[test]
    fn resume_refuses_a_tampered_successor_journal() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("resume-tampered") {
            let root = PathBuf::from(std::env::var("RLLM_EXEC_ROOT").unwrap());
            let proof = std::env::var("RLLM_EXEC_PROOF").unwrap();
            let revision = std::env::var("RLLM_RESUME_REVISION").unwrap();
            let journal = root.join("self/journal.jsonl");
            flip_successor_job(&journal);
            let mut request = resume_request(&root, &revision, true);
            put(&mut request, "args", Arr(vec![s("continue"), s(&proof)]));
            match resume(&request) {
                Err(error) if error == "successor journal row is not content-addressed" => {
                    panic!("refused before exec: {error}");
                }
                other => panic!("resume exec returned {other:?}"),
            }
        }
        let scratch = Scratch::new("resume-tampered");
        let root = scratch.path();
        let (revision, _) = prepare_committed(root);
        link_closed_successor(root);
        let proof = root.join("tampered-proof");
        let output = resume_child(
            "resume-tampered",
            root,
            &proof,
            &revision,
            "self_build::checks::resume_refuses_a_tampered_successor_journal",
        );
        let detail = child_text(&output);
        assert!(!proof.exists(), "tampered chain still exec'd the successor\n{detail}");
        assert!(detail.contains("successor journal row is not content-addressed"), "{detail}");
        assert!(detail.contains("refused before exec"), "{detail}");
        assert!(!detail.contains("successor exec failed"), "{detail}");
        assert!(!output.status.success(), "{detail}");
    }

    #[test]
    fn resume_refuses_a_rewound_companion() {
        let _isolate = isolate();
        if std::env::var("RLLM_SELF_BUILD_PROBE").ok().as_deref() == Some("resume-rewound") {
            let root = PathBuf::from(std::env::var("RLLM_EXEC_ROOT").unwrap());
            let proof = std::env::var("RLLM_EXEC_PROOF").unwrap();
            let revision = std::env::var("RLLM_RESUME_REVISION").unwrap();
            fork_companion(&root.join("self/journal.jsonl"));
            let mut request = resume_request(&root, &revision, true);
            put(&mut request, "args", Arr(vec![s("continue"), s(&proof)]));
            match resume(&request) {
                Err(error) if error == "successor journal rolled back onto a revision already observed" => {
                    panic!("refused before exec: {error}");
                }
                other => panic!("resume exec returned {other:?}"),
            }
        }
        let scratch = Scratch::new("resume-rewound");
        let root = scratch.path();
        let (revision, _) = prepare_committed(root);
        link_three_successors(root);
        let proof = root.join("rewind-proof");
        let output = resume_child(
            "resume-rewound",
            root,
            &proof,
            &revision,
            "self_build::checks::resume_refuses_a_rewound_companion",
        );
        let detail = child_text(&output);
        assert!(!proof.exists(), "rewound chain still exec'd the successor\n{detail}");
        assert!(detail.contains("successor journal rolled back onto a revision already observed"), "{detail}");
        assert!(detail.contains("refused before exec"), "{detail}");
        assert!(!detail.contains("successor exec failed"), "{detail}");
        assert!(!output.status.success(), "{detail}");
    }

    #[test]
    fn resume_dispatch_reaches_the_verb_through_envelope() {
        let _isolate = isolate();
        let scratch = Scratch::new("resume-envelope");
        let root = scratch.path();
        let (revision, digest) = prepare_committed(root);
        link_closed_successor(root);
        let mut request = resume_request(root, &revision, false);
        put(&mut request, "op", s("self_resume"));
        let parsed = parse(&super::envelope(&request.string())).unwrap();
        assert_eq!(get(&parsed, "error"), &Null, "{}", parsed.string());
        let value = get(&parsed, "value");
        assert_eq!(text(value, "commit"), digest);
        assert_eq!(text(value, "revision"), revision);
        assert_eq!(get(value, "review_confirmed"), &Bool(false));
        assert!(hex40(&text(value, "parent_commit")), "{}", value.string());
        fs::write(root.join("self/journal.jsonl"), b"{}\n").unwrap();
        let denied = parse(&super::envelope(&request.string())).unwrap();
        assert_eq!(text(&denied, "error"), "revision is not a journalled revision of this room");
        put(&mut request, "op", s("self_resume_missing"));
        let unknown = parse(&super::envelope(&request.string())).unwrap();
        assert_eq!(text(&unknown, "error"), "Unknown operation");
    }
}

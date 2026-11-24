//! Fingerprint, bounded rebuild plan, content-addressed revision journal, re-exec, commit, and resume.
//! Reads and previews do not write or spawn. A journal append, a git commit, or a successor
//! exec requires review; source commits additionally require a local orchestrator permit.
use super::*;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use crate::host_fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd};
#[cfg(unix)]
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

// Local orchestrator authority; proposals cannot edit their own judge,
// ledger, dispatch permits, or resource policy through either write surface.
const POLICY_PATHS: &[&str] = &[
    "core/improvement.rs", "core/rrsi.rs", "core/harness.rs",
    "core/self_build.rs", "core/application.rs", "core/lib.rs",
    "core/harness_rules.rs", "core/operation.rs", "core/approval_rules.rs",
    "core/resources.rs",
    // The crate roots the sanctioned runners compile and the entry modules
    // those roots wire. A rewrite of a root decides which modules the crate
    // has; a rewrite of an entry module decides what the crate runs. Both
    // live in this one set: the write surfaces read it, never a path.
    "core/console.rs", "core/desktop.rs", "core/stdio.rs",
    "core/http.rs", "core/acp.rs",
];

fn policy_path(path: &Path) -> bool {
    let key = path.components().filter_map(|part| match part {
        std::path::Component::Normal(name) => name.to_str(),
        _ => None,
    }).collect::<Vec<_>>().join("/");
    POLICY_PATHS.contains(&key.as_str())
}

pub(crate) fn write_set(root: &Path, target: &Path) -> Result<()> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    // Canonicalize the longest existing ancestor, retaining only the missing
    // suffix. This covers an in-root directory alias and a not-yet-created file.
    let mut ancestor = target.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(mut resolved) => {
                for name in suffix.iter().rev() { resolved.push(name); }
                let relative = resolved.strip_prefix(&root).map_err(|_| "write target leaves the project")?;
                if policy_path(relative) { return Err("orchestrator policy path".into()); }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(ancestor.file_name().ok_or("write target has no parent")?.to_os_string());
                if !ancestor.pop() { return Err("write target has no parent".into()); }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn self_write_set(root: &Path, target: &Path) -> Result<()> {
    write_set(root, target).map_err(|reason| format!("Self-build refuses protected policy source: {reason}"))
}

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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
unsafe extern "C" {
    fn openat(fd: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    fn fdopendir(fd: i32) -> *mut std::ffi::c_void;
    fn readdir(dir: *mut std::ffi::c_void) -> *mut u8;
    fn closedir(dir: *mut std::ffi::c_void) -> i32;
    fn close(fd: i32) -> i32;
}
#[cfg(unix)]
struct Listed(*mut std::ffi::c_void);
#[cfg(unix)]
impl Drop for Listed {
    fn drop(&mut self) {
        unsafe { closedir(self.0) };
    }
}
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
enum Opened {
    Dir(File),
    File(File),
    Symlink,
    Other,
}
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(windows)]
fn tree(root: &Path, skip: &[String]) -> Result<(String, usize, usize)> {
    let meta = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("source root must be a real directory".into());
    }
    let mut state = Walk { files: Vec::new(), entries: 0, bytes: 0, symlinks: 0 };
    walk_path(root, "", 0, skip, &mut state)?;
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
#[cfg(windows)]
fn walk_path(dir: &Path, rel: &str, depth: usize, skip: &[String], state: &mut Walk) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err("source walk depth bound exceeded".into());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains(['/', '\\', '\0']) {
            return Err("source filename is not a single path segment".into());
        }
        names.push(name);
        if names.len() > MAX_PER_DIR {
            return Err("directory entry bound exceeded".into());
        }
    }
    names.sort();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("directory changed during enumeration".into());
    }
    for name in names {
        state.entries += 1;
        if state.entries > MAX_ENTRIES {
            return Err("source walk entry bound exceeded".into());
        }
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let path = dir.join(&name);
        let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if meta.file_type().is_symlink() {
            state.symlinks += 1;
            continue;
        }
        if meta.is_dir() {
            walk_path(&path, &child_rel, depth + 1, skip, state)?;
        } else if meta.is_file() {
            if skip.iter().any(|item| item == &child_rel) {
                continue;
            }
            let len = meta.len();
            state.bytes = state.bytes.checked_add(len).ok_or("source walk overflow")?;
            if state.bytes > MAX_TOTAL {
                return Err("source walk byte bound exceeded".into());
            }
            let bytes = read_regular(&path, MAX_FILE)?;
            state.files.push((child_rel, sha256(&bytes)));
        }
    }
    Ok(())
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

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}
#[cfg(unix)]
struct ChildGroup(i32);
#[cfg(unix)]
impl Drop for ChildGroup {
    fn drop(&mut self) {
        unsafe {
            kill(-self.0, 9);
        }
    }
}
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(windows)]
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
    super::execution::spawn_bounded(program, command, wall, cancelled)
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
    let error = replace_image(&mut command);
    Err(format!("successor exec failed: {error}"))
}
fn replace_image(command: &mut Command) -> std::io::Error {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        return CommandExt::exec(command);
    }
    #[cfg(windows)]
    {
        match command.spawn() {
            Ok(mut child) => match child.wait() {
                Ok(status) => std::process::exit(status.code().unwrap_or(1)),
                Err(error) => error,
            },
            Err(error) => error,
        }
    }
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

struct CommitPermit {
    binding: String,
    head: String,
    created: std::time::Instant,
}
static COMMIT_PERMITS: OnceLock<Mutex<BTreeMap<String, CommitPermit>>> = OnceLock::new();

fn commit_binding(value: &Json) -> Result<String> {
    use crate::host_fs::MetadataExt;
    let root = root_of(value)?.canonicalize().map_err(|error| error.to_string())?;
    let meta = fs::metadata(&root).map_err(|error| error.to_string())?;
    let mut bound = value.clone();
    if let Obj(fields) = &mut bound {
        for key in ["op", "commit_permit", "review_confirmed"] { fields.remove(key); }
    }
    put(&mut bound, "root", s(&utf8_path(&root)?));
    Ok(sha256(format!("{}:{}:{}", meta.dev(), meta.ino(), bound.string()).as_bytes()))
}

/// Called only by the local controller's review action. There is deliberately
/// no model tool or public self-operation that mints this capability.
pub(crate) fn orchestrator_commit_permit(state: &Json, value: &Json) -> Result<Json> {
    if get(state, "task_run") != &Null || flag(state, "agent_running")
        || flag(state, "agent_pending") || flag(state, "busy")
    {
        return Err("Commit review requires an idle local orchestrator".into());
    }
    only(value, &["root", "binary", "revision", "journal", "parent", "review_confirmed"])?;
    let root = root_of(value)?.canonicalize().map_err(|error| error.to_string())?;
    let approved = PathBuf::from(text(state, "approval_root")).canonicalize()
        .map_err(|_| "Commit review requires the selected workspace")?;
    if root != approved { return Err("Commit review workspace differs from the selected workspace".into()); }
    let binding = commit_binding(value)?;
    let head = read_head_commit(&require_git_dir(&root)?)?;
    let entropy = super::state_crypto::random_bytes(32)
        .map_err(|error| format!("Commit permit entropy unavailable: {error}"))?;
    let token = sha256(&entropy);
    let mut permits = COMMIT_PERMITS.get_or_init(|| Mutex::new(BTreeMap::new())).lock()
        .map_err(|_| "Commit permit ownership unavailable")?;
    permits.retain(|_, permit| permit.created.elapsed() < Duration::from_secs(120));
    if permits.len() >= 64 { return Err("Commit review capacity is occupied".into()); }
    permits.insert(token.clone(), CommitPermit { binding, head, created: std::time::Instant::now() });
    Ok(obj(vec![("commit_permit", s(&token)), ("expires_in_seconds", n(120))]))
}

fn consume_commit_permit(value: &Json, head: &str) -> Result<()> {
    let token = text(value, "commit_permit");
    if !hex64(&token) { return Err("Source commit requires an orchestrator-issued permit".into()); }
    let binding = commit_binding(value)?;
    let permit = COMMIT_PERMITS.get_or_init(|| Mutex::new(BTreeMap::new())).lock()
        .map_err(|_| "Commit permit ownership unavailable")?.remove(&token)
        .ok_or("Commit permit is unknown, consumed or belongs to another process")?;
    if permit.created.elapsed() >= Duration::from_secs(120) { return Err("Commit permit expired".into()); }
    if permit.binding != binding || permit.head != head {
        return Err("Commit permit does not match this request, workspace or head".into());
    }
    Ok(())
}

/// Commit the room's tracked source tree for one journalled revision.
/// `review_confirmed` false journals the intent and leaves every git ref untouched.
/// An execution requires a single-use capability issued by the local orchestrator.
pub(crate) fn commit(value: &Json) -> Result<Json> {
    only(value, &["root", "binary", "revision", "journal", "review_confirmed", "parent", "commit_permit"])?;
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
    consume_commit_permit(value, &parent_commit)?;
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
    let error = replace_image(&mut command);
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
    if policy_path(Path::new(&path)) {
        return Err("Self-build refuses protected policy source: orchestrator policy path".into());
    }
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
    let file = locate(root, &text(&args, "path"), false)?;
    self_write_set(root, &file)?;
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
    let target = locate(root, &path, false)?;
    self_write_set(root, &target)?;
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
    #[cfg(unix)]
    {
        tracked.signal = std::os::unix::process::ExitStatusExt::signal(&status);
    }
    #[cfg(windows)]
    {
        tracked.signal = None;
    }
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
    let reload_note = match crate::rebuild_reload::rebuild_reload(
        root,
        "core/desktop.rs",
        &root.join("RLLM.exe"),
        "source-edit",
        false,
    ) {
        Ok(text) => text,
        Err(error) => format!("Rebuild plan was not recorded: {error}"),
    };
    let saved = format!("Saved {written} bytes to {edited}.\n{reload_note}");
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

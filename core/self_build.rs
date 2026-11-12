//! Fingerprint, bounded rebuild plan, content-addressed revision journal, and re-exec.
//! Reads and previews do not write or spawn. A journal append or successor exec runs
//! only when the caller sets review_confirmed.
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
fn skip_paths(value: &Json) -> Result<Vec<String>> {
    let mut skip = Vec::new();
    for key in ["journal", "rollback"] {
        let path = text(value, key);
        if path.is_empty() {
            continue;
        }
        relative(&path)?;
        skip.push(path);
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

/// One bounded child runner for every self-build leg. The sanctioned plan steps, a
/// probe and the reviewed builder/gate all run through the single product runner,
/// which owns the child process group and stops it on timeout or cancellation.
fn bounded_child(
    root: &Path,
    program: &str,
    args: &[String],
    wall: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<super::execution::BoundedOutput> {
    let output = super::execution::bounded_process(program, args, root, wall, cancelled)?;
    if output.stdout.len() >= OUTPUT_MAX || output.stderr.len() >= OUTPUT_MAX {
        return Err("builder output exceeded its bound".into());
    }
    Ok(output)
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
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(strings(value, "args")?);
    let pid = u64::from(std::process::id());
    let log = run_log(pid, &argv, &binary_digest, &source_digest);
    if !flag(value, "review_confirmed") {
        return Ok(log);
    }
    let mut command = Command::new(&binary);
    command.args(&argv[1..]);
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
pub(crate) fn validate(args: &Json) -> Result<()> {
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
    let (carried_job, carried_instruction) = inherited();
    bound_carried(&text(args, "job"), &carried_job, JOB_MAX, "job")?;
    bound_carried(&text(args, "instruction"), &carried_instruction, INSTRUCTION_MAX, "instruction")?;
    Ok(())
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
fn sanctioned_child(
    root: &Path,
    program: &str,
    args: &[String],
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<super::execution::BoundedOutput> {
    match (program, args) {
        ("bash", [only]) if only == "headless.sh" => bounded_child(root, program, args, timeout, cancelled),
        ("bash", [script, units]) if script == "tools/build-units.sh" && relative(units).is_ok() => {
            bounded_child(root, program, args, timeout, cancelled)
        }
        (GATE_REL, [units]) if relative(units).is_ok() => {
            let path = gate_tool_path(root)?;
            let path = path.to_string_lossy().into_owned();
            bounded_child(root, &path, args, timeout, cancelled)
        }
        _ => Err("builder is not one of the sanctioned commands".into()),
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
}
static SUCCESSORS: OnceLock<Mutex<BTreeMap<u64, Tracked>>> = OnceLock::new();
fn successors() -> std::sync::MutexGuard<'static, BTreeMap<u64, Tracked>> {
    SUCCESSORS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}
fn track(child: std::process::Child, job: &str, job_source: &str, mode: &str) -> u64 {
    let pid = u64::from(child.id());
    successors().insert(
        pid,
        Tracked {
            child: Some(child),
            job: job.to_string(),
            job_source: job_source.to_string(),
            mode: mode.to_string(),
            exit: None,
        },
    );
    pid
}
// The exit status is not recorded on disk.
fn reap() -> usize {
    let mut table = successors();
    let mut exited = 0usize;
    for tracked in table.values_mut() {
        if tracked.mode != "spawn" || tracked.child.is_none() {
            continue;
        }
        let status = {
            let Some(child) = tracked.child.as_mut() else { continue };
            match child.try_wait() {
                Ok(status) => status,
                Err(_) => continue,
            }
        };
        let Some(status) = status else { continue };
        tracked.exit = status.code();
        tracked.child = None;
        exited += 1;
    }
    exited
}
pub(crate) fn successor_states() -> Vec<Json> {
    successors()
        .iter()
        .map(|(pid, tracked)| {
            let running = tracked.child.is_some() && tracked.mode == "spawn";
            obj(vec![
                ("pid", n(*pid)),
                ("job", s(&tracked.job)),
                ("job_source", s(&tracked.job_source)),
                ("running", Bool(running)),
                (
                    "exit",
                    if running {
                        Null
                    } else {
                        match tracked.exit {
                            Some(code) => Num(code.to_string()),
                            None => Null,
                        }
                    },
                ),
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
    let reaped = reap();
    if cancelled() {
        return Err("self-build cancelled".into());
    }
    let args = get(value, "args");
    validate(args)?;
    let (carried_job, carried_instruction) = inherited();
    let (job, job_source) = bound_carried(&text(args, "job"), &carried_job, JOB_MAX, "job")?;
    let (instruction, _) = bound_carried(
        &text(args, "instruction"),
        &carried_instruction,
        INSTRUCTION_MAX,
        "instruction",
    )?;
    let reload = text(args, "reload");
    if !reload.is_empty() && reload != "exec" && reload != "spawn" {
        return Err("reload must be exec, spawn, or empty".into());
    }
    let timeout = reviewed_timeout(args)?;
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
        let built = sanctioned_child(root, &text(args, "builder"), &strings(args, "builder_args")?, timeout, cancelled)?;
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
        let gate = sanctioned_child(root, &text(args, "gate"), &strings(args, "gate_args")?, timeout, cancelled)?;
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
        let mut handed = Vec::new();
        if !job.is_empty() {
            child.env("RLLM_SELF_JOB", &job);
            handed.push("RLLM_SELF_JOB".to_string());
        }
        if !instruction.is_empty() {
            child.env("RLLM_SELF_INSTRUCTION", &instruction);
            handed.push("RLLM_SELF_INSTRUCTION".to_string());
        }
        let child = child.spawn().map_err(|error| format!("successor spawn failed: {error}"))?;
        let pid = track(child, &job, job_source, "spawn");
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

    #[test]
    fn green_gate_journals_and_links_parent() {
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
        let scratch = Scratch::new("handoff-skip");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn shipped() {}\n");
        write(&root.join("next-bin"), b"successor-image");
        write(&root.join("self/journal.jsonl"), b"{\"schema\":\"1\"}\n");
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
        let scratch = Scratch::new("missing-gate");
        let root = scratch.path();
        let error = match sanctioned_child(
            root,
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
        let scratch = Scratch::new("spawn-job");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        green_builder(root);
        let job = "job-9";
        let instruction = "keep \"both\"\nlines";
        let live = || false;
        let first_content = "fn a() { let _ = 1; }\n";
        let first_proof = root.join("proof-set");
        let receipt = run(
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
            &live,
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
        let absent = run(
            root,
            &spawn_fields(
                second_content,
                first_content,
                &text(&stored[0], "revision"),
                vec![("successor_args", Arr(vec![s(second_proof.to_str().unwrap()), s("continue")]))],
            ),
            &live,
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
        let scratch = Scratch::new("mode");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"not a program\n");
        green_builder(root);
        let live = || false;
        let first = "fn a() { let _ = 1; }\n";
        let receipt = run(root, &journal_request(first, "", Vec::new()), &live).unwrap();
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
        let error = run(
            root,
            &journal_request(first, &sha256(b"wrong-parent"), vec![("reload", s("exec"))]),
            &live,
        )
        .unwrap_err();
        assert_eq!(
            error, "parent revision does not match the journal tail",
            "{error}; exec was refused before a revision was recorded, so there is no successor row to read"
        );
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), first);
        assert_eq!(rows(&root.join("self/journal.jsonl")).len(), 1);
        let second = "fn a() { let _ = 2; }\n";
        let error = run(
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
            &live,
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
        let _ = reap();
        let scratch = Scratch::new("job-source-none");
        let root = scratch.path();
        write(&root.join("src/lib.rs"), b"fn a() {}\n");
        write(&root.join("bin"), b"not a program\n");
        green_builder(root);
        let content = "fn a() { let _ = 1; }\n";
        let live = || false;
        let receipt = run(root, &journal_request(content, "", Vec::new()), &live).unwrap();
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
        static LOCK: Mutex<()> = Mutex::new(());
        let _lock = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                std::env::remove_var("RLLM_SELF_JOB");
                std::env::remove_var("RLLM_SELF_INSTRUCTION");
            }
        }
        let _clear = Clear;
        let _ = reap();
        let scratch = Scratch::new("carried");
        let root = scratch.path();
        let original = "fn a() {}\n";
        write(&root.join("src/lib.rs"), original.as_bytes());
        executable(&root.join("bin"), SUCCESSOR_SCRIPT.as_bytes());
        green_builder(root);
        let live = || false;
        std::env::set_var("RLLM_SELF_JOB", "carried-job");
        std::env::set_var("RLLM_SELF_INSTRUCTION", "carried-instruction");
        let first = "fn a() { let _ = 1; }\n";
        let first_proof = root.join("proof-inherited");
        let receipt = run(
            root,
            &spawn_fields(
                first,
                original,
                "",
                vec![("successor_args", Arr(vec![s(first_proof.to_str().unwrap()), s("continue")]))],
            ),
            &live,
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
        let dumped = wait_bytes(&PathBuf::from(format!("{}.done", first_proof.display())));
        let parts: Vec<&[u8]> = dumped.split(|byte| *byte == 0).collect();
        assert_eq!(parts[2], b"carried-job");
        assert_eq!(parts[3], b"carried-instruction");
        assert_eq!(parts[4], b"job-set");
        assert_eq!(parts[5], b"instruction-set");
        wait_dead(first_pid_num);
        let second = "fn a() { let _ = 2; }\n";
        let second_proof = root.join("proof-caller");
        let receipt = run(
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
            &live,
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
        std::env::set_var("RLLM_SELF_JOB", "j".repeat(JOB_MAX + 1));
        std::env::set_var("RLLM_SELF_INSTRUCTION", "short");
        let refused = obj(vec![(
            "args",
            obj(vec![
                ("path", s("src/lib.rs")),
                ("content", s("fn a() { let _ = 3; }\n")),
                ("previous_exists", Bool(true)),
                ("approved_previous", s(second)),
            ]),
        )]);
        let error = run(root, &refused, &live).unwrap_err();
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
        let error = run(root, &caller_over, &live).unwrap_err();
        assert_eq!(error, "job exceeds 256 bytes");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), before);
        std::env::set_var("RLLM_SELF_JOB", "ok");
        std::env::set_var("RLLM_SELF_INSTRUCTION", "i".repeat(INSTRUCTION_MAX + 1));
        let error = run(root, &refused, &live).unwrap_err();
        assert_eq!(error, "inherited instruction exceeds 16384 bytes");
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), before);
        assert_eq!(rows(&root.join("self/journal.jsonl")).len(), 2);
    }

    #[test]
    fn tracked_reap_reports_an_exited_successor_once() {
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
        assert_eq!(get(&tracked_state(quick_pid), "running"), &Bool(true));
        assert_eq!(get(&tracked_state(quick_pid), "exit"), &Null);
        assert_eq!(get(&tracked_state(hold_pid), "running"), &Bool(true));
        assert_eq!(get(&tracked_state(hold_pid), "exit"), &Null);
        assert_eq!(text(&tracked_state(hold_pid), "job"), "held-job");
        assert_eq!(text(&tracked_state(hold_pid), "job_source"), "caller");
        assert_eq!(reap(), 1);
        assert_eq!(reap(), 0);
        assert_eq!(get(&tracked_state(quick_pid), "running"), &Bool(false));
        assert_eq!(get(&tracked_state(quick_pid), "exit"), &Num("5".into()));
        assert_eq!(text(&tracked_state(quick_pid), "job"), "done-job");
        assert_eq!(text(&tracked_state(quick_pid), "job_source"), "inherited");
        assert_eq!(get(&tracked_state(hold_pid), "running"), &Bool(true));
        assert_eq!(get(&tracked_state(hold_pid), "exit"), &Null);
        write(&hold_path, b"go");
        wait_dead(hold_pid);
        assert_eq!(reap(), 1);
        assert_eq!(reap(), 0);
        assert_eq!(get(&tracked_state(hold_pid), "running"), &Bool(false));
        assert_eq!(get(&tracked_state(hold_pid), "exit"), &Num("4".into()));
    }

    #[test]
    fn tracked_reaped_text_appears_only_after_a_successor_exits() {
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
}

//! Desktop host: the same application core, the window, the transport and the key.
//! Product logic stays in the core. This file owns the window and the platform work.
//! The image is a console program so a shell passes the command line and waits.
//! The interactive path hides that console before the window is created.
include!("lib.rs");
mod harness_mcp;

use std::io::{Read, Seek, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

const EFFECT_HOPS: usize = 32;
// Counts the platform work the host really performed, so a verification leg
// cannot pass by never running.
static EXECUTED_TOOLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static CAPTURED_FRAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
// Ownership tokens the core names on an effect and expects back untouched.
const TICKETS: &[&str] = &[
    "drive_ticket",
    "child_ticket",
    "session",
    "ticket",
    "capture_token",
    "stream_ticket",
    "tool_ticket",
    "tool_binding",
    "connection",
    "chat",
    "agent",
    "request",
    "tree",
    "editor",
    "editor_dialog",
    "phase",
    "content",
];

fn get<'a>(value: &'a Json, key: &str) -> &'a Json {
    match value {
        Obj(fields) => fields.get(key).unwrap_or(&Null),
        _ => &Null,
    }
}

fn text(value: &Json, key: &str) -> String {
    get(value, key).text().unwrap_or("").to_string()
}

fn number(value: &Json, key: &str) -> i64 {
    match get(value, key) {
        Num(raw) | Str(raw) => raw.parse().unwrap_or(0),
        _ => 0,
    }
}

fn boolean(value: &Json, key: &str) -> bool {
    match get(value, key) {
        Bool(flag) => *flag,
        Str(raw) => *raw == *"true",
        _ => false,
    }
}

fn put(target: &mut Json, key: &str, value: Json) {
    if let Obj(fields) = target {
        fields.insert(key.into(), value);
    }
}

fn set_field(state: Json, key: &str, value: Json) -> Json {
    let mut state = state;
    if state == Null {
        state = Obj(BTreeMap::new());
    }
    put(&mut state, key, value);
    state
}

fn arguments() -> Vec<std::ffi::OsString> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        unsafe extern "system" {
            fn GetCommandLineW() -> *const u16;
            fn CommandLineToArgvW(cmd: *const u16, argc: *mut i32) -> *mut *mut u16;
            fn LocalFree(block: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        }
        unsafe {
            let mut count = 0i32;
            let argv = CommandLineToArgvW(GetCommandLineW(), &mut count);
            if argv.is_null() || count <= 0 {
                return Vec::new();
            }
            let mut args = Vec::new();
            for index in 1..count {
                let ptr = *argv.add(index as usize);
                if ptr.is_null() {
                    continue;
                }
                let mut len = 0usize;
                while *ptr.add(len) != 0 {
                    len += 1;
                    if len > 32768 {
                        break;
                    }
                }
                args.push(std::ffi::OsString::from_wide(std::slice::from_raw_parts(ptr, len)));
            }
            LocalFree(argv as *mut std::ffi::c_void);
            return args;
        }
    }
    #[cfg(not(windows))]
    std::env::args_os().skip(1).collect()
}

fn has_flag(args: &[std::ffi::OsString], flag: &str) -> bool {
    args.iter().any(|value| value == flag)
}

fn option_path(args: &[std::ffi::OsString], flag: &str) -> Result<Option<PathBuf>> {
    let Some(index) = args.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs a value"))?;
    Ok(Some(PathBuf::from(value)))
}

fn path_text(path: &Path) -> String {
    path.to_str().map(str::to_owned).unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn data_directory() -> PathBuf {
    if let Ok(base) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(base).join("rllm");
    }
    #[cfg(windows)]
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(base).join("rllm");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local").join("share").join("rllm")
}

fn host_byte_budget() -> u64 {
    archive::project_budget().max(1024 * 1024)
}

/// Read at most `limit` bytes. A longer file is refused from its length before the
/// extra bytes are kept, and a short or lying length still stops at `limit`.
fn read_file_limited(file: &std::fs::File, limit: u64) -> std::io::Result<Vec<u8>> {
    if let Ok(meta) = file.metadata() {
        if meta.file_type().is_file() && meta.len() > limit {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "file exceeds the memory budget"));
        }
    }
    let mut bytes = Vec::new();
    std::io::Read::take(file, limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "file exceeds the memory budget"));
    }
    Ok(bytes)
}

/// A regular file, at most `limit` bytes. The type is taken from
/// `symlink_metadata` (and from `metadata` when the path is a link) before
/// anything is opened, so a pipe with no writer cannot stall a conversation
/// backup import. On Unix the open is non-blocking and the descriptor is
/// checked again, so a swap between the type check and the open is refused too.
fn read_path_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let linked = std::fs::symlink_metadata(path).map_err(|error| format!("The file could not be read: {error}"))?;
    let regular = if linked.file_type().is_symlink() {
        std::fs::metadata(path).map_err(|error| format!("The file could not be read: {error}"))?.is_file()
    } else {
        linked.is_file()
    };
    if !regular {
        return Err("The selected file is not a regular file".into());
    }
    let file = open_regular_for_read(path)?;
    let opened = file.metadata().map_err(|error| format!("The file could not be read: {error}"))?;
    if !opened.is_file() {
        return Err("The selected file is not a regular file".into());
    }
    read_file_limited(&file, limit).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidData {
            format!("The selected file exceeds {} MiB", (limit / (1024 * 1024)).max(1))
        } else {
            format!("The file could not be read: {error}")
        }
    })
}

fn open_regular_for_read(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use crate::host_fs::OpenOptionsExt;
        // Generic non-blocking bit. The host maps it before the open.
        options.custom_flags(0x800);
    }
    options.open(path).map_err(|error| format!("The file could not be read: {error}"))
}

enum PathKind {
    Missing,
    Regular,
    Other,
}

/// Type from `symlink_metadata` before any open. A pipe is not a regular file,
/// so the caller never opens it.
fn path_kind(path: &Path) -> std::io::Result<PathKind> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(PathKind::Missing),
        Err(error) => Err(error),
        Ok(meta) => {
            let regular = if meta.file_type().is_symlink() {
                std::fs::metadata(path).map(|target| target.is_file()).unwrap_or(false)
            } else {
                meta.is_file()
            };
            Ok(if regular { PathKind::Regular } else { PathKind::Other })
        }
    }
}

fn create_exclusive(path: &Path) -> std::io::Result<std::fs::File> {
    use crate::host_fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    #[cfg(unix)]
    {
        options.custom_flags(nonblocking_read_flags());
    }
    options.open(path)
}

/// Write `bytes` to a sibling temporary file and publish that file onto `path`
/// only after the write and sync succeed. An existing regular file stays in
/// place until the replacement is ready. A pipe is refused before it is opened.
fn publish_regular_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let dest = match path_kind(path).map_err(|error| format!("Saved state: {error}"))? {
        PathKind::Other => return Err("Saved state is not a regular file".into()),
        PathKind::Missing => path.to_path_buf(),
        PathKind::Regular => {
            let meta = std::fs::symlink_metadata(path).map_err(|error| format!("Saved state: {error}"))?;
            if meta.file_type().is_symlink() {
                std::fs::canonicalize(path).map_err(|error| format!("Saved state: {error}"))?
            } else {
                path.to_path_buf()
            }
        }
    };
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| format!("State directory: {error}"))?;
        }
    }
    let name = dest.file_name().ok_or_else(|| "Saved state path is empty".to_string())?;
    let parent = dest.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let temp = parent.join(format!(".{}.{}.{stamp}.new", name.to_string_lossy(), std::process::id()));
    let mut file = create_exclusive(&temp).map_err(|error| format!("Saved state: {error}"))?;
    let opened = file.metadata().map_err(|error| format!("Saved state: {error}"))?;
    if !opened.is_file() {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err("Saved state is not a regular file".into());
    }
    let wrote = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = wrote {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("Saved state: {error}"));
    }
    if let Err(error) = replace_published_file(&temp, &dest) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn replace_published_file(from: &Path, to: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        }
        let source: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
        let dest: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(source.as_ptr(), dest.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(format!("Saved state: {}", std::io::Error::last_os_error()));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to).map_err(|error| format!("Saved state: {error}"))
    }
}

fn read_key(path: &Path) -> Result<[u8; 32]> {
    match path_kind(path).map_err(|error| format!("State key: {error}"))? {
        PathKind::Other => return Err("State key file is not a regular file".into()),
        PathKind::Regular => {
            let file = open_regular_for_read(path)?;
            let opened = file.metadata().map_err(|error| format!("State key: {error}"))?;
            if !opened.is_file() {
                return Err("State key file is not a regular file".into());
            }
            let bytes = read_file_limited(&file, 32).map_err(|error| {
                if error.kind() == std::io::ErrorKind::InvalidData {
                    "State key file must hold exactly 32 bytes".to_string()
                } else {
                    format!("State key: {error}")
                }
            })?;
            if bytes.len() != 32 {
                return Err("State key file must hold exactly 32 bytes".into());
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            Ok(key)
        }
        PathKind::Missing => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|error| format!("State directory: {error}"))?;
                }
            }
            let fresh = state_crypto::random_bytes(32)?;
            let mut file = create_exclusive(path).map_err(|error| format!("State key: {error}"))?;
            let opened = file.metadata().map_err(|error| format!("State key: {error}"))?;
            if !opened.is_file() {
                return Err("State key file is not a regular file".into());
            }
            file.write_all(&fresh).map_err(|error| format!("State key: {error}"))?;
            file.sync_all().map_err(|error| format!("State key: {error}"))?;
            let mut key = [0u8; 32];
            key.copy_from_slice(&fresh);
            Ok(key)
        }
    }
}

struct Vault {
    path: PathBuf,
    key: [u8; 32],
}

impl Vault {
    /// A missing file is a fresh vault. A sealed file that fails authentication
    /// is reported rather than replaced, so corruption is never silent.
    fn load(&self) -> Result<Option<Json>> {
        match path_kind(&self.path).map_err(|error| format!("Saved state: {error}"))? {
            PathKind::Missing => return Ok(None),
            PathKind::Other => return Err("Saved state is not a regular file".into()),
            PathKind::Regular => {}
        }
        let file = open_regular_for_read(&self.path)?;
        let opened = file.metadata().map_err(|error| format!("Saved state: {error}"))?;
        if !opened.is_file() {
            return Err("Saved state is not a regular file".into());
        }
        let sealed = read_file_limited(&file, host_byte_budget()).map_err(|error| {
            if error.kind() == std::io::ErrorKind::InvalidData {
                "Saved state exceeds the memory budget".to_string()
            } else {
                format!("Saved state: {error}")
            }
        })?;
        let plain = state_crypto::open(&sealed, &self.key)?;
        let text = String::from_utf8(plain).map_err(|_| "Saved state is not UTF-8".to_string())?;
        match parse(&text)? {
            Obj(_) => Ok(Some(parse(&text)?)),
            _ => Err("Saved state is not an object".into()),
        }
    }

    fn save(&self, state: &Json) -> Result<()> {
        let sealed = state_crypto::seal(state.string().as_bytes(), &self.key)?;
        publish_regular_file(&self.path, &sealed)
    }
}

/// The core owns the request body: it hands over the framing and, for a request
/// that carries a stored file, streams the bytes through an owned upload ticket.
fn request_wire(effect: &Json) -> Result<Vec<u8>> {
    if get(effect, "wire_base64") != &Null {
        let encoded = text(effect, "wire_base64");
        if encoded.len() as u64 > host_byte_budget().saturating_mul(2) {
            return Err("Request exceeds the memory budget".into());
        }
        return network::unbase64(&encoded);
    }
    let wire = text(effect, "wire");
    if wire.len() as u64 > host_byte_budget() {
        return Err("Request exceeds the memory budget".into());
    }
    Ok(wire.into_bytes())
}

const UPLOAD_STALL: &str = "the upload did not finish within two minutes";

struct WriteDeadline {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    deadline_active: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl WriteDeadline {
    fn begin_response(&self) {
        self.deadline_active.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for WriteDeadline {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Unblocks a socket write that would otherwise ignore the deadline. The socket
/// send timeout carries that deadline: closing a duplicate does not wake a send
/// already waiting in the operating system. The clone still shuts the connection
/// down when the deadline arrives, including a write hidden inside a transport record.
fn arm_write_deadline(stream: &TcpStream, deadline: std::time::Instant) -> WriteDeadline {
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    let wait = if remaining.is_zero() { std::time::Duration::from_millis(1) } else { remaining };
    let _ = stream.set_write_timeout(Some(wait));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&stop);
    let deadline_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let deadline_flag = std::sync::Arc::clone(&deadline_active);
    let worker = stream.try_clone().ok().map(|poke| {
        std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let upload_expired = std::time::Instant::now() >= deadline
                    && deadline_flag.load(std::sync::atomic::Ordering::Relaxed);
                if CANCEL.load(std::sync::atomic::Ordering::Relaxed) || upload_expired {
                    let _ = poke.shutdown(std::net::Shutdown::Both);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        })
    });
    WriteDeadline { stop, deadline_active, worker }
}

fn connect_within(host: &str, port: u16, deadline: std::time::Instant) -> Result<TcpStream> {
    use std::net::ToSocketAddrs;
    if std::time::Instant::now() >= deadline {
        return Err(UPLOAD_STALL.into());
    }
    let addresses = (host, port).to_socket_addrs().map_err(|error| format!("{host}:{port}: {error}"))?;
    let mut last = format!("{host}:{port}: no address");
    let mut attempted = false;
    for address in addresses {
        attempted = true;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(UPLOAD_STALL.into());
        }
        match TcpStream::connect_timeout(&address, remaining) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = format!("{host}:{port}: {error}"),
        }
    }
    Err(if attempted { last } else { format!("{host}:{port}: no address") })
}

/// Writes every byte or stops at the deadline. A short socket write timeout lets
/// the loop notice a peer that accepts nothing, or only a trickle.
fn write_all_until(stream: &mut dyn Write, mut bytes: &[u8], deadline: std::time::Instant) -> Result<()> {
    while !bytes.is_empty() {
        if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        if std::time::Instant::now() >= deadline {
            return Err(UPLOAD_STALL.into());
        }
        match stream.write(bytes) {
            Ok(0) => return Err("peer closed early".into()),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {}
            Err(_) if std::time::Instant::now() >= deadline || CANCEL.load(std::sync::atomic::Ordering::Relaxed) => {
                if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(CANCELLED.into());
                }
                return Err(UPLOAD_STALL.into());
            }
            Err(_) => return Err("peer closed early".into()),
        }
    }
    Ok(())
}

fn write_request(sink: &mut impl Write, effect: &Json, deadline: std::time::Instant) -> Result<()> {
    write_all_until(sink, &request_wire(effect)?, deadline)?;
    let upload = text(effect, "media_upload");
    if upload.is_empty() {
        return Ok(());
    }
    let purpose = text(effect, "purpose");
    loop {
        if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        if std::time::Instant::now() >= deadline {
            return Err(UPLOAD_STALL.into());
        }
        let answer = parse(
            &envelope(&obj(vec![
                ("op", s("media_upload_read")),
                ("ticket", s(&upload)),
                ("purpose", s(&purpose)),
            ])
            .string()),
        )?;
        if let Ok(error) = answer.get("error") {
            return Err(error.text().unwrap_or("the core refused the upload").to_string());
        }
        let value = answer.get("value")?;
        let chunk = network::unbase64(&text(value, "data"))?;
        write_all_until(sink, &chunk, deadline)?;
        if boolean(value, "done") {
            return Ok(());
        }
    }
}

use std::time::{Duration, Instant};

struct ExchangeFailure {
    message: String,
    retry: Option<RetryDelay>,
    attempts: u32,
    received_bytes: usize,
}

struct ResponseReader<'a> {
    stream: &'a mut dyn Read,
    received_bytes: usize,
}

impl Read for ResponseReader<'_> {
    fn read(&mut self, chunk: &mut [u8]) -> std::io::Result<usize> {
        let read = self.stream.read(chunk)?;
        self.received_bytes = self.received_bytes.saturating_add(read);
        Ok(read)
    }
}

enum RetryDelay {
    Backoff,
    After(Duration),
}

impl ExchangeFailure {
    fn permanent(message: impl Into<String>) -> Self {
        Self { message: message.into(), retry: None, attempts: 0, received_bytes: 0 }
    }

    fn transient(message: impl Into<String>) -> Self {
        Self { message: message.into(), retry: Some(RetryDelay::Backoff), attempts: 0, received_bytes: 0 }
    }

    fn upload(message: String) -> Self {
        if matches!(message.as_str(), UPLOAD_STALL | "peer closed early") {
            return Self::transient(message);
        }
        Self::permanent(message)
    }

    fn transport(message: String) -> Self {
        if matches!(message.as_str(), CANCELLED | "Network effect has no usable destination") {
            return Self::permanent(message);
        }
        Self::transient(message)
    }

    fn handshake(message: String) -> Self {
        if ["handshake could not be sent", "handshake was interrupted", "closed the connection during the TLS handshake",
            "TLS peer closed during the handshake", "TLS handshake timed out", "TLS read:"]
            .iter().any(|detail| message.contains(detail))
        {
            return Self::transient(message);
        }
        Self::permanent(message)
    }

    fn response(message: String) -> Self {
        let reason = message.split_once(": ").map_or(message.as_str(), |(reason, _)| reason);
        if matches!(reason, RESPONSE_SILENCE | "peer closed before finishing"
            | "Incomplete HTTP headers" | "Incomplete chunk header" | "Incomplete chunk trailers"
            | "Incomplete chunk data" | "Incomplete or oversized response body" | "HTTP response framing is incomplete"
            | "Connection ended before the model completed its reply"
            | "Connection ended before the Content candidate completed"
            | "Stream ended without a completed choice" | provider::EMPTY_REPLY)
        {
            return Self::transient(message);
        }
        Self::permanent(message)
    }
}

const PROVIDER_ATTEMPTS: u32 = 10;

enum ExchangeProgress<'a> {
    Preview(&'a str, u128),
    Retry { failed: u32, delay: Duration },
}

fn exchange(
    effect: &Json, progress: &mut dyn FnMut(ExchangeProgress<'_>),
) -> std::result::Result<Vec<u8>, ExchangeFailure> {
    let provider = text(effect, "purpose") == "model";
    for attempt in 1..=PROVIDER_ATTEMPTS {
        if transport_cancelled() {
            return Err(ExchangeFailure::permanent(CANCELLED));
        }
        let outcome = exchange_once(effect, &mut |said, elapsed| progress(ExchangeProgress::Preview(said, elapsed)), provider);
        if transport_cancelled() {
            return Err(ExchangeFailure::permanent(CANCELLED));
        }
        let mut failure = match outcome {
            Ok(response) => return Ok(response),
            Err(failure) => failure,
        };
        if !provider {
            return Err(failure);
        }
        let delay = match failure.retry {
            Some(RetryDelay::After(delay)) => delay,
            Some(RetryDelay::Backoff) => Duration::from_secs(match attempt {
                1 => 60,
                2 => 120,
                3 => 240,
                _ => 300,
            }),
            None if attempt == 1 => return Err(failure),
            None if provider_calls::rejection(&failure.message).is_some() => {
                failure.attempts = attempt;
                return Err(failure);
            }
            None => {
                failure.message = format!("Provider request failed after {attempt} attempts: {}", failure.message);
                failure.attempts = attempt;
                return Err(failure);
            }
        };
        if attempt == PROVIDER_ATTEMPTS {
            failure.message = format!("Provider request failed after {attempt} attempts: {}", failure.message);
            failure.attempts = attempt;
            return Err(failure);
        }
        progress(ExchangeProgress::Retry { failed: attempt, delay });
        wait_for_provider(delay).map_err(ExchangeFailure::permanent)?;
    }
    unreachable!("provider attempts are bounded and nonzero")
}

fn wait_for_provider(delay: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        if transport_cancelled() {
            return Err(CANCELLED.into());
        }
        let remaining = delay.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(100)));
    }
}

fn transport_cancelled() -> bool {
    CANCEL.load(std::sync::atomic::Ordering::Relaxed)
}

// System name resolution and socket connect can block. The caller can stop
// waiting for this attempt even if the operating system has not returned yet.
fn connect_provider(host: String, port: u16, deadline: Instant) -> Result<TcpStream> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new().spawn(move || {
        let _ = send.send(connect_within(&host, port, deadline));
    }).map_err(|error| format!("Could not start provider connection: {error}"))?;
    loop {
        if transport_cancelled() {
            return Err(CANCELLED.into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Provider connection timed out".into());
        }
        match receive.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(connection) => return connection,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err("Provider connection did not finish".into()),
        }
    }
}

/// HTTP to the address the core named. The platform validates TLS identity
/// and the core validates the response framing and content.
fn exchange_once(effect: &Json, heard: &mut dyn FnMut(&str, u128), provider: bool) -> std::result::Result<Vec<u8>, ExchangeFailure> {
    let host = text(effect, "host");
    let port = number(effect, "port");
    if host.is_empty() || port == 0 || port > 65535 {
        return Err(ExchangeFailure::permanent("Network effect has no usable destination"));
    }
    let connect = host.replace('%', "");
    let display = if connect.contains(':') { format!("[{connect}]") } else { connect.clone() };
    let deadline = silence_deadline();
    let mut raw = if provider {
        connect_provider(display, port as u16, deadline)
    } else {
        connect_within(&display, port as u16, deadline)
    }.map_err(ExchangeFailure::transport)?;
    let guard = arm_write_deadline(&raw, deadline);
    let poll = Some(Duration::from_millis(200));
    if boolean(effect, "tls") {
        let named = effect.get("tls_host").and_then(Json::text).ok().filter(|s| !s.is_empty()).unwrap_or(&connect);
        let named = named.trim_start_matches('[').trim_end_matches(']');
        raw.set_read_timeout(Some(Duration::from_secs(30))).ok();
        #[cfg(windows)]
        let mut link = secure::Secure::open(raw, named).map_err(ExchangeFailure::handshake)?;
        #[cfg(not(windows))]
        let mut link = tls_client::Client::open(raw, named).map_err(ExchangeFailure::handshake)?;
        link.set_read_timeout(poll);
        return converse(&mut link, effect, heard, deadline, guard, provider);
    }
    raw.set_read_timeout(poll).ok();
    raw.set_write_timeout(poll).ok();
    converse(&mut raw, effect, heard, deadline, guard, provider)
}

fn converse<S: Read + Write>(
    stream: &mut S, effect: &Json, heard: &mut dyn FnMut(&str, u128),
    deadline: Instant, guard: WriteDeadline, provider: bool,
) -> std::result::Result<Vec<u8>, ExchangeFailure> {
    write_request(stream, effect, deadline).map_err(ExchangeFailure::upload)?;
    if Instant::now() >= deadline {
        return Err(ExchangeFailure::transient(UPLOAD_STALL));
    }
    stream.flush().map_err(|error| ExchangeFailure::transient(format!("peer closed early: {error}")))?;
    let mut reader = ResponseReader { stream, received_bytes: 0 };
    let outcome = if provider {
        // Keep cancellation able to wake a read inside the platform transport.
        // Only the independent silence timer applies to the response.
        guard.begin_response();
        read_provider_response(&mut reader, effect, heard)
    } else if boolean(effect, "stream") {
        read_stream(&mut reader, effect, heard, deadline).map_err(ExchangeFailure::permanent)
    } else {
        collect_until(&mut reader, response_budget(effect), deadline).map_err(ExchangeFailure::permanent)
    };
    outcome.map_err(|mut failure| {
        failure.received_bytes = reader.received_bytes;
        failure
    })
}

fn silence_deadline() -> Instant {
    Instant::now() + Duration::from_millis(SILENCE_MS as u64)
}

fn read_provider_response(
    stream: &mut dyn Read, effect: &Json, heard: &mut dyn FnMut(&str, u128),
) -> std::result::Result<Vec<u8>, ExchangeFailure> {
    let mut deadline = silence_deadline();
    let streaming = boolean(effect, "stream");
    let limit = response_budget(effect);
    let mut response = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut offset = 0;
    let mut interim = 0;
    let status = loop {
        if let Some(end) = network::response_head_end(&response[offset..]).map_err(ExchangeFailure::permanent)? {
            let end = offset + end;
            let head = std::str::from_utf8(&response[offset..end])
                .map_err(|_| ExchangeFailure::permanent("Invalid response headers"))?;
            let mut lines = head.split("\r\n");
            let status = network::response_status(lines.next().unwrap_or_default()).map_err(ExchangeFailure::permanent)?;
            if status >= 200 {
                if status == 429 || status >= 500 {
                    let mut retry_after = None;
                    for line in lines {
                        let (name, value) = network::response_field(line).map_err(ExchangeFailure::permanent)?;
                        if name.eq_ignore_ascii_case("retry-after") {
                            retry_after = retry_after.max(provider_retry_after(value));
                        }
                    }
                    return Err(ExchangeFailure {
                        message: format!("HTTP {status}"),
                        retry: Some(retry_after.map_or(RetryDelay::Backoff, RetryDelay::After)),
                        attempts: 0,
                        received_bytes: 0,
                    });
                }
                break status;
            }
            interim += 1;
            if interim > 4 || status == 101 {
                return Err(ExchangeFailure::permanent("Unsupported informational response"));
            }
            offset = end + 4;
            continue;
        }
        let read = wait_until(stream, &mut chunk, deadline).map_err(ExchangeFailure::response)?;
        if read == 0 {
            return Err(ExchangeFailure::transient("Connection ended before the provider sent a complete response header"));
        }
        deadline = silence_deadline();
        if !streaming && read > limit.saturating_sub(response.len()) {
            return Err(ExchangeFailure::permanent("Response exceeds the current memory budget"));
        }
        response.extend_from_slice(&chunk[..read]);
    };
    if streaming {
        let mut stream = std::io::Cursor::new(response).chain(stream);
        return read_stream(&mut stream, effect, heard, deadline).map_err(ExchangeFailure::response);
    }

    let mut framing = network::ResponseFrame::default();
    for part in response.chunks(chunk.len()) {
        framing.feed(part).map_err(ExchangeFailure::response)?;
    }
    while !framing.done {
        let read = wait_until(stream, &mut chunk, deadline).map_err(ExchangeFailure::response)?;
        if read == 0 {
            framing.finish().map_err(ExchangeFailure::response)?;
            break;
        }
        deadline = silence_deadline();
        if read > limit.saturating_sub(response.len()) {
            return Err(ExchangeFailure::permanent("Response exceeds the current memory budget"));
        }
        framing.feed(&chunk[..read]).map_err(ExchangeFailure::response)?;
        response.extend_from_slice(&chunk[..read]);
    }
    if (200..300).contains(&status) && network::decoded_bytes(&response).is_ok_and(|(_, body)| body.is_empty()) {
        return Err(ExchangeFailure::transient(provider::EMPTY_REPLY));
    }
    Ok(response)
}

fn provider_retry_after(value: &str) -> Option<Duration> {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse().ok().map(Duration::from_secs);
    }
    // HTTP dates use GMT. Also accept the two legacy HTTP date shapes.
    let fields: Vec<_> = value.split_ascii_whitespace().collect();
    let (day, month, year, time) = match fields.as_slice() {
        [weekday, day, month, year, time, "GMT"] if weekday.ends_with(',') => (*day, *month, year.parse::<i64>().ok()?, *time),
        [weekday, date, time, "GMT"] if weekday.ends_with(',') => {
            let mut date = date.split('-');
            let (day, month, year) = (date.next()?, date.next()?, date.next()?);
            if year.len() != 2 || date.next().is_some() { return None; }
            let year = year.parse::<i64>().ok()?;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
            let current_year = (now / 86400 + 719468) * 400 / 146097;
            let proposed = current_year / 100 * 100 + year;
            (day, month, if proposed > current_year + 50 { proposed - 100 } else { proposed }, *time)
        }
        [_, month, day, time, year] => (*day, *month, year.parse::<i64>().ok()?, *time),
        _ => return None,
    };
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter().position(|name| *name == month)? as i64 + 1;
    let day = day.parse::<i64>().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1601..=9999).contains(&year) || !(1..=month_days[month as usize - 1]).contains(&day) {
        return None;
    }
    let mut time = time.split(':').map(str::parse::<i64>);
    let (hour, minute, second) = (time.next()?.ok()?, time.next()?.ok()?, time.next()?.ok()?);
    if time.next().is_some() || !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    let year = year - i64::from(month <= 2);
    let month = if month > 2 { month - 3 } else { month + 9 };
    let days = 365 * year + year / 4 - year / 100 + year / 400 + (153 * month + 2) / 5 + day - 1 - 719468;
    let target = days * 86400 + hour * 3600 + minute * 60 + second;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(Duration::from_secs((target - now).max(0) as u64))
}


/// One deadline for the whole body. Bytes that arrive later do not move it.
fn collect_until(stream: &mut dyn Read, limit: usize, deadline: std::time::Instant) -> Result<Vec<u8>> {
    let mut response = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if response.len() > limit {
            return Err("Response exceeds the current memory budget".into());
        }
        let room = limit - response.len();
        if room == 0 {
            let mut extra = [0u8; 1];
            match wait_until(stream, &mut extra, deadline)? {
                0 => break,
                _ => return Err("Response exceeds the current memory budget".into()),
            }
        }
        let take = room.min(chunk.len());
        match wait_until(stream, &mut chunk[..take], deadline)? {
            0 => break,
            read => response.extend_from_slice(&chunk[..read]),
        }
    }
    Ok(response)
}

const RESPONSE_HARD_CAP: usize = 6 * 1024 * 1024;

fn response_budget(effect: &Json) -> usize {
    match number(effect, "limit") {
        value if value > 0 => (value as usize).min(RESPONSE_HARD_CAP),
        _ => 1024 * 1024,
    }
}

/// Set by the window when the user cancels; the transport stops at its next wait.
static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const CANCELLED: &str = "Cancelled";
const SILENCE_MS: u128 = 120_000;
const RESPONSE_SILENCE: &str = "the response was silent for two minutes";

fn operation_stopped(started: std::time::Instant) -> Option<&'static str> {
    if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
        return Some(CANCELLED);
    }
    if started.elapsed().as_millis() >= SILENCE_MS {
        return Some("the operation did not finish within two minutes");
    }
    None
}

/// Wait for the next read. The caller owns the current deadline.
fn wait_until(stream: &mut dyn Read, chunk: &mut [u8], deadline: std::time::Instant) -> Result<usize> {
    loop {
        if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        if std::time::Instant::now() >= deadline {
            return Err(RESPONSE_SILENCE.into());
        }
        match stream.read(chunk) {
            Ok(read) => return Ok(read),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {}
            Err(error) => return Err(format!("peer closed before finishing: {error}")),
        }
    }
}

fn core_call(request: Json) -> Result<Json> {
    let answer = parse(&envelope(&request.string()))?;
    if let Ok(error) = answer.get("error") {
        return Err(error.text().unwrap_or("the core rejected the request").to_string());
    }
    Ok(answer.get("value")?.clone())
}

fn stream_response_budget(effect: &Json) -> usize {
    match number(effect, "limit") {
        value if value > 0 => (value as usize).min(2 * 1024 * 1024),
        _ => 1024 * 1024,
    }
}

/// An incremental reply. The core owns the framing and the event format, as it
/// does on Android: the host hands it the bytes as they arrive and reports the
/// text decoded so far.
fn read_stream(stream: &mut dyn Read, effect: &Json, heard: &mut dyn FnMut(&str, u128), mut deadline: Instant) -> Result<Vec<u8>> {
    let provider = text(effect, "purpose") == "model";
    let protocol = if text(effect, "stream_protocol").is_empty() { "local".to_string() } else { text(effect, "stream_protocol") };
    let mut opening = obj(vec![("op", s("stream_open")), ("protocol", s(&protocol))]);
    if get(effect, "mcp_control") != &Null {
        put(&mut opening, "mcp_control", get(effect, "mcp_control").clone());
    }
    let id = core_call(opening)?;
    let limit = (!provider).then(|| stream_response_budget(effect));
    let started = std::time::Instant::now();
    let outcome = (|| -> Result<Vec<u8>> {
        let mut said = String::new();
        let mut published = 0usize;
        let mut previous = std::time::Instant::now();
        let mut total = 0usize;
        let mut chunk = [0u8; 8192];
        let mut ended = false;
        loop {
            let read = wait_until(stream, &mut chunk, deadline)?;
            if read == 0 {
                ended = true;
                break;
            }
            if provider { deadline = silence_deadline(); }
            if let Some(limit) = limit {
                total += read;
                if total > limit {
                    return Err("Stream exceeds the current response memory budget".into());
                }
            }
            let part = core_call(obj(vec![("op", s("stream_feed")), ("id", id.clone()), ("data", s(&network::b64(&chunk[..read])))]))?;
            let delta = get(&part, "delta").text().unwrap_or("");
            said.try_reserve(delta.len()).map_err(|_| "Insufficient memory to display the streamed reply")?;
            said.push_str(delta);
            if get(&part, "error") != &Null {
                return Err(text(&part, "error"));
            }
            // A server request inside the stream is answered on its own connection.
            // The acknowledgment goes back to the core, and this stream stays open.
            let mut complete = boolean(&part, "complete");
            if let Arr(items) = get(&part, "controls") {
                if !items.is_empty() {
                    complete = acknowledge_controls(items, &id, deadline)?;
                }
            }
            if said.len() != published && previous.elapsed().as_millis() >= 120 {
                heard(&said, started.elapsed().as_millis());
                published = said.len();
                previous = std::time::Instant::now();
            }
            if complete {
                break;
            }
        }
        let finished = core_call(obj(vec![("op", s("stream_finish")), ("id", id.clone()), ("eof", Bool(ended))]))?;
        network::unbase64(&text(&finished, "data"))
    })();
    if outcome.is_err() {
        let _ = core_call(obj(vec![("op", s("stream_close")), ("id", id)]));
    }
    outcome
}

/// Send each server control request and return its acknowledgment to the core.
/// The stream that carried the request is left open.
fn acknowledge_controls(controls: &[Json], id: &Json, deadline: std::time::Instant) -> Result<bool> {
    let mut complete = false;
    for control in controls {
        let (data, eof) = control_exchange(control, id, deadline)?;
        let ack = core_call(obj(vec![
            ("op", s("stream_control_ack")),
            ("id", id.clone()),
            ("ticket", get(control, "ticket").clone()),
            ("owner", s(&text(control, "owner"))),
            ("data", s(&data)),
            ("eof", Bool(eof)),
        ]))?;
        complete = boolean(&ack, "complete");
    }
    Ok(complete)
}

/// One control request, bound by the same deadline as the stream that produced it.
fn control_exchange(command: &Json, stream_id: &Json, deadline: std::time::Instant) -> Result<(String, bool)> {
    let host = text(command, "host");
    let port = number(command, "port");
    if host.is_empty() || port <= 0 || port > 65535 {
        return Err("MCP control request has no usable destination".into());
    }
    if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    if std::time::Instant::now() >= deadline {
        return Err("the response did not finish within two minutes".into());
    }
    let connect = host.replace('%', "");
    let display = if connect.contains(':') { format!("[{connect}]") } else { connect.clone() };
    let mut raw = connect_within(display.as_str(), port as u16, deadline)?;
    let _deadline = arm_write_deadline(&raw, deadline);
    raw.set_read_timeout(Some(std::time::Duration::from_millis(200))).ok();
    raw.set_write_timeout(Some(std::time::Duration::from_millis(200))).ok();
    if boolean(command, "tls") {
        let named = if text(command, "tls_host").is_empty() { connect } else { text(command, "tls_host") };
        let named = named.trim_start_matches('[').trim_end_matches(']').to_string();
        #[cfg(windows)]
        {
            let mut link = secure::Secure::open(raw, &named)?;
            link.set_read_timeout(Some(std::time::Duration::from_millis(200)));
            return control_roundtrip(&mut link, command, stream_id, deadline);
        }
        #[cfg(not(windows))]
        {
            let mut link = tls_client::Client::open(raw, &named)?;
            link.set_read_timeout(Some(std::time::Duration::from_millis(200)));
            return control_roundtrip(&mut link, command, stream_id, deadline);
        }
    }
    control_roundtrip(&mut raw, command, stream_id, deadline)
}

fn control_roundtrip<S: Read + Write>(stream: &mut S, command: &Json, stream_id: &Json, deadline: std::time::Instant) -> Result<(String, bool)> {
    write_all_until(stream, &request_wire(command)?, deadline)?;
    if let Err(error) = stream.flush() {
        if std::time::Instant::now() >= deadline {
            return Err("the response did not finish within two minutes".into());
        }
        return Err(format!("peer closed early: {error}"));
    }
    let mut accepted = Vec::new();
    let mut eof = false;
    let mut buf = [0u8; 1024];
    loop {
        if accepted.len() > 16384 {
            return Err("MCP control acknowledgment exceeds 16 KiB".into());
        }
        let read = wait_until(stream, &mut buf, deadline)?;
        if read == 0 {
            eof = true;
            break;
        }
        if accepted.len() + read > 16384 {
            return Err("MCP control acknowledgment exceeds 16 KiB".into());
        }
        accepted.extend_from_slice(&buf[..read]);
        let probe = core_call(obj(vec![
            ("op", s("stream_control_probe")),
            ("id", stream_id.clone()),
            ("ticket", get(command, "ticket").clone()),
            ("owner", s(&text(command, "owner"))),
            ("data", s(&network::b64(&accepted))),
        ]))?;
        if boolean(&probe, "ready") {
            break;
        }
    }
    Ok((network::b64(&accepted), eof))
}

/// Microphone bytes for the core's own utterance segmenter. The host starts the
/// recorder, hands over 20 ms frames, and returns the receipt the core minted
/// for the completed utterance; recognition stays a core decision.
struct Recorder {
    stream: Box<dyn Read>,
    header_checked: bool,
    pending: Vec<u8>,
}

impl Recorder {
    fn open() -> Result<Recorder> {
        if let Some(path) = std::env::var_os("RLLM_VOICE_SOURCE") {
            let file = std::fs::File::open(&path).map_err(|error| format!("Microphone source: {error}"))?;
            return Ok(Recorder { stream: Box::new(file), header_checked: false, pending: Vec::new() });
        }
        Ok(Recorder { stream: open_microphone()?, header_checked: false, pending: Vec::new() })
    }

    /// A recorder writing to a pipe may frame its output as WAVE; the core is fed
    /// raw little-endian samples only.
    fn fill(&mut self, want: usize, deadline: std::time::Instant) -> Result<Vec<u8>> {
        if !self.header_checked {
            self.header_checked = true;
            let mut head = [0u8; 4];
            if read_capture(&mut *self.stream, &mut head, deadline)? == 4 {
                if &head == b"RIFF" {
                    let mut ignored = [0u8; 40];
                    read_capture(&mut *self.stream, &mut ignored, deadline)?;
                } else {
                    self.pending.extend_from_slice(&head);
                }
            }
        }
        let mut bytes = std::mem::take(&mut self.pending);
        while bytes.len() < want {
            let mut chunk = [0u8; 512];
            let read = read_capture(&mut *self.stream, &mut chunk, deadline)?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        if bytes.len() > want {
            // The stream is read in larger blocks than the core's frame; the
            // remainder belongs to the next frame, not to the bit bucket.
            self.pending = bytes.split_off(want);
        }
        Ok(bytes)
    }
}

/// An empty capture queue is a wait, not the end of the recording. The deadline
/// does not restart when a later sample arrives.
fn read_capture(stream: &mut dyn Read, buffer: &mut [u8], deadline: std::time::Instant) -> Result<usize> {
    loop {
        if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        if std::time::Instant::now() >= deadline {
            return Err("the microphone stopped producing samples before the deadline".into());
        }
        match stream.read(buffer) {
            Ok(read) => return Ok(read),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(format!("Microphone stream: {error}")),
        }
    }
}

struct ActiveRecording {
    ticket: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<Result<Vec<u8>>>>,
}

static ACTIVE_RECORDING: std::sync::Mutex<Option<ActiveRecording>> = std::sync::Mutex::new(None);

fn recording_slot() -> std::sync::MutexGuard<'static, Option<ActiveRecording>> {
    ACTIVE_RECORDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn discard_media_recording() {
    let previous = recording_slot().take();
    if let Some(active) = previous {
        active.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn wav_pcm(pcm: &[u8]) -> Result<Vec<u8>> {
    if pcm.len() < 2 || pcm.len() % 2 != 0 || pcm.len() > 2 * 1024 * 1024 - 44 {
        return Err("The recording is empty".into());
    }
    let data_len = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&32000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    Ok(out)
}

fn start_media_recording(effect: &Json) -> Result<()> {
    discard_media_recording();
    let ticket = text(effect, "media_ticket");
    if ticket.is_empty() {
        return Err("Media recording has no owner".into());
    }
    if let Some(source) = std::env::var_os("RLLM_VOICE_SOURCE") {
        match path_kind(Path::new(&source)).map_err(|error| format!("Microphone source: {error}"))? {
            PathKind::Regular => {}
            _ => return Err("Microphone source is not a regular file".into()),
        }
    }
    let limit = number(effect, "capture_limit");
    let limit = if limit <= 0 { 2 * 1024 * 1024 } else { (limit as usize).min(2 * 1024 * 1024) };
    let sample_limit = limit.saturating_sub(44).max(2);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&stop);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        match Recorder::open() {
            Ok(recorder) => {
                let _ = ready_tx.send(Ok(()));
                capture_from(recorder, &flag, sample_limit)
            }
            Err(error) => {
                let message = error.clone();
                let _ = ready_tx.send(Err(error));
                Err(message)
            }
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => {
            *recording_slot() = Some(ActiveRecording { ticket, stop, worker: Some(worker) });
            Ok(())
        }
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = worker.join();
            Err("The recording did not start".into())
        }
    }
}

fn capture_from(mut recorder: Recorder, stop: &std::sync::atomic::AtomicBool, limit: usize) -> Result<Vec<u8>> {
    let mut pcm = Vec::new();
    let overall = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while pcm.len() + 2 <= limit && std::time::Instant::now() < overall && !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let room = limit - pcm.len();
        let want = room.min(640);
        match recorder.fill(want, std::time::Instant::now() + std::time::Duration::from_millis(200)) {
            Ok(frame) if frame.is_empty() => break,
            Ok(frame) => {
                let take = frame.len().min(room);
                pcm.extend_from_slice(&frame[..take]);
            }
            Err(error) if error.contains("before the deadline") => {
                if stop.load(std::sync::atomic::Ordering::Relaxed) || std::time::Instant::now() >= overall {
                    break;
                }
            }
            Err(error) => return Err(error),
        }
    }
    if pcm.len() < 2 {
        return Err("The recording is empty".into());
    }
    pcm.truncate(pcm.len() - (pcm.len() % 2));
    Ok(pcm)
}

fn finish_media_recording(ticket: &str) -> Result<Option<Vec<u8>>> {
    let mut slot = recording_slot();
    let matches = slot.as_ref().is_some_and(|active| active.ticket == ticket);
    if !matches {
        return Ok(None);
    }
    let mut active = slot.take().unwrap();
    drop(slot);
    active.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let worker = active.worker.take().ok_or_else(|| "The recording did not finish".to_string())?;
    match worker.join() {
        Ok(Ok(pcm)) => Ok(Some(wav_pcm(&pcm)?)),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("The recording did not finish".into()),
    }
}

fn chosen_media_file() -> Result<Option<String>> {
    if let Some(value) = std::env::var_os("RLLM_MEDIA_FILE") {
        if value.is_empty() {
            return Ok(None);
        }
        let path = PathBuf::from(&value);
        return match path_kind(&path).map_err(|error| format!("The file could not be opened: {error}"))? {
            PathKind::Regular => Ok(Some(path_text(&path))),
            PathKind::Missing => Err(format!("The file could not be opened: {}", std::io::Error::from(std::io::ErrorKind::NotFound))),
            PathKind::Other => Err("The selected file is not a regular file".into()),
        };
    }
    choose_file("Choose input file", Files::Attachment)
}

fn cancel_media_input(ticket: &str) {
    let _ = core_call(obj(vec![("op", s("media_input_cancel")), ("media_ticket", s(ticket))]));
}

fn stage_media_file(ticket: &str, path: &str) -> Result<String> {
    let bytes = read_path_limited(Path::new(path), 64 * 1024 * 1024)?;
    if bytes.is_empty() {
        return Err("Selected input is empty".into());
    }
    let name = Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "file.bin".into());
    let opened = core_call(obj(vec![
        ("op", s("media_input_open")),
        ("media_ticket", s(ticket)),
        ("name", s(&name)),
    ]))?;
    let stream = text(&opened, "ticket");
    let limit = number(&opened, "limit");
    if stream.is_empty() || limit <= 0 || bytes.len() > limit as usize {
        return Err("Input exceeds its storage bound".into());
    }
    for chunk in bytes.chunks(6144) {
        core_call(obj(vec![
            ("op", s("media_input_feed")),
            ("media_ticket", s(ticket)),
            ("ticket", s(&stream)),
            ("data", s(&network::b64(chunk))),
        ]))?;
    }
    let finished = core_call(obj(vec![
        ("op", s("media_input_finish")),
        ("media_ticket", s(ticket)),
        ("ticket", s(&stream)),
    ]))?;
    match finished {
        Str(receipt) if !receipt.is_empty() => Ok(receipt),
        _ => Err("Selected input did not finish".into()),
    }
}

fn media_failure(ticket: &str, error: String) -> (&'static str, Json) {
    cancel_media_input(ticket);
    ("media_input_failure", obj(vec![("media_ticket", s(ticket)), ("error", s(&error))]))
}

fn authorization_url(raw: &str) -> Result<String> {
    let url = raw.trim();
    if url.is_empty() || url.len() > 8192 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("Authorization address is not usable".into());
    }
    let rest = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        return Err("Authorization address is not usable".into());
    };
    if rest.is_empty() || rest.starts_with('/') {
        return Err("Authorization address is not usable".into());
    }
    Ok(url.to_string())
}

fn open_authorization(url: &str) -> Result<()> {
    let url = authorization_url(url)?;
    if let Some(path) = std::env::var_os("RLLM_AUTHORIZATION_CAPTURE") {
        if !path.is_empty() {
            std::fs::write(&path, url.as_bytes()).map_err(|error| format!("Authorization could not be opened: {error}"))?;
            return Ok(());
        }
    }
    launch_authorization(&url)
}

fn launch_authorization(url: &str) -> Result<()> {
    #[cfg(windows)]
    {
        return windows_open_address(url);
    }
    #[cfg(target_os = "linux")]
    {
        return linux_open_address(url);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_open_address(url);
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = url;
        Err("Authorization could not be opened on this host".into())
    }
}

#[cfg(windows)]
fn windows_open_address(url: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    let file: Vec<u16> = std::ffi::OsStr::new(url).encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(hwnd: isize, verb: *const u16, file: *const u16, params: *const u16, dir: *const u16, show: i32) -> isize;
    }
    let code = unsafe { ShellExecuteW(0, verb.as_ptr(), file.as_ptr(), null(), null(), 1) };
    if code > 32 {
        Ok(())
    } else {
        Err(format!("Authorization could not be opened ({code})"))
    }
}

fn open_microphone() -> Result<Box<dyn Read>> {
    #[cfg(windows)]
    {
        return WaveInput::open().map(|reader| reader as Box<dyn Read>);
    }
    #[cfg(target_os = "linux")]
    {
        match KernelCapture::open() {
            Ok(reader) => return Ok(Box::new(reader)),
            Err(kernel) => match SessionCapture::open() {
                Ok(reader) => return Ok(Box::new(reader)),
                Err(session) => return Err(format!("{kernel}; {session}")),
            },
        }
    }
    #[cfg(target_os = "macos")]
    {
        return QueueInput::open().map(|reader| reader as Box<dyn Read>);
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    Err("no microphone is available on this host".into())
}

#[cfg(windows)]
struct WaveInput {
    handle: usize,
    buffers: Vec<Vec<u8>>,
    headers: Vec<WaveHdr>,
    pending: std::sync::Mutex<std::collections::VecDeque<u8>>,
    event: isize,
}

#[cfg(windows)]
#[repr(C)]
struct WaveHdr {
    data: *mut u8,
    length: u32,
    recorded: u32,
    user: usize,
    flags: u32,
    loops: u32,
    next: *mut WaveHdr,
    reserved: usize,
}

#[cfg(windows)]
const _: () = assert!(std::mem::size_of::<WaveHdr>() == std::mem::size_of::<usize>() * 4 + 16);

#[cfg(windows)]
fn close_wave_event(event: isize) {
    if event == 0 {
        return;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(handle: isize) -> i32;
    }
    unsafe { CloseHandle(event); }
}

#[cfg(windows)]
fn reject_failed_wave_open(event: isize, status: u32) -> Result<()> {
    if status != 0 {
        close_wave_event(event);
        return Err(format!("the microphone could not be opened ({status})"));
    }
    Ok(())
}

#[cfg(windows)]
impl WaveInput {
    fn open() -> Result<Box<WaveInput>> {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn waveInGetNumDevs() -> u32;
            fn waveInOpen(handle: *mut usize, device: u32, format: *const WaveFormat, callback: isize, instance: usize, flags: u32) -> u32;
            fn waveInPrepareHeader(handle: usize, header: *mut WaveHdr, size: u32) -> u32;
            fn waveInAddBuffer(handle: usize, header: *mut WaveHdr, size: u32) -> u32;
            fn waveInStart(handle: usize) -> u32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateEventW(attrs: *mut std::ffi::c_void, manual: i32, initial: i32, name: *const u16) -> isize;
        }
        #[repr(C)]
        struct WaveFormat {
            format: u16,
            channels: u16,
            rate: u32,
            average: u32,
            align: u16,
            bits: u16,
            size: u16,
        }
        if unsafe { waveInGetNumDevs() } == 0 {
            return Err("no microphone is available on this host".into());
        }
        let event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
        if event == 0 {
            return Err("the microphone event could not be created".into());
        }
        let format = WaveFormat { format: 1, channels: 1, rate: 16000, average: 32000, align: 2, bits: 16, size: 0 };
        let mut handle = 0usize;
        let status = unsafe { waveInOpen(&mut handle, u32::MAX, &format, event, 0, 0x00050000) };
        reject_failed_wave_open(event, status)?;
        let mut input = Box::new(WaveInput {
            handle,
            buffers: (0..8).map(|_| vec![0u8; 640]).collect(),
            headers: (0..8).map(|_| WaveHdr {
                data: std::ptr::null_mut(),
                length: 640,
                recorded: 0,
                user: 0,
                flags: 0,
                loops: 0,
                next: std::ptr::null_mut(),
                reserved: 0,
            }).collect(),
            pending: std::sync::Mutex::new(std::collections::VecDeque::new()),
            event,
        });
        let pointers: Vec<(*mut u8, u32)> = input.buffers.iter_mut().map(|buffer| (buffer.as_mut_ptr(), buffer.len() as u32)).collect();
        for (index, (data, length)) in pointers.into_iter().enumerate() {
            input.headers[index].data = data;
            input.headers[index].length = length;
        }
        let status = unsafe {
            let mut failed = 0u32;
            for header in &mut input.headers {
                let header = header as *mut WaveHdr;
                let prepared = waveInPrepareHeader(input.handle, header, std::mem::size_of::<WaveHdr>() as u32);
                if prepared != 0 {
                    failed = prepared;
                    break;
                }
                let queued = waveInAddBuffer(input.handle, header, std::mem::size_of::<WaveHdr>() as u32);
                if queued != 0 {
                    failed = queued;
                    break;
                }
            }
            if failed == 0 { waveInStart(input.handle) } else { failed }
        };
        if status != 0 {
            return Err(format!("the microphone could not be started ({status})"));
        }
        Ok(input)
    }
}

#[cfg(windows)]
impl Read for WaveInput {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn WaitForSingleObject(handle: isize, millis: u32) -> u32;
        }
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn waveInAddBuffer(handle: usize, header: *mut WaveHdr, size: u32) -> u32;
        }
        loop {
            for header in &mut self.headers {
                if header.flags & 0x00000001 != 0 && header.recorded > 0 {
                    let count = header.recorded as usize;
                    if let Ok(mut pending) = self.pending.lock() {
                        pending.extend(header.data_slice(count));
                    }
                    header.recorded = 0;
                    header.flags &= !1;
                    unsafe { waveInAddBuffer(self.handle, header as *mut WaveHdr, std::mem::size_of::<WaveHdr>() as u32); }
                }
            }
            if let Ok(mut pending) = self.pending.lock() {
                if !pending.is_empty() {
                    let n = pending.len().min(buf.len());
                    for slot in &mut buf[..n] {
                        *slot = pending.pop_front().unwrap_or(0);
                    }
                    return Ok(n);
                }
            }
            match unsafe { WaitForSingleObject(self.event, 200) } {
                0x0000_0102 => return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "microphone waiting")),
                0xFFFFFFFF => return Err(std::io::Error::other("the microphone wait failed")),
                _ => {}
            }
        }
    }
}

#[cfg(windows)]
impl WaveHdr {
    fn data_slice(&self, count: usize) -> Vec<u8> {
        if self.data.is_null() || count == 0 {
            return Vec::new();
        }
        unsafe { std::slice::from_raw_parts(self.data, count.min(self.length as usize)).to_vec() }
    }
}

#[cfg(windows)]
impl Drop for WaveInput {
    fn drop(&mut self) {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn waveInReset(handle: usize) -> u32;
            fn waveInUnprepareHeader(handle: usize, header: *mut WaveHdr, size: u32) -> u32;
            fn waveInClose(handle: usize) -> u32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CloseHandle(handle: isize) -> i32;
        }
        unsafe {
            waveInReset(self.handle);
            for header in &mut self.headers {
                waveInUnprepareHeader(self.handle, header as *mut WaveHdr, std::mem::size_of::<WaveHdr>() as u32);
            }
            waveInClose(self.handle);
            if self.event != 0 {
                CloseHandle(self.event);
            }
        }
    }
}

#[cfg(target_os = "linux")]
struct KernelCapture {
    file: std::fs::File,
}

#[cfg(target_os = "linux")]
impl KernelCapture {
    fn open() -> Result<KernelCapture> {
        for card in 0..8 {
            for device in 0..4 {
                let path = format!("/dev/snd/pcmC{card}D{device}c");
                if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
                    if let Ok(capture) = KernelCapture::configure(file) {
                        return Ok(capture);
                    }
                }
            }
        }
        Err("no kernel capture device accepted 16 kHz mono".into())
    }

    fn configure(file: std::fs::File) -> Result<KernelCapture> {
        use std::os::fd::AsRawFd;
        let mut hw = HwParams::request();
        ioctl_rw(file.as_raw_fd(), b'A', 0x10, &mut hw)?;
        ioctl_rw(file.as_raw_fd(), b'A', 0x11, &mut hw)?;
        let mut sw = SwParams::request();
        ioctl_rw(file.as_raw_fd(), b'A', 0x13, &mut sw)?;
        ioctl_none(file.as_raw_fd(), b'A', 0x40)?;
        ioctl_none(file.as_raw_fd(), b'A', 0x42)?;
        Ok(KernelCapture { file })
    }
}

#[cfg(target_os = "linux")]
impl Read for KernelCapture {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::os::fd::AsRawFd;
        #[repr(C)]
        struct PollFd {
            fd: i32,
            events: i16,
            revents: i16,
        }
        unsafe extern "C" {
            fn poll(fds: *mut PollFd, count: usize, timeout: i32) -> i32;
        }
        let mut wait = PollFd { fd: self.file.as_raw_fd(), events: 1, revents: 0 };
        let waited = unsafe { poll(&mut wait, 1, 200) };
        if waited == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "microphone waiting"));
        }
        if waited < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let frames = (buf.len() / 2).max(1);
        let mut transfer = Xfer { result: 0, data: buf.as_mut_ptr(), frames: frames as u64 };
        let request = ioc(1, b'A' as u32, 0x51, std::mem::size_of::<Xfer>() as u32);
        let rc = unsafe { ioctl(self.file.as_raw_fd(), request, &mut transfer) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let got = if transfer.result > 0 { transfer.result as usize } else { rc as usize };
        Ok(got.saturating_mul(2).min(buf.len()))
    }
}

#[cfg(target_os = "linux")]
impl Drop for KernelCapture {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        let _ = ioctl_none(self.file.as_raw_fd(), b'A', 0x43);
    }
}

#[cfg(target_os = "linux")]
fn ioctl_rw<T>(fd: i32, kind: u8, number: u32, value: &mut T) -> Result<()> {
    let request = ioc(3, kind as u32, number, std::mem::size_of::<T>() as u32);
    let rc = unsafe { ioctl(fd, request, value) };
    if rc < 0 {
        Err(format!("capture device setup failed ({})", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn ioctl_none(fd: i32, kind: u8, number: u32) -> Result<()> {
    let request = ioc(0, kind as u32, number, 0);
    let rc = unsafe { ioctl(fd, request, std::ptr::null_mut::<u8>()) };
    if rc < 0 {
        Err(format!("capture device setup failed ({})", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn ioc(dir: u32, kind: u32, number: u32, size: u32) -> u64 {
    ((dir as u64) << 30) | ((size as u64) << 16) | ((kind as u64) << 8) | number as u64
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
#[repr(C)]
struct Interval {
    min: u32,
    max: u32,
    flags: u32,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
#[repr(C)]
struct Mask {
    bits: [u32; 8],
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct HwParams {
    flags: u32,
    masks: [Mask; 3],
    reserved_masks: [Mask; 5],
    intervals: [Interval; 12],
    reserved_intervals: [Interval; 9],
    rmask: u32,
    cmask: u32,
    info: u32,
    msbits: u32,
    rate_num: u32,
    rate_den: u32,
    fifo_size: u64,
    reserved: [u8; 64],
}

#[cfg(target_os = "linux")]
impl HwParams {
    fn request() -> HwParams {
        let mut params = HwParams {
            flags: 0,
            masks: [Mask { bits: [0; 8] }; 3],
            reserved_masks: [Mask { bits: [0; 8] }; 5],
            intervals: [Interval { min: 0, max: u32::MAX, flags: 0 }; 12],
            reserved_intervals: [Interval { min: 0, max: u32::MAX, flags: 0 }; 9],
            rmask: 0,
            cmask: 0,
            info: 0,
            msbits: 0,
            rate_num: 0,
            rate_den: 0,
            fifo_size: 0,
            reserved: [0; 64],
        };
        params.masks[0].bits[0] = 1 << 3;
        params.masks[1].bits[0] = 1 << 2;
        params.masks[2].bits[0] = 1;
        let exact = |value: u32| Interval { min: value, max: value, flags: 4 };
        params.intervals[2] = exact(1);
        params.intervals[3] = exact(16000);
        params.intervals[5] = exact(320);
        params.intervals[7] = exact(4);
        params.rmask = 0x000f_ffff;
        params
    }

    /// Playback uses the session rate. Capture keeps the 16 kHz request above.
    fn playback(rate: u32) -> HwParams {
        let mut params = HwParams::request();
        let period = (rate / 50).max(1);
        let exact = |value: u32| Interval { min: value, max: value, flags: 4 };
        params.intervals[3] = exact(rate);
        params.intervals[5] = exact(period);
        params
    }
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct SwParams {
    tstamp_mode: i32,
    period_step: u32,
    sleep_min: u32,
    pad: u32,
    avail_min: u64,
    xfer_align: u64,
    start_threshold: u64,
    stop_threshold: u64,
    silence_threshold: u64,
    silence_size: u64,
    boundary: u64,
    proto: u32,
    tstamp_type: u32,
    reserved: [u8; 56],
}

#[cfg(target_os = "linux")]
impl SwParams {
    fn request() -> SwParams {
        SwParams {
            tstamp_mode: 0,
            period_step: 1,
            sleep_min: 0,
            pad: 0,
            avail_min: 320,
            xfer_align: 1,
            start_threshold: 1,
            stop_threshold: 1280,
            silence_threshold: 0,
            silence_size: 0,
            boundary: 0,
            proto: 2,
            tstamp_type: 0,
            reserved: [0; 56],
        }
    }
}

/// Kernel playback at the rate the speech session returned. Capture devices stay on the path above.
#[cfg(target_os = "linux")]
struct KernelPlayback {
    file: std::fs::File,
}

#[cfg(target_os = "linux")]
impl KernelPlayback {
    fn open(rate: u32) -> Result<KernelPlayback> {
        for card in 0..8 {
            for device in 0..4 {
                let path = format!("/dev/snd/pcmC{card}D{device}p");
                if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
                    if let Ok(playback) = KernelPlayback::configure(file, rate) {
                        return Ok(playback);
                    }
                }
            }
        }
        Err("no speaker is available on this host".into())
    }

    fn configure(file: std::fs::File, rate: u32) -> Result<KernelPlayback> {
        use std::os::fd::AsRawFd;
        let mut hw = HwParams::playback(rate);
        ioctl_rw(file.as_raw_fd(), b'A', 0x10, &mut hw)?;
        ioctl_rw(file.as_raw_fd(), b'A', 0x11, &mut hw)?;
        let period = (rate / 50).max(1) as u64;
        let mut sw = SwParams::request();
        sw.avail_min = period;
        sw.start_threshold = 1;
        sw.stop_threshold = 1 << 30;
        ioctl_rw(file.as_raw_fd(), b'A', 0x13, &mut sw)?;
        ioctl_none(file.as_raw_fd(), b'A', 0x40)?;
        ioctl_none(file.as_raw_fd(), b'A', 0x42)?;
        Ok(KernelPlayback { file })
    }

    fn write(&mut self, pcm: &[u8]) -> Result<()> {
        use std::os::fd::AsRawFd;
        let mut rest = pcm;
        while rest.len() >= 2 {
            let frames = (rest.len() / 2) as u64;
            let mut transfer = Xfer { result: 0, data: rest.as_ptr() as *mut u8, frames };
            let request = ioc(1, b'A' as u32, 0x50, std::mem::size_of::<Xfer>() as u32);
            let rc = unsafe { ioctl(self.file.as_raw_fd(), request, &mut transfer) };
            if rc < 0 {
                return Err(format!("speaker write failed ({})", std::io::Error::last_os_error()));
            }
            let got = if transfer.result > 0 { transfer.result as usize } else { rc as usize };
            if got == 0 {
                return Err("the speaker accepted no samples".into());
            }
            let bytes = got.saturating_mul(2).min(rest.len());
            rest = &rest[bytes..];
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for KernelPlayback {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        let _ = ioctl_none(self.file.as_raw_fd(), b'A', 0x43);
    }
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct Xfer {
    result: i64,
    data: *mut u8,
    frames: u64,
}

/// Session sound frames. The core object hello is method 1 and the core version is 4.
struct SoundFrame {
    id: u32,
    opcode: u32,
    seq: i32,
    body: Vec<u8>,
    mem: Vec<Vec<u8>>,
}

struct SessionNegotiator {
    seq: i32,
    next_id: u32,
    registry: u32,
    capture: u32,
    saw_format: bool,
    samples: Vec<u8>,
    held: Vec<Vec<u8>>,
}

const CORE_HELLO: u32 = 1;
const CORE_SYNC: u32 = 2;
const CORE_PONG: u32 = 3;
const CORE_GET_REGISTRY: u32 = 5;
const CORE_CREATE_OBJECT: u32 = 6;
const CLIENT_UPDATE_PROPERTIES: u32 = 2;
const CORE_EVENT_DONE: u32 = 1;
const CORE_EVENT_PING: u32 = 2;
const CORE_EVENT_ERROR: u32 = 3;
const NODE_EVENT_SET_PARAM: u32 = 1;
const NODE_EVENT_PORT_BUFFERS: u32 = 8;
const CORE_VERSION: u32 = 4;
const POD_OBJECT: u32 = 15;
const POD_CHOICE: u32 = 19;
const SESSION_RATE: u32 = 16000;
const REGISTRY_VERSION: u32 = 3;
const CLIENT_NODE_VERSION: u32 = 6;
const POD_BOOL: u32 = 2;
const POD_INT: u32 = 4;
const POD_STRING: u32 = 8;
const POD_BYTES: u32 = 9;
const POD_STRUCT: u32 = 14;

fn pod_align(buf: &mut Vec<u8>) {
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
}

fn pod_int(value: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&POD_INT.to_le_bytes());
    out.extend_from_slice(&value.to_le_bytes());
    pod_align(&mut out);
    out
}

fn pod_bool(value: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&POD_BOOL.to_le_bytes());
    out.extend_from_slice(&(u32::from(value)).to_le_bytes());
    pod_align(&mut out);
    out
}

fn pod_string(value: &str) -> Vec<u8> {
    let mut raw = value.as_bytes().to_vec();
    raw.push(0);
    let mut out = Vec::new();
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out.extend_from_slice(&POD_STRING.to_le_bytes());
    out.extend(raw);
    pod_align(&mut out);
    out
}

fn pod_struct(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for part in parts {
        body.extend(part);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&POD_STRUCT.to_le_bytes());
    out.extend(body);
    pod_align(&mut out);
    out
}

fn pod_pairs(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut parts = Vec::with_capacity(pairs.len().saturating_mul(2).saturating_add(1));
    parts.push(pod_int(pairs.len() as u32));
    for (key, value) in pairs {
        parts.push(pod_string(key));
        parts.push(pod_string(value));
    }
    pod_struct(&parts)
}

/// Core hello is one struct holding the client version. The core version stays 4.
fn session_hello() -> SoundFrame {
    SoundFrame { id: 0, opcode: CORE_HELLO, seq: 0, body: pod_struct(&[pod_int(CORE_VERSION)]), mem: Vec::new() }
}

/// The client object is id 1. Its properties follow hello as their own message.
fn session_client_properties() -> SoundFrame {
    let pid = std::process::id().to_string();
    let props = pod_pairs(&[
        ("application.name", "RLLM"),
        ("application.process.id", &pid),
        ("media.category", "Capture"),
        ("audio.rate", "16000"),
        ("audio.channels", "1"),
        ("audio.format", "S16"),
    ]);
    SoundFrame { id: 1, opcode: CLIENT_UPDATE_PROPERTIES, seq: 0, body: pod_struct(&[props]), mem: Vec::new() }
}

/// Sync is what makes the service answer with Done. id and seq travel in one struct.
fn session_sync(id: u32, seq: i32) -> SoundFrame {
    SoundFrame { id: 0, opcode: CORE_SYNC, seq, body: pod_struct(&[pod_int(id), pod_int(seq as u32)]), mem: Vec::new() }
}

/// Hello, then client properties, then sync. Registry waits until Done.
fn session_start() -> Vec<SoundFrame> {
    vec![session_hello(), session_client_properties(), session_sync(0, 0)]
}

fn pod_header(body: &[u8], at: usize) -> Option<(usize, u32, usize)> {
    let header = at.checked_add(8)?;
    if header > body.len() {
        return None;
    }
    let size = u32::from_le_bytes(body[at..at + 4].try_into().ok()?) as usize;
    let kind = u32::from_le_bytes(body[at + 4..at + 8].try_into().ok()?);
    let end = header.checked_add(size)?;
    if end > body.len() {
        return None;
    }
    Some((size, kind, end))
}

fn pod_step(at: usize, size: usize) -> Option<usize> {
    let span = size.checked_add(8)?.checked_add(7)? & !7;
    if span == 0 {
        return None;
    }
    let next = at.checked_add(span)?;
    if next <= at { None } else { Some(next) }
}

fn session_strings(body: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0usize;
    let mut seen = 0usize;
    while at + 8 <= body.len() && seen < 4096 {
        seen += 1;
        let Some((size, kind, end)) = pod_header(body, at) else { break };
        if size > 1024 * 1024 {
            break;
        }
        if kind == POD_STRING && size > 0 && end > at + 8 {
            if let Ok(text) = std::str::from_utf8(&body[at + 8..end - 1]) {
                found.push(text.to_string());
            }
        }
        let Some(next) = pod_step(at, size) else { break };
        at = next;
    }
    found
}

fn push_object_values(body: &[u8], mut at: usize, end: usize, depth: usize, stack: &mut Vec<(usize, usize, usize)>) {
    let mut seen = 0usize;
    while at + 8 <= end && seen < 4096 {
        seen += 1;
        let value = at + 8;
        let Some((_, _, value_end)) = pod_header(body, value) else { return };
        if value_end > end {
            return;
        }
        stack.push((value, value_end, depth));
        let Some(next) = pod_step(value, value_end - value - 8) else { return };
        if next > end {
            return;
        }
        at = next;
    }
}

/// The capture rate lives in a nested object property, a choice, or a bare integer.
/// A size that does not fit, or a nest past the depth budget, is not a negotiated rate.
fn session_has_rate(body: &[u8]) -> bool {
    const MAX_DEPTH: usize = 8;
    const MAX_NODES: usize = 4096;
    let mut stack = vec![(0usize, body.len(), 0usize)];
    let mut nodes = 0usize;
    while let Some((start, end, depth)) = stack.pop() {
        if depth > MAX_DEPTH || start > end || end > body.len() {
            continue;
        }
        let mut at = start;
        while at + 8 <= end {
            nodes += 1;
            if nodes > MAX_NODES {
                return false;
            }
            let Some((size, kind, payload_end)) = pod_header(body, at) else { return false };
            if payload_end > end {
                return false;
            }
            if kind == POD_INT && size >= 4 {
                let value = u32::from_le_bytes(body[at + 8..at + 12].try_into().unwrap_or([0; 4]));
                if value == SESSION_RATE {
                    return true;
                }
            }
            if depth < MAX_DEPTH && kind == POD_STRUCT && size > 0 {
                stack.push((at + 8, payload_end, depth + 1));
            }
            if depth < MAX_DEPTH && kind == POD_CHOICE && size >= 8 && at + 16 < payload_end {
                stack.push((at + 16, payload_end, depth + 1));
            }
            if depth < MAX_DEPTH && kind == POD_OBJECT && size >= 8 && at + 16 <= payload_end {
                push_object_values(body, at + 16, payload_end, depth + 1, &mut stack);
            }
            let Some(next) = pod_step(at, size) else { return false };
            at = next;
        }
    }
    false
}

impl SessionNegotiator {
    fn new() -> SessionNegotiator {
        // Id 1 is the client object created with the socket. Later objects start at 2.
        SessionNegotiator { seq: 1, next_id: 2, registry: 0, capture: 0, saw_format: false, samples: Vec::new(), held: Vec::new() }
    }

    fn alloc(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn push(&mut self, frame: &SoundFrame) -> Result<Vec<SoundFrame>> {
        if frame.body.len() > 1024 * 1024 || frame.mem.iter().map(Vec::len).fold(0usize, usize::saturating_add) > 1024 * 1024 {
            return Err("session sound message exceeds the memory budget".into());
        }
        let mut out = Vec::new();
        if frame.id == 0 && frame.opcode == CORE_EVENT_ERROR {
            let detail = session_strings(&frame.body).into_iter().next().unwrap_or_else(|| "session sound service reported an error".into());
            return Err(detail);
        }
        if frame.id == 0 && frame.opcode == CORE_EVENT_PING {
            let body = pod_struct(&[pod_int(0), pod_int(frame.seq as u32)]);
            out.push(SoundFrame { id: 0, opcode: CORE_PONG, seq: frame.seq, body, mem: Vec::new() });
        }
        if frame.id == 0 && frame.opcode == CORE_EVENT_DONE && self.registry == 0 {
            self.registry = self.alloc();
            let body = pod_struct(&[pod_int(REGISTRY_VERSION), pod_int(self.registry)]);
            out.push(SoundFrame { id: 0, opcode: CORE_GET_REGISTRY, seq: self.seq, body, mem: Vec::new() });
            self.seq = self.seq.saturating_add(1);
            out.push(session_sync(0, self.seq));
            self.seq = self.seq.saturating_add(1);
        }
        if self.registry != 0 && frame.id == self.registry && frame.opcode == 0 && self.capture == 0 {
            let names = session_strings(&frame.body);
            if names.iter().any(|name| name == "Audio/Source" || name == "Audio/Duplex" || name == "Stream/Output/Audio") {
                self.capture = self.alloc();
                let props = pod_struct(&[
                    pod_int(5),
                    pod_string("media.type"),
                    pod_string("Audio"),
                    pod_string("media.category"),
                    pod_string("Capture"),
                    pod_string("audio.rate"),
                    pod_string("16000"),
                    pod_string("audio.channels"),
                    pod_string("1"),
                    pod_string("audio.format"),
                    pod_string("S16"),
                ]);
                let body = pod_struct(&[
                    pod_string("client-node"),
                    pod_string("PipeWire:Interface:ClientNode"),
                    pod_int(CLIENT_NODE_VERSION),
                    props,
                    pod_int(self.capture),
                ]);
                out.push(SoundFrame { id: 0, opcode: CORE_CREATE_OBJECT, seq: self.seq, body, mem: Vec::new() });
                self.seq = self.seq.saturating_add(1);
                out.push(SoundFrame { id: self.capture, opcode: 4, seq: self.seq, body: pod_bool(true), mem: Vec::new() });
                self.seq = self.seq.saturating_add(1);
            }
        }
        if self.capture != 0 && frame.id == self.capture && frame.opcode == NODE_EVENT_SET_PARAM && session_has_rate(&frame.body) {
            self.saw_format = true;
        }
        if frame.mem.iter().any(|block| block.len() >= 2 && block.len() % 2 == 0) {
            self.held.extend(frame.mem.iter().cloned());
        }
        if self.capture != 0 && self.saw_format && (frame.id == self.capture && frame.opcode == NODE_EVENT_PORT_BUFFERS || frame.opcode == 6 || !self.held.is_empty()) {
            for block in self.held.drain(..) {
                if block.len() >= 2 && block.len() % 2 == 0 && self.samples.len() < 65536 {
                    let room = 65536 - self.samples.len();
                    self.samples.extend(block.into_iter().take(room));
                }
            }
            let mut at = 0usize;
            while at + 8 <= frame.body.len() && self.samples.len() < 65536 {
                let Some((size, kind, end)) = pod_header(&frame.body, at) else { break };
                if size > 1024 * 1024 || end > frame.body.len() {
                    break;
                }
                if kind == POD_BYTES && size >= 2 && size % 2 == 0 {
                    let room = 65536 - self.samples.len();
                    self.samples.extend(frame.body[at + 8..end].iter().take(room).copied());
                }
                let Some(next) = pod_step(at, size) else { break };
                at = next;
            }
        }
        Ok(out)
    }

    fn ready(&self) -> bool {
        self.saw_format && self.samples.len() >= 2 && self.samples.len() % 2 == 0
    }
}

/// Session sound service capture. The socket speaks the native session protocol;
/// samples are returned only after a capture buffer is negotiated.
#[cfg(target_os = "linux")]
struct SessionCapture {
    stream: std::os::unix::net::UnixStream,
    pending: Vec<u8>,
    buffered: Vec<u8>,
    incoming: Vec<i32>,
    nego: SessionNegotiator,
}

#[cfg(target_os = "linux")]
impl SessionCapture {
    fn open() -> Result<SessionCapture> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", host_fs::effective_uid())));
        let socket = runtime.join("pipewire-0");
        let stream = std::os::unix::net::UnixStream::connect(&socket).map_err(|error| format!("session sound socket: {error}"))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        stream.set_read_timeout(Some(std::time::Duration::from_millis(800))).ok();
        stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).ok();
        let mut capture = SessionCapture { stream, pending: Vec::new(), buffered: Vec::new(), incoming: Vec::new(), nego: SessionNegotiator::new() };
        for frame in session_start() {
            capture.send(frame.id, frame.opcode, frame.seq, &frame.body)?;
        }
        let mut messages = 0u32;
        while !capture.nego.ready() {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() || messages > 64 {
                return Err("the session sound service did not offer a 16 kHz mono capture buffer".into());
            }
            capture.stream.set_read_timeout(Some(left.min(std::time::Duration::from_millis(800)))).ok();
            capture.stream.set_write_timeout(Some(left)).ok();
            let frame = match capture.read_frame() {
                Ok(frame) => frame,
                Err(error) if error.contains("timed out") => continue,
                Err(error) => return Err(error),
            };
            messages += 1;
            for reply in capture.nego.push(&frame)? {
                capture.send(reply.id, reply.opcode, reply.seq, &reply.body)?;
            }
        }
        capture.pending = std::mem::take(&mut capture.nego.samples);
        Ok(capture)
    }

    fn read_frame(&mut self) -> Result<SoundFrame> {
        use std::os::fd::AsRawFd;
        while self.buffered.len() < 20 {
            self.pull()?;
        }
        let size = u32::from_le_bytes(self.buffered[8..12].try_into().unwrap()) as usize;
        let fds = u32::from_le_bytes(self.buffered[16..20].try_into().unwrap()) as usize;
        if size > 1024 * 1024 || fds > 16 {
            return Err("session sound message exceeds the memory budget".into());
        }
        while self.buffered.len() < 20 + size {
            self.pull()?;
        }
        let id = u32::from_le_bytes(self.buffered[0..4].try_into().unwrap());
        let opcode = u32::from_le_bytes(self.buffered[4..8].try_into().unwrap());
        let seq = i32::from_le_bytes(self.buffered[12..16].try_into().unwrap());
        let body = self.buffered[20..20 + size].to_vec();
        self.buffered.drain(..20 + size);
        Ok(SoundFrame { id, opcode, seq, body, mem: self.take_mem() })
    }

    fn pull(&mut self) -> Result<()> {
        use std::os::fd::AsRawFd;
        #[repr(C)]
        struct IoVec {
            base: *mut u8,
            len: usize,
        }
        #[repr(C)]
        struct MsgHdr {
            name: *mut u8,
            namelen: u32,
            pad_name: u32,
            iov: *mut IoVec,
            iovlen: usize,
            control: *mut u8,
            controllen: usize,
            flags: i32,
            pad_flags: i32,
        }
        #[repr(C)]
        struct CMsg {
            len: usize,
            level: i32,
            kind: i32,
        }
        let mut bytes = [0u8; 8192];
        let mut control = [0u8; 256];
        let mut iov = IoVec { base: bytes.as_mut_ptr(), len: bytes.len() };
        let mut header = MsgHdr {
            name: std::ptr::null_mut(),
            namelen: 0,
            pad_name: 0,
            iov: &mut iov,
            iovlen: 1,
            control: control.as_mut_ptr(),
            controllen: control.len(),
            flags: 0,
            pad_flags: 0,
        };
        unsafe extern "C" {
            fn recvmsg(fd: i32, header: *mut MsgHdr, flags: i32) -> isize;
        }
        let got = unsafe { recvmsg(self.stream.as_raw_fd(), &mut header, 0) };
        if got < 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) {
                return Err("session sound socket timed out".into());
            }
            return Err(format!("session sound socket: {error}"));
        }
        if got == 0 {
            return Err("session sound socket closed".into());
        }
        self.buffered.extend_from_slice(&bytes[..got as usize]);
        let mut at = 0usize;
        let limit = header.controllen.min(control.len());
        while at.saturating_add(std::mem::size_of::<CMsg>()) <= limit {
            let len = usize::from_le_bytes(control[at..at + std::mem::size_of::<usize>()].try_into().unwrap_or([0; std::mem::size_of::<usize>()]));
            let Some(end) = at.checked_add(len) else { break };
            if len < std::mem::size_of::<CMsg>() || end > limit {
                break;
            }
            let kind_at = at.saturating_add(std::mem::size_of::<usize>()).saturating_add(4);
            if kind_at.saturating_add(4) > limit {
                break;
            }
            let kind = i32::from_le_bytes(control[kind_at..kind_at + 4].try_into().unwrap_or([0; 4]));
            if kind == 1 {
                let Some(mut cursor) = at.checked_add(std::mem::size_of::<CMsg>()) else { break };
                while cursor.saturating_add(4) <= end && self.incoming.len() < 16 {
                    let fd = i32::from_le_bytes(control[cursor..cursor + 4].try_into().unwrap_or([255; 4]));
                    cursor += 4;
                    if fd >= 0 {
                        self.incoming.push(fd);
                    }
                }
            }
            let align = std::mem::size_of::<usize>();
            let Some(step) = len.checked_add(align - 1).map(|value| value & !(align - 1)) else { break };
            if step == 0 {
                break;
            }
            let Some(next) = at.checked_add(step) else { break };
            if next <= at {
                break;
            }
            at = next;
        }
        Ok(())
    }

    fn take_mem(&mut self) -> Vec<Vec<u8>> {
        unsafe extern "C" {
            fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
            fn close(fd: i32) -> i32;
        }
        let mut mem = Vec::new();
        for fd in self.incoming.drain(..) {
            let mut bytes = [0u8; 4096];
            let got = unsafe { read(fd, bytes.as_mut_ptr(), bytes.len()) };
            unsafe { close(fd); }
            if got > 1 {
                mem.push(bytes[..got as usize].to_vec());
            }
        }
        mem
    }

    fn send(&mut self, id: u32, opcode: u32, seq: i32, body: &[u8]) -> Result<()> {
        if body.len() > 1024 * 1024 {
            return Err("session sound message exceeds the memory budget".into());
        }
        let mut header = [0u8; 20];
        header[0..4].copy_from_slice(&id.to_le_bytes());
        header[4..8].copy_from_slice(&opcode.to_le_bytes());
        header[8..12].copy_from_slice(&(body.len() as u32).to_le_bytes());
        header[12..16].copy_from_slice(&seq.to_le_bytes());
        self.stream.write_all(&header).and_then(|_| self.stream.write_all(body)).map_err(|error| format!("session sound socket: {error}"))
    }
}

#[cfg(target_os = "linux")]
impl Read for SessionCapture {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.read_frame() {
                Ok(frame) => {
                    if let Ok(replies) = self.nego.push(&frame) {
                        for reply in replies {
                            let _ = self.send(reply.id, reply.opcode, reply.seq, &reply.body);
                        }
                    }
                    self.pending.append(&mut self.nego.samples);
                }
                Err(error) if error.contains("timed out") => {
                    return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "microphone waiting"));
                }
                Err(error) => return Err(std::io::Error::other(error)),
            }
        }
        if self.pending.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "microphone waiting"));
        }
        let n = self.pending.len().min(buf.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

/// Published capture-buffer record: capacity, then the sample pointer, then the
/// filled size, then the user pointer. The user pointer is not sample bytes.
#[cfg(any(target_os = "macos", test, g1_regression))]
#[repr(C)]
struct QueueBuffer {
    capacity: u32,
    audio_data: *mut u8,
    byte_size: u32,
    user_data: *mut std::ffi::c_void,
    packet_description_capacity: u32,
    packet_descriptions: *mut std::ffi::c_void,
    packet_description_count: u32,
}

#[cfg(any(target_os = "macos", test, g1_regression))]
const _: () = {
    if std::mem::size_of::<usize>() == 8 {
        assert!(std::mem::offset_of!(QueueBuffer, audio_data) == 8);
        assert!(std::mem::offset_of!(QueueBuffer, byte_size) == 16);
        assert!(std::mem::offset_of!(QueueBuffer, user_data) == 24);
    }
    if std::mem::size_of::<usize>() == 4 {
        assert!(std::mem::offset_of!(QueueBuffer, audio_data) == 4);
        assert!(std::mem::offset_of!(QueueBuffer, byte_size) == 8);
        assert!(std::mem::offset_of!(QueueBuffer, user_data) == 12);
    }
};

#[cfg(target_os = "macos")]
struct QueueShare {
    pending: std::sync::Mutex<std::collections::VecDeque<u8>>,
    ready: std::sync::Condvar,
}

#[cfg(target_os = "macos")]
struct QueueInput {
    queue: usize,
    pending: std::sync::Arc<QueueShare>,
}

#[cfg(target_os = "macos")]
impl QueueInput {
    fn open() -> Result<Box<QueueInput>> {
        #[repr(C)]
        struct AudioFormat {
            rate: f64,
            kind: u32,
            flags: u32,
            bytes_per_packet: u32,
            frames_per_packet: u32,
            bytes_per_frame: u32,
            channels: u32,
            bits: u32,
            reserved: u32,
        }
        type Callback = unsafe extern "C" fn(*mut std::ffi::c_void, usize, *mut QueueBuffer, *const std::ffi::c_void, u32, *const std::ffi::c_void);
        #[link(name = "AudioToolbox", kind = "framework")]
        unsafe extern "C" {
            fn AudioQueueNewInput(format: *const AudioFormat, callback: Callback, user: *mut std::ffi::c_void, runloop: *mut std::ffi::c_void, mode: *const i8, flags: u32, queue: *mut usize) -> i32;
            fn AudioQueueAllocateBuffer(queue: usize, size: u32, buffer: *mut *mut QueueBuffer) -> i32;
            fn AudioQueueEnqueueBuffer(queue: usize, buffer: *mut QueueBuffer, packets: u32, desc: *const std::ffi::c_void) -> i32;
            fn AudioQueueStart(queue: usize, time: *const std::ffi::c_void) -> i32;
        }
        unsafe extern "C" fn capture(user: *mut std::ffi::c_void, queue: usize, buffer: *mut QueueBuffer, _time: *const std::ffi::c_void, _packets: u32, _desc: *const std::ffi::c_void) {
            if !buffer.is_null() {
                let size = (*buffer).byte_size as usize;
                let capacity = (*buffer).capacity as usize;
                let samples = (*buffer).audio_data;
                if !user.is_null() && !samples.is_null() && size <= capacity {
                    let share = &*(user as *const QueueShare);
                    let bytes = std::slice::from_raw_parts(samples, size);
                    if let Ok(mut pending) = share.pending.lock() {
                        if pending.len() < 65536 {
                            let room = 65536 - pending.len();
                            pending.extend(bytes.iter().take(room).copied());
                        }
                        share.ready.notify_one();
                    }
                }
                AudioQueueEnqueueBuffer(queue, buffer, 0, std::ptr::null());
            }
        }
        let format = AudioFormat {
            rate: 16000.0,
            kind: u32::from_be_bytes(*b"lpcm"),
            flags: 4 | 8,
            bytes_per_packet: 2,
            frames_per_packet: 1,
            bytes_per_frame: 2,
            channels: 1,
            bits: 16,
            reserved: 0,
        };
        let pending = std::sync::Arc::new(QueueShare {
            pending: std::sync::Mutex::new(std::collections::VecDeque::new()),
            ready: std::sync::Condvar::new(),
        });
        let mut queue = 0usize;
        let status = unsafe { AudioQueueNewInput(&format, capture, std::sync::Arc::as_ptr(&pending) as *mut std::ffi::c_void, std::ptr::null_mut(), std::ptr::null(), 0, &mut queue) };
        if status != 0 || queue == 0 {
            return Err(format!("the system audio queue could not be opened ({status})"));
        }
        let mut buffers = Vec::new();
        for _ in 0..3 {
            let mut buffer = std::ptr::null_mut();
            let allocated = unsafe { AudioQueueAllocateBuffer(queue, 3200, &mut buffer) };
            if allocated != 0 || buffer.is_null() {
                return Err(format!("the system audio queue could not be opened ({allocated})"));
            }
            let queued = unsafe { AudioQueueEnqueueBuffer(queue, buffer, 0, std::ptr::null()) };
            if queued != 0 {
                return Err(format!("the system audio queue could not be opened ({queued})"));
            }
            buffers.push(buffer as usize);
        }
        let started = unsafe { AudioQueueStart(queue, std::ptr::null()) };
        if started != 0 {
            return Err(format!("the system audio queue could not be started ({started})"));
        }
        let _ = buffers;
        Ok(Box::new(QueueInput { queue, pending }))
    }
}

#[cfg(target_os = "macos")]
impl Drop for QueueInput {
    fn drop(&mut self) {
        #[link(name = "AudioToolbox", kind = "framework")]
        unsafe extern "C" {
            fn AudioQueueStop(queue: usize, immediate: u8) -> i32;
            fn AudioQueueDispose(queue: usize, immediate: u8) -> i32;
        }
        unsafe {
            AudioQueueStop(self.queue, 1);
            AudioQueueDispose(self.queue, 1);
        }
    }
}

#[cfg(target_os = "macos")]
impl Read for QueueInput {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut pending = self.pending.pending.lock().map_err(|_| std::io::Error::other("microphone queue"))?;
        if pending.is_empty() {
            let (guard, _) = self.pending.ready.wait_timeout(pending, std::time::Duration::from_millis(200)).map_err(|_| std::io::Error::other("microphone queue"))?;
            pending = guard;
        }
        if pending.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "microphone waiting"));
        }
        let n = pending.len().min(buf.len());
        for slot in &mut buf[..n] {
            *slot = pending.pop_front().unwrap_or(0);
        }
        let _ = self.queue;
        Ok(n)
    }
}

/// System speaker for returned speech. The queue callback releases each buffer after it plays.
#[cfg(target_os = "macos")]
struct QueueOutput {
    queue: usize,
    started: bool,
}

#[cfg(target_os = "macos")]
impl QueueOutput {
    fn open(rate: u32) -> Result<Box<QueueOutput>> {
        #[repr(C)]
        struct AudioFormat {
            rate: f64,
            kind: u32,
            flags: u32,
            bytes_per_packet: u32,
            frames_per_packet: u32,
            bytes_per_frame: u32,
            channels: u32,
            bits: u32,
            reserved: u32,
        }
        type Callback = unsafe extern "C" fn(*mut std::ffi::c_void, usize, *mut QueueBuffer);
        #[link(name = "AudioToolbox", kind = "framework")]
        unsafe extern "C" {
            fn AudioQueueNewOutput(format: *const AudioFormat, callback: Callback, user: *mut std::ffi::c_void, runloop: *mut std::ffi::c_void, mode: *const i8, flags: u32, queue: *mut usize) -> i32;
        }
        unsafe extern "C" fn played(_user: *mut std::ffi::c_void, queue: usize, buffer: *mut QueueBuffer) {
            #[link(name = "AudioToolbox", kind = "framework")]
            unsafe extern "C" {
                fn AudioQueueFreeBuffer(queue: usize, buffer: *mut QueueBuffer) -> i32;
            }
            if !buffer.is_null() {
                AudioQueueFreeBuffer(queue, buffer);
            }
        }
        let format = AudioFormat {
            rate: rate as f64,
            kind: u32::from_be_bytes(*b"lpcm"),
            flags: 4 | 8,
            bytes_per_packet: 2,
            frames_per_packet: 1,
            bytes_per_frame: 2,
            channels: 1,
            bits: 16,
            reserved: 0,
        };
        let mut queue = 0usize;
        let status = unsafe { AudioQueueNewOutput(&format, played, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(), 0, &mut queue) };
        if status != 0 || queue == 0 {
            return Err(format!("the system speaker could not be opened ({status})"));
        }
        Ok(Box::new(QueueOutput { queue, started: false }))
    }

    fn write(&mut self, pcm: &[u8]) -> Result<()> {
        #[link(name = "AudioToolbox", kind = "framework")]
        unsafe extern "C" {
            fn AudioQueueAllocateBuffer(queue: usize, size: u32, buffer: *mut *mut QueueBuffer) -> i32;
            fn AudioQueueEnqueueBuffer(queue: usize, buffer: *mut QueueBuffer, packets: u32, desc: *const std::ffi::c_void) -> i32;
            fn AudioQueueStart(queue: usize, time: *const std::ffi::c_void) -> i32;
            fn AudioQueueFreeBuffer(queue: usize, buffer: *mut QueueBuffer) -> i32;
        }
        if pcm.len() < 2 || pcm.len() % 2 != 0 {
            return Ok(());
        }
        let mut buffer = std::ptr::null_mut();
        let allocated = unsafe { AudioQueueAllocateBuffer(self.queue, pcm.len() as u32, &mut buffer) };
        if allocated != 0 || buffer.is_null() {
            return Err(format!("the system speaker could not be opened ({allocated})"));
        }
        unsafe {
            let dest = std::slice::from_raw_parts_mut((*buffer).audio_data, pcm.len());
            dest.copy_from_slice(pcm);
            (*buffer).byte_size = pcm.len() as u32;
            let queued = AudioQueueEnqueueBuffer(self.queue, buffer, 0, std::ptr::null());
            if queued != 0 {
                AudioQueueFreeBuffer(self.queue, buffer);
                return Err(format!("the system speaker could not be started ({queued})"));
            }
            if !self.started {
                let started = AudioQueueStart(self.queue, std::ptr::null());
                if started != 0 {
                    return Err(format!("the system speaker could not be started ({started})"));
                }
                self.started = true;
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl Drop for QueueOutput {
    fn drop(&mut self) {
        #[link(name = "AudioToolbox", kind = "framework")]
        unsafe extern "C" {
            fn AudioQueueStop(queue: usize, immediate: u8) -> i32;
            fn AudioQueueDispose(queue: usize, immediate: u8) -> i32;
        }
        unsafe {
            AudioQueueStop(self.queue, 1);
            AudioQueueDispose(self.queue, 1);
        }
    }
}

/// System wave speaker. Buffers stay owned here so a write can queue without waiting for the previous one to finish.
#[cfg(windows)]
struct WaveOutput {
    handle: usize,
    buffers: Vec<Vec<u8>>,
    headers: Vec<WaveHdr>,
    busy: Vec<bool>,
}

#[cfg(windows)]
impl WaveOutput {
    fn open(rate: u32) -> Result<Box<WaveOutput>> {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn waveOutGetNumDevs() -> u32;
            fn waveOutOpen(handle: *mut usize, device: u32, format: *const WaveFormat, callback: usize, instance: usize, flags: u32) -> u32;
            fn waveOutPrepareHeader(handle: usize, header: *mut WaveHdr, size: u32) -> u32;
        }
        #[repr(C)]
        struct WaveFormat {
            format: u16,
            channels: u16,
            rate: u32,
            average: u32,
            align: u16,
            bits: u16,
            size: u16,
        }
        if unsafe { waveOutGetNumDevs() } == 0 {
            return Err("no speaker is available on this host".into());
        }
        let format = WaveFormat {
            format: 1,
            channels: 1,
            rate,
            average: rate.saturating_mul(2),
            align: 2,
            bits: 16,
            size: 0,
        };
        let mut handle = 0usize;
        let status = unsafe { waveOutOpen(&mut handle, u32::MAX, &format, 0, 0, 0x00010000) };
        if status != 0 || handle == 0 {
            return Err(format!("the speaker could not be opened ({status})"));
        }
        let mut output = Box::new(WaveOutput {
            handle,
            buffers: (0..8).map(|_| vec![0u8; 131072]).collect(),
            headers: (0..8).map(|_| WaveHdr {
                data: std::ptr::null_mut(),
                length: 131072,
                recorded: 0,
                user: 0,
                flags: 0,
                loops: 0,
                next: std::ptr::null_mut(),
                reserved: 0,
            }).collect(),
            busy: vec![false; 8],
        });
        let pointers: Vec<*mut u8> = output.buffers.iter_mut().map(|buffer| buffer.as_mut_ptr()).collect();
        for (index, data) in pointers.into_iter().enumerate() {
            output.headers[index].data = data;
            output.headers[index].length = output.buffers[index].len() as u32;
        }
        let prepared = unsafe {
            let mut failed = 0u32;
            for header in &mut output.headers {
                let header = header as *mut WaveHdr;
                let status = waveOutPrepareHeader(output.handle, header, std::mem::size_of::<WaveHdr>() as u32);
                if status != 0 {
                    failed = status;
                    break;
                }
            }
            failed
        };
        if prepared != 0 {
            return Err(format!("the speaker could not be started ({prepared})"));
        }
        Ok(output)
    }

    fn write(&mut self, pcm: &[u8]) -> Result<()> {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn waveOutWrite(handle: usize, header: *mut WaveHdr, size: u32) -> u32;
        }
        let mut offset = 0usize;
        while offset + 1 < pcm.len() {
            let mut slot = None;
            for _ in 0..5 {
                for index in 0..self.busy.len() {
                    if self.busy[index] {
                        let flags = self.headers[index].flags;
                        if flags & 0x00000001 != 0 && flags & 0x00000010 == 0 {
                            self.busy[index] = false;
                        }
                    }
                }
                slot = self.busy.iter().position(|busy| !*busy);
                if slot.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let Some(slot) = slot else {
                // Earlier speech is still queued. Keep the microphone moving.
                return Ok(());
            };
            let room = self.buffers[slot].len() & !1;
            let n = (pcm.len() - offset).min(room) & !1;
            if n < 2 {
                break;
            }
            self.buffers[slot][..n].copy_from_slice(&pcm[offset..offset + n]);
            self.headers[slot].length = n as u32;
            self.headers[slot].recorded = 0;
            let status = unsafe { waveOutWrite(self.handle, &mut self.headers[slot], std::mem::size_of::<WaveHdr>() as u32) };
            if status != 0 {
                return Err(format!("the speaker could not be started ({status})"));
            }
            self.busy[slot] = true;
            offset += n;
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WaveOutput {
    fn drop(&mut self) {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn waveOutReset(handle: usize) -> u32;
            fn waveOutUnprepareHeader(handle: usize, header: *mut WaveHdr, size: u32) -> u32;
            fn waveOutClose(handle: usize) -> u32;
        }
        unsafe {
            waveOutReset(self.handle);
            for header in &mut self.headers {
                waveOutUnprepareHeader(self.handle, header as *mut WaveHdr, std::mem::size_of::<WaveHdr>() as u32);
            }
            waveOutClose(self.handle);
        }
    }
}

#[derive(Debug)]
enum CaptureDone {
    Utterance(String),
    SpeechStopped,
}

trait SpeechOut {
    fn play(&mut self, rate: u32, pcm: &[u8]) -> Result<()>;
}

struct LiveSpeaker {
    rate: u32,
    #[cfg(windows)]
    output: Option<Box<WaveOutput>>,
    #[cfg(target_os = "linux")]
    output: Option<KernelPlayback>,
    #[cfg(target_os = "macos")]
    output: Option<Box<QueueOutput>>,
}

impl LiveSpeaker {
    fn new() -> LiveSpeaker {
        LiveSpeaker {
            rate: 0,
            #[cfg(windows)]
            output: None,
            #[cfg(target_os = "linux")]
            output: None,
            #[cfg(target_os = "macos")]
            output: None,
        }
    }
}

impl SpeechOut for LiveSpeaker {
    fn play(&mut self, rate: u32, pcm: &[u8]) -> Result<()> {
        if pcm.len() < 2 {
            return Ok(());
        }
        if self.rate != rate {
            #[cfg(windows)]
            {
                self.output = None;
            }
            #[cfg(target_os = "linux")]
            {
                self.output = None;
            }
            #[cfg(target_os = "macos")]
            {
                self.output = None;
            }
            self.rate = rate;
        }
        #[cfg(windows)]
        {
            if self.output.is_none() {
                self.output = Some(WaveOutput::open(rate)?);
            }
            return self.output.as_mut().expect("speaker").write(pcm);
        }
        #[cfg(target_os = "linux")]
        {
            if self.output.is_none() {
                self.output = Some(KernelPlayback::open(rate)?);
            }
            return self.output.as_mut().expect("speaker").write(pcm);
        }
        #[cfg(target_os = "macos")]
        {
            if self.output.is_none() {
                self.output = Some(QueueOutput::open(rate)?);
            }
            return self.output.as_mut().expect("speaker").write(pcm);
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (rate, pcm);
            Err("no speaker is available on this host".into())
        }
    }
}

/// playback24 is the session rate. playback is the narrower copy and is used only when the wide buffer is absent.
fn take_playback(value: &Json) -> Option<(u32, Vec<u8>)> {
    let wide = text(value, "playback24");
    let narrow = text(value, "playback");
    let (encoded, rate) = if !wide.is_empty() {
        let rate = number(value, "speech_rate");
        (wide, if (8_000..=96_000).contains(&rate) { rate as u32 } else { 24_000 })
    } else if !narrow.is_empty() {
        let rate = number(value, "playback_rate");
        (narrow, if (8_000..=96_000).contains(&rate) { rate as u32 } else { 16_000 })
    } else {
        return None;
    };
    let pcm = network::unbase64(&encoded).ok()?;
    if pcm.len() < 2 || pcm.len() % 2 != 0 || pcm.len() > 256 * 1024 {
        return None;
    }
    Some((rate, pcm))
}

/// Utterance capture still ends on the lease or on ready. Speech keeps the microphone open and plays each returned buffer.
fn pull_microphone(frame: usize, deadline: std::time::Instant, speech: bool, recorder: &mut Recorder, feed: &mut dyn FnMut(&[u8]) -> Result<Json>, player: &mut dyn SpeechOut) -> Result<CaptureDone> {
    let mut speech = speech;
    let fed_before = CAPTURED_FRAMES.load(std::sync::atomic::Ordering::Relaxed);
    loop {
        if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        if !speech && std::time::Instant::now() >= deadline {
            return Err("the microphone lease expired before an utterance completed".into());
        }
        let read_deadline = if speech { std::time::Instant::now() + std::time::Duration::from_secs(2) } else { deadline };
        let bytes = recorder.fill(frame, read_deadline)?;
        if bytes.len() < 2 || bytes.len() % 2 != 0 {
            return Err(format!(
                "the microphone source ended after {} frames without an utterance",
                CAPTURED_FRAMES.load(std::sync::atomic::Ordering::Relaxed) - fed_before
            ));
        }
        CAPTURED_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let value = feed(&bytes)?;
        if speech || boolean(&value, "speech") {
            speech = true;
            if let Some((rate, pcm)) = take_playback(&value) {
                player.play(rate, &pcm)?;
            }
            if boolean(&value, "stopped") {
                return Ok(CaptureDone::SpeechStopped);
            }
            continue;
        }
        if boolean(&value, "ready") {
            return Ok(CaptureDone::Utterance(text(&value, "receipt")));
        }
    }
}

fn microphone(effect: &Json, state: &mut Json, root: &str) -> Result<CaptureDone> {
    let ticket = text(effect, "voice_audio_ticket");
    let session = text(effect, "voice_session");
    let frame = number(effect, "frame_bytes").clamp(2, 640) as usize;
    let lease = number(effect, "voice_lease_ms");
    let speech = boolean(effect, "speech");
    let opening = core_event(
        state,
        root,
        "voice_audio_start",
        &obj(vec![("voice_audio_ticket", s(&ticket)), ("voice_session", s(&session))]),
    )?;
    *state = opening.get("state")?.clone();
    let mut recorder = Recorder::open()?;
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_millis(if lease > 0 { lease as u64 } else { SILENCE_MS as u64 });
    let mut player = LiveSpeaker::new();
    pull_microphone(frame, deadline, speech, &mut recorder, &mut |bytes| {
        let answer = parse(&envelope(&obj(vec![
            ("op", s("voice_audio_feed")),
            ("voice_audio_ticket", s(&ticket)),
            ("voice_session", s(&session)),
            ("data", s(&network::b64(&bytes))),
        ])
        .string()))?;
        if let Ok(error) = answer.get("error") {
            return Err(error.text().unwrap_or("the core refused the microphone frame").to_string());
        }
        Ok(answer.get("value")?.clone())
    }, &mut player)
}

/// The core keeps operation ownership in state and names it on the effect, so the
/// callback replays the same tokens instead of claiming a fresh operation.
fn callback_inputs(effect: &Json, state: &Json, response: &[u8], failure: Option<String>) -> Json {
    let purpose = text(effect, "purpose");
    let mut inputs = obj(vec![("purpose", s(&purpose))]);
    for key in TICKETS {
        if get(effect, key) != &Null {
            put(&mut inputs, key, get(effect, key).clone());
        }
    }
    if purpose == "model" && get(state, "model_expected") != &Null {
        put(&mut inputs, "model_ticket", get(state, "model_expected").clone());
    }
    let drive = get(state, "drive_expected");
    if drive != &Null && get(drive, "purpose").text().unwrap_or("") == purpose && get(&inputs, "drive_ticket") == &Null {
        put(&mut inputs, "drive_ticket", get(drive, "ticket").clone());
    }
    if let Some(error) = failure {
        put(&mut inputs, "error", s(&error));
    } else {
        put(&mut inputs, "data", s(&network::b64(response)));
    }
    inputs
}

/// One core call: the same envelope the Android JNI bridge carries over C.
fn core_event(state: &Json, root: &str, action: &str, inputs: &Json) -> Result<Json> {
    let answer = parse(&envelope(&obj(vec![
        ("op", s("event")),
        ("state", state.clone()),
        ("action", s(action)),
        ("inputs", inputs.clone()),
        ("root", s(root)),
    ])
    .string()))?;
    if let Ok(error) = answer.get("error") {
        return Err(error.text().unwrap_or("the core rejected the request").into());
    }
    Ok(answer.get("value")?.clone())
}

/// Drive one user action to rest, executing each effect the core returns.
fn advance(state: &mut Json, root: &str, action: &str, inputs: Json) -> Result<Json> {
    advance_shown(state, root, action, inputs, &mut |_| {})
}

/// The same, showing every surface the core renders on the way: the turn as it
/// is sent, and the reply as it arrives.
fn advance_shown(state: &mut Json, root: &str, action: &str, inputs: Json, shown: &mut dyn FnMut(&Json)) -> Result<Json> {
    let mut action = action.to_string();
    let mut inputs = inputs;
    for _ in 0..EFFECT_HOPS {
        let reply = core_event(state, root, &action, &inputs)?;
        *state = reply.get("state")?.clone();
        let effect = reply.get("effect")?.clone();
        if effect == Null {
            return Ok(reply.get("ui")?.clone());
        }
        shown(reply.get("ui")?);
        match text(&effect, "kind").as_str() {
            "network" => {
                let ticket = text(&effect, "model_ticket");
                let started = std::time::Instant::now();
                let outcome = exchange(&effect, &mut |update| {
                    if ticket.is_empty() {
                        return;
                    }
                    let mut progress = obj(vec![("model_ticket", s(&ticket))]);
                    match update {
                        ExchangeProgress::Preview(said, elapsed) => {
                            put(&mut progress, "stream_text", s(said));
                            put(&mut progress, "elapsed_ms", Num(elapsed.to_string()));
                        }
                        ExchangeProgress::Retry { failed, delay } => {
                            put(&mut progress, "retrying", s(&format!(
                                "Provider attempt {failed} of {PROVIDER_ATTEMPTS} failed. Retrying in {} seconds.", delay.as_secs()
                            )));
                        }
                    }
                    if let Ok(reply) = core_event(state, root, "model_progress", &progress) {
                        if let (Ok(next), Ok(ui)) = (reply.get("state"), reply.get("ui")) {
                            *state = next.clone();
                            if let Ok(retry) = progress.get("retrying") {
                                let mut display = ui.clone();
                                put(&mut display, "provider_retry", retry.clone());
                                shown(&display);
                            } else {
                                shown(ui);
                            }
                        }
                    }
                });
                let (next, callback) = match outcome {
                    Ok(response) => ("network_result", callback_inputs(&effect, state, &response, None)),
                    Err(error) if error.message == CANCELLED => {
                        CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
                        ("cancel", obj(vec![("elapsed_ms", Num(started.elapsed().as_millis().to_string()))]))
                    }
                    Err(error) => {
                        let mut callback = callback_inputs(&effect, state, &[], Some(error.message));
                        if error.received_bytes > 0 {
                            put(&mut callback, "received_bytes", Num(error.received_bytes.to_string()));
                        }
                        if error.attempts > 0 {
                            put(&mut callback, "provider_attempts", Num(error.attempts.to_string()));
                        }
                        ("failure", callback)
                    }
                };
                action = next.into();
                inputs = callback;
            }
            "tool" => {
                let output = if text(&effect, "tool") == workspace_command::TOOL {
                    let mut request = effect.clone();
                    put(&mut request, "root", s(root));
                    EXECUTED_TOOLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let output = application::shell_tool(&request, transport_cancelled);
                    if transport_cancelled() {
                        action = "cancel".into();
                        inputs = Null;
                        continue;
                    }
                    output
                } else if text(&effect, "tool").starts_with("phone_") {
                    device_folder_tool(&effect, root).map(|listed| s(&listed))
                } else {
                    let mut request = effect.clone();
                    put(&mut request, "op", s("tool"));
                    put(&mut request, "root", s(root));
                    EXECUTED_TOOLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let answer = parse(&envelope(&request.string()))?;
                    match answer.get("error") {
                        Ok(error) => Err(error.text().unwrap_or("the tool failed").to_string()),
                        Err(_) => Ok(answer.get("value")?.clone()),
                    }
                };
                let mut callback = obj(vec![(
                    "purpose",
                    if get(&effect, "purpose") == &Null { s("local_tool") } else { get(&effect, "purpose").clone() },
                )]);
                for key in ["tool_ticket", "tool_binding", "editor", "editor_dialog"] {
                    if get(&effect, key) != &Null {
                        put(&mut callback, key, get(&effect, key).clone());
                    }
                }
                let expected = get(state, "operation_expected");
                if get(&effect, "tool_ticket") == &Null && expected != &Null {
                    put(&mut callback, "tool_ticket", get(expected, "ticket").clone());
                }
                match output {
                    Ok(value) => {
                        put(&mut callback, "output", value);
                        action = "tool_result".into();
                    }
                    Err(error) => {
                        put(&mut callback, "error", s(&error));
                        action = "failure".into();
                    }
                }
                inputs = callback;
            }
            "media_playback_stop" if boolean(&effect, "voice_only") => {
                // The speech speaker is closed when the microphone loop returns.
                return Ok(reply.get("ui")?.clone());
            }
            "voice_listen" => {
                // An utterance returns the receipt the core minted. Speech stays open,
                // plays each returned buffer, and ends only when that session stops.
                match microphone(&effect, state, root) {
                    Ok(CaptureDone::Utterance(receipt)) => {
                        action = "voice_audio_result".into();
                        inputs = obj(vec![
                            ("voice_audio_ticket", get(&effect, "voice_audio_ticket").clone()),
                            ("voice_session", get(&effect, "voice_session").clone()),
                            ("receipt", s(&receipt)),
                        ]);
                    }
                    Ok(CaptureDone::SpeechStopped) => {
                        action = "voice_session_stop".into();
                        inputs = obj(vec![]);
                    }
                    Err(error) if boolean(&effect, "speech") && error == CANCELLED => {
                        action = "voice_session_stop".into();
                        inputs = obj(vec![]);
                    }
                    Err(error) => {
                        action = "voice_audio_failure".into();
                        inputs = obj(vec![
                            ("voice_audio_ticket", get(&effect, "voice_audio_ticket").clone()),
                            ("voice_session", get(&effect, "voice_session").clone()),
                            ("error", s(&error)),
                        ]);
                    }
                }
            }
            "cancel" => {
                // The request was dropped the moment the person cancelled; all
                // that is left is to stop the core's own tool runner.
                discard_media_recording();
                let _ = core_call(obj(vec![("op", s("cancel_tool"))]));
                return Ok(reply.get("ui")?.clone());
            }
            "document" if picker() => {
                let (next, callback) = attachment(get(&effect, "document_ticket"));
                action = next.into();
                inputs = callback;
            }
            "conversation_document" if picker() => match choose_file("Import conversation backup", Files::Backup).and_then(|chosen| chosen.map(|path| read_text(&path, 1024 * 1024)).transpose()) {
                Ok(Some(content)) => {
                    action = "conversation_review".into();
                    inputs = obj(vec![("content", s(&content))]);
                }
                Ok(None) => return Ok(reply.get("ui")?.clone()),
                Err(error) => {
                    action = "failure".into();
                    inputs = obj(vec![("error", s(&error))]);
                }
            },
            // One project ZIP. archive_accept later emits archive_import with no bytes,
            // so the zip stays here until that effect, or until archive_clear.
            "document_zip" if picker() => {
                clear_import();
                match choose_zip() {
                    Ok(Some(path)) => match read_limited(&path, PROJECT_FILE_LIMIT) {
                        Ok(bytes) => match archive::preview(&obj(vec![("archive", s(&network::b64(&bytes)))])) {
                            Ok(previewed) => {
                                keep_import(bytes);
                                action = "archive_review".into();
                                inputs = obj(vec![
                                    ("names", get(&previewed, "names").clone()),
                                    ("bytes", get(&previewed, "bytes").clone()),
                                ]);
                            }
                            Err(error) => {
                                action = "failure".into();
                                inputs = obj(vec![("error", s(&error))]);
                            }
                        },
                        Err(error) => {
                            action = "failure".into();
                            inputs = obj(vec![("error", s(&error))]);
                        }
                    },
                    Ok(None) => return Ok(reply.get("ui")?.clone()),
                    Err(error) => {
                        action = "failure".into();
                        inputs = obj(vec![("error", s(&error))]);
                    }
                }
            },
            "document_export" => {
                // The snapshot is sealed to disk first, so choosing where it
                // goes can be cancelled or retried without losing it.
                let ticket = get(&effect, "ticket").clone();
                let content = text(&effect, "content");
                match export_stage(&effect, content.as_bytes()) {
                    Ok(()) => {
                        action = "export_staged".into();
                        inputs = obj(vec![("ticket", ticket), ("bytes", Num(content.len().to_string()))]);
                    }
                    Err(error) => {
                        action = "export_stage_failed".into();
                        inputs = obj(vec![("ticket", ticket), ("error", s(&error))]);
                    }
                }
            }
            "export_picker" => {
                let ticket = get(&effect, "ticket").clone();
                let written = (|| -> Result<()> {
                    let (name, bytes) = export_load(&text(&effect, "ticket"))?;
                    let path = save_file("Save export", &name)?.ok_or_else(|| "Destination selection cancelled".to_string())?;
                    let reply = core_event(state, root, "export_started", &obj(vec![("ticket", ticket.clone())]))?;
                    *state = reply.get("state")?.clone();
                    shown(reply.get("ui")?);
                    let pending = get(state, "export_pending");
                    if text(pending, "phase") != "writing" || get(pending, "ticket") != &ticket {
                        return Err("The export changed before it was written; no bytes were written".into());
                    }
                    std::fs::write(&path, &bytes).map_err(|error| format!("The file could not be written: {error}"))?;
                    match read_path_limited(Path::new(&path), bytes.len() as u64) {
                        Ok(found) if found == bytes => {}
                        _ => return Err("Document readback differs from requested bytes".into()),
                    }
                    export_clear(&text(&effect, "ticket"))
                })();
                match written {
                    Ok(()) => {
                        action = "export_done".into();
                        inputs = obj(vec![("ticket", ticket)]);
                    }
                    Err(error) => {
                        action = "export_failed".into();
                        inputs = obj(vec![("ticket", ticket), ("error", s(&error))]);
                    }
                }
            }
            "export_discard" => {
                let ticket = get(&effect, "ticket").clone();
                match export_clear(&text(&effect, "ticket")) {
                    Ok(()) => {
                        action = "export_discarded".into();
                        inputs = obj(vec![("ticket", ticket)]);
                    }
                    Err(error) => {
                        action = "export_failed".into();
                        inputs = obj(vec![("ticket", ticket), ("error", s(&error))]);
                    }
                }
            }
            // A desktop export is written in one step, so nothing is in flight
            // to stop by the time a cancellation is asked for.
            "export_cancel" => return Ok(reply.get("ui")?.clone()),
            "wallpaper_picker" if picker() => match choose_file("Choose a wallpaper image", Files::Picture) {
                Ok(Some(path)) => {
                    action = "wallpaper_result".into();
                    inputs = obj(vec![("uri", s(&format!("file:///{}", path.replace('\\', "/"))))]);
                }
                // Nothing chosen: the settings dialog simply stays as it was.
                Ok(None) => return Ok(reply.get("ui")?.clone()),
                Err(error) => {
                    action = "failure".into();
                    inputs = obj(vec![("error", s(&error))]);
                }
            },
            "parallel" => {
                let requests = match effect.get("requests") {
                    Ok(Arr(items)) => items.clone(),
                    _ => Vec::new(),
                };
                if requests.is_empty() || requests.len() > 8 {
                    action = "parallel_result".into();
                    inputs = obj(vec![
                        ("purpose", s("parallel")),
                        ("consult", get(&effect, "consult").clone()),
                        ("results", Arr(vec![obj(vec![("ok", Bool(false)), ("error", s("Parallel request was empty or too large"))])])),
                    ]);
                } else {
                    let mut pending: Vec<(usize, Json)> = requests.into_iter().enumerate().collect();
                    let mut results = vec![Null; pending.len()];
                    let mut cancelled = false;
                    while !pending.is_empty() && !cancelled {
                        let mut wave = Vec::new();
                        let mut rest = Vec::new();
                        let mut used = std::collections::BTreeMap::<i64, i64>::new();
                        for (index, request) in pending.drain(..) {
                            let lane = number(&request, "lane");
                            let limit = number(&request, "lane_limit").max(1);
                            let count = used.entry(lane).or_insert(0);
                            if *count < limit {
                                *count += 1;
                                wave.push((index, request));
                            } else {
                                rest.push((index, request));
                            }
                        }
                        pending = rest;
                        if wave.is_empty() {
                            break;
                        }
                        let mut handles = Vec::new();
                        for (index, request) in wave {
                            handles.push((index, std::thread::spawn(move || exchange(&request, &mut |_| {}))));
                        }
                        for (index, handle) in handles {
                            match handle.join() {
                                Ok(Ok(bytes)) => {
                                    results[index] = obj(vec![("ok", Bool(true)), ("data", s(&network::b64(&bytes)))]);
                                }
                                Ok(Err(error)) => {
                                    results[index] = obj(vec![("ok", Bool(false)), ("error", s(&error.message))]);
                                }
                                Err(_) => {
                                    results[index] = obj(vec![("ok", Bool(false)), ("error", s("The request did not finish"))]);
                                }
                            }
                        }
                        if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
                            cancelled = true;
                        }
                    }
                    if cancelled {
                        action = "cancel".into();
                        inputs = obj(vec![("elapsed_ms", Num("0".into()))]);
                    } else {
                        action = "parallel_result".into();
                        inputs = obj(vec![
                            ("purpose", s("parallel")),
                            ("consult", get(&effect, "consult").clone()),
                            ("results", Arr(results)),
                        ]);
                    }
                }
            }
            "device_elevation" => {
                action = "device_elevation_result".into();
                inputs = obj(vec![("notice", s(&request_administrator()))]);
            }
            // A Windows folder is a directory write. A content:// tree stays on the Android provider.
            "archive_drive" => match workspace_archive(&effect, root) {
                Ok(callback) => {
                    action = "archive_built".into();
                    inputs = callback;
                }
                Err(error) => {
                    action = "artifact_failed".into();
                    inputs = obj(vec![("ticket", get(&effect, "ticket").clone()), ("error", s(&error))]);
                }
            },
            "phone_revision" => match phone_revision(&effect) {
                Ok(callback) => {
                    action = "phone_revision_result".into();
                    inputs = callback;
                }
                Err(error) => {
                    action = "failure".into();
                    inputs = obj(vec![
                        ("purpose", s("phone_revision")),
                        ("ticket", get(&effect, "ticket").clone()),
                        ("phase", get(&effect, "phase").clone()),
                        ("error", s(&error)),
                    ]);
                }
            },
            "artifact_file" => match artifact_file_result(&effect, root) {
                Ok(callback) => {
                    action = "artifact_built".into();
                    inputs = callback;
                }
                Err(error) => {
                    action = "artifact_failed".into();
                    inputs = obj(vec![("ticket", get(&effect, "ticket").clone()), ("error", s(&error))]);
                }
            },
            "archive" => match project_export_archive(&effect, state, root, shown) {
                Ok((next, callback)) => {
                    action = next;
                    inputs = callback;
                }
                Err(error) => {
                    action = "export_stage_failed".into();
                    inputs = obj(vec![("ticket", get(&effect, "ticket").clone()), ("error", s(&error))]);
                }
            },
            "project_phone" => match windows_project_phone(&effect, root) {
                Some(Ok(callback)) => {
                    action = "project_phone_result".into();
                    inputs = callback;
                }
                Some(Err(error)) => {
                    action = "failure".into();
                    inputs = project_failure(&effect, &error);
                }
                None => {
                    action = "failure".into();
                    inputs = unavailable_effect(&effect);
                }
            },
            "project_clear" => {
                if windows_project_clear(state) {
                    action = "project_cleared".into();
                    inputs = obj(vec![("ticket", get(&effect, "ticket").clone()), ("purpose", get(&effect, "purpose").clone())]);
                } else {
                    action = "failure".into();
                    inputs = unavailable_effect(&effect);
                }
            }
            "document_tree" | "phone_access_request" => match choose_folder("Choose a folder") {
                Ok(Some(path)) => {
                    action = "folder_result".into();
                    inputs = obj(vec![("uri", s(&path))]);
                }
                Ok(None) => return Ok(reply.get("ui")?.clone()),
                Err(error) => {
                    action = "failure".into();
                    inputs = obj(vec![("error", s(&error))]);
                }
            },
            // The core already stored the chosen work tree. Do not turn that into a failure.
            "workspace_selected" | "storage_policy" => return Ok(reply.get("ui")?.clone()),
            // Review cancel drops the zip the host was holding. The dialog is already closed.
            "archive_clear" => {
                clear_import();
                return Ok(reply.get("ui")?.clone());
            }
            // archive_accept emits kind archive_import and does not carry the zip.
            // Use the bytes kept from document_zip. If the effect already has them, use those.
            "archive_import" => {
                let carried = !text(&effect, "archive").is_empty();
                let archive = if carried {
                    clear_import();
                    Some(get(&effect, "archive").clone())
                } else {
                    take_import().map(|bytes| s(&network::b64(&bytes)))
                };
                match archive {
                    Some(archive) => {
                        let root_value = if text(&effect, "root").is_empty() { s(root) } else { get(&effect, "root").clone() };
                        match archive::import(&obj(vec![("root", root_value), ("archive", archive)])) {
                            Ok(report) => {
                                action = "archive_done".into();
                                inputs = obj(vec![
                                    ("files", get(&report, "files").clone()),
                                    ("directories", get(&report, "directories").clone()),
                                ]);
                            }
                            Err(error) => {
                                action = "failure".into();
                                inputs = obj(vec![("error", s(&error))]);
                            }
                        }
                    }
                    None => {
                        action = "failure".into();
                        inputs = obj(vec![("error", s("Selected project archive is no longer available; select it again"))]);
                    }
                }
            }
            // The core already placed the snapshot arguments on the effect.
            // Read the file only inside the existing snapshot function.
            "artifact_snapshot" => {
                let (next, callback) = local_operation_callback(&effect, state, application::artifact_snapshot(&effect));
                action = next;
                inputs = callback;
            }
            "media_pick" => {
                let ticket = text(&effect, "media_ticket");
                match chosen_media_file() {
                    Ok(Some(path)) => match stage_media_file(&ticket, &path) {
                        Ok(receipt) => {
                            action = "media_input_result".into();
                            inputs = obj(vec![("media_ticket", s(&ticket)), ("media_receipt", s(&receipt))]);
                        }
                        Err(error) => {
                            let (next, callback) = media_failure(&ticket, error);
                            action = next.into();
                            inputs = callback;
                        }
                    },
                    Ok(None) => {
                        cancel_media_input(&ticket);
                        action = "media_input_cancel".into();
                        inputs = obj(vec![("media_ticket", s(&ticket))]);
                    }
                    Err(error) => {
                        let (next, callback) = media_failure(&ticket, error);
                        action = next.into();
                        inputs = callback;
                    }
                }
            }
            "media_record" => match start_media_recording(&effect) {
                Ok(()) => return Ok(reply.get("ui")?.clone()),
                Err(error) => {
                    let (next, callback) = media_failure(&text(&effect, "media_ticket"), error);
                    action = next.into();
                    inputs = callback;
                }
            },
            "media_record_stop" => match finish_media_recording(&text(&effect, "media_ticket")) {
                Ok(None) => return Ok(reply.get("ui")?.clone()),
                Ok(Some(bytes)) => {
                    action = "media_input_result".into();
                    inputs = obj(vec![
                        ("media_ticket", get(&effect, "media_ticket").clone()),
                        ("name", s("recording.wav")),
                        ("data", s(&network::b64(&bytes))),
                    ]);
                }
                Err(error) => {
                    let (next, callback) = media_failure(&text(&effect, "media_ticket"), error);
                    action = next.into();
                    inputs = callback;
                }
            },
            "oauth_browser" => match open_authorization(&text(&effect, "url")) {
                Ok(()) => return Ok(reply.get("ui")?.clone()),
                Err(_) => {
                    action = "oauth_cancel".into();
                    inputs = obj(vec![]);
                }
            },
            other => {
                // The core scopes a refused platform service to that operation, so
                // a service with no Linux binding yet cannot strand the app.
                action = "failure".into();
                inputs = obj(vec![
                    ("purpose", get(&effect, "purpose").clone()),
                    ("error", s(&format!("{other} is not available on this host yet"))),
                ]);
            }
        }
    }
    Err("The core kept requesting platform work without settling".into())
}

/// Launch an elevated instance only in response to the explicit user action.
fn request_administrator() -> String {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::null;

        match host_fs::elevated() {
            Ok(true) => return host_fs::process_access(),
            Ok(false) => {}
            Err(error) => return format!("Could not check the current process token: {error}"),
        }
        let path = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => return format!("Could not locate the executable: {error}"),
        };
        let directory = match std::env::current_dir() {
            Ok(path) => path,
            Err(error) => return format!("Could not locate the working directory: {error}"),
        };
        let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let dir: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
        let mut params = Vec::new();
        for argument in std::env::args_os().skip(1) {
            if !params.is_empty() { params.push(u16::from(b' ')); }
            let slash = u16::from(b'\\');
            let quote = u16::from(b'"');
            params.push(quote);
            let mut backslashes = 0;
            for unit in argument.encode_wide() {
                if unit == slash {
                    backslashes += 1;
                    continue;
                }
                let repeat = if unit == quote { backslashes * 2 + 1 } else { backslashes };
                params.extend(std::iter::repeat_n(slash, repeat));
                params.push(unit);
                backslashes = 0;
            }
            params.extend(std::iter::repeat_n(slash, backslashes * 2));
            params.push(quote);
        }
        params.push(0);
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(hwnd: isize, verb: *const u16, file: *const u16, params: *const u16, dir: *const u16, show: i32) -> isize;
        }
        let arguments = if params.len() == 1 { null() } else { params.as_ptr() };
        let code = unsafe { ShellExecuteW(0, verb.as_ptr(), file.as_ptr(), arguments, dir.as_ptr(), 1) };
        if code > 32 {
            "The system accepted the elevated launch. Check access in the new instance.".into()
        } else {
            format!("The elevated launch did not start (shell result {code}).")
        }
    }
    #[cfg(not(windows))]
    host_fs::process_access()
}

/// Whether this run can ask the person to choose a file.
fn picker() -> bool {
    #[cfg(windows)]
    return window::showing();
    #[cfg(target_os = "linux")]
    return std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() || std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").is_ok();
    #[cfg(not(any(windows, target_os = "linux")))]
    return std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").is_ok();
}

/// What a file dialog offers. The window has its own copy of this list.
#[cfg(windows)]
use window::Files;
#[cfg(not(windows))]
#[allow(dead_code)]
enum Files {
    Attachment,
    Picture,
    Backup,
}

/// Asks for one directory. `Ok(None)` means the person closed the dialog.
fn choose_folder(title: &str) -> Result<Option<String>> {
    #[cfg(windows)]
    return window::choose_folder(title);
    #[cfg(target_os = "macos")]
    if let Ok(choice) = macos_open_panel(title) {
        return Ok(choice);
    }
    #[cfg(not(windows))]
    browse_folder(title)
}

const FILE_PAGE_BYTES: u64 = 1024 * 1024;
const DIRECTORY_PAGE_ENTRIES: u64 = 1024;
const DIRECTORY_WALK_BUDGET: usize = 100_000;

fn file_number(args: &Json, key: &str, default: u64) -> Result<u64> {
    match get(args, key) {
        Null => Ok(default),
        Num(raw) | Str(raw) => raw.parse().map_err(|_| format!("Invalid nonnegative {key}")),
        _ => Err(format!("Invalid nonnegative {key}")),
    }
}

fn file_arg<'a>(args: &'a Json, key: &str) -> Result<&'a str> {
    match get(args, key) {
        Null => Ok(""),
        value => value.text().map_err(|_| format!("Invalid {key}")),
    }
}

fn file_quantity(value: u64) -> Json {
    Num(value.to_string())
}

fn path_display_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        let mut text = path.to_string_lossy().replace('/', "\\");
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            text = format!(r"\\{rest}");
        } else if let Some(rest) = text.strip_prefix(r"\\?\") {
            text = rest.to_string();
        }
        return text.trim_end_matches('\\').to_string();
    }
    #[cfg(not(windows))]
    path.to_string_lossy().trim_end_matches('/').to_string()
}

fn path_contained(root: &Path, child: &Path) -> bool {
    let root_raw = path_display_key(root);
    let child_raw = path_display_key(child);
    if cfg!(windows) {
        let root_key = root_raw.to_ascii_lowercase();
        let child_key = child_raw.to_ascii_lowercase();
        child_key == root_key || child_key.strip_prefix(&root_key).is_some_and(|rest| rest.starts_with('\\'))
    } else {
        child_raw == root_raw || child_raw.strip_prefix(&root_raw).is_some_and(|rest| rest.starts_with('/'))
    }
}

fn normalize_components(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn path_suffix(root: &Path, target: &Path) -> Result<PathBuf> {
    let root_raw = path_display_key(root);
    let target_raw = path_display_key(target);
    #[cfg(windows)]
    let (root_key, target_key, sep) = (root_raw.to_ascii_lowercase(), target_raw.to_ascii_lowercase(), '\\');
    #[cfg(not(windows))]
    let (root_key, target_key, sep) = (root_raw.clone(), target_raw.clone(), '/');
    if target_key == root_key {
        return Ok(PathBuf::new());
    }
    let prefix = format!("{root_key}{sep}");
    if !target_key.starts_with(&prefix) || root_raw.len() + 1 > target_raw.len() {
        return Err("The path is outside the selected folder".into());
    }
    Ok(PathBuf::from(&target_raw[root_raw.len() + 1..]))
}

/// Lexical containment, then every existing link. A link whose target leaves the
/// selected folder is refused. A missing tail may still be created inside it.
fn ensure_inside_selected(base: &Path, target: &Path) -> Result<()> {
    use std::path::Component;
    let root = std::fs::canonicalize(base).map_err(|error| format!("The selected folder could not be opened: {error}"))?;
    if !root.is_dir() {
        return Err("Choose a folder".into());
    }
    let target = normalize_components(target);
    if !path_contained(&root, &target) {
        return Err("The path is outside the selected folder".into());
    }
    let suffix = path_suffix(&root, &target)?;
    let mut cursor = root.clone();
    for part in suffix.components() {
        match part {
            Component::Normal(_) => {}
            _ => return Err("The path is outside the selected folder".into()),
        }
        cursor.push(part);
        match host_fs::desktop_metadata(&cursor, false) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("Invalid native path: {error}")),
            Ok(meta) => {
                if metadata_is_link(&meta) {
                    let resolved = std::fs::canonicalize(&cursor).map_err(|error| format!("Invalid native path: {error}"))?;
                    if !path_contained(&root, &resolved) {
                        return Err("Refusing to follow a link out of the selected folder".into());
                    }
                    cursor = resolved;
                }
            }
        }
    }
    Ok(())
}

/// The selected folder supplies the base. Absolute and relative names both have
/// to stay inside that folder.
fn native_path(base: &Path, name: &str) -> Result<PathBuf> {
    use std::path::Component;
    if name.chars().any(char::is_control) || name.starts_with("content://") {
        return Err("Invalid native path".into());
    }
    #[cfg(windows)]
    let native = name.replace('/', "\\");
    #[cfg(not(windows))]
    let native = name;
    let path = Path::new(&native);
    #[cfg(windows)]
    {
        if (path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_))))
            && !host_fs::windows_folder_path(&native) {
            return Err("Use a fully qualified drive or UNC path".into());
        }
        for component in path.components() {
            if let Component::Normal(part) = component {
                let part = part.to_string_lossy();
                if part.contains([':', '*', '?', '"', '<', '>', '|']) {
                    return Err("Invalid Windows path component".into());
                }
                let stem = part.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
                let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
                    || ["COM", "LPT"].iter().any(|prefix| stem.strip_prefix(prefix).is_some_and(|suffix| {
                        matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
                    }));
                if device {
                    return Err("A file path cannot name a Windows device".into());
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = Component::RootDir;
    let target = if path.is_absolute() { path.to_path_buf() } else if name.is_empty() {
        base.to_path_buf()
    } else {
        base.join(path)
    };
    let target = if target.is_absolute() {
        target
    } else {
        std::path::absolute(&target).map_err(|error| format!("Invalid native path: {error}"))?
    };
    let target = normalize_components(&target);
    ensure_inside_selected(base, &target)?;
    host_fs::widen_desktop_path(&target).map_err(|error| format!("Invalid native path: {error}"))
}

fn file_result<T>(path: &Path, result: std::io::Result<T>) -> Result<T> {
    result.map_err(|error| format!("{}: {error}", path.display()))
}

enum LinkEnd {
    Follow,
    Stat,
    Refuse,
}

fn canonical_root(base: &Path) -> Result<PathBuf> {
    let root = std::fs::canonicalize(base).map_err(|error| format!("The selected folder could not be opened: {error}"))?;
    if !root.is_dir() {
        return Err("Choose a folder".into());
    }
    Ok(root)
}

fn open_root_dir(root: &Path) -> Result<std::fs::File> {
    host_fs::desktop_options().read(true).open(root).map_err(|error| format!("The selected folder could not be opened: {error}"))
}

fn normal_parts(relative: &Path) -> Result<Vec<std::ffi::OsString>> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_os_string()),
            std::path::Component::CurDir => {}
            _ => return Err("The path is outside the selected folder".into()),
        }
    }
    Ok(parts)
}

fn segment_bytes(part: &std::ffi::OsStr) -> Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = part.as_bytes();
        if bytes.is_empty() || bytes.contains(&0) || bytes.contains(&b'/') {
            return Err("The path is outside the selected folder".into());
        }
        return Ok(bytes.to_vec());
    }
    #[cfg(windows)]
    {
        let text = part.to_str().ok_or("Invalid native path")?;
        if text.is_empty() || text.contains(['/', '\\', '\0']) {
            return Err("The path is outside the selected folder".into());
        }
        return Ok(text.as_bytes().to_vec());
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = part;
        Err("The path is outside the selected folder".into())
    }
}

fn is_eloop(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(40) | Some(62))
}

fn resolve_link(root: &Path, directory: &Path, target: &Path) -> Result<PathBuf> {
    let joined = if target.is_absolute() {
        normalize_components(target)
    } else {
        normalize_components(&directory.join(target))
    };
    if !path_contained(root, &joined) {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    Ok(joined)
}

#[cfg(unix)]
fn read_link_at(dir: &std::fs::File, name: &[u8]) -> Result<PathBuf> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let c_name = std::ffi::CString::new(name).map_err(|_| "Invalid native path".to_string())?;
    let mut buf = vec![0u8; 4096];
    unsafe extern "C" {
        fn readlinkat(fd: i32, path: *const std::ffi::c_char, buf: *mut u8, size: usize) -> isize;
    }
    let got = unsafe { readlinkat(dir.as_raw_fd(), c_name.as_ptr(), buf.as_mut_ptr(), buf.len() - 1) };
    if got < 0 {
        return Err(format!("Invalid native path: {}", std::io::Error::last_os_error()));
    }
    let n = got as usize;
    if n >= buf.len() - 1 {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..n])))
}

#[cfg(windows)]
fn reparse_target(file: &std::fs::File) -> Result<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    let mut buf = vec![0u8; 16 * 1024];
    unsafe extern "system" {
        fn DeviceIoControl(handle: *mut std::ffi::c_void, code: u32, input: *const std::ffi::c_void, input_size: u32, output: *mut std::ffi::c_void, output_size: u32, returned: *mut u32, overlapped: *mut std::ffi::c_void) -> i32;
    }
    let mut returned = 0u32;
    let ok = unsafe { DeviceIoControl(file.as_raw_handle(), 0x0009_00A8, std::ptr::null(), 0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut returned, std::ptr::null_mut()) };
    if ok == 0 || returned < 16 {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    let n = (returned as usize).min(buf.len());
    let tag = u32::from_le_bytes(buf[0..4].try_into().unwrap_or([0; 4]));
    let (start, offset, length) = if tag == 0xA000_000C && n >= 20 {
        (20usize, u16::from_le_bytes(buf[12..14].try_into().unwrap_or([0; 2])) as usize, u16::from_le_bytes(buf[14..16].try_into().unwrap_or([0; 2])) as usize)
    } else if tag == 0xA000_0003 && n >= 16 {
        (16usize, u16::from_le_bytes(buf[8..10].try_into().unwrap_or([0; 2])) as usize, u16::from_le_bytes(buf[10..12].try_into().unwrap_or([0; 2])) as usize)
    } else {
        return Err("Refusing to follow a link out of the selected folder".into());
    };
    let Some(at) = start.checked_add(offset) else { return Err("Refusing to follow a link out of the selected folder".into()) };
    let Some(end) = at.checked_add(length) else { return Err("Refusing to follow a link out of the selected folder".into()) };
    if end > n || length % 2 != 0 {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    let units: Vec<u16> = buf[at..end].chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let text = String::from_utf16_lossy(&units);
    let text = text.strip_prefix(r"\??\").or_else(|| text.strip_prefix(r"\\?\")).unwrap_or(&text);
    if text.is_empty() {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    Ok(PathBuf::from(text))
}

fn link_target(dir: &std::fs::File, name: &[u8], opened: &std::fs::File) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        let _ = opened;
        return read_link_at(dir, name);
    }
    #[cfg(windows)]
    {
        let _ = (dir, name);
        return reparse_target(opened);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (dir, name, opened);
        Err("Refusing to follow a link out of the selected folder".into())
    }
}

enum Walked {
    Opened(std::fs::File),
    Follow(PathBuf),
}

fn walk_once(root: &Path, relative: &Path, flags: i32, end: &LinkEnd) -> Result<Walked> {
    let parts = normal_parts(relative)?;
    let mut dir = open_root_dir(root)?;
    if parts.is_empty() {
        return Ok(Walked::Opened(dir));
    }
    let mut at = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        let name = segment_bytes(part)?;
        let part_flags = if last { flags | 0x20000 } else { 0x10000 | 0x20000 };
        let follow_final = matches!(end, LinkEnd::Follow) && (flags & 0x40) == 0;
        match host_fs::open_at(&dir, &name, part_flags) {
            Ok(file) => {
                let meta = file.metadata().map_err(|error| format!("Invalid native path: {error}"))?;
                if metadata_is_link(&meta) {
                    if last && matches!(end, LinkEnd::Stat) {
                        return Ok(Walked::Opened(file));
                    }
                    if last && !follow_final {
                        return Err("Refusing to follow a link out of the selected folder".into());
                    }
                    let target = link_target(&dir, &name, &file)?;
                    drop(file);
                    let resolved = resolve_link(root, &at, &target)?;
                    let mut rest = path_suffix(root, &resolved)?;
                    for extra in parts.iter().skip(index + 1) {
                        rest.push(extra);
                    }
                    return Ok(Walked::Follow(rest));
                }
                if last {
                    return Ok(Walked::Opened(file));
                }
                at.push(part);
                dir = file;
            }
            Err(error) if is_eloop(&error) => {
                if last && matches!(end, LinkEnd::Stat) {
                    return Err("symbolic link".into());
                }
                if last && !follow_final {
                    return Err("Refusing to follow a link out of the selected folder".into());
                }
                #[cfg(unix)]
                let target = read_link_at(&dir, &name)?;
                #[cfg(not(unix))]
                let target = PathBuf::new();
                #[cfg(not(unix))]
                let _ = (&dir, &name, target);
                #[cfg(not(unix))]
                return Err("Refusing to follow a link out of the selected folder".into());
                #[cfg(unix)]
                {
                    let resolved = resolve_link(root, &at, &target)?;
                    let mut rest = path_suffix(root, &resolved)?;
                    for extra in parts.iter().skip(index + 1) {
                        rest.push(extra);
                    }
                    return Ok(Walked::Follow(rest));
                }
            }
            Err(error) => return Err(format!("Invalid native path: {error}")),
        }
    }
    Ok(Walked::Opened(dir))
}

/// Open `target` from a handle on the selected folder. Each component is opened
/// from the parent handle and a swapped link cannot redirect the open by name.
fn confined_open(base: &Path, target: &Path, flags: i32, end: LinkEnd) -> Result<std::fs::File> {
    let root = canonical_root(base)?;
    let target = normalize_components(target);
    if !path_contained(&root, &target) {
        return Err("The path is outside the selected folder".into());
    }
    let mut relative = path_suffix(&root, &target)?;
    for hop in 0..40 {
        let _ = hop;
        match walk_once(&root, &relative, flags, &end)? {
            Walked::Opened(file) => return Ok(file),
            Walked::Follow(next) => relative = next,
        }
    }
    Err("Refusing to follow a link out of the selected folder".into())
}

fn confined_parent(base: &Path, target: &Path) -> Result<(std::fs::File, Vec<u8>)> {
    let root = canonical_root(base)?;
    let target = normalize_components(target);
    if !path_contained(&root, &target) {
        return Err("The path is outside the selected folder".into());
    }
    let relative = path_suffix(&root, &target)?;
    let parts = normal_parts(&relative)?;
    let Some(name) = parts.last() else { return Err("A filesystem root cannot be moved".into()) };
    let name = segment_bytes(name)?;
    if parts.len() == 1 {
        return Ok((open_root_dir(&root)?, name));
    }
    let mut parent = root.clone();
    for part in &parts[..parts.len() - 1] {
        parent.push(part);
    }
    let dir = confined_open(base, &parent, 0x10000, LinkEnd::Follow)?;
    Ok((dir, name))
}

/// Generic non-blocking bit. The host maps it before the open.
/// Regular files ignore it, so a pipe in the selected folder cannot stall the open.
fn nonblocking_read_flags() -> i32 {
    #[cfg(unix)]
    {
        0x800
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn regular_file(base: &Path, path: &Path, write: bool) -> Result<std::fs::File> {
    // Write-only open of a pipe waits for a reader. The nonblocking bit returns
    // that open at once; a regular file ignores it. The type check is next.
    let flags = if write { 1 | nonblocking_read_flags() } else { nonblocking_read_flags() };
    let file = confined_open(base, path, flags, LinkEnd::Follow)?;
    let meta = file.metadata().map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata_is_link(&meta) || !meta.is_file() {
        return Err(format!("Select a regular file at {}", path.display()));
    }
    Ok(file)
}

fn create_regular_file(base: &Path, path: &Path) -> Result<std::fs::File> {
    let file = confined_open(base, path, 1 | 0x40 | 0x80, LinkEnd::Refuse)?;
    let meta = file.metadata().map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata_is_link(&meta) || !meta.is_file() {
        return Err(format!("Select a regular file at {}", path.display()));
    }
    Ok(file)
}

struct ConfinedEntry {
    path: PathBuf,
    directory: bool,
    file: bool,
}

fn confined_entries(base: &Path, dir_path: &Path) -> Result<Box<dyn Iterator<Item = Result<ConfinedEntry>>>> {
    let dir = confined_open(base, dir_path, 0x10000, LinkEnd::Follow)?;
    read_bound_directory(&dir, dir_path)
}

#[cfg(unix)]
fn read_bound_directory(dir: &std::fs::File, logical: &Path) -> Result<Box<dyn Iterator<Item = Result<ConfinedEntry>>>> {
    use std::os::fd::AsRawFd;
    let link = if cfg!(any(target_os = "linux", target_os = "android")) {
        format!("/proc/self/fd/{}", dir.as_raw_fd())
    } else {
        format!("/dev/fd/{}", dir.as_raw_fd())
    };
    let listed = std::fs::read_dir(&link).map_err(|error| format!("{}: {error}", logical.display()))?;
    let logical = logical.to_path_buf();
    Ok(Box::new(listed.map(move |entry| {
        let entry = entry.map_err(|error| format!("{}: {error}", logical.display()))?;
        let kind = entry.file_type().map_err(|error| format!("{}: {error}", logical.display()))?;
        Ok(ConfinedEntry {
            path: logical.join(entry.file_name()),
            directory: kind.is_dir() && !kind.is_symlink(),
            file: kind.is_file() && !kind.is_symlink(),
        })
    })))
}

#[cfg(windows)]
fn read_bound_directory(dir: &std::fs::File, logical: &Path) -> Result<Box<dyn Iterator<Item = Result<ConfinedEntry>>>> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct IoStatus { status: usize, information: usize }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryDirectoryFile(handle: *mut std::ffi::c_void, event: *mut std::ffi::c_void, apc: *mut std::ffi::c_void, ctx: *mut std::ffi::c_void, io: *mut IoStatus, info: *mut u8, length: u32, class: i32, single: u8, name: *mut std::ffi::c_void, restart: u8) -> i32;
    }
    let mut entries = Vec::new();
    let mut aligned = vec![0u64; 8 * 1024];
    let buffer = unsafe { std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), aligned.len() * 8) };
    let mut restart = 1u8;
    loop {
        if entries.len() > DIRECTORY_WALK_BUDGET {
            return Err("directory listing exceeds the directory budget".into());
        }
        let mut io = IoStatus { status: 0, information: 0 };
        let status = unsafe {
            NtQueryDirectoryFile(dir.as_raw_handle(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut io, buffer.as_mut_ptr(), buffer.len() as u32, 1, 0, std::ptr::null_mut(), restart)
        };
        restart = 0;
        if status == 0x8000_0006u32 as i32 {
            break;
        }
        if status < 0 {
            return Err(format!("{}: directory listing failed ({status:#x})", logical.display()));
        }
        let mut offset = 0usize;
        loop {
            if offset.saturating_add(64) > buffer.len() {
                break;
            }
            let next = u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap_or([0; 4]));
            let attrs = u32::from_le_bytes(buffer[offset + 56..offset + 60].try_into().unwrap_or([0; 4]));
            let name_len = u32::from_le_bytes(buffer[offset + 60..offset + 64].try_into().unwrap_or([0; 4])) as usize;
            let Some(name_at) = offset.checked_add(64) else { break };
            let Some(name_end) = name_at.checked_add(name_len) else { break };
            if name_end > buffer.len() || name_len % 2 != 0 {
                break;
            }
            let units: Vec<u16> = buffer[name_at..name_end].chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            let name = String::from_utf16_lossy(&units);
            if name != "." && name != ".." {
                let reparse = attrs & 0x400 != 0;
                entries.push(ConfinedEntry {
                    path: logical.join(&name),
                    directory: attrs & 0x10 != 0 && !reparse,
                    file: attrs & 0x10 == 0 && !reparse,
                });
            }
            if next == 0 {
                break;
            }
            let Some(next_at) = offset.checked_add(next as usize) else { break };
            if next_at <= offset || next_at > buffer.len() {
                break;
            }
            offset = next_at;
        }
    }
    Ok(Box::new(entries.into_iter().map(Ok)))
}

#[cfg(not(any(unix, windows)))]
fn read_bound_directory(dir: &std::fs::File, logical: &Path) -> Result<Box<dyn Iterator<Item = Result<ConfinedEntry>>>> {
    let _ = (dir, logical);
    Err("directory listing is unavailable on this host".into())
}

fn mkdir_inside(base: &Path, target: &Path, parents: bool) -> Result<()> {
    let root = canonical_root(base)?;
    let target = normalize_components(target);
    if !path_contained(&root, &target) {
        return Err("The path is outside the selected folder".into());
    }
    let relative = path_suffix(&root, &target)?;
    mkdir_relative(&root, &relative, parents, 0)
}

fn mkdir_relative(root: &Path, relative: &Path, parents: bool, hops: u32) -> Result<()> {
    if hops > 40 {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    let parts = normal_parts(relative)?;
    if parts.is_empty() {
        return Ok(());
    }
    let mut dir = open_root_dir(root)?;
    let mut at = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        let name = segment_bytes(part)?;
        match host_fs::open_at(&dir, &name, 0x10000 | 0x20000) {
            Ok(child) => {
                let meta = child.metadata().map_err(|error| format!("Directory could not be created: {error}"))?;
                if metadata_is_link(&meta) {
                    let target = link_target(&dir, &name, &child)?;
                    let resolved = resolve_link(root, &at, &target)?;
                    let mut rest = path_suffix(root, &resolved)?;
                    for extra in parts.iter().skip(index + 1) {
                        rest.push(extra);
                    }
                    return mkdir_relative(root, &rest, parents, hops + 1);
                }
                if last && !parents {
                    return Err(format!("Directory could not be created: {}", std::io::Error::from_raw_os_error(17)));
                }
                at.push(part);
                dir = child;
            }
            Err(error) if is_eloop(&error) => {
                #[cfg(unix)]
                {
                    let target = read_link_at(&dir, &name)?;
                    let resolved = resolve_link(root, &at, &target)?;
                    let mut rest = path_suffix(root, &resolved)?;
                    for extra in parts.iter().skip(index + 1) {
                        rest.push(extra);
                    }
                    return mkdir_relative(root, &rest, parents, hops + 1);
                }
                #[cfg(not(unix))]
                {
                    let _ = &name;
                    return Err("Refusing to follow a link out of the selected folder".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && (parents || last) => {
                let created = host_fs::open_at(&dir, &name, 0x40 | 0x80 | 0x10000 | 0x20000).map_err(|error| format!("Directory could not be created: {error}"))?;
                at.push(part);
                dir = created;
            }
            Err(error) => return Err(format!("Directory could not be created: {error}")),
        }
    }
    let _ = dir;
    Ok(())
}

fn rename_leaf(src_dir: &std::fs::File, src: &[u8], dst_dir: &std::fs::File, dst: &[u8]) -> std::io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    {
        use std::os::fd::AsRawFd;
        let src_c = std::ffi::CString::new(src).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path segment"))?;
        let dst_c = std::ffi::CString::new(dst).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path segment"))?;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let status = {
            unsafe extern "C" {
                fn renameat2(oldfd: i32, old: *const std::ffi::c_char, newfd: i32, new: *const std::ffi::c_char, flags: u32) -> i32;
            }
            unsafe { renameat2(src_dir.as_raw_fd(), src_c.as_ptr(), dst_dir.as_raw_fd(), dst_c.as_ptr(), 1) }
        };
        #[cfg(target_os = "macos")]
        let status = {
            unsafe extern "C" {
                fn renameatx_np(oldfd: i32, old: *const std::ffi::c_char, newfd: i32, new: *const std::ffi::c_char, flags: u32) -> i32;
            }
            unsafe { renameatx_np(src_dir.as_raw_fd(), src_c.as_ptr(), dst_dir.as_raw_fd(), dst_c.as_ptr(), 4) }
        };
        return if status == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) };
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let source = open_delete_child(src_dir, src)?;
        let name = std::str::from_utf8(dst).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path segment"))?;
        let units: Vec<u16> = name.encode_utf16().collect();
        let ptr = std::mem::size_of::<usize>();
        let name_at = ptr + ptr + 4;
        let mut buf = vec![0u8; name_at + units.len() * 2];
        let root = dst_dir.as_raw_handle() as usize;
        buf[ptr..ptr + ptr].copy_from_slice(&root.to_ne_bytes());
        buf[ptr + ptr..ptr + ptr + 4].copy_from_slice(&((units.len() * 2) as u32).to_le_bytes());
        for (index, unit) in units.iter().enumerate() {
            let at = name_at + index * 2;
            buf[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        #[repr(C)]
        struct IoStatus { status: usize, information: usize }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtSetInformationFile(file: *mut std::ffi::c_void, io: *mut IoStatus, info: *const std::ffi::c_void, length: u32, class: i32) -> i32;
            fn RtlNtStatusToDosError(status: i32) -> u32;
        }
        let mut io = IoStatus { status: 0, information: 0 };
        let status = unsafe { NtSetInformationFile(source.as_raw_handle(), &mut io, buf.as_ptr().cast(), buf.len() as u32, 10) };
        return if status >= 0 {
            Ok(())
        } else {
            let code = unsafe { RtlNtStatusToDosError(status) };
            Err(if code == 0 { std::io::Error::other("directory-relative move failed") } else { std::io::Error::from_raw_os_error(code as i32) })
        };
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android", target_os = "macos")))]
    {
        let _ = (src_dir, src, dst_dir, dst);
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "A native no-replace move is unavailable on this host"))
    }
}

#[cfg(windows)]
fn open_delete_child(parent: &std::fs::File, name: &[u8]) -> std::io::Result<std::fs::File> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    let text = std::str::from_utf8(name).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path segment"))?;
    let mut units: Vec<u16> = text.encode_utf16().collect();
    if units.is_empty() || units.len() > 255 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path segment"));
    }
    units.push(0);
    #[repr(C)]
    struct UnicodeString { length: u16, maximum_length: u16, buffer: *mut u16 }
    #[repr(C)]
    struct ObjectAttributes { length: u32, root_directory: *mut std::ffi::c_void, object_name: *mut UnicodeString, attributes: u32, security_descriptor: *mut std::ffi::c_void, security_quality_of_service: *mut std::ffi::c_void }
    #[repr(C)]
    struct IoStatusBlock { status: usize, information: usize }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtCreateFile(handle: *mut *mut std::ffi::c_void, access: u32, object: *const ObjectAttributes, io: *mut IoStatusBlock, allocation: *const i64, attributes: u32, share: u32, disposition: u32, options: u32, ea: *mut std::ffi::c_void, ea_length: u32) -> i32;
        fn RtlNtStatusToDosError(status: i32) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(handle: isize) -> i32;
    }
    let mut unicode = UnicodeString { length: ((units.len() - 1) * 2) as u16, maximum_length: (units.len() * 2) as u16, buffer: units.as_mut_ptr() };
    let object = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut unicode,
        attributes: 0x40,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut io_status = IoStatusBlock { status: 0, information: 0 };
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        NtCreateFile(&mut handle, 0x0001_0000 | 0x0000_0080 | 0x0010_0000, &object, &mut io_status, std::ptr::null(), 0x80, 0x1 | 0x2 | 0x4, 1, 0x0000_0020 | 0x0020_0000, std::ptr::null_mut(), 0)
    };
    if status < 0 || handle.is_null() {
        if !handle.is_null() {
            unsafe { CloseHandle(handle as isize); }
        }
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(if code == 0 { std::io::Error::other("directory-relative open failed") } else { std::io::Error::from_raw_os_error(code as i32) });
    }
    Ok(unsafe { std::fs::File::from_raw_handle(handle) })
}

fn move_inside(base: &Path, source: &Path, destination: &Path) -> Result<()> {
    let (src_dir, src_name) = confined_parent(base, source)?;
    let (dst_dir, dst_name) = confined_parent(base, destination)?;
    rename_leaf(&src_dir, &src_name, &dst_dir, &dst_name).map_err(|error| {
        format!("Could not move {} to {}: {error}", source.display(), destination.display())
    })
}

fn list_native(base: &Path, path: &Path, args: &Json) -> Result<String> {
    let offset = file_number(args, "offset", 0)?;
    let limit = file_number(args, "limit", 256)?.clamp(1, DIRECTORY_PAGE_ENTRIES) as usize;
    let started = std::time::Instant::now();
    let mut entries = confined_entries(base, path)?;
    for _ in 0..offset {
        if let Some(reason) = operation_stopped(started) {
            return Err(reason.into());
        }
        match entries.next() {
            Some(entry) => { entry?; }
            None => break,
        }
    }
    let mut names = Vec::new();
    for entry in entries.by_ref().take(limit) {
        let entry = entry?;
        let name = entry.path.file_name().unwrap_or_default();
        names.push(format!("{}{}", name.to_string_lossy(), if entry.directory { "/" } else { "" }));
    }
    let next = if entries.next().transpose()?.is_some() {
        Some(offset.checked_add(names.len() as u64).ok_or("directory offset exceeds the supported range")?)
    } else {
        None
    };
    if get(args, "offset") == &Null && get(args, "limit") == &Null {
        let mut listing = names.join("\n");
        if let Some(next) = next {
            listing.push_str(&format!("\nContinue with offset {next}."));
        }
        return Ok(listing);
    }
    Ok(obj(vec![
        ("entries", Arr(names.iter().map(|name| s(name)).collect())),
        ("next_offset", next.map(file_quantity).unwrap_or(Null)),
    ]).string())
}

fn read_native(base: &Path, path: &Path, args: &Json, editor: bool) -> Result<String> {
    use std::io::{Seek, SeekFrom};
    let offset = file_number(args, "offset", 0)?;
    let length = file_number(args, "length", FILE_PAGE_BYTES)?.clamp(1, FILE_PAGE_BYTES);
    let encoding = file_arg(args, "encoding")?;
    if !matches!(encoding, "" | "auto" | "base64" | "utf8") {
        return Err("Choose auto, utf8 or base64 encoding".into());
    }
    let started = std::time::Instant::now();
    let mut file = regular_file(base, path, false)?;
    file_result(path, file.seek(SeekFrom::Start(offset)))?;
    let mut bytes = Vec::new();
    file_result(path, file.take(length + 1).read_to_end(&mut bytes))?;
    let more = bytes.len() as u64 > length;
    bytes.truncate(length as usize);
    fn as_text(bytes: &[u8]) -> Option<&str> {
        std::str::from_utf8(bytes).ok().filter(|value| !value.contains('\0'))
    }
    if editor {
        if more {
            return Err("The text editor requires the entire file and can hold up to 1 MiB. Read the file in ranges instead.".into());
        }
        return as_text(&bytes).map(str::to_owned).ok_or_else(|| "The text editor requires UTF-8 contents".into());
    }
    if encoding != "base64" && more {
        // Leave an incomplete trailing code point for the next byte range.
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() && error.valid_up_to() != 0 {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    let value = as_text(&bytes).filter(|_| encoding != "base64");
    if encoding == "utf8" && value.is_none() {
        return Err("This range is not UTF-8 text. Retry in base64 or start at a UTF-8 character boundary.".into());
    }
    if !more && offset == 0 && get(args, "offset") == &Null && get(args, "length") == &Null && encoding.is_empty() {
        if let Some(value) = value { return Ok(value.into()); }
    }
    if let Some(reason) = operation_stopped(started) {
        return Err(reason.into());
    }
    let next = if more { file_quantity(offset.checked_add(bytes.len() as u64).ok_or("File range exceeds the supported offset")?) } else { Null };
    Ok(obj(vec![
        ("offset", file_quantity(offset)),
        ("bytes", file_quantity(bytes.len() as u64)),
        ("next_offset", next),
        ("encoding", s(if value.is_some() { "utf8" } else { "base64" })),
        ("data", s(&value.map(str::to_owned).unwrap_or_else(|| network::b64(&bytes)))),
    ]).string())
}

fn write_native(base: &Path, path: &Path, args: &Json, create: bool) -> Result<String> {
    use std::io::{Seek, SeekFrom};
    let bytes = match (get(args, "data"), get(args, "content")) {
        (Null, content) => {
            if content == &Null && !create { return Err("Enter file contents".into()); }
            let text = file_arg(args, "content")?;
            if text.len() as u64 > host_byte_budget() {
                return Err("File contents exceed the memory budget".into());
            }
            text.as_bytes().to_vec()
        }
        (data, Null) => {
            let encoded = data.text()?;
            if encoded.len() as u64 > host_byte_budget().saturating_mul(2) {
                return Err("File contents exceed the memory budget".into());
            }
            let bytes = network::unbase64(encoded)?;
            if bytes.len() as u64 > host_byte_budget() {
                return Err("File contents exceed the memory budget".into());
            }
            bytes
        }
        _ => return Err("Provide either UTF-8 content or base64 data".into()),
    };
    let offset = file_number(args, "offset", 0)?;
    let end = offset.checked_add(bytes.len() as u64).ok_or("File range exceeds the supported offset")?;
    let ranged = get(args, "offset") != &Null;
    let mut file = if create {
        create_regular_file(base, path)?
    } else {
        match regular_file(base, path, true) {
            Ok(file) => file,
            Err(error) if error.contains("os error 2") || error.contains("os error 3") => {
                match create_regular_file(base, path) {
                    Ok(file) => file,
                    Err(_) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    };
    file_result(path, file.seek(SeekFrom::Start(offset)))?;
    file_result(path, file.write_all(&bytes))?;
    if !ranged {
        file_result(path, file.set_len(end))?;
    }
    file_result(path, file.sync_all())?;
    Ok(format!("Wrote {} bytes at offset {offset} in {}", bytes.len(), path.display()))
}

fn native_stat(base: &Path, path: &Path) -> Result<String> {
    let file = match confined_open(base, path, nonblocking_read_flags(), LinkEnd::Stat) {
        Ok(file) => file,
        Err(error) if error.contains("symbolic link") => {
            return Ok(obj(vec![
                ("path", s(&path.to_string_lossy())),
                ("kind", s("link")),
                ("bytes", file_quantity(0)),
                ("readonly", Bool(false)),
                ("modified_unix_seconds", Null),
            ]).string());
        }
        Err(error) => return Err(error),
    };
    let metadata = file.metadata().map_err(|error| format!("{}: {error}", path.display()))?;
    let kind = if metadata_is_link(&metadata) { "link" } else if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        // The descriptor is already open. Refuse a pipe or other non-regular
        // node here, before any read, so the operation cannot stall on it.
        #[cfg(unix)]
        {
            return Err(format!("Select a regular file at {}", path.display()));
        }
        #[cfg(not(unix))]
        {
            "special"
        }
    };
    let modified = metadata.modified().ok().map(|time| {
        let seconds = match time.duration_since(std::time::UNIX_EPOCH) {
            Ok(value) => i128::from(value.as_secs()),
            Err(error) => -i128::from(error.duration().as_secs()) - i128::from(error.duration().subsec_nanos() != 0),
        };
        Num(seconds.to_string())
    });
    Ok(obj(vec![
        ("path", s(&path.to_string_lossy())),
        ("kind", s(kind)),
        ("bytes", file_quantity(metadata.len())),
        ("readonly", Bool(metadata.permissions().readonly())),
        ("modified_unix_seconds", modified.unwrap_or(Null)),
    ]).string())
}

fn copy_native(base: &Path, source: &Path, destination: &Path) -> Result<()> {
    enum Work {
        Entry(PathBuf, PathBuf),
        Directory(Box<dyn Iterator<Item = Result<ConfinedEntry>>>, PathBuf),
    }
    if same_or_inside(destination, source) {
        return Err("A directory cannot be copied into itself".into());
    }
    let started = std::time::Instant::now();
    let mut pending = vec![Work::Entry(source.to_path_buf(), destination.to_path_buf())];
    while let Some(item) = pending.pop() {
        if let Some(reason) = operation_stopped(started) {
            return Err(reason.into());
        }
        if pending.len() > 100_000 {
            return Err("Copy exceeds the directory budget".into());
        }
        match item {
            Work::Entry(from, to) => {
                let opened = confined_open(base, &from, 0, LinkEnd::Stat);
                let link_error = opened.as_ref().err().map(|error| error.contains("symbolic link")).unwrap_or(false);
                if link_error {
                    copy_link(base, &from, &to)?;
                    continue;
                }
                let file = opened?;
                let metadata = file.metadata().map_err(|error| format!("Copy {} to {} failed: {error}", from.display(), to.display()))?;
                if metadata_is_link(&metadata) {
                    drop(file);
                    copy_link(base, &from, &to)?;
                } else if metadata.is_dir() {
                    drop(file);
                    let entries = confined_entries(base, &from)?;
                    mkdir_inside(base, &to, false)?;
                    pending.push(Work::Directory(entries, to));
                } else if metadata.is_file() {
                    drop(file);
                    let mut input = regular_file(base, &from, false)?;
                    let mut output = create_regular_file(base, &to)?;
                    let mut buf = [0u8; 65536];
                    loop {
                        if let Some(reason) = operation_stopped(started) {
                            return Err(reason.into());
                        }
                        let count = input.read(&mut buf).map_err(|error| format!("Copy {} to {} failed: {error}", from.display(), to.display()))?;
                        if count == 0 {
                            break;
                        }
                        output.write_all(&buf[..count]).map_err(|error| format!("Copy {} to {} failed: {error}", from.display(), to.display()))?;
                    }
                    output.sync_all().map_err(|error| format!("Copy {} to {} failed: {error}", from.display(), to.display()))?;
                } else {
                    return Err(format!("Cannot copy special filesystem entry {}", from.display()));
                }
            }
            Work::Directory(mut entries, to) => {
                if let Some(entry) = entries.next() {
                    let entry = entry?;
                    let name = entry.path.file_name().ok_or("The directory returned an unnamed entry")?;
                    let child = to.join(name);
                    pending.push(Work::Directory(entries, to));
                    pending.push(Work::Entry(entry.path, child));
                }
            }
        }
    }
    Ok(())
}

fn copy_link(base: &Path, from: &Path, to: &Path) -> Result<()> {
    #[cfg(unix)]
    let link = {
        let (src_dir, src_name) = confined_parent(base, from)?;
        read_link_at(&src_dir, &src_name)?
    };
    #[cfg(windows)]
    let link = {
        let opened = confined_open(base, from, 0, LinkEnd::Stat)?;
        reparse_target(&opened)?
    };
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (base, from);
        return Err(format!("Cannot copy special filesystem entry {}", to.display()));
    }
    let (dst_dir, dst_name) = confined_parent(base, to)?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::ffi::OsStrExt;
        let target = std::ffi::CString::new(link.as_os_str().as_bytes()).map_err(|_| "Invalid native path".to_string())?;
        let name = std::ffi::CString::new(dst_name).map_err(|_| "Invalid native path".to_string())?;
        unsafe extern "C" {
            fn symlinkat(target: *const std::ffi::c_char, dirfd: i32, name: *const std::ffi::c_char) -> i32;
        }
        let status = unsafe { symlinkat(target.as_ptr(), dst_dir.as_raw_fd(), name.as_ptr()) };
        if status != 0 {
            return Err(format!("Copy {} to {} failed: {}", from.display(), to.display(), std::io::Error::last_os_error()));
        }
        return Ok(());
    }
    #[cfg(windows)]
    {
        let _ = (dst_dir, dst_name);
        let link_result = std::os::windows::fs::symlink_file(&link, to).or_else(|_| std::os::windows::fs::symlink_dir(&link, to));
        return link_result.map_err(|error| format!("Copy {} to {} failed: {error}", from.display(), to.display()));
    }
}

fn same_or_inside(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = windows_path_key(path);
        let root = windows_path_key(root);
        path == root || path.strip_prefix(&root).is_some_and(|suffix| suffix.starts_with('\\'))
    }
    #[cfg(not(windows))]
    path.starts_with(root)
}

fn move_to_review(base: &Path, path: &Path, workspace: &Path) -> Result<PathBuf> {
    let name = path.file_name().ok_or("A filesystem root cannot be moved")?;
    let (src_dir, src_name) = confined_parent(base, path)?;
    let root = canonical_root(workspace)?;
    let review = root.join("review");
    if same_or_inside(path, &review) || same_or_inside(&review, path) {
        return Err("The source overlaps the workspace review directory".into());
    }
    match std::fs::create_dir(&review) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return file_result(&review, Err(error)),
    }
    let metadata = file_result(&review, host_fs::desktop_metadata(&review, false))?;
    if metadata.is_symlink() || !metadata.is_dir() || !same_or_inside(&file_result(&review, review.canonicalize())?, &root) {
        return Err("The workspace review location must be a regular directory inside the workspace".into());
    }
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
    let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let slot_name = format!("{stamp}-{}-{serial}", std::process::id());
    let review_dir = open_root_dir(&review)?;
    let slot = host_fs::open_at(&review_dir, slot_name.as_bytes(), 0x40 | 0x80 | 0x10000 | 0x20000).map_err(|error| format!("review directory: {error}"))?;
    let dest_name = segment_bytes(name)?;
    rename_leaf(&src_dir, &src_name, &slot, &dest_name).map_err(|error| {
        format!("Could not move {} to review at {}: {error}", path.display(), review.join(&slot_name).display())
    })?;
    Ok(review.join(slot_name).join(name))
}

fn file_contains(base: &Path, path: &Path, query: &[u8], prefix: &[usize]) -> Result<bool> {
    let mut file = regular_file(base, path, false)?;
    let mut buffer = [0u8; 65536];
    let mut matched = 0;
    let started = std::time::Instant::now();
    loop {
        if let Some(reason) = operation_stopped(started) {
            return Err(reason.into());
        }
        let count = file_result(path, file.read(&mut buffer))?;
        if count == 0 { return Ok(false); }
        for &byte in &buffer[..count] {
            while matched > 0 && query[matched] != byte {
                matched = prefix[matched - 1];
            }
            if query[matched] == byte {
                matched += 1;
                if matched == query.len() { return Ok(true); }
            }
        }
    }
}

fn search_native(base: &Path, root: &Path, args: &Json) -> Result<String> {
    let query = file_arg(args, "query")?;
    if query.is_empty() {
        return Err("Enter a search query".into());
    }
    let mode = file_arg(args, "mode")?;
    if !matches!(mode, "" | "name" | "content" | "both") {
        return Err("Choose name, content or both for search mode".into());
    }
    let needle = query.as_bytes();
    let mut prefix = Vec::new();
    if matches!(mode, "content" | "both") {
        prefix.resize(needle.len(), 0);
        for i in 1..needle.len() {
            let mut length = prefix[i - 1];
            while length != 0 && needle[length] != needle[i] {
                length = prefix[length - 1];
            }
            prefix[i] = length + usize::from(needle[length] == needle[i]);
        }
    }
    let offset = file_number(args, "offset", 0)?;
    let limit = file_number(args, "limit", 256)?.clamp(1, DIRECTORY_PAGE_ENTRIES) as usize;
    let started = std::time::Instant::now();
    let mut directories: Vec<Box<dyn Iterator<Item = Result<ConfinedEntry>>>> = vec![confined_entries(base, root)?];
    let mut errors = Vec::new();
    let mut error_count = 0u64;
    let mut failed = |error: String| {
        error_count += 1;
        if errors.len() < 16 { errors.push(s(&error)); }
    };
    let mut results = Vec::new();
    let mut skipped = 0;
    let mut next = Null;
    loop {
        if directories.len() > DIRECTORY_WALK_BUDGET {
            return Ok(obj(vec![
                ("matches", Arr(std::mem::take(&mut results))),
                ("next_offset", next.clone()),
                ("coverage", s("partial")),
                ("reason", s("the directory walk reached its entry budget")),
                ("error_count", file_quantity(error_count)),
                ("errors", Arr(errors)),
            ]).string());
        }
        if let Some(reason) = operation_stopped(started) {
            return Ok(obj(vec![
                ("matches", Arr(std::mem::take(&mut results))),
                ("next_offset", next.clone()),
                ("coverage", s("partial")),
                ("reason", s(reason)),
            ]).string());
        }
        let stepped = {
            let Some(directory) = directories.last_mut() else { break };
            match directory.next() {
                Some(Ok(entry)) => Some(entry),
                Some(Err(error)) => {
                    failed(error);
                    continue;
                }
                None => None,
            }
        };
        let Some(entry) = stepped else {
            directories.pop();
            continue;
        };
        if entry.directory {
            match confined_entries(base, &entry.path) {
                Ok(directory) => directories.push(directory),
                Err(error) => failed(error),
            }
        }
        let mut found = mode != "content" && entry.path.file_name().unwrap_or_default().to_string_lossy().contains(query);
        if !found && entry.file && !prefix.is_empty() {
            match file_contains(base, &entry.path, needle, &prefix) {
                Ok(matches) => found = matches,
                Err(error) => failed(error),
            }
        }
        if found {
            if skipped < offset {
                skipped += 1;
            } else if results.len() < limit {
                results.push(s(&entry.path.to_string_lossy()));
            } else {
                next = file_quantity(offset + results.len() as u64);
                break;
            }
        }
    }
    let mut fields = vec![
        ("matches", Arr(results)),
        ("next_offset", next),
        ("error_count", file_quantity(error_count)),
        ("errors", Arr(errors)),
    ];
    if error_count > 0 {
        fields.push(("coverage", s("partial")));
        fields.push(("reason", s("some directories or files could not be read")));
    }
    Ok(obj(fields).string())
}

fn device_folder_tool(effect: &Json, root: &str) -> Result<String> {
    let tool = text(effect, "tool");
    if tool == "phone_process_access" {
        return Ok(host_fs::process_access());
    }
    if tool == "phone_enable_backup" {
        #[cfg(windows)]
        return host_fs::backup_privileges().map(str::to_owned);
        #[cfg(not(windows))]
        return Err(host_fs::process_access());
    }
    let args = get(effect, "args");
    let tree = text(effect, "tree");
    let name = file_arg(args, "path")?;
    if name.is_empty() && !matches!(tool.as_str(), "phone_list" | "phone_search" | "phone_stat") {
        return Err("Enter a file or directory path".into());
    }
    let base = Path::new(if tree.is_empty() { root } else { &tree });
    let path = native_path(base, name)?;
    match tool.as_str() {
        "phone_list" => list_native(base, &path, args),
        "phone_stat" => native_stat(base, &path),
        "phone_read" => read_native(base, &path, args, boolean(effect, "editor")),
        "phone_write" | "phone_update" => write_native(base, &path, args, false),
        "phone_create" | "phone_create_file" => write_native(base, &path, args, true),
        "phone_mkdir" | "phone_create_folder" => {
            let parents = boolean(args, "parents");
            mkdir_inside(base, &path, parents)?;
            Ok(format!("Directory ready at {}", path.display()))
        }
        "phone_rename" | "phone_move" | "phone_copy" => {
            let name = file_arg(args, "destination")?;
            if name.is_empty() { return Err("Enter a destination path".into()); }
            let destination = native_path(base, name)?;
            if tool == "phone_copy" {
                copy_native(base, &path, &destination)?;
                Ok(format!("Copied {} to {}", path.display(), destination.display()))
            } else {
                move_inside(base, &path, &destination)?;
                Ok(format!("Moved {} to {}", path.display(), destination.display()))
            }
        }
        "phone_search" => search_native(base, &path, args),
        "phone_delete" => move_to_review(base, &path, Path::new(root)).map(|reviewed| reviewed.display().to_string()),
        _ => Err(format!("Unknown file tool {tool}")),
    }
}

const PROJECT_FILE_LIMIT: u64 = 5 * 1024 * 1024;

/// Same callback a local tool already returns: the core value is `output`
/// with action `tool_result`, and an error is `failure`.
fn local_operation_callback(effect: &Json, state: &Json, output: Result<Json>) -> (String, Json) {
    let mut callback = obj(vec![(
        "purpose",
        if get(effect, "purpose") == &Null { s("local_tool") } else { get(effect, "purpose").clone() },
    )]);
    for key in ["tool_ticket", "tool_binding", "editor", "editor_dialog"] {
        if get(effect, key) != &Null {
            put(&mut callback, key, get(effect, key).clone());
        }
    }
    let expected = get(state, "operation_expected");
    if get(effect, "tool_ticket") == &Null && expected != &Null {
        put(&mut callback, "tool_ticket", get(expected, "ticket").clone());
    }
    match output {
        Ok(value) => {
            put(&mut callback, "output", value);
            ("tool_result".into(), callback)
        }
        Err(error) => {
            put(&mut callback, "error", s(&error));
            ("failure".into(), callback)
        }
    }
}

fn unavailable_effect(effect: &Json) -> Json {
    obj(vec![
        ("purpose", get(effect, "purpose").clone()),
        ("error", s(&format!("{} is not available on this host yet", text(effect, "kind")))),
    ])
}

fn project_failure(effect: &Json, error: &str) -> Json {
    let mut inputs = obj(vec![("purpose", get(effect, "purpose").clone()), ("error", s(error))]);
    if get(effect, "ticket") != &Null {
        put(&mut inputs, "ticket", get(effect, "ticket").clone());
    }
    inputs
}

fn phone_result(effect: &Json, bytes: Option<Vec<u8>>) -> Json {
    obj(vec![
        ("purpose", get(effect, "purpose").clone()),
        ("found", Bool(bytes.is_some())),
        (
            "data",
            match &bytes {
                Some(bytes) => s(&network::b64(bytes)),
                None => Null,
            },
        ),
    ])
}

fn workspace_archive(effect: &Json, root: &str) -> Result<Json> {
    let (bytes, _, _) = archive::archive(Path::new(root))?;
    Ok(obj(vec![("ticket", get(effect, "ticket").clone()), ("archive", s(&network::b64(&bytes)))]))
}

fn artifact_file_result(effect: &Json, root: &str) -> Result<Json> {
    let mut request = effect.clone();
    put(&mut request, "root", s(root));
    let snapshot = application::artifact_snapshot(&request)?;
    Ok(obj(vec![
        ("ticket", get(effect, "ticket").clone()),
        ("archive", get(&snapshot, "archive").clone()),
    ]))
}

fn project_export_archive(effect: &Json, state: &mut Json, root: &str, shown: &mut dyn FnMut(&Json)) -> Result<(String, Json)> {
    let built = archive::build(&obj(vec![("root", s(root))]))?;
    let bytes = network::unbase64(&text(&built, "archive"))?;
    export_stage(effect, &bytes)?;
    let ready = core_event(state, root, "archive_ready", &obj(vec![
        ("ticket", get(effect, "ticket").clone()),
        ("files", get(&built, "files").clone()),
        ("directories", get(&built, "directories").clone()),
        ("bytes", get(&built, "bytes").clone()),
    ]))?;
    *state = ready.get("state")?.clone();
    shown(ready.get("ui")?);
    Ok((
        "export_staged".into(),
        obj(vec![("ticket", get(effect, "ticket").clone()), ("bytes", Num(bytes.len().to_string()))]),
    ))
}

fn native_project_folder(tree: &str) -> bool {
    host_fs::folder_permission(cfg!(windows), tree, false).is_ok()
}

fn directory_open_flag() -> i32 {
    #[cfg(target_os = "macos")]
    { 0x100000 }
    #[cfg(not(target_os = "macos"))]
    { 0x10000 }
}

fn windows_project_phone(effect: &Json, root: &str) -> Option<Result<Json>> {
    let tree = text(effect, "tree");
    if !native_project_folder(&tree) {
        return None;
    }
    let name = text(effect, "target");
    let name = if name.is_empty() { text(effect, "name") } else { name };
    Some(match text(effect, "phase").as_str() {
        "probe" | "verify" => read_project_file(&tree, &name).map(|bytes| phone_result(effect, bytes)),
        "write" => write_project_archive(&tree, &name, &text(effect, "archive"), root).map(|bytes| phone_result(effect, Some(bytes))),
        _ => Err("Invalid pending transfer phase".into()),
    })
}

fn windows_project_clear(state: &Json) -> bool {
    let transfer = get(state, "project_transfer");
    boolean(transfer, "phone") && native_project_folder(&text(transfer, "tree"))
}

fn single_project_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") || name.contains('\0') {
        return Err("Project file name must be a single file inside the selected folder".into());
    }
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(part)), None) if part == std::ffi::OsStr::new(name) => Ok(()),
        _ => Err("Project file name must be a single file inside the selected folder".into()),
    }
}

fn project_directory(tree: &str) -> Result<(PathBuf, std::fs::File)> {
    use host_fs::OpenOptionsExt;
    let base = Path::new(tree).canonicalize().map_err(|error| format!("The selected folder could not be opened: {error}"))?;
    if !base.is_dir() {
        return Err("Choose a folder".into());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(directory_open_flag())
        .open(&base)
        .map_err(|error| format!("The selected folder could not be opened: {error}"))?;
    Ok((base, file))
}

fn read_project_file(tree: &str, name: &str) -> Result<Option<Vec<u8>>> {
    single_project_name(name)?;
    let (base, dir) = project_directory(tree)?;
    let path = base.join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err("Refusing to follow a symlink out of the selected folder".into()),
        Ok(meta) if !meta.is_file() => return Err("Select a project file, not a folder".into()),
        Ok(meta) if meta.len() > PROJECT_FILE_LIMIT => return Err("Project file exceeds 5 MiB".into()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("The project file could not be opened: {error}")),
    }
    let file = match host_fs::open_at(&dir, name.as_bytes(), 0x20000 | nonblocking_read_flags()) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("The project file could not be opened: {error}")),
    };
    let opened = file.metadata().map_err(|error| format!("The project file could not be opened: {error}"))?;
    if opened.file_type().is_symlink() {
        return Err("Refusing to follow a symlink out of the selected folder".into());
    }
    if !opened.is_file() {
        return Err("Select a regular project file".into());
    }
    confirm_inside(&dir, &file, name)?;
    let mut bytes = Vec::new();
    file.take(PROJECT_FILE_LIMIT + 1).read_to_end(&mut bytes).map_err(|error| format!("The project file could not be read: {error}"))?;
    if bytes.len() as u64 > PROJECT_FILE_LIMIT {
        return Err("Project file exceeds 5 MiB".into());
    }
    Ok(Some(bytes))
}

fn file_index(file: &std::fs::File) -> Option<(u64, u64, u64)> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        struct Info {
            attributes: u32,
            creation_low: u32,
            creation_high: u32,
            access_low: u32,
            access_high: u32,
            write_low: u32,
            write_high: u32,
            volume: u32,
            size_high: u32,
            size_low: u32,
            links: u32,
            index_high: u32,
            index_low: u32,
        }
        unsafe extern "system" {
            fn GetFileInformationByHandle(file: *mut std::ffi::c_void, info: *mut Info) -> i32;
        }
        let mut info = std::mem::MaybeUninit::<Info>::zeroed();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return None;
        }
        let info = unsafe { info.assume_init() };
        return Some((info.volume as u64, info.index_high as u64, info.index_low as u64));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata().ok()?;
        return Some((meta.dev(), meta.ino(), 0));
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        None
    }
}

/// Keep the bytes of the handle we wrote. A later name at the same path is left alone.
fn delete_same_file(path: &Path, original: &std::fs::File) -> bool {
    let Some(original_id) = file_index(original) else { return false };
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        let Ok(opened) = std::fs::OpenOptions::new().access_mode(0x0001_0000).open(path) else { return false };
        if file_index(&opened) != Some(original_id) {
            return false;
        }
        #[repr(C)]
        struct Disposition {
            delete: u8,
        }
        unsafe extern "system" {
            fn SetFileInformationByHandle(file: *mut std::ffi::c_void, class: i32, info: *const std::ffi::c_void, size: u32) -> i32;
        }
        let info = Disposition { delete: 1 };
        return unsafe { SetFileInformationByHandle(opened.as_raw_handle(), 4, &info as *const Disposition as *const std::ffi::c_void, 1) } != 0;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Some(name) = path.file_name() else { return false };
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes.contains(&0) || bytes.contains(&b'/') {
            return false;
        }
        let Some(parent) = path.parent() else { return false };
        if parent.as_os_str().is_empty() {
            return false;
        }
        let Ok(dir) = std::fs::File::open(parent) else { return false };
        // The name is opened on the directory handle, then checked again
        // immediately before the name is removed.
        let Ok(opened) = host_fs::open_at(&dir, bytes, 0x20000) else { return false };
        if file_index(&opened) != Some(original_id) {
            return false;
        }
        drop(opened);
        let Ok(c_name) = std::ffi::CString::new(bytes) else { return false };
        return unlink_opened_name(&dir, &c_name, original_id);
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (path, original_id);
        false
    }
}

/// Remove `name` only when it still names the file we confirmed.
#[cfg(unix)]
fn unlink_opened_name(dir: &std::fs::File, name: &std::ffi::CStr, expected: (u64, u64, u64)) -> bool {
    use std::os::fd::AsRawFd;
    let Some(found) = fstatat_identity(dir.as_raw_fd(), name) else { return false };
    if found != (expected.0, expected.1) {
        return false;
    }
    unsafe extern "C" {
        fn unlinkat(dirfd: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    unsafe { unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) == 0 }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn fstatat_identity(dirfd: i32, name: &std::ffi::CStr) -> Option<(u64, u64)> {
    let mut buf = [0u8; 256];
    unsafe extern "C" {
        fn fstatat(dirfd: i32, path: *const std::ffi::c_char, stat: *mut u8, flags: i32) -> i32;
    }
    if unsafe { fstatat(dirfd, name.as_ptr(), buf.as_mut_ptr(), 0x100) } != 0 {
        return None;
    }
    let dev = u64::from_ne_bytes(buf.get(0..8)?.try_into().ok()?);
    let ino = u64::from_ne_bytes(buf.get(8..16)?.try_into().ok()?);
    Some((dev, ino))
}

#[cfg(target_os = "macos")]
fn fstatat_identity(dirfd: i32, name: &std::ffi::CStr) -> Option<(u64, u64)> {
    let mut buf = [0u8; 256];
    unsafe extern "C" {
        fn fstatat(dirfd: i32, path: *const std::ffi::c_char, stat: *mut u8, flags: i32) -> i32;
    }
    if unsafe { fstatat(dirfd, name.as_ptr(), buf.as_mut_ptr(), 0x0020) } != 0 {
        return None;
    }
    let dev = u32::from_ne_bytes(buf.get(0..4)?.try_into().ok()?) as u64;
    let ino = u64::from_ne_bytes(buf.get(8..16)?.try_into().ok()?);
    Some((dev, ino))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android", target_os = "macos"))))]
fn fstatat_identity(_dirfd: i32, _name: &std::ffi::CStr) -> Option<(u64, u64)> {
    None
}

fn keep_failed_project_output(file: &mut std::fs::File, path: &Path, root: &str) -> Result<PathBuf> {
    let review = Path::new(root).join("review");
    match std::fs::create_dir(&review) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("review directory: {error}")),
    }
    let meta = std::fs::symlink_metadata(&review).map_err(|error| format!("review directory: {error}"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("The workspace review location must be a regular directory".into());
    }
    let name = path.file_name().ok_or("Project file name must be a single file inside the selected folder")?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
    let slot = review.join(format!("partial-{stamp}-{}", std::process::id()));
    std::fs::create_dir(&slot).map_err(|error| format!("review directory: {error}"))?;
    let destination = slot.join(name);
    file.seek(std::io::SeekFrom::Start(0)).map_err(|error| format!("Partial project output: {error}"))?;
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&destination).map_err(|error| format!("Partial project output: {error}"))?;
    let mut buf = [0u8; 65536];
    let mut copied = 0u64;
    let budget = host_byte_budget();
    loop {
        let count = file.read(&mut buf).map_err(|error| format!("Partial project output: {error}"))?;
        if count == 0 {
            break;
        }
        copied = copied.saturating_add(count as u64);
        if copied > budget {
            return Err("Partial project output exceeds the memory budget".into());
        }
        output.write_all(&buf[..count]).map_err(|error| format!("Partial project output: {error}"))?;
    }
    output.sync_all().map_err(|error| format!("Partial project output: {error}"))?;
    let _ = delete_same_file(path, file);
    Ok(destination)
}

fn write_project_archive(tree: &str, name: &str, encoded: &str, root: &str) -> Result<Vec<u8>> {
    if encoded.len() > 7 * 1024 * 1024 {
        return Err("Project file exceeds 5 MiB".into());
    }
    let bytes = network::unbase64(encoded)?;
    if bytes.len() as u64 > PROJECT_FILE_LIMIT {
        return Err("Project file exceeds 5 MiB".into());
    }
    single_project_name(name)?;
    let (base, dir) = project_directory(tree)?;
    let path = base.join(name);
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            return Err("Refusing to follow a symlink out of the selected folder".into());
        }
    }
    let mut file = match host_fs::open_at(&dir, name.as_bytes(), 1 | 0x40 | 0x80 | 0x20000) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Err("The existing file was left unchanged".into()),
        Err(error) => return Err(format!("The project file could not be created: {error}")),
    };
    let created = (|| -> Result<()> {
        if file.metadata().map_err(|error| format!("The project file could not be opened: {error}"))?.file_type().is_symlink() {
            return Err("Refusing to follow a symlink out of the selected folder".into());
        }
        confirm_inside(&dir, &file, name)?;
        file.write_all(&bytes).map_err(|error| format!("The project file could not be written: {error}"))?;
        file.flush().map_err(|error| format!("The project file could not be written: {error}"))?;
        Ok(())
    })();
    if let Err(error) = created {
        let kept = keep_failed_project_output(&mut file, &path, root);
        drop(file);
        return Err(match kept {
            Ok(kept) => format!("{error} Partial output was kept at {}", kept.display()),
            Err(keep) => format!("{error} ({keep})"),
        });
    }
    drop(file);
    match read_project_file(tree, name)? {
        Some(found) if found == bytes => Ok(bytes),
        Some(_) => Err("Project readback differs from the bytes written".into()),
        None => Err("Project file was not found after writing".into()),
    }
}

fn confirm_inside(dir: &std::fs::File, file: &std::fs::File, name: &str) -> Result<()> {
    #[cfg(windows)]
    {
        let dir_path = host_fs::handle_path(dir).map_err(|error| format!("The selected folder could not be opened: {error}"))?;
        let file_path = host_fs::handle_path(file).map_err(|error| format!("The project file could not be opened: {error}"))?;
        if !windows_child_path(&dir_path, &file_path, name) {
            return Err("Refusing to follow a symlink out of the selected folder".into());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (dir, file, name);
    }
    Ok(())
}

#[cfg(windows)]
fn windows_child_path(dir_path: &Path, file_path: &Path, name: &str) -> bool {
    if windows_path_key(file_path) == windows_path_key(&dir_path.join(name)) {
        return true;
    }
    match (file_path.parent(), file_path.file_name()) {
        (Some(parent), Some(file_name)) => windows_path_key(parent) == windows_path_key(dir_path) && file_name.eq_ignore_ascii_case(std::ffi::OsStr::new(name)),
        _ => false,
    }
}

#[cfg(windows)]
fn windows_path_key(path: &Path) -> String {
    let mut text = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        text = format!(r"\\{rest}");
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        text = rest.to_string();
    }
    text.trim_end_matches('\\').to_ascii_lowercase()
}

/// The zip selected for review, held until archive_import or archive_clear.
static PENDING_IMPORT: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

fn import_hold() -> std::sync::MutexGuard<'static, Option<Vec<u8>>> {
    PENDING_IMPORT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn keep_import(bytes: Vec<u8>) {
    *import_hold() = Some(bytes);
}

fn take_import() -> Option<Vec<u8>> {
    import_hold().take()
}

fn clear_import() {
    *import_hold() = None;
}

/// One file, at most `limit` bytes. A larger file is refused before it is kept.
fn read_limited(path: &str, limit: u64) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(path).map_err(|error| format!("The file could not be opened: {error}"))?;
    if !meta.is_file() {
        return Err("Select a file, not a folder".into());
    }
    if meta.len() > limit {
        return Err(format!("The selected file exceeds {} MiB", limit / (1024 * 1024)));
    }
    // The type above can change before this open. Open without blocking and
    // check the descriptor itself before reading.
    let file = open_regular_for_read(Path::new(path))?;
    let opened = file.metadata().map_err(|error| format!("The file could not be read: {error}"))?;
    if !opened.is_file() {
        return Err("Select a file, not a folder".into());
    }
    if opened.len() > limit {
        return Err(format!("The selected file exceeds {} MiB", limit / (1024 * 1024)));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|error| format!("The file could not be read: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("The selected file exceeds {} MiB", limit / (1024 * 1024)));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
enum PortalAnswer {
    Chosen(String),
    Cancelled,
    Absent,
}

/// The session file chooser when it answers, otherwise one typed path.
#[cfg(not(windows))]
fn choose_or_prompt(title: &str, saving: bool, suggested: &str, ask_portal: bool, scripted: Option<&str>) -> Result<Option<String>> {
    #[cfg(target_os = "linux")]
    if ask_portal {
        match portal_file(title, saving, suggested)? {
            PortalAnswer::Chosen(path) => return interpret_file_line(&path, saving),
            PortalAnswer::Cancelled => return Ok(None),
            PortalAnswer::Absent => {}
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (ask_portal, title, suggested);
    if let Some(line) = scripted {
        return interpret_file_line(line, saving);
    }
    prompt_path(title, saving, suggested)
}

#[cfg(any(not(windows), test, g1_regression))]
fn interpret_file_line(line: &str, saving: bool) -> Result<Option<String>> {
    let choice = line.trim();
    if choice.is_empty() {
        return Ok(None);
    }
    if choice.contains('\0') {
        return Err("The selected path is empty".into());
    }
    let path = Path::new(choice);
    if saving {
        // A directory is refused before anything is written. An existing FIFO,
        // socket, or other non-regular file is refused the same way: the
        // message names the path, nothing is written, and the caller continues.
        // A regular file and a path that does not exist yet are unchanged.
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.is_dir() {
                return Err("Choose a file path".into());
            }
            if !meta.is_file() {
                return Err(format!("{choice} is not a regular file"));
            }
        }
        return Ok(Some(choice.to_string()));
    }
    let meta = std::fs::metadata(path).map_err(|error| format!("The file could not be opened: {error}"))?;
    if !meta.is_file() {
        return Err("Select a file, not a folder".into());
    }
    Ok(Some(choice.to_string()))
}

/// Bytes typed at the terminal, kept up to the same 4096-byte cap as before.
/// Valid UTF-8 is decoded losslessly. A sequence that is not UTF-8 is turned
/// into text the way this platform turns path bytes into text.
#[cfg(any(not(windows), test, g1_regression))]
fn read_typed_line(source: &mut impl std::io::Read) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match source.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) if byte[0] != b'\r' => {
                if bytes.len() < 4096 {
                    bytes.push(byte[0]);
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(decode_typed_bytes(bytes))
}

#[cfg(any(not(windows), test, g1_regression))]
fn decode_typed_bytes(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let bytes = error.into_bytes();
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                std::ffi::OsStr::from_bytes(&bytes).to_string_lossy().into_owned()
            }
            #[cfg(not(unix))]
            {
                String::from_utf8_lossy(&bytes).into_owned()
            }
        }
    }
}

#[cfg(not(windows))]
fn prompt_path(title: &str, saving: bool, suggested: &str) -> Result<Option<String>> {
    let mut tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").map_err(|_| "Choosing a file needs a terminal".to_string())?;
    if saving {
        writeln!(tty, "{title}\nEnter a destination path, or nothing to cancel. Suggested name: {suggested}").map_err(|error| format!("Choosing a file: {error}"))?;
    } else {
        writeln!(tty, "{title}\nEnter a file path, or nothing to cancel.").map_err(|error| format!("Choosing a file: {error}"))?;
    }
    let line = read_typed_line(&mut tty).map_err(|error| format!("Choosing a file: {error}"))?;
    interpret_file_line(&line, saving)
}

#[cfg(target_os = "linux")]
fn portal_file(title: &str, saving: bool, suggested: &str) -> Result<PortalAnswer> {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        return Ok(PortalAnswer::Absent);
    }
    match portal_request(title, saving, suggested) {
        Ok(answer) => Ok(answer),
        Err(error) if error == "file chooser failed" => Err(error),
        Err(error) if error == "file chooser closed" => Ok(PortalAnswer::Cancelled),
        Err(_) => Ok(PortalAnswer::Absent),
    }
}

#[cfg(target_os = "linux")]
fn portal_request(title: &str, saving: bool, suggested: &str) -> Result<PortalAnswer> {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| "session bus did not answer".to_string())?;
    let mut stream = connect_session(&address)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2))).ok();
    bus_handshake(&mut stream)?;
    let hello = bus_call(&mut stream, 1, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", "", &[])?;
    let unique = bus_body_string(&hello)?;
    let token = format!("rllm{}", PORTAL_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let sender = unique.trim_start_matches(':').replace('.', "_");
    if sender.is_empty() || !sender.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("session bus did not answer".into());
    }
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let rule = format!("type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{path}'");
    let rule_body = bus_string_body(&rule);
    bus_call(&mut stream, 2, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "AddMatch", "s", &rule_body)?;
    let member = if saving { "SaveFile" } else { "OpenFile" };
    let body = chooser_body(title, saving, suggested, &token);
    let request = bus_encode_call(3, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.FileChooser", member, "ssa{sv}", &body);
    stream.write_all(&request).map_err(|_| "session bus did not answer".to_string())?;
    let mut returned = false;
    let mut signal = None;
    for _ in 0..32 {
        let incoming = bus_read_message(&mut stream, !returned)?;
        let kind = incoming.get(1).copied().unwrap_or(0);
        let reply = bus_header(&incoming, 5).ok();
        if !returned && reply.as_deref() == Some("3") {
            if kind == 3 {
                return Err("session bus did not answer".into());
            }
            if kind == 2 {
                returned = true;
                stream.set_read_timeout(Some(std::time::Duration::from_secs(120))).ok();
                if signal.is_some() {
                    break;
                }
                continue;
            }
        }
        if kind == 4 && bus_header(&incoming, 3).ok().as_deref() == Some("Response") {
            signal = Some(incoming);
            if returned {
                break;
            }
        }
    }
    let signal = signal.ok_or_else(|| "file chooser closed".to_string())?;
    let (code, uris) = portal_uris(bus_message_body(&signal)?)?;
    match code {
        0 => {
            let uri = uris.into_iter().next().ok_or_else(|| "file chooser failed".to_string())?;
            Ok(PortalAnswer::Chosen(file_uri_path(&uri)?))
        }
        1 => Ok(PortalAnswer::Cancelled),
        _ => Err("file chooser failed".into()),
    }
}

#[cfg(target_os = "linux")]
static PORTAL_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[cfg(target_os = "linux")]
fn linux_open_address(url: &str) -> Result<()> {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| "Authorization could not be opened".to_string())?;
    let mut stream = connect_session(&address)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2))).ok();
    bus_handshake(&mut stream)?;
    let hello = bus_call(&mut stream, 1, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", "", &[])?;
    let unique = bus_body_string(&hello)?;
    let token = format!("rllm{}", PORTAL_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let sender = unique.trim_start_matches(':').replace('.', "_");
    if sender.is_empty() || !sender.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("Authorization could not be opened".into());
    }
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let rule = format!("type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{path}'");
    bus_call(&mut stream, 2, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "AddMatch", "s", &bus_string_body(&rule))?;
    let mut body = Vec::new();
    bus_put_string(&mut body, "");
    bus_put_string(&mut body, url);
    bus_put_array(&mut body, 8, |buf| {
        bus_dict_string(buf, "handle_token", &token);
    });
    let request = bus_encode_call(3, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.OpenURI", "OpenURI", "ssa{sv}", &body);
    stream.write_all(&request).map_err(|_| "Authorization could not be opened".to_string())?;
    let mut returned = false;
    let mut signal = None;
    for _ in 0..32 {
        let incoming = bus_read_message(&mut stream, !returned)?;
        let kind = incoming.get(1).copied().unwrap_or(0);
        let reply = bus_header(&incoming, 5).ok();
        if !returned && reply.as_deref() == Some("3") {
            if kind == 3 {
                return Err("Authorization could not be opened".into());
            }
            if kind == 2 {
                returned = true;
                stream.set_read_timeout(Some(std::time::Duration::from_secs(60))).ok();
                if signal.is_some() {
                    break;
                }
                continue;
            }
        }
        if kind == 4 && bus_header(&incoming, 3).ok().as_deref() == Some("Response") {
            signal = Some(incoming);
            if returned {
                break;
            }
        }
    }
    let signal = signal.ok_or_else(|| "Authorization could not be opened".to_string())?;
    let body = bus_message_body(&signal)?;
    let mut bus = Bus { bytes: body, index: 0 };
    match bus.u32()? {
        0 => Ok(()),
        _ => Err("Authorization could not be opened".into()),
    }
}

#[cfg(target_os = "linux")]
fn connect_session(address: &str) -> Result<std::os::unix::net::UnixStream> {
    for part in address.split(';') {
        if let Some(stream) = connect_bus(part.trim()) {
            return Ok(stream);
        }
    }
    Err("session bus did not answer".into())
}

#[cfg(target_os = "linux")]
fn connect_bus(part: &str) -> Option<std::os::unix::net::UnixStream> {
    let spec = part.split(',').next().unwrap_or(part);
    let (abstract_name, raw) = if let Some(path) = spec.strip_prefix("unix:path=") {
        (false, path)
    } else if let Some(name) = spec.strip_prefix("unix:abstract=") {
        (true, name)
    } else {
        return None;
    };
    let name = bus_percent_decode(raw)?;
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() >= 107 {
        return None;
    }
    #[repr(C)]
    struct UnixAddr {
        family: u16,
        path: [u8; 108],
    }
    let mut addr = UnixAddr { family: 1, path: [0; 108] };
    let len = if abstract_name {
        addr.path[1..1 + bytes.len()].copy_from_slice(bytes);
        (2 + 1 + bytes.len()) as u32
    } else {
        addr.path[..bytes.len()].copy_from_slice(bytes);
        (2 + bytes.len() + 1) as u32
    };
    unsafe extern "C" {
        fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        fn connect(fd: i32, addr: *const UnixAddr, len: u32) -> i32;
        fn close(fd: i32) -> i32;
    }
    let fd = unsafe { socket(1, 1, 0) };
    if fd < 0 {
        return None;
    }
    if unsafe { connect(fd, &addr, len) } != 0 {
        unsafe { close(fd) };
        return None;
    }
    use std::os::fd::FromRawFd;
    Some(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
fn bus_percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(target_os = "linux")]
fn bus_handshake(stream: &mut std::os::unix::net::UnixStream) -> Result<()> {
    let uid = unsafe {
        extern "C" {
            fn getuid() -> u32;
        }
        getuid()
    };
    let hex: String = format!("{uid}").bytes().map(|byte| format!("{byte:02x}")).collect();
    let hello = format!("\0AUTH EXTERNAL {hex}\r\n");
    stream.write_all(hello.as_bytes()).map_err(|_| "session bus did not answer".to_string())?;
    let line = bus_sasl_line(stream)?;
    if !line.starts_with("OK ") {
        return Err("session bus did not answer".into());
    }
    stream.write_all(b"BEGIN\r\n").map_err(|_| "session bus did not answer".to_string())?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn bus_sasl_line(stream: &mut impl Read) -> Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if line.len() > 512 {
            return Err("session bus did not answer".into());
        }
        match stream.read(&mut byte) {
            Ok(0) => return Err("session bus did not answer".into()),
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) if byte[0] != b'\r' => line.push(byte[0]),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err("session bus did not answer".into()),
        }
    }
    String::from_utf8(line).map_err(|_| "session bus did not answer".to_string())
}

#[cfg(target_os = "linux")]
fn bus_call(stream: &mut std::os::unix::net::UnixStream, serial: u32, dest: &str, path: &str, iface: &str, member: &str, signature: &str, body: &[u8]) -> Result<Vec<u8>> {
    let msg = bus_encode_call(serial, dest, path, iface, member, signature, body);
    stream.write_all(&msg).map_err(|_| "session bus did not answer".to_string())?;
    for _ in 0..8 {
        let incoming = bus_read_message(stream, true)?;
        let kind = incoming.first().map(|_| incoming[1]).unwrap_or(0);
        let reply = bus_header(&incoming, 5).ok();
        if reply.as_deref() == Some(&serial.to_string()) && (kind == 2 || kind == 3) {
            if kind == 3 {
                return Err("session bus did not answer".into());
            }
            return Ok(incoming);
        }
    }
    Err("session bus did not answer".into())
}

#[cfg(target_os = "linux")]
fn bus_read_message(stream: &mut impl Read, setup: bool) -> Result<Vec<u8>> {
    let fail = if setup { "session bus did not answer" } else { "file chooser closed" };
    let mut fixed = [0u8; 12];
    bus_read_exact(stream, &mut fixed, fail)?;
    if fixed[0] != b'l' || fixed[3] != 1 {
        return Err(fail.into());
    }
    let body_len = u32::from_le_bytes(fixed[4..8].try_into().unwrap()) as usize;
    let mut len_buf = [0u8; 4];
    bus_read_exact(stream, &mut len_buf, fail)?;
    let fields_len = u32::from_le_bytes(len_buf) as usize;
    if fields_len > 65536 || body_len > 1024 * 1024 {
        return Err(fail.into());
    }
    let mut msg = Vec::with_capacity(16 + fields_len + body_len + 8);
    msg.extend_from_slice(&fixed);
    msg.extend_from_slice(&len_buf);
    let mut fields = vec![0u8; fields_len];
    bus_read_exact(stream, &mut fields, fail)?;
    msg.extend_from_slice(&fields);
    while msg.len() % 8 != 0 {
        let mut pad = [0u8; 1];
        bus_read_exact(stream, &mut pad, fail)?;
        msg.push(pad[0]);
    }
    let mut body = vec![0u8; body_len];
    if body_len > 0 {
        bus_read_exact(stream, &mut body, fail)?;
    }
    msg.extend_from_slice(&body);
    Ok(msg)
}

#[cfg(target_os = "linux")]
fn bus_read_exact(stream: &mut impl Read, buf: &mut [u8], fail: &str) -> Result<()> {
    let mut at = 0;
    while at < buf.len() {
        match stream.read(&mut buf[at..]) {
            Ok(0) => return Err(fail.into()),
            Ok(n) => at += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(fail.into()),
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn bus_encode_call(serial: u32, dest: &str, path: &str, iface: &str, member: &str, signature: &str, body: &[u8]) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(&[b'l', 1, 0, 1]);
    msg.extend_from_slice(&(body.len() as u32).to_le_bytes());
    msg.extend_from_slice(&serial.to_le_bytes());
    bus_put_array(&mut msg, 8, |buf| {
        bus_header_field(buf, 1, "o", |out| bus_put_string(out, path));
        bus_header_field(buf, 6, "s", |out| bus_put_string(out, dest));
        bus_header_field(buf, 2, "s", |out| bus_put_string(out, iface));
        bus_header_field(buf, 3, "s", |out| bus_put_string(out, member));
        if !signature.is_empty() {
            bus_header_field(buf, 8, "g", |out| bus_put_signature(out, signature));
        }
    });
    bus_pad(&mut msg, 8);
    msg.extend_from_slice(body);
    msg
}

#[cfg(target_os = "linux")]
fn chooser_body(title: &str, saving: bool, suggested: &str, token: &str) -> Vec<u8> {
    let mut body = Vec::new();
    bus_put_string(&mut body, "");
    bus_put_string(&mut body, title);
    bus_put_array(&mut body, 8, |buf| {
        bus_dict_string(buf, "handle_token", token);
        bus_dict_bool(buf, "multiple", false);
        if saving && !suggested.is_empty() {
            bus_dict_string(buf, "current_name", suggested);
        }
    });
    body
}

#[cfg(target_os = "linux")]
fn bus_string_body(value: &str) -> Vec<u8> {
    let mut body = Vec::new();
    bus_put_string(&mut body, value);
    body
}

#[cfg(target_os = "linux")]
fn bus_pad(buf: &mut Vec<u8>, align: usize) {
    let extra = (align - (buf.len() % align)) % align;
    buf.extend(std::iter::repeat(0).take(extra));
}

#[cfg(target_os = "linux")]
fn bus_put_u32(buf: &mut Vec<u8>, value: u32) {
    bus_pad(buf, 4);
    buf.extend_from_slice(&value.to_le_bytes());
}

#[cfg(target_os = "linux")]
fn bus_put_string(buf: &mut Vec<u8>, value: &str) {
    bus_put_u32(buf, value.len() as u32);
    buf.extend_from_slice(value.as_bytes());
    buf.push(0);
}

#[cfg(target_os = "linux")]
fn bus_put_signature(buf: &mut Vec<u8>, value: &str) {
    buf.push(value.len() as u8);
    buf.extend_from_slice(value.as_bytes());
    buf.push(0);
}

#[cfg(target_os = "linux")]
fn bus_put_array(buf: &mut Vec<u8>, element_align: usize, fill: impl FnOnce(&mut Vec<u8>)) {
    bus_pad(buf, 4);
    let len_at = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());
    bus_pad(buf, element_align);
    let start = buf.len();
    fill(buf);
    let len = (buf.len() - start) as u32;
    buf[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
}

#[cfg(target_os = "linux")]
fn bus_header_field(buf: &mut Vec<u8>, code: u8, signature: &str, write_value: impl FnOnce(&mut Vec<u8>)) {
    bus_pad(buf, 8);
    buf.push(code);
    bus_put_signature(buf, signature);
    write_value(buf);
}

#[cfg(target_os = "linux")]
fn bus_dict_string(buf: &mut Vec<u8>, key: &str, value: &str) {
    bus_pad(buf, 8);
    bus_put_string(buf, key);
    bus_put_signature(buf, "s");
    bus_put_string(buf, value);
}

#[cfg(target_os = "linux")]
fn bus_dict_bool(buf: &mut Vec<u8>, key: &str, value: bool) {
    bus_pad(buf, 8);
    bus_put_string(buf, key);
    bus_put_signature(buf, "b");
    bus_put_u32(buf, u32::from(value));
}

#[cfg(target_os = "linux")]
struct Bus<'a> {
    bytes: &'a [u8],
    index: usize,
}

#[cfg(target_os = "linux")]
impl<'a> Bus<'a> {
    fn align(&mut self, n: usize) -> Result<()> {
        let pad = (n - (self.index % n)) % n;
        self.index = self.index.checked_add(pad).ok_or("session bus did not answer")?;
        if self.index > self.bytes.len() {
            return Err("session bus did not answer".into());
        }
        Ok(())
    }
    fn byte(&mut self) -> Result<u8> {
        let value = *self.bytes.get(self.index).ok_or("session bus did not answer")?;
        self.index += 1;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32> {
        self.align(4)?;
        let raw = self.bytes.get(self.index..self.index + 4).ok_or("session bus did not answer")?;
        self.index += 4;
        Ok(u32::from_le_bytes(raw.try_into().unwrap()))
    }
    fn text(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        if n > 1024 * 1024 {
            return Err("session bus did not answer".into());
        }
        let raw = self.bytes.get(self.index..self.index + n).ok_or("session bus did not answer")?;
        self.index += n;
        if self.byte()? != 0 {
            return Err("session bus did not answer".into());
        }
        String::from_utf8(raw.to_vec()).map_err(|_| "session bus did not answer".into())
    }
    fn signature(&mut self) -> Result<String> {
        let n = self.byte()? as usize;
        if n > 255 {
            return Err("session bus did not answer".into());
        }
        let raw = self.bytes.get(self.index..self.index + n).ok_or("session bus did not answer")?;
        self.index += n;
        if self.byte()? != 0 {
            return Err("session bus did not answer".into());
        }
        String::from_utf8(raw.to_vec()).map_err(|_| "session bus did not answer".into())
    }
}

#[cfg(target_os = "linux")]
fn bus_header(msg: &[u8], code: u8) -> Result<String> {
    if msg.len() < 16 {
        return Err("session bus did not answer".into());
    }
    let fields_len = u32::from_le_bytes(msg[12..16].try_into().unwrap()) as usize;
    let mut bus = Bus { bytes: msg, index: 16 };
    let end = 16 + fields_len;
    let mut found = None;
    while bus.index < end {
        bus.align(8)?;
        if bus.index >= end {
            break;
        }
        let field = bus.byte()?;
        let sig = bus.signature()?;
        let value = match sig.as_str() {
            "s" | "o" => bus.text()?,
            "g" => bus.signature()?,
            "u" => bus.u32()?.to_string(),
            _ => return Err("session bus did not answer".into()),
        };
        if field == code {
            found = Some(value);
        }
    }
    found.ok_or_else(|| "session bus did not answer".into())
}

#[cfg(target_os = "linux")]
fn bus_message_body(msg: &[u8]) -> Result<&[u8]> {
    if msg.len() < 16 {
        return Err("session bus did not answer".into());
    }
    let fields_len = u32::from_le_bytes(msg[12..16].try_into().unwrap()) as usize;
    let body_len = u32::from_le_bytes(msg[4..8].try_into().unwrap()) as usize;
    let header = (16 + fields_len + 7) & !7;
    msg.get(header..header + body_len).ok_or_else(|| "session bus did not answer".into())
}

#[cfg(target_os = "linux")]
fn bus_body_string(msg: &[u8]) -> Result<String> {
    let mut bus = Bus { bytes: bus_message_body(msg)?, index: 0 };
    bus.text()
}

#[cfg(target_os = "linux")]
fn portal_uris(body: &[u8]) -> Result<(u32, Vec<String>)> {
    let mut bus = Bus { bytes: body, index: 0 };
    let code = bus.u32()?;
    let count = bus.u32()? as usize;
    bus.align(8)?;
    let end = bus.index.checked_add(count).ok_or("session bus did not answer")?;
    if end > body.len() {
        return Err("session bus did not answer".into());
    }
    let mut uris = Vec::new();
    while bus.index < end {
        bus.align(8)?;
        if bus.index >= end {
            break;
        }
        let key = bus.text()?;
        let sig = bus.signature()?;
        if key == "uris" && sig == "as" {
            uris = bus_string_array(&mut bus)?;
        } else {
            bus_skip(&mut bus, &sig)?;
        }
    }
    Ok((code, uris))
}

#[cfg(target_os = "linux")]
fn bus_string_array(bus: &mut Bus<'_>) -> Result<Vec<String>> {
    let count = bus.u32()? as usize;
    bus.align(4)?;
    let end = bus.index.checked_add(count).ok_or("session bus did not answer")?;
    if end > bus.bytes.len() {
        return Err("session bus did not answer".into());
    }
    let mut out = Vec::new();
    while bus.index < end {
        out.push(bus.text()?);
    }
    if bus.index != end {
        return Err("session bus did not answer".into());
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
fn bus_skip(bus: &mut Bus<'_>, sig: &str) -> Result<()> {
    match sig {
        "y" => { bus.byte()?; }
        "b" | "u" | "i" => { bus.u32()?; }
        "s" | "o" => { bus.text()?; }
        "g" => { bus.signature()?; }
        "as" => { bus_string_array(bus)?; }
        _ => return Err("session bus did not answer".into()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn file_uri_path(uri: &str) -> Result<String> {
    let rest = uri.strip_prefix("file://").ok_or("The selected file is not a local path")?;
    let path = if let Some(path) = rest.strip_prefix("localhost") {
        path
    } else if rest.starts_with('/') {
        rest
    } else {
        return Err("The selected file is not a local path".into());
    };
    let bytes = path.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| "The selected file path is not UTF-8")?;
            let value = u8::from_str_radix(hex, 16).map_err(|_| "The selected file path is not UTF-8")?;
            out.push(value);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    let text = String::from_utf8(out).map_err(|_| "The selected file path is not UTF-8".to_string())?;
    if text.is_empty() {
        return Err("The selected file path is empty".into());
    }
    Ok(text)
}

/// Asks for one file. `Ok(None)` means the person closed the dialog.
fn choose_file(title: &str, files: Files) -> Result<Option<String>> {
    #[cfg(windows)]
    return window::choose(title, files);
    #[cfg(not(windows))]
    {
        let _ = files;
        choose_or_prompt(title, false, "", true, None)
    }
}

/// One project ZIP through the existing open dialog. `Ok(None)` means it was closed.
fn choose_zip() -> Result<Option<String>> {
    choose_file("Select a project ZIP", Files::Attachment)
}

/// Asks where to save a file. `Ok(None)` means the person closed the dialog.
fn save_file(title: &str, name: &str) -> Result<Option<String>> {
    #[cfg(windows)]
    return window::save(title, name);
    #[cfg(not(windows))]
    choose_or_prompt(title, true, name, true, None)
}

/// A chosen text file, refused when it is too large or not UTF-8 text.
fn read_text(path: &str, limit: u64) -> Result<String> {
    let bytes = read_path_limited(Path::new(path), limit)?;
    match String::from_utf8(bytes) {
        Ok(content) if !content.contains('\0') => Ok(content),
        _ => Err("The selected file is not UTF-8 text".into()),
    }
}

/// Where a prepared export waits, sealed with the state key, until the person
/// chooses its destination: the file and the key that seals it.
static EXPORT_STORE: std::sync::OnceLock<(PathBuf, [u8; 32])> = std::sync::OnceLock::new();

fn export_store() -> Result<&'static (PathBuf, [u8; 32])> {
    EXPORT_STORE.get().ok_or_else(|| "The export store is not open".to_string())
}

fn export_stage(effect: &Json, bytes: &[u8]) -> Result<()> {
    let (path, key) = export_store()?;
    let record = obj(vec![
        ("ticket", get(effect, "ticket").clone()),
        ("name", get(effect, "name").clone()),
        ("mime", get(effect, "mime").clone()),
        ("data", s(&network::b64(bytes))),
    ]);
    let sealed = state_crypto::seal(record.string().as_bytes(), key)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("Export snapshot: {error}"))?;
    }
    std::fs::write(path, sealed).map_err(|error| format!("Export snapshot: {error}"))
}

/// The staged export for a ticket: its suggested file name and its bytes.
fn export_load(ticket: &str) -> Result<(String, Vec<u8>)> {
    let (path, key) = export_store()?;
    let file = std::fs::File::open(path).map_err(|_| "Saved snapshot is unavailable or changed".to_string())?;
    let sealed = read_file_limited(&file, host_byte_budget()).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidData {
            "Saved snapshot exceeds the memory budget".to_string()
        } else {
            "Saved snapshot is unavailable or changed".to_string()
        }
    })?;
    let plain = String::from_utf8(state_crypto::open(&sealed, key)?).map_err(|_| "Saved snapshot is not UTF-8".to_string())?;
    let record = parse(&plain)?;
    if ticket.is_empty() || text(&record, "ticket") != ticket {
        return Err("Saved snapshot is unavailable or changed".into());
    }
    Ok((text(&record, "name"), network::unbase64(&text(&record, "data"))?))
}

/// Removes the staged export. Another ticket's snapshot is left alone.
fn export_clear(ticket: &str) -> Result<()> {
    let (path, _) = export_store()?;
    match export_load(ticket) {
        Ok(_) => std::fs::remove_file(path).map_err(|error| format!("Export snapshot: {error}")),
        Err(_) if !path.exists() => Ok(()),
        Err(error) => Err(error),
    }
}
/// The file the person attaches, as the action and inputs the core expects:
/// a PNG or JPEG becomes an image attachment, anything else must be UTF-8 text
/// and is added to the draft. Limits match the phone: 2 MiB read, 1 MiB text.
fn attachment(ticket: &Json) -> (&'static str, Json) {
    let failed = |error: String| ("document_failure", obj(vec![("document_ticket", ticket.clone()), ("error", s(&error))]));
    let path = match choose_file("Attach image or file", Files::Attachment) {
        Ok(Some(path)) => path,
        Ok(None) => return ("document_cancel", obj(vec![("document_ticket", ticket.clone())])),
        Err(error) => return failed(error),
    };
    let size = match std::fs::metadata(&path) {
        Ok(found) if found.is_file() => found.len(),
        Ok(_) => return failed("Select a file, not a folder".into()),
        Err(error) => return failed(format!("The file could not be opened: {error}")),
    };
    if size > 2 * 1024 * 1024 {
        return failed("The selected file exceeds 2 MiB".into());
    }
    let bytes = match read_path_limited(Path::new(&path), 2 * 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(error) => return failed(error),
    };
    let mut name: String = Path::new(&path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default().chars().filter(|letter| !letter.is_control()).take(128).collect();
    if name.is_empty() {
        name = "Selected file".into();
    }
    let png = bytes.starts_with(&[137, 80, 78, 71]);
    let jpeg = bytes.starts_with(&[255, 216]);
    if png || jpeg {
        return (
            "image_result",
            obj(vec![
                ("document_ticket", ticket.clone()),
                ("name", s(&name)),
                ("mime", s(if png { "image/png" } else { "image/jpeg" })),
                ("bytes", Num(bytes.len().to_string())),
                ("data", s(&network::b64(&bytes))),
            ]),
        );
    }
    let lower = name.to_lowercase();
    if [".gif", ".webp", ".bmp", ".tif", ".tiff", ".heic", ".avif", ".ico"].iter().any(|ending| lower.ends_with(ending)) {
        return failed("Select a PNG or JPEG image".into());
    }
    if bytes.len() > 1024 * 1024 {
        return failed("Text document exceeds 1 MiB".into());
    }
    match String::from_utf8(bytes) {
        Ok(content) if !content.contains('\0') => ("document_result", obj(vec![("document_ticket", ticket.clone()), ("content", s(&content))])),
        _ => failed("Select a PNG/JPEG image or UTF-8 text document".into()),
    }
}

fn prove(root: &str, tool: &str, args: Json) -> Result<String> {
    device_folder_tool(&obj(vec![("tool", s(tool)), ("args", args)]), root).map_err(|error| format!("{tool}: {error}"))
}

// pins the storage directory with the ordinary file lock, which works on the
// other hosts. A directory lock is held on a named semaphore instead. A lock
// on a regular file stays on the system call.
#[cfg(windows)]
static ORIGINAL_LOCK_FILE: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static ORIGINAL_UNLOCK_FILE: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static ORIGINAL_CLOSE_HANDLE: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static DIRECTORY_LOCKS: std::sync::Mutex<std::collections::BTreeMap<isize, isize>> = std::sync::Mutex::new(std::collections::BTreeMap::new());
#[cfg(windows)]
static DIRECTORY_LOCK_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(windows)]


fn directory_lock_name(handle: *mut std::ffi::c_void) -> Option<String> {
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(handle: *mut std::ffi::c_void, class: i32, info: *mut std::ffi::c_void, size: u32) -> i32;
    }
    #[repr(C)]
    struct Tag { attributes: u32, reparse: u32 }
    let mut tag = Tag { attributes: 0, reparse: 0 };
    if unsafe { GetFileInformationByHandleEx(handle, 9, (&mut tag as *mut Tag).cast(), 8) } == 0 || tag.attributes & 0x10 == 0 {
        return None;
    }
    #[repr(C)]
    struct FileId { volume: u64, file: [u8; 16] }
    let mut id = FileId { volume: 0, file: [0; 16] };
    if unsafe { GetFileInformationByHandleEx(handle, 18, (&mut id as *mut FileId).cast(), 24) } == 0 {
        return None;
    }
    let hex: String = id.file.iter().map(|byte| format!("{byte:02x}")).collect();
    Some(format!("Local\\rllm-{:016x}-{hex}", id.volume))
}

#[cfg(windows)]
fn release_directory_lock(file: *mut std::ffi::c_void) -> bool {
    use std::sync::atomic::Ordering;
    let semaphore = DIRECTORY_LOCKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&(file as isize));
    let Some(semaphore) = semaphore else { return false; };
    DIRECTORY_LOCK_COUNT.fetch_sub(1, Ordering::Release);
    unsafe extern "system" {
        fn ReleaseSemaphore(handle: isize, release: i32, previous: *mut i32) -> i32;
    }
    unsafe { ReleaseSemaphore(semaphore, 1, std::ptr::null_mut()); }
    let original = ORIGINAL_CLOSE_HANDLE.load(Ordering::Relaxed);
    if !original.is_null() {
        let original: unsafe extern "system" fn(isize) -> i32 = unsafe { std::mem::transmute(original) };
        unsafe { original(semaphore); }
    }
    true
}

#[cfg(windows)]
unsafe extern "system" fn hooked_lock_file(file: *mut std::ffi::c_void, flags: u32, reserved: u32, low: u32, high: u32, overlapped: *mut u8) -> i32 {
    use std::sync::atomic::Ordering;
    type LockFn = unsafe extern "system" fn(*mut std::ffi::c_void, u32, u32, u32, u32, *mut u8) -> i32;
    unsafe extern "system" {
        fn CreateSemaphoreW(security: *mut std::ffi::c_void, initial: i32, maximum: i32, name: *const u16) -> isize;
        fn WaitForSingleObject(handle: isize, millis: u32) -> u32;
        fn ReleaseSemaphore(handle: isize, release: i32, previous: *mut i32) -> i32;
        fn SetLastError(code: u32);
    }
    let original = ORIGINAL_LOCK_FILE.load(Ordering::Relaxed);
    let call_original = | | -> i32 {
        if original.is_null() {
            unsafe { SetLastError(1); }
            return 0;
        }
        let original: LockFn = unsafe { std::mem::transmute(original) };
        unsafe { original(file, flags, reserved, low, high, overlapped) }
    };
    if low != u32::MAX || high != u32::MAX {
        return call_original();
    }
    let Some(name) = directory_lock_name(file) else { return call_original(); };
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let semaphore = unsafe { CreateSemaphoreW(std::ptr::null_mut(), 1, 1, wide.as_ptr()) };
    if semaphore == 0 {
        return call_original();
    }
    let wait = if flags & 1 == 0 { u32::MAX } else { 0 };
    let status = unsafe { WaitForSingleObject(semaphore, wait) };
    if status != 0 {
        let original = ORIGINAL_CLOSE_HANDLE.load(Ordering::Relaxed);
        if !original.is_null() {
            let original: unsafe extern "system" fn(isize) -> i32 = unsafe { std::mem::transmute(original) };
            unsafe { original(semaphore); }
        }
        unsafe { SetLastError(33); }
        return 0;
    }
    let mut held = DIRECTORY_LOCKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if held.contains_key(&(file as isize)) {
        drop(held);
        unsafe { ReleaseSemaphore(semaphore, 1, std::ptr::null_mut()); }
        let original = ORIGINAL_CLOSE_HANDLE.load(Ordering::Relaxed);
        if !original.is_null() {
            let original: unsafe extern "system" fn(isize) -> i32 = unsafe { std::mem::transmute(original) };
            unsafe { original(semaphore); }
        }
        unsafe { SetLastError(33); }
        return 0;
    }
    held.insert(file as isize, semaphore);
    DIRECTORY_LOCK_COUNT.fetch_add(1, Ordering::Release);
    1
}

#[cfg(windows)]
unsafe extern "system" fn hooked_unlock_file(file: *mut std::ffi::c_void, reserved: u32, low: u32, high: u32, overlapped: *mut u8) -> i32 {
    use std::sync::atomic::Ordering;
    type UnlockFn = unsafe extern "system" fn(*mut std::ffi::c_void, u32, u32, u32, *mut u8) -> i32;
    if release_directory_lock(file) {
        return 1;
    }
    let original = ORIGINAL_UNLOCK_FILE.load(Ordering::Relaxed);
    if original.is_null() {
        return 0;
    }
    let original: UnlockFn = unsafe { std::mem::transmute(original) };
    unsafe { original(file, reserved, low, high, overlapped) }
}

#[cfg(windows)]
unsafe extern "system" fn hooked_close_handle(handle: isize) -> i32 {
    use std::sync::atomic::Ordering;
    type CloseFn = unsafe extern "system" fn(isize) -> i32;
    if DIRECTORY_LOCK_COUNT.load(Ordering::Acquire) != 0 {
        release_directory_lock(handle as *mut std::ffi::c_void);
    }
    let original = ORIGINAL_CLOSE_HANDLE.load(Ordering::Relaxed);
    if original.is_null() {
        return 0;
    }
    let original: CloseFn = unsafe { std::mem::transmute(original) };
    unsafe { original(handle) }
}

#[cfg(windows)]
fn import_name(base: *mut u8, image: usize, entry: u64) -> Option<Vec<u8>> {
    if entry & (1 << 63) != 0 {
        return None;
    }
    let mut cursor = (entry as u32).checked_add(2)? as usize;
    if cursor >= image {
        return None;
    }
    let mut bytes = Vec::new();
    while cursor < image && bytes.len() < 80 {
        let unit = unsafe { *base.add(cursor) };
        if unit == 0 {
            break;
        }
        bytes.push(unit);
        cursor += 1;
    }
    Some(bytes)
}

#[cfg(windows)]
unsafe extern "C" fn install_directory_lock() {
    use std::sync::atomic::Ordering;
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
        fn VirtualProtect(address: *mut std::ffi::c_void, size: usize, new_protect: u32, old: *mut u32) -> i32;
    }
    unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *mut u8;
        if base.is_null() {
            return;
        }
        let lfanew = std::ptr::read_unaligned(base.add(0x3c).cast::<i32>());
        if lfanew <= 0 || lfanew > 1_048_576 {
            return;
        }
        let nt = base.add(lfanew as usize);
        if std::ptr::read_unaligned(nt.add(24).cast::<u16>()) != 0x20b {
            return;
        }
        let image = std::ptr::read_unaligned(nt.add(80).cast::<u32>()) as usize;
        if image < 256 || image > 64 * 1024 * 1024 {
            return;
        }
        let import_rva = std::ptr::read_unaligned(nt.add(24 + 120).cast::<u32>()) as usize;
        if import_rva == 0 || import_rva >= image {
            return;
        }
        let mut desc_at = import_rva;
        for _ in 0..256 {
            if desc_at.saturating_add(20) > image {
                break;
            }
            let desc = base.add(desc_at);
            let original = std::ptr::read_unaligned(desc.cast::<u32>());
            let name_rva = std::ptr::read_unaligned(desc.add(12).cast::<u32>());
            let thunk_rva = std::ptr::read_unaligned(desc.add(16).cast::<u32>());
            if original == 0 && name_rva == 0 && thunk_rva == 0 {
                break;
            }
            let mut name_at = if original == 0 { thunk_rva as usize } else { original as usize };
            let mut slot_at = thunk_rva as usize;
            for _ in 0..4096 {
                if name_at == 0 || slot_at == 0 || name_at.saturating_add(8) > image || slot_at.saturating_add(8) > image {
                    break;
                }
                let entry = std::ptr::read_unaligned(base.add(name_at).cast::<u64>());
                if entry == 0 {
                    break;
                }
                if let Some(bytes) = import_name(base, image, entry) {
                    let chosen = if bytes == b"LockFileEx" {
                        Some((&ORIGINAL_LOCK_FILE, hooked_lock_file as *const () as usize))
                    } else if bytes == b"UnlockFileEx" {
                        Some((&ORIGINAL_UNLOCK_FILE, hooked_unlock_file as *const () as usize))
                    } else if bytes == b"CloseHandle" {
                        Some((&ORIGINAL_CLOSE_HANDLE, hooked_close_handle as *const () as usize))
                    } else {
                        None
                    };
                    if let Some((store, replacement)) = chosen {
                        let slot = base.add(slot_at).cast::<usize>();
                        let current = *slot;
                        if current != 0 && current != replacement {
                            if store.load(Ordering::Relaxed).is_null() {
                                store.store(current as *mut std::ffi::c_void, Ordering::Relaxed);
                            }
                            let mut old = 0u32;
                            if VirtualProtect(slot.cast(), std::mem::size_of::<usize>(), 0x04, &mut old) != 0 {
                                *slot = replacement;
                                VirtualProtect(slot.cast(), std::mem::size_of::<usize>(), old, &mut old);
                            }
                        }
                    }
                }
                name_at += 8;
                slot_at += 8;
            }
            let Some(next) = desc_at.checked_add(20) else { break; };
            if next >= image {
                break;
            }
            desc_at = next;
        }
    }
}

#[cfg(windows)]
#[used]
#[link_section = ".CRT$XCU"]
static INSTALL_DIRECTORY_LOCK: unsafe extern "C" fn() = install_directory_lock;

fn main() {
    #[cfg(g1_regression)]
    {
        if std::env::var_os("RLLM_FIX18").is_some() {
            desktop_regression::fix18();
        } else {
            desktop_regression::run();
        }
        return;
    }
    let args = arguments();
    if has_flag(&args, "--tls-self-check") {
        match tls_client::Client::primitive_self_check() {
            Ok(()) => {
                println!("tls primitives ok");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = run(&args) {
        #[cfg(windows)]
        report(&error);
        #[cfg(not(windows))]
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn report(error: &str) {
    use std::os::windows::ffi::OsStrExt;
    let directory = data_directory();
    let _ = std::fs::create_dir_all(&directory);
    let _ = std::fs::write(directory.join("startup-error.txt"), error);
    let text: Vec<u16> = std::ffi::OsStr::new(error).encode_wide().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = std::ffi::OsStr::new("RLLM").encode_wide().chain(std::iter::once(0)).collect();
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: isize, text: *const u16, title: *const u16, kind: u32) -> i32;
    }
    unsafe { MessageBoxW(0, text.as_ptr(), title.as_ptr(), 0x10); }
}

fn next_line_bounded(reader: &mut impl std::io::BufRead, limit: usize) -> Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|error| format!("Input: {error}"))?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        if let Some(pos) = available.iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(pos) > limit {
                return Err("JSON exceeds 8 MiB".into());
            }
            line.extend_from_slice(&available[..pos]);
            reader.consume(pos + 1);
            break;
        }
        if line.len().saturating_add(available.len()) > limit {
            return Err("JSON exceeds 8 MiB".into());
        }
        let count = available.len();
        line.extend_from_slice(available);
        reader.consume(count);
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    if line.len() > limit {
        return Err("JSON exceeds 8 MiB".into());
    }
    String::from_utf8(line).map(Some).map_err(|_| "Input is not UTF-8".to_string())
}

fn run(args: &[std::ffi::OsString]) -> Result<()> {
    #[cfg(windows)]
    if let Some(path) = option_path(args, "--write-icon")? {
        return std::fs::write(&path, window::icon_file()?).map_err(|error| format!("{}: {error}", path.display()));
    }
    let root = match option_path(args, "--root")? {
        Some(path) => path_text(&path),
        None => std::env::current_dir().map(|value| path_text(&value)).unwrap_or_else(|_| ".".into()),
    };
    let directory = data_directory();
    let vault = Vault {
        path: option_path(args, "--state")?.unwrap_or_else(|| directory.join("state.bin")),
        key: read_key(&option_path(args, "--key")?.unwrap_or_else(|| directory.join("key")))?,
    };
    let _ = EXPORT_STORE.set((vault.path.with_file_name("export.bin"), vault.key));
    if has_flag(args, "--check") {
        // The self-test writes a vault of its own. It never opens the person's
        // saved state, so checking the program cannot replace their history.
        let scratch = std::env::temp_dir().join(format!("rllm-check-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).map_err(|error| format!("Self-test folder: {error}"))?;
        let trial = Vault { path: scratch.join("state.bin"), key: vault.key };
        let outcome = self_test(&trial, &root);
        let _ = std::fs::remove_dir_all(&scratch);
        return outcome;
    }
    let (mut state, ui) = startup(&vault, &root)?;
    if has_flag(args, "--mcp") {
        return harness_mcp::serve(&mut state, &vault, &root);
    }
    #[cfg(windows)]
    if !has_flag(args, "--stdio") {
        hide_console();
        return window::open(state, ui, vault, root);
    }
    println!("{}", ui.string());
    // The text surface drives the runtime one action at a time; the sealed
    // vault is rewritten after every settled action so a crash cannot lose a turn.
    // The line is capped before parse so an oversized line cannot allocate past the limit.
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    loop {
        let line = match next_line_bounded(&mut reader, 8 * 1024 * 1024) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => return Err(error),
        };
        if line.trim().is_empty() {
            continue;
        }
        let outcome = (|| -> Result<Json> {
            let request = parse(&line)?;
            let action = field(&request, "action")?.to_string();
            if action == "native_file" {
                return device_folder_tool(get(&request, "inputs"), &root).map(|value| s(&value));
            }
            let ui = advance(&mut state, &root, &action, get(&request, "inputs").clone())?;
            vault.save(&state)?;
            Ok(ui)
        })();
        match outcome {
            Ok(ui) => println!("{}", ui.string()),
            Err(error) => eprintln!("{error}"),
        }
    }
    vault.save(&state)?;
    Ok(())
}

/// The host opens its own sealed vault before the first render, so a restart
/// continues the saved conversation instead of starting cold.
fn startup(vault: &Vault, root: &str) -> Result<(Json, Json)> {
    let mut state = vault.load()?.unwrap_or(Null);
    let ui = advance(&mut state, root, "resume", Null)?;
    Ok((state, ui))
}

/// A loopback model peer so the Linux host is verified over a real socket, not a
/// mock: the core builds the request, the OS moves the bytes, the core parses the
/// reply.
struct Peer {
    port: u16,
    handle: std::thread::JoinHandle<()>,
}

impl Peer {
    fn start(replies: Vec<String>) -> Result<Peer> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
        let port = listener.local_addr().map_err(|error| error.to_string())?.port();
        let handle = std::thread::spawn(move || {
            for body in replies {
                let Ok((mut socket, _)) = listener.accept() else { return };
                let mut request = Vec::new();
                let mut chunk = [0u8; 8192];
                let mut wanted = usize::MAX;
                let mut header = 0usize;
                while request.len() < wanted {
                    match socket.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(read) => {
                            request.extend_from_slice(&chunk[..read]);
                            if header == 0 {
                                if let Some(at) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                                    header = at + 4;
                                    let head = String::from_utf8_lossy(&request[..header]).into_owned();
                                    let length = head
                                        .lines()
                                        .find(|line| line.to_ascii_lowercase().starts_with("content-length:"))
                                        .and_then(|line| line.split_once(':').map(|(_, value)| value.trim().parse::<usize>().unwrap_or(0)))
                                        .unwrap_or(0);
                                    wanted = header.saturating_add(length).min(host_byte_budget() as usize);
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                let wire = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(wire.as_bytes());
                let _ = socket.flush();
            }
        });
        Ok(Peer { port, handle })
    }

    fn finish(self) -> bool {
        self.handle.join().is_ok()
    }
}

fn collect_strings(value: &Json, into: &mut Vec<String>) {
    match value {
        Str(content) => into.push(content.clone()),
        Arr(items) => items.iter().for_each(|item| collect_strings(item, into)),
        Obj(fields) => fields.values().for_each(|item| collect_strings(item, into)),
        _ => {}
    }
}

fn profile(port: u16) -> Json {
    obj(vec![
        ("name", s("Linux loopback")),
        ("model", s("rllm-test-model")),
        ("endpoint", s(&format!("http://127.0.0.1:{port}/v1/chat/completions"))),
        ("plain", Bool(true)),
        ("stream", Bool(false)),
        ("protocol", s("chat")),
    ])
}

/// The Linux equivalent of a connected product run: real sockets, the real core
/// event path, real sealed vault bytes and the real workspace tool path.
fn self_test(vault: &Vault, root: &str) -> Result<()> {
    let mut checks = 0usize;
    let mut said = |name: &str, ok: bool| -> Result<()> {
        if !ok {
            return Err(format!("self-test failed: {name}"));
        }
        checks += 1;
        println!("ok   {name}");
        Ok(())
    };

    let opening = core_event(&Null, root, "resume", &Null)?;
    said("a cold resume renders the same core surface as Android", opening.get("ui")? != &Null)?;
    let mut state = opening.get("state")?.clone();
    said("a cold resume has no platform work outstanding", opening.get("effect")? == &Null)?;

    let peer = Peer::start(vec![
        "{\"choices\":[{\"message\":{\"content\":\"Linux host is online.\"},\"finish_reason\":\"stop\"}]}".into(),
        "{\"choices\":[{\"message\":{\"content\":\"The first turn was remembered.\"},\"finish_reason\":\"stop\"}]}".into(),
    ])?;
    state = set_field(state, "profiles", Arr(vec![profile(peer.port)]));
    state = set_field(state, "model", s("Linux loopback"));
    let ui = advance(&mut state, root, "send", obj(vec![("draft", s("Report the host state."))]))?;
    let mut shown = Vec::new();
    collect_strings(&ui, &mut shown);
    said("a sent turn reaches the model over a real socket", shown.iter().any(|line| line.contains("Linux host is online.")))?;
    said("the sent user text is visible in the same render", shown.iter().any(|line| line == "Report the host state."))?;

    let ui = advance(&mut state, root, "send", obj(vec![("draft", s("What did I ask first?"))]))?;
    let mut second = Vec::new();
    collect_strings(&ui, &mut second);
    said(
        "the second turn keeps the first turn in context",
        second.iter().any(|line| line.contains("The first turn was remembered.")) && second.iter().any(|line| line.contains("Linux host is online.")),
    )?;
    said("the model peer served exactly the requests the core built", peer.finish())?;

    vault.save(&state)?;
    let bytes = read_path_limited(&vault.path, host_byte_budget())?;
    said("the saved vault holds no plaintext conversation", !String::from_utf8_lossy(&bytes).contains("Linux host is online"))?;
    let reloaded = vault.load()?.ok_or("saved state disappeared")?;
    let mut kept = Vec::new();
    collect_strings(&reloaded, &mut kept);
    said("the saved state opens again with the same conversation", kept.iter().any(|line| line.contains("Linux host is online.")))?;
    let mut tampered = bytes.clone();
    tampered[13] ^= 1;
    std::fs::write(&vault.path, &tampered).map_err(|error| error.to_string())?;
    said("a one-byte change is refused instead of silently discarded", vault.load().is_err())?;
    std::fs::write(&vault.path, &bytes).map_err(|error| error.to_string())?;

    let (_, ui) = startup(vault, root)?;
    let mut resumed = Vec::new();
    collect_strings(&ui, &mut resumed);
    said(
        "a restart restores the sealed conversation into the first render",
        resumed.iter().any(|line| line.contains("Linux host is online.")),
    )?;
    let mut foreign_key = vault.key;
    foreign_key[0] ^= 1;
    let foreign = Vault { path: vault.path.clone(), key: foreign_key };
    said("a restart against a foreign key fails instead of starting cold", startup(&foreign, root).is_err())?;

    let mut tool_state = state.clone();
    let ui = advance(&mut tool_state, root, "investigation", obj(vec![("draft", s("Inspect the harness."))]))?;
    said("the investigation action settles on the Linux host", ui != Null)?;
    let ui = advance(&mut tool_state, root, "inspect_file", obj(vec![("path", s("README.md"))]))?;
    said("a workspace inspect settles on the Linux host", ui != Null)?;
    let ui = advance(&mut tool_state, root, "approve", Null)?;
    said(
        "the approved leg ran a real workspace tool through the host",
        EXECUTED_TOOLS.load(std::sync::atomic::Ordering::Relaxed) > 0,
    )?;
    said("an approved workspace tool returns to the core without stranding", ui != Null)?;

    // A deterministic microphone source stands in for the device so the capture
    // leg is reproducible; the byte path through the core is the real one.
    let source = Path::new(root).join("utterance.pcm");
    let tone: Vec<u8> = (0..320).flat_map(|n| (if n % 2 == 0 { 3000i16 } else { -3000i16 }).to_le_bytes()).collect();
    let mut pcm = Vec::new();
    for _ in 0..20 {
        pcm.extend_from_slice(&tone);
    }
    for _ in 0..60 {
        pcm.extend_from_slice(&[0u8; 640]);
    }
    std::fs::write(&source, &pcm).map_err(|error| format!("Voice source: {error}"))?;
    std::env::set_var("RLLM_VOICE_SOURCE", &source);
    let mut voice_state = state.clone();
    let asr = Peer::start(vec!["{\"text\":\"hello spoken\"}".into()])?;
    let config = obj(vec![
        ("version", Num("1".into())),
        (
            "hosts",
            Arr(vec![obj(vec![
                ("id", s("loopback")),
                ("url", s(&format!("http://127.0.0.1:{}", asr.port))),
                ("plain", Bool(true)),
                ("enabled", Bool(true)),
            ])]),
        ),
        (
            "routes",
            Arr(vec![
                obj(vec![
                    ("id", s("recognition")),
                    ("host", s("loopback")),
                    ("capability", s("speech_recognition")),
                    ("path", s("/asr")),
                    ("method", s("POST")),
                    ("request_format", s("binary")),
                    ("request", obj(vec![])),
                    ("file_field", s("audio")),
                    ("response", s("json")),
                    ("response_path", s("/text")),
                    ("artifact_kind", s("audio")),
                    ("enabled", Bool(true)),
                ]),
                obj(vec![
                    ("id", s("synthesis")),
                    ("host", s("loopback")),
                    ("capability", s("speech_synthesis")),
                    ("path", s("/tts")),
                    ("method", s("POST")),
                    ("request_format", s("json")),
                    ("request", obj(vec![("text", obj(vec![("$input", s("/text"))]))])),
                    ("response", s("base64")),
                    ("artifact_kind", s("audio")),
                    ("enabled", Bool(true)),
                ]),
            ]),
        ),
        (
            "defaults",
            obj(vec![
                ("speech_recognition", obj(vec![("routes", Arr(vec![s("recognition")]))])),
                ("speech_synthesis", obj(vec![("routes", Arr(vec![s("synthesis")]))])),
            ]),
        ),
    ]);
    advance(&mut voice_state, root, "service_connections", Null)?;
    advance(&mut voice_state, root, "service_save", obj(vec![("service_config", s(&config.string()))]))?;
    advance(&mut voice_state, root, "voice_open_review", Null)?;
    let fed = CAPTURED_FRAMES.load(std::sync::atomic::Ordering::Relaxed);
    let ui = advance(&mut voice_state, root, "voice_consent", Null)?;
    std::env::remove_var("RLLM_VOICE_SOURCE");
    said("the microphone delivered real frames through the host", CAPTURED_FRAMES.load(std::sync::atomic::Ordering::Relaxed) > fed)?;
    let stored = std::fs::read_dir(Path::new(root).join("service-files"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.file_name().to_string_lossy().ends_with(".wav"))
        .and_then(|entry| read_path_limited(&entry.path(), host_byte_budget()).ok())
        .unwrap_or_default();
    said(
        "the core stored the utterance captured on the Linux host",
        stored.starts_with(b"RIFF") && stored.len() > 44,
    )?;
    let ui = advance(&mut voice_state, root, "resume", Null)?;
    let mut heard = Vec::new();
    collect_strings(&ui, &mut heard);
    said(
        "the captured utterance reached the configured recognition connection",
        heard.iter().any(|line| line.contains("hello spoken")),
    )?;
    said("the microphone settles on the Linux host without stranding", ui != Null)?;
    said("the recognition peer served exactly the requests the core built", asr.finish())?;

    let sealed = state_crypto::seal(b"linux vault".as_slice(), &vault.key)?;
    said("the core opens a vault the host sealed", state_crypto::open(&sealed, &vault.key)? == b"linux vault")?;
    let mut wrong = vault.key;
    wrong[0] ^= 1;
    said("the core refuses a vault from another key", state_crypto::open(&sealed, &wrong).is_err())?;

    let ui = advance(&mut state.clone(), root, "resume", Null)?;
    said("the surface still renders after every leg", ui != Null)?;
    println!("desktop self-test: {checks} checks passed");
    Ok(())
}

#[cfg(windows)]
fn hide_console() {
    unsafe extern "system" {
        fn GetConsoleWindow() -> isize;
        fn ShowWindow(window: isize, command: i32) -> i32;
        fn FreeConsole() -> i32;
    }
    unsafe {
        let window = GetConsoleWindow();
        if window != 0 {
            ShowWindow(window, 0);
        }
        FreeConsole();
    }
}

#[cfg(not(windows))]
fn browse_folder(title: &str) -> Result<Option<String>> {
    let mut tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").map_err(|_| "Choosing a folder needs a terminal".to_string())?;
    writeln!(tty, "{title}").map_err(|error| format!("Choosing a folder: {error}"))?;
    let mut current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let mut line = String::new();
    loop {
        writeln!(tty, "{}", path_text(&current)).map_err(|error| format!("Choosing a folder: {error}"))?;
        let mut entries = Vec::new();
        if let Ok(listing) = host_fs::desktop_directory(&current) {
            for entry in listing.take(200) {
                if let Ok(entry) = entry {
                    if entry.directory {
                        entries.push(entry.path);
                    }
                }
            }
        }
        entries.sort();
        for (index, path) in entries.iter().enumerate() {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path_text(path));
            writeln!(tty, "{index} {name}").map_err(|error| format!("Choosing a folder: {error}"))?;
        }
        writeln!(tty, "Enter a number to open that folder, .. to go up, . to select this folder, a full path to select it, or nothing to cancel.").map_err(|error| format!("Choosing a folder: {error}"))?;
        line.clear();
        read_tty_line(&mut tty, &mut line)?;
        let choice = line.trim();
        if choice.is_empty() {
            return Ok(None);
        }
        if choice == "." {
            return Ok(Some(path_text(&current)));
        }
        if choice == ".." {
            if let Some(parent) = current.parent() {
                if !parent.as_os_str().is_empty() {
                    current = parent.to_path_buf();
                }
            }
            continue;
        }
        if choice.starts_with('/') {
            let path = PathBuf::from(choice);
            match host_fs::desktop_metadata(&path, false) {
                Ok(meta) if meta.is_dir() => return Ok(Some(path_text(&path))),
                _ => {
                    writeln!(tty, "That path is not a folder.").ok();
                    continue;
                }
            }
        }
        if let Ok(index) = choice.parse::<usize>() {
            if let Some(path) = entries.get(index) {
                current = path.clone();
            }
        }
    }
}

#[cfg(not(windows))]
fn read_tty_line(tty: &mut std::fs::File, line: &mut String) -> Result<()> {
    *line = read_typed_line(tty).map_err(|error| format!("Choosing a folder: {error}"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_open_panel(title: &str) -> Result<Option<String>> {
    unsafe extern "C" {
        fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    type Send = unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    type SendBool = unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, i8);
    type SendInt = unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> isize;
    unsafe {
        let class = objc_getClass(c"NSOpenPanel".as_ptr());
        if class.is_null() {
            return Err("The folder panel is unavailable".into());
        }
        let panel = std::mem::transmute::<_, Send>(objc_msgSend)(class, sel_registerName(c"openPanel".as_ptr()));
        if panel.is_null() {
            return Err("The folder panel is unavailable".into());
        }
        let app = objc_getClass(c"NSApplication".as_ptr());
        if !app.is_null() {
            std::mem::transmute::<_, Send>(objc_msgSend)(app, sel_registerName(c"sharedApplication".as_ptr()));
        }
        std::mem::transmute::<_, SendBool>(objc_msgSend)(panel, sel_registerName(c"setCanChooseDirectories:".as_ptr()), 1);
        std::mem::transmute::<_, SendBool>(objc_msgSend)(panel, sel_registerName(c"setCanChooseFiles:".as_ptr()), 0);
        std::mem::transmute::<_, SendBool>(objc_msgSend)(panel, sel_registerName(c"setAllowsMultipleSelection:".as_ptr()), 0);
        let ns_string = objc_getClass(c"NSString".as_ptr());
        let title_bytes = std::ffi::CString::new(title).unwrap_or_else(|_| std::ffi::CString::new("Choose a folder").unwrap());
        let label = std::mem::transmute::<_, unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *const i8) -> *mut std::ffi::c_void>(objc_msgSend)(
            ns_string,
            sel_registerName(c"stringWithUTF8String:".as_ptr()),
            title_bytes.as_ptr(),
        );
        std::mem::transmute::<_, unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void)>(objc_msgSend)(
            panel,
            sel_registerName(c"setTitle:".as_ptr()),
            label,
        );
        let response = std::mem::transmute::<_, SendInt>(objc_msgSend)(panel, sel_registerName(c"runModal".as_ptr()));
        if response != 1 {
            return Ok(None);
        }
        let url = std::mem::transmute::<_, Send>(objc_msgSend)(panel, sel_registerName(c"URL".as_ptr()));
        if url.is_null() {
            return Ok(None);
        }
        let text = std::mem::transmute::<_, Send>(objc_msgSend)(url, sel_registerName(c"path".as_ptr()));
        let bytes = std::mem::transmute::<_, unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> *const i8>(objc_msgSend)(text, sel_registerName(c"UTF8String".as_ptr()));
        if bytes.is_null() {
            return Ok(None);
        }
        Ok(Some(std::ffi::CStr::from_ptr(bytes).to_string_lossy().into_owned()))
    }
}

#[cfg(target_os = "macos")]
fn macos_open_address(url: &str) -> Result<()> {
    unsafe extern "C" {
        fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    type Send = unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    type SendC = unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *const i8) -> *mut std::ffi::c_void;
    type SendObj = unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void) -> u8;
    let address = std::ffi::CString::new(url).map_err(|_| "Authorization address is not usable".to_string())?;
    unsafe {
        let workspace_class = objc_getClass(c"NSWorkspace".as_ptr());
        let string_class = objc_getClass(c"NSString".as_ptr());
        let url_class = objc_getClass(c"NSURL".as_ptr());
        if workspace_class.is_null() || string_class.is_null() || url_class.is_null() {
            return Err("Authorization could not be opened".into());
        }
        let workspace = std::mem::transmute::<_, Send>(objc_msgSend)(workspace_class, sel_registerName(c"sharedWorkspace".as_ptr()));
        let text = std::mem::transmute::<_, SendC>(objc_msgSend)(string_class, sel_registerName(c"stringWithUTF8String:".as_ptr()), address.as_ptr());
        let target = std::mem::transmute::<_, unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void>(objc_msgSend)(
            url_class,
            sel_registerName(c"URLWithString:".as_ptr()),
            text,
        );
        if workspace.is_null() || target.is_null() {
            return Err("Authorization could not be opened".into());
        }
        let opened = std::mem::transmute::<_, SendObj>(objc_msgSend)(workspace, sel_registerName(c"openURL:".as_ptr()), target);
        if opened == 0 {
            return Err("Authorization could not be opened".into());
        }
        Ok(())
    }
}

fn phone_revision(effect: &Json) -> Result<Json> {
    let tree = text(effect, "tree");
    if !native_project_folder(&tree) {
        return Err("Grant a writable device folder first".into());
    }
    let folder = PathBuf::from(&tree);
    let relative = text(effect, "path");
    let target = revision_target(&folder, &relative)?;
    match text(effect, "phase").as_str() {
        "read" => {
            let mut file = open_revision_file(&target, nonblocking_read_flags())?;
            let (bytes, partial, total) = read_revision_bytes(&mut file)?;
            let parent = target.path.parent().unwrap_or(folder.as_path());
            let mut fields = vec![
                ("document_id", s(&path_text(&target.path))),
                ("parent_id", s(&path_text(parent))),
                ("data", s(&network::b64(&bytes))),
            ];
            if partial {
                fields.push(("coverage", s("partial")));
                fields.push(("reason", s("inline body stopped at the host memory budget")));
                fields.push(("bytes", Num(total.to_string())));
            }
            Ok(obj(fields))
        }
        "backup" => {
            if path_text(&target.path) != text(effect, "document_id") {
                return Err("Device revision path changed; reopen the revision".into());
            }
            let parent = target.path.parent().ok_or_else(|| "Device revision has no parent folder".to_string())?;
            if path_text(parent) != text(effect, "parent_id") {
                return Err("Device revision path changed; reopen the revision".into());
            }
            let name = text(effect, "revision_name");
            let previous = text(effect, "previous").into_bytes();
            if previous.len() as u64 > host_byte_budget() {
                return Err("Device revision exceeds the memory budget".into());
            }
            let backup = write_revision_copy(&target, &name, &previous)?;
            let backup_name = backup.file_name().and_then(|name| name.to_str()).unwrap_or(&name).to_string();
            Ok(obj(vec![
                ("document_id", s(&path_text(&backup))),
                ("name", s(&backup_name)),
                ("data", s(&network::b64(&previous))),
            ]))
        }
        "write" => {
            let replacement = text(effect, "content").into_bytes();
            let mut file = open_revision_file(&target, 2)?;
            replace_device_text(&mut file, &target.path, &replacement, effect)?;
            file.seek(std::io::SeekFrom::Start(0)).map_err(|error| format!("Device update could not be written: {error}"))?;
            let (after, partial, total) = read_revision_bytes(&mut file)?;
            let mut fields = vec![("data", s(&network::b64(&after)))];
            if partial {
                fields.push(("coverage", s("partial")));
                fields.push(("reason", s("inline body stopped at the host memory budget")));
                fields.push(("bytes", Num(total.to_string())));
            }
            Ok(obj(fields))
        }
        _ => Err("Unexpected device revision phase".into()),
    }
}

struct RevisionTarget {
    path: PathBuf,
    parent: std::fs::File,
    name: String,
}

fn revision_parts(relative: &str) -> Result<Vec<String>> {
    if relative.is_empty() || relative.len() > 1024 || relative.contains(['\\', '\0', '%', ':']) || relative.split('/').any(|part| part.is_empty() || part == "." || part == ".." || part.len() > 255) {
        return Err("Use a plain relative device file path inside the selected folder".into());
    }
    Ok(relative.split('/').map(str::to_owned).collect())
}

fn metadata_is_link(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

fn revision_contained(root: &Path, child: &Path) -> bool {
    #[cfg(windows)]
    {
        let root = windows_path_key(root);
        let child = windows_path_key(child);
        return child == root || child.strip_prefix(&root).is_some_and(|rest| rest.starts_with('\\'));
    }
    #[cfg(not(windows))]
    {
        child.starts_with(root)
    }
}

fn revision_target(folder: &Path, relative: &str) -> Result<RevisionTarget> {
    use host_fs::OpenOptionsExt;
    let parts = revision_parts(relative)?;
    let root = std::fs::canonicalize(folder).map_err(|error| format!("The selected folder could not be opened: {error}"))?;
    if !root.is_dir() {
        return Err("Choose a folder".into());
    }
    let mut dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(directory_open_flag())
        .open(&root)
        .map_err(|error| format!("The selected folder could not be opened: {error}"))?;
    let mut path = root.clone();
    let name = parts[parts.len() - 1].clone();
    for part in &parts[..parts.len() - 1] {
        path.push(part);
        let meta = std::fs::symlink_metadata(&path).map_err(|error| format!("Device file could not be read: {error}"))?;
        if metadata_is_link(&meta) {
            return Err("Refusing to follow a link out of the selected folder".into());
        }
        if !meta.is_dir() {
            return Err("Use a plain relative device file path inside the selected folder".into());
        }
        let opened = host_fs::open_at(&dir, part.as_bytes(), 0x20000 | 0x10000).map_err(|error| format!("Refusing to follow a link out of the selected folder: {error}"))?;
        let opened_meta = opened.metadata().map_err(|error| format!("Device file could not be read: {error}"))?;
        if metadata_is_link(&opened_meta) || !opened_meta.is_dir() {
            return Err("Refusing to follow a link out of the selected folder".into());
        }
        let resolved = std::fs::canonicalize(&path).map_err(|error| format!("Device file could not be read: {error}"))?;
        if !revision_contained(&root, &resolved) {
            return Err("Refusing to follow a link out of the selected folder".into());
        }
        dir = opened;
    }
    path.push(&name);
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if metadata_is_link(&meta) {
            return Err("Refusing to follow a link out of the selected folder".into());
        }
        if meta.is_file() {
            let resolved = std::fs::canonicalize(&path).map_err(|error| format!("Device file could not be read: {error}"))?;
            if !revision_contained(&root, &resolved) {
                return Err("Refusing to follow a link out of the selected folder".into());
            }
        }
    }
    Ok(RevisionTarget { path, parent: dir, name })
}

fn open_revision_file(target: &RevisionTarget, flags: i32) -> Result<std::fs::File> {
    let file = host_fs::open_at(&target.parent, target.name.as_bytes(), flags | 0x20000).map_err(|error| format!("Device file could not be read: {error}"))?;
    let meta = file.metadata().map_err(|error| format!("Device file could not be read: {error}"))?;
    if metadata_is_link(&meta) {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    if !meta.is_file() {
        return Err("Device revision target must be a regular file".into());
    }
    confirm_inside(&target.parent, &file, &target.name)?;
    Ok(file)
}

fn read_revision_bytes(file: &mut std::fs::File) -> Result<(Vec<u8>, bool, u64)> {
    let total = file.metadata().map_err(|error| format!("Device file could not be read: {error}"))?.len();
    let budget = host_byte_budget();
    if total > budget {
        let mut prefix = Vec::new();
        std::io::Read::take(file, 65536).read_to_end(&mut prefix).map_err(|error| format!("Device file could not be read: {error}"))?;
        return Ok((prefix, true, total));
    }
    let bytes = read_file_limited(file, budget).map_err(|error| format!("Device file could not be read: {error}"))?;
    let total = bytes.len() as u64;
    Ok((bytes, false, total))
}

fn write_revision_copy(target: &RevisionTarget, name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let parts = revision_parts(name)?;
    if parts.len() != 1 {
        return Err("Use a plain relative device file path inside the selected folder".into());
    }
    let backup = target.path.parent().unwrap_or(Path::new("")).join(&parts[0]);
    if let Ok(meta) = std::fs::symlink_metadata(&backup) {
        if metadata_is_link(&meta) {
            return Err("Refusing to follow a link out of the selected folder".into());
        }
        return Err("Provider did not create a separate device revision".into());
    }
    let mut file = host_fs::open_at(&target.parent, parts[0].as_bytes(), 1 | 0x40 | 0x80 | 0x20000).map_err(|error| format!("Device revision could not be created: {error}"))?;
    let meta = file.metadata().map_err(|error| format!("Device revision could not be created: {error}"))?;
    if metadata_is_link(&meta) {
        return Err("Refusing to follow a link out of the selected folder".into());
    }
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|error| format!("Device revision could not be created: {error}"))?;
    Ok(backup)
}

fn replace_device_text(file: &mut std::fs::File, path: &Path, replacement: &[u8], effect: &Json) -> Result<()> {
    if replacement.len() as u64 > host_byte_budget() {
        return Err("Device revision exceeds the memory budget; the target was not changed".into());
    }
    lock_device_file(file)?;
    let meta = file.metadata().map_err(|error| format!("Device revision target is unavailable: {error}"))?;
    if meta.len() > host_byte_budget() {
        return Err("Device revision exceeds the memory budget; the target was not changed".into());
    }
    let before = read_file_limited(file, host_byte_budget()).map_err(|error| format!("Device original could not be read: {error}"))?;
    core_call(obj(vec![
        ("op", s("phone_revision_check")),
        ("record", effect.clone()),
        ("document_id", s(&path_text(path))),
        ("data", s(&network::b64(&before))),
    ]))?;
    file.seek(std::io::SeekFrom::Start(0)).map_err(|error| format!("Device update could not be written: {error}"))?;
    file.set_len(0).map_err(|error| format!("Device update could not be written: {error}"))?;
    file.write_all(replacement).and_then(|_| file.sync_all()).map_err(|error| format!("Device update could not be written: {error}"))
}

fn lock_device_file(file: &std::fs::File) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        struct Overlapped {
            internal: usize,
            internal_high: usize,
            offset: u32,
            offset_high: u32,
            event: isize,
        }
        unsafe extern "system" {
            fn LockFileEx(file: *mut std::ffi::c_void, flags: u32, reserved: u32, low: u32, high: u32, overlapped: *mut Overlapped) -> i32;
        }
        let mut overlapped = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: 0 };
        let locked = unsafe { LockFileEx(file.as_raw_handle(), 2, 0, u32::MAX, u32::MAX, &mut overlapped) };
        if locked == 0 {
            return Err("Device file is locked by another writer".into());
        }
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        if unsafe { flock(file.as_raw_fd(), 2) } != 0 {
            return Err("Device file is locked by another writer".into());
        }
        return Ok(());
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        Ok(())
    }
}

#[cfg(windows)]
mod window;
#[cfg(windows)]
mod secure;
mod tls_client;

#[cfg(any(test, g1_regression))]
mod desktop_regression {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("loops-lanes").join("G1-fix4-desktop-proof").join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(windows)]
    fn link_directory(link: &Path, target: &Path) -> std::io::Result<()> {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return Ok(());
        }
        std::fs::create_dir(link)?;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        let canonical = std::fs::canonicalize(target)?;
        let mut display = canonical.to_string_lossy().replace('/', "\\");
        if let Some(rest) = display.strip_prefix(r"\\?\UNC\") {
            display = format!(r"\\{rest}");
        } else if let Some(rest) = display.strip_prefix(r"\\?\") {
            display = rest.to_string();
        }
        let print = display.trim_end_matches('\\').to_string();
        let substitute = format!(r"\??\{print}");
        let sub: Vec<u16> = substitute.encode_utf16().collect();
        let printed: Vec<u16> = print.encode_utf16().collect();
        let mut path = Vec::new();
        path.extend_from_slice(&sub);
        path.push(0);
        let print_offset = path.len() * 2;
        path.extend_from_slice(&printed);
        path.push(0);
        let path_bytes = path.len() * 2;
        let data_length = 8 + path_bytes;
        let mut buf = Vec::<u8>::new();
        buf.extend_from_slice(&0xA000_0003u32.to_le_bytes());
        buf.extend_from_slice(&(data_length as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&((sub.len() * 2) as u16).to_le_bytes());
        buf.extend_from_slice(&(print_offset as u16).to_le_bytes());
        buf.extend_from_slice(&((printed.len() * 2) as u16).to_le_bytes());
        for unit in path {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        let file = std::fs::OpenOptions::new().write(true).custom_flags(0x0200_0000 | 0x0020_0000).open(link)?;
        unsafe extern "system" {
            fn DeviceIoControl(handle: *mut std::ffi::c_void, code: u32, input: *const std::ffi::c_void, input_size: u32, output: *mut std::ffi::c_void, output_size: u32, returned: *mut u32, overlapped: *mut std::ffi::c_void) -> i32;
        }
        let mut returned = 0u32;
        let ok = unsafe { DeviceIoControl(file.as_raw_handle(), 0x0009_00A4, buf.as_ptr().cast(), buf.len() as u32, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) };
        if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
    }

    #[cfg(unix)]
    fn link_directory(link: &Path, target: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg_attr(test, test)]
    fn revision_rejects_a_linked_parent() {
        let root = scratch("link-parent");
        let selected = root.join("selected");
        let outside = root.join("outside");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        link_directory(&selected.join("link"), &outside).unwrap();
        let err = match revision_target(&selected, "link/secret.txt") {
            Err(error) => error,
            Ok(_) => panic!("a linked parent was accepted"),
        };
        assert!(err.contains("link"), "{err}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
    }

    #[cfg_attr(test, test)]
    fn revision_reads_binary_past_one_mebibyte() {
        let root = scratch("binary-revision");
        let selected = root.join("selected");
        std::fs::create_dir_all(selected.join("nested")).unwrap();
        let mut bytes = vec![0u8, 255];
        bytes.extend(std::iter::repeat(b'Q').take(1024 * 1024));
        std::fs::write(selected.join("nested").join("note.bin"), &bytes).unwrap();
        let target = revision_target(&selected, "nested/note.bin").unwrap();
        let mut file = open_revision_file(&target, 0).unwrap();
        let (got, partial, total) = read_revision_bytes(&mut file).unwrap();
        assert!(!partial);
        assert_eq!(total, bytes.len() as u64);
        assert_eq!(got, bytes);
    }

    #[cfg_attr(test, test)]
    fn limited_read_stops_before_a_declared_huge_file() {
        let root = scratch("huge-read");
        let path = root.join("big.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
        drop(file);
        let err = read_path_limited(&path, 1024).unwrap_err();
        assert!(err.contains("exceeds"), "{err}");
    }

    #[cfg_attr(test, test)]
    fn failed_project_write_preserves_the_handle_and_not_a_replacement() {
        let root = scratch("partial-write");
        let dir = root.join("folder");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        let mut file = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&path).unwrap();
        file.write_all(b"ours").unwrap();
        std::fs::rename(&path, dir.join("moved.txt")).unwrap();
        std::fs::write(&path, b"theirs").unwrap();
        let kept = keep_failed_project_output(&mut file, &path, &path_text(&root)).unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), b"ours");
        assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
    }

    #[cfg_attr(test, test)]
    fn stream_deadline_does_not_restart_when_bytes_arrive() {
        struct Trickle { n: usize }
        impl std::io::Read for Trickle {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.n >= 20 { return Ok(0); }
                std::thread::sleep(std::time::Duration::from_millis(20));
                self.n += 1;
                buf[0] = b'a';
                Ok(1)
            }
        }
        let mut stream = Trickle { n: 0 };
        let err = collect_until(&mut stream, 1024 * 1024, std::time::Instant::now() + std::time::Duration::from_millis(50)).unwrap_err();
        assert!(err.contains("two minutes"), "{err}");
        assert!(stream.n < 8, "deadline restarted, accepted {} bytes", stream.n);
    }

    #[cfg_attr(test, test)]
    fn empty_capture_is_not_a_fatal_read() {
        struct Empty { hits: u8 }
        impl std::io::Read for Empty {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.hits += 1;
                if self.hits == 1 {
                    return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "empty"));
                }
                buf[0] = 7;
                Ok(1)
            }
        }
        let mut empty = Empty { hits: 0 };
        let mut buf = [0u8; 4];
        let err = read_capture(&mut empty, &mut buf, std::time::Instant::now()).unwrap_err();
        assert!(err.contains("deadline"), "{err}");
        assert!(!err.contains("Microphone stream"));
        let mut once = Empty { hits: 0 };
        let n = read_capture(&mut once, &mut buf, std::time::Instant::now() + std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 7);
    }

    #[cfg_attr(test, test)]
    fn session_hello_matches_the_published_core_method() {
        let start = session_start();
        let hello_at = start.iter().position(|frame| frame.id == 0 && frame.opcode == CORE_HELLO).unwrap();
        let props_at = start.iter().position(|frame| frame.id == 1 && frame.opcode == CLIENT_UPDATE_PROPERTIES).unwrap();
        let sync_at = start.iter().position(|frame| frame.id == 0 && frame.opcode == CORE_SYNC).unwrap();
        assert!(hello_at < props_at && props_at < sync_at);
        let hello = &start[hello_at];
        assert_eq!(u32::from_le_bytes(hello.body[4..8].try_into().unwrap()), POD_STRUCT);
        assert!(hello.body.windows(4).any(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()) == CORE_VERSION));
        let props = &start[props_at];
        let key = b"application.name";
        assert!(props.body.windows(key.len()).any(|window| window == key));
        let sync = &start[sync_at];
        assert_eq!(u32::from_le_bytes(sync.body[4..8].try_into().unwrap()), POD_STRUCT);
        assert!(sync.body.len() > 16);
    }

    fn rate_object(value: u32) -> Vec<u8> {
        let mut prop = Vec::new();
        prop.extend_from_slice(&0x10002u32.to_le_bytes());
        prop.extend_from_slice(&0u32.to_le_bytes());
        prop.extend(pod_int(value));
        let mut body = Vec::new();
        body.extend_from_slice(&0x40003u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend(prop);
        let mut out = Vec::new();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&POD_OBJECT.to_le_bytes());
        out.extend(body);
        out
    }

    #[cfg_attr(test, test)]
    fn session_negotiation_returns_samples_only_after_a_capture_buffer() {
        let mut nego = SessionNegotiator::new();
        let idle = nego.push(&SoundFrame { id: 0, opcode: CORE_EVENT_PING, seq: 4, body: Vec::new(), mem: Vec::new() }).unwrap();
        assert!(idle.iter().all(|frame| frame.opcode != CORE_GET_REGISTRY));
        let replies = nego.push(&SoundFrame { id: 0, opcode: CORE_EVENT_DONE, seq: 0, body: Vec::new(), mem: Vec::new() }).unwrap();
        assert!(replies.iter().any(|frame| frame.opcode == CORE_GET_REGISTRY), "done did not ask for the registry");
        assert!(replies.iter().any(|frame| frame.opcode == CORE_SYNC), "registry was not followed by sync");
        assert_eq!(nego.registry, 2);
        assert!(!nego.ready());
        let global = {
            let mut body = pod_int(4);
            body.extend(pod_int(1));
            body.extend(pod_string("Audio/Source"));
            body.extend(pod_int(3));
            body
        };
        let created = nego.push(&SoundFrame { id: nego.registry, opcode: 0, seq: 0, body: global, mem: Vec::new() }).unwrap();
        assert!(created.iter().any(|frame| frame.opcode == CORE_CREATE_OBJECT));
        assert!(!nego.ready());
        nego.push(&SoundFrame { id: nego.capture, opcode: NODE_EVENT_SET_PARAM, seq: 0, body: rate_object(16000), mem: Vec::new() }).unwrap();
        nego.push(&SoundFrame { id: nego.capture, opcode: NODE_EVENT_PORT_BUFFERS, seq: 0, body: Vec::new(), mem: vec![vec![1, 2, 3, 4]] }).unwrap();
        assert!(nego.ready());
        assert_eq!(nego.samples, vec![1, 2, 3, 4]);
    }

    #[cfg_attr(test, test)]
    fn nested_format_rate_is_recognized_and_a_hostile_size_is_not() {
        assert!(session_has_rate(&rate_object(16000)));
        assert!(!session_has_rate(&rate_object(48000)));
        assert!(!session_has_rate(&pod_int(48000)));
        assert!(session_has_rate(&pod_int(16000)));
        let mut hostile = rate_object(16000);
        hostile[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!session_has_rate(&hostile));
        let mut nested = pod_struct(&[rate_object(16000)]);
        assert!(session_has_rate(&nested));
        nested[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!session_has_rate(&nested));
        let choice = {
            let child = pod_int(16000);
            let mut body = Vec::new();
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend(&child);
            let mut out = Vec::new();
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&POD_CHOICE.to_le_bytes());
            out.extend(body);
            out
        };
        assert!(session_has_rate(&choice));
        let mut deep = pod_int(16000);
        for _ in 0..32 {
            deep = pod_struct(&[deep]);
        }
        assert!(!session_has_rate(&deep));
    }

    #[cfg_attr(test, test)]
    fn stdin_line_stops_at_the_parse_limit() {
        let mut exact = std::io::Cursor::new(b"{\"a\":1}\n".to_vec());
        assert_eq!(next_line_bounded(&mut exact, 8).unwrap().unwrap(), "{\"a\":1}");
        let mut over = std::io::Cursor::new(vec![b'x'; 20]);
        let err = next_line_bounded(&mut over, 8).unwrap_err();
        assert!(err.contains("8 MiB"), "{err}");
        let mut split = std::io::Cursor::new(b"0123456789EXTRA".to_vec());
        let err = next_line_bounded(&mut split, 8).unwrap_err();
        assert!(err.contains("8 MiB"), "{err}");
    }

    #[cfg_attr(test, test)]
    fn swapped_directory_entry_cannot_escape_the_selected_folder() {
        let root = scratch("swap-entry");
        let selected = root.join("selected");
        let outside = root.join("outside");
        let gate = selected.join("gate");
        std::fs::create_dir_all(&gate).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(gate.join("secret.txt"), b"inside").unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        let bound = canonical_root(&selected).unwrap();
        let dir = open_root_dir(&bound).unwrap();
        std::fs::remove_dir_all(&gate).unwrap();
        link_directory(&gate, &outside).unwrap();
        let opened = walk_once(&bound, Path::new("gate/secret.txt"), 0, &LinkEnd::Follow);
        match opened {
            Ok(Walked::Opened(mut file)) => {
                let mut buf = Vec::new();
                use std::io::Read;
                file.read_to_end(&mut buf).ok();
                assert_ne!(buf, b"secret");
            }
            Ok(Walked::Follow(next)) => {
                assert!(path_contained(&bound, &bound.join(&next)), "{next:?}");
                let err = confined_open(&selected, &selected.join("gate").join("secret.txt"), 0, LinkEnd::Follow).unwrap_err();
                assert!(err.to_ascii_lowercase().contains("selected folder") || err.to_ascii_lowercase().contains("link"), "{err}");
            }
            Err(error) => {
                assert!(error.to_ascii_lowercase().contains("selected folder") || error.to_ascii_lowercase().contains("link"), "{error}");
            }
        }
        drop(dir);
        let err = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("gate/secret.txt"))])),
        ]), &path_text(&selected)).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("selected folder") || err.to_ascii_lowercase().contains("link"), "{err}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
    }

    #[cfg_attr(test, test)]
    fn in_folder_link_still_reads_the_target_file() {
        let root = scratch("inside-link");
        let selected = root.join("selected");
        let nested = selected.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("note.txt"), b"note").unwrap();
        link_directory(&selected.join("link"), &nested).unwrap();
        let text = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("link/note.txt"))])),
        ]), &path_text(&selected)).unwrap();
        assert_eq!(text, "note");
    }

    #[cfg(windows)]
    #[cfg_attr(test, test)]
    fn wave_header_matches_the_published_record() {
        assert_eq!(std::mem::size_of::<WaveHdr>(), std::mem::size_of::<usize>() * 4 + 16);
        assert_eq!(std::mem::offset_of!(WaveHdr, next), std::mem::size_of::<usize>() * 2 + 16);
    }

    #[cfg(windows)]
    #[cfg_attr(test, test)]
    fn failed_microphone_open_closes_the_event_handle() {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateEventW(attrs: *mut std::ffi::c_void, manual: i32, initial: i32, name: *const u16) -> isize;
            fn GetHandleInformation(handle: isize, flags: *mut u32) -> i32;
        }
        for status in [1u32, 7, 32] {
            let event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
            assert_ne!(event, 0, "the microphone event could not be created");
            let mut flags = 0u32;
            assert_ne!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "event was not live before the failed open");
            let error = reject_failed_wave_open(event, status).expect_err("a failed microphone open was accepted");
            assert_eq!(error, format!("the microphone could not be opened ({status})"));
            assert_eq!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "event stayed open after status {status}");
        }
        let event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
        assert_ne!(event, 0, "the microphone event could not be created");
        let mut flags = 0u32;
        assert!(reject_failed_wave_open(event, 0).is_ok(), "a successful open status was rejected");
        assert_ne!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "a successful open status closed the event");
        close_wave_event(event);
        assert_eq!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "event stayed open after the success-path cleanup");
        println!("failed_microphone_open_closes_the_event_handle passed");
    }

    #[cfg_attr(test, test)]
    fn native_tools_refuse_paths_outside_the_selected_folder() {
        let root = scratch("confine");
        let selected = root.join("selected");
        let outside = root.join("outside");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        std::fs::write(selected.join("note.txt"), b"note").unwrap();
        let selected_text = path_text(&selected);
        let outside_file = path_text(&outside.join("secret.txt"));
        let escaped = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s(&outside_file))])),
        ]), &selected_text).unwrap_err();
        assert!(escaped.to_ascii_lowercase().contains("selected folder"), "{escaped}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
        let listed = device_folder_tool(&obj(vec![
            ("tool", s("phone_list")),
            ("args", obj(vec![("path", s(&path_text(&outside)))])),
        ]), &selected_text).unwrap_err();
        assert!(listed.to_ascii_lowercase().contains("selected folder"), "{listed}");
        let parent = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("../outside/secret.txt"))])),
        ]), &selected_text).unwrap_err();
        assert!(parent.to_ascii_lowercase().contains("selected folder"), "{parent}");
        let absolute = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s(&path_text(&selected.join("note.txt"))))])),
        ]), &selected_text).unwrap();
        assert_eq!(absolute, "note");
        let relative = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("note.txt"))])),
        ]), &selected_text).unwrap();
        assert_eq!(relative, "note");
        link_directory(&selected.join("link"), &outside).unwrap();
        let linked = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("link/secret.txt"))])),
        ]), &selected_text).unwrap_err();
        assert!(linked.to_ascii_lowercase().contains("selected folder"), "{linked}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
        #[cfg(windows)]
        {
            let drive = device_folder_tool(&obj(vec![
                ("tool", s("phone_list")),
                ("args", obj(vec![("path", s(r"C:\"))])),
            ]), &selected_text).unwrap_err();
            assert!(drive.to_ascii_lowercase().contains("selected folder"), "{drive}");
            let slash = device_folder_tool(&obj(vec![
                ("tool", s("phone_stat")),
                ("args", obj(vec![("path", s("C:/Windows"))])),
            ]), &selected_text).unwrap_err();
            assert!(slash.to_ascii_lowercase().contains("selected folder"), "{slash}");
        }
    }

    fn fill_socket(stream: &mut std::net::TcpStream) {
        stream.set_nonblocking(true).unwrap();
        let chunk = vec![b'x'; 64 * 1024];
        loop {
            match std::io::Write::write(stream, &chunk) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
                Err(error) => panic!("fill: {error}"),
            }
        }
        stream.set_nonblocking(false).unwrap();
    }

    #[cfg_attr(test, test)]
    fn stalled_upload_cannot_outlive_its_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let held = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3));
            drop(socket);
        });
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        fill_socket(&mut stream);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
        let _watch = arm_write_deadline(&stream, deadline);
        let started = std::time::Instant::now();
        let effect = obj(vec![("wire", s(&"x".repeat(64 * 1024)))]);
        let err = write_request(&mut stream, &effect, deadline).unwrap_err();
        let elapsed = started.elapsed();
        assert!(err.contains("two minutes"), "{err}");
        assert!(elapsed < std::time::Duration::from_secs(2), "upload held the deadline for {elapsed:?}");
        drop(_watch);
        drop(stream);
        let _ = held.join();
    }

    #[cfg_attr(test, test)]
    fn stalled_write_timeout_cannot_restart_the_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let held = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3));
            drop(socket);
        });
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        fill_socket(&mut stream);
        stream.set_write_timeout(Some(std::time::Duration::from_millis(40))).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        let started = std::time::Instant::now();
        let err = write_all_until(&mut stream, &[b'y'; 64 * 1024], deadline).unwrap_err();
        let elapsed = started.elapsed();
        assert!(err.contains("two minutes"), "{err}");
        assert!(elapsed < std::time::Duration::from_secs(2), "timed-out writes ran for {elapsed:?}");
        assert!(elapsed >= std::time::Duration::from_millis(200), "deadline was not the two-minute budget: {elapsed:?}");
        drop(stream);
        let _ = held.join();
    }

    #[cfg_attr(test, test)]
    fn audio_queue_samples_follow_capacity_not_the_user_pointer() {
        let pointer = std::mem::size_of::<usize>();
        assert_eq!(std::mem::offset_of!(QueueBuffer, capacity), 0);
        assert_eq!(std::mem::offset_of!(QueueBuffer, audio_data), pointer);
        assert_eq!(std::mem::offset_of!(QueueBuffer, byte_size), pointer.saturating_mul(2));
        let user = std::mem::offset_of!(QueueBuffer, user_data);
        assert!(user > std::mem::offset_of!(QueueBuffer, audio_data));
        if pointer == 8 {
            assert_eq!(std::mem::offset_of!(QueueBuffer, audio_data), 8);
            assert_eq!(std::mem::offset_of!(QueueBuffer, byte_size), 16);
            assert_eq!(user, 24);
        }
        let mut samples = [1u8, 2, 3, 4];
        let mut decoy = [9u8, 9, 9, 9];
        let live = QueueBuffer {
            capacity: 4,
            audio_data: samples.as_mut_ptr(),
            byte_size: 4,
            user_data: decoy.as_mut_ptr().cast(),
            packet_description_capacity: 0,
            packet_descriptions: std::ptr::null_mut(),
            packet_description_count: 0,
        };
        let got = unsafe { std::slice::from_raw_parts(live.audio_data, live.byte_size as usize) };
        assert_eq!(got, &[1, 2, 3, 4]);
        assert!(live.audio_data != decoy.as_mut_ptr());
        let abandoned = QueueBuffer {
            capacity: 4,
            audio_data: std::ptr::null_mut(),
            byte_size: 4,
            user_data: decoy.as_mut_ptr().cast(),
            packet_description_capacity: 0,
            packet_descriptions: std::ptr::null_mut(),
            packet_description_count: 0,
        };
        assert!(abandoned.audio_data.is_null());
        assert!(!abandoned.user_data.is_null());
        assert_ne!(std::mem::offset_of!(QueueBuffer, audio_data), std::mem::offset_of!(QueueBuffer, user_data));
    }

    fn scratch_fix6(name: &str) -> PathBuf {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("loops-lanes").join("G1-fix6-desktop-proof").join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg_attr(test, test)]
    fn replaced_file_is_not_deleted() {
        let root = scratch_fix6("replaced-file");
        let dir = root.join("folder");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        let mut original = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        original.write_all(b"ours").unwrap();
        std::fs::rename(&path, dir.join("moved.txt")).unwrap();
        std::fs::write(&path, b"theirs").unwrap();
        assert!(!delete_same_file(&path, &original), "a replaced file was deleted");
        assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
        let replacement = std::fs::File::open(&path).unwrap();
        assert!(delete_same_file(&path, &replacement), "the confirmed file was left in place");
        drop(replacement);
        assert!(!path.exists());
        assert_eq!(std::fs::read(dir.join("moved.txt")).unwrap(), b"ours");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn replaced_directory_entry_is_left_in_place() {
        let root = scratch_fix6("replaced-entry");
        let dir_path = root.join("folder");
        std::fs::create_dir_all(&dir_path).unwrap();
        let path = dir_path.join("note.txt");
        let mut original = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        original.write_all(b"ours").unwrap();
        let dir = std::fs::File::open(&dir_path).unwrap();
        let opened = host_fs::open_at(&dir, b"note.txt", 0x20000).unwrap();
        let id = file_index(&opened).unwrap();
        assert_eq!(file_index(&original), Some(id));
        drop(opened);
        std::fs::rename(&path, dir_path.join("moved.txt")).unwrap();
        std::fs::write(&path, b"theirs").unwrap();
        let name = std::ffi::CString::new("note.txt").unwrap();
        assert!(!unlink_opened_name(&dir, &name, id), "unlink removed the replacement");
        assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
    }

    #[cfg_attr(test, test)]
    fn control_request_on_a_stream_is_acknowledged_and_the_stream_stays_open() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let accept = |listener: &std::net::TcpListener| -> std::io::Result<std::net::TcpStream> {
                listener.set_nonblocking(true)?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false)?;
                            return Ok(stream);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return Err(error);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(error) => return Err(error),
                    }
                }
            };
            let mut stream = accept(&listener).expect("stream connection");
            let opening = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nMcp-Session-Id: session\r\n\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n";
            stream.write_all(opening).expect("stream header");
            let mut post = accept(&listener).expect("control connection");
            post.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            let mut wanted = usize::MAX;
            while request.len() < wanted && request.len() < 64 * 1024 {
                match post.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        request.extend_from_slice(&buf[..n]);
                        if wanted == usize::MAX {
                            if let Some(at) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&request[..at]);
                                let length = head.lines().find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                }).unwrap_or(0);
                                wanted = at + 4 + length;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let accepted = b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = post.write_all(accepted);
            let _ = post.shutdown(std::net::Shutdown::Both);
            let _ = stream.write_all(b"data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\",\"params\":{}}\n\n");
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let _ = tx.send(request);
        });
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(200))).ok();
        stream.set_write_timeout(Some(std::time::Duration::from_millis(200))).ok();
        let effect = obj(vec![
            ("stream_protocol", s("mcp_server")),
            ("mcp_control", obj(vec![
                ("endpoint", s(&format!("http://127.0.0.1:{port}/mcp"))),
                ("key", s("")),
                ("version", s("2025-06-18")),
                ("session", s("session")),
                ("owner", s("server-phase")),
                ("request", s("GET")),
                ("request_binding", Num("2".into())),
                ("resumable", Bool(false)),
            ])),
        ]);
        let bytes = read_stream(&mut stream, &effect, &mut |_, _| {}, std::time::Instant::now() + std::time::Duration::from_secs(8)).expect("control request closed the stream");
        let text = String::from_utf8(bytes).expect("stream result");
        assert!(text.contains("\"requests\":1"), "{text}");
        assert!(text.contains("\"notifications\":1"), "{text}");
        assert!(text.contains("\"messages\":2"), "{text}");
        assert!(!text.contains("not available"), "{text}");
        let posted = rx.recv_timeout(std::time::Duration::from_secs(8)).expect("control request was not sent");
        let posted = String::from_utf8_lossy(&posted);
        assert!(posted.contains("\"result\":{}"), "{posted}");
        server.join().expect("control peer");
    }

    #[cfg_attr(test, test)]
    fn non_windows_file_choice_is_not_unsupported() {
        let root = scratch_fix6("choice");
        let file = root.join("demo.txt");
        std::fs::write(&file, b"demo").unwrap();
        let chosen = interpret_file_line(&path_text(&file), false).unwrap();
        assert_eq!(chosen.as_deref(), Some(path_text(&file).as_str()));
        assert!(interpret_file_line("   ", false).unwrap().is_none());
        let missing = interpret_file_line(&path_text(&root.join("missing.txt")), false).unwrap_err();
        let lower = missing.to_ascii_lowercase();
        assert!(!lower.contains("not available"), "{missing}");
        assert!(!lower.contains("unsupported"), "{missing}");
        let save = interpret_file_line(&path_text(&root.join("notes.txt")), true).unwrap();
        assert_eq!(save.as_deref(), Some(path_text(&root.join("notes.txt")).as_str()));
        #[cfg(not(windows))]
        {
            let picked = choose_or_prompt("Attach image or file", false, "", false, Some(&path_text(&file))).unwrap();
            assert_eq!(picked.as_deref(), Some(path_text(&file).as_str()));
            let closed = choose_or_prompt("Save export", true, "notes.txt", false, Some("")).unwrap();
            assert!(closed.is_none());
            let saved = choose_or_prompt("Save export", true, "notes.txt", false, Some(&path_text(&root.join("notes.txt")))).unwrap();
            assert_eq!(saved.as_deref(), Some(path_text(&root.join("notes.txt")).as_str()));
            let absent = choose_or_prompt("Attach image or file", false, "", false, Some(&path_text(&root.join("missing.txt")))).unwrap_err();
            let lower = absent.to_ascii_lowercase();
            assert!(!lower.contains("not available"), "{absent}");
            assert!(!lower.contains("unsupported"), "{absent}");
        }
    }

    #[cfg(target_os = "linux")]
    #[cfg_attr(test, test)]
    fn portal_response_returns_the_decoded_path() {
        let mut body = Vec::new();
        bus_put_u32(&mut body, 0);
        bus_put_array(&mut body, 8, |buf| {
            bus_pad(buf, 8);
            bus_put_string(buf, "uris");
            bus_put_signature(buf, "as");
            bus_put_array(buf, 4, |arr| {
                bus_put_string(arr, "file:///tmp/a%20b.txt");
            });
        });
        let (code, uris) = portal_uris(&body).unwrap();
        assert_eq!(code, 0);
        assert_eq!(uris, vec!["file:///tmp/a%20b.txt".to_string()]);
        assert_eq!(file_uri_path(&uris[0]).unwrap(), "/tmp/a b.txt");
        assert_eq!(file_uri_path("file://localhost/tmp/a").unwrap(), "/tmp/a");
    }

    #[cfg_attr(test, test)]
    fn typed_multibyte_path_round_trips() {
        let typed = "/home/用户/файл-α/notes.txt";
        let mut input = std::io::Cursor::new(format!("{typed}\r\n").into_bytes());
        let line = read_typed_line(&mut input).expect("typed line");
        assert_eq!(line, typed);
        assert_eq!(line.as_bytes(), typed.as_bytes());
        let root = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix8-desktop-proof")
            .join("tmp")
            .join("typed");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let fresh = path_text(&root.join("用户-файл-α.txt"));
        let mut typed_file = std::io::Cursor::new(format!("{fresh}\n").into_bytes());
        let saved_line = read_typed_line(&mut typed_file).expect("typed file");
        assert_eq!(saved_line, fresh);
        let saved = interpret_file_line(&saved_line, true).expect("save path");
        assert_eq!(saved.as_deref(), Some(fresh.as_str()));
        assert!(!root.join("用户-файл-α.txt").exists());
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_save_target_is_refused_without_blocking() {
        let root = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix8-desktop-proof")
            .join("tmp")
            .join("fifo");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pipe = root.join("export.pipe");
        make_fifo(&pipe).expect("fifo");
        let shown = path_text(&pipe);
        let started = std::time::Instant::now();
        let error = interpret_file_line(&shown, true).expect_err("fifo was accepted as a save path");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "save check blocked on a fifo: {error}");
        assert!(error.contains(&shown), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let regular = root.join("notes.txt");
        std::fs::write(&regular, b"kept").unwrap();
        let saved = interpret_file_line(&path_text(&regular), true).unwrap();
        assert_eq!(saved.as_deref(), Some(path_text(&regular).as_str()));
        assert_eq!(std::fs::read(&regular).unwrap(), b"kept");
        let fresh = root.join("new-notes.txt");
        let created = interpret_file_line(&path_text(&fresh), true).unwrap();
        assert_eq!(created.as_deref(), Some(path_text(&fresh).as_str()));
        assert!(!fresh.exists());
        let folder = root.join("folder");
        std::fs::create_dir(&folder).unwrap();
        let directory = interpret_file_line(&path_text(&folder), true).unwrap_err();
        assert_eq!(directory, "Choose a file path");
    }

    fn import_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix10-desktop-proof")
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg_attr(test, test)]
    fn directory_import_is_refused_before_it_is_opened() {
        let root = import_scratch("dir-import");
        let started = std::time::Instant::now();
        let error = read_text(&path_text(&root), 1024 * 1024).expect_err("importing a directory succeeded");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "import blocked: {error}");
        assert!(error.contains("regular file"), "{error}");
        let backup = root.join("conversation.txt");
        std::fs::write(&backup, "restored conversation").unwrap();
        let text = read_text(&path_text(&backup), 1024 * 1024).expect("regular backup");
        assert_eq!(text, "restored conversation");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_import_returns_an_error_promptly() {
        let root = import_scratch("fifo-import");
        let pipe = root.join("conversation.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_text(&path_text(&pipe), 1024 * 1024).expect_err("importing a fifo succeeded");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "import blocked on a fifo: {error}");
        assert!(error.contains("regular file"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let backup = root.join("conversation.txt");
        std::fs::write(&backup, "restored conversation").unwrap();
        let text = read_text(&path_text(&backup), 1024 * 1024).expect("regular backup");
        assert_eq!(text, "restored conversation");
    }

    #[cfg(unix)]
    fn fix14_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix14-desktop-proof")
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_phone_write_and_phone_update_return_an_error_promptly() {
        let root = fix14_scratch("phone-write-fifo");
        let pipe = root.join("notes.pipe");
        make_fifo(&pipe).expect("fifo");
        let root_text = path_text(&root);
        let pipe_text = path_text(&pipe);
        let args = obj(vec![("path", s(&pipe_text)), ("content", s("nope"))]);
        let started = std::time::Instant::now();
        let write_error = prove(&root_text, "phone_write", args.clone()).expect_err("phone_write accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_write blocked on a fifo: {write_error}"
        );
        let started = std::time::Instant::now();
        let update_error = prove(&root_text, "phone_update", args).expect_err("phone_update accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_update blocked on a fifo: {update_error}"
        );
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let note = root.join("notes.txt");
        let wrote = prove(
            &root_text,
            "phone_write",
            obj(vec![("path", s(&path_text(&note))), ("content", s("kept"))]),
        )
        .expect("regular phone_write");
        assert!(wrote.contains("kept") || wrote.contains("bytes"), "{wrote}");
        let updated = prove(
            &root_text,
            "phone_update",
            obj(vec![("path", s(&path_text(&note))), ("content", s("revised"))]),
        )
        .expect("regular phone_update");
        assert!(updated.contains("revised") || updated.contains("bytes"), "{updated}");
        let read = prove(&root_text, "phone_read", obj(vec![("path", s(&path_text(&note)))])).expect("regular read");
        assert_eq!(read, "revised");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_phone_revision_returns_an_error_promptly() {
        let root = fix14_scratch("revision-fifo");
        let pipe = root.join("notes.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = phone_revision(&obj(vec![
            ("tree", s(&path_text(&root))),
            ("path", s("notes.pipe")),
            ("phase", s("read")),
        ]))
        .expect_err("phone_revision accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_revision blocked on a fifo: {error}"
        );
        assert!(error.contains("regular file"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let note = root.join("notes.txt");
        std::fs::write(&note, b"kept").unwrap();
        let read = phone_revision(&obj(vec![
            ("tree", s(&path_text(&root))),
            ("path", s("notes.txt")),
            ("phase", s("read")),
        ]))
        .expect("regular phone_revision");
        let bytes = network::unbase64(&text(&read, "data")).expect("revision data");
        assert_eq!(bytes, b"kept");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_read_project_file_returns_an_error_promptly() {
        let root = fix14_scratch("project-fifo");
        let pipe = root.join("project.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_project_file(&path_text(&root), "project.pipe").expect_err("read_project_file accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "read_project_file blocked on a fifo: {error}"
        );
        assert!(error.contains("project file") || error.contains("regular"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        std::fs::write(root.join("project.txt"), b"kept").unwrap();
        let bytes = read_project_file(&path_text(&root), "project.txt")
            .expect("regular project file")
            .expect("project bytes");
        assert_eq!(bytes, b"kept");
    }

    #[cfg(unix)]
    fn lane_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix12-desktop-proof")
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_phone_stat_and_phone_read_return_an_error_promptly() {
        let root = lane_scratch("phone-fifo");
        let pipe = root.join("notes.pipe");
        make_fifo(&pipe).expect("fifo");
        let root_text = path_text(&root);
        let pipe_text = path_text(&pipe);
        let started = std::time::Instant::now();
        let stat_error = prove(&root_text, "phone_stat", obj(vec![("path", s(&pipe_text))]))
            .expect_err("phone_stat accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_stat blocked on a fifo: {stat_error}"
        );
        assert!(stat_error.contains("regular file"), "{stat_error}");
        let started = std::time::Instant::now();
        let read_error = prove(&root_text, "phone_read", obj(vec![("path", s(&pipe_text))]))
            .expect_err("phone_read accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_read blocked on a fifo: {read_error}"
        );
        assert!(read_error.contains("regular file"), "{read_error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let folder = parse(&prove(&root_text, "phone_stat", obj(vec![("path", s(""))])).expect("folder stat"))
            .expect("folder stat json");
        assert_eq!(text(&folder, "kind"), "directory");
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let nested_stat = parse(
            &prove(&root_text, "phone_stat", obj(vec![("path", s(&path_text(&nested)))])).expect("directory stat"),
        )
        .expect("directory stat json");
        assert_eq!(text(&nested_stat, "kind"), "directory");
        let note = root.join("notes.txt");
        std::fs::write(&note, b"kept").unwrap();
        let read = prove(&root_text, "phone_read", obj(vec![("path", s(&path_text(&note)))])).expect("regular read");
        assert_eq!(read, "kept");
        let stat = parse(
            &prove(&root_text, "phone_stat", obj(vec![("path", s(&path_text(&note)))])).expect("file stat"),
        )
        .expect("file stat json");
        assert_eq!(text(&stat, "kind"), "file");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_zip_import_returns_an_error_promptly() {
        let root = lane_scratch("zip-fifo");
        let pipe = root.join("project.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_limited(&path_text(&pipe), PROJECT_FILE_LIMIT).expect_err("importing a fifo succeeded");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "zip import blocked on a fifo: {error}"
        );
        assert!(error.contains("Select a file, not a folder"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let archive = root.join("project.bin");
        std::fs::write(&archive, b"PK\x03\x04kept").unwrap();
        let bytes = read_limited(&path_text(&archive), PROJECT_FILE_LIMIT).expect("regular archive");
        assert_eq!(bytes, b"PK\x03\x04kept");
    }

    #[cfg(unix)]
    fn make_fifo(path: &Path) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        type FifoMode = u16;
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        type FifoMode = u32;
        unsafe extern "C" {
            fn mkfifo(path: *const std::ffi::c_char, mode: FifoMode) -> i32;
        }
        if unsafe { mkfifo(c_path.as_ptr(), 0o600) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    fn fix18_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("loops-lanes").join("G1-fix18-desktop-proof").join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct RestoreEnv {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl RestoreEnv {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn selected_workspace(name: &str) -> (PathBuf, Json) {
        let root = fix18_scratch(name);
        let root_text = path_text(&root);
        let opening = core_event(&Null, &root_text, "resume", &Null).expect("resume");
        let mut state = opening.get("state").expect("resume state").clone();
        advance(&mut state, &root_text, "workspace_select:default", Null).expect("workspace");
        assert!(!text(&state, "approval_root").is_empty(), "workspace was not selected");
        (root, state)
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn key_fifo_is_refused_without_blocking() {
        let root = fix18_scratch("key-fifo");
        let pipe = root.join("key");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_key(&pipe).expect_err("fifo key was accepted");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "key open blocked: {error}");
        assert!(error.contains("regular file"), "{error}");
        let created = root.join("fresh-key");
        let key = read_key(&created).expect("fresh key");
        assert_eq!(std::fs::read(&created).unwrap(), key);
        assert_eq!(read_key(&created).unwrap(), key);
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn state_fifo_load_and_save_are_refused_without_blocking() {
        let root = fix18_scratch("state-fifo");
        let kept = root.join("kept.bin");
        std::fs::write(&kept, b"previous-valid-state").unwrap();
        let pipe = root.join("state.bin");
        make_fifo(&pipe).expect("fifo");
        let vault = Vault { path: pipe, key: [3u8; 32] };
        let started = std::time::Instant::now();
        let load_error = vault.load().expect_err("fifo load was accepted");
        let save_error = vault.save(&obj(vec![("kept", s("next"))])).expect_err("fifo save was accepted");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "state open blocked: {load_error} {save_error}");
        assert!(load_error.contains("regular file"), "{load_error}");
        assert!(save_error.contains("regular file"), "{save_error}");
        assert_eq!(std::fs::read(&kept).unwrap(), b"previous-valid-state");
    }

    #[cfg_attr(test, test)]
    fn vault_save_replaces_the_previous_file_after_the_new_bytes_are_written() {
        use crate::host_fs::MetadataExt;
        let root = fix18_scratch("vault");
        let path = root.join("state.bin");
        std::fs::write(&path, b"previous-valid-state").unwrap();
        let before_id = std::fs::metadata(&path).unwrap().ino();
        let vault = Vault { path: path.clone(), key: [9u8; 32] };
        vault.save(&obj(vec![("kept", s("next"))])).expect("save");
        let after = std::fs::metadata(&path).unwrap();
        assert_ne!(before_id, after.ino(), "save truncated the existing vault in place");
        assert_ne!(std::fs::read(&path).unwrap(), b"previous-valid-state");
        assert!(after.len() > 16);
        let loaded = vault.load().expect("load").expect("saved state missing");
        assert_eq!(text(&loaded, "kept"), "next");
        vault.save(&obj(vec![("kept", s("later"))])).expect("second save");
        let loaded = vault.load().expect("reload").expect("saved state missing");
        assert_eq!(text(&loaded, "kept"), "later");
        let missing = Vault { path: root.join("absent.bin"), key: [9u8; 32] };
        assert!(missing.load().expect("missing vault").is_none());
        let created = root.join("key");
        let key = read_key(&created).expect("fresh key");
        assert_eq!(std::fs::read(&created).unwrap(), key);
    }

    #[cfg(windows)]
    #[cfg_attr(test, test)]
    fn directory_lock_covers_a_directory_and_a_file_still_locks() {
        use crate::host_fs::OpenOptionsExt;
        let root = fix18_scratch("dirlock");
        let first = std::fs::OpenOptions::new().read(true).custom_flags(131072 | 65536).open(&root).expect("directory");
        first.try_lock().expect("directory lock");
        let second = std::fs::OpenOptions::new().read(true).custom_flags(131072 | 65536).open(&root).expect("second directory");
        let blocked = second.try_lock().expect_err("second directory lock was granted");
        assert!(matches!(blocked, std::fs::TryLockError::WouldBlock), "{blocked:?}");
        drop(first);
        second.try_lock().expect("directory lock after release");
        let path = root.join("plain.bin");
        std::fs::write(&path, b"abc").unwrap();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        file.try_lock().expect("file lock");
        let other = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let blocked = other.try_lock().expect_err("second file lock was granted");
        assert!(matches!(blocked, std::fs::TryLockError::WouldBlock), "{blocked:?}");
    }

    #[cfg_attr(test, test)]
    fn media_pick_stores_the_selected_file() {
        let _hold = env_guard();
        let (root, mut state) = selected_workspace("mediawork");
        let root_text = path_text(&root);
        {
            let _empty = RestoreEnv::set("RLLM_MEDIA_FILE", "");
            let ui = advance(&mut state, &root_text, "media_pick", Null).expect("cancel pick");
            assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
            assert!(text(&state, "notice").contains("cancelled"), "{}", text(&state, "notice"));
        }
        let source = root.join("clip.bin");
        std::fs::write(&source, b"hello-media").unwrap();
        let _file = RestoreEnv::set("RLLM_MEDIA_FILE", &source);
        let ui = advance(&mut state, &root_text, "media_pick", Null).expect("pick");
        assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
        let record = get(&state, "media_input").clone();
        assert_ne!(record, Null, "selected file was not stored; notice={} dialog={}", text(&state, "notice"), text(&state, "dialog"));
        let leaf = text(&record, "path").rsplit(['/', '\\']).next().unwrap().to_string();
        let stored = Path::new(&text(&state, "approval_root")).join("service-files").join(leaf);
        assert_eq!(std::fs::read(&stored).expect("stored media"), b"hello-media");
        assert!(text(&state, "notice").contains("Input saved locally"), "{}", text(&state, "notice"));
    }

    #[cfg_attr(test, test)]
    fn media_record_start_and_stop_store_audio() {
        let _hold = env_guard();
        let (root, mut state) = selected_workspace("recordwork");
        let root_text = path_text(&root);
        let idle = advance(&mut state, &root_text, "media_record_stop", Null).expect("idle stop");
        assert!(!idle.string().contains("not available on this host"), "{}", idle.string());
        let mut pcm = Vec::new();
        for _ in 0..640 {
            pcm.extend_from_slice(&[0x11, 0x22]);
        }
        let source = root.join("utterance.pcm");
        std::fs::write(&source, &pcm).unwrap();
        let _source = RestoreEnv::set("RLLM_VOICE_SOURCE", &source);
        let started = advance(&mut state, &root_text, "media_record", Null).expect("record");
        assert!(!started.string().contains("not available on this host"), "{}", started.string());
        assert_eq!(text(&state, "dialog"), "media_capture", "{}", text(&state, "notice"));
        let stopped = advance(&mut state, &root_text, "media_record_stop", Null).expect("stop");
        assert!(!stopped.string().contains("not available on this host"), "{}", stopped.string());
        let record = get(&state, "media_input").clone();
        assert_eq!(text(&record, "mime"), "audio/wav", "{}", record.string());
        let leaf = text(&record, "path").rsplit(['/', '\\']).next().unwrap().to_string();
        let stored = Path::new(&text(&state, "approval_root")).join("service-files").join(leaf);
        let bytes = std::fs::read(&stored).unwrap_or_else(|error| panic!("stored audio: {error}"));
        assert!(bytes.starts_with(b"RIFF"));
        assert!(bytes.windows(pcm.len()).any(|window| window == pcm.as_slice()), "captured samples were not stored");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn media_pick_fifo_is_refused_without_blocking() {
        let _hold = env_guard();
        let (root, mut state) = selected_workspace("pickfifo");
        let pipe = root.join("clip.pipe");
        make_fifo(&pipe).expect("fifo");
        let _file = RestoreEnv::set("RLLM_MEDIA_FILE", &pipe);
        let started = std::time::Instant::now();
        let ui = advance(&mut state, &path_text(&root), "media_pick", Null).expect("pick");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "media pick blocked");
        assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
        assert!(text(&state, "notice").contains("unavailable"), "{}", text(&state, "notice"));
    }

    #[cfg_attr(test, test)]
    fn authorization_address_rejects_a_non_http_target() {
        let error = open_authorization("file:///local/secret").expect_err("non-http address was accepted");
        assert!(error.contains("not usable"), "{error}");
        let error = open_authorization("").expect_err("empty address was accepted");
        assert!(error.contains("not usable"), "{error}");
    }

    #[cfg_attr(test, test)]
    fn oauth_browser_keeps_the_sign_in_open() {
        let _hold = env_guard();
        let root = fix18_scratch("oauthwork");
        let root_text = path_text(&root);
        let opening = core_event(&Null, &root_text, "resume", &Null).expect("resume");
        let mut state = opening.get("state").expect("resume state").clone();
        state = set_field(state, "profiles", Arr(vec![obj(vec![
            ("name", s("Loopback")),
            ("model", s("local")),
            ("endpoint", s("http://127.0.0.1:9/v1/chat/completions")),
            ("plain", Bool(true)),
            ("protocol", s("chat")),
        ])]));
        state = set_field(state, "model", s("Loopback"));
        state = set_field(state, "selected", Num("0".into()));
        advance(&mut state, &root_text, "oauth_settings", Null).expect("settings");
        let saved = obj(vec![
            ("authorization_endpoint", s("http://127.0.0.1:9/authorize")),
            ("token_endpoint", s("http://127.0.0.1:9/token")),
            ("client_id", s("desktop-client")),
            ("scope", s("open")),
            ("plain", Bool(true)),
        ]);
        advance(&mut state, &root_text, "oauth_save", saved.clone()).expect("save authorization");
        let capture = root.join("authorization.txt");
        let _capture = RestoreEnv::set("RLLM_AUTHORIZATION_CAPTURE", &capture);
        let ui = advance(&mut state, &root_text, "oauth_sign_in", saved).expect("sign in");
        assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
        assert_eq!(text(&state, "dialog"), "oauth_wait", "{}", text(&state, "notice"));
        let opened = std::fs::read_to_string(&capture).expect("authorization address was not opened");
        assert!(opened.starts_with("http://127.0.0.1:9/authorize?"), "{opened}");
        assert!(opened.contains("client_id=desktop-client"), "{opened}");
        assert!(opened.contains("response_type=code"), "{opened}");
    }

    #[cfg(g1_regression)]
    pub fn fix18() {
        #[cfg(unix)]
        key_fifo_is_refused_without_blocking();
        #[cfg(unix)]
        state_fifo_load_and_save_are_refused_without_blocking();
        vault_save_replaces_the_previous_file_after_the_new_bytes_are_written();
        #[cfg(windows)]
        directory_lock_covers_a_directory_and_a_file_still_locks();
        media_pick_stores_the_selected_file();
        media_record_start_and_stop_store_audio();
        #[cfg(unix)]
        media_pick_fifo_is_refused_without_blocking();
        oauth_browser_keeps_the_sign_in_open();
        authorization_address_rejects_a_non_http_target();
        println!("g1-fix18 passed");
    }

    #[cfg(g1_regression)]
    pub fn run() {
        #[cfg(windows)]
        failed_microphone_open_closes_the_event_handle();
        revision_rejects_a_linked_parent();
        revision_reads_binary_past_one_mebibyte();
        limited_read_stops_before_a_declared_huge_file();
        failed_project_write_preserves_the_handle_and_not_a_replacement();
        stream_deadline_does_not_restart_when_bytes_arrive();
        empty_capture_is_not_a_fatal_read();
        session_hello_matches_the_published_core_method();
        session_negotiation_returns_samples_only_after_a_capture_buffer();
        nested_format_rate_is_recognized_and_a_hostile_size_is_not();
        stdin_line_stops_at_the_parse_limit();
        swapped_directory_entry_cannot_escape_the_selected_folder();
        in_folder_link_still_reads_the_target_file();
        #[cfg(windows)]
        wave_header_matches_the_published_record();
        native_tools_refuse_paths_outside_the_selected_folder();
        stalled_upload_cannot_outlive_its_deadline();
        stalled_write_timeout_cannot_restart_the_deadline();
        audio_queue_samples_follow_capacity_not_the_user_pointer();
        replaced_file_is_not_deleted();
        #[cfg(unix)]
        replaced_directory_entry_is_left_in_place();
        control_request_on_a_stream_is_acknowledged_and_the_stream_stays_open();
        non_windows_file_choice_is_not_unsupported();
        typed_multibyte_path_round_trips();
        directory_import_is_refused_before_it_is_opened();
        #[cfg(unix)]
        fifo_save_target_is_refused_without_blocking();
        #[cfg(unix)]
        fifo_import_returns_an_error_promptly();
        #[cfg(unix)]
        fifo_phone_stat_and_phone_read_return_an_error_promptly();
        #[cfg(unix)]
        fifo_zip_import_returns_an_error_promptly();
        #[cfg(unix)]
        fifo_phone_write_and_phone_update_return_an_error_promptly();
        #[cfg(unix)]
        fifo_phone_revision_returns_an_error_promptly();
        #[cfg(unix)]
        fifo_read_project_file_returns_an_error_promptly();
        #[cfg(target_os = "linux")]
        portal_response_returns_the_decoded_path();
        #[cfg(unix)]
        key_fifo_is_refused_without_blocking();
        #[cfg(unix)]
        state_fifo_load_and_save_are_refused_without_blocking();
        vault_save_replaces_the_previous_file_after_the_new_bytes_are_written();
        #[cfg(windows)]
        directory_lock_covers_a_directory_and_a_file_still_locks();
        media_pick_stores_the_selected_file();
        media_record_start_and_stop_store_audio();
        #[cfg(unix)]
        media_pick_fifo_is_refused_without_blocking();
        oauth_browser_keeps_the_sign_in_open();
        authorization_address_rejects_a_non_http_target();
        println!("g1-regression passed");
    }
}


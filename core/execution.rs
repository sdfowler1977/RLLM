use super::*;
use std::collections::BTreeMap;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant},
};
#[cfg(unix)]
use std::os::unix::{io::AsRawFd, process::CommandExt};
static CANCEL: AtomicBool = AtomicBool::new(false);
// A new worker may reset the legacy flag; it cannot revive an older owner.
static CANCEL_GENERATION: AtomicU64 = AtomicU64::new(0);
const SCOPED_BIT:u64=1<<63;
static SCOPED_SERIAL:AtomicU64=AtomicU64::new(SCOPED_BIT);
static SCOPES:OnceLock<Mutex<BTreeMap<u64,Weak<AtomicBool>>>>=OnceLock::new();
fn scopes()->&'static Mutex<BTreeMap<u64,Weak<AtomicBool>>>{SCOPES.get_or_init(||Mutex::new(BTreeMap::new()))}
pub(crate) fn scoped_token(token:u64)->bool{token!=u64::MAX&&token&SCOPED_BIT!=0}
/// An independent read operation never cancels or resets another operation.
pub(crate) struct CancellationScope{token:u64,_flag:Arc<AtomicBool>}
impl CancellationScope{
 pub(crate) fn new(flag:Arc<AtomicBool>)->Result<Self>{
  let mut held=scopes().lock().map_err(|_|"Operation cancellation unavailable")?;
  held.retain(|_,v|v.strong_count()>0);
  if held.len()>=64{return Err("Concurrent operation capacity reached".into());}
  let token=SCOPED_SERIAL.try_update(Ordering::SeqCst,Ordering::SeqCst,|v|v.checked_add(1).filter(|n|*n<u64::MAX)).map_err(|_|"Operation identity capacity reached")?;
  held.insert(token,Arc::downgrade(&flag));Ok(Self{token,_flag:flag})
 }
 pub(crate) fn token(&self)->u64{self.token}
}
impl Drop for CancellationScope{fn drop(&mut self){if let Ok(mut held)=scopes().lock(){held.remove(&self.token);}}}
pub fn cancellation_token() -> u64 { CANCEL_GENERATION.load(Ordering::SeqCst) }
pub fn token_cancelled(token:u64) -> bool {
    if scoped_token(token){return scopes().lock().ok().and_then(|held|held.get(&token).and_then(Weak::upgrade)).map(|flag|flag.load(Ordering::SeqCst)).unwrap_or(true);}
    token==u64::MAX || CANCEL_GENERATION.load(Ordering::SeqCst)!=token || cancelled()
}
#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}
#[cfg(unix)]
struct Group(i32);
#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        unsafe {
            kill(-self.0, 9);
        }
    }
}
pub fn reset() {
    CANCEL.store(false, Ordering::SeqCst);
}
pub fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}
pub fn cancel() {
    let _=CANCEL_GENERATION.try_update(Ordering::SeqCst,Ordering::SeqCst,|n|Some(if n<SCOPED_BIT-1{n+1}else{u64::MAX}));
    CANCEL.store(true, Ordering::SeqCst);
}
fn drain(input: &mut impl Read, out: &mut Vec<u8>) -> Result<()> {
    let mut buffer = [0; 4096];
    // Yield back to the deadline/cancellation checks even for continuously ready pipes.
    for _ in 0..32 {
        match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let retain = n.min(65536usize.saturating_sub(out.len()));
                out.extend_from_slice(&buffer[..retain]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn run(v: &Json) -> Result<Json> {
    CANCEL.store(false, Ordering::SeqCst);
    let source = field(v, "source")?;
    if source.len() > 128 * 1024 {
        return Err("Shell program exceeds 128 KiB".into());
    }
    let check = v.get("check")? == &Bool(true);
    let mut command = Command::new("/system/bin/sh");
    command
        .arg(if check { "-n" } else { "-s" })
        .process_group(0)
        .env_clear()
        .env("PATH", "/system/bin")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("System shell unavailable: {e}"))?;
    let _group = Group(child.id() as i32);
    let mut stdin = child.stdin.take().unwrap();
    let source = source.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(source.as_bytes()));
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
        let flags = unsafe { fcntl(fd, 3) };
        if flags < 0 || unsafe { fcntl(fd, 4, flags | 2048) } < 0 {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Unable to configure bounded output".into());
        }
    }
    let mut out = Vec::new();
    let mut err = Vec::new();
    let start = Instant::now();
    let status = loop {
        drain(&mut stdout, &mut out)?;
        drain(&mut stderr, &mut err)?;
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if CANCEL.load(Ordering::SeqCst) || start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Program cancelled or exceeded its 30-second limit".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(_group);
    drain(&mut stdout, &mut out)?;
    drain(&mut stderr, &mut err)?;
    drop(writer);
    Ok(s(&format!(
        "{}: exit {}\n{}\n{}",
        if check { "Syntax check" } else { "Execution" },
        status
            .code()
            .map(|c| c.to_string())
            .unwrap_or("signal".into()),
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    )))
}

pub(super) struct BoundedOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Run one child in its own process group and stop it when the time limit or cancellation hits.
/// The caller keeps any bytes it needs; this function does not remove files.
#[cfg(unix)]
pub(super) fn bounded_process(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<BoundedOutput> {
    if timeout.is_zero() || timeout > Duration::from_secs(900) {
        return Err("Child process limit must be from 1 millisecond through 900 seconds".into());
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .process_group(0)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start {program}: {e}"))?;
    let group = Group(child.id() as i32);
    let mut stdout = child.stdout.take().ok_or("Child output is unavailable")?;
    let mut stderr = child.stderr.take().ok_or("Child error output is unavailable")?;
    for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
        let flags = unsafe { fcntl(fd, 3) };
        if flags < 0 || unsafe { fcntl(fd, 4, flags | 2048) } < 0 {
            let _ = child.kill();
            let _ = child.wait();
            drop(group);
            return Err("Unable to configure bounded output".into());
        }
    }
    let mut out = Vec::new();
    let mut err = Vec::new();
    let start = Instant::now();
    let status = loop {
        drain(&mut stdout, &mut out)?;
        drain(&mut stderr, &mut err)?;
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if cancelled() || start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            drop(group);
            return Err("Child process exceeded its time limit or was cancelled".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(group);
    drain(&mut stdout, &mut out)?;
    drain(&mut stderr, &mut err)?;
    Ok(BoundedOutput {
        code: status.code(),
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    })
}

#[cfg(windows)]
fn capture_pipe(mut input: impl Read, slot: Arc<Mutex<(Vec<u8>, bool)>>) {
    let mut buffer = [0u8; 4096];
    loop {
        match input.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Ok(mut held) = slot.lock() {
                    let room = 65536usize.saturating_sub(held.0.len());
                    if n > room {
                        held.1 = true;
                    }
                    let retain = n.min(room);
                    held.0.extend_from_slice(&buffer[..retain]);
                }
            }
        }
    }
}

/// One Windows child in a job so a timeout stops that child. The job is
/// assigned immediately after the process starts.
#[cfg(windows)]
pub(super) fn spawn_bounded(program: &str, mut command: Command, timeout: Duration, cancelled: &dyn Fn() -> bool) -> Result<BoundedOutput> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct BasicLimit {
        per_process: i64,
        per_job: i64,
        limit_flags: u32,
        min_ws: usize,
        max_ws: usize,
        active_process: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }
    unsafe extern "system" {
        fn CreateJobObjectW(attrs: *const u8, name: *const u16) -> isize;
        fn SetInformationJobObject(job: isize, class: i32, info: *const u8, len: u32) -> i32;
        fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
        fn TerminateJobObject(job: isize, code: u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job == 0 {
        return Err(format!("Cannot start {program}: process job unavailable"));
    }
    let info = BasicLimit {
        per_process: 0,
        per_job: 0,
        limit_flags: 0x2000,
        min_ws: 0,
        max_ws: 0,
        active_process: 0,
        affinity: 0,
        priority_class: 0,
        scheduling_class: 0,
    };
    if unsafe { SetInformationJobObject(job, 2, &info as *const BasicLimit as *const u8, std::mem::size_of::<BasicLimit>() as u32) } == 0 {
        unsafe { CloseHandle(job); }
        return Err(format!("Cannot start {program}: process job unavailable"));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            unsafe { CloseHandle(job); }
            return Err(format!("Cannot start {program}: {error}"));
        }
    };
    if unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as isize) } == 0 {
        let _ = child.kill();
        let _ = child.wait();
        unsafe { CloseHandle(job); }
        return Err(format!("Cannot start {program}: process job unavailable"));
    }
    let stdout = child.stdout.take().ok_or("Child output is unavailable")?;
    let stderr = child.stderr.take().ok_or("Child error output is unavailable")?;
    let out = Arc::new(Mutex::new((Vec::new(), false)));
    let err = Arc::new(Mutex::new((Vec::new(), false)));
    let out_slot = Arc::clone(&out);
    let err_slot = Arc::clone(&err);
    let out_thread = std::thread::spawn(move || capture_pipe(stdout, out_slot));
    let err_thread = std::thread::spawn(move || capture_pipe(stderr, err_slot));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if cancelled() || start.elapsed() > timeout {
            unsafe { TerminateJobObject(job, 1); }
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_thread.join();
            let _ = err_thread.join();
            unsafe { CloseHandle(job); }
            return Err("Child process exceeded its time limit or was cancelled".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = out_thread.join();
    let _ = err_thread.join();
    unsafe { CloseHandle(job); }
    let (stdout, _) = out.lock().map(|held| held.clone()).unwrap_or_default();
    let (stderr, _) = err.lock().map(|held| held.clone()).unwrap_or_default();
    Ok(BoundedOutput {
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

#[cfg(windows)]
pub fn run(v: &Json) -> Result<Json> {
    let source = field(v, "source")?;
    if source.len() > 128 * 1024 {
        return Err("Shell program exceeds 128 KiB".into());
    }
    let check = v.get("check")? == &Bool(true);
    if check {
        return Err("The Windows command processor has no separate syntax-check mode".into());
    }
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let command_path = Path::new(&system).join("System32").join("cmd.exe");
    let directory = std::env::temp_dir().join("rllm-shell");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let script = directory.join(format!("script-{}-{}.cmd", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_nanos()).unwrap_or(0)));
    std::fs::write(&script, source.as_bytes()).map_err(|error| error.to_string())?;
    let mut command = Command::new(&command_path);
    command
        .arg("/d")
        .arg("/s")
        .arg("/c")
        .arg(&script)
        .env_clear()
        .env("SystemRoot", &system)
        .env("PATH", Path::new(&system).join("System32"))
        .current_dir(&directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = spawn_bounded("cmd.exe", command, Duration::from_secs(30), &cancelled)?;
    let _ = std::fs::remove_file(&script);
    Ok(s(&format!(
        "Execution: exit {}\n{}\n{}",
        output.code.map(|code| code.to_string()).unwrap_or_else(|| "signal".into()),
        output.stdout,
        output.stderr
    )))
}

#[cfg(windows)]
pub(super) fn bounded_process(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<BoundedOutput> {
    if timeout.is_zero() || timeout > Duration::from_secs(900) {
        return Err("Child process limit must be from 1 millisecond through 900 seconds".into());
    }
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    spawn_bounded(program, command, timeout, cancelled)
}

/// Replace this process with the successor image. Returns only when exec fails.
pub(super) fn exec_replace(program: &Path, args: &[String], cwd: &Path, env: &[(&str, &str)]) -> Result<()> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    #[cfg(unix)]
    {
        let error = command.exec();
        return Err(format!("Successor exec failed: {error}"));
    }
    #[cfg(windows)]
    {
        let mut child = command.spawn().map_err(|error| format!("Successor exec failed: {error}"))?;
        let status = child.wait().map_err(|error| format!("Successor exec failed: {error}"))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

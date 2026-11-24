//! Workspace shell execution. The selected directory is a starting location;
//! commands run with the operating-system permissions of the application.
use super::*;
use std::{
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub(super) const TOOL: &str = "run_shell_command";
pub(super) const DEFAULT_TIMEOUT: u64 = 1200;
pub(super) const MAX_TIMEOUT: u64 = 3600;
const OUTPUT_BYTES: usize = 16 * 1024;

struct Arguments<'a> {
    command: &'a str,
    directory: &'a str,
    timeout: Duration,
}

fn arguments(args: &Json) -> Result<Arguments<'_>> {
    let Obj(fields) = args else { return Err("Shell arguments must be an object".into()); };
    if fields.keys().any(|key| !["command", "working_directory", "timeout_seconds"].contains(&key.as_str())) {
        return Err("Unexpected shell argument".into());
    }
    let command = field(args, "command")?;
    if command.trim().is_empty() || command.contains('\0') || command.len() > 8192 {
        return Err("Enter a command line of up to 8192 bytes without NUL characters".into());
    }
    let directory = match fields.get("working_directory") {
        None => ".",
        Some(value) => value.text()?,
    };
    let path = Path::new(directory);
    if directory.len() > 1024 || directory.chars().any(char::is_control)
        || (!path.is_absolute() && path.components().any(|part| matches!(part, Component::Prefix(_) | Component::RootDir)))
    {
        return Err("Use a task-root-relative path or an absolute working directory inside the task root".into());
    }
    let seconds = match fields.get("timeout_seconds") {
        None => DEFAULT_TIMEOUT,
        Some(Num(value)) => value.parse().unwrap_or(0),
        _ => 0,
    };
    if !(1..=MAX_TIMEOUT).contains(&seconds) {
        return Err(format!("Shell timeout must be an integer from 1 through {MAX_TIMEOUT} seconds"));
    }
    Ok(Arguments { command, directory, timeout: Duration::from_secs(seconds) })
}

pub(super) fn validate(args: &Json) -> Result<()> {
    arguments(args).map(|_| ())
}

pub(super) fn directory(root: &Path, args: &Json) -> Result<PathBuf> {
    let root = root.canonicalize().map_err(|error| format!("Task root unavailable: {error}"))?;
    let target = root.join(arguments(args)?.directory).canonicalize()
        .map_err(|error| format!("Working directory unavailable: {error}"))?;
    if !target.starts_with(&root) || !target.is_dir() {
        return Err("Working directory must be an existing directory inside the task root".into());
    }
    Ok(target)
}

#[derive(Default)]
struct Tail {
    bytes: std::collections::VecDeque<u8>,
    truncated: bool,
}

impl Tail {
    fn push(&mut self, chunk: &[u8]) {
        let discard = (self.bytes.len() + chunk.len()).saturating_sub(OUTPUT_BYTES);
        self.truncated |= discard != 0;
        self.bytes.drain(..discard);
        self.bytes.extend(chunk);
    }

    fn text(&mut self) -> String {
        String::from_utf8_lossy(self.bytes.make_contiguous()).into_owned()
    }
}

pub(super) fn run(root: &Path, args: &Json, expected_dir: &str, cancelled: impl Fn() -> bool) -> Result<String> {
    let args_parsed = arguments(args)?;
    let working_dir = directory(root, args)?;
    if working_dir.as_os_str() != std::ffi::OsStr::new(expected_dir) {
        return Err("The working directory changed after review".into());
    }
    if cancelled() {
        return Err("Shell command cancelled before starting".into());
    }
    let mut command = platform::shell(args_parsed.command)?;
    command.current_dir(platform::working_directory(&working_dir)?)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let (mut child, tree) = platform::spawn(command)?;
    let outcome = (|| {
        let mut stdout = child.stdout.take().ok_or("Shell stdout unavailable")?;
        let mut stderr = child.stderr.take().ok_or("Shell stderr unavailable")?;
        platform::prepare(&stdout)?;
        platform::prepare(&stderr)?;
        let started = Instant::now();
        let mut out = Tail::default();
        let mut err = Tail::default();
        let (status, reason) = loop {
            if cancelled() {
                break (None, "cancelled");
            }
            if started.elapsed() >= args_parsed.timeout {
                break (None, "timed_out");
            }
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                break (Some(status), "exited");
            }
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            std::thread::sleep(Duration::from_millis(20));
        };
        tree.stop()?;
        let status = match status {
            Some(status) => status,
            None => child.wait().map_err(|error| error.to_string())?,
        };
        drain(&mut stdout, &mut out)?;
        drain(&mut stderr, &mut err)?;
        Ok(obj(vec![
            ("status", s(reason)),
            ("exit_code", status.code().map(|code| Num(code.to_string())).unwrap_or(Null)),
            ("stdout", s(&out.text())),
            ("stderr", s(&err.text())),
            ("stdout_truncated", Bool(out.truncated)),
            ("stderr_truncated", Bool(err.truncated)),
        ]).string())
    })();
    if outcome.is_err() {
        let _ = tree.stop();
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

fn drain(pipe: &mut (impl Read + platform::Pipe), tail: &mut Tail) -> Result<()> {
    let mut buffer = [0; 4096];
    for _ in 0..32 {
        let available = platform::available(pipe)?.min(buffer.len());
        if available == 0 { break; }
        match pipe.read(&mut buffer[..available]) {
            Ok(0) => break,
            Ok(count) => tail.push(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(format!("Reading shell output: {error}")),
        }
    }
    Ok(())
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::unix::{io::AsRawFd, process::CommandExt};
    pub(super) use std::os::unix::io::AsRawFd as Pipe;

    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
        fn fcntl(fd: i32, request: i32, ...) -> i32;
    }

    pub(super) fn shell(line: &str) -> Result<Command> {
        let mut command = Command::new(if cfg!(target_os = "android") { "/system/bin/sh" } else { "/bin/sh" });
        command.arg("-c").arg(line);
        Ok(command)
    }

    pub(super) fn working_directory(path: &Path) -> Result<&Path> { Ok(path) }

    pub(super) struct Tree(i32);

    impl Tree {
        pub(super) fn stop(&self) -> Result<()> {
            if unsafe { kill(-self.0, 9) } < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(3) {
                    return Err(format!("Stopping shell process group: {error}"));
                }
            }
            Ok(())
        }
    }

    pub(super) fn spawn(mut command: Command) -> Result<(Child, Tree)> {
        let child = command.process_group(0).spawn().map_err(|error| format!("Starting system shell: {error}"))?;
        let tree = Tree(child.id() as i32);
        Ok((child, tree))
    }

    pub(super) fn prepare(pipe: &impl AsRawFd) -> Result<()> {
        let fd = pipe.as_raw_fd();
        let flags = unsafe { fcntl(fd, 3) };
        let nonblocking = if cfg!(any(target_os = "linux", target_os = "android")) { 2048 } else { 4 };
        if flags < 0 || unsafe { fcntl(fd, 4, flags | nonblocking) } < 0 {
            return Err(format!("Setting shell pipe mode: {}", io::Error::last_os_error()));
        }
        Ok(())
    }

    pub(super) fn available(_: &impl AsRawFd) -> Result<usize> { Ok(4096) }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle, process::CommandExt};
    pub(super) use std::os::windows::io::AsRawHandle as Pipe;

    type Handle = isize;

    #[repr(C)]
    struct BasicLimits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_processes: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }

    #[repr(C)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: [u64; 6],
        memory: [usize; 4],
    }

    #[repr(C)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        id: u32,
        process: u32,
        priority: i32,
        priority_delta: i32,
        flags: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(path: *mut u16, size: u32) -> u32;
        fn CreateJobObjectW(attributes: *const u8, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *const u8, size: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn TerminateJobObject(job: Handle, code: u32) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> Handle;
        fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
        fn ResumeThread(thread: Handle) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
        fn PeekNamedPipe(pipe: Handle, buffer: *mut u8, size: u32, read: *mut u32, available: *mut u32, left: *mut u32) -> i32;
    }

    struct Owned(Handle);

    impl Drop for Owned {
        fn drop(&mut self) { unsafe { CloseHandle(self.0); } }
    }

    pub(super) struct Tree(Owned);

    impl Tree {
        pub(super) fn stop(&self) -> Result<()> {
            if unsafe { TerminateJobObject(self.0.0, 1) } == 0 {
                return Err(format!("Stopping shell process job: {}", io::Error::last_os_error()));
            }
            Ok(())
        }
    }

    pub(super) fn shell(line: &str) -> Result<Command> {
        let mut path = [0u16; 1024];
        let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
        if length == 0 || length >= path.len() {
            return Err("System shell directory unavailable".into());
        }
        let mut command = Command::new(PathBuf::from(std::ffi::OsString::from_wide(&path[..length])).join("cmd.exe"));
        // Pass the command exactly once through the shell's quote parser.
        command.args(["/d", "/s", "/c"]).raw_arg(format!("\"{line}\""));
        Ok(command)
    }

    pub(super) fn working_directory(path: &Path) -> Result<&Path> {
        let path = path.to_str().ok_or("Shell working directory must be representable as text")?;
        let path = path.strip_prefix(r"\\?\").unwrap_or(path);
        if !matches!(Path::new(path).components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_))) {
            return Err("The system shell requires a drive-letter working directory".into());
        }
        Ok(Path::new(path))
    }

    pub(super) fn spawn(mut command: Command) -> Result<(Child, Tree)> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job == 0 { return Err(format!("Creating shell process job: {}", io::Error::last_os_error())); }
        let job = Owned(job);
        let mut limits: ExtendedLimits = unsafe { std::mem::zeroed() };
        limits.basic.flags = 0x2000; // Kill on job close.
        if unsafe { SetInformationJobObject(job.0, 9, std::ptr::from_ref(&limits).cast(), std::mem::size_of_val(&limits) as u32) } == 0 {
            return Err(format!("Configuring shell process job: {}", io::Error::last_os_error()));
        }
        // Suspend the initial thread until the job owns the process. This
        // closes the race where the shell could launch a child before assignment.
        let mut child = command.creation_flags(0x0800_0004).spawn()
            .map_err(|error| format!("Starting system shell: {error}"))?;
        let started = if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as Handle) } == 0 {
            Err(format!("Assigning shell process job: {}", io::Error::last_os_error()))
        } else {
            resume(child.id())
        };
        if let Err(error) = started {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok((child, Tree(job)))
    }

    fn resume(process: u32) -> Result<()> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(4, 0) };
        if snapshot == -1 { return Err(format!("Finding shell thread: {}", io::Error::last_os_error())); }
        let snapshot = Owned(snapshot);
        let mut entry: ThreadEntry = unsafe { std::mem::zeroed() };
        entry.size = std::mem::size_of_val(&entry) as u32;
        let mut found = unsafe { Thread32First(snapshot.0, &mut entry) };
        while found != 0 {
            if entry.process == process {
                let thread = unsafe { OpenThread(2, 0, entry.id) };
                if thread != 0 {
                    let thread = Owned(thread);
                    if unsafe { ResumeThread(thread.0) } == 1 { return Ok(()); }
                }
                break;
            }
            entry.size = std::mem::size_of_val(&entry) as u32;
            found = unsafe { Thread32Next(snapshot.0, &mut entry) };
        }
        Err("Could not resume the shell's initial thread".into())
    }

    pub(super) fn prepare(_: &impl AsRawHandle) -> Result<()> { Ok(()) }

    pub(super) fn available(pipe: &impl AsRawHandle) -> Result<usize> {
        let mut bytes = 0;
        let ok = unsafe { PeekNamedPipe(pipe.as_raw_handle() as Handle, std::ptr::null_mut(), 0,
            std::ptr::null_mut(), &mut bytes, std::ptr::null_mut()) };
        if ok != 0 { return Ok(bytes as usize); }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::BrokenPipe { return Ok(0); }
        Err(format!("Reading shell pipe state: {error}"))
    }
}


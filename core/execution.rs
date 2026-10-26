use super::*;
use std::{
    io::{Read, Write},
    os::unix::{io::AsRawFd, process::CommandExt},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
static CANCEL: AtomicBool = AtomicBool::new(false);
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}
struct Group(i32);
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

#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn continuously_ready_output_cannot_starve_deadline() {
        struct Continuous {
            calls: usize,
        }
        impl Read for Continuous {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.calls += 1;
                buffer.fill(b'x');
                Ok(buffer.len())
            }
        }
        let mut input = Continuous { calls: 0 };
        let mut output = Vec::new();
        drain(&mut input, &mut output).unwrap();
        assert_eq!(input.calls, 32);
        assert_eq!(output.len(), 65536);
    }
}

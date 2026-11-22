//! Rebuild plan for the desktop product.
//! When the destination is free and a link is not already running, rustc writes
//! RLLM.exe. A busy destination is left unchanged. The running process is not
//! stopped.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};

const SOURCE_REL: &str = "core/desktop.rs";
const DEST_NAME: &str = "RLLM.exe";
const SOURCE_CAP: u64 = 16 * 1024 * 1024;
const TEST_COMMAND: &str = "rustc --edition 2021 --test --crate-name rebuild_reload -o build\\rebuild_reload_test.exe core\\rebuild_reload.rs";

/// Read `source_rel` inside `root`, bind a revision to those bytes and the edit
/// marker, and record the desktop link command, the test command, and the
/// successor path. A free destination is linked with rustc. A running link is
/// left alone. A busy destination is not written, and the report says the
/// running process was not replaced. The running process is not stopped.
pub fn rebuild_reload(
    root: &Path,
    source_rel: &str,
    destination: &Path,
    edit_marker: &str,
    link_running: bool,
) -> Result<String, String> {
    if source_rel != SOURCE_REL {
        return Err("desktop source file must be core/desktop.rs".into());
    }
    check_marker(edit_marker)?;
    real_directory(root)?;
    destination_name(destination)?;
    let source = source_file(root)?;
    let bytes = read_source(&source)?;
    let destination_text = path_text(destination)?;
    let busy = destination_busy(destination)?;
    let revision = revision_bytes(edit_marker, &bytes);
    let link = link_command(&source, destination)?;
    let outcome = if busy || link_running {
        LinkOutcome::Idle
    } else {
        match run_link(&source, destination) {
            Ok(()) => LinkOutcome::Wrote,
            Err(error) => LinkOutcome::Failed(error),
        }
    };
    Ok(report(
        edit_marker,
        &link,
        &hex(&revision),
        &destination_text,
        link_running,
        busy,
        outcome,
    ))
}

fn check_marker(marker: &str) -> Result<(), String> {
    if marker.is_empty() || marker.len() > 32 {
        return Err("edit marker must be 1 to 32 bytes".into());
    }
    if !marker
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("edit marker must be ASCII letters, digits, or hyphens".into());
    }
    Ok(())
}

fn real_directory(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("project root must be a real directory".into());
    }
    Ok(())
}

fn destination_name(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("destination is required".into());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("destination must not contain a relative segment".into());
    }
    match path.file_name().and_then(|name| name.to_str()) {
        Some(DEST_NAME) => Ok(()),
        _ => Err("destination file name must be RLLM.exe".into()),
    }
}

fn source_file(root: &Path) -> Result<std::path::PathBuf, String> {
    let path = root.join("core").join("desktop.rs");
    let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("desktop source must be a real file".into());
    }
    if meta.len() > SOURCE_CAP {
        return Err(format!("source file exceeds {SOURCE_CAP} bytes"));
    }
    Ok(path)
}

fn read_source(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > SOURCE_CAP {
        return Err(format!("source file exceeds {SOURCE_CAP} bytes"));
    }
    Ok(bytes)
}

fn path_text(path: &Path) -> Result<String, String> {
    match path.to_str() {
        Some(text) if !text.is_empty() && !text.bytes().any(|byte| byte < 0x20 || byte == b'"') => {
            Ok(text.to_string())
        }
        _ => Err("path cannot be recorded".into()),
    }
}

fn command_arg(text: &str) -> String {
    if text.contains(' ') {
        format!("\"{text}\"")
    } else {
        text.to_string()
    }
}

fn link_command(source: &Path, destination: &Path) -> Result<String, String> {
    let source = path_text(source)?;
    let destination = path_text(destination)?;
    Ok(format!(
        "rustc --edition 2021 --crate-type bin --crate-name rllm_desktop -o {} {}",
        command_arg(&destination),
        command_arg(&source)
    ))
}

fn destination_busy(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
        Ok(meta) if meta.file_type().is_symlink() => Err("destination must not be a link".into()),
        Ok(meta) if !meta.is_file() => Err("destination must be a file".into()),
        Ok(_) => match fs::OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => {
                drop(file);
                Ok(false)
            }
            Err(error) if busy_error(&error) => Ok(true),
            Err(error) => Err(format!("destination was not opened: {error}")),
        },
    }
}

fn busy_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32) | Some(33) | Some(1224))
}

enum LinkOutcome {
    Idle,
    Wrote,
    Failed(String),
}

fn run_link(source: &Path, destination: &Path) -> Result<(), String> {
    let output = std::process::Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg("--crate-type")
        .arg("bin")
        .arg("--crate-name")
        .arg("rllm_desktop")
        .arg("-o")
        .arg(destination)
        .arg(source)
        .output()
        .map_err(|error| format!("rustc was not started: {error}"))?;
    if output.status.success() && destination.is_file() {
        return Ok(());
    }
    let _ = fs::remove_file(destination);
    let err = String::from_utf8_lossy(&output.stderr);
    let text: String = err.chars().filter(|ch| !ch.is_control()).take(180).collect();
    Err(format!("rustc exited without writing the destination: {text}"))
}

fn revision_bytes(marker: &str, source: &[u8]) -> [u8; 32] {
    let mut body = Vec::with_capacity(marker.len() + source.len() + 16);
    body.extend_from_slice(b"marker:");
    body.extend_from_slice(marker.as_bytes());
    body.extend_from_slice(b"\0source:");
    body.extend_from_slice(source);
    sha256(&body)
}

fn report(
    marker: &str,
    link: &str,
    revision_hex: &str,
    successor: &str,
    link_running: bool,
    busy: bool,
    outcome: LinkOutcome,
) -> String {
    let mut lines = Vec::new();
    if busy {
        lines.push("The running process was not replaced.".to_string());
    }
    lines.push(format!("The source file is {SOURCE_REL}."));
    lines.push(format!("The edit marker is {marker}."));
    lines.push(format!("The link command is {link}."));
    lines.push(format!("The test command is {TEST_COMMAND}."));
    lines.push(format!("The revision bytes are {revision_hex}."));
    lines.push(format!("The successor path is {successor}."));
    lines.push("The destination is RLLM.exe.".to_string());
    lines.push("The desktop link command was recorded as data.".to_string());
    match outcome {
        LinkOutcome::Wrote => {
            lines.push("A desktop link was started.".to_string());
            lines.push("The destination file was written.".to_string());
        }
        LinkOutcome::Failed(error) => {
            lines.push(format!("A desktop link failed: {error}."));
            lines.push("The destination file was not written.".to_string());
        }
        LinkOutcome::Idle if link_running => {
            lines.push("A second desktop link was not started.".to_string());
            lines.push("The destination file was not written.".to_string());
        }
        LinkOutcome::Idle => {
            lines.push("A desktop link was not started.".to_string());
            lines.push("The destination file was not written.".to_string());
        }
    }
    lines.push("The running process was not stopped.".to_string());
    lines.join("\n")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn sha256(message: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bits = (message.len() as u64).saturating_mul(8);
    let mut data = Vec::with_capacity(message.len() + 72);
    data.extend_from_slice(message);
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bits.to_be_bytes());
    let mut offset = 0;
    while offset + 64 <= data.len() {
        let chunk = &data[offset..offset + 64];
        let mut words = [0u32; 64];
        for index in 0..16 {
            let start = index * 4;
            words[index] = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
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
            let choose = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
        offset += 64;
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

#[cfg(test)]
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rebuild-reload-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("core")).unwrap();
    fs::write(dir.join("core").join("desktop.rs"), b"fn desktop() {}\n").unwrap();
    dir
}

#[test]
fn sha256_empty_and_abc() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn busy_destination_says_the_running_process_was_not_replaced() {
    let root = scratch("busy");
    let destination = root.join("RLLM.exe");
    fs::write(&destination, b"prior-bytes").unwrap();
    #[cfg(windows)]
    let _hold = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&destination)
            .unwrap()
    };
    let report = rebuild_reload(&root, SOURCE_REL, &destination, "source-edit", true).unwrap();
    let successor = format!("The successor path is {}.", destination.to_str().unwrap());
    assert!(
        report.contains("The running process was not replaced."),
        "{report}"
    );
    assert!(report.contains(&successor), "{report}");
    assert!(report.contains("The source file is core/desktop.rs."), "{report}");
    assert!(!report.contains("was replaced."));
    drop(_hold);
    assert_eq!(fs::read(&destination).unwrap(), b"prior-bytes");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn successful_plan_names_source_rustc_and_destination() {
    let root = scratch("plan");
    let source = root.join("core").join("desktop.rs");
    let destination = root.join("RLLM.exe");
    let report = rebuild_reload(&root, SOURCE_REL, &destination, "source-edit", true).unwrap();
    let link = link_command(&source, &destination).unwrap();
    let revision = hex(&revision_bytes("source-edit", &fs::read(&source).unwrap()));
    assert!(report.contains("The source file is core/desktop.rs."), "{report}");
    assert!(
        link.starts_with("rustc --edition 2021 --crate-type bin --crate-name rllm_desktop -o "),
        "{link}"
    );
    assert!(link.contains("desktop.rs"), "{link}");
    assert!(link.contains("RLLM.exe"), "{link}");
    assert!(report.contains(&format!("The link command is {link}.")), "{report}");
    assert!(report.contains("The destination is RLLM.exe."), "{report}");
    assert!(
        report.contains("The test command is rustc --edition 2021 --test --crate-name rebuild_reload -o build\\rebuild_reload_test.exe core\\rebuild_reload.rs."),
        "{report}"
    );
    assert!(report.contains("The edit marker is source-edit."), "{report}");
    assert!(report.contains(&format!("The revision bytes are {revision}.")), "{report}");
    assert!(
        report.contains(&format!("The successor path is {}.", destination.to_str().unwrap())),
        "{report}"
    );
    assert!(report.contains("The desktop link command was recorded as data."), "{report}");
    assert!(report.contains("A second desktop link was not started."), "{report}");
    assert!(report.contains("The destination file was not written."), "{report}");
    assert!(report.contains("The running process was not stopped."), "{report}");
    assert!(!report.contains("The running process was not replaced."));
    assert!(!destination.exists());
    let other = rebuild_reload(&root, SOURCE_REL, &destination, "other-edit", true).unwrap();
    assert_ne!(
        report.lines().find(|line| line.starts_with("The revision bytes are ")),
        other.lines().find(|line| line.starts_with("The revision bytes are "))
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn free_destination_is_linked_and_written() {
    let root = scratch("link");
    fs::write(root.join("core").join("desktop.rs"), b"fn main() {}\n").unwrap();
    let destination = root.join("RLLM.exe");
    let report = rebuild_reload(&root, SOURCE_REL, &destination, "source-edit", false).unwrap();
    assert!(report.contains("A desktop link was started."), "{report}");
    assert!(report.contains("The destination file was written."), "{report}");
    assert!(report.contains("The running process was not stopped."), "{report}");
    assert!(!report.contains("The running process was not replaced."), "{report}");
    let written = fs::metadata(&destination).unwrap().len();
    assert!(written > 0, "{written}");
    let _ = fs::remove_dir_all(&root);
}

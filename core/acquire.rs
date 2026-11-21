//! Acquiring an external subject into the workspace so the existing binary
//! observation pipeline (inspect -> ELF/PE/DEX structure and control flow ->
//! reconstruct) can reverse-engineer it without the subject being pre-placed by
//! hand. Acquisition reads the source path the request names and stages it as a
//! new workspace file; the only thing in front of it is the ordinary operation
//! review the person already controls. There is no separate allowlist, size cap
//! or confirmation: a refusal here only reports a real failure (the source
//! cannot be opened or read, the chosen name already exists, or the staged
//! bytes do not read back).
use super::*;
use crate::host_fs::{OpenOptionsExt, MetadataExt};
use std::io::{Read, Write, Seek, SeekFrom};

/// A proposed acquisition names the subject's source path and the new workspace
/// file it will create, with an optional note. The output name follows the same
/// new-root-file rule as reconstruction: one plain filename at the workspace
/// root. The source is an ordinary host path and is deliberately unrestricted so
/// a subject anywhere the person can reach is acquirable.
pub(super) fn validate(a: &Json) -> Result<()> {
    let Obj(f) = a else { return Err("Acquisition requires an object".into()); };
    if f.keys().any(|k| !["source", "output", "note"].contains(&k.as_str())) {
        return Err("Acquisition accepts a source path, a new output filename and an optional note".into());
    }
    let source = get(a, "source").text()?;
    if source.is_empty() || source.contains('\0') || source.len() > 4096 {
        return Err("Name the subject's source path to acquire".into());
    }
    let output = get(a, "output").text()?;
    if output.is_empty() || output.len() > 255 || output == "." || output == ".." || output.contains(['/', '\\', '\0']) {
        return Err("Choose a new workspace-root output filename".into());
    }
    if text(a, "note").len() > 512 {
        return Err("Acquisition note exceeds 512 bytes".into());
    }
    Ok(())
}

/// Read the named subject and stage it as a new workspace file, returning a
/// provenance summary the model can act on. The staged bytes feed the existing
/// inspect/reconstruct pipeline, so the recursive inquiry gains new evidence.
pub(super) fn run(root: &Path, args: &Json, access: &operation::InspectionAccess) -> Result<String> {
    validate(args)?;
    acquire_into(root, &text(args, "output"), &text(args, "source"), &|| access.cancelled())
}

/// Stream-copy the subject into the workspace with bounded memory and verify the
/// staged file by reading it back. No size limit: a firmware image, app package
/// or large binary is acquired the same way a small one is.
fn acquire_into(root: &Path, name: &str, source: &str, cancelled: &impl Fn() -> bool) -> Result<String> {
    if cancelled() { return Err("Acquisition cancelled before reading the subject".into()); }
    let meta = fs::metadata(source).map_err(|e| format!("Cannot open the named subject: {e}"))?;
    if !meta.is_file() { return Err("The named subject is not a regular file".into()); }
    let mut input = fs::File::open(source).map_err(|e| format!("Cannot open the named subject: {e}"))?;
    let output = root.join(name);
    let mut out = fs::OpenOptions::new().read(true).write(true).create_new(true).custom_flags(0x20000).open(&output)
        .map_err(|e| format!("Cannot create acquisition output: {e}"))?;
    let mut hasher = incremental_digest::Digest::new();
    let mut buffer = [0u8; 65536];
    let mut total: u64 = 0;
    loop {
        if cancelled() { return Err(format!("Acquisition cancelled; incomplete new file {name} may remain")); }
        let n = input.read(&mut buffer).map_err(|e| format!("Cannot read the named subject: {e}"))?;
        if n == 0 { break; }
        out.write_all(&buffer[..n]).map_err(|e| format!("Acquisition output incomplete ({name}): {e}"))?;
        hasher.update(&buffer[..n]).map_err(|e| e.to_string())?;
        total += n as u64;
    }
    out.sync_all().map_err(|e| format!("Acquisition output not verified ({name}): {e}"))?;
    let sha = hasher.finish();
    out.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut check = incremental_digest::Digest::new();
    let mut read_total: u64 = 0;
    loop {
        if cancelled() { return Err(format!("Acquisition cancelled during readback; new file {name} retained")); }
        let n = out.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        check.update(&buffer[..n]).map_err(|e| e.to_string())?;
        read_total += n as u64;
    }
    if read_total != total || check.finish() != sha {
        return Err(format!("Acquisition output readback mismatch; new file {name} retained"));
    }
    let held = out.metadata().map_err(|e| e.to_string())?;
    let current = fs::symlink_metadata(&output).map_err(|e| e.to_string())?;
    if !current.is_file() || current.dev() != held.dev() || current.ino() != held.ino() {
        return Err(format!("Acquisition output name changed; created file {name} requires inspection"));
    }
    let trimmed: String = source.chars().filter(|c| !c.is_control()).take(256).collect();
    Ok(format!(
        "Acquired subject into the workspace: {name} ({total} bytes, SHA-256 {sha}). Source: {trimmed}. All bytes read back exactly; existing files were not replaced. Inspect {name} to observe its structure.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_requires_a_source_and_a_plain_root_filename() {
        assert!(validate(&obj(vec![("source", s("C:/Downloads/app.bin")), ("output", s("subject.bin"))])).is_ok());
        assert!(validate(&obj(vec![("source", s("/opt/app")), ("output", s("subject.bin")), ("note", s("TV firmware"))])).is_ok());
        assert!(validate(&obj(vec![("output", s("subject.bin"))])).is_err());
        assert!(validate(&obj(vec![("source", s("")), ("output", s("subject.bin"))])).is_err());
        assert!(validate(&obj(vec![("source", s("x")), ("output", s(""))])).is_err());
        assert!(validate(&obj(vec![("source", s("x")), ("output", s("a/b.bin"))])).is_err());
        assert!(validate(&obj(vec![("source", s("x")), ("output", s(".."))])).is_err());
        assert!(validate(&obj(vec![("source", s("x")), ("output", s("subject.bin")), ("other", s("x"))])).is_err());
    }

    #[test]
    fn acquire_streams_the_subject_and_reports_provenance() {
        let dir = std::env::temp_dir().join(format!("rllm-acquire-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let subject = dir.join("app.bin");
        let bytes = b"\x7fELF\x02\x01\x01\x00payload-of-a-subject".to_vec();
        fs::write(&subject, &bytes).unwrap();
        let report = acquire_into(&dir, "subject.bin", subject.to_str().unwrap(), &|| false).unwrap();
        assert!(report.contains("subject.bin"));
        assert!(report.contains(&harness_rules::sha256(&bytes)));
        assert_eq!(fs::read(dir.join("subject.bin")).unwrap(), bytes);
        // A second acquisition to the same name is refused rather than replacing it.
        assert!(acquire_into(&dir, "subject.bin", subject.to_str().unwrap(), &|| false).is_err());
        // A missing source reports a real open failure, not a silent pass.
        assert!(acquire_into(&dir, "missing.bin", dir.join("nope").to_str().unwrap(), &|| false).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}

mod twin;

use std::path::{Path, PathBuf};
use std::process::Command;

use twin::{obj, s, Value};

const SCOPE: &str = "Actual official zipalign and current saved APK payloads. Deliberately damaged temporary copies exercise pre-sign rejection. No new application or signature acceptance is claimed; transient retry is not forced.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project path is not UTF-8"))
        .to_string();
    let alignment = project.join("output/tools/alignment");
    let alignment_text = alignment
        .to_str()
        .unwrap_or_else(|| panic!("alignment path is not UTF-8"))
        .to_string();
    let original = subject(&project);
    let original_text = original
        .to_str()
        .unwrap_or_else(|| panic!("APK path is not UTF-8"))
        .to_string();
    let identity = twin::sha256_bytes(
        &std::fs::read(&original)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", original.display())),
    );
    let readers = verification_readers(&project);
    let publication = publication_record(&project);
    let readback = std::fs::read_to_string(project.join("tools/readback.rs"))
        .unwrap_or_else(|error| panic!("could not read tools/readback.rs: {error}"));
    let default_form = "RLLM_CHECK_APK',";
    let mut passed = Vec::new();

    check(
        &mut passed,
        "every instrument that reads an APK subject resolves it from the publication record instead of a default path it never verifies",
        !readers.is_empty()
            && readers.iter().all(|path| {
                let text = std::fs::read_to_string(path)
                    .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
                text.contains("PUBLICATION-MANIFEST.json") && !text.contains(default_form)
            }),
    );
    check(
        &mut passed,
        "the publication record names the delivered signed APK and the tree states no release APK while delivery is a companion file",
        as_str(publication.get("location")) == Some("companion")
            && as_str(publication.get("path"))
                .is_some_and(|path| path.ends_with(".apk"))
            && number(publication.get("bytes").unwrap_or(&Value::Null)) > 0
            && as_str(publication.get("sha256")).is_some_and(|digest| digest.len() == 64)
            && !release_apks(&project),
    );
    check(
        &mut passed,
        "the readback leg verifies the companion artifact by name and byte identity instead of assuming a release path",
        readback.contains("signed_location == \"companion\"")
            && readback.contains("apks.is_empty()")
            && readback.contains("signed_path != apk_name")
            && readback.contains("apk_bytes.len() as u64 != signed_bytes"),
    );

    let scratch = twin::create_scratch(&project, "apk-alignment-");
    let client_tools = std::env::var("CLIENT_TOOLS")
        .unwrap_or_else(|error| panic!("CLIENT_TOOLS: {error}"));
    let zipalign = Path::new(&client_tools).join("zipalign");
    let aligned = scratch.join("aligned.apk");
    align(&zipalign, &original_text, &aligned.to_string_lossy());
    check(
        &mut passed,
        "real official zipalign retains every current APK payload",
        verify(&alignment_text, &original_text, &aligned.to_string_lossy())
            == Ok(zip_entry_count(&original)),
    );

    let truncated = scratch.join("truncated.apk");
    let aligned_bytes = std::fs::read(&aligned)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", aligned.display()));
    std::fs::write(&truncated, &aligned_bytes[..aligned_bytes.len().min(2_048_000)])
        .unwrap_or_else(|error| panic!("could not write {}: {error}", truncated.display()));
    rejected(
        &mut passed,
        "truncated alignment output is rejected before signing",
        &alignment_text,
        &original_text,
        &truncated.to_string_lossy(),
    );

    let entries = zip_entries(&original);
    for kind in ["changed", "missing", "extra", "duplicate"] {
        let path = scratch.join(format!("{kind}.apk"));
        write_zip(&path, kind, &entries);
        rejected(
            &mut passed,
            &format!("valid ZIP with {kind} payload inventory cannot be signed"),
            &alignment_text,
            &original_text,
            &path.to_string_lossy(),
        );
    }

    let damaged = scratch.join("bad-crc.apk");
    write_zip(
        &damaged,
        "normal",
        &[("classes.dex".to_string(), b"original payload".to_vec())],
    );
    let mut raw = std::fs::read(&damaged)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", damaged.display()));
    let at = raw
        .windows(b"original payload".len())
        .position(|window| window == b"original payload")
        .unwrap_or_else(|| panic!("payload marker is missing"));
    raw[at] ^= 1;
    std::fs::write(&damaged, &raw)
        .unwrap_or_else(|error| panic!("could not write {}: {error}", damaged.display()));
    rejected(
        &mut passed,
        "payload CRC corruption cannot pass identity validation",
        &alignment_text,
        &original_text,
        &damaged.to_string_lossy(),
    );

    check(
        &mut passed,
        "packaging validation leaves the saved signed application unchanged",
        twin::sha256_bytes(
            &std::fs::read(&original)
                .unwrap_or_else(|error| panic!("could not read {}: {error}", original.display())),
        ) == identity,
    );

    let report = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("apk_alignment", &report, true);
    println!("{}", twin::pretty(&report));

    std::fs::remove_dir_all(&scratch)
        .unwrap_or_else(|error| panic!("could not remove {}: {error}", scratch.display()));
    let _ = project_text;
}

fn subject(project: &Path) -> PathBuf {
    if let Ok(configured) = std::env::var("RLLM_CHECK_APK") {
        if !configured.is_empty() {
            return absolute(Path::new(&configured));
        }
    }
    let publication = publication_record(project);
    let record_path = as_str(publication.get("path"))
        .unwrap_or_else(|| panic!("publication record has no signed APK path"));
    let name = Path::new(record_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("publication record has no APK file name"));
    if as_str(publication.get("location")) != Some("companion") {
        return absolute(&project.join("release/RLLM.apk"));
    }
    let mut candidates = vec![project.join(name)];
    let output = project.join("output");
    if let Ok(entries) = std::fs::read_dir(&output) {
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path().join(name)))
            .collect();
        paths.sort();
        candidates.extend(paths);
    }
    for candidate in candidates {
        if candidate.exists() {
            return absolute(&candidate);
        }
    }
    panic!("refusing to invent an artifact: PUBLICATION-MANIFEST.json delivers {name} as a companion file that is not on this machine; set RLLM_CHECK_APK to the APK under test");
}

fn publication_record(project: &Path) -> Value {
    let text = std::fs::read_to_string(project.join("PUBLICATION-MANIFEST.json"))
        .unwrap_or_else(|error| panic!("could not read PUBLICATION-MANIFEST.json: {error}"));
    let manifest = Value::parse(&text);
    manifest
        .get("signed_apk")
        .cloned()
        .unwrap_or_else(|| panic!("PUBLICATION-MANIFEST.json has no signed_apk record"))
}

fn verification_readers(project: &Path) -> Vec<PathBuf> {
    let directory = project.join("verification");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", directory.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "py"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter(|path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
                .contains("RLLM_CHECK_APK")
        })
        .collect()
}

fn release_apks(project: &Path) -> bool {
    let release = project.join("release");
    std::fs::read_dir(&release)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", release.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .any(|path| path.extension().is_some_and(|extension| extension == "apk"))
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|error| panic!("cwd: {error}"))
            .join(path)
    }
}

fn as_str(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    }
}

fn number(value: &Value) -> i128 {
    match value {
        Value::Num(text) => text
            .parse::<i128>()
            .unwrap_or_else(|error| panic!("invalid number {text}: {error}")),
        _ => panic!("expected number, got {}", value.to_string()),
    }
}

fn align(tool: &Path, original: &str, aligned: &str) {
    let status = Command::new("output/tools/alignment")
        .arg("--tool")
        .arg(tool)
        .arg(original)
        .arg(aligned)
        .status()
        .unwrap_or_else(|error| panic!("could not start alignment tool: {error}"));
    if !status.success() {
        panic!("zipalign failed");
    }
}

fn verify(alignment: &str, original: &str, candidate: &str) -> Result<i128, String> {
    let output = Command::new(alignment)
        .arg(original)
        .arg(candidate)
        .env("RLLM_ALIGNMENT_VERIFY", "1")
        .output()
        .unwrap_or_else(|error| panic!("could not start {alignment}: {error}"));
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            "verify failed".to_string()
        };
        return Err(message);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse::<i128>()
        .map_err(|error| format!("invalid verify output {}: {error}", stdout.trim()))
}

fn rejected(
    passed: &mut Vec<String>,
    name: &str,
    alignment: &str,
    original: &str,
    candidate: &str,
) {
    if verify(alignment, original, candidate).is_err() {
        passed.push(name.to_string());
    } else {
        panic!("{name}");
    }
}

fn zip_entry_count(path: &Path) -> i128 {
    zip_entries(path).len() as i128
}

fn zip_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    twin::zip_entries_of(path)
}

fn write_zip(path: &Path, kind: &str, entries: &[(String, Vec<u8>)]) {
    let mut written = Vec::new();
    for (index, (name, data)) in entries.iter().enumerate() {
        if kind == "missing" && index == 0 {
            continue;
        }
        let mut payload = data.clone();
        if kind == "changed" && index == 0 {
            payload.extend_from_slice(b"changed");
        }
        written.push((name.clone(), payload));
    }
    if kind == "extra" {
        written.push(("unexpected.txt".to_string(), b"extra".to_vec()));
    }
    if kind == "duplicate" {
        let (name, data) = entries.first().unwrap_or_else(|| panic!("duplicate ZIP has no member"));
        written.push((name.clone(), data.clone()));
    }
    twin::zip_write(path, &written);
}

fn check(passed: &mut Vec<String>, name: &str, value: bool) {
    if !value {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

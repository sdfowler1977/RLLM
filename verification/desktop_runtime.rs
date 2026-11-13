mod twin;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use twin::{obj, pretty, s, Value};

const SCOPE: &str = "Built Linux/Windows host runtime: script-declared build, runtime self-test over loopback sockets and real workspace tools, vault sealing at the declared path, cross-process resume, per-action settling and persistence, malformed-request tolerance, foreign-key and truncated-vault refusal.";
const REPORT: &str = "output/checks/desktop_runtime.json";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let mut passed = Vec::new();

    let build = run_script(&root);
    keep(
        &mut passed,
        "the host builds through its declared script",
        build.status.success(),
    );

    let binary = build_binary(&build);
    keep(
        &mut passed,
        "the build publishes an executable host",
        binary.is_file() && is_executable(&binary),
    );

    let scratch = Scratch::create(&root);
    let run_root = scratch.path();
    let key = run_root.join("key");
    let state = run_root.join("state.bin");
    let sealed = run_root.join("sealed.bin");

    keep(
        &mut passed,
        "the vault path is honored exactly as declared",
        !state.exists(),
    );

    let checked = host(
        &root,
        &binary,
        &["--check", "--key", key.to_str().unwrap(), "--state", sealed.to_str().unwrap(), "--root", run_root.to_str().unwrap()],
        "",
    );
    keep(
        &mut passed,
        "the host runtime self-test exits clean",
        checked.status.success(),
    );

    let stdout = String::from_utf8_lossy(&checked.stdout);
    let lines = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("ok   "))
        .collect::<Vec<_>>();
    let counted = stdout
        .lines()
        .filter(|line| line.starts_with("desktop self-test: "))
        .collect::<Vec<_>>();
    let expected_count = format!("desktop self-test: {} checks passed", lines.len());
    keep(
        &mut passed,
        "every exercised leg is named and the count matches the legs",
        lines.len() > 10 && counted.len() == 1 && counted[0] == expected_count,
    );

    for leg in [
        "a sent turn reaches the model over a real socket",
        "the model peer served exactly the requests the core built",
        "the approved leg ran a real workspace tool through the host",
        "a restart restores the sealed conversation into the first render",
        "the saved vault holds no plaintext conversation",
        "the microphone delivered real frames through the host",
        "the core stored the utterance captured on the Linux host",
        "the captured utterance reached the configured recognition connection",
    ] {
        keep(
            &mut passed,
            &format!("the self-test names its {leg} leg"),
            lines.iter().any(|line| *line == leg),
        );
    }

    keep(
        &mut passed,
        "the self-test reports no host-side failure",
        checked.stderr.is_empty(),
    );
    keep(
        &mut passed,
        "the self-test seals its vault at the declared path",
        sealed.is_file(),
    );
    let sealed_bytes = fs::read(&sealed).unwrap_or_else(|error| panic!("{}: {error}", sealed.display()));
    keep(
        &mut passed,
        "the sealed vault keeps no plaintext conversation",
        !sealed_bytes.windows(b"Linux host is online".len()).any(|window| window == b"Linux host is online"),
    );

    let served = host(
        &root,
        &binary,
        &["--key", key.to_str().unwrap(), "--state", state.to_str().unwrap(), "--root", run_root.to_str().unwrap()],
        "{\"action\":\"resume\"}\n",
    );
    let served_stdout = String::from_utf8_lossy(&served.stdout);
    keep(
        &mut passed,
        "the host renders on start and serves the resumed surface",
        served.status.success() && served_stdout.matches('\n').count() == 2,
    );

    let first = parse_last_json(&served_stdout);
    keep(
        &mut passed,
        "the host render carries the core surface contract",
        ["page", "messages", "tabs", "profile_fields", "palette"]
            .iter()
            .all(|name| first.get(name).is_some()),
    );

    let saved = host(
        &root,
        &binary,
        &["--key", key.to_str().unwrap(), "--state", state.to_str().unwrap(), "--root", run_root.to_str().unwrap()],
        "{\"action\":\"page\",\"inputs\":{\"page\":\"settings\"}}\n{\"action\":\"resume\"}\n",
    );
    let saved_stdout = String::from_utf8_lossy(&saved.stdout);
    keep(
        &mut passed,
        "the host settles every action and answers each one",
        saved.status.success() && saved_stdout.matches('\n').count() == 3,
    );
    keep(
        &mut passed,
        "the host persists the conversation after settling",
        state.is_file(),
    );

    let restored = host(
        &root,
        &binary,
        &["--key", key.to_str().unwrap(), "--state", state.to_str().unwrap(), "--root", run_root.to_str().unwrap()],
        "{\"action\":\"resume\"}\n",
    );
    let restored_stdout = String::from_utf8_lossy(&restored.stdout);
    keep(
        &mut passed,
        "a new host process resumes from the saved state",
        restored.status.success() && restored_stdout.contains("\"page\""),
    );

    let malformed = host(
        &root,
        &binary,
        &["--key", key.to_str().unwrap(), "--state", state.to_str().unwrap(), "--root", run_root.to_str().unwrap()],
        "not json\n{\"action\":\"resume\"}\n",
    );
    let malformed_stdout = String::from_utf8_lossy(&malformed.stdout);
    let malformed_stderr = String::from_utf8_lossy(&malformed.stderr);
    keep(
        &mut passed,
        "an unusable request is reported without ending the session",
        malformed.status.success()
            && malformed_stdout.matches('\n').count() == 2
            && !malformed_stderr.trim().is_empty(),
    );

    let foreign = run_root.join("foreign");
    fs::write(&foreign, [0u8; 32]).unwrap_or_else(|error| panic!("{}: {error}", foreign.display()));
    let wrong = host(
        &root,
        &binary,
        &["--key", foreign.to_str().unwrap(), "--state", state.to_str().unwrap(), "--root", run_root.to_str().unwrap()],
        "",
    );
    let wrong_stderr = String::from_utf8_lossy(&wrong.stderr);
    keep(
        &mut passed,
        "a foreign key over a saved vault fails closed instead of starting cold",
        !wrong.status.success() && !wrong_stderr.trim().is_empty(),
    );

    let state_bytes = fs::read(&state).unwrap_or_else(|error| panic!("{}: {error}", state.display()));
    let truncated = run_root.join("truncated.bin");
    fs::write(&truncated, &state_bytes[..state_bytes.len().min(24)])
        .unwrap_or_else(|error| panic!("{}: {error}", truncated.display()));
    let cut = host(
        &root,
        &binary,
        &["--key", key.to_str().unwrap(), "--state", truncated.to_str().unwrap(), "--root", run_root.to_str().unwrap()],
        "",
    );
    keep(
        &mut passed,
        "a truncated vault is refused instead of silently discarded",
        !cut.status.success(),
    );

    write_report(&passed);
    println!("{}", pretty(&report_value(&passed)));
}

fn report_value(passed: &[String]) -> Value {
    obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ])
}

fn write_report(passed: &[String]) {
    let path = Path::new(REPORT);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    }
    let mut body = pretty(&report_value(passed));
    body.push('\n');
    fs::write(path, body).unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
}

fn run_script(root: &Path) -> Output {
    Command::new("bash")
        .arg(root.join("desktop.sh"))
        .current_dir(root)
        .env("CLIENT_RUST", std::env::var_os("CLIENT_RUST").unwrap_or_else(|| panic!("CLIENT_RUST is not set")))
        .output()
        .unwrap_or_else(|error| panic!("could not run desktop.sh: {error}"))
}

fn build_binary(build: &Output) -> PathBuf {
    let stdout = String::from_utf8_lossy(&build.stdout);
    let line = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .next_back()
        .unwrap_or_else(|| panic!("desktop.sh did not publish a binary"));
    PathBuf::from(line)
}

fn host(root: &Path, binary: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not start {}: {error}", binary.display()));
    let mut input = child.stdin.take().unwrap_or_else(|| panic!("host stdin was not piped"));
    input
        .write_all(stdin.as_bytes())
        .and_then(|_| input.flush())
        .unwrap_or_else(|error| panic!("host write: {error}"));
    drop(input);
    // The leg runs each host invocation with subprocess.run(timeout=180); an unbounded
    // wait_with_output would let a hung host hang the twin instead of failing the check.
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().unwrap_or_else(|error| panic!("host wait: {error}")),
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("host exceeded its 180 second bound: {}", args.join(" "));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("host wait: {error}"),
        }
    }
}

fn parse_last_json(text: &str) -> Value {
    let line = text
        .lines()
        .next_back()
        .unwrap_or_else(|| panic!("host produced no JSON response"));
    Value::parse(line)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "desktop-runtime"))
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

//! Linux desktop host: the same application core, a loopback web surface, and
//! the platform work the Android Activity performs natively. All product logic
//! stays in the core; this file owns only the window, the transport and the key.
include!("lib.rs");

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

fn arguments() -> Vec<String> {
    std::env::args().skip(1).collect()
}

fn option(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|value| value == flag)?;
    args.get(index + 1).cloned()
}

fn data_directory() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.local/share")
    });
    PathBuf::from(base).join("rllm")
}

fn read_key(path: &Path) -> Result<[u8; 32]> {
    if let Ok(bytes) = std::fs::read(path) {
        if bytes.len() != 32 {
            return Err("State key file must hold exactly 32 bytes".into());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("State directory: {error}"))?;
    }
    let fresh = state_crypto::random_bytes(32)?;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("State key: {error}"))?;
    file.write_all(&fresh).map_err(|error| format!("State key: {error}"))?;
    file.sync_all().map_err(|error| format!("State key: {error}"))?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&fresh);
    Ok(key)
}

struct Vault {
    path: PathBuf,
    key: [u8; 32],
}

impl Vault {
    /// A missing file is a fresh vault. A sealed file that fails authentication
    /// is reported rather than replaced, so corruption is never silent.
    fn load(&self) -> Result<Option<Json>> {
        let sealed = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Saved state: {error}")),
        };
        let plain = state_crypto::open(&sealed, &self.key)?;
        let text = String::from_utf8(plain).map_err(|_| "Saved state is not UTF-8".to_string())?;
        match parse(&text)? {
            Obj(_) => Ok(Some(parse(&text)?)),
            _ => Err("Saved state is not an object".into()),
        }
    }

    fn save(&self, state: &Json) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("State directory: {error}"))?;
        }
        let sealed = state_crypto::seal(state.string().as_bytes(), &self.key)?;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.path)
            .map_err(|error| format!("Saved state: {error}"))?;
        file.write_all(&sealed).map_err(|error| format!("Saved state: {error}"))?;
        file.sync_all().map_err(|error| format!("Saved state: {error}"))?;
        Ok(())
    }
}

/// The core owns the request body: it hands over the framing and, for a request
/// that carries a stored file, streams the bytes through an owned upload ticket.
fn request_wire(effect: &Json) -> Result<Vec<u8>> {
    if get(effect, "wire_base64") != &Null {
        return network::unbase64(&text(effect, "wire_base64"));
    }
    Ok(text(effect, "wire").into_bytes())
}

fn write_request(sink: &mut impl Write, effect: &Json) -> Result<()> {
    sink.write_all(&request_wire(effect)?).map_err(|_| "peer closed early".to_string())?;
    let upload = text(effect, "media_upload");
    if upload.is_empty() {
        return Ok(());
    }
    let purpose = text(effect, "purpose");
    loop {
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
        sink.write_all(&chunk).map_err(|_| "peer closed during upload".to_string())?;
        if boolean(value, "done") {
            return Ok(());
        }
    }
}

/// Plain HTTP to the numeric address the core already resolved. TLS is handed to
/// the operating system's crypto stack, exactly as the Android layer hands it to
/// the platform's own HTTPS client; the core still validates every response byte.
fn exchange(effect: &Json) -> Result<Vec<u8>> {
    let host = text(effect, "host");
    let port = number(effect, "port");
    if host.is_empty() || port == 0 || port > 65535 {
        return Err("Network effect has no usable destination".into());
    }
    // A numeric literal from the core carries no zone id; drop one if it appears.
    let connect = host.replace('%', "");
    let display = if connect.contains(':') { format!("[{connect}]") } else { connect.clone() };
    if boolean(effect, "tls") {
        let peer = format!("{display}:{port}");
        let mut child = Command::new("openssl")
            .args(["s_client", "-connect", &peer, "-servername", &connect, "-quiet", "-brief"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "TLS needs the openssl client; use an http:// endpoint otherwise".to_string())?;
        let mut stdin = child.stdin.take().ok_or_else(|| "TLS stdin closed".to_string())?;
        write_request(&mut stdin, effect)?;
        drop(stdin);
        let mut response = Vec::new();
        child
            .stdout
            .take()
            .ok_or_else(|| "TLS stdout closed".to_string())?
            .read_to_end(&mut response)
            .map_err(|_| "TLS read failed".to_string())?;
        let status = child.wait().map_err(|_| "TLS client did not finish".to_string())?;
        if response.is_empty() {
            return Err(format!("TLS peer returned nothing (status {status})"));
        }
        return Ok(response);
    }
    let address = (display.as_str(), port as u16);
    let mut stream = TcpStream::connect(address).map_err(|error| format!("{host}:{port}: {error}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(120))).ok();
    write_request(&mut stream, effect)?;
    stream.flush().ok();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|_| "peer closed before finishing".to_string())?;
    Ok(response)
}

/// Microphone bytes for the core's own utterance segmenter. The host starts the
/// recorder, hands over 20 ms frames, and returns the receipt the core minted
/// for the completed utterance; recognition stays a core decision.
struct Recorder {
    stream: Box<dyn Read>,
    child: Option<std::process::Child>,
    header_checked: bool,
    pending: Vec<u8>,
}

impl Recorder {
    fn open() -> Result<Recorder> {
        if let Ok(path) = std::env::var("RLLM_VOICE_SOURCE") {
            let file = std::fs::File::open(&path).map_err(|error| format!("Microphone source: {error}"))?;
            return Ok(Recorder { stream: Box::new(file), child: None, header_checked: false, pending: Vec::new() });
        }
        for (program, arguments) in [
            ("pw-record", vec!["--rate", "16000", "--channels", "1", "--format", "s16", "-"]),
            ("arecord", vec!["-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "raw"]),
        ] {
            if let Ok(mut child) = Command::new(program)
                .args(&arguments)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
            {
                let stream = child.stdout.take().ok_or("the recorder produced no audio stream")?;
                return Ok(Recorder { stream: Box::new(stream), child: Some(child), header_checked: false, pending: Vec::new() });
            }
        }
        Err("no microphone recorder is available on the Linux host".into())
    }

    /// A recorder writing to a pipe may frame its output as WAVE; the core is fed
    /// raw little-endian samples only.
    fn fill(&mut self, want: usize) -> Result<Vec<u8>> {
        if !self.header_checked {
            self.header_checked = true;
            let mut head = [0u8; 4];
            if readSome(&mut self.stream, &mut head)? == 4 {
                if &head == b"RIFF" {
                    let mut ignored = [0u8; 40];
                    readSome(&mut self.stream, &mut ignored)?;
                } else {
                    self.pending.extend_from_slice(&head);
                }
            }
        }
        let mut bytes = std::mem::take(&mut self.pending);
        while bytes.len() < want {
            let mut chunk = [0u8; 512];
            let read = readSome(&mut self.stream, &mut chunk)?;
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

fn readSome(stream: &mut Box<dyn Read>, buffer: &mut [u8]) -> Result<usize> {
    match stream.read(buffer) {
        Ok(0) => Ok(0),
        Ok(read) => Ok(read),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => Ok(0),
        Err(error) => Err(format!("Microphone stream: {error}")),
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn microphone(effect: &Json, state: &mut Json, root: &str) -> Result<String> {
    let ticket = text(effect, "voice_audio_ticket");
    let session = text(effect, "voice_session");
    let frame = number(effect, "frame_bytes").clamp(2, 640) as usize;
    let lease = number(effect, "voice_lease_ms");
    let opening = core_event(
        state,
        root,
        "voice_audio_start",
        &obj(vec![("voice_audio_ticket", s(&ticket)), ("voice_session", s(&session))]),
    )?;
    *state = opening.get("state")?.clone();
    let mut recorder = Recorder::open()?;
    let fed_before = CAPTURED_FRAMES.load(std::sync::atomic::Ordering::Relaxed);
    let started = std::time::Instant::now();
    loop {
        if lease > 0 && started.elapsed().as_millis() as i64 >= lease {
            return Err("the microphone lease expired before an utterance completed".into());
        }
        let bytes = recorder.fill(frame)?;
        if bytes.len() < 2 || bytes.len() % 2 != 0 {
            return Err(format!(
                "the microphone source ended after {} frames without an utterance",
                CAPTURED_FRAMES.load(std::sync::atomic::Ordering::Relaxed) - fed_before
            ));
        }
        CAPTURED_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        let value = answer.get("value")?;
        if boolean(value, "ready") {
            return Ok(text(value, "receipt"));
        }
    }
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
    let mut action = action.to_string();
    let mut inputs = inputs;
    for _ in 0..EFFECT_HOPS {
        let reply = core_event(state, root, &action, &inputs)?;
        *state = reply.get("state")?.clone();
        let effect = reply.get("effect")?.clone();
        if effect == Null {
            return Ok(reply.get("ui")?.clone());
        }
        match text(&effect, "kind").as_str() {
            "network" => {
                let (next, callback) = match exchange(&effect) {
                    Ok(response) => ("network_result", callback_inputs(&effect, state, &response, None)),
                    Err(error) => ("failure", callback_inputs(&effect, state, &[], Some(error))),
                };
                action = next.into();
                inputs = callback;
            }
            "tool" => {
                let mut request = effect.clone();
                put(&mut request, "op", s("tool"));
                put(&mut request, "root", s(root));
                EXECUTED_TOOLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let answer = parse(&envelope(&request.string()))?;
                let output = match answer.get("error") {
                    Ok(error) => Err(error.text().unwrap_or("the tool failed").to_string()),
                    Err(_) => Ok(answer.get("value")?.clone()),
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
            "voice_listen" => {
                // The core segments the utterance; the host only supplies bytes
                // and replays the receipt it was given.
                match microphone(&effect, state, root) {
                    Ok(receipt) => {
                        action = "voice_audio_result".into();
                        inputs = obj(vec![
                            ("voice_audio_ticket", get(&effect, "voice_audio_ticket").clone()),
                            ("voice_session", get(&effect, "voice_session").clone()),
                            ("receipt", s(&receipt)),
                        ]);
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
            other => {
                // The core scopes a refused platform service to that operation, so
                // a service with no Linux binding yet cannot strand the app.
                action = "failure".into();
                inputs = obj(vec![
                    ("purpose", get(&effect, "purpose").clone()),
                    ("error", s(&format!("{other} is not available on the Linux host yet"))),
                ]);
            }
        }
    }
    Err("The core kept requesting platform work without settling".into())
}

fn main() {
    if let Err(error) = run(&arguments()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<()> {
    let root = option(args, "--root").unwrap_or_else(|| {
        std::env::current_dir().map(|value| value.display().to_string()).unwrap_or_else(|_| ".".into())
    });
    let directory = data_directory();
    let vault = Vault {
        path: option(args, "--state").map(PathBuf::from).unwrap_or_else(|| directory.join("state.bin")),
        key: read_key(&option(args, "--key").map(PathBuf::from).unwrap_or_else(|| directory.join("key")))?,
    };
    if args.iter().any(|value| value == "--check") {
        return self_test(&vault, &root);
    }
    let (mut state, ui) = startup(&vault, &root)?;
    println!("{}", ui.string());
    // The native surface drives the runtime one action at a time; the sealed
    // vault is rewritten after every settled action so a crash cannot lose a turn.
    for line in std::io::stdin().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => return Err(format!("Input: {error}")),
        };
        if line.trim().is_empty() {
            continue;
        }
        let outcome = (|| -> Result<Json> {
            let request = parse(&line)?;
            let action = field(&request, "action")?.to_string();
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
                                    wanted = header + length;
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
    let bytes = std::fs::read(&vault.path).map_err(|error| error.to_string())?;
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
        .and_then(|entry| std::fs::read(entry.path()).ok())
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

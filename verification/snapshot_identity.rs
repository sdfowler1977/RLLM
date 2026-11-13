mod twin;

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner execution of public legacy project_payload/current project_chunks operations, opaque bytes and pinned connections; unchanged production Android TLS socket block executed on the host JVM against a real local certificate, per-process fixture trust, successful alias/SNI and refused hostname mismatches. No archive extraction, excluded historical fixtures or physical Android/live-user NAS acceptance is claimed.";
const BODY: &[u8] = b"\x00\xffopaque\r\nHost: 100.64.0.2:5006\r\n\x80bytes";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let mut controller = Controller::spawn(&root_text);
    let mut passed = Vec::new();
    for streamed in [false, true] {
        run_protocol_checks(&mut controller, streamed, &mut passed);
    }
    run_production_tls_checks(&mut controller, &root, &mut passed);
    controller.finish();
    if passed.len() != 61 {
        panic!("snapshot_identity emitted {} checks", passed.len());
    }
    let report = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("snapshot_identity", &report, true);
}

fn run_protocol_checks(controller: &mut Controller, streamed: bool, passed: &mut Vec<String>) {
    let label = if streamed { "streamed" } else { "legacy" };
    for phase in ["write", "probe", "verify"] {
        let record = protocol_record(phase, streamed);
        let original = record.clone();
        let reply = protocol_request(controller, &record, streamed, None);
        let route = format!("{label} {phase} retains VPN socket route and HTTPS");
        let value = match reply.get("value") {
            Some(value) => value,
            None => panic!("{route}\n{}", reply.to_string()),
        };
        keep(
            passed,
            &route,
            text_is(value, "host", "100.64.0.2")
                && number_is(value, "port", "5006")
                && matches!(value.get("tls"), Some(Value::Bool(true))),
        );
        keep(
            passed,
            &format!("{label} {phase} retains configured certificate DNS identity"),
            text_is(value, "tls_host", "nas.example.test"),
        );
        let authority = format!("{label} {phase} applies certificate alias only to request authority");
        let wire = wire_bytes(value, &authority);
        let (header, payload) = split_wire(&wire, &authority);
        let mut header_line = header.clone();
        header_line.extend_from_slice(b"\r\n");
        keep(
            passed,
            &authority,
            contains(&header_line, b"\r\nHost: nas.example.test:5006\r\n"),
        );
        let credentials = format!(
            "Authorization: Basic {}",
            b64_encode(b"retained-user:retained-password")
        );
        keep(
            passed,
            &format!("{label} {phase} retains original connection credentials"),
            contains(&header, credentials.as_bytes()),
        );
        let method = if phase == "write" { "PUT" } else { "GET" };
        keep(
            passed,
            &format!("{label} {phase} preserves conditional write or read semantics"),
            wire.starts_with(format!("{method} /share/saved.bin HTTP/1.1\r\n").as_bytes())
                && contains(&header, b"If-None-Match: *") == (phase == "write"),
        );
        if phase == "write" {
            let length = format!("Content-Length: {}", BODY.len());
            let expected: &[u8] = if streamed { b"" } else { BODY };
            keep(
                passed,
                &format!("{label} write preserves exact body framing"),
                contains(&header, length.as_bytes()) && payload == expected,
            );
        } else {
            keep(
                passed,
                &format!("{label} {phase} carries no write payload"),
                payload.is_empty(),
            );
        }
        keep(
            passed,
            &format!("{label} {phase} leaves saved connection and source bytes unchanged"),
            record == original,
        );
        let handle = value
            .get("snapshot_handle")
            .unwrap_or_else(|| panic!("{label} {phase} snapshot handle is missing"))
            .clone();
        protocol_close(controller, streamed, &handle);
    }

    for invalid in [
        "https://nas.example.test",
        "*.example.test",
        "100.64.0.2",
        "bad\r\nInjected: value",
    ] {
        let mut record = protocol_record("write", streamed);
        let mut connection = record
            .get("connection")
            .unwrap_or_else(|| panic!("connection missing"))
            .clone();
        put(&mut connection, "tls_name", s(invalid));
        put(&mut record, "connection", connection);
        let reply = protocol_request(controller, &record, streamed, None);
        keep(
            passed,
            &format!(
                "{label} rejects invalid certificate identity {}",
                python_repr(invalid)
            ),
            reply.get("error").is_some() && reply.get("value").is_none(),
        );
    }

    let mut record = protocol_record("write", streamed);
    let mut connection = record
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    put(&mut connection, "url", s("http://100.64.0.2:5006/share/"));
    put(&mut connection, "plain", twin::b(true));
    put(&mut record, "connection", connection.clone());
    put(
        &mut record,
        "target",
        s("http://100.64.0.2:5006/share/saved.bin"),
    );
    let reply = protocol_request(controller, &record, streamed, None);
    keep(
        passed,
        &format!("{label} refuses certificate identity on unencrypted transport"),
        reply.get("error").is_some() && reply.get("value").is_none(),
    );

    let record = protocol_record("probe", streamed);
    let mut stored = record.clone();
    let mut connection = stored
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    put(&mut connection, "tls_name", s("changed.example.test"));
    put(&mut stored, "connection", connection);
    let reply = protocol_request(controller, &record, streamed, Some(&stored));
    keep(
        passed,
        &format!("{label} refuses changed saved connection before transport"),
        reply.get("error").is_some() && reply.get("value").is_none(),
    );

    let mut record = protocol_record("write", streamed);
    let mut connection = record
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    put(&mut connection, "tls_name", s(""));
    put(&mut record, "connection", connection);
    let reply = protocol_request(controller, &record, streamed, None);
    let ordinary = format!("{label} retains ordinary HTTPS when no alias is configured");
    let value = match reply.get("value") {
        Some(value) => value,
        None => panic!("{ordinary}\n{}", reply.to_string()),
    };
    keep(
        passed,
        &ordinary,
        matches!(value.get("tls"), Some(Value::Bool(true))) && no_tls_host(value),
    );
    let handle = value
        .get("snapshot_handle")
        .unwrap_or_else(|| panic!("{ordinary} snapshot handle is missing"))
        .clone();
    protocol_close(controller, streamed, &handle);
}

fn protocol_record(phase: &str, streamed: bool) -> Value {
    let connection = obj(vec![
        ("url".into(), s("https://100.64.0.2:5006/share/")),
        ("tls_name".into(), s("nas.example.test")),
        ("user".into(), s("retained-user")),
        ("password".into(), s("retained-password")),
        ("plain".into(), twin::b(false)),
    ]);
    let mut record = obj(vec![
        ("stage_ticket".into(), s("owned-fixture")),
        ("purpose".into(), s(&format!("project_transfer:owned-{phase}"))),
        ("bytes".into(), twin::n(&BODY.len().to_string())),
        ("phone".into(), twin::b(false)),
        ("tree".into(), Value::Null),
        ("connection".into(), connection),
        ("target".into(), s("https://100.64.0.2:5006/share/saved.bin")),
        ("mime".into(), s("application/octet-stream")),
        ("source".into(), Value::Null),
        ("phase".into(), s(phase)),
    ]);
    if streamed {
        put(&mut record, "format", twin::n("2"));
    }
    record
}

fn protocol_request(
    controller: &mut Controller,
    record: &Value,
    streamed: bool,
    stored: Option<&Value>,
) -> Value {
    let operation = if streamed { "project_chunks" } else { "project_payload" };
    let stored = stored.unwrap_or(record);
    controller.send(&obj(vec![
        ("op".into(), s(operation)),
        ("action".into(), s("open")),
        ("record".into(), record.clone()),
        ("stored".into(), stored.clone()),
        ("archive".into(), s(&b64_encode(BODY))),
    ]))
}

fn protocol_close(controller: &mut Controller, streamed: bool, handle: &Value) {
    let operation = if streamed { "project_chunks" } else { "project_close" };
    let reply = controller.send(&obj(vec![
        ("op".into(), s(operation)),
        ("action".into(), s("close")),
        ("handle".into(), handle.clone()),
    ]));
    if reply.get("error").is_some() || reply.get("value").is_none() {
        panic!("{operation} close failed: {}", reply.to_string());
    }
}

fn wire_bytes(value: &Value, check: &str) -> Vec<u8> {
    if let Some(encoded) = value.get("wire_base64") {
        let Value::Str(text) = encoded else {
            panic!("{check}");
        };
        let Some(bytes) = decode_loose(text) else {
            panic!("{check}");
        };
        return bytes;
    }
    match value.get("wire") {
        Some(Value::Str(text)) => text.as_bytes().to_vec(),
        _ => panic!("{check}"),
    }
}

fn split_wire(wire: &[u8], check: &str) -> (Vec<u8>, Vec<u8>) {
    let Some(separator) = wire.windows(4).position(|window| window == b"\r\n\r\n") else {
        panic!("{check}");
    };
    (wire[..separator].to_vec(), wire[separator + 4..].to_vec())
}

fn run_production_tls_checks(controller: &mut Controller, root: &Path, passed: &mut Vec<String>) {
    // Activity.java's TLS line is the subject. std has no TLS stack, so the peer
    // is the Python leg's certificate-backed ssl server: SNI comes from ClientHello,
    // and a hostname rejection must not deliver a second request body.
    let mut fixture = TlsFixture::create(root);
    let mut record = protocol_record("write", false);
    let mut connection = record
        .get("connection")
        .unwrap_or_else(|| panic!("connection missing"))
        .clone();
    let url = format!("https://127.0.0.1:{}/share/", fixture.port);
    put(&mut connection, "url", s(&url));
    put(&mut record, "connection", connection);
    put(&mut record, "target", s(&format!("{url}saved.bin")));
    let reply = protocol_request(controller, &record, false, None);
    let value = match reply.get("value") {
        Some(value) => value.clone(),
        None => panic!(
            "actual production TLS binding completes a trusted DNS-alias handshake over an IP socket\n{}",
            reply.to_string()
        ),
    };
    let wire_base64 = match value.get("wire_base64") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("actual production TLS binding completes a trusted DNS-alias handshake over an IP socket"),
    };
    let tls_host = match value.get("tls_host") {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    };
    let handle = value
        .get("snapshot_handle")
        .unwrap_or_else(|| panic!("production TLS snapshot handle is missing"))
        .clone();
    protocol_close(controller, false, &handle);

    let accepted = fixture.run(&tls_host, &wire_base64);
    let accepted_bytes = decode_loose(&accepted.stdout);
    keep_detail(
        passed,
        "actual production TLS binding completes a trusted DNS-alias handshake over an IP socket",
        accepted.success && accepted_bytes.as_ref().is_some_and(|bytes| bytes.ends_with(b"\r\n\r\nOK")),
        &format!(
            "java success={} timed_out={} stdout={} stderr={}",
            accepted.success, accepted.timed_out, accepted.stdout, accepted.stderr
        ),
    );
    keep_detail(
        passed,
        "actual TLS server receives unchanged opaque payload and aliased HTTP authority",
        captured_matches(&fixture.state, BODY),
        &format!(
            "state {}\nserver {}",
            fixture.state.to_string(),
            read_text(&fixture.stderr_path)
        ),
    );

    for name in ["", "wrong.example.test"] {
        let rejected = fixture.run(name, &wire_base64);
        let check = format!(
            "production hostname verification rejects {}",
            if name.is_empty() { "the socket IP" } else { "a mismatched DNS identity" }
        );
        keep_detail(
            passed,
            &check,
            !rejected.success && rejected.stderr.contains("SSLHandshakeException"),
            &format!(
                "java success={} timed_out={} stdout={} stderr={}",
                rejected.success, rejected.timed_out, rejected.stdout, rejected.stderr
            ),
        );
    }
    keep_detail(
        passed,
        "production TLS sends configured SNI while keeping failed peers from request bytes",
        has_name(&fixture.state, "nas.example.test")
            && has_name(&fixture.state, "wrong.example.test")
            && captured_len(&fixture.state) == 1,
        &format!(
            "state {}\nserver {}",
            fixture.state.to_string(),
            read_text(&fixture.stderr_path)
        ),
    );
}

struct TlsFixture {
    folder: PathBuf,
    port: u16,
    server: Child,
    events_path: PathBuf,
    stderr_path: PathBuf,
    jdk: PathBuf,
    events: u64,
    state: Value,
}

impl TlsFixture {
    fn create(root: &Path) -> Self {
        let folder = twin::create_scratch(root, "snapshot-tls");
        let certificate = folder.join("fixture.pem");
        let private = folder.join("fixture.key");
        let mut openssl = Command::new("openssl");
        openssl.args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=nas.example.test",
            "-addext",
            "subjectAltName=DNS:nas.example.test",
            "-keyout",
            private.to_str().unwrap_or_else(|| panic!("key path is not UTF-8")),
            "-out",
            certificate.to_str().unwrap_or_else(|| panic!("certificate path is not UTF-8")),
        ]);
        run_checked("openssl", openssl);
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("key mode: {error}"));
        let jdk = PathBuf::from(std::env::var_os("CLIENT_JDK").unwrap_or_else(|| panic!("CLIENT_JDK is not set")));
        let trust = folder.join("trust.p12");
        let mut keytool = Command::new(jdk.join("bin/keytool"));
        keytool.args([
            "-importcert",
            "-noprompt",
            "-alias",
            "fixture",
            "-file",
            certificate.to_str().unwrap_or_else(|| panic!("certificate path is not UTF-8")),
            "-keystore",
            trust.to_str().unwrap_or_else(|| panic!("trust path is not UTF-8")),
            "-storetype",
            "PKCS12",
            "-storepass",
            "controlled-fixture-password",
        ]);
        run_checked("keytool", keytool);
        let source = folder.join("SnapshotTlsChecks.java");
        std::fs::write(&source, java_source(&production_tls_block(root)))
            .unwrap_or_else(|error| panic!("could not write SnapshotTlsChecks.java: {error}"));
        let mut javac = Command::new(jdk.join("bin/javac"));
        javac
            .arg("--release")
            .arg("8")
            .arg("-d")
            .arg(&folder)
            .arg(&source);
        run_checked("javac", javac);

        let script = folder.join("snapshot_tls_server.py");
        std::fs::write(&script, SERVER_SCRIPT).unwrap_or_else(|error| panic!("could not write TLS server: {error}"));
        let events_path = folder.join("events.json");
        let port_path = folder.join("port");
        let stderr_path = folder.join("server.err");
        let stderr = std::fs::File::create(&stderr_path).unwrap_or_else(|error| panic!("server log: {error}"));
        let mut server = Command::new("python3");
        server
            .arg(&script)
            .arg(&certificate)
            .arg(&private)
            .arg(&port_path)
            .arg(&events_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr);
        let mut server = server
            .spawn()
            .unwrap_or_else(|error| panic!("could not start TLS fixture server: {error}"));
        let port = wait_port(&mut server, &port_path, &stderr_path);
        let state = load_state(&events_path).unwrap_or_else(|| obj(vec![]));
        let events = event_count(&state);
        Self {
            folder,
            port,
            server,
            events_path,
            stderr_path,
            jdk,
            events,
            state,
        }
    }

    fn run(&mut self, host: &str, wire_base64: &str) -> ProcessResult {
        let before = self.events;
        let result = self.java(host, wire_base64);
        if let Err(detail) = self.wait_event(before) {
            panic!(
                "TLS fixture did not record the connection for {host:?}\n{detail}\njava success={} timed_out={} stdout={} stderr={}",
                result.success, result.timed_out, result.stdout, result.stderr
            );
        }
        result
    }

    fn java(&self, host: &str, wire_base64: &str) -> ProcessResult {
        let trust = self.folder.join("trust.p12");
        let mut command = Command::new(self.jdk.join("bin/java"));
        command
            .arg(format!(
                "-Djavax.net.ssl.trustStore={}",
                trust.to_str().unwrap_or_else(|| panic!("trust path is not UTF-8"))
            ))
            .arg("-Djavax.net.ssl.trustStorePassword=controlled-fixture-password")
            .arg("-cp")
            .arg(&self.folder)
            .arg("SnapshotTlsChecks")
            .arg("127.0.0.1")
            .arg(self.port.to_string())
            .arg(host)
            .arg(wire_base64)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command_output(&mut command, Duration::from_secs(20))
    }

    fn wait_event(&mut self, before: u64) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(state) = load_state(&self.events_path) {
                let events = event_count(&state);
                if events > before {
                    self.events = events;
                    self.state = state;
                    return Ok(());
                }
            }
            if let Ok(Some(status)) = self.server.try_wait() {
                return Err(format!(
                    "TLS fixture server exited {status}: {}",
                    read_text(&self.stderr_path)
                ));
            }
            if Instant::now() > deadline {
                return Err(read_text(&self.stderr_path));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for TlsFixture {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn production_tls_block(root: &Path) -> String {
    let path = root.join("platform/com/local/client/Activity.java");
    let activity = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let marker = "if(command.optBoolean(\"tls\")){SSLSocket";
    let Some(start) = activity.find(marker) else {
        panic!("production TLS socket block is missing");
    };
    let end = activity[start..]
        .find('\n')
        .map(|offset| start + offset)
        .unwrap_or(activity.len());
    activity[start..end].trim().to_string()
}

fn java_source(tls_block: &str) -> String {
    let template = r#"
import java.net.*;import java.io.*;import java.util.*;import javax.net.ssl.*;
public class SnapshotTlsChecks{
 static int networkActive(int token,long deadline){return 10000;}
 static class Command{String name;Command(String n){name=n;}boolean optBoolean(String key){return true;}String optString(String key,String fallback){return name.isEmpty()?fallback:name;}boolean has(String key){return !name.isEmpty();}String getString(String key){return name;}}
 public static void main(String[] args)throws Exception{String host=args[0];int port=Integer.parseInt(args[1]),token=1;long transportDeadline=System.currentTimeMillis()+10000;Command command=new Command(args[2]);Socket raw=new Socket();raw.connect(new InetSocketAddress(host,port),5000);raw.setSoTimeout(5000);Socket connection=raw;
 TLS_BLOCK
 try(Socket managed=connection){managed.getOutputStream().write(Base64.getDecoder().decode(args[3]));managed.getOutputStream().flush();ByteArrayOutputStream result=new ByteArrayOutputStream();byte[] block=new byte[4096];int count;while((count=managed.getInputStream().read(block))!=-1)result.write(block,0,count);System.out.println(Base64.getEncoder().encodeToString(result.toByteArray()));}
 }
}
"#;
    template.replacen(" TLS_BLOCK", tls_block, 1)
}

const SERVER_SCRIPT: &str = r#"
import base64, json, os, pathlib, socketserver, ssl, sys, threading
certificate, private_key, port_file, events_file = sys.argv[1:5]
lock = threading.Lock()
state = {"events": 0, "names": [], "captured": [], "refused": []}

def dump():
    path = pathlib.Path(events_file)
    temporary = pathlib.Path(events_file + ".tmp")
    temporary.write_text(json.dumps(state))
    os.replace(temporary, path)

context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(certificate, private_key)

def remember(connection, name, ctx):
    with lock:
        state["names"].append(name)
context.set_servername_callback(remember)

class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        try:
            with context.wrap_socket(self.request, server_side=True) as connection:
                data = b""
                while b"\r\n\r\n" not in data:
                    part = connection.recv(4096)
                    if not part:
                        return
                    data += part
                    assert len(data) <= 65536
                header, payload = data.split(b"\r\n\r\n", 1)
                length = next(int(line.split(b":", 1)[1]) for line in header.split(b"\r\n") if line.lower().startswith(b"content-length:"))
                while len(payload) < length:
                    part = connection.recv(4096)
                    if not part:
                        return
                    payload += part
                with lock:
                    state["captured"].append({"header": base64.b64encode(header).decode(), "payload": base64.b64encode(payload).decode()})
                connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
        except ssl.SSLError as error:
            with lock:
                state["refused"].append(type(error).__name__)
        finally:
            with lock:
                state["events"] += 1
                dump()

class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True

server = Server(("127.0.0.1", 0), Handler)
dump()
pathlib.Path(port_file).write_text(str(server.server_address[1]))
server.serve_forever()
"#;

fn wait_port(server: &mut Child, port_path: &Path, stderr_path: &Path) -> u16 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(port_path) {
            if let Ok(port) = text.trim().parse::<u16>() {
                if port != 0 {
                    return port;
                }
            }
        }
        if let Ok(Some(status)) = server.try_wait() {
            panic!(
                "TLS fixture server exited {status}: {}",
                read_text(stderr_path)
            );
        }
        if Instant::now() > deadline {
            panic!(
                "TLS fixture server did not publish a port: {}",
                read_text(stderr_path)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn load_state(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let owned = text.trim().to_string();
    if !owned.starts_with('{') || !owned.ends_with('}') {
        return None;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Value::parse(&owned))).ok()
}

fn event_count(state: &Value) -> u64 {
    match state.get("events") {
        Some(Value::Num(token)) => token.parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

fn has_name(state: &Value, expect: &str) -> bool {
    let Some(Value::Arr(names)) = state.get("names") else {
        return false;
    };
    names
        .iter()
        .any(|name| matches!(name, Value::Str(text) if text == expect))
}

fn captured_len(state: &Value) -> usize {
    match state.get("captured") {
        Some(Value::Arr(items)) => items.len(),
        _ => 0,
    }
}

fn captured_matches(state: &Value, body: &[u8]) -> bool {
    let Some(Value::Arr(captured)) = state.get("captured") else {
        return false;
    };
    if captured.len() != 1 {
        return false;
    }
    let Some(Value::Str(header_b64)) = captured[0].get("header") else {
        return false;
    };
    let Some(Value::Str(payload_b64)) = captured[0].get("payload") else {
        return false;
    };
    let Some(header) = decode_loose(header_b64) else {
        return false;
    };
    let Some(payload) = decode_loose(payload_b64) else {
        return false;
    };
    payload == body && contains(&header, b"Host: nas.example.test:")
}

fn run_checked(label: &str, mut command: Command) {
    let result = command_output(&mut command, Duration::from_secs(120));
    if !result.success {
        panic!("{label} failed\n{}{}", result.stdout, result.stderr);
    }
}

struct ProcessResult {
    success: bool,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

fn command_output(command: &mut Command, timeout: Duration) -> ProcessResult {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("could not start command: {error}"));
    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("command wait: {error}"),
        }
    };
    let (stdout, stderr) = collect_pipes(&mut child);
    ProcessResult {
        success: status.is_some_and(|status| status.success()),
        timed_out,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

fn collect_pipes(child: &mut Child) -> (Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    (stdout, stderr)
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn keep_detail(passed: &mut Vec<String>, name: &str, ok: bool, detail: &str) {
    if !ok {
        panic!("{name}\n{detail}");
    }
    passed.push(name.to_string());
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn number_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Num(number)) if number == expect)
}

fn no_tls_host(value: &Value) -> bool {
    match value.get("tls_host") {
        None | Some(Value::Null) => true,
        Some(Value::Str(text)) => text.is_empty(),
        _ => false,
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn python_repr(text: &str) -> String {
    let mut out = String::from("'");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\'' => out.push_str("\\'"),
            ch => out.push(ch),
        }
    }
    out.push('\'');
    out
}

fn decode_loose(text: &str) -> Option<Vec<u8>> {
    let filtered: String = text.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
    if filtered
        .bytes()
        .any(|byte| !matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='))
    {
        return None;
    }
    Some(b64_decode(&filtered))
}

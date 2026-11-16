//! MCP catalog notification checks through the real controller and loopback HTTP peer.
mod twin;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const LENGTH_TAIL: &[u8] = b": framed tail\n\n";
const CHUNK_TAIL: &[u8] = b"0\r\nX-Trace: framing\r\n\r\n";
const BAD_CHUNK_TAIL: &[u8] = b"0\r\nContent-Length: 0\r\n\r\n";

fn overlong_chunk_header() -> Vec<u8> {
    let mut header = b"1;".to_vec();
    header.extend(std::iter::repeat(b'x').take(8191));
    header
}

fn limit_chunk_tail() -> Vec<u8> {
    let mut tail = b"0;".to_vec();
    tail.extend(std::iter::repeat(b'x').take(8190));
    tail.extend_from_slice(b"\r\n\r\n");
    tail
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let peer = Peer::start();
    let mut controller = Controller::spawn(root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")));
    let mut passed = Vec::new();
    run(&mut controller, &peer, &mut passed);
    // Count after the checks and before shutdown, matching the Python leg's
    // len(requests) taken while the peer is still serving.
    let http_requests = request_count(&peer);
    controller.finish();
    let body = twin::obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("http_requests".into(), twin::n(&http_requests.to_string())),
    ]);
    twin::report_value("mcp_catalog_events", &body, true);
}

struct PeerState {
    refresh: String,
    stream_case: String,
    post_case: String,
    requests: Vec<(String, String, String)>,
}

struct Peer {
    port: u16,
    state: Arc<Mutex<PeerState>>,
    scratch: PathBuf,
}

impl Peer {
    fn start() -> Peer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")).port();
        let state = Arc::new(Mutex::new(PeerState {
            refresh: "original".to_string(),
            stream_case: "normal".to_string(),
            post_case: "json".to_string(),
            requests: Vec::new(),
        }));
        let thread_state = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = stream.unwrap_or_else(|error| panic!("peer accept: {error}"));
                let state = Arc::clone(&thread_state);
                std::thread::spawn(move || handle(stream, port, state));
            }
        });
        let scratch = std::env::temp_dir().join(format!("mcp-catalog-events-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap_or_else(|error| panic!("scratch: {error}"));
        Peer { port, state, scratch }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn handle(mut stream: TcpStream, _port: u16, state: Arc<Mutex<PeerState>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap_or_else(|error| panic!("peer clone: {error}")));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(':') {
            headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or("")
    };
    if !matches!(path.as_str(), "/alpha" | "/beta" | "/march") {
        panic!("unexpected peer path {path}");
    }
    if header("authorization") != format!("Bearer catalog{path}") {
        panic!("peer authorization for {method} {path}");
    }
    if method == "GET" {
        if header("accept") != "text/event-stream" {
            panic!("peer GET accept for {path}");
        }
        let last = headers
            .iter()
            .find(|(name, _)| name == "last-event-id")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        let mut state = state.lock().unwrap();
        state.requests.push((path.clone(), "GET".to_string(), last));
        let stream_case = state.stream_case.clone();
        drop(state);
        if stream_case == "method-not-allowed" {
            // The endpoint does not offer the optional server GET stream. The
            // refusal is a status, not a broken stream.
            write_headers(
                &mut stream,
                "405 Method Not Allowed",
                &[("Content-Type", "text/event-stream"), ("Content-Length", "0")],
                &[],
            );
            return;
        }
        if stream_case == "unauthorized" {
            // The endpoint refuses the connection and says why in a bearer
            // challenge. The refusal is a status of the credential, not a
            // broken stream.
            write_headers(
                &mut stream,
                "401 Unauthorized",
                &[
                    ("WWW-Authenticate", "Bearer realm=\"mcp\", error=\"invalid_token\""),
                    ("Content-Type", "text/event-stream"),
                    ("Content-Length", "0"),
                ],
                &[],
            );
            return;
        }
        let other = notice("notifications/resources/list_changed", None);
        let changed = notice("notifications/tools/list_changed", Some(obj(vec![("untrusted".into(), s("SERVER_PAYLOAD"))])));
        if matches!(stream_case.as_str(), "trailer-observation" | "chunk-limit-observation") {
            write_headers(&mut stream, "200 OK", &[("Content-Type", "text/event-stream"), ("Transfer-Encoding", "chunked")], &[]);
            let ping = obj(vec![
                ("jsonrpc".into(), s("2.0")),
                ("id".into(), s("server-ping")),
                ("method".into(), s("ping")),
            ]);
            for body in [sse(&[other]), sse(&[changed, ping])] {
                write_chunk(&mut stream, &body);
            }
            let tail = if stream_case == "trailer-observation" {
                BAD_CHUNK_TAIL.to_vec()
            } else {
                overlong_chunk_header()
            };
            stream.write_all(&tail).unwrap_or_else(|error| panic!("peer write: {error}"));
            return;
        }
        let records = if path == "/march" {
            if stream_case == "bad-batch" {
                vec![Value::Arr(vec![changed, notice("notifications/custom", Some(Value::Arr(vec![])))])]
            } else {
                vec![Value::Arr(vec![other, changed])]
            }
        } else if stream_case == "resume" && header("last-event-id").is_empty() {
            vec![other]
        } else {
            vec![other, changed]
        };
        if let Some(prefix) = interim_prefix(&stream_case) {
            stream.write_all(prefix).unwrap_or_else(|error| panic!("peer write: {error}"));
        }
        write_body(&mut stream, 200, "text/event-stream", &sse(&records));
        return;
    }
    if method != "POST" {
        panic!("unexpected peer method {method}");
    }
    let length: usize = header("content-length").parse().unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap_or_else(|error| panic!("peer body: {error}"));
    let call = Value::parse(&String::from_utf8(body).unwrap_or_else(|error| panic!("peer utf8: {error}")));
    if call.get("method").is_none() {
        let ping = call.get("jsonrpc").map(Value::str_) == Some("2.0")
            && call.get("id").map(Value::str_) == Some("server-ping")
            && matches!(call.get("result"), Some(Value::Obj(fields)) if fields.is_empty())
            && matches!(&call, Value::Obj(fields) if fields.len() == 3);
        if !ping {
            panic!("unexpected control reply {}", call.to_string());
        }
        let call_text = call.to_string();
        let mut state = state.lock().unwrap();
        state.requests.push((path.clone(), "control-reply".to_string(), call_text));
        let post_case = state.post_case.clone();
        drop(state);
        if post_case == "control-valid" {
            stream
                .write_all(b"HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\nHTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
                .unwrap_or_else(|error| panic!("peer write: {error}"));
        } else {
            write_body(&mut stream, 202, "application/json", b"");
        }
        return;
    }
    let rpc_method = call.get("method").unwrap().str_().to_string();
    let args = call.get("params").cloned().unwrap_or_else(|| obj(vec![]));
    if rpc_method == "tools/call" {
        let name = args.get("name").map(Value::str_).unwrap_or("");
        let arguments = args.get("arguments").map(Value::to_string).unwrap_or_default();
        if name != "original" || arguments != "{}" {
            panic!("unexpected tools/call {}", args.to_string());
        }
    }
    let args_text = args.to_string();
    let post_case = {
        let mut state = state.lock().unwrap();
        state.requests.push((path.clone(), rpc_method.clone(), args_text));
        state.post_case.clone()
    };
    if rpc_method == "notifications/initialized" {
        write_body(&mut stream, 202, "application/json", b"");
        return;
    }
    let (result, changed) = {
        let state = state.lock().unwrap();
        let refresh = state.refresh.clone();
        let post_case = state.post_case.clone();
        let result = match rpc_method.as_str() {
            "initialize" => obj(vec![
                (
                    "protocolVersion".into(),
                    s(if path == "/march" { "2025-03-26" } else { "2025-11-25" }),
                ),
                (
                    "capabilities".into(),
                    obj(vec![("tools".into(), obj(vec![("listChanged".into(), Value::Bool(path != "/beta"))]))]),
                ),
                (
                    "serverInfo".into(),
                    obj(vec![("name".into(), s(&format!("Catalog peer{path}"))), ("version".into(), s("1"))]),
                ),
            ]),
            "tools/list" => {
                if matches!(post_case.as_str(), "split-list" | "failed-list") {
                    if args.get("cursor").is_none() && args.to_string() == "{}" {
                        obj(vec![("tools".into(), Value::Arr(vec![tool("first")])), ("nextCursor".into(), s("next"))])
                    } else if post_case == "failed-list" {
                        obj(vec![("tools".into(), Value::Arr(vec![obj(vec![("name".into(), s("bad")), ("inputSchema".into(), Value::Bool(false))])]))])
                    } else {
                        obj(vec![("tools".into(), Value::Arr(vec![tool("second")]))])
                    }
                } else if refresh == "bad" {
                    if args.to_string() == "{}" {
                        obj(vec![("tools".into(), Value::Arr(vec![tool("staged")])), ("nextCursor".into(), s("next"))])
                    } else {
                        obj(vec![("tools".into(), Value::Arr(vec![obj(vec![("name".into(), s("bad")), ("inputSchema".into(), Value::Bool(false))])]))])
                    }
                } else if refresh == "empty" {
                    obj(vec![("tools".into(), Value::Arr(vec![]))])
                } else {
                    obj(vec![("tools".into(), Value::Arr(vec![tool("original")]))])
                }
            }
            "tools/call" => obj(vec![(
                "content".into(),
                Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s("Reviewed original ran"))])]),
            )]),
            _ => panic!("unexpected method {rpc_method}"),
        };
        (
            result,
            notice("notifications/tools/list_changed", Some(obj(vec![("untrusted".into(), s("POST_SERVER_PAYLOAD"))]))),
        )
    };
    let reply = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), call.get("id").cloned().unwrap_or(Value::Null)),
        ("result".into(), result),
    ]);
    if rpc_method == "tools/call" && post_case != "json" {
        write_post_stream(&mut stream, &path, post_case.as_str(), reply, changed);
        return;
    }
    if rpc_method == "tools/list" && matches!(post_case.as_str(), "split-list" | "failed-list") && args.to_string() == "{}" {
        write_body(&mut stream, 200, "text/event-stream", &sse(&[changed, reply]));
        return;
    }
    write_body(&mut stream, 200, "application/json", reply.to_string().as_bytes());
}

fn write_post_stream(stream: &mut TcpStream, _path: &str, case: &str, mut reply: Value, changed: Value) {
    if case == "refused-401" {
        // The endpoint refuses the request and says why in a bearer challenge.
        write_headers(
            stream,
            "401 Unauthorized",
            &[
                ("WWW-Authenticate", "Bearer realm=\"mcp\", error=\"invalid_token\""),
                ("Content-Type", "application/json"),
                ("Content-Length", "0"),
            ],
            &[],
        );
        return;
    }
    if case == "refused-framing" {
        // A refused head is framing too. The coding header is repeated, so the
        // framing rules refuse the head before its status can name anything.
        write_headers(
            stream,
            "401 Unauthorized",
            &[
                ("Content-Type", "application/json"),
                ("Content-Encoding", "identity"),
                ("Content-Encoding", "identity"),
                ("Content-Length", "0"),
            ],
            &[],
        );
        return;
    }
    if case.starts_with("control-") {
        write_control_stream(stream, case, reply);
        return;
    }
    if matches!(
        case,
        "framed-length" | "truncated-length" | "framed-chunk" | "truncated-chunk" | "bad-trailer" | "close-delimited"
    ) {
        write_framed(stream, case, sse(&[changed, reply]));
        return;
    }
    if case == "json-spoof" {
        put(&mut reply, "mcp_catalog_changed", Value::Bool(true));
        write_body(stream, 200, "application/json", reply.to_string().as_bytes());
        return;
    }
    if case == "wrong-id" {
        put(&mut reply, "id", s("unrelated-request"));
    }
    if case == "invalid-result" {
        put(&mut reply, "result", obj(vec![("content".into(), Value::Bool(false))]));
    }
    let records = if case == "bad-march" {
        vec![Value::Arr(vec![changed, notice("notifications/custom", Some(Value::Arr(vec![])))]), reply]
    } else if case == "march" {
        vec![Value::Arr(vec![notice("notifications/resources/list_changed", None), changed]), Value::Arr(vec![reply])]
    } else if case == "late" {
        vec![reply, changed]
    } else if case == "duplicate-terminal" {
        let mut duplicate = reply.clone();
        put(
            &mut duplicate,
            "result",
            obj(vec![(
                "content".into(),
                Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s("Conflicting terminal"))])]),
            )]),
        );
        vec![changed, reply, duplicate]
    } else if case == "duplicate" {
        vec![changed, reply.clone(), reply]
    } else if case == "batch-then-single" {
        let error = obj(vec![
            ("jsonrpc".into(), s("2.0")),
            ("id".into(), reply.get("id").cloned().unwrap_or(Value::Null)),
            (
                "error".into(),
                obj(vec![("code".into(), twin::n("-32603")), ("message".into(), s("Conflicting terminal"))]),
            ),
        ]);
        vec![changed, Value::Arr(vec![reply]), error]
    } else if case == "single-then-batch" {
        vec![changed, reply.clone(), Value::Arr(vec![reply])]
    } else if case == "unrelated-after-terminal" {
        let mut unrelated = reply.clone();
        put(&mut unrelated, "id", s("unrelated-request"));
        vec![changed, reply, unrelated]
    } else {
        vec![changed, reply]
    };
    let mime = if case == "wrong-mime" { "text/plain" } else { "text/event-stream" };
    write_body(stream, 200, mime, &sse(&records));
}

fn write_framed(stream: &mut TcpStream, case: &str, mut body: Vec<u8>) {
    let length_text = (body.len() + LENGTH_TAIL.len()).to_string();
    let mut headers = vec![("Content-Type", "text/event-stream")];
    if case == "close-delimited" {
        headers.push(("Connection", "close"));
    }
    if matches!(case, "framed-length" | "truncated-length") {
        headers.push(("Content-Length", length_text.as_str()));
        if case == "framed-length" {
            body.extend_from_slice(LENGTH_TAIL);
        }
    } else if case != "close-delimited" {
        headers.push(("Transfer-Encoding", "chunked"));
        let mut framed = format!("{:x}\r\n", body.len()).into_bytes();
        framed.extend_from_slice(&body);
        framed.extend_from_slice(b"\r\n");
        if case != "truncated-chunk" {
            framed.extend_from_slice(if case == "bad-trailer" { BAD_CHUNK_TAIL } else { CHUNK_TAIL });
        }
        body = framed;
    }
    write_headers(stream, "200 OK", &headers, &body);
}

fn write_control_stream(stream: &mut TcpStream, case: &str, reply: Value) {
    let ping = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), s("server-ping")),
        ("method".into(), s("ping")),
    ]);
    let body = sse(&[ping, reply]);
    let mut head = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n".to_vec();
    if case == "control-header-limit" {
        let interim = b"HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n";
        let mut padded = head.clone();
        padded.extend_from_slice(format!("Content-Length: {}\r\nX-Pad: ", body.len()).as_bytes());
        if padded.len() < 65536 {
            padded.resize(65536, b'x');
        }
        padded.extend_from_slice(b"\r\n\r\n");
        let mut wire = interim.to_vec();
        wire.extend_from_slice(&padded);
        wire.extend_from_slice(&body);
        stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
        return;
    }
    if let Some(tail) = control_tail(case) {
        head.extend_from_slice(b"Transfer-Encoding: chunked\r\n\r\n");
        let mut wire = head;
        wire.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
        wire.extend_from_slice(&body);
        wire.extend_from_slice(b"\r\n");
        wire.extend_from_slice(&tail);
        stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
        return;
    }
    if matches!(case, "control-boundary-valid" | "control-boundary-invalid") {
        let split = 17;
        let first = &body[..split];
        let second = &body[split..];
        let mut wire = head;
        wire.extend_from_slice(b"Transfer-Encoding: chunked\r\n\r\n");
        wire.extend_from_slice(format!("{:x}\r\n", first.len()).as_bytes());
        wire.extend_from_slice(first);
        wire.extend_from_slice(b"\r\n");
        wire.extend_from_slice(format!("{:x}\r\n", second.len()).as_bytes());
        wire.extend_from_slice(second);
        wire.extend_from_slice(if case == "control-boundary-valid" { b"\r\n0\r\n\r\n" } else { b"\rX" });
        stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
        return;
    }
    let mut prefix = Vec::new();
    if case == "control-bad-status" {
        replace_once(&mut head, b" 200", b"  200");
    } else if case == "control-bad-name" {
        head.extend_from_slice(b"Bad Name: value\r\n");
    } else if case == "control-bad-value" {
        head.extend_from_slice(b"X-Trace: bad\x00value\r\n");
    } else if case == "control-bad-interim" {
        prefix.extend_from_slice(b"HTTP/1.1 103 Early Hints\r\nBad Name: value\r\n\r\n");
    } else if case == "control-switching" {
        prefix.extend_from_slice(b"HTTP/1.1 101 Switching Protocols\r\n\r\n");
    } else if case == "control-valid" {
        prefix.extend_from_slice(b"HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n");
    }
    if matches!(case, "control-bad-chunk" | "control-bad-last-chunk" | "control-valid") {
        let extension = if case == "control-bad-chunk" { ";=bad" } else { " \t; flag; key=\"quoted; value\"" };
        let last = if case == "control-bad-last-chunk" { ";=bad" } else { ";done" };
        head.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
        let mut wire = prefix;
        wire.extend_from_slice(&head);
        wire.extend_from_slice(b"\r\n");
        wire.extend_from_slice(format!("{:x}", body.len()).as_bytes());
        wire.extend_from_slice(extension.as_bytes());
        wire.extend_from_slice(b"\r\n");
        wire.extend_from_slice(&body);
        wire.extend_from_slice(b"\r\n0");
        wire.extend_from_slice(last.as_bytes());
        wire.extend_from_slice(b"\r\n\r\n");
        stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
        return;
    }
    let length = if case == "control-bad-length" {
        format!("+{}", body.len())
    } else {
        body.len().to_string()
    };
    head.extend_from_slice(format!("Content-Length: {length}\r\n").as_bytes());
    let mut wire = prefix;
    wire.extend_from_slice(&head);
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(&body);
    stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
}

fn control_tail(case: &str) -> Option<Vec<u8>> {
    match case {
        "control-trailer-forbidden" => Some(BAD_CHUNK_TAIL.to_vec()),
        "control-trailer-line" => Some(BAD_CHUNK_TAIL[..BAD_CHUNK_TAIL.len() - 2].to_vec()),
        "control-trailer-malformed" => Some(b"0\r\nBad Name: value\r\n\r\n".to_vec()),
        "control-trailer-extra" => Some(b"0\r\n\r\nextra".to_vec()),
        "control-trailer-valid" => Some(CHUNK_TAIL.to_vec()),
        "control-chunk-limit" => Some(limit_chunk_tail()),
        "control-chunk-overlong" => Some(overlong_chunk_header()),
        _ => None,
    }
}

fn replace_once(buf: &mut Vec<u8>, from: &[u8], to: &[u8]) {
    let Some(pos) = buf.windows(from.len()).position(|window| window == from) else {
        return;
    };
    buf.splice(pos..pos + from.len(), to.iter().copied());
}

fn notice(method: &str, params: Option<Value>) -> Value {
    let mut fields = vec![("jsonrpc".into(), s("2.0")), ("method".into(), s(method))];
    if let Some(params) = params {
        fields.push(("params".into(), params));
    }
    obj(fields)
}

fn tool(name: &str) -> Value {
    obj(vec![("name".into(), s(name)), ("inputSchema".into(), obj(vec![("type".into(), s("object"))]))])
}

fn sse(records: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, record) in records.iter().enumerate() {
        out.extend_from_slice(format!("id: event-{}\ndata: {}\n\n", index + 1, record.to_string()).as_bytes());
    }
    out
}

fn write_headers(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut wire = format!("HTTP/1.1 {status}\r\n").into_bytes();
    for (name, value) in headers {
        wire.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(body);
    stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
}

fn write_body(stream: &mut TcpStream, status: u16, mime: &str, body: &[u8]) {
    let mut wire = format!("HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body);
    stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
}

/// The informational head a peer may put in front of a real response. An
/// informational head is framing too, so the same three shapes are offered
/// here and in the mcp_catalog group: one input can judge both paths.
fn interim_prefix(case: &str) -> Option<&'static [u8]> {
    Some(match case {
        "interim-identity" => b"HTTP/1.1 103 Early Hints\r\nContent-Encoding: identity\r\n\r\n",
        "interim-duplicate-coding" => {
            b"HTTP/1.1 103 Early Hints\r\nContent-Encoding: identity\r\nContent-Encoding: identity\r\n\r\n"
        }
        "interim-gzip" => b"HTTP/1.1 103 Early Hints\r\nContent-Encoding: gzip\r\n\r\n",
        _ => return None,
    })
}

fn write_chunk(stream: &mut TcpStream, body: &[u8]) {
    let mut wire = format!("{:x}\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body);
    wire.extend_from_slice(b"\r\n");
    stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
}

#[derive(Clone, Copy)]
struct Post {
    feed_error: bool,
    finish_error: bool,
    whole: bool,
    split_terminal: bool,
    read_size: usize,
}

impl Post {
    fn plain() -> Self {
        Self { feed_error: false, finish_error: false, whole: false, split_terminal: false, read_size: 37 }
    }

    fn feed(read_size: usize) -> Self {
        Self { feed_error: true, read_size, ..Self::plain() }
    }
}

fn run(controller: &mut Controller, peer: &Peer, passed: &mut Vec<String>) {
    let root = peer.scratch.to_str().unwrap_or_else(|| panic!("scratch is not UTF-8"));
    let resume = controller.event(None, "resume", obj(vec![]), root);
    let registered = register(controller, state(&resume), "/alpha", true, peer, root);
    let discovered = discover(controller, state(&registered), root);
    keep(passed, "baseline complete catalog from the real peer", same_names(state(&discovered), &["original"]));
    let listening = negotiate(controller, state(&discovered), "mcp_client_listen", root);
    let (sid, obs) = open_get(controller, &listening);
    keep(
        passed,
        "negotiated standalone stream distinguishes tool-list changes",
        obs.get("tool_changes") == Some(&twin::n("1")) && obs.get("notifications") == Some(&twin::n("2")),
    );
    let wrong = progress(controller, &listening, &obs, Some("mcp_client_server:stale"), root);
    keep(passed, "stale progress cannot mark the current connection", unmarked(state(&wrong)));
    let mut replaced = listening.clone();
    set_nested(&mut replaced, "mcp", "key", s("replacement"));
    let rejected = progress(controller, &replaced, &obs, None, root);
    keep(passed, "changed connection credentials invalidate old observations", unmarked(state(&rejected)));
    let mut revoked = listening.clone();
    set_nested(&mut revoked, "storage_access", "mcp", Value::Bool(false));
    let rejected = progress(controller, &revoked, &obs, None, root);
    keep(passed, "revoked server access prevents adoption of live observations", unmarked(state(&rejected)));
    let malformed_counts = with_field(&obs, "tool_changes", twin::n("3"));
    let malformed = progress(controller, &listening, &malformed_counts, None, root);
    keep(passed, "impossible counts cannot mark a catalog", unmarked(state(&malformed)));
    let step = progress(controller, &listening, &obs, None, root);
    keep(
        passed,
        "live owned observation marks a refresh without auto dispatch",
        effect_is_null(&step)
            && state(&step).get("notice").map(Value::str_).unwrap_or("").contains("refresh recommended")
            && same_names(state(&step), &["original"])
            && !step.to_string().contains("SERVER_PAYLOAD"),
    );
    let stepped = with_state(&listening, state(&step));
    let regress_counts = with_field(&obs, "tool_changes", twin::n("0"));
    let regress = progress(controller, &stepped, &regress_counts, None, root);
    keep(
        passed,
        "regressing progress cannot clear the observation",
        state(&regress).get("mcp_client_observation").and_then(|value| value.get("tool_changes")) == Some(&twin::n("1")),
    );
    let closed = finish_get(controller, &stepped, &sid, root, false);
    let marked = changed(controller, state(&closed), root);
    keep(passed, "completed stream leaves the old catalog and offers manual refresh", marked && same_names(state(&closed), &["original"]));
    let observed = controller.event(Some(state(&closed)), "mcp_client_observation", obj(vec![]), root);
    keep(passed, "the stream observation offers the same explicit refresh action", refresh_button(&observed));
    let count = request_count(peer);
    let tools = controller.event(Some(state(&closed)), "mcp_tools", obj(vec![]), root);
    let chosen = controller.event(Some(state(&tools)), "mcp_tool:0", obj(vec![]), root);
    let reviewed = controller.event(Some(state(&chosen)), "mcp_client_review", obj(vec![("arguments".into(), s("{}"))]), root);
    keep(
        passed,
        "dirty catalog preserves explicit review and sends no call before approval",
        effect_is_null(&reviewed) && request_count(peer) == count,
    );
    let approved = controller.event(Some(state(&reviewed)), "mcp_client_approve", obj(vec![]), root);
    let initialized = exchange(controller, &approved, root);
    let notified = exchange(controller, &initialized, root);
    let done = exchange(controller, &notified, root);
    let marked = changed(controller, state(&done), root);
    keep(
        passed,
        "the reviewed tool can complete while the catalog is dirty",
        output_has(state(&done), "Reviewed original ran") && marked,
    );
    set_refresh(peer, "bad");
    let listed = negotiate(controller, state(&done), "mcp_client_list", root);
    let first_page = exchange(controller, &listed, root);
    let bad = exchange(controller, &first_page, root);
    let marked = changed(controller, state(&bad), root);
    keep(
        passed,
        "failed later page retains the old list and refresh indication",
        notice_failed(state(&bad)) && same_names(state(&bad), &["original"]) && marked,
    );
    let beta_registered = register(controller, state(&bad), "/beta", false, peer, root);
    let listen_beta = negotiate(controller, state(&beta_registered), "mcp_client_listen", root);
    let (sid, obs) = open_get(controller, &listen_beta);
    keep(
        passed,
        "unadvertised tool-change event contributes only generic counts",
        obs.get("notifications") == Some(&twin::n("2")) && obs.get("tool_changes") == Some(&twin::n("0")),
    );
    let beta_closed = finish_get(controller, &listen_beta, &sid, root, false);
    let marked = changed(controller, state(&beta_closed), root);
    keep(passed, "other connection has no inherited refresh indication", !marked);
    let selected = select_index(controller, state(&beta_closed), 0, root);
    let marked = changed(controller, state(&selected), root);
    keep(passed, "returning to the original saved connection restores its own indication", marked);
    let resumed = controller.event(Some(state(&selected)), "resume", obj(vec![]), root);
    let marked = changed(controller, state(&resumed), root);
    keep(passed, "saved indication survives controller resume", marked);
    set_refresh(peer, "empty");
    let emptied = discover(controller, state(&resumed), root);
    let marked = changed(controller, state(&emptied), root);
    keep(passed, "terminal empty discovery clears the indication with the catalog", names(state(&emptied)).is_empty() && !marked);
    set_stream_case(peer, "resume");
    let paused = negotiate(controller, state(&emptied), "mcp_client_listen_resume", root);
    let (sid, obs) = open_get(controller, &paused);
    keep(passed, "first resumable segment has no tool-change event", obs.get("tool_changes") == Some(&twin::n("0")));
    let paused = finish_get(controller, &paused, &sid, root, true);
    // The peer counter has to see a second GET when the product really sends
    // one, so a later bound on the number of GETs is not vacuous.
    let gets_before_resume = request_method_count(peer, "GET");
    let resumed = controller.event(Some(state(&paused)), "mcp_client_resume", obj(vec![]), root);
    let (sid, obs) = open_get(controller, &resumed);
    keep(
        passed,
        "the peer counts the second server GET an explicit resume sends",
        request_method_count(peer, "GET") == gets_before_resume + 1,
    );
    keep(
        passed,
        "owned resume retains negotiated change capability and cursor",
        obs.get("tool_changes") == Some(&twin::n("1")) && last_request(peer) == ("/alpha".to_string(), "GET".to_string(), "event-1".to_string()),
    );
    let resumed = finish_get(controller, &resumed, &sid, root, false);
    let marked = changed(controller, state(&resumed), root);
    keep(passed, "final-only resumed observation marks the correct connection", marked);
    let resumed = controller.event(Some(state(&resumed)), "mcp_client_resume_close", obj(vec![]), root);
    let march_registered = register(controller, state(&resumed), "/march", false, peer, root);
    let listening = negotiate(controller, state(&march_registered), "mcp_client_listen", root);
    let (sid, obs) = open_get(controller, &listening);
    keep(
        passed,
        "negotiated March SSE batch counts only the tool-list notification",
        obs.get("tool_changes") == Some(&twin::n("1")) && obs.get("notifications") == Some(&twin::n("2")),
    );
    let closed = finish_get(controller, &listening, &sid, root, false);
    let marked = changed(controller, state(&closed), root);
    keep(passed, "March terminal callback also exposes the refresh action", marked);
    let clean = discover(controller, state(&closed), root);
    set_stream_case(peer, "bad-batch");
    let listening = negotiate(controller, state(&clean), "mcp_client_listen", root);
    let effect = listening.get("effect").cloned().unwrap_or_else(|| panic!("march listen effect"));
    let stream = open_stream(controller, &effect, "mcp_server");
    let raw = network(&effect);
    let bad = feed(controller, &stream, &raw);
    let observation = bad.get("observation").cloned().unwrap_or_else(|| panic!("bad batch observation: {}", bad.to_string()));
    let observed = progress(controller, &listening, &observation, None, root);
    keep(
        passed,
        "a malformed March batch commits no tool-change count or indication",
        bad.get("error").is_some()
            && observation.get("tool_changes") == Some(&twin::n("0"))
            && state(&observed).get("mcp_catalog_changes") == state(&clean).get("mcp_catalog_changes"),
    );
    close_stream(controller, &stream);
    keep(
        passed,
        "no notification automatically dispatches discovery or calls",
        request_method_count(peer, "tools/list") == 5 && request_method_count(peer, "tools/call") == 1,
    );
    set_refresh(peer, "original");
    let selected = select_path(controller, state(&clean), "/alpha", root);
    let alpha = discover(controller, state(&selected), root);
    let pending = call_original(controller, state(&alpha), root);
    set_post_case(peer, "single");
    let result = post_stream(controller, &pending, passed, Post::plain());
    let payload = http_body_json(result.get("data").unwrap_or_else(|| panic!("result data")).str_());
    keep(passed, "negotiated POST SSE emits a separate typed catalog observation", result.get("mcp_catalog_changed") == Some(&Value::Bool(true)));
    keep(passed, "correlated terminal result is unchanged and excludes the notice payload", payload == expected_reply(&pending));
    let stale = post_deliver(controller, &pending, &result, Some("mcp_client_result:stale"), root);
    keep(passed, "stale POST completion cannot mark the catalog", unmarked(state(&stale)));
    let mut revoked = pending.clone();
    set_nested(&mut revoked, "storage_access", "mcp", Value::Bool(false));
    let revoked = post_deliver(controller, &revoked, &result, None, root);
    keep(passed, "revoked connection rejects the POST observation", unmarked(state(&revoked)));
    let mut replaced = pending.clone();
    set_nested(&mut replaced, "mcp", "key", s("replacement"));
    let replaced = post_deliver(controller, &replaced, &result, None, root);
    keep(passed, "changed credentials reject the POST observation", unmarked(state(&replaced)));
    let done = post_deliver(controller, &pending, &result, None, root);
    let marked = changed(controller, state(&done), root);
    keep(
        passed,
        "approved call adopts its result and offers manual refresh without dispatch",
        effect_is_null(&done) && marked && same_names(state(&done), &["original"]) && output_has(state(&done), "Reviewed original ran") && !done.to_string().contains("POST_SERVER_PAYLOAD"),
    );
    let cleared = discover(controller, state(&done), root);
    let replay_event = with_state(&pending, state(&cleared));
    let replay = post_deliver(controller, &replay_event, &result, None, root);
    let marked = changed(controller, state(&replay), root);
    keep(passed, "replayed completion cannot restore a cleared catalog indication", !marked);
    set_post_case(peer, "duplicate-terminal");
    let pending = call_original(controller, state(&alpha), root);
    let bad = post_stream(controller, &pending, passed, Post { whole: true, ..Post::feed(37) });
    let bad_error = error_text(&bad);
    keep(
        passed,
        "two correlated single responses in one fragment fail without publishing either result",
        bad_error.contains("Duplicate correlated MCP terminal response") && is_open(&bad) && bad.get("data").is_none() && bad.get("mcp_catalog_changed").is_none(),
    );
    let another = call_original(controller, state(&alpha), root);
    let bad_split = post_stream(controller, &another, passed, Post { split_terminal: true, ..Post::feed(37) });
    keep(
        passed,
        "duplicate terminal in the next read fails before returning a response",
        error_text(&bad_split).contains("Duplicate correlated MCP terminal response") && bad_split.get("data").is_none(),
    );
    let failed = fail(controller, state(&pending), &purpose_of(&pending), &bad_error, false, root);
    let marked = changed(controller, state(&failed), root);
    keep(
        passed,
        "scoped duplicate failure preserves the last catalog without adopting the notice",
        same_names(state(&failed), &["original"]) && !marked && notice_failed(state(&failed)),
    );
    set_post_case(peer, "single");
    let retry = call_original(controller, state(&failed), root);
    let retry_result = post_stream(controller, &retry, passed, Post::plain());
    let retry = post_deliver(controller, &retry, &retry_result, None, root);
    keep(passed, "a fresh reviewed call still succeeds after duplicate refusal", output_has(state(&retry), "Reviewed original ran"));
    for case in ["late", "json-spoof", "wrong-mime", "wrong-id", "invalid-result"] {
        let pending = call_original(controller, state(&alpha), root);
        set_post_case(peer, case);
        let result = post_stream(controller, &pending, passed, Post { finish_error: case == "wrong-id", ..Post::plain() });
        if case == "wrong-id" {
            keep(passed, "uncorrelated SSE reply exports no catalog observation", result.get("mcp_catalog_changed").is_none());
        } else {
            let done = post_deliver(controller, &pending, &result, None, root);
            let marked = changed(controller, state(&done), root);
            keep(passed, &format!("{case} cannot dirty the catalog"), !marked);
            if case == "invalid-result" {
                keep(passed, "invalid tool result still reports its validation failure", notice_failed(state(&done)));
            } else {
                keep(passed, &format!("{case} preserves the valid call result"), output_has(state(&done), "Reviewed original ran"));
            }
        }
    }
    let beta = select_path(controller, state(&alpha), "/beta", root);
    let beta = discover(controller, state(&beta), root);
    set_post_case(peer, "single");
    let pending = call_original(controller, state(&beta), root);
    let result = post_stream(controller, &pending, passed, Post::plain());
    let done = post_deliver(controller, &pending, &result, None, root);
    let marked = changed(controller, state(&done), root);
    keep(passed, "unadvertised POST notification leaves the other connection clean", result.get("mcp_catalog_changed").is_none() && !marked);
    let march = select_path(controller, state(&beta), "/march", root);
    let march = discover(controller, state(&march), root);
    let pending = call_original(controller, state(&march), root);
    set_post_case(peer, "march");
    let result = post_stream(controller, &pending, passed, Post::plain());
    let done = post_deliver(controller, &pending, &result, None, root);
    let marked = changed(controller, state(&done), root);
    keep(
        passed,
        "March request-direction batch marks the catalog without changing the correlated batch result",
        marked && output_has(state(&done), "Reviewed original ran"),
    );
    set_post_case(peer, "bad-march");
    let pending = call_original(controller, state(&march), root);
    let bad = post_stream(controller, &pending, passed, Post::feed(37));
    keep(passed, "invalid March batch exposes neither a completion nor a POST observation", bad.get("data").is_none() && bad.get("mcp_catalog_changed").is_none());
    let mut paging_final = Value::Null;
    for case in ["split-list", "failed-list"] {
        set_post_case(peer, case);
        let pending = negotiate(controller, state(&alpha), "mcp_client_list", root);
        let streamed = post_stream(controller, &pending, passed, Post::plain());
        let page = post_deliver(controller, &pending, &streamed, None, root);
        keep(
            passed,
            &format!("{case} keeps the last complete catalog while paging"),
            same_names(state(&page), &["original"]) && catalog_flagged(state(&page)),
        );
        let streamed = post_stream(controller, &page, passed, Post::plain());
        paging_final = post_deliver(controller, &page, &streamed, None, root);
        if case == "split-list" {
            let marked = changed(controller, state(&paging_final), root);
            keep(passed, "later clean page retains the indication for the assembled catalog", same_names(state(&paging_final), &["first", "second"]) && marked);
        } else {
            let marked = changed(controller, state(&paging_final), root);
            keep(
                passed,
                "invalid later page retains both the old catalog and the observed change",
                notice_failed(state(&paging_final)) && same_names(state(&paging_final), &["original"]) && marked,
            );
        }
    }
    set_post_case(peer, "json");
    let clean_page = discover(controller, state(&paging_final), root);
    let marked = changed(controller, state(&clean_page), root);
    keep(passed, "clean discovery after an interleaved notice clears the indication", !marked);
    let mut transport_failed = Value::Null;
    for (case, connection) in [("duplicate", &alpha), ("batch-then-single", &march), ("single-then-batch", &march)] {
        let pending = call_original(controller, state(connection), root);
        set_post_case(peer, case);
        let rejected = post_stream(controller, &pending, passed, Post::feed(8192));
        let message = error_text(&rejected);
        keep(
            passed,
            &format!("{case} rejects a second correlated reply in the same transport read"),
            message.contains("Duplicate correlated MCP terminal response")
                && is_open(&rejected)
                && rejected.get("data").is_none()
                && rejected.get("mcp_catalog_changed").is_none(),
        );
        transport_failed = fail(controller, state(&pending), &purpose_of(&pending), &message, true, root);
        let marked = changed(controller, state(&transport_failed), root);
        keep(
            passed,
            &format!("{case} keeps the last complete catalog and yields a scoped error without replay"),
            same_names(state(&transport_failed), &["original"]) && notice_failed(state(&transport_failed)) && !marked && effect_is_null(&transport_failed),
        );
    }
    let evidence = peer.scratch.join("evidence.txt");
    std::fs::write(&evidence, b"local evidence").unwrap_or_else(|error| panic!("evidence: {error}"));
    let read = read_evidence(controller, root);
    keep(passed, "local read works after the MCP transport refusal", read);
    let pending = call_original(controller, state(&transport_failed), root);
    set_post_case(peer, "unrelated-after-terminal");
    let streamed = post_stream(controller, &pending, passed, Post { read_size: 8192, ..Post::plain() });
    let recovered = post_deliver(controller, &pending, &streamed, None, root);
    let marked = changed(controller, state(&recovered), root);
    keep(
        passed,
        "a fresh reviewed call adopts its result when a trailing reply has an unrelated ID",
        output_has(state(&recovered), "Reviewed original ran") && marked,
    );
    for (case, message) in [
        ("control-trailer-forbidden", "Framing trailer forbidden"),
        ("control-trailer-line", "Framing trailer forbidden"),
        ("control-trailer-malformed", "Invalid response header"),
        ("control-trailer-extra", "Bytes after framed response"),
        ("control-chunk-overlong", "Stream chunk header exceeds limit"),
    ] {
        let pending = call_original(controller, state(&alpha), root);
        set_post_case(peer, case);
        let before = request_method_count(peer, "control-reply");
        let rejected = post_stream(controller, &pending, passed, Post::feed(16384));
        keep(
            passed,
            &format!("{case} refuses a same-read ping before exporting a control effect"),
            error_is(&rejected, message) && no_controls(&rejected) && is_open(&rejected),
        );
        let failed = fail(controller, state(&pending), &purpose_of(&pending), message, true, root);
        let marked = changed(controller, state(&failed), root);
        keep(
            passed,
            &format!("{case} retains the catalog without replay or a ping reply"),
            same_names(state(&failed), &["original"]) && !marked && notice_failed(state(&failed)) && effect_is_null(&failed) && request_method_count(peer, "control-reply") == before,
        );
    }
    let pending = call_original(controller, state(&alpha), root);
    let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("trailer effect"));
    set_post_case(peer, "control-trailer-valid");
    let raw = network(&effect);
    let stream = open_stream(controller, &effect, protocol_of(&effect));
    let early = feed(controller, &stream, &raw[..raw.len() - CHUNK_TAIL.len()]);
    keep(
        passed,
        "a validated earlier chunk offers its ping before trailers arrive",
        is_open(&early) && early.get("error").is_none() && early.get("controls").map(|value| value.arr().len()) == Some(1),
    );
    let control = early.get("controls").unwrap().arr()[0].clone();
    let ack = network(&control);
    let ack_reply = control_ack(controller, &stream, &control, &ack, true);
    keep(passed, "the early ping receives a real HTTP acknowledgment while framing is incomplete", is_open(&ack_reply));
    let partial = feed(controller, &stream, &CHUNK_TAIL[..CHUNK_TAIL.len() - 1]);
    let last = feed(controller, &stream, &CHUNK_TAIL[CHUNK_TAIL.len() - 1..]);
    keep(
        passed,
        "valid fragmented trailers wait for final CRLF and emit no duplicate controls",
        partial.get("error").is_none() && is_open(&partial) && no_controls(&partial) && last.get("error").is_none() && is_complete(&last) && no_controls(&last),
    );
    let ended = end_stream(controller, &stream, false);
    let done = post_deliver(controller, &pending, &envelope_value(&ended), None, root);
    keep(passed, "valid trailers preserve the correlated result", output_has(state(&done), "Reviewed original ran"));
    for case in ["control-chunk-limit", "control-chunk-overlong"] {
        let pending = call_original(controller, state(&alpha), root);
        let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("chunk effect"));
        set_post_case(peer, case);
        let raw = network(&effect);
        let stream = open_stream(controller, &effect, protocol_of(&effect));
        let early = feed(controller, &stream, &raw[..8192]);
        keep(
            passed,
            &format!("{case} offers its earlier ping from an Android-sized read"),
            early.get("error").is_none() && is_open(&early) && early.get("controls").map(|value| value.arr().len()) == Some(1),
        );
        let control = early.get("controls").unwrap().arr()[0].clone();
        let ack = network(&control);
        let ack_reply = control_ack(controller, &stream, &control, &ack, true);
        keep(passed, &format!("{case} accepts a real ping acknowledgment before complete chunk framing"), is_open(&ack_reply));
        if case == "control-chunk-overlong" {
            let rejected = feed(controller, &stream, &raw[8192..]);
            keep(
                passed,
                "the next Android-sized read refuses the overlong header before CRLF",
                error_is(&rejected, "Stream chunk header exceeds limit") && no_controls(&rejected) && is_open(&rejected),
            );
            let finish = end_stream(controller, &stream, true);
            keep(passed, "earlier ping acknowledgment cannot settle the refused tool result", finish.get("error").is_some());
            continue;
        }
        let partial = feed(controller, &stream, &raw[8192..raw.len() - 3]);
        keep(passed, "an exact-limit chunk header with pending CR waits for LF", partial.get("error").is_none() && is_open(&partial) && no_controls(&partial));
        let last = feed(controller, &stream, &raw[raw.len() - 3..]);
        keep(
            passed,
            "the split delimiter completes the exact-limit chunk header without a duplicate reply",
            last.get("error").is_none() && is_complete(&last) && no_controls(&last),
        );
        let ended = end_stream(controller, &stream, false);
        let done = post_deliver(controller, &pending, &envelope_value(&ended), None, root);
        keep(passed, "the exact-limit header preserves the correlated result", output_has(state(&done), "Reviewed original ran"));
    }
    for (case, message) in [("trailer-observation", "Framing trailer forbidden"), ("chunk-limit-observation", "Stream chunk header exceeds limit")] {
        let listening = negotiate(controller, state(&alpha), "mcp_client_listen", root);
        let effect = listening.get("effect").cloned().unwrap_or_else(|| panic!("observation effect"));
        set_stream_case(peer, case);
        let raw = network(&effect);
        set_stream_case(peer, "normal");
        let stream = open_stream(controller, &effect, "mcp_server");
        let split = chunk_split(&raw);
        let early = feed(controller, &stream, &raw[..split]);
        let early_obs = early.get("observation").cloned().unwrap_or_else(|| panic!("early observation"));
        let rejected = feed(controller, &stream, &raw[split..]);
        keep(
            passed,
            &format!("{case} preserves prior GET progress and suppresses same-read observations and controls"),
            early_obs.get("notifications") == Some(&twin::n("1"))
                && no_tool_changes(&early_obs)
                && error_is(&rejected, message)
                && rejected.get("observation") == Some(&early_obs)
                && no_controls(&rejected)
                && is_open(&rejected),
        );
        let progressed = progress(controller, &listening, rejected.get("observation").unwrap_or_else(|| panic!("rejected observation")), None, root);
        keep(passed, &format!("{case} rejected GET progress cannot dirty the catalog"), unmarked(state(&progressed)));
        close_stream(controller, &stream);
    }
    // An endpoint that does not offer the optional server GET stream refuses
    // it with a status. The refusal names that optional stream, so the user
    // is not left with a generic transport failure, and it is not retried.
    {
        let before_gets = request_method_count(peer, "GET");
        let listening = negotiate(controller, state(&alpha), "mcp_client_listen", root);
        let effect = listening.get("effect").cloned().unwrap_or_else(|| panic!("listen effect"));
        set_stream_case(peer, "method-not-allowed");
        let raw = network(&effect);
        set_stream_case(peer, "normal");
        let stream = open_stream(controller, &effect, "mcp_server");
        let refused = feed(controller, &stream, &raw);
        keep(
            passed,
            "an endpoint without the optional MCP GET stream names that stream instead of failing generically",
            error_is(
                &refused,
                "This MCP endpoint does not offer an optional server GET stream (HTTP 405); no retry was sent",
            ),
        );
        keep(
            passed,
            "a refused optional MCP GET stream sends no retry request",
            request_method_count(peer, "GET") == before_gets + 1,
        );
        // The host reports the refused stream to the product the way it reports
        // every other failed transport. The product must stop waiting for that
        // stream and hand the user back.
        let reported = fail(
            controller,
            state(&listening),
            effect.get("purpose").map(Value::str_).unwrap_or_else(|| panic!("stream purpose")),
            &error_text(&refused),
            true,
            root,
        );
        keep(
            passed,
            "a refused optional MCP GET stream leaves the product waiting for nothing",
            released(state(&reported).get("mcp_client_expected")),
        );
        keep(
            passed,
            "a refused optional MCP GET stream releases the product to act again",
            idle(state(&reported)),
        );
        close_stream(controller, &stream);
    }
    // A refused MCP exchange is refused where the head is read, in the MCP
    // client's own words. The shared decoder's sentence belongs to the web
    // reader, so a refused MCP head never shows it.
    {
        let pending = call_original(controller, state(&alpha), root);
        let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("refused request effect"));
        set_post_case(peer, "refused-401");
        let raw = network(&effect);
        set_post_case(peer, "json");
        let stream = open_stream(controller, &effect, protocol_of(&effect));
        let refused = feed(controller, &stream, &raw);
        keep_detail(
            passed,
            "a refused MCP request stream is refused in the MCP client's own words",
            error_is(
                &refused,
                "MCP HTTP 401: the endpoint reported the bearer token RLLM sent as invalid. Save a current token for this server and connect again.",
            ),
            format!("error={:?}", error_text(&refused)),
        );
        // The host reports the refused stream to the product the way it reports
        // every other failed transport, so the product stops waiting for it.
        fail(
            controller,
            state(&pending),
            &purpose_of(&pending),
            &error_text(&refused),
            true,
            root,
        );
        close_stream(controller, &stream);
    }
    {
        let listening = negotiate(controller, state(&alpha), "mcp_client_listen", root);
        let effect = listening.get("effect").cloned().unwrap_or_else(|| panic!("listen effect"));
        set_stream_case(peer, "unauthorized");
        let raw = network(&effect);
        set_stream_case(peer, "normal");
        let stream = open_stream(controller, &effect, "mcp_server");
        let refused = feed(controller, &stream, &raw);
        keep_detail(
            passed,
            "a refused MCP server stream is refused in the MCP client's own words",
            error_is(
                &refused,
                "MCP HTTP 401: the endpoint reported the bearer token RLLM sent as invalid. Save a current token for this server and listen again.",
            ),
            format!("error={:?}", error_text(&refused)),
        );
        fail(
            controller,
            state(&listening),
            effect.get("purpose").map(Value::str_).unwrap_or_else(|| panic!("stream purpose")),
            &error_text(&refused),
            true,
            root,
        );
        close_stream(controller, &stream);
    }
    // A refused head is framing too. Its framing is judged before its status
    // decides the outcome, so the same head the request path refuses as
    // framing is refused here as framing and never as a status.
    {
        let pending = call_original(controller, state(&alpha), root);
        let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("refused framing effect"));
        set_post_case(peer, "refused-framing");
        let raw = network(&effect);
        set_post_case(peer, "json");
        let stream = open_stream(controller, &effect, protocol_of(&effect));
        let refused = feed(controller, &stream, &raw);
        keep_detail(
            passed,
            "a refused MCP head with a repeated coding header is refused as framing",
            error_is(&refused, "Duplicate streaming framing header"),
            format!("error={:?}", error_text(&refused)),
        );
        fail(
            controller,
            state(&pending),
            &purpose_of(&pending),
            &error_text(&refused),
            true,
            root,
        );
        close_stream(controller, &stream);
    }
    // An informational head is framing too. The streaming path judges the
    // same three head shapes the request path judges in the mcp_catalog
    // group, so one input can be read against both paths.
    {
        // The plain head first, so the accepted interim head can be compared
        // with what the same stream yields without one.
        let listening = negotiate(controller, state(&alpha), "mcp_client_listen", root);
        let effect = listening.get("effect").cloned().unwrap_or_else(|| panic!("listen effect"));
        let raw = network(&effect);
        let stream = open_stream(controller, &effect, "mcp_server");
        let plain = feed(controller, &stream, &raw);
        let plain_observation = plain.get("observation").cloned();
        close_stream(controller, &stream);
        for (case, description, message) in [
            ("interim-identity", "declares only identity coding", ""),
            ("interim-duplicate-coding", "repeats the coding header", "Duplicate streaming framing header"),
            ("interim-gzip", "declares a compressed coding", "Unsupported streaming content encoding"),
        ] {
            let listening = negotiate(controller, state(&alpha), "mcp_client_listen", root);
            let effect = listening.get("effect").cloned().unwrap_or_else(|| panic!("listen effect"));
            set_stream_case(peer, case);
            let raw = network(&effect);
            set_stream_case(peer, "normal");
            let stream = open_stream(controller, &effect, "mcp_server");
            let part = feed(controller, &stream, &raw);
            if message.is_empty() {
                keep(
                    passed,
                    "an informational head that declares only identity coding leaves the streamed records unchanged",
                    part.get("error").is_none()
                        && plain_observation.is_some()
                        && part.get("observation") == plain_observation.as_ref(),
                );
            } else {
                keep(
                    passed,
                    &format!("an informational head that {description} is refused on the streaming path"),
                    error_is(&part, message) && no_controls(&part),
                );
            }
            close_stream(controller, &stream);
        }
    }
    for (case, tail) in [
        ("framed-length", LENGTH_TAIL),
        ("framed-chunk", CHUNK_TAIL),
        ("truncated-length", &[][..]),
        ("truncated-chunk", &[][..]),
        ("bad-trailer", BAD_CHUNK_TAIL),
        ("close-delimited", &[][..]),
    ] {
        let pending = call_original(controller, state(&alpha), root);
        let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("framed effect"));
        set_post_case(peer, case);
        let raw = network(&effect);
        let stream = open_stream(controller, &effect, protocol_of(&effect));
        if !tail.is_empty() && !raw.ends_with(tail) {
            panic!("{case} response does not end with its framing tail");
        }
        let prefix = if tail.is_empty() { raw.as_slice() } else { &raw[..raw.len() - tail.len()] };
        let observed = feed(controller, &stream, prefix);
        keep(passed, &format!("{case} waits for the declared body or connection close"), observed.get("error").is_none() && is_open(&observed));
        let mut trailer_error = String::new();
        if case == "bad-trailer" {
            let rejected = feed(controller, &stream, &tail[..tail.len() - 1]);
            keep(passed, "a complete forbidden trailer line fails before the final empty line", error_is(&rejected, "Framing trailer forbidden") && is_open(&rejected));
            trailer_error = error_text(&rejected);
        } else if !tail.is_empty() {
            keep(passed, &format!("{case} stays incomplete until the final framing byte"), is_open(&feed(controller, &stream, &tail[..tail.len() - 1])));
            keep(passed, &format!("{case} makes the complete wire available for validation"), is_complete(&feed(controller, &stream, &tail[tail.len() - 1..])));
        }
        let eof = case.starts_with("truncated-") || case == "close-delimited";
        let result = end_stream(controller, &stream, eof);
        if case.starts_with("truncated-") || case == "bad-trailer" {
            keep(passed, &format!("{case} publishes neither a terminal result nor a catalog indication"), result.get("error").is_some() && result.get("value").is_none());
            let message = if case == "bad-trailer" { trailer_error } else { error_text(&result) };
            let failure = fail(controller, state(&pending), &purpose_of(&effect_event(&pending)), &message, true, root);
            let marked = changed(controller, state(&failure), root);
            keep(
                passed,
                &format!("{case} remains a scoped refusal without automatic retry"),
                same_names(state(&failure), &["original"]) && !marked && notice_failed(state(&failure)) && effect_is_null(&failure),
            );
            close_stream(controller, &stream);
        } else {
            let complete = post_deliver(controller, &pending, &envelope_value(&result), None, root);
            let marked = changed(controller, state(&complete), root);
            keep(passed, &format!("{case} publishes the exact correlated result and indication"), output_has(state(&complete), "Reviewed original ran") && marked);
        }
    }
    keep(passed, "ordinary local reads survive incomplete HTTP framing", read_evidence(controller, root));
    let mut invalid_failed = Value::Null;
    for (case, message) in [
        ("control-bad-length", "Invalid response length"),
        ("control-bad-status", "Invalid HTTP status line"),
        ("control-bad-name", "Invalid response header"),
        ("control-bad-value", "Invalid response header"),
        ("control-bad-interim", "Invalid response header"),
        ("control-switching", "Unsupported informational response"),
        ("control-bad-chunk", "Invalid chunk extension"),
        ("control-bad-last-chunk", "Invalid chunk extension"),
    ] {
        let pending = call_original(controller, state(&alpha), root);
        set_post_case(peer, case);
        let before = request_method_count(peer, "control-reply");
        let rejected = post_stream(controller, &pending, passed, Post::feed(8192));
        keep(
            passed,
            &format!("{case} refuses the wire before exporting a control effect"),
            error_is(&rejected, message) && no_controls(&rejected) && is_open(&rejected),
        );
        invalid_failed = fail(controller, state(&pending), &purpose_of(&pending), message, true, root);
        keep(
            passed,
            &format!("{case} retains the catalog without replay or sending a ping reply"),
            same_names(state(&invalid_failed), &["original"])
                && notice_failed(state(&invalid_failed))
                && effect_is_null(&invalid_failed)
                && request_method_count(peer, "control-reply") == before,
        );
    }
    let pending = call_original(controller, state(&invalid_failed), root);
    let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("control-valid effect"));
    set_post_case(peer, "control-valid");
    let raw = network(&effect);
    let stream = open_stream(controller, &effect, protocol_of(&effect));
    let mut acknowledgments = 0usize;
    let mut offset = 0usize;
    while offset < raw.len() {
        let end = (offset + 31).min(raw.len());
        let part = feed(controller, &stream, &raw[offset..end]);
        offset = end;
        if part.get("error").is_some() {
            panic!("control-valid feed: {}", part.to_string());
        }
        let controls = part.get("controls").cloned().unwrap_or_else(|| panic!("control-valid controls: {}", part.to_string()));
        for control in controls.arr() {
            let ack = network(control);
            let split_at = ack.windows(4).position(|window| window == b"\r\n\r\n").unwrap_or_else(|| panic!("ack delimiter"));
            let hint = &ack[..split_at + 4];
            let final_response = &ack[split_at + 4..];
            if !hint.starts_with(b"HTTP/1.1 103") || !final_response.starts_with(b"HTTP/1.1 202") {
                panic!("ack wire {}", String::from_utf8_lossy(&ack));
            }
            let waits = !control_ready(controller, &stream, control, hint) && !control_ready(controller, &stream, control, &ack[..ack.len() - 1]);
            let ready = control_ready(controller, &stream, control, &ack);
            keep(passed, "MCP acknowledgment waits for the final response and its complete delimiter", waits);
            keep(passed, "framed MCP acknowledgment after 103 is ready without EOF", ready);
            control_ack(controller, &stream, control, &ack, false);
            acknowledgments += 1;
        }
    }
    let ended = end_stream(controller, &stream, false);
    let done = post_deliver(controller, &pending, &envelope_value(&ended), None, root);
    keep(
        passed,
        "valid fragmented interim and chunk extensions allow an actual ping reply and result",
        acknowledgments == 1 && last_request(peer).1 == "control-reply" && output_has(state(&done), "Reviewed original ran"),
    );
    keep(passed, "local reads stay available after malformed MCP framing", read_evidence(controller, root));
    let pending = call_original(controller, state(&alpha), root);
    let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("header-limit effect"));
    set_post_case(peer, "control-header-limit");
    let raw = network(&effect);
    let stream = open_stream(controller, &effect, protocol_of(&effect));
    let header_end = second_header_end(&raw);
    let mut offset = 0usize;
    while offset < header_end {
        let end = (offset + 8192).min(header_end);
        let part = feed(controller, &stream, &raw[offset..end]);
        offset = end;
        if part.get("error").is_some() || !is_open(&part) || !no_controls(&part) {
            panic!("header budget feed: {}", part.to_string());
        }
    }
    let part = feed(controller, &stream, &raw[header_end..header_end + 3]);
    keep(
        passed,
        "MCP interim bytes and a pending delimiter do not consume the final header budget",
        part.get("error").is_none() && is_open(&part) && no_controls(&part),
    );
    let part = feed(controller, &stream, &raw[header_end + 3..]);
    keep(
        passed,
        "the exact-limit MCP header releases one real ping after its final delimiter byte",
        part.get("error").is_none() && is_open(&part) && part.get("controls").map(|value| value.arr().len()) == Some(1),
    );
    let control = part.get("controls").unwrap().arr()[0].clone();
    let ack_bytes = network(&control);
    let ack = control_ack(controller, &stream, &control, &ack_bytes, true);
    let ended = end_stream(controller, &stream, false);
    let done = post_deliver(controller, &pending, &envelope_value(&ended), None, root);
    keep(
        passed,
        "the exact-limit split MCP header preserves the acknowledged result",
        is_complete(&ack) && last_request(peer).1 == "control-reply" && output_has(state(&done), "Reviewed original ran"),
    );
    for (case, tail) in [("control-boundary-invalid", &b"\rX"[..]), ("control-boundary-valid", &b"\r\n0\r\n\r\n"[..])] {
        let pending = call_original(controller, state(&alpha), root);
        let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("boundary effect"));
        set_post_case(peer, case);
        let raw = network(&effect);
        if !raw.ends_with(tail) {
            panic!("{case} response does not end with its delimiter");
        }
        let stream = open_stream(controller, &effect, protocol_of(&effect));
        let part = feed(controller, &stream, &raw[..raw.len() - tail.len()]);
        keep(passed, &format!("{case} retains control data until the chunk delimiter arrives"), part.get("error").is_none() && is_open(&part) && no_controls(&part));
        let part = feed(controller, &stream, &tail[..1]);
        keep(passed, &format!("{case} retains control data after only the first delimiter byte"), part.get("error").is_none() && is_open(&part) && no_controls(&part));
        let part = feed(controller, &stream, &tail[1..2]);
        if case == "control-boundary-invalid" {
            keep(
                passed,
                "split invalid delimiter refuses the stream without exporting a reply",
                error_is(&part, "Invalid stream chunk terminator") && no_controls(&part) && is_open(&part),
            );
            let finish = end_stream(controller, &stream, true);
            keep(passed, "invalid chunk delimiter prevents adoption of the terminal result", finish.get("error").is_some());
            let failed = fail(controller, state(&pending), &purpose_of(&effect_event(&pending)), "Invalid stream chunk terminator", true, root);
            keep(
                passed,
                "invalid delimiter remains a scoped failure and retains the catalog",
                same_names(state(&failed), &["original"]) && notice_failed(state(&failed)) && effect_is_null(&failed),
            );
            close_stream(controller, &stream);
            continue;
        }
        keep(
            passed,
            "valid split delimiter exports the ping once across two data chunks",
            part.get("error").is_none() && is_open(&part) && part.get("controls").map(|value| value.arr().len()) == Some(1),
        );
        let control = part.get("controls").unwrap().arr()[0].clone();
        let ack_bytes = network(&control);
        let acknowledgment = control_ack(controller, &stream, &control, &ack_bytes, true);
        keep(passed, "valid ping acknowledgment still waits for the final HTTP chunk", is_open(&acknowledgment));
        let part = feed(controller, &stream, &tail[2..]);
        keep(passed, "final chunk completes the owned stream without a second ping reply", part.get("error").is_none() && is_complete(&part) && no_controls(&part));
        let ended = end_stream(controller, &stream, false);
        let done = post_deliver(controller, &pending, &envelope_value(&ended), None, root);
        keep(
            passed,
            "valid fragmented chunk boundaries retain the exact tool result",
            output_has(state(&done), "Reviewed original ran") && last_request(peer).1 == "control-reply",
        );
    }
    keep(passed, "ordinary local reads survive split invalid chunk delimiters", read_evidence(controller, root));
}

fn effect_event(event: &Value) -> &Value {
    event
}

fn register(controller: &mut Controller, st: &Value, path: &str, initial: bool, peer: &Peer, root: &str) -> Value {
    let action = if initial { "mcp_client_settings" } else { "mcp_client_new" };
    let opened = controller.event(Some(st), action, obj(vec![]), root);
    let endpoint = format!("http://127.0.0.1:{}{path}", peer.port);
    controller.event(
        Some(state(&opened)),
        "mcp_client_save",
        obj(vec![("endpoint".into(), s(&endpoint)), ("key".into(), s(&format!("catalog{path}"))), ("access".into(), Value::Bool(true))]),
        root,
    )
}

fn discover(controller: &mut Controller, st: &Value, root: &str) -> Value {
    let listed = negotiate(controller, st, "mcp_client_list", root);
    exchange(controller, &listed, root)
}

fn negotiate(controller: &mut Controller, st: &Value, action: &str, root: &str) -> Value {
    let started = controller.event(Some(st), action, obj(vec![]), root);
    let initialized = exchange(controller, &started, root);
    exchange(controller, &initialized, root)
}

fn exchange(controller: &mut Controller, event: &Value, root: &str) -> Value {
    let effect = event.get("effect").unwrap_or_else(|| panic!("no network effect: {}", event.to_string()));
    if effect == &Value::Null {
        panic!("null network effect: {}", event.to_string());
    }
    let purpose = purpose_of(event);
    let data = b64_encode(&network(effect));
    controller.event(Some(state(event)), "network_result", obj(vec![("purpose".into(), s(&purpose)), ("data".into(), s(&data))]), root)
}

fn call_original(controller: &mut Controller, st: &Value, root: &str) -> Value {
    let tools = controller.event(Some(st), "mcp_tools", obj(vec![]), root);
    let chosen = controller.event(Some(state(&tools)), "mcp_tool:0", obj(vec![]), root);
    let reviewed = controller.event(Some(state(&chosen)), "mcp_client_review", obj(vec![("arguments".into(), s("{}"))]), root);
    if !effect_is_null(&reviewed) {
        panic!("review dispatched: {}", reviewed.to_string());
    }
    let approved = controller.event(Some(state(&reviewed)), "mcp_client_approve", obj(vec![]), root);
    let initialized = exchange(controller, &approved, root);
    exchange(controller, &initialized, root)
}

fn network(effect: &Value) -> Vec<u8> {
    if effect.get("kind").map(Value::str_) != Some("network") {
        panic!("network effect: {}", effect.to_string());
    }
    let host = effect.get("host").unwrap_or_else(|| panic!("host")).str_();
    let port = match effect.get("port").unwrap_or_else(|| panic!("port")) {
        Value::Num(token) | Value::Str(token) => token.parse::<u16>().unwrap_or_else(|_| panic!("port {token}")),
        other => panic!("port {}", other.to_string()),
    };
    let wire = effect.get("wire").unwrap_or_else(|| panic!("wire")).str_();
    let mut stream = TcpStream::connect((host, port)).unwrap_or_else(|error| panic!("network connect: {error}"));
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).unwrap_or_else(|error| panic!("write timeout: {error}"));
    stream.write_all(wire.as_bytes()).unwrap_or_else(|error| panic!("network write: {error}"));
    let mut out = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buffer[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("network read: {error}"),
        }
    }
    out
}

fn protocol_of(effect: &Value) -> &str {
    effect.get("stream_protocol").map(Value::str_).unwrap_or_else(|| panic!("stream protocol: {}", effect.to_string()))
}

fn open_stream(controller: &mut Controller, effect: &Value, protocol: &str) -> String {
    let control = effect.get("mcp_control").cloned().unwrap_or_else(|| panic!("mcp control: {}", effect.to_string()));
    let reply = controller.op(&obj(vec![
        ("op".into(), s("stream_open")),
        ("protocol".into(), s(protocol)),
        ("mcp_control".into(), control),
    ]));
    match reply {
        Value::Num(id) => id,
        other => panic!("stream id: {}", other.to_string()),
    }
}

fn feed(controller: &mut Controller, stream: &str, data: &[u8]) -> Value {
    controller.op(&obj(vec![
        ("op".into(), s("stream_feed")),
        ("id".into(), twin::n(stream)),
        ("data".into(), s(&b64_encode(data))),
    ]))
}

fn end_stream(controller: &mut Controller, stream: &str, eof: bool) -> Value {
    controller.send(&obj(vec![
        ("op".into(), s("stream_finish")),
        ("id".into(), twin::n(stream)),
        ("eof".into(), Value::Bool(eof)),
    ]))
}

fn close_stream(controller: &mut Controller, stream: &str) {
    controller.op(&obj(vec![("op".into(), s("stream_close")), ("id".into(), twin::n(stream))]));
}

fn envelope_value(envelope: &Value) -> Value {
    envelope.get("value").cloned().unwrap_or_else(|| panic!("stream finish: {}", envelope.to_string()))
}

fn post_stream(controller: &mut Controller, pending: &Value, passed: &mut Vec<String>, flags: Post) -> Value {
    let effect = pending.get("effect").cloned().unwrap_or_else(|| panic!("post effect: {}", pending.to_string()));
    let stream = open_stream(controller, &effect, protocol_of(&effect));
    let raw = network(&effect);
    let parts = fragments(&raw, flags);
    for (index, fragment) in parts.iter().enumerate() {
        let part = feed(controller, &stream, fragment);
        if flags.split_terminal && index == 0 {
            keep(
                passed,
                "first correlated reply does not end a partially received declared body",
                is_open(&part) && part.get("error").is_none(),
            );
        }
        if part.get("error").is_some() {
            if !flags.feed_error {
                panic!("unexpected feed error: {}", part.to_string());
            }
            let finish = end_stream(controller, &stream, true);
            if finish.get("error").is_none() {
                panic!("stream finish unexpectedly succeeded: {}", finish.to_string());
            }
            close_stream(controller, &stream);
            return part;
        }
    }
    if flags.feed_error {
        panic!("expected a feed error");
    }
    let finish = end_stream(controller, &stream, true);
    if flags.finish_error {
        if finish.get("error").is_none() {
            panic!("expected finish error: {}", finish.to_string());
        }
        close_stream(controller, &stream);
        return finish;
    }
    envelope_value(&finish)
}

fn fragments(raw: &[u8], flags: Post) -> Vec<Vec<u8>> {
    if flags.split_terminal {
        let needle = b"id: event-2";
        let boundary = raw.windows(needle.len()).position(|window| window == needle).unwrap_or_else(|| panic!("terminal event boundary"));
        return vec![raw[..boundary].to_vec(), raw[boundary..].to_vec()];
    }
    let size = if flags.whole { raw.len().max(1) } else { flags.read_size.max(1) };
    raw.chunks(size).map(|chunk| chunk.to_vec()).collect()
}

fn post_deliver(controller: &mut Controller, pending: &Value, result: &Value, purpose: Option<&str>, root: &str) -> Value {
    let purpose = purpose.map(str::to_string).unwrap_or_else(|| purpose_of(pending));
    let mut callback = match result {
        Value::Obj(_) => result.clone(),
        other => panic!("result is not an object: {}", other.to_string()),
    };
    put(&mut callback, "purpose", s(&purpose));
    controller.event(Some(state(pending)), "network_result", callback, root)
}

fn open_get(controller: &mut Controller, event: &Value) -> (String, Value) {
    let effect = event.get("effect").cloned().unwrap_or_else(|| panic!("GET effect"));
    let raw = network(&effect);
    let stream = open_stream(controller, &effect, "mcp_server");
    let data = feed(controller, &stream, &raw);
    if data.get("error").is_some() {
        panic!("GET stream failed: {}", data.to_string());
    }
    let observation = data.get("observation").cloned().unwrap_or_else(|| panic!("GET observation: {}", data.to_string()));
    (stream, observation)
}

fn finish_get(controller: &mut Controller, event: &Value, stream: &str, root: &str, suspend: bool) -> Value {
    let op = if suspend { "stream_suspend" } else { "stream_finish" };
    let purpose = purpose_of(event);
    let reply = controller.op(&obj(vec![
        ("op".into(), s(op)),
        ("id".into(), twin::n(stream)),
        ("eof".into(), Value::Bool(true)),
        ("owner".into(), s(&purpose)),
    ]));
    let data = reply.get("data").unwrap_or_else(|| panic!("stream summary: {}", reply.to_string())).str_().to_string();
    controller.event(Some(state(event)), "network_result", obj(vec![("purpose".into(), s(&purpose)), ("data".into(), s(&data))]), root)
}

fn progress(controller: &mut Controller, event: &Value, counts: &Value, purpose: Option<&str>, root: &str) -> Value {
    let purpose = purpose.map(str::to_string).unwrap_or_else(|| purpose_of(event));
    controller.event(
        Some(state(event)),
        "mcp_client_server_progress",
        obj(vec![("purpose".into(), s(&purpose)), ("counts".into(), counts.clone())]),
        root,
    )
}

fn select_index(controller: &mut Controller, st: &Value, index: usize, root: &str) -> Value {
    let listed = controller.event(Some(st), "mcp_client_connections", obj(vec![]), root);
    controller.event(Some(state(&listed)), &format!("mcp_client_select:{index}"), obj(vec![]), root)
}

fn select_path(controller: &mut Controller, st: &Value, path: &str, root: &str) -> Value {
    let connections = st.get("mcp_saved_connections").unwrap_or_else(|| panic!("saved connections")).arr();
    let index = connections
        .iter()
        .position(|config| config.get("endpoint").map(Value::str_).unwrap_or("").ends_with(path))
        .unwrap_or_else(|| panic!("saved connection {path}"));
    select_index(controller, st, index, root)
}

fn control_ack(controller: &mut Controller, stream: &str, control: &Value, data: &[u8], eof: bool) -> Value {
    controller.op(&obj(vec![
        ("op".into(), s("stream_control_ack")),
        ("id".into(), twin::n(stream)),
        ("owner".into(), control.get("owner").cloned().unwrap_or_else(|| panic!("control owner"))),
        ("ticket".into(), control.get("ticket").cloned().unwrap_or_else(|| panic!("control ticket"))),
        ("data".into(), s(&b64_encode(data))),
        ("eof".into(), Value::Bool(eof)),
    ]))
}

fn control_ready(controller: &mut Controller, stream: &str, control: &Value, data: &[u8]) -> bool {
    let reply = controller.op(&obj(vec![
        ("op".into(), s("stream_control_probe")),
        ("id".into(), twin::n(stream)),
        ("owner".into(), control.get("owner").cloned().unwrap_or_else(|| panic!("control owner"))),
        ("ticket".into(), control.get("ticket").cloned().unwrap_or_else(|| panic!("control ticket"))),
        ("data".into(), s(&b64_encode(data))),
    ]));
    reply.get("ready") == Some(&Value::Bool(true))
}

fn fail(controller: &mut Controller, st: &Value, purpose: &str, error: &str, sent: bool, root: &str) -> Value {
    let mut inputs = vec![("purpose".into(), s(purpose)), ("error".into(), s(error))];
    if sent {
        inputs.push(("sent".into(), Value::Bool(true)));
    }
    controller.event(Some(st), "failure", obj(inputs), root)
}

fn read_evidence(controller: &mut Controller, root: &str) -> bool {
    let read = controller.op(&obj(vec![
        ("op".into(), s("tool")),
        ("tool".into(), s("read")),
        ("root".into(), s(root)),
        ("args".into(), obj(vec![("path".into(), s("evidence.txt"))])),
    ]));
    read.str_() == "local evidence"
}

fn expected_reply(pending: &Value) -> Value {
    let id = pending
        .get("effect")
        .and_then(|effect| effect.get("mcp_control"))
        .and_then(|control| control.get("request_id"))
        .cloned()
        .unwrap_or_else(|| panic!("request id"));
    obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), id),
        (
            "result".into(),
            obj(vec![(
                "content".into(),
                Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s("Reviewed original ran"))])]),
            )]),
        ),
    ])
}

fn http_body_json(encoded: &str) -> Value {
    let raw = b64_decode(encoded);
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n").unwrap_or_else(|| panic!("response delimiter"));
    let body = std::str::from_utf8(&raw[split + 4..]).unwrap_or_else(|error| panic!("body utf8: {error}"));
    Value::parse(body)
}

fn second_header_end(raw: &[u8]) -> usize {
    let first = raw.windows(4).position(|window| window == b"\r\n\r\n").unwrap_or_else(|| panic!("first header"));
    let second = raw[first + 4..].windows(4).position(|window| window == b"\r\n\r\n").unwrap_or_else(|| panic!("second header"));
    first + 4 + second
}

fn chunk_split(raw: &[u8]) -> usize {
    let body_start = raw.windows(4).position(|window| window == b"\r\n\r\n").unwrap_or_else(|| panic!("chunk body")) + 4;
    let line_end = raw[body_start..].windows(2).position(|window| window == b"\r\n").unwrap_or_else(|| panic!("chunk size")) + body_start;
    let size = usize::from_str_radix(std::str::from_utf8(&raw[body_start..line_end]).unwrap_or_else(|error| panic!("chunk size utf8: {error}")), 16)
        .unwrap_or_else(|error| panic!("chunk size: {error}"));
    line_end + 2 + size + 2
}

fn purpose_of(event: &Value) -> String {
    match event.get("effect").and_then(|effect| effect.get("purpose")) {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("purpose missing: {}", event.to_string()),
    }
}

fn names(st: &Value) -> Vec<String> {
    st.get("mcp_client_catalog")
        .map(|value| value.arr().iter().map(|row| row.get("name").unwrap().str_().to_string()).collect())
        .unwrap_or_default()
}

fn same_names(st: &Value, expected: &[&str]) -> bool {
    let got = names(st);
    got.len() == expected.len() && got.iter().zip(expected.iter()).all(|(got, expected)| got == expected)
}

fn changed(controller: &mut Controller, st: &Value, root: &str) -> bool {
    let dialog = controller.event(Some(st), "mcp_tools", obj(vec![]), root);
    dialog
        .get("ui")
        .and_then(|ui| ui.get("modal"))
        .and_then(|modal| modal.get("buttons"))
        .map(|buttons| {
            buttons.arr().iter().any(|button| {
                button.get("label").map(Value::str_) == Some("Refresh changed catalog") && button.get("event").map(Value::str_) == Some("mcp_client_list")
            })
        })
        .unwrap_or(false)
}

fn refresh_button(event: &Value) -> bool {
    event
        .get("ui")
        .and_then(|ui| ui.get("modal"))
        .and_then(|modal| modal.get("buttons"))
        .map(|buttons| buttons.arr().iter().any(|button| button.get("event").map(Value::str_) == Some("mcp_client_list")))
        .unwrap_or(false)
}

fn unmarked(st: &Value) -> bool {
    match st.get("mcp_catalog_changes") {
        None | Some(Value::Null) => true,
        Some(Value::Obj(fields)) => fields.is_empty(),
        Some(_) => false,
    }
}

fn catalog_flagged(st: &Value) -> bool {
    match st.get("mcp_catalog_changes") {
        Some(Value::Obj(fields)) => fields.iter().any(|(_, value)| match value {
            Value::Bool(true) => true,
            Value::Null | Value::Bool(false) => false,
            Value::Str(text) => !text.is_empty(),
            Value::Num(token) => token != "0",
            Value::Arr(items) => !items.is_empty(),
            Value::Obj(inner) => !inner.is_empty(),
        }),
        _ => false,
    }
}

fn notice_failed(st: &Value) -> bool {
    match st.get("notice_error") {
        Some(Value::Bool(value)) => *value,
        Some(Value::Str(text)) => !text.is_empty(),
        Some(Value::Num(token)) => token != "0",
        Some(Value::Arr(items)) => !items.is_empty(),
        Some(Value::Obj(fields)) => !fields.is_empty(),
        Some(Value::Null) | None => false,
    }
}

fn no_tool_changes(observation: &Value) -> bool {
    match observation.get("tool_changes") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::Num(token)) => token == "0",
        Some(_) => false,
    }
}

fn no_controls(part: &Value) -> bool {
    match part.get("controls") {
        None | Some(Value::Null) => true,
        Some(Value::Arr(items)) => items.is_empty(),
        Some(_) => false,
    }
}

fn is_complete(part: &Value) -> bool {
    part.get("complete") == Some(&Value::Bool(true))
}

fn is_open(part: &Value) -> bool {
    part.get("complete") == Some(&Value::Bool(false))
}

fn error_is(part: &Value, message: &str) -> bool {
    matches!(part.get("error"), Some(Value::Str(text)) if text == message)
}

fn error_text(part: &Value) -> String {
    match part.get("error") {
        Some(Value::Str(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// A field the product no longer holds.
fn released(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Str(text)) => text.is_empty(),
        _ => false,
    }
}

/// The product is not waiting for an operation to finish.
fn idle(st: &Value) -> bool {
    !matches!(st.get("busy"), Some(Value::Bool(true)))
}

fn output_has(st: &Value, needle: &str) -> bool {
    match st.get("output") {
        Some(Value::Str(text)) => text.contains(needle),
        _ => false,
    }
}

fn effect_is_null(event: &Value) -> bool {
    event.get("effect") == Some(&Value::Null)
}

fn with_field(value: &Value, key: &str, next: Value) -> Value {
    let mut copy = value.clone();
    put(&mut copy, key, next);
    copy
}

fn with_state(event: &Value, replacement: &Value) -> Value {
    with_field(event, "state", replacement.clone())
}

fn set_nested(event: &mut Value, object_key: &str, field: &str, next: Value) {
    let st = state_mut(event);
    let mut object = st.get(object_key).cloned().unwrap_or_else(|| panic!("missing {object_key}"));
    put(&mut object, field, next);
    put(st, object_key, object);
}

fn set_refresh(peer: &Peer, value: &str) {
    peer.state.lock().unwrap().refresh = value.to_string();
}

fn set_stream_case(peer: &Peer, value: &str) {
    peer.state.lock().unwrap().stream_case = value.to_string();
}

fn set_post_case(peer: &Peer, value: &str) {
    peer.state.lock().unwrap().post_case = value.to_string();
}

fn request_count(peer: &Peer) -> usize {
    peer.state.lock().unwrap().requests.len()
}

fn request_method_count(peer: &Peer, method: &str) -> usize {
    peer.state.lock().unwrap().requests.iter().filter(|(_, item, _)| item == method).count()
}

fn last_request(peer: &Peer) -> (String, String, String) {
    peer.state.lock().unwrap().requests.last().cloned().unwrap_or_default()
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

// A check that reports what the product actually said when it does not hold.
// The literal text belongs in the panic, so a failure names the refusal the
// product produced instead of only the check's own name.
fn keep_detail(passed: &mut Vec<String>, name: &str, ok: bool, detail: String) {
    if !ok {
        panic!("{name}: {detail}");
    }
    passed.push(name.to_string());
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn state_mut(value: &mut Value) -> &mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == "state") {
        return &mut fields[index].1;
    }
    fields.push(("state".into(), Value::Null));
    &mut fields.last_mut().unwrap().1
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

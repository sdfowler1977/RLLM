//! Native twin of verification/http_framing.py.
//! Real loopback sockets carry response bytes into the product framing API.
mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner public-API and real loopback persistent-socket framing; actual complete wire bytes, gzip preservation, EOF ownership, bounds, malformed input and scoped session failures.";
const FRAME_LIMIT: usize = 1024 * 1024;

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let mut passed = Vec::new();
    run(&mut controller, &root_text, &mut passed);
    controller.finish();
    twin::report_value("http_framing", &twin::obj(vec![
        ("passed".into(), twin::Value::Arr(passed.iter().map(|name| twin::s(name)).collect())),
        ("scope".into(), twin::s(SCOPE)),
    ]), true);
    println!("{} http_framing checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let cases: [(&str, &[u8], u16, bool); 9] = [
        ("length", b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: 5\r\n\r\nhello", 200, false),
        ("chunked", b"HTTP/1.1 207 Multi-Status\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nResult: done\r\n\r\n", 207, false),
        ("interim", b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n", 201, false),
        ("bodyless", b"HTTP/1.1 204 No Content\r\n\r\n", 204, false),
        ("unchanged length metadata", b"HTTP/1.1 304 Not Modified\r\nContent-Length: 5000000\r\n\r\n", 304, false),
        ("unchanged chunk metadata", b"HTTP/1.1 304 Not Modified\r\nTransfer-Encoding: chunked\r\n\r\n", 304, false),
        ("unchanged coded metadata", b"HTTP/1.1 304 Not Modified\r\nContent-Encoding: gzip\r\nContent-Length: 5000000\r\n\r\n", 304, false),
        ("head chunk metadata", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n", 200, true),
        ("head", b"HTTP/1.1 200 OK\r\nContent-Length: 5000000\r\n\r\n", 200, true),
    ];
    for (label, raw, status, head) in cases {
        let name = format!("{label} completes before the persistent peer closes");
        let (actual, result) = persistent(controller, raw, head, None, None, None).unwrap_or_else(|error| panic!("{name}: {error}"));
        keep_msg(passed, &name, actual == raw && same_meta(&result, status, raw.len()), || exchange_detail(&actual, raw, &result));
    }

    let raw = cat(&[b"HTTP/1.1 207 Multi-Status\r\nContent-Length: 90000\r\n\r\n", &vec![b'x'; 90000]]);
    let name = "large WebDAV207 completes within the response bound";
    let (actual, result) = persistent(controller, &raw, false, None, None, None).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, actual == raw && status_is(&result, 207), || exchange_detail(&actual, &raw, &result));

    let body = twin::gzip_store(b"<html><body>GZIP_FRAME</body></html>");
    let raw = cat(&[b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: ", body.len().to_string().as_bytes(), b"\r\n\r\n", &body]);
    let name = "framing preserves coded response bytes";
    let (actual, _) = persistent(controller, &raw, false, None, None, None).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, actual == raw, || format!("actual {} raw {}", actual.len(), raw.len()));

    let raw = b"HTTP/1.1 200 OK\r\n\r\nclose-delimited";
    let id = open_frame(controller, "response", FRAME_LIMIT, false);
    let reply = feed_ok(controller, id, raw, "response", "close-delimited feed");
    keep(passed, "close-delimited response waits for observed EOF", is_false(&reply, "ready") && finish(controller, id, true, "response").is_ok());
    let id = open_frame(controller, "response", FRAME_LIMIT, false);
    feed_ok(controller, id, raw, "response", "close-delimited second feed");
    keep(passed, "unobserved EOF cannot settle close-delimited response", finish(controller, id, false, "response").is_err());

    for (label, raw) in [
        ("short length", b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\na".as_slice()),
        ("unfinished chunk", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\na".as_slice()),
        ("missing trailer terminator", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nOne: two\r\n".as_slice()),
    ] {
        let id = open_frame(controller, "response", FRAME_LIMIT, false);
        feed_ok(controller, id, raw, "response", label);
        keep(passed, &format!("{label} remains a scoped incomplete response"), finish(controller, id, true, "response").is_err());
    }

    for (label, raw) in [
        ("duplicate length", b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n".as_slice()),
        ("ambiguous framing", b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice()),
        ("bodyless extra bytes", b"HTTP/1.1 304 Not Modified\r\nContent-Length: 1\r\n\r\nx".as_slice()),
        ("extra bytes", b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nab".as_slice()),
        ("framing trailer", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 0\r\n\r\n".as_slice()),
    ] {
        let id = open_frame(controller, "response", FRAME_LIMIT, false);
        keep(passed, &format!("{label} is refused locally"), feed(controller, id, raw, "response").is_err());
        keep(passed, &format!("{label} retires only its failed session"), !close_frame(controller, id, "response"));
    }

    for header in [
        b"Content-Length: 5000000\r\n".as_slice(),
        b"Transfer-Encoding: chunked\r\n".as_slice(),
        b"Content-Encoding: gzip\r\nContent-Length: 5000000\r\n".as_slice(),
    ] {
        let wire = cat(&[b"HTTP/1.1 304 Not Modified\r\n", header, b"\r\n"]);
        let reply = call(controller, obj(vec![("op".into(), s("network_binary")), ("data".into(), s(&b64_encode(&wire)))]));
        let name = format!("downstream bodyless metadata preserves its actual304 diagnostic {}", header_name(header));
        keep_msg(passed, &name, error_text(&reply).as_deref() == Some("HTTP 304: server returned representation metadata."), || reply.to_string());
    }

    let id = open_frame(controller, "response", FRAME_LIMIT, false);
    let foreign = feed(controller, id, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", "foreign");
    let intact = foreign.is_err()
        && !close_frame(controller, id, "foreign")
        && feed(controller, id, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", "response").is_ok()
        && finish(controller, id, false, "response").is_ok();
    keep(passed, "foreign owner leaves the actual response session intact", intact);

    let id = open_frame(controller, "response", 64, false);
    keep(passed, "declared response above its current bound is refused", feed(controller, id, b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n", "response").is_err());
    let id = open_frame(controller, "response", 32, false);
    keep(passed, "wire bytes above current bound are refused", feed(controller, id, &[b'x'; 33], "response").is_err());

    let delimiter = b"\r\n\r\n";
    let trailer_prefix = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n";
    let valid = cat(&[trailer_prefix, &trailer_of_size(16384)]);
    let name = "the exact 16 KiB trailer section completes after its split final LF";
    let (actual, metadata) = persistent(controller, &valid, false, None, None, Some(valid.len() - 1)).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, actual == valid && same_meta(&metadata, 200, valid.len()), || exchange_detail(&actual, &valid, &metadata));

    for suffix in [trailer_of_size(16385), cat(&[b"X-Pad: ", &vec![b'x'; 16378]])] {
        let wire = cat(&[trailer_prefix, &suffix]);
        let id = open_frame(controller, "response", FRAME_LIMIT, false);
        let mut last = String::new();
        let mut failed = false;
        for chunk in wire.chunks(8192) {
            match feed(controller, id, chunk, "response") {
                Ok(_) => failed = false,
                Err(error) => {
                    failed = true;
                    last = error;
                }
            }
        }
        keep_msg(
            passed,
            "an overlong trailer section retires only the affected owned response",
            failed && last == "Trailers exceed bound" && !close_frame(controller, id, "response"),
            || format!("failed={failed} last={last}"),
        );
    }

    let id = open_frame(controller, "response", FRAME_LIMIT, false);
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n";
    let mut saw_ready = false;
    for byte in raw {
        let reply = feed_ok(controller, id, &[*byte], "response", "one-byte fragment");
        saw_ready = is_true(&reply, "ready");
    }
    keep(passed, "one-byte response fragmentation preserves the complete frame", saw_ready && finish(controller, id, false, "response").is_ok());

    let (raw, split) = exact_limit_response(b"header limit payload", b"text/plain");
    let name = "an exact-limit final header split after CR completes before peer close";
    let (actual, result) = persistent(controller, &raw, false, None, None, Some(split)).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, actual == raw && same_meta(&result, 200, raw.len()), || exchange_detail(&actual, &raw, &result));

    let fields = b"HTTP/1.1 200 OK\r\nContent-Length: 0".as_slice();
    for (size, partial) in [(65536usize, 1usize), (65536, 2), (65536, 3), (65535, 2), (65534, 3)] {
        let id = open_frame(controller, "response", FRAME_LIMIT, false);
        let head = padded_header(fields, size);
        let primed = feed_ok(controller, id, &head, "response", "padded header");
        if !is_false(&primed, "ready") {
            panic!("{size}-byte header was ready before its delimiter: {}", primed.to_string());
        }
        let pending = feed_ok(controller, id, &delimiter[..partial], "response", "partial delimiter");
        keep(passed, &format!("{size}-byte header remains pending after {partial} delimiter bytes"), is_false(&pending, "ready"));
        let completed = feed_ok(controller, id, &delimiter[partial..], "response", "rest of delimiter");
        let settled = finish(controller, id, false, "response").unwrap_or_else(|error| panic!("{size}-byte header finish: {error}"));
        keep_msg(
            passed,
            &format!("{size}-byte header completes with the rest of its delimiter"),
            is_true(&completed, "ready") && same_meta(&settled, 200, size + 4),
            || format!("feed {} finish {}", completed.to_string(), settled.to_string()),
        );
    }

    let interim = padded_header(b"HTTP/1.1 103 Early Hints\r\nLink: </resource>", 65536);
    let raw = cat(&[&interim, delimiter, b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n"]);
    let name = "an exact-limit split informational header preserves the final response";
    let (actual, result) = persistent(controller, &raw, false, None, None, Some(interim.len() + 3)).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, actual == raw && same_meta(&result, 201, raw.len()), || exchange_detail(&actual, &raw, &result));

    let stream_body = b"data: {\"choices\":[{\"delta\":{\"content\":\"BOUNDARY_OK\"},\"finish_reason\":\"stop\",\"index\":0}]}\n\ndata: [DONE]\n\n";
    let stream_fields = cat(&[b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: ", stream_body.len().to_string().as_bytes()]);
    let stream_interim = b"HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n";
    for size in [65535usize, 65536] {
        let id = stream_open(controller, "chat");
        let pending = cat(&[stream_interim, &padded_header(&stream_fields, size)]);
        let mut part = Value::Null;
        for chunk in pending.chunks(8192) {
            part = stream_feed(controller, id, chunk);
            if !stream_quiet(&part) {
                panic!("{size}-byte model header fragment was not quiet: {}", part.to_string());
            }
        }
        keep(passed, &format!("consumed interim bytes do not count against a {size}-byte model header"), is_false(&part, "complete"));
        for byte in &delimiter[..3] {
            part = stream_feed(controller, id, &[*byte]);
            if !stream_quiet(&part) {
                panic!("{size}-byte model delimiter fragment was not quiet: {}", part.to_string());
            }
        }
        keep(passed, &format!("model header of {size} bytes waits for the last delimiter byte"), is_false(&part, "complete"));
        part = stream_feed(controller, id, &cat(&[&delimiter[3..], stream_body]));
        let settled = stream_finish(controller, id, false).unwrap_or_else(|error| panic!("{size}-byte model finish: {error}"));
        keep_msg(
            passed,
            &format!("split model header of {size} bytes preserves its real stream result"),
            is_true(&part, "complete") && delta_eq(&part, "BOUNDARY_OK") && data_contains(&settled, b"BOUNDARY_OK"),
            || format!("part {} settled {}", part.to_string(), settled.to_string()),
        );
    }

    let id = stream_open(controller, "chat");
    let primed = stream_feed(controller, id, &padded_header(&stream_fields, 65536));
    if !is_false(&primed, "complete") {
        panic!("overlong model header prefix completed: {}", primed.to_string());
    }
    let part = stream_feed(controller, id, &cat(&[b"x", delimiter, stream_body]));
    keep_msg(
        passed,
        "overlong streaming model header cannot expose text or complete",
        error_text(&part).as_deref() == Some("HTTP headers exceed limit") && delta_eq(&part, "") && is_false(&part, "complete"),
        || part.to_string(),
    );
    keep(passed, "an overlong streaming model header retires its frame", stream_finish(controller, id, true).is_err());

    for (protocol, first, last) in [
        (
            "chat",
            b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"EARLIER\"},\"finish_reason\":null}]}\n\n".as_slice(),
            b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"LATER\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".as_slice(),
        ),
        (
            "local",
            b"{\"message\":{\"content\":\"EARLIER\"},\"done\":false}\n".as_slice(),
            b"{\"message\":{\"content\":\"LATER\"},\"done\":true}\n".as_slice(),
        ),
    ] {
        let response_head = b"HTTP/1.1 200 OK\r\n";
        let body = cat(&[first, last]);
        let length = body.len().to_string();
        let no_content = b"HTTP/1.1 204 No Content\r\n";
        let payloads = [
            ("body without length", cat(&[b"\r\n", &body])),
            ("nonzero declared length", cat(&[b"Content-Length: ", length.as_bytes(), b"\r\n\r\n"])),
            ("chunked framing", b"Transfer-Encoding: chunked\r\n\r\n".to_vec()),
        ];
        for (label, payload) in payloads {
            let id = stream_open(controller, protocol);
            let part = stream_feed(controller, id, &cat(&[no_content, &payload]));
            keep_msg(
                passed,
                &format!("{protocol} 204 {label} exposes no text and cannot complete"),
                error_text(&part).as_deref() == Some("Body on bodyless response") && delta_eq(&part, "") && is_false(&part, "complete"),
                || part.to_string(),
            );
            keep(passed, &format!("{protocol} 204 {label} retires the affected stream"), stream_finish(controller, id, true).is_err());
        }
        for (label, field) in [("implicit empty body", b"".as_slice()), ("explicit zero length", b"Content-Length: 0\r\n".as_slice())] {
            let id = stream_open(controller, protocol);
            let part = stream_feed(controller, id, &cat(&[no_content, field, b"\r\n"]));
            keep_msg(
                passed,
                &format!("{protocol} 204 {label} completes framing without model output"),
                error_text(&part).is_none() && is_true(&part, "complete") && delta_eq(&part, ""),
                || part.to_string(),
            );
            let final_reply = stream_finish(controller, id, false);
            keep_msg(
                passed,
                &format!("{protocol} 204 {label} reports the missing model reply"),
                matches!(final_reply, Err(ref text) if text == "Connection ended before the model completed its reply"),
                || match &final_reply {
                    Ok(value) => value.to_string(),
                    Err(error) => error.clone(),
                },
            );
        }
        let id = stream_open(controller, protocol);
        let opened = stream_feed(controller, id, &cat(&[no_content, b"\r\n"]));
        if !is_true(&opened, "complete") {
            panic!("{protocol} 204 empty framing did not complete: {}", opened.to_string());
        }
        let part = stream_feed(controller, id, &body);
        keep_msg(
            passed,
            &format!("{protocol} 204 later body bytes are refused before output"),
            error_text(&part).as_deref() == Some("Body on bodyless response") && delta_eq(&part, "") && is_false(&part, "complete"),
            || part.to_string(),
        );
        keep(passed, &format!("{protocol} 204 later body bytes retire the affected stream"), stream_finish(controller, id, true).is_err());

        let mut overflow_body = body.clone();
        overflow_body.push(b'x');
        let mut ambiguous = format!("{:x}\r\n", body.len()).into_bytes();
        ambiguous.extend_from_slice(&body);
        ambiguous.extend_from_slice(b"\r\n0\r\n\r\n");
        let cases = vec![
            ("zero-length overflow", b"Content-Length: 0\r\n".to_vec(), body.clone(), "Streaming response exceeds Content-Length"),
            ("nonzero overflow", cat(&[b"Content-Length: ", length.as_bytes(), b"\r\n"]), overflow_body, "Streaming response exceeds Content-Length"),
            ("duplicate length", cat(&[b"Content-Length: 0\r\ncontent-length: ", length.as_bytes(), b"\r\n"]), body.clone(), "Duplicate streaming framing header"),
            ("nondecimal length", cat(&[b"Content-Length: +", length.as_bytes(), b"\r\n"]), body.clone(), "Invalid response length"),
            ("unsupported transfer coding", b"Transfer-Encoding: gzip\r\n".to_vec(), body.clone(), "Unsupported streaming transfer encoding"),
            ("unsupported content coding", b"Content-Encoding: gzip\r\n".to_vec(), body.clone(), "Unsupported streaming content encoding"),
            ("duplicate content coding", b"Content-Encoding: identity\r\ncontent-encoding: identity\r\n".to_vec(), body.clone(), "Duplicate streaming framing header"),
            ("duplicate transfer coding", b"Transfer-Encoding: chunked\r\ntransfer-encoding: chunked\r\n".to_vec(), body.clone(), "Duplicate streaming framing header"),
            ("ambiguous length", b"Content-Length: 999\r\nTransfer-Encoding: chunked\r\n".to_vec(), ambiguous, "Ambiguous streaming response framing"),
        ];
        for (label, length_fields, payload, expected) in cases {
            let id = stream_open(controller, protocol);
            let part = stream_feed(controller, id, &cat(&[response_head, &length_fields, b"\r\n", &payload]));
            keep_msg(
                passed,
                &format!("{protocol} {label} exposes no text and cannot complete"),
                error_text(&part).as_deref() == Some(expected) && delta_eq(&part, "") && is_false(&part, "complete"),
                || part.to_string(),
            );
            keep(passed, &format!("{protocol} {label} retires the affected stream"), stream_finish(controller, id, true).is_err());
        }

        let id = stream_open(controller, protocol);
        let declared = (first.len() + 1).to_string();
        let part = stream_feed(controller, id, &cat(&[response_head, b"Content-Length: ", declared.as_bytes(), b"\r\n\r\n", first]));
        keep_msg(
            passed,
            &format!("{protocol} publishes earlier valid text before the full declared body arrives"),
            error_text(&part).is_none() && delta_eq(&part, "EARLIER") && is_false(&part, "complete"),
            || part.to_string(),
        );
        let part = stream_feed(controller, id, last);
        keep_msg(
            passed,
            &format!("{protocol} subsequent overflow exposes no further text"),
            error_text(&part).as_deref() == Some("Streaming response exceeds Content-Length") && delta_eq(&part, "") && is_false(&part, "complete"),
            || part.to_string(),
        );

        let id = stream_open(controller, protocol);
        let prefix = stream_feed(controller, id, &cat(&[response_head, b"Content-Length: ", length.as_bytes(), b"\r\n\r\n", first]));
        let final_part = stream_feed(controller, id, last);
        let completed = stream_finish(controller, id, false).unwrap_or_else(|error| panic!("{protocol} exact-length finish: {error}"));
        keep_msg(
            passed,
            &format!("{protocol} exact-length split body still completes with both text fragments"),
            delta_eq(&prefix, "EARLIER") && is_false(&prefix, "complete") && delta_eq(&final_part, "LATER") && is_true(&final_part, "complete") && data_contains(&completed, b"EARLIERLATER"),
            || format!("prefix {} final {} settled {}", prefix.to_string(), final_part.to_string(), completed.to_string()),
        );

        let id = stream_open(controller, protocol);
        let mut frames = Vec::new();
        for part in [first, last] {
            frames.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
            frames.extend_from_slice(part);
            frames.extend_from_slice(b"\r\n");
        }
        frames.extend_from_slice(b"0\r\n\r\n");
        let chunked = stream_feed(controller, id, &cat(&[response_head, b"Transfer-Encoding: Chunked\r\nContent-Encoding: identity\r\n\r\n", &frames]));
        keep_msg(
            passed,
            &format!("{protocol} still accepts chunked framing with identity encoding"),
            delta_eq(&chunked, "EARLIERLATER") && is_true(&chunked, "complete") && stream_finish(controller, id, false).is_ok(),
            || chunked.to_string(),
        );

        let chunk_head = cat(&[response_head, b"Transfer-Encoding: chunked\r\n\r\n"]);
        let terminal = cat(&[&chunk_head, format!("{:x}\r\n", body.len()).as_bytes(), &body, b"\r\n0\r\n"]);
        let trailers = vec![
            ("malformed trailer field", b"X-Bad : value\r\n\r\n".to_vec(), "Invalid response header"),
            ("framing trailer", b"Content-Length: 0\r\n\r\n".to_vec(), "Framing trailer forbidden"),
            ("overlong unfinished trailer", cat(&[b"X-Note: ", &vec![b'x'; 16384]]), "Chunk trailers exceed limit"),
            ("bytes after final trailer delimiter", b"\r\nextra".to_vec(), "Bytes after framed response"),
        ];
        for (label, trailer, expected) in trailers {
            let id = stream_open(controller, protocol);
            let part = stream_feed(controller, id, &cat(&[&terminal, &trailer]));
            keep_msg(
                passed,
                &format!("{protocol} {label} refuses completion and preserves validated text"),
                error_text(&part).as_deref() == Some(expected) && delta_eq(&part, "EARLIERLATER") && is_false(&part, "complete"),
                || part.to_string(),
            );
            keep(passed, &format!("{protocol} {label} retires the affected stream"), stream_finish(controller, id, true).is_err());
        }

        let id = stream_open(controller, protocol);
        let part = stream_feed(controller, id, &cat(&[&terminal, b"X-Note: valid\r"]));
        keep_msg(
            passed,
            &format!("{protocol} split trailer field retains text and waits for completion"),
            error_text(&part).is_none() && delta_eq(&part, "EARLIERLATER") && is_false(&part, "complete"),
            || part.to_string(),
        );
        let part = stream_feed(controller, id, b"\n\r");
        keep_msg(
            passed,
            &format!("{protocol} validated trailer still waits for the final blank line"),
            error_text(&part).is_none() && delta_eq(&part, "") && is_false(&part, "complete"),
            || part.to_string(),
        );
        let part = stream_feed(controller, id, b"\n");
        let completed = stream_finish(controller, id, false).unwrap_or_else(|error| panic!("{protocol} trailer finish: {error}"));
        keep_msg(
            passed,
            &format!("{protocol} valid split trailers complete without losing the reply"),
            is_true(&part, "complete") && delta_eq(&part, "") && data_contains(&completed, b"EARLIERLATER"),
            || format!("part {} settled {}", part.to_string(), completed.to_string()),
        );

        let first_chunk = cat(&[format!("{:x}\r\n", first.len()).as_bytes(), first]);
        let last_chunk = cat(&[format!("{:x}\r\n", last.len()).as_bytes(), last]);
        let id = stream_open(controller, protocol);
        let part = stream_feed(controller, id, &cat(&[&chunk_head, &first_chunk, b"\r"]));
        keep_msg(
            passed,
            &format!("{protocol} chunk payload waits for its split trailing LF"),
            error_text(&part).is_none() && delta_eq(&part, "") && is_false(&part, "complete"),
            || part.to_string(),
        );
        let part = stream_feed(controller, id, b"\n");
        keep_msg(
            passed,
            &format!("{protocol} valid split chunk delimiter releases its text promptly"),
            error_text(&part).is_none() && delta_eq(&part, "EARLIER") && is_false(&part, "complete"),
            || part.to_string(),
        );
        let part = stream_feed(controller, id, &cat(&[&last_chunk, b"\r\n0\r\n\r\n"]));
        let completed = stream_finish(controller, id, false).unwrap_or_else(|error| panic!("{protocol} chunk finish: {error}"));
        keep_msg(
            passed,
            &format!("{protocol} validated chunks preserve the complete reply"),
            is_true(&part, "complete") && delta_eq(&part, "LATER") && data_contains(&completed, b"EARLIERLATER"),
            || format!("part {} settled {}", part.to_string(), completed.to_string()),
        );

        let id = stream_open(controller, protocol);
        let part = stream_feed(controller, id, &cat(&[&chunk_head, &first_chunk, b"\r\n", &last_chunk]));
        keep_msg(
            passed,
            &format!("{protocol} one read exposes only the chunk with a validated delimiter"),
            error_text(&part).is_none() && delta_eq(&part, "EARLIER") && is_false(&part, "complete"),
            || part.to_string(),
        );
        let part = stream_feed(controller, id, b"xx");
        keep_msg(
            passed,
            &format!("{protocol} bad split chunk delimiter refuses the pending text"),
            error_text(&part).as_deref() == Some("Invalid stream chunk terminator") && delta_eq(&part, "") && is_false(&part, "complete"),
            || part.to_string(),
        );
        keep(passed, &format!("{protocol} bad split chunk delimiter retires the affected stream"), stream_finish(controller, id, true).is_err());

        let id = stream_open(controller, protocol);
        let part = stream_feed(controller, id, &cat(&[&chunk_head, &first_chunk, b"\r\n", &last_chunk, b"xx"]));
        keep_msg(
            passed,
            &format!("{protocol} same-read delimiter refusal preserves only the earlier validated chunk"),
            error_text(&part).as_deref() == Some("Invalid stream chunk terminator") && delta_eq(&part, "EARLIER") && is_false(&part, "complete"),
            || part.to_string(),
        );
        keep(passed, &format!("{protocol} same-read delimiter refusal retires the affected stream"), stream_finish(controller, id, true).is_err());
    }

    for suffix in [b"x".as_slice(), b"x\r", b"x\r\n\r\n", b"\rx", b"\r\n\rx"] {
        let id = open_frame(controller, "response", FRAME_LIMIT, false);
        let primed = feed_ok(controller, id, &padded_header(fields, 65536), "response", "overlong suffix header");
        if !is_false(&primed, "ready") {
            panic!("overlong suffix header was ready: {}", primed.to_string());
        }
        let reply = feed(controller, id, suffix, "response");
        let name = format!("overlong response header suffix {} retires only that frame", py_bytes(suffix));
        keep_msg(passed, &name, error_text_result(&reply).as_deref() == Some("HTTP headers exceed bound") && !close_frame(controller, id, "response"), || match &reply {
            Ok(value) => value.to_string(),
            Err(error) => error.clone(),
        });
    }

    let (raw, split) = exact_limit_chunked(b"limit payload", b"text/plain");
    let name = "an exact-limit chunk header split after CR completes before peer close";
    let (actual, result) = persistent(controller, &raw, false, None, None, Some(split)).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, actual == raw && same_meta(&result, 200, raw.len()), || exchange_detail(&actual, &raw, &result));

    for split in [65537usize, 65538, 65539] {
        let raw = exact_limit_header(b"actual body");
        let name = format!("exact-limit final response header accepts delimiter split at {split}");
        let (actual, result) = persistent(controller, &raw, false, None, None, Some(split)).unwrap_or_else(|error| panic!("{name}: {error}"));
        keep_msg(passed, &name, actual == raw && same_meta(&result, 200, raw.len()), || exchange_detail(&actual, &raw, &result));
    }

    let mut interim = b"HTTP/1.1 100 Continue\r\nX-Padding: ".to_vec();
    interim.resize(65536, b'x');
    interim.extend_from_slice(b"\r\n\r\n");
    let raw = cat(&[&interim, b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\na"]);
    let name = "exact-limit informational header accepts a partial delimiter before the final response";
    let (actual, result) = persistent(controller, &raw, false, None, None, Some(65539)).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, actual == raw && same_meta(&result, 200, raw.len()), || exchange_detail(&actual, &raw, &result));

    let limited = exact_limit_header(b"");
    for extra in [b"x".as_slice(), b"x\r", b"\r\nx", b"\r\n\rx"] {
        let id = open_frame(controller, "response", FRAME_LIMIT, false);
        feed_ok(controller, id, &limited[..65536], "response", "exact-limit metadata");
        let reply = feed(controller, id, extra, "response");
        let name = format!("one extra metadata byte or a broken delimiter fails within the limit {}", py_bytes(extra));
        keep_msg(passed, &name, error_text_result(&reply).as_deref() == Some("HTTP headers exceed bound") && !close_frame(controller, id, "response"), || match &reply {
            Ok(value) => value.to_string(),
            Err(error) => error.clone(),
        });
    }

    let head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    let terminal = cat(&[b"0;", &vec![b'x'; 8190]]);
    let id = open_frame(controller, "response", FRAME_LIMIT, false);
    let pending = feed_ok(controller, id, &cat(&[head, &terminal, b"\r"]), "response", "terminal chunk CR");
    keep(passed, "an exact-limit terminal chunk stays pending after CR", is_false(&pending, "ready"));
    let completed = feed_ok(controller, id, b"\n\r\n", "response", "terminal chunk LF");
    keep(passed, "the terminal LF and empty trailer section complete the exact-limit frame", is_true(&completed, "ready") && finish(controller, id, false, "response").is_ok());

    for (label, suffix) in [
        ("unterminated overflow", b"x".as_slice()),
        ("overflow followed by CR", b"x\r"),
        ("overflow followed by CRLF", b"x\r\n"),
        ("CR followed by a non-LF byte", b"\rx"),
    ] {
        let id = open_frame(controller, "response", FRAME_LIMIT, false);
        let reply = feed(controller, id, &cat(&[head, &terminal, suffix]), "response");
        keep_msg(
            passed,
            &format!("{label} is refused and retires only that frame"),
            error_text_result(&reply).as_deref() == Some("Chunk header exceeds bound") && !close_frame(controller, id, "response"),
            || match &reply {
                Ok(value) => value.to_string(),
                Err(error) => error.clone(),
            },
        );
    }

    let mut slots = Vec::new();
    for n in 0..8 {
        slots.push(open_frame(controller, &format!("slot-{n}"), FRAME_LIMIT, false));
    }
    let overflow = call(controller, obj(vec![("op".into(), s("network_frame_open")), ("owner".into(), s("overflow")), ("limit".into(), twin::n("100"))]));
    keep_msg(passed, "response slots remain bounded", overflow.get("error").is_some(), || overflow.to_string());
    for (n, id) in slots.iter().enumerate() {
        close_frame(controller, *id, &format!("slot-{n}"));
    }
    let name = "ordinary response remains available after scoped failures";
    let (_, result) = persistent(controller, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", false, None, None, None).unwrap_or_else(|error| panic!("{name}: {error}"));
    keep_msg(passed, name, status_is(&result, 200), || result.to_string());

    integration(controller, root, passed);
    owned_request_lines(controller, root, passed);
}

fn integration(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let primary = bind_listener();
    let child = bind_listener();
    let nas = bind_listener();
    let primary_port = port_of(&primary);
    let child_port = port_of(&child);
    let nas_port = port_of(&nas);
    let scratch = Scratch::create(Path::new(root));
    let root = scratch.path().to_string();

    let fresh = |controller: &mut Controller| -> Value {
        let resumed = controller.event(None, "resume", obj(vec![]), &root);
        let mut st = resumed.get("state").unwrap_or_else(|| panic!("resume returned no state: {}", resumed.to_string())).clone();
        put(&mut st, "profiles", Value::Arr(vec![
            profile("Primary", "Primary", &format!("http://127.0.0.1:{primary_port}/model")),
            profile("Child", "Child", &format!("http://127.0.0.1:{child_port}/model")),
        ]));
        put(&mut st, "agents", Value::Arr(vec![
            obj(vec![
                ("name".into(), s("Parent")),
                ("model".into(), s("0")),
                ("tools".into(), s("read,delegate,drive_read")),
                ("subagents".into(), Value::Arr(vec![twin::n("1")])),
            ]),
            obj(vec![("name".into(), s("Child")), ("model".into(), s("1")), ("tools".into(), s("read"))]),
        ]));
        st
    };

    let st = fresh(controller);
    let mut turn = controller.event(Some(&st), "send", obj(vec![("draft".into(), s("Delegate a simple response."))]), &root);
    turn = model_response(controller, &turn, &primary, "{\"tool\": \"delegate\", \"args\": {\"agent\": 1, \"task\": \"Return READY_CHILD\"}}", None, &root);
    let reviewed = state_of(&turn).clone();
    keep_msg(
        passed,
        "persistent primary HTTP reply opens the actual child review",
        nested_str(&reviewed, &["pending", "tool"]) == Some("delegate"),
        || reviewed.to_string(),
    );
    turn = controller.event(Some(&reviewed), "approve", obj(vec![]), &root);
    turn = model_response(controller, &turn, &child, "READY_CHILD", Some("chunk"), &root);
    let child_state = state_of(&turn).clone();
    let child_ok = field_present(&turn, "effect").is_some_and(|effect| wire_contains(effect, "READY_CHILD")) && num_is(&child_state, "request_index", "0");
    let child_detail = turn.to_string();
    keep_msg(passed, "persistent child reply with an exact-limit split chunk header returns through its captured parent", child_ok, || child_detail);
    turn = model_response(controller, &turn, &primary, "Parent received READY_CHILD.", Some("header"), &root);
    let settled = state_of(&turn).clone();
    keep_msg(
        passed,
        "persistent parent continuation with an exact-limit split response header settles its actual task",
        required_falsy(&settled, "busy") && missing_or_falsy(&settled, "delegation_stack"),
        || settled.to_string(),
    );

    let mut st = fresh(controller);
    put(&mut st, "drive", obj(vec![
        ("url".into(), s(&format!("http://127.0.0.1:{nas_port}/share/"))),
        ("plain".into(), Value::Bool(true)),
        ("user".into(), s("")),
        ("password".into(), s("")),
    ]));
    turn = controller.event(Some(&st), "send", obj(vec![("draft".into(), s("Read actual.txt from the network folder."))]), &root);
    turn = model_response(controller, &turn, &primary, "{\"tool\": \"drive_read\", \"args\": {\"path\": \"actual.txt\"}}", None, &root);
    let approved_state = state_of(&turn).clone();
    turn = controller.event(Some(&approved_state), "approve", obj(vec![]), &root);
    let effect = turn.get("effect").cloned().unwrap_or_else(|| panic!("drive approval returned no effect: {}", turn.to_string()));
    let request = match effect.get("wire") {
        Some(Value::Str(text)) => text.as_bytes().to_vec(),
        _ => panic!("drive effect has no wire: {}", effect.to_string()),
    };
    let purpose = effect.get("purpose").cloned().unwrap_or_else(|| panic!("drive effect missing purpose: {}", effect.to_string()));
    let drive_ticket = effect.get("drive_ticket").cloned().unwrap_or_else(|| panic!("drive effect missing drive_ticket: {}", effect.to_string()));
    let body = b"ACTUAL_PERSISTENT_NAS_FILE";
    let (raw, split) = exact_limit_response(body, b"text/plain");
    let (actual, _) = persistent(controller, &raw, false, Some(&request), Some(&nas), Some(split)).unwrap_or_else(|error| panic!("persistent NAS response: {error}"));
    let nas_state = state_of(&turn).clone();
    turn = controller.event(Some(&nas_state), "network_result", obj(vec![
        ("purpose".into(), purpose),
        ("drive_ticket".into(), drive_ticket),
        ("data".into(), s(&b64_encode(&actual))),
    ]), &root);
    let effect = field_present(&turn, "effect");
    keep_msg(
        passed,
        "persistent NAS response with an exact-limit split response header resumes its requesting model with actual bytes",
        effect.is_some_and(|effect| matches!(effect.get("purpose"), Some(Value::Str(text)) if text == "model") && wire_contains(effect, "ACTUAL_PERSISTENT_NAS_FILE")),
        || turn.to_string(),
    );
    turn = model_response(controller, &turn, &primary, "The network file contains ACTUAL_PERSISTENT_NAS_FILE.", None, &root);
    let state = state_of(&turn);
    keep_msg(passed, "persistent NAS continuation settles without replay", required_falsy(state, "busy"), || state.to_string());
}

fn owned_request_lines(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let scratch = Scratch::create(Path::new(root));
    let root = scratch.path().to_string();
    let sent = send_draft(controller, &root, None, "Read the documentation page.");
    let fed = feed_proposal(controller, &root, &sent, r#"{"tool":"web_read","args":{"url":"https://example.org/owned-page"}}"#);
    let page = follow_web(controller, &root, &fed);
    let wire = response_wire(&page);
    keep_msg(
        passed,
        "owned user agent appears once on a page request",
        wire.matches("\r\nUser-Agent: RLLM web reader\r\n").count() == 1 && wire.matches("\r\nUser-Agent:").count() == 1,
        || format!("{wire}\n{}", page.to_string()),
    );
    keep_msg(
        passed,
        "page request accept line is the product line",
        wire.matches("\r\nAccept: text/html, application/json, text/plain\r\n").count() == 1 && wire.matches("\r\nAccept:").count() == 1,
        || format!("{wire}\n{}", page.to_string()),
    );
    let search = send_draft(controller, &root, Some(obj(vec![
        ("search_url".into(), s("https://search.example/find?q={query}")),
        ("header".into(), s("User-Agent")),
        ("key".into(), s("CallerAgent")),
        ("prefix".into(), s("")),
    ])), "Find the fixture topic.");
    let searched = feed_proposal(controller, &root, &search, r#"{"tool":"web_search","args":{"query":"fixture topic"}}"#);
    let refused = follow_web(controller, &root, &searched);
    let notice = nested_str(event_body(&refused), &["state", "notice"]).unwrap_or("");
    let err = error_text(&refused).unwrap_or_default();
    let refused_wire = response_wire(&refused);
    keep_msg(
        passed,
        "caller supplied user agent is refused",
        (notice.contains("Invalid HTTP header") || err.contains("Invalid HTTP header")) && refused_wire.is_empty(),
        || refused.to_string(),
    );
}

fn send_draft(controller: &mut Controller, root: &str, web: Option<Value>, draft: &str) -> Value {
    let resumed = product_event(controller, None, "resume", obj(vec![]), root);
    let mut st = event_body(&resumed).get("state").cloned().unwrap_or_else(|| panic!("resume returned no state: {}", resumed.to_string()));
    put(&mut st, "profiles", Value::Arr(vec![profile("Primary", "0", "http://127.0.0.1:9/v1/chat/completions")]));
    if let Some(web) = web {
        put(&mut st, "web", web);
    }
    product_event(controller, Some(&st), "send", obj(vec![("draft".into(), s(draft))]), root)
}

fn feed_proposal(controller: &mut Controller, root: &str, sent: &Value, content: &str) -> Value {
    let turn = event_body(sent);
    let state = turn.get("state").cloned().unwrap_or_else(|| panic!("send returned no state: {}", sent.to_string()));
    let ticket = match state.get("model_expected") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("send produced no model ticket: {}", sent.to_string()),
    };
    let body = chat_completion(content);
    let mut bytes = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    product_event(controller, Some(&state), "network_result", obj(vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), s(&ticket)),
        ("data".into(), s(&b64_encode(&bytes))),
    ]), root)
}

fn follow_web(controller: &mut Controller, root: &str, fed: &Value) -> Value {
    let turn = event_body(fed);
    if nested_str(turn, &["effect", "purpose"]) == Some("web_tool") {
        return fed.clone();
    }
    if nested_str(turn, &["state", "dialog"]) == Some("approval") {
        let state = turn.get("state").cloned().unwrap_or(Value::Null);
        return product_event(controller, Some(&state), "approve", obj(vec![]), root);
    }
    fed.clone()
}

fn product_event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.send(&obj(vec![
        ("op".into(), s("event")),
        ("state".into(), state.cloned().unwrap_or(Value::Null)),
        ("action".into(), s(action)),
        ("inputs".into(), inputs),
        ("root".into(), s(root)),
    ]))
}

fn event_body(reply: &Value) -> &Value {
    reply.get("value").unwrap_or(reply)
}

fn response_wire(reply: &Value) -> String {
    nested_str(event_body(reply), &["effect", "wire"]).unwrap_or("").to_string()
}

fn model_response(controller: &mut Controller, turn: &Value, listener: &TcpListener, text: &str, framing: Option<&str>, root: &str) -> Value {
    let data = chat_completion(text);
    let (raw, split) = match framing {
        Some("chunk") => {
            let (raw, split) = exact_limit_chunked(data.as_bytes(), b"application/json");
            (raw, Some(split))
        }
        Some("header") => {
            let (raw, split) = exact_limit_response(data.as_bytes(), b"application/json");
            (raw, Some(split))
        }
        _ => {
            let raw = cat(&[b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: ", data.len().to_string().as_bytes(), b"\r\n\r\n", data.as_bytes()]);
            (raw, None)
        }
    };
    let effect = turn.get("effect").unwrap_or_else(|| panic!("model turn has no effect: {}", turn.to_string()));
    let request = match effect.get("wire") {
        Some(Value::Str(text)) => text.as_bytes().to_vec(),
        _ => panic!("model effect has no wire: {}", effect.to_string()),
    };
    let (actual, _) = persistent(controller, &raw, false, Some(&request), Some(listener), split).unwrap_or_else(|error| panic!("model socket: {error}"));
    let state = state_of(turn).clone();
    let model_ticket = state.get("model_expected").cloned().unwrap_or_else(|| panic!("model turn has no model_expected: {}", state.to_string()));
    controller.event(Some(&state), "network_result", obj(vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), model_ticket),
        ("data".into(), s(&b64_encode(&actual))),
    ]), root)
}

fn chat_completion(text: &str) -> String {
    obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("index".into(), twin::n("0")),
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )])
    .to_string()
}

fn persistent(controller: &mut Controller, raw: &[u8], head: bool, request: Option<&[u8]>, listener: Option<&TcpListener>, split: Option<usize>) -> Result<(Vec<u8>, Value), String> {
    let owned = if listener.is_none() { Some(bind_listener()) } else { None };
    let listener = match listener {
        Some(existing) => existing,
        None => owned.as_ref().expect("owned listener"),
    };
    persistent_raw(controller, raw, FRAME_LIMIT, head, request, listener, split)
}

fn bind_listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"))
}

fn persistent_raw(controller: &mut Controller, raw: &[u8], limit: usize, head: bool, request: Option<&[u8]>, listener: &TcpListener, split: Option<usize>) -> Result<(Vec<u8>, Value), String> {
    let accept_on = listener.try_clone().map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let raw = raw.to_vec();
    let request = request.map(|bytes| bytes.to_vec());
    let client_request = request.clone();
    let (prefix_tx, prefix_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (sent_tx, sent_rx) = mpsc::channel::<()>();
    let (closed_tx, closed_rx) = mpsc::channel::<()>();
    let (expired_tx, expired_rx) = mpsc::channel::<()>();
    let (finished_tx, finished_rx) = mpsc::channel::<Result<(), String>>();
    let handle = thread::spawn(move || {
        let result = serve(accept_on, raw, request, split, prefix_rx, release_rx, sent_tx, expired_tx);
        let _ = closed_tx.send(());
        let _ = finished_tx.send(result);
    });
    let mut halt = Halt {
        release: Some(release_tx),
        prefix: Some(prefix_tx.clone()),
        handle: Some(handle),
    };
    let mut opened = None;
    let outcome = (|| {
        let id = open_frame_result(controller, "response", limit, head)?;
        opened = Some(id);
        drive_client(controller, id, &address, client_request.as_deref(), prefix_tx, sent_rx, closed_rx, expired_rx, split)
    })();
    let joined = halt.stop();
    if let Some(id) = opened {
        let _ = close_frame(controller, id, "response");
    }
    let server = finished_rx.try_recv().unwrap_or(Ok(()));
    match (outcome, server, joined) {
        (Ok(value), Ok(()), Ok(())) => Ok(value),
        (Err(error), Err(server_error), _) => Err(format!("{error}; server: {server_error}")),
        (Err(error), _, Err(_)) => Err(format!("{error}; server thread panicked")),
        (Err(error), _, _) => Err(error),
        (Ok(_), Err(server_error), _) => Err(format!("server: {server_error}")),
        (Ok(_), _, Err(_)) => Err("server thread panicked".into()),
    }
}

struct Halt {
    release: Option<Sender<()>>,
    prefix: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl Halt {
    fn stop(&mut self) -> thread::Result<()> {
        if let Some(sender) = self.release.take() {
            let _ = sender.send(());
        }
        if let Some(sender) = self.prefix.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.join()
        } else {
            Ok(())
        }
    }
}

impl Drop for Halt {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn serve(listener: TcpListener, raw: Vec<u8>, request: Option<Vec<u8>>, split: Option<usize>, prefix_rx: Receiver<()>, release_rx: Receiver<()>, sent_tx: Sender<()>, expired_tx: Sender<()>) -> Result<(), String> {
    let mut stream = accept_timeout(&listener, Duration::from_secs(5))?;
    stream.set_nodelay(true).map_err(|error| error.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(4))).map_err(|error| error.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(4))).map_err(|error| error.to_string())?;
    if let Some(request) = request.as_deref() {
        let mut received = vec![0u8; request.len()];
        stream.read_exact(&mut received).map_err(|error| format!("request read: {error}"))?;
        if received != request {
            return Err(format!("request mismatch: got {} want {}", received.len(), request.len()));
        }
    }
    if let Some(split) = split {
        if split > raw.len() {
            return Err(format!("split {split} past {}", raw.len()));
        }
        stream.write_all(&raw[..split]).and_then(|_| stream.flush()).map_err(|error| error.to_string())?;
        if prefix_rx.recv_timeout(Duration::from_secs(4)).is_err() || release_rx.try_recv().is_ok() {
            return Ok(());
        }
        stream.write_all(&raw[split..]).map_err(|error| error.to_string())?;
    } else {
        stream.write_all(&raw).map_err(|error| error.to_string())?;
    }
    stream.flush().map_err(|error| error.to_string())?;
    sent_tx.send(()).map_err(|_| "sent receiver dropped".to_string())?;
    if release_rx.recv_timeout(Duration::from_secs(10)).is_err() {
        let _ = expired_tx.send(());
    }
    Ok(())
}

fn accept_timeout(listener: &TcpListener, timeout: Duration) -> Result<TcpStream, String> {
    listener.set_nonblocking(true).map_err(|error| error.to_string())?;
    let started = std::time::Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).map_err(|error| error.to_string())?;
                return Ok(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() > timeout {
                    return Err("accept timed out".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(format!("accept: {error}")),
        }
    }
}

fn drive_client(controller: &mut Controller, id: u64, address: &std::net::SocketAddr, request: Option<&[u8]>, prefix_tx: Sender<()>, sent_rx: Receiver<()>, closed_rx: Receiver<()>, expired_rx: Receiver<()>, split: Option<usize>) -> Result<(Vec<u8>, Value), String> {
    let mut stream = TcpStream::connect_timeout(address, Duration::from_secs(1)).map_err(|error| format!("connect: {error}"))?;
    stream.set_nodelay(true).map_err(|error| error.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).map_err(|error| error.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(1))).map_err(|error| error.to_string())?;
    if let Some(request) = request {
        stream.write_all(request).and_then(|_| stream.flush()).map_err(|error| format!("send request: {error}"))?;
    }
    let mut out = Vec::new();
    let mut signaled = false;
    loop {
        let mut buf = [0u8; 8192];
        let count = read_some(&mut stream, &mut buf)?;
        if count == 0 {
            return Err("Unexpected EOF".into());
        }
        out.extend_from_slice(&buf[..count]);
        let reply = feed(controller, id, &buf[..count], "response")?;
        let ready_now = is_true(&reply, "ready");
        if let Some(split) = split {
            if !signaled && out.len() > split {
                return Err(format!("read {} bytes past split {split}", out.len()));
            }
            if !signaled && out.len() == split {
                if ready_now {
                    return Err("Response completed before the split delimiter".into());
                }
                signaled = true;
                prefix_tx.send(()).map_err(|_| "prefix receiver dropped".to_string())?;
            }
        }
        if ready_now {
            break;
        }
    }
    sent_rx.recv_timeout(Duration::from_secs(1)).map_err(|_| "persistent peer did not finish sending".to_string())?;
    if expired_rx.try_recv().is_ok() {
        return Err("persistent peer expired before finish".into());
    }
    if closed_rx.try_recv().is_ok() {
        return Err("persistent peer closed before finish".into());
    }
    let result = finish(controller, id, false, "response")?;
    Ok((out, result))
}

fn read_some(stream: &mut TcpStream, buf: &mut [u8]) -> Result<usize, String> {
    loop {
        match stream.read(buf) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("read: {error}")),
        }
    }
}

fn open_frame(controller: &mut Controller, owner: &str, limit: usize, head: bool) -> u64 {
    open_frame_result(controller, owner, limit, head).unwrap_or_else(|error| panic!("frame open: {error}"))
}

fn open_frame_result(controller: &mut Controller, owner: &str, limit: usize, head: bool) -> Result<u64, String> {
    let reply = call(controller, obj(vec![
        ("op".into(), s("network_frame_open")),
        ("owner".into(), s(owner)),
        ("limit".into(), twin::n(&limit.to_string())),
        ("head_only".into(), Value::Bool(head)),
    ]));
    match reply.get("value") {
        Some(value) => session_id(value),
        None => Err(error_text(&reply).unwrap_or_else(|| reply.to_string())),
    }
}

fn feed(controller: &mut Controller, id: u64, data: &[u8], owner: &str) -> Result<Value, String> {
    let reply = call(controller, obj(vec![
        ("op".into(), s("network_frame_feed")),
        ("owner".into(), s(owner)),
        ("id".into(), twin::n(&id.to_string())),
        ("data".into(), s(&b64_encode(data))),
    ]));
    match reply.get("value") {
        Some(value) => Ok(value.clone()),
        None => Err(error_text(&reply).unwrap_or_else(|| reply.to_string())),
    }
}

fn feed_ok(controller: &mut Controller, id: u64, data: &[u8], owner: &str, context: &str) -> Value {
    feed(controller, id, data, owner).unwrap_or_else(|error| panic!("{context}: {error}"))
}

fn finish(controller: &mut Controller, id: u64, eof: bool, owner: &str) -> Result<Value, String> {
    let reply = call(controller, obj(vec![
        ("op".into(), s("network_frame_finish")),
        ("owner".into(), s(owner)),
        ("id".into(), twin::n(&id.to_string())),
        ("eof".into(), Value::Bool(eof)),
    ]));
    match reply.get("value") {
        Some(value) => Ok(value.clone()),
        None => Err(error_text(&reply).unwrap_or_else(|| reply.to_string())),
    }
}

fn close_frame(controller: &mut Controller, id: u64, owner: &str) -> bool {
    op(controller, obj(vec![
        ("op".into(), s("network_frame_close")),
        ("owner".into(), s(owner)),
        ("id".into(), twin::n(&id.to_string())),
    ]))
    .bool_()
}

fn stream_open(controller: &mut Controller, protocol: &str) -> u64 {
    let value = op(controller, obj(vec![("op".into(), s("stream_open")), ("protocol".into(), s(protocol))]));
    session_id(&value).unwrap_or_else(|error| panic!("stream open: {error}"))
}

fn stream_feed(controller: &mut Controller, id: u64, data: &[u8]) -> Value {
    op(controller, obj(vec![
        ("op".into(), s("stream_feed")),
        ("id".into(), twin::n(&id.to_string())),
        ("data".into(), s(&b64_encode(data))),
    ]))
}

fn stream_finish(controller: &mut Controller, id: u64, eof: bool) -> Result<Value, String> {
    let reply = call(controller, obj(vec![
        ("op".into(), s("stream_finish")),
        ("id".into(), twin::n(&id.to_string())),
        ("eof".into(), Value::Bool(eof)),
    ]));
    match reply.get("value") {
        Some(value) => Ok(value.clone()),
        None => Err(error_text(&reply).unwrap_or_else(|| reply.to_string())),
    }
}

fn call(controller: &mut Controller, envelope: Value) -> Value {
    controller.send(&envelope)
}

fn op(controller: &mut Controller, envelope: Value) -> Value {
    controller.op(&envelope)
}

fn session_id(value: &Value) -> Result<u64, String> {
    match value {
        Value::Num(text) => text.parse::<u64>().map_err(|_| format!("bad session id {text}")),
        other => Err(format!("bad session id {}", other.to_string())),
    }
}

fn same_meta(result: &Value, status: u16, bytes: usize) -> bool {
    result == &obj(vec![("status".into(), twin::n(&status.to_string())), ("bytes".into(), twin::n(&bytes.to_string()))])
}

fn status_is(result: &Value, status: u16) -> bool {
    matches!(result.get("status"), Some(Value::Num(text)) if text == &status.to_string())
}

fn is_true(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Bool(true)))
}

fn is_false(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Bool(false)))
}

fn delta_eq(value: &Value, text: &str) -> bool {
    matches!(value.get("delta"), Some(Value::Str(got)) if got == text)
}

fn error_text(value: &Value) -> Option<String> {
    match value.get("error") {
        Some(Value::Str(text)) => Some(text.clone()),
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

fn error_text_result(value: &Result<Value, String>) -> Option<String> {
    match value {
        Err(text) => Some(text.clone()),
        Ok(value) => error_text(value),
    }
}

fn stream_quiet(value: &Value) -> bool {
    error_text(value).is_none() && delta_eq(value, "") && is_false(value, "complete")
}

fn data_contains(value: &Value, needle: &[u8]) -> bool {
    let Some(Value::Str(text)) = value.get("data") else {
        return false;
    };
    let bytes = b64_decode(text);
    bytes.windows(needle.len()).any(|window| window == needle)
}

fn num_is(value: &Value, key: &str, number: &str) -> bool {
    matches!(value.get(key), Some(Value::Num(text)) if text == number)
}

fn field_present<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value.get(key) {
        None | Some(Value::Null) => None,
        Some(item) => Some(item),
    }
}

fn wire_contains(value: &Value, needle: &str) -> bool {
    matches!(value.get("wire"), Some(Value::Str(text)) if text.contains(needle))
}

fn nested_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in keys {
        current = current.get(key)?;
    }
    match current {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

fn py_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !*flag,
        Value::Num(text) => text == "0",
        Value::Str(text) => text.is_empty(),
        Value::Arr(items) => items.is_empty(),
        Value::Obj(fields) => fields.is_empty(),
    }
}

fn required_falsy(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(py_falsy)
}

fn missing_or_falsy(value: &Value, key: &str) -> bool {
    value.get(key).map(py_falsy).unwrap_or(true)
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn header_name(header: &[u8]) -> String {
    let end = header.iter().position(|byte| *byte == b':').unwrap_or(header.len());
    String::from_utf8_lossy(&header[..end]).into_owned()
}

fn py_bytes(bytes: &[u8]) -> String {
    let mut out = String::from("b'");
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push('\'');
    out
}

fn exchange_detail(actual: &[u8], raw: &[u8], result: &Value) -> String {
    let at = actual.iter().zip(raw.iter()).position(|(left, right)| left != right);
    format!("actual {} raw {} first_diff {at:?} result {}", actual.len(), raw.len(), result.to_string())
}

fn exact_limit_chunked(body: &[u8], mime: &[u8]) -> (Vec<u8>, usize) {
    let mut head = b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nTransfer-Encoding: chunked\r\nContent-Type: ".to_vec();
    head.extend_from_slice(mime);
    head.extend_from_slice(b"\r\n\r\n");
    let mut line = format!("{:x};", body.len()).into_bytes();
    if line.len() < 8192 {
        line.resize(8192, b'x');
    }
    let mut prefix = head;
    prefix.extend_from_slice(&line);
    prefix.push(b'\r');
    let split = prefix.len();
    let mut raw = prefix;
    raw.push(b'\n');
    raw.extend_from_slice(body);
    raw.extend_from_slice(b"\r\n0\r\n\r\n");
    (raw, split)
}

fn exact_limit_header(body: &[u8]) -> Vec<u8> {
    let mut head = b"HTTP/1.1 200 OK\r\nContent-Length: ".to_vec();
    head.extend_from_slice(body.len().to_string().as_bytes());
    head.extend_from_slice(b"\r\nX-Padding: ");
    if head.len() < 65536 {
        head.resize(65536, b'x');
    }
    head.extend_from_slice(b"\r\n\r\n");
    head.extend_from_slice(body);
    head
}

fn exact_limit_response(body: &[u8], mime: &[u8]) -> (Vec<u8>, usize) {
    let fields = cat(&[b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: ", body.len().to_string().as_bytes(), b"\r\nContent-Type: ", mime]);
    let head = padded_header(&fields, 65536);
    let split = head.len() + 1;
    let raw = cat(&[&head, b"\r\n\r\n", body]);
    (raw, split)
}

fn padded_header(fields: &[u8], size: usize) -> Vec<u8> {
    let mut out = fields.to_vec();
    out.extend_from_slice(b"\r\nX-Pad: ");
    if out.len() < size {
        out.resize(size, b'x');
    }
    out
}

fn trailer_of_size(size: usize) -> Vec<u8> {
    let mut out = b"X-Pad: ".to_vec();
    out.extend(std::iter::repeat(b'x').take(size - 11));
    out.extend_from_slice(b"\r\n\r\n");
    out
}

fn profile(name: &str, model: &str, endpoint: &str) -> Value {
    obj(vec![
        ("name".into(), s(name)),
        ("model".into(), s(model)),
        ("protocol".into(), s("chat")),
        ("endpoint".into(), s(endpoint)),
        ("plain".into(), Value::Bool(true)),
        ("stream".into(), Value::Bool(false)),
        ("tool_format".into(), s("structured")),
        ("key".into(), s("")),
    ])
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn keep_msg(passed: &mut Vec<String>, name: &str, ok: bool, detail: impl FnOnce() -> String) {
    if !ok {
        panic!("{name}: {}", detail());
    }
    passed.push(name.to_string());
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn port_of(listener: &TcpListener) -> u16 {
    listener.local_addr().unwrap_or_else(|error| panic!("address: {error}")).port()
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "http-owners"))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

//! Saved HTTP Archive and SAZ captures, read as the same evidence rows.
//! Each answer is a bounded row: method, status, status class, header names, body
//! length and digest, recorded timing, redirect pairing, and SAZ member pairing.
//! A missing answer is a named refusal. Header values and body bytes are not copied.
//! It is not a replay.
//! A SAZ file is a ZIP of Fiddler session text (`index.dat`, `NNN_c.txt`, `NNN_s.txt`).
use super::{obj, parse, s, Json, Json::*};

const ENTRY_CAP: usize = 64;
const URL_CAP: usize = 2048;
const REPORT_CAP: usize = 8192;
// `archive::unpack` allows 500 members and 1 MiB each. These tighter caps fail
// closed while a retained report (128 KiB) can still carry the refusal.
const MEMBER_CAP: usize = 320;
const MEMBER_BYTE_CAP: usize = 64 * 1024;
const HEADER_CAP: usize = 8;
const HEADER_NAME_CAP: usize = 64;
const TIMING_TEXT_CAP: usize = 32;
const ENTRY_CAP_NAME: &str = "saved-capture entry cap";
const URL_CAP_NAME: &str = "saved-capture url cap";
const REPORT_CAP_NAME: &str = "saved-capture report cap";
const HEADER_CAP_NAME: &str = "saved-capture header cap";
const HEADER_NAME_CAP_NAME: &str = "saved-capture header name cap";
const TIMING_TEXT_CAP_NAME: &str = "saved-capture timing cap";
const TITLE: &str = "Saved HTTP Archive capture";
const SCOPE: &str = "Scope: request method, URL, status, class, response MIME, and content.size. Not a replay.";
const OBSERVED: &str = "method, status, class, header names, body length and digest, recorded timings, and SAZ member pairing";
const UNKNOWN: &str = "replay, behaviour, header values, and body bytes";
const CONTRACT: &str = "Observed from bytes: method, status, class, header names, body length and digest, recorded timings, and SAZ member pairing. Unknown: replay, behaviour, header values, and body bytes. It is not a replay.";
const REDACTED: &str = "redacted";
const TRUNCATED: &str = "Refusal: saved capture truncated.";
const UNUSABLE: &str = "Refusal: saved capture JSON is unusable.";
const DIRECTORY: &str = "Refusal: saved capture archive directory is corrupt or truncated.";
const NO_REQUESTS: &str = "Refusal: saved capture has no request members.";
const NOT_SESSION: &str = "Refusal: saved capture is not a session archive.";
const NOT_CAPTURE_ZIP: &str = "Refusal: archive is not a saved capture.";
const NOT_FILE: &str = "Refusal: file is not a saved capture.";
const FILE_LIMIT: &str = "Refusal: saved capture file exceeds the reader limit.";
pub(super) const FILE_SECTION: &str = "Saved capture file\n";
const MEMBER_CAP_REFUSAL: &str = "Refusal: saved-capture member cap.";
const MEMBER_BYTE_REFUSAL: &str = "Refusal: saved-capture member byte cap.";
const DAMAGED: &str = "Refusal: saved capture archive member is damaged.";
const BAD_NAME: &str = "Refusal: saved capture archive member name is invalid.";
const READER_LIMIT: &str = "Refusal: saved capture archive exceeds a reader limit.";
const ARCHIVE_UNUSABLE: &str = "Refusal: saved capture archive is unusable.";
const SENSITIVE: &[&str] = &["authorization", "cookie", "set-cookie", "proxy-authorization", "x-api-key"];

#[derive(Clone, Debug, PartialEq, Eq)]
struct HeaderItem {
    name: String,
    redacted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderState {
    Recorded,
    NotRecorded,
    Unusable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Headers {
    state: HeaderState,
    listed: Vec<HeaderItem>,
    omitted_count: usize,
    omitted_name: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BodyFact {
    NotRecorded,
    Present { len: u64, sha256: String },
    OverBound { len: u64 },
    EncodingUnread { len: u64 },
    Unusable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TimingFact {
    NotRecorded,
    Recorded(String),
    Unusable,
    TooLong,
    Breakdown,
}

/// A redirect target read from the capture. Only a 3xx consults it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Loc {
    Absent,
    One(String),
    Duplicate,
    Unusable,
    TooLong,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    index: usize,
    method: String,
    url: String,
    status: u16,
    mime: String,
    bytes: u64,
    request_headers: Headers,
    response_headers: Headers,
    request_body: BodyFact,
    response_body: BodyFact,
    timing: TimingFact,
    loc: Loc,
    request_member: Option<String>,
    response_member: Option<String>,
}

#[derive(Clone, Debug)]
struct Orphan {
    name: String,
    side: &'static str,
}

#[derive(Clone, Debug)]
struct Stated {
    line: String,
    json: Json,
}

#[derive(Debug)]
struct Capture {
    rows: Vec<Row>,
    caps: Vec<&'static str>,
    report: String,
    observations: Vec<Json>,
}

#[derive(Debug)]
enum Read {
    NotCapture,
    Rows(Capture),
    Refused(String),
}

impl Headers {
    fn not_recorded() -> Self {
        Self { state: HeaderState::NotRecorded, listed: Vec::new(), omitted_count: 0, omitted_name: 0 }
    }

    fn unusable() -> Self {
        Self { state: HeaderState::Unusable, listed: Vec::new(), omitted_count: 0, omitted_name: 0 }
    }

    fn recorded(listed: Vec<HeaderItem>, omitted_count: usize, omitted_name: usize) -> Self {
        Self { state: HeaderState::Recorded, listed, omitted_count, omitted_name }
    }
}

fn at<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Obj(fields) => fields.get(key),
        _ => None,
    }
}

fn whole(value: &Json) -> Option<u64> {
    let Num(text) = value else { return None };
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn clean(value: &str) -> bool {
    value.chars().all(|c| !c.is_control())
}

fn note_cap(caps: &mut Vec<&'static str>, name: &'static str) {
    if !caps.contains(&name) {
        caps.push(name);
    }
}

fn status_class(status: u16) -> Option<&'static str> {
    Some(match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => return None,
    })
}

fn is_redirect(status: u16) -> bool {
    (300..400).contains(&status)
}

fn sensitive(name: &str) -> bool {
    SENSITIVE.iter().any(|item| name.eq_ignore_ascii_case(item))
}

fn header_name_chars(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'#' | b'+'))
}

fn sha256(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h = [0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(bytes.len() + 72);
    msg.extend_from_slice(bytes);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let j = i * 4;
            w[i] = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

/// A document that still has an open string, object, or array was cut.
/// A balanced document that still fails to parse is unusable JSON.
fn closed(text: &str) -> bool {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
    }
    !in_string && depth == 0
}

fn refuse_field(index: usize, field: &str) -> Read {
    Read::Refused(format!("Refusal: entry {index} missing {field}."))
}

fn location_text(text: &str) -> Loc {
    let text = text.trim();
    if text.is_empty() {
        Loc::Absent
    } else if text.len() > URL_CAP {
        Loc::TooLong
    } else if !clean(text) {
        Loc::Unusable
    } else {
        Loc::One(text.to_string())
    }
}

fn merge_loc(current: Loc, extra: Loc) -> Loc {
    match (current, extra) {
        (Loc::Unusable, _) | (_, Loc::Unusable) => Loc::Unusable,
        (Loc::TooLong, _) | (_, Loc::TooLong) => Loc::TooLong,
        (Loc::Duplicate, _) | (_, Loc::Duplicate) => Loc::Duplicate,
        (Loc::Absent, other) | (other, Loc::Absent) => other,
        (Loc::One(left), Loc::One(right)) if left == right => Loc::One(left),
        (Loc::One(_), Loc::One(_)) => Loc::Duplicate,
    }
}

fn json_location(value: Option<&Json>) -> Loc {
    match value {
        None => Loc::Absent,
        Some(Str(text)) => location_text(text),
        Some(_) => Loc::Unusable,
    }
}

fn take_header(listed: &mut Vec<HeaderItem>, omitted_count: &mut usize, omitted_name: &mut usize, name: &str) -> bool {
    if !header_name_chars(name) {
        return false;
    }
    if name.len() > HEADER_NAME_CAP {
        *omitted_name += 1;
        return true;
    }
    if listed.len() >= HEADER_CAP {
        *omitted_count += 1;
        return true;
    }
    listed.push(HeaderItem { name: name.to_string(), redacted: sensitive(name) });
    true
}

fn parse_header_array(items: &[Json]) -> (Headers, Loc) {
    let mut listed = Vec::new();
    let mut omitted_count = 0;
    let mut omitted_name = 0;
    let mut loc = Loc::Absent;
    for item in items {
        let Some(name) = at(item, "name").and_then(|value| match value {
            Str(text) => Some(text.as_str()),
            _ => None,
        }) else {
            return (Headers::unusable(), Loc::Unusable);
        };
        let name = name.trim();
        if !take_header(&mut listed, &mut omitted_count, &mut omitted_name, name) {
            return (Headers::unusable(), Loc::Unusable);
        }
        if name.len() <= HEADER_NAME_CAP && name.eq_ignore_ascii_case("location") {
            loc = merge_loc(loc, json_location(at(item, "value")));
        }
    }
    (Headers::recorded(listed, omitted_count, omitted_name), loc)
}

fn header_block(value: &Json) -> (Headers, Loc) {
    match at(value, "headers") {
        None => (Headers::not_recorded(), Loc::Absent),
        Some(Arr(items)) => parse_header_array(items),
        Some(_) => (Headers::unusable(), Loc::Unusable),
    }
}

fn http_headers(head: &str) -> (Headers, Loc) {
    let mut listed = Vec::new();
    let mut omitted_count = 0;
    let mut omitted_name = 0;
    let mut loc = Loc::Absent;
    for line in head.lines().skip(1) {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return (Headers::unusable(), Loc::Unusable);
        };
        let name = name.trim();
        if !take_header(&mut listed, &mut omitted_count, &mut omitted_name, name) {
            return (Headers::unusable(), Loc::Unusable);
        }
        if name.len() <= HEADER_NAME_CAP && name.eq_ignore_ascii_case("location") {
            loc = merge_loc(loc, location_text(value));
        }
    }
    (Headers::recorded(listed, omitted_count, omitted_name), loc)
}

fn body_text(text: &str, encoding: Option<&str>) -> BodyFact {
    let len = text.len() as u64;
    if text.len() > MEMBER_BYTE_CAP {
        return BodyFact::OverBound { len };
    }
    if encoding.is_some_and(|value| !value.is_empty()) {
        return BodyFact::EncodingUnread { len };
    }
    BodyFact::Present { len, sha256: sha256(text.as_bytes()) }
}

fn raw_body(bytes: &[u8]) -> BodyFact {
    let len = bytes.len() as u64;
    if bytes.len() > MEMBER_BYTE_CAP {
        BodyFact::OverBound { len }
    } else {
        BodyFact::Present { len, sha256: sha256(bytes) }
    }
}

fn text_body(owner: &Json) -> BodyFact {
    let text = match at(owner, "text") {
        None => return BodyFact::NotRecorded,
        Some(Str(text)) => text.as_str(),
        Some(_) => return BodyFact::Unusable,
    };
    let encoding = match at(owner, "encoding") {
        None => None,
        Some(Str(text)) => Some(text.as_str()),
        Some(_) => return BodyFact::Unusable,
    };
    body_text(text, encoding)
}

fn request_body(request: &Json) -> BodyFact {
    match at(request, "postData") {
        None => BodyFact::NotRecorded,
        Some(post) => text_body(post),
    }
}

fn elapsed_digits(text: &str) -> bool {
    let mut parts = text.split('.');
    let Some(whole) = parts.next() else { return false };
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    match parts.next() {
        None => true,
        Some(fraction) => !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit()) && parts.next().is_none(),
    }
}

fn timing_of(entry: &Json) -> TimingFact {
    match at(entry, "time") {
        Some(Num(text)) if elapsed_digits(text) && text.len() > TIMING_TEXT_CAP => TimingFact::TooLong,
        Some(Num(text)) if elapsed_digits(text) => TimingFact::Recorded(text.clone()),
        Some(_) => TimingFact::Unusable,
        None if at(entry, "timings").is_some() => TimingFact::Breakdown,
        None => TimingFact::NotRecorded,
    }
}

fn entry(value: &Json, index: usize) -> Result<Row, Read> {
    let Some(request) = at(value, "request") else {
        return Err(refuse_field(index, "request.method"));
    };
    let method = match at(request, "method") {
        Some(Str(text)) if !text.is_empty() && clean(text) => text.clone(),
        _ => return Err(refuse_field(index, "request.method")),
    };
    let url = match at(request, "url") {
        Some(Str(text)) if !text.is_empty() && clean(text) => text.clone(),
        _ => return Err(refuse_field(index, "request.url")),
    };
    let Some(response) = at(value, "response") else {
        return Err(refuse_field(index, "response.status"));
    };
    let status = match at(response, "status").and_then(whole) {
        Some(code) if code <= 999 => code as u16,
        _ => return Err(refuse_field(index, "response.status")),
    };
    let Some(content) = at(response, "content") else {
        return Err(refuse_field(index, "response.content.mimeType"));
    };
    let mime = match at(content, "mimeType") {
        Some(Str(text)) if clean(text) => text.clone(),
        _ => return Err(refuse_field(index, "response.content.mimeType")),
    };
    let bytes = match at(content, "size").and_then(whole) {
        Some(size) => size,
        None => return Err(refuse_field(index, "response.content.size")),
    };
    let (request_headers, _) = header_block(request);
    let (response_headers, header_loc) = header_block(response);
    let field_loc = json_location(at(response, "redirectURL"));
    let mut loc = merge_loc(header_loc, field_loc);
    if !is_redirect(status) {
        loc = Loc::Absent;
    }
    Ok(Row {
        index,
        method,
        url,
        status,
        mime,
        bytes,
        request_headers,
        response_headers,
        request_body: request_body(request),
        response_body: text_body(content),
        timing: timing_of(value),
        loc,
        request_member: None,
        response_member: None,
    })
}

fn interpret(value: &Json) -> Read {
    let Some(log) = at(value, "log") else { return Read::NotCapture };
    let Some(entries) = at(log, "entries") else { return Read::NotCapture };
    let Arr(entries) = entries else { return Read::Refused(UNUSABLE.into()) };
    let mut pending = Vec::with_capacity(entries.len());
    for (index, value) in entries.iter().enumerate() {
        match entry(value, index) {
            Ok(row) => pending.push(row),
            Err(refusal) => return refusal,
        }
    }
    finish(pending, false, Vec::new())
}

fn cap_line(name: &str) -> String {
    format!("Refusal: {name}.")
}

fn report_reserve() -> usize {
    cap_line(REPORT_CAP_NAME).len() + 1 + CONTRACT.len() + 1
}

fn row_line(row: &Row) -> String {
    match status_class(row.status) {
        Some(class) => format!("{} {} {} {} {} {} {}", row.index, row.method, row.url, row.status, class, row.mime, row.bytes),
        None => format!("{} {} {} {} {} {}", row.index, row.method, row.url, row.status, row.mime, row.bytes),
    }
}

fn stated(line: impl Into<String>, json: Json) -> Stated {
    Stated { line: line.into(), json }
}

fn refusal_line(line: impl Into<String>) -> Stated {
    let line = line.into();
    stated(line.clone(), obj(vec![("kind", s("refusal")), ("reason", s(&line))]))
}

fn index_json(index: Option<usize>) -> Json {
    match index {
        Some(index) => Num(index.to_string()),
        None => Null,
    }
}

fn status_statements(rows: &[Row]) -> Vec<Stated> {
    if rows.is_empty() {
        return Vec::new();
    }
    let same = rows.len() > 1 && rows.iter().all(|row| row.method == rows[0].method && row.status == rows[0].status);
    if same {
        return match status_class(rows[0].status) {
            Some(class) => vec![stated(
                format!("Status method {} status {} class {class} on every entry.", rows[0].method, rows[0].status),
                obj(vec![
                    ("kind", s("status")),
                    ("every", Bool(true)),
                    ("index", Null),
                    ("method", s(&rows[0].method)),
                    ("status", Num(rows[0].status.to_string())),
                    ("class", s(class)),
                ]),
            )],
            None => vec![refusal_line("Refusal: status class is unanswerable on every entry.")],
        };
    }
    rows.iter()
        .map(|row| match status_class(row.status) {
            Some(class) => stated(
                format!("Status {} method {} status {} class {class}.", row.index, row.method, row.status),
                obj(vec![
                    ("kind", s("status")),
                    ("every", Bool(false)),
                    ("index", Num(row.index.to_string())),
                    ("method", s(&row.method)),
                    ("status", Num(row.status.to_string())),
                    ("class", s(class)),
                ]),
            ),
            None => refusal_line(format!("Refusal: entry {} status class is unanswerable.", row.index)),
        })
        .collect()
}

fn redirect_statements(rows: &[Row]) -> Vec<Stated> {
    if rows.is_empty() {
        return Vec::new();
    }
    if rows.iter().all(|row| !is_redirect(row.status)) {
        return vec![stated(
            "Redirect: no 3xx in this capture.",
            obj(vec![("kind", s("redirect")), ("class", s("not-3xx")), ("every", Bool(true))]),
        )];
    }
    let mut out = Vec::new();
    for (position, row) in rows.iter().enumerate() {
        if !is_redirect(row.status) {
            out.push(stated(
                format!("Redirect {} not a 3xx.", row.index),
                obj(vec![("kind", s("redirect")), ("index", Num(row.index.to_string())), ("class", s("not-3xx"))]),
            ));
            continue;
        }
        match &row.loc {
            Loc::Absent => out.push(refusal_line(format!("Refusal: entry {} redirect has no location.", row.index))),
            Loc::Duplicate => out.push(refusal_line(format!("Refusal: entry {} redirect location is duplicated.", row.index))),
            Loc::Unusable => out.push(refusal_line(format!("Refusal: entry {} redirect target is unusable.", row.index))),
            Loc::TooLong => out.push(refusal_line(format!("Refusal: entry {} redirect target exceeds the saved-capture url cap.", row.index))),
            Loc::One(target) => {
                if let Some(later) = rows[position + 1..].iter().find(|next| next.url == *target) {
                    out.push(stated(
                        format!("Redirect {} recorded pairing entry {} target {target}.", row.index, later.index),
                        obj(vec![
                            ("kind", s("redirect")),
                            ("index", Num(row.index.to_string())),
                            ("pairing", s("recorded")),
                            ("later", Num(later.index.to_string())),
                            ("target", s(target)),
                        ]),
                    ));
                } else {
                    out.push(stated(
                        format!("Redirect {} location not followed by a recorded request. target {target}.", row.index),
                        obj(vec![
                            ("kind", s("redirect")),
                            ("index", Num(row.index.to_string())),
                            ("pairing", s("not-recorded")),
                            ("target", s(target)),
                        ]),
                    ));
                }
            }
        }
    }
    out
}

fn header_statements(index: Option<usize>, side: &str, headers: &Headers) -> Vec<Stated> {
    let prefix = match index {
        Some(index) => format!("Header {index} {side}"),
        None => format!("Header {side}"),
    };
    let prefixes = match index {
        Some(index) => format!("Headers {index} {side}"),
        None => format!("Headers {side}"),
    };
    match headers.state {
        HeaderState::NotRecorded => vec![stated(
            format!("{prefixes} not recorded."),
            obj(vec![("kind", s("headers")), ("index", index_json(index)), ("side", s(side)), ("note", s("not recorded"))]),
        )],
        HeaderState::Unusable => vec![refusal_line(format!("Refusal: entry {} {side} header is unusable.", index.unwrap_or(0)))],
        HeaderState::Recorded => {
            let mut out = Vec::new();
            for item in &headers.listed {
                let line = if item.redacted {
                    format!("{prefix} {} {REDACTED}.", item.name)
                } else {
                    format!("{prefix} {}.", item.name)
                };
                out.push(stated(
                    line,
                    obj(vec![
                        ("kind", s("header")),
                        ("index", index_json(index)),
                        ("side", s(side)),
                        ("name", s(&item.name)),
                        ("redacted", Bool(item.redacted)),
                    ]),
                ));
            }
            if headers.omitted_count > 0 {
                out.push(stated(
                    format!("{prefixes} omitted {} by {HEADER_CAP_NAME}.", headers.omitted_count),
                    obj(vec![
                        ("kind", s("headers")),
                        ("index", index_json(index)),
                        ("side", s(side)),
                        ("omitted", Num(headers.omitted_count.to_string())),
                        ("cap", s(HEADER_CAP_NAME)),
                    ]),
                ));
            } else if headers.omitted_name == 0 {
                out.push(stated(
                    format!("{prefixes} omitted 0."),
                    obj(vec![
                        ("kind", s("headers")),
                        ("index", index_json(index)),
                        ("side", s(side)),
                        ("omitted", Num("0".into())),
                    ]),
                ));
            }
            if headers.omitted_name > 0 {
                out.push(stated(
                    format!("{prefixes} omitted {} by {HEADER_NAME_CAP_NAME}.", headers.omitted_name),
                    obj(vec![
                        ("kind", s("headers")),
                        ("index", index_json(index)),
                        ("side", s(side)),
                        ("omitted", Num(headers.omitted_name.to_string())),
                        ("cap", s(HEADER_NAME_CAP_NAME)),
                    ]),
                ));
            }
            out
        }
    }
}

fn headers_statements(rows: &[Row]) -> Vec<Stated> {
    if rows.is_empty() {
        return Vec::new();
    }
    let same = rows.len() > 1
        && rows.iter().all(|row| row.request_headers == rows[0].request_headers && row.response_headers == rows[0].response_headers);
    if same {
        let mut out = vec![stated(
            "Header census same for every entry.",
            obj(vec![("kind", s("headers")), ("every", Bool(true))]),
        )];
        out.extend(header_statements(None, "request", &rows[0].request_headers));
        out.extend(header_statements(None, "response", &rows[0].response_headers));
        out
    } else {
        let mut out = Vec::new();
        for row in rows {
            out.extend(header_statements(Some(row.index), "request", &row.request_headers));
            out.extend(header_statements(Some(row.index), "response", &row.response_headers));
        }
        out
    }
}

fn body_statement(index: Option<usize>, side: &str, body: &BodyFact) -> Stated {
    let label = match index {
        Some(index) => format!("Body {index} {side}"),
        None => format!("Body {side}"),
    };
    match body {
        BodyFact::NotRecorded => stated(
            format!("{label}: not recorded."),
            obj(vec![("kind", s("body")), ("index", index_json(index)), ("side", s(side)), ("note", s("not recorded"))]),
        ),
        BodyFact::Present { len, sha256 } => stated(
            format!("{label} present length {len} sha256 {sha256}."),
            obj(vec![
                ("kind", s("body")),
                ("index", index_json(index)),
                ("side", s(side)),
                ("present", Bool(true)),
                ("length", Num(len.to_string())),
                ("sha256", s(sha256)),
            ]),
        ),
        BodyFact::OverBound { len } => refusal_line(format!(
            "Refusal: entry {} {side} body exceeds the saved-capture member byte cap. size {len}.",
            index.unwrap_or(0)
        )),
        BodyFact::EncodingUnread { len } => {
            refusal_line(format!("Refusal: entry {} {side} body encoding is not read. size {len}.", index.unwrap_or(0)))
        }
        BodyFact::Unusable => refusal_line(format!("Refusal: entry {} {side} body is unusable.", index.unwrap_or(0))),
    }
}

fn body_statements(rows: &[Row]) -> Vec<Stated> {
    if rows.is_empty() {
        return Vec::new();
    }
    let absent = rows.iter().all(|row| matches!(row.request_body, BodyFact::NotRecorded) && matches!(row.response_body, BodyFact::NotRecorded));
    if absent && rows.len() > 1 {
        return vec![
            body_statement(None, "request", &BodyFact::NotRecorded),
            body_statement(None, "response", &BodyFact::NotRecorded),
        ];
    }
    let mut out = Vec::new();
    for row in rows {
        out.push(body_statement(Some(row.index), "request", &row.request_body));
        out.push(body_statement(Some(row.index), "response", &row.response_body));
    }
    out
}

fn timing_statements(rows: &[Row]) -> Vec<Stated> {
    if rows.is_empty() {
        return Vec::new();
    }
    if rows.iter().all(|row| matches!(row.timing, TimingFact::NotRecorded)) {
        return vec![stated(
            "Timing: not recorded.",
            obj(vec![("kind", s("timing")), ("every", Bool(true)), ("note", s("not recorded"))]),
        )];
    }
    rows.iter()
        .map(|row| match &row.timing {
            TimingFact::NotRecorded => stated(
                format!("Timing {} not recorded.", row.index),
                obj(vec![("kind", s("timing")), ("index", Num(row.index.to_string())), ("note", s("not recorded"))]),
            ),
            TimingFact::Recorded(elapsed) => stated(
                format!("Timing {} elapsed {elapsed} ms recorded by the capture, not measured by RLLM.", row.index),
                obj(vec![
                    ("kind", s("timing")),
                    ("index", Num(row.index.to_string())),
                    ("elapsed", s(elapsed)),
                    ("source", s("recorded by the capture, not measured by RLLM")),
                ]),
            ),
            TimingFact::TooLong => refusal_line(format!("Refusal: entry {} timing exceeds the saved-capture timing cap.", row.index)),
            TimingFact::Unusable => refusal_line(format!("Refusal: entry {} timing is not a recorded elapsed time.", row.index)),
            TimingFact::Breakdown => stated(
                format!("Timing {} elapsed not recorded. timings present, recorded by the capture, not measured by RLLM.", row.index),
                obj(vec![
                    ("kind", s("timing")),
                    ("index", Num(row.index.to_string())),
                    ("note", s("not recorded")),
                    ("timings", s("present")),
                    ("source", s("recorded by the capture, not measured by RLLM")),
                ]),
            ),
        })
        .collect()
}

fn shown_member(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && clean(name) && !name.contains(' ')
}

fn pairing_statements(rows: &[Row], saz: bool, orphans: &[Orphan]) -> Vec<Stated> {
    if !saz {
        return vec![
            stated(
                "Session pairing: not recorded.",
                obj(vec![("kind", s("pairing")), ("note", s("not recorded"))]),
            ),
            stated(
                "index.dat: not recorded.",
                obj(vec![("kind", s("index")), ("present", Bool(false)), ("note", s("not recorded"))]),
            ),
        ];
    }
    let mut out = vec![stated(
        "index.dat present.",
        obj(vec![("kind", s("index")), ("present", Bool(true))]),
    )];
    for row in rows {
        match (&row.request_member, &row.response_member) {
            (Some(request), Some(response)) if shown_member(request) && shown_member(response) => {
                out.push(stated(
                    format!("Pairing session {} request {request} response {response}.", row.index),
                    obj(vec![
                        ("kind", s("pairing")),
                        ("session", Num(row.index.to_string())),
                        ("request", s(request)),
                        ("response", s(response)),
                    ]),
                ));
            }
            _ => out.push(refusal_line(format!("Refusal: session {} member name is unusable.", row.index))),
        }
    }
    for orphan in orphans {
        if shown_member(&orphan.name) {
            let missing = if orphan.side == "request" { "response" } else { "request" };
            out.push(refusal_line(format!(
                "Refusal: orphan {} member {}. No {missing} member.",
                orphan.side, orphan.name
            )));
        } else {
            out.push(refusal_line("Refusal: orphan member name is unusable."));
        }
    }
    out
}

fn statements(rows: &[Row], saz: bool, orphans: &[Orphan]) -> Vec<Stated> {
    if rows.is_empty() && !saz {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.extend(status_statements(rows));
    out.extend(redirect_statements(rows));
    out.extend(headers_statements(rows));
    out.extend(body_statements(rows));
    out.extend(timing_statements(rows));
    out.extend(pairing_statements(rows, saz, orphans));
    out
}

fn collect_caps(rows: &[Row], caps: &mut Vec<&'static str>) {
    if rows.iter().any(|row| row.request_headers.omitted_count > 0 || row.response_headers.omitted_count > 0) {
        note_cap(caps, HEADER_CAP_NAME);
    }
    if rows.iter().any(|row| row.request_headers.omitted_name > 0 || row.response_headers.omitted_name > 0) {
        note_cap(caps, HEADER_NAME_CAP_NAME);
    }
    if rows.iter().any(|row| matches!(row.timing, TimingFact::TooLong)) {
        note_cap(caps, TIMING_TEXT_CAP_NAME);
    }
    if rows.iter().any(|row| matches!(row.loc, Loc::TooLong)) {
        note_cap(caps, URL_CAP_NAME);
    }
}

fn render(rows: Vec<Row>, mut caps: Vec<&'static str>, saz: bool, orphans: Vec<Orphan>) -> Read {
    collect_caps(&rows, &mut caps);
    let mut header = vec![TITLE.to_string(), SCOPE.to_string()];
    for name in &caps {
        header.push(cap_line(name));
    }
    let reserve = report_reserve();
    let mut used = header.iter().map(|line| line.len() + 1).sum::<usize>();
    used = used.saturating_add("Entries 64.\n".len());
    let mut emitted = Vec::new();
    let mut cut = false;
    for row in &rows {
        let line = row_line(row);
        if used.saturating_add(line.len()).saturating_add(1).saturating_add(reserve) > REPORT_CAP {
            cut = true;
            break;
        }
        used = used.saturating_add(line.len() + 1);
        emitted.push(row.clone());
    }
    let stated = statements(&emitted, saz, &orphans);
    let mut kept = 0;
    for item in &stated {
        if used.saturating_add(item.line.len()).saturating_add(1).saturating_add(reserve) > REPORT_CAP {
            cut = true;
            break;
        }
        used = used.saturating_add(item.line.len() + 1);
        kept += 1;
    }
    if cut {
        note_cap(&mut caps, REPORT_CAP_NAME);
    }
    let mut report = String::new();
    for line in &header {
        report.push_str(line);
        report.push('\n');
    }
    report.push_str(&format!("Entries {}.\n", emitted.len()));
    for row in &emitted {
        report.push_str(&row_line(row));
        report.push('\n');
    }
    let mut observations = Vec::new();
    for item in stated.iter().take(kept) {
        report.push_str(&item.line);
        report.push('\n');
        observations.push(item.json.clone());
    }
    if cut {
        report.push_str(&cap_line(REPORT_CAP_NAME));
        report.push('\n');
    }
    report.push_str(CONTRACT);
    report.push('\n');
    observations.push(obj(vec![("kind", s("contract")), ("observed", s(OBSERVED)), ("unknown", s(UNKNOWN))]));
    debug_assert!(report.len() <= REPORT_CAP, "{}", report.len());
    Read::Rows(Capture { rows: emitted, caps, report, observations })
}

fn finish(rows: Vec<Row>, saz: bool, orphans: Vec<Orphan>) -> Read {
    let mut caps = Vec::new();
    let mut kept = Vec::new();
    let mut url_cut = false;
    for row in rows {
        if row.url.len() > URL_CAP {
            url_cut = true;
            continue;
        }
        kept.push(row);
    }
    if url_cut {
        note_cap(&mut caps, URL_CAP_NAME);
    }
    if kept.len() > ENTRY_CAP {
        kept.truncate(ENTRY_CAP);
        note_cap(&mut caps, ENTRY_CAP_NAME);
    }
    render(kept, caps, saz, orphans)
}

fn archive_prefix(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x06\x06")
        || bytes.starts_with(b"PK\x06\x07")
}

fn archive_refusal(error: &str) -> &'static str {
    match error {
        "ZIP footer is missing or truncated"
        | "Truncated archive"
        | "Invalid ZIP directory entry"
        | "Invalid ZIP directory bounds"
        | "ZIP directory overflow"
        | "ZIP directory contains extra data"
        | "ZIP local entry differs from directory"
        | "ZIP local path differs from directory"
        | "ZIP descriptor differs from directory"
        | "ZIP data and directory do not meet"
        | "Overlapping or hidden ZIP data"
        | "ZIP member overlaps directory"
        | "Archive offset overflow"
        | "ZIP offset overflow"
        | "Unsupported or invalid ZIP64 footer"
        | "ZIP64 footer conflicts with ZIP directory"
        | "Invalid ZIP entry count"
        | "Required ZIP64 metadata is missing"
        | "Unexpected ZIP64 metadata fields"
        | "Duplicate ZIP64 metadata" => DIRECTORY,
        "ZIP member is damaged" => DAMAGED,
        "Unsafe project path" | "Project path is not UTF-8" => BAD_NAME,
        "ZIP64 value exceeds project limit"
        | "ZIP member exceeds file limit"
        | "Project ZIP must be at most 5 MiB"
        | "Project exceeds 4 MiB data limit"
        | "ZIP size overflow" => READER_LIMIT,
        _ => ARCHIVE_UNUSABLE,
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path)
}

/// `001_c.txt` / `raw/001_s.txt`. The number is the Fiddler session prefix.
fn session_kind(path: &str) -> Option<(u32, u8)> {
    let stem = file_name(path).strip_suffix(".txt")?;
    let (number, kind) = stem.rsplit_once('_')?;
    if kind != "c" && kind != "s" {
        return None;
    }
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((number.parse().ok()?, kind.as_bytes()[0]))
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|window| window == needle)
}

fn split_message(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    if let Some(at) = find_bytes(bytes, b"\r\n\r\n") {
        return Some((&bytes[..at], &bytes[at + 4..]));
    }
    if let Some(at) = find_bytes(bytes, b"\n\n") {
        return Some((&bytes[..at], &bytes[at + 2..]));
    }
    None
}

fn content_type(head: &str) -> Option<String> {
    for line in head.lines().skip(1) {
        let line = line.trim_end_matches('\r');
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.trim().eq_ignore_ascii_case("content-type") {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn request_target(index: usize, bytes: &[u8]) -> Result<(String, String), Read> {
    let head = match split_message(bytes) {
        Some((head, _)) => head,
        None => bytes,
    };
    let head = std::str::from_utf8(head).map_err(|_| refuse_field(index, "request.method"))?;
    let mut parts = head.lines().next().unwrap_or("").trim_end_matches('\r').split_whitespace();
    let method = parts.next().filter(|value| clean(value)).ok_or_else(|| refuse_field(index, "request.method"))?;
    let url = parts.next().filter(|value| clean(value)).ok_or_else(|| refuse_field(index, "request.url"))?;
    Ok((method.to_string(), url.to_string()))
}

fn response_facts(index: usize, bytes: &[u8]) -> Result<(u16, String, u64, BodyFact, Headers, Loc), Read> {
    let Some((head, body)) = split_message(bytes) else {
        return Err(refuse_field(index, "response.content.size"));
    };
    let head = std::str::from_utf8(head).map_err(|_| refuse_field(index, "response.status"))?;
    let mut parts = head.lines().next().unwrap_or("").trim_end_matches('\r').split_whitespace();
    if !parts.next().unwrap_or("").starts_with("HTTP/") {
        return Err(refuse_field(index, "response.status"));
    }
    let status_text = parts
        .next()
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| refuse_field(index, "response.status"))?;
    let status: u64 = status_text.parse().map_err(|_| refuse_field(index, "response.status"))?;
    if status > 999 {
        return Err(refuse_field(index, "response.status"));
    }
    let mime = content_type(head).ok_or_else(|| refuse_field(index, "response.content.mimeType"))?;
    if !clean(&mime) {
        return Err(refuse_field(index, "response.content.mimeType"));
    }
    let (headers, loc) = http_headers(head);
    Ok((status as u16, mime, body.len() as u64, raw_body(body), headers, loc))
}

fn request_parts(index: usize, bytes: &[u8]) -> Result<(String, String, Headers, BodyFact), Read> {
    let (method, url) = request_target(index, bytes)?;
    let (headers, body) = match split_message(bytes) {
        Some((head, body)) => {
            let head = std::str::from_utf8(head).map_err(|_| refuse_field(index, "request.method"))?;
            let (headers, _) = http_headers(head);
            (headers, raw_body(body))
        }
        None => (Headers::not_recorded(), BodyFact::NotRecorded),
    };
    Ok((method, url, headers, body))
}

fn session_row(index: usize, request_name: &str, request: &[u8], response_name: &str, response: &[u8]) -> Result<Row, Read> {
    let (method, url, request_headers, request_body) = request_parts(index, request)?;
    let (status, mime, bytes, response_body, response_headers, mut loc) = response_facts(index, response)?;
    if !is_redirect(status) {
        loc = Loc::Absent;
    }
    Ok(Row {
        index,
        method,
        url,
        status,
        mime,
        bytes,
        request_headers,
        response_headers,
        request_body,
        response_body,
        timing: TimingFact::NotRecorded,
        loc,
        request_member: Some(request_name.to_string()),
        response_member: Some(response_name.to_string()),
    })
}

struct Side {
    name: String,
    data: Vec<u8>,
}

fn interpret_saz(members: Vec<(String, Vec<u8>)>) -> Read {
    let mut has_index = false;
    let mut saw_session = false;
    let mut requests = std::collections::BTreeMap::<u32, Side>::new();
    let mut responses = std::collections::BTreeMap::<u32, Side>::new();
    for (name, data) in members {
        if file_name(&name) == "index.dat" {
            has_index = true;
            continue;
        }
        let Some((id, kind)) = session_kind(&name) else { continue };
        saw_session = true;
        let slot = if kind == b'c' { &mut requests } else { &mut responses };
        if slot.insert(id, Side { name: file_name(&name).to_string(), data }).is_some() {
            return Read::Refused(format!("Refusal: saved capture session {id} is duplicated."));
        }
    }
    if !has_index && !saw_session {
        return Read::Refused(NOT_CAPTURE_ZIP.into());
    }
    if !has_index {
        return Read::Refused(NOT_SESSION.into());
    }
    if requests.is_empty() {
        return Read::Refused(NO_REQUESTS.into());
    }
    let mut rows = Vec::new();
    let mut orphans = Vec::new();
    for (id, request) in &requests {
        let Some(response) = responses.get(id) else {
            orphans.push(Orphan { name: request.name.clone(), side: "request" });
            continue;
        };
        match session_row(*id as usize, &request.name, &request.data, &response.name, &response.data) {
            Ok(row) => rows.push(row),
            Err(refusal) => return refusal,
        }
    }
    for (id, response) in &responses {
        if !requests.contains_key(id) {
            orphans.push(Orphan { name: response.name.clone(), side: "response" });
        }
    }
    if rows.is_empty() {
        let Some((id, _)) = requests.iter().next() else {
            return Read::Refused(NO_REQUESTS.into());
        };
        return refuse_field(*id as usize, "response.status");
    }
    finish(rows, true, orphans)
}

fn read_saz(bytes: &[u8]) -> Read {
    let members = match super::archive::unpack(bytes) {
        Ok(members) => members,
        Err(error) => return Read::Refused(archive_refusal(&error).into()),
    };
    if members.len() > MEMBER_CAP {
        return Read::Refused(MEMBER_CAP_REFUSAL.into());
    }
    if members.iter().any(|(_, data)| data.len() > MEMBER_BYTE_CAP) {
        return Read::Refused(MEMBER_BYTE_REFUSAL.into());
    }
    interpret_saz(members)
}

fn read(bytes: &[u8], complete: bool) -> Read {
    if !complete {
        return Read::Refused(TRUNCATED.into());
    }
    if archive_prefix(bytes) {
        return read_saz(bytes);
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let refusal = if error.error_len().is_none() { TRUNCATED } else { UNUSABLE };
            return Read::Refused(refusal.into());
        }
    };
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return Read::NotCapture;
    }
    match parse(trimmed) {
        Ok(value) => interpret(&value),
        Err(_) if !closed(trimmed) => Read::Refused(TRUNCATED.into()),
        Err(_) => Read::Refused(UNUSABLE.into()),
    }
}

fn view(rows: &[Row], caps: &[&str], refusal: Option<&str>, report: &str, observations: &[Json]) -> Json {
    let entries = rows
        .iter()
        .map(|row| {
            obj(vec![
                ("index", Num(row.index.to_string())),
                ("method", s(&row.method)),
                ("url", s(&row.url)),
                ("status", Num(row.status.to_string())),
                ("mime", s(&row.mime)),
                ("bytes", Num(row.bytes.to_string())),
            ])
        })
        .collect();
    obj(vec![
        ("entries", Arr(entries)),
        ("caps", Arr(caps.iter().copied().map(s).collect())),
        ("refusal", refusal.map(s).unwrap_or(Null)),
        ("report", s(report)),
        ("observations", Arr(observations.to_vec())),
    ])
}

/// `None` leaves an ordinary observation report unchanged.
/// A capture, including a refusal, is attached by the observations route.
pub(super) fn saved_capture(bytes: &[u8]) -> Option<Json> {
    match read(bytes, true) {
        Read::NotCapture => None,
        Read::Rows(capture) => Some(view(&capture.rows, &capture.caps, None, &capture.report, &capture.observations)),
        Read::Refused(reason) => Some(view(&[], &[], Some(&reason), &reason, &[])),
    }
}

fn refused(reason: &str) -> Json {
    view(&[], &[], Some(reason), reason, &[])
}

/// File bytes always produce a view. A document that is not a capture is a
/// named refusal. The report-text route still returns `None` for that case.
pub(super) fn file_capture(bytes: &[u8], complete: bool) -> Json {
    match read(bytes, complete) {
        Read::NotCapture => refused(NOT_FILE),
        Read::Rows(capture) => view(&capture.rows, &capture.caps, None, &capture.report, &capture.observations),
        Read::Refused(reason) => refused(&reason),
    }
}

pub(super) fn file_limit() -> Json {
    refused(FILE_LIMIT)
}

pub(super) fn file_section(capture: &Json) -> String {
    format!("{FILE_SECTION}{}", capture.string())
}

fn section_view(value: &Json) -> bool {
    let Ok(entries) = value.get("entries").and_then(Json::array) else { return false };
    let Ok(caps) = value.get("caps").and_then(Json::array) else { return false };
    let Ok(refusal) = value.get("refusal") else { return false };
    let Ok(report) = value.get("report") else { return false };
    matches!(refusal, Null | Str(_))
        && matches!(report, Str(_))
        && caps.iter().all(|item| matches!(item, Str(_)))
        && entries.iter().all(|row| {
            ["index", "status", "bytes"].iter().all(|key| matches!(row.get(key), Ok(Num(_))))
                && ["method", "url", "mime"].iter().all(|key| matches!(row.get(key), Ok(Str(_))))
        })
}

/// The inspect display carries rows already parsed from file bytes.
/// This does not parse the display as a capture document.
pub(super) fn section_capture(report: &str) -> Option<Json> {
    let line = report.split_once(FILE_SECTION)?.1.lines().next()?;
    let value = parse(line).ok()?;
    section_view(&value).then_some(value)
}

#[cfg(test)]
mod checks {
    use super::*;

    fn one(method: &str, url: &str, status: u16, mime: &str, size: u64) -> String {
        format!(
            r#"{{"request":{{"method":"{method}","url":"{url}","headers":[{{"name":"Content-Type","value":"text/plain"}}]}},"response":{{"status":{status},"content":{{"mimeType":"{mime}","size":{size}}},"bodySize":99999}}}}"#
        )
    }

    fn document(entries: &[String]) -> String {
        format!(
            r#"{{"log":{{"version":"1.2","creator":{{"name":"recorder","version":"1"}},"entries":[{}]}}}}"#,
            entries.join(",")
        )
    }

    fn rows(bytes: &[u8]) -> Capture {
        match read(bytes, true) {
            Read::Rows(capture) => capture,
            Read::Refused(reason) => panic!("expected rows, refused: {reason}"),
            Read::NotCapture => panic!("expected rows, document was not a capture"),
        }
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn zip(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut locals = Vec::new();
        let mut index = Vec::new();
        for (name, data) in members {
            let offset = locals.len() as u32;
            let name_bytes = name.as_bytes();
            push_u32(&mut locals, 0x04034b50);
            push_u16(&mut locals, 20);
            push_u16(&mut locals, 0);
            push_u16(&mut locals, 0);
            push_u16(&mut locals, 0);
            push_u16(&mut locals, 0);
            push_u32(&mut locals, crc32(data));
            push_u32(&mut locals, data.len() as u32);
            push_u32(&mut locals, data.len() as u32);
            push_u16(&mut locals, name_bytes.len() as u16);
            push_u16(&mut locals, 0);
            locals.extend_from_slice(name_bytes);
            locals.extend_from_slice(data);
            index.push(((*name).to_string(), data.to_vec(), offset));
        }
        let dir_at = locals.len();
        let mut out = locals;
        for (name, data, offset) in &index {
            let name_bytes = name.as_bytes();
            push_u32(&mut out, 0x02014b50);
            push_u16(&mut out, 20);
            push_u16(&mut out, 20);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u32(&mut out, crc32(data));
            push_u32(&mut out, data.len() as u32);
            push_u32(&mut out, data.len() as u32);
            push_u16(&mut out, name_bytes.len() as u16);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u32(&mut out, 0);
            push_u32(&mut out, *offset);
            out.extend_from_slice(name_bytes);
        }
        let dir_size = out.len() - dir_at;
        let count = index.len() as u16;
        push_u32(&mut out, 0x06054b50);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, count);
        push_u16(&mut out, count);
        push_u32(&mut out, dir_size as u32);
        push_u32(&mut out, dir_at as u32);
        push_u16(&mut out, 0);
        out
    }

    fn kind_is(row: &Json, kind: &str) -> bool {
        row.get("kind").and_then(|value| value.text()).ok() == Some(kind)
    }

    #[test]
    fn valid_capture_yields_method_url_status_mime_and_byte_count() {
        // Fails if method/url/status are swapped, if MIME comes from the request header,
        // or if the byte count is bodySize (99999) or the URL length instead of content.size.
        let raw = document(&[
            one("POST", "https://example.test/submit", 201, "application/json", 40),
            one("GET", "https://example.test/item", 200, "text/html", 12),
        ]);
        let capture = rows(raw.as_bytes());
        assert!(capture.caps.is_empty(), "{:?}", capture.caps);
        assert_eq!(capture.rows.len(), 2);
        assert_eq!(capture.rows[0].index, 0);
        assert_eq!(capture.rows[0].method, "POST");
        assert_eq!(capture.rows[0].url, "https://example.test/submit");
        assert_eq!(capture.rows[0].status, 201);
        assert_eq!(capture.rows[0].mime, "application/json");
        assert_eq!(capture.rows[0].bytes, 40);
        assert_eq!(capture.rows[1].method, "GET");
        assert_eq!(capture.rows[1].url, "https://example.test/item");
        assert_eq!(capture.rows[1].status, 200);
        assert_eq!(capture.rows[1].mime, "text/html");
        assert_eq!(capture.rows[1].bytes, 12);
        assert!(capture.report.contains("https://example.test/submit"));
        assert!(capture.report.contains("text/html"));
        assert!(!capture.report.contains("99999"), "{}", capture.report);
        assert!(!capture.report.contains("text/plain"), "{}", capture.report);
    }

    #[test]
    fn truncated_capture_is_refused() {
        // Fails if an unfinished file is still emitted as rows, or if a cut token
        // is reported as unusable JSON instead of a truncated capture.
        let whole = document(&[one("GET", "https://example.test/item", 200, "text/html", 12)]);
        match read(whole.as_bytes(), false) {
            Read::Refused(reason) => {
                assert!(reason.contains("truncated"), "{reason}");
                assert!(!reason.contains("unusable"), "{reason}");
            }
            other => panic!("incomplete file was not refused: {other:?}"),
        }
        let cut = br#"{"log":{"version":"1.2","entries":[{"request":{"method":"GET","url":"https://example.test/cut"#;
        match read(cut, true) {
            Read::Refused(reason) => {
                assert!(reason.contains("truncated"), "{reason}");
                assert!(!reason.contains("https://example.test/cut"), "{reason}");
            }
            other => panic!("cut capture was not refused: {other:?}"),
        }
    }

    #[test]
    fn missing_required_field_names_the_entry() {
        // Fails if the refusal names entry 0, omits the index, or still returns the valid sibling row.
        let broken = r#"{"request":{"method":"GET","url":"https://example.test/broken"},"response":{"content":{"mimeType":"text/plain","size":4}}}"#;
        let raw = document(&[
            one("GET", "https://example.test/ok", 200, "text/html", 8),
            broken.to_string(),
        ]);
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert!(reason.contains("entry 1 missing response.status"), "{reason}");
                assert!(!reason.contains("entry 0"), "{reason}");
            }
            other => panic!("broken entry was not refused: {other:?}"),
        }
    }

    #[test]
    fn entry_cap_names_itself_when_it_cuts() {
        // Fails if entry 64 is kept, if the extra entry disappears without the cap name,
        // or if the report cap is blamed for a short list.
        let entries = (0..=ENTRY_CAP).map(|index| one("GET", &format!("https://e.test/{index}"), 200, "text/plain", 1)).collect::<Vec<_>>();
        let capture = rows(document(&entries).as_bytes());
        assert_eq!(capture.rows.len(), ENTRY_CAP);
        assert!(capture.caps.contains(&ENTRY_CAP_NAME), "{:?}", capture.caps);
        assert!(!capture.caps.contains(&REPORT_CAP_NAME), "{:?}", capture.caps);
        assert!(capture.report.contains(ENTRY_CAP_NAME), "{}", capture.report);
        assert!(!capture.rows.iter().any(|row| row.index == ENTRY_CAP));
        assert!(capture.report.len() <= REPORT_CAP);
    }

    #[test]
    fn url_cap_names_itself_when_it_cuts() {
        // Fails if the long URL is stored, shortened, or dropped without the cap name,
        // or if the later short entry is discarded with it.
        let long = format!("https://e.test/{}", "URL-MARKER-".repeat(URL_CAP));
        assert!(long.len() > URL_CAP);
        let raw = document(&[
            one("GET", &long, 200, "text/plain", 2),
            one("HEAD", "https://e.test/ok", 204, "text/plain", 0),
        ]);
        let capture = rows(raw.as_bytes());
        assert_eq!(capture.rows.len(), 1);
        assert_eq!(capture.rows[0].url, "https://e.test/ok");
        assert_eq!(capture.rows[0].method, "HEAD");
        assert!(capture.caps.contains(&URL_CAP_NAME), "{:?}", capture.caps);
        assert!(capture.report.contains(URL_CAP_NAME), "{}", capture.report);
        assert!(!capture.report.contains("URL-MARKER-"), "{}", capture.report);
        assert!(capture.rows.iter().all(|row| row.url.len() <= URL_CAP));
    }

    #[test]
    fn report_cap_names_itself_when_it_cuts() {
        // Fails if every row is kept, if the report grows past the cap,
        // or if the oversized line is shortened without the cap name.
        let mime = "M".repeat(REPORT_CAP);
        let raw = document(&[one("GET", "https://e.test/wide", 200, &mime, 9)]);
        let capture = rows(raw.as_bytes());
        assert!(capture.rows.is_empty(), "{}", capture.rows.len());
        assert_eq!(capture.caps, vec![REPORT_CAP_NAME]);
        assert!(capture.report.contains(REPORT_CAP_NAME), "{}", capture.report);
        assert!(!capture.report.contains("MMMMMMMM"), "{}", capture.report);
        assert!(capture.report.len() <= REPORT_CAP, "{}", capture.report.len());
    }

    #[test]
    fn unusable_json_is_refused() {
        // Fails if a balanced but illegal capture is called truncated, or if entries of the wrong type become rows.
        let raw = r#"{"log":{"version":"1.2","entries":{"not":"an array"}}}"#;
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert!(reason.contains("unusable"), "{reason}");
                assert!(!reason.contains("truncated"), "{reason}");
            }
            other => panic!("unusable capture was not refused: {other:?}"),
        }
    }

    #[test]
    fn page_text_is_not_a_saved_capture() {
        // Fails if an ordinary retained page report grows a saved_capture object on the observations route.
        assert!(saved_capture(b"<html><p>not a capture</p></html>").is_none());
        assert!(saved_capture(br#"{"page":"ok"}"#).is_none());
        let raw = document(&[one("GET", "https://example.test/item", 200, "text/html", 12)]);
        let attached = saved_capture(raw.as_bytes()).expect("capture reaches the evidence object");
        let entries = attached.get("entries").unwrap().array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].get("method").unwrap().text().unwrap(), "GET");
        assert_eq!(entries[0].get("bytes").unwrap(), &Num("12".into()));
        assert_eq!(attached.get("refusal").unwrap(), &Null);
    }

    #[test]
    fn method_status_class_redaction_and_body_digest_are_stated() {
        // Fails if the class is omitted, a sensitive value is copied, or the body text is used in place of its digest.
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256(b"body-marker-xyz"), "0120cc7f8974d602cba67f1a9e22e7d61c048f9c6fbb4e176c5de411c1629099");
        let raw = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://example.test/submit","headers":[{"name":"Authorization","value":"Bearer SUPERSECRETVALUE"},{"name":"Cookie","value":"session=SUPERSECRETVALUE"},{"name":"Proxy-Authorization","value":"Basic SUPERSECRETVALUE"},{"name":"X-Api-Key","value":"SUPERSECRETVALUE"},{"name":"Accept","value":"text/csv-secret"}],"postData":{"mimeType":"application/json","text":"body-marker-xyz"}},"response":{"status":201,"headers":[{"name":"Set-Cookie","value":"a=SUPERSECRETVALUE"}],"content":{"mimeType":"application/json","size":3,"text":"abc"}}}]}}"#;
        let capture = rows(raw.as_bytes());
        assert_eq!(capture.rows[0].method, "POST");
        assert_eq!(capture.rows[0].status, 201);
        assert!(capture.report.contains("Status 0 method POST status 201 class 2xx."), "{}", capture.report);
        assert!(capture.report.contains("0 POST https://example.test/submit 201 2xx application/json 3"), "{}", capture.report);
        for name in ["Authorization", "Cookie", "Proxy-Authorization", "X-Api-Key", "Set-Cookie"] {
            assert!(capture.report.contains(&format!("{name} {REDACTED}.")), "{name} missing in {}", capture.report);
        }
        assert!(capture.report.contains("Header 0 request Accept."), "{}", capture.report);
        assert!(capture.report.contains("Body 0 request present length 15 sha256 0120cc7f8974d602cba67f1a9e22e7d61c048f9c6fbb4e176c5de411c1629099."), "{}", capture.report);
        assert!(capture.report.contains("Body 0 response present length 3 sha256 ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad."), "{}", capture.report);
        assert!(capture.report.ends_with(&format!("{CONTRACT}\n")), "{}", capture.report);
        assert!(capture.report.contains("It is not a replay."));
        let view = saved_capture(raw.as_bytes()).expect("capture view");
        let encoded = view.string();
        assert_eq!(encoded.matches("SUPERSECRETVALUE").count(), 0, "{encoded}");
        assert_eq!(capture.report.matches("body-marker-xyz").count(), 0, "{}", capture.report);
        assert!(!capture.report.contains("text/csv-secret"), "{}", capture.report);
        assert!(!encoded.contains("abc\""), "{encoded}");
        let observations = view.get("observations").unwrap().array().unwrap();
        let redacted = observations.iter().filter(|row| kind_is(row, "header") && row.get("redacted").ok() == Some(&Bool(true))).count();
        assert_eq!(redacted, 5, "{observations:?}");
        assert!(observations.iter().any(|row| kind_is(row, "header") && row.get("name").and_then(|v| v.text()).ok() == Some("Accept") && row.get("value").is_err()));
        assert!(observations.iter().any(|row| kind_is(row, "contract") && row.get("observed").and_then(|v| v.text()).ok() == Some(OBSERVED)));
    }

    #[test]
    fn redirect_pairing_is_recorded_and_a_missing_location_is_a_different_refusal() {
        // Fails if a later URL is described as behaviour that was followed, or if removing Location keeps the pairing line.
        let paired = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://example.test/start"},"response":{"status":302,"headers":[{"name":"Location","value":"https://example.test/next"}],"content":{"mimeType":"text/html","size":0}}},{"request":{"method":"GET","url":"https://example.test/next"},"response":{"status":200,"content":{"mimeType":"text/html","size":1}}},{"request":{"method":"GET","url":"https://example.test/other"},"response":{"status":302,"headers":[{"name":"Location","value":"https://example.test/absent"}],"content":{"mimeType":"text/html","size":0}}}]}}"#;
        let capture = rows(paired.as_bytes());
        let pairing = "Redirect 0 recorded pairing entry 1 target https://example.test/next.";
        let unmatched = "Redirect 2 location not followed by a recorded request. target https://example.test/absent.";
        assert!(capture.report.contains(pairing), "{}", capture.report);
        assert!(capture.report.contains(unmatched), "{}", capture.report);
        assert!(capture.report.contains("Redirect 1 not a 3xx."), "{}", capture.report);
        assert!(!capture.report.contains("followed the redirect"), "{}", capture.report);
        let missing = paired.replace(r#""headers":[{"name":"Location","value":"https://example.test/next"}]"#, r#""headers":[]"#);
        let changed = rows(missing.as_bytes());
        let refusal = "Refusal: entry 0 redirect has no location.";
        assert!(changed.report.contains(refusal), "{}", changed.report);
        assert!(!changed.report.contains(pairing), "{}", changed.report);
        assert_ne!(
            capture.report.lines().find(|line| line.contains("Redirect 0")).unwrap(),
            changed.report.lines().find(|line| line.contains("entry 0 redirect") || line.starts_with("Redirect 0")).unwrap()
        );
    }

    #[test]
    fn timing_absent_says_not_recorded() {
        // Fails if a capture with no time or timings object is described as measured or recorded.
        let raw = document(&[one("GET", "https://example.test/item", 200, "text/html", 12)]);
        let capture = rows(raw.as_bytes());
        assert!(capture.report.contains("Timing: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("recorded by the capture"), "{}", capture.report);
    }

    #[test]
    fn timing_present_is_recorded_by_the_capture() {
        // Fails if the elapsed token is dropped, rewritten, or labeled as an RLLM measurement.
        let raw = r#"{"log":{"entries":[{"time":15,"request":{"method":"GET","url":"https://example.test/item"},"response":{"status":200,"content":{"mimeType":"text/html","size":12}}}]}}"#;
        let capture = rows(raw.as_bytes());
        assert!(capture.report.contains("Timing 0 elapsed 15 ms recorded by the capture, not measured by RLLM."), "{}", capture.report);
        assert!(!capture.report.contains("Timing: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("Timing 0 not recorded."), "{}", capture.report);
    }

    #[test]
    fn header_cap_lists_exactly_the_cap_and_discloses_the_rest() {
        // Fails if header 8 is kept, if the kept count is not the cap, or if the extra name disappears without the cap.
        let headers = (0..=HEADER_CAP)
            .map(|index| format!(r#"{{"name":"n{index}","value":"hidden-{index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let raw = format!(
            r#"{{"log":{{"entries":[{{"request":{{"method":"GET","url":"https://e.test/h","headers":[{headers}]}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
        );
        let capture = rows(raw.as_bytes());
        let names = capture.report.lines().filter(|line| line.starts_with("Header 0 request n")).count();
        assert_eq!(names, HEADER_CAP, "{}", capture.report);
        assert!(capture.caps.contains(&HEADER_CAP_NAME), "{:?}", capture.caps);
        assert!(capture.report.contains(&format!("Headers 0 request omitted 1 by {HEADER_CAP_NAME}.")), "{}", capture.report);
        assert!(!capture.report.contains("Header 0 request n8."), "{}", capture.report);
        assert!(!capture.report.contains("hidden-"), "{}", capture.report);
    }

    #[test]
    fn header_name_cap_keeps_a_name_at_the_bound_and_discloses_past_it() {
        // Fails if a 65-character name is copied, shortened into the census, or dropped without the cap name.
        let at_bound = "N".repeat(HEADER_NAME_CAP);
        let over = "N".repeat(HEADER_NAME_CAP + 1);
        let raw = |name: &str| {
            format!(
                r#"{{"log":{{"entries":[{{"request":{{"method":"GET","url":"https://e.test/n","headers":[{{"name":"{name}","value":"secret-name"}}]}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
            )
        };
        let kept = rows(raw(&at_bound).as_bytes());
        assert!(kept.report.contains(&format!("Header 0 request {at_bound}.")), "{}", kept.report);
        assert!(!kept.caps.contains(&HEADER_NAME_CAP_NAME), "{:?}", kept.caps);
        assert_eq!(at_bound.len(), HEADER_NAME_CAP);
        let cut = rows(raw(&over).as_bytes());
        assert!(cut.caps.contains(&HEADER_NAME_CAP_NAME), "{:?}", cut.caps);
        assert!(cut.report.contains(&format!("omitted 1 by {HEADER_NAME_CAP_NAME}.")), "{}", cut.report);
        assert!(!cut.report.contains(&over), "{}", cut.report);
        assert!(!cut.report.contains("secret-name"), "{}", cut.report);
        assert_eq!(cut.report.lines().filter(|line| line.starts_with("Header 0 request ")).count(), 0);
    }

    #[test]
    fn timing_text_cap_keeps_the_recorded_token_at_the_bound() {
        // Fails if a 33-digit elapsed time is copied or shortened to 32 digits without the cap name.
        let at_bound = "1".repeat(TIMING_TEXT_CAP);
        let over = "1".repeat(TIMING_TEXT_CAP + 1);
        let raw = |time: &str| {
            format!(
                r#"{{"log":{{"entries":[{{"time":{time},"request":{{"method":"GET","url":"https://e.test/t"}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
            )
        };
        let kept = rows(raw(&at_bound).as_bytes());
        assert!(kept.report.contains(&format!("Timing 0 elapsed {at_bound} ms recorded by the capture, not measured by RLLM.")), "{}", kept.report);
        assert!(!kept.caps.contains(&TIMING_TEXT_CAP_NAME), "{:?}", kept.caps);
        assert_eq!(at_bound.len(), TIMING_TEXT_CAP);
        let cut = rows(raw(&over).as_bytes());
        assert!(cut.caps.contains(&TIMING_TEXT_CAP_NAME), "{:?}", cut.caps);
        assert!(cut.report.contains("Refusal: entry 0 timing exceeds the saved-capture timing cap."), "{}", cut.report);
        assert!(!cut.report.contains(&over), "{}", cut.report);
        assert!(!cut.report.contains(&format!("elapsed {at_bound}")), "{}", cut.report);
    }

    #[test]
    fn body_bound_refusal_changes_when_one_byte_is_removed() {
        // Fails if an over-bound body is hashed, or if the in-bound body keeps the refusal and drops the digest.
        let marker = "B".repeat(8);
        let over_len = MEMBER_BYTE_CAP + 1;
        let raw = |len: usize| {
            let body = "B".repeat(len);
            format!(
                r#"{{"log":{{"entries":[{{"request":{{"method":"POST","url":"https://e.test/b","postData":{{"text":"{body}"}}}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
            )
        };
        let over = rows(raw(over_len).as_bytes());
        let over_line = format!("Refusal: entry 0 request body exceeds the saved-capture member byte cap. size {over_len}.");
        assert!(over.report.contains(&over_line), "{}", over.report);
        assert!(!over.report.contains("sha256"), "{}", over.report);
        assert!(!over.report.contains(&marker), "{}", over.report);
        let at = rows(raw(MEMBER_BYTE_CAP).as_bytes());
        let at_line = at.report.lines().find(|line| line.starts_with("Body 0 request present")).expect("in-bound body row");
        assert!(at_line.contains(&format!("length {MEMBER_BYTE_CAP}")), "{at_line}");
        assert!(at_line.contains("sha256 "), "{at_line}");
        assert!(!at.report.contains("exceeds the saved-capture member byte cap"), "{}", at.report);
        assert!(!at.report.contains(&marker), "{}", at.report);
        assert_ne!(over_line, at_line);
    }

    fn session_archive(orphan: &str) -> Vec<u8> {
        let first = b"GET https://example.test/start HTTP/1.1\r\nHost: example.test\r\nAuthorization: Bearer SUPERSECRETVALUE\r\n\r\nREQBODY";
        let redirect = b"HTTP/1.1 302 Found\r\nLocation: https://example.test/next\r\nSet-Cookie: a=SUPERSECRETVALUE\r\nContent-Type: text/html\r\n\r\nNEXTBODY";
        let orphan_bytes = b"GET https://example.test/orphan HTTP/1.1\r\nAuthorization: Bearer SUPERSECRETVALUE\r\n\r\nORPHANBODY";
        let next = b"GET https://example.test/next HTTP/1.1\r\nHost: example.test\r\n\r\n";
        let done = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nBODYOKMARK";
        zip(&[
            ("index.dat", b"session"),
            ("001_c.txt", first),
            ("001_s.txt", redirect),
            (orphan, orphan_bytes),
            ("003_c.txt", next),
            ("003_s.txt", done),
        ])
    }

    #[test]
    fn saz_pairs_members_and_the_orphan_side_changes_the_refusal() {
        // Fails if 002 is paired, if the orphan refusal stays the same after the member suffix changes,
        // or if a header value or body byte from the archive is copied.
        let request_orphan = rows(&session_archive("002_c.txt"));
        assert!(request_orphan.report.contains("index.dat present."), "{}", request_orphan.report);
        assert!(request_orphan.report.contains("Pairing session 1 request 001_c.txt response 001_s.txt."), "{}", request_orphan.report);
        assert!(request_orphan.report.contains("Pairing session 3 request 003_c.txt response 003_s.txt."), "{}", request_orphan.report);
        let request_refusal = "Refusal: orphan request member 002_c.txt. No response member.";
        assert!(request_orphan.report.contains(request_refusal), "{}", request_orphan.report);
        assert!(request_orphan.report.contains("Redirect 1 recorded pairing entry 3 target https://example.test/next."), "{}", request_orphan.report);
        assert!(request_orphan.report.contains("Authorization redacted."), "{}", request_orphan.report);
        assert!(request_orphan.report.contains("Set-Cookie redacted."), "{}", request_orphan.report);
        assert_eq!(request_orphan.rows.len(), 2);
        assert_eq!(request_orphan.rows[0].status, 302);
        assert_eq!(request_orphan.rows[1].method, "GET");
        let response_orphan = rows(&session_archive("002_s.txt"));
        let response_refusal = "Refusal: orphan response member 002_s.txt. No request member.";
        assert!(response_orphan.report.contains(response_refusal), "{}", response_orphan.report);
        assert!(!response_orphan.report.contains(request_refusal), "{}", response_orphan.report);
        assert!(response_orphan.report.contains("Pairing session 1 request 001_c.txt response 001_s.txt."), "{}", response_orphan.report);
        assert_ne!(request_refusal, response_refusal);
        for marker in ["SUPERSECRETVALUE", "REQBODY", "NEXTBODY", "ORPHANBODY", "BODYOKMARK"] {
            assert!(!request_orphan.report.contains(marker), "{marker} in {}", request_orphan.report);
            assert!(!response_orphan.report.contains(marker), "{marker} in {}", response_orphan.report);
        }
        assert!(request_orphan.report.ends_with(&format!("{CONTRACT}\n")));
    }

    #[test]
    fn unclassable_status_is_a_refusal_row() {
        // Fails if status 99 is given a class, or if the other classed rows are dropped with it.
        let raw = document(&[
            one("GET", "https://e.test/continue", 101, "text/plain", 1),
            one("GET", "https://e.test/missing", 404, "text/plain", 1),
            one("GET", "https://e.test/broken", 500, "text/plain", 1),
            one("GET", "https://e.test/odd", 99, "text/plain", 1),
        ]);
        let capture = rows(raw.as_bytes());
        assert_eq!(capture.rows.len(), 4);
        assert!(capture.report.contains("Status 0 method GET status 101 class 1xx."), "{}", capture.report);
        assert!(capture.report.contains("Status 1 method GET status 404 class 4xx."), "{}", capture.report);
        assert!(capture.report.contains("Status 2 method GET status 500 class 5xx."), "{}", capture.report);
        assert!(capture.report.contains("Refusal: entry 3 status class is unanswerable."), "{}", capture.report);
        assert!(!capture.report.lines().any(|line| line.starts_with("3 GET") && line.contains(" 99 ") && line.contains("xx")), "{}", capture.report);
    }
}

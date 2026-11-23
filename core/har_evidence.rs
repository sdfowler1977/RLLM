//! Saved HTTP Archive and SAZ captures, read as the same evidence rows.
//! Each answer is a bounded row: method, status, status class, header names, body
//! length and digest or text-representation length and digest, recorded timing,
//! redirect pairing, and session-archive member pairing.
//! A missing answer is a named refusal. Header values and body bytes are not copied.
//! It is not a replay.
//! A session archive is a ZIP of per-session messages (`NNN_c.txt`, `NNN_s.txt`)
//! and optional session metadata (`NNN_m.xml`). `index.dat` may be absent.
use super::{obj, parse, s, Json, Json::*};
use std::cell::Cell;
use std::time::{Duration, Instant};

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
const OBSERVED: &str = "method, status, class, header names, body length and digest or text-representation length and digest, recorded timings, and SAZ member pairing";
const UNKNOWN: &str = "replay, behaviour, header values, and body bytes";
const CONTRACT: &str = "Observed from bytes: method, status, class, header names, body length and digest or text-representation length and digest, recorded timings, and SAZ member pairing. Unknown: replay, behaviour, header values, and body bytes. It is not a replay.";
const REDACTED: &str = "redacted";
const TRUNCATED: &str = "Refusal: saved capture truncated.";
const UNUSABLE: &str = "Refusal: saved capture JSON is unusable.";
const DIRECTORY: &str = "Refusal: saved capture archive directory is corrupt or truncated.";
const NO_REQUESTS: &str = "Refusal: saved capture has no request members.";
#[allow(dead_code)]
const NOT_SESSION: &str = "Refusal: saved capture is not a session archive.";
const NOT_CAPTURE_ZIP: &str = "Refusal: archive is not a saved capture.";
const NOT_FILE: &str = "Refusal: file is not a saved capture.";
const FILE_LIMIT: &str = "Refusal: saved capture file exceeds the reader limit.";
const DOCUMENT_BYTE_CAP: usize = 8 * 1024 * 1024;
const PARTIAL_STOP: &str = "Refusal: saved capture read is partial because the operation was cancelled.";
const PARTIAL_DEADLINE: &str = "Refusal: saved capture read is partial because the operation deadline passed.";
const READ_BUDGET: Duration = Duration::from_secs(120);
const SESSION_ID_LIMIT: &str = "Refusal: saved capture session id exceeds the reader limit. The session was not read.";
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

/// Which bytes a length and digest were measured from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyMeasure {
    /// HTTP message body bytes.
    Body,
    /// UTF-8 bytes of a capture text field. This is not `content.size`.
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BodyFact {
    NotRecorded,
    Present { len: u64, sha256: String, measure: BodyMeasure },
    OverBound { len: u64, measure: BodyMeasure },
    EncodingUnread { len: u64 },
    /// `postData.params` was present and `text` was not. The count is the recorded parameter list.
    Params { count: u64 },
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
    integer_number(text)
}

/// A published capture number may be an integer spelling, a zero fraction, or an exponent.
/// The value is kept only when it is a whole count that fits in `u64`. A fraction, a sign,
/// or a magnitude past that range is absent — it is not wrapped and not reported as zero.
fn integer_number(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 40 {
        return None;
    }
    if text.bytes().any(|b| !matches!(b, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    let (coeff, exp_text) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], &text[at + 1..]),
        None => (text, ""),
    };
    if coeff.is_empty() {
        return None;
    }
    let exp = if exp_text.is_empty() {
        if text.as_bytes().contains(&b'e') || text.as_bytes().contains(&b'E') {
            return None;
        }
        0i32
    } else {
        let (sign, digits) = if let Some(rest) = exp_text.strip_prefix('+') {
            (1i32, rest)
        } else if let Some(rest) = exp_text.strip_prefix('-') {
            (-1i32, rest)
        } else {
            (1i32, exp_text)
        };
        if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        sign.checked_mul(digits.parse::<i32>().ok()?)?
    };
    let (whole_digits, frac_digits) = match coeff.split_once('.') {
        Some((whole_digits, frac_digits)) => (whole_digits, frac_digits),
        None => (coeff, ""),
    };
    if whole_digits.is_empty()
        || !whole_digits.bytes().all(|b| b.is_ascii_digit())
        || !frac_digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut digits = Vec::new();
    digits.extend(whole_digits.bytes().skip_while(|b| *b == b'0'));
    let point = digits.len() as i32;
    digits.extend(frac_digits.bytes());
    let new_point = point.checked_add(exp)?;
    if new_point < 0 {
        return None;
    }
    if (new_point as usize) < digits.len() {
        if digits[new_point as usize..].iter().any(|b| *b != b'0') {
            return None;
        }
        digits.truncate(new_point as usize);
    } else {
        let extra = (new_point as usize) - digits.len();
        if extra > 20 {
            return None;
        }
        digits.extend(std::iter::repeat(b'0').take(extra));
    }
    if digits.is_empty() {
        return Some(0);
    }
    if digits.len() > 20 {
        return None;
    }
    std::str::from_utf8(&digits).ok()?.parse().ok()
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

/// RFC 9110 `tchar`: a field name is a non-empty token.
fn header_name_chars(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
                )
        })
}

thread_local! {
    static READ_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// `Some` is the cancellation token of the read now on this thread.
    /// `None` means the read is unscoped and follows the process-wide flag.
    static READ_CANCEL: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Test probe: `Some(n)` lets the next `n` deadline checks pass, then the deadline has passed.
#[cfg(any(test, rllm_module_tests))]
thread_local! {
    static DEADLINE_CHECKS_LEFT: Cell<Option<u32>> = const { Cell::new(None) };
}

fn deadline_passed() -> bool {
    #[cfg(any(test, rllm_module_tests))]
    {
        let forced = DEADLINE_CHECKS_LEFT.with(|slot| match slot.get() {
            Some(0) => Some(true),
            Some(count) => {
                slot.set(Some(count - 1));
                Some(false)
            }
            None => None,
        });
        if let Some(passed) = forced {
            return passed;
        }
    }
    READ_DEADLINE.with(|slot| slot.get().is_some_and(|deadline| Instant::now() >= deadline))
}

/// A scoped read stops when its own token is cancelled. An unscoped read stops
/// on the process-wide cancel flag. Either read also stops when its deadline passed.
fn read_cancelled() -> bool {
    READ_CANCEL.with(|slot| match slot.get() {
        Some(token) if super::execution::scoped_token(token) => super::execution::token_cancelled(token),
        _ => super::execution::cancelled(),
    })
}

fn operation_stopped() -> bool {
    read_cancelled() || deadline_passed()
}

fn stopped_read() -> Read {
    if read_cancelled() {
        Read::Refused(PARTIAL_STOP.into())
    } else {
        Read::Refused(PARTIAL_DEADLINE.into())
    }
}

struct CancelGuard {
    previous: Option<u64>,
}

impl CancelGuard {
    fn bind(token: Option<u64>) -> Self {
        let previous = READ_CANCEL.with(|slot| {
            let previous = slot.get();
            slot.set(token);
            previous
        });
        Self { previous }
    }
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        READ_CANCEL.with(|slot| slot.set(previous));
    }
}

fn guard_running() -> Result<(), Read> {
    if operation_stopped() {
        Err(stopped_read())
    } else {
        Ok(())
    }
}

struct DeadlineGuard(bool);

impl Drop for DeadlineGuard {
    fn drop(&mut self) {
        if self.0 {
            READ_DEADLINE.with(|slot| slot.set(None));
        }
    }
}

/// The local budget is a ceiling. An earlier caller deadline stays earlier.
/// A fresh 120-second budget is used only when no caller deadline is active.
fn arm_read_deadline(caller: Option<Instant>) -> bool {
    let ceiling = Instant::now() + READ_BUDGET;
    let proposed = match caller {
        Some(deadline) if deadline < ceiling => deadline,
        _ => ceiling,
    };
    READ_DEADLINE.with(|slot| match slot.get() {
        Some(active) => {
            if proposed < active {
                slot.set(Some(proposed));
            }
            false
        }
        None => {
            slot.set(Some(proposed));
            true
        }
    })
}

/// RFC 9112: `HTTP-version = HTTP-name "/" DIGIT "." DIGIT`, and the name is case-sensitive.
fn http_version(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 8 && bytes.starts_with(b"HTTP/") && bytes[5].is_ascii_digit() && bytes[6] == b'.' && bytes[7].is_ascii_digit()
}

/// Digest of a body the reader is willing to retain. A longer input, or a bit
/// length that does not fit, returns `None` so no digest is claimed.
fn sha256(bytes: &[u8]) -> Option<String> {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    if bytes.len() > MEMBER_BYTE_CAP || operation_stopped() {
        return None;
    }
    let bits = (bytes.len() as u64).checked_mul(8)?;
    let mut h = [0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut block = [0u8; 64];
    let mut filled = 0usize;
    let compress = |h: &mut [u32; 8], chunk: &[u8; 64]| {
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
    };
    for &byte in bytes {
        block[filled] = byte;
        filled += 1;
        if filled == 64 {
            compress(&mut h, &block);
            filled = 0;
            if operation_stopped() {
                return None;
            }
        }
    }
    block[filled] = 0x80;
    filled += 1;
    if filled > 56 {
        while filled < 64 {
            block[filled] = 0;
            filled += 1;
        }
        compress(&mut h, &block);
        block = [0u8; 64];
        filled = 0;
    }
    while filled < 56 {
        block[filled] = 0;
        filled += 1;
    }
    block[56..64].copy_from_slice(&bits.to_be_bytes());
    compress(&mut h, &block);
    Some(h.iter().map(|v| format!("{v:08x}")).collect())
}

enum Scan {
    Closed,
    Open,
    Stopped,
}

/// A document that still has an open string, object, or array was cut.
/// A balanced document that still fails to parse is unusable JSON.
/// Depth uses checked arithmetic, so a hostile nest cannot wrap into "closed".
fn closure(text: &str) -> Scan {
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (seen, byte) in text.bytes().enumerate() {
        if seen % 4096 == 0 && operation_stopped() {
            return Scan::Stopped;
        }
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
            b'{' | b'[' => match depth.checked_add(1) {
                Some(next) => depth = next,
                None => return Scan::Open,
            },
            b'}' | b']' => match depth.checked_sub(1) {
                Some(next) => depth = next,
                None => return Scan::Open,
            },
            _ => {}
        }
    }
    if operation_stopped() {
        return Scan::Stopped;
    }
    if !in_string && depth == 0 { Scan::Closed } else { Scan::Open }
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
        *omitted_name = omitted_name.saturating_add(1);
        return true;
    }
    if listed.len() >= HEADER_CAP {
        *omitted_count = omitted_count.saturating_add(1);
        return true;
    }
    listed.push(HeaderItem { name: name.to_string(), redacted: sensitive(name) });
    true
}

fn parse_header_array(items: &[Json]) -> Result<(Headers, Loc), Read> {
    let mut listed = Vec::new();
    let mut omitted_count = 0;
    let mut omitted_name = 0;
    let mut loc = Loc::Absent;
    for (seen, item) in items.iter().enumerate() {
        if seen % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        let Some(name) = at(item, "name").and_then(|value| match value {
            Str(text) => Some(text.as_str()),
            _ => None,
        }) else {
            return Ok((Headers::unusable(), Loc::Unusable));
        };
        // A field name is a token. Leading or trailing whitespace is not part of the name.
        if !take_header(&mut listed, &mut omitted_count, &mut omitted_name, name) {
            return Ok((Headers::unusable(), Loc::Unusable));
        }
        if name.len() <= HEADER_NAME_CAP && name.eq_ignore_ascii_case("location") {
            loc = merge_loc(loc, json_location(at(item, "value")));
        }
    }
    Ok((Headers::recorded(listed, omitted_count, omitted_name), loc))
}

fn header_block(value: &Json) -> Result<(Headers, Loc), Read> {
    match at(value, "headers") {
        None => Ok((Headers::not_recorded(), Loc::Absent)),
        Some(Arr(items)) => parse_header_array(items),
        Some(_) => Ok((Headers::unusable(), Loc::Unusable)),
    }
}

/// A field line is `token ":" OWS field-value`. The name is not trimmed.
fn split_field(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.split_once(':')?;
    header_name_chars(name).then_some((name, value))
}

fn http_headers(head: &str) -> Result<(Headers, Loc), Read> {
    let mut listed = Vec::new();
    let mut omitted_count = 0;
    let mut omitted_name = 0;
    let mut loc = Loc::Absent;
    for (seen, line) in head.lines().skip(1).enumerate() {
        if seen % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        // The colon follows the token with no whitespace between them.
        let Some((name, value)) = split_field(line) else {
            return Ok((Headers::unusable(), Loc::Unusable));
        };
        if !take_header(&mut listed, &mut omitted_count, &mut omitted_name, name) {
            return Ok((Headers::unusable(), Loc::Unusable));
        }
        if name.len() <= HEADER_NAME_CAP && name.eq_ignore_ascii_case("location") {
            loc = merge_loc(loc, location_text(value));
        }
    }
    Ok((Headers::recorded(listed, omitted_count, omitted_name), loc))
}

fn present_body(bytes: &[u8], measure: BodyMeasure) -> Result<BodyFact, Read> {
    let len = match u64::try_from(bytes.len()) {
        Ok(len) => len,
        Err(_) => return Err(Read::Refused(READER_LIMIT.into())),
    };
    if bytes.len() > MEMBER_BYTE_CAP {
        return Ok(BodyFact::OverBound { len, measure });
    }
    match sha256(bytes) {
        Some(sha256) => Ok(BodyFact::Present { len, sha256, measure }),
        None if operation_stopped() => Err(stopped_read()),
        None => Ok(BodyFact::OverBound { len, measure }),
    }
}

fn body_text(text: &str, encoding: Option<&str>) -> Result<BodyFact, Read> {
    let len = match u64::try_from(text.len()) {
        Ok(len) => len,
        Err(_) => return Err(Read::Refused(READER_LIMIT.into())),
    };
    if text.len() > MEMBER_BYTE_CAP {
        return Ok(BodyFact::OverBound { len, measure: BodyMeasure::Text });
    }
    if encoding.is_some_and(|value| !value.is_empty()) {
        return Ok(BodyFact::EncodingUnread { len });
    }
    present_body(text.as_bytes(), BodyMeasure::Text)
}

fn raw_body(bytes: &[u8]) -> Result<BodyFact, Read> {
    present_body(bytes, BodyMeasure::Body)
}

fn text_body(owner: &Json) -> Result<BodyFact, Read> {
    let text = match at(owner, "text") {
        None => return Ok(BodyFact::NotRecorded),
        Some(Str(text)) => text.as_str(),
        Some(_) => return Ok(BodyFact::Unusable),
    };
    let encoding = match at(owner, "encoding") {
        None => None,
        Some(Str(text)) => Some(text.as_str()),
        Some(_) => return Ok(BodyFact::Unusable),
    };
    body_text(text, encoding)
}

fn request_body(request: &Json) -> Result<BodyFact, Read> {
    match at(request, "postData") {
        None => Ok(BodyFact::NotRecorded),
        Some(post @ Obj(_)) => posted_body(post),
        Some(_) => Ok(BodyFact::Unusable),
    }
}

/// `text` and `params` are alternatives. One records the body; both together are unusable.
fn posted_body(post: &Json) -> Result<BodyFact, Read> {
    match (at(post, "text"), at(post, "params")) {
        (Some(_), Some(_)) => Ok(BodyFact::Unusable),
        (Some(_), None) => text_body(post),
        (None, Some(Arr(items))) => params_body(items),
        (None, Some(_)) => Ok(BodyFact::Unusable),
        (None, None) => Ok(BodyFact::NotRecorded),
    }
}

fn params_body(items: &[Json]) -> Result<BodyFact, Read> {
    let mut count = 0u64;
    for (seen, item) in items.iter().enumerate() {
        if seen % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        let Obj(_) = item else { return Ok(BodyFact::Unusable) };
        match at(item, "name") {
            Some(Str(name)) if clean(name) => {}
            _ => return Ok(BodyFact::Unusable),
        }
        if let Some(value) = at(item, "value") {
            let Str(text) = value else { return Ok(BodyFact::Unusable) };
            if !clean(text) {
                return Ok(BodyFact::Unusable);
            }
        }
        for key in ["fileName", "contentType"] {
            if let Some(value) = at(item, key) {
                let Str(text) = value else { return Ok(BodyFact::Unusable) };
                if !clean(text) {
                    return Ok(BodyFact::Unusable);
                }
            }
        }
        count = match count.checked_add(1) {
            Some(next) => next,
            None => return Err(Read::Refused(READER_LIMIT.into())),
        };
    }
    Ok(BodyFact::Params { count })
}

/// `time` is a non-negative JSON number. The capture spelling is kept, including an exponent.
fn elapsed_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] == b'-' || bytes[0] == b'+' {
        return false;
    }
    let mut index = 0usize;
    if bytes[index] == b'0' {
        index += 1;
    } else if bytes[index].is_ascii_digit() {
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    } else {
        return false;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    index == bytes.len()
}

fn timing_of(entry: &Json) -> TimingFact {
    match at(entry, "time") {
        Some(Num(text)) if elapsed_number(text) && text.len() > TIMING_TEXT_CAP => TimingFact::TooLong,
        Some(Num(text)) if elapsed_number(text) => TimingFact::Recorded(text.clone()),
        Some(_) => TimingFact::Unusable,
        None => match at(entry, "timings") {
            Some(Obj(_)) => TimingFact::Breakdown,
            Some(_) => TimingFact::Unusable,
            None => TimingFact::NotRecorded,
        },
    }
}

struct Required<'a> {
    method: &'a str,
    url: &'a str,
    status: u16,
    mime: &'a str,
    bytes: u64,
}

fn required<'a>(value: &'a Json, index: usize) -> Result<Required<'a>, Read> {
    let Some(request) = at(value, "request") else {
        return Err(refuse_field(index, "request.method"));
    };
    let method = match at(request, "method") {
        Some(Str(text)) if !text.is_empty() && clean(text) => text.as_str(),
        _ => return Err(refuse_field(index, "request.method")),
    };
    let url = match at(request, "url") {
        Some(Str(text)) if !text.is_empty() && clean(text) => text.as_str(),
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
        Some(Str(text)) if clean(text) => text.as_str(),
        _ => return Err(refuse_field(index, "response.content.mimeType")),
    };
    let bytes = match at(content, "size").and_then(whole) {
        Some(size) => size,
        None => return Err(refuse_field(index, "response.content.size")),
    };
    Ok(Required { method, url, status, mime, bytes })
}

fn entry(value: &Json, index: usize) -> Result<Row, Read> {
    let req = required(value, index)?;
    let Some(request) = at(value, "request") else {
        return Err(refuse_field(index, "request.method"));
    };
    let Some(response) = at(value, "response") else {
        return Err(refuse_field(index, "response.status"));
    };
    let Some(content) = at(response, "content") else {
        return Err(refuse_field(index, "response.content.mimeType"));
    };
    let (request_headers, _) = header_block(request)?;
    let (response_headers, header_loc) = header_block(response)?;
    let field_loc = json_location(at(response, "redirectURL"));
    let mut loc = merge_loc(header_loc, field_loc);
    if !is_redirect(req.status) {
        loc = Loc::Absent;
    }
    Ok(Row {
        index,
        method: req.method.to_string(),
        url: req.url.to_string(),
        status: req.status,
        mime: req.mime.to_string(),
        bytes: req.bytes,
        request_headers,
        response_headers,
        request_body: request_body(request)?,
        response_body: text_body(content)?,
        timing: timing_of(value),
        loc,
        request_member: None,
        response_member: None,
    })
}

fn interpret(value: &Json) -> Read {
    if operation_stopped() {
        return stopped_read();
    }
    let Some(log) = at(value, "log") else { return Read::NotCapture };
    let Some(entries) = at(log, "entries") else { return Read::NotCapture };
    let Arr(entries) = entries else { return Read::Refused(UNUSABLE.into()) };
    let mut pending = Vec::new();
    let mut url_cut = false;
    let mut entry_cut = false;
    for (index, value) in entries.iter().enumerate() {
        if index % 16 == 0 && operation_stopped() {
            return stopped_read();
        }
        let req = match required(value, index) {
            Ok(req) => req,
            Err(refusal) => return refusal,
        };
        if req.url.len() > URL_CAP {
            url_cut = true;
            continue;
        }
        if pending.len() >= ENTRY_CAP {
            entry_cut = true;
            continue;
        }
        match entry(value, index) {
            Ok(_) if operation_stopped() => return stopped_read(),
            Ok(row) => pending.push(row),
            Err(refusal) => return refusal,
        }
    }
    finish(pending, false, false, Vec::new(), url_cut, entry_cut)
}

fn cap_line(name: &str) -> String {
    format!("Refusal: {name}.")
}

fn report_reserve() -> usize {
    cap_line(REPORT_CAP_NAME).len().saturating_add(1).saturating_add(CONTRACT.len()).saturating_add(1)
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

struct Reference<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

fn is_scheme(text: &str) -> bool {
    let mut bytes = text.bytes();
    match bytes.next() {
        Some(byte) if byte.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// Split a URI reference. A scheme is present only when it matches the published
/// scheme grammar and its colon comes before `/`, `?`, and `#`.
fn split_reference(input: &str) -> Reference<'_> {
    let mut scheme = None;
    let mut rest = input;
    if let Some(colon) = input.find(':') {
        let before = &input[..colon];
        if before.find(['/', '?', '#']).is_none() && is_scheme(before) {
            scheme = Some(before);
            rest = &input[colon + 1..];
        }
    }
    let (authority, after_authority) = if let Some(stripped) = rest.strip_prefix("//") {
        let end = stripped.find(['/', '?', '#']).unwrap_or(stripped.len());
        (Some(&stripped[..end]), &stripped[end..])
    } else {
        (None, rest)
    };
    let (path_and_query, fragment) = match after_authority.split_once('#') {
        Some((path_and_query, fragment)) => (path_and_query, Some(fragment)),
        None => (after_authority, None),
    };
    let (path, query) = match path_and_query.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (path_and_query, None),
    };
    Reference { scheme, authority, path, query, fragment }
}

fn pop_last_segment(output: &mut String) {
    if let Some(at) = output.rfind('/') {
        output.truncate(at);
    } else {
        output.clear();
    }
}

/// Remove `.` and `..` segments from a path after a relative reference is merged.
fn remove_dot_segments(path: &str) -> String {
    let mut input = path;
    let mut output = String::with_capacity(path.len());
    while !input.is_empty() {
        if let Some(rest) = input.strip_prefix("../") {
            input = rest;
        } else if let Some(rest) = input.strip_prefix("./") {
            input = rest;
        } else if input.starts_with("/./") {
            input = &input[2..];
        } else if input == "/." {
            input = "/";
        } else if input.starts_with("/../") {
            input = &input[3..];
            pop_last_segment(&mut output);
        } else if input == "/.." {
            input = "/";
            pop_last_segment(&mut output);
        } else if input == "." || input == ".." {
            input = "";
        } else {
            let slash = usize::from(input.starts_with('/'));
            let body = &input[slash..];
            let cut = body.find('/').map(|at| at + slash).unwrap_or(input.len());
            output.push_str(&input[..cut]);
            input = &input[cut..];
        }
    }
    output
}

fn merge_paths(base_has_authority: bool, base_path: &str, ref_path: &str) -> String {
    if base_has_authority && base_path.is_empty() {
        let mut merged = String::with_capacity(ref_path.len() + 1);
        merged.push('/');
        merged.push_str(ref_path);
        return merged;
    }
    let prefix = match base_path.rfind('/') {
        Some(index) => &base_path[..=index],
        None => "",
    };
    let mut merged = String::with_capacity(prefix.len() + ref_path.len());
    merged.push_str(prefix);
    merged.push_str(ref_path);
    merged
}

fn recompose(scheme: &str, authority: Option<&str>, path: &str, query: Option<&str>, fragment: Option<&str>) -> String {
    let mut out = String::with_capacity(scheme.len() + authority.map(|item| item.len() + 2).unwrap_or(0) + path.len() + query.map(|item| item.len() + 1).unwrap_or(0) + fragment.map(|item| item.len() + 1).unwrap_or(0));
    out.push_str(scheme);
    out.push(':');
    if let Some(authority) = authority {
        out.push_str("//");
        out.push_str(authority);
    }
    out.push_str(path);
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// Resolve a relative redirect target against the request URL that returned it.
/// An absolute target is returned unchanged so pairing stays a raw comparison.
fn resolve_redirect_target(base: &str, location: &str) -> String {
    let reference = split_reference(location);
    if reference.scheme.is_some() {
        return location.to_string();
    }
    let base_parts = split_reference(base);
    let Some(scheme) = base_parts.scheme else {
        return location.to_string();
    };
    let (authority, path, query) = if reference.authority.is_some() {
        (reference.authority, remove_dot_segments(reference.path), reference.query)
    } else if reference.path.is_empty() {
        let query = if reference.query.is_some() { reference.query } else { base_parts.query };
        (base_parts.authority, base_parts.path.to_string(), query)
    } else if reference.path.starts_with('/') {
        (base_parts.authority, remove_dot_segments(reference.path), reference.query)
    } else {
        let merged = merge_paths(base_parts.authority.is_some(), base_parts.path, reference.path);
        (base_parts.authority, remove_dot_segments(&merged), reference.query)
    };
    recompose(scheme, authority, &path, query, reference.fragment)
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
                let resolved = resolve_redirect_target(&row.url, target);
                if let Some(later) = rows[position + 1..].iter().find(|next| next.url == resolved) {
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
        BodyFact::Present { len, sha256, measure } => {
            let (noun, note) = match measure {
                BodyMeasure::Body => ("present length", "body"),
                BodyMeasure::Text => ("text representation length", "text representation"),
            };
            stated(
                format!("{label} {noun} {len} sha256 {sha256}."),
                obj(vec![
                    ("kind", s("body")),
                    ("index", index_json(index)),
                    ("side", s(side)),
                    ("present", Bool(true)),
                    ("measure", s(note)),
                    ("length", Num(len.to_string())),
                    ("sha256", s(sha256)),
                ]),
            )
        }
        BodyFact::OverBound { len, measure } => {
            let (what, unit) = match measure {
                BodyMeasure::Body => ("body", "size"),
                BodyMeasure::Text => ("text representation", "text length"),
            };
            refusal_line(format!(
                "Refusal: entry {} {side} {what} exceeds the saved-capture member byte cap. {unit} {len}.",
                index.unwrap_or(0)
            ))
        }
        BodyFact::EncodingUnread { len } => {
            refusal_line(format!(
                "Refusal: entry {} {side} text representation is encoded and was not read. text length {len}.",
                index.unwrap_or(0)
            ))
        }
        BodyFact::Params { count } => stated(
            format!("{label}: parameters recorded. count {count}."),
            obj(vec![
                ("kind", s("body")),
                ("index", index_json(index)),
                ("side", s(side)),
                ("note", s("parameters recorded")),
                ("count", Num(count.to_string())),
            ]),
        ),
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

fn pairing_statements(rows: &[Row], saz: bool, index_present: bool, orphans: &[Orphan]) -> Vec<Stated> {
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
    let mut out = vec![if index_present {
        stated(
            "index.dat present.",
            obj(vec![("kind", s("index")), ("present", Bool(true))]),
        )
    } else {
        stated(
            "index.dat: not recorded.",
            obj(vec![("kind", s("index")), ("present", Bool(false)), ("note", s("not recorded"))]),
        )
    }];
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

fn statements(rows: &[Row], saz: bool, index_present: bool, orphans: &[Orphan]) -> Result<Vec<Stated>, Read> {
    if rows.is_empty() && !saz {
        return Ok(Vec::new());
    }
    guard_running()?;
    let mut out = Vec::new();
    out.extend(status_statements(rows));
    guard_running()?;
    out.extend(redirect_statements(rows));
    guard_running()?;
    out.extend(headers_statements(rows));
    guard_running()?;
    out.extend(body_statements(rows));
    guard_running()?;
    out.extend(timing_statements(rows));
    guard_running()?;
    out.extend(pairing_statements(rows, saz, index_present, orphans));
    guard_running()?;
    Ok(out)
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

fn render(rows: Vec<Row>, mut caps: Vec<&'static str>, saz: bool, index_present: bool, orphans: Vec<Orphan>) -> Read {
    if operation_stopped() {
        return stopped_read();
    }
    collect_caps(&rows, &mut caps);
    let mut header = vec![TITLE.to_string(), SCOPE.to_string()];
    for name in &caps {
        header.push(cap_line(name));
    }
    let reserve = report_reserve();
    let mut used = 0usize;
    for line in &header {
        used = used.saturating_add(line.len()).saturating_add(1);
    }
    used = used.saturating_add("Entries 64.\n".len());
    let mut emitted = Vec::new();
    let mut cut = false;
    for row in &rows {
        if operation_stopped() {
            return stopped_read();
        }
        let line = row_line(row);
        if used.saturating_add(line.len()).saturating_add(1).saturating_add(reserve) > REPORT_CAP {
            cut = true;
            break;
        }
        used = used.saturating_add(line.len()).saturating_add(1);
        emitted.push(row.clone());
    }
    let stated = match statements(&emitted, saz, index_present, &orphans) {
        Ok(stated) => stated,
        Err(refusal) => return refusal,
    };
    let mut kept: usize = 0;
    for item in &stated {
        if operation_stopped() {
            return stopped_read();
        }
        if used.saturating_add(item.line.len()).saturating_add(1).saturating_add(reserve) > REPORT_CAP {
            cut = true;
            break;
        }
        used = used.saturating_add(item.line.len()).saturating_add(1);
        kept = kept.saturating_add(1);
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
    if operation_stopped() {
        return stopped_read();
    }
    Read::Rows(Capture { rows: emitted, caps, report, observations })
}

fn finish(rows: Vec<Row>, saz: bool, index_present: bool, orphans: Vec<Orphan>, mut url_cut: bool, mut entry_cut: bool) -> Read {
    let mut caps = Vec::new();
    let mut kept = Vec::new();
    for row in rows {
        if operation_stopped() {
            return stopped_read();
        }
        if row.url.len() > URL_CAP {
            url_cut = true;
            continue;
        }
        if kept.len() >= ENTRY_CAP {
            entry_cut = true;
            continue;
        }
        kept.push(row);
    }
    if url_cut {
        note_cap(&mut caps, URL_CAP_NAME);
    }
    if entry_cut {
        note_cap(&mut caps, ENTRY_CAP_NAME);
    }
    render(kept, caps, saz, index_present, orphans)
}

fn archive_prefix(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x06\x06")
        || bytes.starts_with(b"PK\x06\x07")
}

fn archive_refusal(error: &str) -> &'static str {
    if error.contains("memory budget") || error.contains("addressable memory") {
        return READER_LIMIT;
    }
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

#[cfg(any(test, rllm_module_tests))]
pub(crate) fn registry_reader_limits() {
    let limits = [
        "ZIP64 value exceeds project limit",
        "ZIP member exceeds file limit",
        "Project ZIP must be at most 5 MiB",
        "Project exceeds 4 MiB data limit",
        "ZIP size overflow",
        "Project ZIP exceeds the memory budget",
        "Project exceeds the memory budget",
        "Project archive exceeds the memory budget",
        "Project file exceeds the memory budget",
        "Compressed member exceeds the memory budget",
        "Project import exceeds the memory budget",
        "Project file exceeds addressable memory",
        "Project archive exceeds addressable memory",
        "Select a regular project file within the memory budget",
        "Project snapshot exceeds the memory budget",
        "Pending payload exceeds the memory budget",
        "Pending payload exceeds addressable memory",
    ];
    for sentence in limits {
        assert_eq!(archive_refusal(sentence), READER_LIMIT, "{sentence}");
    }
    assert_eq!(archive_refusal("ZIP member is damaged"), DAMAGED);
    assert_eq!(archive_refusal("Compressed member exceeds block-processing limit"), ARCHIVE_UNUSABLE);
    assert_eq!(archive_refusal("Compressed member exceeds declared size"), ARCHIVE_UNUSABLE);
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path)
}

/// `001_c.txt` / `raw/001_s.txt` / `raw/001_m.xml`. The number is the session prefix.
enum SessionName {
    Other,
    Ready(u32, u8),
    OverLimit,
}

fn session_kind(path: &str) -> SessionName {
    let name = file_name(path);
    let (stem, metadata) = if let Some(stem) = name.strip_suffix(".txt") {
        (stem, false)
    } else if let Some(stem) = name.strip_suffix(".xml") {
        (stem, true)
    } else {
        return SessionName::Other;
    };
    let Some((number, kind)) = stem.rsplit_once('_') else { return SessionName::Other };
    let kind = match (metadata, kind) {
        (false, "c") => b'c',
        (false, "s") => b's',
        (true, "m") => b'm',
        _ => return SessionName::Other,
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return SessionName::Other;
    }
    match number.parse::<u32>() {
        Ok(id) => SessionName::Ready(id, kind),
        Err(_) => SessionName::OverLimit,
    }
}

enum TimerMark {
    Instant(i64),
    Duration,
}

struct TimerSource<'a> {
    head: &'a str,
    inner: &'a str,
}

fn is_xml_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.')
}

fn local_xml_name(name: &str) -> &str {
    name.rsplit_once(':').map(|(_, local)| local).unwrap_or(name)
}

fn opens_timer_element(text: &str, start: usize) -> bool {
    let bytes = text.as_bytes();
    if start == 0 {
        return false;
    }
    let mut index = start - 1;
    if bytes[index] == b':' {
        loop {
            if index == 0 {
                return false;
            }
            index -= 1;
            if !is_xml_name_byte(bytes[index]) || bytes[index] == b':' {
                return bytes[index] == b'<';
            }
        }
    }
    bytes[index] == b'<'
}

fn closes_timer_element(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if start == 0 {
        return None;
    }
    let mut index = start - 1;
    if bytes[index] == b':' {
        loop {
            if index == 0 {
                return None;
            }
            index -= 1;
            if !is_xml_name_byte(bytes[index]) || bytes[index] == b':' {
                break;
            }
        }
    }
    if bytes[index] != b'/' || index == 0 || bytes[index - 1] != b'<' {
        return None;
    }
    Some(index - 1)
}

fn session_timer_source(text: &str) -> Result<Option<TimerSource<'_>>, Read> {
    let needle = "SessionTimers";
    let bytes = text.as_bytes();
    let mut from = 0usize;
    let mut steps = 0usize;
    while from < text.len() {
        if steps % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        let Some(rel) = text[from..].find(needle) else { break };
        let start = from + rel;
        let after = start + needle.len();
        let next_ok = after >= bytes.len() || matches!(bytes[after], b'>' | b'/' | b' ' | b'\t' | b'\r' | b'\n');
        if opens_timer_element(text, start) && next_ok {
            return timer_region(&text[after..]);
        }
        from = after;
    }
    Ok(None)
}

fn timer_region(after: &str) -> Result<Option<TimerSource<'_>>, Read> {
    let bytes = after.as_bytes();
    let mut index = 0usize;
    let mut quote = 0u8;
    let mut steps = 0usize;
    while index < bytes.len() {
        if steps % 128 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        let byte = bytes[index];
        if quote != 0 {
            if byte == quote {
                quote = 0;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = byte;
            index += 1;
            continue;
        }
        if byte == b'>' {
            let mut head = &after[..index];
            let self_close = head.trim_end().ends_with('/');
            head = head.trim().trim_end_matches('/').trim();
            if self_close {
                return Ok(Some(TimerSource { head, inner: "" }));
            }
            let inner = timer_inner(&after[index + 1..])?;
            return Ok(Some(TimerSource { head, inner }));
        }
        index += 1;
    }
    Ok(None)
}

fn timer_inner(after: &str) -> Result<&str, Read> {
    let needle = "SessionTimers";
    let bytes = after.as_bytes();
    let mut from = 0usize;
    let mut steps = 0usize;
    while from < after.len() {
        if steps % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        let Some(rel) = after[from..].find(needle) else { break };
        let start = from + rel;
        let name_end = start + needle.len();
        let next_ok = name_end >= bytes.len() || matches!(bytes[name_end], b'>' | b' ' | b'\t' | b'\r' | b'\n');
        if next_ok {
            if let Some(close_at) = closes_timer_element(after, start) {
                return Ok(&after[..close_at]);
            }
        }
        from = name_end;
    }
    Ok("")
}

fn numeric_entity(entity: &str) -> Option<u32> {
    let digits = entity.strip_prefix('#')?;
    if let Some(hex) = digits.strip_prefix('x').or_else(|| digits.strip_prefix('X')) {
        if hex.is_empty() || hex.len() > 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        return u32::from_str_radix(hex, 16).ok();
    }
    if digits.is_empty() || digits.len() > 7 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => match numeric_entity(entity).and_then(char::from_u32) {
                Some(ch) => out.push(ch),
                None => out.push_str(&rest[..=end]),
            },
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn xml_attributes(head: &str) -> Result<Vec<(String, String)>, Read> {
    let bytes = head.as_bytes();
    let mut index = 0usize;
    let mut out = Vec::new();
    let mut steps = 0usize;
    while index < bytes.len() {
        if steps % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'/' {
            break;
        }
        let name_at = index;
        while index < bytes.len() && is_xml_name_byte(bytes[index]) {
            index += 1;
        }
        if index == name_at {
            index += 1;
            continue;
        }
        let name = local_xml_name(&head[name_at..index]).to_string();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let quote = bytes[index];
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        index += 1;
        let value_at = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        let raw = &head[value_at..index];
        if index < bytes.len() {
            index += 1;
        }
        out.push((name, decode_entities(raw)));
    }
    Ok(out)
}

fn child_timer_values(inner: &str) -> Result<Vec<(String, String)>, Read> {
    let bytes = inner.as_bytes();
    let mut index = 0usize;
    let mut out = Vec::new();
    let mut steps = 0usize;
    while index < bytes.len() {
        if steps % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        if matches!(bytes.get(index + 1).copied(), Some(b'/') | Some(b'!') | Some(b'?')) {
            index += 1;
            continue;
        }
        let name_at = index + 1;
        let mut cursor = name_at;
        while cursor < bytes.len() && is_xml_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_at {
            index += 1;
            continue;
        }
        let name = local_xml_name(&inner[name_at..cursor]).to_string();
        let mut quote = 0u8;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if quote != 0 {
                if byte == quote {
                    quote = 0;
                }
                cursor += 1;
                continue;
            }
            if byte == b'"' || byte == b'\'' {
                quote = byte;
                cursor += 1;
                continue;
            }
            if byte == b'>' {
                break;
            }
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'>' {
            break;
        }
        let self_close = cursor > index && bytes[cursor - 1] == b'/';
        cursor += 1;
        if self_close {
            index = cursor;
            continue;
        }
        let value_at = cursor;
        while cursor < bytes.len() && bytes[cursor] != b'<' {
            cursor += 1;
        }
        let raw = inner[value_at..cursor].trim();
        if !raw.is_empty() {
            out.push((name, decode_entities(raw)));
        }
        index = cursor;
    }
    Ok(out)
}

fn take_digits(text: &str, width: usize) -> Option<(&str, u32)> {
    let bytes = text.as_bytes();
    if bytes.len() < width || !bytes[..width].iter().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = text[..width].parse().ok()?;
    Some((&text[width..], value))
}

fn civil_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn valid_ymd(year: i32, month: u32, day: u32) -> bool {
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if civil_leap(year) {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    (1..=9999).contains(&year) && day >= 1 && day <= max
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !valid_ymd(year, month, day) {
        return None;
    }
    let shifted = if month <= 2 { year - 1 } else { year };
    let shifted = i64::from(shifted);
    let era = if shifted >= 0 { shifted } else { shifted - 399 } / 400;
    let year_of_era = (shifted - era * 400) as u64;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index as u64 + 2) / 5 + day as u64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146097 + day_of_era as i64 - 719468)
}

fn fraction_millis(rest: &str) -> Option<(&str, i64)> {
    let Some(rest) = rest.strip_prefix('.') else {
        return Some((rest, 0));
    };
    let digits = rest.bytes().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let mut millis = 0i64;
    let take = digits.min(3);
    for offset in 0..take {
        millis = millis * 10 + i64::from(rest.as_bytes()[offset] - b'0');
    }
    for _ in take..3 {
        millis *= 10;
    }
    Some((&rest[digits..], millis))
}

fn zone_offset_millis(rest: &str) -> Option<i64> {
    if rest.is_empty() {
        return Some(0);
    }
    if rest == "Z" || rest == "z" {
        return Some(0);
    }
    let (sign, rest) = match rest.as_bytes().first() {
        Some(b'+') => (1i64, &rest[1..]),
        Some(b'-') => (-1i64, &rest[1..]),
        _ => return None,
    };
    let (rest, hour) = take_digits(rest, 2)?;
    if hour > 23 {
        return None;
    }
    let (rest, minute) = if let Some(rest) = rest.strip_prefix(':') {
        take_digits(rest, 2)?
    } else if rest.len() >= 2 && rest.as_bytes()[0].is_ascii_digit() {
        take_digits(rest, 2)?
    } else if rest.is_empty() {
        (rest, 0)
    } else {
        return None;
    };
    if minute > 59 || !rest.is_empty() {
        return None;
    }
    Some(sign * (i64::from(hour) * 3_600_000 + i64::from(minute) * 60_000))
}

/// A recorded timer instant: `YYYY-MM-DDTHH:MM:SS` with an optional fraction and zone.
fn instant_millis(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.len() < 19 || value.len() > 40 {
        return None;
    }
    let (rest, year) = take_digits(value, 4)?;
    let rest = rest.strip_prefix('-')?;
    let (rest, month) = take_digits(rest, 2)?;
    let rest = rest.strip_prefix('-')?;
    let (rest, day) = take_digits(rest, 2)?;
    let year = year as i32;
    if !valid_ymd(year, month, day) {
        return None;
    }
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' '))?;
    let (rest, hour) = take_digits(rest, 2)?;
    let rest = rest.strip_prefix(':')?;
    let (rest, minute) = take_digits(rest, 2)?;
    let rest = rest.strip_prefix(':')?;
    let (rest, second) = take_digits(rest, 2)?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let (rest, fraction) = fraction_millis(rest)?;
    let offset = zone_offset_millis(rest)?;
    let days = days_from_civil(year, month, day)?;
    let local = days
        .checked_mul(86_400_000)?
        .checked_add(i64::from(hour) * 3_600_000)?
        .checked_add(i64::from(minute) * 60_000)?
        .checked_add(i64::from(second) * 1000)?
        .checked_add(fraction)?;
    local.checked_sub(offset)
}

fn timer_mark(value: &str) -> Option<TimerMark> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(millis) = instant_millis(value) {
        return Some(TimerMark::Instant(millis));
    }
    if elapsed_number(value) {
        return Some(TimerMark::Duration);
    }
    None
}

fn named_instant(marks: &[(String, TimerMark)], name: &str) -> Option<i64> {
    marks.iter().find_map(|(found, mark)| match mark {
        TimerMark::Instant(millis) if found == name => Some(*millis),
        _ => None,
    })
}

fn preferred_span(marks: &[(String, TimerMark)]) -> Option<i64> {
    const PAIRS: &[(&str, &str)] = &[
        ("ClientDoneResponse", "ClientBeginRequest"),
        ("ClientDoneResponse", "ClientConnected"),
        ("ServerDoneResponse", "ClientBeginRequest"),
        ("ServerDoneResponse", "ClientConnected"),
        ("ClientDoneRequest", "ClientBeginRequest"),
        ("ServerBeginResponse", "ClientBeginRequest"),
    ];
    for (end, start) in PAIRS {
        if let (Some(end), Some(start)) = (named_instant(marks, end), named_instant(marks, start)) {
            return end.checked_sub(start);
        }
    }
    let mut count = 0usize;
    let mut earliest = i64::MAX;
    let mut latest = i64::MIN;
    for (_, mark) in marks {
        let TimerMark::Instant(millis) = mark else { continue };
        count += 1;
        earliest = earliest.min(*millis);
        latest = latest.max(*millis);
    }
    if count >= 2 { latest.checked_sub(earliest) } else { None }
}

/// `BitFlags` is the session flag word on the `Session` element. The low bit is
/// the recorded HTTPS scheme. The next bit is the recorded FTP scheme. Any other
/// value, or a missing attribute, leaves the scheme unnamed. Text that only
/// contains the name is not that attribute.
fn recorded_scheme(bytes: &[u8]) -> Result<&'static str, Read> {
    if operation_stopped() {
        return Err(stopped_read());
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok("http");
    };
    let Some(head) = session_open_attributes(text)? else {
        return Ok("http");
    };
    let Some(raw) = attribute_value(head, "BitFlags")? else {
        return Ok("http");
    };
    let Some(bits) = flag_bits(raw) else {
        return Ok("http");
    };
    if bits & 1 == 1 {
        Ok("https")
    } else if bits & 2 == 2 {
        Ok("ftp")
    } else {
        Ok("http")
    }
}

fn flag_bits(raw: &str) -> Option<u64> {
    let text = raw.trim();
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Attribute span of the first `Session` opening tag, after the element name and
/// before that tag's `>`. Child elements and text outside the tag are excluded.
/// An unclosed tag keeps its trailing text so a present attribute is still read.
fn session_open_attributes(text: &str) -> Result<Option<&str>, Read> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut steps = 0usize;
    while index < bytes.len() {
        if steps % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'/' | b'!' | b'?') {
            let end = markup_end(bytes, index)?;
            index = if end < bytes.len() { end + 1 } else { end };
            continue;
        }
        let name_at = index;
        while index < bytes.len() && is_xml_name_byte(bytes[index]) {
            index += 1;
        }
        if index == name_at {
            continue;
        }
        let is_session = local_xml_name(&text[name_at..index]) == "Session";
        let end = markup_end(bytes, index)?;
        if is_session {
            return Ok(Some(&text[index..end]));
        }
        index = if end < bytes.len() { end + 1 } else { end };
    }
    Ok(None)
}

fn markup_end(bytes: &[u8], mut index: usize) -> Result<usize, Read> {
    let mut quote = 0u8;
    let mut steps = 0usize;
    while index < bytes.len() {
        if steps % 128 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        let byte = bytes[index];
        if quote != 0 {
            if byte == quote {
                quote = 0;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = byte;
            index += 1;
            continue;
        }
        if byte == b'>' {
            return Ok(index);
        }
        index += 1;
    }
    Ok(bytes.len())
}

fn attribute_value<'a>(text: &'a str, name: &str) -> Result<Option<&'a str>, Read> {
    let bytes = text.as_bytes();
    let needle = name.as_bytes();
    if needle.is_empty() {
        return Ok(None);
    }
    let mut index = 0usize;
    let mut quote = 0u8;
    let mut steps = 0usize;
    while index < bytes.len() {
        if steps % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        let byte = bytes[index];
        if quote != 0 {
            if byte == quote {
                quote = 0;
            }
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(needle) {
            let after = index + needle.len();
            let left_ok = index == 0 || !is_xml_name_byte(bytes[index - 1]);
            let right_ok = after >= bytes.len() || !is_xml_name_byte(bytes[after]);
            if left_ok && right_ok {
                let mut cursor = after;
                while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                if cursor < bytes.len() && bytes[cursor] == b'=' {
                    cursor += 1;
                    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                        cursor += 1;
                    }
                    if cursor < bytes.len() && (bytes[cursor] == b'"' || bytes[cursor] == b'\'') {
                        let mark = bytes[cursor];
                        let value_at = cursor + 1;
                        let mut end = value_at;
                        while end < bytes.len() && bytes[end] != mark {
                            end += 1;
                        }
                        return Ok(Some(&text[value_at..end]));
                    }
                }
            }
        }
        if byte == b'"' || byte == b'\'' {
            quote = byte;
        }
        index += 1;
    }
    Ok(None)
}

fn recorded_elapsed(millis: i64) -> TimingFact {
    if millis < 0 {
        return TimingFact::Breakdown;
    }
    let text = millis.to_string();
    if text.len() > TIMING_TEXT_CAP {
        return TimingFact::TooLong;
    }
    if elapsed_number(&text) { TimingFact::Recorded(text) } else { TimingFact::Breakdown }
}

/// `SessionTimers` attributes are capture-recorded marks. The elapsed fact is the
/// span in whole milliseconds. Marks that are present but have no span stay a
/// breakdown. Absent marks stay not recorded.
fn session_timing(bytes: &[u8]) -> Result<TimingFact, Read> {
    if operation_stopped() {
        return Err(stopped_read());
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(TimingFact::NotRecorded);
    };
    let Some(source) = session_timer_source(text)? else {
        return Ok(TimingFact::NotRecorded);
    };
    let mut marks = Vec::new();
    let mut saw_value = false;
    for (name, value) in xml_attributes(source.head)? {
        if !value.trim().is_empty() {
            saw_value = true;
        }
        if let Some(mark) = timer_mark(&value) {
            marks.push((name, mark));
        }
    }
    for (name, value) in child_timer_values(source.inner)? {
        if !value.trim().is_empty() {
            saw_value = true;
        }
        if let Some(mark) = timer_mark(&value) {
            marks.push((name, mark));
        }
    }
    if marks.is_empty() {
        return Ok(if saw_value { TimingFact::Breakdown } else { TimingFact::NotRecorded });
    }
    match preferred_span(&marks) {
        Some(millis) => Ok(recorded_elapsed(millis)),
        None => Ok(TimingFact::Breakdown),
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Result<Option<usize>, Read> {
    let n = needle.len();
    if n == 0 || n > hay.len() {
        return Ok(None);
    }
    let mut index = 0usize;
    loop {
        let end = match index.checked_add(n) {
            Some(end) if end <= hay.len() => end,
            _ => return Ok(None),
        };
        if index % 4096 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        if &hay[index..end] == needle {
            return Ok(Some(index));
        }
        if end == hay.len() {
            return Ok(None);
        }
        index = match index.checked_add(1) {
            Some(next) => next,
            None => return Err(Read::Refused(READER_LIMIT.into())),
        };
    }
}

fn split_message(bytes: &[u8]) -> Result<Option<(&[u8], &[u8])>, Read> {
    if let Some(at) = find_bytes(bytes, b"\r\n\r\n")? {
        let body = at.checked_add(4).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
        if body > bytes.len() {
            return Err(Read::Refused(READER_LIMIT.into()));
        }
        return Ok(Some((&bytes[..at], &bytes[body..])));
    }
    if let Some(at) = find_bytes(bytes, b"\n\n")? {
        let body = at.checked_add(2).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
        if body > bytes.len() {
            return Err(Read::Refused(READER_LIMIT.into()));
        }
        return Ok(Some((&bytes[..at], &bytes[body..])));
    }
    Ok(None)
}

fn content_type(head: &str) -> Result<Option<String>, Read> {
    for (seen, line) in head.lines().skip(1).enumerate() {
        if seen % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = split_field(line) else { continue };
        if name.eq_ignore_ascii_case("content-type") {
            return Ok(Some(value.trim().to_string()));
        }
    }
    Ok(None)
}

fn first_line(head: &str) -> &str {
    head.lines().next().unwrap_or("").trim_end_matches('\r')
}

/// One SP. A tab or a second space is not the separator.
fn split_sp(line: &str) -> Option<(&str, &str)> {
    let (left, right) = line.split_once(' ')?;
    (!left.is_empty()).then_some((left, right))
}

/// `method SP request-target SP HTTP-version` and nothing after the version.
fn request_line(line: &str) -> Option<(&str, &str)> {
    let (method, rest) = split_sp(line)?;
    let (target, version) = split_sp(rest)?;
    if version.is_empty() || version.contains(' ') || !http_version(version) || !header_name_chars(method) || target.is_empty() || !clean(target) {
        return None;
    }
    Some((method, target))
}

/// `HTTP-version SP status-code SP [reason-phrase]`. The space after the code is required.
fn status_code(line: &str) -> Option<&str> {
    let (version, rest) = split_sp(line)?;
    let (status, _reason) = split_sp(rest)?;
    if !http_version(version) || status.len() != 3 || !status.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(status)
}

fn scheme_absolute(target: &str) -> bool {
    let Some((scheme, rest)) = target.split_once(':') else { return false };
    let bytes = scheme.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_alphabetic()
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        && rest.starts_with("//")
}

fn trim_ows(value: &str) -> &str {
    value.trim_matches([' ', '\t'])
}

fn authority_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= URL_CAP && clean(value) && !value.contains('/') && !value.contains('\\') && !value.contains(' ')
}

fn host_authority(index: usize, head: &str) -> Result<String, Read> {
    let mut found = None;
    for (seen, line) in head.lines().skip(1).enumerate() {
        if seen % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = split_field(line) else { continue };
        if !name.eq_ignore_ascii_case("host") {
            continue;
        }
        let value = trim_ows(value);
        if !authority_text(value) {
            return Err(refuse_field(index, "request.url"));
        }
        if found.is_some() {
            return Err(refuse_field(index, "request.url"));
        }
        found = Some(value.to_string());
    }
    found.ok_or_else(|| refuse_field(index, "request.url"))
}

/// HAR stores an absolute URL. An origin-form target is resolved with the Host authority.
/// The request line names no scheme. `scheme` is the one recorded for the session:
/// `https` when the metadata HTTPS bit is set, `ftp` when the metadata names FTP,
/// and `http` when the metadata names neither. A CONNECT tunnel has no path, so its
/// authority is joined to that same recorded scheme.
fn absolute_request_url(index: usize, method: &str, target: &str, head: &str, scheme: &str) -> Result<String, Read> {
    if !clean(target) {
        return Err(refuse_field(index, "request.url"));
    }
    if scheme_absolute(target) {
        return Ok(target.to_string());
    }
    if target.starts_with('/') && !target.starts_with("//") {
        let host = host_authority(index, head)?;
        return Ok(join_scheme(scheme, &host, target));
    }
    if method.eq_ignore_ascii_case("CONNECT") && authority_text(target) {
        return Ok(join_scheme(scheme, target, ""));
    }
    Err(refuse_field(index, "request.url"))
}

fn join_scheme(scheme: &str, authority: &str, path: &str) -> String {
    let mut url = String::with_capacity(scheme.len() + 3 + authority.len() + path.len());
    url.push_str(scheme);
    url.push_str("://");
    url.push_str(authority);
    url.push_str(path);
    url
}

fn response_facts(index: usize, bytes: &[u8]) -> Result<(u16, String, u64, BodyFact, Headers, Loc), Read> {
    let Some((head, body)) = split_message(bytes)? else {
        return Err(refuse_field(index, "response.content.size"));
    };
    let head = std::str::from_utf8(head).map_err(|_| refuse_field(index, "response.status"))?;
    let Some(status_text) = status_code(first_line(head)) else {
        return Err(refuse_field(index, "response.status"));
    };
    let status: u16 = status_text.parse().map_err(|_| refuse_field(index, "response.status"))?;
    let mime = content_type(head)?.ok_or_else(|| refuse_field(index, "response.content.mimeType"))?;
    if !clean(&mime) {
        return Err(refuse_field(index, "response.content.mimeType"));
    }
    let (headers, loc) = http_headers(head)?;
    let body_len = u64::try_from(body.len()).map_err(|_| Read::Refused(READER_LIMIT.into()))?;
    Ok((status, mime, body_len, raw_body(body)?, headers, loc))
}

fn request_parts(index: usize, bytes: &[u8], scheme: &str) -> Result<(String, String, Headers, BodyFact), Read> {
    let Some((head, body)) = split_message(bytes)? else {
        return Err(refuse_field(index, "request.method"));
    };
    let head = std::str::from_utf8(head).map_err(|_| refuse_field(index, "request.method"))?;
    let Some((method, target)) = request_line(first_line(head)) else {
        return Err(refuse_field(index, "request.method"));
    };
    let url = absolute_request_url(index, method, target, head, scheme)?;
    let (headers, _) = http_headers(head)?;
    Ok((method.to_string(), url, headers, raw_body(body)?))
}

fn session_row(index: usize, request_name: &str, request: &[u8], response_name: &str, response: &[u8], timing: TimingFact, scheme: &str) -> Result<Row, Read> {
    let (method, url, request_headers, request_body) = request_parts(index, request, scheme)?;
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
        timing,
        loc,
        request_member: Some(request_name.to_string()),
        response_member: Some(response_name.to_string()),
    })
}

struct Side {
    name: String,
    data: Vec<u8>,
}

fn session_index(id: u32) -> Result<usize, Read> {
    usize::try_from(id).map_err(|_| Read::Refused(SESSION_ID_LIMIT.into()))
}

fn interpret_saz(members: Vec<(String, Vec<u8>)>) -> Read {
    if operation_stopped() {
        return stopped_read();
    }
    let mut has_index = false;
    let mut saw_session = false;
    let mut requests = std::collections::BTreeMap::<u32, Side>::new();
    let mut responses = std::collections::BTreeMap::<u32, Side>::new();
    let mut metadata = std::collections::BTreeMap::<u32, Side>::new();
    for (name, data) in members {
        if operation_stopped() {
            return stopped_read();
        }
        if file_name(&name) == "index.dat" {
            has_index = true;
            continue;
        }
        match session_kind(&name) {
            SessionName::Other => continue,
            SessionName::OverLimit => return Read::Refused(SESSION_ID_LIMIT.into()),
            SessionName::Ready(id, kind) => {
                let slot = match kind {
                    b'c' => {
                        saw_session = true;
                        &mut requests
                    }
                    b's' => {
                        saw_session = true;
                        &mut responses
                    }
                    _ => &mut metadata,
                };
                if slot.insert(id, Side { name: file_name(&name).to_string(), data }).is_some() {
                    return Read::Refused(format!("Refusal: saved capture session {id} is duplicated."));
                }
            }
        }
    }
    if !has_index && !saw_session {
        return Read::Refused(NOT_CAPTURE_ZIP.into());
    }
    if requests.is_empty() {
        return Read::Refused(NO_REQUESTS.into());
    }
    let mut rows = Vec::new();
    let mut orphans = Vec::new();
    for (id, request) in &requests {
        if operation_stopped() {
            return stopped_read();
        }
        let index = match session_index(*id) {
            Ok(index) => index,
            Err(refusal) => return refusal,
        };
        let Some(response) = responses.get(id) else {
            orphans.push(Orphan { name: request.name.clone(), side: "request" });
            continue;
        };
        let (timing, scheme) = match metadata.get(id) {
            Some(meta) => {
                let timing = match session_timing(&meta.data) {
                    Ok(timing) => timing,
                    Err(refusal) => return refusal,
                };
                let scheme = match recorded_scheme(&meta.data) {
                    Ok(scheme) => scheme,
                    Err(refusal) => return refusal,
                };
                (timing, scheme)
            }
            None => (TimingFact::NotRecorded, "http"),
        };
        match session_row(index, &request.name, &request.data, &response.name, &response.data, timing, scheme) {
            Ok(row) => rows.push(row),
            Err(refusal) => return refusal,
        }
    }
    for (id, response) in &responses {
        if operation_stopped() {
            return stopped_read();
        }
        if !requests.contains_key(id) {
            orphans.push(Orphan { name: response.name.clone(), side: "response" });
        }
    }
    if rows.is_empty() {
        let Some((id, _)) = requests.iter().next() else {
            return Read::Refused(NO_REQUESTS.into());
        };
        let index = match session_index(*id) {
            Ok(index) => index,
            Err(refusal) => return refusal,
        };
        return refuse_field(index, "response.status");
    }
    finish(rows, true, has_index, orphans, false, false)
}

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let chunk = bytes.get(at..end)?;
    Some(u16::from_le_bytes([chunk[0], chunk[1]]))
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let chunk = bytes.get(at..end)?;
    Some(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

fn find_eocd(bytes: &[u8]) -> Result<usize, Read> {
    if bytes.len() < 22 {
        return Err(Read::Refused(DIRECTORY.into()));
    }
    let earliest = bytes.len().saturating_sub(22usize.saturating_add(65535));
    let mut at = bytes.len() - 22;
    let mut steps = 0usize;
    loop {
        if steps % 1024 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        steps = steps.saturating_add(1);
        if le_u32(bytes, at) == Some(0x06054b50) {
            let comment = le_u16(bytes, at.checked_add(20).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as usize;
            if at.checked_add(22).and_then(|base| base.checked_add(comment)) == Some(bytes.len()) {
                return Ok(at);
            }
        }
        if at == earliest {
            break;
        }
        at -= 1;
    }
    Err(Read::Refused(DIRECTORY.into()))
}

/// ZIP64 extra puts the real uncompressed size first when the 32-bit field is the sentinel.
fn zip64_expanded(bytes: &[u8], cursor: usize, name_len: usize, extra_len: usize) -> Result<u64, Read> {
    let extra_at = cursor.checked_add(46).and_then(|base| base.checked_add(name_len)).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
    let extra_end = extra_at.checked_add(extra_len).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
    let extra = bytes.get(extra_at..extra_end).ok_or_else(|| Read::Refused(DIRECTORY.into()))?;
    let mut index = 0usize;
    while index.checked_add(4).is_some_and(|next| next <= extra.len()) {
        if operation_stopped() {
            return Err(stopped_read());
        }
        let id = u16::from_le_bytes([extra[index], extra[index + 1]]);
        let size = u16::from_le_bytes([extra[index + 2], extra[index + 3]]) as usize;
        let data = index.checked_add(4).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
        let next = data.checked_add(size).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
        if next > extra.len() {
            return Err(Read::Refused(DIRECTORY.into()));
        }
        if id == 1 {
            if size < 8 {
                return Err(Read::Refused(DIRECTORY.into()));
            }
            let mut wide = [0u8; 8];
            wide.copy_from_slice(&extra[data..data + 8]);
            return Ok(u64::from_le_bytes(wide));
        }
        index = next;
    }
    Err(Read::Refused(DIRECTORY.into()))
}

/// Refuse on the central-directory sizes before any member is expanded.
fn session_member_caps(bytes: &[u8]) -> Result<(), Read> {
    let end = find_eocd(bytes)?;
    let disk = le_u16(bytes, end.checked_add(4).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))?;
    let disk_start = le_u16(bytes, end.checked_add(6).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))?;
    if disk != 0 || disk_start != 0 {
        return Err(Read::Refused(ARCHIVE_UNUSABLE.into()));
    }
    let count = le_u16(bytes, end.checked_add(10).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as u64;
    let cd_size = le_u32(bytes, end.checked_add(12).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as u64;
    let cd_offset = le_u32(bytes, end.checked_add(16).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as u64;
    if count >= 0xFFFF || cd_size >= 0xFFFF_FFFF || cd_offset >= 0xFFFF_FFFF {
        return Err(Read::Refused(if count >= 0xFFFF { MEMBER_CAP_REFUSAL } else { READER_LIMIT }.into()));
    }
    if count > MEMBER_CAP as u64 {
        return Err(Read::Refused(MEMBER_CAP_REFUSAL.into()));
    }
    let cd_offset = cd_offset as usize;
    let cd_end = cd_offset.checked_add(cd_size as usize).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
    if cd_end > end {
        return Err(Read::Refused(DIRECTORY.into()));
    }
    let mut cursor = cd_offset;
    let mut seen = 0u64;
    let mut total = 0u64;
    let total_cap = (MEMBER_CAP as u64).saturating_mul(MEMBER_BYTE_CAP as u64);
    while seen < count {
        if seen % 32 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        if le_u32(bytes, cursor) != Some(0x02014b50) {
            return Err(Read::Refused(DIRECTORY.into()));
        }
        let uncompressed = le_u32(bytes, cursor.checked_add(24).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as u64;
        let name_len = le_u16(bytes, cursor.checked_add(28).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as usize;
        let extra_len = le_u16(bytes, cursor.checked_add(30).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as usize;
        let comment_len = le_u16(bytes, cursor.checked_add(32).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?).ok_or_else(|| Read::Refused(DIRECTORY.into()))? as usize;
        let expanded = if uncompressed >= 0xFFFF_FFFF {
            zip64_expanded(bytes, cursor, name_len, extra_len)?
        } else {
            uncompressed
        };
        if expanded > MEMBER_BYTE_CAP as u64 {
            return Err(Read::Refused(MEMBER_BYTE_REFUSAL.into()));
        }
        total = total.checked_add(expanded).ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
        if total > total_cap {
            return Err(Read::Refused(MEMBER_BYTE_REFUSAL.into()));
        }
        cursor = cursor
            .checked_add(46)
            .and_then(|base| base.checked_add(name_len))
            .and_then(|base| base.checked_add(extra_len))
            .and_then(|base| base.checked_add(comment_len))
            .ok_or_else(|| Read::Refused(READER_LIMIT.into()))?;
        if cursor > cd_end {
            return Err(Read::Refused(DIRECTORY.into()));
        }
        seen = seen.saturating_add(1);
    }
    if cursor != cd_end {
        return Err(Read::Refused(DIRECTORY.into()));
    }
    Ok(())
}

/// A scoped read expands the archive with that token. Every other read keeps the process-wide check.
fn session_members(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Read> {
    let scoped = READ_CANCEL.with(|slot| slot.get().filter(|token| super::execution::scoped_token(*token)));
    let unpacked = if let Some(token) = scoped {
        super::archive::unpack_checked(bytes, super::archive::ArchiveStop::Token(token))
    } else {
        super::archive::unpack(bytes)
    };
    match unpacked {
        Ok(members) => Ok(members),
        Err(error) if scoped.is_some() && error == "Archive operation cancelled before completion" => Err(stopped_read()),
        Err(error) => Err(Read::Refused(archive_refusal(&error).into())),
    }
}

fn read_saz(bytes: &[u8]) -> Read {
    if operation_stopped() {
        return stopped_read();
    }
    if let Err(refusal) = session_member_caps(bytes) {
        return refusal;
    }
    if operation_stopped() {
        return stopped_read();
    }
    let members = match session_members(bytes) {
        Ok(members) => members,
        Err(refusal) => return refusal,
    };
    if operation_stopped() {
        return stopped_read();
    }
    if members.len() > MEMBER_CAP {
        return Read::Refused(MEMBER_CAP_REFUSAL.into());
    }
    if members.iter().any(|(_, data)| data.len() > MEMBER_BYTE_CAP) {
        return Read::Refused(MEMBER_BYTE_REFUSAL.into());
    }
    interpret_saz(members)
}

fn encoded_width(bytes: &[u8]) -> Option<usize> {
    let first = *bytes.first()?;
    let width = match first {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return None,
    };
    if bytes.len() < width {
        return None;
    }
    let text = std::str::from_utf8(&bytes[..width]).ok()?;
    (text.chars().count() == 1).then_some(width)
}

/// JSON text may begin with any amount of whitespace or a byte-order mark.
/// A long prefix is still a capture when the first value is an object.
fn leading_capture(bytes: &[u8]) -> Result<bool, Read> {
    let mut index = 0usize;
    while index < bytes.len() {
        if index > 0 && index % 4096 == 0 && operation_stopped() {
            return Err(stopped_read());
        }
        let Some(width) = encoded_width(&bytes[index..]) else {
            return Ok(false);
        };
        let end = match index.checked_add(width) {
            Some(end) if end <= bytes.len() => end,
            _ => return Err(Read::Refused(READER_LIMIT.into())),
        };
        let ch = std::str::from_utf8(&bytes[index..end]).ok().and_then(|text| text.chars().next()).unwrap_or('\u{FFFD}');
        if ch.is_whitespace() || ch == '\u{feff}' {
            index = end;
            continue;
        }
        return Ok(ch == '{');
    }
    Ok(false)
}

fn read(bytes: &[u8], complete: bool) -> Read {
    read_until(bytes, complete, None)
}

fn read_scoped(bytes: &[u8], complete: bool, token: u64) -> Read {
    let _cancel = CancelGuard::bind(Some(token));
    read_until(bytes, complete, None)
}

fn read_until(bytes: &[u8], complete: bool, deadline: Option<Instant>) -> Read {
    let _guard = DeadlineGuard(arm_read_deadline(deadline));
    if !complete {
        return Read::Refused(TRUNCATED.into());
    }
    if archive_prefix(bytes) {
        return read_saz(bytes);
    }
    if bytes.len() > DOCUMENT_BYTE_CAP {
        return match leading_capture(bytes) {
            Ok(true) => Read::Refused(FILE_LIMIT.into()),
            Ok(false) => Read::NotCapture,
            Err(refusal) => refusal,
        };
    }
    if operation_stopped() {
        return match leading_capture(bytes) {
            Ok(true) => stopped_read(),
            Ok(false) => Read::NotCapture,
            Err(refusal) => refusal,
        };
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let refusal = if error.error_len().is_none() { TRUNCATED } else { UNUSABLE };
            return Read::Refused(refusal.into());
        }
    };
    let trimmed = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}').trim_end();
    if !trimmed.starts_with('{') {
        return Read::NotCapture;
    }
    match parse(trimmed) {
        Ok(value) => {
            if operation_stopped() {
                return stopped_read();
            }
            let parsed = interpret(&value);
            if matches!(parsed, Read::Rows(_)) && operation_stopped() {
                return stopped_read();
            }
            parsed
        }
        Err(_) => match closure(trimmed) {
            Scan::Open => Read::Refused(TRUNCATED.into()),
            Scan::Stopped => stopped_read(),
            Scan::Closed => Read::Refused(UNUSABLE.into()),
        },
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

fn capture_view(parsed: Read) -> Json {
    match parsed {
        Read::NotCapture => refused(NOT_FILE),
        Read::Rows(capture) => view(&capture.rows, &capture.caps, None, &capture.report, &capture.observations),
        Read::Refused(reason) => refused(&reason),
    }
}

/// File bytes always produce a view. A document that is not a capture is a
/// named refusal. The report-text route still returns `None` for that case.
/// Callers that already have an operation deadline use `file_capture_until`.
pub(super) fn file_capture(bytes: &[u8], complete: bool) -> Json {
    file_capture_scoped(bytes, complete, super::execution::cancellation_token())
}

/// Same rows as `file_capture`, bound to `token`.
/// A scoped token stops this read only when that token is cancelled or the deadline passed.
/// An unscoped token still uses the process-wide cancel flag.
pub(super) fn file_capture_scoped(bytes: &[u8], complete: bool, token: u64) -> Json {
    capture_view(read_scoped(bytes, complete, token))
}

/// Stop at `deadline` when it is earlier than the 120-second read ceiling.
/// A completed row is not returned after that deadline.
pub(super) fn file_capture_until(bytes: &[u8], complete: bool, deadline: Instant) -> Json {
    capture_view(read_until(bytes, complete, Some(deadline)))
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

#[cfg(any(test, rllm_module_tests))]
mod checks {
    use super::*;
    use std::time::{Duration, Instant};

    #[cfg(rllm_module_tests)]
    #[no_mangle]
    pub extern "C" fn g7_har_evidence_tests() {
        super::registry_reader_limits();
        valid_capture_yields_method_url_status_mime_and_byte_count();
        truncated_capture_is_refused();
        missing_required_field_names_the_entry();
        entry_cap_names_itself_when_it_cuts();
        url_cap_names_itself_when_it_cuts();
        report_cap_names_itself_when_it_cuts();
        unusable_json_is_refused();
        page_text_is_not_a_saved_capture();
        method_status_class_redaction_and_body_digest_are_stated();
        redirect_pairing_is_recorded_and_a_missing_location_is_a_different_refusal();
        relative_location_is_resolved_against_the_request_url_before_pairing();
        timing_absent_says_not_recorded();
        timing_present_is_recorded_by_the_capture();
        header_cap_lists_exactly_the_cap_and_discloses_the_rest();
        header_name_cap_keeps_a_name_at_the_bound_and_discloses_past_it();
        timing_text_cap_keeps_the_recorded_token_at_the_bound();
        body_bound_refusal_changes_when_one_byte_is_removed();
        saz_pairs_members_and_the_orphan_side_changes_the_refusal();
        session_without_index_yields_one_paired_entry();
        session_timers_are_recorded_from_metadata();
        https_origin_form_session_reports_https_and_pairs_its_redirect();
        custom_session_flag_value_does_not_set_the_recorded_scheme();
        unclassable_status_is_a_refusal_row();
        notes_do_not_name_a_session_archive_program();
        entry_past_the_cap_still_refuses_a_missing_field();
        oversized_integer_is_not_wrapped();
        whole_number_spellings_match_the_published_numeric_type();
        session_id_past_u32_is_not_read();
        session_id_at_u32_max_is_read();
        capture_past_the_document_budget_is_a_reader_limit();
        cancelled_read_is_partial_and_not_decoded();
        scoped_read_completes_during_unrelated_cancel_and_stops_on_its_token();
        scoped_session_archive_follows_its_token_not_an_unrelated_cancel();
        digest_past_the_member_cap_is_not_claimed();
        utf8_bom_is_still_a_capture();
        exponent_elapsed_time_is_recorded();
        saz_status_is_exactly_three_digits();
        text_representation_is_named_and_not_called_body_size();
        http_version_token_matches_the_status_line_grammar();
        field_name_tokens_match_the_published_grammar();
        wrongly_typed_post_data_is_unusable();
        long_leading_whitespace_is_the_reader_limit();
        parse_that_crosses_the_deadline_is_partial();
        session_caps_refuse_before_expansion();
        malformed_session_messages_are_not_rows();
        header_names_keep_the_colon_against_the_token();
        post_data_params_are_an_alternative_to_text();
        caller_deadline_is_not_replaced_by_a_later_budget();
    }

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

    /// `declared` is the central-directory uncompressed size and may differ from the payload.
    fn declared_zip(members: &[(&str, u16, &[u8], u32, &[u8])]) -> Vec<u8> {
        let mut locals = Vec::new();
        let mut index = Vec::new();
        for (name, method, data, declared, extra) in members {
            let offset = locals.len() as u32;
            let name_bytes = name.as_bytes();
            push_u32(&mut locals, 0x04034b50);
            push_u16(&mut locals, 20);
            push_u16(&mut locals, 0);
            push_u16(&mut locals, *method);
            push_u16(&mut locals, 0);
            push_u16(&mut locals, 0);
            push_u32(&mut locals, crc32(data));
            push_u32(&mut locals, data.len() as u32);
            push_u32(&mut locals, *declared);
            push_u16(&mut locals, name_bytes.len() as u16);
            push_u16(&mut locals, 0);
            locals.extend_from_slice(name_bytes);
            locals.extend_from_slice(data);
            index.push(((*name).to_string(), *method, data.to_vec(), *declared, extra.to_vec(), offset));
        }
        let dir_at = locals.len();
        let mut out = locals;
        for (name, method, data, declared, extra, offset) in &index {
            let name_bytes = name.as_bytes();
            push_u32(&mut out, 0x02014b50);
            push_u16(&mut out, 20);
            push_u16(&mut out, 20);
            push_u16(&mut out, 0);
            push_u16(&mut out, *method);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u32(&mut out, crc32(data));
            push_u32(&mut out, data.len() as u32);
            push_u32(&mut out, *declared);
            push_u16(&mut out, name_bytes.len() as u16);
            push_u16(&mut out, extra.len() as u16);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u32(&mut out, 0);
            push_u32(&mut out, *offset);
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(extra);
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

    fn zip64_extra(size: u64) -> Vec<u8> {
        let mut extra = Vec::new();
        push_u16(&mut extra, 1);
        push_u16(&mut extra, 8);
        extra.extend_from_slice(&size.to_le_bytes());
        extra
    }

    fn kind_is(row: &Json, kind: &str) -> bool {
        row.get("kind").and_then(|value| value.text()).ok() == Some(kind)
    }

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
    fn method_status_class_redaction_and_body_digest_are_stated() {
        // Fails if the class is omitted, a sensitive value is copied, or the body text is used in place of its digest.
        assert_eq!(sha256(b"abc").as_deref(), Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert_eq!(sha256(b"body-marker-xyz").as_deref(), Some("0120cc7f8974d602cba67f1a9e22e7d61c048f9c6fbb4e176c5de411c1629099"));
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
        assert!(capture.report.contains("Body 0 request text representation length 15 sha256 0120cc7f8974d602cba67f1a9e22e7d61c048f9c6fbb4e176c5de411c1629099."), "{}", capture.report);
        assert!(capture.report.contains("Body 0 response text representation length 3 sha256 ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad."), "{}", capture.report);
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
    fn relative_location_is_resolved_against_the_request_url_before_pairing() {
        // Fails if Location /next is compared raw and misses the later absolute request https://e.test/next.
        fn capture_entry(url: &str, status: u16, location: Option<&str>) -> String {
            let headers = match location {
                Some(value) => format!(r#","headers":[{{"name":"Location","value":"{value}"}}]"#),
                None => String::new(),
            };
            format!(
                r#"{{"request":{{"method":"GET","url":"{url}"}},"response":{{"status":{status}{headers},"content":{{"mimeType":"text/html","size":0}}}}}}"#
            )
        }
        let entries = vec![
            capture_entry("https://e.test/dir/page", 302, Some("/next")),
            capture_entry("/next", 200, None),
            capture_entry("https://e.test/next", 200, None),
            capture_entry("https://e.test/dir/page", 302, Some("next")),
            capture_entry("next", 200, None),
            capture_entry("https://e.test/dir/next", 200, None),
            capture_entry("https://e.test/dir/page", 302, Some("//cdn.test/asset")),
            capture_entry("//cdn.test/asset", 200, None),
            capture_entry("https://cdn.test/asset", 200, None),
            capture_entry("https://e.test/search?old=1", 302, Some("?q=1")),
            capture_entry("?q=1", 200, None),
            capture_entry("https://e.test/search?q=1", 200, None),
            capture_entry("https://e.test/dir/page", 302, Some("../other")),
            capture_entry("https://e.test/dir/other", 200, None),
            capture_entry("https://e.test/other", 200, None),
            capture_entry("https://e.test/start", 302, Some("https://e.test/a/../b")),
            capture_entry("https://e.test/b", 200, None),
            capture_entry("https://e.test/a/../b", 200, None),
            capture_entry("https://e.test/start", 302, Some("https://other.test/abs")),
            capture_entry("https://e.test/abs", 200, None),
            capture_entry("https://other.test/abs", 200, None),
            capture_entry("http://e.test/dir/page", 302, Some("//cdn.test/asset")),
            capture_entry("https://cdn.test/asset", 200, None),
            capture_entry("http://cdn.test/asset", 200, None),
            capture_entry("https://e.test:8443/dir/page", 302, Some("/next")),
            capture_entry("https://e.test/next", 200, None),
            capture_entry("https://e.test:8443/next", 200, None),
            capture_entry("https://e.test/a/b", 302, Some("?url=https://other.test/")),
            capture_entry("https://other.test/", 200, None),
            capture_entry("https://e.test/a/b?url=https://other.test/", 200, None),
            capture_entry("https://e.test/dir/page/", 302, Some("next")),
            capture_entry("https://e.test/dir/next", 200, None),
            capture_entry("https://e.test/dir/page/next", 200, None),
            capture_entry("https://e.test/start", 302, Some("/missing")),
        ];
        let raw = document(&entries);
        let capture = rows(raw.as_bytes());
        let paired = [
            "Redirect 0 recorded pairing entry 2 target /next.",
            "Redirect 3 recorded pairing entry 5 target next.",
            "Redirect 6 recorded pairing entry 8 target //cdn.test/asset.",
            "Redirect 9 recorded pairing entry 11 target ?q=1.",
            "Redirect 12 recorded pairing entry 14 target ../other.",
            "Redirect 15 recorded pairing entry 17 target https://e.test/a/../b.",
            "Redirect 18 recorded pairing entry 20 target https://other.test/abs.",
            "Redirect 21 recorded pairing entry 23 target //cdn.test/asset.",
            "Redirect 24 recorded pairing entry 26 target /next.",
            "Redirect 27 recorded pairing entry 29 target ?url=https://other.test/.",
            "Redirect 30 recorded pairing entry 32 target next.",
        ];
        for line in paired {
            assert!(capture.report.contains(line), "missing {line}\n{}", capture.report);
        }
        let unmatched = "Redirect 33 location not followed by a recorded request. target /missing.";
        assert!(capture.report.contains(unmatched), "{}", capture.report);
        for wrong in [
            "Redirect 0 recorded pairing entry 1",
            "Redirect 0 location not followed",
            "Redirect 3 recorded pairing entry 4",
            "Redirect 6 recorded pairing entry 7",
            "Redirect 9 recorded pairing entry 10",
            "Redirect 12 recorded pairing entry 13",
            "Redirect 15 recorded pairing entry 16",
            "Redirect 18 recorded pairing entry 19",
            "Redirect 21 recorded pairing entry 22",
            "Redirect 24 recorded pairing entry 25",
            "Redirect 27 recorded pairing entry 28",
            "Redirect 30 recorded pairing entry 31",
        ] {
            assert!(!capture.report.contains(wrong), "unexpected {wrong}\n{}", capture.report);
        }
        assert!(!capture.report.contains("followed the redirect"), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn timing_absent_says_not_recorded() {
        // Fails if a capture with no time or timings object is described as measured or recorded.
        let raw = document(&[one("GET", "https://example.test/item", 200, "text/html", 12)]);
        let capture = rows(raw.as_bytes());
        assert!(capture.report.contains("Timing: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("recorded by the capture"), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn timing_present_is_recorded_by_the_capture() {
        // Fails if the elapsed token is dropped, rewritten, or labeled as an RLLM measurement.
        let raw = r#"{"log":{"entries":[{"time":15,"request":{"method":"GET","url":"https://example.test/item"},"response":{"status":200,"content":{"mimeType":"text/html","size":12}}}]}}"#;
        let capture = rows(raw.as_bytes());
        assert!(capture.report.contains("Timing 0 elapsed 15 ms recorded by the capture, not measured by RLLM."), "{}", capture.report);
        assert!(!capture.report.contains("Timing: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("Timing 0 not recorded."), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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
        let over_line = format!("Refusal: entry 0 request text representation exceeds the saved-capture member byte cap. text length {over_len}.");
        assert!(over.report.contains(&over_line), "{}", over.report);
        assert!(!over.report.contains("sha256"), "{}", over.report);
        assert!(!over.report.contains(&marker), "{}", over.report);
        assert!(!over.report.contains(&format!("size {over_len}")), "{}", over.report);
        let at = rows(raw(MEMBER_BYTE_CAP).as_bytes());
        let at_line = at.report.lines().find(|line| line.starts_with("Body 0 request text representation")).expect("in-bound body row");
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
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

    #[cfg_attr(not(rllm_module_tests), test)]
    fn notes_do_not_name_a_session_archive_program() {
        // Fails if a note is rebuilt from a list of character codes, or if a session report names a program.
        let source = include_str!("har_evidence.rs");
        assert!(!source.contains(&format!("{}{}", "from_utf8", "(&[")));
        assert!(!source.contains(&format!("{}{}", "u8", ",")));
        assert!(source.contains("session archive"));
        let archive = session_zip(&[
            ("001_c.txt", b"GET https://e.test/s HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
        ]);
        let capture = rows(&archive);
        assert!(capture.report.contains("index.dat present."), "{}", capture.report);
        assert!(capture.report.contains("Pairing session 1 request 001_c.txt response 001_s.txt."), "{}", capture.report);
        assert!(!capture.report.to_ascii_lowercase().contains("program"));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn entry_past_the_cap_still_refuses_a_missing_field() {
        // Fails if rows past the entry cap skip required-field checks and the capture is emitted.
        let mut entries: Vec<String> = (0..ENTRY_CAP).map(|index| one("GET", &format!("https://e.test/{index}"), 200, "text/plain", 1)).collect();
        entries.push(r#"{"request":{"method":"GET","url":"https://e.test/broken"}}"#.to_string());
        match read(document(&entries).as_bytes(), true) {
            Read::Refused(reason) => assert!(reason.contains(&format!("entry {ENTRY_CAP} missing response.status")), "{reason}"),
            other => panic!("entry past the cap was not refused: {other:?}"),
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn oversized_integer_is_not_wrapped() {
        // Fails if a count past u64, or a fractional byte count, becomes a row value.
        let status = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/n"},"response":{"status":18446744073709551616,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        match read(status.as_bytes(), true) {
            Read::Refused(reason) => {
                assert!(reason.contains("entry 0 missing response.status"), "{reason}");
                assert!(!reason.contains("0 GET"), "{reason}");
            }
            other => panic!("oversized status was not refused: {other:?}"),
        }
        let size = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/n"},"response":{"status":200,"content":{"mimeType":"text/plain","size":18446744073709551616}}}]}}"#;
        match read(size.as_bytes(), true) {
            Read::Refused(reason) => assert!(reason.contains("entry 0 missing response.content.size"), "{reason}"),
            other => panic!("oversized size was not refused: {other:?}"),
        }
        let fraction = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/n"},"response":{"status":200,"content":{"mimeType":"text/plain","size":40.5}}}]}}"#;
        match read(fraction.as_bytes(), true) {
            Read::Refused(reason) => assert!(reason.contains("entry 0 missing response.content.size"), "{reason}"),
            other => panic!("fractional size was not refused: {other:?}"),
        }
        let exponent = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/n"},"response":{"status":200,"content":{"mimeType":"text/plain","size":1e20}}}]}}"#;
        match read(exponent.as_bytes(), true) {
            Read::Refused(reason) => assert!(reason.contains("entry 0 missing response.content.size"), "{reason}"),
            other => panic!("exponent past u64 was not refused: {other:?}"),
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn whole_number_spellings_match_the_published_numeric_type() {
        // Fails if 40.0 or 4e1 is dropped, or if 201.0 is not status 201.
        let raw = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/n"},"response":{"status":201.0,"content":{"mimeType":"text/plain","size":40.0}}},{"request":{"method":"GET","url":"https://e.test/e"},"response":{"status":200,"content":{"mimeType":"text/plain","size":4e1}}}]}}"#;
        let capture = rows(raw.as_bytes());
        assert_eq!(capture.rows[0].status, 201);
        assert_eq!(capture.rows[0].bytes, 40);
        assert_eq!(capture.rows[1].status, 200);
        assert_eq!(capture.rows[1].bytes, 40);
    }

    fn session_zip(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut all = vec![("index.dat", &b"session"[..])];
        all.extend_from_slice(members);
        zip(&all)
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn session_id_past_u32_is_not_read() {
        // Fails if a session prefix past u32 is skipped and the other session is reported as a finished pairing.
        let archive = session_zip(&[
            ("001_c.txt", b"GET https://e.test/ok HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
            ("4294967296_c.txt", b"GET https://e.test/HIDDEN HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("4294967296_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nsecret-body"),
        ]);
        match read(&archive, true) {
            Read::Refused(reason) => {
                assert_eq!(reason, SESSION_ID_LIMIT);
                assert!(!reason.contains("HIDDEN"));
                assert!(!reason.contains("Pairing session"));
                assert!(!reason.contains("secret-body"));
            }
            other => panic!("oversized session id was read: {other:?}"),
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn session_id_at_u32_max_is_read() {
        // Fails if the largest prefix that fits is treated as past the limit.
        let name_c = format!("{}_c.txt", u32::MAX);
        let name_s = format!("{}_s.txt", u32::MAX);
        let archive = session_zip(&[
            (name_c.as_str(), b"GET https://e.test/max HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            (name_s.as_str(), b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
        ]);
        let capture = rows(&archive);
        assert_eq!(capture.rows.len(), 1);
        assert_eq!(capture.rows[0].index, u32::MAX as usize);
        assert_eq!(capture.rows[0].status, 200);
        assert!(capture.report.contains(&format!("Pairing session {} ", u32::MAX)), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn capture_past_the_document_budget_is_a_reader_limit() {
        // Fails if a document past the byte budget is scanned into rows, or if a non-capture page becomes a refusal.
        let mut bytes = vec![b'{'; DOCUMENT_BYTE_CAP + 1];
        bytes.extend_from_slice(b"\"url\":\"https://e.test/SHOULD-NOT-PARSE\"");
        match read(&bytes, true) {
            Read::Refused(reason) => {
                assert_eq!(reason, FILE_LIMIT);
                assert!(!reason.contains("SHOULD-NOT-PARSE"));
            }
            other => panic!("oversized capture was not a reader limit: {other:?}"),
        }
        let page = vec![b'<'; DOCUMENT_BYTE_CAP + 1];
        assert!(matches!(read(&page, true), Read::NotCapture));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn cancelled_read_is_partial_and_not_decoded() {
        // Fails if a cancelled read still returns rows or omits the partial reason.
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                super::super::execution::reset();
            }
        }
        let _reset = Reset;
        super::super::execution::cancel();
        let raw = document(&[one("GET", "https://e.test/cancel", 200, "text/plain", 1)]);
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_STOP);
                assert!(reason.contains("partial"), "{reason}");
                assert!(!reason.contains("https://e.test/cancel"), "{reason}");
            }
            other => panic!("cancelled read was not partial: {other:?}"),
        }
        assert!(matches!(read(b"<html>page</html>", true), Read::NotCapture));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn scoped_read_completes_during_unrelated_cancel_and_stops_on_its_token() {
        // Fails if a live scoped token is refused because an unrelated cancel is set,
        // or if cancelling that token still returns rows.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                READ_CANCEL.with(|slot| slot.set(None));
                super::super::execution::reset();
            }
        }
        let _reset = Reset;
        super::super::execution::reset();
        let stale = super::super::execution::cancellation_token();
        let raw = document(&[one("GET", "https://e.test/scope", 200, "text/plain", 1)]);
        let flag = Arc::new(AtomicBool::new(false));
        let scope = super::super::execution::CancellationScope::new(flag.clone()).expect("scoped read");
        let token = scope.token();
        assert!(super::super::execution::scoped_token(token));
        super::super::execution::cancel();
        assert!(super::super::execution::cancelled());
        assert!(!super::super::execution::token_cancelled(token));
        match read_scoped(raw.as_bytes(), true, token) {
            Read::Rows(capture) => {
                assert_eq!(capture.rows.len(), 1);
                assert_eq!(capture.rows[0].method, "GET");
                assert_eq!(capture.rows[0].url, "https://e.test/scope");
                assert_eq!(capture.rows[0].status, 200);
            }
            other => panic!("a live scoped read was partial during an unrelated cancel: {other:?}"),
        }
        let live = file_capture_scoped(raw.as_bytes(), true, token);
        let live_text = live.string();
        assert!(live_text.contains("https://e.test/scope"), "{live_text}");
        assert!(!live_text.contains("partial"), "{live_text}");
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_STOP);
                assert!(!reason.contains("https://e.test/scope"), "{reason}");
            }
            other => panic!("an unscoped read ignored the process-wide cancel: {other:?}"),
        }
        super::super::execution::reset();
        assert!(super::super::execution::token_cancelled(stale));
        assert!(!super::super::execution::cancelled());
        match read_scoped(raw.as_bytes(), true, stale) {
            Read::Rows(capture) => assert_eq!(capture.rows[0].url, "https://e.test/scope"),
            other => panic!("an unscoped token was refused after the process-wide flag was cleared: {other:?}"),
        }
        flag.store(true, Ordering::SeqCst);
        assert!(super::super::execution::token_cancelled(token));
        match read_scoped(raw.as_bytes(), true, token) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_STOP);
                assert!(reason.contains("partial"), "{reason}");
                assert!(!reason.contains("https://e.test/scope"), "{reason}");
            }
            other => panic!("a scoped read whose token was cancelled was not partial: {other:?}"),
        }
        let stopped = file_capture_scoped(raw.as_bytes(), true, token);
        let stopped_text = stopped.string();
        assert!(stopped_text.contains("partial"), "{stopped_text}");
        assert!(stopped_text.contains("cancelled"), "{stopped_text}");
        assert!(!stopped_text.contains("https://e.test/scope"), "{stopped_text}");
        assert!(matches!(read_scoped(b"<html>page</html>", true, token), Read::NotCapture));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn scoped_session_archive_follows_its_token_not_an_unrelated_cancel() {
        // Fails if a live scoped token is refused while an unrelated cancel is set,
        // or if cancelling that token still returns session rows.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                READ_CANCEL.with(|slot| slot.set(None));
                super::super::execution::reset();
            }
        }
        let _reset = Reset;
        super::super::execution::reset();
        let archive = session_zip(&[
            ("001_c.txt", b"GET https://e.test/session-scope HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
        ]);
        let ordinary = rows(&archive);
        assert_eq!(ordinary.rows.len(), 1);
        assert_eq!(ordinary.rows[0].url, "https://e.test/session-scope");
        assert_eq!(ordinary.rows[0].status, 200);
        let flag = Arc::new(AtomicBool::new(false));
        let scope = super::super::execution::CancellationScope::new(flag.clone()).expect("scoped session archive");
        let token = scope.token();
        assert!(super::super::execution::scoped_token(token));
        super::super::execution::cancel();
        assert!(super::super::execution::cancelled());
        assert!(!super::super::execution::token_cancelled(token));
        let process_wide = super::super::archive::unpack(&archive).unwrap_err();
        assert!(process_wide.contains("cancelled"), "{process_wide}");
        assert!(!process_wide.contains("session-scope"), "{process_wide}");
        let opened = super::super::archive::unpack_checked(&archive, super::super::archive::ArchiveStop::Token(token))
            .unwrap_or_else(|error| panic!("a live scoped archive unpack was refused during an unrelated cancel: {error}"));
        assert!(opened.iter().any(|(_, data)| data.windows(b"e.test/session-scope".len()).any(|part| part == b"e.test/session-scope")));
        match read_scoped(&archive, true, token) {
            Read::Rows(capture) => {
                assert_eq!(capture.rows.len(), 1);
                assert_eq!(capture.rows[0].method, "GET");
                assert_eq!(capture.rows[0].url, "https://e.test/session-scope");
                assert_eq!(capture.rows[0].status, 200);
            }
            other => panic!("a live scoped session archive was refused during an unrelated cancel: {other:?}"),
        }
        let live = file_capture_scoped(&archive, true, token);
        let live_text = live.string();
        assert!(live_text.contains("https://e.test/session-scope"), "{live_text}");
        assert!(!live_text.contains("unusable"), "{live_text}");
        assert!(!live_text.contains("partial"), "{live_text}");
        match read(&archive, true) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_STOP);
                assert!(!reason.contains("https://e.test/session-scope"), "{reason}");
            }
            other => panic!("an unscoped session archive ignored the process-wide cancel: {other:?}"),
        }
        super::super::execution::reset();
        assert!(!super::super::execution::cancelled());
        assert!(super::super::archive::unpack(&archive).is_ok());
        flag.store(true, Ordering::SeqCst);
        assert!(super::super::execution::token_cancelled(token));
        let stopped_unpack = super::super::archive::unpack_checked(&archive, super::super::archive::ArchiveStop::Token(token)).unwrap_err();
        assert!(stopped_unpack.contains("cancelled"), "{stopped_unpack}");
        assert!(!stopped_unpack.contains("session-scope"), "{stopped_unpack}");
        assert!(super::super::archive::unpack(&archive).is_ok());
        match read_scoped(&archive, true, token) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_STOP);
                assert!(reason.contains("partial"), "{reason}");
                assert!(reason.contains("cancelled"), "{reason}");
                assert!(!reason.contains("https://e.test/session-scope"), "{reason}");
            }
            other => panic!("a scoped session archive whose token was cancelled was not partial: {other:?}"),
        }
        let stopped = file_capture_scoped(&archive, true, token);
        let stopped_text = stopped.string();
        assert!(stopped_text.contains("partial"), "{stopped_text}");
        assert!(stopped_text.contains("cancelled"), "{stopped_text}");
        assert!(!stopped_text.contains("https://e.test/session-scope"), "{stopped_text}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn digest_past_the_member_cap_is_not_claimed() {
        // Fails if a body past the member cap still produces a digest.
        assert!(sha256(&vec![b'B'; MEMBER_BYTE_CAP + 1]).is_none());
        let digest = sha256(&vec![b'B'; MEMBER_BYTE_CAP]).expect("in-bound body has a digest");
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn utf8_bom_is_still_a_capture() {
        // Fails if a leading byte-order mark makes a capture into "not a capture",
        // or if a marked page is refused as a broken capture.
        let raw = document(&[one("GET", "https://e.test/bom", 200, "text/plain", 4)]);
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(raw.as_bytes());
        let capture = rows(&bytes);
        assert_eq!(capture.rows[0].method, "GET");
        assert_eq!(capture.rows[0].url, "https://e.test/bom");
        assert_eq!(capture.rows[0].status, 200);
        assert_eq!(capture.rows[0].bytes, 4);
        let mut spaced = b"\n\t".to_vec();
        spaced.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        spaced.extend(raw.as_bytes());
        let spaced_capture = rows(&spaced);
        assert_eq!(spaced_capture.rows[0].url, "https://e.test/bom");
        assert!(matches!(read(b"\xEF\xBB\xBF<html>no</html>", true), Read::NotCapture));
        assert!(saved_capture(b"\xEF\xBB\xBF<html>no</html>").is_none());
        assert!(matches!(read(b"\xEF\xBB\xBF", true), Read::NotCapture));
        let mut huge = vec![0xEF, 0xBB, 0xBF, b'{'];
        huge.resize(DOCUMENT_BYTE_CAP + 1, b' ');
        match read(&huge, true) {
            Read::Refused(reason) => {
                assert_eq!(reason, FILE_LIMIT);
                assert!(!reason.contains("decoded"), "{reason}");
            }
            other => panic!("marked capture over the budget was not a reader limit: {other:?}"),
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn exponent_elapsed_time_is_recorded() {
        // Fails if 1e2, or another exponent spelling, is dropped or called unusable.
        for time in ["1e2", "1E2", "1e+2", "1.5e1", "1e-1"] {
            let raw = format!(
                r#"{{"log":{{"entries":[{{"time":{time},"request":{{"method":"GET","url":"https://e.test/t"}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
            );
            let capture = rows(raw.as_bytes());
            assert!(
                capture.report.contains(&format!("Timing 0 elapsed {time} ms recorded by the capture, not measured by RLLM.")),
                "{time} in {}",
                capture.report
            );
            assert!(!capture.report.contains("not a recorded elapsed time"), "{time} in {}", capture.report);
        }
        let negative = r#"{"log":{"entries":[{"time":-1e2,"request":{"method":"GET","url":"https://e.test/t"},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(negative.as_bytes());
        assert!(capture.report.contains("Refusal: entry 0 timing is not a recorded elapsed time."), "{}", capture.report);
        assert!(!capture.report.contains("elapsed -1e2"), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn saz_status_is_exactly_three_digits() {
        // Fails if 0200 is stored as status 200 or classed 2xx, or if a real 200 is refused.
        let ok = session_zip(&[
            ("001_c.txt", b"GET https://e.test/s HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nAB"),
        ]);
        let capture = rows(&ok);
        assert_eq!(capture.rows[0].status, 200);
        assert!(capture.report.contains("class 2xx."), "{}", capture.report);
        let digest = sha256(b"AB").expect("body digest");
        assert!(capture.report.contains(&format!("present length 2 sha256 {digest}.")), "{}", capture.report);
        assert!(!capture.report.contains("text representation"), "{}", capture.report);
        let narrow = session_zip(&[
            ("001_c.txt", b"GET https://e.test/s HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 020 OK\r\nContent-Type: text/plain\r\n\r\nAB"),
        ]);
        let twenty = rows(&narrow);
        assert_eq!(twenty.rows[0].status, 20);
        assert!(twenty.report.contains("Refusal: entry 1 status class is unanswerable."), "{}", twenty.report);
        assert!(!twenty.report.contains("2xx"), "{}", twenty.report);
        assert!(!twenty.report.contains("status 200"), "{}", twenty.report);
        for status in ["0200", "20", "2000", "2", "20000"] {
            let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: text/plain\r\n\r\nbody");
            let archive = session_zip(&[
                ("001_c.txt", b"GET https://e.test/s HTTP/1.1\r\nHost: e.test\r\n\r\n"),
                ("001_s.txt", response.as_bytes()),
            ]);
            match read(&archive, true) {
                Read::Refused(reason) => {
                    assert!(reason.contains("missing response.status"), "{status}: {reason}");
                    assert!(!reason.contains("2xx"), "{status}: {reason}");
                    assert!(!reason.contains("status 200"), "{status}: {reason}");
                }
                Read::Rows(capture) => panic!("{status} became status {:?} report {}", capture.rows.iter().map(|row| row.status).collect::<Vec<_>>(), capture.report),
                other => panic!("{status} was not refused: {other:?}"),
            }
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn text_representation_is_named_and_not_called_body_size() {
        // Fails if the UTF-8 length of capture text is reported as the body size,
        // or if an encoded text field is digested and its character count is called size.
        let raw = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/t"},"response":{"status":200,"content":{"mimeType":"text/plain","size":1,"text":"\u00e9"}}}]}}"#;
        let capture = rows(raw.as_bytes());
        assert_eq!(capture.rows[0].bytes, 1);
        let digest = sha256("\u{00e9}".as_bytes()).expect("text digest");
        assert_eq!("\u{00e9}".len(), 2);
        assert!(capture.report.contains(&format!("Body 0 response text representation length 2 sha256 {digest}.")), "{}", capture.report);
        assert!(!capture.report.contains("present length 2"), "{}", capture.report);
        assert!(!capture.report.contains("body length 2"), "{}", capture.report);
        assert!(!capture.report.contains(" size 2"), "{}", capture.report);
        let encoded = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/t","postData":{"mimeType":"text/plain","text":"YQ==","encoding":"base64"}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1,"text":"YQ==","encoding":"base64"}}}]}}"#;
        let unread = rows(encoded.as_bytes());
        assert_eq!(unread.rows[0].bytes, 1);
        assert!(unread.report.contains("Refusal: entry 0 request text representation is encoded and was not read. text length 4."), "{}", unread.report);
        assert!(unread.report.contains("Refusal: entry 0 response text representation is encoded and was not read. text length 4."), "{}", unread.report);
        assert!(!unread.report.contains("sha256"), "{}", unread.report);
        assert!(!unread.report.contains("size 4"), "{}", unread.report);
        assert!(!unread.report.contains("present length"), "{}", unread.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn http_version_token_matches_the_status_line_grammar() {
        // Fails if HTTP/not-a-version 200 OK is stored as status 200 or classed 2xx.
        let ok = session_zip(&[
            ("001_c.txt", b"GET https://e.test/v HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
        ]);
        let capture = rows(&ok);
        assert_eq!(capture.rows[0].status, 200);
        assert!(capture.report.contains("class 2xx."), "{}", capture.report);
        for (version, status, class) in [("HTTP/1.0", 200, "2xx"), ("HTTP/2.0", 204, "2xx"), ("HTTP/3.0", 100, "1xx")] {
            let response = format!("{version} {status} OK\r\nContent-Type: text/plain\r\n\r\nok");
            let archive = session_zip(&[
                ("001_c.txt", b"GET https://e.test/v HTTP/1.1\r\nHost: e.test\r\n\r\n"),
                ("001_s.txt", response.as_bytes()),
            ]);
            let capture = rows(&archive);
            assert_eq!(capture.rows[0].status, status, "{version}");
            assert!(capture.report.contains(&format!("class {class}.")), "{version} in {}", capture.report);
        }
        for version in ["HTTP/not-a-version", "HTTP/1", "HTTP/1.", "HTTP/1.10", "http/1.1", "HTTP/1.1x", "HTTPS/1.1"] {
            let response = format!("{version} 200 OK\r\nContent-Type: text/plain\r\n\r\nbody");
            let archive = session_zip(&[
                ("001_c.txt", b"GET https://e.test/v HTTP/1.1\r\nHost: e.test\r\n\r\n"),
                ("001_s.txt", response.as_bytes()),
            ]);
            match read(&archive, true) {
                Read::Refused(reason) => {
                    assert!(reason.contains("missing response.status"), "{version}: {reason}");
                    assert!(!reason.contains("2xx"), "{version}: {reason}");
                    assert!(!reason.contains("status 200"), "{version}: {reason}");
                    assert!(!reason.contains("body"), "{version}: {reason}");
                }
                other => panic!("{version} was accepted: {other:?}"),
            }
        }
        let bad_request = session_zip(&[
            ("001_c.txt", b"GET https://e.test/v HTTP/not-a-version\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
        ]);
        match read(&bad_request, true) {
            Read::Refused(reason) => {
                assert!(reason.contains("missing request.method"), "{reason}");
                assert!(!reason.contains("2xx"), "{reason}");
                assert!(!reason.contains("Pairing session"), "{reason}");
            }
            other => panic!("malformed request version was accepted: {other:?}"),
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn field_name_tokens_match_the_published_grammar() {
        // Fails if a published field-name token is marked unusable, or if a separator is kept.
        for name in ["!", "Cost$", "Pct%", "And&", "Star*", "Bit^", "Tick`", "Pipe|", "Tilde~", "Quote'", "Hash#", "Plus+", "X-Trace"] {
            let raw = format!(
                r#"{{"log":{{"entries":[{{"request":{{"method":"GET","url":"https://e.test/h","headers":[{{"name":"{name}","value":"1"}}]}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
            );
            let capture = rows(raw.as_bytes());
            assert!(capture.report.contains(&format!("Header 0 request {name}.")), "{name} in {}", capture.report);
            assert!(!capture.report.contains("header is unusable"), "{name} in {}", capture.report);
        }
        for name in ["Bad(Name", "Bad Name", "Bad/Name", "Bad,Name"] {
            let raw = format!(
                r#"{{"log":{{"entries":[{{"request":{{"method":"GET","url":"https://e.test/h","headers":[{{"name":"{name}","value":"1"}}]}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
            );
            let capture = rows(raw.as_bytes());
            assert!(capture.report.contains("Refusal: entry 0 request header is unusable."), "{name} in {}", capture.report);
            assert!(!capture.report.contains(&format!("Header 0 request {name}.")), "{name} in {}", capture.report);
        }
        let archive = session_zip(&[
            ("001_c.txt", b"GET https://e.test/h HTTP/1.1\r\nWeird!: 1\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nStar*: 1\r\n\r\nok"),
        ]);
        let capture = rows(&archive);
        assert!(capture.report.contains("Header 1 request Weird!."), "{}", capture.report);
        assert!(capture.report.contains("Header 1 response Star*."), "{}", capture.report);
        assert!(!capture.report.contains("header is unusable"), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn wrongly_typed_post_data_is_unusable() {
        // Fails if a present but non-object postData is described as not recorded.
        for value in ["\"text\"", "1", "true", "null", "[\"a\"]"] {
            let raw = format!(
                r#"{{"log":{{"entries":[{{"request":{{"method":"POST","url":"https://e.test/p","postData":{value}}},"response":{{"status":200,"content":{{"mimeType":"text/plain","size":1}}}}}}]}}}}"#
            );
            let capture = rows(raw.as_bytes());
            assert!(capture.report.contains("Refusal: entry 0 request body is unusable."), "{value} in {}", capture.report);
            assert!(!capture.report.contains("Body 0 request: not recorded."), "{value} in {}", capture.report);
        }
        let absent = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/p","postData":{"mimeType":"text/plain"}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(absent.as_bytes());
        assert!(capture.report.contains("Body 0 request: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("request body is unusable"), "{}", capture.report);
        let text = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/p","postData":{"mimeType":"text/plain","text":"hi"}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(text.as_bytes());
        assert!(capture.report.contains("Body 0 request text representation length 2 "), "{}", capture.report);
        assert!(!capture.report.contains("request body is unusable"), "{}", capture.report);
        let bad_text = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/p","postData":{"text":1}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(bad_text.as_bytes());
        assert!(capture.report.contains("Refusal: entry 0 request body is unusable."), "{}", capture.report);
        let timings = r#"{"log":{"entries":[{"timings":"slow","request":{"method":"GET","url":"https://e.test/t"},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(timings.as_bytes());
        assert!(capture.report.contains("Refusal: entry 0 timing is not a recorded elapsed time."), "{}", capture.report);
        assert!(!capture.report.contains("timings present"), "{}", capture.report);
        let breakdown = r#"{"log":{"entries":[{"timings":{"wait":1},"request":{"method":"GET","url":"https://e.test/t"},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(breakdown.as_bytes());
        assert!(capture.report.contains("timings present"), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn long_leading_whitespace_is_the_reader_limit() {
        // Fails if more than 64 bytes of leading whitespace make an oversized object a non-capture.
        for pad in [64usize, 65, 200] {
            let mut bytes = vec![b' '; pad];
            bytes.push(b'{');
            bytes.resize(DOCUMENT_BYTE_CAP + 1, b' ');
            match read(&bytes, true) {
                Read::Refused(reason) => {
                    assert_eq!(reason, FILE_LIMIT, "pad {pad}: {reason}");
                    assert!(!reason.contains("decoded"), "{reason}");
                }
                other => panic!("pad {pad} was not a reader limit: {other:?}"),
            }
        }
        let mut mixed = vec![b'\t'; 40];
        mixed.extend(std::iter::repeat(b'\n').take(40));
        mixed.extend_from_slice(&[0xEF, 0xBB, 0xBF, b'{']);
        mixed.resize(DOCUMENT_BYTE_CAP + 1, b' ');
        match read(&mixed, true) {
            Read::Refused(reason) => assert_eq!(reason, FILE_LIMIT, "{reason}"),
            other => panic!("marked whitespace prefix was not a reader limit: {other:?}"),
        }
        let mut page = vec![b' '; 80];
        page.push(b'<');
        page.resize(DOCUMENT_BYTE_CAP + 1, b' ');
        assert!(matches!(read(&page, true), Read::NotCapture));
        let raw = document(&[one("GET", "https://e.test/ws", 200, "text/plain", 4)]);
        let mut small = vec![b' '; 70];
        small.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        small.extend(raw.as_bytes());
        let capture = rows(&small);
        assert_eq!(capture.rows[0].url, "https://e.test/ws");
        assert_eq!(capture.rows[0].status, 200);
        assert_eq!(capture.rows[0].bytes, 4);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn parse_that_crosses_the_deadline_is_partial() {
        // Fails if a parse that crosses the deadline still returns completed rows.
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                READ_DEADLINE.with(|slot| slot.set(None));
                DEADLINE_CHECKS_LEFT.with(|slot| slot.set(None));
                super::super::execution::reset();
            }
        }
        let _reset = Reset;
        DEADLINE_CHECKS_LEFT.with(|slot| slot.set(None));
        READ_DEADLINE.with(|slot| slot.set(None));
        let raw = document(&[one("GET", "https://e.test/deadline", 200, "text/plain", 1)]);
        DEADLINE_CHECKS_LEFT.with(|slot| slot.set(Some(1)));
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_DEADLINE);
                assert!(reason.contains("partial"), "{reason}");
                assert!(reason.contains("deadline"), "{reason}");
                assert!(!reason.contains("https://e.test/deadline"), "{reason}");
                assert!(!reason.contains("2xx"), "{reason}");
                assert!(!reason.contains("cancelled"), "{reason}");
            }
            other => panic!("a parse that crossed the deadline was completed: {other:?}"),
        }
        DEADLINE_CHECKS_LEFT.with(|slot| slot.set(None));
        READ_DEADLINE.with(|slot| slot.set(Some(Instant::now() - Duration::from_secs(1))));
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_DEADLINE);
                assert!(!reason.contains("https://e.test/deadline"), "{reason}");
                assert!(!reason.contains("cancelled"), "{reason}");
            }
            other => panic!("an expired deadline was completed: {other:?}"),
        }
        READ_DEADLINE.with(|slot| slot.set(None));
        let capture = rows(raw.as_bytes());
        assert_eq!(capture.rows[0].status, 200);
        assert!(capture.report.contains("class 2xx."), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn session_caps_refuse_before_expansion() {
        // Fails if a declared member past 64 KiB, or more than 320 members, is expanded before the cap refusal.
        let hostile = declared_zip(&[("a.txt", 8, b"TINY", (MEMBER_BYTE_CAP as u32).saturating_add(1), b"")]);
        match read(&hostile, true) {
            Read::Refused(reason) => {
                assert_eq!(reason, MEMBER_BYTE_REFUSAL, "{reason}");
                assert!(!reason.contains("TINY"), "{reason}");
                assert!(!reason.contains("decoded"), "{reason}");
            }
            other => panic!("oversized declared member was expanded: {other:?}"),
        }
        let wide = zip64_extra((MEMBER_BYTE_CAP as u64).saturating_add(1));
        let zip64 = declared_zip(&[("a.txt", 0, b"TINY", u32::MAX, &wide)]);
        match session_member_caps(&zip64) {
            Err(Read::Refused(reason)) => assert_eq!(reason, MEMBER_BYTE_REFUSAL, "{reason}"),
            other => panic!("zip64 oversized member was not capped: {other:?}"),
        }
        let small_extra = zip64_extra(4);
        let small_zip64 = declared_zip(&[("note.txt", 0, b"TINY", u32::MAX, &small_extra)]);
        assert!(session_member_caps(&small_zip64).is_ok(), "a four-byte zip64 member was treated as over the byte cap");
        let mut many = Vec::new();
        let payload = b"x";
        for index in 0..321 {
            let name = format!("m{index}.txt");
            let method: u16 = if index == 0 { 99 } else { 0 };
            many.push((name, method, payload.as_slice(), 1u32));
        }
        let listed: Vec<(&str, u16, &[u8], u32, &[u8])> = many.iter().map(|(name, method, data, declared)| (name.as_str(), *method, *data, *declared, b"".as_slice())).collect();
        match read(&declared_zip(&listed), true) {
            Read::Refused(reason) => {
                assert_eq!(reason, MEMBER_CAP_REFUSAL, "{reason}");
                assert!(!reason.contains("decoded"), "{reason}");
            }
            other => panic!("member count past the cap was expanded: {other:?}"),
        }
        let at_cap = vec![b'x'; MEMBER_BYTE_CAP];
        match read(&declared_zip(&[("note.txt", 0, &at_cap, MEMBER_BYTE_CAP as u32, b"")]), true) {
            Read::Refused(reason) => assert_eq!(reason, NOT_CAPTURE_ZIP, "{reason}"),
            other => panic!("a member at the byte cap was not admitted: {other:?}"),
        }
    }

    fn refused_session(request: &[u8], response: &[u8]) -> String {
        let archive = session_zip(&[("001_c.txt", request), ("001_s.txt", response)]);
        match read(&archive, true) {
            Read::Refused(reason) => reason,
            Read::Rows(capture) => panic!("malformed member became rows: {}", capture.report),
            Read::NotCapture => panic!("malformed member was not a capture"),
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn malformed_session_messages_are_not_rows() {
        // Fails if a missing delimiter, an extra request token, or a status line without the required space becomes a completed row.
        let ok_response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok";
        let missing = refused_session(b"GET https://e.test/x HTTP/1.1\r\nHost: e.test", ok_response);
        assert!(missing.contains("missing request.method"), "{missing}");
        assert!(!missing.contains("not recorded"), "{missing}");
        assert!(!missing.contains("2xx"), "{missing}");
        assert!(!missing.contains("https://e.test/x"), "{missing}");
        let extra = refused_session(b"GET /path HTTP/1.1 EXTRA\r\nHost: e.test\r\n\r\n", ok_response);
        assert!(extra.contains("missing request.method"), "{extra}");
        assert!(!extra.contains("http://e.test/path"), "{extra}");
        assert!(!extra.contains("2xx"), "{extra}");
        let bare = refused_session(b"GET https://e.test/s HTTP/1.1\r\nHost: e.test\r\n\r\n", b"HTTP/1.1 200\r\nContent-Type: text/plain\r\n\r\nok");
        assert!(bare.contains("missing response.status"), "{bare}");
        assert!(!bare.contains("2xx"), "{bare}");
        assert!(!bare.contains("status 200"), "{bare}");
        let tabbed = refused_session(b"GET\t/path\tHTTP/1.1\r\nHost: e.test\r\n\r\n", ok_response);
        assert!(tabbed.contains("missing request.method"), "{tabbed}");
        assert!(!tabbed.contains("http://e.test/path"), "{tabbed}");
        let no_host = refused_session(b"GET /path HTTP/1.1\r\n\r\n", ok_response);
        assert!(no_host.contains("missing request.url"), "{no_host}");
        assert!(!no_host.contains("http://"), "{no_host}");
        assert!(!no_host.contains("/path"), "{no_host}");
        let duplicate_host = refused_session(b"GET /a HTTP/1.1\r\nHost: e.test\r\nHost: other.test\r\n\r\n", ok_response);
        assert!(duplicate_host.contains("missing request.url"), "{duplicate_host}");
        assert!(!duplicate_host.contains("http://e.test/a"), "{duplicate_host}");
        let star = refused_session(b"OPTIONS * HTTP/1.1\r\nHost: e.test\r\n\r\n", ok_response);
        assert!(star.contains("missing request.url"), "{star}");
        let origin = session_zip(&[
            ("001_c.txt", b"GET /path HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 \r\nContent-Type: text/plain\r\n\r\nok"),
        ]);
        let capture = rows(&origin);
        assert_eq!(capture.rows[0].url, "http://e.test/path");
        assert_eq!(capture.rows[0].status, 200);
        assert!(capture.report.contains("class 2xx."), "{}", capture.report);
        assert!(!capture.report.lines().any(|line| line.split_whitespace().nth(2) == Some("/path")), "{}", capture.report);
        let queried = session_zip(&[
            ("001_c.txt", b"GET /search?q=1 HTTP/1.1\r\nHost: e.test:8080\r\n\r\n"),
            ("001_s.txt", ok_response),
        ]);
        let capture = rows(&queried);
        assert_eq!(capture.rows[0].url, "http://e.test:8080/search?q=1");
        let absolute = session_zip(&[
            ("001_c.txt", b"GET https://e.test/abs HTTP/1.1\r\nHost: other.test\r\n\r\n"),
            ("001_s.txt", ok_response),
        ]);
        let capture = rows(&absolute);
        assert_eq!(capture.rows[0].url, "https://e.test/abs");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn header_names_keep_the_colon_against_the_token() {
        // Fails if a padded field name is normalized and then reported as valid.
        let padded = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/h","headers":[{"name":"Content-Type ","value":"text/plain"}]},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(padded.as_bytes());
        assert!(capture.report.contains("Refusal: entry 0 request header is unusable."), "{}", capture.report);
        assert!(!capture.report.contains("Header 0 request Content-Type."), "{}", capture.report);
        let leading = r#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/h","headers":[{"name":" Content-Type","value":"text/plain"}]},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(leading.as_bytes());
        assert!(capture.report.contains("Refusal: entry 0 request header is unusable."), "{}", capture.report);
        assert!(!capture.report.contains("Header 0 request Content-Type."), "{}", capture.report);
        let spaced = session_zip(&[
            ("001_c.txt", b"GET https://e.test/h HTTP/1.1\r\nContent-Type : text/plain\r\n\r\n"),
            ("001_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
        ]);
        let capture = rows(&spaced);
        assert!(capture.report.contains("Refusal: entry 1 request header is unusable."), "{}", capture.report);
        assert!(!capture.report.contains("Header 1 request Content-Type."), "{}", capture.report);
        assert!(capture.report.contains("Header 1 response Content-Type."), "{}", capture.report);
        let mime = refused_session(
            b"GET https://e.test/h HTTP/1.1\r\nHost: e.test\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Type : text/plain\r\n\r\nok",
        );
        assert!(mime.contains("missing response.content.mimeType"), "{mime}");
        assert!(!mime.contains("2xx"), "{mime}");
        assert!(!mime.contains("text/plain"), "{mime}");
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn post_data_params_are_an_alternative_to_text() {
        // Fails if params without text are "not recorded", or if text and params are both kept.
        let params = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/p","postData":{"mimeType":"application/x-www-form-urlencoded","params":[{"name":"a","value":"1"},{"name":"b","value":"2"}]}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(params.as_bytes());
        assert!(capture.report.contains("Body 0 request: parameters recorded. count 2."), "{}", capture.report);
        assert!(!capture.report.contains("Body 0 request: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("request body is unusable"), "{}", capture.report);
        let empty = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/p","postData":{"mimeType":"application/x-www-form-urlencoded","params":[]}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(empty.as_bytes());
        assert!(capture.report.contains("Body 0 request: parameters recorded. count 0."), "{}", capture.report);
        assert!(!capture.report.contains("Body 0 request: not recorded."), "{}", capture.report);
        let both = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/p","postData":{"mimeType":"text/plain","text":"a=1","params":[{"name":"a","value":"1"}]}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(both.as_bytes());
        assert!(capture.report.contains("Refusal: entry 0 request body is unusable."), "{}", capture.report);
        assert!(!capture.report.contains("Body 0 request: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("parameters recorded"), "{}", capture.report);
        assert!(!capture.report.contains("text representation"), "{}", capture.report);
        assert!(!capture.report.contains("sha256"), "{}", capture.report);
        let bad = r#"{"log":{"entries":[{"request":{"method":"POST","url":"https://e.test/p","postData":{"params":"a=1"}},"response":{"status":200,"content":{"mimeType":"text/plain","size":1}}}]}}"#;
        let capture = rows(bad.as_bytes());
        assert!(capture.report.contains("Refusal: entry 0 request body is unusable."), "{}", capture.report);
        assert!(!capture.report.contains("Body 0 request: not recorded."), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn caller_deadline_is_not_replaced_by_a_later_budget() {
        // Fails if a fresh 120-second budget completes a read after the caller deadline.
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                READ_DEADLINE.with(|slot| slot.set(None));
                DEADLINE_CHECKS_LEFT.with(|slot| slot.set(None));
                super::super::execution::reset();
            }
        }
        let _reset = Reset;
        DEADLINE_CHECKS_LEFT.with(|slot| slot.set(None));
        READ_DEADLINE.with(|slot| slot.set(None));
        let raw = document(&[one("GET", "https://e.test/owner", 200, "text/plain", 1)]);
        match read_until(raw.as_bytes(), true, Some(Instant::now() - Duration::from_secs(1))) {
            Read::Refused(reason) => {
                assert_eq!(reason, PARTIAL_DEADLINE);
                assert!(!reason.contains("https://e.test/owner"), "{reason}");
                assert!(!reason.contains("2xx"), "{reason}");
            }
            other => panic!("an expired caller deadline was completed: {other:?}"),
        }
        let view = file_capture_until(raw.as_bytes(), true, Instant::now() - Duration::from_millis(1));
        let text = view.string();
        assert!(text.contains("deadline"), "{text}");
        assert!(text.contains("partial"), "{text}");
        assert!(!text.contains("https://e.test/owner"), "{text}");
        READ_DEADLINE.with(|slot| slot.set(Some(Instant::now() - Duration::from_secs(2))));
        match read_until(raw.as_bytes(), true, Some(Instant::now() + READ_BUDGET)) {
            Read::Refused(reason) => assert_eq!(reason, PARTIAL_DEADLINE, "{reason}"),
            other => panic!("a later budget replaced the active deadline: {other:?}"),
        }
        READ_DEADLINE.with(|slot| slot.set(None));
        match read_until(raw.as_bytes(), true, Some(Instant::now() + Duration::from_secs(30))) {
            Read::Rows(capture) => {
                assert_eq!(capture.rows[0].status, 200);
                assert!(capture.report.contains("class 2xx."), "{}", capture.report);
            }
            other => panic!("a live caller deadline refused a short read: {other:?}"),
        }
    }

    fn paired_session(meta: &[u8]) -> Vec<u8> {
        zip(&[
            ("raw/01_c.txt", b"GET https://e.test/s HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("raw/01_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
            ("raw/01_m.xml", meta),
        ])
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn session_without_index_yields_one_paired_entry() {
        // Fails if a session archive without index.dat is refused, or if the three raw members are not one pair.
        let archive = paired_session(b"<Session></Session>");
        let capture = rows(&archive);
        assert_eq!(capture.rows.len(), 1);
        assert_eq!(capture.rows[0].index, 1);
        assert_eq!(capture.rows[0].method, "GET");
        assert_eq!(capture.rows[0].status, 200);
        assert!(capture.report.contains("Pairing session 1 request 01_c.txt response 01_s.txt."), "{}", capture.report);
        assert!(capture.report.contains("index.dat: not recorded."), "{}", capture.report);
        assert!(!capture.report.contains("index.dat present."), "{}", capture.report);
        assert!(capture.report.contains("Timing: not recorded."), "{}", capture.report);
        assert!(matches!(capture.rows[0].timing, TimingFact::NotRecorded));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn session_timers_are_recorded_from_metadata() {
        // Fails if SessionTimers are dropped and the row stays "not recorded".
        // ClientBeginRequest 2020-01-15T23:59:59.500-05:00 is 2020-01-16T04:59:59.500Z.
        // ClientDoneResponse 2020-01-16T05:00:00.750Z is 1250 ms later.
        let attributes = br#"<SessionTimers ClientConnected="2020-01-15T12:00:00.0000000-08:00" ClientBeginRequest="2020-01-15T23:59:59.5000000-05:00" ClientDoneRequest="2020-01-15T23:59:59.7000000-05:00" ServerConnected="2020-01-16T05:00:00.1000000Z" ServerBeginResponse="2020-01-16T05:00:00.4000000Z" ServerDoneResponse="2020-01-16T05:00:00.7000000Z" ClientDoneResponse="2020-01-16T05:00:00.7500000Z" DNSTime="12" TCPConnectTime="30" />"#;
        let capture = rows(&paired_session(attributes));
        assert_eq!(capture.rows.len(), 1);
        assert!(matches!(capture.rows[0].timing, TimingFact::Recorded(ref elapsed) if elapsed == "1250"), "{:?}", capture.rows[0].timing);
        assert!(capture.report.contains("Timing 1 elapsed 1250 ms recorded by the capture, not measured by RLLM."), "{}", capture.report);
        assert!(!capture.report.contains("Timing: not recorded."), "{}", capture.report);
        assert!(capture.report.contains("Pairing session 1 request 01_c.txt response 01_s.txt."), "{}", capture.report);
        let children = br#"<SessionTimers><ClientConnected>2020-01-15T12:00:00.0000000-08:00</ClientConnected><ClientBeginRequest>2020-01-15T23:59:59.5000000-05:00</ClientBeginRequest><ClientDoneRequest>2020-01-15T23:59:59.7000000-05:00</ClientDoneRequest><ServerConnected>2020-01-16T05:00:00.1000000Z</ServerConnected><ServerBeginResponse>2020-01-16T05:00:00.4000000Z</ServerBeginResponse><ServerDoneResponse>2020-01-16T05:00:00.7000000Z</ServerDoneResponse><ClientDoneResponse>2020-01-16T05:00:00.7500000Z</ClientDoneResponse></SessionTimers>"#;
        let nested = rows(&paired_session(children));
        assert!(matches!(nested.rows[0].timing, TimingFact::Recorded(ref elapsed) if elapsed == "1250"), "{:?}", nested.rows[0].timing);
        let kept = rows(&zip(&[
            ("index.dat", b"session"),
            ("raw/01_c.txt", b"GET https://e.test/s HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("raw/01_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
            ("raw/01_m.xml", attributes),
        ]));
        assert!(kept.report.contains("index.dat present."), "{}", kept.report);
        assert!(kept.report.contains("Timing 1 elapsed 1250 ms recorded by the capture, not measured by RLLM."), "{}", kept.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn https_origin_form_session_reports_https_and_pairs_its_redirect() {
        // Fails if an origin-form session with the HTTPS bit stays http://, so a later
        // https request no longer pairs, or if a CONNECT tunnel ignores the recorded scheme.
        // BitFlags 81 keeps the HTTPS bit beside other bits. BitFlags 16 names no scheme.
        // BitFlags 8193 is a CONNECT tunnel whose HTTPS bit is set. BitFlags 2 names FTP.
        let secure = br#"<Session SID="1" BitFlags="81"><SessionTimers ClientConnected="2020-01-16T05:00:00.0000000Z" ClientDoneResponse="2020-01-16T05:00:00.2500000Z"/></Session>"#;
        let plain = br#"<Session SID="3" BitFlags="16"></Session>"#;
        let tunnel = br#"<Session SID="4" BitFlags="8193"></Session>"#;
        let transfer = br#"<Session SID="5" BitFlags="2"></Session>"#;
        let archive = zip(&[
            ("raw/01_c.txt", b"GET /start HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("raw/01_s.txt", b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Type: text/html\r\n\r\n"),
            ("raw/01_m.xml", secure),
            ("raw/02_c.txt", b"GET /next HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("raw/02_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
            ("raw/02_m.xml", secure),
            ("raw/03_c.txt", b"GET /plain HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("raw/03_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
            ("raw/03_m.xml", plain),
            ("raw/04_c.txt", b"CONNECT e.test:443 HTTP/1.1\r\nHost: e.test:443\r\n\r\n"),
            ("raw/04_s.txt", b"HTTP/1.1 200 Connection Established\r\nContent-Type: text/plain\r\n\r\n"),
            ("raw/04_m.xml", tunnel),
            ("raw/05_c.txt", b"GET /file HTTP/1.1\r\nHost: files.test\r\n\r\n"),
            ("raw/05_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
            ("raw/05_m.xml", transfer),
        ]);
        let capture = rows(&archive);
        assert_eq!(capture.rows.len(), 5);
        assert_eq!(capture.rows[0].url, "https://e.test/start");
        assert_eq!(capture.rows[1].url, "https://e.test/next");
        assert_eq!(capture.rows[2].url, "http://e.test/plain");
        assert_eq!(capture.rows[3].url, "https://e.test:443");
        assert_eq!(capture.rows[3].method, "CONNECT");
        assert_eq!(capture.rows[4].url, "ftp://files.test/file");
        assert!(matches!(capture.rows[0].timing, TimingFact::Recorded(ref elapsed) if elapsed == "250"), "{:?}", capture.rows[0].timing);
        let pairing = "Redirect 1 recorded pairing entry 2 target /next.";
        assert!(capture.report.contains(pairing), "{}", capture.report);
        assert!(capture.report.contains("https://e.test/start"), "{}", capture.report);
        assert!(capture.report.contains("https://e.test/next"), "{}", capture.report);
        assert!(capture.report.contains("https://e.test:443"), "{}", capture.report);
        assert!(!capture.report.contains("http://e.test/start"), "{}", capture.report);
        assert!(!capture.report.contains("http://e.test/next"), "{}", capture.report);
        assert!(!capture.report.contains("http://e.test:443"), "{}", capture.report);
        assert!(!capture.report.contains("location not followed"), "{}", capture.report);
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn custom_session_flag_value_does_not_set_the_recorded_scheme() {
        // Fails if a custom session-flag value containing BitFlags='1' is read as
        // the session attribute and GET /plain is reported as https://e.test/plain.
        let meta = br#"<Session SID="3" note="BitFlags='1'">BitFlags='1'<Nested BitFlags="1"/><SessionFlags><SessionFlag N="custom" V="BitFlags='1'"/></SessionFlags></Session>"#;
        let archive = zip(&[
            ("raw/03_c.txt", b"GET /plain HTTP/1.1\r\nHost: e.test\r\n\r\n"),
            ("raw/03_s.txt", b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok"),
            ("raw/03_m.xml", meta),
        ]);
        let capture = rows(&archive);
        assert_eq!(capture.rows.len(), 1);
        assert_eq!(capture.rows[0].method, "GET");
        assert_eq!(capture.rows[0].url, "http://e.test/plain");
        assert!(capture.report.contains("http://e.test/plain"), "{}", capture.report);
        assert!(!capture.report.contains("https://e.test/plain"), "{}", capture.report);
    }
}

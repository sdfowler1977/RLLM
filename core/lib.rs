mod application;
mod host_fs;
#[path = "adb_client.rs"]
mod adb_client;
mod self_build;
mod rrsi;
mod archive;
mod compression;
mod layout;
mod execution;
mod workspace_command;
mod gateway;
mod a2a;
mod network;
mod provider;
mod provider_calls;
mod hosted_search;
mod attachments;
mod generation;
mod resources;
mod stream;
mod state_crypto;
mod mcp_lifecycle;
mod mcp_schema;
mod mcp_identity;
mod document;
mod har_evidence;
mod dotnet_evidence;
mod lifted_block;
mod call_sites;
mod text_catalog;
mod manifest_pool;
mod mach_image;
mod assembly_tables;
mod block_trace;
mod tone_scan;
mod model_graph;
mod datagram_fields;
mod script_layout;
mod session_rows;
mod session_env;
mod refine_record;
mod rebuild_reload;
mod dex_source;
mod ascii_runs;
mod evidence_pack;
mod observer_index;
mod mach_libraries;
mod mach_symbols;
mod candidate_pool;
mod trace_record;
mod device_record;
mod record_plain;
mod evidence_bundle;
mod tensor_edges;
mod spectrum_bins;
mod reach_edges;
mod statement_source;
mod script_text;
mod mach_text;
mod arm_branch;
mod arm_text;
mod string_near_call;
mod dex_strings;
mod dex_use;
mod zip_members;
mod saved_archive;
pub mod apk_build;
mod apk_sign;
mod zipalign;
mod smali;
#[path="js_evidence.rs"] mod js_evidence;
use std::{collections::BTreeMap, ffi::CString, os::raw::c_char};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}
use Json::*;
type Result<T> = std::result::Result<T, String>;
impl Json {
    fn get(&self, k: &str) -> Result<&Json> {
        if let Obj(o) = self {
            o.get(k).ok_or_else(|| format!("Missing {k}"))
        } else {
            Err("Expected object".into())
        }
    }
    fn text(&self) -> Result<&str> {
        if let Str(s) = self {
            Ok(s)
        } else {
            Err("Expected text".into())
        }
    }
    fn array(&self) -> Result<&Vec<Json>> {
        if let Arr(v) = self {
            Ok(v)
        } else {
            Err("Expected array".into())
        }
    }
    fn string(&self) -> String {
        match self {
            Null => "null".into(),
            Bool(b) => b.to_string(),
            Num(s) => s.clone(),
            Str(s) => quote(s),
            Arr(v) => format!(
                "[{}]",
                v.iter().map(Json::string).collect::<Vec<_>>().join(",")
            ),
            Obj(o) => format!(
                "{{{}}}",
                o.iter()
                    .map(|(k, v)| format!("{}:{}", quote(k), v.string()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}
fn obj(fields: Vec<(&str, Json)>) -> Json {
    Obj(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn s(v: &str) -> Json {
    Str(v.into())
}
fn quote(v: &str) -> String {
    let mut out = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}
impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.ws();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn hex(&mut self) -> Result<u32> {
        if self.i + 4 > self.b.len() {
            return Err("Short unicode escape".into());
        }
        let v = std::str::from_utf8(&self.b[self.i..self.i + 4]).map_err(|_| "Invalid unicode")?;
        self.i += 4;
        u32::from_str_radix(v, 16).map_err(|_| "Invalid unicode escape".into())
    }
    fn str(&mut self) -> Result<String> {
        if !self.take(b'"') {
            return Err("Expected quoted text".into());
        }
        let mut out = Vec::new();
        loop {
            let c = *self.b.get(self.i).ok_or("Unterminated text")?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| "Invalid UTF-8".into()),
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or("Missing escape")?;
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(10),
                        b'r' => out.push(13),
                        b't' => out.push(9),
                        b'u' => {
                            let mut n = self.hex()?;
                            if (0xd800..=0xdbff).contains(&n) {
                                if self.b.get(self.i..self.i + 2) != Some(b"\\u") {
                                    return Err("Missing surrogate".into());
                                }
                                self.i += 2;
                                let low = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err("Invalid surrogate".into());
                                }
                                n = 0x10000 + ((n - 0xd800) << 10) + (low - 0xdc00);
                            }
                            let c = char::from_u32(n).ok_or("Invalid unicode scalar")?;
                            let mut buf = [0; 4];
                            out.extend(c.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err("Invalid escape".into()),
                    }
                }
                0..=31 => return Err("Unescaped control character".into()),
                _ => out.push(c),
            }
        }
    }
    fn value(&mut self, depth: usize) -> Result<Json> {
        if depth > 64 {
            return Err("JSON nesting limit exceeded".into());
        }
        self.ws();
        match self.b.get(self.i).copied() {
            Some(b'"') => Ok(Str(self.str()?)),
            Some(b'{') => {
                self.i += 1;
                let mut m = BTreeMap::new();
                if self.take(b'}') {
                    return Ok(Obj(m));
                }
                loop {
                    let k = self.str()?;
                    if !self.take(b':') {
                        return Err("Missing colon".into());
                    }
                    let v = self.value(depth + 1)?;
                    if m.insert(k, v).is_some() {
                        return Err("Duplicate JSON field".into());
                    }
                    if self.take(b'}') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err("Missing comma".into());
                    }
                }
                Ok(Obj(m))
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                if self.take(b']') {
                    return Ok(Arr(v));
                }
                loop {
                    v.push(self.value(depth + 1)?);
                    if self.take(b']') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err("Missing comma".into());
                    }
                }
                Ok(Arr(v))
            }
            Some(b't') => self.literal(b"true", Bool(true)),
            Some(b'f') => self.literal(b"false", Bool(false)),
            Some(b'n') => self.literal(b"null", Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                if self.b[self.i] == b'-' {
                    self.i += 1;
                }
                match self.b.get(self.i) {
                    Some(b'0') => self.i += 1,
                    Some(b'1'..=b'9') => {
                        while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                            self.i += 1
                        }
                    }
                    _ => return Err("Invalid number".into()),
                }
                if self.b.get(self.i) == Some(&b'.') {
                    self.i += 1;
                    let n = self.i;
                    while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                        self.i += 1
                    }
                    if self.i == n {
                        return Err("Missing fraction".into());
                    }
                }
                if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
                    self.i += 1;
                    if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                        self.i += 1
                    }
                    let n = self.i;
                    while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                        self.i += 1
                    }
                    if self.i == n {
                        return Err("Missing exponent".into());
                    }
                }
                Ok(Num(std::str::from_utf8(&self.b[start..self.i])
                    .unwrap()
                    .into()))
            }
            _ => Err("Invalid JSON value".into()),
        }
    }
    fn literal(&mut self, w: &[u8], v: Json) -> Result<Json> {
        if self.b.get(self.i..self.i + w.len()) != Some(w) {
            return Err("Invalid literal".into());
        }
        self.i += w.len();
        Ok(v)
    }
}
fn parse(text: &str) -> Result<Json> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("JSON exceeds 8 MiB".into());
    }
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err("Trailing JSON data".into());
    }
    Ok(v)
}
fn field<'a>(j: &'a Json, k: &str) -> Result<&'a str> {
    j.get(k)?.text()
}
fn request(v: &Json) -> Result<Json> {
    let protocol = field(v, "protocol")?;
    let model = field(v, "model")?;
    if model.trim().is_empty() {
        return Err("Enter a model name".into());
    }
    let source = v.get("messages")?.array()?;
    if source.is_empty() || source.len() > 2000 {
        return Err("Conversation must contain 1–2000 messages".into());
    }
    let mut messages = Vec::new();
    let mut system = Vec::new();
    let mut contents = Vec::new();
    for m in source {
        let role = field(m, "role")?;
        let content = field(m, "content")?;
        if !["user", "assistant", "system"].contains(&role) {
            return Err("Unsupported message role".into());
        }
        if content.len() > 1024 * 1024 {
            return Err("Message exceeds 1 MiB".into());
        }
        if role == "system" {
            system.push(content.to_string());
        }
        if role != "system" || protocol == "chat" || protocol == "local" {
            messages.push(obj(vec![("role", s(role)), ("content", s(content))]));
        }
        if role != "system" {
            contents.push(obj(vec![
                (
                    "role",
                    s(if role == "assistant" { "model" } else { "user" }),
                ),
                ("parts", Arr(vec![obj(vec![("text", s(content))])])),
            ]));
        }
    }
    let body_result: Result<Json> = match protocol {
        "chat" | "local" => Ok(obj(vec![
            ("model", s(model)),
            ("messages", Arr(messages)),
            ("stream", Bool(false)),
        ])),
        "messages" => {
            let mut fields = vec![
                ("model", s(model)),
                ("messages", Arr(messages)),
                ("max_tokens", Num("4096".into())),
                ("stream", Bool(false)),
            ];
            if !system.is_empty() {
                fields.push(("system", s(&system.join("\n\n"))));
            }
            Ok(obj(fields))
        }
        "content" => {
            let mut fields = vec![("contents", Arr(contents))];
            if !system.is_empty() {
                fields.push((
                    "systemInstruction",
                    obj(vec![(
                        "parts",
                        Arr(vec![obj(vec![("text", s(&system.join("\n\n")))])]),
                    )]),
                ));
            }
            Ok(obj(fields))
        }
        _ => Err("Unknown API format".into()),
    };
    let mut body = body_result?;
    provider::apply_attachments(&mut body, source, protocol)?;
    hosted_search::wire(&mut body, source, protocol)?;
    let options = v.get("generation").unwrap_or(&Null);
    generation::apply(&mut body, options, protocol, source)?;
    Ok(body)
}
fn response(v: &Json) -> Result<Json> {
    let reply=provider::reply(field(v,"protocol")?,field(v,"body")?)?;
    // The original text-only headless interface keeps its empty-text contract.
    if reply.text.trim().is_empty(){return Err("Server returned no text response".into());}
    Ok(s(&reply.text))
}

/// The window of a turn: the end-to-end bound of the request it carries. The
/// upper bound is the horizon of the platform transport, so every window the
/// core states can be carried by the request it protects. A longer transport
/// would be refused by the platform and the promotion would outlive the
/// request it protects.
fn turn_window(window_ms: i64) -> i64 {
    window_ms.min(120000).max(30000)
}
/// The time a turn has left against its own window. The promotion of a turn
/// lasts exactly as long as the request it protects, so it can neither outlive
/// the request nor end before it.
fn turn_remaining(window: i64, elapsed_ms: i64) -> i64 {
    (window - elapsed_ms.max(0)).max(0)
}
fn turn_elapsed(elapsed_ms: i64) -> String {
    let total = (elapsed_ms / 1000).max(0);
    if total < 60 {
        format!("{total}s")
    } else {
        format!("{}m {}s", total / 60, total % 60)
    }
}
fn whole_ms(v: &Json, key: &str) -> Result<i64> {
    let text = match v.get(key)? {
        Num(text) | Str(text) => text.as_str(),
        _ => return Err(format!("Invalid {key}")),
    };
    text.parse::<i64>().map_err(|_| format!("Invalid {key}"))
}
// --- The turn owner -----------------------------------------------------------------
// One typed operation holds the whole policy of a model turn: admission, the
// lease, the cadence, the expiry notice, the reply text, the title and channel
// defaults and the durable request identity. The Android side keeps only the
// platform calls, so no turn policy is duplicated in Java.
//
// The durable identity of a turn is the pair of its request ticket and the
// holder token this core issued for it. Only the holder may extend the lease
// or retire the identity, so a turn that is already over cannot strip the
// lease, the notification or the identity of a newer live turn.
//
// A turn has one window: the end-to-end bound of the request it carries. The
// promotion of the turn lasts the time the request has left, so the promotion
// can neither outlive the request nor end before it. The window is the only
// bound the product states, and it is stated here.
//
// Two clocks meet in a turn and they are not the same clock. The request runs
// on the elapsed time since the turn started; the promotion runs on the
// elapsed time since its own lease was decided. A renewal of the lease never
// rewinds the request's clock, so the progress a user reads is the progress
// the turn really made.
//
// The actions are begin, extend, tick, expire, finish, publish and rebind.
// begin is the only writer of the identity. extend decides the lease, tick
// decides the progress and the expiry, expire produces the notice that the
// refused start also produces, finish retires the identity, publish composes
// and holds the reply, and rebind restores the model identity of a turn that
// this process still owns.

/// How often a running turn reports its progress.
const TURN_CADENCE_MS: i64 = 1000;
/// The longest reply text a notification may carry.
const TURN_REPLY_LIMIT: usize = 600;
/// The default label of a running turn.
const TURN_LABEL: &str = "Replying";
/// The title of the progress notification.
const TURN_PROGRESS_TITLE: &str = "RLLM is working";
/// The title of a published reply.
const TURN_REPLY_TITLE: &str = "RLLM reply";
/// The title of a turn that failed.
const TURN_FAILED_TITLE: &str = "RLLM turn failed";
/// The notification channel of a running turn.
const TURN_PROGRESS_CHANNEL: &str = "turn";
/// The notification channel of a published reply.
const TURN_REPLY_CHANNEL: &str = "replies";
/// The display name of the running turn's channel.
const TURN_PROGRESS_CHANNEL_NAME: &str = "Working";
/// The display name of the reply channel.
const TURN_REPLY_CHANNEL_NAME: &str = "Replies";
/// The notice a turn shows when its background protection is lost. The refused
/// start path and the lease expiry path both reach the user through this text.
const TURN_PROTECTION_LOST: &str = "Background protection was lost. The turn still runs.";
/// The notice a turn shows when its outcome cannot be established. Nothing is
/// replayed, because the remote completion of the request is uncertain.
const TURN_UNCERTAIN: &str = "The previous turn did not finish. Nothing was replayed.";
/// The holder tokens of the admitted turns. The counter is process local; the
/// request ticket keeps the pair of ticket and holder unique across a restart.
static TURN_HOLDERS: AtomicU64 = AtomicU64::new(0);

/// A state field. An absent field has no value.
fn state_get<'a>(st: &'a Json, k: &str) -> Option<&'a Json> {
    match st {
        Obj(o) => o.get(k),
        _ => None,
    }
}
/// A state field as text. An absent or null field reads as empty.
fn state_text(st: &Json, k: &str) -> String {
    match state_get(st, k) {
        Some(value) => value.text().unwrap_or("").into(),
        None => String::new(),
    }
}
/// A state field as a flag.
fn state_bool(st: &Json, k: &str) -> bool {
    match state_get(st, k) {
        Some(Bool(true)) => true,
        Some(Str(text)) => text.trim() == "true",
        _ => false,
    }
}
fn state_put(st: &mut Json, k: &str, v: Json) {
    if let Obj(o) = st {
        o.insert(k.into(), v);
    }
}
/// A field of a nested state object.
fn nested_text(st: &Json, outer: &str, inner: &str) -> String {
    match state_get(st, outer) {
        Some(value) => state_text(value, inner),
        None => String::new(),
    }
}
/// An optional request field as text.
fn opt_text<'a>(v: &'a Json, k: &str) -> Option<&'a str> {
    match v.get(k) {
        Ok(value) => value.text().ok(),
        Err(_) => None,
    }
}

/// The durable identity of an in-flight model turn. It lives in the encrypted
/// application state, so it survives a process kill and is the only handle a
/// recreated Activity may re-bind to. Nothing else writes it.
fn turn_identity(st: &Json) -> String {
    nested_text(st, "turn_expected", "ticket")
}
/// The holder token of the durable identity. Only this holder may extend the
/// lease or retire the identity.
fn turn_holder_identity(st: &Json) -> String {
    nested_text(st, "turn_expected", "holder")
}
/// The holder token of a new admission. It is issued here, so the caller never
/// invents one and a stale turn cannot claim the lease of a newer turn.
fn turn_holder() -> String {
    format!("h{}", TURN_HOLDERS.fetch_add(1, Ordering::Relaxed))
}
/// Write the durable turn identity.
fn turn_record(st: &mut Json, ticket: &str, holder: &str, started_ms: i64, window_ms: i64, lease_ms: i64) {
    state_put(
        st,
        "turn_expected",
        obj(vec![
            ("ticket", s(ticket)),
            ("holder", s(holder)),
            ("started_ms", Num(started_ms.to_string())),
            ("window_ms", Num(window_ms.to_string())),
            ("lease_ms", Num(lease_ms.to_string())),
            ("phase", s("running")),
        ]),
    );
}
/// Retire the durable turn identity. Only a decided end of the turn does this:
/// an uncertain remote completion is never adopted and never replayed.
fn turn_retire(st: &mut Json) {
    state_put(st, "turn_expected", Null);
}
/// The label of a running turn.
fn turn_label(v: &Json) -> String {
    let label = opt_text(v, "label").unwrap_or("");
    if label.trim().is_empty() {
        TURN_LABEL.into()
    } else {
        label.trim().into()
    }
}
/// The text a notification may carry. The limit is owned here, so a fresh reply
/// and a drained one are composed by the same rule.
fn turn_text(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= TURN_REPLY_LIMIT {
        return trimmed.into();
    }
    let mut out: String = trimmed.chars().take(TURN_REPLY_LIMIT).collect();
    out.push('…');
    out
}
/// The application state a turn operation works on. A missing state is an
/// empty one, so a turn can be decided before the first state exists.
fn turn_state(v: &Json) -> Json {
    match v.get("state") {
        Ok(value) if matches!(value, Obj(_)) => value.clone(),
        _ => obj(vec![]),
    }
}
fn turn_begin(v: &Json) -> Result<Json> {
    let ticket = field(v, "ticket")?.trim().to_string();
    let window = turn_window(whole_ms(v, "window_ms")?);
    // The elapsed time of the request is measured by the platform. The lease is
    // the time the request has left, so the promotion cannot outlive it.
    let elapsed = whole_ms(v, "elapsed_ms").unwrap_or(0);
    let lease = turn_remaining(window, elapsed);
    let started = whole_ms(v, "started_ms").unwrap_or(0);
    let label = turn_label(v);
    let live = opt_text(v, "live_ticket").unwrap_or("").trim().to_string();
    let mut st = turn_state(v);
    if ticket.is_empty() {
        // A turn without a request identity is refused and no identity is kept.
        turn_retire(&mut st);
        return Ok(obj(vec![
            ("decision", s("refused")),
            ("holder", s("")),
            ("lease_ms", Num(lease.to_string())),
            ("window_ms", Num(window.to_string())),
            ("cadence_ms", Num(TURN_CADENCE_MS.to_string())),
            ("title", s(TURN_PROGRESS_TITLE)),
            ("channel", s(TURN_PROGRESS_CHANNEL)),
            ("channel_name", s(TURN_PROGRESS_CHANNEL_NAME)),
            ("label", s(&label)),
            ("state", st),
        ]));
    }
    // A turn that begins while another one is live takes the lease scope over
    // from it. The holder token of the new turn is the only one that may
    // extend that lease or retire the identity afterwards.
    let decision = if live.is_empty() { "queued" } else { "held" };
    let holder = turn_holder();
    turn_record(&mut st, &ticket, &holder, started, window, lease);
    Ok(obj(vec![
        ("decision", s(decision)),
        ("holder", s(&holder)),
        ("lease_ms", Num(lease.to_string())),
        ("window_ms", Num(window.to_string())),
        ("cadence_ms", Num(TURN_CADENCE_MS.to_string())),
        ("title", s(TURN_PROGRESS_TITLE)),
        ("channel", s(TURN_PROGRESS_CHANNEL)),
        ("channel_name", s(TURN_PROGRESS_CHANNEL_NAME)),
        ("label", s(&label)),
        ("state", st),
    ]))
}
fn turn_extend(v: &Json) -> Result<Json> {
    let ticket = opt_text(v, "ticket").unwrap_or("").trim().to_string();
    let holder = opt_text(v, "holder").unwrap_or("").trim().to_string();
    let window = turn_window(whole_ms(v, "window_ms")?);
    // The lease of a turn that is already running is the time its request has
    // left. The elapsed time is measured by the platform, the lease is decided
    // here.
    let elapsed = whole_ms(v, "elapsed_ms").unwrap_or(0);
    let lease = turn_remaining(window, elapsed);
    let label = turn_label(v);
    let st = turn_state(v);
    // The lease belongs to the holder that was admitted for it. A turn that is
    // no longer the holder is refused the lease, so a stale teardown cannot
    // keep the wake lock of a newer live turn. This action decides the lease
    // and writes nothing: begin is the only writer of the identity.
    let extended = !ticket.is_empty()
        && !holder.is_empty()
        && turn_identity(&st) == ticket
        && turn_holder_identity(&st) == holder;
    Ok(obj(vec![
        ("extended", Bool(extended)),
        ("lease_ms", Num(lease.to_string())),
        ("window_ms", Num(window.to_string())),
        ("cadence_ms", Num(TURN_CADENCE_MS.to_string())),
        ("title", s(TURN_PROGRESS_TITLE)),
        ("channel", s(TURN_PROGRESS_CHANNEL)),
        ("channel_name", s(TURN_PROGRESS_CHANNEL_NAME)),
        ("label", s(&label)),
        ("state", st),
    ]))
}
fn turn_tick(v: &Json) -> Result<Json> {
    // The request's own clock: the elapsed time since the turn started. It is
    // what the progress a user reads is measured from.
    let elapsed = whole_ms(v, "elapsed_ms")?;
    // The promotion's own clock: the elapsed time since its lease was decided.
    // The two clocks are separate, so a decision about the promotion never
    // rewinds the request's progress.
    let lease_elapsed = whole_ms(v, "lease_elapsed_ms").unwrap_or(elapsed);
    let lease = whole_ms(v, "lease_ms")?;
    let label = turn_label(v);
    Ok(obj(vec![
        ("expired", Bool(lease_elapsed >= lease)),
        ("progress_text", s(&format!("{label} · {}", turn_elapsed(elapsed)))),
        ("elapsed_text", s(&turn_elapsed(elapsed))),
        ("cadence_ms", Num(TURN_CADENCE_MS.to_string())),
        ("title", s(TURN_PROGRESS_TITLE)),
        ("channel", s(TURN_PROGRESS_CHANNEL)),
        ("channel_name", s(TURN_PROGRESS_CHANNEL_NAME)),
        ("label", s(&label)),
    ]))
}
fn turn_expire(_v: &Json) -> Result<Json> {
    Ok(obj(vec![
        ("notice", s(TURN_PROTECTION_LOST)),
        ("title", s(TURN_PROGRESS_TITLE)),
        ("channel", s(TURN_PROGRESS_CHANNEL)),
        ("channel_name", s(TURN_PROGRESS_CHANNEL_NAME)),
        ("cadence_ms", Num(TURN_CADENCE_MS.to_string())),
    ]))
}
fn turn_finish(v: &Json) -> Result<Json> {
    let mut st = turn_state(v);
    let ticket = opt_text(v, "ticket").unwrap_or("").trim().to_string();
    let holder = opt_text(v, "holder").unwrap_or("").trim().to_string();
    let held = turn_identity(&st);
    let held_holder = turn_holder_identity(&st);
    // The identity belongs to the holder that was admitted for it. A turn that
    // is not the holder is a stale teardown and leaves the identity of the
    // newer live turn alone.
    let retired = !held.is_empty()
        && !ticket.is_empty()
        && held == ticket
        && held_holder == holder;
    if retired {
        turn_retire(&mut st);
    }
    Ok(obj(vec![("retired", Bool(retired)), ("state", st)]))
}
fn turn_publish(v: &Json) -> Result<Json> {
    let mut st = turn_state(v);
    let drain = state_bool(v, "drain");
    let allowed = state_bool(v, "notifications_allowed");
    let backgrounded = state_bool(v, "backgrounded");
    let failed = state_bool(v, "failed");
    let chat = opt_text(v, "chat").unwrap_or("").trim().to_string();
    let title = match opt_text(v, "title") {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => {
            if failed {
                TURN_FAILED_TITLE.into()
            } else {
                TURN_REPLY_TITLE.into()
            }
        }
    };
    let reply = |st: Json, decision: &str, title: &str, text: &str, chat: &str| {
        obj(vec![
            ("decision", s(decision)),
            ("title", s(title)),
            ("text", s(text)),
            ("chat", s(chat)),
            ("channel", s(TURN_REPLY_CHANNEL)),
            ("channel_name", s(TURN_REPLY_CHANNEL_NAME)),
            ("state", st),
        ])
    };
    if drain {
        let pending = state_get(&st, "turn_pending").cloned().unwrap_or(Null);
        let body = state_text(&pending, "text");
        if body.is_empty() {
            return Ok(reply(st, "suppress", &title, "", &chat));
        }
        let pending_chat = state_text(&pending, "chat");
        let pending_title = state_text(&pending, "title");
        let held_chat = if pending_chat.is_empty() { chat } else { pending_chat };
        let held_title = if pending_title.is_empty() {
            title
        } else {
            pending_title
        };
        // The announcement is delivered when the platform shows it, and surfaced
        // in the app when it cannot be shown. A held reply is cleared only when
        // it was really delivered, so a later grant still announces it.
        if allowed {
            state_put(&mut st, "turn_pending", Null);
            return Ok(reply(st, "publish", &held_title, &body, &held_chat));
        }
        return Ok(reply(st, "surface", &held_title, &body, &held_chat));
    }
    let body = opt_text(v, "body").unwrap_or("");
    let text = turn_text(body);
    if text.is_empty() {
        return Ok(reply(st, "suppress", &title, "", &chat));
    }
    if !backgrounded {
        return Ok(reply(st, "suppress", &title, &text, &chat));
    }
    let ticket = opt_text(v, "ticket").unwrap_or("").trim().to_string();
    state_put(
        &mut st,
        "turn_pending",
        obj(vec![
            ("ticket", s(&ticket)),
            ("chat", s(&chat)),
            ("title", s(&title)),
            ("text", s(&text)),
        ]),
    );
    if allowed {
        state_put(&mut st, "turn_pending", Null);
        return Ok(reply(st, "publish", &title, &text, &chat));
    }
    Ok(reply(st, "hold", &title, &text, &chat))
}
fn turn_rebind(v: &Json) -> Result<Json> {
    let mut st = turn_state(v);
    let live = opt_text(v, "live_ticket").unwrap_or("").trim().to_string();
    let live_holder = opt_text(v, "live_holder").unwrap_or("").trim().to_string();
    let held = turn_identity(&st);
    let held_holder = turn_holder_identity(&st);
    if held.is_empty() {
        return Ok(obj(vec![("decision", s("none")), ("state", st)]));
    }
    if !live.is_empty() && held == live && held_holder == live_holder {
        // The request identity matches the turn this process still owns, so the
        // model identity is restored and the turn continues under its own name.
        state_put(&mut st, "model_expected", s(&live));
        return Ok(obj(vec![
            ("decision", s("rebound")),
            ("ticket", s(&live)),
            ("holder", s(&live_holder)),
            ("state", st),
        ]));
    }
    // The turn is not owned by this process any more. Its remote completion is
    // uncertain, so nothing is adopted and nothing is replayed.
    turn_retire(&mut st);
    state_put(&mut st, "notice", s(TURN_UNCERTAIN));
    Ok(obj(vec![("decision", s("uncertain")), ("state", st)]))
}
fn turn_operation(v: &Json) -> Result<Json> {
    match field(v, "action")? {
        "begin" => turn_begin(v),
        "extend" => turn_extend(v),
        "tick" => turn_tick(v),
        "expire" => turn_expire(v),
        "finish" => turn_finish(v),
        "publish" => turn_publish(v),
        "rebind" => turn_rebind(v),
        _ => Err("Unknown turn action".into()),
    }
}
fn operation(input: &str) -> Result<Json> {
    let v = parse(input)?;
    match field(&v, "op")? {
        "stream_open" | "stream_feed" | "stream_finish" | "stream_close" | "stream_control_ack" | "stream_control_probe" | "stream_abandon" | "stream_suspend" | "stream_resume" | "stream_resume_close" | "stream_resume_probe" | "stream_resume_summary" | "stream_prepare" | "stream_open_cleanup" | "stream_owner_cancel" => stream::operation(&v),
        "mcp_cleanup_response" => mcp_lifecycle::response(&v),
        "mcp_cleanup_plan" => mcp_lifecycle::plan(v.get("context")?,field(v.get("context")?,"session")?,false),
        "phone_revision_check" => document::operation(&v),
        "layout" => layout::plan(&v),
        "state_seal" | "state_open" => state_crypto::operation(&v),
        "script" => execution::run(&v),
        "project_archive" => archive::build(&v),
        "artifact_snapshot" => application::artifact_snapshot(&v),
        "project_chunks" => application::project_chunks(&v),
        "project_payload" => application::project_payload(&v),
        "project_record" => application::project_record(&v),
        "project_readback" => application::project_readback(&v),
        "project_close" => application::project_close(&v),
        "archive_preview" => archive::preview(&v),
        "archive_import" => archive::import(&v),
        "public_web_address" => network::public_web_address(&v),
        "network_frame_open"|"network_frame_feed"|"network_frame_finish"|"network_frame_close"=>network::frame(&v),
        "network_binary" => {
            let raw = network::unbase64(v.get("data")?.text()?)?;
            let (_, body) = network::decoded_bytes(&raw)?;
            if body.len() > 5 * 1024 * 1024 {
                return Err("Project ZIP exceeds 5 MiB".into());
            }
            Ok(obj(vec![("archive", s(&network::b64(&body)))]))
        }
        "gateway_preflight" => gateway::preflight(&v),
        "gateway_size" => gateway::size(&v),
        "gateway_incoming" => gateway::incoming(&v),
        "a2a_close" => { a2a::close(v.get("config")?)?; Ok(Null) },
        "local_tool_start" => application::local_tool_start(&v),
        "local_tool_cancel" => application::local_tool_cancel(&v),
        "harness_policy_commit" => application::harness_policy_commit(&v),
        "gateway_outgoing" => gateway::outgoing(&v),
        "gateway_failure" => gateway::failure(&v),
        "cancel_tool" => {
            application::cancel();
            Ok(Null)
        }
        "event" => application::event(&v),
        "tool" => application::tool(&v),
        "media_stream_feed"|"media_stream_finish"|"media_stream_cancel"=>application::media_stream_operation(&v),
        "media_input_open"|"media_input_feed"|"media_input_finish"|"media_input_cancel"=>application::media_input_operation(&v),
        "voice_audio_feed"|"voice_audio_cancel"|"voice_audio_check"=>application::voice_audio_operation(&v),
        "media_upload_read"|"media_upload_cancel"=>application::media_upload_operation(&v),
        "media_verify"=>application::media_verification(&v),
        "scene_open"|"scene_frame"|"scene_close"=>application::scene_operation(&v),
        "capture_document" => application::capture_document(&v),
        "capture_document_cancel" => application::capture_document_cancel(&v),
        "request" => request(&v),
        "response" => response(&v),
        "response_details" => Ok(provider::reply(field(&v,"protocol")?,field(&v,"body")?)?.details()),
        "validate" => {
            let value = v.get("value")?;
            Ok(value.clone())
        }
        "self_fingerprint" => self_build::fingerprint(&v),
        "self_plan" => self_build::plan(&v),
        "self_journal" => self_build::journal(&v),
        "self_handoff" => self_build::handoff(&v),
        "self_commit" => self_build::commit(&v),
        "self_resume" => self_build::resume(&v),
        "turn" => turn_operation(&v),
        _ => Err("Unknown operation".into()),
    }
}
fn envelope(input: &str) -> String {
    match operation(input) {
        Ok(value) => obj(vec![("value", value)]).string(),
        Err(e) => obj(vec![("error", s(&e))]).string(),
    }
}
#[no_mangle]
pub unsafe extern "C" fn client_process(data: *const u8, len: usize) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        if data.is_null() || len > 8 * 1024 * 1024 {
            return "{\"error\":\"Invalid input size\"}".to_string();
        }
        match std::str::from_utf8(std::slice::from_raw_parts(data, len)) {
            Ok(v) => envelope(v),
            Err(_) => "{\"error\":\"Invalid UTF-8\"}".into(),
        }
    })
    .unwrap_or_else(|_| "{\"error\":\"Core operation failed\"}".into());
    CString::new(result).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn client_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn json_roundtrip() {
        for v in [
            "quote \" newline\n tab\t backslash\\",
            "中文 café \u{1D11E}",
            "\0\u{001f}",
        ] {
            assert_eq!(parse(&quote(v)).unwrap(), s(v));
        }
        assert_eq!(parse("\"\\ud834\\udd1e\"").unwrap(), s("\u{1D11E}"));
    }
    #[test]
    fn bad_json() {
        for v in [
            "{\"x\":1,\"x\":2}",
            "[1,]",
            "01",
            "1e",
            "\"\\ud800\"",
            "true x",
            "\"\n\"",
        ] {
            assert!(parse(v).is_err(), "{v}");
        }
    }
    #[test]
    fn formats() {
        for protocol in ["chat", "local", "messages", "content"] {
            let j = obj(vec![
                ("protocol", s(protocol)),
                ("model", s("custom/model")),
                (
                    "messages",
                    Arr(vec![
                        obj(vec![("role", s("system")), ("content", s("Instructions"))]),
                        obj(vec![("role", s("user")), ("content", s("one"))]),
                        obj(vec![("role", s("assistant")), ("content", s("two"))]),
                        obj(vec![("role", s("user")), ("content", s("three"))]),
                    ]),
                ),
            ]);
            let r = request(&j).unwrap();
            assert!(r.string().contains("three"));
            assert!(r.string().contains("Instructions"));
        }
    }
    #[test]
    fn outputs() {
        for (protocol, body) in [
            (
                "chat",
                r#"{"choices":[{"message":{"content":"a\"b\n中文"}}]}"#,
            ),
            ("local", r#"{"message":{"content":"a\"b\n中文"}}"#),
            (
                "messages",
                r#"{"content":[{"type":"text","text":"a\"b\n中文"}]}"#,
            ),
            (
                "content",
                r#"{"candidates":[{"content":{"parts":[{"text":"a\"b\n中文"}]}}]}"#,
            ),
        ] {
            assert_eq!(
                response(&obj(vec![("protocol", s(protocol)), ("body", s(body))])).unwrap(),
                s("a\"b\n中文")
            );
        }
    }
    #[test]
    fn scoped_failure() {
        assert!(envelope(r#"{"op":"request"}"#).contains("error"));
        assert_eq!(
            envelope(r#"{"op":"validate","value":{"ready":true}}"#),
            r#"{"value":{"ready":true}}"#
        );
    }
    #[test]
    fn the_turn_window_is_clamped_and_the_elapsed_time_is_formatted() {
        // The window is reachable only through the turn operation, so the
        // clamp and the elapsed text cannot be reached by a second path.
        let low = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":1}"#).unwrap();
        assert_eq!(low.get("window_ms").unwrap(), &Num("30000".into()));
        assert_eq!(low.get("lease_ms").unwrap(), &Num("30000".into()));
        let high = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":9000000}"#).unwrap();
        assert_eq!(high.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(high.get("lease_ms").unwrap(), &Num("120000".into()));
        let mid = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":120000}"#).unwrap();
        assert_eq!(mid.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(mid.get("lease_ms").unwrap(), &Num("120000".into()));
        let edge = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":30000}"#).unwrap();
        assert_eq!(edge.get("window_ms").unwrap(), &Num("30000".into()));
        let cap = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":600000}"#).unwrap();
        assert_eq!(cap.get("window_ms").unwrap(), &Num("120000".into()));
        let over = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":600001}"#).unwrap();
        assert_eq!(over.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":0,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "0s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":60000,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "1m 0s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":125000,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "2m 5s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":-1500,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "0s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":"no"}"#).unwrap_err(),
            "Invalid window_ms"
        );
    }
    #[test]
    fn depth_limit() {
        assert!(parse(&("[".repeat(66) + "0" + &"]".repeat(66))).is_err());
    }
    fn turn_request(fields: Vec<(&str, Json)>) -> Json {
        let mut all = vec![("op", s("turn"))];
        all.extend(fields);
        obj(all)
    }
    #[test]
    fn a_turn_keeps_a_durable_identity_and_the_core_lease() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-1")),
                ("window_ms", Num("120000".into())),
                ("started_ms", Num("7".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(begun.get("decision").unwrap().text().unwrap(), "queued");
        assert_eq!(begun.get("lease_ms").unwrap(), &Num("120000".into()));
        assert_eq!(begun.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(begun.get("cadence_ms").unwrap(), &Num("1000".into()));
        assert_eq!(begun.get("label").unwrap().text().unwrap(), "Replying");
        assert_eq!(begun.get("title").unwrap().text().unwrap(), "RLLM is working");
        assert_eq!(begun.get("channel").unwrap().text().unwrap(), "turn");
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        assert!(!holder.is_empty());
        assert_eq!(
            begun
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("ticket")
                .unwrap()
                .text()
                .unwrap(),
            "ticket-1"
        );
        assert_eq!(
            begun
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("holder")
                .unwrap()
                .text()
                .unwrap(),
            holder
        );
        let clamped = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-2")),
                ("window_ms", Num("9000000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(clamped.get("lease_ms").unwrap(), &Num("120000".into()));
        assert_eq!(clamped.get("window_ms").unwrap(), &Num("120000".into()));
    }
    #[test]
    fn the_promotion_of_a_turn_is_the_time_its_request_has_left() {
        // A request that has already spent part of its window gets a promotion
        // that ends with it, never one that outlives it.
        let late = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-late")),
                ("window_ms", Num("120000".into())),
                ("elapsed_ms", Num("30000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(late.get("lease_ms").unwrap(), &Num("90000".into()));
        assert_eq!(late.get("window_ms").unwrap(), &Num("120000".into()));
        // A request whose window is spent gets no promotion at all.
        let spent = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-spent")),
                ("window_ms", Num("120000".into())),
                ("elapsed_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(spent.get("lease_ms").unwrap(), &Num("0".into()));
        // The extension of a running turn is decided the same way.
        let admitted = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-run")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = admitted.get("holder").unwrap().text().unwrap().to_string();
        let state = admitted.get("state").unwrap().clone();
        let extended = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state),
                ("ticket", s("ticket-run")),
                ("holder", s(&holder)),
                ("window_ms", Num("120000".into())),
                ("elapsed_ms", Num("45000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(extended.get("extended").unwrap(), &Bool(true));
        assert_eq!(extended.get("lease_ms").unwrap(), &Num("75000".into()));
        assert_eq!(extended.get("window_ms").unwrap(), &Num("120000".into()));
    }
    #[test]
    fn a_turn_reads_two_clocks_and_never_rewinds_its_own_progress() {
        // The request has run for a minute. The promotion was decided a quarter
        // of a minute ago and its lease is thirty seconds.
        let tick = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("60000".into())),
                ("lease_elapsed_ms", Num("15000".into())),
                ("lease_ms", Num("30000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(tick.get("expired").unwrap(), &Bool(false));
        assert_eq!(
            tick.get("progress_text").unwrap().text().unwrap(),
            "Replying · 1m 0s"
        );
        assert_eq!(tick.get("elapsed_text").unwrap().text().unwrap(), "1m 0s");
        // The promotion's own clock expires while the request is still running,
        // and the request's progress is still the request's own.
        let expired = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("60000".into())),
                ("lease_elapsed_ms", Num("30000".into())),
                ("lease_ms", Num("30000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(expired.get("expired").unwrap(), &Bool(true));
        assert_eq!(
            expired.get("progress_text").unwrap().text().unwrap(),
            "Replying · 1m 0s"
        );
        assert_eq!(expired.get("elapsed_text").unwrap().text().unwrap(), "1m 0s");
    }
    #[test]
    fn a_turn_without_an_identity_is_refused_and_a_live_turn_is_held() {
        let refused = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(refused.get("decision").unwrap().text().unwrap(), "refused");
        assert_eq!(refused.get("state").unwrap().get("turn_expected").unwrap(), &Null);
        let held = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-3")),
                ("live_ticket", s("ticket-1")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(held.get("decision").unwrap().text().unwrap(), "held");
    }
    #[test]
    fn a_turn_reports_its_progress_and_its_expiry_from_the_core() {
        let tick = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("59000".into())),
                ("lease_ms", Num("60000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(tick.get("expired").unwrap(), &Bool(false));
        assert_eq!(tick.get("progress_text").unwrap().text().unwrap(), "Replying · 59s");
        assert_eq!(tick.get("elapsed_text").unwrap().text().unwrap(), "59s");
        let late = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("60000".into())),
                ("lease_ms", Num("60000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(late.get("expired").unwrap(), &Bool(true));
    }
    #[test]
    fn the_expiry_notice_is_the_refused_start_notice() {
        let expire = operation(&turn_request(vec![("action", s("expire"))]).string()).unwrap();
        assert_eq!(
            expire.get("notice").unwrap().text().unwrap(),
            "Background protection was lost. The turn still runs."
        );
    }
    #[test]
    fn the_reply_text_limit_and_the_titles_are_owned_by_the_core() {
        let body = "x".repeat(900);
        let published = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s(&body)),
                ("chat", s("chat-1")),
                ("backgrounded", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(published.get("decision").unwrap().text().unwrap(), "publish");
        assert_eq!(published.get("title").unwrap().text().unwrap(), "RLLM reply");
        assert_eq!(published.get("channel").unwrap().text().unwrap(), "replies");
        let text = published.get("text").unwrap().text().unwrap();
        assert_eq!(text.chars().count(), 601);
        assert!(text.ends_with('…'));
        let failed = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s("boom")),
                ("failed", Bool(true)),
                ("backgrounded", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(failed.get("title").unwrap().text().unwrap(), "RLLM turn failed");
        let foreground = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s("boom")),
                ("backgrounded", Bool(false)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(foreground.get("decision").unwrap().text().unwrap(), "suppress");
    }
    #[test]
    fn a_held_reply_is_announced_after_a_resume() {
        let held = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s("the answer")),
                ("chat", s("chat-9")),
                ("backgrounded", Bool(true)),
                ("notifications_allowed", Bool(false)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(held.get("decision").unwrap().text().unwrap(), "hold");
        let state = held.get("state").unwrap().clone();
        assert_eq!(
            state.get("turn_pending").unwrap().get("text").unwrap().text().unwrap(),
            "the answer"
        );
        let drained = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", state),
                ("drain", Bool(true)),
                ("notifications_allowed", Bool(false)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(drained.get("decision").unwrap().text().unwrap(), "surface");
        assert_eq!(drained.get("text").unwrap().text().unwrap(), "the answer");
        assert_eq!(drained.get("chat").unwrap().text().unwrap(), "chat-9");
        // A reply that could not be shown is still held, so a later grant
        // announces it instead of dropping it.
        assert_eq!(
            drained
                .get("state")
                .unwrap()
                .get("turn_pending")
                .unwrap()
                .get("text")
                .unwrap()
                .text()
                .unwrap(),
            "the answer"
        );
        let granted = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", drained.get("state").unwrap().clone()),
                ("drain", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(granted.get("decision").unwrap().text().unwrap(), "publish");
        assert_eq!(granted.get("text").unwrap().text().unwrap(), "the answer");
        assert_eq!(granted.get("state").unwrap().get("turn_pending").unwrap(), &Null);
        let again = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", granted.get("state").unwrap().clone()),
                ("drain", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(again.get("decision").unwrap().text().unwrap(), "suppress");
    }
    #[test]
    fn a_recreated_activity_rebinds_to_the_live_turn_by_its_identity() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-7")),
                ("window_ms", Num("120000".into())),
                ("started_ms", Num("3".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        let rebound = operation(
            &turn_request(vec![
                ("action", s("rebind")),
                ("state", begun.get("state").unwrap().clone()),
                ("live_ticket", s("ticket-7")),
                ("live_holder", s(&holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(rebound.get("decision").unwrap().text().unwrap(), "rebound");
        assert_eq!(rebound.get("ticket").unwrap().text().unwrap(), "ticket-7");
        assert_eq!(
            rebound.get("state").unwrap().get("model_expected").unwrap().text().unwrap(),
            "ticket-7"
        );
        // A live turn that this process does not hold the lease of is not
        // adopted, even when the request identity is the same.
        let foreign = operation(
            &turn_request(vec![
                ("action", s("rebind")),
                ("state", begun.get("state").unwrap().clone()),
                ("live_ticket", s("ticket-7")),
                ("live_holder", s("h-foreign")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(foreign.get("decision").unwrap().text().unwrap(), "uncertain");
    }
    #[test]
    fn an_uncertain_turn_is_never_replayed() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-8")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let uncertain = operation(
            &turn_request(vec![
                ("action", s("rebind")),
                ("state", begun.get("state").unwrap().clone()),
                ("live_ticket", s("")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(uncertain.get("decision").unwrap().text().unwrap(), "uncertain");
        assert_eq!(
            uncertain.get("state").unwrap().get("notice").unwrap().text().unwrap(),
            "The previous turn did not finish. Nothing was replayed."
        );
        assert_eq!(uncertain.get("state").unwrap().get("turn_expected").unwrap(), &Null);
    }
    #[test]
    fn only_the_turn_that_owns_the_identity_retires_it() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-9")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        let state = begun.get("state").unwrap().clone();
        let stale = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state.clone()),
                ("ticket", s("ticket-8")),
                ("holder", s("h-other")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(stale.get("retired").unwrap(), &Bool(false));
        assert_eq!(
            stale
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("ticket")
                .unwrap()
                .text()
                .unwrap(),
            "ticket-9"
        );
        let own = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state),
                ("ticket", s("ticket-9")),
                ("holder", s(&holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(own.get("retired").unwrap(), &Bool(true));
        assert_eq!(own.get("state").unwrap().get("turn_expected").unwrap(), &Null);
    }
    #[test]
    fn only_the_holder_may_extend_the_lease() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-10")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        let state = begun.get("state").unwrap().clone();
        let own = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state.clone()),
                ("ticket", s("ticket-10")),
                ("holder", s(&holder)),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(own.get("extended").unwrap(), &Bool(true));
        assert_eq!(own.get("lease_ms").unwrap(), &Num("120000".into()));
        // The lease decision writes nothing: begin is the only writer of the
        // identity, so an extension can never take it over.
        assert_eq!(
            own.get("state").unwrap().get("turn_expected").unwrap(),
            state.get("turn_expected").unwrap()
        );
        let foreign = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state.clone()),
                ("ticket", s("ticket-10")),
                ("holder", s("h-other")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(foreign.get("extended").unwrap(), &Bool(false));
        assert_eq!(
            foreign.get("state").unwrap().get("turn_expected").unwrap(),
            state.get("turn_expected").unwrap()
        );
    }
    #[test]
    fn a_stale_teardown_leaves_the_newer_live_turn_alone() {
        let first = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-11")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let first_holder = first.get("holder").unwrap().text().unwrap().to_string();
        let second = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", first.get("state").unwrap().clone()),
                ("ticket", s("ticket-12")),
                ("live_ticket", s("ticket-11")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(second.get("decision").unwrap().text().unwrap(), "held");
        let second_holder = second.get("holder").unwrap().text().unwrap().to_string();
        assert_ne!(first_holder, second_holder);
        let state = second.get("state").unwrap().clone();
        // The turn that is already over may not retire the identity of the
        // newer live turn, and it may not extend its lease either.
        let stale = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state.clone()),
                ("ticket", s("ticket-11")),
                ("holder", s(&first_holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(stale.get("retired").unwrap(), &Bool(false));
        assert_eq!(
            stale
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("ticket")
                .unwrap()
                .text()
                .unwrap(),
            "ticket-12"
        );
        let stale_lease = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state.clone()),
                ("ticket", s("ticket-12")),
                ("holder", s(&first_holder)),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(stale_lease.get("extended").unwrap(), &Bool(false));
        let own = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state),
                ("ticket", s("ticket-12")),
                ("holder", s(&second_holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(own.get("retired").unwrap(), &Bool(true));
        assert_eq!(own.get("state").unwrap().get("turn_expected").unwrap(), &Null);
    }
    #[test]
    fn binary_network_zip_reaches_import_preview() {
        let root = std::env::current_dir()
            .unwrap()
            .join("toolchain")
            .join(format!(
                "client-network-zip-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("binary.dat"), [0, 255, 1]).unwrap();
        let built = operation(
            &obj(vec![
                ("op", s("project_archive")),
                ("root", s(root.to_str().unwrap())),
            ])
            .string(),
        )
        .unwrap();
        let zip = network::unbase64(built.get("archive").unwrap().text().unwrap()).unwrap();
        let mut response =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", zip.len()).into_bytes();
        response.extend_from_slice(&zip);
        let extracted = operation(
            &obj(vec![
                ("op", s("network_binary")),
                ("data", s(&network::b64(&response))),
            ])
            .string(),
        )
        .unwrap();
        let preview = operation(
            &obj(vec![
                ("op", s("archive_preview")),
                ("archive", extracted.get("archive").unwrap().clone()),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(preview.get("files").unwrap(), &Num("1".into()));
        assert!(preview.string().contains("binary.dat"));
        assert_eq!(
            network::unbase64(extracted.get("archive").unwrap().text().unwrap()).unwrap(),
            zip
        );
        // Preserve this owned verification fixture for review.
    }
}

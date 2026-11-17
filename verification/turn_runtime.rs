//! Product JSONL event/effect execution; Android lifecycle is a separate dimension.
mod twin;
use twin::{b, n, obj, s, Controller, Value};

const SCOPE: &str = "Executed output/headless/RLLM --jsonl: send event, returned model network effect, core admission, persisted owner state, detached-view adapter interval, rebind, controlled provider completion, background publication, denied drain and granted drain. The controlled HTTP response is an explicit input, not a claim of remote provider or Android transport execution. Android Service, notification delivery, Activity destruction, process kill, wake locks and measured wall-clock expiry are device-bound and unmeasured.";

fn main() {
    let root = std::env::current_dir().expect("project cwd");
    let root = root.to_str().expect("UTF-8 project path");
    let scratch = Scratch(twin::create_scratch(std::path::Path::new(root), "turn-runtime"));
    let workspace = scratch.0.to_str().expect("UTF-8 workspace");
    let mut product = Controller::spawn(root);
    let mut checks = Vec::new();
    let resumed = product.event(None, "resume", obj(vec![]), workspace);
    let mut current = state(&resumed).clone();
    put(&mut current, "profiles", Value::Arr(vec![obj(vec![
        ("name".into(), s("Controlled connection")),
        ("endpoint".into(), s("http://turn.test/v1/chat/completions")),
        ("protocol".into(), s("chat")), ("model".into(), s("controlled")),
        ("plain".into(), b(true)), ("stream".into(), b(false)),
    ])]));
    put(&mut current, "agents", Value::Arr(vec![obj(vec![
        ("name".into(), s("General")), ("model".into(), s("0")), ("tools".into(), s("")),
        ("investigation_preferences".into(), b(false)),
    ])]));
    put(&mut current, "dialog", Value::Null);
    let sent = product.event(Some(&current), "send", obj(vec![("draft".into(), s("Return a plain background reply."))]), workspace);
    let effect = sent.get("effect").expect("send effect");
    check(&mut checks, "product send returns a model network effect with the controlled request",
        effect.get("kind") == Some(&s("network")) && effect.get("purpose") == Some(&s("model"))
            && effect.get("host") == Some(&s("turn.test"))
            && text(effect, "wire").contains("controlled"));
    let ticket = text(effect, "model_ticket");
    check(&mut checks, "product effect ticket owns the pending model result",
        !ticket.is_empty() && state(&sent).get("model_expected") == Some(&s(&ticket)));
    let begun = turn(&mut product, vec![
        ("action", s("begin")), ("state", state(&sent).clone()), ("ticket", s(&ticket)),
        ("window_ms", n("120000")), ("elapsed_ms", n("30000")),
    ]);
    check(&mut checks, "product admission returns a bounded remaining lease and durable owner",
        begun.get("window_ms") == Some(&n("120000")) && begun.get("lease_ms") == Some(&n("90000"))
            && !text(&begun, "holder").is_empty());
    // No view callback is issued during this interval. Re-read the exact state
    // from disk before returning it through the public product operation.
    let persisted_path = scratch.0.join("turn-state.json");
    std::fs::write(&persisted_path, state(&begun).to_string()).expect("persist owner state");
    let persisted = Value::parse(&std::fs::read_to_string(&persisted_path).expect("read owner state"));
    let tick = turn(&mut product, vec![
        ("action", s("tick")), ("elapsed_ms", n("60000")),
        ("lease_elapsed_ms", n("15000")), ("lease_ms", n("90000")),
    ]);
    check(&mut checks, "product advances request progress during the detached-view adapter interval",
        tick.get("expired") == Some(&b(false)) && tick.get("elapsed_text") == Some(&s("1m 0s")));
    let rebound = turn(&mut product, vec![
        ("action", s("rebind")), ("state", persisted), ("live_ticket", s(&ticket)),
        ("live_holder", begun.get("holder").unwrap().clone()),
    ]);
    check(&mut checks, "product rebind restores the persisted owner without a request effect",
        rebound.get("decision") == Some(&s("rebound"))
            && state(&rebound).get("model_expected") == Some(&s(&ticket))
            && rebound.get("effect").is_none());
    let answer = "Measured background reply survives permission denial.";
    let body = obj(vec![("choices".into(), Value::Arr(vec![obj(vec![
        ("message".into(), obj(vec![("content".into(), s(answer))])), ("finish_reason".into(), s("stop")),
    ])]))]).to_string();
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let done = product.event(Some(state(&rebound)), "network_result", obj(vec![
        ("purpose".into(), s("model")), ("model_ticket".into(), s(&ticket)),
        ("data".into(), s(&twin::b64_encode(wire.as_bytes()))),
    ]), workspace);
    check(&mut checks, "product completion commits the controlled reply and settles its model effect",
        committed_reply(state(&done), answer)
            && state(&done).get("busy") == Some(&b(false))
            && done.get("effect") == Some(&Value::Null));
    let finished = turn(&mut product, vec![
        ("action", s("finish")), ("state", state(&done).clone()),
        ("ticket", s(&ticket)), ("holder", begun.get("holder").unwrap().clone()),
    ]);
    check(&mut checks, "product holder retirement preserves the committed conversation",
        finished.get("retired") == Some(&b(true))
            && state(&finished).get("turn_expected") == Some(&Value::Null)
            && committed_reply(state(&finished), answer));
    let conversation = text(state(&done), "active");
    assert!(!conversation.is_empty(), "completed conversation identity is absent");
    let held = turn(&mut product, vec![
        ("action", s("publish")), ("state", state(&finished).clone()), ("ticket", s(&ticket)),
        ("body", s(answer)), ("chat", s(&conversation)),
        ("backgrounded", b(true)), ("notifications_allowed", b(false)),
    ]);
    check(&mut checks, "product background publication holds the actual completed reply",
        held.get("decision") == Some(&s("hold")) && held.get("text") == Some(&s(answer)));
    let pending = state(&held).get("turn_pending").expect("pending announcement").clone();
    let denied = turn(&mut product, vec![
        ("action", s("publish")), ("state", state(&held).clone()),
        ("drain", b(true)), ("notifications_allowed", b(false)),
    ]);
    check(&mut checks, "product denied drain preserves the completed background reply",
        state(&denied).get("turn_pending") == Some(&pending) && denied.get("decision") == Some(&s("surface")));
    let granted = turn(&mut product, vec![
        ("action", s("publish")), ("state", state(&denied).clone()),
        ("drain", b(true)), ("notifications_allowed", b(true)),
    ]);
    check(&mut checks, "product granted drain publishes once and clears the held announcement",
        granted.get("decision") == Some(&s("publish")) && granted.get("text") == Some(&s(answer)) && granted.get("chat") == Some(&s(&conversation))
            && state(&granted).get("turn_pending") == Some(&Value::Null));
    let empty = turn(&mut product, vec![
        ("action", s("publish")), ("state", state(&granted).clone()),
        ("drain", b(true)), ("notifications_allowed", b(true)),
    ]);
    check(&mut checks, "product repeated drain does not duplicate publication", empty.get("decision") == Some(&s("suppress")));
    let error = product.failed(&obj(vec![("op".into(), s("turn")), ("action".into(), s("not-an-action"))]));
    check(&mut checks, "product invalid turn action is a scoped refusal", error.contains("Unknown turn action"));
    let after = turn(&mut product, vec![("action", s("tick")), ("elapsed_ms", n("1000")), ("lease_ms", n("30000"))]);
    check(&mut checks, "product remains usable after the scoped turn refusal", after.get("elapsed_text") == Some(&s("1s")));
    product.finish();
    let report = obj(vec![
        ("passed".into(), n(&checks.len().to_string())),
        ("checks".into(), Value::Arr(checks.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
        ("product_sha256".into(), s(&twin::sha256_bytes(&std::fs::read("output/headless/RLLM").expect("product bytes")))),
        ("model_effect".into(), effect.clone()),
        ("publication".into(), granted),
    ]);
    twin::report_value("turn_runtime", &report, true);
    println!("{}", twin::pretty(&report));
}

fn turn(product: &mut Controller, fields: Vec<(&str, Value)>) -> Value {
    let mut request = vec![("op".into(), s("turn"))];
    request.extend(fields.into_iter().map(|(key, value)| (key.into(), value)));
    product.op(&obj(request))
}
fn state(value: &Value) -> &Value { value.get("state").expect("product state") }
fn text(value: &Value, key: &str) -> String {
    match value.get(key) { Some(Value::Str(value)) => value.clone(), _ => String::new() }
}
fn put(value: &mut Value, key: &str, replacement: Value) {
    let Value::Obj(fields) = value else { panic!("state is not an object") };
    if let Some((_, prior)) = fields.iter_mut().find(|(name, _)| name == key) { *prior = replacement; }
    else { fields.push((key.into(), replacement)); }
}
fn check(checks: &mut Vec<String>, name: &str, passed: bool) {
    assert!(passed, "{name}"); checks.push(name.into());
}

fn committed_reply(state: &Value, answer: &str) -> bool {
    let active = text(state, "active");
    let Some(Value::Arr(chats)) = state.get("chats") else { return false };
    chats.iter().filter(|chat| text(chat, "id") == active).any(|chat| {
        let Some(Value::Arr(messages)) = chat.get("messages") else { return false };
        messages.iter().any(|message| text(message, "role") == "assistant" && text(message, "content") == answer)
    })
}

// The scratch tree is this twin's own working area. It must not survive the
// twin on any exit path, including a panic, so the removal rides on Drop the
// way the other twins that clean up after themselves do.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

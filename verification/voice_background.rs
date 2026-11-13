//! Owned optional background intent through actual PCM and speech HTTP peers.
mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::time::Duration;

use twin::{b, b64_decode, b64_encode, create_scratch, n, obj, report_value, s, truthy, Controller, Value};

const SCOPE: &str = "Owner execution through actual Rust PCM, local files and speech HTTP peers across all four model formats. Platform service notification/permission/lifecycle still requires separate adapter and physical Android acceptance.";
const FORMATS: [&str; 4] = ["chat", "local", "messages", "content"];

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = create_scratch(&root, "interruption");
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch_text = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let result = catch_unwind(AssertUnwindSafe(|| run(&mut controller, &scratch_text)));
    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);
    let passed = match result {
        Ok(passed) => passed,
        Err(error) => resume_unwind(error),
    };
    write_report(&passed);
}

fn run(controller: &mut Controller, scratch: &str) -> Vec<String> {
    let peer = Peer::start(wav_audio());
    let mut passed = Vec::new();
    for fmt in FORMATS {
        let state = configured(controller, scratch, fmt, peer.port);
        let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} background consent defaults off"),
            field_value(&d, "voice_background") == Some("false"),
        );

        let state = clone_state(&d);
        let d = event(
            controller,
            Some(&state),
            "voice_consent",
            obj(vec![("voice_background".into(), b(true))]),
            scratch,
        );
        let effect = required_effect(&d);
        let session = require_str(effect, "voice_session");
        let lease = num_u64(effect.get("voice_lease_ms"));
        keep(
            &mut passed,
            &format!("{fmt} explicit background intent reaches capture with a bounded remaining lease"),
            field_is(effect, "kind", "voice_listen")
                && present_truthy(effect, "voice_background")
                && lease.is_some_and(|value| value > 0 && value <= 600_000),
        );

        let state = clone_state(&d);
        let wrong = event(
            controller,
            Some(&state),
            "voice_session_stop",
            obj(vec![("voice_session".into(), s(&format!("{session}-stale")))]),
            scratch,
        );
        keep(
            &mut passed,
            &format!("{fmt} unrelated service stop is an exact no-op"),
            state_of(&wrong) == &state && effect_null(&wrong),
        );

        let d = spoken(controller, &d, scratch);
        let d = transport(controller, &d, scratch);
        keep(
            &mut passed,
            &format!("{fmt} actual recognition still reaches the configured primary model"),
            effect_field(&d, "purpose", "model") && present_truthy(state_of(&d), "voice_open"),
        );

        let state = clone_state(&d);
        let d = reply(controller, &state, scratch, fmt, "Background voice response.");
        let d = transport(controller, &d, scratch);
        let effect = required_effect(&d);
        keep(
            &mut passed,
            &format!("{fmt} actual returned speech retains its owned session"),
            field_is(effect, "kind", "media_preview")
                && effect.get("voice_session") == Some(&s(&session))
                && field_is(effect, "media_kind", "audio"),
        );

        let playback_ticket = require_str(effect, "voice_playback_ticket");
        let owner = obj(vec![
            ("voice_session".into(), s(&session)),
            ("voice_playback_ticket".into(), s(&playback_ticket)),
        ]);
        let state = clone_state(&d);
        let d = event(controller, Some(&state), "voice_playback_start", owner.clone(), scratch);
        let status = d.get("ui").and_then(|ui| ui.get("voice_status"));
        keep(
            &mut passed,
            &format!("{fmt} notification status exposes background intent and actual phase"),
            status.is_some_and(|status| present_truthy(status, "background") && field_is(status, "phase", "speaking")),
        );

        let mut completed = owner.clone();
        put(&mut completed, "completed", b(true));
        let state = clone_state(&d);
        let d = event(controller, Some(&state), "voice_playback_result", completed, scratch);
        let effect = required_effect(&d);
        let next_lease = num_u64(effect.get("voice_lease_ms"));
        keep(
            &mut passed,
            &format!("{fmt} next real capture keeps the same session without extending consent"),
            present_truthy(effect, "voice_background")
                && effect.get("voice_session") == Some(&s(&session))
                && lease.is_some()
                && next_lease.is_some_and(|value| value > 0 && lease.is_some_and(|limit| value <= limit)),
        );

        let capture_ticket = require_str(effect, "voice_audio_ticket");
        let state = clone_state(&d);
        let d = event(
            controller,
            Some(&state),
            "voice_session_stop",
            obj(vec![("voice_session".into(), s(&session))]),
            scratch,
        );
        let stopped = clone_state(&d);
        keep(
            &mut passed,
            &format!("{fmt} owned service stop retires the session and actual PCM owner"),
            present_falsy(&stopped, "voice_open")
                && get_falsy(&stopped, "voice_consent")
                && effect_field(&d, "kind", "media_playback_stop")
                && effect_truthy(&d, "voice_only"),
        );

        let stale = event(
            controller,
            Some(&stopped),
            "voice_audio_start",
            obj(vec![
                ("voice_session".into(), s(&session)),
                ("voice_audio_ticket".into(), s(&capture_ticket)),
            ]),
            scratch,
        );
        keep(
            &mut passed,
            &format!("{fmt} stopped session cannot revive capture"),
            state_of(&stale) == &stopped && effect_null(&stale),
        );

        let state = configured(controller, scratch, fmt, peer.port);
        let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), scratch);
        let state = clone_state(&d);
        let d = event(
            controller,
            Some(&state),
            "voice_consent",
            obj(vec![("voice_background".into(), b(true))]),
            scratch,
        );
        let d = spoken(controller, &d, scratch);
        let d = transport(controller, &d, scratch);
        let model_expected = state_of(&d).get("model_expected").cloned();
        let consent = state_of(&d).get("voice_consent").cloned().unwrap_or(Value::Null);
        let state = clone_state(&d);
        let stopped = event(
            controller,
            Some(&state),
            "voice_session_stop",
            obj(vec![("voice_session".into(), consent)]),
            scratch,
        );
        keep(
            &mut passed,
            &format!("{fmt} stopping service preserves an in-flight primary answer"),
            present_truthy(state_of(&stopped), "busy")
                && model_expected.is_some()
                && state_of(&stopped).get("model_expected") == model_expected.as_ref(),
        );

        let state = clone_state(&stopped);
        let done = reply(controller, &state, scratch, fmt, "Complete primary text survives voice stop.");
        keep(
            &mut passed,
            &format!("{fmt} primary result remains text without restarting speech"),
            effect_null(&done)
                && present_falsy(state_of(&done), "busy")
                && history_contains(state_of(&done), "Complete primary text survives voice stop."),
        );

        let state = configured(controller, scratch, fmt, peer.port);
        let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), scratch);
        let state = clone_state(&d);
        let d = event(controller, Some(&state), "voice_consent", obj(vec![]), scratch);
        keep(
            &mut passed,
            &format!("{fmt} ordinary foreground voice still needs no background service"),
            effect_field(&d, "kind", "voice_listen") && effect_falsy(&d, "voice_background"),
        );
        let state = clone_state(&d);
        let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), scratch);
    }

    let state = configured(controller, scratch, "chat", peer.port);
    let d = event(controller, Some(&state), "voice_review", obj(vec![]), scratch);
    let state = clone_state(&d);
    let refused = event(
        controller,
        Some(&state),
        "voice_consent",
        obj(vec![("voice_background".into(), b(true)), ("voice_open".into(), b(false))]),
        scratch,
    );
    keep(
        &mut passed,
        "background cannot silently enable a microphone for manual-only voice",
        effect_null(&refused) && get_truthy(state_of(&refused), "notice_error") && get_falsy(state_of(&refused), "voice_consent"),
    );

    let state = configured(controller, scratch, "chat", peer.port);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), scratch);
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "voice_consent",
        obj(vec![("voice_background".into(), b(true))]),
        scratch,
    );
    let old = state_of(&d).get("voice_consent").cloned().unwrap_or(Value::Null);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_stop", obj(vec![]), scratch);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), scratch);
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "voice_consent",
        obj(vec![("voice_background".into(), b(true))]),
        scratch,
    );
    let state = clone_state(&d);
    let late = event(
        controller,
        Some(&state),
        "voice_session_stop",
        obj(vec![("voice_session".into(), old)]),
        scratch,
    );
    keep(
        &mut passed,
        "old notification cannot stop a newly reviewed session",
        state_of(&late) == &state && effect_null(&late),
    );
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), scratch);

    let state = configured(controller, scratch, "chat", peer.port);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), scratch);
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "voice_consent",
        obj(vec![("voice_background".into(), b(true))]),
        scratch,
    );
    let mut retained = clone_state(&d);
    let expected_draft = "Retained core draft from a paused turn.";
    put(&mut retained, "draft", s(expected_draft));
    let saved = event(controller, Some(&retained), "status", obj(vec![]), scratch);
    keep(
        &mut passed,
        "background callback without stale form input preserves the actual unsent draft",
        state_of(&saved).get("draft") == Some(&s(expected_draft)),
    );
    let state = clone_state(&saved);
    let resumed = event(controller, Some(&state), "phone_access_status", obj(vec![]), scratch);
    keep(
        &mut passed,
        "activity return before form refresh keeps the retained draft",
        state_of(&resumed).get("draft") == Some(&s(expected_draft)),
    );
    let state = clone_state(&resumed);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), scratch);
    passed
}

fn configured(controller: &mut Controller, scratch: &str, fmt: &str, port: u16) -> Value {
    let d = event(controller, None, "resume", obj(vec![]), scratch);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "service_connections", obj(vec![]), scratch);
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "service_save",
        obj(vec![("service_config".into(), s(&service_config(port)))]),
        scratch,
    );
    let mut state = clone_state(&d);
    put(&mut state, "profiles", Value::Arr(vec![profile(fmt)]));
    put(&mut state, "agents", Value::Arr(vec![agent()]));
    state
}

fn service_config(port: u16) -> String {
    let host = obj(vec![
        ("id".into(), s("audio")),
        ("url".into(), s(&format!("http://127.0.0.1:{port}"))),
        ("plain".into(), b(true)),
        ("enabled".into(), b(true)),
    ]);
    let recognition = obj(vec![
        ("id".into(), s("recognition")),
        ("host".into(), s("audio")),
        ("capability".into(), s("speech_recognition")),
        ("path".into(), s("/recognize")),
        ("method".into(), s("POST")),
        ("request_format".into(), s("multipart")),
        ("file_field".into(), s("audio")),
        ("request".into(), obj(vec![])),
        ("response".into(), s("json")),
        ("response_path".into(), s("/text")),
        ("enabled".into(), b(true)),
    ]);
    let synthesis = obj(vec![
        ("id".into(), s("synthesis")),
        ("host".into(), s("audio")),
        ("capability".into(), s("speech_synthesis")),
        ("path".into(), s("/speak")),
        ("method".into(), s("POST")),
        ("response".into(), s("binary")),
        ("artifact_kind".into(), s("audio")),
        ("request".into(), obj(vec![("text".into(), obj(vec![("$input".into(), s("/text"))]))])),
        ("enabled".into(), b(true)),
    ]);
    obj(vec![
        ("version".into(), n("1")),
        ("hosts".into(), Value::Arr(vec![host])),
        ("routes".into(), Value::Arr(vec![recognition, synthesis])),
        (
            "defaults".into(),
            obj(vec![
                ("speech_recognition".into(), obj(vec![("routes".into(), Value::Arr(vec![s("recognition")]))])),
                ("speech_synthesis".into(), obj(vec![("routes".into(), Value::Arr(vec![s("synthesis")]))])),
            ]),
        ),
    ])
    .to_string()
}

fn profile(fmt: &str) -> Value {
    obj(vec![
        ("name".into(), s("Controlled")),
        ("endpoint".into(), s("http://model.test/v1/chat/completions")),
        ("protocol".into(), s(fmt)),
        ("model".into(), s("controlled")),
        ("plain".into(), b(true)),
        ("stream".into(), b(false)),
    ])
}

fn agent() -> Value {
    obj(vec![
        ("name".into(), s("General")),
        ("tools".into(), s("read,service")),
        ("investigation_preferences".into(), b(false)),
    ])
}

fn spoken(controller: &mut Controller, event_value: &Value, scratch: &str) -> Value {
    let effect = required_effect(event_value).clone();
    let session = require_str(&effect, "voice_session");
    let ticket = require_str(&effect, "voice_audio_ticket");
    let owner = obj(vec![
        ("voice_session".into(), s(&session)),
        ("voice_audio_ticket".into(), s(&ticket)),
    ]);
    let state = clone_state(event_value);
    let started = event(controller, Some(&state), "voice_audio_start", owner.clone(), scratch);
    let silence = vec![0u8; 640];
    let tone = tone_frame();
    let mut frames: Vec<&[u8]> = Vec::with_capacity(80);
    for _ in 0..20 {
        frames.push(&silence);
    }
    for _ in 0..20 {
        frames.push(&tone);
    }
    for _ in 0..40 {
        frames.push(&silence);
    }
    for frame in frames {
        let fed = controller.op(&obj(vec![
            ("op".into(), s("voice_audio_feed")),
            ("voice_session".into(), s(&session)),
            ("voice_audio_ticket".into(), s(&ticket)),
            ("data".into(), s(&b64_encode(frame))),
        ]));
        if fed.get("ready") == Some(&b(true)) {
            let receipt = fed.get("receipt").cloned().unwrap_or_else(|| panic!("utterance did not complete"));
            let mut completion = owner;
            put(&mut completion, "receipt", receipt);
            let state = clone_state(&started);
            return event(controller, Some(&state), "voice_audio_result", completion, scratch);
        }
    }
    panic!("utterance did not complete");
}

fn transport(controller: &mut Controller, event_value: &Value, scratch: &str) -> Value {
    let effect = required_effect(event_value).clone();
    let wire = wire_bytes(&effect);
    let host = require_str(&effect, "host");
    let port = as_u16(effect.get("port").unwrap_or_else(|| panic!("missing port")));
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("peer connect {host}:{port}: {error}"));
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
    upload(controller, &effect, &mut stream);
    stream.flush().unwrap_or_else(|error| panic!("peer flush: {error}"));
    let mut fields = response(controller, &effect, &mut stream);
    put(&mut fields, "purpose", effect.get("purpose").cloned().unwrap_or(Value::Null));
    let state = clone_state(event_value);
    event(controller, Some(&state), "network_result", fields, scratch)
}

fn upload(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) {
    let Some(ticket) = nonempty(effect.get("media_upload")) else { return };
    let purpose = require_str(effect, "purpose");
    let limit = as_usize(effect.get("upload_bytes").unwrap_or_else(|| panic!("missing upload_bytes")));
    let owner = obj(vec![("purpose".into(), s(&purpose)), ("ticket".into(), s(&ticket))]);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut count = 0usize;
        loop {
            let mut envelope = owner.clone();
            put(&mut envelope, "op", s("media_upload_read"));
            let part = controller.op(&envelope);
            let data = b64_decode(require_str(&part, "data").as_str());
            if data.len() > 8192 {
                panic!("upload chunk exceeds 8192");
            }
            count += data.len();
            if count > limit {
                panic!("upload exceeded declared length");
            }
            stream.write_all(&data).unwrap_or_else(|error| panic!("upload write: {error}"));
            if part.get("done") == Some(&b(true)) {
                if count != limit {
                    panic!("upload length mismatch {count} != {limit}");
                }
                return;
            }
            if data.is_empty() {
                panic!("upload returned an empty chunk");
            }
        }
    }));
    cancel_op(controller, &owner, "media_upload_cancel");
    if let Err(error) = outcome {
        resume_unwind(error);
    }
}

fn response(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) -> Value {
    let Some(ticket) = nonempty(effect.get("media_download")) else {
        return obj(vec![("data".into(), s(&b64_encode(&read_all(stream))))]);
    };
    let purpose = require_str(effect, "purpose");
    let owner = obj(vec![("purpose".into(), s(&purpose)), ("ticket".into(), s(&ticket))]);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut ready = false;
        loop {
            let chunk = read_chunk(stream);
            if chunk.is_empty() {
                break;
            }
            let mut envelope = owner.clone();
            put(&mut envelope, "op", s("media_stream_feed"));
            put(&mut envelope, "data", s(&b64_encode(&chunk)));
            let value = controller.op(&envelope);
            ready = value.get("ready") == Some(&b(true));
            if ready {
                break;
            }
        }
        let mut envelope = owner.clone();
        put(&mut envelope, "op", s("media_stream_finish"));
        put(&mut envelope, "eof", b(!ready));
        obj(vec![("media_receipt".into(), controller.op(&envelope))])
    }));
    cancel_op(controller, &owner, "media_stream_cancel");
    match outcome {
        Ok(value) => value,
        Err(error) => resume_unwind(error),
    }
}

fn cancel_op(controller: &mut Controller, owner: &Value, name: &str) {
    let mut envelope = owner.clone();
    put(&mut envelope, "op", s(name));
    let _ = controller.op(&envelope);
}

fn reply(controller: &mut Controller, state: &Value, scratch: &str, fmt: &str, answer: &str) -> Value {
    let body = model_body(fmt, answer);
    let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let ticket = state.get("model_expected").cloned().unwrap_or_else(|| panic!("missing model_expected"));
    event(
        controller,
        Some(state),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&b64_encode(wire.as_bytes()))),
        ]),
        scratch,
    )
}

fn model_body(fmt: &str, answer: &str) -> String {
    match fmt {
        "chat" => obj(vec![(
            "choices".into(),
            Value::Arr(vec![obj(vec![
                ("message".into(), obj(vec![("content".into(), s(answer))])),
                ("finish_reason".into(), s("stop")),
            ])]),
        )]),
        "local" => obj(vec![
            ("message".into(), obj(vec![("content".into(), s(answer))])),
            ("done".into(), b(true)),
        ]),
        "messages" => obj(vec![
            (
                "content".into(),
                Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s(answer))])]),
            ),
            ("stop_reason".into(), s("end_turn")),
        ]),
        "content" => obj(vec![(
            "candidates".into(),
            Value::Arr(vec![obj(vec![
                (
                    "content".into(),
                    obj(vec![(
                        "parts".into(),
                        Value::Arr(vec![obj(vec![("text".into(), s(answer))])]),
                    )]),
                ),
                ("finishReason".into(), s("STOP")),
            ])]),
        )]),
        _ => panic!("unknown format {fmt}"),
    }
    .to_string()
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, scratch: &str) -> Value {
    controller.event(state, action, inputs, scratch)
}

fn clone_state(value: &Value) -> Value {
    state_of(value).clone()
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", brief(value)))
}

fn required_effect(value: &Value) -> &Value {
    match value.get("effect") {
        Some(effect) if !matches!(effect, Value::Null) => effect,
        _ => panic!("missing effect; {}", brief(value)),
    }
}

fn effect_null(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn effect_field(value: &Value, key: &str, expect: &str) -> bool {
    value.get("effect").is_some_and(|effect| field_is(effect, key, expect))
}

fn effect_truthy(value: &Value, key: &str) -> bool {
    value.get("effect").is_some_and(|effect| present_truthy(effect, key))
}

fn effect_falsy(value: &Value, key: &str) -> bool {
    value.get("effect").is_some_and(|effect| present_falsy(effect, key))
}

fn field_is(value: &Value, key: &str, expect: &str) -> bool {
    value.get(key) == Some(&s(expect))
}

fn present_truthy(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(truthy)
}

fn present_falsy(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(item) => !truthy(item),
        None => false,
    }
}

fn get_truthy(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(truthy)
}

fn get_falsy(value: &Value, key: &str) -> bool {
    !truthy(value.get(key).unwrap_or(&Value::Null))
}

fn field_value<'a>(event_value: &'a Value, key: &str) -> Option<&'a str> {
    let fields = event_value.get("ui")?.get("modal")?.get("fields")?;
    let Value::Arr(items) = fields else { return None };
    items.iter().find(|field| field.get("key") == Some(&s(key))).and_then(|field| match field.get("value") {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    })
}

fn history_contains(state: &Value, needle: &str) -> bool {
    let Some(Value::Arr(chats)) = state.get("chats") else { return false };
    chats.iter().any(|chat| {
        let Some(Value::Arr(messages)) = chat.get("messages") else { return false };
        messages.iter().any(|message| match message.get("content") {
            Some(Value::Str(text)) => text.contains(needle),
            _ => false,
        })
    })
}

fn require_str(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        _ => panic!("missing {key} in {}", brief(value)),
    }
}

fn nonempty(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Str(text)) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn num_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Num(token) | Value::Str(token)) => token.parse().ok(),
        _ => None,
    }
}

fn as_u16(value: &Value) -> u16 {
    match value {
        Value::Num(token) | Value::Str(token) => token.parse().unwrap_or_else(|_| panic!("bad port {token}")),
        other => panic!("bad port {}", other.to_string()),
    }
}

fn as_usize(value: &Value) -> usize {
    match value {
        Value::Num(token) | Value::Str(token) => token.parse().unwrap_or_else(|_| panic!("bad size {token}")),
        other => panic!("bad size {}", other.to_string()),
    }
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    if let Some(Value::Str(text)) = effect.get("wire_base64") {
        return b64_decode(text);
    }
    if let Some(Value::Str(text)) = effect.get("wire") {
        return text.as_bytes().to_vec();
    }
    panic!("missing wire in {}", brief(effect));
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

fn brief(value: &Value) -> String {
    let text = value.to_string();
    if text.len() <= 400 {
        text
    } else {
        format!("{}…", &text[..400])
    }
}

fn wav_audio() -> Vec<u8> {
    let samples = 1600u32;
    let data_len = samples * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&32000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(out.len() + data_len as usize, 0);
    out
}

fn tone_frame() -> Vec<u8> {
    let mut out = Vec::with_capacity(640);
    for index in 0..320 {
        let sample: i16 = if index % 2 == 0 { 3000 } else { -3000 };
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn read_all(stream: &mut TcpStream) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => data.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
        }
        if data.len() > 8 * 1024 * 1024 {
            panic!("peer response exceeds 8 MiB");
        }
    }
    data
}

fn read_chunk(stream: &mut TcpStream) -> Vec<u8> {
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Vec::new(),
            Ok(count) => return buffer[..count].to_vec(),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("peer read: {error}"),
        }
    }
}

struct Peer {
    port: u16,
}

impl Peer {
    fn start(audio: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind peer: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer addr: {error}")).port();
        let recognized = br#"{"text": "Interrupted question."}"#.to_vec();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
                let Some((path, _)) = read_request(&mut stream) else { continue };
                let response_body = if path == "/speak" { audio.clone() } else { recognized.clone() };
                let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response_body.len());
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&response_body);
                let _ = stream.shutdown(Shutdown::Both);
            }
        });
        Self { port }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut header_end = None;
    let mut length = 0usize;
    loop {
        if data.len() > 8 * 1024 * 1024 {
            return None;
        }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => data.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        if header_end.is_none() {
            if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4) {
                length = content_length(&data[..end]);
                header_end = Some(end);
            }
        }
        if let Some(end) = header_end {
            if data.len() >= end + length {
                break;
            }
        }
    }
    let end = header_end?;
    if data.len() < end + length {
        return None;
    }
    let path = request_path(&data[..end]);
    Some((path, data[end..end + length].to_vec()))
}

fn request_path(header: &[u8]) -> String {
    let text = String::from_utf8_lossy(header);
    text.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string()
}

fn content_length(header: &[u8]) -> usize {
    let text = String::from_utf8_lossy(header);
    text.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn write_report(passed: &[String]) {
    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    report_value("voice_background", &body, true);
    let mut text = twin::pretty(&body);
    text.push('\n');
    let mut stdout = std::io::stdout();
    stdout.write_all(text.as_bytes()).unwrap_or_else(|error| panic!("stdout: {error}"));
    stdout.flush().unwrap_or_else(|error| panic!("stdout flush: {error}"));
}

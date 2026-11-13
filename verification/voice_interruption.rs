//! Owner execution with actual Rust PCM segmentation, files, HTTP service fixtures and controlled model replies/echo-start callbacks. Physical echo suppression, Android microphone/audio routing/lifecycle and live user providers remain unverified.
mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use twin::{b64_decode, b64_encode, n, obj, s, truthy, Controller, Value};

const SCOPE: &str = "Owner execution with actual Rust PCM segmentation, files, HTTP service fixtures and controlled model replies/echo-start callbacks. Physical echo suppression, Android microphone/audio routing/lifecycle and live user providers remain unverified.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&project);
    let root = scratch.path().to_string();
    let peer = Peer::start();
    let mut controller = Controller::spawn(&project_text);
    let outcome = catch_unwind(AssertUnwindSafe(|| run(&mut controller, &peer, &root)));
    controller.finish();
    let passed = match outcome {
        Ok(passed) => passed,
        Err(error) => resume_unwind(error),
    };
    if passed.len() != 104 {
        panic!("voice_interruption emitted {} checks", passed.len());
    }
    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("voice_interruption", &body, true);
    println!("{} voice_interruption checks passed", passed.len());
}

fn run(controller: &mut Controller, peer: &Peer, root: &str) -> Vec<String> {
    let tone = tone_frame();
    let marker = marker_frame();
    let quiet = quiet_frame();
    let mut passed = Vec::new();

    let ready = configured(controller, root, peer.port, "chat");
    let d = event(controller, &ready, "voice_open_review", obj(vec![]), root);
    let modal = d.get("ui").and_then(|ui| ui.get("modal")).map(Value::to_string).unwrap_or_default();
    keep(
        &mut passed,
        "voice review explicitly offers separate interruption and background consent",
        modal.contains("voice_interruption") && modal.contains("echo control") && modal.contains("voice_background") && modal.contains("notification"),
    );

    let ready = configured(controller, root, peer.port, "chat");
    let d = event(controller, &ready, "voice_open_review", obj(vec![]), root);
    let d = event(
        controller,
        &clone_state(&d),
        "voice_consent",
        obj(vec![
            ("voice_interruption".into(), Value::Bool(true)),
            ("voice_open".into(), Value::Bool(true)),
            ("voice_silence_ms".into(), s("600")),
            ("voice_threshold".into(), s("700")),
            ("voice_max_seconds".into(), s("10")),
        ]),
        root,
    );
    let expect_settings = obj(vec![
        ("interruption".into(), Value::Bool(true)),
        ("silence_ms".into(), n("600")),
        ("threshold".into(), n("700")),
        ("max_seconds".into(), n("10")),
    ]);
    keep(
        &mut passed,
        "reviewed microphone controls save their actual input values",
        state_of(&d).get("voice_settings") == Some(&expect_settings),
    );
    event(controller, &clone_state(&d), "voice_stop", obj(vec![]), root);

    let ready = configured(controller, root, peer.port, "chat");
    let d = event(controller, &ready, "voice_open_review", obj(vec![]), root);
    let d = event(controller, &clone_state(&d), "voice_consent", obj(vec![("voice_silence_ms".into(), s("299"))]), root);
    keep(
        &mut passed,
        "invalid reviewed microphone timing cannot start capture",
        effect_is_none(&d) && truthy_at(state_of(&d), "notice_error") && !truthy_at(state_of(&d), "voice_consent"),
    );

    let (d, _) = playback(controller, peer, root, &quiet, &tone, "chat", false);
    let started = event(controller, &clone_state(&d), "voice_playback_start", play_from(&d), root);
    keep(
        &mut passed,
        "default-off retains half-duplex playback without a watch",
        effect_is_none(&started) && !truthy_at(state_of(&started), "voice_watch_expected"),
    );
    event(controller, &clone_state(&started), "voice_stop", obj(vec![]), root);

    for fmt in ["chat", "local", "messages", "content"] {
        let (d, play) = playback(controller, peer, root, &quiet, &tone, fmt, true);
        keep(
            &mut passed,
            &format!("{fmt} reviewed setting reaches owned playback"),
            effect_flag(&d, "voice_interruption"),
        );
        let d = event(controller, &clone_state(&d), "voice_playback_start", play.clone(), root);
        keep(
            &mut passed,
            &format!("{fmt} actual playback start opens only an owned watch"),
            kind_is(&d, "voice_watch") && same_field(effect_of(&d), &play, "voice_playback_ticket") && status_is(&d, "speaking"),
        );
        let owner = watch_owner(effect_of(&d));
        let wrong = event(
            controller,
            &clone_state(&d),
            "voice_watch_start",
            with(owner.clone(), vec![("echo_active", Value::Bool(false)), ("draft", s("injected"))]),
            root,
        );
        keep(&mut passed, &format!("{fmt} inactive echo cannot announce a live microphone"), same_state(&wrong, &d));
        let d = event(controller, &clone_state(&d), "voice_watch_start", with(owner.clone(), vec![("echo_active", Value::Bool(true))]), root);
        keep(
            &mut passed,
            &format!("{fmt} active echo handshake exposes microphone state"),
            d.get("ui").is_some_and(|ui| ui.to_string().contains("microphone on")) && text_is(state_of(&d), "voice_interruption_status", "listening"),
        );
        let again = event(controller, &clone_state(&d), "voice_watch_start", with(owner.clone(), vec![("echo_active", Value::Bool(true))]), root);
        keep(&mut passed, &format!("{fmt} duplicate watch start is an exact no-op"), same_state(&again, &d));
        for frame in iter_frames(&[(&quiet, 30), (&tone, 5), (&quiet, 5)]) {
            let value = feed(controller, &owner, frame);
            if value.get("onset_receipt").is_some() {
                panic!("{fmt} silence and short noise produced an onset");
            }
        }
        keep(
            &mut passed,
            &format!("{fmt} silence and short noise do not interrupt playback"),
            status_is(&d, "speaking") && !truthy_at(state_of(&d), "service_expected"),
        );
        let notice = onset(controller, &owner, &quiet, &marker, &tone);
        let bad = event(
            controller,
            &clone_state(&d),
            "voice_watch_onset",
            with(notice.clone(), vec![("onset_receipt", s("wrong")), ("draft", s("injected"))]),
            root,
        );
        keep(&mut passed, &format!("{fmt} unowned onset receipt is an exact no-op"), same_state(&bad, &d));
        let previous = state_of(&d).get("media_input").cloned().unwrap_or_else(|| panic!("missing media_input"));
        let d = event(controller, &clone_state(&d), "voice_watch_onset", notice.clone(), root);
        keep(
            &mut passed,
            &format!("{fmt} onset promotes the same capture and stops only playback"),
            kind_is(&d, "voice_interrupt_listen")
                && status_is(&d, "listening")
                && state_of(&d).get("voice_audio_expected").and_then(|expected| expected.get("ticket")) == owner.get("voice_audio_ticket")
                && !truthy_at(state_of(&d), "voice_watch_expected")
                && state_of(&d).get("media_input") == Some(&previous),
        );
        for (action, values) in [
            ("voice_watch_onset", notice.clone()),
            ("voice_watch_unavailable", owner.clone()),
            ("voice_playback_result", with(play.clone(), vec![("completed", Value::Bool(true))])),
        ] {
            let stale = event(controller, &clone_state(&d), action, values, root);
            keep(&mut passed, &format!("{fmt} retired callback cannot alter promoted capture {action}"), same_state(&stale, &d));
        }
        let mut receipt = None;
        for frame in iter_frames(&[(&tone, 10), (&quiet, 40)]) {
            let value = feed(controller, &owner, frame);
            if flag(&value, "ready") {
                receipt = Some(require_clone(&value, "receipt"));
                break;
            }
        }
        let receipt = receipt.unwrap_or_else(|| panic!("promoted utterance did not complete"));
        let d = event(controller, &clone_state(&d), "voice_audio_result", with(owner.clone(), vec![("receipt", receipt)]), root);
        keep(
            &mut passed,
            &format!("{fmt} promoted utterance starts ordinary recognition"),
            effect_present(&d) && status_is(&d, "recognizing"),
        );
        let d = transport(controller, &d, root, false);
        let payload = peer.last_body();
        keep(
            &mut passed,
            &format!("{fmt} leading speech PCM survives promotion into actual HTTP audio"),
            wav_contains_sequence(&payload, &marker, &tone),
        );
        keep(
            &mut passed,
            &format!("{fmt} interrupted speech reaches captured primary model"),
            text_is(effect_of(&d), "purpose", "model") && wire_contains(effect_of(&d), "Interrupted question."),
        );
        let pending = d;
        let stale = event(controller, &clone_state(&pending), "voice_watch_onset", with(notice, vec![("draft", s("injected"))]), root);
        keep(&mut passed, &format!("{fmt} stale watch cannot alter the primary request"), same_state(&stale, &pending));
        let d = reply(controller, &pending, root, fmt, "Reply after interruption.");
        let d = transport(controller, &d, root, false);
        keep(
            &mut passed,
            &format!("{fmt} next reply keeps configured output and a new playback owner"),
            kind_is(&d, "media_preview")
                && effect_of(&d).get("voice_playback_ticket").is_some()
                && effect_of(&d).get("voice_playback_ticket") != play.get("voice_playback_ticket")
                && json_body_is(&peer.last_body(), &obj(vec![("text".into(), s("Reply after interruption."))])),
        );
        event(controller, &clone_state(&d), "voice_stop", obj(vec![]), root);
    }

    let (d, owner, _) = watched(controller, peer, root, &quiet, &tone);
    let notice = onset(controller, &owner, &quiet, &marker, &tone);
    let mut receipt = None;
    for frame in iter_frames(&[(&tone, 10), (&quiet, 40)]) {
        let value = feed(controller, &owner, frame);
        if flag(&value, "ready") {
            receipt = Some(require_clone(&value, "receipt"));
            break;
        }
    }
    let receipt = receipt.unwrap_or_else(|| panic!("watch audio did not complete"));
    let blocked = event(controller, &clone_state(&d), "voice_audio_result", with(owner.clone(), vec![("receipt", receipt.clone())]), root);
    keep(
        &mut passed,
        "complete watch audio cannot submit before onset promotion",
        same_state(&blocked, &d) && effect_is_none(&blocked),
    );
    let d = event(controller, &clone_state(&d), "voice_watch_onset", notice, root);
    let d = event(controller, &clone_state(&d), "voice_audio_result", with(owner.clone(), vec![("receipt", receipt)]), root);
    keep(
        &mut passed,
        "queued onset promotes already completed PCM without losing its receipt",
        status_is(&d, "recognizing") && effect_present(&d),
    );
    event(controller, &clone_state(&d), "voice_stop", obj(vec![]), root);

    let (d, owner, play) = watched(controller, peer, root, &quiet, &tone);
    let before = state_value(&d, "voice_playback_expected", "echo failure preserves current reply and consent");
    let requests = peer.len();
    let done = event(controller, &clone_state(&d), "voice_watch_unavailable", owner.clone(), root);
    keep(
        &mut passed,
        "echo failure preserves current reply and consent",
        effect_is_none(&done)
            && status_is(&done, "speaking")
            && state_of(&done).get("voice_playback_expected") == Some(&before)
            && state_of(&done).get("voice_consent") == owner.get("voice_session")
            && peer.len() == requests,
    );
    let retired = raw_feed(controller, &owner, &tone);
    keep(&mut passed, "echo failure retires only its microphone watch", retired.get("value").is_none());
    let manual = event(controller, &clone_state(&done), "voice_interrupt", play, root);
    keep(&mut passed, "manual interruption remains usable after echo failure", kind_is(&manual, "voice_listen"));
    event(controller, &clone_state(&manual), "voice_stop", obj(vec![]), root);

    let (d, owner, _) = watched(controller, peer, root, &quiet, &tone);
    let notice = onset(controller, &owner, &quiet, &marker, &tone);
    let mut edited = clone_state(&d);
    put(&mut edited, "draft", s("Keep these typed words."));
    let done = event(controller, &edited, "voice_watch_onset", notice, root);
    keep(
        &mut passed,
        "onset preserves an unsent draft and makes no recognition request",
        text_is(state_of(&done), "draft", "Keep these typed words.")
            && status_is(&done, "paused")
            && kind_is(&done, "media_playback_stop")
            && !truthy_at(state_of(&done), "service_expected"),
    );
    event(controller, &clone_state(&done), "voice_stop", obj(vec![]), root);

    let (d, _, _) = watched(controller, peer, root, &quiet, &tone);
    let playback_expected = state_value(&d, "voice_playback_expected", "draft editing stops watch without stopping returned speech");
    let edited = event(controller, &clone_state(&d), "capture", obj(vec![("draft".into(), s("Editing during the reply."))]), root);
    keep(
        &mut passed,
        "draft editing stops watch without stopping returned speech",
        status_is(&edited, "speaking")
            && !truthy_at(state_of(&edited), "voice_watch_expected")
            && state_of(&edited).get("voice_playback_expected") == Some(&playback_expected),
    );
    event(controller, &clone_state(&edited), "voice_stop", obj(vec![]), root);

    for action in ["voice_suspend", "voice_stop", "service_connections"] {
        let (d, owner, _) = watched(controller, peer, root, &quiet, &tone);
        let notice = onset(controller, &owner, &quiet, &marker, &tone);
        let stopped = event(controller, &clone_state(&d), action, obj(vec![]), root);
        let late = event(controller, &clone_state(&stopped), "voice_watch_onset", with(notice, vec![("draft", s("injected"))]), root);
        keep(
            &mut passed,
            &format!("{action} revokes the watch and ignores late onset"),
            !truthy_at(state_of(&stopped), "voice_consent") && same_state(&late, &stopped),
        );
        let retired = raw_feed(controller, &owner, &tone);
        keep(&mut passed, &format!("{action} retires microphone PCM ownership"), retired.get("value").is_none());
    }

    let (d, owner, _) = watched(controller, peer, root, &quiet, &tone);
    let notice = onset(controller, &owner, &quiet, &marker, &tone);
    let sent = event(controller, &clone_state(&d), "send", obj(vec![("draft".into(), s("Typed primary request."))]), root);
    let late = event(controller, &clone_state(&sent), "voice_watch_onset", notice, root);
    keep(
        &mut passed,
        "new typed primary owner survives a late spoken interruption",
        text_is(effect_of(&sent), "purpose", "model") && same_state(&late, &sent),
    );
    reply(controller, &sent, root, "chat", "Typed result retained.");

    let (d, owner, play) = watched(controller, peer, root, &quiet, &tone);
    let done = event(controller, &clone_state(&d), "voice_playback_result", with(play, vec![("completed", Value::Bool(true))]), root);
    keep(
        &mut passed,
        "normal playback completion replaces watch with a normal listener",
        kind_is(&done, "voice_listen")
            && effect_of(&done).get("voice_audio_ticket").is_some()
            && effect_of(&done).get("voice_audio_ticket") != owner.get("voice_audio_ticket")
            && !truthy_at(state_of(&done), "voice_watch_expected"),
    );
    event(controller, &clone_state(&done), "voice_stop", obj(vec![]), root);

    for phase in ["listening", "recognizing", "thinking", "synthesizing", "speaking"] {
        let ready = configured(controller, root, peer.port, "chat");
        let mut d = event(controller, &ready, "voice_open_review", obj(vec![]), root);
        d = event(controller, &clone_state(&d), "voice_consent", obj(vec![("voice_interruption".into(), Value::Bool(true))]), root);
        let session = require_clone(state_of(&d), "voice_consent");
        if phase != "listening" {
            d = spoken(controller, d, root, &quiet, &tone);
        }
        if matches!(phase, "thinking" | "synthesizing" | "speaking") {
            d = transport(controller, &d, root, false);
        }
        if matches!(phase, "synthesizing" | "speaking") {
            d = reply(controller, &d, root, "chat", "Retained route test reply.");
        }
        if phase == "speaking" {
            d = transport(controller, &d, root, false);
            let play = obj(vec![
                ("voice_session".into(), session.clone()),
                ("voice_playback_ticket".into(), require_clone(effect_of(&d), "voice_playback_ticket")),
            ]);
            d = event(controller, &clone_state(&d), "voice_playback_start", play, root);
        }
        let delayed = if matches!(phase, "recognizing" | "synthesizing") {
            Some(transport(controller, &d, root, true))
        } else {
            None
        };
        let before = peer.len();
        let wrong = event(
            controller,
            &clone_state(&d),
            "voice_route_lost",
            obj(vec![("voice_session".into(), s("retired-session")), ("draft".into(), s("injected"))]),
            root,
        );
        keep(&mut passed, &format!("{phase} wrong route owner cannot alter the conversation"), same_state(&wrong, &d));
        let stopped = event(controller, &clone_state(&d), "voice_route_lost", obj(vec![("voice_session".into(), session.clone())]), root);
        keep(
            &mut passed,
            &format!("{phase} route loss retires consent and capture without replay"),
            !truthy_at(state_of(&stopped), "voice_consent")
                && !truthy_at(state_of(&stopped), "voice_open")
                && !truthy_at(state_of(&stopped), "voice_audio_expected")
                && !truthy_at(state_of(&stopped), "voice_watch_expected")
                && !truthy_at(state_of(&stopped), "voice_playback_expected")
                && peer.len() == before,
        );
        let again = event(
            controller,
            &clone_state(&stopped),
            "voice_route_lost",
            obj(vec![("voice_session".into(), session.clone()), ("draft".into(), s("injected"))]),
            root,
        );
        keep(&mut passed, &format!("{phase} late route callback is an exact no-op"), same_state(&again, &stopped));
        if phase == "thinking" {
            let done = reply(controller, &stopped, root, "chat", "Primary answer retained after route loss.");
            keep(
                &mut passed,
                "route loss preserves an in-flight primary answer without speaking or rearming",
                effect_is_none(&done) && !truthy_at(state_of(&done), "voice_open") && state_of(&done).to_string().contains("Primary answer retained after route loss.") && peer.len() == before,
            );
        } else if matches!(phase, "recognizing" | "synthesizing") {
            let late = event(controller, &clone_state(&stopped), "network_result", delayed.unwrap(), root);
            keep(
                &mut passed,
                &format!("{phase} late actual HTTP result stays retired"),
                same_state(&late, &stopped) && effect_is_none(&late) && peer.len() == before,
            );
        } else {
            let fresh = event(controller, &clone_state(&stopped), "voice_open_review", obj(vec![]), root);
            let fresh = event(controller, &clone_state(&fresh), "voice_consent", obj(vec![]), root);
            let old = event(controller, &clone_state(&fresh), "voice_route_lost", obj(vec![("voice_session".into(), session)]), root);
            keep(
                &mut passed,
                &format!("{phase} previous route owner cannot stop a newly reviewed microphone"),
                same_state(&old, &fresh) && kind_is(&fresh, "voice_listen"),
            );
            event(controller, &clone_state(&fresh), "voice_stop", obj(vec![]), root);
        }
    }

    std::fs::write(Path::new(root).join("local.txt"), b"local operation retained").unwrap_or_else(|error| panic!("local.txt: {error}"));
    let local = controller.op(&obj(vec![
        ("op".into(), s("tool")),
        ("tool".into(), s("read")),
        ("root".into(), s(root)),
        ("args".into(), obj(vec![("path".into(), s("local.txt"))])),
    ]));
    keep(&mut passed, "local work remains usable without voice or service availability", matches!(local, Value::Str(text) if text == "local operation retained"));
    passed
}

fn configured(controller: &mut Controller, root: &str, port: u16, fmt: &str) -> Value {
    let cfg = obj(vec![
        ("version".into(), n("1")),
        (
            "hosts".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("audio")),
                ("url".into(), s(&format!("http://127.0.0.1:{port}"))),
                ("plain".into(), Value::Bool(true)),
                ("enabled".into(), Value::Bool(true)),
            ])]),
        ),
        (
            "routes".into(),
            Value::Arr(vec![
                obj(vec![
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
                    ("enabled".into(), Value::Bool(true)),
                ]),
                obj(vec![
                    ("id".into(), s("synthesis")),
                    ("host".into(), s("audio")),
                    ("capability".into(), s("speech_synthesis")),
                    ("path".into(), s("/speak")),
                    ("method".into(), s("POST")),
                    ("response".into(), s("binary")),
                    ("artifact_kind".into(), s("audio")),
                    ("request".into(), obj(vec![("text".into(), obj(vec![("$input".into(), s("/text"))]))])),
                    ("enabled".into(), Value::Bool(true)),
                ]),
            ]),
        ),
        (
            "defaults".into(),
            obj(vec![
                ("speech_recognition".into(), obj(vec![("routes".into(), Value::Arr(vec![s("recognition")]))])),
                ("speech_synthesis".into(), obj(vec![("routes".into(), Value::Arr(vec![s("synthesis")]))])),
            ]),
        ),
    ]);
    let d = controller.event(None, "resume", obj(vec![]), root);
    let d = event(controller, &clone_state(&d), "service_connections", obj(vec![]), root);
    let d = event(controller, &clone_state(&d), "service_save", obj(vec![("service_config".into(), s(&cfg.to_string()))]), root);
    let mut st = clone_state(&d);
    put(
        &mut st,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Controlled")),
            ("endpoint".into(), s("http://model.test/v1/chat/completions")),
            ("protocol".into(), s(fmt)),
            ("model".into(), s("controlled")),
            ("plain".into(), Value::Bool(true)),
            ("stream".into(), Value::Bool(false)),
        ])]),
    );
    put(
        &mut st,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("read,service")),
            ("investigation_preferences".into(), Value::Bool(false)),
        ])]),
    );
    st
}

fn playback(controller: &mut Controller, peer: &Peer, root: &str, quiet: &[u8], tone: &[u8], fmt: &str, interruption: bool) -> (Value, Value) {
    let ready = configured(controller, root, peer.port, fmt);
    let mut d = event(controller, &ready, "voice_open_review", obj(vec![]), root);
    d = event(controller, &clone_state(&d), "voice_consent", obj(vec![("voice_interruption".into(), Value::Bool(interruption))]), root);
    d = spoken(controller, d, root, quiet, tone);
    d = transport(controller, &d, root, false);
    d = reply(controller, &d, root, fmt, "Reply before interruption.");
    d = transport(controller, &d, root, false);
    let play = play_owner(effect_of(&d));
    (d, play)
}

fn watched(controller: &mut Controller, peer: &Peer, root: &str, quiet: &[u8], tone: &[u8]) -> (Value, Value, Value) {
    let (d, play) = playback(controller, peer, root, quiet, tone, "chat", true);
    let d = event(controller, &clone_state(&d), "voice_playback_start", play.clone(), root);
    if !kind_is(&d, "voice_watch") {
        panic!("voice_watch did not open: {}", notice_text(&d));
    }
    let owner = watch_owner(effect_of(&d));
    let d = event(controller, &clone_state(&d), "voice_watch_start", with(owner.clone(), vec![("echo_active", Value::Bool(true))]), root);
    if !text_is(state_of(&d), "voice_interruption_status", "listening") {
        panic!("watch did not start listening: {}", notice_text(&d));
    }
    (d, owner, play)
}

fn spoken(controller: &mut Controller, d: Value, root: &str, quiet: &[u8], tone: &[u8]) -> Value {
    let owner = mic_owner(effect_of(&d));
    let d = event(controller, &clone_state(&d), "voice_audio_start", owner.clone(), root);
    for frame in iter_frames(&[(quiet, 20), (tone, 20), (quiet, 40)]) {
        let value = feed(controller, &owner, frame);
        if flag(&value, "ready") {
            return event(controller, &clone_state(&d), "voice_audio_result", with(owner, vec![("receipt", require_clone(&value, "receipt"))]), root);
        }
    }
    panic!("utterance did not complete");
}

fn onset(controller: &mut Controller, owner: &Value, quiet: &[u8], marker: &[u8], tone: &[u8]) -> Value {
    for frame in iter_frames(&[(quiet, 20), (marker, 1), (tone, 11)]) {
        let value = feed(controller, owner, frame);
        if let Some(receipt) = value.get("onset_receipt") {
            return with(owner.clone(), vec![("onset_receipt", receipt.clone())]);
        }
    }
    panic!("speech onset did not complete");
}

fn feed(controller: &mut Controller, owner: &Value, frame: &[u8]) -> Value {
    controller.op(&with(owner.clone(), vec![("op", s("voice_audio_feed")), ("data", s(&b64_encode(frame)))]))
}

fn raw_feed(controller: &mut Controller, owner: &Value, frame: &[u8]) -> Value {
    controller.send(&with(owner.clone(), vec![("op", s("voice_audio_feed")), ("data", s(&b64_encode(frame)))]))
}

fn transport(controller: &mut Controller, event_value: &Value, root: &str, delay: bool) -> Value {
    let effect = required_effect(event_value).clone();
    let wire = wire_bytes(&effect);
    let host = require_string(&effect, "host");
    let port = as_u16(require(&effect, "port"));
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(30))).ok();
    stream.write_all(&wire).unwrap_or_else(|error| panic!("wire write: {error}"));
    upload(controller, &effect, &mut stream);
    let mut fields = response(controller, &effect, &mut stream);
    put(&mut fields, "purpose", require_clone(&effect, "purpose"));
    if delay {
        fields
    } else {
        let state = clone_state(event_value);
        controller.event(Some(&state), "network_result", fields, root)
    }
}

fn upload(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) {
    let Some(ticket) = nonempty(effect.get("media_upload")) else { return };
    let purpose = require_string(effect, "purpose");
    let limit = as_usize(require(effect, "upload_bytes"));
    let owner = obj(vec![("purpose".into(), s(&purpose)), ("ticket".into(), s(&ticket))]);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut count = 0usize;
        loop {
            let part = controller.op(&with(owner.clone(), vec![("op", s("media_upload_read"))]));
            let data = b64_decode(require_string(&part, "data").as_str());
            if data.len() > 8192 {
                panic!("upload chunk exceeds 8192");
            }
            count += data.len();
            if count > limit {
                panic!("upload exceeded declared length");
            }
            stream.write_all(&data).unwrap_or_else(|error| panic!("upload write: {error}"));
            if flag(&part, "done") {
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
    let purpose = require_string(effect, "purpose");
    let owner = obj(vec![("purpose".into(), s(&purpose)), ("ticket".into(), s(&ticket))]);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut ready = false;
        loop {
            let chunk = read_chunk(stream);
            if chunk.is_empty() {
                break;
            }
            let value = controller.op(&with(owner.clone(), vec![("op", s("media_stream_feed")), ("data", s(&b64_encode(&chunk)))]));
            ready = flag(&value, "ready");
            if ready {
                break;
            }
        }
        let receipt = controller.op(&with(owner.clone(), vec![("op", s("media_stream_finish")), ("eof", Value::Bool(!ready))]));
        obj(vec![("media_receipt".into(), receipt)])
    }));
    cancel_op(controller, &owner, "media_stream_cancel");
    match outcome {
        Ok(value) => value,
        Err(error) => resume_unwind(error),
    }
}

fn cancel_op(controller: &mut Controller, owner: &Value, name: &str) {
    let _ = controller.op(&with(owner.clone(), vec![("op", s(name))]));
}

fn reply(controller: &mut Controller, event_value: &Value, root: &str, fmt: &str, answer: &str) -> Value {
    let body = model_body(fmt, answer);
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body.as_bytes());
    let state = clone_state(event_value);
    let ticket = state.get("model_expected").cloned().unwrap_or_else(|| panic!("missing model_expected"));
    controller.event(
        Some(&state),
        "network_result",
        obj(vec![("purpose".into(), s("model")), ("model_ticket".into(), ticket), ("data".into(), s(&b64_encode(&wire)))]),
        root,
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
            ("done".into(), Value::Bool(true)),
        ]),
        "messages" => obj(vec![
            ("content".into(), Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s(answer))])])),
            ("stop_reason".into(), s("end_turn")),
        ]),
        "content" => obj(vec![(
            "candidates".into(),
            Value::Arr(vec![obj(vec![
                ("content".into(), obj(vec![("parts".into(), Value::Arr(vec![obj(vec![("text".into(), s(answer))])]))])),
                ("finishReason".into(), s("STOP")),
            ])]),
        )]),
        _ => panic!("unknown format {fmt}"),
    }
    .to_string()
}

fn event(controller: &mut Controller, state: &Value, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(Some(state), action, inputs, root)
}

fn play_from(event_value: &Value) -> Value {
    play_owner(effect_of(event_value))
}

fn play_owner(effect: &Value) -> Value {
    obj(vec![
        ("voice_session".into(), require_clone(effect, "voice_session")),
        ("voice_playback_ticket".into(), require_clone(effect, "voice_playback_ticket")),
    ])
}

fn mic_owner(effect: &Value) -> Value {
    obj(vec![
        ("voice_session".into(), require_clone(effect, "voice_session")),
        ("voice_audio_ticket".into(), require_clone(effect, "voice_audio_ticket")),
    ])
}

fn watch_owner(effect: &Value) -> Value {
    obj(vec![
        ("voice_session".into(), require_clone(effect, "voice_session")),
        ("voice_audio_ticket".into(), require_clone(effect, "voice_audio_ticket")),
        ("voice_playback_ticket".into(), require_clone(effect, "voice_playback_ticket")),
    ])
}

fn with(mut value: Value, extra: Vec<(&str, Value)>) -> Value {
    for (key, item) in extra {
        put(&mut value, key, item);
    }
    value
}

fn clone_state(value: &Value) -> Value {
    state_of(value).clone()
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", notice_text(value)))
}

fn effect_of(value: &Value) -> &Value {
    match value.get("effect") {
        Some(effect) if !matches!(effect, Value::Null) => effect,
        _ => panic!("missing effect: {}", notice_text(value)),
    }
}

fn required_effect(value: &Value) -> &Value {
    let effect = effect_of(value);
    if !truthy(effect) {
        panic!("empty effect: {}", notice_text(value));
    }
    effect
}

fn effect_is_none(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn effect_present(value: &Value) -> bool {
    value.get("effect").is_some_and(truthy)
}

fn kind_is(value: &Value, kind: &str) -> bool {
    value.get("effect").and_then(|effect| effect.get("kind")).and_then(as_str) == Some(kind)
}

// The leg reads this key by indexing, so a missing key raises and the leg exits
// non-zero. `unwrap_or(Null)` would let both sides be absent and keep the check.
fn state_value(value: &Value, key: &str, check: &str) -> Value {
    state_of(value)
        .get(key)
        .cloned()
        .unwrap_or_else(|| panic!("{check}: state is missing {key}"))
}

fn effect_flag(value: &Value, key: &str) -> bool {
    matches!(value.get("effect").and_then(|effect| effect.get(key)), Some(Value::Bool(true)))
}

fn status_is(value: &Value, status: &str) -> bool {
    text_is(state_of(value), "voice_status", status)
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    value.get(key).and_then(as_str) == Some(expect)
}

fn truthy_at(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(truthy)
}

fn same_state(left: &Value, right: &Value) -> bool {
    state_of(left) == state_of(right)
}

fn same_field(left: &Value, right: &Value, key: &str) -> bool {
    left.get(key).is_some() && left.get(key) == right.get(key)
}

fn wire_contains(effect: &Value, needle: &str) -> bool {
    effect.get("wire").and_then(as_str).is_some_and(|text| text.contains(needle))
}

fn notice_text(value: &Value) -> String {
    value
        .get("state")
        .and_then(|state| state.get("notice"))
        .map(Value::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text.as_str()),
        _ => None,
    }
}

fn flag(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Bool(true)))
}

fn require<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key} in {}", value.to_string()))
}

fn require_clone(value: &Value, key: &str) -> Value {
    require(value, key).clone()
}

fn require_string(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) if !text.is_empty() => text.clone(),
        _ => panic!("missing {key} in {}", value.to_string()),
    }
}

fn nonempty(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Str(text)) if !text.is_empty() => Some(text.clone()),
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
    if effect.get("wire_base64").is_some() {
        return b64_decode(require_string(effect, "wire_base64").as_str());
    }
    if let Some(Value::Str(text)) = effect.get("wire") {
        return text.as_bytes().to_vec();
    }
    panic!("missing wire in {}", effect.to_string());
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
    println!("{name}");
    let _ = std::io::stdout().flush();
}

fn iter_frames<'a>(plan: &'a [(&'a [u8], usize)]) -> impl Iterator<Item = &'a [u8]> + 'a {
    plan.iter().flat_map(|(frame, count)| std::iter::repeat(*frame).take(*count))
}

fn tone_frame() -> Vec<u8> {
    let mut out = Vec::with_capacity(640);
    for index in 0..320 {
        let sample: i16 = if index % 2 == 0 { 3000 } else { -3000 };
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn marker_frame() -> Vec<u8> {
    let mut out = Vec::with_capacity(640);
    for index in 0..320 {
        let sample: i16 = if index % 2 == 0 { 2600 } else { -2600 };
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn quiet_frame() -> Vec<u8> {
    vec![0; 640]
}

fn json_body_is(body: &[u8], expect: &Value) -> bool {
    let Ok(text) = std::str::from_utf8(body) else { return false };
    std::panic::catch_unwind(|| Value::parse(text)).ok().as_ref() == Some(expect)
}

fn wav_contains_sequence(body: &[u8], marker: &[u8], tone: &[u8]) -> bool {
    let Some(offset) = body.windows(4).position(|window| window == b"RIFF") else { return false };
    if body.len() < offset + 12 {
        return false;
    }
    let size = u32::from_le_bytes(body[offset + 4..offset + 8].try_into().unwrap_or([0; 4])) as usize;
    let Some(end) = offset.checked_add(size).and_then(|value| value.checked_add(8)) else { return false };
    if end > body.len() || &body[offset + 8..offset + 12] != b"WAVE" {
        return false;
    }
    let wav = &body[offset..end];
    let mut index = 12usize;
    let mut rate = None;
    let mut pcm = None;
    while index + 8 <= wav.len() {
        let id = &wav[index..index + 4];
        let length = u32::from_le_bytes(wav[index + 4..index + 8].try_into().unwrap_or([0; 4])) as usize;
        index += 8;
        if index + length > wav.len() {
            return false;
        }
        let chunk = &wav[index..index + length];
        if id == b"fmt " {
            if chunk.len() < 16 {
                return false;
            }
            rate = Some(u32::from_le_bytes(chunk[4..8].try_into().unwrap()));
        } else if id == b"data" {
            pcm = Some(chunk);
        }
        index += length + (length % 2);
    }
    let Some(pcm) = pcm else { return false };
    if rate != Some(16000) {
        return false;
    }
    let mut needle = Vec::with_capacity(marker.len() + tone.len() * 11);
    needle.extend_from_slice(marker);
    for _ in 0..11 {
        needle.extend_from_slice(tone);
    }
    !needle.is_empty() && pcm.windows(needle.len()).any(|window| window == needle.as_slice())
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
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind peer: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer addr: {error}")).port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sink = requests.clone();
        let wav = silence_wav();
        let recognized = br#"{"text": "Interrupted question."}"#.to_vec();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
                let Some((path, body)) = read_request(&mut stream) else { continue };
                if let Ok(mut guard) = sink.lock() {
                    guard.push(body);
                }
                let response_body = if path == "/speak" { wav.clone() } else { recognized.clone() };
                let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", response_body.len());
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&response_body);
                let _ = stream.shutdown(Shutdown::Both);
            }
        });
        Self { port, requests }
    }

    fn len(&self) -> usize {
        self.requests.lock().unwrap_or_else(|error| error.into_inner()).len()
    }

    fn last_body(&self) -> Vec<u8> {
        self.requests.lock().unwrap_or_else(|error| error.into_inner()).last().cloned().unwrap_or_else(|| panic!("no HTTP request"))
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
    text.lines().next().and_then(|line| line.split_whitespace().nth(1)).unwrap_or("/").to_string()
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

fn silence_wav() -> Vec<u8> {
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

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "interruption"))
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

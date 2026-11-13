//! Microphone settings navigation and explicit conversation admission through the shared core.
mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use twin::{b, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner execution through current public settings/navigation/core events, actual PCM frames, local files and recognition/synthesis HTTP peers across all four model formats. Native permission observation and playback callbacks are controlled inputs; physical Android permission, visual layout, microphone and speaker acceptance remains separate.";
const REPORT: &str = "output/checks/voice_settings.json";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path(), &peer);
    controller.finish();
    peer.stop();
    write_report(&passed);
}

fn run(controller: &mut Controller, root: &str, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let initial = state(&resumed).clone();

    let d = controller.event(Some(&initial), "config", obj(vec![]), root);
    // The leg compares `set(buttons(d)) == expected`, so the modal's button order is not
    // part of the assertion; only the exact set of events is.
    let mut sections = button_keys(&d);
    sections.sort();
    sections.dedup();
    keep(
        &mut passed,
        "app settings retains every prior section and exposes microphone settings",
        sections
            == [
                "appearance",
                "connections",
                "harness_settings",
                "observability",
                "permissions",
                "resources_edit",
                "services",
                "storage_settings",
                "voice_settings",
            ],
    );

    for action in ["chat_actions", "media_menu", "services"] {
        let d = controller.event(Some(&initial), action, obj(vec![]), root);
        keep(
            &mut passed,
            &format!("{action} exposes the same microphone settings route"),
            button_keys(&d).contains(&"voice_settings".to_string()),
        );
    }

    let d = controller.event(Some(&initial), "voice_settings", obj(vec![]), root);
    let voice_fields = fields(&d);
    keep(
        &mut passed,
        "microphone settings are available with no AI or speech service",
        effect_is_null(&d)
            && text_is(state(&d), "dialog", "voice_settings")
            && !truthy(state(&d).get("voice_consent")),
    );
    keep(
        &mut passed,
        "settings disclose the actual recognition boundary",
        field_text(&voice_fields, "voice_engine").contains("no recognition model is bundled"),
    );
    keep(
        &mut passed,
        "settings show permission as unmeasured until platform observation",
        field_text(&voice_fields, "microphone_permission").contains("checked by Android"),
    );
    keep(
        &mut passed,
        "settings expose all saved capture controls",
        ["voice_silence_ms", "voice_threshold", "voice_max_seconds", "voice_background", "voice_interruption"]
            .iter()
            .all(|key| voice_fields.iter().any(|(name, _)| name == *key)),
    );

    let d = controller.event(
        Some(state(&d)),
        "voice_settings_save",
        obj(vec![
            ("voice_silence_ms".into(), s("300")),
            ("voice_threshold".into(), s("700")),
            ("voice_max_seconds".into(), s("5")),
            ("voice_background".into(), b(true)),
            ("voice_interruption".into(), b(false)),
        ]),
        root,
    );
    let cfg = voice_settings(&d);
    keep(
        &mut passed,
        "saving preferences persists real values without opening microphone or network",
        cfg == Some(cfg_value(300, 700, 5, true, false))
            && effect_is_null(&d)
            && !truthy(state(&d).get("voice_open"))
            && peer.requests().is_empty(),
    );

    let d = controller.event(Some(state(&d)), "voice_permissions", obj(vec![]), root);
    keep(
        &mut passed,
        "permission control opens only the app permission surface",
        effect(&d).is_some_and(|value| *value == obj(vec![("kind".into(), s("microphone_permissions"))]))
            && !truthy(state(&d).get("voice_consent")),
    );

    let d = controller.event(
        Some(state(&d)),
        "phone_access_status",
        obj(vec![("device".into(), obj(vec![("microphone_granted".into(), b(false))]))]),
        root,
    );
    keep(
        &mut passed,
        "platform denial is shown without claiming a missing microphone",
        field_text(&fields(&d), "microphone_permission").contains("not allowed"),
    );

    let d = controller.event(
        Some(state(&d)),
        "phone_access_status",
        obj(vec![("device".into(), obj(vec![("microphone_granted".into(), b(true))]))]),
        root,
    );
    keep(
        &mut passed,
        "platform permission observation reaches its visible control",
        field_text(&fields(&d), "microphone_permission").contains("is allowed"),
    );

    let d = controller.event(Some(state(&d)), "close", obj(vec![]), root);
    let d = controller.event(Some(state(&d)), "voice_settings", obj(vec![]), root);
    keep(
        &mut passed,
        "reopening settings retains saved capture values",
        field_text(&fields(&d), "voice_threshold") == "700"
            && field_text(&fields(&d), "voice_background") == "true",
    );
    let saved = state(&d).get("voice_settings").cloned();

    let invalid = [
        vec![("voice_silence_ms".into(), s("299"))],
        vec![("voice_threshold".into(), s("5001"))],
        vec![("voice_max_seconds".into(), s("31"))],
        vec![("voice_threshold".into(), s("not-a-number"))],
        vec![("voice_threshold".into(), b(true))],
        vec![("voice_threshold".into(), s(""))],
        vec![("voice_background".into(), s("true"))],
    ];
    for inputs in invalid {
        let d = controller.event(Some(state(&d)), "voice_settings", obj(vec![]), root);
        let d = controller.event(Some(state(&d)), "voice_settings_save", obj(inputs.clone()), root);
        keep(
            &mut passed,
            &format!("invalid preferences preserve the saved configuration {}", python_dict(&inputs)),
            effect_is_null(&d)
                && state(&d).get("voice_settings") == saved.as_ref()
                && truthy(state(&d).get("notice_error")),
        );
    }

    let d = controller.event(Some(&initial), "voice_settings_save", obj(vec![]), root);
    keep(
        &mut passed,
        "saving outside its settings dialog cannot change preferences",
        effect_is_null(&d) && !truthy(state(&d).get("voice_settings")),
    );

    let d = controller.event(Some(&initial), "voice_settings", obj(vec![]), root);
    let d = controller.event(
        Some(state(&d)),
        "voice_settings_save",
        obj(vec![
            ("voice_silence_ms".into(), n("600")),
            ("voice_threshold".into(), n("900")),
            ("voice_max_seconds".into(), n("10")),
        ]),
        root,
    );
    let cfg = voice_settings(&d);
    keep(
        &mut passed,
        "typed integer settings save their actual values",
        effect_is_null(&d)
            && cfg.as_ref().is_some_and(|value| {
                value.get("threshold") == Some(&n("900"))
                    && value.get("silence_ms") == Some(&n("600"))
                    && value.get("max_seconds") == Some(&n("10"))
            }),
    );

    for fmt in ["chat", "local", "messages", "content"] {
        let start = configured(controller, peer, fmt, root);
        let mut d = controller.event(Some(&start), "voice_settings", obj(vec![]), root);
        d = controller.event(
            Some(state(&d)),
            "voice_settings_save",
            obj(vec![
                ("voice_silence_ms".into(), s("300")),
                ("voice_threshold".into(), s("700")),
                ("voice_max_seconds".into(), s("5")),
                ("voice_background".into(), b(true)),
                ("voice_interruption".into(), b(false)),
            ]),
            root,
        );
        let request_count = peer.requests().len();
        d = controller.event(Some(state(&d)), "voice_open_review", obj(vec![]), root);
        keep(
            &mut passed,
            &format!("{fmt} open-microphone button reaches an explicit review without capture"),
            effect_is_null(&d)
                && field_text(&fields(&d), "voice_open") == "true"
                && peer.requests().len() == request_count,
        );
        d = controller.event(Some(state(&d)), "voice_consent", obj(vec![]), root);
        let capture = effect(&d).cloned().unwrap_or_else(|| panic!("{fmt} voice consent had no effect"));
        keep(
            &mut passed,
            &format!("{fmt} review starts actual capture with saved preferences and background choice"),
            capture.get("kind") == Some(&s("voice_listen"))
                && capture.get("voice_background") == Some(&b(true))
                && voice_settings(&d) == Some(cfg_value(300, 700, 5, true, false)),
        );

        d = controller.event(Some(state(&d)), "voice_settings", obj(vec![]), root);
        keep(
            &mut passed,
            &format!("{fmt} opening settings retires only its voice session"),
            !truthy(state(&d).get("voice_open"))
                && effect(&d).is_some_and(|value| value.get("voice_only") == Some(&b(true)))
                && text_is(state(&d), "dialog", "voice_settings"),
        );
        let retired_state = state(&d).clone();
        let late = controller.event(
            Some(&retired_state),
            "voice_audio_start",
            obj(vec![
                ("voice_session".into(), capture.get("voice_session").cloned().unwrap()),
                ("voice_audio_ticket".into(), capture.get("voice_audio_ticket").cloned().unwrap()),
            ]),
            root,
        );
        keep(
            &mut passed,
            &format!("{fmt} old microphone callback cannot rearm capture after settings"),
            state(&late) == &retired_state && effect_is_null(&late),
        );

        d = controller.event(Some(state(&d)), "voice_review", obj(vec![]), root);
        keep(
            &mut passed,
            &format!("{fmt} manual review does not silently select background capture"),
            field_text(&fields(&d), "voice_open") == "false"
                && field_text(&fields(&d), "voice_background") == "false",
        );
        d = controller.event(Some(state(&d)), "close", obj(vec![]), root);
        d = controller.event(Some(state(&d)), "voice_open_review", obj(vec![]), root);
        keep(
            &mut passed,
            &format!("{fmt} cancelled manual review preserves saved background preference"),
            field_text(&fields(&d), "voice_background") == "true",
        );
        d = controller.event(Some(state(&d)), "voice_consent", obj(vec![]), root);
        d = utterance(controller, d, root);
        d = exchange(controller, d, root);
        let ticket = state(&d).get("model_expected").cloned().unwrap();
        d = controller.event(Some(state(&d)), "voice_settings", obj(vec![]), root);
        keep(
            &mut passed,
            &format!("{fmt} opening settings during a primary reply preserves that model owner"),
            truthy(state(&d).get("busy"))
                && state(&d).get("model_expected") == Some(&ticket)
                && !truthy(state(&d).get("voice_open")),
        );
        d = controller.event(
            Some(state(&d)),
            "voice_settings_save",
            obj(vec![("voice_threshold".into(), s("800"))]),
            root,
        );
        keep(
            &mut passed,
            &format!("{fmt} saving preferences preserves the in-flight primary reply"),
            truthy(state(&d).get("busy")) && state(&d).get("model_expected") == Some(&ticket),
        );
        d = model(controller, d, fmt, "Primary text retained while editing microphone settings.", root);
        keep(
            &mut passed,
            &format!("{fmt} primary reply completes without starting unreviewed speech"),
            effect_is_null(&d)
                && !truthy(state(&d).get("busy"))
                && chats_contain(&d, "Primary text retained"),
        );

        d = controller.event(Some(state(&d)), "voice_open_review", obj(vec![]), root);
        d = controller.event(Some(state(&d)), "voice_consent", obj(vec![]), root);
        let session = effect(&d).and_then(|value| value.get("voice_session").cloned()).unwrap();
        let lease = effect(&d).and_then(|value| value.get("voice_lease_ms").cloned()).unwrap();
        d = utterance(controller, d, root);
        keep(
            &mut passed,
            &format!("{fmt} configured recognition receives real microphone data"),
            effect(&d)
                .and_then(|value| value.get("purpose"))
                .map(Value::str_)
                .is_some_and(|purpose| purpose.starts_with("service_request:")),
        );
        d = exchange(controller, d, root);
        keep(
            &mut passed,
            &format!("{fmt} recognized text reaches ordinary conversation"),
            effect(&d).and_then(|value| value.get("purpose")).cloned() == Some(s("model"))
                && effect(&d).and_then(|value| value.get("wire")).is_some_and(|value| value.str_().contains("Question from the microphone.")),
        );
        d = model(controller, d, fmt, "Spoken answer through configured services.", root);
        d = exchange(controller, d, root);
        let media = effect(&d).cloned().unwrap();
        keep(
            &mut passed,
            &format!("{fmt} real synthesis result reaches owned playback"),
            media.get("kind") == Some(&s("media_preview"))
                && media.get("media_kind") == Some(&s("audio"))
                && media.get("voice_session") == Some(&session),
        );
        let owner = obj(vec![
            ("voice_session".into(), session.clone()),
            ("voice_playback_ticket".into(), media.get("voice_playback_ticket").cloned().unwrap()),
        ]);
        d = controller.event(Some(state(&d)), "voice_playback_start", owner.clone(), root);
        d = controller.event(
            Some(state(&d)),
            "voice_playback_result",
            obj(vec![
                ("voice_session".into(), session.clone()),
                ("voice_playback_ticket".into(), media.get("voice_playback_ticket").cloned().unwrap()),
                ("completed".into(), b(true)),
            ]),
            root,
        );
        let next = effect(&d).cloned().unwrap();
        keep(
            &mut passed,
            &format!("{fmt} completed spoken reply returns to the same bounded listening session"),
            next.get("kind") == Some(&s("voice_listen"))
                && next.get("voice_background").is_some_and(twin::truthy)
                && next.get("voice_session") == Some(&session)
                && next
                    .get("voice_lease_ms")
                    .is_some_and(|value| {
                        let remaining = number(value);
                        let limit = number(&lease);
                        remaining > 0 && remaining <= limit
                    }),
        );
        d = controller.event(Some(state(&d)), "voice_settings", obj(vec![]), root);
        keep(
            &mut passed,
            &format!("{fmt} returning to settings stops the new listener without removing text"),
            !truthy(state(&d).get("voice_open")) && chats_contain(&d, "Spoken answer through configured services."),
        );
    }

    let start = configured(controller, peer, "chat", root);
    let mut d = controller.event(Some(&start), "send", obj(vec![("draft".into(), s("Prepare a reply for manual speech."))]), root);
    d = model(controller, d, "chat", "Manual speech remains independently available.", root);
    d = controller.event(Some(state(&d)), "voice_speak", obj(vec![]), root);
    let pending = state(&d).get("pending").cloned();
    let phase = state(&d).get("voice_phase").cloned();
    d = controller.event(Some(state(&d)), "voice_settings", obj(vec![]), root);
    d = controller.event(
        Some(state(&d)),
        "voice_settings_save",
        obj(vec![("voice_threshold".into(), n("900"))]),
        root,
    );
    keep(
        &mut passed,
        "microphone preferences preserve a separate manual speech proposal",
        state(&d).get("pending") == pending.as_ref()
            && state(&d).get("voice_phase") == phase.as_ref()
            && effect_is_null(&d),
    );
    d = controller.event(Some(state(&d)), "review", obj(vec![]), root);
    d = controller.event(Some(state(&d)), "approve", obj(vec![]), root);
    d = exchange(controller, d, root);
    keep(
        &mut passed,
        "preserved manual speech proposal still completes its actual output",
        effect(&d).is_some_and(|value| value.get("kind") == Some(&s("media_preview")))
            && effect(&d).is_some_and(|value| value.get("media_kind") == Some(&s("audio")))
            && !truthy(state(&d).get("voice_open")),
    );

    let requests = peer.requests();
    keep(
        &mut passed,
        "recognition and synthesis used the actual configured HTTP peers",
        requests.iter().any(|(path, data)| path == "/recognize" && data.windows(4).any(|window| window == b"RIFF"))
            && requests.iter().any(|(path, data)| {
                path == "/speak"
                    && Value::parse(&String::from_utf8_lossy(data)).get("text").map(Value::str_)
                        == Some("Spoken answer through configured services.")
            }),
    );

    passed
}

fn configured(controller: &mut Controller, peer: &Peer, fmt: &str, root: &str) -> Value {
    let d = controller.event(None, "resume", obj(vec![]), root);
    let d = controller.event(Some(state(&d)), "service_connections", obj(vec![]), root);
    let config = obj(vec![
        ("version".into(), n("1")),
        ("hosts".into(), Value::Arr(vec![obj(vec![
            ("id".into(), s("speech")),
            ("url".into(), s(&format!("http://127.0.0.1:{}", peer.port()))),
            ("plain".into(), b(true)),
            ("enabled".into(), b(true)),
        ])])),
        ("routes".into(), Value::Arr(vec![
            obj(vec![
                ("id".into(), s("recognition")),
                ("host".into(), s("speech")),
                ("capability".into(), s("speech_recognition")),
                ("path".into(), s("/recognize")),
                ("method".into(), s("POST")),
                ("request_format".into(), s("multipart")),
                ("file_field".into(), s("audio")),
                ("request".into(), obj(vec![])),
                ("response".into(), s("json")),
                ("response_path".into(), s("/text")),
                ("enabled".into(), b(true)),
            ]),
            obj(vec![
                ("id".into(), s("synthesis")),
                ("host".into(), s("speech")),
                ("capability".into(), s("speech_synthesis")),
                ("path".into(), s("/speak")),
                ("method".into(), s("POST")),
                ("request".into(), obj(vec![("text".into(), obj(vec![("$input".into(), s("/text"))]))])),
                ("response".into(), s("binary")),
                ("artifact_kind".into(), s("audio")),
                ("enabled".into(), b(true)),
            ]),
        ])),
        ("defaults".into(), obj(vec![
            ("speech_recognition".into(), obj(vec![("routes".into(), Value::Arr(vec![s("recognition")]))])),
            ("speech_synthesis".into(), obj(vec![("routes".into(), Value::Arr(vec![s("synthesis")]))])),
        ])),
    ]);
    let d = controller.event(
        Some(state(&d)),
        "service_save",
        obj(vec![("service_config".into(), s(&twin::pretty(&config)))]),
        root,
    );
    let mut result = state(&d).clone();
    put(
        &mut result,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Configured primary")),
            ("endpoint".into(), s("http://model.test/model")),
            ("protocol".into(), s(fmt)),
            ("model".into(), s("controlled")),
            ("plain".into(), b(true)),
            ("stream".into(), b(false)),
        ])]),
    );
    put(
        &mut result,
        "agents",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("General")),
            ("tools".into(), s("read,service")),
            ("investigation_preferences".into(), b(false)),
        ])]),
    );
    result
}

fn utterance(controller: &mut Controller, mut d: Value, root: &str) -> Value {
    let e = effect(&d).cloned().unwrap();
    let owner = obj(vec![
        ("voice_session".into(), e.get("voice_session").cloned().unwrap()),
        ("voice_audio_ticket".into(), e.get("voice_audio_ticket").cloned().unwrap()),
    ]);
    d = controller.event(Some(state(&d)), "voice_audio_start", owner.clone(), root);
    let quiet = vec![0u8; 640];
    let mut tone = Vec::with_capacity(640);
    for index in 0..320 {
        tone.extend_from_slice(&((if index % 2 == 0 { 3000i16 } else { -3000i16 }).to_le_bytes()));
    }
    for frame in [&quiet; 20].into_iter().chain([&tone; 20].into_iter()).chain([&quiet; 40].into_iter()) {
        let envelope = obj(vec![
            ("op".into(), s("voice_audio_feed")),
            ("voice_session".into(), owner.get("voice_session").cloned().unwrap()),
            ("voice_audio_ticket".into(), owner.get("voice_audio_ticket").cloned().unwrap()),
            ("data".into(), s(&twin::b64_encode(frame))),
        ]);
        let value = controller.op(&envelope);
        if value.get("ready") == Some(&b(true)) {
            let result = obj(vec![
                ("voice_session".into(), owner.get("voice_session").cloned().unwrap()),
                ("voice_audio_ticket".into(), owner.get("voice_audio_ticket").cloned().unwrap()),
                ("receipt".into(), value.get("receipt").cloned().unwrap()),
            ]);
            return controller.event(Some(state(&d)), "voice_audio_result", result, root);
        }
    }
    panic!("Actual microphone frames did not produce an utterance");
}

fn exchange(controller: &mut Controller, d: Value, root: &str) -> Value {
    // The leg's exchange() drives verification/transport.py while the socket is open: the
    // subject's bounded request-chunk and response-stream ops run over the child pipe
    // between the request write and the reply read, so both sides stay moving.
    let e = effect(&d).cloned().unwrap_or_else(|| panic!("exchange: the event carried no effect"));
    let wire = match e.get("wire_base64") {
        Some(Value::Str(encoded)) => twin::b64_decode(encoded),
        Some(other) => panic!("exchange: wire_base64 has the wrong type: {}", other.to_string()),
        None => match e.get("wire") {
            Some(Value::Str(wire)) => wire.as_bytes().to_vec(),
            Some(other) => panic!("exchange: wire has the wrong type: {}", other.to_string()),
            None => panic!("exchange: the effect carries neither wire_base64 nor wire"),
        },
    };
    let host = match e.get("host") {
        Some(Value::Str(host)) => host.clone(),
        Some(other) => panic!("exchange: host has the wrong type: {}", other.to_string()),
        None => panic!("exchange: the effect carries no host"),
    };
    let port = match e.get("port") {
        Some(value) => {
            let port = number(value);
            if !(0..=u16::MAX as i64).contains(&port) {
                panic!("exchange: port out of range: {port}");
            }
            port as u16
        }
        None => panic!("exchange: the effect carries no port"),
    };
    let purpose = match e.get("purpose") {
        Some(Value::Str(purpose)) => purpose.clone(),
        Some(other) => panic!("exchange: purpose has the wrong type: {}", other.to_string()),
        None => panic!("exchange: the effect carries no purpose"),
    };

    let mut stream = TcpStream::connect((host.as_str(), port))
        .unwrap_or_else(|error| panic!("exchange: could not reach {host}:{port}: {error}"));
    stream.write_all(&wire).unwrap_or_else(|error| panic!("exchange: could not write the request: {error}"));

    if e.get("media_upload").is_some_and(twin::truthy) {
        let ticket = match e.get("media_upload") {
            Some(Value::Str(ticket)) => ticket.clone(),
            Some(other) => panic!("exchange: media_upload has the wrong type: {}", other.to_string()),
            None => unreachable!(),
        };
        let upload_bytes = match e.get("upload_bytes") {
            Some(value) => {
                let total = number(value);
                if total < 0 {
                    panic!("exchange: upload_bytes is negative: {total}");
                }
                total as usize
            }
            None => panic!("exchange: media_upload has no upload_bytes"),
        };
        upload(controller, &purpose, &ticket, upload_bytes, &mut stream);
    }

    let reply = if e.get("media_download").is_some_and(twin::truthy) {
        let ticket = match e.get("media_download") {
            Some(Value::Str(ticket)) => ticket.clone(),
            Some(other) => panic!("exchange: media_download has the wrong type: {}", other.to_string()),
            None => unreachable!(),
        };
        download_reply(controller, &purpose, &ticket, &mut stream)
    } else {
        let mut data = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let count = stream.read(&mut buffer).unwrap_or_else(|error| panic!("exchange: peer read: {error}"));
            if count == 0 {
                break;
            }
            data.extend_from_slice(&buffer[..count]);
        }
        obj(vec![("data".into(), s(&twin::b64_encode(&data)))])
    };

    let mut inputs = vec![("purpose".into(), s(&purpose))];
    if let Some(value) = reply.get("data") {
        inputs.push(("data".into(), value.clone()));
    } else if let Some(value) = reply.get("media_receipt") {
        inputs.push(("media_receipt".into(), value.clone()));
    } else {
        panic!("exchange: the peer reply is neither data nor media_receipt: {}", reply.to_string());
    }

    controller.event(Some(state(&d)), "network_result", obj(inputs), root)
}

fn upload(controller: &mut Controller, purpose: &str, ticket: &str, upload_bytes: usize, stream: &mut TcpStream) {
    let mut sent = 0usize;
    loop {
        let part = controller.op(&obj(vec![
            ("purpose".into(), s(purpose)),
            ("ticket".into(), s(ticket)),
            ("op".into(), s("media_upload_read")),
        ]));
        let encoded = match part.get("data") {
            Some(Value::Str(data)) => data.clone(),
            Some(other) => panic!("upload: media_upload_read data has the wrong type: {}", other.to_string()),
            None => panic!("upload: media_upload_read returned no data"),
        };
        let data = twin::b64_decode(&encoded);
        if data.len() > 8192 {
            panic!("upload: media_upload_read part exceeds the 8192-byte bound: {}", data.len());
        }
        sent += data.len();
        if sent > upload_bytes {
            panic!("upload: streamed {sent} bytes past upload_bytes {upload_bytes}");
        }
        stream.write_all(&data).unwrap_or_else(|error| panic!("upload: could not write a request chunk: {error}"));
        let done = match part.get("done") {
            Some(Value::Bool(done)) => *done,
            Some(other) => panic!("upload: media_upload_read done has the wrong type: {}", other.to_string()),
            None => panic!("upload: media_upload_read returned no done flag"),
        };
        if done {
            if sent != upload_bytes {
                panic!("upload: sent {sent} bytes but upload_bytes is {upload_bytes}");
            }
            break;
        }
        if data.is_empty() {
            panic!("upload: a non-final media_upload_read part was empty");
        }
    }
    controller.op(&obj(vec![
        ("purpose".into(), s(purpose)),
        ("ticket".into(), s(ticket)),
        ("op".into(), s("media_upload_cancel")),
    ]));
}

fn download_reply(controller: &mut Controller, purpose: &str, ticket: &str, stream: &mut TcpStream) -> Value {
    let mut ready = false;
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer).unwrap_or_else(|error| panic!("download: peer read: {error}"));
        if count == 0 {
            break;
        }
        let value = controller.op(&obj(vec![
            ("purpose".into(), s(purpose)),
            ("ticket".into(), s(ticket)),
            ("op".into(), s("media_stream_feed")),
            ("data".into(), s(&twin::b64_encode(&buffer[..count]))),
        ]));
        ready = match value.get("ready") {
            Some(Value::Bool(ready)) => *ready,
            Some(other) => panic!("download: media_stream_feed ready has the wrong type: {}", other.to_string()),
            None => panic!("download: media_stream_feed returned no ready flag"),
        };
        if ready {
            break;
        }
    }
    let finished = controller.op(&obj(vec![
        ("purpose".into(), s(purpose)),
        ("ticket".into(), s(ticket)),
        ("op".into(), s("media_stream_finish")),
        ("eof".into(), b(!ready)),
    ]));
    // The leg wraps the op result itself: dict(media_receipt=op(...)). The finish op
    // returns the receipt value directly, not an object holding that key.
    let media_receipt = finished;
    controller.op(&obj(vec![
        ("purpose".into(), s(purpose)),
        ("ticket".into(), s(ticket)),
        ("op".into(), s("media_stream_cancel")),
    ]));
    obj(vec![("media_receipt".into(), media_receipt)])
}

fn model(controller: &mut Controller, d: Value, fmt: &str, text: &str, root: &str) -> Value {
    // The leg selects one of these bodies by format name; each body is the whole response
    // document, not a document keyed by that name.
    let value = match fmt {
        "chat" => obj(vec![("choices".into(), Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("finish_reason".into(), s("stop")),
        ])]))]),
        "local" => obj(vec![
            ("message".into(), obj(vec![("content".into(), s(text))])),
            ("done".into(), b(true)),
        ]),
        "messages" => obj(vec![
            ("content".into(), Value::Arr(vec![obj(vec![
                ("type".into(), s("text")),
                ("text".into(), s(text)),
            ])])),
            ("stop_reason".into(), s("end_turn")),
        ]),
        _ => obj(vec![("candidates".into(), Value::Arr(vec![obj(vec![
            ("content".into(), obj(vec![("parts".into(), Value::Arr(vec![obj(vec![
                ("text".into(), s(text)),
            ])]))])),
            ("finishReason".into(), s("STOP")),
        ])]))]),
    };
    let body = twin::pretty(&value).into_bytes();
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(&body);
    let inputs = obj(vec![
        ("purpose".into(), s("model")),
        ("model_ticket".into(), state(&d).get("model_expected").cloned().unwrap()),
        ("data".into(), s(&twin::b64_encode(&wire))),
    ]);
    controller.event(Some(state(&d)), "network_result", inputs, root)
}


fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}



fn cfg_value(silence: u64, threshold: u64, max_seconds: u64, background: bool, interruption: bool) -> Value {
    obj(vec![
        ("silence_ms".into(), n(&silence.to_string())),
        ("threshold".into(), n(&threshold.to_string())),
        ("max_seconds".into(), n(&max_seconds.to_string())),
        ("background".into(), b(background)),
        ("interruption".into(), b(interruption)),
    ])
}

fn voice_settings(d: &Value) -> Option<Value> {
    state(d).get("voice_settings").cloned()
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn effect(value: &Value) -> Option<&Value> {
    value.get("effect")
}

fn effect_is_null(value: &Value) -> bool {
    matches!(effect(value), Some(Value::Null))
}

// The leg keeps each modal field whole (`{f['key']: f`) and reads `['value']` at the
// comparison site, so the value type is only required where a check needs a string.
fn fields(d: &Value) -> Vec<(String, Value)> {
    d.get("ui")
        .and_then(|ui| ui.get("modal"))
        .and_then(|modal| modal.get("fields"))
        .map(|fields| fields.arr().clone())
        .unwrap_or_default()
        .iter()
        .filter_map(|field| {
            Some((
                field.get("key")?.str_().to_string(),
                field.get("value")?.clone(),
            ))
        })
        .collect()
}

fn field_text<'a>(fields: &'a [(String, Value)], key: &str) -> &'a str {
    let Some((_, value)) = fields.iter().find(|(name, _)| name == key) else {
        panic!("missing field {key}");
    };
    match value {
        Value::Str(text) => text.as_str(),
        other => panic!("field {key} is not a string: {}", other.to_string()),
    }
}

fn button_keys(d: &Value) -> Vec<String> {
    d.get("ui")
        .and_then(|ui| ui.get("modal"))
        .and_then(|modal| modal.get("buttons"))
        .map(|buttons| buttons.arr().clone())
        .unwrap_or_default()
        .iter()
        .filter_map(|button| button.get("event").map(Value::str_).map(str::to_string))
        .collect()
}

fn truthy(value: Option<&Value>) -> bool {
    value.is_some_and(twin::truthy)
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn chats_contain(d: &Value, text: &str) -> bool {
    state(d)
        .get("chats")
        .map(Value::arr)
        .is_some_and(|chats| {
            chats.iter().any(|chat| {
                chat.get("messages")
                    .map(Value::arr)
                    .is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message.get("content").map(Value::str_).is_some_and(|content| content.contains(text))
                        })
                    })
            })
        })
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

fn number(value: &Value) -> i64 {
    let Value::Num(text) = value else {
        panic!("expected a number, got {}", value.to_string());
    };
    text.parse::<i64>().unwrap_or_else(|error| panic!("expected a number, got {text}: {error}"))
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn python_dict(inputs: &[(String, Value)]) -> String {
    let body = inputs
        .iter()
        .map(|(key, value)| {
            let rendered = match value {
                Value::Str(text) => format!("'{text}'"),
                Value::Bool(flag) => if *flag { "True".to_string() } else { "False".to_string() },
                Value::Num(text) => text.clone(),
                _ => value.to_string(),
            };
            format!("'{key}': {rendered}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{body}}}")
}

fn write_report(passed: &[String]) {
    let record = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("voice_settings", &record, true);
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains(SCOPE) || !written.contains("\"passed\": 81") {
        panic!("{REPORT} does not contain the expected report");
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = root.join("output").join(format!("voice-settings-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("could not create {}: {error}", path.display()));
        Self(path)
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

struct Peer {
    listener: TcpListener,
    port: u16,
    requests: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    stop: Arc<Mutex<bool>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(Mutex::new(false));
        let thread_requests = Arc::clone(&requests);
        let thread_stop = Arc::clone(&stop);
        let listen = listener
            .try_clone()
            .unwrap_or_else(|error| panic!("could not clone the peer listener: {error}"));
        std::thread::spawn(move || {
            // The leg serves the peer with ThreadingHTTPServer, so a request the subject
            // keeps half-written must not stall the other connections.
            for connection in listen.incoming() {
                if *thread_stop.lock().unwrap() {
                    break;
                }
                let Ok(mut stream) = connection else {
                    continue;
                };
                let connection_requests = Arc::clone(&thread_requests);
                std::thread::spawn(move || {
                    let (path, body) = read_http_request(&mut stream);
                    let response_body = if path == "/speak" {
                        speech_audio()
                    } else {
                        br#"{"text":"Question from the microphone."}"#.to_vec()
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response_body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(&response_body);
                    connection_requests.lock().unwrap().push((path, body));
                });
            }
        });
        Self { listener, port, requests, stop }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn requests(&self) -> Vec<(String, Vec<u8>)> {
        self.requests.lock().unwrap().clone()
    }

    fn stop(&self) {
        *self.stop.lock().unwrap() = true;
        let _ = self.listener.set_nonblocking(true);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn speech_audio() -> Vec<u8> {
    // The leg synthesises the speech fixture with the wave module: mono 16-bit PCM at
    // 16 kHz, 1600 silent frames, so the subject sees a real WAVE container.
    let frames = vec![0u8; 3200];
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((frames.len() + 36) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&32000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    out.extend_from_slice(&frames);
    out
}

fn read_http_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap_or_else(|error| panic!("peer read: {error}"));
        if count == 0 {
            panic!("peer closed before request headers");
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
            break index;
        }
    };
    let header = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let path = request_line.split_whitespace().nth(1).unwrap_or_default().to_string();
    let length = lines
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    while bytes.len() < header_end + 4 + length {
        let count = stream.read(&mut buffer).unwrap_or_else(|error| panic!("peer read: {error}"));
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    (path, bytes[header_end + 4..].to_vec())
}
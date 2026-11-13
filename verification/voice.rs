mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use twin::{b64_decode, b64_encode, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner HTTP/audio fixture orchestration through all four model formats. Capture callback is controlled; physical microphone and installed playback not exercised.";
const FORMATS: [&str; 4] = ["chat", "local", "messages", "content"];
const RECOGNIZED: &str = "Recognized fixture question.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root, "voice-");
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let peer = Peer::start();
    let audio = wav_fixture();
    let tone = tone_frame();
    let mut passed = Vec::new();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run(&mut controller, &peer, scratch.path(), &audio, &tone, &mut passed);
    }));
    controller.finish();
    if let Err(error) = result {
        resume_unwind(error);
    }
    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("voice", &body, true);
}

fn run(controller: &mut Controller, peer: &Peer, root: &str, audio: &[u8], tone: &[u8], passed: &mut Vec<String>) {
    for fmt in FORMATS {
        manual_turn(controller, peer, root, audio, fmt, passed);
    }
    stop_during_generation(controller, peer, root, passed);
    let done = recognition_keeps_draft(controller, peer, root, audio, passed);
    manual_speech(controller, root, &done, passed);
    for fmt in FORMATS {
        open_microphone(controller, peer, root, tone, fmt, passed);
    }
    suspension_ignores_late_failure(controller, peer, root, passed);
    missing_recognition(controller, peer, root, passed);
    suspension_preserves_model(controller, peer, root, tone, passed);
    playback_failure(controller, peer, root, tone, passed);
    microphone_failure(controller, peer, root, passed);
    settings_retire_microphone(controller, peer, root, passed);
    uncertain_recognition(controller, peer, root, tone, passed);
    explicit_listen_during_speech(controller, peer, root, tone, passed);
    for phase in ["playback_pending", "speaking"] {
        interrupt_phase(controller, peer, root, tone, phase, passed);
    }
    stop_pending_playback(controller, peer, root, tone, passed);
    speak_now_keeps_draft(controller, peer, root, tone, passed);
    stop_during_model(controller, peer, root, tone, passed);
    for fmt in FORMATS {
        speech_review(controller, peer, root, tone, fmt, passed);
    }
    for fmt in FORMATS {
        for heap in [16u64, 24, 32] {
            speech_admission(controller, peer, root, tone, fmt, heap, passed);
        }
        refused_recognition(controller, peer, root, tone, fmt, passed);
        separate_service_refusal(controller, peer, root, fmt, passed);
    }
    for fmt in FORMATS {
        for stage in ["proposal", "admitted"] {
            held_utterance(controller, peer, root, tone, fmt, stage, passed);
        }
    }
    for fmt in FORMATS {
        for refusal in ["text_bound", "storage_quota"] {
            preparation_refusal(controller, peer, tone, fmt, refusal, passed);
        }
    }
}

fn manual_turn(controller: &mut Controller, peer: &Peer, root: &str, audio: &[u8], fmt: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, fmt, root);
    let seen = peer.len();
    let d = event(controller, Some(&state), "voice_review", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} consent review discloses destinations without capturing"),
        effect_is_none(&d) && modal_has(&d, "Recognition") && peer.len() == seen,
    );
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} enabling voice alone never opens microphone"),
        effect_is_none(&d) && py_true(state_of(&d).get("voice_consent")),
    );
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_record", obj(vec![]), root);
    let effect = required_effect(&d);
    keep(
        passed,
        &format!("{fmt} recording begins only through explicit action"),
        kind_is(effect, "media_record"),
    );
    keep(
        passed,
        &format!("{fmt} capture is visible in quota-managed media storage"),
        capture_visible(effect, root),
    );
    let ticket = require_str(effect, "media_ticket");
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), s(&ticket)),
            ("name".into(), s("fixture.wav")),
            ("data".into(), s(&b64_encode(audio))),
        ]),
        root,
    );
    keep(
        passed,
        &format!("{fmt} consented capture starts actual recognition route"),
        purpose_starts(effect_of(&d), "service_request:"),
    );
    let d = transport(controller, &d, root);
    let model = required_effect(&d);
    keep(
        passed,
        &format!("{fmt} transcript becomes ordinary conversation turn"),
        purpose_is(model, "model") && wire_has(model, RECOGNIZED),
    );
    keep(
        passed,
        &format!("{fmt} exact selected audio reached recognition host"),
        contains_bytes(&peer.last_body(), audio),
    );
    let state = clone_state(&d);
    let d = reply(controller, &state, root, fmt, "Actual controlled reply.");
    keep(
        passed,
        &format!("{fmt} final answer starts configured synthesis"),
        purpose_starts(effect_of(&d), "service_request:"),
    );
    let d = transport(controller, &d, root);
    keep(
        passed,
        &format!("{fmt} synthesis receives actual answer text"),
        body_is_text(&peer.last_body(), "Actual controlled reply."),
    );
    let played = required_effect(&d);
    keep(
        passed,
        &format!("{fmt} actual returned audio reaches native playback effect"),
        kind_is(played, "media_preview") && field_is(played, "media_kind", "audio"),
    );
    keep(
        passed,
        &format!("{fmt} text reply remains in history"),
        history_has(state_of(&d), "Actual controlled reply."),
    );
    let before_stop = clone_state(&d);
    let stopped = event(controller, Some(&before_stop), "voice_stop", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} stop revokes consent without removing text"),
        !py_true(state_of(&stopped).get("voice_consent")) && kind_is(required_effect(&stopped), "media_playback_stop"),
    );
    // Resume the pre-stop state. The consent map was cleared by stop; the saved JSON still carries the old token.
    let restored = event(controller, Some(&before_stop), "resume", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} process recovery does not restore automatic voice"),
        !py_true(state_of(&restored).get("voice_consent")),
    );
}

fn stop_during_generation(controller: &mut Controller, peer: &Peer, root: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, "chat", root);
    let d = event(controller, Some(&state), "voice_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "send", obj(vec![("draft".into(), s("Manual text question."))]), root);
    let ticket = state_of(&d).get("model_expected").cloned().unwrap_or_else(|| panic!("missing model_expected"));
    let state = clone_state(&d);
    let stopped = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
    keep(
        passed,
        "stop during primary generation preserves primary owner",
        state_bool_true(state_of(&stopped), "busy")
            && state_of(&stopped).get("model_expected") == Some(&ticket)
            && kind_is(required_effect(&stopped), "media_playback_stop"),
    );
    let state = clone_state(&stopped);
    let done = reply(controller, &state, root, "chat", "Text remains usable.");
    keep(
        passed,
        "stopped voice still returns the primary text answer",
        effect_is_none(&done) && state_bool_false(state_of(&done), "busy"),
    );
}

fn recognition_keeps_draft(controller: &mut Controller, peer: &Peer, root: &str, audio: &[u8], passed: &mut Vec<String>) -> Value {
    let state = configured(controller, peer, "chat", root);
    let d = event(controller, Some(&state), "voice_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "voice_record",
        obj(vec![("draft".into(), s("Existing unsent draft."))]),
        root,
    );
    let ticket = require_str(required_effect(&d), "media_ticket");
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), s(&ticket)),
            ("name".into(), s("fixture.wav")),
            ("data".into(), s(&b64_encode(audio))),
        ]),
        root,
    );
    let done = transport(controller, &d, root);
    keep(
        passed,
        "recognition preserves and appends to existing draft for review",
        state_str(state_of(&done), "draft") == Some("Existing unsent draft.\nRecognized fixture question.") && effect_is_none(&done),
    );
    done
}

fn manual_speech(controller: &mut Controller, root: &str, done: &Value, passed: &mut Vec<String>) {
    let state = clone_state(done);
    let d = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "send", obj(vec![("draft".into(), s("Read this reply aloud."))]), root);
    let state = clone_state(&d);
    let d = reply(controller, &state, root, "chat", "Manual spoken reply.");
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_speak", obj(vec![]), root);
    keep(
        passed,
        "manual speech first uses ordinary request review",
        state_str(state_of(&d), "dialog") == Some("approval") && effect_is_none(&d),
    );
    let state = clone_state(&d);
    let approved = event(controller, Some(&state), "approve", obj(vec![]), root);
    let d = transport(controller, &approved, root);
    let played = required_effect(&d);
    keep(
        passed,
        "manual speech plays actual returned audio",
        kind_is(played, "media_preview") && field_is(played, "media_kind", "audio"),
    );
}

fn open_microphone(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, fmt, root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let effect = required_effect(&d);
    keep(
        passed,
        &format!("{fmt} open microphone starts only after explicit review"),
        kind_is(effect, "voice_listen") && state_str(state_of(&d), "voice_status") == Some("microphone_pending"),
    );
    let owner = mic_owner(effect);
    let session = require_str(&owner, "voice_session");
    let ticket = require_str(&owner, "voice_audio_ticket");
    let mut wrong_inputs = owner.clone();
    put(&mut wrong_inputs, "receipt", s("wrong"));
    let prior = clone_state(&d);
    let wrong = event(controller, Some(&prior), "voice_audio_result", wrong_inputs, root);
    keep(
        passed,
        &format!("{fmt} unowned utterance receipt is exact state no-op"),
        state_of(&wrong) == &prior,
    );
    let silence = vec![0u8; 640];
    for _ in 0..50 {
        let value = controller.op(&feed_envelope(&session, &ticket, &silence));
        if flag(&value, "ready") == Some(true) {
            panic!("{fmt} quiet microphone never creates an upload");
        }
    }
    keep(
        passed,
        &format!("{fmt} quiet microphone never creates an upload"),
        get_is_none(state_of(&d), "service_expected"),
    );
    let d = submit_utterance(controller, &d, root, tone);
    keep(
        passed,
        &format!("{fmt} utterance automatically starts selected recognition route"),
        state_str(state_of(&d), "voice_status") == Some("recognizing") && purpose_starts(effect_of(&d), "service_request:"),
    );
    let d = transport(controller, &d, root);
    keep(
        passed,
        &format!("{fmt} actual microphone PCM reaches recognizer as valid WAVE"),
        wave_contains_tone(&peer.last_body(), tone),
    );
    let model = required_effect(&d);
    keep(
        passed,
        &format!("{fmt} recognition enters the primary conversation"),
        state_str(state_of(&d), "voice_status") == Some("thinking") && purpose_is(model, "model"),
    );
    let state = clone_state(&d);
    let d = reply(controller, &state, root, fmt, "Open microphone reply.");
    keep(
        passed,
        &format!("{fmt} primary reply reaches configured speech output"),
        state_str(state_of(&d), "voice_status") == Some("synthesizing") && purpose_starts(effect_of(&d), "service_request:"),
    );
    let d = transport(controller, &d, root);
    let played = required_effect(&d);
    let play = play_owner(played);
    keep(
        passed,
        &format!("{fmt} speech output carries owned playback callback"),
        kind_is(played, "media_preview") && state_str(state_of(&d), "voice_status") == Some("playback_pending"),
    );
    let blocked = controller.send(&feed_envelope(&session, &ticket, tone));
    keep(
        passed,
        &format!("{fmt} microphone capture is retired while reply plays"),
        lacks_value(&blocked),
    );
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_playback_start", play.clone(), root);
    keep(
        passed,
        &format!("{fmt} speaking state follows actual playback start"),
        state_str(state_of(&d), "voice_status") == Some("speaking"),
    );
    let state = clone_state(&d);
    let duplicate = event(controller, Some(&state), "voice_playback_start", play.clone(), root);
    keep(
        passed,
        &format!("{fmt} duplicate playback start is exact state no-op"),
        state_of(&duplicate) == &state,
    );
    let state = clone_state(&d);
    let done = event(controller, Some(&state), "voice_playback_result", with_completed(&play, true), root);
    let restarted = required_effect(&done);
    keep(
        passed,
        &format!("{fmt} completed spoken reply automatically returns to microphone"),
        kind_is(restarted, "voice_listen") && restarted.get("voice_audio_ticket") != Some(&s(&ticket)),
    );
    let state = clone_state(&done);
    let late = event(controller, Some(&state), "voice_playback_result", with_completed(&play, true), root);
    keep(
        passed,
        &format!("{fmt} old playback completion cannot start a second listener"),
        state_of(&late) == &state,
    );
    let state = clone_state(&done);
    let stopped = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} stop ends open microphone without requiring a text request"),
        state_bool_false(state_of(&stopped), "voice_open") && is_null_field(state_of(&stopped), "voice_audio_expected"),
    );
}

fn suspension_ignores_late_failure(controller: &mut Controller, peer: &Peer, root: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, "chat", root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let owner = mic_owner(required_effect(&d));
    let state = clone_state(&d);
    let suspended = event(controller, Some(&state), "voice_suspend", obj(vec![]), root);
    let state = clone_state(&suspended);
    let late = event(controller, Some(&state), "voice_audio_failure", owner, root);
    keep(
        passed,
        "foreground suspension revokes microphone and ignores late native failure",
        state_bool_false(state_of(&suspended), "voice_open") && state_of(&late) == &state,
    );
}

fn missing_recognition(controller: &mut Controller, peer: &Peer, root: &str, passed: &mut Vec<String>) {
    let mut state = configured(controller, peer, "chat", root);
    drop_recognition(&mut state);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    keep(
        passed,
        "voice setup identifies missing recognition separately from speech output",
        modal_has(&d, "Speech recognition connection is not ready"),
    );
    let state = clone_state(&d);
    let refused = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    keep(
        passed,
        "missing recognition does not open microphone or disable text",
        effect_is_none(&refused) && py_true(state_of(&refused).get("notice_error")),
    );
    let state = clone_state(&refused);
    let sent = event(
        controller,
        Some(&state),
        "send",
        obj(vec![("draft".into(), s("Primary text remains available."))]),
        root,
    );
    keep(
        passed,
        "primary text still works with incomplete voice setup",
        purpose_is(required_effect(&sent), "model"),
    );
}

fn suspension_preserves_model(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], passed: &mut Vec<String>) {
    let d = open_and_recognize(controller, peer, root, tone, "chat");
    let primary = state_of(&d).get("model_expected").cloned().unwrap_or_else(|| panic!("missing model_expected"));
    let state = clone_state(&d);
    let paused = event(controller, Some(&state), "voice_suspend", obj(vec![]), root);
    keep(
        passed,
        "foreground suspension preserves the primary model owner",
        state_of(&paused).get("model_expected") == Some(&primary) && state_bool_true(state_of(&paused), "busy"),
    );
    let state = clone_state(&paused);
    let d = reply(controller, &state, root, "chat", "Reply after suspension.");
    keep(
        passed,
        "suspended voice retains answer without reopening microphone or speech service",
        effect_is_none(&d) && state_bool_false(state_of(&d), "voice_open"),
    );
}

fn playback_failure(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], passed: &mut Vec<String>) {
    let d = spoken_reply(controller, peer, root, tone, "chat", "Playback failure fixture.");
    let play = play_owner(required_effect(&d));
    let state = clone_state(&d);
    let failed = event(controller, Some(&state), "voice_playback_result", with_completed(&play, false), root);
    keep(
        passed,
        "failed playback pauses microphone without retrying generation",
        effect_is_none(&failed) && state_str(state_of(&failed), "voice_status") == Some("paused"),
    );
    let state = clone_state(&failed);
    let resumed = event(controller, Some(&state), "voice_listen", obj(vec![]), root);
    keep(
        passed,
        "explicit resume starts a new listener after playback failure",
        kind_is(required_effect(&resumed), "voice_listen"),
    );
    let state = clone_state(&resumed);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn microphone_failure(controller: &mut Controller, peer: &Peer, root: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, "chat", root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let owner = mic_owner(required_effect(&d));
    let session = require_str(&owner, "voice_session");
    let ticket = require_str(&owner, "voice_audio_ticket");
    let bad = controller.send(&feed_envelope(&session, &ticket, &vec![b'x'; 642]));
    keep(passed, "oversized microphone frames are refused without state mutation", lacks_value(&bad));
    let state = clone_state(&d);
    let failed = event(controller, Some(&state), "voice_audio_failure", owner, root);
    keep(
        passed,
        "microphone failure pauses only the voice session",
        state_str(state_of(&failed), "voice_status") == Some("paused") && effect_is_none(&failed),
    );
    let state = clone_state(&failed);
    let sent = event(
        controller,
        Some(&state),
        "send",
        obj(vec![("draft".into(), s("Text after microphone failure."))]),
        root,
    );
    keep(
        passed,
        "text remains usable after microphone failure",
        purpose_is(required_effect(&sent), "model") && state_bool_false(state_of(&sent), "voice_open"),
    );
    let state = clone_state(&sent);
    let _ = event(controller, Some(&state), "cancel", obj(vec![]), root);
}

fn settings_retire_microphone(controller: &mut Controller, peer: &Peer, root: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, "chat", root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let owner = mic_owner(required_effect(&d));
    let state = clone_state(&d);
    let changed = event(controller, Some(&state), "service_connections", obj(vec![]), root);
    let state = clone_state(&changed);
    let late = event(controller, Some(&state), "voice_audio_start", owner, root);
    keep(
        passed,
        "opening connection settings retires microphone ownership",
        state_bool_false(state_of(&changed), "voice_open") && state_of(&late) == &state,
    );
}

fn uncertain_recognition(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], passed: &mut Vec<String>) {
    let state = configured(controller, peer, "chat", root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let d = submit_utterance(controller, &d, root, tone);
    let purpose = require_str(required_effect(&d), "purpose");
    let state = clone_state(&d);
    let failed = event(
        controller,
        Some(&state),
        "failure",
        obj(vec![
            ("purpose".into(), s(&purpose)),
            ("sent".into(), Value::Bool(true)),
            ("error".into(), s("controlled recognition interruption")),
        ]),
        root,
    );
    keep(
        passed,
        "uncertain recognition failure pauses without replay or microphone restart",
        effect_is_none(&failed) && state_str(state_of(&failed), "voice_status") == Some("paused"),
    );
}

fn explicit_listen_during_speech(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], passed: &mut Vec<String>) {
    let d = spoken_reply(controller, peer, root, tone, "chat", "Reply interrupted by explicit listening.");
    let play = play_owner(required_effect(&d));
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_playback_start", play.clone(), root);
    let state = clone_state(&d);
    let resumed = event(controller, Some(&state), "voice_listen", obj(vec![]), root);
    keep(
        passed,
        "explicit listening during speech starts one new microphone owner",
        effect_of(&resumed).is_some() && kind_is(required_effect(&resumed), "voice_listen"),
    );
    let state = clone_state(&resumed);
    let late = event(controller, Some(&state), "voice_playback_result", with_completed(&play, true), root);
    keep(
        passed,
        "old speech completion cannot replace an explicitly resumed microphone",
        state_of(&late) == &state && effect_is_none(&late),
    );
    let state = clone_state(&resumed);
    let late = event(controller, Some(&state), "voice_playback_result", with_completed(&play, false), root);
    keep(
        passed,
        "old speech interruption cannot pause an explicitly resumed microphone",
        state_of(&late) == &state && effect_is_none(&late),
    );
    let state = clone_state(&resumed);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn interrupt_phase(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], phase: &str, passed: &mut Vec<String>) {
    let d = spoken(controller, peer, root, tone, "chat");
    let play = play_owner(required_effect(&d));
    let d = if phase == "speaking" {
        let state = clone_state(&d);
        event(controller, Some(&state), "voice_playback_start", play.clone(), root)
    } else {
        d
    };
    let mut wrong_play = play.clone();
    put(&mut wrong_play, "voice_session", s("unowned"));
    let state = clone_state(&d);
    let wrong = event(controller, Some(&state), "voice_interrupt", wrong_play, root);
    keep(
        passed,
        &format!("{phase} unowned interruption is an exact state no-op"),
        state_of(&wrong) == &state && effect_is_none(&wrong),
    );
    let state = clone_state(&d);
    let listening = event(controller, Some(&state), "voice_interrupt", play.clone(), root);
    keep(
        passed,
        &format!("{phase} owned Speak now starts exactly one microphone"),
        effect_of(&listening).is_some()
            && kind_is(required_effect(&listening), "voice_listen")
            && is_null_field(state_of(&listening), "voice_playback_expected"),
    );
    let state = clone_state(&listening);
    let duplicate = event(controller, Some(&state), "voice_interrupt", play.clone(), root);
    keep(
        passed,
        &format!("{phase} repeated Speak now cannot create another listener"),
        state_of(&duplicate) == &state && effect_is_none(&duplicate),
    );
    let state = clone_state(&listening);
    let late = event(controller, Some(&state), "voice_playback_result", with_completed(&play, true), root);
    keep(
        passed,
        &format!("{phase} interrupted speech completion cannot change the new listener"),
        state_of(&late) == &state && effect_is_none(&late),
    );
    let d = submit_utterance(controller, &listening, root, tone);
    let d = transport(controller, &d, root);
    keep(
        passed,
        &format!("{phase} interrupted conversation accepts actual new speech"),
        effect_of(&d).is_some() && purpose_is(required_effect(&d), "model"),
    );
    let state = clone_state(&d);
    let d = reply(controller, &state, root, "chat", "Reply to actual speech after interruption.");
    let d = transport(controller, &d, root);
    keep(
        passed,
        &format!("{phase} interrupted conversation can synthesize its next reply"),
        effect_of(&d).is_some() && kind_is(required_effect(&d), "media_preview"),
    );
    let state = clone_state(&d);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn stop_pending_playback(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], passed: &mut Vec<String>) {
    let d = spoken(controller, peer, root, tone, "chat");
    let play = play_owner(required_effect(&d));
    let state = clone_state(&d);
    let stopped = event(controller, Some(&state), "media_playback_stop", obj(vec![]), root);
    keep(
        passed,
        "Stop playback settles pending speech even before native player creation",
        kind_is(required_effect(&stopped), "media_playback_stop")
            && state_str(state_of(&stopped), "voice_status") == Some("paused")
            && is_null_field(state_of(&stopped), "voice_playback_expected"),
    );
    let state = clone_state(&stopped);
    let late = event(controller, Some(&state), "voice_playback_start", play, root);
    keep(
        passed,
        "stopped pending player cannot announce late playback",
        state_of(&late) == &state && effect_is_none(&late),
    );
    let state = clone_state(&stopped);
    let resumed = event(controller, Some(&state), "voice_listen", obj(vec![]), root);
    keep(
        passed,
        "explicit microphone resume remains available after stopping pending playback",
        effect_of(&resumed).is_some() && kind_is(required_effect(&resumed), "voice_listen"),
    );
    let state = clone_state(&resumed);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn speak_now_keeps_draft(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], passed: &mut Vec<String>) {
    let d = spoken(controller, peer, root, tone, "chat");
    let play = play_owner(required_effect(&d));
    let mut state = clone_state(&d);
    put(&mut state, "draft", s("Retain this unsent edit."));
    let blocked = event(controller, Some(&state), "voice_interrupt", play, root);
    keep(
        passed,
        "Speak now preserves an unsent draft and pauses without capturing",
        effect_is_none(&blocked)
            && state_str(state_of(&blocked), "draft") == Some("Retain this unsent edit.")
            && state_str(state_of(&blocked), "voice_status") == Some("paused")
            && py_true(state_of(&blocked).get("notice_error")),
    );
    let state = clone_state(&blocked);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn stop_during_model(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], passed: &mut Vec<String>) {
    let state = configured(controller, peer, "chat", root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let d = submit_utterance(controller, &d, root, tone);
    let d = transport(controller, &d, root);
    let ticket = state_of(&d).get("model_expected").cloned().unwrap_or_else(|| panic!("missing model_expected"));
    let state = clone_state(&d);
    let stopped = event(controller, Some(&state), "media_playback_stop", obj(vec![]), root);
    keep(
        passed,
        "Stop playback during model work preserves the primary request owner",
        kind_is(required_effect(&stopped), "media_playback_stop")
            && state_of(&stopped).get("model_expected") == Some(&ticket)
            && state_bool_true(state_of(&stopped), "busy"),
    );
    let state = clone_state(&stopped);
    let d = reply(controller, &state, root, "chat", "Primary answer still arrives.");
    keep(
        passed,
        "Stop playback during model work preserves configured speech output",
        effect_of(&d).is_some() && purpose_starts(effect_of(&d), "service_request:"),
    );
    let d = transport(controller, &d, root);
    let state = clone_state(&d);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn speech_review(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str, passed: &mut Vec<String>) {
    let d = spoken(controller, peer, root, tone, fmt);
    let play = play_owner(required_effect(&d));
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_playback_result", with_completed(&play, true), root);
    let prior = state_of(&d).get("voice_audio_expected").cloned().unwrap_or_else(|| panic!("{fmt} missing voice_audio_expected"));
    let state = clone_state(&d);
    let review = event(controller, Some(&state), "voice_speak", obj(vec![]), root);
    let before = peer.len();
    let state = clone_state(&review);
    let declined = event(controller, Some(&state), "reject", obj(vec![]), root);
    write_observation(fmt, &review, &declined);
    keep(
        passed,
        &format!("{fmt} rejecting a speech review settles the microphone into a resumable pause"),
        state_str(state_of(&declined), "voice_status") == Some("paused")
            && get_is_none(state_of(&declined), "voice_phase")
            && get_is_none(state_of(&declined), "pending")
            && effect_is_none(&declined)
            && peer.len() == before,
    );
    let session = require_str(&prior, "session");
    let ticket = require_str(&prior, "ticket");
    let checked = controller.send(&obj(vec![
        ("op".into(), s("voice_audio_check")),
        ("voice_session".into(), s(&session)),
        ("voice_audio_ticket".into(), s(&ticket)),
    ]));
    keep(
        passed,
        &format!("{fmt} rejected speech does not retain the prior capture owner"),
        has_error(&checked),
    );
    let state = clone_state(&declined);
    let resumed = event(controller, Some(&state), "voice_listen", obj(vec![]), root);
    let fresh = state_of(&resumed).get("voice_audio_expected").and_then(|v| v.get("ticket"));
    keep(
        passed,
        &format!("{fmt} explicit resume after rejection starts a fresh owned listener"),
        kind_is(required_effect(&resumed), "voice_listen") && fresh != Some(&s(&ticket)),
    );
    let state = clone_state(&resumed);
    let review = event(controller, Some(&state), "voice_speak", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} speech review exposes its actual waiting state"),
        state_str(state_of(&review), "voice_status") == Some("review") && ui_has(&review, "Waiting for voice request approval") && effect_is_none(&review),
    );
    let state = clone_state(&review);
    let hidden = event(controller, Some(&state), "close", obj(vec![]), root);
    let pending = state_of(&review).get("pending").cloned().unwrap_or(Value::Null);
    keep(
        passed,
        &format!("{fmt} dismissing review retains the exact unexecuted proposal"),
        state_of(&hidden).get("pending") == Some(&pending)
            && state_str(state_of(&hidden), "voice_status") == Some("review")
            && effect_is_none(&hidden)
            && peer.len() == before,
    );
    let state = clone_state(&hidden);
    let shown = event(controller, Some(&state), "review", obj(vec![]), root);
    let state = clone_state(&shown);
    let started = event(controller, Some(&state), "approve", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} approval advances the actual admitted speech operation"),
        state_str(state_of(&started), "voice_status") == Some("synthesizing") && purpose_starts(effect_of(&started), "service_request:"),
    );
    let spoken_again = transport(controller, &started, root);
    keep(
        passed,
        &format!("{fmt} approved speech still returns an owned playable artifact"),
        kind_is(required_effect(&spoken_again), "media_preview") && state_str(state_of(&spoken_again), "voice_status") == Some("playback_pending"),
    );
    let play = play_owner(required_effect(&spoken_again));
    let state = clone_state(&spoken_again);
    let listening = event(controller, Some(&state), "voice_playback_result", with_completed(&play, true), root);
    let state = clone_state(&listening);
    let transcript = event(controller, Some(&state), "voice_transcribe", obj(vec![]), root);
    let proposal = state_of(&transcript).get("pending").cloned().unwrap_or(Value::Null);
    let state = clone_state(&transcript);
    let backed = event(controller, Some(&state), "back", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} back navigation keeps a transcription review pending without recording or sending"),
        state_of(&backed).get("pending") == Some(&proposal)
            && state_str(state_of(&backed), "voice_status") == Some("review")
            && effect_is_none(&backed),
    );
    let state = clone_state(&backed);
    let declined = event(controller, Some(&state), "reject", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} rejected transcription also leaves an explicit resumable pause"),
        state_str(state_of(&declined), "voice_status") == Some("paused")
            && get_is_none(state_of(&declined), "voice_phase")
            && get_is_none(state_of(&declined), "pending"),
    );
    let state = clone_state(&declined);
    let transcript = event(controller, Some(&state), "voice_transcribe", obj(vec![]), root);
    let state = clone_state(&transcript);
    let admitted = event(controller, Some(&state), "approve", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt} actual transcription admission reports recognition rather than synthesis"),
        state_str(state_of(&admitted), "voice_status") == Some("recognizing") && purpose_starts(effect_of(&admitted), "service_request:"),
    );
    let written = transport(controller, &admitted, root);
    keep(
        passed,
        &format!("{fmt} manually approved transcription reaches the composer without automatic model submission"),
        effect_is_none(&written)
            && state_str(state_of(&written), "draft") == Some(RECOGNIZED)
            && state_str(state_of(&written), "voice_status") == Some("paused"),
    );
    keep(
        passed,
        &format!("{fmt} retained transcript disables microphone resume until the composer is clear"),
        resume_flag(&written) == Some(false),
    );
    let state = clone_state(&written);
    let cleared = event(controller, Some(&state), "capture", obj(vec![("draft".into(), s(""))]), root);
    keep(
        passed,
        &format!("{fmt} clearing the draft restores the microphone resume control"),
        resume_flag(&cleared) == Some(true),
    );
    let state = clone_state(&cleared);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn speech_admission(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str, heap: u64, passed: &mut Vec<String>) {
    let d = open_and_recognize(controller, peer, root, tone, fmt);
    let mut state = clone_state(&d);
    put(&mut state, "device", device(heap * 1024 * 1024));
    let before = peer.len();
    let d = reply(controller, &state, root, fmt, "Retained answer when speech admission is evaluated.");
    keep(
        passed,
        &format!("{fmt}{heap} speech admission itself sends no HTTP request"),
        peer.len() == before,
    );
    if heap < 32 {
        keep(
            passed,
            &format!("{fmt}{heap} refused speech transfer has no owner or network work"),
            effect_is_none(&d) && get_is_none(state_of(&d), "service_expected") && state_bool_false(state_of(&d), "busy"),
        );
        keep(
            passed,
            &format!("{fmt}{heap} refused speech transfer shows an explicit paused state"),
            state_str(state_of(&d), "voice_status") == Some("paused")
                && get_is_none(state_of(&d), "voice_phase")
                && voice_label(&d) == Some("Paused"),
        );
        keep(
            passed,
            &format!("{fmt}{heap} refusal retains the primary answer and scoped memory explanation"),
            history_has(state_of(&d), "Retained answer when speech admission is evaluated.")
                && notice_has(state_of(&d), "memory cannot admit this transfer"),
        );
        let consent = state_of(&d).get("voice_consent").cloned().unwrap_or(Value::Null);
        let state = clone_state(&d);
        let resumed = event(controller, Some(&state), "voice_listen", obj(vec![]), root);
        keep(
            passed,
            &format!("{fmt}{heap} refused speech resumes only through a fresh explicit microphone"),
            effect_of(&resumed).is_some()
                && kind_is(required_effect(&resumed), "voice_listen")
                && state_of(&resumed).get("voice_consent") == Some(&consent),
        );
        let state = clone_state(&resumed);
        let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
    } else {
        let expected = state_of(&d).get("service_expected").and_then(|v| v.get("ticket")).cloned();
        keep(
            passed,
            &format!("{fmt}{heap} sufficient memory still admits synthesis with a current owner"),
            effect_of(&d).and_then(|e| e.get("purpose")).cloned() == expected
                && state_str(state_of(&d), "voice_status") == Some("synthesizing")
                && state_bool_true(state_of(&d), "busy"),
        );
        let d = transport(controller, &d, root);
        keep(
            passed,
            &format!("{fmt}{heap} admitted synthesis still plays actual returned audio"),
            effect_of(&d).is_some() && kind_is(required_effect(&d), "media_preview"),
        );
        let state = clone_state(&d);
        let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
    }
}

fn refused_recognition(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, fmt, root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let owner = mic_owner(required_effect(&d));
    let mut state = clone_state(&d);
    put(&mut state, "device", device(2 * 1024 * 1024));
    let mut carrying = d.clone();
    put(&mut carrying, "state", state);
    let before = peer.len();
    let d = submit_utterance(controller, &carrying, root, tone);
    keep(
        passed,
        &format!("{fmt} refused recognition has no owner,network work or upload"),
        effect_is_none(&d) && get_is_none(state_of(&d), "service_expected") && state_bool_false(state_of(&d), "busy") && peer.len() == before,
    );
    keep(
        passed,
        &format!("{fmt} refused recognition also pauses and clears its phase"),
        state_str(state_of(&d), "voice_status") == Some("paused")
            && get_is_none(state_of(&d), "voice_phase")
            && resume_flag(&d) == Some(true),
    );
    let state = clone_state(&d);
    let stale = event(controller, Some(&state), "voice_audio_failure", owner, root);
    keep(
        passed,
        &format!("{fmt} retired capture cannot alter a memory-refused voice state"),
        state_of(&stale) == &state && effect_is_none(&stale),
    );
    let state = clone_state(&d);
    let sent = event(
        controller,
        Some(&state),
        "send",
        obj(vec![
            ("draft".into(), s("Ordinary text after the refused recognition.")),
            ("device".into(), device(64 * 1024 * 1024)),
        ]),
        root,
    );
    keep(
        passed,
        &format!("{fmt} ordinary text starts after the scoped memory refusal"),
        effect_of(&sent).is_some() && purpose_is(required_effect(&sent), "model") && state_bool_false(state_of(&sent), "voice_open"),
    );
    let state = clone_state(&sent);
    let done = reply(controller, &state, root, fmt, "Ordinary reply after recognition refusal.");
    keep(
        passed,
        &format!("{fmt} ordinary text answer completes without voice or replay"),
        effect_is_none(&done) && state_bool_false(state_of(&done), "busy"),
    );
}

fn separate_service_refusal(controller: &mut Controller, peer: &Peer, root: &str, fmt: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, fmt, root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let owner = mic_owner(required_effect(&d));
    let session = require_str(&owner, "voice_session");
    let ticket = require_str(&owner, "voice_audio_ticket");
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_audio_start", owner.clone(), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "service_request", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "service_review",
        obj(vec![
            ("service_route".into(), s("synthesis")),
            ("service_capability".into(), s("speech_synthesis")),
            ("service_input".into(), s(&json_text("Separate reviewed service request."))),
        ]),
        root,
    );
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "approve", obj(vec![("device".into(), device(16 * 1024 * 1024))]), root);
    keep(
        passed,
        &format!("{fmt} separate service refusal retains the already-paused voice session"),
        effect_is_none(&d)
            && state_bool_false(state_of(&d), "busy")
            && state_str(state_of(&d), "voice_status") == Some("paused")
            && is_null_field(state_of(&d), "voice_audio_expected")
            && state_of(&d).get("voice_consent") == Some(&s(&session)),
    );
    let retired = controller.send(&feed_envelope(&session, &ticket, &vec![0u8; 640]));
    keep(
        passed,
        &format!("{fmt} separate service review retires actual PCM admission before its refusal"),
        lacks_value(&retired),
    );
    let state = clone_state(&d);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn held_utterance(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str, stage: &str, passed: &mut Vec<String>) {
    let state = configured(controller, peer, fmt, root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let capture = required_effect(&d).clone();
    let held = hold_utterance(controller, &d, root, tone);
    let before = peer.len();
    let state = clone_state(&held.started);
    let d = event(controller, Some(&state), "service_request", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(
        controller,
        Some(&state),
        "service_review",
        obj(vec![
            ("service_route".into(), s("synthesis")),
            ("service_capability".into(), s("speech_synthesis")),
            ("service_input".into(), s(&json_text("Preserve this separately reviewed request."))),
        ]),
        root,
    );
    let proposal = state_of(&d).get("pending").cloned().unwrap_or(Value::Null);
    keep(
        passed,
        &format!("{fmt}{stage} separate request review pauses current capture"),
        state_str(state_of(&d), "voice_status") == Some("paused")
            && get_is_none(state_of(&d), "voice_audio_expected")
            && effect_is_none(&d)
            && !matches!(proposal, Value::Null),
    );
    keep(
        passed,
        &format!("{fmt}{stage} a pending separate review disables microphone resume"),
        resume_flag(&d) == Some(false),
    );
    let d = if stage == "admitted" {
        let state = clone_state(&d);
        event(controller, Some(&state), "approve", obj(vec![]), root)
    } else {
        d
    };
    let prior = clone_state(&d);
    let service = state_of(&d).get("service_expected").cloned();
    let late = event(controller, Some(&prior), "voice_audio_result", held.completion.clone(), root);
    keep(
        passed,
        &format!("{fmt}{stage} a real held utterance cannot replace separate work"),
        state_of(&late) == &prior && effect_is_none(&late) && peer.len() == before,
    );
    let session = require_str(&capture, "voice_session");
    let ticket = require_str(&capture, "voice_audio_ticket");
    let retired = controller.send(&feed_envelope(&session, &ticket, tone));
    keep(
        passed,
        &format!("{fmt}{stage} retired utterance PCM cannot be reactivated"),
        lacks_value(&retired),
    );
    let d = if stage == "proposal" {
        keep(
            passed,
            &format!("{fmt}{stage} exact separate proposal survives the late callback"),
            state_of(&late).get("pending") == Some(&proposal),
        );
        let state = clone_state(&late);
        event(controller, Some(&state), "approve", obj(vec![]), root)
    } else {
        keep(
            passed,
            &format!("{fmt}{stage} exact admitted service owner survives the late callback"),
            service.as_ref().is_some_and(|expected| state_of(&late).get("service_expected") == Some(expected))
                && state_bool_true(state_of(&late), "busy"),
        );
        d
    };
    keep(
        passed,
        &format!("{fmt}{stage} admitted separate work keeps resume unavailable"),
        resume_flag(&d) == Some(false),
    );
    let done = transport(controller, &d, root);
    let status = state_of(&done).get("service_result").and_then(|v| v.get("status"));
    keep(
        passed,
        &format!("{fmt}{stage} actual separate HTTP result still completes"),
        status == Some(&s("response_received"))
            && state_bool_false(state_of(&done), "busy")
            && state_str(state_of(&done), "voice_status") == Some("paused")
            && effect_is_none(&done),
    );
    let state = clone_state(&done);
    let resumed = event(controller, Some(&state), "voice_listen", obj(vec![]), root);
    keep(
        passed,
        &format!("{fmt}{stage} explicit resume gets a new owner after separate completion"),
        effect_of(&resumed).is_some()
            && kind_is(required_effect(&resumed), "voice_listen")
            && required_effect(&resumed).get("voice_audio_ticket") != Some(&s(&ticket)),
    );
    let state = clone_state(&resumed);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn preparation_refusal(controller: &mut Controller, peer: &Peer, tone: &[u8], fmt: &str, refusal: &str, passed: &mut Vec<String>) {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&project, "voice-speech-refusal-");
    let root = scratch.path();
    let d = open_and_recognize(controller, peer, root, tone, fmt);
    let answer = if refusal == "text_bound" {
        "Long controlled reply.".repeat(1800)
    } else {
        "Retained reply when artifact storage is full.".to_string()
    };
    if refusal == "storage_quota" {
        fill_quota(root);
    }
    let before = peer.len();
    let state = clone_state(&d);
    let d = reply(controller, &state, root, fmt, &answer);
    keep(
        passed,
        &format!("{fmt}{refusal} speech preparation refusal is an explicit paused state"),
        state_str(state_of(&d), "voice_status") == Some("paused") && get_is_none(state_of(&d), "voice_phase"),
    );
    keep(
        passed,
        &format!("{fmt}{refusal} speech preparation refusal has no request or replay"),
        effect_is_none(&d) && state_bool_false(state_of(&d), "busy") && get_is_none(state_of(&d), "service_expected") && peer.len() == before,
    );
    keep(
        passed,
        &format!("{fmt}{refusal} complete primary text remains with scoped speech explanation"),
        history_has(state_of(&d), &answer) && notice_has(state_of(&d), "Text reply retained; speech unavailable"),
    );
    keep(
        passed,
        &format!("{fmt}{refusal} speech preparation refusal leaves explicit resume available"),
        resume_flag(&d) == Some(true),
    );
    let state = clone_state(&d);
    let _ = event(controller, Some(&state), "voice_stop", obj(vec![]), root);
}

fn spoken(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str) -> Value {
    spoken_reply(controller, peer, root, tone, fmt, "Actual reply before explicit speech interruption.")
}

fn spoken_reply(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str, answer: &str) -> Value {
    let d = open_and_recognize(controller, peer, root, tone, fmt);
    let state = clone_state(&d);
    let d = reply(controller, &state, root, fmt, answer);
    transport(controller, &d, root)
}

fn open_and_recognize(controller: &mut Controller, peer: &Peer, root: &str, tone: &[u8], fmt: &str) -> Value {
    let state = configured(controller, peer, fmt, root);
    let d = event(controller, Some(&state), "voice_open_review", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "voice_consent", obj(vec![]), root);
    let d = submit_utterance(controller, &d, root, tone);
    transport(controller, &d, root)
}

fn submit_utterance(controller: &mut Controller, event_value: &Value, root: &str, tone: &[u8]) -> Value {
    match utter(controller, event_value, root, tone, true) {
        Utterance::Submitted(value) => value,
        Utterance::Held(_) => panic!("No actual utterance was produced"),
    }
}

fn hold_utterance(controller: &mut Controller, event_value: &Value, root: &str, tone: &[u8]) -> Held {
    match utter(controller, event_value, root, tone, false) {
        Utterance::Held(held) => held,
        Utterance::Submitted(_) => panic!("No actual utterance was produced"),
    }
}

enum Utterance {
    Submitted(Value),
    Held(Held),
}

struct Held {
    started: Value,
    completion: Value,
}

fn utter(controller: &mut Controller, event_value: &Value, root: &str, tone: &[u8], callback: bool) -> Utterance {
    let effect = required_effect(event_value).clone();
    let owner = mic_owner(&effect);
    let session = require_str(&owner, "voice_session");
    let ticket = require_str(&owner, "voice_audio_ticket");
    let state = clone_state(event_value);
    let started = event(controller, Some(&state), "voice_audio_start", owner.clone(), root);
    let silence = vec![0u8; 640];
    let mut frames: Vec<&[u8]> = Vec::with_capacity(80);
    for _ in 0..20 {
        frames.push(&silence);
    }
    for _ in 0..20 {
        frames.push(tone);
    }
    for _ in 0..40 {
        frames.push(&silence);
    }
    for frame in frames {
        let value = controller.op(&feed_envelope(&session, &ticket, frame));
        if flag(&value, "ready") == Some(true) {
            let mut completion = owner;
            put(&mut completion, "receipt", value.get("receipt").cloned().unwrap_or_else(|| panic!("ready feed missing receipt")));
            if callback {
                let state = clone_state(&started);
                return Utterance::Submitted(event(controller, Some(&state), "voice_audio_result", completion, root));
            }
            return Utterance::Held(Held { started, completion });
        }
    }
    panic!("No actual utterance was produced");
}

fn configured(controller: &mut Controller, peer: &Peer, fmt: &str, root: &str) -> Value {
    let d = event(controller, None, "resume", obj(vec![]), root);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "service_connections", obj(vec![]), root);
    let config = service_config(peer.port);
    let state = clone_state(&d);
    let d = event(controller, Some(&state), "service_save", obj(vec![("service_config".into(), s(&config))]), root);
    let mut state = clone_state(&d);
    put(&mut state, "profiles", Value::Arr(vec![profile(fmt)]));
    put(&mut state, "agents", Value::Arr(vec![agent()]));
    state
}

fn service_config(port: u16) -> String {
    let host = obj(vec![
        ("id".into(), s("audio")),
        ("url".into(), s(&format!("http://127.0.0.1:{port}"))),
        ("plain".into(), Value::Bool(true)),
        ("enabled".into(), Value::Bool(true)),
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
        ("enabled".into(), Value::Bool(true)),
    ]);
    let synthesis = obj(vec![
        ("id".into(), s("synthesis")),
        ("host".into(), s("audio")),
        ("capability".into(), s("speech_synthesis")),
        ("path".into(), s("/speak")),
        ("method".into(), s("POST")),
        ("request_format".into(), s("json")),
        ("request".into(), obj(vec![("text".into(), obj(vec![("$input".into(), s("/text"))]))])),
        ("response".into(), s("binary")),
        ("response_path".into(), s("")),
        ("artifact_kind".into(), s("audio")),
        ("enabled".into(), Value::Bool(true)),
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
        ("plain".into(), Value::Bool(true)),
        ("stream".into(), Value::Bool(false)),
    ])
}

fn agent() -> Value {
    obj(vec![
        ("name".into(), s("General")),
        ("tools".into(), s("read,service")),
        ("investigation_preferences".into(), Value::Bool(false)),
    ])
}

fn drop_recognition(state: &mut Value) {
    let defaults = field_mut(field_mut(state, "service_connections"), "defaults");
    remove_key(defaults, "speech_recognition");
}

fn transport(controller: &mut Controller, event_value: &Value, root: &str) -> Value {
    let effect = required_effect(event_value).clone();
    let wire = wire_bytes(&effect);
    let host = require_str(&effect, "host");
    let port = as_u16(effect.get("port").unwrap_or_else(|| panic!("missing port")));
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("peer connect {host}:{port}: {error}"));
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    stream.write_all(&wire).unwrap_or_else(|error| panic!("peer write: {error}"));
    upload(controller, &effect, &mut stream);
    let mut fields = response(controller, &effect, &mut stream);
    put(&mut fields, "purpose", effect.get("purpose").cloned().unwrap_or(Value::Null));
    let state = clone_state(event_value);
    event(controller, Some(&state), "network_result", fields, root)
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
            if flag(&part, "done") == Some(true) {
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
            ready = flag(&value, "ready") == Some(true);
            if ready {
                break;
            }
        }
        let mut envelope = owner.clone();
        put(&mut envelope, "op", s("media_stream_finish"));
        put(&mut envelope, "eof", Value::Bool(!ready));
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

fn reply(controller: &mut Controller, state: &Value, root: &str, fmt: &str, answer: &str) -> Value {
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
        _ => panic!("unknown model format {fmt}"),
    }
    .to_string()
}

fn json_text(text: &str) -> String {
    obj(vec![("text".into(), s(text))]).to_string()
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state, action, inputs, root)
}

fn clone_state(value: &Value) -> Value {
    state_of(value).clone()
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn effect_of(value: &Value) -> Option<&Value> {
    match value.get("effect") {
        Some(effect) if !matches!(effect, Value::Null) => Some(effect),
        _ => None,
    }
}

fn effect_is_none(value: &Value) -> bool {
    matches!(value.get("effect"), Some(Value::Null))
}

fn required_effect(value: &Value) -> &Value {
    effect_of(value).unwrap_or_else(|| panic!("missing effect: {}", value.to_string()))
}

fn kind_is(effect: &Value, kind: &str) -> bool {
    field_is(effect, "kind", kind)
}

fn field_is(value: &Value, key: &str, expect: &str) -> bool {
    value.get(key) == Some(&s(expect))
}

fn purpose_is(effect: &Value, purpose: &str) -> bool {
    field_is(effect, "purpose", purpose)
}

fn purpose_starts(effect: Option<&Value>, prefix: &str) -> bool {
    match effect.and_then(|value| value.get("purpose")) {
        Some(Value::Str(text)) => text.starts_with(prefix),
        _ => false,
    }
}

fn wire_has(effect: &Value, needle: &str) -> bool {
    match effect.get("wire") {
        Some(Value::Str(text)) => text.contains(needle),
        _ => false,
    }
}

fn capture_visible(effect: &Value, root: &str) -> bool {
    let Some(Value::Str(path_text)) = effect.get("capture_path") else { return false };
    let path = std::path::Path::new(path_text);
    let expect = std::path::Path::new(root).join("service-files");
    let limit = effect.get("capture_limit").and_then(as_u64);
    path.parent() == Some(expect.as_path()) && path.is_file() && limit.is_some_and(|limit| limit > 0 && limit <= 2 * 1024 * 1024)
}

fn feed_envelope(session: &str, ticket: &str, data: &[u8]) -> Value {
    obj(vec![
        ("op".into(), s("voice_audio_feed")),
        ("voice_session".into(), s(session)),
        ("voice_audio_ticket".into(), s(ticket)),
        ("data".into(), s(&b64_encode(data))),
    ])
}

fn mic_owner(effect: &Value) -> Value {
    obj(vec![
        ("voice_session".into(), effect.get("voice_session").cloned().unwrap_or(Value::Null)),
        ("voice_audio_ticket".into(), effect.get("voice_audio_ticket").cloned().unwrap_or(Value::Null)),
    ])
}

fn play_owner(effect: &Value) -> Value {
    obj(vec![
        ("voice_session".into(), effect.get("voice_session").cloned().unwrap_or(Value::Null)),
        ("voice_playback_ticket".into(), effect.get("voice_playback_ticket").cloned().unwrap_or(Value::Null)),
    ])
}

fn with_completed(play: &Value, completed: bool) -> Value {
    let mut next = play.clone();
    put(&mut next, "completed", Value::Bool(completed));
    next
}

fn device(heap: u64) -> Value {
    obj(vec![
        ("heap_bytes".into(), n(&heap.to_string())),
        ("memory_bytes".into(), n(&(1024u64 * 1024 * 1024).to_string())),
        ("cores".into(), n("2")),
    ])
}

fn history_has(state: &Value, text: &str) -> bool {
    let Some(Value::Arr(chats)) = state.get("chats") else { return false };
    chats.iter().any(|chat| {
        let Some(Value::Arr(messages)) = chat.get("messages") else { return false };
        messages.iter().any(|message| message.get("content") == Some(&s(text)))
    })
}

fn notice_has(state: &Value, needle: &str) -> bool {
    matches!(state.get("notice"), Some(Value::Str(text)) if text.contains(needle))
}

fn modal_has(value: &Value, needle: &str) -> bool {
    value.get("ui").and_then(|ui| ui.get("modal")).is_some_and(|modal| modal.to_string().contains(needle))
}

fn ui_has(value: &Value, needle: &str) -> bool {
    value.get("ui").is_some_and(|ui| ui.to_string().contains(needle))
}

fn resume_flag(value: &Value) -> Option<bool> {
    match value.get("ui").and_then(|ui| ui.get("voice_status")).and_then(|status| status.get("resume_available")) {
        Some(Value::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

fn voice_label(value: &Value) -> Option<&str> {
    match value.get("ui").and_then(|ui| ui.get("voice_status")).and_then(|status| status.get("label")) {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    }
}

fn state_str<'a>(state: &'a Value, key: &str) -> Option<&'a str> {
    match state.get(key) {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    }
}

fn state_bool_true(state: &Value, key: &str) -> bool {
    matches!(state.get(key), Some(Value::Bool(true)))
}

fn state_bool_false(state: &Value, key: &str) -> bool {
    matches!(state.get(key), Some(Value::Bool(false)))
}

fn is_null_field(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Null))
}

fn get_is_none(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn py_true(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Str(text)) => !text.is_empty(),
        Some(Value::Num(token)) => token != "0",
        Some(Value::Arr(items)) => !items.is_empty(),
        Some(Value::Obj(fields)) => !fields.is_empty(),
        _ => false,
    }
}

fn flag(value: &Value, key: &str) -> Option<bool> {
    match value.get(key) {
        Some(Value::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

fn lacks_value(reply: &Value) -> bool {
    reply.get("value").is_none()
}

fn has_error(reply: &Value) -> bool {
    reply.get("error").is_some()
}

fn require_str(value: &Value, key: &str) -> String {
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

fn as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Num(token) | Value::Str(token) => token.parse().ok(),
        _ => None,
    }
}

fn as_usize(value: &Value) -> usize {
    as_u64(value).and_then(|value| usize::try_from(value).ok()).unwrap_or_else(|| panic!("bad size {}", value.to_string()))
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    if let Some(Value::Str(text)) = effect.get("wire_base64") {
        return b64_decode(text);
    }
    if let Some(Value::Str(text)) = effect.get("wire") {
        return text.as_bytes().to_vec();
    }
    panic!("missing wire in {}", effect.to_string());
}

fn body_is_text(body: &[u8], expect: &str) -> bool {
    let Ok(text) = std::str::from_utf8(body) else { return false };
    if !text.trim_start().starts_with('{') {
        return false;
    }
    let Ok(value) = catch_unwind(|| Value::parse(text)) else { return false };
    match value {
        Value::Obj(fields) => fields.len() == 1 && fields.iter().any(|(key, value)| key == "text" && value == &s(expect)),
        _ => false,
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn wave_contains_tone(body: &[u8], tone: &[u8]) -> bool {
    let Some(offset) = body.windows(4).position(|window| window == b"RIFF") else { return false };
    if body.len() < offset + 12 || &body[offset + 8..offset + 12] != b"WAVE" {
        return false;
    }
    let size = u32::from_le_bytes(body[offset + 4..offset + 8].try_into().unwrap()) as usize;
    let Some(end) = offset.checked_add(size).and_then(|value| value.checked_add(8)) else { return false };
    if end > body.len() {
        return false;
    }
    let wav = &body[offset..end];
    let mut index = 12usize;
    let mut channels = None;
    let mut rate = None;
    let mut frames = None;
    while index + 8 <= wav.len() {
        let id = &wav[index..index + 4];
        let length = u32::from_le_bytes(wav[index + 4..index + 8].try_into().unwrap()) as usize;
        index += 8;
        if index + length > wav.len() {
            return false;
        }
        let chunk = &wav[index..index + length];
        if id == b"fmt " {
            if chunk.len() < 16 {
                return false;
            }
            let format = u16::from_le_bytes(chunk[0..2].try_into().unwrap());
            if format != 1 {
                return false;
            }
            channels = Some(u16::from_le_bytes(chunk[2..4].try_into().unwrap()));
            rate = Some(u32::from_le_bytes(chunk[4..8].try_into().unwrap()));
        } else if id == b"data" {
            frames = Some(chunk);
        }
        index += length + (length % 2);
    }
    channels == Some(1) && rate == Some(16000) && frames.is_some_and(|frames| contains_bytes(frames, tone))
}

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    fields.push((key.to_string(), Value::Null));
    &mut fields.last_mut().unwrap().1
}

fn remove_key(value: &mut Value, key: &str) {
    if let Value::Obj(fields) = value {
        fields.retain(|(name, _)| name != key);
    }
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn write_observation(fmt: &str, review: &Value, declined: &Value) {
    let before = state_of(review);
    let after = state_of(declined);
    let body = obj(vec![
        ("format".into(), s(fmt)),
        ("before_phase".into(), before.get("voice_status").cloned().unwrap_or(Value::Null)),
        ("after_phase".into(), after.get("voice_status").cloned().unwrap_or(Value::Null)),
        ("after_voice_phase".into(), after.get("voice_phase").cloned().unwrap_or(Value::Null)),
        ("pending".into(), after.get("pending").cloned().unwrap_or(Value::Null)),
        ("effect".into(), declined.get("effect").cloned().unwrap_or(Value::Null)),
    ]);
    let mut text = twin::pretty(&body);
    text.push('\n');
    std::fs::write("output/checks/voice-review-observation.json", text).unwrap_or_else(|error| panic!("observation: {error}"));
}

fn fill_quota(root: &str) {
    let directory = std::path::Path::new(root).join("service-files");
    std::fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("quota directory: {error}"));
    for index in 0..512 {
        std::fs::write(directory.join(format!("quota-{index}.bin")), b"").unwrap_or_else(|error| panic!("quota file: {error}"));
    }
}

fn wav_fixture() -> Vec<u8> {
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

struct Recorded {
    body: Vec<u8>,
}

struct Peer {
    port: u16,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind peer: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer addr: {error}")).port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sink = requests.clone();
        let wav = wav_fixture();
        let recognized = br#"{"text": "Recognized fixture question."}"#.to_vec();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
                let Some((path, body)) = read_request(&mut stream) else { continue };
                if let Ok(mut guard) = sink.lock() {
                    guard.push(Recorded { body });
                }
                let response_body = if path == "/speak" { wav.clone() } else { recognized.clone() };
                let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response_body.len());
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
        self.requests.lock().unwrap_or_else(|error| error.into_inner()).last().map(|item| item.body.clone()).unwrap_or_default()
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
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    }).unwrap_or(0)
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path, prefix: &str) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_nanos()).unwrap_or(0);
        let path = root.join("output").join(format!("{prefix}{}-{nanos}-{seq}", std::process::id()));
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

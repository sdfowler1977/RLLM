mod twin;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use twin::{b, b64_decode, b64_encode, n, obj, s, sha256_digest, Controller, Value};

const SCOPE: &str = "Owner checks through actual local HTTP authorization, S256 code verification, token refresh/rotation and model peers across all four formats. Browser-return delivery is controlled; installed Android browser/deep-link/Keystore and live authorization-server acceptance remain unverified.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let peer = Peer::start();
    let scratch = twin::create_scratch(&project, "oauth");
    let root = scratch.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&project_text);
    let passed = run(&mut controller, &project_text, &root, &peer);
    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);
    if passed.len() != 121 {
        panic!("expected 121 checks, got {}", passed.len());
    }
    let checks = Value::Arr(passed.iter().map(|name| s(name)).collect());
    twin::report_value(
        "oauth",
        &obj(vec![
            ("passed".into(), n(&passed.len().to_string())),
            ("checks".into(), checks),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
}

fn run(controller: &mut Controller, project: &str, root: &str, peer: &Peer) -> Vec<String> {
    let mut passed = Vec::new();
    for format in ["chat", "local", "messages", "content"] {
        let configured = setup(controller, root, peer, format);
        let (started, uri) = browser(controller, state(&configured), peer, root);
        let query = query_of(&started);
        let state_value = query.get("state").and_then(|values| values.first()).map(String::as_str).unwrap_or("");
        let challenge = query.get("code_challenge").and_then(|values| values.first()).map(String::as_str).unwrap_or("");
        check(
            &mut passed,
            &format!("{format} uses random S256 transaction proof"),
            state_value.len() >= 43 && challenge.len() == 43 && !query.contains_key("code_verifier"),
        );
        check(
            &mut passed,
            &format!("{format} sign-in preserves existing key and text availability"),
            profile_text(state(&started), "key") == "retained-api-key" && !busy(state(&started)),
        );

        let wrong = event(
            controller,
            Some(state(&started)),
            "oauth_return",
            obj(vec![
                ("uri".into(), s(&uri.replace("state=", "state=wrong"))),
                ("draft".into(), s("Injected stale draft")),
            ]),
            root,
        );
        check(
            &mut passed,
            &format!("{format} wrong return is exact state no-op"),
            same_state(&wrong, &started) && effect_is_null(&wrong),
        );

        let duplicate = event(
            controller,
            Some(state(&started)),
            "oauth_return",
            obj(vec![("uri".into(), s(&format!("{uri}&state=again")))]),
            root,
        );
        check(
            &mut passed,
            &format!("{format} duplicate return parameters are refused before exchange"),
            same_state(&duplicate, &started) && effect_is_null(&duplicate),
        );

        let token = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
        check(
            &mut passed,
            &format!("{format} code is exchanged at configured endpoint only"),
            effect_text(&token, "host") == Some("127.0.0.1")
                && effect_text(&token, "wire").is_some_and(|wire| wire.starts_with("POST /token ")),
        );
        let again = event(controller, Some(state(&token)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
        check(
            &mut passed,
            &format!("{format} code return cannot exchange twice"),
            same_state(&again, &token) && effect_is_null(&again),
        );

        let (adopted, callback) = exchange(controller, &token, peer, root);
        let access = token_str(state(&adopted), "access_token");
        let old_refresh = token_str(state(&adopted), "refresh_token");
        check(
            &mut passed,
            &format!("{format} actual server credential is adopted without replacing API key"),
            profile_flag(state(&adopted), "oauth_enabled") && profile_text(state(&adopted), "key") == "retained-api-key",
        );
        let ui = ui_dump(&adopted);
        check(
            &mut passed,
            &format!("{format} credentials stay out of authorization UI"),
            !access.is_empty() && !old_refresh.is_empty() && !ui.contains(&access) && !ui.contains(&old_refresh),
        );
        let replayed = event(controller, Some(state(&adopted)), "network_result", callback, root);
        check(
            &mut passed,
            &format!("{format} token callback is single-use"),
            same_state(&replayed, &adopted) && effect_is_null(&replayed),
        );

        let sent = event(
            controller,
            Some(state(&adopted)),
            "send",
            obj(vec![("draft".into(), s("Speak to the actual peer."))]),
            root,
        );
        let wire = effect_text(&sent, "wire").unwrap_or("");
        check(
            &mut passed,
            &format!("{format} normal conversation uses bearer auth for its own format"),
            wire.contains(&format!("Authorization: Bearer {access}\r\n")) && !wire.contains("retained-api-key"),
        );
        let (completed, _) = exchange(controller, &sent, peer, root);
        check(
            &mut passed,
            &format!("{format} actual HTTP model response completes"),
            !busy(state(&completed))
                && state(&completed).get("chats").map(Value::to_string).unwrap_or_default().contains("Actual model peer response."),
        );

        let refreshed = event(controller, Some(state(&completed)), "oauth_refresh", empty(), root);
        check(
            &mut passed,
            &format!("{format} refresh uses actual configured refresh credential"),
            effect_text(&refreshed, "wire").unwrap_or("").contains(&format!("refresh_token={}", encode_query(&old_refresh))),
        );
        let (rotated, _) = exchange(controller, &refreshed, peer, root);
        check(
            &mut passed,
            &format!("{format} refreshed credentials rotate together"),
            token_str(state(&rotated), "refresh_token") != old_refresh && token_str(state(&rotated), "access_token") != access,
        );

        let saved = profile_at(state(&rotated), 0).clone();
        let export = event(controller, Some(state(&rotated)), "configuration_export", empty(), root);
        let export_text = effect_dump(&export);
        let saved_key = text_of(&saved, "key");
        let saved_access = text_of(saved.get("oauth_tokens").unwrap_or(&Value::Null), "access_token");
        let saved_refresh = text_of(saved.get("oauth_tokens").unwrap_or(&Value::Null), "refresh_token");
        check(
            &mut passed,
            &format!("{format} configuration export omits all authentication credentials"),
            !saved_key.is_empty()
                && !saved_access.is_empty()
                && !saved_refresh.is_empty()
                && !export_text.contains(&saved_key)
                && !export_text.contains(&saved_access)
                && !export_text.contains(&saved_refresh),
        );

        let key_mode = event(controller, Some(state(&rotated)), "oauth_key_mode", empty(), root);
        let key_sent = event(
            controller,
            Some(state(&key_mode)),
            "send",
            obj(vec![("draft".into(), s("Use retained API settings."))]),
            root,
        );
        let wire = effect_text(&key_sent, "wire").unwrap_or("");
        check(
            &mut passed,
            &format!("{format} choosing API-key mode restores the existing credential"),
            wire.contains("retained-api-key") && !wire.contains(&saved_access),
        );
        event(controller, Some(state(&key_sent)), "cancel", empty(), root);
    }

    for format in ["chat", "local", "messages", "content"] {
        let authorized = sign_in(controller, root, peer, format);
        let expiring_state = expiring(&authorized);
        let access = token_str(&expiring_state, "access_token");
        let before = peer.received_len();
        let pending = event(
            controller,
            Some(&expiring_state),
            "send",
            obj(vec![("draft".into(), s("Refresh before this exact question."))]),
            root,
        );
        check(
            &mut passed,
            &format!("{format} expiry schedules one refresh before any model POST"),
            effect_text(&pending, "purpose").is_some_and(|purpose| purpose.starts_with("oauth_token:"))
                && peer.received_len() == before
                && refresh_uncertain(state(&pending)),
        );
        let (sent, callback) = exchange(controller, &pending, peer, root);
        let wire = effect_text(&sent, "wire").unwrap_or("");
        check(
            &mut passed,
            &format!("{format} owned refresh resumes exact original model question once"),
            effect_text(&sent, "purpose") == Some("model")
                && wire.contains("Refresh before this exact question.")
                && !access.is_empty()
                && !wire.contains(&access),
        );
        let duplicate = event(controller, Some(state(&sent)), "network_result", callback, root);
        check(
            &mut passed,
            &format!("{format} duplicate automatic refresh cannot submit a second model turn"),
            same_state(&duplicate, &sent) && effect_is_null(&duplicate),
        );
        let (done, _) = exchange(controller, &sent, peer, root);
        check(
            &mut passed,
            &format!("{format} automatic refresh completes real model response"),
            !busy(state(&done)) && !refresh_uncertain(state(&done)),
        );
    }

    let authorized = sign_in(controller, root, peer, "chat");
    let pending = event(
        controller,
        Some(&expiring(&authorized)),
        "send",
        obj(vec![("draft".into(), s("Cancelled automatic refresh."))]),
        root,
    );
    let cancelled = event(controller, Some(state(&pending)), "cancel", empty(), root);
    let (_done, callback) = exchange(controller, &pending, peer, root);
    let late = event(controller, Some(state(&cancelled)), "network_result", callback, root);
    check(
        &mut passed,
        "cancelled automatic refresh cannot adopt tokens or resume the model",
        same_state(&late, &cancelled) && effect_is_null(&late),
    );

    let pending = event(
        controller,
        Some(&expiring(&authorized)),
        "send",
        obj(vec![("draft".into(), s("Confirmed pre-send failure."))]),
        root,
    );
    let failed = event(
        controller,
        Some(state(&pending)),
        "failure",
        obj(vec![
            ("purpose".into(), s(effect_text(&pending, "purpose").unwrap_or(""))),
            ("sent".into(), b(false)),
            ("error".into(), s("controlled pre-send refusal")),
        ]),
        root,
    );
    check(
        &mut passed,
        "confirmed pre-send refresh failure retains retryable credential",
        effect_is_null(&failed) && !refresh_uncertain(state(&failed)),
    );
    let retry = event(
        controller,
        Some(state(&failed)),
        "send",
        obj(vec![("draft".into(), s("New explicit question after pre-send failure."))]),
        root,
    );
    check(
        &mut passed,
        "new explicit turn can refresh after confirmed pre-send failure",
        effect_text(&retry, "purpose").is_some_and(|purpose| purpose.starts_with("oauth_token:")),
    );
    event(controller, Some(state(&retry)), "cancel", empty(), root);

    let pending = event(
        controller,
        Some(&expiring(&authorized)),
        "send",
        obj(vec![("draft".into(), s("Uncertain refresh outcome."))]),
        root,
    );
    let failed = event(
        controller,
        Some(state(&pending)),
        "failure",
        obj(vec![
            ("purpose".into(), s(effect_text(&pending, "purpose").unwrap_or(""))),
            ("sent".into(), b(true)),
            ("error".into(), s("controlled uncertain submission")),
        ]),
        root,
    );
    let retry = event(controller, Some(state(&failed)), "oauth_refresh", empty(), root);
    check(
        &mut passed,
        "uncertain rotating refresh credential is never replayed",
        effect_is_null(&retry) && flag(state(&retry), "notice_error") && refresh_uncertain(state(&retry)),
    );

    let pending = event(
        controller,
        Some(&expiring(&authorized)),
        "send",
        obj(vec![("draft".into(), s("Original pending context."))]),
        root,
    );
    let mut changed = pending.clone();
    set_key(obj_mut(&mut changed, "state"), "routing", obj(vec![("primary".into(), n("99"))]));
    let (done, _) = exchange(controller, &changed, peer, root);
    check(
        &mut passed,
        "changed conversation context cannot resume an automatic model request",
        effect_is_null(&done) && flag(state(&done), "notice_error"),
    );

    let reviewed = child_review(controller, root, peer);
    check(
        &mut passed,
        "child authorization refresh does not bypass operation review",
        text_is(state(&reviewed), "dialog", "approval") && effect_is_null(&reviewed),
    );
    let pending = event(controller, Some(state(&reviewed)), "approve", empty(), root);
    check(
        &mut passed,
        "approved child refresh carries captured delegation ownership",
        effect_text(&pending, "purpose").is_some_and(|purpose| purpose.starts_with("oauth_token:"))
            && effect_obj(&pending).and_then(|effect| effect.get("child_ticket")).is_some()
            && stack_len(state(&pending)) == 1,
    );
    let (child, _callback) = exchange(controller, &pending, peer, root);
    check(
        &mut passed,
        "child refresh resumes the exact reviewed model",
        effect_text(&child, "purpose") == Some("model")
            && index_is(state(&child), "1")
            && effect_text(&child, "wire").unwrap_or("").contains("child-model"),
    );
    let parent = model_reply(controller, &child, peer, root, &evidence_reply());
    check(
        &mut passed,
        "refreshed child returns its real result to captured parent once",
        effect_text(&parent, "purpose") == Some("model")
            && index_is(state(&parent), "0")
            && effect_text(&parent, "wire").unwrap_or("").contains("EXACT_OWNED_CHILD_EVIDENCE"),
    );
    let (done, _) = exchange(controller, &parent, peer, root);
    check(
        &mut passed,
        "parent after refreshed child settles normally",
        !busy(state(&done)) && stack_empty(state(&done)),
    );

    let reviewed = child_review(controller, root, peer);
    let pending = event(controller, Some(state(&reviewed)), "approve", empty(), root);
    let child_ticket = effect_obj(&pending)
        .and_then(|effect| effect.get("child_ticket"))
        .cloned()
        .unwrap_or(Value::Null);
    let failed = event(
        controller,
        Some(state(&pending)),
        "failure",
        obj(vec![
            ("purpose".into(), s(effect_text(&pending, "purpose").unwrap_or(""))),
            ("child_ticket".into(), child_ticket),
            ("sent".into(), b(false)),
            ("error".into(), s("Controlled child authorization failure")),
        ]),
        root,
    );
    check(
        &mut passed,
        "child refresh failure returns scoped failure to its captured parent",
        effect_obj(&failed).is_some()
            && index_is(state(&failed), "0")
            && effect_text(&failed, "wire").unwrap_or("").contains("Authorization exchange did not complete"),
    );
    let (_done, _) = exchange(controller, &failed, peer, root);

    let mut current = sign_in(controller, root, peer, "messages");
    current = event(controller, Some(state(&current)), "profile_edit", empty(), root);
    current = event(
        controller,
        Some(state(&current)),
        "profile_save",
        obj(vec![("hosted_search".into(), s("messages_search_v1"))]),
        root,
    );
    current = event(
        controller,
        Some(state(&current)),
        "send",
        obj(vec![("draft".into(), s("Pause with exact hosted context."))]),
        root,
    );
    current = model_reply(controller, &current, peer, root, &pause_reply());
    let mut paused = expiring(&current);
    let profile = profile_at(&paused, 0).clone();
    set_key(obj_mut(&mut paused, "hosted_request"), "profile", profile);
    let pending = event(controller, Some(&paused), "hosted_continue", empty(), root);
    check(
        &mut passed,
        "paused hosted continuation refreshes before continuation POST",
        effect_text(&pending, "purpose").is_some_and(|purpose| purpose.starts_with("oauth_token:")),
    );
    let (continued, _) = exchange(controller, &pending, peer, root);
    check(
        &mut passed,
        "refreshed hosted continuation retains exact captured server context",
        effect_text(&continued, "purpose") == Some("model")
            && effect_text(&continued, "wire").unwrap_or("").contains("paused-proof"),
    );
    let (done, _) = exchange(controller, &continued, peer, root);
    check(
        &mut passed,
        "refreshed hosted continuation settles without replay",
        !busy(state(&done)) && effect_is_null(&done),
    );

    let authorized = sign_in(controller, root, peer, "chat");
    let config = service_config(&peer.origin);
    let mut current = event(controller, Some(&expiring(&authorized)), "service_connections", empty(), root);
    current = event(
        controller,
        Some(state(&current)),
        "service_save",
        obj(vec![("service_config".into(), s(&config))]),
        root,
    );
    current = event(controller, Some(state(&current)), "voice_review", empty(), root);
    current = event(controller, Some(state(&current)), "voice_consent", empty(), root);
    let session = state(&current).get("voice_consent").cloned().unwrap_or(Value::Null);
    let consented = state(&current).clone();
    current = event(controller, Some(state(&current)), "voice_record", empty(), root);
    let ticket = effect_obj(&current)
        .and_then(|effect| effect.get("media_ticket"))
        .cloned()
        .unwrap_or_else(|| panic!("voice record did not return a media ticket: {}", current.to_string()));
    current = event(
        controller,
        Some(state(&current)),
        "media_input_result",
        obj(vec![
            ("media_ticket".into(), ticket),
            ("name".into(), s("recording.wav")),
            ("data".into(), s(&b64_encode(&audio_bytes()))),
        ]),
        root,
    );
    let (pending, _) = exchange(controller, &current, peer, root);
    check(
        &mut passed,
        "actual voice recognition waits for authorization refresh before model request",
        effect_text(&pending, "purpose").is_some_and(|purpose| purpose.starts_with("oauth_token:"))
            && state(&pending).get("voice_consent") == Some(&session),
    );
    let (sent, _) = exchange(controller, &pending, peer, root);
    check(
        &mut passed,
        "owned token rotation preserves the existing voice consent",
        state(&sent).get("voice_consent") == Some(&session) && effect_text(&sent, "purpose") == Some("model"),
    );
    let (spoken, _) = exchange(controller, &sent, peer, root);
    check(
        &mut passed,
        "refreshed voice answer automatically reaches configured synthesis",
        effect_text(&spoken, "purpose").is_some_and(|purpose| purpose.starts_with("service_request:")),
    );
    let (done, _) = exchange(controller, &spoken, peer, root);
    check(
        &mut passed,
        "actual synthesized audio remains available after model token refresh",
        effect_text(&done, "kind") == Some("media_preview") && effect_text(&done, "media_kind") == Some("audio"),
    );
    let mut forged = consented;
    set_token_text(&mut forged, "access_token", "different-token-same-identity");
    let changed = event(controller, Some(&forged), "status", empty(), root);
    check(
        &mut passed,
        "same identity with an unowned token change revokes voice consent",
        consent_absent(state(&changed)),
    );
    event(controller, Some(state(&done)), "voice_stop", empty(), root);

    let mut current = setup(controller, root, peer, "messages");
    let (started, uri) = browser(controller, state(&current), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    current = exchange(controller, &waiting, peer, root).0;
    current = event(controller, Some(state(&current)), "profile_edit", empty(), root);
    current = event(
        controller,
        Some(state(&current)),
        "profile_save",
        obj(vec![("hosted_search".into(), s("messages_search_v1"))]),
        root,
    );
    current = event(controller, Some(state(&current)), "send", obj(vec![("draft".into(), s("First hosted turn."))]), root);
    current = exchange(controller, &current, peer, root).0;
    let identity = hosted_identity(state(&current));
    let auth_identity = token_str(state(&current), "identity");
    let refreshed = event(controller, Some(state(&current)), "oauth_refresh", empty(), root);
    current = exchange(controller, &refreshed, peer, root).0;
    let sent = event(
        controller,
        Some(state(&current)),
        "send",
        obj(vec![("draft".into(), s("Same authorized account."))]),
        root,
    );
    check(
        &mut passed,
        "refresh preserves owned hosted context for the same authorization identity",
        hosted_identity(state(&sent)) == identity
            && token_str(state(&sent), "identity") == auth_identity
            && effect_text(&sent, "wire").unwrap_or("").contains("account-context-proof"),
    );
    current = exchange(controller, &sent, peer, root).0;
    let (started, uri) = browser(controller, state(&current), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    current = exchange(controller, &waiting, peer, root).0;
    let sent = event(controller, Some(state(&current)), "send", obj(vec![("draft".into(), s("New sign-in session."))]), root);
    check(
        &mut passed,
        "fresh sign-in cannot replay previous account private hosted context",
        hosted_identity(state(&sent)) != identity && !effect_text(&sent, "wire").unwrap_or("").contains("account-context-proof"),
    );
    event(controller, Some(state(&sent)), "cancel", empty(), root);

    let configured = setup(controller, root, peer, "chat");
    let changed = event(
        controller,
        Some(state(&configured)),
        "oauth_sign_in",
        obj(vec![("authorization_endpoint".into(), s(&format!("{}/other-authorization", peer.origin)))]),
        root,
    );
    check(
        &mut passed,
        "unsaved authorization destination cannot receive a sign-in request",
        effect_is_null(&changed) && flag(state(&changed), "notice_error"),
    );

    let configured = setup(controller, root, peer, "chat");
    let (started, uri) = browser(controller, state(&configured), peer, root);
    let cancelled = event(controller, Some(state(&started)), "oauth_cancel", empty(), root);
    let late = event(controller, Some(state(&cancelled)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    check(
        &mut passed,
        "cancelled browser return cannot revive authorization",
        same_state(&late, &cancelled) && effect_is_null(&late),
    );

    let (started, uri) = browser(controller, state(&configured), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    let cancelled = event(controller, Some(state(&waiting)), "oauth_cancel", empty(), root);
    let (_done, callback) = exchange(controller, &waiting, peer, root);
    let late = event(controller, Some(state(&cancelled)), "network_result", callback, root);
    check(
        &mut passed,
        "cancelled network result cannot adopt credentials",
        same_state(&late, &cancelled) && !profile_flag(state(&late), "oauth_enabled"),
    );

    let (started, uri) = browser(controller, state(&configured), peer, root);
    let proof = text_of(state(&started).get("oauth_pending").unwrap_or(&Value::Null), "verifier");
    check(
        &mut passed,
        "browser proof remains out of UI and authorization URL",
        !proof.is_empty()
            && !ui_dump(&started).contains(&proof)
            && !effect_text(&started, "url").unwrap_or("").contains(&proof),
    );
    let resumed = event(controller, Some(state(&started)), "resume", empty(), root);
    check(
        &mut passed,
        "recovery restores only the pending browser flow without network replay",
        effect_is_null(&resumed) && text_is(state(&resumed), "dialog", "oauth_wait"),
    );
    let waiting = event(controller, Some(state(&resumed)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    let (done, _) = exchange(controller, &waiting, peer, root);
    check(
        &mut passed,
        "matching return after activity recovery completes actual code exchange",
        profile_flag(state(&done), "oauth_enabled"),
    );

    let (started, uri) = browser(controller, state(&configured), peer, root);
    let saved = state(&started).clone();
    restart(controller, project);
    let resumed = event(controller, Some(&saved), "resume", empty(), root);
    check(
        &mut passed,
        "new process recovers saved-state browser ownership without reopening browser",
        effect_is_null(&resumed) && text_is(state(&resumed), "dialog", "oauth_wait"),
    );
    let wrong = event(
        controller,
        Some(state(&resumed)),
        "oauth_return",
        obj(vec![("uri".into(), s(&uri.replace("state=", "state=wrong")))]),
        root,
    );
    check(
        &mut passed,
        "recovered flow still refuses wrong state exactly",
        same_state(&wrong, &resumed) && effect_is_null(&wrong),
    );
    let waiting = event(controller, Some(state(&resumed)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    check(
        &mut passed,
        "proof is retired from saved state before code submission",
        is_null(state(&waiting), "oauth_pending"),
    );
    let (done, _) = exchange(controller, &waiting, peer, root);
    check(
        &mut passed,
        "new process exchanges original server code with the original S256 verifier",
        profile_flag(state(&done), "oauth_enabled"),
    );
    let duplicate = event(controller, Some(state(&done)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    check(
        &mut passed,
        "recovered browser return is still single use",
        same_state(&duplicate, &done) && effect_is_null(&duplicate),
    );

    for label in ["expired", "future", "destination", "workspace", "proof"] {
        let (started, uri) = browser(controller, state(&configured), peer, root);
        let mut saved = state(&started).clone();
        match label {
            "expired" => set_key(obj_mut(&mut saved, "oauth_pending"), "created", n("1")),
            "future" => set_key(obj_mut(&mut saved, "oauth_pending"), "created", n(&format!("{}", now_seconds() + 3600))),
            "destination" => set_profile_text(&mut saved, "endpoint", &format!("{}/different-model", peer.origin)),
            "workspace" => set_key(obj_mut(&mut saved, "oauth_pending"), "binding", s("different-workspace")),
            "proof" => set_key(obj_mut(&mut saved, "oauth_pending"), "verifier", s("invalid")),
            _ => unreachable!(),
        }
        let resumed = event(controller, Some(&saved), "resume", empty(), root);
        let late = event(controller, Some(state(&resumed)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
        check(
            &mut passed,
            &format!("{label} browser recovery refuses stale ownership without submitting a code"),
            effect_is_null(&resumed)
                && is_null(state(&resumed), "oauth_pending")
                && text_is(state(&resumed), "dialog", "oauth_settings")
                && same_state(&late, &resumed),
        );
    }

    let authorized = sign_in(controller, root, peer, "chat");
    let pending = event(
        controller,
        Some(&expiring(&authorized)),
        "send",
        obj(vec![("draft".into(), s("Restart during rotating refresh."))]),
        root,
    );
    let saved = state(&pending).clone();
    let lost_purpose = effect_text(&pending, "purpose").unwrap_or("").to_string();
    restart(controller, project);
    let resumed = event(controller, Some(&saved), "resume", empty(), root);
    let retry = event(controller, Some(state(&resumed)), "oauth_refresh", empty(), root);
    check(
        &mut passed,
        "process loss during rotating refresh never replays the saved credential",
        effect_is_null(&resumed) && effect_is_null(&retry) && refresh_uncertain(state(&retry)),
    );
    let late = event(
        controller,
        Some(state(&resumed)),
        "network_result",
        obj(vec![("purpose".into(), s(&lost_purpose)), ("data".into(), s(""))]),
        root,
    );
    check(
        &mut passed,
        "process recovery rejects a lost refresh callback exactly",
        same_state(&late, &resumed) && effect_is_null(&late),
    );

    let (started, uri) = browser(controller, state(&authorized), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    let before = peer.received_len();
    let failed = event(
        controller,
        Some(state(&waiting)),
        "failure",
        obj(vec![
            ("purpose".into(), s(effect_text(&waiting, "purpose").unwrap_or(""))),
            ("sent".into(), b(true)),
            ("error".into(), s("uncertain token submission")),
        ]),
        root,
    );
    check(
        &mut passed,
        "uncertain token submission never replays",
        effect_is_null(&failed) && peer.received_len() == before && !busy(state(&failed)),
    );

    let (started, uri) = browser(controller, state(&authorized), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    peer.set_token_mode("refuse");
    let (refused, _) = exchange(controller, &waiting, peer, root);
    peer.set_token_mode("normal");
    check(
        &mut passed,
        "token refusal preserves working API key and excludes server error body",
        profile_text(state(&refused), "key") == "retained-api-key"
            && !notice_text(state(&refused)).contains("must not enter user notice"),
    );

    let (started, uri) = browser(controller, state(&authorized), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    let signed_in = exchange(controller, &waiting, peer, root).0;
    let (started, uri) = browser(controller, state(&signed_in), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    peer.set_token_mode("no-refresh");
    let (signed_in, _) = exchange(controller, &waiting, peer, root);
    peer.set_token_mode("normal");
    check(
        &mut passed,
        "fresh sign-in never inherits a previous account refresh credential",
        token_str(state(&signed_in), "refresh_token").is_empty(),
    );

    let mut changed = state(&signed_in).clone();
    set_profile_text(&mut changed, "endpoint", &format!("{}/other/chat", peer.origin));
    let sent = event(controller, Some(&changed), "send", obj(vec![("draft".into(), s("Destination changed."))]), root);
    check(
        &mut passed,
        "changed model destination cannot receive an old bearer credential",
        effect_is_null(&sent) && flag(state(&sent), "notice_error"),
    );

    let mut expired = state(&signed_in).clone();
    set_token_text(&mut expired, "expires_at", "1");
    let sent = event(controller, Some(&expired), "send", obj(vec![("draft".into(), s("Expired authorization."))]), root);
    check(
        &mut passed,
        "known token expiry is scoped before model submission",
        effect_is_null(&sent) && flag(state(&sent), "notice_error"),
    );
    let api = event(controller, Some(state(&sent)), "oauth_key_mode", empty(), root);
    let sent = event(
        controller,
        Some(state(&api)),
        "send",
        obj(vec![("draft".into(), s("API-key mode still available."))]),
        root,
    );
    check(
        &mut passed,
        "expired optional authorization does not destroy API-key capability",
        effect_obj(&sent).is_some() && effect_text(&sent, "wire").unwrap_or("").contains("retained-api-key"),
    );
    event(controller, Some(state(&sent)), "cancel", empty(), root);

    passed
}

fn setup(controller: &mut Controller, root: &str, peer: &Peer, format: &str) -> Value {
    let resumed = event(controller, None, "resume", empty(), root);
    let created = event(controller, Some(state(&resumed)), "profile_new", empty(), root);
    let saved = event(
        controller,
        Some(state(&created)),
        "profile_save",
        obj(vec![
            ("name".into(), s("Private server")),
            ("protocol".into(), s(format)),
            ("endpoint".into(), s(&format!("{}/model/{format}", peer.origin))),
            ("model".into(), s("controlled")),
            ("key".into(), s("retained-api-key")),
            ("stream".into(), b(false)),
        ]),
        root,
    );
    require_ok(&saved, "profile_save");
    let settings = event(controller, Some(state(&saved)), "oauth_settings", empty(), root);
    let saved = event(
        controller,
        Some(state(&settings)),
        "oauth_save",
        obj(vec![
            ("authorization_endpoint".into(), s(&format!("{}/authorize", peer.origin))),
            ("token_endpoint".into(), s(&format!("{}/token", peer.origin))),
            ("client_id".into(), s("public-client")),
            ("scope".into(), s("conversation offline")),
            ("issuer".into(), s(&peer.origin)),
            ("plain".into(), b(true)),
        ]),
        root,
    );
    require_ok(&saved, "oauth_save");
    saved
}

fn browser(controller: &mut Controller, current: &Value, peer: &Peer, root: &str) -> (Value, String) {
    let started = event(controller, Some(current), "oauth_sign_in", empty(), root);
    let effect = effect_obj(&started).unwrap_or_else(|| {
        panic!("sign-in did not open a browser: {}", notice_text(state(&started)))
    });
    if effect_text(&started, "kind") != Some("oauth_browser") {
        panic!("sign-in effect was not a browser: {}", effect.to_string());
    }
    let url = effect_text(&started, "url").unwrap_or("").to_string();
    let location = peer.fetch_redirect(&url);
    (started, location)
}

fn exchange(controller: &mut Controller, value: &Value, _peer: &Peer, root: &str) -> (Value, Value) {
    let effect = effect_obj(value).unwrap_or_else(|| {
        panic!("missing network effect; notice={}", notice_text(state(value)))
    });
    if effect_text(value, "kind") != Some("network") {
        panic!("expected a network effect: {}", effect.to_string());
    }
    if !matches!(effect.get("tls"), Some(Value::Bool(false))) {
        panic!("authorization peer refuses TLS: {}", effect.to_string());
    }
    let host = effect_text(value, "host").unwrap_or("").to_string();
    let port = number(effect, "port") as u16;
    let mut socket = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    socket.set_read_timeout(Some(Duration::from_secs(20))).unwrap_or_else(|error| panic!("read timeout: {error}"));
    socket.set_write_timeout(Some(Duration::from_secs(20))).unwrap_or_else(|error| panic!("write timeout: {error}"));
    let _ = socket.set_nodelay(true);
    socket.write_all(&wire_bytes(effect)).unwrap_or_else(|error| panic!("request write: {error}"));
    upload(controller, effect, &mut socket);
    let (receipt, output) = if text_of(effect, "media_download").is_empty() {
        (None, read_to_end(&mut socket))
    } else {
        (Some(stream_download(controller, effect, &mut socket)), Vec::new())
    };
    let purpose = effect_text(value, "purpose").unwrap_or("").to_string();
    let mut fields = vec![("purpose".into(), s(&purpose))];
    if let Some(receipt) = receipt {
        fields.push(("media_receipt".into(), receipt));
    } else {
        fields.push(("data".into(), s(&b64_encode(&output))));
    }
    for key in ["model_ticket", "child_ticket", "child_owner"] {
        if let Some(item) = effect.get(key) {
            if !matches!(item, Value::Null) {
                fields.push((key.to_string(), item.clone()));
            }
        }
    }
    let callback = obj(fields);
    let result = event(controller, Some(state(value)), "network_result", callback.clone(), root);
    (result, callback)
}

fn upload(controller: &mut Controller, effect: &Value, socket: &mut TcpStream) {
    let ticket = text_of(effect, "media_upload");
    if ticket.is_empty() {
        return;
    }
    let purpose = text_of(effect, "purpose");
    let expected = number(effect, "upload_bytes") as usize;
    let mut count = 0usize;
    let mut failure = None;
    loop {
        let reply = controller.op(&obj(vec![
            ("op".into(), s("media_upload_read")),
            ("purpose".into(), s(&purpose)),
            ("ticket".into(), s(&ticket)),
        ]));
        let data = b64_decode(text_of(&reply, "data").as_str());
        if data.len() > 8192 {
            failure = Some(format!("upload chunk {} exceeds 8192", data.len()));
            break;
        }
        count += data.len();
        if count > expected {
            failure = Some(format!("upload count {count} exceeds {expected}"));
            break;
        }
        if let Err(error) = socket.write_all(&data) {
            failure = Some(format!("upload write: {error}"));
            break;
        }
        let done = match reply.get("done") {
            Some(Value::Bool(done)) => *done,
            _ => {
                failure = Some(format!("upload part missing done: {}", reply.to_string()));
                break;
            }
        };
        if done {
            if count != expected {
                failure = Some(format!("upload ended at {count}, expected {expected}"));
            }
            break;
        }
        if data.is_empty() {
            failure = Some("empty upload chunk before done".to_string());
            break;
        }
    }
    controller.op(&obj(vec![
        ("op".into(), s("media_upload_cancel")),
        ("purpose".into(), s(&purpose)),
        ("ticket".into(), s(&ticket)),
    ]));
    if let Some(error) = failure {
        panic!("upload: {error}");
    }
}

fn stream_download(controller: &mut Controller, effect: &Value, socket: &mut TcpStream) -> Value {
    let purpose = text_of(effect, "purpose");
    let ticket = text_of(effect, "media_download");
    let mut ready = false;
    let mut failure = None;
    let mut buffer = [0u8; 8192];
    loop {
        let count = match socket.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                failure = Some(format!("media read: {error}"));
                break;
            }
        };
        let reply = controller.op(&obj(vec![
            ("op".into(), s("media_stream_feed")),
            ("purpose".into(), s(&purpose)),
            ("ticket".into(), s(&ticket)),
            ("data".into(), s(&b64_encode(&buffer[..count]))),
        ]));
        match reply.get("ready") {
            Some(Value::Bool(is_ready)) => ready = *is_ready,
            _ => {
                failure = Some(format!("media feed missing ready: {}", reply.to_string()));
                break;
            }
        }
        if ready {
            break;
        }
    }
    let receipt = if failure.is_none() {
        Some(controller.op(&obj(vec![
            ("op".into(), s("media_stream_finish")),
            ("purpose".into(), s(&purpose)),
            ("ticket".into(), s(&ticket)),
            ("eof".into(), b(!ready)),
        ])))
    } else {
        None
    };
    controller.op(&obj(vec![
        ("op".into(), s("media_stream_cancel")),
        ("purpose".into(), s(&purpose)),
        ("ticket".into(), s(&ticket)),
    ]));
    if let Some(error) = failure {
        panic!("download: {error}");
    }
    receipt.unwrap_or(Value::Null)
}

fn read_to_end(socket: &mut TcpStream) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match socket.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("socket read: {error}"),
        }
    }
    output
}

fn sign_in(controller: &mut Controller, root: &str, peer: &Peer, format: &str) -> Value {
    let configured = setup(controller, root, peer, format);
    let (started, uri) = browser(controller, state(&configured), peer, root);
    let waiting = event(controller, Some(state(&started)), "oauth_return", obj(vec![("uri".into(), s(&uri))]), root);
    exchange(controller, &waiting, peer, root).0
}

fn expiring(value: &Value) -> Value {
    let mut current = state(value).clone();
    set_token_text(&mut current, "expires_at", &format!("{}", now_seconds() + 20));
    current
}

fn child_review(controller: &mut Controller, root: &str, peer: &Peer) -> Value {
    let authorized = sign_in(controller, root, peer, "chat");
    let mut current = state(&authorized).clone();
    set_profile_text(&mut current, "tool_format", "structured");
    let mut child = profile_at(&current, 0).clone();
    set_key(&mut child, "name", s("Child"));
    set_key(&mut child, "model", s("child-model"));
    set_key(obj_mut(&mut child, "oauth_tokens"), "expires_at", n(&format!("{}", now_seconds() + 20)));
    match obj_mut(&mut current, "profiles") {
        Value::Arr(items) => items.push(child),
        _ => panic!("profiles are not an array"),
    }
    set_key(
        &mut current,
        "agents",
        Value::Arr(vec![
            obj(vec![
                ("name".into(), s("Parent")),
                ("model".into(), s("0")),
                ("tools".into(), s("read,delegate")),
                ("subagents".into(), Value::Arr(vec![n("1")])),
            ]),
            obj(vec![
                ("name".into(), s("Child")),
                ("model".into(), s("1")),
                ("tools".into(), s("read")),
            ]),
        ]),
    );
    let sent = event(
        controller,
        Some(&current),
        "send",
        obj(vec![("draft".into(), s("Ask the configured child."))]),
        root,
    );
    model_reply(controller, &sent, peer, root, &tool_call_reply())
}

fn model_reply(controller: &mut Controller, value: &Value, peer: &Peer, root: &str, body: &[u8]) -> Value {
    peer.set_model_reply(body.to_vec());
    exchange(controller, value, peer, root).0
}

fn restart(controller: &mut Controller, project: &str) {
    let previous = std::mem::replace(controller, Controller::spawn(project));
    previous.finish();
}

fn event(controller: &mut Controller, state: Option<&Value>, action: &str, inputs: Value, root: &str) -> Value {
    controller.event(state, action, inputs, root)
}

fn empty() -> Value {
    obj(vec![])
}

fn require_ok(value: &Value, step: &str) {
    if flag(state(value), "notice_error") {
        panic!("{step}: {}", notice_text(state(value)));
    }
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn same_state(left: &Value, right: &Value) -> bool {
    state(left) == state(right)
}

fn effect_obj(value: &Value) -> Option<&Value> {
    match value.get("effect") {
        Some(effect) if matches!(effect, Value::Obj(_)) => Some(effect),
        _ => None,
    }
}

fn effect_is_null(value: &Value) -> bool {
    matches!(value.get("effect"), None | Some(Value::Null))
}

fn effect_text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    effect_obj(value).and_then(|effect| match effect.get(key) {
        Some(Value::Str(text)) => Some(text.as_str()),
        _ => None,
    })
}

fn effect_dump(value: &Value) -> String {
    value.get("effect").map(Value::to_string).unwrap_or_else(|| "null".to_string())
}

fn ui_dump(value: &Value) -> String {
    value.get("ui").map(Value::to_string).unwrap_or_default()
}

fn profile_at(current: &Value, index: usize) -> &Value {
    match current.get("profiles") {
        Some(Value::Arr(items)) => items.get(index).unwrap_or_else(|| panic!("missing profile {index}")),
        _ => panic!("missing profiles"),
    }
}

fn profile_text(current: &Value, key: &str) -> String {
    text_of(profile_at(current, 0), key)
}

fn profile_flag(current: &Value, key: &str) -> bool {
    matches!(profile_at(current, 0).get(key), Some(Value::Bool(true)))
}

fn token_str(current: &Value, key: &str) -> String {
    match profile_at(current, 0).get("oauth_tokens") {
        Some(tokens) => text_of(tokens, key),
        None => String::new(),
    }
}

fn hosted_identity(current: &Value) -> Value {
    profile_at(current, 0).get("hosted_identity").cloned().unwrap_or(Value::Null)
}

fn text_of(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(text)) => text.clone(),
        _ => String::new(),
    }
}

fn text_is(value: &Value, key: &str, expected: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expected)
}

fn flag(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Bool(true)))
}

fn is_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn busy(value: &Value) -> bool {
    flag(value, "busy")
}

fn notice_text(value: &Value) -> String {
    text_of(value, "notice")
}

fn refresh_uncertain(current: &Value) -> bool {
    match profile_at(current, 0).get("oauth_tokens").and_then(|tokens| tokens.get("refresh_uncertain")) {
        Some(Value::Bool(true)) => true,
        _ => false,
    }
}

fn index_is(current: &Value, expected: &str) -> bool {
    match current.get("request_index") {
        Some(Value::Num(number)) | Some(Value::Str(number)) => number == expected,
        _ => false,
    }
}

fn stack_len(current: &Value) -> usize {
    match current.get("delegation_stack") {
        Some(Value::Arr(items)) => items.len(),
        _ => 0,
    }
}

fn stack_empty(current: &Value) -> bool {
    match current.get("delegation_stack") {
        Some(Value::Arr(items)) => items.is_empty(),
        Some(Value::Null) | None => true,
        _ => false,
    }
}

fn consent_absent(current: &Value) -> bool {
    match current.get("voice_consent") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::Str(text)) => text.is_empty(),
        _ => false,
    }
}

fn number(value: &Value, key: &str) -> u64 {
    match value.get(key) {
        Some(Value::Num(number)) | Some(Value::Str(number)) => number.parse().unwrap_or_else(|error| panic!("invalid number {number}: {error}")),
        other => panic!("missing number {key}: {}", other.map(Value::to_string).unwrap_or_default()),
    }
}

fn set_key(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(slot) = fields.iter_mut().find(|(name, _)| name == key) {
        slot.1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn obj_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    panic!("missing {key}");
}

fn set_profile_text(current: &mut Value, key: &str, value: &str) {
    let profile = match obj_mut(current, "profiles") {
        Value::Arr(items) => items.get_mut(0).unwrap_or_else(|| panic!("missing profile")),
        _ => panic!("profiles are not an array"),
    };
    set_key(profile, key, s(value));
}

fn set_token_text(current: &mut Value, key: &str, value: &str) {
    let profile = match obj_mut(current, "profiles") {
        Value::Arr(items) => items.get_mut(0).unwrap_or_else(|| panic!("missing profile")),
        _ => panic!("profiles are not an array"),
    };
    set_key(obj_mut(profile, "oauth_tokens"), key, if key == "expires_at" { n(value) } else { s(value) });
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    if let Some(Value::Str(encoded)) = effect.get("wire_base64") {
        return b64_decode(encoded);
    }
    effect.get("wire").map(|value| value.str_().as_bytes().to_vec()).unwrap_or_default()
}

fn query_of(value: &Value) -> std::collections::BTreeMap<String, Vec<String>> {
    let url = effect_text(value, "url").unwrap_or("");
    let query = url.split_once('?').map(|(_, query)| query).unwrap_or("");
    parse_query(query)
}

fn parse_query(text: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut result: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for item in text.split('&').filter(|item| !item.is_empty()) {
        let (key, value) = item.split_once('=').unwrap_or((item, ""));
        result.entry(percent_decode(key)).or_default().push(percent_decode(value));
    }
    result
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = &text[index + 1..index + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    // urllib.parse leaves an escape it cannot decode exactly as written.
                    Err(_) => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

fn encode_query(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn s256(verifier: &str) -> String {
    b64_encode(&sha256_digest(verifier.as_bytes()))
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

fn urlsafe_token(nbytes: usize) -> String {
    let mut bytes = vec![0u8; nbytes];
    let mut file = std::fs::File::open("/dev/urandom").unwrap_or_else(|error| panic!("urandom: {error}"));
    file.read_exact(&mut bytes).unwrap_or_else(|error| panic!("urandom read: {error}"));
    b64_encode(&bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("system clock is before the unix epoch: {error}"))
        .as_secs()
}

fn check(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn tool_call_reply() -> Vec<u8> {
    obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            (
                "message".into(),
                obj(vec![
                    ("content".into(), s("")),
                    (
                        "tool_calls".into(),
                        Value::Arr(vec![obj(vec![
                            ("id".into(), s("owned-delegate")),
                            ("type".into(), s("function")),
                            (
                                "function".into(),
                                obj(vec![
                                    ("name".into(), s("delegate")),
                                    ("arguments".into(), s("{\"agent\": 1, \"task\": \"Return exact child evidence.\"}")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
            ("finish_reason".into(), s("tool_calls")),
        ])]),
    )])
    .to_string()
    .into_bytes()
}

fn evidence_reply() -> Vec<u8> {
    obj(vec![(
        "choices".into(),
        Value::Arr(vec![obj(vec![
            ("message".into(), obj(vec![("content".into(), s("EXACT_OWNED_CHILD_EVIDENCE"))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )])
    .to_string()
    .into_bytes()
}

fn pause_reply() -> Vec<u8> {
    obj(vec![
        ("role".into(), s("assistant")),
        (
            "content".into(),
            Value::Arr(vec![
                obj(vec![
                    ("type".into(), s("thinking")),
                    ("thinking".into(), s("Exact private paused context.")),
                    ("signature".into(), s("paused-proof")),
                ]),
                obj(vec![
                    ("type".into(), s("text")),
                    ("text".into(), s("Paused response.")),
                ]),
            ]),
        ),
        ("stop_reason".into(), s("pause_turn")),
    ])
    .to_string()
    .into_bytes()
}

fn default_reply(format: &str) -> Vec<u8> {
    let body = match format {
        "chat" => obj(vec![(
            "choices".into(),
            Value::Arr(vec![obj(vec![
                ("message".into(), obj(vec![("content".into(), s("Actual model peer response."))])),
                ("finish_reason".into(), s("stop")),
            ])]),
        )]),
        "local" => obj(vec![
            ("message".into(), obj(vec![("content".into(), s("Actual model peer response."))])),
            ("done".into(), b(true)),
        ]),
        "messages" => obj(vec![
            ("role".into(), s("assistant")),
            (
                "content".into(),
                Value::Arr(vec![
                    obj(vec![
                        ("type".into(), s("thinking")),
                        ("thinking".into(), s("Private account context.")),
                        ("signature".into(), s("account-context-proof")),
                    ]),
                    obj(vec![
                        ("type".into(), s("text")),
                        ("text".into(), s("Actual model peer response.")),
                    ]),
                ]),
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
                        Value::Arr(vec![obj(vec![("text".into(), s("Actual model peer response."))])]),
                    )]),
                ),
                ("finishReason".into(), s("STOP")),
            ])]),
        )]),
        _ => panic!("unknown model format {format}"),
    };
    body.to_string().into_bytes()
}

fn service_config(origin: &str) -> String {
    obj(vec![
        ("version".into(), n("1")),
        (
            "hosts".into(),
            Value::Arr(vec![obj(vec![
                ("id".into(), s("audio")),
                ("url".into(), s(origin)),
                ("plain".into(), b(true)),
                ("enabled".into(), b(true)),
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
                    ("enabled".into(), b(true)),
                ]),
                obj(vec![
                    ("id".into(), s("synthesis")),
                    ("host".into(), s("audio")),
                    ("capability".into(), s("speech_synthesis")),
                    ("path".into(), s("/speak")),
                    ("method".into(), s("POST")),
                    ("request_format".into(), s("json")),
                    (
                        "request".into(),
                        obj(vec![(
                            "text".into(),
                            obj(vec![("$input".into(), s("/text"))]),
                        )]),
                    ),
                    ("response".into(), s("binary")),
                    ("response_path".into(), s("")),
                    ("artifact_kind".into(), s("audio")),
                    ("enabled".into(), b(true)),
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
    ])
    .to_string()
}

fn audio_bytes() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36u32 + 3200).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&16000u32.to_le_bytes());
    data.extend_from_slice(&32000u32.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&3200u32.to_le_bytes());
    data.extend_from_slice(&[0u8; 3200]);
    data
}

struct Peer {
    origin: String,
    inner: Arc<PeerInner>,
}

struct PeerInner {
    origin: String,
    codes: Mutex<std::collections::HashMap<String, (String, String, String)>>,
    refreshes: Mutex<std::collections::HashMap<String, String>>,
    received: Mutex<usize>,
    token_mode: Mutex<String>,
    model_reply: Mutex<Option<Vec<u8>>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("peer bind: {error}"));
        let origin = format!("http://{}", listener.local_addr().unwrap_or_else(|error| panic!("peer address: {error}")));
        let inner = Arc::new(PeerInner {
            origin: origin.clone(),
            codes: Mutex::new(std::collections::HashMap::new()),
            refreshes: Mutex::new(std::collections::HashMap::new()),
            received: Mutex::new(0),
            token_mode: Mutex::new("normal".to_string()),
            model_reply: Mutex::new(None),
        });
        let server = Arc::clone(&inner);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let server = Arc::clone(&server);
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                    if let Err(error) = server.handle(&mut stream) {
                        eprintln!("oauth peer: {error}");
                    }
                });
            }
        });
        Self { origin, inner }
    }

    fn received_len(&self) -> usize {
        *self.inner.received.lock().unwrap_or_else(|error| panic!("received lock: {error}"))
    }

    fn set_token_mode(&self, mode: &str) {
        *self.inner.token_mode.lock().unwrap_or_else(|error| panic!("token mode lock: {error}")) = mode.to_string();
    }

    fn set_model_reply(&self, body: Vec<u8>) {
        *self.inner.model_reply.lock().unwrap_or_else(|error| panic!("model reply lock: {error}")) = Some(body);
    }

    fn fetch_redirect(&self, url: &str) -> String {
        let rest = url.strip_prefix("http://").unwrap_or_else(|| panic!("authorization URL is not http: {url}"));
        let (authority, path) = rest.split_once('/').unwrap_or_else(|| panic!("authorization URL has no path: {url}"));
        let mut stream = TcpStream::connect(authority).unwrap_or_else(|error| panic!("authorize connect: {error}"));
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap_or_else(|error| panic!("authorize timeout: {error}"));
        let request = format!("GET /{path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap_or_else(|error| panic!("authorize write: {error}"));
        let mut response = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => response.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("authorize read: {error}"),
            }
        }
        let text = String::from_utf8_lossy(&response);
        let status = text.lines().next().unwrap_or("");
        if !status.contains(" 302 ") {
            panic!("authorization redirect status was {status}");
        }
        text.lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("location") {
                    Some(value.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("authorization redirect has no Location"))
    }
}

impl PeerInner {
    fn handle(&self, stream: &mut TcpStream) -> Result<(), String> {
        let (header, body) = read_request(stream)?;
        // The leg records a request after parsing it, so an accepted connection that never
        // delivered a request must not raise the count a check compares against.
        *self.received.lock().map_err(|_| "received lock".to_string())? += 1;
        let request = header.lines().next().unwrap_or("");
        let target = request.split_whitespace().nth(1).unwrap_or("/");
        let path = target.split('?').next().unwrap_or("/");
        if path == "/authorize" {
            let query = target.split_once('?').map(|(_, query)| query).unwrap_or("");
            let location = self.authorize(query)?;
            write_redirect(stream, &location)?;
            return Ok(());
        }
        if path == "/token" {
            let (status, payload) = self.token(&body)?;
            write_bytes(stream, status, if status == 200 { "OK" } else { "Unauthorized" }, &payload)?;
            return Ok(());
        }
        if path == "/recognize" {
            write_bytes(stream, 200, "OK", &obj(vec![("text".into(), s("Actual recognized voice question."))]).to_string().into_bytes())?;
            return Ok(());
        }
        if path == "/speak" {
            write_bytes(stream, 200, "OK", &audio_bytes())?;
            return Ok(());
        }
        let format = path.rsplit('/').next().unwrap_or("");
        let payload = self
            .model_reply
            .lock()
            .map_err(|_| "model reply lock".to_string())?
            .take()
            .unwrap_or_else(|| default_reply(format));
        write_bytes(stream, 200, "OK", &payload)
    }

    fn authorize(&self, query: &str) -> Result<String, String> {
        let values = parse_query(query);
        let method = values.get("code_challenge_method").and_then(|items| items.first()).cloned().unwrap_or_default();
        let response = values.get("response_type").and_then(|items| items.first()).cloned().unwrap_or_default();
        if method != "S256" || response != "code" || values.contains_key("code_verifier") {
            return Err("authorization request is not an S256 code grant".to_string());
        }
        // The leg indexes q['state'][0], q['code_challenge'][0], q['redirect_uri'][0] and
        // q['client_id'][0]: a missing parameter raises in the peer and the subject gets no
        // redirect. Defaulting them here would let the fixture invent a challenge the subject
        // never advertised and still pass the exchange.
        let required = |name: &str| -> Result<String, String> {
            values
                .get(name)
                .and_then(|items| items.first())
                .cloned()
                .ok_or_else(|| format!("authorization request omits {name}"))
        };
        let state = required("state")?;
        let challenge = required("code_challenge")?;
        let redirect = required("redirect_uri")?;
        let client = required("client_id")?;
        let code = urlsafe_token(24);
        self.codes
            .lock()
            .map_err(|_| "code lock".to_string())?
            .insert(code.clone(), (challenge, redirect.clone(), client));
        Ok(format!(
            "{redirect}?code={}&state={}&iss={}",
            encode_query(&code),
            encode_query(&state),
            encode_query(&self.origin)
        ))
    }

    fn token(&self, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let form = parse_query(&String::from_utf8_lossy(body));
        let mode = self.token_mode.lock().map_err(|_| "token mode lock".to_string())?.clone();
        if mode == "refuse" {
            let payload = obj(vec![
                ("error".into(), s("invalid_grant")),
                ("error_description".into(), s("must not enter user notice")),
            ]);
            return Ok((401, payload.to_string().into_bytes()));
        }
        let grant = form.get("grant_type").and_then(|items| items.first()).cloned().unwrap_or_default();
        let client = form.get("client_id").and_then(|items| items.first()).cloned().unwrap_or_default();
        if grant == "authorization_code" {
            let code = form.get("code").and_then(|items| items.first()).cloned().unwrap_or_default();
            let verifier = form.get("code_verifier").and_then(|items| items.first()).cloned().unwrap_or_default();
            let redirect = form.get("redirect_uri").and_then(|items| items.first()).cloned().unwrap_or_default();
            let saved = self.codes.lock().map_err(|_| "code lock".to_string())?.remove(&code);
            let Some((challenge, expected_redirect, expected_client)) = saved else {
                return Ok((400, obj(vec![("error".into(), s("invalid_grant"))]).to_string().into_bytes()));
            };
            if s256(&verifier) != challenge || redirect != expected_redirect || client != expected_client {
                return Ok((400, obj(vec![("error".into(), s("invalid_grant"))]).to_string().into_bytes()));
            }
        } else if grant == "refresh_token" {
            let refresh = form.get("refresh_token").and_then(|items| items.first()).cloned().unwrap_or_default();
            let saved = self.refreshes.lock().map_err(|_| "refresh lock".to_string())?.remove(&refresh);
            if saved.as_deref() != Some(client.as_str()) {
                return Ok((400, obj(vec![("error".into(), s("invalid_grant"))]).to_string().into_bytes()));
            }
        } else {
            return Err(format!("unsupported grant {grant}"));
        }
        let access = urlsafe_token(32);
        let refresh = urlsafe_token(32);
        self.refreshes
            .lock()
            .map_err(|_| "refresh lock".to_string())?
            .insert(refresh.clone(), client);
        let mut fields = vec![
            ("token_type".into(), s("Bearer")),
            ("access_token".into(), s(&access)),
            ("expires_in".into(), n("3600")),
        ];
        if mode != "no-refresh" {
            fields.push(("refresh_token".into(), s(&refresh)));
        }
        Ok((200, obj(fields).to_string().into_bytes()))
    }
}

fn read_request(stream: &mut TcpStream) -> Result<(String, Vec<u8>), String> {
    let mut buffer = Vec::new();
    let mut temporary = [0u8; 8192];
    let header_end = loop {
        let count = read_chunk(stream, &mut temporary)?;
        if count == 0 {
            return Err("request ended before headers".to_string());
        }
        buffer.extend_from_slice(&temporary[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > 1_048_576 {
            return Err("request headers are too large".to_string());
        }
    };
    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let length = content_length(&header);
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let count = read_chunk(stream, &mut temporary)?;
        if count == 0 {
            return Err("request ended before the body".to_string());
        }
        body.extend_from_slice(&temporary[..count]);
    }
    body.truncate(length);
    Ok((header, body))
}

fn read_chunk(stream: &mut TcpStream, buffer: &mut [u8]) -> Result<usize, String> {
    loop {
        match stream.read(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("request read: {error}")),
        }
    }
}

fn content_length(header: &str) -> usize {
    header
        .lines()
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

fn write_redirect(stream: &mut TcpStream, location: &str) -> Result<(), String> {
    let header = format!("HTTP/1.0 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(header.as_bytes()).map_err(|error| format!("redirect write: {error}"))?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn write_bytes(stream: &mut TcpStream, status: u16, reason: &str, body: &[u8]) -> Result<(), String> {
    let header = format!("HTTP/1.0 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(header.as_bytes()).map_err(|error| format!("response write: {error}"))?;
    stream.write_all(body).map_err(|error| format!("response body: {error}"))?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

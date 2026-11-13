mod twin;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use twin::{b64_decode, b64_encode, s, Controller, Value};

const SCOPE: &str = "Storage-owned public-client grants, actual HTTP storage and model isolation";
const REPORT: &str = "output/checks/storage_oauth.json";

const CHECKS: [&str; 78] = [
    "storage authorization configures without any model",
    "network connection exposes actual authorization route",
    "storage authorization has its own visible owner",
    "authorization review identifies credential use and destination",
    "storage browser flow uses a fresh external S256 request",
    "wrong storage return is an exact no-op",
    "storage token transport owns one exact request",
    "real authorization adopts tokens only into storage",
    "storage grant retains every manually configured credential",
    "authorization UI omits access and refresh tokens",
    "storage token callback cannot adopt twice",
    "network settings show active authorization without secrets",
    "real WebDAV listing uses storage token and no manual header",
    "real authorized read completes ordinary viewer",
    "authorized writes retain ordinary user review",
    "authorized conditional write and exact readback complete",
    "configuration export omits manual and authorization credentials",
    "explicit manual-mode choice restores configured key without losing grant",
    "expired storage grant refuses only its request with no credential fallback",
    "storage refresh marks only its own uncertain credential",
    "real storage refresh rotates credentials and retains manual key",
    "chat primary request never borrows storage credentials",
    "chat real primary reply completes with storage still authorized",
    "chat starting storage authorization cannot replace a primary operation",
    "local primary request never borrows storage credentials",
    "local real primary reply completes with storage still authorized",
    "local starting storage authorization cannot replace a primary operation",
    "messages primary request never borrows storage credentials",
    "messages real primary reply completes with storage still authorized",
    "messages starting storage authorization cannot replace a primary operation",
    "content primary request never borrows storage credentials",
    "content real primary reply completes with storage still authorized",
    "content starting storage authorization cannot replace a primary operation",
    "model authorization explicitly selects its own owner after storage settings",
    "model grant does not overwrite storage credentials",
    "storage request never borrows an authorized model token",
    "authorized model still completes while storage was selected",
    "automatic model refresh ignores selected storage settings",
    "automatic model refresh resumes the exact primary turn",
    "waiting optional storage sign-in does not block the primary model",
    "primary reply completes while an unfinished storage sign-in is retired",
    "retired storage sign-in cannot adopt after the primary reply",
    "confirmed storage pre-send failure permits explicit retry",
    "uncertain storage refresh is never replayed automatically or explicitly",
    "cancelled storage browser return cannot change credentials",
    "storage browser recovery retains its typed owner",
    "recovered storage browser exchange updates storage only",
    "changed storage destination refuses old browser callback without transport",
    "saved grant never follows a changed storage destination or falls back",
    "device-code storage authorization reaches its owned review",
    "storage device polling respects server interval",
    "actual storage device grant adopts only storage credentials",
    "real microphone owner accepts PCM before changing authorization",
    "opening storage authorization retires the microphone without a network request",
    "retired microphone cannot accept more PCM",
    "old voice callback cannot restart capture from authorization settings",
    "invalid authorization mode never selects manual credentials 'true'",
    "invalid authorization mode never selects manual credentials 1",
    "invalid authorization mode never selects manual credentials {}",
    "invalid authorization mode never selects manual credentials []",
    "legacy write uses pinned storage authorization only",
    "legacy write completes at actual authorized storage peer",
    "legacy write refuses a changed pinned storage credential",
    "legacy probe uses pinned storage authorization only",
    "legacy probe completes at actual authorized storage peer",
    "legacy probe refuses a changed pinned storage credential",
    "legacy verify uses pinned storage authorization only",
    "legacy verify completes at actual authorized storage peer",
    "legacy verify refuses a changed pinned storage credential",
    "streamed write uses pinned storage authorization only",
    "streamed write completes at actual authorized storage peer",
    "streamed write refuses a changed pinned storage credential",
    "streamed probe uses pinned storage authorization only",
    "streamed probe completes at actual authorized storage peer",
    "streamed probe refuses a changed pinned storage credential",
    "streamed verify uses pinned storage authorization only",
    "streamed verify completes at actual authorized storage peer",
    "streamed verify refuses a changed pinned storage credential",
];

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let peer = Peer::start();
    let mut controller = Controller::spawn(&root_text);
    let mut passed: Vec<String> = Vec::new();

    let current = setup(&mut controller, &root_text, &peer);
    let current = check_configuration(&mut controller, &root_text, &current, &mut passed);
    let current = authorize(&mut controller, &root_text, &current, &mut passed);
    check_storage_requests(&mut controller, &root_text, &current, &peer, &mut passed);
    let current = check_manual_and_refresh(&mut controller, &root_text, &current, &peer, &mut passed);
    let current = check_model_isolation(&mut controller, &root_text, &current, &peer, &mut passed);
    let current = check_refresh_failures(&mut controller, &root_text, &current, &peer, &mut passed);
    let current = check_browser_recovery(&mut controller, &root_text, &current, &peer, &mut passed);
    let current = check_device_authorization(&mut controller, &root_text, &current, &mut passed);
    check_voice_lifecycle(&mut controller, &root_text, &current, &peer, &mut passed);
    check_invalid_modes(&mut controller, &root_text, &current, &peer, &mut passed);
    check_project_transfers(&mut controller, &root_text, &current, &peer, &mut passed);

    controller.finish();
    if passed.len() != CHECKS.len() {
        panic!("emitted {} checks, expected {}", passed.len(), CHECKS.len());
    }
    let body = twin::obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
    ]);
    twin::report_value("storage_oauth", &body, true);
    let written =
        std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains("\"passed\": 78") {
        panic!("{REPORT} has the wrong passed count for {SCOPE}");
    }
    // The leg prints a different shape from the one it writes: the names list under
    // "passed", with no count and no scope, so the printed channel is built separately.
    println!(
        "{}",
        twin::pretty(&twin::obj(vec![(
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        )]))
    );
}

fn setup(controller: &mut Controller, root: &str, peer: &Peer) -> Value {
    let origin = peer.origin();
    let mut current = event(controller, None, "resume", twin::obj(vec![]), root);
    current = event(
        controller,
        Some(state(&current)),
        "drive_edit",
        twin::obj(vec![]),
        root,
    );
    current = event(
        controller,
        Some(state(&current)),
        "drive_save",
        twin::obj(vec![
            ("url".into(), s(&format!("{origin}/share/"))),
            ("plain".into(), twin::b(true)),
            ("user".into(), s("retained-user")),
            ("password".into(), s("retained-password")),
            ("key".into(), s("retained-key")),
            ("auth_header".into(), s("X-Storage-Key")),
            ("auth_prefix".into(), s("Manual")),
        ]),
        root,
    );
    current = event(
        controller,
        Some(state(&current)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    event(
        controller,
        Some(state(&current)),
        "oauth_save",
        twin::obj(vec![
            ("authorization_endpoint".into(), s(&format!("{origin}/authorize"))),
            ("device_endpoint".into(), s(&format!("{origin}/device"))),
            ("token_endpoint".into(), s(&format!("{origin}/token"))),
            ("client_id".into(), s("public-client")),
            ("scope".into(), s("storage")),
            ("issuer".into(), s(origin)),
            ("plain".into(), twin::b(true)),
        ]),
        root,
    )
}

fn check_configuration(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    passed: &mut Vec<String>,
) -> Value {
    keep(
        passed,
        &CHECKS[0],
        field(state(current), "profiles").arr().is_empty()
            && text(field(state(current), "oauth_target")) == "storage"
            && absent_or_false(state(current), "notice_error"),
    );
    let opened = event(
        controller,
        Some(state(current)),
        "drive_edit",
        twin::obj(vec![]),
        root,
    );
    let buttons = field(field(&opened, "ui"), "modal")
        .get("buttons")
        .unwrap_or_else(|| panic!("missing buttons"))
        .arr();
    keep(
        passed,
        &CHECKS[1],
        buttons.iter().any(|button| {
            text(field(button, "event")) == "storage_oauth_settings"
        }),
    );
    let settings = event(
        controller,
        Some(state(&opened)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[2],
        text(field(field(field(&settings, "ui"), "modal"), "title"))
            == "Network storage authorization",
    );
    let fields = field(field(&settings, "ui"), "modal")
        .get("fields")
        .unwrap_or_else(|| panic!("missing fields"))
        .arr();
    let owner = fields
        .iter()
        .find(|item| text(field(item, "key")) == "authorization_owner")
        .unwrap_or_else(|| panic!("missing authorization_owner"));
    let destination = fields
        .iter()
        .find(|item| text(field(item, "key")) == "authorization_destination")
        .unwrap_or_else(|| panic!("missing authorization_destination"));
    keep(
        passed,
        &CHECKS[3],
        text(field(owner, "value")).contains("network storage connection only")
            && text(field(destination, "value")).contains("127.0.0.1"),
    );
    state(&settings).clone()
}

fn authorize(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    passed: &mut Vec<String>,
) -> Value {
    let (started, uri) = browser(controller, current, root);
    let query = query(text(field(field(&started, "effect"), "url")));
    keep(
        passed,
        &CHECKS[4],
        query.get("state").is_some_and(|value| value.len() == 43)
            && query.get("code_challenge_method").is_some_and(|value| value == "S256")
            && !query.contains_key("code_verifier"),
    );
    let wrong_uri = uri.replacen("state=", "state=wrong", 1);
    let wrong = event(
        controller,
        Some(state(&started)),
        "oauth_return",
        twin::obj(vec![("uri".into(), s(&wrong_uri))]),
        root,
    );
    keep(
        passed,
        &CHECKS[5],
        state(&wrong) == state(&started)
            && matches!(wrong.get("effect"), Some(Value::Null)),
    );
    let pending = event(
        controller,
        Some(state(&started)),
        "oauth_return",
        twin::obj(vec![("uri".into(), s(uri.as_str()))]),
        root,
    );
    keep(
        passed,
        &CHECKS[6],
        text(field(field(&pending, "effect"), "purpose")).starts_with("oauth_token:")
            && field(state(&pending), "busy").bool_(),
    );
    let (adopted, callback) = complete(controller, &pending, root);
    let drive = field(state(&adopted), "drive");
    let token = text(field(field(drive, "oauth_tokens"), "access_token"));
    let refresh = text(field(field(drive, "oauth_tokens"), "refresh_token"));
    keep(
        passed,
        &CHECKS[7],
        field(drive, "oauth_enabled").bool_()
            && field(state(&adopted), "profiles").arr().is_empty()
            && token.contains("storage-access"),
    );
    keep(
        passed,
        &CHECKS[8],
        text(field(drive, "user")) == "retained-user"
            && text(field(drive, "password")) == "retained-password"
            && text(field(drive, "key")) == "retained-key"
            && text(field(drive, "auth_header")) == "X-Storage-Key"
            && text(field(drive, "auth_prefix")) == "Manual",
    );
    let ui = Value::to_string(field(&adopted, "ui"));
    keep(passed, &CHECKS[9], !ui.contains(&token) && !ui.contains(&refresh));
    let late = event(
        controller,
        Some(state(&adopted)),
        "network_result",
        callback,
        root,
    );
    keep(
        passed,
        &CHECKS[10],
        state(&late) == state(&adopted) && matches!(late.get("effect"), Some(Value::Null)),
    );
    let opened = event(
        controller,
        Some(state(&adopted)),
        "drive_edit",
        twin::obj(vec![]),
        root,
    );
    let status = field(field(&opened, "ui"), "modal")
        .get("fields")
        .unwrap_or_else(|| panic!("missing fields"))
        .arr()
        .iter()
        .find(|item| text(field(item, "key")) == "auth_status")
        .unwrap_or_else(|| panic!("missing auth_status"));
    keep(
        passed,
        &CHECKS[11],
        text(field(status, "value")).contains("Storage authorization active")
            && !text(field(status, "value")).contains(&token),
    );
    state(&adopted).clone()
}

fn check_storage_requests(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) -> Value {
    let opened = event(
        controller,
        Some(current),
        "drive_edit",
        twin::obj(vec![]),
        root,
    );
    let listed_event = event(
        controller,
        Some(state(&opened)),
        "drive_list",
        twin::obj(vec![]),
        root,
    );
    let (listed, _) = complete(controller, &listed_event, root);
    let token = text(field(
        field(field(state(&listed), "drive"), "oauth_tokens"),
        "access_token",
    ));
    let request = peer.last_headers();
    keep(
        passed,
        &CHECKS[12],
        request
            .get("authorization")
            .is_some_and(|value| value == &format!("Bearer {token}"))
            && !request.contains_key("x-storage-key")
            && field(state(&listed), "remote").arr().len() == 1,
    );
    let read_event = event(
        controller,
        Some(state(&listed)),
        "remote:0",
        twin::obj(vec![]),
        root,
    );
    let (read, _) = complete(controller, &read_event, root);
    keep(
        passed,
        &CHECKS[13],
        Value::to_string(field(&read, "ui")).contains("Actual authorized storage text."),
    );
    let editor = event(
        controller,
        Some(state(&read)),
        "drive_editor",
        twin::obj(vec![]),
        root,
    );
    let review = event(
        controller,
        Some(state(&editor)),
        "drive_create_review",
        twin::obj(vec![
            ("path".into(), s("authorized.txt")),
            ("content".into(), s("Actual OAuth-owned write.")),
        ]),
        root,
    );
    keep(
        passed,
        &CHECKS[14],
        matches!(review.get("effect"), Some(Value::Null))
            && text(field(state(&review), "dialog")) == "approval",
    );
    let approve = event(
        controller,
        Some(state(&review)),
        "approve",
        twin::obj(vec![]),
        root,
    );
    let (approved, _) = complete(controller, &approve, root);
    let (written, _) = complete(controller, &approved, root);
    keep(
        passed,
        &CHECKS[15],
        peer.file("/share/authorized.txt").as_deref() == Some(b"Actual OAuth-owned write.".as_slice())
            && Value::to_string(field(&written, "ui")).contains("readback verified"),
    );
    let export = event(
        controller,
        Some(state(&written)),
        "configuration_export",
        twin::obj(vec![]),
        root,
    );
    let content = text(field(field(&export, "effect"), "content"));
    let drive = field(state(&export), "drive");
    let token = text(field(field(drive, "oauth_tokens"), "access_token"));
    let refresh = text(field(field(drive, "oauth_tokens"), "refresh_token"));
    keep(
        passed,
        &CHECKS[16],
        !content.contains(&token)
            && !content.contains(&refresh)
            && !content.contains("retained-key")
            && !content.contains("retained-user")
            && !content.contains("retained-password"),
    );
    let ticket = text(field(field(&export, "effect"), "ticket"));
    let cancelled = event(
        controller,
        Some(state(&export)),
        "export_cancel",
        twin::obj(vec![("ticket".into(), s(ticket))]),
        root,
    );
    let manual = event(
        controller,
        Some(state(&cancelled)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let manual = event(
        controller,
        Some(state(&manual)),
        "oauth_key_mode",
        twin::obj(vec![]),
        root,
    );
    let list = event(
        controller,
        Some(state(&manual)),
        "drive_list",
        twin::obj(vec![]),
        root,
    );
    let (manual, _) = complete(controller, &list, root);
    let request = peer.last_headers();
    keep(
        passed,
        &CHECKS[17],
        request
            .get("x-storage-key")
            .is_some_and(|value| value == "Manual retained-key")
            && !request.contains_key("authorization")
            && text(field(
                field(field(state(&manual), "drive"), "oauth_tokens"),
                "access_token"
            )) == token,
    );
    state(&manual).clone()
}

fn check_manual_and_refresh(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) -> Value {
    let settings = event(
        controller,
        Some(current),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let (adopted, _) = authorize_only(controller, state(&settings), root);
    let token = text(field(
        field(field(state(&adopted), "drive"), "oauth_tokens"),
        "access_token",
    ));
    let mut expired = state(&adopted).clone();
    set_path(
        &mut expired,
        &["drive", "oauth_tokens", "expires_at"],
        twin::n("1"),
    );
    let before = peer.request_count();
    let blocked = event(
        controller,
        Some(&expired),
        "drive_list",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[18],
        matches!(blocked.get("effect"), Some(Value::Null))
            && state(&blocked).get("notice_error").is_some_and(twin::truthy)
            && peer.request_count() == before,
    );
    let settings = event(
        controller,
        Some(state(&blocked)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let renewal = event(
        controller,
        Some(state(&settings)),
        "oauth_refresh",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[19],
        field(
            field(field(state(&renewal), "drive"), "oauth_tokens"),
            "refresh_uncertain"
        )
        .bool_()
            && field(state(&renewal), "profiles").arr().is_empty(),
    );
    let (renewed, _) = complete(controller, &renewal, root);
    let drive = field(state(&renewed), "drive");
    keep(
        passed,
        &CHECKS[20],
        text(field(field(drive, "oauth_tokens"), "access_token")) != token
            && text(field(drive, "key")) == "retained-key"
            && absent_or_false(field(drive, "oauth_tokens"), "refresh_uncertain"),
    );
    state(&renewed).clone()
}

fn check_model_isolation(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) -> Value {
    let mut last = Value::Null;
    for (index, format) in ["chat", "local", "messages", "content"].iter().enumerate() {
        let mut st = current.clone();
        set_path(
            &mut st,
            &["profiles"],
            Value::Arr(vec![twin::obj(vec![
                ("name".into(), s("Primary")),
                ("protocol".into(), s(*format)),
                (
                    "endpoint".into(),
                    s(&format!("{}/model/{format}", peer.origin())),
                ),
                ("model".into(), s("controlled")),
                ("key".into(), s("primary-key")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
                ("auth_header".into(), s("Authorization")),
                ("auth_prefix".into(), s("Bearer")),
            ])]),
        );
        let sent = event(
            controller,
            Some(&st),
            "send",
            twin::obj(vec![("draft".into(), s("Keep text independent of storage."))]),
            root,
        );
        let effect = Value::to_string(field(&sent, "effect"));
        keep(
            passed,
            &CHECKS[21 + index * 3],
            twin::truthy(field(&sent, "effect"))
                && !effect.contains("storage-access-")
                && !effect.contains("retained-key"),
        );
        let (done, _) = complete(controller, &sent, root);
        keep(
            passed,
            &CHECKS[22 + index * 3],
            Value::to_string(field(state(&done), "chats")).contains("Actual primary response.")
                && field(state(&done), "drive") == field(&st, "drive"),
        );
        let busy = event(
            controller,
            Some(&st),
            "send",
            twin::obj(vec![("draft".into(), s("Keep this primary owner."))]),
            root,
        );
        let owner = field(state(&busy), "model_expected").clone();
        let refused = event(
            controller,
            Some(state(&busy)),
            "storage_oauth_settings",
            twin::obj(vec![]),
            root,
        );
        keep(
            passed,
            &CHECKS[23 + index * 3],
            matches!(refused.get("effect"), Some(Value::Null))
                && field(state(&refused), "busy").bool_()
                && field(state(&refused), "model_expected") == &owner,
        );
        event(
            controller,
            Some(state(&refused)),
            "cancel",
            twin::obj(vec![]),
            root,
        );
        last = done;
    }
    let mut st = state(&last).clone();
    set_path(&mut st, &["profiles", "0", "protocol"], s("chat"));
    set_path(
        &mut st,
        &["profiles", "0", "endpoint"],
        s(&format!("{}/model/chat", peer.origin())),
    );
    let settings = event(
        controller,
        Some(&st),
        "oauth_settings",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[33],
        text(field(state(&settings), "oauth_target")) == "model"
            && text(field(field(field(&settings, "ui"), "modal"), "title"))
                == "Connection authorization",
    );
    let settings = event(
        controller,
        Some(state(&settings)),
        "oauth_save",
        twin::obj(vec![
            (
                "authorization_endpoint".into(),
                s(&format!("{}/authorize", peer.origin())),
            ),
            ("token_endpoint".into(), s(&format!("{}/token", peer.origin()))),
            ("client_id".into(), s("public-client")),
            ("scope".into(), s("conversation")),
            ("issuer".into(), s(peer.origin())),
            ("plain".into(), twin::b(true)),
        ]),
        root,
    );
    let (authorized, _) = authorize_only(controller, state(&settings), root);
    let model_token = text(field(
        field(profile_at(state(&authorized), 0), "oauth_tokens"),
        "access_token",
    ));
    let storage_token = text(field(
        field(field(state(&authorized), "drive"), "oauth_tokens"),
        "access_token",
    ));
    keep(
        passed,
        &CHECKS[34],
        model_token != storage_token
            && model_token.contains("conversation")
            && storage_token.contains("storage"),
    );
    let settings = event(
        controller,
        Some(state(&authorized)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let list = event(
        controller,
        Some(state(&settings)),
        "drive_list",
        twin::obj(vec![]),
        root,
    );
    let (listed, _) = complete(controller, &list, root);
    let request = peer.last_headers();
    let request_text = peer.last_text();
    keep(
        passed,
        &CHECKS[35],
        request
            .get("authorization")
            .is_some_and(|value| value == &format!("Bearer {storage_token}"))
            && !request_text.contains(&model_token),
    );
    let sent = event(
        controller,
        Some(state(&listed)),
        "send",
        twin::obj(vec![("draft".into(), s("Use only model authorization."))]),
        root,
    );
    let (done, _) = complete(controller, &sent, root);
    let request = peer.last_headers();
    keep(
        passed,
        &CHECKS[36],
        Value::to_string(field(state(&done), "chats")).contains("Actual primary response.")
            && request
                .get("authorization")
                .is_some_and(|value| value == &format!("Bearer {model_token}")),
    );
    let mut st = state(&done).clone();
    set_path(
        &mut st,
        &["profiles", "0", "oauth_tokens", "expires_at"],
        twin::n(&future().to_string()),
    );
    let pending = event(
        controller,
        Some(&st),
        "send",
        twin::obj(vec![("draft".into(), s("Refresh the model, not storage."))]),
        root,
    );
    keep(
        passed,
        &CHECKS[37],
        text(field(field(&pending, "effect"), "purpose")).starts_with("oauth_token:")
            && field(profile_at(state(&pending), 0), "oauth_tokens")
                .get("refresh_uncertain")
                .unwrap_or_else(|| panic!("missing model refresh_uncertain"))
                .bool_()
            && absent_or_false(
                field(field(state(&pending), "drive"), "oauth_tokens"),
                "refresh_uncertain",
            ),
    );
    let (sent, _) = complete(controller, &pending, root);
    let (done, _) = complete(controller, &sent, root);
    keep(
        passed,
        &CHECKS[38],
        Value::to_string(field(state(&done), "chats")).contains("Actual primary response.")
            && field(state(&done), "drive") == field(&st, "drive"),
    );
    let settings = event(
        controller,
        Some(state(&done)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let (waiting, waiting_uri) = browser(controller, state(&settings), root);
    let primary = event(
        controller,
        Some(state(&waiting)),
        "send",
        twin::obj(vec![(
            "draft".into(),
            s("Continue text while storage sign-in is waiting."),
        )]),
        root,
    );
    keep(
        passed,
        &CHECKS[39],
        twin::truthy(field(&primary, "effect"))
            && text(field(field(&primary, "effect"), "purpose")) == "model"
            && field(state(&primary), "busy").bool_(),
    );
    let (finished, _) = complete(controller, &primary, root);
    keep(
        passed,
        &CHECKS[40],
        Value::to_string(field(state(&finished), "chats")).contains("Actual primary response.")
            && field(state(&finished), "drive") == field(state(&done), "drive"),
    );
    let late = event(
        controller,
        Some(state(&finished)),
        "oauth_return",
        twin::obj(vec![("uri".into(), s(&waiting_uri))]),
        root,
    );
    keep(
        passed,
        &CHECKS[41],
        state(&late) == state(&finished) && matches!(late.get("effect"), Some(Value::Null)),
    );
    state(&finished).clone()
}

fn check_refresh_failures(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) -> Value {
    let settings = event(
        controller,
        Some(current),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let start = event(
        controller,
        Some(state(&settings)),
        "oauth_refresh",
        twin::obj(vec![]),
        root,
    );
    let purpose = text(field(field(&start, "effect"), "purpose"));
    let failed = event(
        controller,
        Some(state(&start)),
        "failure",
        twin::obj(vec![
            ("purpose".into(), s(purpose)),
            ("sent".into(), twin::b(false)),
            ("error".into(), s("Controlled pre-send refusal")),
        ]),
        root,
    );
    keep(
        passed,
        &CHECKS[42],
        !twin::truthy(field(
            field(field(state(&failed), "drive"), "oauth_tokens"),
            "refresh_uncertain",
        )),
    );
    let retry = event(
        controller,
        Some(state(&failed)),
        "oauth_refresh",
        twin::obj(vec![]),
        root,
    );
    let (adopted, _) = complete(controller, &retry, root);
    let start = event(
        controller,
        Some(state(&adopted)),
        "oauth_refresh",
        twin::obj(vec![]),
        root,
    );
    let purpose = text(field(field(&start, "effect"), "purpose"));
    let failed = event(
        controller,
        Some(state(&start)),
        "failure",
        twin::obj(vec![
            ("purpose".into(), s(purpose)),
            ("sent".into(), twin::b(true)),
            ("error".into(), s("Controlled uncertain outcome")),
        ]),
        root,
    );
    let before = peer.request_count();
    let retry = event(
        controller,
        Some(state(&failed)),
        "oauth_refresh",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[43],
        matches!(retry.get("effect"), Some(Value::Null))
            && field(
                field(field(state(&retry), "drive"), "oauth_tokens"),
                "refresh_uncertain"
            )
            .bool_()
            && peer.request_count() == before,
    );
    state(&retry).clone()
}

fn check_browser_recovery(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) -> Value {
    let settings = event(
        controller,
        Some(current),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let (started, uri) = browser(controller, state(&settings), root);
    let cancelled = event(
        controller,
        Some(state(&started)),
        "oauth_cancel",
        twin::obj(vec![]),
        root,
    );
    let late = event(
        controller,
        Some(state(&cancelled)),
        "oauth_return",
        twin::obj(vec![("uri".into(), s(&uri))]),
        root,
    );
    keep(
        passed,
        &CHECKS[44],
        state(&late) == state(&cancelled) && matches!(late.get("effect"), Some(Value::Null)),
    );
    let settings = event(
        controller,
        Some(state(&cancelled)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let (started, uri) = browser(controller, state(&settings), root);
    let restored = event(
        controller,
        Some(state(&started)),
        "resume",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[45],
        text(field(state(&restored), "oauth_target")) == "storage"
            && text(field(state(&restored), "dialog")) == "oauth_wait"
            && text(field(field(state(&restored), "oauth_pending"), "target")) == "storage",
    );
    let returned = event(
        controller,
        Some(state(&restored)),
        "oauth_return",
        twin::obj(vec![("uri".into(), s(&uri))]),
        root,
    );
    let (adopted, _) = complete(controller, &returned, root);
    keep(
        passed,
        &CHECKS[46],
        field(field(state(&adopted), "drive"), "oauth_enabled").bool_()
            && text(field(
                field(field(state(&adopted), "drive"), "oauth_tokens"),
                "access_token"
            ))
            .contains("storage"),
    );
    let (started, uri) = browser(controller, state(&adopted), root);
    let mut changed = state(&started).clone();
    set_path(
        &mut changed,
        &["drive", "url"],
        s(&format!("{}/different/", peer.origin())),
    );
    let late = event(
        controller,
        Some(&changed),
        "oauth_return",
        twin::obj(vec![("uri".into(), s(&uri))]),
        root,
    );
    keep(
        passed,
        &CHECKS[47],
        state(&late) == &changed && matches!(late.get("effect"), Some(Value::Null)),
    );
    let mut changed = state(&adopted).clone();
    set_path(
        &mut changed,
        &["drive", "url"],
        s(&format!("{}/different/", peer.origin())),
    );
    let before = peer.request_count();
    let blocked = event(
        controller,
        Some(&changed),
        "drive_list",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[48],
        matches!(blocked.get("effect"), Some(Value::Null))
            && state(&blocked).get("notice_error").is_some_and(twin::truthy)
            && peer.request_count() == before,
    );
    state(&adopted).clone()
}

fn check_device_authorization(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    passed: &mut Vec<String>,
) -> Value {
    let settings = event(
        controller,
        Some(current),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let started = event(
        controller,
        Some(state(&settings)),
        "oauth_device_start",
        twin::obj(vec![]),
        root,
    );
    let (device, _) = complete(controller, &started, root);
    keep(
        passed,
        &CHECKS[49],
        text(field(state(&device), "dialog")) == "oauth_device"
            && text(field(state(&device), "oauth_target")) == "storage",
    );
    let early = event(
        controller,
        Some(state(&device)),
        "oauth_device_poll",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[50],
        matches!(early.get("effect"), Some(Value::Null))
            && text(field(state(&early), "notice")).contains("Wait"),
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let polled = event(
        controller,
        Some(state(&early)),
        "oauth_device_poll",
        twin::obj(vec![]),
        root,
    );
    let (adopted, _) = complete(controller, &polled, root);
    let storage_token = text(field(
        field(field(state(&adopted), "drive"), "oauth_tokens"),
        "access_token",
    ));
    let model_token = text(field(
        field(profile_at(state(&adopted), 0), "oauth_tokens"),
        "access_token",
    ));
    keep(
        passed,
        &CHECKS[51],
        field(field(state(&adopted), "drive"), "oauth_enabled").bool_()
            && storage_token.contains("storage")
            && model_token != storage_token,
    );
    state(&adopted).clone()
}

fn check_voice_lifecycle(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) {
    let settings = event(
        controller,
        Some(current),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    let voice = event(
        controller,
        Some(state(&settings)),
        "service_connections",
        twin::obj(vec![]),
        root,
    );
    let voice = event(
        controller,
        Some(state(&voice)),
        "service_save",
        twin::obj(vec![("service_config".into(), s(&service_config(peer.origin())))]),
        root,
    );
    let voice = event(
        controller,
        Some(state(&voice)),
        "voice_open_review",
        twin::obj(vec![]),
        root,
    );
    let voice = event(
        controller,
        Some(state(&voice)),
        "voice_consent",
        twin::obj(vec![]),
        root,
    );
    let owner = twin::obj(vec![
        (
            "voice_session".into(),
            field(field(&voice, "effect"), "voice_session").clone(),
        ),
        (
            "voice_audio_ticket".into(),
            field(field(&voice, "effect"), "voice_audio_ticket").clone(),
        ),
    ]);
    let voice = event(
        controller,
        Some(state(&voice)),
        "voice_audio_start",
        owner.clone(),
        root,
    );
    let frame = twin::obj(vec![
        ("op".into(), s("voice_audio_feed")),
        ("data".into(), s(&b64_encode(&[0; 640]))),
        (
            "voice_session".into(),
            field(&owner, "voice_session").clone(),
        ),
        (
            "voice_audio_ticket".into(),
            field(&owner, "voice_audio_ticket").clone(),
        ),
    ]);
    let ready = controller.op(&frame);
    keep(passed, &CHECKS[52], !field(&ready, "ready").bool_());
    let before = peer.request_count();
    let stopped = event(
        controller,
        Some(state(&voice)),
        "storage_oauth_settings",
        twin::obj(vec![]),
        root,
    );
    keep(
        passed,
        &CHECKS[53],
        !twin::truthy(field(state(&stopped), "voice_open"))
            && absent_or_false(state(&stopped), "voice_consent")
            && peer.request_count() == before
            && text(field(state(&stopped), "dialog")) == "oauth_settings",
    );
    let error = controller.send(&frame);
    keep(passed, &CHECKS[54], error.get("error").is_some());
    let late = event(
        controller,
        Some(state(&stopped)),
        "voice_audio_start",
        owner,
        root,
    );
    keep(
        passed,
        &CHECKS[55],
        state(&late) == state(&stopped) && matches!(late.get("effect"), Some(Value::Null)),
    );
}

fn check_invalid_modes(
    controller: &mut Controller,
    root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) {
    for (index, value) in [
        Value::Str("true".into()),
        twin::n("1"),
        Value::Obj(vec![]),
        Value::Arr(vec![]),
    ]
    .iter()
    .enumerate()
    {
        let mut st = current.clone();
        set_path(&mut st, &["drive", "oauth_enabled"], value.clone());
        let before = peer.request_count();
        let refused = event(
            controller,
            Some(&st),
            "drive_list",
            twin::obj(vec![]),
            root,
        );
        keep(
            passed,
            &CHECKS[56 + index],
            matches!(refused.get("effect"), Some(Value::Null))
                && state(&refused).get("notice_error").is_some_and(twin::truthy)
                && peer.request_count() == before,
        );
    }
}

fn check_project_transfers(
    controller: &mut Controller,
    _root: &str,
    current: &Value,
    peer: &Peer,
    passed: &mut Vec<String>,
) {
    let payload = b"\x00\xffopaque authorized payload\x80";
    let storage_token = text(field(
        field(field(current, "drive"), "oauth_tokens"),
        "access_token",
    ));
    for streamed in [false, true] {
        let label = if streamed { "streamed" } else { "legacy" };
        for phase in ["write", "probe", "verify"] {
            let target = format!("/share/{label}-oauth.bin");
            let mut record = twin::obj(vec![
                (
                    "stage_ticket".into(),
                    s(&format!("oauth-{label}")),
                ),
                (
                    "purpose".into(),
                    s(&format!("project_transfer:oauth-{phase}")),
                ),
                ("bytes".into(), twin::n(&payload.len().to_string())),
                ("phone".into(), twin::b(false)),
                ("tree".into(), Value::Null),
                (
                    "connection".into(),
                    field(current, "drive").clone(),
                ),
                ("target".into(), s(&format!("{}{target}", peer.origin()))),
                ("mime".into(), s("application/octet-stream")),
                ("source".into(), Value::Null),
                ("phase".into(), s(phase)),
            ]);
            if streamed {
                set_path(&mut record, &["format"], twin::n("2"));
            }
            let values = twin::obj(vec![
                (
                    "op".into(),
                    s(if streamed {
                        "project_chunks"
                    } else {
                        "project_payload"
                    }),
                ),
                ("action".into(), s("open")),
                ("record".into(), record.clone()),
                ("stored".into(), record),
                ("archive".into(), s(&b64_encode(payload))),
            ]);
            let effect = controller.op(&values);
            let wire = wire_bytes(&effect);
            let phase_index = match phase {
                "write" => 0usize,
                "probe" => 1,
                "verify" => 2,
                other => panic!("unknown transfer phase {other}"),
            };
            let base = 60 + (usize::from(streamed) * 3 + phase_index) * 3;
            let authorization = format!("Authorization: Bearer {storage_token}\r\n");
            keep(
                passed,
                &CHECKS[base],
                wire.windows(authorization.len())
                    .any(|window| window == authorization.as_bytes())
                    && !wire.windows(b"X-Storage-Key".len()).any(|window| window == b"X-Storage-Key"),
            );
            let response = exchange_with(
                &effect,
                if streamed && phase == "write" { payload } else { &[] },
            );
            let status = format!(
                "HTTP/1.0 {}",
                if phase == "write" { "201" } else { "200" }
            );
            keep(
                passed,
                &CHECKS[base + 1],
                response.starts_with(status.as_bytes())
                    && peer.file(&target).as_deref() == Some(payload.as_slice()),
            );
            controller.op(&twin::obj(vec![
                (
                    "op".into(),
                    s(if streamed {
                        "project_chunks"
                    } else {
                        "project_close"
                    }),
                ),
                ("action".into(), s("close")),
                (
                    "handle".into(),
                    field(&effect, "snapshot_handle").clone(),
                ),
            ]));
            let mut mismatch = values.clone();
            set_path(
                &mut mismatch,
                &["stored", "connection", "oauth_tokens", "access_token"],
                s("changed-storage-credential"),
            );
            let error = controller.send(&mismatch);
            keep(
                passed,
                &CHECKS[base + 2],
                error.get("value").is_none(),
            );
        }
    }
}

fn browser(controller: &mut Controller, st: &Value, root: &str) -> (Value, String) {
    let started = event(controller, Some(st), "oauth_sign_in", twin::obj(vec![]), root);
    let effect = field(&started, "effect");
    if text(field(effect, "kind")) != "oauth_browser" {
        panic!("browser did not start: {}", started.to_string());
    }
    let uri = redirect_from(text(field(effect, "url")));
    (started, uri)
}

fn redirect_from(url: &str) -> String {
    let rest = url
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("authorization URL is not http: {url}"));
    let (authority, path) = rest
        .split_once('/')
        .unwrap_or_else(|| panic!("authorization URL has no path: {url}"));
    // Not an invented value: the leg hands the parsed port to http.client.HTTPConnection, and that
    // class resolves a None port to its default_port, which is 80. This URL has already been
    // restricted to `http://` above, so 80 is the endpoint the leg would have connected to.
    let (host, port_text) = authority.split_once(':').unwrap_or((authority, "80"));
    let port = port_text
        .parse::<u16>()
        .unwrap_or_else(|error| panic!("authorization port: {error}"));
    let mut stream = TcpStream::connect((host, port))
        .unwrap_or_else(|error| panic!("authorize connect {authority}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("authorize timeout: {error}"));
    let request = format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .unwrap_or_else(|error| panic!("authorize write: {error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("authorize read: {error}"));
    let text = String::from_utf8_lossy(&response);
    let status = text.lines().next().unwrap_or("");
    if !status.starts_with("HTTP/1.0 302 ") && !status.starts_with("HTTP/1.1 302 ") {
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

fn authorize_only(controller: &mut Controller, st: &Value, root: &str) -> (Value, Value) {
    let (started, uri) = browser(controller, st, root);
    let returned = event(
        controller,
        Some(state(&started)),
        "oauth_return",
        twin::obj(vec![("uri".into(), s(&uri))]),
        root,
    );
    complete(controller, &returned, root)
}

fn complete(controller: &mut Controller, request: &Value, root: &str) -> (Value, Value) {
    let effect = field(request, "effect");
    let mut inputs = twin::obj(vec![]);
    for key in [
        "purpose",
        "drive_ticket",
        "model_ticket",
        "child_ticket",
        "child_owner",
    ] {
        if let Some(value) = effect.get(key) {
            set_path(&mut inputs, &[key], value.clone());
        }
    }
    let response = exchange_with(effect, &[]);
    set_path(&mut inputs, &["data"], s(&b64_encode(&response)));
    let result = event(
        controller,
        Some(state(request)),
        "network_result",
        inputs.clone(),
        root,
    );
    (result, inputs)
}

fn exchange_with(effect: &Value, tail: &[u8]) -> Vec<u8> {
    let wire = wire_bytes(effect);
    let host = text(field(effect, "host")).to_string();
    let port = match field(effect, "port") {
        Value::Num(text) | Value::Str(text) => text
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("bad port {text}: {error}")),
        other => panic!("bad port {}", other.to_string()),
    };
    let mut stream = std::net::TcpStream::connect((host.as_str(), port))
        .unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("read timeout: {error}"));
    let mut request = wire;
    request.extend_from_slice(tail);
    stream
        .write_all(&request)
        .unwrap_or_else(|error| panic!("write: {error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("read: {error}"));
    response
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    effect
        .get("wire_base64")
        .map(|value| b64_decode(value.str_()))
        .or_else(|| {
            effect
                .get("wire")
                .map(|value| value.str_().as_bytes().to_vec())
        })
        .unwrap_or_else(|| panic!("network effect has no wire data"))
}

fn event(
    controller: &mut Controller,
    st: Option<&Value>,
    action: &str,
    inputs: Value,
    root: &str,
) -> Value {
    controller.event(st, action, inputs, root)
}

fn absent_or_false(value: &Value, key: &str) -> bool {
    match value.get(key) {
        None | Some(Value::Null) => true,
        Some(item) => !twin::truthy(item),
    }
}

fn profile_at<'a>(current: &'a Value, index: usize) -> &'a Value {
    field(current, "profiles")
        .arr()
        .get(index)
        .unwrap_or_else(|| panic!("missing profile {index}"))
}

fn state(value: &Value) -> &Value {
    field(value, "state")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value
        .get(key)
        .unwrap_or_else(|| panic!("missing {key} in {}", value.to_string()))
}

fn text(value: &Value) -> &str {
    value.str_()
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn set_path(value: &mut Value, path: &[&str], next: Value) {
    let mut current = value;
    for key in &path[..path.len() - 1] {
        current = descend(current, key);
    }
    assign(current, path[path.len() - 1], next);
}

fn descend<'a>(current: &'a mut Value, key: &str) -> &'a mut Value {
    match current {
        Value::Arr(items) => {
            let index = key
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("expected index, got {key}"));
            items
                .get_mut(index)
                .unwrap_or_else(|| panic!("missing index {index}"))
        }
        Value::Obj(fields) => {
            let index = fields
                .iter()
                .position(|(name, _)| name == key)
                .unwrap_or_else(|| panic!("missing {key}"));
            &mut fields[index].1
        }
        other => panic!("expected object or array, got {}", other.to_string()),
    }
}

fn assign(current: &mut Value, key: &str, next: Value) {
    match current {
        Value::Arr(items) => {
            let index = key
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("expected index, got {key}"));
            let slot = items
                .get_mut(index)
                .unwrap_or_else(|| panic!("missing index {index}"));
            *slot = next;
        }
        Value::Obj(fields) => match fields.iter().position(|(name, _)| name == key) {
            Some(index) => fields[index].1 = next,
            None => fields.push((key.to_string(), next)),
        },
        other => panic!("expected object or array, got {}", other.to_string()),
    }
}

fn query(uri: &str) -> std::collections::HashMap<String, String> {
    let query = uri.split_once('?').map(|(_, value)| value).unwrap_or("");
    query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.to_string(), percent_decode(value)))
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                out.push(
                    u8::from_str_radix(&text[index + 1..index + 3], 16)
                        .unwrap_or_else(|error| panic!("bad percent escape: {error}")),
                );
                index += 3;
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
    String::from_utf8(out).unwrap_or_else(|error| panic!("bad utf-8: {error}"))
}

fn service_config(origin: &str) -> String {
    let config = twin::obj(vec![
        ("version".into(), twin::n("1")),
        (
            "hosts".into(),
            Value::Arr(vec![twin::obj(vec![
                ("id".into(), s("speech")),
                ("url".into(), s(origin)),
                ("plain".into(), twin::b(true)),
                ("enabled".into(), twin::b(true)),
            ])]),
        ),
        (
            "routes".into(),
            Value::Arr(vec![
                twin::obj(vec![
                    ("id".into(), s("recognition")),
                    ("host".into(), s("speech")),
                    ("capability".into(), s("speech_recognition")),
                    ("path".into(), s("/recognize")),
                    ("method".into(), s("POST")),
                    ("request_format".into(), s("multipart")),
                    ("file_field".into(), s("audio")),
                    ("request".into(), Value::Obj(vec![])),
                    ("response".into(), s("json")),
                    ("response_path".into(), s("/text")),
                    ("enabled".into(), twin::b(true)),
                ]),
                twin::obj(vec![
                    ("id".into(), s("synthesis")),
                    ("host".into(), s("speech")),
                    ("capability".into(), s("speech_synthesis")),
                    ("path".into(), s("/speak")),
                    ("method".into(), s("POST")),
                    (
                        "request".into(),
                        twin::obj(vec![("text".into(), twin::obj(vec![("$input".into(), s("/text"))]))]),
                    ),
                    ("response".into(), s("binary")),
                    ("artifact_kind".into(), s("audio")),
                    ("enabled".into(), twin::b(true)),
                ]),
            ]),
        ),
        (
            "defaults".into(),
            twin::obj(vec![
                (
                    "speech_recognition".into(),
                    twin::obj(vec![("routes".into(), Value::Arr(vec![s("recognition")]))]),
                ),
                (
                    "speech_synthesis".into(),
                    twin::obj(vec![("routes".into(), Value::Arr(vec![s("synthesis")]))]),
                ),
            ]),
        ),
    ]);
    Value::to_string(&config)
}

fn future() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("system clock: {error}"))
        .as_secs()
        + 20
}

struct Call {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct PeerState {
    origin: String,
    calls: Vec<Call>,
    codes: HashMap<String, HashMap<String, String>>,
    refreshes: HashMap<String, String>,
    accesses: HashMap<String, String>,
    devices: HashMap<String, String>,
    files: HashMap<String, Vec<u8>>,
}

struct Peer {
    origin: String,
    state: Arc<Mutex<PeerState>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("peer address: {error}"))
            .port();
        let origin = format!("http://127.0.0.1:{port}");
        let state = Arc::new(Mutex::new(PeerState {
            origin: origin.clone(),
            calls: Vec::new(),
            codes: HashMap::new(),
            refreshes: HashMap::new(),
            accesses: HashMap::new(),
            devices: HashMap::new(),
            files: HashMap::from([(
                "/share/alpha.txt".to_string(),
                b"Actual authorized storage text.".to_vec(),
            )]),
        }));
        let accept = listener
            .try_clone()
            .unwrap_or_else(|error| panic!("peer listener: {error}"));
        let thread_state = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in accept.incoming() {
                let Ok(stream) = stream else { break };
                let state = Arc::clone(&thread_state);
                std::thread::spawn(move || handle_client(stream, state));
            }
        });
        // Dropping this handle leaves the accept thread's clone bound to 127.0.0.1.
        drop(listener);
        Self { origin, state }
    }

    fn origin(&self) -> &str {
        &self.origin
    }

    fn request_count(&self) -> usize {
        self.lock().calls.len()
    }

    fn last_headers(&self) -> HashMap<String, String> {
        self.lock()
            .calls
            .last()
            .map(|call| call.headers.clone())
            .unwrap_or_else(|| panic!("peer recorded no request"))
    }

    fn last_text(&self) -> String {
        let state = self.lock();
        let call = state.calls.last().unwrap_or_else(|| panic!("peer recorded no request"));
        let mut headers: Vec<_> = call
            .headers
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect();
        headers.sort();
        format!(
            "{} {} {} {}",
            call.method,
            call.path,
            headers.join(" "),
            String::from_utf8_lossy(&call.body)
        )
    }

    fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.lock().files.get(path).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PeerState> {
        self.state.lock().unwrap_or_else(|error| panic!("peer lock: {error}"))
    }
}

fn handle_client(mut stream: TcpStream, state: Arc<Mutex<PeerState>>) {
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("peer timeout: {error}"));
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("peer timeout: {error}"));
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .unwrap_or_else(|error| panic!("peer clone: {error}")),
    );
    let mut request_line = String::new();
    if reader
        .read_line(&mut request_line)
        .unwrap_or_else(|error| panic!("peer request: {error}"))
        == 0
    {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .unwrap_or_else(|error| panic!("peer header: {error}"))
            == 0
        {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if length != 0 {
        reader
            .read_exact(&mut body)
            .unwrap_or_else(|error| panic!("peer body: {error}"));
    }
    let (path, query) = split_target(&target);
    let reply = {
        let mut peer = state.lock().unwrap_or_else(|error| panic!("peer lock: {error}"));
        route(&mut peer, &method, &path, &query, &headers, body)
    };
    stream
        .write_all(&reply)
        .unwrap_or_else(|error| panic!("peer write: {error}"));
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn split_target(target: &str) -> (String, HashMap<String, String>) {
    let (path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    (path.to_string(), form_values(raw_query.as_bytes()))
}

fn route(
    peer: &mut PeerState,
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: Vec<u8>,
) -> Vec<u8> {
    if method == "GET" && path == "/authorize" {
        return authorize_redirect(peer, query);
    }
    if (method == "GET" || method == "POST") && path.starts_with("/model/") {
        return model_reply(peer, path, headers, body);
    }
    if method == "POST" {
        record(peer, "POST", path, headers, &body);
        if path == "/device" {
            return device_reply(peer, &body);
        }
        if path == "/token" {
            return token_reply(peer, &body);
        }
        return http_reply(404, &[]);
    }
    storage_reply(peer, method, path, headers, body)
}

fn record(peer: &mut PeerState, method: &str, path: &str, headers: &HashMap<String, String>, body: &[u8]) {
    peer.calls.push(Call {
        method: method.to_string(),
        path: path.to_string(),
        headers: headers.clone(),
        body: body.to_vec(),
    });
}

fn authorize_redirect(peer: &mut PeerState, query: &HashMap<String, String>) -> Vec<u8> {
    let redirect = required(query, "redirect_uri");
    let state = required(query, "state");
    let code = random_hex(12);
    peer.codes.insert(code.clone(), query.clone());
    let location = format!(
        "{redirect}?code={}&state={}&iss={}",
        percent_encode(&code),
        percent_encode(&state),
        percent_encode(&peer.origin)
    );
    http_reply(302, &[("Location", location.as_str())])
}

fn device_reply(peer: &mut PeerState, body: &[u8]) -> Vec<u8> {
    let values = form_values(body);
    let code = random_hex(12);
    let scope = values.get("scope").cloned().unwrap_or_else(|| "storage".to_string());
    peer.devices.insert(code.clone(), scope);
    json_reply(
        200,
        &twin::obj(vec![
            ("device_code".into(), s(&code)),
            ("user_code".into(), s("OWNED-CODE")),
            ("verification_uri".into(), s(&format!("{}/verify", peer.origin))),
            ("expires_in".into(), twin::n("120")),
            ("interval".into(), twin::n("1")),
        ]),
    )
}

fn token_reply(peer: &mut PeerState, body: &[u8]) -> Vec<u8> {
    let values = form_values(body);
    let grant = values.get("grant_type").map(String::as_str).unwrap_or("");
    match grant {
        "authorization_code" => {
            let code = values.get("code").map(String::as_str).unwrap_or("");
            let Some(config) = peer.codes.remove(code) else {
                return json_reply(400, &twin::obj(vec![("error".into(), s("invalid_grant"))]));
            };
            let verifier = values.get("code_verifier").cloned().unwrap_or_default();
            let proof = s256(&verifier);
            let challenge = required(&config, "code_challenge");
            let redirect = required(&config, "redirect_uri");
            let client = required(&config, "client_id");
            if proof != challenge
                || values.get("redirect_uri").map(String::as_str) != Some(redirect.as_str())
                || values.get("client_id").map(String::as_str) != Some(client.as_str())
            {
                return json_reply(400, &twin::obj(vec![("error".into(), s("invalid_grant"))]));
            }
            let scope = config.get("scope").cloned().unwrap_or_else(|| "storage".to_string());
            issue(peer, &scope)
        }
        "refresh_token" => {
            let token = values.get("refresh_token").map(String::as_str).unwrap_or("");
            let Some(scope) = peer.refreshes.remove(token) else {
                return json_reply(400, &twin::obj(vec![("error".into(), s("invalid_grant"))]));
            };
            issue(peer, &scope)
        }
        "urn:ietf:params:oauth:grant-type:device_code" => {
            let token = values.get("device_code").map(String::as_str).unwrap_or("");
            let Some(scope) = peer.devices.remove(token) else {
                return json_reply(400, &twin::obj(vec![("error".into(), s("invalid_grant"))]));
            };
            issue(peer, &scope)
        }
        _ => json_reply(400, &twin::obj(vec![("error".into(), s("unsupported_grant_type"))])),
    }
}

fn issue(peer: &mut PeerState, scope: &str) -> Vec<u8> {
    let access = format!("{scope}-access-{}", random_hex(8));
    let refresh = format!("{scope}-refresh-{}", random_hex(8));
    peer.accesses.insert(access.clone(), scope.to_string());
    peer.refreshes.insert(refresh.clone(), scope.to_string());
    json_reply(
        200,
        &twin::obj(vec![
            ("token_type".into(), s("Bearer")),
            ("access_token".into(), s(&access)),
            ("refresh_token".into(), s(&refresh)),
            ("expires_in".into(), twin::n("3600")),
        ]),
    )
}

fn model_reply(peer: &mut PeerState, path: &str, headers: &HashMap<String, String>, body: Vec<u8>) -> Vec<u8> {
    record(peer, "MODEL", path, headers, &body);
    let authorization = headers.get("authorization").map(String::as_str).unwrap_or("");
    let scope = peer
        .accesses
        .get(authorization.strip_prefix("Bearer ").unwrap_or(authorization))
        .map(String::as_str);
    if !matches!(authorization, "Bearer primary-key" | "Bearer child-key") && scope != Some("conversation") {
        return http_reply(401, &[]);
    }
    let format = path.rsplit('/').next().unwrap_or("");
    json_reply(200, &model_body(format))
}

fn model_body(format: &str) -> Value {
    let text = "Actual primary response.";
    match format {
        "chat" => twin::obj(vec![(
            "choices".into(),
            Value::Arr(vec![twin::obj(vec![
                (
                    "message".into(),
                    twin::obj(vec![
                        ("role".into(), s("assistant")),
                        ("content".into(), s(text)),
                    ]),
                ),
                ("finish_reason".into(), s("stop")),
            ])]),
        )]),
        "local" => twin::obj(vec![
            (
                "message".into(),
                twin::obj(vec![("content".into(), s(text))]),
            ),
            ("done".into(), twin::b(true)),
        ]),
        "messages" => twin::obj(vec![
            ("role".into(), s("assistant")),
            (
                "content".into(),
                Value::Arr(vec![twin::obj(vec![
                    ("type".into(), s("text")),
                    ("text".into(), s(text)),
                ])]),
            ),
            ("stop_reason".into(), s("end_turn")),
        ]),
        "content" => twin::obj(vec![(
            "candidates".into(),
            Value::Arr(vec![twin::obj(vec![
                (
                    "content".into(),
                    twin::obj(vec![(
                        "parts".into(),
                        Value::Arr(vec![twin::obj(vec![("text".into(), s(text))])]),
                    )]),
                ),
                ("finishReason".into(), s("STOP")),
            ])]),
        )]),
        other => panic!("model request has no reply for {other}"),
    }
}

fn storage_reply(
    peer: &mut PeerState,
    method: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: Vec<u8>,
) -> Vec<u8> {
    record(peer, method, path, headers, &body);
    let authorization = headers.get("authorization").map(String::as_str).unwrap_or("");
    let scope = peer
        .accesses
        .get(authorization.strip_prefix("Bearer ").unwrap_or(authorization))
        .map(String::as_str);
    if scope != Some("storage") && headers.get("x-storage-key").map(String::as_str) != Some("Manual retained-key") {
        return http_reply(401, &[]);
    }
    if method == "PROPFIND" {
        return http_bytes(
            207,
            b"<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/share/alpha.txt</d:href><d:propstat><d:prop><d:displayname>alpha.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>",
            &[],
        );
    }
    if method == "PUT" {
        if headers.get("if-none-match").map(String::as_str) == Some("*") && peer.files.contains_key(path) {
            return http_reply(412, &[]);
        }
        peer.files.insert(path.to_string(), body);
        return http_reply(201, &[]);
    }
    if method == "MKCOL" {
        return http_reply(201, &[]);
    }
    if let Some(content) = peer.files.get(path) {
        return http_bytes(200, content, &[]);
    }
    http_reply(404, &[])
}

fn required(values: &HashMap<String, String>, name: &str) -> String {
    values
        .get(name)
        .cloned()
        .unwrap_or_else(|| panic!("authorization request omits {name}"))
}

fn form_values(body: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(body);
    let mut values = HashMap::new();
    for pair in text.split('&').filter(|part| !part.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key), percent_decode(value));
    }
    values
}

fn percent_encode(text: &str) -> String {
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
    twin::b64_encode(&twin::sha256_digest(verifier.as_bytes()))
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

fn random_hex(bytes: usize) -> String {
    let mut data = vec![0u8; bytes];
    let mut file = std::fs::File::open("/dev/urandom").unwrap_or_else(|error| panic!("urandom: {error}"));
    file.read_exact(&mut data)
        .unwrap_or_else(|error| panic!("urandom: {error}"));
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn json_reply(status: u16, value: &Value) -> Vec<u8> {
    http_bytes(status, value.to_string().as_bytes(), &[])
}

fn http_reply(status: u16, headers: &[(&str, &str)]) -> Vec<u8> {
    http_bytes(status, &[], headers)
}

fn http_bytes(status: u16, body: &[u8], headers: &[(&str, &str)]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        207 => "Multi-Status",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        412 => "Precondition Failed",
        _ => "Response",
    };
    let mut response = format!("HTTP/1.0 {status} {reason}\r\n").into_bytes();
    for (key, value) in headers {
        response.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    response.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    response.extend_from_slice(b"Connection: close\r\n\r\n");
    response.extend_from_slice(body);
    response
}
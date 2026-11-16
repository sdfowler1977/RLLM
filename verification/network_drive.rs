mod twin;

use twin::{b64_encode, n, s, Controller, Value};

const SCOPE: &str =
    "Controlled HTTP replies through real Rust event API; no live NAS or Android TLS handshake";
const REPORT: &str = "output/checks/network-results.json";

const CHECKS: [&str; 48] = [
    "manual PROPFIND route and certificate identity",
    "explicit certificate alias accepted and rewritten",
    "configured route and relative href accepted",
    "unrelated hostname refused",
    "different port refused",
    "HTTP downgrade refused",
    "sibling and outside roots refused",
    "decoded traversal refused",
    "double escapes refused",
    "backslash and control bytes refused",
    "aliased collection self entry omitted",
    "manual file open retains VPN route and displays returned bytes",
    "unconfigured alias refused",
    "WebDAV listing accepts comments without interpreting their markup or entities",
    "CDATA preserves literal property text and usable contained hrefs",
    "malformed or external XML is refused locally: DOCTYPE",
    "malformed or external XML is refused locally: entity declaration",
    "malformed or external XML is refused locally: unfinished comment",
    "malformed or external XML is refused locally: illegal comment",
    "malformed or external XML is refused locally: trailing hyphen comment",
    "malformed or external XML is refused locally: unfinished CDATA",
    "malformed or external XML is refused locally: invalid character",
    "malformed or external XML is refused locally: invalid character reference",
    "delegated listing normalizes URLs and retains actual returned evidence",
    "network evidence child returns to captured parent connection",
    "parent completes after actual child storage result",
    "manual create and actual readback comparison: text",
    "completed create rejects duplicate verification: 26",
    "manual create and actual readback comparison: empty",
    "completed create rejects duplicate verification: 0",
    "manual create and actual readback comparison: Unicode",
    "completed create rejects duplicate verification: 26",
    "manual create and actual readback comparison: maximum text size",
    "completed create rejects duplicate verification: 1048576",
    "mismatched readback never reports save success or replays PUT",
    "missing readback remains unconfirmed",
    "verification transport failure leaves no false success or write replay",
    "unconfirmed prior creation does not block another reviewed create",
    "cancelled write ignores late acknowledgement and reports unknown outcome",
    "cancelled readback cannot certify a saved file",
    "changed drive identity cannot certify prior destination",
    "child waits for readback rather than returning on write acknowledgement",
    "stale child write cannot replace current readback",
    "child receives and retains only verified create result",
    "verified child create returns through captured parent",
    "child readback failure returns once to parent without a false result",
    "page redirect chain is followed for five hops and the sixth redirect is refused without a further request",
    "page redirect back to a visited address is refused as a redirect loop without a further request",
];

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = twin::create_scratch(std::path::Path::new(&root_text), "checks-");
    let mut controller = Controller::spawn(&root_text);
    let scratch_text = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch root is not UTF-8"))
        .to_string();
    let mut passed: Vec<String> = Vec::new();

    let resume = event(&mut controller, None, "resume", twin::obj(vec![]), &scratch_text);
    let mut st = state(&resume).clone();
    set_path(&mut st, &["dialog"], Value::Null);
    set_path(
        &mut st,
        &["drive"],
        twin::obj(vec![
            ("url".into(), s("https://100.64.0.2:5006/share/")),
            ("tls_name".into(), s("nas.example.test")),
            ("user".into(), s("fixture")),
            ("password".into(), s("fixture")),
            ("plain".into(), twin::b(false)),
        ]),
    );

    let d = event(
        &mut controller,
        Some(&st),
        "drive_list",
        twin::obj(vec![]),
        &scratch_text,
    );
    route(field(&d, "effect"), "PROPFIND", "/share/");
    keep(&mut passed, &CHECKS[0]);

    let allowed = [
        "https://nas.example.test:5006/share/a%20b.txt",
        "https://100.64.0.2:5006/share/ip.txt",
        "/share/relative.txt",
    ];
    let forbidden = [
        "https://evil.test:5006/share/x",
        "https://nas.example.test:5007/share/x",
        "http://nas.example.test:5006/share/x",
        "https://nas.example.test:5006/share2/x",
        "https://nas.example.test:5006/secret/x",
        "/share/%2e%2e/x",
        "/share/%252e%252e/x",
        "/share/%5cx",
        "/share/%00x",
        "https://nas.example.test:5006/share/",
    ];
    let mut hrefs = allowed.to_vec();
    hrefs.extend_from_slice(&forbidden);
    let d = reply(
        &mut controller,
        &d,
        &xml(&hrefs),
        207,
        &scratch_text,
    );
    let rows = field(state(&d), "remote").arr();
    keep_if(
        &mut passed,
        &CHECKS[1],
        rows.len() == 3
            && text(field(&rows[0], "url")) == "https://100.64.0.2:5006/share/a%20b.txt"
            && text(field(&rows[1], "url")) == "https://100.64.0.2:5006/share/ip.txt"
            && text(field(&rows[2], "url")) == "https://100.64.0.2:5006/share/relative.txt",
    );
    for index in 2..11 {
        keep(&mut passed, &CHECKS[index]);
    }

    let opened = event(
        &mut controller,
        Some(state(&d)),
        "remote:0",
        twin::obj(vec![]),
        &scratch_text,
    );
    route(field(&opened, "effect"), "GET", "/share/a%20b.txt");
    let read = reply(
        &mut controller,
        &opened,
        "Actual controlled WebDAV file text.",
        200,
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[11],
        Value::to_string(field(&read, "ui")).contains("Actual controlled WebDAV file text."),
    );

    let mut no_alias = st.clone();
    set_path(&mut no_alias, &["drive", "tls_name"], s(""));
    let listed = event(
        &mut controller,
        Some(&no_alias),
        "drive_list",
        twin::obj(vec![]),
        &scratch_text,
    );
    let refused = reply(
        &mut controller,
        &listed,
        &xml(&[allowed[0]]),
        207,
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[12],
        field(state(&refused), "remote").arr().is_empty(),
    );

    let commented = format!(
        "<?xml version=\"1.0\"?><!-- <tags> & ignored -->{}",
        xml(&[allowed[0]]).replacen("<d:response>", "<d:response><!--property comment-->", 1)
    );
    let listed = event(
        &mut controller,
        Some(&st),
        "drive_list",
        twin::obj(vec![]),
        &scratch_text,
    );
    let listed = reply(&mut controller, &listed, &commented, 207, &scratch_text);
    keep_if(
        &mut passed,
        &CHECKS[13],
        field(state(&listed), "remote").arr().len() == 1,
    );

    let character_data = "<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href><![CDATA[/share/file.txt]]></d:href><d:propstat><d:prop><d:displayname><![CDATA[File & <notes> &amp;]]></d:displayname></d:prop></d:propstat></d:response></d:multistatus>";
    let listed = event(
        &mut controller,
        Some(&st),
        "drive_list",
        twin::obj(vec![]),
        &scratch_text,
    );
    let listed = reply(
        &mut controller,
        &listed,
        character_data,
        207,
        &scratch_text,
    );
    let row = &field(state(&listed), "remote").arr()[0];
    keep_if(
        &mut passed,
        &CHECKS[14],
        text(field(row, "name")) == "File & <notes> &amp;"
            && text(field(row, "url")).ends_with("/share/file.txt"),
    );

    let malformed = [
        (
            "DOCTYPE",
            format!("<!DOCTYPE x SYSTEM \"https://evil.test/dtd\">{}", xml(&[allowed[0]])),
        ),
        (
            "entity declaration",
            format!("<!ENTITY x \"bad\">{}", xml(&[allowed[0]])),
        ),
        (
            "unfinished comment",
            "<multistatus><!--unfinished".to_string(),
        ),
        (
            "illegal comment",
            "<multistatus><!--illegal--text--></multistatus>".to_string(),
        ),
        (
            "trailing hyphen comment",
            "<multistatus><!--illegal---></multistatus>".to_string(),
        ),
        (
            "unfinished CDATA",
            "<multistatus><![CDATA[unfinished".to_string(),
        ),
        (
            "invalid character",
            format!("{}\u{0}", xml(&[allowed[0]])),
        ),
        (
            "invalid character reference",
            xml(&[allowed[0]]).replacen("Observed file", "&#0;", 1),
        ),
    ];
    for (offset, (_label, body)) in malformed.iter().enumerate() {
        let listed = event(
            &mut controller,
            Some(&st),
            "drive_list",
            twin::obj(vec![]),
            &scratch_text,
        );
        let refused = reply(&mut controller, &listed, body, 207, &scratch_text);
        keep_if(
            &mut passed,
            &CHECKS[15 + offset],
            matches!(refused.get("effect"), Some(Value::Null))
                && state(&refused)
                    .get("notice_error")
                    .is_some_and(twin::truthy)
                && state(&refused)
                    .get("remote")
                    .is_none_or(|rows| !twin::truthy(rows)),
        );
    }

    set_path(
        &mut st,
        &["profiles"],
        Value::Arr(vec![
            twin::obj(vec![
                ("name".into(), s("Parent connection")),
                ("protocol".into(), s("chat")),
                ("endpoint".into(), s("http://parent.test/v1/chat/completions")),
                ("model".into(), s("parent")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
                ("tool_format".into(), s("structured")),
            ]),
            twin::obj(vec![
                ("name".into(), s("Child connection")),
                ("protocol".into(), s("chat")),
                ("endpoint".into(), s("http://child.test/v1/chat/completions")),
                ("model".into(), s("child")),
                ("plain".into(), twin::b(true)),
                ("stream".into(), twin::b(false)),
                ("tool_format".into(), s("structured")),
            ]),
        ]),
    );
    set_path(
        &mut st,
        &["agents"],
        Value::Arr(vec![
            twin::obj(vec![
                ("name".into(), s("Parent")),
                ("model".into(), s("0")),
                ("tools".into(), s("delegate,drive_list")),
                ("subagents".into(), Value::Arr(vec![n("1")])),
            ]),
            twin::obj(vec![
                ("name".into(), s("Child")),
                ("model".into(), s("1")),
                ("tools".into(), s("drive_list")),
                ("subagents".into(), Value::Arr(vec![])),
            ]),
        ]),
    );

    let mut d = event(
        &mut controller,
        Some(&st),
        "send",
        twin::obj(vec![(
            "draft".into(),
            s("Use a child to inspect the remote folder."),
        )]),
        &scratch_text,
    );
    d = model(
        &mut controller,
        &d,
        Some("delegate"),
        Some(twin::obj(vec![
            ("agent".into(), n("1")),
            (
                "task".into(),
                s("List the folder and report its actual files."),
            ),
        ])),
        "",
        &scratch_text,
    );
    keep_assert(text(field(state(&d), "dialog")) == "approval");
    d = event(
        &mut controller,
        Some(state(&d)),
        "approve",
        twin::obj(vec![]),
        &scratch_text,
    );
    keep_assert(field(state(&d), "request_index") == &n("1"));
    d = model(
        &mut controller,
        &d,
        Some("drive_list"),
        Some(twin::obj(vec![("path".into(), s(""))])),
        "",
        &scratch_text,
    );
    keep_assert(text(field(state(&d), "dialog")) == "approval");
    d = event(
        &mut controller,
        Some(state(&d)),
        "approve",
        twin::obj(vec![]),
        &scratch_text,
    );
    route(field(&d, "effect"), "PROPFIND", "/share/");
    d = reply(&mut controller, &d, &xml(&[allowed[0]]), 207, &scratch_text);
    keep_assert(
        field(state(&d), "request_index") == &n("1")
            && text(field(field(&d, "effect"), "wire")).contains("a%20b.txt"),
    );
    let evidence = inquiry_evidence(state(&d), "drive_list");
    keep_if(
        &mut passed,
        &CHECKS[23],
        evidence.len() == 1 && evidence[0].contains("100.64.0.2"),
    );
    d = model(
        &mut controller,
        &d,
        None,
        None,
        "The observed file is a b.txt.",
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[24],
        field(state(&d), "request_index") == &n("0")
            && text(field(field(&d, "effect"), "wire"))
                .contains("The observed file is a b.txt."),
    );
    d = model(
        &mut controller,
        &d,
        None,
        None,
        "Reported the observed file.",
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[25],
        !field(state(&d), "busy").bool_() && matches!(d.get("effect"), Some(Value::Null)),
    );

    let creations = [
        ("text", "Original network contents.".to_string()),
        ("empty", String::new()),
        ("Unicode", "UTF-8: 日本語\nnext line".to_string()),
        (
            "maximum text size",
            "x".repeat(1024 * 1024),
        ),
    ];
    let mut create_index = 26;
    for (_label, content) in creations {
        let created = create(&mut controller, &st, &content, &scratch_text);
        route(field(&created, "effect"), "PUT", "/share/new%20file.txt");
        keep_assert(
            text(field(field(&created, "effect"), "wire"))
                .contains("\r\nIf-None-Match: *\r\n"),
        );
        let acknowledged = reply(&mut controller, &created, "", 201, &scratch_text);
        route(
            field(&acknowledged, "effect"),
            "GET",
            "/share/new%20file.txt",
        );
        keep_assert(
            number(field(field(&acknowledged, "effect"), "limit")) >= content.len() as u64 + 256
                && text(field(field(state(&acknowledged), "drive_creation"), "phase")) == "verifying"
                && !Value::to_string(field(&acknowledged, "ui")).contains("saved successfully"),
        );
        let verified = reply(&mut controller, &acknowledged, &content, 200, &scratch_text);
        let ui = Value::to_string(field(&verified, "ui"));
        keep_assert(
            matches!(verified.get("effect"), Some(Value::Null))
                && matches!(
                    state(&verified).get("drive_creation"),
                    None | Some(Value::Null)
                )
                && ui.contains("readback verified")
                && ui.contains(&sha256_hex(content.as_bytes())),
        );
        keep(&mut passed, &CHECKS[create_index]);
        let duplicate = reply_raw(
            &mut controller,
            state(&verified),
            field(&acknowledged, "effect"),
            &content,
            200,
            &scratch_text,
        );
        keep_if(
            &mut passed,
            &CHECKS[create_index + 1],
            state(&duplicate) == state(&verified)
                && matches!(duplicate.get("effect"), Some(Value::Null)),
        );
        create_index += 2;
    }

    let created = create(&mut controller, &st, "Expected", &scratch_text);
    let acknowledged = reply(&mut controller, &created, "", 204, &scratch_text);
    let wrong = reply(&mut controller, &acknowledged, "Different", 200, &scratch_text);
    keep_if(
        &mut passed,
        &CHECKS[34],
        matches!(wrong.get("effect"), Some(Value::Null))
            && text(field(state(&wrong), "notice")).to_lowercase().contains("unconfirmed")
            && !field(state(&wrong), "busy").bool_(),
    );

    let created = create(&mut controller, &st, "Expected", &scratch_text);
    let acknowledged = reply(&mut controller, &created, "", 201, &scratch_text);
    let missing = reply(&mut controller, &acknowledged, "Missing", 404, &scratch_text);
    keep_if(
        &mut passed,
        &CHECKS[35],
        matches!(missing.get("effect"), Some(Value::Null))
            && text(field(state(&missing), "notice"))
                .to_lowercase()
                .contains("unconfirmed"),
    );

    let created = create(&mut controller, &st, "Expected", &scratch_text);
    let acknowledged = reply(&mut controller, &created, "", 201, &scratch_text);
    let purpose = text(field(field(&acknowledged, "effect"), "purpose")).to_string();
    let ticket = field(field(&acknowledged, "effect"), "drive_ticket").clone();
    let broken = event(
        &mut controller,
        Some(state(&acknowledged)),
        "failure",
        twin::obj(vec![
            ("purpose".into(), s(&purpose)),
            (
                "error".into(),
                s("Controlled verification transport failure"),
            ),
            ("sent".into(), twin::b(false)),
            ("drive_ticket".into(), ticket),
        ]),
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[36],
        matches!(broken.get("effect"), Some(Value::Null))
            && text(field(state(&broken), "notice"))
                .to_lowercase()
                .contains("unconfirmed")
            && !field(state(&broken), "busy").bool_(),
    );
    let recovered = create(
        &mut controller,
        state(&broken),
        "Separate reviewed contents",
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[37],
        text(field(field(&recovered, "effect"), "wire")).starts_with("PUT "),
    );

    let created = create(&mut controller, &st, "Expected", &scratch_text);
    let cancelled = event(
        &mut controller,
        Some(state(&created)),
        "cancel",
        twin::obj(vec![]),
        &scratch_text,
    );
    let late = reply_raw(
        &mut controller,
        state(&cancelled),
        field(&created, "effect"),
        "",
        201,
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[38],
        matches!(late.get("effect"), Some(Value::Null))
            && state(&late) == state(&cancelled)
            && text(field(state(&late), "notice")).contains("unconfirmed"),
    );

    let created = create(&mut controller, &st, "Expected", &scratch_text);
    let acknowledged = reply(&mut controller, &created, "", 201, &scratch_text);
    let cancelled = event(
        &mut controller,
        Some(state(&acknowledged)),
        "cancel",
        twin::obj(vec![]),
        &scratch_text,
    );
    let late = reply_raw(
        &mut controller,
        state(&cancelled),
        field(&acknowledged, "effect"),
        "Expected",
        200,
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[39],
        matches!(late.get("effect"), Some(Value::Null))
            && state(&late) == state(&cancelled),
    );

    let created = create(&mut controller, &st, "Expected", &scratch_text);
    let acknowledged = reply(&mut controller, &created, "", 201, &scratch_text);
    let mut changed = state(&acknowledged).clone();
    set_path(
        &mut changed,
        &["drive", "url"],
        s("https://other.test/share/"),
    );
    let refused = reply_raw(
        &mut controller,
        &changed,
        field(&acknowledged, "effect"),
        "Expected",
        200,
        &scratch_text,
    );
    keep_assert(
        matches!(refused.get("effect"), Some(Value::Null)) && state(&refused) == &changed,
    );
    let cancelled = event(
        &mut controller,
        Some(state(&refused)),
        "cancel",
        twin::obj(vec![]),
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[40],
        text(field(state(&cancelled), "notice"))
            .to_lowercase()
            .contains("unconfirmed"),
    );

    set_path(
        &mut st,
        &["agents", "0", "tools"],
        s("delegate,drive_list,drive_create"),
    );
    set_path(
        &mut st,
        &["agents", "1", "tools"],
        s("drive_list,drive_create"),
    );
    let mut d = event(
        &mut controller,
        Some(&st),
        "send",
        twin::obj(vec![(
            "draft".into(),
            s("Have the child save a network file and verify it."),
        )]),
        &scratch_text,
    );
    d = model(
        &mut controller,
        &d,
        Some("delegate"),
        Some(twin::obj(vec![
            ("agent".into(), n("1")),
            (
                "task".into(),
                s("Create new file.txt with Expected and verify it."),
            ),
        ])),
        "",
        &scratch_text,
    );
    d = event(
        &mut controller,
        Some(state(&d)),
        "approve",
        twin::obj(vec![]),
        &scratch_text,
    );
    d = model(
        &mut controller,
        &d,
        Some("drive_create"),
        Some(twin::obj(vec![
            ("path".into(), s("new file.txt")),
            ("content".into(), s("Expected")),
        ])),
        "",
        &scratch_text,
    );
    d = event(
        &mut controller,
        Some(state(&d)),
        "approve",
        twin::obj(vec![]),
        &scratch_text,
    );
    let write = d.clone();
    d = reply(&mut controller, &d, "", 201, &scratch_text);
    keep_if(
        &mut passed,
        &CHECKS[41],
        field(state(&d), "request_index") == &n("1")
            && text(field(field(&d, "effect"), "purpose")) == "drive_create_verify",
    );
    let stale = reply_raw(
        &mut controller,
        state(&d),
        field(&write, "effect"),
        "",
        201,
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[42],
        state(&stale) == state(&d) && matches!(stale.get("effect"), Some(Value::Null)),
    );
    d = reply(&mut controller, &d, "Expected", 200, &scratch_text);
    keep_assert(
        field(state(&d), "request_index") == &n("1")
            && text(field(field(&d, "effect"), "wire")).contains("readback verified"),
    );
    let evidence = inquiry_evidence(state(&d), "drive_create");
    keep_if(
        &mut passed,
        &CHECKS[43],
        evidence.len() == 1 && evidence[0].contains("readback verified"),
    );
    d = model(
        &mut controller,
        &d,
        None,
        None,
        "The file was saved and its readback matched.",
        &scratch_text,
    );
    keep_assert(
        field(state(&d), "request_index") == &n("0")
            && text(field(field(&d, "effect"), "wire")).contains("readback matched"),
    );
    d = model(
        &mut controller,
        &d,
        None,
        None,
        "The verified save completed.",
        &scratch_text,
    );
    keep_if(
        &mut passed,
        &CHECKS[44],
        !field(state(&d), "busy").bool_(),
    );

    let mut d = event(
        &mut controller,
        Some(&st),
        "send",
        twin::obj(vec![(
            "draft".into(),
            s("Have the child save a network file."),
        )]),
        &scratch_text,
    );
    d = model(
        &mut controller,
        &d,
        Some("delegate"),
        Some(twin::obj(vec![
            ("agent".into(), n("1")),
            (
                "task".into(),
                s("Create new file.txt with Expected."),
            ),
        ])),
        "",
        &scratch_text,
    );
    d = event(
        &mut controller,
        Some(state(&d)),
        "approve",
        twin::obj(vec![]),
        &scratch_text,
    );
    d = model(
        &mut controller,
        &d,
        Some("drive_create"),
        Some(twin::obj(vec![
            ("path".into(), s("new file.txt")),
            ("content".into(), s("Expected")),
        ])),
        "",
        &scratch_text,
    );
    d = event(
        &mut controller,
        Some(state(&d)),
        "approve",
        twin::obj(vec![]),
        &scratch_text,
    );
    d = reply(&mut controller, &d, "", 201, &scratch_text);
    d = reply(&mut controller, &d, "Wrong", 200, &scratch_text);
    keep_if(
        &mut passed,
        &CHECKS[45],
        field(state(&d), "request_index") == &n("0")
            && text(field(field(&d, "effect"), "purpose")) == "model"
            && text(field(field(&d, "effect"), "wire")).contains("unconfirmed"),
    );

    // The redirect limits of a page read are driven on their own controller:
    // the agents this group configures carry no web read, so the read is
    // driven by a tree whose own default agent carries one.
    let redirect_scratch = twin::create_scratch(std::path::Path::new(&root_text), "checks-");
    let redirect_scratch_text = redirect_scratch
        .to_str()
        .unwrap_or_else(|| panic!("redirect scratch root is not UTF-8"))
        .to_string();
    let mut redirect_controller = Controller::spawn(&root_text);
    leg_redirect_limits(&mut redirect_controller, &redirect_scratch_text, &mut passed);
    redirect_controller.finish();
    let _ = std::fs::remove_dir_all(&redirect_scratch);

    controller.finish();
    let _ = std::fs::remove_dir_all(&scratch);
    if passed.len() != CHECKS.len() {
        panic!("emitted {} checks, expected {}", passed.len(), CHECKS.len());
    }
    let body = twin::obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("network_drive", &body, false);
    if !std::path::Path::new(REPORT).is_file() {
        panic!("{REPORT} was not written");
    }
    println!("{} connected WebDAV checks passed", passed.len());
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

/// The redirect limits of a page read.
///
/// The chain of redirects the product follows, the hop it refuses to follow,
/// and the redirect that points back at an address the chain already visited.
/// Every refusal is driven with real response bytes: the twin hands the
/// product a 301 with a Location header and the product decides what to do
/// with it. The product is started, it issues the read, and it answers, so
/// this group measures a runtime behaviour and not the text of a source file.
fn leg_redirect_limits(controller: &mut Controller, root: &str, passed: &mut Vec<String>) {
    let (mut st, effect) = open_web_read(controller, root, "https://page.invalid/hop1");
    let mut ticket = field(&effect, "web_ticket").clone();
    let mut tickets = vec![ticket.to_string()];
    let mut followed: Vec<(String, String, String)> = Vec::new();
    let mut refusal: Option<(usize, String, String)> = None;
    for hop in 1..=6 {
        let location = format!("/hop{}", hop + 1);
        let reply = event_value(
            controller,
            Some(&st),
            "network_result",
            twin::obj(vec![
                ("purpose".into(), s("web_tool")),
                ("web_ticket".into(), ticket.clone()),
                ("data".into(), s(&b64_encode(&redirect_wire(&location)))),
            ]),
            root,
        );
        let next_effect = field(&reply, "effect").clone();
        let next_state = state(&reply).clone();
        match next_effect.get("web_ticket") {
            Some(next) => {
                followed.push((
                    optional_text(&next_effect, "purpose"),
                    optional_text(&next_state, "notice"),
                    next.to_string(),
                ));
                tickets.push(next.to_string());
                ticket = next.clone();
                st = next_state;
            }
            None => {
                // No web ticket was minted, so no further request was sent.
                refusal = Some((
                    hop,
                    optional_text(&next_effect, "purpose"),
                    chat_text(&next_state),
                ));
                break;
            }
        }
    }
    let mut unique = tickets.clone();
    unique.sort();
    unique.dedup();
    keep_if(
        passed,
        &CHECKS[46],
        followed.len() == 5
            && followed.iter().all(|(purpose, notice, _)| {
                purpose == "web_tool" && notice == "Following the website's redirect."
            })
            && unique.len() == tickets.len()
            && refusal
                .as_ref()
                .is_some_and(|(hop, purpose, messages)| {
                    *hop == 6
                        && purpose == "model"
                        && messages.contains("\"failure_kind\":\"redirect\"")
                        && messages.contains(
                            "Web redirect limit reached (five hops); no additional request was sent",
                        )
                }),
    );
    // The loop: a Location that resolves to an address the chain already
    // visited is refused as a loop, and no further request is sent.
    let (st, effect) = open_web_read(controller, root, "https://page.invalid/loop1");
    let ticket = field(&effect, "web_ticket").clone();
    let reply = event_value(
        controller,
        Some(&st),
        "network_result",
        twin::obj(vec![
            ("purpose".into(), s("web_tool")),
            ("web_ticket".into(), ticket),
            ("data".into(), s(&b64_encode(&redirect_wire("/loop1")))),
        ]),
        root,
    );
    let effect = field(&reply, "effect").clone();
    let messages = chat_text(state(&reply));
    keep_if(
        passed,
        &CHECKS[47],
        effect.get("web_ticket").is_none()
            && optional_text(&effect, "purpose") == "model"
            && messages.contains("\"failure_kind\":\"redirect\"")
            && messages.contains("Web redirect loop detected; no additional request was sent"),
    );
}

/// A profile the model proposes its page read through. The endpoint is a
/// closed port on the loopback interface: the model reply the twin hands the
/// product is the product's own input, and the product never opens a socket to
/// it.
fn web_profile() -> Value {
    twin::obj(vec![
        ("name".into(), s("Primary")),
        ("model".into(), s("0")),
        ("protocol".into(), s("chat")),
        ("endpoint".into(), s("http://127.0.0.1:9/v1/chat/completions")),
        ("plain".into(), twin::b(true)),
        ("stream".into(), twin::b(false)),
        ("tool_format".into(), s("structured")),
        ("key".into(), s("")),
    ])
}

/// The chat completion the model returns: the proposal is the content of the
/// model's own message, so the product reads the proposal out of the reply
/// the model sent.
fn chat_body(content: &str) -> String {
    twin::obj(vec![(
        "choices".into(),
        Value::Arr(vec![twin::obj(vec![
            ("index".into(), n("0")),
            ("message".into(), twin::obj(vec![("content".into(), s(content))])),
            ("finish_reason".into(), s("stop")),
        ])]),
    )])
    .to_string()
}

/// A 301 with a Location header: the bytes a redirecting site answers with.
fn redirect_wire(location: &str) -> Vec<u8> {
    format!("HTTP/1.1 301 Moved\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n").into_bytes()
}

/// The product's own answer to one event, with the product's own refusal text
/// when it refuses.
fn event_value(
    controller: &mut Controller,
    st: Option<&Value>,
    action: &str,
    inputs: Value,
    root: &str,
) -> Value {
    let envelope = twin::obj(vec![
        ("op".into(), s("event")),
        ("state".into(), st.cloned().unwrap_or(Value::Null)),
        ("action".into(), s(action)),
        ("inputs".into(), inputs),
        ("root".into(), s(root)),
    ]);
    let reply = controller.send(&envelope);
    match reply.get("value") {
        Some(value) => value.clone(),
        None => panic!("the product refused the event: {}", reply.to_string()),
    }
}

/// The state after the product has issued the request for a page, and the
/// effect that carries that request.
fn open_web_read(controller: &mut Controller, root: &str, url: &str) -> (Value, Value) {
    let resumed = event_value(controller, None, "resume", twin::obj(vec![]), root);
    let mut st = state(&resumed).clone();
    set_path(&mut st, &["profiles"], Value::Arr(vec![web_profile()]));
    let turn = event_value(
        controller,
        Some(&st),
        "send",
        twin::obj(vec![("draft".into(), s("Read the fixture page."))]),
        root,
    );
    let after_send = state(&turn).clone();
    let ticket = field(&after_send, "model_expected").clone();
    let proposal = format!("{{\"tool\":\"web_read\",\"args\":{{\"url\":\"{url}\"}}}}");
    let body = chat_body(&proposal);
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body.as_bytes());
    let fed = event_value(
        controller,
        Some(&after_send),
        "network_result",
        twin::obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), ticket),
            ("data".into(), s(&b64_encode(&wire))),
        ]),
        root,
    );
    let after_feed = state(&fed).clone();
    let effect = field(&fed, "effect").clone();
    if optional_text(&effect, "purpose") == "web_tool" {
        return (after_feed, effect);
    }
    // The standing public read was not approved on its own; the operator
    // approves it, and the request the product issues then is the one under
    // test.
    let approved = event_value(controller, Some(&after_feed), "approve", twin::obj(vec![]), root);
    (state(&approved).clone(), field(&approved, "effect").clone())
}

/// Every message the chat holds, joined: the product's own report of a failed
/// request is a message in the chat.
fn chat_text(st: &Value) -> String {
    let empty: Vec<Value> = Vec::new();
    let mut out = String::new();
    for chat in st.get("chats").map(|value| value.arr()).unwrap_or(&empty) {
        for message in chat.get("messages").map(|value| value.arr()).unwrap_or(&empty) {
            out.push_str(message.get("content").map(|value| value.str_()).unwrap_or(""));
            out.push('\n');
        }
    }
    out
}

/// A member of an object, as text, or the empty text when it is not there.
fn optional_text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(|value| value.str_().to_string())
        .unwrap_or_default()
}

fn reply(
    controller: &mut Controller,
    request: &Value,
    body: &str,
    status: u16,
    root: &str,
) -> Value {
    reply_raw(
        controller,
        state(request),
        field(request, "effect"),
        body,
        status,
        root,
    )
}

fn reply_raw(
    controller: &mut Controller,
    st: &Value,
    effect: &Value,
    body: &str,
    status: u16,
    root: &str,
) -> Value {
    let mut inputs = twin::obj(vec![]);
    for key in [
        "purpose",
        "child_ticket",
        "child_owner",
        "model_ticket",
        "drive_ticket",
    ] {
        if let Some(value) = effect.get(key) {
            set_path(&mut inputs, &[key], value.clone());
        }
    }
    let bytes = body.as_bytes();
    let mut wire = format!(
        "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    wire.extend_from_slice(bytes);
    set_path(&mut inputs, &["data"], s(&b64_encode(&wire)));
    event(controller, Some(st), "network_result", inputs, root)
}

fn model(
    controller: &mut Controller,
    request: &Value,
    tool: Option<&str>,
    args: Option<Value>,
    text: &str,
    root: &str,
) -> Value {
    let mut message = twin::obj(vec![("content".into(), s(text))]);
    if let Some(name) = tool {
        let arguments = args.unwrap_or_else(|| twin::obj(vec![]));
        set_path(
            &mut message,
            &["tool_calls"],
            Value::Arr(vec![twin::obj(vec![
                ("id".into(), s("call_one")),
                ("type".into(), s("function")),
                (
                    "function".into(),
                    twin::obj(vec![
                        ("name".into(), s(name)),
                        ("arguments".into(), s(&arguments.to_string())),
                    ]),
                ),
            ])]),
        );
    }
    let body = twin::obj(vec![(
        "choices".into(),
        Value::Arr(vec![twin::obj(vec![
            ("index".into(), n("0")),
            ("message".into(), message),
            (
                "finish_reason".into(),
                s(if tool.is_some() { "tool_calls" } else { "stop" }),
            ),
        ])]),
    )]);
    reply(controller, request, &body.to_string(), 200, root)
}

fn create(
    controller: &mut Controller,
    original: &Value,
    content: &str,
    root: &str,
) -> Value {
    let mut base = original.clone();
    set_path(&mut base, &["dialog"], s("drive_editor"));
    set_path(&mut base, &["edit"], twin::obj(vec![]));
    let reviewed = event(
        controller,
        Some(&base),
        "drive_create_review",
        twin::obj(vec![
            ("path".into(), s("new file.txt")),
            ("content".into(), s(content)),
        ]),
        root,
    );
    assert_eq!(text(field(state(&reviewed), "dialog")), "approval");
    event(
        controller,
        Some(state(&reviewed)),
        "approve",
        twin::obj(vec![]),
        root,
    )
}

fn route(effect: &Value, method: &str, path: &str) {
    keep_assert(text(field(effect, "host")) == "100.64.0.2");
    keep_assert(field(effect, "port") == &n("5006"));
    keep_assert(matches!(effect.get("tls"), Some(Value::Bool(true))));
    keep_assert(text(field(effect, "tls_host")) == "nas.example.test");
    let wire = text(field(effect, "wire"));
    keep_assert(wire.starts_with(&format!("{method} {path} HTTP/1.1\r\n")));
    keep_assert(wire.contains("\r\nHost: nas.example.test:5006\r\n"));
}

fn xml(hrefs: &[&str]) -> String {
    let mut out = String::from("<d:multistatus xmlns:d=\"DAV:\">");
    for href in hrefs {
        out.push_str("<d:response><d:href>");
        out.push_str(href);
        out.push_str("</d:href><d:propstat><d:prop><d:displayname>Observed file</d:displayname></d:prop></d:propstat></d:response>");
    }
    out.push_str("</d:multistatus>");
    out
}

fn inquiry_evidence(st: &Value, tool: &str) -> Vec<String> {
    let mut output = Vec::new();
    let items = field(field(st, "inquiry_records"), "items").arr();
    for item in items {
        for evidence in field(item, "evidence").arr() {
            if text(field(evidence, "tool")) == tool {
                output.push(text(field(evidence, "output")).to_string());
            }
        }
    }
    output
}

fn sha256_hex(bytes: &[u8]) -> String {
    twin::sha256_digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn number(value: &Value) -> u64 {
    match value {
        Value::Num(token) => token
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("expected an integer, got {token}")),
        other => panic!("expected a number, got {}", other.to_string()),
    }
}

fn keep(passed: &mut Vec<String>, name: &str) {
    passed.push(name.to_string());
}

fn keep_if(passed: &mut Vec<String>, name: &str, condition: bool) {
    if !condition {
        panic!("check failed: {name}");
    }
    passed.push(name.to_string());
}

fn keep_assert(condition: bool) {
    if !condition {
        panic!("assertion failed");
    }
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

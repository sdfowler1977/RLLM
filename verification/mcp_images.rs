//! Exercise reviewed MCP image results through the actual local HTTP and media paths.
mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{b64_decode, b64_encode, crc32, n, obj, s, sha256_bytes, truthy, zlib_compress, Controller, Value};

const JPEG_ENCODED: &str = concat!(
    "/9j/4AAQSkZJRgABAQAAAAAAAAD/2wBDAAUDBAQEAwUEBAQFBQUGBwwIBwcHBw8LCwkMEQ8SEhEPERETFhwXExQaFRERGCEY",
    "Gh0dHx8fExciJCIeJBweHx7/2wBDAQUFBQcGBw4ICA4eFBEUHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4e",
    "Hh4eHh4eHh4eHh4eHh7/wAARCAABAAEDAREAAhEBAxEB/8QAFAABAAAAAAAAAAAAAAAAAAAAB//EABQQAQAAAAAAAAAAAAAA",
    "AAAAAAD/xAAVAQEBAAAAAAAAAAAAAAAAAAAHCP/EABQRAQAAAAAAAAAAAAAAAAAAAAD/2gAMAwEAAhEDEQA/ACwdLLf/2Q==",
);

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = Scratch(twin::create_scratch(&project, "mcp-images"));
    let workspace = scratch
        .0
        .to_str()
        .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
        .to_string();
    let peer = Peer::start();
    let mut controller = Controller::spawn(&project_text);
    let (checks, requests) = run(&mut controller, &peer, &workspace);
    let body = obj(vec![
        ("passed".into(), n(&checks.len().to_string())),
        (
            "checks".into(),
            Value::Arr(checks.iter().map(|name| s(name)).collect()),
        ),
        ("requests".into(), n(&requests.to_string())),
    ]);
    twin::report_value("mcp_images", &body, true);
    println!("{}", dumps(&body));
    controller.finish();
}

fn run(controller: &mut Controller, peer: &Peer, workspace: &str) -> (Vec<String>, usize) {
    let mut checks = Vec::new();
    let png = png_bytes();
    let encoded = b64_encode(&png);
    let jpeg = b64_decode(JPEG_ENCODED);
    let text = text_item();
    let png_image = image(&encoded, "image/png");
    let declared_jpeg = image(&encoded, "image/jpeg");
    let trailing = image(&format!("{encoded}trailing"), "image/png");
    let gif = image(&encoded, "image/gif");
    let mixed = obj(vec![(
        "content".into(),
        Value::Arr(vec![
            text.clone(),
            png_image.clone(),
            declared_jpeg,
            trailing,
            gif,
            resource_link(),
        ]),
    )]);

    let resumed = controller.event(None, "resume", obj(vec![]), workspace);
    let settings = controller.event(Some(state_of(&resumed)), "mcp_client_settings", obj(vec![]), workspace);
    let saved_connection = controller.event(
        Some(state_of(&settings)),
        "mcp_client_save",
        obj(vec![
            ("endpoint".into(), s(&format!("http://127.0.0.1:{}/mcp", peer.port()))),
            ("key".into(), s("image-test")),
            ("access".into(), twin::b(true)),
        ]),
        workspace,
    );

    let mut d = call(controller, peer, state_of(&saved_connection), &mixed, workspace, &mut checks);
    keep(
        &mut checks,
        "manual MCP result exposes supported image controls",
        ["mcp_image_save:1", "mcp_image_save:2", "mcp_image_save:3"]
            .iter()
            .all(|name| buttons(&d, "manual MCP result exposes supported image controls").iter().any(|event| event == name)),
    );
    keep(
        &mut checks,
        "mixed result preserves exact typed payload and text",
        output_equals(&d, &mixed, "mixed result preserves exact typed payload and text"),
    );
    keep(
        &mut checks,
        "network callback does not write image bytes",
        directory_entries(&Path::new(workspace).join("service-files")).is_empty() && effect_is_null(&d),
    );
    let shown = buttons(&d, "unsupported MIME and resource URI do not gain save controls");
    keep(
        &mut checks,
        "unsupported MIME and resource URI do not gain save controls",
        !shown.iter().any(|event| event == "mcp_image_save:4") && !shown.iter().any(|event| event == "mcp_image_save:5"),
    );

    let raw = controller.event(Some(state_of(&d)), "mcp_image_raw", obj(vec![]), workspace);
    keep(
        &mut checks,
        "raw details expose exact typed result without burying image controls",
        raw_details(&raw, &d, "raw details expose exact typed result without burying image controls"),
    );
    let hidden = "hidden save action is refused in the raw details view";
    let blocked = controller.event(Some(state_of(&raw)), &save_action(&d, 1, hidden), obj(vec![]), workspace);
    keep(
        &mut checks,
        hidden,
        notice_error(&blocked) && directory_entries(&Path::new(workspace).join("service-files")).is_empty(),
    );
    d = controller.event(Some(state_of(&raw)), "mcp_image_back", obj(vec![]), workspace);
    let closed = controller.event(Some(state_of(&d)), "close", obj(vec![]), workspace);
    let closed_label = "closed result does not accept a stale save action";
    let closed_save = controller.event(Some(state_of(&closed)), &save_action(&d, 1, closed_label), obj(vec![]), workspace);
    keep(
        &mut checks,
        closed_label,
        notice_error(&closed_save) && directory_entries(&Path::new(workspace).join("service-files")).is_empty(),
    );

    for index in [0usize, 2, 3, 4, 5, 99] {
        let refused = format!("image selection {index} refuses without writing");
        let preserved = format!("image selection {index} preserves exact result");
        let bad = controller.event(Some(state_of(&d)), &save_action(&d, index, &refused), obj(vec![]), workspace);
        keep(
            &mut checks,
            &refused,
            notice_error(&bad) && effect_is_null(&bad) && directory_entries(&Path::new(workspace).join("service-files")).is_empty(),
        );
        keep(&mut checks, &preserved, same_output(&bad, &d));
    }

    let changed = "changed result cannot be saved as the owned image";
    let mut tampered = state_of(&d).clone();
    let output_text = match tampered.get("output") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("{changed}"),
    };
    put(&mut tampered, "output", s(&format!("{output_text} ")));
    let bad = controller.event(Some(&tampered), &save_action(&d, 1, changed), obj(vec![]), workspace);
    keep(
        &mut checks,
        changed,
        notice_error(&bad) && directory_entries(&Path::new(workspace).join("service-files")).is_empty(),
    );

    let adopted = "changed workspace cannot adopt the earlier image";
    let other = Path::new(workspace).join("other");
    std::fs::create_dir(&other).unwrap_or_else(|error| panic!("{adopted}: {error}"));
    let other_text = other.to_str().unwrap_or_else(|| panic!("{adopted}"));
    let bad = controller.event(Some(state_of(&d)), &save_action(&d, 1, adopted), obj(vec![]), other_text);
    keep(
        &mut checks,
        adopted,
        notice_error(&bad) && directory_entries(&other.join("service-files")).is_empty(),
    );

    let saved_label = "explicit save retains exact original image bytes";
    let saved = controller.event(Some(state_of(&d)), &save_action(&d, 1, saved_label), obj(vec![]), workspace);
    let artifact = state_of(&saved)
        .get("service_artifact")
        .unwrap_or_else(|| panic!("{saved_label}"));
    let artifact_path = match artifact.get("path") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("{saved_label}"),
    };
    let stored = std::fs::read(Path::new(workspace).join(&artifact_path)).unwrap_or_else(|error| panic!("{saved_label}: {error}"));
    let digest = sha256_bytes(&png);
    keep(
        &mut checks,
        saved_label,
        stored == png && matches!(artifact.get("sha256"), Some(Value::Str(text)) if text == &digest),
    );
    keep(
        &mut checks,
        "image saved locally with MIME and source provenance",
        text_is(artifact, "mime", "image/png") && text_is(artifact, "kind", "image") && text_is(artifact, "route", "mcp"),
    );
    keep(
        &mut checks,
        "image is visible in the conversation artifact list",
        artifact_listed(&saved, artifact.get("path").unwrap_or_else(|| panic!("image is visible in the conversation artifact list"))),
    );
    let offered = buttons(&saved, "existing preview and export actions are offered");
    keep(
        &mut checks,
        "existing preview and export actions are offered",
        offered.iter().any(|event| event == "media_preview") && offered.iter().any(|event| event == "media_export"),
    );

    let preview_label = "preview uses the existing native-owned path and record";
    let preview_event = controller.event(Some(state_of(&saved)), "media_preview", obj(vec![]), workspace);
    let preview = match preview_event.get("effect") {
        Some(Value::Obj(_)) => preview_event.get("effect").unwrap_or_else(|| panic!("{preview_label}")),
        _ => panic!("{preview_label}"),
    };
    let preview_path = match preview.get("path") {
        Some(Value::Str(text)) => text.as_str(),
        _ => panic!("{preview_label}"),
    };
    let preview_bytes = std::fs::read(preview_path).unwrap_or_else(|error| panic!("{preview_label}: {error}"));
    keep(
        &mut checks,
        preview_label,
        text_is(preview, "kind", "media_preview") && preview.get("record") == Some(artifact) && preview_bytes == png,
    );
    let verified = controller.send(&with_op(preview, "media_verify"));
    keep(
        &mut checks,
        "native preview readback verifies original bytes",
        match (verified.get("value").and_then(|value| value.get("path")), preview.get("path")) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        },
    );

    let export_label = "native export retains detected PNG and existing transfer contract";
    let mut export_state = state_of(&saved).clone();
    set_phone_default(&mut export_state, export_label);
    put(&mut export_state, "phone_uri", s("content://fixture/tree/owned"));
    let exported = controller.event(Some(&export_state), "media_export", obj(vec![]), workspace);
    let prepare = exported
        .get("effect")
        .and_then(|effect| effect.get("prepare"))
        .unwrap_or_else(|| panic!("{export_label}"));
    keep(
        &mut checks,
        export_label,
        matches!(prepare.get("name"), Some(Value::Str(text)) if text.ends_with(".png")) && text_is(prepare, "mime", "image/png"),
    );

    let again_label = "repeat save reuses the same still-verified artifact";
    let again = controller.event(Some(state_of(&saved)), &save_action(&d, 1, again_label), obj(vec![]), workspace);
    let again_path = state_of(&again).get("service_artifact").and_then(|record| record.get("path"));
    keep(
        &mut checks,
        again_label,
        again_path.is_some() && again_path == artifact.get("path") && directory_entries(&Path::new(workspace).join("service-files")).len() == 1,
    );

    let mut flipped = png.clone();
    flipped[0] ^= 1;
    std::fs::write(Path::new(workspace).join(&artifact_path), &flipped)
        .unwrap_or_else(|error| panic!("same-size tamper fails native preview verification: {error}"));
    let tampered_verify = controller.send(&with_op(preview, "media_verify"));
    keep(
        &mut checks,
        "same-size tamper fails native preview verification",
        tampered_verify.get("error").is_some(),
    );

    let error_result = obj(vec![
        ("content".into(), Value::Arr(vec![text.clone(), png_image.clone()])),
        ("isError".into(), twin::b(true)),
    ]);
    let error = call(controller, peer, state_of(&saved), &error_result, workspace, &mut checks);
    let error_label = "reported tool error keeps the raw result but has no image save";
    keep(
        &mut checks,
        error_label,
        output_equals(&error, &error_result, error_label)
            && !buttons(&error, error_label).iter().any(|event| event.starts_with("mcp_image")),
    );

    let ordinary_result = obj(vec![("content".into(), Value::Arr(vec![text.clone()]))]);
    let ordinary = call(controller, peer, state_of(&error), &ordinary_result, workspace, &mut checks);
    let ordinary_label = "ordinary text tool result remains in its original UI";
    keep(
        &mut checks,
        ordinary_label,
        text_is(state_of(&ordinary), "dialog", "output") && output_equals(&ordinary, &ordinary_result, ordinary_label),
    );

    let mut paged_content = vec![text];
    for _ in 0..8 {
        paged_content.push(png_image.clone());
    }
    paged_content.push(image(JPEG_ENCODED, "image/jpeg"));
    let paged_result = obj(vec![("content".into(), Value::Arr(paged_content))]);
    let paged = call(controller, peer, state_of(&ordinary), &paged_result, workspace, &mut checks);
    let first_page = buttons(&paged, "first image page has eight original result slots");
    keep(
        &mut checks,
        "first image page has eight original result slots",
        first_page.iter().any(|event| event == "mcp_image_next")
            && first_page.iter().any(|event| event == "mcp_image_save:8")
            && !first_page.iter().any(|event| event == "mcp_image_save:9"),
    );
    let page_two = controller.event(Some(state_of(&paged)), "mcp_image_next", obj(vec![]), workspace);
    let second = buttons(&page_two, "second image page preserves the original image index");
    keep(
        &mut checks,
        "second image page preserves the original image index",
        second.iter().any(|event| event == "mcp_image_save:9")
            && second.iter().any(|event| event == "mcp_image_previous")
            && !second.iter().any(|event| event == "mcp_image_next"),
    );
    let page_one = controller.event(Some(state_of(&page_two)), "mcp_image_previous", obj(vec![]), workspace);
    keep(
        &mut checks,
        "previous page restores the original controls",
        buttons(&page_one, "previous page restores the original controls") == buttons(&paged, "previous page restores the original controls"),
    );
    let stale_label = "earlier result identity cannot save from a new result";
    let stale = controller.event(Some(state_of(&page_two)), &save_action(&d, 1, stale_label), obj(vec![]), workspace);
    keep(
        &mut checks,
        stale_label,
        notice_error(&stale) && directory_entries(&Path::new(workspace).join("service-files")).len() == 1,
    );

    let jpeg_label = "JPEG is saved exactly with its own detected format";
    let jpeg_saved = controller.event(Some(state_of(&page_two)), &save_action(&page_two, 9, jpeg_label), obj(vec![]), workspace);
    let jpeg_artifact = state_of(&jpeg_saved)
        .get("service_artifact")
        .unwrap_or_else(|| panic!("{jpeg_label}"));
    let jpeg_path = match jpeg_artifact.get("path") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("{jpeg_label}"),
    };
    let jpeg_bytes = std::fs::read(Path::new(workspace).join(&jpeg_path)).unwrap_or_else(|error| panic!("{jpeg_label}: {error}"));
    let jpeg_digest = sha256_bytes(&jpeg);
    keep(
        &mut checks,
        jpeg_label,
        jpeg_bytes == jpeg
            && matches!(jpeg_artifact.get("sha256"), Some(Value::Str(text)) if text == &jpeg_digest)
            && text_is(jpeg_artifact, "mime", "image/jpeg")
            && matches!(jpeg_artifact.get("name"), Some(Value::Str(text)) if text.ends_with(".jpg")),
    );

    let requests = peer.methods();
    let mut expected = Vec::new();
    for _ in 0..4 {
        expected.extend(["initialize", "notifications/initialized", "tools/call"].map(str::to_string));
    }
    keep(
        &mut checks,
        "all image actions were local; only reviewed calls hit the peer",
        requests == expected,
    );
    let count = requests.len();
    (checks, count)
}

fn call(
    controller: &mut Controller,
    peer: &Peer,
    state: &Value,
    result: &Value,
    workspace: &str,
    checks: &mut Vec<String>,
) -> Value {
    peer.set_result(result);
    let opened = controller.event(Some(state), "mcp_client_call", obj(vec![]), workspace);
    let reviewed = controller.event(
        Some(state_of(&opened)),
        "mcp_client_review",
        obj(vec![("tool".into(), s("image")), ("arguments".into(), s("{}"))]),
        workspace,
    );
    keep(checks, "review sends no request before approval", effect_is_null(&reviewed));
    let mut step = controller.event(Some(state_of(&reviewed)), "mcp_client_approve", obj(vec![]), workspace);
    for _ in 0..3 {
        step = exchange(controller, peer, &step, workspace);
    }
    step
}

fn exchange(controller: &mut Controller, peer: &Peer, step: &Value, workspace: &str) -> Value {
    let effect = require_network(step);
    let host = match effect.get("host") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("{}", notice_text(step)),
    };
    let port = match effect.get("port") {
        Some(Value::Num(text) | Value::Str(text)) => text.parse::<u16>().unwrap_or_else(|_| panic!("{}", notice_text(step))),
        _ => panic!("{}", notice_text(step)),
    };
    let wire = match effect.get("wire") {
        Some(Value::Str(text)) => text.clone(),
        _ => panic!("{}", notice_text(step)),
    };
    let purpose = effect.get("purpose").cloned().unwrap_or_else(|| panic!("{}", notice_text(step)));
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap_or_else(|error| panic!("connect MCP peer: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("timeout: {error}"));
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("timeout: {error}"));
    stream.write_all(wire.as_bytes()).unwrap_or_else(|error| panic!("write MCP request: {error}"));
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap_or_else(|error| panic!("read MCP response: {error}"));
    if let Some(error) = peer.take_error() {
        panic!("{error}");
    }
    if raw.is_empty() {
        panic!("empty MCP response");
    }
    controller.event(
        Some(state_of(step)),
        "network_result",
        obj(vec![("purpose".into(), purpose), ("data".into(), s(&b64_encode(&raw)))]),
        workspace,
    )
}

fn require_network(step: &Value) -> &Value {
    match step.get("effect") {
        Some(Value::Obj(_)) => {
            let effect = step.get("effect").unwrap_or_else(|| panic!("{}", notice_text(step)));
            match effect.get("kind") {
                Some(Value::Str(kind)) if kind == "network" => effect,
                _ => panic!("{}", notice_text(step)),
            }
        }
        _ => panic!("{}", notice_text(step)),
    }
}

fn notice_text(step: &Value) -> String {
    match state_of(step).get("notice") {
        Some(value) => value.to_string(),
        None => step.to_string(),
    }
}

fn buttons(envelope: &Value, label: &str) -> Vec<String> {
    let items = match envelope.get("ui").and_then(|ui| ui.get("modal")).and_then(|modal| modal.get("buttons")) {
        Some(Value::Arr(items)) => items,
        _ => panic!("{label}"),
    };
    items
        .iter()
        .map(|button| {
            let event = match button.get("event") {
                Some(Value::Str(text)) => text.as_str(),
                _ => panic!("{label}"),
            };
            if event.starts_with("mcp_image_save:") {
                event.rsplit_once(':').map(|(prefix, _)| prefix.to_string()).unwrap_or_else(|| event.to_string())
            } else {
                event.to_string()
            }
        })
        .collect()
}

fn save_action(envelope: &Value, index: usize, label: &str) -> String {
    let id = state_of(envelope)
        .get("mcp_image_result")
        .and_then(|result| result.get("id"))
        .unwrap_or_else(|| panic!("{label}"));
    let id_text = match id {
        Value::Str(text) | Value::Num(text) => text.as_str(),
        _ => panic!("{label}"),
    };
    format!("mcp_image_save:{index}:{id_text}")
}

fn raw_details(raw: &Value, shown: &Value, label: &str) -> bool {
    let output = match state_of(shown).get("output") {
        Some(value) => value,
        None => return false,
    };
    let raw_fields = fields_of(raw, label);
    let first = match raw_fields.first().and_then(|field| field.get("value")) {
        Some(value) => value == output,
        None => false,
    };
    let back = buttons(raw, label).iter().any(|event| event == "mcp_image_back");
    let distinct = fields_of(shown, label).iter().all(|field| match field.get("value") {
        Some(value) => value != output,
        None => panic!("{label}"),
    });
    first && back && distinct
}

fn fields_of<'a>(envelope: &'a Value, label: &str) -> &'a Vec<Value> {
    match envelope.get("ui").and_then(|ui| ui.get("modal")).and_then(|modal| modal.get("fields")) {
        Some(Value::Arr(items)) => items,
        _ => panic!("{label}"),
    }
}

fn output_equals(envelope: &Value, expected: &Value, label: &str) -> bool {
    let Some(Value::Str(text)) = state_of(envelope).get("output") else {
        return false;
    };
    match std::panic::catch_unwind(|| Value::parse(text)) {
        Ok(parsed) => parsed == *expected,
        Err(_) => panic!("{label}"),
    }
}

fn same_output(left: &Value, right: &Value) -> bool {
    match (state_of(left).get("output"), state_of(right).get("output")) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn artifact_listed(envelope: &Value, path: &Value) -> bool {
    match envelope.get("ui").and_then(|ui| ui.get("service_artifacts")) {
        Some(Value::Arr(items)) => items.iter().any(|item| item.get("path") == Some(path)),
        _ => false,
    }
}

fn effect_is_null(envelope: &Value) -> bool {
    matches!(envelope.get("effect"), Some(Value::Null))
}

fn notice_error(envelope: &Value) -> bool {
    state_of(envelope).get("notice_error").is_some_and(truthy)
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn state_of(envelope: &Value) -> &Value {
    envelope.get("state").unwrap_or_else(|| panic!("event returned no state: {}", envelope.to_string()))
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

fn set_phone_default(state: &mut Value, label: &str) {
    if state.get("drive").is_none() {
        put(state, "drive", obj(vec![]));
    }
    let Value::Obj(fields) = state else {
        panic!("{label}");
    };
    let drive = fields
        .iter_mut()
        .find(|(key, _)| key == "drive")
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("{label}"));
    let Value::Obj(drive_fields) = drive else {
        panic!("{label}");
    };
    if let Some(index) = drive_fields.iter().position(|(key, _)| key == "phone_default") {
        drive_fields[index].1 = twin::b(true);
    } else {
        drive_fields.push(("phone_default".into(), twin::b(true)));
    }
}

fn with_op(effect: &Value, op_name: &str) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("media effect is not an object");
    };
    let mut fields = fields.clone();
    if let Some(index) = fields.iter().position(|(name, _)| name == "op") {
        fields[index].1 = s(op_name);
    } else {
        fields.push(("op".into(), s(op_name)));
    }
    Value::Obj(fields)
}

fn keep(checks: &mut Vec<String>, label: &str, ok: bool) {
    if !ok {
        panic!("{label}");
    }
    checks.push(label.to_string());
}

fn directory_entries(dir: &Path) -> Vec<PathBuf> {
    let mut paths = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| entry.unwrap_or_else(|error| panic!("directory entry: {error}")).path())
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("directory {}: {error}", dir.display()),
    };
    paths.sort();
    paths
}

fn text_item() -> Value {
    obj(vec![("type".into(), s("text")), ("text".into(), s("Keep this real peer text."))])
}

fn image(data: &str, mime: &str) -> Value {
    obj(vec![
        ("type".into(), s("image")),
        ("data".into(), s(data)),
        ("mimeType".into(), s(mime)),
    ])
}

fn resource_link() -> Value {
    obj(vec![
        ("type".into(), s("resource_link")),
        ("name".into(), s("no fetch")),
        ("uri".into(), s("https://example.invalid/file")),
    ])
}

fn png_bytes() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&1u32.to_be_bytes());
    header.extend_from_slice(&1u32.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    bytes.extend(png_chunk(b"IHDR", &header));
    bytes.extend(png_chunk(b"IDAT", &zlib_compress(&[0, 0xff, 0, 0])));
    bytes.extend(png_chunk(b"IEND", &[]));
    bytes
}

fn png_chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

fn dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => {
            if *flag {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Num(token) => token.clone(),
        Value::Str(text) => quote_json(text),
        Value::Arr(items) => {
            let body = items.iter().map(dumps).collect::<Vec<_>>().join(", ");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let body = fields
                .iter()
                .map(|(key, item)| format!("{}: {}", quote_json(key), dumps(item)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{body}}}")
        }
    }
}

fn quote_json(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch if (ch as u32) <= 0x7f => out.push(ch),
            ch => {
                let code = ch as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let surrogate = code - 0x10000;
                    out.push_str(&format!("\\u{:04x}\\u{:04x}", 0xd800 + (surrogate >> 10), 0xdc00 + (surrogate & 0x3ff)));
                }
            }
        }
    }
    out.push('"');
    out
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Shared {
    requests: Mutex<Vec<String>>,
    result: Mutex<Value>,
    error: Mutex<Option<String>>,
    shutdown: AtomicBool,
}

struct Peer {
    port: u16,
    shared: Arc<Shared>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("peer port: {error}")).port();
        let shared = Arc::new(Shared {
            requests: Mutex::new(Vec::new()),
            result: Mutex::new(Value::Null),
            error: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });
        let thread_shared = Arc::clone(&shared);
        let handle = thread::spawn(move || serve(listener, thread_shared));
        Self {
            port,
            shared,
            handle: Some(handle),
        }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn set_result(&self, value: &Value) {
        *self.shared.result.lock().unwrap_or_else(|error| panic!("result lock: {error}")) = value.clone();
    }

    fn methods(&self) -> Vec<String> {
        self.shared.requests.lock().unwrap_or_else(|error| panic!("request lock: {error}")).clone()
    }

    fn take_error(&self) -> Option<String> {
        self.shared.error.lock().unwrap_or_else(|error| panic!("error lock: {error}")).take()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if shared.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let shared = Arc::clone(&shared);
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Err(error) = handle(&mut stream, &shared) {
                        record_error(&shared, error);
                    }
                }));
                if outcome.is_err() {
                    record_error(&shared, "peer panicked".to_string());
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
            Err(error) => {
                if shared.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                record_error(&shared, format!("accept: {error}"));
                break;
            }
        }
    }
}

fn record_error(shared: &Shared, error: String) {
    *shared.error.lock().unwrap_or_else(|poison| panic!("error lock: {poison}")) = Some(error);
}

fn handle(stream: &mut TcpStream, shared: &Shared) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let request = read_http(stream)?;
    if request.path != "/mcp" {
        return Err(format!("path {}", request.path));
    }
    if request.authorization.as_deref() != Some("Bearer image-test") {
        return Err(format!("authorization {:?}", request.authorization));
    }
    let body = Value::parse(&request.body);
    let method = body.get("method").map(Value::str_).ok_or("missing MCP method")?.to_string();
    shared
        .requests
        .lock()
        .map_err(|_| "request lock".to_string())?
        .push(method.clone());
    match method.as_str() {
        "initialize" => {
            let result = obj(vec![
                ("protocolVersion".into(), s("2025-11-25")),
                ("capabilities".into(), obj(vec![("tools".into(), obj(vec![]))])),
                (
                    "serverInfo".into(),
                    obj(vec![("name".into(), s("Image peer")), ("version".into(), s("1"))]),
                ),
            ]);
            let id = body.get("id").ok_or("missing id")?;
            write_http(stream, 200, "OK", &rpc(id, &result))
        }
        "notifications/initialized" => write_http(stream, 202, "Accepted", ""),
        "tools/call" => {
            let expected = obj(vec![("name".into(), s("image")), ("arguments".into(), obj(vec![]))]);
            if body.get("params") != Some(&expected) {
                return Err(format!(
                    "unexpected tools/call params {}",
                    body.get("params").map(Value::to_string).unwrap_or_default()
                ));
            }
            let result = shared.result.lock().map_err(|_| "result lock".to_string())?.clone();
            let id = body.get("id").ok_or("missing id")?;
            write_http(stream, 200, "OK", &rpc(id, &result))
        }
        other => Err(format!("unexpected MCP method {other}")),
    }
}

fn rpc(id: &Value, result: &Value) -> String {
    obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), id.clone()),
        ("result".into(), result.clone()),
    ])
    .to_string()
}

struct HttpRequest {
    path: String,
    body: String,
    authorization: Option<String>,
}

fn read_http(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("HTTP request exceeds 8 MiB".into());
        }
        let count = stream.read(&mut buffer).map_err(|error| format!("read request: {error}"))?;
        if count == 0 {
            return Err("incomplete HTTP request".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(header_end) = find_subslice(&bytes, b"\r\n\r\n") else {
            continue;
        };
        let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "request headers are not UTF-8".to_string())?;
        let mut lines = header.split("\r\n");
        let request_line = lines.next().ok_or("missing request line")?;
        let path = request_line.split_whitespace().nth(1).ok_or("missing request path")?.to_string();
        let mut length = None;
        let mut authorization = None;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let (name, value) = line.split_once(':').ok_or_else(|| format!("bad header {line}"))?;
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().map_err(|_| "bad content-length".to_string())?);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_string());
            }
        }
        let length = length.ok_or("missing content-length")?;
        let start = header_end + 4;
        if bytes.len() < start + length {
            continue;
        }
        if bytes.len() != start + length {
            return Err("trailing bytes after HTTP request".into());
        }
        let body = std::str::from_utf8(&bytes[start..]).map_err(|_| "request body is not UTF-8".to_string())?.to_string();
        return Ok(HttpRequest { path, body, authorization });
    }
}

fn write_http(stream: &mut TcpStream, status: u16, reason: &str, body: &str) -> Result<(), String> {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|_| stream.write_all(body.as_bytes()))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("write response: {error}"))?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

//! Local review and explicit adoption of typed MCP images.
use super::*;

const PAGE_SIZE: usize = 8;
const RESULT: &str = "mcp_image_result";
const IMAGES: &str = "mcp_images";
const RAW: &str = "mcp_images_raw";
const EVIDENCE: &str = "mcp_image_evidence";

fn content(result: &Json) -> &[Json] {
    get(result, "content").array().map(Vec::as_slice).unwrap_or(&[])
}

fn format(item: &Json) -> Option<(&'static str, &'static str, &'static [u8])> {
    if get(item, "type").text().ok()? != "image" {
        return None;
    }
    match get(item, "mimeType").text().ok()? {
        "image/png" => Some(("PNG", "png", b"\x89PNG\r\n\x1a\n")),
        "image/jpeg" => Some(("JPEG", "jpg", b"\xff\xd8\xff")),
        _ => None,
    }
}

fn has_images(result: &Json) -> bool {
    !boolean(result, "isError") && content(result).iter().any(|item| format(item).is_some())
}

fn open(st: &mut Json, digest: String, retained: bool, savable: bool) {
    let selection = obj(vec![
        ("id", s(&now())),
        ("sha256", s(&digest)),
        ("root", get(st, "approval_root_identity").clone()),
        ("chat", get(st, "active").clone()),
        ("retained", Bool(retained)),
        ("savable", Bool(savable)),
    ]);
    put(st, RESULT, selection);
    put(st, "dialog", s(if savable { IMAGES } else { RAW }));
}

pub(super) fn present(st: &mut Json, result: &Json) {
    let output = result.string();
    if has_images(result) {
        open(st, harness_rules::sha256(output.as_bytes()), false, true);
    } else {
        put(st, RESULT, Null);
        put(st, "dialog", s("output"));
    }
    put(st, "output", s(&output));
}

fn conversation(st: &Json) -> Option<usize> {
    let active = get(st, "active");
    get(st, "chats").array().ok()?.iter().position(|chat| get(chat, "id") == active)
}

fn retained_result(st: &Json) -> Option<&str> {
    let index = conversation(st)?;
    let entry = get(&get(st, "chats").array().ok()?[index], EVIDENCE);
    let root = field(entry, "root").ok()?;
    if root.is_empty() || field(st, "approval_root_identity").ok()? != root {
        return None;
    }
    field(entry, "output").ok()
}

pub(super) fn review_button(st: &Json) -> Option<Json> {
    retained_result(st).map(|_| button("Review MCP images", "mcp_image_review"))
}

pub(super) fn retain(st: &mut Json, raw: String) -> bool {
    let Some(index) = conversation(st) else { return false };
    if text(st, "approval_root_identity").is_empty() {
        return false;
    }
    let record = obj(vec![
        ("root", get(st, "approval_root_identity").clone()),
        ("output", Str(raw)),
    ]);
    let Ok(chats) = get(st, "chats").array() else { return false };
    let old = get(&chats[index], EVIDENCE);
    let size = super::super::mcp_schema::state_bytes(st)
        .saturating_sub(super::super::mcp_schema::state_bytes(old))
        .saturating_add(super::super::mcp_schema::state_bytes(&record) + 64);
    if size > 6 * 1024 * 1024 - 8192 {
        return false;
    }
    let policy = super::super::resources::policy(get(st, "device"), get(st, "resources"), size);
    if number(&policy, "response_bytes") < 65536 {
        return false;
    }
    if let Obj(fields) = st {
        if let Some(Arr(chats)) = fields.get_mut("chats") {
            put(&mut chats[index], EVIDENCE, record);
            return true;
        }
    }
    false
}

pub(super) fn continue_model(st: &mut Json, result: &Json, local: &mut Option<String>) -> Result<Json> {
    let items = content(result);
    let is_image = |item: &Json| get(item, "type").text().ok() == Some("image");
    if !items.iter().any(is_image) {
        return completed_tool(st, &result.string());
    }

    // The text continuation carries the exact non-image content and an image
    // descriptor. Media files are created only by the explicit save action.
    let Obj(fields) = result else { return Err("Invalid MCP result object".into()); };
    let mut output = Obj(fields.iter().filter(|(key, _)| key.as_str() != "content").map(|(key, value)| (key.clone(), value.clone())).collect());
    let items = items.iter().map(|item| {
        if is_image(item) {
            obj(vec![("type", s("image")), ("mimeType", get(item, "mimeType").clone()), ("dataOmitted", Bool(true))])
        } else {
            item.clone()
        }
    }).collect();
    put(&mut output, "content", Arr(items));
    *local = Some(result.string());
    completed_tool(st, &output.string())
}

fn output<'a>(st: &'a Json, saved: &Json) -> Result<&'a str> {
    if boolean(saved, "retained") {
        retained_result(st).ok_or_else(|| "The conversation's MCP result is unavailable in this workspace".into())
    } else {
        field(st, "output")
    }
}

fn selection(st: &Json) -> Result<&Json> {
    let saved = get(st, RESULT);
    if !matches!(get(st, "dialog").text().ok(), Some(IMAGES | RAW)) || saved == &Null
        || get(saved, "root") != get(st, "approval_root_identity")
        || get(saved, "chat") != get(st, "active")
    {
        return Err("This MCP image result is no longer open in its original workspace and conversation".into());
    }
    Ok(saved)
}

pub(super) fn dialog(st: &Json) -> Option<Json> {
    let saved = selection(st).ok()?;
    let output = output(st, saved).ok()?;
    if get(st, "dialog").text().ok() == Some(RAW) {
        return Some(modal("Raw MCP result", vec![field_ui("output", "Output", output, "readonly")],
            if boolean(saved, "savable") || !boolean(saved, "retained") {
                vec![button("Back to images", "mcp_image_back")]
            } else { vec![] }));
    }
    let result = parse(output).ok()?;
    let id = field(saved, "id").ok()?;
    let offset = number(saved, "offset");
    let mut images = content(&result).iter().enumerate().filter_map(|(index, item)| {
        format(item).map(|(label, _, _)| (index, label))
    }).skip(offset);
    let mut buttons: Vec<_> = images.by_ref().take(PAGE_SIZE).map(|(index, label)| {
        button(&format!("Save image {} ({label})", index + 1), &format!("mcp_image_save:{index}:{id}"))
    }).collect();
    if offset > 0 {
        buttons.push(button("Previous images", "mcp_image_previous"));
    }
    if images.next().is_some() {
        buttons.push(button("More images", "mcp_image_next"));
    }
    if get(saved, "path") != &Null && get(saved, "path") == get(get(st, "service_artifact"), "path") {
        buttons.push(button("Preview saved image", "media_preview"));
        buttons.push(button("Export saved image", "media_export"));
    }
    buttons.push(button("View raw result", "mcp_image_raw"));
    Some(modal("MCP result", vec![field_ui("images", "Returned images",
        "Choose an image to save it in this workspace. The raw result includes the original text and image data.", "readonly")], buttons))
}

pub(super) fn handles(action: &str) -> bool {
    matches!(action, "mcp_image_review" | "mcp_image_next" | "mcp_image_previous" | "mcp_image_raw" | "mcp_image_back") || action.starts_with("mcp_image_save:")
}

pub(super) fn event(st: &mut Json, action: &str) -> Result<()> {
    if boolean(st, "busy") || get(st, "pending") != &Null {
        return Err("Finish or cancel the current operation first".into());
    }
    if action == "mcp_image_review" {
        let raw = retained_result(st).ok_or("No MCP image result is available in this conversation and workspace")?;
        let savable = has_images(&parse(raw)?);
        let digest = harness_rules::sha256(raw.as_bytes());
        open(st, digest, true, savable);
        return Ok(());
    }
    let mut saved = selection(st)?.clone();
    let output = output(st, &saved)?;
    if get(&saved, "sha256").text()? != harness_rules::sha256(output.as_bytes()) {
        return Err("The MCP result changed. Review a new result before saving".into());
    }
    let viewing_raw = get(st, "dialog").text().ok() == Some(RAW);
    match (viewing_raw, action) {
        (true, "mcp_image_back") if boolean(&saved, "savable") || !boolean(&saved, "retained") => {
            put(st, "dialog", s(IMAGES));
            return Ok(());
        }
        (false, "mcp_image_raw") => {
            put(st, "dialog", s(RAW));
            return Ok(());
        }
        (true, _) | (false, "mcp_image_back") => return Err("Choose an action from the current MCP result view".into()),
        _ => {}
    }
    let result = parse(output)?;
    if boolean(&result, "isError") {
        return Err("Images cannot be saved from a tool error".into());
    }
    if let Some(target) = action.strip_prefix("mcp_image_save:") {
        let (index, id) = target.split_once(':').ok_or("Invalid MCP image selection")?;
        if field(&saved, "id")? != id {
            return Err("This selection belongs to an earlier MCP result".into());
        }
        let index = index.parse::<usize>().map_err(|_| "Invalid MCP image selection")?;
        let item = content(&result).get(index).ok_or("MCP image selection is absent")?;
        let (_, extension, signature) = format(item).ok_or("Choose a typed PNG or JPEG image")?;
        let encoded = field(item, "data")?;
        let limit = crate::attachments::MAX_IMAGE_BYTES;
        if encoded.len() > limit.div_ceil(3) * 4 {
            return Err("MCP image exceeds 2 MiB".into());
        }
        let bytes = network::unbase64(encoded)?;
        if bytes.len() > limit || network::b64(&bytes) != encoded {
            return Err("MCP image size or canonical base64 is invalid".into());
        }
        if !bytes.starts_with(signature) {
            return Err("MCP image bytes do not match the declared MIME type".into());
        }
        let digest = harness_rules::sha256(&bytes);
        let current = chat(st);
        let record = array(&current, "service_artifacts").into_iter().find(|record| {
            get(record, "route").text().ok() == Some("mcp")
                && get(record, "sha256").text().ok() == Some(digest.as_str())
                && get(record, "root") == get(st, "approval_root_identity")
        });
        let record = match record {
            Some(record) => {
                media::verify(st, &record, limit)?;
                record
            }
            None => {
                if current == Null && get(st, "chats").array().is_ok_and(|chats| chats.len() >= 200) {
                    return Err("History contains 200 conversations. Export a backup before starting more.".into());
                }
                if super::super::mcp_schema::state_bytes(st) > 6 * 1024 * 1024 - 8192 {
                    return Err("History has insufficient space to retain this MCP image".into());
                }
                let record = media::store(st, &bytes, &format!("mcp-image-{}.{extension}", index + 1), "image", "mcp")?;
                media::retain_artifact(st, &record)?;
                record
            }
        };
        put(&mut saved, "path", get(&record, "path").clone());
        put(&mut saved, "chat", get(st, "active").clone());
        put(st, "service_artifact", record);
        put(st, "notice", s("MCP image saved to this workspace"));
    } else {
        let offset = number(&saved, "offset");
        let next = if action == "mcp_image_next" { offset.saturating_add(PAGE_SIZE) } else { offset.saturating_sub(PAGE_SIZE) };
        if next >= content(&result).iter().filter(|item| format(item).is_some()).count() {
            return Err("No more MCP images on that page".into());
        }
        put(&mut saved, "offset", Num(next.to_string()));
    }
    put(st, RESULT, saved);
    Ok(())
}

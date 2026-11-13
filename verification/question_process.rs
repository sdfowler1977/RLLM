//! Native twin of the manual question/design workflow checks.
mod twin;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Native manual controls and real guarded file worker; no model prose validation";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = Scratch::create(&project);
    let root = scratch.path().to_string();
    let mut controller = Controller::spawn(&project_text);
    let passed = run(&mut controller, &root);
    controller.finish();
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    // Python writes json.dumps(..., indent=2) with no extra newline.
    twin::report_value("question_process", &body, false);
    println!("{} manual question-process checks passed", passed.len());
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let mut current = controller.event(None, "resume", empty(), root);
    current = controller.event(Some(inner(&current)), "inquiry_new", empty(), root);
    current = controller.event(
        Some(inner(&current)),
        "inquiry_start",
        inputs(&[("question", "Design an original file index from observed requirements.")]),
        root,
    );
    keep(
        &mut passed,
        "manual question without model or external MCP server",
        is_null(current.get("effect")),
    );

    current = edit(
        controller,
        &current,
        "inquiry_plan",
        "inquiry_plan_save",
        inputs(&[
            ("questions", "Which names must be indexed?"),
            ("constraints", "Original implementation"),
            ("requirements", "Preserve exact filenames"),
        ]),
        root,
        "manual plan, unresolved finding and original design",
    );
    current = edit(
        controller,
        &current,
        "inquiry_note",
        "inquiry_note_save",
        inputs(&[
            ("kind", "unknown"),
            ("text", "Filename encoding needs observation."),
            ("citations", ""),
            ("resolves", ""),
        ]),
        root,
        "manual plan, unresolved finding and original design",
    );
    current = edit(
        controller,
        &current,
        "inquiry_note",
        "inquiry_note_save",
        inputs(&[
            ("kind", "design"),
            ("text", "Use original code to preserve observed filename bytes."),
            ("citations", ""),
            ("resolves", ""),
        ]),
        root,
        "manual plan, unresolved finding and original design",
    );
    keep(&mut passed, "manual plan, unresolved finding and original design", true);

    current = edit(
        controller,
        &current,
        "inquiry_assessment",
        "inquiry_assessment_save",
        inputs(&[
            ("item", "questions"),
            ("index", "1"),
            ("status", "partial"),
            ("explanation", "The requirement is recorded; filename observations remain missing."),
            ("citations", ""),
            ("checks", ""),
        ]),
        root,
        "partial item assessment retains missing evidence",
    );
    let record = last_item(&current, "partial item assessment retains missing evidence");
    let status_ok = record
        .get("assessments")
        .and_then(|items| items.arr().last())
        .and_then(|item| item.get("status"))
        .map(Value::str_)
        == Some("partial");
    // The leg reads `old_plan=c['plan']`, so a record without a plan is a failure. A null stand-in
    // here would let a note carrying `"plan": null` satisfy the prior-plan comparison below.
    let old_plan = record.get("plan").cloned().unwrap_or_else(|| {
        panic!("partial item assessment retains missing evidence: the last inquiry record carries no plan")
    });
    keep(&mut passed, "partial item assessment retains missing evidence", status_ok);

    current = edit(
        controller,
        &current,
        "inquiry_plan",
        "inquiry_plan_save",
        inputs(&[
            ("questions", "Which names and encodings must be indexed?"),
            ("constraints", "Original implementation"),
            ("requirements", "Preserve exact filenames"),
        ]),
        root,
        "plan revision preserves previous assessment and exact prior plan",
    );
    let record = last_item(&current, "plan revision preserves previous assessment and exact prior plan");
    let assessment_count = record.get("assessments").map(|items| items.arr().len()).unwrap_or(0);
    let prior_plan = record.get("notes").is_some_and(|notes| {
        notes.arr().iter().any(|note| note.get("plan") == Some(&old_plan))
    });
    keep(
        &mut passed,
        "plan revision preserves previous assessment and exact prior plan",
        assessment_count == 1 && prior_plan,
    );

    current = controller.event(Some(inner(&current)), "inquiry_section:review", empty(), root);
    let view = current.get("ui").map(Value::to_string).unwrap_or_default();
    keep(
        &mut passed,
        "revised plan does not inherit answered or partial status",
        view.contains("unreviewed"),
    );

    current = controller.event(Some(inner(&current)), "inquiry_export", empty(), root);
    current = controller.event(
        Some(inner(&current)),
        "inquiry_export_review",
        inputs(&[("output", "question.json")]),
        root,
    );
    let export_check = "reviewed export writes actual journal, unresolved state and original design";
    if !dialog_is(&current, "approval") {
        panic!("{export_check}");
    }
    current = controller.event(Some(inner(&current)), "approve", empty(), root);
    let effect = effect_of(&current, export_check);
    if !(text_is(&effect, "kind", "tool") && text_is(&effect, "tool", "write")) {
        panic!("{export_check}");
    }
    let reply = invoke_tool(controller, &effect, root);
    let output = reply.get("value").cloned().unwrap_or_else(|| panic!("{export_check}"));
    let exported = read_json(&file(root, "question.json"), export_check);
    let data = exported.get("data").unwrap_or_else(|| panic!("{export_check}"));
    let item_review = data.get("item_review").unwrap_or_else(|| panic!("{export_check}"));
    let evidence_empty = matches!(data.get("evidence"), Some(Value::Arr(items)) if items.is_empty());
    keep(
        &mut passed,
        export_check,
        exported.get("type").map(Value::str_) == Some("question_process")
            && data
                .get("question")
                .map(Value::str_)
                .is_some_and(|text| text.starts_with("Design an original"))
            && number_is(item_review, "unreviewed", 2)
            && number_is(item_review, "authored_answered", 0)
            && evidence_empty,
    );
    current = controller.event(
        Some(inner(&current)),
        "tool_result",
        tool_result_inputs(&effect, output),
        root,
    );

    current = controller.event(Some(inner(&current)), "inquiry_export", empty(), root);
    current = controller.event(
        Some(inner(&current)),
        "inquiry_export_review",
        inputs(&[("output", "cancelled.json")]),
        root,
    );
    current = controller.event(Some(inner(&current)), "approve", empty(), root);
    let effect = effect_of(&current, "cancelled export cannot execute a stale approved write");
    current = controller.event(Some(inner(&current)), "cancel", empty(), root);
    let refused = invoke_tool(controller, &effect, root);
    keep(
        &mut passed,
        "cancelled export cannot execute a stale approved write",
        refused.get("error").is_some() && !file(root, "cancelled.json").exists(),
    );

    current = controller.event(Some(inner(&current)), "inquiry_export", empty(), root);
    current = controller.event(
        Some(inner(&current)),
        "inquiry_export_review",
        inputs(&[("output", "design-brief.md"), ("format", "design_brief")]),
        root,
    );
    let pending_format = inner(&current)
        .get("pending")
        .and_then(|pending| pending.get("inquiry_export"))
        .and_then(|export| export.get("format"))
        .map(Value::str_);
    keep(
        &mut passed,
        "design brief has explicit contents and ordinary write review",
        dialog_is(&current, "approval")
            && !file(root, "design-brief.md").exists()
            && pending_format == Some("design_brief"),
    );
    current = controller.event(Some(inner(&current)), "approve", empty(), root);
    let effect = effect_of(&current, "changed export format cannot borrow an approved write");
    let altered = with_export_format(&effect, "journal");
    let refused = invoke_tool(controller, &altered, root);
    keep(
        &mut passed,
        "changed export format cannot borrow an approved write",
        refused.get("error").is_some() && !file(root, "design-brief.md").exists(),
    );

    let brief_check = "actual no-model design brief preserves authored design and unresolved work";
    let reply = invoke_tool(controller, &effect, root);
    let output = reply.get("value").cloned().unwrap_or_else(|| panic!("{brief_check}"));
    let brief = std::fs::read_to_string(file(root, "design-brief.md")).unwrap_or_else(|_| panic!("{brief_check}"));
    keep(
        &mut passed,
        brief_check,
        brief.starts_with("# Design brief")
            && brief.contains("Use original code to preserve observed filename bytes.")
            && brief.contains("## Returned-evidence references")
            && brief.contains("Unknown entries without an authored resolution: 1."),
    );
    current = controller.event(
        Some(inner(&current)),
        "tool_result",
        tool_result_inputs(&effect, output),
        root,
    );

    current = controller.event(Some(inner(&current)), "inquiry_export", empty(), root);
    let bad = controller.event(
        Some(inner(&current)),
        "inquiry_export_review",
        inputs(&[("output", "bad-format.md"), ("format", "unknown")]),
        root,
    );
    keep(
        &mut passed,
        "unknown brief mode refuses only that export",
        is_null(bad.get("effect"))
            && inner(&bad).get("notice_error").is_some_and(twin::truthy)
            && !file(root, "bad-format.md").exists(),
    );

    current = controller.event(Some(inner(&bad)), "inquiry_export", empty(), root);
    current = controller.event(
        Some(inner(&current)),
        "inquiry_export_review",
        inputs(&[("output", "cancelled-brief.md"), ("format", "design_brief")]),
        root,
    );
    current = controller.event(Some(inner(&current)), "approve", empty(), root);
    let effect = effect_of(&current, "cancelled design brief cannot execute a stale approved write");
    let cancelled = controller.event(Some(inner(&current)), "cancel", empty(), root);
    let refused = invoke_tool(controller, &effect, root);
    keep(
        &mut passed,
        "cancelled design brief cannot execute a stale approved write",
        refused.get("error").is_some() && !file(root, "cancelled-brief.md").exists(),
    );
    current = cancelled;

    current = controller.event(Some(inner(&current)), "inquiry_new", empty(), root);
    current = controller.event(
        Some(inner(&current)),
        "inquiry_start",
        inputs(&[("question", "Question A to retain")]),
        root,
    );
    let a = selection(&current, "deletion review displays its exact question and captures its identity");
    current = controller.event(Some(inner(&current)), "inquiry_delete", empty(), root);
    let old_a = confirmation(current.get("ui").unwrap_or(&Value::Null));
    let shown = current.get("ui").map(Value::to_string).unwrap_or_default();
    keep(
        &mut passed,
        "deletion review displays its exact question and captures its identity",
        old_a.is_some() && shown.contains("Question A to retain"),
    );
    let old_a = old_a.unwrap_or_else(|| panic!("deletion review displays its exact question and captures its identity"));

    current = controller.event(Some(inner(&current)), "inquiry_new", empty(), root);
    current = controller.event(
        Some(inner(&current)),
        "inquiry_start",
        inputs(&[("question", "Question B to review")]),
        root,
    );
    let b = selection(&current, "stale Question A confirmation cannot delete Question B");
    current = controller.event(Some(inner(&current)), "inquiry_delete", empty(), root);
    let fresh_b = confirmation(current.get("ui").unwrap_or(&Value::Null))
        .unwrap_or_else(|| panic!("stale Question A confirmation cannot delete Question B"));
    let before = records(&current, "stale Question A confirmation cannot delete Question B").clone();
    let stale = controller.event(Some(inner(&current)), &old_a, empty(), root);
    keep(
        &mut passed,
        "stale Question A confirmation cannot delete Question B",
        records(&stale, "stale Question A confirmation cannot delete Question B") == &before
            && notice_has(&stale, "no journal was deleted"),
    );

    let legacy = controller.event(Some(inner(&stale)), "inquiry_delete_confirm", empty(), root);
    keep(
        &mut passed,
        "legacy unbound deletion callback cannot delete a journal",
        records(&legacy, "legacy unbound deletion callback cannot delete a journal") == &before,
    );

    let kept = controller.event(Some(inner(&legacy)), "inquiry", empty(), root);
    let late = controller.event(Some(inner(&kept)), &fresh_b, empty(), root);
    keep(
        &mut passed,
        "leaving deletion review prevents its captured callback from deleting",
        records(&late, "leaving deletion review prevents its captured callback from deleting") == &before,
    );

    current = controller.event(Some(inner(&late)), &format!("inquiry_select:{b}"), empty(), root);
    current = controller.event(Some(inner(&current)), "inquiry_delete", empty(), root);
    let old_b = confirmation(current.get("ui").unwrap_or(&Value::Null))
        .unwrap_or_else(|| panic!("changed journal requires a fresh exact-content deletion confirmation"));
    current = edit(
        controller,
        &current,
        "inquiry_plan",
        "inquiry_plan_save",
        inputs(&[("questions", "A changed question plan"), ("constraints", ""), ("requirements", "")]),
        root,
        "changed journal requires a fresh exact-content deletion confirmation",
    );
    current = controller.event(Some(inner(&current)), "inquiry_delete", empty(), root);
    let fresh_b = confirmation(current.get("ui").unwrap_or(&Value::Null))
        .unwrap_or_else(|| panic!("changed journal requires a fresh exact-content deletion confirmation"));
    let before = records(&current, "changed journal requires a fresh exact-content deletion confirmation").clone();
    let changed = controller.event(Some(inner(&current)), &old_b, empty(), root);
    keep(
        &mut passed,
        "changed journal requires a fresh exact-content deletion confirmation",
        records(&changed, "changed journal requires a fresh exact-content deletion confirmation") == &before && fresh_b != old_b,
    );

    let delete_check = "fresh confirmation deletes only its reviewed journal and preserves chat and exported files";
    let chats = chats_without_inquiry(&changed, delete_check);
    current = controller.event(Some(inner(&changed)), &fresh_b, empty(), root);
    let ids = item_ids(&current, delete_check);
    let remaining = chats_without_inquiry(&current, delete_check);
    let no_b = inner(&current).get("chats").is_some_and(|chats| {
        chats.arr().iter().all(|chat| match chat.get("inquiry_id") {
            Some(Value::Str(id)) => id != &b,
            Some(_) => true,
            None => true,
        })
    });
    keep(
        &mut passed,
        delete_check,
        ids.iter().any(|id| id == &a)
            && !ids.iter().any(|id| id == &b)
            && remaining == chats
            && no_b
            && file(root, "question.json").exists(),
    );

    let repeated = controller.event(Some(inner(&current)), &fresh_b, empty(), root);
    keep(
        &mut passed,
        "completed deletion cannot replay against another question",
        records(&repeated, "completed deletion cannot replay against another question")
            == records(&current, "completed deletion cannot replay against another question"),
    );
    passed
}

fn edit(
    controller: &mut Controller,
    current: &Value,
    open_action: &str,
    save_action: &str,
    inputs: Value,
    root: &str,
    check: &str,
) -> Value {
    let opened = controller.event(Some(inner(current)), open_action, empty(), root);
    let saved = controller.event(Some(inner(&opened)), save_action, inputs, root);
    if !dialog_is(&saved, "inquiry_view") {
        panic!("{check}");
    }
    saved
}

fn empty() -> Value {
    Value::Obj(vec![])
}

fn inputs(fields: &[(&str, &str)]) -> Value {
    Value::Obj(fields.iter().map(|(key, value)| ((*key).to_string(), s(value))).collect())
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn inner(event: &Value) -> &Value {
    event.get("state").unwrap_or_else(|| panic!("event returned no state: {}", event.to_string()))
}

fn is_null(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Null))
}

fn dialog_is(event: &Value, expect: &str) -> bool {
    text_is(inner(event), "dialog", expect)
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn number_is(value: &Value, key: &str, expect: i64) -> bool {
    match value.get(key) {
        Some(Value::Num(text)) => text.parse::<i64>().ok() == Some(expect),
        _ => false,
    }
}

fn notice_has(event: &Value, text: &str) -> bool {
    inner(event).get("notice").map(Value::str_).is_some_and(|notice| notice.contains(text))
}

fn last_item<'a>(event: &'a Value, check: &str) -> &'a Value {
    records(event, check)
        .get("items")
        .and_then(|items| items.arr().last())
        .unwrap_or_else(|| panic!("{check}"))
}

fn records<'a>(event: &'a Value, check: &str) -> &'a Value {
    inner(event).get("inquiry_records").unwrap_or_else(|| panic!("{check}"))
}

fn selection(event: &Value, check: &str) -> String {
    match inner(event).get("inquiry_selection") {
        Some(Value::Str(id)) => id.clone(),
        _ => panic!("{check}"),
    }
}

fn item_ids(event: &Value, check: &str) -> Vec<String> {
    records(event, check)
        .get("items")
        .unwrap_or_else(|| panic!("{check}"))
        .arr()
        .iter()
        .map(|item| match item.get("id") {
            Some(Value::Str(id)) => id.clone(),
            _ => panic!("{check}"),
        })
        .collect()
}

fn chats_without_inquiry(event: &Value, check: &str) -> Vec<Value> {
    let chats = inner(event).get("chats").unwrap_or_else(|| panic!("{check}"));
    chats
        .arr()
        .iter()
        .map(|chat| {
            let Value::Obj(fields) = chat else {
                panic!("{check}");
            };
            Value::Obj(fields.iter().filter(|(key, _)| key != "inquiry_id").cloned().collect())
        })
        .collect()
}

fn confirmation(value: &Value) -> Option<String> {
    match value {
        Value::Obj(fields) => {
            if let Some((_, event)) = fields.iter().find(|(key, _)| key == "event") {
                if let Value::Str(text) = event {
                    if text.starts_with("inquiry_delete_confirm:") {
                        return Some(text.clone());
                    }
                }
            }
            fields.iter().find_map(|(_, child)| confirmation(child))
        }
        Value::Arr(items) => items.iter().find_map(confirmation),
        _ => None,
    }
}

fn effect_of(event: &Value, check: &str) -> Value {
    match event.get("effect") {
        Some(Value::Obj(_)) => event.get("effect").unwrap().clone(),
        _ => panic!("{check}"),
    }
}

fn invoke_tool(controller: &mut Controller, effect: &Value, root: &str) -> Value {
    controller.send(&tool_envelope(effect, root))
}

fn tool_envelope(effect: &Value, root: &str) -> Value {
    let mut envelope = effect.clone();
    set_field(&mut envelope, "op", s("tool"));
    set_field(&mut envelope, "root", s(root));
    envelope
}

fn tool_result_inputs(effect: &Value, output: Value) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("effect is not an object");
    };
    let mut inputs = Vec::new();
    for (key, value) in fields {
        if matches!(key.as_str(), "purpose" | "tool_ticket" | "tool_binding") {
            inputs.push((key.clone(), value.clone()));
        }
    }
    inputs.push(("output".into(), output));
    Value::Obj(inputs)
}

fn with_export_format(effect: &Value, format: &str) -> Value {
    let mut altered = effect.clone();
    let export = field_mut(&mut altered, "inquiry_export");
    set_field(export, "format", s(format));
    altered
}

fn set_field(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    panic!("missing {key}");
}

fn file(root: &str, name: &str) -> std::path::PathBuf {
    std::path::Path::new(root).join(name)
}

fn read_json(path: &std::path::Path, check: &str) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("{check}"));
    Value::parse(&text)
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path) -> Self {
        Self(twin::create_scratch(root, "checks-"))
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

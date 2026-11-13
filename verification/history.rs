//! Isolated shared-core history state checks; no real conversation data.
mod twin;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Isolated native event state transitions; no Android encrypted persistence or real data deletion";
const REPORT: &str = "output/checks/history-results.json";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = Scratch::create(&root);
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, scratch.path());
    controller.finish();
    twin::report("history", &passed, SCOPE);
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains(SCOPE) {
        panic!("{REPORT} missing scope");
    }
}

fn run(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let mut st = state(&resumed).clone();
    put(&mut st, "dialog", Value::Null);
    put(
        &mut st,
        "trash",
        Value::Arr(vec![
            chat("deleted-one", "Fixture one", vec![message("user", "First")]),
            chat("deleted-two", "Fixture two", vec![]),
        ]),
    );

    let refused = controller.event(Some(&st), "trash_empty", obj(vec![]), root);
    keep(
        &mut passed,
        "empty Trash requires current review",
        same(state(&refused), "trash", field(&st, "trash")),
    );

    let opened = controller.event(Some(&st), "trash", obj(vec![]), root);
    let reviewed_event = controller.event(Some(state(&opened)), "trash_empty_review", obj(vec![]), root);
    let reviewed = state(&reviewed_event).clone();
    let review_len = reviewed.get("trash_review").map(|value| value.arr().len()).unwrap_or(0);
    if !text_is(&reviewed, "dialog", "trash_empty_review") || review_len != 2 {
        panic!("trash review did not list both conversations: {}", reviewed.to_string());
    }
    let kept = controller.event(Some(&reviewed), "trash", obj(vec![]), root);
    keep(
        &mut passed,
        "keeping reviewed conversations preserves all items",
        same(state(&kept), "trash", field(&st, "trash")),
    );

    let mut changed = reviewed.clone();
    {
        let first = &mut items_mut(field_mut(&mut changed, "trash"))[0];
        let message = &mut items_mut(field_mut(first, "messages"))[0];
        put(message, "content", s("Changed after review"));
    }
    let refused = controller.event(Some(&changed), "trash_empty", obj(vec![]), root);
    let refused_state = state(&refused);
    let notice = refused_state.get("notice").map(Value::str_).unwrap_or("");
    keep(
        &mut passed,
        "changed message invalidates exact reviewed snapshot",
        same(refused_state, "trash", field(&changed, "trash")) && notice.contains("changed after review"),
    );

    let mut added = reviewed.clone();
    items_mut(field_mut(&mut added, "trash")).push(chat("deleted-three", "New fixture", vec![]));
    let refused = controller.event(Some(&added), "trash_empty", obj(vec![]), root);
    keep(
        &mut passed,
        "newly trashed conversation invalidates old confirmation",
        same(state(&refused), "trash", field(&added, "trash")),
    );

    let mut ambiguous = st.clone();
    put(&mut items_mut(field_mut(&mut ambiguous, "trash"))[1], "id", s("deleted-one"));
    let opened = controller.event(Some(&ambiguous), "trash", obj(vec![]), root);
    let refused = controller.event(Some(state(&opened)), "trash_empty_review", obj(vec![]), root);
    let refused_state = state(&refused);
    keep(
        &mut passed,
        "duplicate identities cannot be purged as a snapshot",
        same(refused_state, "trash", field(&ambiguous, "trash")) && !text_is(refused_state, "dialog", "trash_empty_review"),
    );

    let emptied = controller.event(Some(&reviewed), "trash_empty", obj(vec![]), root);
    let emptied_state = state(&emptied);
    let trash_cleared = matches!(emptied_state.get("trash"), Some(Value::Arr(items)) if items.is_empty());
    keep(
        &mut passed,
        "confirmed exact Trash purge preserves active history",
        trash_cleared && same(emptied_state, "chats", field(&st, "chats")) && same(emptied_state, "active", field(&st, "active")),
    );
    passed
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn same(value: &Value, key: &str, expect: &Value) -> bool {
    value.get(key).is_some_and(|got| got == expect)
}

fn text_is(value: &Value, key: &str, expect: &str) -> bool {
    matches!(value.get(key), Some(Value::Str(text)) if text == expect)
}

fn put(value: &mut Value, key: &str, next: Value) {
    *field_mut(value, key) = next;
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        return &mut fields[index].1;
    }
    fields.push((key.to_string(), Value::Null));
    &mut fields.last_mut().unwrap().1
}

fn items_mut(value: &mut Value) -> &mut Vec<Value> {
    let Value::Arr(items) = value else {
        panic!("expected array");
    };
    items
}

fn message(role: &str, content: &str) -> Value {
    obj(vec![("role".into(), s(role)), ("content".into(), s(content))])
}

fn chat(id: &str, title: &str, messages: Vec<Value>) -> Value {
    obj(vec![
        ("id".into(), s(id)),
        ("title".into(), s(title)),
        ("messages".into(), Value::Arr(messages)),
    ])
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = root.join("output").join(format!("history-checks-{}-{nanos}", std::process::id()));
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

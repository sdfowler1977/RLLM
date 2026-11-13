//! Connected owner checks: actual local HTTP and native shared-core gateway events.
mod twin;

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use twin::{b, n, obj, s, Controller, Value};

fn main() {
    if sha256(b"") != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        || sha256(b"abc") != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    {
        panic!("sha256");
    }
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let workspace = scratch.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_else(|error| panic!("workspace: {error}"));
    std::fs::write(workspace.join("one.txt"), "Actual original observation.\n").unwrap();
    std::fs::write(workspace.join("two.txt"), "Different actual observation.\n").unwrap();
    let outside = scratch.path().join("outside.txt");
    std::fs::write(&outside, "OUTSIDE PRIVATE DATA").unwrap();
    std::os::unix::fs::symlink(&outside, workspace.join("escape")).unwrap_or_else(|error| panic!("symlink: {error}"));
    let token = format!("local-test-secret-{}", "z".repeat(40));
    let secret = scratch.path().join("token");
    std::fs::write(&secret, format!("{token}\n")).unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    let workspace_text = workspace.to_str().unwrap_or_else(|| panic!("workspace path is not UTF-8")).to_string();
    let canonical = workspace.canonicalize().unwrap_or_else(|error| panic!("workspace: {error}"));
    let canonical_text = canonical.to_str().unwrap_or_else(|| panic!("workspace path is not UTF-8")).to_string();

    let mut controller = Controller::spawn(&project);
    let launch = controller.event(None, "launch", obj(vec![]), &workspace_text);
    let launch_state = state(&launch).clone();
    let started = controller.event(Some(&launch_state), "gateway_start", gateway_inputs(&token, "127.0.0.1", false), &workspace_text);
    let gateway_state = state(&started).clone();
    let config = field(field(&started, "effect"), "config").clone();
    let mut passed = Vec::new();

    let identity = config.get("a2a_identity");
    keep(
        &mut passed,
        "native gateway captures selected root and identity",
        matches!(config.get("a2a_root"), Some(Value::Str(text)) if text == &canonical_text)
            && matches!(identity, Some(Value::Arr(items)) if items.len() == 2),
    );
    let gateway = field(field(&started, "state"), "gateway");
    keep(
        &mut passed,
        "native settings retain explicit opt-in",
        matches!(gateway.get("a2a"), Some(Value::Bool(true))) && gateway.get("a2a_identity").is_none(),
    );
    let bad = controller.send(&obj(vec![
        ("op".into(), s("event")),
        ("root".into(), s(&workspace_text)),
        ("state".into(), launch_state.clone()),
        ("action".into(), s("gateway_start")),
        ("inputs".into(), gateway_inputs(&token, "0.0.0.0", true)),
    ]));
    keep(&mut passed, "native workspace LAN exposure refused", lan_refused(&bad));

    let (status, card, _head) = wire(&mut controller, &config, None, Some("1.0"), "/.well-known/agent-card.json", "GET", Some(&token));
    keep(
        &mut passed,
        "v1 card versions and bearer scheme",
        status == 200 && protocol_versions(&card) == Some(vec!["1.0", "0.3"]) && bearer_scheme(&card),
    );
    keep(
        &mut passed,
        "optional capabilities accurately disabled",
        card.get("capabilities") == Some(&obj(vec![("streaming".into(), b(false)), ("pushNotifications".into(), b(false))])),
    );
    let (status, _, head) = wire(&mut controller, &config, None, Some("1.0"), "/a2a", "POST", None);
    keep(&mut passed, "authentication precedes method dispatch", status == 401 && head.contains("WWW-Authenticate: Bearer"));
    keep(&mut passed, "wrong bearer refused", wire(&mut controller, &config, None, Some("1.0"), "/a2a", "POST", Some("wrong")).0 == 401);

    let first_reply = wire(&mut controller, &config, Some(&command("1.0", "message-one", "inspect", path_args("one.txt"), None)), Some("1.0"), "/a2a", "POST", Some(&token)).1;
    let first = task_of(&first_reply, "v1 real inspect task completes");
    keep(&mut passed, "v1 real inspect task completes", state_is(&first, "TASK_STATE_COMPLETED"));
    let output = data_of(&first).cloned().unwrap_or_else(|| panic!("task contains actual shared engine output"));
    let output_text = text_of(&output, "output").unwrap_or_default();
    keep(
        &mut passed,
        "task contains actual shared engine output",
        output_text.contains("one.txt")
            && text_of(&output, "returnedSha256").as_deref() == Some(sha256(output_text.as_bytes()).as_str())
            && output.get("complete").is_some_and(twin::truthy),
    );
    let first_id = text_of(&first, "id").unwrap_or_else(|| panic!("v1 real inspect task completes"));
    let second = task_of(
        &wire(&mut controller, &config, Some(&command("1.0", "message-one", "inspect", path_args("one.txt"), None)), Some("1.0"), "/a2a", "POST", Some(&token)).1,
        "identical message retry retains original task",
    );
    keep(&mut passed, "identical message retry retains original task", text_of(&second, "id").as_deref() == Some(first_id.as_str()));
    let changed = command("1.0", "message-one", "inspect", path_args("two.txt"), None);
    keep(
        &mut passed,
        "changed message ID reuse refused",
        error_code_is(&wire(&mut controller, &config, Some(&changed), Some("1.0"), "/a2a", "POST", Some(&token)).1, -32602),
    );
    let got = rpc(&mut controller, &config, "GetTask", obj(vec![("id".into(), s(&first_id)), ("historyLength".into(), n("0"))]), "1.0", &token);
    keep(&mut passed, "get task omits history on request", got.get("result").is_some_and(|task| task.get("history").is_none()));
    let mut follow = command("1.0", "follow", "inspect", path_args("one.txt"), None);
    put_message(&mut follow, "taskId", s(&first_id));
    keep(
        &mut passed,
        "completed task cannot restart",
        error_code_is(&wire(&mut controller, &config, Some(&follow), Some("1.0"), "/a2a", "POST", Some(&token)).1, -32004),
    );
    let compare = task_of(
        &wire(
            &mut controller,
            &config,
            Some(&command(
                "1.0",
                "comparison",
                "compare",
                obj(vec![("path".into(), s("one.txt")), ("other".into(), s("two.txt")), ("content".into(), s("complete_digest"))]),
                None,
            )),
            Some("1.0"),
            "/a2a",
            "POST",
            Some(&token),
        )
        .1,
        "actual complete file comparison",
    );
    let compare_text = data_of(&compare).and_then(|data| text_of(data, "output")).unwrap_or_default();
    keep(
        &mut passed,
        "actual complete file comparison",
        state_is(&compare, "TASK_STATE_COMPLETED")
            && ["one.txt", "two.txt"].iter().all(|name| compare_text.contains(&sha256(&std::fs::read(workspace.join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))))),
    );
    let directory = task_of(
        &wire(&mut controller, &config, Some(&command("1.0", "directory", "inspect", path_args("."), None)), Some("1.0"), "/a2a", "POST", Some(&token)).1,
        "directory inspection shares working resource limits",
    );
    keep(
        &mut passed,
        "directory inspection shares working resource limits",
        state_is(&directory, "TASK_STATE_COMPLETED") && data_of(&directory).and_then(|data| text_of(data, "output")).is_some_and(|text| text.contains("one.txt")),
    );
    std::fs::create_dir_all(workspace.join("sub")).unwrap();
    std::fs::write(workspace.join("sub").join("one.txt"), "Actual smaller tree").unwrap();
    let directories = task_of(
        &wire(
            &mut controller,
            &config,
            Some(&command("1.0", "directories", "compare", obj(vec![("path".into(), s(".")), ("other".into(), s("sub"))]), None)),
            Some("1.0"),
            "/a2a",
            "POST",
            Some(&token),
        )
        .1,
        "directory comparison shares working resource limits",
    );
    keep(&mut passed, "directory comparison shares working resource limits", state_is(&directories, "TASK_STATE_COMPLETED"));
    let legacy_reply = wire(&mut controller, &config, Some(&command("0.3", "legacy", "inspect", path_args("one.txt"), None)), None, "/a2a", "POST", Some(&token)).1;
    let legacy = legacy_reply.get("result").cloned().unwrap_or_else(|| panic!("missing version defaults to legacy: {}", legacy_reply.to_string()));
    keep(
        &mut passed,
        "missing version defaults to legacy",
        text_of(&legacy, "kind").as_deref() == Some("task") && text_of(field(&legacy, "status"), "state").as_deref() == Some("completed"),
    );
    let legacy_id = text_of(&legacy, "id").unwrap_or_else(|| panic!("missing version defaults to legacy"));
    keep(
        &mut passed,
        "legacy get task",
        rpc(&mut controller, &config, "tasks/get", obj(vec![("id".into(), s(&legacy_id))]), "0.3", &token)
            .get("result")
            .and_then(|task| text_of(task, "id"))
            .as_deref()
            == Some(legacy_id.as_str()),
    );
    keep(
        &mut passed,
        "cross-version cached task representation",
        rpc(&mut controller, &config, "GetTask", obj(vec![("id".into(), s(&legacy_id))]), "1.0", &token)
            .get("result")
            .and_then(|task| task.get("status"))
            .and_then(|status| text_of(status, "state"))
            .as_deref()
            == Some("TASK_STATE_COMPLETED"),
    );
    keep(
        &mut passed,
        "patch version ignored",
        wire(&mut controller, &config, Some(&command("1.0", "patch", "inspect", path_args("one.txt"), None)), Some("1.0.99"), "/a2a", "POST", Some(&token))
            .1
            .get("result")
            .and_then(|value| value.get("task"))
            .is_some_and(|task| state_is(task, "TASK_STATE_COMPLETED")),
    );
    let query_reply = wire(
        &mut controller,
        &config,
        Some(&command("1.0", "query", "inspect", path_args("one.txt"), None)),
        Some("1.0.2"),
        "/a2a?A2A-Version=1.0.7",
        "POST",
        Some(&token),
    )
    .1;
    if query_reply.get("result").is_none() {
        panic!("equivalent query and header patches: {}", query_reply.to_string());
    }
    keep(
        &mut passed,
        "equivalent query and header patches",
        query_reply.get("result").and_then(|value| value.get("task")).is_some_and(|task| state_is(task, "TASK_STATE_COMPLETED")),
    );
    keep(
        &mut passed,
        "conflicting version selection refused",
        wire(&mut controller, &config, Some(&command("1.0", "message-one", "inspect", path_args("one.txt"), None)), Some("1.0"), "/a2a?A2A-Version=0.3", "POST", Some(&token)).0 == 400,
    );
    keep(
        &mut passed,
        "unsupported protocol error",
        error_code_is(&wire(&mut controller, &config, Some(&command("1.0", "message-one", "inspect", path_args("one.txt"), None)), Some("2.0"), "/a2a", "POST", Some(&token)).1, -32009),
    );
    for path in ["../outside.txt", "/etc/passwd"] {
        let name = format!("escaping path refused {path}");
        let reply = wire(
            &mut controller,
            &config,
            Some(&command("1.0", &format!("escape-{path}"), "inspect", path_args(path), None)),
            Some("1.0"),
            "/a2a",
            "POST",
            Some(&token),
        )
        .1;
        keep(&mut passed, &name, reply.get("error").is_some());
    }
    let symlink_reply = wire(&mut controller, &config, Some(&command("1.0", "symlink", "inspect", path_args("escape"), None)), Some("1.0"), "/a2a", "POST", Some(&token)).1;
    let symlink = task_of(&symlink_reply, "escaping symlink never reveals private data");
    keep(&mut passed, "escaping symlink never reveals private data", !symlink.to_string().contains("OUTSIDE PRIVATE DATA"));
    keep(
        &mut passed,
        "write operation is not exposed",
        error_code_is(&wire(&mut controller, &config, Some(&command("1.0", "write", "write", path_args("one.txt"), None)), Some("1.0"), "/a2a", "POST", Some(&token)).1, -32004),
    );
    keep(
        &mut passed,
        "client cannot choose root",
        error_code_is(
            &wire(
                &mut controller,
                &config,
                Some(&command("1.0", "root-override", "inspect", obj(vec![("path".into(), s("one.txt")), ("root".into(), s(scratch.path().to_str().unwrap()))]), None)),
                Some("1.0"),
                "/a2a",
                "POST",
                Some(&token),
            )
            .1,
            -32602,
        ),
    );
    keep(
        &mut passed,
        "non-data part rejected",
        error_code_is(
            &wire(
                &mut controller,
                &config,
                Some(&obj(vec![
                    ("jsonrpc".into(), s("2.0")),
                    ("id".into(), n("1")),
                    ("method".into(), s("SendMessage")),
                    ("params".into(), obj(vec![("message".into(), obj(vec![
                        ("messageId".into(), s("text")),
                        ("role".into(), s("ROLE_USER")),
                        ("parts".into(), Value::Arr(vec![obj(vec![("text".into(), s("read files"))])])),
                    ]))])),
                ])),
                Some("1.0"),
                "/a2a",
                "POST",
                Some(&token),
            )
            .1,
            -32005,
        ),
    );
    keep(
        &mut passed,
        "push notification request refused",
        error_code_is(
            &wire(
                &mut controller,
                &config,
                Some(&command(
                    "1.0",
                    "push",
                    "inspect",
                    path_args("one.txt"),
                    Some(obj(vec![("taskPushNotificationConfig".into(), obj(vec![("url".into(), s("http://attacker.invalid"))]))])),
                )),
                Some("1.0"),
                "/a2a",
                "POST",
                Some(&token),
            )
            .1,
            -32003,
        ),
    );
    keep(&mut passed, "stream capability unavailable", error_code_is(&rpc(&mut controller, &config, "SendStreamingMessage", obj(vec![]), "1.0", &token), -32004));
    keep(&mut passed, "unknown method refused", error_code_is(&rpc(&mut controller, &config, "execute_shell", obj(vec![]), "1.0", &token), -32601));
    let page = rpc(&mut controller, &config, "ListTasks", obj(vec![("pageSize".into(), n("2")), ("historyLength".into(), n("0"))]), "1.0", &token)
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("list pages have bounded rows and counts"));
    let tasks = array_of(&page, "tasks").cloned().unwrap_or_else(|| panic!("list pages have bounded rows and counts"));
    keep(
        &mut passed,
        "list pages have bounded rows and counts",
        tasks.len() == 2 && at_least(page.get("totalSize"), 5) && page.get("nextPageToken").is_some_and(twin::truthy),
    );
    keep(&mut passed, "list omits history and artifacts by default", tasks.iter().all(|task| task.get("history").is_none() && task.get("artifacts").is_none()));
    let times: Vec<String> = tasks.iter().map(|task| {
        let status = task.get("status").unwrap_or_else(|| panic!("list orders latest status first"));
        if status.get("timestamp").is_none() {
            panic!("list orders latest status first");
        }
        text_of(status, "timestamp").unwrap_or_default()
    }).collect();
    let mut ordered = times.clone();
    ordered.sort_by(|left, right| right.cmp(left));
    keep(&mut passed, "list orders latest status first", times == ordered);
    let page_token = page.get("nextPageToken").cloned().unwrap_or(Value::Null);
    keep(
        &mut passed,
        "changed page filters rejected",
        error_code_is(
            &rpc(
                &mut controller,
                &config,
                "ListTasks",
                obj(vec![("pageSize".into(), n("1")), ("historyLength".into(), n("0")), ("pageToken".into(), page_token.clone())]),
                "1.0",
                &token,
            ),
            -32602,
        ),
    );
    let following = rpc(
        &mut controller,
        &config,
        "ListTasks",
        obj(vec![("pageSize".into(), n("2")), ("historyLength".into(), n("0")), ("pageToken".into(), page_token)]),
        "1.0",
        &token,
    )
    .get("result")
    .cloned()
    .unwrap_or_else(|| panic!("cursor advances immutable snapshot"));
    let following_tasks = array_of(&following, "tasks").cloned().unwrap_or_else(|| panic!("cursor advances immutable snapshot"));
    keep(&mut passed, "cursor advances immutable snapshot", ids(&tasks).is_disjoint(&ids(&following_tasks)));
    keep(
        &mut passed,
        "invalid timestamp rejected",
        error_code_is(&rpc(&mut controller, &config, "ListTasks", obj(vec![("statusTimestampAfter".into(), s("2026-01-01T-1:00:00Z"))]), "1.0", &token), -32602),
    );
    keep(
        &mut passed,
        "future timestamp filters older work",
        rpc(&mut controller, &config, "ListTasks", obj(vec![("statusTimestampAfter".into(), s("9999-01-01T00:00:00Z"))]), "1.0", &token)
            .get("result")
            .and_then(|value| value.get("tasks"))
            .and_then(|value| match value {
                Value::Arr(items) => Some(items.is_empty()),
                _ => None,
            })
            .unwrap_or(false),
    );
    keep(&mut passed, "invalid page size rejected", error_code_is(&rpc(&mut controller, &config, "ListTasks", obj(vec![("pageSize".into(), n("0"))]), "1.0", &token), -32602));
    keep(
        &mut passed,
        "terminal cancel returns protocol error",
        error_code_is(&rpc(&mut controller, &config, "CancelTask", obj(vec![("id".into(), s(&legacy_id))]), "1.0", &token), -32002),
    );
    keep(&mut passed, "unknown task uses scoped not-found", error_code_is(&rpc(&mut controller, &config, "GetTask", obj(vec![("id".into(), s("unknown"))]), "1.0", &token), -32001));

    let mut denied = gateway_state.clone();
    put(&mut denied, "harness_policy", deny_policy());
    must_value(&mut controller, &obj(vec![("op".into(), s("harness_policy_commit")), ("state".into(), denied)]), "cached task unavailable after rule revocation");
    keep(
        &mut passed,
        "cached task unavailable after rule revocation",
        error_code_is(&rpc(&mut controller, &config, "GetTask", obj(vec![("id".into(), s(&legacy_id))]), "1.0", &token), -32001),
    );
    keep(
        &mut passed,
        "new denied task refused",
        wire(&mut controller, &config, Some(&command("1.0", "denied", "inspect", path_args("one.txt"), None)), Some("1.0"), "/a2a", "POST", Some(&token)).1.get("error").is_some(),
    );
    keep(&mut passed, "unrelated health remains available", wire(&mut controller, &config, None, Some("1.0"), "/health", "GET", Some(&token)).0 == 200);
    let mut cleared = gateway_state.clone();
    put(&mut cleared, "harness_policy", Value::Null);
    put(&mut cleared, "harness_policy_epoch", n("1"));
    must_value(&mut controller, &obj(vec![("op".into(), s("harness_policy_commit")), ("state".into(), cleared)]), "stopped interface retires task memory");
    must_value(&mut controller, &obj(vec![("op".into(), s("a2a_close")), ("config".into(), config.clone())]), "stopped interface retires task memory");
    keep(&mut passed, "stopped interface retires task memory", rpc(&mut controller, &config, "GetTask", obj(vec![("id".into(), s(&legacy_id))]), "1.0", &token).get("error").is_some());
    let restarted = controller.event(Some(&gateway_state), "gateway_start", gateway_inputs(&token, "127.0.0.1", false), &workspace_text);
    let config = field(field(&restarted, "effect"), "config").clone();
    let held = scratch.path().join("held");
    std::fs::rename(&workspace, &held).unwrap_or_else(|error| panic!("rename workspace: {error}"));
    std::fs::create_dir_all(&workspace).unwrap();
    keep(&mut passed, "root replacement refused", wire(&mut controller, &config, None, Some("1.0"), "/.well-known/agent-card.json", "GET", Some(&token)).0 == 503);
    controller.finish();

    let binary = root.join("output/headless/RLLM");
    let mut server = Command::new(&binary)
        .arg("--a2a")
        .arg("--root")
        .arg(&held)
        .arg("--token-file")
        .arg(&secret)
        .arg("--port")
        .arg("0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("headless listener starts without a model: {error}"));
    let stderr = server.stderr.take().unwrap_or_else(|| panic!("headless listener starts without a model"));
    let port = listen_port(stderr);
    let mut running = Running { child: Some(server) };
    keep(&mut passed, "headless listener starts without a model", port > 0);
    keep(&mut passed, "actual HTTP authentication enforced", http_json(port, "POST", "/a2a", Some(&command("1.0", "message-one", "inspect", path_args("one.txt"), None)), "1.0", None).0 == 401);
    let socket = http_task(port, &command("1.0", "socket", "inspect", path_args("one.txt"), None), "1.0", &token, "actual HTTP shared inspection completes");
    keep(&mut passed, "actual HTTP shared inspection completes", state_is(&socket, "TASK_STATE_COMPLETED"));
    let legacy_http = http_json(port, "POST", "/a2a", Some(&command("0.3", "socket-old", "inspect", path_args("one.txt"), None)), "0.3", Some(&token)).1;
    keep(&mut passed, "actual HTTP legacy binding works", legacy_http.get("result").and_then(|value| text_of(value, "kind")).as_deref() == Some("task"));
    let card = http_json(port, "GET", "/.well-known/agent-card.json", None, "1.0", Some(&token)).1;
    keep(
        &mut passed,
        "actual HTTP card advertised local URL",
        card.get("supportedInterfaces").and_then(first_item).and_then(|item| text_of(item, "url")).as_deref() == Some(format!("http://127.0.0.1:{port}/a2a").as_str()),
    );
    let large = held.join("large.bin");
    std::fs::File::create(&large).and_then(|file| file.set_len(128 * 1024 * 1024)).unwrap_or_else(|error| panic!("large.bin: {error}"));
    let workers = host_workers(&project, held.to_str().unwrap_or_else(|| panic!("held path is not UTF-8")));
    keep(&mut passed, "absent device metadata discovers bounded host workers", (1..=4).contains(&workers));
    let immediate = Some(obj(vec![("returnImmediately".into(), b(true))]));
    let large_args = obj(vec![("path".into(), s("large.bin")), ("content".into(), s("complete_digest"))]);
    let mut jobs = Vec::new();
    for number in 0..workers {
        let reply = http_json(
            port,
            "POST",
            "/a2a",
            Some(&command("1.0", &format!("parallel-{number}"), "inspect", large_args.clone(), immediate.clone())),
            "1.0",
            Some(&token),
        )
        .1;
        if reply.get("result").is_none() {
            panic!("discovered default admits independent actual scans concurrently: {}", reply.to_string());
        }
        jobs.push(task_of(&reply, "discovered default admits independent actual scans concurrently"));
    }
    let mut states = Vec::new();
    for (number, job) in jobs.iter().enumerate() {
        let id = text_of(job, "id").unwrap_or_else(|| panic!("discovered default admits independent actual scans concurrently"));
        states.push(http_result(port, "GetTask", obj(vec![("id".into(), s(&id))]), 300 + number as i64, &token, "discovered default admits independent actual scans concurrently"));
    }
    let concurrent = states.iter().all(|task| pending(task)) && ids(&states).len() == workers;
    if !concurrent {
        panic!("discovered default admits independent actual scans concurrently: {}", states.iter().map(Value::to_string).collect::<Vec<_>>().join(" | "));
    }
    keep(&mut passed, "discovered default admits independent actual scans concurrently", true);
    let overload = http_json(
        port,
        "POST",
        "/a2a",
        Some(&command("1.0", "parallel-overload", "inspect", large_args.clone(), immediate.clone())),
        "1.0",
        Some(&token),
    )
    .1;
    keep(&mut passed, "global task capacity refuses only the excess request", error_code_is(&overload, -32000));
    let first_job = text_of(&jobs[0], "id").unwrap_or_else(|| panic!("one concurrent scan can be cancelled independently"));
    let cancel = http_result(port, "CancelTask", obj(vec![("id".into(), s(&first_job))]), 310, &token, "one concurrent scan can be cancelled independently");
    keep(&mut passed, "one concurrent scan can be cancelled independently", state_is(&cancel, "TASK_STATE_CANCELED") && empty_list(&cancel, "artifacts"));
    let deadline = Instant::now() + Duration::from_secs(35);
    let rest: Vec<String> = jobs[1..].iter().map(|job| text_of(job, "id").unwrap_or_else(|| panic!("other concurrent scans complete after scoped cancellation"))).collect();
    let finished = loop {
        let mut current = Vec::new();
        for (number, id) in rest.iter().enumerate() {
            current.push(http_result(port, "GetTask", obj(vec![("id".into(), s(id))]), 320 + number as i64, &token, "other concurrent scans complete after scoped cancellation"));
        }
        if current.iter().any(|task| state_is(task, "TASK_STATE_FAILED") || state_is(task, "TASK_STATE_CANCELED")) {
            panic!("other concurrent scans complete after scoped cancellation: {}", current.iter().map(Value::to_string).collect::<Vec<_>>().join(" | "));
        }
        if current.iter().all(|task| state_is(task, "TASK_STATE_COMPLETED")) {
            break current;
        }
        if Instant::now() >= deadline {
            panic!("other concurrent scans complete after scoped cancellation");
        }
        thread::sleep(Duration::from_millis(50));
    };
    keep(&mut passed, "other concurrent scans complete after scoped cancellation", finished.len() == workers - 1 && finished.iter().all(|task| state_is(task, "TASK_STATE_COMPLETED")));
    let expected = sha256(&std::fs::read(&large).unwrap_or_else(|error| panic!("large.bin: {error}")));
    keep(&mut passed, "concurrent task results retain exact source and output identity", finished.iter().all(|task| digest_matches(task, &expected)));
    let running_task = http_task(port, &command("1.0", "cancel-running", "inspect", large_args.clone(), immediate.clone()), "1.0", &token, "nonblocking task returns before large scan completes");
    keep(&mut passed, "nonblocking task returns before large scan completes", pending(&running_task));
    let running_id = text_of(&running_task, "id").unwrap_or_else(|| panic!("nonblocking task returns before large scan completes"));
    let cancelled = http_result(port, "CancelTask", obj(vec![("id".into(), s(&running_id))]), 200, &token, "active HTTP task cancellation accepted");
    keep(&mut passed, "active HTTP task cancellation accepted", state_is(&cancelled, "TASK_STATE_CANCELED") && matches!(cancelled.get("artifacts"), Some(Value::Arr(items)) if items.is_empty()));
    let again = http_result(port, "CancelTask", obj(vec![("id".into(), s(&running_id))]), 201, &token, "v1 cancellation is idempotent");
    keep(&mut passed, "v1 cancellation is idempotent", state_is(&again, "TASK_STATE_CANCELED"));
    let retry_deadline = Instant::now() + Duration::from_secs(5);
    let after_cancel = loop {
        let reply = http_json(port, "POST", "/a2a", Some(&command("1.0", "after-cancel", "inspect", path_args("one.txt"), None)), "1.0", Some(&token)).1;
        if reply.get("result").is_some() {
            break reply;
        }
        if !error_code_is(&reply, -32000) || Instant::now() >= retry_deadline {
            panic!("cancelled scan does not cancel another task: {}", reply.to_string());
        }
        thread::sleep(Duration::from_millis(20));
    };
    keep(
        &mut passed,
        "cancelled scan does not cancel another task",
        after_cancel.get("result").and_then(|value| value.get("task")).is_some_and(|task| state_is(task, "TASK_STATE_COMPLETED")),
    );
    let late = http_result(port, "GetTask", obj(vec![("id".into(), s(&running_id))]), 202, &token, "cancelled task publishes no late artifact");
    keep(&mut passed, "cancelled task publishes no late artifact", matches!(late.get("artifacts"), Some(Value::Arr(items)) if items.is_empty()));

    let active = http_task(port, &command("1.0", "blocking-owned", "inspect", large_args.clone(), immediate.clone()), "1.0", &token, "blocking wait admission leaves control capacity");
    let active_id = text_of(&active, "id").unwrap_or_else(|| panic!("blocking wait admission leaves control capacity"));
    let blocking = command("1.0", "blocking-owned", "inspect", large_args, None);
    let (tx, rx) = mpsc::channel();
    let token_for_wait = token.clone();
    thread::spawn(move || {
        let _ = tx.send(http_json(port, "POST", "/a2a", Some(&blocking), "1.0", Some(&token_for_wait)));
    });
    thread::sleep(Duration::from_millis(50));
    let mut probe = command("1.0", "blocked-admission", "inspect", path_args("one.txt"), None);
    put_message(&mut probe, "contextId", s("blocked-context"));
    let overload = http_json(port, "POST", "/a2a", Some(&probe), "1.0", Some(&token)).1;
    keep(&mut passed, "blocking wait admission leaves control capacity", error_code_is(&overload, -32000));
    let began = Instant::now();
    let control = http_json(port, "POST", "/a2a", Some(&rpc_call("GetTask", obj(vec![("id".into(), s(&active_id))]), 203)), "1.0", Some(&token)).1;
    keep(
        &mut passed,
        "status remains responsive during blocking request",
        began.elapsed() < Duration::from_secs(2) && control.get("result").and_then(|value| text_of(value, "id")).as_deref() == Some(active_id.as_str()),
    );
    let cancel = http_result(port, "CancelTask", obj(vec![("id".into(), s(&active_id))]), 204, &token, "cancellation remains available during blocking request");
    keep(&mut passed, "cancellation remains available during blocking request", state_is(&cancel, "TASK_STATE_CANCELED"));
    let waited = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("blocked retry receives terminal cancellation"));
    let waited_task = waited.1.get("result").and_then(|value| value.get("task")).cloned().unwrap_or_else(|| panic!("blocked retry receives terminal cancellation: {}", waited.1.to_string()));
    keep(&mut passed, "blocked retry receives terminal cancellation", state_is(&waited_task, "TASK_STATE_CANCELED"));
    let rejected = http_result(port, "ListTasks", obj(vec![("contextId".into(), s("blocked-context"))]), 205, &token, "overloaded blocking call creates no hidden task");
    keep(&mut passed, "overloaded blocking call creates no hidden task", matches!(rejected.get("tasks"), Some(Value::Arr(items)) if items.is_empty()));
    let fresh = http_task(port, &command("1.0", "after-blocking", "inspect", path_args("one.txt"), None), "1.0", &token, "completion releases blocking admission");
    keep(&mut passed, "completion releases blocking admission", state_is(&fresh, "TASK_STATE_COMPLETED"));
    let duplicate = http_exchange(port, "POST", "/a2a", Some(r#"{"jsonrpc":"2.0","id":1,"id":2}"#), "1.0", Some(&token)).1;
    keep(&mut passed, "duplicate JSON fields rejected on wire", error_code_is(&Value::parse(&duplicate), -32700));

    let mut server = running.child.take().unwrap_or_else(|| panic!("SIGTERM stops local listener cleanly"));
    let pid = server.id() as i32;
    // std has no SIGTERM; SIGKILL would change the asserted exit status
    let signaled = { unsafe extern "C" { fn kill(pid: i32, sig: i32) -> i32; } unsafe { kill(pid, 15) } };
    if signaled != 0 {
        panic!("SIGTERM stops local listener cleanly");
    }
    let (wait_tx, wait_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = wait_tx.send(server.wait());
    });
    let status = wait_rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| panic!("SIGTERM stops local listener cleanly")).unwrap_or_else(|error| panic!("SIGTERM stops local listener cleanly: {error}"));
    keep(&mut passed, "SIGTERM stops local listener cleanly", status.code() == Some(0));

    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut failed_child = Command::new(&binary)
        .arg("--a2a")
        .arg("--root")
        .arg(&workspace)
        .arg("--token-file")
        .arg(&secret)
        .arg("--port")
        .arg("0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("publicly readable token file refused: {error}"));
    let mut stderr = failed_child.stderr.take().unwrap_or_else(|| panic!("publicly readable token file refused"));
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_tx.send(stderr.read_to_end(&mut buf).map(|_| buf));
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match failed_child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let other: Result<Result<std::process::Output, std::io::Error>, mpsc::RecvTimeoutError> = Err(mpsc::RecvTimeoutError::Timeout);
                let _ = failed_child.kill();
                panic!("publicly readable token file refused: {other:?}");
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let other: Result<Result<std::process::Output, std::io::Error>, mpsc::RecvTimeoutError> = Ok(Err(error));
                let _ = failed_child.kill();
                panic!("publicly readable token file refused: {other:?}");
            }
        }
    };
    let stderr_bytes = match stderr_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(bytes)) => bytes,
        other => {
            let _ = failed_child.kill();
            panic!("publicly readable token file refused: {other:?}");
        }
    };
    let failed = std::process::Output { status, stdout: Vec::new(), stderr: stderr_bytes };
    let failed_text = String::from_utf8_lossy(&failed.stderr);
    keep(&mut passed, "publicly readable token file refused", !failed.status.success() && failed_text.contains("private regular file"));

    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("observed_host_workers".into(), n(&workers.to_string())),
    ]);
    twin::report_value("a2a", &body, true);
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn state<'a>(value: &'a Value) -> &'a Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}: {}", value.to_string()))
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

fn put_message(command: &mut Value, key: &str, value: Value) {
    let params = field_mut(command, "params");
    let message = field_mut(params, "message");
    put(message, key, value);
}

fn gateway_inputs(token: &str, listen: &str, lan: bool) -> Value {
    let mut inputs = obj(vec![
        ("listen".into(), s(listen)),
        ("port".into(), s("8080")),
        ("token".into(), s(token)),
        ("a2a".into(), b(true)),
    ]);
    if lan {
        put(&mut inputs, "lan", b(true));
    }
    inputs
}

fn deny_policy() -> Value {
    obj(vec![
        ("schema".into(), n("1")),
        ("rules".into(), Value::Arr(vec![obj(vec![
            ("id".into(), s("current-rule")),
            ("text".into(), s("Do not inspect this workspace")),
            ("scope".into(), s("tool")),
            ("tool".into(), s("inspect")),
            ("decision".into(), s("deny")),
            ("enabled".into(), b(true)),
        ])])),
    ])
}

fn path_args(path: &str) -> Value {
    obj(vec![("path".into(), s(path))])
}

fn command(version: &str, mid: &str, tool: &str, args: Value, config: Option<Value>) -> Value {
    let mut message = obj(vec![
        ("messageId".into(), s(mid)),
        ("role".into(), s(if version.starts_with("1.") { "ROLE_USER" } else { "user" })),
        ("parts".into(), Value::Arr(vec![obj(vec![("data".into(), obj(vec![("tool".into(), s(tool)), ("args".into(), args)]))])])),
    ]);
    if version.starts_with("0.") {
        put(&mut message, "kind", s("message"));
        let parts = field_mut(&mut message, "parts");
        let Value::Arr(items) = parts else {
            panic!("message parts");
        };
        put(&mut items[0], "kind", s("data"));
    }
    let mut params = obj(vec![("message".into(), message)]);
    if let Some(config) = config {
        put(&mut params, "configuration", config);
    }
    obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), s(mid)),
        ("method".into(), s(if version.starts_with("1.") { "SendMessage" } else { "message/send" })),
        ("params".into(), params),
    ])
}

fn rpc_call(method: &str, params: Value, id: i64) -> Value {
    obj(vec![("jsonrpc".into(), s("2.0")), ("id".into(), n(&id.to_string())), ("method".into(), s(method)), ("params".into(), params)])
}

fn wire(controller: &mut Controller, config: &Value, body: Option<&Value>, version: Option<&str>, path: &str, method: &str, auth: Option<&str>) -> (u16, Value, String) {
    let raw = body.map(Value::to_string).unwrap_or_default();
    let mut header = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", raw.len());
    if let Some(version) = version {
        header.push_str(&format!("A2A-Version: {version}\r\n"));
    }
    if let Some(auth) = auth {
        header.push_str(&format!("Authorization: Bearer {auth}\r\n"));
    }
    header.push_str("\r\n");
    let reply_value = controller.op(&obj(vec![
        ("op".into(), s("gateway_incoming")),
        ("config".into(), config.clone()),
        ("header".into(), s(&header)),
        ("body".into(), s(&raw)),
    ]));
    let reply = reply_value.get("reply").and_then(|value| match value {
        Value::Str(text) => Some(text.clone()),
        _ => None,
    }).unwrap_or_else(|| panic!("gateway reply missing: {}", reply_value.to_string()));
    let (head, body) = reply.split_once("\r\n\r\n").unwrap_or_else(|| panic!("gateway reply framing: {reply}"));
    let status = head.split_whitespace().nth(1).unwrap_or_else(|| panic!("gateway status: {head}")).parse::<u16>().unwrap_or_else(|_| panic!("gateway status: {head}"));
    (status, Value::parse(body), head.to_string())
}

fn rpc(controller: &mut Controller, config: &Value, method: &str, params: Value, version: &str, token: &str) -> Value {
    wire(controller, config, Some(&rpc_call(method, params, 123)), Some(version), "/a2a", "POST", Some(token)).1
}

fn must_value(controller: &mut Controller, envelope: &Value, name: &str) -> Value {
    let reply = controller.send(envelope);
    match reply.get("value") {
        Some(value) => value.clone(),
        None => panic!("{name}: {}", reply.to_string()),
    }
}

fn lan_refused(bad: &Value) -> bool {
    if bad.get("error").is_some() {
        return true;
    }
    let Some(value) = bad.get("value") else {
        return false;
    };
    let effect_none = matches!(value.get("effect"), Some(Value::Null));
    let notice = value.get("state").and_then(|state| state.get("notice")).and_then(|notice| match notice {
        Value::Str(text) => Some(text.as_str()),
        _ => None,
    });
    effect_none && notice.is_some_and(|text| text.contains("local-only"))
}

fn protocol_versions(card: &Value) -> Option<Vec<&str>> {
    let Value::Arr(items) = card.get("supportedInterfaces")? else {
        return None;
    };
    let mut versions = Vec::new();
    for item in items {
        match item.get("protocolVersion") {
            Some(Value::Str(text)) => versions.push(text.as_str()),
            _ => return None,
        }
    }
    Some(versions)
}

fn bearer_scheme(card: &Value) -> bool {
    let Some(Value::Arr(requirements)) = card.get("securityRequirements") else {
        return false;
    };
    requirements.first().and_then(|item| item.get("schemes")).and_then(|schemes| schemes.get("bearer"))
        == Some(&obj(vec![("list".into(), Value::Arr(vec![]))]))
}

fn task_of(reply: &Value, name: &str) -> Value {
    reply.get("result").and_then(|value| value.get("task")).cloned().unwrap_or_else(|| panic!("{name}: {}", reply.to_string()))
}

fn data_of(task: &Value) -> Option<&Value> {
    let artifact = first_item(task.get("artifacts")?)?;
    let part = first_item(artifact.get("parts")?)?;
    part.get("data")
}

fn first_item(value: &Value) -> Option<&Value> {
    match value {
        Value::Arr(items) => items.first(),
        _ => None,
    }
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Value::Str(text)) => Some(text.clone()),
        _ => None,
    }
}

fn array_of<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    match value.get(key) {
        Some(Value::Arr(items)) => Some(items),
        _ => None,
    }
}

fn state_is(task: &Value, expect: &str) -> bool {
    task.get("status").and_then(|status| text_of(status, "state")).as_deref() == Some(expect)
}

fn pending(task: &Value) -> bool {
    state_is(task, "TASK_STATE_SUBMITTED") || state_is(task, "TASK_STATE_WORKING")
}

fn at_least(value: Option<&Value>, minimum: u64) -> bool {
    match value {
        Some(Value::Num(token)) => token.parse::<u64>().ok().is_some_and(|number| number >= minimum),
        _ => false,
    }
}

fn error_code_is(body: &Value, expected: i64) -> bool {
    match body.get("error").and_then(|error| error.get("code")) {
        Some(Value::Num(token)) => token.parse::<i64>().ok() == Some(expected),
        _ => false,
    }
}

fn empty_list(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Arr(items)) if items.is_empty())
}

fn ids(tasks: &[Value]) -> HashSet<String> {
    tasks.iter().filter_map(|task| text_of(task, "id")).collect()
}

fn digest_matches(task: &Value, source: &str) -> bool {
    let Some(data) = data_of(task) else {
        return false;
    };
    let Some(output) = text_of(data, "output") else {
        return false;
    };
    output.contains(source) && text_of(data, "returnedSha256").as_deref() == Some(sha256(output.as_bytes()).as_str())
}

fn sha256(bytes: &[u8]) -> String {
    twin::sha256_bytes(bytes)
}

fn listen_port(stderr: impl Read + Send + 'static) -> u16 {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = tx.send(line);
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
    });
    let line = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| panic!("headless listener starts without a model"));
    let end = line.find("/a2a").unwrap_or_else(|| panic!("headless listener starts without a model: {line}"));
    let start = line[..end].rfind(':').unwrap_or_else(|| panic!("headless listener starts without a model: {line}"));
    line[start + 1..end].parse().unwrap_or_else(|_| panic!("headless listener starts without a model: {line}"))
}

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>, version: &str, auth: Option<&str>) -> (u16, Value) {
    let raw = body.map(Value::to_string);
    let (status, text) = http_exchange(port, method, path, raw.as_deref(), version, auth);
    (status, Value::parse(&text))
}

fn http_exchange(port: u16, method: &str, path: &str, body: Option<&str>, version: &str, auth: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|error| panic!("connect {port}: {error}"));
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(40))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(40))).ok();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept-Encoding: identity\r\nContent-Type: application/json\r\nA2A-Version: {version}\r\n");
    if let Some(auth) = auth {
        request.push_str(&format!("Authorization: Bearer {auth}\r\n"));
    }
    if let Some(body) = body {
        request.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    } else {
        request.push_str("\r\n");
    }
    stream.write_all(request.as_bytes()).and_then(|_| stream.flush()).unwrap_or_else(|error| panic!("HTTP write: {error}"));
    read_http(&mut stream)
}

fn read_http(stream: &mut TcpStream) -> (u16, String) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        if buf.len() > 65536 {
            panic!("HTTP header too large");
        }
        let count = stream.read(&mut tmp).unwrap_or_else(|error| panic!("HTTP read: {error}"));
        if count == 0 {
            panic!("HTTP connection closed before headers");
        }
        buf.extend_from_slice(&tmp[..count]);
        if let Some(position) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
    };
    let header = std::str::from_utf8(&buf[..header_end]).unwrap_or_else(|_| panic!("HTTP header is not UTF-8"));
    let status = header.split_whitespace().nth(1).unwrap_or_else(|| panic!("HTTP status missing: {header}")).parse::<u16>().unwrap_or_else(|_| panic!("HTTP status invalid: {header}"));
    let mut length = None;
    for line in header.split("\r\n").skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().unwrap_or_else(|_| panic!("bad content length: {value}")));
            }
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    if let Some(length) = length {
        while body.len() < length {
            let count = stream.read(&mut tmp).unwrap_or_else(|error| panic!("HTTP body: {error}"));
            if count == 0 {
                panic!("HTTP body ended early");
            }
            body.extend_from_slice(&tmp[..count]);
        }
        body.truncate(length);
    } else {
        loop {
            match stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(count) => body.extend_from_slice(&tmp[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut || error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("HTTP body: {error}"),
            }
        }
    }
    (status, String::from_utf8(body).unwrap_or_else(|_| panic!("HTTP body is not UTF-8")))
}

fn http_task(port: u16, body: &Value, version: &str, token: &str, name: &str) -> Value {
    let reply = http_json(port, "POST", "/a2a", Some(body), version, Some(token)).1;
    task_of(&reply, name)
}

fn http_result(port: u16, method: &str, params: Value, id: i64, token: &str, name: &str) -> Value {
    let reply = http_json(port, "POST", "/a2a", Some(&rpc_call(method, params, id)), "1.0", Some(token)).1;
    reply.get("result").cloned().unwrap_or_else(|| panic!("{name}: {}", reply.to_string()))
}

fn host_workers(project: &str, workspace: &str) -> usize {
    let mut probe = Controller::spawn(project);
    let resumed = probe.event(None, "resume", obj(vec![]), workspace);
    let count = resumed.get("state").and_then(|state| state.get("policy")).and_then(|policy| policy.get("workers")).and_then(|value| match value {
        Value::Num(token) | Value::Str(token) => token.parse::<usize>().ok(),
        _ => None,
    });
    probe.finish();
    count.unwrap_or_else(|| panic!("absent device metadata discovers bounded host workers"))
}

struct Scratch(PathBuf);

impl Scratch {
    fn create(root: &Path) -> Self {
        Self(twin::create_scratch(root, "a2a"))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Running {
    child: Option<Child>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

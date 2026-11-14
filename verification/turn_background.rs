mod twin;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use twin::{obj, report_value, s, Value};

const SCOPE: &str = "The turn-promotion contract read from the real manifest, the real Activity and service sources, and read back out of the packaged APK with the real linker and dexdump: declared promotion type, permission count, wake-lock window, held-reply publication and the tap-to-conversation route. No device is attached, so the platform behaviour of a refused background start, the per-app foreground budget and a real notification delivery are asserted as source contract only, not observed on hardware.";

fn main() {
    let mut args = std::env::args().skip(1);
    let apk = match args.next().as_deref() {
        Some("--apk") => Some(args.next().unwrap_or_else(|| panic!("--apk requires a path"))),
        Some(arg) => panic!("unknown argument: {arg}"),
        None => None,
    };
    if let Some(extra) = args.next() {
        panic!("unknown argument: {extra}");
    }

    let project = project_root();
    let passed = run(&project, apk.as_deref());
    let checks = Value::Arr(passed.iter().map(|name| s(name)).collect());
    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), checks),
        ("scope".into(), s(SCOPE)),
    ]);
    report_value("turn_background", &body, true);
    println!("{}", twin::pretty(&body));
}

fn run(project: &Path, apk_argument: Option<&str>) -> Vec<String> {
    let mut passed = Vec::new();

    let manifest = read(project, "AndroidManifest.xml");
    let service = read(project, "platform/com/local/client/TurnService.java");
    let activity = read(project, "platform/com/local/client/Activity.java");

    check(
        &mut passed,
        "a declared foreground service is what promotes an ordinary model turn",
        manifest.contains("android:name=\".TurnService\"")
            && manifest.contains("android:foregroundServiceType=\"dataSync\"")
            && manifest.contains("android.permission.FOREGROUND_SERVICE_DATA_SYNC"),
    );
    check(
        &mut passed,
        "the promotion is started from the model-effect path and asks once for reply permission",
        activity.contains("turnActive=true;startRateClock(token,e.optString(\"model_ticket\"),started);protectionNoted=false;try{if(TurnService.begin(this,e.optString(\"model_ticket\"),started,turnLease(e),\"Replying\")==TurnService.REFUSED)turnProtectionLost();}catch(Exception unavailable){turnProtectionLost();}askReplyNotifications();"),
    );
    check(
        &mut passed,
        "the promotion holds a partial wake lock for the turn window and releases it at the deadline",
        service.contains("PowerManager.PARTIAL_WAKE_LOCK,\"rllm:turn\"")
            && service.contains("wake.setReferenceCounted(false);wake.acquire(lease);")
            && service.contains("if(elapsed>=lease){stopSelf();return;}"),
    );
    check(
        &mut passed,
        "the promotion names the dataSync type on the API level that requires it",
        service.contains("startForeground(PROGRESS_ID,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);"),
    );
    check(
        &mut passed,
        "the promotion is released when the task is removed or the service is destroyed",
        service.contains("public void onTaskRemoved(Intent root){stopSelf();}")
            && service.matches("stopForeground(STOP_FOREGROUND_REMOVE)").count() == 2,
    );
    check(
        &mut passed,
        "the held window is clamped once by the core turn clock for both the fresh and extended paths",
        service.contains("lease=leaseOf(window);")
            && service.contains("long next=leaseOf(window);")
            && service.contains("static long leaseOf(long window)throws Exception")
            && service.contains("\"turn_clock\"")
            && service.contains("return value.getString(\"text\");")
            && !service.contains("Math.max(MIN_LEASE")
            && !service.contains("MIN_LEASE=")
            && !service.contains("MAX_LEASE=")
            && !service.contains("String seconds(")
            && read(project, "core/lib.rs").contains("fn turn_lease(window: i64) -> i64 {\n    window.min(600000).max(30000)\n}")
            && read(project, "core/lib.rs").contains("fn turn_elapsed(elapsed_ms: i64) -> String {"),
    );
    check(
        &mut passed,
        "a core miss supplies no lease and does not substitute the requested timeout",
        activity.contains("private long turnLease(JSONObject effect)throws Exception{return TurnService.leaseOf(effect.optLong(\"timeout_ms\",120000L));}")
            && activity.contains("catch(Exception unavailable){turnProtectionLost();}")
            && !activity.contains("return requested")
            && !activity.contains("Math.max(30000L,Math.min(600000L")
            && service.contains("if(!intent.hasExtra(EXTRA_LEASE)){lost();if(openTicket(value))live=null;stop();return START_NOT_STICKY;}")
            && !service.contains("getLongExtra(EXTRA_LEASE,120000L)")
            && !service.contains("Math.max(MIN_LEASE"),
    );

    let lease_line = activity
        .split('\n')
        .find(|line| line.contains("private long turnLease"))
        .unwrap_or("");
    let transport_line = activity
        .split('\n')
        .find(|line| line.contains("transportDeadline=command.optLong"))
        .unwrap_or("");
    let clock = read(project, "core/lib.rs");
    check(
        &mut passed,
        "the promotion window and the request transport window share one default and one cap so the wake lock can neither outlive nor strand the turn",
        lease_line.contains("120000")
            && transport_line.contains("120000")
            && transport_line.contains("600000")
            && clock.contains("window.min(600000).max(30000)"),
    );

    let mut core_paths = Vec::new();
    for entry in std::fs::read_dir(project.join("core"))
        .unwrap_or_else(|error| panic!("could not read core directory: {error}"))
    {
        let path = entry
            .unwrap_or_else(|error| panic!("could not read core directory entry: {error}"))
            .path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            core_paths.push(path);
        }
    }
    core_paths.sort();
    let core = core_paths
        .iter()
        .map(|path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
        })
        .collect::<Vec<_>>();
    let carriers = core
        .iter()
        .flat_map(|text| text.lines())
        .filter(|line| line.contains("timeout_ms"))
        .map(str::trim)
        .collect::<Vec<_>>();
    let service_pair = "\"purpose\",s(&ticket)";
    let scaled = "(timeout*1000)";
    let model_pair = "put(&mut effect, \"purpose\", s(\"model\"));";
    let joined = core.join("\n");
    check(
        &mut passed,
        "only the service path carries a per-request timeout, so a model turn is promoted on the shared default rather than a guess",
        carriers.len() == 1
            && carriers[0].contains(service_pair)
            && carriers[0].contains(scaled)
            && joined.contains(model_pair),
    );
    check(
        &mut passed,
        "a chained model effect re-extends the live promotion instead of starting a second one",
        service.contains("if(service!=null&&service.active){live=new Ticket(ticket);try{service.extend(ticket,started,lease,label);return HELD;}catch(Exception refused){if(openTicket(ticket))live=null;service.stop();return REFUSED;}}")
            && service.contains("if(active){try{extend(value,from,window,text);}catch(Exception refused){lost();if(openTicket(value))live=null;stop();}return START_NOT_STICKY;}"),
    );
    let refuse_at = service
        .find("if(!openTicket(value)){if(!(active&&openTicket(ticket)))stopSelf(id);return START_NOT_STICKY;}")
        .unwrap_or(usize::MAX);
    let absent_at = service
        .find("if(!intent.hasExtra(EXTRA_LEASE)){lost();if(openTicket(value))live=null;stop();return START_NOT_STICKY;}")
        .unwrap_or(usize::MAX);
    let promote_at = service
        .find("startForeground(PROGRESS_ID,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);")
        .unwrap_or(0);
    let acquire_at = service
        .find("wake.setReferenceCounted(false);wake.acquire(lease);")
        .unwrap_or(0);
    let extend_check = service
        .find("if(!openTicket(value))throw new IllegalStateException(\"Turn ticket is not admitted\");")
        .unwrap_or(usize::MAX);
    let extend_acquire = service
        .find("wake.acquire(next);")
        .unwrap_or(0);
    check(
        &mut passed,
        "a start that arrives after its ticket was retired never promotes or takes the wake lock",
        service.contains("static void finish(){live=null;TurnService service=current;if(service!=null)service.stop();}")
            && refuse_at < absent_at
            && absent_at < promote_at
            && refuse_at < acquire_at
            && absent_at < acquire_at
            && extend_check < extend_acquire,
    );
    check(
        &mut passed,
        "the rate clock re-arms itself while the app is backgrounded so elapsed time keeps advancing",
        rate_clock_pattern(&activity)
            && activity.contains("if(!ratePaused&&(state.optString(\"page\").equals(\"chat\")||state.optString(\"page\").equals(\"diagnostics\")))try{dispatch(\"rate_tick\""),
    );
    check(
        &mut passed,
        "the first frame after a resume re-reads the true elapsed time instead of a stale snapshot",
        activity.contains("private void refreshRateClock(){")
            && activity.contains("if(!rateTicket.equals(stateText(state,\"model_expected\")))return;")
            && activity.contains("ratePaused=false;media.resume();super.onResume();"),
    );
    check(
        &mut passed,
        "a turn is only settled when the core reports nothing else pending",
        activity.contains("if(!stateText(next,\"model_expected\").isEmpty())return;")
            && activity.contains("if(planned!=null)return;")
            && activity.contains("if(next.optJSONObject(\"pending\")!=null)return;")
            && activity.contains("JSONArray stack=next.optJSONArray(\"delegation_stack\");if(stack!=null&&stack.length()>0)return;"),
    );
    check(
        &mut passed,
        "the promotion is released on every turn-ending path",
        activity.matches("TurnService.finish()").count() >= 4,
    );
    check(
        &mut passed,
        "a reply that lands while backgrounded is held and then drained when notifications are allowed",
        activity.contains("if(!ratePaused)return;\n        pendingReplyTitle=title;pendingReplyBody=body.trim();pendingReplyChat=conversation;\n        publishPendingReply();")
            && activity.contains("if(!TurnService.notificationsAllowed(this))return;")
            && activity.contains("TurnService.post(this,title,body,chat);"),
    );
    check(
        &mut passed,
        "the held reply is drained on resume and after the permission result arrives",
        activity.contains("if(code==56)publishPendingReply();")
            && activity.contains("refreshRateClock();openRequestedConversation();"),
    );
    check(
        &mut passed,
        "a reply tap names the conversation it belongs to and reopening waits for the turn to end",
        service.contains("static final String EXTRA_CHAT=\"rllm.conversation\";")
            && activity.contains("String id=intent.getStringExtra(TurnService.EXTRA_CHAT);")
            && activity.contains("if(requestedConversation.isEmpty()||closed||state==null||ratePaused)return;")
            && activity.contains("if(turnActive)return;")
            && activity.contains("requestConversation(intent);"),
    );
    check(
        &mut passed,
        "a refused background start or refused notification cannot fail the committed turn",
        service.matches("catch(RuntimeException refused)").count()
            + service.matches("catch(RuntimeException retired)").count()
            >= 5
            && service.contains("catch(RuntimeException unavailable){/* A refused notification never blocks the committed reply. */}"),
    );

    let apk = subject(project, apk_argument);
    let tools = PathBuf::from(
        std::env::var_os("CLIENT_TOOLS").unwrap_or_else(|| panic!("CLIENT_TOOLS is not set")),
    );
    let scratch = twin::create_scratch(project, "turn-background");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        packaged_checks(project, &apk, &tools, &scratch, &mut passed)
    }));
    let cleaned = std::fs::remove_dir_all(&scratch);
    match (result, cleaned) {
        (Ok(()), Ok(())) => passed,
        (Ok(()), Err(error)) => panic!("could not remove {}: {error}", scratch.display()),
        (Err(payload), Ok(())) => std::panic::resume_unwind(payload),
        (Err(_), Err(error)) => panic!("could not remove {}: {error}", scratch.display()),
    }
}

fn packaged_checks(
    _project: &Path,
    apk: &Path,
    tools: &Path,
    scratch: &Path,
    passed: &mut Vec<String>,
) {
    let tree = Command::new(tools.join("aapt2"))
        .arg("dump")
        .arg("xmltree")
        .arg(apk)
        .arg("--file")
        .arg("AndroidManifest.xml")
        .output()
        .unwrap_or_else(|error| panic!("could not start aapt2: {error}"));
    check(
        passed,
        "the real linker reads the packaged application manifest",
        tree.status.success(),
    );
    let xml = String::from_utf8_lossy(&tree.stdout).into_owned();
    let at = xml.find("name(0x01010003)=\"com.local.client.TurnService\"");
    check(
        passed,
        "the packaged application declares the turn service that outlives focus",
        at.is_some(),
    );
    let block = at
        .map(|position| {
            xml[position..]
                .lines()
                .take(5)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let promoted = foreground_service_type(&block);
    check(
        passed,
        "the packaged turn service is promoted as dataSync",
        promoted.as_deref() == Some("00000001"),
    );
    check(
        passed,
        "the packaged permission set carries the dataSync foreground permission",
        xml.contains("android.permission.FOREGROUND_SERVICE_DATA_SYNC")
            && xml.matches("E: uses-permission").count() == 15,
    );
    check(
        passed,
        "the packaged service set keeps the three prior services beside the turn service",
        xml.matches("E: service ").count() == 4
            && ["com.local.client.VoiceSession", "com.local.client.Execution", "com.local.client.Gateway"]
                .iter()
                .all(|name| xml.contains(name)),
    );

    let names = archive_names(apk);
    check(
        passed,
        "the packaged application carries its dex payloads",
        !names.is_empty(),
    );

    let mut blob = Vec::new();
    let mut dump = String::new();
    for name in names {
        let payload = archive_read(apk, &name);
        blob.extend_from_slice(&payload);
        let path = scratch.join(&name);
        std::fs::write(&path, &payload)
            .unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
        let read_out = Command::new(tools.join("dexdump"))
            .arg("-f")
            .arg(&path)
            .output()
            .unwrap_or_else(|error| panic!("could not start dexdump: {error}"));
        check(passed, &format!("dexdump reads {name}"), read_out.status.success());
        dump.push_str(&String::from_utf8_lossy(&read_out.stdout));
    }

    let service_methods = methods(&dump, "TurnService");
    let required_service = [
        "begin",
        "finish",
        "post",
        "notificationsAllowed",
        "onStartCommand",
        "onTaskRemoved",
        "onDestroy",
        "extend",
        "publish",
        "progress",
        "openTicket",
        "leaseOf",
        "lost",
    ];
    check(
        passed,
        "the packaged dex carries the promotion surface including its lifetime callbacks",
        required_service
            .iter()
            .all(|name| service_methods.contains(*name)),
    );

    let activity_methods = methods(&dump, "Activity");
    let required_activity = [
        "turnSettled",
        "publishReply",
        "publishPendingReply",
        "turnLease",
        "turnProtectionLost",
        "askReplyNotifications",
        "refreshRateClock",
        "requestConversation",
        "openRequestedConversation",
        "startRateClock",
        "replyText",
    ];
    check(
        passed,
        "the packaged dex carries the turn-continuity surface on the activity",
        required_activity
            .iter()
            .all(|name| activity_methods.contains(*name)),
    );
    check(
        passed,
        "the wake-lock tag and reply conversation key survive into the packaged dex",
        contains_bytes(&blob, b"rllm:turn") && contains_bytes(&blob, b"rllm.conversation"),
    );
}

fn check(passed: &mut Vec<String>, name: &str, condition: bool) {
    if !condition {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn read(project: &Path, relative: &str) -> String {
    let path = project.join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

fn project_root() -> PathBuf {
    let manifest = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    if manifest.join("AndroidManifest.xml").is_file() {
        return manifest;
    }
    manifest
        .parent()
        .unwrap_or_else(|| panic!("project root has no parent"))
        .to_path_buf()
}

fn rate_clock_pattern(activity: &str) -> bool {
    let marker = "rateHandler.postDelayed(this,ratePaused?1000:500);";
    let mut base = 0usize;
    while let Some(offset) = activity[base..].find(marker) {
        let start = base + offset;
        let end = start + marker.len();
        let Some(newline) = activity[..start].rfind('\n') else {
            base = end;
            continue;
        };
        let leading = &activity[newline + 1..start];
        let closing = &activity[end..];
        let Some(second) = closing.find('\n') else {
            base = end;
            continue;
        };
        if leading.bytes().all(|byte| byte.is_ascii_whitespace())
            && closing[..second].is_empty()
            && closing[second + 1..].trim_start().starts_with("}};")
        {
            return true;
        }
        base = end;
    }
    false
}

fn foreground_service_type(block: &str) -> Option<String> {
    let start = block.find("foregroundServiceType(0x01010599)=0x")?;
    let tail = &block[start + "foregroundServiceType(0x01010599)=0x".len()..];
    let end = tail
        .find(|character: char| !character.is_ascii_hexdigit())
        .unwrap_or(tail.len());
    Some(tail[..end].to_string())
}

fn archive_names(apk: &Path) -> Vec<String> {
    twin::zip_names(apk)
        .into_iter()
        .filter(|name| name.starts_with("classes") && name.ends_with(".dex"))
        .collect()
}

fn archive_read(apk: &Path, name: &str) -> Vec<u8> {
    twin::zip_read(apk, name)
}

fn methods(dump: &str, class: &str) -> BTreeSet<String> {
    let mut found_names = BTreeSet::new();
    let class_name = format!("com/local/client/{class}");
    for section in dump.split("Class descriptor") {
        let head = section
            .trim_start()
            .strip_prefix(':')
            .unwrap_or("")
            .trim_start();
        let Some(rest) = head.strip_prefix('\'').and_then(|value| value.strip_prefix('L')) else {
            continue;
        };
        // dexdump closes a class descriptor with `;'`, never `';`.
        let Some(end) = rest
            .find(';')
            .filter(|at| rest.as_bytes().get(at + 1) == Some(&b'\''))
        else {
            continue;
        };
        // The leg's character class `[^';]+` stops at an apostrophe, so a descriptor
        // carrying one never matches there; a java identifier cannot hold one either,
        // so this side refuses it rather than accepting a name the leg would drop.
        if rest[..end].contains('\'') {
            continue;
        }
        if &rest[..end] != class_name {
            continue;
        }
        found_names.extend(method_names(section));
    }
    found_names
}

/// Mirrors the leg's `re.findall(r"name\s*:\s*'([^']+)'", section)`: a
/// non-overlapping left-to-right scan of the whole section — the token `name`
/// followed only by whitespace before the colon (`\s` may cross a newline),
/// then whitespace, then an apostrophe-delimited run that stops at the next
/// apostrophe. A bare `split_once("name")` would also match `name_and_type :`
/// and invent a method the leg never sees.
fn method_names(section: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut at = 0usize;
    while let Some(offset) = section[at..].find("name") {
        let start = at + offset;
        let after_name = skip_whitespace(section, start + "name".len());
        if section.as_bytes().get(after_name) != Some(&b':') {
            at = start + 1;
            continue;
        }
        let open = skip_whitespace(section, after_name + 1);
        if section.as_bytes().get(open) != Some(&b'\'') {
            at = start + 1;
            continue;
        }
        let open = open + "'".len();
        let Some(close) = section[open..].find('\'') else {
            at = start + 1;
            continue;
        };
        names.insert(section[open..open + close].to_string());
        at = open + close + "'".len();
    }
    names
}

fn skip_whitespace(text: &str, mut at: usize) -> usize {
    while let Some(character) = text[at..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        at += character.len_utf8();
    }
    at
}

fn contains_bytes(blob: &[u8], needle: &[u8]) -> bool {
    blob.windows(needle.len()).any(|window| window == needle)
}

fn subject(project: &Path, apk_argument: Option<&str>) -> PathBuf {
    if let Some(configured) = apk_argument {
        return absolute(Path::new(configured));
    }
    if let Some(configured) = std::env::var_os("RLLM_CHECK_APK") {
        return absolute(Path::new(&configured));
    }
    let manifest_text = read(project, "PUBLICATION-MANIFEST.json");
    let record = match parse_json(&manifest_text) {
        Ok(value) => value,
        _ => panic!("could not parse PUBLICATION-MANIFEST.json"),
    };
    let path = record
        .get("signed_apk")
        .and_then(|value| value.get("path"))
        .and_then(value_as_str)
        .unwrap_or_else(|| panic!("PUBLICATION-MANIFEST.json has no signed_apk.path"));
    let name = path.rsplit('/').next().unwrap_or(path);
    if record
        .get("signed_apk")
        .and_then(|value| value.get("location"))
        .and_then(value_as_str)
        != Some("companion")
    {
        return absolute(&project.join("release/RLLM.apk"));
    }
    let direct = project.join(name);
    if direct.exists() {
        return absolute(&direct);
    }
    let output = project.join("output");
    if let Ok(entries) = std::fs::read_dir(output) {
        let mut candidates = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join(name))
            .filter(|path| path.exists())
            .collect::<Vec<_>>();
        candidates.sort();
        if let Some(candidate) = candidates.into_iter().next() {
            return absolute(&candidate);
        }
    }
    panic!("refusing to invent an artifact: PUBLICATION-MANIFEST.json delivers {name} as a companion file that is not on this machine; set RLLM_CHECK_APK to the APK under test");
}

fn value_as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

/// Splits a JSON container into its contents and the text that follows its own
/// closing bracket. Slicing off the document's last character instead reads past
/// the end of any nested container that is not the final field, which is what
/// made the delivered record unparseable here.
fn json_container(text: &str) -> Option<(&str, &str)> {
    let opener = *text.as_bytes().first()?;
    if opener != b'{' && opener != b'[' {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in text.bytes().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&text[1..index], &text[index + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_json(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        let Some(body) = json_container(trimmed).map(|(body, _)| body) else {
            return Err("unterminated JSON object".to_string());
        };
        let mut fields = Vec::new();
        let mut rest = body;
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let (key, remainder) = parse_json_string(rest)?;
            rest = remainder.trim_start();
            let Some(after_colon) = rest.strip_prefix(':') else {
                return Err("expected ':' in JSON object".to_string());
            };
            let (value, remainder) = parse_json_value(after_colon.trim_start())?;
            fields.push((key, value));
            rest = remainder.trim_start();
            if let Some(after_comma) = rest.strip_prefix(',') {
                rest = after_comma;
                continue;
            }
            break;
        }
        Ok(Value::Obj(fields))
    } else if trimmed.starts_with('[') {
        let Some(body) = json_container(trimmed).map(|(body, _)| body) else {
            return Err("unterminated JSON array".to_string());
        };
        let mut items = Vec::new();
        let mut rest = body.trim_start();
        loop {
            if rest.is_empty() {
                break;
            }
            let (value, remainder) = parse_json_value(rest)?;
            items.push(value);
            rest = remainder.trim_start();
            if let Some(after_comma) = rest.strip_prefix(',') {
                rest = after_comma.trim_start();
                continue;
            }
            break;
        }
        Ok(Value::Arr(items))
    } else {
        Err("expected a JSON object or array".to_string())
    }
}

fn parse_json_string(text: &str) -> Result<(String, &str), String> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'"') {
        return Err("expected a JSON string".to_string());
    }
    let mut end = 1usize;
    let mut escaped = false;
    while end < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[end] == b'\\' {
            escaped = true;
        } else if bytes[end] == b'"' {
            let value = &text[1..end];
            return Ok((value.to_string(), &text[end + 1..]));
        }
        end += 1;
    }
    Err("unterminated JSON string".to_string())
}

fn parse_json_value(text: &str) -> Result<(Value, &str), String> {
    // The quote stays part of the input: parse_json_string is the scanner that
    // reads it, so stripping it here made every string value fail to parse.
    if text.starts_with('"') {
        let (value, rest) = parse_json_string(text)?;
        return Ok((Value::Str(value), rest));
    }
    if text.starts_with('{') {
        let Some((body, remainder)) = json_container(text) else {
            return Err("unterminated JSON object".to_string());
        };
        let mut fields = Vec::new();
        let mut rest = body.trim_start();
        loop {
            if rest.is_empty() {
                break;
            }
            let (key, remainder) = parse_json_string(rest)?;
            rest = remainder.trim_start();
            let Some(after_colon) = rest.strip_prefix(':') else {
                return Err("expected ':' in JSON object".to_string());
            };
            let (value, remainder) = parse_json_value(after_colon.trim_start())?;
            fields.push((key, value));
            rest = remainder.trim_start();
            if let Some(after_comma) = rest.strip_prefix(',') {
                rest = after_comma.trim_start();
                continue;
            }
            break;
        }
        return Ok((Value::Obj(fields), remainder));
    }
    if text.starts_with('[') {
        let Some((body, remainder)) = json_container(text) else {
            return Err("unterminated JSON array".to_string());
        };
        let mut items = Vec::new();
        let mut rest = body.trim_start();
        loop {
            if rest.is_empty() {
                break;
            }
            let (value, remainder) = parse_json_value(rest)?;
            items.push(value);
            rest = remainder.trim_start();
            if let Some(after_comma) = rest.strip_prefix(',') {
                rest = after_comma.trim_start();
                continue;
            }
            break;
        }
        return Ok((Value::Arr(items), remainder));
    }
    if let Some(rest) = text.strip_prefix("true") {
        return Ok((Value::Bool(true), rest));
    }
    if let Some(rest) = text.strip_prefix("false") {
        return Ok((Value::Bool(false), rest));
    }
    if let Some(rest) = text.strip_prefix("null") {
        return Ok((Value::Null, rest));
    }
    let end = text
        .find(|character: char| !character.is_ascii_digit() && character != '-' && character != '+')
        .unwrap_or(text.len());
    let token = &text[..end];
    let number = token
        .parse::<f64>()
        .map_err(|_| format!("invalid JSON number: {token}"))?;
    Ok((twin::n(&number.to_string()), &text[end..]))
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|error| panic!("cwd: {error}"))
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_container_ends_at_its_own_closer() {
        let record = parse_json(
            r#"{"signed_apk":{"path":"out/x.apk","location":"companion"},"files":[{"name":"a"},{"name":"b"}],"after":"kept"}"#
        )
        .expect("a nested record must parse");
        assert_eq!(
            record.get("after").and_then(value_as_str),
            Some("kept"),
            "a field that follows a nested container was swallowed: {}",
            record.to_string()
        );
        assert_eq!(
            record
                .get("signed_apk")
                .and_then(|value| value.get("location"))
                .and_then(value_as_str),
            Some("companion"),
            "the nested object's own fields"
        );
        let Value::Arr(items) = record.get("files").expect("files array") else {
            panic!("files is not an array");
        };
        assert_eq!(items.len(), 2, "the nested array lost an item");
    }

    #[test]
    fn the_delivered_record_parses_and_names_a_companion_apk() {
        let text = read(&project_root(), "PUBLICATION-MANIFEST.json");
        let record = parse_json(&text).unwrap_or_else(|error| panic!("the delivered record did not parse: {error}"));
        let signed = record.get("signed_apk").expect("signed_apk section");
        let path = signed.get("path").and_then(value_as_str).expect("signed_apk.path");
        assert!(path.ends_with(".apk"), "signed_apk.path does not name an APK: {path}");
        assert_eq!(
            signed.get("location").and_then(value_as_str),
            Some("companion"),
            "the record's delivery location for the signed APK"
        );
    }

    const TURN_SERVICE: &str = "Class descriptor  : 'Lcom/local/client/TurnService;'
  Direct methods  -
    #0              : (in Lcom/local/client/TurnService;)
      name          : 'begin'
      type          : '(J)V'
      access        : 0x0002 (PRIVATE)
    #1              : (in Lcom/local/client/TurnService;)
      name          : 'finish'
      type          : '()V'
      access        : 0x0002 (PRIVATE)
  Virtual methods  -
    #0              : (in Lcom/local/client/TurnService;)
      name          : 'notificationsAllowed'
      type          : '()Z'
      access        : 0x0001 (PUBLIC)
";

    const TWO_CLASSES: &str = "Class descriptor  : 'Lcom/local/client/TurnService;'
      name          : 'begin'
Class descriptor  : 'Lcom/local/client/Activity;'
      name          : 'turnSettled'
";

    fn expected(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn a_matching_class_descriptor_yields_its_method_names() {
        assert_eq!(
            methods(TURN_SERVICE, "TurnService"),
            expected(&["begin", "finish", "notificationsAllowed"])
        );
    }

    #[test]
    fn a_method_is_read_only_from_its_own_class_section() {
        assert_eq!(methods(TWO_CLASSES, "TurnService"), expected(&["begin"]));
        assert_eq!(methods(TWO_CLASSES, "Activity"), expected(&["turnSettled"]));
        assert!(methods(TWO_CLASSES, "Gateway").is_empty());
    }

    #[test]
    fn a_name_token_that_is_not_the_method_key_is_not_a_method() {
        // The leg's regex is `name\s*:\s*'`, so the value of a key named
        // `name_and_type` never enters the set the leg builds.
        let dump = format!("{TURN_SERVICE}      name_and_type     : 'invented'\n");
        assert_eq!(
            methods(&dump, "TurnService"),
            expected(&["begin", "finish", "notificationsAllowed"])
        );
    }

    #[test]
    fn a_class_descriptor_without_the_closing_quote_after_the_semicolon_is_skipped() {
        assert!(methods(
            "Class descriptor  : 'Lcom/local/client/TurnService'\n      name : 'begin'\n",
            "TurnService"
        )
        .is_empty());
        assert!(methods(
            "Class descriptor  : 'Lcom/local/client/TurnService';\n      name : 'begin'\n",
            "TurnService"
        )
        .is_empty());
        // The leg reads at most up to the first `;`, so a descriptor whose first
        // semicolon is not the closing one has no match at all.
        assert!(methods(
            "Class descriptor  : 'Lcom/local/client/TurnService;x;'\n      name : 'begin'\n",
            "TurnService"
        )
        .is_empty());
    }

    #[test]
    fn a_class_descriptor_holding_an_apostrophe_is_skipped_not_split() {
        // The leg's character class is `[^';]+`, so it stops at the apostrophe
        // and then fails to find `;'`; the whole section is dropped. The guard
        // is what keeps this twin from matching an apostrophe-bearing class
        // name that the leg never reads at all.
        assert!(methods(
            "Class descriptor  : 'Lcom/local/client/Turn'Service;'\n      name : 'begin'\n",
            "Turn'Service"
        )
        .is_empty());
    }

    #[test]
    fn every_match_on_one_line_is_collected() {
        assert_eq!(
            method_names("      name : 'a'   name : 'b'\n"),
            expected(&["a", "b"])
        );
    }

    #[test]
    fn the_whitespace_between_the_key_and_the_colon_may_cross_a_newline() {
        assert_eq!(method_names("      name\n          : 'a'\n"), expected(&["a"]));
    }

    #[test]
    fn an_empty_value_is_a_match_the_leg_makes() {
        assert_eq!(method_names("name : ''\n"), expected(&[""]));
    }

    #[test]
    fn a_value_ends_at_the_first_apostrophe() {
        assert_eq!(method_names("name : 'ab'cd'\n"), expected(&["ab"]));
    }

    #[test]
    fn an_unterminated_value_is_not_a_match() {
        assert!(method_names("name : 'abc\n").is_empty());
    }

    #[test]
    fn a_value_must_open_with_an_apostrophe() {
        // `\s*'` immediately follows the colon; a bare word before an apostrophe
        // elsewhere on the line is not a method value for the leg.
        assert!(method_names("name : it's here\n").is_empty());
    }

    #[test]
    fn the_scan_resumes_after_a_completed_match() {
        // `re.findall` is non-overlapping: the `name` inside the first value is
        // consumed by that value, so it cannot start a second match.
        assert_eq!(method_names("name : 'x name : 'y''\n"), expected(&["x name : "]));
    }

    #[test]
    fn a_name_token_at_the_end_of_the_input_neither_matches_nor_panics() {
        assert!(method_names("name").is_empty());
        assert!(method_names("name :").is_empty());
        assert!(method_names("name : ").is_empty());
        assert!(method_names("name : '").is_empty());
    }
}

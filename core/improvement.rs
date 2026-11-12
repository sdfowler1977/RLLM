//! Bounded policy feedback from product-owned verifiers.
use super::*;

const SEARCH_MARGIN: i8 = 3;
const SEARCH_LIMIT: i8 = 4;

pub(super) fn search_enabled(st: &Json) -> bool {
    get(get(st, "web"), "search_learning") != &Bool(false)
}

fn search_score(st: &Json) -> i8 {
    match get(st, "search_preference") {
        Num(value) => value.parse().ok().filter(|n: &i8| n.abs_diff(0) <= SEARCH_LIMIT as u8).unwrap_or(0),
        _ => 0,
    }
}

pub(super) fn search_order(st: &Json) -> usize {
    usize::from(search_enabled(st) && search_score(st) >= SEARCH_MARGIN)
}

/// A comparison is one completed fallback with validated records for the
/// original query. An individual result, empty page or failure earns no vote.
pub(super) fn compare_search(st: &mut Json, winner: usize) {
    let change = if winner == 0 { -1 } else { 1 };
    let score = (search_score(st) + change).clamp(-SEARCH_LIMIT, SEARCH_LIMIT);
    put(st, "search_preference", Num(score.to_string()));
}

pub(super) fn search_summary(st: &Json) -> String {
    let status = if !search_enabled(st) { "Paused" } else { "On" };
    let score = search_score(st);
    let first = if search_order(st) == 0 { "Mwmbl" } else { "DuckDuckGo" };
    format!("{status}. The automatic order starts with {first}. Same-query fallback balance: {score:+}. Only a fallback that returned validated results contributes. A saved search source keeps its selected order.")
}

/// User-owned acceptance criteria are captured before the first model call.
/// They remain local so the model cannot rewrite its own verifier.
pub(super) fn criteria(input: &Json) -> Result<Json> {
    let value = match input {
        Null => return Ok(Null),
        Str(value) if value.len() <= 4096 && !value.contains('\0') => value,
        _ => return Err("Response checks must be text and fit within 4 KiB".into()),
    };
    let lines: Vec<_> = value.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    if lines.len() > 8 || lines.iter().any(|line| line.len() > 512) {
        return Err("Enter up to eight required response phrases, each up to 512 bytes".into());
    }
    if lines.is_empty() { return Ok(Null); }
    let mut unique = Vec::new();
    for line in lines {
        let value = s(line);
        if !unique.contains(&value) { unique.push(value); }
    }
    Ok(Arr(unique))
}

pub(super) fn training_checks(inputs: &Json) -> Result<Json> {
    let checks = get(inputs, "response_checks");
    match get(inputs, "response_check_method") {
        Null => criteria(checks),
        Str(method) if method.is_empty() || method == "phrases" => criteria(checks),
        Str(method) if method == "json_fields" => {
            if checks == &Null || checks.text().is_ok_and(|value| value.trim().is_empty()) {
                Ok(Null)
            } else {
                json_field_checks(checks)
            }
        }
        _ => Err("Select a valid refinement check method".into()),
    }
}

// Capture the verifier once. The request builders only use its scores.
fn json_field_checks(input: &Json) -> Result<Json> {
    if let Ok(value) = input.text() {
        if value.len() <= 8192 {
            if let Ok(Obj(fields)) = parse(value) {
                if valid_json_fields(&fields) {
                    return Ok(obj(vec![("method", s("json_fields")), ("fields", Obj(fields))]));
                }
            }
        }
    }
    Err("Enter a JSON object up to 8 KiB with one to eight JSON Pointer keys and their expected values".into())
}

fn valid_json_fields(fields: &BTreeMap<String, Json>) -> bool {
    (1..=8).contains(&fields.len()) && fields.keys().all(|path| {
        path.len() <= 512 && !path.chars().any(char::is_control)
            && (path.is_empty() || path.starts_with('/'))
            && path.match_indices('~').all(|(index, _)| matches!(path.as_bytes().get(index + 1), Some(b'0' | b'1')))
    })
}

fn json_field<'a>(mut value: &'a Json, path: &str) -> Option<&'a Json> {
    let Some(path) = path.strip_prefix('/') else { return path.is_empty().then_some(value); };
    for part in path.split('/') {
        let key = part.replace("~1", "/").replace("~0", "~");
        value = match value {
            Obj(fields) => fields.get(&key)?,
            Arr(items) => {
                if key.is_empty() || (key.len() > 1 && key.starts_with('0'))
                    || !key.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
                items.get(key.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(value)
}

pub(super) fn refinement_rounds(input: &Json, checks: &Json) -> Result<usize> {
    match input {
        Null => Ok(1),
        Str(value) if value == "1" => Ok(1),
        Str(value) if value == "2" && checks != &Null => Ok(2),
        Str(value) if value == "2" => Err("Add response checks before selecting a second refinement round".into()),
        _ => Err("Select one or up to two refinement rounds".into()),
    }
}

pub(super) fn check_gain(baseline: &Json, candidate: &Json) -> bool {
    preserves_checks(baseline, candidate)
        && number(candidate,"passed")>number(baseline,"passed")
}

/// `floor` is the largest base-to-base spread already recorded for this task and verifier.
/// A floor of 0 is one observation: the legacy comparison is unchanged.
/// Equal scores are not a gain. A lower recorded cost can still admit them, and a higher
/// recorded cost refuses a score that did not rise. Missing cost leaves the legacy bit.
pub(super) fn acceptance(baseline: &Json, candidate: &Json, floor: u64, cost_before: Option<u64>, cost_after: Option<u64>) -> Json {
    let legacy = check_gain(baseline, candidate);
    let before = number(baseline, "passed") as i64;
    let after = number(candidate, "passed") as i64;
    let gain = after - before;
    let mut admit = legacy;
    let mut reason = "score";
    if floor > 0 && legacy && gain <= floor as i64 {
        admit = false;
        reason = "floor";
    }
    let mut cost_known = false;
    if let (Some(cost_before), Some(cost_after)) = (cost_before, cost_after) {
        cost_known = true;
        if after == before && cost_after < cost_before && preserves_checks(baseline, candidate) {
            admit = true;
            reason = "lower cost";
        } else if cost_after > cost_before && after <= before {
            admit = false;
            reason = "cost rose";
        }
    }
    obj(vec![
        ("admit", Bool(admit)),
        ("legacy", Bool(legacy)),
        ("reason", s(reason)),
        ("gain", s(&gain.to_string())),
        ("noise_floor", Num(floor.to_string())),
        ("cost_known", Bool(cost_known)),
    ])
}

pub(super) fn verifier_key(checks: &Json) -> String {
    if checks == &Null { return String::new(); }
    harness_rules::digest(checks)
}

pub(super) fn record_base(run: &mut Json, task: &str, verifier: &str, passed: usize) {
    if task.is_empty() || verifier.is_empty() { return; }
    let mut rows = array(run, "base_evaluations");
    rows.push(obj(vec![("task", s(task)), ("verifier", s(verifier)), ("passed", Num(passed.to_string()))]));
    put(run, "base_evaluations", Arr(rows));
}

pub(super) fn noise_floor(run: &Json, task: &str, verifier: &str) -> u64 {
    let scores: Vec<u64> = array(run, "base_evaluations").into_iter().filter(|row| {
        text(row, "task") == task && text(row, "verifier") == verifier
    }).map(|row| number(&row, "passed") as u64).collect();
    if scores.len() < 2 { return 0; }
    let mut spread = 0u64;
    for left in 0..scores.len() {
        for right in (left + 1)..scores.len() {
            spread = spread.max(scores[left].abs_diff(scores[right]));
        }
    }
    spread
}

pub(super) fn preserves_checks(baseline: &Json, candidate: &Json) -> bool {
    if !comparable(baseline, candidate) || number(candidate, "total") != number(baseline, "total") {
        return false;
    }
    let mask = |score: &Json| field_number(score, "passed_checks");
    match (mask(baseline), mask(candidate)) {
        (Some(before), Some(after)) => before & after == before,
        // Older saved observations recorded only a count.
        _ => number(candidate, "passed") >= number(baseline, "passed"),
    }
}

fn field_number(value: &Json, key: &str) -> Option<u8> {
    match get(value, key) {
        Num(number) => number.parse().ok(),
        _ => None,
    }
}

pub(super) fn json_envelope<'a>(criteria: &Json, reply: &'a str) -> Option<&'a str> {
    if !matches!(text(criteria, "method").as_str(), "json" | "json_fields") { return None; }
    let (open, remainder) = reply.trim().split_once('\n')?;
    let label = open.trim_end_matches(['\r', ' ', '\t']).strip_prefix("```")?;
    if !label.is_empty() && !label.eq_ignore_ascii_case("json") { return None; }
    let (body, close) = remainder.rsplit_once('\n')?;
    (close == "```").then_some(body)
}

fn structured_reply(criteria: &Json, reply: &str) -> Option<(Json, bool)> {
    if let Ok(value) = parse(reply) { return Some((value, false)); }
    let body = json_envelope(criteria, reply)?;
    let value = parse(body).ok()?;
    if harness::known_tool(&text(&value, "tool")) || harness_rules::is_report(body) { return None; }
    Some((value, true))
}

pub(super) fn evaluate(st: &Json, criteria: &Json, reply: &str) -> Json {
    let mut repaired = false;
    let (kind, outcomes) = if let Ok(lines) = criteria.array() {
        if lines.is_empty() || lines.len() > 8 { return Null; }
        let mut outcomes = Vec::with_capacity(lines.len());
        for line in lines {
            let Ok(line) = line.text() else { return Null; };
            if line.is_empty() || line.len() > 512 { return Null; }
            outcomes.push(reply.contains(line));
        }
        ("required phrases", outcomes)
    } else if text(criteria, "method") == "json_fields" {
        let Obj(fields) = get(criteria, "fields") else { return Null; };
        if !valid_json_fields(fields) { return Null; }
        let actual = structured_reply(criteria, reply).map(|(value, unwrapped)| {
            repaired = unwrapped;
            value
        });
        let outcomes = fields.iter().map(|(path, expected)|
            actual.as_ref().and_then(|value| json_field(value, path)) == Some(expected)).collect();
        ("JSON field checks", outcomes)
    } else {
        let Ok(expected) = field(criteria, "expected") else { return Null; };
        match text(criteria, "method").as_str() {
            "equals" if !expected.is_empty() && expected.len() <= 8192 => ("exact text checks", vec![reply == expected]),
            "sha256" if valid_digest(expected) => ("SHA-256 checks", vec![!reply.is_empty() && harness_rules::sha256(reply.as_bytes()).eq_ignore_ascii_case(expected)]),
            "json" if expected.len()<=8192 => {
                let Ok(expected)=parse(expected)else{return Null;};
                let passed = structured_reply(criteria, reply).is_some_and(|(actual, unwrapped)| {
                    repaired = unwrapped;
                    actual == expected
                });
                ("JSON value checks",vec![passed])
            },
            _ => return Null,
        }
    };
    let total = outcomes.len();
    let mask = outcomes.into_iter().enumerate().fold(0u8, |mask, (index, passed)|
        mask | (u8::from(passed) << index));
    let passed = mask.count_ones() as usize;
    let index = number(st, "request_index");
    let route = array(st, "profiles").get(index).map(|profile| {
        let scope = obj(vec![("index", Num(index.to_string())), ("profile", profile.clone()), ("model", get(st, "request_model").clone())]);
        s(&harness_rules::digest(&scope))
    }).unwrap_or(Null);
    let mut score = obj(vec![("passed", Num(passed.to_string())), ("total", Num(total.to_string())), ("strict", Bool(passed == total)), ("route", route), ("kind", s(kind)), ("passed_checks", Num(mask.to_string()))]);
    if repaired { put(&mut score, "json_unwrapped", Bool(true)); }
    score
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn evaluation_tasks(inputs: &Json, training_task: &str) -> Result<(Json, Json)> {
    let first = evaluation(inputs, training_task, "")?;
    let second = evaluation(inputs, training_task, "_2")?;
    if first == Null { return Ok((second, Null)); }
    if second != Null && text(&first, "task").split_whitespace().eq(text(&second, "task").split_whitespace()) {
        return Err("Enter different tasks for the two separate evaluations".into());
    }
    Ok((first, second))
}

fn evaluation(inputs: &Json, training_task: &str, suffix: &str) -> Result<Json> {
    let task_value = get(inputs, &format!("evaluation_task{suffix}"));
    let expected = get(inputs, &format!("evaluation_checks{suffix}"));
    let method = get(inputs, &format!("evaluation_method{suffix}"));
    if task_value != &Null && !matches!(task_value, Str(_)) {
        return Err("The separate evaluation task must be text".into());
    }
    let task = task_value.text().unwrap_or("");
    if task.trim().is_empty() && (expected == &Null || matches!(expected, Str(value) if value.trim().is_empty())) { return Ok(Null); }
    if task.trim().is_empty() || task.len() > 16384 || task.contains('\0') || task.split_whitespace().eq(training_task.split_whitespace()) {
        return Err("Enter a different evaluation task of up to 16 KiB".into());
    }
    if method != &Null && !matches!(method, Str(_)) {
        return Err("Select a valid evaluation check method".into());
    }
    let checks = match method.text().unwrap_or("") {
        ""|"phrases" => criteria(expected)?,
        "equals" => match expected {
            Str(value) if !value.is_empty() && value.len() <= 8192 && !value.contains('\0') => obj(vec![("method", s("equals")), ("expected", s(value))]),
            _ => return Err("Enter the exact expected reply of up to 8 KiB".into()),
        },
        "sha256" => match expected {
            Str(value) if valid_digest(value.trim()) => obj(vec![("method", s("sha256")), ("expected", s(value.trim()))]),
            _ => return Err("Enter the expected SHA-256 digest as 64 hexadecimal digits".into()),
        },
        "json" => match expected {
            Str(value) if value.len()<=8192 && parse(value).is_ok() => obj(vec![("method",s("json")),("expected",s(value))]),
            _ => return Err("Enter one valid JSON value of up to 8 KiB".into()),
        },
        "json_fields" => json_field_checks(expected)?,
        _ => return Err("Select a valid evaluation check method".into()),
    };
    if checks == Null { return Err("Add checks for the separate evaluation task".into()); }
    Ok(obj(vec![("task", s(task)), ("checks", checks)]))
}

pub(super) fn evaluating(st: &Json) -> bool {
    matches!(text(get(st,"task_run"),"phase").as_str(), "evaluation_base" | "evaluation_candidate")
}

pub(super) fn evaluation_context(st: &Json) -> Json {
    let mut scope = obj(["instructions", "harness_policy", "harness_project", "skills", "approval_root", "approval_root_identity",
        "storage_access", "phone_access_mode", "phone_access_granted", "phone_uri", "mcp", "drive", "service_connections"]
        .into_iter().map(|key| (key, get(st,key).clone())).collect());
    let agent=get(get(st,"task_run"),"agent");
    let tools=harness::available_tools(st,agent).into_iter().map(|name|s(&name)).collect();
    put(&mut scope,"tools",Arr(tools));
    s(&harness_rules::digest(&scope))
}

pub(super) fn refinement_summary(run: &Json) -> String {
    let mut rows = Vec::new();
    for (key, name) in [("baseline_score", "Baseline"), ("probe_score", "Candidate"), ("replay_score", "Revised instructions")] {
        let value = get(run, key);
        if value != &Null {
            let kind = field(value, "kind").unwrap_or("required phrases");
            rows.push(format!("{name} — {}/{} {kind}, {}{}", number(value, "passed"), number(value, "total"), if boolean(value, "strict") { "all checks passed" } else { "checks unmet" }, repair_note(value)));
        }
    }
    if rows.is_empty() { return "No user-defined response checks were supplied.".into(); }
    let first=get(get(run,"first_refinement"),"score");
    if first!=&Null {
        let kind = field(first, "kind").unwrap_or("required phrases");
        rows.push(format!("First round retained instructions — {}/{} {kind}{}",number(first,"passed"),number(first,"total"),repair_note(first)));
    }
    if let Ok(note)=field(run,"round_note"){rows.push(note.into());}
    if boolean(run, "retained_probe") { rows.push("The tested probe instructions were retained for review.".into()); }
    let (baseline, selected) = comparison(run);
    if selected != &Null && !comparable(baseline,selected) {
        rows.push("The model connection or refinement context changed. The outcomes are separate observations.".into());
    }
    rows.push("These comparisons measure replies against your configured checks. They do not establish factual correctness or general performance.".into());
    rows.join("\n")
}

pub(super) fn verified_improvement(run: &Json) -> bool {
    let (baseline, selected) = comparison(run);
    let training_gain = comparable(baseline, selected) && !boolean(baseline, "strict") && boolean(selected, "strict");
    if !boolean(run, "evaluation_requested") { return training_gain; }
    let scores = evaluation_scores(run);
    let (reference, _) = scores[0];
    get(run, "evaluation_error") == &Null && get(reference, "context") != &Null
        && scores.iter().all(|&(base, candidate)| comparable(reference, base)
            && comparable(base, candidate) && boolean(candidate, "strict"))
        && (baseline == &Null || (comparable(baseline, selected) && boolean(selected, "strict")))
        && (scores.iter().any(|(base, _)| !boolean(base, "strict"))
            || (training_gain && get(baseline, "route") == get(reference, "route")))
}

fn evaluation_scores(run: &Json) -> Vec<(&Json, &Json)> {
    let mut scores = Vec::with_capacity(2);
    let first = get(run, "evaluation_first");
    if first != &Null {
        scores.push((get(first, "original"), get(first, "retained")));
    }
    scores.push((get(run, "evaluation_base_score"), get(run, "evaluation_candidate_score")));
    scores
}

pub(super) fn comparable(base: &Json, candidate: &Json) -> bool {
    get(base,"route") != &Null && get(base,"route") == get(candidate,"route")
        && get(base,"context") == get(candidate,"context")
}

fn repair_note(score: &Json) -> &str {
    if boolean(score, "json_unwrapped") { " (JSON code block unwrapped locally)" } else { "" }
}

pub(super) fn evaluation_summary(run: &Json) -> String {
    if !boolean(run, "evaluation_requested") { return "No separate evaluation task was supplied.".into(); }
    let result = |score: &Json| if score == &Null { "Unavailable".into() } else {
        format!("{}/{} {}{}", number(score,"passed"), number(score,"total"), text(score,"kind"), repair_note(score))
    };
    let scores = evaluation_scores(run);
    let (reference, _) = scores[0];
    let mut summary = String::new();
    for (index, &(base, candidate)) in scores.iter().enumerate() {
        if scores.len() > 1 {
            if index > 0 { summary.push_str("\n\n"); }
            summary.push_str(&format!("Evaluation task {}\n", index + 1));
        }
        summary.push_str(&format!("Original instructions — {}\nRetained instructions — {}", result(base), result(candidate)));
    }
    if scores.iter().any(|&(base, candidate)| (base != &Null && !comparable(reference, base))
        || (candidate != &Null && !comparable(base, candidate))) {
        summary.push_str("\nThe model connection or evaluation context changed, so the outcomes are reported separately.");
    }
    if let Ok(reason) = field(run,"evaluation_error") { summary.push_str(&format!("\nEvaluation incomplete. {reason}")); }
    if verified_improvement(run) { summary.push_str("\nEligible for reuse after you save the profile."); }
    else { summary.push_str("\nThis candidate can be reviewed, with no measured gain entered into shared lessons."); }
    summary
}

fn comparison(run: &Json) -> (&Json, &Json) {
    let selected = if boolean(run, "retained_probe") { "probe_score" } else { "replay_score" };
    let retained = get(run,"selected_score");
    (get(run, "baseline_score"), if retained==&Null{get(run,selected)}else{retained})
}

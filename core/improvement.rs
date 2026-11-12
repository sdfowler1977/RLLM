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
        Str(value) if value == "3" && checks != &Null => Ok(3),
        Str(value) if value == "2" || value == "3" => Err("Add response checks before selecting a second refinement round".into()),
        _ => Err("Select one, two, or three refinement rounds".into()),
    }
}

pub(super) fn check_gain(baseline: &Json, candidate: &Json) -> bool {
    preserves_checks(baseline, candidate)
        && number(candidate,"passed")>number(baseline,"passed")
}

/// One passed check may pay for this many extra bytes of instruction text, and no more.
/// Cost is the UTF-8 length of that text. beta0 is 0, so a score that did not rise cannot
/// pay for any cost increase. That is the existing "cost rose" arm written as a bound.
pub(super) const COST_BETA0: i64 = 0;
pub(super) const COST_BETA1: i64 = 64;

/// The shape already stored on a ledger row: component, hypothesis, and diff digest.
#[derive(Clone, Copy)]
pub(super) struct CandidateShape<'a> {
    pub component: &'a str,
    pub hypothesis: &'a str,
    pub diff_digest: &'a str,
}

fn cost_limit(gain: i64) -> i64 {
    COST_BETA0.saturating_add(COST_BETA1.saturating_mul(gain))
}

fn signed_cost_delta(before: u64, after: u64) -> Option<i64> {
    let before = i64::try_from(before).ok()?;
    let after = i64::try_from(after).ok()?;
    after.checked_sub(before)
}

fn named_shape(shape: &CandidateShape) -> bool {
    !shape.component.is_empty() && !shape.hypothesis.is_empty() && shape.diff_digest.len() == 64
}

fn shape_recorded(records: &[Json], shape: &CandidateShape) -> bool {
    records.iter().any(|row| {
        text(row, "component") == shape.component
            && text(row, "hypothesis") == shape.hypothesis
            && text(row, "diff_digest") == shape.diff_digest
    })
}

/// `floor` is the largest base-to-base spread already recorded for this task and verifier.
/// A floor of 0 is one observation: the legacy comparison is unchanged.
/// Equal scores are not a gain. A lower recorded cost can still admit them, and a higher
/// recorded cost refuses a score that did not rise. Those two arms are the bound
/// cost change <= beta0 + beta1 * score gain when the gain is not positive.
/// A positive gain whose cost increase exceeds that bound is refused as "cost bound".
/// Missing cost leaves the legacy bit. Inside a measured band (floor > 0) a flat score
/// that is not a cost saving is admitted only when its shape is absent from `seen`.
pub(super) fn acceptance(
    baseline: &Json,
    candidate: &Json,
    floor: u64,
    cost_before: Option<u64>,
    cost_after: Option<u64>,
    shape: Option<CandidateShape>,
    seen: &[Json],
) -> Json {
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
        let over = match signed_cost_delta(cost_before, cost_after) {
            Some(delta) => delta > cost_limit(gain),
            None => true,
        };
        if after == before && cost_after < cost_before && preserves_checks(baseline, candidate) {
            admit = true;
            reason = "lower cost";
        } else if cost_after > cost_before && after <= before {
            admit = false;
            reason = "cost rose";
        } else if admit && reason == "score" && over {
            admit = false;
            reason = "cost bound";
        }
    }
    // A positive gain that does not clear the floor stays a floor refusal.
    // Novelty admits only a flat, check-preserving score inside that band.
    let flat = gain == 0 && preserves_checks(baseline, candidate);
    let in_band = floor > 0 && gain <= floor as i64;
    if in_band && flat && reason == "score" {
        if let Some(shape) = shape {
            if named_shape(&shape) {
                if shape_recorded(seen, &shape) {
                    admit = false;
                    reason = "duplicate shape";
                } else {
                    admit = true;
                    reason = "structural novelty";
                }
            }
        }
    }
    obj(vec![
        ("admit", Bool(admit)),
        ("legacy", Bool(legacy)),
        ("reason", s(reason)),
        ("gain", s(&gain.to_string())),
        ("noise_floor", Num(floor.to_string())),
        ("cost_known", Bool(cost_known)),
        ("beta0", Num(COST_BETA0.to_string())),
        ("beta1", Num(COST_BETA1.to_string())),
    ])
}

pub(super) fn verifier_key(checks: &Json) -> String {
    if checks == &Null { return String::new(); }
    harness_rules::digest(checks)
}

/// Digest of the task body. Base rows store this key, never the body.
pub(super) fn task_key(task: &str) -> String {
    if task.is_empty() { return String::new(); }
    harness_rules::digest(&s(task))
}

pub(super) fn record_base(run: &mut Json, task_key: &str, verifier: &str, passed: usize) {
    if task_key.is_empty() || verifier.is_empty() { return; }
    let mut rows = array(run, "base_evaluations");
    rows.push(obj(vec![("task_key", s(task_key)), ("verifier", s(verifier)), ("passed", Num(passed.to_string()))]));
    put(run, "base_evaluations", Arr(rows));
}

pub(super) fn noise_floor(run: &Json, task_key: &str, verifier: &str) -> u64 {
    let scores: Vec<u64> = array(run, "base_evaluations").into_iter().filter(|row| {
        text(row, "task_key") == task_key && text(row, "verifier") == verifier
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

fn push_secret(out: &mut Vec<String>, value: &str) {
    if value.is_empty() || out.iter().any(|item| item == value) {
        return;
    }
    out.push(value.to_string());
}

fn collect_expected(out: &mut Vec<String>, checks: &Json) {
    if let Arr(lines) = checks {
        for line in lines {
            if let Ok(line) = line.text() {
                push_secret(out, line);
            }
        }
        return;
    }
    match text(checks, "method").as_str() {
        "equals" | "sha256" | "json" => push_secret(out, &text(checks, "expected")),
        "json_fields" => {
            if let Obj(fields) = get(checks, "fields") {
                for value in fields.values() {
                    let rendered = match value {
                        Str(text) | Num(text) => text.clone(),
                        Bool(bit) => bit.to_string(),
                        other => other.string(),
                    };
                    push_secret(out, &rendered);
                }
            }
        }
        _ => {}
    }
}

/// Raw task strings the run still holds. The ledger stores digests, not these strings.
pub(super) fn recorded_tasks(run: &Json) -> Vec<String> {
    let mut tasks = Vec::new();
    push_secret(&mut tasks, &text(run, "task"));
    for key in ["evaluation", "evaluation_next"] {
        push_secret(&mut tasks, &text(get(run, key), "task"));
    }
    tasks
}

/// Raw expected check values the run still holds.
pub(super) fn recorded_expected(run: &Json) -> Vec<String> {
    let mut values = Vec::new();
    collect_expected(&mut values, get(run, "response_checks"));
    for key in ["evaluation", "evaluation_next"] {
        collect_expected(&mut values, get(get(run, key), "checks"));
    }
    values
}

fn copies_secret(body: &str, secret: &str) -> bool {
    !secret.is_empty()
        && (body.contains(secret)
            || behaviour_spec::refuse_subject_copy(body.as_bytes(), secret.as_bytes()).is_err())
}

/// Keep model prose. A hypothesis that carries a recorded task or expected value is stored as a digest.
pub(super) fn stored_hypothesis(hypothesis: &str, run: &Json) -> String {
    let leaked = recorded_tasks(run).iter().any(|task| copies_secret(hypothesis, task))
        || recorded_expected(run).iter().any(|value| copies_secret(hypothesis, value));
    if leaked {
        harness_rules::digest(&s(hypothesis))
    } else {
        hypothesis.to_string()
    }
}

/// Critic over the candidate harness text. It returns a verdict and does not score.
pub(super) fn leakage_critic(candidate: &str, component: &str, tasks: &[String], expected: &[String], unchanged: bool) -> Json {
    for task in tasks {
        if !task.is_empty() && candidate.contains(task) {
            return obj(vec![
                ("leakage", s("task_text")),
                ("reason", s("refused because the candidate contains a recorded task")),
            ]);
        }
    }
    for value in expected {
        if !value.is_empty() && candidate.contains(value) {
            return obj(vec![
                ("leakage", s("answer_text")),
                ("reason", s("refused because the candidate contains a recorded expected value")),
            ]);
        }
    }
    for value in expected {
        if behaviour_spec::refuse_subject_copy(candidate.as_bytes(), value.as_bytes()).is_err() {
            return obj(vec![
                ("leakage", s("answer_text")),
                ("reason", s("refused because the candidate reproduces a recorded expected value")),
            ]);
        }
    }
    if unchanged {
        return obj(vec![
            ("leakage", s("inert")),
            ("reason", s(&crate::rrsi::inert_refusal(component))),
        ]);
    }
    obj(vec![("leakage", s("clean")), ("reason", s(""))])
}

const SPLIT_CASE_MAX: usize = 8;

pub(super) struct SplitCase {
    pub task: String,
    pub checks: Json,
    pub reply: String,
}

fn leg_score(st: &Json, cases: &[SplitCase]) -> Result<Json> {
    if cases.is_empty() || cases.len() > SPLIT_CASE_MAX {
        return Err("a scored leg needs one to eight tasks".into());
    }
    let mut passed = 0usize;
    let mut shape = String::new();
    let mut task_keys = Vec::new();
    let mut verifiers = Vec::new();
    for case in cases {
        if case.task.is_empty() || case.task.contains('\0') || case.task.len() > 16384 {
            return Err("split task must be non-empty text within 16 KiB".into());
        }
        let score = evaluate(st, &case.checks, &case.reply);
        if score == Null {
            return Err("split checks could not be scored".into());
        }
        passed += number(&score, "passed");
        let kind = text(&score, "kind");
        if shape.is_empty() {
            shape = kind;
        } else if !shape.split(',').any(|part| part == kind) {
            shape.push(',');
            shape.push_str(&kind);
        }
        task_keys.push(task_key(&case.task));
        verifiers.push(verifier_key(&case.checks));
    }
    if passed > 1024 {
        return Err("split score exceeds the record cap".into());
    }
    let task_identity = if task_keys.len() == 1 {
        task_keys.remove(0)
    } else {
        harness_rules::digest(&s(&task_keys.join("\n")))
    };
    let verifier = if verifiers.len() == 1 {
        verifiers.remove(0)
    } else {
        harness_rules::digest(&s(&verifiers.join("\n")))
    };
    if verifier.is_empty() {
        return Err("split leg is missing its verifier digest".into());
    }
    Ok(obj(vec![
        ("passed", Num(passed.to_string())),
        ("task_key", s(&task_identity)),
        ("verifier", s(&verifier)),
        ("shape", s(&shape)),
    ]))
}

/// Score one harness on the evolve leg and, when supplied, the held-out and OOD legs.
/// The harness text is the same value for every leg. Task identity is stored as a digest.
pub(super) fn score_split(
    st: &Json,
    harness: &str,
    evolve: &[SplitCase],
    held_out: Option<&[SplitCase]>,
    ood: Option<&[SplitCase]>,
) -> Result<Json> {
    if harness.is_empty() || harness.contains('\0') || harness.len() > 32768 {
        return Err("harness must be non-empty text within 32 KiB".into());
    }
    let evolve_score = leg_score(st, evolve)?;
    let mut out = obj(vec![
        ("harness_digest", s(&harness_rules::digest(&s(harness)))),
        ("held_out_known", Bool(held_out.is_some())),
        ("ood_known", Bool(ood.is_some())),
        ("evolve_passed", get(&evolve_score, "passed").clone()),
        ("evolve_task_key", get(&evolve_score, "task_key").clone()),
        ("evolve_verifier", get(&evolve_score, "verifier").clone()),
        ("evolve_shape", get(&evolve_score, "shape").clone()),
    ]);
    if let Some(cases) = held_out {
        let score = leg_score(st, cases)?;
        put(&mut out, "held_out_passed", get(&score, "passed").clone());
        put(&mut out, "held_out_task_key", get(&score, "task_key").clone());
        put(&mut out, "held_out_verifier", get(&score, "verifier").clone());
    }
    if let Some(cases) = ood {
        let score = leg_score(st, cases)?;
        put(&mut out, "ood_passed", get(&score, "passed").clone());
        put(&mut out, "ood_task_key", get(&score, "task_key").clone());
        put(&mut out, "ood_verifier", get(&score, "verifier").clone());
        put(&mut out, "ood_shape", get(&score, "shape").clone());
    }
    Ok(out)
}

/// A missing held-out leg is unknown. An evolve gain without that leg is not reported as a gain.
/// A held-out score that moves the other way is named in the note.
pub(super) fn split_judgement(
    evolve_before: u64,
    evolve_after: u64,
    held_out: Option<(u64, u64)>,
    ood: Option<(u64, u64)>,
) -> Json {
    let mut notes = Vec::new();
    let mut reported = evolve_after > evolve_before;
    if evolve_after > evolve_before {
        match held_out {
            None => {
                reported = false;
                notes.push("gain unconfirmed because the held-out leg is unknown".to_string());
            }
            Some((before, after)) if after < before => {
                reported = false;
                notes.push(format!(
                    "held-out score moved from {before} to {after} while the evolve score moved from {evolve_before} to {evolve_after}"
                ));
            }
            Some((before, after)) => notes.push(format!(
                "evolve score moved from {evolve_before} to {evolve_after} and held-out score moved from {before} to {after}"
            )),
        }
        if let Some((before, after)) = ood {
            if after < before {
                reported = false;
                notes.push(format!("ood score moved from {before} to {after}"));
            }
        }
    }
    let note = notes.join("; ");
    if note.len() > 240 {
        return obj(vec![
            ("held_out_known", Bool(held_out.is_some())),
            ("ood_known", Bool(ood.is_some())),
            ("reported_gain", Bool(false)),
            ("note", s("gain unconfirmed because the split note exceeded its cap")),
        ]);
    }
    obj(vec![
        ("held_out_known", Bool(held_out.is_some())),
        ("ood_known", Bool(ood.is_some())),
        ("reported_gain", Bool(reported)),
        ("note", s(&note)),
    ])
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn critic_refuses_a_recorded_task_before_any_score() {
        let task = "UNIQUE_TASK_BODY_for_the_critic".to_string();
        let verdict = leakage_critic(&format!("Follow {task} exactly."), "skills", &[task.clone()], &[], false);
        assert_eq!(text(&verdict, "leakage"), "task_text");
        assert_eq!(get(&verdict, "score_before"), &Null);
        assert_eq!(get(&verdict, "score_after"), &Null);
        assert_eq!(get(&verdict, "passed"), &Null);
        assert!(!verdict.string().contains(&task), "{}", verdict.string());
    }

    #[test]
    fn critic_refuses_seventeen_bytes_of_a_recorded_expected_value() {
        let expected = "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345".to_string();
        let seventeen = &expected[..behaviour_spec::QUOTED_FRAGMENT_MAX + 1];
        assert_eq!(seventeen.len(), 17);
        let candidate = format!("match the fixture {seventeen} then stop");
        assert!(!candidate.contains(&expected));
        let verdict = leakage_critic(&candidate, "skills", &[], &[expected.clone()], false);
        assert_eq!(text(&verdict, "leakage"), "answer_text");
        assert!(text(&verdict, "reason").contains("expected value"), "{}", text(&verdict, "reason"));
        assert_eq!(get(&verdict, "score_before"), &Null);
        let sixteen = &expected[..behaviour_spec::QUOTED_FRAGMENT_MAX];
        let allowed = format!("match the fixture {sixteen} then stop");
        let clean = leakage_critic(&allowed, "skills", &[], &[expected], false);
        assert_eq!(text(&clean, "leakage"), "clean", "{}", text(&clean, "reason"));
    }

    #[test]
    fn critic_marks_an_unchanged_component_inert() {
        let verdict = leakage_critic("same body", "skills", &[], &[], true);
        assert_eq!(text(&verdict, "leakage"), "inert");
        assert_eq!(
            text(&verdict, "reason"),
            "refused because the candidate adds no change to component skills"
        );
        assert_eq!(get(&verdict, "score_before"), &Null);
        assert_eq!(get(&verdict, "score_after"), &Null);
    }

    #[test]
    fn legacy_acceptance_is_unchanged_without_history() {
        let score = |passed: &str| {
            obj(vec![
                ("passed", Num(passed.into())),
                ("total", Num("2".into())),
                ("route", s("same")),
                ("context", s("same")),
            ])
        };
        let before = score("1");
        let after = score("2");
        let decision = acceptance(&before, &after, 0, None, None, None, &[]);
        assert_eq!(boolean(&decision, "admit"), check_gain(&before, &after));
        assert_eq!(boolean(&decision, "admit"), true);
        assert_eq!(boolean(&decision, "legacy"), true);
        assert_eq!(text(&decision, "reason"), "score");
        assert_eq!(text(&decision, "gain"), "1");
        assert_eq!(get(&decision, "noise_floor"), &Num("0".into()));
        assert_eq!(boolean(&decision, "cost_known"), false);
        let equal = acceptance(&before, &before, 0, None, None, None, &[]);
        assert_eq!(boolean(&equal, "admit"), false);
        assert_eq!(boolean(&equal, "admit"), check_gain(&before, &before));
        assert_eq!(boolean(&equal, "legacy"), boolean(&equal, "admit"));
        assert_eq!(text(&equal, "reason"), "score");
        assert_eq!(boolean(&equal, "cost_known"), false);
    }

    #[test]
    fn unchanged_harness_records_held_out_and_ood_separately() {
        let st = obj(vec![]);
        let harness = "HARNESS-UNCHANGED";
        let evolve_task = "EVOLVE_TASK_BODY_not_stored";
        let held_task = "HELD_TASK_BODY_not_stored";
        let ood_task = "OOD_TASK_BODY_not_stored";
        let evolve = [SplitCase {
            task: evolve_task.into(),
            checks: Arr(vec![s("evolve-token")]),
            reply: "evolve-token is present".into(),
        }];
        let ood = [SplitCase {
            task: ood_task.into(),
            checks: obj(vec![("method", s("equals")), ("expected", s("exact-ood-reply"))]),
            reply: "exact-ood-reply".into(),
        }];
        let missing = score_split(&st, harness, &evolve, None, Some(&ood)).unwrap();
        assert_eq!(boolean(&missing, "held_out_known"), false);
        assert_eq!(get(&missing, "held_out_passed"), &Null);
        assert_eq!(boolean(&missing, "ood_known"), true);
        assert_eq!(number(&missing, "evolve_passed"), 1);
        assert_eq!(number(&missing, "ood_passed"), 1);
        assert_ne!(text(&missing, "evolve_shape"), text(&missing, "ood_shape"));
        assert_eq!(text(&missing, "harness_digest"), harness_rules::digest(&s(harness)));
        assert_eq!(text(&missing, "evolve_task_key"), task_key(evolve_task));
        assert_eq!(text(&missing, "ood_task_key"), task_key(ood_task));
        let body = missing.string();
        assert!(!body.contains(evolve_task) && !body.contains(ood_task) && !body.contains("exact-ood-reply"), "{body}");
        let held = [SplitCase {
            task: held_task.into(),
            checks: Arr(vec![s("held-token")]),
            reply: "no held token here".into(),
        }];
        let with_held = score_split(&st, harness, &evolve, Some(&held), Some(&ood)).unwrap();
        assert_eq!(text(&with_held, "harness_digest"), text(&missing, "harness_digest"));
        assert_eq!(boolean(&with_held, "held_out_known"), true);
        assert_eq!(number(&with_held, "held_out_passed"), 0);
        assert_eq!(text(&with_held, "held_out_task_key"), task_key(held_task));
        assert!(!with_held.string().contains(held_task));
        let absent = split_judgement(0, 2, None, None);
        assert_eq!(boolean(&absent, "reported_gain"), false);
        assert_eq!(boolean(&absent, "held_out_known"), false);
        assert!(text(&absent, "note").contains("gain unconfirmed"));
        assert!(text(&absent, "note").contains("held-out leg is unknown"));
        let opposite = split_judgement(0, 2, Some((5, 1)), Some((4, 0)));
        assert_eq!(boolean(&opposite, "reported_gain"), false);
        assert!(text(&opposite, "note").contains("held-out score moved from 5 to 1"), "{}", text(&opposite, "note"));
        assert!(text(&opposite, "note").contains("evolve score moved from 0 to 2"), "{}", text(&opposite, "note"));
        assert!(text(&opposite, "note").contains("ood score moved from 4 to 0"), "{}", text(&opposite, "note"));
    }

    #[test]
    fn three_refinement_rounds_are_accepted_and_four_are_refused() {
        let checks = Arr(vec![s("phrase")]);
        assert_eq!(refinement_rounds(&s("1"), &checks).unwrap(), 1);
        assert_eq!(refinement_rounds(&s("2"), &checks).unwrap(), 2);
        assert_eq!(refinement_rounds(&s("3"), &checks).unwrap(), 3);
        assert_eq!(refinement_rounds(&Null, &checks).unwrap(), 1);
        assert_eq!(
            refinement_rounds(&s("2"), &Null).unwrap_err(),
            "Add response checks before selecting a second refinement round"
        );
        assert_eq!(
            refinement_rounds(&s("3"), &Null).unwrap_err(),
            "Add response checks before selecting a second refinement round"
        );
        assert_eq!(
            refinement_rounds(&s("4"), &checks).unwrap_err(),
            "Select one, two, or three refinement rounds"
        );
    }

    fn paired(passed: &str) -> Json {
        obj(vec![
            ("passed", Num(passed.into())),
            ("total", Num("4".into())),
            ("route", s("same")),
            ("context", s("same")),
        ])
    }

    #[test]
    fn cost_bound_refuses_a_rise_the_score_gain_does_not_pay_for() {
        assert_eq!(COST_BETA0, 0);
        assert_eq!(COST_BETA1, 64);
        let before = paired("1");
        let after = paired("2");
        let gain = 1i64;
        let limit = COST_BETA0 + COST_BETA1 * gain;
        assert_eq!(limit, 64);
        let over = acceptance(&before, &after, 0, Some(1000), Some(1000 + limit as u64 + 1), None, &[]);
        assert!(!boolean(&over, "admit"));
        assert_eq!(text(&over, "reason"), "cost bound");
        assert_eq!(get(&over, "beta0"), &Num("0".into()));
        assert_eq!(get(&over, "beta1"), &Num("64".into()));
        assert_eq!(text(&over, "gain"), "1");
        let inside = acceptance(&before, &after, 0, Some(1000), Some(1000 + limit as u64), None, &[]);
        assert!(boolean(&inside, "admit"));
        assert_eq!(text(&inside, "reason"), "score");
        assert_eq!(get(&inside, "beta0"), &Num("0".into()));
        assert_eq!(get(&inside, "beta1"), &Num("64".into()));
        let tied = paired("2");
        let saved = acceptance(&tied, &tied, 0, Some(50), Some(40), None, &[]);
        assert_eq!(text(&saved, "reason"), "lower cost");
        assert!(boolean(&saved, "admit"));
        assert!(40 - 50 <= COST_BETA0 + COST_BETA1 * 0);
        let costly = acceptance(&tied, &tied, 0, Some(40), Some(50), None, &[]);
        assert_eq!(text(&costly, "reason"), "cost rose");
        assert!(!boolean(&costly, "admit"));
        assert!(50 - 40 > COST_BETA0 + COST_BETA1 * 0);
    }

    #[test]
    fn noise_band_admits_a_new_shape_and_refuses_the_duplicate() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let other = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        let score = paired("2");
        let shape = CandidateShape {
            component: "skills",
            hypothesis: "try a cache",
            diff_digest: digest,
        };
        let first = acceptance(&score, &score, 3, None, None, Some(shape), &[]);
        assert!(boolean(&first, "admit"));
        assert_eq!(text(&first, "reason"), "structural novelty");
        assert_eq!(text(&first, "gain"), "0");
        let seen = obj(vec![
            ("component", s("skills")),
            ("hypothesis", s("try a cache")),
            ("diff_digest", s(digest)),
        ]);
        let second = acceptance(&score, &score, 3, None, None, Some(shape), &[seen]);
        assert!(!boolean(&second, "admit"));
        assert_eq!(text(&second, "reason"), "duplicate shape");
        let novel = CandidateShape {
            component: "skills",
            hypothesis: "try an index",
            diff_digest: other,
        };
        let third = acceptance(&score, &score, 3, None, None, Some(novel), &[obj(vec![
            ("component", s("skills")),
            ("hypothesis", s("try a cache")),
            ("diff_digest", s(digest)),
        ])]);
        assert!(boolean(&third, "admit"));
        assert_eq!(text(&third, "reason"), "structural novelty");
        let unmeasured = acceptance(&score, &score, 0, None, None, Some(novel), &[]);
        assert!(!boolean(&unmeasured, "admit"));
        assert_eq!(text(&unmeasured, "reason"), "score");
        let noisy = acceptance(&paired("1"), &paired("2"), 3, None, None, Some(novel), &[]);
        assert!(!boolean(&noisy, "admit"));
        assert_eq!(text(&noisy, "reason"), "floor");
    }

    #[test]
    fn old_ledger_row_without_beta_fields_still_seals() {
        let run = "abababababababababababababababababababababababababababababababab";
        let legacy = crate::rrsi::edit(
            "skills",
            "cache the listing",
            &crate::rrsi::diff_digest("old", "new"),
            1,
            1,
            4,
            4,
            "rejected",
            2,
            run,
        )
        .unwrap();
        assert_eq!(get(&legacy, "beta0"), &Null);
        assert_eq!(get(&legacy, "beta1"), &Null);
        assert_eq!(get(&legacy, "accept_reason"), &Null);
        let saved = crate::rrsi::append_document("", &legacy, run).unwrap();
        let records = crate::rrsi::records_of(&text(&saved, "document")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(crate::rrsi::prior_rejection(&records, run, "skills", "cache the listing"), Some(2));
        assert!(crate::rrsi::incumbent_cost(&records, run).is_none());
        assert!(crate::rrsi::pruned_components(&records, run).is_empty());
        let mut flat = String::new();
        for round in 1..=crate::rrsi::PRUNE_WINDOW as u64 {
            let row = crate::rrsi::edit(
                "skills",
                &format!("flat {round}"),
                &crate::rrsi::diff_digest("a", "b"),
                2,
                2,
                4,
                4,
                "rejected",
                round,
                run,
            )
            .unwrap();
            assert_eq!(get(&row, "accept_reason"), &Null);
            flat = text(&crate::rrsi::append_document(&flat, &row, run).unwrap(), "document");
        }
        let flat_records = crate::rrsi::records_of(&flat).unwrap();
        assert_eq!(crate::rrsi::pruned_components(&flat_records, run), vec!["skills".to_string()]);
        assert_eq!(crate::rrsi::prior_rejection(&flat_records, run, "skills", "flat 1"), Some(1));
        assert!(crate::rrsi::incumbent_cost(&flat_records, run).is_none());
        let mut known = crate::rrsi::edit(
            "prompt",
            "kept",
            &crate::rrsi::diff_digest("a", "b"),
            1,
            2,
            4,
            11,
            "accepted",
            1,
            run,
        )
        .unwrap();
        put(&mut known, "cost_known", Bool(true));
        let measured = text(&crate::rrsi::append_document("", &known, run).unwrap(), "document");
        let measured_records = crate::rrsi::records_of(&measured).unwrap();
        assert_eq!(crate::rrsi::incumbent_cost(&measured_records, run), Some(11));
        assert_eq!(get(&measured_records[0], "beta0"), &Null);
        let mut annotated = known.clone();
        put(&mut annotated, "round", Num("2".into()));
        put(&mut annotated, "beta0", Num(COST_BETA0.to_string()));
        put(&mut annotated, "beta1", Num(COST_BETA1.to_string()));
        put(&mut annotated, "accept_reason", s("cost bound"));
        let both = text(&crate::rrsi::append_document(&measured, &annotated, run).unwrap(), "document");
        let both_records = crate::rrsi::records_of(&both).unwrap();
        assert_eq!(get(&both_records[0], "beta0"), &Null);
        assert_eq!(get(&both_records[0], "accept_reason"), &Null);
        assert_eq!(get(&both_records[1], "beta0"), &Num("0".into()));
        assert_eq!(get(&both_records[1], "beta1"), &Num("64".into()));
        assert_eq!(text(&both_records[1], "accept_reason"), "cost bound");
        assert_eq!(crate::rrsi::incumbent_cost(&both_records, run), Some(11));
        assert_eq!(crate::rrsi::prior_rejection(&both_records, run, "prompt", "kept"), None);
        put(&mut annotated, "accept_reason", s("structural novelty"));
        put(&mut annotated, "round", Num("3".into()));
        let novelty = text(&crate::rrsi::append_document(&both, &annotated, run).unwrap(), "document");
        assert_eq!(text(&crate::rrsi::records_of(&novelty).unwrap()[2], "accept_reason"), "structural novelty");
        put(&mut annotated, "accept_reason", s("duplicate shape"));
        put(&mut annotated, "round", Num("4".into()));
        let duplicate = text(&crate::rrsi::append_document(&novelty, &annotated, run).unwrap(), "document");
        let duplicate_records = crate::rrsi::records_of(&duplicate).unwrap();
        assert_eq!(text(&duplicate_records[3], "accept_reason"), "duplicate shape");
        assert_eq!(crate::rrsi::incumbent_cost(&duplicate_records, run), Some(11));
        assert!(crate::rrsi::pruned_components(&duplicate_records, run).is_empty());
    }
}

//! Per-project Loop, Goal and medical check. Saved with the harness project record.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn slash<'a>(draft: &'a str, name: &str) -> Option<&'a str> {
    let rest = draft.trim().strip_prefix(name)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) { Some(rest.trim()) } else { None }
}

/// `3`, `3m`, `3 min`, `3 minutes` are minutes. `90s` becomes minutes, rounded up.
fn minutes_of(text: &str) -> Option<(u64, &str)> {
    let text = text.trim_start();
    let digits = text.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 { return None; }
    let count: u64 = text[..digits].parse().ok()?;
    if count == 0 { return None; }
    let after = text[digits..].trim_start();
    for (unit, seconds) in [("minutes", false), ("minute", false), ("min", false), ("m", false), ("seconds", true), ("second", true), ("s", true)] {
        if let Some(rest) = after.strip_prefix(unit) {
            if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) { continue; }
            let minutes = if seconds { count.div_ceil(60).max(1) } else { count };
            if minutes > 10080 { return None; }
            return Some((minutes, rest.trim()));
        }
    }
    if text[digits..].is_empty() || text[digits..].starts_with(char::is_whitespace) {
        if count > 10080 { return None; }
        return Some((count, after));
    }
    None
}

pub fn due(st: &Json) -> bool {
    let spec = harness_rules::project_feature(st, "loop");
    if !boolean(&spec, "enabled") || text(&spec, "instructions").trim().is_empty() { return false; }
    if get(st, "task_run") != &Null || get(st, "pending") != &Null || boolean(st, "busy") { return false; }
    if text(st, "active").is_empty() { return false; }
    let minutes = text(&spec, "minutes").parse::<u64>().ok().filter(|n| (1..=10080).contains(n)).unwrap_or(3);
    let last = text(st, "loop_last").parse::<u64>().unwrap_or(0);
    unix_secs().saturating_sub(last) >= minutes.saturating_mul(60)
}

pub(super) fn prompt(st: &Json) -> String {
    format!("Loop:\n{}", text(&harness_rules::project_feature(st, "loop"), "instructions").trim())
}

/// `/loop` and `/goal` from the composer. A locked project keeps the saved text.
pub(super) fn consume_slash(st: &mut Json, draft: &str) -> Result<bool> {
    let (kind, rest) = if let Some(rest) = slash(draft, "/loop") { ("loop", rest) }
        else if let Some(rest) = slash(draft, "/goal") { ("goal", rest) }
        else { return Ok(false); };
    if text(st, "approval_root").is_empty() {
        return Err("Open a project workspace first. Loop and Goal are saved on the project.".into());
    }
    let current = harness_rules::project_feature(st, kind);
    if boolean(&current, "locked") {
        put(st, "notice", s(&format!("{kind} is locked for this project. Change it under Harness rules.")));
        return Ok(true);
    }
    let mut next = current;
    if next == Null { next = obj(vec![]); }
    put(&mut next, "enabled", Bool(true));
    if kind == "loop" {
        if let Some((minutes, instructions)) = minutes_of(rest) {
            put(&mut next, "minutes", Num(minutes.to_string()));
            if !instructions.is_empty() { put(&mut next, "instructions", s(instructions)); }
            put(st, "loop_last", Num(unix_secs().to_string()));
            put(st, "notice", s(&format!("Loop saved for this project, every {minutes} minutes. The first run waits that long. Lock it under Harness rules to keep this text.")));
        } else if !rest.is_empty() {
            put(&mut next, "instructions", s(rest));
            put(st, "loop_last", Num(unix_secs().to_string()));
            let minutes = text(&next, "minutes").parse::<u64>().ok().filter(|n| (1..=10080).contains(n)).unwrap_or(3);
            put(&mut next, "minutes", Num(minutes.to_string()));
            put(st, "notice", s(&format!("Loop instructions saved. It runs every {minutes} minutes. Lock it under Harness rules to keep this text.")));
        } else {
            let minutes = text(&next, "minutes").parse::<u64>().ok().filter(|n| (1..=10080).contains(n)).unwrap_or(3);
            put(&mut next, "minutes", Num(minutes.to_string()));
            put(st, "loop_last", Num(unix_secs().to_string()));
            put(st, "notice", s(&format!("Loop enabled, every {minutes} minutes, using the saved instructions.")));
        }
    } else if !rest.is_empty() {
        put(&mut next, "objective", s(rest));
        put(st, "notice", s("Goal saved for this project and turned on. Lock it under Harness rules to keep this text."));
    } else {
        put(st, "notice", s("Goal enabled for this project, using the saved objective."));
    }
    harness_rules::store_project_feature(st, kind, next)?;
    Ok(true)
}

/// After a normal answer, ask the project's medical-check connection. One check, no tools.
pub(super) fn start_verify(st: &mut Json, answer: &str) -> Result<Option<Json>> {
    let spec = harness_rules::project_feature(st, "verify");
    if !boolean(&spec, "enabled") { return Ok(None); }
    let index = text(&spec, "connection").parse::<usize>().unwrap_or(usize::MAX);
    let Some(profile) = array(st, "profiles").get(index).cloned() else {
        put(st, "notice", s("The answer is saved. Medical check is on, but its connection is missing."));
        return Ok(None);
    };
    if !super::provider::allows(&profile, "subagent") {
        put(st, "notice", s("The answer is saved. Medical check needs a connection that is allowed as a sub-agent."));
        return Ok(None);
    }
    let question = array(&chat(st), "messages").iter().rev().find(|m| text(m, "role") == "user").map(|m| text(m, "content")).unwrap_or_default();
    if question.starts_with("Loop:\n") || question.starts_with("Parallel sub-agent") { return Ok(None); }
    let clip = |v: &str| {
        let end = v.floor_char_boundary(v.len().min(24000));
        v[..end].to_string()
    };
    let task = format!("The user asked:\n{}\n\nA model answered:\n{}\n\nFirst line: AGREE or DISAGREE. Then the correction in plain sentences.", clip(&question), clip(answer));
    let mut agent = harness::active_agent(st);
    put(&mut agent, "tools", s(""));
    put(st, "consult_plain", Bool(true));
    put(st, "task_run", obj(vec![
        ("mode", s("consult")),
        ("phase", s("verify_answer")),
        ("task", s(&task)),
        ("agent", agent),
        ("chat", get(st, "active").clone()),
        ("connection", Num(index.to_string())),
    ]));
    let effect = model_effect_at(st, index)?;
    put(st, "consult_plain", Bool(false));
    put(st, "notice", s(&format!("Answer saved. Checking it with {}…", text(&profile, "model"))));
    Ok(Some(effect))
}

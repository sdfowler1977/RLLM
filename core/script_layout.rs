//! Break one packed script line after `{`, `}`, and `;`.
//! A packed line longer than 4096 characters keeps that many and names the rest.
//! Packed evaluation is not expanded.

const PACKED_MIN: usize = 40;
const BODY_LIMIT: usize = 4096;
const NOTICE: &str = "The script was reformatted. Packed evaluation was not expanded.";

pub fn script_layout(source: &str) -> String {
    let (body, unread) = if source.contains('\n') || source.chars().nth(PACKED_MIN).is_none() {
        (source.to_string(), false)
    } else {
        break_packed(source)
    };
    with_notice(&body, unread)
}

fn break_packed(source: &str) -> (String, bool) {
    let mut body = String::new();
    let mut count = 0usize;
    let mut taken = 0usize;
    for c in source.chars() {
        if count >= BODY_LIMIT {
            break;
        }
        body.push(c);
        count += 1;
        taken += 1;
        if count < BODY_LIMIT && matches!(c, '{' | '}' | ';') {
            body.push('\n');
            count += 1;
        }
    }
    (body, source.chars().nth(taken).is_some())
}

fn with_notice(body: &str, unread: bool) -> String {
    let mut out = String::with_capacity(body.len() + NOTICE.len() + 48);
    out.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
    if unread {
        out.push_str("later characters were not listed\n");
    }
    out.push_str(NOTICE);
    out
}

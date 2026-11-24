//! Host and path lines from saved session text.
//! The scan requires the entries marker and reads literal `"url": "..."` values.
//! It keeps 16 http and https hosts. A further host is named as omitted.
//! The listed host count is calculated from those hosts.
//! It is not a document parser, names are not resolved, and the session is not replayed.
#![forbid(unsafe_code)]

const HOST_LIMIT: usize = 16;
const ENTRIES: &str = "\"entries\"";
const URL_PREFIX: &str = "\"url\": \"";
const REFUSED: &str = "A saved session was not identified.";
const LATER: &str = "later hosts were not listed";
const CLOSING: &str = "The session was not replayed.";

/// List host and path lines from saved session text.
///
/// Literal `"url": "..."` values are read when `"entries"` is present.
/// At most 16 http and https hosts are listed. A further host is not listed.
/// The report names how many hosts were listed, then ends with `The session was not replayed.`
/// Text without the entries marker is refused.
pub fn session_env(bytes: &[u8]) -> String {
    let Some(text) = std::str::from_utf8(bytes).ok() else {
        return REFUSED.to_string();
    };
    if !text.contains(ENTRIES) {
        return REFUSED.to_string();
    }
    let (places, unread) = collect(text);
    if places.is_empty() {
        return REFUSED.to_string();
    }
    let mut report = String::new();
    for place in &places {
        report.push_str(&format!("host {}\npath {}\n", place.host, place.path));
    }
    if unread {
        report.push_str(LATER);
        report.push('\n');
    }
    report.push_str(&format!("hosts {}\n", places.len()));
    report.push_str(CLOSING);
    report.push('\n');
    report
}

struct HostPath {
    host: String,
    path: String,
}

fn collect(text: &str) -> (Vec<HostPath>, bool) {
    let mut places = Vec::new();
    let mut from = 0usize;
    let mut unread = false;
    while from < text.len() {
        let Some(rel) = text[from..].find(URL_PREFIX) else {
            break;
        };
        let start = from + rel + URL_PREFIX.len();
        let tail = &text[start..];
        let Some(end) = tail.find('"') else {
            break;
        };
        let candidate = &tail[..end];
        from = start + end + 1;
        if candidate.chars().any(char::is_whitespace) {
            continue;
        }
        let Some(place) = host_path(candidate) else {
            continue;
        };
        if places.len() == HOST_LIMIT {
            unread = true;
            break;
        }
        places.push(place);
    }
    (places, unread)
}

fn host_path(url: &str) -> Option<HostPath> {
    let rest = after_scheme(url)?;
    let cut = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    if cut == 0 {
        return None;
    }
    let authority = &rest[..cut];
    let suffix = &rest[cut..];
    let named = if let Some(at) = authority.rfind('@') {
        if at == 0 || at + 1 >= authority.len() {
            return None;
        }
        &authority[at + 1..]
    } else {
        authority
    };
    let host = host_from_named(named)?;
    Some(HostPath {
        host,
        path: path_from_suffix(suffix),
    })
}

fn after_scheme(url: &str) -> Option<&str> {
    let bytes = url.as_bytes();
    for prefix in [b"https://".as_slice(), b"http://".as_slice()] {
        if bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return Some(&url[prefix.len()..]);
        }
    }
    None
}

fn host_from_named(named: &str) -> Option<String> {
    let host = if let Some(body) = named.strip_prefix('[') {
        let end = body.find(']')?;
        let tail = &body[end + 1..];
        if !tail.is_empty() && !port_tail(tail) {
            return None;
        }
        let host = &body[..end];
        if host.is_empty() {
            return None;
        }
        host
    } else if let Some((host, port)) = named.split_once(':') {
        if host.is_empty() || !port_ok(port) {
            return None;
        }
        host
    } else {
        named
    };
    if host_ok(host) {
        Some(host.to_string())
    } else {
        None
    }
}

fn port_tail(tail: &str) -> bool {
    match tail.strip_prefix(':') {
        Some(port) => port_ok(port),
        None => false,
    }
}

fn port_ok(port: &str) -> bool {
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    matches!(port.parse::<u16>(), Ok(value) if value > 0)
}

fn host_ok(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|ch| {
            !ch.is_whitespace()
                && !ch.is_control()
                && !matches!(ch, '/' | '\\' | '@' | '?' | '#' | '[' | ']' | '%')
        })
}

fn path_from_suffix(suffix: &str) -> String {
    if suffix.is_empty() || suffix.starts_with('?') || suffix.starts_with('#') {
        return "/".to_string();
    }
    let end = suffix.find(['?', '#']).unwrap_or(suffix.len());
    suffix[..end].to_string()
}

//! URL rows from saved session text.
//! The scan matches two literal markers, keeps 16 quote-bounded URLs, and names a further URL.
//! A URL longer than 64 characters keeps the first 64 and adds `...`.
//! It is not a document parser, and the session is not replayed.

const ROW_LIMIT: usize = 16;
const TEXT_CAP: usize = 64;
const ENTRIES: &str = "\"entries\"";
const URL_KEY: &str = "\"url\": ";

pub fn session_rows(bytes: &[u8]) -> String {
    let Some(text) = std::str::from_utf8(bytes).ok() else {
        return missed();
    };
    if !text.contains(ENTRIES) || !text.contains(URL_KEY) {
        return missed();
    }
    let (urls, unread) = collect(text);
    if urls.is_empty() {
        return missed();
    }
    let mut report = String::new();
    for (index, url) in urls.iter().enumerate() {
        report.push_str(&format!("row {} {}\n", index + 1, shown(url)));
    }
    if unread {
        report.push_str("later rows were not listed\n");
    }
    report.push_str(&format!(
        "session rows {}. The session was not replayed.\n",
        urls.len()
    ));
    report
}

fn missed() -> String {
    "A saved session was not identified.".to_string()
}

fn collect(text: &str) -> (Vec<String>, bool) {
    let prefix = "\"url\": \"";
    let mut urls = Vec::new();
    let mut from = 0usize;
    let mut unread = false;
    while from < text.len() {
        let Some(rel) = text[from..].find(prefix) else {
            break;
        };
        let start = from + rel + prefix.len();
        let tail = &text[start..];
        let Some(end) = tail.find('"') else {
            break;
        };
        let candidate = &tail[..end];
        from = start + end + 1;
        if candidate.chars().any(char::is_whitespace) {
            continue;
        }
        if urls.len() == ROW_LIMIT {
            unread = true;
            break;
        }
        urls.push(candidate.to_string());
    }
    (urls, unread)
}

fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut more = false;
    for (index, ch) in text.chars().enumerate() {
        if index == TEXT_CAP {
            more = true;
            break;
        }
        out.push(ch);
    }
    if more {
        out.push_str("...");
    }
    out
}

//! Saved HTTP Archive captures, read as evidence rows.
//! A row records the request line plus the response MIME and content.size. It is not a replay.
use super::{obj, parse, s, Json, Json::*};

const ENTRY_CAP: usize = 64;
const URL_CAP: usize = 2048;
const REPORT_CAP: usize = 8192;
const ENTRY_CAP_NAME: &str = "saved-capture entry cap";
const URL_CAP_NAME: &str = "saved-capture url cap";
const REPORT_CAP_NAME: &str = "saved-capture report cap";
const TRUNCATED: &str = "Refusal: saved capture truncated.";
const UNUSABLE: &str = "Refusal: saved capture JSON is unusable.";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    index: usize,
    method: String,
    url: String,
    status: u16,
    mime: String,
    bytes: u64,
}

#[derive(Debug)]
struct Capture {
    rows: Vec<Row>,
    caps: Vec<&'static str>,
    report: String,
}

#[derive(Debug)]
enum Read {
    NotCapture,
    Rows(Capture),
    Refused(String),
}

fn at<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Obj(fields) => fields.get(key),
        _ => None,
    }
}

fn whole(value: &Json) -> Option<u64> {
    let Num(text) = value else { return None };
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn clean(value: &str) -> bool {
    value.chars().all(|c| !c.is_control())
}

/// A document that still has an open string, object, or array was cut.
/// A balanced document that still fails to parse is unusable JSON.
fn closed(text: &str) -> bool {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
    }
    !in_string && depth == 0
}

fn refuse_field(index: usize, field: &str) -> Read {
    Read::Refused(format!("Refusal: entry {index} missing {field}."))
}

fn entry(value: &Json, index: usize) -> Result<Row, Read> {
    let Some(request) = at(value, "request") else {
        return Err(refuse_field(index, "request.method"));
    };
    let method = match at(request, "method") {
        Some(Str(text)) if !text.is_empty() && clean(text) => text.clone(),
        _ => return Err(refuse_field(index, "request.method")),
    };
    let url = match at(request, "url") {
        Some(Str(text)) if !text.is_empty() && clean(text) => text.clone(),
        _ => return Err(refuse_field(index, "request.url")),
    };
    let status = match at(value, "response").and_then(|response| at(response, "status")).and_then(whole) {
        Some(code) if code <= 999 => code as u16,
        _ => return Err(refuse_field(index, "response.status")),
    };
    let Some(content) = at(value, "response").and_then(|response| at(response, "content")) else {
        return Err(refuse_field(index, "response.content.mimeType"));
    };
    let mime = match at(content, "mimeType") {
        Some(Str(text)) if clean(text) => text.clone(),
        _ => return Err(refuse_field(index, "response.content.mimeType")),
    };
    let bytes = match at(content, "size").and_then(whole) {
        Some(size) => size,
        None => return Err(refuse_field(index, "response.content.size")),
    };
    Ok(Row { index, method, url, status, mime, bytes })
}

fn interpret(value: &Json) -> Read {
    let Some(log) = at(value, "log") else { return Read::NotCapture };
    let Some(entries) = at(log, "entries") else { return Read::NotCapture };
    let Arr(entries) = entries else { return Read::Refused(UNUSABLE.into()) };
    let mut pending = Vec::with_capacity(entries.len());
    for (index, value) in entries.iter().enumerate() {
        match entry(value, index) {
            Ok(row) => pending.push(row),
            Err(refusal) => return refusal,
        }
    }
    let mut url_cut = false;
    pending.retain(|row| {
        if row.url.len() > URL_CAP {
            url_cut = true;
            false
        } else {
            true
        }
    });
    let entry_cut = pending.len() > ENTRY_CAP;
    if entry_cut {
        pending.truncate(ENTRY_CAP);
    }
    let mut caps = Vec::new();
    if url_cut {
        caps.push(URL_CAP_NAME);
    }
    if entry_cut {
        caps.push(ENTRY_CAP_NAME);
    }
    render(pending, caps)
}

fn cap_line(name: &str) -> String {
    format!("Refusal: {name}.")
}

fn report_reserve() -> usize {
    cap_line(REPORT_CAP_NAME).len() + 1
}

fn row_line(row: &Row) -> String {
    format!("{} {} {} {} {} {}", row.index, row.method, row.url, row.status, row.mime, row.bytes)
}

fn render(candidates: Vec<Row>, mut caps: Vec<&'static str>) -> Read {
    let mut preface = vec![
        "Saved HTTP Archive capture".to_string(),
        "Scope: request method, URL, status, response MIME, and content.size. Not a replay.".to_string(),
    ];
    for name in &caps {
        preface.push(cap_line(name));
    }
    let mut used = preface.iter().map(|line| line.len() + 1).sum::<usize>();
    used = used.saturating_add("Entries 64.\n".len());
    let reserve = report_reserve();
    let mut emitted = Vec::new();
    let mut report_cut = false;
    for row in &candidates {
        let line = row_line(row);
        if used.saturating_add(line.len()).saturating_add(1).saturating_add(reserve) > REPORT_CAP {
            report_cut = true;
            break;
        }
        used = used.saturating_add(line.len() + 1);
        emitted.push(row.clone());
    }
    if report_cut {
        caps.push(REPORT_CAP_NAME);
    }
    let mut report = String::new();
    for line in &preface {
        report.push_str(line);
        report.push('\n');
    }
    let count = format!("Entries {}.", emitted.len());
    report.push_str(&count);
    report.push('\n');
    for row in &emitted {
        report.push_str(&row_line(row));
        report.push('\n');
    }
    if report_cut {
        report.push_str(&cap_line(REPORT_CAP_NAME));
        report.push('\n');
    }
    debug_assert!(report.len() <= REPORT_CAP);
    Read::Rows(Capture { rows: emitted, caps, report })
}

fn read(bytes: &[u8], complete: bool) -> Read {
    if !complete {
        return Read::Refused(TRUNCATED.into());
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let refusal = if error.error_len().is_none() { TRUNCATED } else { UNUSABLE };
            return Read::Refused(refusal.into());
        }
    };
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return Read::NotCapture;
    }
    match parse(trimmed) {
        Ok(value) => interpret(&value),
        Err(_) if !closed(trimmed) => Read::Refused(TRUNCATED.into()),
        Err(_) => Read::Refused(UNUSABLE.into()),
    }
}

fn view(rows: &[Row], caps: &[&str], refusal: Option<&str>, report: &str) -> Json {
    let entries = rows
        .iter()
        .map(|row| {
            obj(vec![
                ("index", Num(row.index.to_string())),
                ("method", s(&row.method)),
                ("url", s(&row.url)),
                ("status", Num(row.status.to_string())),
                ("mime", s(&row.mime)),
                ("bytes", Num(row.bytes.to_string())),
            ])
        })
        .collect();
    obj(vec![
        ("entries", Arr(entries)),
        ("caps", Arr(caps.iter().copied().map(s).collect())),
        ("refusal", refusal.map(s).unwrap_or(Null)),
        ("report", s(report)),
    ])
}

/// `None` leaves an ordinary observation report unchanged.
/// A capture, including a refusal, is attached by the observations route.
pub(super) fn saved_capture(bytes: &[u8]) -> Option<Json> {
    match read(bytes, true) {
        Read::NotCapture => None,
        Read::Rows(capture) => Some(view(&capture.rows, &capture.caps, None, &capture.report)),
        Read::Refused(reason) => Some(view(&[], &[], Some(&reason), &reason)),
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    fn one(method: &str, url: &str, status: u16, mime: &str, size: u64) -> String {
        format!(
            r#"{{"request":{{"method":"{method}","url":"{url}","headers":[{{"name":"Content-Type","value":"text/plain"}}]}},"response":{{"status":{status},"content":{{"mimeType":"{mime}","size":{size}}},"bodySize":99999}}}}"#
        )
    }

    fn document(entries: &[String]) -> String {
        format!(
            r#"{{"log":{{"version":"1.2","creator":{{"name":"recorder","version":"1"}},"entries":[{}]}}}}"#,
            entries.join(",")
        )
    }

    fn rows(bytes: &[u8]) -> Capture {
        match read(bytes, true) {
            Read::Rows(capture) => capture,
            Read::Refused(reason) => panic!("expected rows, refused: {reason}"),
            Read::NotCapture => panic!("expected rows, document was not a capture"),
        }
    }

    #[test]
    fn valid_capture_yields_method_url_status_mime_and_byte_count() {
        // Fails if method/url/status are swapped, if MIME comes from the request header,
        // or if the byte count is bodySize (99999) or the URL length instead of content.size.
        let raw = document(&[
            one("POST", "https://example.test/submit", 201, "application/json", 40),
            one("GET", "https://example.test/item", 200, "text/html", 12),
        ]);
        let capture = rows(raw.as_bytes());
        assert!(capture.caps.is_empty(), "{:?}", capture.caps);
        assert_eq!(capture.rows.len(), 2);
        assert_eq!(capture.rows[0].index, 0);
        assert_eq!(capture.rows[0].method, "POST");
        assert_eq!(capture.rows[0].url, "https://example.test/submit");
        assert_eq!(capture.rows[0].status, 201);
        assert_eq!(capture.rows[0].mime, "application/json");
        assert_eq!(capture.rows[0].bytes, 40);
        assert_eq!(capture.rows[1].method, "GET");
        assert_eq!(capture.rows[1].url, "https://example.test/item");
        assert_eq!(capture.rows[1].status, 200);
        assert_eq!(capture.rows[1].mime, "text/html");
        assert_eq!(capture.rows[1].bytes, 12);
        assert!(capture.report.contains("https://example.test/submit"));
        assert!(capture.report.contains("text/html"));
        assert!(!capture.report.contains("99999"), "{}", capture.report);
        assert!(!capture.report.contains("text/plain"), "{}", capture.report);
    }

    #[test]
    fn truncated_capture_is_refused() {
        // Fails if an unfinished file is still emitted as rows, or if a cut token
        // is reported as unusable JSON instead of a truncated capture.
        let whole = document(&[one("GET", "https://example.test/item", 200, "text/html", 12)]);
        match read(whole.as_bytes(), false) {
            Read::Refused(reason) => {
                assert!(reason.contains("truncated"), "{reason}");
                assert!(!reason.contains("unusable"), "{reason}");
            }
            other => panic!("incomplete file was not refused: {other:?}"),
        }
        let cut = br#"{"log":{"version":"1.2","entries":[{"request":{"method":"GET","url":"https://example.test/cut"#;
        match read(cut, true) {
            Read::Refused(reason) => {
                assert!(reason.contains("truncated"), "{reason}");
                assert!(!reason.contains("https://example.test/cut"), "{reason}");
            }
            other => panic!("cut capture was not refused: {other:?}"),
        }
    }

    #[test]
    fn missing_required_field_names_the_entry() {
        // Fails if the refusal names entry 0, omits the index, or still returns the valid sibling row.
        let broken = r#"{"request":{"method":"GET","url":"https://example.test/broken"},"response":{"content":{"mimeType":"text/plain","size":4}}}"#;
        let raw = document(&[
            one("GET", "https://example.test/ok", 200, "text/html", 8),
            broken.to_string(),
        ]);
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert!(reason.contains("entry 1 missing response.status"), "{reason}");
                assert!(!reason.contains("entry 0"), "{reason}");
            }
            other => panic!("broken entry was not refused: {other:?}"),
        }
    }

    #[test]
    fn entry_cap_names_itself_when_it_cuts() {
        // Fails if entry 64 is kept, if the extra entry disappears without the cap name,
        // or if the report cap is blamed for a short list.
        let entries = (0..=ENTRY_CAP).map(|index| one("GET", &format!("https://e.test/{index}"), 200, "text/plain", 1)).collect::<Vec<_>>();
        let capture = rows(document(&entries).as_bytes());
        assert_eq!(capture.rows.len(), ENTRY_CAP);
        assert!(capture.caps.contains(&ENTRY_CAP_NAME), "{:?}", capture.caps);
        assert!(!capture.caps.contains(&REPORT_CAP_NAME), "{:?}", capture.caps);
        assert!(capture.report.contains(ENTRY_CAP_NAME), "{}", capture.report);
        assert!(!capture.rows.iter().any(|row| row.index == ENTRY_CAP));
        assert!(capture.report.len() <= REPORT_CAP);
    }

    #[test]
    fn url_cap_names_itself_when_it_cuts() {
        // Fails if the long URL is stored, shortened, or dropped without the cap name,
        // or if the later short entry is discarded with it.
        let long = format!("https://e.test/{}", "URL-MARKER-".repeat(URL_CAP));
        assert!(long.len() > URL_CAP);
        let raw = document(&[
            one("GET", &long, 200, "text/plain", 2),
            one("HEAD", "https://e.test/ok", 204, "text/plain", 0),
        ]);
        let capture = rows(raw.as_bytes());
        assert_eq!(capture.rows.len(), 1);
        assert_eq!(capture.rows[0].url, "https://e.test/ok");
        assert_eq!(capture.rows[0].method, "HEAD");
        assert!(capture.caps.contains(&URL_CAP_NAME), "{:?}", capture.caps);
        assert!(capture.report.contains(URL_CAP_NAME), "{}", capture.report);
        assert!(!capture.report.contains("URL-MARKER-"), "{}", capture.report);
        assert!(capture.rows.iter().all(|row| row.url.len() <= URL_CAP));
    }

    #[test]
    fn report_cap_names_itself_when_it_cuts() {
        // Fails if every row is kept, if the report grows past the cap,
        // or if the oversized line is shortened without the cap name.
        let mime = "M".repeat(REPORT_CAP);
        let raw = document(&[one("GET", "https://e.test/wide", 200, &mime, 9)]);
        let capture = rows(raw.as_bytes());
        assert!(capture.rows.is_empty(), "{}", capture.rows.len());
        assert_eq!(capture.caps, vec![REPORT_CAP_NAME]);
        assert!(capture.report.contains(REPORT_CAP_NAME), "{}", capture.report);
        assert!(!capture.report.contains("MMMMMMMM"), "{}", capture.report);
        assert!(capture.report.len() <= REPORT_CAP, "{}", capture.report.len());
    }

    #[test]
    fn unusable_json_is_refused() {
        // Fails if a balanced but illegal capture is called truncated, or if entries of the wrong type become rows.
        let raw = r#"{"log":{"version":"1.2","entries":{"not":"an array"}}}"#;
        match read(raw.as_bytes(), true) {
            Read::Refused(reason) => {
                assert!(reason.contains("unusable"), "{reason}");
                assert!(!reason.contains("truncated"), "{reason}");
            }
            other => panic!("unusable capture was not refused: {other:?}"),
        }
    }

    #[test]
    fn page_text_is_not_a_saved_capture() {
        // Fails if an ordinary retained page report grows a saved_capture object on the observations route.
        assert!(saved_capture(b"<html><p>not a capture</p></html>").is_none());
        assert!(saved_capture(br#"{"page":"ok"}"#).is_none());
        let raw = document(&[one("GET", "https://example.test/item", 200, "text/html", 12)]);
        let attached = saved_capture(raw.as_bytes()).expect("capture reaches the evidence object");
        let entries = attached.get("entries").unwrap().array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].get("method").unwrap().text().unwrap(), "GET");
        assert_eq!(entries[0].get("bytes").unwrap(), &Num("12".into()));
        assert_eq!(attached.get("refusal").unwrap(), &Null);
    }
}

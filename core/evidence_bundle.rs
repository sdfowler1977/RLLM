//! Local document for UTF-8 rows the caller already holds.
//! The listing copies at most 32 rows and leaves the bundle uninstalled.
#![forbid(unsafe_code)]

const ROW_CAP: usize = 32;

/// One document: a header, at most 32 numbered rows, then the bundled count.
/// A further row adds one line that later rows were not bundled.
/// The document ends by stating that the bundle was not installed.
pub fn evidence_bundle(rows: &[&str]) -> String {
    let kept = rows.len().min(ROW_CAP);
    let mut out = String::from("evidence bundle\n");
    for (index, row) in rows.iter().take(kept).enumerate() {
        write_row(&mut out, index + 1, row);
    }
    out.push_str(&format!("bundle rows {kept}\n"));
    if rows.len() > ROW_CAP {
        out.push_str("later rows were not bundled\n");
    }
    out.push_str("The bundle was not installed.\n");
    out
}

fn write_row(out: &mut String, number: usize, row: &str) {
    out.push_str("row ");
    out.push_str(&number.to_string());
    if !row.is_empty() {
        out.push(' ');
        if row.contains(['\n', '\r']) {
            for ch in row.chars() {
                out.push(if ch == '\n' || ch == '\r' { ' ' } else { ch });
            }
        } else {
            out.push_str(row);
        }
    }
    out.push('\n');
}

/// Bundle UTF-8 lines that follow the `EVB1` marker.
/// The marker line is `marker EVB1` when that prefix is present.
/// A missing prefix does not invent a marker.
pub fn evidence_bundle_bytes(bytes: &[u8]) -> String {
    let Some(rest) = bytes.strip_prefix(b"EVB1\n") else {
        return "The bundle was not installed.\n".to_string();
    };
    let text = String::from_utf8_lossy(rest);
    let owned: Vec<String> = text.lines().map(str::to_string).collect();
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut out = evidence_bundle(&refs);
    let header = "evidence bundle\n";
    if out.starts_with(header) {
        out.insert_str(header.len(), "marker EVB1\n");
    }
    out
}

//! Bounded structural observations over original input bytes; never executes data.
use super::*;
use std::fmt::Write as _;

const INPUT_LIMIT: usize = 128 * 1024;
const OUTPUT_LIMIT: usize = 8192;
const NODE_LIMIT: usize = 16384;
const PATH_LIMIT: usize = 24;
const PATH_BYTES: usize = 192;
const ROW_LIMIT: usize = 4096;

fn cut(value: &str, limit: usize) -> &str {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) { end -= 1; }
    &value[..end]
}
fn path(parent: &str, key: &str) -> String {
    let mut out = String::from(cut(parent, PATH_BYTES));
    if out.len() >= PATH_BYTES { return out; }
    out.push('/');
    for c in key.chars() {
        let piece = match c {
            '~' => "~0".into(), '/' => "~1".into(),
            c if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') => format!("\\u{{{:x}}}", c as u32),
            c => c.to_string(),
        };
        if out.len() + piece.len() > PATH_BYTES { break; }
        out.push_str(&piece);
    }
    out
}
struct Structure {
    counts: [usize; 6],
    nodes: usize,
    depth: usize,
    paths: Vec<(String, &'static str)>,
    bounded: bool,
}
impl Structure {
    fn new() -> Self { Self { counts: [0; 6], nodes: 0, depth: 0, paths: Vec::new(), bounded: false } }
    fn visit(&mut self, value: &Json, location: &str, depth: usize) {
        if self.nodes == NODE_LIMIT { self.bounded = true; return; }
        self.nodes += 1; self.depth = self.depth.max(depth);
        let (index, name) = match value {
            Obj(_) => (0, "object"), Arr(_) => (1, "array"), Str(_) => (2, "string"),
            Num(_) => (3, "number"), Bool(_) => (4, "boolean"), Null => (5, "null"),
        };
        self.counts[index] += 1;
        if self.paths.len() < PATH_LIMIT { self.paths.push((location.into(), name)); }
        match value {
            Obj(fields) => for (key, child) in fields {
                if self.nodes == NODE_LIMIT { self.bounded = true; break; }
                self.visit(child, &path(location, key), depth + 1);
            },
            Arr(items) => for (index, child) in items.iter().enumerate() {
                if self.nodes == NODE_LIMIT { self.bounded = true; break; }
                self.visit(child, &path(location, &index.to_string()), depth + 1);
            },
            _ => {}
        }
    }
}

struct Lines<'a> { text: &'a str, position: usize }
impl<'a> Iterator for Lines<'a> {
    type Item = (&'a str, bool);
    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.text.len() { return None; }
        let start = self.position;
        let bytes = self.text.as_bytes();
        while self.position < bytes.len() && !matches!(bytes[self.position], b'\r' | b'\n') { self.position += 1; }
        let end = self.position;
        let terminated = self.position < bytes.len();
        if terminated {
            let c = bytes[self.position]; self.position += 1;
            if c == b'\r' && bytes.get(self.position) == Some(&b'\n') { self.position += 1; }
        }
        Some((&self.text[start..end], terminated))
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Cell { Start, Plain, Quoted, Closed }
struct Table {
    rows: usize, minimum: usize, maximum: usize, first: usize,
    delimiters: usize, quoted: usize, multiline: usize,
    consumed: usize, limited: bool, tail: bool, invalid: Option<usize>,
}
impl Table {
    fn new() -> Self { Self { rows: 0, minimum: usize::MAX, maximum: 0, first: 0, delimiters: 0, quoted: 0, multiline: 0, consumed: 0, limited: false, tail: false, invalid: None } }
    fn row(&mut self, fields: usize) {
        if self.rows == 0 { self.first = fields; }
        self.rows += 1; self.minimum = self.minimum.min(fields); self.maximum = self.maximum.max(fields);
    }
}
// Strict comma/tab dialect: doubled quotes, quoted newlines, CRLF/LF/CR rows.
// Spaces after a closing quote are not silently repaired or accepted.
fn table(text: &str, delimiter: u8, complete: bool) -> Table {
    let bytes = text.as_bytes(); let mut out = Table::new();
    let mut cell = Cell::Start; let mut columns = 1; let mut i = 0; let mut row_start = 0;
    let mut quoted = 0; let mut multiline = 0; let mut separators = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if cell == Cell::Quoted {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') { i += 2; continue; }
                cell = Cell::Closed;
            } else if matches!(c, b'\r' | b'\n') {
                multiline += 1;
                if c == b'\r' && bytes.get(i + 1) == Some(&b'\n') { i += 1; }
            }
            i += 1; continue;
        }
        if c == delimiter {
            columns += 1; separators += 1; cell = Cell::Start; i += 1; continue;
        }
        if matches!(c, b'\r' | b'\n') {
            out.row(columns); out.quoted += quoted; out.multiline += multiline; out.delimiters += separators;
            i += 1;
            if c == b'\r' && bytes.get(i) == Some(&b'\n') { i += 1; }
            out.consumed = i; row_start = i; columns = 1; quoted = 0; multiline = 0; separators = 0; cell = Cell::Start;
            if out.rows == ROW_LIMIT && i < bytes.len() { out.limited = true; break; }
            continue;
        }
        if c == b'"' && cell == Cell::Start { cell = Cell::Quoted; quoted += 1; i += 1; continue; }
        if c == b'"' || cell == Cell::Closed { out.invalid = Some(i); out.consumed = i; return out; }
        cell = Cell::Plain; i += 1;
    }
    if !out.limited && row_start < bytes.len() {
        if !complete { out.tail = true; }
        else if cell == Cell::Quoted { out.invalid = Some(bytes.len()); }
        else { out.row(columns); out.quoted += quoted; out.multiline += multiline; out.delimiters += separators; out.consumed = bytes.len(); }
    }
    out
}

/// `complete` means the caller supplied the entire source, not merely a read
/// chunk. Internal byte limiting can further reduce that scope to a prefix.
pub(super) fn analyze(data: &[u8], complete: bool) -> String {
    let limit = data.len().min(INPUT_LIMIT);
    let (text, examined, split_utf8) = match std::str::from_utf8(&data[..limit]) {
        Ok(text) => (text, limit, false),
        Err(error) if error.error_len().is_none() && (!complete || limit < data.len()) => {
            let end = error.valid_up_to();
            match std::str::from_utf8(&data[..end]) { Ok(text) => (text, end, true), Err(_) => unreachable!() }
        },
        Err(error) => return format!("Structural document observations\nSupplied {} bytes; examined at most {limit} bytes. Source scope: {}.\nThe examined bytes are not valid UTF-8 (first invalid sequence at byte {}). JSON, delimited-text and heading analysis were not performed; this does not identify the binary format or establish validity of unseen bytes.", data.len(), if complete && limit == data.len() { "entire supplied source" } else { "prefix only; unexamined bytes unknown" }, error.valid_up_to()),
    };
    let whole = complete && examined == data.len();
    let scope = if whole { "complete supplied input" } else { "examined prefix only; unexamined remainder unknown" };
    let mut out = format!("Structural document observations\nSupplied {} bytes; examined {examined} bytes (limit {INPUT_LIMIT}). Scope: {scope}.\n", data.len());
    if split_utf8 { out.push_str("An incomplete UTF-8 sequence at the sample boundary was excluded; no whole-input encoding claim.\n"); }
    let content = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = 0; let mut terminated = 0; let mut blank = 0; let mut headings = [0usize; 6]; let mut longest = 0; let mut characters = 0;
    for (line, ended) in (Lines { text: content, position: 0 }) {
        lines += 1; terminated += usize::from(ended); blank += usize::from(line.trim().is_empty());
        longest = longest.max(line.chars().count()); characters += line.chars().count();
        let line = line.trim_start_matches(' '); let hashes = line.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&hashes) && line.as_bytes().get(hashes).map(|c| matches!(c,b' '|b'\t')).unwrap_or(true) { headings[hashes - 1] += 1; }
    }
    let _ = writeln!(out, "Text in this scope: {lines} line segments, {terminated} newline-terminated; {blank} blank; {characters} Unicode scalar values excluding line endings; longest segment {longest} scalars.");
    if !whole && lines > terminated { out.push_str("The final line segment may be incomplete and is not a confirmed complete line.\n"); }
    let _ = writeln!(out,"ATX-style heading markers by level 1–6: {:?}. Marker counts are lexical observations, not a Markdown/source-code parse.",headings);
    let parsed = parse(content.trim());let json_valid = parsed.is_ok();
    match parsed {
        Ok(value) => {
            let mut shape = Structure::new(); shape.visit(&value,"$",0);
            let _ = writeln!(out,"JSON: {} parses as one JSON value under the parser's nesting bound of 64; no code was executed.",if whole{"complete supplied input"}else{"examined prefix by itself (unseen remainder may change document validity)"});
            let _ = writeln!(out,"JSON structure {}: {} visited nodes; deepest visited edge depth {}; objects {}; arrays {}; strings {}; numbers {}; booleans {}; null {}.",if shape.bounded{"partial (node bound reached)"}else{"of the parsed value"},shape.nodes,shape.depth,shape.counts[0],shape.counts[1],shape.counts[2],shape.counts[3],shape.counts[4],shape.counts[5]);
            let _ = writeln!(out,"First {} structural paths (object keys are untrusted data; paths abbreviated to {PATH_BYTES} bytes; ~0/~1 escape ~ and /; scalar contents omitted):",shape.paths.len());
            for (location, kind) in shape.paths { let _ = writeln!(out,"{} : {kind}",quote(&location)); }
        },
        Err(error) if matches!(content.trim().as_bytes().first(),Some(b'{'|b'['|b'"')) => {
            let _ = writeln!(out,"JSON candidate: {}. Parser diagnostic: {}.",if whole{"supplied input does not parse under the parser's limits; this may be syntax or a resource-bound refusal"}else{"prefix does not form a complete parse; whole-document validity is unknown"},cut(&error,192));
        },
        Err(_) => {}
    }
    for (delimiter,name) in [(b',',"Comma"),(b'\t',"Tab")] {
        if json_valid { break; }
        let shape = table(content,delimiter,whole);
        if shape.delimiters == 0 { continue; }
        if let Some(at) = shape.invalid {
            let _ = writeln!(out,"{name}-delimited candidate: strict quoting rejected at examined byte {at}; no complete-table claim. {} complete records preceded the error.",shape.rows);
        } else {
            let _ = writeln!(out,"{name}-delimited candidate: {} complete records; first record {} fields; field-count range {}–{}; {} quoted fields and {} quoted line breaks in those records. Header/type inference not performed.",shape.rows,shape.first,shape.minimum.min(shape.maximum),shape.maximum,shape.quoted,shape.multiline);
            if shape.minimum != shape.maximum { out.push_str("Record field counts differ; a rectangular table was not established.\n"); }
            if shape.tail { out.push_str("The unterminated final sampled record was excluded; it may continue beyond the sample.\n"); }
            if shape.limited { let _ = writeln!(out,"Record analysis stopped after {ROW_LIMIT} records at byte {}; remaining examined text was not parsed as a table.",shape.consumed); }
            if !whole { out.push_str("These delimited observations apply only to the sampled records, not the whole document.\n"); }
        }
    }
    if out.len() > OUTPUT_LIMIT { let mut bounded = cut(&out,OUTPUT_LIMIT-80).to_string();bounded.push_str("\nStructural report abbreviated at the output bound.\n");bounded } else { out }
}

#[cfg(test)]
mod checks {
    use super::*;
    #[test] fn json_structure_omits_scalar_contents_and_escapes_paths() {
        let text = analyze(br#"{"a/b":{"~key":"DO_NOT_REPORT_VALUE"},"list":[1,true,false,null]}"#,true);
        assert!(text.contains("objects 2; arrays 1; strings 1; numbers 1; booleans 2; null 1"));
        assert!(text.contains("$/a~1b/~0key"));assert!(!text.contains("DO_NOT_REPORT_VALUE"));assert!(text.contains("complete supplied input"));
    }
    #[test] fn malformed_depth_and_sample_are_not_complete_claims() {
        let malformed = analyze(br#"{"a":1,}"#,true);assert!(malformed.contains("does not parse"));
        let deep = format!("{}0{}","[".repeat(70),"]".repeat(70));assert!(analyze(deep.as_bytes(),true).contains("nesting limit"));
        let sample = analyze(br#"{"a":1}"#,false);assert!(sample.contains("unseen remainder may change document validity"));assert!(sample.contains("prefix only"));
        assert!(analyze(br#"{"a":"#,false).contains("whole-document validity is unknown"));
    }
    #[test] fn delimited_quotes_multiline_and_partial_tail() {
        let sample = "name,value\r\n\"two\r\nlines\",\"a\"\"b\"\r\n";
        let text = analyze(sample.as_bytes(),true);assert!(text.contains("2 complete records; first record 2 fields; field-count range 2–2"));assert!(text.contains("2 quoted fields and 1 quoted line breaks"));
        let text = analyze(b"a,b\n1,2",false);assert!(text.contains("1 complete records"));assert!(text.contains("final sampled record was excluded"));
        let text = analyze(b"a,b\n\"bad\"z,2",true);assert!(text.contains("strict quoting rejected"));
        let text = analyze(b"a\tb\n1\t2\t3",true);assert!(text.contains("field-count range 2–3"));
    }
    #[test] fn unicode_crlf_empty_and_heading_counts() {
        let text = analyze("# Héllo\r\n\r世界\n## Two\n".as_bytes(),true);
        assert!(text.contains("4 line segments, 4 newline-terminated; 1 blank"));assert!(text.contains("[1, 1, 0, 0, 0, 0]"));
        assert!(analyze(b"",true).contains("0 line segments"));
        assert!(analyze(&[0xff],true).contains("not valid UTF-8"));
        assert!(analyze(&[b'a',0xe2,0x82],false).contains("incomplete UTF-8 sequence"));
        assert!(analyze(&[b'a',0xe2,0x82],true).contains("not valid UTF-8"));
    }
    #[test] fn resource_bounds_are_visible() {
        let data = "a,b\n".repeat(ROW_LIMIT+100);let report=analyze(data.as_bytes(),true);assert!(report.contains("stopped after 4096 records"));assert!(report.len()<=OUTPUT_LIMIT);
        let data = "x".repeat(INPUT_LIMIT+100);let report=analyze(data.as_bytes(),true);assert!(report.contains("prefix only"));assert!(report.len()<=OUTPUT_LIMIT);
        let data=format!("[{}]",vec!["0";NODE_LIMIT+10].join(","));assert!(analyze(data.as_bytes(),true).contains("partial (node bound reached)"));
    }
}

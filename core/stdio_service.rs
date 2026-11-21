//! Stdio service for local file inspection. One JSON value per line on standard
//! input, one JSON value per line on standard output. Diagnostic text stays on
//! standard error so it cannot split a response.
use super::*;
use std::io::{Read, Seek, Write};
use std::path::Path;

const MESSAGE_LIMIT: usize = 8 * 1024 * 1024;
const OVERVIEW_PREFIX: usize = 4 * 1024 * 1024;
const INLINE_MATCHES: usize = 200;

pub(super) fn serve() -> Result<()> {
    #[cfg(windows)]
    binary_pipes();
    let mut input = std::io::BufReader::new(std::io::stdin());
    let mut output = std::io::stdout();
    let mut message = Vec::new();
    loop {
        let mut chunk = [0u8; 8192];
        loop {
            let n = input.read(&mut chunk).map_err(|e| format!("Cannot read a service message: {e}"))?;
            if n == 0 {
                if message.is_empty() {
                    return Ok(());
                }
                return Err("Service input ended inside a message".into());
            }
            message.extend_from_slice(&chunk[..n]);
            if message.len() > MESSAGE_LIMIT {
                return Err("Service message exceeds 8 MiB".into());
            }
            if message.contains(&b'\n') {
                break;
            }
        }
        while let Some(split) = message.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = message.drain(..=split).collect();
            let text = String::from_utf8_lossy(&line);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            let response = match parse(trimmed) {
                Ok(value) => respond(&value),
                Err(error) => Some(failure(&Null, -32700, &format!("Parse error: {error}"))),
            };
            if let Some(response) = response {
                output.write_all(response.string().as_bytes()).map_err(|e| e.to_string())?;
                output.write_all(b"\n").map_err(|e| e.to_string())?;
                output.flush().map_err(|e| e.to_string())?;
                output.flush().map_err(|e| e.to_string())?;
            }
        }
    }
}

#[cfg(windows)]
fn binary_pipes() {
    extern "C" {
        fn _setmode(fd: i32, mode: i32) -> i32;
    }
    unsafe {
        _setmode(0, 0x8000);
        _setmode(1, 0x8000);
    }
}

fn respond(message: &Json) -> Option<Json> {
    let method = text(message, "method");
    let id = get(message, "id");
    if id == &Null {
        return None;
    }
    let params = get(message, "params");
    let result = match method.as_str() {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(obj(vec![])),
        "tools/list" => Ok(obj(vec![("tools", Arr(tool_definitions()))])),
        "tools/call" => call_tool(params).map(|text| tool_result(&text, false)),
        "resources/list" | "prompts/list" => Ok(obj(vec![(if method.starts_with("resources") { "resources" } else { "prompts" }, Arr(vec![]))])),
        _ => Err(format!("Method {method} is not implemented")),
    };
    Some(match result {
        Ok(value) => obj(vec![("jsonrpc", s("2.0")), ("id", id.clone()), ("result", value)]),
        Err(error) if method == "tools/call" => obj(vec![("jsonrpc", s("2.0")), ("id", id.clone()), ("result", tool_result(&error, true))]),
        Err(error) => failure(id, if method.is_empty() { -32600 } else { -32601 }, &error),
    })
}

fn initialize(params: &Json) -> Json {
    let requested = text(params, "protocolVersion");
    let version = if requested.is_empty() { "2025-03-26" } else { &requested };
    obj(vec![
        ("protocolVersion", s(version)),
        ("capabilities", obj(vec![("tools", obj(vec![("listChanged", Bool(false))]))])),
        ("serverInfo", obj(vec![("name", s("RLLM")), ("version", s("1"))])),
        ("instructions", s("Inspect local files. file_report covers a declared PE, ELF, or Mach-O header. instruction_report, container_report, and packet_report write a new file named by output. byte_matches, text_runs, and byte_window return a bounded inline page and write the full result when output is set. A tool that is not listed is not implemented.")),
    ])
}

fn failure(id: &Json, code: i64, message: &str) -> Json {
    obj(vec![
        ("jsonrpc", s("2.0")),
        ("id", id.clone()),
        ("error", obj(vec![("code", Num(code.to_string())), ("message", s(message))])),
    ])
}

fn tool_result(text: &str, is_error: bool) -> Json {
    obj(vec![
        ("content", Arr(vec![obj(vec![("type", s("text")), ("text", s(text))])])),
        ("isError", Bool(is_error)),
    ])
}

fn tool_definitions() -> Vec<Json> {
    vec![
        tool("file_report", "Read one local file and report its length, digest, and the structure its header declares. Instruction text in that report is a bounded summary.", &[("path", "Absolute or relative file path", true)]),
        tool("instruction_report", "Write a complete x86 or x86-64 instruction listing to a new file. Omit width for a declared x86 image. width 32 or 64 lists every byte from address 0. The listing stops where an instruction length is not determined.", &[("path", "File to list", true), ("output", "New file that receives the full listing", true), ("width", "32, 64, or omitted", false)]),
        tool("container_report", "Scan one file for embedded container signatures. A header that establishes a member length is copied beside the report as output.0000 and onward. Other marks are reported at their file offset.", &[("path", "File to scan", true), ("output", "New report file. Its parent directory must exist", true)]),
        tool("packet_report", "Read a saved classic or section-block packet capture and write one line per packet. The line names the addresses, ports, and payload bytes that are present.", &[("path", "Capture file", true), ("output", "New report file", true)]),
        tool("byte_matches", "Find exact byte matches. query is plain text, or hex when hex is true. The inline page lists the first matches. output receives every match.", &[("path", "File to search", true), ("query", "Text or hex bytes to find", true), ("hex", "true to parse query as hex", false), ("output", "Optional new file for every match", false)]),
        tool("text_runs", "List contiguous printable ASCII runs of at least 4 bytes. The inline page lists the first runs. output receives every run.", &[("path", "File to read", true), ("output", "Optional new file for every run", false)]),
        tool("byte_window", "Show bytes at a file offset as hex. length is the number of bytes. The inline page shows the first 4096 bytes. output receives the whole range.", &[("path", "File to read", true), ("offset", "Starting byte offset", true), ("length", "Number of bytes", true), ("output", "Optional new file for the whole range", false)]),
    ]
}

fn tool(name: &str, description: &str, fields: &[(&str, &str, bool)]) -> Json {
    let mut properties = Vec::new();
    let mut required = Vec::new();
    for (name, description, must) in fields {
        properties.push((*name, obj(vec![("type", s("string")), ("description", s(description))])));
        if *must {
            required.push(s(name));
        }
    }
    obj(vec![
        ("name", s(name)),
        ("description", s(description)),
        ("inputSchema", obj(vec![("type", s("object")), ("properties", obj(properties)), ("required", Arr(required))])),
    ])
}

fn call_tool(params: &Json) -> Result<String> {
    let name = text(params, "name");
    let args = get(params, "arguments");
    if name.is_empty() {
        return Err("Tool call requires a name".into());
    }
    match name.as_str() {
        "file_report" => file_overview(args),
        "instruction_report" => instruction_listing(args),
        "container_report" => container_marks(args),
        "packet_report" => packet_report(args),
        "byte_matches" => byte_search(args),
        "text_runs" => text_strings(args),
        "byte_window" => byte_range(args),
        other => Err(format!("Tool {other} is not implemented")),
    }
}

fn path_arg(args: &Json) -> Result<std::path::PathBuf> {
    let text = text(args, "path");
    if text.is_empty() || text.contains('\0') {
        return Err("Name a file path".into());
    }
    Ok(std::path::PathBuf::from(text))
}

fn output_arg(args: &Json, required: bool) -> Result<Option<std::path::PathBuf>> {
    let text = text(args, "output");
    if text.is_empty() {
        if required {
            return Err("Name a new output file".into());
        }
        return Ok(None);
    }
    if text.contains('\0') {
        return Err("Output path is invalid".into());
    }
    let path = std::path::PathBuf::from(text);
    if path.exists() {
        return Err("Output already exists and was not replaced".into());
    }
    Ok(Some(path))
}

fn open_file(path: &Path) -> Result<(std::fs::File, u64)> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Cannot open the named file: {e}"))?;
    if !meta.is_file() {
        return Err("The named path is not a regular file".into());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open the named file: {e}"))?;
    Ok((file, meta.len()))
}

fn read_prefix(path: &Path) -> Result<(u64, String, Vec<u8>)> {
    let (mut file, len) = open_file(path)?;
    let mut hasher = incremental_digest::Digest::new();
    let mut prefix = Vec::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(|e| format!("Cannot read the named file: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]).map_err(|e| e.to_string())?;
        if prefix.len() < OVERVIEW_PREFIX {
            let room = OVERVIEW_PREFIX - prefix.len();
            prefix.extend_from_slice(&buffer[..n.min(room)]);
        }
        total += n as u64;
    }
    if total != len {
        return Err("The file length changed while it was read".into());
    }
    Ok((len, hasher.finish(), prefix))
}

fn file_overview(args: &Json) -> Result<String> {
    let path = path_arg(args)?;
    let (len, digest, prefix) = read_prefix(&path)?;
    let complete = prefix.len() as u64 == len;
    let mut report = format!("file {}\nlength {len}\nSHA-256 {digest}\n", path.display());
    if prefix.starts_with(b"MZ") {
        report.push_str(&super::pe_observation::analyze(&prefix, complete, len, &|| false)?);
        match super::dotnet_evidence::cli_evidence(&prefix, &|| false) {
            Ok(super::dotnet_evidence::CliText::Absent) => {}
            Ok(super::dotnet_evidence::CliText::Observed(text) | super::dotnet_evidence::CliText::Bounded(text)) => {
                report.push('\n');
                report.push_str(&text);
            }
            Err(()) => return Err("Overview cancelled".into()),
        }
    } else if prefix.starts_with(b"\x7fELF") {
        report.push_str(&super::elf_observation::analyze_with_cancel(&prefix, complete, len, &|| false)?);
    } else if super::image_mach::recognized(&prefix) {
        report.push_str(&super::image_mach::report(&prefix));
    } else if prefix.starts_with(b"\0asm") || prefix.starts_with(b"RIFF") || prefix.starts_with(b"RIFX") {
        report.push_str(&super::binary_observation::analyze(&prefix, complete, len));
    } else {
        report.push_str("No PE, ELF, Mach-O, WebAssembly, or RIFF header was identified in the examined prefix.\n");
    }
    if !complete {
        report.push_str("\nThe structure report examined the first 4 MiB. Bytes after that prefix were hashed and were not classified by the header pass.\n");
    }
    Ok(report)
}

fn placed_output(path: &std::path::Path) -> Result<(&Path, String)> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err("Output directory does not exist".into());
    }
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("Output file name is invalid")?;
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err("Output file name is invalid".into());
    }
    Ok((parent, name.to_string()))
}

fn instruction_listing(args: &Json) -> Result<String> {
    let path = path_arg(args)?;
    let output = output_arg(args, true)?.ok_or("Name a new output file")?;
    let (root, name) = placed_output(&output)?;
    let width = text(args, "width");
    let mut fields = vec![("path", s("subject")), ("output", s(&name))];
    if !width.is_empty() {
        fields.push(("width", s(&width)));
    }
    super::subject_reports::run_listing(root, &path, &obj(fields), &|| false)
}

fn container_marks(args: &Json) -> Result<String> {
    let path = path_arg(args)?;
    let output = output_arg(args, true)?.ok_or("Name a new output file")?;
    let (root, name) = placed_output(&output)?;
    super::subject_reports::run_regions(root, &path, &obj(vec![("path", s("subject")), ("output", s(&name))]), &|| false)
}

fn packet_report(args: &Json) -> Result<String> {
    let path = path_arg(args)?;
    let output = output_arg(args, true)?.ok_or("Name a new output file")?;
    let (root, name) = placed_output(&output)?;
    super::subject_reports::run_packets(root, &path, &obj(vec![("path", s("subject")), ("output", s(&name))]), &|| false)
}

fn query_bytes(args: &Json) -> Result<Vec<u8>> {
    let query = text(args, "query");
    if query.is_empty() {
        return Err("Name the bytes to find".into());
    }
    if text(args, "hex") == "true" {
        let hex: String = query.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        if hex.len() % 2 != 0 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Hex query must be an even number of hex digits".into());
        }
        let mut out = Vec::with_capacity(hex.len() / 2);
        let bytes = hex.as_bytes();
        for pair in bytes.chunks(2) {
            out.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
        Ok(out)
    } else {
        Ok(query.into_bytes())
    }
}

fn byte_search(args: &Json) -> Result<String> {
    let path = path_arg(args)?;
    let needle = query_bytes(args)?;
    let output = output_arg(args, false)?;
    let (mut file, _len) = open_file(&path)?;
    let mut writer = match &output {
        Some(path) => Some(std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("Cannot create output: {e}"))?),
        None => None,
    };
    let mut window = vec![0u8; 65536 + needle.len()];
    let mut filled = 0usize;
    let mut file_at = 0u64;
    let mut found = 0u64;
    let mut shown = String::new();
    loop {
        if filled >= window.len() {
            return Err("Search window overflow".into());
        }
        let n = file.read(&mut window[filled..]).map_err(|e| e.to_string())?;
        filled += n;
        if filled < needle.len() {
            break;
        }
        let last = if n == 0 { filled } else { filled - needle.len() + 1 };
        let mut index = 0usize;
        while index + needle.len() <= last {
            if &window[index..index + needle.len()] == needle.as_slice() {
                let at = file_at + index as u64;
                found += 1;
                let line = format!("{at:#x}\n");
                if let Some(writer) = writer.as_mut() {
                    writer.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
                }
                if (found as usize) <= INLINE_MATCHES {
                    shown.push_str(&line);
                }
                index += 1;
            } else {
                index += 1;
            }
        }
        if n == 0 {
            break;
        }
        let keep = filled - index;
        window.copy_within(index..filled, 0);
        file_at += index as u64;
        filled = keep;
    }
    if let Some(writer) = writer.as_mut() {
        writer.flush().map_err(|e| e.to_string())?;
    }
    let mut report = format!("matches {found}\n{shown}");
    if found as usize > INLINE_MATCHES {
        report.push_str(&format!("inline page stopped after {INLINE_MATCHES} matches\n"));
    }
    if let Some(path) = output {
        report.push_str(&format!("full match list written to {}\n", path.display()));
    }
    Ok(report)
}

fn text_strings(args: &Json) -> Result<String> {
    let path = path_arg(args)?;
    let output = output_arg(args, false)?;
    let (mut file, _len) = open_file(&path)?;
    let mut writer = match &output {
        Some(path) => Some(std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("Cannot create output: {e}"))?),
        None => None,
    };
    let mut buffer = [0u8; 65536];
    let mut run = Vec::new();
    let mut run_at = 0u64;
    let mut file_at = 0u64;
    let mut count = 0u64;
    let mut shown = String::new();
    let finish = |run: &mut Vec<u8>, run_at: u64, count: &mut u64, shown: &mut String, writer: &mut Option<std::fs::File>| -> Result<()> {
        if run.len() >= 4 {
            *count += 1;
            let text = String::from_utf8_lossy(run);
            let line = format!("{run_at:#x} {text}\n");
            if let Some(writer) = writer.as_mut() {
                writer.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            }
            if (*count as usize) <= INLINE_MATCHES {
                shown.push_str(&line);
            }
        }
        run.clear();
        Ok(())
    };
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        for (index, byte) in buffer[..n].iter().copied().enumerate() {
            if (0x20..0x7f).contains(&byte) {
                if run.is_empty() {
                    run_at = file_at + index as u64;
                }
                run.push(byte);
            } else {
                finish(&mut run, run_at, &mut count, &mut shown, &mut writer)?;
            }
        }
        file_at += n as u64;
    }
    finish(&mut run, run_at, &mut count, &mut shown, &mut writer)?;
    let mut report = format!("strings {count}\n{shown}");
    if count as usize > INLINE_MATCHES {
        report.push_str(&format!("inline page stopped after {INLINE_MATCHES} strings\n"));
    }
    if let Some(path) = output {
        report.push_str(&format!("full string list written to {}\n", path.display()));
    }
    Ok(report)
}

fn integer_arg(args: &Json, key: &str) -> Result<u64> {
    match get(args, key) {
        Num(value) | Str(value) if !value.is_empty() => value.parse::<u64>().map_err(|_| format!("{key} must be an integer")),
        _ => Err(format!("{key} must be an integer")),
    }
}

fn byte_range(args: &Json) -> Result<String> {
    let path = path_arg(args)?;
    let offset = integer_arg(args, "offset")?;
    let length = integer_arg(args, "length")?;
    let output = output_arg(args, false)?;
    let (mut file, len) = open_file(&path)?;
    if offset > len || length > len - offset {
        return Err("The requested range extends past the end of the file".into());
    }
    file.seek(std::io::SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut writer = match &output {
        Some(path) => Some(std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("Cannot create output: {e}"))?),
        None => None,
    };
    let mut left = length;
    let mut at = offset;
    let mut shown = String::new();
    let mut buffer = [0u8; 4096];
    while left > 0 {
        let room = ((left as usize).min(buffer.len())) as usize;
        file.read_exact(&mut buffer[..room]).map_err(|e| e.to_string())?;
        let line = hex_line(at, &buffer[..room]);
        if let Some(writer) = writer.as_mut() {
            writer.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        }
        if shown.len() < 4096 * 4 {
            shown.push_str(&line);
        }
        at += room as u64;
        left -= room as u64;
    }
    let mut report = format!("range {offset:#x} length {length}\n{shown}");
    if length > 4096 {
        report.push_str("inline page shows the start of the range\n");
    }
    if let Some(path) = output {
        report.push_str(&format!("full range written to {}\n", path.display()));
    }
    Ok(report)
}

fn hex_line(at: u64, bytes: &[u8]) -> String {
    let mut line = format!("{at:#x} ");
    for byte in bytes {
        line.push_str(&format!("{byte:02x}"));
    }
    line.push('\n');
    line
}

#[cfg(test)]
mod checks {
    use super::super::super::{get, obj, parse, s, text, Bool, Null, Num};
    use super::*;

    #[test]
    fn initialize_echoes_the_requested_version_and_lists_tools() {
        let request = obj(vec![
            ("jsonrpc", s("2.0")),
            ("id", Num("1".into())),
            ("method", s("initialize")),
            ("params", obj(vec![("protocolVersion", s("2025-11-25"))])),
        ]);
        let response = respond(&request).unwrap();
        assert_eq!(text(get(&response, "result"), "protocolVersion"), "2025-11-25");
        let listed = parse(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        let response = respond(&listed).unwrap();
        let tools = get(get(&response, "result"), "tools").array().unwrap();
        assert!(tools.iter().any(|tool| text(tool, "name") == "packet_report"));
        assert!(respond(&obj(vec![("jsonrpc", s("2.0")), ("method", s("notifications/initialized"))])).is_none());
    }
}

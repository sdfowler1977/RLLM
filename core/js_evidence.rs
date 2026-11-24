//! Bounded JavaScript and Electron observation over bytes already retained.
//! A file is a JavaScript module only when its bytes contain an import, an export
//! declaration, or a `require` call. A valid ASAR header is an Electron payload.
//! Plain text is a named refusal. Specifiers are classed bare, relative, or absolute.
//! `sourceMappingURL` is reported present, absent, or unreadable, and the map is
//! not decoded. ASAR members and `.node` add-ons are listed from the header.
//! A truncated or self-contradicting header refuses the listing.
//! Computed specifiers stay unknown. Nothing is executed, fetched, or treated as behaviour.
use super::{parse, Json, Json::*};

const MODULE_EDGE_LIMIT: usize = 64;
const MODULE_UNKNOWN_LIMIT: usize = 64;
const CENSUS_LIMIT: usize = 64;
const SOURCE_MAP_LIMIT: usize = 16;
const ASAR_ENTRY_LIMIT: usize = 128;
const SPECIFIER_LIMIT: usize = 512;
const STATEMENT_LIMIT: usize = 4096;
const TEMPLATE_LIMIT: usize = 8;
const REPORT_LIMIT: usize = 12 * 1024;
const NAME_LIMIT: usize = 512;

const MODULE_EDGE_CAP: &str = "javascript module-edge cap";
const MODULE_UNKNOWN_CAP: &str = "javascript module-unknown cap";
const CENSUS_CAP: &str = "javascript string-census cap";
const SOURCE_MAP_CAP: &str = "javascript source-map cap";
const ASAR_ENTRY_CAP: &str = "javascript asar-entry cap";
const SPECIFIER_CAP: &str = "javascript specifier-length cap";
const STATEMENT_CAP: &str = "javascript statement-bound cap";
const TEMPLATE_NEST_CAP: &str = "javascript template-nesting cap";
const REPORT_CAP: &str = "javascript report cap";
const MODULE_FILE_LIMIT: usize = 128;
const ADDON_LIMIT: usize = 64;
const MODULE_FILE_CAP: &str = "javascript module-file cap";
const ADDON_CAP: &str = "javascript native-addon cap";

const ASAR_LENGTH_REFUSAL: &str = "asar directory length exceeds the container";
const ASAR_UTF8_REFUSAL: &str = "asar directory is not UTF-8";
const MAP_DANGLING: &str = "source map reference has no target";
const MAP_UNRESOLVED: &str = "unresolved; the target is not in this observation node";
const MAP_INLINE: &str = "inline data target present in the directive; contents not decoded";
const MAP_PRESENT: &str = "present";
const MAP_ABSENT: &str = "absent";
const MAP_UNREADABLE: &str = "unreadable";
const MAP_REMOTE: &str = "remote source map was not fetched";
const NOT_MODULE: &str = "file is not a JavaScript module";
const ASAR_PREFIX: &str = "asar container was not read whole";
const ASAR_OVERFLOW: &str = "asar entry range overflows";
const ASAR_PAST_END: &str = "asar entry range exceeds the container";
const ASAR_OVERLAP: &str = "asar member ranges overlap";
const ASAR_BOTH: &str = "asar entry is both a file and a directory";
const ASAR_ESCAPE: &str = "asar entry path escapes the container";
const ASAR_DUPLICATE: &str = "asar entry name is duplicated";
const ADDON_UNKNOWN: &str = "native add-on ABI, exports, and whether the add-on loads are not in this file";
const SCRIPT_ADDON_UNKNOWN: &str = "a script file has no container member list; add-ons outside this file were not searched";
const NOT_UTF8: &str = "Retained bytes are not UTF-8 JavaScript source. Module graph and string census are empty.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Script,
    Package,
    Asar,
}

/// Whether a source-map file could be opened. The map bytes are never decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MapFile {
    Present,
    Absent,
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpecClass {
    Bare,
    Relative,
    Absolute,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Edge {
    offset: usize,
    form: &'static str,
    specifier: Option<String>,
    class: Option<SpecClass>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Unknown {
    offset: usize,
    reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Site {
    offset: usize,
    kind: &'static str,
    value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MapRef {
    offset: usize,
    target: String,
    resolution: &'static str,
    module: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModuleFile {
    name: String,
    size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Addon {
    name: String,
    size: u64,
    offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fact {
    field: &'static str,
    value: FactValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FactValue {
    Present(String),
    Absent,
    Unknown(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AsarEntry {
    name: String,
    size: u64,
    unpacked: bool,
    offset: u64,
}

pub(super) struct Observation {
    pub(super) text: String,
    graph: Vec<Edge>,
    unknowns: Vec<Unknown>,
    census: Vec<Site>,
    maps: Vec<MapRef>,
    map_unknowns: Vec<Unknown>,
    package: Vec<Fact>,
    package_withheld: Option<&'static str>,
    asar_entries: Vec<AsarEntry>,
    asar_refusal: Option<&'static str>,
    asar_header_end: Option<u64>,
    modules: Vec<ModuleFile>,
    addons: Vec<Addon>,
    caps: Vec<&'static str>,
    script_note: Option<&'static str>,
    saw_module: bool,
    byte_len: usize,
    label: String,
    kind: Kind,
    complete: bool,
}

enum Arg {
    Literal(String),
    Computed,
    Incomplete(&'static str),
}

enum Template {
    Literal(String),
    Dynamic,
    Unclosed,
}

enum MapNote {
    None,
    Unknown(&'static str),
    Ref(String, &'static str),
}

enum Word {
    Import,
    Export,
    Require,
    IpcMain,
    IpcRenderer,
    WebContents,
    Router,
    Other,
}

/// File-name classification for a retained observation node. The bytes are not opened here.
pub(super) fn node_kind(path: &std::path::Path) -> Option<Kind> {
    let name = path.file_name().and_then(|value| value.to_str())?;
    if name == "package.json" {
        return Some(Kind::Package);
    }
    match path.extension().and_then(|value| value.to_str()) {
        Some("js" | "mjs" | "cjs") => Some(Kind::Script),
        Some("asar") => Some(Kind::Asar),
        _ => None,
    }
}

/// Read one retained node. `complete` is false when the caller kept only a prefix.
pub(super) fn observe(bytes: &[u8], kind: Kind, complete: bool) -> Observation {
    observe_with(bytes, kind, complete, ".", &|_| None)
}

/// `map_file` answers a relative source-map path. `None` leaves the reference unresolved.
pub(super) fn observe_with(
    bytes: &[u8],
    kind: Kind,
    complete: bool,
    module_label: &str,
    map_file: &dyn Fn(&str) -> Option<MapFile>,
) -> Observation {
    let mut out = Observation {
        text: String::new(),
        graph: Vec::new(),
        unknowns: Vec::new(),
        census: Vec::new(),
        maps: Vec::new(),
        map_unknowns: Vec::new(),
        package: Vec::new(),
        package_withheld: None,
        asar_entries: Vec::new(),
        asar_refusal: None,
        asar_header_end: None,
        modules: Vec::new(),
        addons: Vec::new(),
        caps: Vec::new(),
        script_note: None,
        saw_module: false,
        byte_len: bytes.len(),
        label: module_label.to_string(),
        kind,
        complete,
    };
    match kind {
        Kind::Script => read_script_or_payload(bytes, complete, module_label, map_file, &mut out),
        Kind::Package => read_package(bytes, &mut out),
        Kind::Asar => read_asar(bytes, complete, &mut out),
    }
    out.text = render(&out);
    out
}

fn read_script_or_payload(
    bytes: &[u8],
    complete: bool,
    module_label: &str,
    map_file: &dyn Fn(&str) -> Option<MapFile>,
    out: &mut Observation,
) {
    if bytes.len() >= 8 && u32_at(bytes, 0) == Some(4) {
        match asar_directory(bytes) {
            Ok(directory) => {
                out.kind = Kind::Asar;
                if complete {
                    finish_asar(bytes, directory.entries, directory.header_end, out);
                } else {
                    out.asar_refusal = Some(ASAR_PREFIX);
                }
                return;
            }
            Err(_) => {}
        }
    }
    read_script(bytes, complete, module_label, out);
    apply_map_files(out, None, map_file);
}

fn note_cap(caps: &mut Vec<&'static str>, name: &'static str) {
    if !caps.contains(&name) {
        caps.push(name);
    }
}

fn read_script(bytes: &[u8], complete: bool, module_label: &str, out: &mut Observation) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        out.script_note = Some(NOT_UTF8);
        return;
    };
    let bom = text.starts_with('\u{feff}');
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let origin = if bom { 3 } else { 0 };
    lex_source(text, complete, origin, module_label, out);
    if out.saw_module {
        let name = if module_label.is_empty() { "." } else { module_label };
        out.modules.push(ModuleFile { name: name.to_string(), size: out.byte_len as u64 });
    }
}

fn lex_source(text: &str, complete: bool, origin: usize, module_name: &str, out: &mut Observation) {
    let mut lexer = Lexer {
        text,
        i: 0,
        complete,
        brace: 0,
        stop_brace: None,
        expr_end: false,
        template_depth: 0,
        origin,
        module_name,
        out,
    };
    if text.starts_with("#!") {
        lexer.i = text.find('\n').map(|at| at + 1).unwrap_or(text.len());
    }
    lexer.drive();
}

fn read_package(bytes: &[u8], out: &mut Observation) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        out.package_withheld = Some("Package entry facts withheld: retained bytes are not UTF-8. Absence is not claimed.");
        return;
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Ok(value) = parse(text) else {
        out.package_withheld = Some("Package entry facts withheld: retained bytes are not a JSON object. Absence is not claimed.");
        return;
    };
    if !matches!(value, Obj(_)) {
        out.package_withheld = Some("Package entry facts withheld: retained bytes are not a JSON object. Absence is not claimed.");
        return;
    }
    out.package = vec![
        Fact { field: "name", value: string_field(&value, "name") },
        Fact { field: "version", value: string_field(&value, "version") },
        Fact { field: "main", value: string_field(&value, "main") },
        Fact { field: "module", value: string_field(&value, "module") },
        Fact { field: "electron", value: electron_field(&value) },
    ];
}

fn string_field(root: &Json, key: &'static str) -> FactValue {
    match at(root, key) {
        None | Some(Null) => FactValue::Absent,
        Some(Str(value)) => FactValue::Present(value.clone()),
        Some(_) => FactValue::Unknown(match key {
            "name" => "name is not a string",
            "version" => "version is not a string",
            "main" => "main is not a string",
            "module" => "module is not a string",
            _ => "field is not a string",
        }),
    }
}

fn electron_field(root: &Json) -> FactValue {
    match at(root, "electron") {
        None | Some(Null) => FactValue::Absent,
        Some(Str(value)) => FactValue::Present(value.clone()),
        Some(Obj(map)) => match map.get("main") {
            None | Some(Null) => FactValue::Absent,
            Some(Str(value)) => FactValue::Present(value.clone()),
            Some(_) => FactValue::Unknown("electron.main is not a string"),
        },
        Some(_) => FactValue::Unknown("electron entry is not a string"),
    }
}

fn at<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Obj(map) => map.get(key),
        _ => None,
    }
}

fn read_asar(bytes: &[u8], complete: bool, out: &mut Observation) {
    if !complete {
        out.asar_refusal = Some(ASAR_PREFIX);
        return;
    }
    match asar_directory(bytes) {
        Ok(directory) => finish_asar(bytes, directory.entries, directory.header_end, out),
        Err(reason) => {
            out.asar_entries.clear();
            out.asar_refusal = Some(reason);
        }
    }
}

fn finish_asar(bytes: &[u8], mut entries: Vec<AsarEntry>, header_end: u64, out: &mut Observation) {
    let content_len = (bytes.len() as u64).saturating_sub(header_end);
    if let Err(reason) = certify_entries(&mut entries, content_len) {
        out.asar_entries.clear();
        out.asar_refusal = Some(reason);
        return;
    }
    let mut modules: Vec<AsarEntry> = entries.iter().filter(|entry| is_module_name(&entry.name)).cloned().collect();
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    if modules.len() > MODULE_FILE_LIMIT {
        note_cap(&mut out.caps, MODULE_FILE_CAP);
        modules.truncate(MODULE_FILE_LIMIT);
    }
    let mut addons: Vec<AsarEntry> = entries.iter().filter(|entry| is_addon_name(&entry.name)).cloned().collect();
    addons.sort_by(|left, right| left.name.cmp(&right.name));
    if addons.len() > ADDON_LIMIT {
        note_cap(&mut out.caps, ADDON_CAP);
        addons.truncate(ADDON_LIMIT);
    }
    out.modules = modules.iter().map(|entry| ModuleFile { name: entry.name.clone(), size: entry.size }).collect();
    out.addons = addons.iter().map(|entry| Addon { name: entry.name.clone(), size: entry.size, offset: entry.offset }).collect();
    for module in &modules {
        if module.unpacked {
            push_obs_unknown(out, member_origin(header_end, module), "module bytes are not in the container");
            continue;
        }
        let Some(slice) = member_bytes(bytes, header_end, module) else {
            push_obs_unknown(out, member_origin(header_end, module), "module bytes are not in the container");
            continue;
        };
        let Ok(text) = std::str::from_utf8(slice) else {
            push_obs_unknown(out, member_origin(header_end, module), "module bytes are not UTF-8");
            continue;
        };
        let bom = text.starts_with('\u{feff}');
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let origin = member_origin(header_end, module).saturating_add(if bom { 3 } else { 0 });
        lex_source(text, true, origin, &module.name, out);
    }
    apply_map_files(out, Some(&entries), &|_| None);
    if entries.len() > ASAR_ENTRY_LIMIT {
        note_cap(&mut out.caps, ASAR_ENTRY_CAP);
        entries.truncate(ASAR_ENTRY_LIMIT);
    }
    out.asar_header_end = Some(header_end);
    out.asar_entries = entries;
}

fn certify_entries(entries: &mut Vec<AsarEntry>, content_len: u64) -> Result<(), &'static str> {
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    if entries.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(ASAR_DUPLICATE);
    }
    let mut ranges = Vec::new();
    for entry in entries.iter() {
        if entry.unpacked {
            continue;
        }
        let end = entry.offset.checked_add(entry.size).ok_or(ASAR_OVERFLOW)?;
        if end > content_len {
            return Err(ASAR_PAST_END);
        }
        ranges.push((entry.offset, end));
    }
    ranges.sort_unstable();
    let mut last = 0u64;
    let mut seen = false;
    for (start, end) in ranges {
        if seen && start < last {
            return Err(ASAR_OVERLAP);
        }
        last = end;
        seen = true;
    }
    Ok(())
}

fn member_origin(header_end: u64, entry: &AsarEntry) -> usize {
    usize::try_from(header_end.saturating_add(entry.offset)).unwrap_or(usize::MAX)
}

fn member_bytes<'a>(bytes: &'a [u8], header_end: u64, entry: &AsarEntry) -> Option<&'a [u8]> {
    let start = usize::try_from(header_end.checked_add(entry.offset)?).ok()?;
    let end = start.checked_add(usize::try_from(entry.size).ok()?)?;
    bytes.get(start..end)
}

fn file_extension(name: &str) -> Option<&str> {
    let file = name.rsplit('/').next().unwrap_or(name);
    let (base, ext) = file.rsplit_once('.')?;
    if base.is_empty() || ext.is_empty() { None } else { Some(ext) }
}

fn is_module_name(name: &str) -> bool {
    matches!(file_extension(name), Some("js" | "mjs" | "cjs"))
}

fn is_addon_name(name: &str) -> bool {
    file_extension(name) == Some("node")
}

fn push_obs_unknown(out: &mut Observation, offset: usize, reason: &'static str) {
    if out.unknowns.len() >= MODULE_UNKNOWN_LIMIT {
        note_cap(&mut out.caps, MODULE_UNKNOWN_CAP);
        return;
    }
    out.unknowns.push(Unknown { offset, reason });
}

fn apply_map_files(out: &mut Observation, entries: Option<&[AsarEntry]>, map_file: &dyn Fn(&str) -> Option<MapFile>) {
    for map in &mut out.maps {
        if map.resolution != MAP_UNRESOLVED {
            continue;
        }
        if map.target.contains("://") {
            map.resolution = MAP_REMOTE;
            continue;
        }
        let status = if let Some(entries) = entries {
            Some(lookup_asar(entries, &map.module, &map.target))
        } else {
            map_file(&map.target)
        };
        map.resolution = match status {
            None => MAP_UNRESOLVED,
            Some(MapFile::Present) => MAP_PRESENT,
            Some(MapFile::Absent) => MAP_ABSENT,
            Some(MapFile::Unreadable) => MAP_UNREADABLE,
        };
    }
}

fn lookup_asar(entries: &[AsarEntry], module: &str, target: &str) -> MapFile {
    let Some(path) = archive_map_path(module, target) else {
        return MapFile::Unreadable;
    };
    match entries.iter().find(|entry| entry.name == path) {
        None => MapFile::Absent,
        Some(entry) if entry.unpacked => MapFile::Unreadable,
        Some(_) => MapFile::Present,
    }
}

fn archive_map_path(module: &str, target: &str) -> Option<String> {
    if target.is_empty() || target.starts_with('/') || target.contains('\\') || target.contains('\0') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = module.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("").split('/').filter(|part| !part.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("/")) }
}

struct Directory {
    header_end: u64,
    entries: Vec<AsarEntry>,
}

fn asar_directory(bytes: &[u8]) -> Result<Directory, &'static str> {
    if bytes.len() < 8 {
        return Err("asar header is shorter than the uint32 length blocks");
    }
    let size_payload = u32_at(bytes, 0).ok_or("asar header is shorter than the uint32 length blocks")?;
    if size_payload != 4 {
        return Err("asar size pickle is not a uint32 length block");
    }
    let header_len = u32_at(bytes, 4).ok_or("asar header is shorter than the uint32 length blocks")? as usize;
    if header_len > bytes.len().saturating_sub(8) {
        return Err(ASAR_LENGTH_REFUSAL);
    }
    let header = &bytes[8..8 + header_len];
    if header.len() < 8 {
        return Err(ASAR_LENGTH_REFUSAL);
    }
    let payload = u32_at(header, 0).ok_or(ASAR_LENGTH_REFUSAL)? as usize;
    let json_len = u32_at(header, 4).ok_or(ASAR_LENGTH_REFUSAL)? as usize;
    let aligned = align4(json_len);
    if payload != 4 + aligned || header_len != 8 + aligned || json_len > header.len().saturating_sub(8) {
        return Err("asar header pickle does not match the directory length");
    }
    let json_bytes = &header[8..8 + json_len];
    let json = std::str::from_utf8(json_bytes).map_err(|_| ASAR_UTF8_REFUSAL)?;
    let value = parse(json).map_err(|_| "asar directory JSON was refused")?;
    if !matches!(value, Obj(_)) {
        return Err("asar directory is not a JSON object");
    }
    let mut entries = Vec::new();
    walk(&value, "", &mut entries)?;
    Ok(Directory {
        header_end: (8 + header_len) as u64,
        entries,
    })
}

fn component_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.contains('\0') {
        return Err("asar entry name is empty");
    }
    if name.len() > NAME_LIMIT {
        return Err("asar entry name exceeds the name bound");
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(ASAR_ESCAPE);
    }
    Ok(())
}

fn walk(node: &Json, prefix: &str, entries: &mut Vec<AsarEntry>) -> Result<(), &'static str> {
    let files = at(node, "files").ok_or("asar directory has no files object")?;
    let Obj(map) = files else {
        return Err("asar directory files object is absent or not an object");
    };
    for (name, child) in map {
        component_name(name)?;
        let Obj(_) = child else {
            return Err("asar directory entry is not an object");
        };
        let directory = at(child, "files").is_some();
        let file = at(child, "size").is_some() || at(child, "offset").is_some();
        if directory && file {
            return Err(ASAR_BOTH);
        }
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if directory {
            walk(child, &path, entries)?;
        }
        if let Some(entry) = file_entry(child, path)? {
            entries.push(entry);
        }
    }
    Ok(())
}

fn file_entry(node: &Json, name: String) -> Result<Option<AsarEntry>, &'static str> {
    let has_size = at(node, "size").is_some();
    let has_offset = at(node, "offset").is_some();
    let has_unpacked = at(node, "unpacked").is_some();
    if !has_size && !has_offset {
        if has_unpacked && at(node, "files").is_none() {
            return Err("asar file entry lacks size and offset");
        }
        return Ok(None);
    }
    if !has_size || !has_offset {
        return Err("asar file entry lacks size and offset");
    }
    let size = json_u64(at(node, "size").unwrap()).ok_or("asar entry size is not an integer")?;
    let offset = json_u64(at(node, "offset").unwrap()).ok_or("asar entry offset is not a decimal string")?;
    let unpacked = match at(node, "unpacked") {
        None | Some(Null) => false,
        Some(Bool(value)) => *value,
        Some(_) => return Err("asar unpacked flag is not a boolean"),
    };
    Ok(Some(AsarEntry { name, size, unpacked, offset }))
}

fn json_u64(value: &Json) -> Option<u64> {
    let text = match value {
        Num(text) | Str(text) => text.as_str(),
        _ => return None,
    };
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn align4(value: usize) -> usize {
    value.saturating_add(3) & !3
}

struct Lexer<'a> {
    text: &'a str,
    i: usize,
    complete: bool,
    brace: i32,
    stop_brace: Option<i32>,
    expr_end: bool,
    template_depth: usize,
    origin: usize,
    module_name: &'a str,
    out: &'a mut Observation,
}

impl<'a> Lexer<'a> {
    fn drive(&mut self) {
        while self.i < self.text.len() {
            let before = self.i;
            if !self.gap() {
                return;
            }
            if self.i >= self.text.len() {
                return;
            }
            if self.one() {
                return;
            }
            if self.i == before {
                self.i += 1;
            }
        }
    }

    fn gap(&mut self) -> bool {
        loop {
            self.skip_ws();
            if self.i >= self.text.len() {
                return true;
            }
            if self.text.as_bytes().get(self.i..self.i + 2) == Some(b"//") {
                self.line_comment();
                continue;
            }
            if self.text.as_bytes().get(self.i..self.i + 2) == Some(b"/*") {
                if !self.block_comment() {
                    return false;
                }
                continue;
            }
            return true;
        }
    }

    fn skip_ws(&mut self) {
        while let Some(byte) = self.text.as_bytes().get(self.i).copied() {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b) {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn line_comment(&mut self) {
        let offset = self.i;
        self.i += 2;
        let begin = self.i;
        while self.i < self.text.len() && self.text.as_bytes()[self.i] != b'\n' {
            self.i += 1;
        }
        let note = classify_directive(&self.text[begin..self.i]);
        match note {
            MapNote::None => {}
            MapNote::Unknown(reason) => self.push_map_unknown(offset, reason),
            MapNote::Ref(target, resolution) => self.push_map(offset, target, resolution),
        }
    }

    fn block_comment(&mut self) -> bool {
        self.i += 2;
        while self.i + 1 < self.text.len() {
            if self.text.as_bytes().get(self.i..self.i + 2) == Some(b"*/") {
                self.i += 2;
                return true;
            }
            self.i += 1;
        }
        self.i = self.text.len();
        let reason = if self.complete {
            "block comment is unterminated in the retained bytes"
        } else {
            "block comment reaches the sample boundary"
        };
        self.push_unknown(self.i, reason);
        false
    }

    fn one(&mut self) -> bool {
        let byte = self.text.as_bytes()[self.i];
        match byte {
            b'\'' | b'"' => {
                let _ = self.decode_string(byte);
                self.expr_end = true;
                false
            }
            b'`' => {
                let _ = self.take_template();
                self.expr_end = true;
                false
            }
            b'/' => self.slash(),
            c if ident_start_byte(c) => self.word(),
            b'0'..=b'9' => {
                self.skip_number();
                self.expr_end = true;
                false
            }
            b'{' => {
                self.i += 1;
                self.brace += 1;
                self.expr_end = false;
                false
            }
            b'}' => {
                if self.stop_brace == Some(self.brace) {
                    self.i += 1;
                    self.brace -= 1;
                    return true;
                }
                if self.brace > 0 {
                    self.brace -= 1;
                }
                self.i += 1;
                self.expr_end = true;
                false
            }
            b'(' | b'[' => {
                self.i += 1;
                self.expr_end = false;
                false
            }
            b')' | b']' => {
                self.i += 1;
                self.expr_end = true;
                false
            }
            b';' => {
                self.i += 1;
                self.expr_end = false;
                false
            }
            _ => {
                let ch = self.text[self.i..].chars().next().unwrap_or('\u{fffd}');
                self.i += ch.len_utf8();
                self.expr_end = false;
                false
            }
        }
    }

    fn word(&mut self) -> bool {
        let start = self.i;
        let end = ident_end(self.text, start);
        if end == start {
            self.i += 1;
            return false;
        }
        let kind = classify_word(self.text, start, end);
        self.i = end;
        match kind {
            Word::Import => self.after_import(start),
            Word::Export => self.after_export(start),
            Word::Require => self.after_require(start),
            Word::IpcMain => self.after_member(start, "on", "ipcMain.on", "computed ipc channel"),
            Word::IpcRenderer => self.after_member(start, "send", "ipcRenderer.send", "computed ipc channel"),
            Word::WebContents => self.after_member(start, "send", "webContents.send", "computed ipc channel"),
            Word::Router => self.after_router(start),
            Word::Other => self.expr_end = true,
        }
        false
    }

    fn after_import(&mut self, start: usize) {
        if !self.gap() {
            return;
        }
        let next = self.text.as_bytes().get(self.i).copied();
        if next == Some(b'.') {
            self.expr_end = true;
            return;
        }
        if next == Some(b'(') {
            self.i += 1;
            self.out.saw_module = true;
            if !self.gap() {
                return;
            }
            match self.literal_argument() {
                Arg::Literal(value) => self.push_edge(start, "import()", Some(value)),
                Arg::Computed => self.push_unknown(start, "computed import specifier"),
                Arg::Incomplete(reason) => self.push_unknown(start, reason),
            }
            return;
        }
        if matches!(next, Some(b'\'' | b'"' | b'`')) {
            match self.literal_argument() {
                Arg::Literal(value) => self.push_edge(start, "import", Some(value)),
                Arg::Computed => self.push_unknown(start, "computed import specifier"),
                Arg::Incomplete(reason) => self.push_unknown(start, reason),
            }
            return;
        }
        self.from_clause(start, "import");
    }

    fn after_export(&mut self, start: usize) {
        if !self.gap() {
            return;
        }
        let next = self.text.as_bytes().get(self.i).copied();
        if next == Some(b'*') || next == Some(b'{') {
            self.from_clause(start, "export");
            return;
        }
        let end = ident_end(self.text, self.i);
        let word = self.text.get(self.i..end).unwrap_or("");
        if matches!(word, "function" | "class" | "const" | "let" | "var" | "async" | "default") {
            self.push_edge(start, "export", None);
            self.expr_end = false;
            return;
        }
        self.expr_end = true;
    }

    fn after_require(&mut self, start: usize) {
        let mark = self.i;
        if !self.gap() {
            self.expr_end = true;
            return;
        }
        if self.text.as_bytes().get(self.i) != Some(&b'(') {
            self.i = mark;
            self.expr_end = true;
            return;
        }
        self.i += 1;
        self.out.saw_module = true;
        if !self.gap() {
            return;
        }
        match self.literal_argument() {
            Arg::Literal(value) => self.push_edge(start, "require", Some(value)),
            Arg::Computed => self.push_unknown(start, "computed require specifier"),
            Arg::Incomplete(reason) => self.push_unknown(start, reason),
        }
    }

    fn after_member(&mut self, start: usize, method: &str, kind: &'static str, computed: &'static str) {
        let mark = self.i;
        if !self.looks_like_call(mark, method) {
            return;
        }
        match self.literal_argument() {
            Arg::Literal(value) => self.push_site(start, kind, value),
            Arg::Computed => self.push_unknown(start, computed),
            Arg::Incomplete(reason) => self.push_unknown(start, reason),
        }
    }

    fn after_router(&mut self, start: usize) {
        let mark = self.i;
        if !self.gap() {
            self.expr_end = true;
            return;
        }
        if self.text.as_bytes().get(self.i) != Some(&b'.') {
            self.i = mark;
            self.expr_end = true;
            return;
        }
        self.i += 1;
        if !self.gap() {
            return;
        }
        let end = ident_end(self.text, self.i);
        let kind = route_method(self.text.get(self.i..end).unwrap_or(""));
        let Some(kind) = kind else {
            self.i = mark;
            self.expr_end = true;
            return;
        };
        self.i = end;
        if !self.gap() {
            return;
        }
        if self.text.as_bytes().get(self.i) != Some(&b'(') {
            self.i = mark;
            self.expr_end = true;
            return;
        }
        self.i += 1;
        if !self.gap() {
            return;
        }
        match self.literal_argument() {
            Arg::Literal(value) => self.push_site(start, kind, value),
            Arg::Computed => self.push_unknown(start, "computed router path"),
            Arg::Incomplete(reason) => self.push_unknown(start, reason),
        }
    }

    fn looks_like_call(&mut self, mark: usize, method: &str) -> bool {
        if !self.gap() {
            self.expr_end = true;
            return false;
        }
        if self.text.as_bytes().get(self.i) != Some(&b'.') {
            self.i = mark;
            self.expr_end = true;
            return false;
        }
        self.i += 1;
        if !self.gap() {
            return false;
        }
        let end = ident_end(self.text, self.i);
        let matched = self.text.get(self.i..end) == Some(method);
        if !matched {
            self.i = mark;
            self.expr_end = true;
            return false;
        }
        self.i = end;
        if !self.gap() {
            return false;
        }
        if self.text.as_bytes().get(self.i) != Some(&b'(') {
            self.i = mark;
            self.expr_end = true;
            return false;
        }
        self.i += 1;
        self.gap()
    }

    fn from_clause(&mut self, start: usize, form: &'static str) {
        let limit = self.i.saturating_add(STATEMENT_LIMIT);
        let mut saw_from = false;
        while self.i < self.text.len() && self.i < limit {
            if !self.gap() {
                return;
            }
            if self.i >= self.text.len() {
                break;
            }
            let end = ident_end(self.text, self.i);
            if self.text.get(self.i..end) == Some("from") {
                self.i = end;
                saw_from = true;
                break;
            }
            if self.text.as_bytes().get(self.i) == Some(&b';') {
                break;
            }
            if !self.skip_clause_token() {
                break;
            }
        }
        if !saw_from {
            if self.i >= limit {
                self.note(STATEMENT_CAP);
                self.push_unknown(start, "module statement exceeds the observation bound");
            } else if form == "export" {
                self.push_edge(start, form, None);
            }
            return;
        }
        if !self.gap() {
            return;
        }
        // A static import/export specifier is a string. An identifier here is not a computed module edge.
        match self.literal_argument() {
            Arg::Literal(value) => self.push_edge(start, form, Some(value)),
            Arg::Computed => {}
            Arg::Incomplete(reason) => {
                self.out.saw_module = true;
                self.push_unknown(start, reason);
            }
        }
    }

    fn skip_clause_token(&mut self) -> bool {
        let Some(byte) = self.text.as_bytes().get(self.i).copied() else {
            return false;
        };
        match byte {
            b'{' => self.skip_braces(),
            b'\'' | b'"' => {
                let _ = self.decode_string(byte);
                true
            }
            b'`' => {
                let _ = self.take_template();
                true
            }
            c if ident_start_byte(c) => {
                self.i = ident_end(self.text, self.i);
                true
            }
            _ => {
                self.i += 1;
                true
            }
        }
    }

    fn skip_braces(&mut self) -> bool {
        if self.text.as_bytes().get(self.i) != Some(&b'{') {
            return false;
        }
        self.i += 1;
        let mut depth = 1i32;
        while self.i < self.text.len() && depth > 0 {
            if !self.gap() {
                return false;
            }
            let Some(byte) = self.text.as_bytes().get(self.i).copied() else {
                break;
            };
            match byte {
                b'{' => {
                    depth += 1;
                    self.i += 1;
                }
                b'}' => {
                    depth -= 1;
                    self.i += 1;
                }
                b'\'' | b'"' => {
                    let _ = self.decode_string(byte);
                }
                b'`' => {
                    let _ = self.take_template();
                }
                _ => self.i += 1,
            }
        }
        true
    }

    fn literal_argument(&mut self) -> Arg {
        let Some(byte) = self.text.as_bytes().get(self.i).copied() else {
            return Arg::Incomplete(self.boundary_reason());
        };
        match byte {
            b'\'' | b'"' => match self.decode_string(byte) {
                Ok(value) => {
                    if !self.gap() {
                        return Arg::Literal(value);
                    }
                    let next = self.text.as_bytes().get(self.i).copied();
                    if matches!(next, Some(b',' | b')' | b';') | None) {
                        Arg::Literal(value)
                    } else {
                        Arg::Computed
                    }
                }
                Err(reason) => Arg::Incomplete(reason),
            },
            b'`' => match self.take_template() {
                Template::Literal(value) => Arg::Literal(value),
                Template::Dynamic => Arg::Computed,
                Template::Unclosed => Arg::Incomplete(self.boundary_reason()),
            },
            _ => Arg::Computed,
        }
    }

    fn boundary_reason(&self) -> &'static str {
        if self.complete {
            "specifier string is incomplete in the retained bytes"
        } else {
            "specifier reaches the sample boundary"
        }
    }

    fn decode_string(&mut self, quote: u8) -> Result<String, &'static str> {
        self.i += 1;
        let mut out = String::new();
        while self.i < self.text.len() {
            let byte = self.text.as_bytes()[self.i];
            if byte == quote {
                self.i += 1;
                return Ok(out);
            }
            if byte == b'\n' || byte == b'\r' {
                self.abandon_string(quote);
                return Err(self.boundary_reason());
            }
            if byte == b'\\' {
                self.i += 1;
                let Some(escaped) = self.text.as_bytes().get(self.i).copied() else {
                    return Err(self.boundary_reason());
                };
                self.i += 1;
                match escaped {
                    b'\\' | b'\'' | b'"' | b'`' | b'/' => out.push(escaped as char),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'v' => out.push('\u{000b}'),
                    b'0' => out.push('\0'),
                    b'\n' => {}
                    b'\r' => {
                        if self.text.as_bytes().get(self.i) == Some(&b'\n') {
                            self.i += 1;
                        }
                    }
                    b'x' => {
                        let Some(value) = self.hex_value(2) else {
                            self.abandon_string(quote);
                            return Err("string escape is not decoded");
                        };
                        out.push(char::from_u32(value).unwrap_or('\u{fffd}'));
                    }
                    b'u' => match self.unicode_escape() {
                        Some(ch) => out.push(ch),
                        None => {
                            self.abandon_string(quote);
                            return Err("string escape is not decoded");
                        }
                    },
                    _ => {
                        self.abandon_string(quote);
                        return Err("string escape is not decoded");
                    }
                }
                continue;
            }
            let ch = self.text[self.i..].chars().next().unwrap();
            out.push(ch);
            self.i += ch.len_utf8();
        }
        Err(self.boundary_reason())
    }

    fn abandon_string(&mut self, quote: u8) {
        while let Some(byte) = self.text.as_bytes().get(self.i).copied() {
            self.i += 1;
            if byte == quote || byte == b'\n' {
                break;
            }
        }
    }

    fn hex_value(&mut self, digits: usize) -> Option<u32> {
        let text = self.text.get(self.i..self.i + digits)?;
        let value = u32::from_str_radix(text, 16).ok()?;
        self.i += digits;
        Some(value)
    }

    fn unicode_escape(&mut self) -> Option<char> {
        if self.text.as_bytes().get(self.i) == Some(&b'{') {
            self.i += 1;
            let start = self.i;
            while self.text.as_bytes().get(self.i).is_some_and(|byte| byte.is_ascii_hexdigit()) && self.i - start < 6 {
                self.i += 1;
            }
            if start == self.i || self.text.as_bytes().get(self.i) != Some(&b'}') {
                return None;
            }
            let value = u32::from_str_radix(&self.text[start..self.i], 16).ok()?;
            self.i += 1;
            return char::from_u32(value);
        }
        let value = self.hex_value(4)?;
        char::from_u32(value)
    }

    fn take_template(&mut self) -> Template {
        if self.template_depth >= TEMPLATE_LIMIT {
            self.note(TEMPLATE_NEST_CAP);
            self.push_unknown(self.i, "template nesting bound reached; remainder not read");
            self.i = self.text.len();
            return Template::Unclosed;
        }
        self.template_depth += 1;
        let value = self.take_template_inner();
        self.template_depth -= 1;
        value
    }

    fn take_template_inner(&mut self) -> Template {
        if self.text.as_bytes().get(self.i) != Some(&b'`') {
            return Template::Unclosed;
        }
        self.i += 1;
        let mut body = String::new();
        let mut dynamic = false;
        while self.i < self.text.len() {
            let byte = self.text.as_bytes()[self.i];
            if byte == b'\\' {
                self.i += 1;
                if self.i >= self.text.len() {
                    return Template::Unclosed;
                }
                let ch = self.text[self.i..].chars().next().unwrap();
                self.i += ch.len_utf8();
                if !dynamic {
                    body.push(ch);
                }
                continue;
            }
            if byte == b'`' {
                self.i += 1;
                return if dynamic { Template::Dynamic } else { Template::Literal(body) };
            }
            if self.text.as_bytes().get(self.i..self.i + 2) == Some(b"${") {
                dynamic = true;
                body.clear();
                self.i += 2;
                self.brace += 1;
                let saved = self.stop_brace;
                self.stop_brace = Some(self.brace);
                self.drive();
                self.stop_brace = saved;
                continue;
            }
            let ch = self.text[self.i..].chars().next().unwrap();
            self.i += ch.len_utf8();
            if !dynamic {
                body.push(ch);
            }
        }
        Template::Unclosed
    }

    fn slash(&mut self) -> bool {
        if !self.expr_end {
            self.i += 1;
            let mut escaped = false;
            let mut class = false;
            while self.i < self.text.len() {
                let byte = self.text.as_bytes()[self.i];
                self.i += 1;
                if escaped {
                    escaped = false;
                    continue;
                }
                if byte == b'\\' {
                    escaped = true;
                    continue;
                }
                if byte == b'\n' {
                    break;
                }
                if byte == b'[' {
                    class = true;
                    continue;
                }
                if byte == b']' {
                    class = false;
                    continue;
                }
                if byte == b'/' && !class {
                    break;
                }
            }
            while self.text.as_bytes().get(self.i).is_some_and(|byte| byte.is_ascii_alphabetic()) {
                self.i += 1;
            }
            self.expr_end = true;
            return false;
        }
        self.i += 1;
        self.expr_end = false;
        false
    }

    fn skip_number(&mut self) {
        let bytes = self.text.as_bytes();
        if bytes.get(self.i) == Some(&b'0') && matches!(bytes.get(self.i + 1), Some(b'x' | b'X' | b'b' | b'B' | b'o' | b'O')) {
            self.i += 2;
            while self.text.as_bytes().get(self.i).is_some_and(|byte| byte.is_ascii_hexdigit() || *byte == b'_') {
                self.i += 1;
            }
            return;
        }
        while self.text.as_bytes().get(self.i).is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_') {
            self.i += 1;
        }
        if self.text.as_bytes().get(self.i) == Some(&b'.') {
            self.i += 1;
            while self.text.as_bytes().get(self.i).is_some_and(|byte| byte.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.text.as_bytes().get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.text.as_bytes().get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            while self.text.as_bytes().get(self.i).is_some_and(|byte| byte.is_ascii_digit()) {
                self.i += 1;
            }
        }
    }

    fn at(&self, local: usize) -> usize {
        self.origin.saturating_add(local)
    }

    fn push_edge(&mut self, offset: usize, form: &'static str, specifier: Option<String>) {
        self.out.saw_module = true;
        let class = specifier.as_deref().map(spec_class);
        if let Some(value) = &specifier {
            if value.len() > SPECIFIER_LIMIT {
                self.note(SPECIFIER_CAP);
                self.push_unknown(offset, "specifier exceeds the length bound");
                return;
            }
        }
        if self.out.graph.len() >= MODULE_EDGE_LIMIT {
            self.note(MODULE_EDGE_CAP);
            return;
        }
        self.out.graph.push(Edge { offset: self.at(offset), form, specifier, class });
    }

    fn push_unknown(&mut self, offset: usize, reason: &'static str) {
        if self.out.unknowns.len() >= MODULE_UNKNOWN_LIMIT {
            self.note(MODULE_UNKNOWN_CAP);
            return;
        }
        self.out.unknowns.push(Unknown { offset: self.at(offset), reason });
    }

    fn push_site(&mut self, offset: usize, kind: &'static str, value: String) {
        if value.len() > SPECIFIER_LIMIT {
            self.note(SPECIFIER_CAP);
            self.push_unknown(offset, "string census value exceeds the length bound");
            return;
        }
        if self.out.census.len() >= CENSUS_LIMIT {
            self.note(CENSUS_CAP);
            return;
        }
        self.out.census.push(Site { offset: self.at(offset), kind, value });
    }

    fn push_map(&mut self, offset: usize, target: String, resolution: &'static str) {
        if target.len() > SPECIFIER_LIMIT {
            self.note(SPECIFIER_CAP);
            self.push_map_unknown(offset, "source map target exceeds the length bound");
            return;
        }
        if self.out.maps.len() + self.out.map_unknowns.len() >= SOURCE_MAP_LIMIT {
            self.note(SOURCE_MAP_CAP);
            return;
        }
        self.out.maps.push(MapRef { offset: self.at(offset), target, resolution, module: self.module_name.to_string() });
    }

    fn push_map_unknown(&mut self, offset: usize, reason: &'static str) {
        if self.out.maps.len() + self.out.map_unknowns.len() >= SOURCE_MAP_LIMIT {
            self.note(SOURCE_MAP_CAP);
            return;
        }
        self.out.map_unknowns.push(Unknown { offset: self.at(offset), reason });
    }

    fn note(&mut self, name: &'static str) {
        note_cap(&mut self.out.caps, name);
    }
}

fn classify_word(text: &str, start: usize, end: usize) -> Word {
    match text.get(start..end).unwrap_or("") {
        "import" => Word::Import,
        "export" => Word::Export,
        "require" => Word::Require,
        "ipcMain" => Word::IpcMain,
        "ipcRenderer" => Word::IpcRenderer,
        "webContents" => Word::WebContents,
        "router" => Word::Router,
        _ => Word::Other,
    }
}

fn route_method(name: &str) -> Option<&'static str> {
    match name {
        "get" => Some("router.get"),
        "post" => Some("router.post"),
        "put" => Some("router.put"),
        "patch" => Some("router.patch"),
        "delete" => Some("router.delete"),
        "all" => Some("router.all"),
        "use" => Some("router.use"),
        _ => None,
    }
}

fn ident_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic() || (!ch.is_ascii() && ch.is_alphabetic())
}

fn ident_continue(ch: char) -> bool {
    ident_start(ch) || ch.is_ascii_digit()
}

fn ident_end(text: &str, start: usize) -> usize {
    let mut index = start;
    let mut chars = text.get(start..).unwrap_or("").chars();
    let Some(first) = chars.next() else {
        return start;
    };
    if !ident_start(first) {
        return start;
    }
    index += first.len_utf8();
    for ch in chars {
        if !ident_continue(ch) {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

fn classify_directive(body: &str) -> MapNote {
    let trimmed = body.trim_start();
    let rest = if let Some(rest) = trimmed.strip_prefix('#') {
        rest
    } else if let Some(rest) = trimmed.strip_prefix('@') {
        rest
    } else {
        return MapNote::None;
    };
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix("sourceMappingURL") else {
        return MapNote::None;
    };
    if let Some(ch) = rest.chars().next() {
        if ch != '=' && !ch.is_whitespace() {
            return MapNote::None;
        }
    }
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix('=') else {
        return MapNote::Unknown(MAP_DANGLING);
    };
    let url = rest.trim().split_whitespace().next().unwrap_or("");
    if url.is_empty() {
        return MapNote::Unknown(MAP_DANGLING);
    }
    if url.starts_with("data:") {
        MapNote::Ref(url.to_string(), MAP_INLINE)
    } else {
        MapNote::Ref(url.to_string(), MAP_UNRESOLVED)
    }
}

fn spec_class(specifier: &str) -> SpecClass {
    if specifier == "." || specifier == ".." || specifier.starts_with("./") || specifier.starts_with("../") {
        SpecClass::Relative
    } else if specifier.starts_with('/') {
        SpecClass::Absolute
    } else {
        SpecClass::Bare
    }
}

fn class_word(class: SpecClass) -> &'static str {
    match class {
        SpecClass::Bare => "bare",
        SpecClass::Relative => "relative",
        SpecClass::Absolute => "absolute",
    }
}

fn display_name(name: &str) -> String {
    let raw = if name.is_empty() { "." } else { name };
    let mut cleaned = String::new();
    for ch in raw.chars() {
        cleaned.push(match ch {
            '"' | '\n' | '\r' => '\'',
            other => other,
        });
    }
    shown(&cleaned)
}

fn render(out: &Observation) -> String {
    let mut text = String::new();
    let mut caps = out.caps.clone();
    push(&mut text, &mut caps, "JavaScript/Electron observation");
    push(&mut text, &mut caps, "Input: retained observation bytes. No execution, no network, no decompilation, no behaviour claim.");
    let scope = if out.complete {
        "Scope: complete retained bytes."
    } else {
        "Scope: retained prefix only; bytes outside this node remain unknown."
    };
    push(&mut text, &mut caps, scope);
    for cap in &out.caps {
        push(&mut text, &mut caps, &format!("Refusal: {cap}."));
    }
    if let Some(note) = out.script_note {
        push(&mut text, &mut caps, note);
    }
    match out.kind {
        Kind::Script => render_script(out, &mut text, &mut caps),
        Kind::Package => render_package(out, &mut text, &mut caps),
        Kind::Asar => render_asar(out, &mut text, &mut caps),
    }
    text
}

fn render_script(out: &Observation, text: &mut String, caps: &mut Vec<&'static str>) {
    if out.script_note.is_some() {
        push(text, caps, "Kind: not claimed.");
    } else if out.saw_module {
        push(text, caps, "Kind: javascript module");
        push(text, caps, "Modules: 1.");
        push(text, caps, &format!("module \"{}\" size {}.", display_name(&out.label), out.byte_len));
    } else {
        push(text, caps, &format!("Refusal: {NOT_MODULE}."));
        push(text, caps, "Known: retained bytes contain no import, export declaration, or require call.");
        push(text, caps, "Unknown: the bytes were not executed.");
    }
    render_graph(out, text, caps);
    push(text, caps, "Native add-ons: unknown.");
    push(text, caps, &format!("Unknown: {SCRIPT_ADDON_UNKNOWN}."));
}

fn render_package(out: &Observation, text: &mut String, caps: &mut Vec<&'static str>) {
    push(text, caps, "Kind: package.json");
    push(text, caps, "Module graph and string census are not read for a package.json node.");
    if let Some(note) = out.package_withheld {
        push(text, caps, note);
        return;
    }
    for fact in &out.package {
        push(text, caps, &fact_line(fact));
    }
    if out.package.iter().any(|fact| fact.field == "electron" && matches!(fact.value, FactValue::Present(_))) {
        push(text, caps, "Payload: electron manifest field is present.");
    }
    push(text, caps, "Native add-ons: unknown.");
    push(text, caps, "Unknown: package.json does not list native add-on bytes.");
}

fn render_asar(out: &Observation, text: &mut String, caps: &mut Vec<&'static str>) {
    if let Some(reason) = out.asar_refusal {
        push(text, caps, &format!("Refusal: {reason}. No listing returned."));
        push(text, caps, "Modules: 0.");
        push(text, caps, "Native add-ons: unknown.");
        push(text, caps, "Unknown: the container was refused, so members, imports, source maps, and native add-ons were not read.");
        return;
    }
    push(text, caps, "Kind: electron app payload");
    push(text, caps, "Kind: asar container");
    push(text, caps, "Module bytes are read per member. They are not read as one concatenated script.");
    if let Some(header_end) = out.asar_header_end {
        push(text, caps, &format!("ASAR header ends at byte {header_end}. Content offsets are relative to that byte. Entries: {}.", out.asar_entries.len()));
        for entry in &out.asar_entries {
            let container = header_end.checked_add(entry.offset);
            let container = match container {
                Some(value) => value.to_string(),
                None => "unknown".into(),
            };
            push(
                text,
                caps,
                &format!(
                    "entry \"{}\" size {} unpacked {} content-offset {} container-offset {container}.",
                    display_name(&entry.name),
                    entry.size,
                    entry.unpacked,
                    entry.offset
                ),
            );
        }
    }
    push(text, caps, &format!("Modules: {}.", out.modules.len()));
    for module in &out.modules {
        push(text, caps, &format!("module \"{}\" size {}.", display_name(&module.name), module.size));
    }
    if out.caps.contains(&MODULE_FILE_CAP) {
        push(text, caps, "Unknown: module files past the javascript module-file cap were not read.");
    }
    render_graph(out, text, caps);
    push(text, caps, &format!("Native add-ons: {}.", out.addons.len()));
    for addon in &out.addons {
        push(text, caps, &format!("addon \"{}\" size {} content-offset {}.", display_name(&addon.name), addon.size, addon.offset));
    }
    push(text, caps, "Known: .node names are taken from the container member list.");
    push(text, caps, &format!("Unknown: {ADDON_UNKNOWN}."));
    if out.caps.contains(&ADDON_CAP) {
        push(text, caps, "Unknown: native add-on names past the javascript native-addon cap were not listed.");
    }
}

fn render_graph(out: &Observation, text: &mut String, caps: &mut Vec<&'static str>) {
    push(text, caps, &format!("Module graph: {} edges.", out.graph.len()));
    for edge in &out.graph {
        let target = match &edge.specifier {
            Some(value) => format!(" specifier \"{}\"", shown(value)),
            None => " local form, no external specifier".into(),
        };
        push(text, caps, &format!("{} at byte {}{target}.", edge.form, edge.offset));
    }
    push(text, caps, &format!("Unknowns: {}.", out.unknowns.len()));
    for unknown in &out.unknowns {
        push(text, caps, &format!("Unknown at byte {}: {}.", unknown.offset, unknown.reason));
    }
    push(text, caps, &format!("String census: {} sites.", out.census.len()));
    for site in &out.census {
        push(text, caps, &format!("{} at byte {} literal \"{}\".", site.kind, site.offset, shown(&site.value)));
    }
    push(text, caps, &format!("Source maps: {} references, {} unknowns. A dangling reference is an unknown, not an error. Which directive a runtime would select is not claimed. Map bytes are not decoded.", out.maps.len(), out.map_unknowns.len()));
    for map in &out.maps {
        push(text, caps, &format!("Reference at byte {} target \"{}\" resolution {}.", map.offset, shown(&map.target), map.resolution));
    }
    for unknown in &out.map_unknowns {
        push(text, caps, &format!("Unknown at byte {}: {}.", unknown.offset, unknown.reason));
    }
    let classed = out.graph.iter().filter(|edge| edge.specifier.is_some()).count();
    push(text, caps, &format!("Specifier class: {classed}."));
    for edge in &out.graph {
        let Some(value) = &edge.specifier else { continue };
        let word = edge.class.map(class_word).unwrap_or("unknown");
        push(text, caps, &format!("{} at byte {} specifier \"{}\" {word}.", edge.form, edge.offset, shown(value)));
    }
}

fn fact_line(fact: &Fact) -> String {
    match &fact.value {
        FactValue::Present(value) => format!("{}: present \"{}\"", fact.field, shown(value)),
        FactValue::Absent => format!("{}: absent", fact.field),
        FactValue::Unknown(reason) => format!("{}: unknown ({reason})", fact.field),
    }
}

fn push(text: &mut String, caps: &mut Vec<&'static str>, row: &str) {
    if caps.contains(&REPORT_CAP) {
        return;
    }
    let reserve = REPORT_CAP.len() + 12;
    if text.len().saturating_add(row.len()).saturating_add(1).saturating_add(reserve) > REPORT_LIMIT {
        text.push_str(&format!("Refusal: {REPORT_CAP}.\n"));
        caps.push(REPORT_CAP);
        return;
    }
    text.push_str(row);
    text.push('\n');
}

fn shown(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let part = if ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            format!("\\u{{{:x}}}", ch as u32)
        } else {
            ch.to_string()
        };
        if out.len() + part.len() > 256 {
            out.push('…');
            break;
        }
        out.push_str(&part);
    }
    out
}

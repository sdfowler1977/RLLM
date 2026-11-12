//! .NET metadata and CIL observations over inspected PE bytes.
//! Counts and names come from the bytes. Method bodies are not read and IL is not decoded.
#[cfg(test)]
use super::{obj, s, Json, Json::*};
use std::fmt::Write as _;

const TABLE_CAP: usize = 48;
const TABLE_CAP_NAME: &str = "dotnet metadata table cap";
const STREAM_CAP: usize = 16;
const STREAM_CAP_NAME: &str = "dotnet metadata stream cap";
const METHOD_ROW_CAP: usize = 4096;
const METHOD_ROW_CAP_NAME: &str = "dotnet method row cap";
const SECTION_CAP: usize = 96;
const NAME_CAP: usize = 256;
const VERSION_CAP: usize = 256;
#[cfg(test)]
const BODIES: &str = "not read";

#[derive(Clone, Copy)]
struct Heap {
    string: usize,
    guid: usize,
    blob: usize,
    uncompressed: bool,
}

#[derive(Clone, Copy)]
struct Section {
    va: u32,
    virtual_size: u32,
    raw: u32,
    raw_size: u32,
}

#[derive(Clone)]
struct StreamRow {
    name: String,
    offset: u32,
    size: u32,
}

struct TableRow {
    index: usize,
    name: &'static str,
    rows: u32,
}

struct MethodObs {
    declared: u32,
    read: u32,
    nonzero: u32,
}

enum TextField {
    Unread,
    Absent,
    Value(String),
}

struct Evidence {
    clr_rva: u32,
    clr_size: u32,
    directory_offset: u32,
    header_bytes: Option<u32>,
    signature: Option<&'static str>,
    version: TextField,
    streams: Vec<StreamRow>,
    streams_listed: bool,
    tables: Vec<TableRow>,
    tables_listed: bool,
    valid: Option<u64>,
    module_name: TextField,
    assembly_name: TextField,
    methods: Option<MethodObs>,
    caps: Vec<&'static str>,
    refusals: Vec<String>,
}

struct Cli {
    rva: u32,
    size: u32,
    directory_offset: u32,
    section_table: usize,
    section_count: usize,
    headers: u32,
}

enum Located {
    Absent,
    Present(Cli),
}

enum MapFail {
    OutsideSections,
    OutsideFile,
    Overlap,
    Unresolved,
}

fn rd_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let slice = bytes.get(at..end)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn rd_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let slice = bytes.get(at..end)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn rd_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    let slice = bytes.get(at..end)?;
    let mut buf = [0u8; 8];
    buf.copy_from_slice(slice);
    Some(u64::from_le_bytes(buf))
}

fn heap_from(flag: u8, uncompressed: bool) -> Heap {
    if uncompressed {
        Heap { string: 4, guid: 4, blob: 4, uncompressed: true }
    } else {
        Heap {
            string: if flag & 0x01 != 0 { 4 } else { 2 },
            guid: if flag & 0x02 != 0 { 4 } else { 2 },
            blob: if flag & 0x04 != 0 { 4 } else { 2 },
            uncompressed: false,
        }
    }
}

fn simple(rows: &[u32; 64], table: usize, heap: Heap) -> usize {
    if heap.uncompressed || rows[table] >= 0x10000 { 4 } else { 2 }
}

fn coded(rows: &[u32; 64], tag_bits: u32, tables: &[usize], heap: Heap) -> usize {
    let limit = 1u32 << (16 - tag_bits);
    if heap.uncompressed || tables.iter().any(|table| rows[*table] >= limit) { 4 } else { 2 }
}

/// Row stride for an ECMA-335 table. `#-` stores every index in four bytes.
fn row_size(index: usize, rows: &[u32; 64], heap: Heap) -> Option<usize> {
    // `#-` stores every index in four bytes, including heap indexes, even when HeapSizes is clear.
    let str_w = if heap.uncompressed { 4 } else { heap.string };
    let guid = if heap.uncompressed { 4 } else { heap.guid };
    let blob = if heap.uncompressed { 4 } else { heap.blob };
    let t = |table: usize| simple(rows, table, heap);
    let c = |bits: u32, tables: &[usize]| coded(rows, bits, tables, heap);
    let resolution = c(2, &[0x00, 0x1A, 0x23, 0x01]);
    let type_def_or_ref = c(2, &[0x02, 0x01, 0x1B]);
    let has_constant = c(2, &[0x04, 0x08, 0x17]);
    let has_custom = c(5, &[
        0x06, 0x04, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x00, 0x0E, 0x17, 0x14, 0x11, 0x1A, 0x1B, 0x20, 0x23, 0x26, 0x27,
        0x28, 0x2A, 0x2C, 0x2B,
    ]);
    let custom_type = c(3, &[0x06, 0x0A]);
    let has_marshal = c(1, &[0x04, 0x08]);
    let has_security = c(2, &[0x02, 0x06, 0x20]);
    let member_parent = c(3, &[0x02, 0x01, 0x1A, 0x06, 0x1B]);
    let has_semantics = c(1, &[0x14, 0x17]);
    let method_def_or_ref = c(1, &[0x06, 0x0A]);
    let member_forwarded = c(1, &[0x04, 0x06]);
    let implementation = c(2, &[0x26, 0x23, 0x27]);
    let type_or_method = c(1, &[0x02, 0x06]);
    Some(match index {
        0x00 => 2 + str_w + guid * 3,
        0x01 => resolution + str_w * 2,
        0x02 => 4 + str_w * 2 + type_def_or_ref + t(0x04) + t(0x06),
        0x03 => t(0x04),
        0x04 => 2 + str_w + blob,
        0x05 => t(0x06),
        0x06 => 4 + 2 + 2 + str_w + blob + t(0x08),
        0x07 => t(0x08),
        0x08 => 2 + 2 + str_w,
        0x09 => t(0x02) + type_def_or_ref,
        0x0A => member_parent + str_w + blob,
        0x0B => 1 + 1 + has_constant + blob,
        0x0C => has_custom + custom_type + blob,
        0x0D => has_marshal + blob,
        0x0E => 2 + has_security + blob,
        0x0F => 2 + 4 + t(0x02),
        0x10 => 4 + t(0x04),
        0x11 => blob,
        0x12 => t(0x02) + t(0x14),
        0x13 => t(0x14),
        0x14 => 2 + str_w + type_def_or_ref,
        0x15 => t(0x02) + t(0x17),
        0x16 => t(0x17),
        0x17 => 2 + str_w + blob,
        0x18 => 2 + t(0x06) + has_semantics,
        0x19 => t(0x02) + method_def_or_ref * 2,
        0x1A => str_w,
        0x1B => blob,
        0x1C => 2 + member_forwarded + str_w + t(0x1A),
        0x1D => 4 + t(0x04),
        0x1E => 4 + 4,
        0x1F => 4,
        0x20 => 4 + 2 * 4 + 4 + blob + str_w * 2,
        0x21 => 4,
        0x22 => 4 * 3,
        0x23 => 2 * 4 + 4 + blob + str_w * 2 + blob,
        0x24 => 4 + t(0x23),
        0x25 => 4 * 3 + t(0x23),
        0x26 => 4 + str_w + blob,
        0x27 => 4 + 4 + str_w * 2 + implementation,
        0x28 => 4 + 4 + str_w + implementation,
        0x29 => t(0x02) * 2,
        0x2A => 2 + 2 + type_or_method + str_w,
        0x2B => method_def_or_ref + blob,
        0x2C => t(0x2A) + type_def_or_ref,
        _ => return None,
    })
}

fn table_name(index: usize) -> &'static str {
    const NAMES: [&str; 0x2D] = [
        "Module",
        "TypeRef",
        "TypeDef",
        "FieldPtr",
        "Field",
        "MethodPtr",
        "MethodDef",
        "ParamPtr",
        "Param",
        "InterfaceImpl",
        "MemberRef",
        "Constant",
        "CustomAttribute",
        "FieldMarshal",
        "DeclSecurity",
        "ClassLayout",
        "FieldLayout",
        "StandAloneSig",
        "EventMap",
        "EventPtr",
        "Event",
        "PropertyMap",
        "PropertyPtr",
        "Property",
        "MethodSemantics",
        "MethodImpl",
        "ModuleRef",
        "TypeSpec",
        "ImplMap",
        "FieldRVA",
        "ENCLog",
        "ENCMap",
        "Assembly",
        "AssemblyProcessor",
        "AssemblyOS",
        "AssemblyRef",
        "AssemblyRefProcessor",
        "AssemblyRefOS",
        "File",
        "ExportedType",
        "ManifestResource",
        "NestedClass",
        "GenericParam",
        "MethodSpec",
        "GenericParamConstraint",
    ];
    NAMES.get(index).copied().unwrap_or("unlisted")
}

fn blank(cli: &Cli) -> Evidence {
    Evidence {
        clr_rva: cli.rva,
        clr_size: cli.size,
        directory_offset: cli.directory_offset,
        header_bytes: None,
        signature: None,
        version: TextField::Unread,
        streams: Vec::new(),
        streams_listed: false,
        tables: Vec::new(),
        tables_listed: false,
        valid: None,
        module_name: TextField::Unread,
        assembly_name: TextField::Unread,
        methods: None,
        caps: Vec::new(),
        refusals: Vec::new(),
    }
}

fn add_refusal(evidence: &mut Evidence, reason: String) {
    evidence.refusals.push(reason);
}

fn cap_line(name: &str) -> String {
    format!("Refusal: {name}.")
}

/// A PE whose data-directory count does not include entry 14 is not a CLI subject.
fn locate(bytes: &[u8]) -> Option<Located> {
    if bytes.get(0..2) != Some(b"MZ") {
        return None;
    }
    let lfanew = rd_u32(bytes, 0x3c)? as usize;
    if bytes.get(lfanew..lfanew.checked_add(4)?) != Some(b"PE\0\0") {
        return None;
    }
    let coff = lfanew + 4;
    let section_count = rd_u16(bytes, coff + 2)? as usize;
    let opt_size = rd_u16(bytes, coff + 16)? as usize;
    let opt = coff + 20;
    let magic = rd_u16(bytes, opt)?;
    let wide = match magic {
        0x10b => false,
        0x20b => true,
        _ => return None,
    };
    let dir_count_at = if wide { 108 } else { 92 };
    let dir_base = if wide { 112 } else { 96 };
    // PE32+ entry 14 is optional-header offset 224. PE32 is offset 208.
    let entry_at = dir_base + 14 * 8;
    let headers = rd_u32(bytes, opt + 60).unwrap_or(0);
    let section_table = opt.checked_add(opt_size)?;
    let cli_absent = Located::Absent;
    if opt_size < dir_count_at + 4 {
        return Some(cli_absent);
    }
    let count = rd_u32(bytes, opt + dir_count_at)?;
    if count < 15 || opt_size < entry_at + 8 {
        return Some(cli_absent);
    }
    let entry = opt + entry_at;
    let rva = rd_u32(bytes, entry)?;
    let size = rd_u32(bytes, entry + 4)?;
    Some(Located::Present(Cli {
        rva,
        size,
        directory_offset: entry_at as u32,
        section_table,
        section_count,
        headers,
    }))
}

fn sections_of(bytes: &[u8], cli: &Cli) -> (Vec<Section>, bool) {
    let mut sections = Vec::new();
    let mut complete = cli.section_count <= SECTION_CAP;
    let capped = cli.section_count.min(SECTION_CAP);
    for index in 0..capped {
        let at = match cli.section_table.checked_add(index * 40) {
            Some(at) => at,
            None => {
                complete = false;
                break;
            }
        };
        let Some(raw) = bytes.get(at..at + 40) else {
            complete = false;
            break;
        };
        sections.push(Section {
            virtual_size: u32::from_le_bytes(raw[8..12].try_into().unwrap()),
            va: u32::from_le_bytes(raw[12..16].try_into().unwrap()),
            raw_size: u32::from_le_bytes(raw[16..20].try_into().unwrap()),
            raw: u32::from_le_bytes(raw[20..24].try_into().unwrap()),
        });
    }
    (sections, complete)
}

enum WindowHit {
    Miss,
    Partial,
    Short,
    Mapped(usize, usize),
}

fn window_hit(bytes: &[u8], window: Section, rva: u32, size: u32) -> WindowHit {
    let Some(end) = rva.checked_add(size) else {
        return WindowHit::Short;
    };
    let Some(va_end) = window.va.checked_add(window.virtual_size) else {
        return WindowHit::Miss;
    };
    if end <= window.va || rva >= va_end {
        return WindowHit::Miss;
    }
    if rva < window.va || end > va_end {
        return WindowHit::Partial;
    }
    let delta = rva - window.va;
    let Some(file_at) = window.raw.checked_add(delta) else {
        return WindowHit::Short;
    };
    let Some(file_end) = file_at.checked_add(size) else {
        return WindowHit::Short;
    };
    let Some(raw_end) = window.raw.checked_add(window.raw_size) else {
        return WindowHit::Short;
    };
    if file_end > raw_end || file_end as usize > bytes.len() {
        return WindowHit::Short;
    }
    WindowHit::Mapped(file_at as usize, file_end as usize)
}

fn map_range<'a>(bytes: &'a [u8], sections: &[Section], headers: u32, complete: bool, rva: u32, size: u32) -> Result<&'a [u8], MapFail> {
    if !complete {
        return Err(MapFail::Unresolved);
    }
    if size == 0 {
        return Err(MapFail::OutsideFile);
    }
    let mut windows = Vec::with_capacity(sections.len() + 1);
    if headers > 0 {
        let raw_size = (bytes.len() as u32).min(headers);
        windows.push(Section { va: 0, virtual_size: headers, raw: 0, raw_size });
    }
    windows.extend_from_slice(sections);
    let mut mapped = None;
    let mut mapped_count = 0usize;
    let mut short = false;
    let mut partial = false;
    for window in windows {
        match window_hit(bytes, window, rva, size) {
            WindowHit::Miss => {}
            WindowHit::Partial => partial = true,
            WindowHit::Short => short = true,
            WindowHit::Mapped(start, end) => {
                mapped_count += 1;
                mapped = Some((start, end));
            }
        }
    }
    if mapped_count > 1 || (mapped_count == 1 && short) {
        return Err(MapFail::Overlap);
    }
    if let Some((start, end)) = mapped {
        return bytes.get(start..end).ok_or(MapFail::OutsideFile);
    }
    if short || partial {
        return Err(MapFail::OutsideFile);
    }
    Err(MapFail::OutsideSections)
}

fn clr_map_reason(fail: MapFail, rva: u32, size: u32) -> String {
    match fail {
        MapFail::OutsideSections => format!("Refusal: CLR data directory entry 14 RVA {rva:#x} maps outside every section."),
        MapFail::OutsideFile => format!("Refusal: CLR data directory entry 14 size {size} at RVA {rva:#x} points outside the mapped file."),
        MapFail::Overlap => format!("Refusal: CLR data directory entry 14 RVA {rva:#x} maps into overlapping sections."),
        MapFail::Unresolved => "Refusal: PE section table is incomplete; CLR runtime header mapping is unresolved.".into(),
    }
}

fn meta_map_reason(fail: MapFail, rva: u32, size: u32) -> String {
    match fail {
        MapFail::OutsideSections => format!("Refusal: metadata directory RVA {rva:#x} maps outside every section."),
        MapFail::OutsideFile => format!("Refusal: metadata directory size {size} at RVA {rva:#x} points outside the mapped file."),
        MapFail::Overlap => format!("Refusal: metadata directory RVA {rva:#x} maps into overlapping sections."),
        MapFail::Unresolved => "Refusal: PE section table is incomplete; metadata directory mapping is unresolved.".into(),
    }
}

fn version_text(bytes: &[u8]) -> Result<TextField, ()> {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    let text = &bytes[..end];
    if text.is_empty() {
        return Ok(TextField::Absent);
    }
    if text.len() > VERSION_CAP || !text.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return Err(());
    }
    Ok(TextField::Value(String::from_utf8(text.to_vec()).unwrap()))
}

fn heap_text(heap: &[u8], index: u32) -> Result<TextField, ()> {
    if index == 0 {
        return Ok(TextField::Absent);
    }
    let start = index as usize;
    if start >= heap.len() {
        return Err(());
    }
    let tail = &heap[start..];
    let Some(end) = tail.iter().position(|byte| *byte == 0) else {
        return Err(());
    };
    if end > NAME_CAP {
        return Err(());
    }
    let text = &tail[..end];
    if text.is_empty() {
        return Ok(TextField::Absent);
    }
    if !text.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return Err(());
    }
    Ok(TextField::Value(String::from_utf8(text.to_vec()).unwrap()))
}

enum HeaderFail {
    Truncated,
}

fn stream_header(meta: &[u8], at: usize) -> Result<(StreamRow, usize), HeaderFail> {
    let offset = rd_u32(meta, at).ok_or(HeaderFail::Truncated)?;
    let size = rd_u32(meta, at + 4).ok_or(HeaderFail::Truncated)?;
    let mut cursor = at + 8;
    let name_start = cursor;
    loop {
        let byte = *meta.get(cursor).ok_or(HeaderFail::Truncated)?;
        cursor += 1;
        if cursor - name_start > 32 {
            return Err(HeaderFail::Truncated);
        }
        if byte == 0 {
            break;
        }
    }
    let raw_len = cursor - at;
    let padded = raw_len.saturating_add(3) & !3;
    let header_end = at + padded;
    if header_end > meta.len() {
        return Err(HeaderFail::Truncated);
    }
    let name_bytes = &meta[name_start..cursor - 1];
    if !name_bytes.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return Err(HeaderFail::Truncated);
    }
    let name = String::from_utf8(name_bytes.to_vec()).map_err(|_| HeaderFail::Truncated)?;
    Ok((StreamRow { name, offset, size }, header_end))
}

fn stream_body<'a>(meta: &'a [u8], stream: &StreamRow) -> Option<&'a [u8]> {
    let start = stream.offset as usize;
    let end = start.checked_add(stream.size as usize)?;
    meta.get(start..end)
}

fn table_bodies(stream: &[u8]) -> Option<([u32; 64], u64, usize, u8)> {
    if stream.len() < 24 {
        return None;
    }
    let heaps = stream[6];
    let valid = rd_u64(stream, 8)?;
    let count = valid.count_ones() as usize;
    let header = 24usize.checked_add(count.checked_mul(4)?)?;
    if stream.len() < header {
        return None;
    }
    let mut rows = [0u32; 64];
    let mut cursor = 24usize;
    for index in 0..64 {
        if valid & (1u64 << index) == 0 {
            continue;
        }
        rows[index] = rd_u32(stream, cursor)?;
        cursor += 4;
    }
    Some((rows, valid, header, heaps))
}

fn body_offset(rows: &[u32; 64], valid: u64, heap: Heap, table: usize) -> Option<usize> {
    let mut offset = 0usize;
    for index in 0..table {
        if valid & (1u64 << index) == 0 {
            continue;
        }
        let stride = row_size(index, rows, heap)?;
        let count = rows[index] as usize;
        offset = offset.checked_add(stride.checked_mul(count)?)?;
    }
    Some(offset)
}

fn read_name(stream: &[u8], row_at: usize, stride: usize, name_at: usize, width: usize, strings: &[u8]) -> Result<TextField, ()> {
    let row_end = row_at.checked_add(stride).ok_or(())?;
    if row_end > stream.len() {
        return Err(());
    }
    let name_off = row_at.checked_add(name_at).ok_or(())?;
    if name_off.checked_add(width).is_none_or(|end| end > row_end) {
        return Err(());
    }
    let index = read_index(stream, name_off, width).ok_or(())?;
    heap_text(strings, index)
}

fn read_index(bytes: &[u8], at: usize, width: usize) -> Option<u32> {
    match width {
        2 => rd_u16(bytes, at).map(|value| u32::from(value)),
        4 => rd_u32(bytes, at),
        _ => None,
    }
}

fn method_obs(stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize) -> Result<MethodObs, &'static str> {
    if valid & (1u64 << 0x06) == 0 {
        return Ok(MethodObs { declared: 0, read: 0, nonzero: 0 });
    }
    let declared = rows[0x06];
    let stride = row_size(0x06, rows, heap).ok_or("MethodDef row width is unresolved.")?;
    let relative = body_offset(rows, valid, heap, 0x06).ok_or("MethodDef offset is unresolved.")?;
    let start = header.checked_add(relative).ok_or("MethodDef offset is unresolved.")?;
    let limit = declared.min(METHOD_ROW_CAP as u32);
    let mut read = 0u32;
    let mut nonzero = 0u32;
    for index in 0..limit {
        let Some(step) = (index as usize).checked_mul(stride) else {
            break;
        };
        let Some(at) = start.checked_add(step) else {
            break;
        };
        if at.checked_add(stride).is_none_or(|end| end > stream.len()) {
            break;
        }
        let rva = rd_u32(stream, at).ok_or("MethodDef RVA bytes are unavailable.")?;
        if rva != 0 {
            nonzero += 1;
        }
        read += 1;
    }
    if read < declared {
        if declared > METHOD_ROW_CAP as u32 {
            return Err(METHOD_ROW_CAP_NAME);
        }
        return Err("unread MethodDef rows are not counted and their bodies are not claimed.");
    }
    Ok(MethodObs { declared, read, nonzero })
}

fn strings_of<'a>(meta: &'a [u8], streams: &[StreamRow]) -> Option<&'a [u8]> {
    let stream = streams.iter().find(|stream| stream.name == "#Strings")?;
    stream_body(meta, stream)
}

fn fill_names(evidence: &mut Evidence, meta: &[u8], stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize) {
    let Some(strings) = strings_of(meta, &evidence.streams) else {
        if evidence.module_name_needed(valid, rows) || evidence.assembly_name_needed(valid, rows) {
            add_refusal(evidence, "Refusal: #Strings stream is absent; module and assembly names were not read.".into());
        } else {
            evidence.module_name = TextField::Absent;
            evidence.assembly_name = TextField::Absent;
        }
        return;
    };
    evidence.module_name = name_of(stream, rows, valid, heap, header, strings, 0x00, 2);
    evidence.assembly_name = name_of(stream, rows, valid, heap, header, strings, 0x20, 16 + heap.blob);
    if matches!(evidence.module_name, TextField::Unread) {
        add_refusal(evidence, "Refusal: module name bytes were not read.".into());
    }
    if matches!(evidence.assembly_name, TextField::Unread) {
        add_refusal(evidence, "Refusal: assembly name bytes were not read.".into());
    }
}

impl Evidence {
    fn module_name_needed(&self, valid: u64, rows: &[u32; 64]) -> bool {
        valid & 1 != 0 && rows[0x00] > 0
    }
    fn assembly_name_needed(&self, valid: u64, rows: &[u32; 64]) -> bool {
        valid & (1u64 << 0x20) != 0 && rows[0x20] > 0
    }
}

fn name_of(stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize, strings: &[u8], table: usize, name_at: usize) -> TextField {
    if valid & (1u64 << table) == 0 || rows[table] == 0 {
        return TextField::Absent;
    }
    let Some(stride) = row_size(table, rows, heap) else {
        return TextField::Unread;
    };
    let Some(relative) = body_offset(rows, valid, heap, table) else {
        return TextField::Unread;
    };
    let Some(row_at) = header.checked_add(relative) else {
        return TextField::Unread;
    };
    match read_name(stream, row_at, stride, name_at, heap.string, strings) {
        Ok(field) => field,
        Err(()) => TextField::Unread,
    }
}

fn list_tables(evidence: &mut Evidence, rows: &[u32; 64], valid: u64) {
    evidence.valid = Some(valid);
    evidence.tables_listed = true;
    for index in 0..64 {
        if valid & (1u64 << index) == 0 || rows[index] == 0 {
            continue;
        }
        if evidence.tables.len() >= TABLE_CAP {
            if !evidence.caps.contains(&TABLE_CAP_NAME) {
                evidence.caps.push(TABLE_CAP_NAME);
            }
            continue;
        }
        evidence.tables.push(TableRow { index, name: table_name(index), rows: rows[index] });
    }
}

fn read_table_stream(evidence: &mut Evidence, meta: &[u8], streams: &[StreamRow]) {
    let Some(table) = streams.iter().find(|stream| stream.name == "#~").or_else(|| streams.iter().find(|stream| stream.name == "#-")) else {
        evidence.tables_listed = true;
        evidence.module_name = TextField::Absent;
        evidence.assembly_name = TextField::Absent;
        evidence.methods = Some(MethodObs { declared: 0, read: 0, nonzero: 0 });
        return;
    };
    let Some(body) = stream_body(meta, table) else {
        add_refusal(evidence, format!("Refusal: {} stream extent points outside the metadata root. Table row counts were not read.", table.name));
        return;
    };
    let uncompressed = table.name == "#-";
    let Some((rows, valid, header, heaps)) = table_bodies(body) else {
        add_refusal(evidence, "Refusal: metadata table header truncated. No table row count is listed.".into());
        return;
    };
    let heap = heap_from(heaps, uncompressed);
    list_tables(evidence, &rows, valid);
    match method_obs(body, &rows, valid, heap, header) {
        Ok(obs) => evidence.methods = Some(obs),
        Err(reason) if reason == METHOD_ROW_CAP_NAME => {
            evidence.caps.push(METHOD_ROW_CAP_NAME);
            evidence.methods = partial_methods(body, &rows, valid, heap, header);
        }
        Err(reason) => {
            add_refusal(evidence, format!("Refusal: {reason}"));
            evidence.methods = partial_methods(body, &rows, valid, heap, header);
        }
    }
    fill_names(evidence, meta, body, &rows, valid, heap, header);
}

fn partial_methods(stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize) -> Option<MethodObs> {
    let declared = rows[0x06];
    let stride = row_size(0x06, rows, heap)?;
    let relative = body_offset(rows, valid, heap, 0x06)?;
    let start = header.checked_add(relative)?;
    let limit = declared.min(METHOD_ROW_CAP as u32);
    let mut read = 0u32;
    let mut nonzero = 0u32;
    for index in 0..limit {
        let step = (index as usize).checked_mul(stride)?;
        let at = start.checked_add(step)?;
        if at.checked_add(stride).is_none_or(|end| end > stream.len()) {
            break;
        }
        let rva = rd_u32(stream, at)?;
        if rva != 0 {
            nonzero += 1;
        }
        read += 1;
    }
    Some(MethodObs { declared, read, nonzero })
}

fn read_streams(evidence: &mut Evidence, meta: &[u8]) {
    if meta.len() < 16 || &meta[..4] != b"BSJB" {
        let reason = if meta.len() < 4 {
            "Refusal: metadata root truncated. No stream is listed."
        } else {
            "Refusal: metadata signature is not BSJB. No stream is listed."
        };
        add_refusal(evidence, reason.into());
        return;
    }
    evidence.signature = Some("BSJB");
    let version_len = match rd_u32(meta, 12) {
        Some(len) => len as usize,
        None => {
            add_refusal(evidence, "Refusal: metadata root truncated. No stream is listed.".into());
            return;
        }
    };
    if version_len > VERSION_CAP {
        add_refusal(evidence, "Refusal: metadata version length exceeds 256 bytes. Streams were not read.".into());
        return;
    }
    let flags_at = 16usize.checked_add(version_len).unwrap_or(usize::MAX);
    let count_at = flags_at.checked_add(2).unwrap_or(usize::MAX);
    if count_at.checked_add(2).is_none_or(|end| end > meta.len()) {
        add_refusal(evidence, "Refusal: metadata root truncated. No stream is listed.".into());
        return;
    }
    match version_text(&meta[16..16 + version_len]) {
        Ok(field) => evidence.version = field,
        Err(()) => {
            evidence.version = TextField::Unread;
            add_refusal(evidence, "Refusal: metadata version is not bounded ASCII.".into());
        }
    }
    let count = rd_u16(meta, count_at).unwrap_or(0) as usize;
    let mut cursor = count_at + 2;
    let mut streams = Vec::new();
    let limit = count.min(STREAM_CAP);
    for _ in 0..limit {
        match stream_header(meta, cursor) {
            Ok((stream, next)) => {
                cursor = next;
                streams.push(stream);
            }
            Err(HeaderFail::Truncated) => {
                add_refusal(evidence, "Refusal: metadata stream header truncated. No stream is listed.".into());
                return;
            }
        }
    }
    if count > STREAM_CAP {
        evidence.caps.push(STREAM_CAP_NAME);
    }
    evidence.streams = streams;
    evidence.streams_listed = true;
    if count > STREAM_CAP && !evidence.streams.iter().any(|stream| stream.name == "#~" || stream.name == "#-") {
        add_refusal(evidence, "Refusal: dotnet metadata stream cap. A table stream past the retained headers was not read.".into());
        return;
    }
    let listed = evidence.streams.clone();
    read_table_stream(evidence, meta, &listed);
}

fn examine(bytes: &[u8], cli: &Cli) -> Evidence {
    let mut evidence = blank(cli);
    let (sections, complete) = sections_of(bytes, cli);
    let header = match map_range(bytes, &sections, cli.headers, complete, cli.rva, cli.size) {
        Ok(header) => header,
        Err(fail) => {
            add_refusal(&mut evidence, clr_map_reason(fail, cli.rva, cli.size));
            return evidence;
        }
    };
    if header.len() >= 4 {
        evidence.header_bytes = rd_u32(header, 0);
    }
    if header.len() < 16 {
        add_refusal(&mut evidence, "Refusal: CLR runtime header does not cover the metadata directory.".into());
        return evidence;
    }
    let meta_rva = rd_u32(header, 8).unwrap_or(0);
    let meta_size = rd_u32(header, 12).unwrap_or(0);
    if meta_size == 0 {
        add_refusal(&mut evidence, "Refusal: metadata directory size is 0. Metadata root was not read.".into());
        return evidence;
    }
    let meta = match map_range(bytes, &sections, cli.headers, complete, meta_rva, meta_size) {
        Ok(meta) => meta,
        Err(fail) => {
            add_refusal(&mut evidence, meta_map_reason(fail, meta_rva, meta_size));
            return evidence;
        }
    };
    read_streams(&mut evidence, meta);
    evidence
}

#[cfg(test)]
fn field_json(field: &TextField) -> Json {
    match field {
        TextField::Unread => Null,
        TextField::Absent => s("absent"),
        TextField::Value(text) => s(text),
    }
}

#[cfg(test)]
fn num(value: impl ToString) -> Json {
    Num(value.to_string())
}

#[cfg(test)]
fn methods_json(methods: &MethodObs) -> Json {
    obj(vec![
        ("nonzero_rva", num(methods.nonzero)),
        ("rows_read", num(methods.read)),
        ("rows_declared", num(methods.declared)),
        ("bodies", s(BODIES)),
    ])
}

fn report(evidence: &Evidence) -> String {
    let mut out = String::new();
    let _ = writeln!(out, ".NET metadata and CIL observation");
    let _ = writeln!(
        out,
        "Scope: CLR runtime header (data directory entry 14), metadata signature and version, named streams, non-zero table row counts, module and assembly names, and the count of MethodDef rows whose RVA column is nonzero. Method bodies are not read. IL is not decoded. No behaviour is claimed."
    );
    let _ = writeln!(
        out,
        "CLR runtime header: data directory entry 14 at optional-header offset {}, RVA {:#x}, size {}.",
        evidence.directory_offset, evidence.clr_rva, evidence.clr_size
    );
    if let Some(cb) = evidence.header_bytes {
        let _ = writeln!(out, "CLR header cb {cb}.");
    }
    match &evidence.signature {
        Some(signature) => {
            let _ = writeln!(out, "Metadata signature {signature}.");
        }
        None => {}
    }
    match &evidence.version {
        TextField::Value(version) => {
            let _ = writeln!(out, "Metadata version {version}.");
        }
        TextField::Absent => {
            let _ = writeln!(out, "Metadata version absent.");
        }
        TextField::Unread => {}
    }
    if evidence.streams_listed {
        let _ = writeln!(out, "Streams {}.", evidence.streams.len());
        for stream in &evidence.streams {
            let _ = writeln!(out, "stream \"{}\" offset {} size {}.", stream.name, stream.offset, stream.size);
        }
    }
    if evidence.tables_listed {
        if let Some(valid) = evidence.valid {
            let _ = writeln!(out, "Table valid bits {valid:#x}.");
        } else {
            let _ = writeln!(out, "Table stream absent.");
        }
        let _ = writeln!(out, "Non-zero tables {}.", evidence.tables.len());
        for table in &evidence.tables {
            let _ = writeln!(out, "table 0x{:02x} {} rows {}.", table.index, table.name, table.rows);
        }
    }
    let _ = writeln!(out, "Module name: {}.", field_word(&evidence.module_name));
    let _ = writeln!(out, "Assembly name: {}.", field_word(&evidence.assembly_name));
    match &evidence.methods {
        Some(methods) => {
            let _ = writeln!(
                out,
                "MethodDef rows declared {}, rows read {}, nonzero RVA {}. Method bodies were not read. IL was not decoded. No behaviour is claimed.",
                methods.declared, methods.read, methods.nonzero
            );
        }
        None => {
            let _ = writeln!(out, "MethodDef RVA column was not read. Method bodies were not read. IL was not decoded. No behaviour is claimed.");
        }
    }
    for name in &evidence.caps {
        let _ = writeln!(out, "{}", cap_line(name));
    }
    for reason in &evidence.refusals {
        let _ = writeln!(out, "{reason}");
    }
    out
}

fn field_word(field: &TextField) -> String {
    match field {
        TextField::Unread => "not read".into(),
        TextField::Absent => "absent".into(),
        TextField::Value(text) => text.clone(),
    }
}

#[cfg(test)]
fn view(evidence: &Evidence) -> Json {
    let streams = evidence
        .streams
        .iter()
        .map(|stream| {
            obj(vec![
                ("name", s(&stream.name)),
                ("offset", num(stream.offset)),
                ("size", num(stream.size)),
            ])
        })
        .collect();
    let tables = evidence
        .tables
        .iter()
        .map(|table| {
            obj(vec![
                ("index", num(table.index)),
                ("name", s(table.name)),
                ("rows", num(table.rows)),
            ])
        })
        .collect();
    obj(vec![
        ("clr_rva", num(evidence.clr_rva)),
        ("clr_size", num(evidence.clr_size)),
        ("directory_offset", num(evidence.directory_offset)),
        ("header_bytes", evidence.header_bytes.map(num).unwrap_or(Null)),
        ("signature", evidence.signature.map(s).unwrap_or(Null)),
        ("version", field_json(&evidence.version)),
        ("streams", Arr(streams)),
        ("tables", Arr(tables)),
        ("valid", evidence.valid.map(num).unwrap_or(Null)),
        ("module", field_json(&evidence.module_name)),
        ("assembly", field_json(&evidence.assembly_name)),
        ("methods", evidence.methods.as_ref().map(methods_json).unwrap_or(Null)),
        ("caps", Arr(evidence.caps.iter().copied().map(s).collect())),
        ("refusal", evidence.refusals.first().map(|reason| s(reason)).unwrap_or(Null)),
        ("report", s(&report(evidence))),
    ])
}

fn present_cli(bytes: &[u8]) -> Option<Cli> {
    let Located::Present(cli) = locate(bytes)? else {
        return None;
    };
    if cli.rva == 0 && cli.size == 0 {
        return None;
    }
    Some(cli)
}

#[cfg(test)]
fn subject_view(bytes: &[u8]) -> Option<Json> {
    present_cli(bytes).map(|cli| view(&examine(bytes, &cli)))
}

/// None when the bytes are not a PE with a nonzero CLI data directory.
/// A present directory yields the observation text, including a named refusal.
pub(super) fn cli_evidence(bytes: &[u8]) -> Option<String> {
    present_cli(bytes).map(|cli| report(&examine(bytes, &cli)))
}

#[cfg(test)]
mod checks {
    use super::*;

    fn put16(buf: &mut [u8], at: usize, value: u16) {
        buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(buf: &mut [u8], at: usize, value: u32) {
        buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn push16(buf: &mut Vec<u8>, value: u16) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn push32(buf: &mut Vec<u8>, value: u32) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn pe(section: &[u8], cli_rva: u32, cli_size: u32, directories: u32) -> Vec<u8> {
        let mut file = vec![0u8; 0x200 + section.len()];
        file[0] = b'M';
        file[1] = b'Z';
        put32(&mut file, 0x3c, 0x80);
        file[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut file, 0x84, 0x8664);
        put16(&mut file, 0x86, 1);
        put16(&mut file, 0x94, 240);
        let optional = 0x98;
        put16(&mut file, optional, 0x20b);
        put32(&mut file, optional + 60, 0x200);
        put32(&mut file, optional + 108, directories);
        if directories >= 15 {
            put32(&mut file, optional + 224, cli_rva);
            put32(&mut file, optional + 228, cli_size);
        } else {
            put32(&mut file, optional + 224, 0x2000);
            put32(&mut file, optional + 228, 72);
        }
        let section_at = optional + 240;
        file[section_at..section_at + 5].copy_from_slice(b".text");
        put32(&mut file, section_at + 8, section.len() as u32);
        put32(&mut file, section_at + 12, 0x2000);
        put32(&mut file, section_at + 16, section.len() as u32);
        put32(&mut file, section_at + 20, 0x200);
        file[0x200..0x200 + section.len()].copy_from_slice(section);
        file
    }

    fn clr(metadata: &[u8]) -> Vec<u8> {
        clr_sized(metadata, metadata.len() as u32)
    }

    fn clr_sized(metadata: &[u8], metadata_size: u32) -> Vec<u8> {
        let mut section = vec![0u8; 72 + metadata.len()];
        put32(&mut section, 0, 72);
        put16(&mut section, 4, 2);
        put16(&mut section, 6, 5);
        put32(&mut section, 8, 0x2000 + 72);
        put32(&mut section, 12, metadata_size);
        section[72..].copy_from_slice(metadata);
        section
    }

    fn align4(value: usize) -> usize {
        value.saturating_add(3) & !3
    }

    fn metadata(version: &str, streams: &[(&str, &[u8])]) -> Vec<u8> {
        let mut version_bytes = version.as_bytes().to_vec();
        version_bytes.push(0);
        while version_bytes.len() % 4 != 0 {
            version_bytes.push(0);
        }
        let mut header_len = 16 + version_bytes.len() + 4;
        for (name, _) in streams {
            header_len += 8 + align4(name.len() + 1);
        }
        let mut cursor = align4(header_len);
        let mut placed = Vec::new();
        for (name, data) in streams {
            placed.push((cursor, *data, *name));
            cursor = align4(cursor + data.len());
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"BSJB");
        push16(&mut out, 1);
        push16(&mut out, 1);
        push32(&mut out, 0);
        push32(&mut out, version_bytes.len() as u32);
        out.extend_from_slice(&version_bytes);
        push16(&mut out, 0);
        push16(&mut out, streams.len() as u16);
        for (offset, data, name) in &placed {
            push32(&mut out, *offset as u32);
            push32(&mut out, data.len() as u32);
            let mut name_bytes = name.as_bytes().to_vec();
            name_bytes.push(0);
            while name_bytes.len() % 4 != 0 {
                name_bytes.push(0);
            }
            out.extend_from_slice(&name_bytes);
        }
        assert_eq!(out.len(), header_len);
        for (offset, data, _) in &placed {
            while out.len() < *offset {
                out.push(0);
            }
            assert_eq!(out.len(), *offset);
            out.extend_from_slice(data);
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }

    fn tilde(entries: &[(u8, u32)], heaps: u8, body: &[u8]) -> Vec<u8> {
        let mut valid = 0u64;
        for (index, _) in entries {
            valid |= 1u64 << index;
        }
        let mut out = Vec::new();
        push32(&mut out, 0);
        out.push(2);
        out.push(0);
        out.push(heaps);
        out.push(1);
        out.extend_from_slice(&valid.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        for (_, count) in entries {
            push32(&mut out, *count);
        }
        out.extend_from_slice(body);
        out
    }

    fn strings(items: &[&str]) -> (Vec<u8>, Vec<u32>) {
        let mut heap = vec![0u8];
        let mut indexes = Vec::new();
        for item in items {
            indexes.push(heap.len() as u32);
            heap.extend_from_slice(item.as_bytes());
            heap.push(0);
        }
        (heap, indexes)
    }

    fn image_of(metadata: &[u8]) -> Vec<u8> {
        pe(&clr(metadata), 0x2000, 72, 16)
    }

    #[test]
    fn native_pe_without_cli_header_is_not_a_subject() {
        // Fails if a CLI-less PE becomes an empty report, if BSJB bytes inside a section are sniffed
        // without data-directory entry 14, or if a 14-entry directory count is read at optional-header offset 224.
        let native = pe(b"BSJB is not a cli directory by itself", 0, 0, 16);
        assert_eq!(&native[..2], b"MZ");
        assert_eq!(&native[0x80..0x84], b"PE\0\0");
        assert!(subject_view(&native).is_none());
        let short_directory = pe(&[0u8; 16], 0, 0, 14);
        assert_eq!(&short_directory[0x80..0x84], b"PE\0\0");
        assert!(subject_view(&short_directory).is_none());
    }

    #[test]
    fn cli_rva_outside_every_section_is_refused() {
        // Fails if an unmapped RVA becomes None, if the section's own bytes are parsed anyway,
        // or if the refusal omits that the RVA maps outside every section.
        let bytes = pe(b"not-the-cli-header", 0x00ff_0000, 72, 16);
        let found = subject_view(&bytes).expect("a declared CLI directory is a subject");
        let refusal = found.get("refusal").unwrap().text().unwrap();
        assert!(refusal.contains("outside every section"), "{refusal}");
        assert!(refusal.contains("0xff0000"), "{refusal}");
        assert!(refusal.contains("entry 14"), "{refusal}");
        assert!(found.get("streams").unwrap().array().unwrap().is_empty());
        assert_eq!(found.get("methods").unwrap(), &Null);
        assert_eq!(found.get("signature").unwrap(), &Null);
        assert_eq!(found.get("clr_rva").unwrap(), &Num("16711680".into()));
        let rendered = found.string();
        assert!(!rendered.contains("BSJB"), "{rendered}");
        assert!(!rendered.contains("#~"), "{rendered}");
    }

    #[test]
    fn truncated_stream_header_lists_no_streams() {
        // Fails if the complete "#~" header before the cut is kept, or if bytes past the
        // metadata size are used to finish the cut header and publish BAITSTREAM.
        let mut meta = Vec::new();
        meta.extend_from_slice(b"BSJB");
        push16(&mut meta, 1);
        push16(&mut meta, 1);
        push32(&mut meta, 0);
        push32(&mut meta, 12);
        meta.extend_from_slice(b"v4.0.30319\0\0");
        push16(&mut meta, 0);
        push16(&mut meta, 2);
        push32(&mut meta, 0);
        push32(&mut meta, 0);
        meta.extend_from_slice(b"#~\0\0");
        push32(&mut meta, 0);
        assert_eq!(meta.len(), 48);
        let mut carried = meta.clone();
        push32(&mut carried, 0);
        carried.extend_from_slice(b"BAITSTREAM\0\0");
        let bytes = pe(&clr_sized(&carried, meta.len() as u32), 0x2000, 72, 16);
        let found = subject_view(&bytes).expect("truncated metadata is still a CLI subject");
        let refusal = found.get("refusal").unwrap().text().unwrap();
        assert!(refusal.contains("truncated"), "{refusal}");
        assert!(found.get("streams").unwrap().array().unwrap().is_empty());
        assert!(found.get("tables").unwrap().array().unwrap().is_empty());
        let rendered = found.string();
        assert!(!rendered.contains("#~"), "{rendered}");
        assert!(!rendered.contains("BAITSTREAM"), "{rendered}");
    }

    #[test]
    fn table_counts_keep_nonzero_rows_and_name_the_cap() {
        // Fails if a zero-row table is listed, if table 0x30 past the cap is listed,
        // or if the cut is reported without the cap name.
        let mut entries = Vec::new();
        for index in 0..49 {
            let rows = if index == 48 { 3 } else { 1 };
            entries.push((index as u8, rows));
        }
        entries.push((60, 0));
        let stream = tilde(&entries, 0, &[]);
        let root = metadata("v4.0.30319", &[("#~", stream.as_slice())]);
        let found = subject_view(&image_of(&root)).expect("table header is a CLI subject");
        let tables = found.get("tables").unwrap().array().unwrap();
        assert_eq!(tables.len(), TABLE_CAP);
        assert!(tables.iter().all(|table| table.get("rows").unwrap() != &Num("0".into())));
        assert!(tables.iter().all(|table| table.get("index").unwrap() != &Num("48".into())));
        assert!(tables.iter().all(|table| table.get("index").unwrap() != &Num("60".into())));
        assert_eq!(tables[0].get("index").unwrap(), &Num("0".into()));
        assert_eq!(tables[TABLE_CAP - 1].get("index").unwrap(), &Num("47".into()));
        let caps = found.get("caps").unwrap().array().unwrap();
        assert!(caps.iter().any(|cap| cap == &s(TABLE_CAP_NAME)), "{caps:?}");
        let report = found.get("report").unwrap().text().unwrap();
        assert!(report.contains(TABLE_CAP_NAME), "{report}");
        assert!(report.contains("table 0x00 "), "{report}");
        assert!(report.contains("table 0x2f "), "{report}");
        assert!(!report.contains("table 0x30 "), "{report}");
        assert!(!report.contains("table 0x3c "), "{report}");
        assert!(!report.contains("rows 3."), "{report}");
    }

    #[test]
    fn inert_bytes_are_not_a_subject() {
        // Fails if a non-PE byte string becomes an empty .NET report or a refusal object.
        assert!(subject_view(b"not a pe image").is_none());
        assert!(subject_view(b"").is_none());
        assert!(subject_view(b"MZ").is_none());
    }

    #[test]
    fn named_assembly_reports_streams_version_and_unread_bodies() {
        // Fails if the zero RVA row is counted, if a name is swapped between module and assembly,
        // if the zero-row TypeRef table is listed, or if a method body is decoded.
        let (heap, indexes) = strings(&["DemoModule", "DemoAssembly"]);
        let mut body = Vec::new();
        push16(&mut body, 0);
        push16(&mut body, indexes[0] as u16);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push32(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 1);
        push32(&mut body, 0x2000);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 1);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push32(&mut body, 0);
        push16(&mut body, 1);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, 0);
        push32(&mut body, 0);
        push16(&mut body, 0);
        push16(&mut body, indexes[1] as u16);
        push16(&mut body, 0);
        assert_eq!(body.len(), 66);
        let stream = tilde(&[(0x00, 1), (0x01, 0), (0x06, 2), (0x08, 1), (0x20, 1)], 0, &body);
        let root = metadata("v4.0.30319", &[("#~", stream.as_slice()), ("#Strings", heap.as_slice())]);
        let found = subject_view(&image_of(&root)).expect("named assembly is a CLI subject");
        assert_eq!(found.get("refusal").unwrap(), &Null);
        assert!(found.get("caps").unwrap().array().unwrap().is_empty());
        assert_eq!(found.get("directory_offset").unwrap(), &Num("224".into()));
        assert_eq!(found.get("version").unwrap().text().unwrap(), "v4.0.30319");
        assert_eq!(found.get("signature").unwrap().text().unwrap(), "BSJB");
        assert_eq!(found.get("module").unwrap().text().unwrap(), "DemoModule");
        assert_eq!(found.get("assembly").unwrap().text().unwrap(), "DemoAssembly");
        let streams = found.get("streams").unwrap().array().unwrap();
        assert_eq!(streams[0].get("name").unwrap().text().unwrap(), "#~");
        assert_eq!(streams[1].get("name").unwrap().text().unwrap(), "#Strings");
        let tables = found.get("tables").unwrap().array().unwrap();
        assert!(tables.iter().all(|table| table.get("name").unwrap().text().unwrap() != "TypeRef"));
        let method = tables.iter().find(|table| table.get("name").unwrap().text().unwrap() == "MethodDef").unwrap();
        assert_eq!(method.get("rows").unwrap(), &Num("2".into()));
        let methods = found.get("methods").unwrap();
        assert_eq!(methods.get("nonzero_rva").unwrap(), &Num("1".into()));
        assert_eq!(methods.get("rows_read").unwrap(), &Num("2".into()));
        assert_eq!(methods.get("rows_declared").unwrap(), &Num("2".into()));
        assert_eq!(methods.get("bodies").unwrap().text().unwrap(), "not read");
        let report = found.get("report").unwrap().text().unwrap();
        assert!(report.contains("No behaviour is claimed."), "{report}");
        assert!(report.contains("IL was not decoded"), "{report}");
        assert!(!report.contains("ldarg"), "{report}");
    }

    #[test]
    fn method_row_width_tracks_param_rows_heap_flags_and_custom_attribute_tag() {
        // Fails if ParamList stays 2 bytes at 65536 Param rows, if HeapSizes is ignored,
        // or if HasCustomAttribute uses a tag width other than 5.
        let narrow = Heap { string: 2, guid: 2, blob: 2, uncompressed: false };
        let mut rows = [0u32; 64];
        rows[0x08] = 1;
        assert_eq!(row_size(0x00, &rows, narrow), Some(10));
        assert_eq!(row_size(0x06, &rows, narrow), Some(14));
        assert_eq!(row_size(0x20, &rows, narrow), Some(22));
        assert_eq!(row_size(0x0B, &rows, narrow), Some(6));
        rows[0x08] = 65536;
        assert_eq!(row_size(0x06, &rows, narrow), Some(16));
        rows[0x08] = 1;
        let wide = Heap { string: 4, guid: 2, blob: 4, uncompressed: false };
        assert_eq!(row_size(0x06, &rows, wide), Some(18));
        let raw = Heap { string: 2, guid: 2, blob: 2, uncompressed: true };
        assert_eq!(row_size(0x06, &rows, raw), Some(20));
        rows[0x06] = 2048;
        assert_eq!(row_size(0x0C, &rows, narrow), Some(8));
        rows[0x06] = 2047;
        assert_eq!(row_size(0x0C, &rows, narrow), Some(6));
    }

    fn version_root() -> Vec<u8> {
        let version = b"v4.0.30319\0\0";
        let mut out = Vec::new();
        out.extend_from_slice(b"BSJB");
        push16(&mut out, 1);
        push16(&mut out, 1);
        push32(&mut out, 0);
        push32(&mut out, version.len() as u32);
        out.extend_from_slice(version);
        push16(&mut out, 0);
        push16(&mut out, 0);
        out
    }

    fn headed(body: &str) -> String {
        format!(".NET metadata and CIL observation\nScope: CLR runtime header (data directory entry 14), metadata signature and version, named streams, non-zero table row counts, module and assembly names, and the count of MethodDef rows whose RVA column is nonzero. Method bodies are not read. IL is not decoded. No behaviour is claimed.\n{body}")
    }

    #[test]
    fn pe_without_cli_directory_yields_no_dotnet_text() {
        // Fails if a 14-entry directory count is read at optional-header offset 224,
        // or if BSJB bytes in a section become a .NET report without entry 14.
        let native = pe(b"BSJB is not a cli directory by itself", 0, 0, 14);
        assert_eq!(&native[..2], b"MZ");
        assert_eq!(cli_evidence(&native), None);
        assert_eq!(subject_view(&native), None);
        assert_eq!(cli_evidence(&pe(&[0u8; 16], 0, 0, 16)), None);
    }

    #[test]
    fn cli_directory_yields_exact_metadata_lines() {
        // Fails if a declared CLI directory is dropped, or if the published lines
        // omit the BSJB signature, version, or the unread-body statement.
        let bytes = image_of(&version_root());
        let text = cli_evidence(&bytes).expect("a zero-stream CLI directory is a subject");
        let view = subject_view(&bytes).expect("json view stays aligned with the text");
        assert_eq!(view.get("report").unwrap().text().unwrap(), text);
        assert_eq!(
            text,
            headed("CLR runtime header: data directory entry 14 at optional-header offset 224, RVA 0x2000, size 72.\nCLR header cb 72.\nMetadata signature BSJB.\nMetadata version v4.0.30319.\nStreams 0.\nTable stream absent.\nNon-zero tables 0.\nModule name: absent.\nAssembly name: absent.\nMethodDef rows declared 0, rows read 0, nonzero RVA 0. Method bodies were not read. IL was not decoded. No behaviour is claimed.\n")
        );
        assert_eq!(text.matches("Metadata signature BSJB.\n").count(), 1);
        assert_eq!(text.matches("Metadata version v4.0.30319.\n").count(), 1);
        assert_eq!(text.matches(".NET metadata and CIL observation\n").count(), 1);
    }

    #[test]
    fn inconsistent_cli_extent_is_refused_by_name() {
        // Fails if a CLI size past the section virtual extent is dropped, sniffed, or refused without the directory name.
        let bytes = pe(b"short", 0x2000, 72, 16);
        let text = cli_evidence(&bytes).expect("an inconsistent CLI extent is still a subject");
        assert_eq!(
            text,
            headed("CLR runtime header: data directory entry 14 at optional-header offset 224, RVA 0x2000, size 72.\nModule name: not read.\nAssembly name: not read.\nMethodDef RVA column was not read. Method bodies were not read. IL was not decoded. No behaviour is claimed.\nRefusal: CLR data directory entry 14 size 72 at RVA 0x2000 points outside the mapped file.\n")
        );
        assert_eq!(text.matches("Refusal: CLR data directory entry 14 size 72 at RVA 0x2000 points outside the mapped file.\n").count(), 1);
        assert_eq!(text.matches("Metadata signature BSJB.\n").count(), 0);
    }

    #[test]
    fn truncated_metadata_stream_names_the_unread_header() {
        // Fails if bytes past the metadata size finish the cut stream header and publish BAITSTREAM,
        // or if the refusal omits the unread metadata stream header.
        let mut meta = Vec::new();
        meta.extend_from_slice(b"BSJB");
        push16(&mut meta, 1);
        push16(&mut meta, 1);
        push32(&mut meta, 0);
        push32(&mut meta, 12);
        meta.extend_from_slice(b"v4.0.30319\0\0");
        push16(&mut meta, 0);
        push16(&mut meta, 2);
        push32(&mut meta, 0);
        push32(&mut meta, 0);
        meta.extend_from_slice(b"#~\0\0");
        push32(&mut meta, 0);
        assert_eq!(meta.len(), 48);
        let mut carried = meta.clone();
        push32(&mut carried, 0);
        carried.extend_from_slice(b"BAITSTREAM\0\0");
        let bytes = pe(&clr_sized(&carried, meta.len() as u32), 0x2000, 72, 16);
        let text = cli_evidence(&bytes).expect("truncated metadata is still a CLI subject");
        assert_eq!(
            text,
            headed("CLR runtime header: data directory entry 14 at optional-header offset 224, RVA 0x2000, size 72.\nCLR header cb 72.\nMetadata signature BSJB.\nMetadata version v4.0.30319.\nModule name: not read.\nAssembly name: not read.\nMethodDef RVA column was not read. Method bodies were not read. IL was not decoded. No behaviour is claimed.\nRefusal: metadata stream header truncated. No stream is listed.\n")
        );
        assert_eq!(text.matches("Refusal: metadata stream header truncated. No stream is listed.\n").count(), 1);
        assert_eq!(text.matches("BAITSTREAM").count(), 0);
        assert_eq!(text.matches("#~").count(), 0);
    }
}

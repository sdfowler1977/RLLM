//! .NET metadata and CIL observations over inspected PE bytes.
//! Counts, names, and CIL instructions come from the bytes.
//! A tiny or fat method header is classified from its flag byte.
//! Instruction bytes are decoded up to the named caps. No behaviour is claimed.
const TABLE_CAP: usize = 48;
const TABLE_CAP_NAME: &str = "dotnet metadata table cap";
const STREAM_CAP: usize = 16;
const STREAM_CAP_NAME: &str = "dotnet metadata stream cap";
pub(super) const METHOD_ROW_CAP: usize = 4096;
const METHOD_ROW_CAP_NAME: &str = "dotnet method row cap";
const SECTION_CAP: usize = 96;
const NAME_CAP: usize = 256;
const VERSION_CAP: usize = 256;
/// Observation text, including the refusal that names this cap.
pub(super) const OBSERVATION_BYTE_CAP: usize = 16 * 1024;
const OBSERVATION_BYTE_CAP_NAME: &str = "dotnet observation byte cap";
const OBSERVATION_NOTICE: &str = "Refusal: dotnet observation byte cap.\nRefusal: dotnet observation byte cap. Later observation lines were not emitted.\n";
/// Bytes charged for ranges actually mapped and for table, row, header, and name bytes actually read.
/// A declared size that is not backed by those bytes is not charged.
pub(super) const PARSE_WORK_CAP: usize = 1024 * 1024;
const PARSE_WORK_CAP_NAME: &str = "dotnet parse work cap";
const WORK_UNREAD: &str = "Refusal: dotnet parse work cap. Remaining streams, tables, rows, and names were not read.";
const DECLARED_SIZE_CAP_NAME: &str = "dotnet declared size cap";
const DECLARED_UNREAD: &str = "Refusal: dotnet declared size cap. The declared range was not scanned.";
/// Instructions walked in one method body. The next instruction is not decoded.
const INSTRUCTION_CAP: usize = 128;
const INSTRUCTION_CAP_NAME: &str = "dotnet instruction cap";
/// Method bodies whose code bytes are walked in one assembly.
const BODY_DECODE_CAP: usize = 64;
const BODY_DECODE_CAP_NAME: &str = "dotnet method body cap";
const BODY_DECODE_UNREAD: &str = "Refusal: dotnet method body cap. Later method bodies were not decoded.";
/// Exception clauses kept from one well-formed section. Later clauses are not read.
const CLAUSE_CAP: usize = 16;
const CLAUSE_CAP_NAME: &str = "dotnet exception clause cap";
/// `switch` targets consumed. A larger declared count is not consumed, so the walk cannot drift.
const SWITCH_TARGET_CAP: usize = 16;
const SWITCH_TARGET_CAP_NAME: &str = "dotnet switch target cap";
/// Declared extra-section data size, including its 4-byte header. A larger section is not mapped.
const EH_BYTE_CAP: usize = 4096;
/// Extra sections followed from one method's MoreSects chain.
const EH_CHAIN_CAP: usize = 4;
const EH_CAP_NAME: &str = "dotnet exception section cap";
const SCOPE: &str = "Scope: CLR runtime header (data directory entry 14), metadata signature and version, named streams, non-zero table row counts, module and assembly names, the count of MethodDef rows whose RVA column is nonzero, and CIL method bodies up to the instruction, body, clause, and switch caps. A decoded instruction records the opcode, the IL offset, the length, the operand, and any branch or switch target. An exception clause records its kind and its try and handler offsets. No behaviour is claimed.";

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
    rvas: Vec<u32>,
    bodies_decoded: u32,
    instructions: u32,
}

fn method_rows(declared: u32, read: u32, nonzero: u32, rvas: Vec<u32>) -> MethodObs {
    MethodObs { declared, read, nonzero, rvas, bodies_decoded: 0, instructions: 0 }
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
    heap_lines: Vec<String>,
    method_headers: Vec<String>,
    zero_rows: Vec<usize>,
    caps: Vec<&'static str>,
    refusals: Vec<String>,
    stored_bytes: usize,
    output_omitted: bool,
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
    Cancelled,
    Work,
    Declared,
}

enum Halt {
    Cancel,
    Work,
}

struct Budget<'a> {
    left: usize,
    hit: bool,
    cancelled: &'a dyn Fn() -> bool,
}

impl<'a> Budget<'a> {
    fn new(cancelled: &'a dyn Fn() -> bool) -> Self {
        Self { left: PARSE_WORK_CAP, hit: false, cancelled }
    }

    fn step(&mut self, n: usize) -> Result<(), Halt> {
        if (self.cancelled)() {
            return Err(Halt::Cancel);
        }
        if self.hit {
            return Err(Halt::Work);
        }
        if n > self.left {
            self.hit = true;
            self.left = 0;
            return Err(Halt::Work);
        }
        self.left -= n;
        Ok(())
    }
}

enum HeapFail {
    Unread,
    BadUtf8,
    Cancel,
}

enum MethodFail {
    Cancel,
    Message(&'static str),
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
        heap_lines: Vec::new(),
        method_headers: Vec::new(),
        zero_rows: Vec::new(),
        caps: Vec::new(),
        refusals: Vec::new(),
        stored_bytes: 0,
        output_omitted: false,
    }
}

fn note_work(evidence: &mut Evidence) {
    if !evidence.caps.contains(&PARSE_WORK_CAP_NAME) {
        evidence.caps.push(PARSE_WORK_CAP_NAME);
    }
    if !evidence.refusals.iter().any(|item| item == WORK_UNREAD) {
        add_refusal(evidence, WORK_UNREAD.to_string());
    }
}

fn note_declared(evidence: &mut Evidence) {
    if !evidence.caps.contains(&DECLARED_SIZE_CAP_NAME) {
        evidence.caps.push(DECLARED_SIZE_CAP_NAME);
    }
    if !evidence.refusals.iter().any(|item| item == DECLARED_UNREAD) {
        add_refusal(evidence, DECLARED_UNREAD.to_string());
    }
}

fn poll(budget: &mut Budget<'_>) -> Result<(), ()> {
    match budget.step(0) {
        Ok(()) => Ok(()),
        Err(Halt::Cancel) => Err(()),
        Err(Halt::Work) => Ok(()),
    }
}

/// The observation byte cap. Both stored evidence lines and the report charge here.
/// A charge that does not fit sets `omitted` and is not kept.
fn charge_observation(used: &mut usize, omitted: &mut bool, bytes: usize) -> bool {
    if *omitted {
        return false;
    }
    if used.saturating_add(bytes).saturating_add(OBSERVATION_NOTICE.len()) > OBSERVATION_BYTE_CAP {
        *omitted = true;
        return false;
    }
    *used = used.saturating_add(bytes);
    true
}

fn keep_line(evidence: &mut Evidence, line: String) -> Option<String> {
    if charge_observation(&mut evidence.stored_bytes, &mut evidence.output_omitted, line.len() + 1) {
        Some(line)
    } else {
        None
    }
}

fn add_refusal(evidence: &mut Evidence, reason: String) {
    if let Some(reason) = keep_line(evidence, reason) {
        evidence.refusals.push(reason);
    }
}

fn cap_line(name: &str) -> String {
    format!("Refusal: {name}.")
}

/// A PE whose data-directory count does not include entry 14 is not a CLI subject.
fn locate(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Located>, ()> {
    poll(budget)?;
    if bytes.get(0..2) != Some(b"MZ") {
        return Ok(None);
    }
    let Some(lfanew_raw) = rd_u32(bytes, 0x3c) else {
        return Ok(None);
    };
    let lfanew = lfanew_raw as usize;
    let Some(mark_end) = lfanew.checked_add(4) else {
        return Ok(None);
    };
    if bytes.get(lfanew..mark_end) != Some(b"PE\0\0") {
        return Ok(None);
    }
    poll(budget)?;
    let coff = lfanew + 4;
    let Some(section_count_raw) = rd_u16(bytes, coff + 2) else {
        return Ok(None);
    };
    let section_count = section_count_raw as usize;
    let Some(opt_size_raw) = rd_u16(bytes, coff + 16) else {
        return Ok(None);
    };
    let opt_size = opt_size_raw as usize;
    let opt = coff + 20;
    let Some(magic) = rd_u16(bytes, opt) else {
        return Ok(None);
    };
    let wide = match magic {
        0x10b => false,
        0x20b => true,
        _ => return Ok(None),
    };
    let dir_count_at = if wide { 108 } else { 92 };
    let dir_base = if wide { 112 } else { 96 };
    // PE32+ entry 14 is optional-header offset 224. PE32 is offset 208.
    let entry_at = dir_base + 14 * 8;
    let headers = rd_u32(bytes, opt + 60).unwrap_or(0);
    let Some(section_table) = opt.checked_add(opt_size) else {
        return Ok(None);
    };
    let cli_absent = Located::Absent;
    if opt_size < dir_count_at + 4 {
        return Ok(Some(cli_absent));
    }
    let Some(count) = rd_u32(bytes, opt + dir_count_at) else {
        return Ok(None);
    };
    if count < 15 || opt_size < entry_at + 8 {
        return Ok(Some(cli_absent));
    }
    let entry = opt + entry_at;
    let Some(rva) = rd_u32(bytes, entry) else {
        return Ok(None);
    };
    let Some(size) = rd_u32(bytes, entry + 4) else {
        return Ok(None);
    };
    poll(budget)?;
    Ok(Some(Located::Present(Cli {
        rva,
        size,
        directory_offset: entry_at as u32,
        section_table,
        section_count,
        headers,
    })))
}

fn sections_of(bytes: &[u8], cli: &Cli, budget: &mut Budget<'_>) -> Result<(Vec<Section>, bool), ()> {
    let mut sections = Vec::new();
    let mut complete = cli.section_count <= SECTION_CAP;
    let capped = cli.section_count.min(SECTION_CAP);
    for index in 0..capped {
        poll(budget)?;
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
    Ok((sections, complete))
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

fn map_range<'a>(bytes: &'a [u8], sections: &[Section], headers: u32, complete: bool, rva: u32, size: u32, budget: &mut Budget<'_>) -> Result<&'a [u8], MapFail> {
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
        match budget.step(0) {
            Ok(()) => {}
            Err(Halt::Cancel) => return Err(MapFail::Cancelled),
            Err(Halt::Work) => return Err(MapFail::Work),
        }
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
    // A partial hit beside a full hit is two sections claiming one RVA. Taking the full hit would map the wrong section.
    if mapped_count > 1 || (mapped_count == 1 && (short || partial)) {
        return Err(MapFail::Overlap);
    }
    if let Some((start, end)) = mapped {
        let len = end.saturating_sub(start);
        // The declared length alone is over the budget. That is not the cumulative work cap.
        if len > PARSE_WORK_CAP {
            return Err(MapFail::Declared);
        }
        match budget.step(len) {
            Ok(()) => return bytes.get(start..end).ok_or(MapFail::OutsideFile),
            Err(Halt::Cancel) => return Err(MapFail::Cancelled),
            Err(Halt::Work) => return Err(MapFail::Work),
        }
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
        MapFail::Cancelled | MapFail::Work | MapFail::Declared => String::new(),
    }
}

fn meta_map_reason(fail: MapFail, rva: u32, size: u32) -> String {
    match fail {
        MapFail::OutsideSections => format!("Refusal: metadata directory RVA {rva:#x} maps outside every section."),
        MapFail::OutsideFile => format!("Refusal: metadata directory size {size} at RVA {rva:#x} points outside the mapped file."),
        MapFail::Overlap => format!("Refusal: metadata directory RVA {rva:#x} maps into overlapping sections."),
        MapFail::Unresolved => "Refusal: PE section table is incomplete; metadata directory mapping is unresolved.".into(),
        MapFail::Cancelled | MapFail::Work | MapFail::Declared => String::new(),
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

fn heap_text(heap: &[u8], index: u32, budget: &mut Budget<'_>) -> Result<TextField, HeapFail> {
    if index == 0 {
        return Ok(TextField::Absent);
    }
    match budget.step(1) {
        Ok(()) => {}
        Err(Halt::Cancel) => return Err(HeapFail::Cancel),
        Err(Halt::Work) => return Err(HeapFail::Unread),
    }
    let start = index as usize;
    if start >= heap.len() {
        return Err(HeapFail::Unread);
    }
    let tail = &heap[start..];
    let limit = (NAME_CAP + 1).min(tail.len());
    let mut end = None;
    for (at, byte) in tail.iter().take(limit).enumerate() {
        match budget.step(1) {
            Ok(()) => {}
            Err(Halt::Cancel) => return Err(HeapFail::Cancel),
            Err(Halt::Work) => return Err(HeapFail::Unread),
        }
        if *byte == 0 {
            end = Some(at);
            break;
        }
    }
    let Some(end) = end else {
        return Err(HeapFail::Unread);
    };
    if end > NAME_CAP {
        return Err(HeapFail::Unread);
    }
    let text = &tail[..end];
    if text.is_empty() {
        return Ok(TextField::Absent);
    }
    if std::str::from_utf8(text).is_err() {
        return Err(HeapFail::BadUtf8);
    }
    if !text.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return Err(HeapFail::Unread);
    }
    Ok(TextField::Value(String::from_utf8(text.to_vec()).unwrap()))
}

enum HeaderFail {
    Truncated,
    Cancel,
    Work,
}

fn stream_header(meta: &[u8], at: usize, budget: &mut Budget<'_>) -> Result<(StreamRow, usize), HeaderFail> {
    let offset = rd_u32(meta, at).ok_or(HeaderFail::Truncated)?;
    let size = rd_u32(meta, at + 4).ok_or(HeaderFail::Truncated)?;
    match budget.step(8) {
        Ok(()) => {}
        Err(Halt::Cancel) => return Err(HeaderFail::Cancel),
        Err(Halt::Work) => return Err(HeaderFail::Work),
    }
    let mut cursor = at + 8;
    let name_start = cursor;
    loop {
        let byte = *meta.get(cursor).ok_or(HeaderFail::Truncated)?;
        cursor += 1;
        match budget.step(1) {
            Ok(()) => {}
            Err(Halt::Cancel) => return Err(HeaderFail::Cancel),
            Err(Halt::Work) => return Err(HeaderFail::Work),
        }
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

enum OpenFail {
    Outside,
    Declared,
}

/// Bytes of a stream that this parse will actually read.
/// A declared size past the metadata is outside, unless that size alone exceeds the
/// parse budget: the present prefix is then the work, and the declaration is not a refusal.
/// A declared size that is present and larger than the budget is `Declared` and is not scanned.
fn open_stream<'a>(meta: &'a [u8], stream: &StreamRow) -> Result<&'a [u8], OpenFail> {
    let start = stream.offset as usize;
    if start > meta.len() {
        return Err(OpenFail::Outside);
    }
    let declared = stream.size as usize;
    let available = meta.len() - start;
    if declared > PARSE_WORK_CAP && declared <= available {
        return Err(OpenFail::Declared);
    }
    if declared > available {
        if declared > PARSE_WORK_CAP {
            return Ok(&meta[start..]);
        }
        return Err(OpenFail::Outside);
    }
    meta.get(start..start + declared).ok_or(OpenFail::Outside)
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

fn read_name(stream: &[u8], row_at: usize, stride: usize, name_at: usize, width: usize, strings: &[u8], budget: &mut Budget<'_>) -> Result<TextField, HeapFail> {
    let row_end = row_at.checked_add(stride).ok_or(HeapFail::Unread)?;
    if row_end > stream.len() {
        return Err(HeapFail::Unread);
    }
    let name_off = row_at.checked_add(name_at).ok_or(HeapFail::Unread)?;
    if name_off.checked_add(width).is_none_or(|end| end > row_end) {
        return Err(HeapFail::Unread);
    }
    let index = read_index(stream, name_off, width).ok_or(HeapFail::Unread)?;
    heap_text(strings, index, budget)
}

fn read_index(bytes: &[u8], at: usize, width: usize) -> Option<u32> {
    match width {
        2 => rd_u16(bytes, at).map(|value| u32::from(value)),
        4 => rd_u32(bytes, at),
        _ => None,
    }
}

fn method_obs(stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize, budget: &mut Budget<'_>) -> Result<MethodObs, MethodFail> {
    if valid & (1u64 << 0x06) == 0 {
        return Ok(method_rows(0, 0, 0, Vec::new()));
    }
    let declared = rows[0x06];
    let stride = row_size(0x06, rows, heap).ok_or(MethodFail::Message("MethodDef row width is unresolved."))?;
    let relative = body_offset(rows, valid, heap, 0x06).ok_or(MethodFail::Message("MethodDef offset is unresolved."))?;
    let start = header.checked_add(relative).ok_or(MethodFail::Message("MethodDef offset is unresolved."))?;
    let limit = declared.min(METHOD_ROW_CAP as u32);
    let mut read = 0u32;
    let mut nonzero = 0u32;
    let mut rvas = Vec::new();
    let mut stopped = false;
    for index in 0..limit {
        match budget.step(stride) {
            Ok(()) => {}
            Err(Halt::Cancel) => return Err(MethodFail::Cancel),
            Err(Halt::Work) => {
                stopped = true;
                break;
            }
        }
        let Some(step) = (index as usize).checked_mul(stride) else {
            break;
        };
        let Some(at) = start.checked_add(step) else {
            break;
        };
        if at.checked_add(stride).is_none_or(|end| end > stream.len()) {
            break;
        }
        let rva = rd_u32(stream, at).ok_or(MethodFail::Message("MethodDef RVA bytes are unavailable."))?;
        if rva != 0 {
            nonzero += 1;
            rvas.push(rva);
        }
        read += 1;
    }
    if stopped {
        return Ok(method_rows(declared, read, nonzero, rvas));
    }
    if read < declared {
        if declared > METHOD_ROW_CAP as u32 {
            return Err(MethodFail::Message(METHOD_ROW_CAP_NAME));
        }
        return Err(MethodFail::Message("unread MethodDef rows are not counted and their bodies are not claimed."));
    }
    Ok(method_rows(declared, read, nonzero, rvas))
}

fn fill_names(evidence: &mut Evidence, meta: &[u8], stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize, budget: &mut Budget<'_>) -> Result<(), ()> {
    let Some(strings_row) = evidence.streams.iter().find(|item| item.name == "#Strings").cloned() else {
        if evidence.module_name_needed(valid, rows) || evidence.assembly_name_needed(valid, rows) {
            add_refusal(evidence, "Refusal: #Strings stream is absent; module and assembly names were not read.".into());
        } else {
            evidence.module_name = TextField::Absent;
            evidence.assembly_name = TextField::Absent;
        }
        return Ok(());
    };
    if strings_row.offset % 4 != 0 {
        evidence.module_name = if evidence.module_name_needed(valid, rows) { TextField::Unread } else { TextField::Absent };
        evidence.assembly_name = if evidence.assembly_name_needed(valid, rows) { TextField::Unread } else { TextField::Absent };
        return Ok(());
    }
    let strings = match open_stream(meta, &strings_row) {
        Ok(strings) => strings,
        Err(OpenFail::Declared) => {
            note_declared(evidence);
            evidence.module_name = if evidence.module_name_needed(valid, rows) { TextField::Unread } else { TextField::Absent };
            evidence.assembly_name = if evidence.assembly_name_needed(valid, rows) { TextField::Unread } else { TextField::Absent };
            return Ok(());
        }
        Err(OpenFail::Outside) => {
            evidence.module_name = if evidence.module_name_needed(valid, rows) { TextField::Unread } else { TextField::Absent };
            evidence.assembly_name = if evidence.assembly_name_needed(valid, rows) { TextField::Unread } else { TextField::Absent };
            return Ok(());
        }
    };
    evidence.module_name = match name_of(stream, rows, valid, heap, header, strings, 0x00, 2, budget) {
        Ok(field) => field,
        Err(HeapFail::Cancel) => return Err(()),
        Err(HeapFail::BadUtf8) => {
            add_refusal(evidence, "Refusal: #Strings heap module name is not valid UTF-8. Those bytes were skipped.".into());
            TextField::Unread
        }
        Err(HeapFail::Unread) => TextField::Unread,
    };
    evidence.assembly_name = match name_of(stream, rows, valid, heap, header, strings, 0x20, 16 + heap.blob, budget) {
        Ok(field) => field,
        Err(HeapFail::Cancel) => return Err(()),
        Err(HeapFail::BadUtf8) => {
            add_refusal(evidence, "Refusal: #Strings heap assembly name is not valid UTF-8. Those bytes were skipped.".into());
            TextField::Unread
        }
        Err(HeapFail::Unread) => TextField::Unread,
    };
    if matches!(evidence.module_name, TextField::Unread) && !evidence.refusals.iter().any(|item| item.contains("module name")) {
        add_refusal(evidence, "Refusal: module name bytes were not read.".into());
    }
    if matches!(evidence.assembly_name, TextField::Unread) && !evidence.refusals.iter().any(|item| item.contains("assembly name")) {
        add_refusal(evidence, "Refusal: assembly name bytes were not read.".into());
    }
    Ok(())
}

impl Evidence {
    fn module_name_needed(&self, valid: u64, rows: &[u32; 64]) -> bool {
        valid & 1 != 0 && rows[0x00] > 0
    }
    fn assembly_name_needed(&self, valid: u64, rows: &[u32; 64]) -> bool {
        valid & (1u64 << 0x20) != 0 && rows[0x20] > 0
    }
}

fn name_of(stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize, strings: &[u8], table: usize, name_at: usize, budget: &mut Budget<'_>) -> Result<TextField, HeapFail> {
    if valid & (1u64 << table) == 0 || rows[table] == 0 {
        return Ok(TextField::Absent);
    }
    let Some(stride) = row_size(table, rows, heap) else {
        return Ok(TextField::Unread);
    };
    let Some(relative) = body_offset(rows, valid, heap, table) else {
        return Ok(TextField::Unread);
    };
    let Some(row_at) = header.checked_add(relative) else {
        return Ok(TextField::Unread);
    };
    read_name(stream, row_at, stride, name_at, heap.string, strings, budget)
}

fn list_tables(evidence: &mut Evidence, rows: &[u32; 64], valid: u64) {
    evidence.valid = Some(valid);
    evidence.tables_listed = true;
    for index in 0..64 {
        if valid & (1u64 << index) == 0 {
            continue;
        }
        if rows[index] == 0 {
            evidence.zero_rows.push(index);
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

fn read_table_stream(evidence: &mut Evidence, meta: &[u8], streams: &[StreamRow], budget: &mut Budget<'_>) -> Result<(), ()> {
    let Some(table) = streams.iter().find(|stream| stream.name == "#~").or_else(|| streams.iter().find(|stream| stream.name == "#-")).cloned() else {
        evidence.tables_listed = true;
        evidence.module_name = TextField::Absent;
        evidence.assembly_name = TextField::Absent;
        evidence.methods = Some(method_rows(0, 0, 0, Vec::new()));
        return Ok(());
    };
    let body = match open_stream(meta, &table) {
        Ok(body) => body,
        Err(OpenFail::Declared) => {
            note_declared(evidence);
            return Ok(());
        }
        Err(OpenFail::Outside) => {
            add_refusal(evidence, format!("Refusal: {} stream extent points outside the metadata root. Table row counts were not read.", table.name));
            return Ok(());
        }
    };
    let uncompressed = table.name == "#-";
    let Some((rows, valid, header, heaps)) = table_bodies(body) else {
        add_refusal(evidence, "Refusal: metadata table header truncated. No table row count is listed.".into());
        return Ok(());
    };
    match budget.step(header) {
        Ok(()) => {}
        Err(Halt::Cancel) => return Err(()),
        Err(Halt::Work) => {
            note_work(evidence);
            return Ok(());
        }
    }
    let heap = heap_from(heaps, uncompressed);
    list_tables(evidence, &rows, valid);
    match method_obs(body, &rows, valid, heap, header, budget) {
        Ok(obs) => {
            if budget.hit {
                note_work(evidence);
            }
            evidence.methods = Some(obs);
        }
        Err(MethodFail::Cancel) => return Err(()),
        Err(MethodFail::Message(reason)) if reason == METHOD_ROW_CAP_NAME => {
            evidence.caps.push(METHOD_ROW_CAP_NAME);
            evidence.methods = partial_methods(body, &rows, valid, heap, header, budget)?;
            if budget.hit {
                note_work(evidence);
            }
        }
        Err(MethodFail::Message(reason)) => {
            add_refusal(evidence, format!("Refusal: {reason}"));
            evidence.methods = partial_methods(body, &rows, valid, heap, header, budget)?;
            if budget.hit {
                note_work(evidence);
            }
        }
    }
    fill_names(evidence, meta, body, &rows, valid, heap, header, budget)
}

/// Error-path MethodDef scan. The row count is bounded by `METHOD_ROW_CAP`, the same cap
/// `method_obs` stops on. Each row is charged to the parse-work budget, and that charge polls cancellation.
fn partial_methods(stream: &[u8], rows: &[u32; 64], valid: u64, heap: Heap, header: usize, budget: &mut Budget<'_>) -> Result<Option<MethodObs>, ()> {
    let declared = rows[0x06];
    let Some(stride) = row_size(0x06, rows, heap) else {
        return Ok(None);
    };
    let Some(relative) = body_offset(rows, valid, heap, 0x06) else {
        return Ok(None);
    };
    let Some(start) = header.checked_add(relative) else {
        return Ok(None);
    };
    let limit = declared.min(METHOD_ROW_CAP as u32);
    let mut read = 0u32;
    let mut nonzero = 0u32;
    let mut rvas = Vec::new();
    for index in 0..limit {
        match budget.step(stride) {
            Ok(()) => {}
            Err(Halt::Cancel) => return Err(()),
            Err(Halt::Work) => break,
        }
        let Some(step_at) = (index as usize).checked_mul(stride) else {
            break;
        };
        let Some(at) = start.checked_add(step_at) else {
            break;
        };
        if at.checked_add(stride).is_none_or(|end| end > stream.len()) {
            break;
        }
        let Some(rva) = rd_u32(stream, at) else {
            break;
        };
        if rva != 0 {
            nonzero += 1;
            rvas.push(rva);
        }
        read += 1;
    }
    Ok(Some(method_rows(declared, read, nonzero, rvas)))
}

fn note_heaps(evidence: &mut Evidence, meta: &[u8]) {
    for (name, unread) in [
        ("#Strings", "String heap bytes were not read."),
        ("#US", "User strings were not read."),
        ("#Blob", "Blobs were not read."),
        ("#GUID", "GUID heap bytes were not read."),
    ] {
        let Some(stream) = evidence.streams.iter().find(|item| item.name == name).cloned() else {
            continue;
        };
        if stream.offset % 4 != 0 {
            add_refusal(evidence, format!("Refusal: {name} stream offset {} is not 4-byte aligned. {unread}", stream.offset));
            continue;
        }
        match open_stream(meta, &stream) {
            Err(OpenFail::Outside) => {
                add_refusal(evidence, format!("Refusal: {name} stream extent points outside the metadata root. {unread}"));
            }
            Err(OpenFail::Declared) => note_declared(evidence),
            Ok(_) if stream.size as usize > PARSE_WORK_CAP => {}
            Ok(_) => {
                let line = format!(
                    "{name} offset {} size {} is 4-byte aligned and inside the metadata root.",
                    stream.offset, stream.size
                );
                if let Some(line) = keep_line(evidence, line) {
                    evidence.heap_lines.push(line);
                }
            }
        }
    }
}

fn method_summary(methods: &MethodObs) -> String {
    if methods.bodies_decoded == 0 {
        format!(
            "MethodDef rows declared {}, rows read {}, nonzero RVA {}. Method bodies were not read. IL was not decoded. No behaviour is claimed.",
            methods.declared, methods.read, methods.nonzero
        )
    } else {
        format!(
            "MethodDef rows declared {}, rows read {}, nonzero RVA {}. Method bodies decoded {}. IL instructions {}. No behaviour is claimed.",
            methods.declared, methods.read, methods.nonzero, methods.bodies_decoded, methods.instructions
        )
    }
}

fn remember_body(evidence: &mut Evidence, instructions: u32) {
    if let Some(methods) = evidence.methods.as_mut() {
        methods.bodies_decoded = methods.bodies_decoded.saturating_add(1);
        methods.instructions = methods.instructions.saturating_add(instructions);
    }
}

fn body_cap_reached(evidence: &mut Evidence) -> bool {
    let opened = evidence.methods.as_ref().map(|item| item.bodies_decoded).unwrap_or(0);
    if opened < BODY_DECODE_CAP as u32 {
        return false;
    }
    if !evidence.caps.contains(&BODY_DECODE_CAP_NAME) {
        evidence.caps.push(BODY_DECODE_CAP_NAME);
    }
    if !evidence.refusals.iter().any(|item| item == BODY_DECODE_UNREAD) {
        add_refusal(evidence, BODY_DECODE_UNREAD.to_string());
    }
    true
}

fn push_cap(evidence: &mut Evidence, name: &'static str, detail: String) {
    if !evidence.caps.contains(&name) {
        evidence.caps.push(name);
    }
    add_refusal(evidence, detail);
}

/// ECMA-335 operand widths. An opcode absent from the tables has no known width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpKind {
    None,
    I8,
    U8,
    I32,
    I64,
    F32,
    F64,
    Br8,
    Br32,
    Var16,
    Method,
    Field,
    Type,
    Sig,
    Tok,
    String,
    Switch,
}

fn kind_tag(tag: u8) -> OpKind {
    match tag {
        1 => OpKind::I8,
        2 => OpKind::U8,
        3 => OpKind::I32,
        4 => OpKind::I64,
        5 => OpKind::F32,
        6 => OpKind::F64,
        7 => OpKind::Br8,
        8 => OpKind::Br32,
        9 => OpKind::Var16,
        10 => OpKind::Method,
        11 => OpKind::Field,
        12 => OpKind::Type,
        13 => OpKind::Sig,
        14 => OpKind::Tok,
        15 => OpKind::String,
        16 => OpKind::Switch,
        _ => OpKind::None,
    }
}

fn fixed_width(kind: OpKind) -> Option<usize> {
    match kind {
        OpKind::None => Some(0),
        OpKind::I8 | OpKind::U8 | OpKind::Br8 => Some(1),
        OpKind::Var16 => Some(2),
        OpKind::I32 | OpKind::F32 | OpKind::Br32 | OpKind::Method | OpKind::Field | OpKind::Type | OpKind::Sig | OpKind::Tok | OpKind::String => Some(4),
        OpKind::I64 | OpKind::F64 => Some(8),
        OpKind::Switch => None,
    }
}

/// Single-byte opcodes. `0xFE` is the two-byte family and is not listed. Prefixes `0xF8..=0xFD` and `0xFF` are omitted on purpose.
fn opcode_one(op: u8) -> Option<(&'static str, OpKind)> {
    const ROWS: &[(u8, &str, u8)] = &[
        (0x00, "nop", 0), (0x01, "break", 0), (0x02, "ldarg.0", 0), (0x03, "ldarg.1", 0),
        (0x04, "ldarg.2", 0), (0x05, "ldarg.3", 0), (0x06, "ldloc.0", 0), (0x07, "ldloc.1", 0),
        (0x08, "ldloc.2", 0), (0x09, "ldloc.3", 0), (0x0A, "stloc.0", 0), (0x0B, "stloc.1", 0),
        (0x0C, "stloc.2", 0), (0x0D, "stloc.3", 0), (0x0E, "ldarg.s", 2), (0x0F, "ldarga.s", 2),
        (0x10, "starg.s", 2), (0x11, "ldloc.s", 2), (0x12, "ldloca.s", 2), (0x13, "stloc.s", 2),
        (0x14, "ldnull", 0), (0x15, "ldc.i4.m1", 0), (0x16, "ldc.i4.0", 0), (0x17, "ldc.i4.1", 0),
        (0x18, "ldc.i4.2", 0), (0x19, "ldc.i4.3", 0), (0x1A, "ldc.i4.4", 0), (0x1B, "ldc.i4.5", 0),
        (0x1C, "ldc.i4.6", 0), (0x1D, "ldc.i4.7", 0), (0x1E, "ldc.i4.8", 0), (0x1F, "ldc.i4.s", 1),
        (0x20, "ldc.i4", 3), (0x21, "ldc.i8", 4), (0x22, "ldc.r4", 5), (0x23, "ldc.r8", 6),
        (0x24, "unused", 0), (0x25, "dup", 0), (0x26, "pop", 0), (0x27, "jmp", 10),
        (0x28, "call", 10), (0x29, "calli", 13), (0x2A, "ret", 0), (0x2B, "br.s", 7),
        (0x2C, "brfalse.s", 7), (0x2D, "brtrue.s", 7), (0x2E, "beq.s", 7), (0x2F, "bge.s", 7),
        (0x30, "bgt.s", 7), (0x31, "ble.s", 7), (0x32, "blt.s", 7), (0x33, "bne.un.s", 7),
        (0x34, "bge.un.s", 7), (0x35, "bgt.un.s", 7), (0x36, "ble.un.s", 7), (0x37, "blt.un.s", 7),
        (0x38, "br", 8), (0x39, "brfalse", 8), (0x3A, "brtrue", 8), (0x3B, "beq", 8),
        (0x3C, "bge", 8), (0x3D, "bgt", 8), (0x3E, "ble", 8), (0x3F, "blt", 8),
        (0x40, "bne.un", 8), (0x41, "bge.un", 8), (0x42, "bgt.un", 8), (0x43, "ble.un", 8),
        (0x44, "blt.un", 8), (0x45, "switch", 16), (0x46, "ldind.i1", 0), (0x47, "ldind.u1", 0),
        (0x48, "ldind.i2", 0), (0x49, "ldind.u2", 0), (0x4A, "ldind.i4", 0), (0x4B, "ldind.u4", 0),
        (0x4C, "ldind.i8", 0), (0x4D, "ldind.i", 0), (0x4E, "ldind.r4", 0), (0x4F, "ldind.r8", 0),
        (0x50, "ldind.ref", 0), (0x51, "stind.ref", 0), (0x52, "stind.i1", 0), (0x53, "stind.i2", 0),
        (0x54, "stind.i4", 0), (0x55, "stind.i8", 0), (0x56, "stind.r4", 0), (0x57, "stind.r8", 0),
        (0x58, "add", 0), (0x59, "sub", 0), (0x5A, "mul", 0), (0x5B, "div", 0),
        (0x5C, "div.un", 0), (0x5D, "rem", 0), (0x5E, "rem.un", 0), (0x5F, "and", 0),
        (0x60, "or", 0), (0x61, "xor", 0), (0x62, "shl", 0), (0x63, "shr", 0),
        (0x64, "shr.un", 0), (0x65, "neg", 0), (0x66, "not", 0), (0x67, "conv.i1", 0),
        (0x68, "conv.i2", 0), (0x69, "conv.i4", 0), (0x6A, "conv.i8", 0), (0x6B, "conv.r4", 0),
        (0x6C, "conv.r8", 0), (0x6D, "conv.u4", 0), (0x6E, "conv.u8", 0), (0x6F, "callvirt", 10),
        (0x70, "cpobj", 12), (0x71, "ldobj", 12), (0x72, "ldstr", 15), (0x73, "newobj", 10),
        (0x74, "castclass", 12), (0x75, "isinst", 12), (0x76, "conv.r.un", 0), (0x77, "unused", 0),
        (0x78, "unused", 0), (0x79, "unbox", 12), (0x7A, "throw", 0), (0x7B, "ldfld", 11),
        (0x7C, "ldflda", 11), (0x7D, "stfld", 11), (0x7E, "ldsfld", 11), (0x7F, "ldsflda", 11),
        (0x80, "stsfld", 11), (0x81, "stobj", 12), (0x82, "conv.ovf.i1.un", 0), (0x83, "conv.ovf.i2.un", 0),
        (0x84, "conv.ovf.i4.un", 0), (0x85, "conv.ovf.i8.un", 0), (0x86, "conv.ovf.u1.un", 0), (0x87, "conv.ovf.u2.un", 0),
        (0x88, "conv.ovf.u4.un", 0), (0x89, "conv.ovf.u8.un", 0), (0x8A, "conv.ovf.i.un", 0), (0x8B, "conv.ovf.u.un", 0),
        (0x8C, "box", 12), (0x8D, "newarr", 12), (0x8E, "ldlen", 0), (0x8F, "ldelema", 12),
        (0x90, "ldelem.i1", 0), (0x91, "ldelem.u1", 0), (0x92, "ldelem.i2", 0), (0x93, "ldelem.u2", 0),
        (0x94, "ldelem.i4", 0), (0x95, "ldelem.u4", 0), (0x96, "ldelem.i8", 0), (0x97, "ldelem.i", 0),
        (0x98, "ldelem.r4", 0), (0x99, "ldelem.r8", 0), (0x9A, "ldelem.ref", 0), (0x9B, "stelem.i", 0),
        (0x9C, "stelem.i1", 0), (0x9D, "stelem.i2", 0), (0x9E, "stelem.i4", 0), (0x9F, "stelem.i8", 0),
        (0xA0, "stelem.r4", 0), (0xA1, "stelem.r8", 0), (0xA2, "stelem.ref", 0), (0xA3, "ldelem", 12),
        (0xA4, "stelem", 12), (0xA5, "unbox.any", 12), (0xA6, "unused", 0), (0xA7, "unused", 0),
        (0xA8, "unused", 0), (0xA9, "unused", 0), (0xAA, "unused", 0), (0xAB, "unused", 0),
        (0xAC, "unused", 0), (0xAD, "unused", 0), (0xAE, "unused", 0), (0xAF, "unused", 0),
        (0xB0, "unused", 0), (0xB1, "unused", 0), (0xB2, "unused", 0), (0xB3, "conv.ovf.i1", 0),
        (0xB4, "conv.ovf.u1", 0), (0xB5, "conv.ovf.i2", 0), (0xB6, "conv.ovf.u2", 0),
        (0xB7, "conv.ovf.i4", 0), (0xB8, "conv.ovf.u4", 0), (0xB9, "conv.ovf.i8", 0), (0xBA, "conv.ovf.u8", 0),
        (0xBB, "unused", 0), (0xBC, "unused", 0), (0xBD, "unused", 0), (0xBE, "unused", 0),
        (0xBF, "unused", 0), (0xC0, "unused", 0), (0xC1, "unused", 0), (0xC2, "refanyval", 12),
        (0xC3, "ckfinite", 0), (0xC4, "unused", 0), (0xC5, "unused", 0), (0xC6, "mkrefany", 12),
        (0xC7, "unused", 0), (0xC8, "unused", 0), (0xC9, "unused", 0), (0xCA, "unused", 0),
        (0xCB, "unused", 0), (0xCC, "unused", 0), (0xCD, "unused", 0), (0xCE, "unused", 0),
        (0xCF, "unused", 0), (0xD0, "ldtoken", 14), (0xD1, "conv.u2", 0), (0xD2, "conv.u1", 0),
        (0xD3, "conv.i", 0), (0xD4, "conv.ovf.i", 0), (0xD5, "conv.ovf.u", 0), (0xD6, "add.ovf", 0),
        (0xD7, "add.ovf.un", 0), (0xD8, "mul.ovf", 0), (0xD9, "mul.ovf.un", 0), (0xDA, "sub.ovf", 0),
        (0xDB, "sub.ovf.un", 0), (0xDC, "endfault/endfinally", 0), (0xDD, "leave", 8), (0xDE, "leave.s", 7),
        (0xDF, "stind.i", 0), (0xE0, "conv.u", 0), (0xE1, "unused", 0), (0xE2, "unused", 0),
        (0xE3, "unused", 0), (0xE4, "unused", 0), (0xE5, "unused", 0), (0xE6, "unused", 0),
        (0xE7, "unused", 0), (0xE8, "unused", 0), (0xE9, "unused", 0), (0xEA, "unused", 0),
        (0xEB, "unused", 0), (0xEC, "unused", 0), (0xED, "unused", 0), (0xEE, "unused", 0),
        (0xEF, "unused", 0), (0xF0, "unused", 0), (0xF1, "unused", 0), (0xF2, "unused", 0),
        (0xF3, "unused", 0), (0xF4, "unused", 0), (0xF5, "unused", 0), (0xF6, "unused", 0),
        (0xF7, "unused", 0),
    ];
    let row = ROWS.iter().find(|item| item.0 == op)?;
    Some((row.1, kind_tag(row.2)))
}

/// `0xFE` family through opcode.def `0x22`. `volatile.` is `0x13`. `0x20` is unused.
fn opcode_fe(op: u8) -> Option<(&'static str, OpKind)> {
    const ROWS: &[(u8, &str, u8)] = &[
        (0x00, "arglist", 0), (0x01, "ceq", 0), (0x02, "cgt", 0), (0x03, "cgt.un", 0),
        (0x04, "clt", 0), (0x05, "clt.un", 0), (0x06, "ldftn", 10), (0x07, "ldvirtftn", 10),
        (0x08, "unused", 0), (0x09, "ldarg", 9), (0x0A, "ldarga", 9), (0x0B, "starg", 9),
        (0x0C, "ldloc", 9), (0x0D, "ldloca", 9), (0x0E, "stloc", 9), (0x0F, "localloc", 0),
        (0x10, "unused", 0), (0x11, "endfilter", 0), (0x12, "unaligned.", 1), (0x13, "volatile.", 0),
        (0x14, "tail.", 0), (0x15, "initobj", 12), (0x16, "constrained.", 12), (0x17, "cpblk", 0),
        (0x18, "initblk", 0), (0x19, "no.", 2), (0x1A, "rethrow", 0), (0x1B, "unused", 0),
        (0x1C, "sizeof", 12), (0x1D, "refanytype", 0), (0x1E, "readonly.", 0), (0x1F, "unused", 0),
        (0x20, "unused", 0), (0x21, "unused", 0), (0x22, "unused", 0),
    ];
    let row = ROWS.iter().find(|item| item.0 == op)?;
    Some((row.1, kind_tag(row.2)))
}

fn store_method_header(evidence: &mut Evidence, line: String) -> bool {
    let Some(line) = keep_line(evidence, line) else {
        return false;
    };
    evidence.method_headers.push(line);
    true
}

fn il_line(rva: u32, at: usize, name: &str, len: usize, tail: &str) -> String {
    if tail.is_empty() {
        format!("IL {rva:#x} offset {at:#x} {name} length {len}.")
    } else {
        format!("IL {rva:#x} offset {at:#x} {name} length {len} {tail}.")
    }
}

fn le_u16(raw: &[u8]) -> Option<u16> {
    let bytes: [u8; 2] = raw.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn le_u32(raw: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = raw.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn le_u64(raw: &[u8]) -> Option<u64> {
    let bytes: [u8; 8] = raw.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn operand_tail(kind: OpKind, raw: &[u8], next: usize) -> Option<String> {
    let tail = match kind {
        OpKind::None | OpKind::Switch => String::new(),
        OpKind::I8 => format!("i8 {}", raw.first().copied()? as i8),
        OpKind::U8 => format!("var {}", raw.first().copied()?),
        OpKind::I32 => format!("i32 {}", le_u32(raw)? as i32),
        OpKind::I64 => format!("i64 {}", le_u64(raw)? as i64),
        OpKind::F32 => format!("r4 {:#x}", le_u32(raw)?),
        OpKind::F64 => format!("r8 {:#x}", le_u64(raw)?),
        OpKind::Br8 => {
            let delta = raw.first().copied()? as i8 as i64;
            format!("target {:#x}", (next as i64).wrapping_add(delta))
        }
        OpKind::Br32 => {
            let delta = le_u32(raw)? as i32 as i64;
            format!("target {:#x}", (next as i64).wrapping_add(delta))
        }
        OpKind::Var16 => format!("var {}", le_u16(raw)?),
        OpKind::Method => format!("method {:#x}", le_u32(raw)?),
        OpKind::Field => format!("field {:#x}", le_u32(raw)?),
        OpKind::Type => format!("type {:#x}", le_u32(raw)?),
        OpKind::Sig => format!("sig {:#x}", le_u32(raw)?),
        OpKind::Tok => format!("tok {:#x}", le_u32(raw)?),
        OpKind::String => {
            let token = le_u32(raw)?;
            format!("string index {} token {token:#x}", token & 0x00ff_ffff)
        }
    };
    Some(tail)
}

enum WalkEnd {
    Done,
    Cap(&'static str, String),
    Refuse(String),
    Budget,
}

struct Walk {
    lines: Vec<String>,
    instructions: u32,
    end: WalkEnd,
}

fn past_end(rva: u32, name: &str, at: usize, need: usize) -> WalkEnd {
    WalkEnd::Refuse(format!(
        "Refusal: MethodDef RVA {rva:#x} opcode {name} at IL offset {at:#x} needs {need} operand bytes past the code end. Later instructions were not decoded."
    ))
}

fn walk_il(budget: &mut Budget<'_>, evidence: &mut Evidence, rva: u32, code: &[u8]) -> Result<Walk, ()> {
    let mut lines = Vec::new();
    let mut instructions = 0u32;
    let mut at = 0usize;
    while at < code.len() {
        if instructions as usize >= INSTRUCTION_CAP {
            return Ok(Walk {
                lines,
                instructions,
                end: WalkEnd::Cap(
                    INSTRUCTION_CAP_NAME,
                    format!("Refusal: MethodDef RVA {rva:#x} reached the dotnet instruction cap at IL offset {at:#x}. Later instructions in this method were not decoded."),
                ),
            });
        }
        match budget.step(1) {
            Ok(()) => {}
            Err(Halt::Cancel) => return Err(()),
            Err(Halt::Work) => {
                note_work(evidence);
                return Ok(Walk { lines, instructions, end: WalkEnd::Budget });
            }
        }
        let op = code[at];
        let (name, kind, opcode_len) = if op == 0xFE {
            let Some(second) = code.get(at + 1).copied() else {
                return Ok(Walk { lines, instructions, end: past_end(rva, "0xfe", at, 1) });
            };
            match opcode_fe(second) {
                Some((name, kind)) => (name, kind, 2usize),
                None => {
                    return Ok(Walk {
                        lines,
                        instructions,
                        end: WalkEnd::Refuse(format!(
                            "Refusal: MethodDef RVA {rva:#x} opcode 0xfe {second:#x} at IL offset {at:#x} has no operand width. Later instructions were not decoded."
                        )),
                    });
                }
            }
        } else {
            match opcode_one(op) {
                Some((name, kind)) => (name, kind, 1usize),
                None => {
                    return Ok(Walk {
                        lines,
                        instructions,
                        end: WalkEnd::Refuse(format!(
                            "Refusal: MethodDef RVA {rva:#x} opcode {op:#x} at IL offset {at:#x} has no operand width. Later instructions were not decoded."
                        )),
                    });
                }
            }
        };
        let operand_at = at + opcode_len;
        if kind == OpKind::Switch {
            let Some(count_raw) = code.get(operand_at..operand_at + 4) else {
                return Ok(Walk { lines, instructions, end: past_end(rva, name, at, 4) });
            };
            let Some(count) = le_u32(count_raw) else {
                return Ok(Walk { lines, instructions, end: past_end(rva, name, at, 4) });
            };
            if count as usize > SWITCH_TARGET_CAP {
                return Ok(Walk {
                    lines,
                    instructions,
                    end: WalkEnd::Cap(
                        SWITCH_TARGET_CAP_NAME,
                        format!(
                            "Refusal: MethodDef RVA {rva:#x} switch at IL offset {at:#x} declares {count} targets, above the dotnet switch target cap. The switch operand was not consumed and later instructions in this method were not decoded."
                        ),
                    ),
                });
            }
            let Some(tail_bytes) = (count as usize).checked_mul(4) else {
                return Ok(Walk { lines, instructions, end: past_end(rva, name, at, 4) });
            };
            let Some(end) = operand_at.checked_add(4).and_then(|start| start.checked_add(tail_bytes)) else {
                return Ok(Walk { lines, instructions, end: past_end(rva, name, at, 4) });
            };
            if end > code.len() {
                return Ok(Walk { lines, instructions, end: past_end(rva, name, at, 4 + tail_bytes) });
            }
            let mut listed = String::new();
            let mut cursor = operand_at + 4;
            for index in 0..count {
                let Some(delta) = code.get(cursor..cursor + 4).and_then(le_u32) else {
                    return Ok(Walk { lines, instructions, end: past_end(rva, name, at, 4 + tail_bytes) });
                };
                if index > 0 {
                    listed.push(' ');
                }
                listed.push_str(&format!("{:#x}", (end as i64).wrapping_add(delta as i32 as i64)));
                cursor += 4;
            }
            let tail = if count == 0 {
                "target count 0".to_string()
            } else {
                format!("target count {count} targets {listed}")
            };
            lines.push(il_line(rva, at, name, end - at, &tail));
            instructions += 1;
            at = end;
            continue;
        }
        let Some(width) = fixed_width(kind) else {
            return Ok(Walk {
                lines,
                instructions,
                end: WalkEnd::Refuse(format!(
                    "Refusal: MethodDef RVA {rva:#x} opcode {name} at IL offset {at:#x} has no operand width. Later instructions were not decoded."
                )),
            });
        };
        let Some(end) = operand_at.checked_add(width) else {
            return Ok(Walk { lines, instructions, end: past_end(rva, name, at, width) });
        };
        if end > code.len() {
            return Ok(Walk { lines, instructions, end: past_end(rva, name, at, width) });
        }
        let Some(tail) = operand_tail(kind, &code[operand_at..end], end) else {
            return Ok(Walk { lines, instructions, end: past_end(rva, name, at, width) });
        };
        lines.push(il_line(rva, at, name, end - at, &tail));
        instructions += 1;
        at = end;
    }
    Ok(Walk { lines, instructions, end: WalkEnd::Done })
}

struct EhClause {
    kind: &'static str,
    try_off: u32,
    try_len: u32,
    try_end: u32,
    handler_off: u32,
    handler_len: u32,
    handler_end: u32,
    token: Option<u32>,
    filter: Option<u32>,
}

fn parse_clause(raw: &[u8], fat: bool) -> Option<EhClause> {
    let (flags, try_off, try_len, handler_off, handler_len, token) = if fat {
        (
            le_u32(raw.get(..4)?)?,
            le_u32(raw.get(4..8)?)?,
            le_u32(raw.get(8..12)?)?,
            le_u32(raw.get(12..16)?)?,
            le_u32(raw.get(16..20)?)?,
            le_u32(raw.get(20..24)?)?,
        )
    } else {
        (
            u32::from(le_u16(raw.get(..2)?)?),
            u32::from(le_u16(raw.get(2..4)?)?),
            u32::from(*raw.get(4)?),
            u32::from(le_u16(raw.get(5..7)?)?),
            u32::from(*raw.get(7)?),
            le_u32(raw.get(8..12)?)?,
        )
    };
    let (kind, token, filter) = match flags & 0x7 {
        0 => ("catch", Some(token), None),
        1 => ("filter", None, Some(token)),
        2 => ("finally", None, None),
        4 => ("fault", None, None),
        _ => return None,
    };
    Some(EhClause {
        kind,
        try_off,
        try_len,
        try_end: try_off.checked_add(try_len)?,
        handler_off,
        handler_len,
        handler_end: handler_off.checked_add(handler_len)?,
        token,
        filter,
    })
}

fn clause_line(rva: u32, clause: &EhClause) -> String {
    let mut line = format!(
        "EH {rva:#x} clause {} try offset {:#x} length {} end {:#x} handler offset {:#x} length {} end {:#x}",
        clause.kind, clause.try_off, clause.try_len, clause.try_end, clause.handler_off, clause.handler_len, clause.handler_end
    );
    if let Some(token) = clause.token {
        line.push_str(&format!(" token {token:#x}"));
    }
    if let Some(filter) = clause.filter {
        line.push_str(&format!(" filter offset {filter:#x}"));
    }
    line.push('.');
    line
}

enum SectEnd {
    Done,
    Cap(&'static str, String),
    Refuse(String),
    Budget,
}

struct SectRead {
    lines: Vec<String>,
    end: SectEnd,
}

enum Got<'a> {
    Bytes(&'a [u8]),
    Stop,
    Absent,
}

fn map_exact<'a>(bytes: &'a [u8], sections: &[Section], headers: u32, complete: bool, rva: u32, size: u32, budget: &mut Budget<'_>, evidence: &mut Evidence) -> Result<Got<'a>, ()> {
    match map_range(bytes, sections, headers, complete, rva, size, budget) {
        Ok(found) => Ok(Got::Bytes(found)),
        Err(MapFail::Cancelled) => Err(()),
        Err(MapFail::Work) => {
            note_work(evidence);
            Ok(Got::Stop)
        }
        Err(MapFail::Declared) => {
            note_declared(evidence);
            Ok(Got::Stop)
        }
        Err(_) => Ok(Got::Absent),
    }
}

fn read_sections(bytes: &[u8], sections: &[Section], cli: &Cli, complete: bool, evidence: &mut Evidence, budget: &mut Budget<'_>, rva: u32, header_len: u32, code_size: u32) -> Result<SectRead, ()> {
    let mut out = SectRead { lines: Vec::new(), end: SectEnd::Done };
    let Some(body_end) = header_len.checked_add(code_size) else {
        out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section offset overflowed. Clauses were not read."));
        return Ok(out);
    };
    let Some(aligned_sum) = body_end.checked_add(3) else {
        out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section offset overflowed. Clauses were not read."));
        return Ok(out);
    };
    let mut cursor = aligned_sum & !3;
    let mut seen = 0usize;
    loop {
        if seen >= EH_CHAIN_CAP {
            out.end = SectEnd::Cap(
                EH_CAP_NAME,
                format!("Refusal: MethodDef RVA {rva:#x} exception section chain exceeds the dotnet exception section cap. Later sections were not read."),
            );
            return Ok(out);
        }
        seen += 1;
        let Some(sect_rva) = rva.checked_add(cursor) else {
            out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} runs past the mapped bytes. Clauses were not read."));
            return Ok(out);
        };
        let header = match map_exact(bytes, sections, cli.headers, complete, sect_rva, 4, budget, evidence)? {
            Got::Bytes(header) => header,
            Got::Stop => {
                out.end = SectEnd::Budget;
                return Ok(out);
            }
            Got::Absent => {
                out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} runs past the mapped bytes. Clauses were not read."));
                return Ok(out);
            }
        };
        if header.len() < 4 {
            out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} is malformed. Clauses were not read."));
            return Ok(out);
        }
        let kind = header[0];
        let more = kind & 0x80 != 0;
        if kind & !0xC1 != 0 || kind & 0x01 == 0 {
            out.end = SectEnd::Refuse(format!(
                "Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} kind {kind:#x} is not an exception table. Clauses were not read."
            ));
            return Ok(out);
        }
        let fat = kind & 0x40 != 0;
        let data_size = if fat {
            u32::from(header[1]) | (u32::from(header[2]) << 8) | (u32::from(header[3]) << 16)
        } else {
            u32::from(header[1])
        };
        if data_size as usize > EH_BYTE_CAP {
            out.end = SectEnd::Cap(
                EH_CAP_NAME,
                format!("Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} data size {data_size} exceeds the dotnet exception section cap. Clauses were not read."),
            );
            return Ok(out);
        }
        let clause_len = if fat { 24usize } else { 12usize };
        if data_size < 4 || ((data_size - 4) as usize) % clause_len != 0 {
            out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} is malformed. Clauses were not read."));
            return Ok(out);
        }
        let raw = match map_exact(bytes, sections, cli.headers, complete, sect_rva, data_size, budget, evidence)? {
            Got::Bytes(raw) => raw,
            Got::Stop => {
                out.end = SectEnd::Budget;
                return Ok(out);
            }
            Got::Absent => {
                out.end = SectEnd::Refuse(format!(
                    "Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} data size {data_size} runs past the mapped bytes. Clauses were not read."
                ));
                return Ok(out);
            }
        };
        let total = ((data_size - 4) as usize) / clause_len;
        let mut parsed = Vec::new();
        for index in 0..total {
            match budget.step(1) {
                Ok(()) => {}
                Err(Halt::Cancel) => return Err(()),
                Err(Halt::Work) => {
                    note_work(evidence);
                    out.end = SectEnd::Budget;
                    return Ok(out);
                }
            }
            let at = 4 + index * clause_len;
            let Some(clause) = raw.get(at..at + clause_len).and_then(|slice| parse_clause(slice, fat)) else {
                out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section at offset {cursor:#x} is malformed. Clauses were not read."));
                return Ok(out);
            };
            parsed.push(clause);
        }
        let shown = parsed.len().min(CLAUSE_CAP);
        out.lines.push(format!(
            "EH {rva:#x} section kind {kind:#x} data size {data_size} clauses {shown} declared {}.",
            parsed.len()
        ));
        for clause in parsed.iter().take(shown) {
            out.lines.push(clause_line(rva, clause));
        }
        if parsed.len() > CLAUSE_CAP {
            out.end = SectEnd::Cap(
                CLAUSE_CAP_NAME,
                format!(
                    "Refusal: MethodDef RVA {rva:#x} exception section declares {} clauses, above the dotnet exception clause cap. Later clauses in this method were not emitted.",
                    parsed.len()
                ),
            );
            return Ok(out);
        }
        if !more {
            return Ok(out);
        }
        let Some(next) = cursor.checked_add(data_size) else {
            out.end = SectEnd::Refuse(format!("Refusal: MethodDef RVA {rva:#x} exception section offset overflowed. Clauses were not read."));
            return Ok(out);
        };
        cursor = next;
    }
}

enum Head {
    Tiny { code_size: u32 },
    Fat { header_bytes: u32, max_stack: u32, code_size: u32, local_sig: u32, init: bool, more: bool },
}

fn head_line(rva: u32, head: &Head, instructions: Option<u32>) -> String {
    let status = match instructions {
        Some(count) => format!("IL instructions {count}."),
        None => "IL was not decoded.".to_string(),
    };
    match head {
        Head::Tiny { code_size } => format!("MethodDef RVA {rva:#x} header tiny, code size {code_size}. {status}"),
        Head::Fat { header_bytes, max_stack, code_size, local_sig, init, more } => {
            let mut line = format!(
                "MethodDef RVA {rva:#x} header full, header bytes {header_bytes}, max stack {max_stack}, code size {code_size}, local var sig {local_sig:#x}"
            );
            if *init {
                line.push_str(", init locals");
            }
            if *more {
                line.push_str(", more sects");
            }
            line.push_str(". ");
            line.push_str(&status);
            line
        }
    }
}

fn truncated_body(rva: u32, code_size: u32, more: bool) -> String {
    let mut text = format!("Refusal: MethodDef RVA {rva:#x} declared code size {code_size} runs past the mapped bytes. IL was not decoded.");
    if more {
        text.push_str(" Exception sections were not read.");
    }
    text
}

enum Loaded<'a> {
    Code(&'a [u8]),
    Stop,
    Handled,
}

fn load_code<'a>(bytes: &'a [u8], sections: &[Section], cli: &Cli, complete: bool, evidence: &mut Evidence, budget: &mut Budget<'_>, rva: u32, head: &Head, header_len: u32, code_size: u32, more: bool) -> Result<Loaded<'a>, ()> {
    if code_size == 0 {
        return Ok(Loaded::Code(bytes.get(..0).unwrap_or(&[])));
    }
    let Some(span) = header_len.checked_add(code_size) else {
        add_refusal(evidence, truncated_body(rva, code_size, more));
        if !store_method_header(evidence, head_line(rva, head, None)) {
            return Ok(Loaded::Stop);
        }
        return Ok(Loaded::Handled);
    };
    match map_exact(bytes, sections, cli.headers, complete, rva, span, budget, evidence)? {
        Got::Stop => Ok(Loaded::Stop),
        Got::Absent => {
            add_refusal(evidence, truncated_body(rva, code_size, more));
            if !store_method_header(evidence, head_line(rva, head, None)) {
                return Ok(Loaded::Stop);
            }
            Ok(Loaded::Handled)
        }
        Got::Bytes(mapped) => {
            let start = header_len as usize;
            let end = span as usize;
            if start > end || mapped.len() < end {
                add_refusal(evidence, truncated_body(rva, code_size, more));
                if !store_method_header(evidence, head_line(rva, head, None)) {
                    return Ok(Loaded::Stop);
                }
                return Ok(Loaded::Handled);
            }
            Ok(Loaded::Code(&mapped[start..end]))
        }
    }
}

fn apply_end(evidence: &mut Evidence, end: WalkEnd) -> bool {
    match end {
        WalkEnd::Done => true,
        WalkEnd::Budget => false,
        WalkEnd::Cap(name, detail) => {
            push_cap(evidence, name, detail);
            !evidence.output_omitted
        }
        WalkEnd::Refuse(detail) => {
            add_refusal(evidence, detail);
            !evidence.output_omitted
        }
    }
}

fn apply_sect(evidence: &mut Evidence, end: SectEnd) -> bool {
    match end {
        SectEnd::Done => true,
        SectEnd::Budget => false,
        SectEnd::Cap(name, detail) => {
            push_cap(evidence, name, detail);
            !evidence.output_omitted
        }
        SectEnd::Refuse(detail) => {
            add_refusal(evidence, detail);
            !evidence.output_omitted
        }
    }
}

fn publish_body(bytes: &[u8], sections: &[Section], cli: &Cli, complete: bool, evidence: &mut Evidence, budget: &mut Budget<'_>, rva: u32, head: &Head, code: &[u8], header_len: u32, code_size: u32, more: bool) -> Result<bool, ()> {
    let Walk { lines, instructions, end } = walk_il(budget, evidence, rva, code)?;
    let budget_stop = matches!(end, WalkEnd::Budget);
    remember_body(evidence, instructions);
    if !store_method_header(evidence, head_line(rva, head, Some(instructions))) {
        return Ok(false);
    }
    for line in lines {
        if !store_method_header(evidence, line) {
            return Ok(false);
        }
    }
    if budget_stop || !apply_end(evidence, end) {
        return Ok(false);
    }
    if !more {
        return Ok(true);
    }
    let sect = read_sections(bytes, sections, cli, complete, evidence, budget, rva, header_len, code_size)?;
    for line in sect.lines {
        if !store_method_header(evidence, line) {
            return Ok(false);
        }
    }
    Ok(apply_sect(evidence, sect.end))
}

fn classify_headers(bytes: &[u8], sections: &[Section], cli: &Cli, complete: bool, evidence: &mut Evidence, budget: &mut Budget<'_>) -> Result<(), ()> {
    let rvas = evidence.methods.as_ref().map(|methods| methods.rvas.clone()).unwrap_or_default();
    for rva in rvas {
        if evidence.output_omitted {
            return Ok(());
        }
        if budget.hit {
            note_work(evidence);
            return Ok(());
        }
        match budget.step(0) {
            Ok(()) => {}
            Err(Halt::Cancel) => return Err(()),
            Err(Halt::Work) => {
                note_work(evidence);
                return Ok(());
            }
        }
        let first = match map_range(bytes, sections, cli.headers, complete, rva, 1, budget) {
            Ok(first) => first,
            Err(MapFail::Cancelled) => return Err(()),
            Err(MapFail::Work) => {
                note_work(evidence);
                return Ok(());
            }
            Err(MapFail::Declared) => {
                note_declared(evidence);
                return Ok(());
            }
            Err(MapFail::Overlap) => {
                add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} maps into overlapping sections. That method body header was not read."));
                if evidence.output_omitted {
                    return Ok(());
                }
                continue;
            }
            Err(MapFail::OutsideFile) => {
                add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} points outside the mapped file. That method body header was not read."));
                if evidence.output_omitted {
                    return Ok(());
                }
                continue;
            }
            Err(MapFail::Unresolved) => {
                add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} mapping is unresolved. That method body header was not read."));
                if evidence.output_omitted {
                    return Ok(());
                }
                continue;
            }
            Err(MapFail::OutsideSections) => {
                add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} maps outside every section. That method body header was not read."));
                if evidence.output_omitted {
                    return Ok(());
                }
                continue;
            }
        };
        let format = first[0] & 0x3;
        if format == 0x2 {
            let code_size = u32::from(first[0] >> 2);
            if code_size == 0 {
                if !store_method_header(evidence, format!("MethodDef RVA {rva:#x} header tiny, code size {code_size}. IL was not decoded.")) {
                    return Ok(());
                }
                continue;
            }
            if body_cap_reached(evidence) {
                return Ok(());
            }
            let head = Head::Tiny { code_size };
            match load_code(bytes, sections, cli, complete, evidence, budget, rva, &head, 1, code_size, false)? {
                Loaded::Stop => return Ok(()),
                Loaded::Handled => continue,
                Loaded::Code(code) => {
                    if !publish_body(bytes, sections, cli, complete, evidence, budget, rva, &head, code, 1, code_size, false)? {
                        return Ok(());
                    }
                }
            }
            continue;
        }
        if format != 0x3 {
            add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} header format {format} is not tiny or fat. The method body was not read."));
            if evidence.output_omitted {
                return Ok(());
            }
            continue;
        }
        let prefix = match map_range(bytes, sections, cli.headers, complete, rva, 2, budget) {
            Ok(prefix) => prefix,
            Err(MapFail::Cancelled) => return Err(()),
            Err(MapFail::Work) => {
                note_work(evidence);
                return Ok(());
            }
            Err(MapFail::Declared) => {
                note_declared(evidence);
                return Ok(());
            }
            Err(_) => {
                add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} fat header bytes were not mapped. The method body was not read."));
                if evidence.output_omitted {
                    return Ok(());
                }
                continue;
            }
        };
        let flags = u16::from_le_bytes([prefix[0], prefix[1]]);
        let header_bytes = u32::from(flags >> 12).saturating_mul(4);
        if !(12..=64).contains(&header_bytes) {
            add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} fat header is {header_bytes} bytes. The method body was not read."));
            if evidence.output_omitted {
                return Ok(());
            }
            continue;
        }
        let full = match map_range(bytes, sections, cli.headers, complete, rva, header_bytes, budget) {
            Ok(full) => full,
            Err(MapFail::Cancelled) => return Err(()),
            Err(MapFail::Work) => {
                note_work(evidence);
                return Ok(());
            }
            Err(MapFail::Declared) => {
                note_declared(evidence);
                return Ok(());
            }
            Err(_) => {
                add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} fat header bytes were not mapped. The method body was not read."));
                if evidence.output_omitted {
                    return Ok(());
                }
                continue;
            }
        };
        if full.len() < 12 {
            add_refusal(evidence, format!("Refusal: MethodDef RVA {rva:#x} fat header bytes were not mapped. The method body was not read."));
            if evidence.output_omitted {
                return Ok(());
            }
            continue;
        }
        let max_stack = u32::from(u16::from_le_bytes([full[2], full[3]]));
        let code_size = u32::from_le_bytes([full[4], full[5], full[6], full[7]]);
        let local_sig = u32::from_le_bytes([full[8], full[9], full[10], full[11]]);
        let init = flags & 0x10 != 0;
        let more = flags & 0x08 != 0;
        if body_cap_reached(evidence) {
            return Ok(());
        }
        let head = Head::Fat { header_bytes, max_stack, code_size, local_sig, init, more };
        match load_code(bytes, sections, cli, complete, evidence, budget, rva, &head, header_bytes, code_size, more)? {
            Loaded::Stop => return Ok(()),
            Loaded::Handled => continue,
            Loaded::Code(code) => {
                if !publish_body(bytes, sections, cli, complete, evidence, budget, rva, &head, code, header_bytes, code_size, more)? {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

fn read_streams(evidence: &mut Evidence, meta: &[u8], budget: &mut Budget<'_>) -> Result<(), ()> {
    poll(budget)?;
    if meta.len() < 16 || &meta[..4] != b"BSJB" {
        let reason = if meta.len() < 4 {
            "Refusal: metadata root truncated. No stream is listed."
        } else {
            "Refusal: metadata signature is not BSJB. No stream is listed."
        };
        add_refusal(evidence, reason.into());
        return Ok(());
    }
    evidence.signature = Some("BSJB");
    let version_len = match rd_u32(meta, 12) {
        Some(len) => len as usize,
        None => {
            add_refusal(evidence, "Refusal: metadata root truncated. No stream is listed.".into());
            return Ok(());
        }
    };
    if version_len > VERSION_CAP {
        add_refusal(evidence, "Refusal: metadata version length exceeds 256 bytes. Streams were not read.".into());
        return Ok(());
    }
    match budget.step(version_len) {
        Ok(()) => {}
        Err(Halt::Cancel) => return Err(()),
        Err(Halt::Work) => {
            note_work(evidence);
            return Ok(());
        }
    }
    let flags_at = 16usize.checked_add(version_len).unwrap_or(usize::MAX);
    let count_at = flags_at.checked_add(2).unwrap_or(usize::MAX);
    if count_at.checked_add(2).is_none_or(|end| end > meta.len()) {
        add_refusal(evidence, "Refusal: metadata root truncated. No stream is listed.".into());
        return Ok(());
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
        match stream_header(meta, cursor, budget) {
            Ok((stream, next)) => {
                cursor = next;
                streams.push(stream);
            }
            Err(HeaderFail::Truncated) => {
                add_refusal(evidence, "Refusal: metadata stream header truncated. No stream is listed.".into());
                return Ok(());
            }
            Err(HeaderFail::Cancel) => return Err(()),
            Err(HeaderFail::Work) => {
                note_work(evidence);
                break;
            }
        }
    }
    if count > STREAM_CAP {
        evidence.caps.push(STREAM_CAP_NAME);
    }
    evidence.streams = streams;
    evidence.streams_listed = true;
    note_heaps(evidence, meta);
    if budget.hit {
        return Ok(());
    }
    if count > STREAM_CAP && !evidence.streams.iter().any(|stream| stream.name == "#~" || stream.name == "#-") {
        add_refusal(evidence, "Refusal: dotnet metadata stream cap. A table stream past the retained headers was not read.".into());
        return Ok(());
    }
    let listed = evidence.streams.clone();
    read_table_stream(evidence, meta, &listed, budget)
}

fn map_owned<'a>(bytes: &'a [u8], sections: &[Section], headers: u32, complete: bool, rva: u32, size: u32, budget: &mut Budget<'_>) -> Result<Result<&'a [u8], MapFail>, ()> {
    match map_range(bytes, sections, headers, complete, rva, size, budget) {
        Err(MapFail::Cancelled) => Err(()),
        Err(MapFail::Work) => Ok(Err(MapFail::Work)),
        other => Ok(other),
    }
}

fn examine(bytes: &[u8], cli: &Cli, budget: &mut Budget<'_>) -> Result<Evidence, ()> {
    let mut evidence = blank(cli);
    let (sections, complete) = sections_of(bytes, cli, budget)?;
    let header = match map_owned(bytes, &sections, cli.headers, complete, cli.rva, cli.size, budget)? {
        Ok(header) => header,
        Err(MapFail::Work) => {
            note_work(&mut evidence);
            return Ok(evidence);
        }
        Err(MapFail::Declared) => {
            note_declared(&mut evidence);
            return Ok(evidence);
        }
        Err(fail) => {
            add_refusal(&mut evidence, clr_map_reason(fail, cli.rva, cli.size));
            return Ok(evidence);
        }
    };
    if header.len() >= 4 {
        evidence.header_bytes = rd_u32(header, 0);
    }
    if header.len() < 16 {
        add_refusal(&mut evidence, "Refusal: CLR runtime header does not cover the metadata directory.".into());
        return Ok(evidence);
    }
    let meta_rva = rd_u32(header, 8).unwrap_or(0);
    let meta_size = rd_u32(header, 12).unwrap_or(0);
    if meta_size == 0 {
        add_refusal(&mut evidence, "Refusal: metadata directory size is 0. Metadata root was not read.".into());
        return Ok(evidence);
    }
    let meta = match map_owned(bytes, &sections, cli.headers, complete, meta_rva, meta_size, budget)? {
        Ok(meta) => meta,
        Err(MapFail::Work) => {
            note_work(&mut evidence);
            return Ok(evidence);
        }
        Err(MapFail::Declared) => {
            note_declared(&mut evidence);
            return Ok(evidence);
        }
        Err(fail) => {
            add_refusal(&mut evidence, meta_map_reason(fail, meta_rva, meta_size));
            return Ok(evidence);
        }
    };
    read_streams(&mut evidence, meta, budget)?;
    if !budget.hit {
        classify_headers(bytes, &sections, cli, complete, &mut evidence, budget)?;
    }
    Ok(evidence)
}

fn report(evidence: &Evidence) -> (String, bool) {
    let mut out = String::new();
    let mut used = 0usize;
    let mut omitted = false;
    {
    let mut emit = |text: &str| {
        if omitted {
            return;
        }
        if !charge_observation(&mut used, &mut omitted, text.len() + 1) {
            return;
        }
        out.push_str(text);
        out.push('\n');
    };
    emit(".NET metadata and CIL observation");
    emit(SCOPE);
    emit(&format!(
        "CLR runtime header: data directory entry 14 at optional-header offset {}, RVA {:#x}, size {}.",
        evidence.directory_offset, evidence.clr_rva, evidence.clr_size
    ));
    if let Some(cb) = evidence.header_bytes {
        emit(&format!("CLR header cb {cb}."));
    }
    if let Some(signature) = evidence.signature {
        emit(&format!("Metadata signature {signature}."));
    }
    match &evidence.version {
        TextField::Value(version) => emit(&format!("Metadata version {version}.")),
        TextField::Absent => emit("Metadata version absent."),
        TextField::Unread => {}
    }
    if evidence.streams_listed {
        emit(&format!("Streams {}.", evidence.streams.len()));
        for stream in &evidence.streams {
            emit(&format!("stream \"{}\" offset {} size {}.", stream.name, stream.offset, stream.size));
        }
        for line in &evidence.heap_lines {
            emit(line);
        }
    }
    if evidence.tables_listed {
        if let Some(valid) = evidence.valid {
            emit(&format!("Table valid bits {valid:#x}."));
        } else {
            emit("Table stream absent.");
        }
        emit(&format!("Non-zero tables {}.", evidence.tables.len()));
        for table in &evidence.tables {
            emit(&format!("table 0x{:02x} {} rows {}.", table.index, table.name, table.rows));
        }
        for index in &evidence.zero_rows {
            emit(&format!("valid zero-row 0x{:02x} {} rows 0.", index, table_name(*index)));
        }
    }
    emit(&format!("Module name: {}.", field_word(&evidence.module_name)));
    emit(&format!("Assembly name: {}.", field_word(&evidence.assembly_name)));
    match &evidence.methods {
        Some(methods) => emit(&method_summary(methods)),
        None => {
            emit("MethodDef RVA column was not read. Method bodies were not read. IL was not decoded. No behaviour is claimed.");
        }
    }
    for line in &evidence.method_headers {
        emit(line);
    }
    for name in &evidence.caps {
        emit(&cap_line(name));
    }
    for reason in &evidence.refusals {
        emit(reason);
    }
    }
    if omitted {
        out.push_str(OBSERVATION_NOTICE);
    }
    (out, omitted)
}

fn field_word(field: &TextField) -> String {
    match field {
        TextField::Unread => "not read".into(),
        TextField::Absent => "absent".into(),
        TextField::Value(text) => text.clone(),
    }
}

fn present_cli(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Cli>, ()> {
    let Some(located) = locate(bytes, budget)? else {
        return Ok(None);
    };
    let Located::Present(cli) = located else {
        return Ok(None);
    };
    if cli.rva == 0 && cli.size == 0 {
        return Ok(None);
    }
    Ok(Some(cli))
}

/// `Absent` when the bytes are not a PE with a nonzero CLI data directory.
/// `Observed` is a parse, including a malformed-image refusal in the text.
/// `Bounded` is a cap refusal. The text still carries the existing refusal sentences.
/// `Err(())` means `cancelled` returned true during the parse. The inspect route
/// turns that into its existing `Inspection cancelled` error and does not keep the text.
#[derive(Debug)]
pub(super) enum CliText {
    Absent,
    Observed(String),
    Bounded(String),
}

fn is_bounded(evidence: &Evidence, report_omitted: bool) -> bool {
    report_omitted
        || evidence.output_omitted
        || evidence.caps.iter().any(|name| *name == PARSE_WORK_CAP_NAME || *name == DECLARED_SIZE_CAP_NAME || *name == OBSERVATION_BYTE_CAP_NAME)
}

pub(super) fn cli_evidence(bytes: &[u8], cancelled: &dyn Fn() -> bool) -> Result<CliText, ()> {
    let mut budget = Budget::new(cancelled);
    let Some(cli) = present_cli(bytes, &mut budget)? else {
        return Ok(CliText::Absent);
    };
    let evidence = examine(bytes, &cli, &mut budget)?;
    if cancelled() {
        return Err(());
    }
    let (text, omitted) = report(&evidence);
    if is_bounded(&evidence, omitted) {
        Ok(CliText::Bounded(text))
    } else {
        Ok(CliText::Observed(text))
    }
}

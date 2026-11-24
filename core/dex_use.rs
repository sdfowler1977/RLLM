//! DEX const-string, direct invoke, field, and type instructions, with the method that contains them.
//! The instruction is not executed.

#![forbid(unsafe_code)]

const HEADER: usize = 112;
const CLASS_CAP: usize = 256;
const METHOD_CAP: usize = 64;
const FIELD_CAP: usize = 64;
const INSN_CAP: usize = 2048;
const ORDINARY_CAP: usize = 32;
const EXTRA_CAP: usize = 32;
const TEXT_CAP: usize = 64;

const NOT_DEX: &str = "Not a DEX file.";
const SHORT_HEADER: &str = "The DEX header is shorter than its class definitions.";
const NONE: &str = "no const-string, invoke, field, or type instruction was read";
const NOT_EXECUTED: &str = "The instruction was not executed.";

pub fn dex_use(bytes: &[u8]) -> String {
    let mut report = String::new();
    if !bytes.starts_with(b"dex\n") {
        report.push_str(NOT_DEX);
        report.push('\n');
        report.push_str(NOT_EXECUTED);
        return report;
    }
    if bytes.len() < HEADER {
        report.push_str(SHORT_HEADER);
        report.push('\n');
        report.push_str(NOT_EXECUTED);
        return report;
    }
    let class_count = u32_at(bytes, 96).unwrap_or(0) as usize;
    let class_off = u32_at(bytes, 100).unwrap_or(0) as usize;
    let mut ordinary = 0usize;
    let mut extra = 0usize;
    let mut unread = class_count > CLASS_CAP;
    let mut unread_fields = false;
    let mut unread_methods = false;
    let mut unread_data = false;
    let mut unread_insns = false;
    let mut unread_unknown = false;
    let mut unread_fit = false;
    let mut unread_unit = false;
    let mut unread_code = false;
    let mut unread_debug = false;
    let mut unread_positions = false;
    let mut unread_debug_end = false;
    let mut unread_locals = false;
    let mut unread_high = false;
    let mut unread_param = false;
    let mut unread_opcodes = false;
    let mut unread_line = false;
    for index in 0..class_count.min(CLASS_CAP) {
        if ordinary >= ORDINARY_CAP && extra >= EXTRA_CAP {
            unread = true;
            break;
        }
        let Some(at) = class_off.checked_add(index.checked_mul(32).unwrap_or(usize::MAX)) else {
            unread = true;
            break;
        };
        let Some(data_off) = u32_at(bytes, at.saturating_add(24)) else {
            unread = true;
            continue;
        };
        if data_off == 0 {
            continue;
        }
        let mut source = class_notes(bytes, at);
        match walk_class(bytes, data_off as usize, &mut report, &mut ordinary, &mut extra, &mut source, &mut unread_insns, &mut unread_unknown, &mut unread_fit, &mut unread_unit, &mut unread_code, &mut unread_debug, &mut unread_positions, &mut unread_debug_end, &mut unread_locals, &mut unread_high, &mut unread_param, &mut unread_opcodes, &mut unread_line) {
            ClassStop::Done => {}
            ClassStop::Fields => unread_fields = true,
            ClassStop::Methods => unread_methods = true,
            ClassStop::Data => unread_data = true,
        }
    }
    if report.is_empty() {
        report.push_str(NONE);
        report.push('\n');
    }
    if unread {
        report.push_str("later classes were not read\n");
    }
    if unread_fields {
        report.push_str("later class fields were not read\n");
    }
    if unread_methods {
        report.push_str("later methods were not read\n");
    }
    if unread_data {
        report.push_str("later class data was not read\n");
    }
    if unread_insns {
        report.push_str("later instructions were not read\n");
    }
    if unread_unknown {
        report.push_str("an unrecognized instruction stopped the list\n");
    }
    if unread_fit {
        report.push_str("an instruction did not fit\n");
    }
    if unread_unit {
        report.push_str("a code unit was not in the file\n");
    }
    if unread_code {
        report.push_str("the code item was not read\n");
    }
    if unread_debug {
        report.push_str("later debug parameters were not read\n");
    }
    if unread_positions {
        report.push_str("later debug positions were not read\n");
    }
    if unread_debug_end {
        report.push_str("the debug info ended before its end opcode\n");
    }
    if unread_locals {
        report.push_str("later debug locals were not read\n");
    }
    if unread_high {
        report.push_str("a debug local above register 31 was not read\n");
    }
    if unread_param {
        report.push_str("a debug parameter above register 31 was not read\n");
    }
    if unread_opcodes {
        report.push_str("later debug opcodes were not read\n");
    }
    if unread_line {
        report.push_str("the debug line start was not read\n");
    }
    report.push_str(NOT_EXECUTED);
    report
}

fn class_notes(bytes: &[u8], class_at: usize) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(line) = source_line(bytes, class_at) {
        notes.push(line);
    }
    if let Some(line) = class_kind_line(bytes, class_at) {
        notes.push(line);
    }
    if let Some(line) = superclass_line(bytes, class_at) {
        notes.push(line);
    }
    notes.extend(interface_lines(bytes, class_at));
    notes.extend(annotation_lines(bytes, class_at));
    notes
}

fn annotation_lines(bytes: &[u8], class_at: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(dir) = u32_at(bytes, class_at.saturating_add(20)) else {
        return lines;
    };
    if dir == 0 {
        return lines;
    }
    let class_idx = u32_at(bytes, class_at).unwrap_or(u32::MAX);
    let class_name = type_at(bytes, class_idx).unwrap_or_else(|| "?".to_string());
    let Some(set) = u32_at(bytes, dir as usize) else {
        lines.push(format!("class {class_name} annotated unreadable"));
        return lines;
    };
    if set != 0 {
        match annotation_names(bytes, set) {
            None => lines.push(format!("class {class_name} annotated unreadable")),
            Some(names) => {
                for name in names {
                    lines.push(format!("class {class_name} annotated {name}"));
                }
            }
        }
    }
    let fields = u32_at(bytes, (dir as usize).saturating_add(4)).unwrap_or(u32::MAX);
    let methods = u32_at(bytes, (dir as usize).saturating_add(8)).unwrap_or(0);
    if fields != u32::MAX {
        if let Some(field_base) = (dir as usize).checked_add(16) {
            let shown = (fields as usize).min(8);
            for index in 0..shown {
                let at = field_base + index * 8;
                let field_index = u32_at(bytes, at).unwrap_or(u32::MAX);
                let field_set = u32_at(bytes, at.saturating_add(4)).unwrap_or(0);
                let (owner, field) = field_names(bytes, field_index);
                let mut target = format!("{owner}.{field}");
                if let Some(declared) = field_type(bytes, field_index) {
                    target.push(':');
                    target.push_str(&declared);
                }
                match annotation_names(bytes, field_set) {
                    None => lines.push(format!("class {owner} field {target} annotated unreadable")),
                    Some(names) => {
                        for name in names {
                            lines.push(format!("class {owner} field {target} annotated {name}"));
                        }
                    }
                }
            }
            if fields as usize > shown && fields <= 64 {
                lines.push(format!("class {class_name} field annotated ..."));
            }
        }
    }
    if fields > 64 {
        lines.push(format!("class {class_name} method annotations were not located"));
        return lines;
    }
    let Some(base) = (dir as usize).checked_add(16).and_then(|at| at.checked_add((fields as usize).saturating_mul(8))) else {
        lines.push(format!("class {class_name} method annotations were not located"));
        return lines;
    };
    let shown = (methods as usize).min(8);
    for index in 0..shown {
        let at = base + index * 8;
        let method_index = u32_at(bytes, at).unwrap_or(u32::MAX);
        let method_set = u32_at(bytes, at.saturating_add(4)).unwrap_or(0);
        let (owner, method_name) = method_names(bytes, method_index);
        match annotation_names(bytes, method_set) {
            None => lines.push(format!("class {owner} method {method_name} annotated unreadable")),
            Some(names) => {
                for name in names {
                    lines.push(format!("class {owner} method {method_name} annotated {name}"));
                }
            }
        }
    }
    if methods as usize > shown {
        lines.push(format!("class {class_name} method annotated ..."));
    }
    let params = u32_at(bytes, (dir as usize).saturating_add(12)).unwrap_or(0);
    if methods > 64 {
        lines.push(format!("class {class_name} parameter annotations were not located"));
        return lines;
    }
    let Some(param_base) = base.checked_add((methods as usize).saturating_mul(8)) else {
        lines.push(format!("class {class_name} parameter annotations were not located"));
        return lines;
    };
    let shown_params = (params as usize).min(8);
    for index in 0..shown_params {
        let at = param_base + index * 8;
        let method_index = u32_at(bytes, at).unwrap_or(u32::MAX);
        let list = u32_at(bytes, at.saturating_add(4)).unwrap_or(0);
        let (owner, method_name) = method_names(bytes, method_index);
        if list == 0 {
            lines.push(format!("class {owner} method {method_name} parameter annotated unreadable"));
            continue;
        }
        let Some(count) = u32_at(bytes, list as usize) else {
            lines.push(format!("class {owner} method {method_name} parameter annotated unreadable"));
            continue;
        };
        let count_shown = (count as usize).min(8);
        for param in 0..count_shown {
            let Some(set_at) = (list as usize).checked_add(4).and_then(|start| start.checked_add(param.saturating_mul(4))) else {
                break;
            };
            let set = u32_at(bytes, set_at).unwrap_or(0);
            if set == 0 {
                continue;
            }
            match annotation_names(bytes, set) {
                None => lines.push(format!("class {owner} method {method_name} parameter {param} annotated unreadable")),
                Some(names) => {
                    for name in names {
                        lines.push(format!("class {owner} method {method_name} parameter {param} annotated {name}"));
                    }
                }
            }
        }
        if count as usize > count_shown {
            lines.push(format!("class {owner} method {method_name} parameter ..."));
        }
    }
    if params as usize > shown_params {
        lines.push(format!("class {class_name} parameter annotated ..."));
    }
    lines
}

fn annotation_label(bytes: &[u8], item: u32) -> Option<String> {
    let (type_index, next) = uleb(bytes, item as usize + 1)?;
    let mut text = type_at(bytes, type_index).unwrap_or_else(|| "unreadable".to_string());
    let (count, mut at) = uleb(bytes, next)?;
    let shown = (count as usize).min(4);
    for _ in 0..shown {
        let (name_index, next) = uleb(bytes, at)?;
        let Some((value, next)) = encoded_value(bytes, next) else {
            text.push_str(" ...");
            return Some(text);
        };
        at = next;
        let name = string_at(bytes, name_index).unwrap_or_else(|| "?".to_string());
        text.push(' ');
        text.push_str(&name);
        text.push('=');
        text.push_str(&value);
    }
    if count as usize > shown {
        text.push_str(" ...");
    }
    Some(text)
}

fn read_annotation(bytes: &[u8], at: usize, depth: usize) -> Option<(String, usize)> {
    let (type_index, next) = uleb(bytes, at)?;
    let mut text = type_at(bytes, type_index).unwrap_or_else(|| "unreadable".to_string());
    let (count, mut cursor) = uleb(bytes, next)?;
    if count > 32 {
        return None;
    }
    let shown = (count as usize).min(4);
    for _ in 0..shown {
        let (name_index, next) = uleb(bytes, cursor)?;
        let (value, next) = encoded_value_at(bytes, next, depth)?;
        cursor = next;
        let name = string_at(bytes, name_index).unwrap_or_else(|| "?".to_string());
        text.push(' ');
        text.push_str(&name);
        text.push('=');
        text.push_str(&value);
    }
    for _ in shown..count as usize {
        let (_, next) = uleb(bytes, cursor)?;
        cursor = skip_encoded_value(bytes, next)?;
    }
    if count as usize > shown {
        text.push_str(" ...");
    }
    Some((text, cursor))
}

fn encoded_value(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    encoded_value_at(bytes, at, 0)
}

fn encoded_value_at(bytes: &[u8], at: usize, depth: usize) -> Option<(String, usize)> {
    let kind = *bytes.get(at)?;
    let arg = (kind >> 5) as usize;
    let value_type = kind & 0x1f;
    let start = at + 1;
    if value_type == 0x1c {
        return Some((if arg == 0 { "false" } else { "true" }.to_string(), start));
    }
    if value_type == 0x1e {
        return Some(("null".to_string(), start));
    }
    if value_type == 0x1d {
        if depth >= 2 {
            let next = skip_encoded_annotation(bytes, start)?;
            return Some(("@{...}".to_string(), next));
        }
        let (text, next) = read_annotation(bytes, start, depth + 1)?;
        return Some((format!("@{{{text}}}"), next));
    }
    if value_type == 0x1f {
        if depth >= 2 {
            let next = skip_encoded_array(bytes, start)?;
            return Some(("[...]".to_string(), next));
        }
        let (count, mut cursor) = uleb(bytes, start)?;
        if count > 32 {
            return None;
        }
        let shown = (count as usize).min(4);
        let mut parts = Vec::new();
        for _ in 0..shown {
            let (text, next) = encoded_value_at(bytes, cursor, depth + 1)?;
            cursor = next;
            parts.push(text);
        }
        for _ in shown..count as usize {
            cursor = skip_encoded_value(bytes, cursor)?;
        }
        if count as usize > shown {
            parts.push("...".to_string());
        }
        return Some((format!("[{}]", parts.join(",")), cursor));
    }
    let Some(width) = value_width(value_type, arg) else {
        return None;
    };
    let raw_bytes = bytes.get(start..start + width)?;
    let mut raw = 0u64;
    for (place, byte) in raw_bytes.iter().enumerate() {
        raw |= u64::from(*byte) << (8 * place);
    }
    let next = start + width;
    let Some(text) = value_name(bytes, value_type, raw, width) else {
        return None;
    };
    Some((text, next))
}

fn value_width(value_type: u8, arg: usize) -> Option<usize> {
    let width = arg + 1;
    let limit = if matches!(value_type, 0x06 | 0x11) { 8 } else { 4 };
    if width > limit { None } else { Some(width) }
}

fn sign_extend(raw: u64, width: usize) -> i64 {
    let bits = width.saturating_mul(8);
    if bits == 0 || bits >= 64 {
        return raw as i64;
    }
    let shift = 64 - bits;
    ((raw << shift) as i64) >> shift
}

fn value_name(bytes: &[u8], value_type: u8, raw: u64, width: usize) -> Option<String> {
    if matches!(value_type, 0x17 | 0x18 | 0x19 | 0x1a | 0x1b) {
        let index = u32::try_from(raw).ok()?;
        if value_type == 0x17 {
            return Some(string_at(bytes, index).unwrap_or_else(|| "unreadable".to_string()));
        }
        if value_type == 0x18 {
            return Some(type_at(bytes, index).unwrap_or_else(|| "unreadable".to_string()));
        }
        if value_type == 0x19 || value_type == 0x1b {
            let (owner, field) = field_names(bytes, index);
            if owner == "?" && field == "?" {
                return Some(format!("{index:#x}"));
            }
            let mut text = format!("{owner}.{field}");
            if let Some(declared) = field_type(bytes, index) {
                text.push(':');
                text.push_str(&declared);
            }
            return Some(text);
        }
        let (class_name, method_name) = method_names(bytes, index);
        if class_name == "?" && method_name == "?" {
            return Some(format!("{index:#x}"));
        }
        let mut text = format!("{class_name}.{method_name}");
        if let Some(proto) = prototype(bytes, index) {
            text.push_str(&proto);
        }
        return Some(text);
    }
    if matches!(value_type, 0x00 | 0x02 | 0x04 | 0x06) {
        return Some(format!("{}", sign_extend(raw, width)));
    }
    if value_type == 0x03 {
        if (0x20..=0x7e).contains(&raw) {
            return Some(format!("'{}'", char::from(raw as u8)));
        }
        return Some(format!("{raw:#x}"));
    }
    if value_type == 0x10 {
        let shift = (4 - width) * 8;
        let bits = u32::try_from(raw).ok()? << shift;
        let value = f32::from_bits(bits);
        return Some(if value.is_finite() { format!("{value}") } else { format!("{bits:#x}") });
    }
    if value_type == 0x11 {
        let shift = (8 - width) * 8;
        let bits = raw << shift;
        let value = f64::from_bits(bits);
        return Some(if value.is_finite() { format!("{value}") } else { format!("{bits:#x}") });
    }
    if value_type == 0x15 {
        let index = u32::try_from(raw).ok()?;
        return Some(match proto_text(bytes, index) {
            Some(text) => format!("proto {text}"),
            None => format!("proto {index:#x}"),
        });
    }
    if value_type == 0x16 {
        let index = u32::try_from(raw).ok()?;
        return Some(method_handle_name(bytes, index).unwrap_or_else(|| format!("method-handle {index:#x}")));
    }
    None
}

fn skip_encoded_array(bytes: &[u8], at: usize) -> Option<usize> {
    let (count, mut cursor) = uleb(bytes, at)?;
    if count > 32 {
        return None;
    }
    for _ in 0..count {
        cursor = skip_encoded_value(bytes, cursor)?;
    }
    Some(cursor)
}

fn skip_encoded_annotation(bytes: &[u8], at: usize) -> Option<usize> {
    let (_, next) = uleb(bytes, at)?;
    let (count, mut cursor) = uleb(bytes, next)?;
    if count > 32 {
        return None;
    }
    for _ in 0..count {
        let (_, next) = uleb(bytes, cursor)?;
        cursor = skip_encoded_value(bytes, next)?;
    }
    Some(cursor)
}

fn skip_encoded_value(bytes: &[u8], at: usize) -> Option<usize> {
    let kind = *bytes.get(at)?;
    let arg = (kind >> 5) as usize;
    let value_type = kind & 0x1f;
    let start = at.checked_add(1)?;
    if value_type == 0x1c || value_type == 0x1e {
        return Some(start);
    }
    if value_type == 0x1d {
        return skip_encoded_annotation(bytes, start);
    }
    if value_type == 0x1f {
        return skip_encoded_array(bytes, start);
    }
    let width = value_width(value_type, arg)?;
    start.checked_add(width).filter(|end| *end <= bytes.len())
}

fn annotation_names(bytes: &[u8], set: u32) -> Option<Vec<String>> {
    if set == 0 {
        return Some(Vec::new());
    }
    let count = u32_at(bytes, set as usize)?;
    let shown = (count as usize).min(8);
    let mut names = Vec::new();
    for index in 0..shown {
        let at = (set as usize).checked_add(4)?.checked_add(index.saturating_mul(4))?;
        let item = u32_at(bytes, at)?;
        let name = annotation_label(bytes, item).unwrap_or_else(|| "unreadable".to_string());
        names.push(name);
    }
    if count as usize > shown {
        names.push("...".to_string());
    }
    Some(names)
}

fn class_kind_line(bytes: &[u8], class_at: usize) -> Option<String> {
    let flags = u32_at(bytes, class_at.checked_add(4)?)?;
    let kind = if flags & 0x2000 != 0 {
        "annotation"
    } else if flags & 0x200 != 0 {
        "interface"
    } else if flags & 0x4000 != 0 {
        "enum"
    } else {
        return None;
    };
    let class_idx = u32_at(bytes, class_at)?;
    let class_name = type_at(bytes, class_idx).unwrap_or_else(|| "?".to_string());
    Some(format!("class {class_name} kind {kind}"))
}

fn interface_lines(bytes: &[u8], class_at: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(off) = u32_at(bytes, class_at.saturating_add(12)) else {
        return lines;
    };
    if off == 0 {
        return lines;
    }
    let class_idx = u32_at(bytes, class_at).unwrap_or(u32::MAX);
    let class_name = type_at(bytes, class_idx).unwrap_or_else(|| "?".to_string());
    let Some(count) = u32_at(bytes, off as usize) else {
        lines.push(format!("class {class_name} interface unreadable"));
        return lines;
    };
    let shown = count.min(8);
    for index in 0..shown {
        let Some(at) = (off as usize).checked_add(4).and_then(|base| base.checked_add((index as usize).saturating_mul(2))) else {
            lines.push(format!("class {class_name} interface unreadable"));
            return lines;
        };
        let name = u16_at(bytes, at).and_then(|type_index| type_name(bytes, type_index)).unwrap_or_else(|| "unreadable".to_string());
        lines.push(format!("class {class_name} interface {name}"));
    }
    if count > shown {
        lines.push(format!("class {class_name} interface ..."));
    }
    lines
}

fn superclass_line(bytes: &[u8], class_at: usize) -> Option<String> {
    let idx = u32_at(bytes, class_at.checked_add(8)?)?;
    if idx == u32::MAX {
        return None;
    }
    let class_idx = u32_at(bytes, class_at)?;
    let class_name = type_at(bytes, class_idx).unwrap_or_else(|| "?".to_string());
    let parent = type_at(bytes, idx).unwrap_or_else(|| "unreadable".to_string());
    Some(format!("class {class_name} superclass {parent}"))
}

fn source_line(bytes: &[u8], class_at: usize) -> Option<String> {
    let idx = u32_at(bytes, class_at.checked_add(16)?)?;
    if idx == u32::MAX {
        return None;
    }
    let class_idx = u32_at(bytes, class_at)?;
    let class_name = type_at(bytes, class_idx).unwrap_or_else(|| "?".to_string());
    let file = string_at(bytes, idx).unwrap_or_else(|| "unreadable".to_string());
    Some(format!("class {class_name} source {file}"))
}

fn announce(source: &mut Vec<String>, report: &mut String) {
    for line in source.drain(..) {
        report.push_str(&line);
        report.push('\n');
    }
}

enum ClassStop {
    Done,
    Fields,
    Methods,
    Data,
}

fn walk_class(bytes: &[u8], at: usize, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>, unread_insns: &mut bool, unread_unknown: &mut bool, unread_fit: &mut bool, unread_unit: &mut bool, unread_code: &mut bool, unread_debug: &mut bool, unread_positions: &mut bool, unread_debug_end: &mut bool, unread_locals: &mut bool, unread_high: &mut bool, unread_param: &mut bool, unread_opcodes: &mut bool, unread_line: &mut bool) -> ClassStop {
    let Some((static_fields, next)) = uleb(bytes, at) else {
        return ClassStop::Data;
    };
    let Some((instance_fields, next)) = uleb(bytes, next) else {
        return ClassStop::Data;
    };
    let Some((direct_methods, next)) = uleb(bytes, next) else {
        return ClassStop::Data;
    };
    let Some((virtual_methods, mut at)) = uleb(bytes, next) else {
        return ClassStop::Data;
    };
    let fields = static_fields.saturating_add(instance_fields);
    if fields > FIELD_CAP as u32 {
        return ClassStop::Fields;
    }
    if direct_methods.saturating_add(virtual_methods) > METHOD_CAP as u32 {
        return ClassStop::Methods;
    }
    for _ in 0..fields {
        let Some((_, next)) = uleb(bytes, at) else {
            return ClassStop::Data;
        };
        let Some((_, next)) = uleb(bytes, next) else {
            return ClassStop::Data;
        };
        at = next;
    }
    let mut method_index = 0u32;
    let methods = direct_methods.saturating_add(virtual_methods);
    for _ in 0..methods {
        if *ordinary >= ORDINARY_CAP && *extra >= EXTRA_CAP {
            return ClassStop::Methods;
        }
        let Some((diff, next)) = uleb(bytes, at) else {
            return ClassStop::Data;
        };
        let Some((access, next)) = uleb(bytes, next) else {
            return ClassStop::Data;
        };
        let Some((code_off, next)) = uleb(bytes, next) else {
            return ClassStop::Data;
        };
        at = next;
        method_index = method_index.wrapping_add(diff);
        if code_off != 0 {
            read_code(bytes, method_index, code_off as usize, report, ordinary, extra, source, access, unread_insns, unread_unknown, unread_fit, unread_unit, unread_code, unread_debug, unread_positions, unread_debug_end, unread_locals, unread_high, unread_param, unread_opcodes, unread_line);
        } else if !report_uncoded(bytes, method_index, access, report, ordinary, extra, source) {
            return ClassStop::Methods;
        }
    }
    ClassStop::Done
}

fn report_uncoded(bytes: &[u8], method_index: u32, access: u32, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>) -> bool {
    if *ordinary >= ORDINARY_CAP && *extra >= EXTRA_CAP {
        return false;
    }
    let kind = if access & 0x100 != 0 {
        "native"
    } else if access & 0x400 != 0 {
        "abstract"
    } else {
        "no code"
    };
    if !admit(false, ordinary, extra) {
        return false;
    }
    announce(source, report);
    let (class_name, method_name) = method_names(bytes, method_index);
    report.push_str(&format!("class {class_name} method {method_name} {kind}"));
    report.push('\n');
    true
}

fn report_tries(bytes: &[u8], method_index: u32, code_off: usize, insns: usize, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>) {
    let tries = u16_at(bytes, code_off.saturating_add(6)).unwrap_or(0) as usize;
    if tries == 0 {
        return;
    }
    let Some(cursor) = code_off.checked_add(16).and_then(|base| base.checked_add(insns.saturating_mul(2))) else {
        return;
    };
    let cursor = if insns % 2 == 1 { cursor.saturating_add(2) } else { cursor };
    let Some(handlers) = cursor.checked_add(tries.saturating_mul(8)) else {
        return;
    };
    let (class_name, method_name) = method_names(bytes, method_index);
    let shown = tries.min(8);
    for index in 0..shown {
        if *ordinary >= ORDINARY_CAP && *extra >= EXTRA_CAP {
            return;
        }
        let at = cursor + index * 8;
        let Some(start) = u32_at(bytes, at) else {
            report.push_str(&format!("class {class_name} method {method_name} try was not read\n"));
            return;
        };
        let Some(count) = u16_at(bytes, at.saturating_add(4)) else {
            report.push_str(&format!("class {class_name} method {method_name} try was not read\n"));
            return;
        };
        let Some(handler_off) = u16_at(bytes, at.saturating_add(6)) else {
            report.push_str(&format!("class {class_name} method {method_name} try was not read\n"));
            return;
        };
        let catch = catch_text(bytes, handlers, handler_off);
        let marked = notable(&catch);
        if !admit(marked, ordinary, extra) {
            report.push_str("later tries were not read\n");
            return;
        }
        announce(source, report);
        report.push_str(&format!("class {class_name} method {method_name} try {start:#x} count {count} catch {catch}"));
        report.push('\n');
    }
    if tries > shown && *ordinary < ORDINARY_CAP {
        report.push_str(&format!("class {class_name} method {method_name} try ..."));
        report.push('\n');
        *ordinary += 1;
    }
}

fn catch_text(bytes: &[u8], handlers: usize, handler_off: u16) -> String {
    let Some(mut at) = handlers.checked_add(handler_off as usize) else {
        return "unreadable".to_string();
    };
    let Some((size, next)) = sleb(bytes, at) else {
        return "unreadable".to_string();
    };
    at = next;
    let typed = size.unsigned_abs() as usize;
    let mut parts = Vec::new();
    for _ in 0..typed.min(4) {
        let Some((type_index, next)) = uleb(bytes, at) else {
            parts.push("unreadable".to_string());
            return parts.join(", ");
        };
        let Some((addr, next)) = uleb(bytes, next) else {
            parts.push("unreadable".to_string());
            return parts.join(", ");
        };
        at = next;
        let name = type_at(bytes, type_index).unwrap_or_else(|| "unreadable".to_string());
        parts.push(format!("{name} at {addr:#x}"));
    }
    if typed > 4 {
        parts.push("...".to_string());
    }
    if size < 0 {
        let Some((addr, _)) = uleb(bytes, at) else {
            parts.push("any unreadable".to_string());
            return parts.join(", ");
        };
        parts.push(format!("any at {addr:#x}"));
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}

fn read_code(bytes: &[u8], method_index: u32, code_off: usize, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>, access: u32, unread_insns: &mut bool, unread_unknown: &mut bool, unread_fit: &mut bool, unread_unit: &mut bool, unread_code: &mut bool, unread_debug: &mut bool, unread_positions: &mut bool, unread_debug_end: &mut bool, unread_locals: &mut bool, unread_high: &mut bool, unread_param: &mut bool, unread_opcodes: &mut bool, unread_line: &mut bool) {
    let Some(insns_size) = u32_at(bytes, code_off.saturating_add(12)) else {
        *unread_code = true;
        return;
    };
    let registers = u16_at(bytes, code_off).unwrap_or(0);
    let incoming = u16_at(bytes, code_off.saturating_add(2)).unwrap_or(0);
    let debug_off = u32_at(bytes, code_off.saturating_add(8)).unwrap_or(0);
    let (positions, locals) = if debug_off == 0 {
        (Vec::new(), Vec::new())
    } else {
        debug_info(bytes, debug_off, registers, incoming, access & 0x8 != 0, unread_debug, unread_positions, unread_debug_end, unread_locals, unread_high, unread_param, unread_opcodes, unread_line)
    };
    report_tries(bytes, method_index, code_off, insns_size as usize, report, ordinary, extra, source);
    let mut pc = 0usize;
    let limit = (insns_size as usize).min(INSN_CAP);
    while pc < limit {
        if *ordinary >= ORDINARY_CAP && *extra >= EXTRA_CAP {
            *unread_insns = true;
            return;
        }
        let Some(unit) = unit_at(bytes, code_off, pc) else {
            *unread_unit = true;
            return;
        };
        let op = (unit & 0xff) as u8;
        let width = units(op) as usize;
        if width == 0 || pc + width > limit {
            if width == 0 {
                *unread_unknown = true;
            } else if (insns_size as usize) > INSN_CAP && pc + width > INSN_CAP {
                *unread_insns = true;
            } else if pc + width > insns_size as usize {
                *unread_fit = true;
            }
            return;
        }
        let line = line_at(&positions, pc);
        if op == 0x1a || op == 0x1b {
            let index = if op == 0x1a {
                unit_at(bytes, code_off, pc + 1).map(|value| value as u32)
            } else {
                let low = unit_at(bytes, code_off, pc + 1);
                let high = unit_at(bytes, code_off, pc + 2);
                low.zip(high).map(|(low, high)| (low as u32) | ((high as u32) << 16))
            };
            if let Some(index) = index {
                let local = local_name(&locals, pc, (unit >> 8) as u16);
                if !keep(bytes, method_index, op, index, report, ordinary, extra, source, line, local) {
                    *unread_insns = true;
                }
            } else {
                *unread_unit = true;
            }
        } else if let Some(kind) = invoke_name(op) {
            if let Some(index) = unit_at(bytes, code_off, pc + 1) {
                let args = invoke_args(bytes, code_off, pc, op, &locals);
                if !keep_invoke(bytes, method_index, kind, index as u32, report, ordinary, extra, source, line, args) {
                    *unread_insns = true;
                }
            } else {
                *unread_unit = true;
            }
        } else if let Some(kind) = field_name(op) {
            if let Some(index) = unit_at(bytes, code_off, pc + 1) {
                let object = field_object(bytes, code_off, pc, op, &locals);
                if !keep_field(bytes, method_index, kind, index as u32, report, ordinary, extra, source, line, object) {
                    *unread_insns = true;
                }
            } else {
                *unread_unit = true;
            }
        } else if let Some(kind) = type_name_op(op) {
            if let Some(index) = unit_at(bytes, code_off, pc + 1) {
                if !keep_type(bytes, method_index, kind, index, report, ordinary, extra, source, line) {
                    *unread_insns = true;
                }
            } else {
                *unread_unit = true;
            }
        }
        pc += width;
    }
    if pc >= INSN_CAP && (insns_size as usize) > INSN_CAP {
        *unread_insns = true;
    }
}

fn end_line(report: &mut String, place: (Option<i32>, Option<String>), local: Option<String>, args: Option<String>) {
    if let Some(line) = place.0 {
        report.push_str(&format!(" line {line}"));
    }
    if let Some(file) = place.1 {
        report.push_str(&format!(" file {file}"));
    }
    if let Some(local) = local {
        report.push_str(&format!(" local {local}"));
    }
    if let Some(args) = args {
        report.push_str(&format!(" args {args}"));
    }
    report.push('\n');
}

fn invoke_args(bytes: &[u8], code_off: usize, pc: usize, op: u8, locals: &[(u32, Option<u32>, u16, String)]) -> Option<String> {
    let unit = unit_at(bytes, code_off, pc)?;
    let mut regs = Vec::new();
    if matches!(op, 0x6e..=0x72 | 0xd8) {
        let count = ((unit >> 12) & 0xf) as usize;
        let fifth = ((unit >> 8) & 0xf) as u16;
        let words = unit_at(bytes, code_off, pc + 2)?;
        let four = [(words & 0xf) as u16, ((words >> 4) & 0xf) as u16, ((words >> 8) & 0xf) as u16, ((words >> 12) & 0xf) as u16];
        for index in 0..count.min(5) {
            regs.push(if index < 4 { four[index] } else { fifth });
        }
    } else if matches!(op, 0x74..=0x78 | 0xd9) {
        let count = (unit >> 8) as usize;
        let first = unit_at(bytes, code_off, pc + 2)? as usize;
        for index in 0..count.min(8) {
            regs.push((first + index) as u16);
        }
    } else {
        return None;
    }
    if regs.is_empty() {
        return None;
    }
    let mut named = false;
    let mut text = String::new();
    for (index, reg) in regs.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        if let Some(name) = local_name(locals, pc, *reg) {
            named = true;
            text.push_str(&name);
        } else {
            text.push('?');
        }
    }
    if !named {
        return None;
    }
    if matches!(op, 0x74..=0x78 | 0xd9) && (unit >> 8) as usize > 8 {
        text.push_str(",...");
    }
    Some(text)
}

fn line_at(positions: &[(u32, i32, Option<String>)], pc: usize) -> (Option<i32>, Option<String>) {
    let mut found = (None, None);
    for (address, value, file) in positions {
        if *address > pc as u32 {
            break;
        }
        found = (Some(*value), file.clone());
    }
    found
}

fn local_name(locals: &[(u32, Option<u32>, u16, String)], pc: usize, register: u16) -> Option<String> {
    let mut found = None;
    for (start, end, reg, name) in locals {
        if *reg != register || *start > pc as u32 {
            continue;
        }
        if end.map(|end| (pc as u32) < end).unwrap_or(true) {
            found = Some(name.clone());
        }
    }
    found
}

fn debug_name(bytes: &[u8], value: u32) -> Option<String> {
    if value == 0 {
        return None;
    }
    Some(string_at(bytes, value.wrapping_sub(1)).unwrap_or_else(|| "unreadable".to_string()))
}

fn debug_info(bytes: &[u8], off: u32, registers: u16, incoming: u16, is_static: bool, unread_debug: &mut bool, unread_positions: &mut bool, unread_debug_end: &mut bool, unread_locals: &mut bool, unread_high: &mut bool, unread_param: &mut bool, unread_opcodes: &mut bool, unread_line: &mut bool) -> (Vec<(u32, i32, Option<String>)>, Vec<(u32, Option<u32>, u16, String)>) {
    let mut out = Vec::new();
    let mut locals = Vec::new();
    let Some((line_start, next)) = uleb(bytes, off as usize) else {
        *unread_debug_end = true;
        return (out, locals);
    };
    let Some((params, mut at)) = uleb(bytes, next) else {
        *unread_debug_end = true;
        return (out, locals);
    };
    if params > 32 {
        *unread_debug = true;
        return (out, locals);
    }
    let mut param_reg = registers.saturating_sub(incoming);
    if !is_static {
        param_reg = param_reg.saturating_add(1);
    }
    for _ in 0..params {
        let Some((name, next)) = uleb(bytes, at) else {
            *unread_debug_end = true;
            return (out, locals);
        };
        at = next;
        if param_reg > 31 {
            *unread_param = true;
        } else if let Some(text) = debug_name(bytes, name) {
            if u32::from(param_reg) < u32::from(registers) {
                if locals.len() < 64 {
                    locals.push((0, None, param_reg, text));
                } else {
                    *unread_locals = true;
                }
            }
        }
        param_reg = param_reg.saturating_add(1);
    }
    let Ok(mut line) = i32::try_from(line_start) else {
        *unread_line = true;
        return (out, locals);
    };
    let mut address = 0u32;
    let mut file = None;
    let mut ended = false;
    let mut capped = false;
    let mut ran = 0usize;
    for _ in 0..1024 {
        ran += 1;
        let Some(op) = bytes.get(at).copied() else {
            break;
        };
        at += 1;
        match op {
            0x00 => {
                ended = true;
                break;
            }
            0x01 => {
                let Some((diff, next)) = uleb(bytes, at) else { break };
                at = next;
                address = address.wrapping_add(diff);
            }
            0x02 => {
                let Some((diff, next)) = sleb(bytes, at) else { break };
                at = next;
                line = line.saturating_add(diff);
            }
            0x03 | 0x04 => {
                let Some((reg, next)) = uleb(bytes, at) else { break };
                let Some((name, next)) = uleb(bytes, next) else { break };
                let Some((_, next)) = uleb(bytes, next) else { break };
                at = if op == 0x04 {
                    let Some((_, next)) = uleb(bytes, next) else { break };
                    next
                } else {
                    next
                };
                if reg <= 31 {
                    if let Some(slot) = locals.iter_mut().rev().find(|item| item.2 == reg as u16 && item.1.is_none()) {
                        slot.1 = Some(address);
                    }
                    if let Some(text) = debug_name(bytes, name) {
                        if locals.len() < 64 {
                            locals.push((address, None, reg as u16, text));
                        } else {
                            *unread_locals = true;
                        }
                    }
                } else {
                    *unread_high = true;
                }
            }
            0x05 | 0x06 => {
                let Some((reg, next)) = uleb(bytes, at) else { break };
                at = next;
                if reg > 31 {
                    *unread_high = true;
                } else if let Some(slot) = locals.iter_mut().rev().find(|item| item.2 == reg as u16) {
                    slot.1 = if op == 0x05 { Some(address) } else { None };
                }
            }
            0x09 => {
                let Some((name, next)) = uleb(bytes, at) else { break };
                at = next;
                file = if name == 0 {
                    None
                } else {
                    Some(string_at(bytes, name.wrapping_sub(1)).unwrap_or_else(|| "unreadable".to_string()))
                };
            }
            0x07 | 0x08 => {}
            _ => {
                let adjusted = u32::from(op - 0x0a);
                address = address.wrapping_add(adjusted / 15);
                line = line.saturating_add(-4 + (adjusted % 15) as i32);
                out.push((address, line, file.clone()));
                if out.len() == 64 {
                    if bytes.get(at).is_some_and(|op| *op != 0) {
                        *unread_positions = true;
                    }
                    capped = true;
                    break;
                }
            }
        }
    }
    if !ended && !capped {
        if ran == 1024 && bytes.get(at).is_some() {
            *unread_opcodes = true;
        } else {
            *unread_debug_end = true;
        }
    }
    (out, locals)
}

fn sleb(bytes: &[u8], at: usize) -> Option<(i32, usize)> {
    let mut value = 0i32;
    let mut shift = 0u32;
    for step in 0..5 {
        let byte = *bytes.get(at + step)?;
        value |= i32::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 32 && byte & 0x40 != 0 {
                value |= !0 << shift;
            }
            return Some((value, at + step + 1));
        }
    }
    None
}

fn keep(bytes: &[u8], method_index: u32, op: u8, index: u32, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>, line: (Option<i32>, Option<String>), local: Option<String>) -> bool {
    let text = string_at(bytes, index).unwrap_or_else(|| "unreadable".to_string());
    if !admit(notable(&text), ordinary, extra) {
        return false;
    }
    announce(source, report);
    let (class_name, method_name) = method_names(bytes, method_index);
    let kind = if op == 0x1b { "const-string/jumbo" } else { "const-string" };
    report.push_str(&format!("class {class_name} method {method_name} {kind} {index} {text}"));
    end_line(report, line, local, None);
    true
}

fn keep_invoke(bytes: &[u8], method_index: u32, kind: &str, index: u32, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>, line: (Option<i32>, Option<String>), args: Option<String>) -> bool {
    let (callee_class, callee_method) = method_names(bytes, index);
    let mut target = format!("{callee_class}.{callee_method}");
    if let Some(proto) = prototype(bytes, index) {
        target.push_str(&proto);
    }
    let marked = notable(&target) || args.as_deref().is_some_and(notable);
    if !admit(marked, ordinary, extra) {
        return false;
    }
    announce(source, report);
    let (class_name, method_name) = method_names(bytes, method_index);
    report.push_str(&format!("class {class_name} method {method_name} {kind} {index} {target}"));
    end_line(report, line, None, args);
    true
}

fn admit(marked: bool, ordinary: &mut usize, extra: &mut usize) -> bool {
    if *ordinary >= ORDINARY_CAP && !marked {
        return false;
    }
    if *ordinary >= ORDINARY_CAP && *extra >= EXTRA_CAP {
        return false;
    }
    if *ordinary >= ORDINARY_CAP {
        *extra += 1;
    } else {
        *ordinary += 1;
    }
    true
}

fn field_object(bytes: &[u8], code_off: usize, pc: usize, op: u8, locals: &[(u32, Option<u32>, u16, String)]) -> Option<String> {
    if !(0x52..=0x5f).contains(&op) {
        return None;
    }
    let unit = unit_at(bytes, code_off, pc)?;
    let register = ((unit >> 12) & 0xf) as u16;
    local_name(locals, pc, register)
}

fn keep_field(bytes: &[u8], method_index: u32, kind: &str, index: u32, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>, line: (Option<i32>, Option<String>), object: Option<String>) -> bool {
    let (owner, field) = field_names(bytes, index);
    let mut target = format!("{owner}.{field}");
    if let Some(declared) = field_type(bytes, index) {
        target.push(':');
        target.push_str(&declared);
    }
    let marked = notable(&target) || object.as_deref().is_some_and(notable);
    if !admit(marked, ordinary, extra) {
        return false;
    }
    announce(source, report);
    let (class_name, method_name) = method_names(bytes, method_index);
    report.push_str(&format!("class {class_name} method {method_name} {kind} {index} {target}"));
    if let Some(object) = object {
        report.push_str(&format!(" object {object}"));
    }
    end_line(report, line, None, None);
    true
}

fn field_type(bytes: &[u8], field_index: u32) -> Option<String> {
    let count = u32_at(bytes, 80)?;
    if field_index >= count {
        return None;
    }
    let table = u32_at(bytes, 84)? as usize;
    let at = table.checked_add((field_index as usize).checked_mul(8)?)?;
    let type_index = u16_at(bytes, at.checked_add(2)?)?;
    type_name(bytes, type_index)
}

fn field_names(bytes: &[u8], field_index: u32) -> (String, String) {
    let count = u32_at(bytes, 80).unwrap_or(0);
    let table = u32_at(bytes, 84).unwrap_or(0) as usize;
    if field_index >= count {
        return ("?".to_string(), "?".to_string());
    }
    let Some(at) = table.checked_add((field_index as usize).saturating_mul(8)) else {
        return ("?".to_string(), "?".to_string());
    };
    let class_idx = u16_at(bytes, at).unwrap_or(u16::MAX);
    let name_idx = u32_at(bytes, at.saturating_add(4)).unwrap_or(u32::MAX);
    let owner = type_name(bytes, class_idx).unwrap_or_else(|| "?".to_string());
    let field = string_at(bytes, name_idx).unwrap_or_else(|| "?".to_string());
    (owner, field)
}

fn keep_type(bytes: &[u8], method_index: u32, kind: &str, index: u16, report: &mut String, ordinary: &mut usize, extra: &mut usize, source: &mut Vec<String>, line: (Option<i32>, Option<String>)) -> bool {
    let text = type_name(bytes, index).unwrap_or_else(|| "?".to_string());
    if !admit(notable(&text), ordinary, extra) {
        return false;
    }
    announce(source, report);
    let (class_name, method_name) = method_names(bytes, method_index);
    report.push_str(&format!("class {class_name} method {method_name} {kind} {index} {text}"));
    end_line(report, line, None, None);
    true
}

fn type_name_op(op: u8) -> Option<&'static str> {
    match op {
        0x1c => Some("const-class"),
        0x1f => Some("check-cast"),
        0x20 => Some("instance-of"),
        0x22 => Some("new-instance"),
        0x23 => Some("new-array"),
        0x24 => Some("filled-new-array"),
        0x25 => Some("filled-new-array/range"),
        _ => None,
    }
}

fn field_name(op: u8) -> Option<&'static str> {
    match op {
        0x52 => Some("iget"),
        0x53 => Some("iget-wide"),
        0x54 => Some("iget-object"),
        0x55 => Some("iget-boolean"),
        0x56 => Some("iget-byte"),
        0x57 => Some("iget-char"),
        0x58 => Some("iget-short"),
        0x59 => Some("iput"),
        0x5a => Some("iput-wide"),
        0x5b => Some("iput-object"),
        0x5c => Some("iput-boolean"),
        0x5d => Some("iput-byte"),
        0x5e => Some("iput-char"),
        0x5f => Some("iput-short"),
        0x60 => Some("sget"),
        0x61 => Some("sget-wide"),
        0x62 => Some("sget-object"),
        0x63 => Some("sget-boolean"),
        0x64 => Some("sget-byte"),
        0x65 => Some("sget-char"),
        0x66 => Some("sget-short"),
        0x67 => Some("sput"),
        0x68 => Some("sput-wide"),
        0x69 => Some("sput-object"),
        0x6a => Some("sput-boolean"),
        0x6b => Some("sput-byte"),
        0x6c => Some("sput-char"),
        0x6d => Some("sput-short"),
        _ => None,
    }
}

fn invoke_name(op: u8) -> Option<&'static str> {
    match op {
        0x6e => Some("invoke-virtual"),
        0x6f => Some("invoke-super"),
        0x70 => Some("invoke-direct"),
        0x71 => Some("invoke-static"),
        0x72 => Some("invoke-interface"),
        0x74 => Some("invoke-virtual/range"),
        0x75 => Some("invoke-super/range"),
        0x76 => Some("invoke-direct/range"),
        0x77 => Some("invoke-static/range"),
        0x78 => Some("invoke-interface/range"),
        0xd8 => Some("invoke-polymorphic"),
        0xd9 => Some("invoke-polymorphic/range"),
        _ => None,
    }
}

fn prototype(bytes: &[u8], method_index: u32) -> Option<String> {
    let methods = u32_at(bytes, 88)?;
    if method_index >= methods {
        return None;
    }
    let methods_at = u32_at(bytes, 92)? as usize;
    let method_at = methods_at.checked_add((method_index as usize).checked_mul(8)?)?;
    let proto_index = u16_at(bytes, method_at.checked_add(2)?)?;
    proto_text(bytes, u32::from(proto_index))
}

fn method_handle_name(bytes: &[u8], index: u32) -> Option<String> {
    let map_off = u32_at(bytes, 52)? as usize;
    if map_off == 0 {
        return None;
    }
    let count = u32_at(bytes, map_off)?;
    if count == 0 || count > 32 {
        return None;
    }
    let mut handles = None;
    for item in 0..count {
        let at = map_off.checked_add(4)?.checked_add((item as usize).checked_mul(12)?)?;
        let kind = u16_at(bytes, at)?;
        if kind != 0x0008 {
            continue;
        }
        let size = u32_at(bytes, at.checked_add(4)?)?;
        let off = u32_at(bytes, at.checked_add(8)?)? as usize;
        handles = Some((size, off));
        break;
    }
    let (size, off) = handles?;
    if index >= size || size > 64 {
        return None;
    }
    let at = off.checked_add((index as usize).checked_mul(8)?)?;
    let handle_type = u16_at(bytes, at)?;
    let member = u32::from(u16_at(bytes, at.checked_add(4)?)?);
    let (kind, field) = handle_kind(handle_type)?;
    let target = if field {
        handle_field(bytes, member)
    } else {
        handle_method(bytes, member)
    };
    Some(format!("method-handle {kind} {target}"))
}

fn handle_kind(kind: u16) -> Option<(&'static str, bool)> {
    match kind {
        0x00 => Some(("static-put", true)),
        0x01 => Some(("static-get", true)),
        0x02 => Some(("instance-put", true)),
        0x03 => Some(("instance-get", true)),
        0x04 => Some(("invoke-static", false)),
        0x05 => Some(("invoke-instance", false)),
        0x06 => Some(("invoke-constructor", false)),
        0x07 => Some(("invoke-direct", false)),
        0x08 => Some(("invoke-interface", false)),
        _ => None,
    }
}

fn handle_field(bytes: &[u8], index: u32) -> String {
    let (owner, field) = field_names(bytes, index);
    if owner == "?" && field == "?" {
        return format!("{index:#x}");
    }
    let mut text = format!("{owner}.{field}");
    if let Some(declared) = field_type(bytes, index) {
        text.push(':');
        text.push_str(&declared);
    }
    text
}

fn handle_method(bytes: &[u8], index: u32) -> String {
    let (class_name, method_name) = method_names(bytes, index);
    if class_name == "?" && method_name == "?" {
        return format!("{index:#x}");
    }
    let mut text = format!("{class_name}.{method_name}");
    if let Some(proto) = prototype(bytes, index) {
        text.push_str(&proto);
    }
    text
}

fn proto_text(bytes: &[u8], proto_index: u32) -> Option<String> {
    let protos = u32_at(bytes, 72)?;
    if proto_index >= protos {
        return None;
    }
    let protos_at = u32_at(bytes, 76)? as usize;
    let proto_at = protos_at.checked_add((proto_index as usize).checked_mul(12)?)?;
    let return_index = u32_at(bytes, proto_at.checked_add(4)?)?;
    let params_off = u32_at(bytes, proto_at.checked_add(8)?)? as usize;
    let ret = type_at(bytes, return_index).unwrap_or_else(|| "?".to_string());
    let mut text = String::from("(");
    if params_off != 0 {
        let count = u32_at(bytes, params_off)?;
        let shown = count.min(16);
        for index in 0..shown {
            let at = params_off.checked_add(4)?.checked_add((index as usize).checked_mul(2)?)?;
            let type_index = u16_at(bytes, at)?;
            text.push_str(&type_name(bytes, type_index).unwrap_or_else(|| "?".to_string()));
        }
        if count > shown {
            text.push_str("...");
        }
    }
    text.push(')');
    text.push_str(&ret);
    Some(text)
}

fn type_at(bytes: &[u8], index: u32) -> Option<String> {
    type_name(bytes, u16::try_from(index).ok()?)
}

fn method_names(bytes: &[u8], method_index: u32) -> (String, String) {
    let count = u32_at(bytes, 88).unwrap_or(0);
    let table = u32_at(bytes, 92).unwrap_or(0) as usize;
    if method_index >= count {
        return ("?".to_string(), "?".to_string());
    }
    let Some(at) = table.checked_add((method_index as usize).saturating_mul(8)) else {
        return ("?".to_string(), "?".to_string());
    };
    let class_idx = u16_at(bytes, at).unwrap_or(u16::MAX);
    let name_idx = u32_at(bytes, at.saturating_add(4)).unwrap_or(u32::MAX);
    let class_name = type_name(bytes, class_idx).unwrap_or_else(|| "?".to_string());
    let method_name = string_at(bytes, name_idx).unwrap_or_else(|| "?".to_string());
    (class_name, method_name)
}

fn type_name(bytes: &[u8], class_idx: u16) -> Option<String> {
    let count = u32_at(bytes, 64)?;
    if class_idx as u32 >= count {
        return None;
    }
    let table = u32_at(bytes, 68)? as usize;
    let at = table.checked_add((class_idx as usize).checked_mul(4)?)?;
    let descriptor = u32_at(bytes, at)?;
    string_at(bytes, descriptor)
}

fn string_at(bytes: &[u8], index: u32) -> Option<String> {
    let count = u32_at(bytes, 56)?;
    if index >= count {
        return None;
    }
    let table = u32_at(bytes, 60)? as usize;
    let at = table.checked_add((index as usize).checked_mul(4)?)?;
    let data = u32_at(bytes, at)? as usize;
    let (len, start) = uleb(bytes, data)?;
    let stop = start.checked_add(len as usize)?;
    let raw = bytes.get(start..stop)?;
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let full = String::from_utf8_lossy(&raw[..end]);
    let mut text = String::new();
    let mut more = false;
    for (index, ch) in full.chars().enumerate() {
        if index == TEXT_CAP {
            more = true;
            break;
        }
        text.push(if ch == '\n' { ' ' } else { ch });
    }
    if more {
        text.push_str("...");
    }
    Some(text)
}

fn notable(text: &str) -> bool {
    text.contains("permission")
        || text.contains("Audio")
        || text.contains("microphone")
        || text.contains("MediaRecorder")
        || text.contains("RECORD_")
        || text.contains("getUserMedia")
        || text.contains("ultrasonic")
}

fn units(op: u8) -> u8 {
    match op {
        0x00 | 0x01 | 0x04 | 0x07 | 0x0a..=0x12 | 0x1d | 0x1e | 0x21 | 0x27 | 0x28 | 0x7b..=0x8f | 0x90..=0xaf => 1,
        0x02 | 0x05 | 0x08 | 0x13 | 0x15 | 0x16 | 0x19 | 0x1a | 0x1c | 0x1f | 0x20 | 0x22 | 0x23 | 0x29 | 0x2d..=0x3d | 0x44..=0x6d | 0xb0..=0xd7 | 0xdc | 0xdd => 2,
        0x03 | 0x06 | 0x09 | 0x14 | 0x17 | 0x1b | 0x24..=0x26 | 0x2a..=0x2c | 0x6e..=0x72 | 0x74..=0x78 | 0xda | 0xdb => 3,
        0xd8 | 0xd9 => 4,
        0x18 => 5,
        _ => 0,
    }
}

fn uleb(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut value = 0u32;
    let mut shift = 0u32;
    for step in 0..5 {
        let byte = *bytes.get(at + step)?;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, at + step + 1));
        }
        shift += 7;
    }
    None
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let raw = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let raw = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn unit_at(bytes: &[u8], code_off: usize, pc: usize) -> Option<u16> {
    u16_at(bytes, code_off.checked_add(16)?.checked_add(pc.checked_mul(2)?)?)
}

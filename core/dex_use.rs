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

#[cfg(test)]
mod tests {
    use super::dex_use;

    fn sample(insns: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0u8; 0xd0];
        bytes[0..8].copy_from_slice(b"dex\n035\0");
        let strings: [&[u8]; 3] = [b"LRec;", b"open", b"Record"];
        let mut data_at = [0u32; 3];
        for (index, text) in strings.iter().enumerate() {
            data_at[index] = bytes.len() as u32;
            bytes.push(text.len() as u8);
            bytes.extend_from_slice(text);
        }
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[36..40].copy_from_slice(&0x70u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&3u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&0x70u32.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&0x7cu32.to_le_bytes());
        bytes[88..92].copy_from_slice(&1u32.to_le_bytes());
        bytes[92..96].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&0x88u32.to_le_bytes());
        for (index, off) in data_at.iter().enumerate() {
            let at = 0x70 + index * 4;
            bytes[at..at + 4].copy_from_slice(&off.to_le_bytes());
        }
        bytes[0x84..0x88].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x90..0x94].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[0x98..0x9c].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[0xa0..0xa4].copy_from_slice(&0xa8u32.to_le_bytes());
        bytes[0xaa] = 1;
        bytes[0xae] = 0xb0;
        bytes[0xaf] = 0x01;
        bytes[0xb0] = 1;
        let size = insns.len() as u32;
        bytes[0xbc..0xc0].copy_from_slice(&size.to_le_bytes());
        for (index, unit) in insns.iter().enumerate() {
            let at = 0xc0 + index * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn with_proto(insns: &[u16]) -> Vec<u8> {
        let mut bytes = sample(insns);
        let off = bytes.len();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let list = bytes.len() as u32;
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes[off + 8..off + 12].copy_from_slice(&list.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[72..76].copy_from_slice(&1u32.to_le_bytes());
        bytes[76..80].copy_from_slice(&(off as u32).to_le_bytes());
        bytes
    }

    fn with_field(insns: &[u16]) -> Vec<u8> {
        let mut bytes = sample(insns);
        let off = bytes.len() as u32;
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[80..84].copy_from_slice(&1u32.to_le_bytes());
        bytes[84..88].copy_from_slice(&off.to_le_bytes());
        bytes
    }

    #[test]
    fn short_class_table_is_named() {
        let mut bytes = vec![0u8; 144];
        bytes[..4].copy_from_slice(b"dex\n");
        bytes[96..100].copy_from_slice(&2u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&112u32.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "no const-string, invoke, field, or type instruction was read\nlater classes were not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn field_cap_is_named() {
        let mut bytes = vec![0u8; 148];
        bytes[..4].copy_from_slice(b"dex\n");
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&112u32.to_le_bytes());
        bytes[136..140].copy_from_slice(&144u32.to_le_bytes());
        bytes[144] = 65;
        bytes[145] = 0;
        bytes[146] = 0;
        bytes[147] = 0;
        assert_eq!(
            dex_use(&bytes),
            "no const-string, invoke, field, or type instruction was read\nlater class fields were not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn instruction_cap_is_named() {
        let mut bytes = vec![0u8; 168 + 2048 * 2];
        bytes[..4].copy_from_slice(b"dex\n");
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&112u32.to_le_bytes());
        bytes[136..140].copy_from_slice(&144u32.to_le_bytes());
        bytes[144..152].copy_from_slice(&[0, 0, 1, 0, 0, 0, 0x98, 0x01]);
        bytes[152] = 1;
        bytes[164..168].copy_from_slice(&2049u32.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "no const-string, invoke, field, or type instruction was read\nlater instructions were not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn field_names_the_object() {
        let mut bytes = with_field(&[0x1052, 0x0000]);
        bytes[0xad] = 0x08;
        bytes[0xb0] = 2;
        bytes[0xb2] = 1;
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[0x01, 0x01, 0x03, 0x00]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open iget 0 LRec;.Record:LRec; object Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn invoke_names_the_argument() {
        let mut bytes = sample(&[0x106e, 0x0000, 0x0000]);
        bytes[0xad] = 0x08;
        bytes[0xb2] = 1;
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[0x01, 0x01, 0x03, 0x00]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open invoke-virtual 0 LRec;.open args Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn parameter_names_the_register() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xad] = 0x08;
        bytes[0xb2] = 1;
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[0x0e, 0x01, 0x03, 0x0a, 0x00]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record line 10 local Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn local_names_the_register() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[0x0e, 0x00, 0x03, 0x00, 0x03, 0x00, 0x0a, 0x00]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record line 10 local Record\nThe instruction was not executed."
        );
        let mut other = sample(&[0x011a, 0x0002]);
        let other_debug = other.len() as u32;
        other.extend_from_slice(&[0x0e, 0x00, 0x03, 0x00, 0x03, 0x00, 0x0a, 0x00]);
        let other_size = other.len() as u32;
        other[32..36].copy_from_slice(&other_size.to_le_bytes());
        other[0xb8..0xbc].copy_from_slice(&other_debug.to_le_bytes());
        let report = dex_use(&other);
        assert!(report.contains("const-string 2 Record line 10"), "{report}");
        assert!(!report.contains(" local "), "{report}");
    }

    #[test]
    fn line_names_the_instruction() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[0x0e, 0x00, 0x0a, 0x00]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record line 10\nThe instruction was not executed."
        );
        let mut named = sample(&[0x001a, 0x0002]);
        let named_debug = named.len() as u32;
        named.extend_from_slice(&[0x0e, 0x00, 0x09, 0x03, 0x0a, 0x00]);
        let named_size = named.len() as u32;
        named[32..36].copy_from_slice(&named_size.to_le_bytes());
        named[0xb8..0xbc].copy_from_slice(&named_debug.to_le_bytes());
        assert_eq!(
            dex_use(&named),
            "class LRec; method open const-string 2 Record line 10 file Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn interface_names_the_type() {
        let mut bytes = sample(&[0x0000, 0x001a, 0x0002]);
        let list = bytes.len() as u32;
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x94..0x98].copy_from_slice(&list.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; interface LRec;\nclass LRec; method open const-string 2 Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn superclass_names_the_parent() {
        let mut bytes = sample(&[0x0000, 0x001a, 0x0002]);
        bytes[0x90..0x94].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; superclass LRec;\nclass LRec; method open const-string 2 Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn source_file_names_the_class() {
        let mut bytes = sample(&[0x0000, 0x001a, 0x0002]);
        bytes[0x98..0x9c].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; source Record\nclass LRec; method open const-string 2 Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn annotation_nested_names_the_type() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&(dir + 16).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.extend_from_slice(&[1, 0, 1, 1, 0x1d, 0, 0]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        assert!(
            dex_use(&bytes).starts_with("class LRec; annotated LRec; open=@{LRec;}\n"),
            "{}",
            dex_use(&bytes)
        );
        let mut inner = sample(&[0x001a, 0x0002]);
        let inner_dir = inner.len() as u32;
        inner.extend_from_slice(&(inner_dir + 16).to_le_bytes());
        inner.extend_from_slice(&0u32.to_le_bytes());
        inner.extend_from_slice(&0u32.to_le_bytes());
        inner.extend_from_slice(&0u32.to_le_bytes());
        inner.extend_from_slice(&1u32.to_le_bytes());
        inner.extend_from_slice(&(inner_dir + 24).to_le_bytes());
        inner.extend_from_slice(&[1, 0, 1, 1, 0x1d, 0, 1, 1, 0x17, 2]);
        let inner_size = inner.len() as u32;
        inner[32..36].copy_from_slice(&inner_size.to_le_bytes());
        inner[0x9c..0xa0].copy_from_slice(&inner_dir.to_le_bytes());
        assert!(
            dex_use(&inner).starts_with("class LRec; annotated LRec; open=@{LRec; open=Record}\n"),
            "{}",
            dex_use(&inner)
        );
        let mut second = sample(&[0x001a, 0x0002]);
        let second_dir = second.len() as u32;
        second.extend_from_slice(&(second_dir + 16).to_le_bytes());
        second.extend_from_slice(&0u32.to_le_bytes());
        second.extend_from_slice(&0u32.to_le_bytes());
        second.extend_from_slice(&0u32.to_le_bytes());
        second.extend_from_slice(&1u32.to_le_bytes());
        second.extend_from_slice(&(second_dir + 24).to_le_bytes());
        second.extend_from_slice(&[1, 0, 1, 1, 0x1d, 0, 1, 1, 0x1d, 0, 0]);
        let second_size = second.len() as u32;
        second[32..36].copy_from_slice(&second_size.to_le_bytes());
        second[0x9c..0xa0].copy_from_slice(&second_dir.to_le_bytes());
        assert!(
            dex_use(&second).starts_with("class LRec; annotated LRec; open=@{LRec; open=@{LRec;}}\n"),
            "{}",
            dex_use(&second)
        );
        let mut deeper = sample(&[0x001a, 0x0002]);
        let deeper_dir = deeper.len() as u32;
        deeper.extend_from_slice(&(deeper_dir + 16).to_le_bytes());
        deeper.extend_from_slice(&0u32.to_le_bytes());
        deeper.extend_from_slice(&0u32.to_le_bytes());
        deeper.extend_from_slice(&0u32.to_le_bytes());
        deeper.extend_from_slice(&1u32.to_le_bytes());
        deeper.extend_from_slice(&(deeper_dir + 24).to_le_bytes());
        deeper.extend_from_slice(&[1, 0, 1, 1, 0x1d, 0, 2, 1, 0x1d, 0, 1, 1, 0x1d, 0, 0, 2, 0x17, 2]);
        let deeper_size = deeper.len() as u32;
        deeper[32..36].copy_from_slice(&deeper_size.to_le_bytes());
        deeper[0x9c..0xa0].copy_from_slice(&deeper_dir.to_le_bytes());
        assert!(
            dex_use(&deeper).starts_with("class LRec; annotated LRec; open=@{LRec; open=@{LRec; open=@{...}} Record=Record}\n"),
            "{}",
            dex_use(&deeper)
        );
        let mut deep_array = sample(&[0x001a, 0x0002]);
        let array_dir = deep_array.len() as u32;
        deep_array.extend_from_slice(&(array_dir + 16).to_le_bytes());
        deep_array.extend_from_slice(&0u32.to_le_bytes());
        deep_array.extend_from_slice(&0u32.to_le_bytes());
        deep_array.extend_from_slice(&0u32.to_le_bytes());
        deep_array.extend_from_slice(&1u32.to_le_bytes());
        deep_array.extend_from_slice(&(array_dir + 24).to_le_bytes());
        deep_array.extend_from_slice(&[1, 0, 1, 1, 0x1d, 0, 2, 1, 0x1d, 0, 1, 1, 0x1f, 1, 0x17, 2, 2, 0x17, 2]);
        let array_size = deep_array.len() as u32;
        deep_array[32..36].copy_from_slice(&array_size.to_le_bytes());
        deep_array[0x9c..0xa0].copy_from_slice(&array_dir.to_le_bytes());
        assert!(
            dex_use(&deep_array).starts_with("class LRec; annotated LRec; open=@{LRec; open=@{LRec; open=[...]} Record=Record}\n"),
            "{}",
            dex_use(&deep_array)
        );
        let mut long_note = sample(&[0x001a, 0x0002]);
        let note_dir = long_note.len() as u32;
        long_note.extend_from_slice(&(note_dir + 16).to_le_bytes());
        long_note.extend_from_slice(&0u32.to_le_bytes());
        long_note.extend_from_slice(&0u32.to_le_bytes());
        long_note.extend_from_slice(&0u32.to_le_bytes());
        long_note.extend_from_slice(&1u32.to_le_bytes());
        long_note.extend_from_slice(&(note_dir + 24).to_le_bytes());
        long_note.extend_from_slice(&[1, 0, 2, 1, 0x1d, 0, 5, 1, 0x1e, 1, 0x1e, 1, 0x1e, 1, 0x1e, 1, 0x1e, 2, 0x17, 2]);
        let note_size = long_note.len() as u32;
        long_note[32..36].copy_from_slice(&note_size.to_le_bytes());
        long_note[0x9c..0xa0].copy_from_slice(&note_dir.to_le_bytes());
        assert!(
            dex_use(&long_note).starts_with("class LRec; annotated LRec; open=@{LRec; open=null open=null open=null open=null ...} Record=Record\n"),
            "{}",
            dex_use(&long_note)
        );
    }

    fn annotation_file(mut bytes: Vec<u8>, payload: &[u8]) -> Vec<u8> {
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&(dir + 16).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.extend_from_slice(payload);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        bytes
    }

    #[test]
    fn annotation_names_a_field_method_and_enum() {
        let field = annotation_file(with_field(&[0x001a, 0x0002]), &[1, 0, 1, 1, 0x19, 0]);
        assert!(
            dex_use(&field).starts_with("class LRec; annotated LRec; open=LRec;.Record:LRec;\n"),
            "{}",
            dex_use(&field)
        );
        let enumerated = annotation_file(with_field(&[0x001a, 0x0002]), &[1, 0, 1, 1, 0x1b, 0]);
        assert!(
            dex_use(&enumerated).starts_with("class LRec; annotated LRec; open=LRec;.Record:LRec;\n"),
            "{}",
            dex_use(&enumerated)
        );
        let method = annotation_file(with_proto(&[0x001a, 0x0002]), &[1, 0, 1, 1, 0x1a, 0]);
        assert!(
            dex_use(&method).starts_with("class LRec; annotated LRec; open=LRec;.open(LRec;)LRec;\n"),
            "{}",
            dex_use(&method)
        );
        let missing = annotation_file(sample(&[0x001a, 0x0002]), &[1, 0, 1, 1, 0x19, 0]);
        assert!(
            dex_use(&missing).starts_with("class LRec; annotated LRec; open=0x0\n"),
            "{}",
            dex_use(&missing)
        );
    }

    #[test]
    fn annotation_names_a_method_type_and_handle() {
        let typed = annotation_file(with_proto(&[0x001a, 0x0002]), &[1, 0, 2, 1, 0x15, 0, 2, 0x17, 2]);
        assert!(
            dex_use(&typed).starts_with("class LRec; annotated LRec; open=proto (LRec;)LRec; Record=Record\n"),
            "{}",
            dex_use(&typed)
        );
        let missing = annotation_file(sample(&[0x001a, 0x0002]), &[1, 0, 1, 1, 0x15, 0]);
        assert!(
            dex_use(&missing).starts_with("class LRec; annotated LRec; open=proto 0x0\n"),
            "{}",
            dex_use(&missing)
        );
        let handle = annotation_file(sample(&[0x001a, 0x0002]), &[1, 0, 2, 1, 0x16, 0, 2, 0x17, 2]);
        assert!(
            dex_use(&handle).starts_with("class LRec; annotated LRec; open=method-handle 0x0 Record=Record\n"),
            "{}",
            dex_use(&handle)
        );
        let mut resolved = annotation_file(with_field(&[0x001a, 0x0002]), &[1, 0, 2, 1, 0x16, 0, 2, 0x17, 2]);
        let map = resolved.len();
        resolved.extend_from_slice(&1u32.to_le_bytes());
        resolved.extend_from_slice(&8u16.to_le_bytes());
        resolved.extend_from_slice(&0u16.to_le_bytes());
        resolved.extend_from_slice(&1u32.to_le_bytes());
        let offset_at = resolved.len();
        resolved.extend_from_slice(&0u32.to_le_bytes());
        let item = resolved.len() as u32;
        resolved[offset_at..offset_at + 4].copy_from_slice(&item.to_le_bytes());
        resolved.extend_from_slice(&1u16.to_le_bytes());
        resolved.extend_from_slice(&0u16.to_le_bytes());
        resolved.extend_from_slice(&0u16.to_le_bytes());
        resolved.extend_from_slice(&0u16.to_le_bytes());
        resolved[52..56].copy_from_slice(&(map as u32).to_le_bytes());
        let resolved_size = resolved.len() as u32;
        resolved[32..36].copy_from_slice(&resolved_size.to_le_bytes());
        assert!(
            dex_use(&resolved).starts_with("class LRec; annotated LRec; open=method-handle static-get LRec;.Record:LRec; Record=Record\n"),
            "{}",
            dex_use(&resolved)
        );
        let mut invoked = annotation_file(with_proto(&[0x001a, 0x0002]), &[1, 0, 2, 1, 0x16, 0, 2, 0x17, 2]);
        let invoke_map = invoked.len();
        invoked.extend_from_slice(&1u32.to_le_bytes());
        invoked.extend_from_slice(&8u16.to_le_bytes());
        invoked.extend_from_slice(&0u16.to_le_bytes());
        invoked.extend_from_slice(&1u32.to_le_bytes());
        let invoke_off = invoked.len();
        invoked.extend_from_slice(&0u32.to_le_bytes());
        let invoke_item = invoked.len() as u32;
        invoked[invoke_off..invoke_off + 4].copy_from_slice(&invoke_item.to_le_bytes());
        invoked.extend_from_slice(&4u16.to_le_bytes());
        invoked.extend_from_slice(&0u16.to_le_bytes());
        invoked.extend_from_slice(&0u16.to_le_bytes());
        invoked.extend_from_slice(&0u16.to_le_bytes());
        invoked[52..56].copy_from_slice(&(invoke_map as u32).to_le_bytes());
        let invoke_size = invoked.len() as u32;
        invoked[32..36].copy_from_slice(&invoke_size.to_le_bytes());
        assert!(
            dex_use(&invoked).starts_with("class LRec; annotated LRec; open=method-handle invoke-static LRec;.open(LRec;)LRec; Record=Record\n"),
            "{}",
            dex_use(&invoked)
        );
    }

    #[test]
    fn annotation_names_numbers() {
        let negative = annotation_file(sample(&[0x001a, 0x0002]), &[1, 0, 2, 1, 0x00, 0xff, 2, 0x17, 2]);
        assert!(
            dex_use(&negative).starts_with("class LRec; annotated LRec; open=-1 Record=Record\n"),
            "{}",
            dex_use(&negative)
        );
        let wide = annotation_file(sample(&[0x001a, 0x0002]), &[1, 0, 2, 1, 0xe6, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0x17, 2]);
        assert!(
            dex_use(&wide).starts_with("class LRec; annotated LRec; open=1 Record=Record\n"),
            "{}",
            dex_use(&wide)
        );
        let fraction = annotation_file(sample(&[0x001a, 0x0002]), &[1, 0, 1, 1, 0x10, 0x40]);
        assert!(
            dex_use(&fraction).starts_with("class LRec; annotated LRec; open=2\n"),
            "{}",
            dex_use(&fraction)
        );
        let letter = annotation_file(sample(&[0x001a, 0x0002]), &[1, 0, 1, 1, 0x03, 0x52]);
        assert!(
            dex_use(&letter).starts_with("class LRec; annotated LRec; open='R'\n"),
            "{}",
            dex_use(&letter)
        );
    }

    #[test]
    fn annotation_array_names_the_strings() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&(dir + 16).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.extend_from_slice(&[1, 0, 1, 1, 0x1f, 1, 0x17, 2]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        assert!(
            dex_use(&bytes).starts_with("class LRec; annotated LRec; open=[Record]\n"),
            "{}",
            dex_use(&bytes)
        );
        let mut longer = sample(&[0x001a, 0x0002]);
        let longer_dir = longer.len() as u32;
        longer.extend_from_slice(&(longer_dir + 16).to_le_bytes());
        longer.extend_from_slice(&0u32.to_le_bytes());
        longer.extend_from_slice(&0u32.to_le_bytes());
        longer.extend_from_slice(&0u32.to_le_bytes());
        longer.extend_from_slice(&1u32.to_le_bytes());
        longer.extend_from_slice(&(longer_dir + 24).to_le_bytes());
        longer.extend_from_slice(&[1, 0, 2, 1, 0x1f, 5, 0x17, 2, 0x17, 2, 0x17, 2, 0x17, 2, 0x17, 2, 2, 0x17, 2]);
        let longer_size = longer.len() as u32;
        longer[32..36].copy_from_slice(&longer_size.to_le_bytes());
        longer[0x9c..0xa0].copy_from_slice(&longer_dir.to_le_bytes());
        assert!(
            dex_use(&longer).starts_with("class LRec; annotated LRec; open=[Record,Record,Record,Record,...] Record=Record\n"),
            "{}",
            dex_use(&longer)
        );
    }

    #[test]
    fn annotation_value_names_the_string() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&(dir + 16).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.extend_from_slice(&[1, 0, 1, 1, 0x17, 2]);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        assert!(
            dex_use(&bytes).starts_with("class LRec; annotated LRec; open=Record\n"),
            "{}",
            dex_use(&bytes)
        );
    }

    #[test]
    fn parameter_annotation_names_the_type() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 32).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 40).to_le_bytes());
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        assert!(
            dex_use(&bytes).starts_with("class LRec; method open parameter 0 annotated LRec;\n"),
            "{}",
            dex_use(&bytes)
        );
    }

    #[test]
    fn field_annotation_names_the_type() {
        let mut bytes = with_field(&[0x001a, 0x0002]);
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 32).to_le_bytes());
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        assert!(
            dex_use(&bytes).starts_with("class LRec; field LRec;.Record:LRec; annotated LRec;\n"),
            "{}",
            dex_use(&bytes)
        );
    }

    #[test]
    fn method_annotation_names_the_type() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 32).to_le_bytes());
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        assert!(
            dex_use(&bytes).starts_with("class LRec; method open annotated LRec;\n"),
            "{}",
            dex_use(&bytes)
        );
    }

    #[test]
    fn annotation_names_the_type() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let dir = bytes.len() as u32;
        bytes.extend_from_slice(&(dir + 16).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(dir + 24).to_le_bytes());
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        bytes[0x9c..0xa0].copy_from_slice(&dir.to_le_bytes());
        assert!(
            dex_use(&bytes).starts_with("class LRec; annotated LRec;\n"),
            "{}",
            dex_use(&bytes)
        );
    }

    #[test]
    fn class_kind_names_interface_and_annotation() {
        let mut interface = sample(&[0x001a, 0x0002]);
        interface[0x8d] = 0x02;
        assert!(
            dex_use(&interface).starts_with("class LRec; kind interface\n"),
            "{}",
            dex_use(&interface)
        );
        let mut annotation = sample(&[0x001a, 0x0002]);
        annotation[0x8d] = 0x20;
        assert!(dex_use(&annotation).starts_with("class LRec; kind annotation\n"));
        let mut enumerated = sample(&[0x001a, 0x0002]);
        enumerated[0x8d] = 0x40;
        assert!(
            dex_use(&enumerated).starts_with("class LRec; kind enum\n"),
            "{}",
            dex_use(&enumerated)
        );
    }

    #[test]
    fn uncoded_names_native_and_abstract() {
        let mut native = sample(&[0x001a, 0x0002]);
        native[0xad] = 0x80;
        native[0xae] = 0x02;
        native[0xaf] = 0;
        assert_eq!(
            dex_use(&native),
            "class LRec; method open native\nThe instruction was not executed."
        );
        let mut abstract_method = sample(&[0x001a, 0x0002]);
        abstract_method[0xad] = 0x80;
        abstract_method[0xae] = 0x08;
        abstract_method[0xaf] = 0;
        assert!(dex_use(&abstract_method).starts_with("class LRec; method open abstract\n"),);
    }

    #[test]
    fn try_names_the_catch() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xb6] = 1;
        bytes[0xc8..0xca].copy_from_slice(&2u16.to_le_bytes());
        bytes[0xca..0xcc].copy_from_slice(&1u16.to_le_bytes());
        bytes[0xcc] = 1;
        bytes[0xcd] = 1;
        bytes[0xcf] = 4;
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open try 0x0 count 2 catch LRec; at 0x4\nclass LRec; method open const-string 2 Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn short_try_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xb6] = 1;
        bytes.truncate(0xc4);
        assert_eq!(
            dex_use(&bytes),
            "class ? method ? try was not read\nclass ? method ? const-string 2 unreadable\nThe instruction was not executed."
        );
    }

    #[test]
    fn short_catch_handler_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xb6] = 1;
        bytes[0xc8..0xca].copy_from_slice(&2u16.to_le_bytes());
        bytes[0xca..0xcc].copy_from_slice(&1u16.to_le_bytes());
        bytes[0xcd] = 2;
        bytes[0xcf] = 4;
        bytes.truncate(0xd0);
        assert_eq!(
            dex_use(&bytes),
            "class ? method ? try 0x0 count 2 catch unreadable at 0x4, unreadable\nclass ? method ? const-string 2 unreadable\nThe instruction was not executed."
        );
    }

    #[test]
    fn short_catch_all_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xb6] = 1;
        bytes[0xc8..0xca].copy_from_slice(&2u16.to_le_bytes());
        bytes[0xca..0xcc].copy_from_slice(&1u16.to_le_bytes());
        bytes[0xcd] = 0x7f;
        bytes[0xcf] = 4;
        bytes.truncate(0xd0);
        assert_eq!(
            dex_use(&bytes),
            "class ? method ? try 0x0 count 2 catch unreadable at 0x4, any unreadable\nclass ? method ? const-string 2 unreadable\nThe instruction was not executed."
        );
    }

    #[test]
    fn const_string_names_record() {
        let report = dex_use(&sample(&[0x0000, 0x001a, 0x0002]));
        assert_eq!(
            report,
            "class LRec; method open const-string 2 Record\nThe instruction was not executed."
        );
    }

    #[test]
    fn long_string_is_marked() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let data = bytes.len() as u32;
        bytes.push(65);
        bytes.extend(std::iter::repeat(b'A').take(65));
        bytes[0x78..0x7c].copy_from_slice(&data.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        let shown = "A".repeat(64);
        assert_eq!(
            dex_use(&bytes),
            format!("class LRec; method open const-string 2 {shown}...\nThe instruction was not executed.")
        );
    }

    #[test]
    fn jumbo_and_unreadable_and_unknown() {
        let jumbo = dex_use(&sample(&[0x001b, 0x0002, 0x0000]));
        assert!(jumbo.contains("const-string/jumbo 2 Record"), "{jumbo}");
        assert!(jumbo.contains("not executed"), "{jumbo}");
        let missing = dex_use(&sample(&[0x001a, 0x0009]));
        assert!(missing.contains("const-string 9 unreadable"), "{missing}");
        let unknown = dex_use(&sample(&[0x003e, 0x001a, 0x0002]));
        assert!(unknown.starts_with("no const-string, invoke, field, or type instruction was read"), "{unknown}");
        assert!(unknown.contains("an unrecognized instruction stopped the list\nThe instruction was not executed."), "{unknown}");
    }

    #[test]
    fn instruction_did_not_fit() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xbc..0xc0].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "no const-string, invoke, field, or type instruction was read\nan instruction did not fit\nThe instruction was not executed."
        );
    }

    #[test]
    fn missing_code_unit_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xbc..0xc0].copy_from_slice(&3u32.to_le_bytes());
        bytes.truncate(0xc4);
        assert_eq!(
            dex_use(&bytes),
            "class ? method ? const-string 2 unreadable\na code unit was not in the file\nThe instruction was not executed."
        );
    }

    #[test]
    fn short_code_item_is_named() {
        let mut bytes = vec![0u8; 152];
        bytes[..4].copy_from_slice(b"dex\n");
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&112u32.to_le_bytes());
        bytes[136..140].copy_from_slice(&144u32.to_le_bytes());
        bytes[144..152].copy_from_slice(&[0, 0, 1, 0, 0, 0, 0x94, 0x01]);
        assert_eq!(
            dex_use(&bytes),
            "no const-string, invoke, field, or type instruction was read\nthe code item was not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_parameter_cap_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 33]);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\nlater debug parameters were not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_position_cap_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.push(10);
        bytes.push(0);
        bytes.push(0x0a);
        bytes.extend(std::iter::repeat(0x19u8).take(64));
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record line 6\nlater debug positions were not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_end_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 0]);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\nthe debug info ended before its end opcode\nThe instruction was not executed."
        );
    }

    #[test]
    fn short_debug_parameter_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 1]);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\nthe debug info ended before its end opcode\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_line_start_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x08, 0]);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\nthe debug line start was not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_local_cap_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 0]);
        for _ in 0..65 {
            bytes.extend_from_slice(&[0x03, 0x00, 0x01, 0x00]);
        }
        bytes.push(0);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\nlater debug locals were not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_high_register_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 0, 0x03, 0x20, 0x01, 0x00, 0x00]);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\na debug local above register 31 was not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_high_end_local_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 0, 0x05, 0x20, 0x00]);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\na debug local above register 31 was not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_high_parameter_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        bytes[0xb0] = 40;
        bytes[0xb2] = 1;
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 1, 1, 0]);
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\na debug parameter above register 31 was not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn debug_opcode_cap_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let debug = bytes.len() as u32;
        bytes.extend_from_slice(&[1, 0]);
        bytes.extend(std::iter::repeat(0x07u8).take(1025));
        bytes[0xb8..0xbc].copy_from_slice(&debug.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        assert_eq!(
            dex_use(&bytes),
            "class LRec; method open const-string 2 Record\nlater debug opcodes were not read\nThe instruction was not executed."
        );
    }

    #[test]
    fn listing_cap_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let code = bytes.len();
        bytes.resize(code + 16 + 33 * 4, 0);
        bytes[code] = 1;
        bytes[code + 12..code + 16].copy_from_slice(&66u32.to_le_bytes());
        for index in 0..33 {
            let at = code + 16 + index * 4;
            bytes[at..at + 2].copy_from_slice(&0x001au16.to_le_bytes());
            bytes[at + 2..at + 4].copy_from_slice(&2u16.to_le_bytes());
        }
        bytes[0xae] = ((code & 0x7f) as u8) | 0x80;
        bytes[0xaf] = (code >> 7) as u8;
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        let report = dex_use(&bytes);
        assert_eq!(report.matches("const-string 2 Record").count(), 32, "{report}");
        assert!(report.contains("later instructions were not read\nThe instruction was not executed."), "{report}");
    }

    #[test]
    fn try_after_the_listing_cap_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let base = bytes.len();
        let code_size = 20usize;
        let try_at = base + 32 * code_size;
        bytes.resize(try_at + 28, 0);
        for index in 0..32 {
            let off = base + index * code_size;
            bytes[off] = 1;
            bytes[off + 12..off + 16].copy_from_slice(&2u32.to_le_bytes());
            bytes[off + 16..off + 18].copy_from_slice(&0x001au16.to_le_bytes());
            bytes[off + 18..off + 20].copy_from_slice(&2u16.to_le_bytes());
        }
        bytes[try_at] = 1;
        bytes[try_at + 6] = 1;
        bytes[try_at + 12..try_at + 16].copy_from_slice(&2u32.to_le_bytes());
        bytes[try_at + 16..try_at + 18].copy_from_slice(&0x001au16.to_le_bytes());
        bytes[try_at + 18..try_at + 20].copy_from_slice(&2u16.to_le_bytes());
        let mut class_data = vec![0, 0, 33, 0];
        for index in 0..32 {
            class_data.push(0);
            class_data.push(0);
            let mut value = base + index * code_size;
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                class_data.push(byte);
                if value == 0 {
                    break;
                }
            }
        }
        class_data.push(0);
        class_data.push(0);
        let mut value = try_at;
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            class_data.push(byte);
            if value == 0 {
                break;
            }
        }
        let data = bytes.len() as u32;
        bytes.extend_from_slice(&class_data);
        bytes[0xa0..0xa4].copy_from_slice(&data.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        let report = dex_use(&bytes);
        assert_eq!(report.matches("const-string 2 Record").count(), 32, "{report}");
        assert!(report.contains("later tries were not read\n"), "{report}");
    }

    #[test]
    fn uncoded_method_cap_is_named() {
        let mut bytes = sample(&[0x001a, 0x0002]);
        let data = bytes.len() as u32;
        let mut class_data = vec![0, 0, 33, 0];
        for _ in 0..33 {
            class_data.extend_from_slice(&[0, 0x80, 0x02, 0]);
        }
        bytes.extend_from_slice(&class_data);
        bytes[0xa0..0xa4].copy_from_slice(&data.to_le_bytes());
        let file_size = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&file_size.to_le_bytes());
        let report = dex_use(&bytes);
        assert_eq!(report.matches("method open native").count(), 32, "{report}");
        assert!(report.contains("later methods were not read\nThe instruction was not executed."), "{report}");
    }

    #[test]
    fn type_names_the_class() {
        let report = dex_use(&sample(&[0x0022, 0x0000]));
        assert_eq!(
            report,
            "class LRec; method open new-instance 0 LRec;\nThe instruction was not executed."
        );
        let missing = dex_use(&sample(&[0x001f, 0x0009]));
        assert!(missing.contains("check-cast 9 ?"), "{missing}");
    }

    #[test]
    fn field_names_the_owner() {
        let report = dex_use(&with_field(&[0x0052, 0x0000]));
        assert_eq!(
            report,
            "class LRec; method open iget 0 LRec;.Record:LRec;\nThe instruction was not executed."
        );
        let missing = dex_use(&with_field(&[0x0054, 0x0009]));
        assert!(missing.contains("iget-object 9 ?.?"), "{missing}");
    }

    #[test]
    fn invoke_names_the_method() {
        let report = dex_use(&sample(&[0x0000, 0x006e, 0x0000, 0x0010]));
        assert_eq!(
            report,
            "class LRec; method open invoke-virtual 0 LRec;.open\nThe instruction was not executed."
        );
        let missing = dex_use(&sample(&[0x006e, 0x0009, 0x0000]));
        assert!(missing.contains("invoke-virtual 9 ?.?"), "{missing}");
        let typed = dex_use(&with_proto(&[0x0000, 0x006e, 0x0000, 0x0010]));
        assert_eq!(
            typed,
            "class LRec; method open invoke-virtual 0 LRec;.open(LRec;)LRec;\nThe instruction was not executed."
        );
    }

    #[test]
    fn not_dex_and_short_header() {
        assert_eq!(
            dex_use(b""),
            "Not a DEX file.\nThe instruction was not executed."
        );
        let mut short = b"dex\n035\0".to_vec();
        short.resize(40, 0);
        assert!(dex_use(&short).starts_with("The DEX header is shorter than its class definitions."));
    }
}

//! Reconstructed text for a DEX subset: `const/4`, `const/16`, `const`, `add-int/2addr`, `sub-int/2addr`, `mul-int/2addr`, `div-int/2addr`, `rem-int/2addr`, `and-int/2addr`, `or-int/2addr`, `xor-int/2addr`, `shl-int/2addr`, `shr-int/2addr`, `ushr-int/2addr`, `neg-int`, `not-int`, `neg-long`, `not-long`, `neg-float`, `neg-double`, `int-to-long`, `int-to-float`, `int-to-double`, `long-to-int`, `long-to-float`, `long-to-double`, `float-to-int`, `float-to-long`, `float-to-double`, `double-to-int`, `double-to-long`, `double-to-float`, `int-to-byte`, `int-to-char`, `int-to-short`, `add-int`, `sub-int`, `mul-int`, `div-int`, `rem-int`, `and-int`, `or-int`, `xor-int`, `shl-int`, `shr-int`, `ushr-int`, `add-long`, `sub-long`, `mul-long`, `div-long`, `rem-long`, `and-long`, `or-long`, `xor-long`, `shl-long`, `shr-long`, `ushr-long`, `add-float`, `sub-float`, `mul-float`, `div-float`, `rem-float`, `add-double`, `sub-double`, `mul-double`, `div-double`, `rem-double`, `add-long/2addr`, `sub-long/2addr`, `mul-long/2addr`, `div-long/2addr`, `rem-long/2addr`, `and-long/2addr`, `or-long/2addr`, `xor-long/2addr`, `shl-long/2addr`, `shr-long/2addr`, `ushr-long/2addr`, `add-float/2addr`, `sub-float/2addr`, `mul-float/2addr`, `div-float/2addr`, `rem-float/2addr`, `add-double/2addr`, `sub-double/2addr`, `mul-double/2addr`, `div-double/2addr`, `rem-double/2addr`, `add-int/lit16`, `rsub-int`, `mul-int/lit16`, `div-int/lit16`, `rem-int/lit16`, `and-int/lit16`, `or-int/lit16`, `xor-int/lit16`, `add-int/lit8`, `rsub-int/lit8`, `mul-int/lit8`, `div-int/lit8`, `rem-int/lit8`, `and-int/lit8`, `or-int/lit8`, `xor-int/lit8`, `shl-int/lit8`, `shr-int/lit8`, `ushr-int/lit8`, `if-eq`, `if-ne`, `if-lt`, `const-string`, `return`, and `return-void`.
//! This is not a Java compiler and does not recover general Java source.
//! The method is not executed.
//!
//! Little-endian fields this reader parses:
//! - magic `dex\n`
//! - `string_ids_size` `u32` at 56, `string_ids_off` `u32` at 60
//! - `class_defs_size` `u32` at 96, `class_defs_off` `u32` at 100
//! - each string id is a `u32` file offset of a uleb128 byte length (at most five bytes)
//!   and that many data bytes, copied until a 0
//! - each class def is 32 bytes; `class_data_off` is the `u32` at +24
//! - class data is four uleb128 counts (static fields, instance fields, direct methods,
//!   virtual methods), then two uleb128s per field, then three uleb128s per method
//!   (`method_idx_diff`, `access_flags`, `code_off`)
//! - a code item is 16 bytes; `insns_size` is the `u32` at +12; code units follow as `u16`
//! - `const-string` is opcode `0x1a`, two units: destination register in the high byte,
//!   string index in the next unit
//! - `return-void` is opcode `0x0e` with a zero high byte, one unit
//!
//! Register 0 is written as `a`. The word `String` names that opcode. Checksum, signature,
//! endian tag, map, prototypes, debug info, and every other opcode are not reconstructed.

#![forbid(unsafe_code)]

const HEADER: usize = 112;
const MAX_METHODS: usize = 32;
const CLASS_CAP: usize = 64;
const FIELD_CAP: usize = 64;
const SCAN_METHODS: usize = 256;
const INSN_CAP: u32 = 64;
const TEXT_CAP: usize = 64;
const STRING_BYTES: usize = 4096;

const CLOSE: &str = "The method was not executed. This is reconstructed text, not a Java compiler.";

/// Reconstruct const-string / return-void text from a DEX subset.
///
/// At most 32 methods are listed. A later method is named, not printed.
/// The returned text is reconstructed. The method is not executed.
pub fn dex_source(bytes: &[u8]) -> String {
    let mut report = String::new();
    if !bytes.starts_with(b"dex\n") {
        report.push_str("Not a DEX file.\n");
        report.push_str(CLOSE);
        return report;
    }
    if bytes.len() < HEADER {
        report.push_str("The DEX header is shorter than its class definitions.\n");
        report.push_str(CLOSE);
        return report;
    }
    let class_count = u32_at(bytes, 96).unwrap_or(0) as usize;
    let class_off = u32_at(bytes, 100).unwrap_or(0) as usize;
    let mut listed = 0usize;
    let mut more = class_count > CLASS_CAP;
    for index in 0..class_count.min(CLASS_CAP) {
        if listed >= MAX_METHODS {
            more = true;
            break;
        }
        let Some(at) = index.checked_mul(32).and_then(|n| class_off.checked_add(n)) else {
            more = true;
            break;
        };
        let Some(data_at) = at.checked_add(24) else {
            more = true;
            break;
        };
        let Some(data_off) = u32_at(bytes, data_at) else {
            more = true;
            continue;
        };
        if data_off == 0 {
            continue;
        }
        if !class_methods(bytes, data_off as usize, &mut report, &mut listed) {
            more = true;
            break;
        }
    }
    if more {
        report.push_str("later methods were not listed\n");
    }
    report.push_str(CLOSE);
    report
}

fn class_methods(bytes: &[u8], at: usize, report: &mut String, listed: &mut usize) -> bool {
    let Some((static_fields, next)) = uleb(bytes, at) else {
        return false;
    };
    let Some((instance_fields, next)) = uleb(bytes, next) else {
        return false;
    };
    let Some((direct_methods, next)) = uleb(bytes, next) else {
        return false;
    };
    let Some((virtual_methods, mut cursor)) = uleb(bytes, next) else {
        return false;
    };
    let Some(fields) = (static_fields as usize).checked_add(instance_fields as usize) else {
        return false;
    };
    if fields > FIELD_CAP {
        return false;
    }
    for _ in 0..fields {
        let Some((_, next)) = uleb(bytes, cursor) else {
            return false;
        };
        let Some((_, next)) = uleb(bytes, next) else {
            return false;
        };
        cursor = next;
    }
    let Some(total) = (direct_methods as usize).checked_add(virtual_methods as usize) else {
        return false;
    };
    let walk = total.min(SCAN_METHODS);
    for _ in 0..walk {
        let Some((_diff, next)) = uleb(bytes, cursor) else {
            return false;
        };
        let Some((_access, next)) = uleb(bytes, next) else {
            return false;
        };
        let Some((code_off, next)) = uleb(bytes, next) else {
            return false;
        };
        cursor = next;
        if code_off == 0 {
            continue;
        }
        if *listed >= MAX_METHODS {
            return false;
        }
        report.push_str(&method_text(bytes, code_off as usize));
        report.push('\n');
        *listed += 1;
    }
    total <= SCAN_METHODS
}

fn method_text(bytes: &[u8], code_off: usize) -> String {
    let Some(size_at) = code_off.checked_add(12) else {
        return "the code item was not read".to_string();
    };
    let Some(insns_size) = u32_at(bytes, size_at) else {
        return "the code item was not read".to_string();
    };
    if insns_size == 0 || insns_size > INSN_CAP {
        return "the method body was not turned into Java text".to_string();
    }
    let mut pc = 0usize;
    let mut parts = Vec::new();
    while pc < insns_size as usize {
        let Some(unit) = unit_at(bytes, code_off, pc) else {
            return "the code item was not read".to_string();
        };
        let op = (unit & 0xff) as u8;
        if op == 0x1a {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(index) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let Some(text) = string_at(bytes, u32::from(index)) else {
                return "the method body was not turned into Java text".to_string();
            };
            let name = register_name(unit >> 8);
            parts.push(format!("String {name} = \"{}\";", escape(&text)));
            pc += 2;
        } else if op == 0x12 {
            let reg = (unit >> 8) & 0x0f;
            let nibble = i32::from((unit >> 12) & 0x0f);
            let literal = if nibble >= 8 { nibble - 16 } else { nibble };
            parts.push(format!("int {} = {literal};", register_name(reg)));
            pc += 1;
        } else if op == 0x13 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let literal = i32::from(raw as i16);
            parts.push(format!("int {} = {literal};", register_name(unit >> 8)));
            pc += 2;
        } else if op == 0x14 {
            if pc + 2 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(low) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let Some(high) = unit_at(bytes, code_off, pc + 2) else {
                return "the code item was not read".to_string();
            };
            let literal = i32::from_le_bytes([
                low as u8,
                (low >> 8) as u8,
                high as u8,
                (high >> 8) as u8,
            ]);
            parts.push(format!("int {} = {literal};", register_name(unit >> 8)));
            pc += 3;
        } else if op == 0xb0 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} + {right};"));
            pc += 1;
        } else if op == 0xb1 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} - {right};"));
            pc += 1;
        } else if op == 0xb2 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} * {right};"));
            pc += 1;
        } else if op == 0xb3 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} / {right};"));
            pc += 1;
        } else if op == 0xb4 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} % {right};"));
            pc += 1;
        } else if op == 0xb5 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} & {right};"));
            pc += 1;
        } else if op == 0xb6 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} | {right};"));
            pc += 1;
        } else if op == 0xb7 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} ^ {right};"));
            pc += 1;
        } else if op == 0xb8 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} << {right};"));
            pc += 1;
        } else if op == 0xb9 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} >> {right};"));
            pc += 1;
        } else if op == 0xba {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("{left} = {left} >>> {right};"));
            pc += 1;
        } else if op == 0xbb {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} + {right};"));
            pc += 1;
        } else if op == 0xbc {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} - {right};"));
            pc += 1;
        } else if op == 0xbd {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} * {right};"));
            pc += 1;
        } else if op == 0xbe {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} / {right};"));
            pc += 1;
        } else if op == 0xbf {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} % {right};"));
            pc += 1;
        } else if op == 0xc0 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} & {right};"));
            pc += 1;
        } else if op == 0xc1 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} | {right};"));
            pc += 1;
        } else if op == 0xc2 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} ^ {right};"));
            pc += 1;
        } else if op == 0xc3 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} << {right};"));
            pc += 1;
        } else if op == 0xc4 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} >> {right};"));
            pc += 1;
        } else if op == 0xc5 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("long {left} = {left} >>> {right};"));
            pc += 1;
        } else if op == 0xc6 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("float {left} = {left} + {right};"));
            pc += 1;
        } else if op == 0xc7 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("float {left} = {left} - {right};"));
            pc += 1;
        } else if op == 0xc8 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("float {left} = {left} * {right};"));
            pc += 1;
        } else if op == 0xc9 {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("float {left} = {left} / {right};"));
            pc += 1;
        } else if op == 0xca {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("float {left} = {left} % {right};"));
            pc += 1;
        } else if op == 0xcb {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("double {left} = {left} + {right};"));
            pc += 1;
        } else if op == 0xcc {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("double {left} = {left} - {right};"));
            pc += 1;
        } else if op == 0xcd {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("double {left} = {left} * {right};"));
            pc += 1;
        } else if op == 0xce {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("double {left} = {left} / {right};"));
            pc += 1;
        } else if op == 0xcf {
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            parts.push(format!("double {left} = {left} % {right};"));
            pc += 1;
        } else if op == 0xd0 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {source} + {literal};"));
            pc += 2;
        } else if op == 0xd1 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {literal} - {source};"));
            pc += 2;
        } else if op == 0xd2 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {source} * {literal};"));
            pc += 2;
        } else if op == 0xd3 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {source} / {literal};"));
            pc += 2;
        } else if op == 0xd4 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {source} % {literal};"));
            pc += 2;
        } else if op == 0xd5 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {source} & {literal};"));
            pc += 2;
        } else if op == 0xd6 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {source} | {literal};"));
            pc += 2;
        } else if op == 0xd7 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            let literal = i32::from(raw as i16);
            parts.push(format!("{dest} = {source} ^ {literal};"));
            pc += 2;
        } else if op == 0xd8 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} + {literal};"));
            pc += 2;
        } else if op == 0xd9 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {literal} - {source};"));
            pc += 2;
        } else if op == 0xda {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} * {literal};"));
            pc += 2;
        } else if op == 0xdb {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} / {literal};"));
            pc += 2;
        } else if op == 0xdc {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} % {literal};"));
            pc += 2;
        } else if op == 0xdd {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} & {literal};"));
            pc += 2;
        } else if op == 0xde {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} | {literal};"));
            pc += 2;
        } else if op == 0xdf {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} ^ {literal};"));
            pc += 2;
        } else if op == 0xe0 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} << {literal};"));
            pc += 2;
        } else if op == 0xe1 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} >> {literal};"));
            pc += 2;
        } else if op == 0xe2 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let source = register_name(raw & 0xff);
            let literal = i32::from((raw >> 8) as i8);
            parts.push(format!("{dest} = {source} >>> {literal};"));
            pc += 2;
        } else if op == 0x32 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({left} == {right}) goto {shown};"));
            pc += 2;
        } else if op == 0x33 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({left} != {right}) goto {shown};"));
            pc += 2;
        } else if op == 0x34 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let left = register_name((unit >> 8) & 0x0f);
            let right = register_name(unit >> 12);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({left} < {right}) goto {shown};"));
            pc += 2;
        } else if op == 0x7b {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("{dest} = -{source};"));
            pc += 1;
        } else if op == 0x7c {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("{dest} = ~{source};"));
            pc += 1;
        } else if op == 0x7d {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("long {dest} = -{source};"));
            pc += 1;
        } else if op == 0x7e {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("long {dest} = ~{source};"));
            pc += 1;
        } else if op == 0x7f {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("float {dest} = -{source};"));
            pc += 1;
        } else if op == 0x80 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("double {dest} = -{source};"));
            pc += 1;
        } else if op == 0x81 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("long {dest} = (long) {source};"));
            pc += 1;
        } else if op == 0x82 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("float {dest} = (float) {source};"));
            pc += 1;
        } else if op == 0x83 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("double {dest} = (double) {source};"));
            pc += 1;
        } else if op == 0x84 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("int {dest} = (int) {source};"));
            pc += 1;
        } else if op == 0x85 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("float {dest} = (float) (long) {source};"));
            pc += 1;
        } else if op == 0x86 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("double {dest} = (double) (long) {source};"));
            pc += 1;
        } else if op == 0x87 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("int {dest} = (int) (float) {source};"));
            pc += 1;
        } else if op == 0x88 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("long {dest} = (long) (float) {source};"));
            pc += 1;
        } else if op == 0x89 {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("double {dest} = (double) (float) {source};"));
            pc += 1;
        } else if op == 0x8a {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("int {dest} = (int) (double) {source};"));
            pc += 1;
        } else if op == 0x8b {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("long {dest} = (long) (double) {source};"));
            pc += 1;
        } else if op == 0x8c {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("float {dest} = (float) (double) {source};"));
            pc += 1;
        } else if op == 0x8d {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("byte {dest} = (byte) {source};"));
            pc += 1;
        } else if op == 0x8e {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("char {dest} = (char) {source};"));
            pc += 1;
        } else if op == 0x8f {
            let dest = register_name((unit >> 8) & 0x0f);
            let source = register_name(unit >> 12);
            parts.push(format!("short {dest} = (short) {source};"));
            pc += 1;
        } else if op == 0x90 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} + {right};"));
            pc += 2;
        } else if op == 0x91 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} - {right};"));
            pc += 2;
        } else if op == 0x92 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} * {right};"));
            pc += 2;
        } else if op == 0x93 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} / {right};"));
            pc += 2;
        } else if op == 0x94 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} % {right};"));
            pc += 2;
        } else if op == 0x95 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} & {right};"));
            pc += 2;
        } else if op == 0x96 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} | {right};"));
            pc += 2;
        } else if op == 0x97 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} ^ {right};"));
            pc += 2;
        } else if op == 0x98 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} << {right};"));
            pc += 2;
        } else if op == 0x99 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} >> {right};"));
            pc += 2;
        } else if op == 0x9a {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("{dest} = {left} >>> {right};"));
            pc += 2;
        } else if op == 0x9b {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} + {right};"));
            pc += 2;
        } else if op == 0x9c {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} - {right};"));
            pc += 2;
        } else if op == 0x9d {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} * {right};"));
            pc += 2;
        } else if op == 0x9e {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} / {right};"));
            pc += 2;
        } else if op == 0x9f {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} % {right};"));
            pc += 2;
        } else if op == 0xa0 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} & {right};"));
            pc += 2;
        } else if op == 0xa1 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} | {right};"));
            pc += 2;
        } else if op == 0xa2 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} ^ {right};"));
            pc += 2;
        } else if op == 0xa3 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} << {right};"));
            pc += 2;
        } else if op == 0xa4 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} >> {right};"));
            pc += 2;
        } else if op == 0xa5 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("long {dest} = {left} >>> {right};"));
            pc += 2;
        } else if op == 0xa6 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("float {dest} = {left} + {right};"));
            pc += 2;
        } else if op == 0xa7 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("float {dest} = {left} - {right};"));
            pc += 2;
        } else if op == 0xa8 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("float {dest} = {left} * {right};"));
            pc += 2;
        } else if op == 0xa9 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("float {dest} = {left} / {right};"));
            pc += 2;
        } else if op == 0xaa {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("float {dest} = {left} % {right};"));
            pc += 2;
        } else if op == 0xab {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("double {dest} = {left} + {right};"));
            pc += 2;
        } else if op == 0xac {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("double {dest} = {left} - {right};"));
            pc += 2;
        } else if op == 0xad {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("double {dest} = {left} * {right};"));
            pc += 2;
        } else if op == 0xae {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("double {dest} = {left} / {right};"));
            pc += 2;
        } else if op == 0xaf {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let left = register_name(regs & 0xff);
            let right = register_name(regs >> 8);
            parts.push(format!("double {dest} = {left} % {right};"));
            pc += 2;
        } else if op == 0x0f {
            parts.push(format!("return {};", register_name(unit >> 8)));
            pc += 1;
        } else if op == 0x0e && unit >> 8 == 0 {
            parts.push("return;".to_string());
            pc += 1;
        } else {
            return "the method body was not turned into Java text".to_string();
        }
    }
    if parts.is_empty() {
        return "the method body was not turned into Java text".to_string();
    }
    parts.join(" ")
}

fn register_name(reg: u16) -> String {
    if reg < 26 {
        char::from(b'a' + reg as u8).to_string()
    } else {
        format!("v{reg}")
    }
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn string_at(bytes: &[u8], index: u32) -> Option<String> {
    let count = u32_at(bytes, 56)?;
    if index >= count {
        return None;
    }
    let table = u32_at(bytes, 60)? as usize;
    let at = table.checked_add((index as usize).checked_mul(4)?)?;
    let data = u32_at(bytes, at)? as usize;
    string_text(bytes, data)
}

fn string_text(bytes: &[u8], data: usize) -> Option<String> {
    let (len, start) = uleb(bytes, data)?;
    let len = usize::try_from(len).ok()?;
    if len > STRING_BYTES {
        return None;
    }
    let stop = start.checked_add(len)?;
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
        text.push(ch);
    }
    if more {
        text.push_str("...");
    }
    Some(text)
}

fn uleb(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut value = 0u32;
    let mut shift = 0u32;
    for step in 0..5 {
        let pos = at.checked_add(step)?;
        let byte = *bytes.get(pos)?;
        let piece = u32::from(byte & 0x7f);
        if shift == 28 && piece > 0x0f {
            return None;
        }
        value |= piece << shift;
        if byte & 0x80 == 0 {
            let next = pos.checked_add(1)?;
            return Some((value, next));
        }
        shift += 7;
    }
    None
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let raw = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let raw = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn unit_at(bytes: &[u8], code_off: usize, pc: usize) -> Option<u16> {
    let at = code_off.checked_add(16)?.checked_add(pc.checked_mul(2)?)?;
    u16_at(bytes, at)
}

#[cfg(test)]
mod tests {
    use super::dex_source;

    const CLOSE: &str = "The method was not executed. This is reconstructed text, not a Java compiler.";

    fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_uleb(bytes: &mut Vec<u8>, mut value: u32) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn file(literal: &[u8], static_fields: usize, methods: &[Vec<u16>]) -> Vec<u8> {
        let string_ids_off = 112usize;
        let type_ids_off = string_ids_off + 12;
        let method_ids_off = if methods.is_empty() { 0 } else { type_ids_off + 4 };
        let class_defs_off = if methods.is_empty() {
            type_ids_off + 4
        } else {
            method_ids_off + methods.len() * 8
        };
        let mut bytes = vec![0u8; class_defs_off + 32];
        bytes[0..8].copy_from_slice(b"dex\n035\0");
        put_u32(&mut bytes, 56, 3);
        put_u32(&mut bytes, 60, string_ids_off as u32);
        put_u32(&mut bytes, 96, 1);
        put_u32(&mut bytes, 100, class_defs_off as u32);
        if !methods.is_empty() {
            put_u32(&mut bytes, 88, methods.len() as u32);
            put_u32(&mut bytes, 92, method_ids_off as u32);
            for index in 0..methods.len() {
                let at = method_ids_off + index * 8;
                put_u32(&mut bytes, at + 4, 1);
            }
        }
        let texts: [&[u8]; 3] = [b"LRec;", b"open", literal];
        let mut offs = [0u32; 3];
        for (index, text) in texts.iter().enumerate() {
            offs[index] = bytes.len() as u32;
            assert!(text.len() < 128);
            bytes.push(text.len() as u8);
            bytes.extend_from_slice(text);
            bytes.push(0);
        }
        for (index, off) in offs.iter().enumerate() {
            put_u32(&mut bytes, string_ids_off + index * 4, *off);
        }
        let mut code_offs = Vec::new();
        for insns in methods {
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
            code_offs.push(bytes.len() as u32);
            push_u16(&mut bytes, 1);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, insns.len() as u32);
            for unit in insns {
                push_u16(&mut bytes, *unit);
            }
        }
        let class_data = bytes.len() as u32;
        push_uleb(&mut bytes, static_fields as u32);
        push_uleb(&mut bytes, 0);
        push_uleb(&mut bytes, methods.len() as u32);
        push_uleb(&mut bytes, 0);
        for _ in 0..static_fields {
            push_uleb(&mut bytes, 0);
            push_uleb(&mut bytes, 0);
        }
        for (index, code_off) in code_offs.iter().enumerate() {
            push_uleb(&mut bytes, if index == 0 { 0 } else { 1 });
            push_uleb(&mut bytes, 0);
            push_uleb(&mut bytes, *code_off);
        }
        put_u32(&mut bytes, class_defs_off + 24, class_data);
        bytes
    }

    fn body(reg: u16, string_index: u16) -> Vec<u16> {
        vec![(reg << 8) | 0x1a, string_index, 0x000e]
    }

    #[test]
    fn missing_prefix() {
        assert_eq!(
            dex_source(b"nope"),
            format!("Not a DEX file.\n{CLOSE}")
        );
    }

    #[test]
    fn short_header() {
        assert_eq!(
            dex_source(b"dex\n"),
            format!("The DEX header is shorter than its class definitions.\n{CLOSE}")
        );
    }

    #[test]
    fn const_string_and_return_void() {
        let report = dex_source(&file(b"Record", 0, &[body(0, 2)]));
        assert_eq!(
            report,
            format!("String a = \"Record\"; return;\n{CLOSE}")
        );
    }

    #[test]
    fn literal_comes_from_the_pool() {
        let report = dex_source(&file(b"Note", 0, &[body(0, 2)]));
        assert_eq!(report, format!("String a = \"Note\"; return;\n{CLOSE}"));
    }

    #[test]
    fn second_register_is_b() {
        let report = dex_source(&file(b"Record", 0, &[body(1, 2)]));
        assert_eq!(report, format!("String b = \"Record\"; return;\n{CLOSE}"));
    }

    #[test]
    fn high_register_keeps_its_number() {
        let report = dex_source(&file(b"Record", 0, &[body(26, 2)]));
        assert_eq!(report, format!("String v26 = \"Record\"; return;\n{CLOSE}"));
    }

    #[test]
    fn two_assignments_then_return() {
        let insns = vec![0x001a, 2, 0x011a, 2, 0x000e];
        let report = dex_source(&file(b"Record", 0, &[insns]));
        assert_eq!(
            report,
            format!("String a = \"Record\"; String b = \"Record\"; return;\n{CLOSE}")
        );
    }

    #[test]
    fn const_four_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x000e]]));
        assert_eq!(report, format!("int a = 1; return;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0xf112, 0x000e]]));
        assert_eq!(negative, format!("int b = -1; return;\n{CLOSE}"));
    }

    #[test]
    fn const_four_then_return_value() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x000f]]));
        assert_eq!(report, format!("int a = 1; return a;\n{CLOSE}"));
    }

    #[test]
    fn const_sixteen_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x0013, 300, 0x000e]]));
        assert_eq!(report, format!("int a = 300; return;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x0013, 0xfffe, 0x000e]]));
        assert_eq!(negative, format!("int a = -2; return;\n{CLOSE}"));
    }

    #[test]
    fn const_wide_literal_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x0014, 0x86a0, 0x0001, 0x000e]]));
        assert_eq!(report, format!("int a = 100000; return;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x0014, 0xffff, 0xffff, 0x000e]]));
        assert_eq!(negative, format!("int a = -1; return;\n{CLOSE}"));
    }

    #[test]
    fn add_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10b0, 0x000f]]));
        assert_eq!(report, format!("int a = 1; int b = 2; a = a + b; return a;\n{CLOSE}"));
    }

    #[test]
    fn add_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10bb, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 1; int b = 2; long a = a + b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x5012, 0x2112, 0x10bc, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 5; int b = 2; long a = a - b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x4112, 0x10bd, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 3; int b = 4; long a = a * b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn divide_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x6012, 0x2112, 0x10be, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 6; int b = 2; long a = a / b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x7012, 0x3112, 0x10bf, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 7; int b = 3; long a = a % b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn and_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x7012, 0x3112, 0x10c0, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 7; int b = 3; long a = a & b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn or_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10c1, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 1; int b = 2; long a = a | b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn xor_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10c2, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 1; int b = 2; long a = a ^ b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn shift_left_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10c3, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 1; int b = 2; long a = a << b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn shift_right_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x4012, 0x1112, 0x10c4, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 4; int b = 1; long a = a >> b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn unsigned_shift_right_long_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x4012, 0x1112, 0x10c5, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 4; int b = 1; long a = a >>> b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn add_float_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10c6, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 1; int b = 2; float a = a + b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_float_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x5012, 0x2112, 0x10c7, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 5; int b = 2; float a = a - b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_float_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x4112, 0x10c8, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 3; int b = 4; float a = a * b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn divide_float_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x6012, 0x2112, 0x10c9, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 6; int b = 2; float a = a / b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_float_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x7012, 0x3112, 0x10ca, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 7; int b = 3; float a = a % b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn add_double_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10cb, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 1; int b = 2; double a = a + b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_double_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x5012, 0x2112, 0x10cc, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 5; int b = 2; double a = a - b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_double_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x4112, 0x10cd, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 3; int b = 4; double a = a * b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn divide_double_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x6012, 0x2112, 0x10ce, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 6; int b = 2; double a = a / b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_double_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x7012, 0x3112, 0x10cf, 0x000f]]));
        assert_eq!(
            report,
            format!("int a = 7; int b = 3; double a = a % b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn add_register_and_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x10d0, 2, 0x000f]]));
        assert_eq!(positive, format!("int b = 5; a = b + 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x10d0, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 5; a = b + -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d0]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn reverse_subtract_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x10d1, 2, 0x000f]]));
        assert_eq!(positive, format!("int b = 5; a = 2 - b; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x10d1, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 5; a = -2 - b; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d1]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_register_and_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x10d2, 3, 0x000f]]));
        assert_eq!(positive, format!("int b = 5; a = b * 3; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x10d2, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 5; a = b * -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d2]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn divide_register_and_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x10d3, 2, 0x000f]]));
        assert_eq!(positive, format!("int b = 6; a = b / 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x10d3, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 6; a = b / -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d3]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_register_and_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x10d4, 3, 0x000f]]));
        assert_eq!(positive, format!("int b = 7; a = b % 3; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x10d4, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 7; a = b % -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d4]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn and_register_and_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x10d5, 3, 0x000f]]));
        assert_eq!(positive, format!("int b = 7; a = b & 3; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x10d5, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 7; a = b & -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d5]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn or_register_and_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x10d6, 2, 0x000f]]));
        assert_eq!(positive, format!("int b = 1; a = b | 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x10d6, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 1; a = b | -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d6]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn xor_register_and_literal_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x10d7, 2, 0x000f]]));
        assert_eq!(positive, format!("int b = 1; a = b ^ 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x10d7, 0xfffe, 0x000f]]));
        assert_eq!(negative, format!("int b = 1; a = b ^ -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x10d7]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn add_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00d8, 0x0201, 0x000f]]));
        assert_eq!(positive, format!("int b = 1; a = b + 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00d8, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 1; a = b + -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00d8]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn reverse_subtract_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x00d9, 0x0201, 0x000f]]));
        assert_eq!(positive, format!("int b = 5; a = 2 - b; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x00d9, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 5; a = -2 - b; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00d9]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x00da, 0x0301, 0x000f]]));
        assert_eq!(positive, format!("int b = 5; a = b * 3; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x00da, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 5; a = b * -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00da]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn divide_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x00db, 0x0201, 0x000f]]));
        assert_eq!(positive, format!("int b = 6; a = b / 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x00db, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 6; a = b / -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00db]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x00dc, 0x0301, 0x000f]]));
        assert_eq!(positive, format!("int b = 7; a = b % 3; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x00dc, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 7; a = b % -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00dc]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn and_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x00dd, 0x0301, 0x000f]]));
        assert_eq!(positive, format!("int b = 7; a = b & 3; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x00dd, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 7; a = b & -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00dd]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn or_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00de, 0x0201, 0x000f]]));
        assert_eq!(positive, format!("int b = 1; a = b | 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00de, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 1; a = b | -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00de]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn xor_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00df, 0x0201, 0x000f]]));
        assert_eq!(positive, format!("int b = 1; a = b ^ 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00df, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 1; a = b ^ -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00df]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn shift_left_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00e0, 0x0201, 0x000f]]));
        assert_eq!(positive, format!("int b = 1; a = b << 2; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x00e0, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 1; a = b << -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00e0]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn shift_right_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x00e1, 0x0101, 0x000f]]));
        assert_eq!(positive, format!("int b = 4; a = b >> 1; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x00e1, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 4; a = b >> -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00e1]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn unsigned_shift_right_register_and_byte_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x00e2, 0x0101, 0x000f]]));
        assert_eq!(positive, format!("int b = 4; a = b >>> 1; return a;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x00e2, 0xfe01, 0x000f]]));
        assert_eq!(negative, format!("int b = 4; a = b >>> -2; return a;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x00e2]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn if_eq_registers_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1032, 0x0004, 0x000e]]));
        assert_eq!(positive, format!("if (a == b) goto +4; return;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1032, 0xfffe, 0x000e]]));
        assert_eq!(negative, format!("if (a == b) goto -2; return;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x1032]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn if_ne_registers_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1033, 0x0004, 0x000e]]));
        assert_eq!(positive, format!("if (a != b) goto +4; return;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1033, 0xfffe, 0x000e]]));
        assert_eq!(negative, format!("if (a != b) goto -2; return;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x1033]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn if_lt_registers_then_return() {
        let positive = dex_source(&file(b"Record", 0, &[vec![0x1034, 0x0004, 0x000e]]));
        assert_eq!(positive, format!("if (a < b) goto +4; return;\n{CLOSE}"));
        let negative = dex_source(&file(b"Record", 0, &[vec![0x1034, 0xfffe, 0x000e]]));
        assert_eq!(negative, format!("if (a < b) goto -2; return;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x1034]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x5012, 0x2112, 0x10b1, 0x000f]]));
        assert_eq!(report, format!("int a = 5; int b = 2; a = a - b; return a;\n{CLOSE}"));
    }

    #[test]
    fn multiply_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x4112, 0x10b2, 0x000f]]));
        assert_eq!(report, format!("int a = 3; int b = 4; a = a * b; return a;\n{CLOSE}"));
    }

    #[test]
    fn divide_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x6012, 0x2112, 0x10b3, 0x000f]]));
        assert_eq!(report, format!("int a = 6; int b = 2; a = a / b; return a;\n{CLOSE}"));
    }

    #[test]
    fn remainder_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x7012, 0x3112, 0x10b4, 0x000f]]));
        assert_eq!(report, format!("int a = 7; int b = 3; a = a % b; return a;\n{CLOSE}"));
    }

    #[test]
    fn and_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x7012, 0x3112, 0x10b5, 0x000f]]));
        assert_eq!(report, format!("int a = 7; int b = 3; a = a & b; return a;\n{CLOSE}"));
    }

    #[test]
    fn or_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10b6, 0x000f]]));
        assert_eq!(report, format!("int a = 1; int b = 2; a = a | b; return a;\n{CLOSE}"));
    }

    #[test]
    fn xor_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10b7, 0x000f]]));
        assert_eq!(report, format!("int a = 1; int b = 2; a = a ^ b; return a;\n{CLOSE}"));
    }

    #[test]
    fn shift_left_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x1012, 0x2112, 0x10b8, 0x000f]]));
        assert_eq!(report, format!("int a = 1; int b = 2; a = a << b; return a;\n{CLOSE}"));
    }

    #[test]
    fn shift_right_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x4012, 0x1112, 0x10b9, 0x000f]]));
        assert_eq!(report, format!("int a = 4; int b = 1; a = a >> b; return a;\n{CLOSE}"));
    }

    #[test]
    fn unsigned_shift_right_two_registers_then_return() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x4012, 0x1112, 0x10ba, 0x000f]]));
        assert_eq!(report, format!("int a = 4; int b = 1; a = a >>> b; return a;\n{CLOSE}"));
    }

    #[test]
    fn negate_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x007b, 0x000f]]));
        assert_eq!(same, format!("int a = 3; a = -a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x107b, 0x000f]]));
        assert_eq!(other, format!("int b = 3; a = -b; return a;\n{CLOSE}"));
    }

    #[test]
    fn bitwise_not_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x007c, 0x000f]]));
        assert_eq!(same, format!("int a = 3; a = ~a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x107c, 0x000f]]));
        assert_eq!(other, format!("int b = 3; a = ~b; return a;\n{CLOSE}"));
    }

    #[test]
    fn negate_long_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x007d, 0x000f]]));
        assert_eq!(same, format!("int a = 3; long a = -a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x107d, 0x000f]]));
        assert_eq!(other, format!("int b = 3; long a = -b; return a;\n{CLOSE}"));
    }

    #[test]
    fn bitwise_not_long_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x007e, 0x000f]]));
        assert_eq!(same, format!("int a = 3; long a = ~a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x107e, 0x000f]]));
        assert_eq!(other, format!("int b = 3; long a = ~b; return a;\n{CLOSE}"));
    }

    #[test]
    fn negate_float_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x007f, 0x000f]]));
        assert_eq!(same, format!("int a = 3; float a = -a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x107f, 0x000f]]));
        assert_eq!(other, format!("int b = 3; float a = -b; return a;\n{CLOSE}"));
    }

    #[test]
    fn negate_double_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0080, 0x000f]]));
        assert_eq!(same, format!("int a = 3; double a = -a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1080, 0x000f]]));
        assert_eq!(other, format!("int b = 3; double a = -b; return a;\n{CLOSE}"));
    }

    #[test]
    fn int_to_long_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0081, 0x000f]]));
        assert_eq!(same, format!("int a = 3; long a = (long) a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1081, 0x000f]]));
        assert_eq!(other, format!("int b = 3; long a = (long) b; return a;\n{CLOSE}"));
    }

    #[test]
    fn int_to_float_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0082, 0x000f]]));
        assert_eq!(same, format!("int a = 3; float a = (float) a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1082, 0x000f]]));
        assert_eq!(other, format!("int b = 3; float a = (float) b; return a;\n{CLOSE}"));
    }

    #[test]
    fn int_to_double_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0083, 0x000f]]));
        assert_eq!(same, format!("int a = 3; double a = (double) a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1083, 0x000f]]));
        assert_eq!(other, format!("int b = 3; double a = (double) b; return a;\n{CLOSE}"));
    }

    #[test]
    fn long_to_int_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0084, 0x000f]]));
        assert_eq!(same, format!("int a = 3; int a = (int) a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1084, 0x000f]]));
        assert_eq!(other, format!("int b = 3; int a = (int) b; return a;\n{CLOSE}"));
    }

    #[test]
    fn long_to_float_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0085, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; float a = (float) (long) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1085, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; float a = (float) (long) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn long_to_double_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0086, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; double a = (double) (long) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1086, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; double a = (double) (long) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn float_to_int_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0087, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; int a = (int) (float) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1087, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; int a = (int) (float) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn float_to_long_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0088, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; long a = (long) (float) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1088, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; long a = (long) (float) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn float_to_double_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x0089, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; double a = (double) (float) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x1089, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; double a = (double) (float) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn double_to_int_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x008a, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; int a = (int) (double) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x108a, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; int a = (int) (double) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn double_to_long_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x008b, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; long a = (long) (double) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x108b, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; long a = (long) (double) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn double_to_float_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x008c, 0x000f]]));
        assert_eq!(
            same,
            format!("int a = 3; float a = (float) (double) a; return a;\n{CLOSE}")
        );
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x108c, 0x000f]]));
        assert_eq!(
            other,
            format!("int b = 3; float a = (float) (double) b; return a;\n{CLOSE}")
        );
    }

    #[test]
    fn int_to_byte_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x008d, 0x000f]]));
        assert_eq!(same, format!("int a = 3; byte a = (byte) a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x108d, 0x000f]]));
        assert_eq!(other, format!("int b = 3; byte a = (byte) b; return a;\n{CLOSE}"));
    }

    #[test]
    fn int_to_char_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x008e, 0x000f]]));
        assert_eq!(same, format!("int a = 3; char a = (char) a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x108e, 0x000f]]));
        assert_eq!(other, format!("int b = 3; char a = (char) b; return a;\n{CLOSE}"));
    }

    #[test]
    fn int_to_short_then_return() {
        let same = dex_source(&file(b"Record", 0, &[vec![0x3012, 0x008f, 0x000f]]));
        assert_eq!(same, format!("int a = 3; short a = (short) a; return a;\n{CLOSE}"));
        let other = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x108f, 0x000f]]));
        assert_eq!(other, format!("int b = 3; short a = (short) b; return a;\n{CLOSE}"));
    }

    #[test]
    fn add_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0090, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 1; int c = 2; a = b + c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0390, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 1; int c = 2; d = b + c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0090]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x0091, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 5; int c = 2; a = b - c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x0391, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 5; int c = 2; d = b - c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0091]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x0092, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 3; int c = 4; a = b * c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x0392, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 3; int c = 4; d = b * c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0092]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn divide_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x0093, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 6; int c = 2; a = b / c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x0393, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 6; int c = 2; d = b / c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0093]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x0094, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 7; int c = 3; a = b % c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x0394, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 7; int c = 3; d = b % c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0094]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn and_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x0095, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 7; int c = 3; a = b & c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x0395, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 7; int c = 3; d = b & c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0095]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn or_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0096, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 1; int c = 2; a = b | c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0396, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 1; int c = 2; d = b | c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0096]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn xor_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0097, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 1; int c = 2; a = b ^ c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0397, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 1; int c = 2; d = b ^ c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0097]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn shift_left_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0098, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 1; int c = 2; a = b << c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x0398, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 1; int c = 2; d = b << c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0098]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn shift_right_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x0099, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 4; int c = 1; a = b >> c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x0399, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 4; int c = 1; d = b >> c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x0099]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn unsigned_shift_right_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x009a, 0x0201, 0x000f]]));
        assert_eq!(into_a, format!("int b = 4; int c = 1; a = b >>> c; return a;\n{CLOSE}"));
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x039a, 0x0201, 0x030f]]));
        assert_eq!(into_d, format!("int b = 4; int c = 1; d = b >>> c; return d;\n{CLOSE}"));
        let short = dex_source(&file(b"Record", 0, &[vec![0x009a]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn add_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x009b, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 1; int c = 2; long a = b + c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x039b, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 1; int c = 2; long d = b + c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x009b]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x009c, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 5; int c = 2; long a = b - c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x039c, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 5; int c = 2; long d = b - c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x009c]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x009d, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 3; int c = 4; long a = b * c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x039d, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 3; int c = 4; long d = b * c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x009d]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn divide_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x009e, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 6; int c = 2; long a = b / c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x039e, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 6; int c = 2; long d = b / c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x009e]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x009f, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 7; int c = 3; long a = b % c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x039f, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 7; int c = 3; long d = b % c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x009f]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn and_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x00a0, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 7; int c = 3; long a = b & c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x03a0, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 7; int c = 3; long d = b & c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a0]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn or_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x00a1, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 1; int c = 2; long a = b | c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x03a1, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 1; int c = 2; long d = b | c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a1]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn xor_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x00a2, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 1; int c = 2; long a = b ^ c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x03a2, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 1; int c = 2; long d = b ^ c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a2]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn shift_left_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x00a3, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 1; int c = 2; long a = b << c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x03a3, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 1; int c = 2; long d = b << c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a3]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn shift_right_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x00a4, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 4; int c = 1; long a = b >> c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x03a4, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 4; int c = 1; long d = b >> c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a4]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn unsigned_shift_right_long_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x00a5, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 4; int c = 1; long a = b >>> c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x4112, 0x1212, 0x03a5, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 4; int c = 1; long d = b >>> c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a5]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn add_float_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x00a6, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 1; int c = 2; float a = b + c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x03a6, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 1; int c = 2; float d = b + c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a6]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_float_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x00a7, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 5; int c = 2; float a = b - c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x03a7, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 5; int c = 2; float d = b - c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a7]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_float_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x00a8, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 3; int c = 4; float a = b * c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x03a8, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 3; int c = 4; float d = b * c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a8]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn divide_float_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x00a9, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 6; int c = 2; float a = b / c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x03a9, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 6; int c = 2; float d = b / c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00a9]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_float_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x00aa, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 7; int c = 3; float a = b % c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x03aa, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 7; int c = 3; float d = b % c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00aa]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn add_double_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x00ab, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 1; int c = 2; double a = b + c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x1112, 0x2212, 0x03ab, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 1; int c = 2; double d = b + c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00ab]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn subtract_double_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x00ac, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 5; int c = 2; double a = b - c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x5112, 0x2212, 0x03ac, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 5; int c = 2; double d = b - c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00ac]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn multiply_double_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x00ad, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 3; int c = 4; double a = b * c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x3112, 0x4212, 0x03ad, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 3; int c = 4; double d = b * c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00ad]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn divide_double_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x00ae, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 6; int c = 2; double a = b / c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x6112, 0x2212, 0x03ae, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 6; int c = 2; double d = b / c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00ae]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn remainder_double_three_registers_then_return() {
        let into_a = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x00af, 0x0201, 0x000f]]));
        assert_eq!(
            into_a,
            format!("int b = 7; int c = 3; double a = b % c; return a;\n{CLOSE}")
        );
        let into_d = dex_source(&file(b"Record", 0, &[vec![0x7112, 0x3212, 0x03af, 0x0201, 0x030f]]));
        assert_eq!(
            into_d,
            format!("int b = 7; int c = 3; double d = b % c; return d;\n{CLOSE}")
        );
        let short = dex_source(&file(b"Record", 0, &[vec![0x00af]]));
        assert_eq!(
            short,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn other_opcode_is_not_java_text() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x0000]]));
        assert_eq!(
            report,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
        assert!(!report.contains("String a"));
    }

    #[test]
    fn jumbo_const_string_is_not_reconstructed() {
        let report = dex_source(&file(b"Record", 0, &[vec![0x001b, 2, 0, 0x000e]]));
        assert_eq!(
            report,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
    }

    #[test]
    fn field_before_method_is_skipped() {
        let report = dex_source(&file(b"Record", 1, &[body(0, 2)]));
        assert_eq!(report, format!("String a = \"Record\"; return;\n{CLOSE}"));
    }

    #[test]
    fn thirty_two_methods() {
        let methods: Vec<Vec<u16>> = (0..32).map(|_| body(0, 2)).collect();
        let report = dex_source(&file(b"Record", 0, &methods));
        assert_eq!(report.matches("String a = \"Record\"; return;\n").count(), 32);
        assert_eq!(report.matches("later methods were not listed").count(), 0);
        assert!(report.ends_with(CLOSE));
    }

    #[test]
    fn later_methods_are_not_listed() {
        let methods: Vec<Vec<u16>> = (0..33).map(|_| body(0, 2)).collect();
        let report = dex_source(&file(b"Record", 0, &methods));
        assert_eq!(report.matches("String a = \"Record\"; return;\n").count(), 32);
        assert_eq!(report.matches("later methods were not listed\n").count(), 1);
        assert!(report.ends_with(CLOSE));
    }

    #[test]
    fn truncated_code_item() {
        let mut bytes = file(b"Record", 0, &[body(0, 2)]);
        let class_defs_off = u32::from_le_bytes(bytes[100..104].try_into().unwrap()) as usize;
        let class_data = u32::from_le_bytes(bytes[class_defs_off + 24..class_defs_off + 28].try_into().unwrap()) as usize;
        let saved = bytes[class_data..].to_vec();
        bytes.truncate(class_data - 2);
        bytes[8..8 + saved.len()].copy_from_slice(&saved);
        bytes[class_defs_off + 24..class_defs_off + 28].copy_from_slice(&8u32.to_le_bytes());
        let report = dex_source(&bytes);
        assert_eq!(report, format!("the code item was not read\n{CLOSE}"));
        assert!(!report.contains("Record"));
    }

    #[test]
    fn missing_string_is_not_invented() {
        let report = dex_source(&file(b"Record", 0, &[body(0, 9)]));
        assert_eq!(
            report,
            format!("the method body was not turned into Java text\n{CLOSE}")
        );
        assert!(!report.contains("Record"));
    }

    #[test]
    fn quote_is_escaped() {
        let report = dex_source(&file(br#"a"b"#, 0, &[body(0, 2)]));
        assert_eq!(report, format!("String a = \"a\\\"b\"; return;\n{CLOSE}"));
    }

    #[test]
    fn long_literal_is_marked() {
        let literal = vec![b'A'; 65];
        let shown = "A".repeat(64);
        let report = dex_source(&file(&literal, 0, &[body(0, 2)]));
        assert_eq!(
            report,
            format!("String a = \"{shown}...\"; return;\n{CLOSE}")
        );
    }

    #[test]
    fn interior_zero_stops_the_copy() {
        let report = dex_source(&file(b"Re\0cord", 0, &[body(0, 2)]));
        assert_eq!(report, format!("String a = \"Re\"; return;\n{CLOSE}"));
    }

    #[test]
    fn empty_class_has_no_method_body() {
        assert_eq!(dex_source(&file(b"Record", 0, &[])), CLOSE);
    }
}

//! Reconstructed text for a DEX subset: `const/4`, `const/16`, `const`, `add-int/2addr`, `sub-int/2addr`, `mul-int/2addr`, `div-int/2addr`, `rem-int/2addr`, `and-int/2addr`, `or-int/2addr`, `xor-int/2addr`, `shl-int/2addr`, `shr-int/2addr`, `ushr-int/2addr`, `neg-int`, `not-int`, `neg-long`, `not-long`, `neg-float`, `neg-double`, `int-to-long`, `int-to-float`, `int-to-double`, `long-to-int`, `long-to-float`, `long-to-double`, `float-to-int`, `float-to-long`, `float-to-double`, `double-to-int`, `double-to-long`, `double-to-float`, `int-to-byte`, `int-to-char`, `int-to-short`, `add-int`, `sub-int`, `mul-int`, `div-int`, `rem-int`, `and-int`, `or-int`, `xor-int`, `shl-int`, `shr-int`, `ushr-int`, `add-long`, `sub-long`, `mul-long`, `div-long`, `rem-long`, `and-long`, `or-long`, `xor-long`, `shl-long`, `shr-long`, `ushr-long`, `add-float`, `sub-float`, `mul-float`, `div-float`, `rem-float`, `add-double`, `sub-double`, `mul-double`, `div-double`, `rem-double`, `add-long/2addr`, `sub-long/2addr`, `mul-long/2addr`, `div-long/2addr`, `rem-long/2addr`, `and-long/2addr`, `or-long/2addr`, `xor-long/2addr`, `shl-long/2addr`, `shr-long/2addr`, `ushr-long/2addr`, `add-float/2addr`, `sub-float/2addr`, `mul-float/2addr`, `div-float/2addr`, `rem-float/2addr`, `add-double/2addr`, `sub-double/2addr`, `mul-double/2addr`, `div-double/2addr`, `rem-double/2addr`, `add-int/lit16`, `rsub-int`, `mul-int/lit16`, `div-int/lit16`, `rem-int/lit16`, `and-int/lit16`, `or-int/lit16`, `xor-int/lit16`, `add-int/lit8`, `rsub-int/lit8`, `mul-int/lit8`, `div-int/lit8`, `rem-int/lit8`, `and-int/lit8`, `or-int/lit8`, `xor-int/lit8`, `shl-int/lit8`, `shr-int/lit8`, `ushr-int/lit8`, `if-eq`, `if-ne`, `if-lt`, `if-ge`, `if-gt`, `if-le`, `if-eqz`, `if-nez`, `if-ltz`, `if-gez`, `if-gtz`, `if-lez`, `aget`, `aget-wide`, `aget-object`, `aget-boolean`, `aget-byte`, `aget-char`, `aget-short`, `aput`, `aput-wide`, `aput-object`, `aput-boolean`, `aput-byte`, `aput-char`, `aput-short`, `iget`, `iget-wide`, `iget-object`, `iget-boolean`, `iget-byte`, `iget-char`, `iget-short`, `iput`, `iput-wide`, `iput-object`, `iput-boolean`, `iput-byte`, `iput-char`, `iput-short`, `sget`, `sget-wide`, `sget-object`, `sget-boolean`, `sget-byte`, `sget-char`, `sget-short`, `sput`, `sput-wide`, `sput-object`, `sput-boolean`, `sput-byte`, `sput-char`, `sput-short`, `invoke-virtual`, `invoke-super`, `invoke-direct`, `invoke-static`, `invoke-interface`, `invoke-virtual/range`, `invoke-super/range`, `invoke-direct/range`, `invoke-static/range`, `invoke-interface/range`, `const-string`, `return`, and `return-void`.
//! This is not a Java compiler and does not recover general Java source.
//! The method is not executed.
//!
//! Little-endian fields this reader parses:
//! - magic `dex\n`
//! - `string_ids_size` `u32` at 56, `string_ids_off` `u32` at 60
//! - `class_defs_size` `u32` at 96, `class_defs_off` `u32` at 100
//! - each string id is a `u32` file offset of a uleb128 UTF-16 code-unit count
//!   (at most five bytes), then modified UTF-8 bytes ending in a 0 byte.
//!   That count is checked against the decoded code units. U+0000 is the
//!   two bytes `c0 80`. A supplementary character is a surrogate pair.
//!   The string is stored as those UTF-16 code units. An unpaired or
//!   out-of-order surrogate stays that unit and is written `\uXXXX`.
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
            let Some(stored) = string_at(bytes, u32::from(index)) else {
                return "the method body was not turned into Java text".to_string();
            };
            let name = register_name(unit >> 8);
            parts.push(format!("String {name} = \"{}\";", render_stored(&stored)));
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
        } else if op == 0x35 {
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
            parts.push(format!("if ({left} >= {right}) goto {shown};"));
            pc += 2;
        } else if op == 0x36 {
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
            parts.push(format!("if ({left} > {right}) goto {shown};"));
            pc += 2;
        } else if op == 0x37 {
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
            parts.push(format!("if ({left} <= {right}) goto {shown};"));
            pc += 2;
        } else if op == 0x38 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let register = register_name(unit >> 8);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({register} == 0) goto {shown};"));
            pc += 2;
        } else if op == 0x39 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let register = register_name(unit >> 8);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({register} != 0) goto {shown};"));
            pc += 2;
        } else if op == 0x3a {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let register = register_name(unit >> 8);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({register} < 0) goto {shown};"));
            pc += 2;
        } else if op == 0x3b {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let register = register_name(unit >> 8);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({register} >= 0) goto {shown};"));
            pc += 2;
        } else if op == 0x3c {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let register = register_name(unit >> 8);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({register} > 0) goto {shown};"));
            pc += 2;
        } else if op == 0x3d {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(raw) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let register = register_name(unit >> 8);
            let offset = i32::from(raw as i16);
            let shown = if offset < 0 {
                format!("{offset}")
            } else {
                format!("+{offset}")
            };
            parts.push(format!("if ({register} <= 0) goto {shown};"));
            pc += 2;
        } else if op == 0x44 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{dest} = {array}[{index}];"));
            pc += 2;
        } else if op == 0x45 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{dest} = (long) {array}[{index}];"));
            pc += 2;
        } else if op == 0x46 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{dest} = (Object) {array}[{index}];"));
            pc += 2;
        } else if op == 0x47 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{dest} = (boolean) {array}[{index}];"));
            pc += 2;
        } else if op == 0x48 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{dest} = (byte) {array}[{index}];"));
            pc += 2;
        } else if op == 0x49 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{dest} = (char) {array}[{index}];"));
            pc += 2;
        } else if op == 0x4a {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{dest} = (short) {array}[{index}];"));
            pc += 2;
        } else if op == 0x4b {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{array}[{index}] = {value};"));
            pc += 2;
        } else if op == 0x4c {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{array}[{index}] = (long) {value};"));
            pc += 2;
        } else if op == 0x4d {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{array}[{index}] = (Object) {value};"));
            pc += 2;
        } else if op == 0x4e {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{array}[{index}] = (boolean) {value};"));
            pc += 2;
        } else if op == 0x4f {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{array}[{index}] = (byte) {value};"));
            pc += 2;
        } else if op == 0x50 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{array}[{index}] = (char) {value};"));
            pc += 2;
        } else if op == 0x51 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(regs) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            let array = register_name(regs & 0xff);
            let index = register_name(regs >> 8);
            parts.push(format!("{array}[{index}] = (short) {value};"));
            pc += 2;
        } else if op == 0x52 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{dest} = {object}.field {field};"));
            pc += 2;
        } else if op == 0x53 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{dest} = (long) {object}.field {field};"));
            pc += 2;
        } else if op == 0x54 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{dest} = (Object) {object}.field {field};"));
            pc += 2;
        } else if op == 0x55 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{dest} = (boolean) {object}.field {field};"));
            pc += 2;
        } else if op == 0x56 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{dest} = (byte) {object}.field {field};"));
            pc += 2;
        } else if op == 0x57 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{dest} = (char) {object}.field {field};"));
            pc += 2;
        } else if op == 0x58 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{dest} = (short) {object}.field {field};"));
            pc += 2;
        } else if op == 0x59 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{object}.field {field} = {value};"));
            pc += 2;
        } else if op == 0x5a {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{object}.field {field} = (long) {value};"));
            pc += 2;
        } else if op == 0x5b {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{object}.field {field} = (Object) {value};"));
            pc += 2;
        } else if op == 0x5c {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{object}.field {field} = (boolean) {value};"));
            pc += 2;
        } else if op == 0x5d {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{object}.field {field} = (byte) {value};"));
            pc += 2;
        } else if op == 0x5e {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{object}.field {field} = (char) {value};"));
            pc += 2;
        } else if op == 0x5f {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name((unit >> 8) & 0x0f);
            let object = register_name(unit >> 12);
            parts.push(format!("{object}.field {field} = (short) {value};"));
            pc += 2;
        } else if op == 0x60 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            parts.push(format!("{dest} = field {field};"));
            pc += 2;
        } else if op == 0x61 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            parts.push(format!("{dest} = (long) field {field};"));
            pc += 2;
        } else if op == 0x62 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            parts.push(format!("{dest} = (Object) field {field};"));
            pc += 2;
        } else if op == 0x63 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            parts.push(format!("{dest} = (boolean) field {field};"));
            pc += 2;
        } else if op == 0x64 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            parts.push(format!("{dest} = (byte) field {field};"));
            pc += 2;
        } else if op == 0x65 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            parts.push(format!("{dest} = (char) field {field};"));
            pc += 2;
        } else if op == 0x66 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let dest = register_name(unit >> 8);
            parts.push(format!("{dest} = (short) field {field};"));
            pc += 2;
        } else if op == 0x67 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            parts.push(format!("field {field} = {value};"));
            pc += 2;
        } else if op == 0x68 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            parts.push(format!("field {field} = (long) {value};"));
            pc += 2;
        } else if op == 0x69 {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            parts.push(format!("field {field} = (Object) {value};"));
            pc += 2;
        } else if op == 0x6a {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            parts.push(format!("field {field} = (boolean) {value};"));
            pc += 2;
        } else if op == 0x6b {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            parts.push(format!("field {field} = (byte) {value};"));
            pc += 2;
        } else if op == 0x6c {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            parts.push(format!("field {field} = (char) {value};"));
            pc += 2;
        } else if op == 0x6d {
            if pc + 1 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(field) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let value = register_name(unit >> 8);
            parts.push(format!("field {field} = (short) {value};"));
            pc += 2;
        } else if (0x6e..=0x72).contains(&op) {
            if pc + 2 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(index) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let Some(regs) = unit_at(bytes, code_off, pc + 2) else {
                return "the code item was not read".to_string();
            };
            let count = usize::from((unit >> 12) & 0x0f);
            if count > 5 {
                return "the method body was not turned into Java text".to_string();
            }
            let nibbles = [
                regs & 0x0f,
                (regs >> 4) & 0x0f,
                (regs >> 8) & 0x0f,
                regs >> 12,
                (unit >> 8) & 0x0f,
            ];
            let names: Vec<String> = nibbles[..count].iter().copied().map(register_name).collect();
            let word = match op {
                0x6f => "super",
                0x70 => "direct",
                0x72 => "interface",
                _ => "method",
            };
            let call = if op == 0x71 {
                if names.is_empty() {
                    format!("method {index}();")
                } else {
                    format!("method {index}({});", names.join(", "))
                }
            } else if let Some((receiver, rest)) = names.split_first() {
                if rest.is_empty() {
                    format!("{receiver}.{word} {index}();")
                } else {
                    format!("{receiver}.{word} {index}({});", rest.join(", "))
                }
            } else {
                format!("{word} {index}();")
            };
            parts.push(call);
            pc += 3;
        } else if (0x74..=0x78).contains(&op) {
            if pc + 2 >= insns_size as usize {
                return "the method body was not turned into Java text".to_string();
            }
            let Some(index) = unit_at(bytes, code_off, pc + 1) else {
                return "the code item was not read".to_string();
            };
            let Some(first) = unit_at(bytes, code_off, pc + 2) else {
                return "the code item was not read".to_string();
            };
            let count = usize::from(unit >> 8);
            let names: Vec<String> = (0..count)
                .map(|offset| register_name(first.wrapping_add(offset as u16)))
                .collect();
            let word = match op {
                0x75 => "super",
                0x76 => "direct",
                0x78 => "interface",
                _ => "method",
            };
            let call = if op == 0x77 {
                if names.is_empty() {
                    format!("method {index} range();")
                } else {
                    format!("method {index} range({});", names.join(", "))
                }
            } else if let Some((receiver, rest)) = names.split_first() {
                if rest.is_empty() {
                    format!("{{{receiver}}}.{word} {index}();")
                } else {
                    format!("{{{receiver}}}.{word} {index}({});", rest.join(", "))
                }
            } else {
                format!("{word} {index} range();")
            };
            parts.push(call);
            pc += 3;
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

/// Decoded DEX string. `units` are the raw UTF-16 code units, including any
/// unpaired or out-of-order surrogates. `more` means display stopped after
/// 64 decoded characters.
struct StoredString {
    units: Vec<u16>,
    more: bool,
}

fn string_at(bytes: &[u8], index: u32) -> Option<StoredString> {
    let count = u32_at(bytes, 56)?;
    if index >= count {
        return None;
    }
    let table = u32_at(bytes, 60)? as usize;
    let at = table.checked_add((index as usize).checked_mul(4)?)?;
    let data = u32_at(bytes, at)? as usize;
    stored_string(bytes, data)
}

fn stored_string(bytes: &[u8], data: usize) -> Option<StoredString> {
    let (utf16_len, start) = uleb(bytes, data)?;
    let utf16_len = usize::try_from(utf16_len).ok()?;
    if utf16_len > STRING_BYTES {
        return None;
    }
    let units = mutf8_units(bytes, start, utf16_len)?;
    Some(take_display(&units, TEXT_CAP))
}

fn take_display(units: &[u16], cap: usize) -> StoredString {
    let mut kept = Vec::new();
    let mut count = 0usize;
    let mut index = 0usize;
    while index < units.len() {
        if count == cap {
            return StoredString {
                units: kept,
                more: true,
            };
        }
        let unit = units[index];
        if (0xd800..0xdc00).contains(&unit) {
            if let Some(low) = units.get(index.saturating_add(1)).copied() {
                if (0xdc00..0xe000).contains(&low) {
                    kept.push(unit);
                    kept.push(low);
                    index += 2;
                    count += 1;
                    continue;
                }
            }
        }
        kept.push(unit);
        index += 1;
        count += 1;
    }
    StoredString {
        units: kept,
        more: false,
    }
}

fn render_stored(stored: &StoredString) -> String {
    let mut out = String::new();
    for item in char::decode_utf16(stored.units.iter().copied()) {
        match item {
            Ok(ch) => out.push_str(&escape(&ch.to_string())),
            Err(err) => out.push_str(&format!("\\u{:04x}", err.unpaired_surrogate())),
        }
    }
    if stored.more {
        out.push_str("...");
    }
    out
}

/// Read `utf16_len` modified UTF-8 code units and require the terminating 0.
/// Each unit is one UTF-16 code unit. A supplementary character is two units.
fn mutf8_units(bytes: &[u8], start: usize, utf16_len: usize) -> Option<Vec<u16>> {
    let mut units = Vec::with_capacity(utf16_len);
    let mut at = start;
    while units.len() < utf16_len {
        let lead = *bytes.get(at)?;
        if lead == 0 {
            return None;
        }
        let (unit, next) = mutf8_unit(bytes, at)?;
        units.push(unit);
        at = next;
    }
    if bytes.get(at).copied() != Some(0) {
        return None;
    }
    Some(units)
}

fn mutf8_unit(bytes: &[u8], at: usize) -> Option<(u16, usize)> {
    let lead = *bytes.get(at)?;
    if lead & 0x80 == 0 {
        return Some((u16::from(lead), at.checked_add(1)?));
    }
    if lead & 0xe0 == 0xc0 {
        let next_at = at.checked_add(1)?;
        let cont = *bytes.get(next_at)?;
        if cont & 0xc0 != 0x80 {
            return None;
        }
        let unit = (u16::from(lead & 0x1f) << 6) | u16::from(cont & 0x3f);
        return Some((unit, next_at.checked_add(1)?));
    }
    if lead & 0xf0 == 0xe0 {
        let mid_at = at.checked_add(1)?;
        let end_at = at.checked_add(2)?;
        let mid = *bytes.get(mid_at)?;
        let end = *bytes.get(end_at)?;
        if mid & 0xc0 != 0x80 || end & 0xc0 != 0x80 {
            return None;
        }
        let unit = (u16::from(lead & 0x0f) << 12)
            | (u16::from(mid & 0x3f) << 6)
            | u16::from(end & 0x3f);
        return Some((unit, end_at.checked_add(1)?));
    }
    None
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

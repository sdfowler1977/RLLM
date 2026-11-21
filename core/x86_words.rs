//! x86 and x86-64 instruction lengths and integer listings.
//! Format reference: the published x86 instruction-format rules for prefixes,
//! REX, opcode, ModRM, SIB and immediates. The listing is derived here.
#![cfg_attr(not(test), allow(dead_code))]

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Next,
    Branch(Option<i64>),
    Call(Option<i64>),
    Return,
}

pub struct Word {
    pub len: usize,
    pub text: String,
    pub flow: Flow,
}

pub enum Step {
    Word(Word),
    /// Further bytes are not consumed. The reason names why alignment would be lost.
    Stop(&'static str),
}

pub struct Span {
    pub offset: u64,
    pub address: u64,
    pub size: u64,
    pub executable: bool,
}

const SUMMARY_LINES: usize = 96;
const TRAILER: &str = "x86 instruction summary ended.\n";

pub fn summary_trailer() -> &'static str {
    TRAILER
}

struct Prefixed {
    at: usize,
    operand16: bool,
    address_override: bool,
    rex: u8,
}

fn prefixes(bytes: &[u8], bits64: bool) -> Result<Prefixed, &'static str> {
    let mut at = 0;
    let mut operand16 = false;
    let mut address_override = false;
    let mut rex = 0u8;
    while at < bytes.len() && at < 15 {
        let byte = bytes[at];
        match byte {
            0xF0 | 0xF2 | 0xF3 | 0x2E | 0x36 | 0x3E | 0x26 | 0x64 | 0x65 => {
                rex = 0;
                at += 1;
            }
            0x66 => {
                operand16 = true;
                rex = 0;
                at += 1;
            }
            0x67 => {
                address_override = true;
                rex = 0;
                at += 1;
            }
            0x40..=0x4F if bits64 => {
                rex = byte;
                at += 1;
            }
            _ => break,
        }
    }
    if at >= 15 {
        return Err("prefix run exceeds 14 bytes");
    }
    if at >= bytes.len() {
        return Err("instruction continues past the available bytes");
    }
    Ok(Prefixed { at, operand16, address_override, rex })
}

fn address_bits(bits64: bool, address_override: bool) -> u8 {
    if bits64 {
        if address_override { 32 } else { 64 }
    } else if address_override {
        16
    } else {
        32
    }
}

fn operand_bits(bits64: bool, rex_w: bool, operand16: bool) -> u8 {
    if rex_w {
        64
    } else if operand16 {
        16
    } else if bits64 {
        32
    } else {
        32
    }
}

fn need<'a>(bytes: &'a [u8], at: usize, n: usize) -> Option<&'a [u8]> {
    bytes.get(at..at.checked_add(n)?)
}

/// Bytes after the ModRM byte: SIB and displacement. `None` when those bytes are absent.
fn modrm_tail(bytes: &[u8], at: usize, modrm: u8, address_bits: u8) -> Option<usize> {
    let mode = modrm >> 6;
    let rm = modrm & 7;
    if address_bits == 16 {
        let disp = match mode {
            0 if rm == 6 => 2,
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 0,
        };
        need(bytes, at, disp)?;
        return Some(disp);
    }
    let mut extra = 0usize;
    if mode != 3 && rm == 4 {
        extra = 1;
        need(bytes, at, extra)?;
    }
    let base = if extra == 1 { bytes[at] & 7 } else { rm };
    let disp = match mode {
        0 if base == 5 => 4,
        1 => 1,
        2 => 4,
        _ => 0,
    };
    extra += disp;
    need(bytes, at, extra)?;
    Some(extra)
}

#[derive(Clone, Copy)]
enum Imm {
    None,
    Ib,
    Iz,
    Iw,
    Io,
    Moffs,
    Enter,
}

struct Form {
    modrm: bool,
    imm: Imm,
    /// F6/F7: immediate only when the ModRM reg field is 0.
    test_imm: bool,
}

fn primary_form(op: u8, bits64: bool) -> Result<Form, &'static str> {
    let form = |modrm, imm| Form { modrm, imm, test_imm: false };
    match op {
        0x06 | 0x07 | 0x0E | 0x16 | 0x17 | 0x1E | 0x1F | 0x27 | 0x2F | 0x37 | 0x3F | 0x60 | 0x61 | 0xD6 | 0xCE
            if bits64 =>
        {
            Err("opcode is not defined in 64-bit mode")
        }
        0x82 | 0x9A | 0xEA | 0xD4 | 0xD5 if bits64 => Err("opcode is not defined in 64-bit mode"),
        0xC4 | 0xC5 if bits64 => Err("vector prefix; listing stops so later bytes stay aligned"),
        0x62 if bits64 => Err("vector prefix; listing stops so later bytes stay aligned"),
        0x00..=0x03 | 0x08..=0x0B | 0x10..=0x13 | 0x18..=0x1B | 0x20..=0x23 | 0x28..=0x2B | 0x30..=0x33 | 0x38..=0x3B => {
            Ok(form(true, Imm::None))
        }
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C | 0x6A | 0xA8 | 0xB0..=0xB7 | 0xCD | 0xE4..=0xE7 | 0x70..=0x7F | 0xEB | 0xE0..=0xE3 => {
            Ok(form(false, Imm::Ib))
        }
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D | 0x68 | 0xA9 | 0xE8 | 0xE9 => Ok(form(false, Imm::Iz)),
        0x69 => Ok(form(true, Imm::Iz)),
        0x6B | 0x80 | 0x83 | 0xC0 | 0xC1 | 0xC6 => Ok(form(true, Imm::Ib)),
        0x81 | 0xC7 => Ok(form(true, Imm::Iz)),
        0x84..=0x8F | 0xD0..=0xD3 | 0xD8..=0xDF | 0xF6 | 0xF7 | 0xFE | 0xFF | 0x63 => Ok(Form {
            modrm: true,
            imm: Imm::None,
            test_imm: matches!(op, 0xF6 | 0xF7),
        }),
        0xA0..=0xA3 => Ok(form(false, Imm::Moffs)),
        0xB8..=0xBF => Ok(form(false, Imm::Io)),
        0xC2 | 0xCA => Ok(form(false, Imm::Iw)),
        0xC8 => Ok(form(false, Imm::Enter)),
        0xD4 | 0xD5 => Ok(form(false, Imm::Ib)),
        0x9A | 0xEA => Ok(form(false, Imm::Iz)),
        _ => Ok(form(false, Imm::None)),
    }
}

fn secondary_form(op: u8) -> Result<Form, &'static str> {
    let plain = Form { modrm: false, imm: Imm::None, test_imm: false };
    let rm = Form { modrm: true, imm: Imm::None, test_imm: false };
    let rm_ib = Form { modrm: true, imm: Imm::Ib, test_imm: false };
    match op {
        0x04 | 0x0A | 0x0C | 0x0F | 0x36 | 0x39 | 0xB9 => Err("secondary opcode length is not established; listing stops"),
        0x05 | 0x06 | 0x07 | 0x08 | 0x09 | 0x0B | 0x0E | 0x30 | 0x31 | 0x32 | 0x33 | 0x34 | 0x35 | 0x77 | 0xA0 | 0xA1 | 0xA2 | 0xA8 | 0xA9 | 0xAA | 0xC8..=0xCF => {
            Ok(plain)
        }
        0x80..=0x8F => Ok(Form { modrm: false, imm: Imm::Iz, test_imm: false }),
        0x70..=0x73 | 0xA4 | 0xAC | 0xBA | 0xC2 | 0xC4 | 0xC5 | 0xC6 => Ok(rm_ib),
        _ => Ok(rm),
    }
}

fn imm_len(imm: Imm, operand_bits: u8, address_bits: u8) -> usize {
    match imm {
        Imm::None => 0,
        Imm::Ib => 1,
        Imm::Iw => 2,
        Imm::Iz => if operand_bits == 16 { 2 } else { 4 },
        Imm::Io => if operand_bits == 64 { 8 } else if operand_bits == 16 { 2 } else { 4 },
        Imm::Moffs => match address_bits {
            64 => 8,
            16 => 2,
            _ => 4,
        },
        Imm::Enter => 3,
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, byte) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn reg_name(index: u8, bits: u8, byte_high_ok: bool) -> &'static str {
    const R64: [&str; 16] = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];
    const R32: [&str; 16] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d", "r12d", "r13d", "r14d", "r15d"];
    const R16: [&str; 16] = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w", "r13w", "r14w", "r15w"];
    const R8: [&str; 16] = ["al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil", "r8b", "r9b", "r10b", "r11b", "r12b", "r13b", "r14b", "r15b"];
    const R8H: [&str; 4] = ["ah", "ch", "dh", "bh"];
    let index = index & 15;
    match bits {
        64 => R64[index as usize],
        32 => R32[index as usize],
        16 => R16[index as usize],
        _ if byte_high_ok && (4..8).contains(&index) => R8H[(index - 4) as usize],
        _ => R8[index as usize],
    }
}

fn read_u32(bytes: &[u8]) -> i64 {
    i32::from_le_bytes(bytes[..4].try_into().unwrap()) as i64
}

fn read_i8(byte: u8) -> i64 {
    byte as i8 as i64
}

/// Decode one instruction. `Stop` means this byte must not be consumed.
pub fn decode(bytes: &[u8], bits64: bool) -> Step {
    let prefixed = match prefixes(bytes, bits64) {
        Ok(value) => value,
        Err(reason) => return Step::Stop(reason),
    };
    let rex_w = prefixed.rex & 0x08 != 0;
    let rex_r = u8::from(prefixed.rex & 0x04 != 0);
    let rex_b = u8::from(prefixed.rex & 0x01 != 0);
    let op_bits = operand_bits(bits64, rex_w, prefixed.operand16);
    let addr_bits = address_bits(bits64, prefixed.address_override);
    let mut at = prefixed.at;
    let first = bytes[at];
    at += 1;
    let parsed = if first == 0x0F {
        let Some(opcode) = bytes.get(at).copied() else {
            return Step::Stop("instruction continues past the available bytes");
        };
        at += 1;
        if opcode == 0x38 || opcode == 0x3A {
            let Some(third) = bytes.get(at).copied() else {
                return Step::Stop("instruction continues past the available bytes");
            };
            at += 1;
            match finish(bytes, at, opcode, true, Some(third), Form { modrm: true, imm: if opcode == 0x3A { Imm::Ib } else { Imm::None }, test_imm: false }, op_bits, addr_bits, rex_r, rex_b, prefixed.rex != 0, bits64) {
                Ok(word) => Step::Word(word),
                Err(reason) => Step::Stop(reason),
            }
        } else {
            match secondary_form(opcode) {
                Ok(form) => match finish(bytes, at, opcode, true, None, form, op_bits, addr_bits, rex_r, rex_b, prefixed.rex != 0, bits64) {
                    Ok(word) => Step::Word(word),
                    Err(reason) => Step::Stop(reason),
                },
                Err(reason) => Step::Stop(reason),
            }
        }
    } else {
        match primary_form(first, bits64) {
            Ok(form) => match finish(bytes, at, first, false, None, form, op_bits, addr_bits, rex_r, rex_b, prefixed.rex != 0, bits64) {
                Ok(word) => Step::Word(word),
                Err(reason) => Step::Stop(reason),
            },
            Err(reason) => Step::Stop(reason),
        }
    };
    parsed
}

fn finish(
    bytes: &[u8],
    mut at: usize,
    opcode: u8,
    secondary: bool,
    three: Option<u8>,
    form: Form,
    operand_bits: u8,
    address_bits: u8,
    rex_r: u8,
    rex_b: u8,
    rex: bool,
    bits64: bool,
) -> Result<Word, &'static str> {
    let mut modrm = None;
    if form.modrm {
        let byte = *bytes.get(at).ok_or("instruction continues past the available bytes")?;
        at += 1;
        let tail = modrm_tail(bytes, at, byte, address_bits).ok_or("instruction continues past the available bytes")?;
        at += tail;
        modrm = Some(byte);
    }
    let mut imm_kind = form.imm;
    if form.test_imm {
        let reg = (modrm.unwrap() >> 3) & 7;
        if reg == 0 {
            imm_kind = if opcode == 0xF6 { Imm::Ib } else { Imm::Iz };
        }
    }
    let imm_n = imm_len(imm_kind, operand_bits, address_bits);
    let imm = need(bytes, at, imm_n).ok_or("instruction continues past the available bytes")?;
    at += imm_n;
    let len = at;
    let (text, flow) = render(opcode, secondary, three, modrm, imm, operand_bits, rex_r, rex_b, rex, bits64, &bytes[..len]);
    Ok(Word { len, text, flow })
}

fn render(
    opcode: u8,
    secondary: bool,
    three: Option<u8>,
    modrm: Option<u8>,
    imm: &[u8],
    operand_bits: u8,
    rex_r: u8,
    rex_b: u8,
    rex: bool,
    bits64: bool,
    raw: &[u8],
) -> (String, Flow) {
    if secondary || three.is_some() {
        if (0x80..=0x8F).contains(&opcode) && imm.len() >= 4 && !secondary.eq(&false) && three.is_none() {
            let rel = if imm.len() == 2 { i16::from_le_bytes(imm[..2].try_into().unwrap()) as i64 } else { read_u32(imm) };
            return (format!("jcc {rel:+#x}"), Flow::Branch(Some(rel)));
        }
        return (format!("unclassified {}", hex_bytes(raw)), Flow::Next);
    }
    let byte_high = bits64 && !rex;
    let reg = |n: u8, bits: u8| reg_name(n, bits, byte_high);
    let g = |modrm: u8| reg(((modrm >> 3) & 7) | (rex_r << 3), operand_bits);
    let e_reg = |modrm: u8| reg((modrm & 7) | (rex_b << 3), operand_bits);
    let mem_or_reg = |modrm: u8| if modrm >> 6 == 3 { e_reg(modrm).to_string() } else { "mem".to_string() };
    let alu = ["add", "or", "adc", "sbb", "and", "sub", "xor", "cmp"];
    let rel8 = |imm: &[u8]| read_i8(imm[0]);
    let relz = |imm: &[u8]| if imm.len() == 2 { i16::from_le_bytes(imm[..2].try_into().unwrap()) as i64 } else { read_u32(imm) };
    match opcode {
        0x90 => ("nop".into(), Flow::Next),
        0xC3 => ("ret".into(), Flow::Return),
        0xCB => ("retf".into(), Flow::Return),
        0xC2 | 0xCA => (format!("ret {:#x}", u16::from_le_bytes(imm[..2].try_into().unwrap())), Flow::Return),
        0xCC => ("int3".into(), Flow::Next),
        0xE8 => {
            let rel = relz(imm);
            (format!("call {rel:+#x}"), Flow::Call(Some(rel)))
        }
        0xE9 => {
            let rel = relz(imm);
            (format!("jmp {rel:+#x}"), Flow::Branch(Some(rel)))
        }
        0xEB => {
            let rel = rel8(imm);
            (format!("jmp {rel:+#x}"), Flow::Branch(Some(rel)))
        }
        0x70..=0x7F => {
            let rel = rel8(imm);
            (format!("jcc {rel:+#x}"), Flow::Branch(Some(rel)))
        }
        0x50..=0x57 => (format!("push {}", reg((opcode & 7) | (rex_b << 3), if operand_bits == 16 { 16 } else { 64 })), Flow::Next),
        0x58..=0x5F => (format!("pop {}", reg((opcode & 7) | (rex_b << 3), if operand_bits == 16 { 16 } else { 64 })), Flow::Next),
        0xB8..=0xBF => {
            let value = match imm.len() {
                8 => u64::from_le_bytes(imm[..8].try_into().unwrap()),
                2 => u16::from_le_bytes(imm[..2].try_into().unwrap()) as u64,
                _ => u32::from_le_bytes(imm[..4].try_into().unwrap()) as u64,
            };
            (format!("mov {}, {value:#x}", reg((opcode & 7) | (rex_b << 3), operand_bits)), Flow::Next)
        }
        0x89 | 0x8B if modrm.is_some_and(|m| m >> 6 == 3) => {
            let m = modrm.unwrap();
            let (dst, src) = if opcode == 0x89 { (e_reg(m), g(m)) } else { (g(m), e_reg(m)) };
            (format!("mov {dst}, {src}"), Flow::Next)
        }
        0x83 | 0x81 | 0x80 if modrm.is_some() => {
            let m = modrm.unwrap();
            let name = alu[((m >> 3) & 7) as usize];
            let value = if imm.len() == 1 { read_i8(imm[0]) as u64 } else if imm.len() == 2 { u16::from_le_bytes(imm[..2].try_into().unwrap()) as u64 } else { u32::from_le_bytes(imm[..4].try_into().unwrap()) as u64 };
            (format!("{name} {}, {value:#x}", mem_or_reg(m)), Flow::Next)
        }
        0xFF if modrm.is_some() => {
            let m = modrm.unwrap();
            let name = ["inc", "dec", "call", "callf", "jmp", "jmpf", "push", "unclassified"][((m >> 3) & 7) as usize];
            let flow = match (m >> 3) & 7 {
                2 | 3 => Flow::Call(None),
                4 | 5 => Flow::Branch(None),
                _ => Flow::Next,
            };
            (format!("{name} {}", mem_or_reg(m)), flow)
        }
        0x8D if modrm.is_some() => (format!("lea {}, mem", g(modrm.unwrap())), Flow::Next),
        _ => (format!("unclassified {}", hex_bytes(raw)), Flow::Next),
    }
}

pub struct Listing {
    pub text: String,
    pub instructions: u64,
    pub stopped: Option<&'static str>,
}

/// List every instruction in `bytes`. `origin` is the address used for display.
/// `max_lines` of `None` keeps every line. Cancellation returns the lines already formed.
pub fn list(bytes: &[u8], origin: u64, bits64: bool, max_lines: Option<usize>, cancelled: &mut dyn FnMut() -> bool) -> Listing {
    let mut text = String::new();
    let mut at = 0usize;
    let mut instructions = 0u64;
    let mut block = 0u32;
    let mut open = false;
    let mut stopped = None;
    while at < bytes.len() {
        if instructions % 64 == 0 && cancelled() {
            stopped = Some("cancelled");
            break;
        }
        if max_lines.is_some_and(|limit| instructions as usize >= limit) {
            stopped = Some("summary bound reached; the listing operation writes the rest");
            break;
        }
        match decode(&bytes[at..], bits64) {
            Step::Stop(reason) => {
                stopped = Some(reason);
                break;
            }
            Step::Word(word) => {
                if !open {
                    text.push_str(&format!("block {block} at {origin:#x}+{at:#x}\n"));
                    open = true;
                }
                text.push_str(&format!("  {origin:#x}+{at:#x}  {}\n", word.text));
                instructions += 1;
                at += word.len;
                if !matches!(word.flow, Flow::Next) {
                    open = false;
                    block += 1;
                }
            }
        }
    }
    if at < bytes.len() {
        if let Some(reason) = stopped {
            text.push_str(&format!("listing stopped at {origin:#x}+{at:#x}: {reason}\n"));
        }
    }
    Listing { text, instructions, stopped }
}

pub struct Transcript {
    pub consumed: usize,
    pub instructions: u64,
    pub stopped: Option<&'static str>,
}

/// Write one forward listing. When `more` is set, a trailing partial instruction
/// is left unconsumed so the caller can append the next bytes. Any other stop
/// is written and those bytes stay unconsumed.
pub fn transcribe(bytes: &[u8], origin: u64, bits64: bool, more: bool, out: &mut dyn std::io::Write, cancelled: &mut dyn FnMut() -> bool) -> Result<Transcript, String> {
    let mut at = 0usize;
    let mut instructions = 0u64;
    let mut block = 0u64;
    let mut open = false;
    let mut stopped = None;
    while at < bytes.len() {
        if instructions % 64 == 0 && cancelled() {
            stopped = Some("cancelled");
            break;
        }
        match decode(&bytes[at..], bits64) {
            Step::Stop(reason) if more && reason == "instruction continues past the available bytes" => break,
            Step::Stop(reason) => {
                stopped = Some(reason);
                break;
            }
            Step::Word(word) => {
                if !open {
                    writeln!(out, "block {block} at {origin:#x}+{at:#x}").map_err(|e| e.to_string())?;
                    open = true;
                }
                writeln!(out, "  {origin:#x}+{at:#x}  {}", word.text).map_err(|e| e.to_string())?;
                instructions += 1;
                at += word.len;
                if !matches!(word.flow, Flow::Next) {
                    open = false;
                    block += 1;
                }
            }
        }
    }
    if let Some(reason) = stopped {
        writeln!(out, "listing stopped at {origin:#x}+{at:#x}: {reason}").map_err(|e| e.to_string())?;
    }
    Ok(Transcript { consumed: at, instructions, stopped })
}

pub fn summarize_regions(bytes: &[u8], spans: &[Span], bits64: bool) -> String {
    let mut out = String::from(if bits64 { "x86-64 instruction summary\n" } else { "x86 instruction summary\n" });
    let mut any = false;
    let mut cancel = || false;
    for span in spans {
        if !span.executable || span.size == 0 {
            continue;
        }
        let start = span.offset as usize;
        if start >= bytes.len() {
            out.push_str(&format!("executable mapping at file {:#x} is outside the examined bytes\n", span.offset));
            continue;
        }
        any = true;
        let end = start.saturating_add(span.size as usize).min(bytes.len());
        out.push_str(&format!("executable file [{:#x}, {:#x}) virtual {:#x}\n", span.offset, span.offset + (end - start) as u64, span.address));
        let listed = list(&bytes[start..end], span.address, bits64, Some(SUMMARY_LINES), &mut cancel);
        out.push_str(&listed.text);
    }
    if !any {
        out.push_str("No declared executable mapping was available to list.\n");
    }
    out.push_str(TRAILER);
    out
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn integer_words_match_their_lengths_and_names() {
        let nop = match decode(&[0x90], true) {
            Step::Word(word) => word,
            Step::Stop(reason) => panic!("{reason}"),
        };
        assert_eq!((nop.len, nop.text.as_str()), (1, "nop"));
        let ret = match decode(&[0xC3], true) {
            Step::Word(word) => word,
            Step::Stop(reason) => panic!("{reason}"),
        };
        assert_eq!(ret.text, "ret");
        assert!(matches!(ret.flow, Flow::Return));
        let call = match decode(&[0xE8, 0x00, 0x00, 0x00, 0x00], true) {
            Step::Word(word) => word,
            Step::Stop(reason) => panic!("{reason}"),
        };
        assert_eq!((call.len, call.text.as_str()), (5, "call +0x0"));
        let mov = match decode(&[0x48, 0x89, 0xE5], true) {
            Step::Word(word) => word,
            Step::Stop(reason) => panic!("{reason}"),
        };
        assert_eq!((mov.len, mov.text.as_str()), (3, "mov rbp, rsp"));
        let sub = match decode(&[0x48, 0x83, 0xEC, 0x20], true) {
            Step::Word(word) => word,
            Step::Stop(reason) => panic!("{reason}"),
        };
        assert_eq!((sub.len, sub.text.as_str()), (4, "sub rsp, 0x20"));
        let push = match decode(&[0x55], true) {
            Step::Word(word) => word,
            Step::Stop(reason) => panic!("{reason}"),
        };
        assert_eq!(push.text, "push rbp");
        let wide = match decode(&[0x48, 0xB8, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00], true) {
            Step::Word(word) => word,
            Step::Stop(reason) => panic!("{reason}"),
        };
        assert_eq!(wide.len, 10);
        assert_eq!(wide.text, "mov rax, 0x12345678");
    }

    #[test]
    fn vector_prefix_does_not_consume_a_guessed_length() {
        assert!(matches!(decode(&[0xC5, 0xF8, 0x10, 0x00], true), Step::Stop(_)));
    }

    #[test]
    fn summary_trailer_is_present_once() {
        let text = summarize_regions(
            &[0x90, 0xC3],
            &[Span { offset: 0, address: 0x1000, size: 2, executable: true }],
            true,
        );
        assert_eq!(text.matches(TRAILER).count(), 1);
        assert!(text.contains("nop"));
        assert!(text.ends_with(TRAILER));
    }

    #[test]
    fn a_partial_instruction_is_left_for_the_next_bytes() {
        let mut out = Vec::new();
        let partial = transcribe(&[0x90, 0xE8], 0x1000, true, true, &mut out, &mut || false).unwrap();
        assert_eq!(partial.consumed, 1);
        assert!(partial.stopped.is_none());
        assert!(String::from_utf8(out).unwrap().contains("nop"));
        let mut out = Vec::new();
        let stopped = transcribe(&[0x90, 0xE8], 0x1000, true, false, &mut out, &mut || false).unwrap();
        assert_eq!(stopped.consumed, 1);
        assert!(stopped.stopped.is_some());
    }
}

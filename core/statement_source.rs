//! Straight-line integer block transcribed into one Rust function.
//! The input is the same local byte slice the register trace reads:
//! `0x90` nop, `0xC3` return, `0xB8`–`0xBF` mov imm32, and `0x48` as a
//! width prefix when the wide flag is set. At most 16 instructions are
//! compiled. This is not a decompiler for arbitrary PE, ELF, or DEX images.
//! The subject process was not started.

const REGISTERS: usize = 8;
const INSTRUCTION_LIMIT: usize = 16;

enum Control {
    Apply { next: usize, op: Op },
    Return,
    Stop,
}

enum Op {
    Nop,
    Move { rd: usize, value: u64 },
}

struct Decoded {
    ops: Vec<Op>,
    returned: bool,
    unread: bool,
}

/// Transcribe at most 16 instructions into `fn block() -> [u64; 8]`.
///
/// A 17th instruction is the comment `later instructions were not compiled`
/// and is absent from the compiled body. The subject process was not started.
pub fn statement_source(bytes: &[u8], bits64: bool) -> String {
    let decoded = decode_block(bytes, bits64);
    let mut out = String::from("fn block() -> [u64; 8] {\n");
    let writes = decoded.ops.iter().any(|op| matches!(op, Op::Move { .. }));
    if writes {
        out.push_str("    let mut registers = [0u64; 8];\n");
    } else {
        out.push_str("    let registers = [0u64; 8];\n");
    }
    for op in &decoded.ops {
        match op {
            Op::Nop => out.push_str("    // nop\n"),
            Op::Move { rd, value } => {
                out.push_str(&format!("    registers[{rd}] = {value}u64;\n"));
            }
        }
    }
    if decoded.unread {
        out.push_str("    // later instructions were not compiled\n");
    }
    out.push_str("    // The subject process was not started.\n");
    if decoded.returned {
        out.push_str("    return registers;\n");
    } else {
        out.push_str("    registers\n");
    }
    out.push('}');
    out.push('\n');
    out
}

fn decode_block(bytes: &[u8], bits64: bool) -> Decoded {
    let mut ops = Vec::new();
    let mut at = 0usize;
    let mut returned = false;
    while at < bytes.len() && ops.len() < INSTRUCTION_LIMIT {
        match step(bytes, at, bits64) {
            Control::Apply { next, op } => {
                ops.push(op);
                at = next;
            }
            Control::Return => {
                returned = true;
                break;
            }
            Control::Stop => break,
        }
    }
    let unread = ops.len() == INSTRUCTION_LIMIT && at < bytes.len();
    Decoded {
        ops,
        returned,
        unread,
    }
}

fn step(bytes: &[u8], at: usize, bits64: bool) -> Control {
    let Some(opcode) = bytes.get(at).copied() else {
        return Control::Stop;
    };
    match opcode {
        0x90 => match at.checked_add(1) {
            Some(next) => Control::Apply {
                next,
                op: Op::Nop,
            },
            None => Control::Stop,
        },
        0xC3 => Control::Return,
        0x48 if bits64 => match at.checked_add(1) {
            Some(opcode_at) => move_immediate(bytes, opcode_at),
            None => Control::Stop,
        },
        _ => move_immediate(bytes, at),
    }
}

fn move_immediate(bytes: &[u8], at: usize) -> Control {
    let Some(opcode) = bytes.get(at).copied() else {
        return Control::Stop;
    };
    if !(0xB8..=0xBF).contains(&opcode) {
        return Control::Stop;
    }
    let Some(end) = at.checked_add(5) else {
        return Control::Stop;
    };
    let Some(raw) = bytes.get(at + 1..end) else {
        return Control::Stop;
    };
    let mut immediate = [0u8; 4];
    immediate.copy_from_slice(raw);
    let rd = usize::from(opcode - 0xB8);
    if rd >= REGISTERS {
        return Control::Stop;
    }
    Control::Apply {
        next: end,
        op: Op::Move {
            rd,
            value: u64::from(u32::from_le_bytes(immediate)),
        },
    }
}

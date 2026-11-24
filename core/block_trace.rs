//! Straight-line evaluation of a local register file.
//! At most 16 instructions are applied. A further instruction is named.
//! Bytes are applied in order. No process, thread, or device is used.

const REGISTERS: usize = 16;
const INSTRUCTION_LIMIT: usize = 16;

const NAMES32: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
const NAMES64: [&str; 8] = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];

enum Step {
    Next(usize),
    Halt,
}

/// Evaluate at most 16 instructions from `bytes` into sixteen zeroed registers.
pub fn block_trace(bytes: &[u8], bits64: bool) -> String {
    let mut registers = [0u64; REGISTERS];
    let mut at = 0usize;
    let mut executed = 0usize;
    while at < bytes.len() && executed < INSTRUCTION_LIMIT {
        match apply(&mut registers, bytes, at, bits64) {
            Step::Next(next) => {
                at = next;
                executed += 1;
            }
            Step::Halt => break,
        }
    }
    let unread = executed == INSTRUCTION_LIMIT && at < bytes.len();
    report(&registers, bits64, unread)
}

fn apply(registers: &mut [u64; REGISTERS], bytes: &[u8], at: usize, bits64: bool) -> Step {
    let Some(opcode) = bytes.get(at).copied() else {
        return Step::Halt;
    };
    match opcode {
        0x90 => at.checked_add(1).map(Step::Next).unwrap_or(Step::Halt),
        0xC3 => Step::Halt,
        0x48 if bits64 => match at.checked_add(1) {
            Some(opcode_at) => move_immediate(registers, bytes, opcode_at),
            None => Step::Halt,
        },
        _ => move_immediate(registers, bytes, at),
    }
}

fn move_immediate(registers: &mut [u64; REGISTERS], bytes: &[u8], at: usize) -> Step {
    let Some(opcode) = bytes.get(at).copied() else {
        return Step::Halt;
    };
    if !(0xB8..=0xBF).contains(&opcode) {
        return Step::Halt;
    }
    let Some(end) = at.checked_add(5) else {
        return Step::Halt;
    };
    let Some(raw) = bytes.get(at + 1..end) else {
        return Step::Halt;
    };
    let mut immediate = [0u8; 4];
    immediate.copy_from_slice(raw);
    let rd = usize::from(opcode - 0xB8);
    registers[rd] = u64::from(u32::from_le_bytes(immediate));
    Step::Next(end)
}

fn report(registers: &[u64; REGISTERS], bits64: bool, unread: bool) -> String {
    let names = if bits64 { NAMES64 } else { NAMES32 };
    let mut out = String::new();
    for rd in 0..names.len() {
        let value = registers[rd];
        if value == 0 {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(names[rd]);
        out.push(' ');
        out.push_str(&value.to_string());
    }
    if !out.is_empty() {
        out.push('\n');
    }
    if unread {
        out.push_str("later instructions were not evaluated\n");
    }
    out.push_str("The subject process was not started.");
    out
}

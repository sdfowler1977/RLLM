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

#[cfg(test)]
mod tests {
    use super::block_trace;

    #[test]
    fn mov_eax() {
        let text = block_trace(&[0xB8, 0x01, 0x00, 0x00, 0x00, 0xC3], false);
        assert!(text.contains("eax 1"), "{text}");
        assert!(text.contains("not started"), "{text}");
        assert!(text.ends_with("The subject process was not started."));
    }

    #[test]
    fn width_prefix_zero_extends_and_uses_rax_names() {
        let text = block_trace(&[0x48, 0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0xC3], true);
        assert_eq!(
            text,
            "rax 4294967295\nThe subject process was not started."
        );
    }

    #[test]
    fn plain_move_in_wide_mode_uses_wide_names() {
        let text = block_trace(&[0xB9, 0x02, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00], true);
        assert_eq!(
            text,
            "rax 1\nrcx 2\nThe subject process was not started."
        );
    }

    #[test]
    fn width_prefix_without_wide_mode_stops() {
        let text = block_trace(&[0x48, 0xB8, 0x01, 0x00, 0x00, 0x00], false);
        assert_eq!(text, "The subject process was not started.");
    }

    #[test]
    fn return_stops_before_a_later_move() {
        let text = block_trace(
            &[0xB8, 0x01, 0x00, 0x00, 0x00, 0xC3, 0xB9, 0x02, 0x00, 0x00, 0x00],
            false,
        );
        assert_eq!(text, "eax 1\nThe subject process was not started.");
    }

    #[test]
    fn unknown_keeps_earlier_registers() {
        let text = block_trace(
            &[0x90, 0xB8, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xB9, 0x02, 0x00, 0x00, 0x00],
            false,
        );
        assert_eq!(text, "eax 1\nThe subject process was not started.");
    }

    #[test]
    fn zero_result_is_omitted() {
        let text = block_trace(
            &[0xB8, 0x01, 0x00, 0x00, 0x00, 0xB8, 0x00, 0x00, 0x00, 0x00],
            false,
        );
        assert_eq!(text, "The subject process was not started.");
    }

    #[test]
    fn instruction_limit_is_sixteen() {
        let mut bytes = Vec::new();
        for _ in 0..16 {
            bytes.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]);
        }
        bytes.extend_from_slice(&[0xB9, 0x02, 0x00, 0x00, 0x00]);
        let text = block_trace(&bytes, false);
        assert_eq!(
            text,
            "eax 1\nlater instructions were not evaluated\nThe subject process was not started."
        );

        let mut within = vec![0x90u8; 15];
        within.extend_from_slice(&[0xB8, 0x03, 0x00, 0x00, 0x00]);
        assert_eq!(
            block_trace(&within, false),
            "eax 3\nThe subject process was not started."
        );
    }
}

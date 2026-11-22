//! Straight-line lift of a short integer block from a byte slice.
//! At most 32 statements are listed. A further byte is named.
//! Immediate and relative values are little-endian.

const STATEMENT_LIMIT: usize = 32;

const REG32: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
const REG64: [&str; 8] = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];

pub fn lifted_block(bytes: &[u8], bits64: bool) -> String {
    let mut text = String::new();
    let mut ip = 0usize;
    let mut count = 0usize;
    let reason = loop {
        if ip >= bytes.len() {
            break "end of the slice";
        }
        if count >= STATEMENT_LIMIT {
            break "later statements were not listed";
        }
        match statement_at(bytes, ip, bits64) {
            Ok((next, line)) => {
                count += 1;
                text.push_str(&format!("statement {count}: {line}\n"));
                ip = next;
            }
            Err(reason) => break reason,
        }
    };
    text.push_str(&format!("lift stopped: {reason}\n"));
    text
}

fn register_name(rd: u8, bits64: bool) -> &'static str {
    if bits64 {
        REG64[rd as usize]
    } else {
        REG32[rd as usize]
    }
}

fn statement_at(bytes: &[u8], ip: usize, bits64: bool) -> Result<(usize, String), &'static str> {
    let first = bytes[ip];
    if is_vector_prefix(first) {
        return Err("vector prefix");
    }
    let mut opcode_at = ip;
    if bits64 && first == 0x48 {
        opcode_at = ip + 1;
        if opcode_at >= bytes.len() {
            return Err("undecoded sequence");
        }
        let opcode = bytes[opcode_at];
        if is_vector_prefix(opcode) {
            return Err("vector prefix");
        }
        if is_rex(opcode) {
            return Err("other REX");
        }
    } else if bits64 && is_rex(first) {
        return Err("other REX");
    }

    let opcode = bytes[opcode_at];
    let name = register_name(opcode & 7, bits64);
    match opcode {
        0x90 => Ok((opcode_at + 1, "nop".to_string())),
        0xC3 => Ok((opcode_at + 1, "return".to_string())),
        0x50..=0x57 => Ok((opcode_at + 1, format!("push {name}"))),
        0x58..=0x5F => Ok((opcode_at + 1, format!("pop {name}"))),
        0xB8..=0xBF => {
            let imm = read_u32(bytes, opcode_at + 1).ok_or("undecoded sequence")?;
            Ok((opcode_at + 5, format!("move {name}, {imm:#x}")))
        }
        0xE8 | 0xE9 => {
            let disp = read_i32(bytes, opcode_at + 1).ok_or("undecoded sequence")?;
            let next_ip = (opcode_at + 5) as i64;
            let target = next_ip + i64::from(disp);
            let kind = if opcode == 0xE8 { "call" } else { "jump" };
            Ok((opcode_at + 5, format!("{kind} relative {}", hex_target(target))))
        }
        _ => Err("unknown opcode"),
    }
}

fn is_vector_prefix(byte: u8) -> bool {
    matches!(byte, 0xC4 | 0xC5 | 0x62)
}

fn is_rex(byte: u8) -> bool {
    (0x40..=0x4F).contains(&byte)
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let raw = bytes.get(at..end)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_i32(bytes: &[u8], at: usize) -> Option<i32> {
    read_u32(bytes, at).map(|bits| bits as i32)
}

fn hex_target(target: i64) -> String {
    if target < 0 {
        format!("-{:#x}", target.unsigned_abs())
    } else {
        format!("{target:#x}")
    }
}

#[cfg(test)]
mod tests {
    use super::lifted_block;

    #[test]
    fn nop_ret() {
        let text = lifted_block(&[0x90, 0xC3], false);
        assert!(text.contains("nop"), "{text}");
        assert!(text.contains("return"), "{text}");
        assert!(text.contains("lift stopped"), "{text}");
    }

    #[test]
    fn thirty_three_nops_name_the_rest() {
        let text = lifted_block(&[0x90; 33], false);
        assert!(text.contains("statement 32: nop\n"), "{text}");
        assert!(!text.contains("statement 33"), "{text}");
        assert!(text.ends_with("lift stopped: later statements were not listed\n"), "{text}");

        let exact = lifted_block(&[0x90; 32], false);
        assert!(exact.contains("statement 32: nop\n"), "{exact}");
        assert!(exact.ends_with("lift stopped: end of the slice\n"), "{exact}");
    }
}

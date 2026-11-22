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

#[cfg(test)]
mod tests {
    use super::{statement_source, REGISTERS};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    const MOV_EAX_ONE_RET: &str = "\
fn block() -> [u64; 8] {
    let mut registers = [0u64; 8];
    registers[0] = 1u64;
    // The subject process was not started.
    return registers;
}
";

    fn build_dir() -> std::path::PathBuf {
        let cwd = std::env::current_dir().expect("current directory");
        if cwd.join("core").join("statement_source.rs").is_file() {
            cwd.join("build")
        } else if cwd.join("statement_source.rs").is_file() {
            cwd.join("..").join("build")
        } else {
            cwd.join("build")
        }
    }

    fn compile_block(source: &str) -> [u64; 8] {
        assert!(
            source.contains("fn block() -> [u64; 8]"),
            "{source}"
        );
        assert!(
            source.contains("The subject process was not started."),
            "{source}"
        );
        let dir = build_dir();
        std::fs::create_dir_all(&dir).expect("build directory");
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let name = format!("statement_block_{}_{}_{}", std::process::id(), sequence, stamp);
        let source_path = dir.join(format!("{name}.rs"));
        let exe_name = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name
        };
        let exe_path = dir.join(exe_name);
        let mut program = String::new();
        program.push_str(source);
        if !source.ends_with('\n') {
            program.push('\n');
        }
        program.push_str("fn main() {\n");
        program.push_str("    let values = block();\n");
        program.push_str("    for value in values {\n");
        program.push_str("        println!(\"{value}\");\n");
        program.push_str("    }\n");
        program.push_str("}\n");
        std::fs::write(&source_path, program).expect("write generated source");
        let compiled = std::process::Command::new("rustc")
            .arg("--edition")
            .arg("2021")
            .arg("-C")
            .arg("debuginfo=0")
            .arg("-o")
            .arg(&exe_path)
            .arg(&source_path)
            .output()
            .expect("rustc");
        assert!(
            compiled.status.success(),
            "rustc failed for {}:\n{}\n{}",
            source_path.display(),
            String::from_utf8_lossy(&compiled.stderr),
            source
        );
        let ran = std::process::Command::new(&exe_path)
            .output()
            .expect("run generated function");
        assert!(
            ran.status.success(),
            "generated function failed:\n{}{}",
            String::from_utf8_lossy(&ran.stderr),
            source
        );
        let text = String::from_utf8(ran.stdout).expect("utf-8 registers");
        let mut values = [0u64; REGISTERS];
        let mut seen = 0usize;
        for line in text.lines() {
            assert!(seen < REGISTERS, "extra register line {line}");
            values[seen] = line
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("register value {line}"));
            seen += 1;
        }
        assert_eq!(seen, REGISTERS, "register lines:\n{text}\n{source}");
        values
    }

    fn assert_block(bytes: &[u8], bits64: bool, expected: [u64; 8]) {
        let source = statement_source(bytes, bits64);
        let actual = compile_block(&source);
        assert_eq!(actual, expected, "{source}");
    }

    #[test]
    fn mov_eax_one_then_return() {
        let bytes = [0xB8, 0x01, 0x00, 0x00, 0x00, 0xC3];
        let source = statement_source(&bytes, false);
        assert_eq!(source, MOV_EAX_ONE_RET);
        assert!(!source.contains("later instructions were not compiled"));
        assert_eq!(compile_block(&source), [1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn width_prefix_zero_extends_rax() {
        assert_block(
            &[0x48, 0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0xC3],
            true,
            [4294967295, 0, 0, 0, 0, 0, 0, 0],
        );
    }

    #[test]
    fn plain_moves_in_wide_mode() {
        assert_block(
            &[0xB9, 0x02, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00],
            true,
            [1, 2, 0, 0, 0, 0, 0, 0],
        );
    }

    #[test]
    fn width_prefix_without_wide_mode_stops() {
        let source = statement_source(&[0x48, 0xB8, 0x01, 0x00, 0x00, 0x00], false);
        assert!(!source.contains("registers["));
        assert_eq!(compile_block(&source), [0; 8]);
    }

    #[test]
    fn width_prefix_before_nop_stops() {
        let source = statement_source(&[0x48, 0x90, 0xB8, 0x01, 0x00, 0x00, 0x00], true);
        assert!(!source.contains("registers["));
        assert!(!source.contains("// nop"));
        assert_eq!(compile_block(&source), [0; 8]);
    }

    #[test]
    fn return_stops_before_a_later_move() {
        let source = statement_source(
            &[0xB8, 0x01, 0x00, 0x00, 0x00, 0xC3, 0xB9, 0x02, 0x00, 0x00, 0x00],
            false,
        );
        assert!(source.contains("return registers;"));
        assert!(!source.contains("registers[1]"));
        assert!(!source.contains("later instructions were not compiled"));
        assert_eq!(compile_block(&source), [1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn unknown_keeps_earlier_registers() {
        let source = statement_source(
            &[0x90, 0xB8, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xB9, 0x02, 0x00, 0x00, 0x00],
            false,
        );
        assert!(source.contains("// nop"));
        assert!(!source.contains("registers[1]"));
        assert_eq!(compile_block(&source), [1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn zero_immediate_clears_the_register() {
        assert_block(
            &[0xB8, 0x01, 0x00, 0x00, 0x00, 0xB8, 0x00, 0x00, 0x00, 0x00],
            false,
            [0; 8],
        );
    }

    #[test]
    fn instruction_limit_leaves_the_seventeenth_uncompiled() {
        let mut bytes = Vec::new();
        for _ in 0..16 {
            bytes.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]);
        }
        bytes.extend_from_slice(&[0xB9, 0x02, 0x00, 0x00, 0x00]);
        let source = statement_source(&bytes, false);
        assert_eq!(source.matches("registers[0] = 1u64;").count(), 16);
        assert!(!source.contains("registers[1]"));
        assert!(source
            .lines()
            .any(|line| line.trim() == "// later instructions were not compiled"));
        let comment = source
            .find("// later instructions were not compiled")
            .expect("limit comment");
        assert!(
            !source[comment..].contains("registers["),
            "{source}"
        );
        assert_eq!(compile_block(&source), [1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn fifteen_nops_then_a_move_still_compiles() {
        let mut bytes = vec![0x90u8; 15];
        bytes.extend_from_slice(&[0xB8, 0x03, 0x00, 0x00, 0x00]);
        let source = statement_source(&bytes, false);
        assert_eq!(source.matches("// nop").count(), 15);
        assert!(source.contains("registers[0] = 3u64;"));
        assert!(!source.contains("later instructions were not compiled"));
        assert_eq!(compile_block(&source), [3, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn each_register_receives_its_immediate() {
        let mut bytes = Vec::new();
        for rd in 0..8u8 {
            bytes.push(0xB8 + rd);
            bytes.push(rd + 1);
            bytes.extend_from_slice(&[0, 0, 0]);
        }
        bytes.push(0xC3);
        let expected = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_block(&bytes, false, expected);
        assert_block(&bytes, true, expected);
    }

    #[test]
    fn little_endian_immediate_zero_extends() {
        assert_block(
            &[0xB8, 0x04, 0x03, 0x02, 0x01, 0xC3],
            false,
            [0x0102_0304, 0, 0, 0, 0, 0, 0, 0],
        );
        assert_block(
            &[0x48, 0xB8, 0x04, 0x03, 0x02, 0x01, 0xC3],
            true,
            [0x0102_0304, 0, 0, 0, 0, 0, 0, 0],
        );
    }

    #[test]
    fn empty_input_compiles_to_zeros() {
        let source = statement_source(&[], false);
        assert!(!source.contains("later instructions were not compiled"));
        assert!(!source.contains("return registers;"));
        assert_eq!(compile_block(&source), [0; 8]);
    }

    #[test]
    fn seventeenth_return_is_not_compiled() {
        let mut bytes = vec![0x90u8; 16];
        bytes.push(0xC3);
        let source = statement_source(&bytes, false);
        assert_eq!(source.matches("// nop").count(), 16);
        assert!(source
            .lines()
            .any(|line| line.trim() == "// later instructions were not compiled"));
        assert!(!source.contains("return registers;"));
        assert_eq!(compile_block(&source), [0; 8]);
    }

    #[test]
    fn truncated_move_compiles_to_zeros() {
        let source = statement_source(&[0xB8, 0x01, 0x00, 0x00], false);
        assert!(!source.contains("registers["));
        assert!(!source.contains("later instructions were not compiled"));
        assert_eq!(compile_block(&source), [0; 8]);
    }
}

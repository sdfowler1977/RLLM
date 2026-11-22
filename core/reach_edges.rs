//! Direct x86 call and jump edges read from a byte slice.
//! At most 32 edges are listed. A further edge is named.
//! An indirect call stays unresolved. No process is attached.

#![forbid(unsafe_code)]

const EDGE_LIMIT: usize = 32;

enum Note {
    Edge { len: usize, kind: &'static str, target: i64 },
    Unresolved { len: usize },
    Skip { len: usize },
    Truncated,
    Byte,
}

/// Record direct `E8`, `E9`, `EB`, and `0x70..=0x7F` edges.
/// Offsets are positions in `bytes`. The edges are not called.
pub fn reach_edges(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut at = 0usize;
    let mut listed = 0usize;
    let mut capped = false;
    while at < bytes.len() {
        match note_at(bytes, at) {
            Note::Edge { len, kind, target } => {
                if listed >= EDGE_LIMIT {
                    capped = true;
                    break;
                }
                push_line(&mut out, &format!("{kind} at {at:#x} targets {}", hex_offset(target)));
                listed += 1;
                match at.checked_add(len) {
                    Some(next) => at = next,
                    None => break,
                }
            }
            Note::Unresolved { len } => {
                push_line(&mut out, &format!("indirect call at {at:#x} the target was not resolved"));
                match at.checked_add(len) {
                    Some(next) => at = next,
                    None => break,
                }
            }
            Note::Skip { len } => match at.checked_add(len) {
                Some(next) => at = next,
                None => break,
            },
            Note::Truncated => break,
            Note::Byte => at += 1,
        }
    }
    if capped {
        push_line(&mut out, "later edges were not listed");
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("The subject process was not attached.");
    out
}

fn push_line(out: &mut String, line: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(line);
}

fn hex_offset(target: i64) -> String {
    format!("{:#x}", target as u64)
}

fn note_at(bytes: &[u8], at: usize) -> Note {
    let Some(opcode) = bytes.get(at).copied() else {
        return Note::Truncated;
    };
    match opcode {
        0xE8 | 0xE9 => near_edge(bytes, at, opcode),
        0xEB | 0x70..=0x7F => short_edge(bytes, at, opcode),
        0xFF => indirect_or_skip(bytes, at),
        _ => Note::Byte,
    }
}

fn near_edge(bytes: &[u8], at: usize, opcode: u8) -> Note {
    let Some(raw) = bytes.get(at.saturating_add(1)..).and_then(|rest| rest.get(..4)) else {
        return Note::Truncated;
    };
    let mut immediate = [0u8; 4];
    immediate.copy_from_slice(raw);
    let displacement = i64::from(i32::from_le_bytes(immediate));
    let Some(target) = target_at(at, 5, displacement) else {
        return Note::Unresolved { len: 5 };
    };
    let kind = if opcode == 0xE8 { "call" } else { "jump" };
    Note::Edge { len: 5, kind, target }
}

fn short_edge(bytes: &[u8], at: usize, opcode: u8) -> Note {
    let Some(raw) = bytes.get(at.saturating_add(1)).copied() else {
        return Note::Truncated;
    };
    let displacement = i64::from(raw as i8);
    let Some(target) = target_at(at, 2, displacement) else {
        return Note::Unresolved { len: 2 };
    };
    Note::Edge { len: 2, kind: short_name(opcode), target }
}

fn short_name(opcode: u8) -> &'static str {
    match opcode {
        0x70 => "jo",
        0x71 => "jno",
        0x72 => "jb",
        0x73 => "jae",
        0x74 => "je",
        0x75 => "jne",
        0x76 => "jbe",
        0x77 => "ja",
        0x78 => "js",
        0x79 => "jns",
        0x7A => "jp",
        0x7B => "jnp",
        0x7C => "jl",
        0x7D => "jge",
        0x7E => "jle",
        0x7F => "jg",
        _ => "short jump",
    }
}

fn indirect_or_skip(bytes: &[u8], at: usize) -> Note {
    let Some(modrm) = bytes.get(at.saturating_add(1)).copied() else {
        return Note::Truncated;
    };
    let Some(tail) = modrm_tail(bytes, at.saturating_add(2), modrm) else {
        return Note::Truncated;
    };
    let Some(len) = 2usize.checked_add(tail) else {
        return Note::Truncated;
    };
    match (modrm >> 3) & 7 {
        2 | 3 => Note::Unresolved { len },
        _ => Note::Skip { len },
    }
}

/// Displacement bytes after ModRM for a 32-bit or 64-bit address size.
fn modrm_tail(bytes: &[u8], at: usize, modrm: u8) -> Option<usize> {
    let mode = modrm >> 6;
    let rm = modrm & 7;
    if mode == 3 {
        return Some(0);
    }
    let mut extra = 0usize;
    let base = if rm == 4 {
        extra = 1;
        bytes.get(at).copied()? & 7
    } else {
        rm
    };
    let displacement = match mode {
        0 if base == 5 => 4,
        1 => 1,
        2 => 4,
        _ => 0,
    };
    extra = extra.checked_add(displacement)?;
    let end = at.checked_add(extra)?;
    if end > bytes.len() {
        return None;
    }
    Some(extra)
}

fn target_at(at: usize, len: usize, displacement: i64) -> Option<i64> {
    let next = i64::try_from(at.checked_add(len)?).ok()?;
    next.checked_add(displacement)
}

#[cfg(test)]
mod tests {
    use super::reach_edges;

    #[test]
    fn direct_call_records_concrete_target() {
        assert_eq!(
            reach_edges(&[0xE8, 0x10, 0x00, 0x00, 0x00]),
            "call at 0x0 targets 0x15\nThe subject process was not attached."
        );
        assert_eq!(
            reach_edges(&[0xE8, 0xF0, 0xFF, 0xFF, 0xFF]),
            "call at 0x0 targets 0xfffffffffffffff5\nThe subject process was not attached."
        );
    }

    #[test]
    fn indirect_call_stays_unresolved() {
        let report = reach_edges(&[0xFF, 0xD0]);
        assert_eq!(
            report,
            "indirect call at 0x0 the target was not resolved\nThe subject process was not attached."
        );
        assert!(!report.contains("targets"));
        assert_eq!(
            reach_edges(&[0xFF, 0x15, 0x00, 0x00, 0x00, 0x00]),
            "indirect call at 0x0 the target was not resolved\nThe subject process was not attached."
        );
    }

    #[test]
    fn thirty_three_edges_name_the_cap() {
        let mut bytes = Vec::new();
        for _ in 0..33 {
            bytes.extend_from_slice(&[0xE8, 0x00, 0x00, 0x00, 0x00]);
        }
        let report = reach_edges(&bytes);
        assert_eq!(report.matches("call at ").count(), 32, "{report}");
        assert!(report.contains("call at 0x0 targets 0x5\n"), "{report}");
        assert!(report.contains("call at 0x9b targets 0xa0\n"), "{report}");
        assert!(!report.contains("call at 0xa0"), "{report}");
        assert!(report.contains("later edges were not listed\nThe subject process was not attached."), "{report}");
        assert!(report.ends_with("The subject process was not attached."));

        let mut exact = Vec::new();
        for _ in 0..32 {
            exact.extend_from_slice(&[0xE8, 0x00, 0x00, 0x00, 0x00]);
        }
        let exact_report = reach_edges(&exact);
        assert_eq!(exact_report.matches("call at ").count(), 32, "{exact_report}");
        assert!(!exact_report.contains("later edges were not listed"), "{exact_report}");
        assert!(exact_report.ends_with("The subject process was not attached."));
    }

    #[test]
    fn direct_jumps_record_targets() {
        assert_eq!(
            reach_edges(&[0xE9, 0x10, 0x00, 0x00, 0x00]),
            "jump at 0x0 targets 0x15\nThe subject process was not attached."
        );
        assert_eq!(
            reach_edges(&[0xEB, 0x05]),
            "short jump at 0x0 targets 0x7\nThe subject process was not attached."
        );
        assert_eq!(
            reach_edges(&[0x74, 0x05]),
            "je at 0x0 targets 0x7\nThe subject process was not attached."
        );
        assert_eq!(
            reach_edges(&[0x70, 0x00, 0x7F, 0xFE]),
            "jo at 0x0 targets 0x2\njg at 0x2 targets 0x2\nThe subject process was not attached."
        );
    }
}

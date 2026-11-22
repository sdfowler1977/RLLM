//! Printable ASCII and UTF-16LE runs in an untyped byte slice.
//! A run stops at the first byte or pair that does not match, or at the end of the slice.

const RUN_LIMIT: usize = 64;
const EXTRA_LIMIT: usize = 32;
const TEXT_CAP: usize = 64;

struct Run {
    offset: usize,
    encoding: &'static str,
    length: usize,
    end: usize,
    text: String,
}

/// Catalog printable runs.
///
/// The first 64 runs are kept. Up to 32 later runs are also kept when the
/// text contains a permission, audio, microphone, or recording marker.
/// `minimum` is the shortest run to keep. Zero is treated as one.
/// ASCII length counts bytes. UTF-16LE length counts code units.
/// A run that shares any byte with one already kept is omitted.
pub fn text_catalog(bytes: &[u8], minimum: usize) -> String {
    let minimum = minimum.max(1);
    let mut kept: Vec<Run> = Vec::new();
    let mut skipped = false;
    let mut index = 0;
    while index < bytes.len() {
        if ordinary_count(&kept) >= RUN_LIMIT && extra_count(&kept) >= EXTRA_LIMIT {
            if qualifying_after(bytes, index, minimum, &kept) {
                skipped = true;
            }
            break;
        }
        if kept.iter().any(|run| run.offset <= index && index < run.end) {
            index += 1;
            continue;
        }
        let units = utf16_units(bytes, index);
        let ascii = ascii_len(bytes, index);
        if units >= minimum {
            let run = utf16_run(bytes, index, units);
            let end = run.end;
            if !wanted(&kept, &run) {
                skipped = true;
                index += 1;
                continue;
            }
            if keep(&mut kept, run) {
                index = end;
                continue;
            }
            index += 1;
            continue;
        }
        if ascii >= minimum {
            let run = ascii_run(bytes, index, ascii);
            let end = run.end;
            if !wanted(&kept, &run) {
                skipped = true;
                index += 1;
                continue;
            }
            if keep(&mut kept, run) {
                index = end;
                continue;
            }
            index += 1;
            continue;
        }
        if units > 0 {
            index += units * 2;
        } else if ascii > 1 {
            index += ascii - 1;
        } else {
            index += 1;
        }
    }
    render(&kept, skipped)
}

fn notable(text: &str) -> bool {
    text.contains("permission") || text.contains("Audio") || text.contains("microphone") || text.contains("MediaRecorder") || text.contains("RECORD_") || text.contains("getUserMedia") || text.contains("ultrasonic")
}

fn ordinary_count(kept: &[Run]) -> usize {
    kept.iter().filter(|run| !notable(&run.text)).count()
}

fn extra_count(kept: &[Run]) -> usize {
    kept.len() - ordinary_count(kept)
}

fn wanted(kept: &[Run], run: &Run) -> bool {
    if notable(&run.text) {
        ordinary_count(kept) < RUN_LIMIT || extra_count(kept) < EXTRA_LIMIT
    } else {
        ordinary_count(kept) < RUN_LIMIT
    }
}

fn printable(byte: u8) -> bool {
    (0x20..=0x7e).contains(&byte)
}

fn keep(kept: &mut Vec<Run>, run: Run) -> bool {
    if kept.iter().any(|earlier| earlier.offset < run.end && run.offset < earlier.end) {
        return false;
    }
    kept.push(run);
    true
}

fn ascii_len(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().take_while(|byte| printable(**byte)).count()
}

fn ascii_run(bytes: &[u8], start: usize, length: usize) -> Run {
    let text = bytes[start..start + length].iter().map(|byte| char::from(*byte)).collect();
    Run {
        offset: start,
        encoding: "ascii",
        length,
        end: start + length,
        text,
    }
}

fn utf16_unit(bytes: &[u8], index: usize) -> bool {
    match (bytes.get(index), bytes.get(index + 1)) {
        (Some(&low), Some(&0)) => printable(low),
        _ => false,
    }
}

fn utf16_units(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    let mut units = 0;
    while utf16_unit(bytes, index) {
        units += 1;
        index += 2;
    }
    units
}

fn utf16_run(bytes: &[u8], start: usize, units: usize) -> Run {
    let mut text = String::with_capacity(units);
    let mut at = start;
    for _ in 0..units {
        text.push(char::from(bytes[at]));
        at += 2;
    }
    Run {
        offset: start,
        encoding: "utf16",
        length: units,
        end: start + units * 2,
        text,
    }
}

fn render(kept: &[Run], skipped: bool) -> String {
    if kept.is_empty() {
        return if skipped {
            "no text runs were found\nlater text runs were not listed".to_string()
        } else {
            "no text runs were found".to_string()
        };
    }
    let mut out = String::new();
    for (index, run) in kept.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "text 0x{:x} {} length {} {}",
            run.offset, run.encoding, run.length, shown(&run.text)
        ));
    }
    if skipped {
        out.push_str("\nlater text runs were not listed");
    }
    out
}

fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut more = false;
    for (index, ch) in text.chars().enumerate() {
        if index == TEXT_CAP {
            more = true;
            break;
        }
        out.push(ch);
    }
    if more {
        out.push_str("...");
    }
    out
}

fn qualifying_after(bytes: &[u8], mut index: usize, minimum: usize, kept: &[Run]) -> bool {
    while index < bytes.len() {
        if kept.iter().any(|run| run.offset <= index && index < run.end) {
            index += 1;
            continue;
        }
        let units = utf16_units(bytes, index);
        let ascii = ascii_len(bytes, index);
        if units >= minimum || ascii >= minimum {
            return true;
        }
        if units > 0 {
            index += units * 2;
        } else if ascii > 1 {
            index += ascii - 1;
        } else {
            index += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_lists_ascii_length() {
        let out = text_catalog(b"hello\0", 4);
        assert!(out.contains("hello"));
        assert!(out.contains("ascii"));
        assert!(out.contains("length 5"));
        assert_eq!(out, "text 0x0 ascii length 5 hello");
    }

    #[test]
    fn long_ascii_run_is_marked() {
        let bytes = vec![b'A'; 65];
        let text = "A".repeat(64);
        assert_eq!(
            text_catalog(&bytes, 4),
            format!("text 0x0 ascii length 65 {text}...")
        );
    }

    #[test]
    fn short_and_empty_slices_report_no_runs() {
        assert_eq!(text_catalog(b"", 4), "no text runs were found");
        assert_eq!(text_catalog(b"hi\0", 4), "no text runs were found");
        assert_eq!(text_catalog(&[0x1f, 0x7f], 1), "no text runs were found");
    }

    #[test]
    fn zero_minimum_keeps_a_single_byte() {
        assert_eq!(text_catalog(b"Z", 0), "text 0x0 ascii length 1 Z");
    }

    #[test]
    fn utf16_run_uses_code_units_and_hides_overlapped_ascii() {
        let bytes = [b'A', 0, b'B', 0, b'C', 0, b'D', 0];
        assert_eq!(text_catalog(&bytes, 4), "text 0x0 utf16 length 4 ABCD");
        assert_eq!(text_catalog(&bytes, 1), "text 0x0 utf16 length 4 ABCD");
    }

    #[test]
    fn separate_runs_are_both_kept() {
        let mut bytes = b"hello\0".to_vec();
        bytes.extend([b'A', 0, b'B', 0, b'C', 0, b'D', 0]);
        assert_eq!(
            text_catalog(&bytes, 4),
            "text 0x0 ascii length 5 hello\ntext 0x6 utf16 length 4 ABCD"
        );
    }

    #[test]
    fn earlier_run_suppresses_an_overlapping_later_run() {
        let bytes = [b'A', b'B', b'C', 0, b'D', 0, b'E', 0];
        assert_eq!(
            text_catalog(&bytes, 2),
            "text 0x0 ascii length 3 ABC\ntext 0x4 utf16 length 2 DE"
        );
    }

    #[test]
    fn notable_text_far_into_the_slice_is_kept() {
        let mut bytes = vec![0u8; 70_000];
        bytes.extend_from_slice(b"RECORD_AUDIO");
        let report = text_catalog(&bytes, 6);
        assert_eq!(report, "text 0x11170 ascii length 12 RECORD_AUDIO");
    }

    #[test]
    fn enumeration_stops_at_sixty_four() {
        let mut bytes = Vec::new();
        for _ in 0..80 {
            bytes.extend(b"word\0");
        }
        let report = text_catalog(&bytes, 4);
        assert_eq!(report.lines().count(), 65);
        assert!(report.starts_with("text 0x0 ascii length 4 word\n"), "{report}");
        assert!(report.ends_with("later text runs were not listed"));
        bytes.extend(b"RECORD_AUDIO\0");
        let later = text_catalog(&bytes, 4);
        assert!(later.contains("RECORD_AUDIO"), "{later}");
        assert_eq!(later.lines().count(), 66);
        assert!(later.ends_with("later text runs were not listed"));
    }
}

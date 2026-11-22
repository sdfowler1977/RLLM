//! Printable ASCII runs in a captured byte sample.
//! A run is not executed.

const RUN_CAP: usize = 160;
const TEXT_CAP: usize = 256;

pub fn ascii_runs(data: &[u8]) -> String {
    let mut lines = Vec::new();
    let mut start = None;
    let mut unread = false;
    for (index, byte) in data.iter().copied().chain(std::iter::once(0)).enumerate() {
        if byte.is_ascii_graphic() || byte == b' ' {
            if start.is_none() {
                start = Some(index);
            }
            continue;
        }
        let Some(begin) = start.take() else {
            continue;
        };
        if index - begin < 5 {
            continue;
        }
        if lines.len() == RUN_CAP {
            unread = true;
            break;
        }
        let stop = index.min(begin + TEXT_CAP);
        let mut text = String::from_utf8_lossy(&data[begin..stop]).into_owned();
        if index > begin + TEXT_CAP {
            text.push_str("...");
        }
        lines.push(format!("0x{begin:08x}: {text}"));
    }
    if unread {
        lines.push("later strings were not listed".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::ascii_runs;

    #[test]
    fn one_run_keeps_its_offset() {
        assert_eq!(ascii_runs(b"hello"), "0x00000000: hello");
        assert_eq!(ascii_runs(b"abcd"), "");
    }

    #[test]
    fn long_run_is_marked() {
        let data = vec![b'A'; 257];
        let shown = "A".repeat(256);
        assert_eq!(ascii_runs(&data), format!("0x00000000: {shown}..."));
    }

    #[test]
    fn the_list_stops_at_one_hundred_sixty() {
        let mut data = Vec::new();
        for index in 0..161 {
            data.extend(format!("run{index:03}").as_bytes());
            data.push(0);
        }
        let report = ascii_runs(&data);
        let last = 159 * 7;
        let skipped = 160 * 7;
        assert!(report.contains("0x00000000: run000\n"), "{report}");
        assert!(report.contains(&format!("\n0x{last:08x}: run159\n")), "{report}");
        assert!(report.ends_with("later strings were not listed"), "{report}");
        assert!(!report.contains(&format!("0x{skipped:08x}:")), "{report}");
        assert!(!report.contains("run160"), "{report}");
    }
}

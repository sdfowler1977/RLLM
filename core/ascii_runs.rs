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

//! Tensor graph edges from a caller-supplied record.
//!
//! The record is the magic `TGR1`, a little-endian node count, a little-endian
//! edge count, each node name as a little-endian byte length plus that many
//! bytes, then each edge as two little-endian node indexes. At most 32 nodes
//! and 32 edges are listed. This walk does not load a model and does not
//! execute operators.

const MAGIC: &[u8] = b"TGR1";
const LIST_LIMIT: usize = 32;
const NAME_CAP: usize = 128;

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

struct Listed {
    lines: Vec<String>,
    omitted: bool,
    ended: bool,
}

impl<'a> Cursor<'a> {
    fn u16(&mut self) -> Option<u16> {
        let raw = self.bytes.get(self.at..self.at.checked_add(2)?)?;
        self.at += 2;
        Some(u16::from_le_bytes([raw[0], raw[1]]))
    }

    fn name_bytes(&mut self) -> Option<&'a [u8]> {
        let len = usize::from(self.u16()?);
        let end = self.at.checked_add(len)?;
        let raw = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(raw)
    }
}

/// List nodes and edges in a caller-supplied graph record.
///
/// A record that does not begin with `TGR1` and both counts is not identified.
/// An edge index at or past the declared node count is not resolved. A source
/// index read before the destination index is listed, then the record is
/// reported as ending early. The returned text ends with `Operators were not
/// executed.` when a record was identified.
pub fn tensor_edges(bytes: &[u8]) -> String {
    let Some((mut cursor, node_count, edge_count)) = open(bytes) else {
        return "A tensor graph record was not identified.".to_string();
    };
    let nodes = take_nodes(&mut cursor, node_count);
    let edges = if nodes.ended {
        None
    } else {
        Some(take_edges(&mut cursor, edge_count, node_count))
    };
    let mut report = format!("nodes {node_count}\nedges {edge_count}\n");
    for line in &nodes.lines {
        report.push_str(line);
        report.push('\n');
    }
    if nodes.omitted {
        report.push_str("later nodes were not listed\n");
    }
    if nodes.ended {
        report.push_str("The record ended before every declared node was read.\n");
    }
    if let Some(edges) = edges {
        for line in &edges.lines {
            report.push_str(line);
            report.push('\n');
        }
        if edges.omitted {
            report.push_str("later edges were not listed\n");
        }
        if edges.ended {
            report.push_str("The record ended before every declared edge was read.\n");
        }
    }
    report.push_str("Operators were not executed.\n");
    report
}

fn open(bytes: &[u8]) -> Option<(Cursor<'_>, u16, u16)> {
    if bytes.get(..4)? != MAGIC || bytes.len() < 8 {
        return None;
    }
    let node_count = u16::from_le_bytes([bytes[4], bytes[5]]);
    let edge_count = u16::from_le_bytes([bytes[6], bytes[7]]);
    Some((
        Cursor {
            bytes,
            at: 8,
        },
        node_count,
        edge_count,
    ))
}

fn take_nodes(cursor: &mut Cursor<'_>, count: u16) -> Listed {
    let mut lines = Vec::new();
    let mut omitted = false;
    for index in 0..usize::from(count) {
        let Some(raw) = cursor.name_bytes() else {
            return Listed {
                lines,
                omitted,
                ended: true,
            };
        };
        if lines.len() < LIST_LIMIT {
            lines.push(format!("node {index} {}", show_name(raw)));
        } else {
            omitted = true;
        }
    }
    Listed {
        lines,
        omitted,
        ended: false,
    }
}

fn take_edges(cursor: &mut Cursor<'_>, count: u16, nodes: u16) -> Listed {
    let mut lines = Vec::new();
    let mut omitted = false;
    for index in 0..usize::from(count) {
        let Some(from) = cursor.u16() else {
            return Listed {
                lines,
                omitted,
                ended: true,
            };
        };
        let Some(to) = cursor.u16() else {
            if lines.len() < LIST_LIMIT {
                lines.push(partial_edge(index, from, nodes));
            } else {
                omitted = true;
            }
            return Listed {
                lines,
                omitted,
                ended: true,
            };
        };
        if lines.len() < LIST_LIMIT {
            lines.push(edge_line(index, from, to, nodes));
        } else {
            omitted = true;
        }
    }
    Listed {
        lines,
        omitted,
        ended: false,
    }
}

fn edge_line(index: usize, from: u16, to: u16, nodes: u16) -> String {
    if from >= nodes || to >= nodes {
        format!("edge {index} from {from} to {to}: the edge was not resolved")
    } else {
        format!("edge {index} from {from} to {to}")
    }
}

fn partial_edge(index: usize, from: u16, nodes: u16) -> String {
    if from >= nodes {
        format!("edge {index} from {from}: the edge was not resolved")
    } else {
        format!("edge {index} from {from}")
    }
}

fn show_name(raw: &[u8]) -> String {
    let printable = raw.iter().all(|byte| (0x20..=0x7e).contains(byte));
    if !printable {
        let shown = raw.len().min(16);
        let hex: String = raw[..shown].iter().map(|byte| format!("{byte:02x}")).collect();
        if raw.len() > shown {
            return format!("bytes {hex}...");
        }
        return format!("bytes {hex}");
    }
    let text = String::from_utf8_lossy(raw);
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index == NAME_CAP {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

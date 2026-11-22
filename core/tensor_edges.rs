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
/// An edge index at or past the declared node count is not resolved. The
/// returned text ends with `Operators were not executed.` when a record was
/// identified.
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

#[cfg(test)]
mod checks {
    use super::tensor_edges;

    fn record(names: &[&str], edges: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"TGR1");
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(edges.len() as u16).to_le_bytes());
        for name in names {
            let raw = name.as_bytes();
            out.extend_from_slice(&(raw.len() as u16).to_le_bytes());
            out.extend_from_slice(raw);
        }
        for (from, to) in edges {
            out.extend_from_slice(&from.to_le_bytes());
            out.extend_from_slice(&to.to_le_bytes());
        }
        out
    }

    #[test]
    fn two_nodes_and_one_edge() {
        let report = tensor_edges(&record(&["left", "right"], &[(0, 1)]));
        assert_eq!(
            report,
            "nodes 2\nedges 1\nnode 0 left\nnode 1 right\nedge 0 from 0 to 1\nOperators were not executed.\n"
        );
    }

    #[test]
    fn a_thirty_third_node_is_named() {
        let mut names: Vec<String> = (0..32).map(|index| format!("n{index}")).collect();
        names.push("n32".to_string());
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let report = tensor_edges(&record(&refs, &[(0, 1)]));
        assert!(report.starts_with("nodes 33\nedges 1\n"), "{report}");
        assert!(report.contains("node 0 n0\n"), "{report}");
        assert!(report.contains("node 31 n31\n"), "{report}");
        assert!(report.contains("later nodes were not listed\n"), "{report}");
        assert!(report.contains("edge 0 from 0 to 1\n"), "{report}");
        assert!(!report.contains("n32"), "{report}");
        assert!(!report.contains("node 32"), "{report}");
        assert!(report.ends_with("Operators were not executed.\n"), "{report}");
    }

    #[test]
    fn a_dangling_edge_is_unresolved() {
        let report = tensor_edges(&record(&["only"], &[(0, 4)]));
        assert_eq!(
            report,
            "nodes 1\nedges 1\nnode 0 only\nedge 0 from 0 to 4: the edge was not resolved\nOperators were not executed.\n"
        );
    }

    #[test]
    fn a_thirty_third_edge_is_not_listed() {
        let mut edges = vec![(0u16, 1u16); 32];
        edges.push((9, 9));
        let report = tensor_edges(&record(&["a", "b"], &edges));
        assert!(report.contains("edge 31 from 0 to 1\n"), "{report}");
        assert!(report.contains("later edges were not listed\n"), "{report}");
        assert!(!report.contains("from 9 to 9"), "{report}");
        assert!(!report.contains("was not resolved"), "{report}");
        assert!(report.ends_with("Operators were not executed.\n"), "{report}");
    }
}

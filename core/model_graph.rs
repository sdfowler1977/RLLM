//! Identify a tensor graph from caller-supplied bytes. Operators are not executed.

/// Report a TFL3 declared size or a protobuf string field whose value is `graph`.
///
/// A TFL3 prefix of at least eight bytes is read as an identifier plus a
/// little-endian `u32` size. Otherwise a tag `0x0A`, length `5`, and the ASCII
/// bytes `graph` name the graph. No operator is run in either case.
pub fn model_graph(bytes: &[u8]) -> String {
    if bytes.len() >= 8 && bytes.starts_with(b"TFL3") {
        let size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        return format!(
            "tensor graph identifier TFL3 declared {size} bytes. Operators were not executed."
        );
    }
    if bytes.windows(7).any(|window| window == b"\x0A\x05graph") {
        return "tensor graph name graph. Operators were not executed.".to_string();
    }
    "A tensor graph was not identified.".to_string()
}

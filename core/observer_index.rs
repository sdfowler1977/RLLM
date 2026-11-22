//! Internal observer kinds. No external engine is called.

const KINDS: &[&str] = &[
    "listing",
    "text",
    "manifest",
    "image",
    "assembly",
    "trace",
    "tone",
    "graph",
    "datagram",
    "script",
    "session",
    "pack",
];

pub fn observer_index(kind: &str) -> String {
    let state = if KINDS.contains(&kind) {
        "internal"
    } else {
        "not implemented"
    };
    format!("observer {kind} is {state}. An external engine was not invoked.")
}

#[cfg(test)]
mod tests {
    use super::observer_index;

    #[test]
    fn listing_is_internal() {
        let text = observer_index("listing");
        assert!(text.contains("listing"));
        assert!(text.contains("internal"));
        assert!(text.contains("not invoked"));
        assert_eq!(
            text,
            "observer listing is internal. An external engine was not invoked."
        );
    }

    #[test]
    fn unknown_kind_is_not_implemented() {
        let text = observer_index("other");
        assert_eq!(
            text,
            "observer other is not implemented. An external engine was not invoked."
        );
    }
}

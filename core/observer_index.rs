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

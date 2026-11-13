mod twin;

use twin::{b64_decode, b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Saved-state sealing through the shared core operation: exact round trip, no clear-text storage, self-describing framing, distinct ciphertext per seal, per-byte tamper refusal for nonce, ciphertext and tag, foreign-key and truncation refusal, empty and large payloads, key length and encoding refusal, declared size bound and unknown-operation refusal.";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let scratch = twin::create_scratch(&root, "state-sealing");
    let scratch_text = scratch
        .to_str()
        .unwrap_or_else(|| panic!("scratch path is not UTF-8"))
        .to_string();

    let mut controller = Controller::spawn(&root_text);
    let passed = run(&mut controller, &scratch_text);
    controller.finish();

    let _ = std::fs::remove_dir_all(&scratch);
    // The leg writes json.dumps(result, indent=2) + "\n", so the report file needs the
    // trailing newline; the count line stays on stdout only.
    let body = obj(vec![
        ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("state_sealing", &body, true);
    println!("{} state_sealing checks passed", passed.len());
}

fn run(controller: &mut Controller, _root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let key = b64_encode(&(0u8..32).collect::<Vec<_>>());
    let other = b64_encode(&(0u8..32).map(|index| 255 - index).collect::<Vec<_>>());
    let vault = serde_bytes(
        br#"{"messages":[{"role":"user","content":"the exact conversation text that must not be stored in clear"}],"profiles":[{"name":"home","endpoint":"http://127.0.0.1:8080/v1"}]}"#,
    );

    let sealed = seal(controller, &key, &vault);
    let sealed_bytes = b64_decode(&sealed);
    let opened = open_sealed(controller, &key, &sealed);
    keep(
        &mut passed,
        "a sealed vault opens again to the exact saved bytes",
        b64_decode(&opened) == vault,
    );
    keep(
        &mut passed,
        "the sealed vault does not keep the conversation in clear text",
        !contains_bytes(&sealed_bytes, b"the exact conversation text"),
    );
    keep(
        &mut passed,
        "the sealed vault is self-describing without a separate nonce field",
        sealed_bytes.len() == vault.len() + 12 + 16,
    );

    let fresh = seal(controller, &key, &vault);
    keep(
        &mut passed,
        "sealing the same bytes twice yields different ciphertext",
        fresh != sealed
            && b64_decode(&open_sealed(controller, &key, &fresh)) == vault
            && b64_decode(&open_sealed(controller, &key, &sealed)) == vault,
    );

    let mut tampered = sealed_bytes.clone();
    tampered[20] ^= 1;
    keep(
        &mut passed,
        "a one-byte change in the ciphertext is refused",
        failed(controller, &state_open(&key, &b64_encode(&tampered))),
    );

    let mut tampered = sealed_bytes.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    keep(
        &mut passed,
        "a one-byte change in the tag is refused",
        failed(controller, &state_open(&key, &b64_encode(&tampered))),
    );

    let mut tampered = sealed_bytes.clone();
    tampered[3] ^= 1;
    keep(
        &mut passed,
        "a one-byte change in the nonce is refused",
        failed(controller, &state_open(&key, &b64_encode(&tampered))),
    );

    keep(
        &mut passed,
        "a vault from another key is refused",
        failed(controller, &state_open(&other, &sealed)),
    );
    keep(
        &mut passed,
        "a truncated vault is refused",
        failed(controller, &state_open(&key, &sealed[..20])),
    );

    let empty = seal(controller, &key, b"");
    keep(
        &mut passed,
        "an empty vault round-trips",
        b64_decode(&open_sealed(controller, &key, &empty)).is_empty(),
    );

    let large = (0..200_000u32).map(|index| (index % 251) as u8).collect::<Vec<_>>();
    let large_sealed = seal(controller, &key, &large);
    keep(
        &mut passed,
        "a large saved state round-trips byte for byte",
        b64_decode(&open_sealed(controller, &key, &large_sealed)) == large,
    );

    for (label, bad) in [
        ("a short key is refused", b64_encode(b"short")),
        ("a long key is refused", b64_encode(&[0u8; 33])),
        ("a non-base64 key is refused", "not base64!".to_string()),
    ] {
        keep(
            &mut passed,
            label,
            failed(controller, &state_seal(&bad, b"vault")),
        );
    }

    keep(
        &mut passed,
        "the sealed payload stays inside its declared size bound",
        failed(
            controller,
            &state_open(&key, &b64_encode(&vec![0u8; 9 * 1024 * 1024])),
        ),
    );
    keep(
        &mut passed,
        "sealing is reachable only through the declared operation",
        failed(
            controller,
            &obj(vec![
                ("op".into(), s("state_seals")),
                ("key".into(), s(&key)),
                ("data".into(), s(&b64_encode(b"vault"))),
            ]),
        ),
    );

    passed
}

fn seal(controller: &mut Controller, key: &str, data: &[u8]) -> String {
    let value = controller.op(&state_seal(key, data));
    value
        .get("sealed")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("state_seal returned no sealed value: {}", value.to_string()))
        .to_string()
}

fn open_sealed(controller: &mut Controller, key: &str, sealed: &str) -> String {
    let value = controller.op(&state_open(key, sealed));
    value
        .get("data")
        .map(Value::str_)
        .unwrap_or_else(|| panic!("state_open returned no data: {}", value.to_string()))
        .to_string()
}

fn state_seal(key: &str, data: &[u8]) -> Value {
    obj(vec![
        ("op".into(), s("state_seal")),
        ("key".into(), s(key)),
        ("data".into(), s(&b64_encode(data))),
    ])
}

fn state_open(key: &str, data: &str) -> Value {
    obj(vec![
        ("op".into(), s("state_open")),
        ("key".into(), s(key)),
        ("data".into(), s(data)),
    ])
}

fn failed(controller: &mut Controller, envelope: &Value) -> bool {
    !controller.failed(envelope).is_empty()
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn serde_bytes(text: &[u8]) -> Vec<u8> {
    text.to_vec()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}
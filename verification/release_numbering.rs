//! The release-numbering group.
//!
//! Behavioral assertions exercise the product's own surfaces; documentation
//! rows explicitly measure text only. Tree rows join the product report to
//! independently inspected artifacts and a directly parsed record.
//! The release floor is read with the product's release tool, the
//! release numbers are decoded by the product's own manifest reader, the
//! publication record is advanced by the product's own writer and the
//! publication route is the product's publisher. Fixture expectations come
//! from their authored bytes. Live artifact expectations use the sanctioned
//! resource inspector, independently of the product decoder and report.
//!
//! The fixtures are written in the layout a compiled AndroidManifest has:
//! a UTF-8 string pool, one manifest element, and one attribute table of
//! twenty-byte entries. The layout was read from a retained artifact of this
//! tree, not from the product's source, so a fixture that the product reads
//! is a fixture a compiled manifest would also be.

mod twin;

use std::fs;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use twin::{n, obj, report_value, s, sha256_bytes, Value};

const SCOPE: &str =
    "release floors, per-package floors, the publication record and the documented release number";

/// The namespace the android release number belongs to.
const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";
/// The namespace of a third party: an attribute named versionCode that belongs
/// to it is not the android release number.
const OTHER_NS: &str = "http://example.invalid/res/other";
/// The index a string pool entry that does not exist carries.
const NO_ENTRY: u32 = 0xFFFF_FFFF;
/// The value type of a string.
const TYPE_STRING: u8 = 0x03;
/// The value type of a decimal integer.
const TYPE_INT_DEC: u8 = 0x10;

/// The build script a room carries. The route reads the tree's own build
/// inputs before it reads the floor, and it rebuilds the release tool from
/// its own source before it consults that tool, so a room has to carry that
/// shape or the route cannot run in it. A room is a reduced tree, so its
/// build script builds the one tool the floor route consults, from the room's
/// own source, with the compile line the tree's build script uses for that
/// tool. The tool the route consults is therefore the tool the room's source
/// builds, and the floor the route enforces is the product's own floor.
const ROOM_BUILD_TOOLS: &str = r#"#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
output=${1:-"$project_dir/output/tools"}
mkdir -p "$output"
if [[ -n "${CLIENT_RUST:-}" ]]; then
  compiler="$CLIENT_RUST/bin/rustc"
else
  compiler="$(command -v rustc || true)"
fi
if [[ -z "$compiler" ]]; then
  printf '%s\n' 'the room has no Rust compiler to build its release tool with' >&2
  exit 2
fi
if [[ -n "${CLIENT_RUST:-}" ]]; then
  export LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}"
fi
"$compiler" --edition=2021 --crate-name apk \
  --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
  -C linker-features=-lld "$project_dir/tools/apk.rs" -o "$output/apk"
"#;

// ---------------------------------------------------------------------------
// The fixtures: bytes the product reads.
// ---------------------------------------------------------------------------

/// A string pool in the layout a compiled resource pool has: a UTF-8 pool,
/// one offset per string, and one length pair and one NUL-terminated string
/// per entry.
fn pool(strings: &[String]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for text in strings {
        offsets.push(data.len() as u32);
        data.push(text.chars().count() as u8);
        data.push(text.len() as u8);
        data.extend_from_slice(text.as_bytes());
        data.push(0);
    }
    let header = 28usize;
    let size = header + strings.len() * 4 + data.len();
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&1u16.to_le_bytes());
    chunk.extend_from_slice(&(header as u16).to_le_bytes());
    chunk.extend_from_slice(&(size as u32).to_le_bytes());
    chunk.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    chunk.extend_from_slice(&0u32.to_le_bytes());
    chunk.extend_from_slice(&(1u32 << 8).to_le_bytes());
    chunk.extend_from_slice(&((header + strings.len() * 4) as u32).to_le_bytes());
    chunk.extend_from_slice(&0u32.to_le_bytes());
    for offset in &offsets {
        chunk.extend_from_slice(&offset.to_le_bytes());
    }
    chunk.extend_from_slice(&data);
    chunk
}

/// A compiled manifest element with the given attributes. Each attribute is
/// its namespace index, its name index, its value type and its value.
fn element(attributes: &[(u32, u32, u8, u32)]) -> Vec<u8> {
    let mut encoded = Vec::new();
    for (namespace, name, data_type, data) in attributes {
        encoded.extend_from_slice(&namespace.to_le_bytes());
        encoded.extend_from_slice(&name.to_le_bytes());
        encoded.extend_from_slice(&NO_ENTRY.to_le_bytes());
        encoded.extend_from_slice(&8u16.to_le_bytes());
        encoded.push(0);
        encoded.push(*data_type);
        encoded.extend_from_slice(&data.to_le_bytes());
    }
    let mut node = Vec::new();
    node.extend_from_slice(&0x0102u16.to_le_bytes());
    node.extend_from_slice(&16u16.to_le_bytes());
    node.extend_from_slice(&((36 + encoded.len()) as u32).to_le_bytes());
    node.extend_from_slice(&1u32.to_le_bytes());
    node.extend_from_slice(&NO_ENTRY.to_le_bytes());
    node.extend_from_slice(&NO_ENTRY.to_le_bytes());
    node.extend_from_slice(&1u32.to_le_bytes());
    node.extend_from_slice(&20u16.to_le_bytes());
    node.extend_from_slice(&20u16.to_le_bytes());
    node.extend_from_slice(&(attributes.len() as u16).to_le_bytes());
    node.extend_from_slice(&0u16.to_le_bytes());
    node.extend_from_slice(&0u16.to_le_bytes());
    node.extend_from_slice(&0u16.to_le_bytes());
    node.extend_from_slice(&encoded);
    node
}

/// A compiled manifest document.
fn document(strings: &[String], attributes: &[(u32, u32, u8, u32)]) -> Vec<u8> {
    let pool = pool(strings);
    let element = element(attributes);
    let mut document = Vec::new();
    document.extend_from_slice(&3u16.to_le_bytes());
    document.extend_from_slice(&8u16.to_le_bytes());
    document.extend_from_slice(&((8 + pool.len() + element.len()) as u32).to_le_bytes());
    document.extend_from_slice(&pool);
    document.extend_from_slice(&element);
    document
}

/// A compiled manifest that names its package and declares a release number
/// in each of the given namespaces, in order. A namespace of None is an
/// attribute that belongs to no namespace at all: it is named versionCode,
/// and it is not the android release number.
fn manifest(package: &str, release_numbers: &[(Option<&str>, u32)]) -> Vec<u8> {
    let mut strings: Vec<String> = vec![
        ANDROID_NS.to_string(),
        "manifest".to_string(),
        "package".to_string(),
        package.to_string(),
    ];
    let mut attributes: Vec<(u32, u32, u8, u32)> = vec![(NO_ENTRY, 2u32, TYPE_STRING, 3u32)];
    for (namespace, code) in release_numbers {
        let namespace_index = match namespace {
            None => NO_ENTRY,
            Some(uri) => {
                if *uri == ANDROID_NS {
                    0
                } else {
                    strings.push(uri.to_string());
                    (strings.len() - 1) as u32
                }
            }
        };
        let name_index = match strings.iter().position(|entry| entry == "versionCode") {
            Some(index) => index as u32,
            None => {
                strings.push("versionCode".to_string());
                (strings.len() - 1) as u32
            }
        };
        attributes.push((namespace_index, name_index, TYPE_INT_DEC, *code));
    }
    document(&strings, &attributes)
}

// ---------------------------------------------------------------------------
// The room: a tree the product measures, built by this twin.
// ---------------------------------------------------------------------------

/// The outcome of a product invocation.
struct Outcome {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Outcome {
    fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// A tree of the product's own shape, with its own tools, its own record and
/// its own retained artifacts.
struct Room {
    root: PathBuf,
}

impl Room {
    /// A room with the tree's own sources, the tree's own route and the
    /// release tool built from the tree's own source.
    fn new(label: &str, project: &Path, tools: &Path) -> Room {
        let mut root = std::env::temp_dir();
        root.push(format!("rllm-release-numbering-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("tools")).unwrap_or_else(|error| panic!("room tools: {error}"));
        let output = root.join("output");
        fs::create_dir_all(output.join("tools")).unwrap_or_else(|error| panic!("room tools: {error}"));
        fs::create_dir_all(output.join("checks")).unwrap_or_else(|error| panic!("room checks: {error}"));
        for name in ["apk.rs", "package.rs", "harness.rs"] {
            fs::copy(project.join("tools").join(name), root.join("tools").join(name))
                .unwrap_or_else(|error| panic!("room source {name}: {error}"));
        }
        fs::copy(project.join("package.sh"), root.join("package.sh"))
            .unwrap_or_else(|error| panic!("room route: {error}"));
        fs::copy(project.join("AndroidManifest.xml"), root.join("AndroidManifest.xml"))
            .unwrap_or_else(|error| panic!("room manifest: {error}"));
        // The route names the tree's build inputs before it reads the floor
        // and rebuilds the release tool before it consults it, so a room
        // carries that shape: the input directories the fingerprint walks,
        // the aligner the fingerprint lists, and a build script that builds
        // the room's own release tool from the room's own source.
        for directory in ["core", "platform", "res", "notices"] {
            fs::create_dir_all(root.join(directory))
                .unwrap_or_else(|error| panic!("room input {directory}: {error}"));
        }
        fs::copy(project.join("tools").join("alignment.rs"), root.join("tools").join("alignment.rs"))
            .unwrap_or_else(|error| panic!("room source alignment.rs: {error}"));
        fs::write(root.join("tools").join("build-tools.sh"), ROOM_BUILD_TOOLS)
            .unwrap_or_else(|error| panic!("room build script: {error}"));
        for name in ["apk", "package"] {
            fs::copy(tools.join(name), output.join("tools").join(name))
                .unwrap_or_else(|error| panic!("room tool {name}: {error}"));
        }
        Room { root }
    }

    /// The publication record of the room.
    fn record_path(&self) -> PathBuf {
        self.root.join("PUBLICATION-MANIFEST.json")
    }

    fn record(&self, text: &str) {
        fs::write(self.record_path(), text).unwrap_or_else(|error| panic!("room record: {error}"));
    }

    fn record_text(&self) -> String {
        fs::read_to_string(self.record_path()).unwrap_or_else(|error| panic!("room record: {error}"))
    }

    /// A retained artifact of the room: an archive with one compiled manifest
    /// in it.
    fn artifact(&self, name: &str, manifest: &[u8]) {
        let directory = self.root.join("APK");
        fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("room fixtures: {error}"));
        twin::zip_write(
            &directory.join(name),
            &[("AndroidManifest.xml".to_string(), manifest.to_vec())],
        );
    }

    /// The release tool of the room, built from the tree's own source.
    fn release_tool(&self) -> PathBuf {
        self.root.join("output").join("tools").join("apk")
    }

    /// The publisher of the room, built from the tree's own source.
    fn publisher(&self) -> PathBuf {
        self.root.join("output").join("tools").join("package")
    }

    /// The release floor of a package, or of the whole tree when no package
    /// is named, as the product's own tool reports it.
    fn floor(&self, package: Option<&str>) -> String {
        let root = self.root.to_string_lossy().to_string();
        let mut args = vec!["--release-baseline".to_string(), "--root".to_string(), root];
        if let Some(package) = package {
            args.push("--package".to_string());
            args.push(package.to_string());
        }
        let outcome = run(&self.release_tool(), &args, &self.root);
        if outcome.code != 0 {
            panic!("the release floor could not be read: {}", outcome.combined());
        }
        outcome.stdout.trim().to_string()
    }

    /// The release number the product's own tool hands out next.
    fn next(&self, package: Option<&str>) -> String {
        let root = self.root.to_string_lossy().to_string();
        let mut args = vec!["--next-version-code".to_string(), "--root".to_string(), root];
        if let Some(package) = package {
            args.push("--package".to_string());
            args.push(package.to_string());
        }
        let outcome = run(&self.release_tool(), &args, &self.root);
        if outcome.code != 0 {
            panic!("the next release number could not be read: {}", outcome.combined());
        }
        outcome.stdout.trim().to_string()
    }

    /// The product's own machine-readable report of the room.
    fn report(&self) -> Value {
        report_of(&self.release_tool(), &self.root, None)
    }

    /// The packaging route of the room, with the variant and the release
    /// number it is given.
    fn route(&self, variant: &str, code: &str, arguments: &[&str]) -> Outcome {
        let mut command = Command::new("bash");
        command.arg("package.sh");
        for argument in arguments {
            command.arg(argument);
        }
        command
            .current_dir(&self.root)
            .env("CLIENT_VARIANT", variant)
            .env("CLIENT_VERSION_CODE", code)
            // The route rebuilds the release tool from the room's own source
            // before it reads the floor, so the room's build script needs the
            // compiler the twin itself resolved, not an ambient guess.
            .env("CLIENT_RUST", toolchain_root());
        let output = command
            .output()
            .unwrap_or_else(|error| panic!("the packaging route could not be run: {error}"));
        Outcome {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }
    }

    /// The publisher's own release-numbering route.
    fn publisher_route(&self, apply: bool) -> Outcome {
        let mut args = vec!["--release-numbering".to_string()];
        if apply {
            args.push("--apply".to_string());
        }
        run(&self.publisher(), &args, &self.root)
    }

    /// The length and the digest of a room artifact, measured from the file
    /// itself.
    fn artifact_facts(&self, name: &str) -> (i64, String) {
        let file = self.root.join("APK").join(name);
        let bytes = fs::metadata(&file)
            .map(|meta| meta.len() as i64)
            .unwrap_or_else(|error| panic!("room artifact {name}: {error}"));
        let digest = sha256_bytes(
            &fs::read(&file).unwrap_or_else(|error| panic!("room artifact {name}: {error}")),
        );
        (bytes, digest)
    }

    /// The inspected archive record of the room, written from the product's
    /// own measurement of the artifact rather than from a number written by
    /// hand: the release tool reads the release number out of the artifact
    /// itself, and the record names the file, the length it has and its
    /// digest.
    fn inspected_release(&self, name: &str) -> i64 {
        let report = report_of(&self.release_tool(), &self.root, None);
        let artifacts = report
            .get("artifacts")
            .unwrap_or_else(|| panic!("the release report holds no artifacts"))
            .arr();
        let mut measured = None;
        for artifact in artifacts {
            let path = text(artifact, "path").unwrap_or_default();
            if !path.ends_with(name) {
                continue;
            }
            measured = number(artifact, "version_code");
            break;
        }
        let measured = measured
            .unwrap_or_else(|| panic!("the product's own route read no release number from {name}"));
        let (bytes, digest) = self.artifact_facts(name);
        let record = format!(
            "{{\n  \"version\": {measured},\n  \"path\": \"APK/{name}\",\n  \"bytes\": {bytes},\n  \"sha256\": \"{digest}\"\n}}\n"
        );
        fs::write(self.root.join("output").join("checks").join("apk.json"), record)
            .unwrap_or_else(|error| panic!("room inspected archive: {error}"));
        measured
    }

    /// The inspected archive record of the room, read back as it was written.
    fn inspected_record(&self) -> Value {
        let path = self.root.join("output").join("checks").join("apk.json");
        Value::parse(
            fs::read_to_string(&path).unwrap_or_else(|error| panic!("room inspected archive: {error}"))
                .trim(),
        )
    }
}

fn run(program: &Path, arguments: &[String], cwd: &Path) -> Outcome {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("{} could not be run: {error}", program.display()));
    Outcome {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

/// The product's own machine-readable report of a tree.
fn report_of(tool: &Path, root: &Path, package: Option<&str>) -> Value {
    let root = root.to_string_lossy().to_string();
    let mut args = vec!["--release-report-json".to_string(), "--root".to_string(), root.clone()];
    if let Some(package) = package {
        args.push("--package".to_string());
        args.push(package.to_string());
    }
    let outcome = run(tool, &args, Path::new(&root));
    if outcome.code != 0 {
        panic!("the release report could not be read: {}", outcome.combined());
    }
    Value::parse(outcome.stdout.trim())
}

fn number(value: &Value, key: &str) -> Option<i64> {
    match value.get(key) {
        Some(Value::Num(token)) => token.parse().ok(),
        _ => None,
    }
}

/// A number a record holds directly, not as the member of an object.
fn number_value(value: &Value) -> Option<i64> {
    match value {
        Value::Num(token) => token.parse().ok(),
        _ => None,
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Value::Str(token)) => Some(token.clone()),
        _ => None,
    }
}

/// The release number the product's own report gives an artifact, and the
/// package it gives it.
fn artifact_facts(report: &Value, name: &str) -> (Option<String>, Option<String>) {
    let artifacts = report
        .get("artifacts")
        .unwrap_or_else(|| panic!("the release report holds no artifacts: {}", report.to_string()))
        .arr();
    for artifact in artifacts {
        let path = text(artifact, "path").unwrap_or_default();
        if !path.ends_with(name) {
            continue;
        }
        return (
            artifact.get("version_code").map(|value| value.to_string()),
            text(artifact, "package"),
        );
    }
    panic!("the release report holds no artifact named {name}");
}

/// The package a refusal names as the package the release floor is held to.
/// The package is read out of the route's own diagnostic, so the assertion
/// joins two surfaces of the product instead of restating a constant.
fn package_named_in(text: &str) -> Option<String> {
    let rest = text.split("of package ").nth(1)?;
    let end = rest.find(|character: char| character == ';' || character.is_whitespace())?;
    Some(rest[..end].to_string())
}

/// The package a rendered manifest declares.
fn declared_package(text: &str) -> Option<String> {
    let rest = text.split("package=\"").nth(1)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn check(passed: &mut Vec<String>, name: &str, value: bool) {
    if !value {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

// ---------------------------------------------------------------------------
// The legs.
// ---------------------------------------------------------------------------

/// The release number is the android-namespaced attribute, and only that
/// attribute. The fixtures are compiled manifests; the product decodes them.
fn leg_decoder(project: &Path, tools: &Path, passed: &mut Vec<String>) {
    let room = Room::new("decoder", project, tools);
    room.record("{\n  \"package\": \"com.local.client.lab\",\n  \"version\": 1794270237\n}\n");
    room.artifact(
        "namespaced.apk",
        &manifest("com.local.client.lab", &[(Some(ANDROID_NS), 1794270237)]),
    );
    room.artifact(
        "unqualified.apk",
        &manifest("com.local.client.lab", &[(None, 42)]),
    );
    room.artifact(
        "third.apk",
        &manifest("com.local.client.lab", &[(Some(OTHER_NS), 43)]),
    );
    room.artifact(
        "both.apk",
        &manifest(
            "com.local.client.lab",
            &[(None, 44), (Some(ANDROID_NS), 1794270238)],
        ),
    );
    let report = room.report();
    let (code, package) = artifact_facts(&report, "namespaced.apk");
    check(
        passed,
        "the android-namespaced release number is the release number",
        code.as_deref() == Some("1794270237"),
    );
    check(
        passed,
        "the product reads the package a compiled manifest declares",
        package.as_deref() == Some("com.local.client.lab"),
    );
    let (code, _) = artifact_facts(&report, "unqualified.apk");
    check(
        passed,
        "an unqualified versionCode is not the release number",
        code.is_none(),
    );
    let (code, _) = artifact_facts(&report, "third.apk");
    check(
        passed,
        "a versionCode of a third namespace is not the release number",
        code.is_none(),
    );
    let (code, _) = artifact_facts(&report, "both.apk");
    check(
        passed,
        "the namespaced release number is the one that counts when both are declared",
        code.as_deref() == Some("1794270238"),
    );
}

/// The release floor of a package is the join of the record and the release
/// numbers the retained artifacts of that package carry.
fn leg_floor(project: &Path, tools: &Path, passed: &mut Vec<String>) {
    let room = Room::new("floor", project, tools);
    room.record(
        "{\n  \"package\": \"com.local.client.lab\",\n  \"version\": 100,\n  \"distributed_baseline\": 200\n}\n",
    );
    room.artifact(
        "one.apk",
        &manifest("com.local.client.lab", &[(Some(ANDROID_NS), 300)]),
    );
    check(
        passed,
        "the release floor of a package joins the record and the artifacts of that package",
        room.floor(Some("com.local.client.lab")) == "300",
    );
    check(
        passed,
        "the release number handed out next is the one above the floor",
        room.next(Some("com.local.client.lab")) == "301",
    );
    check(
        passed,
        "the release floor of the tree is the highest floor the tree holds",
        room.floor(None) == "300",
    );
    let refused = room.route("lab", "300", &["--check-version-code"]);
    check(
        passed,
        "the route refuses a release number at the release floor and names the code, the floor and the package",
        refused.code != 0
            && refused.stderr.contains(
                "Version code 300 is not above the release floor 300 of package com.local.client.lab; a release must carry a higher code.",
            ),
    );
    let accepted = room.route("lab", "301", &["--check-version-code"]);
    check(
        passed,
        "the route accepts a release number above the release floor",
        accepted.code == 0
            && accepted.stdout.contains(
                "version code 301 accepted: strictly above the release floor 300 of package com.local.client.lab",
            ),
    );
}

/// The record is read at its own top level and written at its own top level:
/// a member of the same name inside another object is neither read nor
/// rewritten.
fn leg_record(project: &Path, tools: &Path, passed: &mut Vec<String>) {
    let room = Room::new("record", project, tools);
    // The decoy sits inside another object and comes before the record's own
    // member, so a reader that descends into a nested object finds the decoy
    // first and a reader that reads the record's own top level does not see
    // it at all. Both orders are exercised by the same record.
    let nested =
        "\"distributed_baseline\": 999999999, \"distributed_baselines\": {\"com.local.client.lab\": 999999999}";
    room.record(&format!(
        "{{\n  \"package\": \"com.local.client.lab\",\n  \"version\": 200,\n  \"history\": {{{nested}}},\n  \"distributed_baseline\": 200\n}}\n"
    ));
    // The record is read back before it is trusted: a decoy that did not land
    // inside the other object, or a record that is not a JSON object at all,
    // would make every assertion below vacuous, so the fixture itself is
    // checked first.
    let written = Value::parse(room.record_text().trim());
    let decoy = match written.get("history") {
        Some(Value::Obj(members)) => members
            .iter()
            .find(|(name, _)| name == "distributed_baseline")
            .map(|(_, value)| value.to_string()),
        _ => None,
    };
    check(
        passed,
        "the record carries the decoy inside another object",
        decoy.as_deref() == Some("999999999")
            && written
                .get("distributed_baseline")
                .map(|value| value.to_string())
                .as_deref()
                == Some("200"),
    );
    check(
        passed,
        "a member of the same name inside another object does not raise the release floor",
        room.floor(Some("com.local.client.lab")) == "200",
    );
    let advanced = room.route("lab", "500", &["--advance-baseline"]);
    check(
        passed,
        "the route advances the release floor of the package it publishes",
        advanced.code == 0
            && advanced.stdout
                .contains("release floor of package com.local.client.lab advanced to 500"),
    );
    let after = room.record_text();
    check(
        passed,
        "the advance leaves a member of the same name inside another object untouched",
        after.contains(nested),
    );
    check(
        passed,
        "the advanced release floor is the one the product reads back",
        room.floor(Some("com.local.client.lab")) == "500",
    );
}

/// The version code of a package is counted per package, so each package
/// carries a release floor of its own.
fn leg_packages(project: &Path, tools: &Path, passed: &mut Vec<String>) {
    let room = Room::new("packages", project, tools);
    room.record(
        "{\n  \"package\": \"com.local.client.lab\",\n  \"version\": 1794270237,\n  \"distributed_baseline\": 1794569619\n}\n",
    );
    let intermediate = room.root.join("output/f04/javac");
    fs::create_dir_all(&intermediate).expect("intermediate directory");
    twin::zip_write(&intermediate.join("package.apk"), &[(
        "AndroidManifest.xml".to_string(),
        manifest("com.local.client", &[(Some(ANDROID_NS), 1)]),
    )]);
    check(
        passed,
        "an unpublished package uses the tree floor instead of an intermediate placeholder",
        room.floor(Some("com.local.client")) == "1794569619",
    );
    check(
        passed,
        "the allocator uses that same conservative floor for an unpublished package",
        room.next(Some("com.local.client")) == "1794569620",
    );
    let unproven = room.route("main", "1", &["--check-version-code"]);
    check(
        passed,
        "the main route refuses a placeholder code when only build intermediates name its package",
        unproven.code == 2 && unproven.stderr.contains("release floor 1794569619 of package com.local.client"),
    );
    room.artifact(
        "main.apk",
        &manifest("com.local.client", &[(Some(ANDROID_NS), 7)]),
    );
    room.artifact(
        "lab.apk",
        &manifest("com.local.client.lab", &[(Some(ANDROID_NS), 1794569620)]),
    );
    check(
        passed,
        "each package carries a release floor of its own",
        room.floor(Some("com.local.client")) == "7"
            && room.floor(Some("com.local.client.lab")) == "1794569620",
    );
    check(
        passed,
        "the release floor of the tree is the highest floor the tree holds",
        room.floor(None) == "1794569620",
    );
    let main_refused = room.route("main", "7", &["--check-version-code"]);
    check(
        passed,
        "the route of one package refuses a release number at the floor of that package",
        main_refused.code != 0
            && main_refused.stderr.contains(
                "Version code 7 is not above the release floor 7 of package com.local.client; a release must carry a higher code.",
            ),
    );
    let main_accepted = room.route("main", "8", &["--check-version-code"]);
    check(
        passed,
        "the route of one package accepts a release number above the floor of that package",
        main_accepted.code == 0
            && main_accepted.stdout.contains(
                "version code 8 accepted: strictly above the release floor 7 of package com.local.client",
            ),
    );
    let lab_refused = room.route("lab", "1794569620", &["--check-version-code"]);
    check(
        passed,
        "the route of the other package refuses a release number at the floor of that package",
        lab_refused.code != 0
            && lab_refused.stderr.contains(
                "Version code 1794569620 is not above the release floor 1794569620 of package com.local.client.lab; a release must carry a higher code.",
            ),
    );
    let lab_accepted = room.route("lab", "1794569621", &["--check-version-code"]);
    check(
        passed,
        "the route of the other package accepts a release number above the floor of that package",
        lab_accepted.code == 0
            && lab_accepted.stdout.contains(
                "version code 1794569621 accepted: strictly above the release floor 1794569620 of package com.local.client.lab",
            ),
    );
    let advanced = room.route("main", "8", &["--advance-baseline"]);
    check(
        passed,
        "the route advances the release floor of the package it publishes",
        advanced.code == 0
            && advanced.stdout
                .contains("release floor of package com.local.client advanced to 8"),
    );
    check(
        passed,
        "the advance leaves the release floor of the other package alone",
        room.floor(Some("com.local.client.lab")) == "1794569620",
    );
    check(
        passed,
        "the advanced release floor of its own package is the one the product reads back",
        room.floor(Some("com.local.client")) == "8",
    );
    let after = room.record_text();
    check(
        passed,
        "the record holds the release floor of the package that was advanced",
        after.contains("\"com.local.client\": 8"),
    );
    check(
        passed,
        "the record holds no release floor for the package that was not advanced",
        !after.contains("\"com.local.client.lab\": "),
    );
}

/// The manifest a build would carry is the manifest the route renders, so the
/// package the rendered manifest declares is the package the release floor is
/// held to: one string and not two. The package is read out of the route's own
/// refusal and compared with the rendered manifest, so the assertion joins two
/// surfaces of the product instead of restating a constant.
fn leg_render(project: &Path, tools: &Path, passed: &mut Vec<String>) {
    let room = Room::new("render", project, tools);
    room.record(
        "{\n  \"package\": \"com.local.client.lab\",\n  \"version\": 1794270237,\n  \"distributed_baseline\": 1794569619\n}\n",
    );
    room.artifact(
        "main.apk",
        &manifest("com.local.client", &[(Some(ANDROID_NS), 7)]),
    );
    room.artifact(
        "lab.apk",
        &manifest("com.local.client.lab", &[(Some(ANDROID_NS), 1794569620)]),
    );
    let refused = room.route("lab", "1794569620", &["--check-version-code"]);
    let floor_package = package_named_in(&refused.stderr);
    check(
        passed,
        "the route names the package it holds the release floor for",
        floor_package.as_deref() == Some("com.local.client.lab"),
    );
    let rendered = room.route("lab", "1794569621", &["--render-manifest"]);
    check(
        passed,
        "the route renders the manifest a build would carry",
        rendered.code == 0 && rendered.stdout.contains("<manifest"),
    );
    check(
        passed,
        "the rendered manifest declares the package the release floor is held to",
        declared_package(&rendered.stdout) == floor_package,
    );
    check(
        passed,
        "the rendered manifest carries the release number the route accepted",
        rendered.stdout.contains("android:versionCode=\"1794569621\""),
    );
    check(
        passed,
        "the rendered manifest keeps the code identity of the tree it ships",
        rendered.stdout.contains("android:name=\"com.local.client.VoiceSession\"")
            && !rendered.stdout.contains("android:name=\"com.local.client.lab."),
    );
    check(
        passed,
        "the rendered manifest publishes the callback scheme of the variant",
        rendered.stdout.contains("android:scheme=\"com.local.client.lab.oauth\""),
    );
    let main_rendered = room.route("main", "8", &["--render-manifest"]);
    check(
        passed,
        "the main variant renders the package identity the tree ships",
        declared_package(&main_rendered.stdout).as_deref() == Some("com.local.client"),
    );
    check(
        passed,
        "the main variant renders no lab package identity",
        !main_rendered.stdout.contains("com.local.client.lab"),
    );
    check(
        passed,
        "the main variant does not ship the placeholder release number",
        main_rendered.stdout.contains("android:versionCode=\"8\"")
            && !main_rendered.stdout.contains("android:versionCode=\"1\""),
    );
}

/// The publication route is the same mechanism as the release tool: it reads
/// the record, refuses a release number below the floor of the package the
/// record names and advances the record's own members.
fn leg_publisher(project: &Path, tools: &Path, passed: &mut Vec<String>) {
    let room = Room::new("publisher", project, tools);
    room.record(
        "{\n  \"package\": \"com.local.client.lab\",\n  \"version\": 100,\n  \"distributed_baseline\": 200\n}\n",
    );
    // The publication route reads the release number out of a staged
    // artifact, so the room holds one, and the number the record declares is
    // the number the product's own route measured out of that artifact.
    room.artifact(
        "below.apk",
        &manifest("com.local.client.lab", &[(Some(ANDROID_NS), 150)]),
    );
    let below = room.inspected_release("below.apk");
    let (below_bytes, below_digest) = room.artifact_facts("below.apk");
    let below_record = room.inspected_record();
    check(
        passed,
        "the inspected archive record names the release number the product's own route measured out of the artifact",
        below == 150
            && number(&below_record, "version") == Some(below)
            && text(&below_record, "path").as_deref() == Some("APK/below.apk")
            && number(&below_record, "bytes") == Some(below_bytes)
            && text(&below_record, "sha256").as_deref() == Some(below_digest.as_str()),
    );
    let before = room.record_text();
    let refused = room.publisher_route(false);
    check(
        passed,
        "the publication route refuses a release number below the release floor of the package the record names",
        refused.code != 0
            && refused.stdout.contains(&format!(
                "Version {below} is below the release floor 200 of package com.local.client.lab; a release may not be recorded downward"
            )),
    );
    check(
        passed,
        "a refused publication leaves the record exactly as it was",
        room.record_text() == before,
    );
    room.artifact(
        "above.apk",
        &manifest("com.local.client.lab", &[(Some(ANDROID_NS), 250)]),
    );
    let above = room.inspected_release("above.apk");
    let (above_bytes, above_digest) = room.artifact_facts("above.apk");
    let above_record = room.inspected_record();
    check(
        passed,
        "the inspected archive record of the accepted artifact names the release number the product's own route measured out of it",
        above == 250
            && number(&above_record, "version") == Some(above)
            && text(&above_record, "path").as_deref() == Some("APK/above.apk")
            && number(&above_record, "bytes") == Some(above_bytes)
            && text(&above_record, "sha256").as_deref() == Some(above_digest.as_str()),
    );
    let dry = room.publisher_route(false);
    check(
        passed,
        "the publication route reports the release numbering it would record",
        dry.code == 0
            && dry.stdout.contains(&format!("\"incoming_version\": {above}"))
            && dry.stdout.contains(&format!("\"next_distributed_baseline\": {above}"))
            && dry.stdout.contains("\"recorded_package\": \"com.local.client.lab\"")
            && dry.stdout.contains("\"recorded_package_release_floor\": 200")
            && dry.stdout.contains("\"applied\": false"),
    );
    check(
        passed,
        "a publication that is not applied leaves the record exactly as it was",
        room.record_text() == before,
    );
    let applied = room.publisher_route(true);
    check(
        passed,
        "the publication route advances the record when it is applied",
        applied.code == 0 && applied.stdout.contains("\"applied\": true"),
    );
    let after = room.record_text();
    check(
        passed,
        "the applied publication holds the advanced release floor of the package the record names",
        after.contains(&format!("\"distributed_baseline\": {above}"))
            && after.contains(&format!("\"com.local.client.lab\": {above}")),
    );
    let floor = room.floor(Some("com.local.client.lab"));
    check(
        passed,
        "the release tool reads the floor the publisher recorded",
        floor == above.to_string(),
    );
}

/// The coverage partition the audit log publishes for this group: the rows
/// that measure product behaviour, the rows that compare documentation text and
/// the row that pins the build configuration.
struct Partition {
    product_behaviour: usize,
    documentation_text: usize,
    build_configuration: usize,
    total_with_toolchain: usize,
    total_without_toolchain: usize,
}

/// The last number that precedes a label in a line of text.
fn last_number_before(line: &str, label: &str) -> Option<usize> {
    let at = line.find(label)?;
    let head: Vec<char> = line[..at].chars().collect();
    let mut found = None;
    let mut index = 0;
    while index < head.len() {
        if head[index].is_ascii_digit() {
            let start = index;
            while index < head.len() && head[index].is_ascii_digit() {
                index += 1;
            }
            found = head[start..index].iter().collect::<String>().parse().ok();
        } else {
            index += 1;
        }
    }
    found
}

/// The partition the audit log publishes, read from the line that carries it.
/// A log that publishes no partition, or one that publishes it unreadably,
/// publishes nothing, and the row that holds it fails.
fn published_partition(text: &str) -> Option<Partition> {
    let line = text.lines().find(|line| {
        line.contains("product-behaviour")
            && line.contains("documentation-text")
            && line.contains("build-configuration")
    })?;
    Some(Partition {
        product_behaviour: last_number_before(line, "product-behaviour")?,
        documentation_text: last_number_before(line, "documentation-text")?,
        build_configuration: last_number_before(line, "build-configuration")?,
        total_with_toolchain: last_number_before(line, "with a configured build toolchain")?,
        total_without_toolchain: last_number_before(line, "without one")?,
    })
}

/// Text-only documentation checks; these rows do not measure runtime behavior.
/// The last of them holds the coverage partition the audit log publishes to the
/// partition this twin measured, so a published number cannot drift away from
/// the rows it describes.
fn leg_documents(
    project: &Path,
    passed: &mut Vec<String>,
    product_behaviour: usize,
    build_configuration: usize,
) {
    let before = passed.len();
    let build = fs::read_to_string(project.join("docs").join("BUILD.md"))
        .unwrap_or_else(|error| panic!("the build guide is unreadable: {error}"));
    let audit = fs::read_to_string(project.join("docs").join("AUDIT.md"))
        .unwrap_or_else(|error| panic!("the audit log is unreadable: {error}"));
    check(
        passed,
        "documentation text: the build guide spells the package-qualified allocator command",
        build.contains("CLIENT_VERSION_CODE=$(")
            && build.contains("--next-version-code --package com.local.client.lab"),
    );
    check(
        passed,
        "documentation text: the build guide contains no wall-clock substitution",
        !build.contains("$(date +%s)"),
    );
    check(
        passed,
        "documentation text: the build guide names the ledger and retained artifacts",
        build.contains("distributed_baselines") && build.contains("retained artifacts"),
    );
    // This row is the last of the documentation-text rows, so the partition it
    // publishes counts itself, and it is the last row of the group, so the
    // total it publishes counts every row this twin registered.
    let documentation_text = passed.len() - before + 1;
    let total = passed.len() + 1;
    let measured = match published_partition(&audit) {
        Some(partition) => {
            partition.product_behaviour == product_behaviour
                && partition.documentation_text == documentation_text
                && partition.build_configuration == 1
                && partition.total_with_toolchain == total + (1 - build_configuration)
                && partition.total_without_toolchain == total - build_configuration
        }
        None => false,
    };
    check(
        passed,
        "documentation text: the audit log spells the package-qualified allocator command and publishes the measured release-numbering coverage partition",
        audit.contains("--next-version-code --package com.local.client.lab") && !audit.contains("CLIENT_VERSION_CODE=$(date +%s)") && measured,
    );
}

/// Filesystem inventory independent of the release-report producer.
fn artifact_paths(directory: &Path, paths: &mut Vec<PathBuf>) {
    if !directory.exists() {
        return;
    }
    for entry in fs::read_dir(directory).expect("artifact inventory is unreadable") {
        let entry = entry.expect("artifact entry is unreadable");
        let kind = entry.file_type().expect("artifact type is unreadable");
        if kind.is_dir() {
            artifact_paths(&entry.path(), paths);
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "apk") {
            paths.push(entry.path());
        }
    }
}

/// The sanctioned resource tools. tooling/env.sh exports the directory as
/// CLIENT_TOOLS and the tree ships the same copy, so the twin falls back to the
/// in-project toolchain instead of depending on an ambient variable. The copy
/// that actually carries the inspector is the one that is used.
fn resource_tools(project: &Path) -> PathBuf {
    let exported = std::env::var_os("CLIENT_TOOLS")
        .map(PathBuf::from)
        .filter(|directory| !directory.as_os_str().is_empty());
    let in_project = project.join("tooling/buildtools/android-15");
    for directory in [exported.clone(), Some(in_project.clone())].into_iter().flatten() {
        if directory.join("aapt2").is_file() {
            return directory;
        }
    }
    exported.unwrap_or(in_project)
}

/// The installed resource inspector reads the actual APK, never the product's
/// JSON or decoder. All artifacts must be measurable, so omission cannot pass.
fn inspected_artifact(project: &Path, path: &Path) -> (String, i64) {
    let inspector = resource_tools(project).join("aapt2");
    let output = Command::new(&inspector).args(["dump", "badging"])
        .arg(path).output().expect("artifact inspector could not run");
    assert!(output.status.success(), "artifact inspection failed: {}: {}",
        path.display(), String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().find(|line| line.starts_with("package: "))
        .expect("artifact inspector returned no package");
    let field = |name: &str| line.split(&format!("{name}='")).nth(1)
        .and_then(|rest| rest.split('\'').next()).map(str::to_string)
        .unwrap_or_else(|| panic!("artifact inspector returned no {name}"));
    (field("name"), field("versionCode").parse().expect("invalid inspected versionCode"))
}

/// A release the tree retains: a signed artifact under the project's APK
/// directory, or the release artifact a build leaves under its own output
/// directory. The product's release floor counts these and nothing else.
fn retained_release(project: &Path, path: &Path) -> bool {
    path.starts_with(project.join("APK"))
        || (path.parent().and_then(Path::parent) == Some(project.join("output").as_path())
            && matches!(path.file_name().and_then(|name| name.to_str()),
                Some("RLLM.apk" | "RLLM-unsigned.apk")))
}

/// The bytes and the digest of an artifact on disk, compared with the members
/// a record names for it. The record's own claim is never the evidence; the
/// file's own bytes are.
fn artifact_facts_match(artifact: &Path, entry: &Value) -> bool {
    let bytes = fs::metadata(artifact).map(|meta| meta.len()).unwrap_or(0);
    let digest = match fs::read(artifact) {
        Ok(content) => sha256_bytes(&content),
        Err(_) => return false,
    };
    bytes as i64 == number(entry, "bytes").unwrap_or(-1)
        && digest == text(entry, "sha256").unwrap_or_default()
}

/// Exact joins against independent inputs, including a nonempty physical
/// artifact inventory and the publication record read directly from disk.
fn leg_tree(project: &Path, tools: &Path, passed: &mut Vec<String>) {
    let mut paths = Vec::new();
    artifact_paths(&project.join("APK"), &mut paths);
    artifact_paths(&project.join("output"), &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "the tree retains APKs, so the floor is measured against real artifacts");
    let record = Value::parse(&fs::read_to_string(project.join("PUBLICATION-MANIFEST.json"))
        .expect("publication record is unreadable"));
    let recorded = text(&record, "package").expect("publication record names no package");
    let mut expected: BTreeMap<String, i64> = BTreeMap::new();
    let mut tree_floor = 0;
    let mut record_floor = 0;
    for key in ["version", "distributed_baseline"] {
        if let Some(code) = number(&record, key) {
            record_floor = record_floor.max(code);
            tree_floor = tree_floor.max(code);
        }
    }
    expected.insert(recorded.clone(), record_floor);
    if let Some(Value::Obj(entries)) = record.get("distributed_baselines") {
        for (package, value) in entries {
            let code: i64 = value.to_string().parse().expect("invalid ledger floor");
            tree_floor = tree_floor.max(code);
            let floor = expected.entry(package.clone()).or_default();
            *floor = (*floor).max(code);
        }
    }
    let mut artifact_packages = Vec::new();
    for path in &paths {
        let (package, code) = inspected_artifact(project, path);
        tree_floor = tree_floor.max(code);
        if retained_release(project, path) {
            let floor = expected.entry(package.clone()).or_default();
            *floor = (*floor).max(code);
        }
        artifact_packages.push(package);
    }
    for package in artifact_packages {
        expected.entry(package).or_insert(tree_floor);
    }
    let report = report_of(&tools.join("apk"), project, None);
    check(
        passed,
        "the tree floor equals independently inspected artifacts joined with the on-disk record",
        number(&report, "release_baseline") == Some(tree_floor),
    );
    let floors = report
        .get("package_floors")
        .unwrap_or_else(|| panic!("the release report holds no package floors"))
        .arr();
    let mut actual = BTreeMap::new();
    for entry in floors {
        let package = text(entry, "package").expect("reported package is absent");
        let floor = number(entry, "release_baseline").expect("reported floor is absent");
        assert!(actual.insert(package, floor).is_none(), "duplicate reported package");
    }
    check(
        passed,
        "package floors equal independently inspected retained releases joined with the on-disk record",
        actual == expected,
    );
    check(
        passed,
        "the reported record package equals the on-disk record and has its independently measured floor",
        text(&report, "recorded_package").as_deref() == Some(recorded.as_str())
            && actual.get(&recorded) == expected.get(&recorded),
    );
    // The record must name the current candidate and the releases it
    // supersedes, and every name must match the artifact measured on disk.
    let mut signed: BTreeMap<String, (String, i64)> = BTreeMap::new();
    for path in &paths {
        if !path.starts_with(project.join("APK")) {
            continue;
        }
        let (package, code) = inspected_artifact(project, path);
        let relative = path
            .strip_prefix(project)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        signed.insert(relative, (package, code));
    }
    let candidate = record
        .get("release_candidate")
        .unwrap_or_else(|| panic!("the publication record names no release candidate"));
    let candidate_path = text(candidate, "path").unwrap_or_default();
    let candidate_code = number(candidate, "version_code");
    let candidate_ok = match (signed.get(&candidate_path), candidate_code) {
        (Some((package, code)), Some(named)) => {
            *code == named
                && Some(package.as_str()) == text(candidate, "package").as_deref()
                && artifact_facts_match(&project.join(&candidate_path), candidate)
                && signed.values().all(|(_, other)| *other <= named)
                && candidate.get("above_distributed_baseline") == Some(&Value::Bool(false))
                && named < number(&record, "distributed_baseline").unwrap_or(i64::MAX)
        }
        _ => false,
    };
    check(
        passed,
        "the publication record names the current candidate, and the name matches the signed release measured on disk",
        candidate_ok,
    );
    let superseded = record
        .get("superseded_releases")
        .unwrap_or_else(|| panic!("the publication record names no superseded releases"))
        .arr();
    let mut named_paths: Vec<String> = Vec::new();
    let mut superseded_ok = !superseded.is_empty();
    for entry in superseded {
        let path = text(entry, "path").unwrap_or_default();
        let measured = signed.get(&path);
        let ok = match (measured, number(entry, "version_code")) {
            (Some((package, code)), Some(named)) => {
                *code == named
                    && Some(package.as_str()) == text(entry, "package").as_deref()
                    && artifact_facts_match(&project.join(&path), entry)
                    && number(entry, "superseded_by_version_code") == candidate_code
            }
            _ => false,
        };
        superseded_ok &= ok;
        named_paths.push(path);
    }
    let complete = named_paths.len() + 1 == signed.len()
        && signed.keys().all(|path| *path == candidate_path || named_paths.contains(path));
    check(
        passed,
        "the publication record names every superseded release, and each name matches the signed release measured on disk",
        superseded_ok && complete,
    );
    // The record's per-package map must pin the floor of the package the
    // record names, and a package the record does not name keeps the
    // tree-wide protection.
    let ledger = record.get("distributed_baselines");
    let ledger_floor = ledger
        .and_then(|map| map.get(&recorded))
        .and_then(number_value);
    let unnamed = recorded
        .rsplit_once('.')
        .map(|(head, _)| head.to_string())
        .unwrap_or_default();
    let unnamed_is_named = ledger.and_then(|map| map.get(&unnamed)).is_some();
    let selected = report_of(&tools.join("apk"), project, Some(&unnamed));
    let tree = number(&report, "release_baseline");
    check(
        passed,
        "the record's per-package floor map pins the floor of the package the record names, and a package it does not name keeps the tree-wide protection",
        ledger_floor.is_some()
            && ledger_floor == actual.get(&recorded).copied()
            && ledger_floor >= number(&record, "distributed_baseline")
            && !unnamed_is_named
            && text(&selected, "selected_package").as_deref() == Some(unnamed.as_str())
            && number(&selected, "selected_release_baseline") == tree
            && number(&selected, "selected_next_version_code") == tree.map(|value| value + 1),
    );
}

// ---------------------------------------------------------------------------
// The tools: built from the tree's own source, never taken from a binary
// another run left behind.
// ---------------------------------------------------------------------------

/// The compiler the tree's own tools are built with.
fn compiler() -> PathBuf {
    if let Some(root) = std::env::var_os("CLIENT_RUST") {
        let compiler = PathBuf::from(root).join("bin").join("rustc");
        let works = Command::new(&compiler)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        assert!(works, "the configured CLIENT_RUST compiler is unavailable");
        return compiler;
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(Path::new(&home).join(".cargo").join("bin").join("rustc"));
    }
    candidates.push(PathBuf::from("rustc"));
    for candidate in &candidates {
        let works = Command::new(candidate)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if works {
            return candidate.clone();
        }
    }
    panic!("no Rust compiler is available to build the release tool from the tree's own source");
}

/// The toolchain root of the compiler this twin builds with. A room's build
/// script is handed that root, so the tool a room's route consults is built
/// by the same compiler this twin resolved for itself.
fn toolchain_root() -> PathBuf {
    if let Some(root) = std::env::var_os("CLIENT_RUST") {
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }
    compiler()
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The release tool and the publisher, built from the tree's own source.
fn build_tools(project: &Path) -> PathBuf {
    let mut workshop = std::env::temp_dir();
    workshop.push(format!("rllm-release-numbering-tools-{}", std::process::id()));
    let _ = fs::remove_dir_all(&workshop);
    fs::create_dir_all(workshop.join("tools")).unwrap_or_else(|error| panic!("workshop: {error}"));
    fs::create_dir_all(workshop.join("output").join("tools"))
        .unwrap_or_else(|error| panic!("workshop: {error}"));
    for name in ["apk.rs", "package.rs", "harness.rs"] {
        fs::copy(project.join("tools").join(name), workshop.join("tools").join(name))
            .unwrap_or_else(|error| panic!("workshop source {name}: {error}"));
    }
    let compiler = compiler();
    for name in ["apk", "package"] {
        let source = workshop.join("tools").join(format!("{name}.rs"));
        let binary = workshop.join("output").join("tools").join(name);
        let status = Command::new(&compiler)
            .arg("--edition=2021")
            .arg("--crate-name")
            .arg(name)
            .arg("--remap-path-prefix")
            .arg(format!("{}=RLLM", workshop.display()))
            .arg("-C")
            .arg("opt-level=2")
            .arg("-C")
            .arg("codegen-units=1")
            .arg("-C")
            .arg("linker-features=-lld")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap_or_else(|error| panic!("{name} could not be compiled: {error}"));
        if !status.success() {
            panic!("{name} could not be compiled from {}", source.display());
        }
    }
    workshop.join("output").join("tools")
}

fn main() {
    let project = std::env::current_dir()
        .unwrap_or_else(|error| panic!("the project root is not readable: {error}"));
    let mut passed: Vec<String> = Vec::new();
    // The build-configuration row is registered only while CLIENT_RUST names
    // the toolchain the release tools are built with; the coverage partition
    // the audit log publishes says so, and this counter holds it to that.
    let mut build_configuration = 0usize;
    if let Some(root) = std::env::var_os("CLIENT_RUST") {
        check(&mut passed, "the release tools use the configured CLIENT_RUST compiler",
            compiler() == PathBuf::from(root).join("bin/rustc"));
        build_configuration = 1;
    }
    let tools = build_tools(&project);
    // Every leg counts the rows it registers: the coverage partition the audit
    // log publishes is measured here and never asserted from a constant.
    let mut product_behaviour = 0usize;
    let mut before = passed.len();
    leg_decoder(&project, &tools, &mut passed);
    product_behaviour += passed.len() - before;
    before = passed.len();
    leg_floor(&project, &tools, &mut passed);
    product_behaviour += passed.len() - before;
    before = passed.len();
    leg_record(&project, &tools, &mut passed);
    product_behaviour += passed.len() - before;
    before = passed.len();
    leg_packages(&project, &tools, &mut passed);
    product_behaviour += passed.len() - before;
    before = passed.len();
    leg_render(&project, &tools, &mut passed);
    product_behaviour += passed.len() - before;
    before = passed.len();
    leg_publisher(&project, &tools, &mut passed);
    product_behaviour += passed.len() - before;
    before = passed.len();
    leg_tree(&project, &tools, &mut passed);
    product_behaviour += passed.len() - before;
    // The documentation leg runs last: it publishes the partition of the whole
    // group, so every other leg has registered its rows before it does.
    leg_documents(&project, &mut passed, product_behaviour, build_configuration);
    let report = obj(vec![
        ("passed".to_string(), n(&passed.len().to_string())),
        (
            "checks".to_string(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".to_string(), s(SCOPE)),
    ]);
    report_value("release_numbering", &report, true);
    println!("{}", twin::pretty(&report));
}

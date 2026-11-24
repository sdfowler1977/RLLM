mod twin;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use twin::{obj, s, Value};

impl Value {
    fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }
}

fn string_ref(value: &Value) -> Option<&str> {
    match value {
        Value::Str(text) => Some(text.as_str()),
        _ => None,
    }
}

fn array_index(value: &Value, index: usize) -> Option<&Value> {
    match value {
        Value::Arr(items) => items.get(index),
        _ => None,
    }
}

const SCOPE: &str = "Owner connected checks: original compiled binaries, LLVM comparisons, inspection, retained queries, native views and exact exports.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let args = Args::parse(&project);
    let mut passed = Vec::new();
    let mut retained_auth = None;

    if let Some(baseline_path) = &args.auth_baseline {
        let baseline = fs::read(baseline_path)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", baseline_path.display()));
        let baseline_hash = twin::sha256_bytes(&baseline);
        let origin = "3d51564e00233805e4cb6cbc0145d1227ed125b7545a036e4a9cb0c8aaebaa6a";
        assert_eq!(
            baseline_hash,
            args.auth_baseline_sha256.as_deref().unwrap_or(origin),
            "authentication baseline digest mismatch"
        );

        let manifest = read_zip_json(baseline_path, "RLLM/PUBLICATION-MANIFEST.json");
        let prior = manifest
            .get("validation")
            .and_then(|value| value.get("retained_compatibility"));
        if baseline_hash != origin {
            assert_eq!(
                prior
                    .and_then(|value| value.get("source_sha256"))
                    .and_then(string_ref),
                Some(origin),
                "retained compatibility source mismatch"
            );
        }

        let reader_names = [
            "lib",
            "binary_evidence",
            "binary_flow",
            "binary_declarations",
            "control_flow",
            "observation_graph",
            "observation_records",
            "file_evidence",
            "mcp_schema",
            "harness_rules",
            "layout",
            "reconstruction",
            "pe_observation",
            "incremental_digest",
        ];
        let reader_paths: BTreeSet<String> = reader_names
            .iter()
            .map(|name| format!("core/{name}.rs"))
            .collect();
        let inherited = prior
            .and_then(|value| value.get("inputs"))
            .filter(|value| !matches!(value, Value::Null))
            .or_else(|| manifest.get("build_inputs"))
            .unwrap_or_else(|| panic!("missing inherited build inputs"));
        for path in &reader_paths {
            assert!(
                inherited.get(path).is_some(),
                "inherited build input is missing {path}"
            );
        }

        let mut inputs = obj(vec![]);
        for path in &reader_paths {
            let digest = inherited
                .get(path)
                .unwrap_or_else(|| panic!("missing inherited build input {path}"));
            let bytes = fs::read(project.join(path)).unwrap_or_else(|error| {
                panic!("could not read {}: {error}", project.join(path).display())
            });
            assert_eq!(
                twin::sha256_bytes(&bytes),
                digest.str_(),
                "Changed historical-reader input requires fresh compatibility execution, {path}"
            );
            if let Value::Obj(fields) = &mut inputs {
                fields.push((path.clone(), digest.clone()));
            }
        }

        let names_value = manifest
            .get("validation")
            .and_then(|value| value.get("checks"))
            .and_then(|value| value.get("binary_declarations_and_flow"))
            .and_then(|value| value.get("passed"))
            .unwrap_or_else(|| panic!("missing retained validation checks"));
        let names: Vec<String> = names_value
            .as_array()
            .unwrap_or_else(|| panic!("retained validation checks are not an array"))
            .iter()
            .map(|value| value.str_().to_string())
            .collect();
        let expected = [
            "actual revision-two controller retains original authenticated-word meaning",
            "current native graph accepts actual revision-two bytes",
            "revision-two authenticated word remains historical evidence",
        ];
        assert_eq!(
            &names[names.len().saturating_sub(3)..],
            expected,
            "retained validation check names changed"
        );

        retained_auth = Some(RetainedAuth {
            source_sha256: origin.to_string(),
            version: 1_793_600_489,
            retained_from: obj(vec![
                (
                    "version".into(),
                    manifest
                        .get("version")
                        .cloned()
                        .unwrap_or_else(|| panic!("missing publication version")),
                ),
                ("sha256".into(), s(&baseline_hash)),
            ]),
            inputs,
            checks: Value::Arr(expected.iter().map(|name| s(name)).collect()),
            scope: s("Previously executed owner checks from verified signed source; named decoder/representation-reader modules unchanged. Current dispatch, native views, HTTP decoding, capture-time interpretation and approved-command completion are exercised separately with current and actual published captures. These three checks describe their original historical execution and were not re-executed."),
        });
    }

    let ndk = PathBuf::from(
        std::env::var("CLIENT_NDK").unwrap_or_else(|error| panic!("CLIENT_NDK: {error}")),
    )
    .join("toolchains/llvm/prebuilt/linux-x86_64/bin");
    let scratch = twin::create_scratch(&project, "binary-flow");
    let root = scratch.to_string_lossy().into_owned();
    let mut current = twin::Controller::spawn(project.to_str().unwrap());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &project,
            &ndk,
            &scratch,
            &root,
            &args,
            &mut current,
            &mut passed,
            &mut retained_auth,
        )
    }));
    current.finish();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }

    let retained_count = if retained_auth.is_some() { 3 } else { 0 };
    let retained_value = retained_auth.map(RetainedAuth::into).unwrap_or(Value::Null);
    let written = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        (
            "executed_checks".into(),
            twin::n(&(passed.len() - retained_count).to_string()),
        ),
        ("retained_compatibility".into(), retained_value),
    ]);
    twin::report_value("binary_flow", &written, true);
    let stdout = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    println!("{}", twin::pretty(&stdout));
    fs::remove_dir_all(&scratch)
        .unwrap_or_else(|error| panic!("could not remove {}: {error}", scratch.display()));
}

struct Args {
    previous_flow_controller: PathBuf,
    previous_auth_controller: PathBuf,
    published_controller: PathBuf,
    auth_baseline: Option<PathBuf>,
    auth_baseline_sha256: Option<String>,
}

impl Args {
    fn parse(project: &Path) -> Self {
        let mut previous_flow_controller = project.join("output/pre-branch-controller");
        let mut previous_auth_controller = project.join("output/pre-auth-controller");
        let mut published_controller = project.join("output/published-controller");
        let mut auth_baseline = None;
        let mut auth_baseline_sha256 = None;

        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--previous-flow-controller" => {
                    previous_flow_controller = PathBuf::from(
                        arguments
                            .next()
                            .unwrap_or_else(|| panic!("--previous-flow-controller requires a path")),
                    );
                }
                "--previous-auth-controller" => {
                    previous_auth_controller = PathBuf::from(
                        arguments
                            .next()
                            .unwrap_or_else(|| panic!("--previous-auth-controller requires a path")),
                    );
                }
                "--published-controller" => {
                    published_controller = PathBuf::from(
                        arguments
                            .next()
                            .unwrap_or_else(|| panic!("--published-controller requires a path")),
                    );
                }
                "--auth-baseline" => {
                    auth_baseline = Some(PathBuf::from(
                        arguments
                            .next()
                            .unwrap_or_else(|| panic!("--auth-baseline requires a path")),
                    ));
                }
                "--auth-baseline-sha256" => {
                    auth_baseline_sha256 = Some(
                        arguments
                            .next()
                            .unwrap_or_else(|| panic!("--auth-baseline-sha256 requires a value")),
                    );
                }
                _ => panic!("unrecognized arguments: {argument}"),
            }
        }

        Self {
            previous_flow_controller,
            previous_auth_controller,
            published_controller,
            auth_baseline,
            auth_baseline_sha256,
        }
    }
}

struct RetainedAuth {
    source_sha256: String,
    version: i64,
    retained_from: Value,
    inputs: Value,
    checks: Value,
    scope: Value,
}

impl From<RetainedAuth> for Value {
    fn from(value: RetainedAuth) -> Self {
        obj(vec![
            ("source_sha256".into(), s(&value.source_sha256)),
            ("version".into(), twin::n(&value.version.to_string())),
            ("retained_from".into(), value.retained_from),
            ("inputs".into(), value.inputs),
            ("checks".into(), value.checks),
            ("scope".into(), value.scope),
        ])
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    project: &Path,
    ndk: &Path,
    scratch: &Path,
    root: &str,
    args: &Args,
    current: &mut twin::Controller,
    passed: &mut Vec<String>,
    retained_auth: &mut Option<RetainedAuth>,
) {
    let asm_path = scratch.join("original.S");
    let elf_path = scratch.join("original.elf");
    fs::write(&asm_path, ASSEMBLY).unwrap_or_else(|error| panic!("could not write assembly: {error}"));

    let clang = ndk.join("aarch64-linux-android26-clang");
    let status = Command::new(&clang)
        .args(["-march=armv8.8-a", "-nostdlib", "-static"])
        .args(["-Wl,--build-id=none", "-Wl,--emit-relocs", "-Wl,-e,entry"])
        .arg(&asm_path)
        .arg("-o")
        .arg(&elf_path)
        .env("TMPDIR", scratch)
        .status()
        .unwrap_or_else(|error| panic!("could not run {}: {error}", clang.display()));
    assert!(status.success(), "assembler failed: {status}");

    let nm = Command::new(ndk.join("llvm-nm"))
        .arg(&elf_path)
        .output()
        .unwrap_or_else(|error| panic!("could not run llvm-nm: {error}"));
    assert!(nm.status.success(), "llvm-nm failed");
    let nm_text = String::from_utf8(nm.stdout)
        .unwrap_or_else(|error| panic!("llvm-nm output: {error}"));
    let mut symbols = std::collections::HashMap::new();
    for line in nm_text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() == 3 {
            symbols.insert(
                fields[2].to_string(),
                i64::from_str_radix(fields[0], 16)
                    .unwrap_or_else(|error| panic!("invalid nm address: {error}")),
            );
        }
    }

    let disassembly = output_text(
        ndk.join("llvm-objdump"),
        &["-d", "--mattr=+hbc,+pauth"],
        &elf_path,
    );
    check(
        passed,
        "independent assembler and disassembler produce branch fixture",
        ["bl\t", "cbz\t", "tbz\t", "ret"]
            .iter()
            .all(|text| disassembly.contains(text)),
    );

    // The leg's event root is its scratch directory. An empty root makes
    // approval refuse to bind the inspect worker, so the ticket check fails
    // before any later comparator step.
    let event = |controller: &mut twin::Controller, state: Option<&Value>, action: &str, inputs: Value| {
        controller.event(state, action, inputs, root)
    };
    let mut d = event(current, None, "resume", Value::Obj(vec![]));
    d = event(current, d.get("state"), "investigation", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        "inspect_file",
        obj(vec![("path".into(), s("original.elf"))]),
    );
    check(
        passed,
        "native inspection enters existing review",
        d.get("state").and_then(|state| state.get("dialog")) == Some(&s("approval")),
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let effect = d
        .get("effect")
        .unwrap_or_else(|| panic!("missing approval effect"))
        .clone();
    check(
        passed,
        "review creates owned shared-engine worker",
        effect.get("kind") == Some(&s("tool"))
            && effect.get("tool") == Some(&s("inspect"))
            && effect.get("tool_ticket").is_some_and(twin::truthy),
    );
    let result = tool(current, &effect, root);
    let payload = Value::parse(&result);
    let graph = payload
        .get("snapshots")
        .and_then(|value| array_index(value, 0))
        .unwrap_or_else(|| panic!("missing snapshot"))
        .clone();
    let flow = graph
        .get("control_flow")
        .unwrap_or_else(|| panic!("missing control flow"))
        .clone();
    let rows = flow
        .get("relations")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| panic!("missing relations"))
        .clone();
    let nodes = graph
        .get("nodes")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| panic!("missing nodes"))
        .clone();
    let by_address: std::collections::HashMap<i64, i64> = nodes
        .iter()
        .skip(1)
        .map(|node| {
            (
                number(node.get("encoding").and_then(|value| value.get("address"))),
                ident(node.get("id")),
            )
        })
        .collect();

    check(
        passed,
        "inspection retains typed source-bound control-flow",
        flow.get("kind") == Some(&s("encoded_control_flow"))
            && flow.get("coverage") == Some(&s("Partial"))
            && !rows.is_empty(),
    );

    let consistent_node = node_at(&nodes, &by_address, symbols["consistent_edge"]);
    let consistent = rows
        .iter()
        .find(|row| {
            same_id(row.get("source"), ident(consistent_node.get("id")))
                && row.get("relation") == Some(&s("encoded_conditional_target"))
        })
        .unwrap_or_else(|| panic!("missing conditional target relation"))
        .clone();
    check(
        passed,
        "independent assembler recognizes BC.cond encoding",
        disassembly.contains("bc.eq")
            && consistent_node
                .get("encoding")
                .and_then(|value| value.get("mnemonic"))
                == Some(&s("BC.cond")),
    );
    check(
        passed,
        "consistent branch retains exact target and feature hint",
        number(consistent.get("target_address")) == symbols["target"]
            && consistent
                .get("operand")
                .and_then(|value| value.get("consistency_hint"))
                == Some(&twin::b(true))
            && consistent
                .get("operand")
                .and_then(|value| value.get("required_feature"))
                == Some(&s("FEAT_HBC"))
            && number(
                consistent
                    .get("operand")
                    .and_then(|value| value.get("condition"))
            ) == 0
            && number(graph.get("decoder_version")) == 3,
    );
    check(
        passed,
        "consistent branch retains a separate conditional continuation",
        rows.iter().any(|row| {
            same_id(row.get("source"), ident(consistent_node.get("id")))
                && row.get("relation") == Some(&s("candidate_not_taken_address"))
                && number(row.get("target_address")) == symbols["consistent_edge"] + 4
        }),
    );

    let mut auth_rows = std::collections::HashMap::new();
    let auth_specs = [
        ("braa", 0, 3, 4, "A"),
        ("braaz", 0, 5, -1, "A"),
        ("brab", 0, 6, 31, "B"),
        ("brabz", 0, 7, -1, "B"),
        ("blraa", 1, 8, 9, "A"),
        ("blraaz", 1, 10, -1, "A"),
        ("blrab", 1, 11, 31, "B"),
        ("blrabz", 1, 12, -1, "B"),
        ("retaa", 2, 30, 31, "A"),
        ("retab", 2, 30, 31, "B"),
    ];
    for (name, kind, register, modifier, key) in auth_specs {
        let node = node_at(&nodes, &by_address, symbols[&format!("auth_{name}")]);
        let relation = [
            "encoded_authenticated_indirect_branch",
            "encoded_authenticated_indirect_call",
            "encoded_authenticated_indirect_return",
        ][kind];
        let row = rows
            .iter()
            .find(|row| {
                same_id(row.get("source"), ident(node.get("id")))
                    && row.get("relation") == Some(&s(relation))
            })
            .unwrap_or_else(|| panic!("missing authenticated relation {name}"))
            .clone();
        let operand = row
            .get("operand")
            .unwrap_or_else(|| panic!("missing operand"))
            .clone();
        auth_rows.insert(name.to_string(), row.clone());
        check(
            passed,
            &format!("independent {name} retains exact authenticated operands"),
            disassembly.contains(name)
                && node
                    .get("encoding")
                    .and_then(|value| value.get("mnemonic"))
                    == Some(&s(&name.to_uppercase()))
                && number(operand.get("register")) == register
                && if modifier < 0 {
                    operand.get("modifier_register") == Some(&Value::Null)
                } else {
                    number(operand.get("modifier_register")) == modifier
                }
                && operand.get("authentication_key") == Some(&s(key))
                && operand.get("required_feature") == Some(&s("FEAT_PAuth"))
                && row.get("target_address") == Some(&Value::Null)
                && row.get("target_node") == Some(&Value::Null),
        );
        let modifier_kind = if modifier < 0 {
            "zero"
        } else if modifier == 31 {
            "SP"
        } else {
            "register"
        };
        let node_id = ident(node.get("id"));
        let has_return_link = rows.iter().any(|candidate| {
            same_id(candidate.get("source"), node_id)
                && candidate.get("relation") == Some(&s("encoded_link_return_address"))
                && number(candidate.get("target_address"))
                    == symbols[&format!("auth_{name}")] + 4
        });
        check(
            passed,
            &format!("{name} retains modifier and encoded link semantics"),
            operand.get("modifier_kind") == Some(&s(modifier_kind))
                && has_return_link == (kind == 1),
        );
    }
    check(
        passed,
        "authenticated return retains the conditional second-modifier context",
        ["retaa", "retab"].iter().all(|name| {
            let operand = auth_rows[*name].get("operand").unwrap();
            operand
                .get("second_modifier")
                .and_then(|value| value.get("register"))
                == Some(&twin::n("16"))
                && operand
                    .get("second_modifier")
                    .and_then(|value| value.get("condition"))
                    == Some(&s("FEAT_PAuth_LR and PSTATE.PACM = 1"))
        }),
    );

    let declarations = graph
        .get("declarations")
        .unwrap_or_else(|| panic!("missing declarations"))
        .clone();
    check(
        passed,
        "typed declaration inventory retains original parser facts",
        declarations.get("kind") == Some(&s("binary_declarations"))
            && declarations.get("mapping_inventory_complete") == Some(&twin::b(true)),
    );

    let headers = output_text(ndk.join("llvm-readobj"), &["--program-headers"], &elf_path);
    let expected: Vec<(i64, i64, i64, i64)> = parse_load_headers(&headers);
    assert!(!expected.is_empty(), "independent program headers were not found");
    let mapping_rows = declarations
        .get("mappings")
        .and_then(|value| value.get("rows"))
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| panic!("missing mapping rows"));
    let actual: Vec<(i64, i64, i64, i64)> = mapping_rows
        .iter()
        .map(|row| {
            (
                number(row.get("file_offset")),
                number(row.get("address")),
                number(row.get("file_size")),
                number(row.get("memory_size")),
            )
        })
        .collect();
    check(
        passed,
        "LOAD declarations match independent program headers",
        actual == expected,
    );

    let classified = declarations
        .get("classifications")
        .and_then(|value| value.get("rows"))
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| panic!("missing classifications"));
    check(
        passed,
        "mapping-symbol data interval remains explicit",
        classified.iter().any(|row| {
            !row.get("code_declaration").is_some_and(twin::truthy)
                && number(row.get("start")) <= symbols["literal"]
                && symbols["literal"] < number(row.get("end"))
                && row.get("excludes_whole_data_words") == Some(&twin::b(true))
        }),
    );
    check(
        passed,
        "declared data is excluded from encoding nodes",
        !by_address.contains_key(&symbols["literal"])
            && by_address.contains_key(&symbols["resume_code"]),
    );

    let reference = output_text(ndk.join("llvm-readobj"), &["--relocations"], &elf_path);
    let relocations = declarations
        .get("relocations")
        .and_then(|value| value.get("rows"))
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| panic!("missing relocations"));
    let call_decl = relocations
        .iter()
        .find(|row| row.get("name") == Some(&s("CALL26")))
        .unwrap_or_else(|| panic!("missing CALL26 declaration"))
        .clone();
    check(
        passed,
        "actual relocation declaration matches independent reference",
        number(call_decl.get("code")) == 283
            && number(call_decl.get("position")) == symbols["entry"]
            && reference.contains("R_AARCH64_CALL26"),
    );
    check(
        passed,
        "relocation is a declaration without applied binding",
        call_decl.get("addend_kind") == Some(&s("explicit signed declaration"))
            && declarations
                .get("meaning")
                .is_some_and(|value| value.str_().contains("Declarations from the held source")),
    );

    let call = rows
        .iter()
        .find(|row| {
            same_id(row.get("source"), by_address[&symbols["entry"]])
                && row.get("relation") == Some(&s("encoded_call_target"))
        })
        .unwrap_or_else(|| panic!("missing call target"))
        .clone();
    check(
        passed,
        "BL target matches independently linked symbol",
        number(call.get("target_address")) == symbols["target"]
            && same_id(call.get("target_node"), by_address[&symbols["target"]]),
    );
    let link = rows
        .iter()
        .find(|row| {
            row.get("source") == call.get("source")
                && row.get("relation") == Some(&s("encoded_link_return_address"))
        })
        .unwrap_or_else(|| panic!("missing return-address link"))
        .clone();
    check(
        passed,
        "link address kept separate from call target",
        number(link.get("target_address")) == symbols["entry"] + 4
            && number(link.get("target_address")) != number(call.get("target_address")),
    );
    let branch = rows
        .iter()
        .find(|row| same_id(row.get("source"), by_address[&(symbols["entry"] + 4)]))
        .unwrap_or_else(|| panic!("missing branch relation"))
        .clone();
    check(
        passed,
        "B target matches independently linked symbol",
        number(branch.get("target_address")) == symbols["finish"],
    );
    let compare = rows
        .iter()
        .find(|row| {
            same_id(row.get("source"), by_address[&symbols["compare"]])
                && row.get("relation") == Some(&s("encoded_compare_target"))
        })
        .unwrap_or_else(|| panic!("missing compare target"))
        .clone();
    check(
        passed,
        "CBZ target and register retained",
        number(compare.get("target_address")) == symbols["target"]
            && number(
                compare
                    .get("operand")
                    .and_then(|value| value.get("register"))
            ) == 0,
    );
    let ret = rows
        .iter()
        .find(|row| row.get("relation") == Some(&s("encoded_indirect_return")))
        .unwrap_or_else(|| panic!("missing indirect return"))
        .clone();
    check(
        passed,
        "return target stays unresolved",
        ret.get("target_address") == Some(&Value::Null)
            && ret.get("target_node") == Some(&Value::Null)
            && number(
                ret.get("operand")
                    .and_then(|value| value.get("register"))
            ) == 30,
    );
    check(
        passed,
        "data has no execution/reachability claim",
        flow.get("meaning")
            .is_some_and(|value| value.str_().contains("execution"))
            && flow
                .get("meaning")
                .is_some_and(|value| value.str_().contains("unresolved")),
    );

    let path_node = node_at(&nodes, &by_address, symbols["arithmetic_target"]);
    let path_text = path_node.get("content").unwrap().str_().to_string();
    check(
        passed,
        "actual ELF retains separate predecessor alternatives",
        path_text.contains("X5 <- unknown")
            && path_text.contains("encoded integer 0x2a")
            && path_text.contains("encoded integer 0x64")
            && path_text.contains("Other incoming entries remain unknown"),
    );
    check(
        passed,
        "captured predecessor join retains common bits separately",
        path_text.contains("Captured-entry join")
            && path_text.contains("X4 <- partial integer bits0x21, known mask0xffffffffffffffb5")
            && path_text.contains("across 2 distinct predecessor edges"),
    );
    check(
        passed,
        "captured join preserves the unknown external-entry interpretation",
        path_text.contains("X5 <- unknown")
            && path_text.contains("External entries keep the original unknown-entry interpretation."),
    );
    check(
        passed,
        "predecessor identity matches independent assembled edge",
        path_text.contains(&format!("edge at ELF declared VA 0x{:x}", symbols["path_edge"]))
            || path_text.contains(&format!("edge at declared VA 0x{:x}", symbols["path_edge"])),
    );
    let representation = path_node.get("representation").unwrap();
    check(
        passed,
        "path representation authenticates exact retained text",
        number(representation.get("bytes")) == path_text.as_bytes().len() as i64
            && representation.get("sha256")
                == Some(&s(&twin::sha256_bytes(path_text.as_bytes()))),
    );

    let knowledge_node = node_at(&nodes, &by_address, symbols["knowledge_mask"]);
    check(
        passed,
        "known mask and variable shift retain the original encoded constant",
        knowledge_node
            .get("encoding")
            .and_then(|value| value.get("mnemonic"))
            == Some(&s("AND"))
            && knowledge_node
                .get("content")
                .unwrap()
                .str_()
                .contains("X18 <- encoded integer 0x1234"),
    );
    check(
        passed,
        "knowledge source is independently assembled and disassembled",
        disassembly.contains("movk")
            && disassembly.contains("lsr")
            && disassembly.contains("and"),
    );

    let join_mask = node_at(&nodes, &by_address, symbols["join_mask"]);
    let join_node = node_at(&nodes, &by_address, symbols["join_follow"]);
    let join_text = join_node.get("content").unwrap().str_().to_string();
    check(
        passed,
        "common captured-entry bits feed the following mask",
        join_mask
            .get("content")
            .unwrap()
            .str_()
            .contains("Captured-entry interpretation")
            && join_mask
                .get("content")
                .unwrap()
                .str_()
                .contains("X6 <- encoded integer 0x1"),
    );
    check(
        passed,
        "common captured-entry result feeds successor arithmetic",
        join_text.contains("Captured-entry interpretation")
            && join_text.contains("under its 2 captured incoming edges")
            && join_text.contains("X7 <- encoded integer 0x2")
            && join_text
                .split_once('\n')
                .map(|(head, _)| head)
                .unwrap_or(join_text.as_str())
                .contains("X7 <- partial integer bits0x0, known mask0xfffffffffffffffc"),
    );

    let arithmetic_node = node_at(&nodes, &by_address, symbols["arithmetic_bits_mask"]);
    let selection_node = node_at(&nodes, &by_address, symbols["selection_edge"]);
    let selection_result = node_at(&nodes, &by_address, symbols["selection_result"]);
    let selection_text = selection_result.get("content").unwrap().str_().to_string();
    check(
        passed,
        "partial addition retains a constant through the following mask",
        arithmetic_node
            .get("content")
            .unwrap()
            .str_()
            .contains("X26 <- encoded integer 0x12340000"),
    );
    check(
        passed,
        "conditional selection retains common bits through following arithmetic",
        selection_node
            .get("content")
            .unwrap()
            .str_()
            .contains("X21 <- partial integer bits0x29, known mask0xfffffffffffffffd")
            && selection_text.contains("X23 <- encoded integer 0x2a"),
    );
    check(
        passed,
        "independent assembler supplies the arithmetic and select encodings",
        disassembly.contains("csel")
            && arithmetic_node
                .get("encoding")
                .and_then(|value| value.get("mnemonic"))
                == Some(&s("AND"))
            && selection_node
                .get("encoding")
                .and_then(|value| value.get("mnemonic"))
                == Some(&s("CSEL"))
            && selection_result
                .get("encoding")
                .and_then(|value| value.get("mnemonic"))
                == Some(&s("ADD")),
    );

    let mut inputs = tool_inputs(&effect);
    if let Value::Obj(fields) = &mut inputs {
        fields.push(("purpose".into(), s("local_tool")));
        fields.push(("output".into(), s(&result)));
    }
    d = event(current, d.get("state"), "tool_result", inputs);
    let record = d
        .get("state")
        .and_then(|state| state.get("investigation_records"))
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .and_then(|items| items.last())
        .unwrap_or_else(|| panic!("missing investigation record"))
        .clone();
    check(
        passed,
        "real worker receipt retains graph",
        record
            .get("snapshots")
            .and_then(|value| array_index(value, 0))
            .and_then(|snapshot| snapshot.get("control_flow"))
            == Some(&flow),
    );

    d = event(
        current,
        d.get("state"),
        &format!("investigation_record:{}", record.get("id").unwrap().str_()),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "investigation_graph", Value::Obj(vec![]));
    let text = collect_fields(d.get("ui").unwrap());
    check(
        passed,
        "native graph page displays relationship meaning",
        text.contains("Encoded control-flow relationships")
            && text.contains("encoded_call_target"),
    );
    check(
        passed,
        "native view includes typed declaration summaries",
        text.contains("Typed binary declarations") && text.contains("relocations:"),
    );

    d = event(
        current,
        d.get("state"),
        &format!("investigation_graph_node:{}", ident(call.get("source"))),
        Value::Obj(vec![]),
    );
    let text = collect_fields(d.get("ui").unwrap());
    check(
        passed,
        "native node view contains its outgoing target",
        text.contains("encoded_call_target") && text.contains(&symbols["target"].to_string()),
    );
    d = event(
        current,
        d.get("state"),
        &format!("investigation_graph_node:{}", ident(knowledge_node.get("id"))),
        Value::Obj(vec![]),
    );
    check(
        passed,
        "native node view retains derived known bits",
        collect_fields(d.get("ui").unwrap()).contains("X18 <- encoded integer 0x1234"),
    );
    d = event(
        current,
        d.get("state"),
        &format!("investigation_graph_node:{}", ident(join_node.get("id"))),
        Value::Obj(vec![]),
    );
    check(
        passed,
        "native node query retains the common-entry successor result",
        collect_fields(d.get("ui").unwrap()).contains("Captured-entry interpretation")
            && collect_fields(d.get("ui").unwrap()).contains("X7 <- encoded integer 0x2"),
    );
    d = event(
        current,
        d.get("state"),
        &format!("investigation_graph_node:{}", ident(arithmetic_node.get("id"))),
        Value::Obj(vec![]),
    );
    check(
        passed,
        "UI description retains partial-arithmetic derived constant",
        collect_fields(d.get("ui").unwrap()).contains("X26 <- encoded integer 0x12340000"),
    );
    d = event(
        current,
        d.get("state"),
        &format!("investigation_graph_node:{}", ident(selection_result.get("id"))),
        Value::Obj(vec![]),
    );
    check(
        passed,
        "UI description retains conditional-selection derived constant",
        collect_fields(d.get("ui").unwrap()).contains("X23 <- encoded integer 0x2a"),
    );
    d = event(
        current,
        d.get("state"),
        &format!(
            "investigation_graph_node:{}",
            ident(auth_rows["braa"].get("source"))
        ),
        Value::Obj(vec![]),
    );
    let text = collect_fields(d.get("ui").unwrap());
    check(
        passed,
        "native authenticated node exposes encoded key and modifier",
        text.contains("encoded_authenticated_indirect_branch")
            && text.contains("FEAT_PAuth")
            && text.contains("authentication_key"),
    );
    d = event(
        current,
        d.get("state"),
        &format!("investigation_graph_node:{}", ident(path_node.get("id"))),
        Value::Obj(vec![]),
    );
    let text = collect_fields(d.get("ui").unwrap());
    check(
        passed,
        "native encoding view includes conditional facts",
        text.contains("encoded integer 0x2a")
            && text.contains("Other incoming entries remain unknown"),
    );
    check(
        passed,
        "native node view retains captured-entry common bits",
        text.contains("Captured-entry join")
            && text.contains("known mask0xffffffffffffffb5"),
    );

    let mut controlled_state = d.get("state").unwrap().clone();
    assign_field(
        &mut controlled_state,
        "profiles",
        Value::Arr(vec![controlled_profile()]),
    );
    d = event(
        current,
        Some(&controlled_state),
        "send",
        obj(vec![("draft".into(), s("Query the saved branch observation."))]),
    );
    assert_eq!(
        d.get("effect").and_then(|value| value.get("purpose")),
        Some(&s("model")),
        "model send did not enter model authority"
    );

    d = model_reply(
        current,
        &d,
        &observation_query(record.get("id").unwrap().str_(), ident(call.get("source"))),
        root,
    );
    check(
        passed,
        "ordinary model query uses existing review",
        d.get("state").and_then(|state| state.get("dialog")) == Some(&s("approval")),
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let wire = d
        .get("effect")
        .and_then(|value| value.get("wire"))
        .unwrap()
        .str_()
        .to_string();
    check(
        passed,
        "ordinary model receives actual typed relationship",
        wire.contains("encoded_call_target") && wire.contains(&symbols["target"].to_string()),
    );
    check(
        passed,
        "ordinary model receives actual typed declarations",
        wire.contains("binary_declarations") && wire.contains("CALL26"),
    );

    d = model_reply(
        current,
        &d,
        &observation_query(record.get("id").unwrap().str_(), ident(path_node.get("id"))),
        root,
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let effect_text = d.get("effect").unwrap().to_string();
    let wire = d.get("effect").and_then(|value| value.get("wire")).unwrap().str_();
    check(
        passed,
        "ordinary model query receives captured-entry common facts",
        effect_text.contains("Captured-entry join")
            && effect_text.contains("known mask0xffffffffffffffb5"),
    );
    check(
        passed,
        "ordinary model query receives actual predecessor facts",
        wire.contains("encoded integer 0x2a")
            && wire.contains("Other incoming entries remain unknown"),
    );

    d = model_reply(
        current,
        &d,
        &observation_query(
            record.get("id").unwrap().str_(),
            ident(auth_rows["blraa"].get("source")),
        ),
        root,
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let effect_text = d.get("effect").unwrap().to_string();
    check(
        passed,
        "ordinary model query receives actual authenticated call operands",
        effect_text.contains("encoded_authenticated_indirect_call")
            && effect_text.contains("FEAT_PAuth")
            && effect_text.contains("authentication_key"),
    );

    d = model_reply(
        current,
        &d,
        &observation_query(record.get("id").unwrap().str_(), ident(knowledge_node.get("id"))),
        root,
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    check(
        passed,
        "ordinary model query receives actual known-bit result",
        d.get("effect").unwrap().to_string().contains("X18 <- encoded integer 0x1234"),
    );

    d = model_reply(
        current,
        &d,
        &observation_query(record.get("id").unwrap().str_(), ident(join_node.get("id"))),
        root,
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let wire = d
        .get("effect")
        .and_then(|value| value.get("wire"))
        .unwrap()
        .str_()
        .to_string();
    check(
        passed,
        "ordinary model query receives common-entry successor facts",
        wire.contains("Captured-entry interpretation")
            && wire.contains("X7 <- encoded integer 0x2"),
    );

    d = model_reply(
        current,
        &d,
        &observation_query(record.get("id").unwrap().str_(), ident(arithmetic_node.get("id"))),
        root,
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    check(
        passed,
        "ordinary model query receives partial-arithmetic findings",
        d.get("effect")
            .and_then(|value| value.get("wire"))
            .unwrap()
            .str_()
            .contains("X26 <- encoded integer 0x12340000"),
    );

    d = model_reply(
        current,
        &d,
        &observation_query(
            record.get("id").unwrap().str_(),
            ident(selection_result.get("id")),
        ),
        root,
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    check(
        passed,
        "ordinary model query receives conditional-selection findings",
        d.get("effect")
            .and_then(|value| value.get("wire"))
            .unwrap()
            .str_()
            .contains("X23 <- encoded integer 0x2a"),
    );

    d = model_reply(
        current,
        &d,
        "Controlled continuation after actual retained observation.",
        root,
    );
    check(
        passed,
        "model query settles through existing continuation",
        !d.get("state")
            .and_then(|state| state.get("busy"))
            .is_some_and(twin::truthy),
    );

    d = event(
        current,
        d.get("state"),
        &format!("investigation_record:{}", record.get("id").unwrap().str_()),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "investigation_graph", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        "investigation_graph_node:0",
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "reconstruction_new", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(record.get("id").unwrap().str_())),
            ("snapshot".into(), s("0")),
            ("node".into(), s("0")),
            ("mode".into(), s("specification")),
            ("output".into(), s("structure.json")),
        ]),
    );
    check(
        passed,
        "structural export enters review",
        d.get("state").and_then(|state| state.get("dialog")) == Some(&s("approval")),
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let export_effect = d.get("effect").unwrap().clone();
    check(
        passed,
        "structural export shares existing authority",
        export_effect.get("tool") == Some(&s("reconstruct"))
            && export_effect
                .get("args")
                .and_then(|value| value.get("path"))
                == Some(&s("original.elf")),
    );
    let export_output = tool(current, &export_effect, root);
    let export_text = fs::read_to_string(scratch.join("structure.json")).unwrap();
    let export = Value::parse(&export_text);
    let export_flow = export
        .get("source")
        .and_then(|value| value.get("structure"))
        .and_then(|value| value.get("control_flow"))
        .unwrap();
    check(
        passed,
        "actual structural export retains typed relationships",
        export_flow.get("relations") == Some(&sorted_rows(&rows)),
    );
    check(
        passed,
        "actual export retains typed declaration inventory",
        export
            .get("source")
            .and_then(|value| value.get("structure"))
            .and_then(|value| value.get("declarations"))
            == Some(&declarations),
    );
    check(
        passed,
        "export labels semantics unresolved",
        export
            .get("content")
            .unwrap()
            .str_()
            .contains("Unresolved")
            && export_output.contains("read back exactly"),
    );

    d = event(
        current,
        d.get("state"),
        "tool_result",
        export_tool_inputs(&export_effect, &export_output),
    );
    d = event(
        current,
        d.get("state"),
        &format!("investigation_record:{}", record.get("id").unwrap().str_()),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "investigation_graph", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        &format!(
            "investigation_graph_node:{}",
            ident(path_node.get("id"))
        ),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "reconstruction_new", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(record.get("id").unwrap().str_())),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&ident(path_node.get("id")).to_string())),
            ("mode".into(), s("representation")),
            ("output".into(), s("conditional.txt")),
        ]),
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let conditional_effect = d.get("effect").unwrap().clone();
    let conditional_output = tool(current, &conditional_effect, root);
    check(
        passed,
        "actual conditional representation exports exact authenticated bytes",
        fs::read(scratch.join("conditional.txt")).unwrap() == path_text.as_bytes()
            && conditional_output.contains("read back exactly"),
    );

    d = event(
        current,
        d.get("state"),
        "tool_result",
        export_tool_inputs(&conditional_effect, &conditional_output),
    );
    d = event(
        current,
        d.get("state"),
        &format!("investigation_record:{}", record.get("id").unwrap().str_()),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "investigation_graph", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        &format!(
            "investigation_graph_node:{}",
            ident(join_node.get("id"))
        ),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "reconstruction_new", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(record.get("id").unwrap().str_())),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&ident(join_node.get("id")).to_string())),
            ("mode".into(), s("representation")),
            ("output".into(), s("common-entry.txt")),
        ]),
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let common_effect = d.get("effect").unwrap().clone();
    let common_output = tool(current, &common_effect, root);
    check(
        passed,
        "common-entry representation exports its exact retained bytes",
        fs::read(scratch.join("common-entry.txt")).unwrap() == join_text.as_bytes()
            && common_output.contains("read back exactly"),
    );
    d = event(
        current,
        d.get("state"),
        "tool_result",
        export_tool_inputs(&common_effect, &common_output),
    );

    d = event(
        current,
        d.get("state"),
        &format!("investigation_record:{}", record.get("id").unwrap().str_()),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "investigation_graph", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        &format!(
            "investigation_graph_node:{}",
            ident(selection_result.get("id"))
        ),
        Value::Obj(vec![]),
    );
    d = event(current, d.get("state"), "reconstruction_new", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        "reconstruction_prepare",
        obj(vec![
            ("record".into(), s(record.get("id").unwrap().str_())),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&ident(selection_result.get("id")).to_string())),
            ("mode".into(), s("representation")),
            ("output".into(), s("selected-value.txt")),
        ]),
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let selected_effect = d.get("effect").unwrap().clone();
    let selected_output = tool(current, &selected_effect, root);
    check(
        passed,
        "conditional-selection representation exports exact retained bytes",
        fs::read(scratch.join("selected-value.txt")).unwrap() == selection_text.as_bytes()
            && selected_output.contains("read back exactly"),
    );
    d = event(
        current,
        d.get("state"),
        "tool_result",
        export_tool_inputs(&selected_effect, &selected_output),
    );

    let coff = scratch.join("original-coff.S");
    fs::write(
        &coff,
        ASSEMBLY
            .replace(".type entry,%function\n", "")
            .replace(".size entry,.-entry\n", ""),
    )
    .unwrap();
    let obj_path = scratch.join("original.obj");
    let pe_path = scratch.join("original.exe");
    run_tool(
        ndk.join("clang"),
        &[
            "--target=aarch64-pc-windows-msvc",
            "-march=armv8.8-a",
            "-c",
        ],
        &[&coff, Path::new("-o"), &obj_path],
        scratch,
    );
    run_tool(
        ndk.join("lld-link"),
        &[],
        &[
            Path::new("/entry:entry"),
            Path::new("/subsystem:console"),
            Path::new("/nodefaultlib"),
            Path::new("/machine:arm64"),
            Path::new(&format!("/out:{}", pe_path.display())),
            &obj_path,
        ],
        scratch,
    );
    let pe_headers = output_text(ndk.join("llvm-readobj"), &["--file-headers"], &pe_path);
    let base = parse_i64_hex_field(&pe_headers, "ImageBase: ").unwrap();
    let pe_reference = output_text(ndk.join("llvm-objdump"), &["-d"], &pe_path);
    let jump = parse_pe_jump(&pe_reference)
        .unwrap_or_else(|| panic!("independent PE call was not found"));
    d = event(current, d.get("state"), "investigation", Value::Obj(vec![]));
    d = event(
        current,
        d.get("state"),
        "inspect_file",
        obj(vec![("path".into(), s("original.exe"))]),
    );
    d = event(current, d.get("state"), "approve", Value::Obj(vec![]));
    let pe_effect = d.get("effect").unwrap().clone();
    let pe_actual = tool(current, &pe_effect, root);
    let pe_payload = Value::parse(&pe_actual);
    let pe_graph = pe_payload
        .get("snapshots")
        .and_then(|value| array_index(value, 0))
        .unwrap_or_else(|| panic!("missing snapshot in {}", pe_payload.to_string()))
        .clone();
    let pe_flow = pe_graph.get("control_flow").unwrap().clone();
    let pe_call = pe_flow
        .get("relations")
        .and_then(|value| value.as_array())
        .unwrap()
        .iter()
        .find(|row| row.get("relation") == Some(&s("encoded_call_target")))
        .unwrap()
        .clone();
    check(
        passed,
        "real pure Arm64 PE retains relative coordinates",
        pe_flow.get("coordinate_kind") == Some(&s("PE RVA"))
            && number(pe_call.get("target_address")) == jump.1 - base
            && number(pe_call.get("target_address")) < base,
    );
    let pe_nodes = pe_graph
        .get("nodes")
        .and_then(|value| value.as_array())
        .unwrap();
    let source_index = number(pe_call.get("source")) as usize;
    check(
        passed,
        "PE call source matches independent disassembly",
        number(
            pe_nodes[source_index]
                .get("encoding")
                .and_then(|value| value.get("address"))
        ) == jump.0 - base,
    );
    d = event(
        current,
        d.get("state"),
        "tool_result",
        export_tool_inputs(&pe_effect, &pe_actual),
    );
    check(
        passed,
        "PE actual callback retains typed flow",
        d.get("state")
            .and_then(|state| state.get("investigation_records"))
            .and_then(|value| value.get("items"))
            .and_then(|value| value.as_array())
            .and_then(|items| items.last())
            .and_then(|item| item.get("snapshots"))
            .and_then(|value| array_index(value, 0))
            .and_then(|snapshot| snapshot.get("control_flow"))
            == Some(&pe_flow),
    );
    let pe_declarations = pe_graph.get("declarations").unwrap();
    check(
        passed,
        "PE classification stays unknown and declared relocation absence is explicit",
        pe_declarations.get("classification_inventory_complete") == Some(&Value::Null)
            && pe_declarations.get("relocation_inventory_complete") == Some(&twin::b(true))
            && number(
                pe_declarations
                    .get("relocations")
                    .and_then(|value| value.get("observed"))
            ) == 0
            && number(
                pe_declarations
                    .get("mappings")
                    .and_then(|value| value.get("retained"))
            ) > 0,
    );

    let mut tampered = d.get("state").unwrap().clone();
    mutate_record_relation(&mut tampered, record.get("id").unwrap().str_(), 4);
    let mut rejected = event(
        current,
        Some(&tampered),
        &format!("investigation_record:{}", record.get("id").unwrap().str_()),
        Value::Obj(vec![]),
    );
    rejected = event(
        current,
        rejected.get("state"),
        "investigation_graph",
        Value::Obj(vec![]),
    );
    check(
        passed,
        "changed flow payload never becomes accepted native data",
        rejected
            .get("state")
            .and_then(|state| state.get("notice_error"))
            .is_some_and(twin::truthy)
            && rejected
                .get("state")
                .and_then(|state| state.get("notice"))
                .is_some_and(|value| value.str_().contains("Malformed binary observation"))
            && rejected
                .get("state")
                .and_then(|state| state.get("dialog"))
                != Some(&s("investigation_graph")),
    );
    check(
        passed,
        "file engine still operates independently of historical payload",
        !current
            .op(&obj(vec![
                ("op".into(), s("tool")),
                ("tool".into(), s("inspect")),
                ("root".into(), s(root)),
                ("args".into(), obj(vec![("path".into(), s("original.S"))])),
            ]))
            .str_()
            .is_empty(),
    );

    let mut previous = twin::Controller::spawn_at(
        &args
            .previous_flow_controller
            .clone()
            .canonicalize()
            .unwrap_or_else(|_| args.previous_flow_controller.clone()),
        project.to_str().unwrap(),
    );
    let mut legacy = event(&mut previous, None, "resume", Value::Obj(vec![]));
    legacy = event(
        &mut previous,
        legacy.get("state"),
        "investigation",
        Value::Obj(vec![]),
    );
    legacy = event(
        &mut previous,
        legacy.get("state"),
        "inspect_file",
        obj(vec![("path".into(), s("original.elf"))]),
    );
    legacy = event(
        &mut previous,
        legacy.get("state"),
        "approve",
        Value::Obj(vec![]),
    );
    let legacy_effect = legacy.get("effect").unwrap().clone();
    let legacy_data = tool(&mut previous, &legacy_effect, root);
    let legacy_payload = Value::parse(&legacy_data);
    let legacy_graph = legacy_payload
        .get("snapshots")
        .and_then(|value| array_index(value, 0))
        .unwrap_or_else(|| panic!("missing snapshot in {}", legacy_payload.to_string()))
        .clone();
    let legacy_nodes = legacy_graph
        .get("nodes")
        .and_then(|value| value.as_array())
        .unwrap();
    check(
        passed,
        "actual previous controller retains its original decoder meaning",
        legacy_graph.get("decoder_version").is_none()
            && legacy_nodes
                .iter()
                .skip(1)
                .find(|node| {
                    number(
                        node.get("encoding")
                            .and_then(|value| value.get("address")),
                    ) == symbols["consistent_edge"]
                })
                .and_then(|node| {
                    node.get("encoding")
                        .and_then(|value| value.get("mnemonic"))
                })
                == Some(&s("unclassified")),
    );
    legacy = event(
        &mut previous,
        legacy.get("state"),
        "tool_result",
        export_tool_inputs(&legacy_effect, &legacy_data),
    );
    let old_record = legacy
        .get("state")
        .and_then(|state| state.get("investigation_records"))
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .and_then(|items| items.last())
        .unwrap()
        .clone();
    previous.finish();

    let mut state = d.get("state").unwrap().clone();
    append_record(&mut state, &old_record);
    let mut legacy = event(
        current,
        Some(&state),
        &format!("investigation_record:{}", old_record.get("id").unwrap().str_()),
        Value::Obj(vec![]),
    );
    legacy = event(
        current,
        legacy.get("state"),
        "investigation_graph",
        Value::Obj(vec![]),
    );
    check(
        passed,
        "current native graph accepts actual legacy observation bytes",
        legacy
            .get("state")
            .and_then(|state| state.get("dialog"))
            == Some(&s("investigation_graph"))
            && !legacy
                .get("state")
                .and_then(|state| state.get("notice_error"))
                .is_some_and(twin::truthy),
    );
    let old_node = old_record
        .get("snapshots")
        .and_then(|value| array_index(value, 0))
        .and_then(|value| value.get("nodes"))
        .and_then(|value| value.as_array())
        .unwrap()
        .iter()
        .skip(1)
        .find(|node| {
            number(
                node.get("encoding")
                    .and_then(|value| value.get("address")),
            ) == symbols["consistent_edge"]
        })
        .unwrap()
        .clone();
    legacy = event(
        current,
        legacy.get("state"),
        &format!("investigation_graph_node:{}", ident(old_node.get("id"))),
        Value::Obj(vec![]),
    );
    check(
        passed,
        "legacy unclassified instruction remains historical evidence",
        collect_fields(legacy.get("ui").unwrap()).contains("unclassified"),
    );

    let mut published = twin::Controller::spawn_at(
        &args
            .published_controller
            .clone()
            .canonicalize()
            .unwrap_or_else(|_| args.published_controller.clone()),
        project.to_str().unwrap(),
    );
    let mut legacy = event(&mut published, None, "resume", Value::Obj(vec![]));
    legacy = event(
        &mut published,
        legacy.get("state"),
        "investigation",
        Value::Obj(vec![]),
    );
    legacy = event(
        &mut published,
        legacy.get("state"),
        "inspect_file",
        obj(vec![("path".into(), s("original.elf"))]),
    );
    legacy = event(
        &mut published,
        legacy.get("state"),
        "approve",
        Value::Obj(vec![]),
    );
    let published_effect = legacy.get("effect").unwrap().clone();
    let published_data = tool(&mut published, &published_effect, root);
    legacy = event(
        &mut published,
        legacy.get("state"),
        "tool_result",
        export_tool_inputs(&published_effect, &published_data),
    );
    let published_record = legacy
        .get("state")
        .and_then(|state| state.get("investigation_records"))
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .and_then(|items| items.last())
        .unwrap()
        .clone();
    let bucket = legacy
        .get("state")
        .and_then(|state| state.get("investigation_records"))
        .unwrap()
        .clone();
    published.finish();

    let mut state = event(current, None, "resume", Value::Obj(vec![]))
        .get("state")
        .unwrap()
        .clone();
    assign_field(&mut state, "investigation_records", bucket);
    let mut legacy = event(
        current,
        Some(&state),
        &format!(
            "investigation_record:{}",
            published_record.get("id").unwrap().str_()
        ),
        Value::Obj(vec![]),
    );
    legacy = event(
        current,
        legacy.get("state"),
        "investigation_graph",
        Value::Obj(vec![]),
    );
    let saved_node = published_record
        .get("snapshots")
        .and_then(|value| array_index(value, 0))
        .and_then(|value| value.get("nodes"))
        .and_then(|value| value.as_array())
        .unwrap()
        .iter()
        .skip(1)
        .find(|node| {
            number(
                node.get("encoding")
                    .and_then(|value| value.get("address")),
            ) == symbols["arithmetic_bits_mask"]
        })
        .unwrap()
        .clone();
    legacy = event(
        current,
        legacy.get("state"),
        &format!(
            "investigation_graph_node:{}",
            ident(saved_node.get("id"))
        ),
        Value::Obj(vec![]),
    );
    check(
        passed,
        "current native view preserves actual published representation",
        !legacy
            .get("state")
            .and_then(|state| state.get("notice_error"))
            .is_some_and(twin::truthy)
            && collect_fields(legacy.get("ui").unwrap())
                .contains(saved_node.get("content").unwrap().str_().trim()),
    );

    let mut state = legacy.get("state").unwrap().clone();
    assign_field(&mut state, "profiles", Value::Arr(vec![controlled_profile()]));
    legacy = event(
        current,
        Some(&state),
        "send",
        obj(vec![(
            "draft".into(),
            s("Read the actual saved published representation."),
        )]),
    );
    legacy = model_reply(
        current,
        &legacy,
        &observation_query(
            published_record.get("id").unwrap().str_(),
            ident(saved_node.get("id")),
        ),
        root,
    );
    legacy = event(current, legacy.get("state"), "approve", Value::Obj(vec![]));
    let wire = legacy
        .get("effect")
        .and_then(|value| value.get("wire"))
        .unwrap()
        .str_()
        .to_string();
    let body = wire.split_once("\r\n\r\n").unwrap().1;
    let response = Value::parse(body);
    let content = response
        .get("messages")
        .and_then(|value| value.as_array())
        .and_then(|messages| messages.last())
        .and_then(|message| message.get("content"))
        .unwrap()
        .str_();
    let saved_text = saved_node.get("content").unwrap().str_();
    check(
        passed,
        "current model query preserves actual published representation",
        legacy
            .get("effect")
            .is_some_and(|value| !matches!(value, Value::Null))
            && content.contains(&json_string_body(saved_text, false)),
    );
    legacy = model_reply(
        current,
        &legacy,
        "Saved published representation received.",
        root,
    );

    legacy = event(
        current,
        legacy.get("state"),
        &format!(
            "investigation_record:{}",
            published_record.get("id").unwrap().str_()
        ),
        Value::Obj(vec![]),
    );
    legacy = event(
        current,
        legacy.get("state"),
        "investigation_graph",
        Value::Obj(vec![]),
    );
    legacy = event(
        current,
        legacy.get("state"),
        &format!(
            "investigation_graph_node:{}",
            ident(saved_node.get("id"))
        ),
        Value::Obj(vec![]),
    );
    legacy = event(
        current,
        legacy.get("state"),
        "reconstruction_new",
        Value::Obj(vec![]),
    );
    legacy = event(
        current,
        legacy.get("state"),
        "reconstruction_prepare",
        obj(vec![
            (
                "record".into(),
                s(published_record.get("id").unwrap().str_()),
            ),
            ("snapshot".into(), s("0")),
            ("node".into(), s(&ident(saved_node.get("id")).to_string())),
            ("mode".into(), s("representation")),
            ("output".into(), s("published-value.txt")),
        ]),
    );
    legacy = event(current, legacy.get("state"), "approve", Value::Obj(vec![]));
    let published_export_effect = legacy.get("effect").unwrap().clone();
    let published_export = tool(current, &published_export_effect, root);
    check(
        passed,
        "current export preserves exact actual published representation",
        fs::read(scratch.join("published-value.txt")).unwrap()
            == saved_node.get("content").unwrap().str_().as_bytes()
            && published_export.contains("read back exactly"),
    );

    if let Some(retained) = retained_auth.take() {
        if let Value::Arr(checks) = retained.checks.clone() {
            for check in checks {
                passed.push(check.str_().to_string());
            }
        }
    } else {
        if !args.previous_auth_controller.is_file() {
            let unavailable = [
                "actual revision-two controller retains original authenticated-word meaning",
                "current native graph accepts actual revision-two bytes",
                "revision-two authenticated word remains historical evidence",
            ];
            let body = obj(vec![
                (
                    "passed".into(),
                    Value::Arr(passed.iter().map(|name| s(name)).collect()),
                ),
                ("executed_checks".into(), twin::n(&passed.len().to_string())),
                ("retained_compatibility".into(), Value::Null),
                (
                    "unavailable".into(),
                    Value::Arr(unavailable.iter().map(|name| s(name)).collect()),
                ),
                (
                    "reason".into(),
                    s("Genuine revision-two comparator absent; reader fingerprint was not weakened or substituted."),
                ),
            ]);
            twin::report_value("binary_flow", &body, true);
            panic!("Genuine revision-two comparator required; earlier current checks were recorded separately");
        }

        let mut previous = twin::Controller::spawn_at(
            &args
                .previous_auth_controller
                .clone()
                .canonicalize()
                .unwrap_or_else(|_| args.previous_auth_controller.clone()),
            project.to_str().unwrap(),
        );
        let mut legacy = event(&mut previous, None, "resume", Value::Obj(vec![]));
        legacy = event(
            &mut previous,
            legacy.get("state"),
            "investigation",
            Value::Obj(vec![]),
        );
        legacy = event(
            &mut previous,
            legacy.get("state"),
            "inspect_file",
            obj(vec![("path".into(), s("original.elf"))]),
        );
        legacy = event(
            &mut previous,
            legacy.get("state"),
            "approve",
            Value::Obj(vec![]),
        );
        let old_effect = legacy.get("effect").unwrap().clone();
        let old_data = tool(&mut previous, &old_effect, root);
        let old_payload = Value::parse(&old_data);
        let old_graph = old_payload
            .get("snapshots")
            .and_then(|value| array_index(value, 0))
            .unwrap_or_else(|| panic!("missing snapshot in {}", old_payload.to_string()))
            .clone();
        let old_nodes = old_graph
            .get("nodes")
            .and_then(|value| value.as_array())
            .unwrap();
        check(
            passed,
            "actual revision-two controller retains original authenticated-word meaning",
            number(old_graph.get("decoder_version")) == 2
                && old_nodes
                    .iter()
                    .skip(1)
                    .find(|node| {
                        number(
                            node.get("encoding")
                                .and_then(|value| value.get("address")),
                        ) == symbols["auth_braa"]
                    })
                    .and_then(|node| {
                        node.get("encoding")
                            .and_then(|value| value.get("mnemonic"))
                    })
                    == Some(&s("unclassified")),
        );
        legacy = event(
            &mut previous,
            legacy.get("state"),
            "tool_result",
            export_tool_inputs(&old_effect, &old_data),
        );
        let old_record = legacy
            .get("state")
            .and_then(|state| state.get("investigation_records"))
            .and_then(|value| value.get("items"))
            .and_then(|value| value.as_array())
            .and_then(|items| items.last())
            .unwrap()
            .clone();
        previous.finish();

        let mut state = d.get("state").unwrap().clone();
        append_record(&mut state, &old_record);
        let mut legacy = event(
            current,
            Some(&state),
            &format!("investigation_record:{}", old_record.get("id").unwrap().str_()),
            Value::Obj(vec![]),
        );
        legacy = event(
            current,
            legacy.get("state"),
            "investigation_graph",
            Value::Obj(vec![]),
        );
        check(
            passed,
            "current native graph accepts actual revision-two bytes",
            legacy
                .get("state")
                .and_then(|state| state.get("dialog"))
                == Some(&s("investigation_graph"))
                && !legacy
                    .get("state")
                    .and_then(|state| state.get("notice_error"))
                    .is_some_and(twin::truthy),
        );
        let old_node = old_record
            .get("snapshots")
            .and_then(|value| array_index(value, 0))
            .and_then(|value| value.get("nodes"))
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .skip(1)
            .find(|node| {
                number(
                    node.get("encoding")
                        .and_then(|value| value.get("address")),
                ) == symbols["auth_braa"]
            })
            .unwrap()
            .clone();
        legacy = event(
            current,
            legacy.get("state"),
            &format!(
                "investigation_graph_node:{}",
                ident(old_node.get("id"))
            ),
            Value::Obj(vec![]),
        );
        check(
        passed,
        "revision-two authenticated word remains historical evidence",
        collect_fields(legacy.get("ui").unwrap()).contains("unclassified"),
    );
    }
}

fn tool(controller: &mut twin::Controller, effect: &Value, root: &str) -> String {
    let mut envelope = effect.clone();
    assign_field(&mut envelope, "op", s("tool"));
    assign_field(&mut envelope, "root", s(root));
    controller.op(&envelope).str_().to_string()
}

fn tool_inputs(effect: &Value) -> Value {
    let mut inputs = obj(vec![]);
    if let Value::Obj(fields) = effect {
        for key in ["tool_ticket", "tool_binding", "editor", "editor_dialog"] {
            if let Some(value) = fields.iter().find(|(name, _)| name == key) {
                if let Value::Obj(target) = &mut inputs {
                    target.push((key.to_string(), value.1.clone()));
                }
            }
        }
    }
    inputs
}

fn export_tool_inputs(effect: &Value, output: &str) -> Value {
    let mut inputs = tool_inputs(effect);
    if let Value::Obj(fields) = &mut inputs {
        fields.push(("purpose".into(), s("local_tool")));
        fields.push(("output".into(), s(output)));
    }
    inputs
}

fn model_reply(controller: &mut twin::Controller, d: &Value, text: &str, root: &str) -> Value {
    let body = format!(
        "{{\"choices\": [{{\"message\": {{\"content\": {}}}, \"finish_reason\": \"stop\"}}]}}",
        py_quote(text)
    );
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let encoded = twin::b64_encode(raw.as_bytes());
    controller.event(
        d.get("state"),
        "network_result",
        obj(vec![
            ("purpose".into(), s("model")),
            (
                "model_ticket".into(),
                d.get("state")
                    .and_then(|state| state.get("model_expected"))
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
            ("data".into(), s(&encoded)),
        ]),
        root,
    )
}

fn controlled_profile() -> Value {
    obj(vec![
        ("name".into(), s("Controlled")),
        ("endpoint".into(), s("http://model.test/v1/chat/completions")),
        ("protocol".into(), s("chat")),
        ("model".into(), s("controlled-model")),
        ("plain".into(), twin::b(true)),
        ("stream".into(), twin::b(false)),
    ])
}

fn observation_query(record_id: &str, node: i64) -> String {
    format!(
        "{{\"tool\": \"observations\", \"args\": {{\"id\": {}, \"snapshot\": 0, \"node\": {node}}}}}",
        py_quote(record_id)
    )
}

fn assign_field(object: &mut Value, key: &str, value: Value) {
    let Value::Obj(fields) = object else {
        panic!("expected object, got {}", object.to_string());
    };
    if let Some((_, existing)) = fields.iter_mut().find(|(name, _)| name == key) {
        *existing = value;
    } else {
        fields.push((key.to_string(), value));
    }
}

fn ident(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Num(text)) | Some(Value::Str(text)) => text
            .parse::<i64>()
            .unwrap_or_else(|error| panic!("invalid identifier {text}: {error}")),
        Some(other) => panic!("expected identifier, got {}", other.to_string()),
        None => panic!("missing identifier"),
    }
}

fn same_id(value: Option<&Value>, id: i64) -> bool {
    match value {
        Some(Value::Num(text)) | Some(Value::Str(text)) => text.parse::<i64>().ok() == Some(id),
        _ => false,
    }
}

fn py_quote(text: &str) -> String {
    format!("\"{}\"", json_string_body(text, true))
}

fn json_string_body(text: &str, ensure_ascii: bool) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch if !ensure_ascii || (ch as u32) <= 0x7f => out.push(ch),
            ch => {
                let code = ch as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let surrogate = code - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (surrogate >> 10),
                        0xdc00 + (surrogate & 0x3ff)
                    ));
                }
            }
        }
    }
    out
}

fn compact_sorted(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => {
            if *flag {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Num(token) => token.clone(),
        Value::Str(text) => py_quote(text),
        Value::Arr(items) => {
            let body = items.iter().map(compact_sorted).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Value::Obj(fields) => {
            let mut fields = fields.clone();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            let body = fields
                .iter()
                .map(|(key, item)| format!("{}:{}", py_quote(key), compact_sorted(item)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

fn collect_fields(value: &Value) -> String {
    let mut output = Vec::new();
    collect_into(value, &mut output);
    output.join("\n")
}

fn collect_into(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Obj(fields) => {
            if let Some(Value::Str(text)) = fields
                .iter()
                .find(|(key, _)| key == "value")
                .map(|(_, value)| value)
            {
                output.push(text.clone());
            }
            for (_, value) in fields {
                collect_into(value, output);
            }
        }
        Value::Arr(items) => {
            for item in items {
                collect_into(item, output);
            }
        }
        _ => {}
    }
}

fn check(passed: &mut Vec<String>, name: &str, value: bool) {
    if !value {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn number(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Num(number)) => number
            .parse::<i64>()
            .or_else(|_| number.parse::<u64>().map(|value| value as i64))
            .unwrap_or_else(|error| panic!("invalid number: {error}")),
        Some(Value::Str(text)) => text
            .parse::<i64>()
            .or_else(|_| text.parse::<u64>().map(|value| value as i64))
            .unwrap_or_else(|error| panic!("invalid number: {error}")),
        Some(Value::Bool(value)) => i64::from(*value),
        _ => 0,
    }
}

fn node_at(
    nodes: &[Value],
    by_address: &std::collections::HashMap<i64, i64>,
    address: i64,
) -> Value {
    let id = *by_address
        .get(&address)
        .unwrap_or_else(|| panic!("missing node address {address:#x}"));
    nodes
        .iter()
        .find(|node| same_id(node.get("id"), id))
        .unwrap_or_else(|| panic!("missing node {id}"))
        .clone()
}

fn sorted_rows(rows: &[Value]) -> Value {
    let mut rows = rows.to_vec();
    rows.sort_by(|left, right| compact_sorted(left).cmp(&compact_sorted(right)));
    Value::Arr(rows)
}

fn append_record(state: &mut Value, record: &Value) {
    if let Value::Obj(fields) = state {
        if let Some((_, records)) = fields
            .iter_mut()
            .find(|(key, _)| key == "investigation_records")
        {
            if let Value::Obj(record_fields) = records {
                if let Some((_, items)) = record_fields
                    .iter_mut()
                    .find(|(key, _)| key == "items")
                {
                    if let Value::Arr(items) = items {
                        items.push(record.clone());
                    }
                }
            }
        }
    }
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let Value::Obj(fields) = value else {
        panic!("expected object, got {}", value.to_string());
    };
    fields
        .iter_mut()
        .find(|(name, _)| name == key)
        .map(|(_, item)| item)
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn items_mut(value: &mut Value) -> &mut Vec<Value> {
    let Value::Arr(items) = value else {
        panic!("expected array, got {}", value.to_string());
    };
    items
}

fn mutate_record_relation(state: &mut Value, record_id: &str, amount: i64) {
    let items = items_mut(field_mut(field_mut(state, "investigation_records"), "items"));
    let record = items
        .iter_mut()
        .find(|item| item.get("id").and_then(string_ref) == Some(record_id))
        .unwrap_or_else(|| panic!("missing investigation record {record_id}"));
    let snapshots = items_mut(field_mut(record, "snapshots"));
    let snapshot = snapshots.first_mut().unwrap_or_else(|| panic!("missing snapshot"));
    let relations = items_mut(field_mut(field_mut(snapshot, "control_flow"), "relations"));
    let relation = relations.first_mut().unwrap_or_else(|| panic!("missing relation"));
    let target = field_mut(relation, "target_address");
    let updated = number(Some(target)) + amount;
    *target = twin::n(&updated.to_string());
}

fn output_text(program: PathBuf, arguments: &[&str], input: &Path) -> String {
    let output = Command::new(&program)
        .args(arguments)
        .arg(input)
        .output()
        .unwrap_or_else(|error| panic!("could not run {}: {error}", program.display()));
    assert!(output.status.success(), "{} failed", program.display());
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("{} output: {error}", program.display()))
}

fn run_tool(program: PathBuf, leading: &[&str], trailing: &[&Path], scratch: &Path) {
    let output = Command::new(&program)
        .args(leading)
        .args(trailing)
        .env("TMPDIR", scratch)
        .output()
        .unwrap_or_else(|error| panic!("could not run {}: {error}", program.display()));
    assert!(
        output.status.success(),
        "{} failed: {}",
        program.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_load_headers(text: &str) -> Vec<(i64, i64, i64, i64)> {
    let mut result = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find("Type: PT_LOAD") {
        rest = &rest[index + "Type: PT_LOAD".len()..];
        let offset = labeled_int(rest, "Offset: ");
        let address = labeled_int(rest, "VirtualAddress: ");
        let file_size = labeled_int(rest, "FileSize: ");
        let memory_size = labeled_int(rest, "MemSize: ");
        if let (Some(offset), Some(address), Some(file_size), Some(memory_size)) =
            (offset, address, file_size, memory_size)
        {
            result.push((offset, address, file_size, memory_size));
        }
        if let Some(mem_at) = rest.find("MemSize:") {
            rest = &rest[mem_at + "MemSize:".len()..];
        } else {
            break;
        }
    }
    result
}

fn labeled_int(text: &str, label: &str) -> Option<i64> {
    let start = text.find(label)?;
    parse_int(&text[start + label.len()..])
}

fn parse_int(text: &str) -> Option<i64> {
    let token = text.trim().split_whitespace().next().unwrap_or("");
    if let Some(hex) = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else if decimal_matches_the_legs_int_base_zero(token) {
        token.parse().ok()
    } else {
        None
    }
}

/// The leg converts with python's `int(v, 0)`, which refuses a decimal token that
/// has a leading zero unless every digit is zero: `int("09", 0)` raises while
/// `int("00", 0)` is 0. `str::parse` accepts both, so without this arm the twin
/// would read a value the leg refuses.
fn decimal_matches_the_legs_int_base_zero(token: &str) -> bool {
    let digits = token
        .strip_prefix('+')
        .or_else(|| token.strip_prefix('-'))
        .unwrap_or(token);
    !digits.starts_with('0') || digits.bytes().all(|byte| byte == b'0')
}

fn parse_i64_hex_field(text: &str, field: &str) -> Option<i64> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(field))
        .and_then(parse_int)
}

fn parse_pe_jump(text: &str) -> Option<(i64, i64)> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let Some((left, rest)) = trimmed.split_once(':') else {
            continue;
        };
        if left.is_empty() || !left.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(source) = i64::from_str_radix(left, 16) else {
            continue;
        };
        let mut words = rest.split_whitespace();
        while let Some(word) = words.next() {
            if word != "bl" {
                continue;
            }
            let Some(token) = words.next() else {
                break;
            };
            let Some(hex) = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) else {
                continue;
            };
            if let Ok(target) = i64::from_str_radix(hex, 16) {
                return Some((source, target));
            }
        }
    }
    None
}

fn read_zip_json(path: &Path, member: &str) -> Value {
    let payload = twin::zip_read(path, member);
    Value::parse(
        std::str::from_utf8(&payload)
            .unwrap_or_else(|_| panic!("archive member {member} is not utf-8")),
    )
}

const ASSEMBLY: &str = r#".text
.global entry
.global target
.global path_edge
.global arithmetic_target
.global consistent_edge
.type entry,%function
entry:
 bl target
 b finish
compare:
 cbz x0,target
 tbz x1,#2,compare
 b.eq target
consistent_edge:
 bc.eq target
 blr x2
 br x3
target:
 ret
finish:
 nop
literal:
 .word 0x11223344
resume_code:
 nop
 mov x4,#41
path_edge:
 b arithmetic_target
 mov x4,#99
arithmetic_target:
 add x5,x4,#1
 ret
.global auth_braa
auth_braa:
 braa x3,x4
.global auth_braaz
auth_braaz:
 braaz x5
.global auth_brab
auth_brab:
 brab x6,sp
.global auth_brabz
auth_brabz:
 brabz x7
.global auth_blraa
auth_blraa:
 blraa x8,x9
.global auth_blraaz
auth_blraaz:
 blraaz x10
.global auth_blrab
auth_blrab:
 blrab x11,sp
.global auth_blrabz
auth_blrabz:
 blrabz x12
.global auth_retaa
auth_retaa:
 retaa
.global auth_retab
auth_retab:
 retab
.global knowledge_start
.global knowledge_mask
knowledge_start:
 movk x16,#0x1234,lsl#16
 mov x10,#16
 lsrv x17,x16,x10
knowledge_mask:
 and x18,x17,#0xffff
 ret
.global join_start
.global join_mask
.global join_follow
join_start:
 mov x4,#41
 b join_mask
 mov x4,#99
join_mask:
 and x6,x4,#1
join_follow:
 add x7,x6,#1
 ret
.global arithmetic_bits_start
.global arithmetic_bits_mask
arithmetic_bits_start:
 movk x24,#0x1234,lsl#16
 add x25,x24,#1
arithmetic_bits_mask:
 and x26,x25,#0xfffe0000
 ret
.global selection_start
.global selection_edge
.global selection_result
selection_start:
 mov x19,#41
 mov x20,#43
selection_edge:
 csel x21,x19,x20,eq
 and x22,x21,#0xfffffffffffffffd
selection_result:
 add x23,x22,#1
 ret
.size entry,.-entry
"#;

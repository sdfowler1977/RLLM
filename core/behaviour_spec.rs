//! Cited clean-room behavioural specifications from retained observations.
//! The specification may contain at most QUOTED_FRAGMENT_MAX contiguous
//! subject bytes. A longer run is a copy of the observed subject and is refused.
use super::*;
use std::collections::{BTreeSet, HashMap};

/// Longest contiguous run of observed subject bytes a behavioural specification may contain.
/// The reconstruct tool description states this same bound.
pub(super) const QUOTED_FRAGMENT_MAX: usize = 16;
const MAX_ENCODING_CLAIMS: usize = 64;
const MAX_RELATION_CLAIMS: usize = 48;
const MAX_DECLARATION_CLAIMS: usize = 48;
const UNCITED: &str = "No supporting observation; this is not an established behavioural claim.";

pub(super) struct Citation {
    pub record: String,
    pub snapshot: String,
    pub node: String,
    pub file_offset: Option<u64>,
    pub address: Option<u64>,
}

enum Placement { Known(Json), Unknown(Json) }

fn id_text(v: &Json) -> String {
    match v {
        Num(n) | Str(n) => n.clone(),
        _ => String::new(),
    }
}

fn u64_of(v: &Json) -> Option<u64> {
    match v {
        Num(n) | Str(n) => n.parse().ok(),
        _ => None,
    }
}

fn supported(c: &Citation) -> bool {
    !c.record.is_empty()
        && !c.snapshot.is_empty()
        && c.snapshot.parse::<u64>().is_ok()
        && !c.node.is_empty()
        && c.node.parse::<u64>().is_ok()
}

fn cited(statement: &str, c: &Citation) -> Json {
    let mut citation = obj(vec![
        ("record", s(&c.record)),
        ("snapshot", Num(c.snapshot.clone())),
        ("node", Num(c.node.clone())),
    ]);
    if let Some(offset) = c.file_offset {
        put(&mut citation, "file_offset", Num(offset.to_string()));
    }
    if let Some(address) = c.address {
        put(&mut citation, "address", Num(address.to_string()));
    }
    obj(vec![("statement", s(statement)), ("citation", citation)])
}

fn unknown_item(statement: &str, reason: &str) -> Json {
    obj(vec![("statement", s(statement)), ("reason", s(reason))])
}

/// A claim with no usable observation is unknown. It is not written as established behaviour.
fn place(statement: &str, citation: Option<Citation>) -> Placement {
    match citation {
        Some(c) if supported(&c) => Placement::Known(cited(statement, &c)),
        _ => Placement::Unknown(unknown_item(statement, UNCITED)),
    }
}

fn push(known: &mut Vec<Json>, unknown: &mut Vec<Json>, statement: String, citation: Option<Citation>) {
    match place(&statement, citation) {
        Placement::Known(v) => known.push(v),
        Placement::Unknown(v) => unknown.push(v),
    }
}

fn base_citation(record: &str, snapshot: &str, node: &str, file_offset: Option<u64>, address: Option<u64>) -> Option<Citation> {
    let c = Citation {
        record: record.to_string(),
        snapshot: snapshot.to_string(),
        node: node.to_string(),
        file_offset,
        address,
    };
    supported(&c).then_some(c)
}

/// Refuse when `body` contains any contiguous `subject` run longer than [`QUOTED_FRAGMENT_MAX`].
pub(super) fn refuse_subject_copy(body: &[u8], subject: &[u8]) -> Result<()> {
    let span = QUOTED_FRAGMENT_MAX + 1;
    if body.len() < span || subject.len() < span {
        return Ok(());
    }
    const RADIX: u64 = 131;
    let mut high = 1u64;
    for _ in 1..span {
        high = high.wrapping_mul(RADIX);
    }
    let hash_window = |bytes: &[u8]| -> u64 {
        let mut h = 0u64;
        for &b in bytes {
            h = h.wrapping_mul(RADIX).wrapping_add(b as u64);
        }
        h
    };
    let mut at = HashMap::<u64, Vec<usize>>::new();
    let mut h = hash_window(&body[..span]);
    at.entry(h).or_default().push(0);
    for i in 1..=body.len() - span {
        h = h
            .wrapping_sub((body[i - 1] as u64).wrapping_mul(high))
            .wrapping_mul(RADIX)
            .wrapping_add(body[i + span - 1] as u64);
        at.entry(h).or_default().push(i);
    }
    let mut h = hash_window(&subject[..span]);
    let hit = |h: u64, j: usize| -> bool {
        at.get(&h).is_some_and(|positions| {
            positions.iter().any(|&i| body[i..i + span] == subject[j..j + span])
        })
    };
    if hit(h, 0) {
        return Err(format!(
            "Behavioural specification copies {span} or more contiguous subject bytes; quoted fragments are limited to {QUOTED_FRAGMENT_MAX} bytes"
        ));
    }
    for j in 1..=subject.len() - span {
        h = h
            .wrapping_sub((subject[j - 1] as u64).wrapping_mul(high))
            .wrapping_mul(RADIX)
            .wrapping_add(subject[j + span - 1] as u64);
        if hit(h, j) {
            return Err(format!(
                "Behavioural specification copies {span} or more contiguous subject bytes; quoted fragments are limited to {QUOTED_FRAGMENT_MAX} bytes"
            ));
        }
    }
    Ok(())
}

struct Gaps {
    omitted_relations: u64,
    unclassified_words: u64,
    omitted_declarations: u64,
}

struct Subject {
    record: String,
    snapshot: String,
    node: String,
    path_hex: String,
    kind: String,
    coverage: String,
    coordinate: String,
    selected_kind: String,
}

fn specify(subject: &Subject, nodes: &[Json], relations: &[Json], declarations: &Json, gaps: &Gaps) -> Json {
    let mut known = Vec::new();
    let mut unknown = Vec::new();
    for statement in [
        "The subject was executed and its outputs were observed.",
        "A runtime call was resolved and entered.",
        "Third-party source was recovered from the subject.",
    ] {
        push(&mut known, &mut unknown, statement.into(), None);
    }
    if subject.kind != "binary" {
        push(
            &mut known,
            &mut unknown,
            "Native encoding, calls and control effects are established for this subject.".into(),
            None,
        );
    }
    if subject.coverage != "Complete" {
        unknown.push(unknown_item(
            "Bytes outside the retained sample establish behaviour.",
            "Coverage is partial or not a complete file; unobserved bytes establish nothing.",
        ));
    }
    if gaps.unclassified_words > 0 {
        unknown.push(unknown_item(
            "Unclassified retained encodings have an established operation.",
            &format!(
                "{} unclassified words are retained without a recognised operation.",
                gaps.unclassified_words
            ),
        ));
    }
    if gaps.omitted_relations > 0 {
        unknown.push(unknown_item(
            "Omitted encoded relations have established behaviour.",
            &format!(
                "{} encoded relations were omitted from the retained observation.",
                gaps.omitted_relations
            ),
        ));
    }
    if gaps.omitted_declarations > 0 {
        unknown.push(unknown_item(
            "Omitted mapping, classification or relocation rows have established behaviour.",
            &format!("{} declaration rows were omitted from the retained observation.", gaps.omitted_declarations),
        ));
    }
    let mut seen_nodes = BTreeSet::new();
    let mut encoding_claims = 0usize;
    let mut skipped_encodings = 0usize;
    for node in nodes {
        if text(node, "kind") != "Encoding" {
            continue;
        }
        let id = id_text(get(node, "id"));
        if !seen_nodes.insert(id.clone()) {
            continue;
        }
        let encoding = get(node, "encoding");
        let mnemonic = text(encoding, "mnemonic");
        let offset = u64_of(get(encoding, "file_offset"));
        let address = u64_of(get(encoding, "address"));
        let block = u64_of(get(encoding, "block"));
        if id.is_empty() || mnemonic.is_empty() {
            push(
                &mut known,
                &mut unknown,
                "An encoding row establishes an operation.".into(),
                None,
            );
            continue;
        }
        if encoding_claims >= MAX_ENCODING_CLAIMS {
            skipped_encodings += 1;
            continue;
        }
        let coordinate = {
            let value = text(encoding, "coordinate_kind");
            if value.is_empty() { subject.coordinate.clone() } else { value }
        };
        let block_text = block.map(|n| n.to_string()).unwrap_or_else(|| "unrecorded".into());
        let statement = format!(
            "A retained encoding is recorded at the cited coordinate ({coordinate}) with mnemonic {mnemonic} in candidate block {block_text}. This records the stored encoding class, not an executed effect."
        );
        push(
            &mut known,
            &mut unknown,
            statement,
            base_citation(&subject.record, &subject.snapshot, &id, offset, address),
        );
        encoding_claims += 1;
    }
    if skipped_encodings > 0 {
        unknown.push(unknown_item(
            "Every retained encoding has an individual behavioural claim.",
            &format!(
                "{skipped_encodings} additional retained encodings were not specified individually; their behaviour is not established by this document."
            ),
        ));
    }
    let kept: BTreeSet<String> = nodes.iter().map(|n| id_text(get(n, "id"))).filter(|id| !id.is_empty()).collect();
    let mut seen_relations = BTreeSet::new();
    let mut relation_claims = 0usize;
    let mut skipped_relations = 0usize;
    for relation in relations {
        let source = id_text(get(relation, "source"));
        if source.is_empty() || !kept.contains(&source) {
            continue;
        }
        let name = text(relation, "relation");
        if name.is_empty() {
            push(&mut known, &mut unknown, "An encoded control-flow row establishes a transfer.".into(), None);
            continue;
        }
        let key = relation.string();
        if !seen_relations.insert(key) {
            continue;
        }
        if relation_claims >= MAX_RELATION_CLAIMS {
            skipped_relations += 1;
            continue;
        }
        let node = nodes.iter().find(|n| id_text(get(n, "id")) == source);
        let fallback = Null;
        let encoding = node.map(|n| get(n, "encoding")).unwrap_or(&fallback);
        let resolution = text(relation, "resolution");
        let resolution = if resolution.is_empty() { "unrecorded".into() } else { resolution };
        let statement = format!(
            "The retained observation records encoded relation {name}. Recorded resolution: {resolution}. This is an encoded relationship, not observed execution."
        );
        push(
            &mut known,
            &mut unknown,
            statement,
            base_citation(
                &subject.record,
                &subject.snapshot,
                &source,
                u64_of(get(encoding, "file_offset")),
                u64_of(get(encoding, "address")),
            ),
        );
        relation_claims += 1;
    }
    if skipped_relations > 0 {
        unknown.push(unknown_item(
            "Every retained encoded relation has an individual behavioural claim.",
            &format!("{skipped_relations} additional encoded relations were not specified individually."),
        ));
    }
    let root_subject = subject.node == "0" || subject.selected_kind == "BinarySource";
    if subject.selected_kind == "Encoding" {
        unknown.push(unknown_item(
            "File-level mappings, classifications and relocations establish this encoding node's behaviour.",
            "Those declarations are outside the selected encoding node and are not claims about it.",
        ));
    } else if root_subject && declarations != &Null {
        let mut declaration_claims = 0usize;
        let mut skipped_declarations = 0usize;
        for group in ["mappings", "classifications", "relocations"] {
            for row in array(get(declarations, group), "rows") {
                if declaration_claims >= MAX_DECLARATION_CLAIMS {
                    skipped_declarations += 1;
                    continue;
                }
                let statement = declaration_statement(group, &row);
                let address = match group {
                    "mappings" => u64_of(get(&row, "address")),
                    "classifications" => u64_of(get(&row, "start")),
                    _ => u64_of(get(&row, "position")).filter(|_| {
                        text(&row, "position_kind").contains("VA") || text(&row, "position_kind").contains("RVA")
                    }).or_else(|| u64_of(get(&row, "target_rva"))),
                };
                let offset = u64_of(get(&row, "file_offset"));
                if group == "classifications" && boolean(&row, "contradicted") {
                    unknown.push(unknown_item(
                        "A contradicted mapping-symbol interval establishes whether its addresses are code or data.",
                        "The observation records the contradiction, so the interval's class is not established.",
                    ));
                    continue;
                }
                push(
                    &mut known,
                    &mut unknown,
                    statement,
                    base_citation(&subject.record, &subject.snapshot, "0", offset, address),
                );
                declaration_claims += 1;
            }
        }
        if skipped_declarations > 0 {
            unknown.push(unknown_item(
                "Every retained declaration row has an individual behavioural claim.",
                &format!("{skipped_declarations} additional declaration rows were not specified individually."),
            ));
        }
    }
    let mut subject_json = obj(vec![
        ("record", s(&subject.record)),
        ("snapshot", Num(subject.snapshot.clone())),
        ("node", Num(subject.node.clone())),
        ("path_hex", s(&subject.path_hex)),
        ("observation_kind", s(&subject.kind)),
        ("coverage", s(&subject.coverage)),
    ]);
    if !subject.coordinate.is_empty() {
        put(&mut subject_json, "coordinate_kind", s(&subject.coordinate));
    }
    obj(vec![
        ("schema", Num("1".into())),
        ("type", s("clean_room_behaviour")),
        ("subject", subject_json),
        ("claims", Arr(known)),
        ("unknown", Arr(unknown)),
        ("quote_limit", Num(QUOTED_FRAGMENT_MAX.to_string())),
    ])
}

fn short_label(value: &str) -> String {
    if value.is_empty() || value.len() > QUOTED_FRAGMENT_MAX {
        "a recorded kind".into()
    } else {
        value.to_string()
    }
}

fn declaration_statement(group: &str, row: &Json) -> String {
    match group {
        "mappings" => format!(
            "A declared file mapping is retained at the cited coordinate. Executable declaration is {}. This is mapping metadata, not observed execution.",
            boolean(row, "executable_declaration")
        ),
        "classifications" => format!(
            "A mapping-symbol interval is retained at the cited addresses, declared as {}. Mapping agreement is {}. This does not establish that the interval executed.",
            if boolean(row, "code_declaration") { "code" } else { "data or a literal pool" },
            boolean(row, "mapping_agrees")
        ),
        _ => format!(
            "A relocation declaration ({}) is retained at the cited coordinate. This is stored relocation metadata, not a resolved or patched call.",
            short_label(&text(row, "name"))
        ),
    }
}

fn binding_from(root: &Json) -> Json {
    let sample = get(root, "sample");
    let hash = text(sample, "sha256");
    let length = id_text(get(sample, "length"));
    if hash.len() != 64 || length.parse::<u64>().is_err() || text(root, "path_hex").is_empty() {
        return Null;
    }
    obj(vec![
        ("path_hex", s(&text(root, "path_hex"))),
        ("length", Num(length)),
        ("sha256", s(&hash)),
        ("identity", get(root, "identity").clone()),
        ("complete_file", Bool(boolean(sample, "complete_file"))),
    ])
}

fn page(st: &Json, record: &str, snapshot: &Json, extra: Vec<(&str, Json)>, model: bool) -> Result<Json> {
    let mut args = vec![("id", s(record)), ("snapshot", snapshot.clone())];
    args.extend(extra);
    parse(&observation_records::query(st, &obj(args), model)?)
}

pub(super) fn project(st: &Json, a: &Json, model: bool) -> Result<Json> {
    let record = text(a, "record");
    let snapshot = get(a, "snapshot").clone();
    let selected_page = page(st, &record, &snapshot, vec![("node", get(a, "node").clone())], model)?;
    let selected = array(&selected_page, "nodes").first().cloned().ok_or("Observed node is unavailable")?;
    let selected_hex = text(&selected, "path_hex");
    let prefix = format!("{selected_hex}2f");
    let mut nodes = Vec::new();
    let mut relations = Vec::new();
    let mut declarations = Null;
    let mut gaps = Gaps { omitted_relations: 0, unclassified_words: 0, omitted_declarations: 0 };
    let mut coordinate = String::new();
    let mut offset = 0usize;
    loop {
        let view = page(st, &record, &snapshot, vec![("offset", Num(offset.to_string()))], model)?;
        for n in array(&view, "nodes") {
            let path = text(&n, "path_hex");
            if path == selected_hex || selected_hex.is_empty() || path.starts_with(&prefix) {
                nodes.push(n);
            }
        }
        let flow = get(&view, "control_flow");
        if flow != &Null {
            if coordinate.is_empty() {
                coordinate = text(flow, "coordinate_kind");
            }
            if gaps.omitted_relations == 0 {
                gaps.omitted_relations = u64_of(get(flow, "omitted_relations")).unwrap_or(0);
                gaps.unclassified_words = u64_of(get(flow, "unclassified_words")).unwrap_or(0);
            }
            relations.extend(array(flow, "relations"));
        }
        if declarations == Null && get(&view, "declarations") != &Null {
            declarations = get(&view, "declarations").clone();
            if coordinate.is_empty() {
                coordinate = text(&declarations, "coordinate_kind");
            }
            for group in ["mappings", "classifications", "relocations"] {
                gaps.omitted_declarations = gaps
                    .omitted_declarations
                    .saturating_add(u64_of(get(get(&declarations, group), "omitted")).unwrap_or(0));
            }
        }
        if get(&view, "next_offset") == &Null {
            break;
        }
        let next = number(&view, "next_offset");
        if next <= offset || next >= 500 {
            return Err("Behavioural specification exceeds observation bounds".into());
        }
        offset = next;
    }
    let root_page = page(st, &record, &snapshot, vec![("node", Num("0".into()))], model)?;
    let root = array(&root_page, "nodes").first().cloned().ok_or("Observed source node is unavailable")?;
    if id_text(get(&root, "id")) != "0" {
        return Err("Behavioural specification could not bind the observed source node".into());
    }
    if coordinate.is_empty() {
        coordinate = text(get(&selected, "encoding"), "coordinate_kind");
    }
    let subject = Subject {
        record: record.clone(),
        snapshot: id_text(&snapshot),
        node: id_text(get(&selected, "id")),
        path_hex: selected_hex,
        kind: text(&selected_page, "kind"),
        coverage: text(&selected_page, "coverage"),
        coordinate,
        selected_kind: text(&selected, "kind"),
    };
    if subject.node.is_empty() {
        return Err("Behavioural specification requires the selected observation node".into());
    }
    let document = specify(&subject, &nodes, &relations, &declarations, &gaps);
    Ok(obj(vec![
        ("document", document),
        ("subject_binding", binding_from(&root)),
    ]))
}

#[cfg(test)]
mod checks {
    use super::*;

    fn encoding(id: &str, mnemonic: &str, offset: u64, address: u64) -> Json {
        obj(vec![
            ("id", Num(id.into())),
            ("kind", s("Encoding")),
            ("path_hex", s("aa")),
            ("content", s("DO_NOT_COPY_THIS_SUBJECT_TEXT_EVER")),
            ("encoding", obj(vec![
                ("file_offset", Num(offset.to_string())),
                ("address", Num(address.to_string())),
                ("block", Num(address.to_string())),
                ("mnemonic", s(mnemonic)),
                ("coordinate_kind", s("ELF declared VA")),
                ("recognized", Bool(mnemonic != "unclassified")),
            ])),
        ])
    }

    fn subject() -> Subject {
        Subject {
            record: "rec".into(),
            snapshot: "0".into(),
            node: "0".into(),
            path_hex: "aa".into(),
            kind: "binary".into(),
            coverage: "Partial".into(),
            coordinate: "ELF declared VA".into(),
            selected_kind: "BinarySource".into(),
        }
    }

    #[test]
    fn uncited_claim_lands_in_unknown() {
        match place("The subject returns zero.", None) {
            Placement::Known(_) => panic!("uncited claim was established"),
            Placement::Unknown(v) => {
                assert_eq!(text(&v, "statement"), "The subject returns zero.");
                assert!(text(&v, "reason").contains("No supporting observation"));
            }
        }
        let cited = Citation { record: "rec".into(), snapshot: "0".into(), node: "3".into(), file_offset: Some(16), address: Some(4096) };
        match place("A retained encoding is recorded.", Some(cited)) {
            Placement::Unknown(_) => panic!("cited claim was treated as unknown"),
            Placement::Known(v) => {
                assert_eq!(text(get(&v, "citation"), "record"), "rec");
                assert_eq!(get(get(&v, "citation"), "node"), &Num("3".into()));
            }
        }
        let bare = obj(vec![("kind", s("Encoding")), ("encoding", obj(vec![("mnemonic", s("RET"))]))]);
        let doc = specify(&subject(), &[bare], &[], &Null, &Gaps { omitted_relations: 0, unclassified_words: 0, omitted_declarations: 0 });
        let claims = array(&doc, "claims");
        let unknown = array(&doc, "unknown");
        assert!(claims.iter().all(|c| !text(c, "statement").contains("executed")));
        assert!(claims.iter().all(|c| !text(c, "statement").contains("RET")));
        assert!(unknown.iter().any(|u| text(u, "statement").contains("executed")));
        assert!(unknown.iter().any(|u| text(u, "statement").contains("encoding row establishes")));
        assert!(!doc.string().contains("DO_NOT_COPY_THIS_SUBJECT_TEXT_EVER"));
    }

    #[test]
    fn encoding_claim_cites_record_snapshot_node_and_coordinate() {
        let doc = specify(
            &subject(),
            &[encoding("3", "ADD", 16, 4096)],
            &[],
            &Null,
            &Gaps { omitted_relations: 0, unclassified_words: 0, omitted_declarations: 0 },
        );
        let claim = array(&doc, "claims").into_iter().find(|c| text(c, "statement").contains("mnemonic ADD")).expect("encoding claim");
        let citation = get(&claim, "citation");
        assert_eq!(text(citation, "record"), "rec");
        assert_eq!(get(citation, "snapshot"), &Num("0".into()));
        assert_eq!(get(citation, "node"), &Num("3".into()));
        assert_eq!(get(citation, "file_offset"), &Num("16".into()));
        assert_eq!(get(citation, "address"), &Num("4096".into()));
        assert!(text(&claim, "statement").contains("not an executed effect"));
        assert!(!doc.string().contains("DO_NOT_COPY_THIS_SUBJECT_TEXT_EVER"));
        assert_eq!(get(&doc, "quote_limit"), &Num(QUOTED_FRAGMENT_MAX.to_string()));
    }

    #[test]
    fn quoted_fragment_allows_sixteen_bytes_and_refuses_seventeen() {
        assert_eq!(QUOTED_FRAGMENT_MAX, 16);
        let subject = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let short = b"prefix ABCDEFGHIJKLMNOP suffix";
        assert_eq!(&short[7..23], &subject[..16]);
        refuse_subject_copy(short, subject).unwrap();
        let long = b"prefix ABCDEFGHIJKLMNOPQ suffix";
        assert_eq!(&long[7..24], &subject[..17]);
        assert!(refuse_subject_copy(long, subject).unwrap_err().contains("17"));
        refuse_subject_copy(&subject[5..21], subject).unwrap();
        assert!(refuse_subject_copy(&subject[5..22], subject).is_err());
        let mut repeated = vec![b'A'; 64];
        refuse_subject_copy(&repeated[..16], &repeated).unwrap();
        assert!(refuse_subject_copy(&repeated[..17], &repeated).is_err());
        repeated[40] = 0;
        let window = repeated[30..47].to_vec();
        assert_eq!(window.len(), 17);
        assert!(refuse_subject_copy(&window, &repeated).is_err());
        assert!(refuse_subject_copy(&window[..16], &repeated).is_ok());
        assert!(refuse_subject_copy(b"short", subject).is_ok());
        assert!(refuse_subject_copy(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ", b"tiny").is_ok());
    }
}

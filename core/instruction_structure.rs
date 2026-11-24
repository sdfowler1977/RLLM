//! Structural reconstruction of the captured instruction graph.
use super::{coordinate_target, direct, mask, reg, statement, terminal, Branch, Instruction, State, Word};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

type Node = usize;
const NO_NODE: Node = usize::MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Block(Node),
    Outside(Option<u64>),
}

#[derive(Clone)]
enum End {
    Next(Target),
    If { predicate: String, yes: Target, no: Target },
    Switch { selector: String, cases: Vec<(u64, Target)>, default: String },
    Stop { text: String, incomplete: bool },
}

impl End {
    fn targets(&self) -> Vec<Target> {
        match self {
            Self::Next(target) => vec![*target],
            Self::If { yes, no, .. } => vec![*yes, *no],
            Self::Switch { cases, .. } => cases.iter().map(|&(_, target)| target).collect(),
            Self::Stop { .. } => Vec::new(),
        }
    }

    fn incomplete(&self) -> bool {
        match self {
            Self::Stop { incomplete, .. } => *incomplete,
            Self::Switch { default, .. } => !default.is_empty(),
            _ => self.targets().iter().any(|target| matches!(target, Target::Outside(_))),
        }
    }
}

struct Block {
    address: u64,
    lines: Vec<String>,
    end: End,
    first: Instruction,
    effect_count: usize,
    predicate_register: Option<u8>,
}

struct Table {
    start: usize,
    branch: usize,
    selector: u8,
    slots: Vec<u64>,
    guard: usize,
}

fn add_step(inst: Instruction) -> Option<(u8, bool, bool, u64)> {
    match inst {
        Instruction::Immediate { wide, subtract, flags: false, d, n, value }
            if d == n && d < 31 && value != 0 => Some((d, wide, subtract, value)),
        _ => None,
    }
}

// An inline table consists of executable branch slots. The preceding unsigned
// range check supplies the slot count. No data bytes are reclassified as code.
fn tables(words: &[Word], starts: &BTreeSet<u64>) -> Vec<Table> {
    let mut tables = Vec::new();
    for (index, window) in words.windows(5).enumerate() {
        let [compare, guard, base, add, jump] = window else { continue };
        if !window.windows(2).all(|pair| pair[0].1.checked_add(4) == Some(pair[1].1)) {
            continue;
        }
        let Instruction::Immediate {
            wide: true, subtract: true, flags: true, d: 31, n: selector, value,
        } = compare.4 else { continue };
        let Some(Branch::Conditional { condition: 8, offset, .. }) = guard.3 else { continue };
        let (Instruction::Address { page, d: base_reg, offset: base_delta },
            Instruction::Arithmetic {
                wide: true, subtract: false, flags: false, d, n, m, shift: 0, amount: 2,
            },
            Some(Branch::Register { kind: 0, register })) = (base.4, add.4, jump.3)
        else { continue };
        if n != base_reg || m != selector || d != register || selector == 31
            || base_reg == 31 || base_reg == selector || d == selector || d == 31
            || starts.contains(&guard.1) || starts.contains(&add.1) || starts.contains(&jump.1)
        {
            continue;
        }
        let Some(table) = coordinate_target(if page { base.1 & !4095 } else { base.1 }, base_delta)
        else { continue };
        let Some(default) = coordinate_target(guard.1, offset) else { continue };
        let Some(count) = value.checked_add(1).and_then(|n| usize::try_from(n).ok())
        else { continue };
        let Ok(first) = words.binary_search_by_key(&table, |word| word.1) else { continue };
        let Some(slots) = first.checked_add(count).and_then(|end| words.get(first..end))
        else { continue };
        if slots.is_empty() || slots.iter().any(|word| word.1 == default)
            || !slots.iter().enumerate().all(|(slot, word)| {
                table.checked_add((slot as u64) * 4) == Some(word.1)
                    && matches!(word.3, Some(Branch::Direct { link: false, .. }))
            })
        {
            continue;
        }
        tables.push(Table {
            start: index + 2,
            branch: index + 4,
            selector,
            slots: slots.iter().map(|word| word.1).collect(),
            guard: index + 1,
        });
    }
    tables
}

fn flag_test(code: u8) -> String {
    let name = ["eq", "ne", "hs", "lo", "mi", "pl", "vs", "vc",
        "hi", "ls", "ge", "lt", "gt", "le", "al", "al"][code as usize];
    format!("{name}(NZCV)")
}

fn condition(branch: Branch, previous: Option<Instruction>) -> (String, Option<u8>) {
    let relation = |nonzero| if nonzero { "!=" } else { "==" };
    match branch {
        Branch::Compare { register, wide, nonzero, .. } =>
            (format!("{} {} 0", operand(register, wide, false), relation(nonzero)), Some(register)),
        Branch::Test { register, bit, nonzero, .. } =>
            (format!("({} & {:#x}) {} 0", operand(register, bit >= 32, false),
                1u64 << bit, relation(nonzero)), Some(register)),
        Branch::Conditional { condition, .. } => {
            if let Some(Instruction::Immediate {
                wide, subtract: true, flags: true, d: 31, n, value,
            }) = previous {
                let width = if wide { 64 } else { 32 };
                let lhs = operand(n, wide, true);
                let relation = match condition {
                    0 => Some(("==", false)), 1 => Some(("!=", false)),
                    2 => Some((">=", false)), 3 => Some(("<", false)),
                    8 => Some((">", false)), 9 => Some(("<=", false)),
                    10 => Some((">=", true)), 11 => Some(("<", true)),
                    12 => Some((">", true)), 13 => Some(("<=", true)),
                    _ => None,
                };
                if let Some((operator, signed)) = relation {
                    let test = if signed {
                        format!("i{width}({lhs}) {operator} {value:#x}")
                    } else {
                        format!("{lhs} {operator} {value:#x}")
                    };
                    return (test, Some(n));
                }
            }
            (flag_test(condition), None)
        }
        _ => unreachable!("conditional terminator"),
    }
}

fn operand(register: u8, wide: bool, sp: bool) -> String {
    if register == 31 && !sp { "0".into() } else { reg(register, wide, sp) }
}

fn add_expression(lhs: &str, rhs: u64, subtract: bool, wide: bool) -> String {
    if rhs == 0 {
        lhs.into()
    } else {
        format!("u{}({lhs} {} {rhs:#x})", if wide { 64 } else { 32 },
            if subtract { "-" } else { "+" })
    }
}

fn definitions(pc: u64, register: &str, expression: &str) -> Vec<String> {
    let temporary = format!("t_{pc:x}_{}", register.to_ascii_lowercase());
    vec![format!("let {temporary} = {expression};"), format!("{register} = {temporary};")]
}

fn address_offset(base: &str, offset: i64) -> String {
    add_expression(base, offset.unsigned_abs(), offset < 0, true)
}

fn lift(word: &Word, state: &mut State, label: &str) -> Vec<String> {
    use Instruction::*;
    let before = state.clone();
    let evidence = statement(word.4, word.3, word.1, state, label);
    let read = |n, wide, sp| {
        before.read(n, wide, sp).full().filter(|value| !value.relative)
            .map_or_else(|| operand(n, wide, sp), |value| format!("{:#x}", value.bits))
    };
    let shifted = |m, wide, shift, amount| {
        let source = read(m, wide, false);
        if amount == 0 { source } else {
            format!("{}{}({source}, {amount})", super::shift_name(shift).to_ascii_lowercase(),
                if wide { 64 } else { 32 })
        }
    };
    let width = |wide| if wide { 64 } else { 32 };
    let mut lines = Vec::new();
    let assignment = |d, wide, sp: bool, expression: String| {
        if d == 31 && !sp { return Vec::new(); }
        let value = state.read(d, wide, sp).full().map_or(expression, |value| {
            if value.relative { format!("address({label}0x{:x})", value.bits) }
            else { format!("{:#x}", value.bits) }
        });
        definitions(word.1, &reg(d, wide, sp), &value)
    };
    let read_memory = |d, wide, expression: &str| {
        if d == 31 { vec![format!("discard({expression});")] }
        else { definitions(word.1, &reg(d, wide, false), expression) }
    };
    match word.4 {
        Nop => {}
        Wide { wide, kind, d, immediate, shift } => {
            let bits = u64::from(immediate) << shift;
            let expr = match kind {
                0 => format!("{:#x}", !bits & mask(wide)),
                2 => format!("{bits:#x}"),
                _ => format!("({} & {:#x}) | {bits:#x}", read(d, wide, false),
                    !(0xffffu64 << shift) & mask(wide)),
            };
            lines.extend(assignment(d, wide, false, expr));
        }
        Address { page, d, offset } => {
            let pc = if page { word.1 & !4095 } else { word.1 };
            lines.extend(assignment(d, true, false, format!("pc_relative({pc:#x}, {offset})")));
        }
        Immediate { wide, subtract, flags, d, n, value } => {
            let lhs = read(n, wide, true);
            if flags {
                lines.push(format!("NZCV = flags_{}{}({lhs}, {value:#x});",
                    if subtract { "sub" } else { "add" }, width(wide)));
            }
            // Evaluate flags before assigning a destination that aliases an input.
            lines.extend(assignment(d, wide, !flags, add_expression(&lhs, value, subtract, wide)));
        }
        Arithmetic { wide, subtract, flags, d, n, m, shift, amount } => {
            let lhs = read(n, wide, false);
            let rhs = shifted(m, wide, shift, amount);
            if flags {
                lines.push(format!("NZCV = flags_{}{}({lhs}, {rhs});",
                    if subtract { "sub" } else { "add" }, width(wide)));
            }
            let expr = if rhs == "0x0" || rhs == "0" { lhs } else {
                format!("u{}({lhs} {} {rhs})", width(wide), if subtract { "-" } else { "+" })
            };
            lines.extend(assignment(d, wide, false, expr));
        }
        Carry { wide, subtract, flags, d, n, m } => {
            let (lhs, rhs) = (read(n, wide, false), read(m, wide, false));
            let name = if subtract { "sbc" } else { "adc" };
            let bits = width(wide);
            let carry = format!("t_{:x}_carry", word.1);
            lines.push(format!("let {carry} = carry(NZCV);"));
            if flags { lines.push(format!("NZCV = flags_{name}{bits}({lhs}, {rhs}, {carry});")); }
            lines.extend(assignment(d, wide, false, format!("{name}{bits}({lhs}, {rhs}, {carry})")));
        }
        Logical { wide, kind, invert, d, n, m, shift, amount } => {
            let lhs = read(n, wide, false);
            let rhs = shifted(m, wide, shift, amount);
            let rhs = if invert { format!("~{rhs}") } else { rhs };
            let operator = ["&", "|", "^", "&"][kind as usize];
            let expression = if n == 31 && kind == 1 && !invert { rhs } else {
                format!("u{}({lhs} {operator} {rhs})", width(wide))
            };
            if kind == 3 { lines.push(format!("NZCV = flags_logical{}({expression});", width(wide))); }
            lines.extend(assignment(d, wide, false, expression));
        }
        LogicalImmediate { wide, kind, d, n, value } => {
            let lhs = read(n, wide, false);
            let operator = ["&", "|", "^", "&"][kind as usize];
            let expression = format!("u{}({lhs} {operator} {value:#x})", width(wide));
            if kind == 3 { lines.push(format!("NZCV = flags_logical{}({expression});", width(wide))); }
            lines.extend(assignment(d, wide, kind != 3, expression));
        }
        Bitfield { wide, kind, d, n, rotate, last } => {
            let source = read(n, wide, false);
            let bits = width(wide);
            let expr = match kind {
                0 => format!("signed_bitfield{bits}({source}, {rotate}, {last})"),
                1 => format!("insert_bitfield{bits}({}, {source}, {rotate}, {last})", read(d, wide, false)),
                _ => format!("unsigned_bitfield{bits}({source}, {rotate}, {last})"),
            };
            lines.extend(assignment(d, wide, false, expr));
        }
        Select { wide, kind, d, n, m, condition: code } => {
            let yes = read(n, wide, false);
            let no = read(m, wide, false);
            let bits = width(wide);
            let no = match kind {
                1 => add_expression(&no, 1, false, wide),
                2 => format!("u{bits}(~{no})"),
                3 => format!("u{bits}(-{no})"),
                _ => no,
            };
            let predicate = flag_test(code);
            lines.extend(assignment(d, wide, false, format!("select({predicate}, {yes}, {no})")));
        }
        Product { wide, subtract, d, n, m, a } => {
            let (lhs, rhs, acc) = (read(n, wide, false), read(m, wide, false), read(a, wide, false));
            let bits = width(wide);
            let operator = if subtract { "-" } else { "+" };
            let expr = if !subtract && a == 31 { format!("u{bits}({lhs} * {rhs})") }
                else { format!("u{bits}({acc} {operator} ({lhs} * {rhs}))") };
            lines.extend(assignment(d, wide, false, expr));
        }
        Binary { wide, kind, d, n, m } => {
            let op = ["udiv_or_zero", "sdiv_wrapping_or_zero", "lsl_masked", "lsr_masked",
                "asr_masked", "ror_masked"][kind as usize];
            let (lhs, rhs) = (read(n, wide, false), read(m, wide, false));
            lines.extend(assignment(d, wide, false, format!("{op}{}({lhs}, {rhs})", width(wide))));
        }
        Extract { wide, d, n, m, amount } => {
            let (high, low) = (read(n, wide, false), read(m, wide, false));
            let expr = if amount == 0 { low } else { format!("extract{}({high}, {low}, {amount})", width(wide)) };
            lines.extend(assignment(d, wide, false, expr));
        }
        Memory { load, size, t, n, offset, mode, .. } => {
            let base = read(n, true, true);
            let update = address_offset(&base, offset);
            let address = if mode == 1 { &base } else { &update };
            let bits = 8usize << size;
            if load {
                lines.extend(read_memory(t, size == 3, &format!("load{bits}({address})")));
            } else {
                lines.push(format!("store{bits}({address}, {});", read(t, size == 3, false)));
            }
            if mode == 1 || mode == 3 { lines.extend(definitions(word.1, &reg(n, true, true), &update)); }
        }
        Pair { load, wide, t, t2, n, offset, mode } => {
            let base = read(n, true, true);
            let update = address_offset(&base, offset);
            let address = if mode == 1 { &base } else { &update };
            let bits = width(wide);
            if load {
                let pair = format!("t_{:x}_pair", word.1);
                lines.push(format!("let {pair} = load_pair{bits}({address});"));
                for (field, register) in [t, t2].into_iter().enumerate() {
                    if register != 31 {
                        lines.extend(definitions(word.1, &reg(register, wide, false), &format!("{pair}.{field}")));
                    }
                }
            } else {
                lines.push(format!("store_pair{bits}({address}, {}, {});",
                    read(t, wide, false), read(t2, wide, false)));
            }
            if mode != 2 { lines.extend(definitions(word.1, &reg(n, true, true), &update)); }
        }
        Literal { wide, t, offset } => {
            let address = coordinate_target(word.1, offset).map_or_else(
                || format!("pc_relative({:#x}, {offset})", word.1),
                |pc| format!("address({label}0x{pc:x})"));
            lines.extend(read_memory(t, wide, &format!("load{}({address})", width(wide))));
        }
        _ => {
            let encoding = evidence.split(';').next().unwrap_or(&evidence);
            lines.push(format!("asm({:#010x}); // {encoding}", word.2));
        }
    }
    lines
}

struct Graph {
    blocks: Vec<Block>,
    successors: Vec<Vec<Node>>,
    predecessors: Vec<Vec<Node>>,
    entries: BTreeSet<Node>,
    guarded_switches: BTreeMap<Node, Node>,
}

impl Graph {
    fn build(words: &[Word], starts: &BTreeSet<u64>, label: &str) -> Self {
        let mut starts = starts.clone();
        let tables = tables(words, &starts);
        for table in &tables { starts.extend(table.slots.iter().copied()); }
        // Give induction updates a distinct block when they precede a back edge.
        for pair in words.windows(2) {
            if pair[0].1.checked_add(4) == Some(pair[1].1) && add_step(pair[0].4).is_some()
                && matches!(pair[1].3, Some(Branch::Direct { link: false, offset })
                    if coordinate_target(pair[1].1, offset).is_some_and(|to| to < pair[0].1
                        && starts.contains(&to)))
            {
                starts.insert(pair[0].1);
            }
        }
        let boundaries: Vec<_> = words.iter().enumerate()
            .filter_map(|(index, word)| starts.contains(&word.1).then_some(index)).collect();
        let by_address: BTreeMap<_, _> = boundaries.iter().enumerate()
            .map(|(id, &index)| (words[index].1, id)).collect();
        let target = |address: Option<u64>| {
            address.filter(|&pc| label != "RVA " || pc <= u64::from(u32::MAX))
                .and_then(|pc| by_address.get(&pc).copied()).map_or(Target::Outside(address), Target::Block)
        };
        let mut entries = BTreeSet::new();
        let blocks: Vec<_> = boundaries.iter().zip(boundaries.iter().skip(1).copied()
            .chain(std::iter::once(words.len()))).map(|(&first, stop)| {
            let mut state = State::new();
            let mut lines = Vec::new();
            let last = &words[stop - 1];
            let last_is_transfer = terminal(last.4);
            let effect_end = stop - usize::from(last_is_transfer);
            let last_effect = effect_end.checked_sub(1)
                .filter(|&at| at >= first).map(|at| words[at].4);
            for word in &words[first..effect_end] { lines.extend(lift(word, &mut state, label)); }
            let next = target(last.1.checked_add(4));
            let direct_target = |offset| target(coordinate_target(last.1, offset));
            let mut predicate_register = None;
            let end = match last.3 {
                Some(Branch::Direct { link: false, offset }) => End::Next(direct_target(offset)),
                Some(Branch::Direct { link: true, offset }) => {
                    let address = coordinate_target(last.1, offset);
                    if let Target::Block(id) = target(address) {
                        if !matches!(next, Target::Block(n) if n == id) { entries.insert(id); }
                    }
                    lines.push(format!("call({});", address.map_or_else(|| "unresolved".into(),
                        |pc| format!("{label}0x{pc:x}"))));
                    End::Next(next)
                }
                Some(Branch::Conditional { condition: 14 | 15, offset, .. }) =>
                    End::Next(direct_target(offset)),
                Some(branch @ (Branch::Compare { .. } | Branch::Test { .. } | Branch::Conditional { .. })) => {
                    let (predicate, register) = condition(branch, last_effect);
                    predicate_register = register;
                    let yes = direct_target(direct(branch).expect("direct conditional"));
                    if yes == next { End::Next(next) } else { End::If { predicate, yes, no: next } }
                }
                Some(Branch::Register { kind: 0, register }) => {
                    if let Some(table) = tables.iter().find(|table| table.start == first && table.branch == stop - 1) {
                        End::Switch {
                            selector: operand(table.selector, true, false),
                            cases: table.slots.iter().enumerate()
                                .map(|(case, &pc)| (case as u64, target(Some(pc)))).collect(),
                            default: format!("indirect_transfer({});", reg(register, true, false)),
                        }
                    } else if let Some(value) = state.read(register, true, false).full().filter(|value| value.relative) {
                        End::Next(target(Some(value.bits)))
                    } else {
                        End::Stop { text: format!("indirect_transfer({});", operand(register, true, false)), incomplete: true }
                    }
                }
                Some(Branch::Register { kind: 2, register }) => End::Stop {
                    text: format!("return_to({});", operand(register, true, false)), incomplete: false,
                },
                Some(Branch::Authenticated { kind: 2, .. }) => End::Stop {
                    text: format!("authenticated_return({:#010x});", last.2), incomplete: false,
                },
                Some(Branch::Register { kind: 1, register }) => {
                    lines.push(format!("call_indirect({});", reg(register, true, false)));
                    End::Next(next)
                }
                Some(Branch::Authenticated { kind: 1, .. }) => {
                    lines.push(format!("authenticated_call({:#010x});", last.2));
                    End::Next(next)
                }
                Some(Branch::Authenticated { .. }) | Some(Branch::Register { .. }) => End::Stop {
                    text: format!("indirect_transfer_encoded({:#010x});", last.2), incomplete: true,
                },
                None if last_is_transfer => End::Stop {
                    text: format!("unresolved_instruction({:#010x}); // {}",
                        last.2, statement(last.4, None, last.1, &mut state, label)), incomplete: true,
                },
                None => End::Next(next),
            };
            Block { address: words[first].1, lines, end, first: words[first].4,
                effect_count: effect_end - first, predicate_register }
        }).collect();
        let mut successors: Vec<Vec<_>> = blocks.iter().map(|block| {
            block.end.targets().into_iter().filter_map(|target| match target {
                Target::Block(node) => Some(node), _ => None,
            }).collect()
        }).collect();
        // A verified sole incoming range guard makes the table exhaustive for
        // the captured entry paths. An alternate captured entry keeps its default.
        let provisional = reverse(&successors);
        let mut blocks = blocks;
        let mut guarded_switches = BTreeMap::new();
        for table in &tables {
            let Some(&dispatch) = by_address.get(&words[table.start].1) else { continue };
            let guard_address = words[table.guard].1;
            let guard = by_address.range(..=guard_address).next_back().map(|(_, &node)| node);
            let Some(guard) = guard else { continue };
            if provisional[dispatch].as_slice() == [guard]
                && !entries.contains(&dispatch)
                && matches!(blocks[guard].end, End::If { yes, no: Target::Block(n), .. }
                    if n == dispatch && yes != Target::Block(dispatch))
            {
                if let End::Switch { default, .. } = &mut blocks[dispatch].end {
                    default.clear();
                    guarded_switches.insert(guard, dispatch);
                }
            }
        }
        for edges in &mut successors { edges.sort_unstable(); edges.dedup(); }
        let predecessors = reverse(&successors);
        Self { blocks, successors, predecessors, entries, guarded_switches }
    }
}

fn reverse(edges: &[Vec<Node>]) -> Vec<Vec<Node>> {
    let mut reversed = vec![Vec::new(); edges.len()];
    for (from, next) in edges.iter().enumerate() {
        for &to in next { reversed[to].push(from); }
    }
    reversed
}

fn reachable(entry: Node, edges: &[Vec<Node>], allow: impl Fn(Node) -> bool) -> BTreeSet<Node> {
    let mut reached = BTreeSet::new();
    let mut work = vec![entry];
    while let Some(node) = work.pop() {
        if allow(node) && reached.insert(node) { work.extend(&edges[node]); }
    }
    reached
}

// Reverse postorder and immediate dominators use linear storage. The same
// computation on the reversed, exit-rooted graph gives postdominators.
fn dominators(entry: Node, edges: &[Vec<Node>], members: &BTreeSet<Node>) -> Vec<Node> {
    let mut seen = vec![false; edges.len()];
    let mut stack = vec![(entry, 0)];
    seen[entry] = true;
    let mut order = Vec::new();
    while let Some((node, index)) = stack.last_mut() {
        if let Some(&next) = edges[*node].get(*index) {
            *index += 1;
            if members.contains(&next) && !seen[next] {
                seen[next] = true;
                stack.push((next, 0));
            }
        } else {
            order.push(*node);
            stack.pop();
        }
    }
    order.reverse();
    let mut rank = vec![NO_NODE; edges.len()];
    for (index, &node) in order.iter().enumerate() { rank[node] = index; }
    let predecessors = reverse(edges);
    let mut parents = vec![NO_NODE; edges.len()];
    parents[entry] = entry;
    loop {
        let mut changed = false;
        for &node in &order[1..] {
            let mut incoming = predecessors[node].iter().copied().filter(|&id| parents[id] != NO_NODE);
            let Some(mut common) = incoming.next() else { continue };
            for mut other in incoming {
                while common != other {
                    while rank[common] > rank[other] { common = parents[common]; }
                    while rank[other] > rank[common] { other = parents[other]; }
                }
            }
            if parents[node] != common { parents[node] = common; changed = true; }
        }
        if !changed { return parents; }
    }
}

fn dominates(parents: &[Node], ancestor: Node, mut node: Node) -> bool {
    while node != NO_NODE {
        if node == ancestor { return true; }
        let parent = parents[node];
        if parent == node { break; }
        node = parent;
    }
    false
}

fn postdominators(graph: &Graph, members: &BTreeSet<Node>) -> Vec<Node> {
    let exit = graph.blocks.len();
    let mut edges = vec![Vec::new(); exit + 1];
    for &node in members {
        for target in graph.blocks[node].end.targets() {
            edges[node].push(match target {
                Target::Block(id) if members.contains(&id) => id,
                _ => exit,
            });
        }
        if edges[node].is_empty() || graph.blocks[node].end.incomplete() { edges[node].push(exit); }
    }
    let exits = reachable(exit, &reverse(&edges), |_| true);
    for &node in members {
        // A non-terminating region has no useful exit postdominator.
        if !exits.contains(&node) { edges[node].push(exit); }
    }
    let nodes = members.iter().copied().chain(std::iter::once(exit)).collect();
    dominators(exit, &reverse(&edges), &nodes)
}

enum LoopKind {
    Head { body: Node, condition: String, update: Option<String> },
    Tail { latch: Node, condition: String },
    Endless,
}

struct Loop {
    natural: BTreeSet<Node>,
    members: BTreeSet<Node>,
    after: Option<Target>,
    kind: LoopKind,
}

struct Structure {
    loops: BTreeMap<Node, Loop>,
    postdom: Vec<Node>,
    irreducible: bool,
}

fn test_toward(end: &End, target: Target) -> Option<(String, Target)> {
    match end {
        End::If { predicate, yes, no } if *yes == target => Some((predicate.clone(), *no)),
        End::If { predicate, yes, no } if *no == target => Some((format!("!({predicate})"), *yes)),
        _ => None,
    }
}

impl Structure {
    fn discover(graph: &Graph, entry: Node, members: &BTreeSet<Node>) -> Self {
        let dom = dominators(entry, &graph.successors, members);
        let mut back = BTreeMap::<Node, Vec<Node>>::new();
        let mut forward_in = vec![0; graph.blocks.len()];
        let mut forward = vec![Vec::new(); graph.blocks.len()];
        for &from in members {
            for &to in &graph.successors[from] {
                if !members.contains(&to) { continue; }
                if dominates(&dom, to, from) {
                    back.entry(to).or_default().push(from);
                } else {
                    forward[from].push(to);
                    forward_in[to] += 1;
                }
            }
        }
        // A graph is reducible iff removing its dominance back edges leaves a DAG.
        let mut ready: VecDeque<_> = members.iter().copied().filter(|&node| forward_in[node] == 0).collect();
        let mut count = 0;
        while let Some(node) = ready.pop_front() {
            count += 1;
            for &to in &forward[node] {
                forward_in[to] -= 1;
                if forward_in[to] == 0 { ready.push_back(to); }
            }
        }
        let irreducible = count != members.len();
        let mut loops = BTreeMap::new();
        for (head, latches) in back {
            let mut region = BTreeSet::from([head]);
            let mut work = latches.clone();
            while let Some(node) = work.pop() {
                if dominates(&dom, head, node) && region.insert(node) { work.extend(&graph.predecessors[node]); }
            }
            let latch = (latches.len() == 1).then_some(latches[0]);
            let tail_test = latch.and_then(|latch| {
                let (condition, after) = test_toward(&graph.blocks[latch].end, Target::Block(head))?;
                (!matches!(after, Target::Block(n) if region.contains(&n))).then_some((latch, condition, after))
            });
            let head_test = graph.blocks[head].end.targets().iter().find_map(|&target| {
                let Target::Block(body) = target else { return None };
                if body == head || !region.contains(&body) { return None; }
                let (condition, after) = test_toward(&graph.blocks[head].end, target)?;
                (!matches!(after, Target::Block(n) if region.contains(&n))).then_some((body, condition, after))
            });
            let (kind, after) = if let Some((body, condition, after)) = head_test {
                let update = latch.filter(|&at| at != head).and_then(|at| {
                    let block = &graph.blocks[at];
                    let (d, wide, subtract, value) = add_step(block.first)?;
                    if block.effect_count != 1 || graph.blocks[head].predicate_register != Some(d)
                        || !matches!(block.end, End::Next(Target::Block(n)) if n == head)
                    { return None; }
                    let register = reg(d, wide, false);
                    Some(format!("{register} = {}", add_expression(&register, value, subtract, wide)))
                });
                (LoopKind::Head { body, condition, update }, Some(after))
            } else if let Some((latch, condition, after)) = tail_test {
                (LoopKind::Tail { latch, condition }, Some(after))
            } else {
                let mut exits = region.iter().flat_map(|&node| graph.blocks[node].end.targets())
                    .filter(|target| !matches!(target, Target::Block(n) if region.contains(n)));
                let after = exits.next().filter(|first| exits.all(|target| target == *first));
                (LoopKind::Endless, after)
            };
            let natural = region.clone();
            if let Some(after) = after {
                // Include single-entry break paths that cannot reach a latch.
                // The continuation and anything reachable from it stay outside.
                let after_reach = match after {
                    Target::Block(n) => reachable(n, &graph.successors, |id| id != head && members.contains(&id)),
                    Target::Outside(_) => BTreeSet::new(),
                };
                let mut work: Vec<_> = region.iter().flat_map(|&id| graph.successors[id].iter().copied()).collect();
                while let Some(node) = work.pop() {
                    if members.contains(&node) && !after_reach.contains(&node)
                        && dominates(&dom, head, node) && region.insert(node)
                    {
                        work.extend(&graph.successors[node]);
                    }
                }
            }
            loops.insert(head, Loop { natural, members: region, after, kind });
        }
        Self { loops, postdom: postdominators(graph, members), irreducible }
    }
}

#[derive(Clone, Copy)]
struct Fence {
    head: Node,
    repeat: Node,
    after: Option<Target>,
}

#[derive(Clone, Copy)]
struct Scope<'a> {
    members: &'a BTreeSet<Node>,
    stop: Option<Node>,
    fence: Option<Fence>,
}

struct Renderer<'a> {
    graph: &'a Graph,
    structure: &'a Structure,
    function: &'a BTreeSet<Node>,
    label: &'a str,
    text: String,
    seen: BTreeSet<Node>,
}

fn evaluated(lines: &[String], predicate: &str) -> String {
    if lines.is_empty() { return predicate.into(); }
    let actions = lines.iter().map(|line| line.split(" // ").next().unwrap_or(line))
        .collect::<Vec<_>>().join(" ");
    format!("evaluate {{ {actions} yield {predicate}; }}")
}

fn grouped_cases(cases: &[(u64, Target)]) -> Vec<(Vec<u64>, Target)> {
    let mut groups: Vec<(Vec<u64>, Target)> = Vec::new();
    for &(value, target) in cases {
        if let Some((values, _)) = groups.iter_mut().find(|(_, existing)| *existing == target) {
            values.push(value);
        } else { groups.push((vec![value], target)); }
    }
    groups
}

impl Renderer<'_> {
    fn line(&mut self, depth: usize, line: &str) {
        for _ in 0..depth { self.text.push_str("  "); }
        self.text.push_str(line);
        self.text.push('\n');
    }

    fn address(&self, id: Node) -> u64 { self.graph.blocks[id].address }

    fn fence_transfer(&self, target: Target, scope: Scope<'_>) -> Option<String> {
        let fence = scope.fence?;
        let address = self.address(fence.head);
        if target == Target::Block(fence.repeat) {
            return Some(format!("continue 'loop_{address:x};"));
        }
        if Some(target) == fence.after { return Some(format!("break 'loop_{address:x};")); }
        None
    }

    fn transfer(&self, target: Target) -> String {
        match target {
            Target::Outside(Some(pc)) => format!("transfer({}0x{pc:x});", self.label),
            Target::Outside(None) => "transfer(unresolved_address);".into(),
            Target::Block(node) => format!("transfer({}0x{:x});", self.label, self.address(node)),
        }
    }

    fn region(&mut self, mut target: Target, scope: Scope<'_>, depth: usize) -> Result<(), ()> {
        if depth > 96 { return Err(()); }
        loop {
            if let Some(line) = self.fence_transfer(target, scope) {
                self.line(depth, &line);
                return Ok(());
            }
            let node = match target {
                Target::Block(n) if scope.members.contains(&n) => n,
                Target::Block(n) if self.function.contains(&n) => return Err(()),
                _ => { self.line(depth, &self.transfer(target)); return Ok(()); }
            };
            if Some(node) == scope.stop { return Ok(()); }
            if !self.seen.insert(node) { return Err(()); }
            if let Some(region) = self.structure.loops.get(&node) {
                if !region.members.is_subset(scope.members) { return Err(()); }
                let Some(next) = self.emit_loop(node, region, depth)? else { return Ok(()) };
                target = next;
                continue;
            }
            let block = &self.graph.blocks[node];
            for line in &block.lines { self.line(depth, line); }
            match &block.end {
                End::Next(next) => target = *next,
                End::Stop { text, .. } => { self.line(depth, text); return Ok(()); }
                End::If { predicate, yes, no } => {
                    if let Some(&dispatch) = self.graph.guarded_switches.get(&node) {
                        if !scope.members.contains(&dispatch) || Some(dispatch) == scope.stop
                            || !self.seen.insert(dispatch)
                        {
                            return Err(());
                        }
                        let table = &self.graph.blocks[dispatch];
                        let Some(next) = self.switch_region(node, table, Some(*yes), scope, depth)? else { return Ok(()) };
                        target = Target::Block(next);
                        continue;
                    }
                    let (yes_action, no_action) = (self.fence_transfer(*yes, scope), self.fence_transfer(*no, scope));
                    if yes_action.is_some() != no_action.is_some() {
                        let (test, action, next) = match (yes_action, no_action) {
                            (Some(action), None) => (predicate.clone(), action, *no),
                            (None, Some(action)) => (format!("!({predicate})"), action, *yes),
                            _ => unreachable!(),
                        };
                        self.line(depth, &format!("if ({test}) {{"));
                        self.line(depth + 1, &action);
                        self.line(depth, "}");
                        target = next;
                        continue;
                    }
                    let join = self.join(node, scope);
                    let inner = Scope { stop: join.or(scope.stop), ..scope };
                    self.line(depth, &format!("if ({predicate}) {{"));
                    self.region(*yes, inner, depth + 1)?;
                    if Some(*no) != inner.stop.map(Target::Block) || self.fence_transfer(*no, inner).is_some() {
                        self.line(depth, "} else {");
                        self.region(*no, inner, depth + 1)?;
                    }
                    self.line(depth, "}");
                    let Some(next) = join else { return Ok(()) };
                    target = Target::Block(next);
                }
                End::Switch { .. } => {
                    let Some(next) = self.switch_region(node, block, None, scope, depth)? else { return Ok(()) };
                    target = Target::Block(next);
                }
            }
        }
    }

    fn switch_region(
        &mut self,
        head: Node,
        table: &Block,
        guard_default: Option<Target>,
        scope: Scope<'_>,
        depth: usize,
    ) -> Result<Option<Node>, ()> {
        let End::Switch { selector, cases, default } = &table.end else { return Err(()) };
        let join = self.join(head, scope);
        let inner = Scope { stop: join.or(scope.stop), ..scope };
        self.line(depth, &format!("switch ({selector}) {{"));
        for (values, to) in grouped_cases(cases) {
            for value in values { self.line(depth + 1, &format!("case {value}:")); }
            self.line(depth + 1, "{");
            if guard_default.is_some() {
                // The branch address is computed only on an in-range path.
                for line in &table.lines { self.line(depth + 2, line); }
            }
            self.region(to, inner, depth + 2)?;
            self.line(depth + 2, "break;");
            self.line(depth + 1, "}");
        }
        if let Some(target) = guard_default {
            self.line(depth + 1, "default: {");
            self.region(target, inner, depth + 2)?;
            self.line(depth + 2, "break;");
            self.line(depth + 1, "}");
        } else if !default.is_empty() {
            self.line(depth + 1, "default:");
            self.line(depth + 2, default);
        }
        self.line(depth, "}");
        Ok(join)
    }

    fn join(&self, node: Node, scope: Scope<'_>) -> Option<Node> {
        let join = self.structure.postdom[node];
        if !scope.members.contains(&join) || self.seen.contains(&join)
            || self.fence_transfer(Target::Block(join), scope).is_some()
        {
            return None;
        }
        if let Some(stop) = scope.stop.filter(|&stop| stop != join) {
            let before_join = reachable(node, &self.graph.successors, |n| {
                n != join && scope.members.contains(&n)
                    && self.fence_transfer(Target::Block(n), scope).is_none()
            });
            if before_join.contains(&stop) { return None; }
        }
        Some(join)
    }

    fn emit_loop(&mut self, head: Node, region: &Loop, depth: usize) -> Result<Option<Target>, ()> {
        let address = self.address(head);
        let block = &self.graph.blocks[head];
        let mut repeat = head;
        let mut tail = None;
        let body = match &region.kind {
            LoopKind::Head { body, condition, update: Some(update) } => {
                repeat = *self.graph.predecessors[head].iter().find(|id| region.members.contains(id)).ok_or(())?;
                if !self.seen.insert(repeat) { return Err(()); }
                self.line(depth, &format!("'loop_{address:x}: for (; {}; {update}) {{", evaluated(&block.lines, condition)));
                Some(Target::Block(*body))
            }
            LoopKind::Head { body, condition, update: None } => {
                self.line(depth, &format!("'loop_{address:x}: while ({}) {{", evaluated(&block.lines, condition)));
                Some(Target::Block(*body))
            }
            LoopKind::Tail { latch, condition } => {
                repeat = *latch;
                if repeat != head && !self.seen.insert(repeat) { return Err(()); }
                self.line(depth, &format!("'loop_{address:x}: do {{"));
                let lines = &self.graph.blocks[repeat].lines;
                if repeat == head {
                    for line in lines { self.line(depth + 1, line); }
                    tail = Some(condition.clone());
                    None
                } else {
                    tail = Some(evaluated(lines, condition));
                    for line in &block.lines { self.line(depth + 1, line); }
                    self.emit_loop_head_end(head, region, repeat, depth + 1)?;
                    None
                }
            }
            LoopKind::Endless => {
                self.line(depth, &format!("'loop_{address:x}: while (true) {{"));
                for line in &block.lines { self.line(depth + 1, line); }
                self.emit_loop_head_end(head, region, repeat, depth + 1)?;
                None
            }
        };
        if let Some(body) = body {
            self.region(body, Scope { members: &region.members, stop: None,
                fence: Some(Fence { head, repeat, after: region.after }) }, depth + 1)?;
        }
        if let Some(condition) = tail {
            self.line(depth, &format!("}} while ({condition});"));
        } else { self.line(depth, "}"); }
        Ok(region.after)
    }

    fn emit_loop_head_end(&mut self, head: Node, region: &Loop, repeat: Node, depth: usize) -> Result<(), ()> {
        let scope = Scope { members: &region.members, stop: None,
            fence: Some(Fence { head, repeat, after: region.after }) };
        match &self.graph.blocks[head].end {
            End::Next(next) => self.region(*next, scope, depth),
            End::If { predicate, yes, no } => {
                self.line(depth, &format!("if ({predicate}) {{"));
                self.region(*yes, scope, depth + 1)?;
                self.line(depth, "} else {");
                self.region(*no, scope, depth + 1)?;
                self.line(depth, "}");
                Ok(())
            }
            _ => Err(()),
        }
    }
}


struct Layout {
    order: Vec<Node>,
    heads: BTreeSet<Node>,
    boundaries: BTreeSet<Node>,
}

impl Layout {
    fn build(graph: &Graph, structure: &Structure, entry: Node, members: &BTreeSet<Node>, loop_head: Option<Node>) -> Result<Self, ()> {
        let mut children: Vec<_> = structure.loops.iter().filter(|(&head, child)| {
            Some(head) != loop_head && child.natural.is_subset(members)
        }).collect();
        children.sort_by_key(|(_, child)| std::cmp::Reverse(child.natural.len()));
        let mut unit = vec![NO_NODE; graph.blocks.len()];
        let mut heads = BTreeSet::new();
        for (&head, child) in children {
            if unit[head] != NO_NODE { continue; }
            heads.insert(head);
            for &node in &child.natural { unit[node] = head; }
        }
        for &node in members {
            if unit[node] == NO_NODE { unit[node] = node; }
        }
        let mut indegree = vec![0; graph.blocks.len()];
        let mut edges = vec![BTreeSet::new(); graph.blocks.len()];
        for &from in members {
            for &to in &graph.successors[from] {
                if unit[to] != NO_NODE && unit[from] != unit[to] && Some(to) != loop_head
                    && edges[unit[from]].insert(unit[to])
                {
                    indegree[unit[to]] += 1;
                }
            }
        }
        let units: BTreeSet<_> = members.iter().copied().filter(|&node| unit[node] == node).collect();
        let mut ready: BTreeSet<_> = units.iter().copied().filter(|&node| indegree[node] == 0).collect();
        let mut order = Vec::new();
        while let Some(node) = ready.pop_first() {
            order.push(node);
            for &to in &edges[node] {
                indegree[to] -= 1;
                if indegree[to] == 0 { ready.insert(to); }
            }
        }
        if order.first() != Some(&entry) || order.len() != units.len() { return Err(()); }
        let mut boundaries = BTreeSet::new();
        for (index, &node) in order.iter().enumerate() {
            for &to in &edges[node] {
                if heads.contains(&node) || Some(&to) != order.get(index + 1) {
                    boundaries.insert(to);
                }
            }
        }
        Ok(Self { order, heads, boundaries })
    }
}

impl Renderer<'_> {
    // Lexical forward exits retain shared tails without duplicating statements
    // or routing a reducible region through a program counter.
    fn lexical(
        &mut self,
        entry: Node,
        members: &BTreeSet<Node>,
        loop_head: Option<Node>,
        active_loops: &mut Vec<Node>,
        forward_blocks: &mut BTreeSet<Node>,
        mut depth: usize,
    ) -> Result<(), ()> {
        let layout = Layout::build(self.graph, self.structure, entry, members, loop_head)?;
        for &node in layout.order.iter().rev().filter(|node| layout.boundaries.contains(node)) {
            self.line(depth, &format!("'block_{:x}: {{", self.address(node)));
            forward_blocks.insert(node);
            depth += 1;
        }
        for (index, &node) in layout.order.iter().enumerate() {
            if layout.boundaries.contains(&node) {
                forward_blocks.remove(&node);
                depth -= 1;
                self.line(depth, "}");
            }
            if layout.heads.contains(&node) {
                self.line(depth, &format!("'loop_{:x}: while (true) {{", self.address(node)));
                active_loops.push(node);
                self.lexical(node, &self.structure.loops[&node].natural, Some(node), active_loops, forward_blocks, depth + 1)?;
                active_loops.pop();
                self.line(depth, "}");
                continue;
            }
            if !self.seen.insert(node) { return Err(()); }
            let next = layout.order.get(index + 1).copied();
            let edge = |renderer: &mut Self, target, depth| {
                if let Target::Block(id) = target {
                    if active_loops.contains(&id) {
                        renderer.line(depth, &format!("continue 'loop_{:x};", renderer.address(id)));
                        return Ok(());
                    }
                    if Some(id) == next { return Ok(()); }
                    if forward_blocks.contains(&id) {
                        renderer.line(depth, &format!("break 'block_{:x};", renderer.address(id)));
                        return Ok(());
                    }
                    if renderer.function.contains(&id) { return Err(()); }
                }
                renderer.line(depth, &renderer.transfer(target));
                Ok(())
            };
            let block = &self.graph.blocks[node];
            for line in &block.lines { self.line(depth, line); }
            self.emit_end(&block.end, depth, edge, true)?;
        }
        Ok(())
    }

    fn lexical_function(&mut self, entry: Node) -> Result<(), ()> {
        self.lexical(entry, self.function, None, &mut Vec::new(), &mut BTreeSet::new(), 1)
    }

    fn emit_end(
        &mut self,
        end: &End,
        depth: usize,
        mut edge: impl FnMut(&mut Self, Target, usize) -> Result<(), ()>,
        case_break: bool,
    ) -> Result<(), ()> {
        match end {
            End::Next(target) => edge(self, *target, depth)?,
            End::Stop { text, .. } => self.line(depth, text),
            End::If { predicate, yes, no } => {
                self.line(depth, &format!("if ({predicate}) {{"));
                edge(self, *yes, depth + 1)?;
                self.line(depth, "} else {");
                edge(self, *no, depth + 1)?;
                self.line(depth, "}");
            }
            End::Switch { selector, cases, default } => {
                self.line(depth, &format!("switch ({selector}) {{"));
                for (values, target) in grouped_cases(cases) {
                    for value in values { self.line(depth + 1, &format!("case {value}:")); }
                    edge(self, target, depth + 2)?;
                    if case_break { self.line(depth + 2, "break;"); }
                }
                if !default.is_empty() {
                    self.line(depth + 1, "default:");
                    self.line(depth + 2, default);
                }
                self.line(depth, "}");
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form { Structured, Dispatch, Goto }

impl Renderer<'_> {
    fn routed(&mut self, entry: Node, form: Form) {
        let dispatch = form == Form::Dispatch;
        let order: Vec<_> = std::iter::once(entry)
            .chain(self.function.iter().copied().filter(|&node| node != entry)).collect();
        let mut labels = BTreeSet::new();
        if dispatch {
            self.line(1, &format!("let next_block = {:#x};", self.address(entry)));
            self.line(1, "while (true) {");
            self.line(2, "switch (next_block) {");
        } else {
            for (index, &node) in order.iter().enumerate() {
                for &to in &self.graph.successors[node] {
                    if self.function.contains(&to) && Some(&to) != order.get(index + 1) {
                        labels.insert(to);
                    }
                }
            }
        }
        let depth = if dispatch { 3 } else { 1 };
        for (index, &node) in order.iter().enumerate() {
            let address = self.address(node);
            if dispatch {
                self.line(depth, &format!("case 0x{address:x}: {{"));
            } else if labels.contains(&node) {
                self.line(depth, &format!("L_{address:x}:"));
            }
            let fallthrough = if dispatch { None } else { order.get(index + 1).copied() };
            let block = &self.graph.blocks[node];
            let body_depth = depth + usize::from(dispatch);
            for line in &block.lines { self.line(body_depth, line); }
            let _ = self.emit_end(&block.end, body_depth, |renderer, target, depth| {
                if Some(target) != fallthrough.map(Target::Block) {
                    renderer.routed_edge(target, dispatch, depth);
                }
                Ok(())
            }, !dispatch);
            if dispatch { self.line(depth, "}"); }
        }
        if dispatch {
            self.line(2, "}");
            self.line(1, "}");
        }
    }

    fn routed_edge(&mut self, target: Target, dispatch: bool, depth: usize) {
        if let Target::Block(node) = target {
            if self.function.contains(&node) {
                let address = self.address(node);
                if dispatch {
                    self.line(depth, &format!("next_block = 0x{address:x};"));
                    self.line(depth, "continue;");
                } else {
                    self.line(depth, &format!("goto L_{address:x};"));
                }
                return;
            }
        }
        self.line(depth, &self.transfer(target));
    }
}

pub(super) struct Reconstruction {
    pub(super) summary: String,
    functions: Vec<String>,
    count: usize,
    pub(super) available_bytes: usize,
}

impl Reconstruction {
    pub(super) fn write(&self, out: &mut String, limit: usize) {
        const NOTE: &str = "The instructions were not executed. This is reconstructed text.\n";
        let mut printed = 0;
        out.push_str(&self.summary);
        for text in &self.functions {
            if out.len() + text.len() + NOTE.len() + 80 <= limit {
                out.push_str(text);
                printed += 1;
            }
        }
        if printed < self.count {
            let _ = writeln!(out, "Structured function bodies omitted by the display bound: {}.", self.count - printed);
        }
        out.push_str(NOTE);
    }
}

pub(super) fn reconstruct(
    words: &[Word],
    starts: &BTreeSet<u64>,
    label: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Reconstruction, String> {
    let graph = Graph::build(words, starts, label);
    let mut covered = BTreeSet::new();
    let mut functions = Vec::new();
    let mut available_bytes = 0;
    let mut entries: [Vec<u64>; 4] = Default::default();
    // Direct call destinations have priority. For the remaining capture,
    // graph roots and then uncovered regions are candidate function entries.
    let candidates = graph.entries.iter().copied()
        .chain((0..graph.blocks.len()).filter(|&node| graph.predecessors[node].is_empty()))
        .chain(0..graph.blocks.len());
    for entry in candidates {
        if cancelled() { return Err("Inspection cancelled during control-flow reconstruction".into()); }
        if covered.contains(&entry) { continue; }
        let members = reachable(entry, &graph.successors, |node| node == entry || !graph.entries.contains(&node));
        let shape = Structure::discover(&graph, entry, &members);
        let mut renderer = Renderer { graph: &graph, structure: &shape, function: &members,
            label, text: String::new(), seen: BTreeSet::new() };
        let address = graph.blocks[entry].address;
        renderer.line(0, &format!("function {label}0x{address:x} {{"));
        let prefix = renderer.text.len();
        let form = if shape.irreducible {
            Form::Goto
        } else if renderer.region(Target::Block(entry), Scope { members: &members, stop: None, fence: None }, 1).is_ok()
            && renderer.seen == members
        {
            Form::Structured
        } else {
            renderer.text.truncate(prefix);
            renderer.seen.clear();
            if renderer.lexical_function(entry).is_ok() && renderer.seen == members {
                Form::Structured
            } else {
                Form::Dispatch
            }
        };
        if form != Form::Structured {
            renderer.text.truncate(prefix);
            renderer.routed(entry, form);
        }
        renderer.line(0, "}");
        let incomplete = members.iter().any(|&node| graph.blocks[node].end.incomplete()
            || graph.blocks[node].end.targets().iter()
                .any(|target| matches!(target, Target::Block(n) if !members.contains(n))));
        let group = match form {
            Form::Structured if !incomplete => 0,
            Form::Structured => 1,
            Form::Dispatch => 2,
            Form::Goto => 3,
        };
        entries[group].push(address);
        if incomplete && matches!(form, Form::Dispatch | Form::Goto) {
            renderer.text.insert_str(prefix, "  // Capture contains unresolved or external transfers.\n");
        }
        if available_bytes + renderer.text.len() <= super::OUTPUT {
            available_bytes += renderer.text.len();
            functions.push(renderer.text);
        }
        covered.extend(members);
    }
    let count = entries.iter().map(Vec::len).sum();
    let mut summary = format!("\nStructured reconstruction over {count} candidate functions (coordinates in {label}namespace).\n");
    for (kind, addresses) in ["Fully structured", "Structured with incomplete capture",
        "Structured block dispatch", "Labelled goto fallback (irreducible)"].into_iter().zip(entries)
    {
        let _ = write!(summary, "{kind} ({}):", addresses.len());
        if addresses.is_empty() { summary.push_str(" none"); }
        for address in addresses { let _ = write!(summary, " 0x{address:x}"); }
        summary.push('\n');
    }
    if count != 0 {
        summary.push_str("Register names denote architectural cells. W-register writes zero-extend into their 64-bit aliases.\n");
    }
    Ok(Reconstruction { summary, functions, count, available_bytes })
}

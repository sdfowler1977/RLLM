//! Dalvik method text and the package executable that holds it.
//!
//! Research: the Dalvik executable format and bytecode (string, type, prototype,
//! method and class items, MUTF-8, ULEB128, instruction formats 10x through 51l).

use super::*;

const NO_INDEX: u32 = 0xffff_ffff;

pub(crate) fn disassemble(dex: &[u8]) -> Result<String> {
    let file = parse(dex)?;
    let mut out = String::new();
    for class in &file.classes {
        let flags = access_text(class.access, AccessKind::Class);
        out.push_str(&format!(".class {flags}{}\n", file.types[class.type_idx]));
        out.push_str(&format!(".super {}\n", file.types[class.super_idx]));
        for interface in &class.interfaces {
            out.push_str(&format!(".implements {}\n", file.types[*interface]));
        }
        if let Some(source) = class.source {
            out.push_str(&format!(".source \"{}\"\n", escape(&file.strings[source])));
        }
        for field in class.fields.iter() {
            let flags = access_text(field.access, AccessKind::Field);
            let (owner, name, ty) = file.fields[field.idx];
            if owner != class.type_idx {
                continue;
            }
            let mut line = format!(".field {flags}{}:{}", file.strings[name], file.types[ty]);
            // A static field carries its initial value in the class data
            // item. The value is written the way the reference writes it,
            // so the text stays comparable and reads back through the
            // assembler.
            if let Some(value) = field.value.as_ref() {
                if let Some(text) = value_text(&file, value)? {
                    line.push_str(" = ");
                    line.push_str(&text);
                }
            }
            out.push_str(&line);
            out.push('\n');
        }
        for method in class.methods.iter() {
            emit_method(&file, class, method, &mut out)?;
        }
        out.push('\n');
    }
    Ok(out)
}

pub(crate) fn assemble(text: &str) -> Result<Vec<u8>> {
    let file = parse_text(text)?;
    emit(&file)
}

fn emit_method(file: &Dex, class: &Class, method: &Method, out: &mut String) -> Result<()> {
    let (owner, proto, name) = file.methods[method.idx];
    if owner != class.type_idx {
        return Ok(());
    }
    let (ret, args) = &file.protos[proto];
    let mut desc = String::from("(");
    for arg in args {
        desc.push_str(&file.types[*arg]);
    }
    desc.push(')');
    desc.push_str(&file.types[*ret]);
    let flags = access_text(method.access, AccessKind::Method);
    out.push_str(&format!(".method {flags}{}{desc}\n", file.strings[name]));
    let Some(code) = method.code.as_ref() else {
        out.push_str(".end method\n");
        return Ok(());
    };
    out.push_str(&format!("    .registers {}\n", code.registers));
    let mut labels = std::collections::BTreeSet::new();
    for insn in &code.insns {
        match insn {
            Insn::Op { target: Some(target), .. } => {
                // A branch names the place it jumps to.
                labels.insert(*target);
            }
            Insn::Switch(_) => {
                // A switch names the place its payload table sits, and every
                // case the table lists jumps to a place of its own.
            }
            Insn::SwitchData(table) => {
                // Every case the table lists jumps to a place of its own.
                // The place the table sits at carries the name of the table
                // and not the ordinary label.
                for target in &table.targets {
                    labels.insert(*target);
                }
            }
            _ => {}
        }
    }
    for catch in &code.catches {
        labels.insert(catch.start);
        labels.insert(catch.end);
        labels.insert(catch.handler);
    }
    // A payload table sits at a place of its own and carries a name of
    // its own, so the place is named by the table and not by the ordinary
    // label.
    let mut payloads: std::collections::BTreeMap<u32, SwitchKind> = std::collections::BTreeMap::new();
    for insn in &code.insns {
        if let Insn::SwitchData(table) = insn {
            payloads.insert(table.base, table.kind);
        }
    }
    let mut at = 0u32;
    for insn in &code.insns {
        if labels.contains(&at) {
            out.push_str(&format!("    {}\n", label_text(at)));
        }
        if let Some(kind) = payloads.get(&at).copied() {
            out.push_str(&format!("    {}\n", payload_label(kind, at)));
        }
        out.push_str("    ");
        out.push_str(&insn_text(file, insn)?);
        out.push('\n');
        at += insn.width();
    }
    if labels.contains(&at) {
        out.push_str(&format!("    {}\n", label_text(at)));
    }
    for catch in &code.catches {
        match &catch.ty {
            Some(ty) => out.push_str(&format!(
                "    .catch {} {{{} .. {}}} {}\n",
                file.types[*ty], label_text(catch.start), label_text(catch.end), label_text(catch.handler)
            )),
            None => out.push_str(&format!(
                "    .catch-all {{{} .. {}}} {}\n",
                label_text(catch.start), label_text(catch.end), label_text(catch.handler)
            )),
        }
    }
    out.push_str(".end method\n");
    Ok(())
}

fn insn_text(file: &Dex, insn: &Insn) -> Result<String> {
    Ok(match insn {
        Insn::Raw(words) => words.iter().map(|w| format!(".unit 0x{w:04x}")).collect::<Vec<_>>().join("\n    "),
        Insn::Switch(payload) => {
            // A switch names the register it reads its key from and the
            // label of the payload table it dispatches through. The table
            // sits at a place of its own, so that place names the table.
            let name = match payload.kind {
                SwitchKind::Packed => "packed-switch",
                SwitchKind::Sparse => "sparse-switch",
            };
            format!("{} v{}, {}", name, payload.reg, payload_label(payload.kind, payload.payload_at))
        }
        Insn::SwitchData(table) => {
            // The payload is written as a block with a label on every case
            // it lists, the way the reference disassembler writes it.
            let mut lines = Vec::new();
            match table.kind {
                SwitchKind::Packed => {
                    lines.push(format!(".packed-switch {}", hex_int(table.first_key)));
                    for target in &table.targets {
                        lines.push(format!("    {}", label_text(*target)));
                    }
                    lines.push(".end packed-switch".to_string());
                }
                SwitchKind::Sparse => {
                    lines.push(".sparse-switch".to_string());
                    for (key, target) in table.keys.iter().zip(table.targets.iter()) {
                        lines.push(format!("    {} -> {}", hex_int(*key), label_text(*target)));
                    }
                    lines.push(".end sparse-switch".to_string());
                }
            }
            lines.join("\n    ")
        }
        Insn::Op { name, regs, imm, target, index } => {
            let mut line = name.clone();
            if name.starts_with("invoke-") || name.starts_with("filled-new-array") {
                let shown = regs.iter().map(|r| format!("v{r}")).collect::<Vec<_>>().join(", ");
                line.push_str(&format!(" {{{shown}}}"));
            } else if !regs.is_empty() {
                line.push(' ');
                line.push_str(&regs.iter().map(|r| format!("v{r}")).collect::<Vec<_>>().join(", "));
            }
            if let Some(imm) = imm {
                if !regs.is_empty() {
                    line.push_str(", ");
                }
                line.push_str(&format!("0x{imm:x}"));
            }
            if let Some(target) = target {
                if !regs.is_empty() || imm.is_some() {
                    line.push_str(", ");
                } else if !line.ends_with(' ') {
                    // A branch without registers still needs the separator,
                    // otherwise the target runs into the mnemonic and the
                    // text no longer reads back as an instruction.
                    line.push(' ');
                }
                line.push_str(&label_text(*target));
            }
            if let Some((kind, idx)) = index {
                if !line.ends_with(' ') && line != *name {
                    line.push_str(", ");
                } else if line == *name {
                    line.push(' ');
                }
                line.push_str(&match *kind {
                    's' => format!("\"{}\"", escape(&file.strings[*idx])),
                    't' => file.types[*idx].clone(),
                    'f' => field_text(file, *idx),
                    'm' => method_text(file, *idx),
                    _ => return Err(format!("Unknown reference kind {kind}")),
                });
            }
            line
        }
    })
}

/// The name a label carries in the method text. Every place the method
/// text names an instruction offset uses this one spelling, so a branch,
/// a payload case and a catch range all name the same place the same way
/// and the text reads back through the assembler. The offset is written in
/// hexadecimal, the way the reference writes it.
fn label_text(offset: u32) -> String {
    format!(":l{:x}", offset)
}

/// The name the payload table of a switch carries in the method text. The
/// table sits at a place of its own in the instruction stream, so it is
/// named by that place rather than by an instruction offset.
fn payload_label(kind: SwitchKind, at: u32) -> String {
    match kind {
        SwitchKind::Packed => format!(":pswitch_data_{:x}", at),
        SwitchKind::Sparse => format!(":sswitch_data_{:x}", at),
    }
}

/// Writes the initial value of a static field the way the reference writes
/// it.
fn value_text(file: &Dex, value: &FieldValue) -> Result<Option<String>> {
    Ok(Some(match value {
        // A value that is null carries no text: the reference writes the
        // field line and stops there, so the line carries no suffix.
        FieldValue::Null => return Ok(None),
        FieldValue::Number { kind, raw, width } => number_text(*kind, *raw, *width),
        FieldValue::Boolean(v) => if *v { "true" } else { "false" }.to_string(),
        FieldValue::String(idx) => format!("\"{}\"", escape(&file.strings[*idx])),
        FieldValue::Type(idx) => file.types[*idx].clone(),
        FieldValue::Field(idx) => field_text(file, *idx),
        FieldValue::Enum(idx) => field_text(file, *idx),
        FieldValue::Method(idx) => method_text(file, *idx),
        FieldValue::MethodType(idx) => proto_text(file, *idx),
        FieldValue::MethodHandle(idx) => format!("method-handle-{}", idx),
        FieldValue::Array(items) => {
            let mut out = String::from("{");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                match value_text(file, item)? {
                    Some(text) => out.push_str(&text),
                    None => out.push_str("null"),
                }
            }
            out.push('}');
            out
        }
        FieldValue::Annotation { type_idx, elements } => {
            let mut out = format!("@{} {{", file.types[*type_idx]);
            for (i, (name, item)) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let text = value_text(file, item)?.unwrap_or_else(|| "null".to_string());
                out.push_str(&format!("{} = {}", file.strings[*name], text));
            }
            out.push('}');
            out
        }
    }))
}

/// Writes a number the way the reference writes it. A whole number is
/// written in unsigned hexadecimal, a long number carries the long mark, a
/// floating point number is written as the number itself and a character is
/// written as its number.
fn number_text(kind: u8, raw: u64, width: usize) -> String {
    match kind {
        0x03 => format!("0x{:x}", raw),
        0x04 => format!("0x{:x}", raw as u32),
        0x06 => format!("0x{:x}L", raw),
        0x10 => {
            let bits = (raw as u32) << ((4 - width.min(4)) * 8);
            format!("{}f", f32::from_bits(bits))
        }
        0x11 => {
            let bits = raw << ((8 - width.min(8)) * 8);
            format!("{}", f64::from_bits(bits))
        }
        0x00 | 0x02 => format!("0x{:x}", sign_extend_value(raw, width) as u32),
        _ => format!("0x{:x}", raw as u32),
    }
}

/// Writes a 32-bit number the way the reference writes it, in unsigned
/// hexadecimal.
fn hex_int(value: i32) -> String {
    format!("0x{:x}", value as u32)
}

/// Writes a 64-bit number the way the reference writes it, in unsigned
/// hexadecimal with the long mark.
fn hex_long(value: i64) -> String {
    format!("0x{:x}L", value as u64)
}

fn proto_text(file: &Dex, idx: usize) -> String {
    let (ret, args) = &file.protos[idx];
    let mut desc = String::from("(");
    for arg in args {
        desc.push_str(&file.types[*arg]);
    }
    desc.push(')');
    desc.push_str(&file.types[*ret]);
    desc
}

fn field_text(file: &Dex, idx: usize) -> String {
    let (owner, name, ty) = file.fields[idx];
    format!("{}->{}:{}", file.types[owner], file.strings[name], file.types[ty])
}

fn method_text(file: &Dex, idx: usize) -> String {
    let (owner, proto, name) = file.methods[idx];
    let (ret, args) = &file.protos[proto];
    let mut desc = String::from("(");
    for arg in args {
        desc.push_str(&file.types[*arg]);
    }
    desc.push(')');
    desc.push_str(&file.types[*ret]);
    format!("{}->{}{}", file.types[owner], file.strings[name], desc)
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(text: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => return Err("Truncated escape".into()),
        }
    }
    Ok(out)
}

#[derive(Clone)]
struct Dex {
    strings: Vec<String>,
    types: Vec<String>,
    protos: Vec<(usize, Vec<usize>)>,
    fields: Vec<(usize, usize, usize)>,
    methods: Vec<(usize, usize, usize)>,
    classes: Vec<Class>,
}

#[derive(Clone)]
struct Class {
    type_idx: usize,
    access: u32,
    super_idx: usize,
    source: Option<usize>,
    interfaces: Vec<usize>,
    fields: Vec<Member>,
    methods: Vec<Method>,
}

#[derive(Clone)]
struct Member {
    idx: usize,
    access: u32,
    /// The initial value of a static field, read from the class data item.
    /// A value that names a string, a type, a field or a method keeps the
    /// number it holds in the identifier table, so the value survives the
    /// renumbering of those tables.
    value: Option<FieldValue>,
}

/// The initial value of a static field, read from the class data item.
/// A value that names a string, a type, a field or a method keeps the
/// number it holds in the identifier table, so the value can be written
/// back after the tables are renumbered.
#[derive(Clone, PartialEq)]
enum FieldValue {
    Null,
    Boolean(bool),
    /// A number keeps the bytes it was stored in, so the value can be
    /// written back in the same shape it was read in.
    Number { kind: u8, raw: u64, width: usize },
    String(usize),
    Type(usize),
    Field(usize),
    Method(usize),
    Enum(usize),
    MethodType(usize),
    MethodHandle(usize),
    Array(Vec<FieldValue>),
    Annotation {
        type_idx: usize,
        elements: Vec<(usize, FieldValue)>,
    },
}

#[derive(Clone)]
struct Method {
    idx: usize,
    access: u32,
    code: Option<Code>,
}

#[derive(Clone)]
struct Code {
    registers: u16,
    ins: u16,
    insns: Vec<Insn>,
    catches: Vec<Catch>,
}

#[derive(Clone)]
struct Catch {
    start: u32,
    end: u32,
    handler: u32,
    ty: Option<usize>,
}

#[derive(Clone)]
enum Insn {
    Raw(Vec<u16>),
    Op {
        name: String,
        regs: Vec<u8>,
        imm: Option<i64>,
        target: Option<u32>,
        index: Option<(char, usize)>,
    },
    /// A switch instruction. It reads its key from one register and names
    /// the payload table it dispatches through by the place the table sits
    /// in the instruction stream.
    Switch(SwitchInsn),
    /// The payload table a switch instruction reads. It sits at its own
    /// place in the instruction stream.
    SwitchData(SwitchData),
}

#[derive(Clone, Copy, PartialEq)]
enum SwitchKind {
    Packed,
    Sparse,
}

/// A switch instruction. It reads its key from one register and names the
/// payload table it dispatches through by the place the table sits in the
/// instruction stream.
#[derive(Clone)]
struct SwitchInsn {
    kind: SwitchKind,
    reg: u8,
    base: u32,
    payload_at: u32,
}

/// A decoded payload table. The keys are the case values and the targets are
/// the instruction offsets each case jumps to. The targets are counted from
/// the switch instruction that reads the table.
#[derive(Clone)]
struct SwitchData {
    kind: SwitchKind,
    /// The address the payload table sits at.
    base: u32,
    /// The address of the switch instruction that reads the table.
    switch_at: u32,
    first_key: i32,
    keys: Vec<i32>,
    targets: Vec<u32>,
}

impl Insn {
    fn width(&self) -> u32 {
        match self {
            Insn::Raw(words) => words.len() as u32,
            Insn::Op { name, .. } => named_width(name) as u32,
            // A switch instruction is 31t: three code units. Its payload is
            // a separate item that carries its own width.
            Insn::Switch(_) => 3,
            // The payload table carries its own width.
            Insn::SwitchData(payload) => switch_table_words(payload) as u32,
        }
    }
}

fn named_width(name: &str) -> usize {
    match name {
        "nop" | "return-void" | "return" | "return-wide" | "return-object" | "const/4" | "move-result"
        | "move-result-wide" | "move-result-object" | "move-exception" | "throw" | "monitor-enter"
        | "monitor-exit" | "array-length" | "goto" => 1,
        "const/16" | "const-string" | "const-class" | "check-cast" | "new-instance" | "sget" | "sget-object"
        | "sput" | "sput-object" | "iget" | "iget-object" | "iput" | "iput-object" | "instance-of"
        | "new-array" | "if-eq" | "if-ne" | "if-lt" | "if-ge" | "if-gt" | "if-le" | "if-eqz" | "if-nez"
        | "if-ltz" | "if-gez" | "if-gtz" | "if-lez" | "add-int/lit8" | "goto/16" => 2,
        "const" | "invoke-virtual" | "invoke-super" | "invoke-direct" | "invoke-static" | "invoke-interface"
        | "packed-switch" | "sparse-switch" => 3,
        "const-wide" => 5,
        _ => 1,
    }
}

fn parse(dex: &[u8]) -> Result<Dex> {
    if dex.len() < 112 || &dex[0..4] != b"dex\n" {
        return Err("Not a Dalvik executable".into());
    }
    let endian = u32_at(dex, 40)?;
    if endian != 0x1234_5678 {
        return Err("Only little-endian executables are read".into());
    }
    let size = u32_at(dex, 32)? as usize;
    if size != dex.len() {
        return Err("Executable size does not match the buffer".into());
    }
    let string_ids = table(dex, 56)?;
    let type_ids = table(dex, 64)?;
    let proto_ids = table(dex, 72)?;
    let field_ids = table(dex, 80)?;
    let method_ids = table(dex, 88)?;
    let class_defs = table(dex, 96)?;
    let mut strings = Vec::with_capacity(string_ids.0);
    for i in 0..string_ids.0 {
        let at = u32_at(dex, string_ids.1 + i * 4)? as usize;
        strings.push(read_mutf8(dex, at)?);
    }
    let mut types = Vec::with_capacity(type_ids.0);
    for i in 0..type_ids.0 {
        let id = u32_at(dex, type_ids.1 + i * 4)? as usize;
        types.push(strings.get(id).cloned().ok_or("Type string is missing")?);
    }
    let mut protos = Vec::with_capacity(proto_ids.0);
    for i in 0..proto_ids.0 {
        let at = proto_ids.1 + i * 12;
        let ret = u32_at(dex, at + 4)? as usize;
        let params = u32_at(dex, at + 8)? as usize;
        let args = if params == 0 { Vec::new() } else { read_type_list(dex, params)? };
        protos.push((ret, args));
    }
    let mut fields = Vec::with_capacity(field_ids.0);
    for i in 0..field_ids.0 {
        let at = field_ids.1 + i * 8;
        fields.push((u16_at(dex, at)? as usize, u32_at(dex, at + 4)? as usize, u16_at(dex, at + 2)? as usize));
    }
    let mut methods = Vec::with_capacity(method_ids.0);
    for i in 0..method_ids.0 {
        let at = method_ids.1 + i * 8;
        methods.push((u16_at(dex, at)? as usize, u16_at(dex, at + 2)? as usize, u32_at(dex, at + 4)? as usize));
    }
    let mut classes = Vec::with_capacity(class_defs.0);
    let mut static_values = Vec::with_capacity(class_defs.0);
    for i in 0..class_defs.0 {
        let at = class_defs.1 + i * 32;
        let source = u32_at(dex, at + 16)?;
        let static_values_off = u32_at(dex, at + 28)? as usize;
        let interfaces_off = u32_at(dex, at + 12)? as usize;
        let interfaces = if interfaces_off == 0 { Vec::new() } else { read_type_list(dex, interfaces_off)? };
        let data_off = u32_at(dex, at + 24)? as usize;
        let mut class = Class {
            type_idx: u32_at(dex, at)? as usize,
            access: u32_at(dex, at + 4)?,
            super_idx: u32_at(dex, at + 8)? as usize,
            source: if source == NO_INDEX { None } else { Some(source as usize) },
            interfaces,
            fields: Vec::new(),
            methods: Vec::new(),
        };
        if data_off != 0 {
            read_class_data(dex, data_off, &mut class, &fields, &methods)?;
        }
        static_values.push(static_values_off);
        classes.push(class);
    }
    let mut file = Dex { strings, types, protos, fields, methods, classes };
    // The class data item carries the initial values of the static fields in
    // the order the static field list declares them. Every value is read
    // before any class is changed, because a value can name a string, a
    // type, a field or a method, and those names live in the tables the
    // class list sits beside.
    let mut arrays = Vec::with_capacity(static_values.len());
    for off in &static_values {
        if *off == 0 {
            arrays.push(Vec::new());
        } else {
            arrays.push(read_encoded_array(dex, *off)?);
        }
    }
    for (class, values) in file.classes.iter_mut().zip(arrays) {
        if values.is_empty() {
            continue;
        }
        // The initial values line up with the static fields in the order
        // the class data item declares them.
        let statics = class.fields.iter().filter(|f| f.access & 0x8 != 0).map(|f| f.idx).collect::<Vec<_>>();
        for (value, idx) in values.into_iter().zip(statics) {
            if let Some(field) = class.fields.iter_mut().find(|f| f.idx == idx) {
                field.value = Some(value);
            }
        }
    }
    Ok(file)
}

fn read_class_data(
    dex: &[u8],
    mut at: usize,
    class: &mut Class,
    fields: &[(usize, usize, usize)],
    methods: &[(usize, usize, usize)],
) -> Result<()> {
    let static_fields = uleb(dex, &mut at)?;
    let instance_fields = uleb(dex, &mut at)?;
    let direct = uleb(dex, &mut at)?;
    let virtuals = uleb(dex, &mut at)?;
    // The static and the instance field lists are separate encoded lists.
    // Each list carries its own running field index: the first entry of a list
    // states its index outright and every later entry states the distance from
    // the entry before it. The index restarts with each list, so one running
    // index cannot span both of them.
    for group in [static_fields, instance_fields] {
        let mut field_idx = 0u32;
        for _ in 0..group {
            field_idx = field_idx.checked_add(uleb(dex, &mut at)?).ok_or("Field index overflow")?;
            if field_idx as usize >= fields.len() {
                return Err("Field index is outside the field table".into());
            }
            let access = uleb(dex, &mut at)?;
            class.fields.push(Member { idx: field_idx as usize, access, value: None });
        }
    }
    // The direct and the virtual method lists are separate encoded lists with
    // the same rule: each list has its own running method index and the index
    // restarts with each list. The direct list holds the static, private and
    // constructor methods, the virtual list holds the rest. Reading both as
    // one list overstates the index and leaves the method table.
    for group in [direct, virtuals] {
        let mut method_idx = 0u32;
        for _ in 0..group {
            method_idx = method_idx.checked_add(uleb(dex, &mut at)?).ok_or("Method index overflow")?;
            let access = uleb(dex, &mut at)?;
            let code_off = uleb(dex, &mut at)? as usize;
            let code = if code_off == 0 { None } else { Some(read_code(dex, code_off)?) };
            if method_idx as usize >= methods.len() {
                return Err("Method index is outside the method table".into());
            }
            class.methods.push(Method { idx: method_idx as usize, access, code });
        }
    }
    Ok(())
}

/// Reads an encoded array item: a count followed by that many encoded
/// values.
fn read_encoded_array(dex: &[u8], at: usize) -> Result<Vec<FieldValue>> {
    let mut pos = at;
    let count = uleb(dex, &mut pos)? as usize;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        out.push(read_encoded_value(dex, &mut pos)?);
    }
    Ok(out)
}

/// Reads one encoded value. The first byte names the value type and
/// carries the argument the type needs, and the value itself follows in the
/// shape that type declares. A number is stored in as few bytes as it needs
/// and the argument names how many, so the number is read at that width and
/// then given the sign of the byte it was stored in.
fn read_encoded_value(dex: &[u8], at: &mut usize) -> Result<FieldValue> {
    let header = *dex.get(*at).ok_or("Encoded value is truncated")?;
    *at += 1;
    let kind = header & 0x1f;
    let arg = (header >> 5) as usize;
    Ok(match kind {
        // Every numeric kind stores the number in as many bytes as it needs
        // and the argument names how many. The bytes are kept as they are, so
        // the value can be written back in the same shape it was read in.
        0x00 | 0x02 | 0x03 | 0x04 | 0x06 | 0x10 | 0x11 => {
            let width = (arg + 1).min(8);
            let bytes = dex.get(*at..*at + width).ok_or("Encoded value is truncated")?;
            let mut raw = 0u64;
            for (place, byte) in bytes.iter().enumerate() {
                raw |= u64::from(*byte) << (8 * place);
            }
            *at += width;
            FieldValue::Number { kind, raw, width }
        }
        // A value that names a string, a type, a field or a method holds
        // the number it has in the table it belongs to. The number is stored
        // in as many bytes as it needs and the argument names how many, so
        // the number is read at that width.
        0x15 | 0x16 | 0x17 | 0x18 | 0x19 | 0x1a | 0x1b => {
            let width = (arg + 1).min(4);
            let bytes = dex.get(*at..*at + width).ok_or("Encoded value is truncated")?;
            let mut raw = 0u32;
            for (place, byte) in bytes.iter().enumerate() {
                raw |= u32::from(*byte) << (8 * place);
            }
            *at += width;
            let index = raw as usize;
            match kind {
                0x15 => FieldValue::MethodType(index),
                0x16 => FieldValue::MethodHandle(index),
                0x17 => FieldValue::String(index),
                0x18 => FieldValue::Type(index),
                0x19 => FieldValue::Field(index),
                0x1a => FieldValue::Method(index),
                _ => FieldValue::Enum(index),
            }
        }
        0x1c => {
            let count = uleb(dex, at)? as usize;
            let mut items = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                items.push(read_encoded_value(dex, at)?);
            }
            FieldValue::Array(items)
        }
        0x1d => {
            let type_idx = uleb(dex, at)? as usize;
            let count = uleb(dex, at)? as usize;
            let mut elements = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                let name = uleb(dex, at)? as usize;
                let value = read_encoded_value(dex, at)?;
                elements.push((name, value));
            }
            FieldValue::Annotation { type_idx, elements }
        }
        0x1e => FieldValue::Null,
        0x1f => FieldValue::Boolean(arg != 0),
        _ => return Err(format!("Unknown encoded value type {kind}")),
    })
}

/// Gives a number that was stored in fewer bytes than its type needs the
/// sign of the byte it was stored in.
fn sign_extend_value(raw: u64, width: usize) -> i64 {
    let bits = width.saturating_mul(8);
    if bits == 0 || bits >= 64 {
        return raw as i64;
    }
    let shift = 64 - bits;
    ((raw << shift) as i64) >> shift
}

fn read_code(dex: &[u8], at: usize) -> Result<Code> {
    let registers = u16_at(dex, at)?;
    let _ins = u16_at(dex, at + 2)?;
    let _outs = u16_at(dex, at + 4)?;
    let tries = u16_at(dex, at + 6)? as usize;
    let insns_size = u32_at(dex, at + 12)? as usize;
    let mut words = Vec::with_capacity(insns_size);
    for i in 0..insns_size {
        words.push(u16_at(dex, at + 16 + i * 2)?);
    }
    let insns = decode_insns(&words)?;
    let mut catches = Vec::new();
    if tries > 0 {
        let mut cursor = at + 16 + insns_size * 2;
        if insns_size % 2 == 1 {
            cursor += 2;
        }
        let list_at = cursor + tries * 8;
        let mut handlers = Vec::new();
        let mut pos = list_at;
        let count = uleb(dex, &mut pos)?;
        for _ in 0..count {
            let start = pos - list_at;
            let size = sleb(dex, &mut pos)?;
            let mut typed = Vec::new();
            let n = size.unsigned_abs();
            for _ in 0..n {
                let ty = uleb(dex, &mut pos)? as usize;
                let addr = uleb(dex, &mut pos)?;
                typed.push((Some(ty), addr));
            }
            if size <= 0 {
                let addr = uleb(dex, &mut pos)?;
                typed.push((None, addr));
            }
            handlers.push((start, typed));
        }
        for i in 0..tries {
            let base = cursor + i * 8;
            let start = u32_at(dex, base)?;
            let count = u16_at(dex, base + 4)? as u32;
            let off = u16_at(dex, base + 6)? as usize;
            let (_, typed) = handlers.iter().find(|(start, _)| *start == off).ok_or("Exception handler is missing")?;
            for (ty, addr) in typed {
                catches.push(Catch { start, end: start + count, handler: *addr, ty: *ty });
            }
        }
    }
    Ok(Code { registers, ins: _ins, insns, catches })
}

fn sign_nibble(b: u8) -> i8 { let v = (b & 0xf) as i8; if v & 8 != 0 { v | !0xf } else { v } }
fn sign_byte(b: u8) -> i8 { b as i8 }

fn decode_insns_fixed(words: &[u16]) -> Result<Vec<Insn>> {
    // Every switch instruction names the place its payload table sits. The
    // tables are read first so the instruction and the table carry the same
    // case list. A table may be read by more than one switch, so the table
    // is keyed by the place it sits at and the switch that reads it is
    // recorded on the table itself.
    let mut tables: std::collections::BTreeMap<usize, SwitchData> = std::collections::BTreeMap::new();
    for k in 0..words.len() {
        let op = words[k] & 0xff;
        if op != 0x2b && op != 0x2c {
            continue;
        }
        if k + 3 > words.len() {
            continue;
        }
        let offset = words[k + 1] as u32 | ((words[k + 2] as u32) << 16);
        let at = k as u32 + offset;
        if at as usize >= words.len() {
            continue;
        }
        if let Some(table) = read_switch_table(words, at as usize, k as u32) {
            tables.entry(at as usize).or_insert(table);
        }
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < words.len() {
        // A payload table is placed in the instruction stream at its own
        // address. It is read as one item and carries its own width.
        if let Some(table) = tables.get(&i) {
            out.push(Insn::SwitchData(table.clone()));
            i += switch_table_words(table);
            continue;
        }
        let op = (words[i] & 0xff) as u8;
        // A switch instruction is 31t: three code units. Its second and
        // third units carry the offset to the payload table.
        if op == 0x2b || op == 0x2c {
            if i + 3 > words.len() {
                return Err("Instruction is truncated".into());
            }
            let reg = (words[i] >> 8) as u8;
            let offset = words[i + 1] as u32 | ((words[i + 2] as u32) << 16);
            let payload_at = i as u32 + offset;
            match tables.get(&(payload_at as usize)) {
                Some(table) => {
                    out.push(Insn::Switch(SwitchInsn {
                        kind: table.kind,
                        reg,
                        base: i as u32,
                        payload_at,
                    }));
                    i += 3;
                    continue;
                }
                None => {
                    out.push(Insn::Raw(words[i..i + 3].to_vec()));
                    i += 3;
                    continue;
                }
            }
        }
        // The payload of a switch table that no switch names is still read,
        // so the case list survives in the text.
        if words[i] == 0x0100 || words[i] == 0x0200 {
            if let Some(table) = read_switch_table(words, i, i as u32) {
                out.push(Insn::SwitchData(table.clone()));
                i += switch_table_words(&table);
                continue;
            }
        }
        if let Some(len) = payload_len(words, i) {
            if i + len > words.len() {
                return Err("Instruction payload is truncated".into());
            }
            out.push(Insn::Raw(words[i..i + len].to_vec()));
            i += len;
            continue;
        }
        let width = opcode_width(op);
        if i + width > words.len() {
            return Err("Instruction is truncated".into());
        }
        let slice = &words[i..i + width];
        let at = i as u32;
        out.push(match name_of(op) {
            Some(name) => decode_at(name, slice, at).unwrap_or_else(|| Insn::Raw(slice.to_vec())),
            None => Insn::Raw(slice.to_vec()),
        });
        i += width;
    }
    Ok(out)
}

/// Reads the payload table a switch instruction dispatches through. The
/// table starts with a kind word. A packed table holds the first key, the
/// number of cases, and then one target per case. A sparse table holds the
/// number of cases, and then a key and a target for each case. Every target
/// is counted from the switch instruction that reads the table.
fn read_switch_table(words: &[u16], at: usize, origin: u32) -> Option<SwitchData> {
    let base = at as u32;
    match words.get(at).copied()? {
        0x0100 => {
            // A packed table holds the number of cases, the first key, and
            // then one target per case.
            let size = *words.get(at + 1)? as usize;
            let first = *words.get(at + 2)? as u16 as i32 | ((*words.get(at + 3)? as u16 as i32) << 16);
            let mut targets = Vec::with_capacity(size);
            for i in 0..size {
                let low = *words.get(at + 4 + i * 2)? as u32;
                let high = *words.get(at + 5 + i * 2)? as u32;
                let rel = (low | (high << 16)) as i32;
                targets.push(origin.wrapping_add(rel as u32));
            }
            Some(SwitchData {
                kind: SwitchKind::Packed,
                base,
                switch_at: origin,
                first_key: first,
                keys: Vec::new(),
                targets,
            })
        }
        0x0200 => {
            // A sparse table holds the number of cases, then one key per
            // case, and then one target per case. The keys and the targets
            // are two lists, not one list of pairs.
            let size = *words.get(at + 1)? as usize;
            let mut keys = Vec::with_capacity(size);
            for i in 0..size {
                let low = *words.get(at + 2 + i * 2)? as u32;
                let high = *words.get(at + 3 + i * 2)? as u32;
                keys.push((low | (high << 16)) as i32);
            }
            let mut targets = Vec::with_capacity(size);
            for i in 0..size {
                let low = *words.get(at + 2 + size * 2 + i * 2)? as u32;
                let high = *words.get(at + 3 + size * 2 + i * 2)? as u32;
                let rel = (low | (high << 16)) as i32;
                targets.push(origin.wrapping_add(rel as u32));
            }
            Some(SwitchData {
                kind: SwitchKind::Sparse,
                base,
                switch_at: origin,
                first_key: keys.first().copied().unwrap_or(0),
                keys,
                targets,
            })
        }
        _ => None,
    }
}

/// The number of words a payload table occupies in the instruction stream.
fn switch_table_words(table: &SwitchData) -> usize {
    match table.kind {
        SwitchKind::Packed => 4 + table.targets.len() * 2,
        SwitchKind::Sparse => 2 + table.targets.len() * 4,
    }
}

fn decode_at(name: &str, w: &[u16], at: u32) -> Option<Insn> {
    let a = ((w[0] >> 8) & 0xf) as u8;
    let b = (w[0] >> 12) as u8;
    let reg8 = (w[0] >> 8) as u8;
    let target_from = |off: i32| Some(at.wrapping_add(off as u32));
    let (regs, imm, target, index): (Vec<u8>, Option<i64>, Option<u32>, Option<(char, usize)>) = match name {
        "nop" | "return-void" => (vec![], None, None, None),
        "return" | "return-wide" | "return-object" | "throw" | "monitor-enter" | "monitor-exit"
        | "move-result" | "move-result-wide" | "move-result-object" | "move-exception" => (vec![reg8], None, None, None),
        "const/4" => (vec![a], Some(sign_nibble(b) as i64), None, None),
        "array-length" => (vec![a, b], None, None, None),
        "goto" => (vec![], None, target_from(sign_byte(reg8) as i32), None),
        "const/16" => (vec![reg8], Some(w[1] as i16 as i64), None, None),
        "add-int/lit8" => (vec![reg8, (w[1] & 0xff) as u8], Some((w[1] >> 8) as i8 as i64), None, None),
        "const-string" => (vec![reg8], None, None, Some(('s', w[1] as usize))),
        "const-class" | "check-cast" | "new-instance" => (vec![reg8], None, None, Some(('t', w[1] as usize))),
        "sget" | "sget-object" | "sput" | "sput-object" => (vec![reg8], None, None, Some(('f', w[1] as usize))),
        "iget" | "iget-object" | "iput" | "iput-object" => (vec![a, b], None, None, Some(('f', w[1] as usize))),
        "instance-of" | "new-array" => (vec![a, b], None, None, Some(('t', w[1] as usize))),
        "if-eqz" | "if-nez" | "if-ltz" | "if-gez" | "if-gtz" | "if-lez" => (vec![reg8], None, target_from(w[1] as i16 as i32), None),
        "if-eq" | "if-ne" | "if-lt" | "if-ge" | "if-gt" | "if-le" => (vec![a, b], None, target_from(w[1] as i16 as i32), None),
        "goto/16" => (vec![], None, target_from(w[1] as i16 as i32), None),
        "const" => {
            let imm = (w[1] as u32) | ((w[2] as u32) << 16);
            (vec![reg8], Some(imm as i32 as i64), None, None)
        }
        "invoke-virtual" | "invoke-super" | "invoke-direct" | "invoke-static" | "invoke-interface" => {
            let count = a as usize;
            let g = b;
            let c = (w[2] & 0xf) as u8;
            let d = ((w[2] >> 4) & 0xf) as u8;
            let e = ((w[2] >> 8) & 0xf) as u8;
            let f = (w[2] >> 12) as u8;
            let pool = [c, d, e, f, g];
            (pool[..count.min(5)].to_vec(), None, None, Some(('m', w[1] as usize)))
        }
        "const-wide" => {
            let imm = (w[1] as u64) | ((w[2] as u64) << 16) | ((w[3] as u64) << 32) | ((w[4] as u64) << 48);
            (vec![reg8], Some(imm as i64), None, None)
        }
        _ => return None,
    };
    Some(Insn::Op { name: name.into(), regs, imm, target, index })
}

fn name_of(op: u8) -> Option<&'static str> {
    Some(match op {
        0x00 => "nop",
        0x0a => "move-result",
        0x0b => "move-result-wide",
        0x0c => "move-result-object",
        0x0d => "move-exception",
        0x0e => "return-void",
        0x0f => "return",
        0x10 => "return-wide",
        0x11 => "return-object",
        0x12 => "const/4",
        0x13 => "const/16",
        0x14 => "const",
        0x18 => "const-wide",
        0x1a => "const-string",
        0x1c => "const-class",
        0x1d => "monitor-enter",
        0x1e => "monitor-exit",
        0x1f => "check-cast",
        0x20 => "instance-of",
        0x21 => "array-length",
        0x22 => "new-instance",
        0x23 => "new-array",
        0x27 => "throw",
        0x28 => "goto",
        0x29 => "goto/16",
        0x32 => "if-eq",
        0x33 => "if-ne",
        0x34 => "if-lt",
        0x35 => "if-ge",
        0x36 => "if-gt",
        0x37 => "if-le",
        0x38 => "if-eqz",
        0x39 => "if-nez",
        0x3a => "if-ltz",
        0x3b => "if-gez",
        0x3c => "if-gtz",
        0x3d => "if-lez",
        0x52 => "iget",
        0x54 => "iget-object",
        0x59 => "iput",
        0x5b => "iput-object",
        0x60 => "sget",
        0x62 => "sget-object",
        0x67 => "sput",
        0x69 => "sput-object",
        0x6e => "invoke-virtual",
        0x6f => "invoke-super",
        0x70 => "invoke-direct",
        0x71 => "invoke-static",
        0x72 => "invoke-interface",
        0xd8 => "add-int/lit8",
        _ => return None,
    })
}

fn opcode_width(op: u8) -> usize {
    match op {
        0x00 | 0x01 | 0x04 | 0x07 | 0x0a..=0x12 | 0x1d | 0x1e | 0x21 | 0x27 | 0x28 | 0x7b..=0x8f | 0xb0..=0xcf => 1,
        0x03 | 0x06 | 0x09 | 0x14 | 0x17 | 0x1b | 0x24..=0x26 | 0x2a..=0x2c | 0x6e..=0x72 | 0x74..=0x78 | 0xfc | 0xfd => 3,
        0x18 => 5,
        0xfa | 0xfb => 4,
        _ => 2,
    }
}

fn payload_len(words: &[u16], i: usize) -> Option<usize> {
    if i >= words.len() {
        return None;
    }
    match words[i] {
        0x0100 => {
            let size = *words.get(i + 1)? as usize;
            Some(4 + size * 2)
        }
        0x0200 => {
            let size = *words.get(i + 1)? as usize;
            Some(2 + size * 4)
        }
        0x0300 => {
            let width = *words.get(i + 1)? as usize;
            let size = (*words.get(i + 2)? as usize) | ((*words.get(i + 3)? as usize) << 16);
            let bytes = width.checked_mul(size)?;
            Some(4 + (bytes + 1) / 2)
        }
        _ => None,
    }
}

struct TextDex {
    classes: Vec<TextClass>,
}

struct TextClass {
    name: String,
    access: u32,
    super_name: String,
    source: Option<String>,
    interfaces: Vec<String>,
    fields: Vec<TextField>,
    methods: Vec<TextMethod>,
}

struct TextField {
    access: u32,
    name: String,
    ty: String,
    /// The initial value of a static field, written the way the reference
    /// writes it.
    value: Option<String>,
}

struct TextMethod {
    access: u32,
    name: String,
    descriptor: String,
    registers: u16,
    lines: Vec<TextLine>,
    catches: Vec<TextCatch>,
}

enum TextLine {
    Label(String),
    Unit(u16),
    Insn(String),
    /// A packed payload table. It holds the first key and the place each
    /// case jumps to.
    PackedSwitch { first_key: i32, targets: Vec<String> },
    /// A sparse payload table. Each case names its key and the place it
    /// jumps to.
    SparseSwitch { cases: Vec<(i32, String)> },
}

struct TextCatch {
    ty: Option<String>,
    start: String,
    end: String,
    handler: String,
}

fn parse_text(text: &str) -> Result<TextDex> {
    let mut classes = Vec::new();
    let mut current: Option<TextClass> = None;
    let mut method: Option<TextMethod> = None;
    let rows: Vec<&str> = text.lines().collect();
    let mut cursor = 0usize;
    while cursor < rows.len() {
        let number = cursor;
        cursor += 1;
        let line_no = number + 1;
        let line = strip_comment(rows[number]).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(".class ") {
            if method.is_some() {
                return Err(format!("Class starts inside a method at line {line_no}"));
            }
            if let Some(class) = current.take() {
                classes.push(class);
            }
            let (access, name) = split_access(rest, AccessKind::Class)?;
            current = Some(TextClass { name, access, super_name: "Ljava/lang/Object;".into(), source: None, interfaces: Vec::new(), fields: Vec::new(), methods: Vec::new() });
        } else if let Some(rest) = line.strip_prefix(".super ") {
            current.as_mut().ok_or("Superclass is outside a class")?.super_name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix(".source ") {
            let name = unquote(rest.trim())?;
            current.as_mut().ok_or("Source is outside a class")?.source = Some(name);
        } else if let Some(rest) = line.strip_prefix(".implements ") {
            if method.is_some() {
                return Err(format!("Interface is inside a method at line {line_no}"));
            }
            let interface = rest.trim();
            if interface.is_empty() {
                return Err(format!("Interface is missing a type at line {line_no}"));
            }
            current.as_mut().ok_or("Interface is outside a class")?.interfaces.push(interface.to_string());
        } else if let Some(rest) = line.strip_prefix(".field ") {
            let (access, body) = split_access(rest, AccessKind::Field)?;
            // A static field carries its initial value after the type, in
            // the form the reference writes it.
            let (body, value) = match body.split_once(" = ") {
                Some((body, value)) => (body, Some(value.trim().to_string())),
                None => (body.as_str(), None),
            };
            let (name, ty) = body.split_once(':').ok_or("Field is missing a type")?;
            current.as_mut().ok_or("Field is outside a class")?.fields.push(TextField { access, name: name.trim().to_string(), ty: ty.trim().to_string(), value });
        } else if let Some(rest) = line.strip_prefix(".method ") {
            if method.is_some() {
                return Err(format!("Nested method at line {line_no}"));
            }
            let (access, body) = split_access(rest, AccessKind::Method)?;
            let split = body.rfind('(').ok_or("Method is missing a descriptor")?;
            method = Some(TextMethod {
                access,
                name: body[..split].trim().to_string(),
                descriptor: body[split..].trim().to_string(),
                registers: 0,
                lines: Vec::new(),
                catches: Vec::new(),
            });
        } else if line == ".end method" {
            let finished = method.take().ok_or("End of method without a method")?;
            current.as_mut().ok_or("Method is outside a class")?.methods.push(finished);
        } else if let Some(rest) = line.strip_prefix(".registers ") {
            let n = parse_u32(rest.trim())? as u16;
            method.as_mut().ok_or("Registers are outside a method")?.registers = n;
        } else if let Some(rest) = line.strip_prefix(".catch-all ") {
            let (start, end, handler) = parse_catch_range(rest)?;
            method.as_mut().ok_or("Catch is outside a method")?.catches.push(TextCatch { ty: None, start, end, handler });
        } else if let Some(rest) = line.strip_prefix(".catch ") {
            let (ty, range) = rest.split_once(' ').ok_or("Catch is missing a type")?;
            let (start, end, handler) = parse_catch_range(range)?;
            method.as_mut().ok_or("Catch is outside a method")?.catches.push(TextCatch { ty: Some(ty.to_string()), start, end, handler });
        } else if let Some(rest) = line.strip_prefix(':') {
            method.as_mut().ok_or("Label is outside a method")?.lines.push(TextLine::Label(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix(".unit ") {
            let word = parse_u32(rest.trim())? as u16;
            method.as_mut().ok_or("Unit is outside a method")?.lines.push(TextLine::Unit(word));
        } else if line == ".packed-switch" || line.starts_with(".packed-switch ") {
            // A packed payload table names the first key and then one place
            // per case. The block runs until the end of the table.
            let first_key = match line.strip_prefix(".packed-switch") {
                Some(rest) if !rest.trim().is_empty() => parse_i64(rest.trim())? as i32,
                _ => 0,
            };
            let mut targets = Vec::new();
            let mut closed = false;
            while cursor < rows.len() {
                let body = strip_comment(rows[cursor]).trim();
                cursor += 1;
                if body == ".end packed-switch" {
                    closed = true;
                    break;
                }
                if body.is_empty() {
                    continue;
                }
                let Some(label) = body.strip_prefix(':') else {
                    return Err(format!("Packed switch case is missing its place at line {cursor}"));
                };
                targets.push(label.trim().to_string());
            }
            if !closed {
                return Err("Packed switch table is not closed".into());
            }
            method.as_mut().ok_or("Switch table is outside a method")?.lines.push(TextLine::PackedSwitch { first_key, targets });
        } else if line == ".sparse-switch" {
            // A sparse payload table names a key and the place it jumps to
            // for each case. The block runs until the end of the table.
            let mut cases = Vec::new();
            let mut closed = false;
            while cursor < rows.len() {
                let body = strip_comment(rows[cursor]).trim();
                cursor += 1;
                if body == ".end sparse-switch" {
                    closed = true;
                    break;
                }
                if body.is_empty() {
                    continue;
                }
                let Some((key, place)) = body.split_once("->") else {
                    return Err(format!("Sparse switch case is missing its place at line {cursor}"));
                };
                let key = parse_i64(key.trim())? as i32;
                let Some(label) = place.trim().strip_prefix(':') else {
                    return Err(format!("Sparse switch case is missing its place at line {cursor}"));
                };
                cases.push((key, label.trim().to_string()));
            }
            if !closed {
                return Err("Sparse switch table is not closed".into());
            }
            method.as_mut().ok_or("Switch table is outside a method")?.lines.push(TextLine::SparseSwitch { cases });
        } else if line.starts_with('.') {
            return Err(format!("Unknown directive at line {line_no}: {line}"));
        } else {
            method.as_mut().ok_or(format!("Instruction is outside a method at line {line_no}"))?.lines.push(TextLine::Insn(line.to_string()));
        }
    }
    if method.is_some() {
        return Err("Method is missing .end method".into());
    }
    if let Some(class) = current {
        classes.push(class);
    }
    if classes.is_empty() {
        return Err("No class in the method text".into());
    }
    Ok(TextDex { classes })
}

fn strip_comment(line: &str) -> &str {
    let mut quote = false;
    let mut escape = false;
    for (i, c) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && quote {
            escape = true;
            continue;
        }
        if c == '"' {
            quote = !quote;
        } else if c == '#' && !quote {
            return &line[..i];
        }
    }
    line
}

fn split_access(text: &str, kind: AccessKind) -> Result<(u32, String)> {
    let mut access = 0u32;
    let mut rest = text.trim();
    loop {
        let Some((word, tail)) = rest.split_once(' ') else { break };
        if !is_access_word(word) {
            break;
        }
        access |= access_bit(word, kind);
        rest = tail.trim();
    }
    if rest.is_empty() {
        return Err("Declaration is missing a name".into());
    }
    Ok((access, rest.to_string()))
}

/// Which declaration the access flags belong to. The same bit means a
/// different name on a field than on a method, so the name is chosen by
/// the kind of the declaration.
#[derive(Clone, Copy, PartialEq)]
enum AccessKind {
    Class,
    Field,
    Method,
}

fn is_access_word(word: &str) -> bool {
    matches!(word, "public" | "private" | "protected" | "static" | "final" | "synchronized" | "bridge" | "varargs" | "native" | "interface" | "abstract" | "strict" | "synthetic" | "annotation" | "enum" | "constructor" | "declared-synchronized" | "volatile" | "transient" | "super")
}

fn access_bit(word: &str, kind: AccessKind) -> u32 {
    match word {
        "public" => 0x1,
        "private" => 0x2,
        "protected" => 0x4,
        "static" => 0x8,
        "final" => 0x10,
        // The same bit names a class differently than it names a method.
        "synchronized" => match kind {
            AccessKind::Method => 0x20,
            _ => 0,
        },
        "super" => match kind {
            AccessKind::Class => 0x20,
            _ => 0,
        },
        // The same bit names a field differently than it names a method.
        "volatile" => match kind {
            AccessKind::Field => 0x40,
            _ => 0,
        },
        "bridge" => match kind {
            AccessKind::Method => 0x40,
            _ => 0,
        },
        "transient" | "varargs" => match kind {
            AccessKind::Class => 0,
            _ => 0x80,
        },
        "native" => match kind {
            AccessKind::Method => 0x100,
            _ => 0,
        },
        "interface" => match kind {
            AccessKind::Class => 0x200,
            _ => 0,
        },
        "abstract" => match kind {
            AccessKind::Class | AccessKind::Method => 0x400,
            AccessKind::Field => 0,
        },
        "strict" => match kind {
            AccessKind::Method => 0x800,
            _ => 0,
        },
        "synthetic" => 0x1000,
        "annotation" => match kind {
            AccessKind::Class => 0x2000,
            _ => 0,
        },
        "enum" => match kind {
            AccessKind::Class | AccessKind::Field => 0x4000,
            AccessKind::Method => 0,
        },
        "constructor" => match kind {
            AccessKind::Method => 0x10000,
            _ => 0,
        },
        "declared-synchronized" => match kind {
            AccessKind::Method => 0x20000,
            _ => 0,
        },
        _ => 0,
    }
}

fn access_text(flags: u32, kind: AccessKind) -> String {
    // Each declaration kind names the same bit in its own way, so the table
    // carries the name each kind uses and an empty name means that kind does
    // not carry the bit at all.
    const NAMES: [(u32, &str, &str, &str); 17] = [
        (0x1, "public", "public", "public"),
        (0x2, "private", "private", "private"),
        (0x4, "protected", "protected", "protected"),
        (0x8, "static", "static", "static"),
        (0x10, "final", "final", "final"),
        (0x20, "super", "", "synchronized"),
        (0x40, "", "volatile", "bridge"),
        (0x80, "", "transient", "varargs"),
        (0x100, "", "", "native"),
        (0x200, "interface", "", ""),
        (0x400, "abstract", "", "abstract"),
        (0x800, "", "", "strict"),
        (0x1000, "synthetic", "synthetic", "synthetic"),
        (0x2000, "annotation", "", ""),
        (0x4000, "enum", "enum", ""),
        (0x10000, "", "", "constructor"),
        (0x20000, "", "", "declared-synchronized"),
    ];
    let mut parts = Vec::new();
    for (bit, class_name, field_name, method_name) in NAMES {
        if flags & bit == 0 {
            continue;
        }
        let name = match kind {
            AccessKind::Class => class_name,
            AccessKind::Field => field_name,
            AccessKind::Method => method_name,
        };
        if !name.is_empty() {
            parts.push(name);
        }
    }
    if parts.is_empty() { String::new() } else { format!("{} ", parts.join(" ")) }
}

fn parse_catch_range(text: &str) -> Result<(String, String, String)> {
    let text = text.trim();
    let open = text.find('{').ok_or("Catch range is missing")?;
    let close = text.find('}').ok_or("Catch range is missing an end")?;
    let body = &text[open + 1..close];
    let (start, end) = body.split_once("..").ok_or("Catch range needs a start and an end")?;
    let handler = text[close + 1..].trim().trim_start_matches(':');
    let start = start.trim().trim_start_matches(':').to_string();
    let end = end.trim().trim_start_matches(':').to_string();
    if handler.is_empty() {
        return Err("Catch is missing a handler".into());
    }
    Ok((start, end, handler.to_string()))
}

fn unquote(text: &str) -> Result<String> {
    let text = text.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        unescape(&text[1..text.len() - 1])
    } else {
        Err("Expected quoted text".into())
    }
}

fn parse_u32(text: &str) -> Result<u32> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).map_err(|_| format!("Bad number {text}"))
    } else if text.starts_with('-') {
        text.parse::<i32>().map(|n| n as u32).map_err(|_| format!("Bad number {text}"))
    } else {
        text.parse::<u32>().map_err(|_| format!("Bad number {text}"))
    }
}

fn parse_i64(text: &str) -> Result<i64> {
    let text = text.trim();
    let (negative, body) = match text.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, text),
    };
    let value = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|_| format!("Bad number {text}"))?
    } else {
        body.parse::<u64>().map_err(|_| format!("Bad number {text}"))?
    };
    Ok(if negative { (value as i64).wrapping_neg() } else { value as i64 })
}

struct Pool {
    strings: Vec<String>,
}

impl Pool {
    fn new() -> Self { Self { strings: Vec::new() } }
    fn intern(&mut self, text: &str) -> usize {
        if let Some(at) = self.strings.iter().position(|s| s == text) {
            at
        } else {
            self.strings.push(text.to_string());
            self.strings.len() - 1
        }
    }
}

fn emit(text: &TextDex) -> Result<Vec<u8>> {
    let mut pool = Pool::new();
    let mut types = Vec::new();
    let mut fields = Vec::new();
    let mut methods = Vec::new();
    let mut protos = Vec::new();
    let mut classes = Vec::new();
    for class in &text.classes {
        let type_idx = intern_type(&mut pool, &mut types, &class.name);
        let super_idx = intern_type(&mut pool, &mut types, &class.super_name);
        let source = class.source.as_ref().map(|s| pool.intern(s));
        let mut interfaces = Vec::new();
        for interface in &class.interfaces {
            interfaces.push(intern_type(&mut pool, &mut types, interface));
        }
        let mut out_fields = Vec::new();
        for field in &class.fields {
            let owner = type_idx;
            let name = pool.intern(&field.name);
            let ty = intern_type(&mut pool, &mut types, &field.ty);
            let idx = push_unique(&mut fields, (owner, name, ty));
            let value = match field.value.as_deref() {
                Some(text) => Some(parse_value_text(&mut pool, &mut types, &mut fields, &mut methods, &mut protos, &field.ty, text)?),
                None => None,
            };
            out_fields.push(Member { idx, access: field.access, value });
        }
        let mut out_methods = Vec::new();
        for method in &class.methods {
            let (ret, args) = split_descriptor(&method.descriptor)?;
            let ret_idx = intern_type(&mut pool, &mut types, &ret);
            let mut arg_idx = Vec::new();
            for arg in &args {
                arg_idx.push(intern_type(&mut pool, &mut types, arg));
            }
            let proto = push_unique(&mut protos, (ret_idx, arg_idx));
            let name = pool.intern(&method.name);
            let idx = push_unique(&mut methods, (type_idx, proto, name));
            let mut access = method.access;
            if method.name == "<init>" || method.name == "<clinit>" {
                access |= 0x10000;
            }
            let code = if access & (0x400 | 0x100) != 0 {
                None
            } else {
                Some(encode_method(&mut pool, &mut types, &mut fields, &mut methods, &mut protos, method)?)
            };
            out_methods.push(Method { idx, access, code });
        }
        classes.push(Class { type_idx, access: class.access, super_idx, source, interfaces, fields: out_fields, methods: out_methods });
    }
    let mut shorties = Vec::new();
    for (ret, args) in &protos {
        let mut text = String::new();
        text.push(short_char(&pool.strings[types[*ret]]));
        for arg in args {
            text.push(short_char(&pool.strings[types[*arg]]));
        }
        shorties.push(text);
    }
    for text in shorties {
        pool.intern(&text);
    }
    // Sort identifier tables and remap.
    let mut string_order: Vec<usize> = (0..pool.strings.len()).collect();
    string_order.sort_by(|&a, &b| utf16(&pool.strings[a]).cmp(&utf16(&pool.strings[b])));
    let mut string_map = vec![0usize; pool.strings.len()];
    let mut strings = Vec::with_capacity(pool.strings.len());
    for (new, old) in string_order.iter().enumerate() {
        string_map[*old] = new;
        strings.push(pool.strings[*old].clone());
    }
    let remap = |n: usize| string_map[n];
    for ty in &mut types {
        *ty = remap(*ty);
    }
    let mut type_order: Vec<usize> = (0..types.len()).collect();
    type_order.sort_by(|&a, &b| types[a].cmp(&types[b]));
    let mut type_map = vec![0usize; types.len()];
    let mut type_names = Vec::with_capacity(types.len());
    for (new, old) in type_order.iter().enumerate() {
        type_map[*old] = new;
        type_names.push(strings[types[*old]].clone());
    }
    let tr = |n: usize| type_map[n];
    for (ret, args) in &mut protos {
        *ret = tr(*ret);
        for arg in args.iter_mut() {
            *arg = tr(*arg);
        }
    }
    let mut proto_order: Vec<usize> = (0..protos.len()).collect();
    proto_order.sort_by(|&a, &b| protos[a].cmp(&protos[b]));
    let mut proto_map = vec![0usize; protos.len()];
    let mut proto_sorted = Vec::with_capacity(protos.len());
    for (new, old) in proto_order.iter().enumerate() {
        proto_map[*old] = new;
        proto_sorted.push(protos[*old].clone());
    }
    for (owner, name, ty) in &mut fields {
        *owner = tr(*owner);
        *name = remap(*name);
        *ty = tr(*ty);
    }
    let mut field_order: Vec<usize> = (0..fields.len()).collect();
    field_order.sort_by(|&a, &b| fields[a].cmp(&fields[b]));
    let mut field_map = vec![0usize; fields.len()];
    let mut field_sorted = Vec::with_capacity(fields.len());
    for (new, old) in field_order.iter().enumerate() {
        field_map[*old] = new;
        field_sorted.push(fields[*old]);
    }
    for (owner, proto, name) in &mut methods {
        *owner = tr(*owner);
        *proto = proto_map[*proto];
        *name = remap(*name);
    }
    let mut method_order: Vec<usize> = (0..methods.len()).collect();
    method_order.sort_by(|&a, &b| {
        let (owner_a, proto_a, name_a) = methods[a];
        let (owner_b, proto_b, name_b) = methods[b];
        (owner_a, name_a, proto_a).cmp(&(owner_b, name_b, proto_b))
    });
    let mut method_map = vec![0usize; methods.len()];
    let mut method_sorted = Vec::with_capacity(methods.len());
    for (new, old) in method_order.iter().enumerate() {
        method_map[*old] = new;
        method_sorted.push(methods[*old]);
    }
    for class in &mut classes {
        class.type_idx = tr(class.type_idx);
        class.super_idx = tr(class.super_idx);
        for interface in &mut class.interfaces {
            *interface = tr(*interface);
        }
        if let Some(source) = class.source.as_mut() {
            *source = remap(*source);
        }
        for field in &mut class.fields {
            field.idx = field_map[field.idx];
            if let Some(value) = field.value.as_mut() {
                remap_field_value(value, &string_map, &type_map, &field_map, &method_map, &proto_map);
            }
        }
        class.fields.sort_by_key(|f| f.idx);
        for method in &mut class.methods {
            method.idx = method_map[method.idx];
            if let Some(code) = method.code.as_mut() {
                for insn in &mut code.insns {
                    if let Insn::Op { index: Some((kind, idx)), .. } = insn {
                        *idx = match kind {
                            's' => remap(*idx),
                            't' => tr(*idx),
                            'f' => field_map[*idx],
                            'm' => method_map[*idx],
                            _ => *idx,
                        };
                    }
                }
                for catch in &mut code.catches {
                    if let Some(ty) = catch.ty.as_mut() {
                        *ty = tr(*ty);
                    }
                }
            }
        }
        class.methods.sort_by_key(|m| m.idx);
    }
    classes.sort_by_key(|c| c.type_idx);
    let file = Dex {
        strings,
        types: type_names,
        protos: proto_sorted,
        fields: field_sorted,
        methods: method_sorted,
        classes,
    };
    write_dex(&file)
}

fn intern_type(pool: &mut Pool, types: &mut Vec<usize>, name: &str) -> usize {
    let sid = pool.intern(name);
    if let Some(at) = types.iter().position(|t| *t == sid) {
        at
    } else {
        types.push(sid);
        types.len() - 1
    }
}

fn push_unique<T: PartialEq + Clone>(list: &mut Vec<T>, item: T) -> usize {
    if let Some(at) = list.iter().position(|v| v == &item) {
        at
    } else {
        list.push(item);
        list.len() - 1
    }
}

fn split_descriptor(desc: &str) -> Result<(String, Vec<String>)> {
    if !desc.starts_with('(') {
        return Err(format!("Bad descriptor {desc}"));
    }
    let close = desc.find(')').ok_or("Descriptor is missing ')'")?;
    let ret = desc[close + 1..].to_string();
    if ret.is_empty() {
        return Err("Descriptor is missing a return type".into());
    }
    let mut args = Vec::new();
    let mut rest = &desc[1..close];
    while !rest.is_empty() {
        let (ty, next) = take_type(rest)?;
        args.push(ty);
        rest = next;
    }
    Ok((ret, args))
}

fn take_type(text: &str) -> Result<(String, &str)> {
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() && bytes[i] == b'[' {
        i += 1;
    }
    if i >= bytes.len() {
        return Err("Descriptor type is truncated".into());
    }
    if bytes[i] == b'L' {
        let end = text[i..].find(';').ok_or("Class descriptor is missing ';'")? + i + 1;
        Ok((text[..end].to_string(), &text[end..]))
    } else {
        Ok((text[..=i].to_string(), &text[i + 1..]))
    }
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn encode_method(
    pool: &mut Pool,
    types: &mut Vec<usize>,
    fields: &mut Vec<(usize, usize, usize)>,
    methods: &mut Vec<(usize, usize, usize)>,
    protos: &mut Vec<(usize, Vec<usize>)>,
    method: &TextMethod,
) -> Result<Code> {
    let mut labels = std::collections::BTreeMap::<String, u32>::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut at = 0u32;
    for line in &method.lines {
        match line {
            TextLine::Label(name) => {
                labels.insert(name.clone(), at);
            }
            TextLine::Unit(word) => {
                pieces.push(Piece::Words(vec![*word]));
                at += 1;
            }
            TextLine::Insn(text) => {
                let width = insn_width_text(text)?;
                pieces.push(Piece::Text(text.clone()));
                at += width;
            }
            TextLine::PackedSwitch { first_key, targets } => {
                // A payload table sits at a place of its own and carries its
                // own width.
                pieces.push(Piece::PackedSwitch { first_key: *first_key, targets: targets.clone() });
                at += 4 + targets.len() as u32 * 2;
            }
            TextLine::SparseSwitch { cases } => {
                pieces.push(Piece::SparseSwitch { cases: cases.clone() });
                at += 2 + cases.len() as u32 * 4;
            }
        }
    }
    let mut insns = Vec::new();
    let mut cursor = 0u32;
    // A payload table counts its targets from the switch instruction that
    // reads it, so the place of that switch is found before the table is
    // written.
    let mut readers: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut scan = 0u32;
    for piece in &pieces {
        match piece {
            Piece::Words(words) => scan += words.len() as u32,
            Piece::Text(text) => {
                if text.starts_with("packed-switch") || text.starts_with("sparse-switch") {
                    if let Some(label) = switch_table_label(text) {
                        if let Some(at) = labels.get(&label) {
                            readers.insert(*at, scan);
                        }
                    }
                }
                scan += insn_width_text(text)?;
            }
            Piece::PackedSwitch { targets, .. } => scan += 4 + targets.len() as u32 * 2,
            Piece::SparseSwitch { cases } => scan += 2 + cases.len() as u32 * 4,
        }
    }
    for piece in &pieces {
        match piece {
            Piece::Words(words) => {
                insns.push(Insn::Raw(words.clone()));
                cursor += words.len() as u32;
            }
            Piece::Text(text) => {
                insns.push(encode_insn(pool, types, fields, methods, protos, text, cursor, &labels)?);
                cursor += insn_width_text(text)?;
            }
            Piece::PackedSwitch { first_key, targets } => {
                // Every case names the place it jumps to.
                let mut places = Vec::with_capacity(targets.len());
                for target in targets {
                    places.push(*labels.get(target).ok_or(format!("Missing label {target}"))?);
                }
                insns.push(Insn::SwitchData(SwitchData {
                    kind: SwitchKind::Packed,
                    base: cursor,
                    switch_at: readers.get(&cursor).copied().unwrap_or(0),
                    first_key: *first_key,
                    keys: Vec::new(),
                    targets: places,
                }));
                cursor += 4 + targets.len() as u32 * 2;
            }
            Piece::SparseSwitch { cases } => {
                let mut keys = Vec::with_capacity(cases.len());
                let mut places = Vec::with_capacity(cases.len());
                for (key, target) in cases {
                    keys.push(*key);
                    places.push(*labels.get(target).ok_or(format!("Missing label {target}"))?);
                }
                insns.push(Insn::SwitchData(SwitchData {
                    kind: SwitchKind::Sparse,
                    base: cursor,
                    switch_at: readers.get(&cursor).copied().unwrap_or(0),
                    first_key: keys.first().copied().unwrap_or(0),
                    keys,
                    targets: places,
                }));
                cursor += 2 + cases.len() as u32 * 4;
            }
        }
    }
    let mut catches = Vec::new();
    for catch in &method.catches {
        let start = *labels.get(&catch.start).ok_or("Catch start label is missing")?;
        let end = *labels.get(&catch.end).ok_or("Catch end label is missing")?;
        let handler = *labels.get(&catch.handler).ok_or("Catch handler label is missing")?;
        let ty = match &catch.ty {
            Some(name) => Some(intern_type(pool, types, name)),
            None => None,
        };
        catches.push(Catch { start, end, handler, ty });
    }
    let ins = incoming_words(method.access, &method.descriptor);
    let registers = method.registers.max(ins);
    Ok(Code { registers, ins, insns, catches })
}

fn incoming_words(access: u32, descriptor: &str) -> u16 {
    let args = split_descriptor(descriptor).map(|(_, args)| args).unwrap_or_default();
    let mut words = if access & 0x8 == 0 { 1u16 } else { 0 };
    for arg in args {
        words = words.saturating_add(if arg.starts_with('[') { 1 } else if arg == "J" || arg == "D" { 2 } else { 1 });
    }
    words
}

enum Piece {
    Words(Vec<u16>),
    Text(String),
    /// A packed payload table and the place each case jumps to.
    PackedSwitch { first_key: i32, targets: Vec<String> },
    /// A sparse payload table. Each case names its key and the place it
    /// jumps to.
    SparseSwitch { cases: Vec<(i32, String)> },
}

fn insn_width_text(text: &str) -> Result<u32> {
    let name = text.split_whitespace().next().unwrap_or("");
    let width = named_width(name);
    if width == 1 && name_code(name).is_none() && !matches!(name, "nop" | "return-void" | "goto" | "array-length" | "return" | "return-wide" | "return-object" | "const/4" | "move-result" | "move-result-wide" | "move-result-object" | "move-exception" | "throw" | "monitor-enter" | "monitor-exit") {
        if name_code(name).is_none() {
            return Err(format!("Unknown instruction {name}"));
        }
    }
    if name_code(name).is_none() {
        return Err(format!("Unknown instruction {name}"));
    }
    Ok(width as u32)
}

fn name_code(name: &str) -> Option<u8> {
    Some(match name {
        "nop" => 0x00,
        "move-result" => 0x0a,
        "move-result-wide" => 0x0b,
        "move-result-object" => 0x0c,
        "move-exception" => 0x0d,
        "return-void" => 0x0e,
        "return" => 0x0f,
        "return-wide" => 0x10,
        "return-object" => 0x11,
        "const/4" => 0x12,
        "const/16" => 0x13,
        "const" => 0x14,
        "const-wide" => 0x18,
        "const-string" => 0x1a,
        "const-class" => 0x1c,
        "monitor-enter" => 0x1d,
        "monitor-exit" => 0x1e,
        "check-cast" => 0x1f,
        "instance-of" => 0x20,
        "array-length" => 0x21,
        "new-instance" => 0x22,
        "new-array" => 0x23,
        "throw" => 0x27,
        "goto" => 0x28,
        "goto/16" => 0x29,
        "packed-switch" => 0x2b,
        "sparse-switch" => 0x2c,
        "if-eq" => 0x32,
        "if-ne" => 0x33,
        "if-lt" => 0x34,
        "if-ge" => 0x35,
        "if-gt" => 0x36,
        "if-le" => 0x37,
        "if-eqz" => 0x38,
        "if-nez" => 0x39,
        "if-ltz" => 0x3a,
        "if-gez" => 0x3b,
        "if-gtz" => 0x3c,
        "if-lez" => 0x3d,
        "iget" => 0x52,
        "iget-object" => 0x54,
        "iput" => 0x59,
        "iput-object" => 0x5b,
        "sget" => 0x60,
        "sget-object" => 0x62,
        "sput" => 0x67,
        "sput-object" => 0x69,
        "invoke-virtual" => 0x6e,
        "invoke-super" => 0x6f,
        "invoke-direct" => 0x70,
        "invoke-static" => 0x71,
        "invoke-interface" => 0x72,
        "add-int/lit8" => 0xd8,
        _ => return None,
    })
}

fn encode_insn(
    pool: &mut Pool,
    types: &mut Vec<usize>,
    fields: &mut Vec<(usize, usize, usize)>,
    methods: &mut Vec<(usize, usize, usize)>,
    protos: &mut Vec<(usize, Vec<usize>)>,
    text: &str,
    at: u32,
    labels: &std::collections::BTreeMap<String, u32>,
) -> Result<Insn> {
    let name = text.split_whitespace().next().ok_or("Empty instruction")?.to_string();
    if name_code(&name).is_none() {
        return Err(format!("Unknown instruction {name}"));
    }
    let rest = text[name.len()..].trim();
    if name.starts_with("invoke-") {
        let (regs, tail) = parse_reg_list(rest)?;
        let index = intern_method(pool, types, methods, protos, tail.trim().trim_start_matches(',').trim())?;
        return Ok(Insn::Op { name, regs, imm: None, target: None, index: Some(('m', index)) });
    }
    if name == "packed-switch" || name == "sparse-switch" {
        // A switch names the register it reads its key from and the label
        // of the payload table it dispatches through.
        let mut reg = 0u8;
        let mut table_label = String::new();
        for arg in split_args(rest) {
            if let Some(number) = arg.strip_prefix('v').or_else(|| arg.strip_prefix('p')) {
                if number.chars().all(|c| c.is_ascii_digit()) {
                    reg = number.parse::<u8>().map_err(|_| format!("Bad register {arg}"))?;
                    continue;
                }
            }
            if let Some(label) = arg.strip_prefix(':') {
                table_label = label.to_string();
            }
        }
        if table_label.is_empty() {
            return Err("Switch is missing its payload table".into());
        }
        let payload_at = *labels.get(&table_label).ok_or(format!("Missing label {table_label}"))?;
        let kind = if name == "packed-switch" { SwitchKind::Packed } else { SwitchKind::Sparse };
        return Ok(Insn::Switch(SwitchInsn {
            kind,
            reg,
            base: at,
            payload_at,
        }));
    }
    let args = split_args(rest);
    let mut regs = Vec::new();
    let mut imm = None;
    let mut target = None;
    let mut index = None;
    for arg in args {
        if let Some(reg) = arg.strip_prefix('v').or_else(|| arg.strip_prefix('p')) {
            if reg.chars().all(|c| c.is_ascii_digit()) {
                regs.push(reg.parse::<u8>().map_err(|_| format!("Bad register {arg}"))?);
                continue;
            }
        }
        if let Some(label) = arg.strip_prefix(':') {
            let dest = *labels.get(label).ok_or(format!("Missing label {label}"))?;
            target = Some(dest);
            continue;
        }
        if arg.starts_with('"') {
            let value = unquote(arg)?;
            index = Some(('s', pool.intern(&value)));
            continue;
        }
        if arg.starts_with('L') || arg.starts_with('[') || matches!(arg, "V" | "Z" | "B" | "S" | "C" | "I" | "J" | "F" | "D") || arg.contains("->") {
            if arg.contains("->") && arg.contains(':') {
                index = Some(('f', intern_field(pool, types, fields, arg)?));
            } else if arg.contains("->") {
                index = Some(('m', intern_method(pool, types, methods, protos, arg)?));
            } else {
                index = Some(('t', intern_type(pool, types, arg)));
            }
            continue;
        }
        imm = Some(parse_i64(arg)?);
    }
    let _ = at;
    Ok(Insn::Op { name, regs, imm, target, index })
}

/// The name of the payload table a switch instruction reads. The switch
/// names the register it reads its key from and the label of the table it
/// dispatches through.
fn switch_table_label(text: &str) -> Option<String> {
    let name = text.split_whitespace().next()?;
    if name != "packed-switch" && name != "sparse-switch" {
        return None;
    }
    let rest = text[name.len()..].trim();
    for arg in split_args(rest) {
        if let Some(label) = arg.strip_prefix(':') {
            return Some(label.to_string());
        }
    }
    None
}

fn parse_reg_list(text: &str) -> Result<(Vec<u8>, &str)> {
    let open = text.find('{').ok_or("Invoke is missing registers")?;
    let close = text.find('}').ok_or("Invoke registers are not closed")?;
    let mut regs = Vec::new();
    for part in text[open + 1..close].split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let num = part.trim_start_matches('v').trim_start_matches('p');
        regs.push(num.parse::<u8>().map_err(|_| format!("Bad register {part}"))?);
    }
    Ok((regs, &text[close + 1..]))
}

fn split_args(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quote = false;
    let mut escape = false;
    for (i, c) in text.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && quote {
            escape = true;
            continue;
        }
        if c == '"' {
            quote = !quote;
        } else if c == ',' && !quote {
            let part = text[start..i].trim();
            if !part.is_empty() {
                out.push(part);
            }
            start = i + 1;
        }
    }
    let part = text[start..].trim();
    if !part.is_empty() {
        out.push(part);
    }
    out
}

fn intern_field(pool: &mut Pool, types: &mut Vec<usize>, fields: &mut Vec<(usize, usize, usize)>, text: &str) -> Result<usize> {
    let (owner, rest) = text.split_once("->").ok_or("Field reference is missing '->'")?;
    let (name, ty) = rest.split_once(':').ok_or("Field reference is missing a type")?;
    let owner = intern_type(pool, types, owner);
    let name = pool.intern(name);
    let ty = intern_type(pool, types, ty);
    Ok(push_unique(fields, (owner, name, ty)))
}

fn intern_method(
    pool: &mut Pool,
    types: &mut Vec<usize>,
    methods: &mut Vec<(usize, usize, usize)>,
    protos: &mut Vec<(usize, Vec<usize>)>,
    text: &str,
) -> Result<usize> {
    let (owner, rest) = text.split_once("->").ok_or("Method reference is missing '->'")?;
    let split = rest.find('(').ok_or("Method reference is missing a descriptor")?;
    let name = &rest[..split];
    let descriptor = &rest[split..];
    let (ret, args) = split_descriptor(descriptor)?;
    let ret = intern_type(pool, types, &ret);
    let mut arg_idx = Vec::new();
    for arg in &args {
        arg_idx.push(intern_type(pool, types, arg));
    }
    let proto = push_unique(protos, (ret, arg_idx));
    let owner = intern_type(pool, types, owner);
    let name = pool.intern(name);
    Ok(push_unique(methods, (owner, proto, name)))
}

fn write_dex(file: &Dex) -> Result<Vec<u8>> {
    let string_count = file.strings.len();
    let type_count = file.types.len();
    let proto_count = file.protos.len();
    let field_count = file.fields.len();
    let method_count = file.methods.len();
    let class_count = file.classes.len();
    let mut data_off = 112 + string_count * 4 + type_count * 4 + proto_count * 12 + field_count * 8 + method_count * 8 + class_count * 32;
    let mut data = Vec::new();
    let mut string_offs = Vec::with_capacity(string_count);
    for text in &file.strings {
        string_offs.push(data_off + data.len());
        data.extend(encode_mutf8(text)?);
    }
    while (data_off + data.len()) % 4 != 0 {
        data.push(0);
    }
    let mut type_list_off = std::collections::BTreeMap::<Vec<usize>, usize>::new();
    let mut proto_param_off = vec![0usize; proto_count];
    for (i, (_, args)) in file.protos.iter().enumerate() {
        if args.is_empty() {
            continue;
        }
        if let Some(off) = type_list_off.get(args) {
            proto_param_off[i] = *off;
            continue;
        }
        let off = data_off + data.len();
        type_list_off.insert(args.clone(), off);
        proto_param_off[i] = off;
        put_u32(&mut data, args.len() as u32);
        for arg in args {
            put_u16(&mut data, *arg as u16);
        }
        if args.len() % 2 == 1 {
            put_u16(&mut data, 0);
        }
    }
    let mut interface_off = vec![0usize; class_count];
    for (i, class) in file.classes.iter().enumerate() {
        if class.interfaces.is_empty() {
            continue;
        }
        if let Some(off) = type_list_off.get(&class.interfaces) {
            interface_off[i] = *off;
            continue;
        }
        let off = data_off + data.len();
        type_list_off.insert(class.interfaces.clone(), off);
        interface_off[i] = off;
        put_u32(&mut data, class.interfaces.len() as u32);
        for interface in &class.interfaces {
            put_u16(&mut data, *interface as u16);
        }
        if class.interfaces.len() % 2 == 1 {
            put_u16(&mut data, 0);
        }
    }
    while (data_off + data.len()) % 4 != 0 {
        data.push(0);
    }
    let mut code_off = Vec::new();
    for class in &file.classes {
        for method in &class.methods {
            if let Some(code) = &method.code {
                while (data_off + data.len()) % 4 != 0 {
                    data.push(0);
                }
                code_off.push(Some(data_off + data.len()));
                write_code(&mut data, code)?;
            } else {
                code_off.push(None);
            }
        }
    }
    let mut class_data_off = Vec::new();
    let mut static_values_off = Vec::new();
    let mut code_cursor = 0usize;
    for class in &file.classes {
        class_data_off.push(data_off + data.len());
        write_class_data(&mut data, class, &code_off[code_cursor..code_cursor + class.methods.len()])?;
        code_cursor += class.methods.len();
        // The initial values of the static fields are written in their own
        // array item, one value for each static field in the order the class
        // data item declares them.
        let statics: Vec<&Member> = class.fields.iter().filter(|f| f.access & 0x8 != 0).collect();
        let values = statics.iter().rposition(|field| field.value.is_some()).map(|i| i + 1).unwrap_or(0);
        if values == 0 {
            static_values_off.push(0);
            continue;
        }
        while (data_off + data.len()) % 4 != 0 {
            data.push(0);
        }
        static_values_off.push(data_off + data.len());
        uleb_push(&mut data, values as u32);
        for field in statics.iter().take(values) {
            if let Some(value) = field.value.as_ref() {
                write_encoded_value(&mut data, value);
            } else {
                // An omitted value between two initialized static fields is
                // the field type's default. Encode that default so later
                // values retain their positions in the class data list.
                let field_type_idx = file.fields[field.idx].2;
                let field_type = &file.types[field_type_idx];
                write_encoded_value(&mut data, &default_field_value(field_type));
            }
        }
    }
    while (data_off + data.len()) % 4 != 0 {
        data.push(0);
    }
    let map_off = data_off + data.len();
    let mut map_items: Vec<(u16, u32, u32)> = vec![(0x0000, 1, 0)];
    let mut cursor = 112usize;
    if string_count > 0 {
        map_items.push((0x0001, string_count as u32, cursor as u32));
        cursor += string_count * 4;
    }
    if type_count > 0 {
        map_items.push((0x0002, type_count as u32, cursor as u32));
        cursor += type_count * 4;
    }
    if proto_count > 0 {
        map_items.push((0x0003, proto_count as u32, cursor as u32));
        cursor += proto_count * 12;
    }
    if field_count > 0 {
        map_items.push((0x0004, field_count as u32, cursor as u32));
        cursor += field_count * 8;
    }
    if method_count > 0 {
        map_items.push((0x0005, method_count as u32, cursor as u32));
        cursor += method_count * 8;
    }
    if class_count > 0 {
        map_items.push((0x0006, class_count as u32, cursor as u32));
    }
    if string_count > 0 {
        map_items.push((0x2002, string_count as u32, string_offs[0] as u32));
    }
    if let Some((_, off)) = type_list_off.iter().min_by_key(|(_, off)| *off) {
        map_items.push((0x1001, type_list_off.len() as u32, *off as u32));
    }
    let code_items = code_off.iter().filter_map(|v| *v).count();
    if code_items > 0 {
        let first = code_off.iter().filter_map(|v| *v).min().unwrap() as u32;
        map_items.push((0x2001, code_items as u32, first));
    }
    if class_count > 0 {
        map_items.push((0x2000, class_count as u32, class_data_off[0] as u32));
    }
    map_items.push((0x1000, 1, map_off as u32));
    map_items.sort_by_key(|item| item.2);
    put_u32(&mut data, map_items.len() as u32);
    for (kind, count, off) in &map_items {
        put_u16(&mut data, *kind);
        put_u16(&mut data, 0);
        put_u32(&mut data, *count);
        put_u32(&mut data, *off);
    }
    let mut out = vec![0u8; data_off];
    out.extend(data);
    out[0..8].copy_from_slice(b"dex\n035\0");
    let file_size = out.len() as u32;
    put_at(&mut out, 32, file_size);
    put_at(&mut out, 36, 112);
    put_at(&mut out, 40, 0x1234_5678);
    put_at(&mut out, 52, map_off as u32);
    put_at(&mut out, 56, string_count as u32);
    put_at(&mut out, 60, if string_count == 0 { 0 } else { 112u32 });
    let mut at = 112 + string_count * 4;
    put_at(&mut out, 64, type_count as u32);
    put_at(&mut out, 68, if type_count == 0 { 0 } else { at as u32 });
    if type_count > 0 {
        at += type_count * 4;
    }
    put_at(&mut out, 72, proto_count as u32);
    put_at(&mut out, 76, if proto_count == 0 { 0 } else { at as u32 });
    if proto_count > 0 {
        at += proto_count * 12;
    }
    put_at(&mut out, 80, field_count as u32);
    put_at(&mut out, 84, if field_count == 0 { 0 } else { at as u32 });
    if field_count > 0 {
        at += field_count * 8;
    }
    put_at(&mut out, 88, method_count as u32);
    put_at(&mut out, 92, if method_count == 0 { 0 } else { at as u32 });
    if method_count > 0 {
        at += method_count * 8;
    }
    put_at(&mut out, 96, class_count as u32);
    put_at(&mut out, 100, if class_count == 0 { 0 } else { at as u32 });
    let data_size = out.len() - data_off;
    put_at(&mut out, 104, data_size as u32);
    put_at(&mut out, 108, data_off as u32);
    let mut cursor = 112usize;
    for off in &string_offs {
        put_at(&mut out, cursor, *off as u32);
        cursor += 4;
    }
    for (i, name) in file.types.iter().enumerate() {
        let sid = file.strings.iter().position(|s| s == name).ok_or("Type string was dropped")?;
        put_at(&mut out, cursor + i * 4, sid as u32);
    }
    cursor += type_count * 4;
    for i in 0..proto_count {
        let (ret, _) = &file.protos[i];
        let shorty = shorty(file, i);
        let sid = file.strings.iter().position(|s| s == &shorty).unwrap_or_else(|| {
            // Shorty is interned below if missing. It must already exist.
            0
        });
        let _ = sid;
        cursor += 0;
        let _ = i;
    }
    // Strings were interned without shorty. Rebuild is too late if shorty is absent.
    // write_dex requires shorty strings to be present. emit() adds them before write.
    write_ids(&mut out, file, proto_param_off, interface_off, class_data_off, static_values_off)?;
    let sha = sha1(&out[32..]);
    out[12..32].copy_from_slice(&sha);
    let sum = adler32(&out[12..]);
    put_at(&mut out, 8, sum);
    Ok(out)
}

fn write_ids(
    out: &mut [u8],
    file: &Dex,
    proto_param_off: Vec<usize>,
    interface_off: Vec<usize>,
    class_data_off: Vec<usize>,
    static_values_off: Vec<usize>,
) -> Result<()> {
    let string_count = file.strings.len();
    let type_count = file.types.len();
    let proto_count = file.protos.len();
    let field_count = file.fields.len();
    let method_count = file.methods.len();
    let class_count = file.classes.len();
    if interface_off.len() != class_count || class_data_off.len() != class_count || static_values_off.len() != class_count {
        return Err("Class table offsets are inconsistent".into());
    }
    let mut cursor = 112 + string_count * 4;
    for (i, name) in file.types.iter().enumerate() {
        let sid = file.strings.iter().position(|s| s == name).ok_or("Type string was dropped")?;
        put_at(out, cursor + i * 4, sid as u32);
    }
    cursor += type_count * 4;
    for i in 0..proto_count {
        let (ret, _) = &file.protos[i];
        let shorty = shorty(file, i);
        let sid = file.strings.iter().position(|s| s == &shorty).ok_or(format!("Missing shorty {shorty}"))?;
        put_at(out, cursor, sid as u32);
        put_at(out, cursor + 4, *ret as u32);
        put_at(out, cursor + 8, proto_param_off[i] as u32);
        cursor += 12;
    }
    for (owner, name, ty) in &file.fields {
        put_u16_at(out, cursor, *owner as u16);
        put_u16_at(out, cursor + 2, *ty as u16);
        put_at(out, cursor + 4, *name as u32);
        cursor += 8;
    }
    for (owner, proto, name) in &file.methods {
        put_u16_at(out, cursor, *owner as u16);
        put_u16_at(out, cursor + 2, *proto as u16);
        put_at(out, cursor + 4, *name as u32);
        cursor += 8;
    }
    for (i, class) in file.classes.iter().enumerate() {
        put_at(out, cursor, class.type_idx as u32);
        put_at(out, cursor + 4, class.access);
        put_at(out, cursor + 8, class.super_idx as u32);
        put_at(out, cursor + 12, interface_off[i] as u32);
        put_at(out, cursor + 16, class.source.map(|v| v as u32).unwrap_or(NO_INDEX));
        put_at(out, cursor + 20, 0);
        put_at(out, cursor + 24, class_data_off[i] as u32);
        put_at(out, cursor + 28, static_values_off[i] as u32);
        cursor += 32;
    }
    let _ = (field_count, method_count);
    Ok(())
}

fn shorty(file: &Dex, proto: usize) -> String {
    let (ret, args) = &file.protos[proto];
    let mut out = String::new();
    out.push(short_char(&file.types[*ret]));
    for arg in args {
        out.push(short_char(&file.types[*arg]));
    }
    out
}

fn short_char(ty: &str) -> char {
    let base = ty.trim_start_matches('[');
    match base.as_bytes().first().copied().unwrap_or(b'L') {
        b'V' | b'Z' | b'B' | b'S' | b'C' | b'I' | b'J' | b'F' | b'D' if base.len() == 1 => base.chars().next().unwrap(),
        _ => 'L',
    }
}

fn ensure_shorty(file: &mut Dex) {
    let mut extra = Vec::new();
    for i in 0..file.protos.len() {
        let text = shorty(file, i);
        if !file.strings.iter().any(|s| s == &text) {
            extra.push(text);
        }
    }
    for text in extra {
        file.strings.push(text);
    }
}

fn write_code(out: &mut Vec<u8>, code: &Code) -> Result<()> {
    let words = encode_words(code)?;
    put_u16(out, code.registers);
    put_u16(out, code.ins);
    put_u16(out, outs_size(code));
    put_u16(out, code.catches.len() as u16);
    put_u32(out, 0);
    put_u32(out, words.len() as u32);
    for word in &words {
        put_u16(out, *word);
    }
    if code.catches.is_empty() {
        return Ok(());
    }
    if words.len() % 2 == 1 {
        put_u16(out, 0);
    }
    let mut handlers = Vec::new();
    let mut encoded = Vec::new();
    uleb_push(&mut encoded, code.catches.len() as u32);
    for catch in &code.catches {
        let start = encoded.len();
        if let Some(ty) = catch.ty {
            sleb_push(&mut encoded, 1);
            uleb_push(&mut encoded, ty as u32);
            uleb_push(&mut encoded, catch.handler);
        } else {
            // A zero count denotes a catch-all with no typed handlers. A
            // negative count would advertise a typed handler as well, causing
            // the following handler entries to be read at the wrong offsets.
            sleb_push(&mut encoded, 0);
            uleb_push(&mut encoded, catch.handler);
        }
        handlers.push(start);
        let _ = catch;
    }
    for (catch, off) in code.catches.iter().zip(handlers) {
        put_u32(out, catch.start);
        put_u16(out, catch.end.wrapping_sub(catch.start) as u16);
        put_u16(out, off as u16);
    }
    out.extend(encoded);
    Ok(())
}

fn outs_size(code: &Code) -> u16 {
    let mut max = 0u16;
    for insn in &code.insns {
        if let Insn::Op { name, regs, .. } = insn {
            if name.starts_with("invoke-") {
                max = max.max(regs.len() as u16);
            }
        }
    }
    max
}

fn encode_words(code: &Code) -> Result<Vec<u16>> {
    let mut words = Vec::new();
    let mut at = 0u32;
    for insn in &code.insns {
        match insn {
            Insn::Raw(raw) => words.extend(raw),
            Insn::Op { name, regs, imm, target, index } => {
                let op = name_code(name).ok_or(format!("Unknown instruction {name}"))?;
                let rel = target.map(|dest| dest.wrapping_sub(at) as i32);
                match name.as_str() {
                    "nop" | "return-void" => words.push(op as u16),
                    "return" | "return-wide" | "return-object" | "throw" | "monitor-enter" | "monitor-exit"
                    | "move-result" | "move-result-wide" | "move-result-object" | "move-exception" => {
                        words.push(((regs.first().copied().unwrap_or(0) as u16) << 8) | op as u16);
                    }
                    "const/4" => {
                        let n = (*imm.as_ref().ok_or("const/4 is missing a value")? as i8) as u8;
                        words.push((((n & 0xf) as u16) << 12) | ((regs[0] as u16) << 8) | op as u16);
                    }
                    "array-length" => words.push(((regs[1] as u16) << 12) | ((regs[0] as u16) << 8) | op as u16),
                    "goto" => {
                        let off = rel.ok_or("goto is missing a target")? as i8;
                        words.push(((off as u8 as u16) << 8) | op as u16);
                    }
                    "const/16" => {
                        words.push(((regs[0] as u16) << 8) | op as u16);
                        words.push(*imm.as_ref().ok_or("const/16 is missing a value")? as u16);
                    }
                    "add-int/lit8" => {
                        words.push(((regs[0] as u16) << 8) | op as u16);
                        let lit = *imm.as_ref().ok_or("literal is missing")? as i8 as u8;
                        words.push(((lit as u16) << 8) | regs[1] as u16);
                    }
                    "const-string" | "const-class" | "check-cast" | "new-instance" | "sget" | "sget-object" | "sput" | "sput-object" => {
                        words.push(((regs[0] as u16) << 8) | op as u16);
                        words.push(index.ok_or("Reference is missing")?.1 as u16);
                    }
                    "iget" | "iget-object" | "iput" | "iput-object" | "instance-of" | "new-array" => {
                        words.push(((regs[1] as u16) << 12) | ((regs[0] as u16) << 8) | op as u16);
                        words.push(index.ok_or("Reference is missing")?.1 as u16);
                    }
                    "if-eqz" | "if-nez" | "if-ltz" | "if-gez" | "if-gtz" | "if-lez" => {
                        words.push(((regs[0] as u16) << 8) | op as u16);
                        words.push(rel.ok_or("Branch is missing a target")? as u16);
                    }
                    "if-eq" | "if-ne" | "if-lt" | "if-ge" | "if-gt" | "if-le" => {
                        words.push(((regs[1] as u16) << 12) | ((regs[0] as u16) << 8) | op as u16);
                        words.push(rel.ok_or("Branch is missing a target")? as u16);
                    }
                    "goto/16" => {
                        words.push(op as u16);
                        words.push(rel.ok_or("Branch is missing a target")? as u16);
                    }
                    "const" => {
                        let imm = *imm.as_ref().ok_or("const is missing a value")? as u32;
                        words.push(((regs[0] as u16) << 8) | op as u16);
                        words.push(imm as u16);
                        words.push((imm >> 16) as u16);
                    }
                    "const-wide" => {
                        let imm = *imm.as_ref().ok_or("const-wide is missing a value")? as u64;
                        words.push(((regs[0] as u16) << 8) | op as u16);
                        words.push(imm as u16);
                        words.push((imm >> 16) as u16);
                        words.push((imm >> 32) as u16);
                        words.push((imm >> 48) as u16);
                    }
                    "invoke-virtual" | "invoke-super" | "invoke-direct" | "invoke-static" | "invoke-interface" => {
                        let mut regs = regs.clone();
                        while regs.len() < 5 {
                            regs.push(0);
                        }
                        let count = index.map(|_| regs.iter().take_while(|_| true).count()).unwrap_or(0);
                        let count = if name.starts_with("invoke-") { words_count(code, name) } else { count };
                        let _ = count;
                        let argc = actual_argc(code, at);
                        let g = if argc > 4 { regs[4] } else { 0 };
                        words.push(((g as u16) << 12) | ((argc as u16) << 8) | op as u16);
                        words.push(index.ok_or("Invoke is missing a method")?.1 as u16);
                        let c = if argc > 0 { regs[0] } else { 0 };
                        let d = if argc > 1 { regs[1] } else { 0 };
                        let e = if argc > 2 { regs[2] } else { 0 };
                        let f = if argc > 3 { regs[3] } else { 0 };
                        words.push(((f as u16) << 12) | ((e as u16) << 8) | ((d as u16) << 4) | c as u16);
                    }
                    "packed-switch" | "sparse-switch" => {
                        // A switch instruction is 31t: three code units.
                        // Its second and third units carry the distance to
                        // the payload table it dispatches through.
                        let to = rel.ok_or("Switch is missing its payload table")? as u32;
                        words.push(((regs.first().copied().unwrap_or(0) as u16) << 8) | op as u16);
                        words.push((to & 0xffff) as u16);
                        words.push((to >> 16) as u16);
                    }
                    other => return Err(format!("Cannot encode {other}")),
                }
            }
            Insn::Switch(payload) => {
                // A switch instruction is 31t: three code units. Its second
                // and third units carry the distance to the payload table it
                // dispatches through.
                let op = if payload.kind == SwitchKind::Packed { 0x2bu16 } else { 0x2cu16 };
                let to = payload.payload_at.wrapping_sub(at);
                words.push(((payload.reg as u16) << 8) | op);
                words.push((to & 0xffff) as u16);
                words.push((to >> 16) as u16);
            }
            Insn::SwitchData(table) => {
                // A payload table carries its kind word, its case count, and
                // then one entry per case.
                match table.kind {
                    SwitchKind::Packed => {
                        words.push(0x0100);
                        words.push(table.targets.len() as u16);
                        words.push(table.first_key as u16);
                        words.push((table.first_key >> 16) as u16);
                        for target in &table.targets {
                            let to = target.wrapping_sub(table.switch_at);
                            words.push((to & 0xffff) as u16);
                            words.push((to >> 16) as u16);
                        }
                    }
                    SwitchKind::Sparse => {
                        words.push(0x0200);
                        words.push(table.keys.len() as u16);
                        for key in &table.keys {
                            words.push(*key as u16);
                            words.push((*key >> 16) as u16);
                        }
                        for target in &table.targets {
                            let to = target.wrapping_sub(table.switch_at);
                            words.push((to & 0xffff) as u16);
                            words.push((to >> 16) as u16);
                        }
                    }
                }
            }
        }
        at += insn.width();
    }
    let _ = at;
    Ok(words)
}

fn words_count(_code: &Code, _name: &str) -> usize { 0 }
fn actual_argc(code: &Code, at: u32) -> usize {
    let mut cursor = 0u32;
    for insn in &code.insns {
        if cursor == at {
            if let Insn::Op { regs, .. } = insn {
                return regs.len();
            }
        }
        cursor += insn.width();
    }
    0
}

fn write_class_data(out: &mut Vec<u8>, class: &Class, code_off: &[Option<usize>]) -> Result<()> {
    let statics: Vec<&Member> = class.fields.iter().filter(|f| f.access & 0x8 != 0).collect();
    let instances: Vec<&Member> = class.fields.iter().filter(|f| f.access & 0x8 == 0).collect();
    let direct: Vec<usize> = class.methods.iter().enumerate().filter(|(_, m)| m.access & 0x8 != 0 || m.access & 0x2 != 0 || m.access & 0x10000 != 0).map(|(i, _)| i).collect();
    let virtuals: Vec<usize> = class.methods.iter().enumerate().filter(|(i, _)| !direct.contains(i)).map(|(i, _)| i).collect();
    uleb_push(out, statics.len() as u32);
    uleb_push(out, instances.len() as u32);
    uleb_push(out, direct.len() as u32);
    uleb_push(out, virtuals.len() as u32);
    for group in [statics, instances] {
        let mut prev = 0u32;
        for field in group {
            uleb_push(out, field.idx as u32 - prev);
            prev = field.idx as u32;
            uleb_push(out, field.access);
        }
    }
    for group in [direct, virtuals] {
        let mut prev = 0u32;
        for idx in group {
            let method = &class.methods[idx];
            uleb_push(out, method.idx as u32 - prev);
            prev = method.idx as u32;
            uleb_push(out, method.access);
            uleb_push(out, code_off[idx].unwrap_or(0) as u32);
        }
    }
    Ok(())
}

/// Writes one initial value into the encoded array item. The value is kept
/// as the text the reference writes, and the text names the kind of value,
/// so the value is read back from the text and then stored in the shape the
/// kind declares.
/// Writes one initial value into the encoded array item. The value names
/// the kind of value it is, so the kind is written first and the value
/// follows in the shape that kind declares. A number is stored in as many
/// bytes as it needs and the argument names how many, and a value that names
/// a string, a type, a field or a method holds the number it has in the
/// table it belongs to.
fn write_encoded_value(out: &mut Vec<u8>, value: &FieldValue) {
    match value {
        FieldValue::Null => out.push(0x1e),
        FieldValue::Boolean(true) => out.push(0x3f),
        FieldValue::Boolean(false) => out.push(0x1f),
        FieldValue::Number { kind, raw, width } => {
            out.push((((*width).saturating_sub(1) as u8) << 5) | *kind);
            out.extend_from_slice(&raw.to_le_bytes()[..(*width).min(8)]);
        }
        FieldValue::String(index) => write_encoded_index(out, 0x17, *index as u32),
        FieldValue::Type(index) => write_encoded_index(out, 0x18, *index as u32),
        FieldValue::Field(index) => write_encoded_index(out, 0x19, *index as u32),
        FieldValue::Enum(index) => write_encoded_index(out, 0x1b, *index as u32),
        FieldValue::Method(index) => write_encoded_index(out, 0x1a, *index as u32),
        FieldValue::MethodType(index) => write_encoded_index(out, 0x15, *index as u32),
        FieldValue::MethodHandle(index) => write_encoded_index(out, 0x16, *index as u32),
        FieldValue::Array(items) => {
            out.push(0x1c);
            write_encoded_count(out, items.len() as u32);
            for item in items {
                write_encoded_value(out, item);
            }
        }
        FieldValue::Annotation { type_idx, elements } => {
            out.push(0x1d);
            write_encoded_count(out, *type_idx as u32);
            write_encoded_count(out, elements.len() as u32);
            for (name, item) in elements {
                write_encoded_count(out, *name as u32);
                write_encoded_value(out, item);
            }
        }
    }
}

fn default_field_value(field_type: &str) -> FieldValue {
    match field_type {
        "B" => FieldValue::Number { kind: 0x00, raw: 0, width: 1 },
        "S" => FieldValue::Number { kind: 0x02, raw: 0, width: 2 },
        "C" => FieldValue::Number { kind: 0x03, raw: 0, width: 2 },
        "I" => FieldValue::Number { kind: 0x04, raw: 0, width: 4 },
        "J" => FieldValue::Number { kind: 0x06, raw: 0, width: 8 },
        "F" => FieldValue::Number { kind: 0x10, raw: 0, width: 4 },
        "D" => FieldValue::Number { kind: 0x11, raw: 0, width: 8 },
        "Z" => FieldValue::Boolean(false),
        _ => FieldValue::Null,
    }
}

/// Writes the number a value has in the table it belongs to. The number is
/// stored in as many bytes as it needs and the argument names how many.
fn write_encoded_index(out: &mut Vec<u8>, kind: u8, index: u32) {
    let width = if index == 0 {
        1
    } else {
        (32 - index.leading_zeros() as usize).div_ceil(8).max(1)
    };
    out.push(((width - 1) as u8) << 5 | kind);
    out.extend_from_slice(&index.to_le_bytes()[..width]);
}

fn remap_field_value(
    value: &mut FieldValue,
    strings: &[usize],
    types: &[usize],
    fields: &[usize],
    methods: &[usize],
    protos: &[usize],
) {
    match value {
        FieldValue::String(index) => *index = strings[*index],
        FieldValue::Type(index) => *index = types[*index],
        FieldValue::Field(index) | FieldValue::Enum(index) => *index = fields[*index],
        FieldValue::Method(index) => *index = methods[*index],
        FieldValue::MethodType(index) => *index = protos[*index],
        FieldValue::Array(items) => {
            for item in items {
                remap_field_value(item, strings, types, fields, methods, protos);
            }
        }
        FieldValue::Annotation { type_idx, elements } => {
            *type_idx = types[*type_idx];
            for (name, item) in elements {
                *name = strings[*name];
                remap_field_value(item, strings, types, fields, methods, protos);
            }
        }
        FieldValue::Null
        | FieldValue::Boolean(_)
        | FieldValue::Number { .. }
        | FieldValue::MethodHandle(_) => {}
    }
}

/// Writes a count that follows a kind word. The count is stored in as many
/// bytes as it needs and the argument names how many.
fn write_encoded_count(out: &mut Vec<u8>, count: u32) {
    let width = if count == 0 {
        1
    } else {
        (32 - count.leading_zeros() as usize).div_ceil(8).max(1)
    };
    out.push(((width - 1) as u8) << 5);
    out.extend_from_slice(&count.to_le_bytes()[..width]);
}

/// Reads the initial value of a static field back from the method text.
/// The text names the kind of value it is, so the kind is read from the text
/// and the value follows in the shape that kind declares. A value that names
/// a string, a type, a field or a method is looked up in the table it
/// belongs to, so the number it holds is the number the table gives it.
fn parse_value_text(
    pool: &mut Pool,
    types: &mut Vec<usize>,
    fields: &mut Vec<(usize, usize, usize)>,
    methods: &mut Vec<(usize, usize, usize)>,
    protos: &mut Vec<(usize, Vec<usize>)>,
    field_type: &str,
    text: &str,
) -> Result<FieldValue> {
    let text = text.trim();
    if text == "null" {
        return Ok(FieldValue::Null);
    }
    if text == "true" {
        return Ok(FieldValue::Boolean(true));
    }
    if text == "false" {
        return Ok(FieldValue::Boolean(false));
    }
    if text.starts_with('"') {
        let value = unquote(text)?;
        return Ok(FieldValue::String(pool.intern(&value)));
    }
    let body = text;
    // The field descriptor supplies the encoded numeric kind. Looking at
    // letters in the literal would mistake the `e` in a hexadecimal resource
    // id for an exponent and would lose the distinction between a float and
    // an integer-looking double such as `0`.
    match field_type {
        "B" | "S" | "C" | "I" | "J" => {
            let body = text.strip_suffix('L').unwrap_or(text);
            let value = parse_i64(body)?;
            let (kind, width) = match field_type {
                "B" => (0x00, 1),
                "S" => (0x02, 2),
                "C" => (0x03, 2),
                "I" => (0x04, 4),
                "J" => (0x06, 8),
                _ => unreachable!(),
            };
            return Ok(FieldValue::Number { kind, raw: value as u64, width });
        }
        "F" => {
            let body = text.strip_suffix('f').or_else(|| text.strip_suffix('F')).unwrap_or(text);
            let number: f32 = body.parse().map_err(|_| format!("Bad number {text}"))?;
            return Ok(FieldValue::Number { kind: 0x10, raw: u64::from(number.to_bits()), width: 4 });
        }
        "D" => {
            let body = text.strip_suffix('d').or_else(|| text.strip_suffix('D')).unwrap_or(text);
            let number: f64 = body.parse().map_err(|_| format!("Bad number {text}"))?;
            return Ok(FieldValue::Number { kind: 0x11, raw: number.to_bits(), width: 8 });
        }
        _ => {}
    }
    // A value that names a type or a field or a method is looked up in the
    // table it belongs to.
    if body.contains("->") && body.contains(':') {
        let index = intern_field(pool, types, fields, body)?;
        let value_type = &pool.strings[types[fields[index].2]];
        if value_type == field_type {
            return Ok(FieldValue::Enum(index));
        }
        return Ok(FieldValue::Field(index));
    }
    if body.contains("->") {
        let index = intern_method(pool, types, methods, protos, body)?;
        return Ok(FieldValue::Method(index));
    }
    if body.starts_with('L') || body.starts_with('[') {
        let index = intern_type(pool, types, body);
        return Ok(FieldValue::Type(index));
    }
    if body.starts_with('(') {
        let (ret, args) = split_descriptor(body)?;
        let ret_idx = intern_type(pool, types, &ret);
        let mut arg_idx = Vec::with_capacity(args.len());
        for arg in &args {
            arg_idx.push(intern_type(pool, types, arg));
        }
        return Ok(FieldValue::MethodType(push_unique(protos, (ret_idx, arg_idx))));
    }
    Err(format!("Unknown field value {text}"))
}

fn encode_mutf8(text: &str) -> Result<Vec<u8>> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out = Vec::new();
    uleb_push(&mut out, units.len() as u32);
    for unit in units {
        if unit == 0 {
            out.extend_from_slice(&[0xc0, 0x80]);
        } else if unit < 0x80 {
            out.push(unit as u8);
        } else if unit < 0x800 {
            out.push(0xc0 | (unit >> 6) as u8);
            out.push(0x80 | (unit & 0x3f) as u8);
        } else {
            out.push(0xe0 | (unit >> 12) as u8);
            out.push(0x80 | ((unit >> 6) & 0x3f) as u8);
            out.push(0x80 | (unit & 0x3f) as u8);
        }
    }
    out.push(0);
    Ok(out)
}

fn read_mutf8(bytes: &[u8], mut at: usize) -> Result<String> {
    let units = uleb(bytes, &mut at)? as usize;
    let mut utf16 = Vec::with_capacity(units);
    while utf16.len() < units {
        let b = *bytes.get(at).ok_or("String is truncated")?;
        at += 1;
        if b == 0 {
            return Err("String ended early".into());
        }
        let unit = if b < 0x80 {
            b as u16
        } else if b & 0xe0 == 0xc0 {
            let d = *bytes.get(at).ok_or("String is truncated")?;
            at += 1;
            ((b as u16 & 0x1f) << 6) | (d as u16 & 0x3f)
        } else if b & 0xf0 == 0xe0 {
            let d = *bytes.get(at).ok_or("String is truncated")?;
            let e = *bytes.get(at + 1).ok_or("String is truncated")?;
            at += 2;
            ((b as u16 & 0x0f) << 12) | ((d as u16 & 0x3f) << 6) | (e as u16 & 0x3f)
        } else {
            return Err("String byte is not MUTF-8".into());
        };
        utf16.push(unit);
    }
    if bytes.get(at) != Some(&0) {
        return Err("String is missing its terminator".into());
    }
    Ok(String::from_utf16(&utf16).map_err(|_| "String is not Unicode")?)
}

fn read_type_list(bytes: &[u8], at: usize) -> Result<Vec<usize>> {
    let n = u32_at(bytes, at)? as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(u16_at(bytes, at + 4 + i * 2)? as usize);
    }
    Ok(out)
}

fn table(bytes: &[u8], at: usize) -> Result<(usize, usize)> {
    let count = u32_at(bytes, at)? as usize;
    let off = u32_at(bytes, at + 4)? as usize;
    if count == 0 {
        return Ok((0, 0));
    }
    Ok((count, off))
}

fn uleb(bytes: &[u8], at: &mut usize) -> Result<u32> {
    let mut value = 0u32;
    for i in 0..5 {
        let b = *bytes.get(*at).ok_or("Truncated ULEB128")?;
        *at += 1;
        value |= ((b & 0x7f) as u32) << (i * 7);
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("ULEB128 is too long".into())
}

fn sleb(bytes: &[u8], at: &mut usize) -> Result<i32> {
    let mut value = 0i32;
    for i in 0..5 {
        let b = *bytes.get(*at).ok_or("Truncated SLEB128")?;
        *at += 1;
        value |= ((b & 0x7f) as i32) << (i * 7);
        if b & 0x80 == 0 {
            if b & 0x40 != 0 && i < 4 {
                value |= !0 << ((i + 1) * 7);
            }
            return Ok(value);
        }
    }
    Err("SLEB128 is too long".into())
}

fn uleb_push(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn sleb_push(out: &mut Vec<u8>, value: i32) {
    let mut more = true;
    let mut word = value;
    while more {
        let mut byte = (word as u8) & 0x7f;
        word >>= 7;
        let sign = (byte & 0x40) != 0;
        if (word == 0 && !sign) || (word == -1 && sign) {
            more = false;
        } else {
            byte |= 0x80;
        }
        out.push(byte);
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    let s = bytes.get(at..at + 2).ok_or("Truncated executable")?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let s = bytes.get(at..at + 4).ok_or("Truncated executable")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u64_at(bytes: &[u8], at: usize) -> Result<u64> {
    let s = bytes.get(at..at + 8).ok_or("Truncated executable")?;
    Ok(u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}
fn put_u16(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_at(out: &mut [u8], at: usize, value: u32) { out[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn put_u16_at(out: &mut [u8], at: usize, value: u16) { out[at..at + 2].copy_from_slice(&value.to_le_bytes()); }

fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in bytes {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn sha1(data: &[u8]) -> [u8; 20] {
    const H0: [u32; 5] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0];
    fn block(state: &mut [u32; 5], chunk: &[u8]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut state = H0;
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for chunk in message.chunks(64) {
        block(&mut state, chunk);
    }
    let mut out = [0u8; 20];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn decode_insns(words: &[u16]) -> Result<Vec<Insn>> {
    decode_insns_fixed(words)
}

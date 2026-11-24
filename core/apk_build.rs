//! On-device package build and edit.
//!
//! Unpack an archive, edit binary XML and the resource table, replace a
//! method through its text form, rebuild, align, and sign. Each step is a
//! tool the harness can call.

use super::*;

pub(crate) const TOOLS: &[&str] = &[
    "apk_unpack",
    "apk_read_strings",
    "apk_edit_string",
    "apk_edit_xml",
    "apk_smali",
    "apk_assemble",
    "apk_rebuild",
    "apk_align",
    "apk_sign",
    "apk_verify",
];

pub(crate) fn run(tool: &str, args: &Json) -> Result<Json> {
    match tool {
        "apk_unpack" => {
            let apk = apk_bytes(args)?;
            let members = super::zipalign::read_members(&apk)?;
            let entries: Vec<Json> = members
                .iter()
                .map(|member| {
                    obj(vec![
                        ("name", s(&member.name)),
                        ("size", Num(member.data.len().to_string())),
                        ("data_b64", s(&super::network::b64(&member.data))),
                    ])
                })
                .collect();
            Ok(obj(vec![("entries", Arr(entries)), ("count", Num(members.len().to_string()))]))
        }
        "apk_read_strings" => {
            let apk = apk_bytes(args)?;
            let members = super::zipalign::read_members(&apk)?;
            let mut lines = Vec::new();
            if let Some(xml) = member(&members, "AndroidManifest.xml") {
                for (index, text) in xml_strings(xml)?.iter().enumerate() {
                    lines.push(s(&format!("xml {index} {text}")));
                }
            }
            if let Some(table) = member(&members, "resources.arsc") {
                for (index, text) in arsc_strings(table)?.iter().enumerate() {
                    lines.push(s(&format!("resource {index} {text}")));
                }
            }
            Ok(obj(vec![("strings", Arr(lines))]))
        }
        "apk_edit_string" => {
            let apk = apk_bytes(args)?;
            let from = text_arg(args, "old")?;
            let to = text_arg(args, "new")?;
            let mut members = super::zipalign::read_members(&apk)?;
            let table = members.iter().position(|member| member.name == "resources.arsc").ok_or("resources.arsc is missing")?;
            members[table].data = edit_arsc_string(&members[table].data, from, to)?;
            members.retain(|member| !signature_entry(&member.name));
            let pairs = pairs_of(&members);
            let built = super::zipalign::write_stored(&pairs, 4)?;
            Ok(obj(vec![("apk_b64", s(&super::network::b64(&built)))]))
        }
        "apk_edit_xml" => {
            let apk = apk_bytes(args)?;
            let from = text_arg(args, "old")?;
            let to = text_arg(args, "new")?;
            let mut members = super::zipalign::read_members(&apk)?;
            let xml = members.iter().position(|member| member.name == "AndroidManifest.xml").ok_or("AndroidManifest.xml is missing")?;
            members[xml].data = edit_xml_string(&members[xml].data, from, to)?;
            members.retain(|member| !signature_entry(&member.name));
            let pairs = pairs_of(&members);
            let built = super::zipalign::write_stored(&pairs, 4)?;
            Ok(obj(vec![("apk_b64", s(&super::network::b64(&built)))]))
        }
        "apk_smali" => {
            let dex = dex_bytes(args)?;
            Ok(obj(vec![("smali", s(&super::smali::disassemble(&dex)?))]))
        }
        "apk_assemble" => {
            let text = text_arg(args, "smali")?;
            let dex = super::smali::assemble(text)?;
            if args.get("apk_b64").is_ok() {
                let mut members = super::zipalign::read_members(&apk_bytes(args)?)?;
                let dex_entry = optional_text(args, "dex_entry").unwrap_or("classes.dex");
                if !valid_dex_entry(dex_entry) {
                    return Err("dex_entry must name a classes*.dex member".into());
                }
                if let Some(slot) = members.iter_mut().find(|member| member.name == dex_entry) {
                    slot.data = dex;
                } else {
                    if dex_entry != "classes.dex" {
                        return Err(format!("DEX member {dex_entry} is missing"));
                    }
                    members.push(super::zipalign::Member { name: dex_entry.into(), data: dex });
                }
                members.retain(|member| !signature_entry(&member.name));
                let built = super::zipalign::write_stored(&pairs_of(&members), 4)?;
                Ok(obj(vec![("apk_b64", s(&super::network::b64(&built)))]))
            } else {
                Ok(obj(vec![("dex_b64", s(&super::network::b64(&dex)))]))
            }
        }
        "apk_rebuild" => {
            let list = args.get("entries")?.array()?;
            let mut pairs = Vec::new();
            for entry in list {
                let name = entry.get("name")?.text()?.to_string();
                let data = super::network::unbase64(entry.get("data_b64")?.text()?)?;
                if !signature_entry(&name) {
                    pairs.push((name, data));
                }
            }
            let built = super::zipalign::write_stored(&pairs, 4)?;
            Ok(obj(vec![("apk_b64", s(&super::network::b64(&built)))]))
        }
        "apk_align" => {
            let apk = apk_bytes(args)?;
            let alignment = u32::try_from(number_or(args, "alignment", 4)?).map_err(|_| "Alignment exceeds the supported integer range")?;
            let aligned = super::zipalign::align(&apk, alignment)?;
            let report = super::zipalign::verify(&aligned, alignment)?;
            Ok(obj(vec![("apk_b64", s(&super::network::b64(&aligned))), ("report", s(&report))]))
        }
        "apk_sign" => {
            let apk = apk_bytes(args)?;
            let bits = usize::try_from(number_or(args, "bits", 2048)?).map_err(|_| "Key size exceeds the supported integer range")?;
            let signed = super::apk_sign::sign(&apk, bits)?;
            Ok(obj(vec![("apk_b64", s(&super::network::b64(&signed)))]))
        }
        "apk_verify" => {
            let apk = apk_bytes(args)?;
            let alignment = u32::try_from(number_or(args, "alignment", 4)?).map_err(|_| "Alignment exceeds the supported integer range")?;
            let report = super::apk_sign::verify(&apk)?;
            let aligned = super::zipalign::verify(&apk, alignment)?;
            Ok(obj(vec![
                ("v1", Bool(report.v1)),
                ("v2", Bool(report.v2)),
                ("v3", Bool(report.v3)),
                ("detail", s(&report.detail)),
                ("alignment", s(&aligned)),
            ]))
        }
        _ => Err(format!("Unknown package tool {tool}")),
    }
}

fn apk_bytes(args: &Json) -> Result<Vec<u8>> {
    super::network::unbase64(text_arg(args, "apk_b64")?)
}

fn dex_bytes(args: &Json) -> Result<Vec<u8>> {
    if args.get("dex_b64").is_ok() {
        return super::network::unbase64(text_arg(args, "dex_b64")?);
    }
    let members = super::zipalign::read_members(&apk_bytes(args)?)?;
    let dex_entry = optional_text(args, "dex_entry").unwrap_or("classes.dex");
    if !valid_dex_entry(dex_entry) {
        return Err("dex_entry must name a classes*.dex member".into());
    }
    member(&members, dex_entry).map(|bytes| bytes.to_vec()).ok_or_else(|| format!("{dex_entry} is missing"))
}

fn text_arg<'a>(args: &'a Json, key: &str) -> Result<&'a str> {
    args.get(key)?.text()
}

fn number_or(args: &Json, key: &str, default: u64) -> Result<u64> {
    match args.get(key) {
        Ok(Num(text)) | Ok(Str(text)) => text.parse().map_err(|_| format!("{key} must be an unsigned integer")),
        Err(_) => Ok(default),
        Ok(_) => Err(format!("{key} must be an unsigned integer")),
    }
}

fn valid_dex_entry(name: &str) -> bool {
    if name == "classes.dex" {
        return true;
    }
    let Some(number) = name.strip_prefix("classes").and_then(|value| value.strip_suffix(".dex")) else {
        return false;
    };
    !number.is_empty()
        && number.chars().all(|character| character.is_ascii_digit())
        && !name.chars().any(|character| character == '/' || character == '\\')
}

fn optional_text<'a>(args: &'a Json, key: &str) -> Option<&'a str> {
    args.get(key).ok()?.text().ok()
}

fn signature_entry(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "META-INF/MANIFEST.MF"
        || (upper.starts_with("META-INF/")
            && (upper.ends_with(".SF") || upper.ends_with(".RSA") || upper.ends_with(".DSA") || upper.ends_with(".EC")))
}

fn member<'a>(members: &'a [super::zipalign::Member], name: &str) -> Option<&'a [u8]> {
    members.iter().find(|member| member.name == name).map(|member| member.data.as_slice())
}

fn pairs_of(members: &[super::zipalign::Member]) -> Vec<(String, Vec<u8>)> {
    members.iter().map(|member| (member.name.clone(), member.data.clone())).collect()
}

pub(crate) fn edit_arsc_string(table: &[u8], from: &str, to: &str) -> Result<Vec<u8>> {
    replace_first_pool(table, 0x0002, from, to)
}

pub(crate) fn edit_xml_string(xml: &[u8], from: &str, to: &str) -> Result<Vec<u8>> {
    replace_first_pool(xml, 0x0003, from, to)
}

pub(crate) fn arsc_strings(table: &[u8]) -> Result<Vec<String>> {
    pool_strings(table, 0x0002)
}

pub(crate) fn xml_strings(xml: &[u8]) -> Result<Vec<String>> {
    pool_strings(xml, 0x0003)
}

fn pool_strings(file: &[u8], kind: u16) -> Result<Vec<String>> {
    if file.len() < 8 || u16_at(file, 0)? != kind {
        return Err("Resource document header is not intact".into());
    }
    let header = u16_at(file, 2)? as usize;
    Ok(parse_pool(file.get(header..).ok_or("Resource pool is missing")?)?.strings)
}

fn replace_first_pool(file: &[u8], kind: u16, from: &str, to: &str) -> Result<Vec<u8>> {
    if file.len() < 8 || u16_at(file, 0)? != kind {
        return Err("Resource document header is not intact".into());
    }
    let header = u16_at(file, 2)? as usize;
    let mut pool = parse_pool(file.get(header..).ok_or("Resource pool is missing")?)?;
    let mut found = false;
    let mut changed = Vec::new();
    for (index, text) in pool.strings.iter_mut().enumerate() {
        if text == from {
            *text = to.to_string();
            found = true;
            changed.push(index);
        }
    }
    if !found {
        return Err(format!("String resource {from} was not found"));
    }
    for index in changed {
        ensure_style_fits(&pool, index, to)?;
    }
    if from != to {
        let flags = u32_at(&pool.header, 16)? & !1;
        put_u32_at(&mut pool.header, 16, flags);
    }
    let pool_len = pool.size;
    let encoded = pool.encode()?;
    let mut out = file[..header].to_vec();
    out.extend(encoded);
    out.extend_from_slice(file.get(header + pool_len..).ok_or("Resource pool is truncated")?);
    let total = u32::try_from(out.len()).map_err(|_| "Resource document is too large")?;
    put_u32_at(&mut out, 4, total);
    Ok(out)
}

struct StringPool {
    header: Vec<u8>,
    strings: Vec<String>,
    style_offsets: Vec<u32>,
    styles: Vec<u8>,
    size: usize,
    utf8: bool,
}

impl StringPool {
    fn encode(&self) -> Result<Vec<u8>> {
        let header_size = self.header.len();
        let count = u32::try_from(self.strings.len()).map_err(|_| "String pool has too many strings")?;
        let style_count = u32::try_from(self.style_offsets.len()).map_err(|_| "String pool has too many styles")?;
        let offsets_bytes = self.strings.len().checked_mul(4).ok_or("String pool offsets overflow")?;
        let style_offsets_bytes = self.style_offsets.len().checked_mul(4).ok_or("String pool style offsets overflow")?;
        let strings_start = header_size.checked_add(offsets_bytes).and_then(|n| n.checked_add(style_offsets_bytes)).ok_or("String pool header is too large")?;
        let mut data = Vec::new();
        let mut offsets = Vec::with_capacity(self.strings.len());
        for text in &self.strings {
            offsets.push(u32::try_from(data.len()).map_err(|_| "String pool data is too large")?);
            if self.utf8 {
                let utf8 = text.as_bytes();
                let utf16_len = text.encode_utf16().count();
                write_len8(&mut data, utf16_len)?;
                write_len8(&mut data, utf8.len())?;
                data.extend_from_slice(utf8);
                data.push(0);
            } else {
                let units: Vec<u16> = text.encode_utf16().collect();
                write_len16(&mut data, units.len())?;
                for unit in units {
                    data.extend_from_slice(&unit.to_le_bytes());
                }
                data.extend_from_slice(&0u16.to_le_bytes());
            }
        }
        let mut out = self.header.clone();
        for offset in offsets {
            put_u32(&mut out, offset);
        }
        for offset in &self.style_offsets {
            put_u32(&mut out, *offset);
        }
        out.extend_from_slice(&data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let styles_start = if self.style_offsets.is_empty() {
            0
        } else {
            u32::try_from(out.len()).map_err(|_| "String pool styles are too large")?
        };
        out.extend_from_slice(&self.styles);
        let size = u32::try_from(out.len()).map_err(|_| "String pool is too large")?;
        put_u32_at(&mut out, 4, size);
        put_u32_at(&mut out, 8, count);
        put_u32_at(&mut out, 12, style_count);
        put_u32_at(&mut out, 20, u32::try_from(strings_start).map_err(|_| "String pool header is too large")?);
        put_u32_at(&mut out, 24, styles_start);
        Ok(out)
    }
}

fn parse_pool(bytes: &[u8]) -> Result<StringPool> {
    if bytes.len() < 28 || u16_at(bytes, 0)? != 0x0001 {
        return Err("String pool is missing".into());
    }
    let size = u32_at(bytes, 4)? as usize;
    let header_size = u16_at(bytes, 2)? as usize;
    if header_size < 28 || size < header_size || size > bytes.len() {
        return Err("String pool size is outside the document".into());
    }
    let count = u32_at(bytes, 8)? as usize;
    let style_count = u32_at(bytes, 12)? as usize;
    let utf8 = u32_at(bytes, 16)? & 0x100 != 0;
    let start = u32_at(bytes, 20)? as usize;
    let offsets_end = header_size
        .checked_add(count.checked_mul(4).ok_or("String pool string count overflows")?)
        .and_then(|n| n.checked_add(style_count.checked_mul(4)?))
        .ok_or("String pool offsets overflow")?;
    if offsets_end > size || start < offsets_end || start > size {
        return Err("String pool offsets are outside the chunk".into());
    }
    let styles_start = u32_at(bytes, 24)? as usize;
    if style_count > 0 && (styles_start < start || styles_start > size) {
        return Err("String pool styles are outside the chunk".into());
    }
    let strings_end = if style_count > 0 { styles_start } else { size };
    let strings_bytes = bytes.get(start..strings_end).ok_or("String pool string data is truncated")?;
    let mut strings = Vec::with_capacity(count);
    for index in 0..count {
        let offset = u32_at(bytes, header_size + index * 4)? as usize;
        let mut at = offset;
        if at >= strings_bytes.len() {
            return Err("String pool entry offset is outside its data".into());
        }
        if utf8 {
            let utf16_len = read_len8(strings_bytes, &mut at)?;
            let byte_len = read_len8(strings_bytes, &mut at)?;
            let end = at.checked_add(byte_len).ok_or("UTF-8 string length overflows")?;
            let raw = strings_bytes.get(at..end).ok_or("UTF-8 string is truncated")?;
            if strings_bytes.get(end) != Some(&0) {
                return Err("UTF-8 string terminator is missing".into());
            }
            let text = String::from_utf8(raw.to_vec()).map_err(|_| "String resource is not UTF-8".to_string())?;
            if text.encode_utf16().count() != utf16_len {
                return Err("UTF-8 and UTF-16 string lengths disagree".into());
            }
            strings.push(text);
        } else {
            let units = read_len16(strings_bytes, &mut at)?;
            let byte_len = units.checked_mul(2).ok_or("UTF-16 string length overflows")?;
            let end = at.checked_add(byte_len).ok_or("UTF-16 string length overflows")?;
            let raw = strings_bytes.get(at..end).ok_or("UTF-16 string is truncated")?;
            if u16_at(strings_bytes, end)? != 0 {
                return Err("UTF-16 string terminator is missing".into());
            }
            let text = raw.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>();
            strings.push(String::from_utf16(&text).map_err(|_| "String resource is not Unicode".to_string())?);
        }
    }
    let mut style_offsets = Vec::with_capacity(style_count);
    for index in 0..style_count {
        style_offsets.push(u32_at(bytes, header_size + count * 4 + index * 4)?);
    }
    let styles = if style_count > 0 { bytes[styles_start..size].to_vec() } else { Vec::new() };
    let pool = StringPool { header: bytes[..header_size].to_vec(), strings, style_offsets, styles, size, utf8 };
    validate_styles(&pool)?;
    Ok(pool)
}

fn ensure_style_fits(pool: &StringPool, index: usize, replacement: &str) -> Result<()> {
    let new_len = replacement.encode_utf16().count() as u64;
    let Some(offset) = pool.style_offsets.get(index).copied() else {
        return Ok(());
    };
    if offset == u32::MAX {
        return Ok(());
    }
    let mut at = offset as usize;
    loop {
        let name = u32_at(&pool.styles, at)?;
        if name == u32::MAX {
            return Ok(());
        }
        let first = u32_at(&pool.styles, at + 4)? as u64;
        let last = u32_at(&pool.styles, at + 8)? as u64;
        if first > last || last >= new_len {
            return Err("Replacement text would invalidate a styled string span".into());
        }
        at += 12;
    }
}

fn validate_styles(pool: &StringPool) -> Result<()> {
    for index in 0..pool.style_offsets.len() {
        let offset = pool.style_offsets[index];
        if offset == u32::MAX {
            continue;
        }
        let mut at = offset as usize;
        loop {
            let name = u32_at(&pool.styles, at)?;
            if name == u32::MAX {
                break;
            }
            if name as usize >= pool.strings.len() {
                return Err("String style references a missing name".into());
            }
            let first = u32_at(&pool.styles, at + 4)?;
            let last = u32_at(&pool.styles, at + 8)?;
            if first > last || last as usize >= pool.strings[index].encode_utf16().count() {
                return Err("String style span is outside its text".into());
            }
            at += 12;
        }
    }
    Ok(())
}

fn read_len8(bytes: &[u8], at: &mut usize) -> Result<usize> {
    let first = *bytes.get(*at).ok_or("String length is missing")?;
    *at += 1;
    if first & 0x80 == 0 {
        Ok(first as usize)
    } else {
        let second = *bytes.get(*at).ok_or("String length is truncated")?;
        *at += 1;
        Ok((((first & 0x7f) as usize) << 8) | second as usize)
    }
}

fn read_len16(bytes: &[u8], at: &mut usize) -> Result<usize> {
    let first = u16_at(bytes, *at)?;
    *at += 2;
    if first & 0x8000 == 0 {
        Ok(first as usize)
    } else {
        let second = u16_at(bytes, *at)?;
        *at += 2;
        Ok((((first & 0x7fff) as usize) << 16) | second as usize)
    }
}

fn write_len8(out: &mut Vec<u8>, length: usize) -> Result<()> {
    if length <= 0x7f {
        out.push(length as u8);
    } else if length <= 0x7fff {
        out.push(((length >> 8) as u8) | 0x80);
        out.push(length as u8);
    } else {
        return Err("UTF-8 string is too long for a binary resource pool".into());
    }
    Ok(())
}

fn write_len16(out: &mut Vec<u8>, length: usize) -> Result<()> {
    if length <= 0x7fff {
        put_u16(out, length as u16);
    } else if length <= 0x7fff_ffff {
        put_u16(out, ((length >> 16) as u16) | 0x8000);
        put_u16(out, length as u16);
    } else {
        return Err("UTF-16 string is too long for a binary resource pool".into());
    }
    Ok(())
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    let s = bytes.get(at..at + 2).ok_or("Truncated resource")?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let s = bytes.get(at..at + 4).ok_or("Truncated resource")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn put_u16(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u16_at(out: &mut [u8], at: usize, value: u16) { out[at..at + 2].copy_from_slice(&value.to_le_bytes()); }
fn put_u32_at(out: &mut [u8], at: usize, value: u32) { out[at..at + 4].copy_from_slice(&value.to_le_bytes()); }


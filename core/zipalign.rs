//! Package archive alignment.
//!
//! Uncompressed members start on the requested boundary. Native libraries
//! start on a 4096-byte boundary. `resources.arsc` stays stored.

use super::*;

const PAGE: u32 = 4096;

pub(crate) struct Member {
    pub name: String,
    pub data: Vec<u8>,
}

pub(crate) fn read_members(apk: &[u8]) -> Result<Vec<Member>> {
    let plain = strip_signing_block(apk)?;
    let entries = match super::archive::unpack(&plain) {
        Ok(entries) => entries,
        Err(_) => read_central(&plain)?,
    };
    Ok(entries.into_iter().map(|(name, data)| Member { name, data }).collect())
}

pub(crate) fn align(apk: &[u8], alignment: u32) -> Result<Vec<u8>> {
    let members = read_members(apk)?;
    let pairs: Vec<(String, Vec<u8>)> = members
        .into_iter()
        .filter(|member| !signature_name(&member.name))
        .map(|m| (m.name, m.data))
        .collect();
    write_stored(&pairs, alignment)
}

pub(crate) fn write_stored(entries: &[(String, Vec<u8>)], alignment: u32) -> Result<Vec<u8>> {
    if entries.is_empty() {
        return Err("An APK must contain at least one file".into());
    }
    let alignment = if alignment == 0 { 4 } else { alignment };
    if !alignment.is_power_of_two() {
        return Err("Alignment must be a power of two".into());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut count = 0u16;
    for (name, data) in entries {
        if name.ends_with('/') {
            continue;
        }
        validate_entry_name(name)?;
        if !names.insert(name.as_str()) {
            return Err(format!("Archive contains a duplicate entry name: {name}"));
        }
        if count >= u16::MAX - 1 {
            return Err("Archive has too many entries for a non-Zip64 package".into());
        }
        if name == "resources.arsc" && data.len() > u32::MAX as usize {
            return Err("resources.arsc is too large".into());
        }
        if data.len() > u32::MAX as usize {
            return Err(format!("Archive entry is too large for a non-Zip64 package: {name}"));
        }
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u16::MAX as usize {
            return Err(format!("Entry name is too long: {name}"));
        }
        let need = if name.ends_with(".so") { alignment.max(PAGE) } else { alignment };
        if need as usize > u16::MAX as usize {
            return Err("Requested alignment exceeds the ZIP extra-field limit".into());
        }
        let base = local.len().checked_add(30).and_then(|n| n.checked_add(name_bytes.len())).ok_or("Archive offset overflow")?;
        let extra = alignment_extra(base, need)?;
        if local.len() > u32::MAX as usize || central.len() > u32::MAX as usize {
            return Err("Archive exceeds the non-Zip64 offset limit".into());
        }
        emit_entry(&mut local, &mut central, name, data, &extra);
        count += 1;
    }
    let cd_off = u32::try_from(local.len()).map_err(|_| "Archive exceeds the non-Zip64 offset limit")?;
    let cd_len = u32::try_from(central.len()).map_err(|_| "Archive exceeds the non-Zip64 central-directory limit")?;
    local.extend(central);
    put_u32(&mut local, 0x0605_4b50);
    put_u16(&mut local, 0);
    put_u16(&mut local, 0);
    put_u16(&mut local, count);
    put_u16(&mut local, count);
    put_u32(&mut local, cd_len);
    put_u32(&mut local, cd_off);
    put_u16(&mut local, 0);
    Ok(local)
}

fn emit_entry(local: &mut Vec<u8>, central: &mut Vec<u8>, name: &str, data: &[u8], extra: &[u8]) {
    let name_bytes = name.as_bytes();
    let crc = crc32(data);
    put_u32(local, 0x0403_4b50);
    put_u16(local, 20);
    put_u16(local, 0x0800);
    put_u16(local, 0);
    put_u16(local, 0);
    put_u16(local, 0x0021);
    put_u32(local, crc);
    put_u32(local, data.len() as u32);
    put_u32(local, data.len() as u32);
    put_u16(local, name_bytes.len() as u16);
    put_u16(local, extra.len() as u16);
    local.extend_from_slice(name_bytes);
    local.extend_from_slice(extra);
    local.extend_from_slice(data);
    let offset = (local.len() - (30 + name_bytes.len() + extra.len() + data.len())) as u32;
    put_u32(central, 0x0201_4b50);
    put_u16(central, 20);
    put_u16(central, 20);
    put_u16(central, 0x0800);
    put_u16(central, 0);
    put_u16(central, 0);
    put_u16(central, 0x0021);
    put_u32(central, crc);
    put_u32(central, data.len() as u32);
    put_u32(central, data.len() as u32);
    put_u16(central, name_bytes.len() as u16);
    put_u16(central, 0);
    put_u16(central, 0);
    put_u16(central, 0);
    put_u16(central, 0);
    put_u32(central, 0);
    put_u32(central, offset);
    central.extend_from_slice(name_bytes);
}

fn alignment_extra(base: usize, need: u32) -> Result<Vec<u8>> {
    let need = need as usize;
    let want = (need - (base % need)) % need;
    let len = if want == 0 {
        0
    } else if want >= 4 {
        want
    } else if want + need <= u16::MAX as usize {
        want + need
    } else {
        return Err("Alignment padding does not fit the local header".into());
    };
    if len > u16::MAX as usize {
        return Err("Alignment padding exceeds the local header limit".into());
    }
    let mut extra = vec![0u8; len];
    if len >= 4 {
        extra[0..2].copy_from_slice(&0xd935u16.to_le_bytes());
        extra[2..4].copy_from_slice(&((len - 4) as u16).to_le_bytes());
    }
    Ok(extra)
}

fn validate_entry_name(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') || name.contains('\0') || name.chars().any(char::is_control) {
        return Err("Archive entry name is not a safe relative path".into());
    }
    if name.split(|ch| ch == '/' || ch == '\\').any(|part| part == ".." || part == ".") {
        return Err("Archive entry name contains a relative traversal component".into());
    }
    Ok(())
}

fn signature_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "META-INF/MANIFEST.MF"
        || (upper.starts_with("META-INF/")
            && (upper.ends_with(".SF") || upper.ends_with(".RSA") || upper.ends_with(".DSA") || upper.ends_with(".EC")))
}

pub(crate) fn verify(apk: &[u8], alignment: u32) -> Result<String> {
    let alignment = if alignment == 0 { 4 } else { alignment };
    if !alignment.is_power_of_two() {
        return Err("Alignment must be a power of two".into());
    }
    let eocd = find_eocd(apk)?;
    let cd = u32_at(apk, eocd + 16)? as usize;
    let content_end = signing_block_start(apk, cd)?.unwrap_or(cd);
    let mut at = 0usize;
    let mut stored = 0usize;
    let mut checked = 0usize;
    while at + 30 <= content_end {
        if u32_at(apk, at)? != 0x0403_4b50 {
            break;
        }
        let method = u16_at(apk, at + 8)?;
        let comp = u32_at(apk, at + 18)? as usize;
        let name_len = u16_at(apk, at + 26)? as usize;
        let extra_len = u16_at(apk, at + 28)? as usize;
        let name = std::str::from_utf8(apk.get(at + 30..at + 30 + name_len).ok_or("Entry name is truncated")?).map_err(|_| "Entry name is not text".to_string())?;
        let data_at = at + 30 + name_len + extra_len;
        if name == "resources.arsc" && method != 0 {
            return Err("resources.arsc must be stored".into());
        }
        if method == 0 && !name.ends_with('/') {
            let need = if name.ends_with(".so") { alignment.max(PAGE) } else { alignment };
            if data_at % need as usize != 0 {
                return Err(format!("{name} starts at {data_at}, which is not a multiple of {need}"));
            }
            stored += 1;
        }
        checked += 1;
        at = data_at.checked_add(comp).ok_or("Entry extends past the archive")?;
        if at > content_end {
            return Err("Entry overlaps the central directory".into());
        }
    }
    Ok(format!("aligned entries {stored}; local headers {checked}; boundary {alignment}"))
}

pub(crate) fn strip_signing_block(apk: &[u8]) -> Result<Vec<u8>> {
    let eocd = find_eocd(apk)?;
    let cd = u32_at(apk, eocd + 16)? as usize;
    let Some(start) = signing_block_start(apk, cd)? else {
        return Ok(apk.to_vec());
    };
    let mut out = Vec::with_capacity(apk.len() - (cd - start));
    out.extend_from_slice(&apk[..start]);
    out.extend_from_slice(&apk[cd..eocd]);
    let mut foot = apk[eocd..].to_vec();
    foot[16..20].copy_from_slice(&(start as u32).to_le_bytes());
    out.extend_from_slice(&foot);
    Ok(out)
}

pub(crate) fn regions(apk: &[u8]) -> Result<(usize, usize, usize)> {
    let eocd = find_eocd(apk)?;
    let cd = u32_at(apk, eocd + 16)? as usize;
    let content_end = signing_block_start(apk, cd)?.unwrap_or(cd);
    Ok((content_end, cd, eocd))
}

fn signing_magic(apk: &[u8], cd: usize) -> bool {
    cd >= 24 && apk.get(cd - 16..cd) == Some(b"APK Sig Block 42")
}

fn signing_block_start(apk: &[u8], cd: usize) -> Result<Option<usize>> {
    if !signing_magic(apk, cd) {
        return Ok(None);
    }
    let size = usize::try_from(u64_at(apk, cd - 24)?).map_err(|_| "Signature block is too large")?;
    let start = cd.checked_sub(size).and_then(|n| n.checked_sub(8)).ok_or("Signature block is outside the archive")?;
    if u64_at(apk, start)? != size as u64 {
        return Err("Signature block sizes disagree".into());
    }
    Ok(Some(start))
}

fn read_central(apk: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let eocd = find_eocd(apk)?;
    let count = u16_at(apk, eocd + 10)? as usize;
    let mut cursor = u32_at(apk, eocd + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(apk, cursor)? != 0x0201_4b50 {
            return Err("Central directory entry is not intact".into());
        }
        let flags = u16_at(apk, cursor + 8)?;
        if flags & 1 != 0 {
            return Err("Encrypted ZIP entries are not supported for package editing".into());
        }
        let method = u16_at(apk, cursor + 10)?;
        let crc = u32_at(apk, cursor + 16)?;
        let comp_size = u32_at(apk, cursor + 20)? as usize;
        let size = u32_at(apk, cursor + 24)? as usize;
        let name_len = u16_at(apk, cursor + 28)? as usize;
        let extra_len = u16_at(apk, cursor + 30)? as usize;
        let comment_len = u16_at(apk, cursor + 32)? as usize;
        let local = u32_at(apk, cursor + 42)? as usize;
        let name = std::str::from_utf8(apk.get(cursor + 46..cursor + 46 + name_len).ok_or("Entry name is truncated")?).map_err(|_| "Entry name is not text".to_string())?.to_string();
        let local_flags = u16_at(apk, local + 6)?;
        let local_method = u16_at(apk, local + 8)?;
        if local_flags & 1 != 0 || local_method != method {
            return Err(format!("Entry {name} has inconsistent local and central headers"));
        }
        let local_name = u16_at(apk, local + 26)? as usize;
        let local_extra = u16_at(apk, local + 28)? as usize;
        let local_name_at = local.checked_add(30).ok_or("Local entry offset overflow")?;
        let local_name_end = local_name_at.checked_add(local_name).ok_or("Local entry name overflows")?;
        let local_extra_end = local_name_end.checked_add(local_extra).ok_or("Local entry header overflows")?;
        let central_name = apk.get(cursor + 46..cursor + 46 + name_len).ok_or("Entry name is truncated")?;
        if apk.get(local_name_at..local_name_end) != Some(central_name) {
            return Err(format!("Entry {name} has inconsistent local and central names"));
        }
        let data_end = local_extra_end.checked_add(comp_size).ok_or("Entry data length overflows")?;
        let compressed = apk.get(local_extra_end..data_end).ok_or("Entry bytes are truncated")?;
        let data = if method == 0 {
            compressed.to_vec()
        } else if method == 8 {
            super::compression::inflate(compressed, size)?
        } else {
            return Err(format!("Entry {name} uses unsupported compression method {method}"));
        };
        if crc32(&data) != crc {
            return Err(format!("Entry {name} failed its checksum"));
        }
        out.push((name, data));
        cursor = cursor + 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

pub(crate) fn find_eocd(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 22 {
        return Err("Archive footer is missing".into());
    }
    let start = bytes.len().saturating_sub(22 + 65535);
    let mut at = bytes.len() - 22;
    loop {
        if bytes.get(at..at + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]) {
            let comment = u16_at(bytes, at + 20)? as usize;
            if at + 22 + comment == bytes.len() {
                return Ok(at);
            }
        }
        if at == start {
            break;
        }
        at -= 1;
    }
    Err("Archive footer was not found".into())
}

pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    let s = bytes.get(at..at + 2).ok_or("Truncated archive")?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let s = bytes.get(at..at + 4).ok_or("Truncated archive")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u64_at(bytes: &[u8], at: usize) -> Result<u64> {
    let s = bytes.get(at..at + 8).ok_or("Truncated archive")?;
    Ok(u64::from_le_bytes(s.try_into().unwrap()))
}
fn put_u16(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }

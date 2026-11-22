//! Stored and raw-deflate package members.
//! The archive is not installed and the members are not executed.

const MEMBER_CAP: usize = 1024 * 1024;
const SCAN_CAP: usize = 256;
const NAME_CAP: usize = 64;

fn shown_name(name: &str) -> String {
    let mut out = String::new();
    let mut more = false;
    for (index, ch) in name.chars().enumerate() {
        if index == NAME_CAP {
            more = true;
            break;
        }
        out.push(ch);
    }
    if more {
        out.push_str("...");
    }
    out
}

pub fn zip_members(bytes: &[u8]) -> String {
    if bytes.len() < 22 || &bytes[..4] != b"PK\x03\x04" {
        return "Not a zip archive.\n".into();
    }
    let Some((directory, names, unread, skipped_library, skipped_dex)) = central_directory(bytes) else {
        return "The zip directory was not found.\n".into();
    };
    let mut out = String::new();
    for (index, name) in names.iter().enumerate() {
        out.push_str(&format!("entry {index} {}\n", shown_name(name)));
    }
    let mut found = false;
    for member in directory {
        if !wanted(&member.name) {
            continue;
        }
        found = true;
        match expand(bytes, &member) {
            Ok(body) => {
                out.push_str(&describe(&member.name, &body));
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Err(error) => {
                out.push_str(&format!("member {} was not extracted: {error}\n", shown_name(&member.name)));
            }
        }
    }
    if !found {
        out.push_str("No manifest, dex, resource, or library member was declared.\n");
    }
    if unread {
        out.push_str("later entries were not listed\n");
    }
    if skipped_library {
        out.push_str("later libraries were not listed\n");
    }
    if skipped_dex {
        out.push_str("later dex files were not listed\n");
    }
    out.push_str("The archive was not installed.\n");
    out
}

struct Member {
    name: String,
    method: u16,
    compressed: usize,
    size: usize,
    local: usize,
}

fn central_directory(bytes: &[u8]) -> Option<(Vec<Member>, Vec<String>, bool, bool, bool)> {
    let end = find_eocd(bytes)?;
    let count = u16(bytes, end + 10)? as usize;
    let offset = u32(bytes, end + 16)? as usize;
    if offset >= bytes.len() {
        return None;
    }
    let unread_cap = count > SCAN_CAP;
    let count = count.min(SCAN_CAP);
    let mut at = offset;
    let mut members: Vec<Member> = Vec::new();
    let mut names = Vec::new();
    let mut read = 0usize;
    let mut unread = unread_cap;
    let mut skipped_library = false;
    let mut skipped_dex = false;
    for _ in 0..count {
        let Some(sig_end) = at.checked_add(4) else {
            unread = true;
            break;
        };
        if bytes.get(at..sig_end) != Some(b"PK\x01\x02") {
            unread = true;
            break;
        }
        let Some(method) = u16(bytes, at + 10) else {
            unread = true;
            break;
        };
        let Some(compressed) = u32(bytes, at + 20) else {
            unread = true;
            break;
        };
        let Some(size) = u32(bytes, at + 24) else {
            unread = true;
            break;
        };
        let Some(name_len) = u16(bytes, at + 28) else {
            unread = true;
            break;
        };
        let Some(extra_len) = u16(bytes, at + 30) else {
            unread = true;
            break;
        };
        let Some(comment_len) = u16(bytes, at + 32) else {
            unread = true;
            break;
        };
        let Some(local) = u32(bytes, at + 42) else {
            unread = true;
            break;
        };
        let name_at = at + 46;
        let Some(name_end) = name_at.checked_add(name_len as usize) else {
            unread = true;
            break;
        };
        let Some(name_bytes) = bytes.get(name_at..name_end) else {
            unread = true;
            break;
        };
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        if names.len() < 32 {
            names.push(name.clone());
        } else {
            unread = true;
        }
        if later_dex(&name) {
            skipped_dex = true;
        }
        if wanted(&name) {
            if so_name(&name) && members.iter().filter(|member| so_name(&member.name)).count() >= 2 {
                skipped_library = true;
            } else {
                members.push(Member {
                    name,
                    method,
                    compressed: compressed as usize,
                    size: size as usize,
                    local: local as usize,
                });
            }
        }
        let Some(next) = name_end.checked_add(extra_len as usize).and_then(|value| value.checked_add(comment_len as usize)) else {
            unread = true;
            break;
        };
        at = next;
        read += 1;
    }
    if read < count {
        unread = true;
    }
    Some((members, names, unread, skipped_library, skipped_dex))
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    let start = bytes.len().saturating_sub(22 + 1024);
    let mut at = bytes.len().saturating_sub(22);
    loop {
        if bytes.get(at..at + 4) == Some(b"PK\x05\x06") {
            let comment = u16(bytes, at + 20)? as usize;
            if at + 22 + comment == bytes.len() {
                return Some(at);
            }
        }
        if at == start {
            return None;
        }
        at -= 1;
    }
}

fn wanted(name: &str) -> bool {
    name == "AndroidManifest.xml" || name == "resources.arsc" || dex_name(name) || so_name(name)
}

fn so_name(name: &str) -> bool {
    name.ends_with(".so") && name.contains("lib/")
}

fn dex_name(name: &str) -> bool {
    name == "classes.dex" || name == "classes2.dex" || name == "classes3.dex"
}

fn later_dex(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("classes") else {
        return false;
    };
    let Some(number) = rest.strip_suffix(".dex") else {
        return false;
    };
    number.parse::<u32>().ok().is_some_and(|value| value >= 4)
}

fn library_line(name: &str, body: &[u8]) -> String {
    if body.len() < 20 || &body[..4] != b"\x7fELF" {
        return format!("library {} is not an ELF image. The library was not loaded.\n", shown_name(name));
    }
    let class = match body[4] {
        1 => "32",
        2 => "64",
        _ => "other",
    };
    let machine = if body[5] == 1 {
        u16::from_le_bytes([body[18], body[19]])
    } else {
        u16::from_be_bytes([body[18], body[19]])
    };
    let mut text = format!("library {} class {class} machine {machine}. The library was not loaded.\n", shown_name(name));
    #[cfg(not(test))]
    {
        if body[4] == 2 && body[5] == 1 && machine == 183 {
            text.push_str(&crate::arm_branch::arm_branch(body));
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&crate::arm_text::arm_text(body));
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
        if let Some(sites) = crate::call_sites::elf_code_sites(body) {
            text.push_str(&sites);
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
        let window = &body[..body.len().min(1 << 20)];
        text.push_str(&crate::text_catalog::text_catalog(window, 6));
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&crate::string_near_call::string_near_call(window));
        if !text.ends_with('\n') {
            text.push('\n');
        }
        match crate::application::elf_observation::analyze_with_cancel(body, true, body.len() as u64, &|| false) {
            Ok(detail) => text.push_str(&detail),
            Err(error) => text.push_str(&error),
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    text
}

fn describe(name: &str, body: &[u8]) -> String {
    #[cfg(not(test))]
    {
        if name == "AndroidManifest.xml" {
            crate::manifest_pool::manifest_pool(body)
        } else if dex_name(name) {
            let mut text = crate::dex_strings::dex_strings(body);
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&crate::dex_use::dex_use(body));
            if let Some(detail) = crate::application::investigation::dex::analyze(body, true, body.len() as u64, &|| false) {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&detail);
            }
            if !text.ends_with('\n') {
                text.push('\n');
            }
            let window = &body[..body.len().min(1 << 20)];
            text.push_str(&crate::text_catalog::text_catalog(window, 6));
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&crate::string_near_call::string_near_call(window));
            text
        } else if so_name(name) {
            library_line(name, body)
        } else if body.len() >= 4 && u32::from_le_bytes([body[0], body[1], body[2], body[3]]) == 0x0008_0003 {
            crate::manifest_pool::manifest_pool(body)
        } else {
            format!("resource table bytes {}. The table was not decoded.\n", body.len())
        }
    }
    #[cfg(test)]
    {
        format!("member {} bytes {}\n", shown_name(name), body.len())
    }
}

fn expand(bytes: &[u8], member: &Member) -> Result<Vec<u8>, String> {
    if member.size > MEMBER_CAP || member.compressed > MEMBER_CAP {
        return Err("member exceeds the size cap".into());
    }
    if bytes.get(member.local..member.local + 4) != Some(b"PK\x03\x04") {
        return Err("local header is absent".into());
    }
    let name_len = u16(bytes, member.local + 26).ok_or("local header is short")? as usize;
    let extra_len = u16(bytes, member.local + 28).ok_or("local header is short")? as usize;
    let data_at = member.local + 30 + name_len + extra_len;
    let data = bytes.get(data_at..data_at + member.compressed).ok_or("member bytes are outside the archive")?;
    if member.method == 0 {
        if data.len() != member.size {
            return Err("stored size does not match".into());
        }
        return Ok(data.to_vec());
    }
    if member.method == 8 {
        return inflate_raw(data, member.size);
    }
    Err(format!("method {} is not extracted", member.method))
}

#[cfg(not(test))]
fn inflate_raw(data: &[u8], size: usize) -> Result<Vec<u8>, String> {
    crate::compression::inflate(data, size)
}

#[cfg(test)]
fn inflate_raw(_data: &[u8], _size: usize) -> Result<Vec<u8>, String> {
    Err("deflate is exercised by the crate build".into())
}

fn u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::{library_line, zip_members};

    fn stored_zip(name: &str, body: &[u8]) -> Vec<u8> {
        let name_bytes = name.as_bytes();
        let mut local = Vec::new();
        local.extend_from_slice(b"PK\x03\x04");
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u32.to_le_bytes());
        local.extend_from_slice(&(body.len() as u32).to_le_bytes());
        local.extend_from_slice(&(body.len() as u32).to_le_bytes());
        local.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name_bytes);
        local.extend_from_slice(body);
        let local_len = local.len();
        let mut central = Vec::new();
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(name_bytes);
        let mut end = Vec::new();
        end.extend_from_slice(b"PK\x05\x06");
        end.extend_from_slice(&0u16.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        end.extend_from_slice(&1u16.to_le_bytes());
        end.extend_from_slice(&1u16.to_le_bytes());
        end.extend_from_slice(&(central.len() as u32).to_le_bytes());
        end.extend_from_slice(&(local_len as u32).to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        let mut zip = local;
        zip.extend_from_slice(&central);
        zip.extend_from_slice(&end);
        zip
    }

    #[test]
    fn stored_manifest_is_read() {
        let mut body = vec![0x03, 0x00, 0x08, 0x00];
        body.extend_from_slice(&[0, 0, 0, 0]);
        let text = zip_members(&stored_zip("AndroidManifest.xml", &body));
        assert!(text.contains("entry 0 AndroidManifest.xml"), "{text}");
        assert!(text.contains("not installed"), "{text}");
    }

    #[test]
    fn other_names_are_skipped() {
        let text = zip_members(&stored_zip("readme.txt", b"hello"));
        assert!(text.contains("No manifest, dex, resource, or library member"), "{text}");
    }

    #[test]
    fn long_entry_name_is_marked() {
        let name = "A".repeat(65);
        let shown = "A".repeat(64);
        assert_eq!(
            zip_members(&stored_zip(&name, b"hello")),
            format!("entry 0 {shown}...\nNo manifest, dex, resource, or library member was declared.\nThe archive was not installed.\n")
        );
    }

    #[test]
    fn long_library_member_name_is_marked() {
        let name = format!("lib/{}.so", "A".repeat(58));
        let shown: String = name.chars().take(64).collect();
        assert_eq!(
            zip_members(&stored_zip(&name, b"hi")),
            format!("entry 0 {shown}...\nmember {shown}... bytes 2\nThe archive was not installed.\n")
        );
        assert_eq!(
            library_line(&name, b"hi"),
            format!("library {shown}... is not an ELF image. The library was not loaded.\n")
        );
        let mut elf = vec![0u8; 20];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        assert_eq!(
            library_line(&name, &elf),
            format!("library {shown}... class 64 machine 0. The library was not loaded.\n")
        );
    }

    #[test]
    fn short_central_directory_is_named() {
        let mut zip = stored_zip("readme.txt", b"hello");
        let eocd = zip.len() - 22;
        zip[eocd + 8..eocd + 10].copy_from_slice(&2u16.to_le_bytes());
        zip[eocd + 10..eocd + 12].copy_from_slice(&2u16.to_le_bytes());
        let text = zip_members(&zip);
        assert!(text.contains("entry 0 readme.txt\n"), "{text}");
        assert!(text.contains("later entries were not listed\nThe archive was not installed.\n"), "{text}");
        assert!(!text.contains("entry 1 "), "{text}");
    }

    #[test]
    fn third_library_is_named() {
        let zip = stored_many(&[
            ("lib/arm64-v8a/liba.so", b"aa"),
            ("lib/arm64-v8a/libb.so", b"bb"),
            ("lib/arm64-v8a/libc.so", b"cc"),
        ]);
        let text = zip_members(&zip);
        assert!(text.contains("member lib/arm64-v8a/liba.so bytes 2\n"), "{text}");
        assert!(text.contains("member lib/arm64-v8a/libb.so bytes 2\n"), "{text}");
        assert!(!text.contains("member lib/arm64-v8a/libc.so"), "{text}");
        assert!(text.contains("entry 2 lib/arm64-v8a/libc.so\n"), "{text}");
        assert!(text.contains("later libraries were not listed\nThe archive was not installed.\n"), "{text}");
    }

    #[test]
    fn later_dex_file_is_named() {
        let zip = stored_many(&[("classes.dex", b"dex\n"), ("classes4.dex", b"dex\n")]);
        let text = zip_members(&zip);
        assert!(text.contains("member classes.dex bytes 4\n"), "{text}");
        assert!(text.contains("entry 1 classes4.dex\n"), "{text}");
        assert!(!text.contains("member classes4.dex"), "{text}");
        assert!(text.contains("later dex files were not listed\nThe archive was not installed.\n"), "{text}");
    }

    fn stored_many(items: &[(&str, &[u8])]) -> Vec<u8> {
        let mut local = Vec::new();
        let mut central = Vec::new();
        for (name, body) in items {
            let local_at = local.len() as u32;
            let name_bytes = name.as_bytes();
            local.extend_from_slice(b"PK\x03\x04");
            local.extend_from_slice(&20u16.to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(&0u32.to_le_bytes());
            local.extend_from_slice(&(body.len() as u32).to_le_bytes());
            local.extend_from_slice(&(body.len() as u32).to_le_bytes());
            local.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(name_bytes);
            local.extend_from_slice(body);
            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&local_at.to_le_bytes());
            central.extend_from_slice(name_bytes);
        }
        let mut end = Vec::new();
        end.extend_from_slice(b"PK\x05\x06");
        end.extend_from_slice(&0u16.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        end.extend_from_slice(&(items.len() as u16).to_le_bytes());
        end.extend_from_slice(&(items.len() as u16).to_le_bytes());
        end.extend_from_slice(&(central.len() as u32).to_le_bytes());
        end.extend_from_slice(&(local.len() as u32).to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&central);
        local.extend_from_slice(&end);
        local
    }

    #[test]
    fn extra_dex_and_resource_table_are_read() {
        let dex = zip_members(&stored_zip("classes2.dex", b"dex\n"));
        assert!(dex.contains("member classes2.dex bytes 4"), "{dex}");
        let table = zip_members(&stored_zip("resources.arsc", b"table"));
        assert!(table.contains("member resources.arsc bytes 5"), "{table}");
        let mut elf = vec![0x7f, b'E', b'L', b'F', 2, 1];
        elf.resize(20, 0);
        elf[18] = 0xb7;
        elf[19] = 0x00;
        let library = zip_members(&stored_zip("lib/arm64-v8a/liba.so", &elf));
        assert!(library.contains("member lib/arm64-v8a/liba.so bytes 20"), "{library}");
    }
}

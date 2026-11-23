use super::*;
use std::{
    fs,
    path::Path,
};
fn short(out: &mut Vec<u8>, n: u16) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn long(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn crc_update(mut crc:u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    crc
}
fn crc32(bytes:&[u8])->u32 { !crc_update(!0,bytes) }
const DOS_DATE: u16 = 0x0021; // 1980-01-01, the first representable ZIP date.
/// Saved-capture readers keep this historical bound. Project archives use `project_budget`.
pub(crate) const UNPACK_BYTE_LIMIT: usize = 5 * 1024 * 1024;
const STREAM_WINDOW: usize = 64 * 1024;
const HISTORICAL_ENTRIES: u64 = 500;
thread_local! {
    static BUDGET_OVERRIDE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
fn query_installed_ram() -> u64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Memory { length: u32, load: u32, total: u64, avail: u64, page: u64, page_avail: u64, virt: u64, virt_avail: u64, ext: u64 }
        #[link(name = "kernel32")]
        unsafe extern "system" { fn GlobalMemoryStatusEx(memory: *mut Memory) -> i32; }
        let mut memory = Memory { length: std::mem::size_of::<Memory>() as u32, load: 0, total: 0, avail: 0, page: 0, page_avail: 0, virt: 0, virt_avail: 0, ext: 0 };
        if unsafe { GlobalMemoryStatusEx(&mut memory) } != 0 && memory.total > 0 { return memory.total; }
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        if let Ok(text) = fs::read_to_string("/proc/meminfo") {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Ok(kb) = rest.split_whitespace().next().unwrap_or("0").parse::<u64>() {
                        if kb > 0 { return kb.saturating_mul(1024); }
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" { fn sysconf(name: i32) -> i64; }
        let page = unsafe { sysconf(29) };
        let pages = unsafe { sysconf(200) };
        if page > 0 && pages > 0 { return (page as u64).saturating_mul(pages as u64); }
    }
    32 << 30
}
fn installed_ram() -> u64 {
    static CACHED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *CACHED.get_or_init(query_installed_ram)
}
/// Half of installed memory (R16). A caller may set a lower ceiling for this thread.
/// A missing override uses the measured memory; it never blocks the operation.
pub(crate) fn configure_project_budget(bytes: u64) {
    BUDGET_OVERRIDE.with(|slot| slot.set(bytes));
}
pub(crate) fn project_budget() -> u64 {
    let half = installed_ram() / 2;
    let ceiling = if half == 0 { 16 << 30 } else { half };
    let chosen = BUDGET_OVERRIDE.with(|slot| slot.get());
    if chosen == 0 { ceiling } else { chosen.min(ceiling) }
}
fn file_limit() -> u64 { project_budget() }
fn data_limit() -> u64 { project_budget() }
fn zip_limit() -> u64 { project_budget() }
fn snapshot_limit() -> u64 { project_budget().min(256 * 1024 * 1024) }
fn entry_limit() -> u64 { (project_budget() / 1024).clamp(HISTORICAL_ENTRIES, 1_048_576) }
fn admits_file(len: u64) -> bool { len <= file_limit() && usize::try_from(len).is_ok() }
fn resident_len(len: u64) -> Result<usize> {
    if len > project_budget() { return Err("Project archive exceeds the memory budget".into()); }
    usize::try_from(len).map_err(|_| "Project archive exceeds addressable memory".into())
}
/// `0xFFFFFFFF` is the ZIP64 sentinel, so that exact size is not a 32-bit length.
fn zip64_size(size: u64) -> bool { size >= u32::MAX as u64 }
/// `0xFFFFFFFF` is the ZIP64 sentinel, so that exact offset is not a 32-bit offset.
fn zip64_offset(offset: u64) -> bool { offset >= u32::MAX as u64 }
/// `0xFFFF` and `0xFFFFFFFF` are footer sentinels, including those exact values.
fn zip64_footer(count: u64, size: u64, offset: u64) -> bool {
    count >= u16::MAX as u64 || size >= u32::MAX as u64 || offset >= u32::MAX as u64
}
/// ZIP bytes, the side records, the still-live entry inventory, and the scratch window.
fn admit_peak(zip_len: u64, side: u64, inventory: u64) -> Result<()> {
    let peak = zip_len.checked_add(side)
        .and_then(|n| n.checked_add(inventory))
        .and_then(|n| n.checked_add(STREAM_WINDOW as u64))
        .ok_or("Archive size overflow")?;
    if peak > project_budget() { Err("Project archive exceeds the memory budget".into()) } else { Ok(()) }
}
/// Every buffer that is live at the same time counts once. A partial sum is not a budget.
fn admit_sum(parts: &[u64]) -> Result<()> {
    let mut total = 0u64;
    for part in parts {
        total = total.checked_add(*part).ok_or("Archive size overflow")?;
    }
    if total > project_budget() { Err("Project exceeds the memory budget".into()) } else { Ok(()) }
}
fn charge(resident: &mut u64, extra: u64) -> Result<()> {
    *resident = resident.checked_add(extra).ok_or("Archive size overflow")?;
    if *resident > project_budget() { Err("Project exceeds the memory budget".into()) } else { Ok(()) }
}
fn base64_len(len: usize) -> Result<u64> {
    let n = len as u64;
    n.checked_add(2).map(|n| n / 3).and_then(|n| n.checked_mul(4)).ok_or("Archive size overflow".into())
}
/// One exact-capacity encoding. The ZIP buffer is the only other live copy.
fn encode_base64(bytes: &[u8]) -> Result<String> {
    let len = usize::try_from(base64_len(bytes.len())?).map_err(|_| "Archive size overflow")?;
    let mut out = String::new();
    out.try_reserve_exact(len).map_err(|_| "Project exceeds the memory budget")?;
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | (chunk.get(2).copied().unwrap_or(0) as u32);
        for i in 0..4 {
            if i > chunk.len() {
                out.push('=');
            } else {
                out.push(table[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
        }
    }
    if out.len() != len { return Err("Archive encoding length mismatch".into()); }
    Ok(out)
}
fn prefix_limit() -> u64 { entry_limit().saturating_mul(64) }
fn stop_unless_running() -> Result<()> {
    if crate::execution::cancelled() {
        Err("Archive operation cancelled before completion".into())
    } else {
        Ok(())
    }
}
fn vec_string_bytes(values: &Vec<String>) -> u64 {
    (values.capacity() * std::mem::size_of::<String>()) as u64
        + values.iter().map(|value| value.capacity() as u64).sum::<u64>()
}
fn tree_string_bytes(values: &std::collections::BTreeSet<String>) -> u64 {
    values.iter().map(|value| value.capacity() as u64 + 96).sum()
}
#[cfg(any(test, rllm_module_tests))]
thread_local! {
    static ENTRY_HANDLES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static ENTRY_HANDLE_PEAK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
#[cfg(any(test, rllm_module_tests))]
fn reset_entry_handles() {
    ENTRY_HANDLES.with(|slot| slot.set(0));
    ENTRY_HANDLE_PEAK.with(|slot| slot.set(0));
}
#[cfg(any(test, rllm_module_tests))]
fn enter_entry_handle() {
    ENTRY_HANDLES.with(|slot| {
        let open = slot.get() + 1;
        slot.set(open);
        ENTRY_HANDLE_PEAK.with(|peak| peak.set(peak.get().max(open)));
    });
}
#[cfg(any(test, rllm_module_tests))]
fn leave_entry_handle() {
    ENTRY_HANDLES.with(|slot| slot.set(slot.get().saturating_sub(1)));
}
#[cfg(any(test, rllm_module_tests))]
fn entry_handle_peak() -> u64 { ENTRY_HANDLE_PEAK.with(|slot| slot.get()) }
#[cfg(any(test, rllm_module_tests))]
fn entry_handles_open() -> u64 { ENTRY_HANDLES.with(|slot| slot.get()) }
#[cfg(not(any(test, rllm_module_tests)))]
fn reset_entry_handles() {}
#[cfg(not(any(test, rllm_module_tests)))]
fn enter_entry_handle() {}
#[cfg(not(any(test, rllm_module_tests)))]
fn leave_entry_handle() {}
/// One project entry at a time. The walk does not retain a handle per node.
struct EntryFile(fs::File);
impl EntryFile {
    fn new(file: fs::File) -> Self { enter_entry_handle(); Self(file) }
}
impl Drop for EntryFile { fn drop(&mut self) { leave_entry_handle(); } }
impl std::ops::Deref for EntryFile {
    type Target = fs::File;
    fn deref(&self) -> &fs::File { &self.0 }
}
impl std::ops::DerefMut for EntryFile {
    fn deref_mut(&mut self) -> &mut fs::File { &mut self.0 }
}
struct LocalHeader { offset: u64, crc_at: usize }
fn local_header(out: &mut Vec<u8>, name: &str, size: u64) -> Result<LocalHeader> {
    let name_bytes = name.as_bytes();
    let name_length = u16::try_from(name_bytes.len()).map_err(|_| "Filename too long")?;
    let zip64 = zip64_size(size);
    let offset = out.len() as u64;
    let size32 = if zip64 { u32::MAX } else { size as u32 };
    long(out, 0x04034b50);
    short(out, if zip64 { 45 } else { 20 });
    short(out, 0x0800);
    short(out, 0);
    short(out, 0);
    short(out, DOS_DATE);
    let crc_at = out.len();
    long(out, 0);
    long(out, size32);
    long(out, size32);
    short(out, name_length);
    short(out, if zip64 { 20 } else { 0 });
    out.extend_from_slice(name_bytes);
    if zip64 {
        short(out, 1);
        short(out, 16);
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }
    Ok(LocalHeader { offset, crc_at })
}
fn patch_crc(out: &mut [u8], at: usize, crc: u32) {
    out[at..at + 4].copy_from_slice(&crc.to_le_bytes());
}
fn central_extra_len(size: u64, offset: u64) -> u64 {
    let sizes = u64::from(zip64_size(size));
    let off = u64::from(zip64_offset(offset));
    if sizes == 0 && off == 0 { 0 } else { 4 + 8 * (sizes * 2 + off) }
}
fn central_header(directory: &mut Vec<u8>, name: &str, crc: u32, size: u64, offset: u64, is_dir: bool) -> Result<()> {
    let name_bytes = name.as_bytes();
    let name_length = u16::try_from(name_bytes.len()).map_err(|_| "Filename too long")?;
    let wide_size = zip64_size(size);
    let wide_offset = zip64_offset(offset);
    let mut extra = Vec::new();
    if wide_size || wide_offset {
        let mut payload = Vec::new();
        if wide_size {
            payload.extend_from_slice(&size.to_le_bytes());
            payload.extend_from_slice(&size.to_le_bytes());
        }
        if wide_offset { payload.extend_from_slice(&offset.to_le_bytes()); }
        short(&mut extra, 1);
        short(&mut extra, u16::try_from(payload.len()).map_err(|_| "ZIP64 metadata is too large")?);
        extra.extend_from_slice(&payload);
    }
    let zip64 = wide_size || wide_offset;
    long(directory, 0x02014b50);
    short(directory, if zip64 { 45 } else { 20 });
    short(directory, if zip64 { 45 } else { 20 });
    short(directory, 0x0800);
    short(directory, 0);
    short(directory, 0);
    short(directory, DOS_DATE);
    long(directory, crc);
    long(directory, if wide_size { u32::MAX } else { size as u32 });
    long(directory, if wide_size { u32::MAX } else { size as u32 });
    short(directory, name_length);
    short(directory, extra.len() as u16);
    short(directory, 0);
    short(directory, 0);
    short(directory, 0);
    long(directory, if is_dir { 0x10 } else { 0 });
    long(directory, if wide_offset { u32::MAX } else { offset as u32 });
    directory.extend_from_slice(name_bytes);
    directory.extend_from_slice(&extra);
    Ok(())
}
fn finish_archive(out: &mut Vec<u8>, directory_at: u64, directory_len: u64, count: u64) -> Result<()> {
    if out.len() as u64 != directory_at.saturating_add(directory_len) {
        return Err("Archive length mismatch".into());
    }
    let offset = directory_at;
    let size = directory_len;
    let zip64 = zip64_footer(count, size, offset);
    if zip64 {
        long(out, 0x06064b50);
        out.extend_from_slice(&44u64.to_le_bytes());
        short(out, 45);
        short(out, 45);
        long(out, 0);
        long(out, 0);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        let record = offset.saturating_add(size);
        long(out, 0x07064b50);
        long(out, 0);
        out.extend_from_slice(&record.to_le_bytes());
        long(out, 1);
    }
    long(out, 0x06054b50);
    short(out, 0);
    short(out, 0);
    short(out, if zip64 { u16::MAX } else { count as u16 });
    short(out, if zip64 { u16::MAX } else { count as u16 });
    long(out, if zip64 { u32::MAX } else { size as u32 });
    long(out, if zip64 { u32::MAX } else { offset as u32 });
    short(out, 0);
    Ok(())
}
// Directory enumeration uses an independently opened descriptor.
#[cfg(any(target_os="linux",target_os="android",target_os="macos"))]
mod source {
    use super::*;
    use std::ffi::{CString, OsStr, OsString};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    #[cfg(target_os="macos")]
    const ABI:(i32,i32,i32,i32,i32,i32,i32) = (-2,0x100000,0x100,0x1000000,4,0,0x200|0x800);
    // AArch64 and 32-bit ARM use O_DIRECTORY 0x4000 and O_NOFOLLOW 0x8000.
    // x86 and x86_64 keep the generic values 0x10000 and 0x20000. PowerPC matches ARM.
    #[cfg(all(not(target_os="macos"), any(target_arch="arm",target_arch="aarch64",target_arch="powerpc",target_arch="powerpc64")))]
    const ABI:(i32,i32,i32,i32,i32,i32,i32) = (-100,0x4000,0x8000,0x80000,0x800,0x200000,0x40|0x80);
    #[cfg(all(not(target_os="macos"), not(any(target_arch="arm",target_arch="aarch64",target_arch="powerpc",target_arch="powerpc64"))))]
    const ABI:(i32,i32,i32,i32,i32,i32,i32) = (-100,0x10000,0x20000,0x80000,0x800,0x200000,0x40|0x80);
    const AT_CWD:i32=ABI.0;
    const DIRECTORY:i32=ABI.1;
    const NOFOLLOW:i32=ABI.2;
    #[cfg(all(not(target_os="macos"), target_arch="aarch64"))]
    const _: () = assert!(DIRECTORY == 0x4000 && NOFOLLOW == 0x8000);
    #[cfg(all(not(target_os="macos"), target_arch="arm"))]
    const _: () = assert!(DIRECTORY == 0x4000 && NOFOLLOW == 0x8000);
    #[cfg(all(not(target_os="macos"), any(target_arch="x86_64", target_arch="x86")))]
    const _: () = assert!(DIRECTORY == 0x10000 && NOFOLLOW == 0x20000);
    const CLOEXEC:i32=ABI.3;
    const NONBLOCK:i32=ABI.4;
    const PATH_ONLY:i32=ABI.5;
    const CREATE_EXCLUSIVE:i32=ABI.6;
    unsafe extern "C" {
        fn openat(fd:i32,path:*const std::ffi::c_char,flags:i32,...)->i32;
        fn fdopendir(fd:i32)->*mut std::ffi::c_void;
        #[cfg_attr(all(target_os="macos",target_arch="x86_64"), link_name="readdir$INODE64")]
        #[cfg_attr(all(not(target_os="macos"), target_pointer_width="32"), link_name="readdir64")]
        fn readdir(dir:*mut std::ffi::c_void)->*mut u8;
        fn closedir(dir:*mut std::ffi::c_void)->i32;
        fn close(fd:i32)->i32;
        fn mkdirat(fd:i32,path:*const std::ffi::c_char,mode:u32)->i32;
        fn unlinkat(fd:i32,path:*const std::ffi::c_char,flags:i32)->i32;
        #[cfg(target_os="macos")] fn fcntl(fd:i32,cmd:i32,...)->i32;
        #[cfg(target_os="linux")] fn __errno_location()->*mut i32;
        #[cfg(target_os="android")] fn __errno()->*mut i32;
        #[cfg(target_os="macos")] fn __error()->*mut i32;
    }
    fn error_slot()->*mut i32 {
        #[cfg(target_os="linux")] {unsafe {__errno_location()}}
        #[cfg(target_os="android")] {unsafe {__errno()}}
        #[cfg(target_os="macos")] {unsafe {__error()}}
    }
    fn open(fd:i32,name:&OsStr,flags:i32)->Result<fs::File> {
        let name=CString::new(name.as_bytes()).map_err(|_|"Invalid project path")?;
        let raw=unsafe {openat(fd,name.as_ptr(),flags|NOFOLLOW|CLOEXEC)};
        if raw<0 {return Err(std::io::Error::last_os_error().to_string());}
        Ok(unsafe {fs::File::from_raw_fd(raw)})
    }
    fn stable(a:&fs::Metadata,b:&fs::Metadata)->bool {
        a.dev()==b.dev() && a.ino()==b.ino() && a.mode()==b.mode() &&
        a.len()==b.len() && a.mtime()==b.mtime() && a.mtime_nsec()==b.mtime_nsec() &&
        a.ctime()==b.ctime() && a.ctime_nsec()==b.ctime_nsec()
    }
    fn same_object(a:&fs::Metadata,b:&fs::Metadata)->bool {
        a.dev()==b.dev() && a.ino()==b.ino() && a.is_dir()==b.is_dir()
    }
    struct Directory(*mut std::ffi::c_void);
    impl Drop for Directory {fn drop(&mut self) {unsafe {closedir(self.0);}}}
    fn names(file:&fs::File)->Result<Vec<String>> {
        let raw=open(file.as_raw_fd(),OsStr::new("."),DIRECTORY)?.into_raw_fd();
        let dir=unsafe {fdopendir(raw)};
        if dir.is_null() {let e=std::io::Error::last_os_error().to_string();unsafe {close(raw);};return Err(e);}
        let dir=Directory(dir);let mut names=Vec::new();
        loop {
            unsafe {*error_slot()=0;}
            let p=unsafe {readdir(dir.0)};
            if p.is_null() {
                if unsafe {*error_slot()}!=0 {return Err(std::io::Error::last_os_error().to_string());}
                break;
            }
            // Read only the returned record. Darwin adds a 16-bit name length
            // before the type byte, and both ABIs use 64-bit directory offsets.
            let length=unsafe {std::ptr::read_unaligned(p.add(16).cast::<u16>())} as usize;
            let name_offset=if cfg!(target_os="macos"){21}else{19};
            let capacity=if cfg!(target_os="macos"){1024}else{256};
            let bound=length.checked_sub(name_offset).ok_or("Invalid directory record")?.min(capacity);
            let name_bytes=unsafe {std::slice::from_raw_parts(p.add(name_offset),bound)};
            let end=name_bytes.iter().position(|&x|x==0).ok_or("Unterminated directory name")?;
            let bytes=&name_bytes[..end];if bytes==b"."||bytes==b".." {continue;}
            if names.len() as u64 >= super::entry_limit() {return Err(format!("Project folder exceeds {} entries", super::entry_limit()));}
            let name=std::str::from_utf8(bytes).map_err(|_|"Project filename is not UTF-8")?;
            valid_name(bytes)?;names.push(name.to_string());
        }
        names.sort();
        if names.windows(2).any(|n|n[0]==n[1]) {return Err("Project directory changed during enumeration".into());}
        Ok(names)
    }
    struct Anchor {file:fs::File,meta:fs::Metadata,name:OsString}
    struct Node {meta:fs::Metadata,parent:usize,name:String,path:String,children:Option<Vec<String>>}
    fn reopen(root:&fs::File,nodes:&[Node],index:usize)->Result<fs::File> {
        if index==0 {return root.try_clone().map_err(|e|e.to_string());}
        let mut chain=Vec::new();let mut cursor=index;
        while cursor!=0 {
            chain.push(cursor);cursor=nodes[cursor].parent;
            if chain.len()>64 {return Err("Project path exceeds 64 components".into());}
        }
        let mut current=root.try_clone().map_err(|e|e.to_string())?;
        for (step,&at) in chain.iter().rev().enumerate() {
            let flags=if step+1==chain.len(){NONBLOCK}else{DIRECTORY|PATH_ONLY};
            let next=open(current.as_raw_fd(),OsStr::new(&nodes[at].name),flags)?;
            let meta=next.metadata().map_err(|e|e.to_string())?;
            if !stable(&nodes[at].meta,&meta) {return Err("Project entry was replaced during archive collection".into());}
            current=next;
        }
        Ok(current)
    }
    fn inventory(nodes:&Vec<Node>,anchors:&Vec<Anchor>,order:&Vec<usize>,folded:&std::collections::BTreeSet<String>)->u64 {
        let mut total=(nodes.capacity()*std::mem::size_of::<Node>()) as u64;
        total+=(anchors.capacity()*std::mem::size_of::<Anchor>()) as u64;
        total+=(order.capacity()*std::mem::size_of::<usize>()) as u64;
        for node in nodes {
            total+=node.name.capacity() as u64+node.path.capacity() as u64;
            if let Some(children)=&node.children {total+=super::vec_string_bytes(children);}
        }
        for anchor in anchors {total+=anchor.name.capacity() as u64;}
        total+super::tree_string_bytes(folded)
    }
    pub(super) fn archive(root:&Path)->Result<(Vec<u8>,usize,usize)> {
        // Resolve deliberate Android/root aliases once, then walk only no-follow
        // descriptors. Re-resolution at the end checks identity, never reads data.
        // Entry descriptors stay on the active path; the node list keeps metadata.
        let resolved=root.canonicalize().map_err(|e|e.to_string())?;
        let first=open(AT_CWD,OsStr::new("/"),DIRECTORY|PATH_ONLY)?;
        let first_meta=first.metadata().map_err(|e|e.to_string())?;
        let mut anchors=vec![Anchor{file:first,meta:first_meta,name:OsString::new()}];
        for part in resolved.components() {
            if let std::path::Component::Normal(name)=part {
                if anchors.len()>=128 {return Err("Project root exceeds 128 ancestor components".into());}
                let file=open(anchors.last().unwrap().file.as_raw_fd(),name,DIRECTORY|PATH_ONLY)?;
                let meta=file.metadata().map_err(|e|e.to_string())?;
                anchors.push(Anchor{file,meta,name:name.to_os_string()});
            }
        }
        let root_file=anchors.last().unwrap().file.try_clone().map_err(|e|e.to_string())?;
        let meta=root_file.metadata().map_err(|e|e.to_string())?;
        super::reset_entry_handles();
        let mut nodes=vec![Node{meta,parent:0,name:String::new(),path:String::new(),children:None}];
        let mut pending=vec![0usize];let mut total=0u64;
        let mut folded=std::collections::BTreeSet::new();
        #[cfg(any(test, rllm_module_tests))] super::source_hook("root_opened");
        super::stop_unless_running()?;
        while let Some(index)=pending.pop() {
            let parent_path=nodes[index].path.clone();
            let dir=super::EntryFile::new(if index==0 {root_file.try_clone().map_err(|e|e.to_string())?} else {reopen(&root_file,&nodes,index)?});
            let children=names(&dir)?;
            #[cfg(any(test, rllm_module_tests))] super::source_hook("enumerated");
            if !stable(&nodes[index].meta,&dir.metadata().map_err(|e|e.to_string())?) {return Err("Project directory changed during archive collection".into());}
            for name in children.iter().cloned() {
                if nodes.len() as u64 > super::entry_limit() {return Err(format!("Project archive exceeds {} entries", super::entry_limit()));}
                let child=super::EntryFile::new(open(dir.as_raw_fd(),OsStr::new(&name),NONBLOCK)?);
                let meta=child.metadata().map_err(|e|e.to_string())?;
                let path=if parent_path.is_empty(){name.clone()}else{format!("{parent_path}/{name}")};
                valid_name(path.as_bytes())?;
                if !folded.insert(path.to_lowercase()) {return Err("Ambiguous project path casing".into());}
                if meta.is_dir() {
                    pending.push(nodes.len());
                } else if meta.is_file() {
                    if !super::admits_file(meta.len()) {return Err("Project file exceeds the memory budget".into());}
                    total=total.checked_add(meta.len()).ok_or("Project archive size overflow")?;
                    if total>super::data_limit() {return Err("Project archive exceeds the memory budget".into());}
                } else {return Err("Project contains an unsupported file type".into());}
                drop(child);
                nodes.push(Node{meta,parent:index,name,path,children:None});
            }
            nodes[index].children=Some(children);
        }
        drop(pending);
        // Metadata stays live. Handles do not: each payload is re-opened from the
        // pinned root, checked, copied through one scratch window, then closed.
        let mut order:Vec<usize>=(1..nodes.len()).collect();
        order.sort_by(|&a,&b| {
            let name=|i:usize| if nodes[i].meta.is_dir(){format!("{}/",nodes[i].path)}else{nodes[i].path.clone()};
            name(a).cmp(&name(b))
        });
        if order.is_empty(){return Err("No project entries to export".into());}
        let mut local_end=0u64;let mut central=0u64;
        for &i in &order {
            let name_len=nodes[i].path.len()+usize::from(nodes[i].meta.is_dir());
            let size=if nodes[i].meta.is_file(){nodes[i].meta.len()}else{0};
            let local_extra=if super::zip64_size(size) {20u64}else{0};
            central=central.checked_add(46+name_len as u64+super::central_extra_len(size,local_end)).ok_or("Archive size overflow")?;
            local_end=local_end.checked_add(30+name_len as u64+local_extra+size).ok_or("Archive size overflow")?;
        }
        let footer=super::zip64_footer(order.len() as u64,central,local_end);
        let length=local_end.checked_add(central).and_then(|n|n.checked_add(22+if footer {76}else{0})).ok_or("Archive size overflow")?;
        let side=(order.len()*(std::mem::size_of::<u32>()+std::mem::size_of::<u64>())) as u64;
        super::admit_peak(length,side,inventory(&nodes,&anchors,&order,&folded))?;
        let mut crcs=Vec::new();crcs.try_reserve_exact(order.len()).map_err(|_| "Cannot allocate project archive")?;
        let mut offsets=Vec::new();offsets.try_reserve_exact(order.len()).map_err(|_| "Cannot allocate project archive")?;
        let reserved=super::resident_len(length)?;
        let mut out=Vec::new();out.try_reserve_exact(reserved).map_err(|_| "Cannot allocate project archive")?;
        let mut files=0usize;let mut directories=0usize;
        let mut scratch=[0u8;super::STREAM_WINDOW];
        for &i in &order {
            let is_dir=nodes[i].meta.is_dir();
            let name=if is_dir{directories+=1;format!("{}/",nodes[i].path)}else{files+=1;nodes[i].path.clone()};
            let size=if is_dir{0}else{nodes[i].meta.len()};
            let header=super::local_header(&mut out,&name,size)?;
            let mut crc=!0u32;
            if !is_dir {
                let mut reader=super::EntryFile::new(reopen(&root_file,&nodes,i)?);
                super::stop_unless_running()?;
                #[cfg(any(test, rllm_module_tests))] super::source_hook("file_opened");
                let mut remaining=usize::try_from(size).map_err(|_| "Project file exceeds addressable memory")?;
                while remaining>0 {
                    super::stop_unless_running()?;
                    let count=remaining.min(scratch.len());
                    reader.read_exact(&mut scratch[..count]).map_err(|e|e.to_string())?;
                    crc=crc_update(crc,&scratch[..count]);out.extend_from_slice(&scratch[..count]);remaining-=count;
                    #[cfg(any(test, rllm_module_tests))] super::source_hook("file_chunk");
                }
                let mut extra=[0u8;1];
                if reader.read(&mut extra).map_err(|e|e.to_string())?!=0 || !stable(&nodes[i].meta,&reader.metadata().map_err(|e|e.to_string())?) {
                    return Err("Project file changed during archive read".into());
                }
            }
            let crc=!crc;
            super::patch_crc(&mut out, header.crc_at, crc);
            crcs.push(crc);offsets.push(header.offset);
        }
        let directory_at=out.len() as u64;
        for (slot,&i) in order.iter().enumerate() {
            let is_dir=nodes[i].meta.is_dir();
            let name=if is_dir{format!("{}/",nodes[i].path)}else{nodes[i].path.clone()};
            let size=if is_dir{0}else{nodes[i].meta.len()};
            super::central_header(&mut out,&name,crcs[slot],size,offsets[slot],is_dir)?;
        }
        let directory_len=out.len() as u64-directory_at;
        super::finish_archive(&mut out,directory_at,directory_len,order.len() as u64)?;
        if out.len() as u64 != length {return Err("Archive length mismatch".into());}
        #[cfg(any(test, rllm_module_tests))] super::source_hook("before_validation");
        // Re-open every pinned entry from the held root. A rename cannot redirect
        // the no-follow walk, and a replaced binding cannot pass the identity check.
        for i in 0..nodes.len() {
            let current=super::EntryFile::new(reopen(&root_file,&nodes,i)?);
            if !stable(&nodes[i].meta,&current.metadata().map_err(|e|e.to_string())?) {return Err("Project changed during archive collection".into());}
            if let Some(expected)=&nodes[i].children {
                if &names(&current)?!=expected {return Err("Project inventory changed during archive collection".into());}
                if !stable(&nodes[i].meta,&current.metadata().map_err(|e|e.to_string())?) {return Err("Project directory changed during archive validation".into());}
            }
        }
        // Ancestor directories may legitimately gain unrelated siblings. Check
        // their identity/binding, while project directories above check changes.
        for i in 1..anchors.len() {
            let current=open(anchors[i-1].file.as_raw_fd(),&anchors[i].name,DIRECTORY|PATH_ONLY)?;
            if !same_object(&anchors[i].meta,&current.metadata().map_err(|e|e.to_string())?) {return Err("Project ancestor was replaced during archive collection".into());}
        }
        if root.canonicalize().map_err(|e|e.to_string())?!=resolved {return Err("Project root alias changed during archive collection".into());}
        Ok((out,files,directories))
    }
    fn optional(fd:i32,name:&OsStr,flags:i32)->Result<Option<fs::File>> {
        let name=CString::new(name.as_bytes()).map_err(|_|"Invalid destination path")?;
        let raw=unsafe{openat(fd,name.as_ptr(),flags|NOFOLLOW|CLOEXEC|NONBLOCK)};
        if raw<0 {let e=std::io::Error::last_os_error();if e.raw_os_error()==Some(2){return Ok(None);}return Err(e.to_string());}
        Ok(Some(unsafe{fs::File::from_raw_fd(raw)}))
    }
    fn directory_at(fd:i32,name:&OsStr)->Result<(fs::File,bool)> {
        if let Some(file)=optional(fd,name,DIRECTORY|PATH_ONLY)? {return Ok((file,false));}
        let c=CString::new(name.as_bytes()).map_err(|_|"Invalid destination directory")?;
        let made=unsafe{mkdirat(fd,c.as_ptr(),0o700)}==0;
        if !made && std::io::Error::last_os_error().raw_os_error()!=Some(17) {return Err(std::io::Error::last_os_error().to_string());}
        Ok((open(fd,name,DIRECTORY|PATH_ONLY)?,made))
    }
    fn open_root(root: &Path) -> Result<fs::File> {
        let resolved=root.canonicalize().map_err(|e|e.to_string())?;
        let mut directory=open(AT_CWD,OsStr::new("/"),DIRECTORY|PATH_ONLY)?;
        for part in resolved.components() {
            if let std::path::Component::Normal(name)=part {
                directory=open(directory.as_raw_fd(),name,DIRECTORY|PATH_ONLY)?;
            }
        }
        Ok(directory)
    }
    pub(super) fn snapshot(root:&Path,parts:&[String])->Result<Vec<u8>> {
        let (name, parents)=parts.split_last().ok_or("Choose a project file")?;
        let mut directory=open_root(root)?;
        for part in parents {
            directory=open(directory.as_raw_fd(),OsStr::new(part),DIRECTORY|PATH_ONLY)?;
        }
        let file=open(directory.as_raw_fd(),OsStr::new(name),NONBLOCK)?;
        let before=file.metadata().map_err(|e|e.to_string())?;
        let bytes=super::read_snapshot(&file,&before)?;
        if !stable(&before,&file.metadata().map_err(|e|e.to_string())?) {
            return Err("Project file changed while reading".into());
        }
        Ok(bytes)
    }
    fn create_file(parent:&fs::File,name:&OsStr,access:i32)->Result<fs::File> {
        let name=CString::new(name.as_bytes()).map_err(|_|"Invalid project filename")?;
        let raw=unsafe{openat(parent.as_raw_fd(),name.as_ptr(),access|CREATE_EXCLUSIVE|NOFOLLOW|CLOEXEC|NONBLOCK,0o600u32)};
        if raw<0 {return Err(std::io::Error::last_os_error().to_string());}
        Ok(unsafe{fs::File::from_raw_fd(raw)})
    }
    fn unlink_at(fd:i32,name:&OsStr)->Result<()> {
        let name=CString::new(name.as_bytes()).map_err(|_|"Invalid project filename")?;
        if unsafe{unlinkat(fd,name.as_ptr(),0)}!=0 {return Err(std::io::Error::last_os_error().to_string());}
        Ok(())
    }
    /// Remove only the inode this handle created. A different directory entry at the same name is left in place.
    fn unlink_created(fd:i32,name:&OsStr,file:&fs::File)->Result<()> {
        let ours=file.metadata().map_err(|e|e.to_string())?;
        let Some(checked)=optional(fd,name,0)? else {return Ok(());};
        let meta=checked.metadata().map_err(|e|e.to_string())?;
        if !same_object(&ours,&meta) {return Err("replacement file occupies the new name".into());}
        remove_checked(fd,name,&checked)
    }
    /// Directory entry currently naming this open file. Empty when the entry is already gone.
    fn held_leaf(file:&fs::File)->Option<OsString> {
        #[cfg(any(target_os="linux",target_os="android"))] {
            let link=format!("/proc/self/fd/{}",file.as_raw_fd());
            let path=fs::read_link(link).ok()?;
            if path.as_os_str().as_bytes().ends_with(b" (deleted)") {return None;}
            return path.file_name().map(|leaf| leaf.to_os_string());
        }
        #[cfg(target_os="macos")] {
            let mut buf=[0u8;1024];
            if unsafe{fcntl(file.as_raw_fd(),50,buf.as_mut_ptr())}==-1 {return None;}
            let end=buf.iter().position(|&byte| byte==0).unwrap_or(buf.len());
            let path=Path::new(OsStr::from_bytes(&buf[..end]));
            return path.file_name().map(|leaf| leaf.to_os_string());
        }
    }
    /// Unlink the directory entry the checked handle still names. The handle stays open across the removal.
    fn remove_checked(fd:i32,name:&OsStr,checked:&fs::File)->Result<()> {
        #[cfg(any(test, rllm_module_tests))] super::source_hook("rollback_unlink");
        let ours=checked.metadata().map_err(|e|e.to_string())?;
        let leaf=held_leaf(checked).unwrap_or_else(|| name.to_os_string());
        let Some(current)=optional(fd,&leaf,0)? else {return Ok(());};
        let meta=current.metadata().map_err(|e|e.to_string())?;
        if !same_object(&ours,&meta) {return Err("replacement file occupies the new name".into());}
        let removed=unlink_at(fd,&leaf);
        drop(current);
        removed
    }
    pub(super) fn deliver(root:&Path,name:&str,create:Option<&[u8]>)->Result<Option<Vec<u8>>> {
        use std::io::{Seek, Write};
        let directory=open_root(root)?;
        let name=OsStr::new(name);
        let Some(data)=create else {
            let Some(file)=optional(directory.as_raw_fd(),name,0)? else {return Ok(None);};
            let before=file.metadata().map_err(|e|e.to_string())?;
            let read=super::read_snapshot(&file,&before)?;
            let check=open(directory.as_raw_fd(),name,PATH_ONLY|NONBLOCK)?;
            if !stable(&before,&check.metadata().map_err(|e|e.to_string())?)
                || !same_object(&directory.metadata().map_err(|e|e.to_string())?,&open_root(root)?.metadata().map_err(|e|e.to_string())?) {
                return Err("Project file or destination changed during readback".into());
            }
            return Ok(Some(read));
        };
        if !same_object(&directory.metadata().map_err(|e|e.to_string())?,&open_root(root)?.metadata().map_err(|e|e.to_string())?) {
            return Err("Destination moved before save".into());
        }
        #[cfg(any(test, rllm_module_tests))] super::source_hook("deliver_create");
        let mut file=create_file(&directory,name,2)?;
        let committed=(||->Result<Vec<u8>>{
            file.write_all(data).and_then(|_|file.sync_all()).and_then(|_|file.rewind())
                .map_err(|e|format!("New file may be partial: {e}"))?;
            let before=file.metadata().map_err(|e|e.to_string())?;
            #[cfg(any(test, rllm_module_tests))] super::source_hook("deliver_verify");
            let read=super::read_snapshot(&file,&before)?;
            let check=open(directory.as_raw_fd(),name,PATH_ONLY|NONBLOCK)?;
            if !stable(&before,&check.metadata().map_err(|e|e.to_string())?)
                || !same_object(&directory.metadata().map_err(|e|e.to_string())?,&open_root(root)?.metadata().map_err(|e|e.to_string())?) {
                return Err("Project file or destination changed during readback".into());
            }
            Ok(read)
        })();
        match committed {
            Ok(read)=>Ok(Some(read)),
            Err(error)=>{
                let cleanup=unlink_created(directory.as_raw_fd(),name,&file);
                drop(file);
                match cleanup {
                    Ok(())=>{
                        if error.contains("may be partial") {Err(error.replacen("may be partial","was removed after a partial write",1))}
                        else {Err(error)}
                    }
                    Err(unlink_error)=>Err(format!("{error}; new file was not removed ({unlink_error})")),
                }
            }
        }
    }
    struct Target {file:fs::File,meta:fs::Metadata,parent:usize,name:OsString}
    fn bindings(nodes:&[Target])->Result<()> {
        for i in 1..nodes.len() {
            let now=open(nodes[nodes[i].parent].file.as_raw_fd(),&nodes[i].name,DIRECTORY|PATH_ONLY)?;
            if !same_object(&nodes[i].meta,&now.metadata().map_err(|e|e.to_string())?) {return Err("Import destination directory was replaced".into());}
        }
        Ok(())
    }
    fn target_path(root:&fs::File,path:&Path,create:bool,identities:&mut std::collections::BTreeMap<std::path::PathBuf,fs::Metadata>)->Result<(Option<Vec<Target>>,usize)> {
        let file=root.try_clone().map_err(|e|e.to_string())?;let meta=file.metadata().map_err(|e|e.to_string())?;
        let mut chain=vec![Target{file,meta,parent:0,name:OsString::new()}];let mut created=0;let mut relative=std::path::PathBuf::new();
        for part in path.components() {
            let std::path::Component::Normal(name)=part else{return Err("Unsafe destination directory".into());};
            relative.push(name);let parent=chain.len()-1;
            let (file,made)=if create {directory_at(chain[parent].file.as_raw_fd(),name)?}else{
                match optional(chain[parent].file.as_raw_fd(),name,DIRECTORY|PATH_ONLY)?{Some(f)=>(f,false),None=>return Ok((None,created))}
            };
            let meta=file.metadata().map_err(|e|e.to_string())?;
            if let Some(expected)=identities.get(&relative) {
                if !same_object(expected,&meta){return Err("Import destination directory identity changed".into());}
            }else{
                if identities.len() as u64 >= super::entry_limit() {return Err("Import directory metadata exceeds bound".into());}
                let identity_bytes=(identities.len() as u64).saturating_mul(256);
                if identity_bytes>super::project_budget(){return Err("Import directory metadata exceeds the memory budget".into());}
                identities.insert(relative.clone(),meta.clone());
            }
            if chain.len()>64{return Err("Import directory depth exceeds bound".into());}
            chain.push(Target{file,meta,parent,name:name.to_os_string()});created+=usize::from(made);
        }
        Ok((Some(chain),created))
    }
    pub(super) fn restore(root:&Path,entries:&[(String,Vec<u8>)])->Result<(usize,usize)> {
        use std::io::Write;
        // Resolve an explicitly selected root alias only once. Missing root
        // components are created relative to held, no-follow ancestors.
        let absolute=if root.is_absolute(){root.to_path_buf()}else{std::env::current_dir().map_err(|e|e.to_string())?.join(root)};
        let mut existing=absolute.clone();let mut missing=Vec::new();
        let resolved=loop {
            match existing.canonicalize(){Ok(p)=>break p,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
                let name=existing.file_name().ok_or("Invalid import root")?.to_os_string();missing.push(name);
                if !existing.pop(){return Err("Invalid import root".into());}
            },Err(e)=>return Err(e.to_string())}
        };
        let file=open(AT_CWD,OsStr::new("/"),DIRECTORY|PATH_ONLY)?;let meta=file.metadata().map_err(|e|e.to_string())?;
        let mut nodes=vec![Target{file,meta,parent:0,name:OsString::new()}];let mut parent=0;
        for part in resolved.components() {
            if let std::path::Component::Normal(name)=part {
                if nodes.len()>=128{return Err("Import root exceeds ancestor limit".into());}
                let file=open(nodes[parent].file.as_raw_fd(),name,DIRECTORY|PATH_ONLY)?;let meta=file.metadata().map_err(|e|e.to_string())?;
                nodes.push(Target{file,meta,parent,name:name.to_os_string()});parent=nodes.len()-1;
            }
        }
        for name in missing.iter().rev(){
            if nodes.len()>=128{return Err("Import root exceeds ancestor limit".into());}
            let(file,_)=directory_at(nodes[parent].file.as_raw_fd(),name)?;let meta=file.metadata().map_err(|e|e.to_string())?;
            nodes.push(Target{file,meta,parent,name:name.clone()});parent=nodes.len()-1;
        }
        let root_index=parent;let mut root_resolved=resolved.clone();for name in missing.iter().rev(){root_resolved.push(name);}
        if absolute.canonicalize().map_err(|e|e.to_string())?!=root_resolved{return Err("Import root alias changed before writing".into());}
        let mut identities=std::collections::BTreeMap::new();
        // Record existing destination identities. Hold only the current ancestor
        // chain, rather than one descriptor for every imported directory. Preflight conflicts
        // before file creation; later create_new is still authoritative.
        for (name,_) in entries {
            let path=Path::new(name.trim_end_matches('/'));let directory=name.ends_with('/');
            let parent_path=if directory{path}else{path.parent().ok_or("Invalid project parent")?};
            let (chain,_)=target_path(&nodes[root_index].file,parent_path,false,&mut identities)?;
            if !directory {if let Some(chain)=chain {
                if optional(chain.last().unwrap().file.as_raw_fd(),path.file_name().ok_or("Invalid project file")?,PATH_ONLY)?.is_some(){return Err(format!("Project path already exists: {name}"));}
            }}
        }
        bindings(&nodes)?;
        #[cfg(any(test, rllm_module_tests))] super::source_hook("import_preflight");
        let mut created=0usize;let mut folders=0usize;
        for (name,data) in entries {
            super::stop_unless_running()?;
            let operation=(||->Result<()>{
                bindings(&nodes)?;
                let path=Path::new(name.trim_end_matches('/'));let directory=name.ends_with('/');
                let parent_path=if directory{path}else{path.parent().ok_or("Invalid project parent")?};
                let(chain,made)=target_path(&nodes[root_index].file,parent_path,true,&mut identities)?;folders+=made;
                let chain=chain.ok_or("Missing destination directory")?;bindings(&chain)?;
                if directory{return Ok(());}
                let at=chain.len()-1;
                // The binding check and the create are not one step. A rename in
                // that gap leaves the new file in the moved directory unless it is removed.
                #[cfg(any(test, rllm_module_tests))] super::source_hook("import_parent");
                let filename=path.file_name().ok_or("Invalid project file")?;
                let mut file=create_file(&chain[at].file,filename,1)?;
                let committed=(||->Result<()>{
                    let first=file.metadata().map_err(|e|e.to_string())?;
                    if !first.is_file(){return Err("Destination is not a regular file".into());}
                    file.write_all(data).and_then(|_|file.sync_all()).map_err(|e|format!("New file may be partial: {e}"))?;
                    #[cfg(any(test, rllm_module_tests))] super::source_hook("import_verify");
                    let check=open(chain[at].file.as_raw_fd(),filename,PATH_ONLY|NONBLOCK)?;let meta=check.metadata().map_err(|e|e.to_string())?;
                    if first.dev()!=meta.dev()||first.ino()!=meta.ino()||meta.len()!=data.len() as u64{return Err("New destination file was replaced".into());}
                    bindings(&chain)?;bindings(&nodes)?;
                    let fresh=open_root(root)?;
                    if !same_object(&nodes[root_index].meta,&fresh.metadata().map_err(|e|e.to_string())?) {
                        return Err("Import destination moved outside the selected folder".into());
                    }
                    Ok(())
                })();
                if let Err(error)=committed {
                    let cleanup=unlink_created(chain[at].file.as_raw_fd(),filename,&file);
                    drop(file);
                    let error=match cleanup {
                        Ok(())=>error,
                        Err(unlink_error)=>format!("{error}; new file was not removed ({unlink_error})"),
                    };
                    return Err(error);
                }
                drop(file);created+=1;Ok(())
            })();
            if let Err(e)=operation{return Err(format!("After {created} verified files and {folders} folders, new files may remain incomplete; {name}: {e}"));}
        }
        bindings(&nodes)?;
        let paths:Vec<_>=identities.keys().cloned().collect();
        for path in paths {
            let(chain,_)=target_path(&nodes[root_index].file,&path,false,&mut identities)?;
            bindings(&chain.ok_or("Imported directory disappeared; new files may remain")?)?;
        }
        if absolute.canonicalize().map_err(|e|e.to_string())?!=root_resolved{return Err("Import root alias changed; new files may remain".into());}
        Ok((created,folders))
    }

}
#[cfg(any(test, rllm_module_tests))]
thread_local! {static SOURCE_HOOK:std::cell::RefCell<Option<Box<dyn FnMut(&str)>>> = std::cell::RefCell::new(None);}
#[cfg(any(test, rllm_module_tests))]
fn source_hook(phase:&str) {SOURCE_HOOK.with(|h|{if let Some(h)=h.borrow_mut().as_mut(){h(phase);}});}
#[cfg(windows)]
mod windows_source {
    use super::*;
    use std::ffi::{c_void, OsStr};
    use std::io::{self, Read, Write};
    use std::os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle},
    };
    use std::path::{Component, PathBuf, Prefix};

    const REPARSE: u32 = 0x400;
    const FILE_OPEN: u32 = 1;
    const FILE_CREATE: u32 = 2;
    const DIRECTORY: u32 = 1;
    const OPEN_REPARSE: u32 = 0x0020_0000;
    const READ_ATTRIBUTES: u32 = 0x80;
    const READ_DATA: u32 = 1;
    const WRITE_DATA: u32 = 2;
    const SHARE_READ: u32 = 1;
    const SHARE_WRITE: u32 = 2;
    const SHARE_DELETE: u32 = 4;
    /// Held project directories deny delete sharing so a rename cannot move them.
    const SHARE_HELD: u32 = SHARE_READ | SHARE_WRITE;
    /// The volume root keeps delete sharing so the open succeeds beside system handles.
    const SHARE_VOLUME: u32 = SHARE_READ | SHARE_WRITE | SHARE_DELETE;
    const LIST_DIRECTORY: u32 = 1;
    const TRAVERSE: u32 = 0x20;
    const DIR_ACCESS: u32 = LIST_DIRECTORY | TRAVERSE | READ_ATTRIBUTES;

    #[repr(C)]
    struct UnicodeString {
        length: u16,
        capacity: u16,
        buffer: *const u16,
    }
    #[repr(C)]
    struct ObjectAttributes {
        length: u32,
        root: *mut c_void,
        name: *const UnicodeString,
        attributes: u32,
        security: *const c_void,
        security_quality: *const c_void,
    }
    #[repr(C)]
    struct IoStatus {
        status: usize,
        information: usize,
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtCreateFile(
            file: *mut *mut c_void, access: u32, attributes: *const ObjectAttributes,
            status: *mut IoStatus, size: *const i64, file_attributes: u32, share: u32,
            disposition: u32, options: u32, ea: *const c_void, ea_length: u32,
        ) -> i32;
        fn RtlNtStatusToDosError(status: i32) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(handle: *mut c_void, class: i32, info: *mut c_void, size: u32) -> i32;
        fn GetFinalPathNameByHandleW(handle: *mut c_void, buffer: *mut u16, length: u32, flags: u32) -> u32;
    }

    fn nt_error(code: i32) -> io::Error {
        let win32 = unsafe { RtlNtStatusToDosError(code) };
        // STATUS_STOPPED_ON_SYMLINK / ERROR_STOPPED_ON_SYMLINK: the final
        // component is a reparse point and was not opened.
        if code as u32 == 0x8000_002D || win32 == 681 {
            return io::Error::new(io::ErrorKind::InvalidData, "Project archive refuses symlinks and other reparse points");
        }
        io::Error::from_raw_os_error(win32 as i32)
    }

    fn metadata(file: &fs::File) -> io::Result<fs::Metadata> {
        let meta = file.metadata()?;
        if meta.file_attributes() & REPARSE != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Project archive refuses symlinks and other reparse points"));
        }
        Ok(meta)
    }

    // A single component is resolved from the held parent. No path is rebuilt
    // for opening or creating an archive entry. Project handles deny delete sharing.
    fn final_path(file: &fs::File) -> io::Result<String> {
        let mut buffer = vec![0u16; 2048];
        let wrote = unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), buffer.as_mut_ptr(), buffer.len() as u32, 0) };
        if wrote == 0 || wrote as usize >= buffer.len() { return Err(io::Error::last_os_error()); }
        let mut text = String::from_utf16_lossy(&buffer[..wrote as usize]).replace('/', "\\");
        if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") { text = format!("\\\\{rest}"); }
        else if let Some(rest) = text.strip_prefix("\\\\?\\") { text = rest.to_string(); }
        Ok(text.trim_end_matches('\\').to_ascii_lowercase())
    }
    fn still_bound(file: &fs::File, pinned: &str) -> Result<()> {
        let current = final_path(file).map_err(|e| e.to_string())?;
        if current == pinned || current.starts_with(&format!("{pinned}\\")) { Ok(()) }
        else { Err("Import destination directory was moved".into()) }
    }
    fn child(parent: &fs::File, part: &OsStr, access: u32, share: u32,
             disposition: u32, options: u32) -> io::Result<(fs::File, fs::Metadata)> {
        metadata(parent)?;
        let wide: Vec<u16> = part.encode_wide().collect();
        let length = u16::try_from(wide.len().saturating_mul(2))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Project filename is too long"))?;
        let name = UnicodeString { length, capacity: length, buffer: wide.as_ptr() };
        let attributes = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root: parent.as_raw_handle(),
            name: &name,
            attributes: 0x40 | 0x1000, // Case-insensitive lookup and fail on reparse.
            security: std::ptr::null(),
            security_quality: std::ptr::null(),
        };
        let mut file = std::ptr::null_mut();
        let mut status = IoStatus { status: 0, information: 0 };
        let code = unsafe {
            NtCreateFile(&mut file, access | READ_ATTRIBUTES | 0x100000, &attributes,
                         &mut status, std::ptr::null(), 0, share, disposition,
                         options | 0x20, std::ptr::null(), 0)
        };
        if code < 0 {
            return Err(nt_error(code));
        }
        let file = unsafe { fs::File::from_raw_handle(file) };
        let meta = metadata(&file)?;
        Ok((file, meta))
    }

    fn segment(part: &str) -> Result<()> {
        let stem = part.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| matches!(suffix, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
            });
        if part.is_empty() || part.ends_with([' ', '.']) || device
            || part.chars().any(|c| c.is_control() || r#"\/:<>*?"|"#.contains(c)) {
            return Err("Unsafe project path on device storage".into());
        }
        Ok(())
    }

    fn open_child(parent: &fs::File, name: &OsStr, access: u32, share: u32) -> io::Result<(fs::File, fs::Metadata)> {
        child(parent, name, access, share, FILE_OPEN, OPEN_REPARSE)
    }

    fn root_parts(root: &Path) -> Result<(PathBuf, Vec<std::ffi::OsString>)> {
        if root.as_os_str().is_empty() {
            return Err("Choose a project folder".into());
        }
        let root = std::path::absolute(root).map_err(|e| e.to_string())?;
        let mut parts = root.components();
        let Some(Component::Prefix(prefix)) = parts.next() else { return Err("Invalid project folder".into()); };
        if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_) | Prefix::UNC(..) | Prefix::VerbatimUNC(..))
            || parts.next() != Some(Component::RootDir) {
            return Err("Choose an absolute drive or network folder".into());
        }
        let mut volume = PathBuf::from(prefix.as_os_str());
        volume.push(Path::new(r"\"));
        let mut names = Vec::new();
        for part in parts {
            let Component::Normal(name) = part else { return Err("Invalid project folder component".into()); };
            if names.len() >= 128 {
                return Err("Project folder exceeds 128 ancestor components".into());
            }
            names.push(name.to_os_string());
        }
        Ok((volume, names))
    }

    fn root_chain(root: &Path, create: bool) -> Result<Vec<fs::File>> {
        let (volume, names) = root_parts(root)?;
        let file = fs::OpenOptions::new().access_mode(DIR_ACCESS | 0x100000)
            .share_mode(SHARE_VOLUME)
            .custom_flags(OPEN_REPARSE | 0x0200_0000)
            .open(volume).map_err(|e| e.to_string())?;
        let mut chain = vec![file];
        let last = names.len();
        for (index, name) in names.into_iter().enumerate() {
            let parent = chain.last().unwrap();
            // Ancestors stay compatible with system handles. The project directory
            // itself denies delete sharing so a rename cannot move it.
            let share = if index + 1 == last { SHARE_HELD } else { SHARE_VOLUME };
            let (file, _) = directory_shared(parent, &name, create, share)?.ok_or("Project folder does not exist")?;
            chain.push(file);
        }
        Ok(chain)
    }

    fn directory(parent: &fs::File, name: &OsStr, create: bool) -> Result<Option<(fs::File, bool)>> {
        directory_shared(parent, name, create, SHARE_HELD)
    }

    fn directory_shared(parent: &fs::File, name: &OsStr, create: bool, share: u32) -> Result<Option<(fs::File, bool)>> {
        match open_child(parent, name, DIR_ACCESS, share) {
            Ok((file, meta)) if meta.is_dir() => Ok(Some((file, false))),
            Ok(_) => Err("Project destination is not a directory".into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound && !create => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match child(parent, name, 0, SHARE_READ | SHARE_WRITE, FILE_CREATE, DIRECTORY) {
                    Ok((file, _)) => Ok(Some((file, true))),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => directory_shared(parent, name, false, share),
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn names(dir: &fs::File) -> Result<Vec<String>> {
        // Walk FILE_ID_BOTH_DIR_INFO on the held handle. A path lookup would
        // follow a directory that was replaced by a junction after the open.
        // The info records contain 8-byte fields, so the buffer is aligned.
        // A query that never reports the end, or a record that never advances,
        // stops instead of spinning. Cancellation is checked on each query.
        let mut names = Vec::new();
        let mut aligned = Box::new([0u64; 8192]);
        let mut restart = true;
        let mut passes = 0u64;
        let pass_limit = super::entry_limit().saturating_add(8);
        loop {
            super::stop_unless_running()?;
            passes += 1;
            if passes > pass_limit { return Err("Directory listing did not finish".into()); }
            let class = if restart { 11 } else { 10 };
            let ok = unsafe {
                GetFileInformationByHandleEx(dir.as_raw_handle(), class,
                    aligned.as_mut_ptr().cast(), 65536)
            };
            let api_error = if ok == 0 { Some(io::Error::last_os_error()) } else { None };
            restart = false;
            if ok == 0 {
                let error = api_error.unwrap();
                if error.raw_os_error() == Some(18) { break; }
                return Err(error.to_string());
            }
            let buffer = unsafe { std::slice::from_raw_parts(aligned.as_ptr().cast::<u8>(), 65536) };
            let mut start = 0usize;
            let mut steps = 0u32;
            loop {
                steps += 1;
                if steps > 1024 { return Err("Directory listing did not finish".into()); }
                let record = buffer.get(start..).filter(|bytes| bytes.len() >= 104).ok_or("Invalid directory record")?;
                let field = |offset: usize| u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
                let next = field(0) as usize;
                let attributes = field(56);
                let length = field(60) as usize;
                let end = 104usize.checked_add(length).ok_or("Invalid directory record")?;
                if length == 0 || length % 2 != 0 || end > record.len() || (next != 0 && (next < end || next > record.len())) {
                    return Err("Invalid directory record".into());
                }
                let wide: Vec<u16> = record[104..end].chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
                let name = String::from_utf16(&wide).map_err(|_| "Project filename is not Unicode")?;
                if next == 0 { start = usize::MAX; } else { start += next; }
                if name == "." || name == ".." {
                    if start == usize::MAX { break; }
                    continue;
                }
                if attributes & REPARSE != 0 {
                    return Err("Project archive refuses symlinks and other reparse points".into());
                }
                if names.len() as u64 >= super::entry_limit() {
                    return Err(format!("Project folder exceeds {} entries", super::entry_limit()));
                }
                segment(&name)?;
                names.push(name);
                if start == usize::MAX { break; }
            }
        }
        names.sort_unstable();
        if names.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("Project directory changed during enumeration".into());
        }
        Ok(names)
    }

    struct Node {
        name: String,
        meta: fs::Metadata,
        children: Option<Vec<String>>,
    }

    fn same_entry(left: &fs::Metadata, right: &fs::Metadata) -> bool {
        left.file_attributes() == right.file_attributes()
            && left.creation_time() == right.creation_time()
            && left.last_write_time() == right.last_write_time()
            && left.len() == right.len()
            && left.is_dir() == right.is_dir()
            && left.is_file() == right.is_file()
    }

    fn open_project_entry(root: &fs::File, name: &str, is_dir: bool) -> Result<(fs::File, fs::Metadata)> {
        let trimmed = name.trim_end_matches('/');
        if trimmed.is_empty() {
            let file = root.try_clone().map_err(|e| e.to_string())?;
            let meta = metadata(&file).map_err(|e| e.to_string())?;
            return Ok((file, meta));
        }
        let mut parts: Vec<&str> = trimmed.split('/').collect();
        let leaf = parts.pop().ok_or("Invalid project path")?;
        let mut dir = root.try_clone().map_err(|e| e.to_string())?;
        for part in parts {
            let (next, meta) = open_child(&dir, OsStr::new(part), DIR_ACCESS, SHARE_HELD).map_err(|e| e.to_string())?;
            if !meta.is_dir() { return Err("Project entry was replaced during archive collection".into()); }
            dir = next;
        }
        let (access, share) = if is_dir { (DIR_ACCESS, SHARE_HELD) } else { (READ_DATA, SHARE_READ) };
        open_child(&dir, OsStr::new(leaf), access, share).map_err(|e| e.to_string())
    }

    fn inventory(nodes: &Vec<Node>, order: &Vec<usize>, folded: &std::collections::BTreeSet<String>) -> u64 {
        let mut total = (nodes.capacity() * std::mem::size_of::<Node>()) as u64;
        total += (order.capacity() * std::mem::size_of::<usize>()) as u64;
        for node in nodes {
            total += node.name.capacity() as u64;
            if let Some(children) = &node.children { total += super::vec_string_bytes(children); }
        }
        total + super::tree_string_bytes(folded)
    }

    pub(super) fn archive(root: &Path) -> Result<(Vec<u8>, usize, usize)> {
        let chain = root_chain(root, false)?;
        let project = chain.last().unwrap();
        super::reset_entry_handles();
        let mut nodes = vec![Node {
            name: String::new(),
            meta: metadata(project).map_err(|e| e.to_string())?,
            children: None,
        }];
        let mut pending = vec![0];
        let mut total: u64 = 0;
        let mut folded = std::collections::BTreeSet::new();
        while let Some(index) = pending.pop() {
            let parent_name = nodes[index].name.clone();
            let (dir, dir_meta) = open_project_entry(project, &parent_name, true)?;
            let dir = super::EntryFile::new(dir);
            if !same_entry(&nodes[index].meta, &dir_meta) {
                return Err("Project entry changed during archive collection".into());
            }
            let children = names(&dir)?;
            for part in &children {
                if nodes.len() as u64 > super::entry_limit() {
                    return Err(format!("Project archive exceeds {} entries", super::entry_limit()));
                }
                segment(part)?;
                let (opened, meta) = open_child(&dir, OsStr::new(part), DIR_ACCESS, SHARE_HELD)
                    .map_err(|e| e.to_string())?;
                let opened = super::EntryFile::new(opened);
                let mut name = format!("{parent_name}{part}");
                valid_name(name.as_bytes())?;
                if !folded.insert(name.to_lowercase()) {
                    return Err("Ambiguous project path casing".into());
                }
                let meta = if meta.is_dir() {
                    name.push('/');
                    pending.push(nodes.len());
                    drop(opened);
                    meta
                } else if meta.is_file() && super::admits_file(meta.len()) {
                    let identity = (meta.file_attributes(), meta.creation_time(), meta.last_write_time(), meta.len());
                    drop(opened);
                    // Confirm the bytes against a read handle, then close it.
                    // Delete sharing stays off only while that handle is open.
                    let (file, meta) = open_child(&dir, OsStr::new(part), READ_DATA, SHARE_READ)
                        .map_err(|e| e.to_string())?;
                    let file = super::EntryFile::new(file);
                    if (meta.file_attributes(), meta.creation_time(), meta.last_write_time(), meta.len()) != identity || !meta.is_file() {
                        return Err("Project file changed during archive read".into());
                    }
                    if !super::admits_file(meta.len()) {
                        return Err("Project file exceeds the memory budget".into());
                    }
                    drop(file);
                    meta
                } else {
                    return Err("Project file exceeds the memory budget".into());
                };
                let size = if meta.is_dir() { 0 } else { meta.len() };
                total = total.checked_add(size).ok_or("Project archive size overflow")?;
                if total > super::data_limit() {
                    return Err("Project archive exceeds the memory budget".into());
                }
                nodes.push(Node { name, meta, children: None });
            }
            nodes[index].children = Some(children);
        }
        drop(pending);
        if nodes.len() == 1 {
            return Err("No project entries to export".into());
        }
        for node in &nodes {
            let (file, meta) = open_project_entry(project, &node.name, node.meta.is_dir())?;
            let file = super::EntryFile::new(file);
            if !same_entry(&node.meta, &meta) {
                return Err("Project entry changed during archive collection".into());
            }
            if let Some(expected) = &node.children {
                if &names(&file)? != expected {
                    return Err("Project inventory changed during archive collection".into());
                }
            }
        }
        let mut order: Vec<usize> = (1..nodes.len()).collect();
        order.sort_by(|&a, &b| nodes[a].name.cmp(&nodes[b].name));
        let mut local_end = 0u64;
        let mut central = 0u64;
        for &index in &order {
            let name_len = nodes[index].name.len();
            let size = if nodes[index].meta.is_dir() { 0 } else { nodes[index].meta.len() };
            let local_extra = if super::zip64_size(size) { 20 } else { 0 };
            central = central.checked_add(46 + name_len as u64 + super::central_extra_len(size, local_end)).ok_or("Archive size overflow")?;
            local_end = local_end.checked_add(30 + name_len as u64 + local_extra + size).ok_or("Archive size overflow")?;
        }
        let footer = super::zip64_footer(order.len() as u64, central, local_end);
        let length = local_end.checked_add(central).and_then(|n| n.checked_add(22 + if footer { 76 } else { 0 })).ok_or("Archive size overflow")?;
        let side = (order.len() * (std::mem::size_of::<u32>() + std::mem::size_of::<u64>())) as u64;
        super::admit_peak(length, side, inventory(&nodes, &order, &folded))?;
        let mut crcs = Vec::new();
        crcs.try_reserve_exact(order.len()).map_err(|_| "Cannot allocate project archive")?;
        let mut offsets = Vec::new();
        offsets.try_reserve_exact(order.len()).map_err(|_| "Cannot allocate project archive")?;
        let reserved = super::resident_len(length)?;
        let mut out = Vec::new();
        out.try_reserve_exact(reserved).map_err(|_| "Cannot allocate project archive")?;
        let mut files = 0usize;
        let mut directories = 0usize;
        let mut scratch = [0u8; super::STREAM_WINDOW];
        for &index in &order {
            let is_dir = nodes[index].meta.is_dir();
            let name = nodes[index].name.clone();
            if is_dir { directories += 1; } else { files += 1; }
            let size = if is_dir { 0 } else { nodes[index].meta.len() };
            let header = super::local_header(&mut out, &name, size)?;
            let mut crc = !0u32;
            if !is_dir {
                let (file, meta) = open_project_entry(project, &name, false)?;
                let mut reader = super::EntryFile::new(file);
                if !same_entry(&nodes[index].meta, &meta) || meta.len() != size {
                    return Err("Project file changed during archive read".into());
                }
                let mut remaining = usize::try_from(size).map_err(|_| "Project file exceeds addressable memory")?;
                while remaining > 0 {
                    super::stop_unless_running()?;
                    let count = remaining.min(scratch.len());
                    reader.read_exact(&mut scratch[..count]).map_err(|e| e.to_string())?;
                    crc = crc_update(crc, &scratch[..count]);
                    out.extend_from_slice(&scratch[..count]);
                    remaining -= count;
                }
                let mut extra = [0u8; 1];
                if reader.read(&mut extra).map_err(|e| e.to_string())? != 0
                    || metadata(&reader).map_err(|e| e.to_string())?.len() != size {
                    return Err("Project file changed during archive read".into());
                }
            }
            let crc = !crc;
            super::patch_crc(&mut out, header.crc_at, crc);
            crcs.push(crc);
            offsets.push(header.offset);
        }
        let directory_at = out.len() as u64;
        for (slot, &index) in order.iter().enumerate() {
            let is_dir = nodes[index].meta.is_dir();
            let name = nodes[index].name.clone();
            let size = if is_dir { 0 } else { nodes[index].meta.len() };
            super::central_header(&mut out, &name, crcs[slot], size, offsets[slot], is_dir)?;
        }
        let directory_len = out.len() as u64 - directory_at;
        super::finish_archive(&mut out, directory_at, directory_len, order.len() as u64)?;
        if out.len() as u64 != length { return Err("Archive length mismatch".into()); }
        Ok((out, files, directories))
    }

    fn parents(root: &fs::File, parts: &[&str], create: bool) -> Result<Option<(Vec<fs::File>, usize)>> {
        let mut chain = Vec::with_capacity(parts.len());
        let mut created = 0;
        for part in parts {
            let parent = chain.last().unwrap_or(root);
            let Some((file, made)) = directory(parent, OsStr::new(part), create)? else { return Ok(None); };
            chain.push(file);
            created += usize::from(made);
        }
        Ok(Some((chain, created)))
    }

    pub(super) fn restore(root: &Path, entries: &[(String, Vec<u8>)]) -> Result<(usize, usize)> {
        let mut planned = Vec::with_capacity(entries.len());
        let mut total = 0u64;
        for (name, data) in entries {
            total = total.checked_add(data.len() as u64).filter(|n| *n <= super::data_limit())
                .ok_or("Project import exceeds the memory budget")?;
            let (name, is_dir) = match name.strip_suffix('/') {
                Some(path) if data.is_empty() => (path, true),
                Some(_) => return Err("ZIP directory contains file data".into()),
                None => (name.as_str(), false),
            };
            valid_name(name.as_bytes())?;
            let mut parts: Vec<_> = name.split('/').collect();
            for part in &parts {
                segment(part)?;
            }
            let file = if is_dir { None } else { parts.pop() };
            planned.push((parts, file, data));
        }
        let roots = root_chain(root, true)?;
        let root = roots.last().unwrap();
        let pinned = final_path(root).map_err(|e| e.to_string())?;
        for (parts, file, _) in &planned {
            if let (Some((chain, _)), Some(name)) = (parents(root, parts, false)?, file) {
                let parent = chain.last().unwrap_or(root);
                match open_child(parent, OsStr::new(name), 0, SHARE_READ | SHARE_WRITE) {
                    Ok(_) => return Err(format!("Project path already exists: {name}")),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        let mut files = 0;
        let mut folders = 0;
        for (parts, name, data) in planned {
            super::stop_unless_running()?;
            let operation = (|| -> Result<()> {
                still_bound(root, &pinned)?;
                let (chain, made) = parents(root, &parts, true)?.ok_or("Import directory disappeared")?;
                folders += made;
                if let Some(name) = name {
                    let parent = chain.last().unwrap_or(root);
                    still_bound(parent, &pinned)?;
                    let (mut file, _) = child(parent, OsStr::new(name), WRITE_DATA, SHARE_READ, FILE_CREATE, 0x40)
                        .map_err(|e| e.to_string())?;
                    file.write_all(data).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
                    if metadata(&file).map_err(|e| e.to_string())?.len() != data.len() as u64 {
                        return Err("New destination file size changed".into());
                    }
                    files += 1;
                }
                Ok(())
            })();
            if let Err(error) = operation {
                return Err(format!("After {files} files and {folders} folders, new files may remain incomplete: {error}"));
            }
        }
        Ok((files, folders))
    }

    pub(super) fn held_directory_rejects_rename(root: &Path) -> Result<()> {
        let chain = root_chain(root, false)?;
        let moved = root.with_extension("moved");
        match fs::rename(root, &moved) {
            Err(_) => Ok(()),
            Ok(()) => {
                let _ = fs::rename(&moved, root);
                let _ = chain;
                Err("Held project directory accepted a rename".into())
            }
        }
    }

    pub(super) fn snapshot(root: &Path, parts: &[String]) -> Result<Vec<u8>> {
        let roots = root_chain(root, false)?;
        let root = roots.last().unwrap();
        let (name, parents_parts) = parts.split_last().ok_or("Choose a project file")?;
        for part in parts {
            segment(part)?;
        }
        let parts: Vec<_> = parents_parts.iter().map(String::as_str).collect();
        let (parents, _) = parents(root, &parts, false)?.ok_or("Project folder does not exist")?;
        let (file, before) = open_child(parents.last().unwrap_or(root), OsStr::new(name), READ_DATA, SHARE_READ)
            .map_err(|e| e.to_string())?;
        let bytes = super::read_snapshot(&file, &before)?;
        let after = metadata(&file).map_err(|e| e.to_string())?;
        if after.len() != before.len() || after.last_write_time() != before.last_write_time() {
            return Err("Project file changed while reading".into());
        }
        Ok(bytes)
    }

    pub(super) fn deliver(root:&Path,name:&str,create:Option<&[u8]>)->Result<Option<Vec<u8>>> {
        use std::io::Seek;
        segment(name)?;
        let roots=root_chain(root,false)?;
        let parent=roots.last().unwrap();
        let name=OsStr::new(name);
        let (file,before)=if let Some(data)=create {
            let (mut file,_)=child(parent,name,READ_DATA|WRITE_DATA,SHARE_READ,FILE_CREATE,0x40)
                .map_err(|e|e.to_string())?;
            file.write_all(data).and_then(|_|file.sync_all()).and_then(|_|file.rewind())
                .map_err(|e|format!("New file may be partial: {e}"))?;
            let meta=metadata(&file).map_err(|e|e.to_string())?;
            (file,meta)
        } else {
            match open_child(parent,name,READ_DATA,SHARE_READ) {
                Ok(entry)=>entry,
                Err(e) if e.kind()==io::ErrorKind::NotFound=>return Ok(None),
                Err(e)=>return Err(e.to_string()),
            }
        };
        let data=super::read_snapshot(&file,&before)?;
        let after=metadata(&file).map_err(|e|e.to_string())?;
        if before.len()!=after.len() || before.last_write_time()!=after.last_write_time() {
            return Err("Project file changed during readback".into());
        }
        Ok(Some(data))
    }
}

#[cfg(all(windows, any(test, rllm_module_tests)))]
fn windows_import_checks() {
    let base = std::env::current_dir()
        .unwrap()
        .join("output")
        .join(format!(
            "windows-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&base).unwrap();
    let root = base.join("project");
    let bytes = zip(&[
        ("dir/".into(), Vec::new()),
        ("dir/file.txt".into(), b"stored".to_vec()),
    ])
    .unwrap();
    assert!(unpack(&bytes).is_ok());
    let request = obj(vec![
        ("root", s(root.to_str().unwrap())),
        ("archive", s(&network::b64(&bytes))),
    ]);
    let report = import(&request).unwrap();
    assert_eq!(report.get("files"), Ok(&Num("1".into())));
    assert_eq!(report.get("directories"), Ok(&Num("1".into())));
    assert_eq!(fs::read(root.join("dir").join("file.txt")).unwrap(), b"stored");
    assert!(import(&request).is_err());
    assert_eq!(fs::read(root.join("dir").join("file.txt")).unwrap(), b"stored");

    for name in ["", ".", "..", "a/../b.txt", "a//b.txt", "a/./b.txt", "./b.txt", "a\\b.txt"] {
        let error = windows_source::restore(&root, &[(name.into(), b"no".to_vec())]).unwrap_err();
        assert!(
            error.contains("Unsafe") || error.contains("leaves") || error.contains("not UTF-8"),
            "{name}: {error}"
        );
    }
    assert!(!root.join("b.txt").exists());
    assert!(!root.join("a").join("b.txt").exists());
    assert!(!base.join("escaped.txt").exists());
    assert!(windows_source::restore(&root, &[("../escaped.txt".into(), b"no".to_vec())]).is_err());
    assert!(!base.join("escaped.txt").exists());
    let absolute = base.join("absolute-escape.txt");
    let absolute_name = absolute.to_str().unwrap().replace('\\', "/");
    assert!(windows_source::restore(&root, &[(absolute_name, b"no".to_vec())]).is_err());
    assert!(!absolute.exists());

    let capped = base.join("capped");
    let payload = vec![0u8; 5 * 1024 * 1024 + 1];
    configure_project_budget(5 * 1024 * 1024);
    let error = windows_source::restore(&capped, &[("a.txt".into(), payload.clone())]).unwrap_err();
    configure_project_budget(0);
    assert!(error.contains("memory budget"), "{error}");
    assert!(!capped.exists());
    assert!(!capped.join("a.txt").exists());
    let accepted = base.join("accepted");
    windows_source::restore(&accepted, &[("a.txt".into(), payload.clone())]).unwrap();
    assert_eq!(fs::read(accepted.join("a.txt")).unwrap(), payload);
    let held = base.join("held");
    fs::create_dir_all(&held).unwrap();
    windows_source::held_directory_rejects_rename(&held).unwrap();

    let outside = base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), b"secret").unwrap();
    let link = root.join("linked");
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J", link.to_str().unwrap(), outside.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(archive(&root).is_err());
    let linked = zip(&[("linked/newer.txt".into(), b"no".to_vec())]).unwrap();
    let linked_request = obj(vec![
        ("root", s(root.to_str().unwrap())),
        ("archive", s(&network::b64(&linked))),
    ]);
    let error = import(&linked_request).unwrap_err();
    assert!(
        error.contains("reparse") || error.contains("symlink") || error.contains("leaves"),
        "{error}"
    );
    assert!(!outside.join("newer.txt").exists());
    assert_eq!(fs::read(outside.join("secret.txt")).unwrap(), b"secret");
    assert_eq!(fs::read(root.join("dir").join("file.txt")).unwrap(), b"stored");
}

fn source_archive(root:&Path)->Result<(Vec<u8>,usize,usize)> {
    #[cfg(any(target_os="linux",target_os="android",target_os="macos"))]
    { return source::archive(root); }
    #[cfg(windows)]
    { return windows_source::archive(root); }
    #[cfg(not(any(windows, target_os="linux", target_os="android", target_os="macos")))]
    { let _=root; Err("Descriptor-safe project archives are unavailable on this target".into()) }
}
pub fn archive(root:&Path)->Result<(Vec<u8>,usize,usize)> {source_archive(root)}
fn read_snapshot(file:&fs::File,before:&fs::Metadata)->Result<Vec<u8>> {
    use std::io::Read;
    if !before.is_file() || !admits_file(before.len()) || before.len() > snapshot_limit() {
        return Err("Select a regular project file within the memory budget".into());
    }
    let size = usize::try_from(before.len()).map_err(|_| "Project file exceeds addressable memory")?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| "Project file exceeds the memory budget")?;
    let mut reader = file;
    let mut remaining = size;
    let mut scratch = [0u8; STREAM_WINDOW];
    while remaining > 0 {
        stop_unless_running()?;
        let count = remaining.min(scratch.len());
        reader.read_exact(&mut scratch[..count]).map_err(|e| e.to_string())?;
        bytes.extend_from_slice(&scratch[..count]);
        remaining -= count;
    }
    let mut extra = [0u8; 1];
    if reader.read(&mut extra).map_err(|e| e.to_string())? != 0 || bytes.len() != size {
        return Err("Project file changed while reading".into());
    }
    Ok(bytes)
}
pub(crate) fn file_snapshot(root:&Path,parts:&[String])->Result<Vec<u8>> {
    #[cfg(any(target_os="linux",target_os="android",target_os="macos"))]
    { return source::snapshot(root,parts); }
    #[cfg(windows)]
    { return windows_source::snapshot(root,parts); }
    #[cfg(not(any(windows, target_os="linux", target_os="android", target_os="macos")))]
    { let _=(root,parts); Err("Descriptor-safe project archives are unavailable on this target".into()) }
}
pub(crate) fn deliver_native(root:&Path,name:&str,create:Option<&[u8]>)->Result<Option<Vec<u8>>> {
    valid_name(name.as_bytes())?;
    if name.contains('/') {return Err("Choose a filename inside the selected folder".into());}
    #[cfg(any(target_os="linux",target_os="android",target_os="macos"))]
    { return source::deliver(root,name,create); }
    #[cfg(windows)]
    { return windows_source::deliver(root,name,create); }
    #[cfg(not(any(windows, target_os="linux", target_os="android", target_os="macos")))]
    { let _=(root,name,create); Err("Descriptor-safe project archives are unavailable on this target".into()) }
}
#[cfg(any(test, rllm_module_tests))]
fn collect(root:&Path)->Result<Vec<(String,Vec<u8>)>> {unpack(&source_archive(root)?.0)}
#[cfg(any(test, windows))]
fn zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in entries {
        let name = name.as_bytes();
        let length = u16::try_from(name.len()).map_err(|_| "Filename too long")?;
        let size = u32::try_from(data.len()).map_err(|_| "File too large")?;
        let offset = u32::try_from(out.len()).map_err(|_| "Archive too large")?;
        let crc = crc32(data);
        long(&mut out, 0x04034b50);
        short(&mut out, 20);
        short(&mut out, 0x0800);
        short(&mut out, 0);
        short(&mut out, 0);
        short(&mut out, DOS_DATE);
        long(&mut out, crc);
        long(&mut out, size);
        long(&mut out, size);
        short(&mut out, length);
        short(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        long(&mut directory, 0x02014b50);
        short(&mut directory, 20);
        short(&mut directory, 20);
        short(&mut directory, 0x0800);
        short(&mut directory, 0);
        short(&mut directory, 0);
        short(&mut directory, DOS_DATE);
        long(&mut directory, crc);
        long(&mut directory, size);
        long(&mut directory, size);
        short(&mut directory, length);
        short(&mut directory, 0);
        short(&mut directory, 0);
        short(&mut directory, 0);
        short(&mut directory, 0);
        long(&mut directory, if name.ends_with(b"/") { 0x10 } else { 0 });
        long(&mut directory, offset);
        directory.extend_from_slice(name);
    }
    let directory_at = out.len() as u64;
    let directory_len = directory.len() as u64;
    out.extend_from_slice(&directory);
    finish_archive(&mut out, directory_at, directory_len, entries.len() as u64)?;
    Ok(out)
}
pub fn build(v: &Json) -> Result<Json> {
    let root = Path::new(v.get("root")?.text()?);
    let (bytes,files,directories) = source_archive(root)?;
    // The ZIP buffer and one encoding are live together. The text is moved
    // into the result, so a second encoding-sized copy is not allocated.
    let zip_len = bytes.len();
    let encoded_len = base64_len(zip_len)?;
    admit_sum(&[zip_len as u64, encoded_len])?;
    let encoded = encode_base64(&bytes)?;
    let file_count = files.to_string();
    let directory_count = directories.to_string();
    let size_text = zip_len.to_string();
    drop(bytes);
    Ok(obj(vec![
        ("archive", Str(encoded)),
        ("files", Num(file_count)),
        ("directories", Num(directory_count)),
        ("bytes", Num(size_text)),
    ]))
}
#[cfg(any(test, rllm_module_tests))]
mod checks {
    use super::*;
    #[cfg(rllm_module_tests)]
    #[no_mangle]
    pub extern "C" fn g2_archive_tests() {
        incremental_crc_matches_known_vector_at_all_boundaries();
        zip64_metadata_is_ordered_bounded_and_unambiguous();
        zip64_footer_requires_consistent_single_disk_directory();
        compressed_archive_checks_descriptors_and_preserves_imports();
        import_preflight_preserves_existing_files();
        directory_roundtrip_preserves_empty_folders_and_existing_data();
        directory_metadata_and_path_conflicts_are_checked();
        archive_captures_project_and_rejects_symlinks();
        windows_import_writes_stored_zip_and_refuses_escape();
        windows_save_round_trip_preserves_nested_empty_binary_and_utf8();
        wide_file_length_is_judged_before_narrowing();
        deflated_members_follow_the_project_budget();
        zip64_sentinel_size_is_recorded_at_the_boundary();
        export_bounds_peak_memory_and_entry_handles();
        live_working_set_counts_the_zip_the_copy_and_the_encoding();
        deflate_bit_cursor_accepts_the_32_bit_boundary();
        flat_project_round_trips_past_the_old_prefix_cap();
        zip_reads_bzip2_lzma_zstandard_and_unicode_paths();
        build_moves_the_encoding_without_a_second_copy();
        legacy_zip_names_use_the_original_byte_page();
        method_93_reads_explicit_tables_treeless_literals_and_weight_streams();
        cancelled_unpack_is_not_reported_complete();
        zip64_exact_sentinels_round_trip();
        encoded_archive_rejects_a_suffix_after_padding();
        legacy_methods_expand_and_wide_window_reaches_64k();
        reduced_member_past_the_window_keeps_its_crc();
        wide_length_code_285_decodes_a_long_match();
        method_93_keeps_a_dictionary_identifier();
        #[cfg(all(any(target_os = "linux", target_os = "android"), target_pointer_width = "64"))]
        destination_descriptors_refuse_parent_swaps_without_following_links();
        #[cfg(all(any(target_os = "linux", target_os = "android"), target_pointer_width = "64"))]
        deliver_rename_gap_does_not_leave_a_file_outside_the_selected_folder();
        #[cfg(all(any(target_os = "linux", target_os = "android"), target_pointer_width = "64"))]
        rollback_keeps_a_replacement_at_the_new_name();
        #[cfg(all(any(target_os = "linux", target_os = "android"), target_pointer_width = "64"))]
        rollback_removes_the_created_file_and_leaves_a_swapped_name();
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn wide_file_length_is_judged_before_narrowing() {
        let wide = (u32::MAX as u64).saturating_add(64);
        assert!((wide as u32 as usize) < 1024);
        configure_project_budget(1024 * 1024);
        assert!(!admits_file(wide));
        assert!(resident_len(wide).is_err());
        assert!(admits_file(100));
        configure_project_budget(0);
        assert!(admits_file(5 * 1024 * 1024 + 1));
    }
    static CASE_ID:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    fn race_case(phase:&'static str, change:impl FnMut(&Path)+'static) {
        let root=std::env::temp_dir().join(format!("rllm-zip-race-{}-{}",std::process::id(),CASE_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed))).join("project");
        fs::create_dir_all(root.join("folder")).unwrap();fs::write(root.join("folder/file.bin"),b"original").unwrap();
        let captured=root.clone();let mut change=change;let mut called=false;
        SOURCE_HOOK.with(|hook|*hook.borrow_mut()=Some(Box::new(move |at|{if at==phase&&!called{called=true;change(&captured);}})));
        let result=collect(&root);SOURCE_HOOK.with(|hook|*hook.borrow_mut()=None);
        assert!(result.is_err(),"mutation at {phase} unexpectedly produced an archive");
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    fn descriptor_collection_rejects_swaps_and_live_mutation() {
        use std::os::unix::fs::symlink;
        race_case("enumerated",|root|{fs::rename(root.join("folder"),root.join("old-folder")).unwrap();symlink(root.join("old-folder"),root.join("folder")).unwrap();});
        race_case("file_opened",|root|{fs::rename(root.join("folder/file.bin"),root.join("old-file.bin")).unwrap();fs::write(root.join("folder/file.bin"),b"replacement").unwrap();});
        race_case("file_opened",|root|{fs::write(root.join("folder/file.bin"),b"altered").unwrap();});
        race_case("before_validation",|root|{fs::write(root.join("folder/new.bin"),b"new").unwrap();});
        race_case("root_opened",|root|{let moved=root.with_extension("moved");fs::rename(root,&moved).unwrap();fs::create_dir_all(root.join("folder")).unwrap();fs::write(root.join("folder/file.bin"),b"replacement").unwrap();});
        race_case("before_validation",|root|{fs::rename(root.join("folder"),root.join("old-folder")).unwrap();fs::create_dir(root.join("folder")).unwrap();fs::write(root.join("folder/file.bin"),b"replacement").unwrap();});
        race_case("root_opened",|root|{let parent=root.parent().unwrap();fs::rename(parent,parent.with_extension("moved")).unwrap();fs::create_dir_all(root.join("folder")).unwrap();fs::write(root.join("folder/file.bin"),b"replacement").unwrap();});
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    fn descriptor_collection_retains_root_aliases_and_empty_directories() {
        use std::os::unix::fs::symlink;
        let base=std::env::temp_dir().join(format!("rllm-zip-alias-{}-{}",std::process::id(),CASE_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        let root=base.join("real");fs::create_dir_all(root.join("empty/deep")).unwrap();fs::write(root.join("binary"),[0,255,128]).unwrap();
        symlink(&root,base.join("alias")).unwrap();
        let a=collect(&root).unwrap();let b=collect(&base.join("alias")).unwrap();assert_eq!(a,b);assert_eq!(unpack(&zip(&a).unwrap()).unwrap(),a);
        let captured=base.clone();let mut changed=false;
        SOURCE_HOOK.with(|hook|*hook.borrow_mut()=Some(Box::new(move |phase|{if phase=="root_opened"&&!changed{changed=true;fs::rename(captured.join("alias"),captured.join("old-alias")).unwrap();fs::create_dir(captured.join("other")).unwrap();symlink(captured.join("other"),captured.join("alias")).unwrap();}})));
        let changed=collect(&base.join("alias"));SOURCE_HOOK.with(|hook|*hook.borrow_mut()=None);assert!(changed.is_err());
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    fn destination_descriptors_refuse_parent_swaps_without_following_links() {
        use std::os::unix::fs::symlink;
        for phase in ["import_preflight","import_parent"] {
            let base=std::env::temp_dir().join(format!("rllm-import-swap-{}-{}",std::process::id(),CASE_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
            let root=base.join("project");fs::create_dir_all(root.join("folder")).unwrap();fs::create_dir_all(base.join("outside")).unwrap();
            let captured=base.clone();let mut changed=false;
            SOURCE_HOOK.with(|hook|*hook.borrow_mut()=Some(Box::new(move |at|{if at==phase&&!changed{changed=true;fs::rename(captured.join("project/folder"),captured.join("held-folder")).unwrap();symlink(captured.join("outside"),captured.join("project/folder")).unwrap();}})));
            let result=source::restore(&root,&[("folder/data".into(),b"payload".to_vec())]);SOURCE_HOOK.with(|hook|*hook.borrow_mut()=None);
            assert!(result.is_err(), "{result:?}");
            assert!(!base.join("outside/data").exists());
            assert!(!base.join("held-folder").join("data").exists(), "moved folder kept the new file");
        }
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    fn destination_descriptors_preserve_aliases_empty_dirs_and_create_only_files() {
        use std::os::unix::fs::symlink;
        let base=std::env::temp_dir().join(format!("rllm-import-dirs-{}-{}",std::process::id(),CASE_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        fs::create_dir_all(base.join("root")).unwrap();symlink(base.join("root"),base.join("alias")).unwrap();
        let entries=vec![("empty/deep/".into(),Vec::new()),("parent/binary".into(),vec![0,255,128])];
        assert_eq!(source::restore(&base.join("alias/new/project"),&entries).unwrap(),(1,3));
        assert_eq!(fs::read(base.join("root/new/project/parent/binary")).unwrap(),vec![0,255,128]);
        assert!(source::restore(&base.join("alias/new/project"),&entries).is_err());
        assert_eq!(fs::read(base.join("root/new/project/parent/binary")).unwrap(),vec![0,255,128]);
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn incremental_crc_matches_known_vector_at_all_boundaries() {
        assert_eq!(crc32(b"123456789"),0xcbf43926);
        let bytes:Vec<u8>=(0..131079).map(|i|(i*37) as u8).collect();
        for size in [1,7,65535,65536,65537,131079] {
            let mut crc=!0;
            for chunk in bytes.chunks(size) {crc=crc_update(crc,chunk);}
            assert_eq!(!crc,crc32(&bytes));
        }
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    fn incremental_source_matches_legacy_zip_and_rejects_chunk_mutation() {
        let root=std::env::temp_dir().join(format!("rllm-zip-chunks-{}-{}",std::process::id(),CASE_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        fs::create_dir_all(root.join("folder/empty")).unwrap();
        let mut entries=vec![("folder/".into(),Vec::new()),("folder/empty/".into(),Vec::new())];
        for (i,size) in [0,65535,65536,65537,1048576].iter().enumerate() {
            let name=format!("folder/{i}.bin");let bytes:Vec<u8>=(0..*size).map(|n|(n*19+i) as u8).collect();
            fs::write(root.join(&name),&bytes).unwrap();entries.push((name,bytes));
        }
        entries.sort_by(|a,b|a.0.cmp(&b.0));
        let (bytes,files,dirs)=source_archive(&root).unwrap();
        assert_eq!((files,dirs),(5,2));assert_eq!(bytes,zip(&entries).unwrap());assert_eq!(unpack(&bytes).unwrap(),entries);
        let captured=root.clone();let mut changed=false;
        SOURCE_HOOK.with(|hook|*hook.borrow_mut()=Some(Box::new(move |phase|{if phase=="file_chunk"&&!changed{changed=true;fs::write(captured.join("folder/4.bin"),b"changed after earlier chunk").unwrap();}})));
        let failed=source_archive(&root);SOURCE_HOOK.with(|hook|*hook.borrow_mut()=None);assert!(failed.is_err());
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn zip64_metadata_is_ordered_bounded_and_unambiguous() {
        let mut extra=vec![1,0,28,0];
        for n in [3u64,5,12] { extra.extend_from_slice(&n.to_le_bytes()); }
        extra.extend_from_slice(&0u32.to_le_bytes());
        let sentinels=[u32::MAX as u64,u32::MAX as u64,u32::MAX as u64,u16::MAX as u64];
        let mut fields=sentinels;
        assert!(zip64_extra(&extra,&mut fields,&sentinels).unwrap());
        assert_eq!(fields,[3,5,12,0]);
        for broken in [extra[..31].to_vec(),[extra.clone(),extra.clone()].concat(),vec![]] {
            assert!(zip64_extra(&broken,&mut sentinels.clone(),&sentinels).is_err());
        }
        assert!(bounded_size(u64::MAX,5*1024*1024).is_err());
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn zip64_footer_requires_consistent_single_disk_directory() {
        let ordinary=zip(&[("note.txt".into(),b"abc".to_vec())]).unwrap();
        let end=ordinary.len()-22;
        let size=read_u32(&ordinary,end+12).unwrap() as u64;
        let offset=read_u32(&ordinary,end+16).unwrap() as u64;
        let mut bytes=ordinary[..end].to_vec();
        long(&mut bytes,0x06064b50);bytes.extend_from_slice(&44u64.to_le_bytes());
        short(&mut bytes,45);short(&mut bytes,45);long(&mut bytes,0);long(&mut bytes,0);
        for n in [1u64,1,size,offset] {bytes.extend_from_slice(&n.to_le_bytes());}
        long(&mut bytes,0x07064b50);long(&mut bytes,0);bytes.extend_from_slice(&(end as u64).to_le_bytes());long(&mut bytes,1);
        bytes.extend_from_slice(&ordinary[end..]);
        let footer=bytes.len()-22;bytes[footer+8..footer+20].fill(255);
        assert_eq!(unpack(&bytes).unwrap(),vec![("note.txt".into(),b"abc".to_vec())]);
        for at in [end+4,end+16,end+24,end+32,end+40,end+48,end+60,end+64,end+72] {
            let mut broken=bytes.clone();broken[at]^=1;assert!(unpack(&broken).is_err(),"offset {at}");
        }
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn compressed_archive_checks_descriptors_and_preserves_imports() {
        // Independently encoded fixed-code raw stream for UTF-8 "abc".
        for descriptor in [0, 12, 16] {
            let mut bytes = zip(&[("source.txt".into(), b"abc".to_vec())]).unwrap();
            let data_at = 30 + "source.txt".len();
            let mut payload = vec![75, 76, 74, 6, 0];
            if descriptor == 16 { long(&mut payload, 0x08074b50); }
            if descriptor != 0 {
                long(&mut payload, crc32(b"abc"));
                long(&mut payload, 5);
                long(&mut payload, 3);
            }
            bytes.splice(data_at..data_at + 3, payload);
            let end = bytes.len() - 22;
            let directory = data_at + 5 + descriptor;
            let flags: u16 = if descriptor == 0 { 0x0800 } else { 0x0808 };
            bytes[6..8].copy_from_slice(&flags.to_le_bytes());
            bytes[8..10].copy_from_slice(&8u16.to_le_bytes());
            bytes[18..22].copy_from_slice(&5u32.to_le_bytes());
            if descriptor != 0 { bytes[14..26].fill(0); }
            bytes[directory + 8..directory + 10].copy_from_slice(&flags.to_le_bytes());
            bytes[directory + 10..directory + 12].copy_from_slice(&8u16.to_le_bytes());
            bytes[directory + 20..directory + 24].copy_from_slice(&5u32.to_le_bytes());
            bytes[end + 16..end + 20].copy_from_slice(&(directory as u32).to_le_bytes());
            let expected = vec![("source.txt".into(), b"abc".to_vec())];
            assert_eq!(unpack(&bytes).unwrap(), expected);
            let input = obj(vec![("archive", s(&network::b64(&bytes)))]);
            assert_eq!(preview(&input).unwrap().get("files"), Ok(&Num("1".into())));
            let mut comment = bytes.clone();
            comment[end + 20..end + 22].copy_from_slice(&4u16.to_le_bytes());
            comment.extend_from_slice(b"note");
            assert_eq!(unpack(&comment).unwrap(), expected);
            let mut damaged = bytes.clone();
            damaged[data_at] ^= 0x10;
            assert!(unpack(&damaged).is_err());
            if descriptor != 0 {
                let mut damaged = bytes.clone();
                damaged[directory - 1] ^= 1;
                assert!(unpack(&damaged).is_err());
            }
            let mut size = bytes.clone();
            size[directory + 24..directory + 28].copy_from_slice(&2u32.to_le_bytes());
            if descriptor == 0 { size[22..26].copy_from_slice(&2u32.to_le_bytes()); }
            assert!(unpack(&size).is_err());
        }
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn import_preflight_preserves_existing_files() {
        let root = std::env::current_dir()
            .unwrap()
            .join("toolchain")
            .join(format!(
                "import-conflict-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("z.txt"), "original").unwrap();
        let archive = zip(&[
            ("a.txt".into(), b"new".to_vec()),
            ("z.txt".into(), b"replace".to_vec()),
        ])
        .unwrap();
        let request = obj(vec![
            ("root", s(root.to_str().unwrap())),
            ("archive", s(&network::b64(&archive))),
        ]);
        assert!(import(&request).is_err());
        assert!(!root.join("a.txt").exists());
        assert_eq!(fs::read(root.join("z.txt")).unwrap(), b"original");
        let mut unsupported = archive.clone();
        let end = unsupported.len() - 22;
        let directory = read_u32(&unsupported, end + 16).unwrap() as usize;
        unsupported[directory + 10] = 8;
        assert!(unpack(&unsupported).is_err());
        let mut link = archive.clone();
        link[directory + 5] = 3;
        let mode = (0o120777u32) << 16;
        link[directory + 38..directory + 42].copy_from_slice(&mode.to_le_bytes());
        assert!(unpack(&link).is_err());
        let mut directory_flag = archive.clone();
        directory_flag[directory + 38] = 0x10;
        assert!(unpack(&directory_flag).is_err());
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn directory_roundtrip_preserves_empty_folders_and_existing_data() {
        let root=std::env::current_dir().unwrap().join("toolchain").join(format!("directory-import-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(root.join("src")).unwrap();fs::write(root.join("original.txt"),b"keep").unwrap();
        let entries=vec![("empty/nested/".into(),Vec::new()),("src/".into(),Vec::new()),("src/new.txt".into(),b"new".to_vec())];
        let bytes=zip(&entries).unwrap();assert_eq!(unpack(&bytes).unwrap(),entries);
        let request=obj(vec![("root",s(root.to_str().unwrap())),("archive",s(&network::b64(&bytes)))]);
        let preview=preview(&request).unwrap();assert_eq!(preview.get("files"),Ok(&Num("1".into())));assert_eq!(preview.get("directories"),Ok(&Num("2".into())));
        let report=import(&request).unwrap();assert_eq!(report.get("files"),Ok(&Num("1".into())));assert_eq!(report.get("directories"),Ok(&Num("2".into())));
        assert!(root.join("empty/nested").is_dir());assert_eq!(fs::read(root.join("src/new.txt")).unwrap(),b"new");assert_eq!(fs::read(root.join("original.txt")).unwrap(),b"keep");
        let exported=collect(&root).unwrap();assert!(exported.iter().any(|(name,_)|name=="empty/nested/"));assert_eq!(unpack(&zip(&exported).unwrap()).unwrap(),exported);
        let directory_only=zip(&[("empty/".into(),Vec::new())]).unwrap();assert!(unpack(&directory_only).is_ok());
        #[cfg(unix)] {std::os::unix::fs::symlink(root.join("src"),root.join("linked")).unwrap();let linked=zip(&[("linked/newer.txt".into(),b"no".to_vec())]).unwrap();let req=obj(vec![("root",s(root.to_str().unwrap())),("archive",s(&network::b64(&linked)))]);assert!(import(&req).is_err());assert!(!root.join("src/newer.txt").exists());}
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn directory_metadata_and_path_conflicts_are_checked() {
        let bytes=zip(&[("folder".into(),Vec::new())]).unwrap();let directory=36;
        let mut dos=bytes.clone();dos[directory+38..directory+42].copy_from_slice(&0x10u32.to_le_bytes());assert_eq!(unpack(&dos).unwrap(),vec![("folder/".into(),Vec::new())]);
        let mut unix=bytes;unix[directory+4..directory+6].copy_from_slice(&0x0314u16.to_le_bytes());unix[directory+38..directory+42].copy_from_slice(&0x41ed0000u32.to_le_bytes());assert_eq!(unpack(&unix).unwrap(),vec![("folder/".into(),Vec::new())]);
        for entries in [vec![("folder/".into(),b"bad".to_vec())],vec![("folder".into(),vec![]),("folder/child".into(),vec![])],vec![("folder/child".into(),vec![]),("folder".into(),vec![])],vec![("folder/".into(),vec![]),("folder".into(),vec![])],vec![("Folder/a".into(),vec![]),("folder/b".into(),vec![])],vec![("../folder/".into(),vec![])],vec![("folder//".into(),vec![])]] {assert!(unpack(&zip(&entries).unwrap()).is_err());}
        let mut conflict=zip(&[("folder/".into(),Vec::new())]).unwrap();let at=37;conflict[at+38..at+42].copy_from_slice(&0x81a40000u32.to_le_bytes());assert!(unpack(&conflict).is_err());
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn archive_captures_project_and_rejects_symlinks() {
        let root = std::env::current_dir()
            .unwrap()
            .join("toolchain")
            .join(format!(
                "archive-check-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.txt"), "hello").unwrap();
        fs::write(root.join("raw.bin"), [0, 255, 1]).unwrap();
        let entries = collect(&root).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().any(|(name,data)|name=="src/"&&data.is_empty()));
        let bytes = zip(&entries).unwrap();
        assert_eq!(&bytes[..4], &0x04034b50u32.to_le_bytes());
        assert_eq!(
            &bytes[bytes.len() - 22..bytes.len() - 18],
            &0x06054b50u32.to_le_bytes()
        );
        assert!(bytes.windows(12).any(|w| w == b"src/main.txt"));
        fs::write(
            root.parent().unwrap().join("archive-verification.zip"),
            &bytes,
        )
        .unwrap();
        let parsed = unpack(&bytes).unwrap();
        assert_eq!(parsed, entries);
        let import_root = root.parent().unwrap().join(format!(
            "archive-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let request = obj(vec![
            ("root", s(import_root.to_str().unwrap())),
            ("archive", s(&network::b64(&bytes))),
        ]);
        assert_eq!(import(&request).unwrap().get("files"), Ok(&Num("2".into())));
        assert_eq!(
            fs::read(import_root.join("src/main.txt")).unwrap(),
            b"hello"
        );
        assert!(import(&request).is_err());
        let mut damaged = bytes.clone();
        damaged[30 + "raw.bin".len()] = 0xff;
        assert!(unpack(&damaged).is_err());
        let unsafe_name = zip(&[("../x.bin".into(), vec![1])]).unwrap();
        assert!(unpack(&unsafe_name).is_err());
        let duplicate = zip(&[("X.txt".into(), vec![1]), ("x.txt".into(), vec![2])]).unwrap();
        assert!(unpack(&duplicate).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("src/main.txt"), root.join("linked")).unwrap();
            assert!(collect(&root).is_err());
        }
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(windows)]
    fn windows_import_writes_stored_zip_and_refuses_escape() {
        super::windows_import_checks();
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(windows)]
    fn windows_save_round_trip_preserves_nested_empty_binary_and_utf8() {
        let base = std::env::current_dir().unwrap().join("output").join(format!(
            "g2-roundtrip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let root = base.join("project");
        fs::create_dir_all(root.join("nested").join("deep")).unwrap();
        fs::create_dir_all(root.join("nested").join("empty")).unwrap();
        let binary = [0u8, 255, 16, 32, 127, 0, 1];
        fs::write(root.join("nested").join("deep").join("payload.bin"), binary).unwrap();
        let text = "projekt Ω";
        fs::write(root.join("café Ω.txt"), text.as_bytes()).unwrap();
        let (bytes, files, directories) = archive(&root).unwrap();
        assert_eq!((files, directories), (2, 3));
        let mut at = 0usize;
        while read_u32(&bytes, at) == Ok(0x04034b50) {
            let flags = read_u16(&bytes, at + 6).unwrap();
            let method = read_u16(&bytes, at + 8).unwrap();
            let crc = read_u32(&bytes, at + 14).unwrap();
            let compressed = read_u32(&bytes, at + 18).unwrap();
            let expanded = read_u32(&bytes, at + 22).unwrap();
            let name_len = read_u16(&bytes, at + 26).unwrap() as usize;
            let extra = read_u16(&bytes, at + 28).unwrap() as usize;
            let name = std::str::from_utf8(&bytes[at + 30..at + 30 + name_len]).unwrap().to_string();
            assert_eq!(method, 0, "{name}");
            assert_eq!(flags & 0x0800, 0x0800, "{name}");
            assert_eq!(compressed, expanded, "{name}");
            let data = &bytes[at + 30 + name_len + extra..at + 30 + name_len + extra + expanded as usize];
            assert_eq!(crc32(data), crc, "{name}");
            if name.ends_with('/') { assert!(data.is_empty(), "{name}"); }
            at += 30 + name_len + extra + expanded as usize;
        }
        assert_eq!(read_u32(&bytes, at), Ok(0x02014b50));
        let expected = vec![
            ("café Ω.txt".into(), text.as_bytes().to_vec()),
            ("nested/".into(), Vec::new()),
            ("nested/deep/".into(), Vec::new()),
            ("nested/deep/payload.bin".into(), binary.to_vec()),
            ("nested/empty/".into(), Vec::new()),
        ];
        assert_eq!(unpack(&bytes).unwrap(), expected);
        let zip_path = std::env::current_dir().unwrap().join("output").join("g2-project-roundtrip.zip");
        fs::create_dir_all(zip_path.parent().unwrap()).unwrap();
        fs::write(&zip_path, &bytes).unwrap();
        let imported = base.join("imported");
        let request = obj(vec![
            ("root", s(imported.to_str().unwrap())),
            ("archive", s(&network::b64(&bytes))),
        ]);
        let report = import(&request).unwrap();
        assert_eq!(report.get("files"), Ok(&Num("2".into())));
        assert_eq!(report.get("directories"), Ok(&Num("3".into())));
        assert_eq!(fs::read(imported.join("nested").join("deep").join("payload.bin")).unwrap(), binary);
        assert_eq!(fs::read(imported.join("café Ω.txt")).unwrap(), text.as_bytes());
        assert!(imported.join("nested").join("empty").is_dir());
        assert!(fs::read_dir(imported.join("nested").join("empty")).unwrap().next().is_none());
        let escaped = zip(&[("..\\escaped.txt".into(), b"no".to_vec())]).unwrap();
        assert!(import(&obj(vec![("root", s(imported.to_str().unwrap())), ("archive", s(&network::b64(&escaped)))])).is_err());
        assert!(!base.join("escaped.txt").exists());
        let absolute = zip(&[("C:/outside.txt".into(), b"no".to_vec())]).unwrap();
        assert!(import(&obj(vec![("root", s(imported.to_str().unwrap())), ("archive", s(&network::b64(&absolute)))])).is_err());
        assert!(!imported.join("outside.txt").exists());
        assert_eq!(fs::read(imported.join("nested").join("deep").join("payload.bin")).unwrap(), binary);
    }
    fn stored_blocks(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = data;
        while !rest.is_empty() {
            let take = rest.len().min(65535);
            let last = take == rest.len();
            out.push(if last { 1 } else { 0 });
            out.extend_from_slice(&(take as u16).to_le_bytes());
            out.extend_from_slice(&(!(take as u16)).to_le_bytes());
            out.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
        out
    }
    fn zip_deflated(name: &str, plain: &[u8], compressed: &[u8]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        let name_bytes = name.as_bytes();
        let length = u16::try_from(name_bytes.len()).map_err(|_| "Filename too long")?;
        let stored = u32::try_from(compressed.len()).map_err(|_| "File too large")?;
        let expanded = u32::try_from(plain.len()).map_err(|_| "File too large")?;
        let offset = u32::try_from(out.len()).map_err(|_| "Archive too large")?;
        let crc = crc32(plain);
        long(&mut out, 0x04034b50);
        short(&mut out, 20);
        short(&mut out, 0x0800);
        short(&mut out, 8);
        short(&mut out, 0);
        short(&mut out, DOS_DATE);
        long(&mut out, crc);
        long(&mut out, stored);
        long(&mut out, expanded);
        short(&mut out, length);
        short(&mut out, 0);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(compressed);
        long(&mut directory, 0x02014b50);
        short(&mut directory, 20);
        short(&mut directory, 20);
        short(&mut directory, 0x0800);
        short(&mut directory, 8);
        short(&mut directory, 0);
        short(&mut directory, DOS_DATE);
        long(&mut directory, crc);
        long(&mut directory, stored);
        long(&mut directory, expanded);
        short(&mut directory, length);
        short(&mut directory, 0);
        short(&mut directory, 0);
        short(&mut directory, 0);
        short(&mut directory, 0);
        long(&mut directory, 0);
        long(&mut directory, offset);
        directory.extend_from_slice(name_bytes);
        let directory_at = u32::try_from(out.len()).map_err(|_| "Archive too large")?;
        let directory_len = u32::try_from(directory.len()).map_err(|_| "Archive too large")?;
        out.extend_from_slice(&directory);
        long(&mut out, 0x06054b50);
        short(&mut out, 0);
        short(&mut out, 0);
        short(&mut out, 1);
        short(&mut out, 1);
        long(&mut out, directory_len);
        long(&mut out, directory_at);
        short(&mut out, 0);
        Ok(out)
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn deflated_members_follow_the_project_budget() {
        let over_member = vec![0x5Au8; 1024 * 1024 + 17];
        let over_compressed = stored_blocks(&over_member);
        assert!(over_member.len() > 1024 * 1024);
        assert!(over_compressed.len() < 5 * 1024 * 1024);
        configure_project_budget(8 * 1024 * 1024);
        let over_zip = zip_deflated("wide.bin", &over_member, &over_compressed).unwrap();
        let over_decoded = unpack(&over_zip);
        configure_project_budget(0);
        assert_eq!(over_decoded.unwrap(), vec![("wide.bin".into(), over_member)]);

        let wide = vec![0x3Cu8; 5 * 1024 * 1024 + 64];
        let wide_compressed = stored_blocks(&wide);
        assert!(wide_compressed.len() > 5 * 1024 * 1024);
        configure_project_budget(16 * 1024 * 1024);
        let wide_zip = zip_deflated("huge.bin", &wide, &wide_compressed).unwrap();
        let wide_decoded = unpack(&wide_zip);
        configure_project_budget(1024 * 1024);
        let refused = unpack(&wide_zip);
        configure_project_budget(0);
        assert_eq!(wide_decoded.unwrap(), vec![("huge.bin".into(), wide)]);
        let error = refused.unwrap_err();
        assert!(error.contains("memory budget"), "{error}");
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn zip64_sentinel_size_is_recorded_at_the_boundary() {
        let exact = u32::MAX as u64;
        let mut local = Vec::new();
        local_header(&mut local, "a.txt", exact).unwrap();
        assert_eq!(read_u16(&local, 4).unwrap(), 45);
        assert_eq!(read_u32(&local, 18).unwrap(), u32::MAX);
        assert_eq!(read_u32(&local, 22).unwrap(), u32::MAX);
        let extra_len = read_u16(&local, 28).unwrap() as usize;
        assert_eq!(extra_len, 20);
        let extra = &local[30 + 5..30 + 5 + extra_len];
        let mut fields = [exact, exact];
        assert!(zip64_extra(extra, &mut fields, &[exact, exact]).unwrap());
        assert_eq!(fields, [exact, exact]);
        let mut missing = [exact, exact];
        assert!(zip64_extra(&[], &mut missing, &[exact, exact]).is_err());

        let mut below = Vec::new();
        local_header(&mut below, "a.txt", exact - 1).unwrap();
        assert_eq!(read_u16(&below, 4).unwrap(), 20);
        assert_eq!(read_u16(&below, 28).unwrap(), 0);
        assert_eq!(read_u32(&below, 18).unwrap(), u32::MAX - 1);

        let mut directory = Vec::new();
        central_header(&mut directory, "a.txt", 1, exact, exact, false).unwrap();
        assert_eq!(directory.len() as u64, 46 + 5 + central_extra_len(exact, exact));
        assert_eq!(read_u32(&directory, 20).unwrap(), u32::MAX);
        assert_eq!(read_u32(&directory, 24).unwrap(), u32::MAX);
        assert_eq!(read_u32(&directory, 42).unwrap(), u32::MAX);
        let central_extra = read_u16(&directory, 30).unwrap() as usize;
        let payload = &directory[46 + 5..46 + 5 + central_extra];
        let mut central_fields = [exact, exact, exact, 0];
        assert!(zip64_extra(payload, &mut central_fields, &[exact, exact, exact, u16::MAX as u64]).unwrap());
        assert_eq!(central_fields, [exact, exact, exact, 0]);

        let mut ordinary = Vec::new();
        central_header(&mut ordinary, "b.txt", 1, 4, exact - 1, false).unwrap();
        assert_eq!(read_u16(&ordinary, 30).unwrap(), 0);
        assert_eq!(read_u32(&ordinary, 42).unwrap(), u32::MAX - 1);
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn export_bounds_peak_memory_and_entry_handles() {
        let base = std::env::current_dir().unwrap()
            .join("loops-lanes").join("G2-refix-archive-proof")
            .join(format!("handles-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let root = base.join("project");
        fs::create_dir_all(&root).unwrap();
        for index in 0..128 {
            fs::write(root.join(format!("f{index:03}.txt")), b"x").unwrap();
        }
        configure_project_budget(0);
        let (bytes, files, directories) = archive(&root).unwrap();
        let peak = entry_handle_peak();
        let open = entry_handles_open();
        assert_eq!((files, directories), (128, 0));
        assert_eq!(open, 0);
        assert!(peak >= 1 && peak <= 8, "entry handle peak {peak}");
        let zip_len = bytes.len() as u64;
        configure_project_budget(zip_len);
        let refused = archive(&root);
        configure_project_budget(zip_len.saturating_add(2 * 1024 * 1024));
        let admitted = archive(&root);
        configure_project_budget(0);
        let error = refused.unwrap_err();
        assert!(error.contains("memory budget"), "{error}");
        admitted.unwrap();
        assert_eq!(unpack(&bytes).unwrap().len(), 128);
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn live_working_set_counts_the_zip_the_copy_and_the_encoding() {
        let root = std::env::current_dir().unwrap().join("loops-lanes/G2-fix3-archive-proof/work");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("blob.bin"), vec![b'q'; 256 * 1024]).unwrap();
        let (bytes, _, _) = archive(&root).unwrap();
        let encoded_len = base64_len(bytes.len()).unwrap();
        configure_project_budget((bytes.len() as u64) + encoded_len - 1);
        let refused = build(&obj(vec![("root", s(root.to_str().unwrap()))]));
        configure_project_budget((bytes.len() as u64) + encoded_len + 2 * 1024 * 1024);
        let admitted = build(&obj(vec![("root", s(root.to_str().unwrap()))]));
        configure_project_budget(1);
        let preview_refused = preview(&obj(vec![("archive", s("AAAA"))]));
        configure_project_budget(0);
        let refused = refused.unwrap_err();
        assert!(refused.contains("memory budget"), "{refused}");
        admitted.unwrap();
        let preview_refused = preview_refused.unwrap_err();
        assert!(preview_refused.contains("memory budget"), "{preview_refused}");
        let stored = zip(&[("n.txt".into(), vec![b'z'; 64 * 1024])]).unwrap();
        configure_project_budget(stored.len() as u64 + 64 * 1024 - 1);
        let early = unpack(&stored);
        configure_project_budget(stored.len() as u64 + 64 * 1024 + 64 * 1024);
        let late = unpack(&stored);
        configure_project_budget(0);
        let early = early.unwrap_err();
        assert!(early.contains("memory budget"), "{early}");
        assert_eq!(late.unwrap()[0].1.len(), 64 * 1024);
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn deflate_bit_cursor_accepts_the_32_bit_boundary() {
        let len = 512 * 1024 * 1024usize;
        assert!((len as u32).checked_mul(8).is_none());
        let bits = (len as u64).checked_mul(8).unwrap();
        assert!(bit_end_fits(0, 1, len));
        assert!(bit_end_fits(bits - 1, 1, len));
        assert!(!bit_end_fits(bits - 1, 2, len));
        assert!(!bit_end_fits(bits, 1, len));
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn flat_project_round_trips_past_the_old_prefix_cap() {
        let entries: Vec<(String, Vec<u8>)> = (0..4097).map(|i| (format!("f{i:04}.txt"), vec![b'a'])).collect();
        let bytes = zip(&entries).unwrap();
        let got = unpack(&bytes).unwrap();
        assert_eq!(got.len(), 4097);
        assert_eq!(got[0].0, "f0000.txt");
        assert_eq!(got[4096].0, "f4096.txt");
        assert!(prefix_limit() > 4096);
    }
    fn pack_member(name: &[u8], method: u16, stored: &[u8], plain: &[u8], extra: &[u8]) -> Vec<u8> {
        let crc = crc32(plain);
        let mut out = Vec::new();
        let mut dir = Vec::new();
        long(&mut out, 0x04034b50);
        short(&mut out, 20);
        short(&mut out, 0);
        short(&mut out, method);
        short(&mut out, 0);
        short(&mut out, DOS_DATE);
        long(&mut out, crc);
        long(&mut out, stored.len() as u32);
        long(&mut out, plain.len() as u32);
        short(&mut out, name.len() as u16);
        short(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(stored);
        long(&mut dir, 0x02014b50);
        short(&mut dir, 20);
        short(&mut dir, 20);
        short(&mut dir, 0);
        short(&mut dir, method);
        short(&mut dir, 0);
        short(&mut dir, DOS_DATE);
        long(&mut dir, crc);
        long(&mut dir, stored.len() as u32);
        long(&mut dir, plain.len() as u32);
        short(&mut dir, name.len() as u16);
        short(&mut dir, extra.len() as u16);
        short(&mut dir, 0);
        short(&mut dir, 0);
        short(&mut dir, 0);
        long(&mut dir, 0);
        long(&mut dir, 0);
        dir.extend_from_slice(name);
        dir.extend_from_slice(extra);
        let dir_off = out.len() as u32;
        let dir_len = dir.len() as u32;
        out.extend_from_slice(&dir);
        long(&mut out, 0x06054b50);
        short(&mut out, 0);
        short(&mut out, 0);
        short(&mut out, 1);
        short(&mut out, 1);
        long(&mut out, dir_len);
        long(&mut out, dir_off);
        short(&mut out, 0);
        out
    }
    fn unicode_extra(header: &[u8], utf8: &str) -> Vec<u8> {
        let mut extra = Vec::new();
        short(&mut extra, 0x7075);
        short(&mut extra, (5 + utf8.len()) as u16);
        extra.push(1);
        extra.extend_from_slice(&crc32(header).to_le_bytes());
        extra.extend_from_slice(utf8.as_bytes());
        extra
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn zip_reads_bzip2_lzma_zstandard_and_unicode_paths() {
        assert_eq!(zstd_member::xxh64(b""), 0xEF46DB3751D8E999);
        zstd_member::tables_fit().unwrap();
        for sample in [b"".as_slice(), b"abc", b"aaaa", b"Hello, world"] {
            let compressed = bzip2_member::encode(sample).unwrap();
            assert_eq!(bzip2_member::decode(&compressed, sample.len()).unwrap(), sample);
            let packed = pack_member(b"note.txt", 12, &compressed, sample, &[]);
            assert_eq!(unpack(&packed).unwrap(), vec![("note.txt".into(), sample.to_vec())]);
        }
        let hi = lzma_member::encode_literals(b"Hi");
        assert_eq!(lzma_member::decode(&hi, 2).unwrap(), b"Hi");
        assert_eq!(unpack(&pack_member(b"hi.txt", 14, &hi, b"Hi", &[])).unwrap()[0].1, b"Hi");
        let run = lzma_member::encode_repeat(b'a', 4);
        assert_eq!(lzma_member::decode(&run, 4).unwrap(), b"aaaa");
        let raw = [0x28, 0xb5, 0x2f, 0xfd, 0x20, 0x03, 0x19, 0x00, 0x00, b'a', b'b', b'c'];
        assert_eq!(zstd_member::decode(&raw, 3).unwrap(), b"abc");
        let rle = [0x28, 0xb5, 0x2f, 0xfd, 0x20, 0x04, 0x23, 0x00, 0x00, b'a'];
        assert_eq!(zstd_member::decode(&rle, 4).unwrap(), b"aaaa");
        let seq = [0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x45, 0x00, 0x00, 0x08, b'a', 0x01, 0x54, 0x01, 0x00, 0x00, 0x01];
        assert_eq!(zstd_member::decode(&seq, 4).unwrap(), b"aaaa");
        assert_eq!(unpack(&pack_member(b"z.txt", 93, &raw, b"abc", &[])).unwrap()[0].1, b"abc");
        let header = b"caf\x82.txt";
        let extra = unicode_extra(header, "caf\u{e9}.txt");
        let packed = pack_member(header, 0, b"ok", b"ok", &extra);
        assert_eq!(unpack(&packed).unwrap()[0].0, "caf\u{e9}.txt");
        let mut stale = unicode_extra(b"plain.txt", "other.txt");
        stale[5] ^= 0xff;
        assert_eq!(unpack(&pack_member(b"plain.txt", 0, b"ok", b"ok", &stale)).unwrap()[0].0, "plain.txt");
        let mut duplicated = extra.clone();
        duplicated.extend_from_slice(&extra);
        assert!(unpack(&pack_member(header, 0, b"ok", b"ok", &duplicated)).unwrap_err().contains("Duplicate"));
        assert_eq!(unpack(&pack_member(header, 0, b"ok", b"ok", &[])).unwrap()[0].0, "caf\u{e9}.txt");
        assert!(unpack(&pack_member(b"no.txt", 99, b"ok", b"ok", &[])).unwrap_err().contains("unsupported"));
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn build_moves_the_encoding_without_a_second_copy() {
        struct Reset;
        impl Drop for Reset { fn drop(&mut self) { configure_project_budget(0); } }
        let _reset = Reset;
        let root = std::env::current_dir().unwrap().join("loops-lanes/G2-fix4-archive-proof/encode");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("blob.bin"), vec![b'q'; 64 * 1024]).unwrap();
        let (bytes, _, _) = archive(&root).unwrap();
        let encoded_len = base64_len(bytes.len()).unwrap();
        let request = obj(vec![("root", s(root.to_str().unwrap()))]);
        configure_project_budget((bytes.len() as u64) + encoded_len - 1);
        let refused = build(&request).unwrap_err();
        assert!(refused.contains("memory budget"), "{refused}");
        configure_project_budget((bytes.len() as u64) + encoded_len);
        let made = build(&request).unwrap();
        let text = made.get("archive").unwrap().text().unwrap();
        assert_eq!(text.len() as u64, encoded_len);
        assert_eq!(text, network::b64(&bytes));
        configure_project_budget(0);
    }
    fn flag_name(mut packed: Vec<u8>, flag: u16) -> Vec<u8> {
        packed[6..8].copy_from_slice(&flag.to_le_bytes());
        let end = packed.len() - 22;
        let directory = u32::from_le_bytes(packed[end + 16..end + 20].try_into().unwrap()) as usize;
        packed[directory + 8..directory + 10].copy_from_slice(&flag.to_le_bytes());
        packed
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn legacy_zip_names_use_the_original_byte_page() {
        let header = b"caf\x82.txt";
        let legacy = unpack(&pack_member(header, 0, b"ok", b"ok", &[])).unwrap();
        assert_eq!(legacy[0].0, "caf\u{e9}.txt");
        assert_eq!(legacy[0].1, b"ok");
        let marked = flag_name(pack_member(header, 0, b"ok", b"ok", &[]), 0x0800);
        let utf8_error = unpack(&marked).unwrap_err();
        assert!(utf8_error.contains("UTF-8"), "{utf8_error}");
        let utf8_name = b"caf\xC3\xA9.txt";
        let as_utf8 = unpack(&flag_name(pack_member(utf8_name, 0, b"ok", b"ok", &[]), 0x0800)).unwrap();
        assert_eq!(as_utf8[0].0, "caf\u{e9}.txt");
        let as_legacy = unpack(&pack_member(utf8_name, 0, b"ok", b"ok", &[])).unwrap();
        assert_eq!(as_legacy[0].0, "caf\u{251c}\u{2310}.txt");
        let nested = unpack(&pack_member(b"dir/caf\x82.txt", 0, b"z", b"z", &[])).unwrap();
        assert_eq!(nested[0].0, "dir/caf\u{e9}.txt");
        for hostile in [b"..".as_slice(), b"a/../b", b"a\\b", b"/abs", b"a\x01b", b"caf\x82/../x"] {
            let error = unpack(&pack_member(hostile, 0, b"z", b"z", &[])).unwrap_err();
            assert!(error.contains("Unsafe") || error.contains("UTF-8"), "{hostile:?} {error}");
        }
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn method_93_reads_explicit_tables_treeless_literals_and_weight_streams() {
        let direct = [
            0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x3d, 0x00, 0x00, 0x22, 0xc0, 0x00, 0x81, 0x10, 0x06, 0x00,
        ];
        assert_eq!(zstd_member::decode(&direct, 2).unwrap(), [0, 1]);
        let weights = [
            0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00,
            0x5c, 0x00, 0x00,
            0x12, 0xc0, 0x01, 0x05, 0x10, 0x88, 0x1f, 0x0c, 0x08, 0x02, 0x00,
            0x2d, 0x00, 0x00,
            0x13, 0x40, 0x00, 0x02, 0x00,
        ];
        assert_eq!(zstd_member::decode(&weights, 2).unwrap(), [3, 3]);
        let explicit = [
            0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x4d, 0x00, 0x00, 0x08, b'a', 0x01, 0x64, 0x01, 0x10, 0x3f, 0x00, 0x40,
        ];
        assert_eq!(zstd_member::decode(&explicit, 4).unwrap(), b"aaaa");
        let treeless_first = [
            0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x2d, 0x00, 0x00, 0x13, 0x40, 0x00, 0x02, 0x00,
        ];
        let missing = zstd_member::decode(&treeless_first, 1).unwrap_err();
        assert!(missing.contains("previous"), "{missing}");
        assert!(!missing.contains("decoded"), "{missing}");
        let wide_table = [
            0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x45, 0x00, 0x00, 0x08, b'a', 0x01, 0x64, 0x01, 0x04, 0x00,
        ];
        let wide = zstd_member::decode(&wide_table, 4).unwrap_err();
        assert!(wide.contains("table") || wide.contains("Truncated"), "{wide}");
        let truncated_weights = [
            0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x39, 0x00, 0x00, 0x12, 0xc0, 0x01, 0x05, 0x10, 0x00,
        ];
        assert!(zstd_member::decode(&truncated_weights, 1).is_err());
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn cancelled_unpack_is_not_reported_complete() {
        struct Clear;
        impl Drop for Clear { fn drop(&mut self) { crate::execution::reset(); } }
        let _clear = Clear;
        crate::execution::cancel();
        let err = unpack(&zip(&[("a.txt".into(), b"hi".to_vec())]).unwrap()).unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
        assert!(!err.contains("decoded") && !err.contains("saved"), "{err}");
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn zip64_exact_sentinels_round_trip() {
        assert!(zip64_footer(u16::MAX as u64, 1, 1));
        assert!(zip64_footer(1, u32::MAX as u64, 1));
        assert!(zip64_footer(1, 1, u32::MAX as u64));
        assert!(!zip64_footer((u16::MAX as u64) - 1, (u32::MAX as u64) - 1, (u32::MAX as u64) - 1));
        let ordinary = zip(&[("note.txt".into(), b"abc".to_vec())]).unwrap();
        let end = ordinary.len() - 22;
        for (at, len) in [(8usize, 2usize), (10, 2), (12, 4), (16, 4)] {
            let mut broken = ordinary.clone();
            for byte in &mut broken[end + at..end + at + len] { *byte = 0xff; }
            let err = unpack(&broken).unwrap_err();
            assert!(err.contains("ZIP64"), "{at} {err}");
            assert!(!err.contains("decoded"), "{err}");
        }
        struct Reset;
        impl Drop for Reset { fn drop(&mut self) { configure_project_budget(0); } }
        let _reset = Reset;
        configure_project_budget(256 * 1024 * 1024);
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for index in 0..65535u32 {
            let name = format!("{index:05}");
            let header = local_header(&mut out, &name, 0).unwrap();
            central_header(&mut directory, &name, 0, 0, header.offset, false).unwrap();
        }
        let directory_at = out.len() as u64;
        out.extend_from_slice(&directory);
        finish_archive(&mut out, directory_at, directory.len() as u64, 65535).unwrap();
        assert!(out.windows(4).any(|word| word == 0x06064b50u32.to_le_bytes()));
        let footer = out.len() - 22;
        assert_eq!(u16::from_le_bytes(out[footer + 10..footer + 12].try_into().unwrap()), u16::MAX);
        let entries = unpack(&out).unwrap();
        assert_eq!(entries.len(), 65535);
        assert_eq!(entries[0].0, "00000");
        assert_eq!(entries[65534].0, "65534");
        assert!(entries.iter().all(|(_, data)| data.is_empty()));
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn encoded_archive_rejects_a_suffix_after_padding() {
        let bytes = zip(&[("a.txt".into(), b"hi".to_vec())]).unwrap();
        let text = network::b64(&bytes);
        assert_eq!(preview(&obj(vec![("archive", s(&text))])).unwrap().get("files"), Ok(&Num("1".into())));
        let unpadded = text.trim_end_matches('=').to_string();
        if unpadded != text {
            assert_eq!(preview(&obj(vec![("archive", s(&unpadded))])).unwrap().get("files"), Ok(&Num("1".into())));
        }
        for hostile in [format!("{text}=AA"), format!("{text}===="), format!("{text}=")] {
            let err = preview(&obj(vec![("archive", s(&hostile))])).unwrap_err();
            assert!(err.contains("Invalid") || err.contains("encoded"), "{hostile} {err}");
            assert!(!err.contains("decoded"), "{err}");
        }
    }
    fn pack_low_bits(bits: &[u8]) -> Vec<u8> {
        let mut padded = bits.to_vec();
        while padded.len() % 8 != 0 { padded.push(0); }
        padded.chunks(8).map(|chunk| chunk.iter().enumerate().fold(0u8, |acc, (index, bit)| acc | (bit << index))).collect()
    }
    fn emit_bits(bits: &mut Vec<u8>, value: usize, width: usize, high_first: bool) {
        if high_first {
            for shift in (0..width).rev() { bits.push(((value >> shift) & 1) as u8); }
        } else {
            for shift in 0..width { bits.push(((value >> shift) & 1) as u8); }
        }
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn legacy_methods_expand_and_wide_window_reaches_64k() {
        legacy::shannon_matches_example().unwrap();
        let shrunk = pack_low_bits(&{
            let mut bits = Vec::new();
            for code in [97usize, 98] {
                for shift in 0..9 { bits.push(((code >> shift) & 1) as u8); }
            }
            bits
        });
        assert_eq!(legacy::shrink(&shrunk, 2).unwrap(), b"ab");
        assert_eq!(unpack(&pack_member(b"s.txt", 1, &shrunk, b"ab", &[])).unwrap()[0].1, b"ab");
        let bad = pack_low_bits(&{
            let mut bits = Vec::new();
            for code in [256usize, 9] {
                for shift in 0..9 { bits.push(((code >> shift) & 1) as u8); }
            }
            bits
        });
        let err = legacy::shrink(&bad, 1).unwrap_err();
        assert!(err.contains("control") || err.contains("Truncated") || err.contains("undefined"), "{err}");
        assert!(!err.contains("decoded"), "{err}");
        let mut reduced = vec![0u8; 192];
        reduced.extend_from_slice(b"Hi");
        assert_eq!(legacy::reduce(2, &reduced, 2).unwrap(), b"Hi");
        for method in [2u16, 3, 4, 5] {
            assert_eq!(unpack(&pack_member(b"r.txt", method, &reduced, b"Hi", &[])).unwrap()[0].1, b"Hi");
        }
        let mut copy = vec![0u8; 192];
        copy.extend_from_slice(&[97, 144, 1, 0]);
        assert_eq!(legacy::reduce(2, &copy, 5).unwrap(), b"aaaaa");
        let mut escaped = vec![0u8; 192];
        escaped.extend_from_slice(&[144, 0]);
        assert_eq!(legacy::reduce(2, &escaped, 1).unwrap(), &[144]);
        assert!(legacy::reduce(2, &[33, 0, 0], 1).unwrap_err().contains("follower"));
        let mut trees = Vec::new();
        for _ in 0..2 {
            trees.push(3);
            trees.extend(std::iter::repeat(0xF7).take(4));
        }
        let mut literal_bits = vec![1u8];
        for shift in 0..8 { literal_bits.push((b'A' >> shift) & 1); }
        trees.extend(pack_low_bits(&literal_bits));
        assert_eq!(legacy::implode(0, &trees, 1).unwrap(), b"A");
        assert_eq!(unpack(&pack_member(b"i.txt", 6, &trees, b"A", &[])).unwrap()[0].1, b"A");
        let err = unpack(&pack_member(b"t.txt", 7, b"no", b"no", &[])).unwrap_err();
        assert!(err.contains("unsupported"), "{err}");
        assert!(!err.contains("decoded"), "{err}");
        let stored = [0x01u8, 0x01, 0x00, 0xFE, 0xFF, b'Z'];
        assert_eq!(raw_deflate::decode_wide(&stored, 1).unwrap(), b"Z");
        assert_eq!(unpack(&pack_member(b"w.txt", 9, &stored, b"Z", &[])).unwrap()[0].1, b"Z");
        let mut wide = vec![0x00, 0x01, 0x80, 0xFE, 0x7F];
        wide.extend(std::iter::repeat(b'a').take(32769));
        let mut distance_bits = Vec::new();
        emit_bits(&mut distance_bits, 1, 1, false);
        emit_bits(&mut distance_bits, 1, 2, false);
        emit_bits(&mut distance_bits, 1, 7, true);
        emit_bits(&mut distance_bits, 30, 5, true);
        emit_bits(&mut distance_bits, 0, 14, false);
        emit_bits(&mut distance_bits, 0, 7, true);
        wide.extend(pack_low_bits(&distance_bits));
        let decoded = raw_deflate::decode_wide(&wide, 32772).unwrap();
        assert_eq!(decoded.len(), 32772);
        assert!(decoded.iter().all(|byte| *byte == b'a'));
        assert!(raw_deflate::decode(&wide, 32772).is_err());
        let mut long = vec![0x00, 0x01, 0x00, 0xFE, 0xFF, b'a'];
        let mut length_bits = Vec::new();
        emit_bits(&mut length_bits, 1, 1, false);
        emit_bits(&mut length_bits, 1, 2, false);
        emit_bits(&mut length_bits, 0xC5, 8, true);
        emit_bits(&mut length_bits, 0, 16, false);
        emit_bits(&mut length_bits, 0, 5, true);
        emit_bits(&mut length_bits, 0, 7, true);
        long.extend(pack_low_bits(&length_bits));
        assert_eq!(raw_deflate::decode_wide(&long, 4).unwrap(), vec![b'a'; 4]);
        let plain = [0x28u8, 0xb5, 0x2f, 0xfd, 0x20, 0x03, 0x19, 0x00, 0x00, b'a', b'b', b'c'];
        assert_eq!(unpack(&pack_member(b"o.txt", 20, &plain, b"abc", &[])).unwrap()[0].1, b"abc");
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn reduced_member_past_the_window_keeps_its_crc() {
        // Method 2 copies from at most 512 bytes back. This member is longer than that window.
        let head: Vec<u8> = (0..512).map(|i| b'A' + (i % 26) as u8).collect();
        let mut plain = head.clone();
        plain.extend_from_slice(&head[..10]);
        assert!(plain.len() > 512);
        let mut stored = vec![0u8; 192];
        stored.extend_from_slice(&head);
        stored.extend_from_slice(&[144, 135, 255]);
        assert_eq!(legacy::reduce(2, &stored, plain.len()).unwrap(), plain);
        let archive = pack_member(b"r.txt", 2, &stored, &plain, &[]);
        let got = unpack(&archive).unwrap();
        assert_eq!(got[0].1, plain);
        assert_eq!(crc32(&got[0].1), crc32(&plain));
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn wide_length_code_285_decodes_a_long_match() {
        let extra = 1000usize;
        let expected = 1 + 3 + extra;
        let mut bytes = vec![0x00, 0x01, 0x00, 0xFE, 0xFF, b'a'];
        let mut bits = Vec::new();
        emit_bits(&mut bits, 1, 1, false);
        emit_bits(&mut bits, 1, 2, false);
        emit_bits(&mut bits, 0xC5, 8, true);
        emit_bits(&mut bits, extra, 16, false);
        emit_bits(&mut bits, 0, 5, true);
        emit_bits(&mut bits, 0, 7, true);
        bytes.extend(pack_low_bits(&bits));
        let plain = vec![b'a'; expected];
        assert_eq!(raw_deflate::decode_wide(&bytes, expected).unwrap(), plain);
        let archive = pack_member(b"w285.txt", 9, &bytes, &plain, &[]);
        let got = unpack(&archive).unwrap();
        assert_eq!(got[0].1, plain);
        assert_eq!(crc32(&got[0].1), crc32(&plain));
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn method_93_keeps_a_dictionary_identifier() {
        let raw = [0x28u8, 0xb5, 0x2f, 0xfd, 0x21, 0x01, 0x03, 0x19, 0x00, 0x00, b'a', b'b', b'c'];
        assert_eq!(zstd_member::decode(&raw, 3).unwrap(), b"abc");
        let mut extra = Vec::new();
        short(&mut extra, 0x5A64);
        short(&mut extra, 8);
        extra.extend_from_slice(&1u32.to_le_bytes());
        extra.extend_from_slice(b"wxyz");
        assert_eq!(unpack(&pack_member(b"z.txt", 93, &raw, b"abc", &extra)).unwrap()[0].1, b"abc");
        let mut wrong = extra.clone();
        wrong[4] = 2;
        let err = unpack(&pack_member(b"z.txt", 93, &raw, b"abc", &wrong)).unwrap_err();
        assert!(err.contains("dictionary"), "{err}");
        assert!(!err.contains("decoded"), "{err}");
        let missing = zstd_member::history_probe(&[], 4, 4).unwrap_err();
        assert!(missing.contains("dictionary"), "{missing}");
        assert!(!missing.contains("decoded"), "{missing}");
        assert_eq!(zstd_member::history_probe(b"abcd", 4, 4).unwrap(), b"abcd");
        let mut formatted = 1u32.to_le_bytes().to_vec();
        formatted.extend_from_slice(&[0x37, 0xA4, 0x30, 0xEC]);
        let err = zstd_member::decode_with(&raw, 3, &formatted).unwrap_err();
        assert!(err.contains("dictionary"), "{err}");
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(all(any(target_os = "linux", target_os = "android"), target_pointer_width = "64"))]
    fn rollback_keeps_a_replacement_at_the_new_name() {
        struct Clear;
        impl Drop for Clear { fn drop(&mut self) { SOURCE_HOOK.with(|hook| *hook.borrow_mut() = None); } }
        let _clear = Clear;
        let base = std::path::PathBuf::from("loops-lanes/G2-fix5-archive-proof/tmp").join(format!(
            "rollback-{}-{}",
            std::process::id(),
            CASE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let root = base.join("project");
        fs::create_dir_all(&root).unwrap();
        let captured = root.clone();
        SOURCE_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(move |at| {
            if at == "deliver_verify" {
                let path = captured.join("data");
                fs::rename(&path, captured.join("created")).unwrap();
                fs::write(&path, b"replacement-keep").unwrap();
            }
        })));
        let result = source::deliver(&root, "data", Some(b"payload"));
        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read(root.join("data")).unwrap(), b"replacement-keep");
        let import_root = base.join("import");
        fs::create_dir_all(&import_root).unwrap();
        let captured = import_root.clone();
        SOURCE_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(move |at| {
            if at == "import_verify" {
                let path = captured.join("note.txt");
                fs::rename(&path, captured.join("created")).unwrap();
                fs::write(&path, b"replacement-keep").unwrap();
            }
        })));
        let result = source::restore(&import_root, &[("note.txt".into(), b"payload".to_vec())]);
        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read(import_root.join("note.txt")).unwrap(), b"replacement-keep");
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    #[cfg(all(any(target_os = "linux", target_os = "android"), target_pointer_width = "64"))]
    fn rollback_removes_the_created_file_and_leaves_a_swapped_name() {
        struct Clear;
        impl Drop for Clear { fn drop(&mut self) { SOURCE_HOOK.with(|hook| *hook.borrow_mut() = None); } }
        let _clear = Clear;
        let base = std::path::PathBuf::from("loops-lanes/G2-fix7-archive-proof/tmp").join(format!(
            "rollback-checked-{}-{}",
            std::process::id(),
            CASE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let plain_root = base.join("plain");
        fs::create_dir_all(&plain_root).unwrap();
        let captured = plain_root.clone();
        SOURCE_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(move |at| {
            if at == "deliver_verify" {
                let mut extra = fs::OpenOptions::new().append(true).open(captured.join("data")).unwrap();
                std::io::Write::write_all(&mut extra, b"!").unwrap();
            }
        })));
        let result = source::deliver(&plain_root, "data", Some(b"payload"));
        assert!(result.is_err(), "{result:?}");
        assert!(!plain_root.join("data").exists(), "rollback left the file it created");
        let swapped = base.join("swapped");
        fs::create_dir_all(&swapped).unwrap();
        let captured = swapped.clone();
        SOURCE_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(move |at| {
            if at == "deliver_verify" {
                let mut extra = fs::OpenOptions::new().append(true).open(captured.join("data")).unwrap();
                std::io::Write::write_all(&mut extra, b"!").unwrap();
            }
            if at == "rollback_unlink" {
                let path = captured.join("data");
                fs::rename(&path, captured.join("created")).unwrap();
                fs::write(&path, b"replacement-keep").unwrap();
            }
        })));
        let result = source::deliver(&swapped, "data", Some(b"payload"));
        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read(swapped.join("data")).unwrap(), b"replacement-keep");
        assert!(!swapped.join("created").exists(), "rollback left the file it created");
        let import_root = base.join("import");
        fs::create_dir_all(&import_root).unwrap();
        let captured = import_root.clone();
        SOURCE_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(move |at| {
            if at == "import_verify" {
                let mut extra = fs::OpenOptions::new().append(true).open(captured.join("note.txt")).unwrap();
                std::io::Write::write_all(&mut extra, b"!").unwrap();
            }
            if at == "rollback_unlink" {
                let path = captured.join("note.txt");
                fs::rename(&path, captured.join("created")).unwrap();
                fs::write(&path, b"replacement-keep").unwrap();
            }
        })));
        let result = source::restore(&import_root, &[("note.txt".into(), b"payload".to_vec())]);
        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read(import_root.join("note.txt")).unwrap(), b"replacement-keep");
        assert!(!import_root.join("created").exists(), "rollback left the file it created");
    }
    #[cfg(all(any(target_os = "linux", target_os = "android"), target_pointer_width = "64"))]
    fn deliver_rename_gap_does_not_leave_a_file_outside_the_selected_folder() {
        let base = std::env::temp_dir().join(format!("rllm-deliver-gap-{}-{}", std::process::id(), CASE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let root = base.join("project");
        fs::create_dir_all(&root).unwrap();
        let captured = base.clone();
        SOURCE_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(move |at| {
            if at == "deliver_create" { fs::rename(captured.join("project"), captured.join("moved")).unwrap(); }
        })));
        let result = source::deliver(&root, "data", Some(b"payload"));
        SOURCE_HOOK.with(|hook| *hook.borrow_mut() = None);
        assert!(result.is_err(), "{result:?}");
        assert!(!base.join("moved").join("data").exists(), "renamed directory kept the new file");
        assert!(!root.join("data").exists());
    }
}
fn read_u16(bytes: &[u8], at: usize) -> Result<u16> {
    let end = at.checked_add(2).ok_or("Archive offset overflow")?;
    Ok(u16::from_le_bytes(
        bytes
            .get(at..end)
            .ok_or("Truncated archive")?
            .try_into()
            .unwrap(),
    ))
}
fn read_u32(bytes: &[u8], at: usize) -> Result<u32> {
    let end = at.checked_add(4).ok_or("Archive offset overflow")?;
    Ok(u32::from_le_bytes(
        bytes
            .get(at..end)
            .ok_or("Truncated archive")?
            .try_into()
            .unwrap(),
    ))
}
fn read_u64(bytes: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(span(bytes, at, 8)?.try_into().unwrap()))
}
fn bounded_size(value: u64, limit: u64) -> Result<usize> {
    if value > limit { return Err("ZIP64 value exceeds project limit".into()); }
    usize::try_from(value).map_err(|_| "ZIP64 value exceeds project limit".into())
}
// Only sentinel fields are replaced, in the defined order. A real size equal to
// the sentinel remains valid once the extra field has supplied it. Unknown extra
// fields stay inert; every field must fit its envelope.
fn zip64_extra(extra: &[u8], fields: &mut [u64], sentinels: &[u64]) -> Result<bool> {
    if fields.len() != sentinels.len() || fields.len() > 4 {
        return Err("Unexpected ZIP64 metadata fields".into());
    }
    let mut required = [false; 4];
    for (index, (value, sentinel)) in fields.iter().zip(sentinels).enumerate() {
        required[index] = value == sentinel;
    }
    let mut cursor = 0;
    let mut found = false;
    while cursor < extra.len() {
        let tag = read_u16(extra, cursor)?;
        let length = read_u16(extra, idx(cursor, 2)?)? as usize;
        let payload = span(extra, idx(cursor, 4)?, length)?;
        if tag == 1 {
            if found { return Err("Duplicate ZIP64 metadata".into()); }
            found = true;
            let mut at = 0;
            for (index, field) in fields.iter_mut().enumerate() {
                if required[index] {
                    if sentinels[index] == u16::MAX as u64 {
                        *field = read_u32(payload, at)? as u64;
                        at += 4;
                    } else {
                        *field = read_u64(payload, at)?;
                        at += 8;
                    }
                }
            }
            if at != payload.len() { return Err("Unexpected ZIP64 metadata fields".into()); }
        }
        cursor = cursor.checked_add(4).and_then(|n| n.checked_add(length)).ok_or("ZIP extra field overflow")?;
    }
    if required[..fields.len()].iter().any(|needed| *needed) && !found {
        return Err("Required ZIP64 metadata is missing".into());
    }
    Ok(found)
}
fn directory_bounds(bytes: &[u8], end: usize) -> Result<(usize, usize, usize)> {
    let disk_count = read_u16(bytes, end + 8)? as u64;
    let total_count = read_u16(bytes, end + 10)? as u64;
    let size32 = read_u32(bytes, end + 12)? as u64;
    let offset32 = read_u32(bytes, end + 16)? as u64;
    let locator = end.checked_sub(20).filter(|&at| read_u32(bytes, at) == Ok(0x07064b50));
    let (count, size, offset, boundary) = if let Some(locator) = locator {
        if read_u32(bytes, locator + 4)? != 0 || read_u32(bytes, locator + 16)? != 1 {
            return Err("Multi-disk ZIP64 is unsupported".into());
        }
        let record = bounded_size(read_u64(bytes, locator + 8)?, locator as u64)?;
        if read_u32(bytes, record)? != 0x06064b50 || read_u64(bytes, record + 4)? != 44
            || record.checked_add(56) != Some(locator) || read_u16(bytes, record + 14)? > 45
            || read_u32(bytes, record + 16)? != 0 || read_u32(bytes, record + 20)? != 0 {
            return Err("Unsupported or invalid ZIP64 footer".into());
        }
        let count = read_u64(bytes, record + 32)?;
        let size = read_u64(bytes, record + 40)?;
        let offset = read_u64(bytes, record + 48)?;
        if read_u64(bytes, record + 24)? != count
            || [disk_count,total_count].iter().any(|&n| n != u16::MAX as u64 && n != count)
            || size32 != u32::MAX as u64 && size32 != size
            || offset32 != u32::MAX as u64 && offset32 != offset {
            return Err("ZIP64 footer conflicts with ZIP directory".into());
        }
        (count,size,offset,record)
    } else {
        if disk_count == u16::MAX as u64 || total_count == u16::MAX as u64
            || size32 == u32::MAX as u64 || offset32 == u32::MAX as u64 {
            return Err("ZIP64 sentinel is missing its ZIP64 footer".into());
        }
        if disk_count != total_count { return Err("Invalid ZIP entry count".into()); }
        (total_count,size32,offset32,end)
    };
    let count = bounded_size(count, entry_limit())?;
    let size = bounded_size(size, bytes.len() as u64)?;
    let offset = bounded_size(offset, bytes.len() as u64)?;
    if count == 0 || offset.checked_add(size) != Some(boundary) {
        return Err("Invalid ZIP directory bounds".into());
    }
    Ok((count,offset,boundary))
}
fn span<'a>(bytes: &'a [u8], at: usize, len: usize) -> Result<&'a [u8]> {
    let end = at.checked_add(len).ok_or("Archive offset overflow")?;
    bytes.get(at..end).ok_or("Truncated archive".into())
}
fn accept_path(name: &str) -> Result<String> {
    if name.is_empty()
        || name.len() > 1024
        || name.split('/').count()>64
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains(':')
        || name.chars().any(char::is_control)
        || name
            .split('/')
            .any(|part| part.is_empty() || part.len()>255 || part == "." || part == "..")
    {
        return Err("Unsafe project path".into());
    }
    Ok(name.into())
}
fn valid_name(bytes: &[u8]) -> Result<String> {
    let name = std::str::from_utf8(bytes).map_err(|_| "Project path is not UTF-8")?;
    accept_path(name)
}
/// Bytes 0x00-0x7F stay as themselves. Bytes 0x80-0xFF use the historical ZIP page.
fn legacy_zip_chars(bytes: &[u8]) -> String {
    const PAGE: [char; 128] = [
        '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}',
        '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}',
        '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
        '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}',
        '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
        '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
        '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
        '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}',
        '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
        '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}',
        '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}',
        '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
        '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}',
        '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
        '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
        '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
    ];
    bytes.iter().map(|&byte| if byte < 0x80 { byte as char } else { PAGE[(byte - 0x80) as usize] }).collect()
}
/// `end` is one past the bit being read. A `usize` product `len * 8` overflows at 512 MiB on 32-bit targets.
pub(super) fn bit_end_fits(at: u64, count: u64, len: usize) -> bool {
    let Some(end) = at.checked_add(count) else { return false; };
    match (len as u64).checked_mul(8) {
        Some(bits) => end <= bits,
        None => {
            let byte = end / 8;
            let rem = end % 8;
            byte < len as u64 || (byte == len as u64 && rem == 0)
        }
    }
}
mod raw_deflate {
    use super::{bit_end_fits, file_limit, zip_limit, Result};
    struct Bits<'a> { bytes: &'a [u8], at: u64 }
    impl Bits<'_> {
        fn take(&mut self, count: usize) -> Result<usize> {
            if count > 16 || !bit_end_fits(self.at, count as u64, self.bytes.len()) {
                return Err("Truncated compressed member".into());
            }
            let mut value = 0;
            for shift in 0..count {
                let byte = usize::try_from(self.at / 8).map_err(|_| "Compressed offset overflow")?;
                value |= (((self.bytes[byte] >> (self.at % 8)) & 1) as usize) << shift;
                self.at += 1;
            }
            Ok(value)
        }
    }
    struct Codes { counts: [usize; 16], first: [usize; 16], offsets: [usize; 16], symbols: Vec<usize> }
    impl Codes {
        fn new(lengths: &[usize], single: bool, empty: bool) -> Result<Self> {
            let mut counts = [0usize; 16];
            for &length in lengths {
                if length > 15 { return Err("Invalid compressed code length".into()); }
                if length != 0 { counts[length] += 1; }
            }
            let size: usize = counts.iter().sum();
            let mut left = 1isize;
            for &count in &counts[1..] {
                left = (left << 1) - count as isize;
                if left < 0 { return Err("Oversubscribed compressed codes".into()); }
            }
            if left != 0 && !(single && size == 1 && counts[1] == 1) && !(empty && size == 0) {
                return Err("Incomplete compressed codes".into());
            }
            let mut first = [0; 16];
            let mut offsets = [0; 16];
            for length in 1..16 {
                first[length] = (first[length - 1] + counts[length - 1]) << 1;
                offsets[length] = offsets[length - 1] + counts[length - 1];
            }
            let mut symbols = vec![0; size];
            let mut next = offsets;
            for (symbol, &length) in lengths.iter().enumerate() {
                if length != 0 { symbols[next[length]] = symbol; next[length] += 1; }
            }
            Ok(Self { counts, first, offsets, symbols })
        }
        fn symbol(&self, bits: &mut Bits<'_>) -> Result<usize> {
            let mut code = 0;
            for length in 1..16 {
                code = (code << 1) | bits.take(1)?;
                if code >= self.first[length] {
                    let index = code - self.first[length];
                    if index < self.counts[length] { return Ok(self.symbols[self.offsets[length] + index]); }
                }
            }
            Err("Invalid compressed symbol".into())
        }
    }
    fn tables(bits: &mut Bits<'_>) -> Result<(Codes, Codes)> {
        let literals = bits.take(5)? + 257;
        let distances = bits.take(5)? + 1;
        if literals > 286 { return Err("Reserved compressed literal count".into()); }
        let count = bits.take(4)? + 4;
        let order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
        let mut lengths = [0; 19];
        for &symbol in &order[..count] { lengths[symbol] = bits.take(3)?; }
        let code = Codes::new(&lengths, false, false)?;
        let mut lengths = Vec::with_capacity(literals + distances);
        while lengths.len() < literals + distances {
            let symbol = code.symbol(bits)?;
            let (value, repeat) = match symbol {
                0..=15 => (symbol, 1),
                16 => (*lengths.last().ok_or("Compressed repeat has no previous length")?, bits.take(2)? + 3),
                17 => (0, bits.take(3)? + 3),
                18 => (0, bits.take(7)? + 11),
                _ => return Err("Invalid compressed length symbol".into()),
            };
            if lengths.len() + repeat > literals + distances { return Err("Compressed length repeat overflow".into()); }
            lengths.resize(lengths.len() + repeat, value);
        }
        if lengths[256] == 0 { return Err("Compressed member lacks end symbol".into()); }
        Ok((Codes::new(&lengths[..literals], true, false)?, Codes::new(&lengths[literals..], true, true)?))
    }
    // Raw deflate blocks. Output stays within the declared member size, which
    // the project budget already bounded. Historical fixed caps do not apply.
    // Method 9 uses the same blocks with a 64KiB window, distance codes 30 and 31,
    // and length code 285 with base 3 and 16 extra bits.
    pub(super) fn decode(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        expand(bytes, expected, false)
    }
    pub(super) fn decode_wide(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        expand(bytes, expected, true)
    }
    fn expand(bytes: &[u8], expected: usize, wide: bool) -> Result<Vec<u8>> {
        if expected as u64 > file_limit() || bytes.len() as u64 > zip_limit() {
            return Err("Compressed member exceeds the memory budget".into());
        }
        let mut bits = Bits { bytes, at: 0 };
        let mut out = Vec::new();
        out.try_reserve_exact(expected).map_err(|_| "Compressed member allocation failed")?;
        let mut lengths = [8; 288];
        lengths[144..256].fill(9);
        lengths[256..280].fill(7);
        let fixed = (Codes::new(&lengths, false, false)?, Codes::new(&[5; 32], false, false)?);
        let mut blocks = 0usize;
        loop {
            blocks += 1;
            if blocks > 65536 { return Err("Compressed member exceeds block-processing limit".into()); }
            super::stop_unless_running()?;
            let final_block = bits.take(1)? != 0;
            match bits.take(2)? {
                0 => {
                    bits.at = bits.at.checked_add(7).ok_or("Compressed offset overflow")? & !7;
                    let length = bits.take(16)?;
                    if (length ^ bits.take(16)?) != 0xffff { return Err("Compressed stored length mismatch".into()); }
                    if out.len().checked_add(length).map(|n| n > expected).unwrap_or(true) {
                        return Err("Compressed member exceeds declared size".into());
                    }
                    let start = usize::try_from(bits.at / 8).map_err(|_| "Compressed offset overflow")?;
                    let end = start.checked_add(length).ok_or("Compressed offset overflow")?;
                    let data = bytes.get(start..end).ok_or("Truncated compressed stored block")?;
                    out.extend_from_slice(data);
                    bits.at = bits.at.checked_add((length as u64).checked_mul(8).ok_or("Compressed offset overflow")?).ok_or("Compressed offset overflow")?;
                }
                kind @ (1 | 2) => {
                    let dynamic;
                    let (literal, distance) = if kind == 1 { (&fixed.0, &fixed.1) }
                        else { dynamic = tables(&mut bits)?; (&dynamic.0, &dynamic.1) };
                    loop {
                        match literal.symbol(&mut bits)? {
                            symbol @ 0..=255 => {
                                if out.len() == expected { return Err("Compressed member exceeds declared size".into()); }
                                out.push(symbol as u8);
                            }
                            256 => break,
                            symbol @ 257..=285 => {
                                const BASE: [usize; 29] = [3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258];
                                const EXTRA: [usize; 29] = [0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0];
                                let (base_len, extra_len) = if wide && symbol == 285 { (3, 16) } else { (BASE[symbol - 257], EXTRA[symbol - 257]) };
                                let length = base_len + bits.take(extra_len)?;
                                let symbol = distance.symbol(&mut bits)?;
                                if symbol > if wide { 31 } else { 29 } { return Err("Reserved compressed distance".into()); }
                                let extra = if symbol < 4 { 0 } else { symbol / 2 - 1 };
                                let base = if symbol < 4 { symbol + 1 } else { (2 + symbol % 2) * (1 << extra) + 1 };
                                let offset = base + bits.take(extra)?;
                                let window = if wide { 65536 } else { 32768 };
                                if offset == 0 || offset > window || offset > out.len() || out.len().checked_add(length).map(|n| n > expected).unwrap_or(true) {
                                    return Err("Compressed back-reference outside member bounds".into());
                                }
                                for step in 0..length {
                                    if step % 1024 == 0 { super::stop_unless_running()?; }
                                    out.push(out[out.len() - offset]);
                                }
                            }
                            _ => return Err("Reserved compressed literal".into()),
                        }
                    }
                }
                _ => return Err("Reserved compressed block type".into()),
            }
            if final_block { break; }
        }
        let consumed = bits.at.checked_add(7).ok_or("Compressed offset overflow")? / 8;
        if out.len() != expected || consumed != bytes.len() as u64 {
            return Err("Compressed member size or trailing data mismatch".into());
        }
        Ok(out)
    }
}
mod bzip2_member {
    use super::{bit_end_fits, file_limit, stop_unless_running, zip_limit, Result};
    const fn crc_table() -> [u32; 256] {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = (i as u32) << 24;
            let mut bit = 0;
            while bit < 8 {
                if crc & 0x8000_0000 != 0 { crc = (crc << 1) ^ 0x04C1_1DB7; } else { crc <<= 1; }
                bit += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    }
    const CRC_TABLE: [u32; 256] = crc_table();
    pub(super) fn checksum(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in bytes {
            crc = (crc << 8) ^ CRC_TABLE[((crc >> 24) ^ byte as u32) as usize];
        }
        !crc
    }
    struct Msb<'a> { bytes: &'a [u8], at: u64 }
    impl Msb<'_> {
        fn bit(&mut self) -> Result<u32> {
            if !bit_end_fits(self.at, 1, self.bytes.len()) { return Err("Truncated BZIP2 stream".into()); }
            let index = (self.at / 8) as usize;
            let value = (self.bytes[index] >> (7 - (self.at as u32 % 8))) & 1;
            self.at += 1;
            Ok(value as u32)
        }
        fn take(&mut self, count: usize) -> Result<u64> {
            if count > 48 { return Err("Truncated BZIP2 stream".into()); }
            let mut value = 0u64;
            for _ in 0..count { value = (value << 1) | self.bit()? as u64; }
            Ok(value)
        }
    }
    struct Tree { counts: [usize; 21], first: [u32; 21], offsets: [usize; 21], symbols: Vec<u16> }
    impl Tree {
        fn new(lengths: &[usize]) -> Result<Self> {
            let mut counts = [0usize; 21];
            for &length in lengths {
                if length == 0 || length > 20 { return Err("Invalid BZIP2 code length".into()); }
                counts[length] += 1;
            }
            let mut first = [0u32; 21];
            let mut offsets = [0usize; 21];
            for length in 1..21 {
                first[length] = (first[length - 1] + counts[length - 1] as u32) << 1;
                offsets[length] = offsets[length - 1] + counts[length - 1];
            }
            let mut symbols = vec![0u16; lengths.len()];
            let mut next = offsets;
            for (symbol, &length) in lengths.iter().enumerate() {
                symbols[next[length]] = symbol as u16;
                next[length] += 1;
            }
            let mut left = 1i32;
            for length in 1..21 { left = left.saturating_mul(2).saturating_sub(counts[length] as i32); if left < 0 { return Err("Oversubscribed BZIP2 codes".into()); } }
            if left != 0 { return Err("Incomplete BZIP2 codes".into()); }
            Ok(Self { counts, first, offsets, symbols })
        }
        fn symbol(&self, bits: &mut Msb<'_>) -> Result<u16> {
            let mut code = 0u32;
            for length in 1..21 {
                code = (code << 1) | bits.bit()?;
                if code >= self.first[length] {
                    let index = (code - self.first[length]) as usize;
                    if index < self.counts[length] { return Ok(self.symbols[self.offsets[length] + index]); }
                }
            }
            Err("Invalid BZIP2 symbol".into())
        }
    }
    fn inverse_bwt(bwt: &[u8], origin: usize) -> Result<Vec<u8>> {
        if origin >= bwt.len() { return Err("BZIP2 block index is outside the block".into()); }
        let mut count = [0usize; 256];
        for &byte in bwt { count[byte as usize] += 1; }
        let mut base = [0usize; 256];
        let mut sum = 0usize;
        for byte in 0..256 { base[byte] = sum; sum += count[byte]; }
        let mut seen = base;
        let mut next = vec![0usize; bwt.len()];
        for (index, &byte) in bwt.iter().enumerate() {
            next[index] = seen[byte as usize];
            seen[byte as usize] += 1;
        }
        let mut out = vec![0u8; bwt.len()];
        let mut cursor = origin;
        for slot in (0..bwt.len()).rev() {
            out[slot] = bwt[cursor];
            cursor = next[cursor];
        }
        Ok(out)
    }
    fn undo_runs(data: &[u8], limit: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.try_reserve(data.len().min(limit)).map_err(|_| "BZIP2 member exceeds the memory budget")?;
        let mut run = 0u8;
        let mut prev = 0u8;
        for &byte in data {
            if run == 4 {
                let extra = byte as usize;
                if out.len().checked_add(extra).map(|n| n > limit).unwrap_or(true) { return Err("BZIP2 member exceeds declared size".into()); }
                out.extend(std::iter::repeat(prev).take(extra));
                run = 0;
                continue;
            }
            if out.len() >= limit { return Err("BZIP2 member exceeds declared size".into()); }
            out.push(byte);
            if run > 0 && byte == prev { run += 1; } else { run = 1; prev = byte; }
        }
        Ok(out)
    }
    pub(super) fn decode(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        if expected as u64 > file_limit() || bytes.len() as u64 > zip_limit() { return Err("Compressed member exceeds the memory budget".into()); }
        stop_unless_running()?;
        if bytes.len() < 14 || &bytes[..3] != b"BZh" || !bytes[3].is_ascii_digit() || bytes[3] == b'0' { return Err("BZIP2 stream header is invalid".into()); }
        let block_cap = (bytes[3] - b'0') as usize * 100_000;
        let mut bits = Msb { bytes, at: 32 };
        let mut out = Vec::new();
        out.try_reserve(expected).map_err(|_| "Compressed member exceeds the memory budget")?;
        let mut combined = 0u32;
        loop {
            stop_unless_running()?;
            let magic = bits.take(48)?;
            if magic == 0x1772_4538_5090 {
                let recorded = bits.take(32)? as u32;
                if recorded != combined { return Err("BZIP2 stream checksum does not match".into()); }
                break;
            }
            if magic != 0x3141_5926_5359 { return Err("BZIP2 block header is invalid".into()); }
            let block_crc = bits.take(32)? as u32;
            if bits.bit()? != 0 { return Err("BZIP2 randomised block is unsupported".into()); }
            let origin = bits.take(24)? as usize;
            let mut used = [false; 256];
            let mut groups = [false; 16];
            for group in &mut groups { *group = bits.bit()? != 0; }
            for (group, present) in groups.iter().enumerate() {
                if *present {
                    for byte in 0..16 { used[group * 16 + byte] = bits.bit()? != 0; }
                }
            }
            let stack: Vec<u8> = (0..256).filter(|&byte| used[byte]).map(|byte| byte as u8).collect();
            let in_use = stack.len();
            if in_use == 0 || in_use > 256 { return Err("BZIP2 block alphabet is empty".into()); }
            let alpha = in_use + 2;
            let end_symbol = (in_use + 1) as u16;
            let n_groups = bits.take(3)? as usize;
            if !(2..=6).contains(&n_groups) { return Err("BZIP2 Huffman group count is invalid".into()); }
            let n_selectors = bits.take(15)? as usize;
            if n_selectors == 0 || n_selectors > 18002 { return Err("BZIP2 selector count is invalid".into()); }
            let mut order: Vec<usize> = (0..n_groups).collect();
            let mut selectors = Vec::with_capacity(n_selectors);
            for _ in 0..n_selectors {
                let mut index = 0usize;
                while bits.bit()? != 0 {
                    index += 1;
                    if index >= n_groups { return Err("BZIP2 selector is invalid".into()); }
                }
                let group = order[index];
                order.remove(index);
                order.insert(0, group);
                selectors.push(group);
            }
            let mut trees = Vec::with_capacity(n_groups);
            for _ in 0..n_groups {
                let mut current = bits.take(5)? as usize;
                let mut lengths = vec![0usize; alpha];
                for length in &mut lengths {
                    loop {
                        if !(1..=20).contains(&current) { return Err("Invalid BZIP2 code length".into()); }
                        if bits.bit()? == 0 { break; }
                        if bits.bit()? == 0 { current += 1; } else { current = current.saturating_sub(1); }
                    }
                    *length = current;
                }
                trees.push(Tree::new(&lengths)?);
            }
            let mut coded = Vec::new();
            let mut selector_at = 0usize;
            let mut left = 0usize;
            loop {
                if left == 0 {
                    if selector_at >= selectors.len() { return Err("BZIP2 selector table ended early".into()); }
                    left = 50;
                    selector_at += 1;
                }
                left -= 1;
                let symbol = trees[selectors[selector_at - 1]].symbol(&mut bits)?;
                if symbol == end_symbol { break; }
                if coded.len() > block_cap.saturating_mul(2) { return Err("BZIP2 block exceeds its declared size".into()); }
                coded.push(symbol);
            }
            let mut mtf = stack;
            let mut bwt = Vec::new();
            let mut cursor = 0usize;
            while cursor < coded.len() {
                let symbol = coded[cursor];
                if symbol <= 1 {
                    let mut count = 0u32;
                    let mut scale = 1u32;
                    while cursor < coded.len() && coded[cursor] <= 1 {
                        count = count.checked_add(scale << coded[cursor]).ok_or("BZIP2 run length overflow")?;
                        scale = scale.checked_mul(2).ok_or("BZIP2 run length overflow")?;
                        cursor += 1;
                    }
                    if mtf.is_empty() { return Err("BZIP2 block alphabet is empty".into()); }
                    if bwt.len().checked_add(count as usize).map(|n| n > block_cap).unwrap_or(true) { return Err("BZIP2 block exceeds its declared size".into()); }
                    bwt.extend(std::iter::repeat(mtf[0]).take(count as usize));
                } else {
                    let index = symbol as usize - 1;
                    if index >= mtf.len() { return Err("BZIP2 symbol is outside the alphabet".into()); }
                    let byte = mtf[index];
                    if bwt.len() >= block_cap { return Err("BZIP2 block exceeds its declared size".into()); }
                    bwt.push(byte);
                    mtf.remove(index);
                    mtf.insert(0, byte);
                    cursor += 1;
                }
            }
            let plain = undo_runs(&inverse_bwt(&bwt, origin)?, expected.saturating_sub(out.len()))?;
            if checksum(&plain) != block_crc { return Err("BZIP2 block checksum does not match".into()); }
            combined = combined.rotate_left(1) ^ block_crc;
            if out.len().checked_add(plain.len()).map(|n| n > expected).unwrap_or(true) { return Err("BZIP2 member exceeds declared size".into()); }
            out.extend_from_slice(&plain);
        }
        let consumed = bits.at.checked_add(7).ok_or("Compressed offset overflow")? / 8;
        if out.len() != expected || consumed != bytes.len() as u64 { return Err("BZIP2 member size or trailing data mismatch".into()); }
        Ok(out)
    }
    #[cfg(any(test, rllm_module_tests))]
    pub(super) fn encode(data: &[u8]) -> Result<Vec<u8>> {
        if data.is_empty() { return Ok(vec![0x42, 0x5a, 0x68, 0x31, 0x17, 0x72, 0x45, 0x38, 0x50, 0x90, 0x00, 0x00, 0x00, 0x00]); }
        let reduced = rle_encode(data);
        let (bwt, origin) = forward_bwt(&reduced);
        let (coded, used) = mtf_symbols(&bwt);
        let alpha = used.len() + 2;
        let mut symbols = coded;
        symbols.push((used.len() + 1) as u16);
        let lengths = complete_lengths(alpha);
        let codes = canonical(&lengths);
        let mut writer = BitsOut::default();
        writer.bytes.extend_from_slice(b"BZh1");
        writer.put(0x3141_5926_5359, 48);
        writer.put(checksum(data) as u64, 32);
        writer.put(0, 1);
        writer.put(origin as u64, 24);
        let mut map = [false; 256];
        for &byte in &used { map[byte as usize] = true; }
        for group in 0..16 { writer.put(u64::from(map[group * 16..group * 16 + 16].iter().any(|bit| *bit)), 1); }
        for group in 0..16 {
            if map[group * 16..group * 16 + 16].iter().any(|bit| *bit) {
                for byte in 0..16 { writer.put(u64::from(map[group * 16 + byte]), 1); }
            }
        }
        writer.put(2, 3);
        let selectors = symbols.len().saturating_add(49) / 50;
        writer.put(selectors as u64, 15);
        for _ in 0..selectors { writer.put(0, 1); }
        for _ in 0..2 { write_lengths(&mut writer, &lengths); }
        for (index, symbol) in symbols.iter().enumerate() {
            let (code, length) = codes[*symbol as usize];
            writer.put(code as u64, length);
            let _ = index;
        }
        writer.put(0x1772_4538_5090, 48);
        // One block: combined = rotate(0) xor block checksum = block checksum.
        writer.put(checksum(data) as u64, 32);
        writer.finish();
        Ok(writer.bytes)
    }
    #[cfg(any(test, rllm_module_tests))]
    fn rle_encode(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut index = 0;
        while index < data.len() {
            let byte = data[index];
            let mut count = 1;
            while index + count < data.len() && data[index + count] == byte && count < 259 { count += 1; }
            if count < 4 { out.extend(std::iter::repeat(byte).take(count)); }
            else { out.extend(std::iter::repeat(byte).take(4)); out.push((count - 4) as u8); }
            index += count;
        }
        out
    }
    #[cfg(any(test, rllm_module_tests))]
    fn forward_bwt(data: &[u8]) -> (Vec<u8>, usize) {
        let mut order: Vec<usize> = (0..data.len()).collect();
        order.sort_by(|&left, &right| {
            for step in 0..data.len() {
                let cmp = data[(left + step) % data.len()].cmp(&data[(right + step) % data.len()]);
                if cmp != std::cmp::Ordering::Equal { return cmp; }
            }
            left.cmp(&right)
        });
        let mut out = vec![0u8; data.len()];
        let mut origin = 0usize;
        for (row, &start) in order.iter().enumerate() {
            out[row] = data[(start + data.len() - 1) % data.len()];
            if start == 0 { origin = row; }
        }
        (out, origin)
    }
    #[cfg(any(test, rllm_module_tests))]
    fn mtf_symbols(block: &[u8]) -> (Vec<u16>, Vec<u8>) {
        let mut seen = [false; 256];
        for &byte in block { seen[byte as usize] = true; }
        let used: Vec<u8> = (0..256).filter(|&byte| seen[byte]).map(|byte| byte as u8).collect();
        let mut stack = used.clone();
        let mut out = Vec::new();
        let mut pending = 0u32;
        let flush = |pending: &mut u32, out: &mut Vec<u16>| {
            if *pending == 0 { return; }
            let mut value = *pending - 1;
            *pending = 0;
            loop {
                out.push(u16::from(value & 1 == 1));
                if value < 2 { break; }
                value = (value - 2) / 2;
            }
        };
        for &byte in block {
            let index = stack.iter().position(|&item| item == byte).unwrap();
            if index == 0 { pending += 1; }
            else {
                flush(&mut pending, &mut out);
                out.push((index + 1) as u16);
                let value = stack.remove(index);
                stack.insert(0, value);
            }
        }
        flush(&mut pending, &mut out);
        (out, used)
    }
    #[cfg(any(test, rllm_module_tests))]
    fn complete_lengths(count: usize) -> Vec<usize> {
        let mut width = 1usize;
        while (1usize << width) < count { width += 1; }
        let long = (count * 2).saturating_sub(1usize << width);
        let mut lengths = vec![width.saturating_sub(1).max(1); count];
        if width == 1 { return vec![1; count]; }
        for slot in 0..long { lengths[count - 1 - slot] = width; }
        lengths
    }
    #[cfg(any(test, rllm_module_tests))]
    fn canonical(lengths: &[usize]) -> Vec<(u32, usize)> {
        let mut counts = [0usize; 21];
        for &length in lengths { counts[length] += 1; }
        let mut first = [0u32; 21];
        let mut next = [0u32; 21];
        for length in 1..21 {
            first[length] = (first[length - 1] + counts[length - 1] as u32) << 1;
            next[length] = first[length];
        }
        let mut codes = vec![(0u32, 0usize); lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            codes[symbol] = (next[length], length);
            next[length] += 1;
        }
        codes
    }
    #[cfg(any(test, rllm_module_tests))]
    fn write_lengths(writer: &mut BitsOut, lengths: &[usize]) {
        let mut current = lengths[0] as u64;
        writer.put(current, 5);
        for &length in lengths {
            let target = length as u64;
            while current != target {
                writer.put(1, 1);
                if current < target { writer.put(0, 1); current += 1; } else { writer.put(1, 1); current -= 1; }
            }
            writer.put(0, 1);
        }
    }
    #[cfg(any(test, rllm_module_tests))]
    #[derive(Default)]
    struct BitsOut { bytes: Vec<u8>, acc: u64, n: usize }
    #[cfg(any(test, rllm_module_tests))]
    impl BitsOut {
        fn put(&mut self, value: u64, nbits: usize) {
            for shift in (0..nbits).rev() {
                self.acc = (self.acc << 1) | ((value >> shift) & 1);
                self.n += 1;
                if self.n == 8 { self.bytes.push(self.acc as u8); self.acc = 0; self.n = 0; }
            }
        }
        fn finish(&mut self) { if self.n > 0 { self.acc <<= 8 - self.n; self.bytes.push(self.acc as u8); self.n = 0; } }
    }
}
mod lzma_member {
    use super::{file_limit, stop_unless_running, zip_limit, Result};
    const LIT_NEXT: [usize; 12] = [0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 4, 5];
    const MATCH_NEXT: [usize; 12] = [7, 7, 7, 7, 7, 7, 7, 10, 10, 10, 10, 10];
    const REP_NEXT: [usize; 12] = [8, 8, 8, 8, 8, 8, 8, 11, 11, 11, 11, 11];
    const SHORT_NEXT: [usize; 12] = [9, 9, 9, 9, 9, 9, 9, 11, 11, 11, 11, 11];
    struct Range<'a> { data: &'a [u8], pos: usize, range: u32, code: u32 }
    impl<'a> Range<'a> {
        fn new(data: &'a [u8]) -> Result<Self> {
            if data.len() < 5 || data[0] != 0 { return Err("LZMA range header is invalid".into()); }
            let mut code = 0u32;
            for byte in &data[1..5] { code = (code << 8) | *byte as u32; }
            Ok(Self { data, pos: 5, range: 0xFFFF_FFFF, code })
        }
        fn renorm(&mut self) -> Result<()> {
            if self.range < 0x0100_0000 {
                if self.pos >= self.data.len() { return Err("Truncated LZMA stream".into()); }
                self.range <<= 8;
                self.code = (self.code << 8) | self.data[self.pos] as u32;
                self.pos += 1;
            }
            Ok(())
        }
        fn bit(&mut self, prob: &mut u16) -> Result<u32> {
            self.renorm()?;
            let bound = (self.range >> 11).saturating_mul(*prob as u32);
            if self.code < bound {
                *prob += (2048 - *prob) >> 5;
                self.range = bound;
                Ok(0)
            } else {
                *prob -= *prob >> 5;
                self.code -= bound;
                self.range -= bound;
                Ok(1)
            }
        }
        fn tree(&mut self, probs: &mut [u16], bits: usize) -> Result<usize> {
            let mut index = 1usize;
            for _ in 0..bits {
                let bit = self.bit(&mut probs[index])? as usize;
                index = (index << 1) | bit;
            }
            Ok(index - (1 << bits))
        }
        fn reverse(&mut self, probs: &mut [u16], bits: usize) -> Result<u32> {
            let mut index = 1usize;
            let mut value = 0u32;
            for shift in 0..bits {
                let bit = self.bit(&mut probs[index])?;
                index = (index << 1) | bit as usize;
                value |= bit << shift;
            }
            Ok(value)
        }
        fn direct(&mut self, bits: usize) -> Result<u32> {
            let mut value = 0u32;
            for _ in 0..bits {
                self.range >>= 1;
                self.code = self.code.wrapping_sub(self.range);
                let mask = ((self.code as i32) >> 31) as u32;
                self.code = self.code.wrapping_add(self.range & mask);
                value = (value << 1) | (mask.wrapping_add(1) & 1);
                self.renorm()?;
            }
            Ok(value)
        }
    }
    struct Length { choice: u16, choice2: u16, low: [[u16; 8]; 16], mid: [[u16; 8]; 16], high: [u16; 256] }
    impl Length {
        fn new() -> Self { Self { choice: 1024, choice2: 1024, low: [[1024; 8]; 16], mid: [[1024; 8]; 16], high: [1024; 256] } }
        fn read(&mut self, range: &mut Range<'_>, pos_state: usize) -> Result<usize> {
            if range.bit(&mut self.choice)? == 0 { Ok(range.tree(&mut self.low[pos_state], 3)? + 2) }
            else if range.bit(&mut self.choice2)? == 0 { Ok(range.tree(&mut self.mid[pos_state], 3)? + 10) }
            else { Ok(range.tree(&mut self.high, 8)? + 18) }
        }
    }
    fn copy_match(out: &mut Vec<u8>, offset: usize, len: usize, expected: usize) -> Result<()> {
        if offset == 0 || offset > out.len() || out.len().checked_add(len).map(|n| n > expected).unwrap_or(true) {
            return Err("LZMA match is outside the member".into());
        }
        for step in 0..len {
            if step % 1024 == 0 { stop_unless_running()?; }
            let byte = out[out.len() - offset];
            out.push(byte);
        }
        Ok(())
    }
    pub(super) fn decode(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        if expected as u64 > file_limit() || bytes.len() as u64 > zip_limit() { return Err("Compressed member exceeds the memory budget".into()); }
        stop_unless_running()?;
        if bytes.len() < 9 { return Err("Truncated LZMA member".into()); }
        let props_size = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
        if props_size != 5 || bytes.len() < 4 + props_size { return Err("LZMA properties are invalid".into()); }
        let props = &bytes[4..9];
        let packed = props[0] as usize;
        if packed > 224 { return Err("LZMA properties are invalid".into()); }
        let lc = packed % 9;
        let rest = packed / 9;
        let lp = rest % 5;
        let pb = rest / 5;
        if pb > 4 { return Err("LZMA properties are invalid".into()); }
        let dict = u32::from_le_bytes(props[1..5].try_into().unwrap()) as usize;
        if dict == 0 { return Err("LZMA dictionary size is zero".into()); }
        let mut range = Range::new(&bytes[9..])?;
        let mut out = Vec::new();
        out.try_reserve(expected).map_err(|_| "Compressed member exceeds the memory budget")?;
        let mut literal = vec![1024u16; 0x300 * (1usize << (lc + lp))];
        let mut is_match = [[1024u16; 16]; 12];
        let mut is_rep = [1024u16; 12];
        let mut is_rep_g0 = [1024u16; 12];
        let mut is_rep_g1 = [1024u16; 12];
        let mut is_rep_g2 = [1024u16; 12];
        let mut is_rep0_long = [[1024u16; 16]; 12];
        let mut pos_slot = [[1024u16; 64]; 4];
        let mut spec = [1024u16; 128];
        let mut align = [1024u16; 16];
        let mut match_len = Length::new();
        let mut rep_len = Length::new();
        let mut state = 0usize;
        let mut reps = [1usize; 4];
        let pos_mask = (1usize << pb) - 1;
        let lit_mask = (1usize << lp) - 1;
        while out.len() < expected {
            if out.len() % 4096 == 0 { stop_unless_running()?; }
            let pos_state = out.len() & pos_mask;
            if range.bit(&mut is_match[state][pos_state])? == 0 {
                let prev = out.last().copied().unwrap_or(0);
                let lit_state = ((out.len() & lit_mask) << lc) + (prev as usize >> (8 - lc));
                let probs = &mut literal[lit_state * 0x300..lit_state * 0x300 + 0x300];
                let mut symbol = 1usize;
                if state >= 7 {
                    let mut match_byte = if reps[0] <= out.len() { out[out.len() - reps[0]] } else { 0 };
                    let mut offs = 0x100usize;
                    while symbol < 0x100 {
                        let match_bit = (match_byte >> 7) & 1;
                        match_byte <<= 1;
                        let bit = range.bit(&mut probs[offs + ((match_bit as usize) << 8) + symbol])?;
                        symbol = (symbol << 1) | bit as usize;
                        if match_bit != bit as u8 { offs = 0; }
                    }
                } else {
                    while symbol < 0x100 {
                        let bit = range.bit(&mut probs[symbol])? as usize;
                        symbol = (symbol << 1) | bit;
                    }
                }
                out.push(symbol as u8);
                state = LIT_NEXT[state];
            } else if range.bit(&mut is_rep[state])? == 0 {
                let length = match_len.read(&mut range, pos_state)?;
                let len_state = (length - 2).min(3);
                let slot = range.tree(&mut pos_slot[len_state], 6)?;
                let dist = if slot < 4 { slot as u32 } else {
                    let extra = (slot >> 1) - 1;
                    let mut dist = ((2 | (slot & 1)) as u32) << extra;
                    if slot < 14 {
                        let base = dist as usize - slot;
                        dist += range.reverse(&mut spec[base..], extra)?;
                    } else {
                        dist += range.direct(extra - 4)? << 4;
                        dist += range.reverse(&mut align, 4)?;
                    }
                    dist
                };
                if dist == u32::MAX { break; }
                let offset = dist as usize + 1;
                if offset > dict { return Err("LZMA match is outside the dictionary".into()); }
                reps[3] = reps[2]; reps[2] = reps[1]; reps[1] = reps[0]; reps[0] = offset;
                copy_match(&mut out, offset, length, expected)?;
                state = MATCH_NEXT[state];
            } else if range.bit(&mut is_rep_g0[state])? == 0 {
                if range.bit(&mut is_rep0_long[state][pos_state])? == 0 {
                    copy_match(&mut out, reps[0], 1, expected)?;
                    state = SHORT_NEXT[state];
                } else {
                    let length = rep_len.read(&mut range, pos_state)?;
                    copy_match(&mut out, reps[0], length, expected)?;
                    state = REP_NEXT[state];
                }
            } else {
                let chosen = if range.bit(&mut is_rep_g1[state])? == 0 {
                    let offset = reps[1];
                    reps[1] = reps[0];
                    offset
                } else if range.bit(&mut is_rep_g2[state])? == 0 {
                    let offset = reps[2];
                    reps[2] = reps[1];
                    reps[1] = reps[0];
                    offset
                } else {
                    let offset = reps[3];
                    reps[3] = reps[2];
                    reps[2] = reps[1];
                    reps[1] = reps[0];
                    offset
                };
                reps[0] = chosen;
                let length = rep_len.read(&mut range, pos_state)?;
                copy_match(&mut out, chosen, length, expected)?;
                state = REP_NEXT[state];
            }
        }
        if out.len() != expected { return Err("LZMA member size or trailing data mismatch".into()); }
        if range.data.len().saturating_sub(range.pos) > 8 { return Err("LZMA member size or trailing data mismatch".into()); }
        Ok(out)
    }
    #[cfg(any(test, rllm_module_tests))]
    pub(super) fn encode_literals(data: &[u8]) -> Vec<u8> { encode_literals_mode(data, false) }
    #[cfg(any(test, rllm_module_tests))]
    fn encode_literals_mode(data: &[u8], repeated: bool) -> Vec<u8> {
        struct Enc { out: Vec<u8>, low: u64, range: u32, cache: u8, cache_size: u64 }
        impl Enc {
            fn shift(&mut self) {
                let high = (self.low >> 32) as u32;
                if high != 0 || self.low < 0xFF00_0000 {
                    let mut temp = self.cache;
                    loop {
                        self.out.push(temp.wrapping_add(high as u8));
                        self.cache_size -= 1;
                        if self.cache_size == 0 { break; }
                        temp = 0xFF;
                    }
                    self.cache = (self.low >> 24) as u8;
                }
                self.cache_size += 1;
                self.low = (self.low & 0x00FF_FFFF) << 8;
            }
            fn bit(&mut self, prob: &mut u16, bit: u32) {
                let bound = (self.range >> 11) * (*prob as u32);
                if bit == 0 { self.range = bound; *prob += (2048 - *prob) >> 5; }
                else { self.low += bound as u64; self.range -= bound; *prob -= *prob >> 5; }
                while self.range < 0x0100_0000 { self.range <<= 8; self.shift(); }
            }
        }
        let mut enc = Enc { out: Vec::new(), low: 0, range: 0xFFFF_FFFF, cache: 0, cache_size: 1 };
        let mut literal = vec![1024u16; 0x300 * 8];
        let mut is_match = [[1024u16; 16]; 12];
        let mut is_rep = [1024u16; 12];
        let mut pos_slot = [[1024u16; 64]; 4];
        let mut choice = 1024u16;
        let mut low = [[1024u16; 8]; 16];
        let lc = 3usize;
        let literal_count = if repeated && data.len() >= 3 { 1 } else { data.len() };
        for pos in 0..literal_count {
            let byte = data[pos];
            let prev = if pos == 0 { 0 } else { data[pos - 1] };
            enc.bit(&mut is_match[0][pos & 3], 0);
            let lit_state = (prev as usize) >> (8 - lc);
            let probs = &mut literal[lit_state * 0x300..];
            let mut symbol = 1usize;
            for shift in (0..8).rev() {
                let bit = ((byte >> shift) & 1) as u32;
                enc.bit(&mut probs[symbol], bit);
                symbol = (symbol << 1) | bit as usize;
            }
        }
        if literal_count < data.len() {
            let length = data.len() - 1;
            let symbol = length - 2;
            enc.bit(&mut is_match[0][1], 1);
            enc.bit(&mut is_rep[0], 0);
            enc.bit(&mut choice, 0);
            let mut index = 1usize;
            for shift in (0..3).rev() {
                let bit = ((symbol >> shift) & 1) as u32;
                enc.bit(&mut low[1][index], bit);
                index = (index << 1) | bit as usize;
            }
            index = 1;
            for _ in 0..6 { enc.bit(&mut pos_slot[0][index], 0); index <<= 1; }
        }
        for _ in 0..5 { enc.shift(); }
        let mut bytes = vec![9, 4, 5, 0, 0x5D, 0x00, 0x10, 0x00, 0x00];
        bytes.extend(enc.out);
        bytes
    }
    #[cfg(any(test, rllm_module_tests))]
    pub(super) fn encode_repeat(byte: u8, count: usize) -> Vec<u8> {
        encode_literals_mode(&vec![byte; count], true)
    }
}
mod zstd_member {
    use super::{file_limit, stop_unless_running, zip_limit, Result};
    const LL_BASE: [u32; 36] = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,18,20,22,24,28,32,40,48,64,128,256,512,1024,2048,4096,8192,16384,32768,65536];
    const LL_BITS: [u8; 36] = [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,1,1,1,2,2,3,3,4,6,7,8,9,10,11,12,13,14,15,16];
    const ML_BASE: [u32; 53] = [3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,37,39,41,43,47,51,59,67,83,99,131,259,515,1027,2051,4099,8195,16387,32771,65539];
    const ML_BITS: [u8; 53] = [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,1,1,1,2,2,3,3,4,4,5,7,8,9,10,11,12,13,14,15,16];
    const OF_BASE: [u32; 32] = [0,1,2,4,8,16,32,64,128,256,512,1024,2048,4096,8192,16384,32768,65536,131072,262144,524288,1048576,2097152,4194304,8388608,16777216,33554432,67108864,134217728,268435456,536870912,1073741824];
    const OF_BITS: [u8; 32] = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31];
    const LL_NORM: [i16; 36] = [4,3,2,2,2,2,2,2,2,2,2,2,2,1,1,1,2,2,2,2,2,2,2,2,2,3,2,1,1,1,1,1,-1,-1,-1,-1];
    const ML_NORM: [i16; 53] = [1,4,3,2,2,2,2,2,2,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,-1,-1,-1,-1,-1,-1,-1];
    const OF_NORM: [i16; 29] = [1,1,1,1,1,1,2,2,2,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,-1,-1,-1,-1];
    #[cfg(any(test, rllm_module_tests))]
    pub(super) fn tables_fit() -> Result<()> {
        Fse::build(&LL_NORM, 6)?;
        Fse::build(&ML_NORM, 6)?;
        Fse::build(&OF_NORM, 5)?;
        Ok(())
    }
    pub(super) fn xxh64(data: &[u8]) -> u64 {
        const P1: u64 = 0x9E3779B185EBCA87;
        const P2: u64 = 0xC2B2AE3D27D4EB4F;
        const P3: u64 = 0x165667B19E3779F9;
        const P4: u64 = 0x85EBCA77C2B2AE63;
        const P5: u64 = 0x27D4EB2F165667C5;
        fn round(acc: u64, input: u64) -> u64 { acc.wrapping_add(input.wrapping_mul(P2)).rotate_left(31).wrapping_mul(P1) }
        let mut offset = 0usize;
        let mut hash = if data.len() >= 32 {
            let mut acc = [P1.wrapping_add(P2), P2, 0, 0u64.wrapping_sub(P1)];
            while offset + 32 <= data.len() {
                for lane in 0..4 {
                    let at = offset + lane * 8;
                    let value = u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
                    acc[lane] = round(acc[lane], value);
                }
                offset += 32;
            }
            let mut mixed = acc[0].rotate_left(1).wrapping_add(acc[1].rotate_left(7)).wrapping_add(acc[2].rotate_left(12)).wrapping_add(acc[3].rotate_left(18));
            for lane in acc { mixed = mixed ^ round(0, lane); mixed = mixed.wrapping_mul(P1).wrapping_add(P4); }
            mixed
        } else { P5 };
        hash = hash.wrapping_add(data.len() as u64);
        while offset + 8 <= data.len() {
            let value = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
            hash ^= round(0, value);
            hash = hash.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
            offset += 8;
        }
        if offset + 4 <= data.len() {
            let value = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as u64;
            hash ^= value.wrapping_mul(P1);
            hash = hash.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
            offset += 4;
        }
        while offset < data.len() {
            hash ^= (data[offset] as u64).wrapping_mul(P5);
            hash = hash.rotate_left(11).wrapping_mul(P1);
            offset += 1;
        }
        hash ^= hash >> 33; hash = hash.wrapping_mul(P2); hash ^= hash >> 29; hash = hash.wrapping_mul(P3); hash ^= hash >> 32;
        hash
    }
    struct Back<'a> { data: &'a [u8], index: usize, bits: u64, nbits: u32 }
    impl<'a> Back<'a> {
        fn new(data: &'a [u8]) -> Result<Self> {
            if data.is_empty() { return Err("Truncated Zstandard bitstream".into()); }
            let last = *data.last().unwrap();
            if last == 0 { return Err("Zstandard bitstream end mark is missing".into()); }
            let high = 31 - (last as u32).leading_zeros();
            let mut reader = Self { data, index: data.len() - 1, bits: 0, nbits: 0 };
            if high > 0 {
                reader.bits = (last as u64) & ((1u64 << high) - 1);
                reader.nbits = high;
            }
            Ok(reader)
        }
        fn take(&mut self, count: u32) -> Result<u32> {
            if count > 32 { return Err("Zstandard bitstream read is too wide".into()); }
            while self.nbits < count {
                if self.index == 0 { return Err("Truncated Zstandard bitstream".into()); }
                self.index -= 1;
                self.bits |= (self.data[self.index] as u64) << self.nbits;
                self.nbits += 8;
            }
            let value = if count == 0 { 0 } else { (self.bits & ((1u64 << count) - 1)) as u32 };
            self.bits >>= count;
            self.nbits -= count;
            Ok(value)
        }
        /// Missing bits are zero. `true` means the read passed the end of the stream.
        fn take_padded(&mut self, count: u32) -> Result<(u32, bool)> {
            if count > 32 { return Err("Zstandard bitstream read is too wide".into()); }
            if count == 0 { return Ok((0, false)); }
            while self.nbits < count {
                if self.index == 0 {
                    let have = self.nbits;
                    let value = if have == 0 { 0 } else { (self.bits & ((1u64 << have) - 1)) as u32 };
                    self.bits = 0;
                    self.nbits = 0;
                    return Ok((value, true));
                }
                self.index -= 1;
                self.bits |= (self.data[self.index] as u64) << self.nbits;
                self.nbits += 8;
            }
            let value = (self.bits & ((1u64 << count) - 1)) as u32;
            self.bits >>= count;
            self.nbits -= count;
            Ok((value, false))
        }
        fn exhausted(&self) -> bool { self.index == 0 }
    }
    struct Fse { log: u32, symbol: Vec<u8>, nbits: Vec<u8>, base: Vec<u16> }
    impl Fse {
        fn rle(symbol: u8) -> Self { Self { log: 0, symbol: vec![symbol], nbits: vec![0], base: vec![0] } }
        fn build(norm: &[i16], log: u32) -> Result<Self> {
            let size = 1usize << log;
            if log == 0 || log > 12 || norm.len() > 256 { return Err("Zstandard table is invalid".into()); }
            let mut spread = vec![0u8; size];
            let mut next = vec![0u32; norm.len()];
            let mut high = size - 1;
            let mut total = 0i32;
            for (sym, &count) in norm.iter().enumerate() {
                if count == -1 {
                    spread[high] = sym as u8;
                    if high == 0 { return Err("Zstandard table is invalid".into()); }
                    high -= 1;
                    next[sym] = 1;
                    total += 1;
                } else if count > 0 {
                    next[sym] = count as u32;
                    total += count as i32;
                } else if count < -1 { return Err("Zstandard table is invalid".into()); }
            }
            if total != size as i32 { return Err(format!("Zstandard table sum {total}")); }
            let step = (size >> 1) + (size >> 3) + 3;
            let mask = size - 1;
            let mut position = 0usize;
            for (sym, &count) in norm.iter().enumerate() {
                if count <= 0 { continue; }
                for _ in 0..count {
                    spread[position] = sym as u8;
                    let mut guard = 0;
                    loop {
                        position = (position + step) & mask;
                        guard += 1;
                        if position <= high || guard > size * 2 { break; }
                    }
                }
            }
            let mut symbol = vec![0u8; size];
            let mut nbits = vec![0u8; size];
            let mut base = vec![0u16; size];
            for cell in 0..size {
                let sym = spread[cell] as usize;
                let state = next[sym];
                next[sym] += 1;
                if state == 0 { return Err("Zstandard table is invalid".into()); }
                let hb = 31 - state.leading_zeros();
                let nb = log - hb;
                symbol[cell] = sym as u8;
                nbits[cell] = nb as u8;
                base[cell] = ((state << nb) - size as u32) as u16;
            }
            Ok(Self { log, symbol, nbits, base })
        }
        fn prime(&self, bits: &mut Back<'_>) -> Result<usize> {
            if self.log == 0 { Ok(0) } else { Ok(bits.take(self.log)? as usize) }
        }
        fn next(&self, state: &mut usize, bits: &mut Back<'_>) -> Result<u8> {
            if *state >= self.symbol.len() { return Err("Zstandard state is outside the table".into()); }
            let symbol = self.symbol[*state];
            let width = self.nbits[*state] as u32;
            let add = bits.take(width)?;
            *state = self.base[*state] as usize + add as usize;
            Ok(symbol)
        }
        fn next_padded(&self, state: &mut usize, bits: &mut Back<'_>) -> Result<(u8, bool)> {
            if *state >= self.symbol.len() { return Err("Zstandard state is outside the table".into()); }
            let symbol = self.symbol[*state];
            let width = self.nbits[*state] as u32;
            let (add, overflow) = bits.take_padded(width)?;
            *state = (self.base[*state] as usize).checked_add(add as usize).ok_or("Zstandard state is outside the table")?;
            Ok((symbol, overflow))
        }
    }
    fn forward_bits(data: &[u8], bit: usize, count: usize) -> Result<u32> {
        if count > 16 { return Err("Zstandard table is invalid".into()); }
        if count == 0 { return Ok(0); }
        let end = bit.checked_add(count).ok_or("Zstandard offset overflow")?;
        let last = (end - 1) / 8;
        if last >= data.len() { return Err("Truncated Zstandard table".into()); }
        let mut value = 0u32;
        for offset in 0..count {
            let at = bit + offset;
            value |= (((data[at / 8] >> (at % 8)) & 1) as u32) << offset;
        }
        Ok(value)
    }
    fn read_explicit_fse(bytes: &[u8], pos: &mut usize, max_symbol: usize, max_log: u32) -> Result<Fse> {
        let data = bytes.get(*pos..).ok_or("Truncated Zstandard table")?;
        if data.is_empty() { return Err("Truncated Zstandard table".into()); }
        let mut bit = 0usize;
        let mut read = |n: usize| -> Result<u32> {
            let value = forward_bits(data, bit, n)?;
            bit += n;
            Ok(value)
        };
        let log = read(4)? + 5;
        if log > max_log || log > 12 { return Err("Zstandard table is invalid".into()); }
        let mut remaining = (1i32 << log) + 1;
        let mut threshold = 1i32 << log;
        let mut nb_bits = log as i32 + 1;
        let mut norm = vec![0i16; max_symbol + 1];
        let mut symbol = 0usize;
        let mut previous_zero = false;
        let mut nonzero = 0usize;
        while remaining > 1 && symbol <= max_symbol {
            stop_unless_running()?;
            if previous_zero {
                let mut extra = 0usize;
                loop {
                    let flag = read(2)? as usize;
                    if flag < 3 {
                        extra += flag;
                        break;
                    }
                    extra += 3;
                    if extra > max_symbol { return Err("Zstandard table is invalid".into()); }
                }
                if symbol.checked_add(extra).map(|next| next > max_symbol).unwrap_or(true) {
                    return Err("Zstandard table symbol is outside the alphabet".into());
                }
                symbol += extra;
            }
            if symbol > max_symbol || nb_bits < 1 { return Err("Zstandard table is invalid".into()); }
            let max_small = threshold.checked_mul(2).and_then(|n| n.checked_sub(1)).and_then(|n| n.checked_sub(remaining)).ok_or("Zstandard table is invalid")?;
            let low = read((nb_bits - 1) as usize)? as i32;
            let raw = if low < max_small {
                low
            } else {
                let extra = read(1)? as i32;
                let mut full = low | (extra << (nb_bits - 1));
                if full >= threshold { full -= max_small; }
                full
            };
            if raw < 0 { return Err("Zstandard table is invalid".into()); }
            let proba = raw - 1;
            let used = if proba < 0 { -proba } else { proba };
            if used >= remaining { return Err("Zstandard table is invalid".into()); }
            remaining -= used;
            norm[symbol] = proba as i16;
            if proba != 0 { nonzero += 1; }
            previous_zero = proba == 0;
            symbol += 1;
            if remaining == 1 { break; }
            while remaining < threshold {
                nb_bits -= 1;
                threshold >>= 1;
                if nb_bits < 1 || threshold < 1 { return Err("Zstandard table is invalid".into()); }
            }
        }
        if remaining != 1 || nonzero < 2 || symbol == 0 { return Err("Zstandard table is invalid".into()); }
        let table = Fse::build(&norm[..symbol], log)?;
        drop(read);
        let consumed = bit.saturating_add(7) / 8;
        if consumed > data.len() { return Err("Truncated Zstandard table".into()); }
        *pos = pos.checked_add(consumed).ok_or("Zstandard offset overflow")?;
        Ok(table)
    }
    fn window_size(byte: u8) -> Result<u64> {
        let exp = (byte >> 3) as u32;
        let mantissa = (byte & 7) as u64;
        let log = 10 + exp;
        if log >= 63 { return Err("Zstandard window is too large".into()); }
        Ok((1u64 << log) + (mantissa << (log - 3)))
    }
    fn take_size(bytes: &[u8], pos: &mut usize, len: usize) -> Result<()> {
        let end = pos.checked_add(len).ok_or("Zstandard offset overflow")?;
        if end > bytes.len() { return Err("Truncated Zstandard frame".into()); }
        *pos = end;
        Ok(())
    }
    fn read_num(bytes: &[u8], pos: &mut usize, len: usize) -> Result<u64> {
        let start = *pos;
        take_size(bytes, pos, len)?;
        let mut buf = [0u8; 8];
        buf[..len].copy_from_slice(&bytes[start..start + len]);
        Ok(u64::from_le_bytes(buf))
    }
    pub(super) fn decode(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        decode_with(bytes, expected, &[])
    }
    pub(super) fn decode_with(bytes: &[u8], expected: usize, dictionary: &[u8]) -> Result<Vec<u8>> {
        if expected as u64 > file_limit() || bytes.len() as u64 > zip_limit() { return Err("Compressed member exceeds the memory budget".into()); }
        stop_unless_running()?;
        let mut pos = 0usize;
        let mut out = Vec::new();
        out.try_reserve(expected).map_err(|_| "Compressed member exceeds the memory budget")?;
        while pos < bytes.len() {
            if pos + 4 > bytes.len() { return Err("Truncated Zstandard frame".into()); }
            let magic = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            if magic & 0xFFFF_FFF0 == 0x184D_2A50 {
                let size = read_num(bytes, &mut pos, 4)? as usize;
                take_size(bytes, &mut pos, size)?;
                continue;
            }
            if magic != 0xFD2F_B528 { return Err("Zstandard frame is invalid".into()); }
            pos = decode_frame(bytes, pos, &mut out, expected, dictionary)?;
        }
        if out.len() != expected { return Err("Zstandard member size or trailing data mismatch".into()); }
        Ok(out)
    }
    pub(super) fn history_probe(dict: &[u8], offset: u32, len: u32) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        apply_match(&mut out, dict, offset, len, len as usize, 1 << 20)?;
        Ok(out)
    }
    fn decode_frame(bytes: &[u8], mut pos: usize, out: &mut Vec<u8>, expected: usize, dictionary: &[u8]) -> Result<usize> {
        if pos >= bytes.len() { return Err("Truncated Zstandard frame".into()); }
        let desc = bytes[pos];
        pos += 1;
        if desc & 0x18 != 0 { return Err("Zstandard frame header is invalid".into()); }
        let fcs_flag = desc >> 6;
        let single = desc & 0x20 != 0;
        let checksum = desc & 0x04 != 0;
        let dict_flag = desc & 3;
        let window = if single { None } else { let byte = *bytes.get(pos).ok_or("Truncated Zstandard frame")?; pos += 1; Some(window_size(byte)?) };
        let dict_id = match dict_flag {
            0 => 0,
            1 => read_num(bytes, &mut pos, 1)?,
            2 => read_num(bytes, &mut pos, 2)?,
            _ => read_num(bytes, &mut pos, 4)?,
        };
        let history: &[u8] = if dict_id == 0 {
            &[]
        } else if dictionary.is_empty() {
            &[]
        } else if dictionary.len() < 4 {
            return Err("Zstandard dictionary is truncated".into());
        } else {
            let supplied = u32::from_le_bytes(dictionary[..4].try_into().unwrap()) as u64;
            if supplied != dict_id { return Err("Zstandard dictionary identifier does not match".into()); }
            let body = &dictionary[4..];
            if body.len() >= 4 && body[..4] == [0x37, 0xA4, 0x30, 0xEC] {
                return Err("Zstandard dictionary tables are not available for this member".into());
            }
            body
        };
        let content = match (fcs_flag, single) {
            (0, true) => Some(read_num(bytes, &mut pos, 1)?),
            (0, false) => None,
            (1, _) => Some(read_num(bytes, &mut pos, 2)? + 256),
            (2, _) => Some(read_num(bytes, &mut pos, 4)?),
            _ => Some(read_num(bytes, &mut pos, 8)?),
        };
        let window = window.unwrap_or(content.unwrap_or(expected as u64).max(1));
        let block_limit = (window as usize).min(128 * 1024);
        let start = out.len();
        let mut previous: Option<(Fse, Fse, Fse)> = None;
        let mut huffman: Option<Vec<(u32, u8)>> = None;
        loop {
            stop_unless_running()?;
            if pos + 3 > bytes.len() { return Err("Truncated Zstandard block".into()); }
            let header = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], 0]);
            pos += 3;
            let last = header & 1 != 0;
            let kind = (header >> 1) & 3;
            let size = (header >> 3) as usize;
            if kind == 3 { return Err("Zstandard block type is reserved".into()); }
            if size > block_limit { return Err("Zstandard block exceeds the block limit".into()); }
            match kind {
                0 => {
                    take_size(bytes, &mut pos, size)?;
                    if out.len().checked_add(size).map(|n| n > expected).unwrap_or(true) { return Err("Zstandard member exceeds declared size".into()); }
                    out.extend_from_slice(&bytes[pos - size..pos]);
                }
                1 => {
                    let byte = *bytes.get(pos).ok_or("Truncated Zstandard block")?;
                    pos += 1;
                    if out.len().checked_add(size).map(|n| n > expected).unwrap_or(true) { return Err("Zstandard member exceeds declared size".into()); }
                    out.extend(std::iter::repeat(byte).take(size));
                }
                _ => {
                    let end = pos.checked_add(size).ok_or("Zstandard offset overflow")?;
                    if end > bytes.len() { return Err("Truncated Zstandard block".into()); }
                    decode_compressed(&bytes[pos..end], out, expected, window, history, &mut previous, &mut huffman)?;
                    pos = end;
                }
            }
            if last { break; }
        }
        if let Some(size) = content {
            if (out.len() - start) as u64 != size { return Err("Zstandard frame size does not match".into()); }
        }
        if checksum {
            let recorded = read_num(bytes, &mut pos, 4)? as u32;
            if xxh64(&out[start..]) as u32 != recorded { return Err("Zstandard frame checksum does not match".into()); }
        }
        Ok(pos)
    }
    fn apply_match(out: &mut Vec<u8>, dict: &[u8], offset: u32, match_len: u32, expected: usize, window: u64) -> Result<()> {
        if offset == 0 || offset as u64 > window { return Err("Zstandard match is outside the window".into()); }
        let offset = offset as usize;
        let match_len = match_len as usize;
        if offset > out.len().saturating_add(dict.len()) {
            return Err(if dict.is_empty() {
                "Zstandard dictionary is not available for this member".into()
            } else {
                "Zstandard match is outside the window".into()
            });
        }
        if out.len().checked_add(match_len).map(|n| n > expected).unwrap_or(true) {
            return Err("Zstandard member exceeds declared size".into());
        }
        let start_virtual = dict.len() + out.len() - offset;
        for step in 0..match_len {
            if step % 1024 == 0 { stop_unless_running()?; }
            let index = start_virtual + step;
            let byte = if index < dict.len() { dict[index] } else { out[index - dict.len()] };
            out.push(byte);
        }
        Ok(())
    }
    fn decode_compressed(block: &[u8], out: &mut Vec<u8>, expected: usize, window: u64, history: &[u8], previous: &mut Option<(Fse, Fse, Fse)>, huffman: &mut Option<Vec<(u32, u8)>>) -> Result<()> {
        let mut pos = 0usize;
        let literals = read_literals(block, &mut pos, huffman)?;
        if pos >= block.len() { return Err("Truncated Zstandard sequences".into()); }
        let (count, widths) = sequence_count(block, &mut pos)?;
        if count == 0 {
            if pos != block.len() { return Err("Zstandard sequences have trailing data".into()); }
            append(out, &literals, expected)?;
            return Ok(());
        }
        let modes = *block.get(pos).ok_or("Truncated Zstandard sequences")?;
        pos += 1;
        if modes & 3 != 0 { return Err("Zstandard sequence mode is reserved".into()); }
        let ll = table_for((modes >> 6) & 3, &LL_NORM, 6, block, &mut pos, previous.as_ref().map(|item| &item.0))?;
        let of = table_for((modes >> 4) & 3, &OF_NORM, 5, block, &mut pos, previous.as_ref().map(|item| &item.1))?;
        let ml = table_for((modes >> 2) & 3, &ML_NORM, 6, block, &mut pos, previous.as_ref().map(|item| &item.2))?;
        *previous = Some((clone_fse(&ll), clone_fse(&of), clone_fse(&ml)));
        let mut stream = Back::new(&block[pos..])?;
        let mut ll_state = ll.prime(&mut stream)?;
        let mut of_state = of.prime(&mut stream)?;
        let mut ml_state = ml.prime(&mut stream)?;
        let mut reps = [1u32, 4, 8];
        let mut lit_at = 0usize;
        for _ in 0..count {
            stop_unless_running()?;
            let ll_code = ll.next(&mut ll_state, &mut stream)? as usize;
            let of_code = of.next(&mut of_state, &mut stream)? as usize;
            let ml_code = ml.next(&mut ml_state, &mut stream)? as usize;
            if ll_code >= LL_BASE.len() || of_code >= OF_BASE.len() || ml_code >= ML_BASE.len() { return Err("Zstandard sequence code is invalid".into()); }
            let lit_len = LL_BASE[ll_code] + stream.take(LL_BITS[ll_code] as u32)?;
            let match_len = ML_BASE[ml_code] + stream.take(ML_BITS[ml_code] as u32)?;
            let offset = offset_of(of_code, &mut stream, lit_len, &mut reps)?;
            if lit_at.checked_add(lit_len as usize).map(|n| n > literals.len()).unwrap_or(true) { return Err("Zstandard literal length exceeds the literals".into()); }
            append(out, &literals[lit_at..lit_at + lit_len as usize], expected)?;
            lit_at += lit_len as usize;
            apply_match(out, history, offset, match_len, expected, window)?;
            let _ = widths;
        }
        append(out, &literals[lit_at..], expected)?;
        if !stream.exhausted() { return Err("Zstandard sequences have trailing data".into()); }
        Ok(())
    }
    fn clone_fse(table: &Fse) -> Fse { Fse { log: table.log, symbol: table.symbol.clone(), nbits: table.nbits.clone(), base: table.base.clone() } }
    fn table_for(mode: u8, norm: &[i16], log: u32, block: &[u8], pos: &mut usize, previous: Option<&Fse>) -> Result<Fse> {
        match mode {
            0 => Fse::build(norm, log),
            1 => {
                let symbol = *block.get(*pos).ok_or("Truncated Zstandard table")?;
                *pos += 1;
                Ok(Fse::rle(symbol))
            }
            2 => {
                let (max_symbol, max_accuracy) = match norm.len() {
                    36 => (35, 9),
                    29 => (31, 8),
                    53 => (52, 9),
                    _ => return Err("Zstandard table is invalid".into()),
                };
                read_explicit_fse(block, pos, max_symbol, max_accuracy)
            }
            _ => previous.map(clone_fse).ok_or_else(|| "Zstandard repeat table is missing".into()),
        }
    }
    fn offset_of(symbol: usize, bits: &mut Back<'_>, lit_len: u32, reps: &mut [u32; 3]) -> Result<u32> {
        let width = OF_BITS[symbol] as u32;
        if width > 1 {
            let offset = OF_BASE[symbol].wrapping_add(bits.take(width)?);
            if offset == 0 { return Err("Zstandard offset is zero".into()); }
            reps[2] = reps[1]; reps[1] = reps[0]; reps[0] = offset;
            Ok(offset)
        } else {
            let mut selector = if width == 0 { symbol as u32 } else { bits.take(1)? };
            if lit_len == 0 { selector += 1; }
            let offset = match selector {
                0 => reps[0],
                1 => { let offset = reps[1]; reps[1] = reps[0]; reps[0] = offset; offset }
                2 => { let offset = reps[2]; reps[2] = reps[1]; reps[1] = reps[0]; reps[0] = offset; offset }
                _ => {
                    let offset = reps[0].wrapping_sub(1);
                    if offset == 0 { return Err("Zstandard offset is zero".into()); }
                    reps[2] = reps[1]; reps[1] = reps[0]; reps[0] = offset;
                    offset
                }
            };
            Ok(offset)
        }
    }
    fn sequence_count(block: &[u8], pos: &mut usize) -> Result<(usize, usize)> {
        let first = *block.get(*pos).ok_or("Truncated Zstandard sequences")? as usize;
        *pos += 1;
        if first < 128 { Ok((first, 1)) }
        else if first < 255 {
            let second = *block.get(*pos).ok_or("Truncated Zstandard sequences")? as usize;
            *pos += 1;
            Ok((((first - 128) << 8) + second, 2))
        } else {
            if *pos + 2 > block.len() { return Err("Truncated Zstandard sequences".into()); }
            let value = block[*pos] as usize + ((block[*pos + 1] as usize) << 8) + 0x7F00;
            *pos += 2;
            Ok((value, 3))
        }
    }
    fn read_literals(block: &[u8], pos: &mut usize, huffman: &mut Option<Vec<(u32, u8)>>) -> Result<Vec<u8>> {
        let header = *block.get(*pos).ok_or("Truncated Zstandard literals")?;
        let kind = header & 3;
        let format = (header >> 2) & 3;
        match kind {
            0 | 1 => {
                let (size, width) = match format {
                    0 | 2 => ((header >> 3) as usize, 1),
                    1 => {
                        let next = *block.get(*pos + 1).ok_or("Truncated Zstandard literals")?;
                        (((header as usize) >> 4) + ((next as usize) << 4), 2)
                    }
                    _ => {
                        if *pos + 3 > block.len() { return Err("Truncated Zstandard literals".into()); }
                        let size = ((header as usize) >> 4) + ((block[*pos + 1] as usize) << 4) + ((block[*pos + 2] as usize) << 12);
                        (size, 3)
                    }
                };
                *pos += width;
                if kind == 0 {
                    let start = *pos;
                    take_size(block, pos, size)?;
                    Ok(block[start..*pos].to_vec())
                } else {
                    let byte = *block.get(*pos).ok_or("Truncated Zstandard literals")?;
                    *pos += 1;
                    Ok(vec![byte; size])
                }
            }
            2 | 3 => read_huffman_literals(block, pos, header, kind, huffman),
            _ => Err("Zstandard literal block is invalid".into()),
        }
    }
    fn read_huffman_literals(block: &[u8], pos: &mut usize, header: u8, kind: u8, huffman: &mut Option<Vec<(u32, u8)>>) -> Result<Vec<u8>> {
        let format = (header >> 2) & 3;
        let (regen, compressed, width, streams) = match format {
            0 | 1 => {
                if *pos + 3 > block.len() { return Err("Truncated Zstandard literals".into()); }
                let b1 = block[*pos + 1] as usize;
                let b2 = block[*pos + 2] as usize;
                let regen = ((header as usize) >> 4) + ((b1 & 0x3F) << 4);
                let compressed = (b1 >> 6) + (b2 << 2);
                (regen, compressed, 3, if format == 0 { 1 } else { 4 })
            }
            2 => {
                if *pos + 4 > block.len() { return Err("Truncated Zstandard literals".into()); }
                let b1 = block[*pos + 1] as usize;
                let b2 = block[*pos + 2] as usize;
                let b3 = block[*pos + 3] as usize;
                let regen = ((header as usize) >> 4) + (b1 << 4) + ((b2 & 0x3F) << 12);
                let compressed = (b2 >> 6) + (b3 << 2);
                (regen, compressed, 4, 4)
            }
            _ => {
                if *pos + 5 > block.len() { return Err("Truncated Zstandard literals".into()); }
                let b1 = block[*pos + 1] as usize;
                let b2 = block[*pos + 2] as usize;
                let b3 = block[*pos + 3] as usize;
                let b4 = block[*pos + 4] as usize;
                let regen = ((header as usize) >> 4) + (b1 << 4) + (b2 << 12) + ((b3 & 0x3F) << 20);
                let compressed = (b3 >> 6) + (b4 << 2);
                (regen, compressed, 5, 4)
            }
        };
        *pos += width;
        let start = *pos;
        take_size(block, pos, compressed)?;
        let payload = &block[start..start + compressed];
        if payload.is_empty() { return Err("Truncated Zstandard Huffman table".into()); }
        let (codes, body) = if kind == 3 {
            let codes = huffman.clone().ok_or_else(|| "Zstandard treeless literals need a previous table".to_string())?;
            (codes, payload)
        } else {
            let (weights, tree_bytes) = huffman_weights(payload)?;
            if tree_bytes > payload.len() { return Err("Truncated Zstandard Huffman table".into()); }
            let codes = huffman_codes(&weights)?;
            *huffman = Some(codes.clone());
            (codes, &payload[tree_bytes..])
        };
        if streams == 1 { return huffman_stream(body, &codes, regen); }
        if body.len() < 6 { return Err("Truncated Zstandard Huffman streams".into()); }
        let s1 = u16::from_le_bytes([body[0], body[1]]) as usize;
        let s2 = u16::from_le_bytes([body[2], body[3]]) as usize;
        let s3 = u16::from_le_bytes([body[4], body[5]]) as usize;
        let rest = body.len() - 6;
        if s1 + s2 + s3 > rest { return Err("Truncated Zstandard Huffman streams".into()); }
        let s4 = rest - (s1 + s2 + s3);
        let base = 6;
        let sizes = [s1, s2, s3, s4];
        let share = regen / 4;
        let mut out = Vec::with_capacity(regen);
        let mut at = base;
        for (index, size) in sizes.iter().enumerate() {
            let count = if index == 3 { regen - share * 3 } else { share };
            out.extend(huffman_stream(&body[at..at + size], &codes, count)?);
            at += size;
        }
        Ok(out)
    }
    fn huffman_weights(payload: &[u8]) -> Result<(Vec<u8>, usize)> {
        if payload.is_empty() { return Err("Truncated Zstandard Huffman table".into()); }
        let header = payload[0];
        if header < 128 {
            let compressed = header as usize;
            if compressed == 0 || payload.len() < 1 + compressed { return Err("Truncated Zstandard Huffman table".into()); }
            let explicit = fse_weight_symbols(&payload[1..1 + compressed])?;
            return Ok((imply_last_weight(explicit)?, 1 + compressed));
        }
        let count = (header - 127) as usize;
        if count == 0 || count > 256 { return Err("Zstandard Huffman alphabet is invalid".into()); }
        if count == 1 { return Ok((vec![1], 1)); }
        let stored = count - 1;
        let bytes = (stored + 1) / 2;
        if payload.len() < 1 + bytes { return Err("Truncated Zstandard Huffman table".into()); }
        let mut explicit = Vec::with_capacity(stored);
        for index in 0..stored {
            let byte = payload[1 + index / 2];
            explicit.push(if index % 2 == 0 { byte >> 4 } else { byte & 0x0F });
        }
        Ok((imply_last_weight(explicit)?, 1 + bytes))
    }
    fn imply_last_weight(mut explicit: Vec<u8>) -> Result<Vec<u8>> {
        if explicit.is_empty() || explicit.len() > 255 { return Err("Zstandard Huffman alphabet is invalid".into()); }
        if explicit.iter().any(|&weight| weight > 12) { return Err("Zstandard Huffman weight is invalid".into()); }
        explicit.push(0);
        let last = explicit.len() - 1;
        let mut total = 0u32;
        for &weight in &explicit[..last] {
            if weight > 0 { total = total.saturating_add(1u32 << (weight - 1)); }
        }
        if total == 0 { explicit[last] = 1; return Ok(explicit); }
        let table_log = 32 - total.leading_zeros();
        if table_log >= 31 || (1u32 << table_log) < total { return Err("Zstandard Huffman weights are invalid".into()); }
        let rest = (1u32 << table_log) - total;
        if rest == 0 { return Err("Zstandard Huffman weights are oversubscribed".into()); }
        let implied = 32 - rest.leading_zeros();
        if implied == 0 || implied > 12 { return Err("Zstandard Huffman weight is invalid".into()); }
        explicit[last] = implied as u8;
        Ok(explicit)
    }
    fn fse_weight_symbols(payload: &[u8]) -> Result<Vec<u8>> {
        let mut pos = 0usize;
        let table = read_explicit_fse(payload, &mut pos, 12, 6)?;
        let stream = payload.get(pos..).ok_or("Truncated Zstandard Huffman weights")?;
        if stream.is_empty() { return Err("Truncated Zstandard Huffman weights".into()); }
        let mut bits = Back::new(stream)?;
        let mut state1 = table.prime(&mut bits)?;
        let mut state2 = table.prime(&mut bits)?;
        let mut out = Vec::new();
        loop {
            if out.len() >= 255 { return Err("Zstandard Huffman alphabet is invalid".into()); }
            stop_unless_running()?;
            let (first, overflow) = table.next_padded(&mut state1, &mut bits)?;
            out.push(first);
            if overflow {
                if out.len() >= 255 { return Err("Zstandard Huffman alphabet is invalid".into()); }
                let (second, _) = table.next_padded(&mut state2, &mut bits)?;
                out.push(second);
                break;
            }
            if out.len() >= 255 { return Err("Zstandard Huffman alphabet is invalid".into()); }
            let (second, overflow) = table.next_padded(&mut state2, &mut bits)?;
            out.push(second);
            if overflow {
                if out.len() >= 255 { return Err("Zstandard Huffman alphabet is invalid".into()); }
                let (third, _) = table.next_padded(&mut state1, &mut bits)?;
                out.push(third);
                break;
            }
        }
        if out.len() < 2 { return Err("Zstandard Huffman weights are invalid".into()); }
        Ok(out)
    }
    fn huffman_codes(weights: &[u8]) -> Result<Vec<(u32, u8)>> {
        let mut total = 0u32;
        for &weight in weights { if weight > 0 { total += 1u32 << (weight - 1); } }
        if !total.is_power_of_two() { return Err("Zstandard Huffman weights are incomplete".into()); }
        let table_log = total.trailing_zeros();
        let mut lengths = vec![0u8; weights.len()];
        let mut counts = [0usize; 16];
        let mut ranked = 0usize;
        for (symbol, &weight) in weights.iter().enumerate() {
            if weight > 0 {
                let bits = table_log as u8 + 1 - weight;
                if bits == 0 || bits > 11 { return Err("Zstandard Huffman code length is invalid".into()); }
                lengths[symbol] = bits;
                counts[bits as usize] += 1;
                ranked += 1;
            }
        }
        if ranked == 1 {
            return Ok(weights.iter().enumerate().map(|(symbol, weight)| if *weight > 0 { ((symbol as u32) + 1, 0) } else { (0, 0) }).collect());
        }
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for length in 1..16 {
            code = (code + counts[length - 1] as u32) << 1;
            next[length] = code;
        }
        let mut codes = vec![(0u32, 0u8); weights.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length > 0 {
                codes[symbol] = (next[length as usize], length);
                next[length as usize] += 1;
            }
        }
        Ok(codes)
    }
    fn huffman_stream(data: &[u8], codes: &[(u32, u8)], count: usize) -> Result<Vec<u8>> {
        if codes.iter().all(|(_, bits)| *bits == 0) {
            let symbol = codes.iter().position(|(code, _)| *code != 0).unwrap_or(0) as u8;
            if data.is_empty() { return Ok(vec![symbol; count]); }
            return Err("Zstandard single-symbol literals have extra data".into());
        }
        if data.is_empty() { return Err("Truncated Zstandard Huffman stream".into()); }
        let mut bits = Back::new(data)?;
        let max = codes.iter().map(|(_, width)| *width).max().unwrap_or(0);
        let mut out = Vec::with_capacity(count);
        while out.len() < count {
            stop_unless_running()?;
            let mut acc = 0u32;
            let mut found = None;
            for length in 1..=max {
                acc |= bits.take(1)? << (length - 1);
                if let Some(symbol) = codes.iter().position(|(code, width)| *width == length && *code == acc) {
                    found = Some(symbol as u8);
                    break;
                }
            }
            out.push(found.ok_or_else(|| "Zstandard Huffman symbol is invalid".to_string())?);
        }
        if !bits.exhausted() { return Err("Zstandard Huffman stream has trailing data".into()); }
        Ok(out)
    }
    fn append(out: &mut Vec<u8>, bytes: &[u8], expected: usize) -> Result<()> {
        if out.len().checked_add(bytes.len()).map(|n| n > expected).unwrap_or(true) { return Err("Zstandard member exceeds declared size".into()); }
        out.extend_from_slice(bytes);
        Ok(())
    }
}
fn idx(base: usize, add: usize) -> Result<usize> {
    base.checked_add(add).ok_or_else(|| "Archive offset overflow".into())
}
/// Info-ZIP Unicode Path (0x7075). A mismatched checksum is stale and is ignored.
fn unicode_path(extra: &[u8], header_name: &[u8]) -> Result<Option<String>> {
    let mut cursor = 0;
    let mut found = None;
    while cursor < extra.len() {
        let tag = read_u16(extra, cursor)?;
        let length = read_u16(extra, idx(cursor, 2)?)? as usize;
        let payload = span(extra, idx(cursor, 4)?, length)?;
        if tag == 0x7075 {
            if payload.first().copied() == Some(1) && payload.len() >= 5 {
                let crc = u32::from_le_bytes(payload[1..5].try_into().unwrap());
                if crc32(header_name) == crc {
                    if found.is_some() { return Err("Duplicate Unicode path".into()); }
                    let text = std::str::from_utf8(&payload[5..]).map_err(|_| "Unicode path is not UTF-8")?;
                    found = Some(text.to_string());
                }
            }
        }
        cursor = idx(cursor, 4)?.checked_add(length).ok_or("ZIP extra field overflow")?;
    }
    Ok(found)
}
fn compression_dictionary(parts: &[&[u8]]) -> Result<Vec<u8>> {
    let mut found: Option<Vec<u8>> = None;
    for extra in parts {
        let mut cursor = 0;
        while cursor < extra.len() {
            let tag = read_u16(extra, cursor)?;
            let length = read_u16(extra, idx(cursor, 2)?)? as usize;
            let payload = span(extra, idx(cursor, 4)?, length)?;
            if tag == 0x5A64 {
                if payload.len() < 4 { return Err("Compression dictionary is invalid".into()); }
                match &found {
                    Some(previous) if previous.as_slice() != payload => {
                        return Err("Compression dictionary is invalid".into());
                    }
                    Some(_) => {}
                    None => found = Some(payload.to_vec()),
                }
            }
            cursor = idx(cursor, 4)?.checked_add(length).ok_or("ZIP extra field overflow")?;
        }
    }
    Ok(found.unwrap_or_default())
}
fn supported_method(method: u16) -> bool {
    matches!(method, 0 | 1 | 2 | 3 | 4 | 5 | 6 | 8 | 9 | 12 | 14 | 20 | 93)
}
mod legacy {
    use super::{bit_end_fits, file_limit, stop_unless_running, zip_limit, Result};
    struct Lsb<'a> { bytes: &'a [u8], at: u64 }
    impl Lsb<'_> {
        fn take(&mut self, count: usize) -> Result<usize> {
            if count > 24 || !bit_end_fits(self.at, count as u64, self.bytes.len()) {
                return Err("Truncated compressed member".into());
            }
            let mut value = 0usize;
            for shift in 0..count {
                let byte = usize::try_from(self.at / 8).map_err(|_| "Compressed offset overflow")?;
                value |= (((self.bytes[byte] >> (self.at % 8)) & 1) as usize) << shift;
                self.at += 1;
            }
            Ok(value)
        }
        fn left(&self) -> u64 { (self.bytes.len() as u64).saturating_mul(8).saturating_sub(self.at) }
    }
    fn reverse16(mut value: u16) -> u16 {
        let mut out = 0u16;
        for _ in 0..16 {
            out = (out << 1) | (value & 1);
            value >>= 1;
        }
        out
    }
    fn shannon(lengths: &[usize]) -> Result<Vec<(u32, u8, u16)>> {
        if lengths.is_empty() || lengths.len() > 320 { return Err("Shannon-Fano tree is invalid".into()); }
        let mut order: Vec<usize> = (0..lengths.len()).collect();
        order.sort_by(|&a, &b| lengths[a].cmp(&lengths[b]).then(a.cmp(&b)));
        let mut code = 0u32;
        let mut increment = 0u32;
        let mut last = 0usize;
        let mut codes = vec![(0u32, 0u8, 0u16); lengths.len()];
        for slot in (0..order.len()).rev() {
            code = code.wrapping_add(increment);
            let width = lengths[order[slot]];
            if width != last {
                if width == 0 || width > 16 { return Err("Shannon-Fano code length is invalid".into()); }
                last = width;
                increment = 1u32 << (16 - width);
            }
            if code > 0xFFFF { return Err("Shannon-Fano code does not fit".into()); }
            let reversed = reverse16(code as u16) as u32;
            let mask = if width == 16 { 0xFFFF } else { (1u32 << width) - 1 };
            codes[order[slot]] = (reversed & mask, width as u8, order[slot] as u16);
        }
        codes.sort_by_key(|item| (item.1, item.0));
        for pair in codes.windows(2) {
            if pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1 {
                return Err("Shannon-Fano codes are ambiguous".into());
            }
        }
        Ok(codes)
    }
    fn symbol(tree: &[(u32, u8, u16)], bits: &mut Lsb<'_>) -> Result<u16> {
        let mut acc = 0u32;
        for width in 1..=16u8 {
            acc |= (bits.take(1)? as u32) << (width - 1);
            if let Some(found) = tree.iter().find(|item| item.1 == width && item.0 == acc) {
                return Ok(found.2);
            }
        }
        Err("Shannon-Fano symbol is invalid".into())
    }
    pub(super) fn shannon_matches_example() -> Result<()> {
        let codes = shannon(&[3, 3, 3, 3, 3, 2, 4, 4])?;
        let code = |symbol: u16| codes.iter().find(|item| item.2 == symbol).copied().ok_or("missing");
        let zero = code(0)?;
        let five = code(5)?;
        let six = code(6)?;
        if zero != (0b101, 3, 0) || five != (0b11, 2, 5) || six != (0b1000, 4, 6) {
            return Err("Shannon-Fano example does not match".into());
        }
        let packed = [0x08u8];
        let mut bits = Lsb { bytes: &packed, at: 0 };
        if symbol(&codes, &mut bits)? != 6 { return Err("Shannon-Fano bit order does not match".into()); }
        Ok(())
    }
    fn read_tree(bytes: &[u8], pos: &mut usize, count: usize) -> Result<Vec<(u32, u8, u16)>> {
        if *pos >= bytes.len() { return Err("Truncated Shannon-Fano tree".into()); }
        let records = bytes[*pos] as usize + 1;
        *pos += 1;
        let end = pos.checked_add(records).ok_or("Shannon-Fano tree is truncated")?;
        if end > bytes.len() { return Err("Truncated Shannon-Fano tree".into()); }
        let mut lengths = Vec::with_capacity(count);
        for &packed in &bytes[*pos..end] {
            let code_len = (packed & 0x0F) as usize + 1;
            let code_count = (packed >> 4) as usize + 1;
            if lengths.len() + code_count > count { return Err("Shannon-Fano tree is too long".into()); }
            lengths.resize(lengths.len() + code_count, code_len);
        }
        *pos = end;
        if lengths.len() != count { return Err("Shannon-Fano tree is incomplete".into()); }
        shannon(&lengths)
    }
    pub(super) fn shrink(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        if expected as u64 > file_limit() || bytes.len() as u64 > zip_limit() { return Err("Compressed member exceeds the memory budget".into()); }
        if expected == 0 { return if bytes.is_empty() { Ok(Vec::new()) } else { Err("Shrunk member has trailing data".into()) }; }
        let mut parent = [0xFFFFu16; 8192];
        let mut suffix = [0u8; 8192];
        let mut used = [false; 8192];
        for code in 0..256 { suffix[code] = code as u8; used[code] = true; }
        let mut bits = Lsb { bytes, at: 0 };
        let mut width = 9usize;
        let mut next_free = 257usize;
        let mut previous: Option<usize> = None;
        let mut out = Vec::new();
        out.try_reserve_exact(expected).map_err(|_| "Compressed member exceeds the memory budget")?;
        while out.len() < expected {
            if out.len() % 1024 == 0 { stop_unless_running()?; }
            let code = bits.take(width)?;
            if code == 256 {
                let control = bits.take(width)?;
                match control {
                    1 => {
                        if width >= 13 { return Err("Shrunk code size is too wide".into()); }
                        width += 1;
                    }
                    2 => {
                        let mut prefix = [false; 8192];
                        for node in 257..8192 {
                            if used[node] {
                                let prior = parent[node] as usize;
                                if prior < 8192 { prefix[prior] = true; }
                            }
                        }
                        for node in 257..8192 {
                            if used[node] && !prefix[node] { used[node] = false; }
                        }
                        next_free = (257..8192).find(|&node| !used[node]).unwrap_or(8192);
                    }
                    _ => return Err("Shrunk control code is invalid".into()),
                }
                continue;
            }
            let text = if code == next_free {
                let prior = previous.ok_or("Shrunk stream starts with an undefined code")?;
                let mut text = walk(prior, &parent, &suffix, &used)?;
                let first = *text.first().ok_or("Shrunk code is empty")?;
                text.push(first);
                text
            } else {
                walk(code, &parent, &suffix, &used)?
            };
            if out.len() + text.len() > expected { return Err("Shrunk member exceeds declared size".into()); }
            if let Some(prior) = previous {
                if next_free < 8192 {
                    parent[next_free] = prior as u16;
                    suffix[next_free] = text[0];
                    used[next_free] = true;
                    next_free += 1;
                    while next_free < 8192 && used[next_free] { next_free += 1; }
                }
            }
            previous = Some(code);
            out.extend_from_slice(&text);
        }
        if bits.left() >= width as u64 { return Err("Shrunk member has trailing data".into()); }
        Ok(out)
    }
    fn walk(mut code: usize, parent: &[u16], suffix: &[u8], used: &[bool]) -> Result<Vec<u8>> {
        if code >= 8192 || !used[code] { return Err("Shrunk code is undefined".into()); }
        let mut stack = Vec::new();
        for _ in 0..8192 {
            stack.push(suffix[code]);
            if parent[code] == 0xFFFF { break; }
            code = parent[code] as usize;
            if code >= 8192 { return Err("Shrunk code is undefined".into()); }
        }
        if parent[code] != 0xFFFF { return Err("Shrunk code loops".into()); }
        stack.reverse();
        Ok(stack)
    }
    fn bits_for(count: usize) -> usize {
        if count <= 1 { 0 } else { (usize::BITS as usize) - (count - 1).leading_zeros() as usize }
    }
    fn reduced_byte(bits: &mut Lsb<'_>, followers: &[Vec<u8>], last: &mut usize) -> Result<Option<u8>> {
        let count = followers[*last].len();
        if count == 0 {
            if bits.left() < 8 { return Ok(None); }
            let byte = bits.take(8)? as u8;
            *last = byte as usize;
            return Ok(Some(byte));
        }
        if bits.left() < 1 { return Ok(None); }
        if bits.take(1)? != 0 {
            if bits.left() < 8 { return Err("Truncated reduced member".into()); }
            let byte = bits.take(8)? as u8;
            *last = byte as usize;
            return Ok(Some(byte));
        }
        let width = bits_for(count);
        if bits.left() < width as u64 { return Err("Truncated reduced member".into()); }
        let index = if width == 0 { 0 } else { bits.take(width)? };
        if index >= count { return Err("Reduced follower index is invalid".into()); }
        let byte = followers[*last][index];
        *last = byte as usize;
        Ok(Some(byte))
    }
    fn reduced_padding(bits: &mut Lsb<'_>) -> Result<()> {
        if bits.left() == 0 { return Ok(()); }
        let leftover = bits.take(bits.left() as usize)?;
        if leftover != 0 { return Err("Reduced member has trailing data".into()); }
        Ok(())
    }
    // Follower bytes expand directly into the output. Factor 1..4 distance fields
    // reach 512, 1024, 2048, and 4096 bytes, so the pre-expansion bytes are not kept.
    fn expand_reduced_byte(factor: u16, state: &mut u8, len: &mut usize, marker: &mut usize, out: &mut Vec<u8>, expected: usize, value: usize) -> Result<()> {
        match *state {
            0 => {
                if value != 144 { out.push(value as u8); }
                else { *state = 1; }
            }
            1 => {
                if value == 0 {
                    out.push(144);
                    *state = 0;
                } else {
                    *marker = value;
                    *len = match factor {
                        1 => value & 0x7F,
                        2 => value & 0x3F,
                        3 => value & 0x1F,
                        _ => value & 0x0F,
                    };
                    let wide = match factor { 1 => 127, 2 => 63, 3 => 31, _ => 15 };
                    *state = if *len == wide { 2 } else { 3 };
                }
            }
            2 => { *len = len.saturating_add(value); *state = 3; }
            _ => {
                let high = match factor {
                    1 => (*marker >> 7) & 1,
                    2 => (*marker >> 6) & 3,
                    3 => (*marker >> 5) & 7,
                    _ => (*marker >> 4) & 0x0F,
                };
                let dist = high * 256 + value + 1;
                let copy = len.checked_add(3).ok_or("Reduced copy length overflows")?;
                if out.len().checked_add(copy).map(|n| n > expected).unwrap_or(true) {
                    return Err("Reduced member exceeds declared size".into());
                }
                let start = out.len() as isize - dist as isize;
                for step in 0..copy {
                    if step % 1024 == 0 { stop_unless_running()?; }
                    let src = start + step as isize;
                    let byte = if src < 0 { 0 } else { out[src as usize] };
                    out.push(byte);
                }
                *state = 0;
            }
        }
        Ok(())
    }
    pub(super) fn reduce(method: u16, bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        if !(2..=5).contains(&method) { return Err("Reduced compression factor is invalid".into()); }
        if expected as u64 > file_limit() || bytes.len() as u64 > zip_limit() { return Err("Compressed member exceeds the memory budget".into()); }
        let factor = method - 1;
        let mut bits = Lsb { bytes, at: 0 };
        let mut followers = vec![Vec::new(); 256];
        for state in (0..256).rev() {
            let count = bits.take(6)?;
            if count > 32 { return Err("Reduced follower set is invalid".into()); }
            let mut set = Vec::with_capacity(count);
            for _ in 0..count { set.push(bits.take(8)? as u8); }
            followers[state] = set;
        }
        let mut out = Vec::new();
        out.try_reserve_exact(expected).map_err(|_| "Compressed member exceeds the memory budget")?;
        let mut last = 0usize;
        let mut state = 0u8;
        let mut len = 0usize;
        let mut marker = 0usize;
        let mut decoded = 0usize;
        while out.len() < expected {
            if decoded % 1024 == 0 { stop_unless_running()?; }
            let Some(byte) = reduced_byte(&mut bits, &followers, &mut last)? else {
                reduced_padding(&mut bits)?;
                return Err("Truncated reduced member".into());
            };
            decoded += 1;
            expand_reduced_byte(factor, &mut state, &mut len, &mut marker, &mut out, expected, byte as usize)?;
        }
        if state != 0 { return Err("Reduced member has trailing data".into()); }
        match reduced_byte(&mut bits, &followers, &mut last)? {
            Some(_) => return Err("Reduced member has trailing data".into()),
            None => reduced_padding(&mut bits)?,
        }
        Ok(out)
    }
    pub(super) fn implode(flags: u16, bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
        if expected as u64 > file_limit() || bytes.len() as u64 > zip_limit() { return Err("Compressed member exceeds the memory budget".into()); }
        let window = if flags & 2 != 0 { 8192 } else { 4096 };
        let three = flags & 4 != 0;
        let min_match = if three { 3 } else { 2 };
        let mut pos = 0usize;
        let literal = if three { Some(read_tree(bytes, &mut pos, 256)?) } else { None };
        let length = read_tree(bytes, &mut pos, 64)?;
        let distance = read_tree(bytes, &mut pos, 64)?;
        let mut bits = Lsb { bytes: &bytes[pos..], at: 0 };
        let mut out = Vec::new();
        out.try_reserve_exact(expected).map_err(|_| "Compressed member exceeds the memory budget")?;
        while out.len() < expected {
            if out.len() % 1024 == 0 { stop_unless_running()?; }
            if bits.take(1)? != 0 {
                let byte = if let Some(tree) = &literal { symbol(tree, &mut bits)? as u8 } else { bits.take(8)? as u8 };
                out.push(byte);
            } else {
                let low_width = if window == 8192 { 7 } else { 6 };
                let low = bits.take(low_width)?;
                let high = symbol(&distance, &mut bits)? as usize;
                let back = ((high << low_width) | low).checked_add(1).ok_or("Imploded distance overflows")?;
                if back > window { return Err("Imploded distance is outside the dictionary".into()); }
                let mut copy = symbol(&length, &mut bits)? as usize + min_match;
                if copy == 63 + min_match { copy = copy.saturating_add(bits.take(8)?); }
                if out.len().checked_add(copy).map(|n| n > expected).unwrap_or(true) {
                    return Err("Imploded member exceeds declared size".into());
                }
                let start = out.len() as isize - back as isize;
                for step in 0..copy {
                    if step % 1024 == 0 { stop_unless_running()?; }
                    let src = start + step as isize;
                    let byte = if src < 0 { 0 } else { out[src as usize] };
                    out.push(byte);
                }
            }
        }
        if bits.left() >= 8 { return Err("Imploded member has trailing data".into()); }
        Ok(out)
    }
}
fn member_bytes(method: u16, flags: u16, compressed: &[u8], expanded: usize, dictionary: &[u8]) -> Result<Vec<u8>> {
    match method {
        0 => {
            let mut owned = Vec::new();
            owned.try_reserve_exact(expanded).map_err(|_| "Project exceeds the memory budget")?;
            owned.extend_from_slice(compressed);
            Ok(owned)
        }
        1 => legacy::shrink(compressed, expanded),
        2..=5 => legacy::reduce(method, compressed, expanded),
        6 => legacy::implode(flags, compressed, expanded),
        8 => raw_deflate::decode(compressed, expanded),
        9 => raw_deflate::decode_wide(compressed, expanded),
        12 => bzip2_member::decode(compressed, expanded),
        14 => lzma_member::decode(compressed, expanded),
        20 | 93 => zstd_member::decode_with(compressed, expanded, dictionary),
        _ => Err("ZIP compression method unsupported".into()),
    }
}
pub(crate) fn unpack(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    unpack_resident(bytes, 0)
}
fn unpack_resident(bytes: &[u8], outside: u64) -> Result<Vec<(String, Vec<u8>)>> {
    stop_unless_running()?;
    if bytes.len() < 22 {
        return Err("ZIP footer is missing or truncated".into());
    }
    let mut resident = outside.checked_add(bytes.len() as u64).ok_or("Archive size overflow")?;
    if bytes.len() as u64 > zip_limit() || resident > project_budget() {
        return Err("Project ZIP exceeds the memory budget".into());
    }
    let end = (bytes.len().saturating_sub(22 + 65535)..=bytes.len() - 22)
        .rev()
        .find(|&at| read_u32(bytes, at) == Ok(0x06054b50)
            && read_u16(bytes, idx(at, 20).unwrap_or(usize::MAX)).map(|n| idx(at, 22).ok().and_then(|base| idx(base, n as usize).ok()) == Some(bytes.len())).unwrap_or(false))
        .ok_or("ZIP footer is missing or truncated")?;
    if read_u16(bytes, end + 4)? != 0
        || read_u16(bytes, end + 6)? != 0
    {
        return Err("Multi-disk ZIP is unsupported".into());
    }
    let (count, offset, end) = directory_bounds(bytes, end)?;
    let mut cursor = offset;
    let mut entries = Vec::new();
    let mut ranges = Vec::new();
    let mut names = std::collections::BTreeMap::new();
    let mut spellings=std::collections::BTreeMap::new();
    for _ in 0..count {
        stop_unless_running()?;
        if read_u32(bytes, cursor)? != 0x02014b50 {
            return Err("Invalid ZIP directory entry".into());
        }
        let made = read_u16(bytes, idx(cursor, 4)?)?;
        let flags = read_u16(bytes, idx(cursor, 8)?)?;
        let method = read_u16(bytes, idx(cursor, 10)?)?;
        if (flags & !0x080e) != 0 || !supported_method(method) || (method == 0 && flags & 6 != 0) || (made >> 8) != 0 && (made >> 8) != 3 {
            return Err("ZIP encryption, compression or platform type unsupported".into());
        }
        let checksum = read_u32(bytes, idx(cursor, 16)?)?;
        let name_len = read_u16(bytes, idx(cursor, 28)?)? as usize;
        let extra_len = read_u16(bytes, idx(cursor, 30)?)? as usize;
        let comment_len = read_u16(bytes, idx(cursor, 32)?)? as usize;
        if extra_len > 4096 || comment_len > 4096 { return Err("Unsupported ZIP member metadata".into()); }
        let mut fields = [read_u32(bytes, idx(cursor, 24)?)? as u64, read_u32(bytes, idx(cursor, 20)?)? as u64,
            read_u32(bytes, idx(cursor, 42)?)? as u64, read_u16(bytes, idx(cursor, 34)?)? as u64];
        let central_zip64 = zip64_extra(span(bytes, idx(idx(cursor, 46)?, name_len)?, extra_len)?, &mut fields,
            &[u32::MAX as u64,u32::MAX as u64,u32::MAX as u64,u16::MAX as u64])?;
        if fields[3] != 0 { return Err("Multi-disk ZIP member is unsupported".into()); }
        let expanded = bounded_size(fields[0], file_limit())?;
        let stored = bounded_size(fields[1], bytes.len() as u64)?;
        let local = bounded_size(fields[2], offset as u64)?;
        if (method == 0 && stored != expanded) || expanded as u64 > file_limit() {
            return Err("ZIP member exceeds file limit".into());
        }
        // The input ZIP and every expanded copy are live together.
        charge(&mut resident, expanded as u64)?;
        let attrs = read_u32(bytes, idx(cursor, 38)?)?;
        let filetype = (attrs >> 16) & 0xf000;
        if !matches!(filetype,0|0x8000|0x4000) {
            return Err("ZIP link or special file rejected".into());
        }
        let name_bytes = span(bytes, idx(cursor, 46)?, name_len)?;
        let trailing=name_bytes.ends_with(b"/");
        let directory=trailing || attrs & 0x10 != 0 || filetype==0x4000;
        if directory && (filetype==0x8000 || expanded!=0 || checksum!=0) {
            return Err("ZIP directory contains file data or conflicting metadata".into());
        }
        let central_extra = span(bytes, idx(idx(cursor, 46)?, name_len)?, extra_len)?;
        let mut unicode = unicode_path(central_extra, name_bytes)?;
        if unicode.is_none() {
            let local_extra_len = read_u16(bytes, idx(local, 28)?)? as usize;
            if local_extra_len <= 4096 {
                if let Ok(local_extra) = span(bytes, idx(idx(local, 30)?, name_len)?, local_extra_len) {
                    unicode = unicode_path(local_extra, name_bytes)?;
                }
            }
        }
        let chosen = name_bytes.strip_suffix(b"/").unwrap_or(name_bytes);
        let canonical = if let Some(text) = unicode.as_deref() {
            accept_path(text.trim_end_matches('/'))?
        } else if flags & 0x800 != 0 {
            valid_name(chosen)?
        } else {
            accept_path(&legacy_zip_chars(chosen))?
        };
        let name=if directory {format!("{canonical}/")}else{canonical.clone()};
        let folded = canonical.to_lowercase();
        if names.insert(folded.clone(),directory).is_some() {
            return Err("Duplicate project path".into());
        }
        for boundary in canonical.char_indices().filter_map(|(i,c)|if c=='/'{Some(i)}else{None}).chain(std::iter::once(canonical.len())) {
            let prefix=&canonical[..boundary];let folded_prefix=prefix.to_lowercase();
            if !spellings.contains_key(&folded_prefix) {
                if spellings.len() as u64 >= prefix_limit() { return Err("Project exceeds the distinct path limit".into()); }
                charge(&mut resident, (folded_prefix.len() + prefix.len() + 96) as u64)?;
            }
            if let Some(previous)=spellings.insert(folded_prefix,prefix.to_string()) {
                if previous!=prefix {return Err("Ambiguous project path casing".into());}
            }
        }
        let descriptor = flags & 8 != 0;
        let local_crc = read_u32(bytes, idx(local, 14)?)?;
        let local_extra = read_u16(bytes, idx(local, 28)?)? as usize;
        if local_extra > 4096 { return Err("Unsupported ZIP local metadata".into()); }
        let mut local_fields = [read_u32(bytes, idx(local, 22)?)? as u64, read_u32(bytes, idx(local, 18)?)? as u64];
        let local_zip64 = zip64_extra(span(bytes, idx(idx(local, 30)?, name_len)?, local_extra)?, &mut local_fields,
            &[u32::MAX as u64,u32::MAX as u64])?;
        let local_expanded = local_fields[0]; let local_stored = local_fields[1];
        if read_u32(bytes, local)? != 0x04034b50
            || read_u16(bytes, idx(local, 6)?)? != flags
            || read_u16(bytes, idx(local, 8)?)? != method
            || !(local_crc == checksum || descriptor && local_crc == 0)
            || !(local_stored == stored as u64 || descriptor && local_stored == 0)
            || !(local_expanded == expanded as u64 || descriptor && local_expanded == 0)
            || read_u16(bytes, idx(local, 26)?)? as usize != name_len
        {
            return Err("ZIP local entry differs from directory".into());
        }
        if local_extra > 4096 || span(bytes, idx(local, 30)?, name_len)? != name_bytes {
            return Err("ZIP local path differs from directory".into());
        }
        let data_at = local
            .checked_add(30)
            .and_then(|n| n.checked_add(name_len))
            .and_then(|n| n.checked_add(local_extra))
            .ok_or("ZIP offset overflow")?;
        let compressed = span(bytes, data_at, stored)?;
        let mut data_end = data_at.checked_add(stored).ok_or("ZIP offset overflow")?;
        if descriptor {
            let wide = local_zip64 || central_zip64;
            let matches = |at: usize| idx(at, if wide { 16 } else { 12 }).ok().map(|end| end <= bytes.len()).unwrap_or(false) && read_u32(bytes, at) == Ok(checksum) && if wide {
                read_u64(bytes, idx(at, 4).unwrap_or(usize::MAX)) == Ok(stored as u64) && read_u64(bytes, idx(at, 12).unwrap_or(usize::MAX)) == Ok(expanded as u64)
            } else { read_u32(bytes, idx(at, 4).unwrap_or(usize::MAX)) == Ok(stored as u32) && read_u32(bytes, idx(at, 8).unwrap_or(usize::MAX)) == Ok(expanded as u32) };
            let base = if wide {20} else {12};
            let length = if read_u32(bytes, data_end) == Ok(0x08074b50) && idx(data_end, 4).ok().and_then(|at| matches(at).then_some(at)).is_some() { base + 4 }
                else if matches(data_end) { base }
                else { return Err("ZIP descriptor differs from directory".into()); };
            data_end = data_end.checked_add(length).ok_or("ZIP offset overflow")?;
        }
        if data_end > offset {
            return Err("ZIP member overlaps directory".into());
        }
        let local_dictionary = span(bytes, idx(idx(local, 30)?, name_len)?, local_extra)?;
        let dictionary = if matches!(method, 20 | 93) {
            compression_dictionary(&[central_extra, local_dictionary])?
        } else {
            Vec::new()
        };
        let data = member_bytes(method, flags, compressed, expanded, &dictionary)?;
        if crc32(&data) != checksum {
            return Err("ZIP member is damaged".into());
        }
        ranges.push((local, data_end));
        entries.push((name, data));
        cursor = cursor
            .checked_add(46)
            .and_then(|n| n.checked_add(name_len))
            .and_then(|n| n.checked_add(extra_len))
            .and_then(|n| n.checked_add(comment_len))
            .ok_or("ZIP directory overflow")?;
        if cursor > end {
            return Err("ZIP directory overflow".into());
        }
    }
    if cursor != end {
        return Err("ZIP directory contains extra data".into());
    }
    ranges.sort_unstable();
    let mut previous = 0;
    for (start, stop) in ranges {
        if start != previous {
            return Err("Overlapping or hidden ZIP data".into());
        }
        previous = stop;
    }
    if previous != offset {
        return Err("ZIP data and directory do not meet".into());
    }
    // Reject a file whose name becomes the parent of another, regardless of member order.
    for name in names.keys() {
        for (i, c) in name.char_indices() {
            if c == '/' && names.get(&name[..i])==Some(&false) {
                return Err("Project file conflicts with a directory".into());
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}
fn decode_archive_base64(text: &str) -> Result<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut padding_at = None;
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => {
                if padding_at.is_some() { return Err("Invalid encoded archive".into()); }
            }
            b'=' => { if padding_at.is_none() { padding_at = Some(index); } }
            _ => return Err("Invalid encoded archive".into()),
        }
    }
    let (body, padding) = if let Some(at) = padding_at {
        let padding = bytes.len() - at;
        if padding > 2 || bytes[at..].iter().any(|&byte| byte != b'=') || (at + padding) % 4 != 0 {
            return Err("Invalid encoded archive".into());
        }
        (&bytes[..at], padding)
    } else if bytes.len() % 4 == 1 {
        return Err("Invalid encoded archive".into());
    } else {
        (bytes, 0usize)
    };
    let rem = body.len() % 4;
    if rem == 1 || padding == 1 && rem != 3 || padding == 2 && rem != 2 {
        return Err("Invalid encoded archive".into());
    }
    let value = |byte: u8| -> u8 {
        match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            _ => 63,
        }
    };
    let out_len = body.len() / 4 * 3 + match rem { 2 => 1, 3 => 2, _ => 0 };
    let mut out = Vec::new();
    out.try_reserve_exact(out_len).map_err(|_| "Project exceeds the memory budget")?;
    let mut index = 0;
    while index + 4 <= body.len() {
        let n = ((value(body[index]) as u32) << 18)
            | ((value(body[index + 1]) as u32) << 12)
            | ((value(body[index + 2]) as u32) << 6)
            | (value(body[index + 3]) as u32);
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        index += 4;
    }
    if rem == 2 {
        let hi = value(body[index]) as u32;
        let lo = value(body[index + 1]) as u32;
        if lo & 0x0F != 0 { return Err("Invalid encoded archive".into()); }
        out.push(((hi << 2) | (lo >> 4)) as u8);
    } else if rem == 3 {
        let a = value(body[index]) as u32;
        let b = value(body[index + 1]) as u32;
        let c = value(body[index + 2]) as u32;
        if c & 0x03 != 0 { return Err("Invalid encoded archive".into()); }
        out.push(((a << 2) | (b >> 4)) as u8);
        out.push((((b & 0x0F) << 4) | (c >> 2)) as u8);
    }
    if out.len() != out_len { return Err("Invalid encoded archive".into()); }
    Ok(out)
}
fn open_encoded_archive(text: &str) -> Result<Vec<u8>> {
    let zip_cap = (text.len() as u64).saturating_add(3) / 4 * 3;
    admit_sum(&[text.len() as u64, zip_cap])?;
    decode_archive_base64(text)
}
pub fn preview(v: &Json) -> Result<Json> {
    let text = v.get("archive")?.text()?;
    let bytes = open_encoded_archive(text)?;
    let entries = unpack_resident(&bytes, text.len() as u64)?;
    let names = entries.iter().map(|(n, _)| s(n)).collect::<Vec<_>>();
    Ok(obj(vec![
        ("files", Num(entries.iter().filter(|(name,_)| !name.ends_with('/')).count().to_string())),
        ("directories", Num(entries.iter().filter(|(name,_)| name.ends_with('/')).count().to_string())),
        ("names", Arr(names)),
        ("bytes", Num(bytes.len().to_string())),
    ]))
}
pub fn import(v: &Json) -> Result<Json> {
    let text = v.get("archive")?.text()?;
    let bytes = open_encoded_archive(text)?;
    let entries = unpack_resident(&bytes, text.len() as u64)?;
    let root = Path::new(v.get("root")?.text()?);
    #[cfg(any(target_os="linux",target_os="android",target_os="macos"))]
    let(created,folders)=source::restore(root,&entries)?;
    #[cfg(windows)]
    let(created,folders)=windows_source::restore(root,&entries)?;
    #[cfg(not(any(windows, target_os="linux", target_os="android", target_os="macos")))]
    { let _=(root,entries); return Err("Descriptor-safe project import is unavailable on this target".into()); }
    Ok(obj(vec![("files",Num(created.to_string())),("directories",Num(folders.to_string()))]))
}

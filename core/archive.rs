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
// These bindings are the shared Linux/Android 64-bit libc ABI. Directory
// enumeration uses an independently opened descriptor, never /proc or paths.
#[cfg(all(any(target_os="linux", target_os="android"), target_pointer_width="64"))]
mod source {
    use super::*;
    use std::ffi::{CString, OsStr, OsString};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    const DIRECTORY:i32=65536;
    const NONBLOCK:i32=2048;
    const PATH_ONLY:i32=2097152;
    unsafe extern "C" {
        fn openat(fd:i32,path:*const std::ffi::c_char,flags:i32,...)->i32;
        fn fdopendir(fd:i32)->*mut std::ffi::c_void;
        fn readdir(dir:*mut std::ffi::c_void)->*mut u8;
        fn closedir(dir:*mut std::ffi::c_void)->i32;
        fn close(fd:i32)->i32;
        fn mkdirat(fd:i32,path:*const std::ffi::c_char,mode:u32)->i32;
        #[cfg(target_os="linux")] fn __errno_location()->*mut i32;
        #[cfg(target_os="android")] fn __errno()->*mut i32;
    }
    fn error_slot()->*mut i32 {
        #[cfg(target_os="linux")] {unsafe {__errno_location()}}
        #[cfg(target_os="android")] {unsafe {__errno()}}
    }
    fn open(fd:i32,name:&OsStr,flags:i32)->Result<fs::File> {
        let name=CString::new(name.as_bytes()).map_err(|_|"Invalid project path")?;
        let raw=unsafe {openat(fd,name.as_ptr(),flags|131072|524288)};
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
            // 64-bit Linux/Android dirent: ino64, off64, reclen16, type8,
            // then d_name. Read only this variable-sized returned record;
            // creating a full struct reference could exceed its allocation.
            let length=unsafe {std::ptr::read_unaligned(p.add(16).cast::<u16>())} as usize;
            let bound=length.checked_sub(19).ok_or("Invalid directory record")?.min(256);
            let name_bytes=unsafe {std::slice::from_raw_parts(p.add(19),bound)};
            let end=name_bytes.iter().position(|&x|x==0).ok_or("Unterminated directory name")?;
            let bytes=&name_bytes[..end];if bytes==b"."||bytes==b".." {continue;}
            if names.len()>=500 {return Err("Project folder exceeds 500 entries".into());}
            let name=std::str::from_utf8(bytes).map_err(|_|"Project filename is not UTF-8")?;
            valid_name(bytes)?;names.push(name.to_string());
        }
        names.sort();
        if names.windows(2).any(|n|n[0]==n[1]) {return Err("Project directory changed during enumeration".into());}
        Ok(names)
    }
    struct Anchor {file:fs::File,meta:fs::Metadata,name:OsString}
    struct Node {file:fs::File,meta:fs::Metadata,parent:usize,name:String,path:String,children:Option<Vec<String>>}
    pub(super) fn archive(root:&Path)->Result<(Vec<u8>,usize,usize)> {
        // Resolve deliberate Android/root aliases once, then walk only no-follow
        // descriptors. Re-resolution at the end checks identity, never reads data.
        let resolved=root.canonicalize().map_err(|e|e.to_string())?;
        let first=open(-100,OsStr::new("/"),DIRECTORY|PATH_ONLY)?;
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
        let file=anchors.last().unwrap().file.try_clone().map_err(|e|e.to_string())?;
        let meta=file.metadata().map_err(|e|e.to_string())?;
        let mut nodes=vec![Node{file,meta,parent:0,name:String::new(),path:String::new(),children:None}];
        let mut pending=vec![0usize];let mut total=0usize;
        #[cfg(test)] super::source_hook("root_opened");
        while let Some(index)=pending.pop() {
            let children=names(&nodes[index].file)?;
            #[cfg(test)] super::source_hook("enumerated");
            nodes[index].children=Some(children.clone());
            for name in children {
                if nodes.len()>500 {return Err("Project archive exceeds 500 entries".into());}
                let file=open(nodes[index].file.as_raw_fd(),OsStr::new(&name),NONBLOCK)?;
                let meta=file.metadata().map_err(|e|e.to_string())?;
                let path=if nodes[index].path.is_empty(){name.clone()}else{format!("{}/{}",nodes[index].path,name)};
                valid_name(path.as_bytes())?;
                if meta.is_dir() {
                    pending.push(nodes.len());
                } else if meta.is_file() {
                    if meta.len()>1024*1024 {return Err("Project file exceeds 1 MiB archive limit".into());}
                    total=total.checked_add(meta.len() as usize).ok_or("Project archive size overflow")?;
                    if total>4*1024*1024 {return Err("Project archive exceeds 4 MiB data limit".into());}
                } else {return Err("Project contains an unsupported file type".into());}
                nodes.push(Node{file,meta,parent:index,name,path,children:None});
            }
        }
        // The complete descriptor inventory precedes payload allocation. Read
        // sorted held files directly into the final ZIP with one fixed scratch
        // buffer; no member-sized or project-sized intermediate payload exists.
        let mut order:Vec<usize>=(1..nodes.len()).collect();
        order.sort_by(|&a,&b| {
            let name=|i:usize| if nodes[i].meta.is_dir(){format!("{}/",nodes[i].path)}else{nodes[i].path.clone()};
            name(a).cmp(&name(b))
        });
        if order.is_empty(){return Err("No project entries to export".into());}
        let mut length=22usize;
        for &i in &order {
            let node=&nodes[i];let name_len=node.path.len()+usize::from(node.meta.is_dir());
            length=length.checked_add(76+name_len*2).and_then(|n|n.checked_add(if node.meta.is_file(){node.meta.len() as usize}else{0})).ok_or("Archive size overflow")?;
        }
        if length>5*1024*1024 {return Err("Project archive exceeds 5 MiB snapshot limit".into());}
        let mut out=Vec::new();out.try_reserve_exact(length).map_err(|_|"Cannot allocate project archive")?;
        let mut directory=Vec::new();let mut files=0usize;let mut directories=0usize;
        let mut scratch=[0u8;64*1024];
        for &i in &order {
            let node=&nodes[i];let is_dir=node.meta.is_dir();
            let name=if is_dir{directories+=1;format!("{}/",node.path)}else{files+=1;node.path.clone()};
            let size=if is_dir{0}else{u32::try_from(node.meta.len()).map_err(|_|"File too large")?};
            let offset=u32::try_from(out.len()).map_err(|_|"Archive too large")?;
            let name_length=u16::try_from(name.len()).map_err(|_|"Filename too long")?;
            long(&mut out,0x04034b50);short(&mut out,20);short(&mut out,0x0800);
            short(&mut out,0);short(&mut out,0);short(&mut out,0);
            long(&mut out,0);long(&mut out,size);long(&mut out,size);
            short(&mut out,name_length);short(&mut out,0);out.extend_from_slice(name.as_bytes());
            let mut crc=!0u32;
            if !is_dir {
                #[cfg(test)] super::source_hook("file_opened");
                let mut reader=&node.file;let mut remaining=size as usize;
                while remaining>0 {
                    let count=remaining.min(scratch.len());
                    reader.read_exact(&mut scratch[..count]).map_err(|e|e.to_string())?;
                    crc=crc_update(crc,&scratch[..count]);out.extend_from_slice(&scratch[..count]);remaining-=count;
                    #[cfg(test)] super::source_hook("file_chunk");
                }
                let mut extra=[0u8;1];
                if reader.read(&mut extra).map_err(|e|e.to_string())?!=0 || !stable(&node.meta,&node.file.metadata().map_err(|e|e.to_string())?) {
                    return Err("Project file changed during archive read".into());
                }
            }
            let crc=!crc;let checksum=offset as usize+14;out[checksum..checksum+4].copy_from_slice(&crc.to_le_bytes());
            long(&mut directory,0x02014b50);short(&mut directory,20);short(&mut directory,20);short(&mut directory,0x0800);
            short(&mut directory,0);short(&mut directory,0);short(&mut directory,0);
            long(&mut directory,crc);long(&mut directory,size);long(&mut directory,size);
            short(&mut directory,name_length);short(&mut directory,0);short(&mut directory,0);short(&mut directory,0);short(&mut directory,0);
            long(&mut directory,if is_dir{0x10}else{0});long(&mut directory,offset);directory.extend_from_slice(name.as_bytes());
        }
        let offset=u32::try_from(out.len()).map_err(|_|"Archive too large")?;
        let directory_size=u32::try_from(directory.len()).map_err(|_|"Archive too large")?;
        out.extend_from_slice(&directory);long(&mut out,0x06054b50);short(&mut out,0);short(&mut out,0);
        let count=u16::try_from(order.len()).map_err(|_|"Too many files")?;
        short(&mut out,count);short(&mut out,count);long(&mut out,directory_size);long(&mut out,offset);short(&mut out,0);
        if out.len()!=length {return Err("Archive length mismatch".into());}
        #[cfg(test)] super::source_hook("before_validation");
        // Validate the complete pinned inventory before publishing any ZIP. A
        // rename cannot redirect reads, and a replaced binding cannot pass.
        for (i,node) in nodes.iter().enumerate() {
            if !stable(&node.meta,&node.file.metadata().map_err(|e|e.to_string())?) {return Err("Project changed during archive collection".into());}
            if i!=0 {
                let current=open(nodes[node.parent].file.as_raw_fd(),OsStr::new(&node.name),NONBLOCK)?;
                if !stable(&node.meta,&current.metadata().map_err(|e|e.to_string())?) {return Err("Project entry was replaced during archive collection".into());}
            }
            if let Some(expected)=&node.children {
                if &names(&node.file)?!=expected {return Err("Project inventory changed during archive collection".into());}
                if !stable(&node.meta,&node.file.metadata().map_err(|e|e.to_string())?) {return Err("Project directory changed during archive validation".into());}
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
        let raw=unsafe{openat(fd,name.as_ptr(),flags|131072|524288)};
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
                if identities.len()>=4096{return Err("Import directory metadata exceeds bound".into());}
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
        let file=open(-100,OsStr::new("/"),DIRECTORY|PATH_ONLY)?;let meta=file.metadata().map_err(|e|e.to_string())?;
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
        #[cfg(test)] super::source_hook("import_preflight");
        let mut created=0usize;let mut folders=0usize;
        for (name,data) in entries {
            let operation=(||->Result<()>{
                bindings(&nodes)?;
                let path=Path::new(name.trim_end_matches('/'));let directory=name.ends_with('/');
                let parent_path=if directory{path}else{path.parent().ok_or("Invalid project parent")?};
                let(chain,made)=target_path(&nodes[root_index].file,parent_path,true,&mut identities)?;folders+=made;
                let chain=chain.ok_or("Missing destination directory")?;bindings(&chain)?;
                if directory{return Ok(());}
                let at=chain.len()-1;
                #[cfg(test)] super::source_hook("import_parent");
                let name=CString::new(path.file_name().ok_or("Invalid project file")?.as_bytes()).map_err(|_|"Invalid project filename")?;
                let raw=unsafe{openat(chain[at].file.as_raw_fd(),name.as_ptr(),1|64|128|131072|524288|NONBLOCK,0o600u32)};
                if raw<0{return Err(std::io::Error::last_os_error().to_string());}
                let mut file=unsafe{fs::File::from_raw_fd(raw)};let first=file.metadata().map_err(|e|e.to_string())?;
                if !first.is_file(){return Err("Destination is not a regular file".into());}
                file.write_all(data).and_then(|_|file.sync_all()).map_err(|e|format!("New file may be partial: {e}"))?;
                let check=open(chain[at].file.as_raw_fd(),path.file_name().unwrap(),PATH_ONLY)?;let meta=check.metadata().map_err(|e|e.to_string())?;
                if first.dev()!=meta.dev()||first.ino()!=meta.ino()||meta.len()!=data.len() as u64{return Err("New destination file was replaced".into());}
                bindings(&chain)?;bindings(&nodes)?;created+=1;Ok(())
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
#[cfg(test)]
thread_local! {static SOURCE_HOOK:std::cell::RefCell<Option<Box<dyn FnMut(&str)>>> = std::cell::RefCell::new(None);}
#[cfg(test)]
fn source_hook(phase:&str) {SOURCE_HOOK.with(|h|{if let Some(h)=h.borrow_mut().as_mut(){h(phase);}});}
fn source_archive(root:&Path)->Result<(Vec<u8>,usize,usize)> {
    #[cfg(all(any(target_os="linux", target_os="android"), target_pointer_width="64"))]
    {source::archive(root)}
    #[cfg(not(all(any(target_os="linux", target_os="android"), target_pointer_width="64")))]
    {let _=root;Err("Descriptor-safe project archives are unavailable on this target".into())}
}
#[cfg(test)]
fn collect(root:&Path)->Result<Vec<(String,Vec<u8>)>> {unpack(&source_archive(root)?.0)}
#[cfg(test)]
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
        short(&mut out, 0);
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
        short(&mut directory, 0);
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
    let offset = u32::try_from(out.len()).map_err(|_| "Archive too large")?;
    let size = u32::try_from(directory.len()).map_err(|_| "Archive too large")?;
    out.extend_from_slice(&directory);
    long(&mut out, 0x06054b50);
    short(&mut out, 0);
    short(&mut out, 0);
    let count = u16::try_from(entries.len()).map_err(|_| "Too many files")?;
    short(&mut out, count);
    short(&mut out, count);
    long(&mut out, size);
    long(&mut out, offset);
    short(&mut out, 0);
    Ok(out)
}
pub fn build(v: &Json) -> Result<Json> {
    let root = Path::new(v.get("root")?.text()?);
    let (bytes,files,directories) = source_archive(root)?;
    Ok(obj(vec![
        ("archive", s(&network::b64(&bytes))),
        ("files", Num(files.to_string())),
        ("directories", Num(directories.to_string())),
        ("bytes", Num(bytes.len().to_string())),
    ]))
}
#[cfg(test)]
mod checks {
    use super::*;
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
    #[test]
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
    #[test]
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
    #[test]
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    fn destination_descriptors_refuse_parent_swaps_without_following_links() {
        use std::os::unix::fs::symlink;
        for phase in ["import_preflight","import_parent"] {
            let base=std::env::temp_dir().join(format!("rllm-import-swap-{}-{}",std::process::id(),CASE_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
            let root=base.join("project");fs::create_dir_all(root.join("folder")).unwrap();fs::create_dir_all(base.join("outside")).unwrap();
            let captured=base.clone();let mut changed=false;
            SOURCE_HOOK.with(|hook|*hook.borrow_mut()=Some(Box::new(move |at|{if at==phase&&!changed{changed=true;fs::rename(captured.join("project/folder"),captured.join("held-folder")).unwrap();symlink(captured.join("outside"),captured.join("project/folder")).unwrap();}})));
            let result=source::restore(&root,&[("folder/data".into(),b"payload".to_vec())]);SOURCE_HOOK.with(|hook|*hook.borrow_mut()=None);
            assert!(result.is_err());assert!(!base.join("outside/data").exists());
        }
    }
    #[test]
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
    #[test]
    fn incremental_crc_matches_known_vector_at_all_boundaries() {
        assert_eq!(crc32(b"123456789"),0xcbf43926);
        let bytes:Vec<u8>=(0..131079).map(|i|(i*37) as u8).collect();
        for size in [1,7,65535,65536,65537,131079] {
            let mut crc=!0;
            for chunk in bytes.chunks(size) {crc=crc_update(crc,chunk);}
            assert_eq!(!crc,crc32(&bytes));
        }
    }
    #[test]
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
    #[test]
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
    #[test]
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
    #[test]
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
    #[test]
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
    #[test]
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
    #[test]
    fn directory_metadata_and_path_conflicts_are_checked() {
        let bytes=zip(&[("folder".into(),Vec::new())]).unwrap();let directory=36;
        let mut dos=bytes.clone();dos[directory+38..directory+42].copy_from_slice(&0x10u32.to_le_bytes());assert_eq!(unpack(&dos).unwrap(),vec![("folder/".into(),Vec::new())]);
        let mut unix=bytes;unix[directory+4..directory+6].copy_from_slice(&0x0314u16.to_le_bytes());unix[directory+38..directory+42].copy_from_slice(&0x41ed0000u32.to_le_bytes());assert_eq!(unpack(&unix).unwrap(),vec![("folder/".into(),Vec::new())]);
        for entries in [vec![("folder/".into(),b"bad".to_vec())],vec![("folder".into(),vec![]),("folder/child".into(),vec![])],vec![("folder/child".into(),vec![]),("folder".into(),vec![])],vec![("folder/".into(),vec![]),("folder".into(),vec![])],vec![("Folder/a".into(),vec![]),("folder/b".into(),vec![])],vec![("../folder/".into(),vec![])],vec![("folder//".into(),vec![])]] {assert!(unpack(&zip(&entries).unwrap()).is_err());}
        let mut conflict=zip(&[("folder/".into(),Vec::new())]).unwrap();let at=37;conflict[at+38..at+42].copy_from_slice(&0x81a40000u32.to_le_bytes());assert!(unpack(&conflict).is_err());
    }
    #[test]
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
fn bounded_size(value: u64, limit: usize) -> Result<usize> {
    if value > limit as u64 { return Err("ZIP64 value exceeds project limit".into()); }
    Ok(value as usize)
}
// APPNOTE 4.5.3: only sentinel fields are replaced, in the defined order.
// Unknown extra fields remain inert metadata; every field must fit its envelope.
fn zip64_extra(extra: &[u8], fields: &mut [u64], sentinels: &[u64]) -> Result<bool> {
    let mut cursor = 0; let mut found = false;
    while cursor < extra.len() {
        let tag = read_u16(extra, cursor)?;
        let length = read_u16(extra, cursor + 2)? as usize;
        let payload = span(extra, cursor + 4, length)?;
        if tag == 1 {
            if found { return Err("Duplicate ZIP64 metadata".into()); }
            found = true; let mut at = 0;
            for (index, field) in fields.iter_mut().enumerate() {
                if *field == sentinels[index] {
                    if sentinels[index] == u16::MAX as u64 {
                        *field = read_u32(payload, at)? as u64; at += 4;
                    } else { *field = read_u64(payload, at)?; at += 8; }
                }
            }
            if at != payload.len() { return Err("Unexpected ZIP64 metadata fields".into()); }
        }
        cursor += 4 + length;
    }
    if fields.iter().zip(sentinels).any(|(value, sentinel)| value == sentinel) {
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
        let record = bounded_size(read_u64(bytes, locator + 8)?, locator)?;
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
        if disk_count != total_count { return Err("Invalid ZIP entry count".into()); }
        (total_count,size32,offset32,end)
    };
    let count = bounded_size(count,500)?;
    let size = bounded_size(size,bytes.len())?;
    let offset = bounded_size(offset,bytes.len())?;
    if count == 0 || offset.checked_add(size) != Some(boundary) {
        return Err("Invalid ZIP directory bounds".into());
    }
    Ok((count,offset,boundary))
}
fn span<'a>(bytes: &'a [u8], at: usize, len: usize) -> Result<&'a [u8]> {
    let end = at.checked_add(len).ok_or("Archive offset overflow")?;
    bytes.get(at..end).ok_or("Truncated archive".into())
}
fn valid_name(bytes: &[u8]) -> Result<String> {
    let name = std::str::from_utf8(bytes).map_err(|_| "Project path is not UTF-8")?;
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
fn unpack(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    if bytes.len() > 5 * 1024 * 1024 || bytes.len() < 22 {
        return Err("Project ZIP must be at most 5 MiB".into());
    }
    let end = (bytes.len().saturating_sub(22 + 65535)..=bytes.len() - 22)
        .rev()
        .find(|&at| read_u32(bytes, at) == Ok(0x06054b50)
            && read_u16(bytes, at + 20).map(|n| at + 22 + n as usize == bytes.len()).unwrap_or(false))
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
    let mut total = 0usize;
    for _ in 0..count {
        if read_u32(bytes, cursor)? != 0x02014b50 {
            return Err("Invalid ZIP directory entry".into());
        }
        let made = read_u16(bytes, cursor + 4)?;
        let flags = read_u16(bytes, cursor + 8)?;
        let method = read_u16(bytes, cursor + 10)?;
        if (flags & !0x080e) != 0 || !matches!(method, 0 | 8) || (method == 0 && flags & 6 != 0) || (made >> 8) != 0 && (made >> 8) != 3 {
            return Err("ZIP encryption, compression or platform type unsupported".into());
        }
        let checksum = read_u32(bytes, cursor + 16)?;
        let name_len = read_u16(bytes, cursor + 28)? as usize;
        let extra_len = read_u16(bytes, cursor + 30)? as usize;
        let comment_len = read_u16(bytes, cursor + 32)? as usize;
        if extra_len > 4096 || comment_len > 4096 { return Err("Unsupported ZIP member metadata".into()); }
        let mut fields = [read_u32(bytes,cursor+24)? as u64,read_u32(bytes,cursor+20)? as u64,
            read_u32(bytes,cursor+42)? as u64,read_u16(bytes,cursor+34)? as u64];
        let central_zip64 = zip64_extra(span(bytes,cursor+46+name_len,extra_len)?, &mut fields,
            &[u32::MAX as u64,u32::MAX as u64,u32::MAX as u64,u16::MAX as u64])?;
        if fields[3] != 0 { return Err("Multi-disk ZIP member is unsupported".into()); }
        let expanded = bounded_size(fields[0],1024*1024)?;
        let stored = bounded_size(fields[1],bytes.len())?;
        let local = bounded_size(fields[2],offset)?;
        if (method == 0 && stored != expanded) || expanded > 1024 * 1024 {
            return Err("ZIP member exceeds file limit".into());
        }
        total = total.checked_add(expanded).ok_or("ZIP size overflow")?;
        if total > 4 * 1024 * 1024 {
            return Err("Project exceeds 4 MiB data limit".into());
        }
        let attrs = read_u32(bytes, cursor + 38)?;
        let filetype = (attrs >> 16) & 0xf000;
        if !matches!(filetype,0|0x8000|0x4000) {
            return Err("ZIP link or special file rejected".into());
        }
        let name_bytes = span(bytes, cursor + 46, name_len)?;
        let trailing=name_bytes.ends_with(b"/");
        let directory=trailing || attrs & 0x10 != 0 || filetype==0x4000;
        if directory && (filetype==0x8000 || expanded!=0 || checksum!=0) {
            return Err("ZIP directory contains file data or conflicting metadata".into());
        }
        let canonical=valid_name(if trailing {&name_bytes[..name_bytes.len()-1]} else {name_bytes})?;
        let name=if directory {format!("{canonical}/")}else{canonical.clone()};
        let folded = canonical.to_lowercase();
        if names.insert(folded.clone(),directory).is_some() {
            return Err("Duplicate project path".into());
        }
        for boundary in canonical.char_indices().filter_map(|(i,c)|if c=='/'{Some(i)}else{None}).chain(std::iter::once(canonical.len())) {
            let prefix=&canonical[..boundary];let folded_prefix=prefix.to_lowercase();
            if !spellings.contains_key(&folded_prefix)&&spellings.len()>=4096 {return Err("Project exceeds 4096 distinct paths".into());}
            if let Some(previous)=spellings.insert(folded_prefix,prefix.to_string()) {
                if previous!=prefix {return Err("Ambiguous project path casing".into());}
            }
        }
        let descriptor = flags & 8 != 0;
        let local_crc = read_u32(bytes, local + 14)?;
        let local_extra = read_u16(bytes, local + 28)? as usize;
        if local_extra > 4096 { return Err("Unsupported ZIP local metadata".into()); }
        let mut local_fields = [read_u32(bytes,local+22)? as u64,read_u32(bytes,local+18)? as u64];
        let local_zip64 = zip64_extra(span(bytes,local+30+name_len,local_extra)?, &mut local_fields,
            &[u32::MAX as u64,u32::MAX as u64])?;
        let local_expanded = local_fields[0]; let local_stored = local_fields[1];
        if read_u32(bytes, local)? != 0x04034b50
            || read_u16(bytes, local + 6)? != flags
            || read_u16(bytes, local + 8)? != method
            || !(local_crc == checksum || descriptor && local_crc == 0)
            || !(local_stored == stored as u64 || descriptor && local_stored == 0)
            || !(local_expanded == expanded as u64 || descriptor && local_expanded == 0)
            || read_u16(bytes, local + 26)? as usize != name_len
        {
            return Err("ZIP local entry differs from directory".into());
        }
        if local_extra > 4096 || span(bytes, local + 30, name_len)? != name_bytes {
            return Err("ZIP local path differs from directory".into());
        }
        let data_at = local
            .checked_add(30)
            .and_then(|n| n.checked_add(name_len))
            .and_then(|n| n.checked_add(local_extra))
            .ok_or("ZIP offset overflow")?;
        let compressed = span(bytes, data_at, stored)?;
        let mut data_end = data_at + stored;
        if descriptor {
            let wide = local_zip64 || central_zip64;
            let matches = |at| read_u32(bytes, at) == Ok(checksum) && if wide {
                read_u64(bytes, at + 4) == Ok(stored as u64) && read_u64(bytes, at + 12) == Ok(expanded as u64)
            } else { read_u32(bytes, at + 4) == Ok(stored as u32) && read_u32(bytes, at + 8) == Ok(expanded as u32) };
            let base = if wide {20} else {12};
            let length = if read_u32(bytes, data_end) == Ok(0x08074b50) && matches(data_end + 4) { base + 4 }
                else if matches(data_end) { base }
                else { return Err("ZIP descriptor differs from directory".into()); };
            data_end += length;
        }
        if data_end > offset {
            return Err("ZIP member overlaps directory".into());
        }
        let data = if method == 8 { compression::inflate(compressed, expanded)? } else { compressed.to_vec() };
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
pub fn preview(v: &Json) -> Result<Json> {
    let bytes = network::unbase64(v.get("archive")?.text()?)?;
    let entries = unpack(&bytes)?;
    let names = entries.iter().map(|(n, _)| s(n)).collect::<Vec<_>>();
    Ok(obj(vec![
        ("files", Num(entries.iter().filter(|(name,_)| !name.ends_with('/')).count().to_string())),
        ("directories", Num(entries.iter().filter(|(name,_)| name.ends_with('/')).count().to_string())),
        ("names", Arr(names)),
        ("bytes", Num(bytes.len().to_string())),
    ]))
}
pub fn import(v: &Json) -> Result<Json> {
    let bytes = network::unbase64(v.get("archive")?.text()?)?;
    let entries = unpack(&bytes)?;
    let root = Path::new(v.get("root")?.text()?);
    #[cfg(all(any(target_os="linux",target_os="android"),target_pointer_width="64"))]
    let(created,folders)=source::restore(root,&entries)?;
    #[cfg(not(all(any(target_os="linux",target_os="android"),target_pointer_width="64")))]
    let(created,folders)={let _=(root,entries);return Err("Descriptor-safe project import is unavailable on this target".into());(0usize,0usize)};
    Ok(obj(vec![("files",Num(created.to_string())),("directories",Num(folders.to_string()))]))
}

//! Selected-directory observations, using held Linux/Android directory identities.
//! This module neither grants access nor runs observed code. Policy comes from
//! its trusted caller; Ask and Deny omit the affected observation.
use std::{ffi::{CString, OsString}, fs::{self, File, Metadata, OpenOptions},
    io::Read, os::{fd::{AsRawFd, FromRawFd}, unix::{ffi::{OsStrExt, OsStringExt}, fs::{MetadataExt, OpenOptionsExt}}},
    path::{Component, Path, PathBuf}, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access { ListDirectory, ReadHeader }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision { Allow, Ask, Deny }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Directory, File, Symlink, Other, Unknown }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage { Complete, Partial, Changed }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub dev: u64, pub ino: u64, pub size: u64,
    pub modified_seconds: i64, pub modified_nanos: i64,
    pub changed_seconds: i64, pub changed_nanos: i64,
}
impl Identity {
    fn of(m: &Metadata) -> Self { Self { dev:m.dev(), ino:m.ino(), size:m.len(),
        modified_seconds:m.mtime(), modified_nanos:m.mtime_nsec(),
        changed_seconds:m.ctime(), changed_nanos:m.ctime_nsec() } }
    fn same_object(&self, other: &Self) -> bool { self.dev==other.dev && self.ino==other.ino }
}
#[derive(Clone, Debug)]
pub struct Sample { pub offset: u64, pub bytes: Vec<u8>, pub complete_file: bool }
#[derive(Clone, Debug)]
pub struct Occurrence {
    /// Exact workspace-relative path bytes. A lossy display name is never identity.
    pub path: Vec<u8>, pub kind: Kind, pub identity: Option<Identity>,
    pub sample: Option<Sample>, pub format_candidates: Vec<&'static str>,
    pub coverage: Coverage, pub notes: Vec<&'static str>,
    listing_observed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contains { pub parent: usize, pub child: usize }
#[derive(Debug)]
pub struct Report {
    pub selected: Vec<u8>, pub workspace_identity: Identity,
    pub occurrences: Vec<Occurrence>, pub contains: Vec<Contains>,
    pub coverage: Coverage, pub notes: Vec<&'static str>, pub sampled_bytes: usize, pub bytes_read: usize,
}
impl Report {
    /// Querying a retained report performs no filesystem reads or rescans.
    pub fn page(&self, offset: usize, count: usize) -> &[Occurrence] {
        let start=offset.min(self.occurrences.len());
        &self.occurrences[start..start.saturating_add(count).min(self.occurrences.len())]
    }
    pub fn children(&self, parent: usize) -> impl Iterator<Item=&Occurrence> {
        self.contains.iter().filter(move |e|e.parent==parent).map(|e|&self.occurrences[e.child])
    }
    pub fn find(&self, raw_path: &[u8]) -> Option<&Occurrence> {
        self.occurrences.binary_search_by(|o|o.path.as_slice().cmp(raw_path)).ok().map(|i|&self.occurrences[i])
    }
    /// A bounded human-readable projection; the typed report retains exact samples.
    pub fn describe(&self, max_bytes: usize) -> String {
        let limit=max_bytes.min(120_000);
        let mut out=format!("Selected directory observation; coverage {:?}; {} retained occurrences; {} sampled bytes.\nNames are escaped workspace-relative text where UTF-8 is valid, otherwise exact hex bytes. Signatures are candidates, not format validation. Prefix samples do not establish whole-file identity. This is not an atomic filesystem snapshot.\n",self.coverage,self.occurrences.len(),self.sampled_bytes);
        if out.len()>limit { out.truncate(out.floor_char_boundary(limit));return out; }
        for (index,o) in self.occurrences.iter().enumerate() {
            let parent=self.contains.iter().find(|edge|edge.child==index).map(|edge|edge.parent.to_string()).unwrap_or_else(||"none".into());
            let row=format!("[{index}] {} {:?} {:?}; parent {parent}; identity {:?}; sample {}; candidates {:?}; notes {:?}\n",
                display_path(&o.path),o.kind,o.coverage,o.identity,
                o.sample.as_ref().map(|s|format!("{}..{} complete_file={}",s.offset,s.offset+s.bytes.len() as u64,s.complete_file)).unwrap_or_else(||"omitted".into()),o.format_candidates,o.notes);
            if out.len().saturating_add(row.len()).saturating_add(64)>limit {
                let note="Further retained rows omitted from this display.\n";
                if out.len()+note.len()<=limit { out.push_str(note); }break;
            }out.push_str(&row);
        }
        for n in &self.notes { if out.len()+n.len()+1<=limit { out.push_str(n);out.push('\n'); } }
        out
    }
}
#[derive(Clone, Debug)]
pub struct Limits {
    pub entries: usize, pub depth: usize, pub header_bytes: usize,
    pub total_sample_bytes: usize, pub report_bytes: usize, pub duration: Duration,
}
impl Default for Limits {
    fn default()->Self { Self { entries:500, depth:8, header_bytes:4096,
        total_sample_bytes:1024*1024, report_bytes:2*1024*1024, duration:Duration::from_secs(2) } }
}
impl Limits {
    fn validate(&self)->Result<(),String> {
        if self.entries==0 || self.entries>4096 || self.depth>32 || self.header_bytes>65536 ||
          self.total_sample_bytes>4*1024*1024 || self.report_bytes<1024 || self.report_bytes>8*1024*1024 ||
          self.duration.is_zero() || self.duration>Duration::from_secs(30) {
            return Err("Directory observation limits are outside supported bounds".into());
        }Ok(())
    }
}
const O_PATH:i32=0o10000000;
const O_NOFOLLOW:i32=0o400000;
const O_CLOEXEC:i32=0o2000000;
const O_DIRECTORY:i32=0o200000;
unsafe extern "C" { fn openat(fd:i32,path:*const std::ffi::c_char,flags:i32,...)->i32; }
fn open_child(dir:&File,name:&[u8])->Result<File,String> {
    if name.is_empty() || name==b"." || name==b".." || name.contains(&b'/') || name.contains(&0) {
        return Err("Invalid directory entry name".into());
    }
    let c=CString::new(name).map_err(|_|"Invalid entry name")?;
    // O_PATH does not open a device/FIFO for I/O and does not follow links.
    let fd=unsafe{openat(dir.as_raw_fd(),c.as_ptr(),O_PATH|O_NOFOLLOW|O_CLOEXEC)};
    if fd<0 { Err(std::io::Error::last_os_error().to_string()) } else { Ok(unsafe{File::from_raw_fd(fd)}) }
}

// Linux/Android 64-bit dirent ABI used by both supported Android targets.
#[repr(C)] struct Dirent { ino:u64, offset:i64, length:u16, kind:u8, name:[u8;256] }
unsafe extern "C" {
    fn fdopendir(fd:i32)->*mut std::ffi::c_void;
    fn readdir(dir:*mut std::ffi::c_void)->*mut u8;
    fn closedir(dir:*mut std::ffi::c_void)->i32;
}
#[cfg(target_os="android")] unsafe extern "C" {fn __errno()->*mut i32;}
#[cfg(not(target_os="android"))] unsafe extern "C" {fn __errno_location()->*mut i32;}
fn errno()->*mut i32 {
    #[cfg(target_os="android")] {unsafe{__errno()}}
    #[cfg(not(target_os="android"))] {unsafe{__errno_location()}}
}
struct DirectoryEntries(*mut std::ffi::c_void);
impl Drop for DirectoryEntries {fn drop(&mut self){unsafe{closedir(self.0);}}}
impl DirectoryEntries {
    fn new(held:&File)->Result<Self,String>{
        let fd=unsafe{openat(held.as_raw_fd(),c".".as_ptr(),O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)};
        if fd<0{return Err(std::io::Error::last_os_error().to_string());}
        let ptr=unsafe{fdopendir(fd)};
        if ptr.is_null(){let error=std::io::Error::last_os_error().to_string();drop(unsafe{File::from_raw_fd(fd)});Err(error)}else{Ok(Self(ptr))}
    }
    fn next(&mut self)->Result<Option<Vec<u8>>,String>{
        loop{
            unsafe{*errno()=0;}
            let entry=unsafe{readdir(self.0)};
            if entry.is_null(){return if unsafe{*errno()}==0{Ok(None)}else{Err(std::io::Error::last_os_error().to_string())};}
            let bytes=entry.cast::<u8>();
            let length=usize::from(unsafe{std::ptr::read_unaligned(bytes.add(16).cast::<u16>())});
            if !(20..=280).contains(&length){return Err("Invalid native directory record length".into());}
            let capacity=(length-19).min(256);
            let names=unsafe{std::slice::from_raw_parts(bytes.add(19),capacity)};
            let end=names.iter().position(|b|*b==0).ok_or("Unterminated native directory name")?;
            let name=&names[..end];if name==b"."||name==b".."{continue;}return Ok(Some(name.to_vec()));
        }
    }
}
fn open_regular(parent:&File,name:&[u8])->Result<File,String>{
    let name=CString::new(name).map_err(|_|"Invalid file name")?;
    // Nonblocking/no-controlling-terminal flags keep substituted special nodes
    // from blocking here; fstat must confirm the exact regular object before read.
    let fd=unsafe{openat(parent.as_raw_fd(),name.as_ptr(),O_NOFOLLOW|O_CLOEXEC|0o4000|0o400)};
    if fd<0{Err(std::io::Error::last_os_error().to_string())}else{Ok(unsafe{File::from_raw_fd(fd)})}
}
fn kind(m:&Metadata)->Kind {
    if m.is_dir(){Kind::Directory}else if m.is_file(){Kind::File}else if m.file_type().is_symlink(){Kind::Symlink}else{Kind::Other}
}
fn normalize(p:&Path)->Result<PathBuf,String> {
    if p.as_os_str().as_bytes().len()>4096 {return Err("Selected directory path exceeds 4096 bytes".into());}
    let mut out=PathBuf::new();for c in p.components(){match c {
        Component::Normal(n)=>{if n.as_bytes().contains(&0){return Err("Selected path contains NUL".into());}out.push(n)},
        Component::CurDir=>{},_=>return Err("Select a relative directory within this workspace".into()),
    }}Ok(out)
}
fn open_relative(root:&File,path:&Path)->Result<File,String> {
    let mut current=root.try_clone().map_err(|e|e.to_string())?;
    for c in path.components() {
        let Component::Normal(n)=c else{return Err("Non-normal observation path".into());};
        if !current.metadata().map_err(|e|e.to_string())?.is_dir(){return Err("An observation parent is not a directory".into());}
        current=open_child(&current,n.as_bytes())?;
        if current.metadata().map_err(|e|e.to_string())?.file_type().is_symlink(){return Err("Observation does not follow symbolic links".into());}
    }Ok(current)
}
fn display_path(bytes:&[u8])->String { match std::str::from_utf8(bytes){Ok(text)=>format!("{text:?}"),Err(_)=>format!("hex:{}",hex(bytes))} }
fn hex(bytes:&[u8])->String { bytes.iter().map(|b|format!("{b:02x}")).collect() }
fn formats(bytes:&[u8],complete:bool)->Vec<&'static str> {
    let mut result=Vec::new();
    for(magic,name)in[(b"\x7fELF".as_slice(),"ELF"),(b"MZ","DOS/PE"),(b"dex\n","DEX"),(b"\x89PNG\r\n\x1a\n","PNG"),(b"%PDF-","PDF"),(b"\0asm","WebAssembly"),(b"GGUF","GGUF"),(b"RIFF","RIFF"),(b"PK\x03\x04","ZIP signature")]{if bytes.starts_with(magic){result.push(name);}}
    if !bytes.is_empty()&&std::str::from_utf8(bytes).is_ok(){result.push(if complete{"UTF-8 complete bytes"}else{"UTF-8 sampled prefix"});}
    result
}
struct Walker<'a,P,C> where P:FnMut(&Path,Access)->Decision,C:Fn()->bool {
    limits:&'a Limits, policy:&'a mut P, cancelled:&'a C, started:Instant,
    report:Report, retained_cost:usize, pending_cost:usize, exhausted:bool,
}
impl<P,C> Walker<'_,P,C> where P:FnMut(&Path,Access)->Decision,C:Fn()->bool {
    fn check(&mut self)->Result<bool,String> {
        if (self.cancelled)(){return Err("Directory observation cancelled; no completed report published".into());}
        if self.started.elapsed()>=self.limits.duration {self.partial("Observation deadline reached; remaining entries were not observed");self.exhausted=true;}
        Ok(!self.exhausted)
    }
    fn partial(&mut self,n:&'static str){if self.report.coverage!=Coverage::Changed{self.report.coverage=Coverage::Partial;}if !self.report.notes.contains(&n){self.report.notes.push(n);}}
    fn changed(&mut self,i:usize,n:&'static str){self.report.coverage=Coverage::Changed;let o=&mut self.report.occurrences[i];o.coverage=Coverage::Changed;if !o.notes.contains(&n){o.notes.push(n);}o.sample=None;o.format_candidates.clear();}
    fn omit(&mut self,i:usize,n:&'static str){let o=&mut self.report.occurrences[i];if o.coverage!=Coverage::Changed{o.coverage=Coverage::Partial;}if !o.notes.contains(&n){o.notes.push(n);}self.partial("Some selected-tree observations were omitted; consult occurrence coverage");}
    fn authorize(&mut self,i:usize,path:&Path,access:Access)->bool {
        match(self.policy)(path,access){Decision::Allow=>true,Decision::Ask=>{self.omit(i,"Current caller policy requires review; no access granted");false},Decision::Deny=>{self.omit(i,"Current caller policy denies this access");false}}
    }
    fn add(&mut self,path:&Path,k:Kind)->Option<usize> {
        let raw=path.as_os_str().as_bytes();let cost=raw.len()+512;
        if self.report.occurrences.len()>=self.limits.entries || self.retained_cost.saturating_add(self.pending_cost).saturating_add(cost)>self.limits.report_bytes {
            self.partial("Occurrence or report-memory bound reached; remaining names were not retained");self.exhausted=true;return None;
        }
        self.retained_cost+=cost;let i=self.report.occurrences.len();self.report.occurrences.push(Occurrence {path:raw.to_vec(),kind:k,identity:None,sample:None,format_candidates:Vec::new(),coverage:Coverage::Complete,notes:Vec::new(),listing_observed:false});Some(i)
    }
    fn header(&mut self,parent:&File,name:&[u8],path:&Path,i:usize,before:&Identity)->Result<(),String> {
        if !self.check()?{self.omit(i,"Header observation did not complete within the work bound");return Ok(());}if !self.authorize(i,path,Access::ReadHeader){return Ok(());}
        let remaining=self.limits.total_sample_bytes.saturating_sub(self.report.bytes_read);
        let room=self.limits.report_bytes.saturating_sub(self.retained_cost.saturating_add(self.pending_cost));
        let take=self.limits.header_bytes.min(remaining).min(room).min(before.size.min(usize::MAX as u64)as usize);
        if take==0&&before.size>0{self.omit(i,"Header byte or memory budget exhausted; content was not read");return Ok(());}
        let mut input=match open_regular(parent,name){Ok(v)=>v,Err(_)=>{self.omit(i,"Held regular file cannot be opened for reading");return Ok(());}};
        let metadata=input.metadata().map_err(|e|e.to_string())?;if !metadata.is_file(){self.changed(i,"Entry was replaced by a non-regular object before reading");return Ok(());}let opened=Identity::of(&metadata);
        if &opened!=before{self.changed(i,"File identity changed before header read");return Ok(());}
        let mut data=Vec::with_capacity(take);let mut block=[0u8;4096];
        while data.len()<take {if !self.check()?{self.omit(i,"Deadline interrupted header read");return Ok(());}
            let amount=(take-data.len()).min(block.len());match input.read(&mut block[..amount]){
                Ok(0)=>break,Ok(n)=>{self.report.bytes_read+=n;data.extend_from_slice(&block[..n]);},Err(_)=>{self.omit(i,"Header read failed; no sample retained");return Ok(());}
            }
        }
        let after=Identity::of(&input.metadata().map_err(|e|e.to_string())?);
        if &after!=before || data.len()!=take {self.changed(i,"File changed or ended unexpectedly during header read");return Ok(());}
        if !self.check()?{self.omit(i,"Header observation did not complete within the work bound");return Ok(());}if !self.authorize(i,path,Access::ReadHeader){return Ok(());}
        let complete=data.len()as u64==before.size;self.report.sampled_bytes+=data.len();self.retained_cost+=data.len();
        let o=&mut self.report.occurrences[i];o.format_candidates=formats(&data,complete);o.sample=Some(Sample{offset:0,bytes:data,complete_file:complete});
        if !complete{o.coverage=Coverage::Partial;o.notes.push("Only a bounded prefix was sampled; the remainder was not read");self.partial("One or more files have prefix-only content coverage");}
        Ok(())
    }
    fn walk(&mut self,dir:&File,path:&Path,index:usize,depth:usize)->Result<(),String> {
        if !self.check()?{self.omit(index,"Directory enumeration did not complete within the work bound");return Ok(());}if !self.authorize(index,path,Access::ListDirectory){return Ok(());}
        if depth>self.limits.depth {self.omit(index,"Selected traversal depth reached; directory contents were not listed");return Ok(());}
        let before=Identity::of(&dir.metadata().map_err(|e|e.to_string())?);
        let mut listing=match DirectoryEntries::new(dir){Ok(v)=>v,Err(_)=>{self.omit(index,"Held directory listing unavailable");return Ok(());}};
        self.report.occurrences[index].listing_observed=true;
        let remaining=self.limits.entries.saturating_sub(self.report.occurrences.len());
        let available=self.limits.report_bytes.saturating_sub(self.retained_cost.saturating_add(self.pending_cost));
        let capacity=remaining.min(available/(std::mem::size_of::<Vec<u8>>()+1));
        let mut names=Vec::with_capacity(capacity);let mut names_cost=names.capacity()*std::mem::size_of::<Vec<u8>>();self.pending_cost+=names_cost;let mut truncated=false;
        loop {
            if !self.check()?{self.omit(index,"Directory enumeration stopped before completion");self.pending_cost-=names_cost;return Ok(());}
            let name=match listing.next() {Ok(Some(v))=>v,Ok(None)=>break,Err(_)=>{self.omit(index,"Directory enumeration failed before completion");break;}};
            if names.len()>=capacity || self.retained_cost.saturating_add(self.pending_cost).saturating_add(name.capacity())>self.limits.report_bytes {
                truncated=true;break;
            }
            names_cost+=name.capacity();self.pending_cost+=name.capacity();names.push(name);
        }
        names.sort();names.dedup();
        if truncated{self.omit(index,"Only a bounded subset of directory entries was enumerated");}
        for name in names {
            if !self.check()?{self.omit(index,"Directory children were not all observed within the work bound");break;}let next=path.join(OsString::from_vec(name.clone()));
            if next.as_os_str().as_bytes().len()>4096{self.omit(index,"An entry path exceeded 4096 bytes and was omitted");continue;}
            let Some(i)=self.add(&next,Kind::Unknown)else{break;};
            let f=match open_child(dir,&name){Ok(v)=>v,Err(_)=>{self.omit(i,"Entry could not be opened by its held parent");continue;}};
            let meta=match f.metadata(){Ok(v)=>v,Err(_)=>{self.omit(i,"Entry identity unavailable");continue;}};let k=kind(&meta);let identity=Identity::of(&meta);
            self.report.occurrences[i].kind=k;self.report.occurrences[i].identity=Some(identity.clone());
            match k {
                Kind::Directory=>self.walk(&f,&next,i,depth+1)?,
                Kind::File=>self.header(dir,&name,&next,i,&identity)?,
                Kind::Symlink=>self.omit(i,"Symbolic-link target was not followed or read"),
                _=>self.omit(i,"Non-regular entry was not opened for content I/O"),
            }
            // Reopen only metadata through the held parent; a replaced child is
            // never mistaken for the object whose prefix was sampled.
            if let Ok(bound)=open_child(dir,&name).and_then(|f|f.metadata().map_err(|e|e.to_string())) {
                if Identity::of(&bound)!=identity {self.changed(i,"Entry binding or metadata changed during observation");}
            }else{self.changed(i,"Entry binding disappeared during observation");}
        }
        self.pending_cost-=names_cost;
        if Identity::of(&dir.metadata().map_err(|e|e.to_string())?)!=before {self.changed(index,"Directory metadata changed while its entries were observed");}
        Ok(())
    }
}
/// Linux/Android implementation. It uses only OS file APIs and its own Rust
/// decoding. Listing and opening use held descriptors and do not require procfs.
pub fn observe<P,C>(root:&Path,selected:&Path,limits:&Limits,policy:&mut P,cancelled:&C)->Result<Report,String>
where P:FnMut(&Path,Access)->Decision,C:Fn()->bool {
    observe_bound(root,selected,None,limits,policy,cancelled)
}
/// Bind a process-held caller permit to the directory actually opened, before listing.
pub fn observe_bound<P,C>(root:&Path,selected:&Path,expected_root:Option<(u64,u64)>,limits:&Limits,policy:&mut P,cancelled:&C)->Result<Report,String>
where P:FnMut(&Path,Access)->Decision,C:Fn()->bool {
    limits.validate()?;if cancelled(){return Err("Directory observation cancelled".into());}
    let selected=normalize(selected)?;let canonical=root.canonicalize().map_err(|e|e.to_string())?;
    let root_file=OpenOptions::new().read(true).custom_flags(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC).open(&canonical).map_err(|e|e.to_string())?;
    let root_identity=Identity::of(&root_file.metadata().map_err(|e|e.to_string())?);
    if expected_root.is_some_and(|(dev,ino)|dev!=root_identity.dev||ino!=root_identity.ino){return Err("Workspace identity differs from the approved operation; no directory listing started".into());}
    let dir=open_relative(&root_file,&selected)?;let m=dir.metadata().map_err(|e|e.to_string())?;
    if !m.is_dir(){return Err("Selected observation target is not a directory".into());}
    let selected_identity=Identity::of(&m);
    let report=Report{selected:selected.as_os_str().as_bytes().to_vec(),workspace_identity:root_identity.clone(),occurrences:Vec::new(),contains:Vec::new(),coverage:Coverage::Complete,notes:Vec::new(),sampled_bytes:0,bytes_read:0};
    let mut walker=Walker{limits,policy,cancelled,started:Instant::now(),report,retained_cost:0,pending_cost:0,exhausted:false};
    let index=walker.add(&selected,Kind::Directory).ok_or("No room for selected directory identity")?;
    walker.report.occurrences[index].identity=Some(selected_identity.clone());walker.walk(&dir,&selected,index,0)?;
    if cancelled(){return Err("Directory observation cancelled; no completed report published".into());}
    let current_root=fs::metadata(&canonical).map(|m|Identity::of(&m)).ok();
    let current_selected=open_relative(&root_file,&selected).and_then(|f|f.metadata().map_err(|e|e.to_string())).map(|m|Identity::of(&m)).ok();
    if !current_root.as_ref().is_some_and(|v|v.same_object(&root_identity)) || current_selected.as_ref()!=Some(&selected_identity) {
        walker.changed(0,"Workspace or selected-directory binding changed before report publication");
    }
    // Revalidate every retained object's identity without rereading content.
    // Parent symlinks/rebindings are rejected by open_relative at every segment.
    for i in 0..walker.report.occurrences.len() {
        if !walker.check()?{for pending in i..walker.report.occurrences.len(){walker.omit(pending,"Final identity validation did not complete within the work bound");}break;}
        let o=&walker.report.occurrences[i];let Some(before)=o.identity.clone()else{continue;};
        if o.kind==Kind::Symlink{continue;}
        let path=PathBuf::from(OsString::from_vec(o.path.clone()));
        let current=open_relative(&root_file,&path).and_then(|f|f.metadata().map_err(|e|e.to_string())).map(|m|Identity::of(&m)).ok();
        if current.as_ref()!=Some(&before){walker.changed(i,"Retained entry changed before report publication");}
    }
    if cancelled(){return Err("Directory observation cancelled; no completed report published".into());}
    // Policy may change after an earlier directory or sample was observed.
    // Refuse this report rather than publish data whose authority was revoked.
    for occurrence in &walker.report.occurrences {
        if cancelled(){return Err("Directory observation cancelled; no completed report published".into());}
        let path=PathBuf::from(OsString::from_vec(occurrence.path.clone()));
        if occurrence.listing_observed && (walker.policy)(&path,Access::ListDirectory)!=Decision::Allow {
            return Err("Directory listing policy changed before publication; no report returned".into());
        }
        if occurrence.sample.is_some() && (walker.policy)(&path,Access::ReadHeader)!=Decision::Allow {
            return Err("File read policy changed before publication; no report returned".into());
        }
    }
    walker.report.sampled_bytes=walker.report.occurrences.iter().filter_map(|o|o.sample.as_ref()).map(|s|s.bytes.len()).sum();
    walker.report.occurrences.sort_by(|a,b|a.path.cmp(&b.path));
    for (i,o) in walker.report.occurrences.iter().enumerate(){
        if o.path==walker.report.selected{continue;}
        let p=Path::new(std::ffi::OsStr::from_bytes(&o.path));
        let parent=p.parent().unwrap_or(Path::new("")).as_os_str().as_bytes();
        if let Ok(id)=walker.report.occurrences.binary_search_by(|entry|entry.path.as_slice().cmp(parent)){
            walker.report.contains.push(Contains{parent:id,child:i});
        }
    }
    Ok(walker.report)
}

#[cfg(test)] mod checks {
    use super::*;
    use std::{collections::BTreeMap, cell::Cell, sync::atomic::{AtomicU64,Ordering}};
    static SERIAL:AtomicU64=AtomicU64::new(1);
    fn fixture()->PathBuf {
        let p=std::env::temp_dir().join(format!("rllm-directory-observation-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),SERIAL.fetch_add(1,Ordering::Relaxed)));
        fs::create_dir(&p).unwrap();p
    }
    fn run(p:&Path,l:&Limits)->Report {observe(p,Path::new(""),l,&mut |_,_|Decision::Allow,&||false).unwrap()}
    #[test] fn actual_tree_raw_names_samples_and_stable_queries(){
        let p=fixture();fs::create_dir(p.join("sub")).unwrap();fs::write(p.join("sub/image"),b"\x89PNG\r\n\x1a\nabc").unwrap();fs::write(p.join(OsString::from_vec(vec![b'x',255])),b"hello").unwrap();
        let r=run(&p,&Limits::default());assert_eq!(r.coverage,Coverage::Complete);assert_eq!(r.occurrences.len(),4);assert_eq!(r.contains.len(),3);
        let image=r.find(b"sub/image").unwrap();assert_eq!(image.sample.as_ref().unwrap().bytes,b"\x89PNG\r\n\x1a\nabc");assert!(image.format_candidates.contains(&"PNG"));assert!(r.find(&[b'x',255]).is_some());
        fs::write(p.join("sub/image"),b"changed later").unwrap();assert_eq!(r.find(b"sub/image").unwrap().sample.as_ref().unwrap().bytes,b"\x89PNG\r\n\x1a\nabc");assert_eq!(r.page(usize::MAX,5).len(),0);assert_eq!(r.children(0).count(),2);assert!(r.describe(500).len()<=500);
    }
    #[test] fn ask_and_deny_do_not_become_access(){
        let p=fixture();fs::create_dir(p.join("private")).unwrap();fs::write(p.join("private/secret"),b"secret").unwrap();fs::write(p.join("asked"),b"private bytes").unwrap();fs::write(p.join("allowed"),b"public").unwrap();let mut seen=Vec::new();
        let r=observe(&p,Path::new(""),&Limits::default(),&mut |path,a|{seen.push((path.to_path_buf(),a));if path==Path::new("private"){Decision::Deny}else if path==Path::new("asked"){Decision::Ask}else{Decision::Allow}},&||false).unwrap();
        assert_eq!(r.coverage,Coverage::Partial);assert!(r.find(b"private/secret").is_none());assert!(r.find(b"asked").unwrap().sample.is_none());assert!(r.find(b"allowed").unwrap().sample.is_some());assert!(!seen.iter().any(|(p,_)|p==Path::new("private/secret")));
    }
    #[test] fn symbolic_links_are_not_followed(){
        let p=fixture();let outside=fixture();fs::write(outside.join("secret"),b"not observed").unwrap();std::os::unix::fs::symlink(&outside,p.join("link")).unwrap();
        let r=run(&p,&Limits::default());assert_eq!(r.find(b"link").unwrap().kind,Kind::Symlink);assert!(r.find(b"link/secret").is_none());assert!(observe(&p,Path::new("link"),&Limits::default(),&mut |_,_|Decision::Allow,&||false).is_err());
    }
    #[test] fn selected_directory_does_not_scan_siblings(){
        let p=fixture();fs::create_dir(p.join("selected")).unwrap();fs::create_dir(p.join("other")).unwrap();fs::write(p.join("selected/a"),b"a").unwrap();fs::write(p.join("other/b"),b"b").unwrap();
        let r=observe(&p,Path::new("./selected"),&Limits::default(),&mut |path,_|{assert!(path.starts_with("selected"));Decision::Allow},&||false).unwrap();assert_eq!(r.selected,b"selected");assert_eq!(r.occurrences.len(),2);assert!(r.find(b"other").is_none());
        for invalid in ["../other","/tmp","a/../../other"]{assert!(observe(&p,Path::new(invalid),&Limits::default(),&mut |_,_|Decision::Allow,&||false).is_err());}
    }
    #[test] fn bounded_prefix_and_entry_depth_budgets_are_honest(){
        let p=fixture();fs::create_dir(p.join("a")).unwrap();fs::write(p.join("a/deep"),b"deep").unwrap();fs::write(p.join("b"),b"0123456789").unwrap();fs::write(p.join("c"),b"0123456789").unwrap();
        let mut l=Limits::default();l.depth=0;l.header_bytes=4;l.total_sample_bytes=5;let r=run(&p,&l);assert_eq!(r.coverage,Coverage::Partial);assert!(r.find(b"a/deep").is_none());assert!(r.sampled_bytes<=5);assert!(!r.find(b"b").unwrap().sample.as_ref().unwrap().complete_file);
        l.entries=2;let r=run(&p,&l);assert!(r.occurrences.len()<=2);assert_eq!(r.coverage,Coverage::Partial);
    }
    #[test] fn replacement_during_policy_callback_invalidates_sample(){
        let p=fixture();fs::write(p.join("data"),b"original").unwrap();let visits=Cell::new(0);
        let r=observe(&p,Path::new(""),&Limits::default(),&mut |path,a|{if path==Path::new("data")&&a==Access::ReadHeader{visits.set(visits.get()+1);if visits.get()==2{fs::rename(p.join("data"),p.join("preserved")).unwrap();fs::write(p.join("data"),b"replacement").unwrap();}}Decision::Allow},&||false).unwrap();
        assert_eq!(r.coverage,Coverage::Changed);assert_eq!(r.find(b"data").unwrap().coverage,Coverage::Changed);assert!(r.find(b"data").unwrap().sample.is_none());
    }
    #[test] fn cancellation_and_deadline_remain_scoped(){
        let p=fixture();fs::write(p.join("data"),b"some bytes").unwrap();let calls=Cell::new(0);
        let r=observe(&p,Path::new(""),&Limits::default(),&mut |_,_|Decision::Allow,&||{calls.set(calls.get()+1);calls.get()>3});assert!(r.unwrap_err().contains("cancelled"));
        let mut l=Limits::default();l.duration=Duration::from_nanos(1);let r=run(&p,&l);assert_eq!(r.coverage,Coverage::Partial);assert_eq!(r.occurrences[0].coverage,Coverage::Partial);assert!(!r.occurrences[0].notes.is_empty());assert_eq!(run(&p,&Limits::default()).coverage,Coverage::Complete);
    }
    #[test] fn current_policy_is_checked_again_after_read(){
        let p=fixture();fs::write(p.join("data"),b"some bytes").unwrap();let calls=Cell::new(0);
        let r=observe(&p,Path::new(""),&Limits::default(),&mut |_,a|{if a==Access::ReadHeader{calls.set(calls.get()+1);if calls.get()>1{return Decision::Ask;}}Decision::Allow},&||false).unwrap();assert!(r.find(b"data").unwrap().sample.is_none());assert_eq!(r.sampled_bytes,0);assert_eq!(r.coverage,Coverage::Partial);
    }
    #[test] fn ancestor_rebinding_never_reads_replacement_subtree(){
        let p=fixture();let outside=fixture();fs::create_dir(p.join("a")).unwrap();fs::write(p.join("a/good"),b"original").unwrap();fs::write(outside.join("secret"),b"outside bytes").unwrap();let moved=Cell::new(false);
        let r=observe(&p,Path::new(""),&Limits::default(),&mut |path,a|{if path==Path::new("a")&&a==Access::ListDirectory&&!moved.replace(true){fs::rename(p.join("a"),p.join("old-a")).unwrap();std::os::unix::fs::symlink(&outside,p.join("a")).unwrap();}Decision::Allow},&||false).unwrap();
        assert_eq!(r.coverage,Coverage::Changed);assert!(r.find(b"a/secret").is_none());assert_eq!(r.find(b"a").unwrap().coverage,Coverage::Changed);assert!(r.find(b"a/good").unwrap().sample.is_none());
    }
    #[test] fn discarded_reads_still_consume_the_total_byte_budget(){
        let p=fixture();for name in ["a","b","c"]{fs::write(p.join(name),b"1234").unwrap();}let mut counts=BTreeMap::new();let mut limits=Limits::default();limits.total_sample_bytes=5;
        let r=observe(&p,Path::new(""),&limits,&mut |path,a|{if a==Access::ReadHeader{let n=counts.entry(path.to_path_buf()).or_insert(0);*n+=1;if *n==2{return Decision::Deny;}}Decision::Allow},&||false).unwrap();
        assert_eq!(r.bytes_read,5);assert_eq!(r.sampled_bytes,0);assert!(r.occurrences.iter().all(|o|o.sample.is_none()));
        assert_eq!(std::mem::offset_of!(Dirent,name),19);
    }

    #[test] fn approved_root_identity_is_checked_before_any_policy_or_listing(){
        let p=fixture();let m=fs::metadata(&p).unwrap();let calls=Cell::new(0);
        let r=observe_bound(&p,Path::new(""),Some((m.dev(),m.ino().wrapping_add(1))),&Limits::default(),&mut |_,_|{calls.set(calls.get()+1);Decision::Allow},&||false);
        assert!(r.unwrap_err().contains("identity differs"));assert_eq!(calls.get(),0);
    }
    #[test] fn pending_name_allocations_share_the_retained_memory_limit(){
        let p=fixture();for n in 0..100{fs::write(p.join(format!("{n:03}-{}","x".repeat(150))),b"data").unwrap();}let mut l=Limits::default();l.report_bytes=2048;
        let r=run(&p,&l);assert_eq!(r.coverage,Coverage::Partial);assert!(r.occurrences.len()<100);assert!(r.bytes_read<=l.total_sample_bytes);
    }

}

#[path="transfer.rs"] mod transfer;
// Recoverable binary project snapshots with source-bound asynchronous replies.
use super::*;
const PREFIX:&str="project_transfer:";
pub(super) fn is_result(i:&Json)->bool{text(i,"purpose").starts_with(PREFIX)}
pub(super) fn interrupt(st:&mut Json){let mut p=get(st,"project_transfer").clone();if p!=Null{if text(&p,"phase")!="clearing"{put(&mut p,"phase",s("interrupted"));put(&mut p,"purpose",Null);}put(st,"project_transfer",p);}put(st,"project_prepare",Null);}
pub(super) fn start(st:&mut Json)->Result<Json>{start_destination(st,false)}
pub(super) fn start_default(st:&mut Json)->Result<Json>{start_destination(st,boolean(get(st,"drive"),"phone_default"))}
fn native_folder(path:&str)->bool {
 if path.is_empty()||path.chars().any(char::is_control){return false;}
 // Windows measures extended paths in UTF-16 units. A byte cap refuses valid non-ASCII names.
 #[cfg(windows)]
 {let mut units=0usize;for ch in path.chars(){units=units.saturating_add(ch.len_utf16());if units>32767{return false;}}}
 #[cfg(not(windows))]
 {if path.len()>32767{return false;}}
 let native=Path::new(path);
 if native.components().any(|part| matches!(part, std::path::Component::CurDir|std::path::Component::ParentDir)){return false;}
 #[cfg(windows)]
 {use std::path::{Component,Prefix};
  crate::host_fs::windows_folder_path(path)&&native.is_absolute()&&
   matches!(native.components().next(),Some(Component::Prefix(prefix))
    if matches!(prefix.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)|Prefix::UNC(..)|Prefix::VerbatimUNC(..)))
 }
 #[cfg(not(windows))]
 {native.is_absolute()}
}
pub(super) fn device_destination(st:&Json,path:&str)->Result<()> {
 if path.starts_with("content://"){return Ok(());}
 if !native_folder(path){return Err("Grant a device folder before saving a project there".into());}
 #[cfg(target_os="android")]
 crate::host_fs::folder_permission(false,path,boolean(st,"phone_access_granted")).map_err(str::to_string)?;
 #[cfg(not(target_os="android"))]
 {let _=st;crate::host_fs::folder_permission(cfg!(windows),path,false).map_err(|_| "Grant a device folder before saving a project there")?;}
 Ok(())
}
fn start_destination(st:&mut Json,phone:bool)->Result<Json>{destination(st,phone,true)}
fn destination(st:&mut Json,phone:bool,inline:bool)->Result<Json>{
 if get(st,"project_transfer")!=&Null{return Err("A project snapshot is pending; verify or retry it before preparing another".into());}
 let tree=if phone {
  workspace::check_tool(st,"phone_write")?;
  let tree=text(st,"phone_uri");device_destination(st,&tree)?;s(&tree)
 } else {drive_root(st)?;Null};
 let ticket=format!("{}-{}",now(),SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed));
 put(st,"project_prepare",obj(vec![("connection",get(st,"drive").clone()),("phone",Bool(phone)),("tree",tree),("ticket",s(&ticket))]));
 put(st,"busy",Bool(true));
 if inline {
  match try_native_project_save(st){Ok(Some(done))=>return Ok(done),Ok(None)=>{},Err(error)=>{clear_prepare(st);return Err(error);}}
  if cfg!(target_os="android"){
   match try_android_large_archive(st){Ok(Some(done))=>return Ok(done),Ok(None)=>{},Err(error)=>{clear_prepare(st);return Err(error);}}
  }
 }
 Ok(obj(vec![("kind",s("archive_drive")),("ticket",s(&ticket)),("prepare",get(st,"project_prepare").clone())]))
}
fn clear_prepare(st:&mut Json){put(st,"project_prepare",Null);put(st,"busy",Bool(false));}
/// Bytes stored inside saved state and sent as one inline body. Larger project ZIPs use a chunked record.
const INLINE_SNAPSHOT_BYTES:usize=5*1024*1024;
#[cfg(any(test, rllm_module_tests))]
thread_local! {static NATIVE_READBACK_CAP:std::cell::Cell<u64>=const{std::cell::Cell::new(0)};}
#[cfg(any(test, rllm_module_tests))]
static CHUNKED_NATIVE_SAVES:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
fn native_readback_limit()->u64{
 let real=crate::archive::project_budget().min(256*1024*1024);
 #[cfg(any(test, rllm_module_tests))]
 {let chosen=NATIVE_READBACK_CAP.with(|slot|slot.get());if chosen!=0{return chosen.min(real);}}
 real
}
/// Generic directory, no-follow and close-on-exec bits. `host_fs::open_at` maps these
/// for the host. Passing already-mapped ARM values is translated a second time.
fn directory_open_flags()->i32{0x10000|0x20000|0x80000}
fn create_snapshot_flags()->i32{2|0x40|0x80|0x20000|0x800|0x80000}
fn read_snapshot_flags()->i32{0x20000}
fn single_snapshot_name(name:&str)->Result<()>{
 if name.is_empty()||name.len()>255||name=="."||name==".."||name.contains('/')||name.contains('\\')||name.contains('\0')||name.chars().any(char::is_control){
  return Err("Choose a filename inside the selected folder".into());
 }
 Ok(())
}
fn snapshot_deadline(bytes:u64)->std::time::Instant{
 let steps=bytes.saturating_add(262143)/262144;
 std::time::Instant::now()+std::time::Duration::from_secs(steps.saturating_add(120).min(900))
}
fn ensure_running(deadline:std::time::Instant)->Result<()>{
 if crate::execution::cancelled(){return Err("Project snapshot was cancelled; it was not reported as saved".into());}
 if std::time::Instant::now()>=deadline{return Err("Project snapshot exceeded its time bound; it was not reported as saved".into());}
 Ok(())
}
fn open_directory_chain(path:&Path)->Result<std::fs::File>{
 if crate::execution::cancelled(){return Err("Project snapshot was cancelled; it was not reported as saved".into());}
 let absolute=std::path::absolute(path).map_err(|e|e.to_string())?;
 if !absolute.is_dir(){return Err("Grant a device folder before saving a project there".into());}
 #[cfg(windows)]
 {
  use std::os::windows::fs::MetadataExt;
  let mut components=absolute.components();
  let Some(std::path::Component::Prefix(prefix))=components.next() else{return Err("Choose a device folder".into());};
  if components.next()!=Some(std::path::Component::RootDir){return Err("Choose a device folder".into());}
  let mut volume=std::path::PathBuf::from(prefix.as_os_str());
  volume.push(std::path::Path::new(r"\"));
  let mut directory=crate::host_fs::desktop_options().read(true).open(&volume).map_err(|e|e.to_string())?;
  for part in components{
   if crate::execution::cancelled(){return Err("Project snapshot was cancelled; it was not reported as saved".into());}
   let std::path::Component::Normal(name)=part else{return Err("Choose a device folder".into());};
   let text=name.to_str().ok_or("Choose a device folder")?;
   directory=crate::host_fs::open_at(&directory,text.as_bytes(),directory_open_flags()).map_err(|e|e.to_string())?;
   let meta=directory.metadata().map_err(|e|e.to_string())?;
   if !meta.is_dir()||meta.file_attributes()&0x400!=0{return Err("Choose a device folder".into());}
  }
  Ok(directory)
 }
 #[cfg(not(windows))]
 {
  use std::os::unix::ffi::OsStrExt;
  let mut directory=std::fs::File::open("/").map_err(|e|e.to_string())?;
  for part in absolute.components(){
   if crate::execution::cancelled(){return Err("Project snapshot was cancelled; it was not reported as saved".into());}
   match part{
    std::path::Component::Normal(name)=>{
     directory=crate::host_fs::open_at(&directory,name.as_bytes(),directory_open_flags()).map_err(|e|e.to_string())?;
     if !directory.metadata().map_err(|e|e.to_string())?.is_dir(){return Err("Choose a device folder".into());}
    }
    std::path::Component::RootDir|std::path::Component::Prefix(_)=>{}
    _=>return Err("Choose a device folder".into()),
   }
  }
  Ok(directory)
 }
}
fn readback_matches(file:&mut std::fs::File,bytes:&[u8])->Result<()>{
 use std::io::Read;
 if bytes.len() as u64>crate::archive::project_budget(){return Err("Project snapshot exceeds the memory budget".into());}
 let deadline=snapshot_deadline(bytes.len() as u64);
 let mut offset=0usize;
 let mut buf=[0u8;65536];
 while offset<bytes.len(){
  ensure_running(deadline)?;
  let n=(bytes.len()-offset).min(buf.len());
  let end=offset.checked_add(n).ok_or("Project snapshot offset overflow")?;
  file.read_exact(&mut buf[..n]).map_err(|_| "Project snapshot readback differs from the saved archive".to_string())?;
  if buf[..n]!=bytes[offset..end]{return Err("Project snapshot readback differs from the saved archive".into());}
  offset=end;
 }
 ensure_running(deadline)?;
 let mut extra=[0u8;1];
 if file.read(&mut extra).map_err(|e|e.to_string())?!=0{return Err("Project snapshot readback differs from the saved archive".into());}
 Ok(())
}
fn write_and_verify_native(dir:&Path,name:&str,bytes:&[u8])->Result<()>{
 use std::io::{Seek,Write};
 if bytes.len() as u64>crate::archive::project_budget(){return Err("Project snapshot exceeds the memory budget".into());}
 let deadline=snapshot_deadline(bytes.len() as u64);
 ensure_running(deadline)?;
 single_snapshot_name(name)?;
 let parent=open_directory_chain(dir)?;
 let mut file=crate::host_fs::open_at(&parent,name.as_bytes(),create_snapshot_flags()).map_err(|e|format!("New file may be partial: {e}"))?;
 #[cfg(windows)]
 {
  use std::os::windows::fs::MetadataExt;
  let meta=file.metadata().map_err(|e|e.to_string())?;
  if meta.file_attributes()&0x400!=0{return Err("Choose a filename inside the selected folder".into());}
 }
 let mut offset=0usize;
 while offset<bytes.len(){
  ensure_running(deadline)?;
  let n=(bytes.len()-offset).min(65536);
  let end=offset.checked_add(n).ok_or("Project snapshot offset overflow")?;
  file.write_all(&bytes[offset..end]).map_err(|e|format!("New file may be partial: {e}"))?;
  offset=end;
 }
 file.sync_all().and_then(|_|file.rewind()).map_err(|e|format!("New file may be partial: {e}"))?;
 readback_matches(&mut file,bytes)
}
fn commit_native_archive(dir:&Path,name:&str,bytes:&[u8])->Result<()>{
 if (bytes.len() as u64)<=native_readback_limit(){
  return match crate::archive::deliver_native(dir,name,Some(bytes)){
   Ok(Some(found)) if found==bytes=>Ok(()),
   Ok(_)=>Err("Project snapshot readback differs from the saved archive".into()),
   Err(error)=>Err(error),
  };
 }
 write_and_verify_native(dir,name,bytes)?;
 #[cfg(any(test, rllm_module_tests))]
 {let _=CHUNKED_NATIVE_SAVES.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
 Ok(())
}
/// A project archive must be one the importer accepts: directory records, members and checksums.
/// A local signature plus an empty end record is not enough.
fn project_zip(bytes:&[u8])->Result<()>{
 if bytes.len()<22{return Err("Project archive is not a ZIP; it was not saved".into());}
 let _working=admit_extra(u64::try_from(bytes.len()).map_err(|_| "Project archive is not a ZIP; it was not saved".to_string())?)?;
 match crate::archive::unpack(bytes){
  Ok(entries) if !entries.is_empty()=>Ok(()),
  Ok(_)=>Err("Project archive is not a ZIP; it was not saved".into()),
  Err(error) if error.contains("memory budget")||error.contains("cancelled")||error.contains("time bound")=>Err(error),
  Err(error)=>Err(format!("Project archive is not a ZIP; it was not saved: {error}")),
 }
}
fn base64_chars(len:usize)->Result<u64>{
 let n=u64::try_from(len).map_err(|_| "Pending payload exceeds the memory budget")?;
 n.checked_add(2).map(|n|n/3).and_then(|n|n.checked_mul(4)).ok_or("Pending payload exceeds the memory budget".into())
}
struct BudgetLedger{reserved:u64,inflight:u64}
static BUDGET_GATE:std::sync::Mutex<()>=std::sync::Mutex::new(());
static BUDGET_LEDGER:std::sync::Mutex<BudgetLedger>=std::sync::Mutex::new(BudgetLedger{reserved:0,inflight:0});
fn budget_gate()->std::sync::MutexGuard<'static,()>{BUDGET_GATE.lock().unwrap_or_else(|poison|poison.into_inner())}
fn budget_ledger()->std::sync::MutexGuard<'static,BudgetLedger>{BUDGET_LEDGER.lock().unwrap_or_else(|poison|poison.into_inner())}
#[derive(Debug)]
struct BudgetHold{bytes:u64}
impl BudgetHold{
 fn release_locked(&mut self){
  if self.bytes==0{return;}
  let mut ledger=budget_ledger();
  ledger.reserved=ledger.reserved.saturating_sub(self.bytes);
  self.bytes=0;
 }
 fn resize(&mut self,next:u64)->Result<()>{
  let _gate=budget_gate();
  if next==self.bytes{return Ok(());}
  let budget=crate::archive::project_budget();
  if next>budget{return Err("Pending payload exceeds the memory budget".into());}
  if next>self.bytes{
   let add=next-self.bytes;
   match charged_bytes()?.checked_add(add){Some(total) if total<=budget=>{},_=>return Err("Pending payload memory capacity is occupied; retry after active transfers stop".into())}
   let mut ledger=budget_ledger();
   ledger.reserved=ledger.reserved.checked_add(add).ok_or("Pending payload exceeds the memory budget")?;
  }else{
   let mut ledger=budget_ledger();
   ledger.reserved=ledger.reserved.saturating_sub(self.bytes-next);
  }
  self.bytes=next;
  Ok(())
 }
}
impl Drop for BudgetHold{
 fn drop(&mut self){
  if self.bytes==0{return;}
  let _gate=budget_gate();
  self.release_locked();
 }
}
#[derive(Debug)]
struct ChargedBytes{bytes:Vec<u8>,hold:BudgetHold,plain:u64}
impl ChargedBytes{
 fn release_plain_locked(&mut self){
  let release=self.plain.min(self.hold.bytes);
  if release==0{self.plain=0;return;}
  let mut ledger=budget_ledger();
  ledger.reserved=ledger.reserved.saturating_sub(release);
  self.hold.bytes=self.hold.bytes.saturating_sub(release);
  self.plain=0;
 }
}
struct LiveBytes{bytes:Vec<u8>,charged:u64}
impl LiveBytes{
 fn release_locked(&mut self){
  if self.charged==0{return;}
  let mut ledger=budget_ledger();
  ledger.inflight=ledger.inflight.saturating_sub(self.charged);
  self.charged=0;
 }
}
impl Drop for LiveBytes{
 fn drop(&mut self){
  if self.charged==0{return;}
  let _gate=budget_gate();
  self.release_locked();
 }
}
fn charged_bytes()->Result<u64>{
 let parked_sum={let slots=parked().lock().map_err(|_| "Pending payload service unavailable".to_string())?;slots.values().map(|held|held.len() as u64).try_fold(0u64,|sum,n|sum.checked_add(n)).ok_or("Pending payload exceeds the memory budget")?};
 let copied={let held=payloads().lock().map_err(|_| "Pending payload service unavailable".to_string())?;held.values().map(|item|item.bytes.len() as u64).try_fold(0u64,|sum,n|sum.checked_add(n)).ok_or("Pending payload exceeds the memory budget")?};
 let ledger=budget_ledger();
 parked_sum.checked_add(copied).and_then(|n|n.checked_add(ledger.reserved)).and_then(|n|n.checked_add(ledger.inflight)).ok_or("Pending payload exceeds the memory budget".into())
}
fn admit_extra(extra:u64)->Result<BudgetHold>{
 if extra==0{return Ok(BudgetHold{bytes:0});}
 let _gate=budget_gate();
 let budget=crate::archive::project_budget();
 if extra>budget{return Err("Pending payload exceeds the memory budget".into());}
 match charged_bytes()?.checked_add(extra){Some(total) if total<=budget=>{},_=>return Err("Pending payload memory capacity is occupied; retry after active transfers stop".into())}
 let mut ledger=budget_ledger();
 ledger.reserved=ledger.reserved.checked_add(extra).ok_or("Pending payload exceeds the memory budget")?;
 Ok(BudgetHold{bytes:extra})
}
fn present_archive<'a>(inputs:&'a Json)->Result<&'a str>{
 match get(inputs,"archive"){Str(value)=>Ok(value.as_str()),_=>Err("Project archive is missing; it was not saved".into())}
}
fn private_spool_root()->std::path::PathBuf{
 #[cfg(windows)]
 {
  if let Some(path)=std::env::var_os("LOCALAPPDATA"){let path=std::path::PathBuf::from(path);if path.is_absolute(){return path.join("RLLM").join("project-spool");}}
  if let Some(path)=std::env::var_os("USERPROFILE"){let path=std::path::PathBuf::from(path);if path.is_absolute(){return path.join("AppData").join("Local").join("RLLM").join("project-spool");}}
 }
 #[cfg(not(windows))]
 {
  if let Some(path)=std::env::var_os("HOME"){let path=std::path::PathBuf::from(path);if path.is_absolute(){return path.join(".rllm").join("project-spool");}}
 }
 std::env::current_exe().ok().and_then(|path|path.parent().map(|dir|dir.join("project-spool"))).unwrap_or_else(||std::path::PathBuf::from("project-spool"))
}
fn spool_dir()->std::path::PathBuf{
 if let Ok(value)=std::env::var("RLLM_PROJECT_SPOOL"){
  let path=std::path::PathBuf::from(value);
  if path.is_absolute()&&!path.starts_with(std::env::temp_dir())&&!path.components().any(|part|matches!(part,std::path::Component::ParentDir)){return path;}
 }
 private_spool_root()
}
fn spool_file(ticket:&str)->Result<std::path::PathBuf>{
 if ticket.is_empty()||ticket.len()>128||ticket=="."||ticket==".."||ticket=="review"||ticket=="spool.key"||ticket.contains("..")||!ticket.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_.".contains(&b)){
  return Err("Invalid snapshot size or identity".into());
 }
 Ok(spool_dir().join(ticket))
}
#[cfg(unix)]
fn native_dir_nofollow()->i32{
 #[cfg(any(target_os="linux",target_os="android"))]
 {
  #[cfg(any(target_arch="aarch64",target_arch="arm",target_arch="powerpc",target_arch="powerpc64"))]
  {return 0x4000|0x8000;}
  #[cfg(not(any(target_arch="aarch64",target_arch="arm",target_arch="powerpc",target_arch="powerpc64")))]
  {return 0x10000|0x20000;}
 }
 #[cfg(any(target_os="macos",target_os="ios",target_os="tvos",target_os="watchos",target_os="visionos"))]
 {return 0x100000|0x100;}
 #[cfg(not(any(target_os="linux",target_os="android",target_os="macos",target_os="ios",target_os="tvos",target_os="watchos",target_os="visionos")))]
 {0}
}
#[cfg(windows)]
fn restrict_to_owner(path:&Path)->Result<()>{
 use std::os::windows::ffi::OsStrExt;
 let sddl:Vec<u16>=std::ffi::OsStr::new("D:P(A;;FA;;;OW)").encode_wide().chain(std::iter::once(0)).collect();
 #[link(name="advapi32")]
 unsafe extern "system"{
  fn ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl:*const u16,revision:u32,sd:*mut *mut std::ffi::c_void,size:*mut u32)->i32;
  fn GetSecurityDescriptorDacl(sd:*mut std::ffi::c_void,present:*mut i32,dacl:*mut *mut std::ffi::c_void,defaulted:*mut i32)->i32;
  fn SetNamedSecurityInfoW(name:*mut u16,object_type:u32,info:u32,owner:*mut std::ffi::c_void,group:*mut std::ffi::c_void,dacl:*mut std::ffi::c_void,sacl:*mut std::ffi::c_void)->u32;
  fn LocalFree(mem:*mut std::ffi::c_void)->*mut std::ffi::c_void;
 }
 let mut descriptor=std::ptr::null_mut();
 let mut size=0u32;
 if unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut descriptor,&mut size)}==0{
  return Err("Pending payload could not be retained".into());
 }
 let mut present=0i32;
 let mut dacl=std::ptr::null_mut();
 let mut defaulted=0i32;
 let described=unsafe{GetSecurityDescriptorDacl(descriptor,&mut present,&mut dacl,&mut defaulted)};
 if described==0||present==0||dacl.is_null(){
  unsafe{LocalFree(descriptor);}
  return Err("Pending payload could not be retained".into());
 }
 let mut name:Vec<u16>=path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
 let status=unsafe{SetNamedSecurityInfoW(name.as_mut_ptr(),1,4|0x8000_0000,std::ptr::null_mut(),std::ptr::null_mut(),dacl,std::ptr::null_mut())};
 unsafe{LocalFree(descriptor);}
 if status!=0{return Err("Pending payload could not be retained".into());}
 Ok(())
}
fn open_private_dir(path:&Path)->Result<std::fs::File>{
 if path.starts_with(std::env::temp_dir()){return Err("Pending payload could not be retained".into());}
 if let Ok(meta)=std::fs::symlink_metadata(path){
  if meta.file_type().is_symlink()||!meta.is_dir(){return Err("Pending payload could not be retained".into());}
 }else{
  std::fs::create_dir_all(path).map_err(|e|e.to_string())?;
 }
 #[cfg(unix)]
 {
  use std::os::unix::fs::{OpenOptionsExt,PermissionsExt};
  let meta=std::fs::symlink_metadata(path).map_err(|e|e.to_string())?;
  if meta.file_type().is_symlink()||!meta.is_dir(){return Err("Pending payload could not be retained".into());}
  let mut perms=meta.permissions();
  perms.set_mode(0o700);
  std::fs::set_permissions(path,perms).map_err(|e|e.to_string())?;
  return std::fs::OpenOptions::new().read(true).custom_flags(native_dir_nofollow()).open(path).map_err(|e|e.to_string());
 }
 #[cfg(windows)]
 {
  use std::os::windows::fs::OpenOptionsExt;
  restrict_to_owner(path)?;
  return std::fs::OpenOptions::new().read(true).custom_flags(0x0200_0000|0x0020_0000).open(path).map_err(|e|e.to_string());
 }
}
fn owner_only(path:&Path)->Result<()>{
 #[cfg(unix)]
 {
  use std::os::unix::fs::PermissionsExt;
  let meta=std::fs::symlink_metadata(path).map_err(|e|e.to_string())?;
  if meta.file_type().is_symlink(){return Err("Pending payload could not be retained".into());}
  let mut perms=meta.permissions();
  perms.set_mode(if meta.is_dir(){0o700}else{0o600});
  std::fs::set_permissions(path,perms).map_err(|e|e.to_string())?;
 }
 #[cfg(windows)]
 {restrict_to_owner(path)?;}
 Ok(())
}
fn move_aside(dir_path:&Path,name:&str){
 let path=dir_path.join(name);
 if std::fs::symlink_metadata(&path).is_err(){return;}
 let review=dir_path.join("review");
 if open_private_dir(&review).is_err(){return;}
 let _=std::fs::rename(&path,review.join(format!("{name}-{}-{}",now(),SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed))));
}
fn create_private_file(dir:&std::fs::File,dir_path:&Path,name:&str)->Result<std::fs::File>{
 let created=crate::host_fs::open_at(dir,name.as_bytes(),create_snapshot_flags());
 let file=match created{
  Ok(file)=>file,
  Err(_)=>{
   move_aside(dir_path,name);
   crate::host_fs::open_at(dir,name.as_bytes(),create_snapshot_flags()).map_err(|e|format!("Pending payload could not be retained: {e}"))?
  }
 };
 owner_only(&dir_path.join(name))?;
 Ok(file)
}
fn read_spool_key(dir:&std::fs::File)->Result<[u8;32]>{
 use std::io::Read;
 let mut file=crate::host_fs::open_at(dir,b"spool.key",read_snapshot_flags()).map_err(|e|e.to_string())?;
 let mut key=[0u8;32];
 file.read_exact(&mut key).map_err(|_| "Pending payload could not be retained".to_string())?;
 Ok(key)
}
fn spool_key(dir:&std::fs::File,dir_path:&Path)->Result<[u8;32]>{
 if let Ok(key)=read_spool_key(dir){return Ok(key);}
 let fresh=crate::state_crypto::random_bytes(32)?;
 if fresh.len()!=32{return Err("Pending payload could not be retained".into());}
 let mut key=[0u8;32];
 key.copy_from_slice(&fresh);
 match crate::host_fs::open_at(dir,b"spool.key",create_snapshot_flags()){
  Ok(mut file)=>{
   owner_only(&dir_path.join("spool.key"))?;
   use std::io::Write;
   file.write_all(&key).map_err(|e|e.to_string())?;
   file.sync_all().map_err(|e|e.to_string())?;
   Ok(key)
  }
  Err(_)=>read_spool_key(dir),
 }
}
fn retire_spool(ticket:&str){
 let Ok(path)=spool_file(ticket) else{return;};
 if std::fs::symlink_metadata(&path).is_err(){return;}
 move_aside(path.parent().unwrap_or(Path::new(".")),path.file_name().and_then(|name|name.to_str()).unwrap_or(ticket));
}
fn write_sealed(dir:&std::fs::File,dir_path:&Path,name:&str,sealed:&[u8],deadline:std::time::Instant)->Result<()>{
 use std::io::Write;
 let mut file=create_private_file(dir,dir_path,name)?;
 let mut offset=0usize;
 while offset<sealed.len(){
  ensure_running(deadline)?;
  let n=(sealed.len()-offset).min(65536);
  let end=offset.checked_add(n).ok_or("Project snapshot offset overflow")?;
  file.write_all(&sealed[offset..end]).map_err(|e|format!("Pending payload could not be retained: {e}"))?;
  offset=end;
 }
 file.sync_all().map_err(|e|e.to_string())?;
 Ok(())
}
fn write_spool(ticket:&str,bytes:&[u8])->Result<()>{
 let path=spool_file(ticket)?;
 let parent=path.parent().ok_or("Pending payload could not be retained")?;
 let dir=open_private_dir(parent)?;
 let seal_len=u64::try_from(bytes.len()).ok().and_then(|n|n.checked_add(64)).ok_or("Pending payload exceeds the memory budget")?;
 let _seal=admit_extra(seal_len)?;
 let key=spool_key(&dir,parent)?;
 let sealed=crate::state_crypto::seal(bytes,&key)?;
 if sealed.len() as u64>seal_len{return Err("Pending payload exceeds the memory budget".into());}
 let partial=format!("{ticket}.partial");
 let deadline=snapshot_deadline(bytes.len() as u64);
 write_sealed(&dir,parent,&partial,&sealed,deadline)?;
 if path.exists()||std::fs::symlink_metadata(&path).is_ok(){retire_spool(ticket);}
 std::fs::rename(parent.join(&partial),&path).map_err(|e|e.to_string())?;
 owner_only(&path)?;
 Ok(())
}
fn read_private(dir:&std::fs::File,name:&str,limit:u64)->Result<ChargedBytes>{
 use std::io::Read;
 let mut file=crate::host_fs::open_at(dir,name.as_bytes(),read_snapshot_flags()).map_err(|_| "Parked bytes were not retained across restart".to_string())?;
 let meta=file.metadata().map_err(|e|e.to_string())?;
 if !meta.is_file()||meta.len()>limit||meta.len()<28{return Err("Parked bytes were not retained across restart".into());}
 let len=usize::try_from(meta.len()).map_err(|_| "Pending payload exceeds addressable memory")?;
 let hold=admit_extra(meta.len())?;
 let mut sealed=Vec::new();
 sealed.try_reserve_exact(len).map_err(|_| "Pending payload exceeds the memory budget")?;
 let deadline=snapshot_deadline(meta.len());
 let mut buf=[0u8;65536];
 while sealed.len()<len{
  ensure_running(deadline)?;
  let n=file.read(&mut buf).map_err(|e|e.to_string())?;
  if n==0{return Err("Parked bytes were not retained across restart".into());}
  let end=sealed.len().checked_add(n).ok_or("Project snapshot offset overflow")?;
  if end>len{return Err("Parked bytes were not retained across restart".into());}
  sealed.extend_from_slice(&buf[..n]);
 }
 Ok(ChargedBytes{bytes:sealed,hold,plain:meta.len()})
}
fn reload_spool(ticket:&str,declared:u64)->Result<()>{
 if parked_len(ticket).is_ok(){return Ok(());}
 if declared>crate::archive::project_budget(){return Err("Parked bytes were not retained across restart".into());}
 let path=spool_file(ticket)?;
 let parent=path.parent().ok_or("Parked bytes were not retained across restart")?;
 let dir=open_private_dir(parent).map_err(|_| "Parked bytes were not retained across restart".to_string())?;
 let mut hold=admit_extra(declared)?;
 let sealed=read_private(&dir,ticket,crate::archive::project_budget())?;
 let key=spool_key(&dir,parent).map_err(|_| "Parked bytes were not retained across restart".to_string())?;
 let plain=crate::state_crypto::open(&sealed.bytes,&key).map_err(|_| "Parked bytes were not retained across restart".to_string())?;
 drop(sealed);
 if plain.len() as u64!=declared{return Err("Parked bytes were not retained across restart".into());}
 hold.resize(declared)?;
 let mut charged=ChargedBytes{bytes:plain,hold,plain:declared};
 remember_parked(ticket,&mut charged)
}
fn write_snapshot_file(path:&Path,bytes:&[u8])->Result<()>{
 use std::io::Write;
 let deadline=snapshot_deadline(bytes.len() as u64);
 ensure_running(deadline)?;
 let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|format!("Project archive could not be staged: {e}"))?;
 let mut offset=0usize;
 while offset<bytes.len(){
  ensure_running(deadline)?;
  let n=(bytes.len()-offset).min(65536);
  let end=offset.checked_add(n).ok_or("Project snapshot offset overflow")?;
  file.write_all(&bytes[offset..end]).map_err(|e|format!("Project archive could not be staged: {e}"))?;
  offset=end;
 }
 file.sync_all().map_err(|e|e.to_string())?;
 Ok(())
}
fn android_stage_within_budget(len:u64)->Result<()>{
 if len>crate::archive::project_budget(){return Err("Project ZIP exceeds the memory budget".into());}
 Ok(())
}
fn snapshot_key(path:&Path)->PathBuf{std::path::absolute(path).unwrap_or_else(|_|path.to_path_buf())}
fn staged_snapshots()->&'static std::sync::Mutex<std::collections::BTreeSet<PathBuf>>{
 static STAGED:std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<PathBuf>>>=std::sync::OnceLock::new();
 STAGED.get_or_init(||std::sync::Mutex::new(std::collections::BTreeSet::new()))
}
fn remember_staged_snapshot(path:&Path){
 let mut slots=staged_snapshots().lock().unwrap_or_else(|poison|poison.into_inner());
 slots.insert(snapshot_key(path));
}
fn staged_snapshot_owned(path:&Path)->bool{
 let slots=staged_snapshots().lock().unwrap_or_else(|poison|poison.into_inner());
 slots.contains(&snapshot_key(path))
}
fn forget_staged_snapshot(path:&Path){
 let mut slots=staged_snapshots().lock().unwrap_or_else(|poison|poison.into_inner());
 slots.remove(&snapshot_key(path));
}
fn publish_staged_snapshot(root:&Path,name:&str,bytes:&[u8])->Result<()>{
 let path=root.join(name);
 write_snapshot_file(&path,bytes)?;
 remember_staged_snapshot(&path);
 Ok(())
}
fn review_dir_follows_link(path:&Path)->bool{
 let Ok(meta)=std::fs::symlink_metadata(path) else{return false;};
 if meta.file_type().is_symlink(){return true;}
 #[cfg(windows)]
 {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return true;}}
 false
}
fn real_review_dir(root:&Path)->Option<PathBuf>{
 let review=root.join("review");
 if review_dir_follows_link(&review){return None;}
 match std::fs::symlink_metadata(&review){
  Ok(meta)=>{if !meta.is_dir(){return None;}Some(review)}
  Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{
   if std::fs::create_dir(&review).is_err(){return None;}
   if review_dir_follows_link(&review){return None;}
   let meta=std::fs::symlink_metadata(&review).ok()?;
   if !meta.is_dir(){return None;}
   Some(review)
  }
  Err(_)=>None,
 }
}
fn retire_workspace_snapshot(st:&Json,source:&str){
 if source.is_empty()||source.contains('/')||source.contains('\\')||source.contains('\0'){return;}
 let root=text(st,"approval_root");
 if root.is_empty(){return;}
 let path=Path::new(&root).join(source);
 if !staged_snapshot_owned(&path){return;}
 let Ok(meta)=std::fs::symlink_metadata(&path) else{return;};
 if meta.file_type().is_symlink()||!meta.is_file(){return;}
 #[cfg(windows)]
 {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return;}}
 let Some(review)=real_review_dir(Path::new(&root)) else{return;};
 if review_dir_follows_link(&review){return;}
 if std::fs::rename(&path,review.join(format!("{source}-{}",now()))).is_ok(){forget_staged_snapshot(&path);}
}
fn try_android_large_archive(st:&mut Json)->Result<Option<Json>>{
 let prepare=get(st,"project_prepare");
 if !boolean(prepare,"phone"){return Ok(None);}
 let tree=text(prepare,"tree");
 if !tree.starts_with("content://"){return Ok(None);}
 let root=text(st,"approval_root");
 if root.is_empty()||!Path::new(&root).is_dir(){return Ok(None);}
 let bytes=match crate::archive::archive(Path::new(&root)){Ok((bytes,_,_))=>bytes,Err(error)=>{clear_prepare(st);return Err(error);}};
 if bytes.len()<=INLINE_SNAPSHOT_BYTES{return Ok(None);}
 if let Err(error)=android_stage_within_budget(bytes.len() as u64){clear_prepare(st);return Err(error);}
 let len=match u64::try_from(bytes.len()){Ok(len)=>len,Err(_)=>{clear_prepare(st);return Err("Project ZIP exceeds the memory budget".into());}};
 let _held=match admit_extra(len){Ok(held)=>held,Err(error)=>{clear_prepare(st);return Err(error);}};
 if let Err(error)=project_zip(&bytes){clear_prepare(st);return Err(error);}
 let ticket=text(prepare,"ticket");
 let name=format!("rllm-snapshot-{ticket}.zip");
 if let Err(error)=publish_staged_snapshot(Path::new(&root),&name,&bytes){clear_prepare(st);return Err(error);}
 drop(bytes);
 let mut prepare=get(st,"project_prepare").clone();
 put(&mut prepare,"artifact",Bool(true));
 put(&mut prepare,"source",s(&name));
 put(&mut prepare,"name",s(&format!("RLLM__Project-Snapshot-{ticket}.zip")));
 put(&mut prepare,"mime",s("application/zip"));
 put(st,"project_prepare",prepare.clone());
 put(st,"notice",s("Reading a bounded project archive for the selected destination"));
 Ok(Some(obj(vec![("kind",s("artifact_file")),("path",s(&name)),("ticket",s(&ticket)),("prepare",prepare)])))
}
#[cfg(windows)]
#[path="secure.rs"]
mod snapshot_tls;
#[cfg(not(windows))]
#[path="tls_client.rs"]
mod resident_tls;
fn resident_https_transport()->&'static str{
 #[cfg(windows)]{"system"}
 #[cfg(not(windows))]{"native"}
}
fn framed_request(connection:&Json,url:&str,method:&str,mime:&str,len:u64)->Result<(network::Url,Vec<u8>)>{
 let plain=boolean(connection,"plain");
 let authorization=network::storage_authorization(connection)?;
 let mut owned=Vec::new();
 if let Some((name,value))=authorization{owned.push((name,value));}
 if method=="PUT"{owned.push(("Content-Type".into(),mime.to_string()));owned.push(("If-None-Match".into(),"*".into()));}
 let refs:Vec<(&str,String)>=owned.iter().map(|(key,value)|(key.as_str(),value.clone())).collect();
 let effect=network::wire(url,plain,method,"",refs)?;
 let framing=text(&effect,"wire");
 let header=if method=="PUT"{
  let next=framing.replacen("Content-Length: 0\r\n",&format!("Content-Length: {len}\r\n"),1);
  if next==framing{return Err("Unable to frame binary request".into());}
  next
 }else{framing};
 Ok((network::url(url,plain)?,header.into_bytes()))
}
#[cfg(any(test, rllm_module_tests))]
static CONNECT_BLOCK_MS:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static CONNECT_SLOTS:std::sync::OnceLock<std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>>=std::sync::OnceLock::new();
fn connect_slots()->&'static std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>{CONNECT_SLOTS.get_or_init(||std::sync::Mutex::new(Vec::new()))}
fn reap_connects(slots:&mut Vec<std::thread::JoinHandle<()>>){
 let pending=std::mem::take(slots);
 for handle in pending{
  if handle.is_finished(){let _=handle.join();}else{slots.push(handle);}
 }
}
fn park_connect(handle:std::thread::JoinHandle<()>){
 let mut slots=connect_slots().lock().unwrap_or_else(|poison|poison.into_inner());
 reap_connects(&mut slots);
 slots.push(handle);
}
fn connect_until(host:&str,port:u16,deadline:std::time::Instant)->Result<std::net::TcpStream>{
 let target=if host.contains(':'){format!("[{host}]:{port}")}else{format!("{host}:{port}")};
 {
  let mut slots=connect_slots().lock().unwrap_or_else(|poison|poison.into_inner());
  reap_connects(&mut slots);
  if !slots.is_empty(){return Err("A previous connection attempt is still finishing; it was not reported as saved".into());}
 }
 let (tx,rx)=std::sync::mpsc::channel();
 let handle=std::thread::Builder::new().name("project-connect".into()).spawn(move ||{
  #[cfg(any(test, rllm_module_tests))]
  {let ms=CONNECT_BLOCK_MS.load(std::sync::atomic::Ordering::Relaxed);if ms>0{std::thread::sleep(std::time::Duration::from_millis(ms));}}
  let _=tx.send(std::net::TcpStream::connect(target));
 }).map_err(|_| "Project destination was not reached; it was not reported as saved".to_string())?;
 loop{
  if let Err(error)=ensure_running(deadline){park_connect(handle);return Err(error);}
  match rx.recv_timeout(std::time::Duration::from_millis(200)){
   Ok(Ok(stream))=>{let _=handle.join();return Ok(stream);}
   Ok(Err(error))=>{let _=handle.join();return Err(format!("Project destination was not reached: {error}"));}
   Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>continue,
   Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>{let _=handle.join();return Err("Project destination was not reached; it was not reported as saved".into());}
  }
 }
}
fn read_some(reader:&mut impl std::io::Read,buf:&mut [u8],deadline:std::time::Instant)->Result<usize>{
 loop{
  ensure_running(deadline)?;
  match reader.read(buf){
   Ok(0)=>return Ok(0),
   Ok(n)=>return Ok(n),
   Err(error) if matches!(error.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut|std::io::ErrorKind::Interrupted)=>continue,
   Err(error)=>return Err(format!("Project destination closed before the snapshot finished: {error}")),
  }
 }
}
fn response_code(header:&str)->Result<u16>{
 let line=header.split("\r\n").next().unwrap_or("");
 let rest=line.strip_prefix("HTTP/1.1 ").or_else(||line.strip_prefix("HTTP/1.0 ")).ok_or("Invalid HTTP status line")?;
 let code=rest.get(..3).ok_or("Invalid HTTP status line")?;
 if !code.bytes().all(|b|b.is_ascii_digit()){return Err("Invalid HTTP status line".into());}
 let status=code.parse::<u16>().map_err(|_| "Invalid HTTP status line")?;
 if !(100..600).contains(&status){return Err("Invalid HTTP status line".into());}
 Ok(status)
}
fn header_length(header:&str)->Result<Option<usize>>{
 let mut found=None;
 for line in header.split("\r\n").skip(1){
  if line.is_empty(){break;}
  let (name,value)=line.split_once(':').ok_or("Invalid response header")?;
  if name.eq_ignore_ascii_case("transfer-encoding"){return Err("Project destination used an unsupported response framing; it was not reported as saved".into());}
  if !name.eq_ignore_ascii_case("content-length"){continue;}
  if found.is_some(){return Err("Invalid response length".into());}
  let value=value.trim();
  if value.is_empty()||!value.bytes().all(|b|b.is_ascii_digit()){return Err("Invalid response length".into());}
  let n=value.parse::<u64>().map_err(|_| "Invalid response length")?;
  found=Some(usize::try_from(n).map_err(|_| "Invalid response length")?);
 }
 Ok(found)
}
fn consume_body(reader:&mut impl std::io::Read,expect:Option<&[u8]>,prefix:&[u8],length:usize,deadline:std::time::Instant)->Result<()>{
 if let Some(expect)=expect{
  if length!=expect.len()||prefix.len()>length{return Err("Project snapshot readback differs from the saved archive".into());}
  if prefix!=&expect[..prefix.len()]{return Err("Project snapshot readback differs from the saved archive".into());}
  let mut offset=prefix.len();
  let mut buf=[0u8;65536];
  while offset<expect.len(){
   ensure_running(deadline)?;
   let n=(expect.len()-offset).min(buf.len());
   let got=read_some(reader,&mut buf[..n],deadline)?;
   if got==0{return Err("Project snapshot readback differs from the saved archive".into());}
   let end=offset.checked_add(got).ok_or("Project snapshot offset overflow")?;
   if buf[..got]!=expect[offset..end]{return Err("Project snapshot readback differs from the saved archive".into());}
   offset=end;
  }
  return Ok(());
 }
 if length>65536||prefix.len()>length{return Err("Project destination response exceeds its bound; it was not reported as saved".into());}
 let mut left=length-prefix.len();
 let mut buf=[0u8;4096];
 while left>0{
  ensure_running(deadline)?;
  let n=left.min(buf.len());
  let got=read_some(reader,&mut buf[..n],deadline)?;
  if got==0{return Err("Project destination closed before the snapshot finished; it was not reported as saved".into());}
  left=left.checked_sub(got).ok_or("Project snapshot offset overflow")?;
 }
 Ok(())
}
fn read_response(reader:&mut impl std::io::Read,expect:Option<&[u8]>,deadline:std::time::Instant)->Result<u16>{
 let mut pending=Vec::new();
 let mut buf=[0u8;4096];
 let mut informational=0u8;
 loop{
  let header_end=loop{
   if let Some(at)=pending.windows(4).position(|mark|mark==b"\r\n\r\n"){break at;}
   if pending.len()>65536{return Err("HTTP headers exceed bound".into());}
   let n=read_some(reader,&mut buf,deadline)?;
   if n==0{return Err("Project destination closed before the snapshot finished; it was not reported as saved".into());}
   pending.extend_from_slice(&buf[..n]);
  };
  let header=std::str::from_utf8(&pending[..header_end]).map_err(|_| "Invalid response header")?;
  let status=response_code(header)?;
  let body_at=header_end.checked_add(4).ok_or("Project snapshot offset overflow")?;
  let already=pending[body_at..].to_vec();
  if (100..200).contains(&status){
   informational=informational.saturating_add(1);
   if informational>3{return Err("Too many informational responses".into());}
   pending=already;
   continue;
  }
  if status==404{
   let length=header_length(header)?.unwrap_or(0).min(65536);
   let kept=already.len().min(length);
   consume_body(reader,None,&already[..kept],length,deadline)?;
   return Ok(404);
  }
  let length=match (status,header_length(header)?,expect){
   (201|204,None,None)|(201|204,Some(0),None)=>0,
   (_,Some(n),_)=>n,
   _=>return Err("Project destination omitted the response length; it was not reported as saved".into()),
  };
  consume_body(reader,expect,&already,length,deadline)?;
  return Ok(status);
 }
}
fn write_request(link:&mut impl std::io::Write,header:&[u8],body:Option<&[u8]>,deadline:std::time::Instant)->Result<()>{
 link.write_all(header).map_err(|e|format!("Project destination closed before the snapshot finished: {e}"))?;
 let Some(body)=body else{link.flush().map_err(|e|e.to_string())?;return Ok(());};
 let mut offset=0usize;
 while offset<body.len(){
  ensure_running(deadline)?;
  let n=(body.len()-offset).min(65536);
  let end=offset.checked_add(n).ok_or("Project snapshot offset overflow")?;
  link.write_all(&body[offset..end]).map_err(|e|format!("Project destination closed before the snapshot finished: {e}"))?;
  offset=end;
 }
 link.flush().map_err(|e|e.to_string())?;
 Ok(())
}
fn exchange(parsed:&network::Url,tls_name:&str,header:&[u8],body:Option<&[u8]>,expect:Option<&[u8]>,deadline:std::time::Instant)->Result<u16>{
 let mut stream=connect_until(&parsed.host,parsed.port,deadline)?;
 stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
 stream.set_write_timeout(Some(std::time::Duration::from_secs(30))).ok();
 if !parsed.tls{
  write_request(&mut stream,header,body,deadline)?;
  return read_response(&mut stream,expect,deadline);
 }
 let name=if tls_name.is_empty(){parsed.host.as_str()}else{tls_name};
 #[cfg(windows)]
 {
  let _=resident_https_transport();
  let mut link=snapshot_tls::Secure::open(stream,name).map_err(|e|format!("Project destination was not reached: {e}"))?;
  write_request(&mut link,header,body,deadline)?;
  return read_response(&mut link,expect,deadline);
 }
 #[cfg(not(windows))]
 {
  let _=resident_https_transport();
  let mut link=resident_tls::Client::open(stream,name).map_err(|e|format!("Project destination was not reached: {e}"))?;
  write_request(&mut link,header,body,deadline)?;
  return read_response(&mut link,expect,deadline);
 }
}
fn remote_once(connection:&Json,url:&str,method:&str,mime:&str,body:Option<&[u8]>,expect:Option<&[u8]>,deadline:std::time::Instant)->Result<u16>{
 let len=body.map(|bytes|bytes.len() as u64).unwrap_or(0);
 let (parsed,header)=framed_request(connection,url,method,mime,len)?;
 exchange(&parsed,&text(connection,"tls_name"),&header,body,expect,deadline)
}
fn deliver_remote(connection:&Json,url:&str,mime:&str,bytes:&[u8],phase:&str)->Result<()>{
 let deadline=snapshot_deadline(bytes.len() as u64);
 ensure_running(deadline)?;
 let put=|deadline:std::time::Instant|->Result<()>{
  let status=remote_once(connection,url,"PUT",mime,Some(bytes),None,deadline)?;
  if status!=201&&status!=204{return Err(format!("Project create returned HTTP {status}; it was not reported as saved"));}
  let status=remote_once(connection,url,"GET",mime,None,Some(bytes),deadline)?;
  if status!=200{return Err(format!("Project readback returned HTTP {status}; it was not reported as saved"));}
  Ok(())
 };
 match phase{
  "write"=>put(deadline),
  "verify"=>{
   let status=remote_once(connection,url,"GET",mime,None,Some(bytes),deadline)?;
   if status!=200{return Err(format!("Project readback returned HTTP {status}; it was not reported as saved"));}
   Ok(())
  }
  "probe"=>{
   match remote_once(connection,url,"GET",mime,None,Some(bytes),deadline){
    Ok(200)=>Ok(()),
    Ok(404)=>put(deadline),
    Ok(status)=>Err(format!("Project readback returned HTTP {status}; it was not reported as saved")),
    Err(error)=>Err(error),
   }
  }
  _=>Err("Invalid pending transfer phase".into()),
 }
}
fn finish_held(st:&mut Json,p:&Json,notice:&str)->Result<Json>{
 unpark_transfer(p);
 put(st,"project_transfer",Null);put(st,"project_prepare",Null);put(st,"busy",Bool(false));
 if ["project_transfer","project_transfer_discard_review"].contains(&text(st,"dialog").as_str()){put(st,"dialog",Null);}
 put(st,"notice",s(notice));Ok(Null)
}
fn take_parked(ticket:&str)->Result<LiveBytes>{
 let _gate=budget_gate();
 let mut slots=parked().lock().map_err(|_| "Pending payload service unavailable".to_string())?;
 let bytes=slots.remove(ticket).ok_or("Pending payload is not available for delivery")?;
 let charged=u64::try_from(bytes.len()).map_err(|_| "Pending payload exceeds the memory budget".to_string())?;
 let mut ledger=budget_ledger();
 ledger.inflight=ledger.inflight.checked_add(charged).ok_or("Pending payload exceeds the memory budget")?;
 Ok(LiveBytes{bytes,charged})
}
fn restore_taken(ticket:&str,mut live:LiveBytes){
 let bytes=std::mem::take(&mut live.bytes);
 let _gate=budget_gate();
 if let Ok(mut slots)=parked().lock(){slots.insert(ticket.to_string(),bytes);}
 live.release_locked();
}
fn complete_remote_resident(st:&mut Json,p:&Json,phase:&str)->Result<Json>{
 let ticket=text(p,"stage_ticket");
 let declared=match get(p,"bytes"){Num(n)=>n.parse::<u64>().map_err(|_| "Invalid snapshot size".to_string())?,_=>return Err("Missing snapshot size".into())};
 let live=take_parked(&ticket)?;
 if live.bytes.len() as u64!=declared||declared>crate::archive::project_budget(){restore_taken(&ticket,live);return Err("Saved payload size differs from its pinned metadata".into());}
 match deliver_remote(get(p,"connection"),&text(p,"target"),&text(p,"mime"),&live.bytes,phase){
  Ok(())=>{drop(live);finish_held(st,p,"Project snapshot saved to WebDAV and exact bytes verified")}
  Err(error)=>{restore_taken(&ticket,live);Err(error)}
 }
}
fn complete_native_resident(st:&mut Json,p:&Json,phase:&str)->Result<Json>{
 let ticket=text(p,"stage_ticket");
 let declared=match get(p,"bytes"){Num(n)=>n.parse::<u64>().map_err(|_| "Invalid snapshot size".to_string())?,_=>return Err("Missing snapshot size".into())};
 let live=take_parked(&ticket)?;
 if live.bytes.len() as u64!=declared{restore_taken(&ticket,live);return Err("Saved payload size differs from its pinned metadata".into());}
 let tree=text(p,"tree");let name=text(p,"target");
 let outcome=(||->Result<()>{
  match phase{
   "write"=>commit_native_archive(Path::new(&tree),&name,&live.bytes),
   "verify"=>readback_native(Path::new(&tree),&name,&live.bytes),
   "probe"=>match readback_native(Path::new(&tree),&name,&live.bytes){Ok(())=>Ok(()),Err(error) if error.contains("not found")=>commit_native_archive(Path::new(&tree),&name,&live.bytes),Err(error)=>Err(error)},
   _=>Err("Invalid pending transfer phase".into()),
  }
 })();
 match outcome{
  Ok(())=>{drop(live);finish_held(st,p,"Project snapshot saved to the selected device folder and exact bytes verified")}
  Err(error)=>{restore_taken(&ticket,live);Err(error)}
 }
}
fn readback_native(dir:&Path,name:&str,bytes:&[u8])->Result<()>{
 single_snapshot_name(name)?;
 let parent=open_directory_chain(dir)?;
 let mut file=match crate::host_fs::open_at(&parent,name.as_bytes(),read_snapshot_flags()){
  Ok(file)=>file,
  Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Err("Project document was not found".into()),
  Err(error)=>return Err(error.to_string()),
 };
 readback_matches(&mut file,bytes)
}
fn try_native_project_save(st:&mut Json)->Result<Option<Json>>{
 let prepare=get(st,"project_prepare");
 if !boolean(prepare,"phone"){return Ok(None);}
 let tree=text(prepare,"tree");
 if tree.starts_with("content://")||!native_folder(&tree){return Ok(None);}
 let root=text(st,"approval_root");
 if root.is_empty()||!Path::new(&root).is_dir(){return Ok(None);}
 let ticket=text(prepare,"ticket");
 let name=format!("RLLM__Project-Snapshot-{ticket}.zip");
 let bytes=match crate::archive::archive(Path::new(&root)){Ok((bytes,_,_))=>bytes,Err(error)=>{clear_prepare(st);return Err(error);}};
 if let Err(error)=commit_native_archive(Path::new(&tree),&name,&bytes){clear_prepare(st);return Err(error);}
 clear_prepare(st);put(st,"notice",s("Project snapshot saved to the selected device folder and exact bytes verified"));Ok(Some(Null))
}
pub(super) fn artifact_start(st:&mut Json,source:&str)->Result<Json>{
 let parts=relative_parts(source)?;let basename=parts.last().ok_or("Choose a project file")?;
 if basename.len()>160 || basename.chars().any(|c|c.is_control()||"%:\\".contains(c)){return Err("Source filename must be at most 160 bytes without control, percent, colon or backslash characters".into());}
 let mut effect=destination(st,boolean(get(st,"drive"),"phone_default"),false)?;let mut prepare=get(st,"project_prepare").clone();
 put(&mut prepare,"artifact",Bool(true));put(&mut prepare,"source",s(source));put(&mut prepare,"name",s(&format!("RLLM__Artifact-{}__{}",now(),basename)));put(&mut prepare,"mime",s("application/octet-stream"));put(st,"project_prepare",prepare);
 #[cfg(not(target_os="android"))]
 if Path::new(&text(st,"approval_root")).is_dir(){return desktop_artifact(st,source);}
 put(&mut effect,"kind",s("artifact_file"));put(&mut effect,"path",s(source));put(&mut effect,"prepare",get(st,"project_prepare").clone());put(st,"dialog",Null);put(st,"notice",s("Reading a bounded immutable file snapshot for the selected destination…"));Ok(effect)
}
#[cfg(not(target_os="android"))]
fn desktop_artifact(st:&mut Json,source:&str)->Result<Json>{
 let root=text(st,"approval_root");
 let parts=relative_parts(source)?;
 let bytes=match crate::archive::file_snapshot(Path::new(&root),&parts){Ok(bytes)=>bytes,Err(error)=>{clear_prepare(st);return Err(error);}};
 let prepare=get(st,"project_prepare").clone();
 let tree=text(&prepare,"tree");
 if boolean(&prepare,"phone")&&native_folder(&tree)&&!tree.starts_with("content://"){
  let name=text(&prepare,"name");
  return match commit_native_archive(Path::new(&tree),&name,&bytes){
   Ok(())=>{clear_prepare(st);put(st,"dialog",Null);put(st,"notice",s("Project file snapshot saved to the selected device folder and exact bytes verified"));Ok(Null)}
   Err(error)=>{clear_prepare(st);Err(error)}
  };
 }
 if bytes.len()<=INLINE_SNAPSHOT_BYTES{
  let input=obj(vec![("ticket",get(&prepare,"ticket").clone()),("archive",s(&network::b64(&bytes)))]);
  return built(st,&input,true);
 }
 let prepared=record(&prepare,bytes.len())?;
 return match deliver_remote(get(&prepared,"connection"),&text(&prepared,"target"),&text(&prepared,"mime"),&bytes,"write"){
  Ok(())=>{clear_prepare(st);put(st,"dialog",Null);put(st,"notice",s("Project file snapshot saved to WebDAV and exact bytes verified"));Ok(Null)}
  Err(error)=>{clear_prepare(st);Err(error)}
 };
}
fn relative_parts(source:&str)->Result<Vec<String>>{
 if source.is_empty()||source.len()>1024||source.contains('\0')||source.contains('\\')||source.starts_with('/') {return Err("Choose a relative project file path".into());}
 let parts:Vec<String>=source.split('/').map(str::to_string).collect();if parts.iter().any(|p|p.is_empty()||p=="."||p=="..") {return Err("Project path cannot contain empty or dot segments".into());}Ok(parts)
}
pub(super) fn artifact_failure(st:&mut Json,inputs:&Json)->Json{
 let p=get(st,"project_prepare");if p==&Null||text(p,"ticket")!=text(inputs,"ticket"){return Null;}
 put(st,"project_prepare",Null);put(st,"busy",Bool(false));put(st,"notice",s("Project file snapshot could not be read; no destination bytes were written. Check the relative file path, file type and size, then retry."));Null
}
pub(super) fn file_snapshot(v:&Json)->Result<Json>{
 let parts=relative_parts(&field(v,"path")?)?;
 let bytes=crate::archive::file_snapshot(Path::new(field(v,"root")?),&parts)?;
 Ok(obj(vec![("archive",s(&network::b64(&bytes))),("bytes",Num(bytes.len().to_string()))]))
}
fn record(prepare:&Json,size:usize)->Result<Json>{
 if text(prepare,"ticket").is_empty(){return Err("Invalid snapshot size or identity".into());}
 if size as u64>crate::archive::project_budget(){return Err("Project ZIP exceeds the memory budget".into());}
 let phone=boolean(prepare,"phone");let connection=get(prepare,"connection").clone();let artifact=boolean(prepare,"artifact");
 let name=if artifact{text(prepare,"name")}else{format!("RLLM__Project-Snapshot-{}.zip",text(prepare,"ticket"))};
 let target=if phone{name.clone()}else{format!("{}{}",drive_root(&obj(vec![("drive",connection.clone())]))?,network::encoded(&name))};
 let mime=if artifact{payload_mime(prepare)?}else{"application/zip".into()};
 Ok(obj(vec![("mime",s(&mime)),("source",get(prepare,"source").clone()),("phone",Bool(phone)),("tree",get(prepare,"tree").clone()),("connection",connection),("target",s(&target)),("archive",Null),("stage_ticket",get(prepare,"ticket").clone()),("bytes",Num(size.to_string())),("phase",s("ready"))]))
}
fn stream_record(prepare:&Json,size:usize)->Result<Json>{
 let budget=crate::archive::project_budget();let artifact=boolean(prepare,"artifact");
 let cap=if artifact{budget.min(256*1024*1024)}else{budget};
 if size as u64>cap{
  if artifact&&size as u64>256*1024*1024{return Err("Chunked snapshots require a regular artifact up to 256 MiB".into());}
  return Err("Project ZIP exceeds the memory budget".into());
 }
 let mut p=record(prepare,0)?;put(&mut p,"bytes",Num(size.to_string()));put(&mut p,"format",Num("2".into()));Ok(p)
}
pub(super) fn chunks(v:&Json)->Result<Json>{if field(v,"action").ok()==Some("parked"){return deliver_parked(v);}transfer::call(v)}
pub(super) fn staging(v:&Json)->Result<Json>{let size=field(v,"bytes")?.parse::<usize>().map_err(|_|"Invalid snapshot size")?;record(get(v,"prepare"),size)}
pub(super) fn built(st:&mut Json,inputs:&Json,artifact:bool)->Result<Json>{
 let prepare=get(st,"project_prepare");if prepare==&Null||boolean(prepare,"artifact")!=artifact||text(prepare,"ticket")!=text(inputs,"ticket"){return Ok(Null);}
 let mut p;
 if boolean(inputs,"external") {
  let supplied=get(inputs,"record");let size=match get(supplied,"bytes"){Num(n)=>n.parse::<usize>().map_err(|_|"Invalid snapshot size")?,_=>return Err("Missing snapshot size".into())};
  p=if get(supplied,"format")==&Num("2".into()){stream_record(prepare,size)?}else{record(prepare,size)?};if &p!=supplied{return Err("Encrypted snapshot metadata differs from the preparation".into());}
  if !artifact{
   let encoded=present_archive(inputs)?;
   let body=payload_bytes(&Null,encoded)?;
   project_zip(&body.bytes)?;
   if body.bytes.len()!=size{return Err("Saved payload size differs from its pinned metadata".into());}
  }
  retire_workspace_snapshot(st,&text(prepare,"source"));
 }else{
  let encoded=present_archive(inputs)?;let mut bytes=payload_bytes(&Null,encoded)?;
  if !artifact{project_zip(&bytes.bytes)?;}
  if bytes.bytes.len()<=INLINE_SNAPSHOT_BYTES{p=record(prepare,bytes.bytes.len())?;put(&mut p,"stage_ticket",Null);put(&mut p,"archive",s(encoded));}
  else{
   p=stream_record(prepare,bytes.bytes.len())?;let ticket=text(&p,"stage_ticket");
   write_spool(&ticket,&bytes.bytes)?;
   if let Err(error)=remember_parked(&ticket,&mut bytes){retire_spool(&ticket);return Err(error);}
   put(&mut p,"resident",Bool(true));
   retire_workspace_snapshot(st,&text(prepare,"source"));
  }
 }
 let parked=!text(&p,"stage_ticket").is_empty()&&get(&p,"format")==&Num("2".into())&&!boolean(inputs,"external");
 let mut next=st.clone();put(&mut next,"project_transfer",p);put(&mut next,"project_prepare",Null);
 if next.string().len()>6*1024*1024-4096{
  if parked{unpark_transfer(get(&next,"project_transfer"));}
  return Err("Encrypted history has insufficient room for this pending record; original files and staged bytes remain unchanged".into());
 }
 let requested=request(&mut next,"write");
 if requested.is_err()&&parked{unpark_transfer(get(&next,"project_transfer"));}
 let e=requested?;*st=next;Ok(e)
}
fn payload_mime(p:&Json)->Result<String>{let value=text(p,"mime");if value.is_empty(){return Ok("application/zip".into());}if ["application/zip","application/octet-stream","image/png","image/jpeg","audio/wav","audio/ogg","audio/mpeg","audio/mp4","video/mp4","application/pdf","model/gltf-binary"].contains(&value.as_str()){Ok(value)}else{Err("Unsupported snapshot content type".into())}}
fn payload_bytes(p:&Json,encoded:&str)->Result<ChargedBytes> {
 let encoded_len=u64::try_from(encoded.len()).map_err(|_| "Pending payload exceeds the memory budget")?;
 let decoded_estimate=encoded_len.saturating_mul(3)/4;
 let mut hold=admit_extra(encoded_len.checked_add(decoded_estimate).ok_or("Pending payload exceeds the memory budget")?)?;
 let bytes=network::unbase64(encoded)?;
 let decoded_len=u64::try_from(bytes.len()).map_err(|_| "Pending payload exceeds the memory budget")?;
 hold.resize(encoded_len.checked_add(decoded_len).ok_or("Pending payload exceeds the memory budget")?)?;
 let limit=crate::archive::project_budget();
 if decoded_len>limit{return Err("Pending payload exceeds the memory budget".into());}
 if !text(p,"stage_ticket").is_empty() && get(p,"bytes")!=&Num(bytes.len().to_string()){return Err("Saved payload size differs from its pinned metadata".into());}
 Ok(ChargedBytes{bytes,hold,plain:decoded_len})
}
static PARKED:std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<String,Vec<u8>>>>=std::sync::OnceLock::new();
fn parked()->&'static std::sync::Mutex<std::collections::BTreeMap<String,Vec<u8>>>{PARKED.get_or_init(||std::sync::Mutex::new(std::collections::BTreeMap::new()))}
fn unpark_transfer(p:&Json){let ticket=text(p,"stage_ticket");if ticket.is_empty(){return;}if let Ok(mut slots)=parked().lock(){slots.remove(&ticket);}retire_spool(&ticket);}
fn remember_parked(ticket:&str,charged:&mut ChargedBytes)->Result<()>{
 if ticket.is_empty()||ticket.len()>128{return Err("Invalid snapshot size or identity".into());}
 let incoming=u64::try_from(charged.bytes.len()).map_err(|_| "Pending payload exceeds the memory budget".to_string())?;
 if incoming!=charged.plain{return Err("Saved payload size differs from its pinned metadata".into());}
 let budget=crate::archive::project_budget();
 if incoming>budget{return Err("Pending payload exceeds the memory budget".into());}
 let _gate=budget_gate();
 let mut slots=parked().lock().map_err(|_| "Pending payload service unavailable".to_string())?;
 if slots.len()>=2&&!slots.contains_key(ticket){return Err("Pending payload memory capacity is occupied; retry after active transfers stop".into());}
 let others=slots.iter().filter(|(key,_)|key.as_str()!=ticket).map(|(_,held)|held.len() as u64).try_fold(0u64,|sum,n|sum.checked_add(n)).ok_or("Pending payload exceeds the memory budget")?;
 let copied={let held=payloads().lock().map_err(|_| "Pending payload service unavailable".to_string())?;held.values().map(|item|item.bytes.len() as u64).try_fold(0u64,|sum,n|sum.checked_add(n)).ok_or("Pending payload exceeds the memory budget")?};
 let ledger=budget_ledger();
 let reserved_other=ledger.reserved.saturating_sub(charged.hold.bytes);
 let encoded_still=charged.hold.bytes.saturating_sub(charged.plain);
 let inflight=ledger.inflight;
 drop(ledger);
 let Some(total)=others.checked_add(incoming).and_then(|n|n.checked_add(copied)).and_then(|n|n.checked_add(reserved_other)).and_then(|n|n.checked_add(encoded_still)).and_then(|n|n.checked_add(inflight)) else{return Err("Pending payload exceeds the memory budget".into());};
 if total>budget{return Err("Pending payload memory capacity is occupied; retry after active transfers stop".into());}
 slots.insert(ticket.to_string(),std::mem::take(&mut charged.bytes));
 drop(slots);
 charged.release_plain_locked();
 Ok(())
}
fn park_project_bytes(ticket:&str,bytes:Vec<u8>,alongside:u64)->Result<()>{
 let incoming=u64::try_from(bytes.len()).map_err(|_| "Pending payload exceeds the memory budget".to_string())?;
 let hold=admit_extra(incoming.checked_add(alongside).ok_or("Pending payload exceeds the memory budget")?)?;
 let mut charged=ChargedBytes{bytes,hold,plain:incoming};
 write_spool(ticket,&charged.bytes)?;
 if let Err(error)=remember_parked(ticket,&mut charged){retire_spool(ticket);return Err(error);}
 Ok(())
}
fn parked_len(ticket:&str)->Result<u64>{
 if ticket.is_empty()||ticket.len()>128{return Err("Invalid snapshot size or identity".into());}
 let slots=parked().lock().map_err(|_| "Pending payload service unavailable".to_string())?;
 let bytes=slots.get(ticket).ok_or("Pending payload is not available for delivery")?;
 u64::try_from(bytes.len()).map_err(|_| "Pending payload exceeds addressable memory".to_string())
}
const DELIVER_CHUNK:usize=65536;
fn deliver_parked(v:&Json)->Result<Json>{
 if crate::execution::cancelled(){return Err("Project snapshot was cancelled; it was not reported as saved".into());}
 let ticket=field(v,"stage_ticket")?;
 let offset=match get(v,"offset"){Null=>0u64,Num(n)=>n.parse::<u64>().map_err(|_| "Invalid payload offset".to_string())?,_=>return Err("Invalid payload offset".into())};
 let slots=parked().lock().map_err(|_| "Pending payload service unavailable".to_string())?;
 let bytes=slots.get(ticket).ok_or("Pending payload is not available for delivery")?;
 let len=u64::try_from(bytes.len()).map_err(|_| "Pending payload exceeds addressable memory".to_string())?;
 if len>crate::archive::project_budget(){return Err("Pending payload exceeds the memory budget".into());}
 if offset>len{return Err("Pending payload offset is past the end".into());}
 let start=usize::try_from(offset).map_err(|_| "Pending payload offset is past the end".to_string())?;
 let remain=usize::try_from(len-offset).map_err(|_| "Pending payload offset overflow".to_string())?;
 let n=remain.min(DELIVER_CHUNK);
 let end=start.checked_add(n).ok_or("Pending payload offset overflow")?;
 let chunk=bytes[start..end].to_vec();
 drop(slots);
 let next=offset.checked_add(n as u64).ok_or("Pending payload offset overflow")?;
 Ok(obj(vec![("stage_ticket",s(ticket)),("offset",Num(offset.to_string())),("next",Num(next.to_string())),("bytes",Num(len.to_string())),("chunk",Num(DELIVER_CHUNK.to_string())),("data",s(&network::b64(&chunk))),("last",Bool(next==len))]))
}
fn parked_effect(p:&Json,purpose:&str)->Result<Json>{
 if !boolean(p,"resident"){return Ok(obj(vec![("kind",s("project_payload")),("record",p.clone()),("purpose",s(purpose))]));}
 let ticket=text(p,"stage_ticket");
 let declared=match get(p,"bytes"){Num(n)=>n.parse::<u64>().map_err(|_| "Invalid snapshot size".to_string())?,_=>return Err("Missing snapshot size".into())};
 let held=parked_len(&ticket)?;
 if held!=declared||held>crate::archive::project_budget(){return Err("Saved payload size differs from its pinned metadata".into());}
 let proof=deliver_parked(&obj(vec![("stage_ticket",s(&ticket)),("offset",Num("0".into()))]))?;
 if get(&proof,"bytes")!=&Num(held.to_string())||get(&proof,"data")==&Null{return Err("Pending payload is not available for delivery".into());}
 let steps=held.saturating_add(262143)/262144;
 let secs=steps.saturating_add(120).min(900);
 Ok(obj(vec![("kind",s("project_payload")),("record",p.clone()),("purpose",s(purpose)),("parked",Bool(true)),("chunk",get(&proof,"chunk").clone()),("bytes",Num(held.to_string())),("deadline_seconds",Num(secs.to_string()))]))
}
fn wire(p:&Json,phase:&str,encoded:&str,bytes:&[u8])->Result<Json> {
 if !["write","probe","verify"].contains(&phase){return Err("Invalid pending transfer phase".into());}
 let mime=payload_mime(p)?;
 if boolean(p,"phone") {return Ok(obj(vec![("kind",s("project_phone")),("mime",s(&mime)),("tree",get(p,"tree").clone()),("name",get(p,"target").clone()),("phase",s(phase)),("purpose",get(p,"purpose").clone()),("archive",if phase=="write" {s(encoded)} else {Null})]));}
 let c=get(p,"connection");let url=text(p,"target");let mut headers=vec![];let authorization=network::storage_authorization(c)?;
 if let Some((name,value))=&authorization{headers.push((name.as_str(),value.clone()));}
 let mut e=if phase=="write"{headers.push(("Content-Type",mime));headers.push(("If-None-Match","*".into()));network::wire_binary(&url,boolean(c,"plain"),&bytes,headers)?}else{network::wire(&url,boolean(c,"plain"),"GET","",headers)?};
 network::tls_identity(&mut e,&text(c,"tls_name"))?;
 put(&mut e,"purpose",get(p,"purpose").clone());put(&mut e,"limit",Num((5*1024*1024+65536).to_string()));Ok(e)
}
fn request(st:&mut Json,phase:&str)->Result<Json>{
 let mut p=get(st,"project_transfer").clone();if p==Null{return Err("No pending project snapshot".into());}
 if text(&p,"phase")=="partial"{
  let reason=text(&p,"partial_reason");
  let reason=if reason.is_empty(){"parked bytes were not retained across restart".to_string()}else{reason};
  return Err(format!("Pending snapshot is partial: {reason}. It was not reported as saved"));
 }
 if text(&p,"phase")=="clearing" {return clear_effect(&p);}
 let purpose=format!("{PREFIX}{}-{}",now(),SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed));put(&mut p,"purpose",s(&purpose));put(&mut p,"phase",s(phase));
 let tree=text(&p,"tree");
 if boolean(&p,"phone")&&native_folder(&tree)&&text(&p,"stage_ticket").is_empty(){
  workspace::check_tool(st,"phone_write")?;
  device_destination(st,&tree)?;
  let encoded=text(&p,"archive");
  let data=if phase=="write"{Some(payload_bytes(&p,&encoded)?)}else{None};
  let response=crate::archive::deliver_native(Path::new(&tree),&text(&p,"target"),data.as_ref().map(|item|item.bytes.as_slice()));
  put(st,"project_transfer",p);
  return match response {
   Ok(actual)=>result(st,"project_phone_result",&obj(vec![
    ("purpose",s(&purpose)),("found",Bool(actual.is_some())),
    ("data",actual.map_or(Null,|bytes|s(&network::b64(&bytes)))),
   ])),
   Err(error)=>{interrupt(st);put(st,"busy",Bool(false));put(st,"notice",s(&error));Ok(Null)}
  };
 }
 let e=if text(&p,"stage_ticket").is_empty(){let encoded=text(&p,"archive");let bytes=payload_bytes(&p,&encoded)?;wire(&p,phase,&encoded,&bytes.bytes)?}else if boolean(&p,"resident")&&boolean(&p,"phone")&&native_folder(&text(&p,"tree"))&&!text(&p,"tree").starts_with("content://"){return complete_native_resident(st,&p,phase);}else if boolean(&p,"resident")&&!boolean(&p,"phone"){return complete_remote_resident(st,&p,phase);}else{parked_effect(&p,&purpose)?};
 put(st,"project_transfer",p);put(st,"busy",Bool(true));put(st,"notice",s("Snapshot pending destination verification"));Ok(e)
}
struct Payload {record:Json,bytes:Vec<u8>}
static PAYLOADS:std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<u64,Payload>>>=std::sync::OnceLock::new();
static RECEIPTS:std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<u64,Json>>>=std::sync::OnceLock::new();
fn receipts()->&'static std::sync::Mutex<std::collections::BTreeMap<u64,Json>>{RECEIPTS.get_or_init(||std::sync::Mutex::new(std::collections::BTreeMap::new()))}
static SERIAL:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
fn payloads()->&'static std::sync::Mutex<std::collections::BTreeMap<u64,Payload>>{PAYLOADS.get_or_init(||std::sync::Mutex::new(std::collections::BTreeMap::new()))}
pub(super) fn payload(v:&Json)->Result<Json>{
 let p=get(v,"record");let stored=get(v,"stored");
 if text(p,"stage_ticket").is_empty()||text(p,"purpose").is_empty(){return Err("Pending snapshot identity is missing".into());}
 for field in ["stage_ticket","bytes","phone","tree","connection","target","mime","source"] {if get(p,field)!=get(stored,field){return Err(format!("Saved snapshot {field} differs from the pinned transfer"));}}
 if get(v,"archive")==&Null{
  if parked_len(&text(p,"stage_ticket")).is_ok(){
   let mut pull=obj(vec![("stage_ticket",get(p,"stage_ticket").clone())]);
   if get(v,"offset")!=&Null{put(&mut pull,"offset",get(v,"offset").clone());}
   return deliver_parked(&pull);
  }
  return Err("Pending payload bytes are missing".into());
 }
 let encoded=match get(v,"archive"){Str(value)=>value.clone(),_=>return Err("Pending payload bytes are missing".into())};
 let mut charged=payload_bytes(p,&encoded)?;
 let wire_extra=if boolean(p,"phone"){0}else{base64_chars(charged.bytes.len().saturating_add(1024))?};
 let _wire=admit_extra(wire_extra)?;
 let mut e=wire(p,&text(p,"phase"),&encoded,&charged.bytes)?;
 let id=(||->Result<u64>{
  let _gate=budget_gate();
  let mut slots=payloads().lock().map_err(|_|"Pending payload service unavailable")?;
  if slots.len()>=2{return Err("Pending payload memory capacity is occupied; retry after active transfers stop".into());}
  let id=SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
  slots.insert(id,Payload{record:p.clone(),bytes:std::mem::take(&mut charged.bytes)});
  drop(slots);
  charged.release_plain_locked();
  Ok(id)
 })()?;
 put(&mut e,"snapshot_handle",Num(id.to_string()));put(&mut e,"stage_ticket",get(p,"stage_ticket").clone());Ok(e)
}
fn handle(v:&Json)->Result<u64>{match get(v,"handle"){Num(n)=>n.parse().map_err(|_|"Invalid payload handle".into()),_=>Err("Missing payload handle".into())}}
pub(super) fn close(v:&Json)->Result<Json>{let id=handle(v)?;payloads().lock().map_err(|_|"Pending payload service unavailable")?.remove(&id);Ok(Null)}
fn device_readback_bytes(v:&Json)->Result<ChargedBytes>{
 let encoded=match get(v,"data"){Str(value)=>value.as_str(),_=>return Err("Device readback is missing payload bytes; snapshot retained".into())};
 payload_bytes(&Null,encoded)
}
pub(super) fn readback(v:&Json)->Result<Json>{
 let id=handle(v)?;
 let phone={let slots=payloads().lock().map_err(|_|"Pending payload service unavailable")?;boolean(&slots.get(&id).ok_or("Pending payload handle expired")?.record,"phone")};
 let device=if phone{
  if get(v,"found")==&Bool(false){None}
  else if get(v,"found")!=&Bool(true){return Err("Missing device lookup result".into());}
  else {Some(device_readback_bytes(v)?)}
 }else{None};
 let network=if phone{None}else{
  let encoded=match get(v,"data"){Str(value)=>value.clone(),_=>return Err("Project readback is missing response bytes; snapshot retained".into())};
  if encoded.len()>8*1024*1024{return Err("Readback exceeds transfer bound".into());}
  let raw=network::unbase64(&encoded)?;
  Some(network::decoded_optional_bytes(&raw)?)
 };
 let (saved_record,saved_bytes)={
  let _gate=budget_gate();
  let mut slots=payloads().lock().map_err(|_|"Pending payload service unavailable")?;
  let saved=slots.remove(&id).ok_or("Pending payload handle expired")?;
  let charged=u64::try_from(saved.bytes.len()).map_err(|_| "Pending payload exceeds the memory budget".to_string())?;
  {
   let mut ledger=budget_ledger();
   match ledger.inflight.checked_add(charged){
    Some(next)=>{ledger.inflight=next;}
    None=>{drop(ledger);slots.insert(id,saved);return Err("Pending payload exceeds the memory budget".into());}
   }
  }
  (saved.record,LiveBytes{bytes:saved.bytes,charged})
 };
 let p=&saved_record;
 let mut out=obj(vec![("purpose",get(p,"purpose").clone()),("stage_ticket",get(p,"stage_ticket").clone()),("phase",get(p,"phase").clone())]);
 if phone {
  if let Some(actual)=device {put(&mut out,"found",Bool(true));put(&mut out,"verified",Bool(actual.bytes==saved_bytes.bytes));}
  else {put(&mut out,"found",Bool(false));}
 }else{
  let (status,body)=network.ok_or("Project readback is missing response bytes; snapshot retained")?;
  put(&mut out,"status",Num(status.to_string()));put(&mut out,"verified",Bool(body==saved_bytes.bytes));
 }
 make_receipt(p,out)
}
fn make_receipt(p:&Json,out:Json)->Result<Json>{
 let receipt=SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed);let mut entries=receipts().lock().map_err(|_|"Readback receipts unavailable")?;
 while entries.len()>=8{if let Some(old)=entries.keys().next().copied(){entries.remove(&old);}}entries.insert(receipt,out);
 Ok(obj(vec![("purpose",get(p,"purpose").clone()),("stage_ticket",get(p,"stage_ticket").clone()),("receipt",Num(receipt.to_string()))]))
}

fn checked_receipt(p:&Json,inputs:&Json)->Result<Json>{
 let id=match get(inputs,"receipt"){Num(n)=>n.parse::<u64>().map_err(|_|"Invalid readback receipt")?,_=>return Err("Missing Rust readback receipt".into())};
 let mut entries=receipts().lock().map_err(|_|"Readback receipts unavailable")?;let value=entries.get(&id).ok_or("Readback receipt was already consumed or expired")?;
 for field in ["purpose","stage_ticket","phase"]{if get(value,field)!=get(p,field){return Err("Readback receipt belongs to a different transfer".into());}}
 Ok(entries.remove(&id).unwrap())
}
fn clear_effect(p:&Json)->Result<Json>{Ok(obj(vec![("kind",s("project_clear")),("ticket",get(p,"stage_ticket").clone()),("purpose",get(p,"purpose").clone())]))}
fn finish(st:&mut Json,mut p:Json,notice:&str)->Result<Json>{
 unpark_transfer(&p);
 if text(&p,"stage_ticket").is_empty(){put(st,"project_transfer",Null);put(st,"busy",Bool(false));if ["project_transfer","project_transfer_discard_review"].contains(&text(st,"dialog").as_str()){put(st,"dialog",Null);}put(st,"notice",s(notice));return Ok(Null);}
 put(&mut p,"phase",s("clearing"));put(&mut p,"purpose",s(&format!("{PREFIX}{}-{}",now(),SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed))));put(&mut p,"completion",s(notice));
 let e=clear_effect(&p)?;put(st,"project_transfer",p);put(st,"busy",Bool(false));put(st,"notice",s("Finalizing encrypted pending storage"));Ok(e)
}
pub(super) fn stage_event(st:&mut Json,action:&str,inputs:&Json)->Result<Json>{
 let p=get(st,"project_transfer").clone();
 if action!="project_cleared"||p==Null||text(&p,"phase")!="clearing"||text(&p,"purpose")!=text(inputs,"purpose")||text(&p,"stage_ticket")!=text(inputs,"ticket"){return Ok(Null);}
 put(st,"notice",get(&p,"completion").clone());put(st,"project_transfer",Null);if ["project_transfer","project_transfer_discard_review"].contains(&text(st,"dialog").as_str()){put(st,"dialog",Null);}Ok(Null)
}
pub(super) fn recover(st:&mut Json,inputs:&Json){
 let current=get(st,"project_transfer").clone();let recovered=get(inputs,"project_record");
 if !text(&current,"stage_ticket").is_empty()&&text(inputs,"project_cleared_ticket")==text(&current,"stage_ticket")&&text(&current,"phase")=="clearing" {put(st,"notice",get(&current,"completion").clone());put(st,"project_transfer",Null);return;}
 if recovered!=&Null&&!text(recovered,"stage_ticket").is_empty() {
  if current==Null {
   let mut p=recovered.clone();put(&mut p,"purpose",Null);
   if boolean(&p,"resident"){
    let declared=match get(&p,"bytes"){Num(n)=>n.parse::<u64>().unwrap_or(0),_=>0};
    match reload_spool(&text(&p,"stage_ticket"),declared){
     Ok(())=>{put(&mut p,"phase",s("interrupted"));put(st,"notice",s("Recovered encrypted pending snapshot; verify or retry the original destination"));}
     Err(error)=>{put(&mut p,"phase",s("partial"));put(&mut p,"resident",Bool(false));put(&mut p,"partial_reason",s(&error));put(st,"notice",s(&format!("Recovered pending snapshot is partial: {error}")));}
    }
   }else{put(&mut p,"phase",s("interrupted"));put(st,"notice",s("Recovered encrypted pending snapshot; verify or retry the original destination"));}
   put(st,"project_transfer",p);
  }
  else if text(&current,"stage_ticket")!=text(recovered,"stage_ticket"){put(st,"notice",s("A different encrypted pending snapshot is preserved; current transfer was not replaced"));}
 }
 if !text(inputs,"project_error").is_empty(){put(st,"notice",s("Encrypted pending snapshot could not be opened; its bytes are preserved. Other chat and project operations remain available."));}
}
pub(super) fn event(st:&mut Json,action:&str)->Result<Json>{
 if action=="project_transfer_discard"{if text(st,"dialog")!="project_transfer_discard_review"{return Err("Review the pending snapshot before discarding it".into());}return finish(st,get(st,"project_transfer").clone(),"Pending snapshot discarded; project and remote files unchanged");}
 if action=="project_transfer_discard_review"{put(st,"dialog",s(action));return Ok(Null);}
 if action=="project_transfer_retry"{return request(st,"probe");}
 if action=="project_transfer_verify"{return request(st,"verify");}
 put(st,"dialog",s("project_transfer"));Ok(Null)
}
pub(super) fn modal(st:&Json)->Json{if text(st,"dialog")=="project_transfer_discard_review"{return super::modal("Discard pending project snapshot?",vec![field_ui("warning","Scope","This discards only the encrypted pending transfer. Project files and any partial or complete remote file remain unchanged.","readonly")],vec![button("Discard pending snapshot","project_transfer_discard")]);}let p=get(st,"project_transfer");super::modal("Pending project snapshot",vec![field_ui("destination","Original destination",&if boolean(p,"phone") {format!("{} · {}",text(p,"tree"),text(p,"target"))} else {endpoint_summary(&text(p,"target"))},"readonly"),field_ui("phase","Transfer state",&text(p,"phase"),"readonly"),field_ui("note","Recovery","The original encrypted archive and destination are retained until exact readback succeeds. Retry checks existing bytes first. A differing existing file is never overwritten; resolve it explicitly before retrying. No device fallback is reported as a remote save.","readonly")],vec![button("Verify existing snapshot","project_transfer_verify"),button("Retry pending snapshot","project_transfer_retry"),button("Discard pending snapshot","project_transfer_discard_review")])}
pub(super) fn result(st:&mut Json,action:&str,inputs:&Json)->Result<Json>{
 let p=get(st,"project_transfer").clone();if p==Null||text(&p,"purpose").is_empty()||text(&p,"purpose")!=text(inputs,"purpose"){return Ok(Null);}
 if text(&p,"phase")=="clearing"{if action=="failure"{put(st,"notice",s("Pending storage cleanup was interrupted; retry finalization from its review screen"));}return Ok(Null);}
 put(st,"busy",Bool(false));
 let result=(||->Result<Json>{
  if action=="failure"{return Err("Project transfer interrupted; original archive and destination retained".into());}
  if boolean(&p,"phone") {
   if action!="project_phone_result" {return Err("Unexpected response for device project transfer".into());}
  } else if action!="network_result" {
   return Err("Unexpected response for network project transfer".into());
  }
  let external=!text(&p,"stage_ticket").is_empty();
  if external&&text(inputs,"stage_ticket")!=text(&p,"stage_ticket"){return Err("Readback does not identify the saved snapshot".into());}
  let receipt;if external{receipt=checked_receipt(&p,inputs)?;}else{receipt=Null;}
  let inputs=if external{&receipt}else{inputs};
  let expected=if external{None}else{Some(payload_bytes(&p,&text(&p,"archive"))?)};
  if text(inputs,"data").len()>8*1024*1024{return Err("Project readback exceeds transfer limit".into());}
  if boolean(&p,"phone") {
   let phase=text(&p,"phase");if !["write","probe","verify"].contains(&phase.as_str()){return Err("No active device transfer matches this response".into());}
   if get(inputs,"found")==&Bool(false) {if phase=="probe" {return request(st,"write");}return Err("Project document was not found; snapshot remains pending".into());}
   if get(inputs,"found")!=&Bool(true){return Err("Device transfer response is missing an explicit lookup result; snapshot retained".into());}
   let matches=if external{get(inputs,"verified")==&Bool(true)}else{let actual=device_readback_bytes(inputs)?;actual.bytes.as_slice()==expected.as_ref().map(|item|item.bytes.as_slice()).unwrap_or(b"")};if !matches {return Err("Device project readback differs; original snapshot retained without overwriting the document".into());}
   return finish(st,p.clone(),"Project snapshot saved to the selected device folder and exact bytes verified");
  }
  let (status,matches)=if external {let status=match get(inputs,"status"){Num(n)=>n.parse::<u16>().map_err(|_|"Invalid readback HTTP status")?,_=>return Err("Missing readback HTTP status".into())};(status,get(inputs,"verified")==&Bool(true))}else{let encoded=match get(inputs,"data"){Str(value)=>value.as_str(),_=>return Err("Project readback is missing response bytes; snapshot retained".into())};let raw=network::unbase64(encoded)?;let(status,body)=network::decoded_optional_bytes(&raw)?;(status,body==expected.as_ref().map(|item|item.bytes.clone()).unwrap_or_default())};
  match text(&p,"phase").as_str(){
   "write"=>{if status!=201&&status!=204{return Err(format!("Project create returned HTTP {status}; snapshot remains pending"));}request(st,"verify")},
   "probe" if status==404=>request(st,"write"),
   "probe"|"verify"=>{if status!=200||!matches{return Err("Project readback differs from the saved snapshot; pending bytes retained".into());}finish(st,p.clone(),"Project snapshot saved to WebDAV and exact bytes verified")},
   _=>Err("No active project transfer matches this response".into())
  }
 })();match result{Ok(e)=>Ok(e),Err(e)=>{interrupt(st);put(st,"notice",s(&e));Ok(Null)}}
}

#[cfg(any(test, rllm_module_tests))] mod tests {
 use super::*;
 #[cfg(rllm_module_tests)]
 #[no_mangle]
 pub extern "C" fn g2_project_store_tests() {
  let spool=std::env::current_dir().unwrap().join("loops-lanes").join("G2-fix7-project_store-proof").join("spool");
  std::fs::create_dir_all(&spool).unwrap();
  std::env::set_var("RLLM_PROJECT_SPOOL",&spool);
  encrypted_pending_metadata_does_not_consume_history_capacity();
  pending_recovery_tombstones_and_receipts_are_identity_scoped();
  artifact_binary_snapshot_rejects_escape_links_and_large_files();
  artifact_pending_pins_bytes_type_and_target_without_replacing_another();
  phone_destination_accepts_content_tree_and_a_windows_path_only_on_windows();
  windows_file_snapshot_reads_a_regular_file_and_refuses_symlink_or_dotdot();
  native_folder_save_and_artifact_snapshot_finish_without_a_host();
  network_project_save_rejects_device_callback();
  project_zip_transfer_follows_archive_budget();
  large_native_folder_save_verifies_completed_bytes();
  missing_device_data_does_not_verify_empty_snapshot();
  parked_inline_bytes_are_delivered();
  cancelled_native_save_is_not_reported_saved();
  missing_archive_is_not_an_empty_save();
  large_project_save_reaches_the_network_folder();
  parked_bytes_survive_restart_or_stay_partial();
  #[cfg(not(target_os="android"))]
  large_artifact_snapshot_reaches_a_network_folder();
  directory_open_flags_stay_generic();
  importer_rejects_a_footer_without_members();
  external_project_record_requires_a_real_zip();
  spool_is_sealed_outside_the_system_temp();
  budget_reservation_counts_inflight_delivery();
  cancelled_connect_does_not_accumulate_threads();
  user_selected_snapshot_name_stays_and_staged_snapshot_is_retired();
  android_path_save_just_over_256_mib_is_not_refused();
  large_https_upload_uses_native_tls_off_windows();
  failed_android_large_admission_clears_prepare_and_next_action_runs();
 }
 #[cfg_attr(not(rllm_module_tests), test)] fn encrypted_pending_metadata_does_not_consume_history_capacity(){
  let mut st=blank();put(&mut st,"drive",obj(vec![("phone_default",Bool(true))]));put(&mut st,"phone_uri",s("content://provider/tree/original"));put(&mut st,"history_fixture",s(&"h".repeat(5*1024*1024)));
  let e=artifact_start(&mut st,"binary.bin").unwrap();let size=5*1024*1024;let stored=record(get(&e,"prepare"),size).unwrap();
  let input=obj(vec![("ticket",get(&e,"ticket").clone()),("external",Bool(true)),("record",stored.clone())]);let pending=built(&mut st,&input,true).unwrap();assert_eq!(text(&pending,"kind"),"project_payload");assert!(st.string().len()<6*1024*1024);assert_eq!(get(get(&st,"project_transfer"),"archive"),&Null);
  let encoded=network::b64(&vec![0xab;size]);let command=payload(&obj(vec![("record",get(&pending,"record").clone()),("stored",stored.clone()),("archive",s(&encoded))])).unwrap();
  let handle=get(&command,"snapshot_handle").clone();let checked=readback(&obj(vec![("handle",handle.clone()),("found",Bool(true)),("data",s(&encoded))])).unwrap();assert!(readback(&obj(vec![("handle",handle)])).is_err());
  let clear=result(&mut st,"project_phone_result",&checked).unwrap();assert_eq!(text(&clear,"kind"),"project_clear");assert!(!boolean(&st,"busy"));assert_eq!(text(get(&st,"project_transfer"),"phase"),"clearing");
  put(&mut st,"busy",Bool(true));put(&mut st,"request_record",s("unrelated model"));stage_event(&mut st,"project_cleared",&clear).unwrap();assert!(boolean(&st,"busy"));assert_eq!(get(&st,"project_transfer"),&Null);assert_eq!(text(&st,"request_record"),"unrelated model");
 }
 #[cfg_attr(not(rllm_module_tests), test)] fn pending_recovery_tombstones_and_receipts_are_identity_scoped(){
  let mut st=blank();put(&mut st,"drive",obj(vec![("phone_default",Bool(true))]));put(&mut st,"phone_uri",s("content://provider/tree/original"));let e=artifact_start(&mut st,"binary.bin").unwrap();let stored=record(get(&e,"prepare"),3).unwrap();
  interrupt(&mut st);put(&mut st,"busy",Bool(true));recover(&mut st,&obj(vec![("project_record",stored.clone())]));assert!(boolean(&st,"busy"));assert_eq!(text(get(&st,"project_transfer"),"stage_ticket"),text(&stored,"stage_ticket"));
  put(&mut st,"busy",Bool(false));let pending=request(&mut st,"probe").unwrap();let forged=obj(vec![("purpose",get(&pending,"purpose").clone()),("stage_ticket",get(&stored,"stage_ticket").clone()),("found",Bool(true)),("verified",Bool(true))]);result(&mut st,"project_phone_result",&forged).unwrap();assert_ne!(get(&st,"project_transfer"),&Null);
  let current=get(&st,"project_transfer").clone();let clear=finish(&mut st,current,"Discarded").unwrap();interrupt(&mut st);recover(&mut st,&obj(vec![("project_cleared_ticket",s("another"))]));assert_ne!(get(&st,"project_transfer"),&Null);
  recover(&mut st,&obj(vec![("project_cleared_ticket",get(&clear,"ticket").clone())]));assert_eq!(get(&st,"project_transfer"),&Null);
 }
 #[cfg_attr(not(rllm_module_tests), test)] fn artifact_binary_snapshot_rejects_escape_links_and_large_files(){
  let root=std::env::temp_dir().join(format!("rllm-artifact-{}",now()));fs::create_dir_all(root.join("nested")).unwrap();fs::write(root.join("nested/file.bin"),[0,255,128,80,75]).unwrap();
  let call=|path:&str|file_snapshot(&obj(vec![("root",s(root.to_str().unwrap())),("path",s(path))]));
  assert_eq!(network::unbase64(&text(&call("nested/file.bin").unwrap(),"archive")).unwrap(),[0,255,128,80,75]);
  for path in ["../file.bin","/etc/passwd","nested/../file.bin","nested//file.bin","nested","missing"]{assert!(call(path).is_err(),"{path}");}
  {let large=fs::File::create(root.join("large.bin")).unwrap();large.set_len(5*1024*1024+1).unwrap();}
  crate::archive::configure_project_budget(5*1024*1024);assert!(call("large.bin").is_err());
  crate::archive::configure_project_budget(8*1024*1024);
  let snap=call("large.bin").unwrap();
  assert_eq!(get(&snap,"bytes"),&Num("5242881".into()));
  crate::archive::configure_project_budget(0);
  #[cfg(unix)] {std::os::unix::fs::symlink(root.join("nested"),root.join("linked")).unwrap();std::os::unix::fs::symlink(root.join("nested/file.bin"),root.join("link.bin")).unwrap();assert!(call("linked/file.bin").is_err());assert!(call("link.bin").is_err());}
 }
 #[cfg_attr(not(rllm_module_tests), test)] fn artifact_pending_pins_bytes_type_and_target_without_replacing_another(){
  let mut st=blank();put(&mut st,"drive",obj(vec![("url",s("https://nas.test/share/")),("user",s("original"))]));
  let e=artifact_start(&mut st,"nested/file Ω.bin").unwrap();assert_eq!(text(&e,"kind"),"artifact_file");
  put(&mut st,"drive",obj(vec![("url",s("https://other.test/")),("phone_default",Bool(true))]));
  let input=obj(vec![("ticket",get(&e,"ticket").clone()),("archive",s(&network::b64(&[0,255,1])))]);
  assert_eq!(built(&mut st,&input,false).unwrap(),Null);
  let wire=built(&mut st,&input,true).unwrap();let bytes=network::unbase64(&text(&wire,"wire_base64")).unwrap();assert!(bytes.ends_with(&[0,255,1]));assert!(String::from_utf8_lossy(&bytes).contains("Content-Type: application/octet-stream"));assert_eq!(text(&wire,"host"),"nas.test");assert!(text(get(&st,"project_transfer"),"target").contains("file%20%CE%A9.bin"));
  let pending=get(&st,"project_transfer").clone();assert!(artifact_start(&mut st,"another.bin").is_err());assert_eq!(get(&st,"project_transfer"),&pending);
  assert_eq!(payload_mime(&obj(vec![])).unwrap(),"application/zip");assert!(payload_mime(&obj(vec![("mime",s("x\r\nInjected: value"))])).is_err());
 }
 #[cfg_attr(not(rllm_module_tests), test)] fn phone_destination_accepts_content_tree_and_a_windows_path_only_on_windows(){
  let mut st=blank();put(&mut st,"phone_uri",s("content://provider/tree/original"));
  let effect=start_destination(&mut st,true).unwrap();
  assert_eq!(text(&effect,"kind"),"archive_drive");
  assert!(boolean(get(&effect,"prepare"),"phone"));
  assert_eq!(text(get(&effect,"prepare"),"tree"),"content://provider/tree/original");
  let mut drive=blank();put(&mut drive,"phone_uri",s(r"C:\Users\Public\RLLM"));
  let mut unc=blank();put(&mut unc,"phone_uri",s(r"\\server\share\projects"));
  let mut slash=blank();put(&mut slash,"phone_uri",s("D:/work/tree"));
  #[cfg(windows)] {
   let saved=start_destination(&mut drive,true).unwrap();
   assert_eq!(text(get(&saved,"prepare"),"tree"),r"C:\Users\Public\RLLM");
   assert!(boolean(get(&saved,"prepare"),"phone"));
   assert_eq!(text(get(&start_destination(&mut unc,true).unwrap(),"prepare"),"tree"),r"\\server\share\projects");
   assert_eq!(text(get(&start_destination(&mut slash,true).unwrap(),"prepare"),"tree"),"D:/work/tree");
   let mut long_path=blank();put(&mut long_path,"phone_uri",s(r"\\?\C:\Users\Public\RLLM"));
   assert_eq!(text(get(&start_destination(&mut long_path,true).unwrap(),"prepare"),"tree"),r"\\?\C:\Users\Public\RLLM");
   let mut verbatim_unc=blank();put(&mut verbatim_unc,"phone_uri",s(r"\\?\UNC\server\share\projects"));
   assert_eq!(text(get(&start_destination(&mut verbatim_unc,true).unwrap(),"prepare"),"tree"),r"\\?\UNC\server\share\projects");
   let mut dotted=blank();put(&mut dotted,"phone_uri",s(r"C:\Users\..\Windows"));
   assert_eq!(start_destination(&mut dotted,true).unwrap_err(),"Grant a device folder before saving a project there");
   let mut wide=String::from(r"\\?\C:\");
   wide.extend(std::iter::repeat('字').take(12000));
   assert!(wide.len()>32767,"{}",wide.len());
   assert!(wide.encode_utf16().count()<=32767);
   let mut wide_path=blank();put(&mut wide_path,"phone_uri",s(&wide));
   assert_eq!(text(get(&start_destination(&mut wide_path,true).unwrap(),"prepare"),"tree"),wide);
   let mut too_wide=String::from(r"\\?\C:\");
   too_wide.extend(std::iter::repeat('字').take(40000));
   let mut too_wide_path=blank();put(&mut too_wide_path,"phone_uri",s(&too_wide));
   assert_eq!(start_destination(&mut too_wide_path,true).unwrap_err(),"Grant a device folder before saving a project there");
  }
  #[cfg(not(windows))] {
   assert_eq!(start_destination(&mut drive,true).unwrap_err(),"Grant a device folder before saving a project there");
   assert_eq!(start_destination(&mut unc,true).unwrap_err(),"Grant a device folder before saving a project there");
   assert_eq!(start_destination(&mut slash,true).unwrap_err(),"Grant a device folder before saving a project there");
   let mut posix=blank();put(&mut posix,"phone_uri",s("/var/rllm-projects"));
   #[cfg(target_os="android")]
   {
    assert!(start_destination(&mut posix,true).is_err());
    put(&mut posix,"phone_access_granted",Bool(true));
   }
   let saved=start_destination(&mut posix,true).unwrap();
   assert_eq!(text(get(&saved,"prepare"),"tree"),"/var/rllm-projects");
   let mut dotted=blank();put(&mut dotted,"phone_uri",s("/tmp/../etc"));
   assert_eq!(start_destination(&mut dotted,true).unwrap_err(),"Grant a device folder before saving a project there");
  }
  let mut other=blank();put(&mut other,"phone_uri",s("file:///tmp/project"));
  assert_eq!(start_destination(&mut other,true).unwrap_err(),"Grant a device folder before saving a project there");
 }
 #[cfg(windows)]
 #[cfg_attr(not(rllm_module_tests), test)]
 fn windows_file_snapshot_reads_a_regular_file_and_refuses_symlink_or_dotdot(){
  let root=std::env::temp_dir().join(format!("rllm-win-snap-{}",now()));
  let outside=std::env::temp_dir().join(format!("rllm-win-out-{}",now()));
  fs::create_dir_all(root.join("nested")).unwrap();fs::create_dir_all(&outside).unwrap();
  fs::write(root.join("nested").join("file.bin"),[7,8,9]).unwrap();fs::write(outside.join("secret.bin"),[1,2,3]).unwrap();
  let call=|path:&str|file_snapshot(&obj(vec![("root",s(root.to_str().unwrap())),("path",s(path))]));
  let snap=call("nested/file.bin").unwrap();
  assert_eq!(network::unbase64(&text(&snap,"archive")).unwrap(),[7,8,9]);
  assert_eq!(get(&snap,"bytes"),&Num("3".into()));
  assert!(call("../secret.bin").is_err());
  assert!(call("nested/../../secret.bin").is_err());
  if std::os::windows::fs::symlink_file(root.join("nested").join("file.bin"),root.join("link.bin")).is_ok(){assert!(call("link.bin").is_err());}
  else{
   let junction=root.join("linked");
   let created=std::process::Command::new("cmd").args(["/c","mklink","/J"]).arg(&junction).arg(&outside).status().map(|status|status.success()).unwrap_or(false);
   if created{assert!(call("linked/secret.bin").is_err());let _=fs::remove_dir(&junction);}else{assert!(call("../secret.bin").is_err());}
  }
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn native_folder_save_and_artifact_snapshot_finish_without_a_host(){
  let root=std::env::temp_dir().join(format!("rllm-native-src-{}",now()));
  let dest=std::env::temp_dir().join(format!("rllm-native-dst-{}",now()));
  fs::create_dir_all(root.join("nested")).unwrap();fs::create_dir_all(&dest).unwrap();
  fs::write(root.join("nested").join("note.txt"),b"saved-project").unwrap();
  fs::write(root.join("picture.bin"),[1,2,3,4]).unwrap();
  let mut st=blank();
  put(&mut st,"approval_root",s(root.to_str().unwrap()));
  put(&mut st,"phone_uri",s(dest.to_str().unwrap()));
  put(&mut st,"drive",obj(vec![("phone_default",Bool(true))]));
  let saved=start_destination(&mut st,true).unwrap();
  assert_eq!(saved,Null);
  assert!(!boolean(&st,"busy"));
  assert!(text(&st,"notice").contains("exact bytes verified"));
  let written=fs::read_dir(&dest).unwrap().map(|entry|entry.unwrap().path()).collect::<Vec<_>>();
  assert_eq!(written.len(),1,"{written:?}");
  let entries=crate::archive::unpack(&fs::read(&written[0]).unwrap()).unwrap();
  assert!(entries.iter().any(|(name,data)| name=="nested/note.txt" && data.as_slice()==b"saved-project"),"{entries:?}");
  #[cfg(not(target_os="android"))]
  {
   let mut artifact=blank();
   put(&mut artifact,"approval_root",s(root.to_str().unwrap()));
   put(&mut artifact,"phone_uri",s(dest.to_str().unwrap()));
   put(&mut artifact,"drive",obj(vec![("phone_default",Bool(true))]));
   let effect=artifact_start(&mut artifact,"picture.bin").unwrap();
   assert_eq!(effect,Null);
   assert!(text(&artifact,"notice").contains("exact bytes verified"));
   let names=fs::read_dir(&dest).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
   let artifact_name=names.iter().find(|name| name.contains("Artifact")).cloned().expect("artifact file");
   assert_eq!(fs::read(dest.join(artifact_name)).unwrap(),vec![1,2,3,4]);
  }
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn network_project_save_rejects_device_callback(){
  let zip=stored_zip("a.txt",b"hi");
  let created=b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n";
  let http=|body:&[u8]|{
   let mut raw=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",body.len()).into_bytes();
   raw.extend_from_slice(body);raw
  };
  let begin=|st:&mut Json|{
   put(st,"drive",obj(vec![("url",s("https://nas.test/share/"))]));
   let effect=start(st).unwrap();
   let input=obj(vec![("ticket",get(&effect,"ticket").clone()),("archive",s(&network::b64(&zip)))]);
   built(st,&input,false).unwrap()
  };
  let mut st=blank();
  let write=begin(&mut st);
  assert_eq!(text(&write,"kind"),"network");
  let rejected=result(&mut st,"project_phone_result",&obj(vec![("purpose",s(&text(&write,"purpose"))),("found",Bool(true)),("data",s(&network::b64(created)))])).unwrap();
  assert_eq!(rejected,Null);
  assert_eq!(text(get(&st,"project_transfer"),"phase"),"interrupted");
  assert!(text(&st,"notice").contains("network project transfer"),"{}",text(&st,"notice"));
  assert!(!text(&st,"notice").contains("exact bytes verified"));
  let mut st=blank();
  let write=begin(&mut st);
  let advanced=result(&mut st,"network_result",&obj(vec![("purpose",s(&text(&write,"purpose"))),("data",s(&network::b64(created)))])).unwrap();
  assert_eq!(text(get(&st,"project_transfer"),"phase"),"verify");
  let forged=result(&mut st,"project_phone_result",&obj(vec![("purpose",s(&text(&advanced,"purpose"))),("found",Bool(true)),("verified",Bool(true)),("data",s(&network::b64(&http(&zip))))])).unwrap();
  assert_eq!(forged,Null);
  assert_eq!(text(get(&st,"project_transfer"),"phase"),"interrupted");
  assert!(!text(&st,"notice").contains("exact bytes verified"));
  let mut st=blank();
  let write=begin(&mut st);
  let advanced=result(&mut st,"network_result",&obj(vec![("purpose",s(&text(&write,"purpose"))),("data",s(&network::b64(created)))])).unwrap();
  let verified=result(&mut st,"network_result",&obj(vec![("purpose",s(&text(&advanced,"purpose"))),("data",s(&network::b64(&http(&zip))))])).unwrap();
  assert_eq!(verified,Null);
  assert_eq!(get(&st,"project_transfer"),&Null);
  assert!(text(&st,"notice").contains("exact bytes verified"),"{}",text(&st,"notice"));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn project_zip_transfer_follows_archive_budget(){
  crate::archive::configure_project_budget(0);
  if crate::archive::project_budget()>256*1024*1024{
   let artifact=obj(vec![("artifact",Bool(true)),("ticket",s("artifact-over-cap"))]);
   let err=stream_record(&artifact,256*1024*1024+1).unwrap_err();
   assert!(err.contains("256 MiB"),"{err}");
   let project=obj(vec![("ticket",s("zip-over-inline")),("phone",Bool(false)),("artifact",Bool(false)),("connection",obj(vec![("url",s("https://nas.test/share/"))])),("tree",Null),("source",Null)]);
   let stored=stream_record(&project,256*1024*1024+1).unwrap();
   assert_eq!(get(&stored,"format"),&Num("2".into()));
   assert_eq!(get(&stored,"bytes"),&Num((256*1024*1024+1).to_string()));
   assert!(record(&project,256*1024*1024+1).is_ok());
  }
  crate::archive::configure_project_budget(1024*1024);
  let project=obj(vec![("ticket",s("zip-budget")),("phone",Bool(false)),("artifact",Bool(false)),("connection",obj(vec![("url",s("https://nas.test/share/"))])),("tree",Null),("source",Null)]);
  assert!(stream_record(&project,1024*1024+1).unwrap_err().contains("memory budget"));
  assert!(record(&project,1024*1024+1).unwrap_err().contains("memory budget"));
  assert!(payload_bytes(&Null,&network::b64(&vec![7u8;1024*1024+1])).unwrap_err().contains("memory budget"));
  crate::archive::configure_project_budget(8*1024*1024);
  let size=5*1024*1024+1;
  let encoded=network::b64(&vec![0x5Au8;size]);
  let rejected=payload_bytes(&Null,&encoded).unwrap_err();
  assert!(rejected.contains("memory budget")||rejected.contains("occupied"),"{rejected}");
  crate::archive::configure_project_budget(16*1024*1024);
  let mut shaped=vec![0u8;size];
  shaped[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
  shaped[size-22..size-18].copy_from_slice(&0x06054b50u32.to_le_bytes());
  let encoded=network::b64(&shaped);
  assert_eq!(payload_bytes(&Null,&encoded).unwrap().bytes,shaped);
  let mut st=blank();
  put(&mut st,"drive",obj(vec![("url",s("https://nas.test/share/"))]));
  let effect=start(&mut st).unwrap();
  let stored=stream_record(get(&effect,"prepare"),6*1024*1024).unwrap();
  let rejected=built(&mut st,&obj(vec![("ticket",get(&effect,"ticket").clone()),("external",Bool(true)),("record",stored)]),false).unwrap_err();
  assert!(rejected.contains("missing")||rejected.contains("not a ZIP"),"{rejected}");
  assert!(rejected.contains("was not saved"),"{rejected}");
  assert!(!rejected.contains("verified"),"{rejected}");
  assert_eq!(get(&st,"project_transfer"),&Null);
  crate::archive::configure_project_budget(0);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn large_native_folder_save_verifies_completed_bytes(){
  use std::io::Seek;
  let full=crate::archive::project_budget();
  if full>256*1024*1024{assert_eq!(native_readback_limit(),256*1024*1024);assert!(256*1024*1024+1>native_readback_limit());}
  crate::archive::configure_project_budget(64*1024);
  assert_eq!(native_readback_limit(),64*1024);
  crate::archive::configure_project_budget(0);
  let dest=std::env::temp_dir().join(format!("rllm-native-large-{}",now()));
  fs::create_dir_all(&dest).unwrap();
  let payload=b"PK\x03\x04exact-native-bytes";
  write_and_verify_native(&dest,"RLLM__Project-Snapshot-1.zip",payload).unwrap();
  assert_eq!(fs::read(dest.join("RLLM__Project-Snapshot-1.zip")).unwrap(),payload);
  fs::write(dest.join("kept.zip"),b"original").unwrap();
  assert!(write_and_verify_native(&dest,"kept.zip",b"replacement").is_err());
  assert_eq!(fs::read(dest.join("kept.zip")).unwrap(),b"original");
  let mut file=fs::File::open(dest.join("kept.zip")).unwrap();
  assert!(readback_matches(&mut file,b"original").is_ok());
  file.seek(std::io::SeekFrom::Start(0)).unwrap();
  assert!(readback_matches(&mut file,b"changed").is_err());
  NATIVE_READBACK_CAP.with(|slot|slot.set(32));
  let before=CHUNKED_NATIVE_SAVES.load(std::sync::atomic::Ordering::Relaxed);
  let root=std::env::temp_dir().join(format!("rllm-native-over-cap-{}",now()));
  let folder=std::env::temp_dir().join(format!("rllm-native-over-dst-{}",now()));
  fs::create_dir_all(root.join("nested")).unwrap();fs::create_dir_all(&folder).unwrap();
  fs::write(root.join("nested").join("note.txt"),b"larger-than-capped-readback").unwrap();
  let mut st=blank();
  put(&mut st,"approval_root",s(root.to_str().unwrap()));
  put(&mut st,"phone_uri",s(folder.to_str().unwrap()));
  let saved=start_destination(&mut st,true).unwrap();
  assert_eq!(saved,Null);
  assert!(text(&st,"notice").contains("exact bytes verified"),"{}",text(&st,"notice"));
  assert!(CHUNKED_NATIVE_SAVES.load(std::sync::atomic::Ordering::Relaxed)>before);
  let written=fs::read_dir(&folder).unwrap().map(|entry|entry.unwrap().path()).collect::<Vec<_>>();
  assert_eq!(written.len(),1,"{written:?}");
  let entries=crate::archive::unpack(&fs::read(&written[0]).unwrap()).unwrap();
  assert!(entries.iter().any(|(name,data)| name=="nested/note.txt" && data.as_slice()==b"larger-than-capped-readback"),"{entries:?}");
  NATIVE_READBACK_CAP.with(|slot|slot.set(0));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn missing_device_data_does_not_verify_empty_snapshot(){
  let phone_empty=|st:&mut Json|->Json{
   put(st,"drive",obj(vec![("phone_default",Bool(true))]));
   put(st,"phone_uri",s("content://provider/tree/original"));
   let effect=artifact_start(st,"empty.bin").unwrap();
   built(st,&obj(vec![("ticket",get(&effect,"ticket").clone()),("archive",s(&network::b64(&[])))]),true).unwrap()
  };
  let mut st=blank();
  let pending=phone_empty(&mut st);
  assert_eq!(text(&pending,"kind"),"project_phone");
  let missing=result(&mut st,"project_phone_result",&obj(vec![("purpose",s(&text(&pending,"purpose"))),("found",Bool(true))])).unwrap();
  assert_eq!(missing,Null);
  assert_eq!(text(get(&st,"project_transfer"),"phase"),"interrupted");
  assert!(text(&st,"notice").contains("missing payload bytes"),"{}",text(&st,"notice"));
  assert!(!text(&st,"notice").contains("exact bytes verified"));
  let mut st=blank();
  let pending=phone_empty(&mut st);
  let verified=result(&mut st,"project_phone_result",&obj(vec![("purpose",s(&text(&pending,"purpose"))),("found",Bool(true)),("data",s(""))])).unwrap();
  assert_eq!(verified,Null);
  assert_eq!(get(&st,"project_transfer"),&Null);
  assert!(text(&st,"notice").contains("exact bytes verified"),"{}",text(&st,"notice"));
  let mut st=blank();
  put(&mut st,"drive",obj(vec![("phone_default",Bool(true))]));
  put(&mut st,"phone_uri",s("content://provider/tree/original"));
  let effect=artifact_start(&mut st,"empty.bin").unwrap();
  let stored=record(get(&effect,"prepare"),0).unwrap();
  let pending=built(&mut st,&obj(vec![("ticket",get(&effect,"ticket").clone()),("external",Bool(true)),("record",stored.clone())]),true).unwrap();
  assert_eq!(text(&pending,"kind"),"project_payload");
  assert!(!boolean(&pending,"parked"));
  let command=payload(&obj(vec![("record",get(&pending,"record").clone()),("stored",stored),("archive",s(""))])).unwrap();
  let handle=get(&command,"snapshot_handle").clone();
  let omitted=readback(&obj(vec![("handle",handle.clone()),("found",Bool(true))]));
  assert!(omitted.is_err(),"{omitted:?}");
  assert!(!omitted.unwrap_err().contains("verified"));
  let checked=readback(&obj(vec![("handle",handle),("found",Bool(true)),("data",s(""))])).unwrap();
  let clear=result(&mut st,"project_phone_result",&checked).unwrap();
  assert_eq!(text(&clear,"kind"),"project_clear");
  assert!(text(get(&st,"project_transfer"),"completion").contains("exact bytes verified"),"{}",text(get(&st,"project_transfer"),"completion"));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn parked_inline_bytes_are_delivered(){
  crate::archive::configure_project_budget(12*1024*1024);
  let size=5*1024*1024+1;
  let body:Vec<u8>=(0..size).map(|i|(i%251) as u8).collect();
  let ticket="park-deliver".to_string();
  park_project_bytes(&ticket,body.clone(),0).unwrap();
  let record=obj(vec![("stage_ticket",s(&ticket)),("bytes",Num(size.to_string())),("phone",Bool(false)),("tree",Null),("connection",obj(vec![("url",s("https://nas.test/share/"))])),("target",s("https://nas.test/share/file.zip")),("mime",s("application/zip")),("source",Null),("purpose",s("project_transfer:park")),("phase",s("write"))]);
  let first=payload(&obj(vec![("record",record.clone()),("stored",record),("offset",Num("0".into()))])).unwrap();
  assert_eq!(get(&first,"verified"),&Null);
  assert_eq!(network::unbase64(&text(&first,"data")).unwrap(),body[..body.len().min(65536)]);
  let mut got=Vec::new();
  let mut offset=0u64;
  for _ in 0..200 {
   let part=chunks(&obj(vec![("action",s("parked")),("stage_ticket",s(&ticket)),("offset",Num(offset.to_string()))])).unwrap();
   assert_eq!(get(&part,"verified"),&Null);
   assert_ne!(text(&part,"last"),"saved");
   let data=network::unbase64(&text(&part,"data")).unwrap();
   assert!(data.len()<=65536);
   got.extend_from_slice(&data);
   let next=match get(&part,"next"){Num(n)=>n.parse::<u64>().unwrap(),other=>panic!("missing next {other:?}")};
   assert_eq!(next,offset+data.len() as u64);
   if boolean(&part,"last"){assert_eq!(next,size as u64);break;}
   assert!(next>offset);
   offset=next;
  }
  assert_eq!(got,body);
  assert!(chunks(&obj(vec![("action",s("parked")),("stage_ticket",s(&ticket)),("offset",Num((size as u64+1).to_string()))])).is_err());
  unpark_transfer(&obj(vec![("stage_ticket",s(&ticket))]));
  let gone=chunks(&obj(vec![("action",s("parked")),("stage_ticket",s(&ticket)),("offset",Num("0".into()))]));
  assert!(gone.is_err(),"{gone:?}");
  assert!(!gone.unwrap_err().contains("verified"));
  crate::archive::configure_project_budget(9*1024*1024);
  park_project_bytes("budget-a",vec![1u8;4*1024*1024],0).unwrap();
  let occupied=park_project_bytes("budget-b",vec![2u8;3*1024*1024],0).unwrap_err();
  assert!(occupied.contains("occupied")||occupied.contains("memory budget"),"{occupied}");
  assert!(!occupied.contains("verified"));
  unpark_transfer(&obj(vec![("stage_ticket",s("budget-a"))]));
  unpark_transfer(&obj(vec![("stage_ticket",s("budget-b"))]));
  crate::archive::configure_project_budget(0);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn cancelled_native_save_is_not_reported_saved(){
  struct Clear;
  impl Drop for Clear{fn drop(&mut self){crate::execution::reset();}}
  let _clear=Clear;
  crate::execution::reset();
  crate::execution::cancel();
  let dest=std::env::temp_dir().join(format!("rllm-native-cancel-{}",now()));
  fs::create_dir_all(&dest).unwrap();
  let err=write_and_verify_native(&dest,"RLLM__Project-Snapshot-1.zip",b"PK\x03\x04cancelled-bytes").unwrap_err();
  assert!(err.contains("cancelled"),"{err}");
  assert!(!err.contains("verified"),"{err}");
  assert!(!dest.join("RLLM__Project-Snapshot-1.zip").exists());
  crate::execution::reset();
 }
 fn content_len(header:&str)->usize{
  for line in header.split("\r\n"){
   if let Some((name,value))=line.split_once(':'){if name.eq_ignore_ascii_case("content-length"){return value.trim().parse().unwrap_or(0);}}
  }
  0
 }
 fn read_exchange(sock:&mut std::net::TcpStream)->(String,Vec<u8>){
  use std::io::Read;
  let mut buf=Vec::new();let mut tmp=[0u8;65536];
  loop{
   if let Some(at)=buf.windows(4).position(|mark|mark==b"\r\n\r\n"){
    let header=String::from_utf8_lossy(&buf[..at]).into_owned();
    let mut body=buf[at+4..].to_vec();let need=content_len(&header);
    while body.len()<need{let n=sock.read(&mut tmp).unwrap_or(0);if n==0{break;}body.extend_from_slice(&tmp[..n]);}
    body.truncate(need);return (header,body);
   }
   let n=sock.read(&mut tmp).unwrap_or(0);if n==0{break;}buf.extend_from_slice(&tmp[..n]);
  }
  (String::new(),Vec::new())
 }
 fn serve_put_get(stored:std::sync::Arc<std::sync::Mutex<Vec<u8>>>)->u16{
  let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let port=listener.local_addr().unwrap().port();
  std::thread::spawn(move ||{
   for _ in 0..8{
    let Ok((mut sock,_))=listener.accept() else{break;};
    sock.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
    let (header,body)=read_exchange(&mut sock);
    let method=header.split(' ').next().unwrap_or("");
    let response=if method=="PUT"{*stored.lock().unwrap()=body;b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()}else{
     let saved=stored.lock().unwrap().clone();let mut raw=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",saved.len()).into_bytes();raw.extend(saved);raw
    };
    use std::io::Write;let _=sock.write_all(&response);
   }
  });
  port
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn missing_archive_is_not_an_empty_save(){
  let mut st=blank();put(&mut st,"drive",obj(vec![("url",s("https://nas.test/share/"))]));
  let effect=start(&mut st).unwrap();
  let ticket=get(&effect,"ticket").clone();
  let missing=built(&mut st,&obj(vec![("ticket",ticket.clone())]),false).unwrap_err();
  assert!(missing.contains("missing"),"{missing}");
  assert!(!missing.contains("verified"),"{missing}");
  assert_eq!(get(&st,"project_transfer"),&Null);
  let bogus=built(&mut st,&obj(vec![("ticket",ticket),("archive",s(&network::b64(b"ZIP")))]),false).unwrap_err();
  assert!(bogus.contains("not a ZIP"),"{bogus}");
  assert!(!text(&st,"notice").contains("verified"));
  assert!(!text(&st,"notice").contains("saved"));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn large_project_save_reaches_the_network_folder(){
  crate::archive::configure_project_budget(32*1024*1024);
  let size=5*1024*1024+1;
  let body=stored_zip("a.txt",&vec![0x11u8;size]);
  let held=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
  let port=serve_put_get(held.clone());
  let mut st=blank();
  put(&mut st,"drive",obj(vec![("url",s(&format!("http://127.0.0.1:{port}/share/"))),("plain",Bool(true))]));
  let effect=start(&mut st).unwrap();
  let done=built(&mut st,&obj(vec![("ticket",get(&effect,"ticket").clone()),("archive",s(&network::b64(&body)))]),false).unwrap();
  assert_eq!(done,Null);
  assert!(text(&st,"notice").contains("exact bytes verified"),"{}",text(&st,"notice"));
  assert_eq!(get(&st,"project_transfer"),&Null);
  assert_eq!(held.lock().unwrap().as_slice(),body.as_slice());
  crate::archive::configure_project_budget(0);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn parked_bytes_survive_restart_or_stay_partial(){
  crate::archive::configure_project_budget(1024*1024);
  let body=b"kept".to_vec();
  park_project_bytes("spool-1",body.clone(),0).unwrap();
  parked().lock().unwrap().clear();
  let record=obj(vec![("stage_ticket",s("spool-1")),("resident",Bool(true)),("bytes",Num("4".into())),("phone",Bool(false)),("phase",s("ready"))]);
  let mut st=blank();
  recover(&mut st,&obj(vec![("project_record",record.clone())]));
  assert_eq!(parked().lock().unwrap().get("spool-1").cloned(),Some(body));
  assert!(text(&st,"notice").contains("retry"),"{}",text(&st,"notice"));
  assert_ne!(text(get(&st,"project_transfer"),"phase"),"partial");
  unpark_transfer(&obj(vec![("stage_ticket",s("spool-1"))]));
  let mut lost=blank();
  recover(&mut lost,&obj(vec![("project_record",record)]));
  assert_eq!(text(get(&lost,"project_transfer"),"phase"),"partial");
  assert!(text(&lost,"notice").contains("partial"),"{}",text(&lost,"notice"));
  assert!(!text(&lost,"notice").contains("verified"));
  let err=request(&mut lost,"probe").unwrap_err();
  assert!(err.contains("partial"),"{err}");
  assert!(!err.contains("verified"));
  crate::archive::configure_project_budget(0);
 }
 #[cfg(not(target_os="android"))]
 #[cfg_attr(not(rllm_module_tests), test)]
 fn large_artifact_snapshot_reaches_a_network_folder(){
  crate::archive::configure_project_budget(8*1024*1024);
  let root=std::env::temp_dir().join(format!("rllm-art-large-{}",now()));
  fs::create_dir_all(&root).unwrap();
  let body=vec![9u8;5*1024*1024+1];
  fs::write(root.join("large.bin"),&body).unwrap();
  let held=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
  let port=serve_put_get(held.clone());
  let mut st=blank();
  put(&mut st,"approval_root",s(root.to_str().unwrap()));
  put(&mut st,"drive",obj(vec![("url",s(&format!("http://127.0.0.1:{port}/share/"))),("plain",Bool(true))]));
  let effect=artifact_start(&mut st,"large.bin").unwrap();
  assert_eq!(effect,Null);
  assert!(text(&st,"notice").contains("exact bytes verified"),"{}",text(&st,"notice"));
  assert_eq!(held.lock().unwrap().as_slice(),body.as_slice());
  crate::archive::configure_project_budget(0);
 }
 fn crc32_bytes(bytes:&[u8])->u32{
  let mut crc=!0u32;
  for &byte in bytes{
   crc^=byte as u32;
   for _ in 0..8{crc=(crc>>1)^(0xedb88320&(0u32.wrapping_sub(crc&1)));}
  }
  !crc
 }
 fn stored_zip(name:&str,data:&[u8])->Vec<u8>{
  let name=name.as_bytes();
  let crc=crc32_bytes(data);
  let size=u32::try_from(data.len()).unwrap();
  let mut out=Vec::new();
  let mut directory=Vec::new();
  let offset=u32::try_from(out.len()).unwrap();
  out.extend_from_slice(&0x04034b50u32.to_le_bytes());
  out.extend_from_slice(&20u16.to_le_bytes());
  out.extend_from_slice(&0x0800u16.to_le_bytes());
  out.extend_from_slice(&0u16.to_le_bytes());
  out.extend_from_slice(&0u16.to_le_bytes());
  out.extend_from_slice(&0x0021u16.to_le_bytes());
  out.extend_from_slice(&crc.to_le_bytes());
  out.extend_from_slice(&size.to_le_bytes());
  out.extend_from_slice(&size.to_le_bytes());
  out.extend_from_slice(&(name.len() as u16).to_le_bytes());
  out.extend_from_slice(&0u16.to_le_bytes());
  out.extend_from_slice(name);
  out.extend_from_slice(data);
  directory.extend_from_slice(&0x02014b50u32.to_le_bytes());
  directory.extend_from_slice(&20u16.to_le_bytes());
  directory.extend_from_slice(&20u16.to_le_bytes());
  directory.extend_from_slice(&0x0800u16.to_le_bytes());
  directory.extend_from_slice(&0u16.to_le_bytes());
  directory.extend_from_slice(&0u16.to_le_bytes());
  directory.extend_from_slice(&0x0021u16.to_le_bytes());
  directory.extend_from_slice(&crc.to_le_bytes());
  directory.extend_from_slice(&size.to_le_bytes());
  directory.extend_from_slice(&size.to_le_bytes());
  directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
  directory.extend_from_slice(&0u16.to_le_bytes());
  directory.extend_from_slice(&0u16.to_le_bytes());
  directory.extend_from_slice(&0u16.to_le_bytes());
  directory.extend_from_slice(&0u16.to_le_bytes());
  directory.extend_from_slice(&0u32.to_le_bytes());
  directory.extend_from_slice(&offset.to_le_bytes());
  directory.extend_from_slice(name);
  let dir_at=u32::try_from(out.len()).unwrap();
  let dir_len=u32::try_from(directory.len()).unwrap();
  out.extend_from_slice(&directory);
  out.extend_from_slice(&0x06054b50u32.to_le_bytes());
  out.extend_from_slice(&0u16.to_le_bytes());
  out.extend_from_slice(&0u16.to_le_bytes());
  out.extend_from_slice(&1u16.to_le_bytes());
  out.extend_from_slice(&1u16.to_le_bytes());
  out.extend_from_slice(&dir_len.to_le_bytes());
  out.extend_from_slice(&dir_at.to_le_bytes());
  out.extend_from_slice(&0u16.to_le_bytes());
  out
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn directory_open_flags_stay_generic(){
  let flags=directory_open_flags();
  assert_eq!(flags&0x10000,0x10000,"{flags:#x}");
  assert_eq!(flags&0x20000,0x20000,"{flags:#x}");
  assert_eq!(flags&0x4000,0,"{flags:#x}");
  let created=create_snapshot_flags();
  assert_eq!(created&0x40,0x40,"{created:#x}");
  assert_eq!(created&0x80,0x80,"{created:#x}");
  assert_eq!(created&0x20000,0x20000,"{created:#x}");
  assert_eq!(read_snapshot_flags()&0x20000,0x20000);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn importer_rejects_a_footer_without_members(){
  let mut hostile=vec![0u8;64];
  hostile[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
  hostile[42..46].copy_from_slice(&0x06054b50u32.to_le_bytes());
  let err=project_zip(&hostile).unwrap_err();
  assert!(err.contains("not a ZIP"),"{err}");
  assert!(err.contains("was not saved"),"{err}");
  assert!(!err.contains("verified"),"{err}");
  assert!(crate::archive::unpack(&hostile).is_err());
  let real=stored_zip("a.txt",b"member");
  project_zip(&real).unwrap();
  let entries=crate::archive::unpack(&real).unwrap();
  assert!(entries.iter().any(|(name,data)| name=="a.txt" && data.as_slice()==b"member"),"{entries:?}");
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn external_project_record_requires_a_real_zip(){
  let mut st=blank();
  put(&mut st,"drive",obj(vec![("url",s("https://nas.test/share/"))]));
  let effect=start(&mut st).unwrap();
  let ticket=get(&effect,"ticket").clone();
  let mut hostile=vec![0u8;64];
  hostile[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
  hostile[42..46].copy_from_slice(&0x06054b50u32.to_le_bytes());
  let stored=record(get(&effect,"prepare"),hostile.len()).unwrap();
  let err=built(&mut st,&obj(vec![("ticket",ticket.clone()),("external",Bool(true)),("record",stored.clone()),("archive",s(&network::b64(&hostile)))]),false).unwrap_err();
  assert!(err.contains("not a ZIP"),"{err}");
  assert!(!err.contains("verified"),"{err}");
  assert_eq!(get(&st,"project_transfer"),&Null);
  let missing=built(&mut st,&obj(vec![("ticket",ticket),("external",Bool(true)),("record",stored)]),false).unwrap_err();
  assert!(missing.contains("missing")||missing.contains("not a ZIP"),"{missing}");
  let mut st=blank();
  put(&mut st,"drive",obj(vec![("url",s("https://nas.test/share/"))]));
  let effect=start(&mut st).unwrap();
  let zip=stored_zip("a.txt",b"external-member");
  let stored=record(get(&effect,"prepare"),zip.len()).unwrap();
  let pending=built(&mut st,&obj(vec![("ticket",get(&effect,"ticket").clone()),("external",Bool(true)),("record",stored),("archive",s(&network::b64(&zip)))]),false).unwrap();
  assert_eq!(text(&pending,"kind"),"project_payload");
  assert!(!text(&st,"notice").contains("exact bytes verified"));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn spool_is_sealed_outside_the_system_temp(){
  let saved=std::env::var("RLLM_PROJECT_SPOOL").ok();
  std::env::remove_var("RLLM_PROJECT_SPOOL");
  assert!(!private_spool_root().starts_with(std::env::temp_dir()));
  std::env::set_var("RLLM_PROJECT_SPOOL",std::env::temp_dir());
  assert!(!spool_dir().starts_with(std::env::temp_dir()));
  match saved{Some(value)=>std::env::set_var("RLLM_PROJECT_SPOOL",value),None=>std::env::remove_var("RLLM_PROJECT_SPOOL")}
  crate::archive::configure_project_budget(1024*1024);
  let marker=b"PLAINTEXT-PROJECT-ARCHIVE-MARKER-0123456789".to_vec();
  park_project_bytes("sealed-1",marker.clone(),0).unwrap();
  let path=spool_file("sealed-1").unwrap();
  assert!(!path.starts_with(std::env::temp_dir()),"{path:?}");
  let raw=fs::read(&path).unwrap();
  assert!(!raw.windows(marker.len()).any(|window| window==marker), "spool stored the archive in the clear");
  #[cfg(unix)]
  {
   use std::os::unix::fs::PermissionsExt;
   let mode=fs::symlink_metadata(&path).unwrap().permissions().mode();
   assert_eq!(mode&0o077,0,"{mode:o}");
  }
  parked().lock().unwrap().clear();
  let record=obj(vec![("stage_ticket",s("sealed-1")),("resident",Bool(true)),("bytes",Num(marker.len().to_string())),("phone",Bool(false)),("phase",s("ready"))]);
  let mut st=blank();
  recover(&mut st,&obj(vec![("project_record",record)]));
  assert_eq!(parked().lock().unwrap().get("sealed-1").cloned(),Some(marker));
  let outside=path.parent().unwrap().join("outside-target");
  let link=path.parent().unwrap().join("linked-1.partial");
  let _=fs::remove_file(&link);
  #[cfg(unix)]
  {let _=std::os::unix::fs::symlink(&outside,&link);}
  #[cfg(windows)]
  {let _=std::os::windows::fs::symlink_file(&outside,&link);}
  if fs::symlink_metadata(&link).map(|meta| meta.file_type().is_symlink()).unwrap_or(false){
   let err=write_spool("linked-1",b"should-not-follow");
   assert!(err.is_err(),"{err:?}");
   let leaked=fs::read(&outside).unwrap_or_default();
   assert!(!leaked.windows(b"should-not-follow".len()).any(|window| window==b"should-not-follow"));
  }
  unpark_transfer(&obj(vec![("stage_ticket",s("sealed-1"))]));
  unpark_transfer(&obj(vec![("stage_ticket",s("linked-1"))]));
  crate::archive::configure_project_budget(0);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn budget_reservation_counts_inflight_delivery(){
  crate::archive::configure_project_budget(2000);
  let first=admit_extra(1200).unwrap();
  let err=admit_extra(900).unwrap_err();
  assert!(err.contains("occupied")||err.contains("memory budget"),"{err}");
  drop(first);
  drop(admit_extra(900).unwrap());
  park_project_bytes("inflight-a",vec![9u8;600],0).unwrap();
  let live=take_parked("inflight-a").unwrap();
  let blocked=park_project_bytes("inflight-b",vec![8u8;900],0).unwrap_err();
  assert!(blocked.contains("occupied")||blocked.contains("memory budget"),"{blocked}");
  drop(live);
  park_project_bytes("inflight-b",vec![8u8;900],0).unwrap();
  unpark_transfer(&obj(vec![("stage_ticket",s("inflight-a"))]));
  unpark_transfer(&obj(vec![("stage_ticket",s("inflight-b"))]));
  crate::archive::configure_project_budget(0);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn cancelled_connect_does_not_accumulate_threads(){
  struct Clear;
  impl Drop for Clear{fn drop(&mut self){CONNECT_BLOCK_MS.store(0,std::sync::atomic::Ordering::Relaxed);crate::execution::reset();}}
  let _clear=Clear;
  CONNECT_BLOCK_MS.store(1500,std::sync::atomic::Ordering::Relaxed);
  let err=connect_until("127.0.0.1",1,std::time::Instant::now()+std::time::Duration::from_millis(250)).unwrap_err();
  assert!(err.contains("time bound"),"{err}");
  assert!(!err.contains("verified"),"{err}");
  let err=connect_until("127.0.0.1",1,std::time::Instant::now()+std::time::Duration::from_millis(250)).unwrap_err();
  assert!(err.contains("still finishing"),"{err}");
  CONNECT_BLOCK_MS.store(0,std::sync::atomic::Ordering::Relaxed);
  let started=std::time::Instant::now();
  let mut err=String::new();
  while started.elapsed()<std::time::Duration::from_secs(15){
   err=connect_until("127.0.0.1",1,std::time::Instant::now()+std::time::Duration::from_millis(200)).unwrap_err();
   if !err.contains("still finishing"){break;}
   std::thread::sleep(std::time::Duration::from_millis(50));
  }
  assert!(!err.contains("still finishing"),"{err}");
  assert!(!err.contains("verified"),"{err}");
 }
 #[cfg(windows)]
 fn mount_review_junction(link:&Path,target:&Path)->std::io::Result<()>{
  use std::os::windows::fs::OpenOptionsExt;
  use std::os::windows::io::AsRawHandle;
  let canonical=std::fs::canonicalize(target)?;
  let mut display=canonical.to_string_lossy().replace('/',"\\");
  if let Some(rest)=display.strip_prefix(r"\\?\UNC\"){display=format!(r"\\{rest}");}
  else if let Some(rest)=display.strip_prefix(r"\\?\"){display=rest.to_string();}
  let print=display.trim_end_matches('\\').to_string();
  let substitute=format!(r"\??\{print}");
  let sub:Vec<u16>=substitute.encode_utf16().collect();
  let printed:Vec<u16>=print.encode_utf16().collect();
  let mut path=Vec::new();
  path.extend_from_slice(&sub);
  path.push(0);
  let print_offset=path.len()*2;
  path.extend_from_slice(&printed);
  path.push(0);
  let path_bytes=path.len()*2;
  let data_length=8+path_bytes;
  let mut buf=Vec::<u8>::new();
  buf.extend_from_slice(&0xA000_0003u32.to_le_bytes());
  buf.extend_from_slice(&(data_length as u16).to_le_bytes());
  buf.extend_from_slice(&0u16.to_le_bytes());
  buf.extend_from_slice(&0u16.to_le_bytes());
  buf.extend_from_slice(&((sub.len()*2) as u16).to_le_bytes());
  buf.extend_from_slice(&(print_offset as u16).to_le_bytes());
  buf.extend_from_slice(&((printed.len()*2) as u16).to_le_bytes());
  for unit in path{buf.extend_from_slice(&unit.to_le_bytes());}
  let file=std::fs::OpenOptions::new().write(true).custom_flags(0x0200_0000|0x0020_0000).open(link)?;
  unsafe extern "system"{
   fn DeviceIoControl(handle:*mut std::ffi::c_void,code:u32,input:*const std::ffi::c_void,input_size:u32,output:*mut std::ffi::c_void,output_size:u32,returned:*mut u32,overlapped:*mut std::ffi::c_void)->i32;
  }
  let mut returned=0u32;
  let ok=unsafe{DeviceIoControl(file.as_raw_handle(),0x0009_00A4,buf.as_ptr().cast(),buf.len() as u32,std::ptr::null_mut(),0,&mut returned,std::ptr::null_mut())};
  if ok==0{Err(std::io::Error::last_os_error())}else{Ok(())}
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn user_selected_snapshot_name_stays_and_staged_snapshot_is_retired(){
  fn finish(root:&Path,source:&str,ticket:&str){
   let mut st=blank();
   put(&mut st,"approval_root",s(root.to_str().unwrap()));
   put(&mut st,"project_prepare",obj(vec![("connection",obj(vec![("url",s("https://nas.test/share/"))])),("phone",Bool(false)),("tree",Null),("ticket",s(ticket)),("artifact",Bool(true)),("source",s(source)),("name",s("RLLM__Artifact-kept")),("mime",s("application/octet-stream"))]));
   let stored=record(get(&st,"project_prepare"),1).unwrap();
   built(&mut st,&obj(vec![("ticket",s(ticket)),("external",Bool(true)),("record",stored)]),true).unwrap();
  }
  let root=std::env::temp_dir().join(format!("rllm-retire-{}",now()));
  fs::create_dir_all(root.join("review")).unwrap();
  fs::write(root.join("review").join("keep.txt"),b"keep").unwrap();
  fs::write(root.join("rllm-snapshot-x"),b"user-owned").unwrap();
  finish(&root,"rllm-snapshot-x","user-ticket");
  assert_eq!(fs::read(root.join("rllm-snapshot-x")).unwrap(),b"user-owned");
  let review_names=fs::read_dir(root.join("review")).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
  assert_eq!(review_names,vec!["keep.txt".to_string()],"{review_names:?}");
  let staged="rllm-snapshot-store.zip";
  publish_staged_snapshot(&root,staged,b"staged-bytes").unwrap();
  finish(&root,staged,"store-ticket");
  assert!(fs::symlink_metadata(root.join(staged)).is_err());
  assert_eq!(fs::read(root.join("rllm-snapshot-x")).unwrap(),b"user-owned");
  let moved=fs::read_dir(root.join("review")).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
  assert!(moved.iter().any(|name| name.starts_with(staged)),"{moved:?}");
  assert_eq!(fs::read(root.join("review").join(moved.iter().find(|name| name.starts_with(staged)).unwrap())).unwrap(),b"staged-bytes");
  assert!(moved.iter().any(|name| name=="keep.txt"));
  let linked=std::env::temp_dir().join(format!("rllm-retire-link-{}",now()));
  let outside=std::env::temp_dir().join(format!("rllm-retire-out-{}",now()));
  fs::create_dir_all(&linked).unwrap();
  fs::create_dir_all(&outside).unwrap();
  let review_link=linked.join("review");
  let mut linked_ok=false;
  #[cfg(unix)]
  {linked_ok=std::os::unix::fs::symlink(&outside,&review_link).is_ok();}
  #[cfg(windows)]
  {linked_ok=std::os::windows::fs::symlink_dir(&outside,&review_link).is_ok();}
  #[cfg(windows)]
  if !linked_ok{
   fs::create_dir(&review_link).unwrap();
   linked_ok=mount_review_junction(&review_link,&outside).is_ok();
  }
  assert!(linked_ok,"review directory link");
  let held="rllm-snapshot-held.zip";
  publish_staged_snapshot(&linked,held,b"stay-put").unwrap();
  finish(&linked,held,"held-ticket");
  assert_eq!(fs::read(linked.join(held)).unwrap(),b"stay-put");
  assert_eq!(fs::read_dir(&outside).unwrap().count(),0);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn android_path_save_just_over_256_mib_is_not_refused(){
  struct Reset;
  impl Drop for Reset{fn drop(&mut self){crate::archive::configure_project_budget(0);}}
  let _reset=Reset;
  let legacy=256*1024*1024u64;
  let just_over=legacy+1;
  crate::archive::configure_project_budget(0);
  let ceiling=crate::archive::project_budget();
  assert!(ceiling>=just_over,"{ceiling}");
  crate::archive::configure_project_budget(just_over);
  assert_eq!(crate::archive::project_budget(),just_over);
  assert!(android_stage_within_budget(just_over).is_ok());
  let over=android_stage_within_budget(just_over+1).unwrap_err();
  assert!(over.contains("memory budget"),"{over}");
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn large_https_upload_uses_native_tls_off_windows(){
  let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let port=listener.local_addr().unwrap().port();
  std::thread::spawn(move||{
   let Ok((sock,_))=listener.accept() else{return;};
   let _=sock.shutdown(std::net::Shutdown::Both);
  });
  let url=format!("https://127.0.0.1:{port}/share/file.zip");
  let connection=obj(vec![("url",s(&format!("https://127.0.0.1:{port}/share/"))),("plain",Bool(false))]);
  let body=stored_zip("a.txt",b"resident-https");
  let err=deliver_remote(&connection,&url,"application/zip",&body,"write").unwrap_err();
  assert!(!err.contains("requires an encrypted connection"),"{err}");
  assert_eq!(resident_https_transport(),if cfg!(windows){"system"}else{"native"});
  #[cfg(not(windows))]
  {assert!(err.contains("not reached")||err.contains("TLS")||err.contains("handshake")||err.contains("closed")||err.contains("trust"),"{err}");}
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn failed_android_large_admission_clears_prepare_and_next_action_runs(){
  struct Reset;
  impl Drop for Reset{fn drop(&mut self){crate::archive::configure_project_budget(0);}}
  let _reset=Reset;
  let root=std::env::temp_dir().join(format!("rllm-android-admit-{}",now()));
  fs::create_dir_all(&root).unwrap();
  fs::File::create(root.join("payload.bin")).unwrap().set_len(6*1024*1024).unwrap();
  let already=charged_bytes().unwrap();
  let budget=already.saturating_add(64*1024*1024);
  crate::archive::configure_project_budget(budget);
  let hold_n=budget.checked_sub(already).and_then(|n|n.checked_sub(5*1024*1024)).unwrap();
  let hold=admit_extra(hold_n).unwrap();
  let mut st=blank();
  put(&mut st,"approval_root",s(root.to_str().unwrap()));
  put(&mut st,"phone_uri",s("content://provider/tree/original"));
  put(&mut st,"drive",obj(vec![("url",s("https://nas.test/share/"))]));
  let err=if cfg!(target_os="android"){
   start_destination(&mut st,true).unwrap_err()
  }else{
   let opened=start_destination(&mut st,true).unwrap();
   assert_eq!(text(&opened,"kind"),"archive_drive");
   assert!(boolean(&st,"busy"));
   assert_ne!(get(&st,"project_prepare"),&Null);
   try_android_large_archive(&mut st).unwrap_err()
  };
  assert!(err.contains("occupied"),"{err}");
  assert!(!boolean(&st,"busy"),"{err}");
  assert_eq!(get(&st,"project_prepare"),&Null);
  drop(hold);
  crate::archive::configure_project_budget(0);
  let next=super::super::event(&obj(vec![("state",st),("action",s("project_save_drive")),("inputs",Null),("root",s(root.to_str().unwrap()))])).unwrap();
  assert_eq!(text(get(&next,"effect"),"kind"),"archive_drive");
 }
}

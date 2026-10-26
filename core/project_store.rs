#[path="transfer.rs"] mod transfer;
// Recoverable binary project snapshots with source-bound asynchronous replies.
use super::*;
const PREFIX:&str="project_transfer:";
pub(super) fn is_result(i:&Json)->bool{text(i,"purpose").starts_with(PREFIX)}
pub(super) fn interrupt(st:&mut Json){let mut p=get(st,"project_transfer").clone();if p!=Null{if text(&p,"phase")!="clearing"{put(&mut p,"phase",s("interrupted"));put(&mut p,"purpose",Null);}put(st,"project_transfer",p);}put(st,"project_prepare",Null);}
pub(super) fn start(st:&mut Json)->Result<Json>{start_destination(st,false)}
pub(super) fn start_default(st:&mut Json)->Result<Json>{start_destination(st,boolean(get(st,"drive"),"phone_default"))}
fn start_destination(st:&mut Json,phone:bool)->Result<Json>{
 if get(st,"project_transfer")!=&Null{return Err("A project snapshot is pending; verify or retry it before preparing another".into());}
 if phone {let tree=text(st,"phone_uri");if !tree.starts_with("content://") {return Err("Grant a phone folder before saving a project there".into());}} else {drive_root(st)?;}let ticket=format!("{}-{}",now(),SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed));put(st,"project_prepare",obj(vec![("connection",get(st,"drive").clone()),("phone",Bool(phone)),("tree",if phone {s(&text(st,"phone_uri"))} else {Null}),("ticket",s(&ticket))]));put(st,"busy",Bool(true));Ok(obj(vec![("kind",s("archive_drive")),("ticket",s(&ticket)),("prepare",get(st,"project_prepare").clone())]))
}
pub(super) fn artifact_start(st:&mut Json,source:&str)->Result<Json>{
 let parts=relative_parts(source)?;let basename=parts.last().ok_or("Choose a project file")?;
 if basename.len()>160 || basename.chars().any(|c|c.is_control()||"%:\\".contains(c)){return Err("Source filename must be at most 160 bytes without control, percent, colon or backslash characters".into());}
 let mut effect=start_default(st)?;let mut prepare=get(st,"project_prepare").clone();
 put(&mut prepare,"artifact",Bool(true));put(&mut prepare,"source",s(source));put(&mut prepare,"name",s(&format!("RLLM__Artifact-{}__{}",now(),basename)));put(&mut prepare,"mime",s("application/octet-stream"));put(st,"project_prepare",prepare);
 put(&mut effect,"kind",s("artifact_file"));put(&mut effect,"path",s(source));put(&mut effect,"prepare",get(st,"project_prepare").clone());put(st,"dialog",Null);put(st,"notice",s("Reading a bounded immutable file snapshot for the selected destination…"));Ok(effect)
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
 #[cfg(any(target_os="linux",target_os="android"))]
 {
  use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};use std::os::unix::ffi::OsStrExt;
  unsafe extern "C" {fn openat(fd:i32,path:*const std::ffi::c_char,flags:i32,...)->i32;}
  const NOFOLLOW:i32=131072;const DIRECTORY:i32=65536;const CLOEXEC:i32=524288;const NONBLOCK:i32=2048;
  let root=Path::new(field(v,"root")?).canonicalize().map_err(|e|e.to_string())?;
  let open=|fd:i32,name:&std::ffi::OsStr,flags:i32|->Result<fs::File>{let c=CString::new(name.as_bytes()).map_err(|_|"Invalid file path")?;let raw=unsafe{openat(fd,c.as_ptr(),flags)};if raw<0{return Err(std::io::Error::last_os_error().to_string());}Ok(unsafe{fs::File::from_raw_fd(raw)})};
  let mut directory=open(-100,root.as_os_str(),NOFOLLOW|DIRECTORY|CLOEXEC)?;
  for component in &parts[..parts.len()-1]{directory=open(directory.as_raw_fd(),std::ffi::OsStr::new(component),NOFOLLOW|DIRECTORY|CLOEXEC)?;}
  let file=open(directory.as_raw_fd(),std::ffi::OsStr::new(parts.last().unwrap()),NOFOLLOW|CLOEXEC|NONBLOCK)?;
  let before=file.metadata().map_err(|e|e.to_string())?;if !before.is_file()||before.len()>5*1024*1024{return Err("Select a regular project file of at most 5 MiB".into());}
  let mut bytes=Vec::new();(&file).take(5*1024*1024+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
  let after=file.metadata().map_err(|e|e.to_string())?;
  if bytes.len()>5*1024*1024||before.len()!=bytes.len() as u64||after.len()!=before.len()||before.modified().ok()!=after.modified().ok(){return Err("Project file changed while reading; no snapshot was transferred".into());}
  return Ok(obj(vec![("archive",s(&network::b64(&bytes))),("bytes",Num(bytes.len().to_string()))]));
 }
 #[cfg(not(any(target_os="linux",target_os="android")))]
 {let _=parts;Err("Secure project-file snapshot is unavailable on this platform".into())}
}
fn record(prepare:&Json,size:usize)->Result<Json>{
 if size>5*1024*1024||text(prepare,"ticket").is_empty(){return Err("Invalid snapshot size or identity".into());}
 let phone=boolean(prepare,"phone");let connection=get(prepare,"connection").clone();let artifact=boolean(prepare,"artifact");
 let name=if artifact{text(prepare,"name")}else{format!("RLLM__Project-Snapshot-{}.zip",text(prepare,"ticket"))};
 let target=if phone{name.clone()}else{format!("{}{}",drive_root(&obj(vec![("drive",connection.clone())]))?,network::encoded(&name))};
 Ok(obj(vec![("mime",s(if artifact{"application/octet-stream"}else{"application/zip"})),("source",get(prepare,"source").clone()),("phone",Bool(phone)),("tree",get(prepare,"tree").clone()),("connection",connection),("target",s(&target)),("archive",Null),("stage_ticket",get(prepare,"ticket").clone()),("bytes",Num(size.to_string())),("phase",s("ready"))]))
}
fn stream_record(prepare:&Json,size:usize)->Result<Json>{if !boolean(prepare,"artifact")||size>256*1024*1024{return Err("Chunked snapshots require a regular artifact up to 256 MiB".into());}let mut p=record(prepare,0)?;put(&mut p,"bytes",Num(size.to_string()));put(&mut p,"format",Num("2".into()));Ok(p)}
pub(super) fn chunks(v:&Json)->Result<Json>{transfer::call(v)}
pub(super) fn staging(v:&Json)->Result<Json>{let size=field(v,"bytes")?.parse::<usize>().map_err(|_|"Invalid snapshot size")?;record(get(v,"prepare"),size)}
pub(super) fn built(st:&mut Json,inputs:&Json,artifact:bool)->Result<Json>{
 let prepare=get(st,"project_prepare");if prepare==&Null||boolean(prepare,"artifact")!=artifact||text(prepare,"ticket")!=text(inputs,"ticket"){return Ok(Null);}
 let mut p;
 if boolean(inputs,"external") {
  let supplied=get(inputs,"record");let size=match get(supplied,"bytes"){Num(n)=>n.parse::<usize>().map_err(|_|"Invalid snapshot size")?,_=>return Err("Missing snapshot size".into())};
  p=if get(supplied,"format")==&Num("2".into()){stream_record(prepare,size)?}else{record(prepare,size)?};if &p!=supplied{return Err("Encrypted snapshot metadata differs from the preparation".into());}
 }else{
  let encoded=text(inputs,"archive");let bytes=payload_bytes(&Null,&encoded)?;p=record(prepare,bytes.len())?;put(&mut p,"stage_ticket",Null);put(&mut p,"archive",s(&encoded));
 }
 let mut next=st.clone();put(&mut next,"project_transfer",p);put(&mut next,"project_prepare",Null);
 if next.string().len()>6*1024*1024-4096{return Err("Encrypted history has insufficient room for this pending record; original files and staged bytes remain unchanged".into());}
 let e=request(&mut next,"write")?;*st=next;Ok(e)
}
fn payload_mime(p:&Json)->Result<String>{let value=text(p,"mime");if value.is_empty(){return Ok("application/zip".into());}if ["application/zip","application/octet-stream"].contains(&value.as_str()){Ok(value)}else{Err("Unsupported snapshot content type".into())}}
fn payload_bytes(p:&Json,encoded:&str)->Result<Vec<u8>> {
 if encoded.len()>7*1024*1024{return Err("Pending payload exceeds its bound".into());}
 let bytes=network::unbase64(encoded)?;
 if bytes.len()>5*1024*1024{return Err("Pending payload exceeds 5 MiB".into());}
 if !text(p,"stage_ticket").is_empty() && get(p,"bytes")!=&Num(bytes.len().to_string()){return Err("Saved payload size differs from its pinned metadata".into());}Ok(bytes)
}
fn wire(p:&Json,phase:&str,encoded:&str)->Result<Json> {
 if !["write","probe","verify"].contains(&phase){return Err("Invalid pending transfer phase".into());}
 let bytes=payload_bytes(p,encoded)?;let mime=payload_mime(p)?;
 if boolean(p,"phone") {return Ok(obj(vec![("kind",s("project_phone")),("mime",s(&mime)),("tree",get(p,"tree").clone()),("name",get(p,"target").clone()),("phase",s(phase)),("purpose",get(p,"purpose").clone()),("archive",if phase=="write" {s(encoded)} else {Null})]));}
 let c=get(p,"connection");let url=text(p,"target");let mut headers=vec![];let user=text(c,"user");
 if !user.is_empty(){headers.push(("Authorization",format!("Basic {}",network::b64(format!("{}:{}",user,text(c,"password")).as_bytes()))));}
 let mut e=if phase=="write"{headers.push(("Content-Type",mime));headers.push(("If-None-Match","*".into()));network::wire_binary(&url,boolean(c,"plain"),&bytes,headers)?}else{network::wire(&url,boolean(c,"plain"),"GET","",headers)?};
 put(&mut e,"purpose",get(p,"purpose").clone());put(&mut e,"limit",Num((5*1024*1024+65536).to_string()));Ok(e)
}
fn request(st:&mut Json,phase:&str)->Result<Json>{
 let mut p=get(st,"project_transfer").clone();if p==Null{return Err("No pending project snapshot".into());}
 if text(&p,"phase")=="clearing" {return clear_effect(&p);}
 let purpose=format!("{PREFIX}{}-{}",now(),SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed));put(&mut p,"purpose",s(&purpose));put(&mut p,"phase",s(phase));
 let e=if text(&p,"stage_ticket").is_empty(){wire(&p,phase,&text(&p,"archive"))?}else{obj(vec![("kind",s("project_payload")),("record",p.clone()),("purpose",s(&purpose))])};
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
 let encoded=text(v,"archive");let bytes=payload_bytes(p,&encoded)?;let mut e=wire(p,&text(p,"phase"),&encoded)?;
 let mut slots=payloads().lock().map_err(|_|"Pending payload service unavailable")?;if slots.len()>=2{return Err("Pending payload memory capacity is occupied; retry after active transfers stop".into());}
 let id=SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed);slots.insert(id,Payload{record:p.clone(),bytes});put(&mut e,"snapshot_handle",Num(id.to_string()));put(&mut e,"stage_ticket",get(p,"stage_ticket").clone());Ok(e)
}
fn handle(v:&Json)->Result<u64>{match get(v,"handle"){Num(n)=>n.parse().map_err(|_|"Invalid payload handle".into()),_=>Err("Missing payload handle".into())}}
pub(super) fn close(v:&Json)->Result<Json>{let id=handle(v)?;payloads().lock().map_err(|_|"Pending payload service unavailable")?.remove(&id);Ok(Null)}
pub(super) fn readback(v:&Json)->Result<Json>{
 let id=handle(v)?;let mut slots=payloads().lock().map_err(|_|"Pending payload service unavailable")?;
 let saved=slots.remove(&id).ok_or("Pending payload handle expired")?;drop(slots);let p=&saved.record;
 let mut out=obj(vec![("purpose",get(p,"purpose").clone()),("stage_ticket",get(p,"stage_ticket").clone()),("phase",get(p,"phase").clone())]);
 if boolean(p,"phone") {
  if get(v,"found")==&Bool(false){put(&mut out,"found",Bool(false));}
  else {if get(v,"found")!=&Bool(true){return Err("Missing phone lookup result".into());}
  let actual=payload_bytes(&Null,&text(v,"data"))?;put(&mut out,"found",Bool(true));put(&mut out,"verified",Bool(actual==saved.bytes));}
 }else{
  let encoded=text(v,"data");if encoded.len()>8*1024*1024{return Err("Readback exceeds transfer bound".into());}
  let raw=network::unbase64(&encoded)?;let (status,body)=network::decoded_optional_bytes(&raw)?;put(&mut out,"status",Num(status.to_string()));put(&mut out,"verified",Bool(body==saved.bytes));
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
  if current==Null {let mut p=recovered.clone();put(&mut p,"phase",s("interrupted"));put(&mut p,"purpose",Null);put(st,"project_transfer",p);put(st,"notice",s("Recovered encrypted pending snapshot; verify or retry the original destination"));}
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
pub(super) fn modal(st:&Json)->Json{if text(st,"dialog")=="project_transfer_discard_review"{return super::modal("Discard pending project snapshot?",vec![field_ui("warning","Scope","This discards only the encrypted pending transfer. Project files and any partial or complete remote file remain unchanged.","readonly")],vec![button("Discard pending snapshot","project_transfer_discard")]);}let p=get(st,"project_transfer");super::modal("Pending project snapshot",vec![field_ui("destination","Original destination",&if boolean(p,"phone") {format!("{} · {}",text(p,"tree"),text(p,"target"))} else {endpoint_summary(&text(p,"target"))},"readonly"),field_ui("phase","Transfer state",&text(p,"phase"),"readonly"),field_ui("note","Recovery","The original encrypted archive and destination are retained until exact readback succeeds. Retry checks existing bytes first. A differing existing file is never overwritten; resolve it explicitly before retrying. No phone fallback is reported as a remote save.","readonly")],vec![button("Verify existing snapshot","project_transfer_verify"),button("Retry pending snapshot","project_transfer_retry"),button("Discard pending snapshot","project_transfer_discard_review")])}
pub(super) fn result(st:&mut Json,action:&str,inputs:&Json)->Result<Json>{
 let p=get(st,"project_transfer").clone();if p==Null||text(&p,"purpose").is_empty()||text(&p,"purpose")!=text(inputs,"purpose"){return Ok(Null);}
 if text(&p,"phase")=="clearing"{if action=="failure"{put(st,"notice",s("Pending storage cleanup was interrupted; retry finalization from its review screen"));}return Ok(Null);}
 put(st,"busy",Bool(false));
 let result=(||->Result<Json>{
  if action=="failure"{return Err("Project transfer interrupted; original archive and destination retained".into());}
  let external=!text(&p,"stage_ticket").is_empty();
  if external&&text(inputs,"stage_ticket")!=text(&p,"stage_ticket"){return Err("Readback does not identify the saved snapshot".into());}
  let receipt;if external{receipt=checked_receipt(&p,inputs)?;}else{receipt=Null;}
  let inputs=if external{&receipt}else{inputs};
  let expected=if external{Vec::new()}else{payload_bytes(&p,&text(&p,"archive"))?};
  if text(inputs,"data").len()>8*1024*1024{return Err("Project readback exceeds transfer limit".into());}
  if boolean(&p,"phone") {
   if action!="project_phone_result" {return Err("Unexpected response for phone project transfer".into());}
   let phase=text(&p,"phase");if !["write","probe","verify"].contains(&phase.as_str()){return Err("No active phone transfer matches this response".into());}
   if get(inputs,"found")==&Bool(false) {if phase=="probe" {return request(st,"write");}return Err("Project document was not found; snapshot remains pending".into());}
   if get(inputs,"found")!=&Bool(true){return Err("Phone transfer response is missing an explicit lookup result; snapshot retained".into());}
   let matches=if external{get(inputs,"verified")==&Bool(true)}else{network::unbase64(&text(inputs,"data"))?==expected};if !matches {return Err("Phone project readback differs; original snapshot retained without overwriting the document".into());}
   return finish(st,p.clone(),"Project snapshot saved to the selected phone folder and exact bytes verified");
  }
  let (status,matches)=if external {let status=match get(inputs,"status"){Num(n)=>n.parse::<u16>().map_err(|_|"Invalid readback HTTP status")?,_=>return Err("Missing readback HTTP status".into())};(status,get(inputs,"verified")==&Bool(true))}else{let raw=network::unbase64(&text(inputs,"data"))?;let(status,body)=network::decoded_optional_bytes(&raw)?;(status,body==expected)};
  match text(&p,"phase").as_str(){
   "write"=>{if status!=201&&status!=204{return Err(format!("Project create returned HTTP {status}; snapshot remains pending"));}request(st,"verify")},
   "probe" if status==404=>request(st,"write"),
   "probe"|"verify"=>{if status!=200||!matches{return Err("Project readback differs from the saved snapshot; pending bytes retained".into());}finish(st,p.clone(),"Project snapshot saved to WebDAV and exact bytes verified")},
   _=>Err("No active project transfer matches this response".into())
  }
 })();match result{Ok(e)=>Ok(e),Err(e)=>{interrupt(st);put(st,"notice",s(&e));Ok(Null)}}
}

#[cfg(test)] mod tests {
 use super::*;
 #[test] fn encrypted_pending_metadata_does_not_consume_history_capacity(){
  let mut st=blank();put(&mut st,"drive",obj(vec![("phone_default",Bool(true))]));put(&mut st,"phone_uri",s("content://provider/tree/original"));put(&mut st,"history_fixture",s(&"h".repeat(5*1024*1024)));
  let e=artifact_start(&mut st,"binary.bin").unwrap();let size=5*1024*1024;let stored=record(get(&e,"prepare"),size).unwrap();
  let input=obj(vec![("ticket",get(&e,"ticket").clone()),("external",Bool(true)),("record",stored.clone())]);let pending=built(&mut st,&input,true).unwrap();assert_eq!(text(&pending,"kind"),"project_payload");assert!(st.string().len()<6*1024*1024);assert_eq!(get(get(&st,"project_transfer"),"archive"),&Null);
  let encoded=network::b64(&vec![0xab;size]);let command=payload(&obj(vec![("record",get(&pending,"record").clone()),("stored",stored.clone()),("archive",s(&encoded))])).unwrap();
  let handle=get(&command,"snapshot_handle").clone();let checked=readback(&obj(vec![("handle",handle.clone()),("found",Bool(true)),("data",s(&encoded))])).unwrap();assert!(readback(&obj(vec![("handle",handle)])).is_err());
  let clear=result(&mut st,"project_phone_result",&checked).unwrap();assert_eq!(text(&clear,"kind"),"project_clear");assert!(!boolean(&st,"busy"));assert_eq!(text(get(&st,"project_transfer"),"phase"),"clearing");
  put(&mut st,"busy",Bool(true));put(&mut st,"request_record",s("unrelated model"));stage_event(&mut st,"project_cleared",&clear).unwrap();assert!(boolean(&st,"busy"));assert_eq!(get(&st,"project_transfer"),&Null);assert_eq!(text(&st,"request_record"),"unrelated model");
 }
 #[test] fn pending_recovery_tombstones_and_receipts_are_identity_scoped(){
  let mut st=blank();put(&mut st,"drive",obj(vec![("phone_default",Bool(true))]));put(&mut st,"phone_uri",s("content://provider/tree/original"));let e=artifact_start(&mut st,"binary.bin").unwrap();let stored=record(get(&e,"prepare"),3).unwrap();
  interrupt(&mut st);put(&mut st,"busy",Bool(true));recover(&mut st,&obj(vec![("project_record",stored.clone())]));assert!(boolean(&st,"busy"));assert_eq!(text(get(&st,"project_transfer"),"stage_ticket"),text(&stored,"stage_ticket"));
  put(&mut st,"busy",Bool(false));let pending=request(&mut st,"probe").unwrap();let forged=obj(vec![("purpose",get(&pending,"purpose").clone()),("stage_ticket",get(&stored,"stage_ticket").clone()),("found",Bool(true)),("verified",Bool(true))]);result(&mut st,"project_phone_result",&forged).unwrap();assert_ne!(get(&st,"project_transfer"),&Null);
  let current=get(&st,"project_transfer").clone();let clear=finish(&mut st,current,"Discarded").unwrap();interrupt(&mut st);recover(&mut st,&obj(vec![("project_cleared_ticket",s("another"))]));assert_ne!(get(&st,"project_transfer"),&Null);
  recover(&mut st,&obj(vec![("project_cleared_ticket",get(&clear,"ticket").clone())]));assert_eq!(get(&st,"project_transfer"),&Null);
 }
 #[test] fn artifact_binary_snapshot_rejects_escape_links_and_large_files(){
  let root=std::env::temp_dir().join(format!("rllm-artifact-{}",now()));fs::create_dir_all(root.join("nested")).unwrap();fs::write(root.join("nested/file.bin"),[0,255,128,80,75]).unwrap();
  let call=|path:&str|file_snapshot(&obj(vec![("root",s(root.to_str().unwrap())),("path",s(path))]));
  assert_eq!(network::unbase64(&text(&call("nested/file.bin").unwrap(),"archive")).unwrap(),[0,255,128,80,75]);
  for path in ["../file.bin","/etc/passwd","nested/../file.bin","nested//file.bin","nested","missing"]{assert!(call(path).is_err(),"{path}");}
  let large=fs::File::create(root.join("large.bin")).unwrap();large.set_len(5*1024*1024+1).unwrap();assert!(call("large.bin").is_err());
  #[cfg(unix)] {std::os::unix::fs::symlink(root.join("nested"),root.join("linked")).unwrap();std::os::unix::fs::symlink(root.join("nested/file.bin"),root.join("link.bin")).unwrap();assert!(call("linked/file.bin").is_err());assert!(call("link.bin").is_err());}
 }
 #[test] fn artifact_pending_pins_bytes_type_and_target_without_replacing_another(){
  let mut st=blank();put(&mut st,"drive",obj(vec![("url",s("https://nas.test/share/")),("user",s("original"))]));
  let e=artifact_start(&mut st,"nested/file Ω.bin").unwrap();assert_eq!(text(&e,"kind"),"artifact_file");
  put(&mut st,"drive",obj(vec![("url",s("https://other.test/")),("phone_default",Bool(true))]));
  let input=obj(vec![("ticket",get(&e,"ticket").clone()),("archive",s(&network::b64(&[0,255,1])))]);
  assert_eq!(built(&mut st,&input,false).unwrap(),Null);
  let wire=built(&mut st,&input,true).unwrap();let bytes=network::unbase64(&text(&wire,"wire_base64")).unwrap();assert!(bytes.ends_with(&[0,255,1]));assert!(String::from_utf8_lossy(&bytes).contains("Content-Type: application/octet-stream"));assert_eq!(text(&wire,"host"),"nas.test");assert!(text(get(&st,"project_transfer"),"target").contains("file%20%CE%A9.bin"));
  let pending=get(&st,"project_transfer").clone();assert!(artifact_start(&mut st,"another.bin").is_err());assert_eq!(get(&st,"project_transfer"),&pending);
  assert_eq!(payload_mime(&obj(vec![])).unwrap(),"application/zip");assert!(payload_mime(&obj(vec![("mime",s("x\r\nInjected: value"))])).is_err());
 }
}

//! Bounded source and verified destination streams. Platform crypto owns only encrypted I/O.
use super::*;
use std::io::{Read,Seek,SeekFrom};
use std::sync::{Mutex,OnceLock};
const CHUNK:usize=65536;
const MAX:u64=256*1024*1024;
struct Source{file:fs::File,meta:fs::Metadata,read:u64}
static SOURCES:OnceLock<Mutex<std::collections::BTreeMap<u64,Source>>>=OnceLock::new();
fn sources()->&'static Mutex<std::collections::BTreeMap<u64,Source>>{SOURCES.get_or_init(||Mutex::new(std::collections::BTreeMap::new()))}
fn stable(a:&fs::Metadata,b:&fs::Metadata)->bool{
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;return a.dev()==b.dev()&&a.ino()==b.ino()&&a.len()==b.len()&&a.mtime()==b.mtime()&&a.mtime_nsec()==b.mtime_nsec()&&a.ctime()==b.ctime()&&a.ctime_nsec()==b.ctime_nsec();}
 #[cfg(not(unix))]{a.len()==b.len()&&a.modified().ok()==b.modified().ok()}
}
fn source_open(v:&Json)->Result<Json>{
 let parts=relative_parts(&field(v,"path")?)?;
 #[cfg(any(target_os="linux",target_os="android"))]{
 use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};use std::os::unix::ffi::OsStrExt;
 unsafe extern "C"{fn openat(fd:i32,path:*const std::ffi::c_char,flags:i32,...)->i32;}
 let root=Path::new(field(v,"root")?).canonicalize().map_err(|e|e.to_string())?;
 let open=|fd:i32,p:&std::ffi::OsStr,flags:i32|->Result<fs::File>{let p=CString::new(p.as_bytes()).map_err(|_|"Invalid path")?;let raw=unsafe{openat(fd,p.as_ptr(),flags|131072|524288)};if raw<0{return Err(std::io::Error::last_os_error().to_string());}Ok(unsafe{fs::File::from_raw_fd(raw)})};
 let mut d=open(-100,root.as_os_str(),65536)?;for p in &parts[..parts.len()-1]{d=open(d.as_raw_fd(),std::ffi::OsStr::new(p),65536)?;}
 let file=open(d.as_raw_fd(),std::ffi::OsStr::new(parts.last().unwrap()),2048)?;let meta=file.metadata().map_err(|e|e.to_string())?;
 if !meta.is_file()||meta.len()>MAX{return Err("Select a regular project file of at most 256 MiB".into());}let size=meta.len();
 let mut slots=sources().lock().map_err(|_|"Source registry unavailable")?;if slots.len()>=2{return Err("Source stream capacity occupied".into());}
 let id=SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed);slots.insert(id,Source{file,meta,read:0});Ok(obj(vec![("handle",Num(id.to_string())),("bytes",Num(size.to_string())),("chunk",Num(CHUNK.to_string()))]))
 }
 #[cfg(not(any(target_os="linux",target_os="android")))]{let _=parts;Err("Secure source streams unavailable".into())}
}
fn source_next(v:&Json,rewind:bool)->Result<Json>{let id=handle(v)?;let mut slots=sources().lock().map_err(|_|"Source registry unavailable")?;let p=slots.get_mut(&id).ok_or("Source expired")?;
 if !stable(&p.meta,&p.file.metadata().map_err(|e|e.to_string())?){slots.remove(&id);return Err("Source changed while staging".into());}
 if rewind {if p.read!=p.meta.len(){return Err("Source stream is incomplete".into());}p.file.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;p.read=0;return Ok(Null);}
 let count=(p.meta.len()-p.read).min(CHUNK as u64)as usize;let mut data=vec![0;count];p.file.read_exact(&mut data).map_err(|e|e.to_string())?;p.read+=count as u64;
 if !stable(&p.meta,&p.file.metadata().map_err(|e|e.to_string())?){slots.remove(&id);return Err("Source changed while staging".into());}
 Ok(obj(vec![("data",s(&network::b64(&data))),("done",Bool(p.read==p.meta.len()))]))}

use crate::network::ResponseFrame as Http;
struct Verify{record:Json,http:Http,decoded:u64,compared:u64,pending:Vec<u8>,matches:bool,phone:bool}
static VERIFY:OnceLock<Mutex<std::collections::BTreeMap<u64,Verify>>>=OnceLock::new();
fn verify()->&'static Mutex<std::collections::BTreeMap<u64,Verify>>{VERIFY.get_or_init(||Mutex::new(std::collections::BTreeMap::new()))}
fn size(p:&Json)->Result<u64>{match get(p,"bytes"){Num(n)=>{let n=n.parse::<u64>().map_err(|_|"Invalid size")?;if n>MAX{return Err("Snapshot exceeds 256 MiB".into());}Ok(n)},_=>Err("Missing snapshot size".into())}}
fn binding(v:&Json)->Result<Json>{let p=get(v,"record");let stored=get(v,"stored");if text(p,"stage_ticket").is_empty()||text(p,"purpose").is_empty()||get(p,"format")!=&Num("2".into()){return Err("Missing chunked snapshot identity".into());}
 for f in ["stage_ticket","bytes","phone","tree","connection","target","mime","source","format"]{if get(p,f)!=get(stored,f){return Err(format!("Saved {f} differs from pinned transfer"));}}
 size(p)?;Ok(Null)
}
fn open(v:&Json)->Result<Json>{binding(v)?;let p=get(v,"record");let len=size(p)?;let phase=text(p,"phase");if !["write","probe","verify"].contains(&phase.as_str()){return Err("Invalid transfer phase".into());}
 let phone=boolean(p,"phone");let mut e=if phone{obj(vec![("kind",s("project_phone")),("tree",get(p,"tree").clone()),("name",get(p,"target").clone()),("mime",s(&payload_mime(p)?)),("phase",s(&phase))])}else{let c=get(p,"connection");let user=text(c,"user");let mut headers=vec![];if !user.is_empty(){headers.push(("Authorization",format!("Basic {}",network::b64(format!("{}:{}",user,text(c,"password")).as_bytes()))));}if phase=="write"{headers.push(("Content-Type",payload_mime(p)?));headers.push(("If-None-Match","*".into()));}let mut e=network::wire(&text(p,"target"),boolean(c,"plain"),if phase=="write"{"PUT"}else{"GET"},"",headers)?;network::tls_identity(&mut e,&text(c,"tls_name"))?;if phase=="write"{let w=text(&e,"wire").replacen("Content-Length: 0\r\n",&format!("Content-Length: {len}\r\n"),1);put(&mut e,"wire",s(&w));}e};
 let mut slots=verify().lock().map_err(|_|"Transfer registry unavailable")?;if slots.len()>=2{return Err("Transfer stream capacity occupied".into());}let id=SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed);slots.insert(id,Verify{record:p.clone(),http:Http::default(),decoded:0,compared:0,pending:Vec::new(),matches:true,phone});put(&mut e,"deadline_seconds",Num((120+(len+262143)/262144).min(900).to_string()));put(&mut e,"record",p.clone());put(&mut e,"snapshot_handle",Num(id.to_string()));put(&mut e,"chunked",Bool(true));put(&mut e,"bytes",Num(len.to_string()));put(&mut e,"phase",s(&phase));put(&mut e,"purpose",get(p,"purpose").clone());Ok(e)
}
fn call_inner(v:&Json)->Result<Json>{match field(v,"action")?{
 "source_open"=>source_open(v),"source_next"=>source_next(v,false),"source_rewind"=>source_next(v,true),"source_close"=>{sources().lock().map_err(|_|"Source registry unavailable")?.remove(&handle(v)?);Ok(Null)},
 "binding"=>binding(v),
 "record"=>{let len=field(v,"bytes")?.parse::<usize>().map_err(|_|"Invalid size")?;stream_record(get(v,"prepare"),len)},"open"=>open(v),
 "close"=>{verify().lock().map_err(|_|"Transfer registry unavailable")?.remove(&handle(v)?);Ok(Null)},
 "feed"|"compare"=>{let mut slots=verify().lock().map_err(|_|"Transfer registry unavailable")?;let p=slots.get_mut(&handle(v)?).ok_or("Transfer expired")?;let encoded=field(v,"data")?;if encoded.len()>175000{return Err("Encoded chunk exceeds bound".into());}let data=network::unbase64(encoded)?;if data.len()>CHUNK*2{return Err("Chunk exceeds bound".into());}
 if field(v,"action")?=="feed"{if p.phone{return Err("Unexpected HTTP for phone transfer".into());}if p.decoded!=p.compared{return Err("Previous response chunk was not compared".into());}let body=p.http.feed(&data)?;p.decoded+=body.len()as u64;p.pending=body.clone();return Ok(obj(vec![("data",s(&network::b64(&body))),("status",Num(p.http.status.to_string())),("done",Bool(p.http.done))]));}
 let encoded=field(v,"expected")?;if encoded.len()>175000{return Err("Encoded expected chunk exceeds bound".into());}let expected=network::unbase64(encoded)?;if expected.len()>CHUNK*2{return Err("Expected chunk exceeds bound".into());}if p.phone{p.decoded+=data.len()as u64;p.pending=data;}if p.compared+p.pending.len()as u64!=p.decoded{return Err("Response chunk sequence differs".into());}p.matches &= p.pending==expected;p.pending.clear();p.compared=p.decoded;if p.compared>MAX{return Err("Readback exceeds bound".into());}Ok(Null)},
 "finish"=>{let mut slots=verify().lock().map_err(|_|"Transfer registry unavailable")?;let p=slots.remove(&handle(v)?).ok_or("Transfer expired")?;let mut out=obj(vec![("purpose",get(&p.record,"purpose").clone()),("stage_ticket",get(&p.record,"stage_ticket").clone()),("phase",get(&p.record,"phase").clone())]);if p.decoded!=p.compared{return Err("Uncompared response bytes".into());}
 if p.phone{let found=get(v,"found");if found!=&Bool(true)&&found!=&Bool(false){return Err("Missing phone lookup result".into());}if found==&Bool(false)&&p.compared!=0{return Err("Absent document contained bytes".into());}put(&mut out,"found",found.clone());}else{p.http.finish()?;put(&mut out,"status",Num(p.http.status.to_string()));}
 put(&mut out,"verified",Bool(p.matches&&p.compared==size(&p.record)?));make_receipt(&p.record,out)},
 _=>Err("Unknown transfer stream action".into())}}

pub(super) fn call(v:&Json)->Result<Json>{let result=call_inner(v);if result.is_err(){if let Ok(id)=handle(v){if let Ok(mut entries)=verify().lock(){entries.remove(&id);}if let Ok(mut entries)=sources().lock(){entries.remove(&id);}}}result}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn http_stream_framing_preserves_binary_and_rejects_truncation(){
  let bytes:Vec<u8>=(0..131113).map(|i|(i%251)as u8).collect();
  for chunked in [false,true]{let mut raw=if chunked{b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec()}else{format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",bytes.len()).into_bytes()};
   if chunked{for part in bytes.chunks(17003){raw.extend(format!("{:x}\r\n",part.len()).bytes());raw.extend(part);raw.extend(b"\r\n");}raw.extend(b"0\r\nCheck: done\r\n\r\n");}else{raw.extend(&bytes);}
   let mut h=Http::default();let mut decoded=Vec::new();for part in raw.chunks(113){decoded.extend(h.feed(part).unwrap());assert!(h.buffered()<=CHUNK*2);}h.finish().unwrap();assert_eq!(decoded,bytes);
   let mut bad=Http::default();for part in raw[..raw.len()-1].chunks(4096){bad.feed(part).unwrap();}assert!(bad.finish().is_err());
  }
  for raw in ["HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n","HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n","HTTP/1.1 204 OK\r\nContent-Length: 1\r\n\r\nx","HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nBad field: value\r\n\r\n"]{assert!(Http::default().feed(raw.as_bytes()).is_err());}
 }
 #[test]fn source_stream_large_binary_rewind_and_change_refusal(){
  let root=std::env::temp_dir().join(format!("rllm-stream-{}",now()));fs::create_dir_all(&root).unwrap();let bytes:Vec<u8>=(0..6*1024*1024+7).map(|i|(i%253)as u8).collect();fs::write(root.join("binary.bin"),&bytes).unwrap();
  let opened=source_open(&obj(vec![("root",s(root.to_str().unwrap())),("path",s("binary.bin"))])).unwrap();let id=get(&opened,"handle").clone();let v=obj(vec![("handle",id.clone())]);
  for _ in 0..2{let mut count=0;loop{let part=source_next(&v,false).unwrap();let data=network::unbase64(&text(&part,"data")).unwrap();assert!(data.len()<=CHUNK);assert_eq!(&bytes[count..count+data.len()],&data);count+=data.len();if boolean(&part,"done"){break;}}assert_eq!(count,bytes.len());source_next(&v,true).unwrap();}
  fs::write(root.join("binary.bin"),b"changed").unwrap();assert!(source_next(&v,false).is_err());assert!(!sources().lock().unwrap().contains_key(&handle(&v).unwrap()));
  #[cfg(unix)]{std::os::unix::fs::symlink(root.join("binary.bin"),root.join("link")).unwrap();assert!(source_open(&obj(vec![("root",s(root.to_str().unwrap())),("path",s("link"))])).is_err());}
 }
}

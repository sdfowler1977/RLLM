//! Owned input/output files; binary payloads do not enter conversation state.
use super::*;
use std::os::unix::fs::{MetadataExt,OpenOptionsExt};
use std::sync::{Mutex,OnceLock};
use std::time::{Instant,Duration};
const INPUT_LIMIT:usize=2*1024*1024;
const OUTPUT_LIMIT:usize=4*1024*1024;
pub(super) const FILE_LIMIT:usize=64*1024*1024;
// Reserve logical disk quota before a stream starts. Only unwritten padding is
// truncated on close; bytes actually received remain available in file management.
pub(super) struct StreamFile{file:fs::File,written:u64,limit:u64}
impl std::io::Write for StreamFile{
 fn write(&mut self,b:&[u8])->std::io::Result<usize>{if b.len()as u64>self.limit.saturating_sub(self.written){return Err(std::io::Error::other("Media storage reservation exceeded"));}let n=self.file.write(b)?;self.written+=n as u64;Ok(n)}
 fn flush(&mut self)->std::io::Result<()>{self.file.flush()}
}
impl StreamFile{fn capture(self)->Result<fs::File>{self.file.try_lock().map_err(|e|e.to_string())?;self.file.try_clone().map_err(|e|e.to_string())}
pub(super) fn finish(&self)->Result<()>{self.file.set_len(self.written).and_then(|_|self.file.sync_all()).map_err(|e|e.to_string())}}
impl Drop for StreamFile{fn drop(&mut self){let _=self.file.set_len(self.written);}}
type Capture=(String,Instant,Option<fs::File>);
static CAPTURES:OnceLock<Mutex<BTreeMap<String,Capture>>>=OnceLock::new();
type Stamp=(u64,u64,u64,i64,i64,i64,i64);
static VERIFIED:OnceLock<Mutex<BTreeMap<(String,String),Stamp>>>=OnceLock::new();
fn stamp(m:&fs::Metadata)->Stamp{(m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())}
fn verified()->&'static Mutex<BTreeMap<(String,String),Stamp>>{VERIFIED.get_or_init(||Mutex::new(BTreeMap::new()))}
fn owned_path(st:&Json,record:&Json,max:usize)->Result<PathBuf>{
 if get(record,"root")!=get(st,"approval_root_identity"){return Err("Media belongs to another workspace".into());}let relative=text(record,"path");let parts:Vec<_>=relative.split('/').collect();if parts.len()!=2||parts[0]!="service-files"||parts[1].is_empty()||parts[1]=="."||parts[1]==".."||parts[1].contains('\\'){return Err("Invalid owned media path".into());}
 let path=directory(st)?.join(parts[1]);let m=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;if !m.is_file()||m.file_type().is_symlink()||m.len()>max as u64||m.len()!=number(record,"bytes")as u64{return Err("Media file size or type changed".into());}Ok(path)
}
pub(super) fn verify(st:&Json,record:&Json,max:usize)->Result<PathBuf>{
 let path=owned_path(st,record,max)?;let m=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;let key=(path.to_string_lossy().into_owned(),text(record,"sha256"));if verified().lock().ok().is_some_and(|v|v.get(&key)==Some(&stamp(&m))){return Ok(path);}
 let mut file=fs::OpenOptions::new().read(true).custom_flags(131072).open(&path).map_err(|e|e.to_string())?;let before=file.metadata().map_err(|e|e.to_string())?;if stamp(&before)!=stamp(&m){return Err("Media changed before verification".into());}
 let mut digest=incremental_digest::Digest::new();let mut chunk=[0u8;65536];let mut count=0u64;loop{let n=file.read(&mut chunk).map_err(|e|e.to_string())?;if n==0{break;}count+=n as u64;if count>max as u64{return Err("Media exceeds verification bound".into());}digest.update(&chunk[..n])?;}
 let after=file.metadata().map_err(|e|e.to_string())?;let current=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;if count!=before.len()||digest.finish()!=text(record,"sha256"){return Err("Media byte count or digest changed; result withheld".into());}if stamp(&before)!=stamp(&after)||stamp(&before)!=stamp(&current){return Err("Media metadata changed during verification; result withheld".into());}if let Ok(mut cache)=verified().lock(){if cache.len()>=128{cache.clear();}cache.insert(key,stamp(&after));}Ok(path)
}
pub(super) fn verification(v:&Json)->Result<Json>{let st=obj(vec![("approval_root",get(v,"root").clone()),("approval_root_identity",get(v,"root_identity").clone())]);let path=verify(&st,get(v,"record"),256*1024*1024)?;Ok(obj(vec![("path",s(&path.to_string_lossy()))]))}
pub(super) fn stream_target(st:&Json,kind:&str,route:&str,limit:usize)->Result<(StreamFile,Json,usize)>{
 let name=if route=="capture"{format!("capture-{}.bin",now())}else{format!("{}.bin",now())};let(file,limit)=allocate(st,&name,limit)?;Ok((file,obj(vec![("path",s(&format!("service-files/{name}"))),("name",s("result.bin")),("kind",s(kind)),("route",s(route)),("root",get(st,"approval_root_identity").clone()),("root_path",get(st,"approval_root").clone())]),limit))
}
fn allocate(st:&Json,name:&str,limit:usize)->Result<(StreamFile,usize)>{
 use std::os::fd::{AsRawFd,FromRawFd};
 let dir=directory(st)?;let pinned=fs::OpenOptions::new().read(true).custom_flags(131072|65536).open(&dir).map_err(|e|e.to_string())?;
 pinned.try_lock().map_err(|e|format!("Media allocation is occupied or unavailable: {e}"))?;
 let current=fs::symlink_metadata(&dir).map_err(|e|e.to_string())?;let original=pinned.metadata().map_err(|e|e.to_string())?;if current.dev()!=original.dev()||current.ino()!=original.ino(){return Err("Media directory changed during allocation".into());}
 let mut count=0;let mut total=0u64;
 for item in fs::read_dir(&dir).map_err(|e|e.to_string())?.take(513){let item=item.map_err(|e|e.to_string())?;count+=1;let meta=fs::symlink_metadata(item.path()).map_err(|e|e.to_string())?;let mut length=meta.len();if item.file_name().to_string_lossy().starts_with("capture-")&&meta.is_file()&&!meta.file_type().is_symlink(){let active=fs::OpenOptions::new().read(true).custom_flags(131072).open(item.path()).map_err(|e|e.to_string())?;match active.try_lock(){Err(std::fs::TryLockError::WouldBlock)=>length=length.max(INPUT_LIMIT as u64),Err(error)=>return Err(error.to_string()),Ok(())=>{}}}total=total.saturating_add(length);}
 let limit=limit.min((256*1024*1024u64).saturating_sub(total)as usize);if count>=512||limit==0{return Err("Local media storage limit reached (512 files / 256 MiB)".into());}
 let current=fs::symlink_metadata(&dir).map_err(|e|e.to_string())?;if current.dev()!=original.dev()||current.ino()!=original.ino(){return Err("Media directory changed during admission".into());}
 unsafe extern "C"{fn openat(fd:i32,name:*const std::ffi::c_char,flags:i32,...)->i32;}let name=std::ffi::CString::new(name).map_err(|_|"Invalid media filename")?;let fd=unsafe{openat(pinned.as_raw_fd(),name.as_ptr(),0x80000|0x20000|0x80|0x40|1,0o600u32)};if fd<0{return Err(std::io::Error::last_os_error().to_string());}let file=unsafe{fs::File::from_raw_fd(fd)};file.set_len(limit as u64).map_err(|e|e.to_string())?;Ok((StreamFile{file,written:0,limit:limit as u64},limit))
}
pub(super) fn stream_complete(mut record:Json,bytes:usize,sha:&str)->Result<Json>{
 if bytes==0{return Err("Returned artifact is empty".into());}put(&mut record,"bytes",Num(bytes.to_string()));put(&mut record,"sha256",s(sha));let st=obj(vec![("approval_root",get(&record,"root_path").clone()),("approval_root_identity",get(&record,"root").clone())]);let path=verify(&st,&record,256*1024*1024)?;let mut file=fs::File::open(path).map_err(|e|e.to_string())?;let mut head=[0u8;512];let n=file.read(&mut head).map_err(|e|e.to_string())?;let(mime,kind)=if head.get(4..8)==Some(b"ftyp"){mp4_identity(&mut file,bytes as u64,&head[8..12])}else{identity(&head[..n])};let expected=text(&record,"kind");if !expected.is_empty()&&expected!="data"&&expected!=kind{return Err("Artifact container differs from configured kind".into());}if mime=="audio/wav"&&(n<44||u32::from_le_bytes(head[4..8].try_into().unwrap())as usize+8!=bytes){return Err("Invalid WAVE container length".into());}if mime=="model/gltf-binary"&&(n<12||head[4..8]!=2u32.to_le_bytes()||u32::from_le_bytes(head[8..12].try_into().unwrap())as usize!=bytes){return Err("Invalid scene container length".into());}put(&mut record,"mime",s(mime));put(&mut record,"kind",s(kind));put(&mut record,"name",s(output_name(mime)));put(&mut record,"validation",s("Complete HTTP framing, disk digest and container identity checked; decoder acceptance occurs at preview."));if let Obj(fields)=&mut record{fields.remove("root_path");}Ok(record)
}
pub(super) fn capture_allowed(ticket:&str)->bool{captures().lock().ok().is_some_and(|v|v.get(ticket).is_some_and(|(_,t,_)|t.elapsed()<Duration::from_secs(120)))}
fn captures()->&'static Mutex<BTreeMap<String,Capture>>{CAPTURES.get_or_init(||Mutex::new(BTreeMap::new()))}
fn binding(st:&Json)->String{harness_rules::digest(&obj(vec![("root",get(st,"approval_root").clone()),("identity",get(st,"approval_root_identity").clone()),("chat",get(st,"active").clone())]))}
fn directory(st:&Json)->Result<PathBuf>{
    let root=PathBuf::from(text(st,"approval_root"));let m=fs::metadata(&root).map_err(|e|e.to_string())?;
    if format!("{}:{}",m.dev(),m.ino())!=text(st,"approval_root_identity"){return Err("Media workspace identity changed".into());}
    let dir=root.join("service-files");match fs::create_dir(&dir){Ok(())=>{},Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(e)=>return Err(e.to_string())}
    let meta=fs::symlink_metadata(&dir).map_err(|e|e.to_string())?;if !meta.is_dir()||meta.file_type().is_symlink(){return Err("Media directory must be an owned directory, without a symbolic link".into());}Ok(dir)
}
fn output_name(mime:&str)->&'static str{match mime{"image/png"=>"result.png","image/jpeg"=>"result.jpg","audio/wav"=>"result.wav","audio/ogg"=>"result.ogg","audio/mpeg"=>"result.mp3","audio/mp4"=>"result.m4a","video/mp4"=>"result.mp4","application/pdf"=>"result.pdf","model/gltf-binary"=>"result.glb",_=>"result.bin"}}
fn export(st:&mut Json,record:&Json)->Result<Json>{verify(st,record,256*1024*1024)?;let mut effect=project_store::artifact_start(st,&text(record,"path"))?;let mut prepare=get(st,"project_prepare").clone();put(&mut prepare,"name",s(&format!("RLLM__Media__{}__{}",now(),filename(&text(record,"name")))));put(&mut prepare,"mime",get(record,"mime").clone());put(st,"project_prepare",prepare.clone());put(&mut effect,"prepare",prepare);Ok(effect)}
fn digest(bytes:&[u8])->Result<String>{let mut d=incremental_digest::Digest::new();d.update(bytes)?;Ok(d.finish())}
pub(super) fn filename(name:&str)->String{let clean:String=name.chars().filter(|c|c.is_ascii_alphanumeric()||"._-".contains(*c)).take(120).collect();if clean.is_empty(){"file.bin".into()}else{clean}}
// Inspect box boundaries and track handlers, rather than guessing from the file extension.
fn mp4_tracks<R:Read+std::io::Seek>(file:&mut R,start:u64,end:u64,depth:u8,budget:&mut usize)->std::io::Result<(bool,bool)>{
    use std::io::{SeekFrom,Error,ErrorKind};let mut pos=start;let(mut audio,mut video)=(false,false);
    while pos<end{if end-pos<8||*budget==0{return Err(Error::new(ErrorKind::InvalidData,"Invalid MP4 box bounds"));}*budget-=1;file.seek(SeekFrom::Start(pos))?;let mut head=[0u8;8];file.read_exact(&mut head)?;let short=u32::from_be_bytes(head[..4].try_into().unwrap());let mut header=8;let size=if short==1{let mut wide=[0;8];file.read_exact(&mut wide)?;header=16;u64::from_be_bytes(wide)}else if short==0{end-pos}else{short as u64};if size<header||size>end-pos{return Err(Error::new(ErrorKind::InvalidData,"Incomplete MP4 box"));}
        if [&b"moov"[..],&b"trak"[..],&b"mdia"[..]].contains(&&head[4..]){if depth>=3{return Err(Error::new(ErrorKind::InvalidData,"MP4 nesting exceeds bounds"));}let(a,v)=mp4_tracks(file,pos+header,pos+size,depth+1,budget)?;audio|=a;video|=v;}
        else if &head[4..]==b"hdlr"&&depth==3{if size<header+12{return Err(Error::new(ErrorKind::InvalidData,"Incomplete track handler"));}let mut handler=[0;12];file.read_exact(&mut handler)?;audio|=&handler[8..]==b"soun";video|=&handler[8..]==b"vide";}
        pos+=size;
    }Ok((audio,video))
}
fn mp4_identity<R:Read+std::io::Seek>(file:&mut R,size:u64,brand:&[u8])->(&'static str,&'static str){
    match mp4_tracks(file,0,size,0,&mut 4096){Ok((_,true))=>("video/mp4","video"),Ok((true,false))=>("audio/mp4","audio"),Ok(_ )if brand==b"M4A "=>("audio/mp4","audio"),_=>("application/octet-stream","data")}
}
fn identity(bytes:&[u8])->(&'static str,&'static str){
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n"){("image/png","image")}
    else if bytes.starts_with(b"\xff\xd8\xff"){("image/jpeg","image")}
    else if bytes.starts_with(b"RIFF")&&bytes.get(8..12)==Some(b"WAVE"){("audio/wav","audio")}
    else if bytes.starts_with(b"OggS"){("audio/ogg","audio")}
    else if bytes.starts_with(b"ID3")||bytes.get(..2).is_some_and(|v|v[0]==0xff&&v[1]&0xe0==0xe0){("audio/mpeg","audio")}
    else if bytes.get(4..8)==Some(b"ftyp"){mp4_identity(&mut std::io::Cursor::new(bytes),bytes.len()as u64,bytes.get(8..12).unwrap_or(&[]))}
    else if bytes.starts_with(b"%PDF-"){("application/pdf","document")}
    else if bytes.starts_with(b"glTF"){("model/gltf-binary","scene")}
    else{("application/octet-stream","data")}
}
pub(super) fn store(st:&Json,bytes:&[u8],name:&str,requested:&str,route:&str)->Result<Json>{
    if bytes.is_empty()||bytes.len()>OUTPUT_LIMIT{return Err("Media file must contain 1 byte to 4 MiB".into());}
    let(mime,kind)=identity(bytes);
    if !requested.is_empty()&&requested!="data"&&requested!=kind{return Err("Returned media container differs from the configured artifact kind".into());}
    if kind=="image"{super::super::attachments::image(&obj(vec![("name",s(&filename(name))),("mime",s(mime)),("bytes",Num(bytes.len().to_string())),("data",s(&network::b64(bytes)))]))?;}
    if mime=="audio/wav"&&(bytes.len()<44||u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize+8!=bytes.len()){return Err("Incomplete or unsupported WAVE container length".into());}
    if mime=="model/gltf-binary"&&(bytes.len()<12||bytes[4..8]!=2u32.to_le_bytes()||u32::from_le_bytes(bytes[8..12].try_into().unwrap())as usize!=bytes.len()){return Err("Invalid binary scene container".into());}
    let file=format!("{}-{}",now(),filename(name));let(mut out,limit)=allocate(st,&file,bytes.len())?;if limit<bytes.len(){return Err("Media storage has insufficient room for this file".into());}out.write_all(bytes).map_err(|e|e.to_string())?;out.finish()?;drop(out);
    Ok(obj(vec![("path",s(&format!("service-files/{file}"))),("name",s(&filename(if name=="result.bin"{output_name(mime)}else{name}))),("mime",s(mime)),("kind",s(kind)),("bytes",Num(bytes.len().to_string())),("sha256",s(&digest(bytes)?)),("root",get(st,"approval_root_identity").clone()),("route",s(route)),("validation",s("Complete bytes and digest; container identity checked. Decoder acceptance occurs during preview."))]))
}
pub(super) fn read(st:&Json,record:&Json,max:usize)->Result<Vec<u8>>{
    let path=verify(st,record,max)?;let file=fs::OpenOptions::new().read(true).custom_flags(131072).open(&path).map_err(|e|e.to_string())?;let before=file.metadata().map_err(|e|e.to_string())?;
    let mut bytes=Vec::new();file.take(max as u64+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;let current=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;
    if bytes.len()>max||bytes.len()!=number(record,"bytes")||stamp(&before)!=stamp(&current)||digest(&bytes)?!=text(record,"sha256"){return Err("Media changed during bounded input reading".into());}Ok(bytes)
}
pub(super) fn input(st:&Json)->Result<Vec<u8>>{read(st,get(st,"media_input"),INPUT_LIMIT)}
pub(super) struct InputFile{pub(super) file:fs::File,path:PathBuf,stamp:Stamp}
impl InputFile{pub(super) fn current(&self)->Result<()>{let live=fs::symlink_metadata(&self.path).map_err(|e|e.to_string())?;let opened=self.file.metadata().map_err(|e|e.to_string())?;if stamp(&live)!=self.stamp||stamp(&opened)!=self.stamp{return Err("Selected file changed during upload; submission was interrupted and will not be replayed".into());}Ok(())}}
pub(super) fn input_file(st:&Json)->Result<InputFile>{
 let record=get(st,"media_input");let path=verify(st,record,FILE_LIMIT)?;let file=fs::OpenOptions::new().read(true).custom_flags(131072).open(&path).map_err(|e|e.to_string())?;let meta=file.metadata().map_err(|e|e.to_string())?;
 if !verified().lock().ok().is_some_and(|v|v.get(&(path.to_string_lossy().into_owned(),text(record,"sha256")))==Some(&stamp(&meta))){return Err("Selected file changed before upload".into());}let out=InputFile{file,path,stamp:stamp(&meta)};out.current()?;Ok(out)
}
pub(super) fn upload(st:&Json,r:&Json,input:&Json,send_file:bool)->Result<(Vec<u8>,String)>{
    let format=text(r,"request_format");if !["","json","binary","multipart"].contains(&format.as_str()){return Err("Unsupported request format".into());}
    if format=="binary"{if !send_file{return Err("Explicit file selection is required for a binary upload".into());}return Ok((self::input(st)?,text(get(st,"media_input"),"mime")));}
    let template=if get(r,"request")==&Null{input.clone()}else{get(r,"request").clone()};
    let mut map=|value:&Json|->Result<Json>{if matches!(value,Obj(m)if m.len()==1&&m.contains_key("$file")){if !send_file{return Err("This mapping requires explicit file:true".into());}match field(value,"$file")?{"base64"=>Ok(s(&network::b64(&self::input(st)?))),"name"=>Ok(get(get(st,"media_input"),"name").clone()),"mime"=>Ok(get(get(st,"media_input"),"mime").clone()),"data_url"=>Ok(s(&format!("data:{};base64,{}",text(get(st,"media_input"),"mime"),network::b64(&self::input(st)?)))),_=>Err("Unknown file mapping".into())}}else{Ok(value.clone())}};
    let mapped=services::map_request(&template,input,0,&mut (12*1024*1024),&mut map)?;
    if format!="multipart"{let bytes=mapped.string().into_bytes();if bytes.len()>4*1024*1024{return Err("Mapped JSON request exceeds 4 MiB".into());}return Ok((bytes,"application/json".into()));}
    if !send_file{return Err("Explicit file:true is required for multipart upload".into());}
    let file=self::input(st)?;let boundary=format!("rllm-{}",now());if file.windows(boundary.len()).any(|v|v==boundary.as_bytes()){return Err("Multipart boundary collides with file contents; review again".into());}
    let mut body=Vec::new();let Obj(fields)=mapped else{return Err("Multipart request mapping must produce an object".into());};
    for(k,v)in fields{if k.is_empty()||k.len()>128||!k.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b)){return Err("Invalid multipart field name".into());}body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{}\r\n",if let Str(v)=v{v}else{v.string()}).as_bytes());}
    let field=text(r,"file_field");let field=if field.is_empty(){"file"}else{&field};if !field.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b)){return Err("Invalid multipart file field".into());}
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",filename(&text(get(st,"media_input"),"name")),text(get(st,"media_input"),"mime")).as_bytes());body.extend_from_slice(&file);body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());Ok((body,format!("multipart/form-data; boundary={boundary}")))
}
pub(super) fn callback(action:&str)->bool{matches!(action,"media_input_result"|"media_input_cancel"|"media_input_failure")}
pub(super) fn consume(st:&mut Json,input:&Json)->bool{
    let expected=get(st,"media_expected");let ticket=text(input,"media_ticket");if text(expected,"ticket")!=ticket{return false;}
    let Ok(mut held)=captures().lock()else{return false;};if !held.get(&ticket).is_some_and(|(b,_,_)|*b==text(expected,"binding")){return false;}let(_,created,_)=held.remove(&ticket).unwrap();if created.elapsed()>Duration::from_secs(120)||text(expected,"binding")!=binding(st){put(st,"media_expected",Null);put(st,"notice",s("Media selection expired or context changed; nothing was adopted"));}true
}
pub(super) fn interrupt(st:&mut Json){let ticket=text(get(st,"media_expected"),"ticket");media_input::cancel(&ticket);if let Ok(mut held)=captures().lock(){held.remove(&ticket);}put(st,"media_expected",Null);}
pub(super) fn retain_artifact(st:&mut Json,record:&Json)->Result<()>{if chat(st)==Null{new_chat(st)?;}let mut current=chat(st);let mut rows=array(&current,"service_artifacts");if rows.iter().any(|r|get(r,"path")==get(record,"path")){return Ok(());}if rows.len()>=64{rows.remove(0);}rows.push(record.clone());put(&mut current,"service_artifacts",Arr(rows));replace_chat(st,current);Ok(())}
fn attach(st:&mut Json,record:&Json)->Result<()>{let bytes=read(st,record,INPUT_LIMIT)?;let image=crate::attachments::image(&obj(vec![("name",get(record,"name").clone()),("mime",get(record,"mime").clone()),("bytes",Num(bytes.len().to_string())),("data",s(&network::b64(&bytes)))]))?;if chat(st)==Null{new_chat(st)?;}let mut current=chat(st);let mut images=array(&current,"draft_attachments");images.push(image);crate::attachments::images(&obj(vec![("role",s("user")),("attachments",Arr(images.clone()))]))?;put(&mut current,"draft_attachments",Arr(images));let mut candidate=st.clone();replace_chat(&mut candidate,current);if super::super::mcp_schema::state_bytes(&candidate)>6*1024*1024-8192{return Err("History has insufficient space for this attachment; existing draft retained".into());}*st=candidate;put(st,"dialog",Null);put(st,"page",s("chat"));put(st,"notice",s("Image attached to the draft for review; no message was sent"));Ok(())}
fn file_snapshot(st:&Json,name:&str)->Result<Json>{
    if name.is_empty()||name=="."||name==".."||name.contains(['/', '\\']){return Err("Choose an owned media filename".into());}let path=directory(st)?.join(name);let m=fs::symlink_metadata(path).map_err(|e|e.to_string())?;if !m.is_file()||m.file_type().is_symlink(){return Err("Media entry is not an owned regular file".into());}
    Ok(obj(vec![("name",s(name)),("path",s(&format!("service-files/{name}"))),("bytes",Num(m.len().to_string())),("root",get(st,"approval_root_identity").clone()),("stamp",s(&format!("{:?}",stamp(&m))))]))
}
fn files(st:&Json)->Result<Vec<Json>>{let mut rows=vec![];for item in fs::read_dir(directory(st)?).map_err(|e|e.to_string())?.take(513){let item=item.map_err(|e|e.to_string())?;let name=item.file_name().to_string_lossy().into_owned();if let Ok(row)=file_snapshot(st,&name){rows.push(row);}}rows.sort_by_key(|r|text(r,"name"));Ok(rows)}
fn selected_file(st:&Json)->Result<Json>{let selected=get(st,"media_selected");let current=file_snapshot(st,&text(selected,"name"))?;if &current!=selected{return Err("Selected file changed or belongs to another workspace; select it again".into());}Ok(current)}
pub(super) fn handles(a:&str)->bool{a.starts_with("media_")}
pub(super) fn event(st:&mut Json,a:&str,input:&Json)->Result<Json>{
    if callback(a){let expected=get(st,"media_expected").clone();put(st,"media_expected",Null);if expected==Null{return Ok(Null);}if a=="media_input_result"{let adopted=(||->Result<Json>{if !text(input,"media_receipt").is_empty(){media_input::take(st,&text(input,"media_ticket"),&text(input,"media_receipt"))}else{let bytes=network::unbase64(&text(input,"data"))?;if bytes.len()>INPUT_LIMIT{return Err("Inline input exceeds 2 MiB; use chunked file import".into());}store(st,&bytes,&text(input,"name"),"data","input")}})();media_input::cancel(&text(input,"media_ticket"));let record=adopted?;put(st,"media_input",record);put(st,"notice",s("Input saved locally. It is sent only with a reviewed file:true request."));if boolean(&expected,"recording")&&voice::authorized(st){put(st,"dialog",Null);return voice::recorded(st);}}else{media_input::cancel(&text(input,"media_ticket"));put(st,"notice",s(if a=="media_input_cancel"{"Media selection cancelled"}else{"Media input unavailable; no request sent"}));}put(st,"dialog",s("media_menu"));return Ok(Null);}
    if boolean(st,"busy")||get(st,"pending")!=&Null{return Err("Finish or cancel the current operation first".into());}
    match a{
        "media_attach_input"|"media_attach_output"=>{let record=get(st,if a=="media_attach_input"{"media_input"}else{"service_artifact"}).clone();attach(st,&record)?;},
        _ if a.starts_with("media_artifact:")=>{let index=a.strip_prefix("media_artifact:").unwrap().parse::<usize>().map_err(|_|"Invalid artifact selection")?;let record=array(&chat(st),"service_artifacts").get(index).cloned().ok_or("Artifact is absent from this conversation")?;put(st,"service_artifact",record);put(st,"dialog",s("media_menu"));},
        "media_files"=>{files(st)?;put(st,"dialog",s("media_files"));},
        _ if a.starts_with("media_file:")=>{let selected=file_snapshot(st,a.strip_prefix("media_file:").unwrap())?;put(st,"media_selected",selected);put(st,"dialog",s("media_file"));},
        "media_file_export"=>{let selected=selected_file(st)?;return project_store::artifact_start(st,&text(&selected,"path"));},
        "media_file_remove"=>{selected_file(st)?;put(st,"dialog",s("media_remove_review"));},
        "media_file_remove_confirm"=>{if text(st,"dialog")!="media_remove_review"{return Err("Review this specific file before permanently removing it".into());}let selected=selected_file(st)?;fs::remove_file(directory(st)?.join(text(&selected,"name"))).map_err(|e|e.to_string())?;for key in ["media_input","service_artifact"]{if get(get(st,key),"path")==get(&selected,"path"){put(st,key,Null);}}put(st,"media_selected",Null);put(st,"dialog",s("media_files"));put(st,"notice",s("Selected local file permanently removed. Server jobs and other files were not changed."));},
        "media_menu"=>put(st,"dialog",s("media_menu")),
        "media_pick"|"media_record"=>{interrupt(st);let ticket=format!("media:{}",now());let b=binding(st);let mut held=captures().lock().map_err(|_|"Media ownership unavailable")?;held.retain(|_,(_,t,_)|t.elapsed()<Duration::from_secs(120));if held.len()>=16{return Err("Too many media selections".into());}if a=="media_pick"{media_input::register(st,&ticket)?;}held.insert(ticket.clone(),(b.clone(),Instant::now(),None));put(st,"media_expected",obj(vec![("ticket",s(&ticket)),("binding",s(&b)),("recording",Bool(a=="media_record"))]));put(st,"dialog",s("media_capture"));let mut effect=obj(vec![("kind",s(if a=="media_pick"{"media_pick"}else{"media_record"})),("media_ticket",s(&ticket))]);if a=="media_record"{let(file,record,limit)=stream_target(st,"audio","capture",INPUT_LIMIT)?;let reservation=file.capture()?;held.get_mut(&ticket).unwrap().2=Some(reservation);put(&mut effect,"capture_path",s(&PathBuf::from(text(st,"approval_root")).join(text(&record,"path")).to_string_lossy()));put(&mut effect,"capture_limit",Num(limit.to_string()));}return Ok(effect);},
        "media_record_stop"=>return Ok(obj(vec![("kind",s("media_record_stop")),("media_ticket",get(get(st,"media_expected"),"ticket").clone())])),
        "media_clear_input"=>{put(st,"media_input",Null);put(st,"notice",s("Input deselected. The local file is retained."));},
        "media_preview"=>{let record=get(st,"service_artifact");let path=owned_path(st,record,256*1024*1024)?;return Ok(obj(vec![("kind",s("media_preview")),("path",s(&path.to_string_lossy())),("root",get(st,"approval_root").clone()),("root_identity",get(st,"approval_root_identity").clone()),("chat",get(st,"active").clone()),("record",record.clone()),("mime",get(record,"mime").clone()),("media_kind",get(record,"kind").clone()),("resource_config",get(st,"resources").clone())]));},
        "media_export"=>{let record=get(st,"service_artifact").clone();return export(st,&record);},
        "media_playback_stop"=>return Ok(obj(vec![("kind",s("media_playback_stop"))])),
        _=>return Err("Unsupported media action".into())}
    Ok(Null)
}
pub(super) fn dialog(st:&Json)->Option<Json>{
    match text(st,"dialog").as_str(){
        "media_files"=>Some(menu("Local media files",match files(st){Ok(rows)=>rows.iter().map(|r|button(&format!("{} · {} bytes",text(r,"name"),number(r,"bytes")),&format!("media_file:{}",text(r,"name")))).collect(),Err(e)=>vec![button(&e,"media_menu")]})),
        "media_file"=>Some(modal("Local media file",vec![field_ui("media_file","Selected local file",&get(st,"media_selected").string(),"readonly")],vec![button("Export this file","media_file_export"),button("Review permanent removal","media_file_remove"),button("Back to files","media_files")])),
        "media_remove_review"=>Some(modal("Permanently remove this file?",vec![field_ui("media_remove","Only this selected file",&format!("{}\nThis permanently removes these local bytes, including any preview or upload that depends on them. Export the file first if you need it. A server job is not cancelled and its record is retained.",get(st,"media_selected").string()),"readonly")],vec![button("Permanently remove this file","media_file_remove_confirm"),button("Keep file","media_files")])),
        "media_menu"=>{let artifact=get(st,"service_artifact");let mut buttons=vec![button("Manage local media files","media_files"),button("Choose input file","media_pick"),button("Record audio","voice_record"),button("Microphone settings","voice_settings"),button("Start open microphone conversation","voice_open_review"),button("Resume microphone","voice_listen"),button("Deselect input","media_clear_input"),button("Transcribe selected audio","voice_transcribe"),button("Speak last reply","voice_speak"),button(if voice::authorized(st){"Review voice conversation"}else{"Enable voice conversation"},"voice_review"),button("Stop voice","voice_stop"),button("Stop playback","media_playback_stop"),button("Service request","service_request")];if text(get(st,"media_input"),"kind")=="image"{buttons.push(button("Attach selected image to draft","media_attach_input"));}if artifact!=&Null{if text(artifact,"kind")=="image"{buttons.push(button("Attach returned image to draft","media_attach_output"));}if ["image","audio","video","document","scene"].contains(&text(artifact,"kind").as_str()){buttons.push(button("Preview returned media","media_preview"));}buttons.push(button("Export returned file","media_export"));}Some(modal("Voice and media",vec![field_ui("input","Selected local input",&get(st,"media_input").string(),"readonly"),field_ui("artifact","Last returned file",&artifact.string(),"readonly")],buttons))},
        "media_capture"=>Some(modal("Media capture",vec![field_ui("capture","Explicit capture","Select a file, or finish the foreground recording. Selected files stream locally up to 64 MiB within available storage. Audio capture is limited to 60 seconds and 2 MiB. Finish returns the capture to the workflow you reviewed. Voice consent determines whether recognition starts.","readonly")],vec![button("Finish recording","media_record_stop"),button("Cancel","cancel")])),
        _=>None}
}

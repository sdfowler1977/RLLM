//! Typed request segments keep selected bytes out of state and JNI envelopes.
use super::*;
use std::io::{Seek,SeekFrom};
use std::sync::{Mutex,OnceLock};
use std::time::{Duration,Instant};
enum Part{Bytes(Vec<u8>),File(bool)}
pub(super) struct Plan{parts:Vec<Part>,file:media::InputFile,size:usize,total:usize,index:usize,offset:usize,created:Instant}
struct Upload{purpose:String,plan:Plan}
static UPLOADS:OnceLock<Mutex<BTreeMap<String,Upload>>>=OnceLock::new();
fn uploads()->&'static Mutex<BTreeMap<String,Upload>>{UPLOADS.get_or_init(||Mutex::new(BTreeMap::new()))}
fn bytes(parts:&mut Vec<Part>,value:impl AsRef<[u8]>)->Result<()>{
 let value=value.as_ref();let inline=parts.iter().map(|p|match p{Part::Bytes(b)=>b.len(),_=>0}).sum::<usize>();if parts.len()>=4096||value.len()>(4*1024*1024usize).saturating_sub(inline){return Err("File request exceeds its segment or inline bound".into());}parts.push(Part::Bytes(value.to_vec()));Ok(())
}
// Values selected by $input are opaque data: their contents are never interpreted
// as file mappings. Only the user's configured request template has that authority.
fn render(v:&Json,input:&Json,record:&Json,parts:&mut Vec<Part>,depth:usize,raw:bool)->Result<()>{
 if depth>24{return Err("File request mapping is too deeply nested".into());}
 if let Obj(fields)=v{if fields.len()==1{
  if fields.contains_key("$input"){let value=services::pointer(input,field(v,"$input")?)?;return bytes(parts,if raw{match value{Str(s)=>s.clone(),_=>value.string()}}else{value.string()});}
  if fields.contains_key("$file"){return match field(v,"$file")?{
   "name"|"mime"=>{let value=get(record,field(v,"$file")?);bytes(parts,if raw{text(record,field(v,"$file")?)}else{value.string()})},
   "base64"|"data_url"=>{if !raw{bytes(parts,b"\"")?;}if field(v,"$file")?=="data_url"{bytes(parts,format!("data:{};base64,",text(record,"mime")))?;}parts.push(Part::File(true));if !raw{bytes(parts,b"\"")?;}Ok(())},
   _=>Err("Unknown file mapping".into())};
  }
 }}
 match v{
  Obj(fields)=>{bytes(parts,b"{")?;for(i,(key,value))in fields.iter().enumerate(){if i>0{bytes(parts,b",")?;}bytes(parts,s(key).string())?;bytes(parts,b":")?;render(value,input,record,parts,depth+1,false)?;}bytes(parts,b"}")},
  Arr(values)=>{bytes(parts,b"[")?;for(i,value)in values.iter().enumerate(){if i>0{bytes(parts,b",")?;}render(value,input,record,parts,depth+1,false)?;}bytes(parts,b"]")},
  Str(value)if raw=>bytes(parts,value),_=>bytes(parts,v.string())
 }
}
fn field_name(value:&str)->Result<()>{if value.is_empty()||value.len()>128||!value.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b)){return Err("Invalid multipart field name".into());}Ok(())}
pub(super) fn prepare(st:&Json,route:&Json,input:&Json)->Result<(Plan,String)>{
 let mut file=media::input_file(st)?;let record=get(st,"media_input");let size=number(record,"bytes");let mut parts=vec![];let template=if get(route,"request")==&Null{obj(vec![("$input",s(""))])}else{get(route,"request").clone()};
 let mime=match text(route,"request_format").as_str(){
  "binary"=>{parts.push(Part::File(false));text(record,"mime")},
  "multipart"=>{
   let boundary=format!("rllm-{}",now());let pattern=boundary.as_bytes();let mut carry=vec![];let mut chunk=[0u8;8192];file.file.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;let mut count=0;
   loop{let n=file.file.read(&mut chunk).map_err(|e|e.to_string())?;if n==0{break;}count+=n;if count>size{return Err("Selected file grew during multipart preparation".into());}carry.extend_from_slice(&chunk[..n]);if carry.windows(pattern.len()).any(|v|v==pattern){return Err("Multipart boundary collides with file contents; review again".into());}if carry.len()>=pattern.len(){carry.drain(..carry.len()-pattern.len()+1);}}
   if count!=size{return Err("Selected file shrank during multipart preparation".into());}file.current()?;
   let fields=if let Obj(values)=&template{if values.len()==1&&values.contains_key("$input"){services::pointer(input,field(&template,"$input")?)?}else{&template}}else{&template};
   let Obj(fields)=fields else{return Err("Multipart request must map to an object".into());};let opaque=matches!(&template,Obj(values)if values.len()==1&&values.contains_key("$input"));
   for(key,value)in fields{field_name(key)?;bytes(&mut parts,format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n"))?;if opaque{bytes(&mut parts,match value{Str(s)=>s.clone(),_=>value.string()})?;}else{render(value,input,record,&mut parts,0,true)?;}bytes(&mut parts,b"\r\n")?;}
   let name=text(route,"file_field");let name=if name.is_empty(){"file"}else{&name};field_name(name)?;bytes(&mut parts,format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",media::filename(&text(record,"name")),text(record,"mime")))?;parts.push(Part::File(false));bytes(&mut parts,format!("\r\n--{boundary}--\r\n"))?;format!("multipart/form-data; boundary={boundary}")
  },
  ""|"json"=>{render(&template,input,record,&mut parts,0,false)?;"application/json".into()},
  _=>return Err("Choose json, binary or multipart request format".into())
 };
 let mut inline=0usize;let mut total=0usize;for part in &parts{let n=match part{Part::Bytes(b)=>{inline=inline.saturating_add(b.len());b.len()},Part::File(base64)=>if *base64{size.div_ceil(3)*4}else{size}};total=total.checked_add(n).ok_or("Upload length overflow")?;}
 if inline>4*1024*1024||total>384*1024*1024||parts.len()>4096{return Err("Mapped request exceeds its inline or streamed bound".into());}file.current()?;Ok((Plan{parts,file,size,total,index:0,offset:0,created:Instant::now()},mime))
}
pub(super) fn length(plan:&Plan)->usize{plan.total}
pub(super) fn register(purpose:&str,plan:Plan)->Result<String>{let mut held=uploads().lock().map_err(|_|"Upload registry unavailable")?;held.retain(|_,v|v.plan.created.elapsed()<Duration::from_secs(600)&&services::stream_allowed(&v.purpose));if held.len()>=4{return Err("Four file uploads are active".into());}let ticket=format!("media-upload:{}",now());held.insert(ticket.clone(),Upload{purpose:purpose.into(),plan});Ok(ticket)}
pub(super) fn cancel(purpose:&str){if let Ok(mut held)=uploads().lock(){held.retain(|_,v|v.purpose!=purpose);}}
pub(super) fn operation(v:&Json)->Result<Json>{
 let ticket=field(v,"ticket")?;let purpose=field(v,"purpose")?;let action=field(v,"op")?;let mut held=uploads().lock().map_err(|_|"Upload registry unavailable")?;
 if action=="media_upload_cancel"{if held.get(ticket).is_some_and(|v|v.purpose==purpose){held.remove(ticket);}return Ok(Null);}
 if !services::stream_allowed(purpose){held.retain(|_,v|v.purpose!=purpose);return Err("File upload is no longer owned".into());}
 let transfer=held.get_mut(ticket).filter(|v|v.purpose==purpose).ok_or("Upload ticket or purpose differs")?;let plan=&mut transfer.plan;plan.file.current()?;
 if plan.created.elapsed()>=Duration::from_secs(600){return Err("File upload expired".into());}
 let mut out=Vec::with_capacity(8192);
 while out.len()<8192&&plan.index<plan.parts.len(){match &plan.parts[plan.index]{
  Part::Bytes(b)=>{let n=(8192-out.len()).min(b.len()-plan.offset);out.extend_from_slice(&b[plan.offset..plan.offset+n]);plan.offset+=n;if plan.offset==b.len(){plan.index+=1;plan.offset=0;}},
  Part::File(base64)=>{if plan.offset==plan.size{plan.index+=1;plan.offset=0;continue;}let room=8192-out.len();let n=if *base64{(room/4*3).min(plan.size-plan.offset)}else{room.min(plan.size-plan.offset)};if n==0{break;}plan.file.file.seek(SeekFrom::Start(plan.offset as u64)).map_err(|e|e.to_string())?;let mut chunk=vec![0;n];plan.file.file.read_exact(&mut chunk).map_err(|e|e.to_string())?;if *base64{out.extend_from_slice(network::b64(&chunk).as_bytes());}else{out.extend_from_slice(&chunk);}plan.offset+=n;}
 }}
 plan.file.current()?;let done=plan.index==plan.parts.len();if done{held.remove(ticket);}Ok(obj(vec![("data",s(&network::b64(&out))),("done",Bool(done))]))
}

//! Picker bytes are staged in bounded chunks under the existing capture owner.
use super::*;
use std::sync::{Mutex,OnceLock};
use std::time::{Duration,Instant};
struct Input{state:Json,binding:String,created:Instant,stream:String,name:String,file:Option<media::StreamFile>,record:Json,digest:incremental_digest::Digest,bytes:usize,limit:usize,receipt:String}
static INPUTS:OnceLock<Mutex<BTreeMap<String,Input>>>=OnceLock::new();
fn inputs()->&'static Mutex<BTreeMap<String,Input>>{INPUTS.get_or_init(||Mutex::new(BTreeMap::new()))}
fn binding(st:&Json)->String{harness_rules::digest(&obj(vec![("root",get(st,"approval_root").clone()),("identity",get(st,"approval_root_identity").clone()),("chat",get(st,"active").clone())]))}
pub(super) fn register(st:&Json,ticket:&str)->Result<()>{
 let mut held=inputs().lock().map_err(|_|"Input staging unavailable")?;held.retain(|_,v|v.created.elapsed()<Duration::from_secs(120));if held.len()>=16{return Err("Too many input selections".into());}
 let state=obj(vec![("approval_root",get(st,"approval_root").clone()),("approval_root_identity",get(st,"approval_root_identity").clone())]);
 held.insert(ticket.into(),Input{state,binding:binding(st),created:Instant::now(),stream:String::new(),name:String::new(),file:None,record:Null,digest:incremental_digest::Digest::new(),bytes:0,limit:0,receipt:String::new()});Ok(())
}
pub(super) fn cancel(ticket:&str){if let Ok(mut held)=inputs().lock(){held.remove(ticket);}}
pub(super) fn operation(v:&Json)->Result<Json>{
 let owner=field(v,"media_ticket")?;let action=field(v,"op")?;
 if action=="media_input_cancel"{cancel(owner);return Ok(Null);}
 if !media::capture_allowed(owner){cancel(owner);return Err("Input selection is no longer owned".into());}
 let mut held=inputs().lock().map_err(|_|"Input staging unavailable")?;let input=held.get_mut(owner).ok_or("Input selection is absent")?;
 if input.created.elapsed()>=Duration::from_secs(120){held.remove(owner);return Err("Input selection expired".into());}
 if action=="media_input_open"{
  if !input.stream.is_empty(){return Err("Input stream was already opened".into());}
  let(file,record,limit)=media::stream_target(&input.state,"data","input",media::FILE_LIMIT)?;input.file=Some(file);input.record=record;input.limit=limit;input.name=media::filename(&text(v,"name"));input.stream=format!("media-input:{}",now());return Ok(obj(vec![("ticket",s(&input.stream)),("limit",Num(limit.to_string()))]));
 }
 if field(v,"ticket")?!=input.stream||input.stream.is_empty()||!input.receipt.is_empty(){return Err("Input stream ticket differs or is already complete".into());}
 match action{
  "media_input_feed"=>{let data=field(v,"data")?;if data.len()>12000{return Err("Input chunk exceeds 8 KiB".into());}let bytes=network::unbase64(data)?;if bytes.is_empty()||bytes.len()>8192{return Err("Input chunk requires 1 byte to 8 KiB".into());}if input.bytes.saturating_add(bytes.len())>input.limit{held.remove(owner);return Err("Input exceeds its storage bound (maximum 64 MiB); partial file retained without adoption".into());}input.file.as_mut().ok_or("Input stream is closed")?.write_all(&bytes).map_err(|e|e.to_string())?;input.digest.update(&bytes)?;input.bytes+=bytes.len();Ok(Num(input.bytes.to_string()))},
  "media_input_finish"=>{if input.bytes==0{return Err("Selected input is empty".into());}let file=input.file.take().ok_or("Input stream is closed")?;file.finish()?;drop(file);let digest=std::mem::replace(&mut input.digest,incremental_digest::Digest::new()).finish();let mut record=media::stream_complete(input.record.clone(),input.bytes,&digest)?;put(&mut record,"name",s(&input.name));put(&mut record,"validation",s("Complete selected bytes and disk digest checked; container identity is descriptive, decoder acceptance occurs at use."));input.record=record;input.receipt=format!("media-input-result:{}",now());Ok(s(&input.receipt))},
  _=>Err("Unknown input stream operation".into())
 }
}
pub(super) fn take(st:&Json,owner:&str,receipt:&str)->Result<Json>{
 let mut held=inputs().lock().map_err(|_|"Input staging unavailable")?;let input=held.get(owner).ok_or("Input result is absent")?;
 if receipt.is_empty()||input.receipt!=receipt||input.binding!=binding(st)||input.created.elapsed()>=Duration::from_secs(120){return Err("Input result changed, expired or belongs to another selection".into());}
 let input=held.remove(owner).unwrap();drop(held);media::verify(st,&input.record,media::FILE_LIMIT)?;Ok(input.record)
}

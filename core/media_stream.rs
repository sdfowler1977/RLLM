//! Bounded, owned HTTP-to-file transfer; large bodies never enter chat state.
use super::*;
use std::sync::{Mutex,OnceLock};
use std::time::{Duration,Instant};
struct Transfer{purpose:String,record:Json,file:fs::File,http:network::ResponseFrame,digest:incremental_digest::Digest,bytes:usize,limit:usize,created:Instant}
struct Receipt{purpose:String,record:Json,created:Instant}
static TRANSFERS:OnceLock<Mutex<BTreeMap<String,Transfer>>>=OnceLock::new();
static RECEIPTS:OnceLock<Mutex<BTreeMap<String,Receipt>>>=OnceLock::new();
fn transfers()->&'static Mutex<BTreeMap<String,Transfer>>{TRANSFERS.get_or_init(||Mutex::new(BTreeMap::new()))}
fn receipts()->&'static Mutex<BTreeMap<String,Receipt>>{RECEIPTS.get_or_init(||Mutex::new(BTreeMap::new()))}
pub(super) fn open(st:&Json,purpose:&str,r:&Json)->Result<(String,usize)>{
 let mut held=transfers().lock().map_err(|_|"Media transfer registry unavailable")?;held.retain(|_,v|v.created.elapsed()<Duration::from_secs(600));if held.len()>=4{return Err("Four media transfers are active".into());}
 let mb=number(r,"artifact_limit_mb");let limit=if mb==0{64}else{mb}.clamp(1,256)*1024*1024;
 let(file,record,limit)=media::stream_target(st,&text(r,"artifact_kind"),&text(r,"id"),limit)?;let ticket=format!("media-stream:{}",now());
 held.insert(ticket.clone(),Transfer{purpose:purpose.into(),record,file,http:network::ResponseFrame::default(),digest:incremental_digest::Digest::new(),bytes:0,limit,created:Instant::now()});Ok((ticket,limit))
}
pub(super) fn cancel_purpose(purpose:&str){if let Ok(mut held)=transfers().lock(){held.retain(|_,v|v.purpose!=purpose);}if let Ok(mut held)=receipts().lock(){held.retain(|_,v|v.purpose!=purpose);}}
pub(super) fn operation(v:&Json)->Result<Json>{
 let purpose=field(v,"purpose")?;let ticket=field(v,"ticket")?;
 if field(v,"op")?=="media_stream_cancel"{let mut held=transfers().lock().map_err(|_|"Media transfer registry unavailable")?;if held.get(ticket).is_some_and(|t|t.purpose==purpose){held.remove(ticket);}return Ok(Null);}
 if !services::stream_allowed(purpose){return Err("Media request is no longer owned".into());}
 let mut held=transfers().lock().map_err(|_|"Media transfer registry unavailable")?;let stream=held.get_mut(ticket).filter(|t|t.purpose==purpose).ok_or("Media transfer ticket or purpose differs")?;
 if stream.created.elapsed()>Duration::from_secs(600){return Err("Media transfer expired".into());}
 match field(v,"op")?{
 "media_stream_feed"=>{let data=field(v,"data")?;if data.len()>12000{return Err("Media chunk exceeds 8 KiB".into());}let bytes=network::unbase64(data)?;if bytes.len()>8192{return Err("Media chunk exceeds 8 KiB".into());}let body=stream.http.feed(&bytes)?;if stream.http.status!=0&&stream.http.status!=200{return Err(format!("HTTP {} during artifact retrieval; generation was not replayed",stream.http.status));}if stream.bytes.saturating_add(body.len())>stream.limit{return Err("Artifact exceeds its configured or storage bound".into());}stream.file.write_all(&body).map_err(|e|e.to_string())?;stream.digest.update(&body)?;stream.bytes+=body.len();Ok(obj(vec![("ready",Bool(stream.http.done)),("bytes",Num(stream.bytes.to_string()))]))},
 "media_stream_finish"=>{if !stream.http.done&&!boolean(v,"eof"){return Err("Artifact ended before framed completion or EOF".into());}stream.http.finish()?;if stream.http.status!=200{return Err("Artifact did not return HTTP 200".into());}let stream=held.remove(ticket).unwrap();drop(held);stream.file.sync_all().map_err(|e|e.to_string())?;drop(stream.file);let record=media::stream_complete(stream.record,stream.bytes,&stream.digest.finish())?;let token=format!("media-result:{}",now());let mut ready=receipts().lock().map_err(|_|"Media result registry unavailable")?;ready.retain(|_,v|v.created.elapsed()<Duration::from_secs(600));if ready.len()>=16{return Err("Result capacity reached; completed file is retained locally".into());}ready.insert(token.clone(),Receipt{purpose:purpose.into(),record,created:Instant::now()});Ok(s(&token))},
 _=>Err("Unknown media stream operation".into())}
}
pub(super) fn take(st:&Json,purpose:&str,token:&str)->Result<Json>{let mut held=receipts().lock().map_err(|_|"Media result registry unavailable")?;if !held.get(token).is_some_and(|v|v.purpose==purpose&&v.created.elapsed()<Duration::from_secs(600)){return Err("Media result belongs to another request, expired or is absent".into());}let record=held.remove(token).unwrap().record;drop(held);media::verify(st,&record,256*1024*1024)?;Ok(record)}

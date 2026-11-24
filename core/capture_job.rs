//! One process-held approved rendering job; no disk or evidence-file authority.
use super::*;
use std::{sync::{Mutex,OnceLock,Arc,atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering}},time::{Instant,Duration},collections::BTreeMap};
use crate::host_fs::MetadataExt;
#[path="resource_url.rs"] mod resource_url;
#[path="resource_plan.rs"] mod resource_plan;
#[path="resource_session.rs"] mod resource_session;
struct Data{html:String,args:Json,authority:Json,source:String,root:PathBuf,dev:u64,ino:u64,budget:usize,ceiling:usize,resources:Option<resource_session::Session>}
struct Entry{data:Option<Data>,cancelled:Arc<AtomicBool>,created:Instant,web_ticket:String,budget:Arc<AtomicUsize>}
static JOBS:OnceLock<Mutex<BTreeMap<String,Entry>>>=OnceLock::new();
static SERIAL:AtomicU64=AtomicU64::new(1);
struct Receipt{token:String,ticket:String,root:PathBuf,dev:u64,ino:u64,args:Json,source:String,report:String,artifact:Json,created:Instant}
static RECEIPT:OnceLock<Mutex<Option<Receipt>>>=OnceLock::new();
fn receipts()->&'static Mutex<Option<Receipt>>{RECEIPT.get_or_init(||Mutex::new(None))}
/// Current worker receipt, never facts parsed from fetched or callback text.
/// Called only after exact process-held web ownership and before cancellation.
pub(super) fn take_receipt(context:&Json,root:&str,report:&str)->Json{
 let mut held=match receipts().lock(){Ok(h)=>h,Err(_)=>return Null};let Some(r)=held.as_ref()else{return Null};
 let valid=(||->Result<bool>{let root=Path::new(root).canonicalize().map_err(|e|e.to_string())?;let m=fs::metadata(&root).map_err(|e|e.to_string())?;Ok(r.created.elapsed()<Duration::from_secs(120)&&r.ticket==text(context,"ticket")&&r.root==root&&r.dev==m.dev()&&r.ino==m.ino()&&r.args==*get(context,"capture")&&r.source==text(context,"source")&&r.report==report)})();
 if valid==Ok(true){held.take().unwrap().artifact}else{Null}
}
fn jobs()->&'static Mutex<BTreeMap<String,Entry>>{JOBS.get_or_init(||Mutex::new(BTreeMap::new()))}
pub(super) fn begin(html:String,args:Json,source:String,root:&str,budget:usize)->Result<String>{
 let root=Path::new(root).canonicalize().map_err(|e|e.to_string())?;let metadata=fs::metadata(&root).map_err(|e|e.to_string())?;if !metadata.is_dir(){return Err("Capture workspace is not a directory".into());}
 if html.len()>262144||args.string().len()>4096{return Err("Remote capture input exceeds its job bound".into());}capture_file::validate(&args)?;let minimum=capture_file::minimum_bytes(&html,&args)?;if minimum>budget{return Err("Current resource budget cannot admit remote capture staging".into());}
 let resources=if text(&args,"resources")=="same_origin"{Some(resource_session::Session::new(&html,&source,budget.saturating_sub(minimum))?)}else{None};
 let mut jobs=jobs().lock().map_err(|_|"Capture ownership unavailable")?;
 jobs.retain(|_,e|{if e.created.elapsed()>=Duration::from_secs(120){e.cancelled.store(true,Ordering::SeqCst);e.data.is_none()}else{true}});
 if !jobs.is_empty(){return Err("A remote capture job is still pending or running; no second job was queued".into());}
 let n=SERIAL.try_update(Ordering::SeqCst,Ordering::SeqCst,|v|v.checked_add(1)).map_err(|_|"Capture ticket space exhausted")?;let ticket=format!("capture-{n}");jobs.insert(ticket.clone(),Entry{data:Some(Data{html,args,authority:Null,source,root,dev:metadata.dev(),ino:metadata.ino(),budget,ceiling:budget,resources}),cancelled:Arc::new(AtomicBool::new(false)),created:Instant::now(),web_ticket:String::new(),budget:Arc::new(AtomicUsize::new(budget))});Ok(ticket)
}
/// Begin one approved capture. Only its opaque token enters application state;
/// HTML and resource bytes remain inside the existing single process-held job.
pub(super) fn prepare(st:&mut Json,context:&Json,html:String,root:&str,budget:usize)->Result<Json>{
 web_operation::admission(st,context)?;
 let ticket=begin(html,get(context,"capture").clone(),text(st,"web_source"),root,budget)?;
 {let mut jobs=jobs().lock().map_err(|_|"Capture ownership unavailable")?;let data=jobs.get_mut(&ticket).and_then(|entry|entry.data.as_mut()).ok_or("Capture owner unavailable before authority binding")?;data.authority=obj(vec![("tool",get(context,"tool").clone()),("args",get(context,"args").clone())]);}
 match next(st,context,&ticket,root){Ok(effect)=>Ok(effect),Err(error)=>{cancel(&ticket);Err(error)}}
}
fn check_root(data:&Data,root:&str)->Result<()>{let root=Path::new(root).canonicalize().map_err(|e|e.to_string())?;let metadata=fs::metadata(&root).map_err(|e|e.to_string())?;if data.root!=root||data.dev!=metadata.dev()||data.ino!=metadata.ino(){return Err("Capture workspace identity changed".into());}Ok(())}
fn current_budget(data:&mut Data,room:usize)->Result<()>{let room=room.min(data.ceiling);let minimum=capture_file::minimum_bytes(&data.html,&data.args)?;if minimum>room{return Err("Current resource budget cannot retain capture staging or admit rendering".into());}if let Some(resources)=data.resources.as_mut(){resources.current_room(room-minimum)?;}data.budget=room;Ok(())}
/// Current operating admission, separate from parsing or transport errors.
pub(super) fn resource_preflight(st:&Json,context:&Json,root:&str)->Result<()>{
 let ticket=text(context,"capture_token");let mut jobs=jobs().lock().map_err(|_|"Capture ownership unavailable")?;let entry=jobs.get_mut(&ticket).ok_or("Capture resource owner is unavailable")?;
 if text(context,"phase")!="resources"||entry.web_ticket!=text(context,"ticket")||entry.web_ticket.is_empty(){return Err("Capture resource callback does not own the current request".into());}
 if entry.cancelled.load(Ordering::SeqCst)||entry.created.elapsed()>=Duration::from_secs(120){return Err("Capture resource owner expired or cancelled".into());}
 let data=entry.data.as_mut().ok_or("Capture rendering has already started")?;check_root(data,root)?;if get(context,"capture")!=&data.args{return Err("Capture settings changed during resource retrieval".into());}
 current_budget(data,web_document::response_room(st))?;entry.budget.store(data.budget,Ordering::SeqCst);Ok(())
}
/// Reconcile pending/running jobs against actual current native resource inputs.
/// A reduction below a running worker's admitted allowance cancels that worker;
/// no unrelated workflow is disabled and no old capture is replayed.
pub(super) fn reconcile(st:&mut Json)->Vec<String>{let token=text(get(st,"web_expected"),"capture_token");if token.is_empty(){return vec![];}let room=web_document::response_room(st);let mut refused=Vec::new();if let Ok(mut jobs)=jobs().lock(){if let Some(entry)=jobs.get_mut(&token){let valid=if let Some(data)=entry.data.as_mut(){let result=current_budget(data,room);if result.is_ok(){entry.budget.store(data.budget,Ordering::SeqCst);}result.is_ok()}else{room>=entry.budget.load(Ordering::SeqCst)};if !valid{entry.cancelled.store(true,Ordering::SeqCst);refused.push(token.clone());}}}for ticket in &refused{cancel(ticket);}let context=get(st,"web_expected").clone();if refused.contains(&text(&context,"capture_token")){web_operation::record(st,&context,"Interrupted","Current resource allowance no longer admits this capture; no completed image result adopted or automatic replay.");put(st,"web_expected",Null);put(st,"busy",Bool(false));put(st,"active_tool",Null);put(st,"notice_error",Bool(true));put(st,"notice",s("Capture interrupted because the current resource allowance decreased; other workflows remain available."));}refused}

fn next(st:&mut Json,context:&Json,ticket:&str,root:&str)->Result<Json>{
 let mut effect={let mut jobs=jobs().lock().map_err(|_|"Capture ownership unavailable")?;let entry=jobs.get_mut(ticket).ok_or("Capture resource owner is unavailable")?;if entry.cancelled.load(Ordering::SeqCst)||entry.created.elapsed()>=Duration::from_secs(120){return Err("Capture resource owner expired or cancelled".into());}let data=entry.data.as_mut().ok_or("Capture rendering has already started")?;check_root(data,root)?;if get(context,"capture")!=&data.args{return Err("Capture resource settings no longer match the approved job".into());}current_budget(data,web_document::response_room(st))?;entry.budget.store(data.budget,Ordering::SeqCst);
  match data.resources.as_mut().map(|r|r.next()).transpose()?.flatten(){Some(effect)=>effect,None=>obj(vec![("kind",s("capture_document")),("purpose",s("web_tool"))])}
 };
 let phase=if text(&effect,"kind")=="network"{"resources"}else{"capture"};let mut owner=context.clone();put(&mut owner,"phase",s(phase));put(&mut owner,"capture_token",s(ticket));let proposal=obj(vec![("tool",s("web_read")),("web_owner",owner)]);let mut candidate=st.clone();web_operation::bind(&mut candidate,&mut effect,&proposal)?;put(&mut effect,"capture_token",s(ticket));
 let state_bytes=super::super::mcp_schema::state_bytes(&candidate);let policy=super::super::resources::policy(get(&candidate,"device"),get(&candidate,"resources"),state_bytes);if state_bytes>6*1024*1024-8192||phase=="resources"&&number(&policy,"response_bytes")<65536{return Err("Current history or response budget cannot admit capture resource retrieval".into());}
 let current_ticket=text(get(&candidate,"web_expected"),"ticket");{let mut jobs=jobs().lock().map_err(|_|"Capture ownership unavailable")?;let entry=jobs.get_mut(ticket).ok_or("Capture owner cancelled before dispatch")?;if entry.cancelled.load(Ordering::SeqCst){return Err("Capture owner cancelled before dispatch".into());}let data=entry.data.as_mut().ok_or("Capture rendering has already started")?;current_budget(data,web_document::response_room(&candidate))?;entry.budget.store(data.budget,Ordering::SeqCst);entry.web_ticket=current_ticket;}
 put(&mut candidate,"busy",Bool(true));put(&mut candidate,"active_tool",s("web_read"));put(&mut candidate,"notice",s(if phase=="resources"{"Fetching approved same-origin CSS/PNG for the captured page."}else{"Rendering approved HTML and adopted resource bytes into a new workspace PNG."}));*st=candidate;Ok(effect)
}
/// Called only after the application's exact web owner/ticket check. The
/// process-held ticket is checked again before accepting or consuming bytes.
pub(super) fn resource_result(st:&mut Json,context:&Json,raw:&[u8],root:&str)->Result<Json>{
 let ticket=text(context,"capture_token");let mut owned=false;let result=(||{
  {let mut jobs=jobs().lock().map_err(|_|"Capture ownership unavailable")?;let entry=jobs.get_mut(&ticket).ok_or("Capture resource owner is cancelled, expired or unavailable")?;if text(context,"phase")!="resources"||entry.web_ticket!=text(context,"ticket")||entry.web_ticket.is_empty(){return Err("Capture resource callback does not own the current request".into());}owned=true;if entry.cancelled.load(Ordering::SeqCst)||entry.created.elapsed()>=Duration::from_secs(120){return Err("Capture resource owner expired or cancelled".into());}let data=entry.data.as_mut().ok_or("Capture render ticket was already consumed")?;check_root(data,root)?;if get(context,"capture")!=&data.args{return Err("Capture settings changed during resource retrieval".into());}current_budget(data,web_document::response_room(st))?;entry.budget.store(data.budget,Ordering::SeqCst);entry.web_ticket.clear();data.resources.as_mut().ok_or("This capture has no approved resource session")?.response(raw)?;}
  next(st,context,&ticket,root)
 })();if result.is_err()&&owned{cancel(&ticket);}result
}
pub(super) fn cancel(ticket:&str){if ticket.is_empty(){return;}if let Ok(mut held)=receipts().lock(){if held.as_ref().is_some_and(|r|r.token==ticket){*held=None;}}if let Ok(mut jobs)=jobs().lock(){if let Some(e)=jobs.get(ticket){e.cancelled.store(true,Ordering::SeqCst);if e.data.is_some(){jobs.remove(ticket);}}}}
struct Held(String);impl Drop for Held{fn drop(&mut self){if let Ok(mut jobs)=jobs().lock(){jobs.remove(&self.0);}}}
pub(super) fn run(v:&Json)->Result<Json>{
 let remaining=number(v,"remaining_ms");if remaining==0||remaining>120000{return Err("Remote capture requires the original remaining deadline".into());}let start=Instant::now();let ticket=field(v,"ticket")?;let root=Path::new(field(v,"root")?).canonicalize().map_err(|e|e.to_string())?;let metadata=fs::metadata(&root).map_err(|e|e.to_string())?;
 let(data,flag,created,web_ticket)={let mut jobs=jobs().lock().map_err(|_|"Capture ownership unavailable")?;let e=jobs.get_mut(ticket).ok_or("Remote capture owner is cancelled, expired or unavailable")?;if e.cancelled.load(Ordering::SeqCst)||e.created.elapsed()>=Duration::from_secs(120){return Err("Remote capture owner expired or cancelled".into());}let pending=e.data.as_ref().ok_or("Remote capture ticket was already consumed")?;if pending.root!=root||pending.dev!=metadata.dev()||pending.ino!=metadata.ino(){return Err("Remote capture workspace identity changed".into());}if pending.resources.as_ref().map(|r|!r.ready()).unwrap_or(false){return Err("Capture resource retrieval has not completed; render was not consumed".into());}let mut data=e.data.take().unwrap();data.budget=e.budget.load(Ordering::SeqCst);(data,e.cancelled.clone(),e.created,e.web_ticket.clone())};let _held=Held(ticket.into());
 let cancelled=||flag.load(Ordering::SeqCst)||start.elapsed()>=Duration::from_millis(remaining as u64)||created.elapsed()>=Duration::from_secs(120);
 if cancelled(){return Err("Remote capture cancelled before workspace access".into());}
 if data.authority!=Null{harness_rules::worker_policy(&data.authority)?;}
 harness_rules::worker_policy(&obj(vec![("tool",s("capture")),("args",data.args.clone())]))?;
 let receipt_root=data.root.clone();let receipt_args=data.args.clone();let receipt_source=data.source.clone();let receipt_dev=data.dev;let receipt_ino=data.ino;
 let (result,artifact)=if let Some(resources)=data.resources{
  if !resources.ready(){return Err("Capture resource retrieval has not completed".into());}
  let summary=resources.summary();capture_file::remote_resources_observed(&root,data.html.as_bytes(),&data.args,&data.source,data.budget,resources.reserved(),&summary,|node,_,_,limit|resources.bytes(node,limit),cancelled)?
 }else{capture_file::remote_observed(&root,data.html.as_bytes(),&data.args,&data.source,data.budget,cancelled)?};if !cancelled(){if let Ok(mut held)=receipts().lock(){*held=Some(Receipt{token:ticket.into(),ticket:web_ticket,root:receipt_root,dev:receipt_dev,ino:receipt_ino,args:receipt_args,source:receipt_source,report:result.clone(),artifact,created:Instant::now()});}}Ok(s(&result))
}

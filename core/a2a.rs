//! Versioned local Agent2Agent JSON-RPC binding for owned workspace readers.
//! Request data cannot choose a root, grant permissions, execute code or fetch URLs.
use super::*;
use std::{path::PathBuf,os::unix::fs::MetadataExt,sync::{Arc,Mutex,OnceLock,Condvar,atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering}},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};

const TASKS:usize=8;
const REALMS:usize=4;
const OUTPUT:usize=24*1024;
const MESSAGE:usize=8*1024;
const REQUEST:usize=64*1024;
static SERIAL:AtomicU64=AtomicU64::new(1);
static ACTIVE:AtomicUsize=AtomicUsize::new(0);
static WAITING:AtomicBool=AtomicBool::new(false);
// The Android binding has two core HTTP workers. At most one may wait on
// task completion, leaving a worker for cancellation/status and other routes.
struct BlockingWait;
impl BlockingWait{fn reserve()->Rpc<Self>{WAITING.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).map_err(|_|RpcError::new(-32000,"A blocking workspace response is pending; use returnImmediately or retry later"))?;Ok(Self)}}
impl Drop for BlockingWait{fn drop(&mut self){WAITING.store(false,Ordering::SeqCst);}}
fn get<'a>(v:&'a Json,k:&str)->&'a Json{if let Obj(m)=v{m.get(k).unwrap_or(&Null)}else{&Null}}
fn text(v:&Json,k:&str)->String{get(v,k).text().unwrap_or("").into()}
fn put(v:&mut Json,k:&str,x:Json){if let Obj(m)=v{m.insert(k.into(),x);}}
fn clip(s0:&str,n:usize)->String{s0[..s0.floor_char_boundary(s0.len().min(n))].into()}
fn id(prefix:&str)->Result<String>{
 let n=SERIAL.try_update(Ordering::Relaxed,Ordering::Relaxed,|n|n.checked_add(1)).map_err(|_|"Task identity capacity reached")?;
 let time=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|"System clock is before the supported epoch")?.as_nanos();
 Ok(format!("{prefix}-{time}-{n}"))
}
fn object(v:&Json)->Result<()>{if matches!(v,Obj(_)){Ok(())}else{Err("Expected an object".into())}}
fn string(v:&Json,k:&str,max:usize)->Result<String>{let s0=get(v,k).text()?;if s0.len()>max||s0.chars().any(char::is_control){return Err(format!("Invalid {k}"));}Ok(s0.into())}
fn optional(v:&Json,k:&str,max:usize)->Result<String>{if get(v,k)==&Null{Ok(String::new())}else{string(v,k,max)}}
fn boolean(v:&Json,k:&str,default:bool)->Result<bool>{match get(v,k){Null=>Ok(default),Bool(b)=>Ok(*b),_=>Err(format!("{k} must be boolean"))}}
fn integer(v:&Json,k:&str,default:usize,max:usize)->Result<usize>{match get(v,k){Null=>Ok(default),Num(n)=>n.parse::<usize>().ok().filter(|n|*n<=max).ok_or(format!("Invalid {k}")),_=>Err(format!("{k} must be an integer"))}}
fn exact(v:&Json,keys:&[&str])->Result<()>{let Obj(m)=v else{return Err("Expected command object".into())};if m.keys().any(|k|!keys.contains(&k.as_str())){return Err("Unknown command field".into());}Ok(())}
fn digest(v:&Json)->String{application::workspace_digest(v.string().as_bytes())}

#[derive(Clone,Copy,PartialEq,Eq)]enum Version{Legacy,Current}
impl Version{
 fn parse(v:&str)->std::result::Result<Self,RpcError>{
  if v.is_empty(){return Ok(Self::Legacy);}
  let parts:Vec<_>=v.split('.').collect();
  if !(2..=3).contains(&parts.len())||parts.iter().any(|p|p.is_empty()||p.len()>6||!p.bytes().all(|b|b.is_ascii_digit())){return Err(RpcError::new(-32009,"Unsupported A2A version; available versions are 1.0 and 0.3"));}
  match (parts[0],parts[1]){("0","3")=>Ok(Self::Legacy),("1","0")=>Ok(Self::Current),_=>Err(RpcError::new(-32009,"Unsupported A2A version; available versions are 1.0 and 0.3"))}
 }
 fn name(self)->&'static str{if self==Self::Current{"1.0"}else{"0.3"}}
}
#[derive(Debug,PartialEq,Eq)]
struct RpcError{code:i32,message:String}
impl RpcError{fn new(code:i32,message:&str)->Self{Self{code,message:clip(message,2048)}}}
type Rpc<T>=std::result::Result<T,RpcError>;
fn params<T>(r:Result<T>)->Rpc<T>{r.map_err(|e|RpcError::new(-32602,&e))}
fn error(id:Json,e:RpcError)->Json{obj(vec![("jsonrpc",s("2.0")),("id",id),("error",obj(vec![("code",Num(e.code.to_string())),("message",s(&e.message))]))])}
fn success(id:Json,value:Json)->Json{obj(vec![("jsonrpc",s("2.0")),("id",id),("result",value)])}

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)]struct Stamp{i:i64,n:u32}
fn leap(y:i64)->bool{y%4==0&&(y%100!=0||y%400==0)}
fn days(y:i64,m:i64,d:i64)->i64{
 let y=y-if m<=2{1}else{0};let era=y.div_euclid(400);let yy=y-era*400;let mm=m+if m>2{-3}else{9};
 era*146097+yy*365+yy/4-yy/100+(153*mm+2)/5+d-1-719468
}
impl Stamp{
 fn now()->Option<Self>{let d=SystemTime::now().duration_since(UNIX_EPOCH).ok()?;let i=i64::try_from(d.as_secs()).ok()?;if i>253402300799{return None;}Some(Self{i,n:d.subsec_nanos()})}
 fn parse(v:&str)->Result<Self>{
  if !(20..=30).contains(&v.len())||!v.ends_with('Z')||!v.is_ascii(){return Err("Timestamp must use UTC RFC3339".into());}
  let b=v.as_bytes();if b[4]!=b'-'||b[7]!=b'-'||b[10]!=b'T'||b[13]!=b':'||b[16]!=b':'{return Err("Invalid timestamp".into());}
  let read=|a:usize,z:usize|if b[a..z].iter().all(u8::is_ascii_digit){v[a..z].parse::<i64>().map_err(|_|"Invalid timestamp".to_string())}else{Err("Invalid timestamp".into())};
  let y=read(0,4)?;let m=read(5,7)?;let d=read(8,10)?;let h=read(11,13)?;let min=read(14,16)?;let sec=read(17,19)?;
  let month=[31,if leap(y){29}else{28},31,30,31,30,31,31,30,31,30,31];
  if !(1..=9999).contains(&y)||!(1..=12).contains(&m)||d<1||d>month[(m-1)as usize]||!(0..=23).contains(&h)||!(0..=59).contains(&min)||!(0..=59).contains(&sec){return Err("Invalid timestamp date or time".into());}
  let fraction=&v[19..v.len()-1];let n=if fraction.is_empty(){0}else{
   let f=fraction.strip_prefix('.').ok_or("Invalid timestamp fraction")?;if f.is_empty()||f.len()>9||!f.bytes().all(|b|b.is_ascii_digit()){return Err("Invalid timestamp fraction".into());}
   f.parse::<u32>().map_err(|_|"Invalid timestamp fraction")?*10u32.pow(9-f.len()as u32)
  };
  Ok(Self{i:days(y,m,d)*86400+h*3600+min*60+sec,n})
 }
 fn string(self)->String{
  let z=self.i.div_euclid(86400)+719468;let era=z.div_euclid(146097);let doe=z-era*146097;let yoe=(doe-doe/1460+doe/36524-doe/146096)/365;let y=yoe+era*400;let doy=doe-(365*yoe+yoe/4-yoe/100);let mp=(5*doy+2)/153;let d=doy-(153*mp+2)/5+1;let m=mp+if mp<10{3}else{-9};let y=y+if m<=2{1}else{0};let t=self.i.rem_euclid(86400);
  format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:09}Z",t/3600,t/60%60,t%60,self.n)
 }
}
#[derive(Clone,Copy,PartialEq,Eq)]enum State{Submitted,Working,Completed,Failed,Canceled}
impl State{
 fn terminal(self)->bool{matches!(self,Self::Completed|Self::Failed|Self::Canceled)}
 fn name(self,v:Version)->&'static str{match(v,self){
  (Version::Legacy,Self::Submitted)=>"submitted",(Version::Legacy,Self::Working)=>"working",(Version::Legacy,Self::Completed)=>"completed",(Version::Legacy,Self::Failed)=>"failed",(Version::Legacy,Self::Canceled)=>"canceled",
  (_,Self::Submitted)=>"TASK_STATE_SUBMITTED",(_,Self::Working)=>"TASK_STATE_WORKING",(_,Self::Completed)=>"TASK_STATE_COMPLETED",(_,Self::Failed)=>"TASK_STATE_FAILED",(_,Self::Canceled)=>"TASK_STATE_CANCELED"}}
}
struct Data{state:State,updated:Option<Stamp>,output:Option<Json>,diagnostic:String}
struct Task{id:String,context:String,input:Json,request:Json,command:Json,created:Instant,cancel:Arc<AtomicBool>,data:Mutex<Data>,done:Condvar}
impl Task{
 fn view(&self,version:Version,history:usize,artifacts:bool)->Rpc<Json>{
  let d=self.data.lock().map_err(|_|RpcError::new(-32603,"Task state unavailable"))?;
  let mut status=obj(vec![("state",s(d.state.name(version)))]);
  if let Some(time)=d.updated{put(&mut status,"timestamp",s(&time.string()));}
  if !d.diagnostic.is_empty(){put(&mut status,"message",message(version,&self.context,&self.id,&format!("{}-status",self.id),false,obj(vec![("text",s(&d.diagnostic))])));}
  let mut value=obj(vec![("id",s(&self.id)),("contextId",s(&self.context)),("status",status)]);
  if version==Version::Legacy{put(&mut value,"kind",s("task"));}
  if history>0{put(&mut value,"history",Arr(vec![convert_message(&self.input,version)]));}
  if artifacts{
   let values=if let Some(output)=&d.output{let mut part=obj(vec![("data",output.clone())]);if version==Version::Legacy{put(&mut part,"kind",s("data"));}else{put(&mut part,"mediaType",s("application/json"));}
    vec![obj(vec![("artifactId",s(&format!("{}-output",self.id))),("name",s("Workspace observation")),("parts",Arr(vec![part]))])]
   }else{vec![]};put(&mut value,"artifacts",Arr(values));
  }
  Ok(value)
 }
}
fn convert_message(input:&Json,v:Version)->Json{
 let mut out=input.clone();let mut parts=match get(input,"parts"){Arr(a)=>a.clone(),_=>vec![]};
 if let Obj(m)=&mut out{m.remove("kind");}
 for part in &mut parts{if let Obj(m)=part{m.remove("kind");}if v==Version::Legacy{put(part,"kind",s(if get(part,"data")!=&Null{"data"}else{"text"}));}}
 put(&mut out,"parts",Arr(parts));put(&mut out,"role",s(if v==Version::Legacy{"user"}else{"ROLE_USER"}));if v==Version::Legacy{put(&mut out,"kind",s("message"));}out
}
fn message(v:Version,context:&str,task:&str,id:&str,user:bool,mut part:Json)->Json{
 if v==Version::Legacy{let kind=if get(&part,"text")!=&Null{"text"}else{"data"};put(&mut part,"kind",s(kind));}
 let mut m=obj(vec![("messageId",s(id)),("contextId",s(context)),("taskId",s(task)),("role",s(match(v,user){(Version::Current,true)=>"ROLE_USER",(Version::Current,false)=>"ROLE_AGENT",(Version::Legacy,true)=>"user",_=>"agent"})),("parts",Arr(vec![part]))]);
 if v==Version::Legacy{put(&mut m,"kind",s("message"));}m
}
struct Cursor{id:String,filter:String,values:Vec<Json>,created:Instant,offset:usize}
struct Store{tasks:BTreeMap<String,Arc<Task>>,cursors:Vec<Cursor>}
struct Realm{root:PathBuf,identity:(u64,u64),budget:usize,workers:usize,closed:AtomicBool,store:Mutex<Store>}
struct RealmEntry{realm:Arc<Realm>,touched:Instant}
static STORES:OnceLock<Mutex<BTreeMap<String,RealmEntry>>>=OnceLock::new();
fn stores()->&'static Mutex<BTreeMap<String,RealmEntry>>{STORES.get_or_init(||Mutex::new(BTreeMap::new()))}
fn check_root(realm:&Realm)->Result<()>{
 if realm.closed.load(Ordering::SeqCst){return Err("Workspace interface stopped".into());}
 let m=std::fs::metadata(&realm.root).map_err(|e|format!("Workspace unavailable: {e}"))?;
 if !m.is_dir()||(m.dev(),m.ino())!=realm.identity{return Err("Workspace identity changed; restart the interface with the intended directory".into());}Ok(())
}
/// Only trusted controllers call bind; incoming messages cannot set this scope.
pub(crate) fn bind(config:&mut Json,root:&str)->Result<()>{
 if text(config,"listen")!="127.0.0.1"{return Err("Workspace A2A HTTP access is local-only. Use 127.0.0.1; no LAN workspace exposure was enabled".into());}
 let token=string(config,"token",8192)?;if token.len()<32||!token.bytes().all(|b|(0x21..=0x7e).contains(&b)){return Err("Use a printable gateway token of at least32 characters".into());}
 let root=std::fs::canonicalize(root).map_err(|e|format!("Workspace unavailable: {e}"))?;let m=std::fs::metadata(&root).map_err(|e|e.to_string())?;
 if !m.is_dir()||root.to_str().is_none(){return Err("Select an existing UTF-8 workspace directory".into());}
 put(config,"a2a_root",s(root.to_str().unwrap()));put(config,"a2a_identity",Arr(vec![Num(m.dev().to_string()),Num(m.ino().to_string())]));put(config,"a2a_instance",s(&id("interface")?));Ok(())
}
fn scope(config:&Json)->Result<String>{
 if get(config,"a2a")!=&Bool(true)||text(config,"listen")!="127.0.0.1"{return Err("Local workspace A2A interface is not enabled".into());}
 let _=string(config,"a2a_instance",128)?;
 Ok(digest(&obj(vec![("root",get(config,"a2a_root").clone()),("identity",get(config,"a2a_identity").clone()),("instance",get(config,"a2a_instance").clone()),("token",get(config,"token").clone())])))
}
fn realm(config:&Json)->Result<Arc<Realm>>{
 let key=scope(config)?;let root=PathBuf::from(string(config,"a2a_root",4096)?);let identity=get(config,"a2a_identity").array()?;
 if identity.len()!=2{return Err("Missing selected workspace identity".into());}
 let read=|n:&Json|match n{Num(n)=>n.parse::<u64>().map_err(|_|"Invalid workspace identity"),_=>Err("Invalid workspace identity")};let identity=(read(&identity[0])?,read(&identity[1])?);
 let mut all=stores().lock().map_err(|_|"Workspace task service unavailable")?;
 all.retain(|_,entry|{if entry.realm.closed.load(Ordering::SeqCst){return entry.touched.elapsed()<Duration::from_secs(300);}if entry.touched.elapsed()<Duration::from_secs(1800){return true;}entry.realm.store.lock().map(|s|s.tasks.values().any(|t|t.data.lock().map(|d|!d.state.terminal()).unwrap_or(true))).unwrap_or(true)});
 if let Some(e)=all.get_mut(&key){e.touched=Instant::now();check_root(&e.realm)?;return Ok(e.realm.clone());}
 if all.values().filter(|e|!e.realm.closed.load(Ordering::SeqCst)).count()>=REALMS||all.len()>=64{return Err("Local workspace service capacity reached; stop an old interface first".into());}
 let budget=integer(config,"limit",1024*1024,4*1024*1024)?.min(1024*1024);let workers=integer(config,"workers",resources::workers(&Null),64)?.clamp(1,4);
 let realm=Arc::new(Realm{root,identity,budget,workers,closed:AtomicBool::new(false),store:Mutex::new(Store{tasks:BTreeMap::new(),cursors:vec![]})});check_root(&realm)?;
 all.insert(key,RealmEntry{realm:realm.clone(),touched:Instant::now()});Ok(realm)
}
pub(crate) fn close(config:&Json)->Result<()>{
 if get(config,"a2a")!=&Bool(true){return Ok(());}
 let key=scope(config)?;let removed={let mut all=stores().lock().map_err(|_|"Workspace task service unavailable")?;all.get_mut(&key).map(|entry|{entry.touched=Instant::now();entry.realm.clone()})};
 if let Some(realm)=removed{realm.closed.store(true,Ordering::SeqCst);let mut store=realm.store.lock().map_err(|_|"Task state unavailable")?;
  for task in store.tasks.values(){task.cancel.store(true,Ordering::SeqCst);if let Ok(mut d)=task.data.lock(){if !d.state.terminal(){d.state=State::Canceled;d.updated=Stamp::now();d.diagnostic="Interface stopped; no result was adopted".into();}}task.done.notify_all();}
  store.tasks.clear();store.cursors.clear();
 }Ok(())
}
fn task_access(realm:&Realm,task:&Task)->Result<()>{
 check_root(realm)?;application::workspace_task_check(&task.command)
}
fn find_task(realm:&Realm,id:&str)->Rpc<Arc<Task>>{
 let task=realm.store.lock().map_err(|_|RpcError::new(-32603,"Task state unavailable"))?.tasks.get(id).cloned().ok_or_else(||RpcError::new(-32001,"Task not found or unavailable"))?;
 task_access(realm,&task).map_err(|_|RpcError::new(-32001,"Task not found or unavailable"))?;Ok(task)
}
struct Worker;
impl Worker{fn reserve(requested:usize)->Rpc<Self>{let limit=std::thread::available_parallelism().map(|n|n.get()).unwrap_or(1).min(requested).clamp(1,4);ACTIVE.try_update(Ordering::SeqCst,Ordering::SeqCst,|v|if v<limit{Some(v+1)}else{None}).map_err(|_|RpcError::new(-32000,"Current workspace workers are busy; retry after a task finishes"))?;Ok(Self)}}
impl Drop for Worker{fn drop(&mut self){ACTIVE.fetch_sub(1,Ordering::SeqCst);}}
fn finish(realm:Arc<Realm>,task:Arc<Task>,worker:Worker){
 if let Ok(mut d)=task.data.lock(){if task.cancel.load(Ordering::SeqCst){drop(worker);task.done.notify_all();return;}d.state=State::Working;d.updated=Stamp::now();}else{return;}
 let output=application::workspace_task_tool(&task.command,realm.identity,task.cancel.clone(),realm.budget);
 if let Ok(mut d)=task.data.lock(){
  if d.state.terminal(){drop(worker);task.done.notify_all();return;}
  if task.cancel.load(Ordering::SeqCst)||realm.closed.load(Ordering::SeqCst){d.state=State::Canceled;d.output=None;}
  else{match output{
   Ok(value)=>{let raw=match value{Str(v)=>v,other=>other.string()};let complete=raw.len()<=OUTPUT;
    d.output=Some(obj(vec![("tool",get(&task.command,"tool").clone()),("output",s(&clip(&raw,OUTPUT))),("complete",Bool(complete)),("returnedBytes",Num(raw.len().to_string())),("returnedSha256",s(&application::workspace_digest(raw.as_bytes()))),("meaning",s("Actual bounded workspace-engine output; a matching prefix is not full-file identity. Complete-file hashes require content:complete_digest."))]));d.state=State::Completed;}
   Err(reason)=>{d.state=State::Failed;d.diagnostic=clip(&reason.replace(&realm.root.to_string_lossy().to_string(),"workspace"),1024);}
  }}
  d.updated=Stamp::now();
  // Publish completion and release execution capacity in the same critical
  // section, before a waiter can observe a terminal task and submit new work.
  drop(worker);
 }else{drop(worker);}
 task.done.notify_all();
}
fn history(params0:&Json)->Rpc<usize>{params(integer(params0,"historyLength",1,100))}
fn send(realm:&Arc<Realm>,p:&Json,v:Version)->Rpc<Json>{
 let m=get(p,"message");params(object(m))?;if m.string().len()>MESSAGE{return Err(RpcError::new(-32602,"Message exceeds8KiB"));}
 let mid=params(string(m,"messageId",128))?;if mid.is_empty(){return Err(RpcError::new(-32602,"messageId is required"));}
 if text(m,"role")!=if v==Version::Legacy{"user"}else{"ROLE_USER"}{return Err(RpcError::new(-32602,"Expected a user message"));}
 let task_id=params(optional(m,"taskId",128))?;let context=params(optional(m,"contextId",128))?;
 if !task_id.is_empty(){let task=find_task(realm,&task_id)?;if !context.is_empty()&&context!=task.context{return Err(RpcError::new(-32602,"Task and context do not match"));}
  return Err(RpcError::new(-32004,"Workspace observations use one command per task. Start a new task; an existing task was not restarted"));
 }
 let parts=params(get(m,"parts").array())?;if parts.len()!=1{return Err(RpcError::new(-32005,"Send one structured data part containing tool and args"));}
 let part=&parts[0];if v==Version::Legacy&&text(part,"kind")!="data"{return Err(RpcError::new(-32005,"Expected a data part"));}
 if ["text","raw","url","file"].iter().any(|k|get(part,k)!=&Null){return Err(RpcError::new(-32005,"Only an inline structured command is supported; files and URLs are not fetched"));}
 let data=get(part,"data");params(exact(data,&["tool","args"]))?;let tool=params(string(data,"tool",32))?;
 if !["inspect","compare"].contains(&tool.as_str()){return Err(RpcError::new(-32004,"This interface supports read-only inspect and compare"));}
 let args=get(data,"args");params(exact(args,if tool=="compare"{&["path","other","content"]}else{&["path","content"]}))?;params(string(args,"path",1024))?;
 if tool=="compare"{params(string(args,"other",1024))?;}
 if get(args,"content")!=&Null&&get(args,"content")!=&s("complete_digest"){return Err(RpcError::new(-32602,"Unsupported content mode"));}
 let configuration=get(p,"configuration");if configuration!=&Null{params(object(configuration))?;}
 if get(configuration,if v==Version::Legacy{"pushNotificationConfig"}else{"taskPushNotificationConfig"})!=&Null{return Err(RpcError::new(-32003,"Push notifications are not supported"));}
 if let Arr(modes)=get(configuration,"acceptedOutputModes"){if !modes.is_empty()&&!modes.iter().any(|m|m==&s("application/json")||m==&s("*/*")){return Err(RpcError::new(-32005,"The requested output modes do not include application/json"));}}
 else if get(configuration,"acceptedOutputModes")!=&Null{return Err(RpcError::new(-32602,"acceptedOutputModes must be an array"));}
 let blocking=if v==Version::Current{!params(boolean(configuration,"returnImmediately",false))?}else{params(boolean(configuration,"blocking",true))?};
 let history=history(configuration)?;
 let command=obj(vec![("op",s("tool")),("tool",s(&tool)),("workers",Num(realm.workers.to_string())),("root",s(realm.root.to_str().ok_or_else(||RpcError::new(-32603,"Workspace path unavailable"))?)),("args",args.clone())]);
 params(application::workspace_task_check(&command))?;
 let _waiting=if blocking{Some(BlockingWait::reserve()?)}else{None};
 let task={
  let mut store=realm.store.lock().map_err(|_|RpcError::new(-32603,"Task state unavailable"))?;
  // Repeated message IDs return the original task; changed reuse is refused.
  if let Some(task)=store.tasks.values().find(|t|text(&t.input,"messageId")==mid){
   if task.request!=convert_message(m,Version::Current){return Err(RpcError::new(-32602,"messageId was already used for a different message"));}
   let task=task.clone();drop(store);task_access(realm,&task).map_err(|_|RpcError::new(-32001,"Task not found or unavailable"))?;return await_result(realm,&task,v,history,blocking);
  }
  if store.tasks.len()>=TASKS{let oldest=store.tasks.iter().filter(|(_,t)|t.data.lock().map(|d|d.state.terminal()).unwrap_or(false)).min_by_key(|(_,t)|t.created).map(|(k,_)|k.clone());if let Some(k)=oldest{store.tasks.remove(&k);}else{return Err(RpcError::new(-32000,"Retained active task capacity reached"));}}
  let worker=Worker::reserve(realm.workers)?;let tid=id("task").map_err(|e|RpcError::new(-32603,&e))?;let context=if context.is_empty(){id("context").map_err(|e|RpcError::new(-32603,&e))?}else{context};
  let mut input=convert_message(m,Version::Current);put(&mut input,"contextId",s(&context));put(&mut input,"taskId",s(&tid));
  let task=Arc::new(Task{id:tid.clone(),context,input,request:convert_message(m,Version::Current),command,created:Instant::now(),cancel:Arc::new(AtomicBool::new(false)),data:Mutex::new(Data{state:State::Submitted,updated:Stamp::now(),output:None,diagnostic:String::new()}),done:Condvar::new()});
  store.tasks.insert(tid,task.clone());
  let r=realm.clone();let t=task.clone();
  if std::thread::Builder::new().name("workspace-task".into()).spawn(move||finish(r,t,worker)).is_err(){if let Ok(mut d)=task.data.lock(){d.state=State::Failed;d.updated=Stamp::now();d.diagnostic="The workspace worker could not start".into();}}
  task
 };
 await_result(realm,&task,v,history,blocking)
}
fn await_result(realm:&Realm,task:&Task,v:Version,history:usize,blocking:bool)->Rpc<Json>{
 if blocking{
  let mut d=task.data.lock().map_err(|_|RpcError::new(-32603,"Task state unavailable"))?;let deadline=Instant::now()+Duration::from_secs(35);
  while !d.state.terminal(){let remaining=deadline.saturating_duration_since(Instant::now());if remaining.is_zero(){task.cancel.store(true,Ordering::SeqCst);d.state=State::Failed;d.updated=Stamp::now();d.diagnostic="Workspace task exceeded its response deadline; no result was adopted".into();break;}
   let waited=task.done.wait_timeout(d,remaining).map_err(|_|RpcError::new(-32603,"Task state unavailable"))?;d=waited.0;
  }
 }
 task_access(realm,&task).map_err(|_|RpcError::new(-32001,"Task not found or unavailable"))?;
 send_result(task.view(v,history,true)?,v)
}
fn send_result(task:Json,v:Version)->Rpc<Json>{Ok(if v==Version::Current{obj(vec![("task",task)])}else{task})}
fn cancel(realm:&Realm,p:&Json,v:Version)->Rpc<Json>{
 let task=find_task(realm,&params(string(p,"id",128))?)?;
 {let mut d=task.data.lock().map_err(|_|RpcError::new(-32603,"Task state unavailable"))?;
  if d.state==State::Canceled&&v==Version::Current{}else if d.state.terminal(){return Err(RpcError::new(-32002,"Task has already reached a terminal state"));}
  else{task.cancel.store(true,Ordering::SeqCst);d.state=State::Canceled;d.updated=Stamp::now();d.output=None;d.diagnostic="Cancellation accepted; no further output will be adopted".into();}
 }task.done.notify_all();task.view(v,1,true)
}
fn list(realm:&Realm,p:&Json,v:Version)->Rpc<Json>{
 let page_size=params(integer(p,"pageSize",50,100))?;if page_size==0{return Err(RpcError::new(-32602,"pageSize must be1..100"));}
 let history=history(p)?;let artifacts=params(boolean(p,"includeArtifacts",false))?;let context=params(optional(p,"contextId",128))?;let status=params(optional(p,"status",64))?;
 let after=params(optional(p,"statusTimestampAfter",32))?;let after=if after.is_empty(){None}else{Some(params(Stamp::parse(&after))?)};
 if !status.is_empty()&&!["TASK_STATE_UNSPECIFIED","TASK_STATE_SUBMITTED","TASK_STATE_WORKING","TASK_STATE_COMPLETED","TASK_STATE_FAILED","TASK_STATE_CANCELED","TASK_STATE_INPUT_REQUIRED","TASK_STATE_REJECTED","TASK_STATE_AUTH_REQUIRED"].contains(&status.as_str()){return Err(RpcError::new(-32602,"Unknown task state"));}
 let token=params(optional(p,"pageToken",256))?;
 let mut filter=p.clone();if let Obj(m)=&mut filter{m.remove("pageToken");}let filter=digest(&filter);
 let mut store=realm.store.lock().map_err(|_|RpcError::new(-32603,"Task state unavailable"))?;store.cursors.retain(|c|c.created.elapsed()<Duration::from_secs(60));
 let (values,start,cursor_id)=if token.is_empty(){
  let mut rows=vec![];
  for task in store.tasks.values(){
   if task_access(realm,task).is_err()||(!context.is_empty()&&task.context!=context){continue;}
   let d=task.data.lock().map_err(|_|RpcError::new(-32603,"Task state unavailable"))?;
   if (!status.is_empty()&&status!="TASK_STATE_UNSPECIFIED"&&d.state.name(v)!=status)||after.is_some_and(|a|d.updated.is_none_or(|b|b<a)){continue;}
   let updated=d.updated;drop(d);rows.push((updated,task.id.clone(),task.view(v,history,artifacts)?));
  }
  rows.sort_by(|a,b|b.0.cmp(&a.0).then_with(||b.1.cmp(&a.1)));(rows.into_iter().map(|r|r.2).collect::<Vec<_>>(),0,id("page").map_err(|e|RpcError::new(-32603,&e))?)
 }else{
  let (id,offset)=token.rsplit_once(':').ok_or_else(||RpcError::new(-32602,"Invalid page token"))?;let offset=offset.parse::<usize>().map_err(|_|RpcError::new(-32602,"Invalid page token"))?;
  let cursor=store.cursors.iter().find(|c|c.id==id&&c.filter==filter).ok_or_else(||RpcError::new(-32602,"Page token expired or belongs to different filters"))?;
  if offset!=cursor.offset{return Err(RpcError::new(-32602,"Page token does not identify the next retained page"));}
  // A page snapshot is never a grant to read an observation after policy revocation.
  for row in &cursor.values{let tid=text(row,"id");let task=store.tasks.get(&tid).ok_or_else(||RpcError::new(-32001,"Task page is no longer retained"))?;if task_access(realm,task).is_err(){return Err(RpcError::new(-32001,"Task page is no longer available"));}}
  (cursor.values.clone(),offset,id.into())
 };
 if start>values.len(){return Err(RpcError::new(-32602,"Invalid page offset"));}
 let end=start.saturating_add(page_size).min(values.len());let next=if end<values.len(){format!("{cursor_id}:{end}")}else{String::new()};
 let page=values[start..end].to_vec();let total=values.len();
 if token.is_empty()&&end<total{if store.cursors.len()>=2{store.cursors.remove(0);}store.cursors.push(Cursor{id:cursor_id,filter,values,created:Instant::now(),offset:end});}
 else if !token.is_empty(){if let Some(c)=store.cursors.iter_mut().find(|c|c.id==cursor_id){c.offset=end;}if next.is_empty(){store.cursors.retain(|c|c.id!=cursor_id);}}
 Ok(obj(vec![("tasks",Arr(page)),("nextPageToken",s(&next)),("pageSize",Num(page_size.to_string())),("totalSize",Num(total.to_string()))]))
}
fn card(config:&Json,v:Version)->Json{
 let port=match get(config,"port"){Str(s0)=>s0.clone(),Num(n)=>n.clone(),_=>"8080".into()};let url=format!("http://127.0.0.1:{port}/a2a");
 let mut c=obj(vec![("name",s("RLLM")),("description",s("Authenticated local read-only inspect/compare over the selected workspace. Send one data part containing {tool,args}; no model, code execution, URL fetch, writes or phone-wide access. HTTP is loopback-only.")),("version",s("1.0")),("capabilities",obj(vec![("streaming",Bool(false)),("pushNotifications",Bool(false))])),("defaultInputModes",Arr(vec![s("application/json")])),("defaultOutputModes",Arr(vec![s("application/json")])),("skills",Arr(["inspect","compare"].iter().map(|name|obj(vec![("id",s(name)),("name",s(if *name=="inspect"{"Workspace inspection"}else{"Workspace comparison"})),("description",s("Actual bounded shared-engine observations. Paths are relative to the selected workspace; content:complete_digest requests complete regular-file hashes.")),("tags",Arr(vec![s("workspace"),s("read-only")]))])).collect()))]);
 if v==Version::Legacy{put(&mut c,"protocolVersion",s("0.3.0"));put(&mut c,"url",s(&url));put(&mut c,"preferredTransport",s("JSONRPC"));put(&mut c,"securitySchemes",obj(vec![("bearer",obj(vec![("type",s("http")),("scheme",s("bearer"))]))]));put(&mut c,"security",Arr(vec![obj(vec![("bearer",Arr(vec![]))])]));
 }else{
  put(&mut c,"supportedInterfaces",Arr(vec![obj(vec![("url",s(&url)),("protocolBinding",s("JSONRPC")),("protocolVersion",s("1.0"))]),obj(vec![("url",s(&url)),("protocolBinding",s("JSONRPC")),("protocolVersion",s("0.3"))])]));
  put(&mut c,"securitySchemes",obj(vec![("bearer",obj(vec![("httpAuthSecurityScheme",obj(vec![("scheme",s("Bearer"))]))]))]));
  put(&mut c,"securityRequirements",Arr(vec![obj(vec![("schemes",obj(vec![("bearer",obj(vec![("list",Arr(vec![]))]))]))])]));
 }c
}
fn rpc(config:&Json,body:&str,v:Version)->Json{
 let request=match parse(body){Ok(v)=>v,Err(e)=>return error(Null,RpcError::new(-32700,&e))};
 let rid=get(&request,"id").clone();
 let valid=matches!(&rid,Str(s0) if !s0.is_empty()&&s0.len()<=128&&!s0.chars().any(char::is_control))||matches!(&rid,Num(n) if n.parse::<i64>().is_ok());
 if !matches!(request,Obj(_))||get(&request,"jsonrpc")!=&s("2.0")||!valid||!matches!(get(&request,"method"),Str(m) if !m.is_empty()&&m.len()<=256&&!m.chars().any(char::is_control))||get(&request,"result")!=&Null||get(&request,"error")!=&Null{return error(Null,RpcError::new(-32600,"A JSON-RPC2.0 request with an integer or nonempty string id is required"));}
 let p=get(&request,"params");let method=text(&request,"method");
 let result=(||->Rpc<Json>{
  if p!=&Null{params(object(p))?;}let tenant=params(optional(p,"tenant",128))?;if !tenant.is_empty(){return Err(RpcError::new(-32602,"This interface does not declare a tenant"));}
  let realm=realm(config).map_err(|e|RpcError::new(-32603,&e))?;
  match (v,method.as_str()){
   (Version::Current,"SendMessage")|(Version::Legacy,"message/send")=>send(&realm,p,v),
   (Version::Current,"GetTask")|(Version::Legacy,"tasks/get")=>{let task=find_task(&realm,&params(string(p,"id",128))?)?;task.view(v,history(p)?,true)},
   (Version::Current,"CancelTask")|(Version::Legacy,"tasks/cancel")=>cancel(&realm,p,v),
   (Version::Current,"ListTasks")=>list(&realm,p,v),
   (_,m)if ["SendStreamingMessage","SubscribeToTask","message/stream","tasks/resubscribe"].contains(&m)=>Err(RpcError::new(-32004,"The requested optional capability is not advertised by this interface")),
   (_,"GetExtendedAgentCard"|"agent/getAuthenticatedExtendedCard")=>Err(RpcError::new(-32007,"An extended agent card is not configured")),
   (_,m)if m.contains("PushNotification")||m.starts_with("tasks/pushNotification")=>Err(RpcError::new(-32003,"Push notifications are not supported")),
   _=>Err(RpcError::new(-32601,"Method not found for the selected A2A version")),
  }
 })();
 match result{Ok(value)=>success(rid,value),Err(e)=>error(rid,e)}
}
fn response(status:u16,value:Json,version:Version)->String{
 let body=value.string();format!("HTTP/1.1 {status} {}\r\nContent-Type: application/json; charset=utf-8\r\nA2A-Version: {}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",if status==200{"OK"}else{"Bad Request"},version.name(),body.len())
}
/// Authentication is performed by the existing gateway before reaching this router.
pub(crate) fn incoming(config:&Json,method:&str,path:&str,headers:&BTreeMap<String,String>,body:&str)->Result<Option<String>>{
 let (route,query)=path.split_once('?').unwrap_or((path,""));if !["/a2a","/.well-known/agent-card.json"].contains(&route){return Ok(None);}
 if get(config,"a2a")!=&Bool(true){return Ok(None);}
 let mut selected=headers.get("a2a-version").cloned().unwrap_or_default();
 for part in query.split('&').filter(|p|!p.is_empty()){
  let(k,v)=part.split_once('=').ok_or("Invalid A2A query")?;if !k.eq_ignore_ascii_case("a2a-version"){return Ok(Some(response(400,error(Null,RpcError::new(-32602,"Unknown A2A query parameter")),Version::Legacy)));}
  if !selected.is_empty()&&Version::parse(&selected).ok()!=Version::parse(v).ok(){return Ok(Some(response(400,error(Null,RpcError::new(-32602,"Conflicting A2A versions")),Version::Legacy)));}selected=v.into();
 }
 let version=match Version::parse(&selected){Ok(v)=>v,Err(e)=>return Ok(Some(response(200,error(parse(body).ok().map(|b|get(&b,"id").clone()).unwrap_or(Null),e),Version::Current)))};
 if method=="GET"&&route=="/.well-known/agent-card.json"{
  let value=match realm(config){Ok(_)=>card(config,version),Err(e)=>return Ok(Some(response(503,obj(vec![("error",s(&e))]),version)))};
  return Ok(Some(response(200,value,version)));
 }
 if method!="POST"||route!="/a2a"{return Ok(Some(response(405,error(Null,RpcError::new(-32600,"Use POST /a2a or GET /.well-known/agent-card.json")),version)));}
 let content=headers.get("content-type").map(|s0|s0.split(';').next().unwrap_or("").trim()).unwrap_or("");
 if !content.eq_ignore_ascii_case("application/json"){return Ok(Some(response(415,error(Null,RpcError::new(-32005,"Send application/json")),version)));}
 if body.len()>REQUEST{return Ok(Some(response(413,error(Null,RpcError::new(-32602,"A2A request exceeds64KiB")),version)));}
 Ok(Some(response(200,rpc(config,body,version),version)))
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn timestamps_round_trip_and_reject_invalid_dates(){
  for input in ["1970-01-01T00:00:00Z","2000-02-29T23:59:59.123456789Z","2026-11-01T02:00:00Z","9999-12-31T23:59:59Z"]{
   let stamp=Stamp::parse(input).unwrap();assert_eq!(Stamp::parse(&stamp.string()).unwrap().i,stamp.i);assert_eq!(Stamp::parse(&stamp.string()).unwrap().n,stamp.n);
  }
  assert_eq!(Stamp::parse("1970-01-01T00:00:00Z").unwrap().i,0);
  for input in ["1900-02-29T00:00:00Z","2026-01-01T-1:00:00Z","2026-01-01T00:-1:00Z","2026-01-01T00:00:60Z","2026-01-01T00:00:00.Z","2026-01-01T00:00:00+00:00"]{assert!(Stamp::parse(input).is_err(),"{input}");}
 }
 #[test]fn version_negotiation_ignores_patch_only(){
  assert!(Version::parse("")==Ok(Version::Legacy));assert!(Version::parse("1.0.97")==Ok(Version::Current));assert!(Version::parse("0.3.0")==Ok(Version::Legacy));
  for v in ["1","1.1","2.0","1.0.bad","1.0.0.0","1.0\n"]{assert!(Version::parse(v).is_err());}
 }
 fn task(state:State)->Task{Task{id:"task-owned".into(),context:"context-owned".into(),input:obj(vec![("role",s("ROLE_USER")),("messageId",s("input-owned")),("parts",Arr(vec![obj(vec![("data",obj(vec![("tool",s("inspect"))]))])]))]),request:Null,command:Null,created:Instant::now(),cancel:Arc::new(AtomicBool::new(false)),data:Mutex::new(Data{state,updated:Stamp::now(),output:Some(obj(vec![("output",s("Actual retained observation"))])),diagnostic:String::new()}),done:Condvar::new()}}
 #[test]fn task_representation_preserves_wire_version_and_history_limit(){
  let t=task(State::Completed);let now=t.view(Version::Current,0,false).unwrap();assert_eq!(get(&now,"history"),&Null);assert_eq!(get(&now,"artifacts"),&Null);assert_eq!(get(get(&now,"status"),"state"),&s("TASK_STATE_COMPLETED"));
  let old=t.view(Version::Legacy,1,true).unwrap();assert_eq!(text(&old,"kind"),"task");let h=&get(&old,"history").array().unwrap()[0];assert_eq!(text(h,"role"),"user");assert_eq!(text(&get(h,"parts").array().unwrap()[0],"kind"),"data");
  assert_eq!(get(&old,"artifacts").array().unwrap().len(),1);
 }
 #[test]fn stopped_realm_does_not_publish_or_restart_work(){
  let realm=Arc::new(Realm{root:PathBuf::from("/must-not-be-opened"),identity:(0,0),budget:1024*1024,workers:1,closed:AtomicBool::new(true),store:Mutex::new(Store{tasks:BTreeMap::new(),cursors:vec![]})});
  assert!(check_root(&realm).unwrap_err().contains("stopped"));let task=Arc::new(task(State::Canceled));task.cancel.store(true,Ordering::SeqCst);
  let worker=Worker::reserve(1).ok().unwrap();finish(realm,task.clone(),worker);assert!(task.data.lock().unwrap().state==State::Canceled);
 }
 #[test]fn blocking_wait_admission_preserves_a_control_worker(){
  let held=BlockingWait::reserve().ok().unwrap();assert!(BlockingWait::reserve().is_err());drop(held);
  assert!(BlockingWait::reserve().is_ok());
 }
}

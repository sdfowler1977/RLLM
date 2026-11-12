//! ACP v1 standard-stream sessions over the product's owned workspace readers.
use super::*;
use std::{io::Write,os::unix::fs::MetadataExt,path::{Path,PathBuf,Component},
    sync::{Arc,mpsc::{self,SyncSender},atomic::{AtomicBool,Ordering}},thread};

const FRAME:usize=64*1024;
const SESSIONS:usize=8;
const OUTPUT:usize=96*1024;
type Rpc<T>=std::result::Result<T,(i32,String)>;
fn value<'a>(v:&'a Json,k:&str)->&'a Json{if let Obj(m)=v{m.get(k).unwrap_or(&Null)}else{&Null}}
fn text(v:&Json,k:&str)->Result<String>{Ok(field(v,k)?.into())}
fn params<T>(v:Result<T>)->Rpc<T>{v.map_err(|e|(-32602,e))}
fn result(id:Json,v:Json)->Json{obj(vec![("jsonrpc",s("2.0")),("id",id),("result",v)])}
fn error(id:Json,code:i32,message:&str)->Json{obj(vec![("jsonrpc",s("2.0")),("id",id),("error",obj(vec![("code",Num(code.to_string())),("message",s(message))]))])}
fn update(session:&str,v:Json)->Json{obj(vec![("jsonrpc",s("2.0")),("method",s("session/update")),("params",obj(vec![("sessionId",s(session)),("update",v)]))])}
fn valid_id(v:&Json)->bool{match v{Str(s)=>s.len()<=256&&!s.chars().any(char::is_control),Num(n)=>n.parse::<i64>().is_ok(),_=>false}}
fn exact(v:&Json,keys:&[&str])->Result<()>{let Obj(m)=v else{return Err("Expected an object".into())};if m.keys().any(|k|!keys.contains(&k.as_str())){return Err("Unsupported command field".into());}Ok(())}
fn object(v:&Json)->Result<()>{if matches!(v,Obj(_)){Ok(())}else{Err("Expected an object".into())}}
fn clip(v:&str,n:usize)->&str{&v[..v.floor_char_boundary(v.len().min(n))]}
fn commands(session:&str)->Json{update(session,obj(vec![("sessionUpdate",s("available_commands_update")),("availableCommands",Arr([("inspect","Inspect actual file or directory structure"),("compare","Compare two actual files or directories")].into_iter().map(|(name,description)|obj(vec![("name",s(name)),("description",s(description)),("input",obj(vec![("hint",s(if name=="inspect"{"relative path, JSON arguments or a local file link"}else{"JSON arguments with path and other, or two local file links"}))]))])).collect()))]))}

struct Turn{id:Json,serial:u64,command:Json,cancel:Arc<AtomicBool>,handle:thread::JoinHandle<()>}
struct Session{turn:Option<Turn>,closing:Option<Json>}
enum Message{Input(Result<String>),Done{session:String,serial:u64,output:Result<Json>},Eof}
struct Server{root:PathBuf,identity:(u64,u64),initialized:bool,sessions:BTreeMap<String,Session>,serial:u64,workers:usize,tx:SyncSender<Message>}
impl Server{
 fn new(root:&str,tx:SyncSender<Message>)->Result<Self>{let root=std::fs::canonicalize(root).map_err(|e|format!("Workspace: {e}"))?;let m=std::fs::metadata(&root).map_err(|e|e.to_string())?;if !m.is_dir()||root.to_str().is_none(){return Err("Select a UTF-8 workspace directory".into());}Ok(Self{root,identity:(m.dev(),m.ino()),initialized:false,sessions:BTreeMap::new(),serial:0,workers:thread::available_parallelism().map(|n|n.get()).unwrap_or(1).clamp(1,4),tx})}
 fn next(&mut self)->Result<u64>{self.serial=self.serial.checked_add(1).ok_or("Session identity capacity reached")?;Ok(self.serial)}
 fn root_check(&self)->Result<()>{let m=std::fs::metadata(&self.root).map_err(|e|format!("Workspace: {e}"))?;if !m.is_dir()||(m.dev(),m.ino())!=self.identity{return Err("Selected workspace identity changed; restart with the intended directory".into());}Ok(())}
 fn cwd(&self,p:&Json)->Result<()>{let cwd=PathBuf::from(field(p,"cwd")?);if !cwd.is_absolute(){return Err("Session cwd must be an absolute directory".into());}let cwd=cwd.canonicalize().map_err(|e|format!("Session cwd: {e}"))?;if cwd!=self.root{return Err("Session cwd must match the startup workspace".into());}self.root_check()?;if value(p,"additionalDirectories")!=&Null&&value(p,"additionalDirectories")!=&Arr(vec![]){return Err("Additional workspace roots are not supported by this interface".into());}match value(p,"mcpServers"){Arr(a)if a.is_empty()=>Ok(()),Null=>Ok(()),_=>Err("This workspace interface does not launch or connect to supplied MCP servers".into())}}
 fn file_link(&self,uri:&str)->Result<String>{
  let encoded=uri.strip_prefix("file://").ok_or("Only local file resource links are supported")?;let encoded=encoded.strip_prefix("localhost/").map(|p|format!("/{p}")).unwrap_or_else(||encoded.into());if !encoded.starts_with('/')||encoded.contains(['?','#']){return Err("File links require an absolute local path without query or fragment".into());}
  let mut bytes=vec![];let b=encoded.as_bytes();let mut n=0;while n<b.len(){if b[n]==b'%'{if n+2>=b.len(){return Err("Invalid file URI escape".into());}let h=std::str::from_utf8(&b[n+1..n+3]).map_err(|_|"Invalid file URI escape")?;bytes.push(u8::from_str_radix(h,16).map_err(|_|"Invalid file URI escape")?);n+=3;}else{bytes.push(b[n]);n+=1;}}
  let decoded=String::from_utf8(bytes).map_err(|_|"File URI must be UTF-8")?;if decoded.chars().any(char::is_control){return Err("Invalid file URI".into());}let path=Path::new(&decoded);let relative=path.strip_prefix(&self.root).map_err(|_|"File link is outside the selected workspace")?;if relative.components().any(|c|!matches!(c,Component::Normal(_)|Component::CurDir)){return Err("File link contains parent traversal".into());}let relative=relative.to_str().ok_or("File URI must be UTF-8")?;Ok(if relative.is_empty(){".".into()}else{relative.into()})
 }
 fn command(&self,p:&Json)->Result<Json>{
  let blocks=p.get("prompt")?.array()?;if blocks.is_empty()||blocks.len()>16{return Err("Prompt requires1–16 content blocks".into());}let mut message=String::new();let mut links=vec![];
  for b in blocks{match field(b,"type")?{"text"=>{let t=field(b,"text")?;if message.len()+t.len()>8192{return Err("Prompt text exceeds8KiB".into());}if !message.is_empty(){message.push('\n');}message.push_str(t);},"resource_link"=>{if links.len()>=2{return Err("At most two local file links are accepted".into());}field(b,"name")?;links.push(self.file_link(field(b,"uri")?)?);},_=>return Err("Only text and local file resource links are supported; media and embedded-context capabilities are not advertised".into())}}
  let mut command=if message.trim_start().starts_with('{'){parse(message.trim())?}else{
   let m=message.trim();let (name,rest)=m.split_once(char::is_whitespace).unwrap_or((m,""));let name=name.strip_prefix('/').ok_or("Use /inspect or /compare, or a JSON command with tool and args")?;let rest=rest.trim();let args=if rest.starts_with('{'){parse(rest)?}else if rest.is_empty(){obj(vec![])}else if name=="inspect"{obj(vec![("path",s(rest))])}else{return Err("Compare requires JSON arguments containing path and other, or two local file links".into())};obj(vec![("tool",s(name)),("args",args)])};
  exact(&command,&["tool","args"])?;let tool=field(&command,"tool")?.to_string();if !["inspect","compare"].contains(&tool.as_str()){return Err("This interface supports inspect and compare only".into());}let mut args=command.get("args")?.clone();exact(&args,&["path","other","content"])?;
  for (n,key) in ["path","other"].into_iter().enumerate(){if key=="other"&&tool=="inspect"{if value(&args,key)!=&Null{return Err("Inspect does not accept other".into());}continue;}if value(&args,key)==&Null{if let Some(path)=links.get(n){if let Obj(m)=&mut args{m.insert(key.into(),s(path));}}}let path=field(&args,key)?;if path.is_empty()||path.len()>1024||path.chars().any(char::is_control){return Err("Invalid workspace path".into());}}
  if value(&args,"content")!=&Null&&value(&args,"content")!=&s("complete_digest"){return Err("Only complete_digest content mode is supported".into());}
  if let Obj(m)=&mut command{m.insert("args".into(),args);m.insert("root".into(),s(self.root.to_str().unwrap()));m.insert("op".into(),s("tool"));}application::workspace_task_check(&command)?;Ok(command)
 }
 fn cancel(&mut self,p:&Json,by_request:bool){for (session,data)in &mut self.sessions{if let Some(turn)=&data.turn{if (by_request&&value(p,"requestId")==&turn.id)||(!by_request&&value(p,"sessionId")==&s(session)){turn.cancel.store(true,Ordering::SeqCst);}}}}
 fn handle(&mut self,input:&str)->Vec<Json>{
  let request=match parse(input){Ok(v)=>v,Err(_)=>return vec![error(Null,-32700,"Invalid JSON")]};let Obj(fields)=&request else{return vec![error(Null,-32600,"Expected JSON-RPC object")]};let id=fields.get("id");
  if fields.get("jsonrpc")!=Some(&s("2.0"))||!matches!(fields.get("method"),Some(Str(_)))||id.is_some_and(|id|!valid_id(id))||fields.contains_key("result")||fields.contains_key("error"){return vec![error(Null,-32600,"Invalid JSON-RPC request")];}
  let method=field(&request,"method").unwrap();let p=value(&request,"params");if id.is_none(){if self.initialized&&matches!(p,Obj(_)){match method{"session/cancel"=>self.cancel(p,false),"$/cancel_request"=>self.cancel(p,true),_=>()}}return vec![];}
  let id=id.unwrap().clone();if self.sessions.values().filter_map(|s|s.turn.as_ref()).any(|t|t.id==id)||self.sessions.values().any(|s|s.closing.as_ref()==Some(&id)){return vec![error(Null,-32600,"Request ID is already active")];}
  let response:Rpc<Vec<Json>>=(||{
   params(object(p))?;
   if method=="initialize"{if self.initialized{return Err((-32600,"Connection is already initialized".into()));}let version=value(p,"protocolVersion");if !matches!(version,Num(n)if n.parse::<u64>().is_ok_and(|n|n>0)){return Err((-32602,"protocolVersion must be a positive integer".into()));}if value(p,"clientCapabilities")!=&Null{params(object(value(p,"clientCapabilities")))?;}if value(p,"clientInfo")!=&Null{for key in ["name","version"]{let v=params(text(value(p,"clientInfo"),key))?;if v.is_empty()||v.len()>256{return Err((-32602,"Invalid client information".into()));}}}
    self.initialized=true;return Ok(vec![result(id.clone(),obj(vec![("protocolVersion",Num("1".into())),("agentCapabilities",obj(vec![("loadSession",Bool(false)),("promptCapabilities",obj(vec![])),("mcpCapabilities",obj(vec![])),("sessionCapabilities",obj(vec![("list",obj(vec![])),("resume",obj(vec![])),("close",obj(vec![]))]))])),("agentInfo",obj(vec![("name",s("RLLM")),("version",s("1"))])),("authMethods",Arr(vec![]))]))]);}
   if !self.initialized{return Err((-32600,"Initialize this connection first".into()));}
   match method{
    "session/new"=>{params(self.cwd(p))?;if self.sessions.len()>=SESSIONS{return Err((-32000,"Session capacity reached; close an idle session first".into()));}let serial=params(self.next())?;let session=format!("workspace-{serial}");self.sessions.insert(session.clone(),Session{turn:None,closing:None});Ok(vec![result(id.clone(),obj(vec![("sessionId",s(&session))])),commands(&session)])},
    "session/list"=>{if value(p,"cursor")!=&Null{return Err((-32602,"This bounded session list has no continuation cursor".into()));}let list=self.sessions.iter().filter(|(_,s)|s.closing.is_none()).map(|(id,_)|obj(vec![("sessionId",s(id)),("cwd",s(self.root.to_str().unwrap())),("title",s("Workspace observations"))])).collect();Ok(vec![result(id.clone(),obj(vec![("sessions",Arr(list))]))])},
    "session/resume"=>{params(self.cwd(p))?;let session=params(text(p,"sessionId"))?;let held=self.sessions.get(&session).ok_or((-32001,"Session is not available in this process".into()))?;if held.closing.is_some(){return Err((-32001,"Session is closing".into()));}Ok(vec![result(id.clone(),obj(vec![])),commands(&session)])},
    "session/close"=>{let session=params(text(p,"sessionId"))?;let held=self.sessions.get_mut(&session).ok_or((-32001,"Unknown session".into()))?;if held.closing.is_some(){return Err((-32000,"Session is already closing".into()));}if let Some(turn)=&held.turn{turn.cancel.store(true,Ordering::SeqCst);held.closing=Some(id.clone());Ok(vec![])}else{self.sessions.remove(&session);Ok(vec![result(id.clone(),obj(vec![]))])}},
    "session/prompt"=>{
     let session=params(text(p,"sessionId"))?;let held=self.sessions.get(&session).ok_or((-32001,"Unknown session".into()))?;if held.turn.is_some()||held.closing.is_some(){return Err((-32000,"Session is busy or closing".into()));}if self.sessions.values().filter(|s|s.turn.is_some()).count()>=self.workers{return Err((-32000,"Workspace workers are occupied; retry when a turn completes".into()));}params(self.root_check())?;let command=params(self.command(p))?;let serial=params(self.next())?;let cancel=Arc::new(AtomicBool::new(false));let tx=self.tx.clone();let sid=session.clone();let job=command.clone();let flag=cancel.clone();let identity=self.identity;
     let handle=thread::Builder::new().name("workspace-observation".into()).spawn(move||{let output=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||application::workspace_task_tool(&job,identity,flag,1024*1024))).unwrap_or_else(|_|Err("Workspace worker failed; no result was adopted".into()));let _=tx.send(Message::Done{session:sid,serial,output});}).map_err(|e|(-32603,format!("Workspace worker could not start: {e}")))?;
     let name=field(&command,"tool").unwrap();let start=update(&session,obj(vec![("sessionUpdate",s("tool_call")),("toolCallId",s(&format!("observation-{serial}"))),("name",s(name)),("title",s(if name=="inspect"{"Workspace inspection"}else{"Workspace comparison"})),("kind",s("read")),("status",s("in_progress")),("rawInput",command.get("args").unwrap().clone())]));self.sessions.get_mut(&session).unwrap().turn=Some(Turn{id:id.clone(),serial,command,cancel,handle});Ok(vec![start])
    },
    _=>Err((-32601,"Method is not supported by the advertised workspace capabilities".into()))
   }
  })();match response{Ok(v)=>v,Err((code,message))=>vec![error(id,code,&message)]}
 }
 fn done(&mut self,session:&str,serial:u64,output:Result<Json>)->Vec<Json>{
  let Some(held)=self.sessions.get_mut(session)else{return vec![]};if !held.turn.as_ref().is_some_and(|t|t.serial==serial){return vec![];}let turn=held.turn.take().unwrap();let closing=held.closing.take();let _=turn.handle.join();let cancelled=turn.cancel.load(Ordering::SeqCst);let output=if cancelled{Err("Operation cancelled; no result was adopted".into())}else{self.root_check().and_then(|_|application::workspace_task_check(&turn.command)).and(output)};let failed=output.is_err();let mut frames=vec![];
  let body=match output{Ok(Str(v))=>v,Ok(v)=>v.string(),Err(e)=>e};let complete=body.len()<=OUTPUT;let visible=clip(&body,OUTPUT);let raw=obj(vec![("output",s(visible)),("complete",Bool(complete)),("returnedBytes",Num(body.len().to_string())),("returnedSha256",s(&application::workspace_digest(body.as_bytes())))]);
  let content = obj(vec![
   ("type", s("content")),
   ("content", obj(vec![
    ("type", s("text")),
    ("text", s(if cancelled { "Operation cancelled" } else { visible })),
   ])),
  ]);
  frames.push(update(session, obj(vec![
   ("sessionUpdate", s("tool_call_update")),
   ("toolCallId", s(&format!("observation-{serial}"))),
   ("status", s(if failed { "failed" } else { "completed" })),
   ("rawOutput", raw),
   ("content", Arr(vec![content])),
  ])));
  if !cancelled{let body=if complete{visible.to_string()}else{format!("{visible}\n[Output truncated at96KiB; full returned output was{}bytes.]",body.len())};let mut rest=body.as_str();while !rest.is_empty(){let part=clip(rest,12*1024);frames.push(update(session,obj(vec![("sessionUpdate",s("agent_message_chunk")),("content",obj(vec![("type",s("text")),("text",s(part))]))])));rest=&rest[part.len()..];}}
  frames.push(result(turn.id,obj(vec![("stopReason",s(if cancelled{"cancelled"}else{"end_turn"}))])));if let Some(id)=closing{self.sessions.remove(session);frames.push(result(id,obj(vec![])));}frames
 }
 fn shutdown(&mut self){for session in self.sessions.values(){if let Some(turn)=&session.turn{turn.cancel.store(true,Ordering::SeqCst);}}}
}

pub(super) fn run(root:&str)->Result<()>{
 let(tx,rx)=mpsc::sync_channel(16);let mut server=Server::new(root,tx.clone())?;let input=tx.clone();let _reader=thread::Builder::new().name("protocol-input".into()).spawn(move||{let mut reader=std::io::BufReader::new(std::io::stdin().lock());loop{match super::stdio::line_limited(&mut reader,FRAME){Ok(Some(v))=>{if input.send(Message::Input(v)).is_err(){break;}},Ok(None)=>{let _=input.send(Message::Eof);break;},Err(e)=>{let _=input.send(Message::Input(Err(format!("Protocol input: {e}"))));let _=input.send(Message::Eof);break;}}}}).map_err(|e|format!("Protocol input could not start: {e}"))?;let mut writer=std::io::BufWriter::new(std::io::stdout().lock());
 let output=(||{while let Ok(message)=rx.recv(){let frames=match message{Message::Input(Ok(input))=>server.handle(&input),Message::Input(Err(e))=>vec![error(Null,-32700,&e)],Message::Done{session,serial,output}=>server.done(&session,serial,output),Message::Eof=>break};for frame in frames{writer.write_all(frame.string().as_bytes()).and_then(|_|writer.write_all(b"\n")).and_then(|_|writer.flush()).map_err(|e|format!("Protocol output: {e}"))?;}}Ok(())})();server.shutdown();drop(rx);output
}

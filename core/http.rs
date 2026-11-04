//! Loopback-only platform binding for the shared authenticated gateway router.
use super::*;
use std::{io::{Read,Write},net::{TcpListener,TcpStream},sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}},time::{Duration,Instant},os::unix::fs::PermissionsExt};

static STOP:AtomicBool=AtomicBool::new(false);
extern "C" fn stop(_:i32){STOP.store(true,Ordering::SeqCst);}
unsafe extern "C"{fn signal(signal:i32,handler:extern "C" fn(i32))->usize;}
struct Slot(Arc<AtomicUsize>);
impl Drop for Slot{fn drop(&mut self){self.0.fetch_sub(1,Ordering::SeqCst);}}
fn response(stream:&mut TcpStream,code:u16,message:&str){
 let body=obj(vec![("error",s(message))]).string();
 let _=write!(stream,"HTTP/1.1 {code} Error\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
}
fn exchange(stream:&mut TcpStream,config:&Json)->Result<()>{
 stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e|e.to_string())?;
 stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e|e.to_string())?;
 let deadline=Instant::now()+Duration::from_secs(15);
 let mut bytes=Vec::with_capacity(1024);
 while !bytes.ends_with(b"\r\n\r\n"){
  if bytes.len()>=16384||Instant::now()>=deadline{return Err("HTTP header exceeds its size or time limit".into());}
  let mut byte=[0];stream.read_exact(&mut byte).map_err(|_|"Incomplete HTTP header")?;
  if !byte[0].is_ascii(){return Err("HTTP headers must be ASCII".into());}bytes.push(byte[0]);
 }
 let header=String::from_utf8(bytes).map_err(|_|"Invalid header")?;
 let size=gateway::size(&obj(vec![("header",s(&header))]))?;
 let Num(size)=size else{return Err("Invalid request length".into())};
 let size=size.parse::<usize>().map_err(|_|"Invalid request length")?;
 let mut bytes=vec![0;size];let mut offset=0;
 while offset<size{
  if Instant::now()>=deadline{return Err("HTTP body exceeded its time limit".into());}
  let n=stream.read(&mut bytes[offset..]).map_err(|_|"Incomplete HTTP body")?;
  if n==0{return Err("Incomplete HTTP body".into());}offset+=n;
 }
 let body=String::from_utf8(bytes).map_err(|_|"Body must be UTF-8")?;
 let routed=gateway::incoming(&obj(vec![("config",config.clone()),("header",s(&header)),("body",s(&body))]))?;
 let reply=field(&routed,"reply")?;
 stream.write_all(reply.as_bytes()).map_err(|_|"HTTP peer disconnected".into())
}
pub fn run(args:&[String])->Result<()>{
 let mut root=None;let mut token_file=None;let mut port=8080u16;
 let mut pairs=args.chunks_exact(2);
 for pair in &mut pairs{match pair[0].as_str(){
  "--root" if root.is_none()=>root=Some(pair[1].as_str()),
  "--token-file" if token_file.is_none()=>token_file=Some(pair[1].as_str()),
  "--port"=>port=pair[1].parse().map_err(|_|"Port must be0..65535")?,
  _=>return Err("Use --a2a --root DIRECTORY --token-file PRIVATE_FILE [--port PORT]".into())
 }}
 if !pairs.remainder().is_empty(){return Err("Incomplete A2A option".into());}
 let root=root.ok_or("Select --root DIRECTORY")?;let file=token_file.ok_or("Select --token-file PRIVATE_FILE")?;
 // Read from one opened descriptor, bounded before retaining bytes. Avoid a
 // stat/read pathname race and refuse symlinks, devices and publicly read secrets.
 use std::os::unix::fs::OpenOptionsExt;
 let mut opened=std::fs::OpenOptions::new().read(true).custom_flags(0x20000|0x800).open(file).map_err(|e|format!("Token file: {e}"))?;
 let metadata=opened.metadata().map_err(|e|e.to_string())?;
 if !metadata.is_file()||metadata.permissions().mode()&0o077!=0||metadata.len()>8194{return Err("Token file must be a private regular file (mode0600 or0400), at most8194bytes".into());}
 let mut token=String::new();std::io::Read::by_ref(&mut opened).take(8195).read_to_string(&mut token).map_err(|_|"Token file must be UTF-8")?;
 let token=token.trim_end_matches(['\r','\n']);
 let listener=TcpListener::bind(("127.0.0.1",port)).map_err(|e|format!("Local listener: {e}"))?;
 let port=listener.local_addr().map_err(|e|e.to_string())?.port();
 let mut config=obj(vec![("a2a",Bool(true)),("listen",s("127.0.0.1")),("port",Num(port.to_string())),("token",s(token)),("profiles",Arr(vec![]))]);
 a2a::bind(&mut config,root)?;
 listener.set_nonblocking(true).map_err(|e|e.to_string())?;
 STOP.store(false,Ordering::SeqCst);unsafe{signal(2,stop);signal(15,stop);}
 eprintln!("RLLM A2A listening http://127.0.0.1:{port}/a2a");
 let active=Arc::new(AtomicUsize::new(0));
 let result=(||->Result<()>{while !STOP.load(Ordering::SeqCst){
  let (mut stream,_)=match listener.accept(){Ok(pair)=>pair,Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>{std::thread::sleep(Duration::from_millis(20));continue},Err(e)=>return Err(format!("Local listener: {e}"))};
  if active.try_update(Ordering::SeqCst,Ordering::SeqCst,|n|if n<8{Some(n+1)}else{None}).is_err(){response(&mut stream,503,"Local connections are busy");continue;}
  let slot=Slot(active.clone());let config=config.clone();
  let _=std::thread::Builder::new().name("local-http".into()).spawn(move||{let _slot=slot;if let Err(reason)=exchange(&mut stream,&config){response(&mut stream,400,&reason);}});
 }Ok(())})();
 a2a::close(&config)?;drop(listener);
 let deadline=Instant::now()+Duration::from_secs(6);
 while active.load(Ordering::SeqCst)>0&&Instant::now()<deadline{std::thread::sleep(Duration::from_millis(20));}
 result
}

use std::ffi::c_void;
#[derive(Clone)]
struct Control { id:(u64,u64), kind:u32, field:Option<usize>, text:&'static str,x:i32,y:i32,w:i32,h:i32,parent_high:u64,parent_low:u64,order:u32,depth:u32,visible:u32,sx:i64,sy:i64,sw:i64,sh:i64,gate:u32 }
const TITLE:&str="RLLM";
const WIDTH:i32=1280;
const HEIGHT:i32=720;
const SHOWN:u32=1;
fn fields()->Vec<i64>{vec![
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
3,
]}
fn controls()->Vec<Control>{vec![
Control{id:(5787775650370682881,3018100862),kind:1,field:Some(0),text:"Chat",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100861),kind:1,field:Some(1),text:"History",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100860),kind:1,field:Some(2),text:"Rail Navigation",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100859),kind:1,field:Some(3),text:"Status Indicator",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100858),kind:1,field:Some(4),text:"Chat Panel",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100857),kind:1,field:Some(5),text:"Message Composer",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100856),kind:1,field:Some(6),text:"Metric Strip",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100855),kind:1,field:Some(7),text:"Workspace",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100854),kind:1,field:Some(8),text:"App settings",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100853),kind:1,field:Some(9),text:"Agent settings",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100852),kind:1,field:Some(10),text:"Diagnostics",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100851),kind:1,field:Some(11),text:"Rail Navigation",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100850),kind:1,field:Some(12),text:"Status Indicator",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100849),kind:1,field:Some(13),text:"Virtual List",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100848),kind:1,field:Some(14),text:"Rail Navigation",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100847),kind:1,field:Some(15),text:"Status Indicator",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100846),kind:1,field:Some(16),text:"Virtual List",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100845),kind:1,field:Some(17),text:"Rail Navigation",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100844),kind:1,field:Some(18),text:"Status Indicator",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100843),kind:1,field:Some(19),text:"Description List",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100842),kind:1,field:Some(20),text:"Rail Navigation",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100841),kind:1,field:Some(21),text:"Status Indicator",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100840),kind:1,field:Some(22),text:"Agent Roster",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100839),kind:1,field:Some(23),text:"Rail Navigation",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100838),kind:1,field:Some(24),text:"Status Indicator",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100837),kind:1,field:Some(25),text:"Metric Strip",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
Control{id:(5787775650370682881,3018100836),kind:1,field:Some(26),text:"Job Monitor",x:0,y:0,w:8,h:8,parent_high:0,parent_low:0,order:0,depth:0,visible:1,gate:0,sx:0,sy:0,sw:8,sh:8},
]}
fn caption(c:&Control,f:&[i64])->String {let label=open_text(c);match c.field {Some(i)=>format!("{}: {}",label,f[i]),None=>label}}
fn activate(c:&Control,f:&mut [i64]) { if let Some(i)=c.field { match c.kind {2=>{if let Some(v)=f[i].checked_add(1){f[i]=v;}},3=>f[i]=if f[i]==0 {1}else{0},_=>{}} } }
fn held_key()->Option<[u8;32]>{static KEY:std::sync::OnceLock<Option<[u8;32]>>=std::sync::OnceLock::new();*KEY.get_or_init(read_held_key)}
fn read_held_key()->Option<[u8;32]>{let dir=roster_dir()?;let text=read_store(&dir.join("access-held-key")).ok()?;let raw=hex_bytes(text.trim())?;if raw.len()!=32{return None;}let mut key=[0u8;32];key.copy_from_slice(&raw);Some(key)}
fn held_stream(key:&[u8;32],n:usize)->Vec<u8>{let mut out=Vec::new();let mut c=0u32;while out.len()<n{let block=hmac(key,&c.to_be_bytes());out.extend_from_slice(&block);c=c.wrapping_add(1);}out.truncate(n);out}
fn open_text(c:&Control)->String{if c.gate!=2{return c.text.to_string();}let Some(body)=c.text.strip_prefix("held:") else {return c.text.to_string();};let Some(raw)=hex_bytes(body) else {return String::new();};let Some(key)=held_key() else {return String::new();};let stream=held_stream(&key,raw.len());let mut plain=Vec::with_capacity(raw.len());let mut i=0usize;while i<raw.len(){plain.push(raw[i]^stream[i]);i+=1;}String::from_utf8(plain).unwrap_or_default()}
const SESSION_SECONDS:u64=3600;
struct Session{expires:u64,token:Option<String>}
static SESSION:std::sync::Mutex<Option<Session>>=std::sync::Mutex::new(None);
fn session_open()->bool{
 let mut held=SESSION.lock().unwrap_or_else(|e| e.into_inner());
 let live=match held.as_ref(){
  Some(s)=>now_secs()<s.expires&&match s.token.as_deref(){Some(t)=>resume_session(t).is_some(),None=>true},
  None=>false,
 };
 if !live{*held=None;}
 live
}
fn begin_session(expires:u64){
 let mut held=SESSION.lock().unwrap_or_else(|e| e.into_inner());
 *held=Some(Session{expires,token:None});
}
fn begin_stored_session(token:String, expires:u64){
 let mut held=SESSION.lock().unwrap_or_else(|e| e.into_inner());
 *held=Some(Session{expires,token:Some(token)});
}
fn sign_in_session(name:&str, secret:&str)->bool{
 if !prove(name,secret){return false;}
 let Some((token,expires))=mint_session(name) else {return false;};
 begin_stored_session(token,expires);
 true
}
fn sign_out_session()->bool{
 let taken=SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
 match taken{Some(Session{token:Some(t),..})=>end_session(&t),Some(_)=>true,None=>false}
}
fn admit(c:&Control)->bool{c.gate!=2||session_open()}
fn rotr(x:u32,n:u32)->u32{x.rotate_right(n)}
fn sha256(data:&[u8])->[u8;32]{
 const K:[u32;64]=[0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
 let mut h:[u32;8]=[0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
 let mut msg=data.to_vec();
 let bits=(data.len() as u64).wrapping_mul(8);
 msg.push(0x80);
 while msg.len()%64!=56{msg.push(0);}
 msg.extend_from_slice(&bits.to_be_bytes());
 for chunk in msg.chunks(64){
  let mut w=[0u32;64];
  for i in 0..16{w[i]=u32::from_be_bytes([chunk[i*4],chunk[i*4+1],chunk[i*4+2],chunk[i*4+3]]);}
  for i in 16..64{
   let s0=rotr(w[i-15],7)^rotr(w[i-15],18)^(w[i-15]>>3);
   let s1=rotr(w[i-2],17)^rotr(w[i-2],19)^(w[i-2]>>10);
   w[i]=w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
  }
  let (mut a,mut b,mut c,mut d,mut e,mut f,mut g,mut hh)=(h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
  for i in 0..64{
   let s1=rotr(e,6)^rotr(e,11)^rotr(e,25);
   let ch=(e&f)^((!e)&g);
   let t1=hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
   let s0=rotr(a,2)^rotr(a,13)^rotr(a,22);
   let maj=(a&b)^(a&c)^(b&c);
   let t2=s0.wrapping_add(maj);
   hh=g;g=f;f=e;e=d.wrapping_add(t1);d=c;c=b;b=a;a=t1.wrapping_add(t2);
  }
  h[0]=h[0].wrapping_add(a);h[1]=h[1].wrapping_add(b);h[2]=h[2].wrapping_add(c);h[3]=h[3].wrapping_add(d);
  h[4]=h[4].wrapping_add(e);h[5]=h[5].wrapping_add(f);h[6]=h[6].wrapping_add(g);h[7]=h[7].wrapping_add(hh);
 }
 let mut out=[0u8;32];
 for i in 0..8{out[i*4..i*4+4].copy_from_slice(&h[i].to_be_bytes());}
 out
}
fn hmac(key:&[u8],msg:&[u8])->[u8;32]{
 let mut k=[0u8;64];
 if key.len()>64{let d=sha256(key);k[..32].copy_from_slice(&d);}else{k[..key.len()].copy_from_slice(key);}
 let mut ip=[0u8;64]; let mut op=[0u8;64];
 for i in 0..64{ip[i]=k[i]^0x36;op[i]=k[i]^0x5c;}
 let mut inner=Vec::with_capacity(64+msg.len()); inner.extend_from_slice(&ip); inner.extend_from_slice(msg);
 let ih=sha256(&inner);
 let mut outer=[0u8;96]; outer[..64].copy_from_slice(&op); outer[64..].copy_from_slice(&ih);
 sha256(&outer)
}
fn stretch(secret:&[u8],salt:&[u8],rounds:u32)->[u8;32]{
 let mut block=[0u8;20];
 block[..salt.len().min(16)].copy_from_slice(&salt[..salt.len().min(16)]);
 block[19]=1;
 let mut u=hmac(secret,&block);
 let mut mixed=u;
 let mut round=1u32;
 while round<rounds{u=hmac(secret,&u); for i in 0..32{mixed[i]^=u[i];} round+=1;}
 mixed
}
fn nibble(c:u8)->Option<u8>{match c{b'0'..=b'9'=>Some(c-b'0'),b'a'..=b'f'=>Some(c-b'a'+10),b'A'..=b'F'=>Some(c-b'A'+10),_=>None}}
fn hex_bytes(text:&str)->Option<Vec<u8>>{
 let b=text.as_bytes(); if b.len()%2!=0{return None;}
 let mut o=Vec::with_capacity(b.len()/2); let mut i=0;
 while i<b.len(){o.push((nibble(b[i])?<<4)|nibble(b[i+1])?); i+=2;}
 Some(o)
}
fn same(a:&[u8],b:&[u8])->bool{ if a.len()!=b.len(){return false;} let mut d=0u8; for i in 0..a.len(){d|=a[i]^b[i];} d==0 }
#[cfg(unix)]
extern "C"{fn geteuid()->u32; fn getegid()->u32;}
#[cfg(unix)]
fn trusted_meta(meta:&std::fs::Metadata)->bool{
 use std::os::unix::fs::MetadataExt;
 let uid=unsafe{geteuid()};
 // Group write is tolerated only for the owner's own private group (uid==gid, the
 // usual umask 002 layout); any other shared group could swap the file.
 let group_ok=meta.mode()&0o020==0||(meta.gid()==unsafe{getegid()}&&meta.gid()==uid);
 meta.uid()==uid&&meta.mode()&0o002==0&&group_ok
}
#[cfg(not(unix))]
fn trusted_meta(_meta:&std::fs::Metadata)->bool{true}
fn store_dir_trusted(dir:&std::path::Path)->bool{
 match std::fs::symlink_metadata(dir){Ok(meta)=>meta.file_type().is_dir()&&trusted_meta(&meta),Err(_)=>false}
}
fn read_store(path:&std::path::Path)->std::io::Result<String>{
 use std::io::Read;
 let refuse=||std::io::Error::new(std::io::ErrorKind::PermissionDenied,"untrusted store file");
 let link=std::fs::symlink_metadata(path)?;
 if !link.file_type().is_file()||!trusted_meta(&link){return Err(refuse());}
 let mut file=std::fs::File::open(path)?;
 let meta=file.metadata()?;
 if !meta.file_type().is_file()||!trusted_meta(&meta){return Err(refuse());}
 #[cfg(unix)]
 {
  use std::os::unix::fs::MetadataExt;
  if meta.dev()!=link.dev()||meta.ino()!=link.ino(){return Err(refuse());}
 }
 if meta.len()>(1<<20){return Err(refuse());}
 let mut text=String::new();
 file.read_to_string(&mut text)?;
 Ok(text)
}
fn store_holds_roster(dir:&std::path::Path)->bool{
 if !store_dir_trusted(dir){return false;}
 match read_store(&dir.join("login-credentials")){
  Ok(text)=>!text.trim().is_empty(),
  Err(_)=>false,
 }
}
fn roster_dir()->Option<std::path::PathBuf>{
 if let Some(chosen)=std::env::var_os("LOGIN_STORE"){
  let dir=std::path::PathBuf::from(chosen);
  return if dir.is_absolute()&&store_holds_roster(&dir) {Some(dir)} else {None};
 }
 let exe=std::env::current_exe().ok()?;
 let shipped=exe.parent()?.to_path_buf();
 if !store_holds_roster(&shipped){return None;}
 user_store(&shipped)
}
// The store this user runs against: an owner-only directory under the user's
// own data directory, named for the shipped store it was seeded from (its place
// and its credentials), so every export and every copy keeps its own accounts.
fn user_data_root()->Option<std::path::PathBuf>{
 #[cfg(windows)]
 {
  let base=std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
  return if base.is_absolute(){Some(base.join("login-stores"))}else{None};
 }
 #[cfg(not(windows))]
 {
  if let Some(xdg)=std::env::var_os("XDG_DATA_HOME"){
   let base=std::path::PathBuf::from(xdg);
   if base.is_absolute(){return Some(base.join("login-stores"));}
  }
  let home=std::path::PathBuf::from(std::env::var_os("HOME")?);
  if !home.is_absolute(){return None;}
  Some(home.join(".local").join("share").join("login-stores"))
 }
}
#[cfg(unix)]
fn make_private_dir(dir:&std::path::Path)->bool{
 use std::os::unix::fs::DirBuilderExt;
 match std::fs::DirBuilder::new().mode(0o700).create(dir){
  Ok(())=>true,
  Err(e)=>e.kind()==std::io::ErrorKind::AlreadyExists,
 }
}
#[cfg(not(unix))]
fn make_private_dir(dir:&std::path::Path)->bool{
 match std::fs::create_dir(dir){Ok(())=>true,Err(e)=>e.kind()==std::io::ErrorKind::AlreadyExists}
}
#[cfg(unix)]
fn owner_only_dir(dir:&std::path::Path)->bool{
 use std::os::unix::fs::PermissionsExt;
 match std::fs::symlink_metadata(dir){Ok(meta)=>meta.file_type().is_dir()&&trusted_meta(&meta)&&meta.permissions().mode()&0o077==0,Err(_)=>false}
}
#[cfg(not(unix))]
fn owner_only_dir(dir:&std::path::Path)->bool{store_dir_trusted(dir)}
fn user_store(shipped:&std::path::Path)->Option<std::path::PathBuf>{
 let roster=read_store(&shipped.join("login-credentials")).ok()?;
 let place=std::fs::canonicalize(shipped).unwrap_or_else(|_| shipped.to_path_buf());
 let mut seed=Vec::new();
 seed.extend_from_slice(place.to_string_lossy().as_bytes());
 seed.push(0);
 seed.extend_from_slice(roster.as_bytes());
 let name=hex_encode(&sha256(&seed)[..16]);
 let root=user_data_root()?;
 if let Some(parent)=root.parent(){ if std::fs::create_dir_all(parent).is_err(){return None;} }
 if !make_private_dir(&root)||!owner_only_dir(&root){return None;}
 let dir=root.join(name);
 if !make_private_dir(&dir)||!owner_only_dir(&dir){return None;}
 for leaf in ["login-credentials","access-held-key"]{
  let target=dir.join(leaf);
  if std::fs::symlink_metadata(&target).is_ok(){continue;}
  let Ok(body)=read_store(&shipped.join(leaf)) else {continue;};
  if !write_private(&target, &body)&&std::fs::symlink_metadata(&target).is_err(){return None;}
 }
 if store_holds_roster(&dir){Some(dir)}else{None}
}
fn fold_name(name:&str)->Option<String>{
 if name.len()>64{return None;}
 let trimmed=name.trim_matches(|c:char| c==' '||c=='\t');
 if trimmed.is_empty()||trimmed.len()>64{return None;}
 let mut folded=String::with_capacity(trimmed.len());
 for c in trimmed.chars(){
  if !c.is_ascii(){return None;}
  let lower=c.to_ascii_lowercase();
  if !(lower.is_ascii_lowercase()||lower.is_ascii_digit()||lower=='.'||lower=='_'||lower=='-'){return None;}
  folded.push(lower);
 }
 Some(folded)
}
fn now_secs()->u64{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)}
struct Attempt{name:String,failures:u32,locked_until:u64}
fn load_lock(path:&std::path::Path)->Option<(Vec<Attempt>,u32,u64)>{
 match read_store(path){
  Err(e)=> if e.kind()==std::io::ErrorKind::NotFound { Some((Vec::new(),0,0)) } else { None },
  Ok(text)=>{
   if text.is_empty(){return Some((Vec::new(),0,0));}
   let mut lines:Vec<&str>=text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
   if lines.last().copied().unwrap_or("").is_empty(){lines.pop();}
   let mut attempts:Vec<Attempt>=Vec::new();
   let mut budget_failures=0u32;
   let mut budget_until=0u64;
   let mut saw_budget=false;
   for line in lines{
    if line.is_empty(){return None;}
    let mut parts=line.split(' ');
    let name=parts.next()?;
    let failures:u32=parts.next()?.parse().ok()?;
    let until:u64=parts.next()?.parse().ok()?;
    if parts.next().is_some()||failures>1000{return None;}
    if name=="*"{
     if saw_budget{return None;}
     saw_budget=true;
     budget_failures=failures;
     budget_until=until;
     continue;
    }
    if name.is_empty()||name.len()>64{return None;}
    if !name.bytes().all(|c| matches!(c, b'a'..=b'z'|b'0'..=b'9'|b'.'|b'_'|b'-')){return None;}
    if attempts.iter().any(|a| a.name==name)||attempts.len()>=64{return None;}
    attempts.push(Attempt{name:name.to_string(),failures,locked_until:until});
   }
   Some((attempts,budget_failures,budget_until))
  }
 }
}
#[cfg(unix)]
fn create_private(path:&std::path::Path, bytes:&[u8])->bool{
 use std::io::Write;
 use std::os::unix::fs::{OpenOptionsExt,PermissionsExt};
 let Ok(mut file)=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path) else {return false;};
 if file.set_permissions(std::fs::Permissions::from_mode(0o600)).is_err(){return false;}
 if file.write_all(bytes).is_err()||file.sync_all().is_err(){return false;}
 match file.metadata(){Ok(meta)=>meta.permissions().mode()&0o777==0o600,Err(_)=>false}
}
#[cfg(windows)]
fn create_private(path:&std::path::Path, bytes:&[u8])->bool{
 use std::os::windows::ffi::OsStrExt;
 use std::ffi::c_void;
 #[repr(C)]
 struct SecurityAttributes{length:u32,descriptor:*mut c_void,inherit:i32}
 #[link(name="advapi32")]
 extern "system"{
  fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text:*const u16,revision:u32,descriptor:*mut *mut c_void,size:*mut u32)->i32;
 }
 #[link(name="kernel32")]
 extern "system"{
  fn CreateFileW(name:*const u16,access:u32,share:u32,attributes:*const SecurityAttributes,disposition:u32,flags:u32,template:*mut c_void)->*mut c_void;
  fn WriteFile(file:*mut c_void,buffer:*const u8,length:u32,written:*mut u32,overlapped:*mut c_void)->i32;
  fn FlushFileBuffers(file:*mut c_void)->i32;
  fn CloseHandle(file:*mut c_void)->i32;
  fn LocalFree(memory:*mut c_void)->*mut c_void;
 }
 let owner_only:Vec<u16>="D:P(A;;FA;;;OW)".encode_utf16().chain(std::iter::once(0)).collect();
 let wide_path:Vec<u16>=path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
 let mut descriptor:*mut c_void=std::ptr::null_mut();
 if unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(owner_only.as_ptr(),1,&mut descriptor,std::ptr::null_mut())}==0{return false;}
 let attributes=SecurityAttributes{length:std::mem::size_of::<SecurityAttributes>() as u32,descriptor,inherit:0};
 let file=unsafe{CreateFileW(wide_path.as_ptr(),0x40000000,0,&attributes,1,0x80,std::ptr::null_mut())};
 unsafe{LocalFree(descriptor);}
 if file.is_null()||file as isize==-1{return false;}
 let mut ok=true;
 let mut at=0usize;
 while ok&&at<bytes.len(){
  let chunk=(bytes.len()-at).min(1<<20) as u32;
  let mut written=0u32;
  if unsafe{WriteFile(file,bytes[at..].as_ptr(),chunk,&mut written,std::ptr::null_mut())}==0||written==0{ok=false;}
  at+=written as usize;
 }
 if ok&&unsafe{FlushFileBuffers(file)}==0{ok=false;}
 unsafe{CloseHandle(file);}
 ok
}
#[cfg(not(any(unix,windows)))]
fn create_private(_path:&std::path::Path, _bytes:&[u8])->bool{false}
fn write_private(path:&std::path::Path, body:&str)->bool{
 let Some(parent)=path.parent() else {return false;};
 let Some(leaf)=path.file_name() else {return false;};
 if !store_dir_trusted(parent){return false;}
 let Some(nonce)=random_bytes(12) else {return false;};
 let mut tmp_name=std::ffi::OsString::from(".");
 tmp_name.push(leaf);
 tmp_name.push(format!(".{}.{}.tmp", std::process::id(), hex_encode(&nonce)));
 let tmp=parent.join(tmp_name);
 if !create_private(&tmp, body.as_bytes()){
  let _=std::fs::remove_file(&tmp);
  return false;
 }
 if std::fs::rename(&tmp, path).is_err(){
  let _=std::fs::remove_file(&tmp);
  return false;
 }
 true
}
fn save_lock(path:&std::path::Path, attempts:&[Attempt], budget_failures:u32, budget_until:u64)->bool{
 let mut body=String::new();
 if budget_failures!=0||budget_until!=0{
  body.push_str("* ");
  body.push_str(&budget_failures.to_string());
  body.push(' ');
  body.push_str(&budget_until.to_string());
  body.push('\n');
 }
 for attempt in attempts{
  if attempt.failures==0&&attempt.locked_until==0{continue;}
  body.push_str(&attempt.name);
  body.push(' ');
  body.push_str(&attempt.failures.to_string());
  body.push(' ');
  body.push_str(&attempt.locked_until.to_string());
  body.push('\n');
 }
 write_private(path,&body)
}
fn check_record(record:&str,secret:&str)->Option<bool>{
 let parts:Vec<&str>=record.split('$').collect();
 if parts.len()!=4||parts[0]!="pbkdf2-sha256"{return None;}
 let rounds:u32=parts[1].parse().ok()?;
 if rounds==0||rounds>600000{return None;}
 let salt=hex_bytes(parts[2])?; let hash=hex_bytes(parts[3])?;
 if salt.len()!=16||hash.len()!=32{return None;}
 Some(same(&stretch(secret.as_bytes(),&salt,rounds),&hash))
}
fn prove(name:&str,secret:&str)->bool{
 let Some(dir)=roster_dir() else { let _=stretch(secret.as_bytes(),&[0u8;16],600000); return false; };
 let Ok(text)=read_store(&dir.join("login-credentials")) else { let _=stretch(secret.as_bytes(),&[0u8;16],600000); return false; };
 if text.trim().is_empty(){ let _=stretch(secret.as_bytes(),&[0u8;16],600000); return false; }
 let lock_path=dir.join("access-lockout");
 let Some((mut attempts, mut budget_failures, mut budget_until))=load_lock(&lock_path) else { return false; };
 let Some(folded)=fold_name(name) else { return false; };
 let now=now_secs();
 if let Some(slot)=attempts.iter_mut().find(|a| a.name==folded){
  if slot.locked_until!=0&&now>=slot.locked_until{slot.failures=0;slot.locked_until=0;}
 }
 if budget_until!=0&&now>=budget_until{budget_failures=0;budget_until=0;}
 let slot_locked=attempts.iter().any(|a| a.name==folded&&a.locked_until!=0&&now<a.locked_until);
 let budget_locked=budget_until!=0&&now<budget_until;
 let mut rounds=600000u32;
 let mut saw=false;
 for raw in text.split('\n'){
  let line=raw.trim_end_matches('\r').trim(); if line.is_empty(){continue;}
  let Some((who,record))=line.split_once('\t') else {continue;};
  if let Some(r)=record.split('$').nth(1).and_then(|t| t.parse::<u32>().ok()){ if r>0&&r<=600000{rounds=r;} }
  let Some(who_folded)=fold_name(who) else {continue;};
  if who_folded==folded{saw=true;}
 }
 if slot_locked||budget_locked{ let _=stretch(secret.as_bytes(),&[0u8;16],rounds); return false; }
 let mut matched=false;
 if saw{
  for raw in text.split('\n'){
   let line=raw.trim_end_matches('\r').trim(); if line.is_empty(){continue;}
   let Some((who,record))=line.split_once('\t') else {continue;};
   let Some(who_folded)=fold_name(who) else {continue;};
   if who_folded!=folded{continue;}
   if let Some(ok)=check_record(record,secret){ if ok{matched=true;} }
  }
 } else {
  let _=stretch(secret.as_bytes(),&[0u8;16],rounds);
 }
 if matched{
  attempts.retain(|a| a.name!=folded);
  return save_lock(&lock_path,&attempts,budget_failures,budget_until);
 }
 const LOCK_FAILURES:u32=5;
 const LOCK_SECONDS:u64=900;
 const BUDGET_FAILURES:u32=50;
 if !attempts.iter().any(|a| a.name==folded){
  if attempts.len()>=64{return false;}
  attempts.push(Attempt{name:folded.clone(),failures:0,locked_until:0});
 }
 if let Some(slot)=attempts.iter_mut().find(|a| a.name==folded){
  if slot.failures<LOCK_FAILURES{slot.failures+=1;}
  if slot.failures>=LOCK_FAILURES{slot.locked_until=now.saturating_add(LOCK_SECONDS);}
 }
 if budget_failures<BUDGET_FAILURES{budget_failures+=1;}
 if budget_failures>=BUDGET_FAILURES{budget_until=now.saturating_add(LOCK_SECONDS);}
 let _=save_lock(&lock_path,&attempts,budget_failures,budget_until);
 false
}
#[repr(C)]
struct LoginAccount{
 enrolled:i32,
 rounds:u32,
 salt:[u8;16],
 hash:[u8;32],
 factor_live:i32,
 factor_key:[u8;32],
 factor_last:u64,
 factor_has_last:i32,
 factor_code:[u8;8],
 factor_code_len:i32,
 failures:i32,
 locked_until_ms:i64,
 reset_live:i32,
 reset_token:[u8;32],
 reset_expires_ms:i64,
 reset_attempts:i32,
}
extern "C"{
 fn login_password_change(account:*mut LoginAccount, now_ms:i64, old_secret:*const u8, old_len:i32, new_secret:*const u8, new_len:i32, new_salt:*const u8)->i32;
 fn login_password_reset_open(account:*mut LoginAccount, now_ms:i64, token:*const u8, ttl_ms:i64)->i32;
 fn login_password_reset_finish(account:*mut LoginAccount, now_ms:i64, token:*const u8, new_secret:*const u8, new_len:i32, new_salt:*const u8)->i32;
}
fn blank_account()->LoginAccount{
 LoginAccount{enrolled:0,rounds:0,salt:[0;16],hash:[0;32],factor_live:0,factor_key:[0;32],factor_last:0,factor_has_last:0,factor_code:[0;8],factor_code_len:0,failures:0,locked_until_ms:0,reset_live:0,reset_token:[0;32],reset_expires_ms:0,reset_attempts:0}
}
fn bytes_ptr(bytes:&[u8])->*const u8{ if bytes.is_empty(){std::ptr::null()} else {bytes.as_ptr()} }
fn now_ms()->i64{
 let millis=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
 i64::try_from(millis).unwrap_or(i64::MAX)
}
fn hex_encode(bytes:&[u8])->String{
 const HEX:&[u8;16]=b"0123456789abcdef";
 let mut out=String::with_capacity(bytes.len()*2);
 for byte in bytes{
  out.push(HEX[(byte>>4) as usize] as char);
  out.push(HEX[(byte&15) as usize] as char);
 }
 out
}
#[cfg(windows)]
fn random_bytes(n:usize)->Option<Vec<u8>>{
 #[link(name="bcrypt")]
 extern "system"{
  fn BCryptGenRandom(algorithm:*mut std::ffi::c_void,buffer:*mut u8,length:u32,flags:u32)->i32;
 }
 let length=u32::try_from(n).ok()?;
 let mut buf=vec![0u8;n];
 let status=unsafe{BCryptGenRandom(std::ptr::null_mut(),buf.as_mut_ptr(),length,0x00000002)};
 if status!=0{return None;}
 Some(buf)
}
#[cfg(not(windows))]
fn random_bytes(n:usize)->Option<Vec<u8>>{
 use std::io::Read;
 let mut buf=vec![0u8;n];
 let mut file=std::fs::File::open("/dev/urandom").ok()?;
 file.read_exact(&mut buf).ok()?;
 Some(buf)
}
fn format_record(account:&LoginAccount)->String{
 let mut out=format!("pbkdf2-sha256${}$", account.rounds);
 out.push_str(&hex_encode(&account.salt));
 out.push('$');
 out.push_str(&hex_encode(&account.hash));
 out
}
fn load_account(record:&str)->Option<LoginAccount>{
 let parts:Vec<&str>=record.split('$').collect();
 if parts.len()!=4||parts[0]!="pbkdf2-sha256"{return None;}
 let rounds:u32=parts[1].parse().ok()?;
 if rounds==0||rounds>600000{return None;}
 let salt=hex_bytes(parts[2])?;
 let hash=hex_bytes(parts[3])?;
 if salt.len()!=16||hash.len()!=32{return None;}
 let mut account=blank_account();
 account.enrolled=1;
 account.rounds=rounds;
 account.salt.copy_from_slice(&salt);
 account.hash.copy_from_slice(&hash);
 Some(account)
}
fn account_line(text:&str, name:&str)->Option<(String,String)>{
 let folded=fold_name(name)?;
 for raw in text.split('\n'){
  let line=raw.trim_end_matches('\r').trim();
  if line.is_empty(){continue;}
  let (who,record)=line.split_once('\t')?;
  if fold_name(who)?==folded{return Some((who.to_string(), record.to_string()));}
 }
 None
}
fn rewrite_record(dir:&std::path::Path, name:&str, record:&str)->bool{
 let path=dir.join("login-credentials");
 let Ok(text)=read_store(&path) else {return false;};
 let Some(folded)=fold_name(name) else {return false;};
 let mut out=String::new();
 let mut saw=false;
 for raw in text.split('\n'){
  let line=raw.trim_end_matches('\r');
  if line.is_empty(){continue;}
  let Some((who,_rest))=line.split_once('\t') else {return false;};
  let Some(who_folded)=fold_name(who) else {return false;};
  if who_folded==folded{
   if saw{return false;}
   saw=true;
   out.push_str(who);
   out.push('\t');
   out.push_str(record);
   out.push('\n');
  } else {
   out.push_str(line);
   out.push('\n');
  }
 }
 if !saw{return false;}
 write_private(&path,&out)
}
fn pull_lock(account:&mut LoginAccount, dir:&std::path::Path, name:&str){
 let Some(folded)=fold_name(name) else {return;};
 let Some((attempts,_,_))=load_lock(&dir.join("access-lockout")) else {return;};
 if let Some(slot)=attempts.iter().find(|item| item.name==folded){
  account.failures=i32::try_from(slot.failures).unwrap_or(i32::MAX);
  account.locked_until_ms=i64::try_from(slot.locked_until.saturating_mul(1000)).unwrap_or(i64::MAX);
 }
}
fn push_lock(account:&LoginAccount, dir:&std::path::Path, name:&str){
 let Some(folded)=fold_name(name) else {return;};
 let path=dir.join("access-lockout");
 let Some((mut attempts, budget_failures, budget_until))=load_lock(&path) else {return;};
 let failures=if account.failures<0 {0u32} else {account.failures as u32};
 let until=if account.locked_until_ms<=0 {0u64} else {(account.locked_until_ms as u64)/1000};
 if let Some(slot)=attempts.iter_mut().find(|item| item.name==folded){
  slot.failures=failures;
  slot.locked_until=until;
 } else if failures!=0||until!=0 {
  if attempts.len()<64{attempts.push(Attempt{name:folded,failures,locked_until:until});}
 }
 let _=save_lock(&path,&attempts,budget_failures,budget_until);
}
struct StoredSession{name:String,digest:String,expires:u64}
fn sessions_path(dir:&std::path::Path)->std::path::PathBuf{dir.join("access-sessions")}
fn load_sessions(dir:&std::path::Path)->Option<Vec<StoredSession>>{
 let text=match read_store(&sessions_path(dir)){
  Ok(text)=>text,
  Err(e)=>return if e.kind()==std::io::ErrorKind::NotFound {Some(Vec::new())} else {None},
 };
 let mut out=Vec::new();
 for raw in text.split('\n'){
  let line=raw.trim_end_matches('\r');
  if line.is_empty(){continue;}
  let mut parts=line.split(' ');
  let name=fold_name(parts.next()?)?;
  let digest=parts.next()?.to_string();
  let expires:u64=parts.next()?.parse().ok()?;
  if parts.next().is_some()||digest.len()!=64||hex_bytes(&digest).is_none(){return None;}
  out.push(StoredSession{name,digest,expires});
 }
 Some(out)
}
fn save_sessions(dir:&std::path::Path, sessions:&[StoredSession])->bool{
 let now=now_secs();
 let mut body=String::new();
 for s in sessions{
  if s.expires<=now{continue;}
  body.push_str(&format!("{} {} {}\n", s.name, s.digest, s.expires));
 }
 write_private(&sessions_path(dir), &body)
}
fn session_digest(token:&str)->Option<String>{
 let raw=hex_bytes(token)?;
 if raw.len()!=32{return None;}
 Some(hex_encode(&sha256(&raw)))
}
fn mint_session(name:&str)->Option<(String,u64)>{
 let dir=roster_dir()?;
 let folded=fold_name(name)?;
 let mut sessions=load_sessions(&dir)?;
 let now=now_secs();
 sessions.retain(|s| s.expires>now);
 while sessions.len()>=64{sessions.remove(0);}
 let token=hex_encode(&random_bytes(32)?);
 let expires=now.saturating_add(SESSION_SECONDS);
 sessions.push(StoredSession{name:folded,digest:session_digest(&token)?,expires});
 if !save_sessions(&dir, &sessions){return None;}
 Some((token,expires))
}
fn resume_session(token:&str)->Option<u64>{
 let dir=roster_dir()?;
 let digest=session_digest(token)?;
 let sessions=load_sessions(&dir)?;
 let now=now_secs();
 let text=read_store(&dir.join("login-credentials")).ok()?;
 let mut found=None;
 for s in &sessions{ if same(s.digest.as_bytes(), digest.as_bytes())&&s.expires>now{found=Some(s);} }
 let s=found?;
 account_line(&text, &s.name)?;
 Some(s.expires)
}
fn end_session(token:&str)->bool{
 let Some(dir)=roster_dir() else {return false;};
 let Some(digest)=session_digest(token) else {return false;};
 let Some(mut sessions)=load_sessions(&dir) else {return false;};
 let before=sessions.len();
 let now=now_secs();
 sessions.retain(|s| !same(s.digest.as_bytes(), digest.as_bytes()));
 let removed=sessions.len()!=before;
 sessions.retain(|s| s.expires>now);
 save_sessions(&dir, &sessions)&&removed
}
fn end_sessions_for(dir:&std::path::Path, name:&str)->bool{
 let Some(folded)=fold_name(name) else {return false;};
 let Some(mut sessions)=load_sessions(dir) else {return false;};
 sessions.retain(|s| s.name!=folded);
 save_sessions(dir, &sessions)
}
fn reset_path(dir:&std::path::Path)->std::path::PathBuf{dir.join("access-reset")}
fn save_reset(dir:&std::path::Path, name:&str, account:&LoginAccount, digest:&[u8;32])->bool{
 if account.reset_live==0{
  let _=std::fs::remove_file(reset_path(dir));
  return true;
 }
 let Some(folded)=fold_name(name) else {return false;};
 let body=format!("{} {} {} {}\n", folded, hex_encode(digest), account.reset_expires_ms, account.reset_attempts);
 write_private(&reset_path(dir), &body)
}
// The reset code is never shown to the caller and never stored beside the
// account: it goes only to the outbox the installer pinned at build time, a
// directory the account holder controls and that is not the credential store.
fn reset_outbox(store:&std::path::Path)->Option<std::path::PathBuf>{
 let dir=std::path::PathBuf::from(option_env!("LOGIN_RESET_OUTBOX")?);
 if !dir.is_absolute()||!store_dir_trusted(&dir){return None;}
 let a=std::fs::canonicalize(&dir).ok()?;
 let b=std::fs::canonicalize(store).ok()?;
 if a==b{return None;}
 Some(dir)
}
fn load_reset(dir:&std::path::Path, name:&str, account:&mut LoginAccount, digest:&mut [u8;32])->bool{
 let text=match read_store(&reset_path(dir)){
  Ok(text)=>text,
  Err(e)=>return e.kind()==std::io::ErrorKind::NotFound,
 };
 let mut parts=text.split_whitespace();
 let Some(stored_name)=parts.next() else {return false;};
 let Some(token)=parts.next() else {return false;};
 let Some(expires)=parts.next() else {return false;};
 let Some(attempts)=parts.next() else {return false;};
 if parts.next().is_some(){return false;}
 let Some(folded)=fold_name(name) else {return false;};
 let Some(stored)=fold_name(stored_name) else {return false;};
 if folded!=stored{return false;}
 let Some(raw)=hex_bytes(token) else {return false;};
 if raw.len()!=32{return false;}
 digest.copy_from_slice(&raw);
 account.reset_expires_ms=expires.parse().unwrap_or(0);
 account.reset_attempts=attempts.parse().unwrap_or(0);
 account.reset_live=1;
 true
}
fn prepared_account(dir:&std::path::Path, name:&str)->LoginAccount{
 let Ok(text)=read_store(&dir.join("login-credentials")) else {return blank_account();};
 account_line(&text, name).and_then(|(_,record)| load_account(&record)).unwrap_or_else(blank_account)
}
fn change_password(name:&str, old:&str, new_secret:&str)->i32{
 let Some(dir)=roster_dir() else {return 401;};
 let mut account=prepared_account(&dir, name);
 pull_lock(&mut account, &dir, name);
 let Some(salt)=random_bytes(16) else {return 500;};
 let mut salt_bytes=[0u8;16];
 salt_bytes.copy_from_slice(&salt);
 let status=unsafe{login_password_change(&mut account, now_ms(), bytes_ptr(old.as_bytes()), old.len() as i32, bytes_ptr(new_secret.as_bytes()), new_secret.len() as i32, salt_bytes.as_ptr())};
 push_lock(&account, &dir, name);
 if status==200{
  if !rewrite_record(&dir, name, &format_record(&account)){return 500;}
  let _=std::fs::remove_file(reset_path(&dir));
  if !end_sessions_for(&dir, name){return 500;}
 }
 status
}
fn reset_open(name:&str)->i32{
 let Some(dir)=roster_dir() else {return 401;};
 let mut account=prepared_account(&dir, name);
 let Some(token)=random_bytes(32) else {return 500;};
 let mut token_bytes=[0u8;32];
 token_bytes.copy_from_slice(&token);
 let Some(outbox)=reset_outbox(&dir) else {return 503;};
 let Some(folded)=fold_name(name) else {return 401;};
 let status=unsafe{login_password_reset_open(&mut account, now_ms(), token_bytes.as_ptr(), 60000)};
 if status==200{
  let digest=sha256(&token_bytes);
  if !save_reset(&dir, name, &account, &digest){return 500;}
  if !write_private(&outbox.join(format!("{}.reset-code", folded)), &format!("{}\n", hex_encode(&token_bytes))){
   let _=std::fs::remove_file(reset_path(&dir));
   return 500;
  }
 }
 status
}
fn reset_finish(name:&str, token_hex:&str, new_secret:&str)->i32{
 let Some(dir)=roster_dir() else {return 401;};
 let mut account=prepared_account(&dir, name);
 let mut digest=[0u8;32];
 if !load_reset(&dir, name, &mut account, &mut digest){return 401;}
 let Some(token)=hex_bytes(token_hex) else {return 401;};
 if token.len()!=32{return 401;}
 if account.reset_live!=0{
  // Only the digest is at rest. A matching code is handed to the account
  // module as the live token; any other code becomes a token that cannot match,
  // so the module still spends its attempt budget and enforces expiry.
  if same(&sha256(&token), &digest){account.reset_token.copy_from_slice(&token);}
  else{for i in 0..32{account.reset_token[i]=!token[i];}}
 }
 let Some(salt)=random_bytes(16) else {return 500;};
 let mut salt_bytes=[0u8;16];
 salt_bytes.copy_from_slice(&salt);
 let status=unsafe{login_password_reset_finish(&mut account, now_ms(), token.as_ptr(), bytes_ptr(new_secret.as_bytes()), new_secret.len() as i32, salt_bytes.as_ptr())};
 if status==200{
  if !rewrite_record(&dir, name, &format_record(&account)){return 500;}
  let _=std::fs::remove_file(reset_path(&dir));
  if let Some(outbox)=reset_outbox(&dir){ if let Some(folded)=fold_name(name){ let _=std::fs::remove_file(outbox.join(format!("{}.reset-code", folded))); } }
  push_lock(&account, &dir, name);
  if !end_sessions_for(&dir, name){return 500;}
  return 200;
 }
 if status==429{
  let _=std::fs::remove_file(reset_path(&dir));
  return 429;
 }
 if status==401 && !save_reset(&dir, name, &account, &digest){return 500;}
 status
}
fn stdin_line()->String{
 let mut line=String::new();
 if std::io::stdin().read_line(&mut line).is_err(){return String::new();}
 while line.ends_with('\n')||line.ends_with('\r'){line.pop();}
 line
}
fn headless()->bool {
 let args:Vec<String>=std::env::args().skip(1).collect();
 if args.is_empty(){return false;}
 if args[0]=="--change-password"||args[0]=="--password"{
  if args.len()!=2{eprintln!("usage: --change-password <name>, current and new password on stdin");std::process::exit(2);}
  let old=stdin_line();
  let new_secret=stdin_line();
  let status=change_password(&args[1],&old,&new_secret);
  println!("{}", status);
  std::process::exit(if status==200 {0} else {2});
 }
 if args[0]=="--reset-open"||args[0]=="--reset"{
  if args.len()!=2{eprintln!("usage: --reset-open <name>");std::process::exit(2);}
  let status=reset_open(&args[1]);
  println!("{}", status);
  std::process::exit(if status==200 {0} else {2});
 }
 if args[0]=="--reset-finish"||args[0]=="--reset-password"{
  if args.len()!=2{eprintln!("usage: --reset-finish <name>, reset code and new password on stdin");std::process::exit(2);}
  let token=stdin_line();
  let new_secret=stdin_line();
  let status=reset_finish(&args[1],&token,&new_secret);
  println!("{}", status);
  std::process::exit(if status==200 {0} else {2});
 }
 if args[0]=="--session-open"{
  if args.len()!=2{eprintln!("usage: --session-open <name>, password on stdin");std::process::exit(2);}
  let secret=stdin_line();
  if !prove(&args[1],&secret){println!("401");std::process::exit(2);}
  let Some((token,expires))=mint_session(&args[1]) else {println!("500");std::process::exit(2);};
  println!("200 {} {}", token, expires);
  std::process::exit(0);
 }
 if args[0]=="--logout"{
  if args.len()!=1{eprintln!("usage: --logout, session token on stdin");std::process::exit(2);}
  let token=stdin_line();
  let status=if end_session(&token) {200} else {401};
  println!("{}", status);
  std::process::exit(if status==200 {0} else {2});
 }
 let mut i=0usize;
 let mut signed=false;
 let mut resumed:Option<u64>=None;
 let mut name=String::new();
 let mut secret=String::new();
 if args[i]=="--sign-in"{
  if args.len()<2{eprintln!("usage: --sign-in <name>, password on stdin");std::process::exit(2);}
  name=args[1].clone(); secret=stdin_line(); signed=true; i=2;
 } else if args[i]=="--session"{
  let token=stdin_line();
  match resume_session(&token){
   Some(expires)=>{resumed=Some(expires);begin_stored_session(token.clone(),expires);}
   None=>{eprintln!("session expired or revoked");std::process::exit(2);}
  }
  i=1;
 }
 if i==args.len(){
  if resumed.is_some(){return false;}
  if !signed{eprintln!("usage: --activate <id-high> <id-low>");std::process::exit(2);}
  if !sign_in_session(&name,&secret){std::process::exit(2);}
  return false;
 }
 if args[i]=="--show"{
  if args.len()!=i+1{eprintln!("usage: --activate <id-high> <id-low>");std::process::exit(2);}
  if signed{ if !prove(&name,&secret){std::process::exit(2);} begin_session(now_secs().saturating_add(SESSION_SECONDS)); }
  for c in controls(){ if c.visible==0||!admit(&c){continue;} println!("{} {} {}",c.id.0,c.id.1,open_text(&c)); }
  return true;
 }
 if args.len()-i!=3||args[i]!="--activate"{eprintln!("usage: --activate <id-high> <id-low>");std::process::exit(2);}
 if signed{ if !prove(&name,&secret){std::process::exit(2);} begin_session(now_secs().saturating_add(SESSION_SECONDS)); }
 let id=match(args[i+1].parse::<u64>(),args[i+2].parse::<u64>()){(Ok(a),Ok(b))=>(a,b),_=>{eprintln!("invalid control identity");std::process::exit(2);}};
 let cs=controls(); let Some(c)=cs.iter().find(|c|c.id==id) else {eprintln!("action unavailable");std::process::exit(2);};
 if !admit(c){eprintln!("sign in required");std::process::exit(2);}
 if c.kind==1{ if c.gate==2&&!c.text.is_empty(){ let f=fields(); println!("{}",caption(c,&f)); return true; } eprintln!("action unavailable");std::process::exit(2);}
 let mut f=fields();activate(c,&mut f);println!("{}",caption(c,&f));true
}
use std::os::raw::{c_char,c_int,c_long,c_ulong};
use std::ffi::CString;
#[repr(C)]
struct ButtonEvent {type_:c_int,serial:c_ulong,send:c_int,display:*mut c_void,window:c_ulong,root:c_ulong,subwindow:c_ulong,time:c_ulong,x:c_int,y:c_int,x_root:c_int,y_root:c_int,state:u32,button:u32,same:c_int}
#[repr(C)]
union Event {type_:c_int,button:std::mem::ManuallyDrop<ButtonEvent>,pad:[c_long;24]}
#[link(name="X11")]
extern "C" {
 fn XOpenDisplay(n:*const c_char)->*mut c_void;
 fn XDefaultRootWindow(d:*mut c_void)->c_ulong;
 fn XCreateSimpleWindow(d:*mut c_void,p:c_ulong,x:c_int,y:c_int,w:u32,h:u32,b:u32,bc:c_ulong,bg:c_ulong)->c_ulong;
 fn XStoreName(d:*mut c_void,w:c_ulong,n:*const c_char)->c_int;
 fn XSelectInput(d:*mut c_void,w:c_ulong,m:c_long)->c_int;
 fn XMapWindow(d:*mut c_void,w:c_ulong)->c_int;
 fn XCreateGC(d:*mut c_void,w:c_ulong,m:c_ulong,v:*mut c_void)->*mut c_void;
 fn XSetForeground(d:*mut c_void,g:*mut c_void,p:c_ulong)->c_int;
 fn XFillRectangle(d:*mut c_void,w:c_ulong,g:*mut c_void,x:c_int,y:c_int,wi:u32,he:u32)->c_int;
 fn XDrawString(d:*mut c_void,w:c_ulong,g:*mut c_void,x:c_int,y:c_int,s:*const c_char,n:c_int)->c_int;
 fn XNextEvent(d:*mut c_void,e:*mut Event)->c_int;
 fn XLookupKeysym(e:*mut ButtonEvent,index:c_int)->c_ulong;
 fn XFlush(d:*mut c_void)->c_int;
 fn XClearWindow(d:*mut c_void,w:c_ulong)->c_int;
 fn XFreeGC(d:*mut c_void,g:*mut c_void)->c_int;
 fn XCloseDisplay(d:*mut c_void)->c_int;
}
// Sign-in role of a control on the login page: 1 name entry, 2 password entry, 3 submit.
fn login_role(c:&Control)->u8{
 if c.gate!=1{return 0;}
 let label=open_text(c).to_ascii_lowercase();
 if label.contains("password"){2}
 else if label.contains("user")||label.contains("email")||label.contains("account"){1}
 else{3}
}
fn entry_caption(c:&Control,f:&[i64],entry:&str)->String{
 match login_role(c){1=>format!("{}: {}",open_text(c),entry),2=>format!("{}: {}",open_text(c),"*".repeat(entry.chars().count())),_=>caption(c,f)}
}
fn submit_sign_in(cs:&[Control],entries:&mut [String])->bool{
 let mut name=String::new();
 let mut secret=String::new();
 for (i,c) in cs.iter().enumerate(){match login_role(c){1=>{if name.is_empty(){name=entries[i].clone();}},2=>{if secret.is_empty(){secret=entries[i].clone();}},_=>{}}}
 let ok=!name.is_empty()&&sign_in_session(&name,&secret);
 for (i,c) in cs.iter().enumerate(){if login_role(c)==2{entries[i].clear();}}
 ok
}
fn main() {if headless(){return;} unsafe {
 let d=XOpenDisplay(std::ptr::null()); if d.is_null(){eprintln!("display unavailable");std::process::exit(1);}
 let w=XCreateSimpleWindow(d,XDefaultRootWindow(d),0,0,WIDTH as u32,HEIGHT as u32,0,0,0xf4f5f7);
 XStoreName(d,w,CString::new(TITLE).unwrap().as_ptr()); XSelectInput(d,w,(1<<15)|(1<<2)|(1<<0)); if SHOWN!=0{XMapWindow(d,w);}
 let g=XCreateGC(d,w,0,std::ptr::null_mut()); let cs=controls(); let mut f=fields(); let mut focus=0usize;
 let mut entries:Vec<String>=vec![String::new();cs.len()];
 loop {
  if focus<cs.len()&&(cs[focus].visible==0||!admit(&cs[focus])){focus=cs.iter().position(|c| c.visible!=0&&admit(c)).unwrap_or(0);}
  XClearWindow(d,w);
  for (i,c) in cs.iter().enumerate(){if c.visible==0||!admit(c){continue;}XSetForeground(d,g,if i==focus {0xc7dcf3}else{0xe0e6ed});XFillRectangle(d,w,g,c.x,c.y,c.w as u32,c.h as u32);
   XSetForeground(d,g,0x162536);let text=entry_caption(c,&f,&entries[i]);XDrawString(d,w,g,c.x+8,c.y+c.h/2+5,text.as_ptr().cast(),text.len() as i32);}
  XFlush(d);let mut e=Event{pad:[0;24]}; XNextEvent(d,&mut e);
  let mut submit=false;
  if e.type_==4 { let b=&e.button; if b.button==1 {for(i,c)in cs.iter().enumerate(){if c.visible==0||!admit(c){continue;}if b.x>=c.x&&b.x<c.x+c.w&&b.y>=c.y&&b.y<c.y+c.h {focus=i;match login_role(c){0=>{activate(c,&mut f);let t=format!("{} — {}",TITLE,caption(c,&f));XStoreName(d,w,CString::new(t).unwrap().as_ptr());},3=>{submit=true;},_=>{}}}}}}
  if e.type_==2 { // Key events share button-event coordinates and keycode slot.
   let shifted=(e.button.state&1)!=0;
   let code=XLookupKeysym(&mut *e.button,if shifted {1} else {0});
   let role=if focus<cs.len() {login_role(&cs[focus])} else {0};
   if code==0xff1b {break;}
   if code==0xffc9 {sign_out_session();for x in entries.iter_mut(){x.clear();}XStoreName(d,w,CString::new(TITLE).unwrap().as_ptr());}
   else if code==0xff09&&!cs.is_empty(){let mut step=0usize;while step<cs.len(){focus=(focus+1)%cs.len();step+=1;if cs[focus].visible!=0&&admit(&cs[focus]){break;}}}
   else if (role==1||role==2)&&code==0xff08 {entries[focus].pop();}
   else if (role==1||role==2)&&code==0xff0d {submit=true;}
   else if (role==1||role==2)&&(0x20..0x7f).contains(&code) {if entries[focus].len()<128{entries[focus].push(code as u8 as char);}}
   else if role==3&&(code==0xff0d||code==0x20) {submit=true;}
   else if (code==0xff0d||code==0x20)&&!cs.is_empty()&&cs[focus].visible!=0&&admit(&cs[focus]){activate(&cs[focus],&mut f);}
  }
  if submit {
   let ok=submit_sign_in(&cs,&mut entries);
   let t=format!("{} — {}",TITLE,if ok {"signed in"} else {"sign in failed"});XStoreName(d,w,CString::new(t).unwrap().as_ptr());
  }
 }
 XFreeGC(d,g);XCloseDisplay(d);
}}

//! Sequential, process-held CSS/PNG retrieval for an explicitly approved page.
//! Resource bytes are data; no scripts, cookies or model credentials are used.
use super::{*,resource_plan::{Plan,Role},resource_url::{self,Url}};
const CSS_LIMIT:usize=262144;
const INPUT_LIMIT:usize=8*1024*1024;
const WIRE_LIMIT:usize=2*1024*1024;
struct Pending{index:usize,current:Url,visited:Vec<String>}
pub(super) struct Session{plan:Plan,next:usize,pending:Option<Pending>,bodies:BTreeMap<usize,Vec<u8>>,failures:BTreeMap<usize,String>,notes:Vec<String>,css_bytes:usize,input_bytes:usize,reserved:usize,room:usize,requests:usize}
fn note_text(value:&str)->String{value.chars().take(512).map(|c|if c.is_control(){'\u{fffd}'}else{c}).collect()}
impl Session{
 pub(super) fn new(html:&str,source:&str,room:usize)->Result<Self>{
  let document=html_document::parse(html.as_bytes(),true);let source=resource_url::parse(source).map_err(|e|e.to_string())?;
  let plan=resource_plan::plan(&document,&source).map_err(|e|e.to_string())?;
  let notes=plan.notes.iter().map(|n|format!("Node {} resource plan: {}",n.node,n.code)).collect();
  Ok(Self{plan,next:0,pending:None,bodies:BTreeMap::new(),failures:BTreeMap::new(),notes,css_bytes:0,input_bytes:0,reserved:0,room,requests:0})
 }
 fn note(&mut self,node:usize,message:&str){let value=note_text(message);self.failures.insert(node,value.clone());if self.notes.len()<256{self.notes.push(format!("Node {node}: {value}"));}}
 pub(super) fn current_room(&mut self,room:usize)->Result<()>{if self.reserved>room{return Err("Current resource budget cannot retain already adopted capture resources".into());}self.room=room;Ok(())}
 pub(super) fn ready(&self)->bool{self.pending.is_none()&&self.next==self.plan.resources.len()}
 pub(super) fn next(&mut self)->Result<Option<Json>>{
  loop{
   if self.pending.is_some(){return self.effect().map(Some);}
   let Some(resource)=self.plan.resources.get(self.next)else{return Ok(None)};
   let(node,target,index)=(resource.node,resource.resolved.clone(),self.next);self.next+=1;
   if !target.same_origin(&self.plan.retrieval_url){self.note(node,"Resource origin is outside the approved final-page origin; no request was sent");continue;}
   if self.input_bytes>=INPUT_LIMIT||resource.role==Role::Stylesheet&&self.css_bytes>=CSS_LIMIT{self.note(node,"Existing aggregate capture input bound exhausted; resource was not fetched");continue;}
   if self.reserved>=self.room{self.note(node,"Current capture memory budget exhausted; resource was not fetched");continue;}
   let url=target.wire_url();self.pending=Some(Pending{index,current:target,visited:vec![url]});return self.effect().map(Some);
  }
 }
 fn effect(&mut self)->Result<Json>{
  let pending=self.pending.as_ref().ok_or("No capture resource request is pending")?;let resource=&self.plan.resources[pending.index];
  // One initial request and at most five redirects per planned occurrence.
  if self.requests>=128*6{return Err("Capture resource request bound exhausted".into());}self.requests+=1;
  let url=pending.current.wire_url();let accept=if resource.role==Role::Stylesheet{"text/css"}else{"image/png"};
  let mut effect=network::page_wire(&url,url.starts_with("http://"),vec![("Accept",accept.into())])?;
  put(&mut effect,"purpose",s("web_tool"));put(&mut effect,"limit",Num(WIRE_LIMIT.to_string()));Ok(effect)
 }
 pub(super) fn response(&mut self,raw:&[u8])->Result<()>{
  let pending=self.pending.as_ref().ok_or("Capture resource response has no pending request")?;let index=pending.index;let node=self.plan.resources[index].node;
  // A malformed/unavailable resource is reported, not silently represented as painted.
  let result=self.accept(raw);if let Err(reason)=result{self.note(node,&reason);self.pending=None;}Ok(())
 }
 fn accept(&mut self,raw:&[u8])->Result<()>{
  let response=representation(raw)?;let pending=self.pending.as_ref().ok_or("Capture resource response has no pending request")?;let resource=&self.plan.resources[pending.index];
  if matches!(response.code,301|302|303|307|308){
   let location=response.location.ok_or("Resource redirect has no Location")?;
   if pending.visited.len()>5{return Err("Resource redirect limit reached; no further request was sent".into());}
   let target=resource_url::resolve(&pending.current,&location).map_err(|e|e.to_string())?;
   if !target.same_origin(&self.plan.retrieval_url){return Err("Resource redirect leaves the approved final-page origin; no request was sent".into());}
   let wire=target.wire_url();if pending.visited.iter().any(|u|u==&wire){return Err("Resource redirect loop; no further request was sent".into());}
   let pending=self.pending.as_mut().unwrap();pending.visited.push(wire);pending.current=target;return Ok(());
  }
  if response.code!=200{return Err(format!("Resource returned HTTP {}; no bytes were adopted",response.code));}
  let wanted=if resource.role==Role::Stylesheet{"text/css"}else{"image/png"};
  if response.media.as_deref()!=Some(wanted){return Err(format!("Resource Content-Type must be {wanted}; no bytes were adopted"));}
  let limit=if resource.role==Role::Stylesheet{CSS_LIMIT.saturating_sub(self.css_bytes)}else{INPUT_LIMIT.saturating_sub(self.input_bytes)};
  let allowance=self.room.saturating_sub(self.reserved).saturating_sub(raw.len().saturating_mul(3))/4;let limit=limit.min(allowance);
  let gzip=http_content_coding::coding(&response.coding)?;
  let mut bytes=if gzip{http_content_coding::decode(&response.body,true,limit)?}else{if response.body.len()>limit{return Err("Resource representation exceeds current decoded memory allowance".into());}response.body};
  if resource.role==Role::Stylesheet{
   let ascii=match response.charset.as_deref(){None|Some("utf-8"|"utf8")=>false,Some("us-ascii"|"ascii")=>true,_=>return Err("Stylesheet charset is unsupported; no bytes were adopted".into())};
   if ascii&&!bytes.is_ascii(){return Err("Stylesheet bytes do not match declared US-ASCII".into());}
   if !ascii&&bytes.starts_with(b"\xef\xbb\xbf"){bytes.drain(..3);}
   std::str::from_utf8(&bytes).map_err(|_|"Stylesheet bytes are not UTF-8")?;
   if bytes.len()>CSS_LIMIT.saturating_sub(self.css_bytes){return Err("Existing aggregate stylesheet input bound exhausted".into());}
  }
  if bytes.len()>INPUT_LIMIT.saturating_sub(self.input_bytes){return Err("Existing aggregate capture input bound exhausted".into());}
  let mut cost=bytes.len().checked_mul(if resource.role==Role::Stylesheet{32}else{2}).ok_or("Resource memory estimate overflow")?;
  if resource.role==Role::ImagePng{
   if bytes.len()<24||!bytes.starts_with(b"\x89PNG\r\n\x1a\n")||&bytes[12..16]!=b"IHDR"{return Err("PNG resource has no supported PNG header".into());}
   let w=u32::from_be_bytes(bytes[16..20].try_into().unwrap())as usize;let h=u32::from_be_bytes(bytes[20..24].try_into().unwrap())as usize;
   cost=cost.checked_add(w.checked_mul(h).and_then(|p|p.checked_mul(32)).ok_or("PNG resource memory estimate overflow")?).ok_or("PNG resource memory estimate overflow")?;
  }
  if cost>self.room.saturating_sub(self.reserved){return Err("Resource exceeds current render memory budget; no bytes were adopted".into());}
  self.reserved+=cost;self.input_bytes+=bytes.len();if resource.role==Role::Stylesheet{self.css_bytes+=bytes.len();}
  let node=resource.node;let address=pending.current.wire_url();let size=bytes.len();self.bodies.insert(node,bytes);if self.notes.len()<256{self.notes.push(format!("Node {node}: loaded {wanted}, {size} bytes, {}",note_text(&address)));}self.pending=None;Ok(())
 }
 pub(super) fn bytes(&self,node:usize,limit:usize)->Result<Vec<u8>>{let bytes=self.bodies.get(&node).ok_or_else(||self.failures.get(&node).cloned().unwrap_or("Resource was not fetched or adopted".into()))?;if bytes.len()>limit{return Err("Retained resource exceeds remaining renderer input bound".into());}Ok(bytes.clone())}
 pub(super) fn reserved(&self)->usize{self.reserved}
 pub(super) fn summary(&self)->String{let mut out=format!("Approved resource mode: same_origin. Final retrieval URL: {}. Selected document base: {}. {} planned CSS/PNG occurrences; {} credential-free GET requests including redirects; {} resource bodies adopted ({} bytes). CSS and PNG inputs retain existing 256 KiB/8 MiB aggregate limits. Resource observations are not browser execution.",self.plan.retrieval_url,self.plan.document_base,self.plan.resources.len(),self.requests,self.bodies.len(),self.input_bytes);for note in &self.notes{out.push('\n');out.push_str(note);}out}
}
struct Representation{code:u16,media:Option<String>,charset:Option<String>,location:Option<String>,coding:String,body:Vec<u8>}
fn token(b:u8)->bool{b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b)}
fn media(value:&str)->Result<(String,Option<String>)>{
 if value.len()>4096||!value.is_ascii(){return Err("Resource Content-Type exceeds its ASCII metadata bound".into());}
 let b=value.as_bytes();let mut at=0;let skip=|at:&mut usize|{while b.get(*at).is_some_and(|v|matches!(v,b' '|b'\t')){*at+=1;}};skip(&mut at);let begin=at;while b.get(at).is_some_and(|v|token(*v)){at+=1;}if at==begin||b.get(at)!=Some(&b'/'){return Err("Invalid resource media type".into());}at+=1;let sub=at;while b.get(at).is_some_and(|v|token(*v)){at+=1;}if at==sub||at-begin>128{return Err("Invalid resource media type".into());}let kind=value[begin..at].to_ascii_lowercase();let mut names=std::collections::BTreeSet::new();let mut charset=None;
 loop{skip(&mut at);if at==b.len(){break;}if b[at]!=b';'{return Err("Invalid resource media parameter".into());}at+=1;skip(&mut at);let begin=at;while b.get(at).is_some_and(|v|token(*v)){at+=1;}if begin==at{return Err("Missing resource media parameter name".into());}let name=value[begin..at].to_ascii_lowercase();if !names.insert(name.clone())||names.len()>16{return Err("Duplicate or excessive resource media parameters".into());}skip(&mut at);if b.get(at)!=Some(&b'='){return Err("Missing resource media parameter value".into());}at+=1;skip(&mut at);let mut val=String::new();if b.get(at)==Some(&b'"'){at+=1;loop{let c=*b.get(at).ok_or("Unterminated resource media parameter")?;at+=1;if c==b'"'{break;}let c=if c==b'\\'{let c=*b.get(at).ok_or("Incomplete resource media quoted pair")?;at+=1;c}else{c};if c<32&&c!=b'\t'||c==127{return Err("Control in resource media parameter".into());}val.push(c as char);}}else{let begin=at;while b.get(at).is_some_and(|v|token(*v)){at+=1;}if begin==at{return Err("Missing resource media parameter value".into());}val.push_str(&value[begin..at]);}if name=="charset"{charset=Some(val.to_ascii_lowercase());}}
 Ok((kind,charset))
}
fn representation(raw:&[u8])->Result<Representation>{
 if raw.len()>WIRE_LIMIT{return Err("Resource HTTP wire exceeds the existing 2 MiB bound".into());}
 let mut rest=raw;
 for interim in 0..=4{
  let end=rest.windows(4).position(|w|w==b"\r\n\r\n").ok_or("Incomplete resource HTTP headers")?;if end>65536{return Err("Resource HTTP headers exceed bound".into());}
  let text=std::str::from_utf8(&rest[..end]).map_err(|_|"Invalid resource HTTP headers")?;let mut lines=text.split("\r\n");let first=lines.next().ok_or("Missing resource status")?;let mut fields=first.split(' ');let version=fields.next().unwrap_or("");let status=fields.next().unwrap_or("");if !matches!(version,"HTTP/1.0"|"HTTP/1.1")||status.len()!=3||!status.bytes().all(|v|v.is_ascii_digit())||first.bytes().any(|v|v<32&&v!=b'\t'||v==127){return Err("Invalid resource HTTP status".into());}let code=status.parse::<u16>().map_err(|_|"Invalid resource status")?;
  if code==101{return Err("Resource protocol upgrades are unsupported".into());}if (100..200).contains(&code){if interim==4{return Err("Excessive resource informational responses".into());}rest=&rest[end+4..];continue;}
  let(mut mime,mut charset,mut location,mut encoding)=(None,None,None,None);let mut count=0;let mut chunked=false;
  for line in lines{count+=1;if count>256{return Err("Excessive resource header fields".into());}let(k,v)=line.split_once(':').ok_or("Invalid resource header")?;if k.is_empty()||!k.bytes().all(token)||v.bytes().any(|v|v<32&&v!=b'\t'||v==127){return Err("Invalid resource header syntax".into());}let v=v.trim_matches([' ','\t']);
   if k.eq_ignore_ascii_case("content-type"){if mime.is_some(){return Err("Duplicate resource Content-Type".into());}let(m,c)=media(v)?;mime=Some(m);charset=c;}
   if k.eq_ignore_ascii_case("content-encoding"){if encoding.is_some(){return Err("Duplicate resource content coding".into());}encoding=Some(v.to_string());}
   if k.eq_ignore_ascii_case("location"){if location.is_some()||v.len()>8192{return Err("Duplicate or oversized resource Location".into());}location=Some(v.into());}
   if k.eq_ignore_ascii_case("transfer-encoding"){chunked=v.eq_ignore_ascii_case("chunked");}
  }
  if chunked{check_trailers(&rest[end+4..])?;}
  // The shared decoder checks exact complete framing. Status is replaced only
  // in this private copy so redirect/error bodies can be discarded as data.
  let mut framed=rest.to_vec();let start=version.len()+1;framed[start..start+3].copy_from_slice(b"200");let(_,body)=network::decoded_coded_bytes(&framed)?;
  return Ok(Representation{code,media:mime,charset,location,coding:encoding.unwrap_or_else(||"identity".into()),body});
 }
 Err("Excessive informational responses".into())
}
fn check_trailers(data:&[u8])->Result<()>{let mut at=0;loop{let end=data.get(at..).ok_or("Invalid resource chunk offset")?.windows(2).position(|w|w==b"\r\n").ok_or("Incomplete resource chunk header")?;let n=usize::from_str_radix(std::str::from_utf8(&data[at..at+end]).map_err(|_|"Invalid resource chunk size")?.split(';').next().unwrap_or("").trim(),16).map_err(|_|"Invalid resource chunk size")?;at=at.checked_add(end+2).ok_or("Resource chunk offset overflow")?;if n==0{let text=std::str::from_utf8(&data[at..]).map_err(|_|"Invalid resource trailers")?;for line in text.split("\r\n").filter(|v|!v.is_empty()){let(k,v)=line.split_once(':').ok_or("Invalid resource trailer")?;if k.is_empty()||!k.bytes().all(token)||v.bytes().any(|v|v<32&&v!=b'\t'||v==127){return Err("Invalid resource trailer syntax".into());}if ["content-type","content-encoding","content-length","transfer-encoding","location"].iter().any(|v|k.eq_ignore_ascii_case(v)){return Err("Resource representation or route metadata in trailers is unsupported".into());}}return Ok(());}at=at.checked_add(n).and_then(|v|v.checked_add(2)).filter(|v|*v<=data.len()).ok_or("Resource chunk bound invalid")?;}}

#[cfg(test)]mod checks{
 use super::*;
 fn http(mime:&str,body:&[u8])->Vec<u8>{let mut raw=format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n",body.len()).into_bytes();raw.extend_from_slice(body);raw}
 #[test]fn base_order_and_credential_free_targets(){let mut session=Session::new("<base href='../assets/'><link rel=stylesheet href='one.css?sig=%2f#f'><style>p{color:red}</style><link rel=stylesheet href='two.css'><img src='//else.invalid/a.png'>","https://source.invalid/docs/page",64*1024*1024).unwrap();let first=session.next().unwrap().unwrap();assert!(text(&first,"wire").starts_with("GET /assets/one.css?sig=%2f HTTP/1.1\r\n"));assert!(!text(&first,"wire").contains("Authorization"));assert!(!text(&first,"wire").contains("Cookie"));session.response(&http("text/css; charset=\"UTF-8\"",b"p{color:blue}")).unwrap();let second=session.next().unwrap().unwrap();assert!(text(&second,"wire").starts_with("GET /assets/two.css HTTP/1.1\r\n"));session.response(&http("text/css",b"p{color:green}")).unwrap();assert!(session.next().unwrap().is_none());assert!(session.ready());assert_eq!(session.bodies.len(),2);assert!(session.summary().contains("outside the approved"));assert!(session.summary().contains("2 credential-free GET requests"));}
 #[test]fn redirect_origin_loop_and_typed_refusal(){let mut session=Session::new("<link rel=stylesheet href='a.css'><img src='p.png'>","http://127.0.0.1:8123/page",64*1024*1024).unwrap();session.next().unwrap();session.response(b"HTTP/1.1 302 Found\r\nLocation: /assets/a.css\r\nContent-Length: 0\r\n\r\n").unwrap();assert!(text(&session.next().unwrap().unwrap(),"wire").starts_with("GET /assets/a.css "));session.response(b"HTTP/1.1 302 Found\r\nLocation: http://else.invalid/a.css\r\nContent-Length: 0\r\n\r\n").unwrap();let image=session.next().unwrap().unwrap();assert!(text(&image,"wire").starts_with("GET /p.png "));session.response(&http("text/html",b"challenge page")).unwrap();assert!(session.next().unwrap().is_none());assert!(session.bodies.is_empty());assert!(session.summary().contains("Content-Type must be image/png"));assert!(session.summary().contains("leaves the approved"));}
 #[test]fn representation_framing_charset_and_trailers(){for wire in [b"HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Type: text/css\r\nContent-Length: 0\r\n\r\n".as_slice(),b"HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Length: 9\r\n\r\nx",b"HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Encoding: gzip\r\nContent-Encoding: identity\r\nContent-Length: 0\r\n\r\n",b"HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Type: image/png\r\n\r\n"]{assert!(representation(wire).is_err());}let mut empty=Session::new("<link rel=stylesheet href='a'>","https://source.invalid/",1024*1024).unwrap();empty.next().unwrap();empty.response(b"HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n").unwrap();assert!(empty.bodies.is_empty());assert!(empty.summary().contains("Empty HTTP gzip"));let mut session=Session::new("<link rel=stylesheet href='a'>","https://source.invalid/",1024*1024).unwrap();session.next().unwrap();session.response(&http("text/css; charset=us-ascii","é".as_bytes())).unwrap();assert!(session.next().unwrap().is_none());assert!(session.bodies.is_empty());assert!(session.summary().contains("do not match"));}
 #[test]fn input_memory_limits_and_one_response_consumption(){let mut session=Session::new("<link rel=stylesheet href='a'><img src='p'>","https://source.invalid/",1024).unwrap();session.next().unwrap();session.response(&http("text/css",&vec![b'x';262145])).unwrap();session.next().unwrap();let mut png=b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();png.extend_from_slice(&u32::MAX.to_be_bytes());png.extend_from_slice(&u32::MAX.to_be_bytes());session.response(&http("image/png",&png)).unwrap();assert!(session.next().unwrap().is_none());assert!(session.bodies.is_empty());assert!(session.response(&[]).is_err());assert!(session.summary().contains("allowance"));let mut session=Session::new("<img src='p'>","https://source.invalid/",0).unwrap();assert!(session.next().unwrap().is_none());assert_eq!(session.requests,0);}
}

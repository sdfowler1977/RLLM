//! Typed, bounded text representations for approved HTTP page reads.
use super::*;
#[path="text_encoding.rs"] mod text_encoding;
#[derive(Debug)]pub(super) enum PageError{Response(String),Resource(String)}
impl PageError{pub(super) fn message(self)->String{match self{Self::Response(v)|Self::Resource(v)=>v}}}
impl From<&str> for PageError{fn from(v:&str)->Self{Self::Response(v.into())}}
impl From<String> for PageError{fn from(v:String)->Self{Self::Response(v)}}
/// Current-memory admission is performed before response parsing/decoding.
pub(super) fn admission(raw_len:usize,room:usize)->std::result::Result<usize,PageError>{if raw_len>2*1024*1024{return Err(PageError::Response("Page response exceeds its 2 MiB bound".into()));}let allowance=(room.saturating_sub(raw_len.saturating_mul(3))/4).min(2*1024*1024);if allowance==0{return Err(PageError::Resource("Current resource budget cannot admit page response decoding".into()));}Ok(allowance)}
fn token(b:u8)->bool{b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b)}
fn ows(v:&str)->&str{v.trim_matches([' ','\t'])}
fn content_type(value:&str)->Result<(String,Option<String>)>{
 if value.len()>4096||!value.is_ascii(){return Err("Page Content-Type exceeds its supported ASCII metadata bound".into());}
 let bytes=value.as_bytes();let mut i=0;let skip=|i:&mut usize|{while bytes.get(*i).is_some_and(|b|matches!(b,b' '|b'\t')){*i+=1;}};
 skip(&mut i);let start=i;while bytes.get(i).is_some_and(|b|token(*b)){i+=1;}if i==start||bytes.get(i)!=Some(&b'/'){return Err("Invalid page media type".into());}i+=1;let sub=i;while bytes.get(i).is_some_and(|b|token(*b)){i+=1;}if i==sub||i-start>128{return Err("Invalid or oversized page media type".into());}let media=value[start..i].to_ascii_lowercase();let mut names=std::collections::BTreeSet::new();let mut charset=None;
 loop{skip(&mut i);if i==bytes.len(){break;}if bytes[i]!=b';'{return Err("Invalid page media-type parameter separator".into());}i+=1;skip(&mut i);let start=i;while bytes.get(i).is_some_and(|b|token(*b)){i+=1;}if start==i{return Err("Missing page media-type parameter name".into());}let name=value[start..i].to_ascii_lowercase();if !names.insert(name.clone())||names.len()>16{return Err("Duplicate or excessive page media-type parameters".into());}skip(&mut i);if bytes.get(i)!=Some(&b'='){return Err("Page media-type parameter requires a value".into());}i+=1;skip(&mut i);
  let mut out=String::new();if bytes.get(i)==Some(&b'"'){i+=1;loop{let b=*bytes.get(i).ok_or("Unterminated page media-type parameter")?;i+=1;if b==b'"'{break;}let b=if b==b'\\'{let escaped=*bytes.get(i).ok_or("Incomplete page media-type quoted pair")?;i+=1;escaped}else{b};if b<32&&b!=b'\t'||b==127{return Err("Control character in page media-type parameter".into());}out.push(b as char);}}
  else{let start=i;while bytes.get(i).is_some_and(|b|token(*b)){i+=1;}if start==i{return Err("Missing page media-type parameter value".into());}out.push_str(&value[start..i]);}
  if name=="charset"{charset=Some(out.to_ascii_lowercase());}
 }
 Ok((media,charset))
}
struct Metadata{media:Option<String>,charset:Option<String>}
fn metadata(raw:&[u8])->Result<Metadata>{
 let mut rest=raw;
 for interim in 0..=4{
  let end=rest.windows(4).position(|w|w==b"\r\n\r\n").ok_or("Incomplete page HTTP headers")?;if end>65536{return Err("Page HTTP headers exceed limit".into());}let h=std::str::from_utf8(&rest[..end]).map_err(|_|"Invalid page HTTP headers")?;let mut lines=h.split("\r\n");let status=lines.next().ok_or("Missing page HTTP status")?;let fields=status.split(' ').collect::<Vec<_>>();if status.bytes().any(|b|b<32&&b!=b'\t'||b==127)||fields.len()<2||!matches!(fields[0],"HTTP/1.0"|"HTTP/1.1")||fields[1].len()!=3||!fields[1].bytes().all(|b|b.is_ascii_digit()){return Err("Invalid page HTTP status".into());}let code=fields[1].parse::<u16>().map_err(|_|"Invalid page status")?;
  if code==101{return Err("HTTP protocol upgrades are not a page representation".into());}if (100..200).contains(&code){if interim==4{return Err("Too many informational page responses".into());}rest=&rest[end+4..];continue;}
  let mut media=None;let mut charset=None;let mut coding=false;let mut chunked=false;let mut count=0;
  for line in lines{count+=1;if count>256{return Err("Page response has excessive header fields".into());}let(k,v)=line.split_once(':').ok_or("Invalid page HTTP field")?;if k.is_empty()||!k.bytes().all(token)||v.bytes().any(|b|b<32&&b!=b'\t'||b==127){return Err("Invalid page HTTP field syntax".into());}
   if k.eq_ignore_ascii_case("content-type"){if media.is_some(){return Err("Duplicate page Content-Type".into());}let(m,c)=content_type(ows(v))?;media=Some(m);charset=c;}
   if k.eq_ignore_ascii_case("content-encoding"){if coding{return Err("Duplicate page Content-Encoding".into());}coding=true;http_content_coding::coding(ows(v))?;}
   if k.eq_ignore_ascii_case("transfer-encoding"){chunked=ows(v).eq_ignore_ascii_case("chunked");}
  }
  if chunked{let data=&rest[end+4..];let mut p=0;loop{let line=data.get(p..).ok_or("Invalid page chunk offset")?.windows(2).position(|w|w==b"\r\n").ok_or("Incomplete page chunk")?;let size=std::str::from_utf8(&data[p..p+line]).map_err(|_|"Invalid page chunk size")?.split(';').next().unwrap_or("");let n=usize::from_str_radix(size.trim(),16).map_err(|_|"Invalid page chunk size")?;p=p.checked_add(line+2).ok_or("Page chunk offset overflow")?;
    if n==0{let trailer=std::str::from_utf8(&data[p..]).map_err(|_|"Invalid page trailers")?;for line in trailer.split("\r\n").filter(|v|!v.is_empty()){let(k,v)=line.split_once(':').ok_or("Invalid page trailer")?;if k.is_empty()||!k.bytes().all(token)||v.bytes().any(|b|b<32&&b!=b'\t'||b==127){return Err("Invalid page trailer syntax".into());}if ["content-type","content-encoding","content-length","transfer-encoding"].iter().any(|v|k.eq_ignore_ascii_case(v)){return Err("Page representation or framing metadata in trailers is unsupported".into());}}break;}
    p=p.checked_add(n).and_then(|v|v.checked_add(2)).filter(|v|*v<=data.len()).ok_or("Invalid page chunk bound")?;
  }}
  return Ok(Metadata{media,charset});
 }
 Err("Too many informational page responses".into())
}
struct Decoded {code:u16,body:String,html:bool,encoding:&'static str,media:Option<String>,gzip:bool}
fn decode(raw:&[u8],limit:usize,room:usize)->std::result::Result<Decoded,PageError>{
 let allowance=admission(raw.len(),room)?;
 // The existing authoritative decoder validates complete framing/status first.
 let(code,bytes)=network::decoded_coded_bytes(raw)?;let meta=metadata(raw)?;let gzip=http_content_coding::header(raw)?;
 let bytes=if gzip{http_content_coding::decode_classified(&bytes,true,allowance).map_err(|e|match e.kind{http_content_coding::ErrorKind::Resource=>PageError::Resource(e.message),http_content_coding::ErrorKind::OutputLimit if allowance<2*1024*1024=>PageError::Resource(e.message),_=>PageError::Response(e.message)})?}else{if bytes.len()>allowance{return Err(if allowance<2*1024*1024{PageError::Resource("Page representation exceeds current memory allowance".into())}else{PageError::Response("Page representation exceeds 2 MiB".into())});}bytes};
 let media=meta.media.as_deref();
 let legacy_allowed=media.is_some_and(|m|m.starts_with("text/")&&!m.ends_with("xml")&&!m.ends_with("json"));
 // This synchronous callback is owned by the current web ticket/generation.
 // The independent local-tool cancellation flag must not govern page decoding.
 let(body,encoding)=text_encoding::decode_classified(&bytes,meta.charset.as_deref(),media==Some("text/html"),legacy_allowed,limit,&||false).map_err(|e|match e{text_encoding::TextError::Response(v)=>PageError::Response(v),text_encoding::TextError::Resource(v)=>PageError::Resource(v)})?;
 let html=match meta.media.as_deref(){None=>body.trim_start().starts_with('<'),Some("text/html")=>true,Some(m) if m.starts_with("text/")||matches!(m,"application/json"|"application/xml"|"application/xhtml+xml")||m.starts_with("application/")&&(m.ends_with("+json")||m.ends_with("+xml"))=>false,Some(_)=>return Err("Page media type is not a supported text representation".into())};
 Ok(Decoded{code,body,html,encoding,media:meta.media,gzip})
}
pub(super) fn read(raw:&[u8])->Result<(u16,String)>{read_with_room(raw,usize::MAX)}
pub(super) fn read_with_room(raw:&[u8],room:usize)->Result<(u16,String)>{read_classified(raw,room).map_err(PageError::message)}
pub(super) fn read_classified(raw:&[u8],room:usize)->std::result::Result<(u16,String),PageError>{read_context_classified(raw,room,&Null)}
pub(super) fn read_context_classified(raw:&[u8],room:usize,owner:&Json)->std::result::Result<(u16,String),PageError>{
 let mut d=decode(raw,2*1024*1024,room)?;let mut program_note=String::new();if page_program::enabled(get(owner,"args"))?{if !d.html{return Err("Inline page programs require an HTML representation".into());}let(token,remaining)=page_program::web_limits(owner).map_err(program_error)?;let output=page_program::run(d.body.as_bytes(),room,&||page_program::web_cancelled(&token),remaining).map_err(program_error)?;d.body=output.html;program_note=output.summary;}let(code,body,html)=(d.code,d.body.as_str(),d.html);
 let kind=if html&&!program_note.is_empty(){"owned inline-program DOM extraction"}else if html{"HTML static extraction"}else{"literal text data (no HTML interpretation)"};let declared=d.media.as_deref().unwrap_or("absent; legacy leading-markup heuristic retained");let text=if html{investigation::html_page_with_room(body,room)?}else{let text=investigation::byte_limit(body,98304);if text.len()<body.len(){format!("{text}\n[Text evidence abbreviated at a UTF-8 boundary; source has {} UTF-8 bytes.]",body.len())}else{text}};
 let mut output=format!("Response representation: Content-Type {declared}; decoding {}; content coding {}; {kind}. Typed HTML byte-order marks take precedence; otherwise absent HTTP charset uses the harness UTF-8 default. In-document encoding declarations and general browser encoding detection are not evaluated.\n{text}",d.encoding,if d.gzip{"gzip"}else{"identity"});if !program_note.is_empty(){output.push_str(&format!("\n{program_note}"));}Ok((code,output))
}

pub(super) fn capture_html(raw:&[u8])->Result<String>{capture_html_with_room(raw,usize::MAX)}
pub(super) fn capture_html_with_room(raw:&[u8],room:usize)->Result<String>{capture_html_classified(raw,room).map_err(PageError::message)}
pub(super) fn capture_html_classified(raw:&[u8],room:usize)->std::result::Result<String,PageError>{let d=decode(raw,262144,room)?;if !d.html{return Err("Remote capture requires an HTML representation, not literal text data".into());}if d.body.len()>262144{return Err("Remote HTML capture exceeds 256 KiB".into());}Ok(d.body)}

fn program_error(e:page_program::Error)->PageError{match e{page_program::Error::Input(v)=>PageError::Response(v),page_program::Error::Resource(v)=>PageError::Resource(v)}}
pub(super) fn capture_context_classified(raw:&[u8],room:usize,owner:&Json)->std::result::Result<(String,String),PageError>{let html=capture_html_classified(raw,room)?;if !page_program::enabled(get(owner,"args"))?{return Ok((html,String::new()));}let(token,remaining)=page_program::web_limits(owner).map_err(program_error)?;let output=page_program::run(html.as_bytes(),room,&||page_program::web_cancelled(&token),remaining).map_err(program_error)?;Ok((output.html,output.summary))}

pub(super) fn response_room(st:&Json)->usize{let bytes=crate::mcp_schema::state_bytes(st);let policy=crate::resources::policy(get(st,"device"),get(st,"resources"),bytes);number(&policy,"budget_bytes").saturating_sub(bytes.saturating_mul(3))}

#[cfg(test)]mod classification_checks{use super::*;
#[test]fn current_memory_and_response_failures_are_typed(){
 assert!(matches!(read_classified(b"not HTTP",0),Err(PageError::Resource(_))));
 assert!(matches!(read_classified(b"not HTTP",usize::MAX),Err(PageError::Response(_))));
 let data=[31,139,8,0,0,0,0,0,2,3,115,116,28,5,35,13,0,0,143,51,129,216,244,1,0,0];let mut raw=format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",data.len()).into_bytes();raw.extend(data);
 let constrained=raw.len()*3+4*64;assert!(matches!(read_classified(&raw,constrained),Err(PageError::Resource(_))));assert!(read_classified(&raw,usize::MAX).is_ok());
 let over=vec![0;2*1024*1024+1];assert!(matches!(read_classified(&over,usize::MAX),Err(PageError::Response(_))));
}
#[test]fn declared_text_resource_and_input_errors_are_typed(){assert!(matches!(text_encoding::decode_classified(b"A",None,false,true,100,&||true),Err(text_encoding::TextError::Resource(_))));assert!(matches!(text_encoding::decode_classified(b"\xff",None,false,true,100,&||false),Err(text_encoding::TextError::Response(_))));}
}

//! Original bounded HTTP(S) reference resolver, not a complete WHATWG parser.
//! RFC relative merging plus WHATWG percent-dot path segments. No networking,
//! credentials, IDNA conversion, legacy numeric IPv4 or backslash rewriting.
use std::{fmt,net::{Ipv4Addr,Ipv6Addr}};
const INPUT_LIMIT:usize=8192;
const SERIALIZED_LIMIT:usize=8192;
const SEGMENT_LIMIT:usize=4096;
#[derive(Clone,Copy,Debug,PartialEq,Eq,Hash)]
pub enum Scheme{Http,Https}
impl Scheme{pub fn as_str(self)->&'static str{match self{Self::Http=>"http",Self::Https=>"https"}}pub fn default_port(self)->u16{match self{Self::Http=>80,Self::Https=>443}}}
#[derive(Clone,Debug,PartialEq,Eq,Hash)]
pub enum Host{Dns(String),Ipv4(Ipv4Addr),Ipv6(Ipv6Addr)}
impl Host{
    /// Host for DNS/TCP use: IPv6 has no surrounding brackets.
    pub fn address(&self)->String{match self{Self::Dns(n)=>n.clone(),Self::Ipv4(n)=>n.to_string(),Self::Ipv6(n)=>ipv6_text(*n)}}
    pub fn authority_host(&self)->String{match self{Self::Ipv6(n)=>format!("[{}]",ipv6_text(*n)),_=>self.address()}}
}
#[derive(Clone,Debug,PartialEq,Eq,Hash)]
pub struct Origin{pub scheme:Scheme,pub host:Host,pub port:u16}
impl fmt::Display for Origin{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"{}://{}",self.scheme.as_str(),self.host.authority_host())?;if self.port!=self.scheme.default_port(){write!(f,":{}",self.port)?;}Ok(())}}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Url{scheme:Scheme,host:Host,port:u16,path:String,query:Option<String>,fragment:Option<String>}
impl Url{
    pub fn origin(&self)->Origin{Origin{scheme:self.scheme,host:self.host.clone(),port:self.port}}
    pub fn same_origin(&self,other:&Self)->bool{self.scheme==other.scheme&&self.port==other.port&&self.host==other.host}
    /// Origin-form HTTP target. Fragments never leave the client.
    pub fn request_target(&self)->String{let mut target=self.path.clone();if let Some(query)=&self.query{target.push('?');target.push_str(query);}target}
    /// Absolute URL accepted by the existing HTTP(S) transport, without fragment.
    pub fn wire_url(&self)->String{format!("{}{}",self.origin(),self.request_target())}
    pub fn serialize(&self)->String{let mut value=self.wire_url();if let Some(fragment)=&self.fragment{value.push('#');value.push_str(fragment);}value}
}
impl fmt::Display for Url{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str(&self.serialize())}}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct UrlError{pub code:&'static str,pub offset:usize}
impl fmt::Display for UrlError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"{} at input byte {}",self.code,self.offset)}}
impl std::error::Error for UrlError{}
type Result<T>=std::result::Result<T,UrlError>;
fn fail(code:&'static str,offset:usize)->UrlError{UrlError{code,offset}}
fn validate_input(value:&str)->Result<()>{
    if value.len()>INPUT_LIMIT{return Err(fail("url-input-length-bound",INPUT_LIMIT));}
    if value.starts_with(' ')||value.ends_with(' '){return Err(fail("surrounding-space-not-supported",0));}
    for (at,c) in value.char_indices(){if c.is_control(){return Err(fail("url-control-character",at));}if c=='\\'{return Err(fail("backslash-url-not-supported",at));}}
    Ok(())
}
fn ipv6_text(value:Ipv6Addr)->String{
    let words=value.segments();let(mut best_start,mut best_len)=(0,0);let mut p=0;
    while p<8{if words[p]!=0{p+=1;continue;}let begin=p;while p<8&&words[p]==0{p+=1;}if p-begin>best_len{best_start=begin;best_len=p-begin;}}
    if best_len<2{return words.iter().map(|n|format!("{n:x}")).collect::<Vec<_>>().join(":");}
    let left=words[..best_start].iter().map(|n|format!("{n:x}")).collect::<Vec<_>>().join(":");let right=words[best_start+best_len..].iter().map(|n|format!("{n:x}")).collect::<Vec<_>>().join(":");format!("{left}::{right}")
}
fn host(value:&str,offset:usize)->Result<Host>{
    if value.is_empty(){return Err(fail("url-host-empty",offset));}
    if !value.is_ascii(){return Err(fail("international-host-idna-not-supported",offset));}
    if value.contains('%'){return Err(fail("percent-encoded-host-not-supported",offset));}
    let lower=value.to_ascii_lowercase();let clean=lower.strip_suffix('.').unwrap_or(&lower);
    if clean.is_empty()||clean.len()>253{return Err(fail("dns-host-length-invalid",offset));}
    if lower.bytes().all(|b|b.is_ascii_digit()||b==b'.'){
        return clean.parse::<Ipv4Addr>().map(Host::Ipv4).map_err(|_|fail("legacy-or-invalid-ipv4-not-supported",offset));
    }
    let last=clean.rsplit('.').next().unwrap_or("");
    if last.bytes().all(|b|b.is_ascii_digit())||last.strip_prefix("0x").map(|s|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_hexdigit())).unwrap_or(false){return Err(fail("legacy-or-ambiguous-numeric-host-not-supported",offset));}
    for label in clean.split('.'){
        if label.is_empty()||label.len()>63||label.starts_with('-')||label.ends_with('-')||!label.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_')){return Err(fail("dns-host-label-invalid",offset));}
    }
    Ok(Host::Dns(lower))
}
fn authority(value:&str,scheme:Scheme,offset:usize)->Result<(Host,u16)>{
    if value.contains('@'){return Err(fail("url-userinfo-forbidden",offset));}
    let (host,port_text)=if value.starts_with('['){
        let end=value.find(']').ok_or(fail("ipv6-bracket-missing",offset))?;let address=&value[1..end];
        if address.contains('%'){return Err(fail("ipv6-zone-id-not-supported",offset+1));}
        let ip=address.parse::<Ipv6Addr>().map_err(|_|fail("ipv6-address-invalid",offset+1))?;
        let tail=&value[end+1..];let port=if tail.is_empty(){None}else{Some(tail.strip_prefix(':').ok_or(fail("ipv6-authority-suffix-invalid",offset+end+1))?)};
        (Host::Ipv6(ip),port)
    }else{
        if value.contains(['[',']']){return Err(fail("ip-bracket-invalid",offset));}
        let (name,port)=match value.split_once(':'){Some((name,port))=>{if port.contains(':'){return Err(fail("ipv6-authority-requires-brackets",offset));}(name,Some(port))},None=>(value,None)};
        (host(name,offset)?,port)
    };
    let port=if let Some(text)=port_text{
        if text.is_empty()||text.len()>5||!text.bytes().all(|b|b.is_ascii_digit()){return Err(fail("url-port-invalid",offset));}
        text.parse::<u16>().ok().filter(|p|*p!=0).ok_or(fail("url-port-invalid",offset))?
    }else{scheme.default_port()};Ok((host,port))
}
#[derive(Clone,Copy)]enum Component{Path,Query,Fragment}
fn encode(value:&str,component:Component,offset:usize)->Result<String>{
    let bytes=value.as_bytes();let mut out=String::new();let mut p=0;const HEX:&[u8;16]=b"0123456789ABCDEF";
    while p<bytes.len(){let b=bytes[p];if b==b'%'{
        if p+2>=bytes.len()||!bytes[p+1].is_ascii_hexdigit()||!bytes[p+2].is_ascii_hexdigit(){return Err(fail("invalid-percent-triplet",offset+p));}
        // Retain existing query/signature spelling and encoded delimiters.
        out.push('%');out.push(bytes[p+1]as char);out.push(bytes[p+2]as char);p+=3;continue;
    }
    let allowed=b.is_ascii_alphanumeric()||b"-._~!$&'()*+,;=:@/".contains(&b)||match component{Component::Path=>false,Component::Query=>b"?[]".contains(&b),Component::Fragment=>b"?[]#".contains(&b)};
    if allowed{out.push(b as char);}else{out.push('%');out.push(HEX[(b>>4)as usize]as char);out.push(HEX[(b&15)as usize]as char);}p+=1;
    if out.len()>SERIALIZED_LIMIT{return Err(fail("encoded-url-length-bound",offset+p));}}
    Ok(out)
}
fn dot(segment:&str)->usize{
    if segment=="."||segment.eq_ignore_ascii_case("%2e"){1}
    else if segment==".."||segment.eq_ignore_ascii_case(".%2e")||segment.eq_ignore_ascii_case("%2e.")||segment.eq_ignore_ascii_case("%2e%2e"){2}else{0}
}
fn normalize_path(path:&str)->Result<String>{
    if !path.starts_with('/'){return Err(fail("internal-path-not-rooted",0));}
    let mut stack=Vec::new();let mut parts=path[1..].split('/').peekable();let mut count=0;
    while let Some(segment)=parts.next(){count+=1;if count>SEGMENT_LIMIT{return Err(fail("url-segment-count-bound",0));}
        match dot(segment){1=>{if parts.peek().is_none(){stack.push("");}},2=>{stack.pop();if parts.peek().is_none(){stack.push("");}},_=>stack.push(segment)}
    }Ok(format!("/{}",stack.join("/")))
}
struct Suffix<'a>{path:&'a str,query:Option<&'a str>,fragment:Option<&'a str>,query_at:usize,fragment_at:usize}
fn suffix(value:&str)->Suffix<'_>{
    let (body,fragment,fragment_at)=match value.find('#'){Some(at)=>(&value[..at],Some(&value[at+1..]),at+1),None=>(value,None,value.len())};
    let(path,query,query_at)=match body.find('?'){Some(at)=>(&body[..at],Some(&body[at+1..]),at+1),None=>(body,None,body.len())};
    Suffix{path,query,fragment,query_at,fragment_at}
}
fn complete(url:Url)->Result<Url>{if url.serialize().len()>SERIALIZED_LIMIT{return Err(fail("resolved-url-length-bound",0));}Ok(url)}
fn scheme_prefix(value:&str)->Result<Option<(Scheme,usize)>>{
    let cutoff=value.find(['/','?','#']).unwrap_or(value.len());
    if let Some(at)=value[..cutoff].find(':'){
        let scheme=&value[..at];if scheme.is_empty()||!scheme.as_bytes()[0].is_ascii_alphabetic()||!scheme.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'+'|b'-'|b'.')){return Err(fail("url-scheme-invalid",0));}
        let kind=if scheme.eq_ignore_ascii_case("http"){Scheme::Http}else if scheme.eq_ignore_ascii_case("https"){Scheme::Https}else{return Err(fail("unsupported-url-scheme",0));};return Ok(Some((kind,at+1)));
    }Ok(None)
}
fn with_authority(scheme:Scheme,rest:&str,offset:usize)->Result<Url>{
    let end=rest.find(['/','?','#']).unwrap_or(rest.len());let(host,port)=authority(&rest[..end],scheme,offset)?;
    let tail=suffix(&rest[end..]);let path=if tail.path.is_empty(){"/".into()}else{normalize_path(&encode(tail.path,Component::Path,offset+end)?)?};
    complete(Url{scheme,host,port,path,query:tail.query.map(|v|encode(v,Component::Query,offset+end+tail.query_at)).transpose()?,fragment:tail.fragment.map(|v|encode(v,Component::Fragment,offset+end+tail.fragment_at)).transpose()?})
}
pub fn parse(value:&str)->Result<Url>{
    validate_input(value)?;let(scheme,after)=scheme_prefix(value)?.ok_or(fail("absolute-http-url-required",0))?;
    let rest=value[after..].strip_prefix("//").ok_or(fail("http-authority-slashes-required",after))?;with_authority(scheme,rest,after+2)
}
/// Resolve against the actual final retrieval URL supplied by the caller.
/// This routine does not select HTML base elements or inherit redirect fragments.
pub fn resolve(base:&Url,reference:&str)->Result<Url>{
    validate_input(reference)?;if scheme_prefix(reference)?.is_some(){return parse(reference);}
    if let Some(rest)=reference.strip_prefix("//"){return with_authority(base.scheme,rest,2);}
    let tail=suffix(reference);let fragment=tail.fragment.map(|v|encode(v,Component::Fragment,tail.fragment_at)).transpose()?;
    let path=if tail.path.is_empty(){base.path.clone()}else{
        let encoded=encode(tail.path,Component::Path,0)?;
        let merged=if encoded.starts_with('/'){encoded}else{let end=base.path.rfind('/').unwrap_or(0);format!("{}{}",&base.path[..end+1],encoded)};normalize_path(&merged)?
    };
    let query=if let Some(query)=tail.query{Some(encode(query,Component::Query,tail.query_at)?)}else if tail.path.is_empty(){base.query.clone()}else{None};
    complete(Url{scheme:base.scheme,host:base.host.clone(),port:base.port,path,query,fragment})
}

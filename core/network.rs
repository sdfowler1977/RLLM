use super::*;
#[derive(Clone)]
pub struct Url {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}
pub fn url(text: &str, plain: bool) -> Result<Url> {
    if text.chars().any(|c| c.is_control() || c == ' ') {
        return Err("Address contains spaces or control characters".into());
    }
    let (tls, rest) = if let Some(v) = text.strip_prefix("https://") {
        (true, v)
    } else if let Some(v) = text.strip_prefix("http://") {
        if !plain {
            return Err(
                "HTTP is disabled. Enable unencrypted access explicitly for a trusted LAN.".into(),
            );
        }
        (false, v)
    } else {
        return Err("Address must begin with https:// or http://".into());
    };
    let cut = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..cut];
    let path = if cut < rest.len() {
        if rest.as_bytes()[cut]==b'?' {format!("/{}",&rest[cut..])}else{rest[cut..].into()}
    } else {
        "/".into()
    };
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('?')
        || text.contains('#')
    {
        return Err("Invalid address; credentials belong in their own fields".into());
    }
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']').ok_or("Invalid IPv6 address")?;
        let tail = &authority[end + 1..];
        (
            authority[1..end].to_string(),
            if tail.is_empty() {
                None
            } else {
                Some(tail.strip_prefix(':').ok_or("Invalid port")?)
            },
        )
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        }
    };
    let port = match port {
        Some(p) => p.parse::<u16>().map_err(|_| "Invalid port")?,
        None => {
            if tls {
                443
            } else {
                80
            }
        }
    };
    if port == 0 || host.is_empty() {
        return Err("Invalid host or port".into());
    }
    Ok(Url {
        tls,
        host,
        port,
        path,
    })
}
fn public_ip(ip:std::net::IpAddr)->bool{
    match ip{
        std::net::IpAddr::V4(ip)=>!matches!(ip.octets(),
            [0|10|127|224..=255, ..] | [100,64..=127, ..] | [169,254, ..] |
            [172,16..=31, ..] | [192,168, ..] | [192,0,0|2,_] | [192,88,99,_] |
            [198,18|19, ..] | [198,51,100,_] | [203,0,113,_]),
        std::net::IpAddr::V6(ip)=>{
            let octets=ip.octets();
            // The well-known DNS64 prefix may carry only a public IPv4 target.
            if octets[..12]==[0,0x64,0xff,0x9b,0,0,0,0,0,0,0,0]{
                let [a,b,c,d]=octets[12..].try_into().unwrap();
                return public_ip(std::net::Ipv4Addr::new(a,b,c,d).into());
            }
            let [first,second,..]=ip.segments();
            matches!(first,0x2000..=0x3fff)
                && !matches!(first,0x2002|0x3fff)
                && !(first==0x2001&&(second<=0x01ff||second==0x0db8))
        }
    }
}

pub fn public_web_url(target:&str)->bool{
    let Ok(url)=url(target,false) else{return false;};
    if url.port!=443||target.len()>2048||target.contains('\\'){return false;}
    let path=url.path.split('?').next().unwrap_or("");
    if path.split('/').any(|part|matches!(part,"."|".."))||path.contains(';'){return false;}
    if path.as_bytes().windows(3).any(|part|{
        if part[0]!=b'%' {return false;}
        let Ok(hex)=std::str::from_utf8(&part[1..])else{return true;};
        match u8::from_str_radix(hex,16){
            Ok(byte)=>byte.is_ascii_alphanumeric()||b"./\\%_-~;".contains(&byte),
            Err(_)=>true,
        }
    }){return false;}
    if url.path.split_once('?').is_some_and(|(_,query)|query.split(['&',';']).any(|field|{
        let key=field.split('=').next().unwrap_or("").to_ascii_lowercase();
        let normalized=key.replace(['_','-'],"");
        key.contains('%') || matches!(normalized.as_str(),
            "apikey"|"accesstoken"|"authtoken"|"idtoken"|"refreshtoken"|"sessiontoken"|"auth"|"authorization"|
            "clientsecret"|"credential"|"key"|"password"|"secret"|"session"|"sessionid"|"signature"|"sig"|"token")
            || key.starts_with("x-amz-") || key.starts_with("x-goog-")
    })){return false;}
    if let Ok(ip)=url.host.parse(){return public_ip(ip);}
    let host=url.host.trim_end_matches('.').to_ascii_lowercase();
    let Some((_,suffix))=host.rsplit_once('.')else{return false;};
    host.len()<=253 && suffix.bytes().any(|b|b.is_ascii_alphabetic())
        && !matches!(suffix,"localhost"|"local"|"internal"|"lan"|"onion")
        && !host.ends_with(".home.arpa")
        && host.split('.').all(|label|!label.is_empty()&&label.len()<=63&&!label.starts_with('-')&&!label.ends_with('-')
            && label.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'))
}

/// The platform must connect to the same numeric address after this check.
pub fn public_web_address(input:&Json)->Result<Json>{
    use std::net::{IpAddr,Ipv4Addr,Ipv6Addr};
    let value=field(input,"address")?;
    if value.len()>24{return Err("Invalid resolved public web address".into());}
    let bytes=unbase64(value)?;
    let ip=match bytes.as_slice(){
        [a,b,c,d]=>IpAddr::V4(Ipv4Addr::new(*a,*b,*c,*d)),
        raw if raw.len()==16=>IpAddr::V6(Ipv6Addr::from(<[u8;16]>::try_from(raw).map_err(|_|"Invalid IPv6 address")?)),
        _=>return Err("Invalid resolved public web address".into()),
    };
    if !public_ip(ip){return Err("Automatic page read stopped because the site resolved to a private or reserved address".into());}
    Ok(Bool(true))
}
pub fn encoded(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char)
        } else {
            out.push_str(&format!("%{b:02X}"))
        }
    }
    out
}
pub(crate) fn field_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}
pub(crate) fn transport_header(name: &str) -> bool {
    ["host", "connection", "content-length", "transfer-encoding", "accept-encoding",
     "te", "trailer", "expect", "upgrade", "proxy-connection", "user-agent"].contains(&name.to_ascii_lowercase().as_str())
}
pub fn credential_header(name:&str,prefix:&str,key:&str)->Result<(String,String)>{
    if name.len()>64||!field_name(name)||transport_header(name)||name.eq_ignore_ascii_case("content-type")
        ||prefix.len()>64||key.chars().chain(prefix.chars()).any(char::is_control){return Err("Invalid authentication header or prefix".into());}
    Ok((name.into(),if prefix.is_empty(){key.into()}else{format!("{prefix} {key}")}))
}
pub(crate) fn bearer_token(token:&str)->bool{let base=token.trim_end_matches('=');!base.is_empty()&&base.bytes().all(|b|b.is_ascii_alphanumeric()||b"-._~+/".contains(&b))}
fn strip_prefix_ci<'a>(value:&'a str,prefix:&str)->Option<&'a str>{
 let bytes=value.as_bytes();
 if bytes.len()<prefix.len()||!bytes[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()){return None;}
 Some(&value[prefix.len()..])
}
/// Strip one layer of matching quotes from a pasted credential.
///
/// The text outside the closing quote is kept, so a scheme-only paste
/// ("Bearer ") still carries the separator that tells it apart from a token.
fn unquote_credential(value:&str)->&str{
 let end=value.trim_end();
 if end.len()>=2{
  let bytes=end.as_bytes();let first=bytes[0];let last=bytes[bytes.len()-1];
  if (first==b'"'&&last==b'"')||(first==b'\''&&last==b'\''){return &value[1..end.len()-1];}
 }
 value
}
/// Recognise the bearer scheme in a pasted credential.
///
/// The scheme is only a prefix when a separator follows it, so a credential
/// that is exactly the word "Bearer" stays the token it legally is. A pasted
/// header line has already proved the intent, so there the scheme word alone
/// is a scheme with no token behind it.
fn bearer_scheme<'a>(value:&'a str,header_line:bool)->Option<&'a str>{
 let rest=strip_prefix_ci(value.trim_start(),"bearer")?;
 if rest.is_empty(){return if header_line{Some(rest)}else{None};}
 if rest.as_bytes()[0].is_ascii_whitespace(){return Some(rest);}
 None
}
/// One canonical MCP bearer credential.
///
/// A pasted credential arrives in the shapes a clipboard or a copied header
/// line produces: bare, padded, quoted, scheme-prefixed, or prefixed with the
/// header name itself. The normaliser returns the bare RFC 6750 b64token that
/// every MCP request then carries, or no credential at all. A value that cannot
/// be a token is refused here instead of being sent. A blank value is no
/// credential, not a malformed one.
pub(crate) fn mcp_bearer_token(raw:&str)->Result<Option<String>>{
 if raw.len()>8192||raw.chars().any(char::is_control){return Err("Invalid MCP bearer token format.".into());}
 if raw.trim().is_empty(){return Ok(None);}
 // The trailing separator is kept while the scheme is recognised, so a
 // scheme-only paste stays apart from a token that happens to be the word
 // "Bearer".
 let mut value=unquote_credential(raw.trim_start());
 let mut header_line=false;
 for _ in 0..12{
  value=unquote_credential(value);
  if let Some(rest)=strip_prefix_ci(value.trim_start(),"authorization:"){
   value=rest.trim_start();
   header_line=true;
   continue;
  }
  if let Some(rest)=bearer_scheme(value,header_line){
   value=rest.trim_start();
   continue;
  }
  break;
 }
 let token=value.trim();
 if token.is_empty()||!bearer_token(token){return Err("Invalid MCP bearer token format.".into());}
 Ok(Some(token.to_string()))
}
/// The canonical Authorization header for an MCP credential.
///
/// The header name is a fixed literal so a caller can push it into the
/// request builder the same way it pushes its own literal header names.
pub(crate) fn mcp_authorization_header(token:&str)->Result<(&'static str,String)>{
 let (_name,value)=credential_header("Authorization","Bearer",token)?;
 Ok(("Authorization",value))
}
/// The status the product's own decoder refused a response for, when the
/// refusal was the status.
///
/// A diagnostic may name only this status. The decoder refuses a head it
/// cannot read before it reads a status, so a status the framing rules
/// rejected never becomes a number a product message could name.
pub(crate) fn mcp_refused_status(error:&str)->Option<u16>{
 let rest=error.strip_prefix("HTTP ")?;
 let digits=rest.as_bytes().iter().take_while(|byte|byte.is_ascii_digit()).count();
 if digits==0{return None;}
 let code:u16=rest[..digits].parse().ok()?;
 let status_refusal=match &rest[digits..]{
  ". Check server address, credentials, model and permissions."=>true,
  ": redirects are not followed; enter the final server address"=>true,
  ": server returned representation metadata."=>true,
  _=>false,
 };
 if status_refusal{Some(code)}else{None}
}
/// The header block a peer answered with, read with the product's own
/// response rules.
///
/// The reader skips the informational replies the product's decoder skips, so
/// a challenge is read from the same head the decoder judged. A head the
/// response rules reject yields no head at all, never a guessed one.
pub(crate) fn mcp_response_head(raw:&[u8])->Option<&str>{
 let mut start=0;
 for _ in 0..=4{
  let relative=raw.get(start..)?.windows(4).position(|window|window==b"\r\n\r\n")?;
  if relative>65536{return None;}
  let end=start+relative;
  let head=std::str::from_utf8(&raw[start..end]).ok()?;
  let code=response_status(head.lines().next()?).ok()?;
  if (100..200).contains(&code){start=end+4;continue;}
  return Some(head);
 }
 None
}
pub(crate) fn storage_oauth_context(connection:&Json)->Json{
    Obj(["url","tls_name","plain","oauth"].into_iter().map(|key|(key.into(),connection.get(key).unwrap_or(&Null).clone())).collect())
}
pub fn storage_authorization(connection:&Json)->Result<Option<(String,String)>>{
    let oauth=match connection.get("oauth_enabled"){Ok(Bool(value))=>*value,Ok(Null)|Err(_)=>false,_=>return Err("Network storage authorization mode must be a toggle".into())};
    if oauth{
        let tokens=connection.get("oauth_tokens").map_err(|_|"Sign in through network storage authorization first")?;
        if tokens.get("storage_connection")!=Ok(&storage_oauth_context(connection)){return Err("Network storage authorization destination changed; sign in again".into());}
        if tokens.get("refresh_uncertain")==Ok(&Bool(true)){return Err("The previous storage authorization refresh outcome is unconfirmed; sign in again".into());}
        let token=tokens.get("access_token").and_then(Json::text).map_err(|_|"Network storage authorization has no access token")?;
        if token.len()>8192||!bearer_token(token){return Err("Network storage authorization returned an invalid access token".into());}
        let expiry=match tokens.get("expires_at"){Ok(Num(v))=>v.parse::<u64>().map_err(|_|"Network storage token lifetime is invalid")?,_=>return Err("Network storage token lifetime is invalid".into())};
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"System clock is before the supported epoch")?.as_secs();
        if expiry>0&&now.saturating_add(15)>=expiry{return Err("Network storage authorization has expired; refresh or sign in from storage settings".into());}
        return credential_header("Authorization","Bearer",token).map(Some);
    }
    fn setting<'a>(connection:&'a Json,key:&str)->Result<&'a str>{match connection{Null=>Ok(""),Obj(fields)=>match fields.get(key){None|Some(Null)=>Ok(""),Some(Str(value))=>Ok(value),_=>Err(format!("Network storage {key} must be text"))},_=>Err("Network storage connection must be an object".into())}}
    let key=setting(connection,"key")?;
    if !key.is_empty(){
        if key.len()>8192{return Err("Network storage access key exceeds8192bytes".into());}
        let name=setting(connection,"auth_header")?.trim();let name=if name.is_empty(){"Authorization"}else{name};
        let prefix=if connection.get("auth_prefix").is_ok_and(|v|v!=&Null){setting(connection,"auth_prefix")?.trim()}else{"Bearer"};
        return credential_header(name,prefix,key).map(Some);
    }
    let user=setting(connection,"user")?;if user.is_empty(){return Ok(None);}
    Ok(Some(("Authorization".into(),format!("Basic {}",b64(format!("{}:{}",user,setting(connection,"password")?).as_bytes())))))
}
pub fn b64(bytes: &[u8]) -> String {
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in bytes.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | (c.get(2).copied().unwrap_or(0) as u32);
        for i in 0..4 {
            if i > c.len() {
                o.push('=')
            } else {
                o.push(table[((n >> (18 - 6 * i)) & 63) as usize] as char)
            }
        }
    }
    o
}
pub fn unbase64(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut n = 0u32;
    let mut bits = 0;
    for b in text.bytes() {
        if b == b'=' {
            break;
        }
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("Invalid encoded response".into()),
        };
        n = (n << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((n >> bits) as u8);
        }
    }
    Ok(out)
}
/// Media chunks carry exact bytes. Reject ignored suffixes and unused bits,
/// while retaining valid padded and unpadded forms.
pub(super) fn unbase64_media_chunk(text: &str) -> Result<Vec<u8>> {
    let bytes = unbase64(text)?;
    let canonical = b64(&bytes);
    if text != canonical && text != canonical.trim_end_matches('=') {
        return Err("Invalid Base64 media chunk".into());
    }
    Ok(bytes)
}
pub fn wire(
    address: &str,
    plain: bool,
    method: &str,
    body: &str,
    headers: Vec<(&str, String)>,
) -> Result<Json> {
    let u = url(address, plain)?;
    if body.len() > 4 * 1024 * 1024 {
        return Err("Request exceeds 4 MiB".into());
    }
    if !["GET", "POST", "PUT", "PROPFIND", "MKCOL", "DELETE"].contains(&method) {
        return Err("Unsupported HTTP method".into());
    }
    let host = if u.host.contains(':') {
        format!("[{}]", u.host)
    } else {
        u.host.clone()
    };
    let mut wire=format!("{} {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nAccept-Encoding: identity\r\nContent-Length: {}\r\n",method,u.path,host,u.port,body.len());
    let mut names = std::collections::BTreeSet::new();
    for (name, value) in headers {
        if value.chars().any(|c| c.is_control()) || !field_name(name) || transport_header(name)
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err("Invalid HTTP header".into());
        }
        wire.push_str(&format!("{name}: {value}\r\n"));
    }
    // The client owns User-Agent so every outbound request identifies this product and a caller cannot omit or replace it.
    wire.push_str("User-Agent: RLLM web reader\r\n");
    wire.push_str("\r\n");
    wire.push_str(body);
    Ok(obj(vec![
        ("kind", s("network")),
        ("host", s(&u.host)),
        ("port", Num(u.port.to_string())),
        ("tls", Bool(u.tls)),
        ("wire", s(&wire)),
    ]))
}
/// An explicitly selected HTTPS identity may differ from its VPN routing address.
/// The platform still validates the ordinary trust chain and this DNS identity.
pub fn tls_identity(effect: &mut Json, name: &str) -> Result<()> {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() { return Ok(()); }
    if effect.get("tls")?!=&Bool(true) { return Err("A certificate hostname requires HTTPS; clear it before using HTTP".into()); }
    if name.len()>253 || name.parse::<std::net::IpAddr>().is_ok() || name.split('.').any(|label| {
        label.is_empty() || label.len()>63 || label.starts_with('-') || label.ends_with('-') ||
        !label.bytes().all(|b| b.is_ascii_alphanumeric() || b==b'-')
    }) { return Err("Enter only the DNS hostname on the server certificate, without scheme, port, path or wildcard".into()); }
    let old_host=field(effect,"host")?.to_string();
    let port=match effect.get("port")?{Num(n)=>n.parse::<u16>().map_err(|_|"Invalid request port")?,_=>return Err("Missing request port".into())};
    let old_host=if old_host.contains(':'){format!("[{old_host}]")}else{old_host};
    let old=format!("\r\nHost: {}:{}\r\n",old_host,port);
    let replacement=format!("\r\nHost: {}:{}\r\n",name,port);
    let binary=effect.get("wire_base64").is_ok_and(|v|v!=&Null);
    if binary&&effect.get("wire").is_ok_and(|v|v!=&Null){return Err("Request framing is ambiguous; no TLS identity was applied".into());}
    let wire=if binary{unbase64(field(effect,"wire_base64")?)?}else{field(effect,"wire")?.as_bytes().to_vec()};
    let end=wire.windows(4).position(|v|v==b"\r\n\r\n").filter(|n|*n<=65536).ok_or("Request headers are incomplete; no TLS identity was applied")?+4;
    let header=std::str::from_utf8(&wire[..end]).map_err(|_|"Request headers are invalid; no TLS identity was applied")?;
    if header.matches(&old).count()!=1{return Err("Request authority is missing or duplicated; no TLS identity was applied".into());}
    let mut updated=header.replacen(&old,&replacement,1).into_bytes();updated.extend_from_slice(&wire[end..]);
    let value=if binary{b64(&updated)}else{String::from_utf8(updated).map_err(|_|"Request text is invalid; no TLS identity was applied")?};
    let Obj(fields)=effect else{return Err("Invalid request effect".into())};
    fields.insert(if binary{"wire_base64"}else{"wire"}.into(),s(&value));
    fields.insert("tls_host".into(),s(&name));
    Ok(())
}
pub fn decoded_bytes(bytes: &[u8]) -> Result<(u16, Vec<u8>)> {
    decoded_bytes_with_interim(bytes, 0, 0, false, false)
}
pub(super) fn response_status(line: &str) -> Result<u16> {
    let bytes = line.as_bytes();
    if bytes.len() < 13 || (!line.starts_with("HTTP/1.0 ") && !line.starts_with("HTTP/1.1 "))
        || !bytes[9..12].iter().all(u8::is_ascii_digit) || bytes[12] != b' '
        || bytes[13..].iter().any(|b| *b < 32 && *b != 9 || *b == 127) {
        return Err("Invalid HTTP status line".into());
    }
    let status = line[9..12].parse::<u16>().map_err(|_| "Invalid status code")?;
    if !(100..600).contains(&status) { return Err("Invalid status code".into()); }
    Ok(status)
}
pub(super) fn response_field(line: &str) -> Result<(&str, &str)> {
    let (name, value) = line.split_once(':').ok_or("Invalid response header")?;
    if !field_name(name) || value.bytes().any(|b| b < 32 && b != 9 || b == 127) {
        return Err("Invalid response header".into());
    }
    Ok((name, value.trim_matches([' ', '\t'])))
}
pub(super) fn response_length(value: &str) -> Result<usize> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid response length".into());
    }
    value.parse().map_err(|_| "Invalid response length".into())
}
pub(super) const RESPONSE_TRAILER_LIMIT: usize = 16384;

pub(super) fn response_trailer(line: &[u8]) -> Result<()> {
    let line = std::str::from_utf8(line).map_err(|_| "Invalid chunk trailers")?;
    let (name, _) = response_field(line)?;
    if ["content-length", "transfer-encoding", "content-encoding"]
        .iter().any(|field| name.eq_ignore_ascii_case(field))
    {
        return Err("Framing trailer forbidden".into());
    }
    Ok(())
}
pub(super) const CHUNK_HEADER_LIMIT: usize = 8192;

pub(super) fn chunk_line_end(pending: &[u8]) -> Result<Option<usize>> {
    bounded_delimiter(pending,b"\r\n",CHUNK_HEADER_LIMIT,"Chunk header exceeds bound")
}

pub(super) fn response_head_end(pending: &[u8]) -> Result<Option<usize>> {
    bounded_delimiter(pending,b"\r\n\r\n",65536,"HTTP headers exceed bound")
}

fn bounded_delimiter(pending: &[u8], delimiter: &[u8], limit: usize, error: &str) -> Result<Option<usize>> {
    let bounded = &pending[..pending.len().min(limit + delimiter.len())];
    if let Some(end) = bounded.windows(delimiter.len()).position(|bytes| bytes == delimiter) {
        return Ok(Some(end));
    }
    // Trailing delimiter bytes can complete in the next read.
    let partial = (1..delimiter.len()).rev().find(|&len|pending.ends_with(&delimiter[..len])).unwrap_or(0);
    if pending.len() - partial > limit {
        return Err(error.into());
    }
    Ok(None)
}

// Parse framing metadata without interpreting application-specific extensions.
pub(super) fn chunk_size(line: &[u8]) -> Result<u64> {
    if line.len() > CHUNK_HEADER_LIMIT { return Err("Chunk header exceeds bound".into()); }
    let token = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b);
    let mut p = 0;
    while p < line.len() && line[p].is_ascii_hexdigit() { p += 1; }
    if p == 0 { return Err("Invalid chunk size".into()); }
    let size = u64::from_str_radix(std::str::from_utf8(&line[..p]).unwrap(), 16)
        .map_err(|_| "Invalid chunk size")?;
    while p < line.len() {
        while p < line.len() && matches!(line[p], b' ' | b'\t') { p += 1; }
        if line.get(p) != Some(&b';') { return Err("Invalid chunk extension".into()); }
        p += 1;
        while p < line.len() && matches!(line[p], b' ' | b'\t') { p += 1; }
        let name = p;
        while p < line.len() && token(line[p]) { p += 1; }
        if p == name { return Err("Invalid chunk extension".into()); }
        let after_name = p;
        while p < line.len() && matches!(line[p], b' ' | b'\t') { p += 1; }
        if line.get(p) != Some(&b'=') { p = after_name; continue; }
        p += 1;
        while p < line.len() && matches!(line[p], b' ' | b'\t') { p += 1; }
        if line.get(p) == Some(&b'"') {
            p += 1;
            loop {
                let b = *line.get(p).ok_or("Invalid chunk extension")?;
                p += 1;
                if b == b'"' { break; }
                if b == b'\\' {
                    let escaped = *line.get(p).ok_or("Invalid chunk extension")?;
                    if !(escaped == 9 || escaped >= 32 && escaped != 127) {
                        return Err("Invalid chunk extension".into());
                    }
                    p += 1;
                } else if !(b == 9 || b == 32 || b == 33 || (35..=91).contains(&b) || b >= 93 && b != 127) {
                    return Err("Invalid chunk extension".into());
                }
            }
        } else {
            let value = p;
            while p < line.len() && token(line[p]) { p += 1; }
            if p == value { return Err("Invalid chunk extension".into()); }
        }
    }
    Ok(size)
}
/// The framing facts one response head carries. The shared head scanner reads
/// every field of a head once, so every buffered surface that routes through
/// the shared decoder judges the same head from the same facts.
struct ResponseHead {
    code: u16,
    length: Option<usize>,
    chunked: bool,
    /// The coding this head applies, when it names one.
    coding: Option<String>,
}

/// Read one response head and return its framing facts with the offset just
/// past the head. Every framing field is counted as it appears, so a repeated
/// field is refused as a duplicate in the order the fields appear, exactly as
/// the frame session refuses a repeated framing header. The coding's transport
/// applicability is left to the caller, because a head that names one coding
/// twice is ambiguous whatever coding the repeated fields carry, and the
/// duplicate refusal must not be masked by the coding's own value.
fn scan_response_head(bytes: &[u8]) -> Result<(ResponseHead, usize)> {
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("Incomplete HTTP headers")?;
    if end > 65536 {
        return Err("HTTP headers exceed limit".into());
    }
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "Invalid response headers")?;
    let mut lines = header.split("\r\n");
    let status = lines.next().ok_or("Missing status")?;
    let code = response_status(status)?;
    let mut chunked = false;
    let mut length = None;
    // One field per name per head, as the frame session enforces it. A single
    // field naming several codings is one field and stays legal input.
    let mut coding: Option<String> = None;
    for line in lines {
        let (k, v) = response_field(line)?;
        match k.to_ascii_lowercase().as_str() {
            "transfer-encoding" => {
                if chunked {
                    return Err("Duplicate transfer encoding".into());
                }
                if v.trim().eq_ignore_ascii_case("chunked") {
                    chunked = true
                } else {
                    return Err("Unsupported transfer encoding".into());
                }
            }
            "content-length" => {
                let n = response_length(v)?;
                if length.replace(n).is_some() {
                    return Err("Duplicate response length".into());
                }
            }
            "content-encoding" => {
                if coding.is_some() {
                    return Err("Duplicate response coding".into());
                }
                coding = Some(v.trim().to_string());
            }
            _ => {}
        }
    }
    Ok((ResponseHead { code, length, chunked, coding }, end + 4))
}
fn decoded_bytes_with_interim(bytes: &[u8], interim_count: u8, allowed_error: u16, coded:bool, provider_stream: bool) -> Result<(u16, Vec<u8>)> {
    if !provider_stream && bytes.len() > 8 * 1024 * 1024 {
        return Err("Response exceeds 8 MiB".into());
    }
    let (head, body_at) = scan_response_head(bytes)?;
    let code = head.code;
    let chunked = head.chunked;
    let length = head.length;
    // The head has been read whole, so its framing facts are settled before the
    // coding's value is judged: a head that names one coding twice is refused
    // as the duplicate it is, and never as the coding this transport refuses.
    if code != 304 && !coded {
        if let Some(coding) = head.coding.as_deref() {
            if !coding.eq_ignore_ascii_case("identity") {
                return Err("Compressed response not supported by this transport".into());
            }
        }
    }
    if chunked && length.is_some() {
        return Err("Ambiguous response framing".into());
    }
    if (100..200).contains(&code) {
        if interim_count >= 4 { return Err("Too many informational HTTP responses".into()); }
        if code == 101 || chunked || length.unwrap_or(0) != 0 {
            return Err("Unsupported informational response".into());
        }
        return decoded_bytes_with_interim(&bytes[body_at..], interim_count + 1, allowed_error, coded, provider_stream);
    }
    let data = &bytes[body_at..];
    let out = if code == 204 || code == 304 {
        if !data.is_empty() || (code == 204 && (chunked || length.unwrap_or(0) != 0)) {
            return Err("Body on bodyless response".into());
        }
        Vec::new()
    } else if chunked {
        let mut out = Vec::new();
        let mut p = 0;
        loop {
            let line = data[p..]
                .windows(2)
                .position(|w| w == b"\r\n")
                .ok_or("Incomplete chunk header")?;
            let n = chunk_size(&data[p..p + line])?;
            let n = usize::try_from(n).map_err(|_| "Invalid chunk size")?;
            p += line + 2;
            if n == 0 {
                let trailers = &data[p..];
                if trailers != b"\r\n" {
                    if !trailers.ends_with(b"\r\n\r\n") || trailers.len() > RESPONSE_TRAILER_LIMIT {
                        return Err("Incomplete chunk trailers".into());
                    }
                    let text =
                        std::str::from_utf8(trailers).map_err(|_| "Invalid chunk trailers")?;
                    for line in text[..text.len() - 4].split("\r\n") {
                        response_trailer(line.as_bytes())?;
                    }
                }
                break;
            }
            let next = p.checked_add(n).and_then(|end| end.checked_add(2))
                .filter(|end| *end <= data.len()).ok_or("Incomplete chunk data")?;
            if &data[next - 2..next] != b"\r\n" {
                return Err("Invalid chunk terminator".into());
            }
            if provider_stream {
                out.try_reserve(n).map_err(|_| "Insufficient memory to decode the provider stream")?;
            }
            out.extend_from_slice(&data[p..next - 2]);
            p = next;
        }
        out
    } else {
        if let Some(n) = length {
            if n != data.len() {
                return Err("Incomplete or oversized response body".into());
            }
        }
        let mut out = Vec::new();
        if provider_stream {
            out.try_reserve_exact(data.len()).map_err(|_| "Insufficient memory to decode the provider stream")?;
        }
        out.extend_from_slice(data);
        out
    };
    if code == 304 {
        return Err("HTTP 304: server returned representation metadata.".into());
    }
    if (300..400).contains(&code) {
        return Err(format!(
            "HTTP {code}: redirects are not followed; enter the final server address"
        ));
    }
    if !(200..300).contains(&code) && (allowed_error == 0 || code != allowed_error) {
        return Err(format!(
            "HTTP {code}. Check server address, credentials, model and permissions."
        ));
    }
    Ok((code, out))
}
pub fn decoded(bytes: &[u8]) -> Result<(u16, String)> {
    let (code, data) = decoded_bytes(bytes)?;
    Ok((
        code,
        String::from_utf8(data).map_err(|_| "File or response is not UTF-8 text")?,
    ))
}
pub(super) const MODEL_RESPONSE_ALLOWANCE: &str = "Model response exceeds the current device response allowance";

pub(super) fn decoded_provider_stream(bytes: &[u8]) -> Result<(u16, String)> {
    let (code, data) = decoded_bytes_with_interim(bytes, 0, 0, false, true)?;
    Ok((code, String::from_utf8(data).map_err(|_| "File or response is not UTF-8 text")?))
}
/// A recoverable create may inspect a framed 404 before a conditional PUT.
/// Redirects, auth failures, malformed framing and all other errors stay errors.
pub fn decoded_optional_file(bytes:&[u8])->Result<(u16,String)>{
    let (code,data)=decoded_bytes_with_interim(bytes,0,404,false,false)?;
    Ok((code,String::from_utf8(data).map_err(|_|"File or response is not UTF-8 text")?))
}
pub fn strong_etag(bytes: &[u8]) -> Result<String> {
    let mut rest = bytes;
    for _ in 0..=4 {
        let end = rest
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("Incomplete HTTP headers")?;
        if end > 65536 {
            return Err("HTTP headers exceed limit".into());
        }
        let header = std::str::from_utf8(&rest[..end]).map_err(|_| "Invalid response headers")?;
        let mut lines = header.split("\r\n");
        let status = lines.next().ok_or("Missing status")?;
        let code = response_status(status)?;
        if (100..200).contains(&code) {
            rest = &rest[end + 4..];
            continue;
        }
        let mut tag: Option<&str> = None;
        for line in lines {
            let (name, value) = response_field(line)?;
            if name.eq_ignore_ascii_case("etag") && tag.replace(value).is_some() {
                return Err("Ambiguous network file ETag".into());
            }
        }
        let tag = tag.ok_or("Network server did not supply an ETag for safe updating")?;
        let bytes = tag.as_bytes();
        if bytes.len() < 2
            || bytes.len() > 256
            || bytes[0] != b'"'
            || bytes[bytes.len() - 1] != b'"'
            || !bytes[1..bytes.len() - 1]
                .iter()
                .all(|b| *b == b'!' || (b'#'..=b'~').contains(b) || *b >= 128)
            || tag.chars().any(char::is_control)
        {
            return Err("Network server did not supply a strong ETag for safe updating".into());
        }
        return Ok(tag.to_string());
    }
    Err("Too many informational HTTP responses".into())
}
pub fn wire_binary(
    address: &str,
    plain: bool,
    body: &[u8],
    headers: Vec<(&str, String)>,
) -> Result<Json> {
    if body.len() > 5 * 1024 * 1024 {
        return Err("Binary request exceeds 5 MiB".into());
    }
    let mut effect = wire(address, plain, "PUT", "", headers)?;
    let framing = effect.get("wire")?.text()?;
    let header = framing.replacen(
        "Content-Length: 0\r\n",
        &format!("Content-Length: {}\r\n", body.len()),
        1,
    );
    if header == framing {
        return Err("Unable to frame binary request".into());
    }
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(body);
    if let Obj(fields) = &mut effect {
        fields.remove("wire");
        fields.insert("wire_base64".into(), s(&b64(&bytes)));
    }
    Ok(effect)
}

pub fn decoded_optional_bytes(bytes:&[u8])->Result<(u16,Vec<u8>)>{decoded_bytes_with_interim(bytes,0,404,false,false)}

/// Explicit page/resource transport only. Default transport remains identity.
pub fn decoded_coded_bytes(bytes:&[u8])->Result<(u16,Vec<u8>)>{decoded_bytes_with_interim(bytes,0,0,true,false)}
pub fn page_wire(address:&str,plain:bool,headers:Vec<(&str,String)>)->Result<Json>{let mut effect=wire(address,plain,"GET","",headers)?;let w=effect.get("wire")?.text()?.replacen("Accept-Encoding: identity\r\n","Accept-Encoding: gzip, identity\r\n",1);if let Obj(ref mut fields)=effect{fields.insert("wire".into(),s(&w));}Ok(effect)}

const FRAME_CHUNK:usize=65536;
const FRAME_MAX:u64=256*1024*1024;
#[derive(Default)]pub(super) struct ResponseFrame{buf:Vec<u8>,pub(super) status:u16,mode:u8,left:u64,total:u64,interim:u8,trailers:usize,pub(super) done:bool,wire_only:bool,head_only:bool,created_body:bool}
impl ResponseFrame{
 pub(super) fn artifact(submission:bool)->Self{Self{created_body:submission,..Self::default()}}
 pub(super) fn feed(&mut self,bytes:&[u8])->Result<Vec<u8>>{
  if bytes.len()>FRAME_CHUNK||self.buf.len()+bytes.len()>FRAME_CHUNK*2{return Err("HTTP frame exceeds bound".into());}if self.done&&!bytes.is_empty(){return Err("Bytes after framed response".into());}self.buf.extend_from_slice(bytes);let mut body=Vec::new();
  loop{if self.status==0 {
   let Some(end)=response_head_end(&self.buf)?else{break;};
   let head=std::str::from_utf8(&self.buf[..end]).map_err(|_|"Invalid HTTP headers")?;let mut lines=head.split("\r\n");let line=lines.next().ok_or("Missing status")?;let status=response_status(line)?;
   let mut length=None;let mut te=false;let mut names=std::collections::BTreeSet::new();
   for line in lines{let(k,value)=response_field(line)?;let k=k.to_ascii_lowercase();if ["content-length","transfer-encoding","content-encoding"].contains(&k.as_str())&&!names.insert(k.clone()){return Err("Duplicate framing header".into());}match k.as_str(){"content-length"=>{if value.is_empty()||!value.bytes().all(|x|x.is_ascii_digit()){return Err("Invalid content length".into());}length=Some(value.parse::<u64>().map_err(|_|"Invalid content length")?);},"transfer-encoding"=>{if !value.eq_ignore_ascii_case("chunked"){return Err("Unsupported transfer encoding".into());}te=true;},"content-encoding"=>if !self.wire_only&&status!=304&&!self.head_only&&!value.eq_ignore_ascii_case("identity"){return Err("Unsupported content encoding".into());},_=>{}}}
   if te&&length.is_some(){return Err("Ambiguous HTTP framing".into());}self.buf.drain(..end+4);
   if status<200{self.interim+=1;if self.interim>4||status==101||te||length.unwrap_or(0)!=0{return Err("Unsupported informational response".into());}continue;}
   self.status=status;if self.head_only||status==304{self.mode=1;self.left=0;}else if status==204{if te||length.unwrap_or(0)!=0{return Err("Body on bodyless response".into());}self.mode=1;self.left=0;}else if te{self.mode=3;}else if let Some(n)=length{if n>FRAME_MAX{return Err("Response exceeds stream bound".into());}self.mode=1;self.left=n;}else{self.mode=2;}
  }
  match self.mode{
   1=>{let n=(self.left as usize).min(self.buf.len());body.extend(self.buf.drain(..n));self.left-=n as u64;if self.left==0{self.done=true;if !self.buf.is_empty(){return Err("Extra HTTP body bytes".into());}}break;},
   2=>{body.append(&mut self.buf);break;},
   3=>{let Some(end)=chunk_line_end(&self.buf)?else{break;};self.left=chunk_size(&self.buf[..end])?;if self.left>FRAME_MAX{return Err("Chunk exceeds bound".into());}self.buf.drain(..end+2);self.mode=if self.left==0{6}else{4};},
   4=>{let n=(self.left as usize).min(self.buf.len());body.extend(self.buf.drain(..n));self.left-=n as u64;if self.left==0{self.mode=5;}else{break;}},
   5=>{if self.buf.len()<2{break;}if &self.buf[..2]!=b"\r\n"{return Err("Missing chunk terminator".into());}self.buf.drain(..2);self.mode=3;},
   6 => {
    let Some(end) = self.buf.windows(2).position(|pair| pair == b"\r\n") else {
     if self.trailers + self.buf.len() > RESPONSE_TRAILER_LIMIT {
      return Err("Trailers exceed bound".into());
     }
     break;
    };
    self.trailers += end + 2;
    if self.trailers > RESPONSE_TRAILER_LIMIT { return Err("Trailers exceed bound".into()); }
    if end != 0 { response_trailer(&self.buf[..end])?; }
    self.buf.drain(..end + 2);
    if end == 0 {
     if !self.buf.is_empty() { return Err("Extra HTTP body bytes".into()); }
     self.done = true;
     break;
    }
   },
   _=>return Err("Invalid HTTP stream".into())
  }if body.len()>FRAME_CHUNK{break;}
  }
  self.total=self.total.checked_add(body.len()as u64).ok_or("Body overflow")?;if self.total>FRAME_MAX||(!self.wire_only&&self.status!=200&&!(self.created_body&&self.status==201)&&self.total>FRAME_CHUNK as u64){return Err("Response body exceeds bound".into());}Ok(body)
 }
 pub(super) fn finish(&self)->Result<()>{if self.status==0||(!self.done&&self.mode!=2)||!self.buf.is_empty(){Err("HTTP response framing is incomplete".into())}else{Ok(())}}
}

struct FrameSession{owner:String,limit:usize,seen:usize,created:std::time::Instant,http:ResponseFrame}
static FRAME_IDS:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
static FRAMES:std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<u64,FrameSession>>>=std::sync::OnceLock::new();
fn frame_number(v:&Json,key:&str)->Result<u64>{match v.get(key)?{Num(n)=>n.parse().map_err(|_|"Invalid response frame number".into()),_=>Err("Response frame number required".into())}}
pub(super) fn frame(v:&Json)->Result<Json>{
 let owner=field(v,"owner")?;if owner.is_empty()||owner.len()>128{return Err("Invalid response frame owner".into());}
 let mut slots=FRAMES.get_or_init(||std::sync::Mutex::new(std::collections::BTreeMap::new())).lock().map_err(|_|"Response framing is occupied".to_string())?;
 slots.retain(|_,s|s.created.elapsed().as_secs()<120);
 if field(v,"op")?=="network_frame_open"{
  let limit=usize::try_from(frame_number(v,"limit")?).map_err(|_|"Invalid response bound")?;if limit==0||limit>6*1024*1024{return Err("Response bound must be1..6291456bytes".into());}if slots.len()>=8{return Err("Response framing capacity occupied".into());}
  let head=match v.get("head_only"){Err(_)=>false,Ok(Bool(v))=>*v,_=>return Err("Invalid HEAD response mode".into())};
  let id=FRAME_IDS.try_update(std::sync::atomic::Ordering::Relaxed,std::sync::atomic::Ordering::Relaxed,|n|n.checked_add(1).filter(|n|*n<=i64::MAX as u64)).map_err(|_|"Response frame identifiers exhausted")?;
  slots.insert(id,FrameSession{owner:owner.into(),limit,seen:0,created:std::time::Instant::now(),http:ResponseFrame{wire_only:true,head_only:head,..Default::default()}});return Ok(Num(id.to_string()));
 }
 let id=frame_number(v,"id")?;let current=slots.get(&id).is_some_and(|s|s.owner==owner);
 if field(v,"op")?=="network_frame_close"{if current{slots.remove(&id);}return Ok(Bool(current));}if !current{return Err("Response frame owner expired or changed".into());}
 if field(v,"op")?=="network_frame_finish"{let session=slots.remove(&id).unwrap();if !session.http.done&&v.get("eof")!=Ok(&Bool(true)){return Err("Response framing requires more bytes or observed EOF".into());}session.http.finish()?;return Ok(obj(vec![("status",Num(session.http.status.to_string())),("bytes",Num(session.seen.to_string()))]));}
 if field(v,"op")?!="network_frame_feed"{return Err("Unknown response frame operation".into());}
 let result=(||{let encoded=field(v,"data")?;if encoded.len()>87400{return Err("Response chunk exceeds bound".into());}let bytes=unbase64(encoded)?;let session=slots.get_mut(&id).unwrap();session.seen=session.seen.checked_add(bytes.len()).ok_or("Response size overflow")?;if session.seen>session.limit{return Err("Response exceeds current memory bound".into());}session.http.feed(&bytes)?;if [1,4].contains(&session.http.mode)&&session.http.left>session.limit.saturating_sub(session.seen)as u64{return Err("Declared response exceeds current memory bound".into());}Ok(obj(vec![("ready",Bool(session.http.done)),("status",Num(session.http.status.to_string()))]))})();
 if result.is_err(){slots.remove(&id);}result
}

/// Device authorization alone needs the protocol's framed HTTP 400 responses.
/// Other HTTP errors and all existing callers retain their strict status policy.
pub fn decoded_authorization(bytes: &[u8]) -> Result<(u16, String)> {
    let (status, data) = decoded_bytes_with_interim(bytes, 0, 400, false, false)?;
    Ok((status, String::from_utf8(data).map_err(|_| "Authorization response is not UTF-8")?))
}

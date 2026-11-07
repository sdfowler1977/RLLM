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
    for (name, value) in headers {
        if value.chars().any(|c| c.is_control())
            || name.chars().any(|c| !c.is_ascii_alphanumeric() && c != '-')
        {
            return Err("Invalid HTTP header".into());
        }
        wire.push_str(&format!("{name}: {value}\r\n"));
    }
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
    let wire=field(effect,"wire")?.to_string();
    if !wire.contains(&old){return Err("Request authority is missing; no TLS identity was applied".into());}
    let Obj(fields)=effect else{return Err("Invalid request effect".into())};
    fields.insert("wire".into(),s(&wire.replacen(&old,&replacement,1)));
    fields.insert("tls_host".into(),s(&name));
    Ok(())
}
pub fn decoded_bytes(bytes: &[u8]) -> Result<(u16, Vec<u8>)> {
    decoded_bytes_with_interim(bytes, 0, 0, false)
}
fn decoded_bytes_with_interim(bytes: &[u8], interim_count: u8, allowed_error: u16, coded:bool) -> Result<(u16, Vec<u8>)> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Response exceeds 8 MiB".into());
    }
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
    if !status.starts_with("HTTP/1.") {
        return Err("Unsupported HTTP response".into());
    }
    let code = status
        .split_whitespace()
        .nth(1)
        .ok_or("Missing status code")?
        .parse::<u16>()
        .map_err(|_| "Invalid status code")?;
    if (100..200).contains(&code) {
        if interim_count >= 4 {
            return Err("Too many informational HTTP responses".into());
        }
        return decoded_bytes_with_interim(&bytes[end + 4..], interim_count + 1, allowed_error, coded);
    }
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let (k, v) = line.split_once(':').ok_or("Invalid response header")?;
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
                let n = v
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Invalid response length")?;
                if length.replace(n).is_some() {
                    return Err("Duplicate response length".into());
                }
            }
            "content-encoding" => {
                if code != 304 && !coded && !v.trim().eq_ignore_ascii_case("identity") {
                    return Err("Compressed response not supported by this transport".into());
                }
            }
            _ => {}
        }
    }
    if chunked && length.is_some() {
        return Err("Ambiguous response framing".into());
    }
    let data = &bytes[end + 4..];
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
            let size = std::str::from_utf8(&data[p..p + line])
                .map_err(|_| "Invalid chunk header")?
                .split(';')
                .next()
                .unwrap();
            let n = usize::from_str_radix(size.trim(), 16).map_err(|_| "Invalid chunk size")?;
            p += line + 2;
            if n == 0 {
                let trailers = &data[p..];
                if trailers != b"\r\n" {
                    if !trailers.ends_with(b"\r\n\r\n") || trailers.len() > 16384 {
                        return Err("Incomplete chunk trailers".into());
                    }
                    let text =
                        std::str::from_utf8(trailers).map_err(|_| "Invalid chunk trailers")?;
                    for line in text.trim_end_matches("\r\n").split("\r\n") {
                        if !line.contains(':') {
                            return Err("Invalid chunk trailer".into());
                        }
                    }
                }
                break;
            }
            if n > 8 * 1024 * 1024 || p.checked_add(n + 2).filter(|e| *e <= data.len()).is_none() {
                return Err("Incomplete chunk data".into());
            }
            if &data[p + n..p + n + 2] != b"\r\n" {
                return Err("Invalid chunk terminator".into());
            }
            out.extend_from_slice(&data[p..p + n]);
            p += n + 2;
        }
        out
    } else {
        if let Some(n) = length {
            if n != data.len() {
                return Err("Incomplete or oversized response body".into());
            }
        }
        data.to_vec()
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
/// A recoverable create may inspect a framed 404 before a conditional PUT.
/// Redirects, auth failures, malformed framing and all other errors stay errors.
pub fn decoded_optional_file(bytes:&[u8])->Result<(u16,String)>{
    let (code,data)=decoded_bytes_with_interim(bytes,0,404,false)?;
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
        if !status.starts_with("HTTP/1.") {
            return Err("Unsupported HTTP response".into());
        }
        let code = status
            .split_whitespace()
            .nth(1)
            .ok_or("Missing status code")?
            .parse::<u16>()
            .map_err(|_| "Invalid status code")?;
        if (100..200).contains(&code) {
            rest = &rest[end + 4..];
            continue;
        }
        let mut tag: Option<&str> = None;
        for line in lines {
            let (name, value) = line.split_once(':').ok_or("Invalid response header")?;
            if name.eq_ignore_ascii_case("etag") && tag.replace(value.trim()).is_some() {
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
                .all(|b| *b == b'!' || (b'#'..=b'~').contains(b))
        {
            return Err("Network server did not supply a strong ETag for safe updating".into());
        }
        return Ok(tag.to_string());
    }
    Err("Too many informational HTTP responses".into())
}
#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn framing() {
        assert_eq!(
            decoded(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n")
                .unwrap()
                .1,
            "abc"
        );
        assert!(decoded(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nabc").is_err());
        assert!(decoded(b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n").is_err());
    }
    #[test]
    fn bodyless_representation_metadata_retains_its_actual_status() {
        for header in ["Content-Length: 5000000\r\n", "Transfer-Encoding: chunked\r\n", "Content-Encoding: gzip\r\nContent-Length: 5000000\r\n"] {
            let raw=format!("HTTP/1.1 304 Not Modified\r\n{header}\r\n");
            assert_eq!(decoded_bytes(raw.as_bytes()).unwrap_err(),"HTTP 304: server returned representation metadata.");
            let mut frame=ResponseFrame::default();assert!(frame.feed(raw.as_bytes()).unwrap().is_empty());assert!(frame.done);frame.finish().unwrap();
            let extra=format!("{raw}x");assert!(decoded_bytes(extra.as_bytes()).unwrap_err().contains("Body on bodyless"));assert!(ResponseFrame::default().feed(extra.as_bytes()).is_err());
        }
        assert!(decoded_bytes(b"HTTP/1.1 204 No Content\r\nContent-Length: 1\r\n\r\nx").is_err());
        assert_eq!(decoded_bytes(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap(),(204,Vec::new()));
    }
    #[test]
    fn addresses() {
        assert!(url("http://192.168.1.2", false).is_err());
        assert_eq!(url("https://[::1]:9443/a", false).unwrap().port, 9443);
        assert!(url("https://user:secret@host", false).is_err());
        assert!(wire(
            "https://host",
            false,
            "POST",
            "",
            vec![("Authorization", "x\r\ny".into())]
        )
        .is_err());
    }
    #[test]
    fn encoding() {
        for text in ["", "f", "fo", "foo", "café 中文"] {
            assert_eq!(unbase64(&b64(text.as_bytes())).unwrap(), text.as_bytes())
        }
    }
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
#[cfg(test)]
mod binary_checks {
    use super::*;
    #[test]
    fn binary_request_has_exact_length_and_preserves_payload() {
        let body = [0, 255, 80, 75, 3, 4];
        let e = wire_binary(
            "https://nas.test/share/project.zip",
            false,
            &body,
            vec![("If-None-Match", "*".into())],
        )
        .unwrap();
        let bytes = unbase64(e.get("wire_base64").unwrap().text().unwrap()).unwrap();
        assert!(bytes.starts_with(b"PUT /share/project.zip HTTP/1.1\r\n"));
        assert!(bytes
            .windows(b"Content-Length: 6\r\n".len())
            .any(|v| v == b"Content-Length: 6\r\n"));
        assert!(bytes.ends_with(&body));
    }
    #[test]
    fn informational_response_chain_is_bounded() {
        let mut response = b"HTTP/1.1 100 Continue\r\n\r\n".repeat(4);
        response.extend_from_slice(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(decoded_bytes(&response).unwrap().0, 201);
        response.splice(0..0, b"HTTP/1.1 100 Continue\r\n\r\n".iter().copied());
        assert!(decoded_bytes(&response).unwrap_err().contains("Too many"));
    }
    #[test]
    fn update_validator_is_strong_single_header_from_final_response() {
        let response = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nETag: \"revision-7\"\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(strong_etag(response).unwrap(), "\"revision-7\"");
        for header in [
            "ETag: W/\"old\"\r\n",
            "ETag: \"a\"\r\nETag: \"b\"\r\n",
            "ETag: invalid\r\n",
        ] {
            let response = format!("HTTP/1.1 200 OK\r\n{header}Content-Length: 0\r\n\r\n");
            assert!(strong_etag(response.as_bytes()).is_err());
        }
    }
}

#[cfg(test)]
mod binary_response_checks {
    use super::*;
    #[test]
    fn preserves_binary_response() {
        let body = [0x50, 0x4b, 3, 4, 0xff, 0];
        let mut raw = b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n".to_vec();
        raw.extend_from_slice(&body);
        assert_eq!(decoded_bytes(&raw).unwrap().1, body);
        assert!(decoded(&raw).is_err());
    }
}

pub fn decoded_optional_bytes(bytes:&[u8])->Result<(u16,Vec<u8>)>{decoded_bytes_with_interim(bytes,0,404,false)}

/// Explicit page/resource transport only. Default transport remains identity.
pub fn decoded_coded_bytes(bytes:&[u8])->Result<(u16,Vec<u8>)>{decoded_bytes_with_interim(bytes,0,0,true)}
pub fn page_wire(address:&str,plain:bool,headers:Vec<(&str,String)>)->Result<Json>{let mut effect=wire(address,plain,"GET","",headers)?;let w=effect.get("wire")?.text()?.replacen("Accept-Encoding: identity\r\n","Accept-Encoding: gzip, identity\r\n",1);if let Obj(ref mut fields)=effect{fields.insert("wire".into(),s(&w));}Ok(effect)}

const FRAME_CHUNK:usize=65536;
const FRAME_MAX:u64=256*1024*1024;
#[derive(Default)]pub(super) struct ResponseFrame{buf:Vec<u8>,pub(super) status:u16,mode:u8,left:u64,total:u64,interim:u8,trailers:usize,pub(super) done:bool,wire_only:bool,head_only:bool,created_body:bool}
impl ResponseFrame{
 pub(super) fn artifact(submission:bool)->Self{Self{created_body:submission,..Self::default()}}
 #[cfg(test)]pub(super) fn buffered(&self)->usize{self.buf.len()}
 pub(super) fn feed(&mut self,bytes:&[u8])->Result<Vec<u8>>{
  if bytes.len()>FRAME_CHUNK||self.buf.len()+bytes.len()>FRAME_CHUNK*2{return Err("HTTP frame exceeds bound".into());}if self.done&&!bytes.is_empty(){return Err("Bytes after framed response".into());}self.buf.extend_from_slice(bytes);let mut body=Vec::new();
  loop{if self.status==0 {
   let Some(end)=self.buf.windows(4).position(|x|x==b"\r\n\r\n")else{if self.buf.len()>FRAME_CHUNK{return Err("HTTP headers exceed bound".into());}break;};if end>FRAME_CHUNK{return Err("HTTP headers exceed bound".into());}
   let head=std::str::from_utf8(&self.buf[..end]).map_err(|_|"Invalid HTTP headers")?;let mut lines=head.split("\r\n");let line=lines.next().ok_or("Missing status")?;if !line.starts_with("HTTP/1.0 ")&&!line.starts_with("HTTP/1.1 "){return Err("Invalid HTTP version".into());}
   let status=line.split_whitespace().nth(1).ok_or("Missing status")?.parse::<u16>().map_err(|_|"Invalid status")?;if !(100..600).contains(&status){return Err("Invalid status".into());}
   let mut length=None;let mut te=false;let mut names=std::collections::BTreeSet::new();
   for line in lines{let(k,value)=line.split_once(':').ok_or("Invalid header")?;if k.is_empty()||!k.bytes().all(|b|b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b)){return Err("Invalid header name".into());}if value.bytes().any(|b|b<32&&b!=9||b==127){return Err("Invalid header value".into());}let k=k.to_ascii_lowercase();let value=value.trim();if ["content-length","transfer-encoding","content-encoding"].contains(&k.as_str())&&!names.insert(k.clone()){return Err("Duplicate framing header".into());}match k.as_str(){"content-length"=>{if value.is_empty()||!value.bytes().all(|x|x.is_ascii_digit()){return Err("Invalid content length".into());}length=Some(value.parse::<u64>().map_err(|_|"Invalid content length")?);},"transfer-encoding"=>{if !value.eq_ignore_ascii_case("chunked"){return Err("Unsupported transfer encoding".into());}te=true;},"content-encoding"=>if !self.wire_only&&status!=304&&!self.head_only&&!value.eq_ignore_ascii_case("identity"){return Err("Unsupported content encoding".into());},_=>{}}}
   if te&&length.is_some(){return Err("Ambiguous HTTP framing".into());}self.buf.drain(..end+4);
   if status<200{self.interim+=1;if self.interim>4||status==101||te||length.unwrap_or(0)!=0{return Err("Unsupported informational response".into());}continue;}
   self.status=status;if self.head_only||status==304{self.mode=1;self.left=0;}else if status==204{if te||length.unwrap_or(0)!=0{return Err("Body on bodyless response".into());}self.mode=1;self.left=0;}else if te{self.mode=3;}else if let Some(n)=length{if n>FRAME_MAX{return Err("Response exceeds stream bound".into());}self.mode=1;self.left=n;}else{self.mode=2;}
  }
  match self.mode{
   1=>{let n=(self.left as usize).min(self.buf.len());body.extend(self.buf.drain(..n));self.left-=n as u64;if self.left==0{self.done=true;if !self.buf.is_empty(){return Err("Extra HTTP body bytes".into());}}break;},
   2=>{body.append(&mut self.buf);break;},
   3=>{let Some(end)=self.buf.windows(2).position(|b|b==b"\r\n")else{if self.buf.len()>8192{return Err("Chunk header exceeds bound".into());}break;};if end>8192{return Err("Chunk header exceeds bound".into());}let line=std::str::from_utf8(&self.buf[..end]).map_err(|_|"Invalid chunk header")?;let n=line.split(';').next().unwrap();if n.is_empty()||!n.bytes().all(|b|b.is_ascii_hexdigit()){return Err("Invalid chunk size".into());}self.left=u64::from_str_radix(n,16).map_err(|_|"Invalid chunk size")?;if self.left>FRAME_MAX{return Err("Chunk exceeds bound".into());}self.buf.drain(..end+2);self.mode=if self.left==0{6}else{4};},
   4=>{let n=(self.left as usize).min(self.buf.len());body.extend(self.buf.drain(..n));self.left-=n as u64;if self.left==0{self.mode=5;}else{break;}},
   5=>{if self.buf.len()<2{break;}if &self.buf[..2]!=b"\r\n"{return Err("Missing chunk terminator".into());}self.buf.drain(..2);self.mode=3;},
   6=>{let Some(end)=self.buf.windows(2).position(|b|b==b"\r\n")else{if self.buf.len()+self.trailers>FRAME_CHUNK{return Err("Trailers exceed bound".into());}break;};self.trailers+=end+2;if self.trailers>FRAME_CHUNK{return Err("Trailers exceed bound".into());}if end==0{self.buf.drain(..2);if !self.buf.is_empty(){return Err("Extra HTTP body bytes".into());}self.done=true;break;}let line=std::str::from_utf8(&self.buf[..end]).map_err(|_|"Invalid trailer")?;let(k,value)=line.split_once(':').ok_or("Invalid trailer")?;if k.is_empty()||!k.bytes().all(|b|b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b))||value.bytes().any(|b|b<32&&b!=9||b==127){return Err("Invalid trailer field".into());}if ["content-length","transfer-encoding","content-encoding"].contains(&k.to_ascii_lowercase().as_str()){return Err("Framing trailer forbidden".into());}self.buf.drain(..end+2);},
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
    let (status, data) = decoded_bytes_with_interim(bytes, 0, 400, false)?;
    Ok((status, String::from_utf8(data).map_err(|_| "Authorization response is not UTF-8")?))
}
#[cfg(test)] mod authorization_response_checks {
    use super::*;
    #[test] fn pending_status_is_scoped_and_framed() {
        let pending=b"HTTP/1.1 400 Bad Request\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(decoded_authorization(pending).unwrap(),(400,"{}".into()));
        assert!(decoded(pending).is_err());
        assert!(decoded(b"HTTP/1.1 000 Invalid\r\nContent-Length: 0\r\n\r\n").is_err());
        for raw in [b"HTTP/1.1 500 Error\r\nContent-Length: 2\r\n\r\n{}".as_slice(),
            b"HTTP/1.1 302 Found\r\nContent-Length: 2\r\n\r\n{}".as_slice(),
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 3\r\n\r\n{}".as_slice()] {
            assert!(decoded_authorization(raw).is_err());
        }
    }
}

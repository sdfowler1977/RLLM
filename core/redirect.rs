//! GET redirect targets and complete framing; no transport or credential state.
use super::*;
fn canonical(value:&str)->Result<String>{let u=network::url(value,value.starts_with("http://"))?;if u.host.contains(['\\','%'])||!u.host.is_ascii(){return Err("Invalid redirect host".into());}let host=if u.host.contains(':'){format!("[{}]",u.host.to_ascii_lowercase())}else{u.host.to_ascii_lowercase()};let (path,query)=u.path.split_once('?').map(|(p,q)|(p,Some(q))).unwrap_or((&u.path,None));let mut parts=Vec::new();for piece in path.split('/') {match piece{"."=>{},".."=>{parts.pop();},_=>parts.push(piece)}}let mut path=parts.join("/");if !path.starts_with('/'){path.insert(0,'/');}if (u.path.split('?').next().unwrap_or("").ends_with("/.")||u.path.split('?').next().unwrap_or("").ends_with("/.."))&&!path.ends_with('/'){path.push('/');}if let Some(q)=query{path.push('?');path.push_str(q);}Ok(format!("{}://{}:{}{}",if u.tls{"https"}else{"http"},host,u.port,path))}
fn authority_has_credentials(address:&str)->bool{let head=if let Some(rest)=address.strip_prefix("https://"){rest}else if let Some(rest)=address.strip_prefix("http://"){rest}else if let Some(rest)=address.strip_prefix("//"){rest}else{return false};head.split(['/','?','#']).next().unwrap_or("").contains('@')}
pub(super) fn target(base:&str,location:&str)->Result<String>{
 if location.is_empty()||location.len()>2048||!location.is_ascii()||location.bytes().any(|b|b<=32||b==127||b==b'\\'){return Err("Redirect Location is empty, oversized or contains invalid URI characters".into());}
 let b=location.as_bytes();for i in 0..b.len(){if b[i]==b'%'&& !(b.get(i+1).is_some_and(u8::is_ascii_hexdigit)&&b.get(i+2).is_some_and(u8::is_ascii_hexdigit)){return Err("Redirect Location has invalid percent encoding".into());}}
 let reference=location.split('#').next().unwrap_or("");let normalized=if reference.get(..8).is_some_and(|v|v.eq_ignore_ascii_case("https://")){format!("https://{}",&reference[8..])}else if reference.get(..7).is_some_and(|v|v.eq_ignore_ascii_case("http://")){format!("http://{}",&reference[7..])}else{reference.to_string()};let reference=normalized.as_str();let u=network::url(base,base.starts_with("http://"))?;let scheme=if u.tls{"https"}else{"http"};let host=if u.host.contains(':'){format!("[{}]",u.host)}else{u.host};let origin=format!("{scheme}://{host}:{}",u.port);
 let joined=if reference.starts_with("https://")||reference.starts_with("http://"){reference.to_string()}else if reference.starts_with("//"){format!("{scheme}:{reference}")}else if reference.split(['/','?']).next().unwrap_or("").contains(':'){return Err("Redirect scheme is not HTTP or HTTPS".into());}else if reference.is_empty(){base.into()}else if reference.starts_with('/') {format!("{origin}{reference}")}else if reference.starts_with('?'){format!("{origin}{}{reference}",u.path.split('?').next().unwrap_or("/"))}else{let path=u.path.split('?').next().unwrap_or("/");let directory=path.rsplit_once('/').map(|(d,_)|d).unwrap_or("");format!("{origin}{directory}/{reference}")};
 if authority_has_credentials(&joined){return Err("Redirect Location carries credentials in its address".into());}let next=canonical(&joined)?;if u.tls&&!next.starts_with("https://"){return Err("Redirect refused an HTTPS to HTTP downgrade".into());}Ok(next)
}
pub(super) fn initial(url:&str)->Result<Json>{let url=canonical(url)?;Ok(obj(vec![("current",s(&url)),("visited",Arr(vec![s(&url)]))]))}
pub(super) fn location(raw:&[u8])->Result<Option<String>>{
 if raw.len()>2*1024*1024{return Err("Redirect response exceeds its 2 MiB bound".into());}let mut start=0;
 for interim in 0..=4 {let tail=&raw[start..];let end=tail.windows(4).position(|w|w==b"\r\n\r\n").ok_or("Redirect HTTP headers are incomplete")?;if end>65536{return Err("Redirect HTTP headers exceed limit".into());}let header=std::str::from_utf8(&tail[..end]).map_err(|_|"Redirect HTTP headers are not UTF-8")?;let mut lines=header.split("\r\n");let first=lines.next().filter(|line|!line.is_empty()).ok_or("Redirect response has no status")?;let fields=first.split(' ').collect::<Vec<_>>();if fields.len()<2||!matches!(fields[0],"HTTP/1.0"|"HTTP/1.1")||fields[1].len()!=3{return Err("Redirect response has invalid HTTP status".into());}let code=fields[1].parse::<u16>().map_err(|_|"Redirect response has invalid status code")?;
 if (100..200).contains(&code){if interim==4{return Err("Redirect response exceeds four informational heads".into());}start+=end+4;continue;}
 if !matches!(code,301|302|303|307|308){return Ok(None);}
 let mut location=None;let mut declared:Option<u64>=None;let mut length_fields=0u8;let mut transfer_coded=false;for line in lines{let(k,v)=line.split_once(':').ok_or("Redirect response has an invalid header field")?;if k.eq_ignore_ascii_case("location"){if location.is_some(){return Err("Duplicate redirect Location".into());}location=Some(v.trim().to_string());}else if k.eq_ignore_ascii_case("content-length"){length_fields+=1;if declared.is_none(){declared=v.trim().parse::<u64>().ok();}}else if k.eq_ignore_ascii_case("transfer-encoding"){transfer_coded=true;}}
 // The redirect surface refuses the one framing contradiction it can see in its own head: a body
 // whose length contradicts this head's single Content-Length. Duplicated, ambiguous or coded
 // framing stays with the authoritative decoder, under the decoder's own wording, never renamed here.
 if length_fields==1&&!transfer_coded{if let Some(n)=declared{if (tail.len()-(end+4))as u64!=n{return Err("Redirect response body does not match its declared length".into());}}}
 // Reuse the authoritative framing decoder with only status made successful.
 // Arbitrary redirect body bytes are discarded, never interpreted as a page.
 let mut framed=tail.to_vec();let code_at=first.find(' ').unwrap()+1;framed[code_at..code_at+3].copy_from_slice(b"200");network::decoded_coded_bytes(&framed)?;
 return location.map(Some).ok_or_else(||"Redirect response has no Location".into());
 }Err("Redirect response exceeds four informational heads".into())
}
#[cfg(test)]
mod checks {
    use super::*;

    // Each declaration owns one surface-issued refusal literal. Successful
    // controls live separately; inherited decoder refusals stay in that layer.
    macro_rules! refusal {
        ($name:ident, $result:expr, $literal:literal) => {
            #[test]
            fn $name() {
                assert_eq!($result.unwrap_err(), $literal, stringify!($name));
            }
        };
    }
    refusal!(host, target("https://e.test/a", "//e%2Etest/a"), "Invalid redirect host");
    #[test]
    fn uri_characters() {
        for value in [String::new(), format!("/{}", "x".repeat(2048)), "/é".into(), "/bad path".into(), "/bad\\x".into()] {
            assert_eq!(target("https://e.test/a", &value).unwrap_err(), "Redirect Location is empty, oversized or contains invalid URI characters");
        }
    }
    refusal!(percent_encoding, target("https://e.test/a", "/bad%GG"), "Redirect Location has invalid percent encoding");
    refusal!(scheme, target("https://e.test/a", "javascript:x"), "Redirect scheme is not HTTP or HTTPS");
    refusal!(credentials, target("https://e.test/a", "https://u:p@e.test/"), "Redirect Location carries credentials in its address");
    refusal!(downgrade, target("https://e.test/a", "http://e.test/"), "Redirect refused an HTTPS to HTTP downgrade");
    refusal!(wire_bound, location(&vec![b'x'; 2 * 1024 * 1024 + 1]), "Redirect response exceeds its 2 MiB bound");
    refusal!(incomplete_head, location(b"HTTP/1.1 302 X\r\nLocation: /a\r\n"), "Redirect HTTP headers are incomplete");
    refusal!(header_bound, location(&padded_head(65537)), "Redirect HTTP headers exceed limit");
    refusal!(header_encoding, location(b"HTTP/1.1 302 X\r\nLocation: /a\r\nX: \xff\r\n\r\n"), "Redirect HTTP headers are not UTF-8");
    refusal!(missing_status, location(b"\r\n\r\n"), "Redirect response has no status");
    refusal!(http_status, location(b"HTTP/2 302 X\r\nLocation: /a\r\n\r\n"), "Redirect response has invalid HTTP status");
    refusal!(status_code, location(b"HTTP/1.1 abc X\r\nLocation: /a\r\n\r\n"), "Redirect response has invalid status code");
    refusal!(informational_bound, location(&informational(5)), "Redirect response exceeds four informational heads");
    refusal!(header_field, location(b"HTTP/1.1 302 X\r\nLocation: /a\r\nBroken\r\n\r\n"), "Redirect response has an invalid header field");
    refusal!(duplicate_location, location(b"HTTP/1.1 302 X\r\nLocation: /a\r\nLocation: /b\r\n\r\n"), "Duplicate redirect Location");
    refusal!(body_length, location(b"HTTP/1.1 308 X\r\nLocation: /a\r\nContent-Length: 1\r\n\r\n"), "Redirect response body does not match its declared length");
    refusal!(missing_location, location(b"HTTP/1.1 307 X\r\nContent-Length: 0\r\n\r\n"), "Redirect response has no Location");

    fn padded_head(size: usize) -> Vec<u8> {
        let mut head = b"HTTP/1.1 302 X\r\nLocation: /a\r\nContent-Length: 0\r\nX-Pad: ".to_vec();
        head.resize(size, b'x');
        head.extend_from_slice(b"\r\n\r\n");
        head
    }
    fn informational(count: usize) -> Vec<u8> {
        let mut wire = b"HTTP/1.1 100 Continue\r\n\r\n".repeat(count);
        wire.extend_from_slice(b"HTTP/1.1 302 X\r\nLocation: /a\r\nContent-Length: 0\r\n\r\n");
        wire
    }
    #[test]
    fn uri_reference_resolution() {
        for (base, reference, expected) in [
            ("https://EXAMPLE.org/a/b?old", "../c?x#part", "https://example.org:443/c?x"),
            ("https://e.test/a/b", "?q", "https://e.test:443/a/b?q"),
            ("https://e.test/a", "//other.test/", "https://other.test:443/"),
            ("https://e.test/a/b", "?url=https://other.test/", "https://e.test:443/a/b?url=https://other.test/"),
            ("https://e.test/a", "HTTPS://OTHER.test/", "https://other.test:443/"),
        ] {
            assert_eq!(target(base, reference).unwrap(), expected);
        }
    }
    #[test]
    fn framing_and_location() {
        assert_eq!(location(b"HTTP/1.1 301 Moved\r\nLocation: /new\r\nContent-Length: 0\r\n\r\n").unwrap(), Some("/new".into()));
        assert_eq!(location(&informational(4)).unwrap(), Some("/a".into()));
        assert_eq!(location(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi").unwrap(), None);
        assert_eq!(location(&padded_head(65536)).unwrap(), Some("/a".into()));
        let header = format!("HTTP/1.1 302 X\r\nLocation: /a\r\nContent-Length: {}\r\n\r\n", 2 * 1024 * 1024 - 1);
        let size = 2 * 1024 * 1024 - header.len();
        let mut wire = format!("HTTP/1.1 302 X\r\nLocation: /a\r\nContent-Length: {size}\r\n\r\n").into_bytes();
        wire.resize(2 * 1024 * 1024, b'x');
        assert_eq!(location(&wire).unwrap(), Some("/a".into()));
        let longest = format!("/{}", "x".repeat(2047));
        assert!(target("https://e.test/a", &longest).unwrap().ends_with(&longest));
    }
}

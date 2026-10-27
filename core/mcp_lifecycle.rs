use super::*;

// Cleanup is best effort and never changes the result of the abandoned operation.
// A plan contains only Rust-authored requests pinned to the old connection.
pub(super) fn plan(context:&Json,session:&str,cancel:bool)->Result<Json>{
    let endpoint=field(context,"endpoint")?;let key=field(context,"key")?;let version=field(context,"version")?;
    if endpoint.len()>8192||key.len()>8192||!["2025-03-26","2025-06-18"].contains(&version){return Err("Invalid MCP lifecycle binding".into());}
    if session.len()>256||!session.bytes().all(|b|(0x21..=0x7e).contains(&b)){return Err("Invalid MCP lifecycle session".into());}
    let mut headers=vec![("Accept","application/json, text/event-stream".into()),("MCP-Protocol-Version",version.into())];
    if !key.is_empty(){headers.push(("Authorization",format!("Bearer {key}")));}
    if !session.is_empty(){headers.push(("Mcp-Session-Id",session.into()));}
    if context.get("request").is_ok(){super::mcp_identity::captured(context)?;}
    let mut requests=Vec::new();
    if cancel&&matches!(context.get("request").and_then(Json::text),Ok("tools/list"|"tools/call")){
        let request_id=super::mcp_identity::captured(context)?.ok_or("MCP cancellation has no request identity")?;
        let body=obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/cancelled")),("params",obj(vec![("requestId",request_id),("reason",s("User requested cancellation"))]))]);
        let mut h=headers.clone();h.push(("Content-Type","application/json".into()));
        let mut e=network::wire(endpoint,endpoint.starts_with("http://"),"POST",&body.string(),h)?;
        if let Obj(fields)=&mut e{fields.insert("cleanup".into(),s("notification"));}requests.push(e);
    }
    if !session.is_empty(){let mut e=network::wire(endpoint,endpoint.starts_with("http://"),"DELETE","",headers)?;if let Obj(fields)=&mut e{fields.insert("cleanup".into(),s("session"));}requests.push(e);}
    Ok(Arr(requests))
}

pub(super) fn response(v:&Json)->Result<Json>{
    let encoded=field(v,"data")?;if encoded.len()>((16384+2)/3)*4{return Err("MCP cleanup acknowledgment exceeds encoded limit".into());}
    let bytes=network::unbase64(encoded)?;if bytes.len()>16384{return Err("MCP cleanup acknowledgment exceeds 16 KiB".into());}
    let framed=bytes.windows(4).position(|w|w==b"\r\n\r\n").and_then(|end|std::str::from_utf8(&bytes[..end]).ok()).map(|head|head.lines().skip(1).any(|line|line.split_once(':').map(|(k,_)|k.eq_ignore_ascii_case("content-length")||k.eq_ignore_ascii_case("transfer-encoding")).unwrap_or(false))).unwrap_or(false);
    if !framed&&v.get("eof")!=Ok(&Bool(true)){return Ok(obj(vec![("ready",Bool(false))]));}
    // Preserve arbitrary final HTTP status while reusing the strict framing decoder.
    // A rejection is observation only: it never triggers a retry or another tool call.
    let Some(line)=bytes.windows(2).position(|w|w==b"\r\n") else{return if v.get("eof")==Ok(&Bool(true)){Err("Incomplete MCP cleanup response".into())}else{Ok(obj(vec![("ready",Bool(false))]))};};
    let header=std::str::from_utf8(&bytes[..line]).map_err(|_|"Invalid MCP cleanup status")?;
    if !header.starts_with("HTTP/1."){return Err("Invalid MCP cleanup HTTP version".into());}
    let status=header.split_whitespace().nth(1).ok_or("Missing MCP cleanup status")?.parse::<u16>().map_err(|_|"Invalid MCP cleanup status")?;
    if !(200..600).contains(&status){return Err("Unexpected MCP cleanup status".into());}
    let mut normalized=b"HTTP/1.1 200 OK".to_vec();normalized.extend_from_slice(&bytes[line..]);
    match network::decoded(&normalized){
        Ok((_,body))=>{let accepted=match field(v,"cleanup")?{"notification"=>status==202&&body.is_empty(),"session"=>(200..300).contains(&status),_=>return Err("Unknown MCP cleanup purpose".into())};Ok(obj(vec![("ready",Bool(true)),("accepted",Bool(accepted)),("status",Num(status.to_string()))]))},
        Err(e)=>if v.get("eof")==Ok(&Bool(true)){Err(e)}else{Ok(obj(vec![("ready",Bool(false))]))},
    }
}

#[cfg(test)] mod checks{
 use super::*;
 fn context(request:&str)->Json{obj(vec![("endpoint",s("https://example.test/proxy/mcp?q=x")),("key",s("OLD")),("version",s("2025-06-18")),("request",s(request)),("request_binding",Num("1".into()))])}
 #[test]fn cancellation_direction_initialize_and_session_binding(){
  let p=plan(&context("tools/call"),"old-session",true).unwrap();let a=p.array().unwrap();assert_eq!(a.len(),2);let w=field(&a[0],"wire").unwrap();assert!(w.starts_with("POST /proxy/mcp?q=x ")&&w.contains("Bearer OLD")&&w.contains("Mcp-Session-Id: old-session")&&w.contains("notifications/cancelled"));assert!(!w.contains("\"id\":1"));assert!(field(&a[1],"wire").unwrap().starts_with("DELETE "));
  assert_eq!(plan(&context("initialize"),"s",true).unwrap().array().unwrap().len(),1);
  assert!(plan(&context("initialize"),"",true).unwrap().array().unwrap().is_empty());
  assert_eq!(plan(&context("tools/list"),"",true).unwrap().array().unwrap().len(),1);
  assert_eq!(plan(&context("tools/call"),"s",false).unwrap().array().unwrap().len(),1);
 }
 #[test]fn bounded_acknowledgments_are_advertisements_not_completion(){
  let r=|b:&[u8],eof:bool,kind:&str|response(&obj(vec![("data",s(&network::b64(b))),("eof",Bool(eof)),("cleanup",s(kind))]));
  assert_eq!(r(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",false,"notification").unwrap().get("accepted"),Ok(&Bool(true)));
  assert_eq!(r(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n",false,"session").unwrap().get("accepted"),Ok(&Bool(false)));
  assert_eq!(r(b"HTTP/1.1 202 Accepted\r\n\r\n",false,"notification").unwrap().get("ready"),Ok(&Bool(false)));
  assert_eq!(r(b"HTTP/1.1 202 Accepted\r\n\r\nx",true,"notification").unwrap().get("accepted"),Ok(&Bool(false)));
  assert!(r(&vec![0;16385],true,"session").is_err());
 }
}

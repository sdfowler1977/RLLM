use super::*;

// Cleanup is best effort and never changes the result of the abandoned operation.
// A plan contains only Rust-authored requests pinned to the old connection.
pub(super) fn plan(context:&Json,session:&str,cancel:bool)->Result<Json>{
    let endpoint=field(context,"endpoint")?;let key=field(context,"key")?;let version=field(context,"version")?;
    if endpoint.len()>8192||key.len()>8192||!super::mcp_schema::VERSIONS.contains(&version){return Err("Invalid MCP lifecycle binding".into());}
    if session.len()>256||!session.bytes().all(|b|(0x21..=0x7e).contains(&b)){return Err("Invalid MCP lifecycle session".into());}
    let mut headers=vec![("Accept","application/json, text/event-stream".into())];
    if context.get("request").and_then(Json::text)!=Ok("initialize"){headers.push(("MCP-Protocol-Version",version.into()));}
    if let Some(token)=network::mcp_bearer_token(&key)?{headers.push(network::mcp_authorization_header(&token)?);}
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

/// The MCP client's own wording for a refused exchange.
///
/// `sent` says whether a bearer token went out, `challenge` is the peer's
/// RFC 6750 error code, and `again` names the MCP action that follows the
/// refusal. Every MCP surface uses this wording, so a refused exchange reads
/// the same way whichever transport carried it, and a refused MCP exchange
/// never borrows the web reader's sentence.
pub(super) fn mcp_refusal(sent:bool,status:u16,challenge:Option<&str>,again:&str)->String{
 match status{
  401 if !sent=>format!("MCP HTTP 401: no bearer token was sent. Save the server bearer token for this connection and {again} again."),
  401 if challenge==Some("invalid_token")=>format!("MCP HTTP 401: the endpoint reported the bearer token RLLM sent as invalid. Save a current token for this server and {again} again."),
  401 if challenge==Some("insufficient_scope")=>format!("MCP HTTP 401: the endpoint reported the bearer token RLLM sent as missing a required scope. Save a token with the required scope and {again} again."),
  401=>format!("MCP HTTP 401: the endpoint rejected the bearer token RLLM sent. Save a current token for this server and {again} again."),
  403=>"MCP HTTP 403: the endpoint refused this connection. Check the access rules this server applies to the credential.".into(),
  404=>"MCP HTTP 404: the MCP endpoint path was not found on this server.".into(),
  405=>"MCP HTTP 405: this MCP endpoint does not accept requests at this path.".into(),
  _=>format!("MCP HTTP {status}: the MCP request was refused by the server."),
 }
}
/// The RFC 6750 error code from a WWW-Authenticate challenge.
///
/// Only a sanitised token is kept: the peer's own text never reaches a
/// product message.
pub(super) fn mcp_challenge(head:&str)->Option<String>{
 for line in head.lines().skip(1){
  let Some((name,value))=line.split_once(':') else{continue;};
  if !name.trim().eq_ignore_ascii_case("www-authenticate"){continue;}
  return mcp_challenge_code(value);
 }
 None
}
fn mcp_challenge_code(value:&str)->Option<String>{
 let bytes=value.as_bytes();let mut index=0;
 while index+5<=bytes.len(){
  if bytes[index..index+5].eq_ignore_ascii_case(b"error"){
   let mut end=index+5;
   if end<bytes.len()&&(bytes[end]==b'_'||bytes[end].is_ascii_alphanumeric()){index=end;continue;}
   while end<bytes.len()&&bytes[end].is_ascii_whitespace(){end+=1;}
   if end>=bytes.len()||bytes[end]!=b'='{index+=5;continue;}
   end+=1;
   while end<bytes.len()&&bytes[end].is_ascii_whitespace(){end+=1;}
   if end>=bytes.len(){return None;}
   let start=if bytes[end]==b'"'{end+=1;let start=end;while end<bytes.len()&&bytes[end]!=b'"'{end+=1;}start}else{let start=end;while end<bytes.len()&&!bytes[end].is_ascii_whitespace()&&bytes[end]!=b','&&bytes[end]!=b';'{end+=1;}start};
   let raw=value.get(start..end)?;
   let token:String=raw.chars().filter(|character|character.is_ascii_alphanumeric()||*character=='_'||*character=='-').take(64).collect();
   let token=token.to_ascii_lowercase();
   if !token.is_empty(){return Some(token);}
   index+=5;
  }else{index+=1;}
 }
 None
}
/// Whether a bearer token went out with an MCP request made for a context.
///
/// The captured request context carries the credential the request boundary
/// composed, so the answer is the credential that actually went out. The
/// credential is read through the same normaliser the request boundary uses,
/// so a blank or unusable credential is reported as no credential.
pub(super) fn mcp_credential_sent(context:&Json)->bool{
 context.get("key").and_then(Json::text).ok().map(|key|network::mcp_bearer_token(&key).ok().flatten().is_some()).unwrap_or(false)
}

#[cfg(test)] mod checks{
 use super::*;
 fn context(request:&str)->Json{obj(vec![("endpoint",s("https://example.test/proxy/mcp?q=x")),("key",s("OLD")),("version",s("2025-06-18")),("request",s(request)),("request_binding",Num("1".into()))])}
 #[test]fn mcp_route_asks_for_json(){
  let planned=plan(&context("initialize"),"session",false).unwrap();
  let raw=field(&planned.array().unwrap()[0],"wire").unwrap();
  assert!(raw.contains("\r\nAccept: application/json, text/event-stream\r\n"), "{raw}");
  assert_eq!(raw.matches("\r\nAccept:").count(), 1, "{raw}");
  assert!(raw.contains("\r\nUser-Agent: RLLM web reader\r\n"), "{raw}");
  assert_eq!(raw.matches("\r\nUser-Agent:").count(), 1, "{raw}");
 }
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
 #[test]fn mcp_refusal_tells_every_refusal_apart(){
  assert_eq!(mcp_refusal(false,401,None,"connect"),"MCP HTTP 401: no bearer token was sent. Save the server bearer token for this connection and connect again.");
  assert_eq!(mcp_refusal(true,401,Some("invalid_token"),"connect"),"MCP HTTP 401: the endpoint reported the bearer token RLLM sent as invalid. Save a current token for this server and connect again.");
  assert_eq!(mcp_refusal(true,401,Some("insufficient_scope"),"listen"),"MCP HTTP 401: the endpoint reported the bearer token RLLM sent as missing a required scope. Save a token with the required scope and listen again.");
  assert_eq!(mcp_refusal(true,401,None,"connect"),"MCP HTTP 401: the endpoint rejected the bearer token RLLM sent. Save a current token for this server and connect again.");
  assert_eq!(mcp_refusal(true,403,None,"connect"),"MCP HTTP 403: the endpoint refused this connection. Check the access rules this server applies to the credential.");
  assert_eq!(mcp_refusal(true,404,None,"connect"),"MCP HTTP 404: the MCP endpoint path was not found on this server.");
  assert_eq!(mcp_refusal(true,405,None,"connect"),"MCP HTTP 405: this MCP endpoint does not accept requests at this path.");
  assert_eq!(mcp_refusal(true,500,None,"connect"),"MCP HTTP 500: the MCP request was refused by the server.");
  // A missing credential and a rejected credential read differently, and the
  // web reader's sentence never reaches an MCP surface.
  assert_ne!(mcp_refusal(false,401,None,"connect"),mcp_refusal(true,401,None,"connect"));
  for status in [401,403,404,405,500]{assert!(!mcp_refusal(true,status,None,"connect").contains("Check server address, credentials, model and permissions."),"status {status}");}
 }
 #[test]fn mcp_challenge_reads_the_rfc6750_error_code(){
  assert_eq!(mcp_challenge("HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"mcp\", error=\"invalid_token\""),Some("invalid_token".into()));
  assert_eq!(mcp_challenge("HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"mcp\", error=insufficient_scope"),Some("insufficient_scope".into()));
  assert_eq!(mcp_challenge("HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"mcp\""),None);
  assert_eq!(mcp_challenge("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0"),None);
  assert_eq!(mcp_challenge("HTTP/1.1 401 Unauthorized"),None);
  // The peer's own text is never carried into a product message.
  assert_eq!(mcp_challenge("HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer error=\"two words\""),Some("twowords".into()));
 }
 #[test]fn mcp_credential_sent_reads_the_credential_the_request_carried(){
  let with=|key:&str|obj(vec![("key",s(key))]);
  assert!(mcp_credential_sent(&with("REMOTE")));
  assert!(mcp_credential_sent(&with("Bearer REMOTE")));
  assert!(!mcp_credential_sent(&with("")));
  assert!(!mcp_credential_sent(&with("   ")));
  assert!(!mcp_credential_sent(&obj(vec![])));
  // A credential the normaliser refuses never went out.
  assert!(!mcp_credential_sent(&with("two words")));
 }
}

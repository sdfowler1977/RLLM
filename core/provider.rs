use super::*;
/// Provider metadata describes interruption; generated prose cannot set it.
pub const EMPTY_REPLY:&str="Server returned no text response";
pub struct Reply {pub text:String,pub stop_reason:Option<String>,pub interrupted:bool,pub proposal:Option<Json>}
pub(super) fn limit_reason(protocol:&str,reason:&str)->bool{
 match protocol{"chat"|"local"=>reason=="length","messages"=>matches!(reason,"max_tokens"|"model_context_window_exceeded"),"content"=>reason=="MAX_TOKENS","responses"=>reason=="max_tokens",_=>false}
}
fn stop(value:&Json,key:&str)->Option<String>{
 // Missing or unknown optional metadata never gates an otherwise valid text
 // reply. Only a bounded explicit known marker can declare a limit.
 match value.get(key){Ok(Str(reason))if reason.len()<=128&&!reason.chars().any(char::is_control)=>Some(reason.clone()),_=>None}
}
fn visible(value:&Json,key:&str)->Result<String>{
 match value.get(key){Ok(Null)|Err(_)=>Ok(String::new()),Ok(value)=>Ok(value.text()?.to_string())}
}
pub fn reply(protocol:&str,source:&str)->Result<Reply>{
 let mut body=parse(source)?;
 if let Ok(error)=body.get("error"){if error!=&Null{return Err(format!("Server error: {}",error.string()));}}
 stream::adopt_namespaced_calls(protocol,&mut body);
 let (text,reason)=match protocol{
  "chat"=>{let choice=super::provider_calls::choice(&body)?;(visible(choice.get("message")?,"content")?,stop(choice,"finish_reason"))},
  "local"=>(visible(body.get("message")?,"content")?,stop(&body,"done_reason")),
  "responses"=>{let (text,reason)=hosted_search::responses_text(&body)?;(text,Some(reason))},
  "messages"=>{let mut parts=Vec::new();for part in body.get("content")?.array()?{if field(part,"type")==Ok("text"){parts.push(field(part,"text")?.to_string());}}(parts.join("\n"),stop(&body,"stop_reason"))},
  "content"=>{let text=stream::content_text(&body)?;let candidate=body.get("candidates")?.array()?.iter().find(|c|c.get("index").unwrap_or(&Num("0".into()))==&Num("0".into())).ok_or("No response candidate zero")?;(text,stop(candidate,"finishReason"))},
  "speech"=>return Err("Full-duplex speech returns audio and text on the open microphone stream".into()),
  _=>return Err("Unknown API format".into())
 };
 let interrupted=reason.as_ref().is_some_and(|reason|limit_reason(protocol,reason));
 let proposal=if interrupted||protocol=="responses"{None}else{super::provider_calls::proposal(protocol,&body)?};
 if text.trim().is_empty()&&!interrupted&&proposal.is_none(){return Err(EMPTY_REPLY.into());}
 if proposal.is_some() && match protocol{"chat"=>reason.as_deref()!=Some("tool_calls"),"messages"=>reason.as_deref()!=Some("tool_use"),"content"=>reason.as_deref()!=Some("STOP"),"local"=>body.get("done")!=Ok(&Bool(true)),_=>false}{return Err("Tool proposal has no completed provider turn".into());}
 Ok(Reply{text,stop_reason:reason,interrupted,proposal})
}
impl Reply{pub fn details(&self)->Json{obj(vec![("version",Num("1".into())),("text",s(&self.text)),("stop_reason",self.stop_reason.as_ref().map(|r|s(r)).unwrap_or(Null)),("interrupted",Bool(self.interrupted)),("proposal",self.proposal.clone().unwrap_or(Null))])}}
fn text<'a>(p: &'a Json, k: &str) -> &'a str {
    p.get(k).and_then(Json::text).unwrap_or("")
}
pub const AGENT_ROLES: [&str; 3] = ["both", "primary", "subagent"];
pub const MAX_STREAMS: usize = 64;
/// Connections saved before these fields existed act as both, with one stream.
pub fn agent_role(p: &Json) -> &str {
    match text(p, "agent_role") { r @ ("primary" | "subagent") => r, _ => "both" }
}
pub fn allows(p: &Json, role: &str) -> bool {
    let allowed = agent_role(p);
    allowed == "both" || allowed == role
}
pub fn streams(p: &Json) -> usize {
    match p.get("streams") { Ok(Num(v) | Str(v)) => v.parse::<usize>().ok(), _ => None }
        .filter(|n| (1..=MAX_STREAMS).contains(n)).unwrap_or(1)
}
pub fn role_label(p: &Json) -> &'static str {
    match agent_role(p) { "primary" => "Primary agent only", "subagent" => "Sub-agent only", _ => "Primary agent or sub-agent" }
}
// Scheme inference is confined to model setup; explicit schemes always win.
fn local_address(input: &str) -> bool {
    let authority = input.split(['/', '?']).next().unwrap_or("");
    let host = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') { Some((host, _)) => host, None => return false }
    } else { authority.split(':').next().unwrap_or("") };
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost")
        || host.to_ascii_lowercase().ends_with(".local") { return true; }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            let octets = ip.octets();
            ip.is_private() || ip.is_loopback() || ip.is_link_local()
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
        Err(_) => false,
    }
}
pub fn is_speech(p: &Json) -> bool { text(p, "protocol") == "speech" }
pub struct SpeechSetup { pub endpoint: String, pub plain: bool, pub model: String, pub voice_prompt: String, pub role_prompt: String, pub authorization: Option<(String, String)> }
pub fn check_speech_prompt(key: &str, value: &str) -> Result<()> {
    let limit = if key == "role_prompt" { 4096 } else { 128 };
    if value.len() > limit || value.chars().any(|c| c.is_control()) {
        return Err(format!("{key} is too long or contains control characters"));
    }
    Ok(())
}
pub fn speech_setup(p: &Json) -> Result<SpeechSetup> {
    if !is_speech(p) { return Err("This connection is not a full-duplex speech connection".into()); }
    Ok(SpeechSetup {
        endpoint: text(p, "endpoint").to_string(),
        plain: p.get("plain") == Ok(&Bool(true)),
        model: text(p, "model").to_string(),
        voice_prompt: text(p, "voice_prompt").to_string(),
        role_prompt: text(p, "role_prompt").to_string(),
        authorization: authorization(p)?,
    })
}
fn normalize_speech(p: &Json) -> Result<Json> {
    let model = text(p, "model").trim();
    if model.is_empty() || model.len() > 256 { return Err("Enter a model name of at most 256 bytes".into()); }
    if text(p, "key").len() > 8192 || text(p, "name").len() > 128 { return Err("Profile field exceeds its size limit".into()); }
    check_speech_prompt("voice_prompt", text(p, "voice_prompt"))?;
    check_speech_prompt("role_prompt", text(p, "role_prompt"))?;
    if text(p, "endpoint").contains('{') { return Err("The speech address cannot contain a model placeholder".into()); }
    let input = text(p, "endpoint").trim();
    if input.is_empty() || input.len() > 2048 { return Err("Enter a server address of at most 2048 bytes".into()); }
    let (explicit_plain, body) = if let Some(rest) = input.strip_prefix("wss://").or_else(|| input.strip_prefix("https://")) {
        (Some(false), rest)
    } else if let Some(rest) = input.strip_prefix("ws://").or_else(|| input.strip_prefix("http://")) {
        (Some(true), rest)
    } else { (None, input) };
    let plain = explicit_plain.unwrap_or_else(|| local_address(body) || p.get("plain") == Ok(&Bool(true)));
    let mut endpoint = if explicit_plain.is_some() {
        format!("{}://{body}", if plain { "http" } else { "https" })
    } else {
        format!("{}://{body}", if plain { "http" } else { "https" })
    };
    let parsed = network::url(&endpoint, plain)?;
    let (path, query) = parsed.path.split_once('?').map(|(path, query)| (path, format!("?{query}"))).unwrap_or((parsed.path.as_str(), String::new()));
    // Only exact default routes are completed; custom paths stay as entered.
    let path = if path == "/" || path == "/api" { "/api/chat" } else { path };
    endpoint = format!("{}://{}{}{path}{query}", if plain { "http" } else { "https" }, if parsed.host.contains(':') { format!("[{}]", parsed.host) } else { parsed.host.clone() }, if (plain && parsed.port == 80) || (!plain && parsed.port == 443) { String::new() } else { format!(":{}", parsed.port) });
    if !["", "default"].contains(&text(p, "thinking")) {
        return Err("Full-duplex speech does not use a thinking level. Clear it or leave the server default.".into());
    }
    if !text(p, "output_field").is_empty() || !text(p, "output_parameter").is_empty() {
        return Err("Clear chat output settings before saving a full-duplex speech connection".into());
    }
    let mut out = p.clone();
    if let Obj(fields) = &mut out {
        fields.insert("protocol".into(), s("speech"));
        fields.insert("plain".into(), Bool(plain));
        fields.insert("endpoint".into(), s(&endpoint));
        fields.insert("model".into(), s(model));
        fields.insert("name".into(), s(text(p, "name").trim()));
        fields.insert("voice_prompt".into(), s(text(p, "voice_prompt").trim()));
        fields.insert("role_prompt".into(), s(text(p, "role_prompt").trim()));
        let role = text(p, "agent_role").trim();
        let role = if role.is_empty() { "both" } else { role };
        if !AGENT_ROLES.contains(&role) { return Err("Choose whether this model is a primary agent, a sub-agent, or both".into()); }
        fields.insert("agent_role".into(), s(role));
        let streams = match p.get("streams") {
            Ok(Null) | Err(_) => 1,
            Ok(Num(v) | Str(v)) => v.trim().parse::<usize>().map_err(|_| "Enter a whole number of streams from 1 to 64")?,
            Ok(_) => return Err("Enter a whole number of streams from 1 to 64".into()),
        };
        if !(1..=MAX_STREAMS).contains(&streams) { return Err("Maximum streams must be from 1 to 64".into()); }
        fields.insert("streams".into(), Num(streams.to_string()));
    }
    hosted_search::validate(&out)?;
    if out.get("oauth_enabled") == Ok(&Bool(true)) { application::oauth::validate_profile(&out)?; } else { authorization(&out)?; }
    Ok(out)
}
pub fn normalized(p: &Json) -> Result<Json> {
    let protocol = text(p, "protocol");
    if protocol == "speech" { return normalize_speech(p); }
    if !["chat", "local", "messages", "content"].contains(&protocol) {
        return Err("Choose an API format".into());
    }
    let model = text(p, "model").trim();
    if model.is_empty() || model.len() > 256 {
        return Err("Enter a model name of at most 256 bytes".into());
    }
    if text(p, "key").len() > 8192 || text(p, "name").len() > 128 {
        return Err("Profile field exceeds its size limit".into());
    }
    let input = text(p, "endpoint").trim();
    if input.is_empty() || input.len() > 2048 {
        return Err("Enter a server address of at most 2048 bytes".into());
    }
    let explicit = input.contains("://");
    let plain = if explicit { input.starts_with("http://") } else {
        protocol == "local" || local_address(input) || p.get("plain") == Ok(&Bool(true))
    };
    let mut endpoint = if explicit {
        input.to_string()
    } else {
        format!("{}://{}", if plain { "http" } else { "https" }, input)
    };
    let parsed = network::url(&endpoint.replace("{model}", "model"), plain)?;
    let suffix = match (protocol, parsed.path.trim_end_matches('/')) {
        ("chat", "") => Some("/v1/chat/completions"),
        ("chat", "/v1") => Some("/chat/completions"),
        ("chat", path) if path.ends_with("/v1") => Some("/chat/completions"),
        ("local", "") => Some("/api/chat"),
        ("local", "/api") => Some("/chat"),
        ("messages", "") => Some("/v1/messages"),
        ("messages", "/v1") => Some("/messages"),
        ("content", "") => Some("/v1beta/models/{model}:generateContent"),
        ("content", "/v1beta") => Some("/models/{model}:generateContent"),
        _ => None,
    };
    if let Some(suffix) = suffix {
        endpoint = endpoint.trim_end_matches('/').to_string() + suffix;
    }
    if protocol=="chat" && (endpoint.ends_with("/api/chat") || endpoint.ends_with("/api/generate") || (endpoint.ends_with("/completions") && !endpoint.ends_with("/chat/completions"))) {return Err("This URL uses a different API. For chat completions, enter the base URL ending /v1 or the complete /v1/chat/completions URL.".into());}
    let mut out = p.clone();
    if let Obj(fields) = &mut out {
        fields.insert("plain".into(),Bool(plain));
        fields.insert("endpoint".into(), s(&endpoint));
        fields.insert("model".into(), s(model));
        fields.insert("name".into(),s(text(p,"name").trim()));
        let role = text(p, "agent_role").trim();
        let role = if role.is_empty() { "both" } else { role };
        if !AGENT_ROLES.contains(&role) {
            return Err("Choose whether this model is a primary agent, a sub-agent, or both".into());
        }
        fields.insert("agent_role".into(), s(role));
        let streams = match p.get("streams") {
            Ok(Null) | Err(_) => 1,
            Ok(Num(v) | Str(v)) => v.trim().parse::<usize>().map_err(|_| "Enter a whole number of streams from 1 to 64")?,
            Ok(_) => return Err("Enter a whole number of streams from 1 to 64".into()),
        };
        if !(1..=MAX_STREAMS).contains(&streams) {
            return Err("Maximum streams must be from 1 to 64".into());
        }
        fields.insert("streams".into(), Num(streams.to_string()));
    }
    hosted_search::validate(&out)?;
    if out.get("oauth_enabled")==Ok(&Bool(true)){application::oauth::validate_profile(&out)?;}else{authorization(&out)?;}
    application::generation::validate(&out)?;
    Ok(out)
}
/// Derive the selected transport without changing the saved connection.
/// Only the standard method suffix changes; custom proxy paths stay intact.
pub fn content_endpoint(endpoint:&str, streaming:bool)->Result<String> {
    let (path,query)=endpoint.split_once('?').unwrap_or((endpoint,""));
    let path=if let Some(base)=path.strip_suffix(":generateContent"){format!("{base}:{}",if streaming{"streamGenerateContent"}else{"generateContent"})}
        else if let Some(base)=path.strip_suffix(":streamGenerateContent"){format!("{base}:{}",if streaming{"streamGenerateContent"}else{"generateContent"})}else{path.to_string()};
    let mut fields=Vec::new();let mut alt=false;
    for item in query.split('&').filter(|v|!v.is_empty()) {
        let (key,value)=item.split_once('=').unwrap_or((item,""));
        let mut decoded=Vec::new();let bytes=key.as_bytes();let mut i=0;
        while i<bytes.len(){if bytes[i]==b'%'&&i+2<bytes.len(){let hex=std::str::from_utf8(&bytes[i+1..i+3]).map_err(|_|"Invalid Content query")?;decoded.push(u8::from_str_radix(hex,16).map_err(|_|"Invalid Content query")?);i+=3;}else{decoded.push(bytes[i]);i+=1;}}
        if decoded==b"alt" {
            if alt {return Err("Duplicate Content alt query parameter".into());}alt=true;
            if streaming && value!="sse" {return Err("Content streaming needs alt=sse; remove the conflicting alt parameter or turn off Stream replies".into());}
            if !streaming && value=="sse" {continue;}
        }
        fields.push(item.to_string());
    }
    if streaming && !alt {fields.push("alt=sse".into());}
    Ok(if fields.is_empty(){path}else{format!("{path}?{}",fields.join("&"))})
}
pub fn authorization(p: &Json) -> Result<Option<(String, String)>> {
    if p.get("oauth_enabled")==Ok(&Bool(true)){return application::oauth::authorization(p);}
    let key = text(p, "key");
    if key.is_empty() {
        return Ok(None);
    }
    let custom = text(p, "auth_header").trim();
    let protocol = text(p, "protocol");
    let (name, prefix) = if custom.is_empty() {
        match protocol {
            "messages" => ("x-api-key", ""),
            "content" => ("x-goog-api-key", ""),
            _ => ("Authorization", "Bearer"),
        }
    } else {
        (custom, text(p, "auth_prefix").trim())
    };
    network::credential_header(name,prefix,key).map(Some)
}
/// Probe the selected server without adding chat messages or sending prompts.
pub fn probe(p: &Json) -> Result<Json> {
    let endpoint=text(p,"endpoint");
    let (suffix,replacement)=match text(p,"protocol") {
        "local" => ("/api/chat","/api/tags"),
        "chat" if hosted_search::responses_endpoint(endpoint) => ("/responses","/models"),
        "chat" => ("/chat/completions","/models"),
        "speech" => return Err("Full-duplex speech has no model list. Open microphone conversation checks the speech server.".into()),
        _ => return Err("This API format has no standard model-list probe. Send a message to test generation.".into()),
    };
    let prefix=endpoint.strip_suffix(suffix).ok_or("Custom API path has no standard model-list URL. Send a message to test generation.")?;
    let auth=authorization(p)?;
    let mut headers=vec![("Accept","application/json".into())];
    if let Some((name,value))=&auth {headers.push((name.as_str(),value.clone()));}
    let mut effect=network::wire(&format!("{prefix}{replacement}"),p.get("plain")==Ok(&Bool(true)),"GET","",headers)?;
    if let Obj(fields)=&mut effect {fields.insert("kind".into(),s("network"));fields.insert("purpose".into(),s("connection_test"));}
    Ok(effect)
}
pub fn probe_result(p:&Json,body:&str)->Result<String> {
    let response=parse(body)?;
    let (key,field)=if text(p,"protocol")=="local" {("models","name")}else{("data","id")};
    let list=match response.get(key)? {Arr(list)=>list,_=>return Err("Server responded, but its model-list format does not match the selected API format".into())};
    let target=text(p,"model");
    let present=list.iter().any(|item|{let name=text(item,field);name==target || (text(p,"protocol")=="local" && !target.contains(':') && name==format!("{target}:latest"))});
    Ok(if present {format!("Server reachable. Model {target} is listed. Return to Chat to send a message.")}else{format!("Server reachable; {} models listed. {target} was not listed. Check the model name; some servers hide or alias models. Generation has not been tested.",list.len())})
}

/// Add validated original image bytes to already-normalized provider messages.
/// No capability is inferred from a model name; server rejection stays explicit.
pub fn apply_attachments(body:&mut Json,source:&[Json],protocol:&str)->Result<()> {
    let encoded=attachments::encoded_bytes(source)?;
    if encoded==0{return Ok(());}
    if !["chat","local","messages","content"].contains(&protocol){return Err("This API cannot transport image attachments".into());}
    // Work transactionally so a malformed projection cannot partially replace text.
    let mut next=body.clone();
    let key=if protocol=="content"{"contents"}else{"messages"};
    let fields=match &mut next{Obj(v)=>v,_=>return Err("Invalid provider body".into())};
    let projected=match fields.get_mut(key){Some(Arr(v))=>v,_=>return Err("Missing provider messages".into())};
    let expected=source.iter().filter(|m|protocol=="chat"||protocol=="local"||text(m,"role")!="system").count();
    if projected.len()!=expected{return Err("Provider message projection does not match attachments".into());}
    let mut index=0;
    for message in source{
        if text(message,"role")=="system"&&!["chat","local"].contains(&protocol){continue;}
        let images=attachments::images(message)?;
        let target=match &mut projected[index]{Obj(v)=>v,_=>return Err("Invalid provider message".into())};index+=1;
        if images.is_empty(){continue;}
        let original=field(message,"content")?;
        match protocol{
            "local"=>{target.insert("images".into(),Arr(images.iter().map(|v|s(text(v,"data"))).collect()));},
            "chat"=>{
                let mut parts=vec![obj(vec![("type",s("text")),("text",s(original))])];
                for image in images{parts.push(obj(vec![("type",s("image_url")),("image_url",obj(vec![("url",s(&format!("data:{};base64,{}",text(&image,"mime"),text(&image,"data"))))]))]));}
                target.insert("content".into(),Arr(parts));
            },
            "messages"=>{
                let mut parts=Vec::new();for image in images{parts.push(obj(vec![("type",s("image")),("source",obj(vec![("type",s("base64")),("media_type",s(text(&image,"mime"))),("data",s(text(&image,"data")))]))]));}
                if !original.is_empty(){parts.push(obj(vec![("type",s("text")),("text",s(original))]));}
                target.insert("content".into(),Arr(parts));
            },
            "content"=>{
                let mut parts=vec![obj(vec![("text",s(original))])];for image in images{parts.push(obj(vec![("inlineData",obj(vec![("mimeType",s(text(&image,"mime"))),("data",s(text(&image,"data")))]))]));}
                target.insert("parts".into(),Arr(parts));
            },_=>unreachable!()
        }
    }
    if next.string().len()>4*1024*1024{return Err("Request including images exceeds 4 MiB".into());}
    *body=next;Ok(())
}

use super::*;
/// Provider metadata describes interruption; generated prose cannot set it.
pub const EMPTY_REPLY:&str="Server returned no text response";
pub(super) const DOCUMENT_MEMORY: &str = "Insufficient memory to complete the provider document";

pub(super) fn append_document_text(out: &mut String, text: &str) -> Result<()> {
    out.try_reserve(text.len()).map_err(|_| DOCUMENT_MEMORY)?;
    out.push_str(text);
    Ok(())
}

pub(super) fn copy_document_text(text: &str) -> Result<String> {
    let mut out = String::new();
    append_document_text(&mut out, text)?;
    Ok(out)
}

/// The completion document is separate from the bounded request and wire parsers.
pub(super) fn parse_stream_document(source: &str) -> Result<Json> {
    let mut parser = Parser { b: source.as_bytes(), i: 0 };
    let value = stream_value(&mut parser, 0)?;
    parser.ws();
    if parser.i != parser.b.len() { return Err("Trailing JSON data".into()); }
    Ok(value)
}

fn stream_string(parser: &mut Parser<'_>) -> Result<String> {
    if !parser.take(b'"') { return Err("Expected quoted text".into()); }
    let mut out = String::new();
    loop {
        let start = parser.i;
        while matches!(parser.b.get(parser.i), Some(32..=255)) &&
            !matches!(parser.b[parser.i], b'"' | b'\\') {
            parser.i += 1;
        }
        let run = std::str::from_utf8(&parser.b[start..parser.i]).map_err(|_| "Invalid UTF-8")?;
        append_document_text(&mut out, run)?;
        let next = *parser.b.get(parser.i).ok_or("Unterminated text")?;
        parser.i += 1;
        match next {
            b'"' => return Ok(out),
            b'\\' => {}
            _ => return Err("Unescaped control character".into()),
        }
        let escape = *parser.b.get(parser.i).ok_or("Missing escape")?;
        parser.i += 1;
        let scalar = match escape {
            b'"' | b'\\' | b'/' => char::from(escape),
            b'b' => '\x08',
            b'f' => '\x0c',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let mut scalar = parser.hex()?;
                if (0xd800..=0xdbff).contains(&scalar) {
                    if parser.b.get(parser.i..parser.i + 2) != Some(b"\\u") {
                        return Err("Missing surrogate".into());
                    }
                    parser.i += 2;
                    let low = parser.hex()?;
                    if !(0xdc00..=0xdfff).contains(&low) { return Err("Invalid surrogate".into()); }
                    scalar = 0x10000 + ((scalar - 0xd800) << 10) + (low - 0xdc00);
                }
                char::from_u32(scalar).ok_or("Invalid unicode scalar")?
            }
            _ => return Err("Invalid escape".into()),
        };
        append_document_text(&mut out, scalar.encode_utf8(&mut [0; 4]))?;
    }
}

fn stream_value(parser: &mut Parser<'_>, depth: usize) -> Result<Json> {
    if depth > 64 { return Err("JSON nesting limit exceeded".into()); }
    parser.ws();
    match parser.b.get(parser.i) {
        Some(b'"') => Ok(Str(stream_string(parser)?)),
        Some(b'{') => {
            parser.i += 1;
            let mut fields = BTreeMap::new();
            if !parser.take(b'}') {
                loop {
                    let key = stream_string(parser)?;
                    if !parser.take(b':') { return Err("Missing colon".into()); }
                    let value = stream_value(parser, depth + 1)?;
                    if fields.insert(key, value).is_some() { return Err("Duplicate JSON field".into()); }
                    if parser.take(b'}') { break; }
                    if !parser.take(b',') { return Err("Missing comma".into()); }
                }
            }
            Ok(Obj(fields))
        }
        Some(b'[') => {
            parser.i += 1;
            let mut items = Vec::new();
            if !parser.take(b']') {
                loop {
                    let value = stream_value(parser, depth + 1)?;
                    items.try_reserve(1).map_err(|_| DOCUMENT_MEMORY)?;
                    items.push(value);
                    if parser.take(b']') { break; }
                    if !parser.take(b',') { return Err("Missing comma".into()); }
                }
            }
            Ok(Arr(items))
        }
        Some(b't') => parser.literal(b"true", Bool(true)),
        Some(b'f') => parser.literal(b"false", Bool(false)),
        Some(b'n') => parser.literal(b"null", Null),
        Some(b'-' | b'0'..=b'9') => {
            let start = parser.i;
            while matches!(parser.b.get(parser.i), Some(b'-' | b'+' | b'.' | b'0'..=b'9' | b'e' | b'E')) {
                parser.i += 1;
            }
            let token = std::str::from_utf8(&parser.b[start..parser.i]).map_err(|_| "Invalid number")?;
            if !stream::plain_number(token) { return Err("Invalid number".into()); }
            Ok(Num(copy_document_text(token)?))
        }
        _ => Err("Invalid JSON value".into()),
    }
}

pub(super) fn serialize_stream_document(value: &Json) -> Result<String> {
    let mut out = String::new();
    write_stream_value(&mut out, value)?;
    Ok(out)
}

fn write_stream_string(out: &mut String, text: &str) -> Result<()> {
    append_document_text(out, "\"")?;
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        let mut hex = *b"\\u0000";
        let escaped = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..=31 => {
                const DIGITS: &[u8] = b"0123456789abcdef";
                hex[4] = DIGITS[usize::from(byte >> 4)];
                hex[5] = DIGITS[usize::from(byte & 15)];
                std::str::from_utf8(&hex).expect("ASCII escape")
            }
            _ => continue,
        };
        append_document_text(out, &text[start..index])?;
        append_document_text(out, escaped)?;
        start = index + 1;
    }
    append_document_text(out, &text[start..])?;
    append_document_text(out, "\"")
}

fn write_stream_value(out: &mut String, value: &Json) -> Result<()> {
    match value {
        Null => append_document_text(out, "null"),
        Bool(value) => append_document_text(out, if *value { "true" } else { "false" }),
        Num(value) => append_document_text(out, value),
        Str(value) => write_stream_string(out, value),
        Arr(items) => {
            append_document_text(out, "[")?;
            for (index, item) in items.iter().enumerate() {
                if index > 0 { append_document_text(out, ",")?; }
                write_stream_value(out, item)?;
            }
            append_document_text(out, "]")
        }
        Obj(fields) => {
            append_document_text(out, "{")?;
            for (index, (key, value)) in fields.iter().enumerate() {
                if index > 0 { append_document_text(out, ",")?; }
                write_stream_string(out, key)?;
                append_document_text(out, ":")?;
                write_stream_value(out, value)?;
            }
            append_document_text(out, "}")
        }
    }
}
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
 match value.get(key){Ok(Null)|Err(_)=>Ok(String::new()),Ok(value)=>copy_document_text(value.text()?)}
}
pub fn reply(protocol:&str,source:&str)->Result<Reply>{
 let mut body=parse(source)?;
 stream_reply(protocol, &mut body)
}

pub(super) fn stream_reply(protocol: &str, body: &mut Json) -> Result<Reply> {
 if let Ok(error)=body.get("error"){if error!=&Null{
     let mut message=String::from("Server error: ");
     write_stream_value(&mut message,error)?;
     return Err(message);
 }}
 stream::adopt_namespaced_calls(protocol,body);
 let (text,reason)=match protocol{
  "chat"=>{let choice=super::provider_calls::choice(body)?;(visible(choice.get("message")?,"content")?,stop(choice,"finish_reason"))},
  "local"=>(visible(body.get("message")?,"content")?,stop(body,"done_reason")),
  "responses"=>{let (text,reason)=hosted_search::responses_text(body)?;(text,Some(reason))},
  "messages"=>{
      let mut text=String::new();
      for (index, part) in body.get("content")?.array()?.iter().filter(|part| field(part,"type")==Ok("text")).enumerate() {
          if index > 0 { append_document_text(&mut text,"\n")?; }
          append_document_text(&mut text,field(part,"text")?)?;
      }
      (text,stop(body,"stop_reason"))
  },
  "content"=>{let text=stream::content_text(body)?;let candidate=body.get("candidates")?.array()?.iter().find(|c|c.get("index").unwrap_or(&Num("0".into()))==&Num("0".into())).ok_or("No response candidate zero")?;(text,stop(candidate,"finishReason"))},
  "speech"=>return Err("Full-duplex speech returns audio and text on the open microphone stream".into()),
  _=>return Err("Unknown API format".into())
 };
 let interrupted=reason.as_ref().is_some_and(|reason|limit_reason(protocol,reason));
 let proposal=if interrupted||protocol=="responses"{None}else{super::provider_calls::proposal(protocol,body)?};
 if text.trim().is_empty()&&!interrupted&&proposal.is_none(){return Err(EMPTY_REPLY.into());}
 if proposal.is_some() && match protocol{"chat"=>reason.as_deref()!=Some("tool_calls"),"messages"=>reason.as_deref()!=Some("tool_use"),"content"=>reason.as_deref()!=Some("STOP"),"local"=>body.get("done")!=Ok(&Bool(true)),_=>false}{return Err("Tool proposal has no completed provider turn".into());}
 Ok(Reply{text,stop_reason:reason,interrupted,proposal})
}

/// Bind the validated calls directly to the streamed turn. The conversation
/// retains the provider wire fields for the next request.
pub(super) fn bind_stream_task_chat(proposal: Json, response: &Json, connection: &str, turn: &str) -> Result<Json> {
    let message = super::provider_calls::choice(response)?.get("message")?;
    let calls = message.get("tool_calls")?.array()?;
    let count = calls.len();
    let mut proposals = proposal;
    let entries = match &mut proposals {
        Arr(entries) => entries.as_mut_slice(),
        single => std::slice::from_mut(single),
    };
    if count != entries.len() { return Err("Chat tool calls differ from their proposals".into()); }
    let mut ids = std::collections::BTreeSet::new();
    for call in calls {
        let Ok(id) = field(call, "id") else { return Ok(proposals); };
        if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) || !ids.insert(id) {
            return Ok(proposals);
        }
    }
    for (index, (call, proposed)) in calls.iter().zip(entries).enumerate() {
        let Obj(fields) = proposed else { continue; };
        let mut exchange = vec![
            ("connection", s(connection)), ("turn", s(turn)),
            ("count", Num(count.to_string())), ("wire", call.clone()),
        ];
        if index == 0 {
            let content = match message.get("content").unwrap_or(&Null) {
                Str(text) => Str(copy_document_text(text)?),
                value => value.clone(),
            };
            let mut assistant = vec![("role", s("assistant")), ("content", content)];
            for key in ["reasoning_content", "reasoning_details"] {
                if let Ok(detail) = message.get(key) {
                    if detail != &Null && serialize_stream_document(detail)?.len() <= 65536 {
                        assistant.push((key, detail.clone()));
                    }
                }
            }
            exchange.push(("assistant", obj(assistant)));
        }
        fields.insert("task_chat_call".into(), obj(exchange));
    }
    Ok(proposals)
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

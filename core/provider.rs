use super::*;
fn text<'a>(p: &'a Json, k: &str) -> &'a str {
    p.get(k).and_then(Json::text).unwrap_or("")
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
pub fn normalized(p: &Json) -> Result<Json> {
    let protocol = text(p, "protocol");
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
    if protocol=="chat" && (endpoint.ends_with("/api/chat") || endpoint.ends_with("/api/generate") || (endpoint.ends_with("/completions") && !endpoint.ends_with("/chat/completions"))) {return Err("This URL uses a different API. For OpenAI chat, enter the base URL ending /v1 or the complete /v1/chat/completions URL.".into());}
    let mut out = p.clone();
    if let Obj(fields) = &mut out {
        fields.insert("plain".into(),Bool(plain));
        fields.insert("endpoint".into(), s(&endpoint));
        fields.insert("model".into(), s(model));
        fields.insert("name".into(),s(text(p,"name").trim()));
    }
    authorization(&out)?;
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
    if name.len() > 64
        || name.is_empty()
        || name
            .bytes()
            .any(|b| !b.is_ascii_alphanumeric() && b != b'-')
        || [
            "host",
            "connection",
            "content-length",
            "transfer-encoding",
            "content-type",
            "accept-encoding",
        ]
        .contains(&name.to_ascii_lowercase().as_str())
        || prefix.len() > 64
        || key.chars().chain(prefix.chars()).any(char::is_control)
    {
        return Err("Invalid authentication header or prefix".into());
    }
    Ok(Some((
        name.into(),
        if prefix.is_empty() {
            key.into()
        } else {
            format!("{prefix} {key}")
        },
    )))
}
/// Probe the selected server without adding chat messages or sending prompts.
pub fn probe(p: &Json) -> Result<Json> {
    let endpoint=text(p,"endpoint");
    let (suffix,replacement)=match text(p,"protocol") {
        "local" => ("/api/chat","/api/tags"),
        "chat" => ("/chat/completions","/models"),
        _ => return Err("This API format has no standard model-list probe. Send a message to test generation.".into()),
    };
    let prefix=endpoint.strip_suffix(suffix).ok_or("Custom API path has no standard model-list URL. Send a message to test generation.")?;
    let auth=authorization(p)?;
    let headers=auth.iter().map(|(name,value)|(name.as_str(),value.clone())).collect();
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

#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn remote_hosts_and_probe_paths_preserve_user_destination() {
        for host in ["192.168.50.7:11434","10.2.0.8:11434","home-server.local:11434","[fd12::7]:11434"] {
            let profile=normalized(&obj(vec![("protocol",s("local")),("endpoint",s(host)),("plain",Bool(true)),("model",s("custom-model"))])).unwrap();
            let effect=probe(&profile).unwrap();
            assert_eq!(effect.get("host").unwrap().text().unwrap(),host.trim_end_matches(":11434").trim_start_matches('[').trim_end_matches(']'));
            assert!(text(&effect,"wire").starts_with("GET /api/tags HTTP/1.1"));
            assert!(!text(&effect,"wire").contains("custom-model"));
        }
        let p=normalized(&obj(vec![("protocol",s("chat")),("endpoint",s("https://server.example/proxy/v1/chat/completions")),("model",s("custom")),("key",s("fixture-key"))])).unwrap();
        let e=probe(&p).unwrap();assert!(text(&e,"wire").starts_with("GET /proxy/v1/models HTTP/1.1"));assert!(text(&e,"wire").contains("Authorization: Bearer fixture-key"));
        assert!(probe_result(&p,r#"{"data":[{"id":"custom"}]}"#).unwrap().contains("is listed"));
        assert!(probe_result(&p,"<html>not an API</html>").is_err());
        assert!(probe_result(&p,r#"{"data":[]}"#).unwrap().contains("was not listed"));
    }
    #[test]
    fn http_requires_visible_explicit_profile_consent() {
        assert!(normalized(&obj(vec![("protocol",s("local")),("endpoint",s("http://192.168.1.2:11434")),("model",s("model"))])).is_ok());
    }
    #[test]
    fn bare_local_address() {
        let p = obj(vec![
            ("protocol", s("local")),
            ("endpoint", s("192.168.1.9:11434")),
            ("plain", Bool(true)),
            ("model", s("manual-model")),
        ]);
        let n = normalized(&p).unwrap();
        assert_eq!(text(&n, "endpoint"), "http://192.168.1.9:11434/api/chat");
        assert_eq!(text(&n, "name"), "");
    }
    #[test]
    fn local_scheme_inference_preserves_explicit_transport() {
        for endpoint in ["100.100.20.30:8080/v1", "100.64.0.1:8080/v1", "100.127.255.254:8080/v1", "192.168.1.9:8080/v1", "10.0.0.2:8080/v1", "172.16.0.2:8080/v1", "127.0.0.1:8080/v1", "localhost:8080/v1", "server.local:8080/v1", "[fd7a:115c:a1e0::1]:8080/v1", "[::1]:8080/v1"] {
            let n=normalized(&obj(vec![("protocol",s("chat")),("endpoint",s(endpoint)),("plain",Bool(false)),("model",s("m"))])).unwrap();
            assert_eq!(text(&n,"endpoint"),format!("http://{endpoint}/chat/completions"));
            assert_eq!(n.get("plain"),Ok(&Bool(true)));
            assert!(!network::url(text(&n,"endpoint"),true).unwrap().tls);
        }
        for endpoint in ["api.example/v1", "100.63.255.255:8080/v1", "100.128.0.1:8080/v1", "172.32.0.1:8080/v1", "server.local.evil/v1"] {
            let n=normalized(&obj(vec![("protocol",s("chat")),("endpoint",s(endpoint)),("model",s("m"))])).unwrap();
            assert!(text(&n,"endpoint").starts_with("https://"));
        }
        for endpoint in ["https://100.100.20.30:8080/v1/chat/completions?route=x", "http://api.example/custom?route=x"] {
            let n=normalized(&obj(vec![("protocol",s("local")),("endpoint",s(endpoint)),("plain",Bool(true)),("model",s("m"))])).unwrap();
            assert_eq!(text(&n,"endpoint"),endpoint);
            assert_eq!(n.get("plain"),Ok(&Bool(endpoint.starts_with("http://"))));
        }
        let n=normalized(&obj(vec![("protocol",s("local")),("endpoint",s("my-model-host:11434")),("model",s("m"))])).unwrap();
        assert_eq!(text(&n,"endpoint"),"http://my-model-host:11434/api/chat");
    }
    #[test]
    fn custom_authentication() {
        let p = obj(vec![
            ("protocol", s("chat")),
            ("key", s("secret")),
            ("auth_header", s("api-key")),
        ]);
        assert_eq!(
            authorization(&p).unwrap(),
            Some(("api-key".into(), "secret".into()))
        );
        let p = obj(vec![("key", s("secret")), ("auth_header", s("Host"))]);
        assert!(authorization(&p).is_err());
    }
}

#[cfg(test)]
mod content_checks {
 use super::*;
 #[test]fn content_transport_preserves_configuration(){
  let base="https://server.example/proxy/models/custom%2Fname:generateContent?key=private&route=chosen";
  let target=content_endpoint(base,true).unwrap();assert_eq!(target,"https://server.example/proxy/models/custom%2Fname:streamGenerateContent?key=private&route=chosen&alt=sse");assert_eq!(content_endpoint(&target,false).unwrap(),base);
  assert_eq!(content_endpoint("https://server.example/custom?route=x",true).unwrap(),"https://server.example/custom?route=x&alt=sse");
  assert_eq!(content_endpoint("https://server.example/custom?route=x&alt=sse",false).unwrap(),"https://server.example/custom?route=x");
  assert_eq!(content_endpoint("https://server.example/custom?alt=json",false).unwrap(),"https://server.example/custom?alt=json");
  assert!(content_endpoint("https://server.example/custom?alt=json",true).is_err());assert!(content_endpoint("https://server.example/custom?alt=sse&%61lt=sse",true).is_err());
 }
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

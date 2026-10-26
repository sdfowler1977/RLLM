use super::*;
fn text<'a>(p: &'a Json, k: &str) -> &'a str {
    p.get(k).and_then(Json::text).unwrap_or("")
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
    let plain = text(p,"endpoint").trim().starts_with("http://") || p.get("plain") == Ok(&Bool(true));
    let input = text(p, "endpoint").trim();
    if input.is_empty() || input.len() > 2048 {
        return Err("Enter a server address of at most 2048 bytes".into());
    }
    let mut endpoint = if input.contains("://") {
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

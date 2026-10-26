use super::*;
fn value<'a>(j: &'a Json, key: &str) -> &'a Json {
    if let Obj(o) = j {
        o.get(key).unwrap_or(&Null)
    } else {
        &Null
    }
}
fn text(j: &Json, key: &str) -> String {
    value(j, key).text().unwrap_or("").into()
}
fn reply(code: u16, body: Json) -> Json {
    let body = body.string();
    s(&format!("HTTP/1.1 {code} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{}",match code{200=>"OK",400=>"Bad Request",401=>"Unauthorized",404=>"Not Found",429=>"Too Many Requests",_=>"Bad Gateway"},body.len(),body))
}
fn error(code: u16, message: &str) -> Json {
    obj(vec![(
        "reply",
        reply(
            code,
            obj(vec![("error", obj(vec![("message", s(message))]))]),
        ),
    )])
}
fn headers(raw: &str) -> Result<(String, String, BTreeMap<String, String>)> {
    if raw.len() > 16384 {
        return Err("Headers exceed 16 KiB".into());
    }
    let mut lines = raw.split("\r\n");
    let mut first = lines
        .next()
        .ok_or("Missing request line")?
        .split_whitespace();
    let method = first.next().ok_or("Missing method")?.to_string();
    let path = first.next().ok_or("Missing path")?.to_string();
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err("Expected HTTP/1.1".into());
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once(':').ok_or("Invalid header")?;
        if fields
            .insert(k.to_ascii_lowercase(), v.trim().into())
            .is_some()
        {
            return Err("Duplicate header".into());
        }
    }
    if fields.contains_key("transfer-encoding") {
        return Err(
            "Send requests with Content-Length; chunked request bodies are not supported".into(),
        );
    }
    Ok((method, path, fields))
}
pub fn size(v: &Json) -> Result<Json> {
    let (_, _, h) = headers(field(v, "header")?)?;
    let n = h
        .get("content-length")
        .map(|v| v.parse::<usize>())
        .transpose()
        .map_err(|_| "Invalid body size")?
        .unwrap_or(0);
    if n > 1024 * 1024 {
        return Err("Request exceeds 1 MiB".into());
    }
    Ok(Num(n.to_string()))
}
pub fn incoming(v: &Json) -> Result<Json> {
    let (method, path, headers) = headers(field(v, "header")?)?;
    let config = v.get("config")?;
    let expected = format!("Bearer {}", field(config, "token")?);
    let supplied = headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let mut mismatch = expected.len() ^ supplied.len();
    for (i, b) in expected.bytes().enumerate() {
        mismatch |= (b ^ supplied.as_bytes().get(i).copied().unwrap_or(0)) as usize;
    }
    if mismatch != 0 {
        return Ok(error(401, "A valid gateway bearer token is required"));
    }
    let profiles = config.get("profiles")?.array()?;
    if method == "GET" && path == "/health" {
        return Ok(obj(vec![(
            "reply",
            reply(
                200,
                obj(vec![
                    ("status", s("listening")),
                    ("configured_profiles", Num(profiles.len().to_string())),
                ]),
            ),
        )]));
    }
    if method == "GET" && path == "/v1/models" {
        let mut models = BTreeMap::new();
        for p in profiles {
            let id = text(p, "model");
            models.entry(id.clone()).or_insert(obj(vec![
                ("id", s(&id)),
                ("object", s("model")),
                ("owned_by", s("user")),
            ]));
        }
        return Ok(obj(vec![(
            "reply",
            reply(
                200,
                obj(vec![
                    ("object", s("list")),
                    ("data", Arr(models.into_values().collect())),
                ]),
            ),
        )]));
    }
    if method != "POST" || path != "/v1/chat/completions" {
        return Ok(error(
            404,
            "Supported routes: /health, /v1/models, /v1/chat/completions",
        ));
    }
    let body = parse(field(v, "body")?)?;
    if let Obj(fields) = &body {
        if fields
            .keys()
            .any(|k| !["model", "messages", "stream"].contains(&k.as_str()))
        {
            return Ok(error(
                400,
                "This checkpoint accepts model, messages and stream only",
            ));
        }
    }
    if value(&body, "stream") == &Bool(true) {
        return Ok(error(400, "This checkpoint supports stream:false only"));
    }
    let model = field(&body, "model")?;
    let profile = match profiles.iter().find(|p| text(p, "model") == model) {
        Some(p) => p,
        None => return Ok(error(404, "Model is not configured in a server profile")),
    };
    let protocol = field(profile, "protocol")?;
    let messages = body.get("messages")?.clone();
    let request = request(&obj(vec![
        ("protocol", s(protocol)),
        ("model", s(model)),
        ("messages", messages),
    ]))?
    .string();
    let authentication = super::provider::authorization(profile)?;
    let mut headers = vec![("Content-Type", "application/json; charset=utf-8".into())];
    if let Some((name, value)) = &authentication {
        headers.push((name.as_str(), value.clone()));
    }
    if protocol == "messages" {
        headers.push(("anthropic-version", "2023-06-01".into()))
    }
    let endpoint = text(profile, "endpoint").replace("{model}", &network::encoded(model));
    let upstream = network::wire(
        &endpoint,
        value(profile, "plain") == &Bool(true),
        "POST",
        &request,
        headers,
    )?;
    Ok(obj(vec![
        ("upstream", upstream),
        ("protocol", s(protocol)),
        ("model", s(model)),
    ]))
}
pub fn outgoing(v: &Json) -> Result<Json> {
    let bytes = network::unbase64(field(v, "data")?)?;
    let (_, body) = network::decoded(&bytes)?;
    let protocol = field(v, "protocol")?;
    let content = response(&obj(vec![("protocol", s(protocol)), ("body", s(&body))]))?;
    let original = parse(&body)?;
    let reason = match protocol {
        "chat" => original
            .get("choices")
            .and_then(Json::array)
            .ok()
            .and_then(|v| v.first())
            .map(|v| value(v, "finish_reason").clone())
            .unwrap_or(Null),
        "local" => match text(&original, "done_reason").as_str() {
            "stop" => s("stop"),
            "length" => s("length"),
            _ => Null,
        },
        "messages" => match text(&original, "stop_reason").as_str() {
            "max_tokens" => s("length"),
            "end_turn" | "stop_sequence" => s("stop"),
            _ => Null,
        },
        "content" => original
            .get("candidates")
            .and_then(Json::array)
            .ok()
            .and_then(|v| v.first())
            .map(|v| match text(v, "finishReason").as_str() {
                "STOP" => s("stop"),
                "MAX_TOKENS" => s("length"),
                "SAFETY" => s("content_filter"),
                _ => Null,
            })
            .unwrap_or(Null),
        _ => Null,
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(reply(
        200,
        obj(vec![
            ("id", s(&format!("chat-{}", timestamp.as_nanos()))),
            ("object", s("chat.completion")),
            ("created", Num(timestamp.as_secs().to_string())),
            ("model", s(field(v, "model")?)),
            (
                "choices",
                Arr(vec![obj(vec![
                    ("index", Num("0".into())),
                    (
                        "message",
                        obj(vec![("role", s("assistant")), ("content", content)]),
                    ),
                    ("finish_reason", reason),
                ])]),
            ),
        ]),
    ))
}
pub fn failure(v: &Json) -> Result<Json> {
    Ok(reply(
        502,
        obj(vec![(
            "error",
            obj(vec![("message", s(field(v, "message")?))]),
        )]),
    ))
}
#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn protected_routes() {
        let config = obj(vec![("token", s("secret")), ("profiles", Arr(vec![]))]);
        let request = obj(vec![
            ("config", config.clone()),
            (
                "header",
                s("GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n"),
            ),
            ("body", s("")),
        ]);
        assert!(text(&incoming(&request).unwrap(), "reply").contains("401"));
        let request = obj(vec![
            ("config", config),
            (
                "header",
                s("GET /health HTTP/1.1\r\nAuthorization: Bearer secret\r\n\r\n"),
            ),
            ("body", s("")),
        ]);
        assert!(text(&incoming(&request).unwrap(), "reply").contains("200 OK"));
    }
    #[test]
    fn request_limits() {
        assert!(size(&obj(vec![(
            "header",
            s("POST / HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n")
        )]))
        .is_err());
        assert!(size(&obj(vec![(
            "header",
            s("POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n")
        )]))
        .is_err());
    }
}

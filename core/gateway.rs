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
    let challenge=if code==401 {"WWW-Authenticate: Bearer realm=\"RLLM\"\r\n"}else{""};
    s(&format!("HTTP/1.1 {code} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n{challenge}\r\n{}",match code{200=>"OK",400=>"Bad Request",401=>"Unauthorized",404=>"Not Found",417=>"Expectation Failed",429=>"Too Many Requests",_=>"Bad Gateway"},body.len(),body))
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
// Both platform listeners and direct shared-core dispatch use this parser.
// Refuse ambiguous framing before it can select an operation or credentials.
fn token(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphanumeric()
        || b"!#$%&'*+-.^_`|~".contains(&b))
}
fn headers(raw: &str) -> Result<(String, String, BTreeMap<String, String>)> {
    if raw.len() > 16384 { return Err("Headers exceed 16 KiB".into()); }
    let block = raw.strip_suffix("\r\n\r\n").ok_or("Incomplete HTTP header block")?;
    let mut lines = block.split("\r\n");
    let mut first = lines.next().ok_or("Missing request line")?.split(' ');
    let method = first.next().ok_or("Missing method")?;
    let path = first.next().ok_or("Missing path")?;
    if !token(method) || path.is_empty() || !path.bytes().all(|b| (33..=126).contains(&b))
        || first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err("Invalid HTTP/1.1 request line".into());
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        let (k, v) = line.split_once(':').ok_or("Invalid header")?;
        if !token(k) || !v.bytes().all(|b| b == b'\t' || (32..=126).contains(&b)) {
            return Err("Invalid HTTP header field".into());
        }
        if fields.insert(k.to_ascii_lowercase(), v.trim_matches([' ', '\t']).into()).is_some() {
            return Err("Duplicate header".into());
        }
    }
    if fields.contains_key("transfer-encoding") {
        return Err("Send requests with Content-Length; chunked request bodies are not supported".into());
    }
    Ok((method.into(), path.into(), fields))
}
fn body_size(headers: &BTreeMap<String, String>) -> Result<usize> {
    let n = match headers.get("content-length") {
        Some(value) => {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Invalid body size".into());
            }
            value.parse::<usize>().map_err(|_| "Invalid body size")?
        },
        None => 0,
    };
    if n > 1024 * 1024 { return Err("Request exceeds 1 MiB".into()); }
    Ok(n)
}
pub fn size(v: &Json) -> Result<Json> {
    let (_, _, headers) = headers(field(v, "header")?)?;
    Ok(Num(body_size(&headers)?.to_string()))
}
fn authenticated(headers: &BTreeMap<String, String>, config: &Json) -> Result<bool> {
    let expected = format!("Bearer {}", field(config, "token")?);
    let supplied = headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let mut mismatch = expected.len() ^ supplied.len();
    for (i, b) in expected.bytes().enumerate() {
        mismatch |= (b ^ supplied.as_bytes().get(i).copied().unwrap_or(0)) as usize;
    }
    Ok(mismatch == 0)
}
fn unsupported_expectation(headers: &BTreeMap<String, String>) -> bool {
    headers.get("expect").map(|v| !v.split(',').all(|part|
        part.trim_matches([' ', '\t']).eq_ignore_ascii_case("100-continue"))).unwrap_or(false)
}
/// Header-only transport admission. This never dispatches an application task.
/// Final refusals precede a body read; an interim response precedes client upload.
pub fn preflight(v: &Json) -> Result<Json> {
    let (_, _, headers) = headers(field(v, "header")?)?;
    let size = body_size(&headers)?;
    if !authenticated(&headers, v.get("config")?)? {
        return Ok(error(401, "A valid gateway bearer token is required"));
    }
    if unsupported_expectation(&headers) {
        return Ok(error(417, "Only the 100-continue expectation is supported"));
    }
    let interim = if size > 0 && headers.contains_key("expect") {
        "HTTP/1.1 100 Continue\r\n\r\n"
    } else { "" };
    Ok(obj(vec![("size", Num(size.to_string())), ("interim", s(interim))]))
}
pub fn incoming(v: &Json) -> Result<Json> {
    let (method, path, headers) = headers(field(v, "header")?)?;
    if field(v, "body")?.len() != body_size(&headers)? {
        return Err("HTTP body does not match Content-Length".into());
    }
    let config = v.get("config")?;
    if !authenticated(&headers, config)? {
        return Ok(error(401, "A valid gateway bearer token is required"));
    }
    if unsupported_expectation(&headers) {
        return Ok(error(417, "Only the 100-continue expectation is supported"));
    }
    if let Some(response)=super::a2a::incoming(config,&method,&path,&headers,field(v,"body")?)? { return Ok(obj(vec![("reply",s(&response))])); }
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
        let global=super::application::gateway_policy_current(config)?;if let Err(reason)=super::application::gateway_policy_check(&global,"observation",&obj(vec![])){return Ok(error(400,&reason));}
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
    let endpoint=text(profile,"endpoint").replace("{model}",&network::encoded(model));
    let global=super::application::gateway_policy_current(config)?;if let Err(reason)=super::application::gateway_policy_check(&global,"model",&obj(vec![("endpoint",s(&endpoint))])){return Ok(error(400,&reason));}
    let messages=Arr(super::application::gateway_policy_messages(&global,body.get("messages")?.array()?.clone()));
    let request = request(&obj(vec![
        ("protocol", s(protocol)),
        ("model", s(model)),
        ("messages", messages),
    ]))?
    .string();
    let authentication = super::provider::authorization(profile)?;
    let mut headers = vec![("Accept", "application/json".into()), ("Content-Type", "application/json; charset=utf-8".into())];
    if let Some((name, value)) = &authentication {
        headers.push((name.as_str(), value.clone()));
    }
    if protocol == "messages" {
        headers.push(("anthropic-version", "2023-06-01".into()))
    }
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
    let code = if value(v, "request") == &Bool(true) { 400 } else { 502 };
    Ok(reply(
        code,
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
    #[test]
    fn malformed_framing_refuses_before_routing() {
        for raw in [
            "POST / HTTP/1.1\r\nContent-Length : 1\r\n\r\n",
            "POST / HTTP/1.1\r\nContent-Length: +1\r\n\r\n",
            "POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
            "POST / HTTP/1.1\r\nContent-Length: \r\n\r\n",
            "POST / HTTP/1.1\r\nContent-Length: 1, 1\r\n\r\n",
            "POST / HTTP/1.1\r\n Transfer-Encoding: chunked\r\n\r\n",
            "GET / HTTP/1.1\r\nX-Test: a\0b\r\n\r\n",
            "GET / HTTP/1.1\r\nX-Test: a\nb\r\n\r\n",
            "GET / HTTP/1.1\r\nX-Test: a\rb\r\n\r\n",
            "GET / HTTP/1.1\r\nX-Test: a\u{7f}b\r\n\r\n",
            "GET / HTTP/1.1\r\n\r\nAuthorization: Bearer late\r\n\r\n",
            "GET / HTTP/1.1\r\nX: y\r\n",
            "GET\t/ HTTP/1.1\r\n\r\n",
            "GET /bad\tpath HTTP/1.1\r\n\r\n",
            "GET / HTTP/1.1\r\nX: é\r\n\r\n",
        ] {
            assert!(size(&obj(vec![("header", s(raw))])).is_err(), "{raw:?}");
        }
    }
    #[test]
    fn decimal_lengths_and_header_tokens() {
        let raw = "POST /a2a HTTP/1.1\r\nX-Valid_!: \tvalue \t\r\ncOnTeNt-LeNgTh:\t0002 \t\r\n\r\n";
        assert_eq!(size(&obj(vec![("header", s(raw))])).unwrap(), Num("2".into()));
        let (_, _, headers) = headers(raw).unwrap();
        assert_eq!(headers.get("x-valid_!").unwrap(), "value");
        assert!(size(&obj(vec![("header", s(&raw.replace("0002", &"9".repeat(100))) )])).is_err());
    }
    #[test]
    fn body_octets_checked_before_authenticated_dispatch() {
        let config = obj(vec![("token", s("secret")), ("profiles", Arr(vec![]))]);
        for (declared, body, allowed) in [("2", "é", true), ("1", "é", false), ("1", "", false), ("0", "x", false)] {
            let raw = format!("GET /health HTTP/1.1\r\nAuthorization: Bearer secret\r\nContent-Length: {declared}\r\n\r\n");
            let request = obj(vec![("config", config.clone()), ("header", s(&raw)), ("body", s(body))]);
            let result = incoming(&request);
            assert_eq!(result.is_ok(), allowed);
            if allowed { assert!(text(&result.unwrap(), "reply").contains("200 OK")); }
        }
        // An invalid request cannot poison the next local operation.
        let request = obj(vec![("config", config), ("header", s("GET /health HTTP/1.1\r\nAuthorization: Bearer secret\r\n\r\n")), ("body", s(""))]);
        assert!(text(&incoming(&request).unwrap(), "reply").contains("200 OK"));
    }

    #[test]
    fn transport_failure_phase_preserves_status() {
        let upstream = obj(vec![("message", s("Upstream unavailable"))]);
        assert!(failure(&upstream).unwrap().text().unwrap().starts_with("HTTP/1.1 502 "));
        let request = obj(vec![("message", s("Invalid request")), ("request", Bool(true))]);
        assert!(failure(&request).unwrap().text().unwrap().starts_with("HTTP/1.1 400 "));
    }

    #[test]
    fn header_only_admission_authenticates_before_continue() {
        let config = obj(vec![("token", s("secret"))]);
        let raw = "POST /a2a HTTP/1.1\r\nAuthorization: Bearer secret\r\nContent-Length: 4\r\nExpect: 100-CoNtInUe\r\n\r\n";
        let request = obj(vec![("config", config.clone()), ("header", s(raw))]);
        let admitted = preflight(&request).unwrap();
        assert_eq!(admitted.get("size").unwrap(), &Num("4".into()));
        assert_eq!(field(&admitted, "interim").unwrap(), "HTTP/1.1 100 Continue\r\n\r\n");
        assert!(admitted.get("reply").is_err());
        let denied = preflight(&obj(vec![("config", config), ("header", s(&raw.replace("Bearer secret", "Bearer wrong")))])).unwrap();
        assert!(field(&denied, "reply").unwrap().starts_with("HTTP/1.1 401 "));
        assert!(denied.get("size").is_err() && denied.get("interim").is_err());
    }
    #[test]
    fn expectation_scope_and_empty_body() {
        let config = obj(vec![("token", s("secret")), ("profiles", Arr(vec![]))]);
        for (expect, length, wanted) in [("", "1", ""), ("Expect: 100-continue\r\n", "0", ""),
            ("Expect: 100-continue, 100-CONTINUE\r\n", "1", "HTTP/1.1 100 Continue\r\n\r\n")] {
            let raw = format!("GET /health HTTP/1.1\r\nAuthorization: Bearer secret\r\nContent-Length: {length}\r\n{expect}\r\n");
            let admitted = preflight(&obj(vec![("config", config.clone()), ("header", s(&raw))])).unwrap();
            assert_eq!(field(&admitted, "interim").unwrap(), wanted);
        }
        for expect in ["fancy-upload", "100-continue, fancy-upload", "100-continue;flag", ""] {
            let raw = format!("GET /health HTTP/1.1\r\nAuthorization: Bearer secret\r\nContent-Length: 0\r\nExpect: {expect}\r\n\r\n");
            let request = obj(vec![("config", config.clone()), ("header", s(&raw)), ("body", s(""))]);
            for result in [preflight(&request).unwrap(), incoming(&request).unwrap()] {
                assert!(field(&result, "reply").unwrap().starts_with("HTTP/1.1 417 Expectation Failed\r\n"));
            }
        }
    }
    #[test]
    fn gateway_route_accepts_json() {
        let body = r#"{"model":"manual-model","messages":[{"role":"user","content":"hello"}]}"#;
        let header = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let config = obj(vec![
            ("token", s("secret")),
            (
                "profiles",
                Arr(vec![obj(vec![
                    ("name", s("Lab")),
                    ("endpoint", s("http://127.0.0.1:9123/v1/chat/completions")),
                    ("model", s("manual-model")),
                    ("protocol", s("chat")),
                    ("plain", Bool(true)),
                ])]),
            ),
        ]);
        let request = obj(vec![
            ("config", config),
            ("header", s(&header)),
            ("body", s(body)),
        ]);
        let got = incoming(&request).unwrap_or_else(|error| panic!("{error}"));
        let raw = text(got.get("upstream").unwrap_or_else(|_| panic!("{}", got.string())), "wire");
        assert!(raw.contains("\r\nAccept: application/json\r\n"), "{raw}");
        assert!(!raw.contains("text/event-stream"), "{raw}");
        assert_eq!(raw.matches("\r\nAccept:").count(), 1, "{raw}");
        assert!(raw.starts_with("POST /v1/chat/completions HTTP/1.1"), "{raw}");
    }
    #[test]
    fn preflight_retains_current_framing_bounds() {
        let config = obj(vec![("token", s("secret"))]);
        for extra in ["Content-Length: +1", "Content-Length: 1048577", "Transfer-Encoding: chunked", "Content-Length: 1\r\nContent-Length: 1"] {
            let raw = format!("POST /a2a HTTP/1.1\r\nAuthorization: Bearer secret\r\nExpect: 100-continue\r\n{extra}\r\n\r\n");
            assert!(preflight(&obj(vec![("config", config.clone()), ("header", s(&raw))])).is_err());
        }
    }

}

mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use twin::{obj, s, Controller, Value};

const SCOPE: &str = "Owner checks with actual local HTTP fixture peers and controlled model responses; no device or live configured provider acceptance.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let project_text = project.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = twin::create_scratch(&project, "services");
    let _cleanup = Cleanup(scratch.clone());
    std::fs::write(scratch.join("evidence.txt"), "actual local file").unwrap_or_else(|error| panic!("evidence: {error}"));
    let scratch_text = scratch.to_str().unwrap_or_else(|| panic!("scratch root is not UTF-8")).to_string();
    let peers = Peer::pair();
    let mut controller = Controller::spawn(&project_text);
    let passed = {
        let mut ctx = Ctx { controller: &mut controller, root: &scratch_text, peers: &peers };
        run(&mut ctx)
    };
    controller.finish();
    write_report(&passed);
}

struct Ctx<'a> {
    controller: &'a mut Controller,
    root: &'a str,
    peers: &'a [Peer],
}

impl Ctx<'_> {
    fn event(&mut self, state: Option<&Value>, action: &str, inputs: Value) -> Value {
        self.controller.event(state, action, inputs, self.root)
    }

    fn configured(&mut self, config: Option<Value>) -> Value {
        let config = config.unwrap_or_else(|| cfg(self.peers));
        let st = state_of(&self.event(None, "resume", obj(vec![]))).clone();
        let opened = self.event(Some(&st), "service_connections", obj(vec![]));
        let saved = self.event(Some(state_of(&opened)), "service_save", obj(vec![("service_config".into(), s(&config.to_string()))]));
        if truthy_at(state_of(&saved).get("notice_error")) {
            panic!("service configuration was rejected: {}", saved.to_string());
        }
        state_of(&saved).clone()
    }

    fn review(&mut self, st: &Value, route: &str, capability: &str, input: Option<Value>) -> Value {
        let opened = self.event(Some(st), "service_request", obj(vec![]));
        let input = input.unwrap_or_else(|| obj(vec![("prompt".into(), s("actual input")), ("number".into(), twin::n("7"))]));
        let shown = self.event(
            Some(state_of(&opened)),
            "service_review",
            obj(vec![
                ("service_route".into(), s(route)),
                ("service_capability".into(), s(capability)),
                ("service_input".into(), s(&input.to_string())),
            ]),
        );
        if as_str(state_of(&shown).get("dialog")) != Some("approval") {
            panic!("service review was not shown: {}", shown.to_string());
        }
        shown
    }

    fn approve_review(&mut self, st: &Value, route: &str, capability: &str) -> Value {
        let reviewed = self.review(st, route, capability, None);
        self.event(Some(state_of(&reviewed)), "approve", obj(vec![]))
    }

    fn send_effect(&mut self, packet: &Value) -> (Value, Value) {
        let effect = packet.get("effect").and_then(present).unwrap_or_else(|| panic!("missing network effect: {}", packet.to_string()));
        if as_str(effect.get("kind")) != Some("network") {
            panic!("expected a network effect: {}", effect.to_string());
        }
        let wire = wire_bytes(effect);
        // The leg indexes e["host"], so a missing host is a KeyError there; a defaulted
    // localhost would turn a malformed effect into a fixture connection.
    let host = as_str(effect.get("host")).unwrap_or_else(|| panic!("missing effect host: {}", effect.to_string()));
        let port = num_u16(effect.get("port")).unwrap_or_else(|| panic!("missing effect port: {}", effect.to_string()));
        let mut stream = TcpStream::connect((host, port)).unwrap_or_else(|error| panic!("fixture connect {host}:{port}: {error}"));
        stream.set_read_timeout(Some(Duration::from_secs(8))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(8))).ok();
        stream.write_all(&wire).unwrap_or_else(|error| panic!("fixture write: {error}"));
        upload(self.controller, effect, &mut stream);
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap_or_else(|error| panic!("fixture read: {error}"));
        let _ = stream.shutdown(Shutdown::Both);
        let mut fields = obj(vec![]);
        for key in ["purpose", "child_ticket", "child_owner"] {
            if let Some(value) = effect.get(key) {
                put(&mut fields, key, value.clone());
            }
        }
        put(&mut fields, "data", s(&twin::b64_encode(&response)));
        let result = self.controller.event(Some(state_of(packet)), "network_result", fields.clone(), self.root);
        (result, fields)
    }

    fn model_reply(&mut self, packet: &Value, format: &str, content: &str) -> Value {
        let body = model_body(format, content);
        let data = body.to_string();
        let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{data}", data.len());
        let mut fields = obj(vec![
            ("purpose".into(), s("model")),
            ("model_ticket".into(), s(as_str(state_of(packet).get("model_expected")).unwrap_or_else(|| panic!("missing model ticket: {}", packet.to_string())))),
            ("data".into(), s(&twin::b64_encode(wire.as_bytes()))),
        ]);
        if let Some(effect) = packet.get("effect").and_then(present) {
            for key in ["child_ticket", "child_owner"] {
                if let Some(value) = effect.get(key) {
                    put(&mut fields, key, value.clone());
                }
            }
        }
        self.event(Some(state_of(packet)), "network_result", fields)
    }
}

fn run(ctx: &mut Ctx) -> Vec<String> {
    let mut passed = Vec::new();
    let fallback_port = ctx.peers[1].port();
    let st = ctx.configured(None);
    let editor = ctx.event(Some(&st), "service_connections", obj(vec![]));
    let modal = modal_of(&editor);
    let masked_text = field_value(modal, "service_config");
    let masked = Value::parse(masked_text);
    keep(&mut passed, "configuration editor masks existing keys", !modal.to_string().contains("fixture-key") && as_str(at(&masked, &["hosts", "0", "key"])) == Some("<unchanged>"));

    let saved = ctx.event(Some(state_of(&editor)), "service_save", obj(vec![("service_config".into(), s(&masked.to_string()))]));
    keep(&mut passed, "masked save retains original credentials", as_str(at(state_of(&saved), &["service_connections", "hosts", "0", "key"])) == Some("fixture-key-0"));

    let mut changed = masked.clone();
    *dig_mut(&mut changed, &["hosts", "0", "url"]) = s("http://changed.invalid");
    let refused = ctx.event(Some(state_of(&editor)), "service_save", obj(vec![("service_config".into(), s(&changed.to_string()))]));
    keep(&mut passed, "host change cannot silently retain credentials", truthy_at(state_of(&refused).get("notice_error")) && state_of(&refused).get("service_connections") == st.get("service_connections"));

    let mut config = cfg(ctx.peers);
    put(dig_mut(&mut config, &["hosts", "0"]), "headers", obj(vec![("X-Extra".into(), s("secret-header"))]));
    let st = ctx.configured(Some(config));
    let editor = ctx.event(Some(&st), "service_connections", obj(vec![]));
    keep(&mut passed, "custom header values are masked", !modal_of(&editor).to_string().contains("secret-header"));

    let mut config = cfg(ctx.peers);
    put(dig_mut(&mut config, &["hosts", "0"]), "headers", obj(vec![("X-Extra".into(), s("one")), ("x-extra".into(), s("two"))]));
    let fresh = state_of(&ctx.event(None, "resume", obj(vec![]))).clone();
    let refused = ctx.event(Some(&fresh), "service_save", obj(vec![("service_config".into(), s(&config.to_string()))]));
    keep(&mut passed, "case variant duplicate headers are refused", truthy_at(state_of(&refused).get("notice_error")));

    let st = ctx.configured(None);
    let d = ctx.review(&st, "main", "", None);
    let shown = modal_of(&d).to_string();
    keep(&mut passed, "review discloses exact destination without host key", shown.contains("/generate") && !shown.contains("fixture-key"));
    let d = ctx.event(Some(state_of(&d)), "approve", obj(vec![]));
    keep(&mut passed, "shared approval dispatches real network effect", as_str(effect_of(&d).and_then(|effect| effect.get("purpose"))).unwrap_or("").starts_with("service_request:"));
    let wrong = ctx.event(Some(state_of(&d)), "network_result", obj(vec![("purpose".into(), s("service_request:wrong")), ("draft".into(), s("injected"))]));
    keep(&mut passed, "wrong callback is exact state no-op", same_state(&wrong, &d));
    let (completed, callback) = ctx.send_effect(&d);
    keep(&mut passed, "typed request mapping executes exact input", body_eq(&ctx.peers[0].last(), &obj(vec![("prompt".into(), s("actual input")), ("number".into(), twin::n("7"))])));
    keep(&mut passed, "host credential sent only to selected server", header(&ctx.peers[0].last(), "X-Token") == Some("fixture-key-0"));
    keep(&mut passed, "response mapping retains actual fixture response", at(state_of(&completed), &["service_result", "value"]) == Some(&obj(vec![("prompt".into(), s("actual input")), ("number".into(), twin::n("7"))])));
    let mut replay = callback;
    put(&mut replay, "draft", s("late"));
    let duplicate = ctx.event(Some(state_of(&completed)), "network_result", replay);
    keep(&mut passed, "duplicate callback is exact state no-op", same_state(&duplicate, &completed));

    let mut config = cfg(ctx.peers);
    *dig_mut(&mut config, &["routes", "0", "path"]) = s("/unavailable");
    let st = ctx.configured(Some(config));
    let d = ctx.approve_review(&st, "", "image_generation");
    let (first, _) = ctx.send_effect(&d);
    keep(&mut passed, "explicit503 advances exactly one configured fallback", effect_of(&first).is_some() && num_is(effect_of(&first).and_then(|effect| effect.get("port")), fallback_port));
    let (finished, _) = ctx.send_effect(&first);
    keep(&mut passed, "fallback has independent host credentials", header(&ctx.peers[0].last(), "X-Token") == Some("fixture-key-1") && as_str(at(state_of(&finished), &["service_result", "route"])) == Some("fallback"));

    let mut config = cfg(ctx.peers);
    *dig_mut(&mut config, &["routes", "0", "path"]) = s("/auth-failed");
    let ready = ctx.configured(Some(config));
    let d = ctx.approve_review(&ready, "", "image_generation");
    let before = ctx.peers[0].len();
    let (failed, _) = ctx.send_effect(&d);
    keep(&mut passed, "unlisted401 is scoped and never cascades", effect_of(&failed).is_none() && ctx.peers[0].len() == before + 1 && as_str(at(state_of(&failed), &["service_result", "status"])) == Some("failed"));

    let mut config = cfg(ctx.peers);
    *dig_mut(&mut config, &["routes", "0", "path"]) = s("/accepted");
    *dig_mut(&mut config, &["routes", "0", "response_path"]) = s("");
    let ready = ctx.configured(Some(config));
    let d = ctx.approve_review(&ready, "", "image_generation");
    let (accepted, _) = ctx.send_effect(&d);
    keep(&mut passed, "202 remains acceptance and no resubmission", effect_of(&accepted).is_none() && as_str(at(state_of(&accepted), &["service_result", "status"])) == Some("accepted"));

    let st = ctx.configured(None);
    let pending = ctx.review(&st, "main", "", None);
    let mut changed = state_of(&pending).clone();
    *dig_mut(&mut changed, &["service_connections", "hosts", "0", "key"]) = s("changed-key");
    let blocked = ctx.event(Some(&changed), "approve", obj(vec![]));
    keep(&mut passed, "changed credentials invalidate displayed review", effect_of(&blocked).is_none() && truthy_at(state_of(&blocked).get("notice_error")));

    let d = ctx.approve_review(&st, "main", "",);
    let cancelled = ctx.event(Some(state_of(&d)), "cancel", obj(vec![]));
    let late = ctx.event(
        Some(state_of(&cancelled)),
        "network_result",
        obj(vec![
            ("purpose".into(), s(as_str(effect_of(&d).and_then(|effect| effect.get("purpose"))).unwrap_or_else(|| panic!("cancelled result cannot change draft or state")))),
            ("data".into(), s("bad")),
            ("draft".into(), s("late")),
        ]),
    );
    keep(&mut passed, "cancelled result cannot change draft or state", same_state(&late, &cancelled));

    let d = ctx.approve_review(&st, "main", "");
    let mut changed = state_of(&d).clone();
    *dig_mut(&mut changed, &["service_connections", "hosts", "0", "key"]) = s("changed");
    let packet = obj(vec![("state".into(), changed), ("effect".into(), effect_of(&d).unwrap().clone())]);
    let (blocked, _) = ctx.send_effect(&packet);
    keep(
        &mut passed,
        "changed callback context withholds all response values",
        as_str(at(state_of(&blocked), &["service_result", "status"])) == Some("failed") && as_str(at(state_of(&blocked), &["service_result", "error"])).unwrap_or("").contains("withheld"),
    );

    for sent in [false, true] {
        let d = ctx.approve_review(&st, "", "image_generation");
        let failed = ctx.event(
            Some(state_of(&d)),
            "failure",
            obj(vec![
                ("purpose".into(), s(as_str(effect_of(&d).and_then(|effect| effect.get("purpose"))).unwrap_or(""))),
                ("sent".into(), twin::b(sent)),
                ("error".into(), s("controlled transport failure")),
            ]),
        );
        let name = if sent { "uncertain sent request never replays" } else { "pre-send fallback" };
        keep(&mut passed, name, effect_of(&failed).is_some() == !sent);
        ctx.event(Some(state_of(&failed)), "cancel", obj(vec![]));
    }

    for mutation in ["header", "duplicate", "pointer", "capability"] {
        let mut config = cfg(ctx.peers);
        match mutation {
            "header" => put(dig_mut(&mut config, &["hosts", "0"]), "headers", obj(vec![("Content-Length".into(), s("7"))])),
            "duplicate" => {
                let copy = config.get("routes").unwrap().arr()[0].clone();
                push_item(dig_mut(&mut config, &["routes"]), copy);
            }
            "pointer" => *dig_mut(&mut config, &["routes", "0", "response_path"]) = s("not-a-pointer"),
            _ => *dig_mut(&mut config, &["routes", "1", "capability"]) = s("other"),
        }
        let opened = ctx.event(Some(&st), "service_connections", obj(vec![]));
        let saved = ctx.event(Some(state_of(&opened)), "service_save", obj(vec![("service_config".into(), s(&config.to_string()))]));
        keep(
            &mut passed,
            &format!("invalid {mutation} rejected without replacing working settings"),
            truthy_at(state_of(&saved).get("notice_error")) && state_of(&saved).get("service_connections") == st.get("service_connections"),
        );
    }

    let mut config = cfg(ctx.peers);
    *dig_mut(&mut config, &["hosts", "0", "key"]) = s("");
    let st_keyless = ctx.configured(Some(config));
    let d = ctx.approve_review(&st_keyless, "main", "");
    let (keyless, _) = ctx.send_effect(&d);
    keep(&mut passed, "keyless hosts work", header(&ctx.peers[0].last(), "X-Token").is_none() && as_str(at(state_of(&keyless), &["service_result", "status"])) == Some("response_received"));

    for format in ["chat", "local", "messages", "content"] {
        let mut model = ctx.configured(None);
        put(&mut model, "profiles", Value::Arr(vec![profile("Controlled", "http://model.test/v1/chat/completions", format, "controlled")]));
        put(&mut model, "agents", Value::Arr(vec![obj(vec![("name".into(), s("General")), ("tools".into(), s("read,service")), ("investigation_preferences".into(), twin::b(false))])]));
        let d = ctx.event(Some(&model), "send", obj(vec![("draft".into(), s("Use the configured image route."))]));
        let wire = as_str(effect_of(&d).and_then(|effect| effect.get("wire"))).unwrap_or("");
        keep(&mut passed, &format!("{format} service catalog excludes host credentials"), wire.contains("service") && !wire.contains("fixture-key"));
        let call = obj(vec![("tool".into(), s("service")), ("args".into(), obj(vec![("route".into(), s("main")), ("input".into(), obj(vec![("prompt".into(), s("model input")), ("number".into(), twin::n("9"))]))]))]);
        let d = ctx.model_reply(&d, format, &call.to_string());
        keep(&mut passed, &format!("{format} model proposal uses ordinary approval"), as_str(state_of(&d).get("dialog")) == Some("approval"));
        let d = ctx.event(Some(state_of(&d)), "approve", obj(vec![]));
        let (d, _) = ctx.send_effect(&d);
        let returned = as_str(effect_of(&d).and_then(|effect| effect.get("purpose"))) == Some("model") && as_str(effect_of(&d).and_then(|effect| effect.get("wire"))).unwrap_or("").contains("model input");
        keep(&mut passed, &format!("{format} actual result returns to requesting model"), returned);
        let d = ctx.model_reply(&d, format, "The actual fixture result was returned.");
        keep(&mut passed, &format!("{format} answer completes"), !truthy_at(state_of(&d).get("busy")));
        let mut malformed = model;
        put(&mut malformed, "service_connections", obj(vec![("version".into(), twin::n("999"))]));
        let mut profiles = malformed.get("profiles").unwrap().arr().clone();
        profiles.push(obj(vec![("protocol".into(), s("unsupported"))]));
        put(&mut malformed, "profiles", Value::Arr(profiles));
        let d = ctx.event(Some(&malformed), "send", obj(vec![("draft".into(), s("Ordinary chat still works."))]));
        keep(&mut passed, &format!("{format} malformed extras do not block primary"), as_str(effect_of(&d).and_then(|effect| effect.get("purpose"))) == Some("model"));
        ctx.event(Some(state_of(&d)), "cancel", obj(vec![]));
    }

    let mut optional = ctx.configured(None);
    *dig_mut(&mut optional, &["service_connections", "routes", "1", "enabled"]) = twin::b(false);
    let d = ctx.approve_review(&optional, "", "image_generation");
    let (done, _) = ctx.send_effect(&d);
    keep(&mut passed, "disabled fallback cannot block primary service", as_str(at(state_of(&done), &["service_result", "status"])) == Some("response_received"));

    let mut optional = ctx.configured(None);
    push_item(
        dig_mut(&mut optional, &["service_connections", "routes"]),
        obj(vec![
            ("id".into(), s("unrelated")),
            ("host".into(), s("absent")),
            ("capability".into(), s("other")),
            ("path".into(), s("invalid")),
            ("method".into(), s("POST")),
            ("response".into(), s("invalid")),
            ("enabled".into(), twin::b(true)),
        ]),
    );
    let d = ctx.approve_review(&optional, "main", "");
    let (done, _) = ctx.send_effect(&d);
    keep(&mut passed, "malformed unrelated service remains scoped", as_str(at(state_of(&done), &["service_result", "status"])) == Some("response_received"));

    let mut provider = ctx.configured(None);
    remove_key(dig_mut(&mut provider, &["service_connections", "defaults"]), "image_generation");
    put(&mut provider, "profiles", Value::Arr(vec![profile("Provider", &format!("http://127.0.0.1:{fallback_port}/v1/chat/completions"), "chat", "provider")]));
    let d = ctx.approve_review(&provider, "", "image_generation");
    keep(
        &mut passed,
        "a capability with no saved default resolves to the selected provider route",
        effect_of(&d).is_some() && as_str(effect_of(&d).and_then(|effect| effect.get("host"))) == Some("127.0.0.1") && num_is(effect_of(&d).and_then(|effect| effect.get("port")), fallback_port),
    );
    let (done, _) = ctx.send_effect(&d);
    keep(&mut passed, "the provider route actually served the capability", ctx.peers[0].last().port == fallback_port && is_bool(state_of(&done).get("notice_error"), false));

    let mut outsider = ctx.configured(None);
    remove_key(dig_mut(&mut outsider, &["service_connections", "defaults"]), "image_generation");
    put(&mut outsider, "profiles", Value::Arr(vec![profile("Elsewhere", "http://elsewhere.test/v1/chat/completions", "chat", "elsewhere")]));
    let opened = ctx.event(Some(&outsider), "service_request", obj(vec![]));
    let held = ctx.event(
        Some(state_of(&opened)),
        "service_review",
        obj(vec![
            ("service_route".into(), s("")),
            ("service_capability".into(), s("image_generation")),
            ("service_input".into(), s(&obj(vec![("prompt".into(), s("no default")), ("number".into(), twin::n("1"))]).to_string())),
        ]),
    );
    keep(
        &mut passed,
        "an undefaulted third-party route is never selected on its own",
        effect_of(&held).is_none() && truthy_at(state_of(&held).get("notice_error")) && as_str(state_of(&held).get("notice")).unwrap_or("").contains("This capability has no configured route") && is_none(state_of(&held).get("pending")),
    );

    for format in ["chat", "local", "messages", "content"] {
        let mut model = ctx.configured(None);
        put(
            &mut model,
            "profiles",
            Value::Arr(vec![
                profile("Parent", "http://parent.test/v1/chat/completions", format, "parent"),
                profile("Child", "http://child.test/v1/chat/completions", format, "child"),
            ]),
        );
        put(
            &mut model,
            "agents",
            Value::Arr(vec![
                obj(vec![("name".into(), s("Parent")), ("model".into(), s("0")), ("tools".into(), s("delegate")), ("subagents".into(), Value::Arr(vec![twin::n("1")]))]),
                obj(vec![("name".into(), s("Child")), ("model".into(), s("1")), ("tools".into(), s("service")), ("role".into(), s("coding"))]),
            ]),
        );
        let d = ctx.event(Some(&model), "send", obj(vec![("draft".into(), s("Ask the child to get server data."))]));
        let call = obj(vec![("tool".into(), s("delegate")), ("args".into(), obj(vec![("agent".into(), twin::n("1")), ("task".into(), s("Use the configured service."))]))]);
        let d = ctx.model_reply(&d, format, &call.to_string());
        let d = ctx.event(Some(state_of(&d)), "approve", obj(vec![]));
        keep(&mut passed, &format!("{format} child starts on captured route"), num_is(state_of(&d).get("request_index"), 1));
        let call = obj(vec![("tool".into(), s("service")), ("args".into(), obj(vec![("route".into(), s("main")), ("input".into(), obj(vec![("prompt".into(), s("actual child input")), ("number".into(), twin::n("3"))]))]))]);
        let d = ctx.model_reply(&d, format, &call.to_string());
        keep(&mut passed, &format!("{format} child service uses review"), as_str(state_of(&d).get("dialog")) == Some("approval"));
        let d = ctx.event(Some(state_of(&d)), "approve", obj(vec![]));
        let (d, _) = ctx.send_effect(&d);
        keep(
            &mut passed,
            &format!("{format} actual result returns to child"),
            effect_of(&d).is_some() && num_is(state_of(&d).get("request_index"), 1) && as_str(effect_of(&d).and_then(|effect| effect.get("wire"))).unwrap_or("").contains("actual child input"),
        );
        let d = ctx.model_reply(&d, format, "ACTUAL_CHILD_SERVICE_RESULT");
        keep(
            &mut passed,
            &format!("{format} child returns to captured parent"),
            effect_of(&d).is_some() && num_is(state_of(&d).get("request_index"), 0) && as_str(effect_of(&d).and_then(|effect| effect.get("wire"))).unwrap_or("").contains("ACTUAL_CHILD_SERVICE_RESULT"),
        );
        let d = ctx.model_reply(&d, format, "Parent answer.");
        keep(&mut passed, &format!("{format} child service settles"), !truthy_at(state_of(&d).get("busy")) && !truthy_at(state_of(&d).get("delegation_stack")));
    }

    let actual = ctx.controller.op(&obj(vec![
        ("op".into(), s("tool")),
        ("tool".into(), s("read")),
        ("root".into(), s(ctx.root)),
        ("args".into(), obj(vec![("path".into(), s("evidence.txt"))])),
    ]));
    keep(&mut passed, "model-free local work remains available", matches!(actual, Value::Str(text) if text == "actual local file"));
    passed
}

fn profile(name: &str, endpoint: &str, protocol: &str, model: &str) -> Value {
    obj(vec![
        ("name".into(), s(name)),
        ("endpoint".into(), s(endpoint)),
        ("protocol".into(), s(protocol)),
        ("model".into(), s(model)),
        ("plain".into(), twin::b(true)),
        ("stream".into(), twin::b(false)),
    ])
}

fn cfg(peers: &[Peer]) -> Value {
    let host0 = peers[0].port();
    let host1 = peers[1].port();
    obj(vec![
        ("version".into(), twin::n("1")),
        ("hosts".into(), Value::Arr(vec![host("host0", host0, "fixture-key-0"), host("host1", host1, "fixture-key-1")])),
        ("routes".into(), Value::Arr(vec![route("main", "host0", "/generate", "/actual", true), route("fallback", "host1", "/generate", "", true)])),
        (
            "defaults".into(),
            obj(vec![(
                "image_generation".into(),
                obj(vec![
                    ("routes".into(), Value::Arr(vec![s("main"), s("fallback")])),
                    ("fallback_codes".into(), Value::Arr(vec![twin::n("503")])),
                    ("connect_fallback".into(), twin::b(true)),
                ]),
            )]),
        ),
    ])
}

fn host(id: &str, port: u16, key: &str) -> Value {
    obj(vec![
        ("id".into(), s(id)),
        ("url".into(), s(&format!("http://127.0.0.1:{port}"))),
        ("plain".into(), twin::b(true)),
        ("enabled".into(), twin::b(true)),
        ("auth_header".into(), s("X-Token")),
        ("auth_prefix".into(), s("")),
        ("key".into(), s(key)),
    ])
}

fn route(id: &str, host_id: &str, path: &str, response_path: &str, enabled: bool) -> Value {
    let mut fields = vec![
        ("id".into(), s(id)),
        ("host".into(), s(host_id)),
        ("capability".into(), s("image_generation")),
        ("path".into(), s(path)),
        ("method".into(), s("POST")),
        ("response".into(), s("json")),
        ("response_path".into(), s(response_path)),
        ("enabled".into(), twin::b(enabled)),
    ];
    if id == "main" {
        fields.insert(5, ("request".into(), obj(vec![("prompt".into(), obj(vec![("$input".into(), s("/prompt"))])), ("number".into(), obj(vec![("$input".into(), s("/number"))]))])));
    }
    obj(fields)
}

/// The format name selects the provider body. It is not a wrapper key.
fn model_body(format: &str, content: &str) -> Value {
    match format {
        "chat" => obj(vec![("choices".into(), Value::Arr(vec![obj(vec![("message".into(), obj(vec![("content".into(), s(content))])), ("finish_reason".into(), s("stop"))])]))]),
        "local" => obj(vec![("message".into(), obj(vec![("content".into(), s(content))])), ("done".into(), twin::b(true))]),
        "messages" => obj(vec![("content".into(), Value::Arr(vec![obj(vec![("type".into(), s("text")), ("text".into(), s(content))])])), ("stop_reason".into(), s("end_turn"))]),
        "content" => obj(vec![("candidates".into(), Value::Arr(vec![obj(vec![("content".into(), obj(vec![("parts".into(), Value::Arr(vec![obj(vec![("text".into(), s(content))])]))])), ("finishReason".into(), s("STOP"))])]))]),
        other => panic!("unknown model format {other}"),
    }
}

fn upload(controller: &mut Controller, effect: &Value, stream: &mut TcpStream) {
    let Some(ticket) = effect.get("media_upload").and_then(present).map(Value::str_) else { return };
    let purpose = as_str(effect.get("purpose")).unwrap_or("");
    let limit = num_u16(effect.get("upload_bytes")).map(usize::from).unwrap_or(0);
    let mut count = 0usize;
    let mut failure = None;
    loop {
        let part = controller.op(&obj(vec![("op".into(), s("media_upload_read")), ("purpose".into(), s(purpose)), ("ticket".into(), s(ticket))]));
        let data = twin::b64_decode(as_str(part.get("data")).unwrap_or(""));
        if data.len() > 8192 {
            failure = Some("upload chunk exceeds 8192");
            break;
        }
        count += data.len();
        if count > limit {
            failure = Some("upload exceeds upload_bytes");
            break;
        }
        if stream.write_all(&data).is_err() {
            failure = Some("upload write failed");
            break;
        }
        if matches!(part.get("done"), Some(Value::Bool(true))) {
            if count != limit {
                failure = Some("upload length mismatch");
            }
            break;
        }
        if data.is_empty() {
            failure = Some("empty upload chunk");
            break;
        }
    }
    let _ = controller.op(&obj(vec![("op".into(), s("media_upload_cancel")), ("purpose".into(), s(purpose)), ("ticket".into(), s(ticket))]));
    if let Some(error) = failure {
        panic!("{error}");
    }
}

fn wire_bytes(effect: &Value) -> Vec<u8> {
    if let Some(text) = as_str(effect.get("wire_base64")) {
        return twin::b64_decode(text);
    }
    if let Some(text) = as_str(effect.get("wire")) {
        return text.as_bytes().to_vec();
    }
    panic!("missing network wire");
}

fn state_of(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("missing state: {}", value.to_string()))
}

fn same_state(left: &Value, right: &Value) -> bool {
    left.get("state") == right.get("state")
}

/// JSON null is Python None. A missing effect is the same for these callbacks.
fn present(value: &Value) -> Option<&Value> {
    match value {
        Value::Null => None,
        other => Some(other),
    }
}

fn effect_of(value: &Value) -> Option<&Value> {
    value.get("effect").and_then(present)
}

fn modal_of(value: &Value) -> &Value {
    value.get("ui").and_then(|ui| ui.get("modal")).unwrap_or_else(|| panic!("missing modal: {}", value.to_string()))
}

fn field_value<'a>(modal: &'a Value, key: &str) -> &'a str {
    modal.get("fields").and_then(Value::as_arr).and_then(|fields| fields.iter().find(|field| as_str(field.get("key")) == Some(key))).and_then(|field| as_str(field.get("value"))).unwrap_or_else(|| panic!("missing field {key}"))
}

fn at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for key in path {
        cursor = match cursor {
            Value::Arr(items) => {
                let index = key.parse::<usize>().ok()?;
                items.get(index)?
            }
            Value::Obj(_) => cursor.get(key)?,
            _ => return None,
        };
    }
    Some(cursor)
}

fn as_str(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::Str(text)) => Some(text),
        _ => None,
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Num(token) => !matches!(token.as_str(), "0" | "-0" | "0.0" | "-0.0"),
        Value::Str(text) => !text.is_empty(),
        Value::Arr(items) => !items.is_empty(),
        Value::Obj(fields) => !fields.is_empty(),
    }
}

fn truthy_at(value: Option<&Value>) -> bool {
    value.map(truthy).unwrap_or(false)
}

fn is_bool(value: Option<&Value>, expected: bool) -> bool {
    matches!(value, Some(Value::Bool(flag)) if *flag == expected)
}

fn is_none(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

fn num_is(value: Option<&Value>, expected: u16) -> bool {
    matches!(value, Some(Value::Num(token)) if token == &expected.to_string())
}

fn num_u16(value: Option<&Value>) -> Option<u16> {
    match value {
        Some(Value::Num(token)) => token.parse().ok(),
        _ => None,
    }
}

fn body_eq(hit: &Hit, expected: &Value) -> bool {
    !hit.body.is_empty() && Value::parse(&hit.body) == *expected
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn remove_key(value: &mut Value, key: &str) {
    let Value::Obj(fields) = value else { panic!("expected object") };
    fields.retain(|(name, _)| name != key);
}

fn push_item(value: &mut Value, item: Value) {
    let Value::Arr(items) = value else { panic!("expected array") };
    items.push(item);
}

fn dig_mut<'a>(value: &'a mut Value, path: &[&str]) -> &'a mut Value {
    if path.is_empty() {
        return value;
    }
    let key = path[0];
    match value {
        Value::Arr(items) => {
            let index = key.parse::<usize>().unwrap_or_else(|_| panic!("expected index, got {key}"));
            let child = items.get_mut(index).unwrap_or_else(|| panic!("missing index {index}"));
            dig_mut(child, &path[1..])
        }
        Value::Obj(fields) => {
            let pos = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
            dig_mut(&mut fields[pos].1, &path[1..])
        }
        _ => panic!("cannot descend at {key}"),
    }
}

struct Hit {
    port: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Clone for Hit {
    fn clone(&self) -> Self {
        Hit { port: self.port, headers: self.headers.clone(), body: self.body.clone() }
    }
}

fn header<'a>(hit: &'a Hit, name: &str) -> Option<&'a str> {
    hit.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}

struct Peer {
    port: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
}

impl Peer {
    fn pair() -> Vec<Peer> {
        let hits = Arc::new(Mutex::new(Vec::new()));
        vec![Peer::spawn(Arc::clone(&hits)), Peer::spawn(hits)]
    }

    fn spawn(hits: Arc<Mutex<Vec<Hit>>>) -> Peer {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_else(|error| panic!("fixture bind: {error}"));
        let port = listener.local_addr().unwrap_or_else(|error| panic!("fixture port: {error}")).port();
        let captured = Arc::clone(&hits);
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                stream.set_read_timeout(Some(Duration::from_secs(8))).ok();
                let Ok((path, headers, body)) = read_request(&mut stream) else { let _ = stream.shutdown(Shutdown::Both); continue };
                let body_text = String::from_utf8_lossy(&body).into_owned();
                captured.lock().unwrap().push(Hit { port, headers, body: body_text.clone() });
                let status = if path == "/unavailable" { 503 } else if path == "/auth-failed" { 401 } else if path == "/accepted" { 202 } else { 200 };
                let actual = if body_text.is_empty() { obj(vec![]) } else { Value::parse(&body_text) };
                let payload = obj(vec![
                    ("actual".into(), actual),
                    ("job".into(), s("server-job")),
                    ("items".into(), Value::Arr(vec![obj(vec![("value".into(), s("returned fixture result"))])])),
                ]).to_string();
                let reason = match status {
                    503 => "Service Unavailable",
                    401 => "Unauthorized",
                    202 => "Accepted",
                    _ => "OK",
                };
                let response = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.shutdown(Shutdown::Both);
            }
        });
        Peer { port, hits }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn len(&self) -> usize {
        self.hits.lock().unwrap().len()
    }

    fn last(&self) -> Hit {
        self.hits.lock().unwrap().last().cloned().unwrap_or_else(|| panic!("no fixture request"))
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, Vec<(String, String)>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        let count = stream.read(&mut tmp)?;
        if count == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "truncated request"));
        }
        buf.extend_from_slice(&tmp[..count]);
        if buf.len() > 1_048_576 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request too large"));
        }
    }
    let split = buf.windows(4).position(|window| window == b"\r\n\r\n").unwrap();
    let head = std::str::from_utf8(&buf[..split]).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut lines = head.split("\r\n");
    let path = lines.next().unwrap_or("").split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut headers = Vec::new();
    let mut length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }
    let mut body = buf[split + 4..].to_vec();
    while body.len() < length {
        let count = stream.read(&mut tmp)?;
        if count == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..count]);
    }
    body.truncate(length);
    Ok((path, headers, body))
}

struct Cleanup(std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_report(passed: &[String]) {
    let body = obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("services", &body, true);
}

trait ArrExt {
    fn as_arr(&self) -> Option<&Vec<Value>>;
}

impl ArrExt for Value {
    fn as_arr(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }
}

//! User-initiated device authorization over the existing owned token transport.
use super::*;

pub(super) struct Device {
    code: String,
    user: String,
    uri: String,
    browser: String,
    expires: Instant,
    next: Instant,
    interval: u64,
}

impl Device {
    pub(super) fn new() -> Self {
        Self { code: String::new(), user: String::new(), uri: String::new(), browser: String::new(),
            expires: Instant::now() + Duration::from_secs(600), next: Instant::now(), interval: 5 }
    }
    pub(super) fn expired(&self) -> bool { Instant::now() >= self.expires }
}

// A shortcut is optional: an invalid one never disables manual verification.
fn shortcut(reply: &Json, uri: &str, code: &str, plain: bool) -> Option<String> {
    let value = field(reply, "verification_uri_complete").ok()?;
    if value.is_empty() || value.len() > 2048 || value.contains(['#', '\\']) { return None; }
    let base = network::url(uri, plain).ok()?;
    let target = network::url(value, plain).ok()?;
    if base.tls != target.tls || base.port != target.port || !base.host.eq_ignore_ascii_case(&target.host) { return None; }
    // Never expose a private protocol credential through a browser URL, including
    // encoded spellings. Decoding is bounded and only used for inspection; the
    // original accepted URL is retained byte for byte for the browser.
    let mut decoded = value.to_string();
    for _ in 0..8 {
        if decoded.contains(code) || decoded.chars().any(char::is_control) { return None; }
        let fields = query(decoded.split_once('?').map(|(_, v)| v).unwrap_or("")).ok()?;
        if fields.keys().any(|k| ["device_code", "access_token", "refresh_token", "client_secret"].contains(&k.to_ascii_lowercase().as_str())) { return None; }
        let next = decode(&decoded).ok()?;
        if next == decoded { return Some(value.into()); }
        decoded = next;
    }
    None
}

fn seconds(value: &Json, key: &str, default: Option<u64>) -> Result<u64> {
    let value = match get(value, key) {
        Null => default.ok_or("Device authorization lifetime is required")?,
        Num(n) => n.parse::<u64>().map_err(|_| "Device authorization timing must be an integer")?,
        _ => return Err("Device authorization timing must be an integer".into()),
    };
    if value == 0 || value > 86400 { return Err("Device authorization timing is outside its supported bound".into()); }
    Ok(value)
}

fn wait(st: &mut Json, ticket: &str, flow: Flow, notice: &str) -> Result<()> {
    flows().lock().map_err(|_| "Authorization is unavailable")?.insert(ticket.into(), flow);
    put(st, "oauth_expected", obj(vec![("ticket", s(ticket)), ("phase", s("device_wait"))]));
    put(st, "dialog", s("oauth_device"));
    put(st, "notice", s(notice));
    Ok(())
}

pub(super) fn response(st: &mut Json, ticket: &str, mut flow: Flow, status: u16, reply: &Json) -> Result<Option<Flow>> {
    let device = flow.device.as_mut().ok_or("Device authorization session is unavailable")?;
    if device.expired() { return Err("Device authorization expired; start sign-in again".into()); }
    if flow.phase == "device_request" {
        if status != 200 || get(reply, "error") != &Null {
            return Err(format!("Device authorization was refused (HTTP{status}); existing credentials were retained"));
        }
        let code = text(reply, "device_code");
        let user = text(reply, "user_code");
        let uri = text(reply, "verification_uri");
        if code.is_empty() || code.len() > 8192 || code.chars().any(char::is_control)
            || user.is_empty() || user.len() > 128 || user.chars().any(char::is_control)
            || uri.is_empty() || uri.len() > 2048 || uri.contains(&code) {
            return Err("Device authorization returned invalid verification details".into());
        }
        let p = current(st, flow.target)?;
        let parsed = network::url(&uri, boolean(get(&p, "oauth"), "plain"))?;
        let q = query(parsed.path.split_once('?').map(|(_, v)| v).unwrap_or(""))?;
        if q.keys().any(|k| ["device_code", "access_token", "refresh_token", "client_secret"].contains(&k.as_str())) {
            return Err("Device verification URI contains private protocol fields".into());
        }
        let lifetime = seconds(reply, "expires_in", None)?.min(600);
        let interval = seconds(reply, "interval", Some(5))?;
        let browser = shortcut(reply, &uri, &code, boolean(get(&p, "oauth"), "plain")).unwrap_or_else(|| uri.clone());
        *device = Device { code, user, uri, browser, expires: Instant::now() + Duration::from_secs(lifetime),
            next: Instant::now() + Duration::from_secs(interval), interval };
        flow.phase = "device_wait".into();
        wait(st, ticket, flow, "Visit the displayed verification address and enter its code. Then check authorization here.")?;
        return Ok(None);
    }
    if status == 400 && matches!(text(reply, "error").as_str(), "authorization_pending" | "slow_down") {
        let slowing = text(reply, "error") == "slow_down";
        if slowing { device.interval = device.interval.saturating_add(5); }
        device.next = Instant::now() + Duration::from_secs(device.interval);
        flow.phase = "device_wait".into();
        wait(st, ticket, flow, if slowing { "The server requested a longer wait before checking authorization again" }
            else { "Authorization is still pending; complete the displayed verification before checking again" })?;
        return Ok(None);
    }
    if status != 200 || get(reply, "error") != &Null {
        return Err("Device authorization ended without a grant; existing credentials were retained. Start sign-in again when ready.".into());
    }
    Ok(Some(flow))
}

pub(super) fn action(st: &mut Json, a: &str) -> Result<Json> {
    let ticket = owner(st);
    let mut held = flows().lock().map_err(|_| "Authorization is unavailable")?;
    let flow = held.get_mut(&ticket).ok_or("Device authorization is no longer active; start sign-in again")?;
    let p = current(st, flow.target)?;
    if flow.binding != flow_binding(st, flow.target, &p) || text(st, "dialog") != "oauth_device" || flow.phase != "device_wait"
        || boolean(st, "busy") || selected(st)? != flow.target || interaction::locked(st) {
        return Err("Device authorization context changed; no request was sent".into());
    }
    let device = flow.device.as_ref().ok_or("Device authorization is unavailable")?;
    if device.expired() { drop(held); interrupt(st); return Err("Device authorization expired; start sign-in again".into()); }
    if a == "oauth_device_open" {
        return Ok(obj(vec![("kind", s("oauth_browser")), ("url", s(&device.browser))]));
    }
    let remaining = device.next.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        put(st, "notice", s(&format!("Wait {} seconds before checking authorization again", remaining.as_secs() + 1)));
        return Ok(Null);
    }
    let code = device.code.clone();
    flow.phase = "device_poll".into();
    drop(held);
    token_request(st, &ticket, get(&p, "oauth"), vec![
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code".into()),
        ("device_code", code), ("client_id", text(get(&p, "oauth"), "client_id"))])
}

pub(super) fn live_wait(st: &Json) -> bool {
    flows().lock().ok().and_then(|m| m.get(&owner(st)).map(|f| {
        f.phase == "device_wait" && f.device.as_ref().is_some_and(|d| !d.expired())
            && current(st, f.target).is_ok_and(|p| flow_binding(st, f.target, &p) == f.binding)
    })).unwrap_or(false)
}

pub(super) fn recover(st: &mut Json) {
    if text(get(st, "oauth_expected"), "phase") != "device_wait" { return; }
    if live_wait(st) { put(st, "dialog", s("oauth_device")); }
    else { interrupt(st); put(st, "dialog", s("oauth_settings"));
        put(st, "notice", s("Device sign-in ended after restart or expiry; start again. Existing credentials were retained.")); }
}

pub(super) fn dialog(st: &Json) -> Json {
    let held = flows().lock().ok();
    let device = held.as_ref().and_then(|m| m.get(&owner(st))).and_then(|f| f.device.as_ref());
    let mut fields = connection_fields(st);
    fields.push(field_ui("status", "Device authorization",
        "Verify the address and code on your authorization server. Check authorization after granting access. Nothing is checked automatically.", "readonly"));
    if let Some(d) = device {
        fields.push(field_ui("verification_uri", "Verification address", &d.uri, "readonly"));
        fields.push(field_ui("user_code", "Verification code", &d.user, "readonly"));
        if d.browser != d.uri { fields.push(field_ui("verification_shortcut", "Browser shortcut address", &d.browser, "readonly")); }
    }
    modal("Device authorization", fields, vec![button("Open verification address", "oauth_device_open"),
        button("Check authorization", "oauth_device_poll"), button("Cancel authorization", "oauth_cancel")])
}

#[cfg(test)] mod checks {
    use super::*;
    #[test] fn timing_bounds_and_default() {
        assert_eq!(seconds(&obj(vec![]), "interval", Some(5)).unwrap(), 5);
        for invalid in [s("5"), Num("0".into()), Num("-1".into()), Num("1.5".into()), Num("86401".into())] {
            assert!(seconds(&obj(vec![("interval", invalid)]), "interval", Some(5)).is_err());
        }
        assert!(seconds(&Null, "expires_in", None).is_err());
    }
    #[test] fn bounded_shortcut_preserves_manual_fallback() {
        let uri = "https://auth.test/verify";
        let code = "private-device-code";
        let good = "https://AUTH.test:443/verify?user_code=CODE-7392&session=%2F";
        assert_eq!(shortcut(&obj(vec![("verification_uri_complete", s(good))]), uri, code, false).as_deref(), Some(good));
        for value in [Null, Bool(true), s(""), s("http://auth.test/verify?user_code=ok"), s("https://other.test/verify"),
            s("https://auth.test:444/verify"), s("https://user:pass@auth.test/verify"), s("https://auth.test/verify#x"),
            s("https://auth.test/verify?device_code=x"), s("https://auth.test/verify?%2564evice_code=x"),
            s("https://auth.test/verify?Access_Token=x"), s("https://auth.test/verify?x=private-device-code"),
            s("https://auth.test/verify?x=%70rivate-device-code"), s("https://auth.test/verify?x=%2570rivate-device-code"),
            s("https://auth.test/verify?x=%0A"), s(&format!("https://auth.test/{}", "x".repeat(2048)))] {
            assert!(shortcut(&obj(vec![("verification_uri_complete", value)]), uri, code, false).is_none());
        }
        assert!(shortcut(&Null, uri, code, false).is_none());
        assert!(shortcut(&obj(vec![("verification_uri_complete", s("http://127.0.0.1/verify?user_code=ok"))]), "http://127.0.0.1/verify", code, true).is_some());
    }

}

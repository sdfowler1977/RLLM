//! General live-network observation. This is not tied to any one target or
//! protocol: a listener records whatever connects to it (an HTTP proxy CONNECT
//! target, a plain HTTP request, a TLS ClientHello's server name, or raw bytes)
//! and can relay to the real destination so the subject keeps working while it
//! is observed; a connect probe records a service's banner or handshake. The
//! findings are written as a workspace report file, so inspect/read/reconstruct
//! and the recursive inquiry reason over them like any other observation. All
//! socket work is bounded by the operation deadline and the cancellation token.
use super::*;
use std::net::{TcpListener, TcpStream, Shutdown};
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};

/// Largest first-chunk we inspect per connection before classifying/relaying.
const PEEK: usize = 16 * 1024;

pub(super) fn validate(a: &Json) -> Result<()> {
    let Obj(f) = a else { return Err("Network observation requires an object".into()); };
    if f.keys().any(|k| !["mode", "address", "upstream", "output", "seconds", "probe", "note"].contains(&k.as_str())) {
        return Err("Network observation accepts mode, address, upstream, output, seconds, probe and note".into());
    }
    let mode = text(a, "mode");
    if !["listen", "connect"].contains(&mode.as_str()) {
        return Err("Choose mode listen (observe/relay what connects here) or connect (probe an endpoint)".into());
    }
    let address = get(a, "address").text()?;
    if address.is_empty() || address.len() > 256 || address.contains(['\0', ' ']) {
        return Err("Give address as host:port (connect) or bind:port such as 0.0.0.0:8080 (listen)".into());
    }
    if mode == "connect" && !address.contains(':') {
        return Err("Connect address must be host:port".into());
    }
    let upstream = text(a, "upstream");
    if upstream.len() > 256 || upstream.contains(['\0', ' ']) || (!upstream.is_empty() && !upstream.contains(':')) {
        return Err("Upstream, when given, must be host:port".into());
    }
    let output = get(a, "output").text()?;
    if output.is_empty() || output.len() > 255 || output == "." || output == ".." || output.contains(['/', '\\', '\0']) {
        return Err("Choose a new workspace-root output filename for the report".into());
    }
    if let Num(n) = get(a, "seconds") {
        match n.parse::<u64>() { Ok(s) if (1..=120).contains(&s) => {}, _ => return Err("seconds must be 1..=120".into()) }
    } else if get(a, "seconds") != &Null {
        return Err("seconds must be a number 1..=120".into());
    }
    if text(a, "probe").len() > 8192 { return Err("probe text is too long".into()); }
    if text(a, "note").len() > 512 { return Err("note exceeds 512 bytes".into()); }
    Ok(())
}

pub(super) fn run(root: &Path, args: &Json, access: &operation::InspectionAccess) -> Result<String> {
    validate(args)?;
    let cancelled = || access.cancelled();
    let window = window_duration(args, access.deadline);
    let report = match text(args, "mode").as_str() {
        "connect" => connect_probe(&text(args, "address"), &text(args, "probe"), window, &cancelled)?,
        _ => listen_observe(&text(args, "address"), &text(args, "upstream"), window, &cancelled)?,
    };
    let name = text(args, "output");
    let offset = capture_file::save_bytes(root, &name, report.as_bytes(), &cancelled)?;
    Ok(format!("Wrote {offset} bytes of network observations to {name}. Read or inspect {name} for the recorded endpoints and exchanges."))
}

fn window_duration(args: &Json, deadline: Instant) -> Duration {
    let requested = match get(args, "seconds") { Num(n) => n.parse::<u64>().unwrap_or(20), _ => 20 };
    // Leave a margin before the operation deadline to write the report and read back.
    let room = deadline.saturating_duration_since(Instant::now()).saturating_sub(Duration::from_secs(3));
    Duration::from_secs(requested).min(room).max(Duration::from_millis(500))
}

/// Connect to one endpoint, optionally send a probe, and record the first bytes
/// it returns (a banner or handshake) as hex and printable text.
fn connect_probe(address: &str, probe: &str, window: Duration, cancelled: &impl Fn() -> bool) -> Result<String> {
    let addr = resolve(address)?;
    let mut stream = TcpStream::connect_timeout(&addr, window.min(Duration::from_secs(15)))
        .map_err(|e| format!("Cannot connect to {address}: {e}"))?;
    stream.set_read_timeout(Some(window.min(Duration::from_secs(15)))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();
    if !probe.is_empty() {
        let bytes = probe_bytes(probe);
        stream.write_all(&bytes).map_err(|e| format!("Cannot send probe to {address}: {e}"))?;
    }
    let mut buffer = vec![0u8; PEEK];
    let mut seen = Vec::new();
    while seen.len() < PEEK {
        if cancelled() { break; }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => seen.extend_from_slice(&buffer[..n]),
            Err(_) => break,
        }
    }
    let mut out = String::new();
    out.push_str(&format!("Network observation: connect {address}\nLocal peer: {}\n", stream.peer_addr().map(|a| a.to_string()).unwrap_or_default()));
    if !probe.is_empty() { out.push_str(&format!("Probe sent: {} bytes\n", probe_bytes(probe).len())); }
    out.push_str(&format!("Received: {} bytes\n", seen.len()));
    out.push_str(&describe(&seen));
    out.push_str(&format!("\n--- hex (first {} bytes) ---\n{}\n", seen.len().min(512), hex_dump(&seen[..seen.len().min(512)])));
    Ok(out)
}

/// Bind a local listener and record every connection for the window. When an
/// upstream (or an in-band CONNECT / HTTP Host) target is known, relay both
/// directions so the subject keeps working while it is observed.
fn listen_observe(bind: &str, upstream: &str, window: Duration, cancelled: &impl Fn() -> bool) -> Result<String> {
    let listener = TcpListener::bind(bind).map_err(|e| format!("Cannot bind {bind}: {e}"))?;
    listener.set_nonblocking(true).ok();
    let local = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| bind.to_string());
    let records: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let end = Instant::now() + window;
    let upstream = upstream.to_string();
    let mut handles = Vec::new();
    while Instant::now() < end && !cancelled() {
        match listener.accept() {
            Ok((stream, peer)) => {
                let records = Arc::clone(&records);
                let upstream = upstream.clone();
                let left = end.saturating_duration_since(Instant::now());
                handles.push(std::thread::spawn(move || handle_conn(stream, peer, &upstream, left, &records)));
                if handles.len() >= 256 { break; }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    drop(listener);
    for h in handles { let _ = h.join(); }
    let lines = Arc::try_unwrap(records).map(|m| m.into_inner().unwrap_or_default()).unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!("Network observation: listen {bind} (bound {local}) for {} s\n", window.as_secs()));
    out.push_str(&format!("Connections observed: {}\n", lines.len()));
    if !upstream.is_empty() { out.push_str(&format!("Relay upstream for direct connections: {upstream}\n")); }
    out.push('\n');
    for (i, line) in lines.iter().enumerate() { out.push_str(&format!("[{}] {line}\n", i + 1)); }
    Ok(out)
}

fn handle_conn(mut client: TcpStream, peer: std::net::SocketAddr, upstream: &str, left: Duration, records: &Arc<Mutex<Vec<String>>>) {
    let budget = left.min(Duration::from_secs(30)).max(Duration::from_secs(1));
    client.set_read_timeout(Some(budget)).ok();
    client.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let mut first = vec![0u8; PEEK];
    let n = match client.read(&mut first) { Ok(n) => n, Err(_) => 0 };
    first.truncate(n);
    let (summary, target) = classify(&first, upstream);
    let record = format!("{peer} -> {summary}");
    if let Ok(mut r) = records.lock() { r.push(record); }
    if n == 0 { let _ = client.shutdown(Shutdown::Both); return; }
    // Relay when a destination is known, so the observed subject keeps working.
    if let Some(dest) = target {
        if let Ok(mut server) = TcpStream::connect_timeout(&dest.0, Duration::from_secs(10)) {
            server.set_read_timeout(Some(budget)).ok();
            server.set_write_timeout(Some(Duration::from_secs(10))).ok();
            // For a proxy CONNECT the client expects a 200 before its own TLS starts,
            // and its first real bytes come after; otherwise forward the bytes we read.
            if dest.1 {
                let _ = client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
            } else {
                let _ = server.write_all(&first);
            }
            relay(client, server, budget);
            return;
        }
    }
    let _ = client.shutdown(Shutdown::Both);
}

/// Pump bytes both ways until either side closes or the budget elapses.
fn relay(a: TcpStream, b: TcpStream, budget: Duration) {
    let (a2, b2) = match (a.try_clone(), b.try_clone()) { (Ok(a2), Ok(b2)) => (a2, b2), _ => return };
    let end = Instant::now() + budget;
    let forward = move |mut from: TcpStream, mut to: TcpStream| {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            if Instant::now() >= end { break; }
            match from.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => { if to.write_all(&buf[..k]).is_err() { break; } }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(_) => break,
            }
        }
        let _ = to.shutdown(Shutdown::Write);
    };
    let t = std::thread::spawn(move || forward(a2, b2));
    forward(b, a);
    let _ = t.join();
}

type Target = (std::net::SocketAddr, bool); // (destination, is_connect_tunnel)

/// Classify a connection's first bytes into a human summary and, when possible,
/// the real destination to relay to (from a proxy CONNECT, an HTTP Host, a TLS
/// SNI, or the configured upstream).
fn classify(first: &[u8], upstream: &str) -> (String, Option<Target>) {
    if let Some((host, port)) = connect_line(first) {
        let dest = resolve(&format!("{host}:{port}")).ok().map(|a| (a, true));
        return (format!("HTTP CONNECT {host}:{port}"), dest);
    }
    if let Some((method, path, host, _)) = http_request(first) {
        let dest = if !host.is_empty() { resolve(&with_default_port(&host, 80)).ok().map(|a| (a, false)) }
                   else if !upstream.is_empty() { resolve(upstream).ok().map(|a| (a, false)) } else { None };
        let hostpart = if host.is_empty() { String::new() } else { format!(" host={host}") };
        return (format!("HTTP {method} {path}{hostpart}"), dest);
    }
    if let Some((sni, alpn)) = tls_client_hello(first) {
        let dest = if !upstream.is_empty() { resolve(upstream).ok().map(|a| (a, false)) } else { None };
        let alpntxt = if alpn.is_empty() { String::new() } else { format!(" alpn={}", alpn.join(",")) };
        return (format!("TLS ClientHello sni={sni}{alpntxt}"), dest);
    }
    let dest = if !upstream.is_empty() { resolve(upstream).ok().map(|a| (a, false)) } else { None };
    (format!("raw {} bytes: {}", first.len(), hex_dump(&first[..first.len().min(48)])), dest)
}

fn describe(bytes: &[u8]) -> String {
    if let Some((sni, alpn)) = tls_client_hello(bytes) { return format!("Looks like a TLS ClientHello; sni={sni} alpn={}\n", alpn.join(",")); }
    if let Some((method, path, host, _)) = http_request(bytes) { return format!("Looks like HTTP; {method} {path} host={host}\n"); }
    if bytes.first() == Some(&0x16) { return "Looks like a TLS record (server handshake).\n".into(); }
    if let Ok(text) = std::str::from_utf8(bytes) { if text.chars().take(64).all(|c| !c.is_control() || c == '\n' || c == '\r' || c == '\t') { return format!("Text banner: {}\n", text.lines().next().unwrap_or("").chars().take(200).collect::<String>()); } }
    String::new()
}

// ---- pure protocol parsers (unit-tested via the throwaway driver) ----

/// Parse "CONNECT host:port HTTP/1.x" (HTTP proxy tunnel request).
fn connect_line(first: &[u8]) -> Option<(String, u16)> {
    let text = std::str::from_utf8(first.get(..first.len().min(512))?).ok()?;
    let line = text.lines().next()?;
    let rest = line.strip_prefix("CONNECT ")?;
    let authority = rest.split_whitespace().next()?;
    let (host, port) = authority.rsplit_once(':')?;
    if host.is_empty() { return None; }
    Some((host.to_string(), port.parse().ok()?))
}

/// Parse an HTTP request's method, path, Host header and headers.
fn http_request(first: &[u8]) -> Option<(String, String, String, Vec<(String, String)>)> {
    let text = std::str::from_utf8(first.get(..first.len().min(8192))?).ok()?;
    let mut lines = text.split("\r\n");
    let request = lines.next()?;
    let mut parts = request.split(' ');
    let method = parts.next()?.to_string();
    if !["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "TRACE"].contains(&method.as_str()) { return None; }
    let path = parts.next()?.to_string();
    let version = parts.next()?;
    if !version.starts_with("HTTP/") { return None; }
    let mut host = String::new();
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() { break; }
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim().to_string(), v.trim().to_string());
            if k.eq_ignore_ascii_case("host") { host = v.clone(); }
            headers.push((k, v));
        }
    }
    Some((method, path, host, headers))
}

/// Parse a TLS ClientHello record for the SNI server name and ALPN protocols.
fn tls_client_hello(buf: &[u8]) -> Option<(String, Vec<String>)> {
    if buf.len() < 5 || buf[0] != 0x16 { return None; } // handshake record
    let rec_len = ((buf[3] as usize) << 8) | buf[4] as usize;
    let body = buf.get(5..5 + rec_len.min(buf.len().saturating_sub(5)))?;
    if body.first() != Some(&0x01) { return None; } // ClientHello
    // handshake: type(1) len(3) version(2) random(32) then session/cipher/comp/extensions
    let mut p = 4 + 2 + 32;
    let sid_len = *body.get(p)? as usize; p += 1 + sid_len;
    let cs_len = ((*body.get(p)? as usize) << 8) | *body.get(p + 1)? as usize; p += 2 + cs_len;
    let comp_len = *body.get(p)? as usize; p += 1 + comp_len;
    if p + 2 > body.len() { return None; }
    let ext_total = ((body[p] as usize) << 8) | body[p + 1] as usize; p += 2;
    let ext_end = (p + ext_total).min(body.len());
    let mut sni = String::new();
    let mut alpn = Vec::new();
    while p + 4 <= ext_end {
        let etype = ((body[p] as usize) << 8) | body[p + 1] as usize;
        let elen = ((body[p + 2] as usize) << 8) | body[p + 3] as usize;
        p += 4;
        let edata = body.get(p..(p + elen).min(body.len()))?;
        if etype == 0x0000 && edata.len() >= 5 {
            // server_name_list: list_len(2) type(1) name_len(2) name
            let name_len = ((edata[3] as usize) << 8) | edata[4] as usize;
            if let Some(name) = edata.get(5..5 + name_len) { sni = String::from_utf8_lossy(name).into_owned(); }
        } else if etype == 0x0010 && edata.len() >= 2 {
            // ALPN: list_len(2) then len(1)+name repeated
            let mut q = 2;
            while q < edata.len() {
                let l = edata[q] as usize; q += 1;
                if let Some(name) = edata.get(q..q + l) { alpn.push(String::from_utf8_lossy(name).into_owned()); }
                q += l;
            }
        }
        p += elen;
    }
    if sni.is_empty() && alpn.is_empty() { return None; }
    Some((sni, alpn))
}

// ---- helpers ----

fn resolve(address: &str) -> Result<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    address.to_socket_addrs().map_err(|e| format!("Cannot resolve {address}: {e}"))?.next().ok_or_else(|| format!("No address for {address}"))
}

fn with_default_port(host: &str, port: u16) -> String {
    if host.contains(':') { host.to_string() } else { format!("{host}:{port}") }
}

fn probe_bytes(probe: &str) -> Vec<u8> {
    // A probe written as \r, \n and \t escapes becomes those bytes; otherwise literal.
    probe.replace("\\r", "\r").replace("\\n", "\n").replace("\\t", "\t").into_bytes()
}

fn hex_dump(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

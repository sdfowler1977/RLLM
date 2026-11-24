//! Full-duplex speech stream.
//! The peer speaks the /api/chat socket: kind 0 handshake, kind 1 audio, kind 2 text.
//! The audio of both directions is an Ogg Opus stream: every kind 1 message
//! carries one page of the stream, and the pages hold the Opus packets.
//! Audio frames are 20 ms at 24 kHz. The microphone side keeps sending while audio and text arrive.

use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use super::opus;
use super::websocket::{self, ByteStream, Socket};

pub const KIND_HANDSHAKE: u8 = 0;
pub const KIND_AUDIO: u8 = 1;
pub const KIND_TEXT: u8 = 2;
pub const KIND_CONTROL: u8 = 3;
pub const KIND_META: u8 = 4;
pub const KIND_ERROR: u8 = 5;
pub const KIND_PING: u8 = 6;

const FRAME: usize = opus::frame_samples();
const HOLD_LIMIT: usize = 24_000 * 2;
const RETURN_LIMIT: usize = FRAME * 8;

pub struct Location {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub tls: bool,
}

pub struct Heard {
    pub pcm: Vec<i16>,
    pub text: String,
    pub dropped: usize,
}

pub struct Session {
    socket: Socket,
    encoder: opus::Encoder,
    decoder: opus::Decoder,
    stream: opus::OggWriter,
    pending: Vec<i16>,
    held: Vec<i16>,
    transcript: String,
    dropped: usize,
    pub decode_failures: u64,
    last_decode_error: Option<String>,
    pub sent_audio: u64,
    pub received_audio: u64,
}

pub fn locate(endpoint: &str, plain: bool) -> Result<Location, String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 || trimmed.chars().any(|c| c.is_control() || c == ' ') {
        return Err("Enter a speech server address".into());
    }
    let (secure, rest) = if let Some(rest) = trimmed.strip_prefix("wss://") {
        (true, rest)
    } else if let Some(rest) = trimmed.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = trimmed.strip_prefix("ws://") {
        (false, rest)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        if !plain {
            return Err("Unencrypted speech needs the connection's unencrypted access setting".into());
        }
        (false, rest)
    } else {
        return Err("Speech address must start with https://, http://, wss://, or ws://".into());
    };
    if !secure && !plain && !trimmed.starts_with("ws://") {
        return Err("Unencrypted speech needs the connection's unencrypted access setting".into());
    }
    let cut = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..cut];
    if authority.is_empty() || authority.contains('@') {
        return Err("Speech address cannot carry embedded credentials".into());
    }
    let (host, port) = split_host(authority, if secure { 443 } else { 80 })?;
    let mut path = if cut < rest.len() { rest[cut..].to_string() } else { "/".to_string() };
    if let Some(query_at) = path.find('?') {
        let (only, query) = path.split_at(query_at);
        let only = only.trim_end_matches('/');
        let only = if only.is_empty() || only == "/api" { "/api/chat" } else { only };
        path = format!("{only}{query}");
    } else {
        let only = path.trim_end_matches('/');
        path = if only.is_empty() || only == "/api" { "/api/chat".to_string() } else { only.to_string() };
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    Ok(Location { host, port, path, tls: secure })
}

pub fn resolve(location: &Location) -> Result<SocketAddr, String> {
    let target = if location.host.contains(':') {
        format!("[{}]:{}", location.host, location.port)
    } else {
        format!("{}:{}", location.host, location.port)
    };
    target.to_socket_addrs().map_err(|error| format!("Speech server address did not resolve: {error}"))?.next().ok_or_else(|| "Speech server address did not resolve".into())
}

impl Session {
    pub fn open(io: Box<dyn ByteStream>, location: &Location, voice: &str, role: &str, _model: &str, auth: Option<(&str, &str)>) -> Result<Self, String> {
        let headers: Vec<(&str, &str)> = match auth {
            Some((name, value)) => vec![(name, value)],
            None => Vec::new(),
        };
        let request_path = prompt_query(&location.path, voice, role);
        let mut socket = Socket::open(io, &location.host, location.port, &request_path, &headers)?;
        socket.set_pause(Some(Duration::from_secs(5)));
        let welcome = socket.recv()?.ok_or("Speech server did not answer the handshake")?;
        if welcome.opcode != 0x2 || welcome.data.first().copied() != Some(KIND_HANDSHAKE) {
            return Err("Speech server handshake did not use the handshake message".into());
        }
        let encoder = opus::Encoder::new();
        let mut stream = opus::OggWriter::new();
        // The peer reads an Ogg Opus stream: the identification and the
        // comment page open it, the audio pages follow.
        let header = stream.header(encoder.pre_skip(), opus::frame_rate() as u32);
        let mut message = Vec::with_capacity(1 + header.len());
        message.push(KIND_AUDIO);
        message.extend(header);
        socket.send_binary(&message)?;
        Ok(Self {
            socket,
            encoder,
            decoder: opus::Decoder::new(),
            stream,
            pending: Vec::new(),
            held: Vec::new(),
            transcript: String::new(),
            dropped: 0,
            decode_failures: 0,
            last_decode_error: None,
            sent_audio: 0,
            received_audio: 0,
        })
    }

    pub fn push(&mut self, pcm: &[i16]) -> Result<Heard, String> {
        self.pending.extend_from_slice(pcm);
        let mut heard = Heard { pcm: Vec::new(), text: String::new(), dropped: 0 };
        while self.pending.len() >= FRAME {
            let frame: Vec<i16> = self.pending.drain(..FRAME).collect();
            let packet = opus::encode(&mut self.encoder, &frame)?;
            let page = self.stream.page(&packet);
            let mut message = Vec::with_capacity(1 + page.len());
            message.push(KIND_AUDIO);
            message.extend(page);
            self.socket.send_binary(&message)?;
            self.sent_audio += 1;
            self.collect(Duration::from_millis(40), &mut heard)?;
        }
        self.emit(&mut heard);
        Ok(heard)
    }

    pub fn take_text(&mut self) -> String {
        std::mem::take(&mut self.transcript)
    }

    pub fn caption(&self) -> String {
        let text = self.transcript.trim();
        let count = text.chars().count();
        if count <= 160 {
            text.to_string()
        } else {
            text.chars().skip(count - 160).collect()
        }
    }

    pub fn last_decode_error(&self) -> Option<&str> {
        self.last_decode_error.as_deref()
    }

    fn collect(&mut self, first_wait: Duration, heard: &mut Heard) -> Result<(), String> {
        self.socket.set_pause(Some(first_wait));
        let mut saw_message = false;
        loop {
            match self.socket.recv() {
                Ok(None) => break,
                Ok(Some(message)) => self.apply(message, heard)?,
                Err(error) if saw_message && error.contains("closed") => break,
                Err(error) => return Err(error),
            }
            saw_message = true;
            self.socket.set_pause(Some(Duration::from_millis(5)));
            if heard.pcm.len() + self.held.len() > RETURN_LIMIT * 2 {
                break;
            }
        }
        Ok(())
    }

    fn apply(&mut self, message: websocket::Message, heard: &mut Heard) -> Result<(), String> {
        if message.opcode == 0x1 {
            self.note_text(&String::from_utf8_lossy(&message.data));
            heard.text.push_str(&String::from_utf8_lossy(&message.data));
            return Ok(());
        }
        if message.opcode != 0x2 || message.data.is_empty() {
            return Ok(());
        }
        let kind = message.data[0];
        let payload = &message.data[1..];
        match kind {
            KIND_HANDSHAKE | KIND_CONTROL | KIND_META => Ok(()),
            KIND_AUDIO => match opus::decode(&mut self.decoder, payload) {
                Ok(pcm) => {
                    self.received_audio += 1;
                    self.held.extend(pcm);
                    Ok(())
                }
                Err(error) => {
                    self.decode_failures = self.decode_failures.saturating_add(1);
                    self.last_decode_error = Some(error);
                    Ok(())
                }
            },
            KIND_TEXT => {
                let text = String::from_utf8_lossy(payload);
                self.note_text(&text);
                heard.text.push_str(&text);
                Ok(())
            }
            KIND_ERROR => Err(format!("Speech server reported: {}", String::from_utf8_lossy(payload).chars().take(300).collect::<String>())),
            KIND_PING => {
                let mut reply = Vec::with_capacity(1 + payload.len().min(32));
                reply.push(KIND_PING);
                reply.extend(payload.iter().take(32));
                self.socket.send_binary(&reply)
            }
            _ => Ok(()),
        }
    }

    fn note_text(&mut self, text: &str) {
        if self.transcript.len() >= 32_768 {
            return;
        }
        let room = 32_768 - self.transcript.len();
        self.transcript.push_str(&text.chars().take(room).collect::<String>());
    }

    fn emit(&mut self, heard: &mut Heard) {
        if self.held.len() > HOLD_LIMIT {
            let extra = self.held.len() - HOLD_LIMIT;
            self.held.drain(..extra);
            self.dropped += extra;
        }
        let take = self.held.len().min(RETURN_LIMIT);
        heard.pcm.extend(self.held.drain(..take));
        heard.dropped = self.dropped;
    }
}

fn prompt_query(path: &str, voice: &str, text: &str) -> String {
    let mut out = String::with_capacity(path.len() + voice.len() + text.len() + 32);
    out.push_str(path);
    if !path.contains('?') {
        out.push('?');
    } else if !path.ends_with('?') && !path.ends_with('&') {
        out.push('&');
    }
    out.push_str("voice_prompt=");
    append_query_component(&mut out, voice);
    out.push_str("&text_prompt=");
    append_query_component(&mut out, text);
    out
}

fn append_query_component(out: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~') {
            out.push(*byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
}

fn split_host(authority: &str, default_port: u16) -> Result<(String, u16), String> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']').ok_or("Speech address has an invalid IPv6 host")?;
        let host = rest[..end].to_string();
        let tail = &rest[end + 1..];
        let port = if tail.is_empty() {
            default_port
        } else {
            tail.strip_prefix(':').ok_or("Speech address has an invalid port")?.parse::<u16>().map_err(|_| "Speech address has an invalid port")?
        };
        if host.is_empty() || port == 0 {
            return Err("Speech address has an invalid host or port".into());
        }
        return Ok((host, port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) {
            let port = port.parse::<u16>().map_err(|_| "Speech address has an invalid port")?;
            if host.is_empty() || port == 0 {
                return Err("Speech address has an invalid host or port".into());
            }
            return Ok((host.to_string(), port));
        }
    }
    if authority.is_empty() {
        return Err("Speech address has an invalid host or port".into());
    }
    Ok((authority.to_string(), default_port))
}

//! Client socket for a framed, masked, two-way byte stream over a connected transport.
//! Secure transports are supplied by the caller. This module frames messages only.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_FRAME: usize = 256 * 1024;
const MAX_HEADER: usize = 8 * 1024;

pub trait ByteStream: Read + Write + Send {
    fn set_pause(&self, timeout: Option<Duration>);
}

impl ByteStream for TcpStream {
    fn set_pause(&self, timeout: Option<Duration>) {
        self.set_read_timeout(timeout).ok();
        self.set_write_timeout(timeout).ok();
    }
}

pub struct Message {
    pub opcode: u8,
    pub data: Vec<u8>,
}

pub struct Socket {
    io: Box<dyn ByteStream>,
    buf: Vec<u8>,
    pos: usize,
    pending: Option<(u8, Vec<u8>)>,
}

impl Socket {
    pub fn open(io: Box<dyn ByteStream>, host: &str, port: u16, path: &str, headers: &[(&str, &str)]) -> Result<Self, String> {
        if host.is_empty() || host.len() > 253 || host.chars().any(|c| c.is_control() || c == ' ') {
            return Err("Speech socket host is not usable".into());
        }
        if !path.starts_with('/') || path.len() > 2048 || path.chars().any(|c| c.is_control() || c == ' ') {
            return Err("Speech socket path is not usable".into());
        }
        let host_header = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
        let key_raw = mask_key_bytes();
        let key = b64(&key_raw);
        let mut request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
        );
        for (name, value) in headers {
            if !header_name(name) || value.chars().any(|c| c.is_control()) {
                return Err("Speech socket header is not usable".into());
            }
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        let mut socket = Self { io, buf: Vec::new(), pos: 0, pending: None };
        socket.io.set_pause(Some(Duration::from_secs(10)));
        socket.io.write_all(request.as_bytes()).map_err(|error| format!("Speech socket handshake failed: {error}"))?;
        socket.io.flush().map_err(|error| format!("Speech socket handshake failed: {error}"))?;
        let header = socket.read_header()?;
        let expected = accept_key(&key);
        if !handshake_ok(&header, &expected) {
            return Err("Speech socket was not accepted".into());
        }
        Ok(socket)
    }

    pub fn set_pause(&self, timeout: Option<Duration>) {
        self.io.set_pause(timeout);
    }

    pub fn send_binary(&mut self, payload: &[u8]) -> Result<(), String> {
        self.send(0x2, payload)
    }

    pub fn recv(&mut self) -> Result<Option<Message>, String> {
        loop {
            let Some(frame) = self.read_frame(self.buf.len() == self.pos)? else {
                return Ok(None);
            };
            let fin = frame.fin;
            let opcode = frame.opcode;
            let data = frame.data;
            match opcode {
                0x8 => return Err("Speech socket closed".into()),
                0x9 => {
                    if data.len() <= 125 {
                        self.send(0xA, &data)?;
                    }
                }
                0xA => {}
                0x0 | 0x1 | 0x2 => {
                    if opcode == 0 {
                        let Some((_, buffer)) = self.pending.as_mut() else {
                            return Err("Speech socket continuation has no message".into());
                        };
                        if buffer.len().saturating_add(data.len()) > MAX_FRAME {
                            return Err("Speech socket message exceeds 256 KiB".into());
                        }
                        buffer.extend(data);
                        if fin {
                            let (kind, buffer) = self.pending.take().unwrap();
                            return Ok(Some(Message { opcode: kind, data: buffer }));
                        }
                    } else {
                        if self.pending.is_some() {
                            return Err("Speech socket started a message before the previous one finished".into());
                        }
                        if fin {
                            return Ok(Some(Message { opcode, data }));
                        }
                        self.pending = Some((opcode, data));
                    }
                }
                _ => return Err("Speech socket frame type is not supported".into()),
            }
        }
    }

    fn send(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        if payload.len() > MAX_FRAME {
            return Err("Speech socket message exceeds 256 KiB".into());
        }
        let mask = mask_key();
        let mut frame = Vec::with_capacity(14 + payload.len());
        frame.push(0x80 | opcode);
        let len = payload.len();
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(index, byte)| byte ^ mask[index % 4]));
        self.io.write_all(&frame).map_err(|error| format!("Speech socket write failed: {error}"))?;
        self.io.flush().map_err(|error| format!("Speech socket write failed: {error}"))?;
        Ok(())
    }

    fn read_header(&mut self) -> Result<String, String> {
        let mut raw = Vec::new();
        let mut tmp = [0u8; 1];
        while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
            if raw.len() >= MAX_HEADER {
                return Err("Speech socket handshake is too large".into());
            }
            match self.io.read(&mut tmp) {
                Ok(0) => return Err("Speech socket closed during the handshake".into()),
                Ok(_) => raw.push(tmp[0]),
                Err(error) => return Err(format!("Speech socket handshake failed: {error}")),
            }
        }
        String::from_utf8(raw).map_err(|_| "Speech socket handshake was not text".into())
    }

    fn read_frame(&mut self, first: bool) -> Result<Option<Frame>, String> {
        if !self.pull(2, first)? {
            return Ok(None);
        }
        let b0 = self.buf[self.pos];
        let b1 = self.buf[self.pos + 1];
        if b1 & 0x80 != 0 {
            return Err("Speech socket peer masked a frame".into());
        }
        let mut header = 2usize;
        let len0 = (b1 & 0x7f) as usize;
        let len = if len0 < 126 {
            len0
        } else if len0 == 126 {
            header = 4;
            if !self.pull(header, false)? {
                return Err("Speech socket frame length was cut off".into());
            }
            u16::from_be_bytes([self.buf[self.pos + 2], self.buf[self.pos + 3]]) as usize
        } else {
            header = 10;
            if !self.pull(header, false)? {
                return Err("Speech socket frame length was cut off".into());
            }
            let wide = u64::from_be_bytes(self.buf[self.pos + 2..self.pos + 10].try_into().unwrap());
            if wide > MAX_FRAME as u64 {
                return Err("Speech socket message exceeds 256 KiB".into());
            }
            wide as usize
        };
        if len > MAX_FRAME {
            return Err("Speech socket message exceeds 256 KiB".into());
        }
        if !self.pull(header + len, false)? {
            return Err("Speech socket frame was cut off".into());
        }
        let data = self.buf[self.pos + header..self.pos + header + len].to_vec();
        self.pos += header + len;
        if self.pos > 4096 && self.pos * 2 > self.buf.len() {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        Ok(Some(Frame { fin: b0 & 0x80 != 0, opcode: b0 & 0x0f, data }))
    }

    fn pull(&mut self, need: usize, first: bool) -> Result<bool, String> {
        while self.buf.len() - self.pos < need {
            let mut tmp = [0u8; 2048];
            let idle = self.buf.len() == self.pos;
            match self.io.read(&mut tmp) {
                Ok(0) => return Err("Speech socket closed".into()),
                Ok(count) => self.buf.extend_from_slice(&tmp[..count]),
                Err(error) if first && idle && timed_out(&error) => return Ok(false),
                Err(error) => return Err(format!("Speech socket read failed: {error}")),
            }
        }
        Ok(true)
    }
}

struct Frame {
    fin: bool,
    opcode: u8,
    data: Vec<u8>,
}

pub fn accept_key(client_key: &str) -> String {
    let mut material = client_key.as_bytes().to_vec();
    material.extend_from_slice(GUID.as_bytes());
    b64(&sha1(&material))
}

fn handshake_ok(header: &str, expected: &str) -> bool {
    let mut lines = header.split("\r\n");
    let Some(status) = lines.next() else { return false };
    if !status.starts_with("HTTP/1.1 101") && !status.starts_with("HTTP/1.0 101") {
        return false;
    }
    let mut accept = false;
    let mut upgrade = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "sec-websocket-accept" && value == expected {
            accept = true;
        }
        if name == "upgrade" && value.eq_ignore_ascii_case("websocket") {
            upgrade = true;
        }
    }
    accept && upgrade
}

fn header_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn timed_out(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock)
}

fn mask_key() -> [u8; 4] {
    let bytes = mask_key_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

fn mask_key_bytes() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    if fill_random(&mut bytes).is_err() {
        let tick = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        let n = MASK_SEQ.fetch_add(1, Ordering::Relaxed);
        let mixed = tick.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(n.wrapping_mul(0xBF58_476D_1CE4_E5B9));
        bytes[..8].copy_from_slice(&mixed.to_le_bytes());
        bytes[8..].copy_from_slice(&mixed.rotate_left(17).to_le_bytes());
    }
    if bytes.iter().all(|byte| *byte == 0) {
        bytes[0] = 1;
    }
    bytes
}

static MASK_SEQ: AtomicU64 = AtomicU64::new(1);

fn fill_random(bytes: &mut [u8]) -> Result<(), ()> {
    #[cfg(windows)]
    {
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn SystemFunction036(buffer: *mut u8, len: u32) -> u8;
        }
        if bytes.len() > u32::MAX as usize {
            return Err(());
        }
        if unsafe { SystemFunction036(bytes.as_mut_ptr(), bytes.len() as u32) } == 0 {
            return Err(());
        }
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::io::Read as _;
        let mut file = std::fs::File::open("/dev/urandom").map_err(|_| ())?;
        file.read_exact(bytes).map_err(|_| ())?;
        return Ok(());
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = bytes;
        Err(())
    }
}

pub fn b64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = ((chunk[0] as u32) << 16) | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8) | chunk.get(2).copied().unwrap_or(0) as u32;
        for index in 0..4 {
            if index > chunk.len() {
                out.push('=');
            } else {
                out.push(TABLE[((word >> (18 - 6 * index)) & 63) as usize] as char);
            }
        }
    }
    out
}

pub fn sha1(message: &[u8]) -> [u8; 20] {
    let mut state = [0x67452301u32, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (message.len() as u64).saturating_mul(8);
    let mut data = message.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    for block in data.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(chunk.try_into().unwrap());
        }
        for index in 16..80 {
            words[index] = (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16]).rotate_left(1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        for index in 0..80 {
            let (f, k) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let next = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(words[index]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn accept_matches_the_framing_example() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}

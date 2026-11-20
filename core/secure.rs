//! HTTPS for the Windows host through the operating system's own TLS stack
//! (Schannel), the counterpart of the platform SSLSocket the Android layer uses.
//! Windows validates the certificate chain and the host name; the core still
//! builds every request byte and validates every response byte.

use std::io::{self, Read, Write};
use std::net::TcpStream;

#[repr(C)]
#[derive(Clone, Copy)]
struct Handle {
    lower: usize,
    upper: usize,
}

#[repr(C)]
struct Buffer {
    size: u32,
    kind: u32,
    data: *mut u8,
}

#[repr(C)]
struct Buffers {
    version: u32,
    count: u32,
    items: *mut Buffer,
}

#[repr(C)]
struct Credentials {
    version: u32,
    certificates: u32,
    certificate_list: *mut u8,
    root_store: *mut u8,
    mappers: u32,
    mapper_list: *mut u8,
    algorithms: u32,
    algorithm_list: *mut u32,
    protocols: u32,
    minimum_strength: u32,
    maximum_strength: u32,
    session_lifespan: u32,
    flags: u32,
    format: u32,
}

#[repr(C)]
#[derive(Default)]
struct Sizes {
    header: u32,
    trailer: u32,
    message: u32,
    buffers: u32,
    block: u32,
}

#[link(name = "secur32")]
unsafe extern "system" {
    fn AcquireCredentialsHandleW(principal: *const u16, package: *const u16, usage: u32, logon: *mut u8, data: *mut Credentials, key: *mut u8, argument: *mut u8, credential: *mut Handle, expiry: *mut i64) -> i32;
    fn InitializeSecurityContextW(credential: *mut Handle, context: *mut Handle, target: *const u16, wanted: u32, reserved: u32, representation: u32, input: *mut Buffers, reserved_two: u32, created: *mut Handle, output: *mut Buffers, granted: *mut u32, expiry: *mut i64) -> i32;
    fn QueryContextAttributesW(context: *mut Handle, attribute: u32, value: *mut Sizes) -> i32;
    fn EncryptMessage(context: *mut Handle, quality: u32, message: *mut Buffers, sequence: u32) -> i32;
    fn DecryptMessage(context: *mut Handle, message: *mut Buffers, sequence: u32, quality: *mut u32) -> i32;
    fn FreeContextBuffer(buffer: *mut u8) -> i32;
    fn DeleteSecurityContext(context: *mut Handle) -> i32;
    fn FreeCredentialsHandle(credential: *mut Handle) -> i32;
}

const OK: i32 = 0;
const CONTINUE: i32 = 0x0009_0312;
const EXPIRED: i32 = 0x0009_0317;
const RENEGOTIATE: i32 = 0x0009_0321;
const INCOMPLETE_CREDENTIALS: i32 = 0x0009_0320;
const INCOMPLETE: i32 = 0x8009_0318_u32 as i32;

const EMPTY: u32 = 0;
const DATA: u32 = 1;
const TOKEN: u32 = 2;
const EXTRA: u32 = 5;
const TRAILER: u32 = 6;
const HEADER: u32 = 7;
const ALERT: u32 = 17;

/// Replay and sequence detection, confidentiality, a stream connection, and
/// output tokens allocated by the provider.
const WANTED: u32 = 0x4 | 0x8 | 0x10 | 0x100 | 0x8000;
/// The chain and the host name are validated by Windows during the handshake;
/// no default client certificate is offered; only strong ciphers are allowed.
const FLAGS: u32 = 0x10 | 0x20 | 0x0040_0000;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn explain(status: i32) -> String {
    match status as u32 {
        0x8009_0322 => "the server certificate does not belong to this host name".into(),
        0x8009_0325 => "the server certificate is not trusted by Windows".into(),
        0x8009_0328 => "the server certificate has expired".into(),
        0x8009_0327 => "the server certificate was rejected".into(),
        0x8009_0331 => "the server offered no TLS version or cipher Windows accepts".into(),
        0x8009_0326 => "the server's TLS handshake was malformed".into(),
        0x8009_0308 => "the server sent an invalid TLS token".into(),
        other => format!("TLS failed (0x{other:08X})"),
    }
}

/// An HTTPS connection over an open TCP stream.
pub struct Secure {
    stream: TcpStream,
    credential: Handle,
    context: Handle,
    sizes: Sizes,
    /// Bytes received that are not decrypted yet.
    sealed: Vec<u8>,
    /// Decrypted bytes not yet handed to the reader.
    plain: Vec<u8>,
    plain_at: usize,
    closed: bool,
}

impl Secure {
    /// Completes the TLS handshake for `host` over `stream`.
    pub fn open(stream: TcpStream, host: &str) -> Result<Secure, String> {
        let mut credential = Handle { lower: 0, upper: 0 };
        let mut data = Credentials {
            version: 4,
            certificates: 0,
            certificate_list: std::ptr::null_mut(),
            root_store: std::ptr::null_mut(),
            mappers: 0,
            mapper_list: std::ptr::null_mut(),
            algorithms: 0,
            algorithm_list: std::ptr::null_mut(),
            protocols: 0,
            minimum_strength: 0,
            maximum_strength: 0,
            session_lifespan: 0,
            flags: FLAGS,
            format: 0,
        };
        let package = wide("Microsoft Unified Security Protocol Provider");
        let mut expiry = 0i64;
        let status = unsafe {
            AcquireCredentialsHandleW(std::ptr::null(), package.as_ptr(), 2, std::ptr::null_mut(), &mut data, std::ptr::null_mut(), std::ptr::null_mut(), &mut credential, &mut expiry)
        };
        if status != OK {
            return Err(format!("Windows TLS is unavailable: {}", explain(status)));
        }
        let mut link = Secure { stream, credential, context: Handle { lower: 0, upper: 0 }, sizes: Sizes::default(), sealed: Vec::new(), plain: Vec::new(), plain_at: 0, closed: false };
        let target = wide(host);
        let mut started = false;
        let mut chunk = vec![0u8; 16384];
        loop {
            let mut input = [Buffer { size: link.sealed.len() as u32, kind: TOKEN, data: link.sealed.as_mut_ptr() }, Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() }];
            let mut inputs = Buffers { version: 0, count: 2, items: input.as_mut_ptr() };
            let mut output = [Buffer { size: 0, kind: TOKEN, data: std::ptr::null_mut() }, Buffer { size: 0, kind: ALERT, data: std::ptr::null_mut() }, Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() }];
            let mut outputs = Buffers { version: 0, count: 3, items: output.as_mut_ptr() };
            let mut granted = 0u32;
            let status = unsafe {
                if started {
                    InitializeSecurityContextW(&mut link.credential, &mut link.context, target.as_ptr(), WANTED, 0, 0x10, &mut inputs, 0, std::ptr::null_mut(), &mut outputs, &mut granted, &mut expiry)
                } else {
                    InitializeSecurityContextW(&mut link.credential, std::ptr::null_mut(), target.as_ptr(), WANTED, 0, 0x10, std::ptr::null_mut(), 0, &mut link.context, &mut outputs, &mut granted, &mut expiry)
                }
            };
            started = true;
            for item in &output {
                if item.kind == TOKEN && item.size > 0 && !item.data.is_null() {
                    let bytes = unsafe { std::slice::from_raw_parts(item.data, item.size as usize) };
                    let sent = link.stream.write_all(bytes);
                    unsafe { FreeContextBuffer(item.data); }
                    sent.map_err(|error| format!("{host}: the TLS handshake could not be sent: {error}"))?;
                } else if !item.data.is_null() {
                    unsafe { FreeContextBuffer(item.data); }
                }
            }
            match status {
                OK | CONTINUE => {
                    link.keep_extra(&input[1]);
                    if status == OK {
                        break;
                    }
                    if !link.sealed.is_empty() {
                        continue;
                    }
                }
                INCOMPLETE => {}
                INCOMPLETE_CREDENTIALS => return Err(format!("{host} asks for a client certificate, which RLLM does not offer")),
                other => return Err(format!("{host}: {}", explain(other))),
            }
            let read = link.stream.read(&mut chunk).map_err(|error| format!("{host}: the TLS handshake was interrupted: {error}"))?;
            if read == 0 {
                return Err(format!("{host} closed the connection during the TLS handshake"));
            }
            link.sealed.extend_from_slice(&chunk[..read]);
        }
        let status = unsafe { QueryContextAttributesW(&mut link.context, 4, &mut link.sizes) };
        if status != OK {
            return Err(format!("{host}: {}", explain(status)));
        }
        Ok(link)
    }

    /// The unread tail of the bytes just handed to the provider, if it left one.
    fn keep_extra(&mut self, item: &Buffer) {
        if item.kind == EXTRA && item.size > 0 && (item.size as usize) <= self.sealed.len() {
            let start = self.sealed.len() - item.size as usize;
            self.sealed.drain(..start);
        } else {
            self.sealed.clear();
        }
    }

    pub fn set_read_timeout(&self, wait: Option<std::time::Duration>) {
        self.stream.set_read_timeout(wait).ok();
    }

    /// Hands post-handshake messages (session tickets, key updates) back to the
    /// provider and sends whatever it answers.
    fn renegotiate(&mut self) -> io::Result<()> {
        let mut input = [Buffer { size: self.sealed.len() as u32, kind: TOKEN, data: self.sealed.as_mut_ptr() }, Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() }];
        let mut inputs = Buffers { version: 0, count: 2, items: input.as_mut_ptr() };
        let mut output = [Buffer { size: 0, kind: TOKEN, data: std::ptr::null_mut() }, Buffer { size: 0, kind: ALERT, data: std::ptr::null_mut() }, Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() }];
        let mut outputs = Buffers { version: 0, count: 3, items: output.as_mut_ptr() };
        let (mut granted, mut expiry) = (0u32, 0i64);
        let status = unsafe { InitializeSecurityContextW(&mut self.credential, &mut self.context, std::ptr::null(), WANTED, 0, 0x10, &mut inputs, 0, std::ptr::null_mut(), &mut outputs, &mut granted, &mut expiry) };
        for item in &output {
            if item.kind == TOKEN && item.size > 0 && !item.data.is_null() {
                let bytes = unsafe { std::slice::from_raw_parts(item.data, item.size as usize) }.to_vec();
                unsafe { FreeContextBuffer(item.data); }
                self.stream.write_all(&bytes)?;
            } else if !item.data.is_null() {
                unsafe { FreeContextBuffer(item.data); }
            }
        }
        match status {
            OK | CONTINUE => {
                self.keep_extra(&input[1]);
                Ok(())
            }
            INCOMPLETE => Ok(()),
            other => Err(io::Error::other(explain(other))),
        }
    }

    /// Decrypts one record from `sealed`. Returns false when more bytes are needed.
    fn open_record(&mut self) -> io::Result<bool> {
        if self.sealed.is_empty() {
            return Ok(false);
        }
        let mut items = [
            Buffer { size: self.sealed.len() as u32, kind: DATA, data: self.sealed.as_mut_ptr() },
            Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() },
            Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() },
            Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() },
        ];
        let mut message = Buffers { version: 0, count: 4, items: items.as_mut_ptr() };
        let status = unsafe { DecryptMessage(&mut self.context, &mut message, 0, std::ptr::null_mut()) };
        match status {
            INCOMPLETE => return Ok(false),
            OK | RENEGOTIATE | EXPIRED => {}
            other => return Err(io::Error::other(explain(other))),
        }
        let mut extra = 0usize;
        for item in &items {
            // A closing alert leaves its own record in the data buffer; it is not
            // reply text.
            if item.kind == DATA && item.size > 0 && !item.data.is_null() && status != EXPIRED {
                let bytes = unsafe { std::slice::from_raw_parts(item.data, item.size as usize) };
                if self.plain_at >= self.plain.len() {
                    self.plain.clear();
                    self.plain_at = 0;
                }
                self.plain.extend_from_slice(bytes);
            } else if item.kind == EXTRA {
                extra = item.size as usize;
            }
        }
        let start = self.sealed.len().saturating_sub(extra);
        self.sealed.drain(..start);
        match status {
            EXPIRED => {
                self.closed = true;
                self.sealed.clear();
            }
            RENEGOTIATE => self.renegotiate()?,
            _ => {}
        }
        Ok(true)
    }
}

impl Read for Secure {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.plain_at < self.plain.len() {
                let count = out.len().min(self.plain.len() - self.plain_at);
                out[..count].copy_from_slice(&self.plain[self.plain_at..self.plain_at + count]);
                self.plain_at += count;
                return Ok(count);
            }
            if self.closed {
                return Ok(0);
            }
            if self.open_record()? {
                continue;
            }
            let mut chunk = [0u8; 16384];
            let read = self.stream.read(&mut chunk)?;
            if read == 0 {
                self.closed = true;
                return Ok(0);
            }
            self.sealed.extend_from_slice(&chunk[..read]);
        }
    }
}

impl Write for Secure {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let count = data.len().min(self.sizes.message.max(1) as usize);
        let (header, trailer) = (self.sizes.header as usize, self.sizes.trailer as usize);
        let mut record = vec![0u8; header + count + trailer];
        record[header..header + count].copy_from_slice(&data[..count]);
        let base = record.as_mut_ptr();
        let mut items = [
            Buffer { size: header as u32, kind: HEADER, data: base },
            Buffer { size: count as u32, kind: DATA, data: unsafe { base.add(header) } },
            Buffer { size: trailer as u32, kind: TRAILER, data: unsafe { base.add(header + count) } },
            Buffer { size: 0, kind: EMPTY, data: std::ptr::null_mut() },
        ];
        let mut message = Buffers { version: 0, count: 4, items: items.as_mut_ptr() };
        let status = unsafe { EncryptMessage(&mut self.context, 0, &mut message, 0) };
        if status != OK {
            return Err(io::Error::other(explain(status)));
        }
        let length = (items[0].size + items[1].size + items[2].size) as usize;
        self.stream.write_all(&record[..length])?;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl Drop for Secure {
    fn drop(&mut self) {
        unsafe {
            DeleteSecurityContext(&mut self.context);
            FreeCredentialsHandle(&mut self.credential);
        }
    }
}

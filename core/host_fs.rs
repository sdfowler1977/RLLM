//! Filesystem identity and directory-relative opens for every host.
//! Linux and Android keep the operating-system traits and `openat`.
//! Windows uses the file index, the volume serial, and reparse-aware opens.
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(unix)]
pub use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

#[cfg(windows)]
mod win {
    use super::*;
    use std::os::windows::fs::MetadataExt as WinMeta;
    use std::os::windows::fs::OpenOptionsExt as WinOpen;

    fn filetime(ticks: u64) -> (i64, i64) {
        const TICKS: u64 = 10_000_000;
        const UNIX_EPOCH: u64 = 11_644_473_600;
        let seconds = ticks / TICKS;
        let unix = seconds.saturating_sub(UNIX_EPOCH) as i64;
        let nanos = ((ticks % TICKS) * 100) as i64;
        (unix, nanos)
    }

    pub trait MetadataExt {
        fn dev(&self) -> u64;
        fn ino(&self) -> u64;
        fn mode(&self) -> u32;
        fn mtime(&self) -> i64;
        fn mtime_nsec(&self) -> i64;
        fn ctime(&self) -> i64;
        fn ctime_nsec(&self) -> i64;
    }

    impl MetadataExt for std::fs::Metadata {
        fn dev(&self) -> u64 {
            // The stable metadata record has no volume serial. One host
            // shares one identity space; creation time separates objects.
            1
        }
        fn ino(&self) -> u64 {
            WinMeta::creation_time(self)
        }
        fn mode(&self) -> u32 {
            if self.is_dir() { 0o700 } else { 0o600 }
        }
        fn mtime(&self) -> i64 {
            filetime(WinMeta::last_write_time(self)).0
        }
        fn mtime_nsec(&self) -> i64 {
            filetime(WinMeta::last_write_time(self)).1
        }
        fn ctime(&self) -> i64 {
            filetime(WinMeta::creation_time(self)).0
        }
        fn ctime_nsec(&self) -> i64 {
            filetime(WinMeta::creation_time(self)).1
        }
    }

    pub trait OpenOptionsExt {
        fn mode(&mut self, mode: u32) -> &mut Self;
        fn custom_flags(&mut self, flags: i32) -> &mut Self;
    }

    impl OpenOptionsExt for OpenOptions {
        fn mode(&mut self, _mode: u32) -> &mut Self {
            self
        }
        fn custom_flags(&mut self, flags: i32) -> &mut Self {
            // Callers pass the Linux flag bits. Map the two that change what
            // is opened: do not follow a reparse point, and allow a directory.
            let mut attributes = 0u32;
            if flags & 0x20000 != 0 {
                attributes |= 0x0020_0000;
            }
            if flags & 0x10000 != 0 {
                attributes |= 0x0200_0000;
            }
            if attributes != 0 {
                WinOpen::custom_flags(self, attributes);
            }
            self
        }
    }
}

#[cfg(windows)]
pub use win::{MetadataExt, OpenOptionsExt};

#[cfg(windows)]
pub fn handle_path(file: &File) -> io::Result<std::path::PathBuf> {
    use std::os::windows::io::AsRawHandle;
    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(handle: *mut std::ffi::c_void, buf: *mut u16, len: u32, flags: u32) -> u32;
    }
    let handle = file.as_raw_handle();
    let mut buf = vec![0u16; 512];
    loop {
        let written = unsafe { GetFinalPathNameByHandleW(handle, buf.as_mut_ptr(), buf.len() as u32, 0) };
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        if (written as usize) < buf.len() {
            buf.truncate(written as usize);
            break;
        }
        buf.resize(written as usize + 1, 0);
    }
    let text = String::from_utf16(&buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path is not Unicode"))?;
    Ok(std::path::PathBuf::from(text))
}

/// Open one path segment under an already opened directory.
/// `flags` uses the same Linux bits the callers already pass.
pub fn open_at(parent: &File, name: &[u8], flags: i32) -> io::Result<File> {
    if name.is_empty() || name.contains(&0) || name.contains(&b'/') || name.contains(&b'\\') || name == b"." || name == b".." {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path segment"));
    }
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        let c_name = std::ffi::CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path segment"))?;
        unsafe extern "C" {
            fn openat(fd: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
        }
        let fd = unsafe { openat(parent.as_raw_fd(), c_name.as_ptr(), flags, 0o600u32) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as WinOpen;
        let text = std::str::from_utf8(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path segment is not Unicode"))?;
        let path = handle_path(parent)?.join(text);
        let mut options = OpenOptions::new();
        match flags & 3 {
            1 => {
                options.write(true);
            }
            2 => {
                options.read(true).write(true);
            }
            _ => {
                options.read(true);
            }
        }
        if flags & 0x80 != 0 {
            options.create_new(true);
        } else if flags & 0x40 != 0 {
            options.create(true);
        }
        let mut attributes = 0x0200_0000u32;
        if flags & 0x20000 != 0 {
            attributes |= 0x0020_0000;
        }
        WinOpen::custom_flags(&mut options, attributes);
        let _ = Path::new(&path);
        options.open(path)
    }
}
